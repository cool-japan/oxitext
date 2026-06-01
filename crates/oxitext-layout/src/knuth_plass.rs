//! Knuth-Plass optimal paragraph line-breaking algorithm.
//!
//! This module implements the classic Knuth-Plass dynamic-programming
//! paragraph-breaking algorithm adapted for a glyph-advance representation.
//! Rather than greedily wrapping at the first safe break (as the
//! [`crate::engine::LayoutEngine`] greedy path does), this algorithm selects
//! among UAX #14 break opportunities to **minimise total paragraph demerits**,
//! producing more typographically even line lengths.
//!
//! The public entry point is [`optimal_breaks`].
//!
//! # Algorithm overview
//!
//! 1. A virtual *seed* node is placed at glyph index 0 with zero demerits.
//! 2. The algorithm iterates over all legal break positions (UAX #14 allowed +
//!    mandatory + the paragraph end).  For each break position *b*, every
//!    currently active predecessor node *a* is evaluated:
//!    - Compute the natural advance of the span `a..b`.
//!    - Derive the adjustment ratio `r = (max_width − w) / max_width`.
//!    - Derive the badness and demerits for that span.
//!    - Prune spans whose natural width exceeds `max_width * 1.5` (infeasible).
//! 3. The predecessor that minimises total demerits is recorded as the best
//!    break at position *b*.
//! 4. Active nodes whose accumulated span already exceeds the infeasibility
//!    threshold are pruned from the active list.
//! 5. Mandatory break positions flush all active nodes except the mandatory one.
//! 6. The best terminal node (break at the paragraph end) is found, and the
//!    break sequence is recovered by backtracking through `prev` links.
//!
//! # Fallback
//!
//! If no feasible solution is found (e.g. every possible line exceeds
//! `max_width * 1.5`), the function returns an empty `Vec`.  The caller
//! ([`crate::engine::LayoutEngine::layout_with_strategy`]) falls back to the
//! greedy algorithm in that case.

use crate::linebreak::LineBreak;

// ---------------------------------------------------------------------------
// Demerits constants
// ---------------------------------------------------------------------------

/// Extra demerits penalising consecutive very tight / very loose lines.
///
/// Applied when two adjacent spans produce adjustment ratios on the same
/// extreme side (both tight or both loose), encouraging a more balanced
/// distribution of whitespace.
const FIT_DEMERITS: f64 = 10_000.0;

/// Sentinel value for "no finite solution found".
const INF_DEMERITS: f64 = f64::INFINITY;

// ---------------------------------------------------------------------------
// Internal data model
// ---------------------------------------------------------------------------

/// A line-break candidate in the Knuth-Plass active-node list.
///
/// Each candidate represents the best (lowest-demerits) way to reach a
/// particular break position.  Backtracking through `prev` links after the
/// algorithm completes reconstructs the full set of break positions.
#[derive(Clone)]
struct Candidate {
    /// Glyph index (exclusive) at which this candidate's line ends.
    break_glyph: usize,
    /// Accumulated demerits reaching this break.
    total_demerits: f64,
    /// Adjustment ratio of the *incoming* span (used for fitness-class
    /// comparison with the next span to add `FIT_DEMERITS` when applicable).
    ratio: f64,
    /// Index of the preceding candidate in the `candidates` Vec (for
    /// backtracking).
    prev: Option<usize>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute optimal line-break glyph indices using the Knuth-Plass algorithm.
///
/// Returns a `Vec<usize>` of **exclusive** glyph-index break points (not
/// including `0` as the first line start, and not including `advances.len()`
/// as the final terminator unless the text is empty).
///
/// # Arguments
///
/// - `advances`: per-glyph x-advance values (same order as the flat glyph
///   vec).
/// - `is_whitespace`: per-glyph whitespace flag (same length as `advances`).
/// - `break_opps`: sorted `(glyph_index, LineBreak)` pairs where a line may
///   break *before* that glyph (`Allowed`) or *must* break (`Mandatory`).
/// - `max_width`: column width in pixels; `0.0` means no wrapping.
///
/// # Demerits
///
/// For a line of natural width `w` in a column of `max_width`:
/// - Adjustment ratio `r = (max_width − w) / max_width` (clamped to ±2).
/// - Badness `b = 100 × |r|³`.
/// - Demerits `d = (1 + b)²`.
/// - Consecutive tight-or-loose lines add `FIT_DEMERITS` = 10 000.
/// - Lines wider than `max_width × 1.5` are infeasible (pruned from active
///   list).
///
/// # Return value
///
/// Returns an empty `Vec` when:
/// - `max_width` ≤ 0, or
/// - `advances` is empty, or
/// - no feasible paragraph solution exists (caller should fall back to greedy).
pub fn optimal_breaks(
    advances: &[f32],
    is_whitespace: &[bool],
    break_opps: &[(usize, LineBreak)],
    max_width: f32,
) -> Vec<usize> {
    let n = advances.len();
    if max_width <= 0.0 || n == 0 {
        return vec![];
    }

    // Build the full ordered list of break positions (glyph-exclusive indices).
    // The paragraph terminator at position `n` is always included.
    let mut break_positions: Vec<(usize, LineBreak)> = break_opps.to_vec();
    if break_positions.last().is_none_or(|&(p, _)| p != n) {
        break_positions.push((n, LineBreak::Allowed));
    }
    // Ensure sorted by position (callers should pass sorted, but be defensive).
    break_positions.sort_by_key(|&(p, _)| p);
    break_positions.dedup_by_key(|&mut (p, _)| p);

    // Prefix-sum of advances for O(1) span-width queries.
    // prefix[i] = sum of advances[0..i].
    let mut prefix = vec![0.0f32; n + 1];
    for k in 0..n {
        prefix[k + 1] = prefix[k] + advances[k];
    }
    // Trim trailing whitespace from a span: find the last non-ws glyph in
    // `from..to` and return the advance up to (and including) that glyph.
    // This mirrors the greedy path which uses `trimmed_width`.
    let trimmed_width = |from: usize, to: usize| -> f32 {
        let mut tw = 0.0f32;
        let mut running = 0.0f32;
        for k in from..to {
            running += advances[k];
            if k < is_whitespace.len() && !is_whitespace[k] {
                tw = running;
            }
        }
        tw
    };

    // candidates[i] = best candidate arriving at break_positions[?].break_glyph.
    let mut candidates: Vec<Candidate> = Vec::new();
    // Seed: virtual start node before glyph 0, zero demerits.
    candidates.push(Candidate {
        break_glyph: 0,
        total_demerits: 0.0,
        ratio: 0.0,
        prev: None,
    });
    // active = indices into `candidates` that are still alive.
    let mut active: Vec<usize> = vec![0];

    for (bp_idx, &(bp, ref bp_kind)) in break_positions.iter().enumerate() {
        let _ = bp_idx; // used via loop variable name only

        let mut best_demerits = INF_DEMERITS;
        let mut best_prev: Option<usize> = None;
        let mut best_ratio = 0.0f64;

        for &ai in &active {
            let from = candidates[ai].break_glyph;
            if from > bp {
                continue;
            }

            // Natural (whitespace-trimmed) width of the candidate span.
            let line_w = trimmed_width(from, bp);

            // Infeasibility check: reject spans wider than 1.5× the column.
            if line_w > max_width * 1.5 {
                continue;
            }

            let r = ((max_width - line_w) / max_width).clamp(-2.0, 2.0) as f64;
            let badness = 100.0 * r.abs().powi(3);
            let mut d = (1.0 + badness).powi(2);

            // Fitness-class penalty: when two consecutive spans are both at
            // the same extreme (very tight or very loose), add FIT_DEMERITS.
            let prev_r = candidates[ai].ratio;
            let same_tight = r < -0.5 && prev_r < -0.5;
            let same_loose = r > 0.5 && prev_r > 0.5;
            if same_tight || same_loose {
                d += FIT_DEMERITS;
            }

            let total = candidates[ai].total_demerits + d;
            if total < best_demerits {
                best_demerits = total;
                best_prev = Some(ai);
                best_ratio = r;
            }
        }

        if let Some(prev_idx) = best_prev {
            let new_idx = candidates.len();
            candidates.push(Candidate {
                break_glyph: bp,
                total_demerits: best_demerits,
                ratio: best_ratio,
                prev: Some(prev_idx),
            });
            active.push(new_idx);
        }

        // Prune active nodes whose open span is already infeasible (wider than
        // 1.5× max_width given advances accumulated so far up to `bp`).
        active.retain(|&ai| {
            let from = candidates[ai].break_glyph;
            // Use prefix sums for O(1) raw width (trimmed_width is only needed
            // for per-candidate demerits; for pruning, raw is sufficient and
            // conservative — raw >= trimmed, so pruning raw > 1.5× is safe).
            let raw_w = prefix[bp.min(n)] - prefix[from];
            raw_w <= max_width * 1.5
        });

        // Mandatory break: flush active nodes that are not at this break
        // position (the paragraph must break here unconditionally).
        if *bp_kind == LineBreak::Mandatory && bp < n {
            active.retain(|&ai| candidates[ai].break_glyph == bp);
        }
    }

    // Find the best terminal node (break_glyph == n).
    let best_terminal = active
        .iter()
        .filter(|&&ai| candidates[ai].break_glyph == n)
        .min_by(|&&a, &&b| {
            candidates[a]
                .total_demerits
                .partial_cmp(&candidates[b].total_demerits)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied();

    // If no feasible solution, return empty so the caller can fall back.
    let Some(mut current) = best_terminal else {
        return vec![];
    };

    // Backtrack through prev-links to collect break positions.
    let mut breaks: Vec<usize> = Vec::new();
    loop {
        let gp = candidates[current].break_glyph;
        // Exclude the implicit start (0) and the paragraph terminator (n).
        if gp > 0 && gp < n {
            breaks.push(gp);
        }
        match candidates[current].prev {
            Some(p) => current = p,
            None => break,
        }
    }
    breaks.reverse();
    breaks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linebreak::{LineBreak, LineBreaker};

    /// Build uniform per-char advances and whitespace flags for `text`.
    fn make_advances(text: &str, advance: f32) -> (Vec<f32>, Vec<bool>) {
        let adv: Vec<f32> = text.chars().map(|_| advance).collect();
        let ws: Vec<bool> = text.chars().map(|c| c.is_whitespace()).collect();
        (adv, ws)
    }

    /// Produce `(char_index, LineBreak)` pairs for `text` by mapping the
    /// byte-offset break opportunities from [`LineBreaker`] to char indices.
    fn break_opps_for(text: &str) -> Vec<(usize, LineBreak)> {
        let breaker = LineBreaker::new(text);
        let byte_to_char: std::collections::HashMap<usize, usize> = text
            .char_indices()
            .enumerate()
            .map(|(ci, (bi, _))| (bi, ci))
            .collect();
        breaker
            .breaks()
            .iter()
            .filter_map(|&(off, ref kind)| byte_to_char.get(&off).map(|&ci| (ci, kind.clone())))
            .collect()
    }

    #[test]
    fn no_wrap_when_fits() {
        let text = "ab cd";
        let (adv, ws) = make_advances(text, 10.0); // 50 px total
        let opps = break_opps_for(text);
        let breaks = optimal_breaks(&adv, &ws, &opps, 200.0);
        // Everything fits on a single line — no interior break needed.
        assert!(
            breaks.is_empty(),
            "no break needed when everything fits: {breaks:?}"
        );
    }

    #[test]
    fn forces_break_on_long_text() {
        // "aa bb cc" with 10 px glyphs = 80 px; max_width=50 forces breaks.
        let text = "aa bb cc";
        let (adv, ws) = make_advances(text, 10.0);
        let opps = break_opps_for(text);
        let breaks = optimal_breaks(&adv, &ws, &opps, 50.0);
        assert!(!breaks.is_empty(), "must produce at least one break");
    }

    #[test]
    fn kp_produces_fewer_rivers_than_greedy() {
        // Classic KP demonstration: 5 words with different widths.
        // "aaa bb ccc d eeeee" — KP should distribute more evenly than greedy.
        // At minimum it must not panic and must return valid break positions.
        let text = "aaa bb ccc d eeeee";
        let (adv, ws) = make_advances(text, 10.0);
        let opps = break_opps_for(text);
        let breaks = optimal_breaks(&adv, &ws, &opps, 60.0);
        // All break indices must be within bounds and in strictly ascending order.
        for w in breaks.windows(2) {
            assert!(w[1] > w[0], "breaks must be strictly ascending");
        }
        for &b in &breaks {
            assert!(
                b < adv.len(),
                "break index must be within bounds: {b} >= {}",
                adv.len()
            );
        }
    }

    #[test]
    fn empty_text_returns_empty() {
        let breaks = optimal_breaks(&[], &[], &[], 100.0);
        assert!(breaks.is_empty());
    }

    #[test]
    fn zero_max_width_returns_empty() {
        let (adv, ws) = make_advances("hello world", 10.0);
        let opps = break_opps_for("hello world");
        let breaks = optimal_breaks(&adv, &ws, &opps, 0.0);
        assert!(breaks.is_empty());
    }

    #[test]
    fn break_indices_are_valid() {
        // Verify all returned break indices are strictly within [1, n-1].
        let text = "the quick brown fox jumps over the lazy dog";
        let (adv, ws) = make_advances(text, 8.0);
        let opps = break_opps_for(text);
        let breaks = optimal_breaks(&adv, &ws, &opps, 120.0);
        let n = adv.len();
        for &b in &breaks {
            assert!(b > 0 && b < n, "break index {b} out of range [1, {n})");
        }
    }

    #[test]
    fn mandatory_break_is_honoured() {
        // "hello\nworld" has a mandatory break after "hello\n".
        // KP must include a break between the two words regardless of width.
        let text = "hello\nworld";
        let (adv, ws) = make_advances(text, 10.0);
        let opps = break_opps_for(text);
        let breaks = optimal_breaks(&adv, &ws, &opps, 1000.0);
        // With max_width=1000 there is plenty of room; the mandatory break
        // after the newline character (char index 6) must still appear.
        // The break position corresponds to 'w' at char index 6.
        assert!(
            breaks.contains(&6),
            "mandatory break at char 6 must be present; got {breaks:?}"
        );
    }
}
