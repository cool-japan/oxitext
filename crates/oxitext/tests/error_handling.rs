//! Integration tests for error handling in the Pipeline facade:
//! - `Pipeline::from_bytes` with invalid data
//! - `PipelineBuilder` validation
//! - `Pipeline::available_features` soundness
//! - Glyph/bitmap length invariants

use oxitext::Pipeline;
use std::path::Path;

fn load_test_font() -> Vec<u8> {
    // 1. Project fixture (deterministic, checked in).
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        return std::fs::read(&fixture).expect("read fixture font");
    }
    // 2. Bundled Noto Sans Regular — always available, no system font required.
    oxifont_bundled::NOTO_SANS_REGULAR.to_vec()
}

/// `Pipeline::from_bytes` must return an error for obviously invalid font data.
#[test]
fn pipeline_from_bytes_invalid_data() {
    let result = Pipeline::from_bytes(b"not a valid font");
    assert!(result.is_err(), "invalid font data should return an error");
}

/// `Pipeline::from_bytes` must return an error for empty bytes.
#[test]
fn pipeline_from_bytes_empty() {
    let result = Pipeline::from_bytes(b"");
    assert!(result.is_err(), "empty font data should return an error");
}

/// `Pipeline::builder()` with no font set must return `FontNotFound`.
#[test]
fn pipeline_builder_no_font_returns_error() {
    let result = Pipeline::builder().build();
    assert!(result.is_err(), "builder with no font should fail");
}

/// `Pipeline::builder()` with invalid font bytes must return an error.
#[test]
fn pipeline_builder_invalid_font() {
    let result = Pipeline::builder().font(b"invalid".to_vec()).build();
    assert!(
        result.is_err(),
        "builder with invalid font data should fail"
    );
}

/// `Pipeline::builder()` with a valid font must succeed.
#[test]
fn pipeline_builder_valid_font() {
    let data = load_test_font();
    let result = Pipeline::builder().font(data).build();
    assert!(
        result.is_ok(),
        "builder with valid font data should succeed"
    );
}

/// `Pipeline::available_features()` must not panic and always returns a `Vec`.
#[test]
fn available_features_does_not_panic() {
    let features = Pipeline::available_features();
    // May be empty if compiled with no features; that is fine.
    // When compiled with `--all-features` it must include "pure".
    for f in features {
        assert!(!f.is_empty(), "feature name must not be empty");
    }
}

/// When compiled with the `pure` feature (the default), `available_features`
/// must include `"pure"`.
#[test]
#[cfg(feature = "pure")]
fn available_features_includes_pure() {
    let features = Pipeline::available_features();
    assert!(
        features.contains(&"pure"),
        "expected \"pure\" in available_features(), got: {features:?}"
    );
}

/// `RenderResult::glyphs` and `RenderResult::bitmaps` must have equal lengths.
#[test]
fn render_result_glyph_bitmap_lengths_match() {
    let data = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&data).expect("valid font");
    let style = oxitext::TextStyle::default();
    let result = pipeline.render("Hello", &style).expect("render");
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "glyphs and bitmaps must have the same length"
    );
    assert!(
        !result.glyphs.is_empty(),
        "render result must have at least one glyph"
    );
}

/// `Pipeline::from_bytes` must succeed for a valid font file.
#[test]
fn pipeline_from_bytes_valid_font() {
    let data = load_test_font();
    let pipeline = Pipeline::from_bytes(&data);
    assert!(
        pipeline.is_ok(),
        "from_bytes with a valid font must succeed"
    );
}
