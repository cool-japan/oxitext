//! UAX #9 bidi visual reordering helpers.
//!
//! Provides [`needs_bidi`] for fast RTL detection and [`line_visual_order`]
//! for the UAX #9 L2 visual reordering algorithm.

use unicode_bidi::Level;

/// Returns `true` if `text` contains any character with RTL bidi class.
///
/// Uses Unicode block ranges for a fast scan without full bidi analysis.
/// Conservatively covers Hebrew (U+0590–U+05FF), Arabic (U+0600–U+06FF),
/// Arabic Supplement (U+0750–U+077F), Arabic Presentation Forms A/B, and
/// directional marks.  False positives (triggering full L2) are acceptable;
/// false negatives (skipping L2 for RTL text) are not.
pub fn needs_bidi(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x0590..=0x05FF).contains(&cp)   // Hebrew
            || (0x0600..=0x06FF).contains(&cp) // Arabic
            || (0x0750..=0x077F).contains(&cp) // Arabic Supplement
            || (0xFB1D..=0xFB4F).contains(&cp) // Hebrew Presentation Forms
            || (0xFB50..=0xFDFF).contains(&cp) // Arabic Presentation Forms A
            || (0xFE70..=0xFEFF).contains(&cp) // Arabic Presentation Forms B
            || cp == 0x200F                    // Right-to-Left Mark
            || cp == 0x202B                    // Right-to-Left Embedding
            || cp == 0x202E // Right-to-Left Override
    })
}

/// Returns the visual permutation for a slice of UAX #9 embedding levels.
///
/// Implements UAX #9 L2: from the highest level down to the lowest odd level,
/// reverse every maximal contiguous run of characters at that level or above.
///
/// Returns indices where `result[visual_pos] = logical_index`, so iterating
/// `result` in order gives visual left-to-right rendering.
pub fn line_visual_order(levels: &[Level]) -> Vec<usize> {
    let n = levels.len();
    let mut indices: Vec<usize> = (0..n).collect();
    if n == 0 {
        return indices;
    }

    let max_level = levels.iter().map(|l| l.number()).max().unwrap_or(0);
    // Find the lowest odd level; if none exist, all text is LTR — no reordering.
    let min_odd = match levels
        .iter()
        .map(|l| l.number())
        .filter(|&l| l % 2 == 1)
        .min()
    {
        Some(l) => l,
        None => return indices,
    };

    // L2: for l from max_level down to min_odd, reverse contiguous runs >= l.
    let mut l = max_level;
    loop {
        let mut run_start: Option<usize> = None;
        for i in 0..=n {
            let at_or_above = i < n && levels[i].number() >= l;
            match (run_start, at_or_above) {
                (None, true) => run_start = Some(i),
                (Some(s), false) => {
                    indices[s..i].reverse();
                    run_start = None;
                }
                _ => {}
            }
        }
        if l <= min_odd {
            break;
        }
        l -= 1;
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels_from_u8(v: &[u8]) -> Vec<Level> {
        v.iter()
            .map(|&n| Level::new(n).expect("valid level"))
            .collect()
    }

    #[test]
    fn all_ltr_is_identity() {
        let levels = levels_from_u8(&[0, 0, 0, 0]);
        let order = line_visual_order(&levels);
        assert_eq!(
            order,
            vec![0, 1, 2, 3],
            "all-LTR must be identity permutation"
        );
    }

    #[test]
    fn all_rtl_reverses_completely() {
        let levels = levels_from_u8(&[1, 1, 1, 1]);
        let order = line_visual_order(&levels);
        assert_eq!(order, vec![3, 2, 1, 0], "all-RTL must fully reverse");
    }

    #[test]
    fn mixed_ltr_rtl_partial_reverse() {
        // levels [0, 0, 1, 1]: the RTL run at indices 2..4 reverses → [0, 1, 3, 2]
        let levels = levels_from_u8(&[0, 0, 1, 1]);
        let order = line_visual_order(&levels);
        assert_eq!(order, vec![0, 1, 3, 2]);
    }

    #[test]
    fn empty_slice_returns_empty() {
        let order = line_visual_order(&[]);
        assert!(order.is_empty(), "empty input yields empty output");
    }

    #[test]
    fn needs_bidi_ascii_false() {
        assert!(!needs_bidi("hello"), "ASCII text needs no bidi processing");
    }

    #[test]
    fn needs_bidi_arabic_true() {
        // U+0645 ARABIC LETTER MEEM is within 0x0600–0x06FF
        assert!(needs_bidi("مرحبا"), "Arabic text must trigger bidi");
    }

    #[test]
    fn needs_bidi_hebrew_true() {
        // U+05D0 HEBREW LETTER ALEF is within 0x0590–0x05FF
        assert!(needs_bidi("שלום"), "Hebrew text must trigger bidi");
    }
}
