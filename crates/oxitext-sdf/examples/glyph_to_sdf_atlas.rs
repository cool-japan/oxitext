//! Demonstrates the primary SDF flow of `oxitext-sdf`: turning real glyph
//! outlines from a font into a packed, serializable SDF atlas.
//!
//! Two complementary paths are shown:
//!
//! 1. **Runtime path** — [`glyph_to_sdf_tile_analytic`] converts a single
//!    glyph outline straight into an [`SdfTile`] (no rasterization step
//!    needed; the analytic pipeline walks the outline directly), which is
//!    then packed into an [`SdfAtlas`], serialized with
//!    [`SdfAtlas::to_bytes`], and read back with [`SdfAtlas::from_bytes`].
//!    This is the shape a GPU text renderer uses at draw time, e.g. via
//!    `oxitext::Pipeline::render_to_sdf_atlas`, which packs exactly the
//!    `(glyph_id, px_size)` pairs a `LayoutResult` reports needing
//!    (`LayoutResult::unique_glyphs_for_atlas`, demonstrated in the
//!    `oxitext-layout` crate's `word_aware_layout` example).
//! 2. **Build-time path** — [`generate_ascii_atlas`] is meant to be called
//!    from a `build.rs` script: it renders every printable ASCII glyph in
//!    one call and writes a ready-to-embed atlas file to disk.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-sdf --example glyph_to_sdf_atlas
//! ```

use oxitext_sdf::{generate_ascii_atlas, glyph_to_sdf_tile_analytic, SdfAtlas};

/// Font bytes embedded at compile time from the workspace's checked-in test
/// fixture. Resolved relative to this source file, so it works regardless
/// of the process's current working directory.
const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

fn main() {
    // ─── 1. Runtime path: single-glyph analytic SDF, packed and round-tripped ───

    let face = ttf_parser::Face::parse(FONT, 0).expect("bundled test font must parse");

    // Collect unique glyph IDs for a handful of printable ASCII characters
    // that are likely to have non-empty outlines in any reasonable font.
    // Deduplicated because an `SdfAtlas` keys its `uv_map` by glyph ID, and
    // "OxiText42" repeats a couple of letters.
    let mut seen = std::collections::HashSet::new();
    let glyph_ids: Vec<u16> = "OxiText42"
        .chars()
        .filter_map(|ch| face.glyph_index(ch))
        .map(|gid| gid.0)
        .filter(|&gid| seen.insert(gid))
        .collect();
    assert!(!glyph_ids.is_empty(), "test font should cover ASCII");

    let px_size = 32.0;
    let tile_size = 48;
    let spread = 4.0;

    let mut tiles = Vec::new();
    for &gid in &glyph_ids {
        // `Ok(None)` means the glyph has no outline (e.g. space) — skip it.
        if let Some(tile) = glyph_to_sdf_tile_analytic(FONT, gid, px_size, tile_size, spread)
            .expect("analytic SDF generation failed")
        {
            tiles.push(tile);
        }
    }
    println!(
        "generated {} SDF tile(s) from {} glyph(s)",
        tiles.len(),
        glyph_ids.len()
    );
    assert!(!tiles.is_empty(), "expected at least one non-empty glyph");

    let atlas = SdfAtlas::pack(&tiles);
    println!(
        "packed into a {}x{} atlas covering {} glyph(s)",
        atlas.width,
        atlas.height,
        atlas.uv_map.len()
    );
    assert_eq!(atlas.uv_map.len(), tiles.len());

    // Serialize to the atlas's on-disk binary format and read it back — the
    // same round trip a renderer performs when caching an atlas between runs.
    let bytes = atlas.to_bytes();
    let reloaded = SdfAtlas::from_bytes(&bytes).expect("just-serialized atlas must deserialize");
    assert_eq!(reloaded.width, atlas.width);
    assert_eq!(reloaded.height, atlas.height);
    assert_eq!(reloaded.uv_map.len(), atlas.uv_map.len());
    println!(
        "round-tripped atlas through {} byte(s) of the binary atlas format",
        bytes.len()
    );

    // ─── 2. Build-time path: whole-font ASCII atlas written straight to disk ───

    // Per COOLJAPAN policy, temp files use `std::env::temp_dir()` rather than
    // a hardcoded path.
    let out_path = std::env::temp_dir().join("oxitext_sdf_example_ascii_atlas.bin");

    // `generate_ascii_atlas` uses a fixed 512x512 canvas with 64px tiles,
    // which comfortably fits a typical UI font's printable-ASCII glyph set
    // but can overflow for fonts with unusually large glyph counts in that
    // range (like this workspace's synthetic test fixture). It reports that
    // condition as `SdfError::InvalidInput` rather than silently truncating
    // the atlas, so a caller can decide how to react — here we just report
    // it and fall back to the fully parameterized `generate_atlas_binary`
    // with a larger canvas that is guaranteed to fit everything.
    match generate_ascii_atlas(FONT, 24.0, &out_path) {
        Ok(()) => {
            let atlas_bytes =
                std::fs::read(&out_path).expect("just-written atlas file must be readable");
            let ascii_atlas =
                SdfAtlas::from_bytes(&atlas_bytes).expect("generated atlas must be valid");
            println!(
                "build-time ASCII atlas: {}x{}, {} glyph(s), written to {}",
                ascii_atlas.width,
                ascii_atlas.height,
                ascii_atlas.uv_map.len(),
                out_path.display()
            );
        }
        Err(e) => {
            println!("default 512x512 ASCII atlas did not fit this font's glyph set: {e}");
            println!("retrying with a larger, explicitly-sized atlas via generate_atlas_binary");

            let ascii_glyph_ids: Vec<u16> = (0x0020u32..=0x007E)
                .filter_map(char::from_u32)
                .filter_map(|ch| face.glyph_index(ch))
                .map(|gid| gid.0)
                .collect();
            oxitext_sdf::generate_atlas_binary(
                FONT,
                &ascii_glyph_ids,
                24.0,
                64,
                4.0,
                1024,
                &out_path,
            )
            .expect("a 1024x1024 canvas must fit the full printable-ASCII glyph set");
            let atlas_bytes =
                std::fs::read(&out_path).expect("just-written atlas file must be readable");
            let ascii_atlas =
                SdfAtlas::from_bytes(&atlas_bytes).expect("generated atlas must be valid");
            println!(
                "build-time ASCII atlas: {}x{}, {} glyph(s), written to {}",
                ascii_atlas.width,
                ascii_atlas.height,
                ascii_atlas.uv_map.len(),
                out_path.display()
            );
            assert!(!ascii_atlas.uv_map.is_empty());
        }
    }

    // Clean up the temp file; examples should not leave stray state behind.
    let _ = std::fs::remove_file(&out_path);
}
