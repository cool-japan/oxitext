//! Thread-local font-cache parity and throughput tests.
//!
//! [`oxitext_raster::get_or_parse_fontdue`] hands out `Arc<fontdue::Font>`
//! handles so that rasterizing a glyph never deep-copies the parsed face.
//! These tests pin both halves of that contract:
//!
//! * **Parity** — bitmaps and metrics produced through the cached
//!   [`FontdueRaster`] path are byte-identical to those produced by a privately
//!   owned `fontdue::Font` (both a freshly parsed one and a `clone()` of it,
//!   which is exactly what the cache used to return).  Covered glyphs are
//!   ASCII, CJK, the `ﬁ` ligature (U+FB01), and whitespace.
//! * **Throughput** — a 30-glyph Japanese cue rasterizes well inside a
//!   generous budget.  Before the `Arc` change every glyph paid a full
//!   deep-copy of the face, which for a large CJK font costs tens to hundreds
//!   of milliseconds *per glyph*.
//!
//! Font resolution follows the rest of the suite: the checked-in fixture
//! first, then well-known system paths, then the statically bundled Noto Sans
//! Regular, so the tests are deterministic and never require system fonts.
//! Point `OXITEXT_TEST_CJK_FONT` at a CJK face (e.g. a Noto Sans JP build) to
//! run the same checks against real CJK outlines — the default faces are Latin
//! and cost ~1.7 ms per deep copy, while a large CJK face costs 67-302 ms, so
//! the throughput budget below discriminates in either configuration.

use oxitext_raster::backend::{FontdueRaster, RasterBackend, RasterOutput};
use oxitext_raster::FontdueRasterizer;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A 30-character Japanese cue, matching the downstream subtitle workload.
const JP_CUE: &str = "日本語の字幕をラスタライズする速度計測用のサンプル文字列です";

/// Latin stand-in used when no CJK-capable face is available.
const LATIN_CUE: &str = "The quick brown fox jumps over";

/// Glyphs exercised by the parity test: ASCII, CJK, a ligature, whitespace.
const PARITY_CHARS: &[char] = &[
    'A', 'g', 'W', '0', '@', ' ', '日', '本', '語', '漢', '字', '\u{FB01}',
];

/// Pixel sizes exercised by the parity test.
const PARITY_SIZES: &[f32] = &[12.0, 16.0, 32.0, 64.0];

/// A test face plus whether it actually covers CJK.
struct TestFace {
    data: Vec<u8>,
    origin: String,
    has_cjk: bool,
}

/// Candidate font paths, in the resolution order used across this suite.
///
/// `OXITEXT_TEST_CJK_FONT` comes first so a CJK face can be substituted
/// without touching the checked-in defaults.
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("OXITEXT_TEST_CJK_FONT") {
        if !p.is_empty() {
            paths.push(PathBuf::from(p));
        }
    }
    paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf"));
    for p in [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ] {
        paths.push(PathBuf::from(p));
    }
    paths
}

/// Resolve the first candidate face that fontdue can parse.
///
/// Falls back to the statically bundled Noto Sans Regular, which is always
/// available because the workspace dev-dependency enables `bundled-noto`.
fn load_test_face() -> TestFace {
    for path in candidate_paths() {
        if !path.exists() {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(font) = fontdue::Font::from_bytes(data.as_slice(), fontdue::FontSettings::default())
        else {
            continue;
        };
        return TestFace {
            has_cjk: font.lookup_glyph_index('日') != 0,
            data,
            origin: path.display().to_string(),
        };
    }

    TestFace {
        data: oxifont_bundled::NOTO_SANS_REGULAR.to_vec(),
        origin: "oxifont-bundled NOTO_SANS_REGULAR".to_string(),
        has_cjk: false,
    }
}

/// Parse `data` into a private `fontdue::Font`, bypassing the thread-local cache.
fn parse_uncached(data: &[u8]) -> fontdue::Font {
    fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).expect("test face must parse")
}

/// Rasterize through a privately owned font, mirroring [`RasterOutput`].
fn rasterize_uncached(font: &fontdue::Font, glyph_id: u16, px_size: f32) -> RasterOutput {
    let (metrics, coverage) = font.rasterize_indexed(glyph_id, px_size);
    RasterOutput {
        width: metrics.width,
        height: metrics.height,
        coverage,
        advance_x: metrics.advance_width,
        advance_y: metrics.advance_height,
        bearing_x: metrics.xmin,
        bearing_y: metrics.ymin,
    }
}

/// Assert that two rasterizations agree on every field, byte for byte.
fn assert_same_output(what: &str, cached: &RasterOutput, reference: &RasterOutput) {
    assert_eq!(cached.width, reference.width, "{what}: width differs");
    assert_eq!(cached.height, reference.height, "{what}: height differs");
    assert_eq!(
        cached.advance_x.to_bits(),
        reference.advance_x.to_bits(),
        "{what}: advance_x differs"
    );
    assert_eq!(
        cached.advance_y.to_bits(),
        reference.advance_y.to_bits(),
        "{what}: advance_y differs"
    );
    assert_eq!(
        cached.bearing_x, reference.bearing_x,
        "{what}: bearing_x differs"
    );
    assert_eq!(
        cached.bearing_y, reference.bearing_y,
        "{what}: bearing_y differs"
    );
    assert_eq!(
        cached.coverage, reference.coverage,
        "{what}: coverage bitmap differs"
    );
}

#[test]
fn cached_rasterization_matches_private_font_bitwise() {
    let face = load_test_face();
    let reference = parse_uncached(&face.data);
    // A `clone()` of the parsed face is precisely what the thread-local cache
    // used to hand out; keeping it in the comparison proves the `Arc` handles
    // changed nothing but the cost.
    let cloned = reference.clone();
    let raster = FontdueRaster::new();

    let mut checked = 0_usize;
    for &ch in PARITY_CHARS {
        let gid = reference.lookup_glyph_index(ch);
        for &px_size in PARITY_SIZES {
            let cached = raster.rasterize(&face.data, gid, px_size);
            let fresh = rasterize_uncached(&reference, gid, px_size);
            let deep_cloned = rasterize_uncached(&cloned, gid, px_size);
            let what = format!("U+{:04X} gid {gid} @ {px_size}px", ch as u32);
            assert_same_output(&what, &cached, &fresh);
            assert_same_output(&what, &cached, &deep_cloned);
            checked += 1;
        }
    }

    assert_eq!(
        checked,
        PARITY_CHARS.len() * PARITY_SIZES.len(),
        "every glyph/size combination must be compared"
    );
}

#[test]
fn repeated_cached_rasterization_is_deterministic() {
    let face = load_test_face();
    let reference = parse_uncached(&face.data);
    let raster = FontdueRaster::new();

    for &ch in PARITY_CHARS {
        let gid = reference.lookup_glyph_index(ch);
        let first = raster.rasterize(&face.data, gid, 48.0);
        for round in 1..4 {
            let again = raster.rasterize(&face.data, gid, 48.0);
            assert_same_output(
                &format!("U+{:04X} round {round}", ch as u32),
                &again,
                &first,
            );
        }
    }
}

#[test]
fn cached_rasterization_matches_across_threads() {
    let face = load_test_face();
    let reference = parse_uncached(&face.data);
    let gids: Vec<u16> = PARITY_CHARS
        .iter()
        .map(|&c| reference.lookup_glyph_index(c))
        .collect();

    let expected: Vec<RasterOutput> = gids
        .iter()
        .map(|&gid| rasterize_uncached(&reference, gid, 32.0))
        .collect();

    let data = face.data;
    let worker_gids = gids.clone();
    let actual = std::thread::spawn(move || {
        let raster = FontdueRaster::new();
        worker_gids
            .iter()
            .map(|&gid| raster.rasterize(&data, gid, 32.0))
            .collect::<Vec<_>>()
    })
    .join()
    .expect("worker thread must not panic");

    for (i, (got, want)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_same_output(&format!("worker glyph #{i}"), got, want);
    }
}

/// Rasterizing a 30-glyph cue must not pay a per-glyph font deep-copy.
///
/// The budget is deliberately generous (30 ms for 30 glyphs at 64 px, i.e.
/// 1 ms/glyph); measured throughput on a laptop is ~5 µs/glyph, whereas the
/// pre-fix deep-clone path cost 67 ms/glyph with Noto Sans JP and 302 ms/glyph
/// with the 23 MB Arial Unicode face.  Only asserted in release builds — debug
/// rasterization is orders of magnitude slower and would flap.
#[test]
fn thirty_glyph_cue_rasterizes_within_budget() {
    let face = load_test_face();
    let font = parse_uncached(&face.data);
    let cue = if face.has_cjk { JP_CUE } else { LATIN_CUE };
    let gids: Vec<u16> = cue.chars().map(|c| font.lookup_glyph_index(c)).collect();
    assert_eq!(gids.len(), 30, "the cue must be exactly 30 glyphs");

    let raster = FontdueRaster::new();
    let px_size = 64.0_f32;

    // Warm the thread-local cache so the measurement excludes the one-time
    // parse of the face (which is unavoidable and not what regressed).
    let cold = Instant::now();
    let _ = raster.rasterize(&face.data, gids[0], px_size);
    let cold_elapsed = cold.elapsed();

    let start = Instant::now();
    let mut covered = 0_usize;
    for &gid in &gids {
        covered += raster.rasterize(&face.data, gid, px_size).coverage.len();
    }
    let elapsed = start.elapsed();
    let budget = Duration::from_millis(30);

    eprintln!(
        "[perf] face={} cjk={} cold(parse+1 glyph)={:?} warm(30 glyphs @{}px)={:?} ({:?}/glyph) coverage_bytes={} budget={:?}",
        face.origin,
        face.has_cjk,
        cold_elapsed,
        px_size as u32,
        elapsed,
        elapsed / 30,
        covered,
        budget,
    );

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < budget,
        "30 glyphs took {elapsed:?} (budget {budget:?}) — the font cache is deep-copying again"
    );
}

/// [`FontdueRasterizer`] must agree with a privately parsed font, byte for
/// byte.
///
/// It used to reach straight for its `Mutex`-guarded LRU, taking a lock on
/// every glyph and parsing the face itself on a miss; it now consults the
/// thread-local cache first.  That must be invisible in the output.
#[test]
fn fontdue_rasterizer_matches_private_font_bitwise() {
    let face = load_test_face();
    let reference = parse_uncached(&face.data);
    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(face.data.clone());
    let rasterizer = FontdueRasterizer::new();

    for &ch in PARITY_CHARS {
        let gid = reference.lookup_glyph_index(ch);
        for &px_size in PARITY_SIZES {
            let cached = rasterizer
                .raster(gid, &shared, px_size)
                .expect("the test face must rasterize");
            let (metrics, pixels) = reference.rasterize_indexed(gid, px_size);
            let what = format!("U+{:04X} gid {gid} @ {px_size}px", ch as u32);
            assert_eq!(cached.width, metrics.width as u32, "{what}: width differs");
            assert_eq!(
                cached.height, metrics.height as u32,
                "{what}: height differs"
            );
            assert_eq!(cached.pixels, pixels, "{what}: coverage bitmap differs");
        }
    }
}

/// An unparseable face must still surface as an error, not a panic — the
/// thread-local fast path returns `None` for it and the `Mutex` path reports
/// the parse failure exactly as before.
#[test]
fn fontdue_rasterizer_still_reports_parse_failures() {
    let rasterizer = FontdueRasterizer::new();
    let junk: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"not a font at all"[..]);
    assert!(rasterizer.raster(0, &junk, 16.0).is_err());
    let empty: std::sync::Arc<[u8]> = std::sync::Arc::from(&[][..]);
    assert!(rasterizer.raster(0, &empty, 16.0).is_err());
}

/// The same 30-glyph budget for [`FontdueRasterizer`], which now shares the
/// thread-local cache instead of locking a `Mutex` and parsing on a miss.
#[test]
fn thirty_glyph_cue_rasterizes_within_budget_through_the_rasterizer() {
    let face = load_test_face();
    let font = parse_uncached(&face.data);
    let cue = if face.has_cjk { JP_CUE } else { LATIN_CUE };
    let gids: Vec<u16> = cue.chars().map(|c| font.lookup_glyph_index(c)).collect();
    assert_eq!(gids.len(), 30, "the cue must be exactly 30 glyphs");

    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(face.data.clone());
    let rasterizer = FontdueRasterizer::new();
    let px_size = 64.0_f32;

    // Warm the thread-local cache so the one-time parse is not measured.
    let cold = Instant::now();
    let _ = rasterizer.raster(gids[0], &shared, px_size);
    let cold_elapsed = cold.elapsed();

    let start = Instant::now();
    let mut covered = 0_usize;
    for &gid in &gids {
        covered += rasterizer
            .raster(gid, &shared, px_size)
            .map_or(0, |bm| bm.pixels.len());
    }
    let elapsed = start.elapsed();
    let budget = Duration::from_millis(30);

    eprintln!(
        "[perf] FontdueRasterizer face={} cold(parse+1 glyph)={:?} warm(30 glyphs @{}px)={:?} ({:?}/glyph) coverage_bytes={} budget={:?}",
        face.origin,
        cold_elapsed,
        px_size as u32,
        elapsed,
        elapsed / 30,
        covered,
        budget,
    );

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < budget,
        "30 glyphs took {elapsed:?} (budget {budget:?}) — the font cache is deep-copying again"
    );
}

/// A freshly constructed [`FontdueRasterizer`] must not re-parse a face this
/// thread has already parsed.
///
/// Its own `Mutex`-guarded LRU is keyed on the `Arc` pointer and lives in the
/// instance, so every new instance — and the parallel render path builds one
/// per thread — used to pay a full `fontdue::Font::from_bytes` on its first
/// glyph.  Consulting the thread-local cache first makes that first call as
/// cheap as any other.
#[test]
fn a_second_rasterizer_reuses_the_thread_local_parse() {
    let face = load_test_face();
    let shared: std::sync::Arc<[u8]> = std::sync::Arc::from(face.data.clone());
    let gid = parse_uncached(&face.data).lookup_glyph_index('A');

    let parse_start = Instant::now();
    let _ = parse_uncached(&face.data);
    let parse = parse_start.elapsed();

    // Warm this thread's cache through one rasterizer…
    let first = FontdueRasterizer::new();
    let _ = first.raster(gid, &shared, 64.0);

    // …then time a brand-new one's very first glyph.
    let second = FontdueRasterizer::new();
    let start = Instant::now();
    let bitmap = second
        .raster(gid, &shared, 64.0)
        .expect("the test face must rasterize");
    let first_call = start.elapsed();

    eprintln!(
        "[perf] second FontdueRasterizer first call: {first_call:?} against a {parse:?} face parse ({} px)",
        bitmap.pixels.len()
    );
    assert!(
        first_call * 10 < parse,
        "a new rasterizer's first glyph cost {first_call:?} against a {parse:?} parse — \
         it is parsing the face again instead of using the thread-local cache"
    );
}

/// The cache must hand back the same parsed instance, not a copy of it.
#[test]
fn cache_returns_shared_handles() {
    let face = load_test_face();
    let a = oxitext_raster::get_or_parse_fontdue(&face.data).expect("test face must parse");
    let b = oxitext_raster::get_or_parse_fontdue(&face.data).expect("test face must parse");
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "get_or_parse_fontdue must return shared Arc handles"
    );
}
