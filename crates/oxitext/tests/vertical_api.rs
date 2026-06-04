//! Integration tests for vertical text rendering via the Pipeline facade.

#[cfg(feature = "pure")]
mod tests {
    use oxitext::{Pipeline, TextStyle};
    use oxitext_core::FlowDirection;
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
    fn vertical_pipeline_render_does_not_panic() {
        let font = load_test_font();
        let mut pipeline = Pipeline::from_bytes(&font).expect("valid font");
        let style = TextStyle::default().with_flow_direction(FlowDirection::Vertical);
        let result = pipeline.render("abc", &style).expect("vertical render");
        assert!(!result.glyphs.is_empty());
        assert_eq!(result.glyphs.len(), result.bitmaps.len());
    }

    #[test]
    fn vertical_glyphs_have_increasing_y() {
        let font = load_test_font();
        let mut pipeline = Pipeline::from_bytes(&font).expect("valid font");
        let style = TextStyle::default().with_flow_direction(FlowDirection::Vertical);
        let result = pipeline.render("abc", &style).expect("vertical render");
        for w in result.glyphs.windows(2) {
            assert!(
                w[1].pos.1 >= w[0].pos.1,
                "vertical y must increase: {} >= {}",
                w[1].pos.1,
                w[0].pos.1
            );
        }
    }
}
