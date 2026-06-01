//! UAX #9 Unicode Bidirectional Algorithm.
//!
//! Wraps the `unicode-bidi` crate to provide paragraph-level bidi analysis,
//! resolving visual run order and embedding levels for mixed-direction text.

use unicode_bidi::{BidiInfo, Level};

/// A run of text with a uniform bidi embedding level.
///
/// Bidi runs are produced by [`BidiParagraph::new`] and represent contiguous
/// slices of the source string that share the same embedding level.  Even-
/// numbered levels are LTR; odd-numbered levels are RTL.
#[derive(Debug, Clone)]
pub struct BidiRun {
    /// Byte-offset start of this run in the source string (inclusive).
    pub start: usize,
    /// Byte-offset end of this run in the source string (exclusive).
    pub end: usize,
    /// UAX #9 embedding level.  Level 0 = LTR paragraph base, 1 = RTL, etc.
    pub level: u8,
}

/// Result of paragraph-level bidi analysis.
///
/// Call [`BidiParagraph::new`] to analyse a string, then inspect
/// [`BidiParagraph::runs`] for the visual-order run sequence.
pub struct BidiParagraph {
    runs: Vec<BidiRun>,
    base_level: u8,
    /// Per-byte embedding levels for the source text, indexed by UTF-8 byte offset.
    levels: Vec<Level>,
}

impl BidiParagraph {
    /// Analyse a paragraph for bidi runs.
    ///
    /// The `base_rtl` argument controls the paragraph base direction:
    /// - `None`  — auto-detect via UAX #9 rules P2/P3 (recommended).
    /// - `Some(true)`  — force RTL base direction.
    /// - `Some(false)` — force LTR base direction.
    ///
    /// The returned runs are in *visual* order (as they would appear on screen),
    /// not logical order.
    pub fn new(text: &str, base_rtl: Option<bool>) -> Self {
        let hint = match base_rtl {
            Some(true) => Some(Level::rtl()),
            Some(false) => Some(Level::ltr()),
            // Pass None to let BidiInfo apply P2/P3 auto-detection.
            None => None,
        };

        let bidi = BidiInfo::new(text, hint);

        // Clone per-byte levels before consuming `bidi` in the run-collecting loop.
        let levels = bidi.levels.clone();

        // Collect visual-order runs from every paragraph in the text.
        let mut runs: Vec<BidiRun> = Vec::new();
        for para in &bidi.paragraphs {
            let para_range = para.range.start..para.range.end;
            let (_run_levels, run_ranges) = bidi.visual_runs(para, para_range);
            for run_range in run_ranges {
                // Use the byte-level embedding level at the run's start position.
                // `bidi.levels` is guaranteed to be indexed by UTF-8 byte offset,
                // and run boundaries always fall on character boundaries.
                let level = if run_range.start < bidi.levels.len() {
                    bidi.levels[run_range.start].number()
                } else {
                    para.level.number()
                };
                runs.push(BidiRun {
                    start: run_range.start,
                    end: run_range.end,
                    level,
                });
            }
        }

        // Resolve the base level from the first paragraph (or default to LTR).
        let base_level = bidi
            .paragraphs
            .first()
            .map(|p| p.level.number())
            .unwrap_or(0);

        BidiParagraph {
            runs,
            base_level,
            levels,
        }
    }

    /// Returns the resolved visual-order bidi runs.
    pub fn runs(&self) -> &[BidiRun] {
        &self.runs
    }

    /// Returns the resolved paragraph base embedding level.
    pub fn base_level(&self) -> u8 {
        self.base_level
    }

    /// Returns `true` if the paragraph base direction is RTL (odd base level).
    pub fn is_rtl(&self) -> bool {
        self.base_level % 2 == 1
    }

    /// Returns the per-byte UAX #9 embedding levels for the source text.
    ///
    /// The returned slice is indexed by UTF-8 byte offset. Multi-byte characters
    /// have their level repeated for each byte.  Use the cluster byte offset from
    /// a [`oxitext_core::ShapedGlyph`] to look up the level for that glyph.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltr_paragraph_base_level_is_even() {
        let para = BidiParagraph::new("hello", Some(false));
        assert!(!para.is_rtl(), "LTR forced base should not be RTL");
    }

    #[test]
    fn rtl_forced_base_level_is_odd() {
        let para = BidiParagraph::new("hello", Some(true));
        assert!(para.is_rtl(), "RTL forced base should be RTL");
    }
}
