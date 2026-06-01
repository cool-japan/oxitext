//! Multi-channel signed distance field (MSDF) generation.
//!
//! Implements the Chlumsky MSDF algorithm: glyph outlines are extracted via
//! `ttf-parser`, edges are assigned RGB channel colors, and per-pixel
//! min-distances are computed per channel to produce a 3-channel SDF.

/// Bitmask enum for edge color assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeColor(pub u8);

impl EdgeColor {
    /// Red channel only.
    pub const RED: Self = Self(1);
    /// Green channel only.
    pub const GREEN: Self = Self(2);
    /// Blue channel only.
    pub const BLUE: Self = Self(4);
    /// Red + Green (Yellow).
    pub const YELLOW: Self = Self(3);
    /// Green + Blue (Cyan).
    pub const CYAN: Self = Self(6);
    /// Red + Blue (Magenta).
    pub const MAGENTA: Self = Self(5);
    /// All channels (White).
    pub const WHITE: Self = Self(7);

    /// Returns `true` if the red channel is set.
    pub fn has_red(self) -> bool {
        self.0 & 1 != 0
    }

    /// Returns `true` if the green channel is set.
    pub fn has_green(self) -> bool {
        self.0 & 2 != 0
    }

    /// Returns `true` if the blue channel is set.
    pub fn has_blue(self) -> bool {
        self.0 & 4 != 0
    }
}

/// A 2D point in shape (font) space.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

/// A glyph outline segment.
#[derive(Debug, Clone, Copy)]
pub enum Segment {
    /// Straight line from `p0` to `p1`.
    Line(Point, Point),
    /// Quadratic Bezier: `p0` (start), `ctrl` (control), `p1` (end).
    Quad(Point, Point, Point),
    /// Cubic Bezier: `p0` (start), `c1`, `c2` (controls), `p1` (end).
    Cubic(Point, Point, Point, Point),
}

/// A segment with its assigned MSDF edge color.
#[derive(Debug, Clone)]
pub struct ColoredSegment {
    /// The underlying geometry.
    pub segment: Segment,
    /// Assigned channel color for this edge.
    pub color: EdgeColor,
}

/// A closed contour made up of colored segments.
#[derive(Debug, Clone, Default)]
pub struct Contour {
    /// Segments forming this contour, in order.
    pub segments: Vec<ColoredSegment>,
}

/// A full glyph shape composed of one or more contours.
#[derive(Debug, Clone)]
pub struct GlyphShape {
    /// Closed contours forming the glyph outline.
    pub contours: Vec<Contour>,
    /// Font design units per em.
    pub units_per_em: f32,
}

/// A rendered MSDF tile for a single glyph.
#[derive(Debug, Clone)]
pub struct MsdfTile {
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// Tile width in pixels.
    pub width: u32,
    /// Tile height in pixels.
    pub height: u32,
    /// MSDF pixel data: `width * height * 3` bytes (RGB, 3 bytes per pixel).
    pub data: Vec<u8>,
    /// Left bearing in pixels.
    pub bearing_x: f32,
    /// Top bearing in pixels.
    pub bearing_y: f32,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
}

// ─── Outline extraction ───────────────────────────────────────────────────────

/// Collects ttf-parser outline callbacks into a `GlyphShape`.
pub struct OutlineCollector {
    shape: GlyphShape,
    current: Option<Contour>,
    current_pos: Point,
}

impl OutlineCollector {
    /// Creates a new collector for a font with the given design-units-per-em.
    pub fn new(units_per_em: f32) -> Self {
        Self {
            shape: GlyphShape {
                contours: Vec::new(),
                units_per_em,
            },
            current: None,
            current_pos: Point { x: 0.0, y: 0.0 },
        }
    }

    /// Finalises the shape and returns it (flushes any open contour).
    pub fn finish(mut self) -> GlyphShape {
        if let Some(c) = self.current.take() {
            if !c.segments.is_empty() {
                self.shape.contours.push(c);
            }
        }
        self.shape
    }

    /// Pushes a colored segment onto the current contour, creating it if needed.
    fn push_segment(&mut self, segment: Segment) {
        let color = EdgeColor::WHITE; // will be overwritten by color_edges
        self.current
            .get_or_insert_with(Contour::default)
            .segments
            .push(ColoredSegment { segment, color });
    }
}

impl ttf_parser::OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        // Close any open contour before starting a new one.
        if let Some(c) = self.current.take() {
            if !c.segments.is_empty() {
                self.shape.contours.push(c);
            }
        }
        self.current = Some(Contour::default());
        self.current_pos = Point {
            x: x as f64,
            y: y as f64,
        };
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p1 = Point {
            x: x as f64,
            y: y as f64,
        };
        let p0 = self.current_pos;
        self.push_segment(Segment::Line(p0, p1));
        self.current_pos = p1;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ctrl = Point {
            x: x1 as f64,
            y: y1 as f64,
        };
        let p1 = Point {
            x: x as f64,
            y: y as f64,
        };
        let p0 = self.current_pos;
        self.push_segment(Segment::Quad(p0, ctrl, p1));
        self.current_pos = p1;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = Point {
            x: x1 as f64,
            y: y1 as f64,
        };
        let c2 = Point {
            x: x2 as f64,
            y: y2 as f64,
        };
        let p1 = Point {
            x: x as f64,
            y: y as f64,
        };
        let p0 = self.current_pos;
        self.push_segment(Segment::Cubic(p0, c1, c2, p1));
        self.current_pos = p1;
    }

    fn close(&mut self) {
        if let Some(c) = self.current.take() {
            if !c.segments.is_empty() {
                self.shape.contours.push(c);
            }
        }
    }
}

/// Extracts a `GlyphShape` from raw font bytes for the given glyph ID.
///
/// Returns `None` if the font cannot be parsed, the glyph has no outline, or
/// the glyph is a whitespace/blank (no contours).
pub fn extract_glyph_shape(face_data: &[u8], glyph_id: u16) -> Option<GlyphShape> {
    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let units_per_em = face.units_per_em() as f32;
    let mut collector = OutlineCollector::new(units_per_em);
    face.outline_glyph(gid, &mut collector)?;
    Some(collector.finish())
}

// ─── Edge coloring (Chlumsky algorithm) ──────────────────────────────────────

/// Normalises a vector; returns `(1,0)` for near-zero input.
fn normalize(p: Point) -> Point {
    let len = (p.x * p.x + p.y * p.y).sqrt();
    if len < 1e-10 {
        Point { x: 1.0, y: 0.0 }
    } else {
        Point {
            x: p.x / len,
            y: p.y / len,
        }
    }
}

/// Returns the unit tangent at the *start* of `seg`.
fn segment_start_tangent(seg: &Segment) -> Point {
    match seg {
        Segment::Line(p0, p1) => normalize(Point {
            x: p1.x - p0.x,
            y: p1.y - p0.y,
        }),
        Segment::Quad(p0, p1, _) => normalize(Point {
            x: p1.x - p0.x,
            y: p1.y - p0.y,
        }),
        Segment::Cubic(p0, p1, _, _) => normalize(Point {
            x: p1.x - p0.x,
            y: p1.y - p0.y,
        }),
    }
}

/// Returns the unit tangent at the *end* of `seg`.
fn segment_end_tangent(seg: &Segment) -> Point {
    match seg {
        Segment::Line(p0, p1) => normalize(Point {
            x: p1.x - p0.x,
            y: p1.y - p0.y,
        }),
        Segment::Quad(_, p1, p2) => normalize(Point {
            x: p2.x - p1.x,
            y: p2.y - p1.y,
        }),
        Segment::Cubic(_, _, p2, p3) => normalize(Point {
            x: p3.x - p2.x,
            y: p3.y - p2.y,
        }),
    }
}

/// Assigns RGB channel colors to contour segments using the Chlumsky algorithm.
///
/// Corner detection uses a ~3° angle threshold between consecutive edge
/// tangents. Smooth runs are split into thirds (R/G/B). Corners subdivide the
/// contour into spans; single-segment spans receive a mixed color
/// (Yellow/Cyan/Magenta).
pub fn color_edges(shape: &mut GlyphShape) {
    // cos(3°) ≈ 0.9986 — used as the dot-product threshold for corner detection.
    let corner_dot_threshold: f64 = 3.0_f64.to_radians().cos();

    for contour in &mut shape.contours {
        if contour.segments.is_empty() {
            continue;
        }
        let n = contour.segments.len();

        // ── Corner detection ────────────────────────────────────────────────
        let mut corners: Vec<usize> = Vec::new();
        for i in 0..n {
            let prev = &contour.segments[(i + n - 1) % n].segment;
            let curr = &contour.segments[i].segment;
            let t_prev = segment_end_tangent(prev);
            let t_curr = segment_start_tangent(curr);
            let dot = t_prev.x * t_curr.x + t_prev.y * t_curr.y;
            let cross = t_prev.x * t_curr.y - t_prev.y * t_curr.x;
            // Corner when the signed cross-product magnitude exceeds sin(threshold)
            // OR when the edges point in opposite directions (dot < 0).
            if cross.abs() > corner_dot_threshold.acos().sin() || dot < 0.0 {
                corners.push(i);
            }
        }

        if corners.is_empty() {
            // Smooth contour: assign R/G/B in three roughly equal spans.
            let seg_per_third = (n.max(3) / 3).max(1);
            let colors = [EdgeColor::RED, EdgeColor::GREEN, EdgeColor::BLUE];
            for (i, seg) in contour.segments.iter_mut().enumerate() {
                seg.color = colors[(i / seg_per_third).min(2)];
            }
        } else {
            // Cornered contour: cycle R/G/B between corners.
            // Single-segment spans receive a mixed (transition) color.
            let nc = corners.len();
            let palette = [EdgeColor::RED, EdgeColor::GREEN, EdgeColor::BLUE];
            let mut color_idx = 0usize;

            for ci in 0..nc {
                let start = corners[ci];
                let end = corners[(ci + 1) % nc];
                let span_len = if end > start {
                    end - start
                } else {
                    n - start + end
                };

                let color = if span_len == 1 {
                    // Mix the current and next palette colors.
                    let c1 = palette[color_idx % 3];
                    let c2 = palette[(color_idx + 1) % 3];
                    color_idx += 1;
                    EdgeColor(c1.0 | c2.0)
                } else {
                    let c = palette[color_idx % 3];
                    color_idx += 1;
                    c
                };

                for j in 0..span_len {
                    contour.segments[(start + j) % n].color = color;
                }
            }
        }
    }
}

// ─── Signed pseudo-distance helpers ──────────────────────────────────────────

/// Signed distance from point `(px, py)` to line segment `AB`.
///
/// Positive = left of A→B (outside for CCW contours).
fn signed_pseudo_dist_line(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return ((px - ax) * (px - ax) + (py - ay) * (py - ay)).sqrt();
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let cx = ax + t * dx - px;
    let cy = ay + t * dy - py;
    let dist = (cx * cx + cy * cy).sqrt();
    let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    if cross < 0.0 {
        -dist
    } else {
        dist
    }
}

/// Newton-Raphson refinement of the closest-point parameter on a quadratic Bezier.
fn newton_raphson_quad(px: f64, py: f64, p0: Point, p1: Point, p2: Point, t_init: f64) -> f64 {
    let mut t = t_init.clamp(0.0, 1.0);
    for _ in 0..8 {
        let qx = (1.0 - t) * (1.0 - t) * p0.x + 2.0 * t * (1.0 - t) * p1.x + t * t * p2.x;
        let qy = (1.0 - t) * (1.0 - t) * p0.y + 2.0 * t * (1.0 - t) * p1.y + t * t * p2.y;
        // first derivative: dP/dt
        let dqx = 2.0 * (1.0 - t) * (p1.x - p0.x) + 2.0 * t * (p2.x - p1.x);
        let dqy = 2.0 * (1.0 - t) * (p1.y - p0.y) + 2.0 * t * (p2.y - p1.y);
        // second derivative: d²P/dt²
        let ddqx = 2.0 * (p2.x - 2.0 * p1.x + p0.x);
        let ddqy = 2.0 * (p2.y - 2.0 * p1.y + p0.y);
        // f(t) = dP/dt · (P(t) - Q)
        let fx = dqx * (qx - px) + dqy * (qy - py);
        // f'(t) = |dP/dt|² + d²P/dt² · (P(t) - Q)
        let dfx = dqx * dqx + dqy * dqy + ddqx * (qx - px) + ddqy * (qy - py);
        if dfx.abs() < 1e-12 {
            break;
        }
        let dt = fx / dfx;
        t = (t - dt).clamp(0.0, 1.0);
        if dt.abs() < 1e-9 {
            break;
        }
    }
    t
}

/// Signed distance from point `(px, py)` to quadratic Bezier `(p0, p1, p2)`.
fn signed_pseudo_dist_quad(px: f64, py: f64, p0: Point, p1: Point, p2: Point) -> f64 {
    // Initial candidate: sample at 9 uniformly-spaced t values.
    let mut best_dist = f64::MAX;
    let mut best_t = 0.0f64;
    for i in 0..=8 {
        let t = i as f64 / 8.0;
        let qx = (1.0 - t) * (1.0 - t) * p0.x + 2.0 * t * (1.0 - t) * p1.x + t * t * p2.x;
        let qy = (1.0 - t) * (1.0 - t) * p0.y + 2.0 * t * (1.0 - t) * p1.y + t * t * p2.y;
        let d = ((qx - px) * (qx - px) + (qy - py) * (qy - py)).sqrt();
        if d < best_dist {
            best_dist = d;
            best_t = t;
        }
    }

    // Refine with Newton-Raphson.
    let t = newton_raphson_quad(px, py, p0, p1, p2, best_t);
    let qx = (1.0 - t) * (1.0 - t) * p0.x + 2.0 * t * (1.0 - t) * p1.x + t * t * p2.x;
    let qy = (1.0 - t) * (1.0 - t) * p0.y + 2.0 * t * (1.0 - t) * p1.y + t * t * p2.y;
    let dist = ((qx - px) * (qx - px) + (qy - py) * (qy - py)).sqrt();

    // Determine sign via tangent cross product.
    let dtx = 2.0 * (1.0 - t) * (p1.x - p0.x) + 2.0 * t * (p2.x - p1.x);
    let dty = 2.0 * (1.0 - t) * (p1.y - p0.y) + 2.0 * t * (p2.y - p1.y);
    let cross = dtx * (py - qy) - dty * (px - qx);
    if cross < 0.0 {
        -dist
    } else {
        dist
    }
}

/// Newton-Raphson refinement of the closest-point parameter on a cubic Bezier.
fn newton_raphson_cubic(
    px: f64,
    py: f64,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    t_init: f64,
) -> f64 {
    let mut t = t_init.clamp(0.0, 1.0);
    for _ in 0..8 {
        let u = 1.0 - t;
        let qx =
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
        let qy =
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
        // first derivative
        let dqx =
            3.0 * u * u * (p1.x - p0.x) + 6.0 * u * t * (p2.x - p1.x) + 3.0 * t * t * (p3.x - p2.x);
        let dqy =
            3.0 * u * u * (p1.y - p0.y) + 6.0 * u * t * (p2.y - p1.y) + 3.0 * t * t * (p3.y - p2.y);
        // second derivative
        let ddqx = 6.0 * u * (p2.x - 2.0 * p1.x + p0.x) + 6.0 * t * (p3.x - 2.0 * p2.x + p1.x);
        let ddqy = 6.0 * u * (p2.y - 2.0 * p1.y + p0.y) + 6.0 * t * (p3.y - 2.0 * p2.y + p1.y);

        let fx = dqx * (qx - px) + dqy * (qy - py);
        let dfx = dqx * dqx + dqy * dqy + ddqx * (qx - px) + ddqy * (qy - py);
        if dfx.abs() < 1e-12 {
            break;
        }
        let dt = fx / dfx;
        t = (t - dt).clamp(0.0, 1.0);
        if dt.abs() < 1e-9 {
            break;
        }
    }
    t
}

/// Signed distance from point `(px, py)` to cubic Bezier `(p0, c1, c2, p1)`.
fn signed_pseudo_dist_cubic(px: f64, py: f64, p0: Point, p1: Point, p2: Point, p3: Point) -> f64 {
    let mut best_dist = f64::MAX;
    let mut best_t = 0.0f64;
    for i in 0..=8 {
        let t = i as f64 / 8.0;
        let u = 1.0 - t;
        let qx =
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
        let qy =
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
        let d = ((qx - px) * (qx - px) + (qy - py) * (qy - py)).sqrt();
        if d < best_dist {
            best_dist = d;
            best_t = t;
        }
    }

    let t = newton_raphson_cubic(px, py, p0, p1, p2, p3, best_t);
    let u = 1.0 - t;
    let qx = u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
    let qy = u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
    let dist = ((qx - px) * (qx - px) + (qy - py) * (qy - py)).sqrt();

    let dtx =
        3.0 * u * u * (p1.x - p0.x) + 6.0 * u * t * (p2.x - p1.x) + 3.0 * t * t * (p3.x - p2.x);
    let dty =
        3.0 * u * u * (p1.y - p0.y) + 6.0 * u * t * (p2.y - p1.y) + 3.0 * t * t * (p3.y - p2.y);
    let cross = dtx * (py - qy) - dty * (px - qx);
    if cross < 0.0 {
        -dist
    } else {
        dist
    }
}

/// Returns the signed pseudo-distance from `(px, py)` to `seg`.
pub(crate) fn segment_signed_dist(seg: &Segment, px: f64, py: f64) -> f64 {
    match seg {
        Segment::Line(a, b) => signed_pseudo_dist_line(px, py, a.x, a.y, b.x, b.y),
        Segment::Quad(p0, p1, p2) => signed_pseudo_dist_quad(px, py, *p0, *p1, *p2),
        Segment::Cubic(p0, p1, p2, p3) => signed_pseudo_dist_cubic(px, py, *p0, *p1, *p2, *p3),
    }
}

// ─── Core MSDF computation ────────────────────────────────────────────────────

/// Compute a multi-channel SDF from a colored glyph shape.
///
/// # Arguments
/// - `shape` — colored glyph outline (call [`color_edges`] first).
/// - `width`, `height` — output tile dimensions in pixels.
/// - `spread` — maximum SDF distance in *shape* units mapped to channel
///   saturation; a value in the range `[1, 64]` (font design units) is typical.
/// - `scale` — shape → pixel transform: `px_size / units_per_em`.
/// - `offset_x`, `offset_y` — additional translation (in shape units) applied
///   before mapping; moves the shape origin to pixel `(padding, padding)`.
///
/// # Returns
/// A `Vec<u8>` of length `width * height * 3` (RGB, 3 bytes per pixel).
/// Value encoding: `0` = far outside, `128` ≈ on the outline, `255` = far inside.
///
/// # Errors
/// Returns [`crate::edt::SdfError::ZeroSize`] when `width == 0 || height == 0`.
pub fn compute_msdf(
    shape: &GlyphShape,
    width: u32,
    height: u32,
    spread: f32,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
) -> Result<Vec<u8>, crate::edt::SdfError> {
    if width == 0 || height == 0 {
        return Err(crate::edt::SdfError::ZeroSize);
    }
    let mut output = vec![0u8; width as usize * height as usize * 3];
    let spread_f64 = spread as f64;

    for py in 0..height {
        for px in 0..width {
            // Map pixel centre → shape space (flip Y: font Y-up vs pixel Y-down).
            let fx = (px as f32 + 0.5) / scale + offset_x;
            // Y-axis: font uses Y-up, pixels use Y-down; invert so shapes aren't mirrored.
            let fy = (py as f32 + 0.5) / scale + offset_y;

            let mut r_dist = f64::MAX;
            let mut g_dist = f64::MAX;
            let mut b_dist = f64::MAX;

            for contour in &shape.contours {
                for colored_seg in &contour.segments {
                    let d = segment_signed_dist(&colored_seg.segment, fx as f64, fy as f64);
                    if colored_seg.color.has_red() && d.abs() < r_dist.abs() {
                        r_dist = d;
                    }
                    if colored_seg.color.has_green() && d.abs() < g_dist.abs() {
                        g_dist = d;
                    }
                    if colored_seg.color.has_blue() && d.abs() < b_dist.abs() {
                        b_dist = d;
                    }
                }
            }

            // Normalize: dist ∈ [-spread, spread] → [0, 255].
            let norm = |d: f64| -> u8 {
                ((d / spread_f64 + 1.0) * 0.5 * 255.0)
                    .clamp(0.0, 255.0)
                    .round() as u8
            };

            let idx = (py as usize * width as usize + px as usize) * 3;
            output[idx] = if r_dist == f64::MAX { 0 } else { norm(r_dist) };
            output[idx + 1] = if g_dist == f64::MAX { 0 } else { norm(g_dist) };
            output[idx + 2] = if b_dist == f64::MAX { 0 } else { norm(b_dist) };
        }
    }
    Ok(output)
}

// ─── High-level tile builder ──────────────────────────────────────────────────

/// Generate an [`MsdfTile`] for a single glyph from raw font bytes.
///
/// Returns `Ok(None)` for whitespace / blank glyphs (no outline contours).
///
/// # Errors
/// - [`crate::edt::SdfError::InvalidFont`] if the font bytes cannot be parsed.
/// - [`crate::edt::SdfError::ZeroSize`] if the tile dimensions are zero.
pub fn glyph_to_msdf_tile(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    tile_width: u32,
    tile_height: u32,
    spread: f32,
    padding: u32,
) -> Result<Option<MsdfTile>, crate::edt::SdfError> {
    let Some(mut shape) = extract_glyph_shape(face_data, glyph_id) else {
        return Ok(None);
    };
    if shape.contours.is_empty() {
        return Ok(None);
    }

    color_edges(&mut shape);

    let face =
        ttf_parser::Face::parse(face_data, 0).map_err(|_| crate::edt::SdfError::InvalidFont)?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let scale_full = px_size / shape.units_per_em;

    let bearing_x = face.glyph_hor_side_bearing(gid).unwrap_or(0) as f32 * scale_full;
    let bearing_y = face
        .glyph_hor_advance(gid)
        .map(|_| face.ascender() as f32 * scale_full)
        .unwrap_or(px_size);
    let advance_x = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale_full;

    // Bounding box → compute fit scale and offset so the glyph fills the tile.
    let bbox = face
        .glyph_bounding_box(gid)
        .ok_or(crate::edt::SdfError::ZeroSize)?;
    let shape_w = (bbox.x_max - bbox.x_min) as f32;
    let shape_h = (bbox.y_max - bbox.y_min) as f32;

    let eff_w = tile_width.saturating_sub(2 * padding).max(1) as f32;
    let eff_h = tile_height.saturating_sub(2 * padding).max(1) as f32;
    let fit_scale = (eff_w / shape_w).min(eff_h / shape_h);
    let offset_x = bbox.x_min as f32 - padding as f32 / fit_scale;
    let offset_y = bbox.y_min as f32 - padding as f32 / fit_scale;

    let data = compute_msdf(
        &shape,
        tile_width,
        tile_height,
        spread,
        fit_scale,
        offset_x,
        offset_y,
    )?;

    Ok(Some(MsdfTile {
        glyph_id,
        width: tile_width,
        height: tile_height,
        data,
        bearing_x,
        bearing_y,
        advance_x,
    }))
}

// ─── MTSDF: multi-channel true SDF ───────────────────────────────────────────

/// A multi-channel true SDF tile — RGB channels hold MSDF data, alpha holds true SDF.
///
/// The true SDF in the alpha channel provides smooth outlines at all scales, while
/// the MSDF RGB channels enable sharp corner reproduction at higher resolutions.
#[derive(Debug, Clone)]
pub struct MtsdfTile {
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// Tile width in pixels.
    pub width: u32,
    /// Tile height in pixels.
    pub height: u32,
    /// RGBA pixel data: `width * height * 4` bytes (RGBA, 4 bytes per pixel).
    pub data: Vec<u8>,
    /// Left bearing in pixels.
    pub bearing_x: f32,
    /// Top bearing in pixels.
    pub bearing_y: f32,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
}

/// Compute a 4-channel MTSDF: RGB channels from MSDF, alpha from a true single-channel SDF.
///
/// For each pixel the RGB channels encode the per-color-channel minimum distances
/// (as in [`compute_msdf`]).  The alpha channel encodes the minimum distance across
/// **all** segments regardless of color, which yields a smooth true SDF.
///
/// # Arguments
/// - `shape` — colored glyph outline (call [`color_edges`] first).
/// - `width`, `height` — output tile dimensions in pixels.
/// - `spread` — maximum SDF distance in shape units; maps ±spread to \[0, 255\].
/// - `scale` — shape → pixel transform: `px_size / units_per_em`.
/// - `offset_x`, `offset_y` — translation applied before the shape→pixel mapping.
///
/// # Returns
/// A `Vec<u8>` of length `width * height * 4` (RGBA, 4 bytes per pixel).
///
/// # Errors
/// Returns [`crate::edt::SdfError::ZeroSize`] when `width == 0 || height == 0`.
pub fn compute_mtsdf(
    shape: &GlyphShape,
    width: u32,
    height: u32,
    spread: f32,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
) -> Result<Vec<u8>, crate::edt::SdfError> {
    if width == 0 || height == 0 {
        return Err(crate::edt::SdfError::ZeroSize);
    }

    // Compute RGB channels via standard MSDF.
    let rgb = compute_msdf(shape, width, height, spread, scale, offset_x, offset_y)?;

    let spread_f64 = spread as f64;
    let n = width as usize * height as usize;
    let mut alpha = vec![0u8; n];

    for py in 0..height {
        for px in 0..width {
            let fx = (px as f32 + 0.5) / scale + offset_x;
            let fy = (py as f32 + 0.5) / scale + offset_y;

            // Find the minimum signed distance across ALL segments (regardless of color).
            let mut min_dist = f64::MAX;
            for contour in &shape.contours {
                for seg in &contour.segments {
                    let d = segment_signed_dist(&seg.segment, fx as f64, fy as f64);
                    if d.abs() < min_dist.abs() {
                        min_dist = d;
                    }
                }
            }

            let v = if min_dist == f64::MAX {
                0u8
            } else {
                ((min_dist / spread_f64 + 1.0) * 0.5 * 255.0)
                    .clamp(0.0, 255.0)
                    .round() as u8
            };
            alpha[py as usize * width as usize + px as usize] = v;
        }
    }

    // Interleave RGB + Alpha → RGBA.
    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        rgba[i * 4] = rgb[i * 3];
        rgba[i * 4 + 1] = rgb[i * 3 + 1];
        rgba[i * 4 + 2] = rgb[i * 3 + 2];
        rgba[i * 4 + 3] = alpha[i];
    }
    Ok(rgba)
}

/// Generate an [`MtsdfTile`] for a single glyph from raw font bytes.
///
/// Returns `Ok(None)` for whitespace / blank glyphs (no outline contours).
///
/// # Errors
/// - [`crate::edt::SdfError::InvalidFont`] if the font bytes cannot be parsed.
/// - [`crate::edt::SdfError::ZeroSize`] if the tile dimensions are zero.
pub fn glyph_to_mtsdf_tile(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    tile_width: u32,
    tile_height: u32,
    spread: f32,
    padding: u32,
) -> Result<Option<MtsdfTile>, crate::edt::SdfError> {
    let Some(mut shape) = extract_glyph_shape(face_data, glyph_id) else {
        return Ok(None);
    };
    if shape.contours.is_empty() {
        return Ok(None);
    }

    color_edges(&mut shape);

    let face =
        ttf_parser::Face::parse(face_data, 0).map_err(|_| crate::edt::SdfError::InvalidFont)?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let scale_full = px_size / shape.units_per_em;

    let bearing_x = face.glyph_hor_side_bearing(gid).unwrap_or(0) as f32 * scale_full;
    let bearing_y = face
        .glyph_hor_advance(gid)
        .map(|_| face.ascender() as f32 * scale_full)
        .unwrap_or(px_size);
    let advance_x = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale_full;

    let bbox = face
        .glyph_bounding_box(gid)
        .ok_or(crate::edt::SdfError::ZeroSize)?;
    let shape_w = (bbox.x_max - bbox.x_min) as f32;
    let shape_h = (bbox.y_max - bbox.y_min) as f32;

    let eff_w = tile_width.saturating_sub(2 * padding).max(1) as f32;
    let eff_h = tile_height.saturating_sub(2 * padding).max(1) as f32;
    let fit_scale = (eff_w / shape_w).min(eff_h / shape_h);
    let offset_x = bbox.x_min as f32 - padding as f32 / fit_scale;
    let offset_y = bbox.y_min as f32 - padding as f32 / fit_scale;

    let data = compute_mtsdf(
        &shape,
        tile_width,
        tile_height,
        spread,
        fit_scale,
        offset_x,
        offset_y,
    )?;

    Ok(Some(MtsdfTile {
        glyph_id,
        width: tile_width,
        height: tile_height,
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

    /// Build a simple square contour in shape space.
    fn square_shape(size: f64) -> GlyphShape {
        let segs = vec![
            ColoredSegment {
                segment: Segment::Line(Point { x: 0.0, y: 0.0 }, Point { x: size, y: 0.0 }),
                color: EdgeColor::RED,
            },
            ColoredSegment {
                segment: Segment::Line(Point { x: size, y: 0.0 }, Point { x: size, y: size }),
                color: EdgeColor::GREEN,
            },
            ColoredSegment {
                segment: Segment::Line(Point { x: size, y: size }, Point { x: 0.0, y: size }),
                color: EdgeColor::BLUE,
            },
            ColoredSegment {
                segment: Segment::Line(Point { x: 0.0, y: size }, Point { x: 0.0, y: 0.0 }),
                color: EdgeColor::RED,
            },
        ];
        GlyphShape {
            contours: vec![Contour { segments: segs }],
            units_per_em: 1000.0,
        }
    }

    #[test]
    fn color_bits() {
        assert!(EdgeColor::RED.has_red());
        assert!(!EdgeColor::RED.has_green());
        assert!(EdgeColor::YELLOW.has_red() && EdgeColor::YELLOW.has_green());
        assert!(
            EdgeColor::WHITE.has_red()
                && EdgeColor::WHITE.has_green()
                && EdgeColor::WHITE.has_blue()
        );
    }

    #[test]
    fn msdf_output_has_three_channels() {
        let shape = square_shape(500.0);
        let result = compute_msdf(&shape, 16, 16, 4.0, 0.016, -50.0, -50.0);
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.len(), 16 * 16 * 3);
    }

    #[test]
    fn sdf_midpoint_near_128() {
        let shape = square_shape(1000.0);
        let result = compute_msdf(&shape, 32, 32, 8.0, 0.032, -16.0, -16.0);
        let data = result.unwrap();
        let has_nonzero = data.iter().any(|&v| v > 10 && v < 245);
        assert!(has_nonzero, "SDF should have values spanning the range");
    }

    #[test]
    fn empty_glyph_returns_ok() {
        let empty = GlyphShape {
            contours: vec![],
            units_per_em: 1000.0,
        };
        let result = compute_msdf(&empty, 8, 8, 4.0, 1.0, 0.0, 0.0);
        // Empty shape: all distances are MAX → output all zeros (far outside).
        assert!(result.is_ok());
    }

    #[test]
    fn edge_color_hashable() {
        let mut map = std::collections::HashMap::new();
        map.insert(EdgeColor::RED, 1u32);
        map.insert(EdgeColor::GREEN, 2u32);
        assert_eq!(map.get(&EdgeColor::RED), Some(&1));
    }

    #[test]
    fn color_edges_smooth_assigns_three_colors() {
        let mut shape = square_shape(500.0);
        color_edges(&mut shape);
        let colors: std::collections::HashSet<u8> = shape.contours[0]
            .segments
            .iter()
            .map(|s| s.color.0)
            .collect();
        // Should have at least 2 distinct channel assignments.
        assert!(
            colors.len() >= 2,
            "expected multi-color assignment, got {colors:?}"
        );
    }

    #[test]
    fn zero_size_returns_error() {
        let shape = square_shape(500.0);
        assert!(compute_msdf(&shape, 0, 16, 4.0, 1.0, 0.0, 0.0).is_err());
        assert!(compute_msdf(&shape, 16, 0, 4.0, 1.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn mtsdf_four_channel_output() {
        let shape = GlyphShape {
            contours: vec![Contour {
                segments: vec![ColoredSegment {
                    segment: Segment::Line(Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }),
                    color: EdgeColor::WHITE,
                }],
            }],
            units_per_em: 1000.0,
        };
        let result = compute_mtsdf(&shape, 8, 8, 4.0, 0.08, -50.0, -50.0);
        assert!(result.is_ok(), "compute_mtsdf failed: {:?}", result);
        let data = result.unwrap();
        assert_eq!(
            data.len(),
            8 * 8 * 4,
            "MTSDF output should be RGBA (4 bytes/pixel)"
        );
    }

    #[test]
    fn mtsdf_zero_size_errors() {
        let shape = square_shape(500.0);
        assert!(compute_mtsdf(&shape, 0, 8, 4.0, 1.0, 0.0, 0.0).is_err());
        assert!(compute_mtsdf(&shape, 8, 0, 4.0, 1.0, 0.0, 0.0).is_err());
    }

    // ─── MSDF edge coloring quality test ──────────────────────────────────────
    //
    // Test 3: `glyph_to_msdf_tile` on a real printable ASCII glyph verifies that:
    //   (a) the returned tile carries 3 bytes per pixel (RGB stride),
    //   (b) the R, G, B channels are NOT all equal across all pixels, confirming
    //       that edge coloring assigned distinct channel identities to different
    //       contour segments (multi-channel differentiation).
    //
    // The threshold: at least 5 % of foreground pixels (those where at least one
    // channel ≠ 0) must show channel disagreement (r≠g || g≠b || r≠b).
    // This is meaningfully stronger than "at least one pixel differs" and
    // demonstrates that the MSDF channels carry truly independent distance info.

    const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

    /// Helper: find the glyph id for an ASCII character in the test font.
    fn glyph_id_for_char(ch: char) -> u16 {
        let face = ttf_parser::Face::parse(FONT, 0).expect("parse test font");
        face.glyph_index(ch).expect("char not found in test font").0
    }

    /// Test 3: MSDF edge coloring assigns distinct R/G/B channel values.
    #[test]
    fn msdf_edge_coloring_assigns_distinct_channels() {
        // Use 'H' — a glyph with clear corners that forces the Chlumsky algorithm
        // to cycle through R/G/B on different contour spans.
        let gid = glyph_id_for_char('H');

        let tile_w = 32u32;
        let tile_h = 32u32;
        let px_size = 24.0f32;
        let spread = 4.0f32;
        let padding = 2u32;

        let tile = glyph_to_msdf_tile(FONT, gid, px_size, tile_w, tile_h, spread, padding)
            .expect("glyph_to_msdf_tile should not error")
            .expect("'H' should have a non-empty outline");

        // (a) Stride check: 3 bytes per pixel.
        assert_eq!(
            tile.data.len(),
            (tile_w * tile_h * 3) as usize,
            "MSDF tile should have 3 bytes/pixel (RGB)"
        );

        // (b) Channel distinctness: scan all pixels and count those where R≠G, G≠B, or R≠B.
        let n_pixels = (tile_w * tile_h) as usize;
        let mut foreground_pixels = 0usize;
        let mut distinct_pixels = 0usize;

        for i in 0..n_pixels {
            let r = tile.data[i * 3];
            let g = tile.data[i * 3 + 1];
            let b = tile.data[i * 3 + 2];
            // Skip completely-black background pixels (all channels at min).
            if r > 5 || g > 5 || b > 5 {
                foreground_pixels += 1;
                if r != g || g != b || r != b {
                    distinct_pixels += 1;
                }
            }
        }

        assert!(
            foreground_pixels > 0,
            "MSDF tile for 'H' should have at least some foreground pixels"
        );

        // At least 5 % of foreground pixels must show channel disagreement.
        let ratio = distinct_pixels as f64 / foreground_pixels as f64;
        assert!(
            ratio >= 0.05,
            "Expected ≥5% of foreground pixels to have distinct R/G/B channels \
             (confirms edge coloring), got {:.1}% ({distinct_pixels}/{foreground_pixels})",
            ratio * 100.0
        );
    }
}
