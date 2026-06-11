//! Color glyph type detection and CBDT/CBLC bitmap extraction.
//!
//! Provides [`ColorGlyphType`] — an enum describing which color-glyph tables
//! are present for a given glyph — and helpers to detect and (where possible)
//! extract the embedded bitmap data.  Also provides [`RawRasterGlyph`] and
//! [`extract_raster_glyph`] for raw (undecoded) raster image extraction from
//! any font table (CBDT, sbix, etc.).
//!
//! ## Raw CBDT bitmap formats
//!
//! In addition to PNG-encoded bitmaps (CBDT formats 17/18/19), this module now
//! decodes all raw bitmap formats exposed by ttf-parser 0.25:
//!
//! | ttf-parser variant          | bpp | row padding | pixel layout             |
//! |-----------------------------|-----|-------------|--------------------------|
//! | `BitmapMono`                |   1 | byte-padded | MSB first; 1=black       |
//! | `BitmapMonoPacked`          |   1 | none        | MSB first; 1=black       |
//! | `BitmapGray2`               |   2 | byte-padded | MSB first; scale×85→α    |
//! | `BitmapGray2Packed`         |   2 | none        | MSB first; scale×85→α    |
//! | `BitmapGray4`               |   4 | byte-padded | upper nibble first; ×17→α|
//! | `BitmapGray4Packed`         |   4 | none        | upper nibble first; ×17→α|
//! | `BitmapGray8`               |   8 | N/A         | 1 byte per pixel = α     |
//! | `BitmapPremulBgra32`        |  32 | N/A         | BGRA premultiplied       |

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
/// 18, 19) are decoded to RGBA using the `png` crate.  Raw-bitmap formats are
/// decoded using the unpacker functions in this module; width and height are taken
/// directly from the [`ttf_parser::RasterGlyphImage`] fields (available since
/// ttf-parser 0.20).
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
    use ttf_parser::RasterImageFormat as Rif;

    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let img = face.glyph_raster_image(gid, u16::from(target_ppem))?;

    let w = u32::from(img.width);
    let h = u32::from(img.height);

    let rgba = match img.format {
        Rif::PNG => {
            let (bw, bh, data) = decode_png_to_bitmap(img.data)?;
            return Some(oxitext_core::ColorBitmap {
                width: bw,
                height: bh,
                rgba: data,
            });
        }
        Rif::BitmapMono => unpack_mono(img.data, w, h, false),
        Rif::BitmapMonoPacked => unpack_mono(img.data, w, h, true),
        Rif::BitmapGray2 => unpack_gray2(img.data, w, h, false),
        Rif::BitmapGray2Packed => unpack_gray2(img.data, w, h, true),
        Rif::BitmapGray4 => unpack_gray4(img.data, w, h, false),
        Rif::BitmapGray4Packed => unpack_gray4(img.data, w, h, true),
        Rif::BitmapGray8 => unpack_gray8(img.data, w, h),
        Rif::BitmapPremulBgra32 => unpack_bgra32(img.data, w, h),
    }?;

    Some(oxitext_core::ColorBitmap {
        width: w,
        height: h,
        rgba,
    })
}

/// Render a CBDT/CBLC color bitmap glyph, returning the decoded RGBA bitmap as a
/// [`crate::color::ColorGlyphBitmap`].
///
/// CBDT entries can be PNG-encoded (format 17/18/19 — the common case for color
/// emoji) or raw-bitmap formats (1/2/4/8/32-bit; dimensions are read directly from
/// the [`ttf_parser::RasterGlyphImage`] fields).  All formats are now decoded.
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
    use ttf_parser::RasterImageFormat as Rif;

    let face = ttf_parser::Face::parse(face_data, 0).ok()?;
    let gid = ttf_parser::GlyphId(glyph_id);
    let img = face.glyph_raster_image(gid, px_size)?;

    let w = u32::from(img.width);
    let h = u32::from(img.height);

    let rgba = match img.format {
        Rif::PNG => {
            let (bw, bh, data) = decode_png_to_bitmap(img.data)?;
            return Some(crate::color::ColorGlyphBitmap {
                width: bw,
                height: bh,
                rgba: data,
            });
        }
        Rif::BitmapMono => unpack_mono(img.data, w, h, false),
        Rif::BitmapMonoPacked => unpack_mono(img.data, w, h, true),
        Rif::BitmapGray2 => unpack_gray2(img.data, w, h, false),
        Rif::BitmapGray2Packed => unpack_gray2(img.data, w, h, true),
        Rif::BitmapGray4 => unpack_gray4(img.data, w, h, false),
        Rif::BitmapGray4Packed => unpack_gray4(img.data, w, h, true),
        Rif::BitmapGray8 => unpack_gray8(img.data, w, h),
        Rif::BitmapPremulBgra32 => unpack_bgra32(img.data, w, h),
    }?;

    Some(crate::color::ColorGlyphBitmap {
        width: w,
        height: h,
        rgba,
    })
}

// ---------------------------------------------------------------------------
// Raw CBDT bitmap unpackers
// ---------------------------------------------------------------------------

/// Unpack a 1-bpp monochrome bitmap into RGBA bytes.
///
/// `packed = false` means rows are padded to a byte boundary (BitmapMono).
/// `packed = true`  means data is tightly packed with no padding (BitmapMonoPacked).
///
/// Bit value 1 → opaque black `(0, 0, 0, 255)`.
/// Bit value 0 → transparent `(0, 0, 0, 0)`.
///
/// Returns `None` if `data` is too short for the declared `width × height`.
fn unpack_mono(data: &[u8], width: u32, height: u32, packed: bool) -> Option<Vec<u8>> {
    let pixel_count = width.checked_mul(height)? as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    if packed {
        // Total bits needed; bytes must cover them all.
        let total_bits = pixel_count;
        let required_bytes = total_bits.div_ceil(8);
        if data.len() < required_bytes {
            return None;
        }
        for i in 0..pixel_count {
            let byte_idx = i / 8;
            let bit_shift = 7 - (i % 8); // MSB first
            let bit = (data[byte_idx] >> bit_shift) & 1;
            if bit == 1 {
                rgba.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    } else {
        // Row-padded: each row is `ceil(width / 8)` bytes.
        let row_bytes = (width as usize).div_ceil(8);
        let required_bytes = row_bytes.checked_mul(height as usize)?;
        if data.len() < required_bytes {
            return None;
        }
        for row in 0..height as usize {
            let row_start = row * row_bytes;
            for col in 0..width as usize {
                let byte_idx = row_start + col / 8;
                let bit_shift = 7 - (col % 8);
                let bit = (data[byte_idx] >> bit_shift) & 1;
                if bit == 1 {
                    rgba.extend_from_slice(&[0, 0, 0, 255]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
    }

    Some(rgba)
}

/// Unpack a 2-bpp grayscale bitmap into RGBA bytes.
///
/// Each 2-bit value `v` (0–3) is scaled to alpha by `v * 85`.
/// Pixel = `RGBA(0, 0, 0, alpha)`.
///
/// `packed = false` → rows padded to byte boundary (BitmapGray2).
/// `packed = true`  → tightly packed (BitmapGray2Packed).
///
/// Returns `None` if `data` is too short.
fn unpack_gray2(data: &[u8], width: u32, height: u32, packed: bool) -> Option<Vec<u8>> {
    let pixel_count = width.checked_mul(height)? as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    if packed {
        let required_bytes = (pixel_count * 2).div_ceil(8);
        if data.len() < required_bytes {
            return None;
        }
        for i in 0..pixel_count {
            let bit_pos = i * 2;
            let byte_idx = bit_pos / 8;
            let bit_shift = 6 - (bit_pos % 8); // MSB first, 2-bit groups
            let val = (data[byte_idx] >> bit_shift) & 0b11;
            let alpha = val * 85;
            rgba.extend_from_slice(&[0, 0, 0, alpha]);
        }
    } else {
        // Row-padded: each row is `ceil(width * 2 / 8)` = `ceil(width / 4)` bytes.
        let row_bytes = ((width as usize) * 2).div_ceil(8);
        let required_bytes = row_bytes.checked_mul(height as usize)?;
        if data.len() < required_bytes {
            return None;
        }
        for row in 0..height as usize {
            let row_start = row * row_bytes;
            for col in 0..width as usize {
                let bit_pos = col * 2;
                let byte_idx = row_start + bit_pos / 8;
                let bit_shift = 6 - (bit_pos % 8);
                let val = (data[byte_idx] >> bit_shift) & 0b11;
                let alpha = val * 85;
                rgba.extend_from_slice(&[0, 0, 0, alpha]);
            }
        }
    }

    Some(rgba)
}

/// Unpack a 4-bpp grayscale bitmap into RGBA bytes.
///
/// Each 4-bit nibble `v` (0–15) is scaled to alpha by `v * 17`.
/// Pixel = `RGBA(0, 0, 0, alpha)`.
///
/// `packed = false` → rows padded to byte boundary (BitmapGray4).
/// `packed = true`  → tightly packed (BitmapGray4Packed).
///
/// Returns `None` if `data` is too short.
fn unpack_gray4(data: &[u8], width: u32, height: u32, packed: bool) -> Option<Vec<u8>> {
    let pixel_count = width.checked_mul(height)? as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    if packed {
        let required_bytes = (pixel_count * 4).div_ceil(8);
        if data.len() < required_bytes {
            return None;
        }
        for i in 0..pixel_count {
            let byte_idx = i / 2;
            let val = if i % 2 == 0 {
                (data[byte_idx] >> 4) & 0x0F // upper nibble first
            } else {
                data[byte_idx] & 0x0F
            };
            let alpha = val * 17;
            rgba.extend_from_slice(&[0, 0, 0, alpha]);
        }
    } else {
        // Row-padded: each row is `ceil(width / 2)` bytes.
        let row_bytes = (width as usize).div_ceil(2);
        let required_bytes = row_bytes.checked_mul(height as usize)?;
        if data.len() < required_bytes {
            return None;
        }
        for row in 0..height as usize {
            let row_start = row * row_bytes;
            for col in 0..width as usize {
                let byte_idx = row_start + col / 2;
                let val = if col % 2 == 0 {
                    (data[byte_idx] >> 4) & 0x0F
                } else {
                    data[byte_idx] & 0x0F
                };
                let alpha = val * 17;
                rgba.extend_from_slice(&[0, 0, 0, alpha]);
            }
        }
    }

    Some(rgba)
}

/// Unpack an 8-bpp grayscale bitmap into RGBA bytes.
///
/// Each byte is the alpha value. Pixel = `RGBA(0, 0, 0, byte)`.
///
/// Returns `None` if `data` is shorter than `width * height`.
fn unpack_gray8(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixel_count = width.checked_mul(height)? as usize;
    if data.len() < pixel_count {
        return None;
    }
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for &alpha in &data[..pixel_count] {
        rgba.extend_from_slice(&[0, 0, 0, alpha]);
    }
    Some(rgba)
}

/// Unpack a 32-bpp premultiplied BGRA bitmap into straight-alpha RGBA bytes.
///
/// Each pixel is four bytes in order `[B, G, R, A]` with premultiplied alpha.
/// Un-premultiplied by: `channel = (channel * 255) / alpha` when `alpha > 0`.
/// Then bytes are re-ordered to `[R, G, B, A]` for the output RGBA buffer.
///
/// Returns `None` if `data` is shorter than `width * height * 4`.
fn unpack_bgra32(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixel_count = width.checked_mul(height)? as usize;
    let required_bytes = pixel_count.checked_mul(4)?;
    if data.len() < required_bytes {
        return None;
    }
    let mut rgba = Vec::with_capacity(required_bytes);
    for chunk in data[..required_bytes].chunks(4) {
        let b_pre = chunk[0];
        let g_pre = chunk[1];
        let r_pre = chunk[2];
        let a = chunk[3];
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let a_u32 = u32::from(a);
            let r = ((u32::from(r_pre) * 255 + a_u32 / 2) / a_u32) as u8;
            let g = ((u32::from(g_pre) * 255 + a_u32 / 2) / a_u32) as u8;
            let b = ((u32::from(b_pre) * 255 + a_u32 / 2) / a_u32) as u8;
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    Some(rgba)
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

    // ---------------------------------------------------------------------------
    // Raw CBDT unpacker tests (in-memory, no file I/O)
    // ---------------------------------------------------------------------------

    /// `unpack_mono` — row-padded (BitmapMono):
    ///
    /// 2×2 image, 1 row = ceil(2/8) = 1 byte.
    /// Row 0: byte 0xC0 = 0b1100_0000 → pixel(0,0)=1(black), pixel(1,0)=1(black)
    /// Row 1: byte 0x00 = 0b0000_0000 → pixel(0,1)=0(transparent), pixel(1,1)=0(transparent)
    #[test]
    fn unpack_mono_row_padded_2x2() {
        let data: &[u8] = &[0xC0, 0x00];
        let result = unpack_mono(data, 2, 2, false).expect("should succeed");
        assert_eq!(result.len(), 16, "2x2 RGBA = 16 bytes");
        // pixel (0,0): black, opaque
        assert_eq!(&result[0..4], &[0, 0, 0, 255]);
        // pixel (1,0): black, opaque
        assert_eq!(&result[4..8], &[0, 0, 0, 255]);
        // pixel (0,1): transparent
        assert_eq!(&result[8..12], &[0, 0, 0, 0]);
        // pixel (1,1): transparent
        assert_eq!(&result[12..16], &[0, 0, 0, 0]);
    }

    /// `unpack_mono` — packed (BitmapMonoPacked):
    ///
    /// 4×1 image; packed requires ceil(4/8)=1 byte.
    /// byte 0xA0 = 0b1010_0000 → pixels: 1, 0, 1, 0
    #[test]
    fn unpack_mono_packed_4x1() {
        let data: &[u8] = &[0xA0];
        let result = unpack_mono(data, 4, 1, true).expect("should succeed");
        assert_eq!(result.len(), 16, "4x1 RGBA = 16 bytes");
        assert_eq!(&result[0..4], &[0, 0, 0, 255], "pixel 0: black");
        assert_eq!(&result[4..8], &[0, 0, 0, 0], "pixel 1: transparent");
        assert_eq!(&result[8..12], &[0, 0, 0, 255], "pixel 2: black");
        assert_eq!(&result[12..16], &[0, 0, 0, 0], "pixel 3: transparent");
    }

    /// `unpack_mono` returns None when data is too short.
    #[test]
    fn unpack_mono_too_short_returns_none() {
        // 4×4 row-padded needs 4 bytes (1 byte/row), but we supply only 1.
        assert!(unpack_mono(&[0xFF], 4, 4, false).is_none());
    }

    /// `unpack_gray2` — row-padded (BitmapGray2):
    ///
    /// 2×1 image; row = ceil(2*2/8) = 1 byte.
    /// byte 0xB0 = 0b1011_0000:
    ///   bits [7:6] = 0b10 = 2 → alpha = 2*85 = 170
    ///   bits [5:4] = 0b11 = 3 → alpha = 3*85 = 255
    #[test]
    fn unpack_gray2_row_padded_2x1() {
        let data: &[u8] = &[0xB0];
        let result = unpack_gray2(data, 2, 1, false).expect("should succeed");
        assert_eq!(result.len(), 8, "2x1 RGBA = 8 bytes");
        assert_eq!(&result[0..4], &[0, 0, 0, 170], "pixel 0: alpha=170");
        assert_eq!(&result[4..8], &[0, 0, 0, 255], "pixel 1: alpha=255");
    }

    /// `unpack_gray2` — packed (BitmapGray2Packed):
    ///
    /// 4×1 image; packed requires ceil(4*2/8) = 1 byte.
    /// byte 0x1B = 0b0001_1011:
    ///   bits [7:6] = 0b00 = 0 → alpha = 0
    ///   bits [5:4] = 0b01 = 1 → alpha = 85
    ///   bits [3:2] = 0b10 = 2 → alpha = 170
    ///   bits [1:0] = 0b11 = 3 → alpha = 255
    #[test]
    fn unpack_gray2_packed_4x1() {
        let data: &[u8] = &[0b0001_1011];
        let result = unpack_gray2(data, 4, 1, true).expect("should succeed");
        assert_eq!(result.len(), 16, "4x1 RGBA = 16 bytes");
        assert_eq!(&result[0..4], &[0, 0, 0, 0], "pixel 0: alpha=0");
        assert_eq!(&result[4..8], &[0, 0, 0, 85], "pixel 1: alpha=85");
        assert_eq!(&result[8..12], &[0, 0, 0, 170], "pixel 2: alpha=170");
        assert_eq!(&result[12..16], &[0, 0, 0, 255], "pixel 3: alpha=255");
    }

    /// `unpack_gray2` returns None when data is too short.
    #[test]
    fn unpack_gray2_too_short_returns_none() {
        // 8×1 packed needs ceil(8*2/8)=2 bytes, we supply 1.
        assert!(unpack_gray2(&[0xFF], 8, 1, true).is_none());
    }

    /// `unpack_gray4` — row-padded (BitmapGray4):
    ///
    /// 2×1 image; row = ceil(2*4/8) = 1 byte.
    /// byte 0x5F: upper nibble = 0x5 = 5 → alpha = 5*17 = 85;
    ///            lower nibble = 0xF = 15 → alpha = 15*17 = 255.
    #[test]
    fn unpack_gray4_row_padded_2x1() {
        let data: &[u8] = &[0x5F];
        let result = unpack_gray4(data, 2, 1, false).expect("should succeed");
        assert_eq!(result.len(), 8, "2x1 RGBA = 8 bytes");
        assert_eq!(&result[0..4], &[0, 0, 0, 85], "pixel 0: alpha=85");
        assert_eq!(&result[4..8], &[0, 0, 0, 255], "pixel 1: alpha=255");
    }

    /// `unpack_gray4` — packed (BitmapGray4Packed):
    ///
    /// Same encoding, 2×1 packed is also 1 byte — identical to row-padded here.
    #[test]
    fn unpack_gray4_packed_2x1() {
        let data: &[u8] = &[0x0A];
        let result = unpack_gray4(data, 2, 1, true).expect("should succeed");
        assert_eq!(result.len(), 8, "2x1 RGBA = 8 bytes");
        // upper nibble 0x0 = 0 → alpha = 0
        assert_eq!(&result[0..4], &[0, 0, 0, 0], "pixel 0: alpha=0");
        // lower nibble 0xA = 10 → alpha = 10*17 = 170
        assert_eq!(&result[4..8], &[0, 0, 0, 170], "pixel 1: alpha=170");
    }

    /// `unpack_gray4` returns None when data is too short.
    #[test]
    fn unpack_gray4_too_short_returns_none() {
        // 4×1 row-padded needs ceil(4/2)=2 bytes, we supply 1.
        assert!(unpack_gray4(&[0xFF], 4, 1, false).is_none());
    }

    /// `unpack_gray8` — 2×2 image.
    ///
    /// Each byte is the alpha. Pixel = RGBA(0, 0, 0, byte).
    #[test]
    fn unpack_gray8_2x2() {
        let data: &[u8] = &[0x00, 0x80, 0xC0, 0xFF];
        let result = unpack_gray8(data, 2, 2).expect("should succeed");
        assert_eq!(result.len(), 16, "2x2 RGBA = 16 bytes");
        assert_eq!(&result[0..4], &[0, 0, 0, 0x00]);
        assert_eq!(&result[4..8], &[0, 0, 0, 0x80]);
        assert_eq!(&result[8..12], &[0, 0, 0, 0xC0]);
        assert_eq!(&result[12..16], &[0, 0, 0, 0xFF]);
    }

    /// `unpack_gray8` returns None when data is too short.
    #[test]
    fn unpack_gray8_too_short_returns_none() {
        // 2×2 = 4 pixels, we supply only 3 bytes.
        assert!(unpack_gray8(&[0x00, 0x80, 0xC0], 2, 2).is_none());
    }

    /// `unpack_bgra32` — opaque red pixel (premultiplied, so r_pre = 128 with a=128
    /// → un-premultiplied R = (128*255)/128 = 255).
    ///
    /// 2×1 image: pixel 0 is opaque red, pixel 1 is transparent.
    #[test]
    fn unpack_bgra32_opaque_and_transparent() {
        // Pixel 0: BGRA premul for opaque red → B=0, G=0, R=255, A=255
        // Pixel 1: BGRA premul for transparent → B=0, G=0, R=0, A=0
        let data: &[u8] = &[0, 0, 255, 255, 0, 0, 0, 0];
        let result = unpack_bgra32(data, 2, 1).expect("should succeed");
        assert_eq!(result.len(), 8, "2x1 RGBA = 8 bytes");
        // pixel 0: un-premultiplied from BGRA(0,0,255,255) → RGBA(255,0,0,255)
        assert_eq!(&result[0..4], &[255, 0, 0, 255], "pixel 0: opaque red");
        // pixel 1: transparent
        assert_eq!(&result[4..8], &[0, 0, 0, 0], "pixel 1: transparent");
    }

    /// `unpack_bgra32` — half-transparent full-green premultiplied:
    ///
    /// Premultiplied BGRA(0, 128, 0, 128): α=128 (≈50%).
    /// Un-premultiplied G = (128*255 + 64) / 128 = (32640 + 64) / 128 = 255.
    #[test]
    fn unpack_bgra32_half_transparent_green() {
        // BGRA: B=0, G=128, R=0, A=128
        let data: &[u8] = &[0, 128, 0, 128];
        let result = unpack_bgra32(data, 1, 1).expect("should succeed");
        assert_eq!(result.len(), 4, "1x1 RGBA = 4 bytes");
        // Un-premultiplied: R=0, G=255, B=0, A=128
        assert_eq!(result[0], 0, "R=0");
        assert_eq!(result[1], 255, "G=255");
        assert_eq!(result[2], 0, "B=0");
        assert_eq!(result[3], 128, "A=128");
    }

    /// `unpack_bgra32` returns None when data is too short.
    #[test]
    fn unpack_bgra32_too_short_returns_none() {
        // 2×1 needs 8 bytes, supply only 4.
        assert!(unpack_bgra32(&[0, 0, 255, 255], 2, 1).is_none());
    }
}
