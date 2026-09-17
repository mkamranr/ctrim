//! The streaming driver: read, detect, process, render.

use std::io::{self, BufRead, Cursor, Write};

use crate::detector::{self, FormatType};
use crate::formatters::{self, Formatter, RenderContext};
use crate::processors::ansi::AnsiStripper;
use crate::processors::dedupe::Deduper;
use crate::processors::diff::DiffMinimizer;
use crate::processors::stacktrace::StackFolder;
use crate::processors::{Chain, LineProcessor, LineSink, ProcessedOutput};
use crate::utils::stats::ReductionStats;
use crate::{token, Config};

/// Lines buffered before detection runs. Bounds the replay buffer, and with it
/// the pipeline's memory use on unbounded streams.
pub const HEAD_LINES: usize = 200;

/// What a completed run reports back to the caller.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub detected_type: FormatType,
    pub error_summary: Vec<String>,
    pub stats: ReductionStats,
}

/// Assemble the processor chain for a detected format.
pub fn build_chain(format: FormatType, config: &Config) -> Chain {
    let mut procs: Vec<Box<dyn LineProcessor>> = Vec::new();
    if !config.preserve_ansi {
        procs.push(Box::new(AnsiStripper::new()));
    }
    if config.dedupe_only {
        procs.push(Box::new(Deduper::new(
            config.dedupe_window,
            config.fuzzy_dedupe,
        )));
        return Chain::new(procs);
    }
    match format {
        FormatType::GitDiff => {
            procs.push(Box::new(DiffMinimizer::new(config.context_lines)));
        }
        FormatType::DockerLog | FormatType::JsonLines => {
            procs.push(Box::new(Deduper::new(
                config.dedupe_window,
                config.fuzzy_dedupe,
            )));
        }
        FormatType::Pytest | FormatType::CargoCheck | FormatType::GenericLog => {
            procs.push(Box::new(StackFolder::new(config.keep_frames)));
            procs.push(Box::new(Deduper::new(
                config.dedupe_window,
                config.fuzzy_dedupe,
            )));
        }
    }
    Chain::new(procs)
}

/// Counts what it forwards, so stats measure content rather than wrapper.
struct CountingSink<'a> {
    writer: &'a mut dyn Write,
    formatter: &'a mut dyn Formatter,
    lines: usize,
    tokens: token::Counter,
    bytes: usize,
    error: Option<io::Error>,
}

impl LineSink for CountingSink<'_> {
    fn emit(&mut self, line: &str) {
        if self.error.is_some() {
            return;
        }
        self.lines += 1;
        self.tokens.add_line(line);
        self.bytes += line.len() + 1;
        if let Err(err) = self.formatter.line(self.writer, line) {
            self.error = Some(err);
        }
    }
}

/// Compress `input` into `output`.
///
/// Memory use is bounded by [`HEAD_LINES`] plus whatever the active processors
/// hold, never by the size of the input.
pub fn run<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    config: &Config,
) -> io::Result<Outcome> {
    let mut raw = RawCounts::default();
    let mut buf = Vec::with_capacity(256);

    // Buffer a bounded head so the detector has something to look at.
    let mut head: Vec<String> = Vec::new();
    while head.len() < HEAD_LINES {
        match read_line(&mut input, &mut buf)? {
            Some(line) => {
                raw.observe(&line);
                head.push(line.into_owned());
            }
            None => break,
        }
    }

    let format = config.preset.as_format().unwrap_or_else(|| {
        // Detect on cleaned text: colour codes hide the very line starts the
        // signatures look for (`error[E0308]`, `@@ `, `diff --git`).
        if config.preserve_ansi {
            detector::detect(&head)
        } else {
            let stripper = AnsiStripper::new();
            let cleaned: Vec<String> = head
                .iter()
                .map(|line| stripper.clean(line).into_owned())
                .collect();
            detector::detect(&cleaned)
        }
    });

    let mut chain = build_chain(format, config);
    let mut formatter = formatters::build(config.format);
    let mut ctx = RenderContext {
        format,
        error_summary: Vec::new(),
    };
    formatter.begin(&mut output, &ctx)?;

    let mut sink = CountingSink {
        writer: &mut output,
        formatter: formatter.as_mut(),
        lines: 0,
        tokens: token::Counter::new(),
        bytes: 0,
        error: None,
    };

    for line in &head {
        chain.on_line(line, &mut sink);
    }
    while let Some(line) = read_line(&mut input, &mut buf)? {
        raw.observe(&line);
        chain.on_line(&line, &mut sink);
        if sink.error.is_some() {
            break;
        }
    }
    chain.finish(&mut sink);

    let (lines, tokens, bytes, error) = (sink.lines, sink.tokens.tokens(), sink.bytes, sink.error);
    if let Some(err) = error {
        return Err(err);
    }

    ctx.error_summary = chain.summary();
    formatter.end(&mut output, &ctx)?;
    output.flush()?;

    Ok(Outcome {
        detected_type: format,
        error_summary: ctx.error_summary,
        stats: ReductionStats {
            raw_bytes: raw.bytes,
            reduced_bytes: bytes,
            raw_tokens: raw.tokens.tokens(),
            reduced_tokens: tokens,
            raw_lines: raw.lines,
            reduced_lines: lines,
        },
    })
}

/// Whole-string convenience wrapper. Holds the output in memory by definition.
pub fn run_str(input: &str, config: &Config) -> ProcessedOutput {
    let mut out: Vec<u8> = Vec::with_capacity(input.len() / 2);
    let outcome = run(Cursor::new(input.as_bytes()), &mut out, config)
        .expect("in-memory pipeline cannot fail on I/O");
    ProcessedOutput {
        content: String::from_utf8_lossy(&out).into_owned(),
        original_line_count: outcome.stats.raw_lines,
        processed_line_count: outcome.stats.reduced_lines,
        detected_type: outcome.detected_type,
        error_summary: outcome.error_summary,
        stats: outcome.stats,
    }
}

#[derive(Default)]
struct RawCounts {
    lines: usize,
    bytes: usize,
    tokens: token::Counter,
}

impl RawCounts {
    fn observe(&mut self, line: &str) {
        self.lines += 1;
        self.bytes += line.len() + 1;
        self.tokens.add_line(line);
    }
}

/// Read one line, tolerating invalid UTF-8 (logs are not always clean).
fn read_line<'a, R: BufRead>(
    input: &mut R,
    buf: &'a mut Vec<u8>,
) -> io::Result<Option<std::borrow::Cow<'a, str>>> {
    buf.clear();
    if input.read_until(b'\n', buf)? == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    }
    Ok(Some(String::from_utf8_lossy(buf)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OutputFormat, Preset};

    fn raw_config() -> Config {
        Config {
            format: OutputFormat::Raw,
            ..Config::default()
        }
    }

    #[test]
    fn passes_clean_input_through_unchanged() {
        let out = run_str("alpha\nbeta\n", &raw_config());
        assert_eq!(out.content, "alpha\nbeta\n");
        assert_eq!(out.original_line_count, 2);
        assert_eq!(out.processed_line_count, 2);
    }

    #[test]
    fn detects_format_beyond_the_head_buffer() {
        let mut input = String::new();
        for i in 0..(HEAD_LINES + 50) {
            input.push_str(&format!(
                "api_1  | 2024-05-01T10:00:0{}Z level=info msg=tick\n",
                i % 10
            ));
        }
        let out = run_str(&input, &raw_config());
        assert_eq!(out.detected_type, FormatType::DockerLog);
        assert!(
            out.processed_line_count < 20,
            "got {} lines",
            out.processed_line_count
        );
    }

    #[test]
    fn detects_format_through_ansi_colour_codes() {
        let input = "\x1b[1m\x1b[91merror[E0308]\x1b[0m: mismatched types\n \x1b[1m\x1b[94m--> \x1b[0msrc/main.rs:6:5\n";
        assert_eq!(
            run_str(input, &raw_config()).detected_type,
            FormatType::CargoCheck
        );
    }

    #[test]
    fn preset_overrides_detection() {
        let config = Config {
            preset: Preset::Diff,
            ..raw_config()
        };
        let out = run_str("nothing diff-like here\n", &config);
        assert_eq!(out.detected_type, FormatType::GitDiff);
    }

    #[test]
    fn invalid_utf8_does_not_abort_the_run() {
        let mut bytes = b"good line\n".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
        bytes.extend_from_slice(b"another good line\n");
        let mut out: Vec<u8> = Vec::new();
        let outcome = run(Cursor::new(bytes), &mut out, &raw_config()).unwrap();
        assert_eq!(outcome.stats.raw_lines, 3);
    }

    #[test]
    fn markdown_format_wraps_content_in_a_fence() {
        let out = run_str("hello\n", &Config::default());
        assert!(out.content.starts_with("```text\n"));
        assert!(out.content.trim_end().ends_with("```"));
    }

    #[test]
    fn xml_format_tags_the_detected_type() {
        let config = Config {
            format: OutputFormat::Xml,
            ..Config::default()
        };
        let out = run_str(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,1 +1,1 @@\n-x\n+y\n",
            &config,
        );
        assert!(out.content.starts_with("<context type=\"git-diff\">"));
        assert!(out.content.trim_end().ends_with("</context>"));
    }

    #[test]
    fn error_summary_reaches_the_output() {
        let input = "Traceback (most recent call last):\n  File \"a.py\", line 1, in f\n    g()\nValueError: boom\n";
        let config = Config {
            format: OutputFormat::Xml,
            ..Config::default()
        };
        let out = run_str(input, &config);
        assert!(out.content.contains("<error_summary>"));
        assert!(out.content.contains("ValueError: boom"));
        assert_eq!(out.error_summary, vec!["ValueError: boom"]);
    }

    #[test]
    fn empty_input_produces_empty_content_and_zero_stats() {
        let out = run_str("", &raw_config());
        assert_eq!(out.content, "");
        assert_eq!(out.stats.raw_tokens, 0);
        assert_eq!(out.stats.reduction_percentage(), 0.0);
    }

    #[test]
    fn dedupe_only_skips_the_other_processors() {
        let config = Config {
            dedupe_only: true,
            ..raw_config()
        };
        let input = "Traceback (most recent call last):\n  File \"/srv/.venv/lib/python3.11/site-packages/a.py\", line 1, in f\n    g()\nValueError: boom\n";
        let out = run_str(input, &config);
        assert!(
            out.content.contains("site-packages"),
            "trace must survive: {}",
            out.content
        );
    }
}
