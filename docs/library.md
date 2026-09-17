# Using ctrim as a library

```toml
[dependencies]
ctrim = "0.1"
```

The binary is a thin wrapper over the crate; anything the CLI does is available
programmatically.

## Whole-string API

```rust
use ctrim::{process, Config, OutputFormat};

let out = process(&raw_log, &Config {
    format: OutputFormat::Xml,
    keep_frames: 1,
    ..Config::default()
});

println!("{}", out.content);
println!("{} -> {} tokens ({:.1}%)",
    out.stats.raw_tokens,
    out.stats.reduced_tokens,
    out.stats.reduction_percentage());
```

`ProcessedOutput` carries the rendered `content`, line counts, the
`detected_type`, the `error_summary` lines lifted out of any stack trace, and
`stats`.

This API holds the whole input and the whole output in memory, by definition.

## Streaming API

```rust
use std::io::{BufReader, stdin, stdout};
use ctrim::{pipeline, Config};

let outcome = pipeline::run(
    BufReader::new(stdin()),
    stdout().lock(),
    &Config::default(),
)?;

eprintln!("{}", outcome.stats.report(outcome.detected_type, false));
```

`pipeline::run` accepts any `BufRead` and any `Write`, and never buffers the
input. Use it for anything large.

## Configuration

```rust
pub struct Config {
    pub preset: Preset,          // Auto, Pytest, Cargo, Diff, Docker, Json, Generic
    pub format: OutputFormat,    // Markdown, Xml, Raw
    pub keep_frames: usize,      // vendor frames per folded run       (3)
    pub preserve_ansi: bool,     //                                    (false)
    pub dedupe_only: bool,       //                                    (false)
    pub context_lines: usize,    // diff context either side           (1)
    pub dedupe_window: usize,    // remembered lines                   (64)
    pub fuzzy_dedupe: bool,      // fold lines differing by numbers    (true)
}
```

## Writing your own processor

```rust
use ctrim::processors::{LineProcessor, LineSink};

struct DropDebugLines;

impl LineProcessor for DropDebugLines {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        if !line.contains("level=debug") {
            out.emit(line);
        }
    }

    fn name(&self) -> &'static str { "drop-debug" }
}
```

Compose processors with `ctrim::processors::Chain`:

```rust
use ctrim::processors::{Chain, ansi::AnsiStripper, dedupe::Deduper};

let mut chain = Chain::new(vec![
    Box::new(AnsiStripper::new()),
    Box::new(DropDebugLines),
    Box::new(Deduper::new(64, true)),
]);

let mut out: Vec<String> = Vec::new();
for line in input.lines() {
    chain.on_line(line, &mut out);
}
chain.finish(&mut out);
```

`Vec<String>` implements `LineSink`, which is handy in tests.

## Token counting

```rust
use ctrim::token;

let tokens = token::count("some text");     // estimate, or exact with the feature
let mut counter = token::Counter::new();    // additive over a stream of lines
counter.add_line("first line");
counter.add_line("second line");
assert!(counter.tokens() > 0);

if !token::is_exact() {
    println!("counts are estimates{}", token::approx_marker());
}
```

See [tokens.md](tokens.md).

## Feature flags

| Feature | Effect |
| :--- | :--- |
| _(default)_ | Dependency-free token estimation |
| `exact-tokens` | Real BPE counts via `tiktoken-rs` (`o200k_base`); ~4 MB larger, slower first call |
