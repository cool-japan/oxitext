//! Integration tests for bidi-aware Pipeline rendering.

#[cfg(feature = "pure")]
mod tests {
    use oxitext::{Pipeline, TextStyle};
    use std::path::Path;

    fn load_test_font() -> Vec<u8> {
        // 1. Project fixture (deterministic, checked in).
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
        if fixture.exists() {
            return std::fs::read(&fixture).expect("read fixture font");
        }
        // 2. Bundled Noto Sans Regular — always available, no system font required.
        oxifont_bundled::NOTO_SANS_REGULAR.to_vec()
    }

    #[test]
    fn has_rtl_detects_hebrew() {
        let font = load_test_font();
        let pipeline = Pipeline::from_bytes(&font).expect("valid font");
        // Hebrew characters are RTL.
        assert!(
            pipeline.has_rtl("AB\u{05D0}\u{05D1}"),
            "Hebrew chars should trigger has_rtl"
        );
        assert!(
            !pipeline.has_rtl("hello"),
            "Pure ASCII should not trigger has_rtl"
        );
    }

    #[test]
    fn bidi_render_does_not_panic() {
        // Test that bidi text (Latin + Hebrew) renders without panicking.
        // Font may not cover Hebrew → .notdef glyphs, but pipeline must not panic.
        let font = load_test_font();
        let mut pipeline = Pipeline::from_bytes(&font).expect("valid font");
        let style = TextStyle::default();
        let result = pipeline
            .render("AB\u{05D0}\u{05D1}", &style)
            .expect("bidi render");
        assert!(!result.glyphs.is_empty(), "bidi render must produce glyphs");
        assert!(
            !result.bitmaps.is_empty(),
            "bidi render must produce bitmaps"
        );
        assert_eq!(
            result.glyphs.len(),
            result.bitmaps.len(),
            "glyphs and bitmaps must match"
        );
        assert!(!result.lines.is_empty(), "must have at least one line");
        assert!(result.metrics.total_width > 0.0, "must have positive width");
    }

    #[test]
    fn ltr_pipeline_unchanged() {
        // Pure LTR text must produce same result as before bidi changes.
        let font = load_test_font();
        let mut pipeline = Pipeline::from_bytes(&font).expect("valid font");
        let style = TextStyle::default();
        let result = pipeline.render("Hello", &style).expect("ltr render");
        assert_eq!(result.glyphs.len(), 5, "5 chars → 5 glyphs");
        // Glyphs in visual order: x positions must be non-decreasing.
        for window in result.glyphs.windows(2) {
            assert!(
                window[1].pos.0 >= window[0].pos.0,
                "LTR x positions must be non-decreasing: {} >= {}",
                window[1].pos.0,
                window[0].pos.0
            );
        }
    }

    #[test]
    fn bidi_shape_and_layout_returns_populated_result() {
        let font = load_test_font();
        let mut pipeline = Pipeline::from_bytes(&font).expect("valid font");
        let style = TextStyle::default();
        let result = pipeline
            .shape_and_layout("AB\u{05D0}\u{05D1}", &style)
            .expect("shape_and_layout");
        assert!(!result.glyphs.is_empty());
        assert!(!result.lines.is_empty());
        assert!(result.metrics.total_width > 0.0);
        assert_eq!(result.metrics.line_count, 1);
    }
}
