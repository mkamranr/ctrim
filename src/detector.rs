//! Input format auto-detection.
//!
//! The pipeline buffers a bounded head of the stream and scores it against the
//! signatures below; the highest score above [`MIN_SCORE`] wins.

use once_cell::sync::Lazy;
use regex::Regex;

/// Recognized input shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatType {
    Pytest,
    CargoCheck,
    GitDiff,
    DockerLog,
    JsonLines,
    #[default]
    GenericLog,
}

impl FormatType {
    /// Stable machine-readable name, used in output tags and stats.
    pub fn label(self) -> &'static str {
        match self {
            FormatType::Pytest => "pytest",
            FormatType::CargoCheck => "cargo",
            FormatType::GitDiff => "git-diff",
            FormatType::DockerLog => "docker-log",
            FormatType::JsonLines => "json-lines",
            FormatType::GenericLog => "generic-log",
        }
    }

    /// Fence language for the Markdown formatter.
    pub fn fence_language(self) -> &'static str {
        match self {
            FormatType::GitDiff => "diff",
            FormatType::JsonLines => "json",
            _ => "text",
        }
    }
}

/// Below this score nothing is confident enough to beat [`FormatType::GenericLog`].
const MIN_SCORE: u32 = 4;

struct Signature {
    format: FormatType,
    weight: u32,
    re: Regex,
}

static SIGNATURES: Lazy<Vec<Signature>> = Lazy::new(|| {
    let sig = |format, weight, pattern: &str| Signature {
        format,
        weight,
        re: Regex::new(pattern).expect("static detector regex is valid"),
    };
    vec![
        sig(FormatType::GitDiff, 6, r"^diff --git "),
        sig(
            FormatType::GitDiff,
            3,
            r"^@@ -\d+(?:,\d+)? \+\d+(?:,\d+)? @@",
        ),
        sig(FormatType::GitDiff, 2, r"^--- a/"),
        sig(
            FormatType::GitDiff,
            2,
            r"^index [0-9a-f]{7,}\.\.[0-9a-f]{7,}",
        ),
        sig(
            FormatType::Pytest,
            6,
            r"^=+ (FAILURES|ERRORS|short test summary info) =+",
        ),
        sig(FormatType::Pytest, 4, r"^collected \d+ items?"),
        sig(FormatType::Pytest, 3, r"^(FAILED|PASSED|ERROR) \S+::"),
        sig(FormatType::Pytest, 2, r"^E\s{2,}\w"),
        sig(FormatType::Pytest, 2, r"=+ \d+ (failed|passed)"),
        sig(FormatType::CargoCheck, 6, r"^error\[E\d{4}\]:"),
        sig(FormatType::CargoCheck, 4, r"^\s+--> \S+:\d+:\d+"),
        sig(
            FormatType::CargoCheck,
            3,
            r"^\s*(Compiling|Checking|Finished) \S+",
        ),
        sig(
            FormatType::CargoCheck,
            2,
            r"^(warning|error)(\[[^\]]+\])?: ",
        ),
        sig(FormatType::DockerLog, 3, r"^[a-zA-Z0-9_.-]+\s+\| "),
        sig(
            FormatType::DockerLog,
            2,
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?\s",
        ),
        sig(
            FormatType::DockerLog,
            2,
            r#"level=(info|warn|warning|error|debug)"#,
        ),
        sig(FormatType::JsonLines, 2, r#"^\s*\{.*"\w+"\s*:.*\}\s*$"#),
    ]
});

/// Score `lines` and return the most likely format.
pub fn detect<S: AsRef<str>>(lines: &[S]) -> FormatType {
    let mut scores = [0u32; 6];
    for line in lines {
        let line = line.as_ref();
        if line.is_empty() {
            continue;
        }
        for sig in SIGNATURES.iter() {
            if sig.re.is_match(line) {
                scores[index(sig.format)] += sig.weight;
            }
        }
    }
    let (best_idx, best) = scores
        .iter()
        .enumerate()
        .max_by_key(|(_, score)| **score)
        .expect("fixed-size score table");
    if *best < MIN_SCORE {
        return FormatType::GenericLog;
    }
    from_index(best_idx)
}

fn index(format: FormatType) -> usize {
    match format {
        FormatType::Pytest => 0,
        FormatType::CargoCheck => 1,
        FormatType::GitDiff => 2,
        FormatType::DockerLog => 3,
        FormatType::JsonLines => 4,
        FormatType::GenericLog => 5,
    }
}

fn from_index(idx: usize) -> FormatType {
    match idx {
        0 => FormatType::Pytest,
        1 => FormatType::CargoCheck,
        2 => FormatType::GitDiff,
        3 => FormatType::DockerLog,
        4 => FormatType::JsonLines,
        _ => FormatType::GenericLog,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_str(input: &str) -> FormatType {
        let lines: Vec<&str> = input.lines().collect();
        detect(&lines)
    }

    #[test]
    fn detects_git_diff() {
        let input = "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..89abcde 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@";
        assert_eq!(detect_str(input), FormatType::GitDiff);
    }

    #[test]
    fn detects_pytest() {
        let input = "collected 42 items\n\ntests/test_api.py .......F\n=================================== FAILURES ===================================\nE   AssertionError: assert 4 == 5";
        assert_eq!(detect_str(input), FormatType::Pytest);
    }

    #[test]
    fn detects_cargo() {
        let input =
            "   Compiling ctrim v0.1.0\nerror[E0308]: mismatched types\n  --> src/main.rs:10:5";
        assert_eq!(detect_str(input), FormatType::CargoCheck);
    }

    #[test]
    fn detects_docker_compose_logs() {
        let input = "api_1  | 2024-05-01T10:00:00Z level=info msg=started\napi_1  | 2024-05-01T10:00:01Z level=info msg=ready";
        assert_eq!(detect_str(input), FormatType::DockerLog);
    }

    #[test]
    fn detects_json_lines() {
        let input = "{\"level\":\"info\",\"msg\":\"a\"}\n{\"level\":\"info\",\"msg\":\"b\"}";
        assert_eq!(detect_str(input), FormatType::JsonLines);
    }

    #[test]
    fn unremarkable_text_is_generic() {
        assert_eq!(
            detect_str("hello world\nsecond line"),
            FormatType::GenericLog
        );
    }

    #[test]
    fn empty_input_is_generic() {
        let empty: [&str; 0] = [];
        assert_eq!(detect(&empty), FormatType::GenericLog);
    }
}
