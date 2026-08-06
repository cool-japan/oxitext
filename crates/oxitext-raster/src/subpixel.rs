//! Subpixel pen-positioning support.
//!
//! Provides [`SubpixelOffset`] — a quarter-pixel bucketed fractional X
//! position — and [`SubpixelCacheKey`], which extends the usual
//! `(glyph_id, size)` cache key with the fractional bucket so that glyphs
//! rasterized at different subpixel origins are stored separately.
//!
//! # Usage
//!
//! ```rust
//! use oxitext_raster::subpixel::{SubpixelOffset, SubpixelCacheKey};
//!
//! let offset = SubpixelOffset::from_float(0.75);
//! let key = SubpixelCacheKey {
//!     glyph_id: 36,
//!     px_size_times_64: (16.0_f32 * 64.0) as u32,
//!     subpixel: offset,
//! };
//! assert_eq!(key.subpixel.as_float(), 0.75);
//! ```

/// Quantised fractional X pen position, bucketed to one of [`SubpixelOffset::N`]
/// evenly-spaced values in `[0, 1)`.
///
/// The default resolution is quarter-pixel (`N = 4`), which gives subpixel
/// values 0.00, 0.25, 0.50, and 0.75.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct SubpixelOffset(u8);

impl SubpixelOffset {
    /// Number of buckets. Quarter-pixel resolution by default.
    pub const N: u8 = 4;

    /// Quantise a fractional pixel offset `frac` into one of [`Self::N`] buckets.
    ///
    /// Only the fractional part of `frac` is used (i.e. `frac.fract()`), so
    /// values outside `[0, 1)` are accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use oxitext_raster::subpixel::SubpixelOffset;
    /// assert_eq!(SubpixelOffset::from_float(0.0).as_float(), 0.0);
    /// assert_eq!(SubpixelOffset::from_float(0.5).as_float(), 0.5);
    /// ```
    pub fn from_float(frac: f32) -> Self {
        let bucket = (frac.fract() * Self::N as f32).floor() as i32;
        // Handle negative fract() on negative numbers.
        let bucket = bucket.rem_euclid(Self::N as i32) as u8;
        SubpixelOffset(bucket.min(Self::N - 1))
    }

    /// Returns this offset as a floating-point value in `[0, 1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use oxitext_raster::subpixel::SubpixelOffset;
    /// let off = SubpixelOffset::from_float(0.75);
    /// assert!((off.as_float() - 0.75).abs() < 1e-5);
    /// ```
    pub fn as_float(self) -> f32 {
        self.0 as f32 / Self::N as f32
    }

    /// Returns the raw bucket index (0 ≤ index < N).
    #[inline]
    pub fn bucket(self) -> u8 {
        self.0
    }
}

/// A cache key for rasterised glyphs that incorporates subpixel positioning.
///
/// Including the fractional pen position in the cache key ensures that glyphs
/// rasterized at different subpixel origins produce distinct entries and are
/// not accidentally conflated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SubpixelCacheKey {
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// Pixel size multiplied by 64 to allow integer comparison while
    /// preserving sub-pixel size differences.
    pub px_size_times_64: u32,
    /// Quantised fractional X offset bucket.
    pub subpixel: SubpixelOffset,
}

impl SubpixelCacheKey {
    /// Construct a cache key from raw components.
    ///
    /// `px_size` is converted to a fixed-point `u32` by multiplying by 64
    /// and rounding.
    pub fn new(glyph_id: u16, px_size: f32, x_offset: f32) -> Self {
        Self {
            glyph_id,
            px_size_times_64: (px_size * 64.0).round() as u32,
            subpixel: SubpixelOffset::from_float(x_offset),
        }
    }
}

/// Configurable bucket count for subpixel pen-position quantization.
///
/// Higher bucket counts give finer subpixel precision at the cost of more
/// cache entries.  The default is [`SubpixelBuckets::Four`] (quarter-pixel),
/// which matches [`SubpixelOffset::N`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubpixelBuckets {
    /// Quarter-pixel precision: 4 buckets (0.00, 0.25, 0.50, 0.75).
    #[default]
    Four,
    /// Eighth-pixel precision: 8 buckets.
    Eight,
    /// Sixteenth-pixel precision: 16 buckets.
    Sixteen,
}

impl SubpixelBuckets {
    /// The integer bucket count corresponding to this variant.
    pub const fn count(self) -> u32 {
        match self {
            SubpixelBuckets::Four => 4,
            SubpixelBuckets::Eight => 8,
            SubpixelBuckets::Sixteen => 16,
        }
    }
}

impl SubpixelOffset {
    /// Quantize a fractional offset to the nearest bucket with a configurable
    /// bucket count via [`SubpixelBuckets`].
    ///
    /// Only the fractional part of `frac` is used.
    ///
    /// # Examples
    ///
    /// ```
    /// # use oxitext_raster::subpixel::{SubpixelOffset, SubpixelBuckets};
    /// let b = SubpixelOffset::bucket_with_count(0.5, SubpixelBuckets::Eight);
    /// assert_eq!(b.bucket(), 4);  // 0.5 * 8 = 4
    /// ```
    pub fn bucket_with_count(frac: f32, buckets: SubpixelBuckets) -> Self {
        let n = buckets.count();
        let idx = (frac.rem_euclid(1.0) * n as f32).round() as u32 % n;
        // Store only the lower 8 bits; n <= 16 so this is always lossless.
        SubpixelOffset(idx as u8)
    }
}

/// A 2-D subpixel pen position combining independent X and Y fractional offsets.
///
/// Useful for rendering engines that need to quantise both axes independently
/// (e.g. for vertical metrics in CJK or grid-snapped vertical text).
///
/// # Examples
///
/// ```rust
/// use oxitext_raster::subpixel::{SubpixelOffset, SubpixelOffsetXY};
///
/// let xy = SubpixelOffsetXY::new(0.25, 0.5);
/// assert_eq!(xy.x.bucket(), 1);
/// assert_eq!(xy.y.bucket(), 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SubpixelOffsetXY {
    /// Fractional X-axis pen offset.
    pub x: SubpixelOffset,
    /// Fractional Y-axis pen offset.
    pub y: SubpixelOffset,
}

impl SubpixelOffsetXY {
    /// Construct a [`SubpixelOffsetXY`] from floating-point X and Y offsets.
    ///
    /// Each axis is quantised independently via [`SubpixelOffset::from_float`].
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: SubpixelOffset::from_float(x),
            y: SubpixelOffset::from_float(y),
        }
    }
}

/// Rasterize a glyph with a fractional X offset applied.
///
/// Uses ab_glyph to produce a coverage bitmap whose left edge is genuinely
/// shifted by `x_offset` pixels within the glyph's pixel grid.  Two calls
/// with the same `glyph_id` / `px_size` but different `x_offset` values will
/// yield bitmaps with different left-edge coverage — which is the whole point
/// of subpixel pen positioning.
///
/// Returns `(cache_key, coverage_bitmap)` on success, or `None` if the font
/// cannot be parsed or the glyph has no outline (e.g. whitespace).
///
/// # Arguments
/// - `face_data`: raw TTF/OTF bytes.
/// - `glyph_id`: the glyph index to rasterize.
/// - `px_size`: pixels-per-em rendering size.
/// - `x_offset`: fractional pen offset in pixels; only the fractional part is
///   used for positioning (`x_offset.fract()`).
pub fn rasterize_with_offset(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    x_offset: f32,
) -> Option<(SubpixelCacheKey, Vec<u8>)> {
    use ab_glyph::{Font, FontRef, GlyphId as AbGlyphId, PxScale};

    let font = FontRef::try_from_slice(face_data).ok()?;
    let scale = PxScale::from(px_size);
    let ab_gid = AbGlyphId(glyph_id);

    // Use only the fractional part of x_offset as the left-edge shift.
    let frac_x = x_offset.fract().abs();

    // Position the glyph at (frac_x, 0.0) so that the rasterizer samples the
    // outline at the desired sub-pixel left edge.  This is the canonical way to
    // achieve genuine fractional-origin positioning with ab_glyph.
    let glyph = ab_gid.with_scale_and_position(scale, ab_glyph::point(frac_x, 0.0));

    let outlined = font.outline_glyph(glyph)?;

    let bounds = outlined.px_bounds();
    let w = bounds.width().ceil() as usize;
    let h = bounds.height().ceil() as usize;
    if w == 0 || h == 0 {
        // Zero-area outline (e.g. space) — return an empty bitmap.
        let key = SubpixelCacheKey::new(glyph_id, px_size, x_offset);
        return Some((key, Vec::new()));
    }

    let mut coverage = vec![0u8; w * h];
    outlined.draw(|x, y, c| {
        let idx = y as usize * w + x as usize;
        if idx < coverage.len() {
            coverage[idx] = (c * 255.0).round() as u8;
        }
    });

    let key = SubpixelCacheKey::new(glyph_id, px_size, x_offset);
    Some((key, coverage))
}

/// Rasterize a glyph with independent fractional X and Y pen offsets applied,
/// returning a full sized [`oxitext_core::Bitmap`].
///
/// Like [`rasterize_with_offset`], this uses `ab_glyph`'s fractional glyph
/// positioning so the sampled outline is genuinely shifted within the pixel
/// grid rather than merely tagged in a cache key: two calls with the same
/// `glyph_id`/`px_size` but different `x_offset`/`y_offset` produce bitmaps
/// with different edge coverage. Unlike [`rasterize_with_offset`] (which
/// returns only the raw coverage bytes and a cache key), this returns the
/// bitmap's `width`/`height` too, since callers need them to interpret the
/// pixel buffer.
///
/// Only the fractional part of each offset is used (via [`f32::rem_euclid`]),
/// matching the `[0.0, 1.0)` contract documented on
/// [`crate::backend::FontdueRaster::raster_positioned`].
///
/// Returns `None` if the font cannot be parsed or the glyph has no outline
/// (e.g. whitespace or an out-of-range glyph id) — callers should fall back
/// to a backend that tolerates such input (e.g. fontdue) in that case.
pub fn rasterize_positioned_bitmap(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    x_offset: f32,
    y_offset: f32,
) -> Option<oxitext_core::Bitmap> {
    use ab_glyph::{Font, FontRef, GlyphId as AbGlyphId, PxScale};

    let font = FontRef::try_from_slice(face_data).ok()?;
    let scale = PxScale::from(px_size);
    let ab_gid = AbGlyphId(glyph_id);

    let frac_x = x_offset.rem_euclid(1.0);
    let frac_y = y_offset.rem_euclid(1.0);

    // Position the glyph at the requested fractional pen offset so the
    // rasterizer samples the outline at that exact sub-pixel origin — the
    // same technique `rasterize_with_offset` uses for the X-only case.
    let glyph = ab_gid.with_scale_and_position(scale, ab_glyph::point(frac_x, frac_y));

    let outlined = font.outline_glyph(glyph)?;

    let bounds = outlined.px_bounds();
    let w = bounds.width().ceil() as usize;
    let h = bounds.height().ceil() as usize;
    if w == 0 || h == 0 {
        // Zero-area outline (e.g. space) — return an empty bitmap rather than
        // `None` so callers can distinguish "no ink" from "unrasterizable".
        return Some(oxitext_core::Bitmap {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        });
    }

    let mut coverage = vec![0u8; w * h];
    outlined.draw(|x, y, c| {
        let idx = y as usize * w + x as usize;
        if idx < coverage.len() {
            coverage[idx] = (c * 255.0).round() as u8;
        }
    });

    Some(oxitext_core::Bitmap {
        width: w as u32,
        height: h as u32,
        pixels: coverage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load_test_font() -> Vec<u8> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
        if fixture.exists() {
            return std::fs::read(&fixture).expect("read fixture font");
        }
        let candidates = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ];
        for p in &candidates {
            if Path::new(p).exists() {
                return std::fs::read(p).expect("read system font");
            }
        }
        // Fall back to statically bundled Noto Sans Regular for deterministic,
        // system-font-independent testing.
        oxifont_bundled::NOTO_SANS_REGULAR.to_vec()
    }

    #[test]
    fn rasterize_positioned_bitmap_differs_across_offsets() {
        // Regression test: `rasterize_positioned_bitmap` (and, through it,
        // `FontdueRaster::raster_positioned`) must genuinely shift the sampled
        // outline, not just accept-and-discard the fractional offsets. GID 36
        // is typically a visible Latin glyph (often 'A'), matching the
        // convention `oxitext-raster`'s other tests use.
        let font_bytes = load_test_font();
        let glyph_id = 36u16;
        let px_size = 32.0_f32;

        let at_origin = rasterize_positioned_bitmap(&font_bytes, glyph_id, px_size, 0.0, 0.0)
            .expect("rasterize at (0.0, 0.0) must succeed for a visible glyph");
        let shifted = rasterize_positioned_bitmap(&font_bytes, glyph_id, px_size, 0.5, 0.5)
            .expect("rasterize at (0.5, 0.5) must succeed for a visible glyph");

        if at_origin.pixels.is_empty() || shifted.pixels.is_empty() {
            // The glyph id resolved to whitespace or an empty outline on
            // whatever font was found — nothing to compare, but the calls
            // above already proved neither offset panics.
            return;
        }

        assert_ne!(
            (at_origin.width, at_origin.height, &at_origin.pixels),
            (shifted.width, shifted.height, &shifted.pixels),
            "a (0.5, 0.5) pen shift must change the rasterized bitmap \
             (dimensions and/or per-pixel coverage), proving the offsets are \
             not silently discarded"
        );
    }

    #[test]
    fn rasterize_positioned_bitmap_empty_font_returns_none() {
        // Unparseable font data must return `None`, not panic — callers (e.g.
        // `FontdueRaster::raster_positioned`) are expected to fall back to
        // another backend in this case.
        assert!(rasterize_positioned_bitmap(&[], 0, 12.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn subpixel_offset_buckets() {
        assert_eq!(SubpixelOffset::from_float(0.0).bucket(), 0);
        assert_eq!(SubpixelOffset::from_float(0.25).bucket(), 1);
        assert_eq!(SubpixelOffset::from_float(0.5).bucket(), 2);
        assert_eq!(SubpixelOffset::from_float(0.75).bucket(), 3);
    }

    #[test]
    fn subpixel_offset_as_float_round_trips() {
        for i in 0..SubpixelOffset::N {
            let frac = i as f32 / SubpixelOffset::N as f32;
            let off = SubpixelOffset::from_float(frac);
            assert!(
                (off.as_float() - frac).abs() < 1e-5,
                "round-trip failed for bucket {i}: got {}",
                off.as_float()
            );
        }
    }

    #[test]
    fn cache_key_uniqueness() {
        let k0 = SubpixelCacheKey::new(36, 16.0, 0.0);
        let k1 = SubpixelCacheKey::new(36, 16.0, 0.5);
        assert_ne!(k0, k1, "keys with different subpixel offsets must differ");
        assert_ne!(k0.subpixel, k1.subpixel);
    }

    #[test]
    fn subpixel_xy_default() {
        let xy = SubpixelOffsetXY::default();
        assert_eq!(xy.x, SubpixelOffset::default());
        assert_eq!(xy.y, SubpixelOffset::default());
    }

    #[test]
    fn subpixel_xy_new_round_trip() {
        let xy = SubpixelOffsetXY::new(0.25, 0.5);
        assert_eq!(xy.x.bucket(), 1);
        assert_eq!(xy.y.bucket(), 2);
    }
}
