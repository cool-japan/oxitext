//! GPU-ready atlas descriptor types.
//!
//! Provides a backend-agnostic description of an SDF or MSDF atlas texture.
//! Callers can pass the [`GpuAtlasDescriptor`] directly to wgpu, Vulkan, Metal,
//! or OpenGL without any dependency on those crates from this library.

use std::collections::HashMap;

use crate::atlas::{MsdfAtlas, SdfAtlas, UvRect};

// ─── Format ───────────────────────────────────────────────────────────────────

/// Pixel format for GPU atlas texture upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuAtlasFormat {
    /// Single-channel greyscale (1 byte/pixel) — for single-channel SDF.
    R8Unorm,
    /// Three-channel RGB (3 bytes/pixel) — for MSDF.
    Rgb8Unorm,
}

// ─── Normalized UV ────────────────────────────────────────────────────────────

/// UV rectangle in normalized [0, 1] texture coordinates.
///
/// All four fields are in the range `[0.0, 1.0]` where `(0, 0)` is the
/// top-left corner and `(1, 1)` is the bottom-right corner of the texture.
#[derive(Debug, Clone, Copy)]
pub struct NormalizedUvRect {
    /// Left edge (U coordinate, column-axis).
    pub u_min: f32,
    /// Top edge (V coordinate, row-axis).
    pub v_min: f32,
    /// Right edge (U coordinate).
    pub u_max: f32,
    /// Bottom edge (V coordinate).
    pub v_max: f32,
}

impl From<&UvRect> for NormalizedUvRect {
    /// Convert from the atlas-internal [`UvRect`] (which already stores [0, 1] values).
    fn from(r: &UvRect) -> Self {
        Self {
            u_min: r.u_min,
            v_min: r.v_min,
            u_max: r.u_max,
            v_max: r.v_max,
        }
    }
}

// ─── Glyph metrics ────────────────────────────────────────────────────────────

/// Per-glyph metrics stored alongside the atlas for rendering.
///
/// These are the values needed by a text renderer to correctly position and
/// size each glyph quad relative to the cursor position.
#[derive(Debug, Clone, Copy)]
pub struct AtlasGlyphMetrics {
    /// Horizontal offset from the cursor to the left edge of the glyph (pixels, signed).
    pub bearing_x: f32,
    /// Vertical offset from the baseline to the top edge of the glyph (pixels, signed).
    pub bearing_y: f32,
    /// Horizontal advance to the next cursor position (pixels).
    pub advance_x: f32,
    /// Glyph width in pixels (matches the packed tile width).
    pub width_px: u32,
    /// Glyph height in pixels (matches the packed tile height).
    pub height_px: u32,
}

// ─── Descriptor ───────────────────────────────────────────────────────────────

/// A GPU-ready atlas descriptor: all the information needed to upload the atlas
/// to a GPU texture (wgpu, Vulkan, Metal, OpenGL) without depending on any GPU crate.
///
/// # Usage
/// ```rust
/// use oxitext_sdf::{SdfAtlas, GpuAtlasFormat};
///
/// let atlas = SdfAtlas::new(256, 256);
/// let desc = atlas.to_gpu_descriptor();
/// assert_eq!(desc.format, GpuAtlasFormat::R8Unorm);
/// assert_eq!(desc.data.len(), 256 * 256);
/// ```
#[derive(Debug, Clone)]
pub struct GpuAtlasDescriptor {
    /// Atlas texture width in pixels.
    pub width: u32,
    /// Atlas texture height in pixels.
    pub height: u32,
    /// Pixel format of the texture data.
    pub format: GpuAtlasFormat,
    /// Raw texture bytes (`width × height × bytes_per_pixel`).
    pub data: Vec<u8>,
    /// UV rectangle for each glyph ID, in normalized [0, 1] texture coordinates.
    pub uv_map: HashMap<u16, NormalizedUvRect>,
    /// Per-glyph metrics in atlas space (bearing, advance in pixels).
    ///
    /// Currently populated only when the atlas was built via APIs that retain
    /// tile metrics. Empty when tile metrics were not preserved by the packer.
    pub glyph_metrics: HashMap<u16, AtlasGlyphMetrics>,
}

// ─── SdfAtlas → GpuAtlasDescriptor ───────────────────────────────────────────

impl SdfAtlas {
    /// Produce a GPU-ready descriptor containing the atlas texture and normalized UV coordinates.
    ///
    /// The returned struct contains everything needed to upload the atlas to a GPU texture.
    /// `glyph_metrics` will be empty because the shelf packer does not retain per-tile metrics
    /// after the atlas is built. If you need metrics, store the [`SdfTile`] list alongside the
    /// atlas before calling `pack`.
    ///
    /// [`SdfTile`]: crate::SdfTile
    pub fn to_gpu_descriptor(&self) -> GpuAtlasDescriptor {
        let uv_map: HashMap<u16, NormalizedUvRect> = self
            .uv_map
            .iter()
            .map(|(&glyph_id, uv)| (glyph_id, NormalizedUvRect::from(uv)))
            .collect();

        GpuAtlasDescriptor {
            width: self.width,
            height: self.height,
            format: GpuAtlasFormat::R8Unorm,
            data: self.texture.clone(),
            uv_map,
            glyph_metrics: HashMap::new(),
        }
    }
}

// ─── MsdfAtlas → GpuAtlasDescriptor ──────────────────────────────────────────

impl MsdfAtlas {
    /// Produce a GPU-ready descriptor for this MSDF atlas.
    ///
    /// Returns [`GpuAtlasFormat::Rgb8Unorm`] (3 bytes per pixel) with the raw
    /// RGB texture data.  `glyph_metrics` is empty for the same reason as for
    /// [`SdfAtlas::to_gpu_descriptor`].
    pub fn to_gpu_descriptor(&self) -> GpuAtlasDescriptor {
        let uv_map: HashMap<u16, NormalizedUvRect> = self
            .uv_map
            .iter()
            .map(|(&glyph_id, uv)| (glyph_id, NormalizedUvRect::from(uv)))
            .collect();

        GpuAtlasDescriptor {
            width: self.width,
            height: self.height,
            format: GpuAtlasFormat::Rgb8Unorm,
            data: self.texture.clone(),
            uv_map,
            glyph_metrics: HashMap::new(),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{SdfAtlas, SdfTile};

    fn make_tile(glyph_id: u16, w: u32, h: u32) -> SdfTile {
        SdfTile {
            glyph_id,
            width: w,
            height: h,
            data: vec![128u8; (w * h) as usize],
            bearing_x: 0,
            bearing_y: 0,
            advance_x: w as f32,
        }
    }

    #[test]
    fn test_gpu_descriptor_uv_normalized() {
        let atlas = SdfAtlas::new(256, 256);
        let desc = atlas.to_gpu_descriptor();
        assert_eq!(desc.width, 256);
        assert_eq!(desc.height, 256);
        assert_eq!(desc.format, GpuAtlasFormat::R8Unorm);
        assert_eq!(desc.data.len(), 256 * 256);
        // No tiles were packed — uv_map should be empty.
        assert!(desc.uv_map.is_empty());
    }

    #[test]
    fn test_gpu_descriptor_uv_values_in_range() {
        let tiles = [make_tile(0, 16, 16), make_tile(1, 32, 32)];
        let atlas = SdfAtlas::pack(&tiles);
        let desc = atlas.to_gpu_descriptor();

        for uv in desc.uv_map.values() {
            assert!(
                uv.u_min >= 0.0 && uv.u_min <= 1.0,
                "u_min out of range: {}",
                uv.u_min
            );
            assert!(
                uv.u_max >= 0.0 && uv.u_max <= 1.0,
                "u_max out of range: {}",
                uv.u_max
            );
            assert!(
                uv.v_min >= 0.0 && uv.v_min <= 1.0,
                "v_min out of range: {}",
                uv.v_min
            );
            assert!(
                uv.v_max >= 0.0 && uv.v_max <= 1.0,
                "v_max out of range: {}",
                uv.v_max
            );
            assert!(uv.u_min <= uv.u_max, "u_min > u_max");
            assert!(uv.v_min <= uv.v_max, "v_min > v_max");
        }
    }

    #[test]
    fn test_msdf_gpu_descriptor_format() {
        let atlas = MsdfAtlas {
            width: 128,
            height: 128,
            texture: vec![0u8; 128 * 128 * 3],
            uv_map: HashMap::new(),
        };
        let desc = atlas.to_gpu_descriptor();
        assert_eq!(desc.format, GpuAtlasFormat::Rgb8Unorm);
        assert_eq!(desc.width, 128);
        assert_eq!(desc.height, 128);
        assert_eq!(desc.data.len(), 128 * 128 * 3);
        assert!(desc.uv_map.is_empty());
    }

    #[test]
    fn test_normalized_uv_rect_from_uv_rect() {
        let uv = UvRect {
            u_min: 0.1,
            v_min: 0.2,
            u_max: 0.5,
            v_max: 0.8,
        };
        let norm = NormalizedUvRect::from(&uv);
        assert!((norm.u_min - 0.1).abs() < f32::EPSILON);
        assert!((norm.v_min - 0.2).abs() < f32::EPSILON);
        assert!((norm.u_max - 0.5).abs() < f32::EPSILON);
        assert!((norm.v_max - 0.8).abs() < f32::EPSILON);
    }
}
