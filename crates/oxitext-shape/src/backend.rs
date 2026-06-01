//! Swappable text shaping backends.
//!
//! Defines the [`ShapeBackend`] trait so consumers can choose between the
//! default swash-based backend and the optional rustybuzz backend without
//! changing any other part of the pipeline.

use oxitext_core::ShapedGlyph;
use std::sync::Arc;

/// Trait for swappable text shaping backends.
///
/// Implementors must be `Send + Sync` so they can be shared across threads.
///
/// All methods receive `face_data: &Arc<[u8]>` rather than `&[u8]` so that the
/// pointer address is preserved across calls.  This allows [`crate::ShapeCache`]
/// (keyed on `Arc::as_ptr`) to produce cache hits when the same allocation is
/// reused.
pub trait ShapeBackend: Send + Sync {
    /// Shape UTF-8 `text` using font bytes `face_data` at `px_size`
    /// pixels-per-em, returning one [`ShapedGlyph`] per output glyph.
    fn shape(&self, face_data: &Arc<[u8]>, text: &str, px_size: f32) -> Vec<ShapedGlyph>;

    /// Shape UTF-8 `text` with an explicit direction hint.
    ///
    /// When `rtl` is `false` this is identical to [`Self::shape`].
    ///
    /// When `rtl` is `true` the backend shapes in the right-to-left direction
    /// and returns glyphs sorted in **ascending `cluster` order** (logical
    /// source order).  Visual reordering is the caller's responsibility.
    ///
    /// Backends that do not override this method fall back to [`Self::shape`]
    /// for LTR and perform a post-sort for RTL, which is correct but may not
    /// select the best glyph forms for bidirectional scripts.
    fn shape_with_direction(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        rtl: bool,
    ) -> Vec<ShapedGlyph> {
        if !rtl {
            return self.shape(face_data, text, px_size);
        }
        let mut glyphs = self.shape(face_data, text, px_size);
        glyphs.sort_by_key(|g| g.cluster);
        glyphs
    }

    /// Shape UTF-8 `text` with an explicit set of OpenType feature overrides.
    ///
    /// The default implementation ignores the `features` slice and delegates
    /// to [`Self::shape`].  Backends that support OpenType feature control
    /// should override this method to apply the requested features.
    fn shape_with_features(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        features: &[crate::ShapeFeature],
    ) -> Vec<ShapedGlyph> {
        let _ = features;
        self.shape(face_data, text, px_size)
    }

    /// Shape text with extended options.
    ///
    /// The default implementation delegates to [`Self::shape_with_features`]
    /// when `features` is non-empty, or [`Self::shape`] otherwise, ignoring
    /// any options not covered by those methods.  Backends that need to honour
    /// additional fields from `options` (script, language, direction, etc.)
    /// should override this method.
    fn shape_with_options(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        rtl: bool,
        features: &[crate::ShapeFeature],
        _options: &crate::ShapeRequest<'_>,
    ) -> Vec<ShapedGlyph> {
        if features.is_empty() {
            self.shape_with_direction(face_data, text, px_size, rtl)
        } else {
            let mut glyphs = self.shape_with_features(face_data, text, px_size, features);
            if rtl {
                glyphs.sort_by_key(|g| g.cluster);
            }
            glyphs
        }
    }

    /// Check if the font has shaping support for a given OpenType script tag.
    ///
    /// Returns `true` if the font likely covers the script, `false` if a
    /// sentinel character from the script's Unicode range returns no glyph.
    /// Unknown scripts (not in the built-in table) return `true` by default
    /// so callers that do not pass script tags are not affected.
    fn supports_script(&self, font_data: &Arc<[u8]>, script: [u8; 4]) -> bool {
        /// Sentinel characters to check per well-known script tag.
        fn sentinel_char(script: [u8; 4]) -> Option<char> {
            match &script {
                b"latn" => Some('A'),
                b"arab" => Some('\u{0627}'), // Arabic letter Alef
                b"hani" => Some('\u{4E00}'), // CJK ideograph
                b"cyrl" => Some('\u{0410}'), // Cyrillic capital A
                b"grek" => Some('\u{0391}'), // Greek capital Alpha
                b"hebr" => Some('\u{05D0}'), // Hebrew Alef
                b"deva" => Some('\u{0905}'), // Devanagari A
                b"thai" => Some('\u{0E01}'), // Thai Ko Kai
                _ => None,
            }
        }

        let Some(ch) = sentinel_char(script) else {
            // Unknown script — permissive default.
            return true;
        };

        ttf_parser::Face::parse(font_data.as_ref(), 0)
            .map(|face| face.glyph_index(ch).is_some())
            .unwrap_or(true) // If parsing fails, assume support.
    }
}

/// Swash-based shaper — delegates to [`crate::SwashShaper`].
///
/// This adapter wraps the existing [`crate::SwashShaper`] (which has its own
/// internal LRU cache) and exposes it via the [`ShapeBackend`] trait.
///
/// Uses [`std::sync::RwLock`] instead of `Mutex` so that read-only operations
/// (e.g. [`ShapeBackend::supports_script`]) can proceed concurrently.  Shape
/// calls still acquire the write lock because the underlying [`crate::SwashShaper`]
/// mutates its internal context on every call.
pub struct SwashShaperBackend {
    inner: std::sync::Arc<std::sync::RwLock<crate::SwashShaper>>,
}

impl SwashShaperBackend {
    /// Creates a new [`SwashShaperBackend`].
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::RwLock::new(crate::SwashShaper::new())),
        }
    }
}

impl Default for SwashShaperBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeBackend for SwashShaperBackend {
    fn shape(&self, face_data: &Arc<[u8]>, text: &str, px_size: f32) -> Vec<ShapedGlyph> {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        match guard.shape(text, Arc::clone(face_data), px_size) {
            Ok(run) => run.glyphs.into_vec(),
            Err(_) => Vec::new(),
        }
    }

    fn shape_with_direction(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        rtl: bool,
    ) -> Vec<ShapedGlyph> {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        match guard.shape_with_direction(text, Arc::clone(face_data), px_size, rtl) {
            Ok(run) => run.glyphs.into_vec(),
            Err(_) => Vec::new(),
        }
    }
}

/// rustybuzz-based shaper.
///
/// Enabled by the `rustybuzz-backend` feature.
#[cfg(feature = "rustybuzz-backend")]
pub struct RustybuzzShaper;

#[cfg(feature = "rustybuzz-backend")]
impl Default for RustybuzzShaper {
    fn default() -> Self {
        Self
    }
}

#[cfg(feature = "rustybuzz-backend")]
impl RustybuzzShaper {
    /// Internal helper: shapes `text` into glyphs using the given rustybuzz direction.
    ///
    /// Returns glyphs sorted to ascending cluster (logical source) order.
    fn shape_internal(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        direction: rustybuzz::Direction,
    ) -> Vec<ShapedGlyph> {
        use rustybuzz::{Face, UnicodeBuffer};

        let face = match Face::from_slice(face_data.as_ref(), 0) {
            Some(f) => f,
            None => return Vec::new(),
        };

        let upem = face.units_per_em() as f32;
        let scale = if upem > 0.0 { px_size / upem } else { 1.0 };

        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        buf.set_direction(direction);

        let shaped = rustybuzz::shape(&face, &[], buf);
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();

        let mut glyphs: Vec<ShapedGlyph> = infos
            .iter()
            .zip(positions.iter())
            .map(|(info, pos)| {
                let is_ws = text
                    .get(info.cluster as usize..)
                    .and_then(|s| s.chars().next())
                    .map(|c| c.is_whitespace())
                    .unwrap_or(false);
                ShapedGlyph {
                    gid: info.glyph_id as u16,
                    cluster: info.cluster,
                    x_advance: pos.x_advance as f32 * scale,
                    y_advance: pos.y_advance as f32 * scale,
                    x_offset: pos.x_offset as f32 * scale,
                    y_offset: pos.y_offset as f32 * scale,
                    is_whitespace: is_ws,
                    // rustybuzz exposes UNSAFE_TO_BREAK in glyph flags; we
                    // conservatively leave it false here (single-pass shaping).
                    unsafe_to_break: false,
                }
            })
            .collect();

        // Guarantee ascending cluster (logical source) order regardless of
        // what rustybuzz emits for RTL runs.
        glyphs.sort_by_key(|g| g.cluster);
        glyphs
    }
}

#[cfg(feature = "rustybuzz-backend")]
impl ShapeBackend for RustybuzzShaper {
    fn shape(&self, face_data: &Arc<[u8]>, text: &str, px_size: f32) -> Vec<ShapedGlyph> {
        self.shape_internal(face_data, text, px_size, rustybuzz::Direction::LeftToRight)
    }

    fn shape_with_direction(
        &self,
        face_data: &Arc<[u8]>,
        text: &str,
        px_size: f32,
        rtl: bool,
    ) -> Vec<ShapedGlyph> {
        let direction = if rtl {
            rustybuzz::Direction::RightToLeft
        } else {
            rustybuzz::Direction::LeftToRight
        };
        self.shape_internal(face_data, text, px_size, direction)
    }
}
