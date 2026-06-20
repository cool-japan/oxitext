# OxiText

**OxiText** is the COOLJAPAN Pure-Rust text **pipeline**: shape → bidi-reorder → line-break
→ layout → rasterize. It replaces the canonical C/C++ text stack —
**harfbuzz** (shaper), **pango/icu** (layout + bidi + segmentation),
**freetype** (outline rasterizer) — with a curated composition of mature
Pure-Rust crates. OxiText complements **OxiFont** (font parsing + discovery)
and is consumed by every Oxi*** subsystem that draws or measures text.

## Status: 0.1.3 — 2026-06-19

All milestones M0–M7 are complete. **655 tests passing** (nextest, all-features).
Zero warnings, pure Rust default features, MSRV 1.89.

### What is implemented

| Feature | Crate | Details |
|---|---|---|
| Text shaping | `oxitext-shape` | swash (primary), rustybuzz (opt-in); Arabic, Devanagari, Thai, CJK, Latin |
| Script detection | `oxitext-shape` | `requires_arabic_shaping`, `requires_indic_shaping`, `requires_mark_positioning` |
| Font fallback chains | `oxitext-shape` | Automatic script-based font selection via oxifont-db |
| UAX #9 bidi reorder | `oxitext-layout` | Full right-to-left, bidirectional text support |
| UAX #14 line-break | `oxitext-layout` | Word-aware greedy wrapping, mandatory breaks, hyphenation |
| Text alignment | `oxitext-layout` | Left, Right, Center, Justify |
| Vertical text (UAX #50) | `oxitext-layout` | Upright/rotated classification, tate-chu-yoko |
| Glyph rasterization | `oxitext-raster` | fontdue (primary), ab_glyph (opt-in), swash (opt-in) |
| Subpixel rendering | `oxitext-raster` | Quarter-pixel positioning + LCD 3-tap / 5-tap FIR filter |
| Color glyphs | `oxitext-raster` | COLRv0, COLRv1 gradients, CBDT/CBLC bitmaps, SVG (resvg) |
| SDF atlas | `oxitext-sdf` | Single-channel SDF, MSDF, MTSDF, analytic, GPU descriptors |
| ICU4X integration | `oxitext-icu` | CLDR segmentation, Unicode Collation, NFC/NFD/NFKC/NFKD |
| Pipeline facade | `oxitext` | `Pipeline::measure`, `shape_and_layout`, `render_to_image`, `composite_to_rgba` |
| SIMD acceleration | `oxitext-raster`, `oxitext-sdf` | wide f32x8 hot-loops, feature-gated |
| LRU shape cache | `oxitext-shape` | Per-Pipeline cache keyed on font Arc pointer + text |
| Variational fonts | `oxitext-shape` | OpenType variation axis support |

## Crate Structure

```
oxitext-core      — shared value types (ShapedGlyph, PositionedGlyph, Bitmap, TextStyle …)
oxitext-shape     — text shaping (swash + rustybuzz backends, script detection, fallback)
oxitext-layout    — bidi reorder, line-break, vertical text, tate-chu-yoko, hyphenation
oxitext-raster    — glyph rasterization (fontdue, ab_glyph, swash, COLRv0/v1, SVG)
oxitext-sdf       — SDF/MSDF/MTSDF atlas generation for GPU text rendering
oxitext-icu       — ICU4X CLDR segmentation, collation, normalization
oxitext           — facade crate: Pipeline combining all layers
oxitext-bench     — criterion benchmarks (publish = false)
```

## Quick Start

```toml
[dependencies]
oxitext = "0.1.3"  # latest stable
```

```rust
use oxitext::{Pipeline, prelude::*};

let font_data = std::fs::read("my-font.ttf")?;
let pipeline = Pipeline::from_bytes(&font_data)?;

// Measure a string
let metrics = pipeline.measure("Hello, world!", &TextStyle::default())?;
println!("width={:.1} height={:.1}", metrics.width, metrics.height);

// Shape, lay out, and rasterize to RGBA pixels
let result = pipeline.render_to_image("Hello, OxiText!", &TextStyle::default())?;
println!("{} lines, {}x{} pixels", result.lines.len(), result.width, result.height);
```

## Features

| Feature | Default | Description |
|---|---|---|
| `pure` | yes | Enables oxitext-shape and oxitext-raster |
| `sdf` | no | Enables SDF atlas generation (oxitext-sdf) |
| `icu` | no | Enables ICU4X CLDR segmentation / collation |
| `simd` | no | SIMD hot-loops via `wide` f32x8 |
| `parallel` | no | Rayon-based parallel batch shaping |
| `png-output` | no | PNG export for atlas and raster results |

### oxitext-raster sub-features

| Feature | Description |
|---|---|
| `ab-glyph-backend` | ab_glyph alternate rasterizer |
| `swash-backend` | Swash rasterizer with TrueType hinting |
| `svg-backend` | SVG color glyph rendering via resvg |
| `oxifont-backend` | oxifont-parser outline extraction |
| `simd` | SIMD raster hot-loop |

### oxitext-shape sub-features

| Feature | Description |
|---|---|
| `rustybuzz-backend` | harfbuzz-compatible alternate shaper |
| `system-fonts` | System font loading via oxifont-db |
| `icu` | ICU4X CLDR line segmentation |

## Replaces (FFI Being Eliminated)

- `harfbuzz` / `harfbuzz-sys` — replaced by `swash` + `rustybuzz`
- `pango` — replaced by `oxitext-layout` (bidi + line-break + vertical)
- `icu_uc-sys` — replaced by `oxitext-icu` (icu4x, 100% Pure Rust)
- `freetype` — replaced by `fontdue` + `ab_glyph` + `swash`

## Inter-Oxi Dependencies

- **Depends on:** [`oxifont`](https://github.com/cool-japan/oxifont) for font
  parsing, OpenType-table access, glyph outlines, and font discovery feeding
  the fallback chain.
- **Depended on by:** `oximedia` (subtitles, captions), `oxigdal-symbology`
  (map labels), `oxiphoton` (text on images), `oxigaf` (PDF/EPUB reflow),
  `OxiUI` (every widget, GPU glyph atlas from `oxitext-sdf`), `oxirag`
  (document rendering).

## Architecture

OxiText is a **3-stage pipeline**: **shape → layout → raster**.

1. **Shape** (`oxitext-shape`): Convert Unicode text to `ShapedRun` (glyphs
   with advances, clusters, GSUB/GPOS applied). The primary backend is
   `swash`; `rustybuzz` is opt-in for harfbuzz test-suite parity.

2. **Layout** (`oxitext-layout`): Bidi-reorder (UAX #9), break into lines
   (UAX #14), align, compute `PositionedGlyph` screen coordinates. `oxitext-icu`
   provides CLDR-accurate break opportunities as opt-in.

3. **Raster** (`oxitext-raster`): Render each `PositionedGlyph` to a
   coverage bitmap via fontdue. Alt backends: ab_glyph (sharper hints on
   some platforms), swash with TrueType hinting, resvg for SVG color glyphs.
   `oxitext-sdf` produces GPU-ready SDF/MSDF atlas tiles from the same bitmaps.

Entirely **Pure Rust** — no FFI escape hatch, not even as an opt-in adapter.
CPU-only and headless-testable by design; GPU rasterization is handled by
OxiUI/wgpu consuming the `oxitext-sdf` atlas output.

## Workspace

- Version: **0.1.3** (2026-06-19)
- Edition: **2021**
- MSRV: **1.89**
- License: **Apache-2.0**
- Author: **COOLJAPAN OU (Team Kitasan)**
- Repository: <https://github.com/cool-japan/oxitext>

## License

Apache-2.0 — see [LICENSE](LICENSE) for details.
