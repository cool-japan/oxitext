use oxitext_sdf::{
    compute_sdf, glyph_to_psdf_tile, glyph_to_sdf_tile, AtlasOptions, MultiPageAtlas,
    PackingAlgorithm, SdfAtlas, SdfTile,
};

/// Create a synthetic coverage bitmap of a filled circle.
fn circle_coverage(size: usize, cx: f32, cy: f32, r: f32) -> Vec<u8> {
    (0..size * size)
        .map(|i| {
            let x = (i % size) as f32;
            let y = (i / size) as f32;
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            if d < r {
                255u8
            } else {
                0u8
            }
        })
        .collect()
}

#[test]
fn sdf_isovalue_near_outline() {
    let size = 64;
    let cx = 32.0_f32;
    let cy = 32.0_f32;
    let r = 20.0_f32;
    let coverage = circle_coverage(size, cx, cy, r);
    let sdf = compute_sdf(&coverage, size, size, 8.0, 0).expect("compute_sdf");
    assert_eq!(sdf.len(), size * size);

    // 1. Pixels well inside the circle should be > 128.
    let center_idx = 32 * size + 32;
    assert!(
        sdf[center_idx] > 128,
        "Center should be > 128, got {}",
        sdf[center_idx]
    );

    // 2. Pixels well outside should be < 128.
    let corner_idx = 0; // top-left corner, far from circle
    assert!(
        sdf[corner_idx] < 128,
        "Corner should be < 128, got {}",
        sdf[corner_idx]
    );

    // 3. Pixels near the outline (r ± a few pixels) should be close to 128.
    let outline_x = (cx + r) as usize; // rightmost point of circle
    let outline_y = cy as usize;
    let outline_idx = outline_y * size + outline_x;
    let v = sdf[outline_idx] as i32;
    assert!(
        (v - 128).abs() < 30,
        "Outline pixel should be near 128, got {v}"
    );

    // 4. A pixel 2 units inside the outline should have a higher SDF value than
    //    a pixel 2 units outside the outline.
    let just_inside = outline_y * size + (outline_x.saturating_sub(2));
    let just_outside = outline_y * size + (outline_x + 2).min(size - 1);
    assert!(
        sdf[just_inside] > sdf[just_outside],
        "Inside ({}) should have higher SDF than outside ({})",
        sdf[just_inside],
        sdf[just_outside]
    );
}

#[test]
fn atlas_packs_tiles_without_overlap() {
    let tiles: Vec<SdfTile> = (0u16..16)
        .map(|id| SdfTile {
            glyph_id: id,
            width: 32,
            height: 32,
            data: vec![128u8; 32 * 32], // neutral SDF
            bearing_x: 0,
            bearing_y: 0,
            advance_x: 32.0,
        })
        .collect();

    let atlas = SdfAtlas::pack(&tiles);
    assert_eq!(atlas.uv_map.len(), 16);
    assert!(atlas.width > 0 && atlas.height > 0);

    // All UVs must be within [0, 1] with u_min < u_max and v_min < v_max.
    for uv in atlas.uv_map.values() {
        assert!(uv.u_min >= 0.0 && uv.u_max <= 1.0);
        assert!(uv.v_min >= 0.0 && uv.v_max <= 1.0);
        assert!(uv.u_min < uv.u_max);
        assert!(uv.v_min < uv.v_max);
    }
}

#[test]
fn glyph_to_sdf_tile_works() {
    let size = 32;
    let coverage = circle_coverage(size, 16.0, 16.0, 10.0);
    let sdf = glyph_to_sdf_tile(&coverage, size, size, 32).expect("glyph_to_sdf_tile");
    assert_eq!(sdf.len(), 32 * 32);
    // Center should be well inside the circle → > 128.
    assert!(sdf[16 * 32 + 16] > 128);
}

#[test]
fn compute_sdf_rejects_bad_dimensions() {
    // coverage has 10 bytes but width*height = 4*4 = 16
    let result = compute_sdf(&[0u8; 10], 4, 4, 8.0, 0);
    assert!(result.is_err(), "should fail with mismatched dimensions");
}

#[test]
fn empty_atlas_is_valid() {
    let atlas = SdfAtlas::pack(&[]);
    assert_eq!(atlas.uv_map.len(), 0);
    assert!(atlas.width > 0 && atlas.height > 0);
}

#[test]
fn test_skyline_non_overlapping_uvs() {
    let tiles: Vec<SdfTile> = (0..15u16)
        .map(|id| SdfTile {
            glyph_id: id,
            width: 20,
            height: 20,
            data: vec![128u8; 400],
            bearing_x: 0,
            bearing_y: 0,
            advance_x: 20.0,
        })
        .collect();
    let options = AtlasOptions {
        atlas_size: 128,
        padding: 1,
        algorithm: PackingAlgorithm::Skyline,
        ..Default::default()
    };
    let (atlas, _stats) = SdfAtlas::pack_with_options(&tiles, &options);
    let uvs: Vec<_> = atlas.uv_map.values().collect();
    for i in 0..uvs.len() {
        for j in (i + 1)..uvs.len() {
            let (a, b) = (uvs[i], uvs[j]);
            assert!(
                !(a.u_min < b.u_max && a.u_max > b.u_min && a.v_min < b.v_max && a.v_max > b.v_min),
                "UV overlap between entries {i} and {j}"
            );
        }
    }
}

#[test]
fn test_multipage_atlas_packs_all_tiles() {
    let tiles: Vec<SdfTile> = (0..40u16)
        .map(|id| SdfTile {
            glyph_id: id,
            width: 32,
            height: 32,
            data: vec![128u8; 1024],
            bearing_x: 0,
            bearing_y: 0,
            advance_x: 32.0,
        })
        .collect();
    let mp = MultiPageAtlas::pack(&tiles, 128, 1);
    let total: usize = mp.pages.iter().map(|p| p.uv_map.len()).sum();
    assert_eq!(total, 40);
    assert!(
        mp.pages.len() >= 2,
        "Should need multiple pages for 40 tiles in 128px atlas"
    );
}

#[test]
fn test_psdf_tile_non_empty() {
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../oxitext/tests/");
    if let Ok(entries) = std::fs::read_dir(&font_path) {
        for entry in entries.flatten() {
            if entry
                .path()
                .extension()
                .map(|e| e == "ttf")
                .unwrap_or(false)
            {
                if let Ok(data) = std::fs::read(entry.path()) {
                    let result = glyph_to_psdf_tile(&data, 3, 16.0, 32, 4.0);
                    match result {
                        Ok(Some(tile)) => {
                            assert_eq!(tile.data.len(), (tile.width * tile.height) as usize);
                            return;
                        }
                        Ok(None) => {}
                        Err(_) => {}
                    }
                }
            }
        }
    }
}
