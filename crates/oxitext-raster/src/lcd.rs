//! LCD subpixel rendering pipeline.
//!
//! Implements the full FreeType-inspired LCD subpixel rendering pipeline:
//!
//! 1. Rasterize the glyph outline at 3× horizontal resolution using `ab_glyph`.
//! 2. Optionally apply FreeType-style stem darkening (boosts thin strokes).
//! 3. Apply a 1-D FIR horizontal convolution (Box, Triangle, or FreeType 5-tap).
//! 4. Decimate by 3 horizontally and pack into an [`LcdBitmap`].
//!
//! The `ab_glyph` rasterizer outputs linear coverage values, so no sRGB
//! conversion is needed before filtering.  Callers that need per-pixel
//! gamma-corrected blending should use the LUTs in [`crate::gamma`].

use crate::options::LcdFilterKernel;
use crate::stem_darken::{apply_stem_darkening, stem_darkening_amount};
use oxitext_core::LcdBitmap;

/// Return the normalised FIR filter kernel for `kind`.
fn fir_kernel(kind: LcdFilterKernel) -> &'static [f32] {
    match kind {
        LcdFilterKernel::Box => &[1.0_f32 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
        LcdFilterKernel::Triangle => &[1.0_f32 / 4.0, 1.0 / 2.0, 1.0 / 4.0],
        LcdFilterKernel::FreeType5Tap => {
            &[1.0_f32 / 9.0, 2.0 / 9.0, 3.0 / 9.0, 2.0 / 9.0, 1.0 / 9.0]
        }
    }
}

/// Apply a 1-D FIR horizontal convolution in-place across every row of `buf`.
///
/// `width` and `height` describe the layout of `buf` (row-major, `width × height`
/// entries).  The kernel is applied symmetrically; samples that fall outside the
/// row boundary are simply omitted (border handling: zero-pad conceptually).
fn fir_horizontal(buf: &mut [f32], width: usize, height: usize, kernel: &[f32]) {
    let half = kernel.len() / 2;
    let mut tmp = vec![0.0f32; buf.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (k, &kv) in kernel.iter().enumerate() {
                let xi = x as isize + k as isize - half as isize;
                if xi >= 0 && (xi as usize) < width {
                    sum += buf[y * width + xi as usize] * kv;
                }
            }
            tmp[y * width + x] = sum;
        }
    }
    buf.copy_from_slice(&tmp);
}

/// Rasterize a glyph with horizontal LCD subpixel rendering.
///
/// Returns `None` if the font cannot be parsed, the glyph has no outline
/// (e.g. whitespace), or the resulting output width is zero.
///
/// # Algorithm
///
/// 1. The glyph is rasterized at `PxScale { x: px_size * 3, y: px_size }` to
///    obtain three coverage samples per output pixel column.
/// 2. Optional stem darkening boosts coverage in the thin-stroke regime.
/// 3. A 1-D FIR convolution blends adjacent samples to produce RGB weights.
/// 4. Every three consecutive samples are decimated into one RGB output pixel.
///
/// # Arguments
///
/// * `face_data` — raw TTF/OTF font bytes.
/// * `glyph_id` — glyph index within the font.
/// * `px_size` — target pixels-per-em for the *y* axis.
/// * `filter` — FIR kernel to apply before decimation.
/// * `stem_darkening_enabled` — whether to apply stem darkening.
pub fn rasterize_lcd(
    face_data: &[u8],
    glyph_id: u16,
    px_size: f32,
    filter: LcdFilterKernel,
    stem_darkening_enabled: bool,
) -> Option<LcdBitmap> {
    use ab_glyph::{Font, FontRef, GlyphId as AbGlyphId, PxScale};

    let font = FontRef::try_from_slice(face_data).ok()?;
    let ab_gid = AbGlyphId(glyph_id);

    // Render at 3× horizontal resolution for subpixel decomposition.
    let scale = PxScale {
        x: px_size * 3.0,
        y: px_size,
    };
    let glyph = ab_gid.with_scale(scale);
    let outlined = font.outline_glyph(glyph)?;

    let bounds = outlined.px_bounds();
    let hi_width = bounds.width().ceil() as usize;
    let height = bounds.height().ceil() as usize;
    if hi_width == 0 || height == 0 {
        return None;
    }

    // Collect linear coverage values (ab_glyph outputs linear [0, 1] directly).
    let mut coverage = vec![0.0f32; hi_width * height];
    outlined.draw(|x, y, c| {
        let idx = y as usize * hi_width + x as usize;
        if idx < coverage.len() {
            coverage[idx] = c;
        }
    });

    // Apply FreeType-style stem darkening (only meaningful at small ppem).
    if stem_darkening_enabled {
        let amount = stem_darkening_amount(px_size);
        apply_stem_darkening(&mut coverage, amount);
    }

    // Apply FIR horizontal convolution to blend subpixel samples.
    let kernel = fir_kernel(filter);
    fir_horizontal(&mut coverage, hi_width, height, kernel);

    // Decimate by 3 horizontally: three consecutive samples → one RGB pixel.
    let out_width = hi_width / 3;
    if out_width == 0 {
        return None;
    }

    let mut rgb = vec![0u8; out_width * height * 3];
    for y in 0..height {
        for x in 0..out_width {
            let r = (coverage[y * hi_width + x * 3].clamp(0.0, 1.0) * 255.0).round() as u8;
            let g = (coverage[y * hi_width + x * 3 + 1].clamp(0.0, 1.0) * 255.0).round() as u8;
            let b = (coverage[y * hi_width + x * 3 + 2].clamp(0.0, 1.0) * 255.0).round() as u8;
            let base = (y * out_width + x) * 3;
            rgb[base] = r;
            rgb[base + 1] = g;
            rgb[base + 2] = b;
        }
    }

    Some(LcdBitmap::new(out_width as u32, height as u32, rgb))
}

/// Return the sum of all kernel weights (should be 1.0 for a normalised kernel).
///
/// Exposed for testing and verification purposes.
pub fn kernel_weight_sum(kind: LcdFilterKernel) -> f32 {
    fir_kernel(kind).iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fir_kernel_sums_to_one() {
        let kinds = [
            LcdFilterKernel::Box,
            LcdFilterKernel::Triangle,
            LcdFilterKernel::FreeType5Tap,
        ];
        for kind in kinds {
            let sum = kernel_weight_sum(kind);
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "kernel {:?} does not sum to 1.0: sum={sum}",
                kind
            );
        }
    }

    #[test]
    fn fir_horizontal_preserves_flat_signal() {
        // Interior pixels of a uniform coverage buffer should be unchanged by any
        // normalised FIR kernel.  Edge pixels accumulate fewer taps (zero-padded)
        // so only verify the interior samples (skip `half` pixels on each side).
        let width = 20usize;
        let height = 2usize;
        let kernel = fir_kernel(LcdFilterKernel::FreeType5Tap);
        let half = kernel.len() / 2;
        let mut buf = vec![0.5f32; width * height];
        fir_horizontal(&mut buf, width, height, kernel);
        for y in 0..height {
            for x in half..(width - half) {
                let v = buf[y * width + x];
                assert!(
                    (v - 0.5).abs() < 1e-4,
                    "uniform signal changed after FIR at ({x},{y}): got {v}"
                );
            }
        }
    }
}
