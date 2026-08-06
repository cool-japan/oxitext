//! Fuzz target for the vendored shaper in `crates/oxitext-swash`, driven with
//! arbitrary bytes as the font.
//!
//! `oxitext-swash` is a vendored fork of `swash` 0.2.10 (see that crate's
//! `PROVENANCE.md`) and is the only crate in this workspace without
//! `#![forbid(unsafe_code)]`: it inherits 54 `unsafe` sites -- 37 blocks and 17
//! `unsafe fn` across 8 files -- essentially all `read_unchecked::<T>()` over
//! untrusted font bytes in the table parsers. `FontRef::from_index` plus a full
//! `ShapeContext` shaping pass is the widest reachable slice of that surface.
//!
//! The strings are fixed rather than fuzzed, and shaped under
//! `Script::Devanagari`, on purpose: that is the `EngineMode::Complex` arm
//! containing `reorder_complex`, the function whose stale-`order` defect this
//! release fixes (silent glyph duplication, and the `index out of bounds` of
//! upstream `dfrg/swash` issue #93). The Arabic and Latin strings keep the
//! joined and `EngineMode::Simple` paths covered in the same run. One
//! `ShapeContext` is reused across all of them, because the fixed defect was
//! precisely a cross-cluster state leak through that reused context -- a
//! per-string fresh context would not have reproduced it.
//!
//! `debug_assert_eq!(j, len, "reorder_complex must produce a full permutation")`
//! is live here: `cargo fuzz` builds with `-C debug-assertions`, so a future
//! regression that leaks a stale index is a fuzz finding rather than a silent
//! identity fallback.
//!
//! **This target is not a gate.** Running a campaign will very likely reproduce
//! upstream's own open fuzz findings in the inherited parse layer (dfrg/swash
//! #123-#126 and #133), which are recorded as known-unfixed in `PROVENANCE.md`
//! and are not triaged as part of the 0.2.2 absorption. Any *new* finding --
//! anything that reproduces against pristine swash 0.2.10 too, or anything in
//! the reordering code this release touched -- goes to `TODO.md` as a 0.2.3
//! item.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run shape_untrusted_font
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxitext_swash::shape::{Direction, ShapeContext};
use oxitext_swash::text::Script;
use oxitext_swash::FontRef;

/// Devanagari words that reproduced the fixed defects, one reph-bearing
/// conjunct each; Arabic for the joined path; Latin for `EngineMode::Simple`.
const STRINGS: &[(&str, Script, Direction)] = &[
    ("स्वर्ग", Script::Devanagari, Direction::LeftToRight),
    ("सूर्य पूर्व वर्षा मार्ग", Script::Devanagari, Direction::LeftToRight),
    ("दिल्ली मार्ग", Script::Devanagari, Direction::LeftToRight),
    ("संघर्ष", Script::Devanagari, Direction::LeftToRight),
    ("العربية", Script::Arabic, Direction::RightToLeft),
    ("Hello, World!", Script::Latin, Direction::LeftToRight),
];

fuzz_target!(|data: &[u8]| {
    // Arbitrary bytes as a font: most inputs are rejected here, and that is
    // the point -- `from_index` parses the collection header and table
    // directory out of them before anything else runs.
    let Some(font) = FontRef::from_index(data, 0) else {
        return;
    };

    // One context for the whole batch: the defect this release fixed lived in
    // scratch state that survives between clusters and between calls.
    let mut context = ShapeContext::new();
    for (text, script, direction) in STRINGS {
        let mut shaper = context
            .builder(font)
            .script(*script)
            .direction(*direction)
            .size(16.0)
            .build();
        shaper.add_str(text);
        // The only property under test is "does not panic": no out-of-bounds
        // index, no failed `debug_assert`, no arithmetic overflow in a debug
        // build, and no `unwrap`/`expect` anywhere on this path (the crate
        // denies both lints).
        shaper.shape_with(|_cluster| {});
    }
});
