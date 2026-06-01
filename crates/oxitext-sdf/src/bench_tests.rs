//! Benchmark-style timing tests for oxitext-sdf.
//! Uses std::time::Instant to measure and report performance.

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::{
        compute_sdf, glyph_to_sdf_tile_analytic, AtlasOptions, AtlasStats, SdfAtlas, SdfTile,
    };

    const FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

    /// Benchmark EDT computation on different grid sizes.
    ///
    /// Creates a synthetic coverage bitmap containing a filled circle,
    /// then measures how long `compute_sdf` takes per grid resolution.
    #[test]
    #[ignore]
    fn bench_edt_grid_sizes() {
        for size in [128usize, 256, 512] {
            let mut coverage = vec![0u8; size * size];
            let cx = size / 2;
            let cy = size / 2;
            let r = (size / 4) as f64;
            for row in 0..size {
                for col in 0..size {
                    let dx = col as f64 - cx as f64;
                    let dy = row as f64 - cy as f64;
                    if dx * dx + dy * dy <= r * r {
                        coverage[row * size + col] = 255;
                    }
                }
            }

            let start = Instant::now();
            let sdf = compute_sdf(&coverage, size, size, 8.0, 0)
                .expect("compute_sdf should succeed on valid input");
            let elapsed = start.elapsed();

            assert_eq!(sdf.len(), size * size);
            eprintln!(
                "[bench] EDT {}x{}: {:?} ({:.2} ms)",
                size,
                size,
                elapsed,
                elapsed.as_secs_f64() * 1000.0
            );

            assert!(
                elapsed.as_secs() < 10,
                "EDT {}x{} took too long: {:?}",
                size,
                size,
                elapsed
            );
        }
    }

    /// Benchmark end-to-end: analytic SDF generation for up to 256 glyphs,
    /// followed by atlas packing.
    ///
    /// Uses `glyph_to_sdf_tile_analytic` which goes directly from font outlines
    /// to SDF tiles without a rasterisation step.
    #[test]
    #[ignore]
    fn bench_end_to_end_256_glyphs() {
        let mut tiles: Vec<SdfTile> = Vec::new();

        let start = Instant::now();
        for gid in 1u16..=256 {
            match glyph_to_sdf_tile_analytic(FONT, gid, 16.0, 32, 4.0) {
                Ok(Some(tile)) => tiles.push(tile),
                Ok(None) => {} // empty glyph — skip
                Err(e) => eprintln!("[bench] glyph {gid} error: {e:?}"),
            }
        }
        let sdf_time = start.elapsed();

        let per_glyph_ms = if tiles.is_empty() {
            0.0
        } else {
            sdf_time.as_secs_f64() * 1000.0 / tiles.len() as f64
        };
        eprintln!(
            "[bench] Analytic SDF for {} glyphs (gids 1–256): {:?} ({:.2} ms/glyph)",
            tiles.len(),
            sdf_time,
            per_glyph_ms,
        );

        // Pack into atlas.
        let start = Instant::now();
        let opts = AtlasOptions {
            atlas_size: 512,
            padding: 1,
            ..Default::default()
        };
        let (atlas, stats) = SdfAtlas::pack_with_options(&tiles, &opts);
        let pack_time = start.elapsed();

        let _utilization: f32 = stats.utilization;
        eprintln!(
            "[bench] Atlas packing {} tiles into {}x{}: {:?} (dropped: {}, utilization: {:.1}%)",
            tiles.len(),
            atlas.width,
            atlas.height,
            pack_time,
            stats.tiles_dropped,
            stats.utilization * 100.0,
        );

        let packed_plus_dropped = stats.tiles_packed + stats.tiles_dropped;
        assert_eq!(
            packed_plus_dropped,
            tiles.len(),
            "packed + dropped should equal total tiles"
        );

        #[cfg(not(debug_assertions))]
        assert!(sdf_time.as_secs() < 60, "SDF generation took too long");
        #[cfg(not(debug_assertions))]
        assert!(pack_time.as_secs() < 5, "atlas packing took too long");

        // Suppress unused-variable warning for AtlasStats binding.
        let _: AtlasStats = stats;
    }

    /// Benchmark raw EDT (`compute_sdf`) versus the analytic pipeline
    /// (`glyph_to_sdf_tile_analytic`) to give a rough cost comparison.
    ///
    /// - The EDT leg measures repeated `compute_sdf` calls on a synthetic
    ///   circular coverage bitmap (no font I/O).
    /// - The analytic leg measures repeated `glyph_to_sdf_tile_analytic` calls
    ///   on a real glyph from the test font (includes outline parsing and NR
    ///   refinement).  These are *different* inputs, so the numbers are
    ///   informative rather than a strict apples-to-apples comparison.
    #[test]
    #[ignore]
    fn bench_raw_edt_vs_analytic_sdf() {
        const N: u32 = 20;
        const SIZE: usize = 32;
        const GLYPH_ID: u16 = 36; // some test glyph

        // Build a synthetic coverage bitmap once (filled circle).
        let mut coverage = vec![0u8; SIZE * SIZE];
        let cx = SIZE / 2;
        let cy = SIZE / 2;
        let r = (SIZE / 4) as f64;
        for row in 0..SIZE {
            for col in 0..SIZE {
                let dx = col as f64 - cx as f64;
                let dy = row as f64 - cy as f64;
                if dx * dx + dy * dy <= r * r {
                    coverage[row * SIZE + col] = 255;
                }
            }
        }

        // --- EDT leg ---
        let start = Instant::now();
        for _ in 0..N {
            let _sdf = compute_sdf(&coverage, SIZE, SIZE, 8.0, 0)
                .expect("compute_sdf should not fail on valid input");
        }
        let edt_time = start.elapsed();

        // --- Analytic leg ---
        let start = Instant::now();
        for _ in 0..N {
            let _ = glyph_to_sdf_tile_analytic(FONT, GLYPH_ID, 16.0, SIZE as u32, 4.0);
        }
        let analytic_time = start.elapsed();

        eprintln!(
            "[bench] Raw EDT {}x{} x{N}: {:?} ({:?}/call)",
            SIZE,
            SIZE,
            edt_time,
            edt_time / N,
        );
        eprintln!(
            "[bench] Analytic SDF {}px glyph {} x{N}: {:?} ({:?}/call)",
            SIZE,
            GLYPH_ID,
            analytic_time,
            analytic_time / N,
        );

        assert!(
            edt_time.as_secs() < 10,
            "EDT bench took too long: {edt_time:?}"
        );
        assert!(
            analytic_time.as_secs() < 60,
            "analytic bench took too long: {analytic_time:?}"
        );
    }
}
