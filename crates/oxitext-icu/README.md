# oxitext-icu — ICU4X-backed Unicode services for OxiText

[![Crates.io](https://img.shields.io/crates/v/oxitext-icu.svg)](https://crates.io/crates/oxitext-icu)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxitext-icu` is the Unicode/CLDR services layer of OxiText. It provides CLDR-compliant text boundary analysis (line, word, grapheme-cluster, sentence), locale-aware collation (Unicode Collation Algorithm), case mapping, normalization (NFC/NFD/NFKC/NFKD), script detection and itemization, and locale-aware number/list/plural/date-time formatting. The CLDR line-break and word-boundary results feed directly into the layout engine in [`oxitext-layout`](../oxitext-layout).

This crate wraps the **ICU4X** family of crates (`icu_segmenter`, `icu_collator`, `icu_casemap`, `icu_normalizer`, `icu_properties`, `icu_locale_core`, `icu_decimal`, `icu_list`, `icu_plurals`, `icu_datetime`, `fixed_decimal`). ICU4X is the Pure-Rust successor to the C/C++ ICU library, so this crate is **100% Pure Rust** — no C ICU, no `libicu`. Unicode/CLDR data is compiled in via ICU4X's `compiled_data` feature, which bakes the tables into the binary at build time.

> **Binary-size note.** Compiled CLDR data adds roughly **5–15 MB** to the final binary depending on which modules are exercised (`icu_segmenter` ≈ 1–3 MB, `icu_collator` ≈ 2–5 MB, others smaller). For size-sensitive targets, use ICU4X's `icu_provider_blob` or `icu_provider_fs` to load data at runtime instead of baking it in.

## Installation

```toml
[dependencies]
oxitext-icu = "0.2.4"
```

This crate has **no default features**; every type is available as soon as the dependency is added. The optional `fonts` feature enables `LocaleFontSelector` (see the API Overview below) for locale-aware font family resolution via `oxifont-db`.

## Quick Start

```rust
use oxitext_icu::{IcuSegmenter, SegmentKind};

let seg = IcuSegmenter::new();
let breaks = seg.break_points("Hello world", SegmentKind::Word);
assert!(breaks.len() >= 2);
```

### Rich segmentation with byte offsets

```rust
use oxitext_icu::{IcuSegmenter, SegmentKind};

let seg = IcuSegmenter::new();
let segs = seg.segments("Hello world", SegmentKind::Word);
let words: Vec<&str> = segs.iter()
    .filter(|s| !s.text.trim().is_empty())
    .map(|s| s.text.as_str())
    .collect();
assert!(words.contains(&"Hello"));
assert!(words.contains(&"world"));
```

### Locale-aware collation

```rust
use oxitext_icu::{IcuCollator, CollationStrength};
use std::cmp::Ordering;

// Primary strength: accents and case are ignored.
let c = IcuCollator::with_strength("en", CollationStrength::Primary)?;
assert_eq!(c.compare("Apple", "apple"), Ordering::Equal);

// Swedish: "z" sorts before "ä".
let sv = IcuCollator::new_for_locale("sv")?;
assert_eq!(sv.compare("z", "ä"), Ordering::Less);
# Ok::<(), oxitext_icu::CollateError>(())
```

## API Overview

### Segmentation — `segment` module

| Item | Kind | Description |
|------|------|-------------|
| `IcuSegmenter` | struct | Multi-kind segmenter backed by compiled CLDR data; construction is allocation-free. |
| `IcuSegmenter::new()` / `with_locale(locale)` / `new_with_locale(id)` | fn | Construct (locale-aware variants return `Result<_, CollateError>`). |
| `break_points(text, kind)` | fn | Boundary byte offsets for the given `SegmentKind` → `Vec<usize>`. |
| `segment_strs(text, kind)` | fn | Borrowed `&str` slices between boundaries. |
| `segments(text, kind)` | fn | Rich `Vec<Segment>` (text + byte range + kind). |
| `iter_segments(text, kind)` | fn | Lazy `SegmentIter` over segments. |
| `word_boundaries(text)` | fn | Convenience UAX #29 word boundaries. |
| `line_break_opportunities(text)` | fn | UAX #14 line-break offsets (for word wrap). |
| `cjk_line_break_opportunities(text)` | fn | CJK-aware line-break offsets. |
| `needs_dictionary_segmentation(text)` | fn (assoc.) | `true` for Thai/CJK and other dictionary-segmented scripts. |
| `SegmentKind` | enum | `Line`, `Word`, `GraphemeCluster`, `Grapheme` (alias), `Sentence`. |
| `Segment` | struct | `text`, `byte_start`, `byte_end`, `kind`. |
| `SegmentIter` | struct | Iterator over `Segment` values. |
| `cldr_line_breaks(text)` *(crate root)* | fn | CLDR line-break offsets; pass straight to `oxitext-layout`'s `layout_with_break_points`. |

### Collation — `collate` module

| Item | Kind | Description |
|------|------|-------------|
| `IcuCollator` | struct | Locale-aware comparator over compiled CLDR tables (no per-comparison allocation). |
| `IcuCollator::new(id)` / `new_for_locale(id)` | fn | Construct for a BCP-47 locale. |
| `IcuCollator::with_strength(id, strength)` | fn | Construct with an explicit `CollationStrength`. |
| `compare(a, b)` | fn | Locale-aware `Ordering`. |
| `sort_key(text)` | fn | Binary sort key (`Vec<u8>`) for repeated comparisons. |
| `compare_sort_keys(a, b)` | fn (assoc.) | Compare two precomputed sort keys. |
| `CollationStrength` | enum | `Primary`, `Secondary`, `Tertiary` (default), `Quaternary`, `Identical`. |

### Case mapping — `casemap` module

| Item | Kind | Description |
|------|------|-------------|
| `CaseMapper` | struct | Locale-aware case conversion. |
| `to_uppercase(text, locale_id)` / `to_lowercase(...)` / `to_titlecase(...)` | fn | Locale-sensitive case folding (e.g. Turkish dotted/dotless `i`). |

### Normalization — `normalize` module

| Item | Kind | Description |
|------|------|-------------|
| `Normalizer` | struct | Holds all four normalizers; cheap to construct. |
| `normalize(text, form)` | fn | Normalize to the requested `NormalizationForm`. |
| `is_normalized(text, form)` | fn | Test whether text is already in a form. |
| `nfc(text)` | fn | Shortcut for NFC. |
| `NormalizationForm` | enum | `Nfc`, `Nfd`, `Nfkc`, `Nfkd`. |

### Character properties — `properties` module

| Item | Kind | Description |
|------|------|-------------|
| `CharProperties` | struct | Property-query engine over compiled UCD data. |
| `script(c)` | fn | Resolve a character's `TextScript`. |
| `is_alphabetic(c)` / `is_whitespace(c)` / `is_numeric(c)` | fn | Boolean property queries. |
| `general_category(c)` | fn | General-category lookup. |
| `itemize(text)` | fn | Split text into `ScriptRun`s sharing one script. |
| `dominant_script(text)` | fn | Most frequent script in the string. |
| `has_rtl(text)` | fn | `true` if any RTL character is present. |
| `TextScript` | enum | `Latin`, `Greek`, `Cyrillic`, `Arabic`, `Hebrew`, `Han`, `Hiragana`, `Katakana`, `Hangul`, `Thai`, `Devanagari`, `Common`, `Inherited`, `Other`; `is_rtl()`. |
| `ScriptRun` | struct | `start`, `end` (byte offsets), `script`. |

### Formatters — `number`, `list`, `plural`, `datetime` modules

| Item | Kind | Description |
|------|------|-------------|
| `IcuNumberFormatter` | struct | Locale-aware numbers: `format_int`, `format_uint`, `format`, `format_with_precision`. |
| `IcuListFormatter` | struct | Locale-aware lists: `format(&[&str])`, `format_owned(&[String])`. |
| `ListType` | enum | `And`, `Or`, `Unit`. |
| `IcuPluralRules` | struct | Plural selection: `category_for`, `ordinal_category_for`, `select`, `categories`. |
| `PluralCategory` | enum | `Zero`, `One`, `Two`, `Few`, `Many`, `Other`. |
| `IcuDateTimeFormatter` | struct | Locale-aware date/time: `format_date`, `format_time`, `format_datetime`, `locale_id`. |
| `DateLength` | enum | `Full`, `Long`, `Medium` (default), `Short`. |
| `TimeLength` | enum | `Full`, `Long`, `Medium` (default), `Short`, `None`. |

### Font selection — `font_select` module (`fonts` feature)

| Item | Kind | Description |
|------|------|-------------|
| `LocaleFontSelector` | struct | Locale-aware font family selector backed by an `oxifont-db` `FontDatabase`; `Send + Sync` once constructed. |
| `LocaleFontSelector::from_system()` | fn | Build from the system font catalog (synchronous directory scan). |
| `LocaleFontSelector::from_db(db)` | fn | Build from a pre-constructed `FontDatabase`. |
| `family_for_locale(bcp47)` | fn | Canonical family name for the best match; the generic CSS family is derived from the locale's language subtag. |
| `locale_name_for_locale(bcp47)` | fn | Localised display name (from the font's `name` table) for the best match. |
| `query_family(bcp47, generic, weight)` | fn | Full CSS Fonts Level 4 match with an explicit generic family and weight. |
| `families_for_locale(bcp47, generic, weight)` | fn | All candidate family names for a locale/generic pair, deduplicated. |
| `batch_resolve(locales)` | fn | Resolves `family_for_locale` for multiple locales at once. |
| `database()` | fn | Returns the underlying `FontDatabase`. |

### Error type

| Variant | Description |
|---------|-------------|
| `CollateError::InvalidLocale(String)` | The locale string is not a valid BCP-47 locale. |
| `CollateError::Icu(String)` | The ICU data provider returned an error (e.g. unknown tailoring). |

`IcuError` is a crate-root type alias for `CollateError`, shared across the formatter constructors.

## Cross-references

- [`oxitext-layout`](../oxitext-layout) — consumes the CLDR break offsets from `cldr_line_breaks` / `line_break_opportunities` for word wrapping.
- [`oxitext`](../oxitext) — re-exports a curated subset of this crate under `oxitext::icu` behind the `icu` feature.
- [`oxitext-core`](../oxitext-core) — shared text/style types.
- [`oxitext-shape`](../oxitext-shape) · [`oxitext-raster`](../oxitext-raster) · [`oxitext-sdf`](../oxitext-sdf) · [`oxitext-bench`](../oxitext-bench) — sibling crates in the OxiText pipeline.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
