# oxitext-layout TODO

_Reviewed for the 0.2.1 release (2026-07-30)._

## Status
Text layout engine covering bidi reordering (UAX #9, `bidi`/`reorder`), line-breaking (UAX #14 greedy + Knuth-Plass optimal, `linebreak`/`knuth_plass`), a word-aware `LayoutEngine` (`engine::{types,functions}`) with alignment/justification, truncation, tab stops, hanging punctuation, and incremental (dirty-range) relayout, multi-style baseline-aligned rich text (`styled`), vertical CJK flow with `vmtx`-driven advances (UAX #50 subset, `vertical`), tate-chu-yoko run detection (`tate_chu_yoko`), ruby/furigana positioning (`ruby`), and hyphenation (`hyphenation`) — alongside the original M1 cursor-advance `SimpleLayouter`. 14 Rust files, ~4.4K lines under `src/` (tokei). All items below are implemented; see `examples/word_aware_layout.rs` for the primary `LayoutEngine` flow end-to-end.

## Core Implementation
- [x] Implement Knuth-Plass optimal line-breaking algorithm: compute paragraph-level optimal break points minimizing total badness with demerits for consecutive hyphenation (~200 SLOC)
- [x] Implement greedy line-breaking with UAX #14 integration: `LayoutEngine` consumes break opportunities from `LineBreaker`, wraps at the last allowed break before overflow (mandatory-break aware, hard-break fallback with overflow flag)
- [x] Add text alignment: Left/Right/Center/Justify with adjustable inter-word spacing for justified text (trailing-whitespace-trimmed line widths; last line of paragraph left-aligned)
- [x] Add bidirectional layout: reorder shaped runs according to `BidiParagraph` visual order within each line (~80 SLOC)
- [x] Add RTL text positioning: mirror cursor direction for RTL runs, handle mixed-direction lines (~50 SLOC)
- [x] Implement vertical text layout: top-to-bottom cursor advance, rotate non-upright glyphs 90deg CW, apply vmtx advances (~100 SLOC)
- [x] Add paragraph layout: handle multiple paragraphs with inter-paragraph spacing, first-line indent (~40 SLOC)
  - **Goal:** `LayoutEngine::layout_paragraphs(paragraphs:&[&str], runs_per_para, max_width, para_spacing, metrics)->LayoutResult` — each paragraph gets independent line-breaking; cursor_y advances by `para_spacing` between paragraphs.
  - **Files:** `crates/oxitext-layout/src/engine.rs`, `crates/oxitext-layout/src/lib.rs`
  - **Tests:** two paragraphs have y-gap > line_height; first para glyphs all before second para glyphs
- [x] Add multi-style layout: accept multiple TextRuns with different fonts/sizes, compute baseline alignment per line (~100 SLOC)
- [x] Add hanging punctuation: detect CJK fullwidth punctuation at line start/end and overhang into margins (~40 SLOC)
- [x] Add hyphenation support: integrate with a hyphenation crate (hypher or similar) for soft-hyphen insertion (~50 SLOC)
- [x] Implement `LineMetrics` struct (ascent, descent, leading, baseline_y, width) per line for precise text bounds
- [x] Add `ParagraphMetrics` (total_height, total_width, line_count, overflow flag)
- [x] Implement text truncation: ellipsis insertion when text overflows constraints (~40 SLOC)
  - **Goal:** After line-breaking, if the last line exceeds `max_width`, walk glyphs backward removing until ellipsis char fits; insert ellipsis glyph; set `ParagraphMetrics.truncated=true`. `TruncationMode{max_width:f32, ellipsis:char}` (default '…' U+2026).
  - **Files:** `crates/oxitext-layout/src/engine.rs`, `crates/oxitext-layout/src/lib.rs`
  - **Tests:** "Hello world" truncated at narrow width produces "Hell…"; truncated=true in metrics; exact fit not truncated
- [x] Add tab stop support: configurable tab stop positions for tabular text layout (~25 SLOC)
  - **Goal:** `TabStops{positions:Vec<f32>, default_interval:f32}`. When `\t` glyph encountered, advance cursor to next tab stop. Default interval = 4× space advance.
  - **Files:** `crates/oxitext-layout/src/engine.rs`
  - **Tests:** glyph after \t has x ≥ first tab stop; multiple \t advances to sequential stops
  - **Fixed (0.2.1):** the `layout_with_options` tab-stop handler was passing a line-relative glyph index into `find_cluster_for_positioned_glyph`/`advance_for_glyph` (both require the *absolute* index into `shaped_runs`), so tab stops on the second and later lines resolved the wrong source character. Both call sites in `src/engine/types.rs` now pass the absolute index `gi`. Regression test: `layout_with_options_tab_stops_resolve_correct_glyph_on_second_line` in `src/engine/functions.rs`.
- [x] Improve tate_chu_yoko: extract combined advance from vmtx instead of using em_size default (~20 SLOC)
- [x] Add ruby annotation layout: position ruby text above/below base text for CJK furigana (~80 SLOC)
- [x] Handle zero-width joiners (ZWJ) and zero-width non-joiners (ZWNJ) correctly during line-breaking (~15 SLOC)
  - **Goal:** During line-breaking, ZWJ (U+200D) suppresses break opportunity between neighbors; ZWNJ (U+200C) permits break. ~15 lines in the break-opportunity loop.
  - **Files:** `crates/oxitext-layout/src/engine.rs`
  - **Tests:** "a\u{200D}b" at 1px width does not break between a and b; "a\u{200C}b" allows break
- [x] Wire `vmtx` per-glyph vertical advance into `layout_vertical` — `vmtx_advance_for_glyph(face_data, glyph_id, em_size)` shared helper in `vertical.rs`; `VerticalMetrics::for_glyph` constructor; `layout_vertical` updated to call `for_glyph` per FlatGlyph using the existing `Arc<[u8]>` font pointer; `tcy_combined_advance` refactored to delegate to the same helper; re-exported from `lib.rs`; 7 tests all pass

## API Improvements
- [x] Add `LayoutEngine` (word-aware, alignment-capable) alongside `SimpleLayouter`; trait abstraction over multiple engines still TODO
- [x] Add `LayoutResult` struct with positioned glyphs + line metrics + paragraph metrics instead of just Vec<PositionedGlyph>
- [x] Add `LayoutOptions` builder: alignment, direction, vertical_mode, hyphenation, tab_stops, truncation
  - **Goal:** `LayoutOptions{alignment, flow_direction, vertical_mode, truncation, tab_stops, paragraph_spacing}` with `LayoutOptions::builder()` fluent API. `LayoutEngine::layout_with_options(text, runs, options, metrics)->LayoutResult`.
  - **Files:** `crates/oxitext-layout/src/options.rs` (new), `crates/oxitext-layout/src/engine.rs`, `crates/oxitext-layout/src/lib.rs`
  - **Tests:** builder sets all fields; layout_with_options produces same output as layout() for default options
- [x] Make `SimpleLayouter` accept `FlowDirection` and switch between horizontal and vertical cursor advance
- [x] Return `Line` structs grouping glyphs by line for per-line rendering and hit-testing
- [x] Add `hit_test(x, y) -> Option<(line_index, glyph_index, cluster)>` for text selection and cursor positioning

## Testing
- [x] Test bidi layout: Arabic "مرحبا" + English "hello" produces visually correct glyph order
- [x] Test bidi with multiple embedding levels: LTR paragraph containing RTL text containing LTR numbers
- [x] Test linebreak integration: UAX #14 allowed breaks drive word-wrapping in `LayoutEngine` (wraps at space, not mid-word)
- [x] Test mandatory line breaks (\n) produce new lines at correct positions
- [x] Test vertical layout: CJK text produces top-to-bottom positioned glyphs with correct advances
- [x] Test tate_chu_yoko: "2024" within vertical CJK text is detected as a combined horizontal run
  - **Goal:** Test that `detect_tcy_runs("縦書き2024年")` identifies "2024" as a combined horizontal run while surrounding CJK characters are not TCY.
  - **Files:** `crates/oxitext-layout/src/tate_chu_yoko.rs` (inline test)
- [x] Test text alignment: centered/right-aligned lines offset correctly within the column
- [x] Test justified last-line stays left-aligned (no expansion); internal-gap counting verified
- [x] Test Knuth-Plass produces fewer rivers than greedy algorithm on sample English paragraph
- [x] Test multi-paragraph layout with different inter-paragraph spacing
  - **Goal:** Test `layout_paragraphs` with two paragraphs produces correct y-separation equal to `para_spacing` above the first line of the second paragraph.
  - **Files:** `crates/oxitext-layout/src/engine.rs` (inline test)
- [x] Test truncation with ellipsis at max_width boundary
  - **Goal:** Test that a string truncated at exactly its natural width is NOT truncated; truncation only triggers when text exceeds max_width.
  - **Files:** `crates/oxitext-layout/src/engine.rs` (inline test)
- [x] Benchmark layout of 10K-character mixed-script text (bench_layout_10k_chars, #[ignore])

## Performance
- [x] Avoid re-allocating positioned glyphs Vec on every layout call (reuse buffer)
- [x] Pre-compute line break opportunities once and cache for re-layout (e.g., window resize)
- [x] Use incremental relayout: only re-layout lines affected by text edits
- [x] Parallelize independent line layout when lines are independent (non-justified text): two-phase placement — place glyphs at pen=0, then apply alignment offsets in parallel via rayon (WASM-guarded); Justify path remains sequential
- [x] Add `ParsedFaceCache` to eliminate per-glyph `ttf_parser::Face::parse` in `layout_vertical` — for a 500-glyph paragraph 500 full face parses become 1 parse + 500 HashMap lookups; ~50–200× speedup on vertical-metrics step (planned 2026-05-27)
  - **Goal:** `LayoutEngine::layout_vertical` constructs one `ParsedFaceCache<'a>` keyed on `Arc::as_ptr(font) as usize` before the per-glyph loop and calls `cache.vmtx_advance_cached(&fg.font, fg.g.gid, font_size)` instead of `VerticalMetrics::for_glyph(...)` per glyph; fallback chain unchanged (empty/garbage face → `em_size`); `VerticalMetrics::for_glyph` preserved as single-glyph public helper.
  - **Design:** New private `pub(crate) struct ParsedFaceCache<'a>` in `crates/oxitext-layout/src/vertical.rs`; `by_ptr: HashMap<usize, Option<(ttf_parser::Face<'a>, u16)>>`; sticky `None` for parse failures; `vmtx_advance_cached` method mirrors existing fallback; wire into `layout_vertical` at `crates/oxitext-layout/src/engine/types.rs:965-1058`.
  - **Files:** `crates/oxitext-layout/src/vertical.rs`, `crates/oxitext-layout/src/engine/types.rs`
  - **Tests:** `parsed_face_cache_returns_same_advance_as_uncached`; `parsed_face_cache_parses_each_font_once` (assert `by_ptr.len()==1` after 100 lookups on same Arc); `parsed_face_cache_handles_parse_failure_sticky`; `layout_vertical_uses_cache` (integration: result identical pre/post cache)

## Integration
- [x] Consume shaped runs with bidi reordering applied (bidi module + visual reorder in layout_impl)
- [x] Feed positioned glyphs to oxitext-raster for per-glyph rasterization
- [x] Use oxitext-icu's line segmenter as an alternative to unicode-linebreak for CLDR-compliant breaking (`layout_cldr` method behind `icu` feature flag; `layout_with_break_points` for external injection)
- [x] Coordinate with oxitext-sdf for SDF atlas generation of positioned glyph set
  - **Demonstrated (0.2.1):** `examples/word_aware_layout.rs` shows the full hand-off end-to-end — `LayoutEngine::layout` → `LayoutResult::unique_glyphs_for_atlas()` — see also `oxitext-sdf`'s `glyph_to_sdf_atlas` example for the receiving side.
- [x] Accept font metrics for accurate ascender/descender/line-gap values (`LayoutEngine::layout` takes `Option<&FontVerticalMetrics>`; the facade feeds oxifont `ParsedFace::metrics`)
