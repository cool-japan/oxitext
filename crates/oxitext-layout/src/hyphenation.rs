//! Hyphenation support: soft-hyphen detection and automatic hyphenation.
//!
//! Provides two complementary break-opportunity sources:
//!
//! - [`soft_hyphen_breaks`] scans the input text for U+00AD SOFT HYPHEN and
//!   returns the byte offset *after* each soft-hyphen character (i.e. the start
//!   of the next character). This offset is where the line may be broken, which
//!   is the same convention used by `unicode-linebreak` and the layout engine's
//!   internal break-opportunity list.
//!
//! - [`automatic_hyphen_breaks`] (behind the `hyphenation` feature) uses TeX
//!   hyphenation patterns via the [`hypher`] crate to find additional break
//!   opportunities within words for the given language.

/// Scan `text` for U+00AD SOFT HYPHEN characters and return the byte offset
/// *after* each one.
///
/// The returned offsets follow the same convention used by the `unicode-linebreak`
/// crate and the layout engine's break-opportunity list: each value is the byte
/// index of the first character of the next segment — i.e. the position at
/// which a line break may be inserted.  (Soft hyphens are invisible but signal
/// a legal hyphenation point; the rendering layer is responsible for drawing a
/// visible hyphen glyph at the break.)
///
/// # Examples
/// ```
/// use oxitext_layout::soft_hyphen_breaks;
/// // "ma·chine" with a soft hyphen between 'a' and 'c'
/// let breaks = soft_hyphen_breaks("ma\u{00AD}chine");
/// assert_eq!(breaks, vec![4]); // byte 4 is 'c' (soft hyphen is 2 bytes)
///
/// assert!(soft_hyphen_breaks("no hyphens").is_empty());
/// ```
pub fn soft_hyphen_breaks(text: &str) -> Vec<usize> {
    text.char_indices()
        .filter_map(|(i, c)| {
            if c == '\u{00AD}' {
                // "after" convention: offset of the char following the soft hyphen
                Some(i + c.len_utf8())
            } else {
                None
            }
        })
        .collect()
}

/// Find automatic hyphenation break opportunities using TeX patterns via
/// the [`hypher`] crate.
///
/// Iterates over whitespace-delimited words in `text`, hyphenates each word
/// with `hypher::hyphenate`, and returns the byte offsets *after* each
/// hyphenation point (relative to the start of `text`).
///
/// The offsets follow the same "after" convention as [`soft_hyphen_breaks`]
/// and the `unicode-linebreak` output.
///
/// Only ASCII-whitespace word boundaries are considered; punctuation attached
/// to words is included in the word passed to the hyphenator.
///
/// # Examples
/// ```
/// use oxitext_layout::hyphenation::automatic_hyphen_breaks;
/// use hypher::Lang;
/// let breaks = automatic_hyphen_breaks("machine", Lang::English);
/// assert!(!breaks.is_empty());
/// ```
#[cfg(feature = "hyphenation")]
pub fn automatic_hyphen_breaks(text: &str, lang: hypher::Lang) -> Vec<usize> {
    let mut result = Vec::new();

    // Iterate over words (split on ASCII whitespace), preserving byte offsets.
    let mut remaining = text;
    let mut base_offset = 0usize;

    loop {
        // Skip leading whitespace
        let trimmed = remaining.trim_start_matches(|c: char| c.is_ascii_whitespace());
        let skipped = remaining.len() - trimmed.len();
        base_offset += skipped;
        remaining = trimmed;

        if remaining.is_empty() {
            break;
        }

        // Find the end of the current word
        let word_len = remaining
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(remaining.len());
        let word = &remaining[..word_len];

        // Hyphenate the word and collect break points
        let syllables: Vec<&str> = hypher::hyphenate(word, lang).collect();
        let mut syl_offset = 0usize;
        for (i, syl) in syllables.iter().enumerate() {
            syl_offset += syl.len();
            // Break point after every syllable except the last
            if i + 1 < syllables.len() {
                result.push(base_offset + syl_offset);
            }
        }

        base_offset += word_len;
        remaining = &remaining[word_len..];
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_hyphen_single() {
        // "ma\u{00AD}chine": soft hyphen (2 bytes) starts at byte 2, ends at byte 4.
        // Break opportunity is at byte 4 (the 'c').
        assert_eq!(soft_hyphen_breaks("ma\u{00AD}chine"), vec![4]);
    }

    #[test]
    fn soft_hyphen_none() {
        assert!(soft_hyphen_breaks("no hyphens").is_empty());
    }

    #[test]
    fn soft_hyphen_multiple() {
        // "a\u{00AD}b\u{00AD}c": bytes: a(0) SHY(1,2) b(3) SHY(4,5) c(6)
        // After-convention offsets: 3, 6
        let breaks = soft_hyphen_breaks("a\u{00AD}b\u{00AD}c");
        assert_eq!(breaks, vec![3, 6]);
    }

    #[test]
    fn soft_hyphen_at_start() {
        // "\u{00AD}abc": soft hyphen at byte 0 (2 bytes), 'a' at byte 2
        let breaks = soft_hyphen_breaks("\u{00AD}abc");
        assert_eq!(breaks, vec![2]);
    }

    #[test]
    fn soft_hyphen_consecutive() {
        // "a\u{00AD}\u{00AD}b": two consecutive soft hyphens
        // After 1st SHY: byte 3, after 2nd SHY: byte 5
        let breaks = soft_hyphen_breaks("a\u{00AD}\u{00AD}b");
        assert_eq!(breaks, vec![3, 5]);
    }

    #[cfg(feature = "hyphenation")]
    mod hyphenation_feature {
        use super::*;
        use hypher::Lang;

        #[test]
        fn automatic_breaks_machine() {
            let breaks = automatic_hyphen_breaks("machine", Lang::English);
            // "machine" -> ["ma", "chine"], break at byte 2
            assert_eq!(breaks, vec![2]);
        }

        #[test]
        fn automatic_breaks_empty() {
            let breaks = automatic_hyphen_breaks("", Lang::English);
            assert!(breaks.is_empty());
        }

        #[test]
        fn automatic_breaks_short_word() {
            // Very short words may not hyphenate
            let breaks = automatic_hyphen_breaks("I", Lang::English);
            // Either empty or single break — just check it doesn't panic
            let _ = breaks;
        }

        #[test]
        fn automatic_breaks_sentence() {
            // Multiple words: break points are relative to the full string
            let breaks = automatic_hyphen_breaks("hyphenation machine", Lang::English);
            // Verify all offsets are within string bounds
            for &b in &breaks {
                assert!(b <= "hyphenation machine".len(), "break {b} out of bounds");
            }
            // Should have at least one break (hyphenation has multiple syllables)
            assert!(!breaks.is_empty());
        }
    }
}
