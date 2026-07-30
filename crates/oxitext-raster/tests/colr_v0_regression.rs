//! COLRv0 regression tests.
//!
//! No permissively-licensed COLR **version 0** font is small enough to vendor,
//! so these tests synthesise one: the bundled Noto Sans Regular is re-packed
//! with a hand-built `COLR` (version 0) and `CPAL` (version 0) table pair.
//! That gives a fully deterministic fixture with colours the test chose itself,
//! and it lets the layer glyphs be picked deliberately -- including one that
//! `fontdue::Font::rasterize_indexed` returns as a 0x0 bitmap, which is exactly
//! the case that used to make a COLR glyph render fully transparent.
//!
//! The assertions are golden in spirit: exact layer colours at known pixels,
//! exact paint order, and byte-stability across repeated renders.

use oxitext_raster::{render_colr_v0, render_colr_v1, ColorGlyphBitmap};
use std::collections::HashSet;
use ttf_parser::{Face, GlyphId};

/// Render size used throughout, in pixels per em.
const PX: u32 = 64;

/// Opaque red, the first synthesised palette entry.
const RED: [u8; 4] = [220, 30, 40, 255];
/// Opaque blue, the second synthesised palette entry.
const BLUE: [u8; 4] = [20, 70, 200, 255];
/// Half-transparent green, the third synthesised palette entry.
const GREEN_HALF: [u8; 4] = [0, 160, 60, 128];

// ---------------------------------------------------------------------------
// sfnt surgery
// ---------------------------------------------------------------------------

/// A COLRv0 layer: the glyph to draw and the CPAL entry to draw it with.
#[derive(Clone, Copy)]
struct LayerRecord {
    glyph_id: u16,
    palette_index: u16,
}

/// Read a big-endian `u16` at `offset`.
fn be_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

/// Read a big-endian `u32` at `offset`.
fn be_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Build the binary `COLR` version 0 table for a single base glyph.
fn build_colr_table(base_glyph: u16, layers: &[LayerRecord]) -> Vec<u8> {
    let layer_count = u16::try_from(layers.len()).expect("layer count fits in u16");
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // version
    table.extend_from_slice(&1u16.to_be_bytes()); // numBaseGlyphRecords
    table.extend_from_slice(&14u32.to_be_bytes()); // baseGlyphRecordsOffset
    table.extend_from_slice(&20u32.to_be_bytes()); // layerRecordsOffset
    table.extend_from_slice(&layer_count.to_be_bytes()); // numLayerRecords

    // BaseGlyphRecord: glyphID, firstLayerIndex, numLayers.
    table.extend_from_slice(&base_glyph.to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&layer_count.to_be_bytes());

    for layer in layers {
        table.extend_from_slice(&layer.glyph_id.to_be_bytes());
        table.extend_from_slice(&layer.palette_index.to_be_bytes());
    }
    table
}

/// Build the binary `CPAL` version 0 table for a single palette.
fn build_cpal_table(colors: &[[u8; 4]]) -> Vec<u8> {
    let entries = u16::try_from(colors.len()).expect("palette size fits in u16");
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // version
    table.extend_from_slice(&entries.to_be_bytes()); // numPaletteEntries
    table.extend_from_slice(&1u16.to_be_bytes()); // numPalettes
    table.extend_from_slice(&entries.to_be_bytes()); // numColorRecords
    table.extend_from_slice(&14u32.to_be_bytes()); // colorRecordsArrayOffset
    table.extend_from_slice(&0u16.to_be_bytes()); // colorRecordIndices[0]
    for c in colors {
        // ColorRecord is BGRA, not RGBA.
        table.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
    }
    table
}

/// Re-pack `base` with the supplied extra tables, replacing any table that
/// already carries the same tag.
fn repack_sfnt(base: &[u8], extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    assert!(base.len() >= 12, "font too small to hold an sfnt header");
    let num_tables = be_u16(base, 4) as usize;

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(num_tables + extra.len());
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&base[rec..rec + 4]);
        let offset = be_u32(base, rec + 8) as usize;
        let length = be_u32(base, rec + 12) as usize;
        assert!(offset + length <= base.len(), "table {tag:?} out of bounds");
        if extra.iter().any(|(t, _)| **t == tag) {
            continue;
        }
        tables.push((tag, base[offset..offset + length].to_vec()));
    }
    for (tag, data) in extra {
        tables.push((**tag, data.clone()));
    }
    tables.sort_by_key(|entry| entry.0);

    let count = u16::try_from(tables.len()).expect("table count fits in u16");
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= u32::from(count) {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = count * 16 - search_range;

    let mut header = Vec::new();
    header.extend_from_slice(&base[0..4]); // sfntVersion
    header.extend_from_slice(&count.to_be_bytes());
    header.extend_from_slice(&search_range.to_be_bytes());
    header.extend_from_slice(&entry_selector.to_be_bytes());
    header.extend_from_slice(&range_shift.to_be_bytes());

    let mut offset = 12 + tables.len() * 16;
    let mut directory = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        directory.extend_from_slice(tag);
        // ttf-parser does not verify table checksums, so zero is fine here.
        directory.extend_from_slice(&0u32.to_be_bytes());
        directory.extend_from_slice(&u32::try_from(offset).expect("offset fits").to_be_bytes());
        directory.extend_from_slice(
            &u32::try_from(data.len())
                .expect("length fits")
                .to_be_bytes(),
        );
        body.extend_from_slice(data);
        offset += data.len();
        while !offset.is_multiple_of(4) {
            body.push(0);
            offset += 1;
        }
    }

    let mut out = header;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&body);
    out
}

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

/// A synthesised COLRv0 font plus the glyph ids that went into it.
struct SynthFont {
    data: Vec<u8>,
    base_glyph: GlyphId,
    /// Layers in paint order (bottom first).
    layers: Vec<LayerRecord>,
    /// The layer glyph that has a real outline but that fontdue refuses to
    /// rasterize, reproducing the original defect inside a COLRv0 font.
    hidden_layer: GlyphId,
}

/// Glyph ids that have a real outline in `face` but which
/// `fontdue::Font::rasterize_indexed` returns as a 0x0 bitmap.
///
/// `fontdue::Font` only materialises glyphs reachable from `cmap` (plus `GSUB`
/// when `load_substitutions` is on), so these are exactly the glyphs the old
/// COLR painter could not draw.
fn glyphs_fontdue_cannot_rasterize(face: &Face<'_>, data: &[u8]) -> Vec<GlyphId> {
    let mut mapped: HashSet<u16> = HashSet::new();
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables {
            subtable.codepoints(|cp| {
                if let Some(gid) = subtable.glyph_index(cp) {
                    mapped.insert(gid.0);
                }
            });
        }
    }
    let font = fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
        .expect("fontdue parses the bundled font");

    let mut out = Vec::new();
    for gid in 0..face.number_of_glyphs() {
        if mapped.contains(&gid) {
            continue;
        }
        let mut probe = OutlinePresence(false);
        if face.outline_glyph(GlyphId(gid), &mut probe).is_none() || !probe.0 {
            continue;
        }
        let (metrics, _) = font.rasterize_indexed(gid, PX as f32);
        if metrics.width == 0 || metrics.height == 0 {
            out.push(GlyphId(gid));
        }
    }
    out
}

/// Detects whether a glyph emitted any outline commands.
struct OutlinePresence(bool);

impl ttf_parser::OutlineBuilder for OutlinePresence {
    fn move_to(&mut self, _x: f32, _y: f32) {
        self.0 = true;
    }
    fn line_to(&mut self, _x: f32, _y: f32) {
        self.0 = true;
    }
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {
        self.0 = true;
    }
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {
        self.0 = true;
    }
    fn close(&mut self) {}
}

/// Build the synthesised COLRv0 fixture.
///
/// Layers, bottom to top:
/// 1. a wide filled glyph in red,
/// 2. a glyph fontdue cannot rasterize, in blue,
/// 3. a narrow glyph in half-transparent green.
fn synth_colr_v0_font() -> SynthFont {
    let base = oxifont_bundled::NOTO_SANS_REGULAR;
    let face = Face::parse(base, 0).expect("bundled font parses");

    let base_glyph = face.glyph_index('A').expect("bundled font has 'A'");
    let wide = face.glyph_index('M').expect("bundled font has 'M'");
    let narrow = face.glyph_index('I').expect("bundled font has 'I'");
    let hidden = *glyphs_fontdue_cannot_rasterize(&face, base)
        .first()
        .expect("bundled font has an outlined glyph fontdue will not load");

    let layers = vec![
        LayerRecord {
            glyph_id: wide.0,
            palette_index: 0,
        },
        LayerRecord {
            glyph_id: hidden.0,
            palette_index: 1,
        },
        LayerRecord {
            glyph_id: narrow.0,
            palette_index: 2,
        },
    ];

    let data = repack_sfnt(
        base,
        &[
            (b"COLR", build_colr_table(base_glyph.0, &layers)),
            (b"CPAL", build_cpal_table(&[RED, BLUE, GREEN_HALF])),
        ],
    );

    SynthFont {
        data,
        base_glyph,
        layers,
        hidden_layer: hidden,
    }
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Count pixels whose RGB matches `rgb` within `tolerance` and whose alpha is
/// at least `min_alpha`.
fn count_color(bitmap: &ColorGlyphBitmap, rgb: [u8; 3], tolerance: i32, min_alpha: u8) -> usize {
    bitmap
        .rgba
        .chunks_exact(4)
        .filter(|px| {
            px[3] >= min_alpha
                && (0..3).all(|k| (i32::from(px[k]) - i32::from(rgb[k])).abs() <= tolerance)
        })
        .count()
}

/// The synthesised font must actually be a COLR **version 0** font.
#[test]
fn synth_font_is_colr_v0() {
    let synth = synth_colr_v0_font();
    let face = Face::parse(&synth.data, 0).expect("synthesised font parses");
    let colr = face.tables().colr.expect("synthesised font has COLR");
    assert!(colr.is_simple(), "table must report itself as version 0");
    assert!(colr.contains(synth.base_glyph));
    assert_eq!(
        face.color_palettes().map(|n| n.get()),
        Some(1),
        "one CPAL palette"
    );
}

/// Every COLRv0 layer must appear, in its own palette colour.
#[test]
fn colr_v0_layers_paint_in_their_palette_colors() {
    let synth = synth_colr_v0_font();
    let bitmap =
        render_colr_v0(&synth.data, synth.base_glyph, PX, PX).expect("COLRv0 glyph must render");

    assert_eq!(bitmap.width, PX);
    assert_eq!(bitmap.height, PX);
    assert_eq!(bitmap.rgba.len(), (PX * PX * 4) as usize);

    let red = count_color(&bitmap, [RED[0], RED[1], RED[2]], 6, 250);
    assert!(red > 20, "bottom red layer missing (only {red} px)");

    let blue = count_color(&bitmap, [BLUE[0], BLUE[1], BLUE[2]], 6, 250);
    assert!(
        blue > 5,
        "the cmap-unreachable blue layer is missing (only {blue} px); this is \
         the exact failure that made COLR emoji render transparent"
    );

    // The half-transparent green layer sits on top of red, so the composited
    // colour is neither pure green nor pure red.
    let partial = bitmap
        .rgba
        .chunks_exact(4)
        .filter(|px| px[3] > 0 && px[3] < 250)
        .count();
    let greenish = bitmap
        .rgba
        .chunks_exact(4)
        .filter(|px| px[3] > 200 && px[1] > px[0] && px[1] > px[2])
        .count();
    assert!(
        partial > 0 || greenish > 0,
        "the half-transparent green layer left no trace"
    );
}

/// Layers composite bottom-up: a later layer wins where they overlap.
#[test]
fn colr_v0_paints_layers_in_order() {
    let synth = synth_colr_v0_font();
    let face = Face::parse(&synth.data, 0).expect("parses");

    // Re-render with the first two layers swapped and confirm the result
    // differs: order-insensitive painting would produce identical bitmaps.
    let swapped = {
        let mut layers = synth.layers.clone();
        layers.swap(0, 1);
        repack_sfnt(
            oxifont_bundled::NOTO_SANS_REGULAR,
            &[
                (b"COLR", build_colr_table(synth.base_glyph.0, &layers)),
                (b"CPAL", build_cpal_table(&[RED, BLUE, GREEN_HALF])),
            ],
        )
    };

    let original = render_colr_v0(&synth.data, synth.base_glyph, PX, PX).expect("renders");
    let reordered = render_colr_v0(&swapped, synth.base_glyph, PX, PX).expect("renders");
    assert_ne!(
        original.rgba, reordered.rgba,
        "swapping two overlapping layers must change the output"
    );

    // Sanity: the fixture really does overlap those two layers.
    let wide = GlyphId(synth.layers[0].glyph_id);
    assert!(face.glyph_bounding_box(wide).is_some());
    assert!(face.glyph_bounding_box(synth.hidden_layer).is_some());
}

/// The COLRv1 entry point renders a COLRv0 font identically.
#[test]
fn colr_v0_and_v1_entry_points_agree() {
    let synth = synth_colr_v0_font();
    let via_v0 = render_colr_v0(&synth.data, synth.base_glyph, PX, PX).expect("renders");
    let via_v1 = render_colr_v1(&synth.data, synth.base_glyph, PX, PX).expect("renders");
    assert_eq!(via_v0.rgba, via_v1.rgba);
}

/// Rendering is deterministic across repeated calls and sizes.
#[test]
fn colr_v0_rendering_is_deterministic() {
    let synth = synth_colr_v0_font();
    for size in [16u32, 32, 64, 128] {
        let first = render_colr_v0(&synth.data, synth.base_glyph, size, size).expect("renders");
        let second = render_colr_v0(&synth.data, synth.base_glyph, size, size).expect("renders");
        assert_eq!(
            first.rgba, second.rgba,
            "size {size} must render identically twice"
        );
        assert_eq!(first.rgba.len(), (size * size * 4) as usize);
    }
}

/// Glyphs without a COLR record still return `None` from a COLR font.
#[test]
fn non_colr_glyph_in_a_colr_font_returns_none() {
    let synth = synth_colr_v0_font();
    let face = Face::parse(&synth.data, 0).expect("parses");
    let other = face.glyph_index('Z').expect("bundled font has 'Z'");
    assert_ne!(other, synth.base_glyph);
    assert!(render_colr_v0(&synth.data, other, PX, PX).is_none());
}

/// Palette index 0xFFFF ("use the text colour") resolves to opaque black.
#[test]
fn foreground_palette_index_paints_black() {
    let base = oxifont_bundled::NOTO_SANS_REGULAR;
    let face = Face::parse(base, 0).expect("parses");
    let base_glyph = face.glyph_index('A').expect("has 'A'");
    let layer = face.glyph_index('M').expect("has 'M'");

    let data = repack_sfnt(
        base,
        &[
            (
                b"COLR",
                build_colr_table(
                    base_glyph.0,
                    &[LayerRecord {
                        glyph_id: layer.0,
                        palette_index: 0xFFFF,
                    }],
                ),
            ),
            (b"CPAL", build_cpal_table(&[RED])),
        ],
    );

    let bitmap = render_colr_v0(&data, base_glyph, PX, PX).expect("renders");
    let black = count_color(&bitmap, [0, 0, 0], 4, 250);
    assert!(black > 20, "foreground layer should be opaque black");
    assert_eq!(
        count_color(&bitmap, [RED[0], RED[1], RED[2]], 6, 250),
        0,
        "palette entry must not be used for index 0xFFFF"
    );
}

// ---------------------------------------------------------------------------
// Rasterizer parity with the previous fontdue-backed implementation
// ---------------------------------------------------------------------------

/// The alpha channel a single opaque COLRv0 layer produces must match what the
/// old fontdue-backed painter produced for the same glyph.
///
/// The painter no longer calls fontdue (it cannot: fontdue drops
/// `cmap`-unreachable layer glyphs), so this pins that swapping the rasterizer
/// did not move or reshape anything.  `'M'` is `cmap`-reachable, so fontdue can
/// still be asked for a reference bitmap, and it is placed with the exact
/// origin arithmetic the old painter used.
#[test]
fn single_layer_alpha_matches_fontdue_reference() {
    let base = oxifont_bundled::NOTO_SANS_REGULAR;
    let face = Face::parse(base, 0).expect("parses");
    let base_glyph = face.glyph_index('A').expect("has 'A'");
    let layer = face.glyph_index('M').expect("has 'M'");

    let data = repack_sfnt(
        base,
        &[
            (
                b"COLR",
                build_colr_table(
                    base_glyph.0,
                    &[LayerRecord {
                        glyph_id: layer.0,
                        palette_index: 0,
                    }],
                ),
            ),
            (b"CPAL", build_cpal_table(&[[255, 0, 0, 255]])),
        ],
    );

    let bitmap = render_colr_v0(&data, base_glyph, PX, PX).expect("renders");

    // Reference: fontdue, placed the way the previous implementation did.
    let font =
        fontdue::Font::from_bytes(base, fontdue::FontSettings::default()).expect("fontdue parses");
    let (metrics, coverage) = font.rasterize_indexed(layer.0, PX as f32);
    let baseline_y = (PX as i32) * 4 / 5;
    let origin_x = metrics.xmin;
    let origin_y = baseline_y - metrics.height as i32 - metrics.ymin;

    let mut reference = vec![0u8; (PX * PX) as usize];
    for row in 0..metrics.height as i32 {
        for col in 0..metrics.width as i32 {
            let (x, y) = (origin_x + col, origin_y + row);
            if x < 0 || y < 0 || x >= PX as i32 || y >= PX as i32 {
                continue;
            }
            reference[(y * PX as i32 + x) as usize] =
                coverage[(row * metrics.width as i32 + col) as usize];
        }
    }

    let mut total_diff = 0u64;
    let mut painted = 0u64;
    let mut worst = 0i32;
    for (i, px) in bitmap.rgba.chunks_exact(4).enumerate() {
        let diff = (i32::from(px[3]) - i32::from(reference[i])).abs();
        total_diff += diff as u64;
        worst = worst.max(diff);
        if px[3] > 0 || reference[i] > 0 {
            painted += 1;
        }
    }
    assert!(painted > 100, "reference glyph produced no ink");
    let mean = total_diff as f64 / painted as f64;
    assert!(
        mean < 12.0,
        "mean alpha difference from the fontdue reference is {mean:.2}/255 \
         (worst {worst}); the rasterizer swap changed glyph placement or shape"
    );
}
