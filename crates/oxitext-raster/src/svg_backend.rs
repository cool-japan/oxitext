//! SVG glyph rendering backend.
//!
//! Renders SVG glyph documents embedded in a font's `SVG ` table to a
//! [`oxitext_core::ColorBitmap`] using the pure-Rust `resvg` and `tiny-skia`
//! crates.
//!
//! ## Entry points
//!
//! - [`render_svg_glyph`] — extract the SVG bytes for a glyph from a font and
//!   render them to a pixel buffer.
//! - [`render_svg_bytes`] — render raw SVG bytes (not from a font) to a
//!   [`oxitext_core::ColorBitmap`]; useful for testing the rendering pipeline
//!   independently of the font-parsing step.
//!
//! ## Premultiplied-alpha note
//!
//! `tiny_skia::Pixmap` stores *premultiplied* RGBA internally.  All public
//! functions in this module convert the output to *straight* (non-premultiplied)
//! RGBA via [`tiny_skia::Pixmap::take_demultiplied`] before returning, so the
//! resulting [`oxitext_core::ColorBitmap`] is compatible with the rest of the
//! OxiText compositing pipeline.

use resvg::tiny_skia;
use resvg::usvg;

/// Render the SVG glyph for `glyph_id` from `face_data` at `px_size` pixels.
///
/// Extracts the raw SVG document bytes from the font's `SVG ` table via
/// `ttf_parser::Face::glyph_svg_image`, parses and renders the document with
/// `resvg`, and returns a [`oxitext_core::ColorBitmap`] in straight RGBA order.
///
/// Returns `None` if:
/// - `face_data` cannot be parsed as a font face,
/// - the glyph has no SVG representation (`SVG ` table absent or no record for
///   this GID),
/// - the SVG document cannot be parsed,
/// - the computed output dimensions are zero, or
/// - a `tiny-skia` pixmap cannot be allocated.
///
/// # Arguments
/// - `face_data`: raw font bytes (TTF, OTF, TTC with index 0).
/// - `glyph_id`: the glyph index to look up.
/// - `px_size`: desired output size in pixels; the SVG is scaled uniformly so
///   that the larger of its width and height equals `px_size`.
pub fn render_svg_glyph(
    face_data: &[u8],
    glyph_id: u16,
    px_size: u16,
) -> Option<oxitext_core::ColorBitmap> {
    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let svg_doc = face.glyph_svg_image(gid)?;
    render_svg_bytes(svg_doc.data, px_size)
}

/// Render raw SVG bytes to a [`oxitext_core::ColorBitmap`] at `px_size` pixels.
///
/// The SVG is scaled uniformly so that the larger of its declared width and
/// height equals `px_size`.  Returns `None` if the data cannot be parsed or
/// the computed output dimensions are zero.
///
/// The returned bitmap is in straight (non-premultiplied) RGBA byte order with
/// `width * height * 4` bytes.
pub fn render_svg_bytes(svg_data: &[u8], px_size: u16) -> Option<oxitext_core::ColorBitmap> {
    if px_size == 0 {
        return None;
    }

    // Parse SVG document.
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg_data, &options).ok()?;

    // Determine output dimensions by scaling the SVG's declared size uniformly
    // so the larger axis equals px_size.
    let size = tree.size();
    let svg_w = size.width();
    let svg_h = size.height();
    if svg_w <= 0.0 || svg_h <= 0.0 {
        return None;
    }

    let scale_x = px_size as f32 / svg_w;
    let scale_y = px_size as f32 / svg_h;
    let scale = scale_x.min(scale_y);

    // Guard against degenerate scales.
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    let out_w = ((svg_w * scale).round() as u32).max(1);
    let out_h = ((svg_h * scale).round() as u32).max(1);

    // Allocate pixmap — tiny-skia uses premultiplied RGBA internally.
    let mut pixmap = tiny_skia::Pixmap::new(out_w, out_h)?;

    // Render the SVG tree onto the pixmap with the computed scale transform.
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // Convert premultiplied → straight RGBA before returning.
    // take_demultiplied() consumes the Pixmap and handles the conversion.
    let rgba = pixmap.take_demultiplied();

    Some(oxitext_core::ColorBitmap {
        width: out_w,
        height: out_h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-SVG font must return `None` gracefully without panicking.
    #[test]
    fn test_svg_glyph_returns_none_for_non_svg_font() {
        // Use the test fixture font (plain TTF, no SVG table).
        let font_data = include_bytes!("../../../tests/fixtures/test-font.ttf");
        let result = render_svg_glyph(font_data, 1, 32);
        assert!(
            result.is_none(),
            "plain TTF has no SVG table; expected None"
        );
    }

    /// Empty font data must return `None` without panicking.
    #[test]
    fn test_svg_glyph_empty_font_returns_none() {
        let result = render_svg_glyph(&[], 0, 32);
        assert!(result.is_none());
    }

    /// Passing `px_size = 0` must return `None` without panicking.
    #[test]
    fn test_svg_bytes_zero_size_returns_none() {
        let minimal_svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#;
        let result = render_svg_bytes(minimal_svg, 0);
        assert!(result.is_none());
    }

    /// Verify that the resvg rendering pipeline works end-to-end with a
    /// minimal inline SVG document.  This tests:
    /// - SVG parsing via `usvg::Tree::from_data`
    /// - Pixmap allocation and rendering via `resvg::render`
    /// - Premultiply→straight RGBA conversion via `take_demultiplied`
    /// - Correct `ColorBitmap` dimensions
    #[test]
    fn test_render_svg_parse() {
        // A 10×10 solid red square — the simplest possible visible SVG.
        let svg_src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
  <rect width="10" height="10" fill="red"/>
</svg>"#;

        let bm = render_svg_bytes(svg_src, 20);
        let bm = bm.expect("resvg should render a minimal SVG to a ColorBitmap");

        // Dimensions: 10×10 SVG scaled to fit 20px on the larger axis.
        // Both axes are equal so scale = 20/10 = 2.0 → 20×20.
        assert_eq!(bm.width, 20, "expected width 20, got {}", bm.width);
        assert_eq!(bm.height, 20, "expected height 20, got {}", bm.height);
        assert_eq!(
            bm.rgba.len(),
            (bm.width * bm.height * 4) as usize,
            "RGBA buffer length must equal width * height * 4"
        );

        // The bitmap should be non-trivially coloured (at least one pixel with
        // a non-zero red channel and opaque alpha, since the fill is "red").
        let has_red_pixel = bm.rgba.chunks_exact(4).any(|px| px[3] > 0 && px[0] > 0);
        assert!(
            has_red_pixel,
            "rendered red square should contain at least one opaque red pixel"
        );
    }

    /// Verify that aspect ratio scaling is applied correctly for non-square SVGs.
    ///
    /// A 20×10 SVG rendered at px_size=40 should produce a 40×20 output
    /// (scale = 40/20 = 2.0 applied uniformly).
    #[test]
    fn test_render_svg_aspect_ratio() {
        let svg_src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
  <rect width="20" height="10" fill="blue"/>
</svg>"#;

        let bm = render_svg_bytes(svg_src, 40).expect("resvg should render a wide SVG");

        assert_eq!(bm.width, 40, "expected width 40, got {}", bm.width);
        assert_eq!(bm.height, 20, "expected height 20, got {}", bm.height);
        assert_eq!(
            bm.rgba.len(),
            (bm.width * bm.height * 4) as usize,
            "RGBA buffer length must equal width * height * 4"
        );
    }

    /// Invalid (non-SVG) bytes must return `None` without panicking.
    #[test]
    fn test_render_svg_bytes_invalid_data_returns_none() {
        let result = render_svg_bytes(b"not svg data at all", 32);
        assert!(result.is_none());
    }
}
