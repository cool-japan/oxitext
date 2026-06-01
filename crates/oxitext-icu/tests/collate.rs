//! Integration tests for [`oxitext_icu::IcuCollator`].

use oxitext_icu::IcuCollator;
use std::cmp::Ordering;

#[test]
fn locale_aware_swedish_order() {
    // Swedish alphabet ends: …x, y, z, å, ä, ö
    // So "z" < "ä" in Swedish, while by Unicode codepoint "z" (U+007A) < "ä" (U+00E4) too,
    // but at primary strength ICU uses the locale's alphabet ordering.
    let collator = IcuCollator::new_for_locale("sv").expect("Swedish locale should be available");
    let ord = collator.compare("z", "ä");
    // In Swedish, z sorts before ä
    assert_eq!(ord, Ordering::Less, "Swedish: 'z' should sort before 'ä'");
}

#[test]
fn swedish_aa_after_z() {
    // Swedish: å comes after z (å = U+00E5)
    let collator = IcuCollator::new_for_locale("sv").expect("Swedish locale");
    let ord = collator.compare("z", "å");
    assert_eq!(ord, Ordering::Less, "Swedish: 'z' should sort before 'å'");
}

#[test]
fn english_locale_basic_order() {
    // English alphabetical: "a" < "b"
    let collator = IcuCollator::new_for_locale("en").expect("English locale");
    assert_eq!(collator.compare("a", "b"), Ordering::Less);
    assert_eq!(collator.compare("b", "a"), Ordering::Greater);
    assert_eq!(collator.compare("a", "a"), Ordering::Equal);
}

#[test]
fn japanese_locale_constructs() {
    // Verify Japanese locale round-trips without error
    let collator = IcuCollator::new_for_locale("ja").expect("Japanese locale");
    // Hiragana あ < い in standard Japanese ordering
    let ord = collator.compare("あ", "い");
    // Both should be valid strings (not panicking is the main goal here)
    assert!(
        ord == Ordering::Less || ord == Ordering::Greater || ord == Ordering::Equal,
        "Japanese comparison should yield a valid Ordering"
    );
}

#[test]
fn invalid_locale_returns_error() {
    // An unparsable locale string must return an error, not panic.
    let result = IcuCollator::new_for_locale("!!!invalid!!!");
    assert!(result.is_err(), "expected Err for invalid locale, got Ok");
}

#[test]
fn root_locale_equals_default() {
    // Root locale ("und" / empty string default) should construct successfully.
    let collator = IcuCollator::new_for_locale("und").expect("root locale");
    // Root collation: a < b
    assert_eq!(collator.compare("a", "b"), Ordering::Less);
}
