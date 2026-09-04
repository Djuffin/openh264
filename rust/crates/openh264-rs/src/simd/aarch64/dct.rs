//! Forward 4x4 DCT and the inverse DCT with prediction add — `WelsDctT4_AArch64_neon`,
//! `WelsDctFourT4_AArch64_neon`, `WelsIDctT4Rec_AArch64_neon`,
//! `WelsIDctFourT4Rec_AArch64_neon` and `WelsIDctRecI16x16Dc_AArch64_neon` in
//! `codec/encoder/core/arm64/reconstruct_aarch64_neon.S`, and
//! `IdctResAddPred_AArch64_neon` in `codec/decoder/core/arm64/block_add_aarch64_neon.S`.
//!
//! # The IDCT's widths, and where this departs from both asm files
//!
//! The C's inverse transform (`IdctResAddPred_c`, `WelsIDctT4Rec_c`) stores its row
//! pass into `int16_t` and runs its column pass in `int`, and both halves are
//! observable on a full-range coefficient block: the narrowing wraps, the column pass
//! does not. Upstream's two arm64 IDCTs each keep one half and drop the other.
//! `WelsIDctT4Rec_AArch64_neon` is `.8h` throughout — right about the narrowing, but
//! its column pass wraps where the C saturates, the same defect the x86_64 file
//! records in `SSE2_IDCT_4x4P`. `IdctResAddPred_AArch64_neon` widens the *row* pass
//! to `.4s` and never narrows it, so it disagrees with its C on any row whose sum
//! leaves `i16`. The port's scalar keeps the C's behaviour exactly, and so does this:
//! the row pass is the encoder asm's 16-bit `ROW_TRANSFORM_1_STEP_TOTAL_16BITS`, the
//! column pass is the decoder asm's 32-bit `COL_TRANSFORM_1_STEP`, and `rshrn #6`
//! narrows the rounded result, which is exact because `|32 + t1 ± t2| <= 114720` puts
//! every residual inside `[-1792, 1792]`. `idct_vertical_pass_does_not_wrap_at_16_bits`
//! and `idct_row_pass_narrows_like_the_scalar` pin the two ends.
//!
//! Everything else is the asm as written. The forward DCT is `.8h` end to end with the
//! same `uzp`/`trn` traffic; its values are bounded by `36 * 255` and cannot overflow.
//! The DC reconstruction's `srshr #6` is the scalar's `(dc + 32) >> 6`.
//!
//! # Entry points
//!
//! Every `idct_*` entry point below is one of two residual kernels — four columns
//! from one block, or eight from two side by side (`ld4 {v.4h}` against `ld4 {v.8h}`
//! in the asm) — under a different way of reaching the prediction and the
//! destination: a plane cursor pair, one cursor in place, a `RecCursor` with an arena
//! prediction, or a `RecCursor` in place. The arithmetic exists twice, once per
//! width, and each wrapper is the load/add/`sqxtun`/store the asm does after it.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, ld8, ld8_i16, st16, st4_i16, st8, to16, to8};
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, SampleCursor};

// ============================================================================
// Forward 4x4 integer DCT
// ============================================================================

/// `DCT_ROW_TRANSFORM_TOTAL_16BITS` on four-lane vectors: one butterfly over the four
/// inputs of every line, with a line per lane.
#[inline]
#[target_feature(enable = "neon")]
fn dct_pass4(d0: int16x4_t, d1: int16x4_t, d2: int16x4_t, d3: int16x4_t) -> (int16x4_t, int16x4_t, int16x4_t, int16x4_t) {
    let s0 = vadd_s16(d0, d3);
    let s3 = vsub_s16(d0, d3);
    let s1 = vadd_s16(d1, d2);
    let s2 = vsub_s16(d1, d2);
    (
        vadd_s16(s0, s1),
        vadd_s16(vshl_n_s16::<1>(s3), s2),
        vsub_s16(s0, s1),
        vsub_s16(s3, vshl_n_s16::<1>(s2)),
    )
}

/// The eight-lane form, for two blocks side by side.
#[inline]
#[target_feature(enable = "neon")]
fn dct_pass8(d0: int16x8_t, d1: int16x8_t, d2: int16x8_t, d3: int16x8_t) -> (int16x8_t, int16x8_t, int16x8_t, int16x8_t) {
    let s0 = vaddq_s16(d0, d3);
    let s3 = vsubq_s16(d0, d3);
    let s1 = vaddq_s16(d1, d2);
    let s2 = vsubq_s16(d1, d2);
    (
        vaddq_s16(s0, s1),
        vaddq_s16(vshlq_n_s16::<1>(s3), s2),
        vsubq_s16(s0, s1),
        vsubq_s16(s3, vshlq_n_s16::<1>(s2)),
    )
}

/// `MATRIX_TRANSFORM_EACH_16BITS_OUT4` on four-lane vectors: `trn1`/`trn2` on words,
/// then on doublewords. Lane `i` of input `j` comes back as lane `j` of output `i`.
#[inline]
#[target_feature(enable = "neon")]
fn transpose4(v0: int16x4_t, v1: int16x4_t, v2: int16x4_t, v3: int16x4_t) -> (int16x4_t, int16x4_t, int16x4_t, int16x4_t) {
    let t0 = vreinterpret_s32_s16(vtrn1_s16(v0, v1));
    let t1 = vreinterpret_s32_s16(vtrn2_s16(v0, v1));
    let t2 = vreinterpret_s32_s16(vtrn1_s16(v2, v3));
    let t3 = vreinterpret_s32_s16(vtrn2_s16(v2, v3));
    (
        vreinterpret_s16_s32(vtrn1_s32(t0, t2)),
        vreinterpret_s16_s32(vtrn1_s32(t1, t3)),
        vreinterpret_s16_s32(vtrn2_s32(t0, t2)),
        vreinterpret_s16_s32(vtrn2_s32(t1, t3)),
    )
}

/// The eight-lane form: each four-lane half is transposed on its own, which is what
/// the `.8h`/`.4s` `trn` pairs do, so two blocks side by side stay side by side.
#[inline]
#[target_feature(enable = "neon")]
fn transpose8(v0: int16x8_t, v1: int16x8_t, v2: int16x8_t, v3: int16x8_t) -> (int16x8_t, int16x8_t, int16x8_t, int16x8_t) {
    let t0 = vreinterpretq_s32_s16(vtrn1q_s16(v0, v1));
    let t1 = vreinterpretq_s32_s16(vtrn2q_s16(v0, v1));
    let t2 = vreinterpretq_s32_s16(vtrn1q_s16(v2, v3));
    let t3 = vreinterpretq_s32_s16(vtrn2q_s16(v2, v3));
    (
        vreinterpretq_s16_s32(vtrn1q_s32(t0, t2)),
        vreinterpretq_s16_s32(vtrn1q_s32(t1, t3)),
        vreinterpretq_s16_s32(vtrn2q_s32(t0, t2)),
        vreinterpretq_s16_s32(vtrn2q_s32(t1, t3)),
    )
}

/// `WelsDctT4_AArch64_neon`.
///
/// The residual's sixteen bytes are `usubl`ed into `[row0 | row1]` and
/// `[row2 | row3]`, and the `uzp1`/`uzp2` pairs turn those into four column vectors
/// — lane `i` of vector `j` is `data[i][j]` — so the first `DCT_ROW_TRANSFORM` is the
/// scalar's row pass on all four rows at once. A transpose makes them row vectors and
/// the same butterfly is the column pass; the outputs are the four rows in order.
#[inline]
#[target_feature(enable = "neon")]
fn dct_4x4_neon<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    for y in 0..4 {
        a[y * 4..][..4].copy_from_slice(&pix1.row_n::<4>(y as isize, 0));
        b[y * 4..][..4].copy_from_slice(&pix2.row_n::<4>(y as isize, 0));
    }
    let (va, vb) = (ld16(&a), ld16(&b));
    let d01 = vreinterpretq_s16_u16(vsubl_u8(vget_low_u8(va), vget_low_u8(vb)));
    let d23 = vreinterpretq_s16_u16(vsubl_high_u8(va, vb));

    let even = vuzp1q_s16(d01, d23);
    let odd = vuzp2q_s16(d01, d23);
    let c02 = vuzp1q_s16(even, odd); // s[0, 4, 8, 12] [1, 5, 9, 13]
    let c13 = vuzp2q_s16(even, odd); // s[2, 6, 10, 14] [3, 7, 11, 15]
    let (c0, c1) = (vget_low_s16(c02), vget_high_s16(c02));
    let (c2, c3) = (vget_low_s16(c13), vget_high_s16(c13));

    let (r0, r1, r2, r3) = dct_pass4(c0, c1, c2, c3);
    let (t0, t1, t2, t3) = transpose4(r0, r1, r2, r3);
    let (o0, o1, o2, o3) = dct_pass4(t0, t1, t2, t3);

    st4_i16(&mut dct[0..], o0);
    st4_i16(&mut dct[4..], o1);
    st4_i16(&mut dct[8..], o2);
    st4_i16(&mut dct[12..], o3);
}

/// `WelsDctFourT4_AArch64_neon`: the 8x8 quadrant as two passes of four eight-wide
/// rows, each holding the left and right 4x4 side by side. Transpose, row pass,
/// transpose, column pass — the asm's order — then the low halves are the left block
/// and the high halves the right one.
#[inline]
#[target_feature(enable = "neon")]
fn dct_four_4x4_neon<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 64], pix1: &A, pix2: &B) {
    for k in 0..2usize {
        let mut d = [vdupq_n_s16(0); 4];
        for (j, row) in d.iter_mut().enumerate() {
            let y = (4 * k + j) as isize;
            let a = ld8(&pix1.row_n::<8>(y, 0));
            let b = ld8(&pix2.row_n::<8>(y, 0));
            *row = vreinterpretq_s16_u16(vsubl_u8(a, b));
        }
        let (c0, c1, c2, c3) = transpose8(d[0], d[1], d[2], d[3]);
        let (r0, r1, r2, r3) = dct_pass8(c0, c1, c2, c3);
        let (t0, t1, t2, t3) = transpose8(r0, r1, r2, r3);
        let (o0, o1, o2, o3) = dct_pass8(t0, t1, t2, t3);

        let left = &mut dct[k * 32..][..16];
        st4_i16(&mut left[0..], vget_low_s16(o0));
        st4_i16(&mut left[4..], vget_low_s16(o1));
        st4_i16(&mut left[8..], vget_low_s16(o2));
        st4_i16(&mut left[12..], vget_low_s16(o3));
        let right = &mut dct[k * 32 + 16..][..16];
        st4_i16(&mut right[0..], vget_high_s16(o0));
        st4_i16(&mut right[4..], vget_high_s16(o1));
        st4_i16(&mut right[8..], vget_high_s16(o2));
        st4_i16(&mut right[12..], vget_high_s16(o3));
    }
}

/// `WelsDctT4_AArch64_neon`.
#[inline]
pub fn dct_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 16], pix1: &A, pix2: &B) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { dct_4x4_neon(dct, pix1, pix2) }
}

/// `WelsDctFourT4_AArch64_neon`.
#[inline]
pub fn dct_four_4x4<A: SampleCursor, B: SampleCursor>(dct: &mut [i16; 64], pix1: &A, pix2: &B) {
    unsafe { dct_four_4x4_neon(dct, pix1, pix2) }
}

// ============================================================================
// Inverse 4x4 integer DCT
// ============================================================================

/// `ROW_TRANSFORM_1_STEP_TOTAL_16BITS` + `TRANSFORM_TOTAL_16BITS`: the row pass in
/// word lanes, a line per lane, wrapping exactly where the scalar's `as i16` does.
#[inline]
#[target_feature(enable = "neon")]
fn idct_row_pass4(c0: int16x4_t, c1: int16x4_t, c2: int16x4_t, c3: int16x4_t) -> (int16x4_t, int16x4_t, int16x4_t, int16x4_t) {
    let e0 = vadd_s16(c0, c2);
    let e1 = vsub_s16(c0, c2);
    let e2 = vsub_s16(vshr_n_s16::<1>(c1), c3);
    let e3 = vadd_s16(c1, vshr_n_s16::<1>(c3));
    (vadd_s16(e0, e3), vadd_s16(e1, e2), vsub_s16(e1, e2), vsub_s16(e0, e3))
}

#[inline]
#[target_feature(enable = "neon")]
fn idct_row_pass8(c0: int16x8_t, c1: int16x8_t, c2: int16x8_t, c3: int16x8_t) -> (int16x8_t, int16x8_t, int16x8_t, int16x8_t) {
    let e0 = vaddq_s16(c0, c2);
    let e1 = vsubq_s16(c0, c2);
    let e2 = vsubq_s16(vshrq_n_s16::<1>(c1), c3);
    let e3 = vaddq_s16(c1, vshrq_n_s16::<1>(c3));
    (vaddq_s16(e0, e3), vaddq_s16(e1, e2), vsubq_s16(e1, e2), vsubq_s16(e0, e3))
}

/// `COL_TRANSFORM_1_STEP` + `TRANSFORM_4BYTES` (block_add): the column pass widened
/// to `.4s` on the way in, the `>> 1` taken on the word before widening as the
/// scalar's `tmp[..] as i32 >> 1` is.
#[inline]
#[target_feature(enable = "neon")]
fn idct_col_pass4(g0: int16x4_t, g1: int16x4_t, g2: int16x4_t, g3: int16x4_t) -> (int32x4_t, int32x4_t, int32x4_t, int32x4_t) {
    let h0 = vaddl_s16(g0, g2);
    let h1 = vsubl_s16(g0, g2);
    let h2 = vsubl_s16(vshr_n_s16::<1>(g1), g3);
    let h3 = vaddl_s16(g1, vshr_n_s16::<1>(g3));
    (vaddq_s32(h0, h3), vaddq_s32(h1, h2), vsubq_s32(h1, h2), vsubq_s32(h0, h3))
}

/// `(v + 32) >> 6` of four column-pass rows, narrowed and paired: `([row0 | row1],
/// [row2 | row3])`.
#[inline]
#[target_feature(enable = "neon")]
fn round_pairs(r0: int32x4_t, r1: int32x4_t, r2: int32x4_t, r3: int32x4_t) -> (int16x8_t, int16x8_t) {
    (
        vcombine_s16(vrshrn_n_s32::<6>(r0), vrshrn_n_s32::<6>(r1)),
        vcombine_s16(vrshrn_n_s32::<6>(r2), vrshrn_n_s32::<6>(r3)),
    )
}

/// The rounded residual of one 4x4 block: `([row0 | row1], [row2 | row3])`.
#[inline]
#[target_feature(enable = "neon")]
fn residual4(dct: &[i16; 16]) -> (int16x8_t, int16x8_t) {
    // SAFETY: sixteen coefficients, which is what `ld4 {v.4h}` de-interleaves.
    let c = unsafe { vld4_s16(dct.as_ptr()) };
    let (f0, f1, f2, f3) = idct_row_pass4(c.0, c.1, c.2, c.3);
    let (g0, g1, g2, g3) = transpose4(f0, f1, f2, f3);
    let (r0, r1, r2, r3) = idct_col_pass4(g0, g1, g2, g3);
    round_pairs(r0, r1, r2, r3)
}

/// The rounded residual of two horizontally adjacent blocks, row `j` as
/// `[left row j | right row j]` — `ld4 {v.8h}` puts the left block's columns in
/// the low lanes and the right block's in the high ones.
#[inline]
#[target_feature(enable = "neon")]
fn residual8(dct: &[i16; 32]) -> [int16x8_t; 4] {
    // SAFETY: thirty-two coefficients, which is what `ld4 {v.8h}` de-interleaves.
    let c = unsafe { vld4q_s16(dct.as_ptr()) };
    let (f0, f1, f2, f3) = idct_row_pass8(c.0, c.1, c.2, c.3);
    let (g0, g1, g2, g3) = transpose8(f0, f1, f2, f3);
    let (l0, l1, l2, l3) = idct_col_pass4(vget_low_s16(g0), vget_low_s16(g1), vget_low_s16(g2), vget_low_s16(g3));
    let (h0, h1, h2, h3) = idct_col_pass4(vget_high_s16(g0), vget_high_s16(g1), vget_high_s16(g2), vget_high_s16(g3));
    [
        vcombine_s16(vrshrn_n_s32::<6>(l0), vrshrn_n_s32::<6>(h0)),
        vcombine_s16(vrshrn_n_s32::<6>(l1), vrshrn_n_s32::<6>(h1)),
        vcombine_s16(vrshrn_n_s32::<6>(l2), vrshrn_n_s32::<6>(h2)),
        vcombine_s16(vrshrn_n_s32::<6>(l3), vrshrn_n_s32::<6>(h3)),
    ]
}

/// `uxtl`, `add`, `sqxtun`: eight prediction bytes plus eight residuals, saturated.
#[inline]
#[target_feature(enable = "neon")]
fn add_clip8(pred: uint8x8_t, res: int16x8_t) -> uint8x8_t {
    vqmovun_s16(vaddq_s16(vreinterpretq_s16_u16(vmovl_u8(pred)), res))
}

/// Two four-byte rows as one eight-lane vector.
#[inline]
#[target_feature(enable = "neon")]
fn pair4(a: &[u8], b: &[u8]) -> uint8x8_t {
    let mut t = [0u8; 8];
    t[..4].copy_from_slice(&a[..4]);
    t[4..].copy_from_slice(&b[..4]);
    ld8(&t)
}

/// `IdctResAddPred_AArch64_neon`: in place on `pred`, rows read before they are written.
#[inline]
#[target_feature(enable = "neon")]
fn idct_res_add_pred_neon(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    let (r01, r23) = residual4(rs);
    let p01 = pair4(pred.row(0, 0, 4), pred.row(1, 0, 4));
    let p23 = pair4(pred.row(2, 0, 4), pred.row(3, 0, 4));
    let o01 = to8(add_clip8(p01, r01));
    let o23 = to8(add_clip8(p23, r23));
    pred.row_mut(0, 0, 4).copy_from_slice(&o01[..4]);
    pred.row_mut(1, 0, 4).copy_from_slice(&o01[4..]);
    pred.row_mut(2, 0, 4).copy_from_slice(&o23[..4]);
    pred.row_mut(3, 0, 4).copy_from_slice(&o23[4..]);
}

/// `WelsIDctT4Rec_AArch64_neon`: prediction from one cursor, reconstruction to another.
#[inline]
#[target_feature(enable = "neon")]
fn idct_t4_rec_neon(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 16]) {
    let (r01, r23) = residual4(dct);
    let p01 = pair4(pred.row(0, 0, 4), pred.row(1, 0, 4));
    let p23 = pair4(pred.row(2, 0, 4), pred.row(3, 0, 4));
    let o01 = to8(add_clip8(p01, r01));
    let o23 = to8(add_clip8(p23, r23));
    rec.row_mut(0, 0, 4).copy_from_slice(&o01[..4]);
    rec.row_mut(1, 0, 4).copy_from_slice(&o01[4..]);
    rec.row_mut(2, 0, 4).copy_from_slice(&o23[..4]);
    rec.row_mut(3, 0, 4).copy_from_slice(&o23[4..]);
}

/// `WelsIDctFourT4Rec_AArch64_neon`: two `ld4 {v.8h}` passes, four eight-byte rows each.
#[inline]
#[target_feature(enable = "neon")]
fn idct_four_t4_rec_neon(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 64]) {
    for k in 0..2usize {
        let sub: &[i16; 32] = (&dct[k * 32..][..32]).try_into().expect("two blocks");
        let rows = residual8(sub);
        for (j, res) in rows.into_iter().enumerate() {
            let y = (4 * k + j) as isize;
            let out = add_clip8(ld8(pred.row(y, 0, 8)), res);
            st8(rec.row_mut(y, 0, 8), out);
        }
    }
}

/// The in-place form of [`idct_four_t4_rec_neon`].
#[inline]
#[target_feature(enable = "neon")]
fn idct_four_t4_rec_in_place_neon(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    for k in 0..2usize {
        let sub: &[i16; 32] = (&dct[k * 32..][..32]).try_into().expect("two blocks");
        let rows = residual8(sub);
        for (j, res) in rows.into_iter().enumerate() {
            let y = (4 * k + j) as isize;
            let out = add_clip8(ld8(rec.row(y, 0, 8)), res);
            st8(rec.row_mut(y, 0, 8), out);
        }
    }
}

/// The seam form: an arena prediction at `pred_stride`, written through `write_row`.
#[inline]
#[target_feature(enable = "neon")]
fn idct_t4_rec_to_view_neon(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 16]) {
    let (r01, r23) = residual4(dct);
    let p01 = pair4(pred, &pred[pred_stride..]);
    let p23 = pair4(&pred[2 * pred_stride..], &pred[3 * pred_stride..]);
    let o01 = to8(add_clip8(p01, r01));
    let o23 = to8(add_clip8(p23, r23));
    rec.write_row::<4>(0, 0, o01[..4].try_into().expect("row"));
    rec.write_row::<4>(1, 0, o01[4..].try_into().expect("row"));
    rec.write_row::<4>(2, 0, o23[..4].try_into().expect("row"));
    rec.write_row::<4>(3, 0, o23[4..].try_into().expect("row"));
}

#[inline]
#[target_feature(enable = "neon")]
fn idct_four_t4_rec_to_view_neon(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 64]) {
    for k in 0..2usize {
        let sub: &[i16; 32] = (&dct[k * 32..][..32]).try_into().expect("two blocks");
        let rows = residual8(sub);
        for (j, res) in rows.into_iter().enumerate() {
            let y = 4 * k + j;
            let out = add_clip8(ld8(&pred[y * pred_stride..]), res);
            rec.write_row::<8>(y as isize, 0, &to8(out));
        }
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn idct_t4_rec_in_place_view_neon(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    let (r01, r23) = residual4(dct);
    let p01 = pair4(&rec.row::<4>(0, 0), &rec.row::<4>(1, 0));
    let p23 = pair4(&rec.row::<4>(2, 0), &rec.row::<4>(3, 0));
    let o01 = to8(add_clip8(p01, r01));
    let o23 = to8(add_clip8(p23, r23));
    rec.write_row::<4>(0, 0, o01[..4].try_into().expect("row"));
    rec.write_row::<4>(1, 0, o01[4..].try_into().expect("row"));
    rec.write_row::<4>(2, 0, o23[..4].try_into().expect("row"));
    rec.write_row::<4>(3, 0, o23[4..].try_into().expect("row"));
}

#[inline]
#[target_feature(enable = "neon")]
fn idct_four_t4_rec_in_place_view_neon(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    for k in 0..2usize {
        let sub: &[i16; 32] = (&dct[k * 32..][..32]).try_into().expect("two blocks");
        let rows = residual8(sub);
        for (j, res) in rows.into_iter().enumerate() {
            let y = (4 * k + j) as isize;
            let out = add_clip8(ld8(&rec.row::<8>(y, 0)), res);
            rec.write_row::<8>(y, 0, &to8(out));
        }
    }
}

/// `WelsIDctRecI16x16Dc_AArch64_neon`'s per-row body: `uxtl`/`uxtl2` the prediction,
/// add the four-lane-wide DC pairs, `sqxtun`/`sqxtun2`.
#[inline]
#[target_feature(enable = "neon")]
fn dc_row(pred: uint8x16_t, lo: int16x8_t, hi: int16x8_t) -> uint8x16_t {
    let a = vaddq_s16(vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(pred))), lo);
    let b = vaddq_s16(vreinterpretq_s16_u16(vmovl_high_u8(pred)), hi);
    vcombine_u8(vqmovun_s16(a), vqmovun_s16(b))
}

/// The sixteen `(dc + 32) >> 6` values — `srshr #6` — as an array, so each group of
/// four rows can `dup` its four.
#[inline]
#[target_feature(enable = "neon")]
fn rounded_dc(dc: &[i16; 16]) -> [i16; 16] {
    let mut out = [0i16; 16];
    super::lanes::st8_i16(&mut out[..8], vrshrq_n_s16::<6>(ld8_i16(&dc[..8])));
    super::lanes::st8_i16(&mut out[8..], vrshrq_n_s16::<6>(ld8_i16(&dc[8..])));
    out
}

/// `WelsIDctRecI16x16Dc_AArch64_neon`.
#[inline]
#[target_feature(enable = "neon")]
fn idct_rec_i16x16_dc_neon(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dc: &[i16; 16]) {
    let d = rounded_dc(dc);
    for g in 0..4usize {
        let lo = vcombine_s16(vdup_n_s16(d[4 * g]), vdup_n_s16(d[4 * g + 1]));
        let hi = vcombine_s16(vdup_n_s16(d[4 * g + 2]), vdup_n_s16(d[4 * g + 3]));
        for j in 0..4 {
            let y = (4 * g + j) as isize;
            let out = dc_row(ld16(pred.row(y, 0, 16)), lo, hi);
            st16(rec.row_mut(y, 0, 16), out);
        }
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn idct_rec_i16x16_dc_to_view_neon(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dc: &[i16; 16]) {
    let d = rounded_dc(dc);
    for g in 0..4usize {
        let lo = vcombine_s16(vdup_n_s16(d[4 * g]), vdup_n_s16(d[4 * g + 1]));
        let hi = vcombine_s16(vdup_n_s16(d[4 * g + 2]), vdup_n_s16(d[4 * g + 3]));
        for j in 0..4 {
            let y = 4 * g + j;
            let out = dc_row(ld16(&pred[y * pred_stride..]), lo, hi);
            rec.write_row::<16>(y as isize, 0, &to16(out));
        }
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// `IdctResAddPred_AArch64_neon`, with the widths the header explains.
#[inline]
pub fn idct_res_add_pred(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { idct_res_add_pred_neon(pred, rs) }
}

/// `WelsIDctT4Rec_AArch64_neon`, with the widths the header explains.
#[inline]
pub fn idct_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 16]) {
    unsafe { idct_t4_rec_neon(rec, pred, dct) }
}

/// [`idct_t4_rec`] in place on `rec`.
#[inline]
pub fn idct_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    unsafe { idct_res_add_pred_neon(rec, dct) }
}

/// `WelsIDctFourT4Rec_AArch64_neon`.
#[inline]
pub fn idct_four_t4_rec(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_neon(rec, pred, dct) }
}

/// [`idct_t4_rec_in_place`] over four 4x4 blocks forming an 8x8 quadrant.
#[inline]
pub fn idct_four_t4_rec_in_place(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_in_place_neon(rec, dct) }
}

/// [`idct_t4_rec`]'s seam flavour.
#[inline]
pub fn idct_t4_rec_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 16]) {
    unsafe { idct_t4_rec_to_view_neon(rec, pred, pred_stride, dct) }
}

/// [`idct_four_t4_rec`]'s seam flavour.
#[inline]
pub fn idct_four_t4_rec_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_to_view_neon(rec, pred, pred_stride, dct) }
}

/// [`idct_t4_rec_in_place`]'s seam flavour.
#[inline]
pub fn idct_t4_rec_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    unsafe { idct_t4_rec_in_place_view_neon(rec, dct) }
}

/// [`idct_four_t4_rec_in_place`]'s seam flavour.
#[inline]
pub fn idct_four_t4_rec_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_in_place_view_neon(rec, dct) }
}

/// The 16x16 luma inter reconstruction: four quadrants of [`idct_four_t4_rec_in_place_view`].
#[inline]
pub fn idct_t4_rec_on_mb_in_place_view(rec: &RecCursor<'_>, dct: &[i16; 256]) {
    const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];
    for (k, &(dx, dy)) in QUADS.iter().enumerate() {
        let sub: &[i16; 64] = (&dct[k << 6..][..64]).try_into().expect("quadrant");
        unsafe { idct_four_t4_rec_in_place_view_neon(&rec.advance(dx, dy), sub) }
    }
}

/// `WelsIDctRecI16x16Dc_AArch64_neon`.
#[inline]
pub fn idct_rec_i16x16_dc(rec: &mut PlaneCursorMut<'_>, pred: &PlaneCursor<'_>, dc: &[i16; 16]) {
    unsafe { idct_rec_i16x16_dc_neon(rec, pred, dc) }
}

/// [`idct_rec_i16x16_dc`]'s seam flavour.
#[inline]
pub fn idct_rec_i16x16_dc_to_view(rec: &RecCursor<'_>, pred: &[u8], pred_stride: usize, dc: &[i16; 16]) {
    unsafe { idct_rec_i16x16_dc_to_view_neon(rec, pred, pred_stride, dc) }
}

// ============================================================================
// Unit Tests & Parity
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers: the
    // dispatchers route to the very kernels under test.
    use crate::decoder::decode_mb_aux::idct_res_add_pred_c;
    use crate::encoder::decode_mb_aux::{idct_rec_i16x16_dc_c, idct_t4_rec_c, idct_t4_rec_in_place_c};
    use crate::encoder::encode_mb_aux as enc;
    use crate::encoder::rec_view::shared_plane_for_test;
    use crate::safe::plane::PaddedPlane;

    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 32) & 0xFF) as u8
    }

    /// Coefficients over the **full `i16` range**, which is what the decoder hands the
    /// IDCT: `rs` comes from the bitstream by way of dequantisation.
    fn lcg_i16(seed: &mut u64) -> i16 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u16 as i16
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
            let mut want = [0i16; 16];
            let mut got = [0i16; 16];
            enc::dct_4x4(&mut want, &p1.cursor(0, 0), &p2.cursor(0, 0));
            dct_4x4(&mut got, &p1.cursor(0, 0), &p2.cursor(0, 0));
            assert_eq!(got, want);
        }
    }

    /// The ends of the residual range — every pixel 255 against 0 and back — reach
    /// the DCT's `36 * 255` bound, where a lane that had widened wrongly would show.
    #[test]
    fn dct_4x4_at_the_residual_extremes() {
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        for &(a, b) in &[(255u8, 0u8), (0, 255), (255, 255), (0, 0)] {
            let mut p1 = PaddedPlane::new(w, h, pad, stride);
            let mut p2 = PaddedPlane::new(w, h, pad, stride);
            for y in 0..8isize {
                for x in 0..8isize {
                    // A checkerboard of the two extremes, so every transform output is large.
                    let flip = (x + y) & 1 == 0;
                    p1.set(x, y, if flip { a } else { b });
                    p2.set(x, y, if flip { b } else { a });
                }
            }
            let mut want = [0i16; 64];
            let mut got = [0i16; 64];
            enc::dct_four_4x4(&mut want, &p1.cursor(0, 0), &p2.cursor(0, 0));
            dct_four_4x4(&mut got, &p1.cursor(0, 0), &p2.cursor(0, 0));
            assert_eq!(got, want, "({a}, {b})");
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
            let mut want = [0i16; 64];
            let mut got = [0i16; 64];
            enc::dct_four_4x4(&mut want, &p1.cursor(0, 0), &p2.cursor(0, 0));
            dct_four_4x4(&mut got, &p1.cursor(0, 0), &p2.cursor(0, 0));
            assert_eq!(got, want);
        }
    }

    /// The forward DCT through the shared cursor, whose `row_n` copies out of cells.
    #[test]
    fn dct_parity_through_the_shared_cursor() {
        let mut seed = 777u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p1 = PaddedPlane::new(w, h, pad, stride);
        let mut p2 = PaddedPlane::new(w, h, pad, stride);
        for y in 0..8isize {
            for x in 0..8isize {
                p1.set(x, y, lcg(&mut seed));
                p2.set(x, y, lcg(&mut seed));
            }
        }
        let mut want = [0i16; 64];
        enc::dct_four_4x4(&mut want, &p1.cursor(0, 0), &p2.cursor(0, 0));
        let v1 = shared_plane_for_test(&mut p1);
        let v2 = shared_plane_for_test(&mut p2);
        let mut got = [0i16; 64];
        dct_four_4x4(&mut got, &v1.cursor(0, 0), &v2.cursor(0, 0));
        assert_eq!(got, want);
        let mut got16 = [0i16; 16];
        dct_4x4(&mut got16, &v1.cursor(4, 4), &v2.cursor(4, 4));
        assert_eq!(got16, want[48..64]);
    }

    #[test]
    fn test_idct_res_add_pred_parity() {
        let mut seed = 98765u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p_c = PaddedPlane::new(w, h, pad, stride);
        let mut p_simd = PaddedPlane::new(w, h, pad, stride);
        for _ in 0..200 {
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
            idct_res_add_pred_c(&mut p_c.cursor_mut(0, 0), &rs);
            idct_res_add_pred(&mut p_simd.cursor_mut(0, 0), &rs);
            assert_eq!(p_simd.as_slice(), p_c.as_slice());
        }
    }

    /// `rs[0] = rs[8] = 20000` with a zero prediction puts `t1 = s0 + s8 = 40000` into
    /// the column pass. In `i32` that is `(32 + 40000) >> 6 = 625`, clipped to 255; in
    /// 16-bit lanes — `WelsIDctT4Rec_AArch64_neon`'s — it wraps to black.
    #[test]
    fn idct_vertical_pass_does_not_wrap_at_16_bits() {
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut p_c = PaddedPlane::new(w, h, pad, stride);
        let mut p_simd = PaddedPlane::new(w, h, pad, stride);
        let mut rs = [0i16; 16];
        rs[0] = 20000;
        rs[8] = 20000;
        idct_res_add_pred_c(&mut p_c.cursor_mut(0, 0), &rs);
        idct_res_add_pred(&mut p_simd.cursor_mut(0, 0), &rs);
        assert_eq!(p_c.at(0, 0), 255, "the scalar reference itself moved");
        assert_eq!(p_simd.as_slice(), p_c.as_slice());
    }

    /// The other end: a row whose sum leaves `i16`. `rs[0] = rs[2] = 32767` makes
    /// `t0 = 65534` and `iSrc[0] = t0 + t3` wraps in the C's `int16_t` — which the
    /// scalar keeps and `IdctResAddPred_AArch64_neon`'s `.4s` row pass would not.
    #[test]
    fn idct_row_pass_narrows_like_the_scalar() {
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        for &(a, c) in &[(32767i16, 32767i16), (-32768, -32768), (32767, 1), (-32768, 32767)] {
            let mut p_c = PaddedPlane::new(w, h, pad, stride);
            let mut p_simd = PaddedPlane::new(w, h, pad, stride);
            for y in 0..4isize {
                for x in 0..4isize {
                    p_c.set(x, y, 128);
                    p_simd.set(x, y, 128);
                }
            }
            let mut rs = [0i16; 16];
            rs[0] = a;
            rs[2] = c;
            rs[5] = a;
            rs[7] = c;
            idct_res_add_pred_c(&mut p_c.cursor_mut(0, 0), &rs);
            idct_res_add_pred(&mut p_simd.cursor_mut(0, 0), &rs);
            assert_eq!(p_simd.as_slice(), p_c.as_slice(), "({a}, {c})");
        }
    }

    #[test]
    fn test_idct_t4_rec_parity() {
        let mut seed = 112233u64;
        let (w, h, pad, stride) = (16usize, 16usize, 16usize, 64usize);
        let mut pred = PaddedPlane::new(w, h, pad, stride);
        for y in 0..4isize {
            for x in 0..4isize {
                pred.set(x, y, lcg(&mut seed));
            }
        }
        let mut rec_c = PaddedPlane::new(w, h, pad, stride);
        let mut rec_simd = PaddedPlane::new(w, h, pad, stride);
        for _ in 0..200 {
            let mut rs = [0i16; 16];
            for v in rs.iter_mut() {
                *v = lcg_i16(&mut seed);
            }
            idct_t4_rec_c(&mut rec_c.cursor_mut(0, 0), &pred.cursor(0, 0), &rs);
            idct_t4_rec(&mut rec_simd.cursor_mut(0, 0), &pred.cursor(0, 0), &rs);
            assert_eq!(rec_simd.as_slice(), rec_c.as_slice());
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
        for _ in 0..40 {
            let mut dc = [0i16; 16];
            for v in dc.iter_mut() {
                *v = lcg_i16(&mut seed);
            }
            idct_rec_i16x16_dc_c(&mut rec_c.cursor_mut(0, 0), &pred.cursor(0, 0), &dc);
            idct_rec_i16x16_dc(&mut rec_simd.cursor_mut(0, 0), &pred.cursor(0, 0), &dc);
            assert_eq!(rec_simd.as_slice(), rec_c.as_slice());
        }
    }

    // ========================================================================
    // The multi-block and reconstruction-seam entry points, each referenced against
    // the scalar applied per block at the sub-offsets, over whole allocations.
    // ========================================================================

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

    fn noisy_pred(seed: &mut u64, stride: usize, rows: usize) -> Vec<u8> {
        (0..stride * rows).map(|_| lcg(seed)).collect()
    }

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
    fn idct_t4_rec_in_place_matches_the_scalar() {
        let mut seed = 0x0DDB_1A5E_5BAD_5EEDu64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 16] = coeffs(&mut seed);
            idct_t4_rec_in_place_c(&mut pa.cursor_mut(5, 7), &dct);
            idct_t4_rec_in_place(&mut pb.cursor_mut(5, 7), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_four_t4_rec_matches_four_scalar_blocks() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 64] = coeffs(&mut seed);
            let pred = noisy_pred(&mut seed, 16, 8);
            let pp = pred_as_plane(&pred, 16, 8);
            for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
                idct_t4_rec_c(&mut pa.cursor_mut(6 + dx, 9 + dy), &pp.cursor(dx, dy), sub);
            }
            idct_four_t4_rec(&mut pb.cursor_mut(6, 9), &pp.cursor(0, 0), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_four_t4_rec_in_place_matches_four_scalar_blocks() {
        let mut seed = 0x8A5C_D789_635D_2DFFu64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 64] = coeffs(&mut seed);
            for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
                idct_t4_rec_in_place_c(&mut pa.cursor_mut(6 + dx, 9 + dy), sub);
            }
            idct_four_t4_rec_in_place(&mut pb.cursor_mut(6, 9), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_t4_rec_to_view_matches_the_scalar() {
        let mut seed = 0x1D87_2E7F_0000_0001u64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 16] = coeffs(&mut seed);
            let pred = noisy_pred(&mut seed, 16, 4);
            let pp = pred_as_plane(&pred, 16, 4);
            idct_t4_rec_c(&mut pa.cursor_mut(5, 7), &pp.cursor(0, 0), &dct);
            let view = shared_plane_for_test(&mut pb);
            idct_t4_rec_to_view(&view.cursor(5, 7), &pred, 16, &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_four_t4_rec_to_view_matches_four_scalar_blocks() {
        let mut seed = 0x6C07_8965_0000_0001u64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 64] = coeffs(&mut seed);
            let pred = noisy_pred(&mut seed, 16, 8);
            let pp = pred_as_plane(&pred, 16, 8);
            for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
                idct_t4_rec_c(&mut pa.cursor_mut(6 + dx, 9 + dy), &pp.cursor(dx, dy), sub);
            }
            let view = shared_plane_for_test(&mut pb);
            idct_four_t4_rec_to_view(&view.cursor(6, 9), &pred, 16, &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_t4_rec_in_place_view_matches_the_scalar() {
        let mut seed = 0x41C6_4E6D_0000_0001u64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 16] = coeffs(&mut seed);
            idct_t4_rec_in_place_c(&mut pa.cursor_mut(5, 7), &dct);
            let view = shared_plane_for_test(&mut pb);
            idct_t4_rec_in_place_view(&view.cursor(5, 7), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_four_t4_rec_in_place_view_matches_four_scalar_blocks() {
        let mut seed = 0x3C6E_F35F_0000_0001u64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 64] = coeffs(&mut seed);
            for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
                idct_t4_rec_in_place_c(&mut pa.cursor_mut(6 + dx, 9 + dy), sub);
            }
            let view = shared_plane_for_test(&mut pb);
            idct_four_t4_rec_in_place_view(&view.cursor(6, 9), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_t4_rec_on_mb_in_place_view_matches_sixteen_scalar_blocks() {
        let mut seed = 0x9E37_79B9_0000_0001u64;
        for _ in 0..10 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dct: [i16; 256] = coeffs(&mut seed);
            for (q, &(qx, qy)) in QUADS.iter().enumerate() {
                for (k, &(dx, dy)) in SUBS.iter().enumerate() {
                    let off = (q << 6) + (k << 4);
                    let sub: &[i16; 16] = (&dct[off..][..16]).try_into().unwrap();
                    idct_t4_rec_in_place_c(&mut pa.cursor_mut(4 + qx + dx, 6 + qy + dy), sub);
                }
            }
            let view = shared_plane_for_test(&mut pb);
            idct_t4_rec_on_mb_in_place_view(&view.cursor(4, 6), &dct);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }

    #[test]
    fn idct_rec_i16x16_dc_to_view_matches_the_scalar() {
        let mut seed = 0xB502_6F5A_0000_0001u64;
        for _ in 0..20 {
            let (mut pa, mut pb) = twin_planes(&mut seed);
            let dc: [i16; 16] = coeffs(&mut seed);
            let pred = noisy_pred(&mut seed, 16, 16);
            let pp = pred_as_plane(&pred, 16, 16);
            idct_rec_i16x16_dc_c(&mut pa.cursor_mut(5, 7), &pp.cursor(0, 0), &dc);
            let view = shared_plane_for_test(&mut pb);
            idct_rec_i16x16_dc_to_view(&view.cursor(5, 7), &pred, 16, &dc);
            assert_eq!(pa.as_slice(), pb.as_slice());
        }
    }
}
