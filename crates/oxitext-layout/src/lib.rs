#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! `oxitext-layout` — Text layout for OxiText.
//!
//! Provides [`SimpleLayouter`], a left-to-right cursor-advance layouter that
//! wraps lines when the current cursor exceeds `max_width`.
//!
//! M1: LTR simple layout.
//! M2: [`bidi`] (UAX #9), [`linebreak`] (UAX #14), [`vertical`] (UAX #50 subset).
//! M3 (deferred): Parley integration for full rich-text layout.
//! M4: [`tate_chu_yoko`] — horizontal run detection within vertical CJK lines.
//! M6: [`engine`] — word-aware ([`linebreak`]-driven) wrapping with horizontal
//!     [`oxitext_core::TextAlignment`] and structured [`engine::LayoutResult`]
//!     output (per-line and per-paragraph metrics).

pub mod bidi;
pub mod engine;
pub mod hyphenation;
pub mod knuth_plass;
pub mod linebreak;
pub mod options;
pub mod reorder;
pub mod ruby;
pub mod styled;
pub mod tate_chu_yoko;
pub mod vertical;

pub use engine::{
    BreakingStrategy, LayoutEngine, LayoutResult, Line, LineMetrics, ParagraphMetrics,
};
pub use hyphenation::soft_hyphen_breaks;
pub use options::{LayoutOptions, LayoutOptionsBuilder, TabStops, TruncationMode};
pub use oxitext_core::{
    DecorationRect, InlineObject, PositionedInlineObject, TextDecoration, VerticalPosition,
};
pub use reorder::needs_bidi;
pub use ruby::{layout_ruby, RubyAnnotation, RubyLayout, RubyPosition};
pub use styled::StyledRun;
pub use tate_chu_yoko::{detect_runs, tcy_combined_advance, GlyphEntry, TateChuYokoRun};
pub use vertical::vmtx_advance_for_glyph;

use oxitext_core::{FlowDirection, LayoutConstraints, OxiTextError, PositionedGlyph, ShapedRun};
use std::sync::Arc;

/// Simple layouter that supports both horizontal (LTR) and vertical flow.
///
/// For horizontal flow, advances a cursor left-to-right, emitting a
/// [`PositionedGlyph`] for each input glyph and wrapping when the cursor
/// exceeds `max_width`.
///
/// For vertical flow, advances the cursor top-to-bottom using each glyph's
/// `y_advance` (falling back to `x_advance` when `y_advance` is zero), and
/// wraps into a new column when `max_width` (treated as max column height) is
/// exceeded.
pub struct SimpleLayouter {
    /// Text flow direction for this layouter instance.
    pub flow_direction: FlowDirection,
}

impl SimpleLayouter {
    /// Creates a new layouter with horizontal flow (the default).
    pub fn new() -> Self {
        Self {
            flow_direction: FlowDirection::Horizontal,
        }
    }

    /// Returns a copy of this layouter with the given flow direction.
    pub fn with_flow_direction(mut self, dir: FlowDirection) -> Self {
        self.flow_direction = dir;
        self
    }

    /// Positions glyphs from the shaped runs according to constraints.
    ///
    /// Dispatches to `Self::layout_horizontal` or `Self::layout_vertical`
    /// based on [`Self::flow_direction`].
    ///
    /// # Errors
    /// Currently infallible; returns `Err` only for forward compatibility.
    pub fn layout(
        &self,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
    ) -> Result<Vec<PositionedGlyph>, OxiTextError> {
        match self.flow_direction {
            FlowDirection::Horizontal => self.layout_horizontal(runs, constraints),
            FlowDirection::Vertical => self.layout_vertical(runs, constraints),
        }
    }

    /// Horizontal (LTR) cursor-advance layout.
    ///
    /// Wraps to a new line when advancing would exceed `constraints.max_width`.
    fn layout_horizontal(
        &self,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
    ) -> Result<Vec<PositionedGlyph>, OxiTextError> {
        let mut positioned = Vec::new();
        let mut cursor_x: f32 = 0.0;
        // Place baseline one line-height below the top of the canvas.
        let line_height = constraints.font_size * 1.4;
        let mut cursor_y: f32 = constraints.font_size * 1.2;

        for run in runs {
            let font_data = Arc::clone(&run.font_data);
            for glyph in &run.glyphs {
                // Word-wrap: if advancing would push us past max_width, newline.
                if constraints.max_width > 0.0 && cursor_x + glyph.x_advance > constraints.max_width
                {
                    cursor_x = 0.0;
                    cursor_y += line_height;
                }
                positioned.push(PositionedGlyph {
                    gid: glyph.gid,
                    font_data: Arc::clone(&font_data),
                    pos: (cursor_x + glyph.x_offset, cursor_y + glyph.y_offset),
                    font_size: constraints.font_size,
                    advance_x: glyph.x_advance,
                    cluster: glyph.cluster,
                });
                cursor_x += glyph.x_advance;
            }
        }

        Ok(positioned)
    }

    /// Vertical (top-to-bottom) cursor-advance layout.
    ///
    /// Advances the cursor downward using each glyph's `y_advance`; when
    /// `y_advance` is 0 (as is common for glyphs shaped with only horizontal
    /// metrics), `x_advance` is used as the vertical advance instead.
    ///
    /// When `constraints.max_width > 0` (treated as the maximum column height),
    /// a new column is started `font_size * 1.2` to the right when the cursor
    /// would overflow.
    fn layout_vertical(
        &self,
        runs: &[ShapedRun],
        constraints: &LayoutConstraints,
    ) -> Result<Vec<PositionedGlyph>, OxiTextError> {
        let mut positioned = Vec::new();
        let column_width = constraints.font_size * 1.2;
        let mut column_x: f32 = 0.0;
        let mut cursor_y: f32 = 0.0;
        let max_col_h = constraints.max_width; // semantically: max column height

        for run in runs {
            let font_data = Arc::clone(&run.font_data);
            for glyph in &run.glyphs {
                // Use y_advance when available; fall back to x_advance.
                let v_adv = if glyph.y_advance > 0.0 {
                    glyph.y_advance
                } else {
                    glyph.x_advance
                };

                // Wrap to next column when column height would be exceeded.
                if max_col_h > 0.0 && cursor_y + v_adv > max_col_h && cursor_y > 0.0 {
                    column_x += column_width;
                    cursor_y = 0.0;
                }

                positioned.push(PositionedGlyph {
                    gid: glyph.gid,
                    font_data: Arc::clone(&font_data),
                    pos: (column_x + glyph.x_offset, cursor_y + glyph.y_offset),
                    font_size: constraints.font_size,
                    advance_x: glyph.x_advance,
                    cluster: glyph.cluster,
                });
                cursor_y += v_adv;
            }
        }

        Ok(positioned)
    }
}

impl Default for SimpleLayouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxitext_core::{LayoutConstraints, ShapedGlyph, ShapedRun};
    use std::sync::Arc;

    fn make_run(advances: &[f32]) -> ShapedRun {
        let glyphs = advances
            .iter()
            .enumerate()
            .map(|(i, &adv)| ShapedGlyph {
                gid: (i + 1) as u16,
                x_advance: adv,
                cluster: i as u32,
                ..Default::default()
            })
            .collect();
        ShapedRun {
            glyphs,
            font_data: Arc::from(&[][..]),
        }
    }

    #[test]
    fn layout_positions_are_monotonically_increasing_x() {
        let run = make_run(&[10.0, 10.0, 10.0, 10.0, 10.0]);
        let constraints = LayoutConstraints {
            max_width: 800.0,
            font_size: 16.0,
        };
        let layouter = SimpleLayouter::new();
        let positioned = layouter
            .layout(&[run], &constraints)
            .expect("layout failed");
        assert_eq!(positioned.len(), 5);
        // Each successive glyph should be 10px further right.
        for window in positioned.windows(2) {
            assert!(
                window[1].pos.0 > window[0].pos.0,
                "x should increase: {} <= {}",
                window[1].pos.0,
                window[0].pos.0
            );
        }
    }

    #[test]
    fn layout_wraps_when_max_width_exceeded() {
        // 5 glyphs × 200px advance; max_width = 800px → wraps at glyph 5
        let run = make_run(&[200.0, 200.0, 200.0, 200.0, 200.0]);
        let constraints = LayoutConstraints {
            max_width: 800.0,
            font_size: 16.0,
        };
        let layouter = SimpleLayouter::new();
        let positioned = layouter
            .layout(&[run], &constraints)
            .expect("layout failed");
        assert_eq!(positioned.len(), 5);
        // The 5th glyph (index 4) needs to wrap because 4×200=800 exactly, and
        // attempting to place 200 more would exceed 800.
        // Glyph 0..3 are on the first line; glyph 4 wraps.
        let y_first = positioned[0].pos.1;
        let y_wrap = positioned[4].pos.1;
        assert!(
            y_wrap > y_first,
            "wrapped glyph should be on a lower line: y_first={y_first}, y_wrap={y_wrap}"
        );
    }
}
