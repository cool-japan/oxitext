//! Benchmark timing tests for glyph rasterization.
//!
//! These are regular `#[test]` functions that measure wall-clock time and print
//! `eprintln!` timing summaries (visible with `cargo nextest run -- --nocapture`
//! or `cargo test -- --nocapture`).  They assert only that each operation
//! completes within a generous time budget, so they never flap on slow CI.

#[cfg(test)]
mod tests {
    use crate::{FontdueRaster, LcdFilterKernel, RasterBackend};
    use std::path::Path;

    /// Load the project test font, falling back to system fonts and then the
    /// statically bundled Noto Sans Regular when fixtures are absent.
    fn load_test_font() -> Vec<u8> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/test-font.ttf");
        if fixture.exists() {
            return std::fs::read(&fixture).expect("read fixture font");
        }
        let candidates = [
            "/Library/Fonts/Arial Unicode.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ];
        for p in &candidates {
            if Path::new(p).exists() {
                return std::fs::read(p).expect("read system font");
            }
        }
        // Fall back to statically bundled Noto Sans Regular for deterministic,
        // system-font-independent testing.
        oxifont_bundled::NOTO_SANS_REGULAR.to_vec()
    }

    /// Benchmark rasterization of 200 unique glyphs at 16 px, 32 px, and 64 px.
    ///
    /// Checks that the entire batch finishes in under 30 seconds at every size
    /// (generous budget — on a modern laptop each run takes < 50 ms).
    #[test]
    #[ignore]
    fn bench_rasterize_1000_glyphs_multisize() {
        use std::time::Instant;

        let font_data = load_test_font();
        let raster = FontdueRaster::new();
        let n: u16 = 200;

        for &px_size in &[16.0_f32, 32.0, 64.0] {
            let start = Instant::now();
            let mut visible = 0usize;
            for gid in 0..n {
                let out = raster.rasterize(&font_data, gid, px_size);
                if out.width > 0 && out.height > 0 {
                    visible += 1;
                }
            }
            let elapsed = start.elapsed();

            eprintln!(
                "[bench] rasterize {} glyphs at {}px: {:?}  ({:?}/glyph)  visible={}",
                n,
                px_size as u32,
                elapsed,
                elapsed / u32::from(n),
                visible,
            );

            #[cfg(not(debug_assertions))]
            assert!(
                elapsed.as_secs() < 30,
                "rasterization took too long at {}px: {:?}",
                px_size as u32,
                elapsed
            );
        }
    }

    /// Compare fontdue vs ab_glyph throughput for printable-ASCII glyph IDs.
    ///
    /// Both backends must complete within 10 seconds; relative performance is
    /// printed but not asserted.
    #[cfg(feature = "ab-glyph-backend")]
    #[test]
    #[ignore]
    fn bench_fontdue_vs_abglyph_throughput() {
        use crate::backend::AbGlyphRaster;
        use std::time::Instant;

        let font_data = load_test_font();
        let glyph_ids: Vec<u16> = (32..96).collect();
        let n = glyph_ids.len() as u32;
        let px_size = 24.0_f32;

        // Fontdue
        let fontdue = FontdueRaster::new();
        let start = Instant::now();
        for &gid in &glyph_ids {
            let _ = fontdue.rasterize(&font_data, gid, px_size);
        }
        let fontdue_time = start.elapsed();

        // ab_glyph
        let abglyph = AbGlyphRaster;
        let start = Instant::now();
        for &gid in &glyph_ids {
            let _ = abglyph.rasterize(&font_data, gid, px_size);
        }
        let abglyph_time = start.elapsed();

        eprintln!(
            "[bench] fontdue  {} glyphs @ {}px: {:?}  ({:?}/glyph)",
            n,
            px_size as u32,
            fontdue_time,
            fontdue_time / n,
        );
        eprintln!(
            "[bench] ab_glyph {} glyphs @ {}px: {:?}  ({:?}/glyph)",
            n,
            px_size as u32,
            abglyph_time,
            abglyph_time / n,
        );

        #[cfg(not(debug_assertions))]
        assert!(
            fontdue_time.as_secs() < 10,
            "fontdue took too long: {:?}",
            fontdue_time
        );
        #[cfg(not(debug_assertions))]
        assert!(
            abglyph_time.as_secs() < 10,
            "ab_glyph took too long: {:?}",
            abglyph_time
        );
    }

    /// Benchmark LCD subpixel rasterization vs standard greyscale.
    ///
    /// Uses glyph IDs 36–59 (a small ASCII subset) at 16 px.  Both pipelines
    /// must complete within 10 seconds; relative timings are printed.
    #[test]
    #[ignore]
    fn bench_lcd_vs_greyscale_rasterization() {
        use crate::lcd::rasterize_lcd;
        use std::time::Instant;

        let font_data = load_test_font();
        let glyph_ids: Vec<u16> = (36..60).collect();
        let n = glyph_ids.len() as u32;
        let px_size = 16.0_f32;

        // Standard greyscale via fontdue
        let raster = FontdueRaster::new();
        let start = Instant::now();
        for &gid in &glyph_ids {
            let _ = raster.rasterize(&font_data, gid, px_size);
        }
        let grey_time = start.elapsed();

        // LCD subpixel via ab_glyph (rasterize_lcd always uses ab_glyph internally)
        let start = Instant::now();
        for &gid in &glyph_ids {
            let _ = rasterize_lcd(
                &font_data,
                gid,
                px_size,
                LcdFilterKernel::FreeType5Tap,
                false,
            );
        }
        let lcd_time = start.elapsed();

        eprintln!(
            "[bench] greyscale {} glyphs @ {}px: {:?}  ({:?}/glyph)",
            n,
            px_size as u32,
            grey_time,
            grey_time / n,
        );
        eprintln!(
            "[bench] LCD       {} glyphs @ {}px: {:?}  ({:?}/glyph)",
            n,
            px_size as u32,
            lcd_time,
            lcd_time / n,
        );

        #[cfg(not(debug_assertions))]
        assert!(
            grey_time.as_secs() < 10,
            "greyscale took too long: {:?}",
            grey_time
        );
        #[cfg(not(debug_assertions))]
        assert!(lcd_time.as_secs() < 10, "LCD took too long: {:?}", lcd_time);
    }
}
