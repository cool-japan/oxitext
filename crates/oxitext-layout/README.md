# oxitext-layout — Text layouter for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-layout.svg)](https://crates.io/crates/oxitext-layout)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-layout` is the **layout** stage of the OxiText pipeline: it consumes the `oxitext_core::ShapedRun`s produced by [`oxitext-shape`](https://crates.io/crates/oxitext-shape) and positions them into lines and paragraphs as `oxitext_core::PositionedGlyph`s. It implements the bidirectional algorithm (UAX #9), the line-breaking algorithm (UAX #14) with both greedy and Knuth-Plass strategies, vertical CJK flow (a UAX #50 subset), tate-chu-yoko, ruby (furigana) annotations, soft-hyphen breaking, and multi-style (rich-text) layout with baseline alignment.

This crate is **100% Pure Rust** and `#![forbid(unsafe_code)]`. It builds on the well-maintained Pure-Rust `unicode-bidi`, `unicode-linebreak`, and `ttf-parser` crates, and uses `rayon` for parallel layout passes. Optional features add `hypher`-driven automatic hyphenation and `oxitext-icu` (ICU4X / CLDR) line breaking.

## Installation

```toml
[dependencies]
oxitext-layout = "0.2.2"
```

With optional capabilities:

```toml
[dependencies]
# Automatic (dictionary) hyphenation and ICU4X/CLDR line breaking
oxitext-layout = { version = "0.2.2", features = ["hyphenation", "icu"] }
```

## Quick Start

### Simple cursor-advance layout

```rust
use oxitext_layout::SimpleLayouter;
use oxitext_core::{LayoutConstraints, ShapedGlyph, ShapedRun};
use std::sync::Arc;

let run = ShapedRun {
    glyphs: [10.0, 10.0, 10.0]
        .iter()
        .map(|&adv| ShapedGlyph { x_advance: adv, ..Default::default() })
        .collect(),
    font_data: Arc::from(&b""[..]),
};

let layouter = SimpleLayouter::new();
let constraints = LayoutConstraints { max_width: 800.0, font_size: 16.0 };
let positioned = layouter.layout(&[run], &constraints)?;
assert_eq!(positioned.len(), 3);
# Ok::<(), oxitext_core::OxiTextError>(())
```

### Rich line-aware layout with the engine

```rust
use oxitext_layout::{LayoutEngine, BreakingStrategy};
use oxitext_core::{LayoutConstraints, TextAlignment, ShapedRun};

let mut engine = LayoutEngine::new();
let runs: Vec<ShapedRun> = Vec::new(); // produced by oxitext-shape
let constraints = LayoutConstraints { max_width: 600.0, font_size: 18.0 };

let result = engine.layout_with_strategy(
    "The quick brown fox",
    &runs,
    &constraints,
    TextAlignment::Justify,
    None,                         // optional FontVerticalMetrics
    BreakingStrategy::KnuthPlass, // optimal line breaking
)?;

println!("{} lines, {}px tall", result.lines.len(), result.metrics.total_height);
# Ok::<(), oxitext_core::OxiTextError>(())
```

## Examples

[`examples/word_aware_layout.rs`](examples/word_aware_layout.rs) walks through the primary `LayoutEngine` flow end-to-end: hand-built `ShapedRun`/`ShapedGlyph` input → UAX #14 word wrapping → per-line/paragraph metrics, plus the hand-off to an SDF atlas via `LayoutResult::unique_glyphs_for_atlas`.

```sh
cargo run -p oxitext-layout --example word_aware_layout
```

## API Overview

### `SimpleLayouter`

A minimal cursor-advance layouter supporting horizontal (LTR) and vertical flow.

| Item | Description |
|------|-------------|
| `flow_direction` (field) | The flow direction for this instance |
| `new()` | Create a horizontal-flow layouter (the default) |
| `with_flow_direction(dir)` | Builder setter for the flow direction |
| `layout(runs, constraints)` | Position glyphs; dispatches to horizontal or vertical based on `flow_direction`. Wraps when the cursor exceeds `max_width` |

### Layout engine — `engine` module

| Item | Description |
|------|-------------|
| `LayoutEngine` | Rich line-aware layouter with an internal break cache and dirty-range tracking |
| `LayoutResult` | Structured output: `glyphs`, `lines`, `metrics`, `decorations`, `inline_objects`; method `hit_test()`, atlas helpers `unique_glyphs_for_atlas()`, `rasterization_inputs()`, `sdf_glyph_set()` |
| `Line` | One laid-out line: `glyph_start`, `glyph_end`, `metrics`; `len()`, `is_empty()` |
| `LineMetrics` | Per-line vertical metrics: `ascent`, `descent`, `leading`, `baseline_y`, `width`; `height()` |
| `ParagraphMetrics` | Aggregate metrics: `total_height`, `total_width`, `line_count`, `overflow`, `truncated` |
| `BreakingStrategy` | `Greedy` (default, O(n)) or `KnuthPlass` (optimal demerit minimisation) |

Key `LayoutEngine` methods:

| Method | Description |
|--------|-------------|
| `new()` | Create an engine |
| `layout(text, runs, constraints, alignment, font_metrics)` | Default layout (CLDR when the `icu` feature is on, else UAX #14 greedy) |
| `layout_uax14(...)` | Force UAX #14 (`unicode-linebreak`) greedy breaking regardless of features |
| `layout_with_strategy(..., strategy)` | Layout with an explicit `BreakingStrategy` |
| `layout_with_break_points(...)` | Layout at caller-supplied break positions |
| `layout_cldr(...)` | CLDR-compliant breaking (feature `icu`) |
| `layout_vertical(...)` | Vertical (top-to-bottom column) layout |
| `layout_paragraphs(...)` | Lay out multiple paragraphs with inter-paragraph spacing |
| `layout_with_options(..., options)` | Layout honouring a full `LayoutOptions` (alignment, truncation, tabs, hanging punctuation, decorations, inline objects) |
| `layout_styled_runs(runs, source_text, max_width, options)` | Multi-style (rich-text) layout with baseline alignment across mixed fonts/sizes |
| `mark_dirty(range)`, `clear_dirty()`, `has_dirty()`, `layout_if_dirty(...)` | Incremental relayout support |

### Layout options — `options` module

| Item | Description |
|------|-------------|
| `LayoutOptions` | Comprehensive per-pass config: `alignment`, `flow_direction`, `truncation`, `tab_stops`, `paragraph_spacing`, `hanging_punctuation`, `decoration`, `inline_objects`; `builder()` |
| `LayoutOptionsBuilder` | Fluent builder: `alignment()`, `flow_direction()`, `truncation()`, `tab_stops()`, `paragraph_spacing()`, `hanging_punctuation()`, `decoration()`, `inline_objects()`, `build()` |
| `TruncationMode` | Ellipsis truncation config: `max_width`, `ellipsis_advance`, `ellipsis_glyph_id` |
| `TabStops` | Tab-stop config: `positions`, `default_interval`; `with_interval()`, `next_stop()` |

### Bidirectional text — `bidi` and `reorder` modules

| Item | Description |
|------|-------------|
| `bidi::BidiParagraph` | UAX #9 paragraph analysis; `new(text, base_rtl)`, `runs()`, `base_level()`, `is_rtl()`, `levels()` |
| `bidi::BidiRun` | A uniform-level run: `start`, `end`, `level` (visual order) |
| `needs_bidi(text)` | Fast RTL-presence check via Unicode block ranges (re-exported from `reorder`) |
| `reorder::line_visual_order(levels)` | UAX #9 L2 visual reordering permutation for a slice of embedding levels |

### Line breaking — `linebreak` and `knuth_plass` modules

| Item | Description |
|------|-------------|
| `linebreak::LineBreaker` | UAX #14 break opportunities; `new(text)`, `breaks()`, `iter()`, `IntoIterator` |
| `linebreak::LineBreak` | `Mandatory` or `Allowed` break classification |
| `knuth_plass::optimal_breaks(advances, is_whitespace, break_opps, max_width)` | Knuth-Plass optimal break positions (minimises total demerits) |

### Vertical CJK — `vertical` and `tate_chu_yoko` modules

| Item | Description |
|------|-------------|
| `vmtx_advance_for_glyph(face_data, glyph_id, em_size)` | Vertical advance from the font's `vmtx` table (re-exported from `vertical`) |
| `vertical::is_upright_in_vertical(c)` | Whether a character stays upright in vertical text |
| `vertical::VerticalMetrics` | Per-glyph vertical metrics; `for_char()`, `for_glyph()` |
| `tate_chu_yoko::detect_runs(entries, em_size)` | Detect short horizontal runs (e.g. 2-digit numbers) within vertical lines |
| `tate_chu_yoko::tcy_combined_advance(...)` | Combined advance of a tate-chu-yoko run |
| `tate_chu_yoko::GlyphEntry`, `TateChuYokoRun`, `MAX_TCY_RUN_LEN` | TCY input/run types and the maximum run length constant |

### Ruby (furigana) — `ruby` module

| Item | Description |
|------|-------------|
| `layout_ruby(base_glyphs, ruby_shaped, annotation, ruby_px_size, base_line_height)` | Position ruby glyphs centred over their base span |
| `RubyAnnotation` | `base_range`, `ruby_text`, `position` |
| `RubyLayout` | `base_glyphs`, `ruby_glyphs`, `ruby_y_offset`, `extra_line_height` |
| `RubyPosition` | `Above` or `Below` |

### Multi-style layout — `styled` module

| Item | Description |
|------|-------------|
| `StyledRun` | A pre-shaped run with its own face/size/colour: `glyphs`, `metrics`, `px_size`, `color`, `font_data`, `vertical_position` |

### Hyphenation — `hyphenation` module

| Item | Description |
|------|-------------|
| `soft_hyphen_breaks(text)` | Byte offsets of soft hyphens (U+00AD) in the text (re-exported at the crate root) |
| `hyphenation::automatic_hyphen_breaks(text, lang)` | Dictionary-based hyphenation points via `hypher` (feature `hyphenation`) |

### Re-exports from `oxitext-core`

For convenience the crate re-exports `DecorationRect`, `InlineObject`, `PositionedInlineObject`, `TextDecoration`, and `VerticalPosition`.

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `hyphenation` | no | Dictionary-based automatic hyphenation via `hypher` (`hyphenation::automatic_hyphen_breaks`) |
| `icu` | no | CLDR-compliant line breaking via `oxitext-icu` (ICU4X); `LayoutEngine::layout` then routes through `layout_cldr` |

## Cross-references

- [`oxitext`](https://crates.io/crates/oxitext) — high-level façade combining all stages.
- [`oxitext-core`](https://crates.io/crates/oxitext-core) — the shared `ShapedRun` / `PositionedGlyph` / styling types.
- [`oxitext-shape`](https://crates.io/crates/oxitext-shape) — produces the `ShapedRun`s this crate lays out.
- [`oxitext-raster`](https://crates.io/crates/oxitext-raster) — rasterizes the `PositionedGlyph`s this crate emits.
- `oxitext-icu` — ICU4X / CLDR line breaking used by the `icu` feature.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
