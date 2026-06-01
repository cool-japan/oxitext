//! Unified raster output combining coverage bitmap with glyph metrics.
//!
//! [`RasterResult`] wraps a [`oxitext_core::RenderOutput`] together with the
//! per-glyph layout metrics (advances and bearings) so that callers can handle
//! both the pixel data and the positioning information from a single value.

use oxitext_core::{Bitmap, RenderOutput};

/// Unified raster output: coverage bitmap plus glyph metrics.
///
/// Every [`crate::backend::RasterBackend::rasterize_full`] call returns a
/// `RasterResult`.  For greyscale paths the `output` variant is
/// [`RenderOutput::Greyscale`]; for LCD or color paths it will be the
/// appropriate variant.
#[derive(Debug, Clone)]
pub struct RasterResult {
    /// The pixel data produced by rasterization.
    pub output: RenderOutput,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
    /// Vertical advance in pixels (usually `0.0` for LTR fonts).
    pub advance_y: f32,
    /// Left bearing in pixels (signed; distance from pen to left glyph edge).
    pub bearing_x: i32,
    /// Top bearing in pixels (signed; distance from baseline to top glyph edge).
    pub bearing_y: i32,
}

impl RasterResult {
    /// Construct an empty [`RasterResult`] representing a zero-coverage glyph
    /// (e.g. whitespace) with all metrics set to zero.
    pub fn empty() -> Self {
        Self {
            output: RenderOutput::Greyscale(Bitmap {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            }),
            advance_x: 0.0,
            advance_y: 0.0,
            bearing_x: 0,
            bearing_y: 0,
        }
    }

    /// Returns `true` if the raster output contains no pixel data.
    ///
    /// Whitespace glyphs and rasterization failures produce empty results.
    pub fn is_empty(&self) -> bool {
        match &self.output {
            RenderOutput::Greyscale(b) => b.pixels.is_empty(),
            RenderOutput::Color(b) => b.rgba.is_empty(),
            RenderOutput::Lcd(b) => b.rgb.is_empty(),
            RenderOutput::Sdf { data, .. } => data.is_empty(),
            RenderOutput::Msdf { data, .. } => data.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_result_is_empty() {
        let r = RasterResult::empty();
        assert!(r.is_empty());
        assert_eq!(r.advance_x, 0.0);
        assert_eq!(r.bearing_x, 0);
    }

    #[test]
    fn non_empty_greyscale_is_not_empty() {
        let r = RasterResult {
            output: RenderOutput::Greyscale(Bitmap {
                width: 1,
                height: 1,
                pixels: vec![128],
            }),
            advance_x: 8.0,
            advance_y: 0.0,
            bearing_x: 0,
            bearing_y: 8,
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn is_empty_matches_all_variants() {
        use oxitext_core::{ColorBitmap, LcdBitmap};
        // Sdf empty
        let sdf = RasterResult {
            output: RenderOutput::Sdf {
                width: 0,
                height: 0,
                data: vec![],
            },
            ..RasterResult::empty()
        };
        assert!(sdf.is_empty());
        // Lcd non-empty
        let lcd = RasterResult {
            output: RenderOutput::Lcd(LcdBitmap::new(1, 1, vec![0, 0, 0])),
            ..RasterResult::empty()
        };
        assert!(!lcd.is_empty());
        // Color non-empty
        let color = RasterResult {
            output: RenderOutput::Color(ColorBitmap {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            }),
            ..RasterResult::empty()
        };
        assert!(!color.is_empty());
        // Msdf empty
        let msdf = RasterResult {
            output: RenderOutput::Msdf {
                width: 0,
                height: 0,
                data: vec![],
            },
            ..RasterResult::empty()
        };
        assert!(msdf.is_empty());
    }
}
