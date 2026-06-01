//! Vertical text orientation utilities — basic UAX #50 subset.
//!
//! Determines whether a Unicode codepoint should be rendered *upright*
//! (CJK-style, keeping its standard orientation) or *rotated 90° clockwise*
//! when used in top-to-bottom vertical text.

/// Returns the scaled vertical advance for `glyph_id` from the font's `vmtx`
/// table.  Falls back to `em_size` if the face cannot be parsed, the `vmtx`
/// table is absent, or the glyph has no explicit vertical advance.
pub fn vmtx_advance_for_glyph(face_data: &[u8], glyph_id: u16, em_size: f32) -> f32 {
    if face_data.is_empty() || em_size <= 0.0 {
        return em_size;
    }
    let face = match ttf_parser::Face::parse(face_data, 0) {
        Ok(f) => f,
        Err(_) => return em_size,
    };
    let units_per_em = face.units_per_em();
    if units_per_em == 0 {
        return em_size;
    }
    match face.glyph_ver_advance(ttf_parser::GlyphId(glyph_id)) {
        Some(adv) => adv as f32 * em_size / units_per_em as f32,
        None => em_size,
    }
}

/// Returns `true` if `c` should be drawn upright in vertical text.
///
/// Returns `false` if the character should be rotated 90° clockwise (the
/// typical behaviour for Latin letters, digits, and punctuation).
///
/// The classification covers the most common CJK and related scripts as
/// defined in Unicode UAX #50 "Orientation" property = "U" (Upright).
pub fn is_upright_in_vertical(c: char) -> bool {
    let cp = c as u32;
    // CJK Unified Ideographs (BMP)
    (0x4E00..=0x9FFF).contains(&cp)
        // CJK Extension A
        || (0x3400..=0x4DBF).contains(&cp)
        // CJK Extension B (SMP)
        || (0x2_0000..=0x2_A6DF).contains(&cp)
        // Hiragana
        || (0x3040..=0x309F).contains(&cp)
        // Katakana (full-width; half-width Katakana U+FF65–FF9F are rotated)
        || (0x30A0..=0x30FF).contains(&cp)
        // CJK Symbols and Punctuation (mostly upright)
        || (0x3000..=0x303F).contains(&cp)
        // Enclosed CJK Letters and Months
        || (0x3200..=0x32FF).contains(&cp)
        // Fullwidth Latin / Fullwidth ASCII variants
        || (0xFF01..=0xFF60).contains(&cp)
        // Hangul Syllables
        || (0xAC00..=0xD7A3).contains(&cp)
        // Hangul Jamo
        || (0x1100..=0x11FF).contains(&cp)
        // Bopomofo
        || (0x3100..=0x312F).contains(&cp)
        // Kangxi Radicals
        || (0x2F00..=0x2FDF).contains(&cp)
}

/// Per-call cache for parsed [`ttf_parser::Face`] instances, keyed by the
/// raw pointer of the font byte slice.
///
/// `ParsedFaceCache<'a>` is a **call-scoped** object: create one at the top of
/// a vertical-layout pass and drop it at the end.  Every unique font face is
/// parsed exactly once; subsequent lookups for the same font reuse the cached
/// `Face`.
///
/// The lifetime `'a` is the borrow lifetime of the font byte slices passed to
/// [`ParsedFaceCache::vmtx_advance_or_default`].  Callers must ensure that all
/// `&'a [u8]` slices remain valid for the entire life of the cache — inside
/// `layout_vertical` this is guaranteed because the `runs` slice (and therefore
/// every `Arc<[u8]>` it contains) is borrowed for the full call duration.
pub(crate) struct ParsedFaceCache<'a> {
    /// Maps raw data pointer → `None` (parse failed) or
    /// `Some((parsed_face, units_per_em))`.
    faces: std::collections::HashMap<usize, Option<(ttf_parser::Face<'a>, u16)>>,
}

impl<'a> ParsedFaceCache<'a> {
    /// Creates an empty cache.
    pub(crate) fn new() -> Self {
        Self {
            faces: std::collections::HashMap::new(),
        }
    }

    /// Returns the scaled `vmtx` vertical advance for `glyph_id` in the face
    /// described by `face_data`, caching the parsed face by pointer identity.
    ///
    /// Falls back to `em_size` when:
    /// - `face_data` is empty or `em_size ≤ 0`,
    /// - the face cannot be parsed (cached as `None` so we don't retry),
    /// - the face has `units_per_em == 0`,
    /// - the glyph has no explicit `vmtx` entry.
    pub(crate) fn vmtx_advance_or_default(
        &mut self,
        face_data: &'a [u8],
        glyph_id: u16,
        em_size: f32,
    ) -> f32 {
        if face_data.is_empty() || em_size <= 0.0 {
            return em_size;
        }
        // Use the raw data pointer as a stable identity key.
        let key = face_data.as_ptr() as usize;
        let entry = self.faces.entry(key).or_insert_with(|| {
            let face = ttf_parser::Face::parse(face_data, 0).ok()?;
            let upem = face.units_per_em();
            if upem == 0 {
                return None;
            }
            Some((face, upem))
        });
        match entry {
            Some((face, upem)) => match face.glyph_ver_advance(ttf_parser::GlyphId(glyph_id)) {
                Some(adv) => adv as f32 * em_size / (*upem as f32),
                None => em_size,
            },
            None => em_size,
        }
    }
}

/// Vertical-layout metrics for a single glyph.
///
/// In vertical text, glyphs advance along the block (Y) axis rather than the
/// inline (X) axis.  The `advance` field reflects the block-direction advance,
/// and `upright` controls whether the glyph is drawn in its natural orientation
/// or rotated.
pub struct VerticalMetrics {
    /// Block-direction advance in the same units as `em_size`.
    /// Ideally sourced from the font's `vmtx` table; falls back to 1 em.
    pub advance: f32,
    /// `true` → draw upright (CJK-style); `false` → rotate 90° clockwise.
    pub upright: bool,
}

impl VerticalMetrics {
    /// Compute vertical metrics for character `c` at the given `em_size`.
    ///
    /// The advance defaults to `em_size` (1 em) when no `vmtx` data is
    /// available.  Use [`VerticalMetrics::for_glyph`] when font bytes are
    /// available for accurate `vmtx` advances.
    pub fn for_char(c: char, em_size: f32) -> Self {
        Self {
            advance: em_size,
            upright: is_upright_in_vertical(c),
        }
    }

    /// Compute vertical metrics for `glyph_id` using the font's `vmtx` table.
    /// Falls back to `em_size` if `vmtx` is unavailable or parsing fails.
    pub fn for_glyph(face_data: &[u8], glyph_id: u16, c: char, em_size: f32) -> Self {
        Self {
            advance: vmtx_advance_for_glyph(face_data, glyph_id, em_size),
            upright: is_upright_in_vertical(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    // ---- ParsedFaceCache tests ----

    #[test]
    fn parsed_face_cache_returns_em_size_for_empty_face() {
        let mut cache = ParsedFaceCache::new();
        let font: Arc<[u8]> = Arc::from(&[][..]);
        assert_eq!(cache.vmtx_advance_or_default(font.as_ref(), 0, 16.0), 16.0);
    }

    #[test]
    fn parsed_face_cache_returns_em_size_for_garbage_face() {
        let mut cache = ParsedFaceCache::new();
        let font: Arc<[u8]> = Arc::from(b"not a font".as_slice());
        assert_eq!(cache.vmtx_advance_or_default(font.as_ref(), 0, 16.0), 16.0);
    }

    #[test]
    fn parsed_face_cache_returns_em_size_for_zero_em() {
        let mut cache = ParsedFaceCache::new();
        let font: Arc<[u8]> = Arc::from(b"not a font".as_slice());
        // em_size == 0.0 triggers the early-exit path; face_data.is_empty() is
        // false here, but em_size <= 0.0 wins.
        assert_eq!(cache.vmtx_advance_or_default(font.as_ref(), 0, 0.0), 0.0);
    }

    #[test]
    fn parsed_face_cache_single_parse_per_face() {
        let mut cache = ParsedFaceCache::new();
        let font: Arc<[u8]> = Arc::from(b"garbage bytes".as_slice());
        // Call 100 times with the same Arc data — only one entry should be
        // created in the cache (the parse failure is cached as `None`).
        for _ in 0..100 {
            let _ = cache.vmtx_advance_or_default(font.as_ref(), 1, 16.0);
        }
        assert_eq!(
            cache.faces.len(),
            1,
            "only one cache entry per unique data pointer"
        );
    }

    #[test]
    fn parsed_face_cache_two_fonts_two_entries() {
        let mut cache = ParsedFaceCache::new();
        let font_a: Arc<[u8]> = Arc::from(b"garbage_a".as_slice());
        let font_b: Arc<[u8]> = Arc::from(b"garbage_b".as_slice());
        // Distinct Arc allocations → distinct pointer → two entries.
        let _ = cache.vmtx_advance_or_default(font_a.as_ref(), 0, 16.0);
        let _ = cache.vmtx_advance_or_default(font_b.as_ref(), 0, 16.0);
        assert_eq!(
            cache.faces.len(),
            2,
            "two distinct fonts → two cache entries"
        );
    }

    #[test]
    fn parsed_face_cache_matches_uncached_for_invalid_font() {
        let font: Arc<[u8]> = Arc::from(b"not a valid font".as_slice());
        let uncached = vmtx_advance_for_glyph(font.as_ref(), 5, 20.0);
        let mut cache = ParsedFaceCache::new();
        let cached = cache.vmtx_advance_or_default(font.as_ref(), 5, 20.0);
        assert_eq!(
            cached, uncached,
            "cached and uncached paths must agree for invalid font data"
        );
    }

    // ---- Original vertical tests ----

    #[test]
    fn cjk_ideograph_is_upright() {
        assert!(is_upright_in_vertical('日'));
        assert!(is_upright_in_vertical('語'));
    }

    #[test]
    fn latin_letter_is_rotated() {
        assert!(!is_upright_in_vertical('A'));
        assert!(!is_upright_in_vertical('z'));
    }

    /// Verifies the fallback path: when no font data is present, advance == em_size.
    #[test]
    fn vertical_metrics_advance_equals_em() {
        let vm = VerticalMetrics::for_char('日', 16.0);
        assert!((vm.advance - 16.0).abs() < f32::EPSILON);
        assert!(vm.upright);
    }

    #[test]
    fn vmtx_advance_empty_face_returns_em_size() {
        assert_eq!(vmtx_advance_for_glyph(&[], 0, 16.0), 16.0);
    }

    #[test]
    fn vmtx_advance_invalid_face_returns_em_size() {
        // Garbage bytes — face parse fails, should return em_size.
        assert_eq!(vmtx_advance_for_glyph(b"not a font", 0, 16.0), 16.0);
    }

    #[test]
    fn vmtx_advance_zero_em_size() {
        // em_size == 0.0 triggers the early return.
        assert_eq!(vmtx_advance_for_glyph(&[], 0, 0.0), 0.0);
    }

    #[test]
    fn vmtx_advance_scales_linearly_with_em() {
        // Try common font paths; skip silently if none are available.
        let candidates = [
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/test-font.ttf")
                .to_path_buf(),
            Path::new("/Library/Fonts/Arial Unicode.ttf").to_path_buf(),
            Path::new("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf").to_path_buf(),
        ];
        let font_bytes = candidates
            .iter()
            .filter(|p| p.exists())
            .find_map(|p| std::fs::read(p).ok());
        let bytes = match font_bytes {
            Some(b) => b,
            None => return, // No font available — skip this test.
        };
        // Find the first glyph id that has a vmtx advance.
        let face = match ttf_parser::Face::parse(&bytes, 0) {
            Ok(f) => f,
            Err(_) => return,
        };
        // Try glyph IDs 1..=100 until we find one with a vmtx advance.
        let gid = (1u16..=100).find(|&g| face.glyph_ver_advance(ttf_parser::GlyphId(g)).is_some());
        let gid = match gid {
            Some(g) => g,
            None => return, // Font has no vmtx entries — skip.
        };
        let adv16 = vmtx_advance_for_glyph(&bytes, gid, 16.0);
        let adv32 = vmtx_advance_for_glyph(&bytes, gid, 32.0);
        assert!(
            (adv32 - 2.0 * adv16).abs() < 1e-3,
            "adv at 32px should be 2× adv at 16px: adv16={adv16}, adv32={adv32}"
        );
    }

    #[test]
    fn for_glyph_upright_cjk() {
        // Empty face → advance falls back to em_size; '日' is upright.
        let vm = VerticalMetrics::for_glyph(&[], 0, '日', 16.0);
        assert!(vm.upright);
        assert!((vm.advance - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn for_glyph_rotated_latin() {
        // Empty face → advance falls back to em_size; 'A' is not upright.
        let vm = VerticalMetrics::for_glyph(&[], 0, 'A', 16.0);
        assert!(!vm.upright);
        assert!((vm.advance - 16.0).abs() < f32::EPSILON);
    }
}
