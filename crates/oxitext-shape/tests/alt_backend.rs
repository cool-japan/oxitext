//! Alternative shaping backend integration tests.
//!
//! Verifies that `SwashShaperBackend` and `RustybuzzShaper` (when enabled)
//! produce the same number of glyphs for a simple ASCII string.

use oxitext_shape::backend::{ShapeBackend, SwashShaperBackend};
use std::path::Path;
use std::sync::Arc;

fn load_test_font() -> Arc<[u8]> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        let bytes = std::fs::read(&fixture).expect("read fixture font");
        return Arc::from(bytes.as_slice());
    }
    let candidates = [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            let bytes = std::fs::read(p).expect("read system font");
            return Arc::from(bytes.as_slice());
        }
    }
    panic!("no test font found — add tests/fixtures/test-font.ttf");
}

/// SwashShaperBackend must produce non-zero advances for "Hello".
#[test]
fn swash_backend_shapes_hello() {
    let font_data = load_test_font();
    let shaper = SwashShaperBackend::new();
    let glyphs = shaper.shape(&font_data, "Hello", 16.0);
    assert!(
        !glyphs.is_empty(),
        "SwashShaperBackend must produce glyphs for 'Hello'"
    );
    for g in &glyphs {
        assert!(
            g.x_advance > 0.0,
            "all glyphs from SwashShaperBackend must have positive x_advance"
        );
    }
}

/// rustybuzz backend must produce the same glyph count as swash for "Hello".
#[test]
#[cfg(feature = "rustybuzz-backend")]
fn rustybuzz_backend_agrees_with_swash() {
    use oxitext_shape::backend::RustybuzzShaper;

    let font_data = load_test_font();

    let swash = SwashShaperBackend::new();
    let ruzz = RustybuzzShaper;

    let swash_glyphs = swash.shape(&font_data, "Hello", 16.0);
    let ruzz_glyphs = ruzz.shape(&font_data, "Hello", 16.0);

    assert_eq!(
        swash_glyphs.len(),
        ruzz_glyphs.len(),
        "SwashShaperBackend and RustybuzzShaper must produce the same glyph \
         count for 'Hello' (swash={}, rustybuzz={})",
        swash_glyphs.len(),
        ruzz_glyphs.len()
    );
}

/// rustybuzz backend: advances must be positive for "Hello" when enabled.
#[test]
#[cfg(feature = "rustybuzz-backend")]
fn rustybuzz_backend_positive_advances() {
    use oxitext_shape::backend::RustybuzzShaper;

    let font_data = load_test_font();
    let shaper = RustybuzzShaper;
    let glyphs = shaper.shape(&font_data, "Hello", 16.0);
    assert!(
        !glyphs.is_empty(),
        "RustybuzzShaper must produce glyphs for 'Hello'"
    );
    for g in &glyphs {
        assert!(
            g.x_advance > 0.0,
            "all glyphs from RustybuzzShaper must have positive x_advance"
        );
    }
}
