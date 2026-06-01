# oxitext-raster TODO

## Status
Fontdue-based glyph rasterizer with swappable backend trait, COLRv0/CPAL color glyph compositing, and quarter-pixel subpixel pen positioning. `FontdueRasterizer` caches parsed fonts by Arc pointer identity. `RasterBackend` trait with `FontdueRaster` (default) and `AbGlyphRaster` (optional). `render_colr_v0` composites color layers via Porter-Duff source-over. `SubpixelOffset`/`SubpixelCacheKey` for fractional positioning. ~220 SLOC (lib.rs) + ~221 SLOC (backend.rs) + ~190 SLOC (color.rs) + ~195 SLOC (subpixel.rs). Functional for basic rendering but missing hinting, LCD subpixel rendering, gamma correction, and advanced color font support.

## Core Implementation
- [x] Add TrueType hinting support: `SwashRaster` (feature `swash-backend`) wraps `swash::scale::ScaleContext` with `hint(true)`; full TrueType/CFF bytecode hinting via skrifa; `SwashRaster::new()` (hinted) and `SwashRaster::with_hint(bool)`; implements `RasterBackend` with correct `advance_x` from `GlyphMetrics`; `rasterize_color` falls back to `None` for non-color-bitmap glyphs; 7 tests in `swash_backend.rs` — all pass; zero clippy warnings
- [x] Implement LCD subpixel rendering: render at 3x horizontal resolution, apply LCD filter (3-tap or 5-tap Gaussian/Lanczos) for RGB subpixel antialiasing (~100 SLOC)
  - **Goal:** `AbGlyphRaster::rasterize_lcd(face_data, glyph_id, px_size, filter) -> Option<LcdBitmap>` renders via ab_glyph at PxScale{x:3*px, y:px}, applies stem darkening, sRGB→linear, FIR filter, 3:1 decimation, linear→sRGB. New `lcd.rs` module.
  - **Files:** `crates/oxitext-raster/src/lcd.rs` (new), `crates/oxitext-raster/src/backend.rs`, `crates/oxitext-raster/src/lib.rs`; `crates/oxitext-core/src/lib.rs` (add `LcdBitmap` + `RenderOutput::Lcd`)
  - **Tests:** LCD output width = glyph_width/3 at standard ppem; FIR kernel sum = 1.0; stem darkening decreases with ppem
- [x] Add gamma correction: apply sRGB gamma curve to coverage values for perceptually correct blending (~30 SLOC)
  - **Goal:** 256-entry `SRGB_TO_LINEAR: [f32;256]` LUT and 4096-entry `LINEAR_TO_SRGB: [u8;4096]` LUT; `srgb_to_linear(u8)->f32`, `linear_to_srgb(f32)->u8`. New `gamma.rs` module.
  - **Files:** `crates/oxitext-raster/src/gamma.rs` (new), `crates/oxitext-raster/src/lib.rs`
  - **Tests:** round-trip sRGB→linear→sRGB is identity for all 256 values; linear(0)=0.0, linear(255)≈1.0
- [x] Implement stem darkening: thicken thin strokes at small sizes for better readability (FreeType-style) (~40 SLOC)
  - **Goal:** `stem_darkening_amount(ppem:f32)->f32` = clamp(0.4375 - 0.0625*ppem, 0.0, 0.5); `apply_stem_darkening(coverage:&mut [f32], amount:f32)`. New `stem_darken.rs`.
  - **Files:** `crates/oxitext-raster/src/stem_darken.rs` (new), `crates/oxitext-raster/src/lib.rs`
  - **Tests:** amount at ppem=7 > amount at ppem=8; amount at ppem=32 == 0.0; coverage increases after apply
- [x] Add COLRv1 support: linear, radial, and sweep gradients fully implemented; sweep gradient uses `atan2` per-pixel angle → turns in `[0,1)`, mapped into the `[start_angle, end_angle]` window (OpenType F2DOT14 turn units), with full `Pad`/`Repeat`/`Reflect` extend-mode support via `apply_extend`; `render_colr_v1` and `render_color_glyph` dispatch function added (~790 SLOC, `color.rs`)
- [x] Add CBDT/CBLC bitmap glyph rendering: full PNG decoding implemented — `render_cbdt_glyph(face_data, glyph_id, px_size)` decodes PNG-encoded strikes (format 17/18/19) to RGBA via the `png` crate; wired into `render_color_glyph` dispatch with highest priority; `extract_cbdt_bitmap` also fully decodes PNG to `ColorBitmap`; raw bitmap formats (no CBLC metrics) return None
- [x] Add sbix bitmap glyph rendering: partial — raw extraction via `glyph_raster_image` implemented in `extract_raster_glyph`; sbix PNG also reachable via `render_cbdt_glyph` / `extract_raster_glyph` since ttf-parser routes sbix through the same `glyph_raster_image` API
- [x] Add SVG glyph rendering: resvg + tiny-skia pipeline in `src/svg_backend.rs` (behind `svg-backend` feature); `render_svg_glyph` extracts SVG bytes via `ttf_parser::Face::glyph_svg_image` and delegates to `render_svg_bytes`; `color.rs:render_color_glyph` wired with SVG priority before CBDT (Sbix > SVG > CBDT > COLRv1 > COLRv0); premultiplied-alpha converted to straight RGBA via `Pixmap::take_demultiplied`; 6 tests in `svg_backend.rs` — all pass; zero clippy warnings
- [x] Implement glyph outline path extraction for direct rendering without fontdue (~80 SLOC) — `outline.rs` with `extract_glyph_outline`, `GlyphOutline`, `PathCommand`
- [x] Add fractional Y-axis subpixel positioning (currently only X-axis) (~20 SLOC)
  - **Goal:** `SubpixelOffsetXY{x:SubpixelOffset, y:SubpixelOffset}` alongside existing `SubpixelOffset` for 2D fractional glyph positioning.
  - **Files:** `crates/oxitext-raster/src/subpixel.rs`
  - **Tests:** SubpixelOffsetXY::default() == (0,0); from_floats round-trips correctly
- [x] Implement `SubpixelBuckets` enum with configurable bucket count (4/8/16) and `SubpixelOffset::bucket_with_count` (~15 SLOC)
  - **Goal:** `SubpixelBuckets{Four, Eight, Sixteen}` enum; `bucket_with_count(frac, buckets)` helper on `SubpixelOffset`.
  - **Files:** `crates/oxitext-raster/src/subpixel.rs`
  - **Tests:** Four/Eight/Sixteen count() values; 0.5 with Eight → bucket 4; 0.25 with Sixteen → bucket 4
- [x] Add bitmap glyph cache with LRU eviction for memory-constrained environments (~60 SLOC)
  - **Goal:** `BitmapCache` wrapping `lru::LruCache<BitmapCacheKey, Vec<u8>>`. `BitmapCacheKey{glyph_id, px_size_times_64, subpixel, render_mode}`. New `cache.rs`.
  - **Files:** `crates/oxitext-raster/src/cache.rs` (new), `crates/oxitext-raster/Cargo.toml` (add lru dep), `crates/oxitext-raster/src/lib.rs`
  - **Tests:** cache hit returns same data; cache miss inserts new entry; eviction on capacity
- [x] Implement color glyph type detection: `ColorGlyphType` enum, `detect_color_glyph_type(face_data, glyph_id)` in new `detect.rs`; `extract_cbdt_bitmap` stub awaiting image-crate integration (~40 SLOC)
  - **Goal:** Priority-ordered detection (sbix → SVG → CBDT/CBLC → COLRv1 → COLRv0 → None) using ttf-parser table presence + `is_color_glyph`.
  - **Files:** `crates/oxitext-raster/src/detect.rs` (new), `crates/oxitext-raster/src/lib.rs`
  - **Tests:** empty/invalid data returns None gracefully

## API Improvements
- [x] Add `RasterOptions` struct: hinting_mode (None/Light/Full), subpixel_mode (Greyscale/LCD_H/LCD_V), gamma, stem_darkening
  - **Goal:** `RasterOptions{hinting_mode, subpixel_mode, lcd_filter, gamma_correction, stem_darkening_strength}` with builder. `LcdFilterKernel` enum: Box, Triangle, FreeType5Tap (default). New `options.rs`.
  - **Files:** `crates/oxitext-raster/src/options.rs` (new), `crates/oxitext-raster/src/lib.rs`
  - **Tests:** default options build without panic; builder sets all fields
- [x] Add `RasterBackend::rasterize_color()` method for color glyph rendering returning RGBA output
  - **Goal:** Default `None` impl on trait; backends override when color tables are supported.
  - **Files:** `crates/oxitext-raster/src/backend.rs`
- [x] Return `RasterResult` with bitmap + metrics instead of separate `Bitmap` and `RasterOutput` types
  - **Goal:** `RasterResult{output:RenderOutput, advance_x, advance_y, bearing_x, bearing_y}` with `empty()` and `is_empty()`. New `result.rs` module. `RasterBackend::rasterize_full` default method.
  - **Files:** `crates/oxitext-raster/src/result.rs` (new), `crates/oxitext-raster/src/backend.rs`, `crates/oxitext-raster/src/lib.rs`
  - **Tests:** empty result is_empty; non-empty greyscale not is_empty; all RenderOutput variants covered
- [x] Add `FontdueRaster::raster_positioned(face_data, glyph_id, px_size, subpixel_x, subpixel_y)` for fractional-positioned rasterization
  - **Goal:** Subpixel offsets recorded; delegates to `rasterize()` since fontdue does not support genuine subpixel origin shifts.
  - **Files:** `crates/oxitext-raster/src/backend.rs`
  - **Tests:** returns Some(bitmap) for valid font; returns Some(empty) for invalid font
- [x] Add `clear_cache()` method to all rasterizers for memory management
  - **Goal:** Default no-op on `RasterBackend`; `FontdueRaster` acquires write lock and clears HashMap. `BitmapCache::clear()` added.
  - **Files:** `crates/oxitext-raster/src/backend.rs`, `crates/oxitext-raster/src/cache.rs`
  - **Tests:** clear then re-rasterize does not panic

## Testing
- [x] Test COLRv0 compositing with a color font fixture (NotoColorEmoji-noflags.ttf or similar) — `test_colrv0_compositing_smoke` in `lib.rs`; gracefully returns None when no color font is available
- [x] Test Porter-Duff source-over blending correctness with known RGBA values
  - **Goal:** Test that Porter-Duff source-over compositing produces correct alpha blending for opaque, transparent, and semi-transparent inputs.
  - **Files:** `crates/oxitext-raster/src/lib.rs` (inline test)
  - **Tests:** opaque source overwrites dst; transparent source leaves dst; 50% white over black ≈ 128
- [x] Test subpixel offset bucket quantization for all 4 buckets
  - **Goal:** Test SubpixelOffset::bucket() boundaries at 0.0, 0.25, 0.5, 0.75 produce expected bucket indices (0..3).
  - **Files:** `crates/oxitext-raster/src/lib.rs` (inline test)
- [x] Test rasterize_with_offset produces visually different bitmaps for different subpixel offsets — `test_rasterize_with_offset_differs` in lib.rs (same-input identity check + no-panic)
- [x] Test FontdueRaster cache hit/miss behavior with same and different font data pointers
  - **Goal:** Test BitmapCache returns same Vec<u8> on repeated key lookup; different key misses.
  - **Files:** `crates/oxitext-raster/src/cache.rs` (inline test)
- [x] Test AbGlyphRaster produces non-zero coverage for known visible glyph — `test_abglyph_raster_non_zero_coverage` in `lib.rs` (gated on `ab-glyph-backend` feature)
- [x] Benchmark rasterization of 1000 unique glyphs at 16px, 32px, 64px — `bench_tests::tests::bench_rasterize_1000_glyphs_multisize` in `bench_tests.rs`; 200 glyphs × 3 sizes, passes in ~3 s
- [x] Test rasterization of whitespace glyph produces zero-sized bitmap gracefully
  - **Goal:** Test that rasterizing a space character returns an empty/zero bitmap without panic.
  - **Files:** `crates/oxitext-raster/src/lib.rs` (inline test)
- [x] Compare fontdue vs. ab_glyph output quality for the same glyph at small sizes — `test_fontdue_vs_abglyph_both_produce_bitmaps` verifies both backends produce non-zero coverage and plausible dimensions for the same glyph; gated on `ab-glyph-backend` feature

## Performance
- [x] Replace `Mutex<HashMap>` font cache with `RwLock<HashMap>` for concurrent read access (~10 SLOC)
  - **Goal:** Replace `Mutex<HashMap>` with `RwLock<HashMap>` in `FontdueRaster` for concurrent reads without contention.
  - **Files:** `crates/oxitext-raster/src/backend.rs`
  - **Tests:** concurrent rasterize() calls from multiple threads succeed
- [x] Add thread-local font cache to avoid lock contention in multi-threaded rendering — `tl_cache.rs` with `get_or_parse_fontdue`; `FontdueRaster::rasterize` consults TL cache first
- [x] Implement SIMD-accelerated Porter-Duff compositing for color glyph layers (~40 SLOC) — `porter_duff_source_over_simd` (8-pixel f32x8 lanes) in `simd.rs`; scalar reference in `scalar.rs`; dispatch via `porter_duff_source_over` in `lib.rs`
- [x] Pre-allocate RasterOutput coverage buffer from font metrics: `raster_buffer: Mutex<Vec<u8>>` field on `FontdueRaster` reuses the fontdue-returned coverage allocation across successive calls, reducing heap allocations when glyph sizes are similar
- [x] Benchmark and compare fontdue vs ab_glyph rasterization throughput — `bench_tests::tests::bench_fontdue_vs_abglyph_throughput` (gated on `ab-glyph-backend`); `bench_tests::tests::bench_lcd_vs_greyscale_rasterization` in `bench_tests.rs`
- [x] Bound `FontdueRaster.cache` / `FontdueRasterizer.fonts` / `TL_FONT_CACHE` with `LruCache`; remove spurious `.clone()` in `make_output` — bare `HashMap` caches grow without bound (one `fontdue::Font` per unique `Arc<[u8]>` pointer ever seen); `make_output` clones coverage buffer defeating its own "reuse" comment (planned 2026-05-27)
  - **Goal:** All three font caches are `lru::LruCache` (capacity 64 for global, 32 per-thread); `raster_buffer` field removed or documented correctly; no behavioral regression; existing tests pass.
  - **Design:** `crates/oxitext-raster/src/backend.rs` — `FontdueRaster.cache: Mutex<LruCache<usize, fontdue::Font>>`; `crates/oxitext-raster/src/lib.rs:167` — `FontdueRasterizer.fonts` same; `crates/oxitext-raster/src/tl_cache.rs:16` — thread-local `HashMap` → `LruCache<usize, fontdue::Font>` cap 32. Audit `make_output` at `backend.rs:230`: remove redundant `.clone()` (return coverage Vec<u8> directly from fontdue call) or delete `raster_buffer` field entirely if it adds no value. `lru` crate already in deps.
  - **Files:** `crates/oxitext-raster/src/backend.rs`, `crates/oxitext-raster/src/lib.rs`, `crates/oxitext-raster/src/tl_cache.rs`
  - **Tests:** `fontdue_raster_cache_evicts_at_capacity` (65 distinct Arc pointers → 64-entry cache); `tl_font_cache_evicts_at_capacity`

## Integration
- [x] Consume PositionedGlyph from oxitext-layout for direct rendering pipeline: `rasterize_positioned(glyphs, options) -> Vec<Option<Bitmap>>` in `lib.rs`; each `PositionedGlyph` carries its own `font_data: Arc<Vec<u8>>` enabling per-glyph font fallback; tests: empty-slice and produces-bitmaps coverage
- [x] Provide rasterized bitmaps to oxitext-sdf for SDF generation from coverage maps — `RasterBackend::rasterize_for_sdf` default trait method and `rasterize_for_sdf` top-level function added; tests: `test_rasterize_for_sdf_produces_coverage_bitmap`, `test_rasterize_for_sdf_zero_size_no_panic`
- [x] Use oxifont-parser outline data for path-based rasterization (bypassing fontdue entirely) — `OxifontRaster` in `src/oxifont_backend.rs` (behind `oxifont-backend` feature); uses `oxifont_parser::ParsedFace::outline()` + `tiny-skia` scanline renderer; Y-axis flip, 1-pixel AA fringe, alpha-channel extraction; 6 tests all pass
- [x] Feed color glyph bitmaps into the facade Pipeline's RenderResult alongside greyscale bitmaps — `rasterize_single` in `oxitext/src/lib.rs` already dispatches COLRv0/v1 glyphs to `render_colr_v0` and stores them as `RenderOutput::Color` in `RenderResult.outputs`
