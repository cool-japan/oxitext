//! Swappable glyph rasterization backends.
//!
//! Defines the [`RasterBackend`] trait so that consumers can choose between
//! the default fontdue backend and the optional `ab_glyph` backend without
//! changing any other part of the pipeline.

/// Output produced by a [`RasterBackend`] rasterization call.
#[derive(Debug, Clone)]
pub struct RasterOutput {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Greyscale coverage, one byte per pixel (`width × height` bytes).
    pub coverage: Vec<u8>,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
    /// Vertical advance in pixels (usually `0.0` for LTR fonts).
    pub advance_y: f32,
    /// Left bearing in pixels (signed).
    pub bearing_x: i32,
    /// Top bearing in pixels (signed).
    pub bearing_y: i32,
}

/// Trait for swappable glyph rasterization backends.
///
/// Implementors must be `Send + Sync` so they can be shared across threads.
pub trait RasterBackend: Send + Sync {
    /// Rasterize glyph `glyph_id` from `face_data` at `px_size` pixels-per-em.
    ///
    /// Returns a [`RasterOutput`] containing the grayscale coverage bitmap and
    /// layout metrics. A zero-sized output (`width == 0` or `height == 0`)
    /// represents a non-visible glyph (e.g. whitespace).
    fn rasterize(&self, face_data: &[u8], glyph_id: u16, px_size: f32) -> RasterOutput;

    /// Rasterize glyph `glyph_id` with horizontal LCD subpixel rendering.
    ///
    /// The default implementation returns `None`; backends that support LCD
    /// rendering (e.g. [`AbGlyphRaster`]) override this method.
    ///
    /// [`AbGlyphRaster`]: struct.AbGlyphRaster.html
    fn rasterize_lcd(
        &self,
        _face_data: &[u8],
        _glyph_id: u16,
        _px_size: f32,
    ) -> Option<oxitext_core::LcdBitmap> {
        None
    }

    /// Rasterize glyph `glyph_id` and return a [`crate::result::RasterResult`]
    /// combining the coverage bitmap with layout metrics.
    ///
    /// The default implementation delegates to [`Self::rasterize`] and wraps
    /// the result; backends may override this for more efficient metric
    /// extraction.
    fn rasterize_full(
        &self,
        face_data: &[u8],
        glyph_id: u16,
        px_size: f32,
    ) -> crate::result::RasterResult {
        let out = self.rasterize(face_data, glyph_id, px_size);
        crate::result::RasterResult {
            output: oxitext_core::RenderOutput::Greyscale(oxitext_core::Bitmap {
                width: out.width as u32,
                height: out.height as u32,
                pixels: out.coverage,
            }),
            advance_x: out.advance_x,
            advance_y: out.advance_y,
            bearing_x: out.bearing_x,
            bearing_y: out.bearing_y,
        }
    }

    /// Rasterize a color glyph, returning an RGBA [`oxitext_core::ColorBitmap`].
    ///
    /// The default implementation returns `None`; backends with color-font
    /// support (COLR, CBDT, sbix, SVG) override this method.
    fn rasterize_color(
        &self,
        _face_data: &[u8],
        _glyph_id: u16,
        _px_size: f32,
    ) -> Option<oxitext_core::ColorBitmap> {
        None
    }

    /// Clear any internally cached font data or glyph bitmaps.
    ///
    /// The default implementation is a no-op, appropriate for backends that do
    /// not maintain a cache.  Backends with an internal cache (e.g.
    /// [`FontdueRaster`]) override this to release memory.
    fn clear_cache(&self) {}

    /// Rasterize glyph `glyph_id` and return the greyscale coverage bitmap
    /// suitable as input for SDF (Signed Distance Field) generation.
    ///
    /// Equivalent to [`Self::rasterize`] but communicates the intended use
    /// case: the caller intends to pass the returned bitmap into an EDT-based
    /// SDF generator (e.g. `oxitext-sdf`).  The returned bitmap contains
    /// greyscale coverage values where `0` is fully transparent and `255` is
    /// fully opaque.
    ///
    /// Returns `Some(bitmap)` for visible glyphs (non-zero dimensions) and
    /// `None` for whitespace or other non-visible glyphs.
    fn rasterize_for_sdf(
        &self,
        face_data: &[u8],
        glyph_id: u16,
        px_size: f32,
    ) -> Option<oxitext_core::Bitmap> {
        let out = self.rasterize(face_data, glyph_id, px_size);
        if out.width == 0 || out.height == 0 {
            return None;
        }
        Some(oxitext_core::Bitmap {
            width: out.width as u32,
            height: out.height as u32,
            pixels: out.coverage,
        })
    }
}

/// Default capacity for the per-instance LRU font cache.
const DEFAULT_FONT_CACHE_CAP: usize = 64;

/// The default fontdue-based rasterizer.
///
/// Caches parsed [`fontdue::Font`] instances keyed on the pointer identity of
/// `face_data` (callers should pass the same slice address across calls).
/// The internal cache is bounded by 64 entries using
/// [`lru::LruCache`], so memory usage is capped regardless of how many distinct
/// font byte-slices are ever seen.
///
/// The cache is protected by a [`std::sync::Mutex`] rather than an `RwLock`
/// because `LruCache::get` requires a mutable reference to update recency
/// order, making a shared read-path impossible without defeating LRU semantics.
pub struct FontdueRaster {
    cache: std::sync::Mutex<lru::LruCache<usize, fontdue::Font>>,
}

impl FontdueRaster {
    /// Creates a new [`FontdueRaster`] with an empty, capacity-bounded font cache.
    pub fn new() -> Self {
        // SAFETY: 64 is a non-zero compile-time constant.
        let cap = std::num::NonZeroUsize::new(DEFAULT_FONT_CACHE_CAP)
            .expect("DEFAULT_FONT_CACHE_CAP is non-zero");
        Self {
            cache: std::sync::Mutex::new(lru::LruCache::new(cap)),
        }
    }
}

impl Default for FontdueRaster {
    fn default() -> Self {
        Self::new()
    }
}

impl FontdueRaster {
    /// Rasterize `glyph_id` at a specific fractional pen position.
    ///
    /// `subpixel_x` and `subpixel_y` are fractional offsets in `[0.0, 1.0)`.
    /// They represent the sub-pixel position of the glyph origin within the
    /// destination pixel grid — two calls with the same glyph and size but
    /// different offsets produce bitmaps with subtly different coverage near
    /// the glyph edges.
    ///
    /// # Implementation
    /// fontdue itself does not expose a subpixel-positioned rasterization
    /// API — it always places the glyph at integer coordinates. This method
    /// therefore renders through [`crate::subpixel::rasterize_positioned_bitmap`]
    /// (`ab_glyph`, which accepts a fractional pen position and re-samples the
    /// outline at that exact offset) instead, and only falls back to the
    /// grid-fitted fontdue path — with the offsets having no effect, as
    /// before — when `ab_glyph` cannot parse `face_data` or the glyph has no
    /// outline. `ab_glyph` is an unconditional dependency of this crate, so
    /// this fallback is the only case where fractional positioning is lost.
    ///
    /// Note that the returned bitmap's `width`/`height` come from `ab_glyph`'s
    /// own outline bounds when that path succeeds, which need not exactly
    /// match the dimensions [`Self::rasterize`] (fontdue) would report for the
    /// same glyph at offset `(0.0, 0.0)` — both are valid anti-aliased
    /// rasterizations of the same outline, not byte-identical bitmaps.
    pub fn raster_positioned(
        &self,
        face_data: &[u8],
        glyph_id: u16,
        px_size: f32,
        subpixel_x: f32,
        subpixel_y: f32,
    ) -> Option<oxitext_core::Bitmap> {
        if let Some(bitmap) = crate::subpixel::rasterize_positioned_bitmap(
            face_data, glyph_id, px_size, subpixel_x, subpixel_y,
        ) {
            return Some(bitmap);
        }

        // Fallback: face_data didn't parse under ab_glyph, or the glyph has no
        // outline there either. Render through the grid-fitted fontdue path so
        // callers still get a usable bitmap; the offsets have no effect here,
        // matching this method's pre-subpixel-support behaviour.
        let out = self.rasterize(face_data, glyph_id, px_size);
        if out.width == 0 && out.height == 0 {
            // Whitespace or unparseable font — return empty bitmap rather than None
            // so callers can distinguish "no outline" from "error".
            return Some(oxitext_core::Bitmap {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            });
        }
        Some(oxitext_core::Bitmap {
            width: out.width as u32,
            height: out.height as u32,
            pixels: out.coverage,
        })
    }
}

impl RasterBackend for FontdueRaster {
    fn rasterize(&self, face_data: &[u8], glyph_id: u16, px_size: f32) -> RasterOutput {
        // Fast path: try the thread-local cache first to avoid any lock contention.
        if let Some(font) = crate::tl_cache::get_or_parse_fontdue(face_data) {
            let (metrics, coverage) = font.rasterize_indexed(glyph_id, px_size);
            return RasterOutput {
                width: metrics.width,
                height: metrics.height,
                coverage,
                advance_x: metrics.advance_width,
                advance_y: metrics.advance_height,
                bearing_x: metrics.xmin,
                bearing_y: metrics.ymin,
            };
        }

        // Fallback: global Mutex-protected LRU cache.
        let key = face_data.as_ptr() as usize;

        let mut guard = match self.cache.lock() {
            Ok(g) => g,
            Err(_) => {
                return RasterOutput {
                    width: 0,
                    height: 0,
                    coverage: Vec::new(),
                    advance_x: 0.0,
                    advance_y: 0.0,
                    bearing_x: 0,
                    bearing_y: 0,
                };
            }
        };

        // Insert if not present (LruCache has no entry API).
        if !guard.contains(&key) {
            let font = match fontdue::Font::from_bytes(face_data, fontdue::FontSettings::default())
            {
                Ok(f) => f,
                Err(_) => {
                    return RasterOutput {
                        width: 0,
                        height: 0,
                        coverage: Vec::new(),
                        advance_x: 0.0,
                        advance_y: 0.0,
                        bearing_x: 0,
                        bearing_y: 0,
                    };
                }
            };
            guard.put(key, font);
        }

        match guard.get(&key) {
            Some(font) => {
                let (metrics, coverage) = font.rasterize_indexed(glyph_id, px_size);
                RasterOutput {
                    width: metrics.width,
                    height: metrics.height,
                    coverage,
                    advance_x: metrics.advance_width,
                    advance_y: metrics.advance_height,
                    bearing_x: metrics.xmin,
                    bearing_y: metrics.ymin,
                }
            }
            None => RasterOutput {
                width: 0,
                height: 0,
                coverage: Vec::new(),
                advance_x: 0.0,
                advance_y: 0.0,
                bearing_x: 0,
                bearing_y: 0,
            },
        }
    }

    fn clear_cache(&self) {
        if let Ok(mut g) = self.cache.lock() {
            g.clear();
        }
    }
}

/// Alternative rasterizer backed by [`ab_glyph`].
///
/// Enabled by the `ab-glyph-backend` feature.
#[cfg(feature = "ab-glyph-backend")]
pub struct AbGlyphRaster;

#[cfg(feature = "ab-glyph-backend")]
impl Default for AbGlyphRaster {
    fn default() -> Self {
        Self
    }
}

#[cfg(feature = "ab-glyph-backend")]
impl RasterBackend for AbGlyphRaster {
    fn rasterize_lcd(
        &self,
        face_data: &[u8],
        glyph_id: u16,
        px_size: f32,
    ) -> Option<oxitext_core::LcdBitmap> {
        crate::lcd::rasterize_lcd(
            face_data,
            glyph_id,
            px_size,
            crate::options::LcdFilterKernel::FreeType5Tap,
            false,
        )
    }

    fn rasterize(&self, face_data: &[u8], glyph_id: u16, px_size: f32) -> RasterOutput {
        use ab_glyph::{Font, FontRef, GlyphId as AbGlyphId, PxScale, ScaleFont};

        let font = match FontRef::try_from_slice(face_data) {
            Ok(f) => f,
            Err(_) => {
                return RasterOutput {
                    width: 0,
                    height: 0,
                    coverage: Vec::new(),
                    advance_x: 0.0,
                    advance_y: 0.0,
                    bearing_x: 0,
                    bearing_y: 0,
                };
            }
        };

        let scale = PxScale::from(px_size);
        let scaled = font.as_scaled(scale);
        let ab_gid = AbGlyphId(glyph_id);
        let advance_x = scaled.h_advance(ab_gid);
        let advance_y = 0.0_f32;
        let h_bearing = scaled.h_side_bearing(ab_gid);

        // Create a positioned glyph at the origin with the requested subpixel offset.
        let glyph = ab_gid.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));

        let outlined = match font.outline_glyph(glyph) {
            Some(og) => og,
            None => {
                return RasterOutput {
                    width: 0,
                    height: 0,
                    coverage: Vec::new(),
                    advance_x,
                    advance_y,
                    bearing_x: h_bearing as i32,
                    bearing_y: 0,
                };
            }
        };

        let bounds = outlined.px_bounds();
        let w = bounds.width().ceil() as usize;
        let h = bounds.height().ceil() as usize;
        if w == 0 || h == 0 {
            return RasterOutput {
                width: 0,
                height: 0,
                coverage: Vec::new(),
                advance_x,
                advance_y,
                bearing_x: h_bearing as i32,
                bearing_y: 0,
            };
        }

        let mut coverage = vec![0u8; w * h];
        outlined.draw(|x, y, c| {
            let idx = y as usize * w + x as usize;
            if idx < coverage.len() {
                coverage[idx] = (c * 255.0).round() as u8;
            }
        });

        RasterOutput {
            width: w,
            height: h,
            coverage,
            advance_x,
            advance_y,
            bearing_x: bounds.min.x as i32,
            bearing_y: bounds.min.y as i32,
        }
    }
}
