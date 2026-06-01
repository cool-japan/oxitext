//! [`OxifontRaster`] — outline-based rasterizer using `oxifont-parser` + `tiny-skia`.
//!
//! Extracts glyph outlines via [`oxifont_parser::ParsedFace`] and fills them
//! using [`tiny_skia`]'s anti-aliased scanline renderer.  Does not depend on
//! fontdue, ab_glyph, or swash — pure outline rendering with no hinting.
//!
//! # Accuracy notes
//!
//! - Coordinates are in font design units scaled to pixels; no grid-fitting or
//!   hinting is applied.
//! - The bounding box is derived from the outline control points, not the
//!   glyph's `hmtx` bounding box, so glyphs with ink outside the control point
//!   convex hull (uncommon) may be slightly mis-sized.
//! - The +1/−1 pixel fringe added around the bitmap prevents AA clipping at
//!   the edges.

use oxifont_core::{FontFace as _, GlyphOutline as FontOutline};
use oxifont_parser::ParsedFace;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::backend::{RasterBackend, RasterOutput};

// ─── OxifontRaster ───────────────────────────────────────────────────────────

/// Glyph rasterizer that uses `oxifont-parser` for outline extraction and
/// `tiny-skia` for anti-aliased scanline rendering.
///
/// Every call to [`RasterBackend::rasterize`] re-parses the face from raw
/// bytes.  For repeated rasterisation of the same font, wrap `OxifontRaster`
/// in a cache layer (e.g. [`crate::BitmapCache`]).
///
/// # Thread safety
/// `OxifontRaster` holds no mutable state and is `Send + Sync`.
pub struct OxifontRaster;

impl OxifontRaster {
    /// Creates a new [`OxifontRaster`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for OxifontRaster {
    fn default() -> Self {
        Self::new()
    }
}

impl RasterBackend for OxifontRaster {
    fn rasterize(&self, face_data: &[u8], glyph_id: u16, px_size: f32) -> RasterOutput {
        rasterize_glyph(face_data, glyph_id, px_size).unwrap_or_else(|| RasterOutput {
            width: 0,
            height: 0,
            coverage: Vec::new(),
            advance_x: 0.0,
            advance_y: 0.0,
            bearing_x: 0,
            bearing_y: 0,
        })
    }
}

// ─── core implementation ──────────────────────────────────────────────────────

fn rasterize_glyph(face_data: &[u8], glyph_id: u16, px_size: f32) -> Option<RasterOutput> {
    let face = ParsedFace::parse(face_data.to_vec(), 0).ok()?;

    let units_per_em = face.units_per_em() as f32;
    if units_per_em <= 0.0 || px_size <= 0.0 {
        return None;
    }
    let scale = px_size / units_per_em;

    let advance_x = face.advance_width(glyph_id).unwrap_or(0) as f32 * scale;

    let outlines = face.outline(glyph_id)?;
    if outlines.is_empty() {
        return Some(RasterOutput {
            width: 0,
            height: 0,
            coverage: Vec::new(),
            advance_x,
            advance_y: 0.0,
            bearing_x: 0,
            bearing_y: 0,
        });
    }

    let (min_x, min_y, max_x, max_y) = bbox_from_outlines(&outlines);
    if min_x >= max_x || min_y >= max_y {
        return Some(RasterOutput {
            width: 0,
            height: 0,
            coverage: Vec::new(),
            advance_x,
            advance_y: 0.0,
            bearing_x: 0,
            bearing_y: 0,
        });
    }

    let w = ((max_x - min_x) * scale).ceil() as u32 + 2;
    let h = ((max_y - min_y) * scale).ceil() as u32 + 2;
    if w == 0 || h == 0 {
        return None;
    }

    let path = build_path(&outlines, min_x, max_y, scale)?;

    let mut pixmap = Pixmap::new(w, h)?;

    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    pixmap.as_mut().fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let coverage: Vec<u8> = pixmap.data().chunks_exact(4).map(|px| px[3]).collect();

    let bearing_x = (min_x * scale).floor() as i32 - 1;
    let bearing_y = (max_y * scale).ceil() as i32 + 1;

    Some(RasterOutput {
        width: w as usize,
        height: h as usize,
        coverage,
        advance_x,
        advance_y: 0.0,
        bearing_x,
        bearing_y,
    })
}

fn build_path(
    outlines: &[FontOutline],
    min_x: f32,
    max_y: f32,
    scale: f32,
) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();

    for cmd in outlines {
        match cmd {
            FontOutline::MoveTo { x, y } => {
                pb.move_to((x - min_x) * scale + 1.0, (max_y - y) * scale + 1.0);
            }
            FontOutline::LineTo { x, y } => {
                pb.line_to((x - min_x) * scale + 1.0, (max_y - y) * scale + 1.0);
            }
            FontOutline::QuadTo { cx, cy, x, y } => {
                pb.quad_to(
                    (cx - min_x) * scale + 1.0,
                    (max_y - cy) * scale + 1.0,
                    (x - min_x) * scale + 1.0,
                    (max_y - y) * scale + 1.0,
                );
            }
            FontOutline::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                pb.cubic_to(
                    (cx1 - min_x) * scale + 1.0,
                    (max_y - cy1) * scale + 1.0,
                    (cx2 - min_x) * scale + 1.0,
                    (max_y - cy2) * scale + 1.0,
                    (x - min_x) * scale + 1.0,
                    (max_y - y) * scale + 1.0,
                );
            }
            FontOutline::Close => {
                pb.close();
            }
        }
    }

    pb.finish()
}

fn bbox_from_outlines(outlines: &[FontOutline]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for cmd in outlines {
        match cmd {
            FontOutline::MoveTo { x, y } | FontOutline::LineTo { x, y } => {
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
            }
            FontOutline::QuadTo { cx, cy, x, y } => {
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *cx, *cy);
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
            }
            FontOutline::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *cx1, *cy1);
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *cx2, *cy2);
                update_bbox(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
            }
            FontOutline::Close => {}
        }
    }

    (min_x, min_y, max_x, max_y)
}

#[inline]
fn update_bbox(min_x: &mut f32, min_y: &mut f32, max_x: &mut f32, max_y: &mut f32, x: f32, y: f32) {
    if x < *min_x {
        *min_x = x;
    }
    if y < *min_y {
        *min_y = y;
    }
    if x > *max_x {
        *max_x = x;
    }
    if y > *max_y {
        *max_y = y;
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::RasterBackend;
    use std::path::Path;

    fn load_test_font() -> Vec<u8> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
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

    fn glyph_id_for_char(font: &[u8], c: char) -> u16 {
        let face = ParsedFace::parse(font.to_vec(), 0).expect("font must parse");
        face.glyph_for_char(c).expect("glyph for char must exist")
    }

    #[test]
    fn oxifont_raster_produces_bitmap_for_visible_glyph() {
        let font = load_test_font();
        let gid = glyph_id_for_char(&font, 'A');
        let raster = OxifontRaster::new();
        let out = raster.rasterize(&font, gid, 16.0);
        assert!(out.width > 0, "visible glyph should have non-zero width");
        assert!(out.height > 0, "visible glyph should have non-zero height");
        assert_eq!(out.coverage.len(), out.width * out.height);
    }

    #[test]
    fn oxifont_raster_has_nonzero_coverage() {
        let font = load_test_font();
        let gid = glyph_id_for_char(&font, 'A');
        let raster = OxifontRaster::new();
        let out = raster.rasterize(&font, gid, 24.0);
        assert!(
            out.coverage.iter().any(|&p| p > 0),
            "rasterized 'A' should have non-zero coverage"
        );
    }

    #[test]
    fn oxifont_raster_invalid_font_returns_zero_output() {
        let raster = OxifontRaster::new();
        let out = raster.rasterize(b"not a font", 36, 16.0);
        assert_eq!(out.width, 0);
        assert_eq!(out.height, 0);
    }

    #[test]
    fn oxifont_raster_larger_size_produces_larger_bitmap() {
        let font = load_test_font();
        let gid = glyph_id_for_char(&font, 'A');
        let raster = OxifontRaster::new();
        let small = raster.rasterize(&font, gid, 8.0);
        let large = raster.rasterize(&font, gid, 32.0);
        assert!(large.width >= small.width);
        assert!(large.height >= small.height);
    }

    #[test]
    fn oxifont_raster_advance_width_is_positive_for_visible_glyph() {
        let font = load_test_font();
        let gid = glyph_id_for_char(&font, 'A');
        let raster = OxifontRaster::new();
        let out = raster.rasterize(&font, gid, 16.0);
        assert!(out.advance_x > 0.0);
    }

    #[test]
    fn oxifont_raster_rasterize_full_roundtrips() {
        let font = load_test_font();
        let gid = glyph_id_for_char(&font, 'B');
        let raster = OxifontRaster::new();
        let result = raster.rasterize_full(&font, gid, 20.0);
        assert!(!result.is_empty());
    }
}
