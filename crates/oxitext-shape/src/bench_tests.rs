//! Benchmark-style timing tests for oxitext-shape.
//! These are regular tests that measure and report performance timings
//! via eprintln! (not assertions — timings vary by machine).

#[cfg(test)]
mod tests {
    use crate::{ShapeResult, SwashShaper};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Instant;

    fn load_test_font() -> Arc<[u8]> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/test-font.ttf");
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

    fn glyph_count(r: &Result<ShapeResult, oxitext_core::OxiTextError>) -> usize {
        r.as_ref().map(|s| s.glyphs.len()).unwrap_or(0)
    }

    /// Benchmark shaping 10K characters with SwashShaper.
    /// Generates a long Latin text and measures time to shape it.
    #[test]
    #[ignore]
    fn bench_swash_10k_characters() {
        let font_bytes = load_test_font();
        let unit =
            "Hello World Testing ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz 0123456789 ";
        let repeated = unit.repeat((10_000 / unit.len()) + 1);
        let text = &repeated[..10_000.min(repeated.len())];

        let mut shaper = SwashShaper::new();

        // Warmup
        let _ = shaper.shape_full(&font_bytes, &text[..100.min(text.len())], 16.0);

        let start = Instant::now();
        let result = shaper.shape_full(&font_bytes, text, 16.0);
        let elapsed = start.elapsed();

        eprintln!(
            "[bench] swash 10K chars: {:?} ({} glyphs)",
            elapsed,
            glyph_count(&result)
        );

        assert!(
            elapsed.as_secs() < 5,
            "shaping 10K chars took too long: {:?}",
            elapsed
        );
    }

    /// Benchmark SwashShaper with LRU cache on repeated text.
    #[test]
    #[ignore]
    fn bench_swash_cached_vs_uncached() {
        let font_bytes = load_test_font();
        let text = "The quick brown fox jumps over the lazy dog. ";
        let n: u32 = 50;

        // Uncached (no cache)
        let mut uncached = SwashShaper::new();
        let start = Instant::now();
        for _ in 0..n {
            let _ = uncached.shape(text, Arc::clone(&font_bytes), 16.0);
        }
        let uncached_time = start.elapsed();

        // Cached
        let mut cached = SwashShaper::with_cache(128);
        let start = Instant::now();
        for _ in 0..n {
            let _ = cached.shape(text, Arc::clone(&font_bytes), 16.0);
        }
        let cached_time = start.elapsed();

        eprintln!(
            "[bench] swash uncached x{n}: {:?} avg {:?}",
            uncached_time,
            uncached_time / n
        );
        eprintln!(
            "[bench] swash cached x{n}: {:?} avg {:?}",
            cached_time,
            cached_time / n
        );

        assert!(uncached_time.as_secs() < 10);
        assert!(cached_time.as_secs() < 10);
    }

    /// Benchmark batch shaping vs individual shaping.
    #[test]
    #[ignore]
    fn bench_shape_batch_vs_individual() {
        let font_bytes = load_test_font();
        let segments: Vec<&str> = (0..20)
            .map(|i| {
                if i % 2 == 0 {
                    "Hello World"
                } else {
                    "Testing 1234"
                }
            })
            .collect();

        let mut shaper = SwashShaper::new();

        let start = Instant::now();
        for s in &segments {
            let _ = shaper.shape_full(&font_bytes, s, 16.0);
        }
        let individual_time = start.elapsed();

        let start = Instant::now();
        let _ = shaper.shape_batch(&font_bytes, &segments, 16.0);
        let batch_time = start.elapsed();

        eprintln!(
            "[bench] shape individual x{}: {:?}",
            segments.len(),
            individual_time
        );
        eprintln!("[bench] shape batch x{}: {:?}", segments.len(), batch_time);

        assert!(individual_time.as_secs() < 5);
        assert!(batch_time.as_secs() < 5);
    }

    /// Benchmark: Swash vs Rustybuzz on the same text.
    #[cfg(feature = "rustybuzz-backend")]
    #[test]
    #[ignore]
    fn bench_swash_vs_rustybuzz() {
        use crate::backend::{RustybuzzShaper, ShapeBackend};

        let font_bytes = load_test_font();
        let text = "The quick brown fox jumps over the lazy dog.";
        let n: u32 = 100;

        let mut swash = SwashShaper::new();
        let start = Instant::now();
        for _ in 0..n {
            let _ = swash.shape_full(&font_bytes, text, 16.0);
        }
        let swash_time = start.elapsed();

        let rb = RustybuzzShaper;
        let start = Instant::now();
        for _ in 0..n {
            let _ = rb.shape(&font_bytes, text, 16.0);
        }
        let rb_time = start.elapsed();

        eprintln!(
            "[bench] swash x{n}: {:?} ({:?}/iter)",
            swash_time,
            swash_time / n
        );
        eprintln!(
            "[bench] rustybuzz x{n}: {:?} ({:?}/iter)",
            rb_time,
            rb_time / n
        );
    }
}
