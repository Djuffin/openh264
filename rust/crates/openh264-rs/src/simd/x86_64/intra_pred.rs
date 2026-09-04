//! x86_64 SSE2 Intra Prediction Kernels (Phase 4).
//!
//! Accelerated implementations for 16x16 luma, 8x8 chroma, and 4x4 luma intra predictors,
//! serving both the encoder candidate generator and the decoder in-place reconstructor.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursorMut, RefSamples};

// ============================================================================
// 16x16 Luma Intra Prediction (SSE2)
// ============================================================================

/// Vertical 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_v_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let top = rec.row_n::<16>(-1, 0);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        enc_i16x16_luma_pred_v_sse2_impl(pred, &top);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn enc_i16x16_luma_pred_v_sse2_impl(pred: &mut [u8; 256], top: &[u8; 16]) {
    let v = _mm_loadu_si128(top.as_ptr() as *const __m128i);
    for y in 0..16 {
        _mm_storeu_si128(pred.as_mut_ptr().add(y * 16) as *mut __m128i, v);
    }
}

/// Vertical 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_v_sse2(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_loadu_si128(top.as_ptr() as *const __m128i);
        for dy in 0..16 {
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, v);
        }
    }
}

/// Horizontal 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_h_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        for y in 0..16 {
            let val = rec.at(-1, y as isize);
            let v = _mm_set1_epi8(val as i8);
            _mm_storeu_si128(pred.as_mut_ptr().add(y * 16) as *mut __m128i, v);
        }
    }
}

/// Horizontal 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_h_sse2(pred: &mut PlaneCursorMut<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        for dy in 0..16 {
            let val = pred.at(-1, dy);
            let v = _mm_set1_epi8(val as i8);
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, v);
        }
    }
}

/// DC 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_dc_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let top = rec.row_n::<16>(-1, 0);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let top_v = _mm_loadu_si128(top.as_ptr() as *const __m128i);
        let sad = _mm_sad_epu8(top_v, _mm_setzero_si128());
        let sum_top = _mm_cvtsi128_si32(sad) + _mm_extract_epi16(sad, 4);

        let mut sum_left = 0i32;
        for y in 0..16 {
            sum_left += rec.at(-1, y as isize) as i32;
        }

        let mean = ((16 + sum_top + sum_left) >> 5) as u8;
        let v = _mm_set1_epi8(mean as i8);
        for y in 0..16 {
            _mm_storeu_si128(pred.as_mut_ptr().add(y * 16) as *mut __m128i, v);
        }
    }
}

/// DC 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_sse2(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let top_v = _mm_loadu_si128(top.as_ptr() as *const __m128i);
        let sad = _mm_sad_epu8(top_v, _mm_setzero_si128());
        let sum_top = _mm_cvtsi128_si32(sad) + _mm_extract_epi16(sad, 4);

        let mut sum_left = 0i32;
        for y in 0..16 {
            sum_left += pred.at(-1, y as isize) as i32;
        }

        let mean = ((16 + sum_top + sum_left) >> 5) as u8;
        let v = _mm_set1_epi8(mean as i8);
        for dy in 0..16 {
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, v);
        }
    }
}

/// DC Top 16x16 predictor (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_top_sse2(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 16] = pred.row(-1, 0, 16).try_into().unwrap();
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let top_v = _mm_loadu_si128(top.as_ptr() as *const __m128i);
        let sad = _mm_sad_epu8(top_v, _mm_setzero_si128());
        let sum_top = _mm_cvtsi128_si32(sad) + _mm_extract_epi16(sad, 4);
        let mean = ((8 + sum_top) >> 4) as u8;
        let v = _mm_set1_epi8(mean as i8);
        for dy in 0..16 {
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, v);
        }
    }
}

/// DC NA 16x16 predictor (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_dc_na_sse2(pred: &mut PlaneCursorMut<'_>) {
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let v = _mm_set1_epi8(0x80u8 as i8);
        for dy in 0..16 {
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, v);
        }
    }
}

/// Plane 16x16 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_i16x16_luma_pred_plane_sse2(pred: &mut [u8; 256], rec: &RecCursor<'_>) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..8isize {
        top_sum += (i as i32 + 1)
            * (rec.at(8 + i, -1) as i32 - rec.at(6 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (rec.at(-1, 8 + i) as i32 - rec.at(-1, 6 - i) as i32);
    }

    let lt_shift = (rec.at(-1, 15) as i32 + rec.at(15, -1) as i32) << 4;
    let top_shift = (5 * top_sum + 32) >> 6;
    let left_shift = (5 * left_sum + 32) >> 6;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        enc_i16x16_luma_pred_plane_sse2_impl(pred, lt_shift, top_shift, left_shift);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn enc_i16x16_luma_pred_plane_sse2_impl(
    pred: &mut [u8; 256],
    lt_shift: i32,
    top_shift: i32,
    left_shift: i32,
) {
    let inc_minus = _mm_setr_epi16(-7, -6, -5, -4, -3, -2, -1, 0);
    let inc = _mm_setr_epi16(1, 2, 3, 4, 5, 6, 7, 8);
    let b_vec = _mm_set1_epi16(top_shift as i16);
    let c_vec = _mm_set1_epi16(left_shift as i16);
    let s_init = lt_shift + 16 - 7 * left_shift;
    let mut s_vec = _mm_set1_epi16(s_init as i16);

    let term_lo = _mm_mullo_epi16(b_vec, inc_minus);
    let term_hi = _mm_mullo_epi16(b_vec, inc);

    for y in 0..16 {
        let row_lo = _mm_srai_epi16(_mm_add_epi16(term_lo, s_vec), 5);
        let row_hi = _mm_srai_epi16(_mm_add_epi16(term_hi, s_vec), 5);
        let row = _mm_packus_epi16(row_lo, row_hi);
        _mm_storeu_si128(pred.as_mut_ptr().add(y * 16) as *mut __m128i, row);
        s_vec = _mm_add_epi16(s_vec, c_vec);
    }
}

/// Plane 16x16 predictor in place (decoder).
#[inline]
pub fn dec_i16x16_luma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..8isize {
        top_sum += (i as i32 + 1)
            * (pred.at(8 + i, -1) as i32 - pred.at(6 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (pred.at(-1, 8 + i) as i32 - pred.at(-1, 6 - i) as i32);
    }

    let lt_shift = (pred.at(-1, 15) as i32 + pred.at(15, -1) as i32) << 4;
    let top_shift = (5 * top_sum + 32) >> 6;
    let left_shift = (5 * left_sum + 32) >> 6;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let inc_minus = _mm_setr_epi16(-7, -6, -5, -4, -3, -2, -1, 0);
        let inc = _mm_setr_epi16(1, 2, 3, 4, 5, 6, 7, 8);
        let b_vec = _mm_set1_epi16(top_shift as i16);
        let c_vec = _mm_set1_epi16(left_shift as i16);
        let s_init = lt_shift + 16 - 7 * left_shift;
        let mut s_vec = _mm_set1_epi16(s_init as i16);

        let term_lo = _mm_mullo_epi16(b_vec, inc_minus);
        let term_hi = _mm_mullo_epi16(b_vec, inc);

        for dy in 0..16 {
            let row_lo = _mm_srai_epi16(_mm_add_epi16(term_lo, s_vec), 5);
            let row_hi = _mm_srai_epi16(_mm_add_epi16(term_hi, s_vec), 5);
            let row = _mm_packus_epi16(row_lo, row_hi);
            _mm_storeu_si128(pred.row_mut(dy, 0, 16).as_mut_ptr() as *mut __m128i, row);
            s_vec = _mm_add_epi16(s_vec, c_vec);
        }
    }
}

// ============================================================================
// 8x8 Chroma Intra Prediction (SSE2)
// ============================================================================

/// Vertical Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_v_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let top = rec.row_n::<8>(-1, 0);
    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let top_u64 = *(top.as_ptr() as *const i64);
        let v = _mm_set1_epi64x(top_u64);
        for y in 0..4 {
            _mm_storeu_si128(pred.as_mut_ptr().add(y * 16) as *mut __m128i, v);
        }
    }
}

/// Vertical Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_v_sse2(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let top_u64 = u64::from_ne_bytes(top);
    for dy in 0..8 {
        let row: &mut [u8; 8] = pred.row_mut(dy, 0, 8).try_into().unwrap();
        *row = top_u64.to_ne_bytes();
    }
}

/// Horizontal Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_h_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    for y in 0..8 {
        let val = rec.at(-1, y as isize);
        let v = val as u64 * 0x0101_0101_0101_0101u64;
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = v.to_ne_bytes();
    }
}

/// Horizontal Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_h_sse2(pred: &mut PlaneCursorMut<'_>) {
    for dy in 0..8 {
        let val = pred.at(-1, dy);
        let v = val as u64 * 0x0101_0101_0101_0101u64;
        let row: &mut [u8; 8] = pred.row_mut(dy, 0, 8).try_into().unwrap();
        *row = v.to_ne_bytes();
    }
}

/// DC Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_dc_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let top = rec.row_n::<8>(-1, 0);
    let mut sum_top0 = 0i32;
    let mut sum_top1 = 0i32;
    for i in 0..4 {
        sum_top0 += top[i] as i32;
        sum_top1 += top[i + 4] as i32;
    }
    let mut sum_left0 = 0i32;
    let mut sum_left1 = 0i32;
    for y in 0..4 {
        sum_left0 += rec.at(-1, y as isize) as i32;
        sum_left1 += rec.at(-1, (y + 4) as isize) as i32;
    }

    let mean1 = ((sum_top0 + sum_left0 + 4) >> 3) as u8;
    let mean2 = ((sum_top1 + 2) >> 2) as u8;
    let mean3 = ((sum_left1 + 2) >> 2) as u8;
    let mean4 = ((sum_top1 + sum_left1 + 4) >> 3) as u8;

    let top_mean = u64::from_ne_bytes([mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2]);
    let bot_mean = u64::from_ne_bytes([mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4]);

    for y in 0..4 {
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = top_mean.to_ne_bytes();
    }
    for y in 4..8 {
        let row: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *row = bot_mean.to_ne_bytes();
    }
}

/// DC Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_dc_sse2(pred: &mut PlaneCursorMut<'_>) {
    let top: [u8; 8] = pred.row(-1, 0, 8).try_into().unwrap();
    let mut sum_top0 = 0i32;
    let mut sum_top1 = 0i32;
    for i in 0..4 {
        sum_top0 += top[i] as i32;
        sum_top1 += top[i + 4] as i32;
    }
    let mut sum_left0 = 0i32;
    let mut sum_left1 = 0i32;
    for y in 0..4 {
        sum_left0 += pred.at(-1, y as isize) as i32;
        sum_left1 += pred.at(-1, (y + 4) as isize) as i32;
    }

    let mean1 = ((sum_top0 + sum_left0 + 4) >> 3) as u8;
    let mean2 = ((sum_top1 + 2) >> 2) as u8;
    let mean3 = ((sum_left1 + 2) >> 2) as u8;
    let mean4 = ((sum_top1 + sum_left1 + 4) >> 3) as u8;

    let top_mean = u64::from_ne_bytes([mean1, mean1, mean1, mean1, mean2, mean2, mean2, mean2]);
    let bot_mean = u64::from_ne_bytes([mean3, mean3, mean3, mean3, mean4, mean4, mean4, mean4]);

    for dy in 0..4 {
        let row: &mut [u8; 8] = pred.row_mut(dy, 0, 8).try_into().unwrap();
        *row = top_mean.to_ne_bytes();
    }
    for dy in 4..8 {
        let row: &mut [u8; 8] = pred.row_mut(dy, 0, 8).try_into().unwrap();
        *row = bot_mean.to_ne_bytes();
    }
}

/// Plane Chroma 8x8 predictor for packed candidate buffer (encoder).
#[inline]
pub fn enc_chroma_pred_plane_sse2(pred: &mut [u8; 64], rec: &RecCursor<'_>) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..4isize {
        top_sum += (i as i32 + 1)
            * (rec.at(4 + i, -1) as i32 - rec.at(2 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (rec.at(-1, 4 + i) as i32 - rec.at(-1, 2 - i) as i32);
    }

    let lt_shift = (rec.at(-1, 7) as i32 + rec.at(7, -1) as i32) << 4;
    let top_shift = (17 * top_sum + 16) >> 5;
    let left_shift = (17 * left_sum + 16) >> 5;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        enc_chroma_pred_plane_sse2_impl(pred, lt_shift, top_shift, left_shift);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn enc_chroma_pred_plane_sse2_impl(
    pred: &mut [u8; 64],
    lt_shift: i32,
    top_shift: i32,
    left_shift: i32,
) {
    let mul_b = _mm_setr_epi16(-3, -2, -1, 0, 1, 2, 3, 4);
    let b_vec = _mm_set1_epi16(top_shift as i16);
    let c_vec = _mm_set1_epi16(left_shift as i16);
    let s_init = lt_shift + 16 - 3 * left_shift;
    let mut s_vec = _mm_set1_epi16(s_init as i16);
    let term = _mm_mullo_epi16(b_vec, mul_b);

    for y in 0..8 {
        let row_w = _mm_srai_epi16(_mm_add_epi16(term, s_vec), 5);
        let row_b = _mm_packus_epi16(row_w, row_w);
        let row_u64 = _mm_cvtsi128_si64(row_b) as u64;
        let dst: &mut [u8; 8] = (&mut pred[y * 8..][..8]).try_into().unwrap();
        *dst = row_u64.to_ne_bytes();
        s_vec = _mm_add_epi16(s_vec, c_vec);
    }
}

/// Plane Chroma 8x8 predictor in place (decoder).
#[inline]
pub fn dec_chroma_pred_plane_sse2(pred: &mut PlaneCursorMut<'_>) {
    let mut top_sum: i32 = 0;
    let mut left_sum: i32 = 0;
    for i in 0..4isize {
        top_sum += (i as i32 + 1)
            * (pred.at(4 + i, -1) as i32 - pred.at(2 - i, -1) as i32);
        left_sum += (i as i32 + 1)
            * (pred.at(-1, 4 + i) as i32 - pred.at(-1, 2 - i) as i32);
    }

    let lt_shift = (pred.at(-1, 7) as i32 + pred.at(7, -1) as i32) << 4;
    let top_shift = (17 * top_sum + 16) >> 5;
    let left_shift = (17 * left_sum + 16) >> 5;

    // unsafe-cat: simd-kernel(x86_64)
    unsafe {
        let mul_b = _mm_setr_epi16(-3, -2, -1, 0, 1, 2, 3, 4);
        let b_vec = _mm_set1_epi16(top_shift as i16);
        let c_vec = _mm_set1_epi16(left_shift as i16);
        let s_init = lt_shift + 16 - 3 * left_shift;
        let mut s_vec = _mm_set1_epi16(s_init as i16);
        let term = _mm_mullo_epi16(b_vec, mul_b);

        for dy in 0..8 {
            let row_w = _mm_srai_epi16(_mm_add_epi16(term, s_vec), 5);
            let row_b = _mm_packus_epi16(row_w, row_w);
            let row_u64 = _mm_cvtsi128_si64(row_b) as u64;
            let dst: &mut [u8; 8] = pred.row_mut(dy, 0, 8).try_into().unwrap();
            *dst = row_u64.to_ne_bytes();
            s_vec = _mm_add_epi16(s_vec, c_vec);
        }
    }
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
        let t_u32 = *(top.as_ptr() as *const i32);
        let v = _mm_set1_epi32(t_u32);
        _mm_storeu_si128(pred.as_mut_ptr() as *mut __m128i, v);
    }
}

/// Vertical 4x4 predictor in place (decoder).
#[inline]
pub fn dec_i4x4_luma_pred_v_sse2(pred: &mut PlaneCursorMut<'_>) {
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
pub fn dec_i4x4_luma_pred_h_sse2(pred: &mut PlaneCursorMut<'_>) {
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
pub fn dec_i4x4_luma_pred_dc_sse2(pred: &mut PlaneCursorMut<'_>) {
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
        enc_i16x16_luma_pred_v_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "16x16 V mismatch");

        // H
        let mut pred_c = [0u8; 256];
        let mut pred_simd = [0u8; 256];
        WelsI16x16LumaPredH_c(&mut pred_c, &rec);
        enc_i16x16_luma_pred_h_sse2(&mut pred_simd, &rec);
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
        enc_chroma_pred_v_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma V mismatch");

        // H
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredH_c(&mut pred_c, &rec);
        enc_chroma_pred_h_sse2(&mut pred_simd, &rec);
        assert_eq!(pred_c, pred_simd, "Chroma H mismatch");

        // DC
        let mut pred_c = [0u8; 64];
        let mut pred_simd = [0u8; 64];
        WelsIChromaPredDc_c(&mut pred_c, &rec);
        enc_chroma_pred_dc_sse2(&mut pred_simd, &rec);
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
}
