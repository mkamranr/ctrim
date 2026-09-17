//! Repeated-line compression.
//!
//! Two mechanisms, both bounded in memory:
//!
//! * an *adjacent run* collapses consecutive similar lines into
//!   `[Repeated N times: "..."]`;
//! * a *window* of the last `M` emitted lines catches near-contiguous repeats
//!   (retry loops interleaved with other output) and reports them when the
//!   entry falls out of the window.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use super::{LineProcessor, LineSink};

/// Lines shorter than this only fold when adjacent — the window is too likely
/// to swallow meaningful short repeats such as `}` or `---`.
const MIN_WINDOW_LEN: usize = 24;
/// Longest prefix of the sample line kept inside a `[Repeated ...]` note.
const SAMPLE_LEN: usize = 120;
/// Emitted lines to wait after the last repeat before reporting a windowed
/// duplicate, so the note lands near the lines it describes.
const NOTE_DELAY: usize = 3;

struct RepeatRun {
    original: String,
    key: Vec<u8>,
    hash: u64,
    count: usize,
}

struct WindowEntry {
    key: Vec<u8>,
    hash: u64,
    sample: String,
    extra: usize,
    last_hit: usize,
}

/// Collapses repeated log lines.
pub struct Deduper {
    run: Option<RepeatRun>,
    window: VecDeque<WindowEntry>,
    capacity: usize,
    fuzzy: bool,
    /// Count of lines emitted so far, used to place windowed-duplicate notes.
    seq: usize,
    last_blank: bool,
}

impl Deduper {
    /// `fuzzy` folds lines that differ only in their numbers and ids, which is
    /// what makes retry storms and progress polling collapse; turn it off to
    /// require byte-identical text.
    pub fn new(capacity: usize, fuzzy: bool) -> Self {
        Self {
            run: None,
            window: VecDeque::new(),
            capacity: capacity.max(1),
            fuzzy,
            seq: 0,
            last_blank: false,
        }
    }

    /// Single exit point for output, so blank runs stay collapsed even when the
    /// lines between them were suppressed.
    fn emit(&mut self, line: &str, out: &mut dyn LineSink) {
        let blank = line.trim().is_empty();
        if blank && self.last_blank {
            return;
        }
        self.last_blank = blank;
        self.seq += 1;
        out.emit(line);
    }

    /// Report windowed duplicates once their run of repeats has gone quiet.
    fn drain_stale_notes(&mut self, out: &mut dyn LineSink) {
        let mut notes: Vec<String> = Vec::new();
        for entry in self.window.iter_mut() {
            if entry.extra > 0 && self.seq.saturating_sub(entry.last_hit) >= NOTE_DELAY {
                notes.push(repeat_extra_note(entry));
                entry.extra = 0;
            }
        }
        for note in notes {
            self.emit(&note, out);
        }
    }

    fn flush_run(&mut self, out: &mut dyn LineSink) {
        let Some(run) = self.run.take() else { return };
        if run.key.is_empty() {
            // Blank runs collapse to a single blank line; a note would cost
            // more tokens than the lines it replaces.
            self.emit("", out);
            return;
        }
        self.drain_stale_notes(out);
        if run.count == 1 {
            self.emit(&run.original, out);
        } else {
            let note = repeat_note(run.count, &run.original);
            self.emit(&note, out);
        }
        self.window_insert(run.key, run.hash, run.original, out);
    }

    fn window_insert(&mut self, key: Vec<u8>, hash: u64, original: String, out: &mut dyn LineSink) {
        if key.len() < MIN_WINDOW_LEN {
            return;
        }
        while self.window.len() >= self.capacity {
            if let Some(evicted) = self.window.pop_front() {
                if evicted.extra > 0 {
                    let note = repeat_extra_note(&evicted);
                    self.emit(&note, out);
                }
            }
        }
        let last_hit = self.seq;
        self.window.push_back(WindowEntry {
            key,
            hash,
            sample: original,
            extra: 0,
            last_hit,
        });
    }

    /// Returns true when the line matched a windowed entry and was suppressed.
    fn window_bump(&mut self, key: &[u8], hash: u64) -> bool {
        if key.len() < MIN_WINDOW_LEN {
            return false;
        }
        // Hashes are compared first so a full window costs 64 integer compares.
        let hit = self
            .window
            .iter()
            .rposition(|entry| entry.hash == hash && entry.key == key);
        match hit {
            Some(idx) => {
                // Refresh recency so hot lines stay in the window.
                let mut entry = self.window.remove(idx).expect("index from rposition");
                entry.extra += 1;
                entry.last_hit = self.seq;
                self.window.push_back(entry);
                true
            }
            None => false,
        }
    }
}

impl LineProcessor for Deduper {
    fn on_line(&mut self, line: &str, out: &mut dyn LineSink) {
        let key = normalize(line, self.fuzzy);
        let hash = hash_of(&key);

        if let Some(run) = &mut self.run {
            if run.hash == hash && run.key == key {
                run.count += 1;
                return;
            }
        }
        self.flush_run(out);

        if self.window_bump(&key, hash) {
            return;
        }

        self.run = Some(RepeatRun {
            original: line.to_string(),
            key,
            hash,
            count: 1,
        });
    }

    fn finish(&mut self, out: &mut dyn LineSink) {
        self.flush_run(out);
        while let Some(entry) = self.window.pop_front() {
            if entry.extra > 0 {
                let note = repeat_extra_note(&entry);
                self.emit(&note, out);
            }
        }
    }

    fn name(&self) -> &'static str {
        "dedupe"
    }
}

fn repeat_extra_note(entry: &WindowEntry) -> String {
    format!(
        "[+{} more occurrences of: \"{}\"]",
        entry.extra,
        sample(&entry.sample)
    )
}

fn repeat_note(count: usize, original: &str) -> String {
    format!("[Repeated {} times: \"{}\"]", count, sample(original))
}

fn sample(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= SAMPLE_LEN {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(SAMPLE_LEN).collect();
    format!("{cut}...")
}

/// Build the comparison key for a line.
///
/// Whitespace is collapsed so indentation changes do not matter. With `fuzzy`
/// on, digit runs collapse to `#` and alphanumeric runs that contain digits and
/// are at least [`ID_LEN`] long collapse entirely, which is what lets
/// `attempt=41` fold with `attempt=42` and one request id fold with the next.
///
/// One pass over the bytes, no regular expressions and no UTF-8 decoding: this
/// runs on every line of the input. The key is never displayed, so it does not
/// need to be valid text.
pub fn normalize(line: &str, fuzzy: bool) -> Vec<u8> {
    let bytes = line.as_bytes();
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace());
    let Some(start) = start else {
        return Vec::new();
    };
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);

    let mut out: Vec<u8> = Vec::with_capacity(end - start);
    let mut pending_space = false;
    let mut run = Run::default();

    for &byte in &bytes[start..end] {
        if byte.is_ascii_whitespace() {
            run.finish(&mut out, fuzzy);
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(b' ');
            pending_space = false;
        }
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            if run.len == 0 {
                run.start = out.len();
            }
            if fuzzy && byte.is_ascii_digit() {
                if !run.last_was_digit {
                    out.push(b'#');
                }
                run.last_was_digit = true;
                run.has_digit = true;
            } else {
                out.push(byte);
                run.last_was_digit = false;
            }
            run.len += 1;
        } else {
            run.finish(&mut out, fuzzy);
            out.push(byte);
        }
    }
    run.finish(&mut out, fuzzy);
    out
}

/// Alphanumeric runs at least this long that contain a digit are ids.
const ID_LEN: usize = 8;

/// State for the alphanumeric run currently being written into the key.
#[derive(Default)]
struct Run {
    start: usize,
    len: usize,
    has_digit: bool,
    last_was_digit: bool,
}

impl Run {
    fn finish(&mut self, out: &mut Vec<u8>, fuzzy: bool) {
        if fuzzy && self.has_digit && self.len >= ID_LEN {
            out.truncate(self.start);
            out.push(b'#');
        }
        *self = Run::default();
    }
}

fn hash_of(key: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str], capacity: usize) -> Vec<String> {
        let mut proc = Deduper::new(capacity, true);
        let mut out: Vec<String> = Vec::new();
        for line in lines {
            proc.on_line(line, &mut out);
        }
        proc.finish(&mut out);
        out
    }

    #[test]
    fn collapses_adjacent_identical_lines() {
        let out = run(&["Connecting to database...."; 5], 64);
        assert_eq!(
            out,
            vec![r#"[Repeated 5 times: "Connecting to database...."]"#]
        );
    }

    #[test]
    fn single_occurrence_passes_through_unchanged() {
        let out = run(&["one line only, nothing repeated"], 64);
        assert_eq!(out, vec!["one line only, nothing repeated"]);
    }

    #[test]
    fn timestamps_do_not_prevent_folding() {
        let out = run(
            &[
                "2024-05-01T10:00:01Z WARN retrying upstream connection",
                "2024-05-01T10:00:02Z WARN retrying upstream connection",
                "2024-05-01T10:00:03Z WARN retrying upstream connection",
            ],
            64,
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("[Repeated 3 times:"), "got {}", out[0]);
    }

    #[test]
    fn near_identical_lines_fold_within_ninety_percent() {
        let out = run(
            &[
                "waiting for container startup, attempt 9",
                "waiting for container startup, attempt 10",
                "waiting for container startup, attempt 11",
            ],
            64,
        );
        assert_eq!(out.len(), 1, "got {out:?}");
    }

    #[test]
    fn distinct_lines_are_preserved_in_order() {
        let out = run(&["alpha", "beta", "gamma"], 64);
        assert_eq!(out, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn interleaved_repeats_are_counted_by_the_window() {
        let out = run(
            &[
                "polling job status endpoint for completion",
                "still running the background worker task",
                "polling job status endpoint for completion",
                "still running the background worker task",
                "polling job status endpoint for completion",
            ],
            64,
        );
        let joined = out.join("\n");
        assert!(joined.contains("+2 more occurrences"), "got {joined}");
        assert!(joined.contains("+1 more occurrences"), "got {joined}");
        // Both distinct lines still appear once in their original position.
        assert_eq!(out[0], "polling job status endpoint for completion");
        assert_eq!(out[1], "still running the background worker task");
    }

    #[test]
    fn short_lines_are_not_window_deduped() {
        // Closing braces repeat legitimately in source and diffs.
        let out = run(&["}", "  return None", "}", "  return None"], 64);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn blank_runs_collapse_without_a_note() {
        let out = run(&["a", "", "", "", "b"], 64);
        assert_eq!(out, vec!["a", "", "b"]);
    }

    #[test]
    fn blank_lines_left_by_suppression_also_collapse() {
        let block = [
            "the same warning line repeated in a block",
            "  with an indented detail line under it",
            "",
        ];
        let mut lines: Vec<&str> = Vec::new();
        for _ in 0..4 {
            lines.extend_from_slice(&block);
        }
        lines.push("tail");
        let out = run(&lines, 64);
        let blanks = out.iter().filter(|l| l.is_empty()).count();
        assert_eq!(blanks, 1, "got {out:?}");
    }

    #[test]
    fn notes_land_near_the_lines_they_describe() {
        let mut lines: Vec<String> = Vec::new();
        for i in 0..3 {
            lines.push("a repeated warning line in the middle of output".to_string());
            lines.push(format!("unique filler line number {i} here"));
        }
        for i in 0..20 {
            lines.push(format!("later unrelated output line number {i}"));
        }
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let out = run(&refs, 64);
        let note_at = out
            .iter()
            .position(|l| l.starts_with("[+"))
            .expect("note emitted");
        assert!(note_at < 10, "note at {note_at} in {out:?}");
    }

    #[test]
    fn open_run_is_flushed_at_end_of_input() {
        let out = run(
            &[
                "x",
                "tail line repeated twice here",
                "tail line repeated twice here",
            ],
            64,
        );
        assert_eq!(out.len(), 2);
        assert!(out[1].starts_with("[Repeated 2 times:"));
    }

    #[test]
    fn long_samples_are_truncated_in_the_note() {
        let long = "e".repeat(400);
        let out = run(&[long.as_str(), long.as_str()], 64);
        assert!(out[0].ends_with("...\"]"), "got {}", out[0]);
        assert!(out[0].len() < 200);
    }

    #[test]
    fn exact_mode_keeps_lines_that_differ_by_a_number() {
        let lines = [
            "waiting for container startup, attempt 9",
            "waiting for container startup, attempt 10",
        ];
        let mut proc = Deduper::new(64, false);
        let mut out: Vec<String> = Vec::new();
        for line in lines {
            proc.on_line(line, &mut out);
        }
        proc.finish(&mut out);
        assert_eq!(out.len(), 2, "got {out:?}");
    }

    #[test]
    fn normalize_masks_numbers_and_ids_only_when_fuzzy() {
        let line = "req 2024-05-01 id=9f8e7d6c5b4a3210 took 42ms";
        assert_eq!(normalize(line, true), b"req #-#-# id=# took #ms");
        assert_eq!(normalize(line, false), line.as_bytes());
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("  a\t\t b  ", false), b"a b");
    }
}
