//! Locale-aware Unicode case mapping via ICU4X.
//!
//! Provides [`CaseMapper`] for uppercase, lowercase, and titlecase conversion
//! following Unicode full case mapping rules with locale-specific tailoring
//! (e.g. Turkish dotted-i, German ß→SS, Greek final sigma).
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::CaseMapper;
//!
//! let m = CaseMapper::new();
//! assert_eq!(m.to_uppercase("hello", "en"), "HELLO");
//! assert_eq!(m.to_lowercase("WORLD", "en"), "world");
//! // German ß uppercases to SS
//! assert_eq!(m.to_uppercase("straße", "de"), "STRASSE");
//! ```

use icu_casemap::options::TitlecaseOptions;
use icu_casemap::{CaseMapper as IcuCaseMapper, TitlecaseMapper};
use icu_locale_core::LanguageIdentifier;
use std::str::FromStr;

/// Locale-aware Unicode case mapper.
///
/// Wraps `icu_casemap::CaseMapperBorrowed` (compiled data) and
/// `icu_casemap::TitlecaseMapperBorrowed` with convenience methods that accept
/// BCP-47 locale strings and return owned `String` results.
///
/// Construction is essentially free: all data lives in static compiled tables.
pub struct CaseMapper {
    inner: icu_casemap::CaseMapperBorrowed<'static>,
    title: icu_casemap::TitlecaseMapperBorrowed<'static>,
}

/// The root (undetermined) locale language identifier.
fn root_langid() -> LanguageIdentifier {
    // "und" is always valid BCP-47 for the undetermined / root locale.
    LanguageIdentifier::from_str("und").expect("'und' is a valid BCP-47 language identifier")
}

/// Parse a BCP-47 `locale_id` string to a `LanguageIdentifier`.
///
/// Falls back silently to the root locale (`"und"`) if the string is invalid.
fn parse_langid(locale_id: &str) -> LanguageIdentifier {
    LanguageIdentifier::from_str(locale_id).unwrap_or_else(|_| root_langid())
}

impl CaseMapper {
    /// Creates a new case mapper using compiled CLDR data.
    pub fn new() -> Self {
        Self {
            inner: IcuCaseMapper::new(),
            title: TitlecaseMapper::new(),
        }
    }

    /// Converts `text` to uppercase using locale `locale_id` (BCP-47, e.g. `"en"`, `"tr"`, `"und"`).
    ///
    /// The result may be longer than the input for characters like the German
    /// ß → SS, or shorter for multi-codepoint sequences that contract.
    /// Falls back to the root locale (`"und"`) if the locale string is invalid.
    pub fn to_uppercase(&self, text: &str, locale_id: &str) -> String {
        let lang = parse_langid(locale_id);
        self.inner.uppercase_to_string(text, &lang).into_owned()
    }

    /// Converts `text` to lowercase using locale `locale_id`.
    ///
    /// Falls back to the root locale (`"und"`) if the locale string is invalid.
    pub fn to_lowercase(&self, text: &str, locale_id: &str) -> String {
        let lang = parse_langid(locale_id);
        self.inner.lowercase_to_string(text, &lang).into_owned()
    }

    /// Converts `text` to titlecase (first character uppercased, rest lowercased)
    /// using locale `locale_id`.
    ///
    /// Treats the entire string as a single segment and titlecases only its
    /// first cased character.  To titlecase each word independently, split on
    /// word boundaries (see [`crate::IcuSegmenter`]) and call this method on
    /// each segment.
    ///
    /// Falls back to the root locale (`"und"`) if the locale string is invalid.
    pub fn to_titlecase(&self, text: &str, locale_id: &str) -> String {
        let lang = parse_langid(locale_id);
        self.title
            .titlecase_segment_to_string(text, &lang, TitlecaseOptions::default())
            .into_owned()
    }
}

impl Default for CaseMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uppercase_basic_ascii() {
        let m = CaseMapper::new();
        assert_eq!(m.to_uppercase("hello", "en"), "HELLO");
    }

    #[test]
    fn lowercase_basic_ascii() {
        let m = CaseMapper::new();
        assert_eq!(m.to_lowercase("WORLD", "en"), "world");
    }

    #[test]
    fn german_sharp_s_uppercase() {
        // German ß uppercases to SS in standard Unicode case mapping.
        let m = CaseMapper::new();
        let result = m.to_uppercase("straße", "de");
        assert_eq!(result, "STRASSE", "ß should uppercase to SS");
    }

    #[test]
    fn turkish_uppercase_i() {
        // Turkish: lowercase i → uppercase İ (dotted I, U+0130).
        let m = CaseMapper::new();
        let result = m.to_uppercase("istanbul", "tr");
        assert_eq!(result, "İSTANBUL");
    }

    #[test]
    fn turkish_lowercase_capital_i() {
        // Turkish: uppercase I → lowercase ı (dotless i, U+0131).
        let m = CaseMapper::new();
        let result = m.to_lowercase("I", "tr");
        assert_eq!(result, "ı");
    }

    #[test]
    fn turkish_dotted_i_lowercase() {
        // Turkish İ (U+0130) → i.
        let m = CaseMapper::new();
        let result = m.to_lowercase("İSTANBUL", "tr");
        assert!(result.contains('i'), "Turkish İ should lowercase to i");
    }

    #[test]
    fn invalid_locale_falls_back_gracefully() {
        let m = CaseMapper::new();
        // Should not panic with an invalid locale string.
        let result = m.to_uppercase("test", "not-a-locale!!!");
        assert_eq!(result, "TEST");
    }

    #[test]
    fn unicode_greek_sigma() {
        // Greek uppercase Σ has two lowercase forms: σ (medial) and ς (final).
        let m = CaseMapper::new();
        let result = m.to_lowercase("ΣΙΓΜΑ", "el");
        assert!(!result.is_empty());
        // The first character should be a lowercase sigma.
        let first_char = result.chars().next().expect("non-empty result");
        assert!(
            first_char == 'σ' || first_char == 'ς',
            "Expected lowercase sigma, got {first_char}"
        );
    }

    #[test]
    fn titlecase_basic() {
        let m = CaseMapper::new();
        // Single-segment titlecase: only first word is titlecased.
        let result = m.to_titlecase("hello world", "en");
        assert!(
            result.starts_with('H'),
            "Expected titlecase to start with H, got: {result}"
        );
    }

    #[test]
    fn titlecase_turkish_i() {
        // Turkish: lowercase i titlecases to İ (U+0130).
        let m = CaseMapper::new();
        let result = m.to_titlecase("istanbul", "tr");
        assert_eq!(result, "İstanbul");
    }

    #[test]
    fn round_trip_lower_upper() {
        let m = CaseMapper::new();
        let original = "Hello, World!";
        let lower = m.to_lowercase(original, "en");
        let upper = m.to_uppercase(&lower, "en");
        // The uppercase of the lowercased string should equal
        // the uppercase of the original.
        let expected = m.to_uppercase(original, "en");
        assert_eq!(upper, expected);
    }

    #[test]
    fn cyrillic_case() {
        let m = CaseMapper::new();
        assert_eq!(m.to_uppercase("привет мир", "ru"), "ПРИВЕТ МИР");
        assert_eq!(m.to_lowercase("ПРИВЕТ МИР", "ru"), "привет мир");
    }
}
