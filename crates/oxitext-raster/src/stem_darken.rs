//! FreeType-style stem darkening for improved readability at small sizes.
//!
//! At small pixel sizes (ppem < 7), thin strokes can disappear in greyscale
//! rasterization.  Stem darkening boosts dark regions of coverage to make
//! strokes appear thicker and more legible.
//!
//! The amount decreases linearly with ppem and reaches zero at ppem ≥ 7,
//! mirroring the FreeType stem-darkening heuristic.

/// Compute the FreeType-style stem darkening boost amount for `ppem`.
///
/// Returns a value in `[0.0, 0.5]` that decreases linearly with ppem,
/// reaching 0.0 at ppem ≥ 7 (where `0.4375 − 0.0625 × 7 = 0.0`).
///
/// # Examples
///
/// ```
/// use oxitext_raster::stem_darken::stem_darkening_amount;
/// assert_eq!(stem_darkening_amount(7.0), 0.0);
/// assert_eq!(stem_darkening_amount(32.0), 0.0);
/// ```
pub fn stem_darkening_amount(ppem: f32) -> f32 {
    (0.4375 - 0.0625 * ppem).clamp(0.0, 0.5)
}

/// Apply stem darkening to a mutable slice of coverage values in `[0.0, 1.0]`.
///
/// Each coverage value is boosted toward `1.0` proportionally to `amount`
/// scaled by the *dark* fraction `(1 − v)`.  The result is clamped to `[0, 1]`.
///
/// # Arguments
///
/// * `coverage` — mutable slice of linear coverage values.
/// * `amount` — darkening strength, typically from [`stem_darkening_amount`].
pub fn apply_stem_darkening(coverage: &mut [f32], amount: f32) {
    for v in coverage.iter_mut() {
        *v = (*v + amount * (1.0 - *v)).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decreasing_with_ppem() {
        assert!(stem_darkening_amount(6.0) > stem_darkening_amount(7.0));
        assert_eq!(stem_darkening_amount(7.0), 0.0);
        assert_eq!(stem_darkening_amount(32.0), 0.0);
    }

    #[test]
    fn boosts_coverage() {
        let mut cov = vec![0.5f32];
        let before = cov[0];
        apply_stem_darkening(&mut cov, 0.1);
        assert!(cov[0] > before);
    }

    #[test]
    fn clamps_coverage_to_one() {
        let mut cov = vec![1.0f32];
        apply_stem_darkening(&mut cov, 0.5);
        assert_eq!(cov[0], 1.0);
    }

    #[test]
    fn zero_amount_is_noop() {
        let mut cov = vec![0.5f32, 0.3f32];
        let before = cov.clone();
        apply_stem_darkening(&mut cov, 0.0);
        assert_eq!(cov, before);
    }
}
