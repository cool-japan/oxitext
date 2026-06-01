#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! `oxitext-sdf` — Signed Distance Field glyph atlas generation for OxiText.
//!
//! Provides a pure-Rust implementation of the Felzenszwalb-Huttenlocher EDT
//! for computing signed distance fields from glyph coverage bitmaps, an
//! atlas packer for producing GPU-ready SDF texture atlases, and a multi-channel
//! SDF (MSDF) pipeline that generates 3-channel distance fields directly from
//! glyph outlines for sharper rendering at large magnifications.
//!
//! # Quick start (single-channel SDF)
//!
//! ```rust
//! use oxitext_sdf::{compute_sdf, SdfAtlas, SdfTile};
//!
//! // A solid 32×32 square (inside everywhere).
//! let coverage = vec![255u8; 32 * 32];
//! let sdf = compute_sdf(&coverage, 32, 32, 8.0, 0).expect("compute_sdf");
//! assert_eq!(sdf.len(), 32 * 32);
//!
//! // Pack a single tile into an atlas.
//! let tile = SdfTile {
//!     glyph_id: 0,
//!     width: 32,
//!     height: 32,
//!     data: sdf,
//!     bearing_x: 0,
//!     bearing_y: 0,
//!     advance_x: 32.0,
//! };
//! let atlas = SdfAtlas::pack(&[tile]);
//! assert!(atlas.uv_map.contains_key(&0));
//! ```

pub mod analytic;
mod atlas;
pub mod build_helper;
mod convert;
mod edt;
mod gpu;
pub mod msdf;
pub mod psdf;

pub use analytic::glyph_to_sdf_tile_analytic;
pub use atlas::{
    pack_growing, AtlasOptions, AtlasStats, MsdfAtlas, MultiPageAtlas, PackingAlgorithm, SdfAtlas,
    SdfTile, UvRect,
};
pub use build_helper::{generate_ascii_atlas, generate_atlas_binary};
pub use convert::bitmap_to_sdf_tile;
pub use edt::{compute_sdf, SdfError};
pub use gpu::{AtlasGlyphMetrics, GpuAtlasDescriptor, GpuAtlasFormat, NormalizedUvRect};
pub use msdf::{
    color_edges, compute_msdf, compute_mtsdf, extract_glyph_shape, glyph_to_msdf_tile,
    glyph_to_mtsdf_tile, EdgeColor, GlyphShape, MsdfTile, MtsdfTile,
};
pub use psdf::{glyph_to_psdf_tile, PsdfTile};

// ─── Bilinear resampling ──────────────────────────────────────────────────────

/// Sample a float-valued image at fractional coordinates using bilinear interpolation.
///
/// `u` and `v` are pixel-space coordinates (not normalised UV).  Values are
/// clamped to the image bounds.
fn bilinear_sample(src: &[f32], src_w: usize, src_h: usize, u: f32, v: f32) -> f32 {
    let x = u.clamp(0.0, (src_w as f32) - 1.0);
    let y = v.clamp(0.0, (src_h as f32) - 1.0);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(src_w - 1);
    let y1 = (y0 + 1).min(src_h - 1);
    let fx = x - x.floor();
    let fy = y - y.floor();
    let s00 = src[y0 * src_w + x0];
    let s10 = src[y0 * src_w + x1];
    let s01 = src[y1 * src_w + x0];
    let s11 = src[y1 * src_w + x1];
    s00 * (1.0 - fx) * (1.0 - fy) + s10 * fx * (1.0 - fy) + s01 * (1.0 - fx) * fy + s11 * fx * fy
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Generate a signed-distance-field tile for a single glyph.
///
/// Input: a grayscale coverage bitmap (fontdue/ab_glyph output,
/// 0 = outside, 255 = maximum inside coverage).
///
/// Output: an SDF bitmap of size `tile_size × tile_size`, where:
/// - pixel value `< 128` = outside the outline,
/// - pixel value `≈ 128` = near the outline (the 0.5 isovalue),
/// - pixel value `> 128` = inside the outline.
///
/// The SDF is computed at the source resolution (`src_width × src_height`)
/// and then the tile is returned at `tile_size × tile_size`. When the source
/// dimensions match `tile_size`, the result is returned directly. Otherwise
/// bilinear resampling is applied.
///
/// The default spread (maximum SDF distance) is `8.0` pixels.
///
/// # Errors
/// Propagates [`SdfError::InvalidInput`] if the input is malformed.
pub fn glyph_to_sdf_tile(
    coverage: &[u8],
    src_width: usize,
    src_height: usize,
    tile_size: u32,
) -> Result<Vec<u8>, SdfError> {
    let tile = tile_size as usize;

    // Compute SDF at source resolution (no padding).
    let sdf_src = compute_sdf(coverage, src_width, src_height, 8.0, 0)?;

    // If dimensions already match, return directly.
    if src_width == tile && src_height == tile {
        return Ok(sdf_src);
    }

    // Convert u8 SDF to f32 for bilinear sampling.
    let sdf_f32: Vec<f32> = sdf_src.iter().map(|&v| v as f32).collect();

    // Bilinear resample to tile_size × tile_size.
    let mut out = vec![0u8; tile * tile];
    for ty in 0..tile {
        for tx in 0..tile {
            // Map destination pixel centre to source pixel space.
            let u = (tx as f32 + 0.5) * src_width as f32 / tile as f32 - 0.5;
            let v = (ty as f32 + 0.5) * src_height as f32 / tile as f32 - 0.5;
            let val = bilinear_sample(&sdf_f32, src_width, src_height, u, v);
            out[ty * tile + tx] = val.clamp(0.0, 255.0).round() as u8;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod bench_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bilinear_sample_corner_values() {
        // A 2×2 grid: top-left=0, top-right=255, bottom-left=128, bottom-right=64
        let grid = [0.0f32, 255.0, 128.0, 64.0];
        let width = 2usize;
        let height = 2usize;
        // Sample exact corners — should return exact values
        let tl = bilinear_sample(&grid, width, height, 0.0, 0.0);
        let tr = bilinear_sample(&grid, width, height, 1.0, 0.0);
        assert!((tl - 0.0).abs() < 1.0, "top-left should be ~0, got {tl}");
        assert!(
            (tr - 255.0).abs() < 1.0,
            "top-right should be ~255, got {tr}"
        );
    }

    #[test]
    fn test_bilinear_sample_midpoint_interpolation() {
        // Uniform grid should return the same value anywhere
        let grid = vec![100.0f32; 4];
        let v = bilinear_sample(&grid, 2, 2, 0.5, 0.5);
        assert!(
            (v - 100.0).abs() < 0.01,
            "uniform grid midpoint should be 100.0, got {v}"
        );
    }

    #[test]
    fn test_bilinear_sample_clamps_out_of_bounds() {
        // Out-of-bounds coordinates should clamp to nearest edge
        let grid = [10.0f32, 20.0, 30.0, 40.0];
        let tl = bilinear_sample(&grid, 2, 2, -1.0, -1.0);
        assert!(
            (tl - 10.0).abs() < 0.01,
            "negative coords should clamp to top-left, got {tl}"
        );
        let br = bilinear_sample(&grid, 2, 2, 100.0, 100.0);
        assert!(
            (br - 40.0).abs() < 0.01,
            "large coords should clamp to bottom-right, got {br}"
        );
    }
}
