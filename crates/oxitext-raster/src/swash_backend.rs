//! Swash-based rasterizer backend with TrueType bytecode hinting.
//!
//! [`SwashRaster`] wraps `swash::scale::ScaleContext` and provides full
//! TrueType hinted outline rendering via `swash`'s skrifa-backed hinting
//! engine.  Hinting is enabled by default (`hint = true`) and can be disabled
//! via [`SwashRaster::with_hint`].
//!
//! # Thread safety
//!
//! `ScaleContext` is not `Send + Sync` because it contains mutable scratch
//! buffers and LRU caches.  [`SwashRaster`] wraps the context in a
//! [`std::sync::Mutex`] so that the struct is safe to share across threads;
//! concurrent rasterize calls are serialised on the mutex.

use swash::{
    scale::{Render, ScaleContext, Source, StrikeWith},
    FontRef,
};

use crate::backend::{RasterBackend, RasterOutput};

/// Swash-based glyph rasterizer with optional TrueType hinting.
///
/// Uses `swash::scale::ScaleContext` with `hint(true)` by default so that
/// outline instructions are applied before rasterization, producing
/// grid-fitted bitmaps that look sharper at small pixel sizes.
///
/// # Examples
///
/// ```rust,ignore
/// use oxitext_raster::SwashRaster;
/// use oxitext_raster::backend::RasterBackend;
///
/// let raster = SwashRaster::new();
/// let out = raster.rasterize(font_data, glyph_id, 16.0);
/// ```
pub struct SwashRaster {
    context: std::sync::Mutex<ScaleContext>,
    /// Whether TrueType bytecode hinting is applied to outlines.
    hint: bool,
}

impl SwashRaster {
    /// Creates a new [`SwashRaster`] with hinting **enabled**.
    pub fn new() -> Self {
        Self {
            context: std::sync::Mutex::new(ScaleContext::new()),
            hint: true,
        }
    }

    /// Creates a new [`SwashRaster`] with the given hinting preference.
    ///
    /// Pass `hint = true` for TrueType bytecode hinting (grid-fitted outlines),
    /// or `hint = false` for unhinted outlines.
    pub fn with_hint(hint: bool) -> Self {
        Self {
            context: std::sync::Mutex::new(ScaleContext::new()),
            hint,
        }
    }
}

impl Default for SwashRaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper: compute the horizontal advance for `glyph_id` from `font_ref` at
/// `px_size` using swash `GlyphMetrics`.
///
/// Returns `0.0` if the font cannot be measured (e.g. `units_per_em == 0`).
fn glyph_advance(font_ref: FontRef<'_>, glyph_id: u16, px_size: f32) -> f32 {
    let gm = font_ref.glyph_metrics(&[]).scale(px_size);
    gm.advance_width(glyph_id)
}

impl RasterBackend for SwashRaster {
    /// Rasterize `glyph_id` from `face_data` at `px_size` pixels-per-em.
    ///
    /// Uses swash's outline renderer with the hinting flag set at construction
    /// time.  Returns a zero-sized [`RasterOutput`] (no panic) for:
    ///
    /// - Invalid / unrecognised font data.
    /// - A glyph with no outline (e.g. whitespace).
    /// - A poisoned mutex (internal error, should never occur in practice).
    fn rasterize(&self, face_data: &[u8], glyph_id: u16, px_size: f32) -> RasterOutput {
        // Attempt to construct a FontRef from the raw bytes.
        let Some(font_ref) = FontRef::from_index(face_data, 0) else {
            return zero_output();
        };

        // Retrieve advance before acquiring the mutex so we release the borrow
        // on `face_data` / `font_ref` before the mutable context lock.
        let advance_x = glyph_advance(font_ref, glyph_id, px_size);

        // Lock the ScaleContext.
        let mut ctx = match self.context.lock() {
            Ok(g) => g,
            Err(_) => return zero_output(),
        };

        // Build a scaler for the requested size and hinting mode.
        //
        // `font_ref` borrows `face_data`; the lifetime is tied to this scope
        // so we rebuild it inside the lock to keep borrow scopes simple.
        let Some(fr) = FontRef::from_index(face_data, 0) else {
            return zero_output();
        };
        let mut scaler = ctx.builder(fr).size(px_size).hint(self.hint).build();

        // Render: prefer outline (supports hinting), fall back to an alpha
        // bitmap strike only if no outline is available.
        let mut render = Render::new(&[Source::ColorBitmap(StrikeWith::BestFit), Source::Outline]);
        render.format(swash::zeno::Format::Alpha);

        let Some(image) = render.render(&mut scaler, glyph_id) else {
            // No outline and no suitable bitmap strike — return zero-sized output
            // with correct advance so that callers can still advance the pen.
            return RasterOutput {
                width: 0,
                height: 0,
                coverage: Vec::new(),
                advance_x,
                advance_y: 0.0,
                bearing_x: 0,
                bearing_y: 0,
            };
        };

        let p = image.placement;

        // A zero-area image still carries placement data; normalise to empty.
        if p.width == 0 || p.height == 0 {
            return RasterOutput {
                width: 0,
                height: 0,
                coverage: Vec::new(),
                advance_x,
                advance_y: 0.0,
                bearing_x: p.left,
                bearing_y: p.top,
            };
        }

        RasterOutput {
            width: p.width as usize,
            height: p.height as usize,
            coverage: image.data,
            advance_x,
            advance_y: 0.0,
            bearing_x: p.left,
            bearing_y: p.top,
        }
    }

    /// Rasterize a color glyph, returning an RGBA [`oxitext_core::ColorBitmap`].
    ///
    /// Attempts `Source::ColorBitmap(StrikeWith::BestFit)` first; if the glyph
    /// is not a color bitmap glyph this returns `None`.  Regular (outline)
    /// glyphs should be rendered via [`Self::rasterize`] instead.
    fn rasterize_color(
        &self,
        face_data: &[u8],
        glyph_id: u16,
        px_size: f32,
    ) -> Option<oxitext_core::ColorBitmap> {
        let mut ctx = self.context.lock().ok()?;
        let fr = FontRef::from_index(face_data, 0)?;
        let mut scaler = ctx.builder(fr).size(px_size).build();

        let render = Render::new(&[Source::ColorBitmap(StrikeWith::BestFit)]);

        let image = render.render(&mut scaler, glyph_id)?;

        // Only expose color-bitmap results; plain alpha masks are handled by
        // `rasterize()`.  swash reports color bitmaps as `Content::Color`.
        if image.content != swash::scale::image::Content::Color {
            return None;
        }

        let p = image.placement;
        if p.width == 0 || p.height == 0 {
            return None;
        }

        // `image.data` is RGBA for color bitmaps.
        Some(oxitext_core::ColorBitmap {
            width: p.width,
            height: p.height,
            rgba: image.data,
        })
    }
}

/// Returns a zero-area [`RasterOutput`] suitable as an error / whitespace result.
#[inline]
fn zero_output() -> RasterOutput {
    RasterOutput {
        width: 0,
        height: 0,
        coverage: Vec::new(),
        advance_x: 0.0,
        advance_y: 0.0,
        bearing_x: 0,
        bearing_y: 0,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::RasterBackend;
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
        panic!("no test font found — add tests/fixtures/test-font.ttf");
    }

    #[test]
    fn swash_raster_hinted_produces_bitmap() {
        let font = load_test_font();
        let raster = SwashRaster::new();
        // GID 36 is a visible printable glyph in NotoSans
        let out = raster.rasterize(&font, 36, 16.0);
        // Should have non-zero dimensions for a visible glyph
        assert!(
            out.width > 0 && out.height > 0,
            "expected non-zero bitmap for GID 36 (hinted), got {}x{}",
            out.width,
            out.height
        );
        // Coverage buffer must match declared dimensions.
        assert_eq!(
            out.coverage.len(),
            out.width * out.height,
            "coverage buffer length mismatch"
        );
        // At least one non-zero pixel in the coverage.
        assert!(
            out.coverage.iter().any(|&v| v > 0),
            "coverage bitmap is all-zeros for GID 36"
        );
    }

    #[test]
    fn swash_raster_unhinted_produces_bitmap() {
        let font = load_test_font();
        let raster = SwashRaster::with_hint(false);
        let out = raster.rasterize(&font, 36, 16.0);
        // Unhinted should still produce a valid bitmap
        assert!(
            out.width > 0 && out.height > 0,
            "expected non-zero bitmap for GID 36 (unhinted)"
        );
        assert_eq!(out.coverage.len(), out.width * out.height);
    }

    #[test]
    fn swash_raster_hinted_vs_unhinted_same_dimensions() {
        // Hinting may grid-fit the outline, but resulting dimensions should
        // stay within 2 pixels of the unhinted rasterization.
        let font = load_test_font();
        let hinted = SwashRaster::new();
        let unhinted = SwashRaster::with_hint(false);
        let bm_h = hinted.rasterize(&font, 36, 16.0);
        let bm_u = unhinted.rasterize(&font, 36, 16.0);
        assert!(
            (bm_h.width as i64 - bm_u.width as i64).abs() <= 2,
            "width differs too much: hinted={}, unhinted={}",
            bm_h.width,
            bm_u.width
        );
        assert!(
            (bm_h.height as i64 - bm_u.height as i64).abs() <= 2,
            "height differs too much: hinted={}, unhinted={}",
            bm_h.height,
            bm_u.height
        );
    }

    #[test]
    fn swash_raster_invalid_font_returns_zero_sized() {
        let raster = SwashRaster::new();
        let out = raster.rasterize(b"not a font", 36, 16.0);
        // Invalid font: must return zero-sized, not panic
        assert_eq!(
            out.width, 0,
            "invalid font should produce zero-width bitmap"
        );
        assert_eq!(
            out.height, 0,
            "invalid font should produce zero-height bitmap"
        );
        assert!(out.coverage.is_empty());
    }

    #[test]
    fn swash_raster_whitespace_glyph_handles_gracefully() {
        // GID 3 is typically space in many fonts — it has no outline.
        let font = load_test_font();
        let raster = SwashRaster::new();
        let out = raster.rasterize(&font, 3, 16.0);
        // Space character has zero coverage — either zero-sized bitmap or
        // all-zero coverage.
        if out.width > 0 && out.height > 0 {
            assert!(
                out.coverage.iter().all(|&p| p == 0),
                "space glyph should have all-zero coverage"
            );
        }
        // Must not panic — just return gracefully.
    }

    #[test]
    fn swash_raster_default_is_hinted() {
        // SwashRaster::default() must equal SwashRaster::new() (hint = true).
        let font = load_test_font();
        let a = SwashRaster::default();
        let b = SwashRaster::new();
        let out_a = a.rasterize(&font, 36, 16.0);
        let out_b = b.rasterize(&font, 36, 16.0);
        assert_eq!(out_a.width, out_b.width);
        assert_eq!(out_a.height, out_b.height);
        assert_eq!(out_a.coverage, out_b.coverage);
    }

    #[test]
    fn swash_raster_coverage_buffer_length_consistent() {
        // For multiple GIDs the coverage buffer length must always equal
        // width * height.
        let font = load_test_font();
        let raster = SwashRaster::new();
        for gid in 30u16..50 {
            let out = raster.rasterize(&font, gid, 16.0);
            assert_eq!(
                out.coverage.len(),
                out.width * out.height,
                "coverage length mismatch for GID {gid}"
            );
        }
    }

    #[test]
    fn swash_raster_larger_size_larger_bitmap() {
        // The hinted bitmap at 32px should generally be larger than at 8px.
        let font = load_test_font();
        let raster = SwashRaster::new();
        let small = raster.rasterize(&font, 36, 8.0);
        let large = raster.rasterize(&font, 36, 32.0);
        // Both should be valid.
        if small.width > 0 && large.width > 0 {
            assert!(
                large.width >= small.width,
                "32px should be wider than 8px: large={}, small={}",
                large.width,
                small.width
            );
        }
    }
}
