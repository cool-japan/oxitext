//! Locale-aware plural rules via `icu_plurals`.
//!
//! Wraps [`icu_plurals::PluralRules`] to determine the correct plural category
//! for a numeric value in a given locale, and to select the appropriate string
//! form from a set of plural variants.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::plural::{IcuPluralRules, PluralCategory};
//!
//! let rules = IcuPluralRules::new("en").expect("English plural rules");
//! assert_eq!(rules.category_for(1), PluralCategory::One);
//! assert_eq!(rules.category_for(0), PluralCategory::Other);
//! assert_eq!(rules.select(1, "item", "items"), "item");
//! assert_eq!(rules.select(5, "item", "items"), "items");
//! ```

use icu_locale_core::Locale;
use icu_plurals::{PluralCategory as IcuCategory, PluralOperands, PluralRuleType, PluralRules};

use crate::CollateError;

/// CLDR plural category for a numeric value.
///
/// Maps to the six categories defined by the Unicode Plural Rules specification
/// (LDML § Plural Rules). Not all categories are used in every locale — for
/// example, English only uses `One` and `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    /// Used in languages that have a special form for zero (e.g. Arabic, Welsh).
    Zero,
    /// Singular form (e.g. "1 item" in English).
    One,
    /// Dual form (e.g. used in Arabic, Hebrew, Slovenian).
    Two,
    /// "Few" form (e.g. used in Polish, Czech, Russian for 2–4).
    Few,
    /// "Many" form (e.g. used in Polish for 5+, or in Irish).
    Many,
    /// Default/general plural (e.g. "5 items" in English).
    Other,
}

impl PluralCategory {
    fn from_icu(cat: IcuCategory) -> Self {
        match cat {
            IcuCategory::Zero => PluralCategory::Zero,
            IcuCategory::One => PluralCategory::One,
            IcuCategory::Two => PluralCategory::Two,
            IcuCategory::Few => PluralCategory::Few,
            IcuCategory::Many => PluralCategory::Many,
            IcuCategory::Other => PluralCategory::Other,
        }
    }
}

/// Locale-aware plural-rule evaluator backed by ICU4X compiled CLDR data.
///
/// Determines the correct plural category for a count value and can select
/// the appropriate string form from a set of variants. Supports both cardinal
/// (counting) and ordinal (ranking) rules.
pub struct IcuPluralRules {
    cardinal: PluralRules,
    ordinal: PluralRules,
}

impl IcuPluralRules {
    /// Creates plural rules for the given BCP-47 locale string.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError`] if the locale string cannot be parsed or if the
    /// CLDR data for the locale cannot be loaded.
    pub fn new(locale: &str) -> Result<Self, CollateError> {
        let loc: Locale = locale
            .parse()
            .map_err(|e| CollateError::InvalidLocale(format!("{e}")))?;
        let prefs = icu_plurals::PluralRulesPreferences::from(loc);
        let cardinal_opts: icu_plurals::PluralRulesOptions = PluralRuleType::Cardinal.into();
        let ordinal_opts: icu_plurals::PluralRulesOptions = PluralRuleType::Ordinal.into();
        let cardinal = PluralRules::try_new(prefs, cardinal_opts)
            .map_err(|e| CollateError::Icu(format!("{e}")))?;
        let ordinal = PluralRules::try_new(prefs, ordinal_opts)
            .map_err(|e| CollateError::Icu(format!("{e}")))?;
        Ok(Self { cardinal, ordinal })
    }

    /// Returns the cardinal [`PluralCategory`] for the given count value.
    ///
    /// Cardinal categories are used for counting ("1 item", "2 items").
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::plural::{IcuPluralRules, PluralCategory};
    ///
    /// let rules = IcuPluralRules::new("en").expect("rules");
    /// assert_eq!(rules.category_for(1), PluralCategory::One);
    /// assert_eq!(rules.category_for(2), PluralCategory::Other);
    /// ```
    pub fn category_for(&self, count: u64) -> PluralCategory {
        let operands: PluralOperands = count.into();
        PluralCategory::from_icu(self.cardinal.category_for(operands))
    }

    /// Returns the ordinal [`PluralCategory`] for the given count value.
    ///
    /// Ordinal categories are used for ranking ("1st", "2nd", "3rd" in English).
    pub fn ordinal_category_for(&self, count: u64) -> PluralCategory {
        let operands: PluralOperands = count.into();
        PluralCategory::from_icu(self.ordinal.category_for(operands))
    }

    /// Selects between a singular and plural form based on the cardinal category.
    ///
    /// For languages with only `One` / `Other` categories (e.g. English),
    /// `one` is returned when `count == 1`, and `other` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::plural::IcuPluralRules;
    ///
    /// let rules = IcuPluralRules::new("en").expect("rules");
    /// assert_eq!(rules.select(1, "item", "items"), "item");
    /// assert_eq!(rules.select(2, "item", "items"), "items");
    /// assert_eq!(rules.select(0, "item", "items"), "items");
    /// ```
    pub fn select<'a>(&self, count: u64, one: &'a str, other: &'a str) -> &'a str {
        match self.category_for(count) {
            PluralCategory::One => one,
            _ => other,
        }
    }

    /// Returns all plural categories used by this locale's cardinal rules.
    ///
    /// Collects the iterator into a `Vec` of ICU category values.  Useful for
    /// template systems that need to know which forms to request from translators.
    pub fn categories(&self) -> Vec<IcuCategory> {
        self.cardinal.categories().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_english_cardinal() {
        let rules = IcuPluralRules::new("en").expect("English plural rules");
        assert_eq!(rules.category_for(0), PluralCategory::Other);
        assert_eq!(rules.category_for(1), PluralCategory::One);
        assert_eq!(rules.category_for(2), PluralCategory::Other);
        assert_eq!(rules.category_for(5), PluralCategory::Other);
        assert_eq!(rules.category_for(100), PluralCategory::Other);
    }

    #[test]
    fn plural_english_select() {
        let rules = IcuPluralRules::new("en").expect("rules");
        assert_eq!(rules.select(1, "item", "items"), "item");
        assert_eq!(rules.select(2, "item", "items"), "items");
        assert_eq!(rules.select(0, "item", "items"), "items");
    }

    #[test]
    fn plural_russian_has_few_category() {
        // Russian uses One (1, 21, 31…), Few (2-4, 22-24…), Many (5-20, 25-30…), Other
        let rules = IcuPluralRules::new("ru").expect("Russian plural rules");
        assert_eq!(rules.category_for(1), PluralCategory::One);
        assert_eq!(rules.category_for(2), PluralCategory::Few);
        assert_eq!(rules.category_for(5), PluralCategory::Many);
        assert_eq!(rules.category_for(11), PluralCategory::Many);
        assert_eq!(rules.category_for(21), PluralCategory::One);
    }

    #[test]
    fn plural_arabic_zero_category() {
        // Arabic uses Zero, One, Two, Few (3-10), Many (11-99), Other (100+)
        let rules = IcuPluralRules::new("ar").expect("Arabic plural rules");
        assert_eq!(rules.category_for(0), PluralCategory::Zero);
        assert_eq!(rules.category_for(1), PluralCategory::One);
        assert_eq!(rules.category_for(2), PluralCategory::Two);
    }

    #[test]
    fn plural_english_ordinal() {
        // English ordinals: One (1st), Two (2nd), Few (3rd), Other (4th, 5th…)
        let rules = IcuPluralRules::new("en").expect("rules");
        assert_eq!(rules.ordinal_category_for(1), PluralCategory::One);
        assert_eq!(rules.ordinal_category_for(2), PluralCategory::Two);
        assert_eq!(rules.ordinal_category_for(3), PluralCategory::Few);
        assert_eq!(rules.ordinal_category_for(4), PluralCategory::Other);
    }

    #[test]
    fn categories_returns_vec() {
        let rules = IcuPluralRules::new("en").expect("rules");
        let cats = rules.categories();
        assert!(!cats.is_empty(), "categories should not be empty");
    }

    #[test]
    fn invalid_locale_returns_error() {
        let result = IcuPluralRules::new("not-a-locale!!!");
        assert!(result.is_err(), "invalid locale should produce error");
    }
}
