//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use oxitext_core::{
    DecorationRect, PositionedGlyph, ShapedGlyph, ShapedRun, TextAlignment, TextDecoration,
};
use std::sync::Arc;

use super::types::{LayoutResult, Line};

/// Compute per-line [`DecorationRect`]s from a [`TextDecoration`] and layout
/// data.
///
/// For each non-empty line the function derives the x-span from the first and
/// last glyph positions and applies the decoration relative to the line's
/// baseline.  Empty lines produce no output.
pub(super) fn compute_decoration_rects(
    lines: &[Line],
    glyphs: &[PositionedGlyph],
    decoration: TextDecoration,
) -> Vec<DecorationRect> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let gs = line.glyph_start;
        let ge = line.glyph_end.min(glyphs.len());
        if gs >= ge {
            continue;
        }
        let x_start = glyphs[gs].pos.0;
        let last = &glyphs[ge - 1];
        let x_end = last.pos.0 + last.advance_x;
        let width = (x_end - x_start).max(0.0);
        if width == 0.0 {
            continue;
        }
        let baseline_y = line.metrics.baseline_y;
        let ascent = line.metrics.ascent;
        let rect = match decoration {
            TextDecoration::Underline {
                color,
                thickness,
                offset,
            } => DecorationRect {
                x: x_start,
                y: baseline_y + offset,
                width,
                height: thickness,
                color,
            },
            TextDecoration::Overline {
                color,
                thickness,
                offset,
            } => DecorationRect {
                x: x_start,
                y: baseline_y - ascent - offset,
                width,
                height: thickness,
                color,
            },
            TextDecoration::Strikethrough { color, thickness } => DecorationRect {
                x: x_start,
                y: baseline_y - ascent * 0.5,
                width,
                height: thickness,
                color,
            },
        };
        out.push(rect);
    }
    out
}

/// Returns `true` if `c` is a CJK fullwidth punctuation character that should
/// be allowed to hang into the margin.
///
/// Covers the most common CJK sentence-ending, clause-separating, and quoting
/// punctuation per CSS Text Module Level 3 §3 "Hanging Punctuation" and JIS X
/// 4051 §4.2.
pub(super) fn is_hanging_punctuation(c: char) -> bool {
    matches!(
        c,
        '\u{3001}'
            | '\u{3002}'
            | '\u{FF01}'
            | '\u{FF02}'
            | '\u{FF0C}'
            | '\u{FF0E}'
            | '\u{FF1A}'
            | '\u{FF1B}'
            | '\u{FF1F}'
    )
}
/// Apply hanging-punctuation post-processing to a laid-out result.
///
/// For each line:
/// - If the last (rightmost) glyph is a hanging-punctuation character, shift
///   it rightward by half its advance so it overhangs into the right margin.
/// - If the first (leftmost) glyph is a hanging-punctuation character, shift
///   it leftward by half its advance so it overhangs into the left margin.
///
/// The `source_text` slice is used to identify codepoints from glyph cluster
/// byte offsets.
pub(super) fn apply_hanging_punctuation(result: &mut LayoutResult, source_text: &str) {
    for line in &result.lines {
        let gs = line.glyph_start;
        let ge = line.glyph_end;
        if gs >= ge {
            continue;
        }
        let last_gi = ge - 1;
        {
            let cluster_off = result.glyphs[last_gi].cluster as usize;
            let ch = source_text
                .get(cluster_off..)
                .and_then(|s| s.chars().next());
            if let Some(c) = ch {
                if is_hanging_punctuation(c) {
                    let half_adv = result.glyphs[last_gi].advance_x * 0.5;
                    result.glyphs[last_gi].pos.0 += half_adv;
                }
            }
        }
        {
            let cluster_off = result.glyphs[gs].cluster as usize;
            let ch = source_text
                .get(cluster_off..)
                .and_then(|s| s.chars().next());
            if let Some(c) = ch {
                if is_hanging_punctuation(c) {
                    let half_adv = result.glyphs[gs].advance_x * 0.5;
                    result.glyphs[gs].pos.0 -= half_adv;
                }
            }
        }
    }
}
/// Convert a list of exclusive break-glyph indices (as returned by
/// [`crate::knuth_plass::optimal_breaks`]) into `(start, end)` line ranges.
///
/// `flat_len` is the total number of glyphs.
pub(super) fn build_ranges_from_kp_breaks(
    kp_breaks: &[usize],
    flat_len: usize,
    line_ranges: &mut Vec<(usize, usize)>,
) {
    if flat_len == 0 {
        line_ranges.push((0, 0));
        return;
    }
    let mut prev = 0usize;
    for &bp in kp_breaks {
        if bp > prev {
            line_ranges.push((prev, bp));
        }
        prev = bp;
    }
    if prev <= flat_len {
        line_ranges.push((prev, flat_len));
    }
}
/// Count whitespace glyphs that sit *between* non-whitespace glyphs (i.e.
/// internal gaps eligible for justification expansion). Leading/trailing
/// whitespace is excluded.
pub(super) fn count_internal_ws_gaps<'a>(glyphs: impl Iterator<Item = &'a ShapedGlyph>) -> usize {
    let collected: Vec<&ShapedGlyph> = glyphs.collect();
    let first_vis = collected.iter().position(|g| !g.is_whitespace);
    let last_vis = collected.iter().rposition(|g| !g.is_whitespace);
    match (first_vis, last_vis) {
        (Some(f), Some(l)) if l > f => collected[f..=l].iter().filter(|g| g.is_whitespace).count(),
        _ => 0,
    }
}
/// Compute the line's starting X offset and per-gap justification expansion.
///
/// Returns `(x_offset, justify_extra_per_gap)`.
pub(super) fn compute_alignment(
    alignment: TextAlignment,
    line_width: f32,
    max_width: f32,
    wrap: bool,
    is_last_line: bool,
    internal_ws_gaps: usize,
) -> (f32, f32) {
    if !wrap || max_width <= 0.0 {
        return (0.0, 0.0);
    }
    let slack = (max_width - line_width).max(0.0);
    match alignment {
        TextAlignment::Left => (0.0, 0.0),
        TextAlignment::Right => (slack, 0.0),
        TextAlignment::Center => (slack * 0.5, 0.0),
        TextAlignment::Justify => {
            if is_last_line || internal_ws_gaps == 0 || slack <= 0.0 {
                (0.0, 0.0)
            } else {
                (0.0, slack / internal_ws_gaps as f32)
            }
        }
    }
}
/// Apply ellipsis truncation to the last line of a [`LayoutResult`].
///
/// If the last line's total advance width exceeds `trunc.max_width`, glyphs are
/// removed from the end until the remaining advance plus `trunc.ellipsis_advance`
/// fits within `max_width`.  A synthetic ellipsis [`PositionedGlyph`] is then
/// appended and `ParagraphMetrics::truncated` is set to `true`.
///
/// If the last line already fits, the result is returned unchanged.
pub(super) fn apply_truncation(
    mut result: LayoutResult,
    trunc: &crate::options::TruncationMode,
) -> LayoutResult {
    let last_line_idx = match result.lines.len().checked_sub(1) {
        Some(i) => i,
        None => return result,
    };
    let line = &result.lines[last_line_idx];
    let gs = line.glyph_start;
    let ge = line.glyph_end;
    if gs >= ge {
        return result;
    }
    let total_advance = {
        let first_x = result.glyphs[gs].pos.0;
        let mut last_x = first_x;
        let mut last_adv = 0.0f32;
        for gi in gs..ge {
            if gi + 1 < ge {
                last_adv = result.glyphs[gi + 1].pos.0 - result.glyphs[gi].pos.0;
            }
            last_x = result.glyphs[gi].pos.0;
        }
        (last_x - first_x) + last_adv.max(0.0)
    };
    if total_advance <= trunc.max_width {
        return result;
    }
    let ellipsis_adv = trunc.ellipsis_advance;
    let mut keep_end = ge;
    while keep_end > gs {
        let kept_advance = if keep_end > gs {
            let kgs = gs;
            let kge = keep_end;
            let first_x = result.glyphs[kgs].pos.0;
            let mut last_x = first_x;
            let mut last_a = 0.0f32;
            for gi in kgs..kge {
                if gi + 1 < kge {
                    last_a = result.glyphs[gi + 1].pos.0 - result.glyphs[gi].pos.0;
                }
                last_x = result.glyphs[gi].pos.0;
            }
            (last_x - first_x) + last_a.max(0.0)
        } else {
            0.0
        };
        if kept_advance + ellipsis_adv <= trunc.max_width {
            break;
        }
        keep_end -= 1;
    }
    let ellipsis_x = if keep_end > gs {
        let last_kept = &result.glyphs[keep_end - 1];
        let adv = if keep_end < ge {
            result.glyphs[keep_end].pos.0 - last_kept.pos.0
        } else {
            0.0
        };
        last_kept.pos.0 + adv.max(0.0)
    } else if gs < result.glyphs.len() {
        result.glyphs[gs].pos.0
    } else {
        0.0
    };
    let ellipsis_y = result.glyphs[gs].pos.1;
    let line_font_size = result.glyphs[gs].font_size;
    let ellipsis_font = Arc::clone(&result.glyphs[gs].font_data);
    result.glyphs.truncate(keep_end);
    result.glyphs.push(PositionedGlyph {
        gid: trunc.ellipsis_glyph_id,
        font_data: ellipsis_font,
        pos: (ellipsis_x, ellipsis_y),
        font_size: line_font_size,
        advance_x: ellipsis_adv,
        cluster: u32::MAX,
    });
    result.lines[last_line_idx].glyph_end = result.glyphs.len();
    result.metrics.truncated = true;
    let new_width: f32 = result
        .lines
        .iter()
        .map(|l| l.metrics.width)
        .fold(0.0_f32, f32::max);
    result.metrics.total_width = new_width.max(ellipsis_x + ellipsis_adv);
    result
}
/// Returns the UTF-8 byte cluster offset for the `glyph_idx`-th glyph within
/// the line (0-based within the line), by walking the shaped runs.
///
/// `line_glyph_start` is the absolute glyph index of the line's first glyph,
/// used only to compute relative positions; we linearly flatten the runs and
/// pick the `glyph_idx`-th element.
///
/// Returns `None` if the index is out of range.
pub(super) fn find_cluster_for_positioned_glyph(
    line_local_idx: usize,
    runs: &[ShapedRun],
    _line_glyph_start: usize,
) -> Option<usize> {
    let mut count = 0usize;
    for run in runs {
        for g in &run.glyphs {
            if count == line_local_idx {
                return Some(g.cluster as usize);
            }
            count += 1;
        }
    }
    None
}
/// Returns the `x_advance` for the `glyph_idx`-th glyph (0-based) within the
/// flat run list.
pub(super) fn advance_for_glyph(
    line_local_idx: usize,
    runs: &[ShapedRun],
    _line_glyph_start: usize,
) -> f32 {
    let mut count = 0usize;
    for run in runs {
        for g in &run.glyphs {
            if count == line_local_idx {
                return g.x_advance;
            }
            count += 1;
        }
    }
    0.0
}
#[cfg(test)]
mod tests {
    use super::super::types::{
        BreakingStrategy, LayoutEngine, LayoutResult, Line, LineMetrics, ParagraphMetrics,
    };
    use super::*;
    use oxitext_core::{
        FontVerticalMetrics, LayoutConstraints, ShapedGlyph, ShapedRun, TextAlignment,
    };
    use std::sync::Arc;
    /// Build a run whose glyphs correspond 1:1 to the chars of `text`, each
    /// with advance `adv` (whitespace flagged automatically). Cluster offsets
    /// are byte offsets into `text`.
    fn run_from_text(text: &str, adv: f32) -> ShapedRun {
        let mut glyphs = Vec::new();
        for (byte_idx, ch) in text.char_indices() {
            glyphs.push(ShapedGlyph {
                gid: 1,
                x_advance: adv,
                cluster: byte_idx as u32,
                is_whitespace: ch.is_whitespace(),
                ..Default::default()
            });
        }
        ShapedRun {
            glyphs: glyphs.into(),
            font_data: Arc::from(&[][..]),
        }
    }
    #[test]
    fn single_line_when_fits() {
        let text = "hello world";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.lines.len(), 1, "everything fits on one line");
        assert_eq!(res.glyphs.len(), text.chars().count());
    }
    #[test]
    fn wraps_at_space_not_mid_word() {
        let text = "hello world";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 70.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.lines.len(), 2, "should wrap into two lines");
        let first = &res.lines[0];
        assert!(first.len() >= 5, "first line keeps the whole word 'hello'");
        let second_first = &res.glyphs[res.lines[1].glyph_start];
        assert!(
            (second_first.pos.0 - 0.0).abs() < 1e-3,
            "wrapped line starts at x=0"
        );
    }
    #[test]
    fn mandatory_break_on_newline() {
        let text = "a\nb";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.lines.len(), 2, "newline forces a second line");
    }
    #[test]
    fn center_alignment_offsets_line() {
        let text = "ab";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Center, None)
            .expect("layout");
        let first = &res.glyphs[0];
        assert!(
            (first.pos.0 - 40.0).abs() < 1e-3,
            "centered start x should be 40, got {}",
            first.pos.0
        );
    }
    #[test]
    fn right_alignment_offsets_line() {
        let text = "ab";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Right, None)
            .expect("layout");
        let first = &res.glyphs[0];
        assert!(
            (first.pos.0 - 80.0).abs() < 1e-3,
            "right start x should be 80, got {}",
            first.pos.0
        );
    }
    #[test]
    fn baselines_increase_per_line() {
        let text = "a\nb\nc";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.lines.len(), 3);
        assert!(res.lines[1].metrics.baseline_y > res.lines[0].metrics.baseline_y);
        assert!(res.lines[2].metrics.baseline_y > res.lines[1].metrics.baseline_y);
    }
    #[test]
    fn font_metrics_drive_line_height() {
        let text = "a\nb";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 100.0,
        };
        let metrics = FontVerticalMetrics {
            units_per_em: 1000,
            ascender: 800,
            descender: -200,
            line_gap: 0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, Some(&metrics))
            .expect("layout");
        let dy = res.lines[1].metrics.baseline_y - res.lines[0].metrics.baseline_y;
        assert!(
            (dy - 100.0).abs() < 1e-3,
            "line advance should equal 100, got {dy}"
        );
    }
    #[test]
    fn empty_text_yields_one_empty_line() {
        let text = "";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints::default();
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.glyphs.len(), 0);
        assert_eq!(res.lines.len(), 1);
        assert!(res.lines[0].is_empty());
    }
    #[test]
    fn justify_expands_internal_gaps() {
        let text = "a b c";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Justify, None)
            .expect("layout");
        let g0 = &res.glyphs[0];
        assert!((g0.pos.0 - 0.0).abs() < 1e-3);
    }
    #[test]
    fn unbreakable_token_sets_overflow() {
        let text = "aaaaaaaa";
        let run = run_from_text(text, 20.0);
        let c = LayoutConstraints {
            max_width: 50.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert!(
            res.metrics.overflow,
            "expected overflow flag for unbreakable token"
        );
        assert!(res.lines.len() > 1, "long token hard-wraps across lines");
    }
    #[test]
    fn bidi_hebrew_is_visually_reversed() {
        let text = "AB\u{05D0}\u{05D1}";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.glyphs.len(), 4, "4 glyphs total");
        assert_eq!(res.lines.len(), 1, "one line");
        for (i, g) in res.glyphs.iter().enumerate() {
            let expected_x = (i as f32) * 10.0;
            assert!(
                (g.pos.0 - expected_x).abs() < 1e-3,
                "glyph {} x should be {}, got {}",
                i,
                expected_x,
                g.pos.0
            );
        }
    }
    #[test]
    fn bidi_ltr_regression() {
        let text = "hello";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        for (i, g) in res.glyphs.iter().enumerate() {
            let expected_x = (i as f32) * 10.0;
            assert!(
                (g.pos.0 - expected_x).abs() < 1e-3,
                "glyph {} x should be {}, got {}",
                i,
                expected_x,
                g.pos.0
            );
        }
    }
    #[test]
    fn kp_single_line_when_fits() {
        let text = "hello world";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_with_strategy(
                text,
                &[run],
                &c,
                TextAlignment::Left,
                None,
                BreakingStrategy::KnuthPlass,
            )
            .expect("layout");
        assert_eq!(res.lines.len(), 1, "KP: everything fits on one line");
        assert_eq!(res.glyphs.len(), text.chars().count());
    }
    #[test]
    fn kp_wraps_long_text() {
        let text = "aaa bb ccc d eeeee";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 60.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_with_strategy(
                text,
                &[run],
                &c,
                TextAlignment::Left,
                None,
                BreakingStrategy::KnuthPlass,
            )
            .expect("layout");
        assert!(res.lines.len() > 1, "KP: must produce multiple lines");
        assert_eq!(res.glyphs.len(), text.chars().count(), "all glyphs present");
    }
    #[test]
    fn kp_mandatory_break_honoured() {
        let text = "hello\nworld";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_with_strategy(
                text,
                &[run],
                &c,
                TextAlignment::Left,
                None,
                BreakingStrategy::KnuthPlass,
            )
            .expect("layout");
        assert_eq!(res.lines.len(), 2, "KP: newline forces a second line");
    }
    #[test]
    fn vertical_layout_positions_glyphs_top_to_bottom() {
        let text = "abc";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_vertical(text, &[run], 0.0, 16.0, None)
            .expect("vertical layout");
        assert!(!res.glyphs.is_empty());
        for w in res.glyphs.windows(2) {
            assert!(
                w[1].pos.1 >= w[0].pos.1,
                "vertical y must increase: {} >= {}",
                w[1].pos.1,
                w[0].pos.1
            );
        }
    }
    #[test]
    fn vertical_layout_column_break_on_max_height() {
        let text = "abcde";
        let run = run_from_text(text, 16.0);
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_vertical(text, &[run], 48.0, 16.0, None)
            .expect("vertical layout");
        assert!(
            res.lines.len() >= 2,
            "expected >= 2 columns, got {}",
            res.lines.len()
        );
        if res.lines.len() >= 2 {
            let first_col_x = res.glyphs[res.lines[0].glyph_start].pos.0;
            let second_col_x = res.glyphs[res.lines[1].glyph_start].pos.0;
            assert!(
                second_col_x > first_col_x,
                "second column x ({}) must be > first column x ({})",
                second_col_x,
                first_col_x
            );
        }
    }
    #[test]
    fn vertical_layout_metrics_have_positive_dimensions() {
        let text = "hello";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_vertical(text, &[run], 0.0, 16.0, None)
            .expect("vertical layout");
        assert!(
            res.metrics.total_height > 0.0,
            "total_height must be positive"
        );
        assert!(
            res.metrics.total_width > 0.0,
            "total_width must be positive"
        );
    }
    #[test]
    fn layout_with_tab_stops() {
        let ts = crate::options::TabStops::with_interval(80.0);
        assert!(
            (ts.next_stop(10.0) - 80.0).abs() < 1.0,
            "next stop from 10 should be 80"
        );
        assert!(
            (ts.next_stop(0.0) - 80.0).abs() < 1.0,
            "next stop from 0 should be 80"
        );
        assert!(
            (ts.next_stop(80.0) - 160.0).abs() < 1.0,
            "next stop from 80 should be 160"
        );
    }
    #[test]
    fn layout_with_options_tab_stops_resolve_correct_glyph_on_second_line() {
        // Regression test for the `layout_with_options` tab-stop handler:
        // `find_cluster_for_positioned_glyph`/`advance_for_glyph` walk
        // `shaped_runs` from its very first glyph (they ignore the
        // `line_glyph_start` argument), so callers must pass the glyph's
        // *absolute* index within `shaped_runs`/`result.glyphs`, not an
        // index relative to the line. Passing the line-local index made
        // every line after the first resolve the wrong source character,
        // silently missing tab stops.
        //
        // `text` forces a mandatory break right after '\n' so line 2 starts
        // at a non-zero glyph offset, and contains two consecutive tabs so
        // the tab-stop cascade (each stop computed from the previous one)
        // gives an unambiguous signal that both tabs on line 2 were
        // correctly recognised as `\t`.
        let text = "aa\nbb\t\tcc";
        let run = run_from_text(text, 10.0);
        let ts = crate::options::TabStops::with_interval(80.0);
        let opts = crate::options::LayoutOptions::builder()
            .tab_stops(ts)
            .build();
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_with_options(text, &[run], 10000.0, &opts, None, 16.0)
            .expect("layout_with_options");
        assert_eq!(
            res.lines.len(),
            2,
            "mandatory break after '\\n' should yield exactly 2 lines"
        );
        let line2 = &res.lines[1];
        // Line 2 glyphs, in source order: 'b', 'b', '\t', '\t', 'c', 'c'.
        assert_eq!(line2.len(), 6, "line 2 should have 6 glyphs");
        let tab2_idx = line2.glyph_start + 3;
        // The second tab must be positioned at the first tab's snapped stop
        // (80.0), proving both tabs on this second line were identified as
        // `\t` and the snap correctly cascaded. With the pre-fix line-local
        // index, neither tab on line 2 is recognised as `\t` at all, and
        // the second tab keeps its untouched natural (non-cascaded)
        // position instead.
        assert_eq!(
            res.glyphs[tab2_idx].pos.0, 80.0,
            "second tab on line 2 must snap forward from the first tab's stop"
        );
    }
    #[test]
    fn truncation_mode_basic() {
        let trunc = crate::options::TruncationMode {
            max_width: 50.0,
            ellipsis_advance: 10.0,
            ellipsis_glyph_id: 0,
        };
        assert_eq!(trunc.max_width, 50.0);
        assert_eq!(trunc.ellipsis_advance, 10.0);
        assert_eq!(trunc.ellipsis_glyph_id, 0);
    }
    #[test]
    fn layout_options_builder() {
        let opts = crate::options::LayoutOptions::builder()
            .alignment(oxitext_core::TextAlignment::Center)
            .paragraph_spacing(12.0)
            .build();
        assert_eq!(opts.paragraph_spacing, 12.0);
        assert_eq!(opts.alignment, oxitext_core::TextAlignment::Center);
    }
    #[test]
    fn truncation_applied_on_overflow() {
        let text = "hello world";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let trunc = crate::options::TruncationMode {
            max_width: 60.0,
            ellipsis_advance: 10.0,
            ellipsis_glyph_id: 0,
        };
        let opts = crate::options::LayoutOptions::builder()
            .truncation(trunc)
            .build();
        let res = engine
            .layout_with_options(text, &[run], 10000.0, &opts, None, 16.0)
            .expect("layout_with_options");
        let last = res.glyphs.last().expect("at least one glyph");
        assert_eq!(last.gid, 0, "last glyph should be ellipsis (gid 0)");
        assert!(res.metrics.truncated, "metrics.truncated should be true");
    }
    #[test]
    fn no_truncation_when_fits() {
        let text = "hi";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let trunc = crate::options::TruncationMode {
            max_width: 200.0,
            ellipsis_advance: 10.0,
            ellipsis_glyph_id: 0,
        };
        let opts = crate::options::LayoutOptions::builder()
            .truncation(trunc)
            .build();
        let res = engine
            .layout_with_options(text, &[run], 10000.0, &opts, None, 16.0)
            .expect("layout_with_options");
        assert!(!res.metrics.truncated, "short text should not be truncated");
        assert_eq!(res.glyphs.len(), 2, "all glyphs present");
    }
    #[test]
    fn layout_paragraphs_offsets_y() {
        let text1 = "ab";
        let text2 = "cd";
        let run1 = run_from_text(text1, 10.0);
        let run2 = run_from_text(text2, 10.0);
        let mut engine = LayoutEngine::new();
        let runs1 = [run1];
        let runs2 = [run2];
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let opts = crate::options::LayoutOptions::builder()
            .alignment(TextAlignment::Left)
            .build();
        let res = engine
            .layout_paragraphs(
                &[text1, text2],
                &[runs1.as_slice(), runs2.as_slice()],
                &c,
                20.0,
                &opts,
                None,
            )
            .expect("layout_paragraphs");
        assert!(res.lines.len() >= 2, "should have at least 2 lines");
        let y0 = res.lines[0].metrics.baseline_y;
        let y1 = res.lines[1].metrics.baseline_y;
        assert!(
            y1 > y0,
            "second paragraph must be below first: y0={y0} y1={y1}"
        );
    }
    #[test]
    fn zwj_suppresses_break() {
        let text = "a\u{200D}b";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert_eq!(res.glyphs.len(), 3, "a + ZWJ + b = 3 glyphs");
    }
    #[test]
    fn zwnj_allows_break() {
        let text = "a\u{200C}b";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 15.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout");
        assert!(res.glyphs.len() == 3, "a + ZWNJ + b = 3 glyphs");
        assert!(!res.lines.is_empty());
    }
    /// Build a synthetic LayoutResult with known positions for hit-test testing.
    ///
    /// Three glyphs on a single line, each 10px wide, baseline_y = 16.
    fn make_hit_test_result() -> LayoutResult {
        use std::sync::Arc;
        let font: Arc<[u8]> = Arc::from(&[][..]);
        let glyphs = vec![
            PositionedGlyph {
                gid: 1,
                font_data: Arc::clone(&font),
                pos: (0.0, 16.0),
                font_size: 16.0,
                advance_x: 10.0,
                cluster: 0,
            },
            PositionedGlyph {
                gid: 2,
                font_data: Arc::clone(&font),
                pos: (10.0, 16.0),
                font_size: 16.0,
                advance_x: 10.0,
                cluster: 1,
            },
            PositionedGlyph {
                gid: 3,
                font_data: Arc::clone(&font),
                pos: (20.0, 16.0),
                font_size: 16.0,
                advance_x: 10.0,
                cluster: 2,
            },
        ];
        let lines = vec![Line {
            glyph_start: 0,
            glyph_end: 3,
            metrics: LineMetrics {
                ascent: 12.8,
                descent: 3.2,
                leading: 0.0,
                baseline_y: 16.0,
                width: 30.0,
            },
        }];
        LayoutResult {
            glyphs,
            lines,
            metrics: ParagraphMetrics {
                total_height: 22.4,
                total_width: 30.0,
                line_count: 1,
                overflow: false,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        }
    }
    #[test]
    fn hit_test_finds_correct_glyph() {
        let res = make_hit_test_result();
        let hit = res.hit_test(5.0, 16.0).expect("hit_test returned None");
        assert_eq!(hit.0, 0, "should be on line 0");
        assert_eq!(hit.1, 0, "glyph index in line should be 0 (first glyph)");
        assert_eq!(hit.2, 0, "cluster should be 0");
        let hit = res.hit_test(15.0, 16.0).expect("hit_test returned None");
        assert_eq!(hit.1, 1, "glyph index in line should be 1");
        assert_eq!(hit.2, 1, "cluster should be 1");
        let hit = res.hit_test(25.0, 16.0).expect("hit_test returned None");
        assert_eq!(hit.1, 2, "glyph index in line should be 2");
        assert_eq!(hit.2, 2, "cluster should be 2");
    }
    #[test]
    fn hit_test_out_of_bounds_clamps() {
        let res = make_hit_test_result();
        let hit = res.hit_test(-100.0, 16.0).expect("hit_test returned None");
        assert_eq!(hit.1, 0, "far-left hit should clamp to glyph 0");
        let hit = res.hit_test(99999.0, 16.0).expect("hit_test returned None");
        assert_eq!(hit.1, 2, "far-right hit should clamp to glyph 2");
    }
    #[test]
    fn hit_test_y_outside_all_lines_picks_nearest() {
        let res = make_hit_test_result();
        let hit = res.hit_test(5.0, -100.0).expect("hit_test returned None");
        assert_eq!(hit.0, 0, "y far above should still return line 0");
        let hit = res.hit_test(5.0, 99999.0).expect("hit_test returned None");
        assert_eq!(hit.0, 0, "y far below should still return line 0");
    }
    #[test]
    fn hit_test_empty_layout_returns_none() {
        let res = LayoutResult {
            glyphs: vec![],
            lines: vec![],
            metrics: ParagraphMetrics {
                total_height: 0.0,
                total_width: 0.0,
                line_count: 0,
                overflow: false,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        };
        assert!(
            res.hit_test(0.0, 0.0).is_none(),
            "empty layout should return None"
        );
    }
    #[test]
    fn hanging_punctuation_flag_in_options() {
        let opts = crate::options::LayoutOptions::builder().build();
        assert!(
            !opts.hanging_punctuation,
            "hanging_punctuation should default to false"
        );
        let opts_on = crate::options::LayoutOptions::builder()
            .hanging_punctuation(true)
            .build();
        assert!(
            opts_on.hanging_punctuation,
            "hanging_punctuation should be settable to true"
        );
    }
    #[test]
    fn hanging_punctuation_shifts_terminal_punct() {
        let text = "abc\u{3002}";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let opts = crate::options::LayoutOptions::builder()
            .hanging_punctuation(true)
            .build();
        let res_no_hang = engine
            .layout_with_options(
                text,
                std::slice::from_ref(&run),
                1000.0,
                &crate::options::LayoutOptions::default(),
                None,
                16.0,
            )
            .expect("layout no-hang");
        let res_hang = engine
            .layout_with_options(text, std::slice::from_ref(&run), 1000.0, &opts, None, 16.0)
            .expect("layout hang");
        let last_no_hang = res_no_hang
            .glyphs
            .last()
            .expect("no-hang: last glyph")
            .pos
            .0;
        let last_hang = res_hang.glyphs.last().expect("hang: last glyph").pos.0;
        assert!(
            (last_hang - (last_no_hang + 5.0)).abs() < 1e-3,
            "hanging punct should shift last glyph right by half advance (5px); \
             no_hang={last_no_hang}, hang={last_hang}"
        );
    }
    #[test]
    fn test_external_break_points() {
        let text = "Hello there";
        let run = run_from_text(text, 8.0);
        let mut engine = LayoutEngine::new();
        let c_base = LayoutConstraints {
            max_width: 0.0,
            font_size: 16.0,
        };
        let base = engine
            .layout(
                text,
                std::slice::from_ref(&run),
                &c_base,
                TextAlignment::Left,
                None,
            )
            .expect("base layout");
        assert_eq!(base.lines.len(), 1, "no-wrap baseline should be 1 line");
        let c_narrow = LayoutConstraints {
            max_width: 50.0,
            font_size: 16.0,
        };
        let result = engine
            .layout_with_break_points(text, &[run], &c_narrow, TextAlignment::Left, None, &[5])
            .expect("layout_with_break_points");
        assert!(!result.lines.is_empty(), "should produce at least one line");
        assert_eq!(
            result.glyphs.len(),
            text.chars().count(),
            "all glyphs should be present"
        );
        assert!(!result.lines.is_empty());
    }
    #[test]
    fn external_break_points_single_word_no_wrap() {
        let text = "abcdef";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let c = LayoutConstraints {
            max_width: 40.0,
            font_size: 16.0,
        };
        let result = engine
            .layout_with_break_points(text, &[run], &c, TextAlignment::Left, None, &[3])
            .expect("layout");
        assert!(
            result.lines.len() >= 2,
            "expected >= 2 lines, got {}",
            result.lines.len()
        );
        assert_eq!(result.glyphs.len(), 6, "all 6 glyphs present");
        assert!(
            !result.metrics.overflow,
            "external break should avoid hard-break overflow flag"
        );
    }
    #[test]
    fn external_break_points_empty_slice() {
        let text = "hello";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let result = engine
            .layout_with_break_points(text, &[run], &c, TextAlignment::Left, None, &[])
            .expect("layout");
        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.glyphs.len(), 5);
    }
    #[test]
    fn test_parallel_layout_left_align() {
        let text = "Hello world test text okay";
        let run = run_from_text(text, 6.0);
        let mut engine = LayoutEngine::new();
        let opts = crate::options::LayoutOptions::default();
        let result = engine
            .layout_with_options(text, &[run], 60.0, &opts, None, 16.0)
            .expect("layout_with_options");
        assert!(!result.glyphs.is_empty(), "glyphs should be non-empty");
        for (li, line) in result.lines.iter().enumerate() {
            if line.glyph_start < line.glyph_end {
                let first_x = result.glyphs[line.glyph_start].pos.0;
                assert!(
                    first_x.abs() < 1.0,
                    "left-aligned line {} first glyph x should be ~0, got {}",
                    li,
                    first_x
                );
            }
        }
    }
    #[test]
    fn test_parallel_layout_center_align() {
        let text = "hi";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let result = engine
            .layout(text, &[run], &c, TextAlignment::Center, None)
            .expect("layout center");
        assert!(!result.glyphs.is_empty());
        let first_x = result.glyphs[0].pos.0;
        assert!(
            (first_x - 40.0).abs() < 1e-3,
            "center-aligned first glyph x should be 40, got {first_x}"
        );
    }
    #[test]
    fn test_parallel_layout_right_align() {
        let text = "hi";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let result = engine
            .layout(text, &[run], &c, TextAlignment::Right, None)
            .expect("layout right");
        assert!(!result.glyphs.is_empty());
        let first_x = result.glyphs[0].pos.0;
        assert!(
            (first_x - 80.0).abs() < 1e-3,
            "right-aligned first glyph x should be 80, got {first_x}"
        );
    }
    #[test]
    fn test_multi_line_parallel_offsets() {
        let text = "abcd\nefgh\nijkl";
        let run = run_from_text(text, 10.0);
        let mut engine = LayoutEngine::new();
        let c = LayoutConstraints {
            max_width: 100.0,
            font_size: 16.0,
        };
        let result = engine
            .layout(text, &[run], &c, TextAlignment::Center, None)
            .expect("multi-line center");
        assert!(
            result.lines.len() >= 3,
            "should have 3 lines for \\n-separated text"
        );
        for line in &result.lines {
            if line.glyph_start < line.glyph_end {
                let x = result.glyphs[line.glyph_start].pos.0;
                assert!(
                    x >= 0.0,
                    "center-aligned line x should be non-negative, got {x}"
                );
            }
        }
    }
    #[test]
    #[ignore]
    fn bench_layout_10k_chars() {
        let text: String = "Hello world ".repeat(850);
        let run = run_from_text(&text, 8.0);
        let c = LayoutConstraints {
            max_width: 600.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let start = std::time::Instant::now();
        let result = engine
            .layout(&text, &[run], &c, TextAlignment::Left, None)
            .expect("bench layout");
        let elapsed = start.elapsed();
        println!(
            "10K layout: {:?}  ({} lines, {} glyphs)",
            elapsed,
            result.lines.len(),
            result.glyphs.len()
        );
    }

    // --- Incremental relayout API tests ---

    #[test]
    fn test_mark_dirty_sets_has_dirty() {
        let mut engine = LayoutEngine::new();
        assert!(!engine.has_dirty(), "fresh engine should not be dirty");
        engine.mark_dirty(0..5);
        assert!(
            engine.has_dirty(),
            "engine should be dirty after mark_dirty"
        );
        engine.clear_dirty();
        assert!(
            !engine.has_dirty(),
            "engine should be clean after clear_dirty"
        );
    }

    #[test]
    fn test_mark_dirty_accumulates_multiple_ranges() {
        let mut engine = LayoutEngine::new();
        engine.mark_dirty(0..3);
        engine.mark_dirty(10..20);
        engine.mark_dirty(30..40);
        assert!(engine.has_dirty());
        engine.clear_dirty();
        assert!(!engine.has_dirty());
    }

    #[test]
    fn test_layout_if_dirty_returns_cached_when_clean() {
        let text = "hello";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        // Produce an initial layout result to use as the cache.
        let initial = engine
            .layout(
                text,
                std::slice::from_ref(&run),
                &c,
                TextAlignment::Left,
                None,
            )
            .expect("initial layout");
        let initial_glyph_count = initial.glyphs.len();

        // Engine is clean — layout_if_dirty should return the cached result.
        let returned = engine.layout_if_dirty(Some(initial), |eng| {
            eng.layout(
                text,
                std::slice::from_ref(&run),
                &c,
                TextAlignment::Left,
                None,
            )
            .expect("relayout")
        });
        assert_eq!(
            returned.glyphs.len(),
            initial_glyph_count,
            "cached result should be returned unchanged when engine is clean"
        );
        // Engine should still be clean (no dirty was set, none was cleared).
        assert!(!engine.has_dirty());
    }

    #[test]
    fn test_layout_if_dirty_relayouts_when_dirty() {
        let text = "hello";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();

        // Mark a range dirty to force relayout.
        engine.mark_dirty(0..5);
        assert!(engine.has_dirty());

        let relayout_called = std::cell::Cell::new(false);
        let _result = engine.layout_if_dirty(None, |eng| {
            relayout_called.set(true);
            eng.layout(
                text,
                std::slice::from_ref(&run),
                &c,
                TextAlignment::Left,
                None,
            )
            .expect("relayout")
        });

        assert!(
            relayout_called.get(),
            "layout_fn should be called when dirty"
        );
        // Dirty markers should be cleared after relayout.
        assert!(
            !engine.has_dirty(),
            "dirty should be cleared after layout_if_dirty"
        );
    }

    #[test]
    fn test_layout_if_dirty_calls_fn_when_no_cached_even_if_clean() {
        let text = "hi";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 500.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        // Engine is clean but no cached result — layout_fn must be called.
        let called = std::cell::Cell::new(false);
        let _result = engine.layout_if_dirty(None, |eng| {
            called.set(true);
            eng.layout(
                text,
                std::slice::from_ref(&run),
                &c,
                TextAlignment::Left,
                None,
            )
            .expect("layout")
        });
        assert!(
            called.get(),
            "layout_fn should be called when cached is None"
        );
    }

    // --- layout_uax14 explicit UAX #14 path ---

    #[test]
    fn test_layout_uax14_explicit() {
        let text = "Hello World";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 1000.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_uax14(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout_uax14");
        assert_eq!(res.glyphs.len(), text.chars().count(), "all glyphs present");
        assert!(!res.lines.is_empty(), "at least one line");
    }

    #[test]
    fn test_layout_uax14_wraps_at_word_boundary() {
        // 11 chars × 10px = 110px; max_width = 60px → wraps after "Hello "
        let text = "Hello World";
        let run = run_from_text(text, 10.0);
        let c = LayoutConstraints {
            max_width: 60.0,
            font_size: 16.0,
        };
        let mut engine = LayoutEngine::new();
        let res = engine
            .layout_uax14(text, &[run], &c, TextAlignment::Left, None)
            .expect("layout_uax14 wrap");
        assert!(res.lines.len() >= 2, "should wrap to at least 2 lines");
    }

    // ----- LayoutResult atlas / rasterisation helpers -----

    /// Build a minimal `LayoutResult` from explicit `PositionedGlyph` values.
    /// `ParagraphMetrics` is constructed with zeroed fields since we only test
    /// the helpers and not the metrics themselves.
    fn make_result(glyphs: Vec<oxitext_core::PositionedGlyph>) -> LayoutResult {
        let n = glyphs.len();
        let lines = if n == 0 {
            vec![]
        } else {
            vec![Line {
                glyph_start: 0,
                glyph_end: n,
                metrics: LineMetrics {
                    ascent: 12.0,
                    descent: 4.0,
                    leading: 0.0,
                    baseline_y: 12.0,
                    width: 0.0,
                },
            }]
        };
        LayoutResult {
            glyphs,
            lines,
            metrics: ParagraphMetrics {
                total_height: 0.0,
                total_width: 0.0,
                line_count: 0,
                overflow: false,
                truncated: false,
            },
            decorations: Vec::new(),
            inline_objects: Vec::new(),
        }
    }

    #[test]
    fn test_unique_glyphs_for_atlas_deduplicates() {
        // glyph 65 appears twice; glyph 66 appears once.
        // unique_glyphs_for_atlas should return exactly 2 entries.
        let font: Arc<[u8]> = Arc::from(&[][..]);
        let g1 = oxitext_core::PositionedGlyph {
            gid: 65,
            font_data: Arc::clone(&font),
            pos: (0.0, 0.0),
            font_size: 16.0,
            advance_x: 10.0,
            cluster: 0,
        };
        let g2 = oxitext_core::PositionedGlyph {
            gid: 65,
            font_data: Arc::clone(&font),
            pos: (10.0, 0.0),
            font_size: 16.0,
            advance_x: 10.0,
            cluster: 1,
        };
        let g3 = oxitext_core::PositionedGlyph {
            gid: 66,
            font_data: Arc::clone(&font),
            pos: (20.0, 0.0),
            font_size: 16.0,
            advance_x: 10.0,
            cluster: 2,
        };
        let result = make_result(vec![g1, g2, g3]);
        let unique = result.unique_glyphs_for_atlas();
        assert_eq!(
            unique.len(),
            2,
            "expected 2 unique (gid, size) pairs, got {}",
            unique.len()
        );
        assert!(
            unique.contains(&(65, 16.0)),
            "pair (65, 16.0) must be present"
        );
        assert!(
            unique.contains(&(66, 16.0)),
            "pair (66, 16.0) must be present"
        );
    }

    #[test]
    fn test_unique_glyphs_different_sizes_are_distinct() {
        // same glyph ID but different font sizes should be treated as distinct pairs.
        let font: Arc<[u8]> = Arc::from(&[][..]);
        let g1 = oxitext_core::PositionedGlyph {
            gid: 65,
            font_data: Arc::clone(&font),
            pos: (0.0, 0.0),
            font_size: 16.0,
            advance_x: 10.0,
            cluster: 0,
        };
        let g2 = oxitext_core::PositionedGlyph {
            gid: 65,
            font_data: Arc::clone(&font),
            pos: (0.0, 20.0),
            font_size: 32.0,
            advance_x: 20.0,
            cluster: 1,
        };
        let result = make_result(vec![g1, g2]);
        let unique = result.unique_glyphs_for_atlas();
        assert_eq!(
            unique.len(),
            2,
            "different sizes must be counted separately"
        );
    }

    #[test]
    fn test_rasterization_inputs_preserves_order() {
        let font: Arc<[u8]> = Arc::from(&[][..]);
        let glyphs: Vec<oxitext_core::PositionedGlyph> = vec![
            oxitext_core::PositionedGlyph {
                gid: 10,
                font_data: Arc::clone(&font),
                pos: (0.0, 1.0),
                font_size: 14.0,
                advance_x: 8.0,
                cluster: 0,
            },
            oxitext_core::PositionedGlyph {
                gid: 20,
                font_data: Arc::clone(&font),
                pos: (8.0, 1.0),
                font_size: 14.0,
                advance_x: 8.0,
                cluster: 1,
            },
            oxitext_core::PositionedGlyph {
                gid: 30,
                font_data: Arc::clone(&font),
                pos: (16.0, 1.0),
                font_size: 14.0,
                advance_x: 8.0,
                cluster: 2,
            },
        ];
        let result = make_result(glyphs);
        let inputs = result.rasterization_inputs();
        assert_eq!(inputs.len(), 3, "one entry per glyph");
        assert_eq!(inputs[0], (10, 0.0, 1.0, 14.0));
        assert_eq!(inputs[1], (20, 8.0, 1.0, 14.0));
        assert_eq!(inputs[2], (30, 16.0, 1.0, 14.0));
    }

    #[test]
    fn test_sdf_glyph_set_equals_unique_glyphs() {
        // sdf_glyph_set is an alias; its output must be identical to unique_glyphs_for_atlas.
        let font: Arc<[u8]> = Arc::from(&[][..]);
        let g1 = oxitext_core::PositionedGlyph {
            gid: 7,
            font_data: Arc::clone(&font),
            pos: (0.0, 0.0),
            font_size: 24.0,
            advance_x: 12.0,
            cluster: 0,
        };
        let result = make_result(vec![g1]);
        assert_eq!(result.sdf_glyph_set(), result.unique_glyphs_for_atlas());
    }

    #[test]
    fn test_unique_glyphs_empty_layout() {
        let result = make_result(vec![]);
        assert!(
            result.unique_glyphs_for_atlas().is_empty(),
            "no glyphs → empty set"
        );
        assert!(
            result.rasterization_inputs().is_empty(),
            "no glyphs → empty inputs"
        );
    }
}
