//! Devanagari reph-reordering regression corpus.
//!
//! These tests pin the two defects fixed in `crates/oxitext-swash`
//! (OxiText's vendored fork of `swash` 0.2.10 — see that crate's
//! `PROVENANCE.md`), both caused by one bug in
//! `swash::shape::buffer::reorder_complex`:
//!
//! * **(a) silent reph loss / glyph duplication** — a reph (`र्`) came out as a
//!   duplicate of its neighbour instead of the repositioned `ra`;
//! * **(b) `index out of bounds` panic** at `shape/buffer.rs:680` — upstream
//!   `dfrg/swash` issue #93, "Panic While Shaping".
//!
//! Mechanism: `reorder_complex` writes a permutation into `order`, a scratch
//! `Vec<usize>` owned by swash's `ShapeContext` that `State::reset()` never
//! clears and `Vec::resize` only ever grows.  When the fill loop ends with
//! `j < len`, the tail of `order[..len]` still holds indices written by a
//! *previous* cluster — so a stale index `< len` duplicates a glyph and drops
//! the intended one, and a stale index `>= len` indexes past the slice.
//!
//! Two consequences shape this file:
//!
//! * the defects need **no** state setup to appear (case 1 fails on a brand-new
//!   shaper), and
//! * they are **cluster-to-cluster inside a single `add_str`**, so a
//!   multi-word single call reproduces them (case 2).
//!
//! Glyph-id goldens are specific to Noto Sans Devanagari **2.006**
//! (`tests/fixtures/NotoSansDevanagari-Regular.ttf`, SHA-256 pinned in
//! `tests/fixtures/README.md`).  Cases 4 and 5 are pure properties — no
//! reference implementation and no font version — and are the half that
//! survives a fixture update.
//!
//! Every test skips gracefully when the fixture is absent, per
//! `tests/fixtures/README.md`.

use oxitext_core::ShapedGlyph;
use oxitext_shape::{ShapeDirection, ShapeRequest, SwashShaper};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

/// The 24-word Hindi corpus, transcribed from the OxiGIS print canary
/// (`crates/oxigis-ui/src/print/shape.rs`).  Twelve reph-bearing words, then
/// twelve mixed conjunct/matra words.
const HINDI_CORPUS: [&str; 24] = [
    "कर्म",
    "धर्म",
    "वर्ष",
    "मार्ग",
    "सूर्य",
    "पूर्व",
    "कार्य",
    "दर्शन",
    "पर्वत",
    "सर्व",
    "गर्व",
    "अर्थ",
    "स्वर्ग",
    "निर्माण",
    "पूर्ण",
    "वर्तमान",
    "वर्षा",
    "आदर्श",
    "संघर्ष",
    "उत्तर",
    "दिल्ली",
    "हिन्दी",
    "भारत",
    "कि",
];

/// The repositioned-`ra` (reph) glyph of Noto Sans Devanagari 2.006.
const NOTO_REPH_GID: u16 = 506;

/// OpenType script tag for Devanagari v2 shaping rules.
const DEV2: [u8; 4] = *b"dev2";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}"))
}

/// Loads the Devanagari fixture, or `None` when a checkout has no binaries.
fn load_devanagari() -> Option<Vec<u8>> {
    let path = fixture("NotoSansDevanagari-Regular.ttf");
    if !path.exists() {
        eprintln!("skipping: {} is absent", path.display());
        return None;
    }
    std::fs::read(&path).ok()
}

/// `Some(font)` or an early `return` — the skip-gracefully contract.
macro_rules! devanagari_or_skip {
    () => {
        match load_devanagari() {
            Some(font) => font,
            None => return,
        }
    };
}

fn shape_dev(shaper: &mut SwashShaper, font: &[u8], text: &str) -> Vec<ShapedGlyph> {
    let req = ShapeRequest::builder()
        .text(text)
        .font_data(font)
        .px_size(32.0)
        .direction(ShapeDirection::Ltr)
        .script(DEV2)
        .build()
        .expect("shape request builds");
    shaper.shape_request(&req).expect("font parses")
}

fn gids(glyphs: &[ShapedGlyph]) -> Vec<u16> {
    glyphs.iter().map(|g| g.gid).collect()
}

/// What shaping one string did — glyphs, or the panic that defect (b) raised.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Glyphs(Vec<u16>),
    Panicked,
}

/// Shapes `text`, converting defect (b)'s panic into a value so a whole corpus
/// can be swept in one test and reported as a count rather than a stack trace.
fn shape_catching(shaper: &mut SwashShaper, font: &[u8], text: &str) -> Outcome {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = catch_unwind(AssertUnwindSafe(|| gids(&shape_dev(shaper, font, text))));
    std::panic::set_hook(previous);
    match caught {
        Ok(g) => Outcome::Glyphs(g),
        Err(_) => Outcome::Panicked,
    }
}

/// No two neighbouring glyphs share a glyph id.
///
/// This is the cheap, font-independent shape of defect (a): the stale index
/// made `reorder_complex` emit one glyph twice in a row and drop another.
fn adjacent_duplicates(ids: &[u16]) -> Vec<(usize, u16)> {
    ids.windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == w[1])
        .map(|(i, w)| (i, w[0]))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 1 — the strongest case: a fresh shaper, no state setup at all.
// ─────────────────────────────────────────────────────────────────────────────

/// `स्वर्ग` ("heaven") on a brand-new shaper must keep its reph.
///
/// Pre-fix this returned `[256, 84, 58, 58]`: the second cluster's fill loop
/// ended at `j = 2` with `len = 3`, so `order[2]` still held the `2` left by the
/// first cluster — the base — and the reph was replaced by a copy of it.
#[test]
fn reph_survives_a_fresh_shaper() {
    let font = devanagari_or_skip!();
    let mut shaper = SwashShaper::new();
    let ids = gids(&shape_dev(&mut shaper, &font, "स्वर्ग"));
    assert_eq!(
        ids,
        vec![256, 84, 58, NOTO_REPH_GID],
        "स्वर्ग must end in the reph, not a duplicate of the base"
    );
    assert!(
        adjacent_duplicates(&ids).is_empty(),
        "स्वर्ग must not contain an adjacent duplicate glyph: {ids:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 2 — the same staleness *inside one call*, across two words.
// ─────────────────────────────────────────────────────────────────────────────

/// Two words in a single `add_str` — the second inherits the first's `order`.
///
/// The two-*call* form of this pair (`दिल्ली` then `मार्ग` on the same shaper)
/// deliberately has no test: it reproduces on Windows' Nirmala but not on Noto,
/// because the stale index depends on the previous cluster's length, which is
/// font-specific.  The one-call phrase reproduces on both.
#[test]
fn reph_survives_two_words_in_one_call() {
    let font = devanagari_or_skip!();
    let mut shaper = SwashShaper::new();
    let ids = gids(&shape_dev(&mut shaper, &font, "दिल्ली मार्ग"));
    let tail = &ids[ids.len().saturating_sub(2)..];
    assert_eq!(
        tail,
        [58, NOTO_REPH_GID],
        "'दिल्ली मार्ग' must end with the base then its reph; got {ids:?}"
    );
    assert!(
        adjacent_duplicates(&ids).is_empty(),
        "'दिल्ली मार्ग' must not contain an adjacent duplicate glyph: {ids:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 3 — defect (b): the out-of-bounds panic.
// ─────────────────────────────────────────────────────────────────────────────

/// A four-word phrase in one call must not panic.
///
/// Pre-fix a stale index `>= len` reached `glyphs[i] = buf[*j]` at
/// `shape/buffer.rs:680` — the exact site of upstream `dfrg/swash` issue #93.
/// In a wasm build (OxiGIS's map) that panic aborts the whole canvas.
#[test]
fn four_word_phrase_does_not_panic() {
    let font = devanagari_or_skip!();
    let mut shaper = SwashShaper::new();
    let outcome = shape_catching(&mut shaper, &font, "सूर्य पूर्व वर्षा मार्ग");
    let ids = match outcome {
        Outcome::Glyphs(ids) => ids,
        Outcome::Panicked => panic!(
            "'सूर्य पूर्व वर्षा मार्ग' panicked in reorder_complex \
             (dfrg/swash#93, shape/buffer.rs:680)"
        ),
    };
    assert_eq!(ids.len(), 19, "expected 19 glyphs, got {ids:?}");
    assert_eq!(
        ids.iter().filter(|&&g| g == NOTO_REPH_GID).count(),
        4,
        "each of the four words carries exactly one reph; got {ids:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 4 — pure property: shaper reuse must not change the answer.
// ─────────────────────────────────────────────────────────────────────────────

/// One reused shaper must agree with one fresh shaper per word, for all 24.
///
/// This needs no reference implementation and no font version: shaping is a
/// pure function of (font, text, params), and the defect was precisely a
/// violation of that purity through `ShapeContext`'s never-cleared scratch.
#[test]
fn reused_shaper_matches_fresh_shaper_over_the_corpus() {
    let font = devanagari_or_skip!();
    let mut reused = SwashShaper::new();
    let mut disagreements = Vec::new();
    for word in HINDI_CORPUS {
        let mut fresh = SwashShaper::new();
        let a = shape_catching(&mut fresh, &font, word);
        let b = shape_catching(&mut reused, &font, word);
        if a != b {
            disagreements.push(format!("{word}: fresh={a:?} reused={b:?}"));
        }
    }
    assert!(
        disagreements.is_empty(),
        "a reused shaper must equal a fresh one; {} of {} words disagree:\n  {}",
        disagreements.len(),
        HINDI_CORPUS.len(),
        disagreements.join("\n  ")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 5 — pure property: no panic anywhere in the corpus, one shaper.
// ─────────────────────────────────────────────────────────────────────────────

/// All 24 corpus words through ONE shaper: zero panics, zero adjacent
/// duplicates.
///
/// Pre-fix, four words panicked (`पूर्ण`, `वर्तमान`, `आदर्श`, `संघर्ष`) and
/// several more silently lost their reph to a duplicate.
#[test]
fn corpus_shapes_without_panicking() {
    let font = devanagari_or_skip!();
    let mut shaper = SwashShaper::new();
    let mut panicked = Vec::new();
    let mut duplicated = Vec::new();
    for word in HINDI_CORPUS {
        match shape_catching(&mut shaper, &font, word) {
            Outcome::Panicked => panicked.push(word),
            Outcome::Glyphs(ids) => {
                let dups = adjacent_duplicates(&ids);
                if !dups.is_empty() {
                    duplicated.push(format!("{word}: {ids:?} duplicates at {dups:?}"));
                }
            }
        }
    }
    assert!(
        panicked.is_empty(),
        "{} of {} corpus words panicked: {panicked:?}",
        panicked.len(),
        HINDI_CORPUS.len()
    );
    assert!(
        duplicated.is_empty(),
        "{} corpus words emitted an adjacent duplicate glyph:\n  {}",
        duplicated.len(),
        duplicated.join("\n  ")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 6 — control: the Simple-mode paths must be untouched.
// ─────────────────────────────────────────────────────────────────────────────

/// Latin shaping must be byte-identical before and after the reordering fix.
///
/// `reorder_complex`/`reorder_myanmar` are reached only from
/// `EngineMode::Complex`, and mode selection requires `script.is_complex()`.
/// Latin, Cyrillic, Greek, Arabic, Hebrew, CJK and Hangul all take
/// `EngineMode::Simple` and never enter either function, so this control is
/// expected GREEN on both sides of the fix — that is its whole job.
#[test]
fn latin_is_byte_identical_across_the_fix() {
    let path = fixture("test-font.ttf");
    if !path.exists() {
        eprintln!("skipping: {} is absent", path.display());
        return;
    }
    let Ok(font) = std::fs::read(&path) else {
        return;
    };

    let mut shaper = SwashShaper::new();
    let latin = |s: &mut SwashShaper| -> Vec<u16> {
        let req = ShapeRequest::builder()
            .text("Hello, World! Affix office fi fl")
            .font_data(&font)
            .px_size(32.0)
            .direction(ShapeDirection::Ltr)
            .script(*b"latn")
            .build()
            .expect("shape request builds");
        gids(&s.shape_request(&req).expect("font parses"))
    };

    let first = latin(&mut shaper);
    assert!(!first.is_empty(), "Latin shaping must produce glyphs");

    // Purity across reuse, and across a Devanagari call in between: the fix
    // must not have made Simple mode depend on the Complex scratch buffer.
    if let Some(dev) = load_devanagari() {
        let mut interleaved = SwashShaper::new();
        let before = latin(&mut interleaved);
        let _ = shape_catching(&mut interleaved, &dev, "स्वर्ग");
        let after = latin(&mut interleaved);
        assert_eq!(
            before, after,
            "Latin output must not change after a Devanagari call on the same shaper"
        );
        assert_eq!(before, first, "Latin output must not depend on shaper age");
    }

    let mut fresh = SwashShaper::new();
    assert_eq!(
        first,
        latin(&mut fresh),
        "Latin output must be identical on a fresh shaper"
    );
}
