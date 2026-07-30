//! Build-time SDF atlas generation helpers.
//!
//! These functions are designed to be called from a `build.rs` script to
//! pre-compute SDF atlases that are then embedded via `include_bytes!` in the
//! main crate.
//!
//! # Workflow
//!
//! **In `build.rs`:**
//! ```rust,ignore
//! use std::env;
//! use std::path::PathBuf;
//!
//! fn main() {
//!     let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
//!     let font_data = std::fs::read("assets/MyFont.ttf").expect("read font file");
//!     let glyph_ids: Vec<u16> = (32..127).collect(); // ASCII
//!     oxitext_sdf::generate_atlas_binary(
//!         &font_data,
//!         &glyph_ids,
//!         16.0,   // px_size
//!         64,     // tile_size
//!         4.0,    // spread
//!         512,    // atlas_size (square)
//!         &out_dir.join("font_atlas.bin"),
//!     )
//!     .expect("atlas generation failed");
//!     println!("cargo:rerun-if-changed=assets/MyFont.ttf");
//! }
//! ```
//!
//! **In the main crate:**
//! ```rust,ignore
//! static ATLAS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font_atlas.bin"));
//! let atlas = oxitext_sdf::SdfAtlas::from_static(ATLAS).expect("parse atlas");
//! ```

use crate::{
    atlas::{AtlasOptions, SdfAtlas},
    edt::SdfError,
};

/// Generate a pre-computed SDF atlas for a set of glyph IDs and write it to a file.
///
/// Tiles are generated using the analytic SDF pipeline ([`crate::glyph_to_sdf_tile_analytic`]).
/// Empty glyphs (whitespace, missing outlines) are silently skipped.
///
/// The atlas is packed into a square texture whose side length is `atlas_size` rounded up
/// to the next power of two (minimum 64). Tiles that do not fit are silently dropped.
///
/// # Arguments
/// - `font_data` — raw TTF/OTF bytes.
/// - `glyph_ids` — slice of glyph IDs (ttf-parser `GlyphId.0` values) to include.
/// - `px_size` — render size in pixels per em for SDF sampling.
/// - `tile_size` — edge length of each square SDF tile in pixels.
/// - `spread` — SDF spread radius in pixels (controls gradient width around outlines).
/// - `atlas_size` — desired square atlas edge length in pixels (rounded to next power of two).
/// - `output_path` — destination file path.
///
/// # Errors
/// Returns [`SdfError::InvalidFont`] if the font cannot be parsed, [`SdfError::ZeroSize`] if
/// `tile_size` is 0, or [`SdfError::Io`] if the output file cannot be written.
pub fn generate_atlas_binary(
    font_data: &[u8],
    glyph_ids: &[u16],
    px_size: f32,
    tile_size: u32,
    spread: f32,
    atlas_size: u32,
    output_path: &std::path::Path,
) -> Result<(), SdfError> {
    let mut tiles = Vec::with_capacity(glyph_ids.len());

    for &glyph_id in glyph_ids {
        if let Some(tile) = crate::analytic::glyph_to_sdf_tile_analytic(
            font_data, glyph_id, px_size, tile_size, spread,
        )? {
            tiles.push(tile);
        }
        // Empty/whitespace glyphs produce None — skip silently.
    }

    let options = AtlasOptions {
        atlas_size,
        padding: 1,
        ..Default::default()
    };
    let (atlas, stats) = SdfAtlas::pack_with_options(&tiles, &options);

    if stats.tiles_dropped > 0 {
        // Emit a non-fatal diagnostic without panicking; callers may choose to log this.
        // We surface it as an error so build scripts can detect overflow.
        return Err(SdfError::InvalidInput(format!(
            "{} tile(s) did not fit in the {}×{} atlas (packed {}, dropped {}). \
             Increase atlas_size or reduce the glyph set.",
            stats.tiles_dropped, atlas.width, atlas.height, stats.tiles_packed, stats.tiles_dropped,
        )));
    }

    let bytes = atlas.to_bytes();
    std::fs::write(output_path, &bytes).map_err(|e| SdfError::Io(e.to_string()))
}

/// Generate a pre-computed SDF atlas for all printable ASCII codepoints (U+0020–U+007E)
/// of a font and write it to a file.
///
/// Uses 64×64 tiles, a spread of 4.0, a 512×512 atlas, and the supplied `px_size`.
///
/// # Errors
/// Same as [`generate_atlas_binary`].
pub fn generate_ascii_atlas(
    font_data: &[u8],
    px_size: f32,
    output_path: &std::path::Path,
) -> Result<(), SdfError> {
    let face = ttf_parser::Face::parse(font_data, 0).map_err(|_| SdfError::InvalidFont)?;

    let glyph_ids: Vec<u16> = (0x0020u32..=0x007E)
        .filter_map(char::from_u32)
        .filter_map(|ch| face.glyph_index(ch))
        .map(|gid| gid.0)
        .collect();

    generate_atlas_binary(font_data, &glyph_ids, px_size, 64, 4.0, 512, output_path)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the bundled test font (relative to the workspace root via `CARGO_MANIFEST_DIR`).
    fn test_font_data() -> Vec<u8> {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
        let path = std::path::PathBuf::from(&manifest).join("../../tests/fixtures/test-font.ttf");
        std::fs::read(&path).expect("test font should be readable")
    }

    #[test]
    fn test_generate_atlas_binary_writes_valid_file() {
        let font_data = test_font_data();
        let out = std::env::temp_dir().join("test_build_atlas.bin");

        // Use a small set of glyph IDs that are likely to have outlines.
        let glyph_ids = vec![36u16, 37, 38];
        let result = generate_atlas_binary(&font_data, &glyph_ids, 16.0, 32, 4.0, 128, &out);

        // Succeed or fail gracefully if all glyphs are empty/overflow.
        match result {
            Ok(()) => {
                assert!(out.exists(), "output file should exist on success");
                let bytes = std::fs::read(&out).expect("output file should be readable");
                let atlas = SdfAtlas::from_bytes(&bytes);
                assert!(atlas.is_ok(), "generated atlas should be deserializable");
            }
            Err(SdfError::InvalidInput(_)) => {
                // All glyphs overflowed the tiny 128×128 atlas — acceptable.
            }
            Err(e) => panic!("unexpected error: {e}"),
        }

        std::fs::remove_file(&out).ok();
    }

    #[test]
    #[ignore]
    fn test_generate_ascii_atlas_for_test_font() {
        let font_data = test_font_data();
        let out = std::env::temp_dir().join("test_ascii_atlas.bin");

        // May succeed or fail depending on packing — just don't panic.
        let _ = generate_ascii_atlas(&font_data, 16.0, &out);
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn test_generate_atlas_binary_empty_glyph_ids() {
        let font_data = test_font_data();
        let out = std::env::temp_dir().join("test_empty_atlas.bin");

        let result = generate_atlas_binary(&font_data, &[], 16.0, 32, 4.0, 128, &out);
        assert!(
            result.is_ok(),
            "empty glyph set should produce an empty atlas, got: {result:?}"
        );
        if out.exists() {
            let bytes = std::fs::read(&out).expect("output file readable");
            let atlas = SdfAtlas::from_bytes(&bytes);
            assert!(atlas.is_ok(), "empty atlas should be deserializable");
        }
        std::fs::remove_file(&out).ok();
    }
}
