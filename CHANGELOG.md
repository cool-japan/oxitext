# Changelog

All notable changes to OxiText are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-07-30

### Fixed

- **`oxitext-layout`: tab stops on the second and later lines resolved the wrong source character** — `LayoutEngine::layout_with_options`'s tab-stop handler passed `find_cluster_for_positioned_glyph`/`advance_for_glyph` a glyph index *relative to the current line* (`gi - gs`), but both helpers walk `shaped_runs` from its very first glyph and ignore the `line_glyph_start` argument entirely — they need the glyph's *absolute* index within `shaped_runs`/`result.glyphs`. Every line after the first therefore looked up the tab-stop cascade against an unrelated character, so a `\t` past the first line could fail to be recognised as a tab and keep its untouched natural (non-cascaded) position. Both call sites now pass the absolute index `gi`. Regression test: `layout_with_options_tab_stops_resolve_correct_glyph_on_second_line`.
- **`oxitext-sdf`: `SdfAtlas::from_bytes` no longer risks an out-of-bounds panic on a malformed atlas** — `expected_len` was computed as `ENTRIES_OFFSET + num_entries * ENTRY_SIZE + texture_len` in plain `usize` arithmetic from three header fields that come straight from the (potentially untrusted) input buffer; a crafted or corrupted header (e.g. `atlas_w`/`atlas_h`/`num_entries` near `u32::MAX`) could overflow the multiplication/addition, wrap `expected_len` down to a small value, sail past the `data.len() < expected_len` guard, and then panic on an out-of-bounds slice read (or panic immediately on the overflow in a debug build). The length computation now uses `checked_mul`/`checked_add` throughout and returns `SdfError::InvalidData` on overflow instead. Regression test: `from_bytes_rejects_overflowing_header_without_panicking`.
- **`cargo deny check bans` passes again: the banned `miniz_oxide` is gone from the default dependency graph** — `oxitext-sdf` depended on the `png` crate unconditionally (for `SdfAtlas::export_png` / `MsdfAtlas::export_png`), and `png` pulls `flate2` → `miniz_oxide`, both of which `deny.toml` bans in favour of the COOLJAPAN `oxiarc-*` stack. Rather than hiding the two exporters behind an off-by-default feature, PNG *writing* is now done by a new in-tree encoder (`oxitext-core::png_encode`) built on `oxiarc-deflate`/`oxiarc-core`, so `export_png` and `RenderResult::to_png` keep working in a plain `cargo build` and the `png` crate is no longer reachable from any default build. Output was verified byte-identical after a round trip through an independent inflate (Python `zlib`) and accepted by libpng (`file`) and macOS ImageIO (`sips`). The only remaining `png` user is `oxitext-raster`'s PNG-compressed CBDT/sbix *decoder*, which stays behind the off-by-default `png-bitmap` (`oxitext/color-bitmap-fonts`) feature.
- **`oxitext-raster`: COLR colour glyphs no longer rasterize to a fully transparent bitmap** — the COLRv0/COLRv1 painter drew every layer through `fontdue::Font::rasterize_indexed`, but `fontdue::Font` only materialises the glyphs reachable from the font's `cmap` (plus `GSUB` when `load_substitutions` is on). COLR *layer* glyphs are deliberately mapped from no codepoint, so fontdue returned a `0x0` bitmap for each of them and every layer contributed nothing: `render_colr_v0`/`render_colr_v1` returned `Some(bitmap)` with all 4096 pixels at alpha 0 for a 64 px emoji. Verified against `twemoji_smiley-glyf_colr_1.ttf`, `noto_handwriting-glyf_colr_1.ttf` and the full 4.6 MB Noto COLRv1 emoji build — all previously produced **0.0 % coverage**, and all now render (the Twemoji smileys at 88 % coverage with ~50 distinct colours). Outlines are now rasterized directly from `ttf-parser` outlines (so `glyf`, `CFF` and `CFF2` all work) by a new internal anti-aliased scanline rasterizer, with the same em-scale and baseline placement the fontdue path used, so single-layer COLRv0 output is unchanged to within a mean alpha difference of well under 12/255 (asserted in `tests/colr_v0_regression.rs`).
- **`oxitext-raster`: COLRv1 `PaintTransform`, `PaintScale*`, `PaintRotate*`, `PaintSkew*` and `PaintTranslate` are applied** — `push_transform`/`pop_transform` were empty stubs, so a `PaintTransform` around a layer drew that layer untransformed (all repeated components landed on top of each other) and gradient control points were interpreted in the wrong coordinate space. The painter now keeps a transform stack that is applied both while flattening outlines and, through its inverse, when sampling gradients.
- **`oxitext-raster`: COLRv1 `PaintGlyph` clip regions and `ClipList` clip boxes are honoured** — `push_clip`, `push_clip_box` and `pop_clip` were empty stubs and the pending glyph id was consumed by whichever `paint` happened to come next, so a `PaintGlyph` wrapping a nested `PaintColrLayers`/`PaintComposite` lost its clip entirely. Both now rasterize into a coverage mask that is intersected onto a clip stack.
- **`oxitext-raster`: COLRv1 `PaintComposite` implements all 28 composite modes** — `push_layer`/`pop_layer` were empty stubs, which collapsed every `PaintComposite` to plain source-over (the COLRv1 conformance font's 60 composite-mode glyphs produced only 7 distinct bitmaps between them; they now produce 58). Layers render into their own premultiplied `f32` targets and are combined with the thirteen Porter-Duff operators, the eleven separable CSS blend modes (`Screen`, `Overlay`, `Darken`, `Lighten`, `ColorDodge`, `ColorBurn`, `HardLight`, `SoftLight`, `Difference`, `Exclusion`, `Multiply`) and the four non-separable ones (`Hue`, `Saturation`, `Color`, `Luminosity`).
- **`oxitext-raster`: COLRv1 sweep-gradient angles were off by a factor of two** — `startAngle`/`endAngle` are F2DOT14 values counting **180 degrees per 1.0** (half turns), but they were used directly as turns, so every sweep covered half the intended arc.
- **`oxitext-raster`: COLRv1 linear gradients honour the `p2` rotation point** — `p2` was ignored and the colour line was taken as `p0 -> p1`. Per the specification the line is `p0 -> p3`, where `p3` is the projection of `p1` onto the line through `p0` perpendicular to `p0 -> p2`; fonts that emit a non-perpendicular `p2` were rendered with an unsheared gradient.
- **`oxitext-raster`: COLRv1 radial gradients solve the real two-circle cone** — the previous code approximated `t` as `dist(pixel, c0) / dist(c1, c0)`, which is only correct for concentric circles with `r0 == 0`; `r0`, `r1` and offset centres were all ignored. The two-point conical quadratic is now solved per pixel, and pixels no circle of the family reaches are left unpainted instead of being filled with the nearest stop.
- **`oxitext-raster`: an out-of-range CPAL palette index returns `None`** — every palette lookup failed silently and the caller received a blank bitmap.
- **`oxitext`: the render pipeline used the COLRv0 entry point for COLRv1 fonts** — `rasterize_single` called `render_colr_v0` for both `ColorGlyphType::ColrV0` and `ColorGlyphType::ColrV1`, and the old `ColrV0Painter` dropped every non-`Solid` paint, so gradient-based emoji lost all their layers on top of the fontdue problem above. It now calls `render_colr_v1`, which drives the full paint graph for both table versions.
- **`oxitext-raster`: thread-local font cache no longer deep-copies the parsed face on every glyph** — `tl_cache::get_or_parse_fontdue` ended in `LruCache::get(&key).cloned()`, which cloned the whole `fontdue::Font` (every parsed glyph outline in the face) on *each* call instead of handing back the cached instance. `FontdueRaster::rasterize` consults this cache first, so every single glyph rasterization paid a full face copy: measured **302 ms/glyph** with the 23 MB Arial Unicode face and **67 ms/glyph** with Noto Sans JP (4.5 MB), making a 30-glyph Japanese subtitle cue take ~2 s where the swash backend took well under a millisecond. The cache now stores `Arc<fontdue::Font>` and clones the refcount, not the glyph tables: the same 30-glyph CJK cue at 64 px rasterizes in **151 µs (5.0 µs/glyph)**, a ~13,000x improvement, with bitmaps and metrics byte-identical to before (verified over 28 glyphs × 3 sizes for both a TrueType and a CFF/OTF CJK face).

### Changed

- **`oxitext`: the `png-output` feature no longer adds a third-party PNG dependency** — it now maps to `oxitext-core/png-encode` instead of `dep:png`. The feature name, `RenderResult::to_png`'s signature and its output format are unchanged; only the error text on an encoder failure differs (`png encode: …` rather than `png write: …`).
- **`oxitext-sdf` depends on `oxitext-core` with `features = ["png-encode"]`** — replacing its unconditional `png` dependency. `SdfAtlas::export_png` and `MsdfAtlas::export_png` keep their signatures and stay available in the default build; both now also report a texture-length mismatch through `SdfError::Io` instead of relying on the `png` crate's own length check.
- **`oxitext-raster`: `render_colr_v0` and `render_colr_v1` share one paint interpreter** — ttf-parser dispatches on the COLR table version internally, so both entry points now produce identical output for a given font and glyph. `render_colr_v0` is kept for API compatibility; new code should prefer `render_colr_v1`. Both also return `None` (rather than a zero-sized bitmap) when `width` or `height` is zero.
- **`oxitext-raster`: `ColorGlyphBitmap::rgba` is documented as straight (non-premultiplied) RGBA** — unchanged in practice, but the painter now composites internally in premultiplied `f32` and un-premultiplies once at the end, so intermediate layers no longer accumulate 8-bit rounding error.
- **`oxitext-raster`: `get_or_parse_fontdue` returns `Option<Arc<fontdue::Font>>`** — previously `Option<fontdue::Font>`. `Arc<fontdue::Font>` derefs to `fontdue::Font`, so callers that immediately rasterize (`font.rasterize_indexed(gid, px)`) need no change; only code that requires an owned `fontdue::Font` must clone through the handle. Returning an owned font is what forced the per-glyph deep copy, so the shared handle is now the only shape this API can have.
- **`oxitext-raster`: `FontdueRasterizer::raster` consults the thread-local font cache before its own `Mutex`** — its `Mutex<LruCache>` is keyed on the `Arc` pointer of the font bytes and lives *in the instance*, so every new `FontdueRasterizer` paid a full `fontdue::Font::from_bytes` on its first glyph, and every later glyph paid a lock. Since `oxitext`'s parallel render path builds one rasterizer per thread, that was a whole face parse per thread per render. `raster` now tries `tl_cache::get_or_parse_fontdue` first and only falls back to the locked LRU for bytes the thread-local cache refuses — which is exactly the set fontdue cannot parse, so the error behaviour is unchanged. A second rasterizer's first glyph on an already-warm thread went from a full parse to **2.4 µs against a 6.8 ms parse (~2,800x)** in release (37 µs against 46.8 ms in debug) with byte-identical bitmaps and metrics, asserted over 12 glyphs × 4 sizes in `tests/font_cache_parity.rs`.
- **`oxitext`: the pipeline's COLR path uses `render_colr_cached`** — `rasterize_single` already holds the font bytes as an `Arc<[u8]>`, so the colour branch now goes through the memoized entry point and a repeated glyph costs a refcount bump instead of a paint-graph walk. Output is unchanged.
- **oxifont ecosystem updated 0.2.0 → 0.2.1** — all six oxifont workspace dependencies (`oxifont`, `oxifont-core`, `oxifont-parser`, `oxifont-bundled`, `oxifont-subset`, `oxifont-adapter-native`) updated to 0.2.1, tracking the oxifont 0.2.1 release, which fixes a WOFF2 `Read255UShort` decoding bug and a WOFF2 glyf-reconstruction allocation DoS, and updates its own `oxiarc-brotli` dependency to 0.4.0 (this also resolves the duplicate `oxiarc-brotli`/`oxiarc-core`/`oxiarc-deflate` versions previously pulled transitively through `oxifont-webfont`).
- **Routine dependency bumps** — `unicode-segmentation` 1.13.2 → 1.13.3, `icu_collator` 2.2.0 → 2.2.1, `wide` 1.4.0 → 1.5.0, `smallvec` 1.15.1 → 1.15.2.

### Added

- **`oxitext-core`: `png_encode` module (feature `png-encode`, off by default)** — a self-contained 8-bit PNG writer on top of `oxiarc-deflate` (zlib stream) and `oxiarc-core` (CRC-32), exposing `encode_png`, `PngColorType` (`Grayscale8` / `GrayscaleAlpha8` / `Rgb8` / `Rgba8`) and `PngEncodeError`. Non-interlaced, adaptive per-scanline filtering with the specification's minimum-sum-of-absolute-differences heuristic over all five filter types, `IDAT` split into ≤ 1 MiB chunks, deflate level 6 to match what `flate2`/`png` produced. 11 unit tests cover every colour type, 1×1 and single-row images, chunk CRCs, buffer-length and dimension validation, and the `Paeth`/`Average` predictors against the specification's reference values; each round trip is re-decoded (inflate + unfilter) and compared pixel for pixel.
- **`oxitext-raster`: `render_colr_with_palette`** — renders a COLR glyph against a specific CPAL palette instead of palette 0, for fonts that ship alternate (e.g. dark-background) palettes. Re-exported from the crate root.
- **`oxitext-raster`: `render_colr_glyph_sized` + `ColorGlyphImage`** — a COLR entry point for laying colour glyphs out next to shaped text, rather than into a fixed preview square. The existing `render_colr_v0`/`render_colr_v1`/`render_colr_with_palette`/`render_color_glyph` all scale one em to the caller's `height` and pin the baseline at `height * 4 / 5`, so the visible window is `[0, width/height]` em horizontally and `[-0.2, 0.8]` em vertically — which **clips real emoji**: Noto's COLRv1 build paints out to 1.16 em right of the pen and up to 0.91 em above the baseline (its `ClipList` boxes span x `[-0.125, 1.375]` em and y `[-0.344, 1.0]` em), so U+1F600 at 81 px lost its right edge and its top row. The new function takes the em size in pixels, derives the bitmap from the glyph's own paint box (COLR `ClipList` entry → base-glyph outline bbox → a generous ±0.25 em box around advance/ascender/descender), trims the result to its ink, and returns `ColorGlyphImage { width, height, bearing_x, bearing_y, rgba }` using the same bearing convention as `SwashRaster`'s `RasterOutput` (left edge from the pen, top edge from the baseline, positive upwards). Bitmap edges are capped at 4096 px so a malformed `ClipList` cannot ask for gigabytes, and `None` is returned for a non-positive or non-finite em size, a glyph with no COLR record, an out-of-range palette, a degenerate box, or a glyph that paints nothing. Measured against the vendored 4.99 MB Noto COLRv1 build at 80.6 px: 87x83 px and 592 distinct opaque colours for 😀 in 0.26 ms, 84x89 px for 🎬 in 1.6 ms (`Face::parse` itself is 151 ns, so the cost is all paint-graph work). Four integration tests in `tests/colr_color_glyphs.rs` cover ink-tightness, scaling with the em, refusal of non-colour glyphs, and — against `OXITEXT_TEST_COLR_FONT` — that nothing is clipped outside the font's own clip boxes.
- **COLRv1 test fixtures** — `tests/fixtures/twemoji_smiley-glyf_colr_1.ttf` (7.4 KB, 15 real Twemoji smileys), `tests/fixtures/noto_handwriting-glyf_colr_1.ttf` (5.1 KB, the Noto Emoji ✍️ with linear and radial gradients) and `tests/fixtures/test_glyphs-glyf_colr_1.ttf` (21.6 KB, 201 glyphs covering every paint format, extend mode, transform and composite mode). All three come from [`googlefonts/color-fonts`](https://github.com/googlefonts/color-fonts) under Apache-2.0; `tests/fixtures/README.md` records source, licence, byte size, SHA-256 and the regeneration command for each.
- **`oxitext-raster`: `tests/colr_color_glyphs.rs`** — 18 COLRv1 tests. A real emoji at 64 px must cover more than 5 % of the em box with at least two distinct opaque colours; every smiley in the fixture must render; the gradient emoji must produce a colour ramp; the whole 201-glyph paint-format matrix must render (only the two deliberate `PaintColrGlyph` recursion cycles may be blank); composite modes must produce distinct bitmaps; `PaintTransform` must place two translated copies on opposite sides of the glyph; degenerate sizes, missing glyphs and bad palettes must not panic; and a caption's worth of emoji must stay inside a per-glyph time budget. `layer_glyphs_are_unreachable_through_fontdue` asserts the root cause itself, so the defect cannot silently return. Set `OXITEXT_TEST_COLR_FONT` to a full Noto COLRv1 build to additionally sweep 200 sampled glyphs from it.
- **`oxitext-raster`: `tests/colr_v0_regression.rs`** — 8 COLR **v0** tests built on a synthesised fixture: the bundled Noto Sans Regular is re-packed with hand-written `COLR` v0 and `CPAL` v0 tables, so layer glyphs, palette entries and paint order are all chosen by the test. One layer is deliberately a glyph fontdue refuses to rasterize, which reproduces the original defect inside a v0 font (that layer was missing before the fix). Also pins paint order, determinism across sizes, the `0xFFFF` foreground-colour palette index, and a mean-alpha-difference bound against a fontdue reference bitmap so the rasterizer swap provably did not move or reshape glyphs.
- **`oxitext`: `tests/color_emoji.rs` pipeline coverage** — `Pipeline::render` on a COLRv1 emoji font must emit `RenderOutput::Color` with real, multi-coloured pixels, `composite_to_rgba` must leave saturated emoji colours on the canvas, and a gradient emoji must survive the whole pipeline.
- **`oxitext-raster`: `render_colr_glyph_sized_cached` + `render_colr_cached` + the `colr_cache` module** — a thread-local LRU memo for COLR paint-graph rasterization, because both entry points are pure functions of `(font bytes, glyph id, size, palette)` and a caption renderer asks for the same emoji at the same size on every frame. Painting one costs **37–159 µs in release and 0.42–1.97 ms in debug** on the vendored COLRv1 fixtures at 64 px (0.26–1.6 ms in release on the full 4.6 MB Noto COLRv1 build); a warm lookup costs **13–15 ns in release and ~150 ns in debug** — a **~2,600–4,800x** saving — and returns the cached `Arc` without copying a pixel. `clear_colr_cache` drops the memo and `colr_cache_stats` reports per-thread hits/misses/entries/bytes.

  The cached entry points take the caller's `Arc<[u8]>` rather than a `&[u8]`, and the cache **retains that handle** for as long as the entry lives. This is a deliberate correctness choice: a bounded content hash (the first-64-bytes key `tl_cache` uses) is not an identity — two re-packed variants of one font collide on any fixed sample, and unlike a wrong *parse* a collision here would hand back an unrelated *picture* — while hashing a whole 4.6 MB font costs more than the render it would save, and a bare pointer is recycled by the allocator as soon as the caller's buffer is freed. Retaining the handle makes the address sound, because a resident entry keeps its font allocation alive. Distinct handles over equal bytes therefore *miss* rather than alias, which is the safe direction. `render_colr_glyph_sized`, `render_colr_v0`, `render_colr_v1`, `render_colr_with_palette` and `render_color_glyph` are unchanged and still always paint.

  Each cache is bounded by entry count (256) and by total pixel bytes (8 MiB), and a single result above 2 MiB is returned uncached so one 1200 px emoji cannot evict a working set.
- **`oxitext-raster`: `tests/colr_cache.rs`** — 15 tests for the COLR memo. Cached output is asserted byte-identical to the uncached entry points' over 3 fixtures × 6 glyphs × 4 em sizes (and 3 bitmap shapes for the fixed-size path); `Arc::ptr_eq` and the hit counter prove the cache path engages and that `clear_colr_cache` really drops entries and releases the retained font handles; glyph id, em size, palette, bitmap dimensions and font handle are each shown to separate keys; the entry and byte bounds are shown to hold under 360 renders; an oversized result is shown to be returned but not stored; caches are shown to be per-thread yet equivalent; and two budget tests pin the cost — a warm lookup must be 20x cheaper than a `fontdue::Font::from_bytes` of the same face (measured 2,600x in release, 5,000x in debug), and 30 distinct colour glyphs must render inside 30 ms cold and 2 ms warm in release builds (measured 1.9 ms and 0.4 µs). Set `OXITEXT_TEST_COLR_FONT` to run the same warm/cold comparison against a full Noto COLRv1 build.
- **`oxitext-raster`: `tests/font_cache_parity.rs`** — regression tests for the thread-local font cache. Bitmaps and metrics from the cached path are asserted byte-identical to those from a privately parsed `fontdue::Font` *and* to a `clone()` of it (the pre-fix behaviour), over ASCII, CJK, the U+FB01 `ﬁ` ligature and whitespace at 12/16/32/64 px; output is checked to be stable across repeated calls and across threads; `get_or_parse_fontdue` is checked to return shared `Arc` handles; and a 30-glyph cue must rasterize inside a 30 ms budget in release builds. The same parity, error-reporting and 30-glyph budget checks are repeated through `FontdueRasterizer`, plus a guard that a freshly constructed rasterizer's first glyph costs at most a tenth of a face parse. Point `OXITEXT_TEST_CJK_FONT` at a CJK face to run the whole file against real CJK outlines.
- **`oxitext-layout`: `examples/word_aware_layout.rs`** — walks the primary `LayoutEngine` flow (hand-built `ShapedRun`/`ShapedGlyph` values → UAX #14 word-aware wrapping → per-line/per-paragraph metrics) and shows the hand-off to an SDF atlas via `LayoutResult::unique_glyphs_for_atlas`.
- **`oxitext-sdf`: `examples/glyph_to_sdf_atlas.rs`** — demonstrates both the runtime path (`glyph_to_sdf_tile_analytic` → `SdfAtlas` pack → `to_bytes`/`from_bytes` round trip) and the build-time path (`generate_ascii_atlas` for embedding a ready-made atlas from a `build.rs`).

## [0.2.0] - 2026-06-23

### Changed

- **oxifont ecosystem updated 0.1.x → 0.2.0** — all six oxifont workspace dependencies (`oxifont`, `oxifont-core`, `oxifont-parser`, `oxifont-bundled`, `oxifont-subset`, `oxifont-adapter-native`) updated to 0.2.0, tracking the oxifont 0.2.0 release which brings improved font parsing and adapter APIs.
- **Workspace version bump 0.1.4 → 0.2.0** — all oxitext workspace crates and internal dependency version references updated to 0.2.0.

## [0.1.3] - 2026-06-19

### Changed

- **Version bump to 0.1.3** — all workspace-internal crate version references (`oxitext-core`, `oxitext-shape`, `oxitext-layout`, `oxitext-raster`, `oxitext-sdf`, `oxitext-icu`, `oxitext`) updated from 0.1.2 to 0.1.3 throughout the root `Cargo.toml`.

## [0.1.2] - 2026-06-10

### Added

- **`oxitext-raster`: full raw CBDT bitmap format support** — `extract_cbdt_bitmap` and `render_cbdt_glyph` now decode all eight raw `RasterImageFormat` variants exposed by ttf-parser: `BitmapMono`, `BitmapMonoPacked`, `BitmapGray2`, `BitmapGray2Packed`, `BitmapGray4`, `BitmapGray4Packed`, `BitmapGray8`, and `BitmapPremulBgra32`. Each format is decoded by a dedicated unpacker function (`unpack_mono`, `unpack_gray2`, `unpack_gray4`, `unpack_gray8`, `unpack_bgra32`). Width and height are taken directly from `ttf_parser::RasterGlyphImage` fields, eliminating the previous `None` fallback for non-PNG bitmaps.
- **`oxitext-raster`: `unpack_*` public helper functions** — `unpack_mono`, `unpack_gray2`, `unpack_gray4`, `unpack_gray8`, and `unpack_bgra32` are exported for downstream use when raw CBDT pixel data needs to be decoded outside the standard extraction path.
- **`pango-sys` workspace dependency** — `pango-sys = "0.22.0"` added to workspace dependencies to support ICU/pango-based text segmentation integration in optional downstream crates.

### Changed

- **`oxitext-raster`: `extract_cbdt_bitmap` and `render_cbdt_glyph` API** — both functions now always return `Some` for any supported bitmap format rather than falling back to `None` for raw (non-PNG) CBDT entries. All code paths now use `ttf_parser::RasterImageFormat as Rif` for ergonomic match arms.
- **Version bump to 0.1.2** — all workspace-internal crate version references updated throughout `Cargo.toml` and in-code doc examples (`oxitext/src/lib.rs`).

## [0.1.1] - 2026-06-04

### Added

- **`oxitext-icu`: `fonts` feature + `LocaleFontSelector`** — new `font_select` module (behind the `fonts` Cargo feature) provides `LocaleFontSelector`, a locale-aware font family selector backed by an `oxifont-db` `FontDatabase`. Exposes `family_for_locale`, `locale_name_for_locale`, `query_family`, `families_for_locale`, and `batch_resolve` for BCP-47 locale → font family resolution with CJK/RTL-aware CSS-Level-4 generic mapping.
- **`oxitext-shape`: `native-fallback` feature + `native_fallback` module** — new feature gate re-exports `oxifont_adapter_native::shaper_bridge` for OS-native font fallback (CoreText on macOS, DirectWrite on Windows, filesystem scan on Linux). Exposes `collect_fallback_fonts_for_text`, `collect_fonts_for_text`, `find_native_font_for_codepoint`, `load_best_native_font_for_text`, and `load_native_font_for_codepoint_with_index`.
- **`oxitext`: `font-subset` feature + `pdf_subset` module** — new module with `TextFontSubsetter`, a streaming accumulator for on-the-fly font subsetting during PDF composition. Wraps `oxifont_subset::pdf_subset::PdfFontSubsetter` with ergonomic `feed_text`, `feed_char`, `feed_gid`, `feed_gids`, `merge`, `finalize`, `finalize_into_result`, and `reset` methods; includes PDF and web presets (`for_pdf`, `for_web`). Re-exports `PdfSubsetResult`, `SubsetError`, `SubsetOptions`, and `SubsetStats`.
- **`oxifont-bundled` dev-dependency** — `oxifont-bundled` (with `bundled-noto` feature) added as a workspace-level dep and as a dev-dependency in `oxitext`, `oxitext-raster`, enabling all integration tests to use the statically embedded Noto Sans Regular.
- **`oxifont-subset` and `oxifont-adapter-native` workspace deps** — `oxifont-subset` and `oxifont-adapter-native` added to workspace dependencies to back the new `font-subset` and `native-fallback` features.

### Changed

- **Test determinism: system-font panics replaced with `oxifont-bundled` fallback** — all integration and bench tests in `oxitext`, `oxitext-raster`, and `oxitext-shape` previously panicked when system fonts were absent. They now fall back to `oxifont_bundled::NOTO_SANS_REGULAR`, ensuring reproducible CI results without hardcoded absolute font paths.
- **`oxitext-shape`: `system-fonts` dep alignment** — `oxifont` optional dep entry reformatted alongside new `oxifont-adapter-native` dep for consistency.

## [0.1.0] - 2026-06-01

### Added

#### oxitext-core
- Rich value types: `GlyphMetrics`, `GlyphCluster`, `ColorBitmap`, `RenderOutput`, `TextAlignment`, `WritingMode`, `LineSpacing`, `Decoration`/`DecorationLine`, `Rgba8`, `ParagraphStyle`, `TextRun`, `FontVerticalMetrics`
- `ShapedGlyph::{is_whitespace, unsafe_to_break}` flags and `Default` derive
- `PositionedGlyph::font_size` field
- `Hash` on `FlowDirection` and `TextAlignment`
- `TextStyle` builder methods
- `FlowDirection` enum (Horizontal/Vertical), `TextScript`, `ScriptRun`

#### oxitext-shape
- `ShapeBackend` trait with pluggable backends: `SwashShaperBackend` (primary) and `RustybuzzShaper` (opt-in `backend-rustybuzz` feature)
- LRU shape cache (`ShapeCache` / `ShapeKey`) behind `SwashShaper::with_cache`
- SIMD-accelerated shape hot-loop via `wide` f32x8
- Script detection: `requires_arabic_shaping`, `requires_indic_shaping`, `requires_mark_positioning`
- Arabic joining-form support; Devanagari conjunct shaping; Thai mark positioning
- Font fallback chain support with automatic script-based font selection
- `shape_by_script`, `shape_with_fallback`, `shape_with_features`, `shape_with_variations`
- Variational font axis support
- Vertical text shaping with `vert`/`vrt2` feature injection
- `ShapeBatch` for parallel shaping of multiple text segments

#### oxitext-layout
- UAX #9 bidi reordering via `unicode-bidi` (`BidiParagraph`, `BidiRun`, visual-order runs)
- UAX #14 line-breaking via `unicode-linebreak` (`LineBreaker`, Mandatory/Allowed breaks)
- Word-aware greedy line-breaking engine (`LayoutEngine`) driven by UAX #14 opportunities
- Left/Right/Center/Justify text alignment
- `LineMetrics`, `ParagraphMetrics`, `Line`, `LayoutResult` types
- Font-metric-driven line height with mandatory-break awareness and overflow detection
- UAX #50 vertical text orientation (CJK/Hangul/Kana upright, Latin rotated)
- Tate-chu-yoko detection (`MAX_TCY_RUN_LEN=4`, `GlyphEntry`, `TateChuYokoRun`)
- Hyphenation integration with soft-hyphen (U+00AD) detection; automatic hyphenation via `hypher` behind `hyphenation` feature
- ICU4X CLDR segmentation as drop-in replacement for `unicode-linebreak` when `icu` feature enabled

#### oxitext-raster
- `RasterBackend` trait with `FontdueRaster` (primary) and `AbGlyphRaster` (opt-in `backend-ab-glyph` feature)
- COLRv0/CPAL color glyph compositing with Porter-Duff source-over blending
- COLRv1 gradients: linear, radial, and sweep with Pad/Repeat/Reflect extend modes
- CBDT/CBLC and sbix PNG-encoded color bitmap extraction
- SVG glyph rendering via `resvg` + `tiny-skia` behind `svg-backend` feature
- SubpixelOffset (quarter-pixel) positioning and `SubpixelCacheKey`
- LCD subpixel rendering with configurable 3-tap and 5-tap FIR filter kernels and sRGB gamma
- `SwashRaster` backend with TrueType hinting for grid-fitted outlines (behind `swash-backend` feature)
- SIMD-accelerated raster hot-loop (`wide` f32x8) behind `simd` feature
- Thread-local font cache for zero-lock rasterization

#### oxitext-sdf
- Felzenszwalb-Huttenlocher 2D Euclidean Distance Transform (`compute_sdf`, `glyph_to_sdf_tile`)
- `SdfAtlas` shelf-packer with `UvRect` UV coordinates
- SIMD-accelerated 1D EDT pass
- Multi-channel SDF (MSDF): Chlumsky edge coloring + 3-channel distance fields
- Multi-channel + true SDF (MTSDF): 4-channel variant for GPU rendering
- Pseudodistance SDF (PSDF) variant
- Analytic SDF from outline segments (Bézier-accurate)
- Atlas serialization/deserialization (binary round-trip)
- PNG atlas export
- GPU descriptor types (`GpuGlyphDescriptor`, `NormalizedUvRect`) for direct shader upload
- Build helper (`generate_atlas_binary`) for compile-time atlas baking
- Growing atlas packer and MaxRects-based non-overlapping placement

#### oxitext-icu
- Unicode normalization (`Normalizer`): NFC, NFD, NFKC, NFKD
- Script detection and itemization (`CharProperties`, `TextScript`, `ScriptRun`)
- Character property queries (alphabetic, numeric, whitespace, general-category)
- `IcuSegmenter` with line/word/grapheme/sentence segmentation via ICU4X CLDR compiled data
- `IcuCollator` with Unicode Collation Algorithm, locale-aware string comparison

#### oxitext (facade)
- `Pipeline` orchestrating shape → layout → rasterize with real font metrics
- `Pipeline::measure`, `shape_and_layout`, `render_to_image`, `composite_to_rgba`, `has_rtl`, `font_metrics`
- `RenderResult` extended with `lines` and `metrics`
- `prelude` module for ergonomic glob imports
- Feature matrix documented in module docs; `docs.rs` metadata with `all-features = true`
- oxifont integration for system font loading and automatic fallback chain selection

#### oxitext-bench
- Criterion benchmarks for shape / raster / pipeline on mixed-script text
- `harfbuzz-sys` dev-dependency for baseline comparison
- Purity tripwire to detect FFI escape at benchmark time

### Workspace
- Workspace version `0.1.0`, edition 2021, MSRV 1.89
- Pure Rust default features — no C/C++/Fortran in default dependency tree
- `deny.toml` with cargo-deny license and security policy
- FFI audit Dockerfile for CI-level purity verification
- End-to-end conformance tests in `tests/`

[0.2.1]: https://github.com/cool-japan/oxitext/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/cool-japan/oxitext/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/cool-japan/oxitext/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/cool-japan/oxitext/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/cool-japan/oxitext/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cool-japan/oxitext/releases/tag/v0.1.1
[0.1.0]: https://github.com/cool-japan/oxitext/releases/tag/v0.1.0
