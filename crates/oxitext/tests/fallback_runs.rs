//! Per-font run splitting on `.notdef` fallback (0.2.3).
//!
//! Until 0.2.3 `shape_run_with_notdef_fallback` shaped into ONE `ShapedRun`
//! and, whenever any glyph fell back, overwrote that run's `font_data` with
//! the fallback's. Since a `ShapedRun` carries one font for all its glyphs,
//! this re-pointed the glyphs the PRIMARY had resolved too: they kept the
//! primary's glyph ids but were now attributed to the fallback face. A caller
//! rasterising per `(font_data, gid)` therefore drew the primary's ids out of
//! the fallback's outline table — wrong letters, not wrong metrics.
//!
//! The defect was invisible with the usual font pairings, because Noto Sans,
//! Meiryo, MS Gothic, MS Mincho and Yu Gothic all number basic Latin
//! identically. These tests use bundled faces only, so they are deterministic
//! and need no system fonts.

use oxitext::{Pipeline, TextStyle};

/// Primary: Noto Sans Regular. Does NOT cover U+221E INFINITY.
fn primary() -> Vec<u8> {
    oxifont_bundled::NOTO_SANS_REGULAR.to_vec()
}

/// Fallback: Noto Sans Mono Regular — a genuinely DIFFERENT face that does
/// cover U+221E, so the fallback path is actually taken.
fn fallback() -> Vec<u8> {
    oxifont_bundled::NOTO_SANS_MONO_REGULAR.to_vec()
}

/// The `(font pointer, gid)` pairs a string lays out to, in logical order.
fn shaped(pipeline: &mut Pipeline, text: &str) -> Vec<(usize, u16)> {
    let style = TextStyle::default();
    let layout = pipeline.shape_and_layout(text, &style).expect("lays out");
    layout
        .glyphs
        .iter()
        .map(|glyph| (glyph.font_data.as_ptr() as usize, glyph.gid))
        .collect()
}

#[test]
fn the_fallback_is_actually_needed_for_this_fixture() {
    // The premise every other test here rests on. If Noto Sans ever gains
    // U+221E, these tests stop exercising the fallback path at all and would
    // pass for the wrong reason.
    let primary_face = ttf_parser::Face::parse(oxifont_bundled::NOTO_SANS_REGULAR, 0)
        .expect("bundled Noto Sans parses");
    let fallback_face = ttf_parser::Face::parse(oxifont_bundled::NOTO_SANS_MONO_REGULAR, 0)
        .expect("bundled Noto Sans Mono parses");
    assert!(
        primary_face.glyph_index('\u{221E}').is_none(),
        "the primary must NOT cover U+221E, or no fallback happens",
    );
    assert!(
        fallback_face.glyph_index('\u{221E}').is_some(),
        "the fallback must cover U+221E",
    );
}

#[test]
fn a_mixed_run_keeps_primary_glyphs_on_the_primary_font() {
    let mut pipeline = Pipeline::from_bytes(&primary()).expect("valid font");

    // Latin-only, no fallback registered: the baseline gids and the primary's
    // font pointer.
    let latin_only = shaped(&mut pipeline, "AB");
    assert_eq!(latin_only.len(), 2, "two Latin glyphs: {latin_only:?}");
    let primary_ptr = latin_only[0].0;
    let latin_gids: Vec<u16> = latin_only.iter().map(|(_, gid)| *gid).collect();

    pipeline.set_fallback_fonts(vec![fallback()]);
    let mixed = shaped(&mut pipeline, "AB\u{221E}");
    assert_eq!(mixed.len(), 3, "three glyphs: {mixed:?}");

    // THE FIX. The two Latin glyphs must still belong to the PRIMARY, with
    // the primary's gids; only the infinity sign moves to the fallback.
    assert_eq!(mixed[0].0, primary_ptr, "'A' must stay on the primary font",);
    assert_eq!(mixed[1].0, primary_ptr, "'B' must stay on the primary font",);
    assert_ne!(
        mixed[2].0, primary_ptr,
        "U+221E must be drawn from the fallback font",
    );
    assert_eq!(
        mixed[..2].iter().map(|(_, gid)| *gid).collect::<Vec<u16>>(),
        latin_gids,
        "the Latin gids must be unchanged by the presence of a fallback",
    );
}

#[test]
fn a_leading_fallback_glyph_does_not_capture_the_rest_of_the_run() {
    // The mirror case: the fallback character comes FIRST. The old run-level
    // overwrite was order independent — it claimed the whole run either way.
    let mut pipeline = Pipeline::from_bytes(&primary()).expect("valid font");
    let latin_only = shaped(&mut pipeline, "AB");
    let primary_ptr = latin_only[0].0;

    pipeline.set_fallback_fonts(vec![fallback()]);
    let mixed = shaped(&mut pipeline, "\u{221E}AB");
    assert_eq!(mixed.len(), 3, "three glyphs: {mixed:?}");
    assert_ne!(mixed[0].0, primary_ptr, "U+221E comes from the fallback");
    assert_eq!(mixed[1].0, primary_ptr, "'A' must stay on the primary");
    assert_eq!(mixed[2].0, primary_ptr, "'B' must stay on the primary");
}

#[test]
fn a_run_needing_no_fallback_is_untouched() {
    // The fast path, which must remain byte-identical to pre-0.2.3: with a
    // fallback registered but nothing to fall back to, every glyph stays on
    // the primary and the result matches the no-fallback shaping exactly.
    let mut pipeline = Pipeline::from_bytes(&primary()).expect("valid font");
    let before = shaped(&mut pipeline, "Hello world");
    pipeline.set_fallback_fonts(vec![fallback()]);
    let after = shaped(&mut pipeline, "Hello world");
    assert_eq!(
        before, after,
        "registering an unused fallback must not change shaping",
    );
}

#[test]
fn an_unresolvable_character_stays_on_the_primary_as_notdef() {
    // Neither face covers U+10FFFD (a Plane 16 private-use character), so no
    // fallback wins. The glyph must remain the primary's `.notdef` rather than
    // being attributed to some fallback that could not draw it either.
    let mut pipeline = Pipeline::from_bytes(&primary()).expect("valid font");
    let latin_only = shaped(&mut pipeline, "A");
    let primary_ptr = latin_only[0].0;

    pipeline.set_fallback_fonts(vec![fallback()]);
    let mixed = shaped(&mut pipeline, "A\u{10FFFD}");
    assert!(
        mixed.iter().all(|(ptr, _)| *ptr == primary_ptr),
        "an unresolvable character must not re-point the run: {mixed:?}",
    );
}

#[test]
fn every_run_font_is_one_the_caller_supplied() {
    // A structural invariant worth pinning: the layout never invents a font.
    // Every glyph must come from either the primary or a registered fallback.
    let mut pipeline = Pipeline::from_bytes(&primary()).expect("valid font");
    pipeline.set_fallback_fonts(vec![fallback()]);
    let mixed = shaped(&mut pipeline, "AB\u{221E}CD\u{221E}");
    let distinct: std::collections::BTreeSet<usize> = mixed.iter().map(|(ptr, _)| *ptr).collect();
    assert_eq!(
        distinct.len(),
        2,
        "exactly two faces should appear — primary and one fallback: {mixed:?}",
    );
}
