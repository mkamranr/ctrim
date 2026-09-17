//! Reduction accounting and the one-line STDERR report.

use crate::detector::FormatType;
use crate::token;

/// Before/after sizes for one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReductionStats {
    pub raw_bytes: usize,
    pub reduced_bytes: usize,
    pub raw_tokens: usize,
    pub reduced_tokens: usize,
    pub raw_lines: usize,
    pub reduced_lines: usize,
}

impl ReductionStats {
    /// Percentage of tokens removed. Zero-input runs report no reduction.
    pub fn reduction_percentage(&self) -> f64 {
        if self.raw_tokens == 0 {
            return 0.0;
        }
        let saved = self.raw_tokens as f64 - self.reduced_tokens as f64;
        saved / self.raw_tokens as f64 * 100.0
    }

    /// `[ctrim] pytest: 14,200 -> 2,150 tokens (-84.8%) | copied to clipboard`
    pub fn report(&self, format: FormatType, clipboard: bool) -> String {
        let marker = token::approx_marker();
        let mut line = format!(
            "[ctrim] {}: {}{} -> {}{} tokens (-{:.1}%)",
            format.label(),
            marker,
            thousands(self.raw_tokens),
            marker,
            thousands(self.reduced_tokens),
            self.reduction_percentage(),
        );
        line.push_str(&format!(
            " | {} -> {} lines",
            thousands(self.raw_lines),
            thousands(self.reduced_lines)
        ));
        if clipboard {
            line.push_str(" | copied to clipboard");
        }
        line
    }
}

/// `14200` -> `14,200`
pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(14200), "14,200");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    #[test]
    fn computes_reduction_percentage() {
        let stats = ReductionStats {
            raw_tokens: 14200,
            reduced_tokens: 2150,
            ..ReductionStats::default()
        };
        assert!((stats.reduction_percentage() - 84.85).abs() < 0.01);
    }

    #[test]
    fn empty_input_reports_zero_reduction() {
        assert_eq!(ReductionStats::default().reduction_percentage(), 0.0);
    }

    #[test]
    fn report_mentions_the_clipboard_only_when_used() {
        let stats = ReductionStats {
            raw_tokens: 100,
            reduced_tokens: 20,
            raw_lines: 10,
            reduced_lines: 3,
            ..ReductionStats::default()
        };
        let plain = stats.report(FormatType::Pytest, false);
        assert!(plain.contains("pytest"));
        assert!(plain.contains("-80.0%"));
        assert!(!plain.contains("clipboard"));
        assert!(stats
            .report(FormatType::Pytest, true)
            .contains("copied to clipboard"));
    }
}
