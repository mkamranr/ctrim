# Architecture

```
                   ┌──────────────────────────────────────────┐
  STDIN / file ──> │ read_line (lossy UTF-8, bounded buffer)   │
                   └──────────────────┬───────────────────────┘
                                      v
                   ┌──────────────────────────────────────────┐
                   │ head buffer (200 lines) ──> detector      │
                   └──────────────────┬───────────────────────┘
                                      v
                   ┌──────────────────────────────────────────┐
                   │ Chain: ansi ─> stacktrace ─> dedupe       │  (per format)
                   │        ansi ─> diff                       │
                   └──────────────────┬───────────────────────┘
                                      v
                   ┌──────────────────────────────────────────┐
                   │ CountingSink ──> Formatter ──> writer     │
                   └──────────────────┬───────────────────────┘
                                      v
                        STDOUT / file / clipboard
                        STDERR: reduction summary
```

Source map:

| File | Role |
| :--- | :--- |
| [`src/pipeline.rs`](../src/pipeline.rs) | The driver: read, detect, build the chain, render, count |
| [`src/detector.rs`](../src/detector.rs) | Weighted signature scoring over the head buffer |
| [`src/processors/mod.rs`](../src/processors/mod.rs) | `LineProcessor`, `LineSink`, `VecSink`, `Chain` |
| [`src/processors/*.rs`](../src/processors) | ansi, dedupe, stacktrace, diff |
| [`src/formatters/*.rs`](../src/formatters) | markdown, xml, raw |
| [`src/token/*.rs`](../src/token) | Estimator, optional exact BPE, streaming `Counter` |
| [`src/utils/*.rs`](../src/utils) | Clipboard, reduction stats |

---

## The core trait

```rust
pub trait LineProcessor {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink);
    fn finish(&mut self, out: &mut dyn LineSink) {}
    fn name(&self) -> &'static str;
    fn summary(&self) -> Vec<String> { Vec::new() }
}
```

A processor may emit zero, one or many lines per input line. State held across
lines — an open dedupe run, a half-parsed trace, a buffered hunk — must be
released in `finish`, which the chain calls once at end of input.

`summary` is how the stack-trace folder surfaces the exception line into
`<error_summary>` without the formatter knowing anything about tracebacks.

### Why not the whole string?

The specification this project started from proposed
`fn process(&mut self, input: &str) -> ProcessedOutput`. That signature forces
the entire log into memory, which contradicts the memory target in the same
document. The line-streaming trait is the internal contract; the whole-string
version survives as [`ctrim::process`](library.md), built on top of it.

---

## The chain

`Chain::feed` walks processors by index, sending each stage's output into the
next:

```rust
fn feed<S: AsRef<str>>(&mut self, stage: usize, lines: &[S], out: &mut dyn LineSink) {
    if stage >= self.procs.len() { /* emit and return */ }
    let mut sink = std::mem::take(&mut self.scratch[stage]);
    sink.reset();
    for line in lines { self.procs[stage].on_line(line.as_ref(), &mut sink); }
    self.feed(stage + 1, sink.as_slice(), out);
    self.scratch[stage] = sink;
}
```

Two details worth knowing before you change it:

1. **`mem::take` on the scratch buffer.** Taking the stage's buffer out of
   `self` lets it be borrowed immutably while the chain is borrowed mutably for
   the recursive call. It is put back afterwards, so the allocation is reused on
   every line.
2. **`finish` output flows downstream.** `Chain::finish` flushes stage `i`, then
   feeds whatever came out through stages `i+1..n`. A trace flushed by the
   stack folder at EOF still passes through the deduplicator.

`VecSink` reuses its `String` allocations across resets, so the steady state
allocates nothing per line.

---

## Memory

Bounded by construction, not by convention:

| Holder | Bound |
| :--- | :--- |
| Head buffer | 200 lines, once, for detection |
| Read buffer | One line |
| Dedupe window | `--dedupe-window` entries (default 64) |
| Dedupe run | One line |
| Stack folder | One trace block |
| Diff minimizer | One hunk |
| Chain scratch | One buffer per stage, reused |

Measured: **~6 MB RSS on a 100 MB input**, and the same on a 1 GB input.

---

## Throughput

~50 MB/s, measured with `scripts/bench-large.sh 100` on a 2019 Intel laptop.
The original 200 ms target for 100 MB (≈500 MB/s, near memcpy speed) is not
reachable with per-line semantic parsing; the honest number is in
[CHANGELOG.md](../CHANGELOG.md).

Where the time goes on a trace-heavy 100 MB log:

| Stage | Cost |
| :--- | :--- |
| Read + token counting + write | ~1.0 s |
| ANSI stripping | ~0.2 s |
| Repeated-line folding | ~0.3 s |
| Stack-trace folding | ~0.9 s |

Four decisions carry most of that performance, and reversing any of them costs
several-fold:

- **Byte loops, not char loops.** Token estimation and dedupe keys walk
  `as_bytes()`. Every classification involved is an ASCII test, and a UTF-8
  continuation byte can never be mistaken for one.
- **Dedupe keys are `Vec<u8>`, compared by hash first.** A full window costs 64
  integer comparisons. An earlier version ran Levenshtein against every window
  entry and took 15 seconds for the same input.
- **Regexes sit behind byte guards.** `could_be_js_frame` and friends reject
  ordinary log lines with a `starts_with` before the regex engine is touched.
- **`Cow` from the ANSI stripper.** A line with no escape character is borrowed,
  not copied.

Benchmarks:

```bash
cargo bench                  # criterion, per-size throughput
scripts/bench-large.sh 100   # 100MB end to end, wall time and peak RSS
```

---

## Adding a processor

See [CONTRIBUTING.md](../CONTRIBUTING.md#adding-a-processor). The three rules
are: bounded memory, byte-identical survivors, flush in `finish`.
