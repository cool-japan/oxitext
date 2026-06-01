// Bench: rasterization benchmarks for oxitext FontdueRasterizer.

use criterion::{criterion_group, criterion_main, Criterion};
use oxitext_raster::FontdueRasterizer;
use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;

fn load_bench_font() -> Arc<[u8]> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        return Arc::from(
            std::fs::read(&fixture)
                .expect("read fixture font")
                .into_boxed_slice(),
        );
    }
    let candidates = [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            return Arc::from(
                std::fs::read(p)
                    .expect("read system font")
                    .into_boxed_slice(),
            );
        }
    }
    Arc::from(Box::new([]) as Box<[u8]>)
}

/// Benchmark rasterizing a single glyph at 16px.
fn bench_raster_glyph_16px(c: &mut Criterion) {
    let font_data = load_bench_font();
    let rasterizer = FontdueRasterizer::new();
    c.bench_function("raster_glyph_16px", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            // GID 36 is typically 'A' in standard Latin fonts.
            let result = rasterizer.raster(black_box(36), &font_data, black_box(16.0_f32));
            black_box(result.ok());
        })
    });
}

/// Benchmark rasterizing a single glyph at 48px (larger bitmap path).
fn bench_raster_glyph_48px(c: &mut Criterion) {
    let font_data = load_bench_font();
    let rasterizer = FontdueRasterizer::new();
    c.bench_function("raster_glyph_48px", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            let result = rasterizer.raster(black_box(36), &font_data, black_box(48.0_f32));
            black_box(result.ok());
        })
    });
}

/// Benchmark rasterizing multiple glyphs in sequence to exercise font cache.
fn bench_raster_multi_glyph(c: &mut Criterion) {
    let font_data = load_bench_font();
    let rasterizer = FontdueRasterizer::new();
    // GIDs 36–47 span a short range of Latin glyphs.
    let gids: Vec<u16> = (36..48).collect();
    c.bench_function("raster_multi_glyph_12", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            for &gid in &gids {
                let result = rasterizer.raster(black_box(gid), &font_data, black_box(16.0_f32));
                black_box(result.ok());
            }
        })
    });
}

/// Benchmark the scalar coverage accumulation path.
fn bench_accumulate_coverage_scalar(c: &mut Criterion) {
    let src: Vec<f32> = (0..256).map(|i| (i as f32) / 255.0).collect();
    let mut dst = vec![0.0_f32; 256];
    c.bench_function("accumulate_coverage_scalar_256", |b| {
        b.iter(|| {
            oxitext_raster::accumulate_coverage(black_box(&mut dst), black_box(src.as_slice()));
        })
    });
}

/// Benchmark the f32-to-u8 coverage conversion.
fn bench_coverage_f32_to_u8(c: &mut Criterion) {
    let src: Vec<f32> = (0..256).map(|i| (i as f32) / 255.0).collect();
    let mut dst = vec![0_u8; 256];
    c.bench_function("coverage_f32_to_u8_256", |b| {
        b.iter(|| {
            oxitext_raster::coverage_f32_to_u8(black_box(&mut dst), black_box(src.as_slice()));
        })
    });
}

criterion_group!(
    benches,
    bench_raster_glyph_16px,
    bench_raster_glyph_48px,
    bench_raster_multi_glyph,
    bench_accumulate_coverage_scalar,
    bench_coverage_f32_to_u8,
);
criterion_main!(benches);
