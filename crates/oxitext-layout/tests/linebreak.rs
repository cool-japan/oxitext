use oxitext_layout::linebreak::{LineBreak, LineBreaker};

#[test]
fn nbsp_prevents_break_at_its_position() {
    // NBSP (U+00A0) between "hello" and "world" should not be a break point.
    let text = "hello\u{00A0}world";
    let lb = LineBreaker::new(text);
    let breaks: Vec<_> = lb.iter().cloned().collect();

    // There must be at least one break at the end of the whole string.
    assert!(
        !breaks.is_empty(),
        "should have at least one break opportunity"
    );

    // The NBSP itself should not yield an Allowed break immediately after it.
    // The NBSP ends at byte offset "hello\u{00A0}".len() = 5 + 2 = 7.
    let nbsp_end = "hello\u{00A0}".len();
    let break_after_nbsp = breaks
        .iter()
        .any(|(pos, kind)| *pos == nbsp_end && *kind == LineBreak::Allowed);
    assert!(
        !break_after_nbsp,
        "NBSP should suppress an Allowed break at its trailing edge (pos {nbsp_end})"
    );
}

#[test]
fn break_after_space() {
    let text = "hello world";
    let lb = LineBreaker::new(text);
    let breaks: Vec<_> = lb.into_iter().collect();
    assert!(
        !breaks.is_empty(),
        "should have break opportunities in 'hello world'"
    );
}

#[test]
fn newline_produces_mandatory_break() {
    let text = "hello\nworld";
    let lb = LineBreaker::new(text);
    let has_mandatory = lb.iter().any(|(_, kind)| *kind == LineBreak::Mandatory);
    assert!(has_mandatory, "newline should produce a Mandatory break");
}

#[test]
fn empty_string_does_not_panic() {
    let lb = LineBreaker::new("");
    // May produce zero or one entry — must not panic.
    let _ = lb.breaks();
}
