//! Demonstrates the four independent ICU4X-backed facilities `oxitext-icu`
//! provides: CLDR text segmentation, Unicode normalization, locale-aware
//! collation, and locale-aware case mapping. None of these require any
//! Cargo feature — they are unconditionally available once the crate is a
//! dependency.
//!
//! Run with:
//! ```text
//! cargo run -p oxitext-icu --example cldr_text_processing
//! ```

use oxitext_icu::{
    CaseMapper, CollationStrength, IcuCollator, IcuSegmenter, NormalizationForm, Normalizer,
    SegmentKind,
};

fn main() {
    // ─── 1. CLDR segmentation: word, grapheme-cluster, and line breaks ──────

    let seg = IcuSegmenter::new();
    let sentence = "The quick fox jumps.";

    let segments = seg.segments(sentence, SegmentKind::Word);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .map(|s| s.text.as_str())
        .collect();
    println!("words in {sentence:?}: {words:?}");
    assert!(words.contains(&"quick"));

    // A grapheme cluster is a user-perceived character; "é" written as
    // "e" + a combining acute accent is two `char`s (byte offsets 0..1 and
    // 1..3) but one grapheme cluster, so there must be no boundary at byte
    // offset 1 — only at the start (0), after the merged "é" (3), and after
    // each subsequent plain-ASCII letter of "clair".
    let combining_e = "e\u{0301}clair"; // "e" + U+0301 COMBINING ACUTE ACCENT
    let graphemes = seg.break_points(combining_e, SegmentKind::GraphemeCluster);
    println!("{combining_e:?} grapheme cluster boundaries (byte offsets): {graphemes:?}");
    assert_eq!(
        graphemes,
        vec![0, 3, 4, 5, 6, 7, 8],
        "\"e\" + combining accent must merge into one grapheme cluster \
         (no boundary at byte offset 1, between the base letter and the accent)"
    );

    // ─── 2. Unicode normalization ────────────────────────────────────────────

    let normalizer = Normalizer::new();
    // Precomposed "é" (U+00E9) vs. decomposed "e" + U+0301 must normalize to
    // the same NFC string.
    let precomposed = "caf\u{00E9}";
    let decomposed = "cafe\u{0301}";
    let nfc_a = normalizer.normalize(precomposed, NormalizationForm::Nfc);
    let nfc_b = normalizer.normalize(decomposed, NormalizationForm::Nfc);
    println!(
        "NFC({precomposed:?}) == NFC({decomposed:?}) -> {}",
        nfc_a == nfc_b
    );
    assert_eq!(nfc_a, nfc_b);
    assert!(normalizer.is_normalized(&nfc_a, NormalizationForm::Nfc));

    // ─── 3. Locale-aware collation ───────────────────────────────────────────

    // Primary strength ignores case and accents, so "resume" and "Résumé"
    // compare equal even though they differ byte-for-byte.
    let collator = IcuCollator::with_strength("en", CollationStrength::Primary)
        .expect("English collator must build");
    let a = "resume";
    let b = "R\u{00E9}sum\u{00E9}";
    println!(
        "IcuCollator(en, Primary).compare({a:?}, {b:?}) = {:?}",
        collator.compare(a, b)
    );
    assert_eq!(collator.compare(a, b), std::cmp::Ordering::Equal);

    // ─── 4. Locale-aware case mapping ────────────────────────────────────────
    //
    // Turkish famously differs from English in how it lowercases 'I': plain
    // ASCII case folding gives "i", but Turkish's dotless/dotted-I rules
    // lowercase capital 'I' to dotless 'ı' (U+0131).
    let case_mapper = CaseMapper::new();
    let en_lower = case_mapper.to_lowercase("I", "en");
    let tr_lower = case_mapper.to_lowercase("I", "tr");
    println!("to_lowercase(\"I\", en) = {en_lower:?}, to_lowercase(\"I\", tr) = {tr_lower:?}");
    assert_eq!(en_lower, "i");
    assert_eq!(tr_lower, "\u{0131}");
}
