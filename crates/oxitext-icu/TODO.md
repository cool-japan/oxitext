# oxitext-icu TODO

## Status
Version 0.2.1 (2026-07-30). ICU4X-backed Unicode/CLDR services layer for OxiText. `IcuSegmenter` wraps 4 ICU4X segmenters (line, word, grapheme-cluster, sentence) using compiled CLDR data with LSTM/dictionary models for complex scripts (Thai, Khmer, Lao, Myanmar, Japanese, CJK). `IcuCollator` wraps `icu_collator::Collator` for the Unicode Collation Algorithm with strength control and sort-key generation. Also includes Unicode normalization (`Normalizer`: NFC/NFD/NFKC/NFKD), locale-aware case mapping (`CaseMapper`), character-property queries and script itemization (`CharProperties`), and locale-aware number/list/plural/date-time formatting. Behind the optional `fonts` feature, `LocaleFontSelector` adds locale-aware font family resolution via `oxifont-db`. ~2,000 SLOC across 11 source files (`tokei`). Feature-complete for all items below; 135 tests passing (`cargo nextest run -p oxitext-icu --all-features`), 36 doctests.

## Core Implementation
- [x] Add Unicode normalization: NFC, NFD, NFKC, NFKD via `icu_normalizer` (`Normalizer`, `NormalizationForm`, `is_normalized`, `nfc` helper)
- [x] Add case mapping: to_uppercase, to_lowercase, to_titlecase with locale awareness via `icu_casemap` (~50 SLOC)
- [x] Add locale-aware number formatting via `icu_decimal` / `icu_compactdecimal` (~60 SLOC)
- [x] Add locale-aware date/time formatting via `icu_datetime` (~80 SLOC)
- [x] Add locale-aware list formatting via `icu_list` (~30 SLOC)
- [x] Add plural rules via `icu_plurals` for correct plural-form selection (~30 SLOC)
- [x] Add text direction detection: `CharProperties::has_rtl` + `TextScript::is_rtl` determine script directionality from text content
- [x] Add locale-aware word segmentation with dictionary-based Thai/Khmer/Lao/Myanmar/Japanese support (~20 SLOC, already using LSTM model -- verify dictionary fallback)
- [x] Add character property queries: is_alphabetic, is_numeric, is_whitespace, general_category via `icu_properties` (`CharProperties`)
- [x] Add script detection: `CharProperties::script`/`dominant_script` + `itemize` (script-run segmentation) via `icu_properties::Script`
- [x] Add collation key generation: `IcuCollator::sort_key(text) -> Vec<u8>` for efficient multi-key sorting (~20 SLOC)
- [x] Add collation strength control: primary/secondary/tertiary/quaternary/identical levels (~15 SLOC)
  - **Goal:** `CollationStrength` enum: Primary, Secondary, Tertiary, Quaternary, Identical. `IcuCollator::with_strength(s:CollationStrength)->Self`. Maps to `icu_collator::Strength`.
  - **Files:** `crates/oxitext-icu/src/collate.rs`, `crates/oxitext-icu/src/lib.rs`
  - **Tests:** Primary strength treats é == e; Tertiary strength treats é != e; strength builder round-trips
- [x] Add `IcuSegmenter::segments(text, kind) -> Vec<&str>` returning actual text segments, not just break points
- [x] Add bidirectional text detection: `CharProperties::has_rtl(text)` for fast RTL check

## API Improvements
- [x] Add `IcuSegmenter::with_locale(locale)` constructor for locale-specific segmentation rules
- [x] Add `IcuCollator::compare_sort_keys(a: &[u8], b: &[u8]) -> Ordering` for pre-computed key comparison
  - **Goal:** `IcuCollator::sort_key(s:&str)->Vec<u8>` and `IcuCollator::compare_sort_keys(a:&[u8],b:&[u8])->Ordering` for pre-computed key comparison (faster bulk sorting).
  - **Files:** `crates/oxitext-icu/src/collate.rs`, `crates/oxitext-icu/src/lib.rs`
  - **Tests:** sort_key("a") < sort_key("b") byte-wise; compare_sort_keys(key_a, key_b) == compare("a","b")
- [x] Add `IcuCollator::default()` using root locale collation
  - **Goal:** `impl Default for IcuCollator` using `Collator::try_new(&Default::default(), Default::default()).expect("default collator")`.
  - **Files:** `crates/oxitext-icu/src/collate.rs`
  - **Tests:** IcuCollator::default() constructs without panic; compares "a" < "b"
- [x] Return `Segment { start, end, kind }` structs from break_points instead of raw byte offsets
  - **Goal:** `Segment{text:String, byte_start:usize, byte_end:usize, kind:SegmentKind}`. `SegmentKind` enum: Word, Sentence, Grapheme, Line. `IcuSegmenter::segments(text:&str, kind:SegmentKind)->Vec<Segment>`.
  - **Files:** `crates/oxitext-icu/src/segment.rs`, `crates/oxitext-icu/src/lib.rs`
  - **Tests:** word segments of "hello world" have byte_start/end matching "hello"/"world"; segments are non-overlapping and cover the full string
- [x] Add `SegmentIterator` for lazy iteration over segments without collecting into Vec

## Testing
- [x] Test word segmentation on Thai text (requires dictionary/LSTM: "กรุงเทพมหานคร")
  - **Goal:** Test that Thai text "สวัสดีชาวโลก" segments into correct word-level pieces (Thai has no spaces; ICU CLDR data drives the breaks).
  - **Files:** `crates/oxitext-icu/src/segment.rs` (inline test)
- [x] Test word segmentation on Japanese text (dictionary-based: "東京都は日本の首都です")
  - **Goal:** Test that Japanese text "日本語テスト" segments into meaningful word units using ICU word-break rules.
  - **Files:** `crates/oxitext-icu/src/segment.rs` (inline test)
- [x] Test grapheme cluster segmentation on combining characters (e.g., "e\u{0301}" -> 1 cluster)
- [x] Test grapheme cluster segmentation on emoji with ZWJ sequences
- [x] Test sentence segmentation with abbreviations ("Dr. Smith went to Washington.")
  - **Goal:** Test that "Dr. Smith went home. He was tired." segments into 2 sentences (not 3, since "Dr." is an abbreviation).
  - **Files:** `crates/oxitext-icu/src/segment.rs` (inline test)
- [x] Test line-break segmentation on CJK text (break between any two CJK characters)
- [x] Test Swedish collation: "z" < "a" (Swedish sort order differs from English)
  - **Goal:** Test Swedish locale collation: "ä" sorts after "z" (Swedish alphabet order: a-z, å, ä, ö).
  - **Files:** `crates/oxitext-icu/src/collate.rs` (inline test)
- [x] Test German phonebook collation: "ae" == "a" (phonebook variant)
  - **Goal:** Test German phonebook collation: "ö" sorts as "oe", so "Österreich" sorts near "Oe" strings.
  - **Files:** `crates/oxitext-icu/src/collate.rs` (inline test)
- [x] Test Japanese collation: hiragana vs katakana ordering
  - **Goal:** Test that Japanese collation orders hiragana, katakana, and kanji correctly according to JIS standard.
  - **Files:** `crates/oxitext-icu/src/collate.rs` (inline test)
- [x] Test case-insensitive collation: "abc" == "ABC" at secondary strength
  - **Goal:** Test that primary-strength collation treats "Apple" == "apple" == "APPLE".
  - **Files:** `crates/oxitext-icu/src/collate.rs` (inline test)
- [x] Benchmark segmentation of 100K-character multilingual text
- [x] Test normalization (NFC/NFD/NFKC/NFKD) including round-trip on precomposed vs decomposed Unicode and compatibility folding

## Performance
- [x] Measure ICU4X compiled data size impact on binary (can be significant: 10+ MB for full CLDR)
- [x] Evaluate lazy data loading to reduce startup time
- [x] Cache segmenter results for repeated calls on the same text
- [x] Benchmark collation key generation vs. direct comparison for sorting 10K strings

## Integration
- [x] Replace unicode-linebreak in oxitext-layout with IcuSegmenter(Line) for CLDR-compliant line breaking
- [x] Provide script detection to oxitext-shape for script-aware shaping itemization (`CharProperties::itemize` ready; shape-side wiring pending)
  - **Goal:** Wire `CharProperties::itemize(text)` → group by script → shape each script run with appropriate script tag. Implemented in `crates/oxitext/src/lib.rs` (facade) when `icu` feature enabled, not in the shape crate. Avoids cross-crate dep.
  - **Files:** `crates/oxitext/src/lib.rs` (facade Wave 2)
  - **Status:** Implemented — `itemize_by_script` + `script_to_opentype_tag` helpers in facade; LTR multi-script path wired in `shape_and_layout`.
- [x] Feed normalization into oxitext-shape to ensure text is in NFC before shaping (`Normalizer::nfc` ready; shape-side wiring done)
  - **Implemented:** When the `icu` feature is enabled in `oxitext-shape`, `SwashShaper::shape_request` normalizes text to NFC via `oxitext_icu::Normalizer::new().nfc(text)` before shaping so precomposed and decomposed spellings produce identical glyph runs.
- [x] Provide locale-aware word boundaries to oxitext-layout for dictionary-based line breaking in CJK/Thai
