//! Dependency-free token estimation.
//!
//! BPE vocabularies map common words to a single token, split digits into small
//! groups, and spend a token on most punctuation. Counting those three things
//! separately reproduces real tokenizer output closely, at gigabytes per
//! second and with no vocabulary to ship.
//!
//! The weights below were fitted against `o200k_base` over this repository's
//! fixture corpus and source tree: worst case 13% error, mean 7%. Refit them
//! with `cargo run --release --example calibrate --features exact-tokens`.
//!
//! Counting is done in *units* ([`SCALE`] units per token) so that a stream
//! summed line by line matches counting the whole text at once — rounding once
//! per line would drift by thousands of tokens over a large log.

/// Units per token. Weights are expressed in units to keep the hot loop integer.
pub const SCALE: usize = 100;

/// A run of letters and underscores, the shape of a vocabulary entry.
const WORD_UNITS: usize = 105;
/// Each digit, which tokenizers group far more densely than letters.
const DIGIT_UNITS: usize = 95;
/// Each ASCII punctuation character.
const PUNCT_UNITS: usize = 65;
/// Each newline.
pub const NEWLINE_UNITS: usize = 35;
/// Each non-ASCII character, which rarely shares a token with its neighbours.
pub(crate) const NON_ASCII_UNITS: usize = 100;
/// Each escape character. A colour sequence such as `ESC [ 1 m` costs about
/// one and a half tokens beyond the punctuation it contains, which is why
/// stripping ANSI saves so much on coloured build output.
const ESCAPE_UNITS: usize = 150;

/// Estimated tokens in `text`, rounded once.
pub fn count(text: &str) -> usize {
    units(text) / SCALE
}

/// Estimated tokens in `text`, in [`SCALE`] units per token.
///
/// Walks bytes rather than chars: every classification below is a pure ASCII
/// test, and a UTF-8 continuation byte can never be mistaken for one, so the
/// leading byte of a multi-byte character is charged once.
pub fn units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_word = false;
    for &byte in text.as_bytes() {
        if byte.is_ascii_alphabetic() || byte == b'_' {
            if !in_word {
                total += WORD_UNITS;
            }
            in_word = true;
            continue;
        }
        in_word = false;
        match byte {
            b'0'..=b'9' => total += DIGIT_UNITS,
            b'\n' => total += NEWLINE_UNITS,
            0x1b => total += ESCAPE_UNITS,
            // Spaces and tabs are absorbed into neighbouring tokens.
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => {}
            0x80..=0xbf => {} // UTF-8 continuation byte, already charged
            b if b.is_ascii() => total += PUNCT_UNITS,
            _ => total += NON_ASCII_UNITS,
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_has_no_tokens() {
        assert_eq!(count(""), 0);
        assert_eq!(units(""), 0);
    }

    #[test]
    fn units_are_additive_over_lines() {
        let text = "error: something failed\n  at main.rs:10\nsecond line here\n";
        let summed: usize = text.lines().map(|l| units(l) + NEWLINE_UNITS).sum();
        assert_eq!(summed, units(text));
    }

    #[test]
    fn longer_text_costs_more_tokens() {
        assert!(count("a much longer line of log output here") > count("short"));
    }

    #[test]
    fn punctuation_and_digits_cost_more_than_prose_of_equal_length() {
        let prose = "the quick brown fox jumps over the lazy dogs today";
        let dense = "a={b:1,c:[2,3]};d=(e?f:g)&&h||i;j-=k*l/m%n;o^p~q!r";
        assert_eq!(prose.len(), dense.len());
        assert!(
            count(dense) > count(prose),
            "{} vs {}",
            count(dense),
            count(prose)
        );
    }

    #[test]
    fn estimate_is_in_a_sane_range_for_prose() {
        // 51 characters of English; real tokenizers land near 11-13 tokens.
        let tokens = count("the quick brown fox jumps over the lazy dog today ok");
        assert!((8..=16).contains(&tokens), "estimated {tokens}");
    }

    #[test]
    fn escape_sequences_cost_more_than_plain_punctuation() {
        assert!(units("\u{1b}[1m") > units("x[1m"));
    }

    #[test]
    fn multibyte_characters_are_charged_once() {
        assert_eq!(units("é"), NON_ASCII_UNITS);
        assert_eq!(units("→"), NON_ASCII_UNITS);
    }

    #[test]
    fn indentation_is_free() {
        assert_eq!(units("        indented"), units("indented"));
    }
}
