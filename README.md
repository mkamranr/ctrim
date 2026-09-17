# ctrim

**Strip the noise out of terminal output before it reaches your AI coding agent.**

Test runs, build logs and diffs are mostly repetition: retry loops, vendor stack
frames, untouched diff context, lockfile churn, ANSI colour codes. Your model
pays for every token of it, and the signal gets buried.

`ctrim` sits in the pipe and removes the noise — nothing else.

```bash
pytest 2>&1 | ctrim --clip     # failure analysis, minus the framework internals
git diff | ctrim --format xml  # the changed lines, minus lockfiles and blob hashes
docker logs api | ctrim        # the one error, minus 500 retry lines
```

```
[ctrim] docker-log: ~7,531 -> ~487 tokens (-93.5%) | 174 -> 13 lines
```

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/mkamranr/ctrim/main/install.sh | sh
```

<details>
<summary>Other ways</summary>

```bash
cargo install ctrim                        # from crates.io
brew install mkamranr/tap/ctrim        # Homebrew
docker run --rm -i ghcr.io/mkamranr/ctrim < build.log
```

Prebuilt binaries for macOS, Linux (gnu and musl) and Windows are on the
[releases page](https://github.com/mkamranr/ctrim/releases).
</details>

## What it actually does

| Noise | What `ctrim` does |
| --- | --- |
| ANSI colour codes, progress-bar redraws | removed; only the final frame of a `\r` sequence survives |
| Repeated log lines (`retrying...` ×120) | folded into `[Repeated 120 times: "..."]`, including lines that differ only by timestamp, id or counter |
| Vendor stack frames (`site-packages`, `node_modules`, `/rustc/`) | folded into `[... 9 vendor frames omitted ...]`; your frames and the exception always survive |
| Untouched diff context | trimmed to one line either side of each change, with `@@` headers recomputed so the patch stays valid |
| Lockfiles and generated files in a diff | replaced by `[skipped generated file Cargo.lock: +412/-87 lines]` |
| `index 89abc12..def3456` blob hashes | removed |

Nothing is summarised, paraphrased or sent anywhere. It is a filter: text in,
less text out, and every line that survives is byte-identical to the input.

## Measured reduction

Exact `o200k_base` token counts over the fixtures in this repository
(`cargo test --test reduction_test` enforces these as floors):

| Input | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| `docker logs` with a retry storm | 7,174 | 477 | **−93%** |
| `git diff` touching a lockfile | 628 | 173 | **−72%** |
| `cargo check` with colour | 1,409 | 521 | **−63%** |
| Jest failure with a deep trace | 430 | 259 | **−40%** |
| pytest run, 2 failures | 1,520 | 999 | **−34%** |

Log-shaped input compresses hardest, because logs repeat. Output that is
already terse — a short pytest failure — compresses least, which is the honest
answer: there was not much to remove.

## Usage

```
ctrim [OPTIONS] [FILE]
<command> | ctrim [OPTIONS]
```

| Flag | Short | Default | Description |
| --- | --- | --- | --- |
| `--format` | `-f` | `markdown` | Output layout: `markdown`, `xml`, `raw` |
| `--preset` | `-p` | `auto` | Parser: `auto`, `pytest`, `cargo`, `diff`, `docker`, `json`, `generic` |
| `--clip` | `-c` | off | Copy the result to the system clipboard |
| `--out` | `-o` | — | Write to a file instead of STDOUT |
| `--keep-frames` | `-k` | `3` | Vendor frames kept per folded run |
| `--context-lines` | | `1` | Diff context lines kept either side of a change |
| `--dedupe-window` | | `64` | Lines the repeated-line detector remembers |
| `--dedupe-only` | | off | Run only the repeated-line compressor |
| `--exact-dupes` | | off | Fold only byte-identical lines |
| `--preserve-ansi` | | off | Keep colour escape sequences |
| `--quiet` | `-q` | off | Suppress the summary on STDERR |

The summary goes to STDERR, so it never contaminates the piped output:

```bash
pytest 2>&1 | ctrim | pbcopy       # STDOUT is clean
[ctrim] pytest: ~1,646 -> ~1,145 tokens (-30.4%) | 94 -> 68 lines
```

`--format xml` wraps the result in `<context type="pytest">` with the failing
assertions repeated in `<error_summary>` — the layout models parse most
reliably.

## How it works

```
input ─> detector ─> processor chain ─> formatter ─> output
```

The detector scores the first 200 lines against per-format signatures. Each
processor is a streaming state machine over lines, so memory is bounded by the
window sizes rather than the input: **100MB of logs uses about 6MB of RSS**, at
roughly 50MB/s on a 2019 laptop. Piping a multi-gigabyte log through `ctrim` is
fine.

Token counts are estimated by a dependency-free model fitted against
`o200k_base` (within 15% on the fixture corpus, shown with a `~`). Build with
`--features exact-tokens` for real BPE counts:

```bash
cargo install ctrim --features exact-tokens
```

Several vendors keep their tokenizer private, so even "exact" counts are an
approximation for those models; they are exact for OpenAI `o200k` models.

## Using it as a library

```rust
use ctrim::{process, Config, OutputFormat};

let out = process(&raw_log, &Config { format: OutputFormat::Xml, ..Config::default() });
println!("{} -> {} tokens", out.stats.raw_tokens, out.stats.reduced_tokens);
```

`ctrim::pipeline::run` takes any `BufRead` and `Write` and never buffers the
whole input.

## Contributing

The most useful contribution is **output we handle badly**. Open an issue with
a paste of the noisy log and what should have survived — there is an issue
template for exactly that. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to
add a processor or a format.

## License

MIT OR Apache-2.0, at your option.
