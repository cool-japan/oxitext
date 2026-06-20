# oxitext-sdf — Signed-distance-field glyph atlas generation for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-sdf.svg)](https://crates.io/crates/oxitext-sdf)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-sdf` turns glyph coverage bitmaps and outlines into GPU-ready signed-distance-field (SDF) texture atlases. It powers resolution-independent text rendering: a single small atlas can be sampled in a fragment shader to draw crisp glyphs at any magnification, with cheap effects such as outlines, glows, and soft shadows.

The crate is **100% Pure Rust** — no C/C++ dependencies. It implements the Felzenszwalb–Huttenlocher Euclidean distance transform (EDT) for single-channel SDFs, a Chlumsky-style edge-coloring pipeline for multi-channel SDFs (MSDF/MTSDF) generated directly from outlines, an analytic per-pixel SDF generator, a pseudo-distance (PSDF) variant, three bin-packing algorithms (shelf, MaxRects, skyline) with single- and multi-page atlases, a self-describing binary atlas format, and a backend-agnostic GPU descriptor that uploads to wgpu/Vulkan/Metal/OpenGL without depending on any of them. Outline parsing uses `ttf-parser`; packing parallelizes with `rayon`.

## Installation

```toml
[dependencies]
oxitext-sdf = "0.1.3"
```

### Feature flags

```toml
# SIMD-accelerated EDT inner loop (pulls in the `wide` crate)
oxitext-sdf = { version = "0.1.3", features = ["simd"] }
```

| Feature | Default | Effect |
|---------|---------|--------|
| `sdf` | no | Marker feature for the public SDF API. All symbols are public by default; enabling it makes intent explicit. |
| `simd` | no | Enables `wide`-based SIMD lanes in the distance-transform hot loop. No API change. |

## Quick Start

Single-channel SDF from a coverage bitmap, packed into an atlas:

```rust
use oxitext_sdf::{compute_sdf, SdfAtlas, SdfTile};

// A solid 32×32 square (inside everywhere).
let coverage = vec![255u8; 32 * 32];
let sdf = compute_sdf(&coverage, 32, 32, 8.0, 0)?;
assert_eq!(sdf.len(), 32 * 32);

// Pack a single tile into an atlas.
let tile = SdfTile {
    glyph_id: 0,
    width: 32,
    height: 32,
    data: sdf,
    bearing_x: 0,
    bearing_y: 0,
    advance_x: 32.0,
};
let atlas = SdfAtlas::pack(&[tile]);
assert!(atlas.uv_map.contains_key(&0));
# Ok::<(), oxitext_sdf::SdfError>(())
```

Multi-channel SDF straight from a font outline, then GPU upload:

```rust,no_run
use oxitext_sdf::{glyph_to_msdf_tile, MsdfAtlas, GpuAtlasFormat};

let font = std::fs::read("font.ttf")?;
// 64×64 MSDF tile at 48px, spread 4, padding 2.
let tile = glyph_to_msdf_tile(&font, /* glyph_id */ 36, 48.0, 64, 64, 4.0, 2)?
    .expect("glyph has an outline");

let atlas = MsdfAtlas::pack(&[tile], 512);
let desc = atlas.to_gpu_descriptor();
assert_eq!(desc.format, GpuAtlasFormat::Rgb8Unorm);
// Upload desc.data (RGB) + desc.uv_map to your GPU texture.
# Ok::<(), Box<dyn std::error::Error>>(())
```

## API Overview

### Single-channel SDF (coverage-based)

| Item | Kind | Description |
|------|------|-------------|
| `compute_sdf(coverage, width, height, spread, padding)` | fn | Felzenszwalb–Huttenlocher EDT over a `u8` coverage bitmap → `Vec<u8>` SDF (`<128` outside, `≈128` on the outline, `>128` inside). |
| `edt_2d(grid, width, height)` | fn | Raw 2D Euclidean distance transform over an `f32` seed grid (squared distances). |
| `glyph_to_sdf_tile(coverage, src_width, src_height, tile_size)` | fn | Compute an SDF at source resolution then bilinearly resample to `tile_size × tile_size`. Default spread `8.0`. |
| `bitmap_to_sdf_tile(bitmap, glyph_id, bearing_x, bearing_y, advance_x, spread)` | fn | Convert an `oxitext-core` coverage `Bitmap` into an `SdfTile` (`None` for empty bitmaps). |

### Outline-based SDF generators

| Item | Kind | Description |
|------|------|-------------|
| `glyph_to_sdf_tile_analytic(face_data, glyph_id, px_size, tile_size, spread)` | fn | Analytic per-pixel true SDF rendered directly from the outline → `Option<SdfTile>`. |
| `glyph_to_psdf_tile(face_data, glyph_id, px_size, tile_size, spread)` | fn | Pseudo-distance-field tile (perpendicular distance) → `Option<PsdfTile>`. |
| `extract_glyph_shape(face_data, glyph_id)` | fn | Parse a glyph outline into a `GlyphShape` (`Option`). |
| `color_edges(&mut shape)` | fn | Assign Chlumsky edge colors to a `GlyphShape` in place (call before `compute_msdf`). |

### Multi-channel SDF (MSDF / MTSDF)

| Item | Kind | Description |
|------|------|-------------|
| `compute_msdf(shape, width, height, spread, scale, offset_x, offset_y)` | fn | 3-channel RGB MSDF from a colored `GlyphShape` → `Vec<u8>` (`w·h·3`). |
| `compute_mtsdf(shape, width, height, spread, scale, offset_x, offset_y)` | fn | 4-channel RGBA MTSDF: RGB from MSDF, alpha from a true SDF → `Vec<u8>` (`w·h·4`). |
| `glyph_to_msdf_tile(face_data, glyph_id, px_size, tile_width, tile_height, spread, padding)` | fn | End-to-end MSDF tile from a font → `Option<MsdfTile>`. |
| `glyph_to_mtsdf_tile(face_data, glyph_id, px_size, tile_width, tile_height, spread, padding)` | fn | End-to-end MTSDF tile from a font → `Option<MtsdfTile>`. |
| `EdgeColor` | struct | Bit-flag edge color (`RED`, `GREEN`, `BLUE`, `YELLOW`, `CYAN`, `MAGENTA`, `WHITE`) with `has_red`/`has_green`/`has_blue`. |
| `GlyphShape` | struct | Decomposed outline (contours, `units_per_em`) used by all outline generators. |
| `MsdfTile` | struct | `glyph_id`, `width`, `height`, `data` (RGB), `bearing_x`, `bearing_y`, `advance_x`. |
| `MtsdfTile` | struct | As `MsdfTile` but `data` is RGBA (`w·h·4`). |

### Tiles and atlases

| Item | Kind | Description |
|------|------|-------------|
| `SdfTile` | struct | A single single-channel tile: `glyph_id`, `width`, `height`, `data`, `bearing_x/y`, `advance_x`. |
| `SdfTile::from_coverage(glyph_id, coverage, width, height, spread, bearing_x, bearing_y, advance_x)` | fn | Build a tile from an `f32` coverage bitmap (values in `[0, 1]`). |
| `SdfAtlas` | struct | Single-channel atlas: `width`, `height`, `texture` (`u8`), `uv_map: HashMap<u16, UvRect>`. |
| `SdfAtlas::new(width, height)` / `with_capacity(...)` | fn | Construct a blank / capacity-hinted atlas. |
| `SdfAtlas::pack(tiles)` | fn | Power-of-two shelf-pack a tile set. |
| `SdfAtlas::pack_with_options(tiles, options)` | fn | Pack with `AtlasOptions`; returns `(SdfAtlas, AtlasStats)`. |
| `SdfAtlas::pack_growing(tiles, initial_size, max_size)` | fn | Pack with dynamic atlas growth; returns `(SdfAtlas, AtlasStats)`. |
| `SdfAtlas::add_tile(&mut self, tile)` / `remove_tile(glyph_id)` | fn | Incrementally insert (`Option<UvRect>`) or remove (`bool`) a tile. |
| `SdfAtlas::to_bytes()` / `from_bytes(data)` / `from_static(data)` | fn | Serialize to / load from the `SDFA` binary atlas format. |
| `SdfAtlas::export_png(path)` | fn | Write the atlas texture as a greyscale PNG. |
| `MsdfAtlas` | struct | Multi-channel atlas; `MsdfAtlas::pack(tiles, atlas_size)`, `export_png(path)`. |
| `MultiPageAtlas` | struct | Multi-page packer for tile sets that exceed one texture; `pack(tiles, page_size, padding)`, `lookup(glyph_id) -> Option<(page, &UvRect)>`. |
| `pack_growing(tiles, initial_size, max_size, padding)` | fn | Free-function growing packer. |
| `UvRect` | struct | `u_min`, `v_min`, `u_max`, `v_max` (all in `[0, 1]`). |
| `AtlasOptions` | struct | `atlas_size`, `padding`, `max_size`, `algorithm`. |
| `AtlasStats` | struct | `tiles_packed`, `tiles_dropped`, `utilization`, `wasted_pixels`. |
| `PackingAlgorithm` | enum | `Shelf` (default), `MaxRects`, `Skyline`. |

### GPU descriptors

| Item | Kind | Description |
|------|------|-------------|
| `SdfAtlas::to_gpu_descriptor()` / `MsdfAtlas::to_gpu_descriptor()` | fn | Produce a backend-agnostic `GpuAtlasDescriptor`. |
| `GpuAtlasDescriptor` | struct | `width`, `height`, `format`, `data`, `uv_map: HashMap<u16, NormalizedUvRect>`, `glyph_metrics`. |
| `GpuAtlasFormat` | enum | `R8Unorm` (single-channel) or `Rgb8Unorm` (MSDF). |
| `NormalizedUvRect` | struct | UV rectangle in normalized `[0, 1]` coordinates. |
| `AtlasGlyphMetrics` | struct | `bearing_x`, `bearing_y`, `advance_x`, `width_px`, `height_px`. |

### Build-time helpers

| Item | Kind | Description |
|------|------|-------------|
| `generate_atlas_binary(font_data, glyph_ids, px_size, tile_size, spread, atlas_size, output_path)` | fn | Render a glyph set to a `.bin` atlas on disk (errors if tiles overflow). |
| `generate_ascii_atlas(font_data, px_size, output_path)` | fn | Convenience: pre-bake the printable-ASCII range (64×64 tiles, spread 4, 512×512 atlas). |

### Error type

| Variant | Description |
|---------|-------------|
| `SdfError::InvalidInput(String)` | Coverage slice length does not match `width × height`. |
| `SdfError::ZeroSize` | Width or height was zero. |
| `SdfError::InvalidFont` | Font bytes could not be parsed by `ttf-parser`. |
| `SdfError::InvalidData(String)` | Binary atlas data is malformed (bad magic, version mismatch, truncated). |
| `SdfError::Io(String)` | I/O error (e.g. during PNG/atlas export). |

## Cross-references

- [`oxitext`](../oxitext) — the facade; re-exports this crate under `oxitext::sdf` behind the `sdf` feature and provides `Pipeline::render_to_sdf_atlas`.
- [`oxitext-core`](../oxitext-core) — shared `Bitmap`/glyph types consumed by `bitmap_to_sdf_tile`.
- [`oxitext-raster`](../oxitext-raster) — produces the coverage bitmaps that feed `SdfTile::from_coverage`.
- [`oxitext-shape`](../oxitext-shape) · [`oxitext-layout`](../oxitext-layout) · [`oxitext-icu`](../oxitext-icu) · [`oxitext-bench`](../oxitext-bench) — sibling crates in the OxiText pipeline.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
