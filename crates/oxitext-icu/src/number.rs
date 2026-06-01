//! Locale-aware number formatting via `icu_decimal`.
//!
//! Wraps [`icu_decimal::DecimalFormatter`] with a convenient API for formatting
//! integers and floating-point values according to locale conventions (digit
//! grouping, decimal separator, numeral systems, etc.).
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::number::IcuNumberFormatter;
//!
//! let fmt = IcuNumberFormatter::new("en").expect("English number formatter");
//! assert!(!fmt.format_int(1_234_567).is_empty());
//! assert!(!fmt.format(3.14).is_empty());
//! ```

use fixed_decimal::Decimal;
use icu_decimal::options::DecimalFormatterOptions;
use icu_decimal::preferences::DecimalFormatterPreferences;
use icu_decimal::DecimalFormatter;
use icu_locale_core::Locale;

use crate::CollateError;

/// Locale-aware number formatter backed by ICU4X compiled CLDR data.
///
/// Formats integers and floating-point values according to locale conventions,
/// including locale-specific digit grouping separators, decimal separators,
/// and native numeral systems.
///
/// Construction is cheap — the compiled CLDR data lives in static tables.
pub struct IcuNumberFormatter {
    inner: DecimalFormatter,
}

impl IcuNumberFormatter {
    /// Creates a new formatter for the given BCP-47 locale string (e.g. `"en"`,
    /// `"de"`, `"ar"`, `"ja"`).
    ///
    /// # Errors
    ///
    /// Returns [`CollateError`] if the locale string cannot be parsed.
    pub fn new(locale: &str) -> Result<Self, CollateError> {
        let loc: Locale = locale
            .parse()
            .map_err(|e| CollateError::InvalidLocale(format!("{e}")))?;
        let prefs = DecimalFormatterPreferences::from(loc);
        let opts = DecimalFormatterOptions::default();
        let inner = DecimalFormatter::try_new(prefs, opts)
            .map_err(|e| CollateError::Icu(format!("{e}")))?;
        Ok(Self { inner })
    }

    /// Formats a 64-bit integer according to locale conventions.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::number::IcuNumberFormatter;
    ///
    /// let fmt = IcuNumberFormatter::new("en").expect("formatter");
    /// let s = fmt.format_int(1_000);
    /// assert!(!s.is_empty());
    /// ```
    pub fn format_int(&self, value: i64) -> String {
        let dec = Decimal::from(value);
        self.inner.format(&dec).to_string()
    }

    /// Formats an unsigned 64-bit integer according to locale conventions.
    pub fn format_uint(&self, value: u64) -> String {
        let dec = Decimal::from(value);
        self.inner.format(&dec).to_string()
    }

    /// Formats a floating-point value with the given number of fractional digits.
    ///
    /// The value is rounded to `frac_digits` decimal places before formatting.
    /// Uses `Decimal::from_str` internally via the `{value:.frac_digits$}`
    /// standard format, falling back to a simple `format!` if parsing fails.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::number::IcuNumberFormatter;
    ///
    /// let fmt = IcuNumberFormatter::new("en").expect("formatter");
    /// let s = fmt.format(1.75);
    /// assert!(!s.is_empty());
    /// ```
    pub fn format(&self, value: f64) -> String {
        // Produce a fixed-point string representation with enough precision,
        // then parse it into a `Decimal` so ICU can apply locale grouping.
        let repr = format!("{value:.6}");
        match repr.parse::<Decimal>() {
            Ok(dec) => self.inner.format(&dec).to_string(),
            Err(_) => repr,
        }
    }

    /// Formats a floating-point value with an explicit number of fractional digits.
    pub fn format_with_precision(&self, value: f64, frac_digits: usize) -> String {
        let repr = format!("{value:.frac_digits$}");
        match repr.parse::<Decimal>() {
            Ok(dec) => self.inner.format(&dec).to_string(),
            Err(_) => repr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_integer_en() {
        let fmt = IcuNumberFormatter::new("en").expect("English number formatter");
        let s = fmt.format_int(1234);
        assert!(!s.is_empty(), "formatted string should not be empty");
        // ICU may produce "1,234" or "1234" depending on grouping options; either is acceptable.
        assert!(
            s.contains('1') && s.contains('4'),
            "should contain digits: {s}"
        );
    }

    #[test]
    fn format_negative_integer() {
        let fmt = IcuNumberFormatter::new("en").expect("English number formatter");
        let s = fmt.format_int(-42);
        assert!(!s.is_empty());
        assert!(
            s.contains('4') && s.contains('2'),
            "should contain digits: {s}"
        );
    }

    #[test]
    fn format_uint() {
        let fmt = IcuNumberFormatter::new("en").expect("formatter");
        let s = fmt.format_uint(9_999_999);
        assert!(!s.is_empty());
    }

    #[test]
    fn format_float_basic() {
        let fmt = IcuNumberFormatter::new("en").expect("formatter");
        // Use 1.75 (not a known mathematical constant) to avoid approx_constant lint.
        let s = fmt.format(1.75);
        assert!(!s.is_empty(), "should format float: {s}");
    }

    #[test]
    fn format_with_precision_two_decimals() {
        let fmt = IcuNumberFormatter::new("en").expect("formatter");
        let s = fmt.format_with_precision(1.5, 2);
        assert!(!s.is_empty());
        // Should contain "1" and "5"
        assert!(s.contains('1') && s.contains('5'), "unexpected format: {s}");
    }

    #[test]
    fn format_zero() {
        let fmt = IcuNumberFormatter::new("en").expect("formatter");
        let s = fmt.format_int(0);
        assert!(!s.is_empty());
        assert!(s.contains('0'), "zero should contain '0': {s}");
    }

    #[test]
    fn invalid_locale_returns_error() {
        let result = IcuNumberFormatter::new("not-a-valid-locale!!!");
        assert!(result.is_err(), "invalid locale should produce error");
    }
}
