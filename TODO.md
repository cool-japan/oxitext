# OxiText Project TODO

## Status — v0.2.3 released (2026-08-12)

All milestones M0–M7 complete. **851 tests passing** (`cargo nextest run --workspace --exclude oxitext-bench --all-features`; 16 further tests are `#[ignore]`/env-var-gated fixture sweeps not counted here; 749 pass with default features) — 845 before 0.2.3's fallback-run fix below, plus its 6 new tests — with 80 doctests, zero warnings, pure Rust default features, MSRV 1.89 (verified on a real 1.89 toolchain). See `CHANGELOG.md`'s `[0.2.3]` section for what has landed since the 0.2.2 release.

**0.2.3**: `Pipeline::shape_run_with_notdef_fallback` (the private fn behind `shape_and_layout`) now returns one `ShapedRun` per font actually used instead of one run whose `font_data` got overwritten wholesale on the first `.notdef` fallback hit — that overwrite had silently re-pointed every glyph the PRIMARY font had already resolved to the fallback face too (wrong letters, not wrong metrics, invisible whenever the two faces share glyph-id numbering for the affected codepoints). Regression suite: `crates/oxitext/tests/fallback_runs.rs` (6 tests, bundled fonts only). Also: `oxitext_shape::{Script, Tag, tag_from_bytes}` re-exported (and re-exported again from the `oxitext` facade under `pure`), so a caller can check whether the shaper accepts a script tag before passing it to `ShapeRequest::script`.

**8 crates in the workspace since 0.2.2**: `crates/oxitext-swash` is a vendored fork of `swash` 0.2.10 by Chad Brokaw carrying OxiText's fix for two Indic reordering defects (one of them upstream `dfrg/swash#93`). It is the only vendored crate and the only one without `#![forbid(unsafe_code)]`, but since the 2026-08-05 user election it otherwise obeys COOLJAPAN house style in full — the 2000-line limit (upstream's 5 491-line `text/unicode_data.rs` is a seven-submodule directory now), one crate-level clippy allow, no `.unwrap()`, and the `oxiarc-*` stack in place of upstream's `yazi`. `crates/oxitext-swash/PROVENANCE.md` is its audit trail and `CONTRIBUTING.md` carries the vendored-code rules. It roughly doubles the workspace's Rust code (22 970 SLoC over 68 files), which is a recurring tax on every future `clippy`/`fmt`/`nextest`/toolchain bump — budget it.

**KNOWN DRIFT 2026-08-06 (release-check, toolchain 1.95.0)**: the release-check's mandatory
`cargo clippy --all-targets --all-features -- -D warnings` gate found and fixed 2 new
`clippy::collapsible_match` errors in the vendored `oxitext-swash` crate (`scale/bitmap/png.rs`'s `GAMA`
arm, `shape/buffer.rs`'s `Reph`/`Pref` arms — the latter inside the file's documented reph fix), not present
when `PROVENANCE.md` was last written. Both fixes are mechanical (clippy's own suggested guard-clause
collapse, semantics-preserving, re-verified against the full test suite) — see `CHANGELOG.md`'s
`oxitext-swash` entry for detail. **Not done as part of this pass**: refreshing `PROVENANCE.md`'s "7 hunks"
count for `shape/buffer.rs` and its per-file divergence table to account for these 2 additional hunks. That
requires a full re-diff against pristine upstream `swash` 0.2.10 and is out of scope for a release-check —
flagged here as a follow-up.

Pure Rust text rendering pipeline: shape, bidi-reorder, line-break, layout, rasterize. ~24,600 Rust SLOC across 96 source files. Covers LTR/RTL text shaping (swash + rustybuzz backends), UAX #9 bidi analysis, UAX #14 line-breaking (now driving word-aware wrapping in the layout engine), vertical text orientation (UAX #50), tate-chu-yoko detection, fontdue/ab_glyph rasterization with subpixel positioning, COLRv0/COLRv1 color glyph compositing (paint transforms, clips, all 28 composite modes, cached), SDF/MSDF/MTSDF atlas generation, ICU4X CLDR segmentation/collation, Unicode normalization (NFC/NFD/NFKC/NFKD), and script-itemization/character-property queries.

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
- [x] CBDT/CBLC/sbix/SVG color glyph rendering — `oxitext_raster::render_cbdt_glyph` decodes uncompressed CBDT strike formats (mono/gray2/gray4/gray8/BGRA32) unconditionally; PNG-encoded CBDT strikes and *all* `sbix` strikes (Apple's table only stores PNG) require the off-by-default `png-bitmap` feature (`oxitext-raster/src/detect.rs::decode_png_to_bitmap` is a `#[cfg(not(feature = "png-bitmap"))]` stub returning `None` otherwise). With `png-bitmap` on, *every* still-image PNG colour type decodes — greyscale, truecolour, indexed and both alpha forms, bit depths 1–16, interlaced or not — through `oxitext-core`'s own `png_decode` module (built on `oxiarc-deflate`), so the feature is deny-clean: it adds no `png`/`flate2`/`miniz_oxide` edge. It stays off by default only so builds that never draw bitmap emoji do not compile the decoder. SVG is implemented via resvg + tiny-skia in `crates/oxitext-raster/src/svg_backend.rs` behind the off-by-default `svg-backend` feature (not deny-clean even when enabled — see that feature's Cargo.toml doc comment). `Pipeline` (the `oxitext` facade) reaches uncompressed CBDT/sbix unconditionally, PNG-encoded CBDT/sbix via the facade's `color-bitmap-fonts` feature, and SVG via the facade's `svg-glyphs` feature — all three land in `rasterize_single` (`crates/oxitext/src/lib.rs`), whose colour-format probe now uses `detect_color_glyph_type_at` (see CHANGELOG 0.2.2: the old probe reported bitmap fonts as non-colour).

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

---

<!-- swash-absorb 2026-08-05 -->
## Workstream — swash absorption into `crates/oxitext-swash` (2026-08-05)

Executed from `docs/plans/swash-absorb.md` (three-lens Opus synthesis, decisions D1–D22,
stages S0–S8). Branch `0.2.2`, base `5c82218`, toolchain 1.97.1. **Nothing committed,
staged, pushed, published or version-bumped** — the tree is left dirty for the user.

**Vendored.** `swash` 0.2.10 (crates.io 2026-07-17; upstream commit `7773843`; Chad
Brokaw; `Apache-2.0 OR MIT`) → `crates/oxitext-swash`, a published workspace member.
61 source files, **22 865 code SLoC** (`tokei`), byte-copied from the registry checkout;
`.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock`, `Cargo.toml.orig`, `.gitignore`,
`.github/`, `.typos.toml`, upstream's `README.md` and `.clippy.toml` were not vendored
(the last folded into the workspace `clippy.toml`, since a crate-local one does not
inherit `msrv = "1.89"`). Licence unified to **Apache-2.0** per the user's election of
D10's recorded override (`license.workspace = true`); `LICENSE-APACHE` and `LICENSE-MIT`
both ship verbatim, with `NOTICE`/`PROVENANCE.md` recording that OxiText redistributes
Chad Brokaw's dual-licensed work under its Apache-2.0 arm.

**The bug, in one line.** `reorder_complex`/`reorder_myanmar` build a permutation into
`order`, a scratch `Vec<usize>` owned by `ShapeContext` that `State::reset()` never
clears and `Vec::resize` only grows — so when the fill loop ends with `j < len` the tail
still holds a *previous cluster's* indices, and `glyphs[i] = buf[*j]` either duplicates a
glyph and drops the reph (defect a) or indexes out of bounds (defect b, upstream
`dfrg/swash#93`, open since 2025-04-20).

**The fix** (7 hunks, `src/shape/buffer.rs`, applied by byte-copying the proven patched
file — not retyped): four range ends corrected from a length to an exclusive index; every
emission routed through a guarded `emit!` (`index < len`, `!placed[index]`, `j < len`);
the hole closed explicitly (`if let Some(i) = reph`), then swept, then
`debug_assert_eq!(j, len)`. Also removes the crate's `anus.take().unwrap()`.

**Corpus, RED → GREEN.** `crates/oxitext-shape/tests/devanagari_reorder.rs`, 6 cases over
`tests/fixtures/NotoSansDevanagari-Regular.ttf` (244 284 B, SHA-256 `306b53ec…96cd`,
OFL-1.1, `OFL.txt` beside it — OxiText had **no** Devanagari test against a real
Devanagari font before this). Against the pristine vendored code: **5 RED / 1 GREEN**,
captured verbatim —
`स्वर्ग` → `[256, 84, 58, 58]` (want `…, 506`);
`"दिल्ली मार्ग"` → `[544, 73, 252, 83, 33, 3, 80, 31, 58, 58]` (want `…, 58, 506`);
`"सूर्य पूर्व वर्षा मार्ग"` → **panicked**;
4 of 24 corpus words panicked (`पूर्ण`, `वर्तमान`, `आदर्श`, `संघर्ष`) and those same 4 made a
reused shaper disagree with a fresh one; the Latin control passed on both sides, which is
its whole job. After the fix: **6 / 6 GREEN**. Every D8 golden re-measured against Noto
with the final patch matched the plan's expected values exactly — notably `दिल्ली`, which
carries a VPre matra, is byte-identical pre- and post-fix, so the VPre/VMPre range
correction produced no deviation to investigate. Plus **8** font-independent
permutation-invariant unit tests in `oxitext-swash`'s own `shape/buffer.rs`, one of them
driven by a deliberately pre-poisoned `order`.

**Numbers.** G2 **827 → 841** (827 unchanged + 6 corpus + 8 unit), 0 failed, 16 skipped.
G3 **55 → 72** doctests (the 17 rewritten by the `[lib] name` decision). G4 **0** warnings
at default and all-features under `-D warnings`, down from 41 in the vendored crate plus 1
pre-existing in `oxitext-raster`. All 13 upstream `.unwrap()` sites converted, gated by
`#![deny(clippy::unwrap_used, clippy::expect_used)]`; upstream's five crate-level allows
reduced to two, each carrying its rationale. **37 of 61 vendored files remain
byte-identical to upstream**, including both generated Unicode tables. G8 graph
assertions all hold: `yazi`/`zeno` gone from `oxitext-shape`'s normal graph (they were in
it before), `zeno` still reachable for `oxitext-raster --features swash-backend`,
`cargo tree -e no-dev -i swash` matches nothing. **D2 proved: `crates/oxitext-shape/Cargo.toml`
is byte-unchanged and not one `use swash::…` line moved in any oxitext-owned crate.**

**Downstream (record only — `I:\rust\oxigis` was READ-ONLY for this work).** OxiGIS
reaches swash through exactly one root, `oxitext`/`oxitext-raster` 0.2.1. Route A is to
wait for `oxitext 0.2.2` on crates.io and bump both direct deps; Cargo unifies with
`oxiui-text`'s `^0.2.1`, so `cargo tree -d | grep oxitext` stays empty and `oxiui-text`
needs no republish. A plain git dep is **wrong** (git and registry sources never unify, so
the map would draw with two shapers of different Indic behaviour); if OxiGIS cannot wait,
only a `[patch.crates-io]` covering all five crates preserves the invariant. **The canary
flip is the trap:** `crates/oxigis-ui/src/print/shape.rs`'s
`swash_still_garbles_and_panics_on_indic_a_canary_not_a_complaint` *asserts the defects
exist*, so it goes RED the day 0.2.2 enters the graph — that red is the success signal.
Invert it in the same commit as the bump (fresh `मार्ग` unchanged; reused `मार्ग`
`assert_ne!` → `assert_eq!`; corpus panics 4 → 0; `स्वर्ग` → `[738, 250, 330]`), keep the
harness verbatim, and keep the honesty log warning about Myanmar, Khmer and Thai — the
Myanmar staleness is fixed but Myanmar shaping correctness is otherwise unvalidated.
This unblocks print v1.4 item 1 (LTR complex-script itemisation) for **v1.6**; screen-side
Indic parity is a separate multi-crate feature and must not be bundled with it.

**Upstream credit.** The fix is original, but **dfrg/swash#93 "Panic While Shaping"** is
the prior public report of defect (b) — same function, same line, reported via
parley/vello_editor on `बर्नार्ड` — and is cited in `CHANGELOG.md`, `PROVENANCE.md` and the
`buffer.rs` header. Defect (a) has no upstream issue. The S4 tree state (fix only, no
conformance churn) is preserved as the upstream-offerable diff; **posting anything to
`dfrg/swash` requires explicit user approval and was not done.**

**Five deviations**, each recorded at the moment it was taken, below.

<!-- swash-absorb-deviations 2026-08-05 -->
## swash absorption — DEVIATIONS from `docs/plans/swash-absorb.md`

DEVIATION 2026-08-05 S9b: the vendored crate's `scale` feature now implies `std`
(`scale = ["std", "dep:oxiarc-deflate", "dep:zeno"]`), which upstream's did not.
Measured reason: the amended D16 directs `yazi` → `oxiarc-deflate`, and
`oxiarc-deflate 0.4.0` is std-only **by design** — it declares no `#![no_std]` and
uses `std::io` (22 sites), `std::sync`, `std::thread` — whereas `yazi` was
`no_std`-capable via its own `std` feature. So a `no_std` + `scale` build, which
upstream permitted, is no longer possible. Accepted rather than worked around: that
combination has no consumer in this workspace (`oxitext-raster`'s `swash-backend` is
a std crate), the plan's own six-combo gate still passes (the `libm,scale,render`
combo now additionally turns `std` on, which D18(b)'s `not(feature = "std")` cfgs
already handle), and `no_std` *shaping* — `libm` without `scale`, the configuration
that matters for a wasm map — is untouched. The constraint is stated in the feature
table's own comment. A `no_std` mode for `oxiarc-deflate` is the clean upstream fix
and is filed in the 0.2.3 backlog.

DEVIATION 2026-08-05 S9a: `splitrs` was run and its output rejected in favour of a
manual module split (explicitly permitted by the amended D12: "a manual module split
of the 5 const tables is acceptable with the reason recorded"). Measured reason:
`splitrs --max-lines 2000` did preserve the dense generated formatting (5 491 → 5 526
lines, all modules under budget), but it named the modules `constants`, `constants_2`
… `constants_5` — meaningless for tables whose whole value is being findable — and it
omitted `pub use constants_2::*;` from the generated `mod.rs` while that module holds
items the re-exported `functions.rs` reads. The manual split uses the data's own seams
and semantic names, keeps private trie tables private beside the functions that index
them, and was verified to preserve the generated data **byte-for-byte** (normalised
comparison against the pre-split file: identical modulo the 3 added `use super::…`
import lines and the `#![allow(dead_code)]` moved up to `mod.rs`).

DEVIATION 2026-08-05 S8: G9's "`cargo publish --dry-run` per crate in the D21 order"
is only satisfiable for the first two crates, and is completed for the other six by
`cargo package --workspace --exclude oxitext-bench --allow-dirty` instead. Measured
reason: a dry run resolves dependencies against the *registry*, and crates.io carries
0.2.1, not 0.2.2 — `oxitext-shape` fails with `failed to select a version for the
requirement 'oxitext-core = "^0.2.2"' / candidate versions found which didn't match:
0.2.1, 0.2.0, …`. That is structural to a workspace publishing a new version in
dependency order (each crate must actually be published before the next can dry-run),
predates this workstream, and is not a defect in anything the absorption touched.
`cargo publish --dry-run` is therefore green for `oxitext-swash` (70 files) and
`oxitext-core`; the workspace-wide `cargo package` — which verifies against sibling
workspace packages rather than the registry — packaged AND verified **all 8 crates**
clean, which is the same evidence G9 exists to produce.

DEVIATION 2026-08-05 S6: two departures from the plan's confine list / D9, both
forced by gates the plan itself makes binding.
(1) `crates/oxitext-raster/src/color.rs:340` was edited — the plan confines edits to
`crates/oxitext-swash/**` plus a named file list, and no `.rs` file in an oxitext-owned
crate was to change. But S0 measured this as the workspace's ONE pre-existing clippy
warning (`clippy::question_mark`, from toolchain 1.97.1, predating this workstream), and
G4's bar is literally zero. Leaving it would have made every later "G4 green" report
mean "green except one", under which a real regression hides. The fix is clippy's own
machine-applicable suggestion — `match rgba.get(start..end) { Some(row) => …, None =>
return None }` → `out.extend_from_slice(rgba.get(start..end)?);` — in a COLR clipping
helper that touches no swash import, so D2's "zero import rewrites" claim is untouched.
It was deliberately deferred until S6 (the conformance stage) so that S1's checkpoint,
"G2 green with zero source changes in any oxitext-owned crate", held literally.
(2) `#[allow(clippy::redundant_static_lifetimes)]` was added in `src/text/mod.rs`, scoped
to the `lang_data` and `unicode_data` module declarations. D9 says delete that allow
crate-wide AND says the two generated tables must stay byte-identical to upstream; with
the crate-level allow gone, `--fix` rewrote one line in each (`[&'static str; N]` →
`[&str; N]`), so the two requirements collide. The module-scoped allow satisfies both:
the crate is fixed rather than silenced everywhere, and both generated files were
restored and verified byte-identical to `$REG`.

DEVIATION 2026-08-05 S5: the S5 gate clause "bare `--no-default-features` fails with
**our** `compile_error!` message" is **not satisfiable** and is recorded as met-in-part.
Measured reason: cargo compiles dependencies before the crate, and with neither `std`
nor `libm` the `skrifa → read-fonts 0.41.0` edge dies first —
`error[E0599]: no method named 'sin_cos' found for type 'f32'` at
`read-fonts-0.41.0/src/tables/varc.rs:412` (plus two `tan` sites) — so the build aborts
before `oxitext-swash` is ever reached. No arrangement of our source can preempt that.
D18(a)'s guard still ships (it is the correct declaration, and it names the fault for any
consumer whose graph does compile); it was verified live by temporarily inverting its
`cfg` to `any(...)`, which produced exactly
`error: oxitext-swash requires exactly one of the 'std' or 'libm' features` at
`src/lib.rs:38`, then reverting. All six positive combos build warning-free.

DEVIATION 2026-08-05 S4: D5's "byte-copy `$SCRATCH\swash-probe\swash\src\shape\buffer.rs`"
was executed as a byte-copy **followed by a mechanical CRLF→LF normalization**.
Measured reason: the proven patched file carries CRLF terminators (754 CRLF lines,
24 265 B) while pristine swash 0.2.10 and the other 60 vendored files are LF
(708 lines, `file`: "ASCII text"). Copied verbatim it would be the only CRLF file
in the crate and — decisively — `diff -r crates/oxitext-swash/src $REG/src`, which
D11 makes the standing audit and the whole value of vendoring, would report every
line of `buffer.rs` as changed, hiding the 7 real hunks. The normalization is
mechanical, not transcription: the copy was asserted byte-equal to the probe file
before normalizing, and `diff --strip-trailing-cr -q` against it is empty after
(23 511 B). Semantic delta against pristine is unchanged at **7 hunks**.

DEVIATION 2026-08-05 S0: G2/G4's literal `--all-features` cannot be run in this
environment and is replaced, for every stage, by an explicit all-features-minus-
`oxitext-shape/native-fallback` feature list. Measured reason: on Windows
`--all-features` turns on `oxitext-shape/native-fallback`, which pulls
`oxifont-adapter-native 0.2.1`, whose Windows-only `src/directwrite.rs:352` calls
`face.postscript_name()` without `use oxifont_core::FontFace;` in scope —
`error[E0599] ... private field, not a method`, so the workspace does not build at
all with `--all-features` on the *pristine* tree (verified at S0 before any edit).
The plan's 827/16 baseline is macOS-measured, where `directwrite.rs` is `cfg`-ed
out. The substitute feature list (every feature of every selected member except
`native-fallback`) reproduces the plan's figures exactly: **827 passed / 16 skipped**.
Substitute list recorded in `docs/plans/swash-absorb.md` notes and used verbatim at
every gate below. Upstream defect belongs to `oxifont-adapter-native`, not oxitext —
filed as a 0.2.3 note below, not fixed here (oxifont is a read-only separate repo).

### S0 preflight baseline (pristine tree, branch `0.2.2` @ `5c82218`, toolchain 1.97.1 (8bab26f4f 2026-07-14))

- G1 `cargo build --workspace` — GREEN (24.92s).
- G2 (substitute list, see deviation above) — **827 passed / 0 failed / 16 skipped**, matching the plan.
- G3 `cargo test --doc --workspace` — GREEN, **55 doctests passed**, 3 ignored.
- G4 `cargo clippy --all-targets -- -D warnings` — **RED, 1 pre-existing warning**:
  `crates/oxitext-raster/src/color.rs:340` `clippy::question_mark` ("this `match` expression
  can be replaced with `?`"), machine-applicable. Introduced by the toolchain, not by any
  source change: the workspace's last zero-warning certification predates clippy 0.1.97.
  Same single warning with the substitute all-features list (no others workspace-wide).
- G5 `cargo fmt --all --check` — GREEN, 0 diffs.
- G6 `bash scripts/ffi-audit.sh` — **RED, pre-existing false positive**: the script greps
  `brotli v`, which matches the pure-Rust COOLJAPAN crate `oxiarc-brotli v0.4.0`
  (`oxifont 0.2.1 → oxifont-webfont → oxiarc-brotli`). Re-run with an anchored pattern
  (`(^|[^-])brotli v`) the audit is empty: no `freetype-sys`/`fontconfig-sys`/`harfbuzz-sys`/
  real `brotli`/`flate2`/`miniz_oxide`/`ring` in the normal graph. `scripts/` is outside the
  plan's confine list, so the script is left untouched; the anchored re-run is the evidence.
- G7 `cargo deny check bans` — GREEN (`bans ok`).
- G8 baseline: `yazi v0.2.1` and `zeno v0.3.3` **are** in `oxitext-shape`'s normal graph
  today (the freight D4 removes); `cargo tree -e no-dev -i swash` → one root
  (`swash 0.2.10 → oxitext-shape → oxitext`).

### S9 — COOLJAPAN-ification (user election 2026-08-05, after S0–S8 landed)

「swash を取り込んだなら、COOLJAPAN 流儀にしてしまって構わない（2000行ルール、minimum
dependencies etc.)、それと、oxiarc に置き換えるという箇所は今回やってかまわない。」
Amended D9/D11/D12/D16/D19/G8. Byte-identity with upstream is no longer the audit;
the §4(b) headers and `PROVENANCE.md`'s divergence table are.

- **S9a — the 2000-line rule now applies.** Upstream's single generated 5 491-line
  `src/text/unicode_data.rs` → a seven-submodule directory, tables byte-for-byte
  unchanged, public paths preserved by glob re-exports so **no consumer changed**:
  `enums.rs` 813 · `script_tables.rs` 331 · `record_index.rs` 1 469 · `records.rs`
  1 146 · `compose.rs` 353 · `decompose_index.rs` 686 · `decompose.rs` 732 ·
  `mod.rs` 40. Largest file in the crate is now `shape/at.rs` (1 833), and the whole
  workspace is under the limit. `splitrs` was run and rejected — see the S9a
  deviation. **Allow policy:** with the byte-identity argument void, clippy's
  `const`→`static` conversion was taken where `large_const_arrays` fired (`RECORDS`,
  `RECORD_INDEX2`, `DECOMPOSE`, `DECOMPOSE_COMPAT`, `LANG_ENTRIES` — all private to
  the crate, so no public item changed) and both remaining allows were dropped:
  crate-level `large_const_arrays` and the module-scoped
  `redundant_static_lifetimes` from the S6 deviation. **Exactly one crate-level
  allow survives, `too_many_arguments`**, down from upstream's five.
- **S9b — `yazi` → `oxiarc-deflate`.** `scale/bitmap/png.rs` now inflates PNG-embedded
  CBDT/sbix strikes with the same COOLJAPAN inflater `oxitext-core`'s PNG reader uses.
  Mechanism: collect the IDAT payloads, inflate once — the zlib stream *is* their
  concatenation (PNG spec 11.2.4), so it is exact, not an approximation; a single
  IDAT chunk is borrowed with no copy. `yazi` removed from the root
  `[workspace.dependencies]` and the vendored manifest, and **banned in `deny.toml`**
  beside the `swash` ratchet. Evidence: 4 new tests in `png.rs` build real zlib
  streams with `oxiarc-deflate` and decode them to exact RGBA — single chunk, split
  across 2/3/5 chunks (identical output), a corrupt stream (rejected via the Adler-32
  trailer the streaming path never checked), and a PNG with no IDAT (rejected).
- **S9 gate — full D20 battery, all green.** G1 ✓ · G2 **845 passed / 0 failed / 16
  skipped** (841 → 845, +4 png tests; *not* shrunk) · G3 **72 doctests** · G4 **0**
  warnings at default and all-features · G5 clean · G6 unchanged (the pre-existing
  `oxiarc-brotli` false positive; anchored re-run empty, and now also `yazi`-free) ·
  G7 `bans ok` with the new yazi ban live · G8 all four assertions, including
  **`cargo tree -i yazi` empty under every feature combination and workspace-wide** ·
  G9 `cargo publish --dry-run -p oxitext-swash` clean (**77 files**, 255.6 KiB
  compressed) · G10 green on the real 1.89 toolchain.
- **The shaping fix is provably undisturbed.** `shape/buffer.rs` still differs from the
  proven patched file by exactly 2 hunks — its §4(b) header and the appended unit-test
  module — with the fix body byte-identical, and the S3/S4 Devanagari corpus is **6/6
  green with the same D8 goldens** after the restyle.
- Vendored crate: 61 → **68 files**, 22 970 code SLoC; **37 files still byte-identical**
  to upstream.

### S7/S8 results, and the 0.2.3 backlog this absorption creates

- S7 gate: G1 green; G2 **841 passed / 0 failed / 16 skipped**; G3 **72 doctests**; G4 **0**
  warnings at default and at all-features (folding `OxiText`/`OxiGIS`/`COOLJAPAN` into
  `clippy.toml`'s `doc-valid-idents` — the vendored manifest's `[lints.clippy]
  doc_markdown = "warn"` made every prose mention of our own project names a hard error
  under `-D warnings`); G5 clean; G7 `bans ok` **with the new `{ name = "swash" }`
  ratchet in place**; G10 `cargo +1.89 check -p oxitext-swash` green. G6 unchanged from
  its S0 state (the pre-existing `oxiarc-brotli` false positive; anchored re-run empty).
- S8 gate: `cargo publish --dry-run` clean for `oxitext-swash` (**70 files**, 1.3 MiB /
  247.2 KiB compressed: 61 sources + `LICENSE-APACHE` + `LICENSE-MIT` + `NOTICE` +
  `PROVENANCE.md` + `README.md` + the 4 cargo-generated files) and for `oxitext-core`.
  See the S8 deviation below for the other six. `git status` shows exactly the intended
  dirty set; `crates/oxitext-shape/Cargo.toml` is **byte-unchanged** and no `.rs` file in
  any oxitext-owned crate changed except the one recorded in the S6 deviation.
- **Nothing was committed, staged, pushed or published; no version was bumped.**

**Future backlog created by the absorption** (none blocked 0.2.2; none landed in 0.2.3 either — that release's scope was the notdef-fallback fix above instead):

- [x] `yazi` → `oxiarc-deflate` in `oxitext-swash`'s `scale/bitmap/png.rs` — **done in S9b**
      (user election 2026-08-05). `yazi` is gone from every manifest and every feature
      combination of the graph, and `deny.toml` bans it.
- [ ] Ask `oxiarc` for a `no_std` mode in `oxiarc-deflate`. It is std-only by design today
      (`std::io` ×22, `std::sync`, `std::thread`), which is why `oxitext-swash`'s `scale`
      feature had to gain `std` when it replaced the `no_std`-capable `yazi` (S9b deviation).
      Nothing in this workspace needs `no_std` + `scale`, but the capability was lost and
      the fix belongs upstream, not in a fork of a fork.
- [ ] Raise `skrifa` past swash's `<= 0.44` ceiling to 0.45.1, and collapse the
      0.42.1/0.44.0 duplicate in OxiGIS's lockfile.
- [ ] De-unsafe `oxitext-swash/src/internal/parse.rs` (18 of the 54 inherited sites), then
      add `#![warn(clippy::undocumented_unsafe_blocks)]` and `#![deny(unsafe_op_in_unsafe_fn)]`.
      Both are red-on-arrival today under `-D warnings`, which is why 0.2.2 ships the
      inventory and the fuzz target instead.
- [ ] Run a `shape_untrusted_font` campaign and triage. Expect to reproduce upstream's own
      open findings (dfrg/swash #123–#126, #133) in inherited parse code; only *new*
      findings are ours.
- [ ] Upstream `dfrg/swash`: PR the reordering fix against #93, confirm the reproducer,
      offer the `--all-features` dead-import fix, and consider PRs #130/#135. **Requires
      explicit user approval** — the S4 tree state is preserved as the PR-able diff (fix
      only, no conformance churn) so the option stays open at zero cost.
- [ ] Upstream #105 (clusters over `MAX_CLUSTER_SIZE` producing overlapping source ranges)
      stays unfixed and is recorded in `PROVENANCE.md`, together with the
      `reorder_complex`'s literal `64` ↔ `shape/mod.rs`'s `.min(64)` ↔ `MAX_CLUSTER_SIZE = 32`
      coupling. Do not "unify" those constants.
- [ ] Non-Devanagari Complex-script coverage gap: `reorder_complex` serves every
      `EngineMode::Complex` script, but the shipped corpus only covers Devanagari, because
      no redistributable Bengali/Tamil/Oriya fixture was added. The A/B sweep did show
      Bengali `র্ক` and Oriya `ର୍କ` recovering their reph. Adding fixtures would make
      fresh-==-reused and no-panic assertions available for them at zero reference cost.
- [ ] Reph placement is not cross-validated against HarfBuzz. For `last_base == first_base`
      the fix places the reph at the END of the syllable — where swash's accidentally-correct
      path put it, and what Noto and Nirmala render correctly — but dev2's spec position is
      `REPH_POS_BEFORE_POST`, so a single-base syllable *also* carrying post-base matras may
      differ. No such word appeared in the 24-word corpus or the 21-case sweep. `rustybuzz`
      is already an optional `oxitext-shape` dep and is pure Rust: that is the cheap
      settlement if the question is ever raised.
- [ ] `oxifont-adapter-native 0.2.1` does not compile on Windows —
      `src/directwrite.rs:352` calls `face.postscript_name()` without
      `use oxifont_core::FontFace;` in scope. It is reached only through
      `oxitext-shape/native-fallback`, so `--all-features` cannot be run on Windows at all
      (see the S0 deviation). Belongs to `oxifont`, a separate read-only repo on its own
      release train; report it there.
- [ ] `scripts/ffi-audit.sh` greps the unanchored pattern `brotli v`, which matches the
      pure-Rust COOLJAPAN crate `oxiarc-brotli`, so G6 has been red since `oxifont 0.2.1`
      entered the graph. `scripts/` was outside this workstream's confine list. One-line
      fix: anchor the alternation (`(^|[^-])brotli v`).
