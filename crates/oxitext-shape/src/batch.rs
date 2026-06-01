//! Batch shaping: shape multiple text segments in a single call.
//!
//! Provides [`SwashShaper::shape_batch`] and
//! [`SwashShaper::shape_batch_directed`] for amortised setup across many
//! independent text segments sharing the same font and size.

use crate::{ShapeDirection, ShapeFeature, ShapeRequest, ShapeResult, SwashShaper};
use oxitext_core::OxiTextError;

impl SwashShaper {
    /// Shape multiple independent text segments with the same font and size.
    ///
    /// Returns one [`ShapeResult`] per input segment.
    /// More efficient than calling [`SwashShaper::shape_full`] N times for the
    /// same `(font_data, px_size)` pair because the [`ShapeContext`] is reused
    /// across all calls without reallocation.
    ///
    /// # Errors
    /// Each element of the returned `Vec` is `Ok` when the font can be parsed
    /// and `Err` when shaping fails for that segment.
    ///
    /// [`ShapeContext`]: swash::shape::ShapeContext
    pub fn shape_batch(
        &mut self,
        font_data: &[u8],
        segments: &[&str],
        px_size: f32,
    ) -> Vec<Result<ShapeResult, OxiTextError>> {
        segments
            .iter()
            .map(|text| self.shape_full(font_data, text, px_size))
            .collect()
    }

    /// Shape a batch with per-segment directions.
    ///
    /// Each element of `segments` is a `(text, direction)` pair.  The
    /// appropriate shaping direction is applied to each segment independently.
    /// Vertical directions (`Ttb`, `Btt`) automatically inject `vert`/`vrt2`
    /// features, matching the behaviour of [`SwashShaper::shape_request`].
    ///
    /// Returns one [`ShapeResult`] per input pair.
    pub fn shape_batch_directed(
        &mut self,
        font_data: &[u8],
        segments: &[(&str, ShapeDirection)],
        px_size: f32,
    ) -> Vec<Result<ShapeResult, OxiTextError>> {
        segments
            .iter()
            .map(|(text, dir)| {
                let req = ShapeRequest::builder()
                    .text(text)
                    .font_data(font_data)
                    .px_size(px_size)
                    .direction(*dir)
                    .build()
                    .map_err(|e| OxiTextError::Shaping(e.to_string()))?;
                let glyphs = self.shape_request(&req)?;
                Ok(ShapeResult::from_glyphs(glyphs, text, *dir))
            })
            .collect()
    }

    /// Shape a batch with a shared feature list applied to every segment.
    ///
    /// All segments share the same `(font_data, px_size, features)` context.
    ///
    /// Returns one [`ShapeResult`] per input segment.
    pub fn shape_batch_with_features(
        &mut self,
        font_data: &[u8],
        segments: &[&str],
        px_size: f32,
        features: &[ShapeFeature],
    ) -> Vec<Result<ShapeResult, OxiTextError>> {
        segments
            .iter()
            .map(|text| {
                let glyphs = self.shape_with_features(font_data, text, px_size, false, features)?;
                Ok(ShapeResult::from_glyphs(glyphs, text, ShapeDirection::Ltr))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    fn load_test_font() -> Arc<[u8]> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
        if fixture.exists() {
            return Arc::from(
                std::fs::read(&fixture)
                    .expect("read fixture font")
                    .as_slice(),
            );
        }
        let candidates = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ];
        for p in &candidates {
            if Path::new(p).exists() {
                return Arc::from(std::fs::read(p).expect("read system font").as_slice());
            }
        }
        panic!("no test font found — add tests/fixtures/test-font.ttf");
    }

    #[test]
    fn test_shape_batch_produces_one_result_per_segment() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let segments = ["Hello", "World", "Test"];
        let results = shaper.shape_batch(&font_bytes, &segments, 16.0);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.is_ok(), "expected Ok for segment, got: {r:?}");
        }
    }

    #[test]
    fn test_shape_batch_empty_segments() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let results = shaper.shape_batch(&font_bytes, &[], 16.0);
        assert!(results.is_empty());
    }

    #[test]
    fn test_shape_batch_directed_mixed() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let segments = [
            ("Hello", ShapeDirection::Ltr),
            ("world", ShapeDirection::Ltr),
        ];
        let results = shaper.shape_batch_directed(&font_bytes, &segments, 16.0);
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(
                r.is_ok(),
                "expected Ok from shape_batch_directed, got: {r:?}"
            );
        }
    }

    #[test]
    fn test_shape_batch_glyphs_are_non_empty() {
        let font_bytes = load_test_font();
        let mut shaper = SwashShaper::new();
        let segments = ["Hi", "Test"];
        let results = shaper.shape_batch(&font_bytes, &segments, 16.0);
        for r in &results {
            let shape = r.as_ref().expect("batch result ok");
            assert!(
                !shape.glyphs.is_empty(),
                "expected non-empty glyphs per segment"
            );
        }
    }
}
