//! Self-contained 8-bit PNG encoder built on the COOLJAPAN `oxiarc` stack.
//!
//! OxiText writes PNG files in a few places — SDF/MSDF atlas dumps
//! ([`oxitext-sdf`](https://docs.rs/oxitext-sdf)) and the facade's
//! `RenderResult::to_png`. The obvious dependency for that job, the `png`
//! crate, transitively pulls `flate2` and `miniz_oxide`, both of which this
//! repository's `deny.toml` bans in favour of the pure-Rust `oxiarc-*` crates.
//! This module therefore implements the (small) encoder side of the PNG
//! specification directly on top of [`oxiarc_deflate::zlib_compress`] and
//! [`oxiarc_core::Crc32`].
//!
//! Scope: the exact feature set OxiText needs — non-interlaced, 8 bits per
//! channel, greyscale / greyscale+alpha / RGB / RGBA. Scanlines are filtered
//! adaptively with the standard minimum-sum-of-absolute-differences heuristic
//! from the PNG specification, so output size is comparable to any general
//! purpose encoder at the same deflate level.
//!
//! ```
//! use oxitext_core::png_encode::{encode_png, PngColorType};
//!
//! // A 2×2 greyscale checkerboard.
//! let pixels = [0u8, 255, 255, 0];
//! let png = encode_png(2, 2, PngColorType::Grayscale8, &pixels)?;
//! assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
//! # Ok::<(), oxitext_core::png_encode::PngEncodeError>(())
//! ```

use oxiarc_core::Crc32;
use oxiarc_deflate::zlib_compress;

/// PNG file signature (`\x89PNG\r\n\x1a\n`).
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Deflate level used for the `IDAT` zlib stream.
///
/// Level 6 is the customary "balanced" default shared by zlib, `flate2` and the
/// `png` crate, so replacing `png` with this module does not change file sizes
/// in any surprising direction.
const DEFLATE_LEVEL: u8 = 6;

/// Maximum payload size of a single `IDAT` chunk.
///
/// A PNG chunk length field is a 31-bit value, and the specification explicitly
/// allows the image data to be split across consecutive `IDAT` chunks. Emitting
/// bounded chunks keeps the length field in range for arbitrarily large images.
const IDAT_CHUNK_LIMIT: usize = 1 << 20;

/// Largest dimension permitted by the PNG specification (2³¹ − 1).
const MAX_DIMENSION: u32 = 0x7fff_ffff;

/// The 8-bit-per-channel colour formats this encoder supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PngColorType {
    /// One byte per pixel: luminance.
    Grayscale8,
    /// Two bytes per pixel: luminance, alpha.
    GrayscaleAlpha8,
    /// Three bytes per pixel: red, green, blue.
    Rgb8,
    /// Four bytes per pixel: red, green, blue, alpha.
    Rgba8,
}

impl PngColorType {
    /// Number of channels (and, at 8 bits per channel, bytes) per pixel.
    #[must_use]
    pub const fn channels(self) -> usize {
        match self {
            Self::Grayscale8 => 1,
            Self::GrayscaleAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }

    /// The `IHDR` colour-type code defined by the PNG specification.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Grayscale8 => 0,
            Self::Rgb8 => 2,
            Self::GrayscaleAlpha8 => 4,
            Self::Rgba8 => 6,
        }
    }
}

/// Errors returned by [`encode_png`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PngEncodeError {
    /// `width` or `height` was zero, or exceeded the PNG limit of 2³¹ − 1.
    InvalidDimensions {
        /// The requested width in pixels.
        width: u32,
        /// The requested height in pixels.
        height: u32,
    },
    /// The pixel buffer length did not match `width × height × channels`.
    BufferSize {
        /// Number of bytes the requested image geometry needs.
        expected: usize,
        /// Number of bytes actually supplied.
        actual: usize,
    },
    /// The image is too large to address on this platform.
    ImageTooLarge {
        /// The requested width in pixels.
        width: u32,
        /// The requested height in pixels.
        height: u32,
    },
    /// The `oxiarc-deflate` zlib compressor failed.
    Compression(String),
}

impl core::fmt::Display for PngEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => write!(
                f,
                "invalid PNG dimensions {width}×{height}: each side must be 1..={MAX_DIMENSION}"
            ),
            Self::BufferSize { expected, actual } => write!(
                f,
                "PNG pixel buffer has {actual} bytes, expected exactly {expected}"
            ),
            Self::ImageTooLarge { width, height } => {
                write!(f, "PNG image {width}×{height} does not fit in memory")
            }
            Self::Compression(msg) => write!(f, "PNG deflate stage failed: {msg}"),
        }
    }
}

impl std::error::Error for PngEncodeError {}

/// Encode an 8-bit-per-channel, non-interlaced PNG image.
///
/// `pixels` must hold exactly `width × height × color.channels()` bytes in
/// top-to-bottom, left-to-right order.
///
/// # Errors
///
/// - [`PngEncodeError::InvalidDimensions`] if either side is `0` or larger than
///   2³¹ − 1.
/// - [`PngEncodeError::ImageTooLarge`] if the raw image does not fit in a
///   `usize` on this platform.
/// - [`PngEncodeError::BufferSize`] if `pixels` has the wrong length.
/// - [`PngEncodeError::Compression`] if the zlib stage fails.
pub fn encode_png(
    width: u32,
    height: u32,
    color: PngColorType,
    pixels: &[u8],
) -> Result<Vec<u8>, PngEncodeError> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngEncodeError::InvalidDimensions { width, height });
    }

    let channels = color.channels();
    let row_len = (width as usize)
        .checked_mul(channels)
        .ok_or(PngEncodeError::ImageTooLarge { width, height })?;
    let expected = row_len
        .checked_mul(height as usize)
        .ok_or(PngEncodeError::ImageTooLarge { width, height })?;
    if pixels.len() != expected {
        return Err(PngEncodeError::BufferSize {
            expected,
            actual: pixels.len(),
        });
    }

    // Each scanline gains a one-byte filter tag in the raw PNG data stream.
    let raw_stream_len = row_len
        .checked_add(1)
        .and_then(|stride| stride.checked_mul(height as usize))
        .ok_or(PngEncodeError::ImageTooLarge { width, height })?;

    let filtered = filter_scanlines(pixels, row_len, height as usize, channels, raw_stream_len);
    let compressed = zlib_compress(&filtered, DEFLATE_LEVEL)
        .map_err(|e| PngEncodeError::Compression(e.to_string()))?;

    let mut out = Vec::with_capacity(PNG_SIGNATURE.len() + 25 + compressed.len() + 12);
    out.extend_from_slice(&PNG_SIGNATURE);

    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = color.code();
    ihdr[10] = 0; // compression method: deflate
    ihdr[11] = 0; // filter method: adaptive
    ihdr[12] = 0; // interlace method: none
    write_chunk(&mut out, b"IHDR", &ihdr);

    for part in compressed.chunks(IDAT_CHUNK_LIMIT) {
        write_chunk(&mut out, b"IDAT", part);
    }
    write_chunk(&mut out, b"IEND", &[]);

    Ok(out)
}

/// Append a length-prefixed, CRC-suffixed PNG chunk to `out`.
///
/// `data.len()` is always `<= IDAT_CHUNK_LIMIT` (or a fixed small header) at
/// every call site, so the length field can never overflow.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    debug_assert!(
        data.len() <= IDAT_CHUNK_LIMIT,
        "PNG chunk payload must stay within the 31-bit length field"
    );
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.value().to_be_bytes());
}

/// Build the raw PNG data stream: every scanline prefixed by its filter byte.
///
/// Each row is trial-filtered with all five filter types and the one with the
/// smallest sum of absolute (signed) byte values wins — the heuristic
/// recommended by the PNG specification and used by libpng.
fn filter_scanlines(
    pixels: &[u8],
    row_len: usize,
    height: usize,
    bpp: usize,
    raw_stream_len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw_stream_len);

    // Five reusable candidate buffers, one per filter type.
    let mut candidates: [Vec<u8>; 5] = [
        vec![0u8; row_len],
        vec![0u8; row_len],
        vec![0u8; row_len],
        vec![0u8; row_len],
        vec![0u8; row_len],
    ];
    let zero_row = vec![0u8; row_len];

    for y in 0..height {
        let start = y * row_len;
        let current = &pixels[start..start + row_len];
        let previous = if y == 0 {
            &zero_row[..]
        } else {
            &pixels[start - row_len..start]
        };

        for x in 0..row_len {
            let raw = current[x];
            let left = if x >= bpp { current[x - bpp] } else { 0 };
            let up = previous[x];
            let up_left = if x >= bpp { previous[x - bpp] } else { 0 };

            candidates[0][x] = raw;
            candidates[1][x] = raw.wrapping_sub(left);
            candidates[2][x] = raw.wrapping_sub(up);
            candidates[3][x] = raw.wrapping_sub(average(left, up));
            candidates[4][x] = raw.wrapping_sub(paeth(left, up, up_left));
        }

        let mut best = 0usize;
        let mut best_score = u64::MAX;
        for (index, candidate) in candidates.iter().enumerate() {
            let score = filter_score(candidate);
            if score < best_score {
                best_score = score;
                best = index;
            }
        }

        // `best` indexes `candidates`, so the cast is exact (0..=4).
        out.push(best as u8);
        out.extend_from_slice(&candidates[best]);
    }

    out
}

/// `floor((a + b) / 2)` computed without overflow, per the `Average` filter.
#[inline]
fn average(a: u8, b: u8) -> u8 {
    // u16 arithmetic keeps the sum exact; the result is always < 256.
    (((a as u16) + (b as u16)) / 2) as u8
}

/// The PNG `Paeth` predictor (specification 9.4).
#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Sum of absolute values of the filtered bytes read as signed integers.
#[inline]
fn filter_score(row: &[u8]) -> u64 {
    row.iter()
        .map(|&b| u64::from((b as i8).unsigned_abs()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxiarc_deflate::zlib_decompress;

    /// Minimal PNG reader used to prove the encoder round-trips.
    struct DecodedPng {
        width: u32,
        height: u32,
        color_code: u8,
        bit_depth: u8,
        pixels: Vec<u8>,
    }

    /// Walk the chunk stream, verify every CRC, inflate `IDAT` and unfilter.
    fn decode(png: &[u8]) -> DecodedPng {
        assert_eq!(&png[..8], &PNG_SIGNATURE, "signature");
        let mut offset = 8usize;
        let mut header: Option<(u32, u32, u8, u8)> = None;
        let mut idat = Vec::new();
        let mut saw_end = false;

        while offset < png.len() {
            let len = u32::from_be_bytes([
                png[offset],
                png[offset + 1],
                png[offset + 2],
                png[offset + 3],
            ]) as usize;
            let kind = &png[offset + 4..offset + 8];
            let data = &png[offset + 8..offset + 8 + len];
            let stored = u32::from_be_bytes([
                png[offset + 8 + len],
                png[offset + 9 + len],
                png[offset + 10 + len],
                png[offset + 11 + len],
            ]);
            let mut crc = Crc32::new();
            crc.update(kind);
            crc.update(data);
            assert_eq!(crc.value(), stored, "CRC mismatch in chunk at {offset}");

            match kind {
                b"IHDR" => {
                    assert_eq!(len, 13);
                    let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                    assert_eq!(data[10], 0, "compression method");
                    assert_eq!(data[11], 0, "filter method");
                    assert_eq!(data[12], 0, "interlace method");
                    header = Some((width, height, data[9], data[8]));
                }
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => {
                    assert_eq!(len, 0);
                    saw_end = true;
                }
                other => panic!("unexpected chunk {:?}", core::str::from_utf8(other)),
            }
            offset += 12 + len;
        }
        assert!(saw_end, "missing IEND");
        assert_eq!(offset, png.len(), "trailing bytes");

        let (width, height, color_code, bit_depth) = header.expect("IHDR present");
        let channels = match color_code {
            0 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            other => panic!("unsupported colour code {other}"),
        };
        let raw = zlib_decompress(&idat).expect("inflate IDAT");
        let row_len = width as usize * channels;
        assert_eq!(raw.len(), (row_len + 1) * height as usize);

        let mut pixels = vec![0u8; row_len * height as usize];
        for y in 0..height as usize {
            let filter = raw[y * (row_len + 1)];
            let src = &raw[y * (row_len + 1) + 1..(y + 1) * (row_len + 1)];
            for x in 0..row_len {
                let left = if x >= channels {
                    pixels[y * row_len + x - channels]
                } else {
                    0
                };
                let up = if y > 0 {
                    pixels[(y - 1) * row_len + x]
                } else {
                    0
                };
                let up_left = if y > 0 && x >= channels {
                    pixels[(y - 1) * row_len + x - channels]
                } else {
                    0
                };
                let value = match filter {
                    0 => src[x],
                    1 => src[x].wrapping_add(left),
                    2 => src[x].wrapping_add(up),
                    3 => src[x].wrapping_add(average(left, up)),
                    4 => src[x].wrapping_add(paeth(left, up, up_left)),
                    other => panic!("unsupported filter {other}"),
                };
                pixels[y * row_len + x] = value;
            }
        }

        DecodedPng {
            width,
            height,
            color_code,
            bit_depth,
            pixels,
        }
    }

    fn round_trip(width: u32, height: u32, color: PngColorType, pixels: &[u8]) {
        let png = encode_png(width, height, color, pixels).expect("encode");
        let decoded = decode(&png);
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
        assert_eq!(decoded.bit_depth, 8);
        assert_eq!(decoded.color_code, color.code());
        assert_eq!(decoded.pixels, pixels);
    }

    #[test]
    fn grayscale_round_trips() {
        let pixels: Vec<u8> = (0..64u16).map(|v| (v * 4) as u8).collect();
        round_trip(8, 8, PngColorType::Grayscale8, &pixels);
    }

    #[test]
    fn grayscale_alpha_round_trips() {
        let pixels: Vec<u8> = (0..32u16).flat_map(|v| [(v * 8) as u8, 255]).collect();
        round_trip(8, 4, PngColorType::GrayscaleAlpha8, &pixels);
    }

    #[test]
    fn rgb_round_trips() {
        let mut pixels = Vec::new();
        for y in 0..6u8 {
            for x in 0..5u8 {
                pixels.extend_from_slice(&[x * 40, y * 30, x.wrapping_mul(y)]);
            }
        }
        round_trip(5, 6, PngColorType::Rgb8, &pixels);
    }

    #[test]
    fn rgba_round_trips() {
        let mut pixels = Vec::new();
        for y in 0..7u8 {
            for x in 0..9u8 {
                pixels.extend_from_slice(&[x * 20, y * 30, 255 - x * 20, x.wrapping_add(y) * 3]);
            }
        }
        round_trip(9, 7, PngColorType::Rgba8, &pixels);
    }

    #[test]
    fn single_pixel_round_trips() {
        round_trip(1, 1, PngColorType::Rgba8, &[1, 2, 3, 4]);
    }

    #[test]
    fn flat_image_round_trips() {
        // A constant image exercises the Up/Sub filters and compresses hard.
        let pixels = vec![128u8; 32 * 32];
        round_trip(32, 32, PngColorType::Grayscale8, &pixels);
        let png = encode_png(32, 32, PngColorType::Grayscale8, &pixels).expect("encode");
        assert!(
            png.len() < pixels.len(),
            "flat image should compress: {} vs {}",
            png.len(),
            pixels.len()
        );
    }

    #[test]
    fn wide_row_round_trips() {
        // Wider than one shelf of the deflate window, so the row loop and the
        // filter heuristic both see non-trivial input.
        let pixels: Vec<u8> = (0..(600 * 3usize)).map(|i| ((i * 7) % 251) as u8).collect();
        round_trip(600, 1, PngColorType::Rgb8, &pixels);
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert_eq!(
            encode_png(0, 4, PngColorType::Grayscale8, &[]),
            Err(PngEncodeError::InvalidDimensions {
                width: 0,
                height: 4
            })
        );
        assert_eq!(
            encode_png(4, 0, PngColorType::Grayscale8, &[]),
            Err(PngEncodeError::InvalidDimensions {
                width: 4,
                height: 0
            })
        );
    }

    #[test]
    fn rejects_buffer_size_mismatch() {
        let err = encode_png(4, 4, PngColorType::Rgb8, &[0u8; 47]).expect_err("must fail");
        assert_eq!(
            err,
            PngEncodeError::BufferSize {
                expected: 48,
                actual: 47
            }
        );
        assert!(err.to_string().contains("expected exactly 48"));
    }

    #[test]
    fn channel_counts_match_color_codes() {
        assert_eq!(PngColorType::Grayscale8.channels(), 1);
        assert_eq!(PngColorType::GrayscaleAlpha8.channels(), 2);
        assert_eq!(PngColorType::Rgb8.channels(), 3);
        assert_eq!(PngColorType::Rgba8.channels(), 4);
        assert_eq!(PngColorType::Grayscale8.code(), 0);
        assert_eq!(PngColorType::Rgb8.code(), 2);
        assert_eq!(PngColorType::GrayscaleAlpha8.code(), 4);
        assert_eq!(PngColorType::Rgba8.code(), 6);
    }

    #[test]
    fn paeth_matches_specification() {
        // Reference cases from the PNG specification's worked examples.
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(10, 20, 30), 10);
        assert_eq!(paeth(20, 10, 30), 10);
        assert_eq!(paeth(100, 100, 50), 100);
        assert_eq!(average(255, 255), 255);
        assert_eq!(average(1, 2), 1);
    }
}
