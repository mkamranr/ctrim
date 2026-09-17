//! Guards the reduction numbers published in the README, and the content that
//! must survive compression. If a heuristic change moves these, the README
//! table moves with it.

use ctrim::{Config, OutputFormat, Preset};

fn compress(fixture: &str, config: &Config) -> ctrim::ProcessedOutput {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let input = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    ctrim::process(&input, config)
}

fn raw() -> Config {
    Config {
        format: OutputFormat::Raw,
        ..Config::default()
    }
}

/// (fixture, minimum token reduction in percent)
const BANDS: &[(&str, f64)] = &[
    ("cargo_check.log", 55.0),
    ("docker_retry.log", 88.0),
    ("git_diff.log", 68.0),
    ("node_stacktrace.log", 35.0),
    ("pytest_failure.log", 27.0),
];

#[test]
fn every_fixture_meets_its_reduction_floor() {
    for (fixture, floor) in BANDS {
        let out = compress(fixture, &raw());
        let actual = out.stats.reduction_percentage();
        assert!(
            actual >= *floor,
            "{fixture}: reduced {actual:.1}%, floor is {floor:.1}%"
        );
    }
}

#[test]
fn compression_never_grows_the_input() {
    for (fixture, _) in BANDS {
        let out = compress(fixture, &raw());
        assert!(
            out.stats.reduced_tokens <= out.stats.raw_tokens,
            "{fixture} grew: {} -> {} tokens",
            out.stats.raw_tokens,
            out.stats.reduced_tokens
        );
    }
}

#[test]
fn pytest_keeps_the_failing_assertions_and_user_frames() {
    let out = compress("pytest_failure.log", &raw());
    for needle in [
        "E   TypeError: unsupported operand type(s) for *: 'Decimal' and 'NoneType'",
        "E   app.errors.RefundError: refund 1500 exceeds charge 1200",
        "tests/test_checkout.py:64: in test_checkout_totals",
        "app/services/pricing.py:57: in total_for",
        "2 failed, 146 passed",
    ] {
        assert!(out.content.contains(needle), "lost {needle:?}");
    }
    assert!(out.content.contains("vendor frames omitted"));
}

#[test]
fn cargo_keeps_the_error_code_and_location() {
    let out = compress("cargo_check.log", &raw());
    assert!(out.content.contains("error[E0308]: mismatched types"));
    assert!(out.content.contains("src/main.rs:6:5"));
    assert!(!out.content.contains('\x1b'), "ANSI codes survived");
}

#[test]
fn diff_keeps_source_changes_and_drops_lockfile_churn() {
    let out = compress("git_diff.log", &raw());
    assert!(out.content.contains("+    let user = authenticate(&req)?;"));
    assert!(out.content.contains("+    metrics.record(\"handled\", 1);"));
    assert!(out.content.contains("[skipped generated file Cargo.lock"));
    assert!(
        !out.content.contains("crates.io-index"),
        "lockfile body survived"
    );
    assert!(
        !out.content.contains("index 3dbd561"),
        "blob hashes survived"
    );
}

#[test]
fn docker_keeps_the_error_after_folding_the_retry_storm() {
    let out = compress("docker_retry.log", &raw());
    assert!(out.content.contains("redis unavailable after 120 attempts"));
    // The storm is broken up by worker output, so it folds as adjacent runs
    // plus windowed repeats; between them every retry line is accounted for.
    assert!(
        out.content.contains("Repeated 20 times"),
        "got:\n{}",
        out.content
    );
    assert!(
        out.content.contains("[+100 more occurrences"),
        "got:\n{}",
        out.content
    );
    assert!(out.content.contains("starting api server"));
}

#[test]
fn node_keeps_user_frames_and_folds_node_modules() {
    let out = compress("node_stacktrace.log", &raw());
    assert!(out
        .content
        .contains("TypeError: Cannot read properties of undefined"));
    assert!(out.content.contains("src/checkout/tax.ts:22:31"));
    assert!(out.content.contains("vendor frames omitted"));
}

#[test]
fn dedupe_only_leaves_stack_traces_intact() {
    let config = Config {
        dedupe_only: true,
        ..raw()
    };
    let out = compress("pytest_failure.log", &config);
    assert!(!out.content.contains("vendor frames omitted"));
    assert!(out
        .content
        .contains("site-packages/httpx/_client.py:1145: in post"));
}

#[test]
fn keep_frames_zero_removes_every_vendor_frame() {
    let config = Config {
        keep_frames: 0,
        preset: Preset::Pytest,
        ..raw()
    };
    let out = compress("pytest_failure.log", &config);
    let vendor_frames = out
        .content
        .lines()
        .filter(|l| l.contains("site-packages") && l.contains(": in "))
        .count();
    assert_eq!(vendor_frames, 0, "got:\n{}", out.content);
    assert!(out.content.contains("E   TypeError"));
}
