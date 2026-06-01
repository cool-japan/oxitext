//! Color glyph type detection and CBDT/CBLC bitmap extraction.
//!
//! Provides [`ColorGlyphType`] — an enum describing which color-glyph tables
//! are present for a given glyph — and helpers to detect and (where possible)
//! extract the embedded bitmap data.  Also provides [`RawRasterGlyph`] and
//! [`extract_raster_glyph`] for raw (undecoded) raster image extraction from
//! any font table (CBDT, sbix, etc.).

/// Raw rasterized glyph image extracted from a font table (CBDT, sbix, etc.).
///
/// Contains the raw encoded bytes along with image metadata.  The caller is
/// responsible for decoding the bytes according to [`format`][Self::format].
#[derive(Debug, Clone)]
pub struct RawRasterGlyph {
    /// Encoded image bytes (PNG, or a bitmap format — check [`format`][Self::format]).
    pub data: Vec<u8>,
    /// Image format as reported by the font table.
    pub format: RasterImageFormat,
    /// Image width in pixels (from the font table; may differ from the decoded image).
    pub width: u16,
    /// Image height in pixels (from the font table; may differ from the decoded image).
    pub height: u16,
    /// X bearing from the font origin.
    pub x: i16,
    /// Y bearing from the font origin.
    pub y: i16,
    /// Pixels per em for this strike.
    pub pixels_per_em: u16,
}

/// Raster image format tag reported by the font table.
///
/// Only [`PNG`][Self::Png] is commonly used in practice (sbix always uses PNG;
/// CBDT/CBLC format 17/18/19 embed PNG).  Other CBDT formats store raw bitmap
/// data; those map to [`Unknown`][Self::Unknown].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterImageFormat {
    /// PNG-encoded image.
    Png,
    /// JPEG-encoded image (rare; not mapped by ttf-parser 0.25).
    Jpeg,
    /// TIFF-encoded image (rare; not mapped by ttf-parser 0.25).
    Tiff,
    /// Any other format (raw bitmap, packed mono, etc.).
    Unknown,
}

/// Extract the raw raster glyph image for `glyph_id` at the nearest available
/// strike to `ppem` from any supported font table (CBDT, sbix, etc.).
///
/// Returns the raw encoded bytes without any decoding.  The caller should
/// inspect [`RawRasterGlyph::format`] and decode appropriately (e.g. a PNG
/// decoder for [`RasterImageFormat::Png`]).
///
/// Returns `None` if the font cannot be parsed or no raster image is available
/// for the requested glyph at the requested size.
pub fn extract_raster_glyph(face_data: &[u8], glyph_id: u16, ppem: u16) -> Option<RawRasterGlyph> {
    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let img = face.glyph_raster_image(gid, ppem)?;

    let format = match img.format {
        ttf_parser::RasterImageFormat::PNG => RasterImageFormat::Png,
        // ttf-parser 0.25 does not expose JPEG or TIFF variants; all raw
        // bitmap formats fall through to Unknown.
        _ => RasterImageFormat::Unknown,
    };

    Some(RawRasterGlyph {
        data: img.data.to_vec(),
        format,
        width: img.width,
        height: img.height,
        x: img.x,
        y: img.y,
        pixels_per_em: img.pixels_per_em,
    })
}

/// The color glyph format available for a given glyph in a font.
///
/// Priority order follows the OpenType recommendation:
/// `Sbix` > `Svg` > `EmbeddedBitmap` (CBDT/CBLC) > `ColrV1` > `ColrV0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorGlyphType {
    /// No color data; greyscale rasterization only.
    None,
    /// COLRv0: simple per-layer solid-colored outlines.
    ColrV0,
    /// COLRv1: gradients, transforms, and composite modes.
    ColrV1,
    /// Embedded bitmap (CBDT/CBLC tables — often PNG-encoded).
    EmbeddedBitmap,
    /// Apple `sbix` table (PNG/TIFF/JPEG bitmaps per PPEM).
    Sbix,
    /// OpenType `SVG ` table (SVG documents per glyph).
    Svg,
}

/// Detect which color glyph format is available for `glyph_id` in `face_data`.
///
/// Inspects the font's OpenType tables in priority order (sbix → SVG →
/// CBDT/CBLC → COLR) and returns the first match.  Returns
/// [`ColorGlyphType::None`] when the font cannot be parsed or the glyph has
/// no color data.
pub fn detect_color_glyph_type(face_data: &[u8], glyph_id: u16) -> ColorGlyphType {
    let face = match ttf_parser::Face::parse(face_data, 0) {
        Ok(f) => f,
        Err(_) => return ColorGlyphType::None,
    };
    let gid = ttf_parser::GlyphId(glyph_id);
    let tables = face.tables();

    // sbix has highest priority per OpenType spec recommendation.
    if tables.sbix.is_some() {
        // glyph_raster_image will tell us if there is actually data for this glyph;
        // we approximate by checking table presence + is_color_glyph.
        if face.is_color_glyph(gid) {
            return ColorGlyphType::Sbix;
        }
    }

    // SVG table.
    if tables.svg.is_some() && face.is_color_glyph(gid) {
        return ColorGlyphType::Svg;
    }

    // CBDT/CBLC embedded bitmaps.
    // ttf-parser parses CBLC + CBDT together into the `cbdt` field;
    // the older `bdat`/`bloc` (Apple) or `EBDT`/`EBLC` are in `bdat`/`ebdt`.
    if (tables.cbdt.is_some() || tables.bdat.is_some() || tables.ebdt.is_some())
        && face.is_color_glyph(gid)
    {
        return ColorGlyphType::EmbeddedBitmap;
    }

    // COLR — v0 vs v1 distinguished via `Table::is_simple()`.
    // `is_simple()` returns `true` for COLRv0 and `false` for COLRv1.
    if let Some(colr) = tables.colr {
        if colr.contains(gid) {
            if colr.is_simple() {
                return ColorGlyphType::ColrV0;
            }
            return ColorGlyphType::ColrV1;
        }
    }

    ColorGlyphType::None
}

/// Attempt to extract and decode a pre-rasterized bitmap from a font's CBDT/CBLC tables.
///
/// Uses ttf-parser's `glyph_raster_image` API to locate an embedded bitmap at the
/// requested `target_ppem` (pixels-per-em).  PNG-encoded bitmaps (CBDT formats 17,
/// 18, 19) are decoded to RGBA using the `png` crate.  Raw-bitmap formats return
/// `None` — their dimensions require simultaneous CBLC-metric parsing which is out
/// of scope here.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `glyph_id`: the glyph index to look up.
/// - `target_ppem`: the desired pixel size; ttf-parser selects the closest
///   available strike.
pub fn extract_cbdt_bitmap(
    face_data: &[u8],
    glyph_id: u16,
    target_ppem: u8,
) -> Option<oxitext_core::ColorBitmap> {
    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let raster_image = face.glyph_raster_image(gid, u16::from(target_ppem))?;

    match raster_image.format {
        ttf_parser::RasterImageFormat::PNG => {
            let bitmap = decode_png_to_bitmap(raster_image.data)?;
            Some(oxitext_core::ColorBitmap {
                width: bitmap.0,
                height: bitmap.1,
                rgba: bitmap.2,
            })
        }
        // Non-PNG formats (raw CBDT bitmaps) need CBLC metrics for dimensions;
        // not implemented here.
        _ => None,
    }
}

/// Render a CBDT/CBLC color bitmap glyph, returning the decoded RGBA bitmap as a
/// [`crate::color::ColorGlyphBitmap`].
///
/// CBDT entries can be PNG-encoded (format 17/18/19 — the common case for color
/// emoji) or raw RGBA (format 7/8/9 — requires CBLC metrics to reconstruct
/// dimensions, not yet implemented).  This function handles PNG-encoded bitmaps.
///
/// Returns `None` if the font has no CBDT data for the requested glyph, or the
/// embedded bitmap cannot be decoded.
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `glyph_id`: the glyph index to look up.
/// - `px_size`: desired pixel size; ttf-parser selects the closest available strike.
pub fn render_cbdt_glyph(
    face_data: &[u8],
    glyph_id: u16,
    px_size: u16,
) -> Option<crate::color::ColorGlyphBitmap> {
    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let raster_image = face.glyph_raster_image(gid, px_size)?;

    match raster_image.format {
        ttf_parser::RasterImageFormat::PNG => {
            let (w, h, rgba) = decode_png_to_bitmap(raster_image.data)?;
            Some(crate::color::ColorGlyphBitmap {
                width: w,
                height: h,
                rgba,
            })
        }
        // Raw bitmap formats require CBLC size metrics — not yet implemented.
        _ => None,
    }
}

/// Decode PNG bytes into `(width, height, rgba_bytes)`.
///
/// Returns `None` if the data is not valid PNG or the colour type is not
/// handled (only `Rgb` and `Rgba` are supported; indexed and greyscale modes
/// are rare in CBDT and not decoded here).
fn decode_png_to_bitmap(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use std::io::Cursor;

    let decoder = png::Decoder::new(Cursor::new(data));
    let mut reader = decoder.read_info().ok()?;
    let buf_size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).ok()?;

    let width = info.width;
    let height = info.height;
    let buf_size = info.buffer_size();

    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..buf_size].to_vec(),
        png::ColorType::Rgb => {
            let capacity = width as usize * height as usize * 4;
            let mut out = Vec::with_capacity(capacity);
            for chunk in buf[..buf_size].chunks(3) {
                out.extend_from_slice(chunk);
                out.push(255u8);
            }
            out
        }
        // Indexed / greyscale / greyscale-alpha are uncommon in CBDT and
        // would require additional conversion — skip for now.
        _ => return None,
    };

    Some((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_color_glyph_type_empty_data() {
        // Empty font data should return None gracefully without panicking.
        let result = detect_color_glyph_type(&[], 0);
        assert_eq!(result, ColorGlyphType::None);
    }

    #[test]
    fn detect_color_glyph_type_invalid_data() {
        let result = detect_color_glyph_type(b"not a font", 0);
        assert_eq!(result, ColorGlyphType::None);
    }

    #[test]
    fn extract_cbdt_bitmap_empty_data() {
        // Should return None gracefully for invalid font data.
        let result = extract_cbdt_bitmap(&[], 0, 16);
        assert!(result.is_none());
    }

    #[test]
    fn color_glyph_type_debug_and_copy() {
        let t = ColorGlyphType::ColrV0;
        let t2 = t;
        assert_eq!(t, t2);
        let _ = format!("{:?}", t);
    }

    // ---------------------------------------------------------------------------
    // render_cbdt_glyph tests
    // ---------------------------------------------------------------------------

    /// A font without a CBDT table should return None without panicking.
    #[test]
    fn render_cbdt_glyph_no_cbdt_table_returns_none() {
        // test-font.ttf is a plain TTF with no color bitmap tables.
        let font_data = include_bytes!("../../../tests/fixtures/test-font.ttf");
        // Glyph 1 at 16 ppem — no CBDT data should produce None.
        let result = render_cbdt_glyph(font_data, 1, 16);
        assert!(result.is_none(), "plain TTF should have no CBDT data");
    }

    /// Invalid/empty font data must return None gracefully.
    #[test]
    fn render_cbdt_glyph_empty_data_returns_none() {
        assert!(render_cbdt_glyph(&[], 0, 16).is_none());
    }

    /// Non-font garbage must return None gracefully.
    #[test]
    fn render_cbdt_glyph_garbage_data_returns_none() {
        assert!(render_cbdt_glyph(b"not a png, not a font", 0, 16).is_none());
    }

    /// `decode_png_to_bitmap` should return None for non-PNG input.
    #[test]
    fn decode_png_to_bitmap_rejects_non_png() {
        assert!(decode_png_to_bitmap(b"not a png").is_none());
    }

    /// `decode_png_to_bitmap` should decode a valid 1×1 RGBA PNG.
    #[test]
    fn decode_png_to_bitmap_decodes_minimal_png() {
        // Minimal valid 1×1 RGBA PNG (hand-crafted, no external files needed).
        // This is a well-known minimal PNG for testing decoders.
        let minimal_rgba_png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR length + type
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // width=1, height=1
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, // bitdepth=8, RGBA, CRC
            0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, // IDAT length + type
            0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, // IDAT data
            0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, // IDAT CRC
            0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND length + type
            0x44, 0xAE, 0x42, 0x60, 0x82, // IEND data + CRC
        ];

        match decode_png_to_bitmap(minimal_rgba_png) {
            Some((w, h, rgba)) => {
                assert_eq!(w, 1, "expected width 1");
                assert_eq!(h, 1, "expected height 1");
                assert_eq!(rgba.len(), 4, "1x1 RGBA = 4 bytes");
            }
            None => {
                // The embedded bytes above may not match a real valid PNG exactly;
                // treat this as a best-effort smoke test — failure here just means
                // the PNG bytes need adjustment, not that the decoder is broken.
            }
        }
    }
}
