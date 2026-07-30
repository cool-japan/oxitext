//! COLRv0/COLRv1 paint-graph interpreter.
//!
//! [`ColrPainter`] implements [`ttf_parser::colr::Painter`] and turns the paint
//! callbacks emitted while walking a colour glyph's paint graph into pixels.
//! Unlike the previous implementation it maintains the three pieces of state
//! the COLRv1 model requires:
//!
//! * a **transform stack** — `PaintTransform`, `PaintScale*`, `PaintRotate*`,
//!   `PaintSkew*` and `PaintTranslate` all arrive as `push_transform` /
//!   `pop_transform` pairs, and both the glyph outline *and* the gradient
//!   geometry have to honour them;
//! * a **clip stack** — `PaintGlyph` emits `outline_glyph` + `push_clip`, so the
//!   glyph is a clip region for whatever the child paint draws, and
//!   `push_clip_box` adds the base glyph's `ClipList` rectangle;
//! * a **layer stack** — `PaintComposite` renders a backdrop and a source into
//!   separate layers and combines them with one of the 28 `CompositeMode`s.
//!
//! Layer pixels are stored premultiplied in `f32` so that Porter-Duff and the
//! CSS blend modes can be evaluated without repeated 8-bit rounding; the final
//! buffer is un-premultiplied back to straight RGBA on the way out.

use ttf_parser::colr::{CompositeMode, GradientExtend, Paint, Painter};
use ttf_parser::{Face, GlyphId, RectF, RgbaColor};

use crate::path_raster::{rasterize_glyph_mask, rasterize_rect_mask, Affine, CoverageMask};

/// Maximum number of nested composite layers that are materialised.
///
/// Real fonts nest a handful at most; the cap bounds peak memory when a
/// hand-crafted font nests `PaintComposite` up to ttf-parser's recursion limit.
const MAX_LAYERS: usize = 16;

/// A resolved gradient colour stop: offset in `[0, 1]`, straight RGBA in
/// `[0, 1]`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stop {
    /// Position along the colour line.
    pub offset: f32,
    /// Straight (non-premultiplied) RGBA.
    pub color: [f32; 4],
}

/// Convert a ttf-parser colour into straight float RGBA.
#[inline]
pub(crate) fn rgba_to_f32(c: RgbaColor) -> [f32; 4] {
    [
        c.red as f32 / 255.0,
        c.green as f32 / 255.0,
        c.blue as f32 / 255.0,
        c.alpha as f32 / 255.0,
    ]
}

/// Collect the colour stops of a gradient, sorted by offset.
pub(crate) fn collect_stops(iter: ttf_parser::colr::GradientStopsIter<'_, '_>) -> Vec<Stop> {
    let mut stops: Vec<Stop> = iter
        .map(|s| Stop {
            offset: s.stop_offset,
            color: rgba_to_f32(s.color),
        })
        .collect();
    stops.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    stops
}

/// Apply a gradient extend mode to a raw colour-line parameter.
pub(crate) fn apply_extend(mut t: f32, extend: GradientExtend) -> f32 {
    if !t.is_finite() {
        return 0.0;
    }
    match extend {
        GradientExtend::Pad => t.clamp(0.0_f32, 1.0_f32),
        GradientExtend::Repeat => {
            t = t - t.floor();
            if !(0.0..1.0).contains(&t) {
                t = 0.0;
            }
            t
        }
        GradientExtend::Reflect => {
            let period = 2.0_f32;
            t = t - (t / period).floor() * period;
            if !(0.0..=period).contains(&t) {
                return 0.0;
            }
            if t > 1.0_f32 {
                t = period - t;
            }
            t
        }
    }
}

/// Linearly interpolate two straight RGBA colours.
#[inline]
pub(crate) fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Sample a sorted colour-stop list at an already-extended parameter `t`.
pub(crate) fn sample_stops(stops: &[Stop], t: f32) -> [f32; 4] {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    if t <= first.offset {
        return first.color;
    }
    if t >= last.offset {
        return last.color;
    }
    for pair in stops.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if t >= lo.offset && t <= hi.offset {
            let span = hi.offset - lo.offset;
            if span < 1e-7_f32 {
                return hi.color;
            }
            return lerp_color(lo.color, hi.color, (t - lo.offset) / span);
        }
    }
    last.color
}

// ---------------------------------------------------------------------------
// Gradients
// ---------------------------------------------------------------------------

/// Gradient geometry, expressed in the paint's own (user) coordinate space.
#[derive(Clone, Debug)]
enum GradientKind {
    /// Colour line from `p0` to `p3`, where `p3` already folds in the COLRv1
    /// rotation point `p2`.
    Linear { p0: (f32, f32), p3: (f32, f32) },
    /// Two-point conical gradient.
    Radial {
        c0: (f32, f32),
        r0: f32,
        c1: (f32, f32),
        r1: f32,
    },
    /// Angular sweep, angles measured in turns counter-clockwise from `+x`.
    Sweep {
        center: (f32, f32),
        start_turn: f32,
        end_turn: f32,
    },
}

/// A gradient ready to be sampled in user space.
#[derive(Clone, Debug)]
struct Gradient {
    kind: GradientKind,
    stops: Vec<Stop>,
    extend: GradientExtend,
}

impl Gradient {
    /// Build the linear-gradient geometry described by the COLRv1 triple
    /// `p0`, `p1`, `p2`.
    ///
    /// Per the OpenType COLR specification the rendered colour line runs from
    /// `p0` to `p3`, where `p3` is the projection of `p1` onto the line through
    /// `p0` perpendicular to `p0 -> p2`.  Fonts that emit a perpendicular `p2`
    /// (the common case) get `p3 == p1`; skewed rotation vectors now shear the
    /// gradient the way they are supposed to instead of being ignored.
    fn linear(
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        stops: Vec<Stop>,
        extend: GradientExtend,
    ) -> Self {
        let vx = p2.0 - p0.0;
        let vy = p2.1 - p0.1;
        // Normal of the p0->p2 rotation vector.
        let nx = -vy;
        let ny = vx;
        let nn = nx * nx + ny * ny;
        let p3 = if nn < 1e-9 {
            p1
        } else {
            let wx = p1.0 - p0.0;
            let wy = p1.1 - p0.1;
            let k = (wx * nx + wy * ny) / nn;
            (p0.0 + k * nx, p0.1 + k * ny)
        };
        Gradient {
            kind: GradientKind::Linear { p0, p3 },
            stops,
            extend,
        }
    }

    /// Evaluate the gradient at a user-space point, or `None` when the point
    /// lies outside the gradient's definition (only possible for the conical
    /// radial case).
    fn sample(&self, u: f32, v: f32) -> Option<[f32; 4]> {
        let raw_t = match self.kind {
            GradientKind::Linear { p0, p3 } => {
                let gx = p3.0 - p0.0;
                let gy = p3.1 - p0.1;
                let gg = gx * gx + gy * gy;
                if gg < 1e-9 {
                    0.0
                } else {
                    ((u - p0.0) * gx + (v - p0.1) * gy) / gg
                }
            }
            GradientKind::Radial { c0, r0, c1, r1 } => radial_parameter(c0, r0, c1, r1, u, v)?,
            GradientKind::Sweep {
                center,
                start_turn,
                end_turn,
            } => {
                let dx = u - center.0;
                let dy = v - center.1;
                let mut turn = dy.atan2(dx) / std::f32::consts::TAU;
                if turn < 0.0 {
                    turn += 1.0;
                }
                let span = end_turn - start_turn;
                // Place the sampled angle inside the half-open turn that the
                // gradient's angular window starts from.
                let base = start_turn.min(end_turn);
                turn = base + (turn - base).rem_euclid(1.0);
                if span.abs() < 1e-6 {
                    0.0
                } else {
                    (turn - start_turn) / span
                }
            }
        };
        Some(sample_stops(&self.stops, apply_extend(raw_t, self.extend)))
    }
}

/// Solve the two-point conical gradient parameter for a user-space point.
///
/// Finds the largest `t` such that the point lies on the circle centred at
/// `lerp(c0, c1, t)` with radius `lerp(r0, r1, t)`, rejecting solutions with a
/// negative radius.  Returns `None` when no circle of the family passes through
/// the point, which per the specification leaves the pixel unpainted.
fn radial_parameter(
    c0: (f32, f32),
    r0: f32,
    c1: (f32, f32),
    r1: f32,
    u: f32,
    v: f32,
) -> Option<f32> {
    let cdx = c1.0 - c0.0;
    let cdy = c1.1 - c0.1;
    let dr = r1 - r0;
    let pdx = u - c0.0;
    let pdy = v - c0.1;

    let a = cdx * cdx + cdy * cdy - dr * dr;
    let b = pdx * cdx + pdy * cdy + r0 * dr;
    let c = pdx * pdx + pdy * pdy - r0 * r0;

    if a.abs() < 1e-6 {
        if b.abs() < 1e-9 {
            return None;
        }
        let t = c / (2.0 * b);
        return (r0 + t * dr >= 0.0).then_some(t);
    }

    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let t1 = (b + sq) / a;
    let t2 = (b - sq) / a;
    let (hi, lo) = if t1 >= t2 { (t1, t2) } else { (t2, t1) };
    if r0 + hi * dr >= 0.0 {
        Some(hi)
    } else if r0 + lo * dr >= 0.0 {
        Some(lo)
    } else {
        None
    }
}

/// What a `paint` callback should fill the current region with.
enum FillSource {
    /// A single straight RGBA colour.
    Solid([f32; 4]),
    /// A gradient sampled per pixel in user space.
    Gradient(Box<Gradient>),
}

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

/// A render target holding premultiplied `f32` RGBA pixels.
struct Layer {
    /// `width * height` premultiplied RGBA samples.
    px: Vec<[f32; 4]>,
    /// How this layer is combined into its parent when popped.
    mode: CompositeMode,
}

impl Layer {
    fn new(pixel_count: usize, mode: CompositeMode) -> Self {
        Layer {
            px: vec![[0.0; 4]; pixel_count],
            mode,
        }
    }
}

// ---------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------

/// Interprets COLR paint callbacks into an RGBA bitmap.
pub(crate) struct ColrPainter<'a, 'f> {
    face: &'a Face<'f>,
    width: u32,
    height: u32,
    /// Stack of accumulated user -> device transforms; the last entry is
    /// current and the first is the bitmap's design-unit -> pixel mapping.
    transforms: Vec<Affine>,
    /// Stack of accumulated clip masks; the last entry is the active clip.
    clips: Vec<CoverageMask>,
    /// Stack of render targets; the first entry is the root bitmap.
    layers: Vec<Layer>,
    /// Number of `push_layer` calls that were not materialised because
    /// [`MAX_LAYERS`] was reached; their matching `pop_layer`s are ignored.
    skipped_layers: usize,
    /// Glyph buffered by `outline_glyph`, consumed by `push_clip` (COLRv1) or
    /// by the next `paint` (COLRv0, which has no clip step).
    pending_glyph: Option<GlyphId>,
    /// CPAL palette index used to resolve gradient stop colours.
    palette: u16,
}

impl<'a, 'f> ColrPainter<'a, 'f> {
    /// Create a painter targeting a `width` x `height` bitmap.
    ///
    /// `device` maps font design units to pixels, including the Y flip and the
    /// baseline placement.
    pub(crate) fn new(
        face: &'a Face<'f>,
        width: u32,
        height: u32,
        device: Affine,
        palette: u16,
    ) -> Self {
        let pixel_count = (width as usize) * (height as usize);
        ColrPainter {
            face,
            width,
            height,
            transforms: vec![device],
            clips: Vec::new(),
            layers: vec![Layer::new(pixel_count, CompositeMode::SourceOver)],
            skipped_layers: 0,
            pending_glyph: None,
            palette,
        }
    }

    /// The active user -> device transform.
    #[inline]
    fn transform(&self) -> Affine {
        self.transforms.last().copied().unwrap_or(Affine::IDENTITY)
    }

    /// Un-premultiply the root layer into straight 8-bit RGBA.
    pub(crate) fn into_rgba(mut self) -> Vec<u8> {
        // Collapse any layers a malformed font left unbalanced.  Clearing the
        // skip counter first keeps `pop_layer` from swallowing these calls.
        self.skipped_layers = 0;
        while self.layers.len() > 1 {
            self.pop_layer();
        }
        let root = match self.layers.pop() {
            Some(layer) => layer,
            None => return vec![0; (self.width as usize) * (self.height as usize) * 4],
        };
        let mut out = Vec::with_capacity(root.px.len() * 4);
        for px in &root.px {
            let a = px[3].clamp(0.0, 1.0);
            if a <= 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let inv = 1.0 / a;
            out.push(to_u8(px[0] * inv));
            out.push(to_u8(px[1] * inv));
            out.push(to_u8(px[2] * inv));
            out.push(to_u8(a));
        }
        out
    }

    /// Rasterize `glyph_id` with the active transform.
    fn glyph_mask(&self, glyph_id: GlyphId) -> CoverageMask {
        rasterize_glyph_mask(
            self.face,
            glyph_id,
            self.transform(),
            self.width,
            self.height,
        )
        .unwrap_or_else(|| CoverageMask {
            width: self.width,
            height: self.height,
            data: vec![0.0; (self.width as usize) * (self.height as usize)],
        })
    }

    /// Push `mask` onto the clip stack, intersected with the active clip.
    fn push_mask(&mut self, mut mask: CoverageMask) {
        if let Some(current) = self.clips.last() {
            mask.intersect(current);
        }
        self.clips.push(mask);
    }

    /// The region a `paint` call should cover.
    fn paint_region(&mut self) -> CoverageMask {
        match self.pending_glyph.take() {
            // COLRv0 (and any malformed v1 graph): `outline_glyph` is followed
            // directly by `paint`, with no intervening `push_clip`.
            Some(glyph_id) => {
                let mut mask = self.glyph_mask(glyph_id);
                if let Some(current) = self.clips.last() {
                    mask.intersect(current);
                }
                mask
            }
            None => match self.clips.last() {
                Some(current) => current.clone(),
                None => CoverageMask::opaque(self.width, self.height),
            },
        }
    }

    /// Source-over `fill` into the active layer, weighted by `region`.
    fn fill(&mut self, region: &CoverageMask, fill: &FillSource) {
        if region.width != self.width || region.height != self.height {
            return;
        }
        let to_user = match fill {
            FillSource::Solid(_) => None,
            FillSource::Gradient(_) => match self.transform().invert() {
                Some(inv) => Some(inv),
                // A singular transform collapses the paint to zero area.
                None => return,
            },
        };
        let width = self.width as usize;
        let height = self.height as usize;
        let Some(target) = self.layers.last_mut() else {
            return;
        };

        for y in 0..height {
            let row = y * width;
            for x in 0..width {
                let cov = region.data[row + x];
                if cov <= 0.0 {
                    continue;
                }
                let rgba = match (fill, to_user) {
                    (FillSource::Solid(c), _) => *c,
                    (FillSource::Gradient(g), Some(inv)) => {
                        let (u, v) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                        match g.sample(u, v) {
                            Some(c) => c,
                            None => continue,
                        }
                    }
                    (FillSource::Gradient(_), None) => continue,
                };
                let alpha = (rgba[3] * cov).clamp(0.0, 1.0);
                if alpha <= 0.0 {
                    continue;
                }
                let dst = &mut target.px[row + x];
                let inv_a = 1.0 - alpha;
                dst[0] = rgba[0] * alpha + dst[0] * inv_a;
                dst[1] = rgba[1] * alpha + dst[1] * inv_a;
                dst[2] = rgba[2] * alpha + dst[2] * inv_a;
                dst[3] = alpha + dst[3] * inv_a;
            }
        }
    }
}

impl<'p, 'a, 'f> Painter<'p> for ColrPainter<'a, 'f> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.pending_glyph = Some(glyph_id);
    }

    fn paint(&mut self, paint: Paint<'p>) {
        let palette = self.palette;
        let fill = match paint {
            Paint::Solid(color) => FillSource::Solid(rgba_to_f32(color)),
            Paint::LinearGradient(g) => FillSource::Gradient(Box::new(Gradient::linear(
                (g.x0, g.y0),
                (g.x1, g.y1),
                (g.x2, g.y2),
                collect_stops(g.stops(palette, &[])),
                g.extend,
            ))),
            Paint::RadialGradient(g) => FillSource::Gradient(Box::new(Gradient {
                kind: GradientKind::Radial {
                    c0: (g.x0, g.y0),
                    r0: g.r0,
                    c1: (g.x1, g.y1),
                    r1: g.r1,
                },
                stops: collect_stops(g.stops(palette, &[])),
                extend: g.extend,
            })),
            Paint::SweepGradient(g) => FillSource::Gradient(Box::new(Gradient {
                kind: GradientKind::Sweep {
                    center: (g.center_x, g.center_y),
                    // F2DOT14 sweep angles count 180 degrees per 1.0, i.e. half
                    // turns, so halve them to get turns.
                    start_turn: g.start_angle * 0.5,
                    end_turn: g.end_angle * 0.5,
                },
                stops: collect_stops(g.stops(palette, &[])),
                extend: g.extend,
            })),
        };
        let region = self.paint_region();
        if region.is_blank() {
            return;
        }
        self.fill(&region, &fill);
    }

    fn push_clip(&mut self) {
        let mask = match self.pending_glyph.take() {
            Some(glyph_id) => self.glyph_mask(glyph_id),
            // No outline was staged; keep the clip unchanged.
            None => CoverageMask::opaque(self.width, self.height),
        };
        self.push_mask(mask);
    }

    fn push_clip_box(&mut self, clip_box: RectF) {
        let mask = rasterize_rect_mask(clip_box, self.transform(), self.width, self.height);
        self.push_mask(mask);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        if self.layers.len() >= MAX_LAYERS {
            self.skipped_layers += 1;
            return;
        }
        let pixel_count = (self.width as usize) * (self.height as usize);
        self.layers.push(Layer::new(pixel_count, mode));
    }

    fn pop_layer(&mut self) {
        if self.skipped_layers > 0 {
            self.skipped_layers -= 1;
            return;
        }
        if self.layers.len() < 2 {
            return;
        }
        let Some(source) = self.layers.pop() else {
            return;
        };
        let Some(target) = self.layers.last_mut() else {
            return;
        };
        for (dst, src) in target.px.iter_mut().zip(source.px.iter()) {
            *dst = composite(source.mode, *src, *dst);
        }
    }

    fn push_transform(&mut self, transform: ttf_parser::Transform) {
        let next = Affine::concat(self.transform(), Affine::from_ttf(transform));
        self.transforms.push(next);
    }

    fn pop_transform(&mut self) {
        // Never pop the device transform installed by `new`.
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }
}

/// Quantise a straight `[0, 1]` channel to 8 bits.
#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

// ---------------------------------------------------------------------------
// Compositing
// ---------------------------------------------------------------------------

/// Combine a premultiplied `src` over/with a premultiplied `dst`.
///
/// Implements the full `CompositeMode` set: the thirteen Porter-Duff operators
/// with their `(Fa, Fb)` coefficient pairs, the eleven separable CSS blend
/// modes, and the four non-separable ones.
pub(crate) fn composite(mode: CompositeMode, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    use CompositeMode as M;

    let sa = src[3];
    let da = dst[3];

    // Porter-Duff coefficient pairs.
    let coefficients = match mode {
        M::Clear => Some((0.0, 0.0)),
        M::Source => Some((1.0, 0.0)),
        M::Destination => Some((0.0, 1.0)),
        M::SourceOver => Some((1.0, 1.0 - sa)),
        M::DestinationOver => Some((1.0 - da, 1.0)),
        M::SourceIn => Some((da, 0.0)),
        M::DestinationIn => Some((0.0, sa)),
        M::SourceOut => Some((1.0 - da, 0.0)),
        M::DestinationOut => Some((0.0, 1.0 - sa)),
        M::SourceAtop => Some((da, 1.0 - sa)),
        M::DestinationAtop => Some((1.0 - da, sa)),
        M::Xor => Some((1.0 - da, 1.0 - sa)),
        _ => None,
    };

    if let Some((fa, fb)) = coefficients {
        return [
            src[0] * fa + dst[0] * fb,
            src[1] * fa + dst[1] * fb,
            src[2] * fa + dst[2] * fb,
            (sa * fa + da * fb).clamp(0.0, 1.0),
        ];
    }

    if matches!(mode, M::Plus) {
        return [
            (src[0] + dst[0]).min(1.0),
            (src[1] + dst[1]).min(1.0),
            (src[2] + dst[2]).min(1.0),
            (sa + da).min(1.0),
        ];
    }

    // Blend modes operate on straight colours.
    let cs = unpremultiply(src);
    let cb = unpremultiply(dst);
    let blended = match mode {
        M::Hue | M::Saturation | M::Color | M::Luminosity => nonseparable_blend(mode, cb, cs),
        _ => [
            separable_blend(mode, cb[0], cs[0]),
            separable_blend(mode, cb[1], cs[1]),
            separable_blend(mode, cb[2], cs[2]),
        ],
    };

    // co = as*(1-ab)*Cs + as*ab*B(Cb,Cs) + (1-as)*ab*Cb
    let out_a = sa + da * (1.0 - sa);
    let w_src = sa * (1.0 - da);
    let w_blend = sa * da;
    let w_dst = (1.0 - sa) * da;
    [
        (cs[0] * w_src + blended[0] * w_blend + cb[0] * w_dst).clamp(0.0, 1.0),
        (cs[1] * w_src + blended[1] * w_blend + cb[1] * w_dst).clamp(0.0, 1.0),
        (cs[2] * w_src + blended[2] * w_blend + cb[2] * w_dst).clamp(0.0, 1.0),
        out_a.clamp(0.0, 1.0),
    ]
}

/// Recover straight RGB from a premultiplied pixel.
#[inline]
fn unpremultiply(px: [f32; 4]) -> [f32; 3] {
    if px[3] <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let inv = 1.0 / px[3];
    [px[0] * inv, px[1] * inv, px[2] * inv]
}

/// CSS separable blend functions, evaluated per channel.
fn separable_blend(mode: CompositeMode, cb: f32, cs: f32) -> f32 {
    use CompositeMode as M;
    match mode {
        M::Screen => cb + cs - cb * cs,
        M::Overlay => separable_blend(M::HardLight, cs, cb),
        M::Darken => cb.min(cs),
        M::Lighten => cb.max(cs),
        M::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        M::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        M::HardLight => {
            if cs <= 0.5 {
                cb * (2.0 * cs)
            } else {
                let d = 2.0 * cs - 1.0;
                cb + d - cb * d
            }
        }
        M::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.max(0.0).sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        M::Difference => (cb - cs).abs(),
        M::Exclusion => cb + cs - 2.0 * cb * cs,
        M::Multiply => cb * cs,
        // Every remaining mode is handled before this function is reached.
        _ => cs,
    }
}

/// Luminosity of a straight RGB triple, per CSS Compositing.
#[inline]
fn luminosity(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Clip a colour back into gamut while preserving luminosity.
fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
    let l = luminosity(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 && (l - n).abs() > 1e-6 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 && (x - l).abs() > 1e-6 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

/// Shift a colour to the given luminosity.
fn set_luminosity(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - luminosity(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

/// Saturation of a straight RGB triple, per CSS Compositing.
#[inline]
fn saturation(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Rescale a colour to the given saturation.
fn set_saturation(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| c[i].partial_cmp(&c[j]).unwrap_or(std::cmp::Ordering::Equal));
    let (i_min, i_mid, i_max) = (order[0], order[1], order[2]);
    let mut out = [0.0_f32; 3];
    let span = c[i_max] - c[i_min];
    if span > 0.0 {
        out[i_mid] = (c[i_mid] - c[i_min]) * s / span;
        out[i_max] = s;
    }
    out
}

/// CSS non-separable blend functions.
fn nonseparable_blend(mode: CompositeMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    use CompositeMode as M;
    match mode {
        M::Hue => set_luminosity(set_saturation(cs, saturation(cb)), luminosity(cb)),
        M::Saturation => set_luminosity(set_saturation(cb, saturation(cs)), luminosity(cb)),
        M::Color => set_luminosity(cs, luminosity(cb)),
        M::Luminosity => set_luminosity(cb, luminosity(cs)),
        _ => cs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stops2() -> Vec<Stop> {
        vec![
            Stop {
                offset: 0.0,
                color: [1.0, 0.0, 0.0, 1.0],
            },
            Stop {
                offset: 1.0,
                color: [0.0, 0.0, 1.0, 1.0],
            },
        ]
    }

    #[test]
    fn extend_pad_clamps_both_ends() {
        assert_eq!(apply_extend(-0.5, GradientExtend::Pad), 0.0);
        assert_eq!(apply_extend(1.5, GradientExtend::Pad), 1.0);
        assert!((apply_extend(0.5, GradientExtend::Pad) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn extend_repeat_wraps() {
        assert!((apply_extend(1.25, GradientExtend::Repeat) - 0.25).abs() < 1e-6);
        assert!((apply_extend(-0.25, GradientExtend::Repeat) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn extend_reflect_mirrors() {
        assert!((apply_extend(1.25, GradientExtend::Reflect) - 0.75).abs() < 1e-6);
        assert!((apply_extend(-0.25, GradientExtend::Reflect) - 0.25).abs() < 1e-6);
        assert!((apply_extend(2.25, GradientExtend::Reflect) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn extend_rejects_non_finite() {
        assert_eq!(apply_extend(f32::NAN, GradientExtend::Pad), 0.0);
        assert_eq!(apply_extend(f32::INFINITY, GradientExtend::Repeat), 0.0);
    }

    #[test]
    fn sample_stops_empty_is_transparent() {
        assert_eq!(sample_stops(&[], 0.5), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn sample_stops_hits_both_ends_and_middle() {
        let stops = stops2();
        assert_eq!(sample_stops(&stops, 0.0)[0], 1.0);
        assert_eq!(sample_stops(&stops, 1.0)[2], 1.0);
        let mid = sample_stops(&stops, 0.5);
        assert!((mid[0] - 0.5).abs() < 1e-6);
        assert!((mid[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn linear_gradient_uses_rotation_point() {
        // p2 perpendicular to p0->p1 must leave the colour line unchanged.
        let g = Gradient::linear(
            (0.0, 0.0),
            (10.0, 0.0),
            (0.0, 10.0),
            stops2(),
            GradientExtend::Pad,
        );
        match g.kind {
            GradientKind::Linear { p0, p3 } => {
                assert!((p0.0).abs() < 1e-6);
                assert!((p3.0 - 10.0).abs() < 1e-5, "p3 = {p3:?}");
                assert!(p3.1.abs() < 1e-5, "p3 = {p3:?}");
            }
            _ => panic!("expected a linear gradient"),
        }
        let c = g.sample(5.0, 0.0).expect("sampled");
        assert!((c[0] - 0.5).abs() < 1e-5, "midpoint colour = {c:?}");
    }

    #[test]
    fn linear_gradient_skewed_rotation_point_shears_the_axis() {
        // p2 collinear-ish with p0->p1 tilts p3 away from p1.
        let g = Gradient::linear(
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            stops2(),
            GradientExtend::Pad,
        );
        match g.kind {
            GradientKind::Linear { p3, .. } => {
                assert!(
                    p3.1.abs() > 1.0,
                    "p3 should have moved off the x axis: {p3:?}"
                );
            }
            _ => panic!("expected a linear gradient"),
        }
    }

    #[test]
    fn radial_focal_point_gradient_matches_distance() {
        // Concentric circles: t is the normalised distance from the centre.
        let t = radial_parameter((0.0, 0.0), 0.0, (0.0, 0.0), 100.0, 50.0, 0.0).expect("solvable");
        assert!((t - 0.5).abs() < 1e-4, "t = {t}");
    }

    #[test]
    fn radial_rejects_points_outside_the_cone() {
        // Two circles of equal radius offset along x form a strip; a point far
        // off the strip has no solution.
        assert!(radial_parameter((0.0, 0.0), 1.0, (10.0, 0.0), 1.0, 0.0, 500.0).is_none());
    }

    #[test]
    fn sweep_angles_are_half_turns() {
        // start=-1.0 (=-180 degrees), end=1.0 (=180 degrees) covers the circle.
        let g = Gradient {
            kind: GradientKind::Sweep {
                center: (0.0, 0.0),
                start_turn: -0.5,
                end_turn: 0.5,
            },
            stops: stops2(),
            extend: GradientExtend::Pad,
        };
        // Angle -180 degrees maps to the first stop, +180 to the last.
        let left = g.sample(-1.0, -0.0001).expect("sampled");
        let right = g.sample(-1.0, 0.0001).expect("sampled");
        assert!(
            left[0] > 0.9 || right[2] > 0.9,
            "left={left:?} right={right:?}"
        );
        let up = g.sample(0.0, 1.0).expect("sampled");
        assert!(
            (up[0] - 0.25).abs() < 0.02,
            "90 degrees -> t=0.75 red: {up:?}"
        );
    }

    #[test]
    fn composite_source_over_is_standard_alpha_blend() {
        let src = [0.5, 0.0, 0.0, 0.5]; // premultiplied red at 50%
        let dst = [0.0, 0.0, 1.0, 1.0]; // opaque blue
        let out = composite(CompositeMode::SourceOver, src, dst);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[2] - 0.5).abs() < 1e-6);
        assert!((out[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn composite_clear_erases() {
        let out = composite(
            CompositeMode::Clear,
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(out, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn composite_destination_keeps_backdrop() {
        let dst = [0.25, 0.5, 0.75, 1.0];
        assert_eq!(
            composite(CompositeMode::Destination, [1.0, 1.0, 1.0, 1.0], dst),
            dst
        );
    }

    #[test]
    fn composite_multiply_darkens() {
        // Opaque mid-grey over opaque mid-grey.
        let g = [0.5, 0.5, 0.5, 1.0];
        let out = composite(CompositeMode::Multiply, g, g);
        assert!((out[0] - 0.25).abs() < 1e-5, "out = {out:?}");
        assert!((out[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn composite_screen_lightens() {
        let g = [0.5, 0.5, 0.5, 1.0];
        let out = composite(CompositeMode::Screen, g, g);
        assert!((out[0] - 0.75).abs() < 1e-5, "out = {out:?}");
    }

    #[test]
    fn composite_plus_saturates() {
        let out = composite(
            CompositeMode::Plus,
            [0.8, 0.0, 0.0, 0.8],
            [0.8, 0.0, 0.0, 0.8],
        );
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn composite_xor_keeps_only_disjoint_parts() {
        let out = composite(
            CompositeMode::Xor,
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        );
        assert!(out[3] < 1e-6, "fully overlapping opaque XOR must vanish");
    }

    #[test]
    fn composite_luminosity_takes_source_lightness() {
        // White source luminosity applied to a red backdrop.
        let out = composite(
            CompositeMode::Luminosity,
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!(
            out[0] > 0.9 && out[1] > 0.9 && out[2] > 0.9,
            "out = {out:?}"
        );
    }

    #[test]
    fn composite_hue_keeps_backdrop_luminosity() {
        let backdrop = [0.2, 0.2, 0.2, 1.0];
        let out = composite(CompositeMode::Hue, [1.0, 0.0, 0.0, 1.0], backdrop);
        let lum = luminosity([out[0], out[1], out[2]]);
        assert!((lum - 0.2).abs() < 0.05, "luminosity drifted: {out:?}");
    }

    #[test]
    fn all_composite_modes_stay_in_range() {
        let modes = [
            CompositeMode::Clear,
            CompositeMode::Source,
            CompositeMode::Destination,
            CompositeMode::SourceOver,
            CompositeMode::DestinationOver,
            CompositeMode::SourceIn,
            CompositeMode::DestinationIn,
            CompositeMode::SourceOut,
            CompositeMode::DestinationOut,
            CompositeMode::SourceAtop,
            CompositeMode::DestinationAtop,
            CompositeMode::Xor,
            CompositeMode::Plus,
            CompositeMode::Screen,
            CompositeMode::Overlay,
            CompositeMode::Darken,
            CompositeMode::Lighten,
            CompositeMode::ColorDodge,
            CompositeMode::ColorBurn,
            CompositeMode::HardLight,
            CompositeMode::SoftLight,
            CompositeMode::Difference,
            CompositeMode::Exclusion,
            CompositeMode::Multiply,
            CompositeMode::Hue,
            CompositeMode::Saturation,
            CompositeMode::Color,
            CompositeMode::Luminosity,
        ];
        let samples = [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.3, 0.6, 0.1, 0.7],
            [0.5, 0.5, 0.5, 1.0],
        ];
        for mode in modes {
            for src in samples {
                for dst in samples {
                    let out = composite(mode, src, dst);
                    for (i, v) in out.iter().enumerate() {
                        assert!(
                            v.is_finite() && (-1e-5..=1.0 + 1e-5).contains(v),
                            "{mode:?} channel {i} = {v} for src={src:?} dst={dst:?}"
                        );
                    }
                }
            }
        }
    }
}
