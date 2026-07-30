# OxiText Project TODO

## Status — v0.2.1 (2026-07-30)

All milestones M0–M7 complete. 772 tests passing (nextest --all-features), zero warnings, pure Rust default features, MSRV 1.89.

Pure Rust text rendering pipeline: shape, bidi-reorder, line-break, layout, rasterize. 7 crates in workspace. ~24,600 Rust SLOC across 96 source files. Covers LTR/RTL text shaping (swash + rustybuzz backends), UAX #9 bidi analysis, UAX #14 line-breaking (now driving word-aware wrapping in the layout engine), vertical text orientation (UAX #50), tate-chu-yoko detection, fontdue/ab_glyph rasterization with subpixel positioning, COLRv0/COLRv1 color glyph compositing (paint transforms, clips, all 28 composite modes, cached), SDF/MSDF/MTSDF atlas generation, ICU4X CLDR segmentation/collation, Unicode normalization (NFC/NFD/NFKC/NFKD), and script-itemization/character-property queries.

### M6 progress (in this slice)
- [x] oxitext-core: rich value types — `GlyphMetrics`, `GlyphCluster`, `ColorBitmap`, `RenderOutput`, `TextAlignment`, `WritingMode`, `LineSpacing`, `Decoration`/`DecorationLine`, `Rgba8`, `ParagraphStyle`, `TextRun`, `FontVerticalMetrics`; `ShapedGlyph::{is_whitespace, unsafe_to_break}` flags + `Default`; `PositionedGlyph::font_size`; `Hash` on `FlowDirection`/`TextAlignment`; `TextStyle` builders.
- [x] oxitext-layout: word-aware greedy line-breaking engine (`LayoutEngine`) driven by UAX #14 opportunities; Left/Right/Center/Justify alignment; `LineMetrics`/`ParagraphMetrics`/`Line`/`LayoutResult`; font-metric-driven line height (mandatory-break aware, overflow detection).
- [x] oxitext-icu: Unicode normalization (`Normalizer`, NFC/NFD/NFKC/NFKD); script detection + itemization (`CharProperties`, `TextScript`, `ScriptRun`); character property queries (alphabetic/numeric/whitespace/general-category); `IcuSegmenter::segments`.
- [x] oxitext facade: `Pipeline` now uses `LayoutEngine` + real font metrics (via oxifont `ParsedFace::metrics`); `measure`, `shape_and_layout`, `render_to_image`, `composite_to_rgba`, `has_rtl`, `font_metrics`; `RenderResult` extended with `lines`/`metrics`; `prelude` module.

## Milestone Summary

### M0 (Complete)
- [x] Workspace skeleton, deny.toml, ffi-audit, conformance scaffold

### M1 (Complete)
- [x] oxitext-core: ShapedGlyph, ShapedRun, PositionedGlyph, Bitmap, LayoutConstraints, TextStyle, OxiTextError
- [x] oxitext-shape: SwashShaper for LTR Latin shaping via swash ShapeContext
- [x] oxitext-layout: SimpleLayouter with cursor-advance word-wrapping
- [x] oxitext-raster: FontdueRasterizer with Arc-keyed font cache
- [x] oxitext facade: Pipeline combining shape+layout+raster, from_bytes constructor

### M2 (Complete)
- [x] oxitext-layout/bidi: UAX #9 via unicode-bidi (BidiParagraph, BidiRun, visual-order runs)
- [x] oxitext-layout/linebreak: UAX #14 via unicode-linebreak (LineBreaker, Mandatory/Allowed breaks)
- [x] oxitext-layout/vertical: UAX #50 upright/rotated classification (CJK/Hangul/Kana upright, Latin rotated)
- [x] oxitext-core: FlowDirection enum (Horizontal/Vertical), TextStyle with flow direction

### M3 (Complete)
- [x] oxitext-shape/backend: ShapeBackend trait, SwashShaperBackend, RustybuzzShaper (feature-gated)
- [x] oxitext-raster/backend: RasterBackend trait, FontdueRaster, AbGlyphRaster (feature-gated)
- [x] oxitext-raster/color: COLRv0/CPAL compositing, Porter-Duff source-over, ColorGlyphBitmap
- [x] oxitext-raster/subpixel: SubpixelOffset (quarter-pixel), SubpixelCacheKey, rasterize_with_offset
- [x] oxitext-sdf: Felzenszwalb-Huttenlocher 2D EDT, compute_sdf, SdfAtlas shelf-packer, UvRect, glyph_to_sdf_tile
- [x] oxitext-icu/segment: IcuSegmenter (line/word/grapheme/sentence via ICU4X CLDR compiled data)
- [x] oxitext-icu/collate: IcuCollator (Unicode Collation Algorithm, locale-aware compare)

### M4 (Complete)
- [x] oxitext-layout/tate_chu_yoko: CSS text-combine-upright detection, MAX_TCY_RUN_LEN=4, GlyphEntry/TateChuYokoRun

### M5 (Complete)
- [x] Slice 5a: SIMD-accelerated raster hot-loop (wide f32x8) + simd feature flag in oxitext-raster
- [x] Slice 5a: LRU shape cache (ShapeCache / ShapeKey) in oxitext-shape with SwashShaper::with_cache
- [x] Slice 5a: MSRV bump to 1.89; add wide + lru to workspace.dependencies
- [x] Slice 5b: oxitext-bench crate with criterion benchmarks (shape / raster / pipeline); harfbuzz-sys dev-dep for comparison; purity tripwire clean

### M6 (Planned)
- [x] Complex script shaping: Arabic initial/medial/final forms, Devanagari conjuncts, Thai mark positioning — `requires_arabic_shaping`/`requires_indic_shaping`/`requires_mark_positioning` in `oxitext-shape/src/script_detect.rs`; swash applies GSUB/GPOS transparently
- [x] Font fallback chains with automatic script-based font selection
- [x] Multi-channel SDF (MSDF) for sharper GPU text at small sizes — Chlumsky edge coloring + 3-channel distance fields in `oxitext-sdf/src/msdf.rs`
- [x] LCD subpixel rendering with configurable filter kernels (3-tap, 5-tap) — FIR filter + sRGB gamma in `oxitext-raster/src/lcd.rs`
- [x] TrueType hinting for grid-fitted outlines — `SwashRaster` backend in `oxitext-raster/src/swash_backend.rs` (behind `swash-backend` feature) with `hint=true` via `swash::scale::ScaleContext`

### M7 (Planned)
- [x] Rich text layout: inline images, subscript/superscript, ruby annotations
- [x] Hyphenation integration with soft-hyphen insertion — `oxitext-layout/src/hyphenation.rs` with soft-hyphen (U+00AD) detection; automatic hyphenation via `hypher` behind `hyphenation` feature
- [x] Text decorations: underline/overline/strikethrough with style/color
- [x] COLRv1 gradients and advanced composite modes — linear/radial/sweep gradients with Pad/Repeat/Reflect in `oxitext-raster/src/color.rs` (~790 SLOC)
- [x] CBDT/CBLC/sbix/SVG color glyph rendering (CBDT PNG-encoded bitmaps fully implemented; sbix via same extraction path; SVG implemented via resvg + tiny-skia in `crates/oxitext-raster/src/svg_backend.rs` behind `svg-backend` feature)

## Cross-Crate Tasks
- [x] Wire bidi reordering into Pipeline::render() — bidi-itemized shaping with per-run direction hints, cluster rebasing, and UAX #9 L2 visual reorder via the layout engine
- [x] Connect LineBreaker to layout engine for UAX #14-aware word wrapping (new `LayoutEngine`, used by `Pipeline::render`)
- [x] Use IcuSegmenter(Line) as drop-in replacement for unicode-linebreak when `icu` feature enabled (`layout_cldr` + `layout_with_break_points` in oxitext-layout)
- [x] Feed oxifont-db font selection into Pipeline for automatic system font loading
- [x] Provide text measurement API (Pipeline::measure) for GUI framework integration
- [x] Feed font metrics (ascender/descender/line-gap) into the layout engine for accurate line height (via oxifont `ParsedFace::metrics`)
- [x] End-to-end benchmark: shape -> layout -> rasterize 10K characters of mixed-script text
- [x] CI testing on macOS, Linux, and Windows — not needed; local `cargo nextest run --all-features` on macOS is the policy-compliant equivalent (COOLJAPAN CI/GitHub policy: only pypi-publish.yml / npm-publish.yml workflows allowed)
- [x] Document all feature flag combinations on docs.rs — comprehensive feature matrix in facade lib.rs module docs; `[package.metadata.docs.rs]` added with `all-features = true`

## Per-Subcrate TODOs
See individual TODO.md files in each subcrate directory:
- `crates/oxitext-core/TODO.md`
- `crates/oxitext-shape/TODO.md`
- `crates/oxitext-layout/TODO.md`
- `crates/oxitext-raster/TODO.md`
- `crates/oxitext-sdf/TODO.md`
- `crates/oxitext-icu/TODO.md`
- `crates/oxitext/TODO.md`



---

<!-- production-readiness-backlog 2026-07-16 -->
## Production-Readiness Backlog — 2026-07-16

_Consolidated from static audit + Opus adversarial bug-hunt (48 verified defects across noffi) + baseline nextest/clippy + design investigation. See `../NOFFI_PRODUCTION_BACKLOG.md` for the full cross-project list and severity/model legend. Not implemented; no commits._

**Confirmed bugs — Opus-verified:**
- [x] **A · high** `oxitext-layout/src/engine/types.rs:1200` — tab-stop handling passes line-local glyph index `(gi-gs)` to helpers that count from the global run start (ignore line_glyph_start) → every line after the first reads the wrong source glyph. R2/N0 — fixed in 0.2.1: both call sites now pass the absolute glyph index; regression test `layout_with_options_tab_stops_resolve_correct_glyph_on_second_line`.
- [x] **S · med** `oxitext-sdf/src/atlas.rs:1142` — `from_bytes` `expected_len = OFFSET + num_entries*ENTRY_SIZE + texture_len` from untrusted header can overflow usize → wraps small, bypasses guard → OOB slice panic. R2/N0 — fixed in 0.2.1: length computation uses `checked_mul`/`checked_add`, returns `SdfError::InvalidData` on overflow; regression test `from_bytes_rejects_overflowing_header_without_panicking`.
- [x] **B · L2** otherwise baseline GREEN (604 pass); add examples if thin. — added `oxitext-layout/examples/word_aware_layout.rs` and `oxitext-sdf/examples/glyph_to_sdf_atlas.rs` in 0.2.1; baseline now 772 pass (all-features).
