//! Ruby annotation layout for CJK furigana and phonetic glosses.
//!
//! Ruby (ルビ) is a typographic convention used in East Asian text to place
//! small pronunciation or reading guides above (or below) base characters.
//! This module provides the data types and positioning algorithm for rendering
//! ruby annotations on top of an already-shaped base text.
//!
//! # Usage
//!
//! 1. Shape both the base text and the ruby text (using any shaper).
//! 2. Call [`layout_ruby`] with the base glyphs and ruby shaped glyphs.
//! 3. Render [`RubyLayout::base_glyphs`] normally, then render
//!    [`RubyLayout::ruby_glyphs`] with the y-offset from
//!    [`RubyLayout::ruby_y_offset`].
//!
//! The calling code is responsible for increasing the line height by
//! [`RubyLayout::extra_line_height`] to avoid overlapping adjacent lines.

use oxitext_core::{PositionedGlyph, ShapedGlyph};
use std::sync::Arc;

/// Where the ruby annotation is placed relative to the base text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RubyPosition {
    /// Place the annotation above the base text (default for horizontal CJK).
    Above,
    /// Place the annotation below the base text.
    Below,
}

/// A ruby (furigana) annotation attached to a span of base text.
#[derive(Debug, Clone)]
pub struct RubyAnnotation {
    /// Byte range in the base text this annotation applies to.
    ///
    /// These byte offsets index into the original UTF-8 source string and
    /// correspond to [`oxitext_core::PositionedGlyph::cluster`] values.
    pub base_range: std::ops::Range<usize>,
    /// The annotation text (e.g. a furigana pronunciation reading).
    pub ruby_text: String,
    /// Whether to place the annotation above or below the base text.
    pub position: RubyPosition,
}

/// The result of a ruby layout pass.
///
/// Contains the base glyphs (unchanged from input) alongside newly positioned
/// ruby glyphs that are centered over their base span.
#[derive(Debug, Clone)]
pub struct RubyLayout {
    /// Base glyphs positioned as normal (same as the input `base_glyphs`).
    pub base_glyphs: Vec<PositionedGlyph>,
    /// Ruby glyphs centered horizontally over the base span.
    ///
    /// These glyphs should be rendered at `(x, y + ruby_y_offset)` relative
    /// to the line baseline.
    pub ruby_glyphs: Vec<PositionedGlyph>,
    /// Y offset for ruby glyphs from the line baseline.
    ///
    /// Negative for [`RubyPosition::Above`] (ruby is above the baseline),
    /// positive for [`RubyPosition::Below`].
    pub ruby_y_offset: f32,
    /// Extra vertical space needed above (or below) the line to accommodate
    /// the ruby text without overlapping neighbouring lines.
    pub extra_line_height: f32,
}

/// Compute ruby layout given pre-shaped base glyphs and ruby glyphs.
///
/// # Arguments
///
/// - `base_glyphs` — already-shaped and positioned glyphs for the base text.
///   Must not be empty when `annotation.base_range` is non-empty; the function
///   will return an empty ruby result if `base_glyphs` is empty.
/// - `ruby_shaped` — already-shaped (but not yet positioned) glyphs for the
///   ruby text.  `x_advance` values are interpreted as pixel advances scaled
///   by `ruby_px_size` (i.e. the shaper was called with `ruby_px_size` as the
///   font size).
/// - `annotation` — the annotation metadata (base byte range + position).
/// - `ruby_px_size` — font size in pixels used to shape the ruby text.
///   Typically `base_px_size * 0.5`.
/// - `base_line_height` — height of the base line in pixels.  Used to compute
///   the y offset so that the ruby does not overlap the base glyphs.
///
/// # Returns
///
/// A [`RubyLayout`] containing the base glyphs unchanged and the ruby glyphs
/// positioned horizontally centred over the base span.
pub fn layout_ruby(
    base_glyphs: &[PositionedGlyph],
    ruby_shaped: &[ShapedGlyph],
    annotation: &RubyAnnotation,
    ruby_px_size: f32,
    base_line_height: f32,
) -> RubyLayout {
    // --- Step 1: find base glyphs that belong to annotation.base_range ---
    let range_start = annotation.base_range.start as u32;
    let range_end = annotation.base_range.end as u32;

    // Collect base glyphs whose cluster falls within [base_range.start, base_range.end)
    let span_glyphs: Vec<&PositionedGlyph> = base_glyphs
        .iter()
        .filter(|g| g.cluster >= range_start && g.cluster < range_end)
        .collect();

    // --- Step 2: compute x span of the base ---
    let (x_start, x_end) = if span_glyphs.is_empty() {
        // Fallback: use the start of the first base glyph or 0.0
        let fallback_x = base_glyphs.first().map_or(0.0, |g| g.pos.0);
        (fallback_x, fallback_x)
    } else {
        let x_start = span_glyphs.iter().map(|g| g.pos.0).fold(f32::MAX, f32::min);
        let x_end = span_glyphs
            .iter()
            .map(|g| g.pos.0 + g.advance_x)
            .fold(f32::MIN, f32::max);
        (x_start, x_end)
    };
    let span_width = x_end - x_start;

    // --- Step 3: compute total ruby width ---
    // x_advance values are already in pixels (shaped at ruby_px_size)
    let ruby_width: f32 = ruby_shaped.iter().map(|g| g.x_advance).sum();

    // --- Step 4: centre the ruby glyphs over the base span ---
    let ruby_start_x = x_start + (span_width - ruby_width) * 0.5;

    // --- Step 5 & 6: compute y offset based on position ---
    let (ruby_y_offset, extra_line_height) = match annotation.position {
        RubyPosition::Above => {
            let offset = -(base_line_height + ruby_px_size * 0.2);
            let extra = ruby_px_size * 1.2;
            (offset, extra)
        }
        RubyPosition::Below => {
            let offset = base_line_height + ruby_px_size * 0.2;
            let extra = ruby_px_size * 1.2;
            (offset, extra)
        }
    };

    // --- Step 7 & 8: position each ruby glyph ---
    // Derive font_data from the base glyphs if available, otherwise use an
    // empty placeholder (the caller should ensure base_glyphs is non-empty
    // for meaningful output).
    let ruby_font_data: Arc<[u8]> = base_glyphs
        .first()
        .map_or_else(|| Arc::from(&[][..]), |g| Arc::clone(&g.font_data));

    let mut ruby_glyphs = Vec::with_capacity(ruby_shaped.len());
    let mut ruby_x = ruby_start_x;

    for g in ruby_shaped {
        let advance = g.x_advance;
        ruby_glyphs.push(PositionedGlyph {
            gid: g.gid,
            font_data: Arc::clone(&ruby_font_data),
            pos: (ruby_x, ruby_y_offset),
            font_size: ruby_px_size,
            advance_x: advance,
            cluster: annotation.base_range.start as u32,
        });
        ruby_x += advance;
    }

    RubyLayout {
        base_glyphs: base_glyphs.to_vec(),
        ruby_glyphs,
        ruby_y_offset,
        extra_line_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxitext_core::ShapedGlyph;

    fn make_base_glyph(gid: u16, x: f32, advance: f32, cluster: u32) -> PositionedGlyph {
        PositionedGlyph {
            gid,
            font_data: Arc::from(&[][..]),
            pos: (x, 0.0),
            font_size: 16.0,
            advance_x: advance,
            cluster,
        }
    }

    fn make_ruby_shaped(gid: u16, x_advance: f32) -> ShapedGlyph {
        ShapedGlyph {
            gid,
            x_advance,
            ..Default::default()
        }
    }

    #[test]
    fn ruby_above_single_glyph_centered() {
        // Base: glyph at x=10, advance=20 (x span 10..30)
        // Ruby: glyph with advance=10 → should centre at x=15
        let base = vec![make_base_glyph(1, 10.0, 20.0, 0)];
        let ruby = vec![make_ruby_shaped(2, 10.0)];
        let ann = RubyAnnotation {
            base_range: 0..1,
            ruby_text: "ふ".into(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);

        assert_eq!(result.ruby_glyphs.len(), 1);
        // Expected: ruby_start_x = 10 + (20 - 10) / 2 = 15
        let rx = result.ruby_glyphs[0].pos.0;
        assert!(
            (rx - 15.0).abs() < 1e-4,
            "ruby x should be centred at 15.0, got {rx}"
        );
    }

    #[test]
    fn ruby_above_y_offset_is_negative() {
        let base = vec![make_base_glyph(1, 0.0, 20.0, 0)];
        let ruby = vec![make_ruby_shaped(2, 10.0)];
        let ann = RubyAnnotation {
            base_range: 0..1,
            ruby_text: "あ".into(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        assert!(
            result.ruby_y_offset < 0.0,
            "Above position should have negative y_offset, got {}",
            result.ruby_y_offset
        );
    }

    #[test]
    fn ruby_below_y_offset_is_positive() {
        let base = vec![make_base_glyph(1, 0.0, 20.0, 0)];
        let ruby = vec![make_ruby_shaped(2, 10.0)];
        let ann = RubyAnnotation {
            base_range: 0..1,
            ruby_text: "あ".into(),
            position: RubyPosition::Below,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        assert!(
            result.ruby_y_offset > 0.0,
            "Below position should have positive y_offset, got {}",
            result.ruby_y_offset
        );
    }

    #[test]
    fn extra_line_height_always_positive() {
        let base = vec![make_base_glyph(1, 0.0, 20.0, 0)];
        let ruby = vec![make_ruby_shaped(2, 10.0)];

        for pos in [RubyPosition::Above, RubyPosition::Below] {
            let ann = RubyAnnotation {
                base_range: 0..1,
                ruby_text: "x".into(),
                position: pos,
            };
            let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
            assert!(
                result.extra_line_height > 0.0,
                "extra_line_height should be positive"
            );
        }
    }

    #[test]
    fn ruby_glyphs_preserve_cluster() {
        let base = vec![make_base_glyph(1, 5.0, 30.0, 3)];
        let ruby = vec![make_ruby_shaped(2, 15.0), make_ruby_shaped(3, 15.0)];
        let ann = RubyAnnotation {
            base_range: 3..6,
            ruby_text: "ふに".into(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        for rg in &result.ruby_glyphs {
            assert_eq!(rg.cluster, 3, "ruby cluster should match base_range.start");
        }
    }

    #[test]
    fn ruby_glyphs_advance_incrementally() {
        // Two ruby glyphs of equal width → second should be advance further right
        let base = vec![make_base_glyph(1, 0.0, 40.0, 0)];
        let ruby = vec![make_ruby_shaped(2, 10.0), make_ruby_shaped(3, 10.0)];
        let ann = RubyAnnotation {
            base_range: 0..1,
            ruby_text: "ab".into(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        assert_eq!(result.ruby_glyphs.len(), 2);
        let x0 = result.ruby_glyphs[0].pos.0;
        let x1 = result.ruby_glyphs[1].pos.0;
        assert!(
            (x1 - x0 - 10.0).abs() < 1e-4,
            "second ruby glyph should be 10px right of first; got x0={x0}, x1={x1}"
        );
    }

    #[test]
    fn base_glyphs_unchanged() {
        let base = vec![
            make_base_glyph(1, 0.0, 20.0, 0),
            make_base_glyph(2, 20.0, 20.0, 2),
        ];
        let ruby = vec![make_ruby_shaped(3, 10.0)];
        let ann = RubyAnnotation {
            base_range: 0..2,
            ruby_text: "x".into(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        assert_eq!(result.base_glyphs.len(), 2);
        assert_eq!(result.base_glyphs[0].pos.0, 0.0);
        assert_eq!(result.base_glyphs[1].pos.0, 20.0);
    }

    #[test]
    fn empty_ruby_shaped() {
        // No ruby glyphs → empty ruby_glyphs, but layout should not panic
        let base = vec![make_base_glyph(1, 0.0, 20.0, 0)];
        let ruby: Vec<ShapedGlyph> = vec![];
        let ann = RubyAnnotation {
            base_range: 0..1,
            ruby_text: String::new(),
            position: RubyPosition::Above,
        };
        let result = layout_ruby(&base, &ruby, &ann, 8.0, 20.0);
        assert!(result.ruby_glyphs.is_empty());
    }
}
