//! Integration tests for [`oxitext_icu::IcuSegmenter`].

use oxitext_icu::{IcuSegmenter, SegmentKind};

#[test]
fn line_break_cjk_latin() {
    let seg = IcuSegmenter::new();
    let text = "Hello world 日本語テキスト";
    let breaks = seg.break_points(text, SegmentKind::Line);
    assert!(
        breaks.len() > 1,
        "expected multiple line-break opportunities, got: {breaks:?}"
    );
    // Last break must be at the end of text
    assert_eq!(
        *breaks.last().expect("non-empty"),
        text.len(),
        "last break-point must equal text length"
    );
}

#[test]
fn word_break_english() {
    let seg = IcuSegmenter::new();
    let text = "Hello world";
    let breaks = seg.break_points(text, SegmentKind::Word);
    // ICU returns [0, 5, 6, 11] for "Hello world"
    assert!(
        breaks.len() >= 2,
        "expected at least 2 word break points, got: {breaks:?}"
    );
}

#[test]
fn word_break_japanese() {
    // Japanese has no spaces but ICU uses dictionary-based segmentation.
    let seg = IcuSegmenter::new();
    let text = "日本語のテキスト";
    let breaks = seg.break_points(text, SegmentKind::Word);
    // ICU should find at least 2 segments in this 8-char Japanese text.
    assert!(
        breaks.len() >= 2,
        "expected ≥2 word break points in Japanese, got: {breaks:?}"
    );
}

#[test]
fn word_break_thai() {
    let seg = IcuSegmenter::new();
    // "สวัสดีครับ" — "Hello" (polite male form) in Thai (no spaces, needs LSTM/dict)
    let text = "สวัสดีครับ";
    let breaks = seg.break_points(text, SegmentKind::Word);
    assert!(
        !breaks.is_empty(),
        "expected at least 1 word break point in Thai, got: {breaks:?}"
    );
    // Last break must be at the end of the string.
    assert_eq!(
        *breaks.last().expect("non-empty"),
        text.len(),
        "last break-point must equal text length"
    );
}

#[test]
fn grapheme_cluster_emoji() {
    let seg = IcuSegmenter::new();
    // U+1F1FA U+1F1F8 = flag emoji (US) — a two-codepoint grapheme cluster
    let text = "\u{1F1FA}\u{1F1F8}";
    let breaks = seg.break_points(text, SegmentKind::GraphemeCluster);
    // Should detect this as a single grapheme cluster (2 breaks: 0 and 8 bytes)
    assert!(
        !breaks.is_empty(),
        "expected grapheme cluster breaks, got: {breaks:?}"
    );
    assert_eq!(
        *breaks.last().expect("non-empty"),
        text.len(),
        "last break must equal text length"
    );
}

#[test]
fn sentence_break_two_sentences() {
    let seg = IcuSegmenter::new();
    let text = "Hello world. Goodbye world.";
    let breaks = seg.break_points(text, SegmentKind::Sentence);
    // Should detect 2 sentence boundaries (plus boundary at 0 and end)
    assert!(
        breaks.len() >= 2,
        "expected ≥2 sentence breaks, got: {breaks:?}"
    );
}

#[test]
fn default_segmenter_same_as_new() {
    let seg1 = IcuSegmenter::new();
    let seg2 = IcuSegmenter::default();
    let text = "test text";
    assert_eq!(
        seg1.break_points(text, SegmentKind::Word),
        seg2.break_points(text, SegmentKind::Word),
    );
}
