//! Direct bezier-to-SDF pipeline using analytic (Newton-Raphson refined) distance.
//!
//! Unlike the PSDF module which approximates curves with polylines, this module
//! uses the same Newton-Raphson closest-point refinement employed by the MSDF
//! pipeline, giving exact analytic signed distances for all segment types.
//!
//! # Quality advantage
//! - Lines: exact signed perpendicular distance.
//! - Quadratic Beziers: Newton-Raphson refined closest point (up to 8 iterations).
//! - Cubic Beziers: Newton-Raphson refined closest point (up to 8 iterations).
//!
//! This gives higher quality than EDT (which is coverage-bitmap based) and
//! higher accuracy than PSDF's polyline approximation, especially at small sizes
//! where the approximation error of polylines would be visible.
//!
//! # Output format
//! Returns [`crate::SdfTile`] — identical byte layout to single-channel EDT tiles,
//! so tiles can be packed into an [`crate::SdfAtlas`] without conversion.

use crate::atlas::SdfTile;
use crate::edt::SdfError;
use crate::msdf::{extract_glyph_shape, segment_signed_dist, GlyphShape, Point, Segment};

// ─── Bezier evaluation helpers ────────────────────────────────────────────────

/// Evaluate a quadratic Bezier at parameter `t`.
#[inline]
fn eval_quad(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    (
        u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
        u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
    )
}

/// Evaluate a cubic Bezier at parameter `t`.
#[inline]
fn eval_cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let u = 1.0 - t;
    (
        u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
        u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
    )
}

// ─── Winding number helpers ───────────────────────────────────────────────────

/// Cross-product sign: (b - a) × (p - a).  Positive = p is left of a→b.
#[inline]
fn cross2d(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// Winding contribution from a quadratic Bezier, approximated by N line segments.
fn quad_winding(p0: Point, p1: Point, p2: Point, px: f32, py: f32) -> i32 {
    const N: usize = 8;
    let mut winding = 0i32;
    let p0f = (p0.x as f32, p0.y as f32);
    let p1f = (p1.x as f32, p1.y as f32);
    let p2f = (p2.x as f32, p2.y as f32);
    for i in 0..N {
        let t0 = i as f32 / N as f32;
        let t1 = (i + 1) as f32 / N as f32;
        let a = eval_quad(p0f, p1f, p2f, t0);
        let b = eval_quad(p0f, p1f, p2f, t1);
        if a.1 <= py {
            if b.1 > py && cross2d(a.0, a.1, b.0, b.1, px, py) > 0.0 {
                winding += 1;
            }
        } else if b.1 <= py && cross2d(a.0, a.1, b.0, b.1, px, py) < 0.0 {
            winding -= 1;
        }
    }
    winding
}

/// Winding contribution from a cubic Bezier, approximated by N line segments.
fn cubic_winding(p0: Point, c1: Point, c2: Point, p1: Point, px: f32, py: f32) -> i32 {
    const N: usize = 8;
    let mut winding = 0i32;
    let p0f = (p0.x as f32, p0.y as f32);
    let c1f = (c1.x as f32, c1.y as f32);
    let c2f = (c2.x as f32, c2.y as f32);
    let p1f = (p1.x as f32, p1.y as f32);
    for i in 0..N {
        let t0 = i as f32 / N as f32;
        let t1 = (i + 1) as f32 / N as f32;
        let a = eval_cubic(p0f, c1f, c2f, p1f, t0);
        let b = eval_cubic(p0f, c1f, c2f, p1f, t1);
        if a.1 <= py {
            if b.1 > py && cross2d(a.0, a.1, b.0, b.1, px, py) > 0.0 {
                winding += 1;
            }
        } else if b.1 <= py && cross2d(a.0, a.1, b.0, b.1, px, py) < 0.0 {
            winding -= 1;
        }
    }
    winding
}

/// Compute the winding number of `point` with respect to `shape`.
///
/// Uses a ray-cast approach: a horizontal ray from `point` to `+∞` is tested
/// against every segment in the shape.  Upward crossings add +1, downward
/// crossings add -1.
///
/// Non-zero winding rule: a point is inside the glyph when `w != 0`.  TTF fonts
/// use clockwise outer contours, which produce `w = -1` inside the outer stroke
/// and `w = 0` outside the glyph (and inside counter-shaped holes).
fn winding_number(point: (f32, f32), shape: &GlyphShape) -> i32 {
    let mut winding = 0i32;
    let (px, py) = point;

    for contour in &shape.contours {
        for seg in &contour.segments {
            match seg.segment {
                Segment::Line(a, b) => {
                    let ay = a.y as f32;
                    let by = b.y as f32;
                    if ay <= py {
                        if by > py && cross2d(a.x as f32, ay, b.x as f32, by, px, py) > 0.0 {
                            winding += 1;
                        }
                    } else if by <= py && cross2d(a.x as f32, ay, b.x as f32, by, px, py) < 0.0 {
                        winding -= 1;
                    }
                }
                Segment::Quad(p0, p1, p2) => {
                    winding += quad_winding(p0, p1, p2, px, py);
                }
                Segment::Cubic(p0, c1, c2, p1) => {
                    winding += cubic_winding(p0, c1, c2, p1, px, py);
                }
            }
        }
    }

    winding
}

// ─── Analytic SDF at a point ──────────────────────────────────────────────────

/// Returns the signed analytic distance from `point` (in design-unit space) to
/// the nearest segment in `shape`.
///
/// Sign convention: positive = inside, negative = outside.
/// The encoding formula `0.5 + d / (2 * spread_du)` maps inside to >0.5 and
/// outside to <0.5 in the \[0, 255\] output range.
///
/// Non-zero winding rule is used for inside/outside determination so that
/// counter-shaped holes (which have `w = 0` even though they are "inside" the
/// bounding box) correctly produce a negative (outside) distance.
fn analytic_sdf_at_point(point: (f32, f32), shape: &GlyphShape) -> f64 {
    let (px, py) = point;
    let mut min_abs = f64::MAX;

    for contour in &shape.contours {
        for seg in &contour.segments {
            let d = segment_signed_dist(&seg.segment, px as f64, py as f64);
            if d.abs() < min_abs {
                min_abs = d.abs();
            }
        }
    }

    let w = winding_number(point, shape);
    // TTF outer contours are CW → w = -1 inside, 0 outside.
    // Non-zero rule: w != 0 means inside.
    if w != 0 {
        min_abs
    } else {
        -min_abs
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Compute an analytic single-channel SDF tile for a single glyph.
///
/// Converts bezier outlines directly to SDF without intermediate bitmap
/// rasterization.  Uses Newton-Raphson refined exact distances for quadratic
/// and cubic Bezier segments, giving higher quality than EDT or PSDF at small
/// tile sizes.
///
/// Returns a [`SdfTile`] (same type as the EDT pipeline) so tiles can be packed
/// into [`crate::SdfAtlas`] without conversion.
///
/// # Arguments
/// - `face_data` — raw font file bytes.
/// - `glyph_id` — glyph index within the font.
/// - `px_size` — desired pixel size (controls the shape → pixel scale).
/// - `tile_size` — output tile dimensions (square, in pixels).
/// - `spread` — how many pixels from the outline map to the \[0, 255\] range.
///
/// # Returns
/// `Ok(Some(tile))` for glyphs with an outline, `Ok(None)` for blank/whitespace
/// glyphs that have no outline.
///
/// # Errors
/// Returns [`SdfError::InvalidFont`] when the font bytes cannot be parsed, or
/// [`SdfError::ZeroSize`] when `tile_size == 0`.
pub fn glyph_to_sdf_tile_analytic(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    tile_size: u32,
    spread: f32,
) -> Result<Option<SdfTile>, SdfError> {
    if tile_size == 0 {
        return Err(SdfError::ZeroSize);
    }

    let face = ttf_parser::Face::parse(face_data, 0).map_err(|_| SdfError::InvalidFont)?;
    let gid = ttf_parser::GlyphId(glyph_id);

    let Some(shape) = extract_glyph_shape(face_data, glyph_id) else {
        return Ok(None);
    };
    if shape.contours.is_empty() {
        return Ok(None);
    }

    let units_per_em = shape.units_per_em;
    let scale = px_size / units_per_em;

    // Compute bearing / advance from font metrics.
    let bearing_x = face.glyph_hor_side_bearing(gid).unwrap_or(0) as i32;
    let bearing_y = face.ascender() as i32;
    let advance_x = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale;

    // Bounding box in design units.
    let bbox = face.glyph_bounding_box(gid).ok_or(SdfError::ZeroSize)?;

    let shape_w = (bbox.x_max - bbox.x_min) as f32;
    let shape_h = (bbox.y_max - bbox.y_min) as f32;

    // Scale to fit the tile with a border proportional to `spread`.
    let border_du = spread / scale; // spread expressed in design units
    let fit_scale_x = tile_size as f32 / (shape_w + 2.0 * border_du);
    let fit_scale_y = tile_size as f32 / (shape_h + 2.0 * border_du);
    let fit_scale = fit_scale_x.min(fit_scale_y).max(1e-6);

    let offset_x = bbox.x_min as f32 - border_du;
    let offset_y = bbox.y_min as f32 - border_du;

    let n = tile_size as usize;
    let mut data = vec![0u8; n * n];

    // Distance spread in design units (for normalization).
    let spread_du = spread / fit_scale;

    for py in 0..n {
        for px in 0..n {
            // Map pixel centre → shape (design) space.
            // Font Y is up; pixel row 0 is at the top — flip Y.
            let sx = px as f32 / fit_scale + offset_x;
            let sy = (n - 1 - py) as f32 / fit_scale + offset_y;

            let signed_dist = analytic_sdf_at_point((sx, sy), &shape);

            // Normalize: spread_du design-units ↔ 127 grey levels on each side of 128.
            let normalized = 0.5 + signed_dist / (2.0 * spread_du as f64);
            let clamped = normalized.clamp(0.0, 1.0);
            data[py * n + px] = (clamped * 255.0).round() as u8;
        }
    }

    Ok(Some(SdfTile {
        glyph_id,
        width: tile_size,
        height: tile_size,
        data,
        bearing_x,
        bearing_y,
        advance_x,
    }))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Raw bytes of the bundled test font.
    const TEST_FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

    #[test]
    fn test_analytic_sdf_non_panic() {
        // Any glyph_id that has an outline should succeed without panic.
        let result = glyph_to_sdf_tile_analytic(TEST_FONT, 36, 32.0, 32, 4.0);
        assert!(result.is_ok(), "unexpected error: {:?}", result.err());
    }

    #[test]
    fn test_analytic_sdf_glyph_0_ok() {
        // Glyph 0 (.notdef) may have an outline or not; either None or Some is fine.
        let result = glyph_to_sdf_tile_analytic(TEST_FONT, 0, 16.0, 16, 4.0);
        assert!(result.is_ok(), "unexpected error: {:?}", result.err());
    }

    #[test]
    fn test_analytic_sdf_tile_dimensions() {
        // When a tile is returned its size must match the request.
        let tile = glyph_to_sdf_tile_analytic(TEST_FONT, 36, 32.0, 32, 4.0)
            .expect("should not error")
            .expect("glyph 36 should have an outline");
        assert_eq!(tile.width, 32);
        assert_eq!(tile.height, 32);
        assert_eq!(tile.data.len(), 32 * 32);
    }

    #[test]
    fn test_analytic_sdf_has_inside_and_outside() {
        // An outlined glyph at a reasonable size should have both inside (>128)
        // and outside (<128) pixels, confirming correct sign assignment via the
        // non-zero winding rule.
        let tile = glyph_to_sdf_tile_analytic(TEST_FONT, 36, 64.0, 64, 8.0)
            .expect("should not error")
            .expect("glyph 36 should have an outline");
        assert_eq!(tile.data.len(), 64 * 64);
        let has_inside = tile.data.iter().any(|&v| v > 128);
        let has_outside = tile.data.iter().any(|&v| v < 128);
        assert!(
            has_inside && has_outside,
            "SDF tile should have both inside (>128) and outside (<128) pixels"
        );
    }

    #[test]
    fn test_analytic_sdf_zero_tile_size_errors() {
        let result = glyph_to_sdf_tile_analytic(TEST_FONT, 36, 32.0, 0, 4.0);
        assert!(
            matches!(result, Err(SdfError::ZeroSize)),
            "zero tile_size should return ZeroSize error"
        );
    }

    #[test]
    fn test_analytic_sdf_invalid_font_errors() {
        let bad = b"not a font";
        let result = glyph_to_sdf_tile_analytic(bad, 0, 16.0, 16, 4.0);
        assert!(
            matches!(result, Err(SdfError::InvalidFont)),
            "bad font bytes should return InvalidFont error"
        );
    }

    #[test]
    fn test_analytic_sdf_tile_packable() {
        // Verify the returned SdfTile can be packed into an SdfAtlas without conversion.
        let tile = glyph_to_sdf_tile_analytic(TEST_FONT, 36, 32.0, 32, 4.0)
            .expect("should not error")
            .expect("glyph 36 should have an outline");
        let atlas = crate::SdfAtlas::pack(&[tile]);
        assert!(
            atlas.uv_map.contains_key(&36),
            "packed atlas should contain glyph 36"
        );
    }
}
