//! Locale-aware list formatting via `icu_list`.
//!
//! Wraps [`icu_list::ListFormatter`] to join sequences of strings into a
//! locale-appropriate conjunction, disjunction, or unit list.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::list::{IcuListFormatter, ListType};
//!
//! let fmt = IcuListFormatter::new("en", ListType::And).expect("English list formatter");
//! assert_eq!(fmt.format(&["apples", "oranges", "pears"]), "apples, oranges, and pears");
//! assert_eq!(fmt.format(&["a", "b"]), "a and b");
//! assert_eq!(fmt.format(&["sole"]), "sole");
//! assert_eq!(fmt.format(&[]), "");
//! ```

use icu_list::options::ListFormatterOptions;
use icu_list::ListFormatter;
use icu_locale_core::Locale;

use crate::CollateError;

/// The logical type of a list: conjunction ("and"), disjunction ("or"), or
/// unit formatting (e.g. "1 meter, 2 centimeters").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListType {
    /// Conjunctive list ("a, b, and c" in English).
    And,
    /// Disjunctive list ("a, b, or c" in English).
    Or,
    /// Unit list used for measurement combinations.
    Unit,
}

/// Locale-aware list formatter backed by ICU4X compiled CLDR data.
///
/// Joins a slice of string items into a locale-appropriate list string,
/// e.g. `["a", "b", "c"]` → `"a, b, and c"` (English) or
/// `"a、b、そして c"` (Japanese).
pub struct IcuListFormatter {
    inner: ListFormatter,
}

impl IcuListFormatter {
    /// Creates a new list formatter for the given BCP-47 locale and list type.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError`] if the locale string cannot be parsed or if the
    /// CLDR data cannot be loaded for the requested type.
    pub fn new(locale: &str, list_type: ListType) -> Result<Self, CollateError> {
        let loc: Locale = locale
            .parse()
            .map_err(|e| CollateError::InvalidLocale(format!("{e}")))?;
        let opts = ListFormatterOptions::default();
        let inner = match list_type {
            ListType::And => ListFormatter::try_new_and(loc.into(), opts),
            ListType::Or => ListFormatter::try_new_or(loc.into(), opts),
            ListType::Unit => ListFormatter::try_new_unit(loc.into(), opts),
        }
        .map_err(|e| CollateError::Icu(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Formats a slice of string items as a localized list.
    ///
    /// Returns an empty string for an empty slice, a bare item for a single
    /// element, and a locale-appropriate joined list otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::list::{IcuListFormatter, ListType};
    ///
    /// let fmt = IcuListFormatter::new("en", ListType::And).expect("formatter");
    /// assert_eq!(fmt.format(&["apples", "oranges", "pears"]),
    ///            "apples, oranges, and pears");
    /// ```
    pub fn format(&self, items: &[&str]) -> String {
        if items.is_empty() {
            return String::new();
        }
        self.inner.format(items.iter()).to_string()
    }

    /// Formats an owned string slice.
    ///
    /// Convenience wrapper so callers with `Vec<String>` or `&[String]` need not
    /// manually convert.
    pub fn format_owned(&self, items: &[String]) -> String {
        if items.is_empty() {
            return String::new();
        }
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        self.inner.format(refs.iter()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_list_en_and() {
        let fmt = IcuListFormatter::new("en", ListType::And).expect("English and-list");
        assert_eq!(
            fmt.format(&["apples", "oranges", "pears"]),
            "apples, oranges, and pears"
        );
        assert_eq!(fmt.format(&["a", "b"]), "a and b");
        assert_eq!(fmt.format(&["sole"]), "sole");
        assert_eq!(fmt.format(&[]), "");
    }

    #[test]
    fn format_list_en_or() {
        let fmt = IcuListFormatter::new("en", ListType::Or).expect("English or-list");
        let s = fmt.format(&["cats", "dogs"]);
        assert!(!s.is_empty(), "should produce non-empty or-list: {s}");
        assert!(
            s.contains("cats") && s.contains("dogs"),
            "should contain both items: {s}"
        );
    }

    #[test]
    fn format_list_single_item() {
        let fmt = IcuListFormatter::new("en", ListType::And).expect("formatter");
        assert_eq!(fmt.format(&["only"]), "only");
    }

    #[test]
    fn format_list_two_items() {
        let fmt = IcuListFormatter::new("en", ListType::And).expect("formatter");
        assert_eq!(fmt.format(&["a", "b"]), "a and b");
    }

    #[test]
    fn format_list_empty() {
        let fmt = IcuListFormatter::new("en", ListType::And).expect("formatter");
        assert_eq!(fmt.format(&[]), "");
    }

    #[test]
    fn format_owned_strings() {
        let fmt = IcuListFormatter::new("en", ListType::And).expect("formatter");
        let items = vec!["red".to_string(), "green".to_string(), "blue".to_string()];
        let s = fmt.format_owned(&items);
        assert!(
            s.contains("red") && s.contains("green") && s.contains("blue"),
            "should contain all colours: {s}"
        );
    }

    #[test]
    fn invalid_locale_returns_error() {
        let result = IcuListFormatter::new("not-a-locale!!!", ListType::And);
        assert!(result.is_err(), "invalid locale should produce error");
    }
}
