//! Signed Pseudo-Distance Field (PSDF) generation.
//!
//! PSDF uses the perpendicular signed distance to the nearest outline segment
//! rather than the Euclidean distance transform.  This tends to give crisper
//! results near corners at the cost of some edge continuity.

use crate::edt::SdfError;
use crate::msdf::{extract_glyph_shape, GlyphShape, Segment};

// ─── Public tile type ─────────────────────────────────────────────────────────

/// A single-channel PSDF glyph tile.
///
/// Each pixel stores the signed perpendicular distance to the nearest outline
/// edge, normalized to \[0, 255\] with `128` representing "on the edge".
#[derive(Debug, Clone)]
pub struct PsdfTile {
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// Tile width in pixels.
    pub width: u32,
    /// Tile height in pixels.
    pub height: u32,
    /// 1-byte-per-pixel PSDF data (`width × height` bytes); `128` = on the edge.
    pub data: Vec<u8>,
    /// Left bearing in pixels (signed).
    pub bearing_x: i32,
    /// Top bearing in pixels (signed).
    pub bearing_y: i32,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
}

// ─── Low-level distance primitives ───────────────────────────────────────────

/// Compute the signed perpendicular distance from `point` to line segment `p0→p1`.
///
/// Positive = left of the directed edge (inside for CCW contours), negative = right.
/// When the projected foot falls outside the segment the distance to the nearest
/// endpoint is returned (unsigned; sign still determined by cross-product).
fn perpendicular_distance_to_segment(point: (f32, f32), p0: (f32, f32), p1: (f32, f32)) -> f32 {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-10 {
        let epx = point.0 - p0.0;
        let epy = point.1 - p0.1;
        return (epx * epx + epy * epy).sqrt();
    }
    let t = ((point.0 - p0.0) * dx + (point.1 - p0.1) * dy) / len2;
    let t_clamped = t.clamp(0.0, 1.0);
    let cx = p0.0 + t_clamped * dx - point.0;
    let cy = p0.1 + t_clamped * dy - point.1;
    let dist = (cx * cx + cy * cy).sqrt();
    // Sign: cross product of edge direction and vector from edge to point.
    // cross = dx*(point.y - p0.y) - dy*(point.x - p0.x)
    let cross = dx * (point.1 - p0.1) - dy * (point.0 - p0.0);
    if cross < 0.0 {
        -dist
    } else {
        dist
    }
}

/// Compute the approximate signed perpendicular distance from `point` to a
/// quadratic Bezier curve `(p0, p1, p2)`.
///
/// The curve is approximated by `N = 8` linear segments, and the minimum
/// perpendicular distance across all of them is returned.
fn perpendicular_distance_to_quad(
    point: (f32, f32),
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
) -> f32 {
    const N: usize = 8;
    let mut min_d = f32::MAX;
    for i in 0..N {
        let t0 = i as f32 / N as f32;
        let t1 = (i + 1) as f32 / N as f32;
        let seg_p0 = eval_quad(p0, p1, p2, t0);
        let seg_p1 = eval_quad(p0, p1, p2, t1);
        let d = perpendicular_distance_to_segment(point, seg_p0, seg_p1);
        if d.abs() < min_d.abs() {
            min_d = d;
        }
    }
    min_d
}

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

/// Compute the approximate signed perpendicular distance from `point` to a
/// cubic Bezier curve `(p0, c1, c2, p1)`.
fn perpendicular_distance_to_cubic(
    point: (f32, f32),
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p1: (f32, f32),
) -> f32 {
    const N: usize = 8;
    let mut min_d = f32::MAX;
    for i in 0..N {
        let t0 = i as f32 / N as f32;
        let t1 = (i + 1) as f32 / N as f32;
        let seg_p0 = eval_cubic(p0, c1, c2, p1, t0);
        let seg_p1 = eval_cubic(p0, c1, c2, p1, t1);
        let d = perpendicular_distance_to_segment(point, seg_p0, seg_p1);
        if d.abs() < min_d.abs() {
            min_d = d;
        }
    }
    min_d
}

// ─── Winding number ───────────────────────────────────────────────────────────

/// Compute the winding number of `point` with respect to `shape`.
///
/// Uses a ray-cast approach: a horizontal ray from `point` to `+∞` is tested
/// against every segment in the shape.  Upward crossings add +1, downward
/// crossings add -1.  A non-zero winding number means the point is inside the
/// glyph (non-zero winding rule).  TTF outer contours are CW in screen coords
/// so `w = -1` inside, but the non-zero test `w != 0` handles both CW and CCW.
fn winding_number(point: (f32, f32), shape: &GlyphShape) -> i32 {
    let mut winding = 0i32;
    let (px, py) = point;

    for contour in &shape.contours {
        for seg in &contour.segments {
            match seg.segment {
                Segment::Line(a, b) => {
                    let ay = a.y as f32;
                    let by = b.y as f32;
                    let ax = a.x as f32;
                    let bx = b.x as f32;
                    if ay <= py {
                        if by > py {
                            // Upward crossing: check if point is left of edge.
                            if cross2d(ax, ay, bx, by, px, py) > 0.0 {
                                winding += 1;
                            }
                        }
                    } else if by <= py {
                        // Downward crossing.
                        if cross2d(ax, ay, bx, by, px, py) < 0.0 {
                            winding -= 1;
                        }
                    }
                }
                Segment::Quad(p0, p1, p2) => {
                    // Approximate the quadratic with N line segments.
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

/// Cross-product sign: (b - a) × (p - a).  Positive = p is left of a→b.
#[inline]
fn cross2d(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// Winding contribution from a quadratic Bezier, approximated by N segments.
fn quad_winding(
    p0: crate::msdf::Point,
    p1: crate::msdf::Point,
    p2: crate::msdf::Point,
    px: f32,
    py: f32,
) -> i32 {
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

/// Winding contribution from a cubic Bezier, approximated by N segments.
fn cubic_winding(
    p0: crate::msdf::Point,
    c1: crate::msdf::Point,
    c2: crate::msdf::Point,
    p1: crate::msdf::Point,
    px: f32,
    py: f32,
) -> i32 {
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

// ─── Minimum unsigned PSDF to any segment ────────────────────────────────────

/// Returns the minimum absolute perpendicular distance from `point` to any
/// segment in `shape`, with sign determined by `winding_number`.
fn psdf_at_point(point: (f32, f32), shape: &GlyphShape) -> f32 {
    let mut min_abs = f32::MAX;

    for contour in &shape.contours {
        for seg in &contour.segments {
            let d = match seg.segment {
                Segment::Line(a, b) => perpendicular_distance_to_segment(
                    point,
                    (a.x as f32, a.y as f32),
                    (b.x as f32, b.y as f32),
                ),
                Segment::Quad(p0, p1, p2) => perpendicular_distance_to_quad(
                    point,
                    (p0.x as f32, p0.y as f32),
                    (p1.x as f32, p1.y as f32),
                    (p2.x as f32, p2.y as f32),
                ),
                Segment::Cubic(p0, c1, c2, p1) => perpendicular_distance_to_cubic(
                    point,
                    (p0.x as f32, p0.y as f32),
                    (c1.x as f32, c1.y as f32),
                    (c2.x as f32, c2.y as f32),
                    (p1.x as f32, p1.y as f32),
                ),
            };
            if d.abs() < min_abs {
                min_abs = d.abs();
            }
        }
    }

    let w = winding_number(point, shape);
    // TTF outer contours are CW in screen-Y-down coords → w = -1 inside, 0 outside.
    // Non-zero winding rule: w != 0 means inside the glyph.
    if w != 0 {
        min_abs
    } else {
        -min_abs
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Compute a PSDF tile for a single glyph.
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
pub fn glyph_to_psdf_tile(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    tile_size: u32,
    spread: f32,
) -> Result<Option<PsdfTile>, SdfError> {
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

    // Bounding box for the glyph (in design units).
    let bbox = face.glyph_bounding_box(gid).ok_or(SdfError::ZeroSize)?;

    let shape_w = (bbox.x_max - bbox.x_min) as f32;
    let shape_h = (bbox.y_max - bbox.y_min) as f32;

    // Scale the shape to fit the tile with a small border equal to `spread`.
    let border_du = spread / scale; // spread in design units
    let fit_scale_x = tile_size as f32 / (shape_w + 2.0 * border_du);
    let fit_scale_y = tile_size as f32 / (shape_h + 2.0 * border_du);
    let fit_scale = fit_scale_x.min(fit_scale_y).max(1e-6);

    let offset_x = bbox.x_min as f32 - border_du;
    let offset_y = bbox.y_min as f32 - border_du;

    let n = tile_size as usize;
    let mut data = vec![0u8; n * n];

    for py in 0..n {
        for px in 0..n {
            // Map pixel centre → shape (design) space.
            // Font Y is up; pixel Y is down — flip the Y axis.
            let sx = px as f32 / fit_scale + offset_x;
            let sy = (n - 1 - py) as f32 / fit_scale + offset_y;
            let signed_dist = psdf_at_point((sx, sy), &shape);
            // Normalize: `spread` design-units ↔ 127 grey levels on each side of 128.
            let spread_du = spread / fit_scale;
            let normalized = 0.5 + signed_dist / (2.0 * spread_du);
            let clamped = normalized.clamp(0.0, 1.0);
            data[py * n + px] = (clamped * 255.0).round() as u8;
        }
    }

    Ok(Some(PsdfTile {
        glyph_id,
        width: tile_size,
        height: tile_size,
        data,
        bearing_x,
        bearing_y,
        advance_x,
    }))
}
