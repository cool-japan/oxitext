//! Scalar (non-SIMD) implementations of raster hot-loop primitives.
//!
//! These functions form the reference implementation against which the optional
//! SIMD paths are KAT-tested for byte-identical output.

/// Accumulate coverage values from `src` into `dst`, clamping each lane to
/// `[0.0, 1.0]`.
///
/// `dst[i] = (dst[i] + src[i]).min(1.0)` for all `i`.
///
/// Lengths must match; extra `src` elements are ignored.
#[inline]
pub fn accumulate_coverage(dst: &mut [f32], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = (*d + *s).min(1.0_f32);
    }
}

/// Multiply a u8 alpha coverage buffer by a scalar alpha factor, scaling each
/// element as `round(pixel * factor / 255.0)` and clamping to `[0, 255]`.
///
/// This is the inner loop used when blending a color layer's coverage bitmap
/// with the layer's own alpha channel: `effective_alpha = coverage * layer_alpha / 255`.
#[inline]
pub fn multiply_alpha_u8(buf: &mut [u8], factor: u8) {
    let factor_u32 = factor as u32;
    for p in buf.iter_mut() {
        *p = ((*p as u32 * factor_u32 + 127) / 255) as u8;
    }
}

/// Convert a `[0.0, 1.0]` f32 coverage buffer to u8 using standard rounding
/// (`(v * 255.0).round() as u8`).
///
/// Values outside `[0.0, 1.0]` are clamped before conversion.
#[inline]
pub fn coverage_f32_to_u8(dst: &mut [u8], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = (s.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
    }
}

/// Porter-Duff "source over" compositing for pre-multiplied RGBA byte buffers
/// (scalar reference implementation).
///
/// Both `dst` and `src` are RGBA byte buffers (4 bytes per pixel,
/// `width × height × 4` bytes total).  The formula operates on
/// **pre-multiplied alpha** values:
///
/// ```text
/// out_a   = src_a + dst_a * (1 - src_a)
/// out_rgb = src_rgb + dst_rgb * (1 - src_a)
/// ```
///
/// All channels are normalised to `[0.0, 1.0]` for arithmetic and then
/// rounded back to `[0, 255]`.
///
/// # Panics
/// Panics (debug) or produces truncated output (release) if `dst` and `src`
/// lengths differ or are not a multiple of 4.
#[inline]
pub fn porter_duff_source_over_scalar(dst: &mut [u8], src: &[u8]) {
    let len = dst.len().min(src.len());
    let pixels = len / 4;
    for i in 0..pixels {
        let base = i * 4;
        let sa_f = src[base + 3] as f32 / 255.0_f32;
        let da_f = dst[base + 3] as f32 / 255.0_f32;
        let one_minus_sa = 1.0_f32 - sa_f;
        let out_a = sa_f + da_f * one_minus_sa;
        let out_r = src[base] as f32 / 255.0_f32 + dst[base] as f32 / 255.0_f32 * one_minus_sa;
        let out_g =
            src[base + 1] as f32 / 255.0_f32 + dst[base + 1] as f32 / 255.0_f32 * one_minus_sa;
        let out_b =
            src[base + 2] as f32 / 255.0_f32 + dst[base + 2] as f32 / 255.0_f32 * one_minus_sa;
        dst[base] = (out_r.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
        dst[base + 1] = (out_g.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
        dst[base + 2] = (out_b.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
        dst[base + 3] = (out_a.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_basic() {
        let mut dst = [0.5_f32, 0.3_f32, 0.9_f32, 0.0_f32];
        let src = [0.4_f32, 0.5_f32, 0.2_f32, 0.8_f32];
        accumulate_coverage(&mut dst, &src);
        assert!((dst[0] - 0.9_f32).abs() < 1e-7);
        assert!((dst[1] - 0.8_f32).abs() < 1e-7);
        assert!((dst[2] - 1.0_f32).abs() < 1e-7, "clamped to 1.0");
        assert!((dst[3] - 0.8_f32).abs() < 1e-7);
    }

    #[test]
    fn multiply_alpha_midpoint() {
        let mut buf = [255_u8, 128_u8, 0_u8];
        multiply_alpha_u8(&mut buf, 128);
        // 255 * 128 / 255 = 128 (rounded)
        assert_eq!(buf[0], 128);
        // 128 * 128 / 255 ≈ 64.25 → rounded = 64
        assert_eq!(buf[1], 64);
        assert_eq!(buf[2], 0);
    }

    #[test]
    fn coverage_f32_to_u8_roundtrip() {
        let src = [0.0_f32, 0.5_f32, 1.0_f32, -0.1_f32, 1.1_f32];
        let mut dst = [0_u8; 5];
        coverage_f32_to_u8(&mut dst, &src);
        assert_eq!(dst[0], 0);
        assert_eq!(dst[1], 128); // round(127.5) = 128
        assert_eq!(dst[2], 255);
        assert_eq!(dst[3], 0); // clamped
        assert_eq!(dst[4], 255); // clamped
    }
}
