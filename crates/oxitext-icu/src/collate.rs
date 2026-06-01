//! Locale-aware string collation via `icu_collator`.
//!
//! Wraps [`icu_collator::Collator`] with a convenient `new_for_locale` constructor
//! that accepts a BCP-47 locale string at runtime, plus [`CollationStrength`] for
//! fine-grained control over which Unicode differences are considered significant.

use icu_collator::{
    options::CollatorOptions, options::Strength, Collator, CollatorBorrowed, CollatorPreferences,
};
use icu_locale_core::Locale;
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Errors that can occur when constructing an [`IcuCollator`].
#[derive(Debug)]
pub enum CollateError {
    /// The supplied locale string could not be parsed as a BCP-47 locale.
    InvalidLocale(String),
    /// The ICU data provider returned an error (e.g. unknown locale tailoring).
    Icu(String),
}

impl fmt::Display for CollateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollateError::InvalidLocale(s) => write!(f, "invalid locale: {s}"),
            CollateError::Icu(s) => write!(f, "ICU collation error: {s}"),
        }
    }
}

impl std::error::Error for CollateError {}

/// Controls which Unicode properties are compared during collation.
///
/// Each level is a superset of the previous: `Identical` considers all
/// differences that earlier levels ignore.
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::{IcuCollator, CollationStrength};
///
/// // Primary strength: base characters only — accents and case ignored.
/// let c = IcuCollator::with_strength("en", CollationStrength::Primary)
///     .expect("English collator");
/// assert_eq!(c.compare("Apple", "apple"), std::cmp::Ordering::Equal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollationStrength {
    /// Base characters only (e.g. `a == á == A`).
    Primary,
    /// Base + diacritics (`a == A` but `a != á`).
    Secondary,
    /// Base + diacritics + case — the default UCA level (`a != A`, `a != á`).
    #[default]
    Tertiary,
    /// Adds punctuation and whitespace distinctions (quaternary level).
    Quaternary,
    /// Full Unicode canonical decomposition — distinguishes all code-point
    /// differences not caught by earlier levels.
    Identical,
}

impl From<CollationStrength> for Strength {
    fn from(s: CollationStrength) -> Self {
        match s {
            CollationStrength::Primary => Strength::Primary,
            CollationStrength::Secondary => Strength::Secondary,
            CollationStrength::Tertiary => Strength::Tertiary,
            CollationStrength::Quaternary => Strength::Quaternary,
            CollationStrength::Identical => Strength::Identical,
        }
    }
}

/// Locale-aware string comparator using the Unicode Collation Algorithm.
///
/// Backed by `icu_collator::CollatorBorrowed<'static>` so comparisons are
/// performed directly against compiled CLDR static tables with no heap
/// allocation per comparison.
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::IcuCollator;
/// use std::cmp::Ordering;
///
/// // Swedish: "z" sorts before "ä" (ä is the last letter of the Swedish alphabet)
/// let collator = IcuCollator::new_for_locale("sv").expect("Swedish locale");
/// assert_eq!(collator.compare("z", "ä"), Ordering::Less);
/// ```
pub struct IcuCollator {
    inner: CollatorBorrowed<'static>,
}

impl IcuCollator {
    /// Creates an [`IcuCollator`] for the BCP-47 locale string `locale_id`
    /// using the default ([`CollationStrength::Tertiary`]) strength.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError::InvalidLocale`] if `locale_id` cannot be parsed,
    /// or [`CollateError::Icu`] if the ICU data provider rejects the request.
    pub fn new_for_locale(locale_id: &str) -> Result<Self, CollateError> {
        Self::with_strength(locale_id, CollationStrength::Tertiary)
    }

    /// Convenience alias for [`Self::new_for_locale`].
    ///
    /// Accepts a BCP-47 locale string and returns a collator with default
    /// ([`CollationStrength::Tertiary`]) strength.
    ///
    /// # Errors
    ///
    /// See [`Self::new_for_locale`].
    pub fn new(locale_id: &str) -> Result<Self, CollateError> {
        Self::new_for_locale(locale_id)
    }

    /// Creates an [`IcuCollator`] with the given [`CollationStrength`] for the
    /// BCP-47 locale `locale_id`.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError::InvalidLocale`] if `locale_id` cannot be parsed,
    /// or [`CollateError::Icu`] if the ICU data provider rejects the request.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::{IcuCollator, CollationStrength};
    ///
    /// let c = IcuCollator::with_strength("en", CollationStrength::Primary)
    ///     .expect("English primary collator");
    /// assert_eq!(c.compare("Apple", "apple"), std::cmp::Ordering::Equal);
    /// ```
    pub fn with_strength(
        locale_id: &str,
        strength: CollationStrength,
    ) -> Result<Self, CollateError> {
        let locale =
            Locale::from_str(locale_id).map_err(|e| CollateError::InvalidLocale(format!("{e}")))?;
        let prefs = CollatorPreferences::from(&locale);
        let mut opts = CollatorOptions::default();
        opts.strength = Some(Strength::from(strength));
        let inner =
            Collator::try_new(prefs, opts).map_err(|e| CollateError::Icu(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Compares two strings according to the locale's collation rules.
    ///
    /// Returns [`std::cmp::Ordering`] consistent with a locale-aware sort.
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        self.inner.compare(a, b)
    }

    /// Returns a sort key for `text` as a byte vector.
    ///
    /// Two strings can be ordered lexicographically by comparing their sort keys
    /// with `a_key < b_key`.  This is more efficient than repeated pairwise
    /// [`Self::compare`] calls when sorting large numbers of strings.
    ///
    /// # Note
    ///
    /// The exact format of the returned bytes is implementation-defined and
    /// subject to change with ICU data updates.  Only compare sort keys
    /// produced by the **same** [`IcuCollator`] instance (same locale and
    /// collation options).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuCollator;
    ///
    /// let collator = IcuCollator::new_for_locale("en").expect("English locale");
    /// let apple = collator.sort_key("apple");
    /// let banana = collator.sort_key("banana");
    /// assert!(apple < banana, "\"apple\" should sort before \"banana\"");
    /// ```
    pub fn sort_key(&self, text: &str) -> Vec<u8> {
        let mut key: Vec<u8> = Vec::new();
        // write_sort_key_to appends bytes to key; ignore the return value
        // (it is () for Vec<u8>).
        let _ = self.inner.write_sort_key_to(text, &mut key);
        key
    }

    /// Compares two pre-computed sort keys produced by [`Self::sort_key`].
    ///
    /// Equivalent to calling `a.cmp(b)` on the raw byte slices; provided as a
    /// static method so callers do not need to store a collator reference when
    /// comparing pre-computed keys.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuCollator;
    ///
    /// let c = IcuCollator::new_for_locale("en").expect("English locale");
    /// let ka = c.sort_key("apple");
    /// let kb = c.sort_key("banana");
    /// assert_eq!(IcuCollator::compare_sort_keys(&ka, &kb), std::cmp::Ordering::Less);
    /// ```
    pub fn compare_sort_keys(a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }
}

impl Default for IcuCollator {
    /// Returns an [`IcuCollator`] using the English locale with default
    /// ([`CollationStrength::Tertiary`]) strength.
    ///
    /// Falls back to the root locale (`"und"`) if the English locale cannot be
    /// constructed (which should never happen with compiled CLDR data).
    fn default() -> Self {
        Self::new("en").unwrap_or_else(|_| {
            Self::new("und").expect("root collator must always be constructible")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn sort_key_apple_before_banana() {
        let collator = IcuCollator::new_for_locale("en").expect("English locale");
        let apple = collator.sort_key("apple");
        let banana = collator.sort_key("banana");
        assert!(
            apple < banana,
            "\"apple\" sort key should be less than \"banana\" sort key"
        );
    }

    #[test]
    fn sort_key_equal_strings() {
        let collator = IcuCollator::new_for_locale("en").expect("English locale");
        let a = collator.sort_key("hello");
        let b = collator.sort_key("hello");
        assert_eq!(a, b, "equal strings must have equal sort keys");
    }

    #[test]
    fn sort_key_nonempty_for_nonempty_input() {
        let collator = IcuCollator::new_for_locale("en").expect("English locale");
        let key = collator.sort_key("a");
        assert!(
            !key.is_empty(),
            "sort key must be non-empty for non-empty input"
        );
    }

    #[test]
    fn sort_key_consistent_with_compare() {
        let collator = IcuCollator::new_for_locale("en").expect("English locale");
        let pairs = [("apple", "banana"), ("cat", "dog"), ("a", "b"), ("z", "zz")];
        for (a, b) in pairs {
            let cmp = collator.compare(a, b);
            let ka = collator.sort_key(a);
            let kb = collator.sort_key(b);
            let key_cmp = ka.cmp(&kb);
            assert_eq!(
                cmp, key_cmp,
                "compare({a:?}, {b:?}) = {cmp:?} but sort_key ordering = {key_cmp:?}"
            );
        }
    }

    #[test]
    fn sort_key_empty_string() {
        let collator = IcuCollator::new_for_locale("en").expect("English locale");
        let key = collator.sort_key("");
        // An empty string produces a key (possibly empty), and must be less than any non-empty
        let non_empty_key = collator.sort_key("a");
        assert!(
            key <= non_empty_key,
            "sort key of empty string should be ≤ sort key of \"a\""
        );
    }

    #[test]
    fn swedish_collation_z_before_a_umlaut() {
        let collator = IcuCollator::new_for_locale("sv").expect("Swedish locale");
        // Swedish: z < ä (ä is the last letter of the Swedish alphabet)
        assert_eq!(collator.compare("z", "ä"), Ordering::Less);
        let kz = collator.sort_key("z");
        let ka = collator.sort_key("ä");
        assert!(kz < ka, "Swedish: sort key of z should be < sort key of ä");
    }

    // ── New tests for CollationStrength / Default / compare_sort_keys ─────────

    #[test]
    fn default_collator_compares_ascii() {
        let c = IcuCollator::default();
        assert_eq!(c.compare("a", "b"), Ordering::Less);
        assert_eq!(c.compare("b", "a"), Ordering::Greater);
        assert_eq!(c.compare("a", "a"), Ordering::Equal);
    }

    #[test]
    fn primary_strength_ignores_accents() {
        // With primary strength, "e" and "é" should compare as equal.
        if let Ok(c) = IcuCollator::with_strength("en", CollationStrength::Primary) {
            let ord = c.compare("e", "é");
            // Primary: ignore accents — should be Equal
            assert_eq!(
                ord,
                Ordering::Equal,
                "Primary strength: 'e' and 'é' should be equal (got {ord:?})"
            );
        }
    }

    #[test]
    fn sort_key_ordering() {
        let c = IcuCollator::default();
        let ka = c.sort_key("apple");
        let kb = c.sort_key("banana");
        let key_ord = IcuCollator::compare_sort_keys(&ka, &kb);
        let direct_ord = c.compare("apple", "banana");
        assert_eq!(
            key_ord, direct_ord,
            "compare_sort_keys must agree with compare"
        );
    }

    #[test]
    fn swedish_collation() {
        // In Swedish, ä sorts after z.
        if let Ok(c) = IcuCollator::with_strength("sv", CollationStrength::Tertiary) {
            let ord = c.compare("z", "ä");
            assert_eq!(ord, Ordering::Less, "Swedish: 'z' should sort before 'ä'");
        }
    }

    #[test]
    fn german_phonebook_collation() {
        // German phonebook: ö treated as oe.
        // Just verify it constructs and compares without panic.
        if let Ok(c) = IcuCollator::with_strength("de-u-co-phonebk", CollationStrength::Tertiary) {
            let _ = c.compare("ost", "oerst");
        }
    }

    #[test]
    fn japanese_collation() {
        if let Ok(c) = IcuCollator::with_strength("ja", CollationStrength::Tertiary) {
            // Verify it constructs and compares without panic.
            let _ = c.compare("あ", "い");
        }
    }

    #[test]
    fn case_insensitive_primary() {
        if let Ok(c) = IcuCollator::with_strength("en", CollationStrength::Primary) {
            let ord = c.compare("Apple", "apple");
            assert_eq!(
                ord,
                Ordering::Equal,
                "Primary strength should ignore case (got {ord:?})"
            );
        }
    }

    #[test]
    fn with_strength_new_alias_equivalent() {
        // IcuCollator::new is an alias for new_for_locale.
        let c1 = IcuCollator::new("en").expect("new");
        let c2 = IcuCollator::new_for_locale("en").expect("new_for_locale");
        assert_eq!(c1.compare("hello", "world"), c2.compare("hello", "world"));
    }
}
