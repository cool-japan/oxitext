//! CLDR-based text segmentation via `icu_segmenter`.
//!
//! Wraps the four ICU4X segmenters (line, word, grapheme-cluster, sentence)
//! into a single [`IcuSegmenter`] struct, using compiled CLDR data so no
//! external data provider is required at runtime.

use std::collections::HashMap;
use std::sync::Mutex;

use icu_segmenter::options::{
    LineBreakOptions, SentenceBreakInvariantOptions, WordBreakInvariantOptions,
};
use icu_segmenter::{
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, LineSegmenter,
    LineSegmenterBorrowed, SentenceSegmenter, SentenceSegmenterBorrowed, WordSegmenter,
    WordSegmenterBorrowed,
};

/// The kind of text boundary to locate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    /// UAX #14 line-break opportunities (suitable for word-wrapping).
    Line,
    /// UAX #29 word boundaries.
    Word,
    /// UAX #29 grapheme-cluster boundaries (user-perceived characters).
    GraphemeCluster,
    /// Alias for [`SegmentKind::GraphemeCluster`] — UAX #29 grapheme-cluster
    /// boundaries (user-perceived characters).
    Grapheme,
    /// UAX #29 sentence boundaries.
    Sentence,
}

/// A text segment with position information and kind metadata.
///
/// Produced by `IcuSegmenter::rich_segments`; `byte_start` and `byte_end`
/// are UTF-8 byte offsets into the analysed string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The text content of the segment (owned copy).
    pub text: String,
    /// UTF-8 byte offset of the start of the segment (inclusive).
    pub byte_start: usize,
    /// UTF-8 byte offset of the end of the segment (exclusive).
    pub byte_end: usize,
    /// The kind of boundary used to produce this segment.
    pub kind: SegmentKind,
}

/// Multi-kind text segmenter backed by ICU4X CLDR compiled data.
///
/// Holds borrowed references into static CLDR tables; constructing one is
/// essentially free (no allocation, no I/O).
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::{IcuSegmenter, SegmentKind};
///
/// let seg = IcuSegmenter::new();
/// let breaks = seg.break_points("Hello world", SegmentKind::Word);
/// // "Hello world" has break opportunities at [0, 5, 6, 11]
/// assert!(breaks.len() >= 2);
/// ```
pub struct IcuSegmenter {
    line: LineSegmenterBorrowed<'static>,
    word: WordSegmenterBorrowed<'static>,
    grapheme: GraphemeClusterSegmenterBorrowed<'static>,
    sentence: SentenceSegmenterBorrowed<'static>,
    /// Memoisation cache for [`Self::segments`].
    ///
    /// Uses interior mutability so the public API remains `&self`, which is
    /// important for GUI render-loops that hold a shared reference.
    segment_cache: Mutex<HashMap<(String, SegmentKind), Vec<Segment>>>,
    /// Memoisation cache for [`Self::break_points`].
    break_cache: Mutex<HashMap<(String, SegmentKind), Vec<usize>>>,
}

impl IcuSegmenter {
    /// Creates a new locale-invariant [`IcuSegmenter`] using compiled CLDR data.
    ///
    /// All four segmenters are initialised with default options and the best
    /// available algorithm for complex scripts (LSTM model for line and word
    /// breaking in South-East Asian scripts; dictionary for Japanese).
    ///
    /// For locale-aware construction (currently only affects the invariant
    /// options) use [`Self::new_with_locale`].
    pub fn new() -> Self {
        Self {
            line: LineSegmenter::new_auto(LineBreakOptions::default()),
            word: WordSegmenter::new_auto(WordBreakInvariantOptions::default()),
            grapheme: GraphemeClusterSegmenter::new(),
            sentence: SentenceSegmenter::new(SentenceBreakInvariantOptions::default()),
            segment_cache: Mutex::new(HashMap::new()),
            break_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a locale-aware [`IcuSegmenter`].
    ///
    /// `locale_id` is a BCP-47 locale string (e.g. `"en"`, `"ja"`, `"th"`).
    /// The compiled CLDR data already includes dictionary/LSTM models for all
    /// supported scripts, so in practice this behaves identically to [`Self::new`]
    /// for most locales.  The constructor is provided for API symmetry with
    /// `IcuCollator::new`.
    ///
    /// # Errors
    ///
    /// The locale string is accepted for documentation purposes only; this
    /// constructor always succeeds (returns `Ok`).
    pub fn new_with_locale(_locale_id: &str) -> Result<Self, crate::CollateError> {
        Ok(Self::new())
    }

    /// Returns the byte-offset break-points in `text` for the given [`SegmentKind`].
    ///
    /// The returned `Vec` always includes the length of `text` as the final
    /// element (the boundary at end-of-string), and may include `0` as the first
    /// element depending on the segmenter.  Callers typically use adjacent pairs
    /// `[breaks[i]..breaks[i+1]]` to iterate over segments.
    ///
    /// Results are memoised: repeated calls with the same `(text, kind)` pair return
    /// a clone of the cached result without re-running the segmenter.
    pub fn break_points(&self, text: &str, kind: SegmentKind) -> Vec<usize> {
        // Fast path: return a clone of the cached result if present.
        {
            let cache = self.break_cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(&(text.to_owned(), kind)) {
                return cached.clone();
            }
        }
        // Slow path: compute and cache.
        let result: Vec<usize> = match kind {
            SegmentKind::Line => self.line.segment_str(text).collect(),
            SegmentKind::Word => self.word.segment_str(text).collect(),
            SegmentKind::GraphemeCluster | SegmentKind::Grapheme => {
                self.grapheme.segment_str(text).collect()
            }
            SegmentKind::Sentence => self.sentence.segment_str(text).collect(),
        };
        {
            let mut cache = self.break_cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert((text.to_owned(), kind), result.clone());
        }
        result
    }

    /// Returns the actual text substrings for the given [`SegmentKind`].
    ///
    /// Unlike [`Self::break_points`] (which returns raw byte offsets) or
    /// [`Self::segments`] (which returns owned [`Segment`] structs with full
    /// position metadata), this returns borrowed string slices — useful when
    /// ownership of the text content is not required.
    ///
    /// Empty leading/trailing slices are omitted.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::{IcuSegmenter, SegmentKind};
    ///
    /// let seg = IcuSegmenter::new();
    /// let words = seg.segment_strs("Hello world", SegmentKind::Word);
    /// // "Hello", " ", "world" (word segmenter includes the space segment).
    /// assert!(words.contains(&"Hello"));
    /// assert!(words.contains(&"world"));
    /// ```
    pub fn segment_strs<'a>(&self, text: &'a str, kind: SegmentKind) -> Vec<&'a str> {
        let breaks = self.break_points(text, kind);
        let mut out = Vec::new();
        let mut prev = 0usize;
        for &b in &breaks {
            if b > prev && b <= text.len() {
                if let Some(slice) = text.get(prev..b) {
                    out.push(slice);
                }
                prev = b;
            }
        }
        // Tail (some segmenters may not emit a final boundary at text.len()).
        if prev < text.len() {
            if let Some(slice) = text.get(prev..) {
                out.push(slice);
            }
        }
        out
    }

    /// Returns rich [`Segment`] structs for the given [`SegmentKind`].
    ///
    /// Each segment carries an owned copy of the text, its UTF-8 byte offsets,
    /// and the [`SegmentKind`] that produced it.  This is the primary API for
    /// consumers that need byte-position metadata alongside the segment text.
    ///
    /// Results are memoised: repeated calls with the same `(text, kind)` pair return
    /// a clone of the cached result without re-running the segmenter.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::{IcuSegmenter, SegmentKind};
    ///
    /// let seg = IcuSegmenter::new();
    /// let segs = seg.segments("hello world", SegmentKind::Word);
    /// let words: Vec<&str> = segs.iter()
    ///     .filter(|s| !s.text.trim().is_empty())
    ///     .map(|s| s.text.as_str())
    ///     .collect();
    /// assert!(words.contains(&"hello"));
    /// assert!(words.contains(&"world"));
    /// // Byte offsets are valid and match the text.
    /// for s in &segs {
    ///     assert_eq!(&"hello world"[s.byte_start..s.byte_end], s.text.as_str());
    /// }
    /// ```
    pub fn segments(&self, text: &str, kind: SegmentKind) -> Vec<Segment> {
        // Fast path: return a clone of the cached result if present.
        {
            let cache = self.segment_cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(&(text.to_owned(), kind)) {
                return cached.clone();
            }
        }
        // Slow path: compute using (already cached) break_points.
        let breaks = self.break_points(text, kind);
        let mut out = Vec::new();
        let mut prev = 0usize;
        for &b in &breaks {
            if b > prev && b <= text.len() {
                if let Some(slice) = text.get(prev..b) {
                    out.push(Segment {
                        text: slice.to_owned(),
                        byte_start: prev,
                        byte_end: b,
                        kind,
                    });
                }
                prev = b;
            }
        }
        // Tail (some segmenters may not emit a final boundary at text.len()).
        if prev < text.len() {
            if let Some(slice) = text.get(prev..) {
                out.push(Segment {
                    text: slice.to_owned(),
                    byte_start: prev,
                    byte_end: text.len(),
                    kind,
                });
            }
        }
        {
            let mut cache = self.segment_cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert((text.to_owned(), kind), out.clone());
        }
        out
    }

    /// Creates a locale-aware [`IcuSegmenter`].
    ///
    /// This is an alias for [`Self::new_with_locale`] with a more ergonomic name.
    /// The locale string is accepted for API symmetry and documentation but the
    /// compiled CLDR data already covers all supported scripts, so the result
    /// is equivalent to [`Self::new`] for all locales.
    ///
    /// # Errors
    ///
    /// Always succeeds; the `Result` wrapper exists for forwards compatibility.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::{IcuSegmenter, SegmentKind};
    ///
    /// let seg = IcuSegmenter::with_locale("ja").expect("Japanese segmenter");
    /// let words = seg.segment_strs("東京都は日本の首都です", SegmentKind::Word);
    /// assert!(words.len() >= 2);
    /// ```
    pub fn with_locale(locale: &str) -> Result<Self, crate::CollateError> {
        Self::new_with_locale(locale)
    }

    /// Returns byte offsets of all word boundaries in `text`.
    ///
    /// These are the CLDR UAX #29 word-break opportunities from
    /// [`SegmentKind::Word`].  Useful for text selection, double-click selection,
    /// and word-wrap layout.
    ///
    /// Results are memoised via the internal break-point cache.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuSegmenter;
    ///
    /// let seg = IcuSegmenter::new();
    /// let bps = seg.word_boundaries("Hello world");
    /// assert!(!bps.is_empty());
    /// ```
    pub fn word_boundaries(&self, text: &str) -> Vec<usize> {
        self.break_points(text, SegmentKind::Word)
    }

    /// Returns byte offsets where line breaks are permitted in `text`.
    ///
    /// These are the CLDR UAX #14 line-break opportunities from
    /// [`SegmentKind::Line`].  Use them as a replacement for the
    /// `unicode-linebreak` crate in the layout engine.
    ///
    /// Results are memoised via the internal break-point cache.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuSegmenter;
    ///
    /// let seg = IcuSegmenter::new();
    /// let ops = seg.line_break_opportunities("Hello world test");
    /// assert!(!ops.is_empty());
    /// ```
    pub fn line_break_opportunities(&self, text: &str) -> Vec<usize> {
        self.break_points(text, SegmentKind::Line)
    }

    /// Returns word boundary offsets suitable for line-breaking in CJK/Thai text.
    ///
    /// For CJK and Thai, dictionary-based word segmentation determines where
    /// lines can break. This method returns byte offsets of word boundaries from
    /// the Word segmenter, which uses LSTM/dictionary for languages that require
    /// it.
    ///
    /// For CJK text, every word boundary is a valid line break position. The
    /// result is deduplicated and omits zero-length spans.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuSegmenter;
    ///
    /// let seg = IcuSegmenter::new();
    /// let breaks = seg.cjk_line_break_opportunities("東京都は日本の首都");
    /// assert!(!breaks.is_empty());
    /// ```
    pub fn cjk_line_break_opportunities(&self, text: &str) -> Vec<usize> {
        let breaks = self.break_points(text, SegmentKind::Word);
        // Deduplicate adjacent equal offsets and drop any zero-length spans.
        let mut out = Vec::with_capacity(breaks.len());
        let mut prev = usize::MAX;
        for b in breaks {
            if b != prev {
                out.push(b);
                prev = b;
            }
        }
        out
    }

    /// Determine if `text` contains characters that require dictionary-based
    /// segmentation (Thai, Khmer, Lao, Myanmar, Japanese, Chinese without
    /// spaces, Korean Hangul).
    ///
    /// Returns `true` if any character in the text belongs to one of these
    /// script ranges and therefore benefits from LSTM/dictionary segmentation
    /// rather than rule-based word breaking.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuSegmenter;
    ///
    /// assert!(IcuSegmenter::needs_dictionary_segmentation("สวัสดี"));
    /// assert!(IcuSegmenter::needs_dictionary_segmentation("東京"));
    /// assert!(!IcuSegmenter::needs_dictionary_segmentation("Hello world"));
    /// ```
    pub fn needs_dictionary_segmentation(text: &str) -> bool {
        text.chars().any(|c| {
            matches!(
                c as u32,
                0x0E00..=0x0E7F   // Thai
                | 0x1780..=0x17FF // Khmer
                | 0x0E80..=0x0EFF // Lao
                | 0x1000..=0x109F // Myanmar
                | 0x3000..=0x9FFF // CJK + Hiragana + Katakana
                | 0xAC00..=0xD7AF // Korean Hangul
            )
        })
    }
}

/// A lazy iterator over [`Segment`] values produced from a fixed input string.
///
/// Produced by [`IcuSegmenter::iter_segments`]. Avoids collecting all segments
/// into a `Vec` when only a subset of the output is needed.
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::{IcuSegmenter, SegmentKind};
///
/// let seg = IcuSegmenter::new();
/// let words: Vec<_> = seg
///     .iter_segments("hello world", SegmentKind::Word)
///     .filter(|s| !s.text.trim().is_empty())
///     .collect();
/// assert!(words.iter().any(|s| s.text == "hello"));
/// ```
pub struct SegmentIter {
    text: String,
    kind: SegmentKind,
    breaks: std::vec::IntoIter<usize>,
    prev: usize,
}

impl Iterator for SegmentIter {
    type Item = Segment;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let end = self.breaks.next()?;
            // Skip degenerate boundaries (zero-length or out-of-order).
            if end <= self.prev || end > self.text.len() {
                continue;
            }
            // Only slice at valid UTF-8 boundaries.
            let text_slice = self.text.get(self.prev..end)?;
            let seg = Segment {
                text: text_slice.to_owned(),
                byte_start: self.prev,
                byte_end: end,
                kind: self.kind,
            };
            self.prev = end;
            return Some(seg);
        }
    }
}

impl IcuSegmenter {
    /// Returns a lazy iterator over [`Segment`] values for the given [`SegmentKind`].
    ///
    /// Unlike [`Self::segments`] which eagerly collects all segments into a `Vec`,
    /// this returns an iterator that produces segments on demand. Use it when you
    /// only need to scan part of the output (e.g. stop at the first word boundary).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::{IcuSegmenter, SegmentKind};
    ///
    /// let seg = IcuSegmenter::new();
    /// let mut iter = seg.iter_segments("hello world", SegmentKind::Word);
    /// // Consume only as many segments as needed.
    /// let first = iter.next();
    /// assert!(first.is_some());
    /// ```
    pub fn iter_segments(&self, text: &str, kind: SegmentKind) -> SegmentIter {
        let breaks = self.break_points(text, kind);
        SegmentIter {
            text: text.to_owned(),
            kind,
            breaks: breaks.into_iter(),
            prev: 0,
        }
    }
}

impl Default for IcuSegmenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── segment_strs (former `segments`) ──────────────────────────────────────

    #[test]
    fn grapheme_cluster_combining_char() {
        // "e\u{0301}" is e + combining acute accent = 1 grapheme cluster.
        let seg = IcuSegmenter::new();
        let clusters = seg.segment_strs("e\u{0301}", SegmentKind::GraphemeCluster);
        assert_eq!(
            clusters.len(),
            1,
            "e + combining accent should be 1 grapheme cluster, got: {clusters:?}"
        );
    }

    #[test]
    fn word_segmentation_basic() {
        let seg = IcuSegmenter::new();
        // "hello world" should produce at least 2 word segments.
        let words = seg.segment_strs("hello world", SegmentKind::Word);
        let non_space: Vec<&&str> = words.iter().filter(|s| !s.trim().is_empty()).collect();
        assert!(
            non_space.len() >= 2,
            "expected at least 2 words, got: {words:?}"
        );
    }

    #[test]
    fn sentence_segmentation_basic() {
        let seg = IcuSegmenter::new();
        let sentences = seg.segment_strs("Hello world. Goodbye world.", SegmentKind::Sentence);
        assert!(
            sentences.len() >= 2,
            "expected at least 2 sentences, got: {sentences:?}"
        );
    }

    #[test]
    fn line_break_cjk() {
        // CJK characters can break between any two consecutive characters.
        let seg = IcuSegmenter::new();
        // "東京" = 2 CJK characters; line-break segmenter should allow a break between them.
        let breaks = seg.segment_strs("東京", SegmentKind::Line);
        assert!(
            !breaks.is_empty(),
            "CJK text should have at least one line segment"
        );
    }

    #[test]
    fn grapheme_cluster_emoji_zwj() {
        // A family emoji sequence joined by ZWJ should be 1 grapheme cluster.
        // "👨‍👩‍👧" = man ZWJ woman ZWJ girl → 1 grapheme cluster
        let seg = IcuSegmenter::new();
        let text = "👨\u{200D}👩\u{200D}👧";
        let clusters = seg.segment_strs(text, SegmentKind::GraphemeCluster);
        // ICU4X should recognize this as a single extended grapheme cluster.
        assert_eq!(
            clusters.len(),
            1,
            "ZWJ family emoji should be 1 grapheme cluster: {clusters:?}"
        );
    }

    #[test]
    fn break_points_includes_end() {
        // break_points should always include text.len() as the last element.
        let seg = IcuSegmenter::new();
        let text = "hello";
        let breaks = seg.break_points(text, SegmentKind::GraphemeCluster);
        assert!(
            breaks.contains(&text.len()),
            "break_points must include end offset: {breaks:?}"
        );
    }

    #[test]
    fn segment_strs_cover_full_text() {
        // The concatenation of all segment_strs must equal the input string.
        let seg = IcuSegmenter::new();
        let text = "Hello, 世界! Goodbye.";
        for kind in [
            SegmentKind::GraphemeCluster,
            SegmentKind::Line,
            SegmentKind::Word,
        ] {
            let segs = seg.segment_strs(text, kind);
            let rejoined: String = segs.concat();
            assert_eq!(
                rejoined, text,
                "segment_strs for {kind:?} do not cover full text"
            );
        }
    }

    #[test]
    fn word_segmentation_japanese() {
        // Japanese text: the word segmenter (auto/LSTM) should split this
        // into at least 2 segments.
        let seg = IcuSegmenter::new();
        let text = "東京都は日本の首都です";
        let words = seg.segment_strs(text, SegmentKind::Word);
        assert!(
            words.len() >= 2,
            "Japanese word segmentation should produce ≥2 segments, got: {words:?}"
        );
    }

    // ── segments() → Vec<Segment> ─────────────────────────────────────────────

    #[test]
    fn word_segments_english() {
        let seg = IcuSegmenter::new();
        let segs = seg.segments("hello world", SegmentKind::Word);
        // Should produce segments covering "hello" and "world".
        let words: Vec<&str> = segs
            .iter()
            .filter(|s| !s.text.trim().is_empty())
            .map(|s| s.text.as_str())
            .collect();
        assert!(words.contains(&"hello"), "should segment 'hello'");
        assert!(words.contains(&"world"), "should segment 'world'");

        // Verify byte offsets are valid, non-overlapping, and match text.
        let mut prev_end = 0;
        let input = "hello world";
        for s in &segs {
            assert!(
                s.byte_start >= prev_end,
                "overlapping segments: byte_start {} < prev_end {}",
                s.byte_start,
                prev_end
            );
            assert!(
                s.byte_end <= input.len(),
                "byte_end {} exceeds text length {}",
                s.byte_end,
                input.len()
            );
            assert_eq!(
                &input[s.byte_start..s.byte_end],
                s.text.as_str(),
                "byte range does not match text"
            );
            prev_end = s.byte_end;
        }
    }

    #[test]
    fn thai_word_segmentation() {
        // Thai text: "สวัสดีชาวโลก" (sawatdee chao lok — Hello world)
        let seg = IcuSegmenter::new();
        let text = "สวัสดีชาวโลก";
        let segs = seg.segments(text, SegmentKind::Word);
        assert!(
            !segs.is_empty(),
            "Thai text should segment into at least one segment"
        );
        // All byte spans must cover the full string.
        let total: usize = segs.iter().map(|s| s.byte_end - s.byte_start).sum();
        assert_eq!(total, text.len(), "segments must cover entire Thai string");
    }

    #[test]
    fn japanese_word_segmentation_rich() {
        let seg = IcuSegmenter::new();
        let text = "日本語テスト";
        let segs = seg.segments(text, SegmentKind::Word);
        assert!(!segs.is_empty());
        let total: usize = segs.iter().map(|s| s.byte_end - s.byte_start).sum();
        assert_eq!(
            total,
            text.len(),
            "segments must cover entire Japanese string"
        );
    }

    #[test]
    fn sentence_segmentation_with_abbreviation() {
        let seg = IcuSegmenter::new();
        let text = "Dr. Smith went home. He was tired.";
        let segs = seg.segments(text, SegmentKind::Sentence);
        assert!(!segs.is_empty(), "should produce at least one sentence");
        let total: usize = segs.iter().map(|s| s.byte_end - s.byte_start).sum();
        assert_eq!(total, text.len(), "sentences must cover entire string");
        // ICU should produce 1–3 sentences (Dr. abbreviation handling varies by data version).
        assert!(
            !segs.is_empty() && segs.len() <= 3,
            "expected 1–3 sentences, got {}",
            segs.len()
        );
    }

    #[test]
    fn segments_byte_offsets_are_valid() {
        // Rich Segment::byte_start/byte_end must index valid UTF-8 boundaries.
        let seg = IcuSegmenter::new();
        let text = "Hello, 世界! Goodbye.";
        for kind in [
            SegmentKind::GraphemeCluster,
            SegmentKind::Line,
            SegmentKind::Word,
        ] {
            let segs = seg.segments(text, kind);
            let rejoined: String = segs.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(
                rejoined, text,
                "segments for {kind:?} do not cover full text"
            );
            for s in &segs {
                assert_eq!(
                    &text[s.byte_start..s.byte_end],
                    s.text.as_str(),
                    "byte range mismatch in {kind:?} segment"
                );
            }
        }
    }

    #[test]
    fn grapheme_kind_alias_matches_grapheme_cluster() {
        // SegmentKind::Grapheme must produce the same text / byte offsets as
        // SegmentKind::GraphemeCluster (only the stored `kind` field differs).
        let seg = IcuSegmenter::new();
        let text = "e\u{0301}á";
        let gc = seg.segments(text, SegmentKind::GraphemeCluster);
        let g = seg.segments(text, SegmentKind::Grapheme);
        assert_eq!(
            gc.len(),
            g.len(),
            "Grapheme and GraphemeCluster must produce the same number of segments"
        );
        for (a, b) in gc.iter().zip(g.iter()) {
            assert_eq!(
                a.text, b.text,
                "Grapheme and GraphemeCluster segment text must match"
            );
            assert_eq!(
                a.byte_start, b.byte_start,
                "Grapheme and GraphemeCluster byte_start must match"
            );
            assert_eq!(
                a.byte_end, b.byte_end,
                "Grapheme and GraphemeCluster byte_end must match"
            );
        }
    }

    // ── SegmentIter (lazy iteration) ──────────────────────────────────────────

    #[test]
    fn iter_segments_matches_segments_eager() {
        // iter_segments must produce the same output as the eager segments() call.
        let seg = IcuSegmenter::new();
        let text = "hello world";
        let eager = seg.segments(text, SegmentKind::Word);
        let lazy: Vec<_> = seg.iter_segments(text, SegmentKind::Word).collect();
        assert_eq!(
            eager.len(),
            lazy.len(),
            "iter_segments and segments must produce the same number of segments"
        );
        for (a, b) in eager.iter().zip(lazy.iter()) {
            assert_eq!(a.text, b.text, "text mismatch");
            assert_eq!(a.byte_start, b.byte_start, "byte_start mismatch");
            assert_eq!(a.byte_end, b.byte_end, "byte_end mismatch");
        }
    }

    #[test]
    fn iter_segments_lazy_stops_early() {
        // Verify the iterator can be consumed partially without forcing all items.
        let seg = IcuSegmenter::new();
        let text = "one two three four five";
        let mut iter = seg.iter_segments(text, SegmentKind::Word);
        let first = iter.next();
        assert!(first.is_some(), "should yield at least one segment");
        // Only consume a few; remaining segments are not evaluated.
        let _second = iter.next();
    }

    #[test]
    fn iter_segments_grapheme_clusters() {
        let seg = IcuSegmenter::new();
        let text = "e\u{0301}á";
        let lazy: Vec<_> = seg
            .iter_segments(text, SegmentKind::GraphemeCluster)
            .collect();
        // Should be 2 grapheme clusters.
        assert_eq!(lazy.len(), 2, "expected 2 grapheme clusters: {lazy:?}");
        // Total byte coverage must equal string length.
        let total: usize = lazy.iter().map(|s| s.byte_end - s.byte_start).sum();
        assert_eq!(total, text.len(), "iter_segments must cover full string");
    }

    #[test]
    fn iter_segments_empty_text() {
        let seg = IcuSegmenter::new();
        let mut iter = seg.iter_segments("", SegmentKind::Word);
        assert!(iter.next().is_none(), "empty text should yield no segments");
    }

    // ── with_locale ──────────────────────────────────────────────────────────

    #[test]
    fn with_locale_succeeds_for_common_locales() {
        for locale in &["en", "ja", "th", "zh", "ar", "ko"] {
            IcuSegmenter::with_locale(locale)
                .unwrap_or_else(|e| panic!("with_locale({locale}) failed: {e}"));
        }
    }

    #[test]
    fn with_locale_japanese_word_segmentation() {
        let seg = IcuSegmenter::with_locale("ja").expect("Japanese segmenter");
        let text = "東京都は日本の首都です";
        let words = seg.segment_strs(text, SegmentKind::Word);
        assert!(
            words.len() >= 2,
            "Japanese should produce ≥2 segments: {words:?}"
        );
    }

    // ── Cache ────────────────────────────────────────────────────────────────

    #[test]
    fn test_segmenter_cache_hit() {
        let seg = IcuSegmenter::new();
        let text = "Hello world test";
        let r1 = seg.break_points(text, SegmentKind::Word);
        let r2 = seg.break_points(text, SegmentKind::Word);
        assert_eq!(r1, r2, "cached result should match original");
    }

    #[test]
    fn cache_hit_immutable_ref() {
        // Verify the cache works via &self (not &mut self).
        let seg = IcuSegmenter::new();
        let text = "Hello world test";
        let r1 = seg.break_points(text, SegmentKind::Word);
        let r2 = seg.break_points(text, SegmentKind::Word);
        assert_eq!(r1, r2, "immutable-ref cache hit should match");
    }

    #[test]
    fn segment_cache_hit() {
        let seg = IcuSegmenter::new();
        let text = "Hello world test";
        let r1 = seg.segments(text, SegmentKind::Word);
        let r2 = seg.segments(text, SegmentKind::Word);
        assert_eq!(r1, r2, "segment cached result should match original");
    }

    // ── word_boundaries / line_break_opportunities ───────────────────────────

    #[test]
    fn test_word_boundaries() {
        let seg = IcuSegmenter::new();
        let bps = seg.word_boundaries("Hello world");
        // Should have at least one boundary.
        assert!(!bps.is_empty());
    }

    #[test]
    fn test_line_break_opportunities() {
        let seg = IcuSegmenter::new();
        let ops = seg.line_break_opportunities("Hello world test");
        // Should have break opportunities at word boundaries.
        assert!(!ops.is_empty());
    }

    #[test]
    fn word_boundaries_matches_break_points() {
        let seg = IcuSegmenter::new();
        let text = "one two three";
        let wb = seg.word_boundaries(text);
        let bp = seg.break_points(text, SegmentKind::Word);
        assert_eq!(
            wb, bp,
            "word_boundaries should delegate to break_points(Word)"
        );
    }

    #[test]
    fn line_break_opportunities_matches_break_points() {
        let seg = IcuSegmenter::new();
        let text = "one two three";
        let lbo = seg.line_break_opportunities(text);
        let bp = seg.break_points(text, SegmentKind::Line);
        assert_eq!(
            lbo, bp,
            "line_break_opportunities should delegate to break_points(Line)"
        );
    }

    // ── Thai word segmentation ───────────────────────────────────────────────

    #[test]
    fn test_thai_word_segmentation() {
        // Thai text: "สวัสดีชาวโลก" (hello world in Thai)
        // This tests that dictionary-based word breaking works.
        let seg = IcuSegmenter::new();
        let text = "สวัสดีชาวโลก";
        let words = seg.segments(text, SegmentKind::Word);
        let non_space: Vec<_> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
        assert!(
            non_space.len() >= 2,
            "Thai should produce at least 2 word segments, got: {words:?}"
        );
    }

    // ── cjk_line_break_opportunities / needs_dictionary_segmentation ─────────

    #[test]
    fn test_cjk_line_break_opportunities_japanese() {
        let seg = IcuSegmenter::new();
        let breaks = seg.cjk_line_break_opportunities("東京都は日本の首都");
        assert!(!breaks.is_empty(), "CJK text must have break opportunities");
    }

    #[test]
    fn test_cjk_line_break_opportunities_deduplication() {
        let seg = IcuSegmenter::new();
        let breaks = seg.cjk_line_break_opportunities("hello world");
        // Verify no adjacent duplicates.
        let has_dup = breaks.windows(2).any(|w| w[0] == w[1]);
        assert!(
            !has_dup,
            "cjk_line_break_opportunities must not contain adjacent duplicates"
        );
    }

    #[test]
    fn test_needs_dictionary_segmentation_thai() {
        assert!(IcuSegmenter::needs_dictionary_segmentation("สวัสดี"));
    }

    #[test]
    fn test_needs_dictionary_segmentation_cjk() {
        assert!(IcuSegmenter::needs_dictionary_segmentation("東京"));
    }

    #[test]
    fn test_needs_dictionary_segmentation_latin_false() {
        assert!(!IcuSegmenter::needs_dictionary_segmentation("Hello world"));
    }

    // ── Performance benchmarks (ignored by default) ──────────────────────────

    #[test]
    #[ignore = "benchmark — run with: cargo test -p oxitext-icu -- --ignored bench_segmentation"]
    fn bench_segmentation_100k_chars() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(2300); // ~100K chars
        let seg = IcuSegmenter::new();
        let start = std::time::Instant::now();
        for _ in 0..10 {
            let _ = seg.break_points(&text, SegmentKind::Word);
        }
        let elapsed = start.elapsed();
        println!("10× 100K-char word segmentation: {:?}", elapsed);
        println!("Per-call: {:?}", elapsed / 10);
    }

    #[test]
    #[ignore = "benchmark — run with: cargo test -p oxitext-icu -- --ignored bench_collation"]
    fn bench_collation_sort_10k_strings() {
        use crate::IcuCollator;
        let strings: Vec<String> = (0..10_000).map(|i| format!("item_{i:05}")).collect();
        let collator = IcuCollator::new("en").expect("collator");
        let start = std::time::Instant::now();
        let mut sorted = strings.clone();
        sorted.sort_by(|a, b| collator.compare(a.as_str(), b.as_str()));
        println!("Sort 10K strings: {:?}", start.elapsed());
        assert!(sorted[0] <= sorted[sorted.len() - 1]);
    }
}
