//! Demonstrates the three glyph-rasterization entry points of
//! `oxitext-raster`: the default greyscale [`FontdueRaster`] backend,
//! genuine sub-pixel pen positioning via [`FontdueRaster::raster_positioned`],
//! and color-glyph detection + COLRv1 paint-graph rendering.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-raster --example rasterize_glyph
//! ```

use oxitext_raster::{
    detect_color_glyph_type, render_colr_v1, ColorGlyphType, FontdueRaster, RasterBackend,
};
use ttf_parser::{Face, GlyphId};

/// Font bytes embedded at compile time from the workspace's checked-in test
/// fixture. Resolved relative to this source file, so it works regardless of
/// the process's current working directory.
const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

/// A COLRv1 test fixture (real Twemoji smileys) used to demonstrate color
/// glyph detection and rendering.
const COLR_FONT: &[u8] = include_bytes!("../../../tests/fixtures/twemoji_smiley-glyf_colr_1.ttf");

fn main() {
    let raster = FontdueRaster::new();

    // ─── 1. Plain greyscale rasterization ───────────────────────────────────

    let face = Face::parse(FONT, 0).expect("bundled test font must parse");
    let glyph_id = face.glyph_index('A').expect("test font should cover 'A'").0;

    let px_size = 48.0;
    let out = raster.rasterize(FONT, glyph_id, px_size);
    println!(
        "greyscale 'A' @ {px_size}px: {}x{} coverage bitmap, advance={:.2}px",
        out.width, out.height, out.advance_x
    );
    assert!(
        out.width > 0 && out.height > 0,
        "'A' must have a visible outline"
    );

    // ─── 2. Sub-pixel pen positioning ───────────────────────────────────────
    //
    // `raster_positioned` samples the outline at the requested fractional
    // pen offset (via `ab_glyph`), so two calls at different offsets produce
    // bitmaps with different edge coverage — unlike `rasterize`, which always
    // grid-fits to integer coordinates.

    let at_origin = raster
        .raster_positioned(FONT, glyph_id, px_size, 0.0, 0.0)
        .expect("raster_positioned must succeed for a visible glyph");
    let shifted = raster
        .raster_positioned(FONT, glyph_id, px_size, 0.5, 0.0)
        .expect("raster_positioned must succeed for a visible glyph");
    println!(
        "raster_positioned 'A': offset (0.0, 0.0) -> {}x{}, offset (0.5, 0.0) -> {}x{}, \
         pixel data differs = {}",
        at_origin.width,
        at_origin.height,
        shifted.width,
        shifted.height,
        at_origin.pixels != shifted.pixels || at_origin.width != shifted.width
    );
    assert!(
        at_origin.pixels != shifted.pixels || at_origin.width != shifted.width,
        "a 0.5px pen shift must change the rasterized bitmap"
    );

    // ─── 3. Color glyph detection + COLRv1 rendering ────────────────────────

    let colr_face = Face::parse(COLR_FONT, 0).expect("COLRv1 fixture must parse");
    let color_gid = (0..colr_face.number_of_glyphs())
        .find(|&g| detect_color_glyph_type(COLR_FONT, g) == ColorGlyphType::ColrV1)
        .expect("the Twemoji smiley fixture must contain at least one COLRv1 glyph");

    let dim = 64u32;
    let bitmap = render_colr_v1(COLR_FONT, GlyphId(color_gid), dim, dim)
        .expect("a detected COLRv1 glyph must render");
    let opaque_pixels = bitmap.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
    println!(
        "COLRv1 glyph {color_gid}: {}x{} bitmap, {opaque_pixels} of {} pixels painted",
        bitmap.width,
        bitmap.height,
        bitmap.width as usize * bitmap.height as usize
    );
    assert!(
        opaque_pixels > 0,
        "a real emoji glyph must paint visible pixels"
    );
}
