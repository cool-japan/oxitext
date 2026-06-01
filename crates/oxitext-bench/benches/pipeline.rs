// Bench: end-to-end Pipeline::render benchmarks.

use criterion::{criterion_group, criterion_main, Criterion};
use oxitext::{Pipeline, TextStyle};
use std::hint::black_box;
use std::path::Path;

fn load_bench_font() -> Vec<u8> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
    if fixture.exists() {
        return std::fs::read(&fixture).expect("read fixture font");
    }
    let candidates = [
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            return std::fs::read(p).expect("read system font");
        }
    }
    Vec::new()
}

/// Benchmark the full shape → layout → raster pipeline for a short Latin string.
fn bench_pipeline_hello_world(c: &mut Criterion) {
    let font_bytes = load_bench_font();
    if font_bytes.is_empty() {
        // No font available — register a no-op bench so the harness still runs.
        c.bench_function("pipeline_hello_world", |b| {
            b.iter(|| black_box(()));
        });
        return;
    }
    let mut pipeline = match Pipeline::from_bytes(&font_bytes) {
        Ok(p) => p,
        Err(_) => {
            c.bench_function("pipeline_hello_world", |b| {
                b.iter(|| black_box(()));
            });
            return;
        }
    };
    let style = TextStyle::default();
    c.bench_function("pipeline_hello_world", |b| {
        b.iter(|| {
            let result = pipeline.render(black_box("Hello World"), black_box(&style));
            black_box(result.ok());
        })
    });
}

/// Benchmark the pipeline for a longer repeated string (~100 chars).
fn bench_pipeline_long_text(c: &mut Criterion) {
    let font_bytes = load_bench_font();
    if font_bytes.is_empty() {
        c.bench_function("pipeline_long_text", |b| {
            b.iter(|| black_box(()));
        });
        return;
    }
    let mut pipeline = match Pipeline::from_bytes(&font_bytes) {
        Ok(p) => p,
        Err(_) => {
            c.bench_function("pipeline_long_text", |b| {
                b.iter(|| black_box(()));
            });
            return;
        }
    };
    let style = TextStyle::default();
    let long_text = "The quick brown fox jumps over the lazy dog. ".repeat(3);
    c.bench_function("pipeline_long_text", |b| {
        b.iter(|| {
            let result = pipeline.render(black_box(long_text.as_str()), black_box(&style));
            black_box(result.ok());
        })
    });
}

/// Benchmark Pipeline construction (includes font loading overhead).
fn bench_pipeline_construction(c: &mut Criterion) {
    let font_bytes = load_bench_font();
    if font_bytes.is_empty() {
        c.bench_function("pipeline_construction", |b| {
            b.iter(|| black_box(()));
        });
        return;
    }
    c.bench_function("pipeline_construction", |b| {
        b.iter(|| {
            let p = Pipeline::from_bytes(&font_bytes).ok();
            black_box(p);
        })
    });
}

criterion_group!(
    benches,
    bench_pipeline_hello_world,
    bench_pipeline_long_text,
    bench_pipeline_construction,
);
criterion_main!(benches);
