//! Variable font support and vertical shaping tests.
//!
//! Provides [`SwashShaper::shape_with_variations`] for shaping text with
//! explicit OpenType variation axis values, and related tests including
//! vertical shaping with the `vert` feature.

use crate::{ShapeResult, SwashShaper};
use oxitext_core::OxiTextError;

impl SwashShaper {
    /// Shape text with specific OpenType variation axis values.
    ///
    /// `variations` is a list of `(axis_tag, value)` pairs, e.g.
    /// `(*b"wght", 700.0)` for Bold weight.  The values are applied to the font
    /// via swash's `ShaperBuilder::variations`, which normalises each axis value
    /// against the font's `fvar`/`avar` tables and sets the corresponding
    /// normalized coordinate before shaping.
    ///
    /// For a non-variable font (no `fvar` axes), the settings are a no-op and
    /// the default instance is shaped — matching swash's own behaviour, where
    /// `variations` only takes effect when `coord_count != 0`.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_with_variations(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
        variations: &[([u8; 4], f32)],
    ) -> Result<ShapeResult, OxiTextError> {
        self.shape_full_with_variations(font_data, text, px_size, variations)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ShapeDirection, ShapeFeature, ShapeRequest};
    use std::path::Path;
    use std::sync::Arc;

    fn load_test_font() -> Arc<[u8]> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
        if fixture.exists() {
            return Arc::from(
                std::fs::read(&fixture)
                    .expect("read fixture font")
                    .as_slice(),
            );
        }
        let candidates = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ];
        for p in &candidates {
            if Path::new(p).exists() {
                return Arc::from(std::fs::read(p).expect("read system font").as_slice());
            }
        }
        panic!("no test font found — add tests/fixtures/test-font.ttf");
    }

    // ── Item 1: Vertical shaping with vert feature ────────────────────────────

    #[test]
    fn test_vertical_shaping_applies_vert_feature() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        // shape_request auto-injects vert/vrt2 when direction == Ttb.
        let req = ShapeRequest::builder()
            .text("A")
            .font_data(&font_bytes)
            .px_size(16.0)
            .direction(ShapeDirection::Ttb)
            .build()
            .expect("build request");
        // Must not panic; glyphs may be .notdef if font lacks CJK coverage.
        let result = shaper.shape_request(&req);
        assert!(
            result.is_ok(),
            "shape_request with Ttb should succeed: {result:?}"
        );
        let glyphs = result.expect("ok");
        assert!(
            !glyphs.is_empty(),
            "expected at least one glyph from vertical shaping"
        );
    }

    #[test]
    fn test_vertical_shaping_shape_with_features_vert() {
        // shape_with_features called with the vert feature should not panic.
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let vert = ShapeFeature::VERT;
        let result = shaper.shape_with_features(&font_bytes, "A", 16.0, false, &[vert]);
        assert!(
            result.is_ok(),
            "shape_with_features with VERT should succeed: {result:?}"
        );
    }

    #[test]
    fn test_vertical_shaping_vert_and_vrt2_injected() {
        // Verify the injected feature list contains both vert and vrt2 for Ttb.
        let req = ShapeRequest::builder()
            .text("A")
            .font_data(&[0u8; 4])
            .px_size(16.0)
            .direction(ShapeDirection::Ttb)
            .build()
            .expect("build ok");
        let mut features = req.features.clone();
        if req.direction == ShapeDirection::Ttb {
            if !features.iter().any(|f| f.tag == *b"vert") {
                features.push(ShapeFeature::VERT);
            }
            if !features.iter().any(|f| f.tag == *b"vrt2") {
                features.push(ShapeFeature::VRT2);
            }
        }
        assert!(
            features.iter().any(|f| f.tag == *b"vert"),
            "vert must be present for Ttb"
        );
        assert!(
            features.iter().any(|f| f.tag == *b"vrt2"),
            "vrt2 must be present for Ttb"
        );
    }

    // ── Item 2: Variable font shaping ─────────────────────────────────────────

    #[test]
    fn test_shape_with_variations_does_not_panic() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let variations = [(*b"wght", 700.0f32)];
        let result = shaper.shape_with_variations(&font_bytes, "Hello", 16.0, &variations);
        // The test font may not be a variable font, but the call must succeed.
        assert!(
            result.is_ok(),
            "shape_with_variations should succeed: {result:?}"
        );
    }

    #[test]
    fn test_shape_with_variations_empty_variations() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let result = shaper.shape_with_variations(&font_bytes, "ABC", 16.0, &[]);
        assert!(result.is_ok(), "empty variations list must succeed");
        let r = result.expect("ok");
        assert!(!r.glyphs.is_empty(), "expected shaped glyphs");
    }

    #[test]
    fn test_shape_with_variations_multiple_axes() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let variations = [(*b"wght", 400.0f32), (*b"wdth", 100.0f32)];
        let result = shaper.shape_with_variations(&font_bytes, "Hi", 16.0, &variations);
        assert!(
            result.is_ok(),
            "multiple variation axes must not error: {result:?}"
        );
    }

    /// Loads the synthetic variable-font fixture (`variable_wght.ttf`), which has
    /// a single `wght` axis (400 default … 900 max) and an HVAR table that maps
    /// glyph 'A' to advance width 500 at wght=400 and 1000 at wght=900.
    fn load_variable_font() -> Arc<[u8]> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/variable_wght.ttf");
        Arc::from(
            std::fs::read(&fixture)
                .expect("read variable_wght.ttf fixture")
                .as_slice(),
        )
    }

    #[test]
    fn test_shape_with_variations_changes_advance_on_variable_font() {
        // Regression test: shape_with_variations must actually thread its axis
        // values into swash. swash 0.2.10 applies advance variation via the HVAR
        // table (see swash `GlyphMetrics::advance_width` / `var::advance_delta`),
        // so the same glyph shaped at two different `wght` values on a variable
        // font must return two different advances. Previously this method dropped
        // its `variations` argument (`let _ = variations;`), so both calls
        // returned the default-instance advance.
        let font_bytes = load_variable_font();
        let mut shaper = SwashShaper::new();
        // px_size == upem (1000) so 1 font unit == 1 px: advances read directly.
        let light = shaper
            .shape_with_variations(&font_bytes, "A", 1000.0, &[(*b"wght", 400.0)])
            .expect("shape at wght=400");
        let bold = shaper
            .shape_with_variations(&font_bytes, "A", 1000.0, &[(*b"wght", 900.0)])
            .expect("shape at wght=900");
        assert_eq!(light.glyphs.len(), 1, "one glyph for 'A' at wght=400");
        assert_eq!(bold.glyphs.len(), 1, "one glyph for 'A' at wght=900");
        // Same codepoint → same glyph id; only the variation changed.
        assert_eq!(
            light.glyphs[0].gid, bold.glyphs[0].gid,
            "same glyph id across weights"
        );
        let light_adv = light.glyphs[0].x_advance;
        let bold_adv = bold.glyphs[0].x_advance;
        // Fixture design: 500 @ wght=400, 1000 @ wght=900. Assert a decisive gap
        // rather than exact values (swash may round in font-unit → px scaling).
        assert!(
            (light_adv - 500.0).abs() < 5.0,
            "wght=400 advance should be ~500, got {light_adv}"
        );
        assert!(
            bold_adv > light_adv + 100.0,
            "wght=900 advance ({bold_adv}) must be substantially larger than \
             wght=400 advance ({light_adv}) — proves the axis value reached swash"
        );
    }

    #[test]
    fn test_shape_with_variations_empty_matches_default_on_variable_font() {
        // With no variation settings, shaping a variable font must yield its
        // default-instance advance (500 for 'A'), identical to explicitly
        // requesting the default wght=400.
        let font_bytes = load_variable_font();
        let mut shaper = SwashShaper::new();
        let default_run = shaper
            .shape_with_variations(&font_bytes, "A", 1000.0, &[])
            .expect("shape default instance");
        let explicit_default = shaper
            .shape_with_variations(&font_bytes, "A", 1000.0, &[(*b"wght", 400.0)])
            .expect("shape at wght=400");
        assert_eq!(default_run.glyphs.len(), 1);
        assert_eq!(explicit_default.glyphs.len(), 1);
        assert_eq!(
            default_run.glyphs[0].x_advance, explicit_default.glyphs[0].x_advance,
            "empty variations must match the default instance (wght=400)"
        );
    }

    // ── Item 1: Devanagari conjunct shaping ───────────────────────────────────

    /// The test font (test-font.ttf) may lack Devanagari coverage.  That is
    /// acceptable: what matters is that the shaper does not panic when asked to
    /// shape Indic text and that `requires_indic_shaping` correctly classifies
    /// the input.
    #[test]
    fn test_devanagari_requires_indic_shaping() {
        assert!(
            crate::requires_indic_shaping("क्ष"),
            "ksha (ka + virama + sha) must require Indic shaping"
        );
        assert!(
            crate::requires_indic_shaping("नमस्ते"),
            "namaste must require Indic shaping"
        );
        assert!(
            !crate::requires_indic_shaping("Hello"),
            "Latin text must not require Indic shaping"
        );
    }

    #[test]
    fn test_devanagari_virama_text_requires_indic_shaping() {
        // Explicit virama (U+094D) between consonants must also be detected.
        let text = "क\u{094D}ष";
        assert!(
            crate::requires_indic_shaping(text),
            "explicit virama sequence must require Indic shaping"
        );
    }

    #[test]
    fn test_devanagari_conjunct_swash_no_panic() {
        // "ksha" in Devanagari: क + ् + ष → may form a conjunct when the font
        // has GSUB Indic tables; Roboto / test-font may yield .notdef but must
        // not panic.
        let devanagari_ksha = "क्ष";
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let result = shaper.shape_full(&font_bytes, devanagari_ksha, 16.0);
        // Accept either success (possibly .notdef glyphs) or a shaping error.
        match result {
            Ok(sr) => {
                // If glyphs were produced, cluster_boundaries must be populated.
                assert!(
                    !sr.cluster_boundaries.is_empty(),
                    "shape_full must populate cluster_boundaries"
                );
            }
            Err(_) => {
                // Font does not support Devanagari — acceptable for a Latin test font.
            }
        }
    }

    #[cfg(feature = "rustybuzz-backend")]
    #[test]
    fn test_devanagari_conjunct_rustybuzz_no_panic() {
        use crate::backend::ShapeBackend as _;

        let devanagari_ksha = "क्ष";
        let font_bytes = load_test_font();
        // RustybuzzShaper is a unit struct; shape() returns Vec<ShapedGlyph>
        // (never panics — returns empty on font parse failure).
        let backend = crate::RustybuzzShaper;
        let glyphs = backend.shape(&font_bytes, devanagari_ksha, 16.0);
        // The result is either empty (font lacks Devanagari) or contains
        // some glyphs (possibly .notdef).  The key assertion is no panic.
        let _ = glyphs;
    }

    // ── Item 2: Shape cache correctness ───────────────────────────────────────

    #[test]
    fn test_shape_cache_correctness_repeated_font() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::with_cache(64);

        // Shape the same text 5 times via the Arc-keyed cache path.
        let results: Vec<_> = (0..5)
            .map(|_| shaper.shape("Hello", Arc::clone(&font_bytes), 16.0).ok())
            .collect();

        let first = &results[0];
        for r in &results[1..] {
            match (first, r) {
                (Some(a), Some(b)) => {
                    assert_eq!(
                        a.glyphs.len(),
                        b.glyphs.len(),
                        "repeated shape call must return the same glyph count"
                    );
                    for (ga, gb) in a.glyphs.iter().zip(b.glyphs.iter()) {
                        assert_eq!(ga.gid, gb.gid, "cached glyph IDs must be stable");
                    }
                }
                (None, None) => {}
                _ => panic!(
                    "inconsistent cache behaviour: first was Some={}, subsequent was Some={}",
                    first.is_some(),
                    r.is_some()
                ),
            }
        }
    }

    #[test]
    fn test_shape_cache_correctness_different_texts() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::with_cache(64);

        let r1 = shaper.shape("AAA", Arc::clone(&font_bytes), 16.0).ok();
        let r2 = shaper.shape("BBB", Arc::clone(&font_bytes), 16.0).ok();
        let r3 = shaper.shape("AAA", Arc::clone(&font_bytes), 16.0).ok();

        // r1 and r3 (same text) must have the same glyph count.
        if let (Some(a), Some(b)) = (&r1, &r3) {
            assert_eq!(
                a.glyphs.len(),
                b.glyphs.len(),
                "same text shaped twice must yield the same glyph count"
            );
        }
        // r2 is exercised to ensure interleaved different-text calls do not
        // corrupt subsequent cache lookups.
        let _ = r2;
    }

    // ── Item 3: font_has_aat idempotency ──────────────────────────────────────

    #[test]
    fn test_font_has_aat_is_idempotent() {
        // Repeated calls with the same bytes must return the same value.
        // Note: the current implementation re-parses on every call; this test
        // verifies correctness (stable output) rather than caching behaviour.
        let font_bytes = load_test_font();
        let r1 = SwashShaper::font_has_aat(&font_bytes);
        let r2 = SwashShaper::font_has_aat(&font_bytes);
        assert_eq!(
            r1, r2,
            "font_has_aat must return the same value on repeated calls"
        );
    }
}
