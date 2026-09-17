//! Stack-trace folding for Python, pytest, JavaScript and Rust.
//!
//! A trace is collected frame by frame, frames are classified as user or vendor
//! code by path, and each contiguous vendor run longer than `keep_frames` is
//! replaced by `[... N vendor frames omitted ...]`. The exception line and every
//! user frame always survive.

use once_cell::sync::Lazy;
use regex::Regex;

use super::{LineProcessor, LineSink};

/// Path fragments that mark a frame as third-party code.
const VENDOR_MARKERS: &[&str] = &[
    "/node_modules/",
    "\\node_modules\\",
    "/site-packages/",
    "/dist-packages/",
    "/.venv/",
    "/venv/lib/",
    "/.pyenv/",
    "/usr/lib/python",
    "/lib/python3",
    "<frozen ",
    "node:internal/",
    "/rustc/",
    "/.cargo/registry/",
    "/.rustup/toolchains/",
    "/go/pkg/mod/",
    "/vendor/",
    "\\site-packages\\",
    // A frame with no source file of its own is runtime plumbing, never the
    // user's code.
    "(<anonymous>)",
    "(native)",
];

/// `  File "app/main.py", line 42, in handler`
static PY_FRAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^\s*File "(?P<path>[^"]*)", line \d+(?:, in .*)?$"#).unwrap());
/// pytest long-form frame: `tests/test_api.py:12: in test_get`
static PYTEST_FRAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?P<path>[^\s:][^:]*):\d+: in \S+\s*$").unwrap());
/// `    at Object.handler (/srv/app/index.js:10:15)`
static JS_FRAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s+at\s+\S.*$").unwrap());
/// `   7: core::panicking::panic_fmt`
static RUST_FRAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\d+:\s+\S+").unwrap());
/// `             at /rustc/abc/library/core/src/panicking.rs:72`
static RUST_AT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s+at\s+\S+").unwrap());
/// `ValueError: expected an int` / `AssertionError: assert 4 == 5`
static EXCEPTION_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[\w.]*(?:Error|Exception|Warning|Failure|Panic)\b.*").unwrap());

/// Most summary lines worth carrying into `<error_summary>`.
const MAX_SUMMARY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Idle,
    Python,
    Pytest,
    Js,
    Rust,
}

struct Frame {
    lines: Vec<String>,
    vendor: bool,
}

/// Folds vendor frames out of stack traces.
pub struct StackFolder {
    mode: Mode,
    header: Vec<String>,
    frames: Vec<Frame>,
    keep_frames: usize,
    /// Whether frame 0 of the current dialect is worth pinning (see [`fold`]).
    pin_first: bool,
    summary: Vec<String>,
    prev: String,
}

impl StackFolder {
    pub fn new(keep_frames: usize) -> Self {
        Self {
            mode: Mode::Idle,
            header: Vec::new(),
            frames: Vec::new(),
            keep_frames,
            pin_first: false,
            summary: Vec::new(),
            prev: String::new(),
        }
    }

    fn start(&mut self, mode: Mode, header: Option<&str>) {
        self.mode = mode;
        // Python and pytest list the outermost frame first, so frame 0 is the
        // entry point; JavaScript lists the throw site first. Both are worth
        // keeping. A Rust backtrace starts at `rust_begin_unwind`, which is
        // never worth keeping.
        self.pin_first = matches!(mode, Mode::Python | Mode::Pytest | Mode::Js);
        self.header.clear();
        self.frames.clear();
        if let Some(header) = header {
            self.header.push(header.to_string());
        }
    }

    fn push_frame(&mut self, line: &str, path: &str) {
        self.frames.push(Frame {
            lines: vec![line.to_string()],
            vendor: is_vendor(path),
        });
    }

    fn append_to_frame(&mut self, line: &str) {
        match self.frames.last_mut() {
            Some(frame) => frame.lines.push(line.to_string()),
            None => self.header.push(line.to_string()),
        }
    }

    fn note_summary(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() || self.summary.len() >= MAX_SUMMARY {
            return;
        }
        let is_error = EXCEPTION_LINE.is_match(trimmed)
            || trimmed.starts_with("E ")
            || trimmed.starts_with("error:")
            || trimmed.starts_with("error[");
        if is_error && !self.summary.iter().any(|s| s == trimmed) {
            self.summary.push(trimmed.to_string());
        }
    }

    /// Emit the collected block with vendor runs folded.
    fn flush(&mut self, out: &mut dyn LineSink) {
        for line in std::mem::take(&mut self.header) {
            out.emit(&line);
        }
        let frames = std::mem::take(&mut self.frames);
        for line in fold(frames, self.keep_frames, self.pin_first) {
            out.emit(&line);
        }
        self.mode = Mode::Idle;
    }

    /// Leave trace mode because `line` does not belong to the trace.
    fn close_with(&mut self, line: &str, out: &mut dyn LineSink) {
        self.flush(out);
        self.note_summary(line);
        out.emit(line);
        self.prev = line.to_string();
    }
}

/// Cheap byte tests that must pass before a regex runs. Ordinary log lines fail
/// all of them, which keeps the regex engine off the hot path.
fn could_be_pytest_frame(line: &str) -> bool {
    line.contains(": in ")
}

fn could_be_js_frame(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && line.trim_start().starts_with("at ")
}

fn could_be_rust_frame(line: &str) -> bool {
    line.trim_start().starts_with(|c: char| c.is_ascii_digit())
}

impl LineProcessor for StackFolder {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        match self.mode {
            Mode::Idle => {
                let trimmed = line.trim();
                if trimmed.starts_with("Traceback (most recent call last)") {
                    self.start(Mode::Python, Some(line));
                } else if trimmed == "stack backtrace:" {
                    self.start(Mode::Rust, Some(line));
                } else if let Some(caps) = could_be_pytest_frame(line)
                    .then(|| PYTEST_FRAME.captures(line))
                    .flatten()
                {
                    let path = caps["path"].to_string();
                    self.start(Mode::Pytest, None);
                    self.push_frame(line, &path);
                } else if could_be_js_frame(line) && JS_FRAME.is_match(line) {
                    let prev = std::mem::take(&mut self.prev);
                    self.note_summary(&prev);
                    self.start(Mode::Js, None);
                    self.push_frame(line, line);
                } else {
                    self.note_summary(line);
                    out.emit(line);
                    self.prev = line.to_string();
                }
            }
            Mode::Python => {
                if let Some(caps) = line
                    .trim_start()
                    .starts_with("File \"")
                    .then(|| PY_FRAME.captures(line))
                    .flatten()
                {
                    let path = caps["path"].to_string();
                    self.push_frame(line, &path);
                } else if line.trim().is_empty() || line.starts_with(char::is_whitespace) {
                    self.append_to_frame(line);
                } else {
                    // Column-zero, non-empty: the exception line ends the trace.
                    self.close_with(line, out);
                }
            }
            Mode::Pytest => {
                if let Some(caps) = could_be_pytest_frame(line)
                    .then(|| PYTEST_FRAME.captures(line))
                    .flatten()
                {
                    let path = caps["path"].to_string();
                    self.push_frame(line, &path);
                } else if line.starts_with(char::is_whitespace) || line.starts_with("E ") {
                    self.note_summary(line);
                    self.append_to_frame(line);
                } else {
                    self.close_with(line, out);
                }
            }
            Mode::Js => {
                if could_be_js_frame(line) && JS_FRAME.is_match(line) {
                    self.push_frame(line, line);
                } else {
                    self.close_with(line, out);
                }
            }
            Mode::Rust => {
                if could_be_rust_frame(line) && RUST_FRAME.is_match(line) {
                    self.push_frame(line, line);
                } else if could_be_js_frame(line) && RUST_AT.is_match(line) {
                    let vendor = is_vendor(line);
                    if let Some(frame) = self.frames.last_mut() {
                        frame.vendor = frame.vendor || vendor;
                        frame.lines.push(line.to_string());
                    } else {
                        self.header.push(line.to_string());
                    }
                } else {
                    self.close_with(line, out);
                }
            }
        }
    }

    fn finish(&mut self, out: &mut dyn LineSink) {
        if self.mode != Mode::Idle {
            self.flush(out);
        }
    }

    fn name(&self) -> &'static str {
        "stacktrace"
    }

    fn summary(&self) -> Vec<String> {
        self.summary.clone()
    }
}

fn is_vendor(path: &str) -> bool {
    VENDOR_MARKERS.iter().any(|marker| path.contains(marker))
}

/// Keep every user frame, the outermost frame, and the `keep` deepest frames of
/// each vendor run; replace the rest with a single marker line.
fn fold(frames: Vec<Frame>, keep: usize, pin_first: bool) -> Vec<String> {
    let total = frames.len();
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < total {
        if !frames[idx].vendor {
            out.extend(frames[idx].lines.iter().cloned());
            idx += 1;
            continue;
        }
        let start = idx;
        while idx < total && frames[idx].vendor {
            idx += 1;
        }
        let run = &frames[start..idx];
        let pinned = usize::from(start == 0 && pin_first);
        if run.len() <= keep + pinned {
            for frame in run {
                out.extend(frame.lines.iter().cloned());
            }
            continue;
        }
        if pinned == 1 {
            out.extend(run[0].lines.iter().cloned());
        }
        let omitted = run.len() - keep - pinned;
        out.push(format!("[... {omitted} vendor frames omitted ...]"));
        for frame in &run[run.len() - keep..] {
            out.extend(frame.lines.iter().cloned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, keep: usize) -> (Vec<String>, Vec<String>) {
        let mut proc = StackFolder::new(keep);
        let mut out: Vec<String> = Vec::new();
        for line in input.lines() {
            proc.on_line(line, &mut out);
        }
        proc.finish(&mut out);
        let summary = proc.summary();
        (out, summary)
    }

    const PY_TRACE: &str = r#"Traceback (most recent call last):
  File "app/main.py", line 42, in handler
    return service.run(payload)
  File "/srv/.venv/lib/python3.11/site-packages/flask/app.py", line 1, in wsgi_app
    rv = self.dispatch_request()
  File "/srv/.venv/lib/python3.11/site-packages/flask/app.py", line 2, in dispatch_request
    rv = self.ensure_sync(self.view_functions[rule.endpoint])
  File "/srv/.venv/lib/python3.11/site-packages/werkzeug/local.py", line 3, in inner
    return f(*args, **kwargs)
  File "/srv/.venv/lib/python3.11/site-packages/werkzeug/local.py", line 4, in inner2
    return f(*args, **kwargs)
  File "/srv/.venv/lib/python3.11/site-packages/click/core.py", line 5, in invoke
    return ctx.invoke(self.callback)
  File "app/service.py", line 7, in run
    raise ValueError("bad payload")
ValueError: bad payload"#;

    #[test]
    fn python_user_frames_and_exception_survive() {
        let (out, _) = run(PY_TRACE, 3);
        let joined = out.join("\n");
        assert!(joined.contains("app/main.py"));
        assert!(joined.contains("app/service.py"));
        assert!(joined.ends_with("ValueError: bad payload"));
        assert!(joined.starts_with("Traceback (most recent call last):"));
    }

    #[test]
    fn python_vendor_run_is_folded_with_a_count() {
        let (out, _) = run(PY_TRACE, 3);
        let joined = out.join("\n");
        assert!(
            joined.contains("[... 2 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
        // 5 vendor frames in, 3 kept.
        assert_eq!(joined.matches("site-packages").count(), 3, "got:\n{joined}");
    }

    #[test]
    fn keep_frames_zero_folds_the_whole_vendor_run() {
        let (out, _) = run(PY_TRACE, 0);
        let joined = out.join("\n");
        assert!(
            joined.contains("[... 5 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
        assert!(!joined.contains("site-packages"));
    }

    #[test]
    fn python_exception_line_becomes_the_summary() {
        let (_, summary) = run(PY_TRACE, 3);
        assert_eq!(summary, vec!["ValueError: bad payload"]);
    }

    #[test]
    fn short_vendor_runs_are_left_alone() {
        let input = r#"Traceback (most recent call last):
  File "app/main.py", line 1, in a
    b()
  File "/srv/.venv/lib/python3.11/site-packages/x.py", line 2, in b
    c()
  File "app/other.py", line 3, in c
    raise RuntimeError("x")
RuntimeError: x"#;
        let (out, _) = run(input, 3);
        assert!(!out.join("\n").contains("omitted"));
    }

    #[test]
    fn javascript_frames_fold_node_modules() {
        let input = r#"TypeError: Cannot read properties of undefined
    at handler (/srv/app/index.js:10:15)
    at /srv/node_modules/express/lib/router/layer.js:95:5
    at next (/srv/node_modules/express/lib/router/route.js:144:13)
    at Route.dispatch (/srv/node_modules/express/lib/router/route.js:114:3)
    at Layer.handle (/srv/node_modules/express/lib/router/layer.js:95:5)
    at processTicksAndRejections (node:internal/process/task_queues:95:5)
    at main (/srv/app/server.js:4:1)
done"#;
        let (out, summary) = run(input, 2);
        let joined = out.join("\n");
        assert!(
            joined.contains("[... 3 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
        assert!(joined.contains("at handler (/srv/app/index.js:10:15)"));
        assert!(joined.contains("at main (/srv/app/server.js:4:1)"));
        assert!(joined.ends_with("done"));
        assert_eq!(
            summary,
            vec!["TypeError: Cannot read properties of undefined"]
        );
    }

    #[test]
    fn frames_without_a_source_file_count_as_vendor() {
        let input = "TypeError: boom\n    at fail (/srv/app/a.js:1:1)\n    at /srv/node_modules/x/y.js:2:2\n    at new Promise (<anonymous>)\n    at /srv/node_modules/x/z.js:3:3\n    at main (/srv/app/b.js:4:4)\ndone";
        let (out, _) = run(input, 1);
        let joined = out.join("\n");
        assert!(
            joined.contains("[... 2 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
    }

    #[test]
    fn pytest_long_form_frames_fold() {
        let input = r#"____________________ test_get ____________________

tests/test_api.py:12: in test_get
    resp = client.get("/users")
../.venv/lib/python3.11/site-packages/requests/api.py:73: in get
    return request("get", url, **kwargs)
../.venv/lib/python3.11/site-packages/requests/api.py:59: in request
    return session.request(method=method, url=url, **kwargs)
../.venv/lib/python3.11/site-packages/requests/sessions.py:589: in request
    resp = self.send(prep, **send_kwargs)
../.venv/lib/python3.11/site-packages/requests/adapters.py:519: in send
    raise ConnectionError(e, request=request)
E   requests.exceptions.ConnectionError: Max retries exceeded"#;
        let (out, summary) = run(input, 1);
        let joined = out.join("\n");
        assert!(joined.contains("tests/test_api.py:12: in test_get"));
        assert!(
            joined.contains("[... 3 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
        assert!(joined.contains("E   requests.exceptions.ConnectionError"));
        assert_eq!(summary.len(), 1);
    }

    #[test]
    fn rust_backtrace_folds_std_frames() {
        let input = r#"stack backtrace:
   0: rust_begin_unwind
             at /rustc/abc/library/std/src/panicking.rs:665:5
   1: core::panicking::panic_fmt
             at /rustc/abc/library/core/src/panicking.rs:74:14
   2: core::panicking::panic
             at /rustc/abc/library/core/src/panicking.rs:148:5
   3: core::option::Option<T>::unwrap
             at /rustc/abc/library/core/src/option.rs:935:21
   4: myapp::config::load
             at ./src/config.rs:22:9
note: run with RUST_BACKTRACE=full"#;
        let (out, _) = run(input, 1);
        let joined = out.join("\n");
        assert!(
            joined.contains("[... 3 vendor frames omitted ...]"),
            "got:\n{joined}"
        );
        assert!(joined.contains("myapp::config::load"));
        assert!(joined.ends_with("note: run with RUST_BACKTRACE=full"));
    }

    #[test]
    fn trace_open_at_end_of_input_is_still_emitted() {
        let input =
            "Traceback (most recent call last):\n  File \"app/main.py\", line 1, in a\n    b()";
        let (out, _) = run(input, 3);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn plain_text_passes_through_untouched() {
        let input = "hello\nworld";
        let (out, _) = run(input, 3);
        assert_eq!(out, vec!["hello", "world"]);
    }
}
