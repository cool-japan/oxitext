//! Tate-chu-yoko: horizontal runs within vertical text.
//!
//! Implements CSS `text-combine-upright` semantics: detects short runs of
//! horizontal-script characters (ASCII digits, ASCII letters) inside a
//! vertical CJK text flow, and marks them as combined units that should be
//! drawn rotated 90° so they occupy a single vertical cell.
//!
//! # Background
//!
//! In vertical Japanese and Chinese layout, sequences of one to four ASCII
//! digits or short Latin characters are conventionally rotated so that they
//! read horizontally within the vertical line.  Examples: years ("2024"),
//! page numbers, short measurements.  The combined unit occupies the same
//! block-direction advance as a single CJK character (1 em).
//!
//! # Interface
//!
//! [`detect_runs`] takes a slice of [`GlyphEntry`] values — lightweight
//! descriptors that pair each [`PositionedGlyph`] with the Unicode codepoint
//! it represents.  Callers build this from their shaping pipeline's cluster
//! mapping; tests construct it directly.
//!
//! # References
//!
//! - CSS Writing Modes Level 4 §9.1 "text-combine-upright"
//! - Unicode UAX #50 "Unicode Vertical Text Layout"

use oxitext_core::PositionedGlyph;

/// The maximum number of glyphs that may form a tate-chu-yoko run.
///
/// CSS Writing Modes Level 4 limits `text-combine-upright: digits` to 1–2
/// digits; the general `all` value allows up to 4 characters.  We use 4 as
/// the cap here.
pub const MAX_TCY_RUN_LEN: usize = 4;

/// A positioned glyph paired with its source codepoint.
///
/// The codepoint is needed to decide whether the glyph is eligible for
/// tate-chu-yoko (TCY) treatment.  [`PositionedGlyph`] itself does not carry
/// cluster/codepoint information, so callers must supply it explicitly.
#[derive(Debug, Clone)]
pub struct GlyphEntry<'a> {
    /// Reference to the positioned glyph.
    pub glyph: &'a PositionedGlyph,
    /// The Unicode codepoint that this glyph represents.
    pub codepoint: char,
}

impl<'a> GlyphEntry<'a> {
    /// Constructs a new [`GlyphEntry`].
    pub fn new(glyph: &'a PositionedGlyph, codepoint: char) -> Self {
        Self { glyph, codepoint }
    }
}

/// A detected tate-chu-yoko run inside a vertical text flow.
///
/// The glyphs in `[start_glyph, end_glyph)` should be drawn horizontally
/// (rotated 90° counter-clockwise relative to the vertical baseline) as a
/// single combined unit.  The `combined_advance` gives the block-direction
/// (vertical) advance that the whole run consumes — typically 1 em.
#[derive(Debug, Clone, PartialEq)]
pub struct TateChuYokoRun {
    /// Index of the first glyph in the run (inclusive).
    pub start_glyph: usize,
    /// Index past the last glyph in the run (exclusive).
    pub end_glyph: usize,
    /// Block-direction (vertical) advance consumed by the entire combined run,
    /// in the same pixel units as `PositionedGlyph::pos`.
    pub combined_advance: f32,
}

/// Returns `true` if `c` is eligible for tate-chu-yoko treatment.
///
/// Eligible characters are ASCII digits and ASCII letters.
fn is_tcy_char(c: char) -> bool {
    matches!(c,
        // ASCII digits
        '0'..='9'
        // ASCII uppercase letters
        | 'A'..='Z'
        // ASCII lowercase letters
        | 'a'..='z'
    )
}

/// Detects tate-chu-yoko candidate runs in a sequence of glyph entries.
///
/// Scans `entries` for consecutive subsequences where every glyph's codepoint
/// is an ASCII digit or ASCII letter.  Runs of length 1 to [`MAX_TCY_RUN_LEN`]
/// are returned as [`TateChuYokoRun`] values; longer runs are truncated at the
/// cap so they do not overflow the 1-em vertical cell.
///
/// The `combined_advance` for each run is computed as the vertical span from
/// the first glyph's y-position to the last glyph's y-position plus one
/// `em_size`, so the run occupies a full em of vertical space.  For single-glyph
/// runs the combined advance equals `em_size`.
///
/// # Parameters
///
/// - `entries` — glyph entries to scan (each entry pairs a positioned glyph
///   with its source codepoint).
/// - `em_size` — the font em size in pixels; used as the vertical advance for
///   single-glyph runs and as the final-cell contribution for multi-glyph runs.
///
/// # Examples
///
/// ```rust
/// use oxitext_layout::tate_chu_yoko::{detect_runs, GlyphEntry};
/// use oxitext_core::PositionedGlyph;
/// use std::sync::Arc;
///
/// let font: Arc<[u8]> = Arc::from(&[][..]);
/// let g0 = PositionedGlyph { gid: 19, font_data: Arc::clone(&font), pos: (0.0, 0.0), font_size: 16.0, advance_x: 16.0, cluster: 0 };
/// let g1 = PositionedGlyph { gid: 20, font_data: Arc::clone(&font), pos: (0.0, 16.0), font_size: 16.0, advance_x: 16.0, cluster: 1 };
/// let g2 = PositionedGlyph { gid: 1,  font_data: Arc::clone(&font), pos: (0.0, 32.0), font_size: 16.0, advance_x: 16.0, cluster: 2 };
///
/// let entries = [
///     GlyphEntry::new(&g0, '2'),
///     GlyphEntry::new(&g1, '4'),
///     GlyphEntry::new(&g2, '日'),  // CJK — not TCY
/// ];
/// let runs = detect_runs(&entries, 16.0);
/// assert_eq!(runs.len(), 1);
/// assert_eq!(runs[0].start_glyph, 0);
/// assert_eq!(runs[0].end_glyph, 2);
/// ```
pub fn detect_runs(entries: &[GlyphEntry<'_>], em_size: f32) -> Vec<TateChuYokoRun> {
    let mut runs = Vec::new();
    let mut i = 0;

    while i < entries.len() {
        let run_start = i;
        let mut j = i;

        // Extend the run while chars are TCY-eligible and run stays within cap.
        while j < entries.len()
            && (j - run_start) < MAX_TCY_RUN_LEN
            && is_tcy_char(entries[j].codepoint)
        {
            j += 1;
        }

        let run_len = j - run_start;
        if run_len >= 1 {
            let combined_advance = if run_len == 1 {
                em_size
            } else {
                // Span from first glyph y to last glyph y, plus one em for the
                // final cell.
                let y_first = entries[run_start].glyph.pos.1;
                let y_last = entries[run_start + run_len - 1].glyph.pos.1;
                let span = (y_last - y_first).abs();
                if span > 0.0 {
                    span + em_size
                } else {
                    em_size
                }
            };

            runs.push(TateChuYokoRun {
                start_glyph: run_start,
                end_glyph: run_start + run_len,
                combined_advance,
            });
            i = run_start + run_len;
        } else {
            // No TCY char at position i — skip it.
            i += 1;
        }
    }

    runs
}

/// Returns the block-direction (vertical) advance for a tate-chu-yoko run by
/// querying the font's `vmtx` table for the first glyph in the run.
///
/// The `vmtx` table stores per-glyph vertical advances in font design units.
/// These are scaled to pixels by `advance * em_size / units_per_em`.
///
/// # Fallback
///
/// If `face_data` cannot be parsed, or if the face has no `vmtx` table, or if
/// no entry exists for the requested glyph, the function returns `em_size`
/// unchanged.  This is the same value that [`detect_runs`] uses by default,
/// so callers do not need to special-case the fallback.
///
/// # Parameters
///
/// - `face_data` — raw font bytes (the same `Arc<[u8]>` stored on
///   [`oxitext_core::PositionedGlyph`]).
/// - `glyph_ids` — glyph IDs for the run (only the first is consulted).
/// - `em_size` — target em size in pixels.
/// - `units_per_em` — font's design units per em (from the `head` table).
///
/// # Examples
///
/// ```rust
/// use oxitext_layout::tate_chu_yoko::tcy_combined_advance;
///
/// // With no real font data the function falls back to em_size.
/// let adv = tcy_combined_advance(&[], &[1], 16.0, 1000);
/// assert!((adv - 16.0).abs() < f32::EPSILON);
/// ```
pub fn tcy_combined_advance(
    face_data: &[u8],
    glyph_ids: &[u16],
    em_size: f32,
    units_per_em: u16,
) -> f32 {
    let first_gid = match glyph_ids.first() {
        Some(&id) => id,
        None => return em_size,
    };

    if units_per_em == 0 {
        return em_size;
    }

    crate::vertical::vmtx_advance_for_glyph(face_data, first_gid, em_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxitext_core::PositionedGlyph;
    use std::sync::Arc;

    fn pg(y: f32) -> PositionedGlyph {
        PositionedGlyph {
            gid: 1,
            font_data: Arc::from(&[][..]),
            pos: (0.0, y),
            font_size: 16.0,
            advance_x: 16.0,
            cluster: 0,
        }
    }

    #[test]
    fn detects_two_digit_run() {
        let g0 = pg(0.0);
        let g1 = pg(16.0);
        let g2 = pg(32.0);
        let entries = [
            GlyphEntry::new(&g0, '2'),
            GlyphEntry::new(&g1, '4'),
            GlyphEntry::new(&g2, '日'), // CJK — not TCY
        ];
        let runs = detect_runs(&entries, 16.0);
        assert_eq!(runs.len(), 1, "expected exactly one TCY run");
        let run = &runs[0];
        assert_eq!(run.start_glyph, 0);
        assert_eq!(run.end_glyph, 2, "run should cover the 2 digit glyphs");
        assert!(
            run.combined_advance > 0.0,
            "combined_advance must be positive"
        );
    }

    #[test]
    fn single_digit_run() {
        let g0 = pg(0.0);
        let g1 = pg(16.0);
        let entries = [GlyphEntry::new(&g0, '5'), GlyphEntry::new(&g1, '日')];
        let runs = detect_runs(&entries, 16.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start_glyph, 0);
        assert_eq!(runs[0].end_glyph, 1);
    }

    #[test]
    fn no_run_for_cjk_only() {
        let g0 = pg(0.0);
        let g1 = pg(16.0);
        let g2 = pg(32.0);
        let entries = [
            GlyphEntry::new(&g0, '日'),
            GlyphEntry::new(&g1, '本'),
            GlyphEntry::new(&g2, '語'),
        ];
        let runs = detect_runs(&entries, 16.0);
        assert_eq!(runs.len(), 0, "CJK-only text should yield no TCY runs");
    }

    #[test]
    fn run_capped_at_max() {
        // 5-character ASCII run — first 4 form a run, 5th starts a new run.
        let glyphs: Vec<PositionedGlyph> = (0..6).map(|i| pg(i as f32 * 16.0)).collect();
        let chars = ['H', 'e', 'l', 'l', 'o', '日'];
        let entries: Vec<GlyphEntry<'_>> = glyphs
            .iter()
            .zip(chars.iter())
            .map(|(g, &c)| GlyphEntry::new(g, c))
            .collect();
        let runs = detect_runs(&entries, 16.0);
        // First run: H,e,l,l (4) — capped at MAX_TCY_RUN_LEN
        assert!(!runs.is_empty());
        assert_eq!(runs[0].end_glyph - runs[0].start_glyph, MAX_TCY_RUN_LEN);
        // Second run: 'o' alone
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].start_glyph, 4);
        assert_eq!(runs[1].end_glyph, 5);
    }

    #[test]
    fn empty_entries_yields_no_runs() {
        let runs = detect_runs(&[], 16.0);
        assert!(runs.is_empty());
    }

    #[test]
    fn combined_advance_uses_em_for_single_glyph() {
        let em = 20.0_f32;
        let g0 = pg(0.0);
        let g1 = pg(em);
        let entries = [GlyphEntry::new(&g0, '9'), GlyphEntry::new(&g1, '日')];
        let runs = detect_runs(&entries, em);
        assert_eq!(runs.len(), 1);
        assert!(
            (runs[0].combined_advance - em).abs() < f32::EPSILON,
            "single-glyph run should have combined_advance == em_size"
        );
    }

    #[test]
    fn ascii_letters_are_tcy() {
        let g0 = pg(0.0);
        let g1 = pg(16.0);
        let entries = [GlyphEntry::new(&g0, 'A'), GlyphEntry::new(&g1, 'B')];
        let runs = detect_runs(&entries, 16.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].end_glyph - runs[0].start_glyph, 2);
    }

    #[test]
    fn mixed_cjk_digits_cjk() {
        // Pattern: CJK, digit, digit, CJK — should produce one run in the middle.
        let glyphs: Vec<PositionedGlyph> = (0..4).map(|i| pg(i as f32 * 16.0)).collect();
        let chars = ['日', '2', '3', '本'];
        let entries: Vec<GlyphEntry<'_>> = glyphs
            .iter()
            .zip(chars.iter())
            .map(|(g, &c)| GlyphEntry::new(g, c))
            .collect();
        let runs = detect_runs(&entries, 16.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start_glyph, 1);
        assert_eq!(runs[0].end_glyph, 3);
    }

    #[test]
    fn detects_2024_in_vertical_cjk() {
        // "縦書き2024年" — the '2', '0', '2', '4' chars should be detected as a
        // TCY run while the CJK characters around them are not.
        //
        // We build GlyphEntry slices from the individual chars of this string.
        let text = "縦書き2024年";
        let chars: Vec<char> = text.chars().collect();
        // Each char gets a PositionedGlyph at successive y positions (vertical).
        let glyphs: Vec<PositionedGlyph> = chars
            .iter()
            .enumerate()
            .map(|(i, _)| pg(i as f32 * 16.0))
            .collect();
        let entries: Vec<GlyphEntry<'_>> = glyphs
            .iter()
            .zip(chars.iter())
            .map(|(g, &c)| GlyphEntry::new(g, c))
            .collect();

        let runs = detect_runs(&entries, 16.0);

        // Find all codepoints that fall inside any TCY run.
        let tcy_chars: Vec<char> = runs
            .iter()
            .flat_map(|r| chars[r.start_glyph..r.end_glyph].iter().copied())
            .collect();

        let tcy_str: String = tcy_chars.iter().collect();
        assert!(
            tcy_str.contains("2024"),
            "Expected '2024' to be covered by a TCY run, got: {tcy_str:?}"
        );

        // CJK characters must NOT appear in any TCY run.
        assert!(
            !tcy_chars.contains(&'縦'),
            "CJK char '縦' must not be in a TCY run"
        );
        assert!(
            !tcy_chars.contains(&'書'),
            "CJK char '書' must not be in a TCY run"
        );
        assert!(
            !tcy_chars.contains(&'年'),
            "CJK char '年' must not be in a TCY run"
        );
    }
}
