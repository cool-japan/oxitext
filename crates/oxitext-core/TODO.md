# oxitext-core TODO

## Status
Core value types for the OxiText pipeline: `ShapedGlyph`/`ShapedRun`/`GlyphCluster`, `PositionedGlyph`, `Bitmap`/`ColorBitmap`/`LcdBitmap`/`RenderOutput`, `TextStyle`/`ParagraphStyle`/`TextRun`, `LayoutConstraints`, `FlowDirection`/`TextAlignment`/`WritingMode`, decoration types, and `OxiTextError`. New in 0.2.1 (2026-07-30): the `png_encode` module (feature `png-encode`, off by default) — a self-contained 8-bit PNG writer on the `oxiarc-deflate`/`oxiarc-core` stack, so downstream crates don't need `flate2`/`miniz_oxide`. ~1,480 SLOC (tokei) across `lib.rs` + `png_encode.rs`. Type surface and PNG encoder are both complete — all checklist items below are done; 55 tests pass (`cargo nextest run -p oxitext-core --all-features`). Contributions welcome via PR.

## Core Implementation
- [x] Add `GlyphMetrics` struct (bearing_x, bearing_y, advance_x, advance_y, width, height) for layout use without rasterizing
- [x] Add `ColorBitmap` struct (width, height, rgba: Vec<u8>) for color glyph output alongside greyscale `Bitmap`
- [x] Add `TextAlignment` enum (Left, Right, Center, Justify) to `TextStyle`
- [x] Add `LineSpacing` struct (leading, line_height_multiplier) to `TextStyle`
- [x] Add `ParagraphStyle` struct (alignment, indent, spacing_before, spacing_after, direction, line_spacing)
- [x] Add `TextRun` struct pairing text + font data + style + decoration, representing a styled span within a paragraph
- [x] Add `WritingMode` enum (HorizontalTb, VerticalRl, VerticalLr) per CSS Writing Modes Level 4 (+ `flow_direction()`/`is_vertical()`)
- [x] Add `Decoration` struct (underline, overline, strikethrough with position/thickness/color via `DecorationLine` + `Rgba8`)
- [x] Add `ShapedGlyph::is_whitespace` flag for layout engines to distinguish visible glyphs
- [x] Add `ShapedGlyph::unsafe_to_break` flag for line-break decisions within glyph clusters
- [x] Add `PositionedGlyph::font_size` field for multi-size text rendering
- [x] Add `RenderOutput` enum (Greyscale(Bitmap), Color(ColorBitmap), Sdf{...}) for unified output
- [x] Add `no_std` + `alloc` support behind a feature gate (~30 SLOC)
- [x] Add `GlyphCluster` struct grouping multiple ShapedGlyphs that form a single grapheme cluster (+ `advance()`/`is_empty()`)
- [x] Add `FontVerticalMetrics` (font-library-agnostic ascender/descender/line-gap) to drive layout line height
- [x] Add `png_encode` module (feature `png-encode`, off by default; requires `std`) — self-contained 8-bit PNG writer (`encode_png`, `PngColorType`, `PngEncodeError`) on `oxiarc-deflate`/`oxiarc-core`, so downstream crates avoid the banned `png`/`flate2`/`miniz_oxide` stack

## API Improvements
- [x] Implement `Display` for `OxiTextError` with more context (source text snippet, glyph index)
- [x] Add `Serialize`/`Deserialize` behind a `serde` feature gate for the plain-data value types (`ShapedGlyph`, `Bitmap`, `ColorBitmap`, `LcdBitmap`, `RenderOutput`, `TextStyle`, `ParagraphStyle`, styling enums, etc.); types holding `Arc<[u8]>`/`SmallVec` (`ShapedRun`, `PositionedGlyph`, `TextRun`) and `OxiTextError` are intentionally left out
- [x] Implement `Default` for `ShapedGlyph` (zero-advance, GID 0)
- [x] Add `Hash` derive to `FlowDirection`, `TextAlignment` for use as HashMap keys
- [x] Change `ShapedRun::font_data` from `Arc<Vec<u8>>` to `Arc<[u8]>` to avoid double indirection (cascades through shape/raster/cache — deferred)

## Testing
- [x] Test `LayoutConstraints::default()` values match documented defaults
- [x] Test `TextStyle::default()` values
- [x] Test `OxiTextError::Display` formatting for all variants
- [x] Add property-based tests for `FlowDirection` equality/clone
- [x] Test `ShapedGlyph` with negative offsets (combining marks)
- [x] Test all new value types (GlyphMetrics, GlyphCluster, RenderOutput, LineSpacing, Decoration, WritingMode, TextRun) + Send+Sync
- [x] Add round-trip and error-path unit tests for `png_encode` (11 tests: grayscale/grayscale-alpha/RGB/RGBA round-trips incl. a wide row and a flat-image compression check, zero-dimension and buffer-size-mismatch rejection, channel/color-code table, Paeth predictor reference values)

## Performance
- [x] Evaluate `SmallVec<[ShapedGlyph; 8]>` for `ShapedRun::glyphs` (most runs have <8 glyphs)
- [x] Use `Arc<[u8]>` instead of `Arc<Vec<u8>>` for font_data to eliminate one indirection level

## Integration
- [x] Ensure all types are `Send + Sync` for multi-threaded text rendering pipelines (verified by test)
- [x] Bridge raster-backend output into this crate's types via `RenderOutput::into_bitmap()` and `From<RenderOutput> for Option<Bitmap>`; the reverse `From<RasterOutput> for RenderOutput` conversion lives in `oxitext-raster` (which depends on `oxitext-core`), not here, to avoid a circular dependency
- [x] Align `Bitmap` with oxitext-sdf's SDF tile format for unified atlas packing
