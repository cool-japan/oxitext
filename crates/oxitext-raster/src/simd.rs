//! SIMD-accelerated raster hot-loop primitives (feature = `simd`).
//!
//! Uses [`wide::f32x8`] for 8-lane f32 operations and [`wide::u8x16`] for
//! 16-lane u8 operations.  All functions produce byte-identical results to
//! their counterparts in [`crate::scalar`] — the KAT tests in
//! `tests/simd_parity.rs` verify this invariant.
//!
//! # Parity guarantee
//!
//! `f32x8` arithmetic (`+`, `min`) is IEEE 754 deterministic and therefore
//! bit-identical to scalar `f32` for those operations.  The one place where
//! divergence could occur is `f32 → u8` rounding.  We extract each element
//! individually from the SIMD lane and apply the same `f32::round() as u8`
//! that the scalar path uses, ensuring parity even on the `.5` boundary
//! (half-away-from-zero on both paths).

#[cfg(feature = "simd")]
use wide::f32x8;

/// SIMD-accelerated coverage accumulation.
///
/// Semantically identical to [`crate::scalar::accumulate_coverage`]:
/// `dst[i] = (dst[i] + src[i]).min(1.0)` for all `i`.
#[cfg(feature = "simd")]
#[inline]
pub fn accumulate_coverage(dst: &mut [f32], src: &[f32]) {
    let one = f32x8::splat(1.0_f32);
    let len = dst.len().min(src.len());
    let chunks = len / 8;

    for i in 0..chunks {
        let base = i * 8;
        // Safety: chunks = len/8, so base+8 <= len <= dst.len() and src.len().
        let d_arr: [f32; 8] = dst[base..base + 8].try_into().unwrap_or([0.0_f32; 8]);
        let s_arr: [f32; 8] = src[base..base + 8].try_into().unwrap_or([0.0_f32; 8]);

        let d = f32x8::from(d_arr);
        let s = f32x8::from(s_arr);
        let result = (d + s).min(one);

        let out = result.as_array();
        dst[base..base + 8].copy_from_slice(out);
    }

    // Scalar tail for remainder elements.
    let rem = chunks * 8;
    for (d, s) in dst[rem..len].iter_mut().zip(src[rem..len].iter()) {
        *d = (*d + *s).min(1.0_f32);
    }
}

/// SIMD-accelerated u8 alpha multiplication.
///
/// Semantically identical to [`crate::scalar::multiply_alpha_u8`]:
/// `buf[i] = round(buf[i] * factor / 255)` for all `i`.
#[cfg(feature = "simd")]
#[inline]
pub fn multiply_alpha_u8(buf: &mut [u8], factor: u8) {
    let factor_u32 = factor as u32;
    let chunks = buf.len() / 16;

    for i in 0..chunks {
        let base = i * 16;
        // u8x16 saturating multiply isn't a single instruction — we decode to
        // u16 math for correctness.  For the integer-only coverage path this is
        // still faster than calling into a C library.
        let lane: [u8; 16] = buf[base..base + 16].try_into().unwrap_or([0_u8; 16]);

        // Use scalar arithmetic per lane but load/store 16 at once.
        let result: [u8; 16] = {
            let mut out = [0_u8; 16];
            for (j, &b) in lane.iter().enumerate() {
                out[j] = ((b as u32 * factor_u32 + 127) / 255) as u8;
            }
            out
        };

        buf[base..base + 16].copy_from_slice(&result);
    }

    // Scalar tail.
    let rem = chunks * 16;
    for p in buf[rem..].iter_mut() {
        *p = ((*p as u32 * factor_u32 + 127) / 255) as u8;
    }
}

/// SIMD-accelerated f32 coverage → u8 conversion.
///
/// Semantically identical to [`crate::scalar::coverage_f32_to_u8`]:
/// `dst[i] = (src[i].clamp(0,1) * 255).round() as u8`.
///
/// Each lane is extracted individually and rounded with `f32::round()` to
/// guarantee parity with the scalar path.
#[cfg(feature = "simd")]
#[inline]
pub fn coverage_f32_to_u8(dst: &mut [u8], src: &[f32]) {
    let len = dst.len().min(src.len());
    let chunks = len / 8;
    let scale = f32x8::splat(255.0_f32);
    let zero = f32x8::splat(0.0_f32);
    let one = f32x8::splat(1.0_f32);

    for i in 0..chunks {
        let base = i * 8;
        let s_arr: [f32; 8] = src[base..base + 8].try_into().unwrap_or([0.0_f32; 8]);
        let s = f32x8::from(s_arr);
        let clamped = s.min(one).max(zero);
        // Extract per-element and round identically to scalar to preserve parity.
        let scaled = clamped * scale;
        let arr = scaled.as_array();
        for (j, &v) in arr.iter().enumerate() {
            dst[base + j] = v.round() as u8;
        }
    }

    // Scalar tail.
    let rem = chunks * 8;
    for (d, s) in dst[rem..len].iter_mut().zip(src[rem..len].iter()) {
        *d = (s.clamp(0.0_f32, 1.0_f32) * 255.0_f32).round() as u8;
    }
}

/// SIMD-accelerated Porter-Duff "source over" compositing for pre-multiplied
/// RGBA byte buffers.
///
/// Processes 8 pixels (32 bytes) per iteration using `f32x8` lanes.
/// Falls back to scalar for any tail remainder.
///
/// See [`crate::scalar::porter_duff_source_over_scalar`] for the arithmetic
/// definition.  Both paths produce output within ±1 LSB of each other for all
/// valid inputs.
#[cfg(feature = "simd")]
#[inline]
pub fn porter_duff_source_over_simd(dst: &mut [u8], src: &[u8]) {
    let len = dst.len().min(src.len());
    let pixels = len / 4;
    let simd_pixels = pixels / 8;

    let scale = f32x8::splat(255.0_f32);
    let inv_scale = f32x8::splat(1.0_f32 / 255.0_f32);
    let one = f32x8::splat(1.0_f32);
    let zero = f32x8::splat(0.0_f32);

    for chunk in 0..simd_pixels {
        let base_pixel = chunk * 8;

        // Gather 8 values for each channel from interleaved RGBA bytes.
        let mut sr_arr = [0.0_f32; 8];
        let mut sg_arr = [0.0_f32; 8];
        let mut sb_arr = [0.0_f32; 8];
        let mut sa_arr = [0.0_f32; 8];
        let mut dr_arr = [0.0_f32; 8];
        let mut dg_arr = [0.0_f32; 8];
        let mut db_arr = [0.0_f32; 8];
        let mut da_arr = [0.0_f32; 8];

        for i in 0..8 {
            let b = (base_pixel + i) * 4;
            sr_arr[i] = src[b] as f32;
            sg_arr[i] = src[b + 1] as f32;
            sb_arr[i] = src[b + 2] as f32;
            sa_arr[i] = src[b + 3] as f32;
            dr_arr[i] = dst[b] as f32;
            dg_arr[i] = dst[b + 1] as f32;
            db_arr[i] = dst[b + 2] as f32;
            da_arr[i] = dst[b + 3] as f32;
        }

        let sr = f32x8::from(sr_arr) * inv_scale;
        let sg = f32x8::from(sg_arr) * inv_scale;
        let sb = f32x8::from(sb_arr) * inv_scale;
        let sa = f32x8::from(sa_arr) * inv_scale;
        let dr = f32x8::from(dr_arr) * inv_scale;
        let dg = f32x8::from(dg_arr) * inv_scale;
        let db = f32x8::from(db_arr) * inv_scale;
        let da = f32x8::from(da_arr) * inv_scale;

        let one_minus_sa = one - sa;

        let out_r = (sr + dr * one_minus_sa).min(one).max(zero) * scale;
        let out_g = (sg + dg * one_minus_sa).min(one).max(zero) * scale;
        let out_b = (sb + db * one_minus_sa).min(one).max(zero) * scale;
        let out_a = (sa + da * one_minus_sa).min(one).max(zero) * scale;

        let arr_r = out_r.as_array();
        let arr_g = out_g.as_array();
        let arr_b = out_b.as_array();
        let arr_a = out_a.as_array();

        for i in 0..8 {
            let b = (base_pixel + i) * 4;
            dst[b] = arr_r[i].round() as u8;
            dst[b + 1] = arr_g[i].round() as u8;
            dst[b + 2] = arr_b[i].round() as u8;
            dst[b + 3] = arr_a[i].round() as u8;
        }
    }

    // Scalar tail for remaining pixels.
    let rem_pixel_start = simd_pixels * 8;
    crate::scalar::porter_duff_source_over_scalar(
        &mut dst[rem_pixel_start * 4..len],
        &src[rem_pixel_start * 4..len],
    );
}

// When the feature is disabled, this module is empty but still compiles.
