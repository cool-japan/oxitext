//! Demonstrates `oxitext-core`'s value types and their builder APIs.
//!
//! `oxitext-core` has no shaping/layout/rasterization logic of its own — it
//! is the shared vocabulary the rest of the OxiText pipeline (`oxitext-shape`,
//! `oxitext-layout`, `oxitext-raster`, and the `oxitext` facade) passes
//! between stages. This example builds each of the main style/metric types
//! by hand the way a caller assembling a custom pipeline would, and exercises
//! the `Hash`/`Eq` derives that let enum-like style types (e.g.
//! [`FlowDirection`], [`TextAlignment`]) key a cache.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-core --example build_text_styles
//! ```

use oxitext_core::{
    Decoration, DecorationLine, FlowDirection, ParagraphStyle, Rgba8, TextAlignment, TextStyle,
};
use std::collections::HashSet;

fn main() {
    // ─── 1. TextStyle via its builder methods ───────────────────────────────

    let style = TextStyle::default()
        .with_font_size(18.0)
        .with_max_width(400.0)
        .with_alignment(TextAlignment::Justify);
    println!(
        "TextStyle: font_size={} max_width={} alignment={:?}",
        style.font_size, style.max_width, style.alignment
    );
    assert_eq!(style.font_size, 18.0);
    assert_eq!(style.alignment, TextAlignment::Justify);

    // ─── 2. ParagraphStyle: indent + spacing on top of an alignment ────────

    let para = ParagraphStyle {
        alignment: TextAlignment::Center,
        indent: 24.0,
        spacing_before: 8.0,
        spacing_after: 8.0,
        direction: FlowDirection::Horizontal,
        ..ParagraphStyle::default()
    };
    println!(
        "ParagraphStyle: alignment={:?} indent={} spacing=({}, {})",
        para.alignment, para.indent, para.spacing_before, para.spacing_after
    );

    // ─── 3. Decoration: underline + strikethrough with distinct colors ─────

    let red = Rgba8 {
        r: 220,
        g: 40,
        b: 40,
        a: 255,
    };
    let black = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    let decoration = Decoration {
        underline: Some(DecorationLine {
            position: -2.0,
            thickness: 1.5,
            color: black,
        }),
        overline: None,
        strikethrough: Some(DecorationLine {
            position: 6.0,
            thickness: 1.5,
            color: red,
        }),
    };
    println!(
        "Decoration::any() = {} (underline={}, overline={}, strikethrough={})",
        decoration.any(),
        decoration.underline.is_some(),
        decoration.overline.is_some(),
        decoration.strikethrough.is_some()
    );
    assert!(decoration.any());
    assert!(!Decoration::default().any());

    // ─── 4. Hash + Eq on style enums: dedupe a batch of layout requests ────
    //
    // A real cache (e.g. a shape cache keyed on `(font, text, style-ish
    // bits)`) relies on exactly this: `FlowDirection`/`TextAlignment` are
    // `Hash + Eq` so they can appear directly in a key without being converted
    // to an integer tag first.
    let mut seen_combinations: HashSet<(FlowDirection, TextAlignment)> = HashSet::new();
    let requests = [
        (FlowDirection::Horizontal, TextAlignment::Left),
        (FlowDirection::Horizontal, TextAlignment::Left), // duplicate
        (FlowDirection::Horizontal, TextAlignment::Center),
        (FlowDirection::Vertical, TextAlignment::Left),
    ];
    for req in requests {
        seen_combinations.insert(req);
    }
    println!(
        "{} requests collapsed to {} unique (flow, alignment) combinations",
        requests.len(),
        seen_combinations.len()
    );
    assert_eq!(
        seen_combinations.len(),
        3,
        "the duplicate request must collapse away"
    );
}
