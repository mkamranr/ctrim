//! End-to-end CLI behaviour.

use std::io::Write;
use std::process::{Command, Stdio};

use assert_cmd::prelude::*;
use predicates::prelude::*;

fn ctrim() -> Command {
    Command::cargo_bin("ctrim").expect("binary builds")
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Pipe `input` into ctrim with `args` and return (stdout, stderr).
fn pipe(args: &[&str], input: &str) -> (String, String) {
    let mut child = ctrim()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ctrim");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "ctrim failed: {:?}", out.status);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn reads_from_stdin_and_reports_to_stderr() {
    let input = "\u{1b}[31mFAILED\u{1b}[0m\nsame repeated line of output here\nsame repeated line of output here\n";
    let (stdout, stderr) = pipe(&["--format", "raw"], input);
    assert_eq!(
        stdout,
        "FAILED\n[Repeated 2 times: \"same repeated line of output here\"]\n"
    );
    assert!(stderr.starts_with("[ctrim] "), "got {stderr:?}");
    assert!(stderr.contains("tokens"));
}

#[test]
fn reads_from_a_file_argument() {
    ctrim()
        .arg(fixture("git_diff.log"))
        .arg("--format")
        .arg("raw")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[skipped generated file Cargo.lock",
        ));
}

#[test]
fn quiet_suppresses_the_summary() {
    let (_, stderr) = pipe(&["--quiet"], "hello\n");
    assert_eq!(stderr, "");
}

#[test]
fn markdown_is_the_default_format() {
    let (stdout, _) = pipe(&[], "hello\n");
    assert_eq!(stdout, "```text\nhello\n```\n");
}

#[test]
fn xml_format_wraps_content_in_context_tags() {
    let (stdout, _) = pipe(&["--format", "xml"], "hello\n");
    assert_eq!(
        stdout,
        "<context type=\"generic-log\">\nhello\n</context>\n"
    );
}

#[test]
fn preset_forces_a_parser() {
    let (stdout, stderr) = pipe(&["--preset", "docker", "--format", "raw"], "hello\n");
    assert_eq!(stdout, "hello\n");
    assert!(stderr.contains("docker-log"), "got {stderr:?}");
}

#[test]
fn out_writes_a_file_and_leaves_stdout_empty() {
    let dir = std::env::temp_dir().join(format!("ctrim-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let target = dir.join("reduced.md");
    let (stdout, _) = pipe(
        &["--format", "raw", "--out", target.to_str().unwrap()],
        "hello\n",
    );
    assert_eq!(stdout, "");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn keep_frames_flag_changes_how_much_is_folded() {
    let trace = "Traceback (most recent call last):\n  File \"app/a.py\", line 1, in a\n    b()\n  File \"/srv/.venv/lib/python3.11/site-packages/x/1.py\", line 1, in b\n    c()\n  File \"/srv/.venv/lib/python3.11/site-packages/x/2.py\", line 2, in c\n    d()\n  File \"/srv/.venv/lib/python3.11/site-packages/x/3.py\", line 3, in d\n    e()\n  File \"/srv/.venv/lib/python3.11/site-packages/x/4.py\", line 4, in e\n    raise ValueError(1)\nValueError: 1\n";
    let (kept, _) = pipe(&["--format", "raw", "--keep-frames", "3"], trace);
    let (folded, _) = pipe(&["--format", "raw", "--keep-frames", "0"], trace);
    assert!(
        kept.contains("[... 1 vendor frames omitted ...]"),
        "got {kept}"
    );
    assert!(
        folded.contains("[... 4 vendor frames omitted ...]"),
        "got {folded}"
    );
    assert!(folded.contains("ValueError: 1"));
}

#[test]
fn preserve_ansi_keeps_escape_codes() {
    let (stdout, _) = pipe(
        &["--format", "raw", "--preserve-ansi"],
        "\u{1b}[31mred\u{1b}[0m\n",
    );
    assert!(stdout.contains('\u{1b}'));
}

#[test]
fn missing_file_exits_with_code_two() {
    ctrim()
        .arg("/nonexistent/path/to.log")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("cannot read"));
}

#[test]
fn empty_input_is_not_an_error() {
    let (stdout, _) = pipe(&["--format", "raw"], "");
    assert_eq!(stdout, "");
}

#[test]
fn help_lists_the_documented_flags() {
    ctrim()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--max-tokens").not())
        .stdout(predicate::str::contains("--keep-frames"))
        .stdout(predicate::str::contains("--dedupe-only"))
        .stdout(predicate::str::contains("--clip"));
}
