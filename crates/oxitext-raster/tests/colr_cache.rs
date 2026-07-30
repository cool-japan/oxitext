//! COLR paint-graph cache: identity, keying and cost.
//!
//! Rendering a colour glyph is a pure function of `(font bytes, glyph id, size,
//! palette)` but an expensive one — the paint graph is walked, every layer
//! outline is flattened and rasterized, and the layers are composited in
//! premultiplied `f32`.  Measured on the vendored COLRv1 fixtures at 64 px in
//! release, a single glyph costs **37–159 µs** (0.42–1.97 ms in debug), and a
//! caption renderer asks for the same emoji at the same size on every frame.
//!
//! `oxitext_raster::colr_cache` memoizes those results per thread, behind the
//! `_cached` entry points that take the caller's `Arc<[u8]>` font handle.  This
//! file pins the three properties that make the memo safe and worthwhile:
//!
//! * **Byte identity** — a cached result is indistinguishable, field by field
//!   and byte by byte, from the uncached entry point's output.
//! * **Keying** — glyph id, em size, palette, bitmap dimensions and the font
//!   handle all take part in the key, so a hit is never the wrong picture, and
//!   the handle is *retained* by the entry so its address cannot be recycled.
//! * **Cost** — the cache actually engages (proved by `Arc::ptr_eq` and by the
//!   hit counter) and a warm call is far cheaper than even a
//!   `fontdue::Font::from_bytes` of the same face, which is the budget pattern
//!   `tests/font_cache_parity.rs` uses for the fontdue cache.
//!
//! Fixtures come from `googlefonts/color-fonts` (Apache-2.0); see
//! `tests/fixtures/README.md` for provenance and hashes.

use oxitext_raster::{
    clear_colr_cache, colr_cache_stats, render_colr_cached, render_colr_glyph_sized,
    render_colr_glyph_sized_cached, render_colr_v1, render_colr_with_palette, ColorGlyphImage,
    ColrCacheStats,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ttf_parser::{Face, GlyphId};

/// Real Twemoji smiley emoji with `PaintSolid` layers and transforms.
const TWEMOJI_SMILEY: &str = "twemoji_smiley-glyf_colr_1.ttf";
/// The Noto Emoji writing hand, with linear and radial gradients.
const NOTO_HANDWRITING: &str = "noto_handwriting-glyf_colr_1.ttf";
/// Synthetic glyphs covering the whole COLRv1 paint-format matrix.
const COLR_TEST_GLYPHS: &str = "test_glyphs-glyf_colr_1.ttf";

/// Every checked-in COLR fixture.
const FIXTURES: &[&str] = &[TWEMOJI_SMILEY, NOTO_HANDWRITING, COLR_TEST_GLYPHS];

/// Em sizes exercised by the identity tests.
const SIZES: &[f32] = &[16.0, 32.0, 64.0, 81.0];

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Absolute path of a checked-in fixture.
fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Load a fixture as a shared handle, failing the test when it is missing.
fn require_fixture(name: &str) -> Arc<[u8]> {
    let path = fixture_path(name);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("missing fixture {name} ({e}); see tests/fixtures/README.md"));
    Arc::from(bytes)
}

/// The first `limit` COLR base glyphs of a face, in glyph order.
fn colr_gids(data: &[u8], limit: usize) -> Vec<u16> {
    let face = Face::parse(data, 0).expect("fixture parses");
    let Some(colr) = face.tables().colr else {
        return Vec::new();
    };
    (0..face.number_of_glyphs())
        .filter(|&gid| colr.contains(GlyphId(gid)))
        .take(limit)
        .collect()
}

/// Assert that two colour glyph images agree on every field, byte for byte.
fn assert_same_image(what: &str, got: &ColorGlyphImage, want: &ColorGlyphImage) {
    assert_eq!(got.width, want.width, "{what}: width differs");
    assert_eq!(got.height, want.height, "{what}: height differs");
    assert_eq!(got.bearing_x, want.bearing_x, "{what}: bearing_x differs");
    assert_eq!(got.bearing_y, want.bearing_y, "{what}: bearing_y differs");
    assert_eq!(got.rgba, want.rgba, "{what}: rgba bytes differ");
}

/// Median of `samples`, which must not be empty.
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

// ---------------------------------------------------------------------------
// Byte identity
// ---------------------------------------------------------------------------

/// A cached sized image must be byte-identical to the uncached entry point's.
#[test]
fn cached_sized_image_matches_uncached_bytewise() {
    let mut compared = 0_usize;
    for &name in FIXTURES {
        let data = require_fixture(name);
        let gids = colr_gids(&data, 6);
        assert!(!gids.is_empty(), "{name} must contain COLR glyphs");

        for &gid in &gids {
            for &px in SIZES {
                // The uncached entry point never consults the cache at all.
                let Some(reference) = render_colr_glyph_sized(&data, gid, px, 0) else {
                    continue;
                };

                clear_colr_cache();
                let cold = render_colr_glyph_sized_cached(&data, gid, px, 0)
                    .expect("a glyph that renders uncached must render cached");
                assert_eq!(
                    colr_cache_stats().hits,
                    0,
                    "a cleared cache must not report hits"
                );

                // Same call again — this one must come out of the cache.
                let warm = render_colr_glyph_sized_cached(&data, gid, px, 0)
                    .expect("a glyph that rendered once must render again");
                assert_eq!(
                    colr_cache_stats().hits,
                    1,
                    "{name} gid {gid} @{px}px: the second call did not hit the cache"
                );
                assert!(
                    Arc::ptr_eq(&cold, &warm),
                    "{name} gid {gid} @{px}px: the hit was not the stored allocation"
                );

                let what = format!("{name} gid {gid} @{px}px");
                assert_same_image(&what, &cold, &reference);
                compared += 1;
            }
        }
    }
    assert!(compared > 50, "only {compared} images compared");
}

/// The same guarantee for the fixed-size entry points, which share the second
/// cache.
#[test]
fn cached_fixed_size_bitmap_matches_uncached_bytewise() {
    for &name in FIXTURES {
        let data = require_fixture(name);
        for &gid in &colr_gids(&data, 4) {
            for (w, h) in [(16_u32, 16_u32), (64, 64), (96, 32)] {
                let Some(reference) = render_colr_v1(&data, GlyphId(gid), w, h) else {
                    continue;
                };
                clear_colr_cache();
                let cold =
                    render_colr_cached(&data, GlyphId(gid), w, h, 0).expect("must render cached");
                let warm =
                    render_colr_cached(&data, GlyphId(gid), w, h, 0).expect("must render again");
                assert_eq!(
                    colr_cache_stats().hits,
                    1,
                    "{name} gid {gid} {w}x{h}: the second call did not hit the cache"
                );
                assert!(Arc::ptr_eq(&cold, &warm));
                assert_eq!(cold.width, reference.width);
                assert_eq!(cold.height, reference.height);
                assert_eq!(
                    cold.rgba, reference.rgba,
                    "{name} gid {gid} {w}x{h}: rgba bytes differ"
                );
            }
        }
    }
}

/// Degenerate arguments must be rejected before anything is cached.
#[test]
fn bad_arguments_are_rejected_and_leave_no_entries() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");
    let junk: Arc<[u8]> = Arc::from(&b"not a font"[..]);

    clear_colr_cache();
    assert!(render_colr_glyph_sized_cached(&data, gid, 0.0, 0).is_none());
    assert!(render_colr_glyph_sized_cached(&data, gid, -16.0, 0).is_none());
    assert!(render_colr_glyph_sized_cached(&data, gid, f32::NAN, 0).is_none());
    assert!(render_colr_glyph_sized_cached(&data, gid, f32::INFINITY, 0).is_none());
    assert!(render_colr_glyph_sized_cached(&junk, 1, 16.0, 0).is_none());
    assert!(render_colr_cached(&data, GlyphId(gid), 0, 16, 0).is_none());
    assert!(render_colr_cached(&data, GlyphId(gid), 16, 0, 0).is_none());
    assert!(render_colr_cached(&junk, GlyphId(1), 16, 16, 0).is_none());
    assert_eq!(colr_cache_stats().entries, 0);
}

// ---------------------------------------------------------------------------
// The cache path engages
// ---------------------------------------------------------------------------

/// Repeated lookups must hand back the *same* allocation.
#[test]
fn repeated_lookups_return_one_shared_allocation() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    clear_colr_cache();
    let first = render_colr_glyph_sized_cached(&data, gid, 64.0, 0).expect("must render");
    let second = render_colr_glyph_sized_cached(&data, gid, 64.0, 0).expect("must render");
    assert!(
        Arc::ptr_eq(&first, &second),
        "the cache must hand back a shared handle, not a re-render"
    );
    let stats = colr_cache_stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.entries, 1);
    assert!(
        stats.bytes > 0,
        "a cached image must account for its pixels"
    );

    // Clearing must actually drop the memo: a new allocation, same pixels.
    clear_colr_cache();
    let third = render_colr_glyph_sized_cached(&data, gid, 64.0, 0).expect("must render");
    assert!(
        !Arc::ptr_eq(&first, &third),
        "clear_colr_cache did not drop the entry"
    );
    assert_same_image("after clear", &third, &first);
}

/// A resident entry must keep the caller's font handle alive — that is what
/// makes keying on its address sound.
#[test]
fn a_resident_entry_retains_the_font_handle() {
    let data = require_fixture(NOTO_HANDWRITING);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    clear_colr_cache();
    let before = Arc::strong_count(&data);
    let _image = render_colr_glyph_sized_cached(&data, gid, 48.0, 0).expect("must render");
    assert!(
        Arc::strong_count(&data) > before,
        "the cache must retain the font handle"
    );
    clear_colr_cache();
    assert_eq!(
        Arc::strong_count(&data),
        before,
        "clearing must release the font handle"
    );
}

/// Every call after the first must be a hit.
#[test]
fn only_the_first_call_paints() {
    let data = require_fixture(NOTO_HANDWRITING);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    clear_colr_cache();
    for _ in 0..8 {
        let _ = render_colr_glyph_sized_cached(&data, gid, 48.0, 0).expect("must render");
    }
    let stats = colr_cache_stats();
    assert_eq!(stats.misses, 1, "only the first call may paint");
    assert_eq!(stats.hits, 7, "every later call must be a hit");
    assert_eq!(stats.entries, 1);
}

// ---------------------------------------------------------------------------
// Keying
// ---------------------------------------------------------------------------

/// Glyph id, em size and palette must all take part in the key.
#[test]
fn key_separates_glyph_size_and_palette() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gids = colr_gids(&data, 3);
    assert!(
        gids.len() >= 2,
        "fixture must have at least two COLR glyphs"
    );

    clear_colr_cache();
    let a = render_colr_glyph_sized_cached(&data, gids[0], 64.0, 0).expect("must render");
    let b = render_colr_glyph_sized_cached(&data, gids[1], 64.0, 0).expect("must render");
    assert!(!Arc::ptr_eq(&a, &b), "two glyph ids shared one entry");
    assert_ne!(a.rgba, b.rgba, "two glyph ids produced identical pixels");

    let small = render_colr_glyph_sized_cached(&data, gids[0], 32.0, 0).expect("must render");
    assert!(!Arc::ptr_eq(&a, &small), "two em sizes shared one entry");
    assert!(
        small.width < a.width || small.height < a.height,
        "32 px must not be as large as 64 px"
    );

    // An out-of-range palette must stay `None` even with palette 0 cached.
    let palettes = Face::parse(&data, 0)
        .expect("fixture parses")
        .color_palettes()
        .map_or(0, |n| n.get());
    assert!(
        render_colr_glyph_sized_cached(&data, gids[0], 64.0, palettes).is_none(),
        "an out-of-range palette must not be served from the palette-0 entry"
    );

    // The fixed-size cache keys on the dimensions too.
    clear_colr_cache();
    let wide = render_colr_cached(&data, GlyphId(gids[0]), 96, 32, 0).expect("must render");
    let tall = render_colr_cached(&data, GlyphId(gids[0]), 32, 96, 0).expect("must render");
    assert!(!Arc::ptr_eq(&wide, &tall), "two shapes shared one entry");
    assert_eq!((wide.width, wide.height), (96, 32));
    assert_eq!((tall.width, tall.height), (32, 96));

    // …and it agrees with the uncached palette-aware entry point.
    let uncached =
        render_colr_with_palette(&data, GlyphId(gids[0]), 96, 32, 0).expect("must render");
    assert_eq!(wide.rgba, uncached.rgba);
}

/// Two different fonts must never share an entry, even for the same glyph id at
/// the same size.
#[test]
fn key_separates_different_fonts() {
    let twemoji = require_fixture(TWEMOJI_SMILEY);
    let handwriting = require_fixture(NOTO_HANDWRITING);

    let handwriting_gids = colr_gids(&handwriting, 32);
    let shared_gid = colr_gids(&twemoji, 32)
        .into_iter()
        .find(|gid| handwriting_gids.contains(gid))
        .expect("the two fixtures must share at least one COLR glyph id");

    clear_colr_cache();
    let from_twemoji =
        render_colr_glyph_sized_cached(&twemoji, shared_gid, 64.0, 0).expect("must render");
    let from_handwriting =
        render_colr_glyph_sized_cached(&handwriting, shared_gid, 64.0, 0).expect("must render");
    assert_eq!(colr_cache_stats().misses, 2, "the fonts shared a key");
    assert!(!Arc::ptr_eq(&from_twemoji, &from_handwriting));

    assert_same_image(
        "twemoji",
        &from_twemoji,
        &render_colr_glyph_sized(&twemoji, shared_gid, 64.0, 0).expect("must render"),
    );
    assert_same_image(
        "handwriting",
        &from_handwriting,
        &render_colr_glyph_sized(&handwriting, shared_gid, 64.0, 0).expect("must render"),
    );
}

/// Two byte-identical fonts held in *separate* allocations are separate cache
/// entries — a miss, never a wrong picture.
///
/// This is the direction the ownership-based key errs in: it can miss where a
/// content hash would hit, but it can never hand back another font's glyph.
#[test]
fn distinct_handles_over_equal_bytes_do_not_alias() {
    let a = require_fixture(TWEMOJI_SMILEY);
    let b: Arc<[u8]> = Arc::from(a.to_vec());
    assert_eq!(
        a.as_ref(),
        b.as_ref(),
        "the two handles must hold equal bytes"
    );
    let gid = *colr_gids(&a, 1).first().expect("fixture has COLR glyphs");

    clear_colr_cache();
    let from_a = render_colr_glyph_sized_cached(&a, gid, 64.0, 0).expect("must render");
    let from_b = render_colr_glyph_sized_cached(&b, gid, 64.0, 0).expect("must render");
    assert_eq!(
        colr_cache_stats().misses,
        2,
        "distinct handles shared a key"
    );
    assert!(!Arc::ptr_eq(&from_a, &from_b));
    assert_same_image("equal bytes, distinct handles", &from_b, &from_a);

    // Cloning a handle *is* the same font, and must hit.
    let clone = Arc::clone(&a);
    let from_clone = render_colr_glyph_sized_cached(&clone, gid, 64.0, 0).expect("must render");
    assert!(
        Arc::ptr_eq(&from_a, &from_clone),
        "a cloned handle must reuse the entry"
    );
}

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// Filling the cache must not let it grow without limit.
#[test]
fn cache_respects_its_entry_and_byte_bounds() {
    let data = require_fixture(COLR_TEST_GLYPHS);
    let gids = colr_gids(&data, 120);
    assert!(gids.len() > 40, "fixture must have many COLR glyphs");

    clear_colr_cache();
    for &gid in &gids {
        for &px in &[24.0_f32, 48.0, 96.0] {
            let _ = render_colr_glyph_sized_cached(&data, gid, px, 0);
        }
    }
    let stats = colr_cache_stats();
    assert!(stats.entries <= 256, "entry bound broken: {stats:?}");
    assert!(
        stats.bytes <= 8 * 1024 * 1024,
        "byte bound broken: {stats:?}"
    );
}

/// A single huge glyph must still render, but must not be kept.
#[test]
fn oversized_results_are_returned_but_not_cached() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    clear_colr_cache();
    // 1200 px per em yields well over the 2 MiB per-entry ceiling.
    let big = render_colr_glyph_sized_cached(&data, gid, 1200.0, 0).expect("must render");
    assert!(
        big.rgba.len() > 2 * 1024 * 1024,
        "the test needs a result above the per-entry ceiling, got {} bytes",
        big.rgba.len()
    );
    let stats = colr_cache_stats();
    assert_eq!(
        stats.entries, 0,
        "an oversized result was cached: {stats:?}"
    );
    assert_eq!(stats.bytes, 0);

    // Still correct, just recomputed each time.
    let again = render_colr_glyph_sized_cached(&data, gid, 1200.0, 0).expect("must render");
    assert!(!Arc::ptr_eq(&big, &again), "an oversized result was cached");
    assert_same_image("oversized", &again, &big);
}

// ---------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------

/// Each thread keeps its own cache, and every thread sees the same pixels.
#[test]
fn caches_are_per_thread_but_equivalent() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    clear_colr_cache();
    let main = render_colr_glyph_sized_cached(&data, gid, 64.0, 0).expect("must render");

    let worker_data = Arc::clone(&data);
    let (worker, worker_stats) = std::thread::spawn(move || {
        // A fresh thread starts with an empty cache.
        let before = colr_cache_stats();
        let image =
            render_colr_glyph_sized_cached(&worker_data, gid, 64.0, 0).expect("must render");
        (image, before)
    })
    .join()
    .expect("worker thread must not panic");

    assert_eq!(
        worker_stats,
        ColrCacheStats::default(),
        "a new thread must start with an empty cache"
    );
    assert!(
        !Arc::ptr_eq(&worker, &main),
        "the caches must not be shared between threads"
    );
    assert_same_image("worker thread", &worker, &main);
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

/// A warm colour-glyph lookup must cost far less than parsing the face.
///
/// This is the budget pattern `tests/font_cache_parity.rs` uses, applied to the
/// colour path: `fontdue::Font::from_bytes` is the cheapest full font parse the
/// crate performs, so if a colour render ever costs a parse again — which is
/// what `color.rs` used to do, once per layer, per call — the warm/parse ratio
/// collapses.  Measured on `test_glyphs-glyf_colr_1.ttf` (201 glyphs): parse
/// 40.7 µs, warm lookup 0.18 µs in release (parse 725 µs, warm 0.87 µs in
/// debug), so the 20x margin asserted below has two orders of magnitude of
/// headroom in either profile.
#[test]
fn warm_lookup_costs_far_less_than_a_font_parse() {
    let data = require_fixture(COLR_TEST_GLYPHS);
    let gid = *colr_gids(&data, 1)
        .first()
        .expect("fixture has COLR glyphs");

    let parse = median(
        (0..9)
            .map(|_| {
                let start = Instant::now();
                let font =
                    fontdue::Font::from_bytes(data.as_ref(), fontdue::FontSettings::default());
                let elapsed = start.elapsed();
                assert!(font.is_ok(), "fixture must parse with fontdue");
                elapsed
            })
            .collect(),
    );

    clear_colr_cache();
    let cold_start = Instant::now();
    let _ = render_colr_glyph_sized_cached(&data, gid, 64.0, 0).expect("must render");
    let cold = cold_start.elapsed();

    // Time a run of warm lookups so the clock resolution does not dominate.
    const WARM_ITERATIONS: u32 = 500;
    let warm = median(
        (0..9)
            .map(|_| {
                let start = Instant::now();
                for _ in 0..WARM_ITERATIONS {
                    let _ = render_colr_glyph_sized_cached(&data, gid, 64.0, 0);
                }
                start.elapsed() / WARM_ITERATIONS
            })
            .collect(),
    );

    eprintln!(
        "[perf] {COLR_TEST_GLYPHS} gid {gid} @64px: fontdue parse {parse:?}, cold render {cold:?}, warm lookup {warm:?}"
    );
    assert!(
        warm * 20 < parse,
        "a warm colour lookup cost {warm:?} against a {parse:?} font parse — \
         the COLR cache is not engaging"
    );
    assert!(
        warm * 10 < cold,
        "a warm colour lookup cost {warm:?} against a {cold:?} cold render — \
         the COLR cache is not engaging"
    );
}

/// Thirty distinct colour glyphs must paint inside a generous budget, warm or
/// cold.
///
/// Mirrors `font_cache_parity::thirty_glyph_cue_rasterizes_within_budget`: the
/// budget is only asserted in release, because an unoptimised paint graph is
/// 10-30x slower and would flap.  Measured in release: 30 cold glyphs in
/// ~2.5 ms and 30 warm ones in ~6 µs.
#[test]
fn thirty_colour_glyphs_render_within_budget() {
    let data = require_fixture(TWEMOJI_SMILEY);
    let gids = colr_gids(&data, 15);
    assert!(!gids.is_empty(), "fixture must have COLR glyphs");
    // 15 glyphs at two sizes: 30 distinct cache keys.
    let work: Vec<(u16, f32)> = gids
        .iter()
        .flat_map(|&gid| [(gid, 48.0_f32), (gid, 64.0_f32)])
        .collect();
    assert_eq!(work.len(), 30, "the workload must be exactly 30 glyphs");

    clear_colr_cache();
    let cold_start = Instant::now();
    let mut painted = 0_usize;
    for &(gid, px) in &work {
        if let Some(image) = render_colr_glyph_sized_cached(&data, gid, px, 0) {
            painted += image.rgba.len();
        }
    }
    let cold = cold_start.elapsed();

    let warm_start = Instant::now();
    for &(gid, px) in &work {
        let _ = render_colr_glyph_sized_cached(&data, gid, px, 0);
    }
    let warm = warm_start.elapsed();

    let cold_budget = Duration::from_millis(30);
    let warm_budget = Duration::from_millis(2);
    eprintln!(
        "[perf] 30 colour glyphs: cold {cold:?} ({:?}/glyph, budget {cold_budget:?}), warm {warm:?} ({:?}/glyph, budget {warm_budget:?}), bytes {painted}",
        cold / 30,
        warm / 30,
    );
    assert!(painted > 0, "nothing was painted");
    assert_eq!(colr_cache_stats().hits, 30, "the warm pass missed");

    #[cfg(not(debug_assertions))]
    {
        assert!(
            cold < cold_budget,
            "30 cold colour glyphs took {cold:?} (budget {cold_budget:?})"
        );
        assert!(
            warm < warm_budget,
            "30 warm colour glyphs took {warm:?} (budget {warm_budget:?}) — \
             the COLR cache is not engaging"
        );
    }
}

/// The opt-in whole-font check: a real 4.6 MB Noto COLRv1 build is where the
/// per-call cost actually hurt, so run the same warm/cold comparison on it when
/// one is available.
///
/// Set `OXITEXT_TEST_COLR_FONT` to a local copy; the font is not vendored.
#[test]
fn full_colrv1_font_warm_lookup_when_available() {
    let Ok(path) = std::env::var("OXITEXT_TEST_COLR_FONT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let data: Arc<[u8]> = Arc::from(bytes);
    let gids = colr_gids(&data, 40);
    if gids.is_empty() {
        return;
    }

    clear_colr_cache();
    let cold_start = Instant::now();
    let mut rendered = 0_u32;
    for &gid in &gids {
        if render_colr_glyph_sized_cached(&data, gid, 80.6, 0).is_some() {
            rendered += 1;
        }
    }
    let cold = cold_start.elapsed();
    assert!(rendered > 0, "{path} produced no colour glyphs");

    let warm_start = Instant::now();
    for &gid in &gids {
        let _ = render_colr_glyph_sized_cached(&data, gid, 80.6, 0);
    }
    let warm = warm_start.elapsed();

    eprintln!(
        "[perf] {path}: {rendered} glyphs @80.6px cold {cold:?} ({:?} each), warm {warm:?} ({:?} each)",
        cold / rendered,
        warm / gids.len() as u32,
    );
    assert!(
        warm * 10 < cold,
        "warm {warm:?} against cold {cold:?} — the COLR cache is not engaging"
    );
}
