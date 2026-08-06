//! Self-contained PNG decoder built on the COOLJAPAN `oxiarc` stack.
//!
//! OxiText reads PNG data in exactly one place: the PNG-compressed strikes of
//! the OpenType `CBDT`/`CBLC` and Apple `sbix` colour-bitmap tables (see
//! `oxitext-raster`'s `detect` module). The obvious dependency for that job,
//! the `png` crate, transitively pulls `flate2` and `miniz_oxide`, both of
//! which this repository's `deny.toml` bans in favour of the pure-Rust
//! `oxiarc-*` crates. This module is the mirror image of [`crate::png_encode`]:
//! it implements the decoder side of the PNG specification directly on top of
//! [`oxiarc_deflate::zlib_decompress`] and [`oxiarc_core::Crc32`].
//!
//! ## Scope
//!
//! The full still-image PNG feature set, normalised to straight-alpha RGBA8:
//!
//! | IHDR colour type      | code | bit depths      | notes                       |
//! |-----------------------|------|-----------------|-----------------------------|
//! | Greyscale             |    0 | 1, 2, 4, 8, 16  | `tRNS` grey key honoured    |
//! | Truecolour            |    2 | 8, 16           | `tRNS` RGB key honoured     |
//! | Indexed               |    3 | 1, 2, 4, 8      | `PLTE` + per-entry `tRNS`   |
//! | Greyscale + alpha     |    4 | 8, 16           |                             |
//! | Truecolour + alpha    |    6 | 8, 16           |                             |
//!
//! Both interlace methods are supported: `0` (none) and `1` (Adam7). All five
//! scanline filters (None/Sub/Up/Average/Paeth) are implemented. 16-bit samples
//! are scaled down to 8 bits (`v >> 8`) *after* `tRNS` comparison, which the
//! specification requires to happen at native sample depth.
//!
//! Ancillary chunks other than `tRNS` (`gAMA`, `iCCP`, `pHYs`, `tEXt`, …) are
//! skipped after CRC validation. APNG (`acTL`/`fcTL`/`fdAT`) is not
//! interpreted: the still image in `IDAT` is returned, which is exactly the
//! APNG-unaware behaviour the specification prescribes.
//!
//! ```
//! use oxitext_core::png_decode::decode_png_rgba8;
//!
//! // A 2×1 RGBA8 PNG (opaque red, half-transparent green) written by an
//! // unrelated encoder, byte for byte.
//! let png: [u8; 72] = [
//!     0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
//!     0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01,
//!     0x08, 0x06, 0x00, 0x00, 0x00, 0xf4, 0x22, 0x7f, 0x8a, 0x00, 0x00, 0x00,
//!     0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
//!     0x1f, 0x08, 0x1b, 0x00, 0x10, 0x79, 0x03, 0x7e, 0xc4, 0x18, 0x72, 0x90,
//!     0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
//! ];
//!
//! let image = decode_png_rgba8(&png)?;
//! assert_eq!((image.width, image.height), (2, 1));
//! assert_eq!(image.rgba, vec![255, 0, 0, 255, 0, 255, 0, 128]);
//! # Ok::<(), oxitext_core::png_decode::PngDecodeError>(())
//! ```

use oxiarc_core::Crc32;
use oxiarc_deflate::zlib_decompress_into;

/// PNG file signature (`\x89PNG\r\n\x1a\n`).
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Largest expansion a deflate stream can achieve, rounded up.
///
/// A deflate block encodes a match of up to 258 bytes in as little as 2 bits,
/// which bounds the ratio at 1032:1. Requiring the declared image data to be
/// reachable at that ratio lets a bomb be rejected before a single byte is
/// allocated, without rejecting any legitimately well-compressed image.
const MAX_DEFLATE_RATIO: usize = 1032;

/// Largest dimension permitted by the PNG specification (2³¹ − 1).
const MAX_DIMENSION: u32 = 0x7fff_ffff;

/// Adam7 first row of each of the seven interlace passes.
const ADAM7_ROW_START: [u32; 7] = [0, 0, 4, 0, 2, 0, 1];
/// Adam7 first column of each of the seven interlace passes.
const ADAM7_COL_START: [u32; 7] = [0, 4, 0, 2, 0, 1, 0];
/// Adam7 row stride of each of the seven interlace passes.
const ADAM7_ROW_STEP: [u32; 7] = [8, 8, 8, 4, 4, 2, 2];
/// Adam7 column stride of each of the seven interlace passes.
const ADAM7_COL_STEP: [u32; 7] = [8, 8, 4, 4, 2, 2, 1];

/// A decoded image in straight-alpha, 8-bit RGBA order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// `width × height × 4` bytes, row-major, top-to-bottom, `[R, G, B, A]`.
    pub rgba: Vec<u8>,
}

/// Errors returned by [`decode_png_rgba8`].
///
/// Every variant names a specific, recoverable rejection reason: no input is
/// ever silently decoded into a wrong-looking image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PngDecodeError {
    /// The data does not start with the 8-byte PNG signature.
    NotAPng,
    /// The byte stream ended in the middle of a chunk header, payload or CRC.
    Truncated {
        /// Byte offset at which more data was required.
        offset: usize,
        /// Number of additional bytes the parser needed.
        needed: usize,
    },
    /// The first chunk was not `IHDR`, or `IHDR` was not 13 bytes long.
    InvalidHeader,
    /// The file is Apple's proprietary `CgBI` variant, not a standard PNG.
    ///
    /// Xcode rewrites app-bundle PNGs into this format: a `CgBI` chunk before
    /// `IHDR`, premultiplied BGRA channel order, and raw deflate with no zlib
    /// wrapper. It is not the format the PNG specification describes and it
    /// never appears in a font's colour-bitmap table, so it is reported rather
    /// than guessed at.
    AppleCgBi,
    /// A chunk's stored CRC-32 did not match its recomputed value.
    ChecksumMismatch {
        /// The four-byte chunk type, as ASCII.
        chunk: [u8; 4],
        /// CRC-32 stored in the file.
        stored: u32,
        /// CRC-32 computed over the chunk type and payload.
        computed: u32,
    },
    /// `IHDR` declared a zero side or one larger than 2³¹ − 1.
    InvalidDimensions {
        /// Declared width in pixels.
        width: u32,
        /// Declared height in pixels.
        height: u32,
    },
    /// `IHDR` declared a colour type outside `{0, 2, 3, 4, 6}`.
    UnsupportedColorType(u8),
    /// The bit depth is not one the declared colour type allows.
    UnsupportedBitDepth {
        /// The declared colour type.
        color_type: u8,
        /// The declared bit depth.
        bit_depth: u8,
    },
    /// `IHDR` declared a compression method other than `0` (deflate).
    UnsupportedCompressionMethod(u8),
    /// `IHDR` declared a filter method other than `0` (adaptive).
    UnsupportedFilterMethod(u8),
    /// `IHDR` declared an interlace method other than `0` (none) or `1` (Adam7).
    UnsupportedInterlaceMethod(u8),
    /// An indexed-colour image had no `PLTE` chunk.
    MissingPalette,
    /// A `PLTE` chunk's length was not a positive multiple of three, or it
    /// declared more than 256 entries.
    InvalidPalette {
        /// The chunk's payload length in bytes.
        length: usize,
    },
    /// A pixel referenced a palette index beyond the end of `PLTE`.
    PaletteIndexOutOfRange {
        /// The out-of-range index.
        index: usize,
        /// Number of entries the palette actually has.
        palette_len: usize,
    },
    /// The image contained no `IDAT` chunk.
    MissingImageData,
    /// The `oxiarc-deflate` zlib decompressor rejected the `IDAT` stream.
    Decompression(String),
    /// The inflated stream was shorter than the declared geometry requires.
    DataSize {
        /// Number of bytes the geometry needs.
        expected: usize,
        /// Number of bytes actually inflated.
        actual: usize,
    },
    /// The image data stream carries more bytes than `IHDR` accounts for.
    ExcessImageData {
        /// Number of bytes the declared geometry needs.
        expected: usize,
    },
    /// `IHDR` declares more image data than the `IDAT` payload could possibly
    /// inflate to — a decompression bomb or a corrupt header.
    CompressionRatioExceeded {
        /// Number of bytes the declared geometry needs.
        expected: usize,
        /// Length of the compressed `IDAT` payload.
        compressed: usize,
    },
    /// A scanline declared a filter type outside `0..=4`.
    InvalidFilterType(u8),
    /// The declared geometry does not fit in a `usize` on this platform.
    ImageTooLarge {
        /// Declared width in pixels.
        width: u32,
        /// Declared height in pixels.
        height: u32,
    },
}

impl core::fmt::Display for PngDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAPng => write!(f, "data does not start with the PNG signature"),
            Self::Truncated { offset, needed } => write!(
                f,
                "PNG data ends prematurely at byte {offset}: {needed} more byte(s) required"
            ),
            Self::InvalidHeader => write!(f, "PNG IHDR chunk is missing or malformed"),
            Self::AppleCgBi => write!(
                f,
                "file is Apple's proprietary CgBI PNG variant (premultiplied BGRA, \
                 zlib-wrapper-less deflate), not a standard PNG"
            ),
            Self::ChecksumMismatch {
                chunk,
                stored,
                computed,
            } => {
                let name = String::from_utf8_lossy(chunk).into_owned();
                write!(
                    f,
                    "PNG chunk {name} CRC mismatch: stored {stored:#010x}, computed {computed:#010x}"
                )
            }
            Self::InvalidDimensions { width, height } => write!(
                f,
                "invalid PNG dimensions {width}×{height}: each side must be 1..={MAX_DIMENSION}"
            ),
            Self::UnsupportedColorType(ct) => {
                write!(
                    f,
                    "unsupported PNG colour type {ct} (expected 0, 2, 3, 4 or 6)"
                )
            }
            Self::UnsupportedBitDepth {
                color_type,
                bit_depth,
            } => write!(
                f,
                "PNG bit depth {bit_depth} is not allowed for colour type {color_type}"
            ),
            Self::UnsupportedCompressionMethod(m) => {
                write!(f, "unsupported PNG compression method {m} (expected 0)")
            }
            Self::UnsupportedFilterMethod(m) => {
                write!(f, "unsupported PNG filter method {m} (expected 0)")
            }
            Self::UnsupportedInterlaceMethod(m) => {
                write!(f, "unsupported PNG interlace method {m} (expected 0 or 1)")
            }
            Self::MissingPalette => write!(f, "indexed-colour PNG has no PLTE chunk"),
            Self::InvalidPalette { length } => write!(
                f,
                "PNG PLTE chunk length {length} is not a positive multiple of 3 (max 768)"
            ),
            Self::PaletteIndexOutOfRange { index, palette_len } => write!(
                f,
                "PNG palette index {index} is out of range for a {palette_len}-entry PLTE"
            ),
            Self::MissingImageData => write!(f, "PNG has no IDAT chunk"),
            Self::Decompression(msg) => write!(f, "PNG inflate stage failed: {msg}"),
            Self::DataSize { expected, actual } => write!(
                f,
                "PNG image data has {actual} bytes after inflate, expected at least {expected}"
            ),
            Self::ExcessImageData { expected } => write!(
                f,
                "PNG image data inflates past the {expected} bytes its IHDR accounts for"
            ),
            Self::CompressionRatioExceeded {
                expected,
                compressed,
            } => write!(
                f,
                "PNG IHDR needs {expected} bytes of image data, unreachable from a \
                 {compressed}-byte IDAT payload (deflate expands at most {MAX_DEFLATE_RATIO}:1)"
            ),
            Self::InvalidFilterType(t) => {
                write!(
                    f,
                    "PNG scanline declares unknown filter type {t} (expected 0..=4)"
                )
            }
            Self::ImageTooLarge { width, height } => {
                write!(f, "PNG image {width}×{height} does not fit in memory")
            }
        }
    }
}

impl std::error::Error for PngDecodeError {}

/// Parsed `IHDR` fields, validated for internal consistency.
#[derive(Debug, Clone, Copy)]
struct Header {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
}

impl Header {
    /// Number of samples per pixel for this colour type.
    const fn channels(self) -> usize {
        match self.color_type {
            0 | 3 => 1,
            4 => 2,
            2 => 3,
            _ => 4,
        }
    }

    /// Filter unit distance in bytes: `ceil(channels × bit_depth / 8)`, at
    /// least one, exactly as the PNG specification defines `bpp`.
    const fn filter_unit(self) -> usize {
        let bits = self.channels() * self.bit_depth as usize;
        let bytes = bits / 8;
        if bytes == 0 {
            1
        } else {
            bytes
        }
    }

    /// Byte length of one scanline `pixels_wide` pixels across (filter byte
    /// excluded). Returns `None` on `usize` overflow.
    fn row_bytes(self, pixels_wide: u32) -> Option<usize> {
        let bits = (pixels_wide as usize)
            .checked_mul(self.channels())?
            .checked_mul(self.bit_depth as usize)?;
        Some(bits.div_ceil(8))
    }
}

/// Transparency information from a `tRNS` chunk, interpreted per colour type.
#[derive(Debug, Clone, Default)]
struct Transparency {
    /// Colour type 0: the fully transparent grey sample, at native depth.
    grey_key: Option<u16>,
    /// Colour type 2: the fully transparent `(r, g, b)` triple, at native depth.
    rgb_key: Option<(u16, u16, u16)>,
    /// Colour type 3: per-palette-entry alpha; entries beyond the end are 255.
    palette_alpha: Vec<u8>,
}

/// Decode a PNG image into straight-alpha RGBA8.
///
/// Returns the image geometry declared by `IHDR` together with a
/// `width × height × 4` byte buffer.
///
/// # Memory safety against hostile input
///
/// Nothing is sized from a declared field alone. The image-data stream is
/// inflated into a buffer of exactly the length `IHDR` implies, and that length
/// is first rejected outright unless the compressed `IDAT` payload is long
/// enough to produce it at deflate's maximum 1032:1 expansion
/// ([`PngDecodeError::CompressionRatioExceeded`]). A tiny file therefore cannot
/// force a large allocation either by declaring a huge `IHDR` or by shipping a
/// decompression bomb, and a stream that inflates past the declared geometry is
/// reported as [`PngDecodeError::ExcessImageData`] rather than silently
/// truncated.
///
/// # Errors
///
/// Returns a [`PngDecodeError`] describing the first problem encountered: a
/// missing signature, a truncated or CRC-invalid chunk, an unsupported or
/// self-inconsistent `IHDR`, a missing `PLTE`/`IDAT`, a failed inflate, an
/// image-data stream that is shorter or longer than the declared geometry, or
/// an unknown scanline filter type. This function never returns a partially
/// decoded image.
pub fn decode_png_rgba8(data: &[u8]) -> Result<PngImage, PngDecodeError> {
    if data.len() < PNG_SIGNATURE.len() || data[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(PngDecodeError::NotAPng);
    }

    let mut cursor = PNG_SIGNATURE.len();
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns_raw: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_idat = false;

    while cursor < data.len() {
        let (kind, payload, next) = read_chunk(data, cursor)?;
        cursor = next;

        match &kind {
            b"IHDR" => {
                if header.is_some() {
                    return Err(PngDecodeError::InvalidHeader);
                }
                header = Some(parse_header(payload)?);
            }
            b"PLTE" => {
                if payload.is_empty() || payload.len() % 3 != 0 || payload.len() > 768 {
                    return Err(PngDecodeError::InvalidPalette {
                        length: payload.len(),
                    });
                }
                palette = payload
                    .chunks_exact(3)
                    .map(|c| [c[0], c[1], c[2]])
                    .collect();
            }
            b"tRNS" => trns_raw = Some(payload.to_vec()),
            b"IDAT" => {
                saw_idat = true;
                idat.extend_from_slice(payload);
            }
            b"IEND" => break,
            // Apple's Xcode-rewritten variant. It precedes `IHDR`, so it is
            // seen before the "first chunk must be IHDR" check below would
            // report the far less helpful `InvalidHeader`.
            b"CgBI" => return Err(PngDecodeError::AppleCgBi),
            // Any other chunk (ancillary metadata, APNG control chunks) is
            // CRC-checked by `read_chunk` and then ignored.
            _ => {}
        }

        if header.is_none() {
            return Err(PngDecodeError::InvalidHeader);
        }
    }

    let header = header.ok_or(PngDecodeError::InvalidHeader)?;
    if !saw_idat {
        return Err(PngDecodeError::MissingImageData);
    }
    if header.color_type == 3 && palette.is_empty() {
        return Err(PngDecodeError::MissingPalette);
    }

    let transparency = parse_transparency(trns_raw.as_deref(), header, palette.len());
    let raw = inflate_image_data(&idat, header)?;

    decode_pixels(&raw, header, &palette, &transparency)
}

/// Inflate the concatenated `IDAT` payload into a buffer sized by `IHDR`.
///
/// Two guards, in this order, keep a hostile file from turning into a large
/// allocation:
///
/// 1. The declared geometry is rejected unless `compressed` is long enough to
///    produce it at deflate's maximum expansion ratio, so a few hundred bytes
///    with a `65535×65535` `IHDR` never allocate anything at all.
/// 2. The output buffer is then exactly the declared length, and the inflater
///    writes straight into it, so a decompression bomb hits the buffer limit
///    instead of the allocator.
fn inflate_image_data(compressed: &[u8], header: Header) -> Result<Vec<u8>, PngDecodeError> {
    let required = required_raw_len(header).ok_or(PngDecodeError::ImageTooLarge {
        width: header.width,
        height: header.height,
    })?;

    let ceiling = compressed.len().saturating_mul(MAX_DEFLATE_RATIO);
    if required > ceiling {
        return Err(PngDecodeError::CompressionRatioExceeded {
            expected: required,
            compressed: compressed.len(),
        });
    }

    let mut raw = vec![0u8; required];
    let written = zlib_decompress_into(compressed, &mut raw).map_err(|e| {
        // `BufferTooSmall` is the inflater refusing to write past the buffer:
        // the stream carries more image data than `IHDR` accounts for, which
        // is a malformed file rather than a decompression failure.
        if matches!(e, oxiarc_core::OxiArcError::BufferTooSmall { .. }) {
            PngDecodeError::ExcessImageData { expected: required }
        } else {
            PngDecodeError::Decompression(e.to_string())
        }
    })?;

    if written < required {
        return Err(PngDecodeError::DataSize {
            expected: required,
            actual: written,
        });
    }

    Ok(raw)
}

/// Read one length-prefixed, CRC-suffixed chunk starting at `offset`.
///
/// Returns the chunk type, its payload and the offset of the next chunk.
fn read_chunk(data: &[u8], offset: usize) -> Result<([u8; 4], &[u8], usize), PngDecodeError> {
    let need_header = 8usize;
    if data.len() - offset < need_header {
        return Err(PngDecodeError::Truncated {
            offset,
            needed: need_header - (data.len() - offset),
        });
    }
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&data[offset..offset + 4]);
    let length = u32::from_be_bytes(len_bytes) as usize;
    let mut kind = [0u8; 4];
    kind.copy_from_slice(&data[offset + 4..offset + 8]);

    let payload_start = offset + 8;
    let total = length
        .checked_add(4)
        .and_then(|t| payload_start.checked_add(t))
        .ok_or(PngDecodeError::Truncated {
            offset,
            needed: length,
        })?;
    if total > data.len() {
        return Err(PngDecodeError::Truncated {
            offset,
            needed: total - data.len(),
        });
    }

    let payload = &data[payload_start..payload_start + length];
    let mut stored_bytes = [0u8; 4];
    stored_bytes.copy_from_slice(&data[payload_start + length..total]);
    let stored = u32::from_be_bytes(stored_bytes);

    let mut crc = Crc32::new();
    crc.update(&kind);
    crc.update(payload);
    let computed = crc.value();
    if computed != stored {
        return Err(PngDecodeError::ChecksumMismatch {
            chunk: kind,
            stored,
            computed,
        });
    }

    Ok((kind, payload, total))
}

/// Parse and validate a 13-byte `IHDR` payload.
fn parse_header(payload: &[u8]) -> Result<Header, PngDecodeError> {
    if payload.len() != 13 {
        return Err(PngDecodeError::InvalidHeader);
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&payload[0..4]);
    let width = u32::from_be_bytes(buf);
    buf.copy_from_slice(&payload[4..8]);
    let height = u32::from_be_bytes(buf);
    let bit_depth = payload[8];
    let color_type = payload[9];
    let compression = payload[10];
    let filter = payload[11];
    let interlace = payload[12];

    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngDecodeError::InvalidDimensions { width, height });
    }
    if compression != 0 {
        return Err(PngDecodeError::UnsupportedCompressionMethod(compression));
    }
    if filter != 0 {
        return Err(PngDecodeError::UnsupportedFilterMethod(filter));
    }
    if interlace > 1 {
        return Err(PngDecodeError::UnsupportedInterlaceMethod(interlace));
    }

    let depth_ok = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(bit_depth, 8 | 16),
        other => return Err(PngDecodeError::UnsupportedColorType(other)),
    };
    if !depth_ok {
        return Err(PngDecodeError::UnsupportedBitDepth {
            color_type,
            bit_depth,
        });
    }

    Ok(Header {
        width,
        height,
        bit_depth,
        color_type,
        interlace,
    })
}

/// Interpret a `tRNS` payload for the image's colour type.
///
/// A `tRNS` chunk that is too short for its colour type is ignored rather than
/// rejected, matching the specification's "decoders may ignore" latitude for
/// ancillary chunks; the image then decodes fully opaque.
fn parse_transparency(payload: Option<&[u8]>, header: Header, palette_len: usize) -> Transparency {
    let mut out = Transparency::default();
    let Some(payload) = payload else {
        return out;
    };
    match header.color_type {
        0 if payload.len() >= 2 => {
            out.grey_key = Some(u16::from_be_bytes([payload[0], payload[1]]));
        }
        2 if payload.len() >= 6 => {
            out.rgb_key = Some((
                u16::from_be_bytes([payload[0], payload[1]]),
                u16::from_be_bytes([payload[2], payload[3]]),
                u16::from_be_bytes([payload[4], payload[5]]),
            ));
        }
        3 => {
            let take = payload.len().min(palette_len);
            out.palette_alpha = payload[..take].to_vec();
        }
        // Colour types 4 and 6 carry their own alpha channel; `tRNS` is
        // forbidden there and is ignored if present.
        _ => {}
    }
    out
}

/// Unfilter the inflated stream and expand it into RGBA8.
fn decode_pixels(
    raw: &[u8],
    header: Header,
    palette: &[[u8; 3]],
    trns: &Transparency,
) -> Result<PngImage, PngDecodeError> {
    let too_large = PngDecodeError::ImageTooLarge {
        width: header.width,
        height: header.height,
    };
    let out_len = (header.width as usize)
        .checked_mul(header.height as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or(too_large.clone())?;

    // Validate the inflated length against the declared geometry *before*
    // allocating the output buffer, so a crafted IHDR cannot force a huge
    // allocation on a tiny input.
    let required = required_raw_len(header).ok_or(too_large)?;
    if raw.len() < required {
        return Err(PngDecodeError::DataSize {
            expected: required,
            actual: raw.len(),
        });
    }

    let mut rgba = vec![0u8; out_len];
    let mut consumed = 0usize;

    if header.interlace == 0 {
        expand_pass(
            raw,
            &mut consumed,
            header,
            palette,
            trns,
            PassGeometry {
                width: header.width,
                height: header.height,
                col_start: 0,
                col_step: 1,
                row_start: 0,
                row_step: 1,
            },
            &mut rgba,
        )?;
    } else {
        for pass in 0..7 {
            let geometry = adam7_geometry(header, pass);
            if geometry.width == 0 || geometry.height == 0 {
                continue;
            }
            expand_pass(
                raw,
                &mut consumed,
                header,
                palette,
                trns,
                geometry,
                &mut rgba,
            )?;
        }
    }

    Ok(PngImage {
        width: header.width,
        height: header.height,
        rgba,
    })
}

/// Placement rule for one (possibly interlaced) pass of scanlines.
#[derive(Debug, Clone, Copy)]
struct PassGeometry {
    /// Number of pixels in each of this pass's scanlines.
    width: u32,
    /// Number of scanlines in this pass.
    height: u32,
    /// Destination column of this pass's first pixel.
    col_start: u32,
    /// Destination column stride between consecutive pass pixels.
    col_step: u32,
    /// Destination row of this pass's first scanline.
    row_start: u32,
    /// Destination row stride between consecutive pass scanlines.
    row_step: u32,
}

/// Adam7 geometry of `pass` (`0..7`) for an image of `header`'s size.
fn adam7_geometry(header: Header, pass: usize) -> PassGeometry {
    let col_start = ADAM7_COL_START[pass];
    let row_start = ADAM7_ROW_START[pass];
    let col_step = ADAM7_COL_STEP[pass];
    let row_step = ADAM7_ROW_STEP[pass];
    let width = header.width.saturating_sub(col_start).div_ceil(col_step);
    let height = header.height.saturating_sub(row_start).div_ceil(row_step);
    PassGeometry {
        width,
        height,
        col_start,
        col_step,
        row_start,
        row_step,
    }
}

/// Total inflated length the declared geometry requires, filter bytes included.
fn required_raw_len(header: Header) -> Option<usize> {
    if header.interlace == 0 {
        header
            .row_bytes(header.width)?
            .checked_add(1)?
            .checked_mul(header.height as usize)
    } else {
        let mut total = 0usize;
        for pass in 0..7 {
            let geometry = adam7_geometry(header, pass);
            if geometry.width == 0 || geometry.height == 0 {
                continue;
            }
            let pass_len = header
                .row_bytes(geometry.width)?
                .checked_add(1)?
                .checked_mul(geometry.height as usize)?;
            total = total.checked_add(pass_len)?;
        }
        Some(total)
    }
}

/// Unfilter one pass's scanlines and scatter its pixels into `rgba`.
///
/// `consumed` tracks the read position inside the inflated stream across
/// passes; it is advanced by exactly this pass's byte count.
fn expand_pass(
    raw: &[u8],
    consumed: &mut usize,
    header: Header,
    palette: &[[u8; 3]],
    trns: &Transparency,
    geometry: PassGeometry,
    rgba: &mut [u8],
) -> Result<(), PngDecodeError> {
    let too_large = PngDecodeError::ImageTooLarge {
        width: header.width,
        height: header.height,
    };
    let row_len = header.row_bytes(geometry.width).ok_or(too_large)?;
    let filter_unit = header.filter_unit();

    let mut previous = vec![0u8; row_len];
    let mut current = vec![0u8; row_len];

    for pass_y in 0..geometry.height {
        let stride = row_len + 1;
        if raw.len() - *consumed < stride {
            return Err(PngDecodeError::DataSize {
                expected: *consumed + stride,
                actual: raw.len(),
            });
        }
        let filter = raw[*consumed];
        current.copy_from_slice(&raw[*consumed + 1..*consumed + stride]);
        *consumed += stride;

        unfilter_row(filter, &mut current, &previous, filter_unit)?;

        let dest_y = geometry.row_start + pass_y * geometry.row_step;
        for pass_x in 0..geometry.width {
            let pixel = read_pixel(&current, header, palette, trns, pass_x)?;
            let dest_x = geometry.col_start + pass_x * geometry.col_step;
            let idx = ((dest_y as usize) * (header.width as usize) + dest_x as usize) * 4;
            rgba[idx..idx + 4].copy_from_slice(&pixel);
        }

        core::mem::swap(&mut previous, &mut current);
    }

    Ok(())
}

/// Reverse one scanline filter in place, per PNG specification §9.2.
fn unfilter_row(
    filter: u8,
    current: &mut [u8],
    previous: &[u8],
    bpp: usize,
) -> Result<(), PngDecodeError> {
    match filter {
        0 => {}
        1 => {
            for i in bpp..current.len() {
                current[i] = current[i].wrapping_add(current[i - bpp]);
            }
        }
        2 => {
            for (cur, up) in current.iter_mut().zip(previous.iter()) {
                *cur = cur.wrapping_add(*up);
            }
        }
        3 => {
            for i in 0..current.len() {
                let left = if i >= bpp {
                    u16::from(current[i - bpp])
                } else {
                    0
                };
                let up = u16::from(previous[i]);
                current[i] = current[i].wrapping_add(((left + up) / 2) as u8);
            }
        }
        4 => {
            for i in 0..current.len() {
                let left = if i >= bpp { current[i - bpp] } else { 0 };
                let up = previous[i];
                let up_left = if i >= bpp { previous[i - bpp] } else { 0 };
                current[i] = current[i].wrapping_add(paeth(left, up, up_left));
            }
        }
        other => return Err(PngDecodeError::InvalidFilterType(other)),
    }
    Ok(())
}

/// The PNG Paeth predictor: pick whichever of `a`, `b`, `c` is closest to
/// `a + b - c`, breaking ties towards `a` then `b`.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Extract sample `index` of an unfiltered scanline at the image's bit depth.
///
/// Sub-byte depths are read MSB-first, matching the specification's packing
/// order. The value is returned at native depth (a 1-bit sample yields `0` or
/// `1`, a 16-bit sample the full `u16`) so `tRNS` keys can be compared before
/// any scaling happens.
fn sample(row: &[u8], bit_depth: u8, index: usize) -> u16 {
    match bit_depth {
        16 => {
            let byte = index * 2;
            u16::from_be_bytes([row[byte], row[byte + 1]])
        }
        8 => u16::from(row[index]),
        _ => {
            let bits = bit_depth as usize;
            let bit_pos = index * bits;
            let byte = bit_pos / 8;
            let shift = 8 - bits - (bit_pos % 8);
            let mask = (1u16 << bits) - 1;
            (u16::from(row[byte]) >> shift) & mask
        }
    }
}

/// Scale a native-depth sample to 8 bits.
///
/// Depths below 8 are expanded by the specification's exact-replication factor
/// (`255 / (2^depth − 1)`: ×255, ×85, ×17); 16-bit samples take the high byte.
fn scale_to_u8(value: u16, bit_depth: u8) -> u8 {
    match bit_depth {
        16 => (value >> 8) as u8,
        8 => value as u8,
        4 => (value * 17) as u8,
        2 => (value * 85) as u8,
        _ => (value * 255) as u8,
    }
}

/// Convert pixel `x` of an unfiltered scanline into straight-alpha RGBA8.
fn read_pixel(
    row: &[u8],
    header: Header,
    palette: &[[u8; 3]],
    trns: &Transparency,
    x: u32,
) -> Result<[u8; 4], PngDecodeError> {
    let depth = header.bit_depth;
    let x = x as usize;
    let channels = header.channels();
    let base = x * channels;

    let pixel = match header.color_type {
        0 => {
            let raw_grey = sample(row, depth, base);
            let grey = scale_to_u8(raw_grey, depth);
            let alpha = if trns.grey_key == Some(raw_grey) {
                0
            } else {
                255
            };
            [grey, grey, grey, alpha]
        }
        2 => {
            let raw_r = sample(row, depth, base);
            let raw_g = sample(row, depth, base + 1);
            let raw_b = sample(row, depth, base + 2);
            let alpha = if trns.rgb_key == Some((raw_r, raw_g, raw_b)) {
                0
            } else {
                255
            };
            [
                scale_to_u8(raw_r, depth),
                scale_to_u8(raw_g, depth),
                scale_to_u8(raw_b, depth),
                alpha,
            ]
        }
        3 => {
            let index = sample(row, depth, base) as usize;
            let entry = palette
                .get(index)
                .ok_or(PngDecodeError::PaletteIndexOutOfRange {
                    index,
                    palette_len: palette.len(),
                })?;
            let alpha = trns.palette_alpha.get(index).copied().unwrap_or(255);
            [entry[0], entry[1], entry[2], alpha]
        }
        4 => {
            let grey = scale_to_u8(sample(row, depth, base), depth);
            let alpha = scale_to_u8(sample(row, depth, base + 1), depth);
            [grey, grey, grey, alpha]
        }
        // Colour type 6; `parse_header` has already rejected every other code.
        _ => [
            scale_to_u8(sample(row, depth, base), depth),
            scale_to_u8(sample(row, depth, base + 1), depth),
            scale_to_u8(sample(row, depth, base + 2), depth),
            scale_to_u8(sample(row, depth, base + 3), depth),
        ],
    };

    Ok(pixel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxiarc_deflate::zlib_compress;

    /// Assemble a PNG file from a raw (already filtered) image data stream.
    ///
    /// Used to build spec vectors the in-tree encoder cannot emit: sub-byte
    /// bit depths, palettes, 16-bit samples, interlacing and hand-chosen
    /// scanline filters.
    fn build_png(ihdr: [u8; 13], extra_chunks: &[(&[u8; 4], Vec<u8>)], raw: &[u8]) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = Crc32::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.value().to_be_bytes());
        }

        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        chunk(&mut out, b"IHDR", &ihdr);
        for (kind, data) in extra_chunks {
            chunk(&mut out, kind, data);
        }
        let compressed = zlib_compress(raw, 6).expect("zlib_compress of a test vector");
        chunk(&mut out, b"IDAT", &compressed);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    /// Build an `IHDR` payload from its seven fields.
    fn ihdr(width: u32, height: u32, bit_depth: u8, color_type: u8, interlace: u8) -> [u8; 13] {
        let mut out = [0u8; 13];
        out[..4].copy_from_slice(&width.to_be_bytes());
        out[4..8].copy_from_slice(&height.to_be_bytes());
        out[8] = bit_depth;
        out[9] = color_type;
        out[12] = interlace;
        out
    }

    // -----------------------------------------------------------------
    // Round-trip against the in-tree encoder
    //
    // Gated on `png-encode`: `png-decode` is usable on its own, and these
    // tests are the only ones that need the writer.
    // -----------------------------------------------------------------

    #[cfg(feature = "png-encode")]
    use crate::png_encode::{encode_png, PngColorType};

    #[cfg(feature = "png-encode")]
    #[test]
    fn round_trip_rgba8() {
        let pixels: Vec<u8> = (0..(4 * 3 * 4)).map(|i| (i * 7 % 256) as u8).collect();
        let png = encode_png(4, 3, PngColorType::Rgba8, &pixels).expect("encode");
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!((image.width, image.height), (4, 3));
        assert_eq!(image.rgba, pixels);
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn round_trip_rgb8_gains_opaque_alpha() {
        let pixels = vec![10u8, 20, 30, 40, 50, 60];
        let png = encode_png(2, 1, PngColorType::Rgb8, &pixels).expect("encode");
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn round_trip_grayscale8() {
        let pixels = vec![0u8, 128, 255, 64];
        let png = encode_png(2, 2, PngColorType::Grayscale8, &pixels).expect("encode");
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(
            image.rgba,
            vec![0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255, 64, 64, 64, 255]
        );
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn round_trip_grayscale_alpha8() {
        let pixels = vec![200u8, 10, 30, 255];
        let png = encode_png(2, 1, PngColorType::GrayscaleAlpha8, &pixels).expect("encode");
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![200, 200, 200, 10, 30, 30, 30, 255]);
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn round_trip_large_image_spanning_many_scanlines() {
        // 64×64 RGBA exercises multi-scanline filtering including the adaptive
        // encoder's Average/Paeth choices.
        let mut pixels = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64u32 {
            for x in 0..64u32 {
                pixels.extend_from_slice(&[
                    (x * 4) as u8,
                    (y * 4) as u8,
                    ((x ^ y) * 3) as u8,
                    255 - (x as u8 / 2),
                ]);
            }
        }
        let png = encode_png(64, 64, PngColorType::Rgba8, &pixels).expect("encode");
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, pixels);
    }

    // -----------------------------------------------------------------
    // Hand-assembled spec vectors
    // -----------------------------------------------------------------

    /// Every scanline filter type 0..=4 must reverse to the same known image.
    ///
    /// The four 3-byte RGB pixels below are filtered by hand, one row per
    /// filter type, so the decoder's unfilter path is what is under test.
    #[test]
    fn all_five_filter_types_reverse_correctly() {
        // Target image: 2 pixels wide, 5 rows, RGB8. Every row is identical.
        let row: [u8; 6] = [10, 20, 30, 200, 100, 50];
        let bpp = 3usize;

        let mut raw = Vec::new();
        // Row 0 — filter 0 (None).
        raw.push(0);
        raw.extend_from_slice(&row);
        // Row 1 — filter 1 (Sub): out[i] = cur[i] - cur[i-bpp].
        raw.push(1);
        for (i, &value) in row.iter().enumerate() {
            let left = if i >= bpp { row[i - bpp] } else { 0 };
            raw.push(value.wrapping_sub(left));
        }
        // Row 2 — filter 2 (Up): previous row is identical, so all zeros.
        raw.push(2);
        raw.extend_from_slice(&[0u8; 6]);
        // Row 3 — filter 3 (Average).
        raw.push(3);
        for (i, &value) in row.iter().enumerate() {
            let left = if i >= bpp { u16::from(row[i - bpp]) } else { 0 };
            let up = u16::from(value);
            raw.push(value.wrapping_sub(((left + up) / 2) as u8));
        }
        // Row 4 — filter 4 (Paeth).
        raw.push(4);
        for (i, &value) in row.iter().enumerate() {
            let left = if i >= bpp { row[i - bpp] } else { 0 };
            let up = value;
            let up_left = if i >= bpp { row[i - bpp] } else { 0 };
            raw.push(value.wrapping_sub(paeth(left, up, up_left)));
        }

        let png = build_png(ihdr(2, 5, 8, 2, 0), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!((image.width, image.height), (2, 5));
        for y in 0..5usize {
            let off = y * 2 * 4;
            assert_eq!(
                &image.rgba[off..off + 8],
                &[10, 20, 30, 255, 200, 100, 50, 255],
                "row {y} (filter {y}) decoded incorrectly"
            );
        }
    }

    /// 1-bit greyscale: MSB-first packing, `tRNS` key makes black transparent.
    #[test]
    fn grayscale_1bit_with_trns_key() {
        // 4×1 pixels, bits 1,0,1,1 -> 0b1011_0000.
        let raw = vec![0u8, 0b1011_0000];
        let trns = vec![0x00, 0x00]; // sample value 0 is transparent
        let png = build_png(ihdr(4, 1, 1, 0, 0), &[(b"tRNS", trns)], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(
            image.rgba,
            vec![
                255, 255, 255, 255, // 1 -> white, opaque
                0, 0, 0, 0, // 0 -> black, transparent via tRNS
                255, 255, 255, 255, //
                255, 255, 255, 255,
            ]
        );
    }

    /// 2-bit greyscale scales samples by 85.
    #[test]
    fn grayscale_2bit_scales_by_85() {
        // 4×1 pixels: 0, 1, 2, 3 -> 0b00_01_10_11.
        let raw = vec![0u8, 0b0001_1011];
        let png = build_png(ihdr(4, 1, 2, 0, 0), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        let greys: Vec<u8> = image.rgba.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(greys, vec![0, 85, 170, 255]);
    }

    /// 4-bit greyscale scales samples by 17.
    #[test]
    fn grayscale_4bit_scales_by_17() {
        // 2×1 pixels: 0x0, 0xF.
        let raw = vec![0u8, 0x0F];
        let png = build_png(ihdr(2, 1, 4, 0, 0), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    /// Indexed colour with a partial `tRNS`: entries past its end are opaque.
    #[test]
    fn indexed_4bit_with_palette_and_trns() {
        let plte = vec![
            255, 0, 0, // 0: red
            0, 255, 0, // 1: green
            0, 0, 255, // 2: blue
        ];
        let trns = vec![0, 128]; // index 0 fully transparent, 1 half, 2 implicit 255
                                 // 3×1 pixels at 4 bpp: indices 0,1,2 -> 0x01, 0x20.
        let raw = vec![0u8, 0x01, 0x20];
        let png = build_png(
            ihdr(3, 1, 4, 3, 0),
            &[(b"PLTE", plte), (b"tRNS", trns)],
            &raw,
        );
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(
            image.rgba,
            vec![255, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255]
        );
    }

    /// 16-bit RGB is truncated to the high byte, and `tRNS` matches at full
    /// 16-bit precision (not after scaling).
    #[test]
    fn truecolor_16bit_trns_matches_at_native_depth() {
        // 2×1 pixels. Pixel 0 = (0x1234, 0x5678, 0x9abc) — the tRNS key.
        // Pixel 1 = (0x12ff, 0x5678, 0x9abc) — same high bytes, different
        // 16-bit value, so it must stay opaque.
        let mut raw = vec![0u8];
        raw.extend_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        raw.extend_from_slice(&[0x12, 0xff, 0x56, 0x78, 0x9a, 0xbc]);
        let trns = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let png = build_png(ihdr(2, 1, 16, 2, 0), &[(b"tRNS", trns)], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(
            image.rgba,
            vec![0x12, 0x56, 0x9a, 0x00, 0x12, 0x56, 0x9a, 0xff]
        );
    }

    /// 16-bit greyscale+alpha keeps the high byte of both channels.
    #[test]
    fn grayscale_alpha_16bit() {
        let mut raw = vec![0u8];
        raw.extend_from_slice(&[0xab, 0xcd, 0x80, 0x00]);
        let png = build_png(ihdr(1, 1, 16, 4, 0), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![0xab, 0xab, 0xab, 0x80]);
    }

    /// Adam7: an 8×8 image whose every pass carries pixels must reassemble
    /// byte-for-byte identically to the same image stored non-interlaced.
    #[test]
    fn adam7_interlaced_matches_non_interlaced() {
        // Reference: 8×8 greyscale where pixel value = y * 8 + x.
        let mut reference = Vec::with_capacity(64);
        for y in 0..8u32 {
            for x in 0..8u32 {
                reference.push((y * 8 + x) as u8);
            }
        }

        // Interlaced raw stream: for each Adam7 pass, one filter-0 scanline
        // per pass row containing that pass's pixels in order.
        let header = Header {
            width: 8,
            height: 8,
            bit_depth: 8,
            color_type: 0,
            interlace: 1,
        };
        let mut raw = Vec::new();
        for pass in 0..7usize {
            let geometry = adam7_geometry(header, pass);
            if geometry.width == 0 || geometry.height == 0 {
                continue;
            }
            for py in 0..geometry.height {
                raw.push(0u8); // filter: None
                let y = geometry.row_start + py * geometry.row_step;
                for px in 0..geometry.width {
                    let x = geometry.col_start + px * geometry.col_step;
                    raw.push(reference[(y * 8 + x) as usize]);
                }
            }
        }

        let png = build_png(ihdr(8, 8, 8, 0, 1), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!((image.width, image.height), (8, 8));
        let greys: Vec<u8> = image.rgba.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(greys, reference);
        // Alpha is fully opaque everywhere (no tRNS).
        assert!(image.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }

    /// Adam7 with a size that leaves several passes empty (1×1 only has pass 0).
    #[test]
    fn adam7_single_pixel_image() {
        let raw = vec![0u8, 0x7f];
        let png = build_png(ihdr(1, 1, 8, 0, 1), &[], &raw);
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![0x7f, 0x7f, 0x7f, 255]);
    }

    // -----------------------------------------------------------------
    // Third-party reference vectors
    //
    // These files were written by unrelated encoders (netpbm's `pnmtopng`
    // and Pillow) from known pixel data, so decoding them checks this module
    // against an independent implementation rather than against itself.
    // -----------------------------------------------------------------

    /// 9×7 Adam7-interlaced, 8-bit indexed PNG written by `pnmtopng
    /// -interlace` from the pixel formula asserted below. Exercises the
    /// interlace pass geometry *and* the `PLTE` path on a size where four of
    /// the seven passes are partial.
    const ADAM7_PALETTE_9X7: [u8; 342] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x07, 0x08, 0x03, 0x00, 0x00, 0x01, 0x9a,
        0x42, 0xa7, 0xc4, 0x00, 0x00, 0x00, 0xbd, 0x50, 0x4c, 0x54, 0x45, 0x00, 0x00, 0x00, 0xa0,
        0xb4, 0x8c, 0x00, 0x1e, 0x0a, 0x14, 0x00, 0x0a, 0x00, 0x3c, 0x14, 0x14, 0x1e, 0x14, 0x00,
        0x5a, 0x1e, 0x28, 0x00, 0x14, 0x14, 0x3c, 0x1e, 0x00, 0x78, 0x28, 0x28, 0x1e, 0x1e, 0x14,
        0x5a, 0x28, 0x00, 0x96, 0x32, 0x3c, 0x00, 0x1e, 0x28, 0x3c, 0x28, 0x14, 0x78, 0x32, 0x00,
        0xb4, 0x3c, 0x3c, 0x1e, 0x28, 0x28, 0x5a, 0x32, 0x14, 0x96, 0x3c, 0x50, 0x00, 0x28, 0x3c,
        0x3c, 0x32, 0x28, 0x78, 0x3c, 0x14, 0xb4, 0x46, 0x50, 0x1e, 0x32, 0x3c, 0x5a, 0x3c, 0x28,
        0x96, 0x46, 0x64, 0x00, 0x32, 0x50, 0x3c, 0x3c, 0x3c, 0x78, 0x46, 0x28, 0xb4, 0x50, 0x64,
        0x1e, 0x3c, 0x50, 0x5a, 0x46, 0x3c, 0x96, 0x50, 0x78, 0x00, 0x3c, 0x64, 0x3c, 0x46, 0x50,
        0x78, 0x50, 0x3c, 0xb4, 0x5a, 0x78, 0x1e, 0x46, 0x64, 0x5a, 0x50, 0x50, 0x96, 0x5a, 0x8c,
        0x00, 0x46, 0x78, 0x3c, 0x50, 0x64, 0x78, 0x5a, 0x50, 0xb4, 0x64, 0x8c, 0x1e, 0x50, 0x78,
        0x5a, 0x5a, 0x64, 0x96, 0x64, 0xa0, 0x00, 0x50, 0x8c, 0x3c, 0x5a, 0x78, 0x78, 0x64, 0x64,
        0xb4, 0x6e, 0xa0, 0x1e, 0x5a, 0x8c, 0x5a, 0x64, 0x78, 0x96, 0x6e, 0xa0, 0x3c, 0x64, 0x8c,
        0x78, 0x6e, 0x78, 0xb4, 0x78, 0xa0, 0x5a, 0x6e, 0x8c, 0x96, 0x78, 0xa0, 0x78, 0x78, 0x8c,
        0xb4, 0x82, 0xa0, 0x96, 0x82, 0x24, 0x34, 0x45, 0xf1, 0x00, 0x00, 0x00, 0x54, 0x49, 0x44,
        0x41, 0x54, 0x08, 0x99, 0x05, 0xc1, 0x87, 0x02, 0x42, 0x00, 0x00, 0x05, 0xc0, 0x67, 0xcb,
        0x4a, 0x56, 0xd9, 0x51, 0x59, 0x59, 0x45, 0xd9, 0xfe, 0xff, 0xb3, 0xdc, 0x01, 0x2d, 0x0c,
        0x9c, 0x9e, 0x2b, 0xb8, 0x18, 0xd6, 0x17, 0xb4, 0xe2, 0x15, 0x03, 0xd4, 0xa0, 0x9c, 0x08,
        0x50, 0xb2, 0x9b, 0x83, 0x37, 0x1f, 0x1f, 0x9c, 0xfd, 0xf7, 0x88, 0xeb, 0xab, 0xdb, 0x40,
        0x32, 0xc2, 0xe5, 0x16, 0x26, 0x55, 0x0f, 0x56, 0xd4, 0xec, 0x7b, 0x5a, 0xff, 0x66, 0x48,
        0xba, 0x13, 0x65, 0xcd, 0x7f, 0xd9, 0x0f, 0x01, 0x3c, 0x07, 0xa2, 0x99, 0xed, 0x69, 0xfa,
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// 3×1 16-bit greyscale PNG written by Pillow from samples
    /// `0x0000, 0x8000, 0xffff`.
    const GRAY16_3X1: [u8; 72] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x6e,
        0x1b, 0x97, 0x2b, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60,
        0x60, 0x68, 0x60, 0xf8, 0xff, 0x1f, 0x00, 0x05, 0x04, 0x02, 0x7f, 0xe3, 0x80, 0x4b, 0xe0,
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// 8×2 1-bit bilevel PNG written by Pillow; bit pattern asserted below.
    const BILEVEL_8X2: [u8; 69] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x4d,
        0xef, 0xa0, 0x40, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x58,
        0xc3, 0x60, 0x0a, 0x00, 0x02, 0x3d, 0x00, 0xe2, 0x5e, 0x17, 0x3c, 0xc0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn third_party_adam7_indexed_image_decodes_exactly() {
        let image = decode_png_rgba8(&ADAM7_PALETTE_9X7).expect("decode");
        assert_eq!((image.width, image.height), (9, 7));
        for y in 0..7u32 {
            for x in 0..9u32 {
                let idx = ((y * 9 + x) * 4) as usize;
                let expected = [
                    (x * 20 % 256) as u8,
                    (y * 30 % 256) as u8,
                    ((x + y) * 10 % 256) as u8,
                    255,
                ];
                assert_eq!(
                    &image.rgba[idx..idx + 4],
                    &expected,
                    "pixel ({x}, {y}) mismatched"
                );
            }
        }
    }

    #[test]
    fn third_party_16bit_greyscale_takes_high_byte() {
        let image = decode_png_rgba8(&GRAY16_3X1).expect("decode");
        assert_eq!((image.width, image.height), (3, 1));
        assert_eq!(
            image.rgba,
            vec![
                0x00, 0x00, 0x00, 0xff, // 0x0000
                0x80, 0x80, 0x80, 0xff, // 0x8000
                0xff, 0xff, 0xff, 0xff, // 0xffff
            ]
        );
    }

    #[test]
    fn third_party_1bit_bilevel_unpacks_msb_first() {
        let image = decode_png_rgba8(&BILEVEL_8X2).expect("decode");
        assert_eq!((image.width, image.height), (8, 2));
        let bits = [
            1u8, 0, 1, 0, 1, 1, 0, 0, // row 0
            0, 0, 1, 1, 0, 1, 0, 1, // row 1
        ];
        let greys: Vec<u8> = image.rgba.chunks_exact(4).map(|p| p[0]).collect();
        let expected: Vec<u8> = bits.iter().map(|&b| if b == 1 { 255 } else { 0 }).collect();
        assert_eq!(greys, expected);
    }

    /// Bytes after `IEND` are ignored, not treated as a truncated chunk.
    ///
    /// This matters for real fonts: a `CBDT`/`sbix` strike record is sized by
    /// the table's own offsets, which are commonly padded, so the byte slice
    /// handed to the decoder can be longer than the PNG inside it.
    #[cfg(feature = "png-encode")]
    #[test]
    fn trailing_bytes_after_iend_are_ignored() {
        let pixels = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let png = encode_png(2, 1, PngColorType::Rgba8, &pixels).expect("encode");
        let clean = decode_png_rgba8(&png).expect("decode");

        for padding in [&[0u8][..], &[0u8; 3][..], &[0xff; 64][..]] {
            let mut padded = png.clone();
            padded.extend_from_slice(padding);
            assert_eq!(
                decode_png_rgba8(&padded).expect("padded decode"),
                clean,
                "{} trailing byte(s) changed the result",
                padding.len()
            );
        }
    }

    /// Unknown ancillary chunks are CRC-checked and skipped, not rejected.
    #[test]
    fn ancillary_chunks_are_skipped() {
        let raw = vec![0u8, 0x40];
        let png = build_png(
            ihdr(1, 1, 8, 0, 0),
            &[
                (b"gAMA", vec![0, 1, 0x86, 0xa0]),
                (b"tEXt", b"key\0v".to_vec()),
            ],
            &raw,
        );
        let image = decode_png_rgba8(&png).expect("decode");
        assert_eq!(image.rgba, vec![0x40, 0x40, 0x40, 255]);
    }

    /// Image data split across several `IDAT` chunks inflates as one stream.
    #[cfg(feature = "png-encode")]
    #[test]
    fn multiple_idat_chunks_are_concatenated() {
        let pixels: Vec<u8> = (0..(16 * 16 * 4)).map(|i| (i % 251) as u8).collect();
        let png = encode_png(16, 16, PngColorType::Rgba8, &pixels).expect("encode");
        // Re-split the single IDAT payload into two chunks.
        let decoded_once = decode_png_rgba8(&png).expect("decode");
        let mut idat_payload = Vec::new();
        let mut cursor = PNG_SIGNATURE.len();
        let mut rebuilt = Vec::new();
        rebuilt.extend_from_slice(&PNG_SIGNATURE);
        let mut tail = Vec::new();
        while cursor < png.len() {
            let (kind, payload, next) = read_chunk(&png, cursor).expect("chunk");
            match &kind {
                b"IDAT" => idat_payload.extend_from_slice(payload),
                b"IEND" => tail.extend_from_slice(&png[cursor..next]),
                _ => rebuilt.extend_from_slice(&png[cursor..next]),
            }
            cursor = next;
        }
        let (first, second) = idat_payload.split_at(idat_payload.len() / 2);
        for part in [first, second] {
            let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
            rebuilt.extend_from_slice(&len.to_be_bytes());
            rebuilt.extend_from_slice(b"IDAT");
            rebuilt.extend_from_slice(part);
            let mut crc = Crc32::new();
            crc.update(b"IDAT");
            crc.update(part);
            rebuilt.extend_from_slice(&crc.value().to_be_bytes());
        }
        rebuilt.extend_from_slice(&tail);

        let decoded_split = decode_png_rgba8(&rebuilt).expect("decode split");
        assert_eq!(decoded_split, decoded_once);
    }

    // -----------------------------------------------------------------
    // Error paths
    // -----------------------------------------------------------------

    #[test]
    fn rejects_non_png() {
        assert_eq!(
            decode_png_rgba8(b"not a png at all"),
            Err(PngDecodeError::NotAPng)
        );
        assert_eq!(decode_png_rgba8(&[]), Err(PngDecodeError::NotAPng));
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn rejects_truncated_stream() {
        let png = encode_png(2, 2, PngColorType::Rgba8, &[0u8; 16]).expect("encode");
        let cut = &png[..png.len() - 6];
        assert!(matches!(
            decode_png_rgba8(cut),
            Err(PngDecodeError::Truncated { .. })
        ));
    }

    #[cfg(feature = "png-encode")]
    #[test]
    fn rejects_crc_mismatch() {
        let mut png = encode_png(1, 1, PngColorType::Grayscale8, &[9]).expect("encode");
        // Corrupt the last byte of the IHDR CRC (signature 8 + len 4 + type 4
        // + payload 13 + crc 4 => byte 32 is the final CRC byte).
        png[32] ^= 0xff;
        assert!(matches!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn rejects_zero_dimension() {
        let png = build_png(ihdr(0, 1, 8, 0, 0), &[], &[0u8]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::InvalidDimensions {
                width: 0,
                height: 1
            })
        );
    }

    #[test]
    fn rejects_unknown_color_type() {
        let png = build_png(ihdr(1, 1, 8, 5, 0), &[], &[0u8, 0]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::UnsupportedColorType(5))
        );
    }

    #[test]
    fn rejects_bad_bit_depth_for_color_type() {
        // Colour type 2 (truecolour) has no 4-bit form.
        let png = build_png(ihdr(1, 1, 4, 2, 0), &[], &[0u8, 0]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::UnsupportedBitDepth {
                color_type: 2,
                bit_depth: 4
            })
        );
    }

    #[test]
    fn rejects_unknown_interlace_method() {
        let png = build_png(ihdr(1, 1, 8, 0, 2), &[], &[0u8, 0]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::UnsupportedInterlaceMethod(2))
        );
    }

    #[test]
    fn rejects_indexed_without_palette() {
        let png = build_png(ihdr(1, 1, 8, 3, 0), &[], &[0u8, 0]);
        assert_eq!(decode_png_rgba8(&png), Err(PngDecodeError::MissingPalette));
    }

    #[test]
    fn rejects_palette_index_beyond_plte() {
        let plte = vec![1, 2, 3]; // one entry only
        let raw = vec![0u8, 1]; // index 1 -> out of range
        let png = build_png(ihdr(1, 1, 8, 3, 0), &[(b"PLTE", plte)], &raw);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::PaletteIndexOutOfRange {
                index: 1,
                palette_len: 1
            })
        );
    }

    #[test]
    fn rejects_malformed_palette_length() {
        let png = build_png(ihdr(1, 1, 8, 3, 0), &[(b"PLTE", vec![1, 2])], &[0u8, 0]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::InvalidPalette { length: 2 })
        );
    }

    #[test]
    fn rejects_short_image_data() {
        // IHDR promises 4 rows, the stream only carries one.
        let png = build_png(ihdr(1, 4, 8, 0, 0), &[], &[0u8, 0]);
        assert!(matches!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::DataSize { .. })
        ));
    }

    /// A decompression bomb must be stopped by the output buffer, not decoded
    /// into memory and silently truncated to the declared geometry.
    #[test]
    fn rejects_image_data_that_inflates_past_the_declared_geometry() {
        // 10 MiB of zeros compresses to a few KiB; the IHDR declares a 2×2
        // RGBA image, i.e. 18 bytes of raw stream.
        let bomb = vec![0u8; 10 * 1024 * 1024];
        let compressed = zlib_compress(&bomb, 6).expect("compress");
        assert!(
            compressed.len() * MAX_DEFLATE_RATIO > bomb.len(),
            "test vector must pass the ratio pre-check so the buffer guard is what fires"
        );

        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        let mut push = |kind: &[u8; 4], data: &[u8]| {
            let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = Crc32::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.value().to_be_bytes());
        };
        push(b"IHDR", &ihdr(2, 2, 8, 6, 0));
        push(b"IDAT", &compressed);
        push(b"IEND", &[]);

        assert_eq!(
            decode_png_rgba8(&out),
            Err(PngDecodeError::ExcessImageData { expected: 18 })
        );
    }

    /// A tiny file whose `IHDR` declares a huge image is rejected before any
    /// buffer is allocated for it.
    #[test]
    fn rejects_geometry_unreachable_from_the_idat_length() {
        // 65535×65535 greyscale needs ~4.3 GB of raw stream; the IDAT below is
        // a few bytes, so no expansion ratio could produce it.
        let raw = vec![0u8; 4];
        let png = build_png(ihdr(65535, 65535, 8, 0, 0), &[], &raw);
        match decode_png_rgba8(&png) {
            Err(PngDecodeError::CompressionRatioExceeded {
                expected,
                compressed,
            }) => {
                assert_eq!(expected, (65535usize + 1) * 65535);
                assert!(compressed > 0 && compressed * MAX_DEFLATE_RATIO < expected);
            }
            other => panic!("expected CompressionRatioExceeded, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_filter_type() {
        let png = build_png(ihdr(1, 1, 8, 0, 0), &[], &[7u8, 0]);
        assert_eq!(
            decode_png_rgba8(&png),
            Err(PngDecodeError::InvalidFilterType(7))
        );
    }

    #[test]
    fn rejects_missing_idat() {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        let header = ihdr(1, 1, 8, 0, 0);
        let mut push = |kind: &[u8; 4], data: &[u8]| {
            let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = Crc32::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.value().to_be_bytes());
        };
        push(b"IHDR", &header);
        push(b"IEND", &[]);
        assert_eq!(
            decode_png_rgba8(&out),
            Err(PngDecodeError::MissingImageData)
        );
    }

    /// Apple's `CgBI` variant is reported as itself, not as a malformed
    /// header: Xcode rewrites every PNG in an app bundle into that format, and
    /// "IHDR chunk is missing or malformed" would misdirect anyone who hits it.
    #[test]
    fn reports_apple_cgbi_variant_distinctly() {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        let payload = [0x50, 0x00, 0x20, 0x02];
        let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(b"CgBI");
        out.extend_from_slice(&payload);
        let mut crc = Crc32::new();
        crc.update(b"CgBI");
        crc.update(&payload);
        out.extend_from_slice(&crc.value().to_be_bytes());
        assert_eq!(decode_png_rgba8(&out), Err(PngDecodeError::AppleCgBi));
    }

    #[test]
    fn rejects_stream_whose_first_chunk_is_not_ihdr() {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        let data = [0u8; 4];
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(b"gAMA");
        out.extend_from_slice(&data);
        let mut crc = Crc32::new();
        crc.update(b"gAMA");
        crc.update(&data);
        out.extend_from_slice(&crc.value().to_be_bytes());
        assert_eq!(decode_png_rgba8(&out), Err(PngDecodeError::InvalidHeader));
    }

    #[test]
    fn rejects_corrupt_deflate_stream() {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        let header = ihdr(1, 1, 8, 0, 0);
        let mut push = |kind: &[u8; 4], data: &[u8]| {
            let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = Crc32::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.value().to_be_bytes());
        };
        push(b"IHDR", &header);
        push(b"IDAT", &[0xde, 0xad, 0xbe, 0xef]);
        push(b"IEND", &[]);
        assert!(matches!(
            decode_png_rgba8(&out),
            Err(PngDecodeError::Decompression(_))
        ));
    }

    /// Every error variant renders a non-empty, distinct message.
    #[test]
    fn error_display_is_populated() {
        let variants = [
            PngDecodeError::NotAPng,
            PngDecodeError::Truncated {
                offset: 1,
                needed: 2,
            },
            PngDecodeError::InvalidHeader,
            PngDecodeError::AppleCgBi,
            PngDecodeError::ChecksumMismatch {
                chunk: *b"IDAT",
                stored: 1,
                computed: 2,
            },
            PngDecodeError::InvalidDimensions {
                width: 0,
                height: 0,
            },
            PngDecodeError::UnsupportedColorType(9),
            PngDecodeError::UnsupportedBitDepth {
                color_type: 2,
                bit_depth: 3,
            },
            PngDecodeError::UnsupportedCompressionMethod(1),
            PngDecodeError::UnsupportedFilterMethod(1),
            PngDecodeError::UnsupportedInterlaceMethod(3),
            PngDecodeError::MissingPalette,
            PngDecodeError::InvalidPalette { length: 2 },
            PngDecodeError::PaletteIndexOutOfRange {
                index: 4,
                palette_len: 2,
            },
            PngDecodeError::MissingImageData,
            PngDecodeError::Decompression("boom".to_string()),
            PngDecodeError::DataSize {
                expected: 4,
                actual: 1,
            },
            PngDecodeError::ExcessImageData { expected: 4 },
            PngDecodeError::CompressionRatioExceeded {
                expected: 4096,
                compressed: 1,
            },
            PngDecodeError::InvalidFilterType(9),
            PngDecodeError::ImageTooLarge {
                width: 1,
                height: 2,
            },
        ];
        for v in &variants {
            assert!(!v.to_string().is_empty());
        }
    }

    #[test]
    fn paeth_predictor_matches_specification_examples() {
        // p = a + b - c; ties break towards a, then b.
        assert_eq!(paeth(10, 20, 15), 15);
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(255, 0, 0), 255);
        assert_eq!(paeth(1, 2, 3), 1);
    }
}
