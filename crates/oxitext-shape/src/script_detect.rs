//! Script detection helpers for oxitext-shape.
//!
//! Provides lightweight Unicode block-range checks to determine whether text
//! contains characters from a given script family, without requiring a full
//! ICU4X or Unicode database lookup.  These functions are used internally by
//! [`crate::SwashShaper::shape_request`] to apply smart direction defaults and
//! may also be called directly by consumers.

// ──────────────────────────────────────────────────────────────────────────────
// Public helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Returns `true` if `text` contains Arabic/RTL characters requiring complex GSUB.
///
/// Covers the Arabic block (U+0600–U+06FF), Arabic Supplement (U+0750–U+077F),
/// and both Arabic Presentation Forms blocks (U+FB50–U+FDFF and U+FE70–U+FEFF).
pub fn requires_arabic_shaping(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x0600..=0x06FF).contains(&cp)
            || (0x0750..=0x077F).contains(&cp)
            || (0xFB50..=0xFDFF).contains(&cp)
            || (0xFE70..=0xFEFF).contains(&cp)
    })
}

/// Returns `true` if `text` contains Indic characters needing conjunct handling.
///
/// Covers Devanagari (U+0900–U+097F), Bengali (U+0980–U+09FF),
/// Tamil (U+0B80–U+0BFF), Telugu (U+0C00–U+0C7F), and Kannada (U+0C80–U+0CFF).
pub fn requires_indic_shaping(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x0900..=0x097F).contains(&cp)
            || (0x0980..=0x09FF).contains(&cp)
            || (0x0B80..=0x0BFF).contains(&cp)
            || (0x0C00..=0x0C7F).contains(&cp)
            || (0x0C80..=0x0CFF).contains(&cp)
    })
}

/// Returns `true` if `text` requires mark positioning (Thai, Khmer, Myanmar).
///
/// Covers Thai (U+0E00–U+0E7F), Khmer (U+1780–U+17FF), and Myanmar (U+1000–U+109F).
pub fn requires_mark_positioning(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x0E00..=0x0E7F).contains(&cp)
            || (0x1780..=0x17FF).contains(&cp)
            || (0x1000..=0x109F).contains(&cp)
    })
}
