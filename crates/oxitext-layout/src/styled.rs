//! Multi-style layout: baseline-aligned layout across [`StyledRun`]s.
//!
//! [`LayoutEngine::layout_styled_runs`] accepts a slice of pre-shaped
//! [`StyledRun`]s each with its own font metrics and pixel size, and positions
//! them so that all glyphs on the same line share a common baseline.  The
//! common baseline is determined by the maximum ascender among all runs
//! contributing to that line (UAX #14-aware greedy line-breaking is applied
//! first).

use crate::{
    engine::{LayoutEngine, LayoutResult, Line, LineMetrics, ParagraphMetrics},
    linebreak::{LineBreak, LineBreaker},
    options::LayoutOptions,
};
use oxitext_core::{FontVerticalMetrics, PositionedGlyph, Rgba8, ShapedGlyph, VerticalPosition};
use std::sync::Arc;

/// A pre-shaped run of glyphs with its own font metrics and pixel size.
///
/// Used for mixed-font/size layouts where each run may have a different face.
/// Each `StyledRun` carries a shared reference to the font bytes so that the
/// rasteriser downstream can look up the correct face without re-parsing.
pub struct StyledRun {
    /// Pre-shaped glyphs for this run.
    pub glyphs: Vec<ShapedGlyph>,
    /// Font vertical metrics for this run's face.
    pub metrics: FontVerticalMetrics,
    /// Font size in pixels for this run.
    pub px_size: f32,
    /// Text colour (RGBA). Stored for downstream use; layout does not render.
    pub color: Rgba8,
    /// Raw font bytes backing the glyphs in this run.
    pub font_data: Arc<[u8]>,
    /// Vertical positioning for subscript/superscript effects.
    /// Defaults to [`VerticalPosition::Normal`].
    pub vertical_position: VerticalPosition,
}

/// Per-glyph bookkeeping during multi-style layout.
struct FlatEntry<'a> {
    glyph: &'a ShapedGlyph,
    font_data: &'a Arc<[u8]>,
    run_ascent: f32,
    run_descent: f32,
    px_size: f32,
    vertical_position: VerticalPosition,
}

impl LayoutEngine {
    /// Lays out a slice of pre-shaped [`StyledRun`]s with baseline alignment.
    ///
    /// # Algorithm
    ///
    /// 1. Flatten all glyphs from all runs into a single sequence, tagging each
    ///    with its run's vertical metrics and pixel size.
    /// 2. Compute UAX #14 line-break opportunities from a synthetic "cluster
    ///    string" built from the cluster byte offsets; use greedy wrapping
    ///    against `max_width`.
    /// 3. Group glyphs into lines.
    /// 4. For each line compute:
    ///    - `line_ascender = max(run.ascent_px)` across all runs on the line.
    ///    - `line_descender = max(run.descent_px)` across all runs on the line.
    /// 5. Assign `baseline_y = cursor_y + line_ascender`.  Each glyph's `y` is
    ///    `baseline_y + (line_ascender − run_ascender)`, lifting shorter-ascender
    ///    glyphs up so their own ascender aligns with the tallest.
    /// 6. Advance `cursor_y += line_ascender + line_descender + line_gap +
    ///    options.paragraph_spacing`.
    ///
    /// # Parameters
    /// - `runs` – pre-shaped runs in logical order.
    /// - `source_text` – the original string the runs were shaped from (required
    ///   for UAX #14 linebreak analysis; cluster offsets in each
    ///   [`ShapedGlyph`] index into this string).
    /// - `max_width` – maximum line width in pixels; `0.0` disables wrapping.
    /// - `options` – paragraph options (paragraph_spacing is used as extra
    ///   leading after each line).
    ///
    /// # Errors
    /// Currently infallible for well-formed input; returns `Err` only for
    /// forward compatibility.
    pub fn layout_styled_runs(
        &mut self,
        runs: &[StyledRun],
        source_text: &str,
        max_width: f32,
        options: &LayoutOptions,
    ) -> Result<LayoutResult, oxitext_core::OxiTextError> {
        // ------------------------------------------------------------------
        // Step 1: Flatten all glyphs from all runs, recording per-run metrics.
        // ------------------------------------------------------------------
        let mut flat: Vec<FlatEntry<'_>> = Vec::new();
        for run in runs {
            let run_ascent = run.metrics.ascent_px(run.px_size);
            let run_descent = run.metrics.descent_px(run.px_size);
            for g in &run.glyphs {
                flat.push(FlatEntry {
                    glyph: g,
                    font_data: &run.font_data,
                    run_ascent,
                    run_descent,
                    px_size: run.px_size,
                    vertical_position: run.vertical_position,
                });
            }
        }

        // ------------------------------------------------------------------
        // Step 2: Compute break opportunities (reuse engine cache if text
        // is identical to the last call).
        // ------------------------------------------------------------------
        if source_text != self.break_cache_text {
            let b = LineBreaker::new(source_text).breaks().to_vec();
            self.break_cache_text = source_text.to_owned();
            self.break_cache_ops = b;
        }
        let breaks = &self.break_cache_ops;

        let break_at = |off: usize| -> Option<LineBreak> {
            breaks
                .iter()
                .find(|(pos, _)| *pos == off)
                .map(|(_, kind)| kind.clone())
        };

        // ------------------------------------------------------------------
        // Step 3: Greedy line-breaking to produce (start, end) glyph ranges.
        // ------------------------------------------------------------------
        let wrap = max_width > 0.0;
        let mut line_ranges: Vec<(usize, usize)> = Vec::new();
        let mut line_start = 0usize;
        let mut cursor_x = 0.0f32;
        let mut last_safe: Option<usize> = None;
        let mut width_at_break = 0.0f32;

        let mut i = 0usize;
        while i < flat.len() {
            let adv = flat[i].glyph.x_advance;
            let cluster_off = flat[i].glyph.cluster as usize;

            if i > line_start {
                // Determine effective break class.
                let current_char = source_text
                    .get(cluster_off..)
                    .and_then(|s| s.chars().next());
                let preceding_char = if cluster_off == 0 {
                    None
                } else {
                    (1..=4usize).find_map(|back| {
                        let start = cluster_off.checked_sub(back)?;
                        if source_text.is_char_boundary(start) {
                            source_text[start..cluster_off].chars().next_back()
                        } else {
                            None
                        }
                    })
                };
                let zwj_precedes = preceding_char == Some('\u{200D}');
                let is_zwnj = current_char == Some('\u{200C}');

                let effective_break: Option<LineBreak> = if zwj_precedes {
                    None
                } else if is_zwnj {
                    Some(LineBreak::Allowed)
                } else {
                    break_at(cluster_off)
                };

                if let Some(kind) = effective_break {
                    if kind == LineBreak::Mandatory {
                        line_ranges.push((line_start, i));
                        line_start = i;
                        cursor_x = 0.0;
                        last_safe = None;
                        width_at_break = 0.0;
                        continue;
                    } else {
                        last_safe = Some(i);
                        width_at_break = cursor_x;
                    }
                }
            }

            if wrap && cursor_x + adv > max_width && i > line_start {
                if let Some(brk) = last_safe {
                    if brk > line_start {
                        line_ranges.push((line_start, brk));
                        line_start = brk;
                        cursor_x -= width_at_break;
                        last_safe = None;
                        width_at_break = 0.0;
                        continue;
                    }
                }
                // Hard-break fallback.
                line_ranges.push((line_start, i));
                line_start = i;
                cursor_x = 0.0;
                last_safe = None;
                width_at_break = 0.0;
                continue;
            }

            cursor_x += adv;
            i += 1;
        }
        if line_start < flat.len() {
            line_ranges.push((line_start, flat.len()));
        } else if line_ranges.is_empty() {
            line_ranges.push((0, 0));
        }
        if line_ranges.is_empty() {
            line_ranges.push((0, 0));
        }

        // ------------------------------------------------------------------
        // Step 4-6: Position glyphs with baseline alignment per line.
        // ------------------------------------------------------------------
        let mut positioned: Vec<PositionedGlyph> = Vec::with_capacity(flat.len());
        let mut lines: Vec<Line> = Vec::with_capacity(line_ranges.len());
        let mut cursor_y = 0.0f32;
        let mut total_width = 0.0f32;

        for &(start, end) in &line_ranges {
            let line_slice = &flat[start..end];

            // Compute line-wide max ascender and descender.
            let line_ascender = line_slice
                .iter()
                .map(|fe| fe.run_ascent)
                .fold(0.0f32, f32::max);
            let line_descender = line_slice
                .iter()
                .map(|fe| fe.run_descent)
                .fold(0.0f32, f32::max);
            let line_gap = if line_slice.is_empty() {
                0.0
            } else {
                // Take the max line-gap across runs; use first run's metrics for gap.
                runs.iter()
                    .map(|r| r.metrics.line_gap_px(r.px_size))
                    .fold(0.0f32, f32::max)
            };

            let baseline_y = cursor_y + line_ascender;
            let glyph_start = positioned.len();
            let mut pen_x = 0.0f32;
            let mut line_width = 0.0f32;

            for fe in line_slice {
                // Shift glyph up/down so its own ascender aligns with line ascender.
                let y_shift = line_ascender - fe.run_ascent;
                // Apply sub/superscript baseline adjustment (positive = upward, so subtract from y).
                let vp_adjust = fe.vertical_position.baseline_adjustment(fe.px_size);
                let glyph_y = baseline_y + y_shift + fe.glyph.y_offset - vp_adjust;
                let effective_font_size = fe.vertical_position.effective_size(fe.px_size);

                positioned.push(PositionedGlyph {
                    gid: fe.glyph.gid,
                    font_data: Arc::clone(fe.font_data),
                    pos: (pen_x + fe.glyph.x_offset, glyph_y),
                    font_size: effective_font_size,
                    advance_x: fe.glyph.x_advance,
                    cluster: fe.glyph.cluster,
                });

                pen_x += fe.glyph.x_advance;
                if !fe.glyph.is_whitespace {
                    line_width = pen_x;
                }
            }

            total_width = total_width.max(line_width);

            lines.push(Line {
                glyph_start,
                glyph_end: positioned.len(),
                metrics: LineMetrics {
                    ascent: line_ascender,
                    descent: line_descender,
                    leading: line_gap,
                    baseline_y,
                    width: line_width,
                },
            });

            cursor_y += line_ascender + line_descender + line_gap + options.paragraph_spacing;
        }

        let total_height = cursor_y;

        Ok(LayoutResult {
            glyphs: positioned,
            lines,
            metrics: ParagraphMetrics {
                total_height,
                total_width,
                line_count: line_ranges.len(),
                overflow: false,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxitext_core::{FontVerticalMetrics, Rgba8, ShapedGlyph, VerticalPosition};
    use std::sync::Arc;

    fn make_shaped_glyph(cluster: u32, x_advance: f32, is_whitespace: bool) -> ShapedGlyph {
        ShapedGlyph {
            gid: 1,
            x_advance,
            y_advance: 0.0,
            x_offset: 0.0,
            y_offset: 0.0,
            cluster,
            is_whitespace,
            unsafe_to_break: false,
        }
    }

    /// Produce font metrics with a specific ascender ratio.
    fn make_metrics(ascender: i16, descender: i16) -> FontVerticalMetrics {
        FontVerticalMetrics {
            units_per_em: 1000,
            ascender,
            descender,
            line_gap: 0,
        }
    }

    #[test]
    fn test_multistyle_baseline_alignment() {
        // Two runs with different sizes but identical normalised metrics.
        // big_run: px_size=20  → ascent = 800/1000 * 20 = 16.0 px, descent = 200/1000 * 20 = 4.0 px
        // small_run: px_size=10 → ascent = 8.0 px, descent = 2.0 px
        // On a shared line: line_ascender = max(16.0, 8.0) = 16.0.
        // big_run glyphs: y_shift = 16.0 - 16.0 = 0.0  → baseline_y + 0
        // small_run glyphs: y_shift = 16.0 - 8.0 = 8.0 → baseline_y + 8.0
        // Both conceptual "tops" land at baseline_y + y_shift - run_ascent = baseline_y - run_ascent:
        //   big:   baseline_y + 0.0 - 16.0 = baseline_y - 16.0
        //   small: baseline_y + 8.0 - 8.0  = baseline_y - 8.0 ← correctly lifts smaller text up
        //
        // More concisely: the *bottom* of each run's ascent zone lines up at baseline_y.

        let big_metrics = make_metrics(800, -200);
        let small_metrics = make_metrics(800, -200);

        let big_run = StyledRun {
            glyphs: vec![
                make_shaped_glyph(0, 20.0, false),
                make_shaped_glyph(1, 20.0, false),
            ],
            metrics: big_metrics,
            px_size: 20.0,
            color: Rgba8::BLACK,
            font_data: Arc::from(&[][..]),
            vertical_position: VerticalPosition::Normal,
        };
        let small_run = StyledRun {
            glyphs: vec![
                make_shaped_glyph(2, 10.0, false),
                make_shaped_glyph(3, 10.0, false),
            ],
            metrics: small_metrics,
            px_size: 10.0,
            color: Rgba8::new(255, 0, 0, 255),
            font_data: Arc::from(&[][..]),
            vertical_position: VerticalPosition::Normal,
        };

        let runs = [big_run, small_run];
        let source_text = "abcd";
        let mut engine = LayoutEngine::new();
        let opts = LayoutOptions::default();
        let result = engine
            .layout_styled_runs(&runs, source_text, 1000.0, &opts)
            .expect("layout_styled_runs");

        // All 4 glyphs should be placed on a single line.
        assert_eq!(result.glyphs.len(), 4);
        assert_eq!(result.lines.len(), 1);

        // Line ascender = 16.0 (big run), line descender = 4.0.
        let line = &result.lines[0];
        assert!(
            (line.metrics.ascent - 16.0).abs() < 1e-3,
            "line ascent should be 16.0, got {}",
            line.metrics.ascent
        );

        // baseline_y = 0 (cursor_y) + 16.0 = 16.0.
        let baseline_y = line.metrics.baseline_y;

        // Big-run glyphs (indices 0,1): y_shift=0, so pos.y = baseline_y.
        for gi in 0..2 {
            assert!(
                (result.glyphs[gi].pos.1 - baseline_y).abs() < 1e-3,
                "big-run glyph {} y should equal baseline_y={}, got {}",
                gi,
                baseline_y,
                result.glyphs[gi].pos.1
            );
        }

        // Small-run glyphs (indices 2,3): ascent=8, y_shift=8, pos.y = baseline_y+8.
        let expected_small_y = baseline_y + 8.0;
        for gi in 2..4 {
            assert!(
                (result.glyphs[gi].pos.1 - expected_small_y).abs() < 1e-3,
                "small-run glyph {} y should be baseline_y+8={}, got {}",
                gi,
                expected_small_y,
                result.glyphs[gi].pos.1
            );
        }
    }

    #[test]
    fn test_multistyle_single_run_no_shift() {
        // A single run should produce y == baseline_y (zero shift).
        let m = make_metrics(800, -200);
        let run = StyledRun {
            glyphs: vec![
                make_shaped_glyph(0, 10.0, false),
                make_shaped_glyph(1, 10.0, false),
                make_shaped_glyph(2, 10.0, false),
            ],
            metrics: m,
            px_size: 16.0,
            color: Rgba8::BLACK,
            font_data: Arc::from(&[][..]),
            vertical_position: VerticalPosition::Normal,
        };

        let source = "abc";
        let mut engine = LayoutEngine::new();
        let opts = LayoutOptions::default();
        let result = engine
            .layout_styled_runs(&[run], source, 1000.0, &opts)
            .expect("layout_styled_runs single");

        let baseline_y = result.lines[0].metrics.baseline_y;
        for g in &result.glyphs {
            assert!(
                (g.pos.1 - baseline_y).abs() < 1e-3,
                "single-run glyph y should equal baseline_y={}, got {}",
                baseline_y,
                g.pos.1
            );
        }
    }

    #[test]
    fn test_multistyle_wraps_at_max_width() {
        // 4 glyphs × 10px; max_width=25 → wraps after 2 glyphs.
        let m = make_metrics(800, -200);
        let run = StyledRun {
            glyphs: (0..4)
                .map(|i| make_shaped_glyph(i as u32, 10.0, false))
                .collect(),
            metrics: m,
            px_size: 12.0,
            color: Rgba8::BLACK,
            font_data: Arc::from(&[][..]),
            vertical_position: VerticalPosition::Normal,
        };

        let source = "abcd";
        let mut engine = LayoutEngine::new();
        let opts = LayoutOptions::default();
        let result = engine
            .layout_styled_runs(&[run], source, 25.0, &opts)
            .expect("layout_styled_runs wrap");

        assert!(result.lines.len() >= 2, "expected wrapping");
        assert_eq!(result.glyphs.len(), 4);
    }

    #[test]
    fn test_multistyle_empty_runs() {
        // Zero runs should produce an empty result with one (empty) line.
        let mut engine = LayoutEngine::new();
        let opts = LayoutOptions::default();
        let result = engine
            .layout_styled_runs(&[], "", 400.0, &opts)
            .expect("layout_styled_runs empty");
        assert_eq!(result.glyphs.len(), 0);
        assert_eq!(result.lines.len(), 1);
    }
}
