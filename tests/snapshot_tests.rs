//! Golden output for every fixture and format.
//!
//! These are the regression net for heuristic tuning: review changes with
//! `cargo insta review` (or `INSTA_UPDATE=always cargo test` to accept all).

use ctrim::{Config, OutputFormat};

const FIXTURES: &[&str] = &[
    "cargo_check.log",
    "docker_retry.log",
    "git_diff.log",
    "node_stacktrace.log",
    "pytest_failure.log",
];

fn compress(fixture: &str, format: OutputFormat) -> String {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let input = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    ctrim::process(
        &input,
        &Config {
            format,
            ..Config::default()
        },
    )
    .content
}

#[test]
fn raw_output_is_stable() {
    for fixture in FIXTURES {
        insta::assert_snapshot!(
            format!("raw__{fixture}"),
            compress(fixture, OutputFormat::Raw)
        );
    }
}

#[test]
fn markdown_output_is_stable() {
    for fixture in FIXTURES {
        insta::assert_snapshot!(
            format!("markdown__{fixture}"),
            compress(fixture, OutputFormat::Markdown)
        );
    }
}

#[test]
fn xml_output_is_stable() {
    for fixture in FIXTURES {
        insta::assert_snapshot!(
            format!("xml__{fixture}"),
            compress(fixture, OutputFormat::Xml)
        );
    }
}
