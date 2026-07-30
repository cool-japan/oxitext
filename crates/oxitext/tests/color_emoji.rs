//! Colour-glyph integration tests for the `oxitext` facade.
//!
//! These cover the two layers a caller sees:
//!
//! 1. `oxitext_raster::render_colr_v0` / `render_colr_v1` directly, and
//! 2. the `oxitext::Pipeline`, which must classify an emoji as a colour glyph
//!    and hand back a [`RenderOutput::Color`] with real pixels in it.
//!
//! The pipeline used to call `render_colr_v0` for COLR**v1** fonts, and the
//! painter behind it silently produced a fully transparent bitmap, so an emoji
//! run rendered as nothing at all.  [`pipeline_renders_colrv1_emoji_in_colour`]
//! is the end-to-end guard for that.
//!
//! COLR fixtures come from `googlefonts/color-fonts` (Apache-2.0); see
//! `tests/fixtures/README.md`.  A checkout without them still builds: the
//! plain-font tests keep working and the COLR tests point at the missing path.

use oxitext::{RenderOutput, Rgba8};
use oxitext_raster::{render_colr_v0, render_colr_v1, ColorGlyphBitmap};
use std::collections::HashSet;
use std::path::Path;
use ttf_parser::GlyphId;

/// A real COLRv1 emoji font: Twemoji smileys with solid layers and transforms.
const TWEMOJI_SMILEY: &str = "../../tests/fixtures/twemoji_smiley-glyf_colr_1.ttf";
/// A real COLRv1 emoji font using linear and radial gradients.
const NOTO_HANDWRITING: &str = "../../tests/fixtures/noto_handwriting-glyf_colr_1.ttf";

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

/// Load a COLR fixture, failing loudly when the checkout is incomplete.
fn require_colr_font(relative: &str) -> Vec<u8> {
    load_font_opt(relative)
        .unwrap_or_else(|| panic!("missing fixture {relative}; see tests/fixtures/README.md"))
}

/// Fraction of pixels carrying any alpha.
fn coverage(bitmap: &ColorGlyphBitmap) -> f32 {
    let painted = bitmap.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
    painted as f32 / (bitmap.width * bitmap.height) as f32
}

/// Distinct near-opaque colours, quantised to 5 bits per channel so that
/// anti-aliased edges of a single-colour fill do not inflate the count.
fn distinct_opaque_colors(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|px| px[3] >= 250)
        .map(|px| (px[0] >> 3, px[1] >> 3, px[2] >> 3))
        .collect::<HashSet<_>>()
        .len()
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

/// A checked-in COLRv1 emoji renders with ink and more than one colour.
#[test]
fn colr_v1_fixture_renders_in_colour() {
    let font_data = require_colr_font(TWEMOJI_SMILEY);
    let face = ttf_parser::Face::parse(&font_data, 0).expect("parse colr fixture");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let bitmap = render_colr_v1(&font_data, gid, 64, 64).expect("COLRv1 glyph must render");
    assert_bitmap_valid(&bitmap, 64, 64);
    let painted = coverage(&bitmap);
    assert!(
        painted > 0.05,
        "emoji covered only {:.1}% of the em box",
        painted * 100.0
    );
    assert!(
        distinct_opaque_colors(&bitmap.rgba) >= 2,
        "emoji must use more than one colour"
    );
}

// ---------------------------------------------------------------------------
// End-to-end pipeline
// ---------------------------------------------------------------------------

/// Name of a [`RenderOutput`] variant, for assertion messages.
fn output_kind(output: &RenderOutput) -> &'static str {
    match output {
        RenderOutput::Color(_) => "Color",
        RenderOutput::Greyscale(_) => "Greyscale",
        RenderOutput::Lcd(_) => "Lcd",
        RenderOutput::Sdf { .. } => "Sdf",
        RenderOutput::Msdf { .. } => "Msdf",
    }
}

/// The pipeline must classify a COLRv1 emoji as a colour glyph and produce a
/// non-transparent, multi-coloured bitmap for it.
#[test]
fn pipeline_renders_colrv1_emoji_in_colour() {
    let font_data = require_colr_font(TWEMOJI_SMILEY);
    let mut pipeline = oxitext::Pipeline::from_bytes(&font_data).expect("valid emoji font");
    let style = oxitext::TextStyle {
        font_size: 64.0,
        ..Default::default()
    };

    let result = pipeline.render("\u{1F601}", &style).expect("render emoji");
    assert!(!result.glyphs.is_empty(), "emoji must shape to a glyph");

    let mut color_outputs = 0;
    for output in &result.outputs {
        let RenderOutput::Color(cbm) = output else {
            continue;
        };
        color_outputs += 1;
        let painted = cbm.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(
            painted * 20 > (cbm.width * cbm.height) as usize,
            "colour bitmap covered only {painted} of {} pixels",
            cbm.width * cbm.height
        );
        assert!(
            distinct_opaque_colors(&cbm.rgba) >= 2,
            "colour bitmap must use more than one colour"
        );
    }
    assert!(
        color_outputs > 0,
        "the emoji must be rasterized as RenderOutput::Color, got {:?}",
        result.outputs.iter().map(output_kind).collect::<Vec<_>>()
    );
}

/// Compositing an emoji onto a canvas must leave visible, coloured pixels.
#[test]
fn composite_to_rgba_keeps_emoji_colour() {
    let font_data = require_colr_font(TWEMOJI_SMILEY);
    let mut pipeline = oxitext::Pipeline::from_bytes(&font_data).expect("valid emoji font");
    let style = oxitext::TextStyle {
        font_size: 64.0,
        ..Default::default()
    };
    let result = pipeline.render("\u{1F601}", &style).expect("render emoji");

    let canvas = result.composite_to_rgba(
        128,
        128,
        Rgba8::new(255, 255, 255, 255),
        Rgba8::new(0, 0, 0, 255),
    );

    let non_white = canvas
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0] != 255 || px[1] != 255 || px[2] != 255)
        .count();
    assert!(
        non_white > 200,
        "emoji left only {non_white} non-background pixels on the canvas"
    );

    // The glyph is a yellow face; a greyscale fallback would leave no
    // saturated pixels behind.
    let saturated = canvas
        .rgba
        .chunks_exact(4)
        .filter(|px| {
            let max = px[0].max(px[1]).max(px[2]);
            let min = px[0].min(px[1]).min(px[2]);
            u32::from(max) - u32::from(min) > 60
        })
        .count();
    assert!(
        saturated > 100,
        "expected saturated emoji colours, found {saturated} such pixels"
    );
}

/// A gradient emoji survives the pipeline too.
#[test]
fn pipeline_renders_gradient_emoji() {
    let font_data = require_colr_font(NOTO_HANDWRITING);
    let mut pipeline = oxitext::Pipeline::from_bytes(&font_data).expect("valid emoji font");
    let style = oxitext::TextStyle {
        font_size: 64.0,
        ..Default::default()
    };
    let result = pipeline.render("\u{270D}", &style).expect("render emoji");

    let mut saw_color = false;
    for output in &result.outputs {
        if let RenderOutput::Color(cbm) = output {
            saw_color = true;
            let colors = distinct_opaque_colors(&cbm.rgba);
            assert!(
                colors >= 12,
                "gradient emoji should produce a colour ramp, got {colors} colours"
            );
        }
    }
    assert!(
        saw_color,
        "gradient emoji must produce a colour output, got {:?}",
        result.outputs.iter().map(output_kind).collect::<Vec<_>>()
    );
}
