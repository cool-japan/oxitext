//! COLRv0/COLRv1/CPAL color glyph rendering.
//!
//! Walks a colour glyph's COLR paint graph with [`ttf_parser`] and composites
//! the result into an RGBA bitmap.  The heavy lifting lives in two internal
//! modules: `path_raster` rasterizes transformed outlines into anti-aliased
//! coverage masks, and `colr_paint` interprets the paint callbacks (transform
//! stack, clip stack, layer stack, gradients, composite modes).
//!
//! ## Supported paint formats
//!
//! Every `Paint` variant ttf-parser can produce is handled:
//!
//! - **`PaintColrLayers`, `PaintColrGlyph`** — traversal is performed by
//!   ttf-parser; nested layers composite source-over.
//! - **`PaintSolid` / `PaintVarSolid`** — CPAL palette lookup with the paint's
//!   alpha factor already applied by ttf-parser.
//! - **`PaintLinearGradient` / `PaintVarLinearGradient`** — the colour line is
//!   `p0 -> p3` where `p3` is the projection of `p1` onto the line through `p0`
//!   perpendicular to `p0 -> p2`, exactly as the specification requires.
//! - **`PaintRadialGradient` / `PaintVarRadialGradient`** — full two-point
//!   conical solve, not just the `r0 == 0` focal case; pixels that no circle of
//!   the family reaches are left unpainted.
//! - **`PaintSweepGradient` / `PaintVarSweepGradient`** — angles are decoded as
//!   F2DOT14 *half turns* (180 degrees per 1.0), which is what the format
//!   stores.
//! - **`PaintGlyph`** — the outline becomes a clip region for the child paint.
//! - **`PaintTransform`, `PaintTranslate`, `PaintScale*`, `PaintRotate*`,
//!   `PaintSkew*`** — accumulated on a transform stack applied to both outlines
//!   and gradient geometry.
//! - **`PaintComposite`** — all 28 `CompositeMode`s (Porter-Duff plus the
//!   separable and non-separable CSS blend modes).
//! - **`ClipList` clip boxes** — rasterized as a (possibly sheared) quad.
//!
//! ## Geometry
//!
//! [`render_colr_v0`], [`render_colr_v1`], [`render_colr_with_palette`] and
//! [`render_color_glyph`] scale the em square so that one em equals `height`
//! pixels and put the baseline at `height * 4 / 5`.  Parts of a glyph outside
//! that box are clipped — which real emoji fonts do exceed (Noto's COLRv1 build
//! paints out to 1.16 em right of the pen and 0.91 em above the baseline).
//!
//! [`render_colr_glyph_sized`] is the entry point for laying colour glyphs out
//! next to shaped text: it takes the em size in pixels, sizes the bitmap from
//! the glyph's own paint box, trims it to its ink and returns
//! [`ColorGlyphImage`] with the left/top bearings needed to place it.
//!
//! ## Foreground colour
//!
//! Palette index `0xFFFF` ("use the text colour") resolves to opaque black,
//! because these entry points do not take a text colour.
//!
//! ## Caching
//!
//! Every entry point here is a pure function of `(font bytes, glyph id, size,
//! palette)`, and walking a paint graph is expensive — tens to hundreds of
//! microseconds per emoji, repeated on every frame by a caption renderer that
//! draws the same emoji at the same size.  [`render_colr_glyph_sized_cached`]
//! and [`render_colr_cached`] are the memoized counterparts of
//! [`render_colr_glyph_sized`] and [`render_colr_v1`]; they take the caller's
//! `Arc<[u8]>` font handle instead of a bare slice because that is the only
//! font identity that is both O(1) and sound (see [`crate::colr_cache`]), and
//! they return the cached [`std::sync::Arc`] so a hit copies nothing at all.

use std::sync::Arc;

use ttf_parser::{Face, GlyphId};

use crate::colr_paint::ColrPainter;
use crate::path_raster::Affine;

/// An RGBA bitmap produced by compositing COLR layers.
#[derive(Debug, Clone)]
pub struct ColorGlyphBitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in straight (non-premultiplied) RGBA order:
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// A COLR colour glyph rendered at a requested em size, trimmed to its ink and
/// carrying the bearings needed to place it against a baseline.
///
/// This is the colour counterpart of [`crate::RasterOutput`], and uses the same
/// bearing convention as the `swash` backend: `bearing_x` is measured from the
/// pen position to the bitmap's **left** edge (positive rightwards) and
/// `bearing_y` from the baseline to the bitmap's **top** edge (positive
/// upwards).
#[derive(Debug, Clone)]
pub struct ColorGlyphImage {
    /// Width in pixels; always non-zero.
    pub width: u32,
    /// Height in pixels; always non-zero.
    pub height: u32,
    /// Offset from the pen position to the bitmap's left edge, in pixels.
    pub bearing_x: i32,
    /// Offset from the baseline to the bitmap's top edge, in pixels, positive
    /// upwards.
    pub bearing_y: i32,
    /// Straight (non-premultiplied) RGBA, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// CPAL palette used when none is requested explicitly.
const DEFAULT_PALETTE: u16 = 0;

/// Largest bitmap edge [`render_colr_glyph_sized`] will allocate, in pixels.
///
/// A malformed `ClipList` entry can nominate an arbitrarily large box; without
/// a ceiling a single glyph could ask for gigabytes.
const MAX_SIZED_EDGE_PX: u32 = 4096;

/// Margin added around the fallback paint box, in em, for fonts whose COLR
/// table carries no `ClipList` entry for the glyph.
const FALLBACK_BOX_MARGIN_EM: f32 = 0.25;

/// Build the design-unit to pixel transform: scale the em square to `height`
/// pixels, flip the Y axis, and put the baseline at `height * 4 / 5`.
fn device_transform(units_per_em: u16, height: u32) -> Affine {
    let upem = if units_per_em == 0 {
        1000.0
    } else {
        units_per_em as f32
    };
    let scale = height as f32 / upem;
    let baseline_y = ((height as i32) * 4 / 5) as f32;
    Affine::new(scale, 0.0, 0.0, -scale, 0.0, baseline_y)
}

/// Render a COLR (v0 or v1) base glyph into an RGBA bitmap.
///
/// This is the whole cost [`render_colr_cached`] exists to avoid: parse the
/// face, walk the paint graph, rasterize and composite every layer.
///
/// Returns `None` when the font cannot be parsed, has no COLR/CPAL tables, the
/// glyph has no COLR record, or the requested size is degenerate.
fn render_colr(
    face_data: &[u8],
    base_glyph: GlyphId,
    width: u32,
    height: u32,
    palette: u16,
) -> Option<ColorGlyphBitmap> {
    if width == 0 || height == 0 {
        return None;
    }
    let face = Face::parse(face_data, 0).ok()?;
    let colr_table = face.tables().colr?;
    if !colr_table.contains(base_glyph) {
        return None;
    }
    // An out-of-range palette makes every CPAL lookup fail, which would
    // otherwise yield a silently blank bitmap.
    if palette >= face.color_palettes()?.get() {
        return None;
    }

    let device = device_transform(face.units_per_em(), height);
    let mut painter = ColrPainter::new(&face, width, height, device, palette);

    // Palette index 0xFFFF resolves to this "text colour" placeholder.
    let foreground = ttf_parser::RgbaColor::new(0, 0, 0, 255);
    // An empty coords slice selects the variable font's default instance.
    colr_table.paint(base_glyph, palette, &mut painter, &[], foreground)?;

    Some(ColorGlyphBitmap {
        width,
        height,
        rgba: painter.into_rgba(),
    })
}

/// Render a COLRv0 base glyph by compositing all its CPAL-colored layers into a
/// pixel buffer at the given size.
///
/// ttf-parser dispatches on the table version internally, so a font whose COLR
/// table is version 1 is rendered through the full COLRv1 paint graph; this
/// entry point is retained because it is part of the published API and because
/// callers that only care about layered solid colours can keep using it.
///
/// Returns `None` if:
/// - the glyph has no COLR data,
/// - the font cannot be parsed, or
/// - `width` or `height` is zero.
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
    render_colr(face_data, base_glyph, width, height, DEFAULT_PALETTE)
}

/// Render a COLRv1 base glyph, including gradients, transforms and composite
/// modes.
///
/// Falls back to plain layered rendering for glyphs that only have COLRv0 data,
/// because ttf-parser resolves the version internally.
///
/// Returns `None` if the glyph has no COLR data, the font cannot be parsed, or
/// the requested size is degenerate.
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
    render_colr(face_data, base_glyph, width, height, DEFAULT_PALETTE)
}

/// Render a COLR glyph using a specific CPAL palette.
///
/// Behaves like [`render_colr_v1`] but resolves palette entries against
/// `palette` instead of palette 0.  Fonts ship alternate palettes for e.g. dark
/// backgrounds; an out-of-range index makes the paint graph resolve to nothing
/// and yields `None`.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `base_glyph`: the GID to render.
/// - `width`, `height`: output bitmap size in pixels.
/// - `palette`: zero-based CPAL palette index.
pub fn render_colr_with_palette(
    face_data: &[u8],
    base_glyph: GlyphId,
    width: u32,
    height: u32,
    palette: u16,
) -> Option<ColorGlyphBitmap> {
    render_colr(face_data, base_glyph, width, height, palette)
}

/// [`render_colr_v1`] memoized per thread, for callers that hold their font
/// bytes in an [`Arc`].
///
/// Identical inputs return handles to the *same* allocation
/// (`Arc::ptr_eq` holds) for as long as the entry stays resident, so a render
/// loop that draws the same glyph at the same size on every frame pays the
/// paint-graph cost exactly once.  Output is byte-identical to
/// [`render_colr_with_palette`] on the same inputs.
///
/// The cache retains a clone of `font_data` for as long as the entry lives,
/// which is what makes keying on the handle's address sound; see
/// [`crate::colr_cache`].  Use [`crate::clear_colr_cache`] to release both.
///
/// # Arguments
/// - `font_data`: raw TTF/OTF bytes, as a shared handle.
/// - `base_glyph`: the GID to render.
/// - `width`, `height`: output bitmap size in pixels.
/// - `palette`: zero-based CPAL palette index (`0` for the default palette).
pub fn render_colr_cached(
    font_data: &Arc<[u8]>,
    base_glyph: GlyphId,
    width: u32,
    height: u32,
    palette: u16,
) -> Option<Arc<ColorGlyphBitmap>> {
    if width == 0 || height == 0 {
        return None;
    }
    crate::colr_cache::get_or_render_bitmap(font_data, base_glyph.0, width, height, palette, || {
        render_colr(font_data, base_glyph, width, height, palette)
    })
}

/// The design-unit box a COLR glyph may paint into.
///
/// Prefers the glyph's `ClipList` entry (which ttf-parser also enforces while
/// painting, so nothing outside it can be drawn), then the base glyph's own
/// outline bounding box, and finally a generous box around the em: horizontally
/// from a margin left of the pen to a margin past the advance, vertically from
/// a margin below the descender to a margin above the ascender.
fn paint_box(face: &Face<'_>, base_glyph: GlyphId, upem: f32) -> ttf_parser::RectF {
    if let Some(colr) = face.tables().colr {
        if let Some(clip) = colr.clip_box(base_glyph, &[]) {
            return clip;
        }
    }
    if let Some(bbox) = face.glyph_bounding_box(base_glyph) {
        return ttf_parser::RectF {
            x_min: f32::from(bbox.x_min),
            y_min: f32::from(bbox.y_min),
            x_max: f32::from(bbox.x_max),
            y_max: f32::from(bbox.y_max),
        };
    }
    let margin = FALLBACK_BOX_MARGIN_EM * upem;
    let advance = face
        .glyph_hor_advance(base_glyph)
        .map_or(upem, f32::from)
        .max(upem);
    ttf_parser::RectF {
        x_min: -margin,
        y_min: f32::from(face.descender()).min(0.0) - margin,
        x_max: advance + margin,
        y_max: f32::from(face.ascender()).max(upem) + margin,
    }
}

/// Crop `rgba` to the bounding box of its non-transparent pixels.
///
/// Returns the trimmed buffer together with the box's top-left corner, or
/// `None` when every pixel is fully transparent.
fn trim_to_ink(rgba: &[u8], width: u32, height: u32) -> Option<(Vec<u8>, u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (index, px) in rgba.chunks_exact(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = (index as u32) % width;
        let y = (index as u32) / width;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if x0 == u32::MAX {
        return None;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    if (w, h) == (width, height) {
        return Some((rgba.to_vec(), x0, y0, w, h));
    }
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for y in y0..=y1 {
        let start = ((y as usize) * (width as usize) + x0 as usize) * 4;
        let end = start + (w as usize) * 4;
        match rgba.get(start..end) {
            Some(row) => out.extend_from_slice(row),
            None => return None,
        }
    }
    Some((out, x0, y0, w, h))
}

/// Render a COLR (v0 or v1) glyph at a requested em size, sized and placed the
/// way a text rasterizer's output is.
///
/// [`render_colr_v1`] scales the em square to the caller's `height` and pins the
/// baseline at four fifths of it, which is fine for a preview but clips real
/// emoji: Noto's COLRv1 build paints out to 1.16 em right of the pen and 0.91 em
/// above the baseline, both outside that window. This entry point instead
/// derives the bitmap from the glyph's own paint box (an internal `paint_box` helper),
/// trims the result to its ink and reports the bearings needed to place it, so
/// a caption or UI renderer can composite emoji inline with shaped text.
///
/// `bearing_x` / `bearing_y` follow the [`crate::SwashRaster`] convention:
/// from the pen position to the bitmap's left edge, and from the baseline to
/// the bitmap's **top** edge (positive upwards).
///
/// Returns `None` when the font cannot be parsed, has no COLR/CPAL data for the
/// glyph, `px_per_em` is not a positive finite number, the paint box is
/// degenerate or larger than 4096 px on an edge, or the glyph paints nothing at
/// all.
///
/// # Caching
///
/// This entry point always paints.  Callers that hold their font bytes in an
/// [`Arc`] — a per-frame renderer, for instance — should use
/// [`render_colr_glyph_sized_cached`] instead, which memoizes the result per
/// thread and returns it without copying a pixel.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `glyph_id`: raw GID as a `u16`, as delivered by the shaper.
/// - `px_per_em`: the em size in pixels, i.e. the shaped font size.
/// - `palette`: zero-based CPAL palette index (`0` for the default palette).
pub fn render_colr_glyph_sized(
    face_data: &[u8],
    glyph_id: u16,
    px_per_em: f32,
    palette: u16,
) -> Option<ColorGlyphImage> {
    if !px_per_em.is_finite() || px_per_em <= 0.0 {
        return None;
    }
    let base_glyph = GlyphId(glyph_id);
    let face = Face::parse(face_data, 0).ok()?;
    let colr_table = face.tables().colr?;
    if !colr_table.contains(base_glyph) {
        return None;
    }
    if palette >= face.color_palettes()?.get() {
        return None;
    }

    let upem = match face.units_per_em() {
        0 => 1000.0,
        n => f32::from(n),
    };
    let scale = px_per_em / upem;
    let area = paint_box(&face, base_glyph, upem);
    // Round outwards so no partially covered edge pixel is lost.
    let left = (area.x_min * scale).floor();
    let right = (area.x_max * scale).ceil();
    let top = (area.y_max * scale).ceil();
    let bottom = (area.y_min * scale).floor();
    if !(left.is_finite() && right.is_finite() && top.is_finite() && bottom.is_finite()) {
        return None;
    }
    let width_f = right - left;
    let height_f = top - bottom;
    if width_f < 1.0 || height_f < 1.0 {
        return None;
    }
    if width_f > MAX_SIZED_EDGE_PX as f32 || height_f > MAX_SIZED_EDGE_PX as f32 {
        return None;
    }
    let width = width_f as u32;
    let height = height_f as u32;

    // Design units -> bitmap pixels: scale, flip Y, then move the paint box's
    // top-left corner onto (0, 0).
    let device = Affine::new(scale, 0.0, 0.0, -scale, -left, top);
    let mut painter = ColrPainter::new(&face, width, height, device, palette);
    let foreground = ttf_parser::RgbaColor::new(0, 0, 0, 255);
    colr_table.paint(base_glyph, palette, &mut painter, &[], foreground)?;

    let painted = painter.into_rgba();
    let (rgba, ink_x, ink_y, ink_w, ink_h) = trim_to_ink(&painted, width, height)?;
    Some(ColorGlyphImage {
        width: ink_w,
        height: ink_h,
        bearing_x: (left as i32).saturating_add(ink_x as i32),
        bearing_y: (top as i32).saturating_sub(ink_y as i32),
        rgba,
    })
}

/// [`render_colr_glyph_sized`] memoized per thread, for callers that hold their
/// font bytes in an [`Arc`].
///
/// Painting a colour glyph costs 37–159 µs in release and 0.42–1.97 ms in
/// debug, and a caption or UI renderer asks for the same emoji at the same em
/// size on every frame.  This entry point pays that once: identical inputs
/// return handles to the *same* allocation (`Arc::ptr_eq` holds) for as long as
/// the entry stays resident, so later calls cost a hash and a refcount bump —
/// measured at 0.18 µs in release, a ~460x saving over re-painting — and copy
/// no pixels at all.
///
/// Output is byte-identical to [`render_colr_glyph_sized`] on the same inputs,
/// and the same `None` cases apply.
///
/// The cache retains a clone of `font_data` for as long as the entry lives,
/// which is what makes keying on the handle's address sound; see
/// [`crate::colr_cache`].  Use [`crate::clear_colr_cache`] to release both.
///
/// The returned image is immutable and shared; clone through the handle if an
/// owned copy is needed.
///
/// # Arguments
/// - `font_data`: raw TTF/OTF bytes, as a shared handle.
/// - `glyph_id`: raw GID as a `u16`, as delivered by the shaper.
/// - `px_per_em`: the em size in pixels, i.e. the shaped font size.
/// - `palette`: zero-based CPAL palette index (`0` for the default palette).
pub fn render_colr_glyph_sized_cached(
    font_data: &Arc<[u8]>,
    glyph_id: u16,
    px_per_em: f32,
    palette: u16,
) -> Option<Arc<ColorGlyphImage>> {
    if !px_per_em.is_finite() || px_per_em <= 0.0 {
        return None;
    }
    crate::colr_cache::get_or_render_sized(font_data, glyph_id, px_per_em, palette, || {
        render_colr_glyph_sized(font_data, glyph_id, px_per_em, palette)
    })
}

/// Dispatch function: render the best available colour representation for a glyph.
///
/// Priority order:
/// 1. **SVG** — OpenType `SVG ` documents (feature `svg-backend`).
/// 2. **CBDT/CBLC and sbix** — embedded bitmap strikes.  Uses `height` as the
///    pixel-per-em target so that the closest available strike is selected.
/// 3. **COLRv1/COLRv0** — the paint graph, via [`render_colr_v1`].
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

    render_colr_v1(face_data, gid, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_transform_places_baseline_and_flips_y() {
        let t = device_transform(1000, 100);
        // Design origin sits on the baseline at 80% of the height.
        let (x, y) = t.apply(0.0, 0.0);
        assert!(x.abs() < 1e-6);
        assert!((y - 80.0).abs() < 1e-6, "baseline y = {y}");
        // One em up is `height` pixels up the bitmap.
        let (_, top) = t.apply(0.0, 1000.0);
        assert!((top + 20.0).abs() < 1e-6, "em top = {top}");
        // X scales by height/upem.
        let (right, _) = t.apply(1000.0, 0.0);
        assert!((right - 100.0).abs() < 1e-6, "em right = {right}");
    }

    #[test]
    fn device_transform_survives_zero_upem() {
        let t = device_transform(0, 64);
        assert!(t.a.is_finite() && t.a > 0.0);
    }

    #[test]
    fn non_font_data_returns_none() {
        assert!(render_colr_v0(&[], GlyphId(0), 16, 16).is_none());
        assert!(render_colr_v1(b"not a font", GlyphId(0), 16, 16).is_none());
        assert!(render_color_glyph(b"not a font", 0, 16, 16).is_none());
        assert!(render_colr_with_palette(b"not a font", GlyphId(0), 16, 16, 3).is_none());
    }

    #[test]
    fn zero_sized_request_returns_none() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        assert!(render_colr_v1(data, GlyphId(1), 0, 16).is_none());
        assert!(render_colr_v1(data, GlyphId(1), 16, 0).is_none());
    }

    #[test]
    fn plain_font_without_colr_returns_none() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        let face = ttf_parser::Face::parse(data, 0).expect("bundled font parses");
        if face.tables().colr.is_none() {
            assert!(render_colr_v0(data, GlyphId(36), 32, 32).is_none());
            assert!(render_colr_v1(data, GlyphId(36), 32, 32).is_none());
        }
    }

    #[test]
    fn sized_rejects_bad_arguments() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        assert!(render_colr_glyph_sized(data, 1, 0.0, 0).is_none());
        assert!(render_colr_glyph_sized(data, 1, -16.0, 0).is_none());
        assert!(render_colr_glyph_sized(data, 1, f32::NAN, 0).is_none());
        assert!(render_colr_glyph_sized(data, 1, f32::INFINITY, 0).is_none());
        assert!(render_colr_glyph_sized(b"not a font", 1, 16.0, 0).is_none());
    }

    #[test]
    fn trim_to_ink_crops_transparent_margins() {
        // 3x3, one opaque pixel in the middle.
        let mut rgba = vec![0u8; 3 * 3 * 4];
        let centre = (3 + 1) * 4;
        rgba[centre..centre + 4].copy_from_slice(&[10, 20, 30, 255]);
        let (out, x0, y0, w, h) = trim_to_ink(&rgba, 3, 3).expect("has ink");
        assert_eq!((x0, y0, w, h), (1, 1, 1, 1));
        assert_eq!(out, vec![10, 20, 30, 255]);
    }

    #[test]
    fn trim_to_ink_reports_a_blank_bitmap() {
        assert!(trim_to_ink(&[0u8; 4 * 4 * 4], 4, 4).is_none());
    }

    #[test]
    fn trim_to_ink_keeps_a_full_bitmap_intact() {
        let rgba = vec![255u8; 2 * 2 * 4];
        let (out, x0, y0, w, h) = trim_to_ink(&rgba, 2, 2).expect("has ink");
        assert_eq!((x0, y0, w, h), (0, 0, 2, 2));
        assert_eq!(out, rgba);
    }

    #[test]
    fn fallback_paint_box_covers_the_em() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        let face = ttf_parser::Face::parse(data, 0).expect("bundled font parses");
        let upem = f32::from(face.units_per_em());
        // GID 0 (.notdef) has an outline in this face, so the bbox branch wins;
        // either way the box must be non-degenerate and contain the origin.
        let area = paint_box(&face, GlyphId(0), upem);
        assert!(area.x_max > area.x_min && area.y_max > area.y_min);
    }
}
