# Changelog

All notable changes to OxiText are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-01

### Added

#### oxitext-core
- Rich value types: `GlyphMetrics`, `GlyphCluster`, `ColorBitmap`, `RenderOutput`, `TextAlignment`, `WritingMode`, `LineSpacing`, `Decoration`/`DecorationLine`, `Rgba8`, `ParagraphStyle`, `TextRun`, `FontVerticalMetrics`
- `ShapedGlyph::{is_whitespace, unsafe_to_break}` flags and `Default` derive
- `PositionedGlyph::font_size` field
- `Hash` on `FlowDirection` and `TextAlignment`
- `TextStyle` builder methods
- `FlowDirection` enum (Horizontal/Vertical), `TextScript`, `ScriptRun`

#### oxitext-shape
- `ShapeBackend` trait with pluggable backends: `SwashShaperBackend` (primary) and `RustybuzzShaper` (opt-in `backend-rustybuzz` feature)
- LRU shape cache (`ShapeCache` / `ShapeKey`) behind `SwashShaper::with_cache`
- SIMD-accelerated shape hot-loop via `wide` f32x8
- Script detection: `requires_arabic_shaping`, `requires_indic_shaping`, `requires_mark_positioning`
- Arabic joining-form support; Devanagari conjunct shaping; Thai mark positioning
- Font fallback chain support with automatic script-based font selection
- `shape_by_script`, `shape_with_fallback`, `shape_with_features`, `shape_with_variations`
- Variational font axis support
- Vertical text shaping with `vert`/`vrt2` feature injection
- `ShapeBatch` for parallel shaping of multiple text segments

#### oxitext-layout
- UAX #9 bidi reordering via `unicode-bidi` (`BidiParagraph`, `BidiRun`, visual-order runs)
- UAX #14 line-breaking via `unicode-linebreak` (`LineBreaker`, Mandatory/Allowed breaks)
- Word-aware greedy line-breaking engine (`LayoutEngine`) driven by UAX #14 opportunities
- Left/Right/Center/Justify text alignment
- `LineMetrics`, `ParagraphMetrics`, `Line`, `LayoutResult` types
- Font-metric-driven line height with mandatory-break awareness and overflow detection
- UAX #50 vertical text orientation (CJK/Hangul/Kana upright, Latin rotated)
- Tate-chu-yoko detection (`MAX_TCY_RUN_LEN=4`, `GlyphEntry`, `TateChuYokoRun`)
- Hyphenation integration with soft-hyphen (U+00AD) detection; automatic hyphenation via `hypher` behind `hyphenation` feature
- ICU4X CLDR segmentation as drop-in replacement for `unicode-linebreak` when `icu` feature enabled

#### oxitext-raster
- `RasterBackend` trait with `FontdueRaster` (primary) and `AbGlyphRaster` (opt-in `backend-ab-glyph` feature)
- COLRv0/CPAL color glyph compositing with Porter-Duff source-over blending
- COLRv1 gradients: linear, radial, and sweep with Pad/Repeat/Reflect extend modes
- CBDT/CBLC and sbix PNG-encoded color bitmap extraction
- SVG glyph rendering via `resvg` + `tiny-skia` behind `svg-backend` feature
- SubpixelOffset (quarter-pixel) positioning and `SubpixelCacheKey`
- LCD subpixel rendering with configurable 3-tap and 5-tap FIR filter kernels and sRGB gamma
- `SwashRaster` backend with TrueType hinting for grid-fitted outlines (behind `swash-backend` feature)
- SIMD-accelerated raster hot-loop (`wide` f32x8) behind `simd` feature
- Thread-local font cache for zero-lock rasterization

#### oxitext-sdf
- Felzenszwalb-Huttenlocher 2D Euclidean Distance Transform (`compute_sdf`, `glyph_to_sdf_tile`)
- `SdfAtlas` shelf-packer with `UvRect` UV coordinates
- SIMD-accelerated 1D EDT pass
- Multi-channel SDF (MSDF): Chlumsky edge coloring + 3-channel distance fields
- Multi-channel + true SDF (MTSDF): 4-channel variant for GPU rendering
- Pseudodistance SDF (PSDF) variant
- Analytic SDF from outline segments (Bézier-accurate)
- Atlas serialization/deserialization (binary round-trip)
- PNG atlas export
- GPU descriptor types (`GpuGlyphDescriptor`, `NormalizedUvRect`) for direct shader upload
- Build helper (`generate_atlas_binary`) for compile-time atlas baking
- Growing atlas packer and MaxRects-based non-overlapping placement

#### oxitext-icu
- Unicode normalization (`Normalizer`): NFC, NFD, NFKC, NFKD
- Script detection and itemization (`CharProperties`, `TextScript`, `ScriptRun`)
- Character property queries (alphabetic, numeric, whitespace, general-category)
- `IcuSegmenter` with line/word/grapheme/sentence segmentation via ICU4X CLDR compiled data
- `IcuCollator` with Unicode Collation Algorithm, locale-aware string comparison

#### oxitext (facade)
- `Pipeline` orchestrating shape → layout → rasterize with real font metrics
- `Pipeline::measure`, `shape_and_layout`, `render_to_image`, `composite_to_rgba`, `has_rtl`, `font_metrics`
- `RenderResult` extended with `lines` and `metrics`
- `prelude` module for ergonomic glob imports
- Feature matrix documented in module docs; `docs.rs` metadata with `all-features = true`
- oxifont integration for system font loading and automatic fallback chain selection

#### oxitext-bench
- Criterion benchmarks for shape / raster / pipeline on mixed-script text
- `harfbuzz-sys` dev-dependency for baseline comparison
- Purity tripwire to detect FFI escape at benchmark time

### Workspace
- Workspace version `0.1.0`, edition 2021, MSRV 1.89
- Pure Rust default features — no C/C++/Fortran in default dependency tree
- `deny.toml` with cargo-deny license and security policy
- FFI audit Dockerfile for CI-level purity verification
- End-to-end conformance tests in `tests/`

[0.1.0]: https://github.com/cool-japan/oxitext/releases/tag/v0.1.0
