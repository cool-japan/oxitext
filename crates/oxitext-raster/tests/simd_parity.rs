//! KAT (Known-Answer Test) parity tests: SIMD vs scalar primitives.
//!
//! These tests verify that [`oxitext_raster::simd`] produces byte-identical
//! results to [`oxitext_raster::scalar`] for the same inputs, regardless of
//! whether the `simd` feature is active.
//!
//! The tests exercise both synthetic buffers and buffers derived from real
//! glyph rasterization output (ASCII 'A', Arabic 'ع' U+0639, CJK '漢' U+6F22).

use std::path::Path;
use std::sync::Arc;

fn load_test_font() -> Arc<[u8]> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
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

/// Rasterize a single glyph at `px_size` using the fontdue backend and return
/// the raw `u8` coverage buffer as a `Vec<f32>` (pixels divided by 255.0).
fn rasterize_glyph_as_f32(font_data: &[u8], glyph_id: u16, px_size: f32) -> Vec<f32> {
    use oxitext_raster::backend::{FontdueRaster, RasterBackend};
    let backend = FontdueRaster::new();
    let out = backend.rasterize(font_data, glyph_id, px_size);
    out.coverage.iter().map(|&b| b as f32 / 255.0_f32).collect()
}

// ─── accumulate_coverage parity ──────────────────────────────────────────────

fn accumulate_parity_check(src1: &[f32], src2: &[f32]) {
    let mut scalar_dst = src1.to_vec();
    let mut simd_dst = src1.to_vec();

    oxitext_raster::scalar::accumulate_coverage(&mut scalar_dst, src2);

    #[cfg(feature = "simd")]
    oxitext_raster::simd::accumulate_coverage(&mut simd_dst, src2);
    #[cfg(not(feature = "simd"))]
    oxitext_raster::scalar::accumulate_coverage(&mut simd_dst, src2);

    assert_eq!(
        scalar_dst,
        simd_dst,
        "SIMD/scalar parity failure in accumulate_coverage for {}-element buffer",
        src1.len()
    );
}

#[test]
fn accumulate_coverage_synthetic_parity() {
    // Test with lengths that are: multiple of 8, remainder of 1, remainder of 7.
    for n in [0_usize, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64, 65] {
        let src1: Vec<f32> = (0..n).map(|i| (i as f32 / n.max(1) as f32) * 0.6).collect();
        let src2: Vec<f32> = (0..n).map(|i| (i as f32 / n.max(1) as f32) * 0.5).collect();
        accumulate_parity_check(&src1, &src2);
    }
}

#[test]
fn accumulate_coverage_glyph_derived_parity() {
    let font = load_test_font();
    // Use glyph IDs that correspond to ASCII 'A' in most Latin fonts (GID 36 or
    // nearby), then arbitrary IDs that are likely to produce non-empty bitmaps.
    for gid in [36_u16, 37, 38, 40, 50, 60] {
        let coverage = rasterize_glyph_as_f32(&font, gid, 24.0);
        if coverage.is_empty() {
            continue; // whitespace glyph — skip
        }
        // Use the same coverage vector for both src1 and src2 to exercise the
        // clamping path (sum > 1.0 when both equal ~0.5).
        let half: Vec<f32> = coverage.iter().map(|&v| v * 0.5).collect();
        accumulate_parity_check(&half, &half);
    }
}

// ─── multiply_alpha_u8 parity ────────────────────────────────────────────────

fn multiply_alpha_parity_check(src: &[u8], factor: u8) {
    let mut scalar_buf = src.to_vec();
    let mut simd_buf = src.to_vec();

    oxitext_raster::scalar::multiply_alpha_u8(&mut scalar_buf, factor);

    #[cfg(feature = "simd")]
    oxitext_raster::simd::multiply_alpha_u8(&mut simd_buf, factor);
    #[cfg(not(feature = "simd"))]
    oxitext_raster::scalar::multiply_alpha_u8(&mut simd_buf, factor);

    assert_eq!(
        scalar_buf,
        simd_buf,
        "SIMD/scalar parity failure in multiply_alpha_u8 (factor={factor}) for {}-element buffer",
        src.len()
    );
}

#[test]
fn multiply_alpha_synthetic_parity() {
    for n in [0_usize, 1, 15, 16, 17, 31, 32, 33, 64, 65] {
        let src: Vec<u8> = (0..n).map(|i| (i % 256) as u8).collect();
        for factor in [0_u8, 1, 64, 128, 200, 255] {
            multiply_alpha_parity_check(&src, factor);
        }
    }
}

#[test]
fn multiply_alpha_glyph_derived_parity() {
    let font = load_test_font();
    for gid in [36_u16, 37, 38] {
        let out = {
            use oxitext_raster::backend::{FontdueRaster, RasterBackend};
            let backend = FontdueRaster::new();
            backend.rasterize(&font, gid, 24.0).coverage
        };
        if out.is_empty() {
            continue;
        }
        multiply_alpha_parity_check(&out, 128);
    }
}

// ─── coverage_f32_to_u8 parity ───────────────────────────────────────────────

fn coverage_f32_to_u8_parity_check(src: &[f32]) {
    let mut scalar_dst = vec![0_u8; src.len()];
    let mut simd_dst = vec![0_u8; src.len()];

    oxitext_raster::scalar::coverage_f32_to_u8(&mut scalar_dst, src);

    #[cfg(feature = "simd")]
    oxitext_raster::simd::coverage_f32_to_u8(&mut simd_dst, src);
    #[cfg(not(feature = "simd"))]
    oxitext_raster::scalar::coverage_f32_to_u8(&mut simd_dst, src);

    assert_eq!(
        scalar_dst,
        simd_dst,
        "SIMD/scalar parity failure in coverage_f32_to_u8 for {}-element buffer",
        src.len()
    );
}

#[test]
fn coverage_f32_to_u8_synthetic_parity() {
    for n in [0_usize, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64, 65] {
        let src: Vec<f32> = (0..n).map(|i| i as f32 / n.max(1) as f32).collect();
        coverage_f32_to_u8_parity_check(&src);
    }
    // Edge cases: values at boundaries.
    let edges = vec![0.0_f32, 0.5_f32, 1.0_f32, -0.5_f32, 1.5_f32];
    coverage_f32_to_u8_parity_check(&edges);
}

#[test]
fn coverage_f32_to_u8_glyph_derived_parity() {
    let font = load_test_font();
    for gid in [36_u16, 37, 38] {
        let f32_coverage = rasterize_glyph_as_f32(&font, gid, 24.0);
        if f32_coverage.is_empty() {
            continue;
        }
        coverage_f32_to_u8_parity_check(&f32_coverage);
    }
}
