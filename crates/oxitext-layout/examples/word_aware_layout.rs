//! Demonstrates the primary word-aware layout flow of `oxitext-layout`.
//!
//! [`LayoutEngine`] is the M6 entry point: it takes pre-shaped glyph runs
//! (as produced by a shaper such as `oxitext-shape`) together with the
//! source text they were shaped from, wraps them at UAX #14 line-break
//! opportunities, applies horizontal alignment, and returns a
//! [`LayoutResult`] with per-line and per-paragraph metrics.
//!
//! This example builds [`ShapedRun`]/[`ShapedGlyph`] values by hand (a real
//! shaper would produce these), lays out a short paragraph that must wrap,
//! then walks the resulting lines. It also shows the hand-off point to an
//! SDF atlas: [`LayoutResult::unique_glyphs_for_atlas`] enumerates exactly
//! the `(glyph_id, px_size)` pairs a rasterizer or SDF atlas needs to
//! pre-warm before drawing this layout (see the `oxitext-sdf` crate's
//! `glyph_to_sdf_atlas` example for the SDF side of that hand-off).
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-layout --example word_aware_layout
//! ```

use oxitext_core::{LayoutConstraints, ShapedGlyph, ShapedRun, TextAlignment};
use oxitext_layout::LayoutEngine;
use std::sync::Arc;

/// Builds a synthetic [`ShapedRun`] whose glyphs correspond 1:1 to the
/// characters of `text`, each advancing the cursor by `advance` pixels.
///
/// `cluster` offsets are the UTF-8 byte offset of each character within
/// `text`, matching the convention a real shaper (e.g. `oxitext-shape`)
/// uses so that the layout engine can map glyphs back to source-text byte
/// ranges for line breaking.
fn shaped_run_from_text(text: &str, advance: f32) -> ShapedRun {
    let glyphs: Vec<ShapedGlyph> = text
        .char_indices()
        .enumerate()
        .map(|(i, (byte_idx, ch))| ShapedGlyph {
            // Glyph 0 is usually `.notdef`; offset by one to avoid it.
            gid: (i + 1) as u16,
            x_advance: advance,
            cluster: byte_idx as u32,
            is_whitespace: ch.is_whitespace(),
            ..Default::default()
        })
        .collect();
    ShapedRun {
        glyphs: glyphs.into(),
        // A real pipeline stores the font bytes here so downstream stages
        // (rasterizer, SDF atlas) can look up glyph outlines.
        font_data: Arc::from(&[][..]),
    }
}

fn main() {
    let text = "The quick brown fox jumps";
    let run = shaped_run_from_text(text, 12.0);

    // Wrap at 120px — narrow enough that the paragraph spans several lines.
    let constraints = LayoutConstraints {
        max_width: 120.0,
        font_size: 16.0,
    };

    let mut engine = LayoutEngine::new();
    let result = engine
        .layout(text, &[run], &constraints, TextAlignment::Left, None)
        .expect("layout is currently infallible for well-formed input");

    println!(
        "laid out {} glyph(s) into {} line(s); paragraph size = {:.1} x {:.1}px",
        result.glyphs.len(),
        result.lines.len(),
        result.metrics.total_width,
        result.metrics.total_height,
    );
    assert!(
        result.lines.len() > 1,
        "narrow max_width should force wraps"
    );

    for (i, line) in result.lines.iter().enumerate() {
        let glyphs = &result.glyphs[line.glyph_start..line.glyph_end];
        let first_x = glyphs.first().map(|g| g.pos.0).unwrap_or(0.0);
        println!(
            "  line {i}: {} glyph(s), starts at x={first_x:.1}, width={:.1}px",
            line.len(),
            line.metrics.width,
        );
        // Every wrapped line must restart at the left edge.
        assert!((first_x - 0.0).abs() < 1e-3);
    }

    // Hand-off to a rasterizer / SDF atlas: the unique (glyph_id, px_size)
    // pairs actually used by this layout, in first-occurrence order.
    let glyph_set = result.unique_glyphs_for_atlas();
    println!(
        "{} unique glyph(s) needed for rasterization",
        glyph_set.len()
    );
    assert!(!glyph_set.is_empty());
}
