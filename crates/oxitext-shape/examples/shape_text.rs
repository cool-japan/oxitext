//! Demonstrates the primary shaping flow of `oxitext-shape`: turning a
//! Unicode string into positioned glyphs via [`SwashShaper`], plus the
//! script-detection helpers that tell a caller which complex-script code
//! paths a run of text needs, and OpenType variation-axis shaping for
//! variable fonts.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-shape --example shape_text
//! ```

use oxitext_shape::{requires_arabic_shaping, requires_mark_positioning, SwashShaper};

/// Font bytes embedded at compile time from the workspace's checked-in test
/// fixture. Resolved relative to this source file, so it works regardless of
/// the process's current working directory.
const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

/// A synthetic variable-font fixture with a single `wght` axis (400..900),
/// used to demonstrate [`SwashShaper::shape_with_variations`].
const VARIABLE_FONT: &[u8] = include_bytes!("../../../tests/fixtures/variable_wght.ttf");

fn main() {
    let mut shaper = SwashShaper::new();

    // ─── 1. Basic shaping: text → glyphs ────────────────────────────────────

    let text = "OxiText";
    let px_size = 32.0;
    let result = shaper
        .shape_full(FONT, text, px_size)
        .expect("shaping the bundled test font must succeed");

    println!("shaped {text:?} into {} glyph(s):", result.glyphs.len());
    let mut pen_x = 0.0f32;
    for g in &result.glyphs {
        println!(
            "  gid={:<4} cluster={:<3} advance={:.2}px pen_x={:.2}px",
            g.gid, g.cluster, g.x_advance, pen_x
        );
        pen_x += g.x_advance;
    }
    assert_eq!(result.glyphs.len(), text.chars().count());
    assert!(
        result.missing_codepoints.is_empty(),
        "the bundled ASCII test font should cover every character of {text:?}"
    );

    // ─── 2. Script-detection helpers ────────────────────────────────────────
    //
    // These flag which complex-script GSUB/GPOS behaviour a run of text
    // needs; `oxitext-layout`'s bidi/itemization stage uses them to decide
    // per-run shaping direction and feature sets before handing runs to the
    // shaper.

    for sample in [
        "Hello",
        "\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645}",
        "na\u{0301}ive",
    ] {
        println!(
            "requires_arabic_shaping({sample:?}) = {}, requires_mark_positioning({sample:?}) = {}",
            requires_arabic_shaping(sample),
            requires_mark_positioning(sample)
        );
    }
    assert!(requires_arabic_shaping(
        "\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645}"
    ));
    assert!(!requires_arabic_shaping("Hello"));

    // ─── 3. Variation-axis shaping on a variable font ───────────────────────
    //
    // `shape_with_variations` threads `(axis_tag, value)` pairs into swash's
    // shaper builder, so the same glyph shaped at two different `wght`
    // values on a variable font reports two different advances (see
    // `oxitext-shape/src/variational.rs` for the regression test this
    // mirrors).

    let light = shaper
        .shape_with_variations(VARIABLE_FONT, "A", 1000.0, &[(*b"wght", 400.0)])
        .expect("shape at wght=400");
    let bold = shaper
        .shape_with_variations(VARIABLE_FONT, "A", 1000.0, &[(*b"wght", 900.0)])
        .expect("shape at wght=900");
    println!(
        "variable font 'A': wght=400 advance={:.1}, wght=900 advance={:.1}",
        light.glyphs[0].x_advance, bold.glyphs[0].x_advance
    );
    assert!(
        bold.glyphs[0].x_advance > light.glyphs[0].x_advance,
        "a heavier weight should be at least as wide on this fixture"
    );
}
