# oxitext-core — Core traits and value types for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-core.svg)](https://crates.io/crates/oxitext-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-core` defines the shared data types that flow through the entire OxiText pipeline. It contains no shaping, layout, or rasterization logic — those concrete stages live in the sibling crates: `oxitext-shape` (complex-script shaping, HarfBuzz-like), `oxitext-layout` (bidi, line breaking, paragraph layout), and `oxitext-raster` (glyph rasterization). By depending only on `oxitext-core`, each stage stays decoupled from the others and free of any font-parser dependency.

This crate is **100% Pure Rust**, `#![forbid(unsafe_code)]`, and `#![no_std]`-compatible (enable the default `std` feature for `std::error::Error` integration; otherwise it builds against `alloc`). Its only runtime dependency is `smallvec` for inline glyph storage.

## Installation

```toml
[dependencies]
oxitext-core = "0.2.0"
```

For `no_std` targets, disable default features:

```toml
[dependencies]
oxitext-core = { version = "0.2.0", default-features = false }
```

## Quick Start

```rust
use oxitext_core::{ShapedGlyph, ShapedRun, TextStyle, TextAlignment, LineSpacing};
use std::sync::Arc;

// A run of shaped glyphs sharing a single font face.
let run = ShapedRun {
    glyphs: [ShapedGlyph { gid: 36, x_advance: 9.0, ..Default::default() }]
        .into_iter()
        .collect(),
    font_data: Arc::from(&b""[..]),
};
assert_eq!(run.glyphs.len(), 1);

// Builder-style text styling.
let style = TextStyle::default()
    .with_font_size(24.0)
    .with_alignment(TextAlignment::Center)
    .with_max_width(400.0);
assert_eq!(style.font_size, 24.0);

// Resolve an effective line height from the natural font line height.
let line_height = LineSpacing { leading: 2.0, line_height_multiplier: 1.5 };
assert_eq!(line_height.resolve(20.0), 32.0); // 20 * 1.5 + 2
```

## API Overview

### Shaping types

| Type | Key fields / methods | Description |
|------|----------------------|-------------|
| `ShapedGlyph` | `gid`, `x_advance`, `y_advance`, `x_offset`, `y_offset`, `cluster`, `is_whitespace`, `unsafe_to_break` | One glyph from the shaper. `Default` yields a `.notdef` (GID 0). `unsafe_to_break` mirrors HarfBuzz's flag for multi-glyph clusters |
| `ShapedRun` | `glyphs: SmallVec<[ShapedGlyph; 8]>`, `font_data: Arc<[u8]>` | A run of glyphs sharing one font face; inline capacity of 8 avoids heap allocation for short runs |
| `GlyphCluster` | `glyphs`, `source_start`, `source_end`; `advance()`, `is_empty()` | Glyphs forming one user-perceived grapheme cluster (base + marks, emoji ZWJ); the atomic unit for cursor movement and line breaking |
| `GlyphMetrics` | `bearing_x`, `bearing_y`, `advance_x`, `advance_y`, `width`, `height` | Per-glyph pixel metrics usable for layout without rasterizing |
| `FontVerticalMetrics` | `units_per_em`, `ascender`, `descender`, `line_gap`; `ascent_px()`, `descent_px()`, `line_gap_px()` | Font-library-agnostic mirror of `hhea`/`OS/2` vertical metrics, in design units |

### Positioned (post-layout) types

| Type | Key fields | Description |
|------|-----------|-------------|
| `PositionedGlyph` | `gid`, `font_data`, `pos: (f32, f32)`, `font_size`, `advance_x`, `cluster` | A glyph placed on the canvas; carries per-glyph font size so a line may mix sizes |
| `InlineObject` | `id`, `width`, `height`, `baseline_offset`, `advance` | An inline image/widget treated as a glyph with known advance |
| `PositionedInlineObject` | `object`, `x`, `y`, `line` | An `InlineObject` placed during a layout pass |

### Bitmap / render-output types

| Type | Key fields / methods | Description |
|------|----------------------|-------------|
| `Bitmap` | `width`, `height`, `pixels: Vec<u8>`; `is_empty()`, `invert_coverage()`, `threshold()`, `crop()`, `tight_bounds()` | Greyscale coverage bitmap (1 byte/pixel) with SDF/atlas helper methods |
| `ColorBitmap` | `width`, `height`, `rgba: Vec<u8>`; `is_empty()` | RGBA color glyph bitmap (4 bytes/pixel) for color fonts |
| `LcdBitmap` | `width`, `height`, `rgb: Vec<u8>`; `new()`, `is_empty()` | LCD subpixel bitmap (3 bytes/pixel) for ClearType-style rendering |
| `RenderOutput` | enum: `Greyscale`, `Color`, `Sdf`, `Lcd`, `Msdf`; `into_bitmap()` | Unified per-glyph render output so a pipeline can return mixed glyph kinds through one channel |

`RenderOutput` also implements `From<RenderOutput> for Option<Bitmap>` (greyscale → `Some`, otherwise `None`).

### Style and layout-configuration types

| Type | Key fields / methods | Description |
|------|----------------------|-------------|
| `TextStyle` | `font_size`, `max_width`, `flow_direction`, `alignment`, `line_spacing`; `with_font_size()`, `with_max_width()`, `with_alignment()`, `with_flow_direction()` | Text rendering style with builder-style setters |
| `ParagraphStyle` | `alignment`, `indent`, `spacing_before`, `spacing_after`, `direction`, `line_spacing` | Paragraph-level layout style (CSS Text / Writing Modes) |
| `TextRun` | `text`, `font_data`, `style`, `decoration` | A styled span pairing text with font bytes for rich-text layout |
| `LayoutConstraints` | `max_width`, `font_size` | Minimal constraints for the layouter (`max_width == 0.0` disables wrapping) |
| `LineSpacing` | `leading`, `line_height_multiplier`; `resolve()` | Line spacing; effective height = `natural * multiplier + leading` |

### Direction, alignment, and writing-mode enums

| Type | Variants | Description |
|------|----------|-------------|
| `FlowDirection` | `Horizontal` (default), `Vertical` | Text flow; vertical enables top-to-bottom CJK (UAX #50) |
| `TextAlignment` | `Left` (default), `Right`, `Center`, `Justify` | Horizontal alignment per CSS Text Level 3 |
| `WritingMode` | `HorizontalTb` (default), `VerticalRl`, `VerticalLr`; `flow_direction()`, `is_vertical()` | CSS Writing Modes Level 4 `writing-mode` |
| `VerticalPosition` | `Normal` (default), `Superscript { .. }`, `Subscript { .. }`; `effective_size()`, `baseline_adjustment()` | Subscript/superscript positioning |

### Color and decoration types

| Type | Key fields / methods | Description |
|------|----------------------|-------------|
| `Rgba8` | `r`, `g`, `b`, `a`; consts `BLACK`, `TRANSPARENT`; `new()` | sRGB color with straight (non-premultiplied) alpha |
| `TextDecoration` | enum: `Underline { .. }`, `Overline { .. }`, `Strikethrough { .. }` | Decoration style applied to a run |
| `DecorationLine` | `position`, `thickness`, `color` | A single decoration line relative to the baseline |
| `Decoration` | `underline`, `overline`, `strikethrough`; `any()` | Per-run set of decoration lines |
| `DecorationRect` | `x`, `y`, `width`, `height`, `color` | A positioned decoration rectangle ready for compositing |

### `OxiTextError`

The pipeline-wide error type. Implements `Display` and `core::error::Error`.

| Variant | Description |
|---------|-------------|
| `Shaping(String)` | An error occurred during glyph shaping |
| `Layout(String)` | An error occurred during layout computation |
| `Raster(String)` | An error occurred during glyph rasterization |
| `FontNotFound` | No usable font was found |
| `InvalidFont` | Font data is corrupt or in an unsupported format |
| `Other(String)` | Any other error not covered above |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Links `std`; enables `std::error::Error` for `OxiTextError`. Disable for `no_std`/`alloc` builds |
| `serde` | no | Derives `Serialize`/`Deserialize` for the plain-data types (`ShapedGlyph`, `Bitmap`, `ColorBitmap`, `LcdBitmap`, `RenderOutput`, `TextStyle`, `ParagraphStyle`, styling enums, etc.) |

## Cross-references

- [`oxitext`](https://crates.io/crates/oxitext) — the high-level façade that ties shaping, layout, and rasterization together.
- [`oxitext-shape`](https://crates.io/crates/oxitext-shape) — produces `ShapedRun`/`ShapedGlyph` from text + font bytes.
- [`oxitext-layout`](https://crates.io/crates/oxitext-layout) — consumes `ShapedRun` and produces `PositionedGlyph`.
- [`oxitext-raster`](https://crates.io/crates/oxitext-raster) — rasterizes glyphs into `Bitmap`/`RenderOutput`.
- `oxitext-sdf`, `oxitext-icu`, `oxitext-bench` — signed-distance-field generation, ICU4X integration, and benchmarks.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
