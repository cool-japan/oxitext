//! Gamma correction LUTs for sRGB ↔ linear conversion.
//!
//! Implements the IEC 61966-2-1 sRGB transfer function in both directions
//! as pre-computed look-up tables for fast per-byte conversion.
//!
//! - [`srgb_to_linear`]: convert a single sRGB byte to linear `f32` via a
//!   256-entry LUT.
//! - [`linear_to_srgb`]: convert a linear `f32` in `[0, 1]` to an sRGB byte
//!   via a 4096-entry LUT.

use std::sync::LazyLock;

/// 256-entry LUT: sRGB byte value (0..=255) → linear light intensity (0.0..=1.0).
///
/// Built once at first access using the IEC 61966-2-1 transfer function.
static SRGB_TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut lut = [0.0f32; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let c = i as f32 / 255.0;
        *entry = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
    }
    lut
});

/// 4096-entry LUT: quantised linear intensity → sRGB byte value (0..=255).
///
/// Built once at first access using the inverse IEC 61966-2-1 transfer function.
/// Index `i` corresponds to linear value `i / 4095.0`.
static LINEAR_TO_SRGB: LazyLock<[u8; 4096]> = LazyLock::new(|| {
    let mut lut = [0u8; 4096];
    for (i, entry) in lut.iter_mut().enumerate() {
        let linear = i as f32 / 4095.0;
        let srgb = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        *entry = (srgb.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    lut
});

/// Convert an sRGB byte `v` (0..=255) to a linear light intensity (`0.0..=1.0`).
///
/// Uses a 256-entry pre-computed LUT for constant-time lookup.
#[inline]
pub fn srgb_to_linear(v: u8) -> f32 {
    SRGB_TO_LINEAR[v as usize]
}

/// Convert a linear light intensity `v` (clamped to `[0, 1]`) to an sRGB byte.
///
/// Uses a 4096-entry pre-computed LUT for fast approximate conversion.
#[inline]
pub fn linear_to_srgb(v: f32) -> u8 {
    LINEAR_TO_SRGB[(v.clamp(0.0, 1.0) * 4095.0).round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_linear_round_trip() {
        for v in 0u8..=255 {
            let l = srgb_to_linear(v);
            let back = linear_to_srgb(l);
            assert!(
                (back as i32 - v as i32).abs() <= 1,
                "round-trip failed for {v}: linear={l}, back={back}"
            );
        }
    }

    #[test]
    fn extremes() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 0.001);
        assert_eq!(linear_to_srgb(0.0), 0);
        assert_eq!(linear_to_srgb(1.0), 255);
    }
}
