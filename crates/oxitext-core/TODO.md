# oxitext-core TODO

## Status
Core value types and traits for the OxiText pipeline. Provides `ShapedGlyph`, `ShapedRun`, `PositionedGlyph`, `Bitmap`, `LayoutConstraints`, `TextStyle`, `FlowDirection`, and `OxiTextError`. ~135 SLOC. Minimal but functional type surface. Needs richer glyph metadata, color support, and additional layout primitives.

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

## API Improvements
- [x] Implement `Display` for `OxiTextError` with more context (source text snippet, glyph index)
- [x] Add `Serialize`/`Deserialize` behind a `serde` feature gate for all public types
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

## Performance
- [x] Evaluate `SmallVec<[ShapedGlyph; 8]>` for `ShapedRun::glyphs` (most runs have <8 glyphs)
- [x] Use `Arc<[u8]>` instead of `Arc<Vec<u8>>` for font_data to eliminate one indirection level

## Integration
- [x] Ensure all types are `Send + Sync` for multi-threaded text rendering pipelines (verified by test)
- [x] Add `From<RasterOutput>` impl for `Bitmap` to bridge oxitext-raster backend output
- [x] Align `Bitmap` with oxitext-sdf's SDF tile format for unified atlas packing
