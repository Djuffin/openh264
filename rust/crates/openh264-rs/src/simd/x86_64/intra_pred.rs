//! x86_64 SSE2 Intra Prediction Kernels (Phase 4).
//!
//! Accelerated implementations for 16x16 luma, 8x8 chroma, and 4x4 luma intra predictors,
//! serving both the encoder candidate generator and the decoder in-place reconstructor.
//!
//! **`_sse2` in a name here means the body contains intrinsics**, directly or through
//! one of the shared halves below; the predictors that vectorize nothing carry no
//! suffix. Those are not redundant — they are word-wide rewrites of their scalar twins
//! and the init tables install them deliberately — but a name that says `_sse2` sends a
//! reader looking for a kernel that is not there.
//!
//! `sse2_suffix_means_the_body_has_intrinsics` enforces the rule; it is easy to break
//! by accident, since swapping an `_mm_storeu_si128` for a `copy_from_slice` empties a
//! kernel without touching its name.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursorMut, RefSamples};

// ============================================================================
// 16x16 Luma Intra Prediction (SSE2)
// ============================================================================

// ============================================================================
// The enc/dec pairs, once
// ============================================================================

/// **Where a predictor's rows go — the only thing that differed between the pairs.**
///
/// The `enc_`/`dec_` pairs share their neighbour reads (`RefSamples::at` and `row_n`,
/// which `RecCursor` and `PlaneCursorMut` both implement) and every line of arithmetic;
/// they diverged only at the store — the encoder fills a packed candidate buffer at a
/// fixed pitch, the decoder writes back through the same cursor it read from. So the
/// bodies are generic over this trait and the entry points are three lines each. The
/// stores are `copy_from_slice` over a fixed-size array, which is the same instruction
/// as `_mm_storeu_si128` and one less `unsafe`.
///
/// The decoder reads neighbours and writes rows through one cursor, so a body cannot
/// hold `&S` and `&mut O` to it at once. Every collapsed body therefore reads and
/// computes from `&S` first, then writes — the order the arithmetic was already in.
trait PredOut {
    /// Writes `row` as row `dy` of the destination.
    fn put<const N: usize>(&mut self, dy: usize, row: &[u8; N]);
}

/// The encoder's packed candidate buffer — `W` bytes per row, `W * W` long, the
/// `sMemPredMb` arena shape.
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

/// Fills `rows` rows of `N` bytes with one value — V, H, DC and the DC variants all
/// reduce to this once their mean or their source row is known.
#[inline(always)]
fn fill_rows<const N: usize, O: PredOut>(out: &mut O, rows: usize, row: &[u8; N]) {
    for dy in 0..rows {
        out.put(dy, row);
    }
}

/// `(lt_shift, top_shift, left_shift)` for the 16x16 plane predictor.
///
/// C++: `WelsI16x16LumaPredPlane_c`, `codec/common/src/intra_pred_common.cpp`.
#[inline(always)]
fn i16x16_plane_coeffs<S: RefSamples>(src: &S) -> (i32, i32, i32) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..8isize {
        top_sum += (i as i32 + 1) * (src.at(8 + i, -1) as i32 - src.at(6 - i, -1) as i32);
        left_sum += (i as i32 + 1) * (src.at(-1, 8 + i) as i32 - src.at(-1, 6 - i) as i32);
    }
    let lt_shift = (src.at(-1, 15) as i32 + src.at(15, -1) as i32) << 4;
    ((5 * top_sum + 32) >> 6, (5 * left_sum + 32) >> 6, lt_shift)
}

/// The 16x16 plane fill, from the three coefficients.
#[target_feature(enable = "sse2")]
unsafe fn i16x16_plane_fill_sse2<O: PredOut>(
    out: &mut O,
    top_shift: i32,
    left_shift: i32,
    lt_shift: i32,
) {
    let inc_minus = _mm_setr_epi16(-7, -6, -5, -4, -3, -2, -1, 0);
    let inc = _mm_setr_epi16(1, 2, 3, 4, 5, 6, 7, 8);
    let b_vec = _mm_set1_epi16(top_shift as i16);
    let c_vec = _mm_set1_epi16(left_shift as i16);
    let mut s_vec = _mm_set1_epi16((lt_shift + 16 - 7 * left_shift) as i16);

    let term_lo = _mm_mullo_epi16(b_vec, inc_minus);
    let term_hi = _mm_mullo_epi16(b_vec, inc);

    for dy in 0..16 {
        let row_lo = _mm_srai_epi16(_mm_add_epi16(term_lo, s_vec), 5);
        let row_hi = _mm_srai_epi16(_mm_add_epi16(term_hi, s_vec), 5);
        let mut row = [0u8; 16];
        _mm_storeu_si128(row.as_mut_ptr() as *mut __m128i, _mm_packus_epi16(row_lo, row_hi));
        out.put(dy, &row);
        s_vec = _mm_add_epi16(s_vec, c_vec);
    }
}

/// The 16x16 DC mean over whichever of the two neighbour edges the variant uses.
///
/// C++: `WelsI16x16LumaPredDc_c` and its `_T`/`_NA` siblings — one body, because the
/// three differ only in which sums are in scope and what they are rounded by.
#[target_feature(enable = "sse2")]
unsafe fn i16x16_dc_mean_sse2<S: RefSamples>(src: &S, use_top: bool, use_left: bool) -> u8 {
    unsafe {
        let sum_top = if use_top {
            let top = src.row_n::<16>(-1, 0);
            let sad = _mm_sad_epu8(
                _mm_loadu_si128(top.as_ptr() as *const __m128i),
                _mm_setzero_si128(),
            );
            _mm_cvtsi128_si32(sad) + _mm_extract_epi16(sad, 4)
        } else {
            0
        };
        let sum_left = if use_left {
            (0..16).map(|y| src.at(-1, y as isize) as i32).sum()
        } else {
            0
        };
        match (use_top, use_left) {
            (true, true) => ((16 + sum_top + sum_left) >> 5) as u8,
            (true, false) => ((8 + sum_top) >> 4) as u8,
            (false, true) => ((8 + sum_left) >> 4) as u8,
            (false, false) => 0x80,
        }
    }
}

/// The four 4x4-quadrant means of the 8x8 chroma DC predictor, as the two row values
/// the top and bottom halves are filled with.
///
/// C++: `WelsIChromaPredDc_c`.
#[inline(always)]
fn chroma_dc_rows<S: RefSamples>(src: &S) -> ([u8; 8], [u8; 8]) {
    let top = src.row_n::<8>(-1, 0);
    let sum_top0: i32 = (0..4).map(|i| top[i] as i32).sum();
    let sum_top1: i32 = (0..4).map(|i| top[i + 4] as i32).sum();
    let sum_left0: i32 = (0..4).map(|y| src.at(-1, y as isize) as i32).sum();
    let sum_left1: i32 = (0..4).map(|y| src.at(-1, (y + 4) as isize) as i32).sum();

    let mean1 = ((sum_top0 + sum_left0 + 4) >> 3) as u8;
    let mean2 = ((sum_top1 + 2) >> 2) as u8;
    let mean3 = ((sum_left1 + 2) >> 2) as u8;
    let mean4 = ((sum_top1 + sum_left1 + 4) >> 3) as u8;

    (
        [mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2],
        [mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4],
    )
}

/// `(top_shift, left_shift, lt_shift)` for the 8x8 chroma plane predictor.
///
/// C++: `WelsIChromaPredPlane_c`.
#[inline(always)]
fn chroma_plane_coeffs<S: RefSamples>(src: &S) -> (i32, i32, i32) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..4isize {
        top_sum += (i as i32 + 1) * (src.at(4 + i, -1) as i32 - src.at(2 - i, -1) as i32);
        left_sum += (i as i32 + 1) * (src.at(-1, 4 + i) as i32 - src.at(-1, 2 - i) as i32);
    }
    let lt_shift = (src.at(-1, 7) as i32 + src.at(7, -1) as i32) << 4;
    ((17 * top_sum + 16) >> 5, (17 * left_sum + 16) >> 5, lt_shift)
}

/// The 8x8 chroma plane fill, from the three coefficients.
#[target_feature(enable = "sse2")]
unsafe fn chroma_plane_fill_sse2<O: PredOut>(
    out: &mut O,
    top_shift: i32,
    left_shift: i32,
    lt_shift: i32,
) {
    let mul_b = _mm_setr_epi16(-3, -2, -1, 0, 1, 2, 3, 4);
    let b_vec = _mm_set1_epi16(top_shift as i16);
    let c_vec = _mm_set1_epi16(left_shift as i16);
    let mut s_vec = _mm_set1_epi16((lt_shift + 16 - 3 * left_shift) as i16);
    let term = _mm_mullo_epi16(b_vec, mul_b);

    for dy in 0..8 {
        let row_w = _mm_srai_epi16(_mm_add_epi16(term, s_vec), 5);
        let row_b = _mm_packus_epi16(row_w, row_w);
        out.put(dy, &(_mm_cvtsi128_si64(row_b) as u64).to_ne_bytes());
        s_vec = _mm_add_epi16(s_vec, c_vec);
    }
}

/// Fills every row with the neighbour row above — the V predictors, at either width.
#[inline(always)]
fn pred_v<const N: usize, S: RefSamples, O: PredOut>(src: &S, out: &mut O, rows: usize) {
    let top = src.row_n::<N>(-1, 0);
    fill_rows(out, rows, &top);
}

/// Fills each row with its own left neighbour — the H predictors, at either width.
#[inline(always)]
fn pred_h<const N: usize, S: RefSamples, O: PredOut>(src: &S, out: &mut O, rows: usize) {
    for dy in 0..rows {
        out.put(dy, &[src.at(-1, dy as isize); N]);
    }
}

/// Vertical 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_v(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    pred_v::<16, _, _>(rec, &mut Packed::<16>(pred), 16)
}


/// Vertical 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<16>(-1, 0);
    fill_rows(pred, 16, &top)
}

/// Horizontal 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_h(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    pred_h::<16, _, _>(rec, &mut Packed::<16>(pred), 16)
}

/// Horizontal 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..16 {
        let v = pred.at(-1, dy as isize);
        pred.put(dy, &[v; 16]);
    }
}

/// DC 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_dc_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    let mean = unsafe { i16x16_dc_mean_sse2(rec, true, true) };
    fill_rows(&mut Packed::<16>(pred), 16, &[mean; 16])
}

/// DC 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_sse2(pred: &mut PlaneCursorMut<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    let mean = unsafe { i16x16_dc_mean_sse2(pred, true, true) };
    fill_rows(pred, 16, &[mean; 16])
}

/// DC Top 16x16 predictor (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_top_sse2(pred: &mut PlaneCursorMut<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    let mean = unsafe { i16x16_dc_mean_sse2(pred, true, false) };
    fill_rows(pred, 16, &[mean; 16])
}

/// DC NA 16x16 predictor (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_na(pred: &mut PlaneCursorMut<'_>) {
    fill_rows(pred, 16, &[0x80u8; 16])
}

/// Plane 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_plane_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let (top_shift, left_shift, lt_shift) = i16x16_plane_coeffs(rec);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe { i16x16_plane_fill_sse2(&mut Packed::<16>(pred), top_shift, left_shift, lt_shift) }
}


/// Plane 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let (top_shift, left_shift, lt_shift) = i16x16_plane_coeffs(pred);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe { i16x16_plane_fill_sse2(pred, top_shift, left_shift, lt_shift) }
}

// ============================================================================
// 8x8 Chroma Intra Prediction (SSE2)
// ============================================================================

/// Vertical Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_v(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    pred_v::<8, _, _>(rec, &mut Packed::<8>(pred), 8)
}

/// Vertical Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top = pred.row_n::<8>(-1, 0);
    fill_rows(pred, 8, &top)
}

/// Horizontal Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_h(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    pred_h::<8, _, _>(rec, &mut Packed::<8>(pred), 8)
}

/// Horizontal Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        let v = pred.at(-1, dy as isize);
        pred.put(dy, &[v; 8]);
    }
}

/// DC Chroma 8x8 predictor for packed candidate buffer (encoder).
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

/// DC Chroma 8x8 predictor in place (decoder).
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

/// Plane Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_plane_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let (top_shift, left_shift, lt_shift) = chroma_plane_coeffs(rec);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe { chroma_plane_fill_sse2(&mut Packed::<8>(pred), top_shift, left_shift, lt_shift) }
}


/// Plane Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let (top_shift, left_shift, lt_shift) = chroma_plane_coeffs(pred);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe { chroma_plane_fill_sse2(pred, top_shift, left_shift, lt_shift) }
}

// ============================================================================
// 4x4 Luma Intra Prediction (SSE2)
// ============================================================================

/// Vertical 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_v_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<4>(-1, 0);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let t_u32 = i32::from_ne_bytes(top);
        let v = _mm_set1_epi32(t_u32);
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Vertical 4x4 predictor in place (decoder).
#[inline]
pub fn dec_i4x4_luma_pred_v(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 4] = pred.row(-1, 0, 4).try_into().unwrap();
    let t_u32 = u32::from_ne_bytes(top);
    for dy in 0..4 {
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = t_u32.to_ne_bytes();
    }
}

/// Horizontal 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_h_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let l0 = rec.at(-1, 0) as i8;
    let l1 = rec.at(-1, 1) as i8;
    let l2 = rec.at(-1, 2) as i8;
    let l3 = rec.at(-1, 3) as i8;
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(l0, l0, l0, l0, l1, l1, l1, l1, l2, l2, l2, l2, l3, l3, l3, l3);
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Horizontal 4x4 predictor in place (decoder).
#[inline]
pub fn dec_i4x4_luma_pred_h(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..4 {
        let val = pred.at(-1, dy);
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = [val; 4];
    }
}

/// DC 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_dc_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<4>(-1, 0);
    let mut sum = 4i32;
    for y in 0..4 {
        sum += rec.at(-1, y as isize) as i32;
        sum += top[y] as i32;
    }
    let mean = (sum >> 3) as u8;
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_set1_epi8(mean as i8);
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// DC 4x4 predictor in place (decoder).
#[inline]
pub fn dec_i4x4_luma_pred_dc(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 4] = pred.row(-1, 0, 4).try_into().unwrap();
    let mut sum = 4i32;
    for y in 0..4 {
        sum += pred.at(-1, y as isize) as i32;
        sum += top[y] as i32;
    }
    let mean = (sum >> 3) as u8;
    for dy in 0..4 {
        let row: &mut [u8; 4] = pred.row_mut(dy, 0, 4).try_into().unwrap();
        *row = [mean; 4];
    }
}

/// Diagonal Down-Left (DDL) 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_ddl_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<8>(-1, 0);
    let t = |i: usize| top[i] as i32;
    let ddl0 = ((2 + t(0) + t(2) + (t(1) << 1)) >> 2) as u8;
    let ddl1 = ((2 + t(1) + t(3) + (t(2) << 1)) >> 2) as u8;
    let ddl2 = ((2 + t(2) + t(4) + (t(3) << 1)) >> 2) as u8;
    let ddl3 = ((2 + t(3) + t(5) + (t(4) << 1)) >> 2) as u8;
    let ddl4 = ((2 + t(4) + t(6) + (t(5) << 1)) >> 2) as u8;
    let ddl5 = ((2 + t(5) + t(7) + (t(6) << 1)) >> 2) as u8;
    let ddl6 = ((2 + t(6) + t(7) + (t(7) << 1)) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            ddl0 as i8, ddl1 as i8, ddl2 as i8, ddl3 as i8,
            ddl1 as i8, ddl2 as i8, ddl3 as i8, ddl4 as i8,
            ddl2 as i8, ddl3 as i8, ddl4 as i8, ddl5 as i8,
            ddl3 as i8, ddl4 as i8, ddl5 as i8, ddl6 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Diagonal Down-Right (DDR) 4x4 predictor for packed candidate buffer (encoder).
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
    let ddr0 = ((tl0 + lt0) >> 2) as u8;
    let ddr1 = ((lt0 + t01) >> 2) as u8;
    let ddr2 = ((t01 + t12) >> 2) as u8;
    let ddr3 = ((t12 + t23) >> 2) as u8;
    let ddr4 = ((tl0 + l01) >> 2) as u8;
    let ddr5 = ((l01 + l12) >> 2) as u8;
    let ddr6 = ((l12 + l23) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            ddr0 as i8, ddr1 as i8, ddr2 as i8, ddr3 as i8,
            ddr4 as i8, ddr0 as i8, ddr1 as i8, ddr2 as i8,
            ddr5 as i8, ddr4 as i8, ddr0 as i8, ddr1 as i8,
            ddr6 as i8, ddr5 as i8, ddr4 as i8, ddr0 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Vertical Right (VR) 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_vr_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let lt = rec.at(-1, -1) as i32;
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let top = rec.row_n::<4>(-1, 0);
    let (t0, t1, t2, t3) = (top[0] as i32, top[1] as i32, top[2] as i32, top[3] as i32);
    let vr0 = ((1 + lt + t0) >> 1) as u8;
    let vr1 = ((1 + t0 + t1) >> 1) as u8;
    let vr2 = ((1 + t1 + t2) >> 1) as u8;
    let vr3 = ((1 + t2 + t3) >> 1) as u8;
    let vr4 = ((2 + l0 + (lt << 1) + t0) >> 2) as u8;
    let vr5 = ((2 + lt + (t0 << 1) + t1) >> 2) as u8;
    let vr6 = ((2 + t0 + (t1 << 1) + t2) >> 2) as u8;
    let vr7 = ((2 + t1 + (t2 << 1) + t3) >> 2) as u8;
    let vr8 = ((2 + lt + (l0 << 1) + l1) >> 2) as u8;
    let vr9 = ((2 + l0 + (l1 << 1) + l2) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            vr0 as i8, vr1 as i8, vr2 as i8, vr3 as i8,
            vr4 as i8, vr5 as i8, vr6 as i8, vr7 as i8,
            vr8 as i8, vr0 as i8, vr1 as i8, vr2 as i8,
            vr9 as i8, vr4 as i8, vr5 as i8, vr6 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Horizontal Down (HD) 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_hd_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let lt = rec.at(-1, -1) as i32;
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let l3 = rec.at(-1, 3) as i32;
    let top = rec.row_n::<4>(-1, 0);
    let (t0, t1, t2) = (top[0] as i32, top[1] as i32, top[2] as i32);
    let hd0 = ((1 + lt + l0) >> 1) as u8;
    let hd1 = ((2 + lt + (l0 << 1) + l1) >> 2) as u8;
    let hd2 = ((1 + l0 + l1) >> 1) as u8;
    let hd3 = ((2 + l0 + (l1 << 1) + l2) >> 2) as u8;
    let hd4 = ((1 + l1 + l2) >> 1) as u8;
    let hd5 = ((2 + l1 + (l2 << 1) + l3) >> 2) as u8;
    let hd6 = ((1 + l2 + l3) >> 1) as u8;
    let hd7 = ((2 + l0 + (lt << 1) + t0) >> 2) as u8;
    let hd8 = ((2 + lt + (t0 << 1) + t1) >> 2) as u8;
    let hd9 = ((2 + t0 + (t1 << 1) + t2) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            hd0 as i8, hd7 as i8, hd8 as i8, hd9 as i8,
            hd2 as i8, hd1 as i8, hd0 as i8, hd7 as i8,
            hd4 as i8, hd3 as i8, hd2 as i8, hd1 as i8,
            hd6 as i8, hd5 as i8, hd4 as i8, hd3 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Vertical Left (VL) 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_vl_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let top = rec.row_n::<7>(-1, 0);
    let t = |i: usize| top[i] as i32;
    let vl0 = ((1 + t(0) + t(1)) >> 1) as u8;
    let vl1 = ((1 + t(1) + t(2)) >> 1) as u8;
    let vl2 = ((1 + t(2) + t(3)) >> 1) as u8;
    let vl3 = ((1 + t(3) + t(4)) >> 1) as u8;
    let vl4 = ((1 + t(4) + t(5)) >> 1) as u8;
    let vl5 = ((2 + t(0) + (t(1) << 1) + t(2)) >> 2) as u8;
    let vl6 = ((2 + t(1) + (t(2) << 1) + t(3)) >> 2) as u8;
    let vl7 = ((2 + t(2) + (t(3) << 1) + t(4)) >> 2) as u8;
    let vl8 = ((2 + t(3) + (t(4) << 1) + t(5)) >> 2) as u8;
    let vl9 = ((2 + t(4) + (t(5) << 1) + t(6)) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            vl0 as i8, vl1 as i8, vl2 as i8, vl3 as i8,
            vl5 as i8, vl6 as i8, vl7 as i8, vl8 as i8,
            vl1 as i8, vl2 as i8, vl3 as i8, vl4 as i8,
            vl6 as i8, vl7 as i8, vl8 as i8, vl9 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Horizontal Up (HU) 4x4 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i4x4_luma_pred_hu_sse2(pred: &mut [u8; 16], rec: &RecCursor<'_>) {
    let l0 = rec.at(-1, 0) as i32;
    let l1 = rec.at(-1, 1) as i32;
    let l2 = rec.at(-1, 2) as i32;
    let l3 = rec.at(-1, 3) as i32;
    let l01 = 1 + l0 + l1;
    let l12 = 1 + l1 + l2;
    let l23 = 1 + l2 + l3;
    let hu0 = (l01 >> 1) as u8;
    let hu1 = ((l01 + l12) >> 2) as u8;
    let hu2 = (l12 >> 1) as u8;
    let hu3 = ((l12 + l23) >> 2) as u8;
    let hu4 = (l23 >> 1) as u8;
    let hu5 = ((1 + l23 + (l3 << 1)) >> 2) as u8;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_setr_epi8(
            hu0 as i8, hu1 as i8, hu2 as i8, hu3 as i8,
            hu2 as i8, hu3 as i8, hu4 as i8, hu5 as i8,
            hu4 as i8, hu5 as i8, l3 as i8, l3 as i8,
            l3 as i8, l3 as i8, l3 as i8, l3 as i8,
        );
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
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

    /// **The `_sse2` naming rule, enforced rather than asserted in a comment.**
    ///
    /// The module header states it; this decides it. It reads this file's own source and
    /// checks that every `pub fn` whose name ends in `_sse2` reaches at least one `_mm_*`
    /// intrinsic — in its own body, or in a function it calls (one hop, which is how the
    /// wrappers here delegate to their `_impl` and to the shared fills).
    ///
    /// Why a test and not a review habit: the rule was written into the header and broken
    /// six times by the same commit that wrote it. The `PredOut` collapse replaced
    /// `_mm_storeu_si128` with `copy_from_slice`, which is the same instruction and one
    /// less `unsafe`, but it silently emptied `enc_i16x16_luma_pred_v_sse2`,
    /// `dec_i16x16_luma_pred_v_sse2`, the `_h` pair, `dec_i16x16_luma_pred_dc_na_sse2`
    /// and `enc_chroma_pred_v_sse2` of every intrinsic while leaving the suffix on. A
    /// reader looking for a kernel would not have found one.
    #[test]
    fn sse2_suffix_means_the_body_has_intrinsics() {
        let src = include_str!("intra_pred.rs");

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
                    // Only once an opening brace has been seen: a `fn ` match inside a
                    // comment can otherwise reach a closing brace first and underflow.
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

        // Every `fn` in the file, by name, so a one-hop call can be resolved.
        let mut bodies: Vec<(String, &str)> = Vec::new();
        for (off, _) in src.match_indices("fn ") {
            let after = &src[off + 3..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                bodies.push((name, body_at(src, off)));
            }
        }
        let intrinsics = |b: &str| b.contains("_mm_");

        let mut offenders = Vec::new();
        for (name, body) in bodies.iter().filter(|(n, _)| n.ends_with("_sse2")) {
            if intrinsics(body) {
                continue;
            }
            // One hop: any function this body *names as a whole identifier* that reaches
            // an intrinsic. Substring matching is not enough — `..._dc_na_sse2` contains
            // `..._dc`, so a plain `contains` lets an empty kernel borrow a sibling's
            // intrinsics and the check passes on a body that has none.
            let called: std::collections::HashSet<&str> = body
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|t| !t.is_empty())
                .collect();
            let reaches = bodies.iter().any(|(callee, cbody)| {
                callee != name && called.contains(callee.as_str()) && intrinsics(cbody)
            });
            if !reaches {
                offenders.push(name.clone());
            }
        }

        assert!(
            offenders.is_empty(),
            "these are named `_sse2` but reach no `_mm_*` intrinsic — drop the suffix or \
             implement the kernel (see the module header): {offenders:?}"
        );
    }
}
