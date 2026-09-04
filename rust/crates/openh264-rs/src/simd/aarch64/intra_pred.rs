//! Intra prediction — `WelsI16x16LumaPred*_AArch64_neon`, `WelsIChromaPred*_AArch64_neon`
//! and `WelsI4x4LumaPred*_AArch64_neon` in `codec/common/arm64/intra_pred_common_aarch64_neon.S`
//! and `codec/encoder/core/arm64/intra_pred_aarch64_neon.S`, whose decoder twins in
//! `codec/decoder/core/arm64/intra_pred_aarch64_neon.S` are the same bodies writing
//! back through the stride they read from — which is what the `PredOut` seam below
//! is, so each predictor is written once here too.
//!
//! # What is vectorised
//!
//! The two plane predictors compute their `H`/`V` sums the asm's way — the neighbour
//! differences `mul`led by `[5 .. 40]` or `[17 .. 68]` and reduced — and fill with
//! `mla` and `sqrshrun #5`. The DC means are `uaddlv` reductions (`uaddlp` pairs for
//! the four chroma quadrants). The 4x4 directional predictors are the asm's `ext`,
//! `uaddl`, `uqrshrn` sequences on one eight-byte neighbour line, with the rows read
//! back out of the result at the offsets the asm stores from.
//!
//! Upstream has no arm64 `DDR` — on arm64 it stays C — so `enc_i4x4_luma_pred_ddr`
//! is written here in the idiom of its `HD` and `VR` neighbours: the nine-sample line
//! `l3 .. lt .. t3`, one three-tap pass, four rows at `ext` offsets. Nor has it `V`
//! predictors at any size, a `DC_128` fill, or a 4x4 `H`: those are broadcasts and
//! fills, and `every_kernel_here_reaches_an_intrinsic` lists them as scalar by
//! design.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::lanes::{ld16, ld8, ld8_i16, low4, to16, to8};
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursorMut, RefSamples};

// ============================================================================
// The enc/dec seam
// ============================================================================

/// Where a predictor's rows go: the encoder's packed candidate buffer, or the
/// decoder's picture through the cursor it read its neighbours from.
trait PredOut {
    fn put<const N: usize>(&mut self, dy: usize, row: &[u8; N]);
}

/// The encoder's packed candidate buffer, `W` bytes per row.
struct Packed<'a, const W: usize>(&'a mut [u8]);

impl<const W: usize> PredOut for Packed<'_, W> {
    #[inline(always)]
    fn put<const N: usize>(&mut self, dy: usize, row: &[u8; N]) {
        self.0[dy * W..][..N].copy_from_slice(row);
    }
}

impl PredOut for PlaneCursorMut<'_> {
    #[inline(always)]
    fn put<const N: usize>(&mut self, dy: usize, row: &[u8; N]) {
        self.row_mut(dy as isize, 0, N).copy_from_slice(row);
    }
}

#[inline(always)]
fn fill_rows<const N: usize, O: PredOut>(out: &mut O, rows: usize, row: &[u8; N]) {
    for dy in 0..rows {
        out.put(dy, row);
    }
}

/// The asm's constant pools.
static INTRA_1_TO_8: [i16; 8] = [5, 10, 15, 20, 25, 30, 35, 40];
static INTRA_M7_TO_0: [i16; 8] = [-7, -6, -5, -4, -3, -2, -1, 0];
static INTRA_P1_TO_8: [i16; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
static INTRA_1_TO_4: [i16; 8] = [17, 34, 51, 68, 17, 34, 51, 68];
static INTRA_M3_TO_P4: [i16; 8] = [-3, -2, -1, 0, 1, 2, 3, 4];

/// Eight left neighbours from row `y0` down, as one vector.
#[inline]
#[target_feature(enable = "neon")]
fn left8<S: RefSamples>(src: &S, y0: isize) -> uint8x8_t {
    let mut l = [0u8; 8];
    for (y, v) in l.iter_mut().enumerate() {
        *v = src.at(-1, y0 + y as isize);
    }
    ld8(&l)
}

/// Sixteen left neighbours.
#[inline]
#[target_feature(enable = "neon")]
fn left16<S: RefSamples>(src: &S) -> uint8x16_t {
    let mut l = [0u8; 16];
    for (y, v) in l.iter_mut().enumerate() {
        *v = src.at(-1, y as isize);
    }
    ld16(&l)
}

/// Four rows of four, packed.
#[inline(always)]
fn pack4(r0: [u8; 4], r1: [u8; 4], r2: [u8; 4], r3: [u8; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&r0);
    out[4..8].copy_from_slice(&r1);
    out[8..12].copy_from_slice(&r2);
    out[12..].copy_from_slice(&r3);
    out
}

/// Lanes `4..8` as an array — the asm's `st1 {v.s}[1]`.
#[inline]
#[target_feature(enable = "neon")]
fn high4(v: uint8x8_t) -> [u8; 4] {
    vget_lane_u32::<1>(vreinterpret_u32_u8(v)).to_ne_bytes()
}

// ============================================================================
// 16x16 luma
// ============================================================================

/// `WelsI16x16LumaPredDc_AArch64_neon` and its `DcTop`/`DcLeft` siblings, and the
/// `DC_128` case: `uaddlv` over whichever edges are in scope, then the rounding the
/// asm does with `uqrshrn`.
#[inline]
#[target_feature(enable = "neon")]
fn i16x16_dc_mean<S: RefSamples>(src: &S, use_top: bool, use_left: bool) -> u8 {
    let sum_top = if use_top { vaddlvq_u8(ld16(&src.row_n::<16>(-1, 0))) as i32 } else { 0 };
    let sum_left = if use_left { vaddlvq_u8(left16(src)) as i32 } else { 0 };
    match (use_top, use_left) {
        (true, true) => ((16 + sum_top + sum_left) >> 5) as u8,
        (true, false) => ((8 + sum_top) >> 4) as u8,
        (false, true) => ((8 + sum_left) >> 4) as u8,
        (false, false) => 0x80,
    }
}

/// `WelsI16x16LumaPredPlane_AArch64_neon`, the coefficient half: `(a, b, c)`.
///
/// The asm loads `top[-1 .. 7]` reversed and `top[8 .. 16]`, subtracts, multiplies
/// by `[5, 10 .. 40]` and `saddlv`s — that is `5 * Σ (i + 1) (top[8 + i] - top[6 - i])`
/// — and `sqrshrn #6`s it to `b`; the same down the left edge gives `c`; `a` is
/// `(top[15] + left[15]) << 4`.
#[inline]
#[target_feature(enable = "neon")]
fn i16x16_plane_coeffs<S: RefSamples>(src: &S) -> (i16, i16, i16) {
    let top_lo = ld8(&src.row_n::<8>(-1, -1));
    let top_hi = ld8(&src.row_n::<8>(-1, 8));
    let left_lo = left8(src, -1);
    let left_hi = left8(src, 8);
    let dt = vreinterpretq_s16_u16(vsubl_u8(top_hi, vrev64_u8(top_lo)));
    let dl = vreinterpretq_s16_u16(vsubl_u8(left_hi, vrev64_u8(left_lo)));
    let weights = ld8_i16(&INTRA_1_TO_8);
    let h = vaddlvq_s16(vmulq_s16(dt, weights));
    let v = vaddlvq_s16(vmulq_s16(dl, weights));
    let b = ((h + 32) >> 6) as i16;
    let c = ((v + 32) >> 6) as i16;
    let a = (vget_lane_u8::<7>(top_hi) as i16 + vget_lane_u8::<7>(left_hi) as i16) << 4;
    (a, b, c)
}

/// The fill half: `a + b (x - 7) + c (y - 7)`, `sqrshrun #5` per row, `c` added
/// between rows.
#[inline]
#[target_feature(enable = "neon")]
fn i16x16_plane_fill<O: PredOut>(out: &mut O, a: i16, b: i16, c: i16) {
    let base = vdupq_n_s16(a.wrapping_sub(c.wrapping_mul(7)));
    let mut lo = vmlaq_n_s16(base, ld8_i16(&INTRA_M7_TO_0), b);
    let mut hi = vmlaq_n_s16(base, ld8_i16(&INTRA_P1_TO_8), b);
    let cv = vdupq_n_s16(c);
    for dy in 0..16 {
        let row = vcombine_u8(vqrshrun_n_s16::<5>(lo), vqrshrun_n_s16::<5>(hi));
        out.put(dy, &to16(row));
        lo = vaddq_s16(lo, cv);
        hi = vaddq_s16(hi, cv);
    }
}

// ============================================================================
// 8x8 chroma
// ============================================================================

/// `WelsIChromaPredDc_AArch64_neon`: `[top | left]` in one register, `uaddlp` twice
/// to the four quadrant sums, `urshr` for the four means, as the two row values the
/// top and bottom halves are filled with.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_dc_rows<S: RefSamples>(src: &S) -> ([u8; 8], [u8; 8]) {
    let both = vcombine_u8(ld8(&src.row_n::<8>(-1, 0)), left8(src, 0));
    let sums = vpaddlq_u16(vpaddlq_u8(both)); // [T0, T1, L0, L1]
    let tl = vadd_u32(vget_low_u32(sums), vget_high_u32(sums)); // [T0 + L0, T1 + L1]
    let quads = vrshrq_n_u32::<2>(sums);
    let pairs = vrshr_n_u32::<3>(tl);
    let mean1 = vget_lane_u32::<0>(pairs) as u8;
    let mean2 = vgetq_lane_u32::<1>(quads) as u8;
    let mean3 = vgetq_lane_u32::<3>(quads) as u8;
    let mean4 = vget_lane_u32::<1>(pairs) as u8;
    ([mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2], [mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4])
}

/// `WelsIChromaPredPlane_AArch64_neon`, the coefficient half.
///
/// `[t2 t1 t0 t-1 | l2 l1 l0 l-1]` subtracted from `[t4 .. t7 | l4 .. l7]`,
/// `mul` by `[17, 34, 51, 68]` twice over, `saddlp`/`addp` to `17H` and `17V`,
/// `sqrshrn #5`.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_plane_coeffs<S: RefSamples>(src: &S) -> (i16, i16, i16) {
    let t = src.row_n::<4>(-1, -1);
    let t4 = src.row_n::<4>(-1, 4);
    let inner = ld8(&[t[3], t[2], t[1], t[0], src.at(-1, 2), src.at(-1, 1), src.at(-1, 0), src.at(-1, -1)]);
    let outer = ld8(&[t4[0], t4[1], t4[2], t4[3], src.at(-1, 4), src.at(-1, 5), src.at(-1, 6), src.at(-1, 7)]);
    let d = vreinterpretq_s16_u16(vsubl_u8(outer, inner));
    let m = vmulq_s16(d, ld8_i16(&INTRA_1_TO_4));
    let p = vpaddlq_s16(m);
    let hv = vpaddq_s32(p, p);
    let b = ((vgetq_lane_s32::<0>(hv) + 16) >> 5) as i16;
    let c = ((vgetq_lane_s32::<1>(hv) + 16) >> 5) as i16;
    let a = (t4[3] as i16 + src.at(-1, 7) as i16) << 4;
    (a, b, c)
}

/// The fill half: `a + b (x - 3) + c (y - 3)`, `sqrshrun #5`.
#[inline]
#[target_feature(enable = "neon")]
fn chroma_plane_fill<O: PredOut>(out: &mut O, a: i16, b: i16, c: i16) {
    let base = vdupq_n_s16(a.wrapping_sub(c.wrapping_mul(3)));
    let mut row = vmlaq_n_s16(base, ld8_i16(&INTRA_M3_TO_P4), b);
    let cv = vdupq_n_s16(c);
    for dy in 0..8 {
        out.put(dy, &to8(vqrshrun_n_s16::<5>(row)));
        row = vaddq_s16(row, cv);
    }
}

// ============================================================================
// 4x4 luma
// ============================================================================

/// `WelsI4x4LumaPredDc_AArch64_neon`: `[top | left]`, `uaddlv`, `uqrshrn #3`.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_dc<S: RefSamples>(src: &S) -> u8 {
    let t = src.row_n::<4>(-1, 0);
    let both = ld8(&[t[0], t[1], t[2], t[3], src.at(-1, 0), src.at(-1, 1), src.at(-1, 2), src.at(-1, 3)]);
    ((vaddlv_u8(both) as u32 + 4) >> 3) as u8
}

/// `WelsI4x4LumaPredDDL_AArch64_neon`: `t0 + 2 t1 + t2` and on, with `t7` repeated
/// past the end, `uqrshrn #2`, rows at `ext` offsets 0 to 3.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_ddl<S: RefSamples>(src: &S) -> [u8; 16] {
    let top = ld8(&src.row_n::<8>(-1, 0));
    let last = vdup_lane_u8::<7>(top);
    let t1 = vext_u8::<1>(top, last);
    let t2 = vext_u8::<2>(top, last);
    let sum = vaddq_u16(vaddl_u8(t2, top), vshll_n_u8::<1>(t1));
    let r = vqrshrn_n_u16::<2>(sum);
    pack4(low4(r), low4(vext_u8::<1>(r, r)), low4(vext_u8::<2>(r, r)), low4(vext_u8::<3>(r, r)))
}

/// `WelsI4x4LumaPredVL_AArch64_neon`: the two-tap and three-tap lines of the top
/// row, rows 0 and 1 from their heads and rows 2 and 3 one lane along.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_vl<S: RefSamples>(src: &S) -> [u8; 16] {
    let top = ld8(&src.row_n::<8>(-1, 0));
    let pairs = vaddl_u8(vext_u8::<1>(top, top), top);
    let two = vqrshrn_n_u16::<1>(pairs);
    let triples = vaddq_u16(vextq_u16::<1>(pairs, pairs), pairs);
    let three = vqrshrn_n_u16::<2>(triples);
    pack4(low4(two), low4(three), low4(vext_u8::<1>(two, two)), low4(vext_u8::<1>(three, three)))
}

/// `WelsI4x4LumaPredVR_AArch64_neon` on the line `l2 l1 l0 lt t0 t1 t2 t3`.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_vr<S: RefSamples>(src: &S) -> [u8; 16] {
    let t = src.row_n::<4>(-1, 0);
    let line = ld8(&[src.at(-1, 2), src.at(-1, 1), src.at(-1, 0), src.at(-1, -1), t[0], t[1], t[2], t[3]]);
    let pairs = vaddl_u8(vext_u8::<7>(line, line), line);
    let triples = vaddq_u16(pairs, vextq_u16::<7>(pairs, pairs));
    let three = vqrshrn_n_u16::<2>(triples);
    let two = vqrshrn_n_u16::<1>(pairs);
    let row2 = vset_lane_u8::<4>(vget_lane_u8::<3>(three), vext_u8::<7>(two, two));
    let shifted = vext_u8::<7>(three, three);
    let row3 = vset_lane_u8::<4>(vget_lane_u8::<3>(shifted), shifted);
    pack4(high4(two), high4(three), high4(row2), high4(row3))
}

/// `WelsI4x4LumaPredHU_AArch64_neon` on the line `l3 l3 l3 l3 l0 l1 l2 l3`.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_hu<S: RefSamples>(src: &S) -> [u8; 16] {
    let l3 = src.at(-1, 3);
    let line = ld8(&[l3, l3, l3, l3, src.at(-1, 0), src.at(-1, 1), src.at(-1, 2), l3]);
    let pairs = vaddl_u8(line, vext_u8::<1>(line, line));
    let triples = vaddq_u16(vextq_u16::<1>(pairs, pairs), pairs);
    let two = vqrshrn_n_u16::<1>(pairs);
    let three = vqrshrn_n_u16::<2>(triples);
    let z = vzip2_u8(two, three); // hu0 hu1 hu2 hu3 hu4 hu5 l3 l3
    let z = vset_lane_u8::<7>(l3, vset_lane_u8::<6>(l3, z));
    pack4(low4(z), low4(vext_u8::<2>(z, line)), high4(z), [l3; 4])
}

/// `WelsI4x4LumaPredHD_AArch64_neon` on the line `l3 l2 l1 l0 lt t0 t1 t2`.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_hd<S: RefSamples>(src: &S) -> [u8; 16] {
    let t = src.row_n::<4>(-1, 0);
    let line = ld8(&[src.at(-1, 3), src.at(-1, 2), src.at(-1, 1), src.at(-1, 0), src.at(-1, -1), t[0], t[1], t[2]]);
    let pairs = vaddl_u8(line, vext_u8::<1>(line, line));
    let triples = vaddq_u16(vextq_u16::<1>(pairs, pairs), pairs);
    let two = vqrshrn_n_u16::<1>(pairs); // hd6 hd4 hd2 hd0 ..
    let three = vqrshrn_n_u16::<2>(triples); // hd5 hd3 hd1 hd7 hd8 hd9 ..
    let z = vzip1_u8(two, three); // hd6 hd5 hd4 hd3 hd2 hd1 hd0 hd7
    let tail = vreinterpret_u8_u16(vset_lane_u16::<0>(vget_lane_u16::<2>(vreinterpret_u16_u8(three)), vreinterpret_u16_u8(line)));
    pack4(low4(vext_u8::<6>(z, tail)), high4(z), low4(vext_u8::<2>(z, tail)), low4(z))
}

/// Diagonal down-right, in the idiom of `HD` and `VR` — see the header. On the line
/// `l3 l2 l1 l0 lt t0 t1 t2 t3` the three-tap outputs `f[0..7]` are `ddr6 .. ddr4,
/// ddr0 .. ddr3`, and row `r` is `f[3 - r ..]`.
#[inline]
#[target_feature(enable = "neon")]
fn i4x4_ddr<S: RefSamples>(src: &S) -> [u8; 16] {
    let t = src.row_n::<4>(-1, 0);
    let line = [
        src.at(-1, 3),
        src.at(-1, 2),
        src.at(-1, 1),
        src.at(-1, 0),
        src.at(-1, -1),
        t[0],
        t[1],
        t[2],
        t[3],
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let (l0, l1, l2) = (ld8(&line[0..]), ld8(&line[1..]), ld8(&line[2..]));
    let sum = vaddq_u16(vaddl_u8(l0, l2), vshll_n_u8::<1>(l1));
    let f = vqrshrn_n_u16::<2>(sum);
    pack4(low4(vext_u8::<3>(f, f)), low4(vext_u8::<2>(f, f)), low4(vext_u8::<1>(f, f)), low4(f))
}

/// Four packed rows into a `PredOut`.
#[inline(always)]
fn put4<O: PredOut>(out: &mut O, rows: &[u8; 16]) {
    for dy in 0..4 {
        let row: &[u8; 4] = rows[dy * 4..][..4].try_into().expect("row");
        out.put(dy, row);
    }
}

// ============================================================================
// The entry points, named as the slots they fill
// ============================================================================

/// `WelsI16x16LumaPredV_AArch64_neon`.
#[inline]
pub fn enc_i16x16_luma_pred_v(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let top = rec.row_n::<16>(-1, 0);
    fill_rows(&mut Packed::<16>(pred), 16, &top)
}

/// `WelsDecoderI16x16LumaPredV_AArch64_neon`.
#[inline]
pub fn dec_i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<16>(-1, 0);
    fill_rows(pred, 16, &top)
}

/// `WelsI16x16LumaPredH_AArch64_neon`.
#[inline]
pub fn enc_i16x16_luma_pred_h(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let out = &mut Packed::<16>(pred);
    for dy in 0..16 {
        out.put(dy, &[rec.at(-1, dy as isize); 16]);
    }
}

/// `WelsDecoderI16x16LumaPredH_AArch64_neon`.
#[inline]
pub fn dec_i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..16 {
        let l = pred.at(-1, dy as isize);
        pred.put(dy, &[l; 16]);
    }
}

/// `WelsI16x16LumaPredDc_AArch64_neon`.
#[inline]
pub fn enc_i16x16_luma_pred_dc(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    // SAFETY: NEON is baseline on aarch64; see the module header.
    let mean = unsafe { i16x16_dc_mean(rec, true, true) };
    fill_rows(&mut Packed::<16>(pred), 16, &[mean; 16])
}

/// `WelsDecoderI16x16LumaPredDc_AArch64_neon`.
#[inline]
pub fn dec_i16x16_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let mean = unsafe { i16x16_dc_mean(pred, true, true) };
    fill_rows(pred, 16, &[mean; 16])
}

/// `WelsDecoderI16x16LumaPredDcTop_AArch64_neon`.
#[inline]
pub fn dec_i16x16_luma_pred_dc_top(pred: &mut PlaneCursorMut<'_>) {
    let mean = unsafe { i16x16_dc_mean(pred, true, false) };
    fill_rows(pred, 16, &[mean; 16])
}

/// The `DC_128` fill, which upstream keeps in C.
#[inline]
pub fn dec_i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    fill_rows(pred, 16, &[0x80u8; 16])
}

/// `WelsI16x16LumaPredPlane_AArch64_neon`.
#[inline]
pub fn enc_i16x16_luma_pred_plane(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let (a, b, c) = unsafe { i16x16_plane_coeffs(rec) };
    unsafe { i16x16_plane_fill(&mut Packed::<16>(pred), a, b, c) }
}

/// `WelsDecoderI16x16LumaPredPlane_AArch64_neon`.
#[inline]
pub fn dec_i16x16_luma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    let (a, b, c) = unsafe { i16x16_plane_coeffs(pred) };
    unsafe { i16x16_plane_fill(pred, a, b, c) }
}

/// `WelsIChromaPredV_AArch64_neon`.
#[inline]
pub fn enc_chroma_pred_v(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let top = rec.row_n::<8>(-1, 0);
    fill_rows(&mut Packed::<8>(pred), 8, &top)
}

/// `WelsDecoderIChromaPredV_AArch64_neon`.
#[inline]
pub fn dec_chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<8>(-1, 0);
    fill_rows(pred, 8, &top)
}

/// `WelsIChromaPredH_AArch64_neon`.
#[inline]
pub fn enc_chroma_pred_h(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let out = &mut Packed::<8>(pred);
    for dy in 0..8 {
        out.put(dy, &[rec.at(-1, dy as isize); 8]);
    }
}

/// `WelsDecoderIChromaPredH_AArch64_neon`.
#[inline]
pub fn dec_chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        let l = pred.at(-1, dy as isize);
        pred.put(dy, &[l; 8]);
    }
}

/// `WelsIChromaPredDc_AArch64_neon`.
#[inline]
pub fn enc_chroma_pred_dc(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let (top, bot) = unsafe { chroma_dc_rows(rec) };
    let out = &mut Packed::<8>(pred);
    for dy in 0..4 {
        out.put(dy, &top);
    }
    for dy in 4..8 {
        out.put(dy, &bot);
    }
}

/// `WelsDecoderIChromaPredDc_AArch64_neon`.
#[inline]
pub fn dec_chroma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let (top, bot) = unsafe { chroma_dc_rows(pred) };
    for dy in 0..4 {
        pred.put(dy, &top);
    }
    for dy in 4..8 {
        pred.put(dy, &bot);
    }
}

/// `WelsIChromaPredPlane_AArch64_neon`.
#[inline]
pub fn enc_chroma_pred_plane(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let (a, b, c) = unsafe { chroma_plane_coeffs(rec) };
    unsafe { chroma_plane_fill(&mut Packed::<8>(pred), a, b, c) }
}

/// `WelsDecoderIChromaPredPlane_AArch64_neon`.
#[inline]
pub fn dec_chroma_pred_plane(pred: &mut PlaneCursorMut<'_>) {
    let (a, b, c) = unsafe { chroma_plane_coeffs(pred) };
    unsafe { chroma_plane_fill(pred, a, b, c) }
}

/// The 4x4 vertical fill, which upstream keeps in C.
#[inline]
pub fn enc_i4x4_luma_pred_v(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<4>(-1, 0);
    put4(&mut Packed::<4>(pred), &pack4(top, top, top, top))
}

/// The 4x4 vertical fill, in place.
#[inline]
pub fn dec_i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<4>(-1, 0);
    fill_rows(pred, 4, &top)
}

/// `WelsI4x4LumaPredH_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_h(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let out = &mut Packed::<4>(pred);
    for dy in 0..4 {
        out.put(dy, &[rec.at(-1, dy as isize); 4]);
    }
}

/// `WelsDecoderI4x4LumaPredH_AArch64_neon`.
#[inline]
pub fn dec_i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..4 {
        let l = pred.at(-1, dy as isize);
        pred.put(dy, &[l; 4]);
    }
}

/// `WelsI4x4LumaPredDc_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_dc(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let mean = unsafe { i4x4_dc(rec) };
    *pred = [mean; 16];
}

/// `WelsDecoderI4x4LumaPredDc_AArch64_neon`.
#[inline]
pub fn dec_i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let mean = unsafe { i4x4_dc(pred) };
    fill_rows(pred, 4, &[mean; 4])
}

/// `WelsI4x4LumaPredDDL_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_ddl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_ddl(rec) };
}

/// Diagonal down-right; see the header.
#[inline]
pub fn enc_i4x4_luma_pred_ddr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_ddr(rec) };
}

/// `WelsI4x4LumaPredVR_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_vr(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_vr(rec) };
}

/// `WelsI4x4LumaPredHD_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_hd(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_hd(rec) };
}

/// `WelsI4x4LumaPredVL_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_vl(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_vl(rec) };
}

/// `WelsI4x4LumaPredHU_AArch64_neon`.
#[inline]
pub fn enc_i4x4_luma_pred_hu(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    *pred = unsafe { i4x4_hu(rec) };
}

// ============================================================================
// Unit Tests (Parity against scalar implementations)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::get_intra_predictor::*;
    use crate::encoder::rec_view::shared_plane_for_test;
    use crate::safe::plane::PaddedPlane;

    fn test_plane(w: usize, h: usize, pad: usize, stride: usize) -> PaddedPlane {
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, (((y + 37) * stride as isize + (x + 43)) % 251) as u8);
            }
        }
        p
    }

    /// A 64-bit LCG, so a failing seed is replayable.
    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u8
    }

    fn noise_plane(seed: &mut u64) -> PaddedPlane {
        let (w, h, pad, stride) = (32usize, 32usize, 16usize, 64usize);
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, lcg(seed));
            }
        }
        p
    }

    /// A plane whose neighbours are all one value — the ends of every mean and of
    /// the plane predictors' `a`.
    fn flat_plane(v: u8) -> PaddedPlane {
        let (w, h, pad, stride) = (32usize, 32usize, 16usize, 64usize);
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                p.set(x, y, v);
            }
        }
        p
    }

    /// The steepest plane gradients: 0 on one side of the edge, 255 on the other.
    fn step_plane(rising: bool) -> PaddedPlane {
        let (w, h, pad, stride) = (32usize, 32usize, 16usize, 64usize);
        let mut p = PaddedPlane::new(w, h, pad, stride);
        for y in -(pad as isize)..(h + pad) as isize {
            for x in -(pad as isize)..(w + pad) as isize {
                let hi = (x + y >= 8) == rising;
                p.set(x, y, if hi { 255 } else { 0 });
            }
        }
        p
    }

    fn enc_planes() -> Vec<PaddedPlane> {
        let mut seed = 0x5DEECE66Du64;
        let mut v = vec![test_plane(32, 32, 16, 64), flat_plane(0), flat_plane(255), step_plane(true), step_plane(false)];
        for _ in 0..6 {
            v.push(noise_plane(&mut seed));
        }
        v
    }

    #[test]
    fn test_i16x16_luma_pred_parity() {
        for mut p in enc_planes() {
            let view = shared_plane_for_test(&mut p);
            for anchor in [(0isize, 0isize), (16, 16), (8, 4)] {
                let rec = view.cursor(anchor.0, anchor.1);
                let pairs: [(&str, fn(&mut [u8; 256], &RecCursor), fn(&mut [u8; 256], &RecCursor)); 4] = [
                    ("V", WelsI16x16LumaPredV_c, enc_i16x16_luma_pred_v),
                    ("H", WelsI16x16LumaPredH_c, enc_i16x16_luma_pred_h),
                    ("DC", WelsI16x16LumaPredDc_c, enc_i16x16_luma_pred_dc),
                    ("Plane", WelsI16x16LumaPredPlane_c, enc_i16x16_luma_pred_plane),
                ];
                for (name, scalar, simd) in pairs {
                    let mut want = [0u8; 256];
                    let mut got = [0u8; 256];
                    scalar(&mut want, &rec);
                    simd(&mut got, &rec);
                    assert_eq!(got, want, "16x16 {name} mismatch at {anchor:?}");
                }
            }
        }
    }

    #[test]
    fn test_chroma_pred_parity() {
        for mut p in enc_planes() {
            let view = shared_plane_for_test(&mut p);
            for anchor in [(0isize, 0isize), (8, 8), (16, 4)] {
                let rec = view.cursor(anchor.0, anchor.1);
                let pairs: [(&str, fn(&mut [u8; 64], &RecCursor), fn(&mut [u8; 64], &RecCursor)); 4] = [
                    ("V", WelsIChromaPredV_c, enc_chroma_pred_v),
                    ("H", WelsIChromaPredH_c, enc_chroma_pred_h),
                    ("DC", WelsIChromaPredDc_c, enc_chroma_pred_dc),
                    ("Plane", WelsIChromaPredPlane_c, enc_chroma_pred_plane),
                ];
                for (name, scalar, simd) in pairs {
                    let mut want = [0u8; 64];
                    let mut got = [0u8; 64];
                    scalar(&mut want, &rec);
                    simd(&mut got, &rec);
                    assert_eq!(got, want, "Chroma {name} mismatch at {anchor:?}");
                }
            }
        }
    }

    #[test]
    fn test_i4x4_luma_pred_parity() {
        for mut p in enc_planes() {
            let view = shared_plane_for_test(&mut p);
            for anchor in [(0isize, 0isize), (4, 4), (12, 8), (8, 20)] {
                let rec = view.cursor(anchor.0, anchor.1);
                let pairs: [(&str, fn(&mut [u8; 16], &RecCursor), fn(&mut [u8; 16], &RecCursor)); 9] = [
                    ("V", WelsI4x4LumaPredV_c, enc_i4x4_luma_pred_v),
                    ("H", WelsI4x4LumaPredH_c, enc_i4x4_luma_pred_h),
                    ("DC", WelsI4x4LumaPredDc_c, enc_i4x4_luma_pred_dc),
                    ("DDL", WelsI4x4LumaPredDDL_c, enc_i4x4_luma_pred_ddl),
                    ("DDR", WelsI4x4LumaPredDDR_c, enc_i4x4_luma_pred_ddr),
                    ("VR", WelsI4x4LumaPredVR_c, enc_i4x4_luma_pred_vr),
                    ("HD", WelsI4x4LumaPredHD_c, enc_i4x4_luma_pred_hd),
                    ("VL", WelsI4x4LumaPredVL_c, enc_i4x4_luma_pred_vl),
                    ("HU", WelsI4x4LumaPredHU_c, enc_i4x4_luma_pred_hu),
                ];
                for (name, scalar, simd) in pairs {
                    let mut want = [0u8; 16];
                    let mut got = [0u8; 16];
                    scalar(&mut want, &rec);
                    simd(&mut got, &rec);
                    assert_eq!(got, want, "4x4 {name} mismatch at {anchor:?}");
                }
            }
        }
    }

    // ========================================================================
    // The decoder-side predictors: the in-place reconstructors, compared over the
    // whole allocation of two identically built planes. The reference is
    // `decoder::get_intra_predictor`, which has no SIMD dispatch of its own.
    // ========================================================================

    fn assert_dec_parity(name: &str, scalar: fn(&mut PlaneCursorMut<'_>), simd: fn(&mut PlaneCursorMut<'_>)) {
        let mut seed = 0xB502_6F5Au64;
        let mut planes = vec![test_plane(32, 32, 16, 64), flat_plane(0), flat_plane(255), step_plane(true), step_plane(false)];
        planes.push(noise_plane(&mut seed));
        planes.push(noise_plane(&mut seed));
        for pa in planes {
            for anchor in [(8isize, 8isize), (4, 12), (16, 16)] {
                let mut a = pa.clone();
                let mut b = pa.clone();
                scalar(&mut a.cursor_mut(anchor.0, anchor.1));
                simd(&mut b.cursor_mut(anchor.0, anchor.1));
                assert_eq!(a.as_slice(), b.as_slice(), "{name}: NEON and scalar disagree somewhere in the allocation");
            }
        }
    }

    #[test]
    fn dec_i16x16_luma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("16x16 V", dec::i16x16_luma_pred_v, dec_i16x16_luma_pred_v);
        assert_dec_parity("16x16 H", dec::i16x16_luma_pred_h, dec_i16x16_luma_pred_h);
        assert_dec_parity("16x16 DC", dec::i16x16_luma_pred_dc, dec_i16x16_luma_pred_dc);
        assert_dec_parity("16x16 DC top", dec::i16x16_luma_pred_dc_top, dec_i16x16_luma_pred_dc_top);
        assert_dec_parity("16x16 DC n/a", dec::i16x16_luma_pred_dc_na, dec_i16x16_luma_pred_dc_na);
        assert_dec_parity("16x16 Plane", dec::i16x16_luma_pred_plane, dec_i16x16_luma_pred_plane);
    }

    #[test]
    fn dec_chroma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("Chroma V", dec::chroma_pred_v, dec_chroma_pred_v);
        assert_dec_parity("Chroma H", dec::chroma_pred_h, dec_chroma_pred_h);
        assert_dec_parity("Chroma DC", dec::chroma_pred_dc, dec_chroma_pred_dc);
        assert_dec_parity("Chroma Plane", dec::chroma_pred_plane, dec_chroma_pred_plane);
    }

    #[test]
    fn dec_i4x4_luma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("4x4 V", dec::i4x4_luma_pred_v, dec_i4x4_luma_pred_v);
        assert_dec_parity("4x4 H", dec::i4x4_luma_pred_h, dec_i4x4_luma_pred_h);
        assert_dec_parity("4x4 DC", dec::i4x4_luma_pred_dc, dec_i4x4_luma_pred_dc);
    }

    /// **The naming rule, enforced.** Every public kernel here reaches at least one
    /// NEON intrinsic — in its own body or in a function it calls — unless it is one
    /// of the fills listed below as scalar by design. The x86_64 file explains why
    /// this is a test and not a review habit: a refactor emptied five kernels of
    /// their intrinsics there without touching their names.
    #[test]
    fn every_kernel_here_reaches_an_intrinsic() {
        /// The broadcasts and fills: a V fill is a row copy, an H fill a byte splat,
        /// `DC_128` a constant. Upstream keeps most of these in C on arm64 too.
        const SCALAR_BY_DESIGN: [&str; 13] = [
            "enc_i16x16_luma_pred_v",
            "dec_i16x16_luma_pred_v",
            "enc_i16x16_luma_pred_h",
            "dec_i16x16_luma_pred_h",
            "dec_i16x16_luma_pred_dc_na",
            "enc_chroma_pred_v",
            "dec_chroma_pred_v",
            "enc_chroma_pred_h",
            "dec_chroma_pred_h",
            "enc_i4x4_luma_pred_v",
            "dec_i4x4_luma_pred_v",
            "enc_i4x4_luma_pred_h",
            "dec_i4x4_luma_pred_h",
        ];

        let src = include_str!("intra_pred.rs");
        let src = src.split("#[cfg(test)]").next().expect("source before the tests");

        // Body of the item starting at byte `i`, by brace matching.
        fn body_at(s: &str, i: usize) -> &str {
            let b = s.as_bytes();
            let (mut depth, mut j, mut started) = (0usize, i, false);
            while j < b.len() {
                match b[j] {
                    b'{' => {
                        depth += 1;
                        started = true;
                    }
                    b'}' if started => {
                        depth -= 1;
                        if depth == 0 {
                            return &s[i..=j];
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            &s[i..]
        }

        let ident = |s: &str| -> String { s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect() };

        let mut bodies: Vec<(String, &str)> = Vec::new();
        for (off, _) in src.match_indices("fn ") {
            let name = ident(&src[off + 3..]);
            if !name.is_empty() {
                bodies.push((name, body_at(src, off)));
            }
        }

        let mut public: Vec<String> = Vec::new();
        for (off, _) in src.match_indices("pub ") {
            let rest = &src[off + 4..];
            let rest = rest.strip_prefix("(crate) ").unwrap_or(rest);
            let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
            if let Some(rest) = rest.strip_prefix("fn ") {
                let name = ident(rest);
                if !name.is_empty() {
                    public.push(name);
                }
            }
        }
        assert!(public.len() >= 30, "found only {} public kernels — the scan broke", public.len());

        // A NEON intrinsic: `v…` with a lane-type suffix, which no other identifier
        // in this file has.
        fn is_intrinsic(tok: &str) -> bool {
            tok.starts_with('v')
                && ["_u8", "_s8", "_u16", "_s16", "_u32", "_s32", "_u64", "_s64"].iter().any(|s| tok.ends_with(s))
        }
        let intrinsics = |b: &str| b.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(is_intrinsic);

        let mut offenders = Vec::new();
        for name in &public {
            if SCALAR_BY_DESIGN.contains(&name.as_str()) {
                continue;
            }
            let body = bodies.iter().find(|(n, _)| n == name).map(|(_, b)| *b).unwrap_or("");
            if intrinsics(body) {
                continue;
            }
            let called: std::collections::HashSet<&str> =
                body.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|t| !t.is_empty()).collect();
            let reaches = bodies.iter().any(|(callee, cbody)| callee != name && called.contains(callee.as_str()) && intrinsics(cbody));
            if !reaches {
                offenders.push(name.clone());
            }
        }
        assert!(
            offenders.is_empty(),
            "these are installed as aarch64 kernels but reach no NEON intrinsic — implement them, or add them to \
             `SCALAR_BY_DESIGN` and say why: {offenders:?}"
        );

        for name in SCALAR_BY_DESIGN {
            let body = bodies.iter().find(|(n, _)| n == name).map(|(_, b)| *b);
            let body = body.unwrap_or_else(|| panic!("`{name}` is exempt but no longer exists"));
            assert!(!intrinsics(body), "`{name}` is exempt but now has intrinsics — drop it from the list");
        }
    }
}
