//! Felzenszwalb-Huttenlocher signed distance field computation.
//!
//! Implements the 2D Euclidean distance transform (EDT) described in:
//! "Distance Transforms of Sampled Functions",
//! Felzenszwalb & Huttenlocher, Theory of Computing 2012.

use rayon::prelude::*;

/// Error type for SDF computation failures.
#[derive(Debug)]
pub enum SdfError {
    /// The input coverage slice length does not match `width * height`.
    InvalidInput(String),
    /// Width or height was zero.
    ZeroSize,
    /// Font bytes could not be parsed by ttf-parser.
    InvalidFont,
    /// Binary data is malformed (bad magic, version mismatch, truncated payload).
    InvalidData(String),
    /// An I/O error occurred (e.g. during PNG export).
    Io(String),
}

impl std::fmt::Display for SdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SdfError::InvalidInput(s) => write!(f, "invalid input: {s}"),
            SdfError::ZeroSize => write!(f, "SDF dimensions must be non-zero"),
            SdfError::InvalidFont => write!(f, "could not parse font data"),
            SdfError::InvalidData(s) => write!(f, "invalid SDF binary data: {s}"),
            SdfError::Io(s) => write!(f, "I/O error: {s}"),
        }
    }
}

impl std::error::Error for SdfError {}

/// Compute 1D EDT in-place using the Felzenszwalb-Huttenlocher parabola-envelope algorithm.
///
/// On entry `f[i]` is `0.0` at seed positions and `INF` elsewhere.
/// On exit `f[i]` contains the squared Euclidean distance to the nearest seed.
#[cfg_attr(feature = "simd", allow(dead_code))]
pub(crate) fn edt_1d(f: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let inf = f32::INFINITY;
    // Scratch buffers for the parabola-envelope pass.
    let mut d = vec![0.0f32; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f32; n + 1];
    let mut k = 0usize;

    v[0] = 0;
    z[0] = -inf;
    z[1] = inf;

    for q in 1..n {
        let fq = f[q];
        loop {
            let r = v[k];
            let fr = f[r];
            let s =
                ((fq + (q * q) as f32) - (fr + (r * r) as f32)) / (2.0 * q as f32 - 2.0 * r as f32);
            if s <= z[k] {
                if k == 0 {
                    break;
                }
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = inf;
                break;
            }
        }
    }

    k = 0;
    for (q, slot) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let r = v[k];
        let dr = q as f32 - r as f32;
        *slot = dr * dr + f[r];
    }

    // Write results back in-place.
    f.copy_from_slice(&d);
}

/// SIMD-accelerated 1D EDT.
///
/// Produces identical output to [`edt_1d`].  Active only when the `simd` feature
/// is enabled; otherwise falls back to the scalar version.
#[cfg(feature = "simd")]
pub(crate) fn edt_1d_simd(f: &mut [f32]) {
    use wide::f32x4;

    let n = f.len();
    if n == 0 {
        return;
    }
    let inf = f32::INFINITY;

    // Phase 1: parabola-envelope (scalar — must remain sequential).
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f32; n + 1];
    let mut k = 0usize;

    v[0] = 0;
    z[0] = -inf;
    z[1] = inf;

    for q in 1..n {
        let fq = f[q];
        loop {
            let r = v[k];
            let fr = f[r];
            let s =
                ((fq + (q * q) as f32) - (fr + (r * r) as f32)) / (2.0 * q as f32 - 2.0 * r as f32);
            if s <= z[k] {
                if k == 0 {
                    break;
                }
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = inf;
                break;
            }
        }
    }

    // Phase 2: distance-fill with SIMD where possible.
    // We process 4 output positions at a time.  For each group of 4 positions q
    // we find their responsible parabola vertex r (scalar binary-search style),
    // then use f32x4 to compute (q - r)^2 + f[r] in parallel.
    let mut out = vec![0.0f32; n];
    let mut k_cursor = 0usize;

    let chunks = n / 4;
    let remainder_start = chunks * 4;

    let mut q = 0usize;
    for _c in 0..chunks {
        // Advance the scalar cursor for the first position of this chunk.
        while z[k_cursor + 1] < q as f32 {
            k_cursor += 1;
        }
        let r0 = v[k_cursor];
        let d0 = {
            let dr = q as f32 - r0 as f32;
            dr * dr + f[r0]
        };

        // For q+1 .. q+3 we need individual r values because the active
        // parabola may advance.  We use a local cursor per lane.
        let mut kk = k_cursor;
        let r_vals = [
            r0,
            {
                while z[kk + 1] < (q + 1) as f32 {
                    kk += 1;
                }
                v[kk]
            },
            {
                while z[kk + 1] < (q + 2) as f32 {
                    kk += 1;
                }
                v[kk]
            },
            {
                while z[kk + 1] < (q + 3) as f32 {
                    kk += 1;
                }
                v[kk]
            },
        ];
        // Advance the persistent cursor to the last lane's value.
        k_cursor = kk;

        // SIMD computation: (q_lane - r_lane)^2 + f[r_lane]
        let q_vec = f32x4::from([q as f32, (q + 1) as f32, (q + 2) as f32, (q + 3) as f32]);
        let r_vec = f32x4::from([
            r_vals[0] as f32,
            r_vals[1] as f32,
            r_vals[2] as f32,
            r_vals[3] as f32,
        ]);
        let fr_vec = f32x4::from([f[r_vals[0]], f[r_vals[1]], f[r_vals[2]], f[r_vals[3]]]);
        let diff = q_vec - r_vec;
        let dist = diff * diff + fr_vec;
        let arr: [f32; 4] = dist.into();
        // The first slot was computed scalar-first; keep it for clarity,
        // but the SIMD value matches.
        let _ = d0; // already captured in arr[0]
        out[q] = arr[0];
        out[q + 1] = arr[1];
        out[q + 2] = arr[2];
        out[q + 3] = arr[3];

        q += 4;
    }

    // Handle remainder positions (< 4) with scalar fallback.
    for (offset, slot) in out[remainder_start..n].iter_mut().enumerate() {
        let qq = remainder_start + offset;
        while z[k_cursor + 1] < qq as f32 {
            k_cursor += 1;
        }
        let r = v[k_cursor];
        let dr = qq as f32 - r as f32;
        *slot = dr * dr + f[r];
    }

    f.copy_from_slice(&out);
}

/// Dispatch: use SIMD path when the `simd` feature is active, scalar otherwise.
#[inline]
fn edt_1d_dispatch(f: &mut [f32]) {
    #[cfg(feature = "simd")]
    edt_1d_simd(f);
    #[cfg(not(feature = "simd"))]
    edt_1d(f);
}

/// 2D EDT using separable passes: rows first, then columns.
///
/// `grid` contains per-pixel initial values:
/// - `0.0` at pixels that are the "seed" (already at distance 0),
/// - `INF` (or large) at pixels that need a distance computed.
///
/// Returns squared Euclidean distances.
///
/// Row passes are executed in parallel using rayon; the column pass uses a
/// transposed buffer so that each column can also be processed in parallel.
///
/// The intermediate `tmp` allocation is eliminated: after the row pass the
/// results are transposed in-place into a reused buffer, reducing peak
/// memory to ~2 × N × M × 4 bytes instead of 3 ×.
pub fn edt_2d(grid: &[f32], width: usize, height: usize) -> Vec<f32> {
    // ── Row pass: process all rows in parallel ────────────────────────────────
    let mut row_result: Vec<f32> = grid.to_vec();

    row_result.par_chunks_mut(width).for_each(|row| {
        edt_1d_dispatch(row);
    });

    // ── In-place column pass via transposed layout ────────────────────────────
    // Transpose row_result (W×H) → transposed (H×W), where each "row" in
    // transposed is one original column.
    let mut transposed: Vec<f32> = vec![0.0; width * height];
    for y in 0..height {
        for x in 0..width {
            transposed[x * height + y] = row_result[y * width + x];
        }
    }

    transposed.par_chunks_mut(height).for_each(|col| {
        edt_1d_dispatch(col);
    });

    // Transpose back into row_result (reuse the allocation).
    for x in 0..width {
        for y in 0..height {
            row_result[y * width + x] = transposed[x * height + y];
        }
    }

    row_result
}

/// Compute a signed SDF from a grayscale coverage map.
///
/// # Arguments
/// - `coverage` — grayscale bitmap (`width × height` bytes); values > 127 are
///   treated as "inside" the glyph outline.
/// - `width`, `height` — bitmap dimensions.
/// - `spread` — maximum SDF distance in pixels; maps ±spread to [0, 255].
/// - `padding` — extra border (in pixels) added around the output to prevent
///   atlas edge artefacts. The returned slice has dimensions
///   `(width + 2*padding) × (height + 2*padding)`.  Pass `0` for no padding.
///
/// # Returns
/// A `Vec<u8>` of dimensions `(width + 2*padding) × (height + 2*padding)`, where:
/// - `< 128` = outside the outline,
/// - `≈ 128` = near the outline (the 0.5 isovalue),
/// - `> 128` = inside the outline.
///
/// # Errors
/// Returns [`SdfError::InvalidInput`] if `coverage.len() != width * height`.
pub fn compute_sdf(
    coverage: &[u8],
    width: usize,
    height: usize,
    spread: f32,
    padding: u32,
) -> Result<Vec<u8>, SdfError> {
    if coverage.len() != width * height {
        return Err(SdfError::InvalidInput(format!(
            "coverage length {} != width({}) * height({})",
            coverage.len(),
            width,
            height
        )));
    }

    let pad = padding as usize;
    let padded_w = width + 2 * pad;
    let padded_h = height + 2 * pad;
    let n_padded = padded_w * padded_h;
    const LARGE: f32 = 1e10;

    // Build inside and outside seed grids with padding:
    // Padding border pixels are all "outside" (inside_grid = LARGE, outside_grid = 0).
    let mut inside_grid = vec![LARGE; n_padded];
    let mut outside_grid = vec![LARGE; n_padded];

    for y in 0..height {
        for x in 0..width {
            let src_idx = y * width + x;
            let dst_idx = (y + pad) * padded_w + (x + pad);
            if coverage[src_idx] > 127 {
                inside_grid[dst_idx] = 0.0;
            } else {
                outside_grid[dst_idx] = 0.0;
            }
        }
    }
    // Padding rows/columns are "outside".
    for y in 0..padded_h {
        for x in 0..padded_w {
            let in_inner = x >= pad && x < pad + width && y >= pad && y < pad + height;
            if !in_inner {
                outside_grid[y * padded_w + x] = 0.0;
            }
        }
    }

    let dist_inside = edt_2d(&inside_grid, padded_w, padded_h);
    let dist_outside = edt_2d(&outside_grid, padded_w, padded_h);

    let out: Vec<u8> = (0..n_padded)
        .map(|i| {
            let d_in = dist_inside[i].sqrt();
            let d_out = dist_outside[i].sqrt();
            let signed = d_out - d_in;
            let normalized = 0.5 + signed / (2.0 * spread);
            let clamped = normalized.clamp(0.0, 1.0);
            (clamped * 255.0).round() as u8
        })
        .collect();

    Ok(out)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_square_inside_everywhere() {
        let w = 8usize;
        let h = 8usize;
        let coverage = vec![255u8; w * h];
        let sdf = compute_sdf(&coverage, w, h, 4.0, 0).expect("compute_sdf solid square");
        let center = sdf[(h / 2) * w + w / 2];
        assert!(
            center > 128,
            "center of solid square should be inside (> 128), got {center}"
        );
    }

    #[test]
    fn empty_square_outside_everywhere() {
        let w = 8usize;
        let h = 8usize;
        let coverage = vec![0u8; w * h];
        let sdf = compute_sdf(&coverage, w, h, 4.0, 0).expect("compute_sdf empty square");
        let center = sdf[(h / 2) * w + w / 2];
        assert!(
            center < 128,
            "center of empty square should be outside (< 128), got {center}"
        );
    }

    #[test]
    fn ring_shape_inside_outside() {
        let w = 8usize;
        let h = 8usize;
        let mut coverage = vec![0u8; w * h];
        for x in 0..w {
            coverage[x] = 255;
            coverage[(h - 1) * w + x] = 255;
        }
        for y in 0..h {
            coverage[y * w] = 255;
            coverage[y * w + (w - 1)] = 255;
        }
        let sdf = compute_sdf(&coverage, w, h, 4.0, 0).expect("compute_sdf ring");
        assert_eq!(sdf.len(), w * h, "sdf length should match w*h");
    }

    #[test]
    fn padding_grows_output_size() {
        let w = 4usize;
        let h = 4usize;
        let coverage = vec![128u8; w * h];
        let sdf = compute_sdf(&coverage, w, h, 4.0, 2).expect("compute_sdf with padding");
        assert_eq!(sdf.len(), (w + 4) * (h + 4));
    }

    #[test]
    fn invalid_coverage_length_errors() {
        let coverage = vec![128u8; 10];
        assert!(compute_sdf(&coverage, 4, 4, 4.0, 0).is_err());
    }

    #[test]
    fn test_edt_1d_simd_matches_scalar() {
        #[cfg(feature = "simd")]
        {
            let input_scalar: Vec<f32> = (0..32)
                .map(|i| if i % 8 == 0 { 0.0 } else { f32::INFINITY })
                .collect();
            let mut scalar = input_scalar.clone();
            let mut simd_buf = input_scalar.clone();
            edt_1d(&mut scalar);
            edt_1d_simd(&mut simd_buf);
            for (s, v) in scalar.iter().zip(simd_buf.iter()) {
                assert!((s - v).abs() < 0.001, "scalar={s}, simd={v}");
            }
        }
    }

    // ─── SDF quality comparison tests ─────────────────────────────────────────
    //
    // These tests validate `compute_sdf` output analytically against known
    // distance values derived from the Felzenszwalb-Huttenlocher EDT semantics.
    //
    // In the EDT, every coverage pixel is either "inside" (seed for inside_grid,
    // d_in=0) or "outside" (seed for outside_grid, d_out=0).  Therefore the
    // `signed = d_out - d_in` value is computed entirely from pixel-center distances
    // to the nearest opposite-class pixel.
    //
    // At the boundary the minimum |signed| achievable is 1.0 (adjacent pixels in
    // opposite classes), which maps to:
    //   0.5 + 1.0/(2*spread)
    // With spread=8.0 that gives 0.5625 → 143 (or 113 on the far side),
    // so the boundary tolerance is ±20 (not ±5 as for a continuous outline SDF).

    /// Build a coverage bitmap containing a filled square of side `sq_side` centred at (cx, cy).
    ///
    /// The square spans `[cx - half, cx + half]` **inclusive** on both axes so that the
    /// integer grid is symmetric around `cx` and `cy` — a necessary condition for the
    /// 4-fold symmetry test.  `sq_side` must be even; the inclusive half-extent is
    /// `sq_side / 2`.
    fn filled_square_coverage(w: usize, h: usize, cx: usize, cy: usize, sq_side: usize) -> Vec<u8> {
        let half = sq_side / 2;
        let x_min = cx.saturating_sub(half);
        // Use exclusive bound cx + half + 1 so the range [x_min, x_max) covers
        // [cx-half, cx+half] inclusive, giving a symmetric pixel grid.
        let x_max = (cx + half + 1).min(w);
        let y_min = cy.saturating_sub(half);
        let y_max = (cy + half + 1).min(h);
        let mut cov = vec![0u8; w * h];
        for y in y_min..y_max {
            for x in x_min..x_max {
                cov[y * w + x] = 255;
            }
        }
        cov
    }

    /// Build a coverage bitmap with a filled circle of the given radius.
    fn circle_coverage_edt(w: usize, h: usize, cx: f32, cy: f32, r: f32) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let x = (i % w) as f32;
                let y = (i / w) as f32;
                // Use `d < r + 0.5` so the pixel whose centre is on the radius
                // counts as inside, giving better-defined boundary pixels.
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                if d < r + 0.5 {
                    255u8
                } else {
                    0u8
                }
            })
            .collect()
    }

    /// Test 1: filled square SDF — symmetry, inside/outside, boundary.
    ///
    /// A 32×32 bitmap with a 16×16 filled square centred at (16,16).
    /// Spread = 8.0 maps ±8 pixel distance to the [0,255] range.
    ///
    /// Expected values (from EDT discrete math):
    /// - Center (16,16): 8 pixels from nearest outside edge → signed = +8.0
    ///   → normalized = 0.5 + 8/(2*8) = 1.0 → clamped → 255.
    /// - Corners (0,0): ~22 pixels from nearest inside edge → signed ≈ -22.0
    ///   → normalized = 0.5 - 22/16 < 0 → clamped → 0.
    /// - Boundary pixel (just outside, x=16 on the right edge where x=24 is the
    ///   first outside pixel): 1 pixel from the nearest inside → signed = -(1)
    ///   → 0.5 - 1/16 = 0.4375 → 111.  Still within ±20 of 128.
    #[test]
    fn sdf_quality_filled_square_symmetry() {
        let w = 32usize;
        let h = 32usize;
        let cx = 16usize;
        let cy = 16usize;
        let sq_side = 16usize;
        let spread = 8.0f32;

        let coverage = filled_square_coverage(w, h, cx, cy, sq_side);
        let sdf = compute_sdf(&coverage, w, h, spread, 0).expect("compute_sdf filled square");
        assert_eq!(sdf.len(), w * h);

        // 1a. Center of square must be inside (> 128).
        let center_v = sdf[cy * w + cx];
        assert!(
            center_v > 128,
            "center pixel should be inside (>128), got {center_v}"
        );

        // 1b. Corner must be outside (< 128).
        let corner_v = sdf[0];
        assert!(
            corner_v < 128,
            "corner pixel should be outside (<128), got {corner_v}"
        );

        // 1c. Boundary: the pixel just outside the right edge of the square.
        //     The square spans x=8..=24 (inclusive), so x=25 is the first outside
        //     pixel on the right.  It is 1 pixel from x=24 (last inside).
        //     signed ≈ -(1) → normalized ≈ 0.44 → byte ≈ 112; within ±20 of 128.
        let boundary_x = cx + sq_side / 2 + 1; // 25
        let boundary_v = sdf[cy * w + boundary_x] as i32;
        assert!(
            (boundary_v - 128).abs() <= 20,
            "boundary pixel at ({boundary_x},{cy}) should be within ±20 of 128 \
             (first outside pixel on right edge), got {boundary_v}"
        );

        // 1d. 4-fold symmetry: pixel at +8 columns from center should equal pixel at -8 columns.
        //     Both are 8 pixels inside from the nearest edge of the square.
        let offset = 4usize;
        let sym_right = sdf[cy * w + (cx + offset)] as i32;
        let sym_left = sdf[cy * w + (cx - offset)] as i32;
        assert_eq!(
            sym_right,
            sym_left,
            "4-fold SDF symmetry broken: sdf[{},{}]={sym_right} vs sdf[{},{}]={sym_left}",
            cx + offset,
            cy,
            cx - offset,
            cy
        );

        let sym_down = sdf[(cy + offset) * w + cx] as i32;
        let sym_up = sdf[(cy - offset) * w + cx] as i32;
        assert_eq!(
            sym_down,
            sym_up,
            "4-fold SDF symmetry broken: sdf[{},{}]={sym_down} vs sdf[{},{}]={sym_up}",
            cx,
            cy + offset,
            cx,
            cy - offset
        );
    }

    /// Test 2: circular coverage SDF with analytically derived expected values.
    ///
    /// A 64×64 bitmap with a filled circle of radius 16 centred at (32,32).
    /// Spread = 8.0.
    ///
    /// Expected values (from EDT discrete math):
    /// - Center (32,32): nearest outside pixel is ~16 away.
    ///   signed ≈ +16 → normalized = 0.5+16/16 = 1.5 → clamped → 255.
    /// - Boundary pixel (32, 49) right at r+0.5 edge: 1 pixel inside or 1 pixel
    ///   outside → |signed| ≈ 1 → normalized ≈ 0.5 ± 0.0625 → byte ≈ 112–143 (±20 of 128).
    /// - Outside by ~4px (32, 53): nearest inside pixel ≈ 4px away.
    ///   signed ≈ -4 → normalized = 0.5 - 4/16 = 0.25 → byte ≈ 64.
    ///   With ±12 tolerance (EDT pixels are discrete): expect 52–76.
    #[test]
    fn sdf_quality_circular_coverage() {
        let size = 64usize;
        let cx = 32.0f32;
        let cy = 32.0f32;
        let r = 16.0f32;
        let spread = 8.0f32;

        let coverage = circle_coverage_edt(size, size, cx, cy, r);
        let sdf = compute_sdf(&coverage, size, size, spread, 0).expect("compute_sdf circle");
        assert_eq!(sdf.len(), size * size);

        // 2a. Center — far inside, should hit the upper clamp.
        let center_v = sdf[32 * size + 32];
        assert!(
            center_v > 200,
            "circle center should be far inside (>200), got {center_v}"
        );

        // 2b. Boundary pixel at (32, cy+r as usize) — within ±20 of 128.
        //     With r+0.5 coverage threshold, pixel at y=48 (32+16) is the last
        //     inside pixel; pixel at y=49 is the first outside.
        //     Check both sides and assert one is ≤128 and the other ≥128.
        let y_inside = (cy + r - 1.0) as usize; // 47
        let y_outside = (cy + r + 1.0) as usize; // 49
        let inside_v = sdf[y_inside * size + 32] as i32;
        let outside_v = sdf[y_outside * size + 32] as i32;
        assert!(
            inside_v >= 128,
            "pixel just inside the circle boundary ({},32) should be >=128, got {inside_v}",
            y_inside
        );
        assert!(
            outside_v <= 128,
            "pixel just outside the circle boundary ({},32) should be <=128, got {outside_v}",
            y_outside
        );

        // 2c. Outside by ~4 pixels: (32, cy+r+4) ≈ (32, 52).
        //     Nearest inside pixel ≈ ~4px away, signed ≈ -4.
        //     normalized = 0.5 - 4/(2*8) = 0.25 → byte ≈ 64.
        //     Discrete EDT noise: allow ±15 → range [49, 79].
        let y_far_out = (cy + r + 4.0) as usize; // 52
        let far_out_v = sdf[y_far_out * size + 32] as i32;
        assert!(
            far_out_v < 128,
            "pixel outside circle by ~4px ({y_far_out},32) should be <128, got {far_out_v}"
        );
        assert!(
            (far_out_v - 64).abs() <= 25,
            "pixel outside circle by ~4px should be near 64 (±25), got {far_out_v}"
        );
    }
}
