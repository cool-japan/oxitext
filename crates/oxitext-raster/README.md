# oxitext-raster — Fontdue-based glyph rasterizer for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-raster.svg)](https://crates.io/crates/oxitext-raster)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-raster` is the **rasterization** stage of the OxiText pipeline: it turns glyph IDs and font bytes into `oxitext_core::Bitmap` coverage maps (and color, LCD, and SDF-ready output). The default backend wraps [fontdue](https://crates.io/crates/fontdue); optional backends add `ab_glyph`, swash (with TrueType hinting), SVG-in-OpenType rendering, and an `oxifont` backend. It also includes full COLRv0/COLRv1/CPAL color glyph compositing — linear/radial/sweep gradients, the `PaintTransform`/`PaintScale*`/`PaintRotate*`/`PaintSkew*` transform stack, `PaintGlyph`/`ClipList` clipping, and all 28 Porter-Duff/CSS `PaintComposite` modes — plus CBDT/CBLC/sbix bitmap-glyph extraction, glyph-outline extraction, full LCD subpixel rendering, sRGB gamma LUTs, FreeType-style stem darkening, quarter-pixel positioning, and SIMD-accelerated coverage/compositing primitives.

This crate is **100% Pure Rust** and `#![forbid(unsafe_code)]`. The default fontdue path carries no C/C++ dependencies; `ttf-parser` is likewise Pure Rust. Optional features pull in `wide` (portable SIMD), `swash`, `resvg`/`tiny-skia`, the `oxifont` parser, and — only behind the opt-in `png-bitmap` feature — `png`, for decoding PNG-compressed CBDT/sbix bitmap strikes. It consumes `PositionedGlyph` / `Bitmap` / `RenderOutput` from [`oxitext-core`](https://crates.io/crates/oxitext-core).

## Installation

```toml
[dependencies]
oxitext-raster = "0.2.3"
```

With optional capabilities:

```toml
[dependencies]
# Portable SIMD primitives, swash hinting backend, and SVG-glyph rendering
oxitext-raster = { version = "0.2.3", features = ["simd", "swash-backend", "svg-backend"] }
```

## Quick Start

### Rasterize a single glyph to a greyscale bitmap

```rust,no_run
use oxitext_raster::FontdueRasterizer;
use std::sync::Arc;

let font_data: Arc<[u8]> = Arc::from(std::fs::read("font.ttf")?.as_slice());

let rasterizer = FontdueRasterizer::new();
let bitmap = rasterizer.raster(36, &font_data, 32.0)?; // glyph id 36 at 32px
println!("{}x{} coverage bitmap", bitmap.width, bitmap.height);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Rasterize pre-laid-out glyphs

```rust,no_run
use oxitext_raster::{rasterize_positioned, RasterOptions};
use oxitext_core::PositionedGlyph;

let glyphs: Vec<PositionedGlyph> = Vec::new(); // produced by oxitext-layout
let options = RasterOptions::default();

let bitmaps = rasterize_positioned(&glyphs, &options);
assert_eq!(bitmaps.len(), glyphs.len()); // one Option<Bitmap> per glyph
```

### Render a COLR/CPAL color (emoji) glyph

`render_colr_glyph_sized` sizes the output from the glyph's own paint box —
rather than a fixed preview square — so gradients, transforms and composite
layers that paint outside a naive 1em box are not clipped. It covers both
COLRv0 and COLRv1 (ttf-parser dispatches on the table version internally).

```rust,no_run
use oxitext_raster::render_colr_glyph_sized;

let font_data = std::fs::read("emoji-font.ttf")?;
let glyph_id: u16 = 42; // resolved via cmap/shaping for the emoji codepoint
let palette = 0; // default CPAL palette

if let Some(img) = render_colr_glyph_sized(&font_data, glyph_id, 64.0, palette) {
    // `img.rgba` is straight (non-premultiplied) RGBA, trimmed to ink.
    println!(
        "{}x{} color glyph, bearing ({}, {})",
        img.width, img.height, img.bearing_x, img.bearing_y
    );
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Callers that hold their font bytes in an `Arc<[u8]>` (e.g. a caption renderer
drawing the same emoji every frame) should prefer
`render_colr_glyph_sized_cached`, which memoizes the paint graph per thread
and returns a shared `Arc<ColorGlyphImage>` on a cache hit.

## API Overview

### Top-level rasterizer and functions

| Item | Description |
|------|-------------|
| `FontdueRasterizer` | Thread-safe rasterizer with a bounded (64-entry) LRU font cache keyed by `Arc` pointer identity; `new()`, `raster(glyph_id, font_data, size)` |
| `rasterize_positioned(glyphs, options)` | Rasterize a slice of `PositionedGlyph`s; returns one `Option<Bitmap>` per glyph |
| `rasterize_for_sdf(font_data, glyph_id, px_size)` | Rasterize to a coverage bitmap suitable for EDT-based SDF generation |
| `accumulate_coverage(dst, src)` | Add coverage values (clamped to `[0,1]`); SIMD path under feature `simd` |
| `multiply_alpha_u8(buf, factor)` | Multiply a u8 alpha buffer in place; SIMD path under feature `simd` |
| `coverage_f32_to_u8(dst, src)` | Convert an f32 coverage buffer to u8 (clamped, rounded); SIMD path under feature `simd` |
| `porter_duff_source_over(dst, src)` | Porter-Duff "source over" for premultiplied RGBA; SIMD path under feature `simd` |
| `porter_duff_source_over_scalar(dst, src)` | The scalar reference implementation (always available) |

### Backends — `backend` module

| Item | Description |
|------|-------------|
| `RasterBackend` (trait) | Swappable rasterizer (`Send + Sync`). Methods: `rasterize`, `rasterize_lcd`, `rasterize_full`, `rasterize_color`, `rasterize_for_sdf`, `clear_cache` |
| `RasterOutput` | Backend output: `width`, `height`, `coverage`, `advance_x`, `advance_y`, `bearing_x`, `bearing_y` |
| `FontdueRaster` | Default fontdue backend with a bounded LRU font cache; `new()`, `raster_positioned(...)` for fractional pen positions |
| `AbGlyphRaster` | `ab_glyph` backend (feature `ab-glyph-backend`) with horizontal LCD support |

### Unified result — `result` module

| Item | Description |
|------|-------------|
| `RasterResult` | A `RenderOutput` plus metrics: `output`, `advance_x`, `advance_y`, `bearing_x`, `bearing_y`; `empty()`, `is_empty()` |

### Rasterization options — `options` module

| Item | Description |
|------|-------------|
| `RasterOptions` | Quality options: `hinting_mode`, `subpixel_mode`, `lcd_filter`, `gamma_correction`, `stem_darkening`; `builder()`, `lcd()` |
| `HintingMode` | `None` (default), `Normal`, `Full` |
| `SubpixelMode` | `None` (default), `Horizontal` (LCD) |
| `LcdFilterKernel` | `Box`, `Triangle`, `FreeType5Tap` (default) |

### Glyph caching — `cache` module

| Item | Description |
|------|-------------|
| `BitmapCache` | LRU bitmap glyph cache; `new(capacity)`, `get()`, `insert()`, `len()`, `is_empty()`, `clear()` |
| `BitmapCacheKey` | `glyph_id`, `px_size_times_64`, `render_mode` |
| `RenderMode` | Render-mode discriminator used in the cache key |

### Subpixel positioning — `subpixel` module

| Item | Description |
|------|-------------|
| `SubpixelOffset` | Quantised quarter-pixel offset (`N = 4`); `from_float()`, `as_float()`, `bucket()`, `bucket_with_count()` |
| `SubpixelBuckets` | Bucket granularity: `Four`, `Eight`, `Sixteen`; `count()` |
| `SubpixelOffsetXY` | 2-D subpixel pen position; `new(x, y)` |
| `SubpixelCacheKey` | Glyph + size + subpixel cache key; `new(glyph_id, px_size, x_offset)` |

### Color glyphs — `color`, `colr_cache`, and `detect` modules

COLR layer glyphs are rasterized by an internal `path_raster` scanline rasterizer working directly off `ttf-parser` outlines, not fontdue — fontdue only materializes glyphs reachable from the font's `cmap`, and COLR layer glyphs are deliberately not mapped from any codepoint, so routing them through fontdue previously produced fully transparent output for every color emoji. Paint-graph interpretation (transform stack, clip stack, layer stack, gradients, composite modes) lives in an internal `colr_paint` module.

| Item | Description |
|------|-------------|
| `color::render_colr_v0(...)`, `render_colr_v1(...)` | Render a COLR (v0 or v1) base glyph into a fixed-size RGBA bitmap. ttf-parser dispatches on the table version internally, so both entry points drive the full paint graph — gradients, transforms, clipping, composite modes — for a COLRv1 font |
| `color::render_colr_with_palette(...)` | Like `render_colr_v1`, against an explicit (non-default) CPAL palette index |
| `color::render_colr_glyph_sized(...)` → `ColorGlyphImage` | Sizes the bitmap from the glyph's own paint box (`ClipList` entry, else outline bbox, else a margin around the em) instead of a fixed preview square, trims it to ink, and reports the bearings needed to place it against a baseline |
| `color::render_colr_cached(...)`, `render_colr_glyph_sized_cached(...)` | Thread-local LRU-memoized counterparts of `render_colr_with_palette` / `render_colr_glyph_sized`, keyed on `Arc<[u8]>` font identity; a hit returns a shared `Arc` and copies no pixels |
| `color::render_color_glyph(...)` | Dispatch to the best available representation: SVG (feature `svg-backend`) → CBDT/CBLC/sbix → COLRv1/v0 |
| `color::ColorGlyphBitmap` | Fixed-size RGBA color-glyph output (`width`, `height`, `rgba`) |
| `colr_cache::clear_colr_cache()`, `colr_cache_stats()` → `ColrCacheStats` | Drop, or inspect (hits/misses/entries/bytes), the thread-local COLR paint-graph memo |
| `detect::detect_color_glyph_type(face_data, glyph_id)` → `ColorGlyphType` | Detect the color-glyph technology present (`Sbix` > `Svg` > `EmbeddedBitmap` > `ColrV1` > `ColrV0` > `None`) |
| `detect::extract_cbdt_bitmap(...)`, `render_cbdt_glyph(...)` | CBDT/CBLC bitmap extraction/rendering; PNG-encoded strikes (format 17/18/19) need feature `png-bitmap`, the 8 raw bitmap formats (`BitmapMono[Packed]`, `BitmapGray2/4/8[Packed]`, `BitmapPremulBgra32`) decode unconditionally |
| `detect::extract_raster_glyph(...)` → `RawRasterGlyph` | Extract an embedded raster glyph (CBDT or sbix) undecoded, as raw bytes tagged with `RasterImageFormat` |

### Glyph outlines — `outline` module

| Item | Description |
|------|-------------|
| `extract_glyph_outline(face_data, glyph_id, scale)` → `GlyphOutline` | Extract the vector outline as `PathCommand`s |
| `GlyphOutline`, `PathCommand` | Outline container and its move/line/quad/cubic/close commands |

### LCD subpixel rendering — `lcd` module

| Item | Description |
|------|-------------|
| `lcd::rasterize_lcd(...)` | Full LCD subpixel rendering pipeline (3× horizontal resolution + filter) |
| `lcd::kernel_weight_sum(kind)` | Sum of a `LcdFilterKernel`'s tap weights |

### Gamma and stem darkening — `gamma`, `stem_darken` modules

| Item | Description |
|------|-------------|
| `srgb_to_linear(v)`, `linear_to_srgb(v)` | IEC 61966-2-1 sRGB ↔ linear conversion LUTs |
| `stem_darken::stem_darkening_amount(ppem)` | FreeType-style darkening amount at a given ppem |
| `stem_darken::apply_stem_darkening(coverage, amount)` | Apply stem darkening to a coverage buffer |

### Hot-loop primitives — `scalar`, `simd` modules

`scalar` holds the reference implementations of `accumulate_coverage`, `multiply_alpha_u8`, `coverage_f32_to_u8`, and `porter_duff_source_over_scalar`. `simd` (feature `simd`) provides `wide::f32x8`-accelerated equivalents. The top-level dispatch functions select the SIMD path automatically when the feature is active.

### Thread-local cache — `tl_cache` module

| Item | Description |
|------|-------------|
| `get_or_parse_fontdue(face_data)` | Parse-or-fetch a `fontdue::Font` from a bounded thread-local LRU (lock-free fast path for `FontdueRaster`) |

### Optional backends

| Item | Feature | Description |
|------|---------|-------------|
| `SwashRaster` | `swash-backend` | swash backend with TrueType hinting; `new()`, `with_hint(hint)` |
| `render_svg_glyph(...)`, `render_svg_bytes(svg_data, px_size)` | `svg-backend` | Render SVG-in-OpenType glyphs via `resvg`/`tiny-skia` |
| `OxifontRaster` | `oxifont-backend` | Rasterizer backed by the `oxifont` parser; `new()` |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `simd` | no | Portable SIMD (`wide::f32x8`) coverage and compositing primitives |
| `ab-glyph-backend` | no | Adds the `AbGlyphRaster` backend (with horizontal LCD support) |
| `swash-backend` | no | Adds the `SwashRaster` hinting backend (pulls in `swash`) |
| `svg-backend` | no | SVG-in-OpenType glyph rendering via `resvg` + `tiny-skia` |
| `oxifont-backend` | no | Adds the `OxifontRaster` backend (pulls in `oxifont-parser` / `oxifont-core` + `tiny-skia`) |
| `png-bitmap` | no | Decode PNG-compressed CBDT/CBLC and sbix bitmap strikes (pulls in `png`). Off by default: `png` pulls `flate2` → `miniz_oxide`, both banned by this repository's `deny.toml` in favor of the `oxiarc-*` stack. Uncompressed CBDT bitmap formats and COLRv0/v1 vector color glyphs are unaffected and need no feature |

## Error variants

Fallible methods return `Result<_, oxitext_core::OxiTextError>` and use the `OxiTextError::Raster(String)` variant — e.g. a poisoned internal mutex or a fontdue parse failure. Many entry points (`rasterize_positioned`, `FontdueRaster::rasterize`) are infallible and return a (possibly zero-sized) bitmap rather than an error.

## Cross-references

- [`oxitext`](https://crates.io/crates/oxitext) — high-level façade combining all stages.
- [`oxitext-core`](https://crates.io/crates/oxitext-core) — the shared `Bitmap` / `RenderOutput` / `PositionedGlyph` types.
- [`oxitext-shape`](https://crates.io/crates/oxitext-shape) — produces the glyphs that get rasterized.
- [`oxitext-layout`](https://crates.io/crates/oxitext-layout) — positions glyphs before rasterization.
- `oxitext-sdf` — consumes `rasterize_for_sdf` output to build signed-distance fields.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
