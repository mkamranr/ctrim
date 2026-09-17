//! Unified-diff minimization.
//!
//! Three savings: untouched context beyond `context_lines` is dropped (hunks
//! are split and their `@@` headers recomputed so the result is still a valid
//! patch), generated files are replaced by a one-line summary, and `index`
//! blob-hash lines are removed.

use once_cell::sync::Lazy;
use regex::Regex;

use super::{LineProcessor, LineSink};

/// `@@ -12,7 +12,9 @@ fn handler()`
static HUNK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap());
/// `diff --git a/src/main.rs b/src/main.rs`
static DIFF_GIT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^diff --git a/(.+?) b/(.+)$").unwrap());

/// Files whose diffs are churn: generated, minified, or lock files.
const NOISE_BASENAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "composer.lock",
    "Gemfile.lock",
    "poetry.lock",
    "go.sum",
    "flake.lock",
    "bun.lockb",
];
const NOISE_SUFFIXES: &[&str] = &[".lock", ".min.js", ".min.css", ".map", ".pb.go", "_pb2.py"];

struct HunkLine {
    kind: char,
    text: String,
}

#[derive(Default)]
struct FileState {
    path: String,
    noise: bool,
    added: usize,
    removed: usize,
    binary: bool,
    header_emitted: bool,
}

/// Minimizes a unified diff stream.
pub struct DiffMinimizer {
    context: usize,
    file: Option<FileState>,
    hunk_header: Option<(usize, usize)>,
    body: Vec<HunkLine>,
}

impl DiffMinimizer {
    pub fn new(context: usize) -> Self {
        Self {
            context,
            file: None,
            hunk_header: None,
            body: Vec::new(),
        }
    }

    fn flush_hunk(&mut self, out: &mut dyn LineSink) {
        let Some((old_start, new_start)) = self.hunk_header.take() else {
            self.body.clear();
            return;
        };
        let body = std::mem::take(&mut self.body);
        let noise = self.file.as_ref().is_some_and(|f| f.noise);
        if let Some(file) = &mut self.file {
            for line in &body {
                match line.kind {
                    '+' => file.added += 1,
                    '-' => file.removed += 1,
                    _ => {}
                }
            }
        }
        if noise {
            return;
        }
        for line in render_hunk(&body, old_start, new_start, self.context) {
            out.emit(&line);
        }
    }

    fn flush_file(&mut self, out: &mut dyn LineSink) {
        self.flush_hunk(out);
        let Some(file) = self.file.take() else { return };
        if file.noise {
            out.emit(&format!(
                "[skipped generated file {}: +{}/-{} lines]",
                file.path, file.added, file.removed
            ));
        } else if file.binary {
            out.emit(&format!("[binary file {} changed]", file.path));
        }
    }

    /// Header lines are held back so a noise file emits nothing but its summary.
    fn emit_header(&mut self, line: &str, out: &mut dyn LineSink) {
        let noise = self.file.as_ref().is_some_and(|f| f.noise);
        if noise {
            return;
        }
        if let Some(file) = &mut self.file {
            file.header_emitted = true;
        }
        out.emit(line);
    }
}

impl LineProcessor for DiffMinimizer {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        if let Some(caps) = DIFF_GIT.captures(line) {
            self.flush_file(out);
            let path = caps[2].to_string();
            let noise = is_noise(&path);
            self.file = Some(FileState {
                noise,
                path,
                ..FileState::default()
            });
            self.emit_header(line, out);
            return;
        }

        if let Some(caps) = HUNK.captures(line) {
            self.flush_hunk(out);
            let old_start = caps[1].parse().unwrap_or(1);
            let new_start = caps[3].parse().unwrap_or(1);
            self.hunk_header = Some((old_start, new_start));
            return;
        }

        if self.hunk_header.is_some() {
            let kind = line.chars().next().unwrap_or(' ');
            if matches!(kind, ' ' | '+' | '-' | '\\') || line.is_empty() {
                self.body.push(HunkLine {
                    kind: if line.is_empty() { ' ' } else { kind },
                    text: line.to_string(),
                });
                return;
            }
            // Anything else ends the hunk (e.g. a trailing summary line).
            self.flush_hunk(out);
        }

        if self.file.is_some() {
            if line.starts_with("index ") || line.starts_with("similarity index ") {
                return; // Blob hashes carry no meaning for a model.
            }
            if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                if let Some(file) = &mut self.file {
                    file.binary = true;
                }
                return;
            }
            self.emit_header(line, out);
            return;
        }

        out.emit(line);
    }

    fn finish(&mut self, out: &mut dyn LineSink) {
        self.flush_file(out);
    }

    fn name(&self) -> &'static str {
        "diff"
    }
}

fn is_noise(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    NOISE_BASENAMES.contains(&base) || NOISE_SUFFIXES.iter().any(|s| path.ends_with(s))
}

/// Split a hunk body into the regions worth keeping and render each as its own
/// `@@` hunk with recomputed line numbers.
fn render_hunk(
    body: &[HunkLine],
    old_start: usize,
    new_start: usize,
    context: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    if body.is_empty() {
        return out;
    }

    // Line numbers each body entry occupies in the old and new file.
    let mut old_no = vec![0usize; body.len()];
    let mut new_no = vec![0usize; body.len()];
    let (mut old_cur, mut new_cur) = (old_start, new_start);
    for (i, line) in body.iter().enumerate() {
        old_no[i] = old_cur;
        new_no[i] = new_cur;
        match line.kind {
            ' ' => {
                old_cur += 1;
                new_cur += 1;
            }
            '-' => old_cur += 1,
            '+' => new_cur += 1,
            _ => {}
        }
    }

    let changes: Vec<usize> = body
        .iter()
        .enumerate()
        .filter(|(_, l)| matches!(l.kind, '+' | '-'))
        .map(|(i, _)| i)
        .collect();
    if changes.is_empty() {
        return out; // Context-only hunk: nothing changed, nothing to say.
    }

    // Keep ranges around each change, merged when they touch or overlap.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &idx in &changes {
        let start = idx.saturating_sub(context);
        let end = (idx + context + 1).min(body.len());
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }

    for (start, end) in ranges {
        // A `\ No newline` marker belongs to the line before it.
        let mut end = end;
        if end < body.len() && body[end].kind == '\\' {
            end += 1;
        }
        let slice = &body[start..end];
        let old_count = slice.iter().filter(|l| matches!(l.kind, ' ' | '-')).count();
        let new_count = slice.iter().filter(|l| matches!(l.kind, ' ' | '+')).count();
        let old_begin = if old_count == 0 {
            old_no[start].saturating_sub(1)
        } else {
            old_no[start]
        };
        let new_begin = if new_count == 0 {
            new_no[start].saturating_sub(1)
        } else {
            new_no[start]
        };
        out.push(format!(
            "@@ -{old_begin},{old_count} +{new_begin},{new_count} @@"
        ));
        for line in slice {
            out.push(line.text.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, context: usize) -> String {
        let mut proc = DiffMinimizer::new(context);
        let mut out: Vec<String> = Vec::new();
        for line in input.lines() {
            proc.on_line(line, &mut out);
        }
        proc.finish(&mut out);
        out.join("\n")
    }

    #[test]
    fn drops_context_beyond_the_configured_window() {
        let mut input = String::from("diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,12 +1,12 @@\n");
        for i in 1..=8 {
            input.push_str(&format!(" line {i}\n"));
        }
        input.push_str("-old value\n+new value\n");
        for i in 9..=12 {
            input.push_str(&format!(" line {i}\n"));
        }
        let out = run(&input, 2);
        assert!(
            !out.contains("line 1\n"),
            "far context should be dropped:\n{out}"
        );
        assert!(out.contains(" line 7"));
        assert!(out.contains(" line 8"));
        assert!(out.contains("-old value"));
        assert!(out.contains("+new value"));
        assert!(out.contains(" line 9"));
        assert!(!out.contains("line 11"));
    }

    #[test]
    fn recomputed_hunk_header_counts_match_the_body() {
        let input = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -10,5 +10,5 @@\n one\n two\n-three\n+THREE\n four\n";
        let out = run(input, 1);
        let header = out.lines().find(|l| l.starts_with("@@")).unwrap();
        assert_eq!(header, "@@ -11,3 +11,3 @@");
    }

    #[test]
    fn distant_changes_split_into_separate_hunks() {
        let mut input = String::from(
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,30 +1,30 @@\n",
        );
        input.push_str("-first change\n+first changed\n");
        for i in 1..=20 {
            input.push_str(&format!(" filler {i}\n"));
        }
        input.push_str("-second change\n+second changed\n");
        let out = run(&input, 2);
        assert_eq!(out.matches("@@ -").count(), 2, "got:\n{out}");
        assert!(!out.contains("filler 10"));
    }

    #[test]
    fn index_lines_are_removed() {
        let input = "diff --git a/a.txt b/a.txt\nindex 89abc12..def3456 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-a\n+b\n";
        let out = run(input, 3);
        assert!(!out.contains("index 89abc12"));
        assert!(out.contains("--- a/a.txt"));
    }

    #[test]
    fn lockfiles_collapse_to_a_summary_line() {
        let input = "diff --git a/Cargo.lock b/Cargo.lock\nindex 1..2 100644\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,4 +1,4 @@\n-old\n-older\n+new\n+newer\n+newest\ndiff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,1 @@\n-x\n+y\n";
        let out = run(input, 3);
        assert!(
            out.contains("[skipped generated file Cargo.lock: +3/-2 lines]"),
            "got:\n{out}"
        );
        assert!(!out.contains("--- a/Cargo.lock"));
        assert!(out.contains("--- a/src/main.rs"));
        assert!(out.contains("+y"));
    }

    #[test]
    fn binary_files_collapse_to_a_marker() {
        let input = "diff --git a/logo.png b/logo.png\nindex 1..2 100644\nBinary files a/logo.png and b/logo.png differ\n";
        let out = run(input, 3);
        assert_eq!(
            out,
            "diff --git a/logo.png b/logo.png\n[binary file logo.png changed]"
        );
    }

    #[test]
    fn context_only_hunks_are_dropped_entirely() {
        let input =
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n two\n";
        let out = run(input, 3);
        assert!(!out.contains("@@"));
    }

    #[test]
    fn no_newline_marker_stays_with_its_line() {
        let input = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-a\n+b\n\\ No newline at end of file\n";
        let out = run(input, 3);
        assert!(out.contains("\\ No newline at end of file"));
    }

    #[test]
    fn non_diff_input_passes_through() {
        let out = run("hello\nworld", 3);
        assert_eq!(out, "hello\nworld");
    }
}
