//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::linebreak::{LineBreak, LineBreaker};
use oxitext_core::{
    FontVerticalMetrics, LayoutConstraints, OxiTextError, PositionedGlyph, ShapedGlyph, ShapedRun,
    TextAlignment,
};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use std::sync::Arc;

use super::functions::{
    apply_hanging_punctuation, apply_truncation, build_ranges_from_kp_breaks, compute_alignment,
    count_internal_ws_gaps,
};

/// Controls which line-breaking algorithm the layout engine uses.
///
/// The default is [`BreakingStrategy::Greedy`], which runs in O(n) and matches
/// the behaviour of browsers' `white-space: normal` wrapping.
/// [`BreakingStrategy::KnuthPlass`] minimises total paragraph demerits (see
/// [`crate::knuth_plass`]) and typically produces more even line lengths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BreakingStrategy {
    /// Greedy (first-fit) algorithm — O(n), default.
    #[default]
    Greedy,
    /// Knuth-Plass optimal algorithm — minimises total paragraph demerits.
    ///
    /// Falls back to greedy when `max_width` is 0 or no feasible solution
    /// exists.
    KnuthPlass,
}
/// The structured result of a layout pass.
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// All positioned glyphs in logical (reading) order.
    pub glyphs: Vec<PositionedGlyph>,
    /// Line records indexing into [`Self::glyphs`].
    pub lines: Vec<Line>,
    /// Aggregate metrics.
    pub metrics: ParagraphMetrics,
    /// Decoration rectangles computed from the layout (underlines, overlines,
    /// strikethroughs). Empty unless decorations are requested via
    /// [`crate::options::LayoutOptions::decoration`].
    pub decorations: Vec<oxitext_core::DecorationRect>,
    /// Positioned inline objects (images, widgets) placed during layout.
    pub inline_objects: Vec<oxitext_core::PositionedInlineObject>,
}
impl LayoutResult {
    /// Find the glyph nearest to pixel coordinates `(x, y)` for hit-testing
    /// and cursor placement during text selection.
    ///
    /// Returns `Some((line_index, glyph_index_within_line, cluster_byte_offset))`
    /// where:
    /// - `line_index` is the index into [`LayoutResult::lines`],
    /// - `glyph_index_within_line` is the 0-based position within that line's
    ///   glyph range (i.e. `0` is the first glyph of the line),
    /// - `cluster_byte_offset` is [`PositionedGlyph::cluster`] — the UTF-8
    ///   byte offset of the glyph's source character.
    ///
    /// If `(x, y)` falls outside all lines the nearest line is chosen. If it
    /// falls outside all glyphs on the chosen line the nearest endpoint glyph
    /// is returned.
    ///
    /// Returns `None` only when `self.lines` is empty.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<(usize, usize, u32)> {
        if self.lines.is_empty() {
            return None;
        }
        let line_idx = {
            let mut best = 0usize;
            let mut best_dist = f32::MAX;
            'line_search: for (li, line) in self.lines.iter().enumerate() {
                let top = line.metrics.baseline_y - line.metrics.ascent;
                let bottom = line.metrics.baseline_y + line.metrics.descent;
                if y >= top && y <= bottom {
                    best = li;
                    break 'line_search;
                }
                let mid = (top + bottom) * 0.5;
                let dist = (y - mid).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best = li;
                }
            }
            best
        };
        let line = &self.lines[line_idx];
        let gs = line.glyph_start;
        let ge = line.glyph_end;
        if gs >= ge {
            return Some((line_idx, 0, 0));
        }
        let mut best_gi = gs;
        let mut best_dist = f32::MAX;
        'glyph_search: for gi in gs..ge {
            let g = &self.glyphs[gi];
            let left = g.pos.0;
            let right = g.pos.0 + g.advance_x;
            if x >= left && x <= right {
                best_gi = gi;
                break 'glyph_search;
            }
            let mid = (left + right) * 0.5;
            let dist = (x - mid).abs();
            if dist < best_dist {
                best_dist = dist;
                best_gi = gi;
            }
        }
        let glyph_idx_in_line = best_gi - gs;
        let cluster = self.glyphs[best_gi].cluster;
        Some((line_idx, glyph_idx_in_line, cluster))
    }

    /// Return the unique `(glyph_id, font_size)` pairs present in this layout,
    /// suitable for pre-warming an SDF atlas or batching rasterisation.
    ///
    /// Each `(gid, px_size)` pair appears exactly once regardless of how many
    /// times that glyph occurs in the layout.  The order of the returned entries
    /// is stable: pairs are emitted in the order their first occurrence is
    /// encountered when iterating [`Self::glyphs`] from index 0.
    ///
    /// The `font_size_bits` value stored internally as a `u32` key is obtained
    /// via `font_size.to_bits()` so that equal IEEE-754 floats are always
    /// treated as equal keys even in a `HashSet`.
    ///
    /// # Rasteriser usage
    ///
    /// ```rust,ignore
    /// for (glyph_id, px_size) in layout.unique_glyphs_for_atlas() {
    ///     atlas.pre_warm(glyph_id, px_size);
    /// }
    /// ```
    pub fn unique_glyphs_for_atlas(&self) -> Vec<(u16, f32)> {
        use std::collections::HashSet;
        let mut seen: HashSet<(u16, u32)> = HashSet::new();
        let mut result = Vec::new();
        for g in &self.glyphs {
            let key = (g.gid, g.font_size.to_bits());
            if seen.insert(key) {
                result.push((g.gid, g.font_size));
            }
        }
        result
    }

    /// Return per-glyph `(glyph_id, x, y, font_size)` tuples ready for direct
    /// handoff to a rasteriser.
    ///
    /// Positions are in pixel coordinates with the origin at the top-left of
    /// the text block (matching [`PositionedGlyph::pos`]).  The returned
    /// `Vec` preserves logical (reading) order and contains exactly one entry
    /// per glyph in [`Self::glyphs`].
    ///
    /// # Rasteriser usage
    ///
    /// ```rust,ignore
    /// for (gid, x, y, px_size) in layout.rasterization_inputs() {
    ///     rasterizer.draw(gid, x, y, px_size);
    /// }
    /// ```
    pub fn rasterization_inputs(&self) -> Vec<(u16, f32, f32, f32)> {
        self.glyphs
            .iter()
            .map(|g| (g.gid, g.pos.0, g.pos.1, g.font_size))
            .collect()
    }

    /// Returns the set of glyphs that should be pre-loaded into an SDF atlas
    /// before rendering.  Each entry is `(glyph_id, px_size)`.
    ///
    /// This is an alias for [`Self::unique_glyphs_for_atlas`] with an
    /// SDF-oriented name for use at the oxitext-sdf integration boundary.
    ///
    /// # Usage with oxitext-sdf
    ///
    /// ```rust,ignore
    /// let layout = engine.layout(text, runs, &constraints, alignment, None)?;
    /// for (glyph_id, px_size) in layout.sdf_glyph_set() {
    ///     if let Ok(Some(tile)) =
    ///         oxitext_sdf::glyph_to_sdf_tile(font_data, glyph_id, px_size, 64, 4.0)
    ///     {
    ///         atlas.pack_tile(tile);
    ///     }
    /// }
    /// ```
    pub fn sdf_glyph_set(&self) -> Vec<(u16, f32)> {
        self.unique_glyphs_for_atlas()
    }
}
/// Resolved vertical line metrics derived from font metrics or a size fallback.
#[derive(Debug, Clone, Copy)]
struct VerticalLineModel {
    ascent: f32,
    descent: f32,
    leading: f32,
    line_height: f32,
}
impl VerticalLineModel {
    /// Build a vertical model from optional font metrics and a font size.
    ///
    /// When `metrics` is `Some`, the design-unit ascender/descender/line-gap
    /// are scaled to pixels by `font_size / units_per_em`. Otherwise a
    /// reasonable fallback of `0.8 / 0.2 / 0.4 × font_size` is used (the same
    /// proportions the legacy [`crate::SimpleLayouter`] assumed).
    fn from_metrics(metrics: Option<&FontVerticalMetrics>, font_size: f32) -> Self {
        match metrics {
            Some(m) => {
                let ascent = m.ascent_px(font_size);
                let descent = m.descent_px(font_size);
                let leading = m.line_gap_px(font_size);
                Self {
                    ascent,
                    descent,
                    leading,
                    line_height: ascent + descent + leading,
                }
            }
            None => {
                let ascent = font_size * 0.8;
                let descent = font_size * 0.2;
                let leading = font_size * 0.4;
                Self {
                    ascent,
                    descent,
                    leading,
                    line_height: ascent + descent + leading,
                }
            }
        }
    }
}
/// Word-aware, alignment-capable layout engine.
///
/// Carries two optional caches to improve throughput in GUI loops and other
/// scenarios where the same (or similarly-sized) text is laid out repeatedly:
///
/// - **`scratch`** — a reusable [`PositionedGlyph`] buffer.  On every
///   [`Self::layout_with_strategy`] call the buffer is cleared (keeping its
///   allocated capacity) and refilled, so the heap allocation survives across
///   calls.
/// - **`break_cache_text` / `break_cache_ops`** — the last source string and
///   its precomputed UAX #14 break opportunities.  When the caller re-lays
///   out the same text (e.g. after a window resize) the expensive
///   [`crate::linebreak::LineBreaker`] pass is skipped.
/// - **`dirty_ranges`** — byte offset ranges in the source text that have
///   changed since the last layout pass.  When non-empty, the next layout
///   call will re-break all affected lines.  Cleared automatically by
///   [`Self::layout_if_dirty`] after a successful relayout.
#[derive(Debug, Default)]
pub struct LayoutEngine {
    /// Reusable scratch buffer for positioned glyphs (capacity survives calls).
    scratch: Vec<PositionedGlyph>,
    /// Source text of the last break-opportunity computation.
    pub(crate) break_cache_text: String,
    /// Break opportunities from the last computation: `(byte_offset, kind)`.
    pub(crate) break_cache_ops: Vec<(usize, crate::linebreak::LineBreak)>,
    /// Dirty ranges (byte offsets in the source text) that have changed since
    /// the last layout pass.  If non-empty, the next layout call will re-break
    /// all lines.  In a future optimisation only lines overlapping dirty ranges
    /// would be re-broken; for now the full paragraph is always re-laid out.
    dirty_ranges: Vec<std::ops::Range<usize>>,
}
impl LayoutEngine {
    /// Creates a new layout engine.
    pub fn new() -> Self {
        Self {
            scratch: Vec::new(),
            break_cache_text: String::new(),
            break_cache_ops: Vec::new(),
            dirty_ranges: Vec::new(),
        }
    }

    /// Mark a byte range of the source text as modified (content changed,
    /// inserted, or deleted).  The next layout call will re-layout lines
    /// affected by this range.
    ///
    /// Multiple overlapping or disjoint ranges can be accumulated before
    /// triggering a layout pass.  All dirty markers are cleared automatically
    /// by [`Self::layout_if_dirty`] after a successful relayout.
    pub fn mark_dirty(&mut self, range: std::ops::Range<usize>) {
        self.dirty_ranges.push(range);
    }

    /// Clear all dirty markers.
    ///
    /// Called automatically by [`Self::layout_if_dirty`] after a layout pass.
    /// You can also call this manually to discard pending dirty state without
    /// triggering a relayout (e.g. after discarding the associated text edit).
    pub fn clear_dirty(&mut self) {
        self.dirty_ranges.clear();
    }

    /// Returns `true` if any text range has been marked dirty since the last
    /// [`Self::clear_dirty`] or [`Self::layout_if_dirty`] call.
    pub fn has_dirty(&self) -> bool {
        !self.dirty_ranges.is_empty()
    }

    /// Relayout only if dirty; otherwise return the cached layout result.
    ///
    /// - `cached`: the previous [`LayoutResult`] to return unchanged when no
    ///   dirty ranges are pending and a cached result is available.
    /// - `layout_fn`: a closure that produces a fresh [`LayoutResult`] when a
    ///   relayout is needed.  The closure receives `&mut LayoutEngine` so it
    ///   can call any of the layout methods directly.
    ///
    /// After a relayout `layout_fn` is invoked, all dirty markers are cleared
    /// automatically.  If the engine is clean *and* `cached` is `None`, the
    /// closure is still called (there is nothing to return otherwise).
    pub fn layout_if_dirty<F>(&mut self, cached: Option<LayoutResult>, layout_fn: F) -> LayoutResult
    where
        F: FnOnce(&mut LayoutEngine) -> LayoutResult,
    {
        if self.dirty_ranges.is_empty() {
            if let Some(prev) = cached {
                return prev;
            }
        }
        let result = layout_fn(self);
        self.clear_dirty();
        result
    }
    /// Lays out `runs` over `source_text`, wrapping at line-break opportunities.
    ///
    /// When the `icu` feature is enabled the layout uses CLDR-compliant line
    /// breaking via [`Self::layout_cldr`] (better quality for CJK, Thai, and
    /// other complex scripts).  Without the `icu` feature this falls back to
    /// UAX #14 line breaking via the greedy (first-fit) algorithm.
    ///
    /// To explicitly request UAX #14 line breaking regardless of the `icu`
    /// feature, use [`Self::layout_uax14`].
    ///
    /// - `source_text` must be the exact string the runs were shaped from, so
    ///   that [`ShapedGlyph::cluster`] byte offsets index into it.
    /// - `constraints.max_width` of `0.0` disables wrapping (single line per
    ///   mandatory break).
    /// - `alignment` controls horizontal placement within `max_width`.
    /// - `font_metrics`, when supplied, drives accurate line height; otherwise
    ///   a size-proportional fallback is used.
    ///
    /// # Errors
    /// Currently infallible for well-formed input; returns `Err` only for
    /// forward compatibility.
    pub fn layout(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
    ) -> Result<LayoutResult, OxiTextError> {
        #[cfg(feature = "icu")]
        {
            // When ICU is available, use CLDR-compliant line breaking for
            // better quality segmentation across complex scripts.
            self.layout_cldr(source_text, runs, constraints, alignment, font_metrics)
        }
        #[cfg(not(feature = "icu"))]
        {
            // Fall back to UAX #14 unicode-linebreak (greedy algorithm).
            self.layout_with_strategy(
                source_text,
                runs,
                constraints,
                alignment,
                font_metrics,
                BreakingStrategy::Greedy,
            )
        }
    }

    /// Lays out `runs` using UAX #14 (`unicode-linebreak`) line breaking,
    /// regardless of whether the `icu` feature is compiled in.
    ///
    /// This is the explicit opt-out from CLDR line breaking.  Use this when
    /// you need a consistent UAX #14 code path independent of feature flags,
    /// for example in tests that compare break positions.
    ///
    /// Uses the greedy (first-fit) algorithm.  For Knuth-Plass optimal breaking
    /// call [`LayoutEngine::layout_with_strategy`] directly with
    /// [`BreakingStrategy::KnuthPlass`].
    ///
    /// # Errors
    /// Currently infallible for well-formed input; returns `Err` only for
    /// forward compatibility.
    pub fn layout_uax14(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
    ) -> Result<LayoutResult, OxiTextError> {
        self.layout_with_strategy(
            source_text,
            runs,
            constraints,
            alignment,
            font_metrics,
            BreakingStrategy::Greedy,
        )
    }
    /// Lays out `runs` over `source_text` using the specified breaking
    /// strategy.
    ///
    /// This is the full-featured entry point.  [`LayoutEngine::layout`] is a
    /// convenience wrapper that always uses [`BreakingStrategy::Greedy`].
    ///
    /// When `strategy` is [`BreakingStrategy::KnuthPlass`] and
    /// `constraints.max_width > 0`, the algorithm calls
    /// [`crate::knuth_plass::optimal_breaks`] to compute globally optimal
    /// break positions before positioning glyphs.  If the KP solver finds no
    /// feasible solution it automatically falls back to the greedy algorithm.
    ///
    /// # Errors
    /// Currently infallible for well-formed input; returns `Err` only for
    /// forward compatibility.
    pub fn layout_with_strategy(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
        strategy: BreakingStrategy,
    ) -> Result<LayoutResult, OxiTextError> {
        self.layout_impl(
            source_text,
            runs,
            constraints,
            alignment,
            font_metrics,
            strategy,
            None,
        )
    }
    /// Lays out `runs` using externally-supplied break point byte offsets.
    ///
    /// Identical to [`LayoutEngine::layout_with_strategy`] (greedy algorithm)
    /// except that instead of computing UAX #14 break opportunities internally,
    /// this method treats every offset in `break_points` as an
    /// [`crate::linebreak::LineBreak::Allowed`] opportunity.  This allows
    /// callers — e.g. the facade or ICU-backed pipeline — to inject their own
    /// (CLDR-compliant) break points without re-running the built-in linebreaker.
    ///
    /// # Arguments
    /// - `source_text` — the source string the runs were shaped from.
    /// - `runs` — shaped glyph runs.
    /// - `constraints` — layout constraints (max width, font size).
    /// - `alignment` — horizontal text alignment.
    /// - `font_metrics` — optional font vertical metrics.
    /// - `break_points` — slice of UTF-8 byte offsets where line breaks are
    ///   permitted.  The slice need not be sorted (it will be searched with
    ///   binary search after sorting internally).
    ///
    /// # Errors
    /// Currently infallible for well-formed input.
    pub fn layout_with_break_points(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
        break_points: &[usize],
    ) -> Result<LayoutResult, OxiTextError> {
        self.layout_impl(
            source_text,
            runs,
            constraints,
            alignment,
            font_metrics,
            BreakingStrategy::Greedy,
            Some(break_points),
        )
    }
    /// CLDR-compliant layout using [`oxitext_icu::IcuSegmenter`] for line breaking.
    ///
    /// When the `icu` feature is enabled, this method creates an
    /// [`oxitext_icu::IcuSegmenter`], queries CLDR line-break opportunities for
    /// `source_text`, and delegates to [`Self::layout_with_break_points`].
    ///
    /// This provides CLDR-compliant line breaking as a drop-in replacement for
    /// the UAX #14 unicode-linebreak path used by [`Self::layout`].
    ///
    /// # Errors
    /// Currently infallible for well-formed input.
    #[cfg(feature = "icu")]
    pub fn layout_cldr(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
    ) -> Result<LayoutResult, OxiTextError> {
        let seg = oxitext_icu::IcuSegmenter::new();
        let icu_breaks = seg.line_break_opportunities(source_text);

        // Build the combined break list: ICU Allowed breaks merged with
        // Mandatory breaks at every hard-break character (`\n`, `\r\n`).
        // We pre-seed break_cache_ops so that layout_impl sees the correct
        // LineBreak::Mandatory entries for hard newlines — ICU returns
        // line-break *opportunities* as Allowed only.
        let mut ops: Vec<(usize, LineBreak)> = icu_breaks
            .iter()
            .map(|&off| (off, LineBreak::Allowed))
            .collect();

        for (i, c) in source_text.char_indices() {
            if c == '\n' {
                // Byte offset of the character *after* the newline is the break point.
                let after_newline = i + c.len_utf8();
                ops.push((after_newline, LineBreak::Mandatory));
            }
        }

        // Also merge soft-hyphen (U+00AD) opportunities for parity with the
        // non-ICU path.
        let soft = crate::hyphenation::soft_hyphen_breaks(source_text);
        for off in soft {
            ops.push((off, LineBreak::Allowed));
        }

        // Sort by offset, then deduplicate: Mandatory wins over Allowed at the
        // same offset.
        ops.sort_unstable_by_key(|(off, _)| *off);
        ops.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                if later.1 == LineBreak::Mandatory {
                    earlier.1 = LineBreak::Mandatory;
                }
                true // remove `later` (keep `earlier`, now possibly upgraded)
            } else {
                false
            }
        });

        // Pre-seed the cache so layout_impl reuses our ops.
        self.break_cache_text = source_text.to_owned();
        self.break_cache_ops = ops;

        // Delegate directly to layout_with_strategy (bypassing layout() to avoid
        // re-entering layout_cldr when the icu feature is enabled).  The break
        // cache is already populated, so no LineBreaker pass will be triggered.
        self.layout_with_strategy(
            source_text,
            runs,
            constraints,
            alignment,
            font_metrics,
            BreakingStrategy::Greedy,
        )
    }
    /// Internal layout implementation shared by all horizontal layout paths.
    ///
    /// `external_breaks`, when `Some`, bypasses the UAX #14 `LineBreaker` and
    /// treats every provided byte offset as an [`LineBreak::Allowed`]
    /// opportunity. When `None`, break opportunities are computed (and cached)
    /// by the built-in [`LineBreaker`].
    #[allow(clippy::too_many_arguments)]
    fn layout_impl(
        &mut self,
        source_text: &str,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
        alignment: TextAlignment,
        font_metrics: Option<&FontVerticalMetrics>,
        strategy: BreakingStrategy,
        external_breaks: Option<&[usize]>,
    ) -> Result<LayoutResult, OxiTextError> {
        let model = VerticalLineModel::from_metrics(font_metrics, constraints.font_size);
        let bidi_levels: Option<Vec<unicode_bidi::Level>> =
            if crate::reorder::needs_bidi(source_text) {
                Some(
                    crate::bidi::BidiParagraph::new(source_text, None)
                        .levels()
                        .to_vec(),
                )
            } else {
                None
            };
        let ext_sorted: Option<Vec<usize>> = external_breaks.map(|bp| {
            let mut v = bp.to_vec();
            v.sort_unstable();
            v
        });
        if ext_sorted.is_none() && source_text != self.break_cache_text {
            let breaker = LineBreaker::new(source_text);
            let mut ops = breaker.breaks().to_vec();

            // Merge soft-hyphen break opportunities (U+00AD).
            // `soft_hyphen_breaks` returns "after" offsets matching the
            // same convention used by unicode-linebreak and LineBreaker.
            let soft = crate::hyphenation::soft_hyphen_breaks(source_text);
            for off in soft {
                ops.push((off, LineBreak::Allowed));
            }

            // Sort by offset; deduplicate with Mandatory winning over Allowed
            // at the same position.
            ops.sort_unstable_by_key(|(off, _)| *off);
            ops.dedup_by(|later, earlier| {
                if later.0 == earlier.0 {
                    if later.1 == LineBreak::Mandatory {
                        earlier.1 = LineBreak::Mandatory;
                    }
                    true
                } else {
                    false
                }
            });

            self.break_cache_ops = ops;
            self.break_cache_text = source_text.to_owned();
        }
        struct FlatGlyph<'a> {
            g: &'a ShapedGlyph,
            font: &'a Arc<[u8]>,
        }
        let mut flat: Vec<FlatGlyph<'_>> = Vec::new();
        for run in runs {
            for g in &run.glyphs {
                flat.push(FlatGlyph {
                    g,
                    font: &run.font_data,
                });
            }
        }
        let wrap = constraints.max_width > 0.0;
        let max_w = constraints.max_width;
        let mut line_ranges: Vec<(usize, usize)> = Vec::new();
        let mut overflow = false;
        let use_kp = strategy == BreakingStrategy::KnuthPlass && wrap && ext_sorted.is_none();
        let mut kp_succeeded = false;
        if use_kp {
            let breaks = &self.break_cache_ops;
            let flat_advances: Vec<f32> = flat.iter().map(|fg| fg.g.x_advance).collect();
            let flat_is_ws: Vec<bool> = flat.iter().map(|fg| fg.g.is_whitespace).collect();
            let mut byte_to_glyph_idx: std::collections::HashMap<usize, usize> =
                std::collections::HashMap::new();
            for (i, fg) in flat.iter().enumerate() {
                byte_to_glyph_idx.entry(fg.g.cluster as usize).or_insert(i);
            }
            let break_opps: Vec<(usize, LineBreak)> = breaks
                .iter()
                .filter_map(|(off, kind)| byte_to_glyph_idx.get(off).map(|&gi| (gi, kind.clone())))
                .collect();
            let kp_breaks =
                crate::knuth_plass::optimal_breaks(&flat_advances, &flat_is_ws, &break_opps, max_w);
            if !kp_breaks.is_empty() || flat.is_empty() {
                build_ranges_from_kp_breaks(&kp_breaks, flat.len(), &mut line_ranges);
                kp_succeeded = true;
            }
        }
        if !kp_succeeded {
            // Helper: return the Unicode code-point that ends immediately
            // before byte offset `off` (i.e. the last char of `source_text[..off]`).
            let char_preceding = |off: usize| -> Option<char> {
                if off == 0 {
                    return None;
                }
                for back in 1..=4usize {
                    if back > off {
                        break;
                    }
                    let start = off - back;
                    if source_text.is_char_boundary(start) {
                        return source_text[start..off].chars().next_back();
                    }
                }
                None
            };
            let break_at_fn = |off: usize| -> Option<LineBreak> {
                if let Some(ref sorted) = ext_sorted {
                    // Mandatory breaks: newline characters always force a new line.
                    // The external break list does not carry mandatory/allowed
                    // distinction, so we infer it from the preceding character.
                    if matches!(
                        char_preceding(off),
                        Some('\n')
                            | Some('\r')
                            | Some('\u{000C}')
                            | Some('\u{0085}')
                            | Some('\u{2028}')
                            | Some('\u{2029}')
                    ) {
                        return Some(LineBreak::Mandatory);
                    }
                    if sorted.binary_search(&off).is_ok() {
                        return Some(LineBreak::Allowed);
                    }
                    return None;
                }
                self.break_cache_ops
                    .iter()
                    .find(|(pos, _)| *pos == off)
                    .map(|(_, kind)| kind.clone())
            };
            let char_at =
                |byte_off: usize| -> Option<char> { source_text.get(byte_off..)?.chars().next() };
            let mut line_start = 0usize;
            let mut cursor = 0.0f32;
            let mut last_safe_break: Option<usize> = None;
            let mut width_at_break = 0.0f32;
            let mut i = 0usize;
            while i < flat.len() {
                let adv = flat[i].g.x_advance;
                let cluster_off = flat[i].g.cluster as usize;
                if i > line_start {
                    let current_char = char_at(cluster_off);
                    let preceding_char = char_preceding(cluster_off);
                    let zwj_precedes = preceding_char == Some('\u{200D}');
                    let is_zwnj = current_char == Some('\u{200C}');
                    let effective_break: Option<LineBreak> = if zwj_precedes {
                        None
                    } else if is_zwnj {
                        Some(LineBreak::Allowed)
                    } else {
                        break_at_fn(cluster_off)
                    };
                    if let Some(kind) = effective_break {
                        if kind == LineBreak::Mandatory {
                            line_ranges.push((line_start, i));
                            line_start = i;
                            cursor = 0.0;
                            last_safe_break = None;
                            width_at_break = 0.0;
                            continue;
                        } else {
                            last_safe_break = Some(i);
                            width_at_break = cursor;
                        }
                    }
                }
                if wrap && cursor + adv > max_w && i > line_start {
                    if let Some(brk) = last_safe_break {
                        if brk > line_start {
                            line_ranges.push((line_start, brk));
                            line_start = brk;
                            cursor -= width_at_break;
                            last_safe_break = None;
                            width_at_break = 0.0;
                            continue;
                        }
                    }
                    overflow = true;
                    line_ranges.push((line_start, i));
                    line_start = i;
                    cursor = 0.0;
                    last_safe_break = None;
                    width_at_break = 0.0;
                    continue;
                }
                cursor += adv;
                i += 1;
            }
            if line_start < flat.len() {
                line_ranges.push((line_start, flat.len()));
            } else if line_ranges.is_empty() {
                line_ranges.push((0, 0));
            }
        }
        if line_ranges.is_empty() {
            line_ranges.push((0, 0));
        }
        self.scratch.clear();
        /// Per-line alignment metadata collected during Phase 1.
        struct LineAlignMeta {
            glyph_start: usize,
            glyph_end: usize,
            x_offset: f32,
            trimmed_width: f32,
            baseline_y: f32,
        }
        let last_line_idx = line_ranges.len().saturating_sub(1);
        let mut line_metas: Vec<LineAlignMeta> = Vec::with_capacity(line_ranges.len());
        let mut total_width = 0.0f32;
        let mut baseline_y = model.ascent;
        let is_justify = alignment == TextAlignment::Justify;
        for (li, &(start, end)) in line_ranges.iter().enumerate() {
            let mut trimmed_width = 0.0f32;
            {
                let mut running = 0.0f32;
                for fg in &flat[start..end] {
                    running += fg.g.x_advance;
                    if !fg.g.is_whitespace {
                        trimmed_width = running;
                    }
                }
            }
            let ws_gaps = count_internal_ws_gaps(flat[start..end].iter().map(|fg| fg.g));
            let (x_offset, justify_extra) = compute_alignment(
                alignment,
                trimmed_width,
                max_w,
                wrap,
                li == last_line_idx,
                ws_gaps,
            );
            let glyph_start = self.scratch.len();
            let pen_start = if is_justify { x_offset } else { 0.0 };
            let mut pen = pen_start;
            match &bidi_levels {
                Some(levels) => {
                    let line_levels: Vec<unicode_bidi::Level> = flat[start..end]
                        .iter()
                        .map(|fg| {
                            let idx = fg.g.cluster as usize;
                            levels
                                .get(idx)
                                .copied()
                                .unwrap_or_else(unicode_bidi::Level::ltr)
                        })
                        .collect();
                    let visual_order = crate::reorder::line_visual_order(&line_levels);
                    for vi in &visual_order {
                        let fg = &flat[start + vi];
                        let adv = fg.g.x_advance
                            + if justify_extra > 0.0 && fg.g.is_whitespace {
                                justify_extra
                            } else {
                                0.0
                            };
                        self.scratch.push(PositionedGlyph {
                            gid: fg.g.gid,
                            font_data: Arc::clone(fg.font),
                            pos: (pen + fg.g.x_offset, baseline_y + fg.g.y_offset),
                            font_size: constraints.font_size,
                            advance_x: adv,
                            cluster: fg.g.cluster,
                        });
                        pen += adv;
                    }
                }
                None => {
                    for fg in &flat[start..end] {
                        let adv = fg.g.x_advance
                            + if justify_extra > 0.0 && fg.g.is_whitespace {
                                justify_extra
                            } else {
                                0.0
                            };
                        self.scratch.push(PositionedGlyph {
                            gid: fg.g.gid,
                            font_data: Arc::clone(fg.font),
                            pos: (pen + fg.g.x_offset, baseline_y + fg.g.y_offset),
                            font_size: constraints.font_size,
                            advance_x: adv,
                            cluster: fg.g.cluster,
                        });
                        pen += adv;
                    }
                }
            }
            total_width = total_width.max(trimmed_width);
            line_metas.push(LineAlignMeta {
                glyph_start,
                glyph_end: self.scratch.len(),
                x_offset: if is_justify { 0.0 } else { x_offset },
                trimmed_width,
                baseline_y,
            });
            baseline_y += model.line_height;
        }
        if !is_justify {
            let glyphs_slice = self.scratch.as_mut_slice();
            let mut per_line: Vec<(f32, &mut [PositionedGlyph])> =
                Vec::with_capacity(line_metas.len());
            let mut remaining: &mut [PositionedGlyph] = glyphs_slice;
            let mut consumed = 0usize;
            for meta in &line_metas {
                let line_len = meta.glyph_end - meta.glyph_start;
                let (line_slice, rest) = remaining.split_at_mut(line_len);
                per_line.push((meta.x_offset, line_slice));
                remaining = rest;
                consumed += line_len;
            }
            let _ = consumed;
            #[cfg(not(target_arch = "wasm32"))]
            per_line.par_iter_mut().for_each(|(x_off, line_glyphs)| {
                if *x_off != 0.0 {
                    for g in line_glyphs.iter_mut() {
                        g.pos.0 += *x_off;
                    }
                }
            });
            #[cfg(target_arch = "wasm32")]
            for (x_off, line_glyphs) in per_line.iter_mut() {
                if *x_off != 0.0 {
                    for g in line_glyphs.iter_mut() {
                        g.pos.0 += *x_off;
                    }
                }
            }
        }
        let mut lines: Vec<Line> = Vec::with_capacity(line_metas.len());
        for meta in &line_metas {
            lines.push(Line {
                glyph_start: meta.glyph_start,
                glyph_end: meta.glyph_end,
                metrics: LineMetrics {
                    ascent: model.ascent,
                    descent: model.descent,
                    leading: model.leading,
                    baseline_y: meta.baseline_y,
                    width: meta.trimmed_width,
                },
            });
        }
        let total_height = if lines.is_empty() {
            0.0
        } else {
            model.line_height * lines.len() as f32
        };
        let mut glyphs: Vec<PositionedGlyph> = Vec::with_capacity(self.scratch.len());
        glyphs.append(&mut self.scratch);
        Ok(LayoutResult {
            glyphs,
            lines,
            metrics: ParagraphMetrics {
                total_height,
                total_width,
                line_count: line_ranges.len(),
                overflow,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        })
    }
    /// Lays out `runs` in vertical top-to-bottom flow.
    ///
    /// Each glyph advances the cursor downward by its vertical advance (falling
    /// back to `font_size` when no `vmtx` data is available).  When
    /// `max_column_height > 0.0`, the text wraps into additional columns once
    /// the current column's height would be exceeded; each column advances the
    /// `x` origin by `font_size * 1.2`.
    ///
    /// A "line" in this context is one vertical *column* of glyphs.  The
    /// returned [`Line`] structs therefore index into the column-by-column
    /// glyph list, and [`ParagraphMetrics::line_count`] equals the number of
    /// columns used.
    ///
    /// Note: bidi reordering is **not** applied in vertical mode; vertical CJK
    /// text is always read top-to-bottom in column order.
    ///
    /// # Errors
    /// Currently infallible for well-formed input; returns `Err` only for
    /// forward compatibility.
    pub fn layout_vertical(
        &mut self,
        _source_text: &str,
        runs: &[ShapedRun],
        max_column_height: f32,
        font_size: f32,
        _font_metrics: Option<&FontVerticalMetrics>,
    ) -> Result<LayoutResult, OxiTextError> {
        struct FlatGlyph<'a> {
            g: &'a ShapedGlyph,
            font: &'a Arc<[u8]>,
        }
        let mut flat: Vec<FlatGlyph<'_>> = Vec::new();
        for run in runs {
            for g in &run.glyphs {
                flat.push(FlatGlyph {
                    g,
                    font: &run.font_data,
                });
            }
        }
        let column_width = font_size * 1.2;
        let mut glyphs: Vec<PositionedGlyph> = Vec::with_capacity(flat.len());
        let mut lines: Vec<Line> = Vec::new();
        let mut column_x = 0.0f32;
        let mut cursor_y = 0.0f32;
        let mut col_glyph_start = 0usize;
        let mut max_y_in_column = 0.0f32;
        let mut max_total_y = 0.0f32;
        // Cache parsed ttf_parser::Face instances keyed by byte-slice pointer so
        // each unique font face is parsed exactly once across the entire glyph loop
        // instead of once per glyph.
        let mut face_cache = crate::vertical::ParsedFaceCache::new();
        for fg in &flat {
            let v_adv = face_cache.vmtx_advance_or_default(fg.font.as_ref(), fg.g.gid, font_size);
            if max_column_height > 0.0 && cursor_y + v_adv > max_column_height && cursor_y > 0.0 {
                let metrics = LineMetrics {
                    ascent: font_size * 0.8,
                    descent: font_size * 0.2,
                    leading: 0.0,
                    baseline_y: column_x,
                    width: max_y_in_column,
                };
                lines.push(Line {
                    glyph_start: col_glyph_start,
                    glyph_end: glyphs.len(),
                    metrics,
                });
                max_total_y = max_total_y.max(cursor_y);
                column_x += column_width;
                cursor_y = 0.0;
                max_y_in_column = 0.0;
                col_glyph_start = glyphs.len();
            }
            glyphs.push(PositionedGlyph {
                gid: fg.g.gid,
                font_data: Arc::clone(fg.font),
                pos: (column_x + fg.g.x_offset, cursor_y + fg.g.y_offset),
                font_size,
                advance_x: fg.g.x_advance,
                cluster: fg.g.cluster,
            });
            cursor_y += v_adv;
            max_y_in_column = max_y_in_column.max(cursor_y);
        }
        {
            let metrics = LineMetrics {
                ascent: font_size * 0.8,
                descent: font_size * 0.2,
                leading: 0.0,
                baseline_y: column_x,
                width: max_y_in_column,
            };
            lines.push(Line {
                glyph_start: col_glyph_start,
                glyph_end: glyphs.len(),
                metrics,
            });
            max_total_y = max_total_y.max(cursor_y);
        }
        if lines.is_empty() {
            lines.push(Line {
                glyph_start: 0,
                glyph_end: 0,
                metrics: LineMetrics {
                    ascent: font_size * 0.8,
                    descent: font_size * 0.2,
                    leading: 0.0,
                    baseline_y: 0.0,
                    width: 0.0,
                },
            });
        }
        let num_columns = lines.len();
        let total_width = num_columns as f32 * column_width;
        let total_height = max_total_y;
        Ok(LayoutResult {
            glyphs,
            lines,
            metrics: ParagraphMetrics {
                total_height,
                total_width,
                line_count: num_columns,
                overflow: false,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        })
    }
    /// Lays out multiple paragraphs stacked vertically.
    ///
    /// Each paragraph is laid out independently using [`LayoutEngine::layout`]
    /// (greedy algorithm).  The y-positions of each paragraph's glyphs and line
    /// baselines are offset by the accumulated height of all previous paragraphs
    /// plus `para_spacing` between them.
    ///
    /// The returned [`LayoutResult`] has all glyphs and lines merged into a
    /// single flat list.  [`ParagraphMetrics`] reflects the combined extent.
    ///
    /// # Errors
    /// Propagates any error returned by the inner [`LayoutEngine::layout`]
    /// calls.
    pub fn layout_paragraphs(
        &mut self,
        paragraphs: &[&str],
        shaped_runs_per_paragraph: &[&[ShapedRun]],
        constraints: &LayoutConstraints,
        para_spacing: f32,
        options: &crate::options::LayoutOptions,
        font_metrics: Option<&FontVerticalMetrics>,
    ) -> Result<LayoutResult, OxiTextError> {
        let alignment = options.alignment;
        let mut combined_glyphs: Vec<PositionedGlyph> = Vec::new();
        let mut combined_lines: Vec<Line> = Vec::new();
        let mut cursor_y = 0.0f32;
        let mut total_width = 0.0f32;
        let mut overflow = false;
        let mut para_count = 0usize;
        let n = paragraphs.len().min(shaped_runs_per_paragraph.len());
        for idx in 0..n {
            let text = paragraphs[idx];
            let runs = shaped_runs_per_paragraph[idx];
            let result = self.layout(text, runs, constraints, alignment, font_metrics)?;
            let glyph_offset = combined_glyphs.len();
            for g in &result.glyphs {
                combined_glyphs.push(PositionedGlyph {
                    gid: g.gid,
                    font_data: std::sync::Arc::clone(&g.font_data),
                    pos: (g.pos.0, g.pos.1 + cursor_y),
                    font_size: g.font_size,
                    advance_x: g.advance_x,
                    cluster: g.cluster,
                });
            }
            for line in &result.lines {
                combined_lines.push(Line {
                    glyph_start: line.glyph_start + glyph_offset,
                    glyph_end: line.glyph_end + glyph_offset,
                    metrics: LineMetrics {
                        ascent: line.metrics.ascent,
                        descent: line.metrics.descent,
                        leading: line.metrics.leading,
                        baseline_y: line.metrics.baseline_y + cursor_y,
                        width: line.metrics.width,
                    },
                });
            }
            total_width = total_width.max(result.metrics.total_width);
            overflow |= result.metrics.overflow;
            para_count += result.metrics.line_count;
            cursor_y += result.metrics.total_height;
            if idx + 1 < n {
                cursor_y += para_spacing;
            }
        }
        let total_height = cursor_y;
        Ok(LayoutResult {
            glyphs: combined_glyphs,
            lines: combined_lines,
            metrics: ParagraphMetrics {
                total_height,
                total_width,
                line_count: para_count,
                overflow,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        })
    }
    /// Lays out a single text block using comprehensive [`crate::options::LayoutOptions`].
    ///
    /// This is a unified entry point that dispatches to the appropriate layout
    /// path based on [`crate::options::LayoutOptions::flow_direction`] and applies optional
    /// post-processing (truncation).
    ///
    /// Tab stop handling for `\t` characters: when a glyph's cluster character
    /// is `\t`, the cursor advances to the next tab stop instead of using the
    /// glyph's natural advance.  The positioned glyph's x is placed at the
    /// pre-tab cursor position (the whitespace gap itself is empty).
    ///
    /// # Errors
    /// Propagates any error returned by the inner layout calls.
    pub fn layout_with_options(
        &mut self,
        source_text: &str,
        shaped_runs: &[ShapedRun],
        max_width: f32,
        options: &crate::options::LayoutOptions,
        font_metrics: Option<&FontVerticalMetrics>,
        font_size: f32,
    ) -> Result<LayoutResult, OxiTextError> {
        use oxitext_core::FlowDirection;
        let constraints = LayoutConstraints {
            max_width,
            font_size,
        };
        let mut result = match options.flow_direction {
            FlowDirection::Vertical => {
                self.layout_vertical(source_text, shaped_runs, max_width, font_size, font_metrics)?
            }
            FlowDirection::Horizontal => self.layout(
                source_text,
                shaped_runs,
                &constraints,
                options.alignment,
                font_metrics,
            )?,
        };
        let tab_stops = &options.tab_stops;
        if !source_text.is_empty() {
            for line in &result.lines {
                let gs = line.glyph_start;
                let ge = line.glyph_end;
                if gs >= ge {
                    continue;
                }
                let mut pen = result.glyphs[gs].pos.0;
                for gi in gs..ge {
                    let glyph_pos = result.glyphs[gi].pos;
                    // Resolve the source character straight from the positioned
                    // glyph's own `cluster` byte offset. This is correct for
                    // both LTR and bidi/RTL text: `result.glyphs` may be emitted
                    // in UAX#9 L2 *visual* order (see `layout_impl`), so the flat
                    // shaped-run walk that the old `find_cluster_for_positioned_glyph`
                    // performed — which is in *logical* order — read an unrelated
                    // character for any line mixing RTL runs with a TAB. The
                    // per-glyph `cluster` field is populated from the same source
                    // glyph regardless of visual reordering, so it is authoritative.
                    let cluster_off = result.glyphs[gi].cluster as usize;
                    let char_at_cluster: Option<char> = source_text
                        .get(cluster_off..)
                        .and_then(|s| s.chars().next());
                    if char_at_cluster == Some('\t') {
                        let snap = tab_stops.next_stop(pen);
                        result.glyphs[gi].pos = (pen, glyph_pos.1);
                        pen = snap;
                    } else {
                        let next_x = if gi + 1 < ge {
                            result.glyphs[gi + 1].pos.0
                        } else {
                            // Use the positioned glyph's own advance, which — unlike
                            // the raw shaped advance the old `advance_for_glyph`
                            // returned — already folds in the justification extra
                            // added during line building.
                            glyph_pos.0 + result.glyphs[gi].advance_x
                        };
                        pen = next_x;
                    }
                }
            }
        }
        if let Some(trunc) = &options.truncation {
            result = apply_truncation(result, trunc);
        }
        if options.hanging_punctuation {
            apply_hanging_punctuation(&mut result, source_text);
        }
        if let Some(decoration) = options.decoration {
            result.decorations = super::functions::compute_decoration_rects(
                &result.lines,
                &result.glyphs,
                decoration,
            );
        }
        // Append inline objects at the end of the last line.
        if !options.inline_objects.is_empty() {
            let last_line_y = result
                .lines
                .last()
                .map(|l| l.metrics.baseline_y)
                .unwrap_or(0.0);
            let mut cursor_x = result
                .glyphs
                .last()
                .map(|g| g.pos.0 + g.advance_x)
                .unwrap_or(0.0);
            let last_line_idx = result.lines.len().saturating_sub(1);
            for obj in &options.inline_objects {
                result
                    .inline_objects
                    .push(oxitext_core::PositionedInlineObject {
                        object: obj.clone(),
                        x: cursor_x,
                        y: last_line_y,
                        line: last_line_idx,
                    });
                cursor_x += obj.advance;
            }
        }
        Ok(result)
    }
}
/// Aggregate metrics for a whole laid-out block of text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphMetrics {
    /// Total height of all lines stacked vertically, in pixels.
    pub total_height: f32,
    /// Width of the widest line, in pixels.
    pub total_width: f32,
    /// Number of lines produced.
    pub line_count: usize,
    /// `true` if any line's natural width exceeded `max_width` and could not
    /// be broken (e.g. a single unbreakable token wider than the column).
    pub overflow: bool,
    /// `true` if the last line was truncated with an ellipsis because it
    /// exceeded `TruncationMode::max_width`.
    pub truncated: bool,
}
/// Vertical metrics for a single laid-out line, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    /// Distance from the line's top to its baseline (positive).
    pub ascent: f32,
    /// Distance from the baseline to the line's bottom (positive).
    pub descent: f32,
    /// Extra leading distributed below the line.
    pub leading: f32,
    /// Absolute Y coordinate of the baseline from the layout origin.
    pub baseline_y: f32,
    /// Total advance width of the line's glyphs (before alignment), in pixels.
    pub width: f32,
}
impl LineMetrics {
    /// Total height consumed by the line (ascent + descent + leading).
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.leading
    }
}
/// A single laid-out line: a contiguous slice of positioned glyphs plus its
/// metrics.
#[derive(Debug, Clone)]
pub struct Line {
    /// Index of the first glyph of this line in [`LayoutResult::glyphs`].
    pub glyph_start: usize,
    /// Index past the last glyph of this line in [`LayoutResult::glyphs`].
    pub glyph_end: usize,
    /// Vertical and width metrics for the line.
    pub metrics: LineMetrics,
}
impl Line {
    /// Number of glyphs in the line.
    pub fn len(&self) -> usize {
        self.glyph_end - self.glyph_start
    }
    /// Returns `true` if the line has no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyph_start == self.glyph_end
    }
}
