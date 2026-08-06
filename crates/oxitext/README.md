# oxitext — The COOLJAPAN Pure-Rust text rendering facade

[![Crates.io](https://img.shields.io/crates/v/oxitext.svg)](https://crates.io/crates/oxitext)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext` is the top-level facade crate of OxiText, the COOLJAPAN Pure-Rust text pipeline. It wires the sibling crates — shaping, layout, rasterization, and (optionally) SDF and ICU — into a single ergonomic [`Pipeline`] that turns a string and a [`TextStyle`] into positioned glyphs, per-glyph bitmaps, and line/paragraph metrics. The complete data flow is **shape → bidi → line-break → layout → rasterize**.

The crate is a thin orchestration layer: nearly everything it exposes is **re-exported** from the underlying crates so callers depend on one name. The default feature set is **100% Pure Rust** — no C/C++ libraries. Shaping uses `swash`, rasterization uses `fontdue`, bidi/line-breaking use `unicode-bidi`/`unicode-linebreak`, and the optional `icu` layer uses Pure-Rust ICU4X. The `pure` feature (on by default) provides the full pipeline; bidi, line-break, and vertical-writing helpers are always available regardless of features.

## Installation

```toml
[dependencies]
# Default: full Pure-Rust pipeline
oxitext = "0.2.3"

# Explicit minimal pipeline
oxitext = { version = "0.2.3", features = ["pure"] }

# Pipeline + SDF atlas generation for GPU rendering
oxitext = { version = "0.2.3", features = ["pure", "sdf"] }

# Everything: pipeline + ICU + SDF + PNG output
oxitext = { version = "0.2.3", features = ["pure", "sdf", "icu", "png-output"] }
```

## Quick Start

```rust,no_run
use oxitext::{Pipeline, TextStyle};

let font_bytes = std::fs::read("path/to/font.ttf")?;
let mut pipeline = Pipeline::from_bytes(&font_bytes)?;
let style = TextStyle::default();
let result = pipeline.render("Hello, world", &style)?;
println!("{} glyphs, {} bitmaps", result.glyphs.len(), result.bitmaps.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Render straight to an RGBA image (or a PNG file with the `png-output` feature):

```rust,no_run
use oxitext::{Pipeline, TextStyle};
use oxitext::prelude::Rgba8;

let font_bytes = std::fs::read("font.ttf")?;
let mut pipeline = Pipeline::from_bytes(&font_bytes)?;
let style = TextStyle::default();

let bg = Rgba8 { r: 255, g: 255, b: 255, a: 255 };
let fg = Rgba8 { r: 0, g: 0, b: 0, a: 255 };
let image = pipeline.render_to_image("Hello", &style, bg, fg)?;
println!("{}×{} RGBA canvas", image.width, image.height);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Pipeline

[`Pipeline`] combines [`SwashShaper`] + the word-aware [`LayoutEngine`] + [`FontdueRasterizer`]. It wraps text at UAX #14 (or CLDR, with `icu`) opportunities, honours mandatory breaks, applies [`TextAlignment`], and uses the font's real ascender/descender/line-gap for accurate line spacing.

| Method | Description |
|--------|-------------|
| `Pipeline::from_bytes(font_bytes)` | Build a pipeline from a TTF/OTF byte slice. |
| `Pipeline::new(font_db)` | Build from an `oxifont::FontDatabase`. |
| `Pipeline::new_with_system_font(family)` | Build from a named installed font family. |
| `Pipeline::builder()` | Fluent [`PipelineBuilder`] (`.font(bytes).build()`). |
| `Pipeline::with_backend(...)` | Build with a custom `ShapeBackend` (LTR-only path). |
| `render(text, style)` | Shape → layout → raster → [`RenderResult`]. |
| `render_paragraph(paragraphs, style)` | Render multiple paragraphs with inter-paragraph spacing. |
| `render_styled(runs, max_width)` | Render mixed-font/size [`TextRun`]s (LTR-only first pass). |
| `render_to_image(text, style, bg, fg)` | Render and composite to a `ColorBitmap`. |
| `render_to_sdf_atlas(text, style, &mut atlas)` *(feature `sdf`)* | Populate an `oxitext_sdf::SdfAtlas`; returns `(LayoutResult, Vec<u16>)` of newly packed glyph IDs. |
| `shape_and_layout(text, style)` | Shape + layout without rasterizing → `LayoutResult`. |
| `measure(text, style)` | `ParagraphMetrics` only (no rasterization). |
| `shape_with_fallback(...)` | Shape with the fallback-font chain. |
| `set_fallback_fonts(fonts)` | Set the `.notdef` fallback chain. |
| `font_metrics()` | The font's `FontVerticalMetrics` (if available). |
| `has_rtl(text)` | Whether the text contains RTL runs. |
| `renders_color_glyphs(&self)` | Whether the primary font carries COLR color glyphs (always `true` for a `pure`-feature build). Both COLRv0 and COLRv1 — including gradients, transforms, and composite paints — are rendered correctly through the full `Pipeline`, not just flat-color fallback. |
| `available_features()` | Static slice of compiled-in feature names. |
| `benchmark(text, style, iterations)` / `profile(text, style)` | Rough in-process timing helpers (use Criterion for precision). |

[`RenderResult`] carries `glyphs`, `bitmaps`, `outputs` (greyscale/color/LCD/SDF), `lines`, `metrics`, and `decoration_rects`. It also offers `composite_to_rgba(width, height, bg, fg)` and, with `png-output`, `to_png(path, width, height, bg, fg)`.

Top-level helper: `best_font_for_char(ch, primary, fallbacks)` returns the index of the first font in the chain that has a glyph for `ch`.

## Feature Flags

| Feature | Default | Enables |
|---------|---------|---------|
| `pure` | yes | [`Pipeline`], [`SwashShaper`], [`LayoutEngine`], [`SimpleLayouter`], [`FontdueRasterizer`], and the backend traits — the full pipeline. Pulls in `swash`, `fontdue`, `unicode-bidi`, `unicode-linebreak`. |
| `sdf` | no | The `sdf` module (re-export of [`oxitext-sdf`](../oxitext-sdf)) and `Pipeline::render_to_sdf_atlas`. ~+150 KB. |
| `icu` | no | The `icu` module (curated re-export of [`oxitext-icu`](../oxitext-icu)) plus CLDR line-breaking/segmentation in layout. Adds ~5–15 MB of compiled CLDR data. |
| `simd` | no | SIMD-accelerated rasterization paths in the fontdue backend. No API change. |
| `parallel` | no | Rayon-based parallel rasterization (implies `pure`). No API change. |
| `png-output` | no | `RenderResult::to_png` for writing rendered text to PNG, via `oxitext-core`'s in-tree Pure-Rust PNG writer (no `png`/`flate2` dependency). |
| `font-subset` | no | The [`pdf_subset`] module ([`pdf_subset::TextFontSubsetter`]) for on-the-fly font subsetting in PDF text-rendering pipelines. Pulls in `oxifont-subset`. |
| `color-bitmap-fonts` | no | PNG-compressed CBDT/sbix color-bitmap strike decoding (implies `pure`). Off by default — pulls in `png`/`flate2`, which this workspace's `deny.toml` bans. COLRv0/v1 vector color emoji need no such dependency and already render correctly in the default build. |

> **Blueprint deviation.** The original blueprint listed `default = ["pure", "emoji"]`, but the `emoji` feature depends on the planned `oxitext-emoji` crate (M3). The current default is `["pure"]` only; emoji support lands when `oxitext-emoji` does.

## Re-exported types at the crate root

From [`oxitext-core`](../oxitext-core) (always available):

- Style/layout inputs: `TextStyle`, `ParagraphStyle`, `TextAlignment`, `TextDecoration`, `Decoration`, `DecorationLine`, `DecorationRect`, `LayoutConstraints`, `LineSpacing`, `FlowDirection`, `WritingMode`, `TextRun`
- Glyph/metric types: `GlyphCluster`, `GlyphMetrics`, `PositionedGlyph`, `ShapedGlyph`, `ShapedRun`, `FontVerticalMetrics`
- Output types: `Bitmap`, `ColorBitmap`, `Rgba8`, `RenderOutput`
- Error: `OxiTextError`

From [`oxitext-layout`](../oxitext-layout) (always available):

- `LayoutEngine`, `LayoutResult`, `Line`, `LineMetrics`, `ParagraphMetrics`
- Bidi: `BidiParagraph`, `BidiRun` (`bidi` submodule)
- Line-break: `LineBreak`, `LineBreaker` (`linebreak` submodule)
- Vertical: `VerticalMetrics`, `is_upright_in_vertical` (`vertical` submodule)
- Tate-chu-yoko: `detect_runs`, `GlyphEntry`, `TateChuYokoRun`

Behind `pure`: `SwashShaper`, `SimpleLayouter`, `FontdueRasterizer`, and the backend traits `ShapeBackend`, `RasterBackend`, `ShapeDirection`, `ShapeFeature`, `ShapeRequest`.

Behind `sdf`: the `oxitext::sdf` module (full re-export of [`oxitext-sdf`](../oxitext-sdf)).

Behind `icu`: the `oxitext::icu` module re-exporting `CaseMapper`, `CharProperties`, `CollateError`, `IcuCollator`, `IcuSegmenter`, `NormalizationForm`, `Normalizer`, `ScriptRun`, `SegmentKind`, `TextScript`.

Behind `font-subset`: the `oxitext::pdf_subset` module, centered on `pdf_subset::TextFontSubsetter` — accumulate codepoints/GIDs across pages via `feed_text`/`feed_char`/`feed_gid`, then call `finalize()` to produce a minimal subset font for embedding.

## Prelude

```rust
use oxitext::prelude::*;
let style = TextStyle::default().with_alignment(TextAlignment::Center);
assert_eq!(style.alignment, TextAlignment::Center);
```

Imports the most common types: `Bitmap`, `ColorBitmap`, `Decoration`, `FlowDirection`, `GlyphMetrics`, `LayoutConstraints`, `OxiTextError`, `ParagraphStyle`, `PositionedGlyph`, `RenderOutput`, `Rgba8`, `TextAlignment`, `TextStyle`, `WritingMode`, `LayoutResult`, `Line`, `ParagraphMetrics`, and (with `pure`) `Pipeline`, `RenderResult`.

## Error Variants — `OxiTextError`

| Variant | Description |
|---------|-------------|
| `Shaping(String)` | Error during glyph shaping. |
| `Layout(String)` | Error during layout computation. |
| `Raster(String)` | Error during glyph rasterization. |
| `FontNotFound` | No usable font was found. |
| `InvalidFont` | Font data is corrupt or an unsupported format. |
| `Other(String)` | Any other error (e.g. PNG/SDF failures). |

## OxiText Crate Map

| Crate | Role |
|-------|------|
| [`oxitext`](.) | Facade — this crate; assembles the pipeline and re-exports the public surface. |
| [`oxitext-core`](../oxitext-core) | Shared traits and types (styles, glyphs, bitmaps, `OxiTextError`). |
| [`oxitext-shape`](../oxitext-shape) | Glyph shaping (`SwashShaper`, `ShapeBackend`). |
| [`oxitext-layout`](../oxitext-layout) | Word-aware layout, bidi, line-breaking, vertical writing. |
| [`oxitext-raster`](../oxitext-raster) | Glyph rasterization (`FontdueRasterizer`, COLR color glyphs). |
| [`oxitext-sdf`](../oxitext-sdf) | Signed-distance-field atlas generation (GPU text). |
| [`oxitext-icu`](../oxitext-icu) | ICU4X-backed CLDR segmentation, collation, normalization, properties. |
| [`oxitext-bench`](../oxitext-bench) | Criterion benchmarks (dev-only). |

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
