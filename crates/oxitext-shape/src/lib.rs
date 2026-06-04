#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! `oxitext-shape` — Swash-based text shaper for OxiText.
//!
//! Provides [`SwashShaper`], which wraps swash's [`ShapeContext`] and produces
//! [`ShapedRun`]s from UTF-8 text + raw font bytes.
//!
//! M1: LTR Latin shaping. Bidi (M2) and script-specific itemisation (M3) are
//! deferred.
//!
//! # M3 additions
//!
//! - [`backend`]: Swappable [`backend::ShapeBackend`] trait, with the default
//!   [`backend::SwashShaperBackend`] wrapper and optional
//!   [`backend::RustybuzzShaper`] (feature `rustybuzz-backend`).
//!
//! # M5 additions (Slice 5a)
//!
//! - [`cache`]: Bounded LRU shape cache ([`cache::ShapeCache`],
//!   [`cache::ShapeKey`]) backed by [`lru::LruCache`].
//! - [`SwashShaper::with_cache`]: creates a `SwashShaper` with an attached
//!   `ShapeCache`; subsequent `shape()` calls check the cache before invoking
//!   swash.
//!
//! # Feature-aware shaping (Slice 6)
//!
//! - [`ShapeFeature`]: an OpenType feature tag-value pair.
//! - [`ShapeDirection`]: direction enum (Ltr/Rtl/Ttb/Btt).
//! - [`ShapeRequest`] / [`ShapeRequestBuilder`]: builder pattern for a full
//!   shaping request including text, font, size, direction, script, language,
//!   and a list of [`ShapeFeature`]s.
//! - [`SwashShaper::shape_request`]: shapes a complete [`ShapeRequest`], with
//!   automatic `vert`/`vrt2` feature injection for top-to-bottom text.
//! - [`SwashShaper::shape_with_features`]: lower-level entry point that
//!   accepts a feature slice directly.

pub mod backend;
pub mod batch;
pub mod cache;
pub mod script_detect;
pub mod variational;

#[cfg(feature = "system-fonts")]
pub mod system_fonts;
#[cfg(feature = "system-fonts")]
pub use system_fonts::{
    build_system_db, load_best_font_for_text, load_best_font_for_text_from, load_font_for_family,
    load_font_for_family_from,
};

/// Native OS font fallback for complex script coverage.
///
/// When the `native-fallback` Cargo feature is enabled, this module re-exports
/// the [`oxifont_adapter_native::shaper_bridge`] API, allowing shaping engines
/// to resolve Unicode codepoints to OS-native font bytes (CoreText on macOS,
/// DirectWrite on Windows, pure filesystem scan on Linux).
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "native-fallback")]
/// # {
/// use oxitext_shape::native_fallback;
///
/// let primary = std::fs::read("NotoSans-Regular.ttf").unwrap();
/// // For Arabic/Hebrew/CJK text that NotoSans may not cover:
/// let fallbacks = native_fallback::collect_fallback_fonts_for_text("مرحبا", &primary);
/// println!("{} fallback font(s) provided", fallbacks.len());
/// # }
/// ```
#[cfg(feature = "native-fallback")]
pub mod native_fallback {
    pub use oxifont_adapter_native::shaper_bridge::{
        collect_fallback_fonts_for_text, collect_fonts_for_text, find_native_font_for_codepoint,
        load_best_native_font_for_text, load_native_font_for_codepoint_with_index,
    };
}

#[cfg(feature = "rustybuzz-backend")]
pub use backend::RustybuzzShaper;
pub use backend::ShapeBackend;
pub use backend::SwashShaperBackend;
pub use cache::{FontId, ShapeCache, ShapeKey};
use oxitext_core::{OxiTextError, ShapedGlyph, ShapedRun};
pub use script_detect::{
    requires_arabic_shaping, requires_indic_shaping, requires_mark_positioning,
};
use smallvec::SmallVec;
use std::sync::Arc;
use swash::shape::{Direction, ShapeContext};
use swash::FontRef;
// ──────────────────────────────────────────────────────────────────────────────
// ShapeFeature
// ──────────────────────────────────────────────────────────────────────────────

/// An OpenType feature tag-value pair.
///
/// The `tag` is a 4-byte ASCII identifier (e.g. `b"liga"`, `b"kern"`,
/// `b"smcp"`).  A `value` of `0` disables the feature, `1` enables it, and
/// values `>1` select an alternate index for features such as `salt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeFeature {
    /// 4-byte ASCII OpenType feature tag.
    pub tag: [u8; 4],
    /// Feature value: 0 = disable, 1 = enable, >1 = alternate index.
    pub value: u32,
}

impl ShapeFeature {
    /// Creates a new feature with an arbitrary value.
    pub const fn new(tag: [u8; 4], value: u32) -> Self {
        Self { tag, value }
    }

    /// Creates an enabled feature (`value = 1`).
    pub const fn enable(tag: [u8; 4]) -> Self {
        Self { tag, value: 1 }
    }

    /// Creates a disabled feature (`value = 0`).
    pub const fn disable(tag: [u8; 4]) -> Self {
        Self { tag, value: 0 }
    }

    /// Standard ligatures.
    pub const LIGA: Self = Self::enable(*b"liga");
    /// Kerning.
    pub const KERN: Self = Self::enable(*b"kern");
    /// Small capitals.
    pub const SMCP: Self = Self::enable(*b"smcp");
    /// Contextual alternates.
    pub const CALT: Self = Self::enable(*b"calt");
    /// Vertical forms (substitution of upright CJK glyphs with vertical ones).
    pub const VERT: Self = Self::enable(*b"vert");
    /// Vertical rotation (alternative to `vert` for some CJK contexts).
    pub const VRT2: Self = Self::enable(*b"vrt2");
}

// ──────────────────────────────────────────────────────────────────────────────
// ShapeDirection
// ──────────────────────────────────────────────────────────────────────────────

/// Text direction for a shaping request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeDirection {
    /// Left-to-right (default for Latin, Cyrillic, etc.).
    #[default]
    Ltr,
    /// Right-to-left (Arabic, Hebrew, etc.).
    Rtl,
    /// Top-to-bottom (CJK vertical text).
    Ttb,
    /// Bottom-to-top (rare).
    Btt,
}

// ──────────────────────────────────────────────────────────────────────────────
// ShapeRequest / ShapeRequestBuilder
// ──────────────────────────────────────────────────────────────────────────────

/// A complete shaping request with all parameters.
///
/// Build via [`ShapeRequest::builder`] and then call
/// [`SwashShaper::shape_request`].
#[derive(Debug, Clone)]
pub struct ShapeRequest<'a> {
    /// UTF-8 text to shape.
    pub text: &'a str,
    /// Raw font bytes.
    pub font_data: &'a [u8],
    /// Font size in pixels-per-em.
    pub px_size: f32,
    /// Shaping direction.
    pub direction: ShapeDirection,
    /// OpenType script tag (e.g. `b"latn"`, `b"arab"`), or `None` for
    /// auto-detection.
    pub script: Option<[u8; 4]>,
    /// OpenType language tag (e.g. `b"ENG "`, `b"ARA "`), or `None`.
    pub language: Option<[u8; 4]>,
    /// OpenType feature overrides.
    pub features: Vec<ShapeFeature>,
}

impl<'a> ShapeRequest<'a> {
    /// Returns a new [`ShapeRequestBuilder`].
    pub fn builder() -> ShapeRequestBuilder<'a> {
        ShapeRequestBuilder::default()
    }
}

/// Builder for [`ShapeRequest`].
#[derive(Debug, Default)]
pub struct ShapeRequestBuilder<'a> {
    text: Option<&'a str>,
    font_data: Option<&'a [u8]>,
    px_size: f32,
    direction: ShapeDirection,
    script: Option<[u8; 4]>,
    language: Option<[u8; 4]>,
    features: Vec<ShapeFeature>,
}

/// Errors that can occur when building a [`ShapeRequest`].
#[derive(Debug)]
pub enum ShapeRequestError {
    /// The `text` field was not provided.
    MissingText,
    /// The `font_data` field was not provided.
    MissingFont,
}

impl std::fmt::Display for ShapeRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShapeRequestError::MissingText => f.write_str("text not set"),
            ShapeRequestError::MissingFont => f.write_str("font_data not set"),
        }
    }
}

impl std::error::Error for ShapeRequestError {}

impl<'a> ShapeRequestBuilder<'a> {
    /// Sets the text to shape.
    pub fn text(mut self, t: &'a str) -> Self {
        self.text = Some(t);
        self
    }

    /// Sets the raw font bytes.
    pub fn font_data(mut self, d: &'a [u8]) -> Self {
        self.font_data = Some(d);
        self
    }

    /// Sets the font size in pixels-per-em.
    pub fn px_size(mut self, s: f32) -> Self {
        self.px_size = s;
        self
    }

    /// Sets the shaping direction.
    pub fn direction(mut self, d: ShapeDirection) -> Self {
        self.direction = d;
        self
    }

    /// Pins the OpenType script tag (overrides swash's auto-detection).
    pub fn script(mut self, tag: [u8; 4]) -> Self {
        self.script = Some(tag);
        self
    }

    /// Pins the OpenType language tag for language-specific GSUB/GPOS rules.
    pub fn language(mut self, tag: [u8; 4]) -> Self {
        self.language = Some(tag);
        self
    }

    /// Appends an OpenType feature override.
    pub fn feature(mut self, f: ShapeFeature) -> Self {
        self.features.push(f);
        self
    }

    /// Builds the [`ShapeRequest`].
    ///
    /// # Errors
    /// Returns [`ShapeRequestError::MissingText`] or
    /// [`ShapeRequestError::MissingFont`] if the respective fields were not
    /// provided.
    pub fn build(self) -> Result<ShapeRequest<'a>, ShapeRequestError> {
        Ok(ShapeRequest {
            text: self.text.ok_or(ShapeRequestError::MissingText)?,
            font_data: self.font_data.ok_or(ShapeRequestError::MissingFont)?,
            px_size: self.px_size,
            direction: self.direction,
            script: self.script,
            language: self.language,
            features: self.features,
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal parameter bundle used by shape_with_features_internal
// ──────────────────────────────────────────────────────────────────────────────

/// Internal parameter bundle for the unified shaping entry point.
///
/// Groups all shaping inputs into a single struct so `shape_with_features_internal`
/// stays under the clippy `too_many_arguments` threshold.
struct ShapeParams<'a> {
    font_data: &'a [u8],
    text: &'a str,
    px_size: f32,
    rtl: bool,
    script_tag: Option<[u8; 4]>,
    language_tag: Option<[u8; 4]>,
    features: &'a [ShapeFeature],
}

// ──────────────────────────────────────────────────────────────────────────────
// SwashShaper
// ──────────────────────────────────────────────────────────────────────────────

/// Text shaper backed by [swash].
///
/// Keep a single `SwashShaper` alive across multiple layout passes to amortise
/// the cost of the internal LRU caches that swash maintains in [`ShapeContext`].
///
/// Optionally attach a [`ShapeCache`] via [`Self::with_cache`] to skip swash
/// entirely on repeated requests for the same `(font, text, size)` tuple.
pub struct SwashShaper {
    ctx: ShapeContext,
    /// Optional application-level shape cache.
    cache: Option<Arc<ShapeCache>>,
    /// Cached text string for script-run reuse (Item 4).
    #[cfg(feature = "icu")]
    script_cache_text: String,
    /// Cached script runs for the cached text (Item 4).
    #[cfg(feature = "icu")]
    script_cache_runs: Vec<oxitext_icu::ScriptRun>,
}

impl SwashShaper {
    /// Creates a new shaper with default cache settings and no shape cache.
    pub fn new() -> Self {
        Self {
            ctx: ShapeContext::new(),
            cache: None,
            #[cfg(feature = "icu")]
            script_cache_text: String::new(),
            #[cfg(feature = "icu")]
            script_cache_runs: Vec::new(),
        }
    }

    /// Creates a new shaper with an attached [`ShapeCache`] of `capacity` entries.
    ///
    /// Repeated calls to [`Self::shape`] with the same `(font_data, text, size)`
    /// tuple will be served from the cache after the first miss.
    ///
    /// # Arguments
    /// - `capacity`: maximum number of [`ShapedRun`]s to keep in the cache.
    ///   Passing `0` uses a minimum capacity of 1.
    pub fn with_cache(capacity: usize) -> Self {
        Self {
            ctx: ShapeContext::new(),
            cache: Some(Arc::new(ShapeCache::new(capacity))),
            #[cfg(feature = "icu")]
            script_cache_text: String::new(),
            #[cfg(feature = "icu")]
            script_cache_runs: Vec::new(),
        }
    }

    /// Returns a reference to the attached shape cache, if any.
    pub fn shape_cache(&self) -> Option<&Arc<ShapeCache>> {
        self.cache.as_ref()
    }

    /// Shapes `text` using the font in `font_data` at `size` pixels-per-em.
    ///
    /// Returns a [`ShapedRun`] containing one [`ShapedGlyph`] per output glyph.
    /// The `x_advance` of each glyph is in pixels (already scaled by `size`).
    ///
    /// When an attached [`ShapeCache`] is present the result is looked up
    /// before invoking swash.  Cache keys incorporate `font_data` pointer
    /// identity, the exact text, and `size`.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed by
    /// swash.
    pub fn shape(
        &mut self,
        text: &str,
        font_data: Arc<[u8]>,
        size: f32,
    ) -> Result<ShapedRun, OxiTextError> {
        // Build a deterministic axis hash from the size (no variation axes yet).
        let axis_hash = size.to_bits() as u64;

        // Check cache if attached.
        if let Some(ref cache) = self.cache {
            let key = ShapeKey::new(&font_data, text, axis_hash);
            if let Some(cached) = cache.get(&key) {
                return Ok((*cached).clone());
            }
        }

        // Cache miss — invoke swash.
        let font = FontRef::from_index(&font_data, 0)
            .ok_or_else(|| OxiTextError::Shaping("swash could not parse font bytes".into()))?;

        let mut shaper = self.ctx.builder(font).size(size).build();
        shaper.add_str(text);

        let mut glyphs: SmallVec<[ShapedGlyph; 8]> = SmallVec::new();
        shaper.shape_with(|cluster| {
            // A cluster is whitespace if every source char it covers is
            // whitespace. Most whitespace clusters cover a single space/tab.
            let cluster_range = cluster.source.start as usize..cluster.source.end as usize;
            let is_ws = text
                .get(cluster_range)
                .map(|slice| !slice.is_empty() && slice.chars().all(|c| c.is_whitespace()))
                .unwrap_or(false);
            // More than one glyph in a cluster means inner glyphs are unsafe
            // to break before (ligature / mark attachment).
            let multi = cluster.glyphs.len() > 1;
            for (idx, glyph) in cluster.glyphs.iter().enumerate() {
                // A glyph is unsafe to break before if it is inside a
                // multi-glyph cluster (idx > 0) OR if it carries the mark
                // attachment flag (combining mark attached to a base glyph).
                let utb = (multi && idx > 0) || glyph.info.is_mark();
                glyphs.push(ShapedGlyph {
                    gid: glyph.id,
                    x_advance: glyph.advance,
                    y_advance: 0.0,
                    x_offset: glyph.x,
                    y_offset: glyph.y,
                    cluster: cluster.source.start,
                    is_whitespace: is_ws,
                    unsafe_to_break: utb,
                });
            }
        });

        let run = ShapedRun {
            glyphs,
            font_data: Arc::clone(&font_data),
        };

        // Populate cache on miss.
        if let Some(ref cache) = self.cache {
            let key = ShapeKey::new(&font_data, text, axis_hash);
            cache.insert(key, Arc::new(run.clone()));
        }

        Ok(run)
    }

    /// Shapes `text` with explicit direction control.
    ///
    /// When `rtl` is `false` this is identical to [`Self::shape`].
    ///
    /// When `rtl` is `true` the shaper signals `Direction::RightToLeft` to
    /// swash (enabling correct Arabic/Hebrew form selection via OpenType GSUB),
    /// then **sorts** the resulting glyphs by ascending `cluster` byte offset so
    /// the output is always in **logical source order** regardless of what swash
    /// emits.  The caller (bidi engine) is responsible for visual reordering.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_with_direction(
        &mut self,
        text: &str,
        font_data: Arc<[u8]>,
        size: f32,
        rtl: bool,
    ) -> Result<ShapedRun, OxiTextError> {
        if !rtl {
            return self.shape(text, font_data, size);
        }
        // RTL path: shape with the explicit RightToLeft hint so swash can apply
        // direction-sensitive GSUB lookups, then sort to ascending cluster order
        // (logical order) to satisfy the architecture contract.
        let mut run = self.do_shape_rtl(text, font_data, size)?;
        run.glyphs.sort_by_key(|g| g.cluster);
        Ok(run)
    }

    /// Shapes text using all parameters in a [`ShapeRequest`].
    ///
    /// When `direction` is [`ShapeDirection::Ttb`] or [`ShapeDirection::Btt`],
    /// the `vert` and `vrt2` OpenType features are **automatically appended**
    /// to the feature list (if not already present) so that fonts with a
    /// vertical substitution table produce the correct glyph variants.
    ///
    /// Script and language tags, if provided, are forwarded to swash's
    /// `ShaperBuilder` for language-specific GSUB/GPOS rule selection.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_request(
        &mut self,
        req: &ShapeRequest<'_>,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        // When the `icu` feature is enabled, normalize text to NFC before shaping
        // so that precomposed and decomposed spellings produce identical glyph runs.
        #[cfg(feature = "icu")]
        let normalized_text: String;
        #[cfg(feature = "icu")]
        let req_text: &str = {
            normalized_text = oxitext_icu::Normalizer::new().nfc(req.text);
            normalized_text.as_str()
        };
        #[cfg(not(feature = "icu"))]
        let req_text: &str = req.text;

        // When direction is Ltr but the text is Arabic, auto-upgrade to Rtl
        // so swash can apply the correct Arabic GSUB form-selection lookups.
        let effective_direction = if req.direction == ShapeDirection::Ltr
            && requires_arabic_shaping(req_text)
        {
            #[cfg(debug_assertions)]
            eprintln!("[oxitext-shape] Arabic text detected with Ltr direction; upgrading to Rtl");
            ShapeDirection::Rtl
        } else {
            req.direction
        };

        // Auto-inject vertical OpenType features for vertical directions.
        let mut features = req.features.clone();
        if effective_direction == ShapeDirection::Ttb || effective_direction == ShapeDirection::Btt
        {
            if !features.iter().any(|f| f.tag == *b"vert") {
                features.push(ShapeFeature::VERT);
            }
            if !features.iter().any(|f| f.tag == *b"vrt2") {
                features.push(ShapeFeature::VRT2);
            }
        }

        let rtl = effective_direction == ShapeDirection::Rtl;
        self.shape_with_features_internal(ShapeParams {
            font_data: req.font_data,
            text: req_text,
            px_size: req.px_size,
            rtl,
            script_tag: req.script,
            language_tag: req.language,
            features: &features,
        })
    }

    /// Shapes text with an explicit list of OpenType feature overrides.
    ///
    /// Unlike [`Self::shape_request`] this entry point does **not** inject
    /// vertical features automatically; callers are responsible for supplying
    /// the full feature list.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_with_features(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
        rtl: bool,
        features: &[ShapeFeature],
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        self.shape_with_features_internal(ShapeParams {
            font_data,
            text,
            px_size,
            rtl,
            script_tag: None,
            language_tag: None,
            features,
        })
    }

    /// Internal implementation shared by [`Self::shape_request`] and
    /// [`Self::shape_with_features`].
    fn shape_with_features_internal(
        &mut self,
        params: ShapeParams<'_>,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        use swash::tag_from_bytes;
        use swash::text::{Language, Script};

        let font = FontRef::from_index(params.font_data, 0)
            .ok_or_else(|| OxiTextError::Shaping("swash could not parse font bytes".into()))?;

        let direction = if params.rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };

        // Resolve the optional script tag to a swash Script enum value.
        let script = params
            .script_tag
            .and_then(|t| Script::from_opentype(tag_from_bytes(&t)))
            .unwrap_or(Script::Latin);

        // Resolve the optional language tag to a swash Language.
        let language = params.language_tag.and_then(|t| {
            // swash Language::parse expects a BCP-47 string; for OpenType tags
            // we convert the raw bytes to a lossy str and try to parse them.
            let s = std::str::from_utf8(&t).unwrap_or("").trim_end();
            Language::parse(s)
        });

        // Convert our ShapeFeature slice to swash-compatible (tag, value) pairs.
        // swash's `ShaperBuilder::features` accepts any iterator whose items
        // implement `Into<Setting<u16>>`.  The swash crate provides
        // `From<&([u8; 4], T)> for Setting<T>`, so we pass an iterator of
        // references to satisfy the bound.
        let swash_features: Vec<([u8; 4], u16)> = params
            .features
            .iter()
            .map(|f| (f.tag, f.value.min(u32::from(u16::MAX)) as u16))
            .collect();

        let mut shaper = self
            .ctx
            .builder(font)
            .size(params.px_size)
            .direction(direction)
            .script(script)
            .language(language)
            .features(swash_features.iter())
            .build();

        shaper.add_str(params.text);

        let mut glyphs: Vec<ShapedGlyph> = Vec::new();
        shaper.shape_with(|cluster| {
            let cluster_range = cluster.source.start as usize..cluster.source.end as usize;
            let is_ws = params
                .text
                .get(cluster_range)
                .map(|slice| !slice.is_empty() && slice.chars().all(|c| c.is_whitespace()))
                .unwrap_or(false);
            let multi = cluster.glyphs.len() > 1;
            for (idx, glyph) in cluster.glyphs.iter().enumerate() {
                let utb = (multi && idx > 0) || glyph.info.is_mark();
                glyphs.push(ShapedGlyph {
                    gid: glyph.id,
                    x_advance: glyph.advance,
                    y_advance: 0.0,
                    x_offset: glyph.x,
                    y_offset: glyph.y,
                    cluster: cluster.source.start,
                    is_whitespace: is_ws,
                    unsafe_to_break: utb,
                });
            }
        });

        if params.rtl {
            glyphs.sort_by_key(|g| g.cluster);
        }

        Ok(glyphs)
    }

    /// Internal RTL shaping path: invokes swash with `Direction::RightToLeft`.
    ///
    /// Returns glyphs in whatever order swash produces; the public
    /// [`Self::shape_with_direction`] sorts them to ascending cluster order.
    fn do_shape_rtl(
        &mut self,
        text: &str,
        font_data: Arc<[u8]>,
        size: f32,
    ) -> Result<ShapedRun, OxiTextError> {
        let font = FontRef::from_index(&font_data, 0)
            .ok_or_else(|| OxiTextError::Shaping("swash could not parse font bytes".into()))?;

        let mut shaper = self
            .ctx
            .builder(font)
            .size(size)
            .direction(Direction::RightToLeft)
            .build();
        shaper.add_str(text);

        let mut glyphs: SmallVec<[ShapedGlyph; 8]> = SmallVec::new();
        shaper.shape_with(|cluster| {
            let cluster_range = cluster.source.start as usize..cluster.source.end as usize;
            let is_ws = text
                .get(cluster_range)
                .map(|slice| !slice.is_empty() && slice.chars().all(|c| c.is_whitespace()))
                .unwrap_or(false);
            let multi = cluster.glyphs.len() > 1;
            for (idx, glyph) in cluster.glyphs.iter().enumerate() {
                let utb = (multi && idx > 0) || glyph.info.is_mark();
                glyphs.push(ShapedGlyph {
                    gid: glyph.id,
                    x_advance: glyph.advance,
                    y_advance: 0.0,
                    x_offset: glyph.x,
                    y_offset: glyph.y,
                    cluster: cluster.source.start,
                    is_whitespace: is_ws,
                    unsafe_to_break: utb,
                });
            }
        });

        Ok(ShapedRun {
            glyphs,
            font_data: Arc::clone(&font_data),
        })
    }

    /// Shapes `text` and returns a rich [`ShapeResult`] with metadata.
    ///
    /// The result includes the glyph list, the direction used, and any
    /// codepoints that could not be mapped (glyph ID 0 / `.notdef`).
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_full(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
    ) -> Result<ShapeResult, OxiTextError> {
        use unicode_segmentation::UnicodeSegmentation;

        let glyphs = self.shape_with_features_internal(ShapeParams {
            font_data,
            text,
            px_size,
            rtl: false,
            script_tag: None,
            language_tag: None,
            features: &[],
        })?;
        let mut result = ShapeResult::from_glyphs(glyphs, text, ShapeDirection::Ltr);
        // Populate grapheme cluster boundaries: start offset of each grapheme
        // plus the end-of-text sentinel.
        result.cluster_boundaries = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        Ok(result)
    }

    /// Shapes `text` using raw font bytes supplied as `&[u8]` (LTR).
    ///
    /// A convenience wrapper over `Self::shape_with_features_internal` for
    /// callers that already hold raw font bytes and do not need the `Arc` wrapping
    /// or cache infrastructure of [`Self::shape`].
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_slice(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        self.shape_with_features_internal(ShapeParams {
            font_data,
            text,
            px_size,
            rtl: false,
            script_tag: None,
            language_tag: None,
            features: &[],
        })
    }

    /// Shapes `text` using raw font bytes supplied as `&[u8]` (RTL).
    ///
    /// Like [`Self::shape_slice`] but shapes in right-to-left direction and
    /// returns glyphs in ascending `cluster` (logical source) order.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_slice_rtl(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        self.shape_with_features_internal(ShapeParams {
            font_data,
            text,
            px_size,
            rtl: true,
            script_tag: None,
            language_tag: None,
            features: &[],
        })
    }

    /// Shapes `text` with a font fallback chain.
    ///
    /// For each codepoint that produces `glyph_id == 0` (`.notdef`), the
    /// corresponding text run is re-shaped with each successive fallback font
    /// in `fonts[1..]`.  If a fallback produces a non-zero glyph ID the
    /// fallback glyphs replace the `.notdef` glyphs in the result; otherwise
    /// the `.notdef` glyphs are preserved (best-effort).
    ///
    /// `fonts[0]` is the primary font; `fonts[1..]` are tried in order.
    ///
    /// # Note on cluster offsets
    ///
    /// When a sub-string is re-shaped with a fallback font, swash emits cluster
    /// byte offsets **relative to that sub-string** (starting at 0).  This
    /// function adds the original start offset back before merging so all
    /// returned glyphs carry absolute offsets into `text`.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the primary font cannot be parsed.
    pub fn shape_with_fallback(
        &mut self,
        fonts: &[&[u8]],
        text: &str,
        px_size: f32,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        let primary = fonts
            .first()
            .ok_or_else(|| OxiTextError::Shaping("font list is empty".into()))?;

        // 1. Shape with the primary font.
        let mut result = self.shape_with_features_internal(ShapeParams {
            font_data: primary,
            text,
            px_size,
            rtl: false,
            script_tag: None,
            language_tag: None,
            features: &[],
        })?;

        if fonts.len() <= 1 {
            return Ok(result);
        }

        // 2. Find contiguous runs of .notdef (glyph ID 0) glyphs.
        let notdef_runs = collect_notdef_runs(&result, text);

        // 3. For each .notdef run try the fallback fonts.
        for (run_text_start, run_text_end) in notdef_runs {
            let sub_text = match text.get(run_text_start..run_text_end) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };

            // Try each fallback font in order.
            'fallback: for fallback_font in &fonts[1..] {
                let fallback_glyphs = match self.shape_with_features_internal(ShapeParams {
                    font_data: fallback_font,
                    text: sub_text,
                    px_size,
                    rtl: false,
                    script_tag: None,
                    language_tag: None,
                    features: &[],
                }) {
                    Ok(g) => g,
                    Err(_) => continue,
                };

                // Only use this fallback if it resolved at least one glyph.
                if fallback_glyphs.iter().all(|g| g.gid == 0) {
                    continue;
                }

                // Adjust cluster offsets from sub-string-relative to
                // text-absolute and replace the .notdef glyphs in result.
                let start_offset = run_text_start as u32;
                let adjusted: Vec<ShapedGlyph> = fallback_glyphs
                    .into_iter()
                    .map(|mut g| {
                        g.cluster += start_offset;
                        g
                    })
                    .collect();

                // Replace glyphs in the result whose cluster falls in [run_text_start, run_text_end).
                result.retain(|g| {
                    let c = g.cluster as usize;
                    !(c >= run_text_start && c < run_text_end && g.gid == 0)
                });

                // Insert adjusted fallback glyphs at the correct position.
                let insert_pos = result.partition_point(|g| (g.cluster as usize) < run_text_start);
                for (i, g) in adjusted.into_iter().enumerate() {
                    result.insert(insert_pos + i, g);
                }

                break 'fallback;
            }
        }

        Ok(result)
    }

    /// Returns `true` if the given font data contains AAT layout tables.
    ///
    /// Checks for the presence of `morx` (extended glyph metamorphosis rules),
    /// `kerx` (extended kerning data), or `ankr` (anchor point) tables — the
    /// three primary tables that distinguish Apple Advanced Typography (AAT)
    /// fonts from pure OpenType fonts.
    ///
    /// Swash's [`ShapeContext`] already applies AAT tables transparently when
    /// present, so this function is informational only; it does not change the
    /// shaping path.
    pub fn font_has_aat(font_data: &[u8]) -> bool {
        ttf_parser::Face::parse(font_data, 0)
            .map(|face| {
                face.raw_face()
                    .table(ttf_parser::Tag::from_bytes(b"morx"))
                    .is_some()
                    || face
                        .raw_face()
                        .table(ttf_parser::Tag::from_bytes(b"kerx"))
                        .is_some()
                    || face
                        .raw_face()
                        .table(ttf_parser::Tag::from_bytes(b"ankr"))
                        .is_some()
            })
            .unwrap_or(false)
    }

    /// Shape using AAT if the font has Morx/Kerx tables, otherwise fall back to
    /// standard OpenType shaping.
    ///
    /// Swash handles both AAT and OpenType tables transparently via its
    /// `ShapeContext`; this method is informational. It delegates directly to
    /// `Self::shape_with_features_internal` regardless of table presence.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_with_aat_fallback(
        &mut self,
        font_data: &[u8],
        text: &str,
        px_size: f32,
    ) -> Result<ShapeResult, OxiTextError> {
        use unicode_segmentation::UnicodeSegmentation;

        let glyphs = self.shape_with_features_internal(ShapeParams {
            font_data,
            text,
            px_size,
            rtl: false,
            script_tag: None,
            language_tag: None,
            features: &[],
        })?;
        let mut result = ShapeResult::from_glyphs(glyphs, text, ShapeDirection::Ltr);
        result.cluster_boundaries = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        Ok(result)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// ShapeResult
// ──────────────────────────────────────────────────────────────────────────────

/// Extended shaping result with metadata.
///
/// Produced by [`SwashShaper::shape_full`]; includes the glyph list, the
/// direction resolved by the shaper, the OpenType script tag (if known), and a
/// list of Unicode codepoints that could not be mapped (glyph ID 0 / `.notdef`).
#[derive(Debug, Clone)]
pub struct ShapeResult {
    /// Shaped glyphs in logical cluster order.
    pub glyphs: Vec<ShapedGlyph>,
    /// OpenType script tag detected (e.g. `b"latn"`, `b"arab"`), or `None` if
    /// unknown.  May be set by the caller after construction.
    pub script_detected: Option<[u8; 4]>,
    /// Direction resolved by the shaper.
    pub direction: ShapeDirection,
    /// Unicode codepoints that produced a `.notdef` glyph (ID 0).
    pub missing_codepoints: Vec<char>,
    /// Byte offsets (in the original text) where grapheme cluster boundaries fall.
    ///
    /// Populated by [`SwashShaper::shape_full`].  Empty when [`SwashShaper::shape`]
    /// is called directly.  The first entry is `0` (start of text) and the last
    /// entry is `text.len()` (end of text).
    pub cluster_boundaries: Vec<usize>,
}

impl ShapeResult {
    /// Constructs a [`ShapeResult`] from a glyph vector, the source text, and
    /// the shaping direction.
    ///
    /// `script_detected` is left as `None`; callers may set it afterwards.
    pub fn from_glyphs(glyphs: Vec<ShapedGlyph>, text: &str, direction: ShapeDirection) -> Self {
        let missing: Vec<char> = {
            let mut seen = std::collections::HashSet::new();
            let mut missing = Vec::new();
            for g in &glyphs {
                if g.gid == 0 {
                    if let Some(ch) = text
                        .get(g.cluster as usize..)
                        .and_then(|s| s.chars().next())
                    {
                        if seen.insert(ch) {
                            missing.push(ch);
                        }
                    }
                }
            }
            missing
        };
        Self {
            glyphs,
            script_detected: None,
            direction,
            missing_codepoints: missing,
            cluster_boundaries: Vec::new(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Collect contiguous byte ranges in `text` that are covered exclusively by
/// `.notdef` (glyph ID 0) glyphs in `glyphs`.
///
/// Returns a `Vec` of `(start, end)` byte offset pairs into `text`.
fn collect_notdef_runs(glyphs: &[ShapedGlyph], text: &str) -> Vec<(usize, usize)> {
    if glyphs.is_empty() {
        return Vec::new();
    }

    // Build a sorted, deduplicated list of cluster byte offsets that are .notdef.
    let mut notdef_clusters: Vec<usize> = glyphs
        .iter()
        .filter(|g| g.gid == 0)
        .map(|g| g.cluster as usize)
        .collect();
    notdef_clusters.sort_unstable();
    notdef_clusters.dedup();

    // Build a sorted list of all cluster start offsets (regardless of gid).
    let mut all_starts: Vec<usize> = glyphs.iter().map(|g| g.cluster as usize).collect();
    all_starts.sort_unstable();
    all_starts.dedup();

    // For each .notdef cluster, determine the end offset: it's the byte offset
    // of the next cluster in `all_starts`, or `text.len()` for the last one.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &start in &notdef_clusters {
        let end = all_starts
            .iter()
            .find(|&&s| s > start)
            .copied()
            .unwrap_or(text.len());
        // Merge with the previous run if adjacent.
        if let Some(last) = runs.last_mut() {
            if last.1 == start {
                last.1 = end;
                continue;
            }
        }
        runs.push((start, end));
    }
    runs
}

impl Default for SwashShaper {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// System font convenience methods (feature `system-fonts`)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "system-fonts")]
impl SwashShaper {
    /// Shape `text` using the best system font for its Unicode content.
    ///
    /// Calls [`system_fonts::load_best_font_for_text`] to discover a system
    /// font whose OS/2 Unicode range bits cover the codepoints in `text`, then
    /// shapes with that font at `px_size` pixels-per-em.
    ///
    /// This is a convenience wrapper; callers that need to reuse the same
    /// font database for many shaping calls should load the font bytes once
    /// with [`system_fonts::load_best_font_for_text`] and then call
    /// [`Self::shape_slice`] directly.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] when no suitable system font can be
    /// found or when the discovered font bytes cannot be parsed by swash.
    pub fn shape_with_system_font(
        &mut self,
        text: &str,
        px_size: f32,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        let font_data = system_fonts::load_best_font_for_text(text)
            .ok_or_else(|| OxiTextError::Shaping("no system font found for text".into()))?;
        self.shape_slice(&font_data, text, px_size)
    }

    /// Shape `text` using the system font that best matches `family`.
    ///
    /// `family` may be a concrete font family name (e.g. `"Arial"`) or a CSS
    /// generic alias (e.g. `"sans-serif"`).  The best CSS Level 4 match from
    /// the system catalog is used.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] when no font matching `family` can be
    /// found in the system catalog or when the font bytes cannot be parsed.
    pub fn shape_with_family(
        &mut self,
        text: &str,
        family: &str,
        px_size: f32,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        let font_data = system_fonts::load_font_for_family(family).ok_or_else(|| {
            OxiTextError::Shaping(format!("no system font found for family '{family}'"))
        })?;
        self.shape_slice(&font_data, text, px_size)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Script-aware itemization (Feature 1, behind `icu` feature gate)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "icu")]
/// Maps a [`oxitext_icu::TextScript`] to a 4-byte OpenType script tag.
fn text_script_to_ot_tag(s: oxitext_icu::TextScript) -> [u8; 4] {
    use oxitext_icu::TextScript;
    match s {
        TextScript::Latin => *b"latn",
        TextScript::Arabic => *b"arab",
        TextScript::Devanagari => *b"dev2",
        TextScript::Han => *b"hani",
        TextScript::Hangul => *b"hang",
        TextScript::Hiragana | TextScript::Katakana => *b"kana",
        TextScript::Hebrew => *b"hebr",
        TextScript::Thai => *b"thai",
        TextScript::Greek => *b"grek",
        TextScript::Cyrillic => *b"cyrl",
        _ => *b"DFLT",
    }
}

#[cfg(feature = "icu")]
impl SwashShaper {
    /// Shapes `text` by first splitting it into per-script runs using ICU4X
    /// script itemization, then shaping each run with the appropriate OpenType
    /// script tag.
    ///
    /// Returns one [`ShapedRun`] per script run, in logical (source) order.
    /// Each glyph's `cluster` offset is absolute (relative to the start of
    /// `text`), not relative to the sub-run.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Shaping`] if the font bytes cannot be parsed.
    pub fn shape_by_script(
        &mut self,
        font_data: Arc<[u8]>,
        text: &str,
        px_size: f32,
        features: &[ShapeFeature],
    ) -> Result<Vec<ShapedRun>, OxiTextError> {
        // Reuse cached script runs when the text is unchanged (Item 4 cache).
        if self.script_cache_text != text {
            let props = oxitext_icu::CharProperties::new();
            self.script_cache_runs = props.itemize(text);
            self.script_cache_text = text.to_owned();
        }
        let script_runs = self.script_cache_runs.clone();

        let mut result: Vec<ShapedRun> = Vec::with_capacity(script_runs.len());

        for run in &script_runs {
            let sub_text = text
                .get(run.start..run.end)
                .ok_or_else(|| OxiTextError::Shaping("invalid script run byte range".into()))?;

            let ot_tag = text_script_to_ot_tag(run.script);
            let is_rtl = run.script.is_rtl();

            let mut glyphs = self.shape_with_features_internal(ShapeParams {
                font_data: &font_data,
                text: sub_text,
                px_size,
                rtl: is_rtl,
                script_tag: Some(ot_tag),
                language_tag: None,
                features,
            })?;

            // Adjust cluster offsets from sub-run-relative to text-absolute.
            let start_offset = run.start as u32;
            for g in &mut glyphs {
                g.cluster += start_offset;
            }

            result.push(ShapedRun {
                glyphs: glyphs.into(),
                font_data: Arc::clone(&font_data),
            });
        }

        Ok(result)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Kashida insertion opportunities (Feature 2)
// ──────────────────────────────────────────────────────────────────────────────

/// Returns `true` when `c` is an Arabic character with Dual_Joining type.
///
/// Dual-joining characters connect to neighbours on both sides and are
/// therefore eligible for kashida (tatweel) stretching. This approximation
/// covers the mainstream Arabic block: U+0626..=U+063A and U+0641..=U+064A,
/// excluding known non-joiners (Alef U+0627, Dhal–Zain U+062F..=U+0632,
/// Waw U+0648).
fn is_arabic_dual_joining(c: char) -> bool {
    let cp = c as u32;
    match cp {
        // Lower Arabic range: Ba through Ghain (excludes Alef 0x0627,
        // Dal-Zain 0x062F–0x0632, and Waw 0x0648 which are right-joining only)
        0x0626..=0x063A => !matches!(cp, 0x0627 | 0x062F..=0x0632),
        // Upper Arabic range: Fa through Ya
        0x0641..=0x064A => !matches!(cp, 0x0648),
        _ => false,
    }
}

/// Returns glyph indices (into `glyphs`) after which a kashida stretch can be
/// inserted for Arabic justification.
///
/// A position is a kashida opportunity when the source character at the
/// glyph's cluster byte offset is an Arabic dual-joining character (one that
/// connects on both sides and can therefore be stretched with tatweel).
///
/// If `text` does not contain Arabic text, or if no glyph's cluster maps to a
/// dual-joining character, the returned `Vec` is empty.
pub fn find_kashida_opportunities(text: &str, glyphs: &[ShapedGlyph]) -> Vec<usize> {
    let mut result = Vec::new();
    for (idx, glyph) in glyphs.iter().enumerate() {
        let byte_pos = glyph.cluster as usize;
        if let Some(ch) = text.get(byte_pos..).and_then(|s| s.chars().next()) {
            if is_arabic_dual_joining(ch) {
                result.push(idx);
            }
        }
    }
    result
}

// ──────────────────────────────────────────────────────────────────────────────
// Emoji ZWJ sequence detection (Feature 3)
// ──────────────────────────────────────────────────────────────────────────────

/// Returns byte ranges in `text` that correspond to ZWJ-joined emoji sequences.
///
/// A ZWJ emoji sequence is a grapheme cluster that:
/// 1. Contains U+200D (ZERO WIDTH JOINER), **and**
/// 2. Has at least two non-ZWJ codepoints (i.e. it is not a bare ZWJ followed
///    by nothing).
///
/// The returned ranges are contiguous byte spans in `text` covering each such
/// cluster. When multiple such clusters are adjacent (share no separator) they
/// are reported individually.
///
/// Uses [`unicode_segmentation::UnicodeSegmentation::grapheme_indices`] for
/// grapheme-cluster boundaries so that the detection is consistent with UAX #29.
pub fn detect_emoji_zwj_sequences(text: &str) -> Vec<std::ops::Range<usize>> {
    use unicode_segmentation::UnicodeSegmentation;

    let mut result = Vec::new();
    for (start, cluster) in text.grapheme_indices(true) {
        // A ZWJ sequence must contain the joiner itself.
        if !cluster.contains('\u{200D}') {
            continue;
        }
        // Must also have at least 2 non-ZWJ codepoints.
        let non_zwj_count = cluster.chars().filter(|&c| c != '\u{200D}').count();
        if non_zwj_count >= 2 {
            let end = start + cluster.len();
            result.push(start..end);
        }
    }
    result
}

#[cfg(test)]
mod bench_tests;
#[cfg(test)]
mod tests_inline;
