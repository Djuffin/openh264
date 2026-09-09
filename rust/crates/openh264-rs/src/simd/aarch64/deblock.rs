//! Deblocking — `DeblockLuma{Lt4,Eq4}{V,H}_AArch64_neon` and
//! `DeblockChroma{Lt4,Eq4}{V,H}_AArch64_neon`, `codec/common/arm64/deblocking_aarch64_neon.S`.
//!
//! # Byte lanes, not words
//!
//! Where the x86_64 kernels widen every sample to a word and clip with `max`/`min`
//! against 255, the asm stays in byte lanes throughout, and so does this. The
//! filter conditions are `uabd` and `cmhi` on bytes (`MASK_MATRIX`); the weak
//! filter's `p1`/`q1` delta is `urhadd`/`uhadd` then `usubl`/`sqxtn` into a
//! signed byte, clipped with `smax`/`smin` against `±tc0`; the `p0`/`q0` delta is
//! `usubl`s and a `shl` into words and one `sqrshrn #3` back; and the clip to
//! `[0, 255]` is the `EXTRACT_DELTA_INTO_TWO_PART` trick — the delta split into a
//! non-negative part and a non-positive part so that `uqadd`/`uqsub` saturate it in.
//! The strong filter's taps are `uaddl`/`uaddw` sums and `rshrn`s, and `bsl`
//! selects between the strong, weak and unfiltered samples, which is the scalar's
//! `if` tree as masks.
//!
//! The 16-line kernels take their six (or eight, or four) tap rows as `[u8; 16]`
//! arrays; the edge-direction wrappers at the bottom gather those rows from the
//! cursor and write back only the taps the filter may change, exactly as the x86_64
//! wrappers do, and their preconditions apply here too. For a vertical edge that is
//! a transpose each way. The asm does it in the load unit, with `ld3`/`ld4`/`st4`
//! lane loads straight from the picture, which `PlaneSamples` cannot express; here
//! the lines are gathered as eight-byte rows and turned into tap vectors by an 8x8
//! byte transpose — `trn1`/`trn2` on bytes, then halfwords, then words — which is
//! its own inverse and so also brings them back.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{any_set, ld4, ld8, ld8_i16, ld16, st16, to8};
use crate::safe::plane::{BlockRows, PlaneSamples, RefSamples};

// ============================================================================
// Lane helpers
// ============================================================================

/// `MASK_MATRIX`: `|p0 - q0| < alpha`, `|p1 - p0| < beta`, `|q1 - q0| < beta`.
#[inline]
#[target_feature(enable = "neon")]
fn mask_matrix(
    p1: uint8x16_t,
    p0: uint8x16_t,
    q0: uint8x16_t,
    q1: uint8x16_t,
    alpha: uint8x16_t,
    beta: uint8x16_t,
) -> uint8x16_t {
    let m = vcgtq_u8(alpha, vabdq_u8(p0, q0));
    let m = vandq_u8(m, vcgtq_u8(beta, vabdq_u8(p1, p0)));
    vandq_u8(m, vcgtq_u8(beta, vabdq_u8(q1, q0)))
}

/// `DIFF_LUMA_LT4_P1_Q1`: the new `p1` (or `q1`, with the taps mirrored) and the
/// `|p2 - p0| < beta` condition that gated it, which the caller turns into a `tc`
/// increment.
///
/// `(p2 + ((p0 + q0 + 1) >> 1)) >> 1` is `urhadd` then `uhadd`; the `- p1` is a
/// `usubl` into words and a `sqxtn` back, which is where the difference turns
/// signed; then `smax`/`smin` against `±tc0`.
#[inline]
#[target_feature(enable = "neon")]
fn lt4_p1(
    p2: uint8x16_t,
    p1: uint8x16_t,
    p0: uint8x16_t,
    q0: uint8x16_t,
    beta: uint8x16_t,
    neg_tc0: int8x16_t,
    tc0: int8x16_t,
    flag: uint8x16_t,
) -> (uint8x16_t, uint8x16_t) {
    let t = vhaddq_u8(p2, vrhaddq_u8(p0, q0));
    let d_lo = vqmovn_s16(vreinterpretq_s16_u16(vsubl_u8(
        vget_low_u8(t),
        vget_low_u8(p1),
    )));
    let d_hi = vqmovn_s16(vreinterpretq_s16_u16(vsubl_high_u8(t, p1)));
    let d = vminq_s8(vmaxq_s8(vcombine_s8(d_lo, d_hi), neg_tc0), tc0);
    let cond = vcgtq_u8(beta, vabdq_u8(p2, p0));
    let d = vandq_s8(d, vreinterpretq_s8_u8(vandq_u8(cond, flag)));
    (vaddq_u8(p1, vreinterpretq_u8_s8(d)), cond)
}

/// `DIFF_LUMA_LT4_P0_Q0`: `(((q0 - p0) << 2) + (p1 - q1) + 4) >> 3`, saturated to a
/// signed byte by `sqrshrn #3`.
#[inline]
#[target_feature(enable = "neon")]
fn lt4_delta(p1: uint8x16_t, p0: uint8x16_t, q0: uint8x16_t, q1: uint8x16_t) -> int8x16_t {
    let lo = vaddq_s16(
        vreinterpretq_s16_u16(vsubl_u8(vget_low_u8(p1), vget_low_u8(q1))),
        vshlq_n_s16::<2>(vreinterpretq_s16_u16(vsubl_u8(
            vget_low_u8(q0),
            vget_low_u8(p0),
        ))),
    );
    let hi = vaddq_s16(
        vreinterpretq_s16_u16(vsubl_high_u8(p1, q1)),
        vshlq_n_s16::<2>(vreinterpretq_s16_u16(vsubl_high_u8(q0, p0))),
    );
    vcombine_s8(vqrshrn_n_s16::<3>(lo), vqrshrn_n_s16::<3>(hi))
}

/// `EXTRACT_DELTA_INTO_TWO_PART`: `delta` as `pos - neg` with both non-negative, so
/// `p0 + delta` is `uqsub(uqadd(p0, pos), neg)` and saturates to `[0, 255]` on
/// either side.
#[inline]
#[target_feature(enable = "neon")]
fn split_delta(delta: int8x16_t) -> (uint8x16_t, uint8x16_t) {
    let raw = vreinterpretq_u8_s8(delta);
    let pos = vandq_u8(raw, vcgezq_s8(delta));
    (pos, vsubq_u8(pos, raw))
}

/// `DIFF_LUMA_EQ4_P2P1P0` on eight lanes: the strong filter's `p0`, `p1` and `p2`
/// for one side, with `p0` already selected between its strong and weak forms by
/// `strong`. Mirror the taps for the `q` side.
#[inline]
#[target_feature(enable = "neon")]
fn eq4_half(
    p3: uint8x8_t,
    p2: uint8x8_t,
    p1: uint8x8_t,
    p0: uint8x8_t,
    q0: uint8x8_t,
    q1: uint8x8_t,
    strong: uint8x8_t,
) -> (uint8x8_t, uint8x8_t, uint8x8_t) {
    let s = vaddq_u16(vaddl_u8(p2, p1), vaddl_u8(p0, q0)); // p2 + p1 + p0 + q0
    let t = vaddq_u16(vshlq_n_u16::<1>(vaddl_u8(p3, p2)), s); // 2 p3 + 3 p2 + p1 + p0 + q0
    let p1n = vrshrn_n_u16::<2>(s);
    let p2n = vrshrn_n_u16::<3>(t);
    let u = vaddq_u16(vsubl_u8(q1, p2), vshlq_n_u16::<1>(s)); // p2 + 2 p1 + 2 p0 + 2 q0 + q1
    let w = vaddw_u8(vaddw_u8(vaddl_u8(p1, q1), p1), p0); // 2 p1 + p0 + q1
    let p0c = vbsl_u8(strong, vrshrn_n_u16::<3>(u), vrshrn_n_u16::<2>(w));
    (p0c, p1n, p2n)
}

#[inline]
#[target_feature(enable = "neon")]
fn eq4_side(
    p3: uint8x16_t,
    p2: uint8x16_t,
    p1: uint8x16_t,
    p0: uint8x16_t,
    q0: uint8x16_t,
    q1: uint8x16_t,
    strong: uint8x16_t,
) -> (uint8x16_t, uint8x16_t, uint8x16_t) {
    let (a0, a1, a2) = eq4_half(
        vget_low_u8(p3),
        vget_low_u8(p2),
        vget_low_u8(p1),
        vget_low_u8(p0),
        vget_low_u8(q0),
        vget_low_u8(q1),
        vget_low_u8(strong),
    );
    let (b0, b1, b2) = eq4_half(
        vget_high_u8(p3),
        vget_high_u8(p2),
        vget_high_u8(p1),
        vget_high_u8(p0),
        vget_high_u8(q0),
        vget_high_u8(q1),
        vget_high_u8(strong),
    );
    (
        vcombine_u8(a0, b0),
        vcombine_u8(a1, b1),
        vcombine_u8(a2, b2),
    )
}

/// `DIFF_CHROMA_EQ4_P0Q0` on eight lanes: `(2 p1 + p0 + q1 + 2) >> 2` and its mirror.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_eq4_half(
    p1: uint8x8_t,
    p0: uint8x8_t,
    q0: uint8x8_t,
    q1: uint8x8_t,
) -> (uint8x8_t, uint8x8_t) {
    let v = vshlq_n_u16::<1>(vaddl_u8(p1, q1));
    (
        vrshrn_n_u16::<2>(vaddq_u16(v, vsubl_u8(p0, q1))),
        vrshrn_n_u16::<2>(vaddq_u16(v, vsubl_u8(q0, p1))),
    )
}

/// The luma `tc0` vector — `ld4r` then `trn1`s: each of the four values over its
/// four lines.
#[inline]
#[target_feature(enable = "neon")]
fn tc_luma(tc: &[i8; 4]) -> int8x16_t {
    let mut t = [0u8; 16];
    for (i, v) in t.iter_mut().enumerate() {
        *v = tc[i >> 2] as u8;
    }
    vreinterpretq_s8_u8(ld16(&t))
}

/// The chroma `tc0` vector — each value over two lines, once per plane.
#[inline]
#[target_feature(enable = "neon")]
fn tc_chroma(tc: &[i8; 4]) -> int8x16_t {
    let mut t = [0u8; 16];
    for (i, v) in t.iter_mut().enumerate() {
        *v = tc[(i >> 1) & 3] as u8;
    }
    vreinterpretq_s8_u8(ld16(&t))
}

/// An 8x8 byte transpose, its own inverse: after the three `trn` stages, lane `i`
/// of input `j` is lane `j` of output `i`.
#[inline]
#[target_feature(enable = "neon")]
fn transpose8x8(r: [uint8x8_t; 8]) -> [uint8x8_t; 8] {
    let b0 = vreinterpret_u16_u8(vtrn1_u8(r[0], r[1]));
    let b1 = vreinterpret_u16_u8(vtrn2_u8(r[0], r[1]));
    let b2 = vreinterpret_u16_u8(vtrn1_u8(r[2], r[3]));
    let b3 = vreinterpret_u16_u8(vtrn2_u8(r[2], r[3]));
    let b4 = vreinterpret_u16_u8(vtrn1_u8(r[4], r[5]));
    let b5 = vreinterpret_u16_u8(vtrn2_u8(r[4], r[5]));
    let b6 = vreinterpret_u16_u8(vtrn1_u8(r[6], r[7]));
    let b7 = vreinterpret_u16_u8(vtrn2_u8(r[6], r[7]));
    let c0 = vreinterpret_u32_u16(vtrn1_u16(b0, b2));
    let c2 = vreinterpret_u32_u16(vtrn2_u16(b0, b2));
    let c1 = vreinterpret_u32_u16(vtrn1_u16(b1, b3));
    let c3 = vreinterpret_u32_u16(vtrn2_u16(b1, b3));
    let c4 = vreinterpret_u32_u16(vtrn1_u16(b4, b6));
    let c6 = vreinterpret_u32_u16(vtrn2_u16(b4, b6));
    let c5 = vreinterpret_u32_u16(vtrn1_u16(b5, b7));
    let c7 = vreinterpret_u32_u16(vtrn2_u16(b5, b7));
    [
        vreinterpret_u8_u32(vtrn1_u32(c0, c4)),
        vreinterpret_u8_u32(vtrn1_u32(c1, c5)),
        vreinterpret_u8_u32(vtrn1_u32(c2, c6)),
        vreinterpret_u8_u32(vtrn1_u32(c3, c7)),
        vreinterpret_u8_u32(vtrn2_u32(c0, c4)),
        vreinterpret_u8_u32(vtrn2_u32(c1, c5)),
        vreinterpret_u8_u32(vtrn2_u32(c2, c6)),
        vreinterpret_u8_u32(vtrn2_u32(c3, c7)),
    ]
}

/// A vertical luma edge's sixteen lines, taps `-4 .. 4` of each, as eight tap
/// vectors of sixteen lines: two 8x8 transposes, one per half.
///
/// The lines come out of **one** span rather than sixteen `row_n` calls — the same
/// block, one bounds check instead of sixteen; see [`RefSamples::span`]. The cut is
/// the exact bounding box of what the taps read, so the reach is unchanged.
#[inline]
#[target_feature(enable = "neon")]
fn gather_luma_lines(pix: &impl RefSamples) -> [[u8; 16]; 8] {
    let s = pix.span::<8, 16>(0, -4);
    let mut lines = [vdup_n_u8(0); 16];
    for (i, l) in lines.iter_mut().enumerate() {
        *l = ld8(&s.row::<8>(i, 0));
    }
    let top = transpose8x8([
        lines[0], lines[1], lines[2], lines[3], lines[4], lines[5], lines[6], lines[7],
    ]);
    let bot = transpose8x8([
        lines[8], lines[9], lines[10], lines[11], lines[12], lines[13], lines[14], lines[15],
    ]);
    let mut t = [[0u8; 16]; 8];
    for (x, tap) in t.iter_mut().enumerate() {
        st16(tap, vcombine_u8(top[x], bot[x]));
    }
    t
}

/// The inverse of [`gather_luma_lines`] for taps `FIRST .. FIRST + N` only — the
/// ones the filter may have changed — written back at column `FIRST - 4`.
#[inline]
#[target_feature(enable = "neon")]
fn scatter_luma_lines<const FIRST: usize, const N: usize>(
    pix: &mut impl PlaneSamples,
    t: &[[u8; 16]; 8],
) {
    let mut top = [vdup_n_u8(0); 8];
    let mut bot = [vdup_n_u8(0); 8];
    for x in 0..8 {
        let v = ld16(&t[x]);
        top[x] = vget_low_u8(v);
        bot[x] = vget_high_u8(v);
    }
    let top = transpose8x8(top);
    let bot = transpose8x8(bot);
    // Sixteen lines as one block: `set_block` cuts a single span for them, where a
    // `set_row_n` apiece re-derived and re-checked each line's address.
    let out: [[u8; N]; 16] = std::array::from_fn(|i| {
        let line = to8(if i < 8 { top[i] } else { bot[i - 8] });
        line[FIRST..FIRST + N].try_into().expect("taps")
    });
    pix.set_block::<N, 16>(0, FIRST as isize - 4, &out);
}

/// A vertical chroma edge's eight lines per plane, taps `-2 .. 2`, as four tap
/// vectors holding the Cb lines low and the Cr lines high.
#[inline]
#[target_feature(enable = "neon")]
fn gather_chroma_lines(cb: &impl RefSamples, cr: &impl RefSamples) -> [[u8; 16]; 4] {
    let (sb, sr) = (cb.span::<4, 8>(0, -2), cr.span::<4, 8>(0, -2));
    let mut a = [vdup_n_u8(0); 8];
    let mut b = [vdup_n_u8(0); 8];
    for i in 0..8 {
        a[i] = ld4(&sb.row::<4>(i, 0));
        b[i] = ld4(&sr.row::<4>(i, 0));
    }
    let a = transpose8x8(a);
    let b = transpose8x8(b);
    let mut t = [[0u8; 16]; 4];
    for (x, tap) in t.iter_mut().enumerate() {
        st16(tap, vcombine_u8(a[x], b[x]));
    }
    t
}

/// The inverse of [`gather_chroma_lines`] for `p0` and `q0` — taps 1 and 2.
#[inline]
#[target_feature(enable = "neon")]
fn scatter_chroma_lines(cb: &mut impl PlaneSamples, cr: &mut impl PlaneSamples, t: &[[u8; 16]; 4]) {
    let mut a = [vdup_n_u8(0); 8];
    let mut b = [vdup_n_u8(0); 8];
    for x in 0..4 {
        let v = ld16(&t[x]);
        a[x] = vget_low_u8(v);
        b[x] = vget_high_u8(v);
    }
    let a = transpose8x8(a);
    let b = transpose8x8(b);
    let out_cb: [[u8; 2]; 8] = std::array::from_fn(|i| to8(a[i])[1..3].try_into().expect("p0, q0"));
    let out_cr: [[u8; 2]; 8] = std::array::from_fn(|i| to8(b[i])[1..3].try_into().expect("p0, q0"));
    cb.set_block::<2, 8>(0, -1, &out_cb);
    cr.set_block::<2, 8>(0, -1, &out_cr);
}

// ============================================================================
// The 16-line filters
// ============================================================================

/// `DeblockLumaLt4V_AArch64_neon`'s body, on gathered rows.
#[inline]
#[target_feature(enable = "neon")]
fn luma_lt4_16(
    p2: &[u8; 16],
    p1: &mut [u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &mut [u8; 16],
    q2: &[u8; 16],
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    let (alpha_v, beta_v) = (vdupq_n_u8(alpha as u8), vdupq_n_u8(beta as u8));
    let tc0 = tc_luma(tc);
    let (vp2, vp1, vp0) = (ld16(p2), ld16(p1), ld16(p0));
    let (vq0, vq1, vq2) = (ld16(q0), ld16(q1), ld16(q2));

    let flag = vandq_u8(
        vcgezq_s8(tc0),
        mask_matrix(vp1, vp0, vq0, vq1, alpha_v, beta_v),
    );
    if !any_set(flag) {
        return;
    }
    let neg_tc0 = vnegq_s8(tc0);

    let (p1n, cond_p) = lt4_p1(vp2, vp1, vp0, vq0, beta_v, neg_tc0, tc0, flag);
    let (q1n, cond_q) = lt4_p1(vq2, vq1, vq0, vp0, beta_v, neg_tc0, tc0, flag);

    // `abs` of a `0xFF` mask is 1: the scalar's `tc_i += 1` per side.
    let tc_i = vaddq_s8(
        vaddq_s8(tc0, vabsq_s8(vreinterpretq_s8_u8(cond_p))),
        vabsq_s8(vreinterpretq_s8_u8(cond_q)),
    );
    let d = vminq_s8(
        vmaxq_s8(lt4_delta(vp1, vp0, vq0, vq1), vnegq_s8(tc_i)),
        tc_i,
    );
    let d = vandq_s8(d, vreinterpretq_s8_u8(flag));
    let (pos, neg) = split_delta(d);

    st16(p1, p1n);
    st16(p0, vqsubq_u8(vqaddq_u8(vp0, pos), neg));
    st16(q0, vqaddq_u8(vqsubq_u8(vq0, pos), neg));
    st16(q1, q1n);
}

/// `DeblockLumaEq4V_AArch64_neon`'s body, on gathered rows.
#[inline]
#[target_feature(enable = "neon")]
fn luma_eq4_16(
    p3: &[u8; 16],
    p2: &mut [u8; 16],
    p1: &mut [u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &mut [u8; 16],
    q2: &mut [u8; 16],
    q3: &[u8; 16],
    alpha: i32,
    beta: i32,
) {
    let (alpha_v, beta_v) = (vdupq_n_u8(alpha as u8), vdupq_n_u8(beta as u8));
    let (vp3, vp2, vp1, vp0) = (ld16(p3), ld16(p2), ld16(p1), ld16(p0));
    let (vq0, vq1, vq2, vq3) = (ld16(q0), ld16(q1), ld16(q2), ld16(q3));

    let mask = mask_matrix(vp1, vp0, vq0, vq1, alpha_v, beta_v);
    if !any_set(mask) {
        return;
    }
    let small = vcgtq_u8(vdupq_n_u8(((alpha >> 2) + 2) as u8), vabdq_u8(vp0, vq0));
    let cond_p = vandq_u8(vcgtq_u8(beta_v, vabdq_u8(vp2, vp0)), small);
    let cond_q = vandq_u8(vcgtq_u8(beta_v, vabdq_u8(vq2, vq0)), small);

    let (p0c, p1n, p2n) = eq4_side(vp3, vp2, vp1, vp0, vq0, vq1, cond_p);
    let (q0c, q1n, q2n) = eq4_side(vq3, vq2, vq1, vq0, vp0, vp1, cond_q);
    let sel_p = vandq_u8(cond_p, mask);
    let sel_q = vandq_u8(cond_q, mask);

    st16(p2, vbslq_u8(sel_p, p2n, vp2));
    st16(p1, vbslq_u8(sel_p, p1n, vp1));
    st16(p0, vbslq_u8(mask, p0c, vp0));
    st16(q0, vbslq_u8(mask, q0c, vq0));
    st16(q1, vbslq_u8(sel_q, q1n, vq1));
    st16(q2, vbslq_u8(sel_q, q2n, vq2));
}

/// `DeblockChromaLt4V_AArch64_neon`'s body: the Cb lines in the low eight lanes, Cr
/// in the high eight.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_lt4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    let (alpha_v, beta_v) = (vdupq_n_u8(alpha as u8), vdupq_n_u8(beta as u8));
    let tc0 = tc_chroma(tc);
    let (vp1, vp0, vq0, vq1) = (ld16(p1), ld16(p0), ld16(q0), ld16(q1));

    let flag = vandq_u8(
        vcgtzq_s8(tc0),
        mask_matrix(vp1, vp0, vq0, vq1, alpha_v, beta_v),
    );
    if !any_set(flag) {
        return;
    }
    let d = vminq_s8(vmaxq_s8(lt4_delta(vp1, vp0, vq0, vq1), vnegq_s8(tc0)), tc0);
    let d = vandq_s8(d, vreinterpretq_s8_u8(flag));
    let (pos, neg) = split_delta(d);

    st16(p0, vqsubq_u8(vqaddq_u8(vp0, pos), neg));
    st16(q0, vqaddq_u8(vqsubq_u8(vq0, pos), neg));
}

/// `DeblockChromaEq4V_AArch64_neon`'s body.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_eq4_16(
    p1: &[u8; 16],
    p0: &mut [u8; 16],
    q0: &mut [u8; 16],
    q1: &[u8; 16],
    alpha: i32,
    beta: i32,
) {
    let (alpha_v, beta_v) = (vdupq_n_u8(alpha as u8), vdupq_n_u8(beta as u8));
    let (vp1, vp0, vq0, vq1) = (ld16(p1), ld16(p0), ld16(q0), ld16(q1));

    let mask = mask_matrix(vp1, vp0, vq0, vq1, alpha_v, beta_v);
    if !any_set(mask) {
        return;
    }
    let (p0_lo, q0_lo) = chroma_eq4_half(
        vget_low_u8(vp1),
        vget_low_u8(vp0),
        vget_low_u8(vq0),
        vget_low_u8(vq1),
    );
    let (p0_hi, q0_hi) = chroma_eq4_half(
        vget_high_u8(vp1),
        vget_high_u8(vp0),
        vget_high_u8(vq0),
        vget_high_u8(vq1),
    );

    st16(p0, vbslq_u8(mask, vcombine_u8(p0_lo, p0_hi), vp0));
    st16(q0, vbslq_u8(mask, vcombine_u8(q0_lo, q0_hi), vq0));
}

// ============================================================================
// The edge-direction wrappers
// ============================================================================
//
// # Preconditions
//
// The scalar twin is stride-agnostic — it addresses in flat byte offsets — so any
// `(step_x, step_y)` pair is meaningful to it. These wrappers address in 2D through
// the cursor instead (`row_n::<N>(dy, dx)`), so the direction guard testing
// `step_y == 1` / `step_x == 1` is only half the contract: the *other* step must
// also be the cursor's own stride, which the `debug_assert!`s keep true.

/// `DeblockLumaLt4V_AArch64_neon` / `DeblockLumaLt4H_AArch64_neon`.
pub fn deblock_luma_lt4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Horizontal edge: taps step vertically in y (-3, -2, -1, 0, 1, 2), which is
        // one 16-wide, 6-tall span cut at `dy0 = -3` and indexed from its own row 0.
        let (p2, mut p1, mut p0, mut q0, mut q1, q2) = {
            let s = pix.span::<16, 6>(-3, 0);
            (
                s.row::<16>(0, 0),
                s.row::<16>(1, 0),
                s.row::<16>(2, 0),
                s.row::<16>(3, 0),
                s.row::<16>(4, 0),
                s.row::<16>(5, 0),
            )
        };

        // SAFETY: NEON is baseline on aarch64; see the module header.
        unsafe {
            luma_lt4_16(
                &p2, &mut p1, &mut p0, &mut q0, &mut q1, &q2, alpha, beta, tc,
            )
        };

        pix.set_block::<16, 4>(-2, 0, &[p1, p0, q0, q1]);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, pix.stride() as isize);
        // Vertical edge: line i has its taps at row i, columns -4..4.
        let mut t = unsafe { gather_luma_lines(&*pix) };
        let [
            _,
            ref t1,
            ref mut t2,
            ref mut t3,
            ref mut t4,
            ref mut t5,
            ref t6,
            _,
        ] = t;
        unsafe { luma_lt4_16(t1, t2, t3, t4, t5, t6, alpha, beta, tc) };
        // Write back only the columns the filter can modify — `p1..q1` — since at
        // `iEdge == 0` the outer columns belong to the previous macroblock.
        unsafe { scatter_luma_lines::<2, 4>(pix, &t) };
    } else {
        crate::common::deblocking_common::deblock_luma_lt4_scalar(
            pix, step_x, step_y, alpha, beta, tc,
        );
    }
}

/// `DeblockLumaEq4V_AArch64_neon` / `DeblockLumaEq4H_AArch64_neon`.
pub fn deblock_luma_eq4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, pix.stride() as isize);
        // Taps `-4 .. 3`: one 16-wide, 8-tall span, as in `deblock_luma_lt4`.
        let (p3, mut p2, mut p1, mut p0, mut q0, mut q1, mut q2, q3) = {
            let s = pix.span::<16, 8>(-4, 0);
            (
                s.row::<16>(0, 0),
                s.row::<16>(1, 0),
                s.row::<16>(2, 0),
                s.row::<16>(3, 0),
                s.row::<16>(4, 0),
                s.row::<16>(5, 0),
                s.row::<16>(6, 0),
                s.row::<16>(7, 0),
            )
        };

        unsafe {
            luma_eq4_16(
                &p3, &mut p2, &mut p1, &mut p0, &mut q0, &mut q1, &mut q2, &q3, alpha, beta,
            )
        };

        pix.set_block::<16, 6>(-3, 0, &[p2, p1, p0, q0, q1, q2]);
    } else if step_x == 1 {
        debug_assert_eq!(step_y, pix.stride() as isize);
        let mut t = unsafe { gather_luma_lines(&*pix) };
        let [
            ref t0,
            ref mut t1,
            ref mut t2,
            ref mut t3,
            ref mut t4,
            ref mut t5,
            ref mut t6,
            ref t7,
        ] = t;
        unsafe { luma_eq4_16(t0, t1, t2, t3, t4, t5, t6, t7, alpha, beta) };
        // `p2..q2` only, as above.
        unsafe { scatter_luma_lines::<1, 6>(pix, &t) };
    } else {
        crate::common::deblocking_common::deblock_luma_eq4_scalar(pix, step_x, step_y, alpha, beta);
    }
}

/// `DeblockChromaLt4V_AArch64_neon` / `DeblockChromaLt4H_AArch64_neon`.
pub fn deblock_chroma_lt4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        // Taps `-2 .. 1` of each plane: one 8-wide, 4-tall span apiece, Cb into the
        // low eight lanes and Cr into the high eight.
        let mut p1 = [0u8; 16];
        let mut p0 = [0u8; 16];
        let mut q0 = [0u8; 16];
        let mut q1 = [0u8; 16];
        {
            let (sb, sr) = (cb.span::<8, 4>(-2, 0), cr.span::<8, 4>(-2, 0));
            for (k, row) in [&mut p1, &mut p0, &mut q0, &mut q1].into_iter().enumerate() {
                row[..8].copy_from_slice(&sb.row::<8>(k, 0));
                row[8..].copy_from_slice(&sr.row::<8>(k, 0));
            }
        }

        unsafe { chroma_lt4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta, tc) };

        cb.set_block::<8, 2>(
            -1,
            0,
            &[
                p0[..8].try_into().expect("cb p0"),
                q0[..8].try_into().expect("cb q0"),
            ],
        );
        cr.set_block::<8, 2>(
            -1,
            0,
            &[
                p0[8..].try_into().expect("cr p0"),
                q0[8..].try_into().expect("cr q0"),
            ],
        );
    } else if step_x == 1 {
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        let mut t = unsafe { gather_chroma_lines(&*cb, &*cr) };
        let [ref t0, ref mut t1, ref mut t2, ref t3] = t;
        unsafe { chroma_lt4_16(t0, t1, t2, t3, alpha, beta, tc) };
        unsafe { scatter_chroma_lines(cb, cr, &t) };
    } else {
        crate::common::deblocking_common::deblock_chroma_lt4_scalar(
            cb, cr, step_x, step_y, alpha, beta, tc,
        );
    }
}

/// `DeblockChromaEq4V_AArch64_neon` / `DeblockChromaEq4H_AArch64_neon`.
pub fn deblock_chroma_eq4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    if step_y == 1 {
        debug_assert_eq!(step_x, cb.stride() as isize);
        debug_assert_eq!(step_x, cr.stride() as isize);
        // Taps `-2 .. 1` of each plane: one 8-wide, 4-tall span apiece, Cb into the
        // low eight lanes and Cr into the high eight.
        let mut p1 = [0u8; 16];
        let mut p0 = [0u8; 16];
        let mut q0 = [0u8; 16];
        let mut q1 = [0u8; 16];
        {
            let (sb, sr) = (cb.span::<8, 4>(-2, 0), cr.span::<8, 4>(-2, 0));
            for (k, row) in [&mut p1, &mut p0, &mut q0, &mut q1].into_iter().enumerate() {
                row[..8].copy_from_slice(&sb.row::<8>(k, 0));
                row[8..].copy_from_slice(&sr.row::<8>(k, 0));
            }
        }

        unsafe { chroma_eq4_16(&p1, &mut p0, &mut q0, &q1, alpha, beta) };

        cb.set_block::<8, 2>(
            -1,
            0,
            &[
                p0[..8].try_into().expect("cb p0"),
                q0[..8].try_into().expect("cb q0"),
            ],
        );
        cr.set_block::<8, 2>(
            -1,
            0,
            &[
                p0[8..].try_into().expect("cr p0"),
                q0[8..].try_into().expect("cr q0"),
            ],
        );
    } else if step_x == 1 {
        debug_assert_eq!(step_y, cb.stride() as isize);
        debug_assert_eq!(step_y, cr.stride() as isize);
        let mut t = unsafe { gather_chroma_lines(&*cb, &*cr) };
        let [ref t0, ref mut t1, ref mut t2, ref t3] = t;
        unsafe { chroma_eq4_16(t0, t1, t2, t3, alpha, beta) };
        unsafe { scatter_chroma_lines(cb, cr, &t) };
    } else {
        crate::common::deblocking_common::deblock_chroma_eq4_scalar(
            cb, cr, step_x, step_y, alpha, beta,
        );
    }
}

// ============================================================================
// Boundary strength
// ============================================================================
//
// `DeblockingBSCalcEnc_AArch64_neon`, `codec/common/arm64/deblocking_aarch64_neon.S:827`,
// built there from the `BS_NZC_CHECK` and `BS_MV_CHECK` macros. Both directions'
// sixteen edge positions in one pass: `2` where either 4x4 block has a non-zero
// coefficient count, else `1` where the two motion vectors differ by at least a whole
// sample in either component, else `0`.
//
// # Three departures from the asm, each named where it is
//
// * The column-major rearrangement of the counts is one `tbl` rather than the asm's
//   two `ins`/`zip1` pairs — the same permutation, a quarter of the instructions.
// * The counts are combined with `orr` and tested against zero, where the asm adds
//   and compares. Both answer "is either non-zero", which is what the scalar's
//   `(a | b) != 0` asks; `orr` cannot overflow a lane, so it holds for raw counts as
//   well as normalised ones.
// * The motion-vector difference is a saturating subtraction each way and the larger
//   of the two, where the asm uses `sabd`. See [`mv_ge4`].
//
// The asm reaches its neighbours by pointer arithmetic off the current macroblock
// (`pCurMb - 1`, `pCurMb - iMbStride`) and so assumes one contiguous macroblock array
// and a single reference frame — which is why upstream installs it only under
// `SINGLE_REF_FRAME`. Here both neighbours arrive as plain arrays the caller looked
// up through the macroblock window, so neither assumption is made; the absent-
// neighbour case is `None` and is *defined* to give zero, rather than the asm's stale
// register that its caller happens to overwrite.

use crate::encoder::encoder_context::SMVUnitXY;

const _: () = assert!(
    size_of::<SMVUnitXY>() == 4 && align_of::<SMVUnitXY>() == 2,
    "`ld_mv4` reads `[SMVUnitXY; 16]` as 32 contiguous `i16`"
);

/// The 4x4 transpose of a macroblock's sixteen block indices: entry `j` is block
/// `4 * (j % 4) + j / 4`, so the four vertical-edge positions of one column land in
/// one word. The asm builds it with two `ins`/`zip1` rounds; `tbl` is the same
/// permutation in one instruction.
const COLUMN_MAJOR: [u8; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];

/// Four bytes into lanes 12..16, the rest zero — where `ext #12` picks the
/// neighbouring macroblock's edge column up from.
#[inline]
#[target_feature(enable = "neon")]
fn edge_word(w: u32) -> uint8x16_t {
    vreinterpretq_u8_u32(vsetq_lane_u32::<3>(w, vdupq_n_u32(0)))
}

/// The four counts at `idx` of a neighbour's table, as one word.
#[inline]
fn nzc_word(nzc: &[i8; 24], idx: [usize; 4]) -> u32 {
    u32::from_ne_bytes(idx.map(|i| nzc[i] as u8))
}

/// A macroblock's sixteen luma counts as bytes. Only their non-zero-ness is read, so
/// the sign the C++ never sets in them cannot matter.
#[inline]
fn nzc_word_bytes(nzc: &[i8; 24]) -> [u8; 16] {
    std::array::from_fn(|i| nzc[i] as u8)
}

/// `BS_NZC_CHECK`'s tail: `2` wherever either of the two blocks has a coefficient.
#[inline]
#[target_feature(enable = "neon")]
fn nzc_term(cur: uint8x16_t, prev: uint8x16_t) -> uint8x16_t {
    let both = vorrq_u8(cur, prev);
    vandq_u8(vtstq_u8(both, both), vdupq_n_u8(2))
}

/// The `k`-th group of four motion vectors, as eight halfword lanes.
#[inline]
#[target_feature(enable = "neon")]
fn ld_mv4<const K: usize>(mv: &[SMVUnitXY; 16]) -> int16x8_t {
    const { assert!(K < 4, "a macroblock holds four groups of four vectors") };
    // SAFETY: `SMVUnitXY` is `#[repr(C)]` over two `i16` — asserted above — so
    // `[SMVUnitXY; 16]` is 32 contiguous `i16` with no padding, and `K < 4` puts the
    // eight lanes read here at halfwords `8K .. 8K + 8` of it.
    unsafe { vld1q_s16(mv.as_ptr().cast::<i16>().add(K * 8)) }
}

/// Four motion vectors gathered from scattered indices — the left neighbour's last
/// column, which is the one place the asm's contiguous `ld1` cannot serve.
#[inline]
#[target_feature(enable = "neon")]
fn gather_mv4(mv: &[SMVUnitXY; 16], idx: [usize; 4]) -> int16x8_t {
    let mut w = [0i16; 8];
    for (j, &i) in idx.iter().enumerate() {
        w[2 * j] = mv[i].iMvX;
        w[2 * j + 1] = mv[i].iMvY;
    }
    ld8_i16(&w)
}

/// `BS_COMPARE_MV`: for four vector pairs, a set mask where either component differs
/// by four or more — the whole-sample threshold `MB_BS_MV`/`SMB_EDGE_MV` test.
///
/// `|a - b| >= 4` is spelled as a saturating subtraction each way and the larger of
/// the two, not as the asm's `sabd`: `sabd` wraps for a difference past `i16::MAX`,
/// where saturation clamps to `i16::MAX` — still at or above 4 — so this answers the
/// scalar's `i32` question over the whole `i16` range rather than only over the
/// motion range the encoder happens to produce.
#[inline]
#[target_feature(enable = "neon")]
fn mv_ge4(a: int16x8_t, b: int16x8_t) -> uint16x8_t {
    let d = vmaxq_s16(vqsubq_s16(a, b), vqsubq_s16(b, a));
    vcgeq_s16(d, vdupq_n_s16(4))
}

/// The four groups' comparisons folded to sixteen `1`/`0` bytes: `pmax` pairs each
/// vector's two components together, then the halfword masks narrow to bytes.
#[inline]
#[target_feature(enable = "neon")]
fn mv_term(m: [uint16x8_t; 4]) -> uint8x16_t {
    let lo = vpmaxq_u16(m[0], m[1]);
    let hi = vpmaxq_u16(m[2], m[3]);
    vandq_u8(vcombine_u8(vmovn_u16(lo), vmovn_u16(hi)), vdupq_n_u8(1))
}

/// The per-direction mask: the macroblock edge kept or cleared, the three interior
/// edges reduced to what this macroblock kind's rule allows. See
/// [`bs_calc`] for what the caller puts in `inside`.
#[inline]
#[target_feature(enable = "neon")]
fn bs_mask(edge_present: bool, inside: u8) -> uint8x16_t {
    let mut m = [inside; 16];
    m[..4].fill(if edge_present { 0xFF } else { 0 });
    ld16(&m)
}

/// `DeblockingBSCalcEnc_AArch64_neon` — the deblocking boundary strengths of one
/// macroblock, both directions, from plain per-macroblock data.
///
/// `bs[0]` is the vertical (left-hand) edges and `bs[1]` the horizontal (top) ones;
/// within each, `[0]` is the macroblock boundary and `[1..4]` the interior edges, and
/// the four bytes of each are the positions along it. A `None` neighbour gives `0`
/// across that macroblock edge, which is what `DeblockingBSCalc_c` writes when the
/// boundary flag is clear.
///
/// `inside` is ANDed into the interior edges, and is where the three macroblock kinds
/// differ: `0xFF` for a partitioned inter macroblock (`DeblockingBSInsideMBNormal`),
/// `0x02` for `MB_TYPE_16x16`, whose rule is the coefficient term alone
/// (`DeblockingBSInsideMBAvsbase`), and `0x00` for `MB_TYPE_SKIP`, which has no
/// interior edges. On the data the encoder actually produces the last two masks
/// change nothing — a 16x16 partition replicates one vector to all sixteen blocks, so
/// the vector term is already zero, and a skip macroblock has neither coefficients
/// nor differing vectors — and `deblocking::tests::the_kind_masks_are_no_ops_on_the`
/// `_data_the_encoder_produces` is that claim. They are applied anyway, because "the
/// encoder cannot produce it" is not a property of this kernel.
///
/// Nothing here reads a reference index. Neither does the scalar it must match, and
/// neither does the asm — which is why upstream guards its installation with
/// `SINGLE_REF_FRAME`.
pub fn bs_calc(
    cur_nzc: &[i8; 24],
    cur_mv: &[SMVUnitXY; 16],
    left: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    top: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    inside: u8,
    bs: &mut [[[u8; 4]; 4]; 2],
) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    unsafe { bs_calc_neon(cur_nzc, cur_mv, left, top, inside, bs) }
}

#[inline]
#[target_feature(enable = "neon")]
fn bs_calc_neon(
    cur_nzc: &[i8; 24],
    cur_mv: &[SMVUnitXY; 16],
    left: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    top: Option<(&[i8; 24], &[SMVUnitXY; 16])>,
    inside: u8,
    bs: &mut [[[u8; 4]; 4]; 2],
) {
    // The counts, in both orders: raster for the horizontal edges, column-major for
    // the vertical ones. Only the sixteen luma entries take part.
    let rows = ld16(&nzc_word_bytes(cur_nzc));
    let cols = vqtbl1q_u8(rows, ld16(&COLUMN_MAJOR));

    // `ext #12` slides the neighbour's edge column in ahead of this macroblock's own
    // first three columns, so lane `i` meets the block across its edge.
    let prev_rows = vextq_u8::<12>(
        edge_word(top.map_or(0, |(n, _)| nzc_word(n, [12, 13, 14, 15]))),
        rows,
    );
    let prev_cols = vextq_u8::<12>(
        edge_word(left.map_or(0, |(n, _)| nzc_word(n, [3, 7, 11, 15]))),
        cols,
    );

    // The vectors, likewise: four rows of four, and their 4x4 transpose.
    let (r0, r1, r2, r3) = (
        ld_mv4::<0>(cur_mv),
        ld_mv4::<1>(cur_mv),
        ld_mv4::<2>(cur_mv),
        ld_mv4::<3>(cur_mv),
    );
    let (z0, z1) = (
        vreinterpretq_s16_u32(vzip1q_u32(
            vreinterpretq_u32_s16(r0),
            vreinterpretq_u32_s16(r2),
        )),
        vreinterpretq_s16_u32(vzip2q_u32(
            vreinterpretq_u32_s16(r0),
            vreinterpretq_u32_s16(r2),
        )),
    );
    let (z2, z3) = (
        vreinterpretq_s16_u32(vzip1q_u32(
            vreinterpretq_u32_s16(r1),
            vreinterpretq_u32_s16(r3),
        )),
        vreinterpretq_s16_u32(vzip2q_u32(
            vreinterpretq_u32_s16(r1),
            vreinterpretq_u32_s16(r3),
        )),
    );
    let (c0, c1) = (
        vreinterpretq_s16_u32(vzip1q_u32(
            vreinterpretq_u32_s16(z0),
            vreinterpretq_u32_s16(z2),
        )),
        vreinterpretq_s16_u32(vzip2q_u32(
            vreinterpretq_u32_s16(z0),
            vreinterpretq_u32_s16(z2),
        )),
    );
    let (c2, c3) = (
        vreinterpretq_s16_u32(vzip1q_u32(
            vreinterpretq_u32_s16(z1),
            vreinterpretq_u32_s16(z3),
        )),
        vreinterpretq_s16_u32(vzip2q_u32(
            vreinterpretq_u32_s16(z1),
            vreinterpretq_u32_s16(z3),
        )),
    );

    let zero_mv = vdupq_n_s16(0);
    let prev_r = top.map_or(zero_mv, |(_, m)| ld_mv4::<3>(m));
    let prev_c = left.map_or(zero_mv, |(_, m)| gather_mv4(m, [3, 7, 11, 15]));

    let mv_rows = mv_term([
        mv_ge4(prev_r, r0),
        mv_ge4(r0, r1),
        mv_ge4(r1, r2),
        mv_ge4(r2, r3),
    ]);
    let mv_cols = mv_term([
        mv_ge4(prev_c, c0),
        mv_ge4(c0, c1),
        mv_ge4(c1, c2),
        mv_ge4(c2, c3),
    ]);

    // `umax`: the coefficient term is 2 and the vector term 1, so the larger is the
    // scalar's "2 if either block has a coefficient, else 1 if the vectors differ".
    let vertical = vandq_u8(
        vmaxq_u8(nzc_term(cols, prev_cols), mv_cols),
        bs_mask(left.is_some(), inside),
    );
    let horizontal = vandq_u8(
        vmaxq_u8(nzc_term(rows, prev_rows), mv_rows),
        bs_mask(top.is_some(), inside),
    );

    let (v, h) = bs.split_at_mut(1);
    st16(v[0].as_flattened_mut(), vertical);
    st16(h[0].as_flattened_mut(), horizontal);
}

// ============================================================================
// Unit Tests & Parity Verification
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::deblocking_common as scalar;
    use crate::safe::plane::PaddedPlane;

    fn make_test_plane(w: usize, h: usize, pad: usize, stride: usize) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, (((x * 17) ^ (y * 31) ^ 0x5a) & 0xff) as u8);
            }
        }
        p
    }

    fn lcg(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 32) as u32
    }

    /// Noise of amplitude `amp` around a slow ramp — what a reconstructed picture looks
    /// like near a block edge that the filter will actually touch. A pure-noise plane
    /// takes the early-out on nearly every line and tests the compare and nothing else.
    fn smooth_plane(
        w: usize,
        h: usize,
        pad: usize,
        stride: usize,
        amp: u32,
        seed: &mut u64,
    ) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                let base = 96 + ((x + y + 64) / 4) as u32;
                p.set(x, y, (base + lcg(seed) % (amp + 1)).min(255) as u8);
            }
        }
        p
    }

    fn assert_planes_equal(a: &PaddedPlane, b: &PaddedPlane, what: &str) {
        assert_eq!(a.as_slice(), b.as_slice(), "{what}");
    }

    #[test]
    fn test_deblock_luma_lt4_parity() {
        let stride = 64;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut a = make_test_plane(32, 32, 16, stride);
            let mut b = a.clone();
            let tc = [2i8, 3, 1, 4];
            scalar::deblock_luma_lt4_scalar(&mut a.cursor_mut(8, 8), step_x, step_y, 20, 12, &tc);
            deblock_luma_lt4(&mut b.cursor_mut(8, 8), step_x, step_y, 20, 12, &tc);
            assert_planes_equal(&a, &b, &format!("luma lt4 horiz={is_horiz}"));
        }
    }

    #[test]
    fn test_deblock_luma_eq4_parity() {
        let stride = 64;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut a = make_test_plane(32, 32, 16, stride);
            let mut b = a.clone();
            scalar::deblock_luma_eq4_scalar(&mut a.cursor_mut(8, 8), step_x, step_y, 24, 15);
            deblock_luma_eq4(&mut b.cursor_mut(8, 8), step_x, step_y, 24, 15);
            assert_planes_equal(&a, &b, &format!("luma eq4 horiz={is_horiz}"));
        }
    }

    #[test]
    fn test_deblock_chroma_lt4_parity() {
        let stride = 32;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut cb_a = make_test_plane(16, 16, 8, stride);
            let mut cr_a = make_test_plane(16, 16, 8, stride);
            let mut cb_b = cb_a.clone();
            let mut cr_b = cr_a.clone();
            let tc = [1i8, 2, 0, 3];
            scalar::deblock_chroma_lt4_scalar(
                &mut cb_a.cursor_mut(4, 4),
                &mut cr_a.cursor_mut(4, 4),
                step_x,
                step_y,
                18,
                10,
                &tc,
            );
            deblock_chroma_lt4(
                &mut cb_b.cursor_mut(4, 4),
                &mut cr_b.cursor_mut(4, 4),
                step_x,
                step_y,
                18,
                10,
                &tc,
            );
            assert_planes_equal(&cb_a, &cb_b, &format!("chroma lt4 cb horiz={is_horiz}"));
            assert_planes_equal(&cr_a, &cr_b, &format!("chroma lt4 cr horiz={is_horiz}"));
        }
    }

    #[test]
    fn test_deblock_chroma_eq4_parity() {
        let stride = 32;
        for is_horiz in [true, false] {
            let (step_x, step_y) = if is_horiz {
                (stride as isize, 1)
            } else {
                (1, stride as isize)
            };
            let mut cb_a = make_test_plane(16, 16, 8, stride);
            let mut cr_a = make_test_plane(16, 16, 8, stride);
            let mut cb_b = cb_a.clone();
            let mut cr_b = cr_a.clone();
            scalar::deblock_chroma_eq4_scalar(
                &mut cb_a.cursor_mut(4, 4),
                &mut cr_a.cursor_mut(4, 4),
                step_x,
                step_y,
                22,
                14,
            );
            deblock_chroma_eq4(
                &mut cb_b.cursor_mut(4, 4),
                &mut cr_b.cursor_mut(4, 4),
                step_x,
                step_y,
                22,
                14,
            );
            assert_planes_equal(&cb_a, &cb_b, &format!("chroma eq4 cb horiz={is_horiz}"));
            assert_planes_equal(&cr_a, &cr_b, &format!("chroma eq4 cr horiz={is_horiz}"));
        }
    }

    /// The byte-lane arithmetic has no headroom to hide in, so sweep it: smooth
    /// planes at several noise amplitudes (so the conditions hold on most lines and
    /// fail on some), every direction, and `alpha`/`beta`/`tc` over their whole
    /// tables — `alpha` to 255, `beta` to 18, `tc` from -1 to 25 — including the
    /// zero and negative `tc` that gate lines off.
    #[test]
    fn deblock_parity_sweep() {
        let mut seed = 0x0DDB_1A5E_5BAD_5EEDu64;
        let alphas = [0, 1, 4, 15, 40, 90, 160, 255];
        let betas = [0, 1, 3, 6, 10, 14, 18];
        let tcs: [[i8; 4]; 6] = [
            [0, 0, 0, 0],
            [-1, 0, 1, 2],
            [3, 2, 3, 1],
            [25, 25, 25, 25],
            [1, -1, 13, 0],
            [7, 9, 11, 13],
        ];
        for amp in [2u32, 8, 24, 80] {
            for &alpha in &alphas {
                for &beta in &betas {
                    for tc in &tcs {
                        for is_horiz in [true, false] {
                            let stride = 64;
                            let (step_x, step_y) = if is_horiz {
                                (stride as isize, 1)
                            } else {
                                (1, stride as isize)
                            };
                            let mut a = smooth_plane(32, 32, 16, stride, amp, &mut seed);
                            let mut b = a.clone();
                            scalar::deblock_luma_lt4_scalar(
                                &mut a.cursor_mut(8, 8),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                                tc,
                            );
                            deblock_luma_lt4(
                                &mut b.cursor_mut(8, 8),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                                tc,
                            );
                            assert_planes_equal(
                                &a,
                                &b,
                                &format!(
                                    "luma lt4 amp={amp} alpha={alpha} beta={beta} tc={tc:?} horiz={is_horiz}"
                                ),
                            );

                            let mut a = smooth_plane(32, 32, 16, stride, amp, &mut seed);
                            let mut b = a.clone();
                            scalar::deblock_luma_eq4_scalar(
                                &mut a.cursor_mut(8, 8),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                            );
                            deblock_luma_eq4(&mut b.cursor_mut(8, 8), step_x, step_y, alpha, beta);
                            assert_planes_equal(
                                &a,
                                &b,
                                &format!(
                                    "luma eq4 amp={amp} alpha={alpha} beta={beta} horiz={is_horiz}"
                                ),
                            );

                            let stride = 32;
                            let (step_x, step_y) = if is_horiz {
                                (stride as isize, 1)
                            } else {
                                (1, stride as isize)
                            };
                            let mut cb_a = smooth_plane(16, 16, 8, stride, amp, &mut seed);
                            let mut cr_a = smooth_plane(16, 16, 8, stride, amp, &mut seed);
                            let mut cb_b = cb_a.clone();
                            let mut cr_b = cr_a.clone();
                            scalar::deblock_chroma_lt4_scalar(
                                &mut cb_a.cursor_mut(4, 4),
                                &mut cr_a.cursor_mut(4, 4),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                                tc,
                            );
                            deblock_chroma_lt4(
                                &mut cb_b.cursor_mut(4, 4),
                                &mut cr_b.cursor_mut(4, 4),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                                tc,
                            );
                            assert_planes_equal(
                                &cb_a,
                                &cb_b,
                                &format!(
                                    "chroma lt4 cb amp={amp} alpha={alpha} beta={beta} tc={tc:?} horiz={is_horiz}"
                                ),
                            );
                            assert_planes_equal(
                                &cr_a,
                                &cr_b,
                                &format!(
                                    "chroma lt4 cr amp={amp} alpha={alpha} beta={beta} tc={tc:?} horiz={is_horiz}"
                                ),
                            );

                            let mut cb_a = smooth_plane(16, 16, 8, stride, amp, &mut seed);
                            let mut cr_a = smooth_plane(16, 16, 8, stride, amp, &mut seed);
                            let mut cb_b = cb_a.clone();
                            let mut cr_b = cr_a.clone();
                            scalar::deblock_chroma_eq4_scalar(
                                &mut cb_a.cursor_mut(4, 4),
                                &mut cr_a.cursor_mut(4, 4),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                            );
                            deblock_chroma_eq4(
                                &mut cb_b.cursor_mut(4, 4),
                                &mut cr_b.cursor_mut(4, 4),
                                step_x,
                                step_y,
                                alpha,
                                beta,
                            );
                            assert_planes_equal(
                                &cb_a,
                                &cb_b,
                                &format!(
                                    "chroma eq4 cb amp={amp} alpha={alpha} beta={beta} horiz={is_horiz}"
                                ),
                            );
                            assert_planes_equal(
                                &cr_a,
                                &cr_b,
                                &format!(
                                    "chroma eq4 cr amp={amp} alpha={alpha} beta={beta} horiz={is_horiz}"
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    /// The saturating add/sub trick against samples already at the rails: an edge
    /// between a black and a white block, where `p0 + delta` and `q0 - delta` leave
    /// `[0, 255]` and the scalar's `WelsClip1` is what has to be matched.
    #[test]
    fn deblock_clips_at_the_rails_like_the_scalar() {
        let stride = 64;
        for (dark, light) in [(0u8, 255u8), (2, 250), (255, 0), (0, 40)] {
            for is_horiz in [true, false] {
                let (step_x, step_y) = if is_horiz {
                    (stride as isize, 1)
                } else {
                    (1, stride as isize)
                };
                let mut a = PaddedPlane::new(32, 32, 16, stride);
                for y in -16..48isize {
                    for x in -16..48isize {
                        let before_edge = if is_horiz { y < 8 } else { x < 8 };
                        a.set(x, y, if before_edge { dark } else { light });
                    }
                }
                let mut b = a.clone();
                let tc = [25i8, 25, 25, 25];
                scalar::deblock_luma_lt4_scalar(
                    &mut a.cursor_mut(8, 8),
                    step_x,
                    step_y,
                    255,
                    18,
                    &tc,
                );
                deblock_luma_lt4(&mut b.cursor_mut(8, 8), step_x, step_y, 255, 18, &tc);
                assert_planes_equal(
                    &a,
                    &b,
                    &format!("lt4 rails ({dark}, {light}) horiz={is_horiz}"),
                );

                let mut a2 = a.clone();
                let mut b2 = a.clone();
                scalar::deblock_luma_eq4_scalar(&mut a2.cursor_mut(8, 8), step_x, step_y, 255, 18);
                deblock_luma_eq4(&mut b2.cursor_mut(8, 8), step_x, step_y, 255, 18);
                assert_planes_equal(
                    &a2,
                    &b2,
                    &format!("eq4 rails ({dark}, {light}) horiz={is_horiz}"),
                );
            }
        }
    }
}
