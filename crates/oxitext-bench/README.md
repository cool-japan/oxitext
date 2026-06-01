# oxitext-bench — Criterion benchmarks for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-bench.svg)](https://crates.io/crates/oxitext-bench)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-bench` is the benchmark harness for the OxiText pipeline. It measures the three performance-critical stages — **shaping**, **rasterization**, and the **end-to-end pipeline** — using [Criterion](https://crates.io/crates/criterion) with HTML reports. It exists to track regressions across releases and to compare the Pure-Rust path against the de-facto C/C++ baselines (HarfBuzz, Pango) when those optional features are enabled.

This is a **dev-only** crate: `publish = false`, no library API, and no `src/`. All code lives in `benches/` and is exercised through `cargo bench`. The default build is **100% Pure Rust**; the only C/C++ dependencies (`harfbuzz-sys`, `pango-sys`) are gated behind opt-in features per the COOLJAPAN Pure-Rust default-features policy, and are used solely as comparison baselines.

## Installation

This crate is not published and is consumed only inside the OxiText workspace. To run its benchmarks, clone the repository and use Cargo's `-p` selector (see below).

## Quick Start

```bash
# Run all benchmark groups (Pure-Rust, default features)
cargo bench -p oxitext-bench

# Run a single group
cargo bench -p oxitext-bench --bench shape
cargo bench -p oxitext-bench --bench raster
cargo bench -p oxitext-bench --bench pipeline

# Filter to one benchmark function by name
cargo bench -p oxitext-bench --bench pipeline -- pipeline_hello_world

# Compile the benchmarks without running them (CI gate)
cargo bench -p oxitext-bench --no-run
```

Criterion writes HTML reports to `target/criterion/`.

### Comparison baselines (optional, non-Pure-Rust)

```bash
# Add the HarfBuzz FFI comparison to the shaping group
cargo bench -p oxitext-bench --features harfbuzz --bench shape

# Enable the Pango baseline
cargo bench -p oxitext-bench --features pango
```

The `harfbuzz` and `pango` features link C libraries via FFI and are intended only for cross-implementation comparison, not for production builds.

## Benchmark Groups

### `shape` — glyph shaping

Drives `oxitext_shape::SwashShaper` across representative scripts; when `harfbuzz` is enabled, adds a HarfBuzz FFI baseline.

| Benchmark | What it measures |
|-----------|------------------|
| `shape_latin_oxitext` | Shaping `"Hello World"` (Latin) at 16px. |
| `shape_arabic_oxitext` | Shaping Arabic text (joining/RTL) at 16px. |
| `shape_cjk_oxitext` | Shaping CJK text at 16px. |
| `harfbuzz_version_string_ffi` *(feature `harfbuzz`)* | Minimal HarfBuzz FFI call confirming the C baseline links. |

### `raster` — glyph rasterization

Drives `oxitext_raster::FontdueRasterizer` and the coverage-buffer helpers.

| Benchmark | What it measures |
|-----------|------------------|
| `raster_glyph_16px` | Rasterizing a single glyph at 16px. |
| `raster_glyph_48px` | Rasterizing a single glyph at 48px (larger-bitmap path). |
| `raster_multi_glyph_12` | Rasterizing 12 glyphs in sequence (exercises the parse cache). |
| `accumulate_coverage_scalar_256` | Scalar coverage accumulation over 256 samples. |
| `coverage_f32_to_u8_256` | `f32` → `u8` coverage conversion over 256 samples. |

### `pipeline` — end-to-end render

Drives the full `oxitext::Pipeline` (shape → layout → raster).

| Benchmark | What it measures |
|-----------|------------------|
| `pipeline_hello_world` | Full render of a short Latin string. |
| `pipeline_long_text` | Full render of a ~135-char repeated string. |
| `pipeline_construction` | `Pipeline::from_bytes` construction cost (font loading). |

## Fonts

Each bench loads `tests/fixtures/test-font.ttf` from the workspace root via `CARGO_MANIFEST_DIR`. If the fixture is missing, it probes a few common system font paths; if none are found, the benchmark registers a no-op so the harness still compiles and runs without panicking.

## Feature Flags

| Feature | Default | Pure Rust | Description |
|---------|---------|-----------|-------------|
| *(none)* | yes | yes | Benchmarks the Pure-Rust OxiText pipeline only. |
| `harfbuzz` | no | no (C FFI) | Adds a `harfbuzz-sys` shaping baseline to the `shape` group. |
| `pango` | no | no (C FFI) | Adds a `pango-sys` baseline. |

## Cross-references

- [`oxitext`](../oxitext) — the facade exercised by the `pipeline` group.
- [`oxitext-shape`](../oxitext-shape) — the `SwashShaper` measured by the `shape` group.
- [`oxitext-raster`](../oxitext-raster) — the `FontdueRasterizer` measured by the `raster` group.
- [`oxitext-core`](../oxitext-core) · [`oxitext-layout`](../oxitext-layout) · [`oxitext-icu`](../oxitext-icu) · [`oxitext-sdf`](../oxitext-sdf) — sibling crates in the OxiText pipeline.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
