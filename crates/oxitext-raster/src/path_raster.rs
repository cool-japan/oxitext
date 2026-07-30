//! Affine transforms and an anti-aliased scanline coverage rasterizer.
//!
//! This module exists because COLR layer glyphs cannot be rasterized through
//! fontdue: `fontdue::Font` only materialises the glyphs that are reachable
//! from the font's `cmap` (plus, optionally, `GSUB`), and COLR layer glyphs are
//! deliberately *not* mapped from any codepoint.  Asking fontdue for such a GID
//! yields a 0x0 bitmap, which is why the COLR painter used to emit fully
//! transparent output for every real colour font.
//!
//! The rasterizer here works directly on [`ttf_parser`] outlines (so `glyf`,
//! `CFF` and `CFF2` are all covered) and applies an arbitrary affine transform
//! while flattening, which is what COLRv1's `PaintTransform` family requires.
//!
//! Coverage is computed with the signed-area accumulation technique (the same
//! approach used by `font-rs` and, internally, by fontdue): every edge deposits
//! a signed area delta into a scanline buffer, and a per-row prefix sum turns
//! those deltas into non-zero-winding coverage in `[0, 1]`.

use ttf_parser::{Face, GlyphId, OutlineBuilder, RectF};

/// Maximum number of line segments a single Bézier curve is flattened into.
///
/// Guards against pathological transforms (e.g. a `PaintScale` with a huge
/// factor) turning one curve into an unbounded amount of work.
const MAX_CURVE_SEGMENTS: usize = 128;

/// Squared device-space deviation below which a curve is drawn as a
/// single straight segment.
const FLAT_TOLERANCE_SQ: f32 = 0.1;

/// A 2-D affine transform in the same component order as
/// [`ttf_parser::Transform`].
///
/// A point is mapped as `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine {
    /// Row 0, column 0.
    pub a: f32,
    /// Row 1, column 0.
    pub b: f32,
    /// Row 0, column 1.
    pub c: f32,
    /// Row 1, column 1.
    pub d: f32,
    /// Horizontal translation.
    pub e: f32,
    /// Vertical translation.
    pub f: f32,
}

impl Affine {
    /// The identity transform.
    pub(crate) const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Build a transform from its six components.
    pub(crate) const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Affine { a, b, c, d, e, f }
    }

    /// Convert a [`ttf_parser::Transform`] (as delivered by the COLR painter
    /// callbacks) into an [`Affine`].
    pub(crate) fn from_ttf(t: ttf_parser::Transform) -> Self {
        Affine::new(t.a, t.b, t.c, t.d, t.e, t.f)
    }

    /// Compose two transforms: the result applies `inner` first, then `outer`.
    pub(crate) fn concat(outer: Affine, inner: Affine) -> Self {
        Affine {
            a: outer.a * inner.a + outer.c * inner.b,
            b: outer.b * inner.a + outer.d * inner.b,
            c: outer.a * inner.c + outer.c * inner.d,
            d: outer.b * inner.c + outer.d * inner.d,
            e: outer.a * inner.e + outer.c * inner.f + outer.e,
            f: outer.b * inner.e + outer.d * inner.f + outer.f,
        }
    }

    /// Map a point through the transform.
    #[inline]
    pub(crate) fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Invert the transform, or return `None` when it is singular.
    pub(crate) fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        let a = self.d * inv_det;
        let b = -self.b * inv_det;
        let c = -self.c * inv_det;
        let d = self.a * inv_det;
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }
}

/// A per-pixel coverage mask in `[0.0, 1.0]`, laid out row-major.
#[derive(Debug, Clone)]
pub(crate) struct CoverageMask {
    /// Mask width in pixels.
    pub width: u32,
    /// Mask height in pixels.
    pub height: u32,
    /// `width * height` coverage samples.
    pub data: Vec<f32>,
}

impl CoverageMask {
    /// A mask that covers every pixel completely.
    pub(crate) fn opaque(width: u32, height: u32) -> Self {
        CoverageMask {
            width,
            height,
            data: vec![1.0; (width as usize) * (height as usize)],
        }
    }

    /// Intersect this mask with `other` by multiplying coverage per pixel.
    ///
    /// Masks of differing sizes are left untouched; all masks produced inside a
    /// single render share the target bitmap's dimensions, so this is a
    /// defensive no-op rather than a reachable state.
    pub(crate) fn intersect(&mut self, other: &CoverageMask) {
        if self.width != other.width || self.height != other.height {
            return;
        }
        for (dst, src) in self.data.iter_mut().zip(other.data.iter()) {
            *dst *= *src;
        }
    }

    /// `true` when no pixel has any coverage.
    pub(crate) fn is_blank(&self) -> bool {
        self.data.iter().all(|&v| v <= 0.0)
    }
}

/// Accumulates signed edge areas and turns them into a [`CoverageMask`].
///
/// Path input is given in *user* coordinates and mapped through `transform`
/// before being flattened, so curve subdivision density is chosen from the
/// on-screen size rather than the design-unit size.
pub(crate) struct MaskRasterizer {
    width: usize,
    height: usize,
    /// Row stride of [`Self::acc`]; two columns wider than `width` so the
    /// `x0 + 1` / `x1` writes of the accumulation kernel always land in-bounds.
    stride: usize,
    acc: Vec<f32>,
    transform: Affine,
    /// First point of the current sub-path, in device space.
    start: (f32, f32),
    /// Current pen position, in device space.
    cur: (f32, f32),
    /// `true` once a `move_to` has established a sub-path.
    open: bool,
}

impl MaskRasterizer {
    /// Create a rasterizer targeting a `width` x `height` mask, mapping input
    /// coordinates through `transform`.
    pub(crate) fn new(width: u32, height: u32, transform: Affine) -> Self {
        let width = width as usize;
        let height = height as usize;
        let stride = width + 2;
        MaskRasterizer {
            width,
            height,
            stride,
            acc: vec![0.0; stride * height],
            transform,
            start: (0.0, 0.0),
            cur: (0.0, 0.0),
            open: false,
        }
    }

    /// Add a signed value into the accumulation buffer, ignoring out-of-range
    /// indices instead of panicking.
    #[inline]
    fn bump(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.acc.get_mut(index) {
            *slot += value;
        }
    }

    /// Deposit the signed area of a device-space line segment.
    ///
    /// Horizontal extents are clamped into `[0, width]`: geometry to the left
    /// of the mask still has to contribute its winding to column 0, and
    /// geometry to the right must not contribute to any visible column.
    fn add_line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if !(p0.0.is_finite() && p0.1.is_finite() && p1.0.is_finite() && p1.1.is_finite()) {
            return;
        }
        if (p0.1 - p1.1).abs() <= f32::EPSILON {
            return;
        }
        let (dir, top, bottom) = if p0.1 < p1.1 {
            (1.0_f32, p0, p1)
        } else {
            (-1.0_f32, p1, p0)
        };

        let dxdy = (bottom.0 - top.0) / (bottom.1 - top.1);
        if !dxdy.is_finite() {
            return;
        }

        // Advance the pen to the first visible scanline.
        let mut x = if top.1 < 0.0 {
            top.0 - top.1 * dxdy
        } else {
            top.0
        };

        let y_first = top.1.max(0.0);
        if y_first >= self.height as f32 {
            return;
        }
        let y_start = y_first as usize;
        let y_stop = {
            let ceil = bottom.1.ceil();
            if ceil <= 0.0 {
                return;
            }
            (ceil as usize).min(self.height)
        };

        let max_x = self.width as f32;
        for y in y_start..y_stop {
            let row = y * self.stride;
            let dy = ((y + 1) as f32).min(bottom.1) - (y as f32).max(top.1);
            if dy <= 0.0 {
                continue;
            }
            let xnext = x + dxdy * dy;
            let d = dy * dir;

            let (lo, hi) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0 = lo.clamp(0.0, max_x);
            let x1 = hi.clamp(0.0, max_x);

            let x0_floor = x0.floor();
            let x0i = x0_floor as usize;
            let x1_ceil = x1.ceil();
            let x1i = x1_ceil as usize;

            if x1i <= x0i + 1 {
                // The segment spans at most one pixel on this scanline.
                let xmf = 0.5 * (x0 + x1) - x0_floor;
                self.bump(row + x0i, d - d * xmf);
                self.bump(row + x0i + 1, d * xmf);
            } else {
                let span = x1 - x0;
                let s = 1.0 / span;
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.bump(row + x0i, d * a0);
                if x1i == x0i + 2 {
                    self.bump(row + x0i + 1, d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.bump(row + x0i + 1, d * (a1 - a0));
                    for xi in (x0i + 2)..(x1i - 1) {
                        self.bump(row + xi, d * s);
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.bump(row + x1i - 1, d * (1.0 - a2 - am));
                }
                self.bump(row + x1i, d * am);
            }

            x = xnext;
        }
    }

    /// Close the sub-path currently being built, if any.
    fn close_subpath(&mut self) {
        if self.open {
            let (start, cur) = (self.start, self.cur);
            self.add_line(cur, start);
            self.cur = start;
        }
    }

    /// Append a device-space line and advance the pen.
    #[inline]
    fn emit_to(&mut self, p: (f32, f32)) {
        let cur = self.cur;
        self.add_line(cur, p);
        self.cur = p;
    }

    /// Number of flattening steps for a curve whose largest second-difference
    /// magnitude (in device pixels) is `deviation`.
    ///
    /// The chord error of an `n`-segment approximation scales with
    /// `deviation / n^2`; `factor` folds the polynomial degree and the target
    /// error (0.1 px) into a single constant.
    fn segment_count(deviation_sq: f32, factor: f32) -> usize {
        if !deviation_sq.is_finite() || deviation_sq < FLAT_TOLERANCE_SQ {
            return 1;
        }
        let n = (factor * deviation_sq.sqrt()).sqrt().ceil();
        if !n.is_finite() || n < 1.0 {
            return 1;
        }
        (n as usize).clamp(1, MAX_CURVE_SEGMENTS)
    }

    /// Finish the path and produce the coverage mask.
    pub(crate) fn finish(mut self) -> CoverageMask {
        self.close_subpath();
        let mut data = vec![0.0_f32; self.width * self.height];
        for y in 0..self.height {
            let acc_row = y * self.stride;
            let out_row = y * self.width;
            let mut running = 0.0_f32;
            for x in 0..self.width {
                running += self.acc[acc_row + x];
                data[out_row + x] = running.abs().min(1.0);
            }
        }
        CoverageMask {
            width: self.width as u32,
            height: self.height as u32,
            data,
        }
    }

    /// Rasterize an axis-aligned rectangle given in user coordinates.
    ///
    /// The transform may rotate or skew it, so all four corners are emitted as
    /// a closed quadrilateral rather than a scanline span.
    pub(crate) fn add_rect(&mut self, rect: RectF) {
        self.move_to(rect.x_min, rect.y_min);
        self.line_to(rect.x_max, rect.y_min);
        self.line_to(rect.x_max, rect.y_max);
        self.line_to(rect.x_min, rect.y_max);
        self.close();
    }
}

impl OutlineBuilder for MaskRasterizer {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_subpath();
        let p = self.transform.apply(x, y);
        self.start = p;
        self.cur = p;
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.transform.apply(x, y);
        self.emit_to(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.cur;
        let p1 = self.transform.apply(x1, y1);
        let p2 = self.transform.apply(x, y);

        let dev_x = p0.0 - 2.0 * p1.0 + p2.0;
        let dev_y = p0.1 - 2.0 * p1.1 + p2.1;
        // Second derivative of a quadratic is 2*dev; error ~= |dev| / (4 n^2),
        // so n >= sqrt(2.5 * |dev|) keeps the chord error under 0.1 px.
        let n = Self::segment_count(dev_x * dev_x + dev_y * dev_y, 2.5);
        if n == 1 {
            self.emit_to(p2);
            return;
        }

        let step = 1.0 / n as f32;
        let mut t = step;
        for _ in 1..n {
            let mt = 1.0 - t;
            let px = mt * mt * p0.0 + 2.0 * mt * t * p1.0 + t * t * p2.0;
            let py = mt * mt * p0.1 + 2.0 * mt * t * p1.1 + t * t * p2.1;
            self.emit_to((px, py));
            t += step;
        }
        self.emit_to(p2);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.cur;
        let p1 = self.transform.apply(x1, y1);
        let p2 = self.transform.apply(x2, y2);
        let p3 = self.transform.apply(x, y);

        let d1x = p0.0 - 2.0 * p1.0 + p2.0;
        let d1y = p0.1 - 2.0 * p1.1 + p2.1;
        let d2x = p1.0 - 2.0 * p2.0 + p3.0;
        let d2y = p1.1 - 2.0 * p2.1 + p3.1;
        let dev_sq = (d1x * d1x + d1y * d1y).max(d2x * d2x + d2y * d2y);
        // Second derivative of a cubic peaks at 6*dev; error ~= 0.75*|dev|/n^2,
        // so n >= sqrt(7.5 * |dev|) keeps the chord error under 0.1 px.
        let n = Self::segment_count(dev_sq, 7.5);
        if n == 1 {
            self.emit_to(p3);
            return;
        }

        let step = 1.0 / n as f32;
        let mut t = step;
        for _ in 1..n {
            let mt = 1.0 - t;
            let w0 = mt * mt * mt;
            let w1 = 3.0 * mt * mt * t;
            let w2 = 3.0 * mt * t * t;
            let w3 = t * t * t;
            let px = w0 * p0.0 + w1 * p1.0 + w2 * p2.0 + w3 * p3.0;
            let py = w0 * p0.1 + w1 * p1.1 + w2 * p2.1 + w3 * p3.1;
            self.emit_to((px, py));
            t += step;
        }
        self.emit_to(p3);
    }

    fn close(&mut self) {
        self.close_subpath();
    }
}

/// Rasterize a glyph outline into a coverage mask.
///
/// Returns `None` when the glyph has no outline at all (whitespace, or a GID
/// outside the font); an outline that falls entirely outside the mask still
/// yields `Some` with a blank mask.
pub(crate) fn rasterize_glyph_mask(
    face: &Face<'_>,
    glyph_id: GlyphId,
    transform: Affine,
    width: u32,
    height: u32,
) -> Option<CoverageMask> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut rasterizer = MaskRasterizer::new(width, height, transform);
    face.outline_glyph(glyph_id, &mut rasterizer)?;
    Some(rasterizer.finish())
}

/// Rasterize a rectangle (a COLRv1 clip box) into a coverage mask.
pub(crate) fn rasterize_rect_mask(
    rect: RectF,
    transform: Affine,
    width: u32,
    height: u32,
) -> CoverageMask {
    let mut rasterizer = MaskRasterizer::new(width, height, transform);
    rasterizer.add_rect(rect);
    rasterizer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage_sum(mask: &CoverageMask) -> f32 {
        mask.data.iter().sum()
    }

    #[test]
    fn identity_round_trips_points() {
        let (x, y) = Affine::IDENTITY.apply(3.5, -2.25);
        assert!((x - 3.5).abs() < 1e-6);
        assert!((y + 2.25).abs() < 1e-6);
    }

    #[test]
    fn concat_applies_inner_first() {
        let scale = Affine::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
        let translate = Affine::new(1.0, 0.0, 0.0, 1.0, 10.0, 0.0);
        // Scale first, then translate.
        let combined = Affine::concat(translate, scale);
        let (x, y) = combined.apply(1.0, 1.0);
        assert!((x - 12.0).abs() < 1e-6, "x = {x}");
        assert!((y - 2.0).abs() < 1e-6, "y = {y}");
    }

    #[test]
    fn invert_round_trips() {
        let t = Affine::new(2.0, 0.5, -0.25, 3.0, 7.0, -4.0);
        let inv = t.invert().expect("non-singular");
        let (x, y) = t.apply(1.5, -2.5);
        let (rx, ry) = inv.apply(x, y);
        assert!((rx - 1.5).abs() < 1e-4, "rx = {rx}");
        assert!((ry + 2.5).abs() < 1e-4, "ry = {ry}");
    }

    #[test]
    fn invert_rejects_singular() {
        assert!(Affine::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).invert().is_none());
    }

    #[test]
    fn axis_aligned_rect_covers_expected_area() {
        let rect = RectF {
            x_min: 2.0,
            y_min: 2.0,
            x_max: 6.0,
            y_max: 6.0,
        };
        let mask = rasterize_rect_mask(rect, Affine::IDENTITY, 8, 8);
        // A 4x4 rectangle inside an 8x8 mask.
        let sum = coverage_sum(&mask);
        assert!((sum - 16.0).abs() < 0.01, "coverage sum = {sum}");
        let idx = |x: usize, y: usize| mask.data[y * 8 + x];
        assert!(idx(3, 3) > 0.99, "interior must be opaque");
        assert!(idx(0, 0) < 0.01, "exterior must be empty");
    }

    #[test]
    fn transformed_rect_is_translated() {
        let rect = RectF {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 2.0,
            y_max: 2.0,
        };
        let t = Affine::new(1.0, 0.0, 0.0, 1.0, 4.0, 4.0);
        let mask = rasterize_rect_mask(rect, t, 8, 8);
        assert!(mask.data[5 * 8 + 5] > 0.99);
        assert!(mask.data[8 + 1] < 0.01);
    }

    #[test]
    fn geometry_outside_the_mask_is_clamped_not_panicking() {
        let rect = RectF {
            x_min: -1000.0,
            y_min: -1000.0,
            x_max: 1000.0,
            y_max: 1000.0,
        };
        let mask = rasterize_rect_mask(rect, Affine::IDENTITY, 4, 4);
        assert!(mask.data.iter().all(|&v| v > 0.99), "mask must be filled");
    }

    #[test]
    fn empty_rect_produces_blank_mask() {
        let rect = RectF {
            x_min: 1.0,
            y_min: 1.0,
            x_max: 1.0,
            y_max: 1.0,
        };
        let mask = rasterize_rect_mask(rect, Affine::IDENTITY, 4, 4);
        assert!(mask.is_blank());
    }

    #[test]
    fn intersect_multiplies_coverage() {
        let mut a = CoverageMask::opaque(2, 2);
        let b = CoverageMask {
            width: 2,
            height: 2,
            data: vec![0.0, 0.5, 1.0, 0.25],
        };
        a.intersect(&b);
        assert_eq!(a.data, vec![0.0, 0.5, 1.0, 0.25]);
    }

    #[test]
    fn intersect_ignores_mismatched_sizes() {
        let mut a = CoverageMask::opaque(2, 2);
        let b = CoverageMask::opaque(3, 3);
        a.intersect(&b);
        assert_eq!(a.data.len(), 4);
    }

    #[test]
    fn nonzero_winding_leaves_holes_empty() {
        // Outer square wound clockwise, inner square wound counter-clockwise.
        let mut r = MaskRasterizer::new(10, 10, Affine::IDENTITY);
        r.move_to(1.0, 1.0);
        r.line_to(9.0, 1.0);
        r.line_to(9.0, 9.0);
        r.line_to(1.0, 9.0);
        r.close();
        r.move_to(3.0, 3.0);
        r.line_to(3.0, 7.0);
        r.line_to(7.0, 7.0);
        r.line_to(7.0, 3.0);
        r.close();
        let mask = r.finish();
        assert!(mask.data[5 * 10 + 5] < 0.01, "hole must be empty");
        assert!(mask.data[2 * 10 + 5] > 0.99, "ring must be opaque");
    }

    #[test]
    fn curves_are_flattened() {
        let mut r = MaskRasterizer::new(16, 16, Affine::IDENTITY);
        r.move_to(2.0, 8.0);
        r.quad_to(8.0, 0.0, 14.0, 8.0);
        r.curve_to(12.0, 14.0, 4.0, 14.0, 2.0, 8.0);
        r.close();
        let mask = r.finish();
        assert!(coverage_sum(&mask) > 20.0, "curved blob must have area");
        assert!(mask.data[8 * 16 + 8] > 0.5, "centre must be inside");
    }

    #[test]
    fn segment_count_is_bounded() {
        assert_eq!(MaskRasterizer::segment_count(0.0, 7.5), 1);
        assert_eq!(MaskRasterizer::segment_count(f32::NAN, 7.5), 1);
        assert!(MaskRasterizer::segment_count(1e12, 7.5) <= MAX_CURVE_SEGMENTS);
    }

    #[test]
    fn zero_sized_target_is_rejected() {
        let face = ttf_parser::Face::parse(oxifont_bundled::NOTO_SANS_REGULAR, 0);
        if let Ok(face) = face {
            assert!(rasterize_glyph_mask(&face, GlyphId(1), Affine::IDENTITY, 0, 8).is_none());
        }
    }
}
