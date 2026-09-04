//! Forward 4x4 DCT and the IDCT-plus-prediction family on `wide` lane types — the
//! twin of `simd::x86_64::dct`.
//!
//! The intrinsic kernels do their row passes in scalar code — the forward DCT pulls
//! the row's four sums out of the register with `movd` and rebuilds the result with
//! `pinsrw`, and the IDCT computes each row's butterfly on `i16` scalars before
//! widening — and vectorise only the column pass. This file keeps that split: the
//! row passes are the same scalar arithmetic, the column passes are lane-wise
//! `i16x8`/`i32x4` ops, and the prediction add and clip are a widen, an add and a
//! `packuswb`.
//!
//! The IDCT's vertical pass runs in `i32x4` for the reason the intrinsic file gives:
//! the horizontal pass truncates to `i16`, so the column sums overflow sixteen bits
//! and must not wrap where the scalar saturates.

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, i32x4, i32x8};

use super::lanes::{load16, load4, low4, merge_lo64, narrow, widen_hi, widen_lo};
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples, SampleCursor};

// ============================================================================
// Forward 4x4 DCT
// ============================================================================

/// The 1D forward DCT of the row in the low four lanes of `d`, back in the low four
/// lanes. Scalar, as the intrinsic kernel's is.
#[inline(always)]
fn dct_row(d: i16x8) -> i16x8 {
    let a = d.to_array();
    let (d0, d1, d2, d3) = (a[0] as i32, a[1] as i32, a[2] as i32, a[3] as i32);
    let s0 = d0 + d3;
    let s1 = d1 + d2;
    let s2 = d1 - d2;
    let s3 = d0 - d3;
    i16x8::new([
        (s0 + s1) as i16,
        ((s3 << 1i32) + s2) as i16,
        (s0 - s1) as i16,
        (s3 - (s2 << 1i32)) as i16,
        0,
        0,
        0,
        0,
    ])
}

#[inline(always)]
fn diff_row<A: SampleCursor, B: SampleCursor>(pix1: &A, pix2: &B, dy: isize) -> i16x8 {
    widen_lo(load4(&pix1.row_n::<4>(dy, 0))) - widen_lo(load4(&pix2.row_n::<4>(dy, 0)))
}

#[inline(always)]
fn dct_4x4_impl<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    let y0 = dct_row(diff_row(pix1, pix2, 0));
    let y1 = dct_row(diff_row(pix1, pix2, 1));
    let y2 = dct_row(diff_row(pix1, pix2, 2));
    let y3 = dct_row(diff_row(pix1, pix2, 3));

    // The column pass, lane-wise over the four rows.
    let s0 = y0 + y3;
    let s3 = y0 - y3;
    let s1 = y1 + y2;
    let s2 = y1 - y2;

    let out0 = s0 + s1;
    let out1 = (s3 << 1i32) + s2;
    let out2 = s0 - s1;
    let out3 = s3 - (s2 << 1i32);

    dct[..8].copy_from_slice(merge_lo64(out0, out1).as_array());
    dct[8..].copy_from_slice(merge_lo64(out2, out3).as_array());
}

/// C++: `WelsDctT4_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn dct_4x4_sse2<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    dct_4x4_impl(dct, pix1, pix2)
}

/// C++: `WelsDctFourT4_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn dct_four_4x4_sse2<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 64], pix1: &A, pix2: &B) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &mut [i16; 16] = (&mut dct[k << 4i32..][..16]).try_into().unwrap();
        dct_4x4_impl(sub, &pix1.advance(dx, dy), &pix2.advance(dx, dy));
    }
}

// ============================================================================
// Inverse 4x4 DCT and prediction add
// ============================================================================

/// The 1D inverse DCT of one row, in scalar, widened to `i32` lanes on the way out —
/// the intrinsic kernel's `idct_row_sse2` and `widen_lo_i16_to_i32_sse2` in one.
#[inline(always)]
fn idct_row(r0: i16, r1: i16, r2: i16, r3: i16) -> i32x4 {
    let (r0, r1, r2, r3) = (r0 as i32, r1 as i32, r2 as i32, r3 as i32);
    let t0 = r0 + r2;
    let t1 = r0 - r2;
    let t2 = (r1 >> 1i32) - r3;
    let t3 = r1 + (r3 >> 1i32);
    // The horizontal pass truncates to `i16`, as the C++'s `int16_t` array does;
    // the truncation is observable and the scalar keeps it.
    i32x4::new([
        (t0 + t3) as i16 as i32,
        (t1 + t2) as i16 as i32,
        (t1 - t2) as i16 as i32,
        (t0 - t3) as i16 as i32,
    ])
}

/// Saturates the four `i32` lanes to the low four `i16` lanes — `packssdw` against
/// zero. Unreachable saturation here, as in the intrinsic kernel: the residuals
/// are bounded by 1792 after the `>> 6`.
#[inline(always)]
fn narrow_i32(v: i32x4) -> i16x8 {
    i16x8::from_i32x8_saturate(cast::<[i32x4; 2], i32x8>([v, i32x4::ZERO]))
}

/// The four residual rows of a 4x4 block, in the low four lanes each.
#[inline(always)]
fn compute_idct_residuals(dct: &[i16; 16]) -> [i16x8; 4] {
    let s0 = idct_row(dct[0], dct[1], dct[2], dct[3]);
    let s4 = idct_row(dct[4], dct[5], dct[6], dct[7]);
    let s8 = idct_row(dct[8], dct[9], dct[10], dct[11]);
    let s12 = idct_row(dct[12], dct[13], dct[14], dct[15]);

    let c32 = i32x4::splat(32);

    let t1_a = s0 + s8;
    let t2_a = s4 + (s12 >> 1i32);
    let res0 = (t1_a + t2_a + c32) >> 6i32;
    let res3 = (t1_a - t2_a + c32) >> 6i32;

    let t1_b = s0 - s8;
    let t2_b = (s4 >> 1i32) - s12;
    let res1 = (t1_b + t2_b + c32) >> 6i32;
    let res2 = (t1_b - t2_b + c32) >> 6i32;

    [narrow_i32(res0), narrow_i32(res1), narrow_i32(res2), narrow_i32(res3)]
}

/// `clip(pred + res)` for one row of four.
#[inline(always)]
fn add_res_and_clip(pred: [u8; 4], res: i16x8) -> [u8; 4] {
    let sum = widen_lo(load4(&pred)) + res;
    low4(narrow(sum, sum))
}

/// C++: `IdctResAddPred_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_res_add_pred_sse2(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    for (dy, res) in compute_idct_residuals(rs).into_iter().enumerate() {
        let row: &mut [u8; 4] = pred.row_mut(dy as isize, 0, 4).try_into().unwrap();
        *row = add_res_and_clip(*row, res);
    }
}

/// C++: `WelsIDctT4Rec_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_t4_rec_sse2(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 16]) {
    for (dy, res) in compute_idct_residuals(dct).into_iter().enumerate() {
        let p: [u8; 4] = pred.row_view(dy as isize, 0, 4).try_into().unwrap();
        let row: &mut [u8; 4] = rec.row_mut(dy as isize, 0, 4).try_into().unwrap();
        *row = add_res_and_clip(p, res);
    }
}

/// [`idct_t4_rec_sse2`] in place on `rec`.
#[inline]
pub fn idct_t4_rec_in_place_sse2(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    idct_res_add_pred_sse2(rec, dct)
}

/// C++: `WelsIDctFourT4Rec_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_four_t4_rec_sse2(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
        idct_t4_rec_sse2(&mut rec.reborrow(dx, dy), &pred.advance(dx, dy), sub);
    }
}

/// [`idct_t4_rec_in_place_sse2`] over four 4x4 blocks forming an 8x8 quadrant.
#[inline]
pub fn idct_four_t4_rec_in_place_sse2(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
        idct_t4_rec_in_place_sse2(&mut rec.reborrow(dx, dy), sub);
    }
}

/// [`idct_t4_rec_to_view`](crate::encoder::decode_mb_aux::idct_t4_rec_to_view) on `wide`.
#[inline]
pub fn idct_t4_rec_to_view_sse2(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 16]) {
    for (dy, res) in compute_idct_residuals(dct).into_iter().enumerate() {
        let p: [u8; 4] = pred[dy * pred_stride..][..4].try_into().unwrap();
        let out = add_res_and_clip(p, res);
        rec.write_row::<4>(dy as isize, 0, &out);
    }
}

/// [`idct_four_t4_rec_to_view`](crate::encoder::decode_mb_aux::idct_four_t4_rec_to_view) on `wide`.
#[inline]
pub fn idct_four_t4_rec_to_view_sse2(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
        let off = dy as usize * pred_stride + dx as usize;
        idct_t4_rec_to_view_sse2(&rec.advance(dx, dy), &pred[off..], pred_stride, sub);
    }
}

/// [`idct_t4_rec_in_place_view`](crate::encoder::decode_mb_aux::idct_t4_rec_in_place_view) on `wide`.
#[inline]
pub fn idct_t4_rec_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    for (dy, res) in compute_idct_residuals(dct).into_iter().enumerate() {
        let cur = rec.row::<4>(dy as isize, 0);
        let out = add_res_and_clip(cur, res);
        rec.write_row::<4>(dy as isize, 0, &out);
    }
}

/// [`idct_four_t4_rec_in_place_view`](crate::encoder::decode_mb_aux::idct_four_t4_rec_in_place_view) on `wide`.
#[inline]
pub fn idct_four_t4_rec_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    for (k, &(dx, dy)) in SUBS.iter().enumerate() {
        let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
        idct_t4_rec_in_place_view_sse2(&rec.advance(dx, dy), sub);
    }
}

/// [`idct_t4_rec_on_mb_in_place_view`](crate::encoder::decode_mb_aux::idct_t4_rec_on_mb_in_place_view) on `wide`.
#[inline]
pub fn idct_t4_rec_on_mb_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 256]) {
    const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];
    for (k, &(dx, dy)) in QUADS.iter().enumerate() {
        let sub: &[i16; 64] = (&dct[k << 6i32..][..64]).try_into().unwrap();
        idct_four_t4_rec_in_place_view_sse2(&rec.advance(dx, dy), sub);
    }
}

// ============================================================================
// 16x16 DC-only reconstruction
// ============================================================================

/// The rounded DC offsets of row `i`, spread four lanes each: `[d0 x4 | d1 x4]` and
/// `[d2 x4 | d3 x4]`.
#[inline(always)]
fn dc_row_offsets(dc: &[i16; 16], i: usize) -> (i16x8, i16x8) {
    let dc_row = i & 0x0C;
    let d = |k: usize| ((dc[dc_row + k] as i32 + 32) >> 6i32) as i16;
    let (d0, d1, d2, d3) = (d(0), d(1), d(2), d(3));
    (
        i16x8::new([d0, d0, d0, d0, d1, d1, d1, d1]),
        i16x8::new([d2, d2, d2, d2, d3, d3, d3, d3]),
    )
}

#[inline(always)]
fn dc_add_row(pred: &[u8], dc_lo: i16x8, dc_hi: i16x8) -> [u8; 16] {
    let p = load16(pred);
    narrow(widen_lo(p) + dc_lo, widen_hi(p) + dc_hi).to_array()
}

/// C++: `WelsIDctRecI16x16Dc_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_rec_i16x16_dc_sse2(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dc: &[i16; 16]) {
    for i in 0..16usize {
        let (dc_lo, dc_hi) = dc_row_offsets(dc, i);
        let out = dc_add_row(pred.row(i as isize, 0, 16), dc_lo, dc_hi);
        rec.row_mut(i as isize, 0, 16).copy_from_slice(&out);
    }
}

/// 16x16 macroblock DC luma reconstruction to a shared view.
#[inline]
pub fn idct_rec_i16x16_dc_to_view_sse2(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dc: &[i16; 16]) {
    for i in 0..16usize {
        let (dc_lo, dc_hi) = dc_row_offsets(dc, i);
        let out = dc_add_row(&pred[i * pred_stride..][..16], dc_lo, dc_hi);
        rec.write_row::<16>(i as isize, 0, &out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_mb_aux::{dct_4x4, dct_four_4x4};
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers:
    // the dispatchers route to the very SSE2 kernels under test, which would
    // make every assertion below a tautology.
    use crate::encoder::decode_mb_aux::{
        idct_rec_i16x16_dc_c as idct_rec_i16x16_dc, idct_t4_rec_c as idct_t4_rec,
        idct_t4_rec_in_place_c as idct_t4_rec_in_place,
    };
    use crate::decoder::decode_mb_aux::idct_res_add_pred_c as idct_res_add_pred;
    use crate::safe::plane::PaddedPlane;
    use crate::encoder::rec_view::shared_plane_for_test;

    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 32i32) & 0xFF) as u8
    }

    /// Coefficients over the **full `i16` range**, which is what the decoder hands the
    /// IDCT: `rs` comes from the bitstream by way of dequantisation, not from this
    /// port's own quantiser. A narrower cap keeps the vertical pass inside the range
    /// where 16- and 32-bit lanes agree, and passes on a kernel that is wrong — see
    /// `compute_idct_residuals_sse2`.
    fn lcg_i16(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32i32) as u16 as i16
    }

    #[test]
    fn test_dct_4x4_parity() {
        let mut seed = 12345u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p1 = PaddedPlane::new(w, h, pad, stride);
        let mut p2 = PaddedPlane::new(w, h, pad, stride);

        for _ in 0..100 {
            for y in 0..4isize {
                for x in 0..4isize {
                    p1.set(x, y, lcg(&mut seed));
                    p2.set(x, y, lcg(&mut seed));
                }
            }

            let mut dct_c = [0i16; 16];
            let mut dct_simd = [0i16; 16];

            dct_4x4(&mut dct_c, &p1.cursor(0, 0), &p2.cursor(0, 0));
            dct_4x4_sse2(&mut dct_simd, &p1.cursor(0, 0), &p2.cursor(0, 0));

            assert_eq!(dct_simd, dct_c);
        }
    }

    #[test]
    fn test_dct_four_4x4_parity() {
        let mut seed = 54321u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p1 = PaddedPlane::new(w, h, pad, stride);
        let mut p2 = PaddedPlane::new(w, h, pad, stride);

        for _ in 0..50 {
            for y in 0..8isize {
                for x in 0..8isize {
                    p1.set(x, y, lcg(&mut seed));
                    p2.set(x, y, lcg(&mut seed));
                }
            }

            let mut dct_c = [0i16; 64];
            let mut dct_simd = [0i16; 64];

            dct_four_4x4(&mut dct_c, &p1.cursor(0, 0), &p2.cursor(0, 0));
            dct_four_4x4_sse2(&mut dct_simd, &p1.cursor(0, 0), &p2.cursor(0, 0));

            assert_eq!(dct_simd, dct_c);
        }
    }

    #[test]
    fn test_idct_res_add_pred_parity() {
        let mut seed = 98765u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p_c = PaddedPlane::new(w, h, pad, stride);
        let mut p_simd = PaddedPlane::new(w, h, pad, stride);

        for _ in 0..100 {
            for y in 0..4isize {
                for x in 0..4isize {
                    let v = lcg(&mut seed);
                    p_c.set(x, y, v);
                    p_simd.set(x, y, v);
                }
            }

            let mut rs = [0i16; 16];
            for v in rs.iter_mut() {
                *v = lcg_i16(&mut seed);
            }

            idct_res_add_pred(&mut p_c.cursor_mut(0, 0), &rs);
            idct_res_add_pred_sse2(&mut p_simd.cursor_mut(0, 0), &rs);

            for y in 0..4isize {
                for x in 0..4isize {
                    assert_eq!(p_simd.at(x, y), p_c.at(x, y), "mismatch at ({x}, {y})");
                }
            }
        }
    }

    /// The exact case the 16-bit vertical pass got wrong, pinned so a future
    /// "optimisation" back to `epi16` fails here instead of in someone's stream.
    ///
    /// `rs[0] = rs[8] = 20000` with a zero prediction puts `t1 = s0 + s8 = 40000` into
    /// the vertical butterfly. In `i32` that is `(32 + 40000) >> 6 = 625`, clipped to
    /// 255. In 16-bit lanes it wrapped to `-25536`, `>> 6 = -399`, and `packus`
    /// saturated it to 0 — black where the scalar produces white.
    #[test]
    fn idct_vertical_pass_does_not_wrap_at_16_bits() {
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p_c = PaddedPlane::new(w, h, pad, stride);
        let mut p_simd = PaddedPlane::new(w, h, pad, stride);
        for y in 0..4isize {
            for x in 0..4isize {
                p_c.set(x, y, 0);
                p_simd.set(x, y, 0);
            }
        }

        let mut rs = [0i16; 16];
        rs[0] = 20000;
        rs[8] = 20000;

        idct_res_add_pred(&mut p_c.cursor_mut(0, 0), &rs);
        idct_res_add_pred_sse2(&mut p_simd.cursor_mut(0, 0), &rs);

        assert_eq!(p_c.at(0, 0), 255, "the scalar reference itself moved");
        for y in 0..4isize {
            for x in 0..4isize {
                assert_eq!(p_simd.at(x, y), p_c.at(x, y), "mismatch at ({x}, {y})");
            }
        }
    }

    #[test]
    fn test_idct_t4_rec_parity() {
        let mut seed = 112233u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let pred = PaddedPlane::new(w, h, pad, stride);
        let mut rec_c = PaddedPlane::new(w, h, pad, stride);
        let mut rec_simd = PaddedPlane::new(w, h, pad, stride);

        for _ in 0..100 {
            let mut rs = [0i16; 16];
            for v in rs.iter_mut() {
                *v = lcg_i16(&mut seed);
            }

            idct_t4_rec(&mut rec_c.cursor_mut(0, 0), &pred.cursor(0, 0), &rs);
            idct_t4_rec_sse2(&mut rec_simd.cursor_mut(0, 0), &pred.cursor(0, 0), &rs);

            for y in 0..4isize {
                for x in 0..4isize {
                    assert_eq!(rec_simd.at(x, y), rec_c.at(x, y), "mismatch at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn test_idct_rec_i16x16_dc_parity() {
        let mut seed = 445566u64;
        let (w, h, pad, stride) = (32usize, 32usize, 16usize, 64usize);
        let mut pred = PaddedPlane::new(w, h, pad, stride);
        let mut rec_c = PaddedPlane::new(w, h, pad, stride);
        let mut rec_simd = PaddedPlane::new(w, h, pad, stride);

        for y in 0..16isize {
            for x in 0..16isize {
                pred.set(x, y, lcg(&mut seed));
            }
        }

        for _ in 0..20 {
            let mut dc = [0i16; 16];
            for v in dc.iter_mut() {
                *v = lcg_i16(&mut seed);
            }

            idct_rec_i16x16_dc(&mut rec_c.cursor_mut(0, 0), &pred.cursor(0, 0), &dc);
            idct_rec_i16x16_dc_sse2(&mut rec_simd.cursor_mut(0, 0), &pred.cursor(0, 0), &dc);

            for y in 0..16isize {
                for x in 0..16isize {
                    assert_eq!(rec_simd.at(x, y), rec_c.at(x, y), "dc mismatch at ({x}, {y})");
                }
            }
        }
    }

    // ========================================================================
    // The reconstruction-seam entry points.
    //
    // Each runs an `_sse2` kernel against `idct_t4_rec_c` / `idct_t4_rec_in_place_c` /
    // `idct_rec_i16x16_dc_c`, which cannot route back here. Note the
    // `*_matches_the_plane_cursor_form` tests in `encoder/decode_mb_aux.rs` are *not*
    // this: on x86_64 both of their sides dispatch into this file, so they pin the
    // `RecCursor`-vs-`PlaneCursorMut` equivalence and not SSE2 against scalar.
    //
    // The multi-block forms are referenced against the scalar applied per block at the
    // sub-offsets, not against this file's own four-block loop, so a transposed
    // `(dx, dy)` in the hand-written `off`/`advance` arithmetic fails rather than being
    // a shared assumption. Whole allocations are compared, never just the block.
    // ========================================================================

    /// Two planes of identical geometry filled with the same noise, padding included —
    /// so an out-of-block write shows up as an allocation difference.
    fn twin_planes(seed: &mut u64) -> (PaddedPlane, PaddedPlane) {
        let (w, h, pad, stride) = (32usize, 32usize, 16usize, 64usize);
        let mut a = PaddedPlane::new(w, h, pad, stride);
        let mut b = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                let v = lcg(seed);
                a.set(x, y, v);
                b.set(x, y, v);
            }
        }
        (a, b)
    }

    /// A flat prediction arena at `stride` — the shape `sMemPredMb` has.
    fn noisy_pred(seed: &mut u64, stride: usize, rows: usize) -> Vec<u8> {
        (0..stride * rows).map(|_| lcg(seed)).collect()
    }

    /// The same bytes as a plane, for the `PlaneCursorMut` scalar reference.
    fn pred_as_plane(pred: &[u8], stride: usize, rows: usize) -> PaddedPlane {
        let mut pp = PaddedPlane::new(stride, rows, 0, stride);
        for y in 0..rows as isize {
            for x in 0..stride as isize {
                pp.set(x, y, pred[y as usize * stride + x as usize]);
            }
        }
        pp
    }

    fn coeffs<const N: usize>(seed: &mut u64) -> [i16; N] {
        core::array::from_fn(|_| lcg_i16(seed))
    }

    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];

    #[test]
    fn idct_t4_rec_in_place_sse2_matches_the_scalar() {
        let mut seed = 0x0DDB_1A5E_5BAD_5EEDu64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 16] = coeffs(&mut seed);

        idct_t4_rec_in_place(&mut pa.cursor_mut(5, 7), &dct);
        idct_t4_rec_in_place_sse2(&mut pb.cursor_mut(5, 7), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_four_t4_rec_sse2_matches_four_scalar_blocks() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 64] = coeffs(&mut seed);
        let pred = noisy_pred(&mut seed, 16, 8);
        let pp = pred_as_plane(&pred, 16, 8);

        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
            idct_t4_rec(&mut pa.cursor_mut(6 + dx, 9 + dy), &pp.cursor(dx, dy), sub);
        }
        idct_four_t4_rec_sse2(&mut pb.cursor_mut(6, 9), &pp.cursor(0, 0), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_four_t4_rec_in_place_sse2_matches_four_scalar_blocks() {
        let mut seed = 0x8A5C_D789_635D_2DFFu64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 64] = coeffs(&mut seed);

        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
            idct_t4_rec_in_place(&mut pa.cursor_mut(6 + dx, 9 + dy), sub);
        }
        idct_four_t4_rec_in_place_sse2(&mut pb.cursor_mut(6, 9), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_t4_rec_to_view_sse2_matches_the_scalar() {
        let mut seed = 0x1D87_2E7F_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 16] = coeffs(&mut seed);
        let pred = noisy_pred(&mut seed, 16, 4);
        let pp = pred_as_plane(&pred, 16, 4);

        idct_t4_rec(&mut pa.cursor_mut(5, 7), &pp.cursor(0, 0), &dct);

        let view = shared_plane_for_test(&mut pb);
        idct_t4_rec_to_view_sse2(&view.cursor(5, 7), &pred, 16, &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_four_t4_rec_to_view_sse2_matches_four_scalar_blocks() {
        let mut seed = 0x6C07_8965_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 64] = coeffs(&mut seed);
        let pred = noisy_pred(&mut seed, 16, 8);
        let pp = pred_as_plane(&pred, 16, 8);

        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
            idct_t4_rec(&mut pa.cursor_mut(6 + dx, 9 + dy), &pp.cursor(dx, dy), sub);
        }

        let view = shared_plane_for_test(&mut pb);
        idct_four_t4_rec_to_view_sse2(&view.cursor(6, 9), &pred, 16, &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_t4_rec_in_place_view_sse2_matches_the_scalar() {
        let mut seed = 0x41C6_4E6D_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 16] = coeffs(&mut seed);

        idct_t4_rec_in_place(&mut pa.cursor_mut(5, 7), &dct);

        let view = shared_plane_for_test(&mut pb);
        idct_t4_rec_in_place_view_sse2(&view.cursor(5, 7), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_four_t4_rec_in_place_view_sse2_matches_four_scalar_blocks() {
        let mut seed = 0x3C6E_F35F_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 64] = coeffs(&mut seed);

        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4i32..][..16]).try_into().unwrap();
            idct_t4_rec_in_place(&mut pa.cursor_mut(6 + dx, 9 + dy), sub);
        }

        let view = shared_plane_for_test(&mut pb);
        idct_four_t4_rec_in_place_view_sse2(&view.cursor(6, 9), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    /// The 16x16 form: four quadrants of four blocks, so a quadrant-level `(dx, dy)`
    /// swap and a block-level one are both visible.
    #[test]
    fn idct_t4_rec_on_mb_in_place_view_sse2_matches_sixteen_scalar_blocks() {
        let mut seed = 0x9E37_79B9_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dct: [i16; 256] = coeffs(&mut seed);

        for (q, &(qx, qy)) in QUADS.iter().enumerate() {
            for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                let off = (q << 6i32) + (k << 4i32);
                let sub: &[i16; 16] = (&dct[off..][..16]).try_into().unwrap();
                idct_t4_rec_in_place(&mut pa.cursor_mut(4 + qx + dx, 6 + qy + dy), sub);
            }
        }

        let view = shared_plane_for_test(&mut pb);
        idct_t4_rec_on_mb_in_place_view_sse2(&view.cursor(4, 6), &dct);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }

    #[test]
    fn idct_rec_i16x16_dc_to_view_sse2_matches_the_scalar() {
        let mut seed = 0xB502_6F5A_0000_0001u64;
        let (mut pa, mut pb) = twin_planes(&mut seed);
        let dc: [i16; 16] = coeffs(&mut seed);
        let pred = noisy_pred(&mut seed, 16, 16);
        let pp = pred_as_plane(&pred, 16, 16);

        idct_rec_i16x16_dc(&mut pa.cursor_mut(5, 7), &pp.cursor(0, 0), &dc);

        let view = shared_plane_for_test(&mut pb);
        idct_rec_i16x16_dc_to_view_sse2(&view.cursor(5, 7), &pred, 16, &dc);

        assert_eq!(pa.as_slice(), pb.as_slice());
    }
}
