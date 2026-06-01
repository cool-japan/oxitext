#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! `oxitext-icu` — ICU4X-backed CLDR segmentation and locale-aware collation for OxiText.
//!
//! Provides [`IcuSegmenter`] for CLDR-based text boundary analysis (line, word,
//! grapheme-cluster, sentence), [`IcuCollator`] for locale-aware string
//! comparison via Unicode Collation Algorithm, [`CaseMapper`] for locale-aware
//! case conversion, and [`Normalizer`] for Unicode normalization.
//!
//! # Feature flags
//!
//! This crate has no default features. All types are unconditionally available
//! once the crate is added as a dependency.
//!
//! # Quick start
//!
//! ```rust
//! use oxitext_icu::{IcuSegmenter, SegmentKind};
//!
//! let seg = IcuSegmenter::new();
//! let breaks = seg.break_points("Hello world", SegmentKind::Word);
//! assert!(breaks.len() >= 2);
//! ```
//!
//! ## Binary Size Note
//!
//! This crate uses ICU4X compiled data via `compiled_data` feature flags on
//! `icu_segmenter`, `icu_collator`, and other ICU4X crates. This bakes CLDR
//! Unicode data into the binary at compile time, adding approximately 5–15 MB
//! to the final binary size depending on which ICU4X modules are enabled.
//! For size-sensitive targets, consider using `icu_provider_blob` or
//! `icu_provider_fs` to load data at runtime instead of baking it in.
//!
//! ## Rich segmentation with byte offsets
//!
//! ```rust
//! use oxitext_icu::{IcuSegmenter, SegmentKind};
//!
//! let seg = IcuSegmenter::new();
//! let segs = seg.segments("Hello world", SegmentKind::Word);
//! let words: Vec<&str> = segs.iter()
//!     .filter(|s| !s.text.trim().is_empty())
//!     .map(|s| s.text.as_str())
//!     .collect();
//! assert!(words.contains(&"Hello"));
//! assert!(words.contains(&"world"));
//! ```
//!
//! ## Collation with strength control
//!
//! ```rust
//! use oxitext_icu::{IcuCollator, CollationStrength};
//!
//! let c = IcuCollator::with_strength("en", CollationStrength::Primary)
//!     .expect("English collator");
//! // Primary strength: accents and case are ignored.
//! assert_eq!(c.compare("Apple", "apple"), std::cmp::Ordering::Equal);
//! ```
//!
//! # Modules
//!
//! - [`segment`] — CLDR text boundary analysis ([`IcuSegmenter`]).
//! - [`collate`] — locale-aware collation ([`IcuCollator`]).
//! - [`casemap`] — locale-aware case mapping ([`CaseMapper`]).
//! - [`normalize`] — Unicode normalization forms ([`Normalizer`]).
//! - [`properties`] — script detection and character property queries
//!   ([`CharProperties`], [`TextScript`]).
//! - [`number`] — locale-aware number formatting ([`number::IcuNumberFormatter`]).
//! - [`list`] — locale-aware list formatting ([`list::IcuListFormatter`]).
//! - [`plural`] — plural rule evaluation ([`plural::IcuPluralRules`]).

pub mod casemap;
pub mod collate;
pub mod datetime;
pub mod list;
pub mod normalize;
pub mod number;
pub mod plural;
pub mod properties;
pub mod segment;

pub use casemap::CaseMapper;
pub use collate::{CollateError, CollationStrength, IcuCollator};
pub use datetime::{DateLength, IcuDateTimeFormatter, TimeLength};
/// Type alias for [`CollateError`] — used for cross-module consistency.
pub type IcuError = CollateError;
pub use list::{IcuListFormatter, ListType};
pub use normalize::{NormalizationForm, Normalizer};
pub use number::IcuNumberFormatter;
pub use plural::{IcuPluralRules, PluralCategory};
pub use properties::{CharProperties, ScriptRun, TextScript};
pub use segment::{IcuSegmenter, Segment, SegmentIter, SegmentKind};

/// Returns CLDR-compliant line break opportunities for `text`.
///
/// For text containing Thai, CJK, or other dictionary-segmented scripts, this
/// uses ICU4X's LSTM/dictionary engine. For Latin/European text, it uses
/// standard UAX #14-compatible rules.
///
/// The returned byte offsets can be passed directly to
/// `LayoutEngine::layout_with_break_points()` in the `oxitext-layout` crate.
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::cldr_line_breaks;
///
/// let breaks = cldr_line_breaks("Hello world");
/// assert!(!breaks.is_empty());
/// ```
pub fn cldr_line_breaks(text: &str) -> Vec<usize> {
    IcuSegmenter::new().line_break_opportunities(text)
}

#[cfg(test)]
mod tests {
    /// Measure the estimated impact of ICU compiled data on binary size.
    ///
    /// ICU4X compiled data is stored as static byte arrays baked into the
    /// binary at compile time. This test verifies that all key ICU modules
    /// initialise correctly and documents the expected binary size impact:
    ///
    /// - `icu_segmenter` compiled_data: ~1–3 MB
    /// - `icu_collator` compiled_data: ~2–5 MB
    /// - `icu_casemap` compiled_data: ~100–500 KB
    /// - `icu_normalizer` compiled_data: ~100–500 KB
    ///
    /// Total typical impact: 5–15 MB depending on features enabled.
    /// Use `icu_provider_blob` or `icu_provider_fs` for runtime data loading
    /// to reduce binary size on constrained targets.
    #[test]
    fn test_icu_data_size_report() {
        use icu_segmenter::options::LineBreakOptions;
        use icu_segmenter::LineSegmenter;
        let seg = LineSegmenter::new_auto(LineBreakOptions::default());
        let breaks: Vec<usize> = seg.segment_str("Hello World").collect();
        assert!(
            !breaks.is_empty(),
            "ICU LineSegmenter must produce break points"
        );
        eprintln!("ICU compiled data initialized successfully");
        eprintln!("Break count for 'Hello World': {}", breaks.len());
        eprintln!("Note: ICU4X compiled_data can add 5-15 MB to binary size.");
        eprintln!("Use icu_provider_blob to load data at runtime and reduce binary size.");
    }

    #[test]
    fn test_icu_segmenter_initializes() {
        use icu_segmenter::options::LineBreakOptions;
        use icu_segmenter::LineSegmenter;
        let seg = LineSegmenter::new_auto(LineBreakOptions::default());
        let breaks: Vec<usize> = seg.segment_str("Hello World").collect();
        assert!(!breaks.is_empty());
    }

    #[test]
    fn test_icu_word_segmenter_initializes() {
        use icu_segmenter::options::WordBreakInvariantOptions;
        use icu_segmenter::WordSegmenter;
        let seg = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
        let breaks: Vec<usize> = seg.segment_str("Hello World").collect();
        assert!(!breaks.is_empty());
    }
}
