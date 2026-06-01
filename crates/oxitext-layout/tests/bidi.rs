use oxitext_layout::bidi::BidiParagraph;

#[test]
fn ltr_only_text_has_even_levels() {
    let para = BidiParagraph::new("hello world", None);
    // All runs should have even embedding levels (LTR).
    assert!(
        para.runs().iter().all(|r| r.level % 2 == 0),
        "pure LTR text should produce only even-level runs"
    );
}

#[test]
fn rtl_embedded_has_multiple_runs() {
    // Hebrew characters embedded in an LTR sentence.
    // "abc " + ALEF BET GIMEL + " def"
    let text = "abc \u{05D0}\u{05D1}\u{05D2} def";
    let para = BidiParagraph::new(text, None);
    assert!(
        para.runs().len() >= 2,
        "mixed LTR+RTL text should produce at least 2 bidi runs, got {}",
        para.runs().len()
    );
    // At least one run should be RTL (odd level).
    assert!(
        para.runs().iter().any(|r| r.level % 2 == 1),
        "expected at least one RTL run in mixed text"
    );
}

#[test]
fn forced_rtl_base() {
    let para = BidiParagraph::new("hello", Some(true));
    assert!(
        para.is_rtl(),
        "paragraph with forced RTL base should report is_rtl()=true"
    );
}

#[test]
fn forced_ltr_base() {
    let para = BidiParagraph::new("hello", Some(false));
    assert!(
        !para.is_rtl(),
        "paragraph with forced LTR base should report is_rtl()=false"
    );
}

#[test]
fn empty_string_does_not_panic() {
    let para = BidiParagraph::new("", None);
    // Empty string may produce zero runs — just must not panic.
    let _ = para.runs();
    let _ = para.base_level();
}

#[test]
fn pure_rtl_text_auto_detected() {
    // A string consisting entirely of Hebrew should auto-detect as RTL.
    let text = "\u{05E9}\u{05DC}\u{05D5}\u{05DD}"; // שלום (shalom)
    let para = BidiParagraph::new(text, None);
    assert!(
        para.is_rtl(),
        "pure Hebrew text should auto-detect as RTL paragraph"
    );
}
