#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! `oxitext` — Pure-Rust text rendering pipeline.
//!
//! Default features: `["pure"]` wires swash shaping + simple LTR layout +
//! fontdue rasterization into a single [`Pipeline`].  M2 re-exports
//! [`BidiParagraph`], [`LineBreaker`], and [`VerticalMetrics`] unconditionally.
//!
//! ## Feature Flags
//!
//! | Feature | Enables | Default |
//! |---------|---------|---------|
//! | `pure` | [`Pipeline`], [`SwashShaper`], [`LayoutEngine`], [`FontdueRasterizer`] — the full pipeline | yes |
//! | `sdf` | [`sdf`] module: [`sdf::SdfAtlas`], [`sdf::MsdfAtlas`], [`sdf::glyph_to_sdf_tile`] — SDF atlas generation | no |
//! | `icu` | [`icu`] module: ICU4X CLDR line-breaking, word segmentation, collation, normalization | no |
//! | `simd` | SIMD-accelerated rasterization paths in the fontdue backend | no |
//! | `parallel` | Parallel rasterization via rayon (implies `pure`) | no |
//! | `png-output` | [`RenderResult::to_png`] — write rendered text to a PNG file | no |
//! | `font-subset` | [`pdf_subset`] module: on-the-fly font subsetting for PDF rendering pipelines | no |
//! | `color-bitmap-fonts` | PNG-compressed CBDT/sbix strike decoding (pulls `png` → `flate2`) | no |
//!
//! ### Combining features
//!
//! ```toml
//! # Minimal: pipeline only
//! oxitext = { version = "0.2.3", features = ["pure"] }
//!
//! # With SDF atlas for GPU rendering
//! oxitext = { version = "0.2.3", features = ["pure", "sdf"] }
//!
//! # Full: pipeline + ICU + SDF + PNG output
//! oxitext = { version = "0.2.3", features = ["pure", "sdf", "icu", "png-output"] }
//! ```
//!
//! ### What each feature pulls in
//!
//! - **`pure`**: Enables [`Pipeline`] and all core text rendering types. Required for any
//!   text rendering. Pulls in `swash`, `fontdue`, `unicode-bidi`, `unicode-linebreak`.
//! - **`sdf`**: Signed Distance Field atlas generation via [`sdf::SdfAtlas`] (greyscale EDT) and
//!   [`sdf::MsdfAtlas`] (multi-channel Chlumsky edge-coloring). Suitable for GPU rendering
//!   pipelines (wgpu, Vulkan) that resolve glyphs from SDF textures at runtime. Approximately
//!   +150 KB binary overhead.
//! - **`icu`**: ICU4X CLDR-backed Unicode processing. Enables CLDR-compliant line-breaking,
//!   grapheme/word/sentence segmentation, Unicode collation (locale-aware sort), and NFC/NFD/
//!   NFKC/NFKD normalization. **Note:** Adds approximately 5–15 MB of compiled CLDR data to
//!   your binary. See [`icu`] module docs for size-reduction strategies.
//! - **`simd`**: Enables wide f32x8 SIMD lanes in the fontdue rasterization hot-loop and the
//!   Porter-Duff compositing path. Requires a CPU with AVX2 or equivalent. No API surface
//!   change; the same functions are called and automatically dispatch to wider code paths.
//! - **`parallel`**: Enables rayon-based parallel rasterization. Each rayon worker thread
//!   owns its own `FontdueRasterizer` instance (no `Mutex` contention). Implies `pure`.
//!   No API surface change; [`Pipeline::render`] automatically parallelizes the raster phase.
//! - **`png-output`**: Adds [`RenderResult::to_png`] for writing rendered bitmaps directly to
//!   PNG files. Backed by `oxitext-core`'s Pure-Rust `png_encode` module (`oxiarc-deflate` /
//!   `oxiarc-core`), not the `png` crate. Useful for testing and offline rendering.
//! - **`font-subset`**: Adds the [`pdf_subset`] module with [`pdf_subset::TextFontSubsetter`],
//!   a streaming accumulator for on-the-fly font subsetting during PDF text rendering.
//!   Pulls in `oxifont-subset` (~300 KB). Feed text via [`pdf_subset::TextFontSubsetter::feed_text`]
//!   during page composition, then call [`pdf_subset::TextFontSubsetter::finalize`] to produce
//!   a minimal subset font for embedding in the PDF stream.
//!
//! **Blueprint deviation:** The original blueprint listed `default = ["pure","emoji"]`,
//! but the `emoji` feature depends on the `oxitext-emoji` crate which is
//! planned for M3. The M1 default is `["pure"]` only; emoji support will be
//! added when `oxitext-emoji` lands.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use oxitext::{Pipeline, TextStyle};
//!
//! let font_bytes = std::fs::read("path/to/font.ttf").expect("read font");
//! let mut pipeline = Pipeline::from_bytes(&font_bytes).expect("invalid font");
//! let style = TextStyle::default();
//! let result = pipeline.render("Hello, world", &style).expect("render failed");
//! println!("{} glyphs, {} bitmaps", result.glyphs.len(), result.bitmaps.len());
//! ```

pub use oxitext_core::{
    Bitmap, ColorBitmap, Decoration, DecorationLine, DecorationRect, FlowDirection,
    FontVerticalMetrics, GlyphCluster, GlyphMetrics, LayoutConstraints, LineSpacing, OxiTextError,
    ParagraphStyle, PositionedGlyph, RenderOutput, Rgba8, ShapedGlyph, ShapedRun, TextAlignment,
    TextDecoration, TextRun, TextStyle, WritingMode,
};

// M6: word-aware layout engine with alignment and structured metrics.
pub use oxitext_layout::{LayoutEngine, LayoutResult, Line, LineMetrics, ParagraphMetrics};

// M3: SDF atlas generation (optional feature).
#[cfg(feature = "sdf")]
/// SDF atlas generation (re-exported from `oxitext-sdf`).
pub mod sdf {
    pub use oxitext_sdf::*;
}

/// On-the-fly font subsetting for PDF text rendering pipelines.
///
/// Enabled by the `font-subset` feature flag:
///
/// ```toml
/// oxitext = { version = "0.2.3", features = ["font-subset"] }
/// ```
///
/// The main entry point is [`pdf_subset::TextFontSubsetter`], which accumulates
/// glyph usage across multiple pages and produces a minimal subset font via
/// [`pdf_subset::TextFontSubsetter::finalize`].
#[cfg(feature = "font-subset")]
pub mod pdf_subset;

// Re-exports of backend traits and key shaping types for users who want
// custom backends without taking a direct dependency on the sub-crates.
#[cfg(feature = "pure")]
pub use oxitext_raster::RasterBackend;
#[cfg(feature = "pure")]
pub use oxitext_shape::{ShapeBackend, ShapeDirection, ShapeFeature, ShapeRequest};

// M2: bidi, line-break, and vertical orientation are always available (no feature gate).
pub use oxitext_layout::bidi::{BidiParagraph, BidiRun};
pub use oxitext_layout::linebreak::{LineBreak, LineBreaker};
pub use oxitext_layout::vertical::{is_upright_in_vertical, VerticalMetrics};

// M4: tate-chu-yoko helpers (always available; icu feature adds CLDR segmentation/collation).
pub use oxitext_layout::{detect_runs, GlyphEntry, TateChuYokoRun};

/// ICU4X-backed CLDR segmentation, normalization, properties, collation, and case mapping.
///
/// Enabled by the `icu` feature flag.
#[cfg(feature = "icu")]
pub mod icu {
    /// Re-exports from `oxitext-icu`.
    pub use oxitext_icu::{
        CaseMapper, CharProperties, CollateError, IcuCollator, IcuSegmenter, NormalizationForm,
        Normalizer, ScriptRun, SegmentKind, TextScript,
    };
}

#[cfg(feature = "pure")]
pub use oxitext_layout::SimpleLayouter;
#[cfg(feature = "pure")]
pub use oxitext_raster::FontdueRasterizer;
#[cfg(feature = "pure")]
pub use oxitext_shape::SwashShaper;

/// The result of rendering a string of text.
pub struct RenderResult {
    /// Positioned glyphs in layout order.
    pub glyphs: Vec<PositionedGlyph>,
    /// Per-glyph greyscale bitmaps in the same order as `glyphs`.
    ///
    /// For color glyphs this contains an empty [`Bitmap`]; use
    /// [`Self::outputs`] to access the [`RenderOutput::Color`] variant.
    pub bitmaps: Vec<Bitmap>,
    /// Per-glyph render outputs including both greyscale and color variants.
    ///
    /// Always the same length as [`Self::glyphs`].  For greyscale glyphs this
    /// mirrors `bitmaps`; for COLRv0/v1 color glyphs the entry is
    /// [`RenderOutput::Color`].
    pub outputs: Vec<RenderOutput>,
    /// Per-line records indexing into [`Self::glyphs`].
    ///
    /// Populated by the word-aware layout engine. Lets callers render or
    /// hit-test on a per-line basis.
    pub lines: Vec<Line>,
    /// Aggregate paragraph metrics (total width/height, line count, overflow).
    pub metrics: ParagraphMetrics,
    /// Decoration rectangles (underlines, overlines, strikethroughs) produced
    /// by the layout engine when a `TextDecoration` was requested via
    /// `LayoutOptions::decoration` (in `oxitext_layout`).
    ///
    /// Empty when the basic [`Pipeline::render`] path is used (which does not
    /// pass a decoration through to the layout engine).  Populated when calling
    /// `Pipeline::layout_with_options` or [`Pipeline::render_paragraph`] with a
    /// decoration-enabled `LayoutOptions`.
    pub decoration_rects: Vec<DecorationRect>,
}

impl RenderResult {
    /// Composites all greyscale glyph bitmaps onto an RGBA canvas of the given
    /// size, painting glyphs in `text_color` over `bg_color`.
    ///
    /// Returns a [`ColorBitmap`] (`width * height * 4` RGBA bytes). Glyph
    /// coverage is treated as an alpha mask and blended with straight-alpha
    /// Porter-Duff "source over".
    pub fn composite_to_rgba(
        &self,
        width: u32,
        height: u32,
        bg_color: Rgba8,
        text_color: Rgba8,
    ) -> ColorBitmap {
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        // Fill background.
        for px in rgba.chunks_exact_mut(4) {
            px[0] = bg_color.r;
            px[1] = bg_color.g;
            px[2] = bg_color.b;
            px[3] = bg_color.a;
        }

        for (i, glyph) in self.glyphs.iter().enumerate() {
            let ox = glyph.pos.0.round() as i32;
            let oy = glyph.pos.1.round() as i32;

            match self.outputs.get(i) {
                Some(RenderOutput::Greyscale(bm)) => {
                    if bm.is_empty() {
                        continue;
                    }
                    // Position the glyph bitmap with its top-left at the pen
                    // position. `pos` is the pen origin on the baseline; callers
                    // wanting precise bearings should use the per-glyph metrics
                    // API.
                    blit_coverage(
                        &mut rgba,
                        width as i32,
                        height as i32,
                        bm,
                        ox,
                        oy,
                        text_color,
                    );
                }
                Some(RenderOutput::Color(cbm)) => {
                    if cbm.is_empty() {
                        continue;
                    }
                    // Porter-Duff source-over blit for RGBA color glyphs.
                    // The COLR palette already carries color; ignore text_color
                    // tint so the emoji retains its native appearance.
                    blit_color(&mut rgba, width as i32, height as i32, cbm, ox, oy);
                }
                Some(RenderOutput::Lcd(lcd_bm)) => {
                    if lcd_bm.is_empty() {
                        continue;
                    }
                    // LCD subpixel bitmaps: average the three sub-pixel channels
                    // into a single coverage value and render as greyscale with
                    // the requested text color.
                    let synthetic_bm = lcd_to_greyscale(lcd_bm);
                    blit_coverage(
                        &mut rgba,
                        width as i32,
                        height as i32,
                        &synthetic_bm,
                        ox,
                        oy,
                        text_color,
                    );
                }
                Some(RenderOutput::Sdf { .. }) | Some(RenderOutput::Msdf { .. }) => {
                    // SDF/MSDF tiles encode a signed distance field, not a
                    // direct coverage mask.  Compositing them correctly requires
                    // a GPU shader (threshold + smooth-step) that is outside the
                    // scope of the CPU compositor.  Skip silently so that text
                    // mixed with SDF glyphs does not panic or produce artefacts.
                    continue;
                }
                None => {
                    // No output recorded for this glyph (e.g. whitespace glyph
                    // with no rasterized form).  Skip gracefully.
                    continue;
                }
            }
        }

        // Composite decoration rectangles (underlines, overlines, strikethroughs)
        // on top of the glyph layer.
        for rect in &self.decoration_rects {
            let x0 = rect.x.max(0.0) as u32;
            let y0 = rect.y.max(0.0) as u32;
            let x1 = (rect.x + rect.width).ceil() as u32;
            let y1 = (rect.y + rect.height).ceil() as u32;
            for row in y0..y1.min(height) {
                for col in x0..x1.min(width) {
                    let idx = (row * width + col) as usize * 4;
                    if idx + 3 < rgba.len() {
                        rgba[idx] = rect.color.r;
                        rgba[idx + 1] = rect.color.g;
                        rgba[idx + 2] = rect.color.b;
                        rgba[idx + 3] = rect.color.a;
                    }
                }
            }
        }

        ColorBitmap {
            width,
            height,
            rgba,
        }
    }

    /// Write the render result as a PNG file.
    ///
    /// Composites all glyph bitmaps onto a canvas of `width × height` pixels
    /// using [`Self::composite_to_rgba`], then encodes the result as a 32-bit
    /// RGBA PNG and writes it to `path`.
    ///
    /// # Parameters
    /// - `path`   — Destination file path; the file is created (or truncated).
    /// - `width`  — Canvas width in pixels.
    /// - `height` — Canvas height in pixels.
    /// - `bg`     — Background colour as `Rgba8`.
    /// - `fg`     — Foreground (text) colour as `Rgba8`.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Other`] if the file cannot be created or the
    /// PNG encoder fails.
    ///
    /// # Feature
    /// Only available when compiled with the `png-output` feature.
    #[cfg(feature = "png-output")]
    pub fn to_png(
        &self,
        path: &std::path::Path,
        width: u32,
        height: u32,
        bg: Rgba8,
        fg: Rgba8,
    ) -> Result<(), OxiTextError> {
        use oxitext_core::png_encode::{encode_png, PngColorType};

        let canvas = self.composite_to_rgba(width, height, bg, fg);
        let bytes = encode_png(width, height, PngColorType::Rgba8, &canvas.rgba)
            .map_err(|e| OxiTextError::Other(format!("png encode: {e}")))?;
        std::fs::write(path, bytes).map_err(|e| OxiTextError::Other(format!("png write: {e}")))
    }
}

/// Blit a greyscale coverage bitmap onto an RGBA canvas at `(ox, oy)` using
/// `color` as the source RGB and coverage as the source alpha.
fn blit_coverage(canvas: &mut [u8], cw: i32, ch: i32, bm: &Bitmap, ox: i32, oy: i32, color: Rgba8) {
    for gy in 0..bm.height as i32 {
        for gx in 0..bm.width as i32 {
            let dx = ox + gx;
            let dy = oy + gy;
            if dx < 0 || dy < 0 || dx >= cw || dy >= ch {
                continue;
            }
            let cov = bm.pixels[(gy as u32 * bm.width + gx as u32) as usize];
            if cov == 0 {
                continue;
            }
            // Source alpha = glyph coverage * color alpha.
            let sa = (cov as u32 * color.a as u32 / 255) as u8;
            if sa == 0 {
                continue;
            }
            let idx = ((dy * cw + dx) * 4) as usize;
            source_over(&mut canvas[idx..idx + 4], color.r, color.g, color.b, sa);
        }
    }
}

/// Blit an RGBA color bitmap onto an RGBA canvas at `(ox, oy)` using
/// Porter-Duff straight-alpha "source over".
///
/// The color bitmap's own alpha channel drives the compositing; the caller's
/// text color is intentionally ignored so that COLR/CPAL glyphs retain their
/// native palette colors.
fn blit_color(canvas: &mut [u8], cw: i32, ch: i32, cbm: &ColorBitmap, ox: i32, oy: i32) {
    for gy in 0..cbm.height as i32 {
        for gx in 0..cbm.width as i32 {
            let dx = ox + gx;
            let dy = oy + gy;
            if dx < 0 || dy < 0 || dx >= cw || dy >= ch {
                continue;
            }
            let src_idx = ((gy as u32 * cbm.width + gx as u32) * 4) as usize;
            if src_idx + 3 >= cbm.rgba.len() {
                continue;
            }
            let sr = cbm.rgba[src_idx];
            let sg = cbm.rgba[src_idx + 1];
            let sb = cbm.rgba[src_idx + 2];
            let sa = cbm.rgba[src_idx + 3];
            if sa == 0 {
                continue;
            }
            let dst_idx = ((dy * cw + dx) * 4) as usize;
            source_over(&mut canvas[dst_idx..dst_idx + 4], sr, sg, sb, sa);
        }
    }
}

/// Convert an [`oxitext_core::LcdBitmap`] (3 bytes/pixel: R, G, B sub-pixel channels)
/// into a greyscale [`Bitmap`] by averaging the three sub-pixel channels per pixel.
///
/// This is a coarse fallback that allows the CPU compositor to handle LCD glyphs
/// without requiring a sub-pixel-aware blending pipeline.
fn lcd_to_greyscale(lcd: &oxitext_core::LcdBitmap) -> Bitmap {
    let pixel_count = (lcd.width * lcd.height) as usize;
    let mut pixels = Vec::with_capacity(pixel_count);
    for i in 0..pixel_count {
        let base = i * 3;
        if base + 2 < lcd.rgb.len() {
            let avg =
                (lcd.rgb[base] as u16 + lcd.rgb[base + 1] as u16 + lcd.rgb[base + 2] as u16) / 3;
            pixels.push(avg as u8);
        } else {
            pixels.push(0);
        }
    }
    Bitmap {
        width: lcd.width,
        height: lcd.height,
        pixels,
    }
}

/// In-place straight-alpha Porter-Duff "source over".
fn source_over(dst: &mut [u8], sr: u8, sg: u8, sb: u8, sa: u8) {
    let sa_f = sa as f32 / 255.0;
    let da_f = dst[3] as f32 / 255.0;
    let out_a = sa_f + da_f * (1.0 - sa_f);
    if out_a < 1e-6 {
        return;
    }
    let blend = |s: u8, d: u8| -> u8 {
        let s_f = s as f32 / 255.0;
        let d_f = d as f32 / 255.0;
        let out = (s_f * sa_f + d_f * da_f * (1.0 - sa_f)) / out_a;
        (out.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    dst[0] = blend(sr, dst[0]);
    dst[1] = blend(sg, dst[1]);
    dst[2] = blend(sb, dst[2]);
    dst[3] = (out_a * 255.0).round() as u8;
}

/// Extract font vertical metrics from raw font bytes via oxifont, if possible.
///
/// Returns `None` if the bytes cannot be parsed or the font declares no
/// metrics; the layout engine then falls back to size-proportional defaults.
#[cfg(feature = "pure")]
fn extract_vertical_metrics(font_bytes: &[u8]) -> Option<FontVerticalMetrics> {
    use oxifont::{FontFace as _, ParsedFace};
    let arc: std::sync::Arc<[u8]> = font_bytes.to_vec().into();
    let parsed = ParsedFace::parse(arc, 0).ok()?;
    let m = parsed.metrics()?;
    Some(FontVerticalMetrics {
        units_per_em: m.units_per_em,
        ascender: m.ascender,
        descender: m.descender,
        line_gap: m.line_gap,
    })
}

/// Validates that `font_bytes` can be parsed (quick font validity check).
///
/// Returns `Ok(())` if the bytes look like a valid font, or
/// `Err(OxiTextError::Other(...))` otherwise.
#[cfg(feature = "pure")]
fn validate_font(font_bytes: &[u8]) -> Result<(), OxiTextError> {
    use oxifont::ParsedFace;
    let arc: std::sync::Arc<[u8]> = font_bytes.to_vec().into();
    ParsedFace::parse(arc, 0).map_err(|e| OxiTextError::Other(format!("invalid font: {e}")))?;
    Ok(())
}

/// Returns `true` if the font identified by `font_data` contains a glyph for
/// the given Unicode character.
///
/// Uses `ttf_parser` to probe the cmap; returns `false` on any parse error.
#[cfg(feature = "pure")]
fn font_has_glyph(font_data: &[u8], ch: char) -> bool {
    ttf_parser::Face::parse(font_data, 0)
        .map(|face| face.glyph_index(ch).is_some())
        .unwrap_or(false)
}

/// Return the index of the best font (from the primary + fallback chain) that
/// contains a glyph for `ch`.
///
/// - Returns `0` for the primary font.
/// - Returns `i + 1` for the `i`-th fallback font.
/// - Returns `0` (primary) if no font in the chain has a glyph for `ch`.
#[cfg(feature = "pure")]
pub fn best_font_for_char(ch: char, primary: &[u8], fallbacks: &[Vec<u8>]) -> usize {
    if font_has_glyph(primary, ch) {
        return 0;
    }
    for (i, fallback) in fallbacks.iter().enumerate() {
        if font_has_glyph(fallback, ch) {
            return i + 1;
        }
    }
    // Default to primary even if glyph is missing.
    0
}

/// Rasterize a single glyph, trying its detected color-glyph format (COLRv0,
/// COLRv1, CBDT/CBLC, `sbix`, or -- with the `svg-glyphs` feature -- `SVG `)
/// first, then falling back to greyscale.
///
/// Returns a [`RenderOutput`] that is [`RenderOutput::Color`] for color glyphs
/// successfully decoded and [`RenderOutput::Greyscale`] for all others,
/// including color formats this build cannot decode (e.g. a PNG-encoded CBDT
/// strike without the `color-bitmap-fonts` feature, or an `SVG ` glyph
/// without `svg-glyphs`).
///
/// `rasterizer` is the caller's [`FontdueRasterizer`] so its parse cache is
/// reused across glyphs.  The parallel path creates a fresh rasterizer per
/// thread; the sequential path reuses the pipeline's instance.
#[cfg(feature = "pure")]
fn rasterize_single(
    gid: u16,
    font_data: &std::sync::Arc<[u8]>,
    px_size: f32,
    rasterizer: &FontdueRasterizer,
) -> RenderOutput {
    use oxitext_raster::{detect_color_glyph_type_at, render_colr_cached, ColorGlyphType};

    // Which colour-glyph format this glyph has *at the size we are about to
    // render*: bitmap strikes are per-ppem, so probing at `px_size` keeps
    // detection and rendering in agreement (a strike that does not cover this
    // size falls through to the next format instead of promising a bitmap
    // `render_cbdt_glyph` cannot produce).
    let probe_ppem = (px_size.ceil() as u32).clamp(1, u16::MAX as u32) as u16;
    let color_type = detect_color_glyph_type_at(font_data, gid, probe_ppem);
    match color_type {
        ColorGlyphType::ColrV0 | ColorGlyphType::ColrV1 => {
            // `render_colr_cached` drives the full paint graph and handles both
            // table versions; the previous `render_colr_v0` call silently
            // discarded every gradient and composite paint, which left most
            // COLRv1 emoji as a fully transparent bitmap.  The `_cached` form
            // memoizes the paint graph per thread, keyed on this very
            // `Arc<[u8]>`, so a repeated glyph costs a refcount bump.
            let dim = px_size.ceil() as u32;
            let glyph_id = ttf_parser::GlyphId(gid);
            if let Some(cbm) = render_colr_cached(font_data, glyph_id, dim, dim, 0) {
                return RenderOutput::Color(ColorBitmap {
                    width: cbm.width,
                    height: cbm.height,
                    rgba: cbm.rgba.clone(),
                });
            }
        }
        ColorGlyphType::EmbeddedBitmap | ColorGlyphType::Sbix => {
            // CBDT/CBLC and Apple `sbix` both surface their strikes through
            // ttf-parser's uniform `glyph_raster_image` API, so one call
            // (`render_cbdt_glyph`) covers both -- and it is always reachable
            // here, with no extra facade feature, because `oxitext-raster`'s
            // `detect` module is unconditional. Uncompressed CBDT strike
            // formats (mono/gray2/gray4/gray8/BGRA32) decode unconditionally;
            // PNG-encoded strikes -- the common case for real colour emoji
            // fonts (e.g. NotoColorEmoji's CBDT build) and the *only* format
            // `sbix` uses -- additionally require the `color-bitmap-fonts`
            // feature. That feature is deny-clean (the decoder is
            // `oxitext-core`'s `oxiarc`-backed `png_decode`, not the banned
            // `png`/`flate2` stack); it is merely off by default, and without
            // it a PNG strike returns `None` and falls through to the
            // greyscale path below exactly as an unsupported colour format
            // already did.
            let px_size_u16 = probe_ppem;
            if let Some(cgb) = oxitext_raster::render_cbdt_glyph(font_data, gid, px_size_u16) {
                return RenderOutput::Color(ColorBitmap {
                    width: cgb.width,
                    height: cgb.height,
                    rgba: cgb.rgba,
                });
            }
        }
        #[cfg(feature = "svg-glyphs")]
        ColorGlyphType::Svg => {
            // Requires the `svg-glyphs` feature (off by default: `resvg`'s
            // `usvg` dependency pulls `flate2` -> `miniz_oxide`
            // unconditionally for `.svgz` support, banned by `deny.toml`; see
            // `oxitext-raster`'s Cargo.toml `svg-backend` doc comment). When
            // the feature is off, `Svg` falls through to the `_` arm below
            // exactly like an unsupported colour format.
            if let Some(bmp) = oxitext_raster::render_svg_glyph(font_data, gid, probe_ppem) {
                return RenderOutput::Color(bmp);
            }
        }
        _ => {}
    }

    // Greyscale fallback: rasterize via fontdue using the caller's cached instance.
    match rasterizer.raster(gid, font_data, px_size) {
        Ok(bm) => RenderOutput::Greyscale(bm),
        Err(_) => RenderOutput::Greyscale(Bitmap {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }),
    }
}

/// Maps a [`oxitext_icu::TextScript`] to the corresponding OpenType 4-byte script tag.
///
/// Follows the OpenType Script tag registry.
#[cfg(all(feature = "pure", feature = "icu"))]
fn script_to_opentype_tag(script: oxitext_icu::TextScript) -> [u8; 4] {
    use oxitext_icu::TextScript;
    match script {
        TextScript::Latin => *b"latn",
        TextScript::Greek => *b"grek",
        TextScript::Cyrillic => *b"cyrl",
        TextScript::Arabic => *b"arab",
        TextScript::Hebrew => *b"hebr",
        TextScript::Han => *b"hani",
        TextScript::Hiragana | TextScript::Katakana => *b"kana",
        TextScript::Hangul => *b"hang",
        TextScript::Thai => *b"thai",
        TextScript::Devanagari => *b"deva",
        TextScript::Common | TextScript::Inherited | TextScript::Other => *b"DFLT",
    }
}

/// Splits `text` into contiguous runs sharing a single Unicode script,
/// returning `(byte_start, byte_end, opentype_script_tag)` triples.
///
/// Uses [`oxitext_icu::CharProperties::itemize`] under the hood.
#[cfg(all(feature = "pure", feature = "icu"))]
fn itemize_by_script(text: &str) -> Vec<(usize, usize, [u8; 4])> {
    use oxitext_icu::CharProperties;
    let props = CharProperties::new();
    props
        .itemize(text)
        .into_iter()
        .map(|r| (r.start, r.end, script_to_opentype_tag(r.script)))
        .collect()
}

/// Fluent builder for [`Pipeline`].
///
/// Obtain via [`Pipeline::builder`].
#[cfg(feature = "pure")]
pub struct PipelineBuilder {
    font_data: Option<Vec<u8>>,
}

#[cfg(feature = "pure")]
impl PipelineBuilder {
    /// Sets the primary font bytes (TTF or OTF).
    pub fn font(mut self, data: Vec<u8>) -> Self {
        self.font_data = Some(data);
        self
    }

    /// Builds the [`Pipeline`].
    ///
    /// # Errors
    /// - [`OxiTextError::FontNotFound`] if no font data was provided.
    /// - [`OxiTextError::Other`] if the font data is invalid or unparseable.
    pub fn build(self) -> Result<Pipeline, OxiTextError> {
        let data = self.font_data.ok_or(OxiTextError::FontNotFound)?;
        Pipeline::from_bytes(&data)
    }
}

/// Internal shaper selection: either the default `SwashShaper` (with full
/// bidi/ICU/vertical support) or a user-supplied custom backend.
///
/// The `Custom` variant uses a simplified LTR-only code path because the
/// `ShapeBackend` trait does not carry script-itemization or bidi-resolution
/// context.
///
/// Both variants are boxed to keep the enum size uniform.
#[cfg(feature = "pure")]
enum ShaperKind {
    Default(Box<SwashShaper>),
    Custom(Box<dyn ShapeBackend + Send + Sync>),
}

/// End-to-end text rendering pipeline.
///
/// Combines [`SwashShaper`] + the word-aware [`LayoutEngine`] +
/// [`FontdueRasterizer`] into a single convenient entry point. Enabled by the
/// `pure` feature (default).
///
/// The layout engine wraps text at UAX #14 opportunities, honours mandatory
/// breaks, applies [`TextAlignment`], and uses the font's real
/// ascender/descender/line-gap (extracted via oxifont) for accurate line
/// spacing when available.
#[cfg(feature = "pure")]
pub struct Pipeline {
    shaper: ShaperKind,
    engine: LayoutEngine,
    // Used by the sequential rasterization path; the parallel path creates a fresh
    // rasterizer per thread.  The field is intentionally kept here so the sequential
    // path can benefit from the parse cache without an `Option`-wrapping ceremony.
    #[allow(dead_code)]
    rasterizer: FontdueRasterizer,
    font_data: std::sync::Arc<[u8]>,
    vmetrics: Option<FontVerticalMetrics>,
    /// Fallback font chain.  When a glyph is `.notdef` (gid == 0) in the
    /// primary font, the pipeline re-shapes the text slice through each
    /// fallback in order until a non-zero glyph ID is found.
    fallback_fonts: Vec<std::sync::Arc<[u8]>>,
    // ── Shape cache (Feature 2) ──────────────────────────────────────────────
    /// Source text from the last successful `shape_and_layout` call.
    ///
    /// The cache is invalidated whenever the text changes, the font changes
    /// (via `from_bytes` / `with_backend`), or the fallback chain changes
    /// (via `set_fallback_fonts`).
    shape_cache_text: String,
    /// FNV-style hash of the style fields that affect shaping: `font_size.to_bits()`
    /// XOR-mixed with `flow_direction` discriminant.  Alignment is intentionally
    /// excluded because it does not affect glyph shapes or advances.
    shape_cache_style_hash: u64,
    /// Cached shaped runs from the last `shape_and_layout` call.
    shape_cache_runs: Vec<ShapedRun>,
}

#[cfg(feature = "pure")]
impl Pipeline {
    /// Returns a fluent builder for constructing a [`Pipeline`].
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder { font_data: None }
    }

    /// Lists the optional features compiled into this build of oxitext.
    ///
    /// The returned slice contains zero or more of: `"pure"`, `"sdf"`, `"icu"`,
    /// `"parallel"`.
    pub fn available_features() -> &'static [&'static str] {
        const FEATURES: &[&str] = &[
            #[cfg(feature = "pure")]
            "pure",
            #[cfg(feature = "sdf")]
            "sdf",
            #[cfg(feature = "icu")]
            "icu",
            #[cfg(feature = "parallel")]
            "parallel",
        ];
        FEATURES
    }

    /// Returns `true` when this build can render COLRv0/v1 color glyphs.
    ///
    /// Always returns `true` in a `pure`-feature build (color detection and
    /// compositing are unconditionally compiled in).
    pub fn renders_color_glyphs(&self) -> bool {
        true
    }

    /// Compute a style hash that captures all fields affecting shaping.
    ///
    /// `font_size` and `flow_direction` are included; `alignment` is excluded
    /// because it does not change glyph shapes or advances.
    fn style_hash(style: &TextStyle) -> u64 {
        // FNV-1a inspired mixing (no external dep needed for this small hash).
        let mut h: u64 = 14_695_981_039_346_656_037;
        let mix = |h: &mut u64, v: u64| {
            *h ^= v;
            *h = h.wrapping_mul(1_099_511_628_211);
        };
        mix(&mut h, style.font_size.to_bits() as u64);
        mix(&mut h, style.flow_direction as u64);
        h
    }

    /// Invalidate the shape cache.
    ///
    /// Must be called whenever the font data or fallback chain changes so that
    /// the next `shape_and_layout` call re-shapes from scratch.
    fn invalidate_shape_cache(&mut self) {
        self.shape_cache_text.clear();
        self.shape_cache_style_hash = 0;
        self.shape_cache_runs.clear();
    }

    /// Creates a pipeline using the first font face in `font_db`.
    ///
    /// The font file pointed to by the first [`FaceInfo`] entry is read from
    /// disk and loaded into the pipeline.
    ///
    /// # Errors
    /// - [`OxiTextError::FontNotFound`] if `font_db` is empty.
    /// - [`OxiTextError::Other`] if the font file cannot be read or is invalid.
    ///
    /// [`FaceInfo`]: oxifont::FaceInfo
    pub fn new(font_db: &oxifont::FontDatabase) -> Result<Self, OxiTextError> {
        use oxifont::FontCatalog as _;
        let faces = font_db.faces();
        let first = faces.first().ok_or(OxiTextError::FontNotFound)?;
        let bytes = std::fs::read(&first.path)
            .map_err(|e| OxiTextError::Other(format!("font read error: {e}")))?;
        Self::from_bytes(&bytes)
    }

    /// Creates a pipeline by querying the system font database for a font
    /// matching `family` (e.g. `"Arial"`, `"DejaVu Sans"`, `"Helvetica"`).
    ///
    /// Uses [`oxifont::FontDatabase::system`] to enumerate installed fonts, then
    /// selects the best match via CSS Fonts Level 4 family matching.  If no font
    /// matches `family` the first available system font is used as a fallback.
    ///
    /// # Errors
    /// - [`OxiTextError::FontNotFound`] if no system fonts are discoverable.
    /// - [`OxiTextError::Other`] if the selected font file cannot be read or
    ///   parsed.
    ///
    /// # Example
    /// ```rust,no_run
    /// use oxitext::Pipeline;
    ///
    /// let mut pipeline = Pipeline::new_with_system_font("DejaVu Sans")
    ///     .expect("system font not found");
    /// ```
    pub fn new_with_system_font(family: &str) -> Result<Self, OxiTextError> {
        use oxifont::{FontCatalog as _, FontDatabase, FontQuery};

        // Scan the system font directories with the pure filesystem adapter.
        let db = FontDatabase::system()
            .map_err(|e| OxiTextError::Other(format!("font db scan failed: {e}")))?;

        // Select the best CSS match for `family`; fall back to the first face.
        let face = db
            .find_css(&FontQuery::new().family(family))
            .or_else(|| db.faces().first())
            .ok_or(OxiTextError::FontNotFound)?;

        let bytes = std::fs::read(&face.path)
            .map_err(|e| OxiTextError::Other(format!("read font {:?}: {e}", face.path)))?;

        Self::from_bytes(&bytes)
    }

    /// Creates a pipeline from raw font bytes (TTF or OTF).
    ///
    /// This constructor bypasses font discovery entirely and is the recommended
    /// approach in tests and environments without system fonts. Font metrics
    /// are extracted once here for accurate line spacing.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Other`] if the font bytes cannot be parsed.
    pub fn from_bytes(font_bytes: &[u8]) -> Result<Self, OxiTextError> {
        validate_font(font_bytes)?;
        let vmetrics = extract_vertical_metrics(font_bytes);
        Ok(Self {
            shaper: ShaperKind::Default(Box::new(SwashShaper::new())),
            engine: LayoutEngine::new(),
            rasterizer: FontdueRasterizer::new(),
            font_data: std::sync::Arc::from(font_bytes),
            vmetrics,
            fallback_fonts: Vec::new(),
            shape_cache_text: String::new(),
            shape_cache_style_hash: 0,
            shape_cache_runs: Vec::new(),
        })
    }

    /// Configures a font fallback chain.
    ///
    /// When a glyph maps to GID 0 (`.notdef`) in the primary font, the
    /// pipeline re-shapes that character cluster through each fallback font
    /// in order until a non-zero GID is produced.  The first font that yields
    /// a valid glyph wins; if none do, the `.notdef` glyph from the primary
    /// font is kept.
    ///
    /// Call this before any render/shape operations. Passing an empty `Vec`
    /// clears the fallback chain.
    pub fn set_fallback_fonts(&mut self, fonts: Vec<Vec<u8>>) {
        self.fallback_fonts = fonts
            .into_iter()
            .map(|v| std::sync::Arc::from(v.as_slice()) as std::sync::Arc<[u8]>)
            .collect();
        // Fallback chain change affects shaping results — invalidate cache.
        self.invalidate_shape_cache();
    }

    /// Shape text using script-aware font fallback.
    ///
    /// Each run of consecutive characters that map to the same font in the
    /// fallback chain is shaped together. This ensures multilingual text uses
    /// appropriate fonts for each script rather than forcing everything through
    /// the primary font and patching `.notdef` glyphs after the fact.
    ///
    /// The selection strategy is:
    /// 1. Check the primary font via cmap lookup (`ttf_parser`).
    /// 2. If the character is absent, walk the fallback chain in order and
    ///    use the first font that has a cmap entry.
    /// 3. If no font has the character, fall back to the primary font.
    ///
    /// All shaping is LTR; for bidirectional or vertical text use
    /// [`Self::shape_and_layout`] / [`Self::render`] which apply UAX #9.
    ///
    /// # Errors
    /// Propagates errors from the underlying shaper.
    pub fn shape_with_fallback(
        &mut self,
        text: &str,
        px_size: f32,
    ) -> Result<oxitext_shape::ShapeResult, OxiTextError> {
        // Collect owned clones of primary and fallback data up-front so that
        // the borrow checker allows calling `&mut self` methods afterwards.
        let primary_data: Vec<u8> = self.font_data.as_ref().to_vec();
        let fallback_owned: Vec<Vec<u8>> = self
            .fallback_fonts
            .iter()
            .map(|arc| arc.as_ref().to_vec())
            .collect();

        // Fast path: no fallback fonts configured — shape everything with primary.
        if fallback_owned.is_empty() {
            let glyphs = self.shape_segment(text, px_size, &primary_data, 0)?;
            return Ok(oxitext_shape::ShapeResult {
                glyphs,
                script_detected: None,
                direction: oxitext_shape::ShapeDirection::default(),
                missing_codepoints: vec![],
                cluster_boundaries: vec![],
            });
        }

        // Walk characters and collect run boundaries where the selected font changes.
        // Each entry is (run_start_byte, run_end_byte, font_idx).
        let mut runs: Vec<(usize, usize, usize)> = Vec::new();
        let mut current_font_idx: usize = best_font_for_char(
            text.chars().next().unwrap_or('\0'),
            &primary_data,
            &fallback_owned,
        );
        let mut run_start: usize = 0;

        for (byte_pos, ch) in text.char_indices() {
            let font_idx = best_font_for_char(ch, &primary_data, &fallback_owned);
            if font_idx != current_font_idx {
                // Close the current run (byte_pos > run_start guaranteed when
                // we have processed at least one character before this one).
                if byte_pos > run_start {
                    runs.push((run_start, byte_pos, current_font_idx));
                }
                current_font_idx = font_idx;
                run_start = byte_pos;
            }
        }
        // Close the final run.
        if run_start < text.len() {
            runs.push((run_start, text.len(), current_font_idx));
        }

        // Shape each run with the chosen font.
        let mut all_glyphs: Vec<ShapedGlyph> = Vec::new();
        for (start, end, font_idx) in runs {
            let segment = &text[start..end];
            let font_data_for_run: &Vec<u8> = if font_idx == 0 {
                &primary_data
            } else {
                &fallback_owned[font_idx - 1]
            };
            let glyphs = self.shape_segment(segment, px_size, font_data_for_run, start)?;
            all_glyphs.extend(glyphs);
        }

        Ok(oxitext_shape::ShapeResult {
            glyphs: all_glyphs,
            script_detected: None,
            direction: oxitext_shape::ShapeDirection::default(),
            missing_codepoints: vec![],
            cluster_boundaries: vec![],
        })
    }

    /// Shape a single text segment using `font_data`.  Cluster byte offsets in
    /// the returned glyphs are rebased by `byte_offset` so they refer to
    /// positions in the original (full) text rather than the segment slice.
    fn shape_segment(
        &mut self,
        segment: &str,
        px_size: f32,
        font_data: &[u8],
        byte_offset: usize,
    ) -> Result<Vec<ShapedGlyph>, OxiTextError> {
        let font_arc: std::sync::Arc<[u8]> = std::sync::Arc::from(font_data);

        let mut glyphs: Vec<ShapedGlyph> = match &mut self.shaper {
            ShaperKind::Default(s) => s
                .shape(segment, std::sync::Arc::clone(&font_arc), px_size)
                .map(|r| r.glyphs.into_vec())?,
            ShaperKind::Custom(s) => s.shape_with_direction(&font_arc, segment, px_size, false),
        };

        // Rebase cluster byte offsets to the full-text coordinate space.
        let offset_u32 = byte_offset as u32;
        for g in &mut glyphs {
            g.cluster += offset_u32;
        }
        Ok(glyphs)
    }

    /// Creates a [`Pipeline`] that delegates shaping and rasterization to
    /// custom backend implementations.
    ///
    /// The `shaper` receives the raw font bytes of the primary font and
    /// returns [`ShapedGlyph`] slices; the `rasterizer` is used for all glyph
    /// rendering calls.  Custom shapers follow a **simplified LTR-only** code
    /// path — bidi itemization and ICU script-itemization are not applied when
    /// a custom shaper is in use.
    ///
    /// # Errors
    /// Returns [`OxiTextError::Other`] if `font_data` cannot be parsed.
    pub fn with_backend(
        font_data: Vec<u8>,
        shaper: Box<dyn ShapeBackend + Send + Sync>,
        rasterizer: FontdueRasterizer,
    ) -> Result<Self, OxiTextError> {
        validate_font(&font_data)?;
        let vmetrics = extract_vertical_metrics(&font_data);
        Ok(Self {
            shaper: ShaperKind::Custom(shaper),
            engine: LayoutEngine::new(),
            rasterizer,
            font_data: std::sync::Arc::from(font_data.as_slice()),
            vmetrics,
            fallback_fonts: Vec::new(),
            shape_cache_text: String::new(),
            shape_cache_style_hash: 0,
            shape_cache_runs: Vec::new(),
        })
    }

    /// Returns the font's vertical metrics, if they could be extracted.
    pub fn font_metrics(&self) -> Option<&FontVerticalMetrics> {
        self.vmetrics.as_ref()
    }

    /// Returns `true` if `text` contains any right-to-left runs per UAX #9.
    ///
    /// When `true`, [`Self::render`] and [`Self::shape_and_layout`] automatically
    /// apply bidi-itemized shaping via UAX #9, shaping each run in its resolved
    /// direction and rebasing cluster offsets before handing the runs to the
    /// layout engine for UAX #9 L2 visual reordering.
    pub fn has_rtl(&self, text: &str) -> bool {
        let para = BidiParagraph::new(text, None);
        // Either the resolved base direction is RTL, or any embedded run has an
        // odd (RTL) level — the latter catches RTL embedded in an LTR base.
        para.is_rtl() || para.runs().iter().any(|r| r.level % 2 == 1)
    }

    /// Shapes `text_slice` with fallback support, returning a `ShapedRun` whose
    /// glyphs use the primary font except where GID is 0 (`.notdef`), in which
    /// case each contiguous notdef cluster is re-shaped through the fallback
    /// chain in order.
    ///
    /// The `rtl` flag controls shaping direction; `cluster_offset` is added to
    /// every glyph's `cluster` field so the returned run refers to byte positions
    /// in the full source string rather than in the sub-slice.
    fn shape_run_with_notdef_fallback(
        &mut self,
        text_slice: &str,
        px_size: f32,
        rtl: bool,
        cluster_offset: u32,
    ) -> Result<ShapedRun, OxiTextError> {
        // Shape with primary font.
        let mut run = match &mut self.shaper {
            ShaperKind::Default(s) => s.shape_with_direction(
                text_slice,
                std::sync::Arc::clone(&self.font_data),
                px_size,
                rtl,
            )?,
            ShaperKind::Custom(s) => {
                let glyphs = s.shape_with_direction(&self.font_data, text_slice, px_size, rtl);
                ShapedRun {
                    glyphs: glyphs.into(),
                    font_data: std::sync::Arc::clone(&self.font_data),
                }
            }
        };

        // Rebase cluster offsets.
        for g in &mut run.glyphs {
            g.cluster += cluster_offset;
        }

        // Early-exit when there are no fallback fonts or no notdef glyphs.
        if self.fallback_fonts.is_empty() || run.glyphs.iter().all(|g| g.gid != 0) {
            return Ok(run);
        }

        // For each glyph with gid == 0, try fallback fonts in order.
        // We operate on the glyph array by index to avoid overlapping borrows.
        let cluster_offset_u32 = cluster_offset;
        let fallbacks: Vec<std::sync::Arc<[u8]>> = self.fallback_fonts.clone();
        let n = run.glyphs.len();

        let mut idx = 0;
        while idx < n {
            if run.glyphs[idx].gid != 0 {
                idx += 1;
                continue;
            }
            let notdef_cluster = run.glyphs[idx].cluster;
            // Determine the byte range of this cluster in text_slice.
            let cluster_start = (notdef_cluster.saturating_sub(cluster_offset_u32)) as usize;
            // Next cluster boundary: scan forward for a glyph with a different cluster.
            let next_cluster = run
                .glyphs
                .iter()
                .skip(idx + 1)
                .find(|g2| g2.cluster != notdef_cluster)
                .map(|g2| (g2.cluster.saturating_sub(cluster_offset_u32)) as usize);
            let cluster_end = next_cluster
                .unwrap_or(text_slice.len())
                .min(text_slice.len());
            if cluster_start >= cluster_end {
                idx += 1;
                continue;
            }
            let slice = match text_slice.get(cluster_start..cluster_end) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    idx += 1;
                    continue;
                }
            };

            // Try each fallback font.
            'fallback_loop: for fb_data in &fallbacks {
                let fb_glyphs: Vec<ShapedGlyph> = match &mut self.shaper {
                    ShaperKind::Default(s) => {
                        match s.shape_with_direction(
                            slice,
                            std::sync::Arc::clone(fb_data),
                            px_size,
                            rtl,
                        ) {
                            Ok(r) => r.glyphs.into_vec(),
                            Err(_) => continue,
                        }
                    }
                    ShaperKind::Custom(s) => s.shape_with_direction(fb_data, slice, px_size, rtl),
                };
                if let Some(winner) = fb_glyphs.into_iter().find(|g| g.gid != 0) {
                    run.glyphs[idx].gid = winner.gid;
                    run.glyphs[idx].x_advance = winner.x_advance;
                    run.glyphs[idx].y_advance = winner.y_advance;
                    run.glyphs[idx].x_offset = winner.x_offset;
                    run.glyphs[idx].y_offset = winner.y_offset;
                    // Update the run-level font_data Arc so the rasterizer uses
                    // the fallback font for this glyph.  Note: ShapedRun holds one
                    // font_data for the whole run; callers needing per-glyph font
                    // selection should use `render_styled` with separate TextRuns.
                    run.font_data = std::sync::Arc::clone(fb_data);
                    break 'fallback_loop;
                }
            }
            idx += 1;
        }

        Ok(run)
    }

    /// Dispatch to either CLDR-aware layout (`icu` feature) or the built-in
    /// UAX #14 layout (default).  The signatures are identical so call sites
    /// remain uniform regardless of the feature flag.
    fn layout_dispatch(
        &mut self,
        text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
    ) -> Result<LayoutResult, OxiTextError> {
        #[cfg(feature = "icu")]
        {
            self.engine
                .layout_cldr(text, runs, constraints, alignment, self.vmetrics.as_ref())
        }
        #[cfg(not(feature = "icu"))]
        {
            self.engine
                .layout(text, runs, constraints, alignment, self.vmetrics.as_ref())
        }
    }

    /// Shapes and lays out `text` without rasterizing, returning the structured
    /// [`LayoutResult`] (positioned glyphs + line/paragraph metrics).
    ///
    /// For LTR-only text this is a single `shape` call (fast path). When
    /// `needs_bidi(text)` is true the method itemizes runs via UAX #9, shapes
    /// each run with its resolved direction, rebases cluster byte offsets to
    /// full-text positions, then passes all runs in logical order to the
    /// layout engine which applies UAX #9 L2 visual reordering per line.
    ///
    /// When the `icu` feature is enabled, LTR text is also itemized by Unicode
    /// script so each script run is shaped with the appropriate OpenType script
    /// tag (e.g. `latn`, `arab`, `hani`), and CLDR line breaking is used
    /// instead of UAX #14.
    ///
    /// When a custom shaper was set via [`Self::with_backend`], a simplified
    /// LTR-only code path is used (no bidi/ICU itemization).
    ///
    /// Shaped runs are cached keyed by source text and a hash of the
    /// shaping-relevant style fields (`font_size`, `flow_direction`).  On a
    /// cache hit the layout engine is re-invoked with the cached runs (which
    /// re-applies alignment and wrapping at negligible cost) but shaping is
    /// skipped entirely.
    ///
    /// # Errors
    /// Propagates errors from the shaper and layout engine.
    pub fn shape_and_layout(
        &mut self,
        text: &str,
        style: &TextStyle,
    ) -> Result<LayoutResult, OxiTextError> {
        let constraints = LayoutConstraints {
            max_width: style.max_width,
            font_size: style.font_size,
        };

        // Custom shaper: simplified LTR-only path (no bidi/ICU, no cache).
        if matches!(self.shaper, ShaperKind::Custom(_)) {
            let run = self.shape_run_with_notdef_fallback(text, style.font_size, false, 0)?;
            return self.layout_dispatch(text, &[run], &constraints, style.alignment);
        }

        // Vertical text path: shape as LTR (no bidi), lay out top-to-bottom.
        // ICU line-break is not applicable here; use the dedicated vertical engine.
        if style.flow_direction == FlowDirection::Vertical {
            let run = self.shape_run_with_notdef_fallback(text, style.font_size, false, 0)?;
            return self.engine.layout_vertical(
                text,
                &[run],
                style.max_width, // repurposed as max column height (0 = unbounded)
                style.font_size,
                self.vmetrics.as_ref(),
            );
        }

        // ── Shape cache check (Feature 2) ────────────────────────────────────
        // Skip re-shaping when the source text and all shaping-relevant style
        // fields are identical to the previous call.  Alignment and max_width
        // are intentionally excluded from the key because they do not affect
        // glyph shapes or advances.
        let style_hash = Self::style_hash(style);
        if text == self.shape_cache_text
            && style_hash == self.shape_cache_style_hash
            && !self.shape_cache_runs.is_empty()
        {
            let cached: Vec<ShapedRun> = self.shape_cache_runs.clone();
            return self.layout_dispatch(text, &cached, &constraints, style.alignment);
        }

        // Fast path: LTR-only text — single shape call, no bidi overhead.
        if !oxitext_layout::needs_bidi(text) {
            // ICU script itemization: when the `icu` feature is enabled and
            // text spans multiple scripts, shape each script run separately
            // with the appropriate OpenType script tag.
            #[cfg(feature = "icu")]
            {
                let script_runs = itemize_by_script(text);
                if script_runs.len() > 1 {
                    let mut runs: Vec<ShapedRun> = Vec::with_capacity(script_runs.len());
                    for (run_start, run_end, script_tag) in &script_runs {
                        let run_text = &text[*run_start..*run_end];
                        if run_text.is_empty() {
                            continue;
                        }
                        // ICU path uses shape_request directly on the default shaper.
                        if let ShaperKind::Default(s) = &mut self.shaper {
                            let req = oxitext_shape::ShapeRequest::builder()
                                .text(run_text)
                                .font_data(&self.font_data)
                                .px_size(style.font_size)
                                .script(*script_tag)
                                .build()
                                .map_err(|e| OxiTextError::Shaping(e.to_string()))?;
                            let mut glyphs = s.shape_request(&req)?;
                            // Rebase cluster offsets from sub-slice to full-text positions.
                            for g in &mut glyphs {
                                g.cluster += *run_start as u32;
                            }
                            runs.push(ShapedRun {
                                glyphs: glyphs.into(),
                                font_data: std::sync::Arc::clone(&self.font_data),
                            });
                        }
                    }
                    // Cache the multi-script runs.
                    self.shape_cache_text = text.to_owned();
                    self.shape_cache_style_hash = style_hash;
                    self.shape_cache_runs = runs.clone();
                    return self.layout_dispatch(text, &runs, &constraints, style.alignment);
                }
            }
            // Single-script LTR (or non-icu build): one shape call with fallback.
            let run = self.shape_run_with_notdef_fallback(text, style.font_size, false, 0)?;
            let runs = vec![run];
            self.shape_cache_text = text.to_owned();
            self.shape_cache_style_hash = style_hash;
            self.shape_cache_runs = runs.clone();
            return self.layout_dispatch(text, &runs, &constraints, style.alignment);
        }

        // Bidi path: itemize by UAX #9 level runs, shape each in its resolved direction.
        let para = oxitext_layout::bidi::BidiParagraph::new(text, None);
        // Sort visual-order runs back to logical (source) order for shaping.
        let mut bidi_runs: Vec<oxitext_layout::bidi::BidiRun> = para.runs().to_vec();
        bidi_runs.sort_by_key(|r| r.start);

        let mut runs: Vec<ShapedRun> = Vec::with_capacity(bidi_runs.len());
        for br in &bidi_runs {
            let slice = &text[br.start..br.end];
            if slice.is_empty() {
                continue;
            }
            let rtl = br.level % 2 == 1;
            let run =
                self.shape_run_with_notdef_fallback(slice, style.font_size, rtl, br.start as u32)?;
            runs.push(run);
        }

        // Cache the bidi-shaped runs.
        self.shape_cache_text = text.to_owned();
        self.shape_cache_style_hash = style_hash;
        self.shape_cache_runs = runs.clone();

        self.layout_dispatch(text, &runs, &constraints, style.alignment)
    }

    /// Lays out and rasterizes multiple paragraphs separated by blank lines.
    ///
    /// Each element of `paragraphs` is rendered independently with `style`,
    /// and the results are stitched into a single [`RenderResult`].  Paragraphs
    /// are separated by `style.font_size * 0.5` extra vertical space beyond the
    /// normal line spacing.
    ///
    /// Empty paragraph strings produce a single blank line worth of spacing.
    ///
    /// # Errors
    /// Propagates errors from the shaper, layout engine, and rasterizer.
    pub fn render_paragraph(
        &mut self,
        paragraphs: &[&str],
        style: &TextStyle,
    ) -> Result<RenderResult, OxiTextError> {
        let para_spacing = style.font_size * 0.5;
        let mut all_glyphs: Vec<PositionedGlyph> = Vec::new();
        let mut all_bitmaps: Vec<Bitmap> = Vec::new();
        let mut all_outputs: Vec<RenderOutput> = Vec::new();
        let mut all_lines: Vec<Line> = Vec::new();
        let mut y_offset = 0.0_f32;
        let mut total_width = 0.0_f32;
        let mut total_lines: usize = 0;
        let mut has_overflow = false;

        for &para_text in paragraphs {
            // Empty paragraph: advance by one blank-line height + spacing.
            if para_text.is_empty() {
                y_offset += style.font_size + para_spacing;
                total_lines += 1;
                continue;
            }

            let result = self.render(para_text, style)?;

            // Offset every glyph's y-position into the accumulated canvas.
            let glyph_base = all_glyphs.len();
            for mut g in result.glyphs {
                g.pos.1 += y_offset;
                all_glyphs.push(g);
            }
            all_bitmaps.extend(result.bitmaps);
            all_outputs.extend(result.outputs);

            // Offset lines and rebase their glyph indices.
            for mut line in result.lines {
                line.metrics.baseline_y += y_offset;
                // Shift glyph index range by the number of glyphs already accumulated.
                line.glyph_start += glyph_base;
                line.glyph_end += glyph_base;
                all_lines.push(line);
            }

            total_width = total_width.max(result.metrics.total_width);
            has_overflow |= result.metrics.overflow;
            total_lines += result.metrics.line_count;
            y_offset += result.metrics.total_height + para_spacing;
        }

        let metrics = ParagraphMetrics {
            total_width,
            total_height: y_offset,
            line_count: total_lines,
            overflow: has_overflow,
            truncated: false,
        };

        Ok(RenderResult {
            glyphs: all_glyphs,
            bitmaps: all_bitmaps,
            outputs: all_outputs,
            lines: all_lines,
            metrics,
            decoration_rects: Vec::new(),
        })
    }

    /// Renders a sequence of [`TextRun`]s with mixed fonts, sizes, and styles.
    ///
    /// Each run carries its own font bytes ([`TextRun::font_data`]) and
    /// [`TextRun::style`]; they are shaped independently, cluster offsets are
    /// rebased into a unified byte space, and the combined shaped output is
    /// laid out together within `max_width`.
    ///
    /// This is an LTR-only first pass.  Bidi/vertical support for styled runs
    /// is planned for a future milestone.
    ///
    /// # Errors
    /// Propagates errors from the shaper, layout engine, and rasterizer.
    pub fn render_styled(
        &mut self,
        runs: &[TextRun],
        max_width: f32,
    ) -> Result<RenderResult, OxiTextError> {
        // Build a unified text string and pre-compute per-run byte offsets.
        let mut unified_text = String::new();
        let mut run_offsets: Vec<(usize, usize)> = Vec::with_capacity(runs.len()); // (start, end)
        for run in runs {
            let start = unified_text.len();
            unified_text.push_str(&run.text);
            run_offsets.push((start, unified_text.len()));
        }

        // Shape each run with its own font/size and rebase cluster offsets.
        let mut shaped_runs: Vec<ShapedRun> = Vec::with_capacity(runs.len());
        for (run, &(byte_start, _byte_end)) in runs.iter().zip(run_offsets.iter()) {
            if run.text.is_empty() {
                continue;
            }
            // Shape using the default shaper (custom shapers in this impl
            // are only available via with_backend, not render_styled).
            let mut shaped = match &mut self.shaper {
                ShaperKind::Default(s) => s.shape(
                    &run.text,
                    std::sync::Arc::clone(&run.font_data),
                    run.style.font_size,
                )?,
                ShaperKind::Custom(s) => {
                    let glyphs = s.shape(&run.font_data, &run.text, run.style.font_size);
                    ShapedRun {
                        glyphs: glyphs.into(),
                        font_data: std::sync::Arc::clone(&run.font_data),
                    }
                }
            };
            // Rebase cluster offsets from run-local to unified-text positions.
            for g in &mut shaped.glyphs {
                g.cluster += byte_start as u32;
            }
            shaped_runs.push(shaped);
        }

        // Lay out the combined runs.
        // Use the style of the first run for global layout options; fall back
        // to defaults if no runs were provided.
        let first_style = runs.first().map(|r| &r.style).cloned().unwrap_or_default();
        let constraints = LayoutConstraints {
            max_width,
            font_size: first_style.font_size,
        };
        let layout = self.engine.layout(
            &unified_text,
            &shaped_runs,
            &constraints,
            first_style.alignment,
            self.vmetrics.as_ref(),
        )?;

        // Rasterize each glyph using its run's font_data.
        let (bitmaps, outputs) = self.rasterize_glyphs(&layout.glyphs)?;

        Ok(RenderResult {
            glyphs: layout.glyphs,
            bitmaps,
            outputs,
            lines: layout.lines,
            metrics: layout.metrics,
            decoration_rects: Vec::new(),
        })
    }

    /// Measures `text` under `style` without rasterizing, returning the
    /// [`ParagraphMetrics`] (total width/height, line count, overflow flag).
    ///
    /// # Errors
    /// Propagates errors from the shaper and layout engine.
    pub fn measure(
        &mut self,
        text: &str,
        style: &TextStyle,
    ) -> Result<ParagraphMetrics, OxiTextError> {
        Ok(self.shape_and_layout(text, style)?.metrics)
    }

    /// Rasterizes glyphs from `layout.glyphs` with deduplication and optional
    /// parallel execution (Feature 3 + 4).
    ///
    /// Returns `(bitmaps, outputs)` both with length == `layout.glyphs.len()`.
    ///
    /// - Unique `(gid, font_size_bits, font_ptr)` triples are rasterized once.
    /// - Color glyphs are detected via [`oxitext_raster::detect_color_glyph_type`]
    ///   and rendered with [`oxitext_raster::render_colr_v1`] when the type is
    ///   `ColrV0` or `ColrV1`; that entry point drives the full COLR paint graph
    ///   (gradients, transforms and composite modes included) for both table
    ///   versions.
    /// - When compiled with `--features parallel` (and not targeting WASM), the
    ///   unique-glyph set is rasterized in parallel via rayon.
    fn rasterize_glyphs(
        &self,
        glyphs: &[PositionedGlyph],
    ) -> Result<(Vec<Bitmap>, Vec<RenderOutput>), OxiTextError> {
        use std::collections::HashMap;

        // Build dedup table: key = (gid, font_size_bits, font Arc ptr address).
        type DedupeKey = (u16, u32, usize);

        // Collect the unique keys preserving insertion order for determinism.
        let mut key_order: Vec<DedupeKey> = Vec::new();
        let mut key_set: HashMap<DedupeKey, ()> = HashMap::new();
        for g in glyphs {
            let key: DedupeKey = (
                g.gid,
                g.font_size.to_bits(),
                std::sync::Arc::as_ptr(&g.font_data) as *const u8 as usize,
            );
            if key_set.insert(key, ()).is_none() {
                key_order.push(key);
            }
        }

        // Gather font_data references for the rasterisation pass.
        // We need the Arc for each unique key but glyph list is the cheapest source.
        let key_to_font: HashMap<DedupeKey, std::sync::Arc<[u8]>> = glyphs
            .iter()
            .map(|g| {
                let key: DedupeKey = (
                    g.gid,
                    g.font_size.to_bits(),
                    std::sync::Arc::as_ptr(&g.font_data) as *const u8 as usize,
                );
                (key, std::sync::Arc::clone(&g.font_data))
            })
            .collect();

        // Rasterize each unique glyph once.
        //
        // Parallel path (`parallel` feature, non-WASM): each rayon worker thread
        // owns its own `FontdueRasterizer` (created by `map_init`) so there is no
        // shared Mutex contention and rasterisation truly runs in parallel.
        //
        // Sequential fallback: use `self.rasterizer` directly.

        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        let results: Vec<(DedupeKey, RenderOutput)> = {
            use rayon::prelude::*;
            key_order
                .par_iter()
                .map_init(FontdueRasterizer::new, |thread_rast, &key| {
                    key_to_font.get(&key).map(|font_data| {
                        let (gid, px_bits, _) = key;
                        let px_size = f32::from_bits(px_bits);
                        (key, rasterize_single(gid, font_data, px_size, thread_rast))
                    })
                })
                .filter_map(|x| x)
                .collect()
        };

        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        let results: Vec<(DedupeKey, RenderOutput)> = key_order
            .iter()
            .filter_map(|&key| {
                let font_data = key_to_font.get(&key)?;
                let (gid, px_bits, _) = key;
                let px_size = f32::from_bits(px_bits);
                let output = rasterize_single(gid, font_data, px_size, &self.rasterizer);
                Some((key, output))
            })
            .collect();

        // Build lookup map from unique key → RenderOutput.
        let dedup_map: HashMap<DedupeKey, RenderOutput> = results.into_iter().collect();

        // Assemble per-glyph bitmaps and outputs in layout order.
        let mut bitmaps: Vec<Bitmap> = Vec::with_capacity(glyphs.len());
        let mut outputs: Vec<RenderOutput> = Vec::with_capacity(glyphs.len());
        for g in glyphs {
            let key: DedupeKey = (
                g.gid,
                g.font_size.to_bits(),
                std::sync::Arc::as_ptr(&g.font_data) as *const u8 as usize,
            );
            let output = dedup_map
                .get(&key)
                .cloned()
                .unwrap_or(RenderOutput::Greyscale(Bitmap {
                    width: 0,
                    height: 0,
                    pixels: Vec::new(),
                }));
            let bm = match &output {
                RenderOutput::Greyscale(b) => b.clone(),
                // Color glyphs: bitmaps entry is empty; callers use `outputs`.
                _ => Bitmap {
                    width: 0,
                    height: 0,
                    pixels: Vec::new(),
                },
            };
            bitmaps.push(bm);
            outputs.push(output);
        }

        Ok((bitmaps, outputs))
    }

    /// Renders `text` with the given style, returning positioned glyphs,
    /// per-glyph bitmaps, and line/paragraph metrics.
    ///
    /// The returned [`RenderResult::glyphs`], [`RenderResult::bitmaps`], and
    /// [`RenderResult::outputs`] slices always have the same length. Text is
    /// wrapped at UAX #14 opportunities (or CLDR when `icu` feature enabled)
    /// and aligned per `style.alignment`.
    ///
    /// # Errors
    /// Propagates errors from the shaper, layout engine, and rasterizer.
    pub fn render(&mut self, text: &str, style: &TextStyle) -> Result<RenderResult, OxiTextError> {
        let layout = self.shape_and_layout(text, style)?;
        let (bitmaps, outputs) = self.rasterize_glyphs(&layout.glyphs)?;
        Ok(RenderResult {
            glyphs: layout.glyphs,
            bitmaps,
            outputs,
            lines: layout.lines,
            metrics: layout.metrics,
            decoration_rects: Vec::new(),
        })
    }

    /// Renders `text` and composites it onto an RGBA canvas sized to the laid
    /// out text, returning a ready-to-use [`ColorBitmap`].
    ///
    /// The canvas dimensions are derived from the paragraph metrics (rounded
    /// up). Glyphs are painted in `text_color` over `bg_color`.
    ///
    /// # Errors
    /// Propagates errors from the shaper, layout engine, and rasterizer.
    pub fn render_to_image(
        &mut self,
        text: &str,
        style: &TextStyle,
        bg_color: Rgba8,
        text_color: Rgba8,
    ) -> Result<ColorBitmap, OxiTextError> {
        let result = self.render(text, style)?;
        // Canvas width: prefer the wrap column when wrapping is enabled, else
        // the natural text width. Height from total line height.
        let width = if style.max_width > 0.0 {
            style.max_width.ceil() as u32
        } else {
            result.metrics.total_width.ceil() as u32
        }
        .max(1);
        let height = result.metrics.total_height.ceil() as u32 + style.font_size.ceil() as u32;
        let height = height.max(1);
        Ok(result.composite_to_rgba(width, height, bg_color, text_color))
    }

    /// Benchmark the full render pipeline (shape → layout) for the given text.
    ///
    /// Runs [`Self::measure`] (shape + layout, no rasterization) `iterations`
    /// times and returns the average duration per iteration.
    ///
    /// # Arguments
    /// - `text`: the text to benchmark
    /// - `style`: text style (font size, max width, etc.)
    /// - `iterations`: how many times to run (clamped to 1 minimum)
    ///
    /// This is a convenience method for rough profiling — use a proper
    /// benchmarking tool (e.g. criterion) for precise measurements.
    pub fn benchmark(
        &mut self,
        text: &str,
        style: &TextStyle,
        iterations: usize,
    ) -> std::time::Duration {
        let iterations = iterations.max(1);
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            // measure() = shape_and_layout() without bitmap allocation.
            let _ = self.measure(text, style);
        }
        start.elapsed() / iterations as u32
    }

    /// Profile the pipeline and return a rough time breakdown.
    ///
    /// Returns `(shape_layout_duration, remainder_duration, total_duration)`.
    ///
    /// - `shape_layout_duration`: time for one `shape_and_layout` call.
    /// - `remainder_duration`: extra time spent by the second `measure` call
    ///   (typically layout re-run on cached shapes) minus the first call.
    /// - `total_duration`: wall time across both calls.
    ///
    /// # Note
    /// The second `measure` call hits the shape cache, so `remainder_duration`
    /// reflects layout + cache-lookup overhead rather than a true raster phase.
    /// Use criterion for precise per-phase measurements.
    pub fn profile(
        &mut self,
        text: &str,
        style: &TextStyle,
    ) -> (
        std::time::Duration,
        std::time::Duration,
        std::time::Duration,
    ) {
        let t0 = std::time::Instant::now();
        let _ = self.shape_and_layout(text, style);
        let t1 = std::time::Instant::now();
        // Second call hits the shape cache; measures layout+cache overhead.
        let _ = self.measure(text, style);
        let t2 = std::time::Instant::now();

        let shape_layout = t1 - t0;
        let total = t2 - t0;
        let remainder = total.saturating_sub(shape_layout);

        (shape_layout, remainder, total)
    }
}

#[cfg(all(feature = "pure", feature = "sdf"))]
impl Pipeline {
    /// Shape and layout text, then populate an SDF atlas with the required glyphs.
    ///
    /// Each unique `(glyph_id, font_size)` pair from the layout that is not
    /// already present in the atlas is rendered analytically into an SDF tile
    /// and inserted with [`oxitext_sdf::SdfAtlas::add_tile`].
    ///
    /// Returns the [`LayoutResult`] (for UV lookup) and a list of glyph IDs
    /// that were newly packed into the atlas during this call.
    ///
    /// # Usage
    ///
    /// ```rust
    /// use oxitext::{Pipeline, TextStyle};
    /// use oxitext_sdf::SdfAtlas;
    ///
    /// // A bundled Noto Sans font keeps this example self-contained and
    /// // deterministic (no filesystem or network access needed).
    /// let mut pipeline =
    ///     Pipeline::from_bytes(oxifont_bundled::NOTO_SANS_REGULAR).expect("valid bundled font");
    /// let mut atlas = SdfAtlas::new(512, 512);
    /// let style = TextStyle::default();
    ///
    /// let (layout, new_ids) = pipeline
    ///     .render_to_sdf_atlas("Hello", &style, &mut atlas)
    ///     .expect("render_to_sdf_atlas failed");
    ///
    /// assert!(!layout.glyphs.is_empty());
    /// assert!(!new_ids.is_empty(), "first call should pack at least one new glyph");
    /// // Every newly packed glyph must have a UV entry in the atlas.
    /// for gid in &new_ids {
    ///     assert!(atlas.uv_map.contains_key(gid));
    /// }
    ///
    /// // A second call with the same text reuses the already-packed glyphs.
    /// let (_layout2, new_ids2) = pipeline
    ///     .render_to_sdf_atlas("Hello", &style, &mut atlas)
    ///     .expect("render_to_sdf_atlas failed");
    /// assert!(new_ids2.is_empty(), "repeated glyphs should not be re-packed");
    ///
    /// // GPU upload: use atlas.texture + atlas.uv_map
    /// ```
    ///
    /// # Errors
    /// Propagates shape/layout errors and SDF generation errors via [`OxiTextError::Other`].
    pub fn render_to_sdf_atlas(
        &mut self,
        text: &str,
        style: &TextStyle,
        atlas: &mut oxitext_sdf::SdfAtlas,
    ) -> Result<(oxitext_layout::LayoutResult, Vec<u16>), OxiTextError> {
        // 1. Shape and layout the text.
        let layout_result = self.shape_and_layout(text, style)?;

        // 2. Collect unique (glyph_id, font_size) pairs — deduplicate to avoid
        //    generating the same SDF tile twice.
        let mut seen = std::collections::HashSet::<(u16, u32)>::new();
        let mut glyph_set: Vec<(u16, f32)> = Vec::new();
        for g in &layout_result.glyphs {
            if seen.insert((g.gid, g.font_size.to_bits())) {
                glyph_set.push((g.gid, g.font_size));
            }
        }

        let font_bytes: &[u8] = &self.font_data;
        let mut packed_ids: Vec<u16> = Vec::new();

        for (glyph_id, px_size) in glyph_set {
            // Skip glyphs already in the atlas.
            if atlas.uv_map.contains_key(&glyph_id) {
                continue;
            }

            // Generate SDF tile analytically.  Returns Ok(None) for whitespace.
            let maybe_tile = oxitext_sdf::glyph_to_sdf_tile_analytic(
                font_bytes, glyph_id, px_size, 64,  // tile_size in pixels
                4.0, // spread in pixels
            )
            .map_err(|e| OxiTextError::Other(format!("sdf tile error: {e}")))?;

            if let Some(tile) = maybe_tile {
                // add_tile returns None when the atlas is full; skip gracefully.
                if atlas.add_tile(&tile).is_some() {
                    packed_ids.push(glyph_id);
                }
            }
        }

        Ok((layout_result, packed_ids))
    }
}

/// Convenient glob-import of the most common OxiText types.
///
/// ```rust
/// use oxitext::prelude::*;
/// let style = TextStyle::default().with_alignment(TextAlignment::Center);
/// assert_eq!(style.alignment, TextAlignment::Center);
/// ```
pub mod prelude {
    pub use oxitext_core::{
        Bitmap, ColorBitmap, Decoration, FlowDirection, GlyphMetrics, LayoutConstraints,
        OxiTextError, ParagraphStyle, PositionedGlyph, RenderOutput, Rgba8, TextAlignment,
        TextStyle, WritingMode,
    };
    pub use oxitext_layout::{LayoutResult, Line, ParagraphMetrics};

    #[cfg(feature = "pure")]
    pub use crate::{Pipeline, RenderResult};
}
