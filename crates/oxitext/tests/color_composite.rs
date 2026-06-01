//! Tests for `RenderResult::composite_to_rgba` with color-glyph outputs.
//!
//! Validates that the compositor handles all `RenderOutput` variants correctly:
//! - `Greyscale` → tinted blit using text_color
//! - `Color`     → Porter-Duff source-over with native RGBA, ignoring text_color
//! - `Lcd`       → averaged sub-pixel channels rendered as greyscale
//! - `Sdf`/`Msdf` → silently skipped (no panic)
//! - missing output entry → gracefully skipped

use oxitext::{Bitmap, ColorBitmap, PositionedGlyph, RenderOutput, Rgba8};
use oxitext_core::LcdBitmap;
use oxitext_layout::{Line, LineMetrics, ParagraphMetrics};
use std::path::Path;
use std::sync::Arc;

/// Build a minimal [`oxitext::RenderResult`] from component slices for unit testing.
fn make_render_result(
    glyphs: Vec<PositionedGlyph>,
    bitmaps: Vec<Bitmap>,
    outputs: Vec<RenderOutput>,
) -> oxitext::RenderResult {
    let line_count = if glyphs.is_empty() { 0 } else { 1 };
    let lines = if glyphs.is_empty() {
        vec![]
    } else {
        vec![Line {
            glyph_start: 0,
            glyph_end: glyphs.len(),
            metrics: LineMetrics {
                baseline_y: 16.0,
                ascent: 12.0,
                descent: 4.0,
                leading: 0.0,
                width: 64.0,
            },
        }]
    };
    oxitext::RenderResult {
        glyphs,
        bitmaps,
        outputs,
        lines,
        metrics: ParagraphMetrics {
            total_width: 64.0,
            total_height: 20.0,
            line_count,
            overflow: false,
            truncated: false,
        },
        decoration_rects: vec![],
    }
}

/// Helper: create a positioned glyph at the given canvas position.
fn make_glyph(x: f32, y: f32) -> PositionedGlyph {
    PositionedGlyph {
        gid: 1,
        font_data: Arc::from(&[][..]),
        pos: (x, y),
        font_size: 16.0,
        advance_x: 10.0,
        cluster: 0,
    }
}

// ── Test 1: synthetic Color glyph composites onto canvas ──────────────────────

/// A 4×4 red color bitmap placed at (0,0) should produce red pixels at
/// the top-left of the canvas with non-zero alpha.
#[test]
fn color_output_composites_non_zero_alpha() {
    // 4×4 fully-opaque red RGBA bitmap.
    let red_rgba: Vec<u8> = (0..16).flat_map(|_| [255u8, 0, 0, 255]).collect();
    let cbm = ColorBitmap {
        width: 4,
        height: 4,
        rgba: red_rgba,
    };

    let glyph = make_glyph(0.0, 0.0);
    let result = make_render_result(
        vec![glyph],
        vec![Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        }],
        vec![RenderOutput::Color(cbm)],
    );

    let canvas = result.composite_to_rgba(
        64,
        64,
        Rgba8::new(255, 255, 255, 255), // white background
        Rgba8::BLACK,                   // text color (should be ignored for Color)
    );

    assert_eq!(canvas.width, 64);
    assert_eq!(canvas.height, 64);

    // Top-left pixel (0,0) should be red (255,0,0,255) after source-over.
    let idx = 0;
    assert_eq!(canvas.rgba[idx], 255, "R channel should be 255 (red)");
    assert_eq!(canvas.rgba[idx + 1], 0, "G channel should be 0");
    assert_eq!(canvas.rgba[idx + 2], 0, "B channel should be 0");
    assert_eq!(
        canvas.rgba[idx + 3],
        255,
        "alpha should be 255 (fully opaque)"
    );

    // Pixel at (4,4) (outside the color bitmap) should be the background white.
    let bg_idx = (4 * 64 + 4) * 4;
    assert_eq!(canvas.rgba[bg_idx], 255, "background R should be 255");
    assert_eq!(canvas.rgba[bg_idx + 1], 255, "background G should be 255");
    assert_eq!(canvas.rgba[bg_idx + 2], 255, "background B should be 255");
    assert_eq!(
        canvas.rgba[bg_idx + 3],
        255,
        "background alpha should be 255"
    );
}

// ── Test 2: mixed Greyscale + Color outputs ───────────────────────────────────

/// Two glyphs: one greyscale (should be tinted with text_color) and one color
/// (should retain native RGBA).  Both must appear on the canvas.
#[test]
fn mixed_greyscale_and_color_outputs() {
    // Greyscale: 2×2 fully covered.
    let grey_bm = Bitmap {
        width: 2,
        height: 2,
        pixels: vec![255u8; 4],
    };

    // Color: 2×2 fully-opaque blue.
    let blue_rgba: Vec<u8> = (0..4).flat_map(|_| [0u8, 0, 255, 255]).collect();
    let blue_cbm = ColorBitmap {
        width: 2,
        height: 2,
        rgba: blue_rgba,
    };

    // Place greyscale glyph at (0,0), color glyph at (10,0).
    let glyph_grey = make_glyph(0.0, 0.0);
    let glyph_color = make_glyph(10.0, 0.0);

    let result = make_render_result(
        vec![glyph_grey, glyph_color],
        vec![
            grey_bm.clone(),
            Bitmap {
                width: 0,
                height: 0,
                pixels: vec![],
            },
        ],
        vec![
            RenderOutput::Greyscale(grey_bm),
            RenderOutput::Color(blue_cbm),
        ],
    );

    // Use green text color for greyscale glyphs.
    let canvas = result.composite_to_rgba(
        64,
        32,
        Rgba8::new(0, 0, 0, 255),   // black background
        Rgba8::new(0, 255, 0, 255), // green text color
    );

    // Greyscale area (0,0)-(1,1) should be green (text_color).
    let g_idx = 0usize;
    assert_eq!(
        canvas.rgba[g_idx], 0,
        "greyscale R should follow text_color (green → R=0)"
    );
    assert_eq!(
        canvas.rgba[g_idx + 1],
        255,
        "greyscale G should follow text_color (green → G=255)"
    );
    assert_eq!(
        canvas.rgba[g_idx + 2],
        0,
        "greyscale B should follow text_color (green → B=0)"
    );
    assert_eq!(canvas.rgba[g_idx + 3], 255, "greyscale alpha should be 255");

    // Color area at (10,0) should be blue, NOT tinted green.
    let c_idx = 10 * 4;
    assert_eq!(canvas.rgba[c_idx], 0, "color R should be 0 (blue glyph)");
    assert_eq!(
        canvas.rgba[c_idx + 1],
        0,
        "color G should be 0 (blue glyph, not tinted)"
    );
    assert_eq!(
        canvas.rgba[c_idx + 2],
        255,
        "color B should be 255 (blue glyph)"
    );
    assert_eq!(canvas.rgba[c_idx + 3], 255, "color alpha should be 255");
}

// ── Test 3: Sdf, Msdf, and Lcd outputs do not panic ──────────────────────────

/// Sdf and Msdf outputs must be silently skipped without panicking or producing
/// canvas artefacts.
#[test]
fn sdf_and_msdf_are_silently_skipped() {
    let glyph = make_glyph(5.0, 5.0);

    let result = make_render_result(
        vec![glyph],
        vec![Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        }],
        vec![RenderOutput::Sdf {
            width: 8,
            height: 8,
            data: vec![128u8; 64],
        }],
    );

    // This must not panic.
    let canvas = result.composite_to_rgba(32, 32, Rgba8::new(200, 200, 200, 255), Rgba8::BLACK);
    assert_eq!(canvas.rgba.len(), 32 * 32 * 4);

    // Canvas should be entirely the background color (SDF was skipped).
    for px in canvas.rgba.chunks(4) {
        assert_eq!(px[0], 200);
        assert_eq!(px[1], 200);
        assert_eq!(px[2], 200);
        assert_eq!(px[3], 255);
    }
}

#[test]
fn msdf_is_silently_skipped() {
    let glyph = make_glyph(0.0, 0.0);

    let result = make_render_result(
        vec![glyph],
        vec![Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        }],
        vec![RenderOutput::Msdf {
            width: 4,
            height: 4,
            data: vec![100u8; 48],
        }],
    );

    let canvas = result.composite_to_rgba(16, 16, Rgba8::new(50, 50, 50, 255), Rgba8::BLACK);
    assert_eq!(canvas.rgba.len(), 16 * 16 * 4);

    for px in canvas.rgba.chunks(4) {
        assert_eq!(px[0], 50, "canvas should be unchanged background");
        assert_eq!(px[1], 50);
        assert_eq!(px[2], 50);
        assert_eq!(px[3], 255);
    }
}

#[test]
fn lcd_output_composites_without_panic() {
    // 4×4 fully saturated red LCD sub-pixels.
    let rgb: Vec<u8> = (0..16).flat_map(|_| [255u8, 0u8, 0u8]).collect();
    let lcd = LcdBitmap::new(4, 4, rgb);

    let glyph = make_glyph(0.0, 0.0);

    let result = make_render_result(
        vec![glyph],
        vec![Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        }],
        vec![RenderOutput::Lcd(lcd)],
    );

    // Must not panic; the LCD coverage should produce non-zero alpha in the
    // glyph area when text_color is opaque.
    let canvas = result.composite_to_rgba(
        16,
        16,
        Rgba8::new(0, 0, 0, 255),
        Rgba8::new(255, 255, 255, 255), // white text color
    );
    assert_eq!(canvas.rgba.len(), 16 * 16 * 4);
    // At least one pixel in the 4×4 area should be non-background.
    let bg_is_black = canvas
        .rgba
        .chunks(4)
        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0);
    assert!(
        bg_is_black,
        "LCD glyph should have produced some non-black pixels"
    );
}

// ── Test 4: missing outputs entry does not panic ──────────────────────────────

/// When `outputs` is shorter than `glyphs` (should not happen in normal flow
/// but is guarded defensively), the function must not panic.
#[test]
fn missing_outputs_entry_is_skipped_gracefully() {
    let glyph = make_glyph(0.0, 0.0);

    // outputs vector is empty — get(0) returns None.
    let result = make_render_result(
        vec![glyph],
        vec![Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        }],
        vec![], // intentionally shorter than glyphs
    );

    // Must not panic.
    let canvas = result.composite_to_rgba(16, 16, Rgba8::new(128, 128, 128, 255), Rgba8::BLACK);
    assert_eq!(canvas.rgba.len(), 16 * 16 * 4);
}

// ── Test 5: smoke test with system font (skip if not available) ───────────────

fn load_font_opt(relative: &str) -> Option<Vec<u8>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if fixture.exists() {
        Some(std::fs::read(&fixture).expect("read font"))
    } else {
        None
    }
}

/// Smoke test: render ASCII text and call composite_to_rgba end-to-end.
/// Skips if no test font is available.
#[test]
fn smoke_composite_with_system_font() {
    let Some(font_bytes) = load_font_opt("../../tests/fixtures/test-font.ttf").or_else(|| {
        let candidates = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        candidates.iter().find_map(|p| {
            if Path::new(p).exists() {
                Some(std::fs::read(p).expect("read system font"))
            } else {
                None
            }
        })
    }) else {
        // No font available — skip.
        return;
    };

    let mut pipeline = oxitext::Pipeline::from_bytes(&font_bytes).expect("valid font");
    let style = oxitext::TextStyle::default();

    let result = pipeline.render("Hello", &style).expect("render");

    let canvas = result.composite_to_rgba(200, 40, Rgba8::new(255, 255, 255, 255), Rgba8::BLACK);

    assert_eq!(canvas.width, 200);
    assert_eq!(canvas.height, 40);
    assert_eq!(canvas.rgba.len(), 200 * 40 * 4);

    // At least some non-white pixels should be present (actual rendered text).
    let has_ink = canvas.rgba.chunks(4).any(|px| px[3] < 255 || px[0] < 255);
    assert!(
        has_ink,
        "composite_to_rgba should produce visible ink for 'Hello'"
    );
}
