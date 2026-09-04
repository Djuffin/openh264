//! SSE2 implementations of Forward 4x4 DCT and Inverse DCT (IDCT) with Prediction Addition.
#![allow(unsafe_code)]

use core::arch::x86_64::*;
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples, SampleCursor};

// ============================================================================
// Forward 4x4 Integer DCT
// ============================================================================

/// Forward 1D DCT on 4 horizontal samples in low 64 bits of `__m128i`.
#[target_feature(enable = "sse2")]
unsafe fn dct_row_sse2(d: __m128i) -> __m128i {
    let d_rev = _mm_shufflelo_epi16(d, 0b00_01_10_11); // [d3, d2, d1, d0]
    let sum = _mm_add_epi16(d, d_rev); // [s0, s1, s1, s0]
    let diff = _mm_sub_epi16(d, d_rev); // [s3, s2, -s2, -s3]

    let s0 = _mm_cvtsi128_si32(sum) as i16 as i32;
    let s1 = (_mm_cvtsi128_si32(sum) >> 16) as i16 as i32;
    let s3 = _mm_cvtsi128_si32(diff) as i16 as i32;
    let s2 = (_mm_cvtsi128_si32(diff) >> 16) as i16 as i32;

    let y0 = (s0 + s1) as i16;
    let y1 = ((s3 << 1) + s2) as i16;
    let y2 = (s0 - s1) as i16;
    let y3 = (s3 - (s2 << 1)) as i16;

    _mm_set_epi16(0, 0, 0, 0, y3, y2, y1, y0)
}

/// 4x4 Forward Integer DCT of the pixel difference `(pix1 - pix2)` using SSE2.
///
/// C++: `WelsDctT4_sse2`, `codec/common/x86/dct.asm`.
#[target_feature(enable = "sse2")]
unsafe fn dct_4x4_sse2_impl<A: SampleCursor, B: SampleCursor>(
    dct: &mut [i16; 16],
    pix1: &A,
    pix2: &B,
) {
    unsafe {
        let zero = _mm_setzero_si128();

        // Load 4 rows of 4 bytes difference
        let r1_0 = pix1.row_n::<4>(0, 0);
        let r2_0 = pix2.row_n::<4>(0, 0);
        let diff0 = _mm_sub_epi16(
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r1_0.as_ptr() as *const i32).read_unaligned()), zero),
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r2_0.as_ptr() as *const i32).read_unaligned()), zero),
        );

        let r1_1 = pix1.row_n::<4>(1, 0);
        let r2_1 = pix2.row_n::<4>(1, 0);
        let diff1 = _mm_sub_epi16(
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r1_1.as_ptr() as *const i32).read_unaligned()), zero),
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r2_1.as_ptr() as *const i32).read_unaligned()), zero),
        );

        let r1_2 = pix1.row_n::<4>(2, 0);
        let r2_2 = pix2.row_n::<4>(2, 0);
        let diff2 = _mm_sub_epi16(
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r1_2.as_ptr() as *const i32).read_unaligned()), zero),
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r2_2.as_ptr() as *const i32).read_unaligned()), zero),
        );

        let r1_3 = pix1.row_n::<4>(3, 0);
        let r2_3 = pix2.row_n::<4>(3, 0);
        let diff3 = _mm_sub_epi16(
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r1_3.as_ptr() as *const i32).read_unaligned()), zero),
            _mm_unpacklo_epi8(_mm_cvtsi32_si128((r2_3.as_ptr() as *const i32).read_unaligned()), zero),
        );

        // Horizontal 1D DCT on each row
        let y0 = dct_row_sse2(diff0);
        let y1 = dct_row_sse2(diff1);
        let y2 = dct_row_sse2(diff2);
        let y3 = dct_row_sse2(diff3);

        // Vertical 1D DCT across all 4 columns simultaneously
        let s0 = _mm_add_epi16(y0, y3);
        let s3 = _mm_sub_epi16(y0, y3);
        let s1 = _mm_add_epi16(y1, y2);
        let s2 = _mm_sub_epi16(y1, y2);

        let out0 = _mm_add_epi16(s0, s1);
        let out1 = _mm_add_epi16(_mm_slli_epi16(s3, 1), s2);
        let out2 = _mm_sub_epi16(s0, s1);
        let out3 = _mm_sub_epi16(s3, _mm_slli_epi16(s2, 1));

        let out01 = _mm_unpacklo_epi64(out0, out1);
        let out23 = _mm_unpacklo_epi64(out2, out3);

        _mm_storeu_si128(dct.as_mut_ptr() as *mut __m128i, out01);
        _mm_storeu_si128(dct.as_mut_ptr().add(8) as *mut __m128i, out23);
    }
}

/// 4x4 Forward Integer DCT of the pixel difference `(pix1 - pix2)` using SSE2.
///
/// C++: `WelsDctT4_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn dct_4x4_sse2<A: SampleCursor, B: SampleCursor>(
    dct: &mut [i16; 16],
    pix1: &A,
    pix2: &B,
) {
    unsafe { dct_4x4_sse2_impl(dct, pix1, pix2) }
}

#[target_feature(enable = "sse2")]
unsafe fn dct_four_4x4_sse2_impl<A: SampleCursor, B: SampleCursor>(
    dct: &mut [i16; 64],
    pix1: &A,
    pix2: &B,
) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    unsafe {
        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &mut [i16; 16] = (&mut dct[k << 4..][..16]).try_into().unwrap();
            dct_4x4_sse2_impl(sub, &pix1.advance(dx, dy), &pix2.advance(dx, dy));
        }
    }
}

/// Performs 4x4 FDCT on four adjacent 4x4 blocks forming an 8x8 quadrant using SSE2.
///
/// C++: `WelsDctFourT4_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn dct_four_4x4_sse2<A: SampleCursor, B: SampleCursor>(
    dct: &mut [i16; 64],
    pix1: &A,
    pix2: &B,
) {
    unsafe { dct_four_4x4_sse2_impl(dct, pix1, pix2) }
}

// ============================================================================
// Inverse 4x4 Integer DCT & Prediction Addition
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn idct_row_sse2(r0: i16, r1: i16, r2: i16, r3: i16) -> __m128i {
    let r0 = r0 as i32;
    let r1 = r1 as i32;
    let r2 = r2 as i32;
    let r3 = r3 as i32;

    let t0 = r0 + r2;
    let t1 = r0 - r2;
    let t2 = (r1 >> 1) - r3;
    let t3 = r1 + (r3 >> 1);

    let s0 = (t0 + t3) as i16;
    let s1 = (t1 + t2) as i16;
    let s2 = (t1 - t2) as i16;
    let s3 = (t0 - t3) as i16;

    _mm_set_epi16(0, 0, 0, 0, s3, s2, s1, s0)
}

#[target_feature(enable = "sse2")]
unsafe fn add_res_and_clip_sse2(pred_4bytes: [u8; 4], res: __m128i) -> [u8; 4] {
    unsafe {
        let p32 = (pred_4bytes.as_ptr() as *const i32).read_unaligned();
        let p_vec = _mm_cvtsi32_si128(p32);
        let p_unp = _mm_unpacklo_epi8(p_vec, _mm_setzero_si128());
        let sum = _mm_add_epi16(p_unp, res);
        let packed = _mm_packus_epi16(sum, sum);
        let res32 = _mm_cvtsi128_si32(packed);
        res32.to_ne_bytes()
    }
}

/// Sign-extends the four live `i16` lanes in the low half of `v` to four `i32` lanes.
///
/// SSE2 has no `pmovsxwd` — that is SSE4.1 — so duplicate each word into a pair and
/// shift the pair down arithmetically: `[a b c d …]` becomes `[a a b b]` and then
/// `[a b c d]` as `i32`, sign carried by the shift.
#[target_feature(enable = "sse2")]
unsafe fn widen_lo_i16_to_i32_sse2(v: __m128i) -> __m128i {
    _mm_srai_epi32(_mm_unpacklo_epi16(v, v), 16)
}

/// Computes the 4x4 IDCT residual vectors for 4 rows using SSE2.
///
/// # The vertical pass runs in `i32`, unlike upstream's asm
///
/// The horizontal pass truncates to `i16` in both this and the scalar — `iSrc` is an
/// `int16_t[16]` in the C++ and the truncation is observable — so `s0..s12` span the
/// full `i16` range and `s0 + s8` overflows 16 bits. In `epi16` lanes that wrapped
/// where the scalar saturates; `idct_vertical_pass_does_not_wrap_at_16_bits` pins the
/// case.
///
/// **This is a deliberate divergence from `SSE2_IDCT_4x4P`**
/// (`codec/common/x86/dct.asm:450`), which is 16-bit and has the same overflow.
/// Upstream is not consistent about it either: `IdctResAddPred_AArch64_neon`
/// (`codec/decoder/core/arm64/block_add_aarch64_neon.S:70`) widens to `.4s` and agrees
/// with its own C. Widening picks the answer that agrees with this port's scalar on
/// every architecture.
#[target_feature(enable = "sse2")]
unsafe fn compute_idct_residuals_sse2(dct: &[i16; 16]) -> (__m128i, __m128i, __m128i, __m128i) {
    let s0 = widen_lo_i16_to_i32_sse2(idct_row_sse2(dct[0], dct[1], dct[2], dct[3]));
    let s4 = widen_lo_i16_to_i32_sse2(idct_row_sse2(dct[4], dct[5], dct[6], dct[7]));
    let s8 = widen_lo_i16_to_i32_sse2(idct_row_sse2(dct[8], dct[9], dct[10], dct[11]));
    let s12 = widen_lo_i16_to_i32_sse2(idct_row_sse2(dct[12], dct[13], dct[14], dct[15]));

    let c32 = _mm_set1_epi32(32);

    let t1_a = _mm_add_epi32(s0, s8);
    let t2_a = _mm_add_epi32(s4, _mm_srai_epi32(s12, 1));
    let res0 = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(t1_a, t2_a), c32), 6);
    let res3 = _mm_srai_epi32(_mm_add_epi32(_mm_sub_epi32(t1_a, t2_a), c32), 6);

    let t1_b = _mm_sub_epi32(s0, s8);
    let t2_b = _mm_sub_epi32(_mm_srai_epi32(s4, 1), s12);
    let res1 = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(t1_b, t2_b), c32), 6);
    let res2 = _mm_srai_epi32(_mm_add_epi32(_mm_sub_epi32(t1_b, t2_b), c32), 6);

    // Back to the `i16` lanes `add_res_and_clip_sse2` adds the prediction in. The
    // signed saturation `packs` applies is unreachable and so exact: `|s*| <= 32768`
    // bounds `|t1| <= 65536` and `|t2| <= 49152`, giving `|32 + t1 ± t2| <= 114720`
    // and `|result| <= 1792` after the `>> 6`.
    let zero = _mm_setzero_si128();
    (
        _mm_packs_epi32(res0, zero),
        _mm_packs_epi32(res1, zero),
        _mm_packs_epi32(res2, zero),
        _mm_packs_epi32(res3, zero),
    )
}

#[target_feature(enable = "sse2")]
unsafe fn idct_res_add_pred_sse2_impl(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    unsafe {
        let (res0, res1, res2, res3) = compute_idct_residuals_sse2(rs);
        for (dy, res) in [res0, res1, res2, res3].into_iter().enumerate() {
            let row: &mut [u8; 4] = pred.row_mut(dy as isize, 0, 4).try_into().unwrap();
            *row = add_res_and_clip_sse2(*row, res);
        }
    }
}

/// 4x4 inverse integer DCT of `rs`, added to `pred` and saturated to `[0, 255]` in place using SSE2.
///
/// C++: `IdctResAddPred_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_res_add_pred_sse2(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16]) {
    unsafe { idct_res_add_pred_sse2_impl(pred, rs) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_t4_rec_sse2_impl(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dct: &[i16; 16],
) {
    unsafe {
        let (res0, res1, res2, res3) = compute_idct_residuals_sse2(dct);
        for (dy, res) in [res0, res1, res2, res3].into_iter().enumerate() {
            let p: [u8; 4] = pred.row_view(dy as isize, 0, 4).try_into().unwrap();
            let row: &mut [u8; 4] = rec.row_mut(dy as isize, 0, 4).try_into().unwrap();
            *row = add_res_and_clip_sse2(p, res);
        }
    }
}

/// 4x4 IDCT with separate source prediction cursor using SSE2.
///
/// C++: `WelsIDctT4Rec_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_t4_rec_sse2(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dct: &[i16; 16],
) {
    unsafe { idct_t4_rec_sse2_impl(rec, pred, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_t4_rec_in_place_sse2_impl(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    unsafe {
        idct_res_add_pred_sse2_impl(rec, dct);
    }
}

/// [`idct_t4_rec_sse2`] in place on `rec`.
#[inline]
pub fn idct_t4_rec_in_place_sse2(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 16]) {
    unsafe { idct_t4_rec_in_place_sse2_impl(rec, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_four_t4_rec_sse2_impl(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dct: &[i16; 64],
) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    unsafe {
        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
            idct_t4_rec_sse2_impl(&mut rec.reborrow(dx, dy), &pred.advance(dx, dy), sub);
        }
    }
}

/// IDCT over four 4x4 blocks forming an 8x8 quadrant.
///
/// C++: `WelsIDctFourT4Rec_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_four_t4_rec_sse2(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dct: &[i16; 64],
) {
    unsafe { idct_four_t4_rec_sse2_impl(rec, pred, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_four_t4_rec_in_place_sse2_impl(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    unsafe {
        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
            idct_t4_rec_in_place_sse2_impl(&mut rec.reborrow(dx, dy), sub);
        }
    }
}

/// [`idct_t4_rec_in_place_sse2`] over four 4x4 blocks forming an 8x8 quadrant.
#[inline]
pub fn idct_four_t4_rec_in_place_sse2(rec: &mut PlaneCursorMut<'_>, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_in_place_sse2_impl(rec, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_t4_rec_to_view_sse2_impl(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dct: &[i16; 16],
) {
    unsafe {
        let (res0, res1, res2, res3) = compute_idct_residuals_sse2(dct);
        for (dy, res) in [res0, res1, res2, res3].into_iter().enumerate() {
            let p: [u8; 4] = pred[dy * pred_stride..][..4].try_into().unwrap();
            let out = add_res_and_clip_sse2(p, res);
            rec.write_row::<4>(dy as isize, 0, &out);
        }
    }
}

/// [`idct_t4_rec_to_view`] using SSE2.
#[inline]
pub fn idct_t4_rec_to_view_sse2(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dct: &[i16; 16],
) {
    unsafe { idct_t4_rec_to_view_sse2_impl(rec, pred, pred_stride, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_four_t4_rec_to_view_sse2_impl(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dct: &[i16; 64],
) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    unsafe {
        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
            let off = dy as usize * pred_stride + dx as usize;
            idct_t4_rec_to_view_sse2_impl(&rec.advance(dx, dy), &pred[off..], pred_stride, sub);
        }
    }
}

/// [`idct_four_t4_rec_to_view`] using SSE2.
#[inline]
pub fn idct_four_t4_rec_to_view_sse2(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dct: &[i16; 64],
) {
    unsafe { idct_four_t4_rec_to_view_sse2_impl(rec, pred, pred_stride, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_t4_rec_in_place_view_sse2_impl(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    unsafe {
        let (res0, res1, res2, res3) = compute_idct_residuals_sse2(dct);
        for (dy, res) in [res0, res1, res2, res3].into_iter().enumerate() {
            let cur = rec.row::<4>(dy as isize, 0);
            let out = add_res_and_clip_sse2(cur, res);
            rec.write_row::<4>(dy as isize, 0, &out);
        }
    }
}

/// [`idct_t4_rec_in_place_view`] using SSE2.
#[inline]
pub fn idct_t4_rec_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 16]) {
    unsafe { idct_t4_rec_in_place_view_sse2_impl(rec, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_four_t4_rec_in_place_view_sse2_impl(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    const SUBS: [(isize, isize); 4] = [(0, 0), (4, 0), (0, 4), (4, 4)];
    unsafe {
        for (k, &(dx, dy)) in SUBS.iter().enumerate() {
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
            idct_t4_rec_in_place_view_sse2_impl(&rec.advance(dx, dy), sub);
        }
    }
}

/// [`idct_four_t4_rec_in_place_view`] using SSE2.
#[inline]
pub fn idct_four_t4_rec_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 64]) {
    unsafe { idct_four_t4_rec_in_place_view_sse2_impl(rec, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_t4_rec_on_mb_in_place_view_sse2_impl(rec: &RecCursor<'_>, dct: &[i16; 256]) {
    const QUADS: [(isize, isize); 4] = [(0, 0), (8, 0), (0, 8), (8, 8)];
    unsafe {
        for (k, &(dx, dy)) in QUADS.iter().enumerate() {
            let sub: &[i16; 64] = (&dct[k << 6..][..64]).try_into().unwrap();
            idct_four_t4_rec_in_place_view_sse2_impl(&rec.advance(dx, dy), sub);
        }
    }
}

/// [`idct_t4_rec_on_mb_in_place_view`] using SSE2.
#[inline]
pub fn idct_t4_rec_on_mb_in_place_view_sse2(rec: &RecCursor<'_>, dct: &[i16; 256]) {
    unsafe { idct_t4_rec_on_mb_in_place_view_sse2_impl(rec, dct) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_rec_i16x16_dc_sse2_impl(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dc: &[i16; 16],
) {
    unsafe {
        let zero = _mm_setzero_si128();

        for i in 0..16usize {
            let dc_row = i & 0x0C;
            let d0 = ((dc[dc_row] as i32 + 32) >> 6) as i16;
            let d1 = ((dc[dc_row + 1] as i32 + 32) >> 6) as i16;
            let d2 = ((dc[dc_row + 2] as i32 + 32) >> 6) as i16;
            let d3 = ((dc[dc_row + 3] as i32 + 32) >> 6) as i16;

            let dc_lo = _mm_set_epi16(d1, d1, d1, d1, d0, d0, d0, d0);
            let dc_hi = _mm_set_epi16(d3, d3, d3, d3, d2, d2, d2, d2);

            let p_bytes = pred.row(i as isize, 0, 16);
            let p_vec = _mm_loadu_si128(p_bytes.as_ptr() as *const __m128i);

            let p_lo = _mm_unpacklo_epi8(p_vec, zero);
            let p_hi = _mm_unpackhi_epi8(p_vec, zero);

            let sum_lo = _mm_add_epi16(p_lo, dc_lo);
            let sum_hi = _mm_add_epi16(p_hi, dc_hi);

            let packed = _mm_packus_epi16(sum_lo, sum_hi);
            let r: &mut [u8; 16] = rec.row_mut(i as isize, 0, 16).try_into().unwrap();
            _mm_storeu_si128(r.as_mut_ptr() as *mut __m128i, packed);
        }
    }
}

/// 16x16 macroblock DC luma reconstruction using SSE2.
///
/// C++: `WelsIDctRecI16x16Dc_sse2`, `codec/common/x86/dct.asm`.
#[inline]
pub fn idct_rec_i16x16_dc_sse2(
    rec: &mut PlaneCursorMut<'_>,
    pred: &PlaneCursor<'_>,
    dc: &[i16; 16],
) {
    unsafe { idct_rec_i16x16_dc_sse2_impl(rec, pred, dc) }
}

#[target_feature(enable = "sse2")]
unsafe fn idct_rec_i16x16_dc_to_view_sse2_impl(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dc: &[i16; 16],
) {
    unsafe {
        let zero = _mm_setzero_si128();

        for i in 0..16usize {
            let dc_row = i & 0x0C;
            let d0 = ((dc[dc_row] as i32 + 32) >> 6) as i16;
            let d1 = ((dc[dc_row + 1] as i32 + 32) >> 6) as i16;
            let d2 = ((dc[dc_row + 2] as i32 + 32) >> 6) as i16;
            let d3 = ((dc[dc_row + 3] as i32 + 32) >> 6) as i16;

            let dc_lo = _mm_set_epi16(d1, d1, d1, d1, d0, d0, d0, d0);
            let dc_hi = _mm_set_epi16(d3, d3, d3, d3, d2, d2, d2, d2);

            let p_bytes = &pred[i * pred_stride..][..16];
            let p_vec = _mm_loadu_si128(p_bytes.as_ptr() as *const __m128i);

            let p_lo = _mm_unpacklo_epi8(p_vec, zero);
            let p_hi = _mm_unpackhi_epi8(p_vec, zero);

            let sum_lo = _mm_add_epi16(p_lo, dc_lo);
            let sum_hi = _mm_add_epi16(p_hi, dc_hi);

            let packed = _mm_packus_epi16(sum_lo, sum_hi);
            let mut out = [0u8; 16];
            _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, packed);
            rec.write_row::<16>(i as isize, 0, &out);
        }
    }
}

/// 16x16 macroblock DC luma reconstruction to view using SSE2.
#[inline]
pub fn idct_rec_i16x16_dc_to_view_sse2(
    rec: &RecCursor<'_>,
    pred: &[u8],
    pred_stride: usize,
    dc: &[i16; 16],
) {
    unsafe { idct_rec_i16x16_dc_to_view_sse2_impl(rec, pred, pred_stride, dc) }
}

// ============================================================================
// Unit Tests & Parity
// ============================================================================

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
        ((*seed >> 32) & 0xFF) as u8
    }

    /// Coefficients over the **full `i16` range**, which is what the decoder hands the
    /// IDCT: `rs` comes from the bitstream by way of dequantisation, not from this
    /// port's own quantiser. A narrower cap keeps the vertical pass inside the range
    /// where 16- and 32-bit lanes agree, and passes on a kernel that is wrong — see
    /// `compute_idct_residuals_sse2`.
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
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
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
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
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
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
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
            let sub: &[i16; 16] = (&dct[k << 4..][..16]).try_into().unwrap();
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
                let off = (q << 6) + (k << 4);
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
