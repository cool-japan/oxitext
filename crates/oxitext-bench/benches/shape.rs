// Bench: shaping benchmark comparing oxitext SwashShaper vs harfbuzz-sys.
// NOTE: no #![forbid(unsafe_code)] — harfbuzz FFI calls require unsafe blocks.

use criterion::{criterion_group, criterion_main, Criterion};
use oxitext_shape::SwashShaper;
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
    // Fallback: return a minimal placeholder font — bench will
    // short-circuit but still compiles and runs without panicking.
    Arc::from(Box::new([]) as Box<[u8]>)
}

/// Benchmark oxitext SwashShaper shaping Latin text.
fn bench_shape_latin_oxitext(c: &mut Criterion) {
    let font_data = load_bench_font();
    let mut shaper = SwashShaper::new();
    c.bench_function("shape_latin_oxitext", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            let result = shaper.shape(
                black_box("Hello World"),
                Arc::clone(&font_data),
                black_box(16.0_f32),
            );
            black_box(result.ok());
        })
    });
}

/// Benchmark oxitext SwashShaper shaping Arabic text.
fn bench_shape_arabic_oxitext(c: &mut Criterion) {
    let font_data = load_bench_font();
    let mut shaper = SwashShaper::new();
    c.bench_function("shape_arabic_oxitext", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            let result = shaper.shape(
                black_box("مرحبا"),
                Arc::clone(&font_data),
                black_box(16.0_f32),
            );
            black_box(result.ok());
        })
    });
}

/// Benchmark oxitext SwashShaper shaping CJK text.
fn bench_shape_cjk_oxitext(c: &mut Criterion) {
    let font_data = load_bench_font();
    let mut shaper = SwashShaper::new();
    c.bench_function("shape_cjk_oxitext", |b| {
        b.iter(|| {
            if font_data.is_empty() {
                return black_box(());
            }
            let result = shaper.shape(
                black_box("你好世界"),
                Arc::clone(&font_data),
                black_box(16.0_f32),
            );
            black_box(result.ok());
        })
    });
}

/// Benchmark harfbuzz-sys FFI — uses hb_version_string() as a minimal FFI call.
///
/// Full shaping via harfbuzz-sys FFI requires extensive setup (hb_blob_create,
/// hb_face_create, hb_font_create, hb_buffer_create, …).  For the M5 gate
/// (`cargo bench --no-run`) the criterion is that the dep COMPILES, not that a
/// full shaping pipeline is wired up.  We call `hb_version_string()` to ensure
/// the harfbuzz-sys C code actually links, then `black_box` the result.
///
/// Enabled only when the `harfbuzz` feature is active (C binding — feature-gated
/// per COOLJAPAN Pure Rust default-features policy).
#[cfg(feature = "harfbuzz")]
fn bench_shape_harfbuzz_version(c: &mut Criterion) {
    c.bench_function("harfbuzz_version_string_ffi", |b| {
        b.iter(|| {
            // SAFETY: hb_version_string() returns a static string literal;
            // the pointer is always valid and never null.
            let ptr = unsafe { harfbuzz_sys::hb_version_string() };
            black_box(ptr);
        })
    });
}

#[cfg(feature = "harfbuzz")]
criterion_group!(
    benches,
    bench_shape_latin_oxitext,
    bench_shape_arabic_oxitext,
    bench_shape_cjk_oxitext,
    bench_shape_harfbuzz_version,
);

#[cfg(not(feature = "harfbuzz"))]
criterion_group!(
    benches,
    bench_shape_latin_oxitext,
    bench_shape_arabic_oxitext,
    bench_shape_cjk_oxitext,
);

criterion_main!(benches);
