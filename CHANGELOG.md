# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned for 0.2

* `--max-tokens` / `--target-reduction`: apply progressively more aggressive
  folding until the output fits a token budget.
* JSON and structured-payload pruning: truncate long arrays inside JSON log
  lines while keeping the key schema.
* A config file for vendor paths and generated-file globs, so a project can
  teach `ctrim` about its own layout.
* Java, Go and Ruby stack-trace dialects.
* `--stats-json` for CI dashboards.

## [0.1.0] - 2026-09-17

First tagged release. Binaries for macOS, Linux and Windows are attached to the
GitHub release; the crates.io publish and the Homebrew tap update run only once
their tokens are configured.

### Added

* Streaming line pipeline: detect, process, render. Bounded memory regardless of
  input size (~6MB RSS on a 100MB log, ~50MB/s).
* Format auto-detection for git diffs, pytest, cargo/rustc, docker logs, JSON
  lines and generic logs, with `--preset` to override.
* ANSI escape stripping, including carriage-return progress frames and
  backspaces.
* Repeated-line compression, with optional folding of lines that differ only by
  number or id (`--exact-dupes` to disable), an adjacent-run collapse and a
  sliding window for interleaved repeats.
* Stack-trace folding for Python, pytest long form, JavaScript and Rust
  backtraces, keeping user frames and the exception while folding vendor runs.
* Unified-diff minimization: context trimming with recomputed `@@` headers,
  generated-file summaries, binary markers, `index` line removal.
* Output as Markdown, XML (`<context>` / `<error_summary>`) or raw, to STDOUT,
  a file (`--out`) or the system clipboard (`--clip`).
* Reduction summary on STDERR, kept out of the piped output.
* Token estimation with no dependencies, fitted against `o200k_base` to within
  15%; exact BPE counts behind the `exact-tokens` feature.

### Known limitations

* The design target of 100MB in under 200ms is not met: the current pipeline
  does 100MB in about 2 seconds. Memory, at ~6MB, is well inside the 20MB
  target.
* Token counts are approximate for any model whose tokenizer is not public.
