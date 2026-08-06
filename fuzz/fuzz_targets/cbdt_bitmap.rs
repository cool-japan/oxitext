//! Fuzz target for `oxitext-raster`'s color-glyph-strike parsers over
//! untrusted font bytes: `detect_color_glyph_type`, `extract_cbdt_bitmap`,
//! and `render_cbdt_glyph` (CBDT/CBLC and `sbix` raster-strike extraction,
//! including the PNG-encoded strike path via `decode_png_to_bitmap` --
//! enabled here through this crate's `png-bitmap` feature on its
//! `oxitext-raster` dependency, see `fuzz/Cargo.toml`).
//!
//! None of these functions should ever panic on malformed input: each is
//! documented to return `None`/`ColorGlyphType::None` for data it cannot
//! parse or decode.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run cbdt_bitmap
//! ```
//!
//! # Known finding (upstream, not an oxitext defect)
//!
//! A short run of this target reliably reproduces a `debug_assert!` panic
//! inside `ttf-parser` 0.25.1's `Stream::read_bytes`
//! (`Stream::read_bytes` -> `RawFace::parse` -> `Face::parse`, called from
//! `detect_color_glyph_type`): `assertion failed: self.offset as u64 + len
//! as u64 <= u32::MAX as u64`. This fires ONLY because `cargo fuzz`
//! deliberately builds with `-C debug-assertions -C overflow-checks` even in
//! its `release` profile, specifically so `debug_assert!`-guarded invariants
//! like this one are exercised. It is not reachable in a normal `cargo
//! build --release` of any oxitext crate (or of a downstream consumer that
//! does not itself opt into `debug-assertions = true`): with the assert
//! compiled out, the very next line — `self.data.get(self.offset..
//! self.offset + len)?` — is `slice::get`, which returns `None` on an
//! out-of-range range instead of panicking, so the call safely bubbles up as
//! `Face::parse`'s `Err`/`detect_color_glyph_type`'s `ColorGlyphType::None`
//! exactly as documented. 0.25.1 is the latest version on crates.io (checked
//! via `cargo info ttf-parser`), so there is no newer release to pick up a
//! fix, and `ttf-parser` is an external crate this workspace cannot patch.
//! Left as-is (not silenced with `catch_unwind`, which would mask genuine
//! future oxitext-side panics on this same fuzz target) so this comment is
//! the record of the finding for whoever next runs this target; consider
//! reporting it upstream at <https://github.com/harfbuzz/ttf-parser>.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Steal a handful of leading bytes to drive the (glyph_id, ppem/px_size)
    // parameters, and treat the remainder as font bytes -- the same "some
    // structure, rest is a blob" split libfuzzer-oriented targets commonly
    // use without pulling in the `arbitrary` crate.
    if data.len() < 5 {
        return;
    }
    let glyph_id = u16::from_le_bytes([data[0], data[1]]);
    let target_ppem = data[2];
    let px_size = u16::from_le_bytes([data[3], data[4]]);
    let face_data = &data[5..];

    let _ = oxitext_raster::detect_color_glyph_type(face_data, glyph_id);
    let _ = oxitext_raster::extract_cbdt_bitmap(face_data, glyph_id, target_ppem);
    let _ = oxitext_raster::render_cbdt_glyph(face_data, glyph_id, px_size);
});
