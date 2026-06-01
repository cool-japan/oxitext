//! Conversion utilities: coverage bitmaps → SDF tiles.
//!
//! Bridges `oxitext-raster` / `oxitext-core` bitmap output with the SDF pipeline.

use oxitext_core::Bitmap;

use crate::atlas::SdfTile;

/// Convert a greyscale coverage bitmap (from `oxitext-raster`) to a signed distance field tile.
///
/// The coverage bitmap should be a rasterized glyph (1 byte per pixel, 0 = transparent,
/// 255 = fully covered). Internally runs the Euclidean Distance Transform on the coverage
/// bitmap to produce the SDF.
///
/// # Arguments
/// - `bitmap`: coverage bitmap from `FontdueRaster::rasterize()` or similar.
/// - `glyph_id`: glyph index (stored in the tile for atlas lookup).
/// - `bearing_x`, `bearing_y`: font metrics in pixels (signed, from cursor to glyph left/top).
/// - `advance_x`: horizontal advance in pixels.
/// - `spread`: distance in pixels mapped to the full [0, 255] SDF range (typically 4.0–8.0).
///
/// # Returns
/// `None` if `bitmap` has zero width, zero height, or empty pixel data.
/// `None` if the EDT fails (should not happen for well-formed bitmaps).
pub fn bitmap_to_sdf_tile(
    bitmap: &Bitmap,
    glyph_id: u16,
    bearing_x: i32,
    bearing_y: i32,
    advance_x: f32,
    spread: f32,
) -> Option<SdfTile> {
    if bitmap.width == 0 || bitmap.height == 0 || bitmap.pixels.is_empty() {
        return None;
    }
    let sdf_data = crate::edt::compute_sdf(
        &bitmap.pixels,
        bitmap.width as usize,
        bitmap.height as usize,
        spread,
        0,
    )
    .ok()?;
    Some(SdfTile {
        glyph_id,
        width: bitmap.width,
        height: bitmap.height,
        data: sdf_data,
        bearing_x,
        bearing_y,
        advance_x,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_to_sdf_tile_basic() {
        // 4×4 coverage bitmap: all-zero border, opaque 2×2 centre.
        let pixels = vec![0u8, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0];
        let bitmap = Bitmap {
            width: 4,
            height: 4,
            pixels,
        };
        let tile = bitmap_to_sdf_tile(&bitmap, 65, 0, 0, 8.0, 4.0);
        assert!(tile.is_some());
        let tile = tile.unwrap();
        assert_eq!(tile.glyph_id, 65);
        assert_eq!(tile.data.len(), 16);
        assert_eq!(tile.width, 4);
        assert_eq!(tile.height, 4);
    }

    #[test]
    fn test_bitmap_to_sdf_tile_empty_returns_none() {
        let bitmap = Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        assert!(bitmap_to_sdf_tile(&bitmap, 0, 0, 0, 0.0, 4.0).is_none());
    }

    #[test]
    fn test_bitmap_to_sdf_tile_zero_width_returns_none() {
        let bitmap = Bitmap {
            width: 0,
            height: 4,
            pixels: vec![],
        };
        assert!(bitmap_to_sdf_tile(&bitmap, 1, 0, 0, 8.0, 4.0).is_none());
    }

    #[test]
    fn test_bitmap_to_sdf_tile_bearing_and_advance_stored() {
        let pixels = vec![128u8; 4];
        let bitmap = Bitmap {
            width: 2,
            height: 2,
            pixels,
        };
        let tile = bitmap_to_sdf_tile(&bitmap, 32, -1, 12, 16.0, 6.0);
        let tile = tile.expect("should produce a tile for a valid 2×2 bitmap");
        assert_eq!(tile.bearing_x, -1);
        assert_eq!(tile.bearing_y, 12);
        assert!((tile.advance_x - 16.0).abs() < f32::EPSILON);
    }
}
