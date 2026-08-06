//! End-to-end walk through the `oxitext` facade: the same flow shown in the
//! crate's README Quick Start, kept honest by being compiled and run as part
//! of `cargo build --examples` / `cargo run --example quick_start`.
//!
//! Demonstrates, in order:
//! 1. [`Pipeline::from_bytes`] — load a font.
//! 2. [`Pipeline::measure`] — shape + lay out text without rasterizing, to
//!    get paragraph metrics (total width/height, line count).
//! 3. [`Pipeline::render`] — the full shape → layout → rasterize pipeline,
//!    returning per-glyph and per-line data.
//! 4. [`Pipeline::render_to_image`] — the same pipeline composited onto an
//!    RGBA canvas, ready to blit or write out as an image.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext --example quick_start
//! ```

use oxitext::prelude::*;
use oxitext::Pipeline;

/// Font bytes embedded at compile time from the workspace's checked-in test
/// fixture. Resolved relative to this source file, so it works regardless of
/// the process's current working directory.
const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

fn main() -> Result<(), OxiTextError> {
    let mut pipeline = Pipeline::from_bytes(FONT)?;

    // ─── 1. Measure a string without rasterizing ───────────────────────────

    let style = TextStyle::default().with_font_size(24.0);
    let metrics = pipeline.measure("Hello, world!", &style)?;
    println!(
        "measure: {:.1}x{:.1} px, {} line(s), overflow={}",
        metrics.total_width, metrics.total_height, metrics.line_count, metrics.overflow
    );
    assert!(metrics.total_width > 0.0, "non-empty text must have width");
    assert_eq!(
        metrics.line_count, 1,
        "short text with no wrap should be one line"
    );

    // ─── 2. Full render: per-glyph positions + per-line layout data ────────

    let text = "Hello, OxiText!";
    let result = pipeline.render(text, &style)?;
    println!(
        "render: {} glyph(s) across {} line(s)",
        result.glyphs.len(),
        result.lines.len()
    );
    assert_eq!(result.glyphs.len(), result.bitmaps.len());
    assert_eq!(result.glyphs.len(), result.outputs.len());
    for (i, line) in result.lines.iter().enumerate() {
        println!(
            "  line {i}: glyphs [{}, {}), width={:.1}px",
            line.glyph_start, line.glyph_end, line.metrics.width
        );
    }

    // ─── 3. Render straight to an RGBA canvas ───────────────────────────────

    let bg = Rgba8 {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    let fg = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    let image = pipeline.render_to_image(text, &style, bg, fg)?;
    let expected_len = (image.width * image.height * 4) as usize;
    println!(
        "render_to_image: {}x{} RGBA pixels ({} bytes)",
        image.width,
        image.height,
        image.rgba.len()
    );
    assert_eq!(image.rgba.len(), expected_len);
    let painted_pixels = image.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
    assert!(
        painted_pixels > 0,
        "rendered text must paint at least one pixel"
    );

    // ─── 4. A couple of bonus facade queries ───────────────────────────────

    println!("has_rtl(\"Hello\") = {}", pipeline.has_rtl("Hello"));
    println!(
        "has_rtl(\"שלום\") = {}",
        pipeline.has_rtl("\u{5E9}\u{5DC}\u{5D5}\u{5DD}")
    );
    if let Some(fm) = pipeline.font_metrics() {
        println!(
            "font metrics (design units, upem={}): ascender={} descender={} line_gap={}",
            fm.units_per_em, fm.ascender, fm.descender, fm.line_gap
        );
    }

    Ok(())
}
