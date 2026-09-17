//! ANSI escape sequence removal and carriage-return collapsing.

use std::borrow::Cow;

use once_cell::sync::Lazy;
use regex::Regex;

use super::{LineProcessor, LineSink};

/// CSI (`ESC [ ... final`), OSC (`ESC ] ... BEL|ST`), charset selection and the
/// two-byte Fe escapes. Written as one alternation so a line is scanned once.
static ANSI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"\x1b\[[0-9;:?<>=]*[ -/]*[@-~]",      // CSI
        r"|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", // OSC, BEL- or ST-terminated
        r"|\x1b[PX^_][^\x1b]*(?:\x1b\\)?",     // DCS / SOS / PM / APC
        r"|\x1b[()][A-Za-z0-9]",               // charset selection
        r"|\x1b[@-Z\\-_]",                     // remaining Fe escapes
    ))
    .expect("static ANSI regex is valid")
});

/// Strips ANSI sequences and collapses `\r`-overwritten progress output.
///
/// A progress bar writes many frames separated by carriage returns; a terminal
/// shows only the last one, so that is what survives here.
#[derive(Debug, Default)]
pub struct AnsiStripper {
    collapse_cr: bool,
}

impl AnsiStripper {
    pub fn new() -> Self {
        Self { collapse_cr: true }
    }

    /// Clean a single line, borrowing it unchanged when there is nothing to do —
    /// which is the common case, so it must not allocate.
    pub fn clean<'a>(&self, line: &'a str) -> Cow<'a, str> {
        let line = if self.collapse_cr {
            last_cr_segment(line)
        } else {
            line
        };
        if line.as_bytes().contains(&0x1b) || line.as_bytes().contains(&0x08) {
            let stripped = ANSI.replace_all(line, "");
            Cow::Owned(strip_backspaces(stripped.as_ref()))
        } else {
            Cow::Borrowed(line)
        }
    }
}

/// Everything after the final carriage return, unless that would blank the line.
fn last_cr_segment(line: &str) -> &str {
    match line.rfind('\r') {
        Some(idx) => {
            let tail = &line[idx + 1..];
            if tail.trim().is_empty() {
                // A trailing `\r` with nothing after it: keep the last segment
                // that actually had content.
                line[..idx].rsplit('\r').next().unwrap_or(tail)
            } else {
                tail
            }
        }
        None => line,
    }
}

/// Applies `\x08` (backspace) the way a terminal would.
fn strip_backspaces(line: &str) -> String {
    if !line.contains('\x08') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    for ch in line.chars() {
        if ch == '\x08' {
            out.pop();
        } else {
            out.push(ch);
        }
    }
    out
}

impl LineProcessor for AnsiStripper {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        out.emit(&self.clean(line));
    }

    fn name(&self) -> &'static str {
        "ansi"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(s: &str) -> String {
        AnsiStripper::new().clean(s).into_owned()
    }

    #[test]
    fn strips_sgr_color_codes() {
        assert_eq!(
            clean("\x1b[31mFAILED\x1b[0m test_login"),
            "FAILED test_login"
        );
    }

    #[test]
    fn strips_cursor_and_erase_sequences() {
        assert_eq!(clean("\x1b[2K\x1b[1Gbuilding"), "building");
    }

    #[test]
    fn strips_osc_hyperlinks() {
        assert_eq!(
            clean("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            "link"
        );
    }

    #[test]
    fn keeps_leading_whitespace_intent() {
        assert_eq!(clean("\x1b[33m    at foo.js:1\x1b[0m"), "    at foo.js:1");
    }

    #[test]
    fn collapses_carriage_return_progress_frames() {
        assert_eq!(clean("10%\r50%\r100% done"), "100% done");
    }

    #[test]
    fn trailing_cr_keeps_last_content_segment() {
        assert_eq!(clean("downloading\r"), "downloading");
    }

    #[test]
    fn applies_backspaces() {
        assert_eq!(clean("abcX\x08"), "abc");
    }

    #[test]
    fn plain_line_is_untouched() {
        assert_eq!(
            clean("  File \"a.py\", line 3, in f"),
            "  File \"a.py\", line 3, in f"
        );
    }
}
