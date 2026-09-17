//! Token counting.
//!
//! The default counter is a dependency-free estimator ([`heuristic`]). Building
//! with the `exact-tokens` feature swaps in a real BPE tokenizer ([`bpe`]) at
//! the cost of roughly 4MB of binary and a slower first call.

pub mod heuristic;

#[cfg(feature = "exact-tokens")]
pub mod bpe;

/// Tokens in `text`.
pub fn count(text: &str) -> usize {
    #[cfg(feature = "exact-tokens")]
    {
        bpe::count(text)
    }
    #[cfg(not(feature = "exact-tokens"))]
    {
        heuristic::count(text)
    }
}

/// Accumulates a token count over a stream of lines.
///
/// Counting in fixed-point units and rounding once at the end keeps a line-by-
/// line total identical to counting the whole text in one call.
#[derive(Debug, Default, Clone)]
pub struct Counter {
    units: usize,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one line of text, including the newline that terminates it.
    pub fn add_line(&mut self, line: &str) {
        #[cfg(feature = "exact-tokens")]
        {
            self.units += (bpe::count(line) + 1) * heuristic::SCALE;
        }
        #[cfg(not(feature = "exact-tokens"))]
        {
            self.units += heuristic::units(line) + heuristic::NEWLINE_UNITS;
        }
    }

    /// The accumulated token count.
    pub fn tokens(&self) -> usize {
        self.units / heuristic::SCALE
    }
}

/// Whether counts are exact BPE counts rather than estimates.
pub fn is_exact() -> bool {
    cfg!(feature = "exact-tokens")
}

/// `~` when counts are estimated, empty when they are exact, so reported
/// numbers never overstate their own precision.
pub fn approx_marker() -> &'static str {
    if is_exact() {
        ""
    } else {
        "~"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_matches_whole_text_counting() {
        let lines = [
            "first line of output",
            "second: line = 42",
            "third [line] here",
        ];
        let mut counter = Counter::new();
        for line in lines {
            counter.add_line(line);
        }
        let whole = format!("{}\n", lines.join("\n"));
        let difference = counter.tokens().abs_diff(count(&whole));
        assert!(difference <= 1, "{} vs {}", counter.tokens(), count(&whole));
    }

    #[test]
    fn empty_counter_reports_zero() {
        assert_eq!(Counter::new().tokens(), 0);
    }
}
