//! Unicode normalization (UAX #15) via `icu_normalizer`.
//!
//! Provides the four Unicode Normalization Forms:
//!
//! - **NFC** — Canonical Decomposition followed by Canonical Composition.
//! - **NFD** — Canonical Decomposition.
//! - **NFKC** — Compatibility Decomposition followed by Canonical Composition.
//! - **NFKD** — Compatibility Decomposition.
//!
//! Text should be normalized to NFC before shaping so that precomposed and
//! decomposed spellings of the same string produce identical glyph runs.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::{Normalizer, NormalizationForm};
//!
//! let n = Normalizer::new();
//! // "é" can be written as U+00E9 (NFC) or "e" + U+0301 (NFD).
//! let composed = n.normalize("e\u{0301}", NormalizationForm::Nfc);
//! assert_eq!(composed, "é");
//! assert_eq!(composed.chars().count(), 1);
//! ```

use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};

/// One of the four Unicode Normalization Forms (UAX #15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizationForm {
    /// Canonical Decomposition + Canonical Composition.
    Nfc,
    /// Canonical Decomposition.
    Nfd,
    /// Compatibility Decomposition + Canonical Composition.
    Nfkc,
    /// Compatibility Decomposition.
    Nfkd,
}

/// Multi-form Unicode normalizer backed by ICU4X compiled data.
///
/// Holds all four borrowed normalizers; construction is cheap (no allocation,
/// no I/O — the data lives in static tables compiled into the binary).
pub struct Normalizer {
    nfc: icu_normalizer::ComposingNormalizerBorrowed<'static>,
    nfkc: icu_normalizer::ComposingNormalizerBorrowed<'static>,
    nfd: icu_normalizer::DecomposingNormalizerBorrowed<'static>,
    nfkd: icu_normalizer::DecomposingNormalizerBorrowed<'static>,
}

impl Normalizer {
    /// Creates a new normalizer using compiled CLDR/UCD data.
    pub fn new() -> Self {
        Self {
            nfc: ComposingNormalizer::new_nfc(),
            nfkc: ComposingNormalizer::new_nfkc(),
            nfd: DecomposingNormalizer::new_nfd(),
            nfkd: DecomposingNormalizer::new_nfkd(),
        }
    }

    /// Normalizes `text` into the requested [`NormalizationForm`].
    ///
    /// Returns an owned `String`. When `text` is already in the requested
    /// form, ICU4X borrows internally and this still allocates only the final
    /// `String` (cheap for already-normalized input).
    pub fn normalize(&self, text: &str, form: NormalizationForm) -> String {
        match form {
            NormalizationForm::Nfc => self.nfc.normalize(text).into_owned(),
            NormalizationForm::Nfkc => self.nfkc.normalize(text).into_owned(),
            NormalizationForm::Nfd => self.nfd.normalize(text).into_owned(),
            NormalizationForm::Nfkd => self.nfkd.normalize(text).into_owned(),
        }
    }

    /// Returns `true` if `text` is already in the requested normalization form.
    pub fn is_normalized(&self, text: &str, form: NormalizationForm) -> bool {
        // A string is in form F iff normalizing it to F is a no-op.
        self.normalize(text, form) == text
    }

    /// Convenience: normalize to NFC (the recommended pre-shaping form).
    pub fn nfc(&self, text: &str) -> String {
        self.normalize(text, NormalizationForm::Nfc)
    }
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_composes_combining_accent() {
        let n = Normalizer::new();
        // "e" + combining acute accent → precomposed "é".
        let out = n.normalize("e\u{0301}", NormalizationForm::Nfc);
        assert_eq!(out, "é");
        assert_eq!(out.chars().count(), 1);
    }

    #[test]
    fn nfd_decomposes_precomposed() {
        let n = Normalizer::new();
        // Precomposed "é" → "e" + combining acute accent.
        let out = n.normalize("é", NormalizationForm::Nfd);
        assert_eq!(out.chars().count(), 2);
        assert_eq!(out, "e\u{0301}");
    }

    #[test]
    fn nfc_nfd_round_trip() {
        let n = Normalizer::new();
        let original = "Crème Brûlée";
        let decomposed = n.normalize(original, NormalizationForm::Nfd);
        let recomposed = n.normalize(&decomposed, NormalizationForm::Nfc);
        let original_nfc = n.normalize(original, NormalizationForm::Nfc);
        assert_eq!(recomposed, original_nfc);
    }

    #[test]
    fn nfkc_folds_compatibility_chars() {
        let n = Normalizer::new();
        // U+FB01 LATIN SMALL LIGATURE FI → "fi" under compatibility forms.
        let out = n.normalize("\u{FB01}", NormalizationForm::Nfkc);
        assert_eq!(out, "fi");
    }

    #[test]
    fn nfkd_decomposes_superscript() {
        let n = Normalizer::new();
        // U+00B2 SUPERSCRIPT TWO → "2" under compatibility decomposition.
        let out = n.normalize("\u{00B2}", NormalizationForm::Nfkd);
        assert_eq!(out, "2");
    }

    #[test]
    fn is_normalized_detects_form() {
        let n = Normalizer::new();
        assert!(n.is_normalized("é", NormalizationForm::Nfc));
        assert!(!n.is_normalized("e\u{0301}", NormalizationForm::Nfc));
        assert!(n.is_normalized("e\u{0301}", NormalizationForm::Nfd));
    }

    #[test]
    fn ascii_is_unchanged() {
        let n = Normalizer::new();
        for form in [
            NormalizationForm::Nfc,
            NormalizationForm::Nfd,
            NormalizationForm::Nfkc,
            NormalizationForm::Nfkd,
        ] {
            assert_eq!(n.normalize("hello world 123", form), "hello world 123");
        }
    }

    #[test]
    fn nfc_convenience_matches_explicit() {
        let n = Normalizer::new();
        assert_eq!(
            n.nfc("e\u{0301}"),
            n.normalize("e\u{0301}", NormalizationForm::Nfc)
        );
    }
}
