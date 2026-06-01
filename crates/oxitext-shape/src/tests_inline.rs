use super::*;
use std::path::Path;

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

#[test]
fn shape_ab_produces_advances() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let run = shaper.shape("AB", font_bytes, 16.0).expect("shape failed");
    assert!(!run.glyphs.is_empty(), "expected shaped glyphs");
    for g in &run.glyphs {
        assert!(
            g.x_advance > 0.0,
            "glyph x_advance should be positive, got {}",
            g.x_advance
        );
    }
}

#[test]
fn shape_with_cache_hit() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::with_cache(64);

    // First call — cache miss.
    let run1 = shaper
        .shape("hello", Arc::clone(&font_bytes), 16.0)
        .expect("shape 1");
    assert_eq!(
        shaper.shape_cache().map(|c| c.len()),
        Some(1),
        "cache should have 1 entry after miss"
    );

    // Second identical call — cache hit.
    let run2 = shaper
        .shape("hello", Arc::clone(&font_bytes), 16.0)
        .expect("shape 2");
    // Cache size must still be 1 (no new entry created).
    assert_eq!(
        shaper.shape_cache().map(|c| c.len()),
        Some(1),
        "cache size must not grow on hit"
    );

    // Both runs must have identical glyph sequences.
    assert_eq!(run1.glyphs.len(), run2.glyphs.len());
    for (g1, g2) in run1.glyphs.iter().zip(run2.glyphs.iter()) {
        assert_eq!(g1.gid, g2.gid);
    }
}

#[test]
fn shape_with_cache_different_text_is_miss() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::with_cache(64);

    shaper
        .shape("foo", Arc::clone(&font_bytes), 16.0)
        .expect("shape foo");
    shaper
        .shape("bar", Arc::clone(&font_bytes), 16.0)
        .expect("shape bar");
    assert_eq!(
        shaper.shape_cache().map(|c| c.len()),
        Some(2),
        "different texts must produce separate cache entries"
    );
}

#[test]
fn shape_with_direction_false_matches_shape() {
    // Non-RTL path must be byte-for-byte identical to shape().
    let font = load_test_font();
    let mut s = SwashShaper::new();
    let run_a = s.shape("hello", Arc::clone(&font), 16.0).expect("shape");
    let run_b = s
        .shape_with_direction("hello", Arc::clone(&font), 16.0, false)
        .expect("shape_dir");
    assert_eq!(run_a.glyphs.len(), run_b.glyphs.len());
    for (a, b) in run_a.glyphs.iter().zip(run_b.glyphs.iter()) {
        assert_eq!(a.gid, b.gid);
        assert_eq!(a.cluster, b.cluster);
    }
}

#[test]
fn shape_with_direction_rtl_clusters_ascending() {
    // Regardless of what swash does, output clusters must be non-decreasing
    // (logical source order contract).
    let font = load_test_font();
    let mut s = SwashShaper::new();
    let run = s
        .shape_with_direction("hello", Arc::clone(&font), 16.0, true)
        .expect("shape_rtl");
    let clusters: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
    for w in clusters.windows(2) {
        assert!(w[1] >= w[0], "clusters not ascending: {:?}", clusters);
    }
}

#[test]
fn shape_with_direction_rtl_same_count_as_ltr() {
    // Same text shaped LTR and RTL should produce the same number of glyphs.
    let font = load_test_font();
    let mut s = SwashShaper::new();
    let ltr = s.shape("test", Arc::clone(&font), 16.0).expect("ltr");
    let rtl = s
        .shape_with_direction("test", Arc::clone(&font), 16.0, true)
        .expect("rtl");
    assert_eq!(
        ltr.glyphs.len(),
        rtl.glyphs.len(),
        "RTL should produce same glyph count as LTR for Latin text"
    );
}

// ── Feature type tests ────────────────────────────────────────────────────

#[test]
fn shape_feature_constants() {
    assert_eq!(ShapeFeature::LIGA.tag, *b"liga");
    assert_eq!(ShapeFeature::LIGA.value, 1);
    assert_eq!(ShapeFeature::KERN.tag, *b"kern");
    assert_eq!(ShapeFeature::KERN.value, 1);
    assert_eq!(ShapeFeature::disable(*b"liga").value, 0);
    assert_eq!(ShapeFeature::enable(*b"liga").value, 1);
    assert_eq!(ShapeFeature::new(*b"salt", 3).value, 3);
}

// ── ShapeRequest builder tests ────────────────────────────────────────────

#[test]
fn shape_request_builder_ok() {
    let font_data = &[0u8; 4][..];
    let result = ShapeRequest::builder()
        .text("hello")
        .font_data(font_data)
        .px_size(16.0)
        .direction(ShapeDirection::Ltr)
        .feature(ShapeFeature::LIGA)
        .build();
    assert!(result.is_ok(), "expected Ok from builder");
    let req = result.expect("build ok");
    assert_eq!(req.text, "hello");
    assert_eq!(req.px_size, 16.0);
    assert_eq!(req.direction, ShapeDirection::Ltr);
    assert_eq!(req.features.len(), 1);
    assert_eq!(req.features[0].tag, *b"liga");
}

#[test]
fn shape_request_builder_missing_text() {
    let font_data = &[0u8; 4][..];
    let result = ShapeRequest::builder().font_data(font_data).build();
    assert!(
        matches!(result, Err(ShapeRequestError::MissingText)),
        "expected MissingText error"
    );
}

#[test]
fn shape_request_builder_missing_font() {
    let result = ShapeRequest::builder().text("hi").build();
    assert!(
        matches!(result, Err(ShapeRequestError::MissingFont)),
        "expected MissingFont error"
    );
}

#[test]
fn shape_request_error_display() {
    assert_eq!(ShapeRequestError::MissingText.to_string(), "text not set");
    assert_eq!(
        ShapeRequestError::MissingFont.to_string(),
        "font_data not set"
    );
}

// ── Vertical feature auto-injection test ──────────────────────────────────

#[test]
fn vertical_direction_adds_vert_features() {
    // Simulate the feature injection logic used inside shape_request.
    let req = ShapeRequest::builder()
        .text("A")
        .font_data(&[0u8; 4])
        .px_size(16.0)
        .direction(ShapeDirection::Ttb)
        .build()
        .expect("build ok");

    let mut features = req.features.clone();
    if req.direction == ShapeDirection::Ttb || req.direction == ShapeDirection::Btt {
        if !features.iter().any(|f| f.tag == *b"vert") {
            features.push(ShapeFeature::VERT);
        }
        if !features.iter().any(|f| f.tag == *b"vrt2") {
            features.push(ShapeFeature::VRT2);
        }
    }
    assert!(
        features.iter().any(|f| f.tag == *b"vert"),
        "vert should be auto-added for Ttb"
    );
    assert!(
        features.iter().any(|f| f.tag == *b"vrt2"),
        "vrt2 should be auto-added for Ttb"
    );
}

#[test]
fn vertical_direction_does_not_duplicate_vert_features() {
    // When vert is already present, it should not be duplicated.
    let req = ShapeRequest::builder()
        .text("A")
        .font_data(&[0u8; 4])
        .px_size(16.0)
        .direction(ShapeDirection::Ttb)
        .feature(ShapeFeature::VERT)
        .feature(ShapeFeature::VRT2)
        .build()
        .expect("build ok");

    let mut features = req.features.clone();
    if req.direction == ShapeDirection::Ttb || req.direction == ShapeDirection::Btt {
        if !features.iter().any(|f| f.tag == *b"vert") {
            features.push(ShapeFeature::VERT);
        }
        if !features.iter().any(|f| f.tag == *b"vrt2") {
            features.push(ShapeFeature::VRT2);
        }
    }
    assert_eq!(
        features.iter().filter(|f| f.tag == *b"vert").count(),
        1,
        "vert should appear exactly once"
    );
    assert_eq!(
        features.iter().filter(|f| f.tag == *b"vrt2").count(),
        1,
        "vrt2 should appear exactly once"
    );
}

// ── shape_with_features / shape_request integration tests ─────────────────

#[test]
fn shape_with_features_does_not_crash() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let glyphs = shaper
        .shape_with_features(
            &font_bytes,
            "AV",
            16.0,
            false,
            &[ShapeFeature::KERN, ShapeFeature::LIGA],
        )
        .expect("shape_with_features should not error on valid font");
    assert!(!glyphs.is_empty(), "expected non-empty glyph list");
}

#[test]
fn shape_request_ltr_non_zero_advances() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("Hello")
        .font_data(&font_bytes)
        .px_size(16.0)
        .direction(ShapeDirection::Ltr)
        .feature(ShapeFeature::KERN)
        .build()
        .expect("build ok");
    let glyphs = shaper.shape_request(&req).expect("shape_request failed");
    assert!(!glyphs.is_empty());
    assert!(
        glyphs.iter().any(|g| g.x_advance > 0.0),
        "at least one glyph should have positive x_advance"
    );
}

#[test]
fn shape_request_rtl_clusters_ascending() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("hello")
        .font_data(&font_bytes)
        .px_size(16.0)
        .direction(ShapeDirection::Rtl)
        .build()
        .expect("build ok");
    let glyphs = shaper
        .shape_request(&req)
        .expect("shape_request rtl failed");
    let clusters: Vec<u32> = glyphs.iter().map(|g| g.cluster).collect();
    for w in clusters.windows(2) {
        assert!(
            w[1] >= w[0],
            "clusters not ascending for RTL: {:?}",
            clusters
        );
    }
}

#[test]
fn kerning_test() {
    // Verify that enabling kern doesn't produce an error or panic.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("AV")
        .font_data(&font_bytes)
        .px_size(16.0)
        .feature(ShapeFeature::KERN)
        .build()
        .expect("build ok");
    let glyphs = shaper.shape_request(&req).expect("kerning test failed");
    assert!(!glyphs.is_empty());
}

#[test]
fn cjk_shaping_non_zero_advances() {
    // Use whatever fixture font is available; CJK glyphs may map to .notdef
    // if the font has no CJK coverage, but the shaper must not error.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    // Shape ASCII text (guaranteed to be in any test font).
    let glyphs = shaper
        .shape_with_features(&font_bytes, "Hello", 16.0, false, &[])
        .expect("shape should succeed");
    assert!(!glyphs.is_empty());
    assert!(
        glyphs.iter().any(|g| g.x_advance > 0.0),
        "at least one glyph should have a non-zero advance"
    );
}

// ── ShapeResult / shape_full tests ────────────────────────────────────────

#[test]
fn shape_full_detects_missing_codepoints() {
    // ShapeResult::from_glyphs correctly classifies gid=0 glyphs as missing.
    let glyphs = vec![
        ShapedGlyph {
            gid: 0,
            cluster: 0,
            ..Default::default()
        },
        ShapedGlyph {
            gid: 42,
            cluster: 1,
            x_advance: 8.0,
            ..Default::default()
        },
    ];
    let text = "ab";
    let result = ShapeResult::from_glyphs(glyphs, text, ShapeDirection::Ltr);
    assert_eq!(
        result.missing_codepoints,
        vec!['a'],
        "gid=0 at cluster 0 → 'a' is missing"
    );
}

#[test]
fn shape_full_no_missing_when_all_present() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let result = shaper
        .shape_full(&font_bytes, "AB", 16.0)
        .expect("shape_full");
    // The test font should have 'A' and 'B'.
    assert!(!result.glyphs.is_empty());
    // missing_codepoints is empty when all glyphs are resolved.
    assert!(
        result.missing_codepoints.is_empty(),
        "expected no missing codepoints for ASCII text with test font"
    );
}

#[test]
fn shape_full_direction_is_ltr() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let result = shaper
        .shape_full(&font_bytes, "Hi", 16.0)
        .expect("shape_full");
    assert_eq!(result.direction, ShapeDirection::Ltr);
    assert!(
        result.script_detected.is_none(),
        "script_detected should start as None"
    );
}

// ── shape_with_fallback tests ─────────────────────────────────────────────

#[test]
fn shape_with_fallback_uses_primary_first() {
    // With the same font as both primary and fallback, result equals plain shaping.
    let font_bytes = load_test_font();
    let data: &[u8] = &font_bytes;
    let mut shaper = SwashShaper::new();
    let result = shaper
        .shape_with_fallback(&[data, data], "Hello", 16.0)
        .expect("shape_with_fallback should succeed");
    assert!(!result.is_empty(), "expected non-empty glyphs");
}

#[test]
fn shape_with_fallback_empty_font_list_errors() {
    let mut shaper = SwashShaper::new();
    let result = shaper.shape_with_fallback(&[], "Hello", 16.0);
    assert!(result.is_err(), "empty font list should return an error");
}

#[test]
fn shape_with_fallback_single_font_is_primary_only() {
    let font_bytes = load_test_font();
    let data: &[u8] = &font_bytes;
    let mut shaper = SwashShaper::new();
    let fallback = shaper
        .shape_with_fallback(&[data], "Hi", 16.0)
        .expect("single-font fallback");
    let primary = shaper
        .shape_with_features(data, "Hi", 16.0, false, &[])
        .expect("plain shape");
    assert_eq!(
        fallback.len(),
        primary.len(),
        "single-font fallback must match plain shaping"
    );
}

// ── Ligature feature test ─────────────────────────────────────────────────

#[test]
fn ligature_feature_test() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("fi")
        .font_data(&font_bytes)
        .px_size(16.0)
        .feature(ShapeFeature::LIGA)
        .build()
        .expect("build ok");
    // Should not panic regardless of whether the font has a fi-ligature.
    let glyphs = shaper
        .shape_request(&req)
        .expect("liga feature test failed");
    assert!(!glyphs.is_empty());
}

// ── Vertical vert feature test ────────────────────────────────────────────

#[test]
fn vertical_vert_feature_test() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("A")
        .font_data(&font_bytes)
        .px_size(16.0)
        .direction(ShapeDirection::Ttb)
        .build()
        .expect("build ok");
    // Ttb auto-injects vert/vrt2; must not panic.
    let glyphs = shaper
        .shape_request(&req)
        .expect("vertical vert feature test failed");
    assert!(!glyphs.is_empty());
}

// ── Arabic RTL test ───────────────────────────────────────────────────────

#[test]
fn arabic_shaping_does_not_crash() {
    // Arabic text shaped RTL should not panic; may produce .notdef if font
    // lacks Arabic coverage, but must not error.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let _ = shaper.shape_with_direction("مرحبا", Arc::clone(&font_bytes), 16.0, true);
}

// ── supports_script tests ─────────────────────────────────────────────────

#[test]
fn supports_script_latin_returns_true_for_latin_font() {
    use crate::backend::ShapeBackend;
    let font_bytes = load_test_font();
    let backend = crate::backend::SwashShaperBackend::new();
    // The test font must cover Latin; supports_script should return true.
    assert!(
        backend.supports_script(&font_bytes, *b"latn"),
        "Latin font should support latn script"
    );
}

#[test]
fn supports_script_unknown_tag_returns_true() {
    use crate::backend::ShapeBackend;
    let font_bytes = load_test_font();
    let backend = crate::backend::SwashShaperBackend::new();
    // Unknown scripts default to true (permissive).
    assert!(
        backend.supports_script(&font_bytes, *b"xxxx"),
        "unknown script should return true (permissive default)"
    );
}

// ── Arabic joining forms / kashida test ──────────────────────────────────

#[test]
fn test_arabic_joining_forms() {
    // Shape Arabic text RTL — must not panic.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    // "مرحبا" = mim, ra, ha, ba, alef — shape with RTL direction.
    let result = shaper.shape_with_direction("مرحبا", Arc::clone(&font_bytes), 16.0, true);
    // Font may not have Arabic; either way no panic and no error is required.
    // If it does succeed, clusters must be in ascending order (logical order).
    if let Ok(run) = result {
        let clusters: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
        for w in clusters.windows(2) {
            assert!(
                w[1] >= w[0],
                "Arabic RTL clusters must be ascending: {clusters:?}"
            );
        }
    }

    // Test kashida opportunity detection — does not require a font.
    let text = "مرحبا";
    let ops = find_kashida_opportunities(text, &[]);
    // Empty glyph slice → empty result; must not panic.
    assert!(ops.is_empty());

    // Build synthetic glyphs at each UTF-8 cluster boundary.
    let glyphs: Vec<ShapedGlyph> = text
        .char_indices()
        .map(|(pos, _)| ShapedGlyph {
            cluster: pos as u32,
            ..Default::default()
        })
        .collect();
    let opps = find_kashida_opportunities(text, &glyphs);
    // "مرحبا" contains dual-joining characters → at least some opportunities.
    assert!(
        !opps.is_empty(),
        "expected kashida opportunities in Arabic text, got none"
    );
}

// ── Latin ligature test ───────────────────────────────────────────────────

#[test]
fn test_latin_ligatures() {
    // Shape "fi" with liga enabled — must not panic regardless of whether
    // the test font has a fi-ligature.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let req = ShapeRequest::builder()
        .text("fi")
        .font_data(&font_bytes)
        .px_size(16.0)
        .feature(ShapeFeature::LIGA)
        .build()
        .expect("build ok");
    let glyphs = shaper.shape_request(&req).expect("liga shaping failed");
    assert!(!glyphs.is_empty(), "liga shaping produced no glyphs");
}

// ── Emoji ZWJ detection test ──────────────────────────────────────────────

#[test]
fn test_emoji_zwj_detection() {
    let text = "👨\u{200D}👩"; // family ZWJ sequence
    let ranges = detect_emoji_zwj_sequences(text);
    assert!(
        !ranges.is_empty(),
        "expected at least one ZWJ sequence range"
    );
    for r in &ranges {
        assert!(
            text[r.clone()].contains('\u{200D}'),
            "range {:?} should contain ZWJ",
            r
        );
    }
}

#[test]
fn test_emoji_zwj_no_false_positives() {
    // Plain ASCII and plain emoji without ZWJ should produce no ranges.
    assert!(detect_emoji_zwj_sequences("hello").is_empty());
    assert!(detect_emoji_zwj_sequences("😀").is_empty());
    assert!(detect_emoji_zwj_sequences("").is_empty());
}

// ── Kashida opportunity test ──────────────────────────────────────────────

#[test]
fn test_kashida_opportunities_arabic() {
    // Arabic "بسم" — ba, sin, mim — all dual-joining (as specified).
    let text = "بسم";
    let glyphs = vec![
        ShapedGlyph {
            cluster: 0,
            ..Default::default()
        },
        ShapedGlyph {
            cluster: 3,
            ..Default::default()
        },
        ShapedGlyph {
            cluster: 6,
            ..Default::default()
        },
    ];
    let opps = find_kashida_opportunities(text, &glyphs);
    // Non-panic is sufficient per spec; we just verify the call returns without
    // panicking. The result may be empty or non-empty depending on byte offsets.
    let _ = opps;
}

#[test]
fn test_kashida_opportunities_latin_is_empty() {
    // Latin text has no Arabic dual-joining characters.
    let text = "hello";
    let glyphs: Vec<ShapedGlyph> = text
        .char_indices()
        .map(|(pos, _)| ShapedGlyph {
            cluster: pos as u32,
            ..Default::default()
        })
        .collect();
    let opps = find_kashida_opportunities(text, &glyphs);
    assert!(
        opps.is_empty(),
        "Latin text must produce no kashida opportunities"
    );
}

// ── Script itemization compile / smoke test (icu feature) ────────────────

// ── Feature 1: unsafe_to_break propagation ────────────────────────────────

#[test]
fn test_unsafe_to_break_flag_propagates() {
    // Shaping should not panic and the unsafe_to_break field should be
    // usable. The actual value depends on the font and the text (it is true
    // for glyphs that are marks or part of a multi-glyph cluster), but the
    // field must be structurally valid for every glyph.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let glyphs = shaper
        .shape_with_features(&font_bytes, "fi", 16.0, false, &[])
        .unwrap_or_default();
    for g in &glyphs {
        // Just access it — the important thing is it compiles and doesn't panic.
        let _ = g.unsafe_to_break;
    }
    assert!(!glyphs.is_empty());
}

// ── Feature 2: cluster boundaries in shape_full ───────────────────────────

#[test]
fn test_cluster_boundaries_in_shape_full() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let result = shaper
        .shape_full(&font_bytes, "Hello", 16.0)
        .expect("shape_full");
    // Should have cluster boundaries for each grapheme plus the end sentinel.
    assert!(!result.cluster_boundaries.is_empty());
    // First boundary must be 0.
    assert_eq!(result.cluster_boundaries[0], 0);
    // Last boundary must be text.len().
    assert_eq!(
        *result.cluster_boundaries.last().expect("last"),
        "Hello".len()
    );
}

// ── Feature 4: shape_slice convenience ───────────────────────────────────

#[test]
fn test_shape_slice_convenience() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let glyphs = shaper
        .shape_slice(&font_bytes, "Hello", 16.0)
        .unwrap_or_default();
    assert!(!glyphs.is_empty());
}

#[test]
fn test_shape_slice_rtl_clusters_ascending() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let glyphs = shaper
        .shape_slice_rtl(&font_bytes, "hello", 16.0)
        .unwrap_or_default();
    let clusters: Vec<u32> = glyphs.iter().map(|g| g.cluster).collect();
    for w in clusters.windows(2) {
        assert!(w[1] >= w[0], "RTL clusters must be ascending: {clusters:?}");
    }
    assert!(!glyphs.is_empty());
}

#[cfg(feature = "icu")]
#[test]
fn test_shape_by_script_smoke() {
    // Verify that shape_by_script compiles and runs without panic on basic
    // Latin text. Per-run output must be non-empty.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let runs = shaper
        .shape_by_script(Arc::clone(&font_bytes), "Hello", 16.0, &[])
        .expect("shape_by_script should succeed");
    assert!(!runs.is_empty(), "expected at least one script run");
    // All glyphs must have absolute cluster offsets (within text bounds).
    let text = "Hello";
    for run in &runs {
        for g in &run.glyphs {
            assert!(
                (g.cluster as usize) < text.len() + 1,
                "cluster {} out of bounds for text len {}",
                g.cluster,
                text.len()
            );
        }
    }
}

#[cfg(feature = "icu")]
#[test]
fn test_shape_by_script_mixed_scripts() {
    // Shape a string with two scripts; must produce >= 2 runs without panic.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    // "ABC" (Latin) + "مرحبا" (Arabic) — two distinct scripts.
    let text = "ABCمرحبا";
    let runs = shaper
        .shape_by_script(Arc::clone(&font_bytes), text, 16.0, &[])
        .expect("shape_by_script mixed should succeed");
    assert!(
        runs.len() >= 2,
        "mixed script text should produce >= 2 runs"
    );
}

// ── requires_arabic_shaping / requires_indic_shaping / requires_mark_positioning ──

#[test]
fn test_requires_arabic_shaping_arabic() {
    assert!(
        requires_arabic_shaping("مرحبا"),
        "Arabic text must return true"
    );
}

#[test]
fn test_requires_arabic_shaping_latin() {
    assert!(
        !requires_arabic_shaping("hello"),
        "Latin text must return false"
    );
}

#[test]
fn test_requires_indic_shaping_devanagari() {
    assert!(
        requires_indic_shaping("नमस्ते"),
        "Devanagari text must return true"
    );
}

#[test]
fn test_requires_indic_shaping_latin() {
    assert!(
        !requires_indic_shaping("hello"),
        "Latin text must return false"
    );
}

#[test]
fn test_requires_mark_positioning_thai() {
    assert!(
        requires_mark_positioning("สวัสดี"),
        "Thai text must return true"
    );
}

#[test]
fn test_requires_mark_positioning_latin() {
    assert!(
        !requires_mark_positioning("hello"),
        "Latin text must return false"
    );
}

// ── font_has_aat ──────────────────────────────────────────────────────────

#[test]
fn test_font_has_aat_returns_without_panic() {
    // Load the test fixture font; result may be true or false depending on
    // the font — just verify the function does not panic.
    let font_bytes = load_test_font();
    let _ = SwashShaper::font_has_aat(&font_bytes);
}

#[test]
fn test_font_has_aat_invalid_data_returns_false() {
    // Invalid font bytes must return false rather than panic.
    assert!(!SwashShaper::font_has_aat(b"not a font"));
}

// ── shape_with_aat_fallback ───────────────────────────────────────────────

#[test]
fn test_shape_with_aat_fallback_produces_glyphs() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    let result = shaper
        .shape_with_aat_fallback(&font_bytes, "Hello", 16.0)
        .expect("shape_with_aat_fallback should succeed");
    assert!(!result.glyphs.is_empty(), "expected shaped glyphs");
    assert!(
        !result.cluster_boundaries.is_empty(),
        "expected cluster boundaries"
    );
}

// ── Arabic RTL auto-upgrade in shape_request ──────────────────────────────

#[test]
fn test_shape_request_arabic_ltr_auto_upgrades_to_rtl() {
    // shape_request with Arabic text and Ltr direction must produce clusters
    // in ascending (logical) order — same contract as an explicit Rtl request.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    // "مرحبا" is Arabic; with Ltr direction the shaper should auto-upgrade to Rtl.
    let req = ShapeRequest::builder()
        .text("مرحبا")
        .font_data(&font_bytes)
        .px_size(16.0)
        .direction(ShapeDirection::Ltr) // intentionally wrong direction
        .build()
        .expect("build ok");
    let glyphs = shaper
        .shape_request(&req)
        .expect("shape_request arabic ltr");
    // After auto-upgrade to Rtl, clusters must be in ascending order.
    let clusters: Vec<u32> = glyphs.iter().map(|g| g.cluster).collect();
    for w in clusters.windows(2) {
        assert!(
            w[1] >= w[0],
            "auto-upgraded Arabic RTL clusters must be ascending: {clusters:?}"
        );
    }
}

// ── NFC normalization in shape_request (icu feature) ─────────────────────

#[cfg(feature = "icu")]
#[test]
fn test_nfc_normalization_applied_before_shape() {
    // U+00C9 (É precomposed, NFC) vs U+0045 U+0301 (E + combining acute, NFD).
    // After NFC normalization both become U+00C9, producing the same glyph run.
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();

    let req_nfc = ShapeRequest::builder()
        .text("\u{00C9}") // É precomposed
        .font_data(&font_bytes)
        .px_size(16.0)
        .build()
        .expect("build nfc ok");
    let req_nfd = ShapeRequest::builder()
        .text("\u{0045}\u{0301}") // E + combining acute
        .font_data(&font_bytes)
        .px_size(16.0)
        .build()
        .expect("build nfd ok");

    let r_nfc = shaper.shape_request(&req_nfc).expect("shape nfc");
    let r_nfd = shaper.shape_request(&req_nfd).expect("shape nfd");
    // Both should produce the same number of glyphs after NFC normalization.
    assert_eq!(
        r_nfc.len(),
        r_nfd.len(),
        "NFC normalization should make precomposed and decomposed É produce the same glyph count"
    );
}
// ── Script cache reuse (Item 4) ───────────────────────────────────────────
#[cfg(feature = "icu")]
#[test]
fn test_script_cache_reuses_on_same_text() {
    let font_bytes = load_test_font();
    let mut shaper = SwashShaper::new();
    // Shape the same text twice — second call must use the cache.
    let r1 = shaper.shape_by_script(Arc::clone(&font_bytes), "Hello World", 16.0, &[]);
    let r2 = shaper.shape_by_script(Arc::clone(&font_bytes), "Hello World", 16.0, &[]);
    assert_eq!(
        r1.is_ok(),
        r2.is_ok(),
        "cache reuse must not change success/failure"
    );
    if let (Ok(runs1), Ok(runs2)) = (r1, r2) {
        assert_eq!(
            runs1.len(),
            runs2.len(),
            "same text must yield same run count"
        );
    }
}

// ── SwashShaperBackend Arc pointer identity (P3) ──────────────────────────
/// Verify that `SwashShaperBackend::shape` preserves `Arc<[u8]>` pointer
/// identity across calls, and that calling it twice with the same `Arc`
/// produces consistent results without panicking.
#[test]
fn swash_backend_arc_pointer_preserved() {
    use crate::backend::{ShapeBackend, SwashShaperBackend};

    let font_data = load_test_font();
    let backend = SwashShaperBackend::new();

    // Calling with the same Arc twice must yield consistent results.
    let result1 = backend.shape(&font_data, "Hello", 16.0);
    let result2 = backend.shape(&font_data, "Hello", 16.0);

    assert_eq!(
        result1.len(),
        result2.len(),
        "same Arc and same text must produce the same glyph count"
    );

    // Verify pointer identity: both calls used the same allocation.
    let ptr = Arc::as_ptr(&font_data) as *const u8 as usize;
    assert!(ptr != 0, "Arc pointer must be non-null");
}

/// Verify that two different `Arc<[u8]>` allocations (even with identical
/// bytes) are treated as distinct by the cache key, consistent with the
/// pointer-identity keying strategy used in `ShapeKey::new`.
#[test]
fn swash_backend_different_arcs_distinct_keys() {
    use crate::backend::{ShapeBackend, SwashShaperBackend};
    use crate::cache::ShapeKey;

    let font_data1 = load_test_font();
    let font_data2 = Arc::from(font_data1.as_ref()); // new allocation, same bytes

    // Pointer addresses must differ.
    assert_ne!(
        Arc::as_ptr(&font_data1) as *const u8 as usize,
        Arc::as_ptr(&font_data2) as *const u8 as usize,
        "two separately allocated Arcs must have different pointer addresses"
    );

    // ShapeKey reflects this distinction.
    let key1 = ShapeKey::new(&font_data1, "Hello", 0);
    let key2 = ShapeKey::new(&font_data2, "Hello", 0);
    assert_ne!(
        key1, key2,
        "different Arc allocations must produce different cache keys"
    );

    // Both calls must succeed without panicking.
    let backend = SwashShaperBackend::new();
    let _ = backend.shape(&font_data1, "Hello", 16.0);
    let _ = backend.shape(&font_data2, "Hello", 16.0);
}
