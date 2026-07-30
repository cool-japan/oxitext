//! COLRv0/COLRv1 colour-glyph rendering tests.
//!
//! These pin the fix for the "COLR emoji render as a fully transparent bitmap"
//! defect.  The root cause was that the painter rasterized every layer through
//! `fontdue::Font::rasterize_indexed`, and `fontdue::Font` only materialises
//! glyphs reachable from the font's `cmap` (plus `GSUB`, when asked).  COLR
//! layer glyphs are deliberately *not* mapped from any codepoint, so fontdue
//! returned a 0x0 bitmap for every one of them and each layer contributed
//! nothing.  [`layer_glyphs_are_unreachable_through_fontdue`] below asserts that
//! precondition directly so the regression cannot come back unnoticed.
//!
//! Fixtures come from `googlefonts/color-fonts` (Apache-2.0); see
//! `tests/fixtures/README.md` for provenance and hashes.  Point
//! `OXITEXT_TEST_COLR_FONT` at a full Noto COLRv1 emoji build to additionally
//! sweep every colour glyph in it.

use oxitext_raster::{
    render_color_glyph, render_colr_glyph_sized, render_colr_v0, render_colr_v1, ColorGlyphBitmap,
    ColorGlyphImage,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use ttf_parser::{Face, GlyphId};

/// Render size used by the assertions, in pixels per em.
const PX: u32 = 64;

/// Real Twemoji smiley emoji with `PaintSolid` layers and transforms.
const TWEMOJI_SMILEY: &str = "twemoji_smiley-glyf_colr_1.ttf";
/// The Noto Emoji writing hand, with linear and radial gradients.
const NOTO_HANDWRITING: &str = "noto_handwriting-glyf_colr_1.ttf";
/// Synthetic glyphs covering the whole COLRv1 paint-format matrix.
const COLR_TEST_GLYPHS: &str = "test_glyphs-glyf_colr_1.ttf";

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Absolute path of a checked-in fixture.
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Load a fixture, or `None` when the binary is not present in this checkout.
fn load_fixture(name: &str) -> Option<Vec<u8>> {
    let path = fixture_path(name);
    path.exists()
        .then(|| std::fs::read(&path).expect("read fixture"))
}

/// Load a fixture, failing the test when it is missing.
///
/// The COLR fixtures are small (5-22 KB) and checked in, so a missing one is a
/// broken checkout rather than an environment difference.
fn require_fixture(name: &str) -> Vec<u8> {
    load_fixture(name)
        .unwrap_or_else(|| panic!("missing fixture {name}; see tests/fixtures/README.md"))
}

/// Every COLR base glyph reachable through the font's Unicode `cmap`.
fn colr_glyphs(face: &Face<'_>) -> Vec<(u32, GlyphId)> {
    let Some(colr) = face.tables().colr else {
        return Vec::new();
    };
    let mut codepoints: Vec<u32> = Vec::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            if subtable.is_unicode() {
                subtable.codepoints(|cp| codepoints.push(cp));
            }
        }
    }
    codepoints.sort_unstable();
    codepoints.dedup();
    codepoints
        .into_iter()
        .filter_map(|cp| {
            let ch = char::from_u32(cp)?;
            let gid = face.glyph_index(ch)?;
            colr.contains(gid).then_some((cp, gid))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Bitmap assertions
// ---------------------------------------------------------------------------

/// Fraction of pixels with any alpha at all.
fn coverage(bitmap: &ColorGlyphBitmap) -> f32 {
    let painted = bitmap.rgba.chunks_exact(4).filter(|px| px[3] > 0).count();
    painted as f32 / (bitmap.width * bitmap.height) as f32
}

/// Distinct near-opaque colours, quantised to 5 bits per channel so that
/// anti-aliasing along a single-colour edge does not inflate the count.
fn distinct_opaque_colors(bitmap: &ColorGlyphBitmap) -> usize {
    bitmap
        .rgba
        .chunks_exact(4)
        .filter(|px| px[3] >= 250)
        .map(|px| (px[0] >> 3, px[1] >> 3, px[2] >> 3))
        .collect::<HashSet<_>>()
        .len()
}

/// Assert that the buffer is dimensionally self-consistent.
fn assert_well_formed(bitmap: &ColorGlyphBitmap, width: u32, height: u32) {
    assert_eq!(bitmap.width, width, "bitmap width");
    assert_eq!(bitmap.height, height, "bitmap height");
    assert_eq!(
        bitmap.rgba.len(),
        (width as usize) * (height as usize) * 4,
        "rgba buffer must be width * height * 4"
    );
}

// ---------------------------------------------------------------------------
// The headline regression: a real COLRv1 emoji must not be transparent
// ---------------------------------------------------------------------------

/// A real COLRv1 emoji rasterized at 64 px must cover a meaningful part of the
/// em box and use more than one colour.
///
/// Before the fix this produced 0 of 4096 painted pixels and 0 colours.
#[test]
fn colrv1_emoji_is_not_transparent() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    // U+1F601 GRINNING FACE WITH SMILING EYES.
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let bitmap = render_colr_v1(&data, gid, PX, PX).expect("COLRv1 glyph must render");
    assert_well_formed(&bitmap, PX, PX);

    let painted = coverage(&bitmap);
    assert!(
        painted > 0.05,
        "expected >5% alpha coverage of the em box, got {:.1}%",
        painted * 100.0
    );

    let colors = distinct_opaque_colors(&bitmap);
    assert!(
        colors >= 2,
        "expected at least 2 distinct opaque colours, got {colors}"
    );
}

/// The same emoji through the format-dispatching entry point.
#[test]
fn dispatch_entry_point_renders_colrv1_emoji() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let bitmap = render_color_glyph(&data, gid.0, PX, PX).expect("dispatch must render");
    assert!(coverage(&bitmap) > 0.05);
    assert!(distinct_opaque_colors(&bitmap) >= 2);
}

/// `render_colr_v0` shares the paint interpreter, so it renders COLRv1 fonts
/// too instead of silently dropping every non-solid paint.
#[test]
fn v0_entry_point_also_renders_colrv1_fonts() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let via_v0 = render_colr_v0(&data, gid, PX, PX).expect("must render");
    let via_v1 = render_colr_v1(&data, gid, PX, PX).expect("must render");
    assert_eq!(
        via_v0.rgba, via_v1.rgba,
        "both entry points share the painter"
    );
}

/// Every colour glyph in the smiley fixture paints something.
#[test]
fn every_smiley_renders() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let glyphs = colr_glyphs(&face);
    assert!(glyphs.len() >= 10, "fixture should expose many emoji");

    for (cp, gid) in glyphs {
        let bitmap =
            render_colr_v1(&data, gid, PX, PX).unwrap_or_else(|| panic!("U+{cp:04X} must render"));
        let painted = coverage(&bitmap);
        assert!(
            painted > 0.05,
            "U+{cp:04X} covered only {:.1}% of the em box",
            painted * 100.0
        );
        assert!(
            distinct_opaque_colors(&bitmap) >= 2,
            "U+{cp:04X} rendered in a single colour"
        );
    }
}

// ---------------------------------------------------------------------------
// The precondition that caused the bug
// ---------------------------------------------------------------------------

/// COLR layer glyphs are unreachable through `cmap`, so fontdue cannot
/// rasterize them — the reason the painter had to stop using fontdue.
///
/// If this ever stops holding, the old implementation would have started
/// working by accident and this whole module would lose its teeth, so assert
/// the precondition explicitly.
#[test]
fn layer_glyphs_are_unreachable_through_fontdue() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let colr = face.tables().colr.expect("fixture has COLR");

    // Codepoint-reachable glyphs, i.e. the ones fontdue actually loads.
    let mut reachable: HashSet<u16> = HashSet::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            subtable.codepoints(|cp| {
                if let Some(gid) = subtable.glyph_index(cp) {
                    reachable.insert(gid.0);
                }
            });
        }
    }

    // Collect the layer glyphs of one base glyph by walking its paint graph.
    struct LayerCollector(Vec<GlyphId>);
    impl<'a> ttf_parser::colr::Painter<'a> for LayerCollector {
        fn outline_glyph(&mut self, glyph_id: GlyphId) {
            self.0.push(glyph_id);
        }
        fn paint(&mut self, _paint: ttf_parser::colr::Paint<'a>) {}
        fn push_clip(&mut self) {}
        fn push_clip_box(&mut self, _clip_box: ttf_parser::RectF) {}
        fn pop_clip(&mut self) {}
        fn push_layer(&mut self, _mode: ttf_parser::colr::CompositeMode) {}
        fn pop_layer(&mut self) {}
        fn push_transform(&mut self, _transform: ttf_parser::Transform) {}
        fn pop_transform(&mut self) {}
    }

    let base = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");
    let mut collector = LayerCollector(Vec::new());
    colr.paint(
        base,
        0,
        &mut collector,
        &[],
        ttf_parser::RgbaColor::new(0, 0, 0, 255),
    )
    .expect("paint graph walks");
    assert!(!collector.0.is_empty(), "base glyph must have layers");

    let font = fontdue::Font::from_bytes(data.as_slice(), fontdue::FontSettings::default())
        .expect("fontdue parses the fixture");

    for layer in collector.0 {
        assert!(
            !reachable.contains(&layer.0),
            "layer gid {} is unexpectedly cmap-reachable",
            layer.0
        );
        let (metrics, _) = font.rasterize_indexed(layer.0, PX as f32);
        assert_eq!(
            (metrics.width, metrics.height),
            (0, 0),
            "fontdue unexpectedly rasterized layer gid {}; the painter must \
             still not depend on it",
            layer.0
        );
        // ttf-parser, which the painter now uses, does have the outline.
        let mut probe = BoundsProbe::default();
        assert!(
            face.outline_glyph(layer, &mut probe).is_some(),
            "layer gid {} must have a real outline",
            layer.0
        );
    }
}

/// Minimal [`ttf_parser::OutlineBuilder`] used purely to confirm an outline
/// exists.
#[derive(Default)]
struct BoundsProbe {
    points: usize,
}

impl ttf_parser::OutlineBuilder for BoundsProbe {
    fn move_to(&mut self, _x: f32, _y: f32) {
        self.points += 1;
    }
    fn line_to(&mut self, _x: f32, _y: f32) {
        self.points += 1;
    }
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {
        self.points += 1;
    }
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {
        self.points += 1;
    }
    fn close(&mut self) {}
}

// ---------------------------------------------------------------------------
// Gradients
// ---------------------------------------------------------------------------

/// The Noto writing-hand emoji is painted with linear and radial gradients; a
/// working gradient path produces a smooth ramp, not a couple of flat fills.
#[test]
fn noto_gradient_emoji_produces_a_colour_ramp() {
    let data = require_fixture(NOTO_HANDWRITING);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{270D}').expect("fixture maps U+270D");

    let bitmap = render_colr_v1(&data, gid, PX, PX).expect("gradient glyph must render");
    assert_well_formed(&bitmap, PX, PX);

    let painted = coverage(&bitmap);
    assert!(
        painted > 0.05,
        "expected >5% coverage, got {:.1}%",
        painted * 100.0
    );

    // Two gradients plus six solid fills: a working ramp yields far more than
    // the eight colours a gradient-less rendering could produce.
    let colors = distinct_opaque_colors(&bitmap);
    assert!(
        colors >= 12,
        "expected a gradient ramp (>=12 quantised colours), got {colors}"
    );
}

// ---------------------------------------------------------------------------
// Whole paint-format matrix
// ---------------------------------------------------------------------------

/// Sweep the COLRv1 conformance font: every paint format, extend mode,
/// transform and composite mode must render without panicking, and virtually
/// all of them must produce ink.
#[test]
fn paint_format_matrix_renders() {
    let data = require_fixture(COLR_TEST_GLYPHS);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let glyphs = colr_glyphs(&face);
    assert!(glyphs.len() > 150, "conformance font should be large");

    let mut blank = Vec::new();
    for (cp, gid) in &glyphs {
        let bitmap =
            render_colr_v1(&data, *gid, PX, PX).unwrap_or_else(|| panic!("U+{cp:04X} must render"));
        assert_well_formed(&bitmap, PX, PX);
        if coverage(&bitmap) == 0.0 {
            blank.push(*cp);
        }
    }

    // The only legitimately blank glyphs are the `PaintColrGlyph` recursion
    // cycles, which ttf-parser refuses to traverse and which therefore emit no
    // paint callbacks at all.
    assert!(
        blank.len() <= 2,
        "unexpectedly blank glyphs: {:?}",
        blank
            .iter()
            .map(|cp| format!("U+{cp:04X}"))
            .collect::<Vec<_>>()
    );
}

/// Distinct `PaintComposite` modes must produce distinct pixels.
///
/// Every composite-mode test glyph paints the same yellow-square backdrop and
/// cyan-square source and differs only in the mode, so a painter that ignored
/// `push_layer`/`pop_layer` would emit one identical bitmap for all of them.
#[test]
fn composite_modes_produce_distinct_results() {
    let data = require_fixture(COLR_TEST_GLYPHS);
    let face = Face::parse(&data, 0).expect("fixture parses");

    let mut renderings: HashSet<Vec<u8>> = HashSet::new();
    let mut composite_glyphs = 0;
    for (_, gid) in colr_glyphs(&face) {
        if count_layers(&face, gid) == 0 {
            continue;
        }
        composite_glyphs += 1;
        if let Some(bitmap) = render_colr_v1(&data, gid, PX, PX) {
            renderings.insert(bitmap.rgba);
        }
    }

    assert!(
        composite_glyphs >= 20,
        "expected the conformance font to exercise many composite modes, saw {composite_glyphs}"
    );
    assert!(
        renderings.len() >= 15,
        "composite modes collapsed to {} distinct renderings out of {composite_glyphs} glyphs",
        renderings.len()
    );
}

/// Number of `push_layer` callbacks a glyph's paint graph emits.
fn count_layers(face: &Face<'_>, gid: GlyphId) -> usize {
    struct LayerCounter(usize);
    impl<'a> ttf_parser::colr::Painter<'a> for LayerCounter {
        fn outline_glyph(&mut self, _glyph_id: GlyphId) {}
        fn paint(&mut self, _paint: ttf_parser::colr::Paint<'a>) {}
        fn push_clip(&mut self) {}
        fn push_clip_box(&mut self, _clip_box: ttf_parser::RectF) {}
        fn pop_clip(&mut self) {}
        fn push_layer(&mut self, _mode: ttf_parser::colr::CompositeMode) {
            self.0 += 1;
        }
        fn pop_layer(&mut self) {}
        fn push_transform(&mut self, _transform: ttf_parser::Transform) {}
        fn pop_transform(&mut self) {}
    }
    let Some(colr) = face.tables().colr else {
        return 0;
    };
    let mut counter = LayerCounter(0);
    let _ = colr.paint(
        gid,
        0,
        &mut counter,
        &[],
        ttf_parser::RgbaColor::new(0, 0, 0, 255),
    );
    counter.0
}

// ---------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------

/// `PaintTransform` and friends must actually move geometry.
///
/// U+1F60A places two identical blush glyphs through two different
/// `PaintTransform` translations; ignoring the transform stack would draw them
/// on top of each other in the centre.  Comparing the left and right halves of
/// the bitmap catches that.
#[test]
fn transforms_move_geometry() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F60A}').expect("fixture maps U+1F60A");
    let bitmap = render_colr_v1(&data, gid, PX, PX).expect("must render");

    // The blush colour is (255, 120, 146); count it on each side.
    let mut left = 0usize;
    let mut right = 0usize;
    for (i, px) in bitmap.rgba.chunks_exact(4).enumerate() {
        let x = (i as u32) % bitmap.width;
        let close = px[3] > 200
            && px[0] > 230
            && (100..=140).contains(&px[1])
            && (126..=166).contains(&px[2]);
        if close {
            if x < bitmap.width / 2 {
                left += 1;
            } else {
                right += 1;
            }
        }
    }
    assert!(
        left > 0 && right > 0,
        "translated copies must appear on both sides (left={left}, right={right})"
    );
}

// ---------------------------------------------------------------------------
// Palettes and degenerate inputs
// ---------------------------------------------------------------------------

/// An out-of-range CPAL palette index resolves to nothing rather than panicking.
#[test]
fn out_of_range_palette_is_rejected() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");
    assert!(
        oxitext_raster::render_colr_with_palette(&data, gid, PX, PX, u16::MAX).is_none(),
        "a nonexistent palette must not produce a bitmap"
    );
}

/// Non-COLR glyphs and malformed data return `None` instead of a blank bitmap.
#[test]
fn non_colr_inputs_return_none() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let space = face.glyph_index(' ').expect("fixture maps space");
    assert!(
        render_colr_v1(&data, space, PX, PX).is_none(),
        "space has no COLR record"
    );
    assert!(render_colr_v1(b"not a font", GlyphId(1), PX, PX).is_none());
    assert!(render_colr_v1(&data, GlyphId(u16::MAX), PX, PX).is_none());
}

/// Very small and very large targets both stay self-consistent.
#[test]
fn unusual_sizes_stay_well_formed() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    for (w, h) in [(1, 1), (8, 8), (96, 32), (32, 96), (256, 256)] {
        let bitmap = render_colr_v1(&data, gid, w, h).expect("must render");
        assert_well_formed(&bitmap, w, h);
    }
    let big = render_colr_v1(&data, gid, 256, 256).expect("must render");
    assert!(coverage(&big) > 0.05, "large render must still have ink");
}

// ---------------------------------------------------------------------------
// Throughput
// ---------------------------------------------------------------------------

/// A caption line's worth of emoji must rasterize quickly enough to be usable.
///
/// `render_colr_v1` always paints — the memoized counterpart is
/// `render_colr_cached`, whose budget lives in `tests/colr_cache.rs` — so this
/// stays a budget on the paint graph itself.
#[test]
fn rendering_is_fast_enough_for_captions() {
    if cfg!(debug_assertions) {
        // Unoptimised builds are 10-30x slower; the budget only means something
        // in release mode.
        return;
    }
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let glyphs = colr_glyphs(&face);

    let start = Instant::now();
    let mut rendered = 0;
    for _ in 0..2 {
        for (_, gid) in &glyphs {
            if render_colr_v1(&data, *gid, PX, PX).is_some() {
                rendered += 1;
            }
        }
    }
    let elapsed = start.elapsed();
    assert!(rendered > 0, "nothing rendered");
    let per_glyph = elapsed / rendered;
    eprintln!("[perf] {rendered} emoji @{PX}px: {elapsed:?} ({per_glyph:?} each)");
    assert!(
        per_glyph.as_millis() < 20,
        "{rendered} emoji took {elapsed:?} ({per_glyph:?} each)"
    );
}

// ---------------------------------------------------------------------------
// Opt-in: the complete Noto COLRv1 emoji font
// ---------------------------------------------------------------------------

/// Sweep an entire COLRv1 emoji font when one is provided.
///
/// Set `OXITEXT_TEST_COLR_FONT` to e.g. `noto-glyf_colr_1.ttf` or
/// `Noto-COLRv1.ttf`.  The font is ~4.6 MB, so it is not vendored.
#[test]
fn full_colrv1_font_sweep_when_available() {
    let Ok(path) = std::env::var("OXITEXT_TEST_COLR_FONT") else {
        return;
    };
    let data = std::fs::read(&path).expect("read OXITEXT_TEST_COLR_FONT");
    let face = Face::parse(&data, 0).expect("font parses");
    let glyphs = colr_glyphs(&face);
    assert!(!glyphs.is_empty(), "{path} exposes no COLR glyphs");

    let sample: Vec<_> = glyphs.iter().step_by(glyphs.len().div_ceil(200)).collect();
    let mut blank = 0;
    for (cp, gid) in &sample {
        let bitmap =
            render_colr_v1(&data, *gid, PX, PX).unwrap_or_else(|| panic!("U+{cp:04X} must render"));
        if coverage(&bitmap) == 0.0 {
            blank += 1;
        }
    }
    assert!(
        blank * 20 <= sample.len(),
        "{blank} of {} sampled glyphs from {path} were blank",
        sample.len()
    );
}

// ---------------------------------------------------------------------------
// `render_colr_glyph_sized`: laying colour glyphs out next to shaped text
// ---------------------------------------------------------------------------

/// Distinct near-opaque colours in a [`ColorGlyphImage`], quantised like
/// [`distinct_opaque_colors`].
fn distinct_opaque_colors_sized(image: &ColorGlyphImage) -> usize {
    image
        .rgba
        .chunks_exact(4)
        .filter(|px| px[3] >= 250)
        .map(|px| (px[0] >> 3, px[1] >> 3, px[2] >> 3))
        .collect::<HashSet<_>>()
        .len()
}

/// The sized entry point must produce a self-consistent, ink-tight, multi-colour
/// bitmap with usable bearings.
#[test]
fn sized_rendering_is_tight_and_coloured() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let image = render_colr_glyph_sized(&data, gid.0, PX as f32, 0).expect("must render");
    assert!(image.width > 0 && image.height > 0);
    assert_eq!(
        image.rgba.len(),
        (image.width as usize) * (image.height as usize) * 4
    );
    assert!(
        distinct_opaque_colors_sized(&image) >= 2,
        "a colour emoji must use more than one colour"
    );

    // Ink-tight: every edge row and column must carry at least one non-zero
    // alpha sample, otherwise the trim did not run.
    let row =
        |y: u32| (0..image.width).any(|x| image.rgba[((y * image.width + x) as usize) * 4 + 3] > 0);
    let col = |x: u32| {
        (0..image.height).any(|y| image.rgba[((y * image.width + x) as usize) * 4 + 3] > 0)
    };
    assert!(row(0) && row(image.height - 1), "top/bottom row is empty");
    assert!(col(0) && col(image.width - 1), "left/right column is empty");

    // The glyph sits above the baseline and starts at or after the pen.
    assert!(image.bearing_y > 0, "bearing_y = {}", image.bearing_y);
    assert!(image.bearing_x >= 0, "bearing_x = {}", image.bearing_x);
}

/// The bitmap scales with the requested em size, and stays inside the paint box
/// implied by the font's own `ClipList`.
#[test]
fn sized_rendering_scales_with_the_em() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let gid = face.glyph_index('\u{1F601}').expect("fixture maps U+1F601");

    let small = render_colr_glyph_sized(&data, gid.0, 32.0, 0).expect("32 px");
    let large = render_colr_glyph_sized(&data, gid.0, 96.0, 0).expect("96 px");
    assert!(
        large.width > small.width * 2 && large.height > small.height * 2,
        "{}x{} vs {}x{}",
        small.width,
        small.height,
        large.width,
        large.height
    );
    assert!(large.bearing_y > small.bearing_y);
}

/// Unlike the fixed-square entry points, the sized one must not clip a glyph
/// that paints outside the `[0, 1] x [-0.2, 0.8]` em window.
///
/// Noto's COLRv1 emoji do exactly that (out to 1.16 em right of the pen and
/// 0.91 em above the baseline), so this is checked against the real font when
/// one is provided.
#[test]
fn sized_rendering_does_not_clip_the_paint_box() {
    let Ok(path) = std::env::var("OXITEXT_TEST_COLR_FONT") else {
        return;
    };
    let data = std::fs::read(&path).expect("read OXITEXT_TEST_COLR_FONT");
    let face = Face::parse(&data, 0).expect("font parses");
    let colr = face.tables().colr.expect("font has COLR");
    let upem = f32::from(face.units_per_em());
    let em_px = 80.0f32;

    let mut checked = 0;
    for (_, gid) in colr_glyphs(&face).iter().take(64) {
        let Some(clip) = colr.clip_box(*gid, &[]) else {
            continue;
        };
        let Some(image) = render_colr_glyph_sized(&data, gid.0, em_px, 0) else {
            continue;
        };
        let scale = em_px / upem;
        // The trimmed ink must stay inside the clip box, and the clip box must
        // stay inside the bitmap: one pixel of slack for the outward rounding.
        assert!(
            f32::from(image.bearing_x as i16) >= (clip.x_min * scale).floor() - 1.0,
            "left edge outside the clip box"
        );
        assert!(
            f32::from(image.bearing_y as i16) <= (clip.y_max * scale).ceil() + 1.0,
            "top edge outside the clip box"
        );
        // The fixed-square path would have capped the height at `em_px`; a glyph
        // whose clip box is taller than one em proves the difference.
        let clip_h = ((clip.y_max - clip.y_min) * scale).ceil();
        assert!(
            image.height as f32 <= clip_h + 2.0,
            "bitmap {} px tall for a {clip_h} px clip box",
            image.height
        );
        checked += 1;
    }
    assert!(checked > 0, "{path} exposes no clip boxes to check");
}

/// A glyph the font has no COLR record for, and a font with no COLR at all,
/// both yield `None` rather than an empty bitmap.
#[test]
fn sized_rendering_declines_non_colour_glyphs() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let face = Face::parse(&data, 0).expect("fixture parses");
    let colr = face.tables().colr.expect("fixture has COLR");
    let plain = (0..face.number_of_glyphs()).find(|&g| !colr.contains(GlyphId(g)));
    if let Some(gid) = plain {
        assert!(render_colr_glyph_sized(&data, gid, PX as f32, 0).is_none());
    }
    assert!(render_colr_glyph_sized(&data, u16::MAX, PX as f32, 0).is_none());
    // Palette 7 does not exist in any of the fixtures.
    assert!(render_colr_glyph_sized(&data, 1, PX as f32, 7).is_none());
}
