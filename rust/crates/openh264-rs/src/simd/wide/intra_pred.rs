//! Intra prediction on `wide` lane types — the twin of `simd::x86_64::intra_pred`,
//! serving both the encoder's packed candidate buffers and the decoder's in-place
//! reconstruction through the same `PredOut` seam.
//!
//! The two plane fills are word multiplies, adds, an arithmetic shift and a
//! `packuswb`, all direct `wide` calls. The 16x16 DC mean is where the intrinsic
//! kernel used `psadbw` against zero as a byte sum; here it is two zero-extends and
//! a `pmaddwd` reduce. The 4x4 predictors were a `_mm_setr_epi8` and a store in the
//! intrinsic file, which is a 16-byte array assignment by another name, and that is
//! what they are here.
//!
//! Names keep the intrinsic file's `_sse2` suffix where it has one, so the install
//! tables resolve the same identifiers against either module.

#![forbid(unsafe_code)]

use wide::bytemuck::cast;
use wide::{i16x8, u32x4, u8x16};

use super::lanes::{hsum_i16, load16, low8, narrow, widen_hi, widen_lo};
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

// ============================================================================
// 16x16 luma
// ============================================================================

/// `(top_shift, left_shift, lt_shift)` for the 16x16 plane predictor.
#[inline(always)]
fn i16x16_plane_coeffs<S: RefSamples>(src: &S) -> (i32, i32, i32) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..8isize {
        top_sum += (i as i32 + 1) * (src.at(8 + i, -1) as i32 - src.at(6 - i, -1) as i32);
        left_sum += (i as i32 + 1) * (src.at(-1, 8 + i) as i32 - src.at(-1, 6 - i) as i32);
    }
    let lt_shift = (src.at(-1, 15) as i32 + src.at(15, -1) as i32) << 4i32;
    ((5 * top_sum + 32) >> 6i32, (5 * left_sum + 32) >> 6i32, lt_shift)
}

/// The 16x16 plane fill from the three coefficients.
#[inline(always)]
fn i16x16_plane_fill<O: PredOut>(out: &mut O, top_shift: i32, left_shift: i32, lt_shift: i32) {
    let inc_minus = i16x8::new([-7, -6, -5, -4, -3, -2, -1, 0]);
    let inc = i16x8::new([1, 2, 3, 4, 5, 6, 7, 8]);
    let b_vec = i16x8::splat(top_shift as i16);
    let c_vec = i16x8::splat(left_shift as i16);
    let mut s_vec = i16x8::splat((lt_shift + 16 - 7 * left_shift) as i16);

    let term_lo = b_vec * inc_minus;
    let term_hi = b_vec * inc;

    for dy in 0..16 {
        let row_lo = (term_lo + s_vec) >> 5i32;
        let row_hi = (term_hi + s_vec) >> 5i32;
        out.put(dy, &narrow(row_lo, row_hi).to_array());
        s_vec = s_vec + c_vec;
    }
}

/// The 16x16 DC mean over whichever neighbour edges the variant uses.
#[inline(always)]
fn i16x16_dc_mean<S: RefSamples>(src: &S, use_top: bool, use_left: bool) -> u8 {
    let sum_top = if use_top {
        let top = load16(&src.row_n::<16>(-1, 0));
        hsum_i16(widen_lo(top) + widen_hi(top))
    } else {
        0
    };
    let sum_left = if use_left { (0..16).map(|y| src.at(-1, y as isize) as i32).sum() } else { 0 };
    match (use_top, use_left) {
        (true, true) => ((16 + sum_top + sum_left) >> 5i32) as u8,
        (true, false) => ((8 + sum_top) >> 4i32) as u8,
        (false, true) => ((8 + sum_left) >> 4i32) as u8,
        (false, false) => 0x80,
    }
}

/// Fills every row with the neighbour row above — the V predictors.
#[inline(always)]
fn pred_v<const N: usize, S: RefSamples, O: PredOut>(src: &S, out: &mut O, rows: usize) {
    let top = src.row_n::<N>(-1, 0);
    fill_rows(out, rows, &top);
}

/// Fills each row with its own left neighbour — the H predictors.
#[inline(always)]
fn pred_h<const N: usize, S: RefSamples, O: PredOut>(src: &S, out: &mut O, rows: usize) {
    for dy in 0..rows {
        out.put(dy, &[src.at(-1, dy as isize); N]);
    }
}

#[inline]
pub fn enc_i16x16_luma_pred_v(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    pred_v::<16, _, _>(rec, &mut Packed::<16>(pred), 16)
}

#[inline]
pub fn dec_i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<16>(-1, 0);
    fill_rows(pred, 16, &top)
}

#[inline]
pub fn enc_i16x16_luma_pred_h(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    pred_h::<16, _, _>(rec, &mut Packed::<16>(pred), 16)
}

#[inline]
pub fn dec_i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..16 {
        let v = pred.at(-1, dy as isize);
        pred.put(dy, &[v; 16]);
    }
}

#[inline]
pub fn enc_i16x16_luma_pred_dc_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let mean = i16x16_dc_mean(rec, true, true);
    fill_rows(&mut Packed::<16>(pred), 16, &[mean; 16])
}

#[inline]
pub fn dec_i16x16_luma_pred_dc_sse2(pred: &mut PlaneCursorMut<'_>) {
    let mean = i16x16_dc_mean(pred, true, true);
    fill_rows(pred, 16, &[mean; 16])
}

#[inline]
pub fn dec_i16x16_luma_pred_dc_top_sse2(pred: &mut PlaneCursorMut<'_>) {
    let mean = i16x16_dc_mean(pred, true, false);
    fill_rows(pred, 16, &[mean; 16])
}

#[inline]
pub fn dec_i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    fill_rows(pred, 16, &[0x80u8; 16])
}

#[inline]
pub fn enc_i16x16_luma_pred_plane_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let (top_shift, left_shift, lt_shift) = i16x16_plane_coeffs(rec);
    i16x16_plane_fill(&mut Packed::<16>(pred), top_shift, left_shift, lt_shift)
}

#[inline]
pub fn dec_i16x16_luma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let (top_shift, left_shift, lt_shift) = i16x16_plane_coeffs(pred);
    i16x16_plane_fill(pred, top_shift, left_shift, lt_shift)
}

// ============================================================================
// 8x8 chroma
// ============================================================================

/// The four quadrant means of the chroma DC predictor, as the two row values.
#[inline(always)]
fn chroma_dc_rows<S: RefSamples>(src: &S) -> ([u8; 8], [u8; 8]) {
    let top = src.row_n::<8>(-1, 0);
    let sum_top0: i32 = (0..4).map(|i| top[i] as i32).sum();
    let sum_top1: i32 = (0..4).map(|i| top[i + 4] as i32).sum();
    let sum_left0: i32 = (0..4).map(|y| src.at(-1, y as isize) as i32).sum();
    let sum_left1: i32 = (0..4).map(|y| src.at(-1, (y + 4) as isize) as i32).sum();

    let mean1 = ((sum_top0 + sum_left0 + 4) >> 3i32) as u8;
    let mean2 = ((sum_top1 + 2) >> 2i32) as u8;
    let mean3 = ((sum_left1 + 2) >> 2i32) as u8;
    let mean4 = ((sum_top1 + sum_left1 + 4) >> 3i32) as u8;

    (
        [mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2],
        [mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4],
    )
}

/// `(top_shift, left_shift, lt_shift)` for the chroma plane predictor.
#[inline(always)]
fn chroma_plane_coeffs<S: RefSamples>(src: &S) -> (i32, i32, i32) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..4isize {
        top_sum += (i as i32 + 1) * (src.at(4 + i, -1) as i32 - src.at(2 - i, -1) as i32);
        left_sum += (i as i32 + 1) * (src.at(-1, 4 + i) as i32 - src.at(-1, 2 - i) as i32);
    }
    let lt_shift = (src.at(-1, 7) as i32 + src.at(7, -1) as i32) << 4i32;
    ((17 * top_sum + 16) >> 5i32, (17 * left_sum + 16) >> 5i32, lt_shift)
}

/// The chroma plane fill from the three coefficients.
#[inline(always)]
fn chroma_plane_fill<O: PredOut>(out: &mut O, top_shift: i32, left_shift: i32, lt_shift: i32) {
    let mul_b = i16x8::new([-3, -2, -1, 0, 1, 2, 3, 4]);
    let b_vec = i16x8::splat(top_shift as i16);
    let c_vec = i16x8::splat(left_shift as i16);
    let mut s_vec = i16x8::splat((lt_shift + 16 - 3 * left_shift) as i16);
    let term = b_vec * mul_b;

    for dy in 0..8 {
        let row_w = (term + s_vec) >> 5i32;
        out.put(dy, &low8(narrow(row_w, row_w)));
        s_vec = s_vec + c_vec;
    }
}

#[inline]
pub fn enc_chroma_pred_v(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    pred_v::<8, _, _>(rec, &mut Packed::<8>(pred), 8)
}

#[inline]
pub fn dec_chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<8>(-1, 0);
    fill_rows(pred, 8, &top)
}

#[inline]
pub fn enc_chroma_pred_h(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    pred_h::<8, _, _>(rec, &mut Packed::<8>(pred), 8)
}

#[inline]
pub fn dec_chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        let v = pred.at(-1, dy as isize);
        pred.put(dy, &[v; 8]);
    }
}

#[inline]
pub fn enc_chroma_pred_dc(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let (top, bot) = chroma_dc_rows(rec);
    let out = &mut Packed::<8>(pred);
    for dy in 0..4 {
        out.put(dy, &top);
    }
    for dy in 4..8 {
        out.put(dy, &bot);
    }
}

#[inline]
pub fn dec_chroma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let (top, bot) = chroma_dc_rows(pred);
    for dy in 0..4 {
        pred.put(dy, &top);
    }
    for dy in 4..8 {
        pred.put(dy, &bot);
    }
}

#[inline]
pub fn enc_chroma_pred_plane_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let (top_shift, left_shift, lt_shift) = chroma_plane_coeffs(rec);
    chroma_plane_fill(&mut Packed::<8>(pred), top_shift, left_shift, lt_shift)
}

#[inline]
pub fn dec_chroma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let (top_shift, left_shift, lt_shift) = chroma_plane_coeffs(pred);
    chroma_plane_fill(pred, top_shift, left_shift, lt_shift)
}

// ============================================================================
// 4x4 luma
// ============================================================================

#[inline]
pub fn enc_i4x4_luma_pred_v_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<4>(-1, 0);
    // The intrinsic kernel's `_mm_set1_epi32` and store: one dword to four lanes.
    *pred = cast(u32x4::splat(u32::from_ne_bytes(top)));
}

#[inline]
pub fn dec_i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 4] = pred.row(-1, 0, 4).try_into().unwrap();
    for dy in 0..4 {
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = top;
    }
}

#[inline]
pub fn enc_i4x4_luma_pred_h_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let l = |y: isize| rec.at(-1, y);
    let (l0, l1, l2, l3) = (l(0), l(1), l(2), l(3));
    *pred = [l0, l0, l0, l0, l1, l1, l1, l1, l2, l2, l2, l2, l3, l3, l3, l3];
}

#[inline]
pub fn dec_i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..4 {
        let val = pred.at(-1, dy);
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = [val; 4];
    }
}

#[inline]
pub fn enc_i4x4_luma_pred_dc_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<4>(-1, 0);
    let mut sum = 4i32;
    for y in 0..4 {
        sum += rec.at(-1, y as isize) as i32;
        sum += top[y] as i32;
    }
    *pred = u8x16::splat((sum >> 3i32) as u8).to_array();
}

#[inline]
pub fn dec_i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 4] = pred.row(-1, 0, 4).try_into().unwrap();
    let mut sum = 4i32;
    for y in 0..4 {
        sum += pred.at(-1, y as isize) as i32;
        sum += top[y] as i32;
    }
    let mean = (sum >> 3i32) as u8;
    for dy in 0..4 {
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = [mean; 4];
    }
}

#[inline]
pub fn enc_i4x4_luma_pred_ddl_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<8>(-1, 0);
    let t = |i: usize| top[i] as i32;
    let ddl0 = ((2 + t(0) + t(2) + (t(1) << 1i32)) >> 2i32) as u8;
    let ddl1 = ((2 + t(1) + t(3) + (t(2) << 1i32)) >> 2i32) as u8;
    let ddl2 = ((2 + t(2) + t(4) + (t(3) << 1i32)) >> 2i32) as u8;
    let ddl3 = ((2 + t(3) + t(5) + (t(4) << 1i32)) >> 2i32) as u8;
    let ddl4 = ((2 + t(4) + t(6) + (t(5) << 1i32)) >> 2i32) as u8;
    let ddl5 = ((2 + t(5) + t(7) + (t(6) << 1i32)) >> 2i32) as u8;
    let ddl6 = ((2 + t(6) + t(7) + (t(7) << 1i32)) >> 2i32) as u8;
    *pred = [
        ddl0, ddl1, ddl2, ddl3, ddl1, ddl2, ddl3, ddl4, ddl2, ddl3, ddl4, ddl5, ddl3, ddl4, ddl5, ddl6,
    ];
}

#[inline]
pub fn enc_i4x4_luma_pred_ddr_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let lt = rec.at(-1, -1) as i32;
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let l3 = rec.at(-1, 3) as i32;
    let top = rec.row_n::<4>(-1, 0);
    let (t0, t1, t2, t3) = (top[0] as i32, top[1] as i32, top[2] as i32, top[3] as i32);
    let tl0 = 1 + lt + l0;
    let lt0 = 1 + lt + t0;
    let t01 = 1 + t0 + t1;
    let t12 = 1 + t1 + t2;
    let t23 = 1 + t2 + t3;
    let l01 = 1 + l0 + l1;
    let l12 = 1 + l1 + l2;
    let l23 = 1 + l2 + l3;
    let ddr0 = ((tl0 + lt0) >> 2i32) as u8;
    let ddr1 = ((lt0 + t01) >> 2i32) as u8;
    let ddr2 = ((t01 + t12) >> 2i32) as u8;
    let ddr3 = ((t12 + t23) >> 2i32) as u8;
    let ddr4 = ((tl0 + l01) >> 2i32) as u8;
    let ddr5 = ((l01 + l12) >> 2i32) as u8;
    let ddr6 = ((l12 + l23) >> 2i32) as u8;
    *pred = [
        ddr0, ddr1, ddr2, ddr3, ddr4, ddr0, ddr1, ddr2, ddr5, ddr4, ddr0, ddr1, ddr6, ddr5, ddr4, ddr0,
    ];
}

#[inline]
pub fn enc_i4x4_luma_pred_vr_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let lt = rec.at(-1, -1) as i32;
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let top = rec.row_n::<4>(-1, 0);
    let (t0, t1, t2, t3) = (top[0] as i32, top[1] as i32, top[2] as i32, top[3] as i32);
    let vr0 = ((1 + lt + t0) >> 1i32) as u8;
    let vr1 = ((1 + t0 + t1) >> 1i32) as u8;
    let vr2 = ((1 + t1 + t2) >> 1i32) as u8;
    let vr3 = ((1 + t2 + t3) >> 1i32) as u8;
    let vr4 = ((2 + l0 + (lt << 1i32) + t0) >> 2i32) as u8;
    let vr5 = ((2 + lt + (t0 << 1i32) + t1) >> 2i32) as u8;
    let vr6 = ((2 + t0 + (t1 << 1i32) + t2) >> 2i32) as u8;
    let vr7 = ((2 + t1 + (t2 << 1i32) + t3) >> 2i32) as u8;
    let vr8 = ((2 + lt + (l0 << 1i32) + l1) >> 2i32) as u8;
    let vr9 = ((2 + l0 + (l1 << 1i32) + l2) >> 2i32) as u8;
    *pred = [vr0, vr1, vr2, vr3, vr4, vr5, vr6, vr7, vr8, vr0, vr1, vr2, vr9, vr4, vr5, vr6];
}

#[inline]
pub fn enc_i4x4_luma_pred_hd_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let lt = rec.at(-1, -1) as i32;
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let l3 = rec.at(-1, 3) as i32;
    let top = rec.row_n::<4>(-1, 0);
    let (t0, t1, t2) = (top[0] as i32, top[1] as i32, top[2] as i32);
    let hd0 = ((1 + lt + l0) >> 1i32) as u8;
    let hd1 = ((2 + lt + (l0 << 1i32) + l1) >> 2i32) as u8;
    let hd2 = ((1 + l0 + l1) >> 1i32) as u8;
    let hd3 = ((2 + l0 + (l1 << 1i32) + l2) >> 2i32) as u8;
    let hd4 = ((1 + l1 + l2) >> 1i32) as u8;
    let hd5 = ((2 + l1 + (l2 << 1i32) + l3) >> 2i32) as u8;
    let hd6 = ((1 + l2 + l3) >> 1i32) as u8;
    let hd7 = ((2 + l0 + (lt << 1i32) + t0) >> 2i32) as u8;
    let hd8 = ((2 + lt + (t0 << 1i32) + t1) >> 2i32) as u8;
    let hd9 = ((2 + t0 + (t1 << 1i32) + t2) >> 2i32) as u8;
    *pred = [hd0, hd7, hd8, hd9, hd2, hd1, hd0, hd7, hd4, hd3, hd2, hd1, hd6, hd5, hd4, hd3];
}

#[inline]
pub fn enc_i4x4_luma_pred_vl_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<7>(-1, 0);
    let t = |i: usize| top[i] as i32;
    let vl0 = ((1 + t(0) + t(1)) >> 1i32) as u8;
    let vl1 = ((1 + t(1) + t(2)) >> 1i32) as u8;
    let vl2 = ((1 + t(2) + t(3)) >> 1i32) as u8;
    let vl3 = ((1 + t(3) + t(4)) >> 1i32) as u8;
    let vl4 = ((1 + t(4) + t(5)) >> 1i32) as u8;
    let vl5 = ((2 + t(0) + (t(1) << 1i32) + t(2)) >> 2i32) as u8;
    let vl6 = ((2 + t(1) + (t(2) << 1i32) + t(3)) >> 2i32) as u8;
    let vl7 = ((2 + t(2) + (t(3) << 1i32) + t(4)) >> 2i32) as u8;
    let vl8 = ((2 + t(3) + (t(4) << 1i32) + t(5)) >> 2i32) as u8;
    let vl9 = ((2 + t(4) + (t(5) << 1i32) + t(6)) >> 2i32) as u8;
    *pred = [vl0, vl1, vl2, vl3, vl5, vl6, vl7, vl8, vl1, vl2, vl3, vl4, vl6, vl7, vl8, vl9];
}

#[inline]
pub fn enc_i4x4_luma_pred_hu_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let l3 = rec.at(-1, 3) as i32;
    let l01 = 1 + l0 + l1;
    let l12 = 1 + l1 + l2;
    let l23 = 1 + l2 + l3;
    let hu0 = (l01 >> 1i32) as u8;
    let hu1 = ((l01 + l12) >> 2i32) as u8;
    let hu2 = (l12 >> 1i32) as u8;
    let hu3 = ((l12 + l23) >> 2i32) as u8;
    let hu4 = (l23 >> 1i32) as u8;
    let hu5 = ((1 + l23 + (l3 << 1i32)) >> 2i32) as u8;
    let l3 = l3 as u8;
    *pred = [hu0, hu1, hu2, hu3, hu2, hu3, hu4, hu5, hu4, hu5, l3, l3, l3, l3, l3, l3];
}

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

    #[test]
    fn test_i16x16_luma_pred_sse2_parity() {
        let mut p = test_plane(32, 32, 16, 64);
        let view = shared_plane_for_test(&mut p);
        let rec = view.cursor(0, 0);

        // V
        let mut pred_c = [0u8; 256];
        let mut pred_simd = [0u8; 256];
        WelsI16x16LumaPredV_c(&mut pred_c, &rec);
        enc_i16x16_luma_pred_v(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "16x16 V mismatch");

        // H
        let mut pred_c = [0u8; 256];
        let mut pred_simd = [0u8; 256];
        WelsI16x16LumaPredH_c(&mut pred_c, &rec);
        enc_i16x16_luma_pred_h(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "16x16 H mismatch");

        // DC
        let mut pred_c = [0u8; 256];
        let mut pred_simd = [0u8; 256];
        WelsI16x16LumaPredDc_c(&mut pred_c, &rec);
        enc_i16x16_luma_pred_dc_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "16x16 DC mismatch");

        // Plane
        let mut pred_c = [0u8; 256];
        let mut pred_simd = [0u8; 256];
        WelsI16x16LumaPredPlane_c(&mut pred_c, &rec);
        enc_i16x16_luma_pred_plane_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "16x16 Plane mismatch");
    }

    #[test]
    fn test_chroma_pred_sse2_parity() {
        let mut p = test_plane(32, 32, 16, 64);
        let view = shared_plane_for_test(&mut p);
        let rec = view.cursor(0, 0);

        // V
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredV_c(&mut pred_c, &rec);
        enc_chroma_pred_v(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma V mismatch");

        // H
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredH_c(&mut pred_c, &rec);
        enc_chroma_pred_h(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma H mismatch");

        // DC
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredDc_c(&mut pred_c, &rec);
        enc_chroma_pred_dc(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma DC mismatch");

        // Plane
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredPlane_c(&mut pred_c, &rec);
        enc_chroma_pred_plane_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma Plane mismatch");
    }

    #[test]
    fn test_i4x4_luma_pred_sse2_parity() {
        let mut p = test_plane(32, 32, 16, 64);
        let view = shared_plane_for_test(&mut p);
        let rec = view.cursor(0, 0);

        // V
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredV_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_v_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 V mismatch");

        // H
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredH_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_h_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 H mismatch");

        // DC
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredDc_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_dc_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 DC mismatch");

        // DDL
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredDDL_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_ddl_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 DDL mismatch");

        // DDR
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredDDR_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_ddr_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 DDR mismatch");

        // VR
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredVR_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_vr_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 VR mismatch");

        // HD
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredHD_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_hd_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 HD mismatch");

        // VL
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredVL_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_vl_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 VL mismatch");

        // HU
        let mut pred_c = [0u8; 16];
        let mut pred_simd = [0u8; 16];
        WelsI4x4LumaPredHU_c(&mut pred_c, &rec);
        enc_i4x4_luma_pred_hu_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "4x4 HU mismatch");
    }

    // ========================================================================
    // The decoder-side predictors.
    //
    // The three tests above cover the twelve `enc_*` kernels — the encoder's packed
    // candidate buffers — and nothing covered the thirteen `dec_*` ones, which are the
    // in-place reconstructors the decoder installs at `decoder_core.rs:1817..1830`.
    // They are a different shape, not a different arithmetic: the encoder writes a
    // dense `[u8; N]` candidate, the decoder writes back into the picture through a
    // `PlaneCursorMut` whose neighbours are the samples it just read. That shape is
    // exactly where an off-by-one row or column hides, so these compare the *whole
    // allocation* of two identically built planes rather than the block.
    //
    // The reference on the other side is `decoder::get_intra_predictor`, which has no
    // SIMD dispatch of its own — the SSE2 kernels are installed over it in the table,
    // never called from it — so no assertion here can route back into the kernel under
    // test.
    // ========================================================================

    /// Two planes with identical content, both padded, for an in-place kernel pair.
    fn twin_pred_planes() -> (PaddedPlane, PaddedPlane) {
        (test_plane(32, 32, 16, 64), test_plane(32, 32, 16, 64))
    }

    /// Runs `scalar` and `simd` on twin planes anchored at `(8, 8)` and compares every
    /// byte of both allocations.
    fn assert_dec_parity(
        name: &str,
        scalar: fn(&mut PlaneCursorMut<'_>),
        simd: fn(&mut PlaneCursorMut<'_>),
    ) {
        let (mut pa, mut pb) = twin_pred_planes();
        assert_eq!(pa.as_slice(), pb.as_slice(), "{name}: twins started out different");
        scalar(&mut pa.cursor_mut(8, 8));
        simd(&mut pb.cursor_mut(8, 8));
        assert_eq!(
            pa.as_slice(),
            pb.as_slice(),
            "{name}: SSE2 and scalar disagree somewhere in the allocation"
        );
    }

    #[test]
    fn dec_i16x16_luma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("16x16 V", dec::i16x16_luma_pred_v, dec_i16x16_luma_pred_v);
        assert_dec_parity("16x16 H", dec::i16x16_luma_pred_h, dec_i16x16_luma_pred_h);
        assert_dec_parity("16x16 DC", dec::i16x16_luma_pred_dc, dec_i16x16_luma_pred_dc_sse2);
        assert_dec_parity(
            "16x16 DC top",
            dec::i16x16_luma_pred_dc_top,
            dec_i16x16_luma_pred_dc_top_sse2,
        );
        assert_dec_parity(
            "16x16 DC n/a",
            dec::i16x16_luma_pred_dc_na,
            dec_i16x16_luma_pred_dc_na,
        );
        assert_dec_parity(
            "16x16 Plane",
            dec::i16x16_luma_pred_plane,
            dec_i16x16_luma_pred_plane_sse2,
        );
    }

    #[test]
    fn dec_chroma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("Chroma V", dec::chroma_pred_v, dec_chroma_pred_v);
        assert_dec_parity("Chroma H", dec::chroma_pred_h, dec_chroma_pred_h);
        assert_dec_parity("Chroma DC", dec::chroma_pred_dc, dec_chroma_pred_dc);
        assert_dec_parity("Chroma Plane", dec::chroma_pred_plane, dec_chroma_pred_plane_sse2);
    }

    #[test]
    fn dec_i4x4_luma_pred_parity() {
        use crate::decoder::get_intra_predictor as dec;
        assert_dec_parity("4x4 V", dec::i4x4_luma_pred_v, dec_i4x4_luma_pred_v);
        assert_dec_parity("4x4 H", dec::i4x4_luma_pred_h, dec_i4x4_luma_pred_h);
        assert_dec_parity("4x4 DC", dec::i4x4_luma_pred_dc, dec_i4x4_luma_pred_dc);
    }

}
