//! COLRv0 color glyph rendering integration tests.
//!
//! These tests validate the `render_colr_v0` function. Because the standard
//! test fixture (`test-font.ttf`) is a plain Latin font without COLR data,
//! we verify two behaviours:
//!
//! 1. A font **without** COLR data returns `None` for any glyph.
//! 2. The function handles the case gracefully without panicking.
//!
//! If a COLRv0 font fixture is placed at
//! `tests/fixtures/colr-v0-test.ttf` relative to the workspace root,
//! the test will additionally verify that `render_colr_v0` returns `Some`
//! with a non-empty bitmap.

use oxitext_raster::{render_colr_v0, ColorGlyphBitmap};
use std::path::Path;
use ttf_parser::GlyphId;

fn load_font_opt(relative: &str) -> Option<Vec<u8>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if fixture.exists() {
        Some(std::fs::read(&fixture).expect("read font"))
    } else {
        None
    }
}

fn load_test_font() -> Vec<u8> {
    // 1. Project fixture (deterministic, checked in).
    // 2. Bundled Noto Sans Regular — always available, no system font required.
    load_font_opt("../../tests/fixtures/test-font.ttf")
        .unwrap_or_else(|| oxifont_bundled::NOTO_SANS_REGULAR.to_vec())
}

/// Verifies that `render_colr_v0` does not panic on a plain font and returns
/// `None` for glyphs that have no COLR data.
#[test]
fn non_colr_font_returns_none() {
    let font_data = load_test_font();
    let face = ttf_parser::Face::parse(&font_data, 0).expect("parse face");
    if face.tables().colr.is_none() {
        // Plain font — must return None.
        let result = render_colr_v0(&font_data, GlyphId(36), 32, 32);
        assert!(
            result.is_none(),
            "render_colr_v0 must return None for a font without COLR data"
        );
    }
    // If the test font happens to have COLR, we just confirm no panic.
}

/// Asserts that a [`ColorGlyphBitmap`] has consistent dimensions and buffer size.
fn assert_bitmap_valid(bm: &ColorGlyphBitmap, expected_w: u32, expected_h: u32) {
    assert_eq!(bm.width, expected_w, "bitmap width mismatch");
    assert_eq!(bm.height, expected_h, "bitmap height mismatch");
    assert_eq!(
        bm.rgba.len(),
        (expected_w * expected_h * 4) as usize,
        "rgba buffer must be width*height*4 bytes"
    );
}

/// If a COLRv0 fixture is available at `tests/fixtures/colr-v0-test.ttf`,
/// verifies that `render_colr_v0` returns a correctly sized, non-empty bitmap.
#[test]
fn colr_v0_fixture_renders_if_available() {
    let Some(font_data) = load_font_opt("../../tests/fixtures/colr-v0-test.ttf") else {
        return; // optional fixture absent — skip
    };

    let face = ttf_parser::Face::parse(&font_data, 0).expect("parse colr fixture");
    let colr = match face.tables().colr {
        Some(c) => c,
        None => return, // fixture has no COLR table — skip
    };

    let count = face.number_of_glyphs();
    let colr_glyph = (0..count).map(GlyphId).find(|&gid| colr.contains(gid));
    let Some(gid) = colr_glyph else { return };

    let result = render_colr_v0(&font_data, gid, 32, 32)
        .expect("render_colr_v0 must succeed for a COLR glyph");
    assert_bitmap_valid(&result, 32, 32);
    let has_ink = result.rgba.chunks(4).any(|p| p[3] > 0);
    assert!(
        has_ink,
        "COLR bitmap must have at least one non-transparent pixel"
    );
}
