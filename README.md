# OxiText

**OxiText** is the COOLJAPAN Pure-Rust text **pipeline**: shape → bidi-reorder → line-break
→ layout → rasterize. It replaces the canonical C/C++ text stack —
**harfbuzz** (shaper), **pango/icu** (layout + bidi + segmentation),
**freetype** (outline rasterizer) — with a curated composition of mature
Pure-Rust crates. OxiText complements **OxiFont** (font parsing + discovery)
and is consumed by every Oxi*** subsystem that draws or measures text.

## Status: 0.2.4 (2026-08-12)

All milestones M0–M7 are complete. See `CHANGELOG.md`'s `[0.2.3]` section for
what landed since the 0.2.2 release. **851 tests passing**
(`cargo nextest run --workspace --exclude oxitext-bench --all-features`; 16
further tests are `#[ignore]`/env-var-gated fixture sweeps not counted here;
749 pass with default features), plus 80 doctests.
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
| Color glyphs | `oxitext-raster` | COLRv0, COLRv1 gradients (default); CBDT/CBLC + `sbix` bitmaps (uncompressed strikes default; PNG strikes need `color-bitmap-fonts`, which is deny-clean — the decoder is `oxitext-core`'s own `oxiarc`-backed `png_decode`, not the banned `png`/`flate2` stack); SVG via resvg (needs `svg-backend`, or `svg-glyphs` through the `oxitext` facade — not deny-clean, see Cargo.toml) |
| SDF atlas | `oxitext-sdf` | Single-channel SDF, MSDF, MTSDF, analytic, GPU descriptors |
| ICU4X integration | `oxitext-icu` | CLDR segmentation, Unicode Collation, NFC/NFD/NFKC/NFKD |
| Pipeline facade | `oxitext` | `Pipeline::measure`, `shape_and_layout`, `render_to_image`, `composite_to_rgba` |
| SIMD acceleration | `oxitext-raster`, `oxitext-sdf` | wide f32x8 hot-loops, feature-gated |
| LRU shape cache | `oxitext-shape` | Per-Pipeline cache keyed on font Arc pointer + text |
| Variational fonts | `oxitext-shape` | OpenType variation axis support |

## Crate Structure

```
oxitext-core      — shared value types (ShapedGlyph, PositionedGlyph, Bitmap, TextStyle …)
oxitext-swash     — vendored fork of `swash` (see "Third-party code" below); shaper backend
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
oxitext = "0.2.4"  # latest stable
```

```rust
use oxitext::{Pipeline, prelude::*};

let font_data = std::fs::read("my-font.ttf")?;
let mut pipeline = Pipeline::from_bytes(&font_data)?;

// Measure a string
let metrics = pipeline.measure("Hello, world!", &TextStyle::default())?;
println!("width={:.1} height={:.1}", metrics.total_width, metrics.total_height);

// Shape, lay out, and rasterize to RGBA pixels
let bg = Rgba8 { r: 255, g: 255, b: 255, a: 255 };
let fg = Rgba8 { r: 0, g: 0, b: 0, a: 255 };
let image = pipeline.render_to_image("Hello, OxiText!", &TextStyle::default(), bg, fg)?;
println!("{}x{} RGBA pixels", image.width, image.height);
```

See `crates/oxitext/examples/quick_start.rs` for the compile-checked, runnable version of
this snippet (`cargo run -p oxitext --example quick_start`), including the lower-level
`Pipeline::render` call that returns per-line and per-glyph layout data.

## Features

| Feature | Default | Description |
|---|---|---|
| `pure` | yes | Enables oxitext-shape and oxitext-raster |
| `sdf` | no | Enables SDF atlas generation (oxitext-sdf) |
| `icu` | no | Enables ICU4X CLDR segmentation / collation |
| `simd` | no | SIMD hot-loops via `wide` f32x8 |
| `parallel` | no | Rayon-based parallel batch shaping |
| `png-output` | no | PNG export for atlas and raster results (via `oxitext-core`'s deny-clean encoder, not the `png` crate) |
| `font-subset` | no | Font subsetting via oxifont-subset |
| `color-bitmap-fonts` | no | Decode PNG-encoded CBDT/sbix color-bitmap strikes through `Pipeline` (pulls banned `png`/`flate2`; uncompressed CBDT/sbix strikes work without it) |
| `svg-glyphs` | no | Render OpenType `SVG ` color glyphs through `Pipeline` (pulls banned `flate2` via `usvg`, unconditionally — not deny-clean even so; documented trade-off) |

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

- Version: **0.2.4** (2026-08-12)
- Edition: **2021**
- MSRV: **1.89**
- License: **Apache-2.0**
- Author: **COOLJAPAN OU (Team Kitasan)**
- Repository: <https://github.com/cool-japan/oxitext>

## License

Apache-2.0 — see [LICENSE](LICENSE) for details.

### Third-party code

`crates/oxitext-swash` is a **vendored fork of
[`swash`](https://github.com/dfrg/swash) 0.2.10 by Chad Brokaw**, carrying
OxiText's fix for two Indic reordering defects (one of them upstream issue
[dfrg/swash#93](https://github.com/dfrg/swash/issues/93)). Upstream offers that
work under `Apache-2.0 OR MIT`; OxiText elects and redistributes it under the
Apache-2.0 arm, so the workspace licence is unchanged. Both upstream licence
files ship verbatim beside the code —
[`crates/oxitext-swash/LICENSE-APACHE`](crates/oxitext-swash/LICENSE-APACHE) and
[`crates/oxitext-swash/LICENSE-MIT`](crates/oxitext-swash/LICENSE-MIT) — together
with `NOTICE` and a `PROVENANCE.md` recording exactly which files diverge from
upstream and why.
