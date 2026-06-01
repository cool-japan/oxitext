use oxitext_layout::vertical::{is_upright_in_vertical, VerticalMetrics};

#[test]
fn cjk_is_upright() {
    assert!(
        is_upright_in_vertical('日'),
        "'日' should be upright in vertical text"
    );
    assert!(
        is_upright_in_vertical('本'),
        "'本' should be upright in vertical text"
    );
    assert!(
        is_upright_in_vertical('語'),
        "'語' should be upright in vertical text"
    );
}

#[test]
fn hiragana_is_upright() {
    assert!(
        is_upright_in_vertical('あ'),
        "'あ' should be upright in vertical text"
    );
    assert!(
        is_upright_in_vertical('い'),
        "'い' should be upright in vertical text"
    );
}

#[test]
fn katakana_is_upright() {
    assert!(
        is_upright_in_vertical('ア'),
        "'ア' should be upright in vertical text"
    );
    assert!(
        is_upright_in_vertical('イ'),
        "'イ' should be upright in vertical text"
    );
}

#[test]
fn latin_is_not_upright() {
    assert!(
        !is_upright_in_vertical('A'),
        "'A' should rotate in vertical text"
    );
    assert!(
        !is_upright_in_vertical('z'),
        "'z' should rotate in vertical text"
    );
}

#[test]
fn digits_are_not_upright() {
    assert!(
        !is_upright_in_vertical('0'),
        "'0' should rotate in vertical text"
    );
    assert!(
        !is_upright_in_vertical('9'),
        "'9' should rotate in vertical text"
    );
}

#[test]
fn hangul_is_upright() {
    // '가' U+AC00 is the first Hangul syllable.
    assert!(
        is_upright_in_vertical('\u{AC00}'),
        "Hangul should be upright"
    );
}

#[test]
fn vertical_metrics_upright_flag() {
    let vm_cjk = VerticalMetrics::for_char('字', 16.0);
    assert!(vm_cjk.upright, "CJK char should have upright=true");
    let vm_latin = VerticalMetrics::for_char('A', 16.0);
    assert!(!vm_latin.upright, "Latin char should have upright=false");
}

#[test]
fn vertical_metrics_advance_equals_em() {
    let vm = VerticalMetrics::for_char('字', 24.0);
    assert!(
        (vm.advance - 24.0).abs() < f32::EPSILON,
        "advance should equal em_size (fallback path)"
    );
}
