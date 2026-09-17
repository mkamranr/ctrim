<div align="center">

# ctrim

**Strip the noise out of terminal output before it reaches your AI coding agent.**

[![CI](https://github.com/mkamranr/ctrim/actions/workflows/ci.yml/badge.svg)](https://github.com/mkamranr/ctrim/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/mkamranr/ctrim?color=blue)](https://github.com/mkamranr/ctrim/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-1.86%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Binary size](https://img.shields.io/badge/binary-1.8MB-green)](https://github.com/mkamranr/ctrim/releases)

[Install](#install) · [What it does](#what-it-actually-does) · [Benchmarks](#measured-reduction) · [Usage](#usage) · [Docs](docs/) · [Contributing](CONTRIBUTING.md)

</div>

---

Test runs, build logs and diffs are mostly repetition: retry loops, vendor stack
frames, untouched diff context, lockfile churn, ANSI colour codes. Your model
pays for every token of it, and the signal gets buried.

`ctrim` sits in the pipe and removes the noise — nothing else.

```console
$ docker logs api | ctrim
[ctrim] docker-log: ~7,531 -> ~487 tokens (-93.5%) | 174 -> 13 lines
```

```diff
- api_1 | 10:00:04 level=warn msg="connection to redis refused, retrying" attempt=1
- api_1 | 10:00:05 level=warn msg="connection to redis refused, retrying" attempt=2
- ... 118 more retry lines ...
+ [Repeated 120 times: "api_1 | level=warn msg=\"connection to redis refused, retrying\""]
  api_1 | 10:02:10 level=error msg="redis unavailable after 120 attempts"
```

Everything that survives is **byte-identical to the input**. Nothing is
summarised, reworded, or sent anywhere.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/mkamranr/ctrim/main/install.sh | sh
```

<details>
<summary><b>Other ways to install</b></summary>

<br>

```bash
cargo install ctrim                    # from crates.io
brew install mkamranr/tap/ctrim        # Homebrew (macOS and Linux)
docker run --rm -i ghcr.io/mkamranr/ctrim < build.log
```

Prebuilt binaries for macOS (Intel and Apple Silicon), Linux (gnu and musl,
x86-64 and arm64) and Windows are attached to every
[release](https://github.com/mkamranr/ctrim/releases). Building from source
needs Rust 1.86 or newer.

</details>

<details>
<summary><b>Shell aliases worth having</b></summary>

<br>

```bash
alias t='ctrim --clip'                       # anything | t  -> clipboard, ready to paste
alias tdiff='git diff | ctrim --format xml'
alias tpy='pytest 2>&1 | ctrim --preset pytest --clip'
```

</details>

## What it actually does

| Noise | What `ctrim` does |
| :--- | :--- |
| ANSI colour codes, progress-bar redraws | Removed. Only the final frame of a `\r` sequence survives |
| Repeated log lines (`retrying...` ×120) | Folded into `[Repeated 120 times: "..."]`, including lines that differ only by timestamp, id or counter |
| Vendor stack frames (`site-packages`, `node_modules`, `/rustc/`) | Folded into `[... 9 vendor frames omitted ...]`. Your frames and the exception always survive |
| Untouched diff context | Trimmed to one line either side of each change, with `@@` headers recomputed so the patch still applies |
| Lockfiles and generated files in a diff | Replaced by `[skipped generated file Cargo.lock: +412/-87 lines]` |
| `index 89abc12..def3456` blob hashes | Removed |

Full rules, and the reasoning behind each one, are in
**[docs/heuristics.md](docs/heuristics.md)**.

## Measured reduction

Exact `o200k_base` token counts over the fixtures in this repository.
`cargo test --test reduction_test` enforces these as floors, so the table cannot
quietly rot:

| Input | Before | After | Reduction |
| :--- | ---: | ---: | ---: |
| `docker logs` with a retry storm | 7,174 | 477 | **−93%** |
| `git diff` touching a lockfile | 628 | 173 | **−72%** |
| `cargo check` with colour | 1,409 | 521 | **−63%** |
| Jest failure with a deep trace | 430 | 259 | **−40%** |
| pytest run, 2 failures | 1,520 | 999 | **−34%** |

Log-shaped input compresses hardest, because logs repeat. Output that is already
terse — a short pytest failure — compresses least, which is the honest answer:
there was not much to remove.

## Usage

```console
$ ctrim [OPTIONS] [FILE]
$ <command> | ctrim [OPTIONS]
```

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
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

The summary goes to **STDERR**, so it never contaminates the piped output:

```console
$ pytest 2>&1 | ctrim | pbcopy
[ctrim] pytest: ~1,646 -> ~1,145 tokens (-30.4%) | 94 -> 68 lines
```

`--format xml` wraps the result in `<context type="pytest">` with the failing
assertions repeated in `<error_summary>` — the layout models parse most
reliably.

More recipes — CI integration, agent wiring, per-project tuning — in
**[docs/usage.md](docs/usage.md)**.

## How it works

```
input ──> detector ──> processor chain ──> formatter ──> output
           │            │                   │
           │            ├─ ansi             ├─ markdown
           │            ├─ dedupe           ├─ xml
           │            ├─ stacktrace       └─ raw
           │            └─ diff
           └─ scores the first 200 lines
```

Every processor is a streaming state machine over lines, so memory is bounded by
window sizes rather than input size:

| Metric | Measured |
| :--- | :--- |
| Throughput | ~50 MB/s |
| Memory, 100MB input | ~6 MB RSS |
| Memory, 1GB input | ~6 MB RSS |
| Binary size | 1.8 MB |

Piping a multi-gigabyte log through `ctrim` is fine. The internals are written
up in **[docs/architecture.md](docs/architecture.md)**.

### Token counting

Counts come from a dependency-free estimator fitted against `o200k_base` —
within 15% on the fixture corpus, and shown with a `~` so the number never
overstates itself. For real BPE counts:

```bash
cargo install ctrim --features exact-tokens
```

Several vendors keep their tokenizer private, so even "exact" counts approximate
those models; they are exact for OpenAI `o200k` models.

## Using it as a library

```rust
use ctrim::{process, Config, OutputFormat};

let out = process(&raw_log, &Config {
    format: OutputFormat::Xml,
    ..Config::default()
});

println!("{} -> {} tokens", out.stats.raw_tokens, out.stats.reduced_tokens);
println!("{}", out.content);
```

`ctrim::pipeline::run` takes any `BufRead` and `Write` and never buffers the
whole input. See **[docs/library.md](docs/library.md)**.

## Contributing

The most useful contribution is **output we handle badly**. Open an issue with a
paste of the noisy log and what should have survived — there is an
[issue template](.github/ISSUE_TEMPLATE/noisy-output.yml) for exactly that.

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to add a processor or a format,
and [docs/architecture.md](docs/architecture.md) for how the pieces fit.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Contributions are accepted under the same terms.
