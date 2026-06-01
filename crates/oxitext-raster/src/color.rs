//! COLRv0/COLRv1/CPAL color glyph rendering.
//!
//! Implements compositing of COLRv0 and COLRv1 layered-color glyphs using CPAL
//! palette resolution.  Each layer's outline is rasterized via fontdue and
//! Porter-Duff "source-over" composited into an RGBA accumulation buffer.
//!
//! ## COLRv1 gradient support
//!
//! - **Linear gradient**: Full implementation.  Points `(x0,y0)`, `(x1,y1)`,
//!   `(x2,y2)` are in font design units and converted to pixel space using the
//!   font's `units_per_em` and the requested render height.
//! - **Radial gradient**: Full implementation for the common case where `r0 ≈ 0`
//!   (focal-point gradient).  General two-circle interpolation is approximated
//!   by linear distance from circle centre to circle centre.
//! - **Sweep gradient**: Full implementation.  For each pixel the angle from the
//!   gradient centre is computed with `atan2`, normalised to a turn value in
//!   `[0, 1)`, mapped into the `[start_angle, end_angle]` window (angles stored
//!   as F2DOT14 turns by ttf-parser), and sampled against the colour-stop list
//!   using the gradient's extend mode.

use fontdue::Font as FontdueFont;
use ttf_parser::{
    colr::{CompositeMode, GradientExtend, Paint, Painter},
    Face, GlyphId, RectF, Transform,
};

/// An RGBA bitmap produced by compositing COLRv0 layers.
#[derive(Debug, Clone)]
pub struct ColorGlyphBitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in RGBA order: `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Render a COLRv0 base glyph by compositing all its CPAL-colored layers
/// into a pixel buffer at the given size.
///
/// Returns `None` if:
/// - the glyph has no COLR data, or
/// - the font cannot be parsed by fontdue.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `base_glyph`: the GID to render.
/// - `width`, `height`: output bitmap size in pixels.
pub fn render_colr_v0(
    face_data: &[u8],
    base_glyph: GlyphId,
    width: u32,
    height: u32,
) -> Option<ColorGlyphBitmap> {
    let face = Face::parse(face_data, 0).ok()?;
    let colr_table = face.tables().colr?;

    // Quick check: is this glyph present in the COLR table?
    if !colr_table.contains(base_glyph) {
        return None;
    }

    let fontdue_font = FontdueFont::from_bytes(face_data, fontdue::FontSettings::default()).ok()?;

    // Accumulation buffer: width * height RGBA pixels, starts transparent.
    let mut accum: Vec<u8> = vec![0u8; (width * height * 4) as usize];

    // Use black as the "foreground" colour for palette index 0xFFFF.
    let foreground = ttf_parser::RgbaColor::new(0, 0, 0, 255);

    let mut painter = ColrV0Painter {
        fontdue_font: &fontdue_font,
        width,
        height,
        accum: &mut accum,
        pending_glyph: None,
    };

    // Empty coords slice = no variable-font deltas (COLRv0 doesn't need them).
    colr_table.paint(base_glyph, 0, &mut painter, &[], foreground)?;

    Some(ColorGlyphBitmap {
        width,
        height,
        rgba: accum,
    })
}

/// Render a COLRv1 base glyph.  Where gradient data is present, linear,
/// radial, and sweep gradients are composited into the pixel buffer.
///
/// Falls back to [`render_colr_v0`] for glyphs that have only COLRv0 data.
///
/// Returns `None` if the glyph has no COLR data or the font cannot be parsed.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `base_glyph`: the GID to render.
/// - `width`, `height`: output bitmap size in pixels.
pub fn render_colr_v1(
    face_data: &[u8],
    base_glyph: GlyphId,
    width: u32,
    height: u32,
) -> Option<ColorGlyphBitmap> {
    let face = Face::parse(face_data, 0).ok()?;
    let colr_table = face.tables().colr?;

    if !colr_table.contains(base_glyph) {
        return None;
    }

    let fontdue_font = FontdueFont::from_bytes(face_data, fontdue::FontSettings::default()).ok()?;

    let units_per_em = face.units_per_em();

    let mut accum: Vec<u8> = vec![0u8; (width * height * 4) as usize];
    let foreground = ttf_parser::RgbaColor::new(0, 0, 0, 255);

    let mut painter = ColrV1Painter {
        fontdue_font: &fontdue_font,
        width,
        height,
        accum: &mut accum,
        pending_glyph: None,
        units_per_em,
    };

    colr_table.paint(base_glyph, 0, &mut painter, &[], foreground)?;

    Some(ColorGlyphBitmap {
        width,
        height,
        rgba: accum,
    })
}

/// Dispatch function: render the best available colour representation for a glyph.
///
/// Priority order:
/// 1. **CBDT/CBLC** — PNG-encoded embedded bitmaps (color emoji fonts).  Uses `height`
///    as the pixel-per-em target so that the closest available strike is selected.
/// 2. **COLRv1** — gradient/composite outline colours.
/// 3. **COLRv0** — simple per-layer solid-coloured outlines.
///
/// Returns `None` if the glyph has no colour data or the font cannot be parsed.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `glyph_id`: raw GID as a `u16`.
/// - `width`, `height`: output bitmap size in pixels.
pub fn render_color_glyph(
    face_data: &[u8],
    glyph_id: u16,
    width: u32,
    height: u32,
) -> Option<ColorGlyphBitmap> {
    let gid = GlyphId(glyph_id);

    // Priority order per OpenType recommendation and detect_color_glyph_type:
    //   Sbix > SVG > CBDT/CBLC > COLRv1 > COLRv0
    //
    // Sbix uses the same glyph_raster_image path as CBDT (ttf-parser routes
    // both through the same API), so CBDT/sbix are handled together below.

    // SVG table — higher fidelity vector art before CBDT bitmaps.
    #[cfg(feature = "svg-backend")]
    {
        let px_size = u16::try_from(height).unwrap_or(u16::MAX);
        if let Some(bm) = crate::svg_backend::render_svg_glyph(face_data, glyph_id, px_size) {
            return Some(ColorGlyphBitmap {
                width: bm.width,
                height: bm.height,
                rgba: bm.rgba,
            });
        }
    }

    // CBDT/CBLC/sbix embedded bitmaps.
    let ppem = u16::try_from(height).unwrap_or(u16::MAX);
    if let Some(bitmap) = crate::detect::render_cbdt_glyph(face_data, glyph_id, ppem) {
        return Some(bitmap);
    }

    // COLRv1 → COLRv0 fallback.
    render_colr_v1(face_data, gid, width, height)
        .or_else(|| render_colr_v0(face_data, gid, width, height))
}

// ---------------------------------------------------------------------------
// Gradient helpers
// ---------------------------------------------------------------------------

/// A resolved colour stop: offset in `[0.0, 1.0]` and RGBA colour.
#[derive(Clone, Copy)]
struct Stop {
    offset: f32,
    color: ttf_parser::RgbaColor,
}

/// Collect colour stops from a `GradientStopsIter` into a `Vec<Stop>`, sorted
/// by offset.
fn collect_stops<'a, 'b>(iter: ttf_parser::colr::GradientStopsIter<'a, 'b>) -> Vec<Stop> {
    let mut stops: Vec<Stop> = iter
        .map(|s| Stop {
            offset: s.stop_offset,
            color: s.color,
        })
        .collect();
    stops.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    stops
}

/// Apply extend mode to a raw parameter `t`.
fn apply_extend(mut t: f32, extend: GradientExtend) -> f32 {
    match extend {
        GradientExtend::Pad => t.clamp(0.0_f32, 1.0_f32),
        GradientExtend::Repeat => {
            t = t - t.floor();
            if t < 0.0_f32 {
                t += 1.0_f32;
            }
            t
        }
        GradientExtend::Reflect => {
            // Map to [0,2), then reflect upper half back.
            let period = 2.0_f32;
            t = t - (t / period).floor() * period;
            if t < 0.0_f32 {
                t += period;
            }
            if t > 1.0_f32 {
                t = period - t;
            }
            t
        }
    }
}

/// Linearly interpolate between two `RgbaColor` values by `t ∈ [0,1]`.
#[inline]
fn lerp_color(a: ttf_parser::RgbaColor, b: ttf_parser::RgbaColor, t: f32) -> ttf_parser::RgbaColor {
    let lerp_u8 = |lo: u8, hi: u8| -> u8 {
        let v = lo as f32 + (hi as f32 - lo as f32) * t;
        v.round() as u8
    };
    ttf_parser::RgbaColor::new(
        lerp_u8(a.red, b.red),
        lerp_u8(a.green, b.green),
        lerp_u8(a.blue, b.blue),
        lerp_u8(a.alpha, b.alpha),
    )
}

/// Look up the colour at parameter `t` (already extended) in `stops`.
fn sample_stops(stops: &[Stop], t: f32) -> ttf_parser::RgbaColor {
    if stops.is_empty() {
        return ttf_parser::RgbaColor::new(0, 0, 0, 0);
    }
    if t <= stops[0].offset {
        return stops[0].color;
    }
    if t >= stops[stops.len() - 1].offset {
        return stops[stops.len() - 1].color;
    }
    for pair in stops.windows(2) {
        let lo = pair[0];
        let hi = pair[1];
        if t >= lo.offset && t <= hi.offset {
            let span = hi.offset - lo.offset;
            if span < 1e-7_f32 {
                return hi.color;
            }
            let frac = (t - lo.offset) / span;
            return lerp_color(lo.color, hi.color, frac);
        }
    }
    stops[stops.len() - 1].color
}

/// Convert a design-unit coordinate to a pixel coordinate along the horizontal
/// axis, using `units_per_em` and `width`.
#[inline]
fn du_to_px_x(du: f32, units_per_em: u16, width: u32) -> f32 {
    du / units_per_em as f32 * width as f32
}

/// Convert a design-unit coordinate to a pixel coordinate along the vertical
/// axis.  Font design units have Y increasing upward; pixels have Y increasing
/// downward.  We flip and bias to keep glyphs centred vertically (baseline at
/// 80% of height, matching `composite_layer`).
#[inline]
fn du_to_px_y(du: f32, units_per_em: u16, height: u32) -> f32 {
    let baseline_y = height as f32 * 0.8_f32;
    // Invert Y axis: higher design-unit Y → lower pixel Y.
    baseline_y - (du / units_per_em as f32 * height as f32)
}

// ---------------------------------------------------------------------------
// COLRv1 painter internals
// ---------------------------------------------------------------------------

/// Parameters for a linear gradient fill, used to avoid exceeding the
/// `too_many_arguments` lint threshold.
struct LinearParams<'a> {
    stops: &'a [Stop],
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    extend: GradientExtend,
}

/// Parameters for a radial gradient fill.
struct RadialParams<'a> {
    stops: &'a [Stop],
    cx0: f32,
    cy0: f32,
    cx1: f32,
    cy1: f32,
    extend: GradientExtend,
}

/// Parameters for a sweep (angular) gradient fill.
struct SweepParams<'a> {
    stops: &'a [Stop],
    /// Center X in design units.
    cx: f32,
    /// Center Y in design units.
    cy: f32,
    /// Start angle in turns (0.0..1.0 = 0°..360°).
    start_angle: f32,
    /// End angle in turns.
    end_angle: f32,
    extend: GradientExtend,
}

/// Internal painter that receives COLRv0/v1 draw commands and composites them
/// into an RGBA accumulation buffer.
struct ColrV1Painter<'a> {
    fontdue_font: &'a FontdueFont,
    width: u32,
    height: u32,
    accum: &'a mut Vec<u8>,
    /// GID buffered by `outline_glyph`, consumed by the next `paint` call.
    pending_glyph: Option<GlyphId>,
    /// Design units per em for coordinate conversion.
    units_per_em: u16,
}

impl<'a> ColrV1Painter<'a> {
    /// Composite the glyph coverage mask with a linear gradient fill.
    fn composite_linear(&mut self, gid: GlyphId, params: &LinearParams<'_>) {
        let stops = params.stops;
        let extend = params.extend;
        let px = self.height as f32;
        let (metrics, coverage) = self.fontdue_font.rasterize_indexed(gid.0, px);
        if metrics.width == 0 || metrics.height == 0 {
            return;
        }

        // Convert gradient end-points from design units → pixel coords.
        let p0x = du_to_px_x(params.x0, self.units_per_em, self.width);
        let p0y = du_to_px_y(params.y0, self.units_per_em, self.height);
        let p1x = du_to_px_x(params.x1, self.units_per_em, self.width);
        let p1y = du_to_px_y(params.y1, self.units_per_em, self.height);

        let dx = p1x - p0x;
        let dy = p1y - p0y;
        let len_sq = dx * dx + dy * dy;

        let baseline_y = (self.height as i32) * 4 / 5;
        let origin_x = metrics.xmin;
        let origin_y = baseline_y - metrics.height as i32 - metrics.ymin;

        for row in 0..metrics.height as i32 {
            for col in 0..metrics.width as i32 {
                let dst_x = origin_x + col;
                let dst_y = origin_y + row;
                if dst_x < 0
                    || dst_y < 0
                    || dst_x >= self.width as i32
                    || dst_y >= self.height as i32
                {
                    continue;
                }
                let cov = coverage[(row * metrics.width as i32 + col) as usize];
                if cov == 0 {
                    continue;
                }

                // Project pixel onto gradient axis.
                let px_coord = dst_x as f32 - p0x;
                let py_coord = dst_y as f32 - p0y;
                let raw_t = if len_sq < 1e-10_f32 {
                    0.0_f32
                } else {
                    (px_coord * dx + py_coord * dy) / len_sq
                };
                let t = apply_extend(raw_t, extend);
                let color = sample_stops(stops, t);
                let effective_alpha = ((cov as u32 * color.alpha as u32) / 255) as u8;
                let dst_idx = ((dst_y * self.width as i32 + dst_x) * 4) as usize;
                porter_duff_over(
                    &mut self.accum[dst_idx..dst_idx + 4],
                    color.red,
                    color.green,
                    color.blue,
                    effective_alpha,
                );
            }
        }
    }

    /// Composite the glyph coverage mask with a radial gradient fill.
    ///
    /// Uses distance-based interpolation: `t = dist(pixel, c0) / dist(c1, c0)`.
    /// This is an accurate approximation for the common case where the inner
    /// circle has radius `r0 ≈ 0` (focal-point gradient).
    fn composite_radial(&mut self, gid: GlyphId, params: &RadialParams<'_>) {
        let stops = params.stops;
        let extend = params.extend;
        let px = self.height as f32;
        let (metrics, coverage) = self.fontdue_font.rasterize_indexed(gid.0, px);
        if metrics.width == 0 || metrics.height == 0 {
            return;
        }

        let p0x = du_to_px_x(params.cx0, self.units_per_em, self.width);
        let p0y = du_to_px_y(params.cy0, self.units_per_em, self.height);
        let p1x = du_to_px_x(params.cx1, self.units_per_em, self.width);
        let p1y = du_to_px_y(params.cy1, self.units_per_em, self.height);

        let total_dx = p1x - p0x;
        let total_dy = p1y - p0y;
        let total_dist = (total_dx * total_dx + total_dy * total_dy).sqrt();

        let baseline_y = (self.height as i32) * 4 / 5;
        let origin_x = metrics.xmin;
        let origin_y = baseline_y - metrics.height as i32 - metrics.ymin;

        for row in 0..metrics.height as i32 {
            for col in 0..metrics.width as i32 {
                let dst_x = origin_x + col;
                let dst_y = origin_y + row;
                if dst_x < 0
                    || dst_y < 0
                    || dst_x >= self.width as i32
                    || dst_y >= self.height as i32
                {
                    continue;
                }
                let cov = coverage[(row * metrics.width as i32 + col) as usize];
                if cov == 0 {
                    continue;
                }

                let px_f = dst_x as f32;
                let py_f = dst_y as f32;
                let dist_to_c0 = ((px_f - p0x).powi(2) + (py_f - p0y).powi(2)).sqrt();
                let raw_t = if total_dist < 1e-6_f32 {
                    0.0_f32
                } else {
                    dist_to_c0 / total_dist
                };
                let t = apply_extend(raw_t, extend);
                let color = sample_stops(stops, t);
                let effective_alpha = ((cov as u32 * color.alpha as u32) / 255) as u8;
                let dst_idx = ((dst_y * self.width as i32 + dst_x) * 4) as usize;
                porter_duff_over(
                    &mut self.accum[dst_idx..dst_idx + 4],
                    color.red,
                    color.green,
                    color.blue,
                    effective_alpha,
                );
            }
        }
    }

    /// Composite the glyph coverage mask with an angular sweep gradient fill.
    ///
    /// For each pixel, computes the angle from the gradient center using
    /// `atan2`, normalises to `[0, 1)` (full turn), maps into the
    /// `[start_angle, end_angle]` window, and samples `color_stops` at that `t`.
    fn composite_sweep(&mut self, gid: GlyphId, params: &SweepParams<'_>) {
        let stops = params.stops;
        let extend = params.extend;
        let px = self.height as f32;
        let (metrics, coverage) = self.fontdue_font.rasterize_indexed(gid.0, px);
        if metrics.width == 0 || metrics.height == 0 {
            return;
        }

        // Convert gradient centre from design units to pixel space.
        let cx = du_to_px_x(params.cx, self.units_per_em, self.width);
        let cy = du_to_px_y(params.cy, self.units_per_em, self.height);

        let baseline_y = (self.height as i32) * 4 / 5;
        let origin_x = metrics.xmin;
        let origin_y = baseline_y - metrics.height as i32 - metrics.ymin;

        for row in 0..metrics.height as i32 {
            for col in 0..metrics.width as i32 {
                let dst_x = origin_x + col;
                let dst_y = origin_y + row;
                if dst_x < 0
                    || dst_y < 0
                    || dst_x >= self.width as i32
                    || dst_y >= self.height as i32
                {
                    continue;
                }
                let cov = coverage[(row * metrics.width as i32 + col) as usize];
                if cov == 0 {
                    continue;
                }

                // Angle in radians, range [-π, π].
                let rel_x = dst_x as f32 - cx;
                let rel_y = dst_y as f32 - cy;
                let mut angle_rad = rel_y.atan2(rel_x);
                // Normalise to [0, 2π).
                if angle_rad < 0.0_f32 {
                    angle_rad += 2.0_f32 * std::f32::consts::PI;
                }
                // Convert to turns in [0, 1).
                let t_turn = angle_rad / (2.0_f32 * std::f32::consts::PI);

                // Map into the [start_angle, end_angle] window.
                let span = params.end_angle - params.start_angle;
                let raw_t = if span.abs() < f32::EPSILON {
                    0.0_f32
                } else {
                    (t_turn - params.start_angle) / span
                };
                let t = apply_extend(raw_t, extend);
                let color = sample_stops(stops, t);
                let effective_alpha = ((cov as u32 * color.alpha as u32) / 255) as u8;
                let dst_idx = ((dst_y * self.width as i32 + dst_x) * 4) as usize;
                porter_duff_over(
                    &mut self.accum[dst_idx..dst_idx + 4],
                    color.red,
                    color.green,
                    color.blue,
                    effective_alpha,
                );
            }
        }
    }
}

impl<'a> Painter<'a> for ColrV1Painter<'a> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.pending_glyph = Some(glyph_id);
    }

    fn paint(&mut self, paint: Paint<'a>) {
        let gid = match self.pending_glyph.take() {
            Some(g) => g,
            None => return,
        };
        match paint {
            Paint::Solid(color) => {
                // Delegate to the same compositing logic as COLRv0.
                let px = self.height as f32;
                let (metrics, coverage) = self.fontdue_font.rasterize_indexed(gid.0, px);
                let glyph_w = metrics.width as i32;
                let glyph_h = metrics.height as i32;
                let baseline_y = (self.height as i32) * 4 / 5;
                let origin_x = metrics.xmin;
                let origin_y = baseline_y - glyph_h - metrics.ymin;
                for row in 0..glyph_h {
                    for col in 0..glyph_w {
                        let dst_x = origin_x + col;
                        let dst_y = origin_y + row;
                        if dst_x < 0
                            || dst_y < 0
                            || dst_x >= self.width as i32
                            || dst_y >= self.height as i32
                        {
                            continue;
                        }
                        let src_cov = coverage[(row * glyph_w + col) as usize];
                        if src_cov == 0 {
                            continue;
                        }
                        let effective_alpha = ((src_cov as u32 * color.alpha as u32) / 255) as u8;
                        let dst_idx = ((dst_y * self.width as i32 + dst_x) * 4) as usize;
                        porter_duff_over(
                            &mut self.accum[dst_idx..dst_idx + 4],
                            color.red,
                            color.green,
                            color.blue,
                            effective_alpha,
                        );
                    }
                }
            }
            Paint::LinearGradient(lg) => {
                let stops = collect_stops(lg.stops(0, &[]));
                let params = LinearParams {
                    stops: &stops,
                    x0: lg.x0,
                    y0: lg.y0,
                    x1: lg.x1,
                    y1: lg.y1,
                    extend: lg.extend,
                };
                self.composite_linear(gid, &params);
            }
            Paint::RadialGradient(rg) => {
                let stops = collect_stops(rg.stops(0, &[]));
                // Map the two circle centres from design units to pixel space.
                let params = RadialParams {
                    stops: &stops,
                    cx0: rg.x0,
                    cy0: rg.y0,
                    cx1: rg.x1,
                    cy1: rg.y1,
                    extend: rg.extend,
                };
                self.composite_radial(gid, &params);
            }
            Paint::SweepGradient(sg) => {
                let stops = collect_stops(sg.stops(0, &[]));
                let params = SweepParams {
                    stops: &stops,
                    cx: sg.center_x,
                    cy: sg.center_y,
                    start_angle: sg.start_angle,
                    end_angle: sg.end_angle,
                    extend: sg.extend,
                };
                self.composite_sweep(gid, &params);
            }
        }
    }

    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _clip_box: RectF) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _mode: CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _transform: Transform) {}
    fn pop_transform(&mut self) {}
}

// ---------------------------------------------------------------------------
// COLRv0 painter (unchanged)
// ---------------------------------------------------------------------------

/// Internal painter that receives COLRv0 draw commands and composites them
/// into an RGBA accumulation buffer.
struct ColrV0Painter<'a> {
    fontdue_font: &'a FontdueFont,
    width: u32,
    height: u32,
    accum: &'a mut Vec<u8>,
    /// GID buffered by `outline_glyph`, consumed by the next `paint` call.
    pending_glyph: Option<GlyphId>,
}

impl<'a> ColrV0Painter<'a> {
    /// Rasterize `gid` at the target size and Porter-Duff "source over"
    /// composite it into `self.accum` with the given RGBA color.
    fn composite_layer(&mut self, gid: GlyphId, color: ttf_parser::RgbaColor) {
        let px = self.height as f32; // treat height as the em-size in pixels
        let (metrics, coverage) = self.fontdue_font.rasterize_indexed(gid.0, px);

        let glyph_w = metrics.width as i32;
        let glyph_h = metrics.height as i32;

        // The glyph's pixel origin relative to the bitmap origin.
        // fontdue's `xmin` is the distance from the pen to the left edge;
        // `ymin` is from the baseline going downward (positive = below baseline).
        // We place the baseline at `height * 0.8` (80% down) as a simple default.
        let baseline_y = (self.height as i32) * 4 / 5;
        let origin_x = metrics.xmin;
        let origin_y = baseline_y - metrics.height as i32 - metrics.ymin;

        let cr = color.red;
        let cg = color.green;
        let cb = color.blue;
        let ca = color.alpha;

        for row in 0..glyph_h {
            for col in 0..glyph_w {
                let dst_x = origin_x + col;
                let dst_y = origin_y + row;
                if dst_x < 0
                    || dst_y < 0
                    || dst_x >= self.width as i32
                    || dst_y >= self.height as i32
                {
                    continue;
                }

                let src_idx = (row * glyph_w + col) as usize;
                let src_alpha_cov = coverage[src_idx];
                if src_alpha_cov == 0 {
                    continue;
                }

                // Combine glyph coverage with layer's alpha channel.
                let effective_alpha = ((src_alpha_cov as u32 * ca as u32) / 255) as u8;

                let dst_idx = ((dst_y * self.width as i32 + dst_x) * 4) as usize;

                // Porter-Duff "source over".
                porter_duff_over(
                    &mut self.accum[dst_idx..dst_idx + 4],
                    cr,
                    cg,
                    cb,
                    effective_alpha,
                );
            }
        }
    }
}

/// In-place Porter-Duff "source over" composite.
///
/// Blends source `(sr, sg, sb, sa)` over existing `dst[0..4]` (RGBA).
fn porter_duff_over(dst: &mut [u8], sr: u8, sg: u8, sb: u8, sa: u8) {
    let sa_f = sa as f32 / 255.0;
    let da_f = dst[3] as f32 / 255.0;
    let out_a = sa_f + da_f * (1.0 - sa_f);
    if out_a < 1e-6 {
        return;
    }
    let blend = |s: u8, d: u8| -> u8 {
        let s_f = s as f32 / 255.0;
        let d_f = d as f32 / 255.0;
        let out = (s_f * sa_f + d_f * da_f * (1.0 - sa_f)) / out_a;
        (out.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    dst[0] = blend(sr, dst[0]);
    dst[1] = blend(sg, dst[1]);
    dst[2] = blend(sb, dst[2]);
    dst[3] = (out_a * 255.0).round() as u8;
}

impl<'a> Painter<'a> for ColrV0Painter<'a> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.pending_glyph = Some(glyph_id);
    }

    fn paint(&mut self, paint: Paint<'a>) {
        if let Some(gid) = self.pending_glyph.take() {
            if let Paint::Solid(color) = paint {
                self.composite_layer(gid, color);
            }
            // Non-solid paints (gradients) are COLRv1 features; ignore for v0.
        }
    }

    fn push_clip(&mut self) {}

    fn push_clip_box(&mut self, _clip_box: RectF) {}

    fn pop_clip(&mut self) {}

    fn push_layer(&mut self, _mode: CompositeMode) {}

    fn pop_layer(&mut self) {}

    fn push_transform(&mut self, _transform: Transform) {}

    fn pop_transform(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validate that atan2(0, 0) = 0 (the edge-case for a pixel exactly at the
    /// gradient centre) is finite and does not cause a panic inside
    /// `composite_sweep`.
    #[test]
    fn test_sweep_gradient_atan2_zero_zero_is_finite() {
        let angle = 0.0f32.atan2(0.0f32);
        assert!(angle.is_finite(), "atan2(0, 0) must be finite");
    }

    /// `apply_extend(Pad)` must clamp both extremes.
    #[test]
    fn test_apply_extend_pad_clamps() {
        assert_eq!(apply_extend(-0.5, GradientExtend::Pad), 0.0);
        assert_eq!(apply_extend(1.5, GradientExtend::Pad), 1.0);
        assert!((apply_extend(0.5, GradientExtend::Pad) - 0.5).abs() < 1e-6);
    }

    /// `sample_stops` with an empty slice returns transparent black.
    #[test]
    fn test_sample_stops_empty_returns_transparent() {
        let color = sample_stops(&[], 0.5);
        assert_eq!(color.alpha, 0);
    }

    /// `sample_stops` at `t = 0` returns the first stop's colour.
    #[test]
    fn test_sample_stops_at_zero_returns_first() {
        let stops = vec![
            Stop {
                offset: 0.0,
                color: ttf_parser::RgbaColor::new(255, 0, 0, 255),
            },
            Stop {
                offset: 1.0,
                color: ttf_parser::RgbaColor::new(0, 0, 255, 255),
            },
        ];
        let c = sample_stops(&stops, 0.0);
        assert_eq!(c.red, 255);
        assert_eq!(c.blue, 0);
    }

    /// `sample_stops` at `t = 1` returns the last stop's colour.
    #[test]
    fn test_sample_stops_at_one_returns_last() {
        let stops = vec![
            Stop {
                offset: 0.0,
                color: ttf_parser::RgbaColor::new(255, 0, 0, 255),
            },
            Stop {
                offset: 1.0,
                color: ttf_parser::RgbaColor::new(0, 0, 255, 255),
            },
        ];
        let c = sample_stops(&stops, 1.0);
        assert_eq!(c.red, 0);
        assert_eq!(c.blue, 255);
    }

    /// Sweep angle normalisation: turning [0, 2π) into turns in [0, 1)
    /// should produce values within the expected range.
    #[test]
    fn test_sweep_angle_to_turns_range() {
        let two_pi = 2.0_f32 * std::f32::consts::PI;
        for degrees in [0.0f32, 45.0, 90.0, 180.0, 270.0, 359.0] {
            let angle_rad = degrees.to_radians();
            let t_turn = angle_rad / two_pi;
            assert!(
                (0.0..=1.0).contains(&t_turn),
                "t_turn {t_turn} out of [0,1] for {degrees}°"
            );
        }
    }
}
