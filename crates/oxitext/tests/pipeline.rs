use oxitext::{best_font_for_char, FontdueRasterizer, Pipeline, SwashShaper, TextStyle};
use std::path::Path;
use std::sync::Arc;

fn load_test_font() -> Vec<u8> {
    // Try the workspace-level fixture first (relative to this crate).
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        return std::fs::read(&fixture).expect("read fixture font");
    }
    // Fallback: well-known macOS/Linux system fonts (TTF only, not TTC).
    let candidates = [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in &candidates {
        if Path::new(path).exists() {
            return std::fs::read(path).expect("read system font");
        }
    }
    panic!(
        "no test font found — add a TTF to {}/tests/fixtures/test-font.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
}

#[test]
fn pipeline_hello_world() {
    let font_bytes = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("valid font");
    let style = TextStyle::default();
    let result = pipeline.render("Hello", &style).expect("render failed");

    assert!(
        !result.glyphs.is_empty(),
        "expected at least 1 glyph, got 0"
    );
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "glyphs and bitmaps must have the same length"
    );
    // Verify at least one bitmap contains non-zero ink.
    let any_nonzero = result
        .bitmaps
        .iter()
        .any(|bm| bm.width > 0 && bm.height > 0 && bm.pixels.iter().any(|&p| p > 0));
    assert!(any_nonzero, "all bitmaps are empty/zero for 'Hello'");
    // Verify bitmap dimensions are consistent with pixel buffer length.
    for (i, bm) in result.bitmaps.iter().enumerate() {
        assert_eq!(
            bm.pixels.len(),
            (bm.width * bm.height) as usize,
            "bitmap[{i}] pixel buffer length mismatch: {}×{} but pixels.len()={}",
            bm.width,
            bm.height,
            bm.pixels.len()
        );
    }
}

#[test]
fn shaper_produces_advances() {
    let font_bytes: Arc<[u8]> = Arc::from(load_test_font().as_slice());
    let mut shaper = SwashShaper::new();
    let run = shaper.shape("AB", font_bytes, 16.0).expect("shape failed");
    assert!(!run.glyphs.is_empty(), "expected shaped glyphs for 'AB'");
    for g in &run.glyphs {
        assert!(
            g.x_advance > 0.0,
            "glyph x_advance should be positive, got {}",
            g.x_advance
        );
    }
}

#[test]
fn rasterizer_produces_bitmap() {
    let font_bytes: Arc<[u8]> = Arc::from(load_test_font().as_slice());
    let rasterizer = FontdueRasterizer::new();
    // GID 36 is commonly 'A' in Latin fonts (varies by font).
    let bm = rasterizer
        .raster(36, &font_bytes, 16.0)
        .expect("raster failed");
    // Pixel buffer must be consistent with declared dimensions.
    assert_eq!(
        bm.pixels.len(),
        (bm.width * bm.height) as usize,
        "pixel buffer length mismatch"
    );
    if bm.width > 0 && bm.height > 0 {
        let pixel_sum: u32 = bm.pixels.iter().map(|&p| p as u32).sum();
        assert!(pixel_sum > 0, "GID 36 bitmap is all zeros");
    }
}

#[test]
fn pipeline_hello_world_glyph_positions_increase() {
    let font_bytes = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("valid font");
    let style = TextStyle::default();
    let result = pipeline.render("Hello", &style).expect("render failed");
    // With LTR layout and default max_width=800, no wrapping expected for
    // 5 glyphs at 16px → successive glyphs should have non-decreasing x.
    let positions: Vec<f32> = result.glyphs.iter().map(|g| g.pos.0).collect();
    for window in positions.windows(2) {
        assert!(
            window[1] >= window[0],
            "x positions should not decrease: {} > {}",
            window[0],
            window[1]
        );
    }
}

// ── script-aware fallback tests ──────────────────────────────────────────────

#[test]
fn test_shape_with_fallback_single_font_same_as_shape() {
    let font_bytes = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("build");
    // No fallbacks — should behave like regular shape.
    let result = pipeline.shape_with_fallback("Hello", 16.0);
    assert!(
        result.is_ok(),
        "shape_with_fallback failed: {:?}",
        result.err()
    );
    let r = result.expect("already checked");
    assert!(!r.glyphs.is_empty(), "expected glyphs for 'Hello'");
}

#[test]
fn test_best_font_for_char_prefers_primary() {
    let font_bytes = load_test_font();
    // 'A' is a basic Latin character present in any standard test font.
    // Should return 0 (primary) with no fallbacks.
    let idx = best_font_for_char('A', &font_bytes, &[]);
    assert_eq!(idx, 0, "should prefer primary when glyph is present");
}

#[test]
fn test_best_font_for_char_falls_back_when_missing() {
    let font_bytes = load_test_font();
    // Create a trivially empty fallback (0 bytes — ttf_parser will fail to parse it,
    // so font_has_glyph returns false).  The function should still return 0 (primary)
    // as the safe default.
    let empty_fallback: Vec<u8> = Vec::new();
    let idx = best_font_for_char('A', &font_bytes, &[empty_fallback]);
    // Primary has 'A', so we still expect 0.
    assert_eq!(idx, 0, "primary should win when it has the glyph");
}

#[test]
fn test_shape_with_fallback_empty_text() {
    let font_bytes = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("build");
    let result = pipeline.shape_with_fallback("", 16.0);
    assert!(result.is_ok(), "empty text should succeed");
    let r = result.expect("already checked");
    assert!(r.glyphs.is_empty(), "empty text should produce no glyphs");
}

#[test]
fn test_shape_with_fallback_with_fallback_font() {
    let font_bytes = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("build");
    // Use the same font as fallback — all characters stay in font 0.
    pipeline.set_fallback_fonts(vec![font_bytes.clone()]);
    let result = pipeline.shape_with_fallback("Hello", 16.0);
    assert!(result.is_ok(), "shape with self-fallback failed");
    let r = result.expect("already checked");
    assert!(
        !r.glyphs.is_empty(),
        "expected glyphs when primary is also fallback"
    );
}
