//! Integration tests for the M6 facade layout/measurement APIs:
//! `measure`, `shape_and_layout`, `render_to_image`, alignment, and `has_rtl`.

use oxitext::{Pipeline, Rgba8, TextAlignment, TextStyle};
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

#[test]
fn measure_returns_positive_dimensions() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let metrics = pipeline.measure("Hello world", &style).expect("measure");
    assert!(
        metrics.total_width > 0.0,
        "measured width should be positive"
    );
    assert!(
        metrics.total_height > 0.0,
        "measured height should be positive"
    );
    assert_eq!(metrics.line_count, 1, "short text fits on one line");
}

#[test]
fn measure_matches_render_glyph_positions() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let layout = pipeline.shape_and_layout("Hello", &style).expect("layout");
    let render = pipeline.render("Hello", &style).expect("render");
    assert_eq!(layout.glyphs.len(), render.glyphs.len());
    // Positions must match between the layout-only and full render paths.
    for (a, b) in layout.glyphs.iter().zip(render.glyphs.iter()) {
        assert!((a.pos.0 - b.pos.0).abs() < 1e-3);
        assert!((a.pos.1 - b.pos.1).abs() < 1e-3);
    }
}

#[test]
fn font_metrics_extracted_from_real_font() {
    let pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    // A real TTF/OTF should yield vertical metrics with a sensible em size.
    if let Some(m) = pipeline.font_metrics() {
        assert!(m.units_per_em > 0, "units_per_em should be positive");
        assert!(m.ascender > 0, "ascender should be positive");
    }
}

#[test]
fn wrapping_produces_multiple_lines() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    // Narrow column forces wrapping of a multi-word string.
    let style = TextStyle::default().with_max_width(40.0);
    let metrics = pipeline
        .measure("alpha beta gamma delta", &style)
        .expect("measure");
    assert!(
        metrics.line_count >= 2,
        "narrow column should wrap into multiple lines"
    );
}

#[test]
fn center_alignment_shifts_glyphs_right() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let left = pipeline
        .shape_and_layout("hi", &TextStyle::default().with_max_width(400.0))
        .expect("left layout");
    let center = pipeline
        .shape_and_layout(
            "hi",
            &TextStyle::default()
                .with_max_width(400.0)
                .with_alignment(TextAlignment::Center),
        )
        .expect("center layout");
    // The first glyph of centered text starts further right than left-aligned.
    assert!(
        center.glyphs[0].pos.0 > left.glyphs[0].pos.0,
        "centered first glyph x ({}) should exceed left-aligned x ({})",
        center.glyphs[0].pos.0,
        left.glyphs[0].pos.0
    );
}

#[test]
fn render_to_image_produces_canvas() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default().with_max_width(200.0);
    let img = pipeline
        .render_to_image(
            "Hello",
            &style,
            Rgba8::new(255, 255, 255, 255),
            Rgba8::BLACK,
        )
        .expect("render_to_image");
    assert_eq!(img.width, 200, "canvas width follows max_width");
    assert!(img.height > 0);
    assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
    // Background is white; at least some pixels should be darkened by glyph ink.
    let has_ink = img
        .rgba
        .chunks_exact(4)
        .any(|px| px[0] < 250 || px[1] < 250 || px[2] < 250);
    assert!(has_ink, "rendered image should contain glyph ink");
}

#[test]
fn render_result_carries_lines_and_metrics() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let result = pipeline.render("a\nb", &style).expect("render");
    assert_eq!(result.lines.len(), 2, "newline should produce two lines");
    assert_eq!(result.metrics.line_count, 2);
}

#[test]
fn has_rtl_detects_arabic() {
    let pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    assert!(!pipeline.has_rtl("hello world"), "pure Latin is not RTL");
    // Pure Arabic resolves to an RTL base direction.
    assert!(
        pipeline.has_rtl("مرحبا"),
        "Arabic should be detected as RTL"
    );
    // Mixed LTR-base text with an embedded RTL run exercises the run-level
    // detection branch (base level is LTR here, but a run is RTL).
    assert!(
        pipeline.has_rtl("hello مرحبا"),
        "mixed LTR+RTL text should be detected via embedded RTL run"
    );
}

/// Test: PUA characters (U+E000) produce .notdef glyphs when no fallback is set.
#[test]
fn fallback_absent_yields_notdef_for_pua() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    // U+E000 is Private Use Area — virtually no standard font maps it.
    let result = pipeline
        .render("\u{E000}\u{E001}\u{E002}", &style)
        .expect("render should not fail even for missing glyphs");
    // Pipeline must return a bitmap for every glyph (possibly empty for notdef).
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "glyphs and bitmaps must have equal lengths"
    );
    // All glyphs should be .notdef (gid == 0) since no font covers PUA.
    for g in &result.glyphs {
        assert_eq!(g.gid, 0, "PUA glyph without fallback should be .notdef");
    }
}

/// Test: set_fallback_fonts with an empty Vec clears the fallback chain.
#[test]
fn set_fallback_fonts_empty_clears_chain() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    // Set a fallback chain.
    pipeline.set_fallback_fonts(vec![load_test_font()]);
    // Clear it.
    pipeline.set_fallback_fonts(vec![]);
    // Should compile and not panic — basic smoke test.
    let style = TextStyle::default();
    let result = pipeline
        .render("hello", &style)
        .expect("render after clearing fallback");
    assert!(
        !result.glyphs.is_empty(),
        "glyphs should be present after clearing fallback"
    );
}

/// Test: render_paragraph produces stitched output with correct glyph count.
#[test]
fn render_paragraph_stitches_paragraphs() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let result = pipeline
        .render_paragraph(&["Hello", "World"], &style)
        .expect("render_paragraph");
    assert!(
        result.metrics.line_count >= 2,
        "two paragraphs should produce at least 2 lines"
    );
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "glyph and bitmap arrays must have equal lengths"
    );
    // Glyphs from the second paragraph should have a greater y-position than
    // those from the first.
    if let (Some(first_g), Some(last_g)) = (result.glyphs.first(), result.glyphs.last()) {
        assert!(
            last_g.pos.1 >= first_g.pos.1,
            "last paragraph glyph y ({}) should be >= first paragraph glyph y ({})",
            last_g.pos.1,
            first_g.pos.1
        );
    }
}

/// Test: render_paragraph with an empty inner paragraph advances spacing.
#[test]
fn render_paragraph_empty_paragraph_adds_spacing() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let with_blank = pipeline
        .render_paragraph(&["Hello", "", "World"], &style)
        .expect("render_paragraph with blank");
    let without_blank = pipeline
        .render_paragraph(&["Hello", "World"], &style)
        .expect("render_paragraph without blank");
    // Blank paragraph adds extra vertical space, so the total height should be larger.
    assert!(
        with_blank.metrics.total_height > without_blank.metrics.total_height,
        "blank paragraph should add spacing"
    );
}

/// Test: with_backend accepts a custom shaper and produces non-empty output.
#[test]
fn with_backend_custom_shaper_produces_output() {
    use oxitext::{FontdueRasterizer, ShapeBackend, ShapedGlyph};

    /// A trivial no-op shaper that returns empty glyph lists.
    struct NoopShaper;
    impl ShapeBackend for NoopShaper {
        fn shape(&self, _face: &std::sync::Arc<[u8]>, _text: &str, _px: f32) -> Vec<ShapedGlyph> {
            Vec::new()
        }
    }

    let font_bytes = load_test_font();
    let rasterizer = FontdueRasterizer::new();
    let pipeline = oxitext::Pipeline::with_backend(font_bytes, Box::new(NoopShaper), rasterizer);
    assert!(
        pipeline.is_ok(),
        "with_backend should succeed with valid font data"
    );
    let mut pipeline = pipeline.expect("pipeline");
    let style = oxitext::TextStyle::default();
    let result = pipeline
        .render("hello", &style)
        .expect("render with custom shaper");
    // No-op shaper produces no glyphs.
    assert!(
        result.glyphs.is_empty(),
        "no-op shaper should yield 0 glyphs"
    );
    assert!(
        result.bitmaps.is_empty(),
        "no-op shaper should yield 0 bitmaps"
    );
}

/// Test: with_backend rejects invalid font data.
#[test]
fn with_backend_invalid_font_returns_error() {
    use oxitext::{FontdueRasterizer, ShapeBackend, ShapedGlyph};

    struct NoopShaper;
    impl ShapeBackend for NoopShaper {
        fn shape(&self, _face: &std::sync::Arc<[u8]>, _text: &str, _px: f32) -> Vec<ShapedGlyph> {
            Vec::new()
        }
    }

    let result = oxitext::Pipeline::with_backend(
        b"not a font".to_vec(),
        Box::new(NoopShaper),
        FontdueRasterizer::new(),
    );
    assert!(
        result.is_err(),
        "with_backend should fail for invalid font bytes"
    );
}

/// Test: render_styled with two runs of the same font produces combined output.
#[test]
fn render_styled_two_runs_combined() {
    use oxitext::{Decoration, TextRun};
    use std::sync::Arc as SyncArc;

    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let font_arc: SyncArc<[u8]> = SyncArc::from(load_test_font().as_slice());
    let runs = [
        TextRun {
            text: "Hello ".to_string(),
            font_data: SyncArc::clone(&font_arc),
            style: oxitext::TextStyle::default(),
            decoration: Decoration::default(),
        },
        TextRun {
            text: "World".to_string(),
            font_data: SyncArc::clone(&font_arc),
            style: oxitext::TextStyle::default(),
            decoration: Decoration::default(),
        },
    ];
    let result = pipeline.render_styled(&runs, 400.0).expect("render_styled");
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "glyph and bitmap arrays must have equal lengths"
    );
    assert!(
        !result.glyphs.is_empty(),
        "styled render should produce glyphs"
    );
}

// ── Feature 1: color glyph flag ──────────────────────────────────────────────

/// `Pipeline::renders_color_glyphs()` always returns `true` in a `pure` build.
#[test]
fn test_renders_color_glyphs_flag() {
    let pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    assert!(
        pipeline.renders_color_glyphs(),
        "renders_color_glyphs should return true in a pure-feature build"
    );
}

// ── Feature 2: shape cache reuse ─────────────────────────────────────────────

/// Calling `render` twice with the same text and style should return identical
/// glyph positions (verifying that the shape cache produces consistent output).
#[test]
fn test_shape_cache_reuse() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let text = "Hello cache";

    let r1 = pipeline.render(text, &style).expect("first render");
    let r2 = pipeline
        .render(text, &style)
        .expect("second render (cache hit)");

    assert_eq!(
        r1.glyphs.len(),
        r2.glyphs.len(),
        "glyph count should be stable"
    );
    for (a, b) in r1.glyphs.iter().zip(r2.glyphs.iter()) {
        assert!(
            (a.pos.0 - b.pos.0).abs() < 1e-3,
            "x positions should be identical across cache hits"
        );
    }
}

// ── Feature 3: batch rasterization dedup ─────────────────────────────────────

/// Rendering text with repeated characters should produce bitmaps and outputs
/// of the same length as the glyph list (deduplication is an implementation
/// detail — the output arrays are still glyph-aligned).
#[test]
fn test_batch_rasterization_dedup() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    // "aaa" has three identical glyphs — dedup rasterizes GID once, but
    // outputs must still be length 3.
    let result = pipeline.render("aaa", &style).expect("render");
    assert_eq!(
        result.glyphs.len(),
        result.bitmaps.len(),
        "bitmaps must be glyph-aligned after dedup"
    );
    assert_eq!(
        result.glyphs.len(),
        result.outputs.len(),
        "outputs must be glyph-aligned after dedup"
    );
    assert_eq!(result.glyphs.len(), 3, "three chars → three glyphs");
}

// ── png-output feature ───────────────────────────────────────────────────────

/// `RenderResult::to_png` writes a valid PNG file to disk.
///
/// Verifies that the output file:
/// - exists and is non-empty after the call.
/// - starts with the PNG magic signature (`\x89PNG`).
#[cfg(feature = "png-output")]
#[test]
fn test_render_result_to_png_produces_file() {
    use oxitext::Rgba8;

    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default().with_max_width(200.0);
    let result = pipeline.render("Hello PNG", &style).expect("render");

    let out_path = std::env::temp_dir().join("oxitext_test_to_png.png");
    // Remove any stale file from a previous run.
    let _ = std::fs::remove_file(&out_path);

    let width = 200_u32;
    let height = (result.metrics.total_height.ceil() as u32 + style.font_size.ceil() as u32).max(1);

    result
        .to_png(
            &out_path,
            width,
            height,
            Rgba8::new(255, 255, 255, 255),
            Rgba8::BLACK,
        )
        .expect("to_png should succeed");

    // File must exist and be non-empty.
    let meta = std::fs::metadata(&out_path).expect("output file must exist");
    assert!(meta.len() > 0, "output PNG must be non-empty");

    // Verify PNG magic signature: first 4 bytes must be 0x89 0x50 0x4E 0x47.
    let mut file = std::fs::File::open(&out_path).expect("open output PNG");
    let mut header = [0u8; 4];
    use std::io::Read as _;
    file.read_exact(&mut header).expect("read PNG header");
    assert_eq!(
        header,
        [0x89, 0x50, 0x4E, 0x47],
        "output file must begin with PNG magic signature"
    );

    // Cleanup.
    let _ = std::fs::remove_file(&out_path);
}

// ── benchmark / profile ───────────────────────────────────────────────────────

/// `Pipeline::benchmark` should return a sensible duration (not wildly large).
#[test]
fn test_benchmark_returns_bounded_duration() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let duration = pipeline.benchmark("Hello World", &style, 3);
    // Each iteration must be under 10 seconds on any reasonable machine.
    assert!(
        duration.as_nanos() < 10_000_000_000,
        "benchmark duration per iter should be under 10s, got {:?}",
        duration
    );
}

/// `Pipeline::benchmark` with `iterations = 0` should be clamped to 1 and not panic.
#[test]
fn test_benchmark_zero_iterations_clamps_to_one() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    // Should not panic; clamps to 1.
    let duration = pipeline.benchmark("X", &style, 0);
    assert!(
        duration.as_nanos() < 10_000_000_000,
        "benchmark with 0 iterations must clamp gracefully, got {:?}",
        duration
    );
}

/// `Pipeline::profile` returns three finite durations with shape+layout <= total.
#[test]
fn test_profile_returns_three_durations() {
    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let (shape, remainder, total) = pipeline.profile("Test", &style);
    // All durations must be under 10 seconds (sanity guard).
    assert!(
        shape.as_nanos() < 10_000_000_000,
        "shape duration must be finite, got {:?}",
        shape
    );
    assert!(
        remainder.as_nanos() < 10_000_000_000,
        "remainder duration must be finite, got {:?}",
        remainder
    );
    assert!(
        total.as_nanos() < 10_000_000_000,
        "total duration must be finite, got {:?}",
        total
    );
    // shape + remainder should be <= total (saturating_sub ensures >= 0).
    assert!(
        shape + remainder <= total + std::time::Duration::from_millis(1),
        "shape + remainder must not exceed total (plus 1ms tolerance)"
    );
}

// ── End-to-end pipeline benchmark ────────────────────────────────────────────

/// End-to-end render benchmark: shape → layout → rasterize ~100 glyphs.
///
/// Runs the full pipeline 10 times, reports the average per-iteration time,
/// and guards against catastrophic slowdowns in release builds.
#[cfg(feature = "pure")]
#[test]
#[ignore]
fn test_end_to_end_pipeline_100_glyphs() {
    // ~100-glyph English sentence covering both upper and lower case plus digits.
    let text = "The quick brown fox jumps over the lazy dog. 1234567890 ABCDEFGHIJ KLMNOPQRST";
    let font_data = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_data).expect("build pipeline");
    let style = TextStyle::default();

    // Warmup: fill caches before measuring.
    let _ = pipeline.render(text, &style);

    let n: u32 = 10;
    let start = std::time::Instant::now();
    for _ in 0..n {
        let _ = pipeline.render(text, &style);
    }
    let total = start.elapsed();
    let per_render = total / n;

    eprintln!(
        "[bench] end-to-end {} chars x{n}: {:?} avg ({:.3} ms/render)",
        text.len(),
        per_render,
        per_render.as_secs_f64() * 1_000.0
    );

    // Only assert timing in release builds; debug builds are too slow for meaningful thresholds.
    #[cfg(not(debug_assertions))]
    assert!(
        per_render.as_millis() < 50,
        "pipeline too slow in release mode: {:?}/render",
        per_render
    );
}

// ── SDF atlas pipeline ────────────────────────────────────────────────────────

/// `Pipeline::render_to_sdf_atlas` populates the atlas with glyph tiles.
#[cfg(feature = "sdf")]
#[test]
fn test_render_to_sdf_atlas_populates_atlas() {
    use oxitext::sdf::SdfAtlas;

    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let mut atlas = SdfAtlas::new(512, 512);

    let (layout, new_ids) = pipeline
        .render_to_sdf_atlas("Hello", &style, &mut atlas)
        .expect("render_to_sdf_atlas should succeed");

    // The layout should contain glyphs.
    assert!(
        !layout.glyphs.is_empty(),
        "layout must have at least one glyph"
    );
    // At least some glyph IDs should have been packed.
    assert!(
        !new_ids.is_empty(),
        "at least one glyph tile should have been packed into the atlas"
    );
    // Every packed glyph ID should be present in the atlas UV map.
    for gid in &new_ids {
        assert!(
            atlas.uv_map.contains_key(gid),
            "packed glyph id {gid} must appear in atlas.uv_map"
        );
    }
}

/// A second call with the same text should not re-pack glyphs already in the atlas.
#[cfg(feature = "sdf")]
#[test]
fn test_render_to_sdf_atlas_no_duplicate_packing() {
    use oxitext::sdf::SdfAtlas;

    let mut pipeline = Pipeline::from_bytes(&load_test_font()).expect("valid font");
    let style = TextStyle::default();
    let mut atlas = SdfAtlas::new(512, 512);

    let (_, ids1) = pipeline
        .render_to_sdf_atlas("Hello", &style, &mut atlas)
        .expect("first render_to_sdf_atlas");

    let (_, ids2) = pipeline
        .render_to_sdf_atlas("Hello", &style, &mut atlas)
        .expect("second render_to_sdf_atlas");

    // Second call should pack nothing since all glyphs are already present.
    assert!(
        ids2.is_empty(),
        "second call with identical text should not re-pack glyphs; got {:?}",
        ids2
    );
    // Total entries in the atlas should be the same as after the first call.
    assert_eq!(
        atlas.uv_map.len(),
        ids1.len(),
        "atlas size should not grow on a cache-hit call"
    );
}

// ── Decoration tests ──────────────────────────────────────────────────────────

#[cfg(feature = "pure")]
#[test]
fn test_render_with_underline_decoration_does_not_panic() {
    // Verify that the basic render() path produces empty decoration_rects
    // (decorations are only populated via layout_with_options).
    let font_data = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_data).expect("build pipeline");
    let style = TextStyle::default();
    let result = pipeline.render("Hello", &style).expect("render");
    // Basic render() path yields no decorations (decoration must come from
    // LayoutOptions which is separate from TextStyle).
    assert!(result.decoration_rects.is_empty());
}

#[cfg(feature = "pure")]
#[test]
fn test_layout_options_with_underline_decoration() {
    use oxitext::{LayoutEngine, Rgba8, TextDecoration};
    use oxitext_layout::options::LayoutOptions as LayoutOpts;

    // Verify that LayoutOptions builder accepts a TextDecoration.
    let opts = LayoutOpts::builder()
        .decoration(TextDecoration::Underline {
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            thickness: 1.0,
            offset: 2.0,
        })
        .build();
    assert!(opts.decoration.is_some(), "decoration should be set");

    // Verify that layout_with_options populates decoration_rects for shaped text.
    let font_data = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_data).expect("build pipeline");
    let style = oxitext::TextStyle::default();

    // Shape the runs via the pipeline and then lay them out manually.
    let layout = pipeline.shape_and_layout("Hi!", &style).expect("layout");
    assert!(!layout.glyphs.is_empty(), "glyphs should be non-empty");

    // Re-run with layout_with_options to get decorations.
    // We construct a ShapedRun from the positioned glyphs' metadata.
    // The simplest approach: use LayoutEngine directly with synthetic runs.
    let mut engine = LayoutEngine::new();
    let font_data_arc = layout.glyphs[0].font_data.clone();
    let shaped_run = oxitext_core::ShapedRun {
        glyphs: layout
            .glyphs
            .iter()
            .map(|g| oxitext_core::ShapedGlyph {
                gid: g.gid,
                x_advance: g.advance_x,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: g.cluster,
                is_whitespace: false,
                unsafe_to_break: false,
            })
            .collect(),
        font_data: font_data_arc,
    };
    let result = engine
        .layout_with_options("Hi!", &[shaped_run], 800.0, &opts, None, 16.0)
        .expect("layout_with_options");

    // decoration_rects should be populated for non-empty text with decoration set.
    assert!(
        !result.decorations.is_empty(),
        "decoration_rects should be non-empty"
    );
    // Each rect should have positive width.
    for rect in &result.decorations {
        assert!(rect.width > 0.0, "decoration rect width must be positive");
        assert_eq!(rect.height, 1.0, "decoration thickness should be 1.0");
    }
}

#[cfg(feature = "pure")]
#[test]
fn test_decoration_rect_is_composited() {
    use oxitext::{DecorationRect, Rgba8};

    // Verify that composite_to_rgba applies decoration_rects to the output.
    let font_data = load_test_font();
    let mut pipeline = Pipeline::from_bytes(&font_data).expect("build pipeline");
    let style = TextStyle::default();
    let mut result = pipeline.render("Hi", &style).expect("render");

    // Manually inject a decoration rect (full-width red line at row 5).
    result.decoration_rects.push(DecorationRect {
        x: 0.0,
        y: 5.0,
        width: 50.0,
        height: 2.0,
        color: Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
    });

    let canvas = result.composite_to_rgba(
        60,
        30,
        Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        Rgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
    );

    // Row 5, col 0 should be red (255, 0, 0, 255).
    let idx = (5 * 60) as usize * 4;
    assert_eq!(
        canvas.rgba[idx], 255,
        "red channel should be 255 (row 5, col 0)"
    );
    assert_eq!(canvas.rgba[idx + 1], 0, "green channel should be 0");
    assert_eq!(canvas.rgba[idx + 2], 0, "blue channel should be 0");
    assert_eq!(canvas.rgba[idx + 3], 255, "alpha channel should be 255");
}
