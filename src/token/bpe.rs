//! Exact BPE counting via `tiktoken-rs` (feature `exact-tokens`).
//!
//! `o200k_base` is the closest public tokenizer to the models `ctrim` output is
//! usually pasted into. Several vendors keep their tokenizer private, so these
//! counts approximate those models; they are exact for OpenAI `o200k` models.

use once_cell::sync::Lazy;
use tiktoken_rs::CoreBPE;

static BPE: Lazy<CoreBPE> = Lazy::new(|| tiktoken_rs::o200k_base().expect("bundled o200k_base"));

/// Exact token count of `text` under `o200k_base`.
pub fn count(text: &str) -> usize {
    BPE.encode_ordinary(text).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_a_known_string() {
        assert!(count("hello world") <= 3);
        assert_eq!(count(""), 0);
    }

    /// Guards the accuracy claim made in `heuristic`'s documentation: the
    /// estimator stays within 15% of real BPE counts on the fixture corpus.
    #[test]
    fn heuristic_tracks_exact_on_the_fixture_corpus() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("fixtures directory") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("log") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read fixture");
            let exact = count(&text) as f64;
            let estimate = crate::token::heuristic::count(&text) as f64;
            let error = (estimate - exact).abs() / exact;
            assert!(
                error < 0.15,
                "{}: estimate {estimate} vs exact {exact} ({:.1}% off)",
                path.display(),
                error * 100.0
            );
            checked += 1;
        }
        assert!(
            checked >= 5,
            "expected the fixture corpus, checked {checked}"
        );
    }

    /// Prints the error per fixture; run with
    /// `cargo test --features exact-tokens -- --ignored --nocapture`.
    #[test]
    #[ignore = "reporting aid, not a check"]
    fn calibration_report() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
        for entry in std::fs::read_dir(dir).expect("fixtures directory") {
            let path = entry.expect("dir entry").path();
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let exact = count(&text) as f64;
            let estimate = crate::token::heuristic::count(&text) as f64;
            println!(
                "{:<28} exact {:>7.0}  estimate {:>7.0}  {:+.1}%",
                path.file_name().unwrap().to_string_lossy(),
                exact,
                estimate,
                (estimate - exact) / exact * 100.0
            );
        }
    }
}
