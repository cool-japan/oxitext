# oxitext (facade) TODO

## Status
Facade crate providing the end-to-end text pipeline (shape → bidi → line-break → layout → rasterize), current as of 0.2.4 (2026-08-12). Re-exports core types and the bidi/linebreak/vertical/tate-chu-yoko layout modules unconditionally; SDF atlas generation behind `sdf`, CLDR/ICU behind `icu`, PDF font subsetting behind `font-subset`, CBDT/sbix bitmap-strike decoding behind `color-bitmap-fonts`. The `Pipeline` struct (behind the default-on `pure` feature) combines `SwashShaper` + `LayoutEngine`/`SimpleLayouter` + `FontdueRasterizer` into a single `render()` call, with bidi-aware, vertical-writing, font-fallback, and COLRv0/v1 color-glyph rendering (gradients, transforms, composite paints) all wired in. ~1,100 SLOC. Every item below is implemented; 74 tests + 7 doctests pass with `--all-features`.

## Core Implementation
- [x] Add `Pipeline::render_to_image(text, style, bg, fg) -> ColorBitmap` producing a ready-to-use RGBA pixel buffer (canvas sized from paragraph metrics)
- [x] Add `Pipeline::render_paragraph(paragraphs: &[&str], style) -> RenderResult` for multi-paragraph layout (~30 SLOC)
- [x] Add `Pipeline::set_fallback_fonts(fonts: Vec<Vec<u8>>)` for font fallback chain configuration (~20 SLOC)
  - **Goal:** `Pipeline::set_fallback_fonts(fonts:Vec<Vec<u8>>)` stores fallback chain; shape_and_layout tries primary font first, then each fallback for missing glyphs (glyph_id == 0 or .notdef).
  - **Files:** `crates/oxitext/src/lib.rs`
  - **Tests:** text with glyph missing from primary but present in fallback renders with fallback GID; no fallback returns notdef
- [x] Add `Pipeline::with_backend(shaper, layouter, rasterizer)` for custom backend injection (~25 SLOC)
  - **Goal:** `Pipeline::with_backend(shaper, layouter, rasterizer)` for dependency injection of custom backends.
  - **Files:** `crates/oxitext/src/lib.rs`
  - **Tests:** custom no-op shaper produces empty glyph list; custom rasterizer called for each glyph
- [x] Add `Pipeline::render_styled(runs: &[TextRun]) -> RenderResult` for mixed-font/size/color text (~50 SLOC)
- [x] Add `Pipeline::measure(text, style) -> ParagraphMetrics` returning text dimensions without rasterizing
- [x] Add `RenderResult::composite_to_rgba(width, height, bg, fg) -> ColorBitmap` for blitting bitmaps onto a canvas
- [x] Add `RenderResult::to_png(path)` for direct PNG output (behind optional feature) — deferred (avoid transitive deps)
- [x] Add `Pipeline::shape_and_layout(text, style) -> LayoutResult` for shape+layout without rasterization
- [x] Implement bidi-aware rendering: `Pipeline::has_rtl` detects RTL; bidi-itemized shaping with per-run direction hints and UAX #9 L2 visual reorder fully wired in `shape_and_layout`
- [x] Implement vertical text rendering: use FlowDirection::Vertical in Pipeline::render with vertical layout (~25 SLOC)
- [x] Add `PipelineBuilder` for fluent construction: `Pipeline::builder().font(data).shaper(SwashShaper::new()).build()` (~30 SLOC)
  - **Goal:** `Pipeline::builder().font(data).shaper(SwashShaper::new()).layouter(engine).rasterizer(raster).build()->Result<Pipeline,_>`. Validates font data at construction time.
  - **Files:** `crates/oxitext/src/lib.rs`
  - **Tests:** builder with valid font builds; builder with invalid font data returns error; default builder uses SwashShaper+LayoutEngine+FontdueRasterizer
- [x] Add color text rendering: detect color fonts, produce RGBA bitmaps alongside greyscale (~30 SLOC); SVG-table glyphs now also supported via oxitext-raster's `svg-backend` feature
- [x] Fix `RenderResult::composite_to_rgba` to render color glyphs from `self.outputs` — current loop walks only `self.bitmaps`, silently dropping COLRv0/v1 emoji; rewrite to walk `self.outputs` by index and Porter-Duff blit RGBA color bitmaps at glyph positions (planned 2026-05-27)
  - **Goal:** `pipeline.render_to_image(text, &style)` correctly renders color emoji as RGBA; greyscale glyphs render unchanged; alpha-channel output is non-zero at emoji positions.
  - **Design:** Walk `self.glyphs.iter().enumerate()`; for each glyph check `self.outputs[i]`: `Greyscale` → existing coverage blit; `Color(rgba)` → Porter-Duff source-over the RGBA bitmap at `glyph.pos.round()` onto the canvas; `Sdf`/`Msdf` → skip with doc comment; `Lcd` → greyscale fallback. Use `oxitext_raster::scalar::porter_duff_source_over` or the SIMD variant for the blit.
  - **Files:** `crates/oxitext/src/lib.rs` (`composite_to_rgba`), `crates/oxitext/tests/color_composite.rs` (new)
  - **Tests:** synthetic color glyph composite (4×4 red bitmap, assert non-zero alpha); mixed greyscale+color; system color font smoke test (skip-on-absent)
- [x] Fix `Pipeline::shape_run_with_notdef_fallback` so a `.notdef` fallback hit no longer mis-attributes every glyph in the run to the fallback font, and no longer drops a mark glyph a fallback attaches to a base glyph (done 2026-08-12)
  - **Goal:** Every glyph a caller rasterizes via `(font_data, gid)` is paired with the font that actually produced that glyph id; a fallback that shapes a cluster into base + mark keeps both glyphs.
  - **Design:** Previously, the first `.notdef` fallback hit anywhere in a run shaped the whole text slice into a single `ShapedRun` and overwrote that run's one `font_data` field with the fallback's. Since a `ShapedRun` carries one `font_data` for its entire glyph slice, this silently re-pointed every other glyph too — glyphs the primary font had already resolved kept the primary's glyph ids but were now attributed to the fallback face, so a caller rasterizing per `(font_data, gid)` drew the primary's glyph ids out of the fallback's `glyf`/`CFF ` table (wrong letters, not wrong metrics). This was invisible whenever the two faces number glyphs alike for the affected codepoints (Noto Sans, Meiryo, MS Gothic, MS Mincho, Yu Gothic all do, for basic Latin). A second, related defect fixed at the same time: only the first non-notdef glyph a fallback produced for a cluster was kept, so a fallback that shaped a cluster into a base + mark lost the mark. Fix: `shape_run_with_notdef_fallback` now returns one `ShapedRun` per font actually used, split at font boundaries in logical order; every caller already accepted `&[ShapedRun]` (the bidi path has always produced several), so no caller signature changed.
  - **Files:** `crates/oxitext/src/lib.rs` (`Pipeline::shape_run_with_notdef_fallback`), `crates/oxitext/tests/fallback_runs.rs` (new)
  - **Tests:** 6 new regression tests in `tests/fallback_runs.rs`, using only bundled fonts (oxifont-bundled's `NOTO_SANS_REGULAR` as primary, `NOTO_SANS_MONO_REGULAR` as fallback) — no system-font dependency.

## API Improvements
- [x] Add a prelude module: `use oxitext::prelude::*` importing Pipeline, TextStyle, RenderResult, Bitmap, alignment/metrics types
- [x] Document feature flag matrix and their effects on available types
- [x] Add `Pipeline::available_features() -> &'static [&'static str]` listing which optional features are compiled in
  - **Goal:** `Pipeline::available_features()->&'static [&'static str]` listing compiled features: "sdf" if cfg(feature="sdf"), "icu" if cfg(feature="icu"), "pure" if cfg(feature="pure"), etc.
  - **Files:** `crates/oxitext/src/lib.rs`
  - **Tests:** available_features() always returns non-empty slice; "pure" present when compiled with pure feature
- [x] Re-export `ShapeBackend` and `RasterBackend` traits for custom backend registration
  - **Goal:** `pub use oxitext_shape::{ShapeBackend, SwashShaper}` and `pub use oxitext_raster::{RasterBackend, FontdueRasterizer}` in facade so users don't need direct crate deps.
  - **Files:** `crates/oxitext/src/lib.rs`
  - **Tests:** traits accessible via `oxitext::ShapeBackend` in integration test
- [x] Add `TextStyle::with_alignment(TextAlignment)` (+ `with_font_size`/`with_max_width`); alignment honoured by the layout engine
- [x] Re-export `Script`, `Tag`, `tag_from_bytes` from `oxitext_shape` behind `pure` (done 2026-08-12)
  - **Goal:** Let a caller check whether the shaper accepts a given OpenType script tag — `Script::from_opentype(tag)` round-tripped through `Script::to_opentype()` — before passing it to `ShapeRequest::script`, without shaping first or taking a direct dependency on the shaper crate.
  - **Files:** `crates/oxitext/src/lib.rs`

## Testing
- [x] Integration test: full pipeline render of "Hello" at 16px produces non-empty bitmaps; measure/shape_and_layout/render_to_image/alignment/has_rtl covered in `tests/layout_api.rs`
- [x] Integration test: bidi rendering of Arabic+English mixed text
- [x] Integration test: vertical CJK text rendering with tate_chu_yoko for embedded numbers
- [x] Integration test: render with font fallback (primary font missing a glyph, fallback font provides it)
- [x] Test Pipeline::from_bytes with invalid font data returns appropriate error
  - **Goal:** Test `Pipeline::from_bytes(b"not a font")` returns `Err(OxiTextError::InvalidFont)` or similar, not a panic.
  - **Files:** `crates/oxitext/tests/error_handling.rs` (new)
- [x] Test Pipeline::new with empty FontDatabase returns FontNotFound error
  - **Goal:** Test `Pipeline::new()` on a system with no matching fonts returns `Err(OxiTextError::FontNotFound)` or graceful error.
  - **Files:** `crates/oxitext/tests/error_handling.rs`
- [x] Test RenderResult glyphs and bitmaps arrays have matching lengths
  - **Goal:** Test that `render_result.glyphs.len() == render_result.bitmaps.len()` (or outputs.len()) after a successful render call.
  - **Files:** `crates/oxitext/tests/error_handling.rs`
- [x] Test all feature combinations compile: `--no-default-features`, `--features=sdf`, `--features=icu`, `--all-features`

## Performance
- [x] Avoid re-shaping text when only style (size/alignment) changes (cache shaped runs)
- [x] Batch rasterization: pass all glyph IDs at once instead of one-by-one
- [x] Use parallel rasterization for independent glyphs via rayon
- [x] Profile and optimize the full pipeline path for interactive text rendering (target: <5ms for 100 glyphs)

## Integration
- [x] Depend on oxifont for font discovery (`Pipeline::new`) and metric extraction (`ParsedFace::metrics` → `FontVerticalMetrics`)
- [x] Use oxitext-icu for CLDR-compliant line breaking and word segmentation when `icu` feature is enabled
- [x] Feed SDF atlas data to GPU rendering backends (wgpu, vulkan) for real-time text
- [x] Provide text measurement API for GUI framework integration (`Pipeline::measure` → `ParagraphMetrics`)
