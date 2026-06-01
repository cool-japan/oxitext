//! Subpixel pen-position integration tests.
//!
//! Verifies that rasterizing the same glyph at `x_offset = 0.0` versus
//! `x_offset = 0.5` yields coverage bitmaps with different edge content
//! (i.e. the subpixel shift is actually reflected in the pixel data).

use oxitext_raster::subpixel::rasterize_with_offset;
use std::path::Path;

fn load_test_font() -> Vec<u8> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        return std::fs::read(&fixture).expect("read fixture font");
    }
    let candidates = [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            return std::fs::read(p).expect("read system font");
        }
    }
    panic!("no test font found — add tests/fixtures/test-font.ttf");
}

/// Rasterizing the same glyph at x_offset = 0.0 vs x_offset = 0.5 must
/// produce different coverage bitmaps, demonstrating that subpixel shift
/// affects the rendered output.
#[test]
fn subpixel_shift_changes_coverage() {
    let font_data = load_test_font();
    // GID 36 is typically a visible Latin glyph (often 'A').
    let glyph_id = 36u16;
    let px_size = 32.0_f32;

    let (key0, cov0) = rasterize_with_offset(&font_data, glyph_id, px_size, 0.0)
        .expect("rasterize at offset 0.0 failed");

    let (key1, cov1) = rasterize_with_offset(&font_data, glyph_id, px_size, 0.5)
        .expect("rasterize at offset 0.5 failed");

    // Keys must differ (different subpixel bucket).
    assert_ne!(
        key0, key1,
        "cache keys for offset=0.0 and offset=0.5 must be different"
    );

    // If the glyph produced empty coverage (e.g. whitespace), we can't
    // compare pixel content — just confirm the keys differ.
    if cov0.is_empty() || cov1.is_empty() {
        return;
    }

    // The bitmaps should differ: a 0.5-pixel horizontal shift changes the
    // sub-pixel blending at left/right edges.
    assert_ne!(
        cov0, cov1,
        "coverage bitmaps for x_offset=0.0 and x_offset=0.5 must differ \
         (subpixel shift should change edge anti-aliasing)"
    );
}

/// Confirms that `SubpixelCacheKey` equality respects the fractional bucket.
#[test]
fn cache_key_respects_subpixel_bucket() {
    use oxitext_raster::subpixel::{SubpixelCacheKey, SubpixelOffset};

    let k_zero = SubpixelCacheKey::new(36, 16.0, 0.0);
    let k_half = SubpixelCacheKey::new(36, 16.0, 0.5);
    let k_zero_dup = SubpixelCacheKey::new(36, 16.0, 0.0);

    assert_ne!(k_zero, k_half);
    assert_eq!(k_zero, k_zero_dup);
    assert_eq!(SubpixelOffset::from_float(0.0).bucket(), 0);
    assert_eq!(SubpixelOffset::from_float(0.5).bucket(), 2);
}
