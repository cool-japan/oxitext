//! Integration tests: bidi + vertical working together on mixed-script text.

use oxitext_layout::bidi::BidiParagraph;
use oxitext_layout::vertical::is_upright_in_vertical;

#[test]
fn mixed_cjk_latin_bidi_does_not_panic() {
    let text = "Hello 日本語 World";
    let para = BidiParagraph::new(text, None);
    // Mixed CJK + Latin is still an LTR paragraph.
    assert!(
        !para.runs().is_empty(),
        "mixed CJK+Latin should produce at least one bidi run"
    );
    assert!(
        !para.is_rtl(),
        "CJK+Latin paragraph should have LTR base direction"
    );
}

#[test]
fn vertical_orientation_of_mixed() {
    let mixed = "abc日本語def";
    let upright_count = mixed.chars().filter(|c| is_upright_in_vertical(*c)).count();
    let rotated_count = mixed
        .chars()
        .filter(|c| !is_upright_in_vertical(*c) && c.is_alphanumeric())
        .count();
    assert_eq!(upright_count, 3, "exactly 3 CJK chars should be upright");
    assert_eq!(rotated_count, 6, "exactly 6 Latin chars should rotate");
}

#[test]
fn arabic_text_auto_detects_rtl() {
    // Basic Arabic text: مرحبا (marhaba = hello)
    let text = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
    let para = BidiParagraph::new(text, None);
    assert!(
        para.is_rtl(),
        "Arabic text should auto-detect as RTL paragraph"
    );
}
