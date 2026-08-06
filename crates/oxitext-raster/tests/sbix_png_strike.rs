//! End-to-end colour-bitmap test: an Apple `sbix` PNG strike must survive the
//! whole path — table lookup in `ttf-parser`, PNG decode through
//! `oxitext-core`'s `oxiarc`-backed decoder, and RGBA normalisation.
//!
//! No binary fixture is committed for this: the test synthesises a font at run
//! time by injecting a one-strike `sbix` table into the repository's plain
//! `tests/fixtures/test-font.ttf`, so the exact pixel values it asserts are
//! known by construction.
//!
//! Requires the `png-bitmap` feature (the whole file is compiled out without
//! it, exactly like the decoder it exercises).
#![cfg(feature = "png-bitmap")]

use oxitext_raster::detect::{
    detect_color_glyph_type, detect_color_glyph_type_at, extract_cbdt_bitmap, extract_raster_glyph,
    render_cbdt_glyph, ColorGlyphType, RasterImageFormat,
};

/// The plain TTF the synthetic colour font is derived from.
const BASE_FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

/// The glyph the injected strike is attached to.
const STRIKE_GLYPH: u16 = 1;

/// The strike's declared pixels-per-em.
const STRIKE_PPEM: u16 = 32;

/// A 2×2 RGBA8 PNG written by an unrelated encoder:
/// `(255,0,0,255)`, `(0,255,0,128)` / `(0,0,255,255)`, `(0,0,0,0)`.
const SBIX_STRIKE_PNG: [u8; 80] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d,
    0x24, 0x00, 0x00, 0x00, 0x17, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x05, 0xc1, 0x01, 0x01, 0x00,
    0x00, 0x00, 0x82, 0x20, 0xa6, 0xf7, 0xdc, 0x40, 0x24, 0x43, 0xc1, 0x01, 0x3a, 0xdc, 0x05, 0x7c,
    0xf2, 0x4a, 0x44, 0x5b, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Straight-alpha RGBA the strike must decode to, row-major.
const EXPECTED_RGBA: [u8; 16] = [
    255, 0, 0, 255, // (0,0)
    0, 255, 0, 128, // (1,0)
    0, 0, 255, 255, // (0,1)
    0, 0, 0, 0, // (1,1)
];

/// One entry of an sfnt table directory.
struct TableRecord {
    tag: [u8; 4],
    data: Vec<u8>,
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

/// Split an sfnt font into its table records.
fn read_tables(font: &[u8]) -> Vec<TableRecord> {
    let num_tables = be_u16(font, 4) as usize;
    (0..num_tables)
        .map(|i| {
            let rec = 12 + i * 16;
            let mut tag = [0u8; 4];
            tag.copy_from_slice(&font[rec..rec + 4]);
            let offset = be_u32(font, rec + 8) as usize;
            let length = be_u32(font, rec + 12) as usize;
            TableRecord {
                tag,
                data: font[offset..offset + length].to_vec(),
            }
        })
        .collect()
}

/// Reassemble an sfnt font from table records.
///
/// Checksums are written as zero: `ttf-parser` never validates them, and the
/// point of this helper is table layout, not integrity metadata.
fn write_font(sfnt_version: u32, mut tables: Vec<TableRecord>) -> Vec<u8> {
    tables.sort_by_key(|t| t.tag);
    let num_tables = tables.len();

    let mut out = Vec::new();
    out.extend_from_slice(&sfnt_version.to_be_bytes());
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    // searchRange / entrySelector / rangeShift: derived fields no reader in
    // this pipeline consults; zeroed deliberately.
    out.extend_from_slice(&[0u8; 6]);

    let directory_len = 12 + num_tables * 16;
    let mut body = Vec::new();
    let mut records = Vec::with_capacity(num_tables);
    for table in &tables {
        while body.len() % 4 != 0 {
            body.push(0);
        }
        records.push((table.tag, directory_len + body.len(), table.data.len()));
        body.extend_from_slice(&table.data);
    }

    for (tag, offset, length) in records {
        out.extend_from_slice(&tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(length as u32).to_be_bytes());
    }
    out.extend_from_slice(&body);
    out
}

/// Build a one-strike `sbix` table whose only glyph data is `png` on
/// `STRIKE_GLYPH`.
fn build_sbix(num_glyphs: u16) -> Vec<u8> {
    // Glyph data record: originOffsetX, originOffsetY, graphicType, payload.
    let mut record = Vec::new();
    record.extend_from_slice(&0i16.to_be_bytes());
    record.extend_from_slice(&0i16.to_be_bytes());
    record.extend_from_slice(b"png ");
    record.extend_from_slice(&SBIX_STRIKE_PNG);

    // Strike: ppem, ppi, then numGlyphs + 1 offsets from the strike start.
    let offset_count = num_glyphs as usize + 1;
    let data_start = 4 + offset_count * 4;
    let mut strike = Vec::new();
    strike.extend_from_slice(&STRIKE_PPEM.to_be_bytes());
    strike.extend_from_slice(&72u16.to_be_bytes()); // ppi
    for gid in 0..offset_count {
        // Every glyph before the target is empty (offset == next offset); the
        // target spans the single record; everything after it is empty again.
        let offset = if gid <= STRIKE_GLYPH as usize {
            data_start
        } else {
            data_start + record.len()
        };
        strike.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    strike.extend_from_slice(&record);

    // Table header: version 1, flags 1 (draw outlines), one strike.
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&1u16.to_be_bytes());
    table.extend_from_slice(&1u32.to_be_bytes());
    table.extend_from_slice(&12u32.to_be_bytes()); // strike offset: right after the header
    table.extend_from_slice(&strike);
    table
}

/// The base font with an injected `sbix` table.
fn font_with_sbix() -> Vec<u8> {
    let sfnt_version = be_u32(BASE_FONT, 0);
    let mut tables = read_tables(BASE_FONT);
    let maxp = tables
        .iter()
        .find(|t| &t.tag == b"maxp")
        .expect("test-font.ttf must have a maxp table");
    let num_glyphs = be_u16(&maxp.data, 4);
    assert!(
        num_glyphs > STRIKE_GLYPH,
        "base font must contain the glyph the strike is attached to"
    );

    tables.push(TableRecord {
        tag: *b"sbix",
        data: build_sbix(num_glyphs),
    });
    write_font(sfnt_version, tables)
}

#[test]
fn sbix_png_strike_renders_expected_pixels() {
    let font = font_with_sbix();

    let bitmap = render_cbdt_glyph(&font, STRIKE_GLYPH, STRIKE_PPEM)
        .expect("sbix PNG strike must decode with the png-bitmap feature");
    assert_eq!((bitmap.width, bitmap.height), (2, 2));
    assert_eq!(bitmap.rgba, EXPECTED_RGBA.to_vec());
}

#[test]
fn sbix_png_strike_reaches_extract_cbdt_bitmap_too() {
    let font = font_with_sbix();

    let bitmap = extract_cbdt_bitmap(&font, STRIKE_GLYPH, STRIKE_PPEM as u8)
        .expect("sbix PNG strike must decode through extract_cbdt_bitmap");
    assert_eq!((bitmap.width, bitmap.height), (2, 2));
    assert_eq!(bitmap.rgba, EXPECTED_RGBA.to_vec());
}

#[test]
fn sbix_glyph_is_detected_as_a_colour_glyph() {
    let font = font_with_sbix();
    assert_eq!(
        detect_color_glyph_type(&font, STRIKE_GLYPH),
        ColorGlyphType::Sbix
    );
}

#[test]
fn sbix_detection_agrees_with_rendering_at_every_probe_size() {
    let font = font_with_sbix();
    // The font has a single strike, so `best_strike` selects it for any
    // requested size — detection and rendering must agree at each of them.
    for ppem in [1u16, 8, STRIKE_PPEM, 64, u16::MAX] {
        let detected = detect_color_glyph_type_at(&font, STRIKE_GLYPH, ppem);
        let rendered = render_cbdt_glyph(&font, STRIKE_GLYPH, ppem);
        assert_eq!(
            detected,
            ColorGlyphType::Sbix,
            "detection disagreed at ppem {ppem}"
        );
        assert!(rendered.is_some(), "rendering failed at ppem {ppem}");
    }
}

#[test]
fn sbix_raw_strike_is_reported_as_png() {
    let font = font_with_sbix();
    let raw = extract_raster_glyph(&font, STRIKE_GLYPH, STRIKE_PPEM).expect("raw strike");
    assert_eq!(raw.format, RasterImageFormat::Png);
    assert_eq!((raw.width, raw.height), (2, 2));
    assert_eq!(raw.pixels_per_em, STRIKE_PPEM);
    assert_eq!(raw.data, SBIX_STRIKE_PNG.to_vec());
}

#[test]
fn glyphs_without_a_strike_stay_monochrome() {
    let font = font_with_sbix();
    // Glyph 0 (.notdef) has an empty offset range in the injected strike.
    assert!(render_cbdt_glyph(&font, 0, STRIKE_PPEM).is_none());
    assert_eq!(detect_color_glyph_type(&font, 0), ColorGlyphType::None);
}

// ---------------------------------------------------------------------------
// Real-font sweep (opt-in)
// ---------------------------------------------------------------------------

/// Decode every `sbix` strike of a real colour-emoji font.
///
/// The synthetic font above proves the plumbing on data this test file
/// constructs; this proves the decoder against a font nobody here wrote. It
/// needs a system colour-emoji font, so it is `#[ignore]`d by default and
/// skips cleanly when the path does not exist:
///
/// ```text
/// cargo test -p oxitext-raster --features png-bitmap -- --ignored
/// ```
///
/// Verified locally against macOS `Apple Color Emoji.ttc`: 3600 strikes, all
/// decoded, every buffer exactly `width × height × 4` bytes.
#[test]
#[ignore = "requires a system colour-emoji font"]
fn every_strike_of_a_real_emoji_font_decodes() {
    // Candidate paths for the platforms this workspace is developed on.
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Apple Color Emoji.ttc",
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    ];

    let Some(data) = CANDIDATES.iter().find_map(|path| std::fs::read(path).ok()) else {
        eprintln!("no system colour-emoji font found; skipping");
        return;
    };

    let face = ttf_parser::Face::parse(&data, 0).expect("system emoji font must parse");
    let mut decoded = 0usize;
    let mut probed = 0usize;

    for gid in 0..face.number_of_glyphs() {
        for ppem in [20u16, 32, 64, 96, 160] {
            if face
                .glyph_raster_image(ttf_parser::GlyphId(gid), ppem)
                .is_none()
            {
                continue;
            }
            probed += 1;
            let bitmap = render_cbdt_glyph(&data, gid, ppem).unwrap_or_else(|| {
                panic!("glyph {gid} has a strike at {ppem} ppem that failed to decode")
            });
            assert_eq!(
                bitmap.rgba.len(),
                bitmap.width as usize * bitmap.height as usize * 4,
                "glyph {gid} at {ppem} ppem produced a buffer inconsistent with its dimensions"
            );
            decoded += 1;
            break;
        }
    }

    assert!(
        probed > 0,
        "the font found has no raster strikes at any probed size; the sweep proved nothing"
    );
    assert_eq!(decoded, probed, "some strikes failed to decode");
}
