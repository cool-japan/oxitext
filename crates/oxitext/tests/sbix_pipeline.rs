//! End-to-end facade test for PNG colour-bitmap glyphs.
//!
//! `Pipeline::render` used to return a greyscale outline for every `sbix` and
//! `CBDT` glyph: the colour-format probe was gated on `ttf_parser::Face::
//! is_color_glyph`, which reports `COLR` coverage only, so bitmap emoji fonts
//! were classified as non-colour and never reached the strike decoder.
//!
//! The font here is synthesised at run time by injecting a one-strike `sbix`
//! table (whose payload is a PNG written by an unrelated encoder) into the
//! repository's plain `tests/fixtures/test-font.ttf`, so the colours asserted
//! below are known by construction and no binary fixture is committed.
//!
//! Requires the `color-bitmap-fonts` feature; the whole file compiles out
//! without it, since that is the feature that enables PNG strike decoding.
#![cfg(feature = "color-bitmap-fonts")]

use oxitext::{Pipeline, RenderOutput, TextStyle};

/// The plain TTF the synthetic colour font is derived from.
const BASE_FONT: &[u8] = include_bytes!("../../../tests/fixtures/test-font.ttf");

/// The character whose glyph carries the injected strike.
const STRIKE_CHAR: char = 'A';

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

fn be_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

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

/// Reassemble an sfnt font from table records (checksums zeroed: `ttf-parser`
/// never validates them).
fn write_font(sfnt_version: u32, mut tables: Vec<TableRecord>) -> Vec<u8> {
    tables.sort_by_key(|t| t.tag);
    let num_tables = tables.len();

    let mut out = Vec::new();
    out.extend_from_slice(&sfnt_version.to_be_bytes());
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&[0u8; 6]); // searchRange / entrySelector / rangeShift

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
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(length as u32).to_be_bytes());
    }
    out.extend_from_slice(&body);
    out
}

/// Build a one-strike `sbix` table whose only glyph data is `png` on
/// `strike_glyph`.
fn build_sbix(num_glyphs: u16, strike_glyph: u16) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&0i16.to_be_bytes()); // originOffsetX
    record.extend_from_slice(&0i16.to_be_bytes()); // originOffsetY
    record.extend_from_slice(b"png ");
    record.extend_from_slice(&SBIX_STRIKE_PNG);

    let offset_count = num_glyphs as usize + 1;
    let data_start = 4 + offset_count * 4;
    let mut strike = Vec::new();
    strike.extend_from_slice(&STRIKE_PPEM.to_be_bytes());
    strike.extend_from_slice(&72u16.to_be_bytes()); // ppi
    for gid in 0..offset_count {
        let offset = if gid <= strike_glyph as usize {
            data_start
        } else {
            data_start + record.len()
        };
        strike.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    strike.extend_from_slice(&record);

    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_be_bytes()); // version
    table.extend_from_slice(&1u16.to_be_bytes()); // flags
    table.extend_from_slice(&1u32.to_be_bytes()); // numStrikes
    table.extend_from_slice(&12u32.to_be_bytes()); // strike offset
    table.extend_from_slice(&strike);
    table
}

/// The base font with an `sbix` strike attached to `STRIKE_CHAR`'s glyph.
fn font_with_sbix() -> Vec<u8> {
    let face = ttf_parser::Face::parse(BASE_FONT, 0).expect("base font must parse");
    let strike_glyph = face
        .glyph_index(STRIKE_CHAR)
        .expect("base font must map the strike character")
        .0;

    let sfnt_version = be_u32(BASE_FONT, 0);
    let mut tables = read_tables(BASE_FONT);
    let maxp = tables
        .iter()
        .find(|t| &t.tag == b"maxp")
        .expect("base font must have a maxp table");
    let num_glyphs = be_u16(&maxp.data, 4);

    tables.push(TableRecord {
        tag: *b"sbix",
        data: build_sbix(num_glyphs, strike_glyph),
    });
    write_font(sfnt_version, tables)
}

#[test]
fn pipeline_renders_sbix_png_glyph_in_colour() {
    let font = font_with_sbix();
    let mut pipeline = Pipeline::from_bytes(&font).expect("pipeline from synthetic sbix font");

    let style = TextStyle {
        font_size: f32::from(STRIKE_PPEM),
        ..Default::default()
    };
    let result = pipeline
        .render(&STRIKE_CHAR.to_string(), &style)
        .expect("render");

    assert_eq!(result.outputs.len(), 1, "one glyph expected");
    match &result.outputs[0] {
        RenderOutput::Color(bitmap) => {
            assert_eq!((bitmap.width, bitmap.height), (2, 2));
            assert_eq!(bitmap.rgba, EXPECTED_RGBA.to_vec());
        }
        other => panic!("expected a colour bitmap for an sbix glyph, got {other:?}"),
    }
}

#[test]
fn pipeline_still_renders_plain_glyphs_greyscale() {
    // The same synthetic font, but a character without a strike must keep
    // going down the monochrome outline path.
    let font = font_with_sbix();
    let mut pipeline = Pipeline::from_bytes(&font).expect("pipeline from synthetic sbix font");

    let style = TextStyle {
        font_size: f32::from(STRIKE_PPEM),
        ..Default::default()
    };
    let result = pipeline.render("B", &style).expect("render");

    assert_eq!(result.outputs.len(), 1);
    assert!(
        matches!(result.outputs[0], RenderOutput::Greyscale(_)),
        "a glyph without a strike must stay greyscale, got {:?}",
        result.outputs[0]
    );
}
