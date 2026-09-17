# Contributing to ctrim

## The most valuable contribution

Output we compress badly. Open an issue with the raw text and the lines that
should have survived; the "noisy output" issue template asks for exactly that.
Every fixture in `tests/fixtures/` came from real output, and that is what keeps
the heuristics honest.

## Getting set up

```bash
git clone https://github.com/mkamranr/ctrim
cd ctrim
cargo test
```

Before opening a PR:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## Adding a processor

A processor is a streaming state machine over lines. Implement
[`LineProcessor`](src/processors/mod.rs):

```rust
impl LineProcessor for MyProcessor {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) { /* emit 0..n lines */ }
    fn finish(&mut self, out: &mut dyn LineSink) { /* flush held state */ }
    fn name(&self) -> &'static str { "my-processor" }
}
```

Three rules, because they are what make `ctrim` safe to put in a pipe:

1. **Bounded memory.** Hold a fixed number of lines, never the whole input.
2. **Byte-identical survivors.** Lines that pass through are never reworded,
   only dropped or replaced by an explicit `[...]` marker.
3. **Flush in `finish`.** Anything held when input ends must still be emitted.

Then wire it into `build_chain` in [`src/pipeline.rs`](src/pipeline.rs).

## Adding a format

1. Add the variant to `FormatType` in [`src/detector.rs`](src/detector.rs) with
   its `label` and `fence_language`.
2. Add signature regexes to `SIGNATURES` with weights. A signature should be
   something the format prints and nothing else does.
3. Give it a chain in `build_chain`.
4. Add a `--preset` value in [`src/main.rs`](src/main.rs).

## Tests

| Kind | Where | What it protects |
| --- | --- | --- |
| Unit | next to each module | state-machine edges: traces at EOF, hunk arithmetic, nested ANSI |
| Snapshot | `tests/snapshot_tests.rs` | exact output per fixture and format |
| Reduction floors | `tests/reduction_test.rs` | the numbers published in the README |
| CLI | `tests/integration_tests.rs` | flags, exit codes, STDIN/STDOUT/STDERR split |

Adding a fixture means adding it to `FIXTURES` in the snapshot test and to
`BANDS` in the reduction test. Review snapshot changes with `cargo insta review`
(or accept them with `INSTA_UPDATE=always cargo test`), and read the diff — a
snapshot change is the tool's behaviour changing.

If you tune the token estimator, refit it rather than guessing:

```bash
cargo run --release --example calibrate --features exact-tokens -- tests/fixtures/*.log src/*.rs
```

## Performance

`ctrim` is meant to be usable on a multi-gigabyte log. Two checks:

```bash
cargo bench                  # per-processor throughput
scripts/bench-large.sh 100   # 100MB end to end, wall time and peak RSS
```

Current numbers: ~50MB/s, ~6MB RSS regardless of input size. A change that
allocates per line will show up immediately in both.

## Commits and PRs

Conventional commit subjects (`feat:`, `fix:`, `perf:`, `docs:`, `test:`).
Keep a PR to one concern, and say in the description what output changed and
why it is safe for a model to lose it.

## Code of conduct

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
