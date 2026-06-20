# oxitext-shape — Swash-based text shaper for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-shape.svg)](https://crates.io/crates/oxitext-shape)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-shape` is the **shaping** stage of the OxiText pipeline (a HarfBuzz-like step). It turns UTF-8 text plus raw font bytes into ordered `oxitext_core::ShapedGlyph`s, applying OpenType GSUB/GPOS (ligatures, kerning, marks), right-to-left form selection, vertical CJK substitution (`vert`/`vrt2`), and font fallback. The default backend wraps [swash](https://crates.io/crates/swash); an optional [rustybuzz](https://crates.io/crates/rustybuzz) backend is available behind a feature flag.

This crate is **100% Pure Rust** and `#![forbid(unsafe_code)]`. The default shaping path (swash) carries no C/C++ dependencies. Optional features add script-aware itemisation via `oxitext-icu` (ICU4X) and system-font discovery via `oxifont`. Shaping consumes types from [`oxitext-core`](https://crates.io/crates/oxitext-core); downstream, [`oxitext-layout`](https://crates.io/crates/oxitext-layout) positions the glyphs and [`oxitext-raster`](https://crates.io/crates/oxitext-raster) rasterizes them.

## Installation

```toml
[dependencies]
oxitext-shape = "0.1.3"
```

With optional capabilities:

```toml
[dependencies]
# rustybuzz alternative backend, ICU4X script itemisation, and system-font lookup
oxitext-shape = { version = "0.1.3", features = ["rustybuzz-backend", "icu", "system-fonts"] }
```

## Quick Start

Keep a single `SwashShaper` alive across passes to amortise swash's internal caches.

```rust,no_run
use oxitext_shape::SwashShaper;
use std::sync::Arc;

let font_data: Arc<[u8]> = Arc::from(std::fs::read("font.ttf")?.as_slice());

let mut shaper = SwashShaper::new();
let run = shaper.shape("Hello", Arc::clone(&font_data), 16.0)?;

// Each glyph's x_advance is already in pixels (scaled by the 16px size).
let total: f32 = run.glyphs.iter().map(|g| g.x_advance).sum();
println!("{} glyphs, total advance {total}px", run.glyphs.len());
# Ok::<(), oxitext_core::OxiTextError>(())
```

### Feature-aware shaping with a request builder

```rust,no_run
use oxitext_shape::{SwashShaper, ShapeRequest, ShapeDirection, ShapeFeature};

let font: Vec<u8> = std::fs::read("font.ttf")?;
let mut shaper = SwashShaper::new();

let req = ShapeRequest::builder()
    .text("ﬁle")
    .font_data(&font)
    .px_size(18.0)
    .direction(ShapeDirection::Ltr)
    .feature(ShapeFeature::LIGA)   // standard ligatures
    .feature(ShapeFeature::KERN)   // kerning
    .build()?;

let glyphs = shaper.shape_request(&req)?;
println!("{} shaped glyphs", glyphs.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## API Overview

### `SwashShaper`

The primary shaper. Methods that take `Arc<[u8]>` cooperate with the optional `ShapeCache`; methods taking `&[u8]` are lower-level conveniences.

| Method | Description |
|--------|-------------|
| `new()` | Create a shaper with no application-level shape cache |
| `with_cache(capacity)` | Create a shaper with an attached LRU `ShapeCache` of `capacity` entries |
| `shape_cache()` | Returns the attached `Arc<ShapeCache>`, if any |
| `shape(text, font_data, size)` | Shape LTR; returns a `ShapedRun`. Checks the cache before invoking swash |
| `shape_with_direction(text, font_data, size, rtl)` | Shape with explicit RTL control; RTL output is sorted to logical (ascending-cluster) order |
| `shape_request(&req)` | Shape a full `ShapeRequest`; auto-injects `vert`/`vrt2` for vertical text and auto-upgrades Arabic to RTL |
| `shape_with_features(font_data, text, px_size, rtl, features)` | Lower-level entry point accepting an explicit feature slice (no auto-injection) |
| `shape_full(font_data, text, px_size)` | Shape LTR and return a rich `ShapeResult` (glyphs, direction, missing codepoints, cluster boundaries) |
| `shape_slice(font_data, text, px_size)` | Shape LTR from raw bytes, returning `Vec<ShapedGlyph>` |
| `shape_slice_rtl(font_data, text, px_size)` | Shape RTL from raw bytes, in logical order |
| `shape_with_fallback(fonts, text, px_size)` | Shape with a fallback chain: re-shape `.notdef` runs with `fonts[1..]` |
| `font_has_aat(font_data)` | `true` if the font carries AAT tables (`morx`/`kerx`/`ankr`) — informational |
| `shape_with_aat_fallback(font_data, text, px_size)` | Shape via swash (handles AAT transparently); returns a `ShapeResult` |

### Builder-pattern request types

| Type | Key items | Description |
|------|-----------|-------------|
| `ShapeRequest<'a>` | `text`, `font_data`, `px_size`, `direction`, `script`, `language`, `features`; `builder()` | A complete shaping request |
| `ShapeRequestBuilder<'a>` | `text()`, `font_data()`, `px_size()`, `direction()`, `script()`, `language()`, `feature()`, `build()` | Fluent builder for `ShapeRequest` |
| `ShapeFeature` | `tag: [u8; 4]`, `value: u32`; `new()`, `enable()`, `disable()`; consts `LIGA`, `KERN`, `SMCP`, `CALT`, `VERT`, `VRT2` | An OpenType feature tag-value pair |
| `ShapeDirection` | `Ltr` (default), `Rtl`, `Ttb`, `Btt` | Text direction for a shaping request |

### Results and metadata

| Type | Key fields / methods | Description |
|------|----------------------|-------------|
| `ShapeResult` | `glyphs`, `script_detected`, `direction`, `missing_codepoints`, `cluster_boundaries`; `from_glyphs()` | Extended shaping result with metadata |

### Backends — `backend` module

| Item | Description |
|------|-------------|
| `ShapeBackend` (trait) | Swappable shaping backend (`Send + Sync`). Methods: `shape`, `shape_with_direction`, `shape_with_features`, `shape_with_options`, `supports_script` |
| `SwashShaperBackend` | Default backend wrapping `SwashShaper` behind a `RwLock`; `new()` |
| `RustybuzzShaper` | Alternative rustybuzz backend (feature `rustybuzz-backend`) |

### Shape cache — `cache` module

| Item | Description |
|------|-------------|
| `ShapeCache` | Bounded LRU cache of `Arc<ShapedRun>`; `new(capacity)`, `get()`, `insert()`, `len()`, `is_empty()` |
| `ShapeKey` | Cache key over font pointer identity + text + axis hash; `new(font_data, text, axis_values_hash)` |
| `FontId` | Type alias `u64` for font identity |

### Batch shaping — `batch` module (`SwashShaper` methods)

| Method | Description |
|--------|-------------|
| `shape_batch(font_data, segments, px_size)` | Shape many segments sharing one font/size; one `ShapeResult` per segment |
| `shape_batch_directed(font_data, segments, px_size)` | Shape `(text, direction)` pairs; vertical directions inject `vert`/`vrt2` |
| `shape_batch_with_features(font_data, segments, px_size, features)` | Shape a batch with a shared feature list |

### Variable fonts — `variational` module (`SwashShaper` method)

| Method | Description |
|--------|-------------|
| `shape_with_variations(font_data, text, px_size, variations)` | Shape with `(axis_tag, value)` variation pairs (API surface; swash 0.2.x applies axes internally) |

### Script detection — `script_detect` module

| Function | Description |
|----------|-------------|
| `requires_arabic_shaping(text)` | `true` if the text contains Arabic-script characters needing joining/form selection |
| `requires_indic_shaping(text)` | `true` if the text contains Indic-script characters needing reordering |
| `requires_mark_positioning(text)` | `true` if the text contains combining marks needing GPOS positioning |

### Free functions

| Function | Description |
|----------|-------------|
| `find_kashida_opportunities(text, glyphs)` | Glyph indices after which an Arabic kashida (tatweel) stretch may be inserted for justification |
| `detect_emoji_zwj_sequences(text)` | Byte ranges of ZWJ-joined emoji grapheme clusters (UAX #29) |

### System fonts — `system_fonts` module (feature `system-fonts`)

| Item | Description |
|------|-------------|
| `build_system_db()` | Build a system `FontDatabase` |
| `load_font_for_family(family)` / `load_font_for_family_from(db, family)` | Load font bytes for a family name or CSS generic alias |
| `load_best_font_for_text(text)` / `load_best_font_for_text_from(db, text)` | Discover the best system font covering the text's Unicode content |
| `SwashShaper::shape_with_system_font(text, px_size)` | Shape using the best system font for the text |
| `SwashShaper::shape_with_family(text, family, px_size)` | Shape using the system font that best matches `family` |

### ICU4X itemisation (feature `icu`)

| Method | Description |
|--------|-------------|
| `SwashShaper::shape_by_script(font_data, text, px_size, features)` | Split text into per-script runs via ICU4X, then shape each with the right OpenType script tag; returns one `ShapedRun` per run (NFC-normalised) |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rustybuzz-backend` | no | Adds the `RustybuzzShaper` backend (pulls in `rustybuzz`) |
| `icu` | no | Script-aware itemisation and NFC normalisation via `oxitext-icu` (ICU4X) |
| `system-fonts` | no | System-font discovery via `oxifont` (the `system_fonts` module and `SwashShaper::shape_with_system_font` / `shape_with_family`) |

## Error variants

Shaping methods return `Result<_, oxitext_core::OxiTextError>`; failures use the `OxiTextError::Shaping(String)` variant (e.g. unparseable font bytes, empty font list, invalid script-run byte range). The request builder returns its own error:

| `ShapeRequestError` variant | Description |
|-----------------------------|-------------|
| `MissingText` | `ShapeRequestBuilder::build()` called without `text` |
| `MissingFont` | `ShapeRequestBuilder::build()` called without `font_data` |

## Cross-references

- [`oxitext`](https://crates.io/crates/oxitext) — high-level façade combining all stages.
- [`oxitext-core`](https://crates.io/crates/oxitext-core) — the shared `ShapedGlyph` / `ShapedRun` / `OxiTextError` types.
- [`oxitext-layout`](https://crates.io/crates/oxitext-layout) — positions `ShapedRun`s into lines and paragraphs.
- [`oxitext-raster`](https://crates.io/crates/oxitext-raster) — rasterizes shaped/positioned glyphs into bitmaps.
- `oxitext-icu` — ICU4X script itemisation and normalisation used by the `icu` feature.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
