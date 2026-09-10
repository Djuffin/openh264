//! SSE2 implementations of Motion Compensation (MC) kernels:
//! - Pixel averaging (`pixel_avg`)
//! - Chroma motion compensation (`mc_chroma`)
//! - Horizontal 6-tap Wiener filter (`mc_hor_ver20`)
//! - Vertical 6-tap Wiener filter (`mc_hor_ver02`)
//! - 2D center 6x6-tap Wiener filter (`mc_hor_ver22`)
//! - Luma quarter-pel motion compensation (`mc_luma`)
#![allow(unsafe_code)]

use crate::common::mc::mc_luma_with;
use crate::common::mc::{
    McLeaves, WelsClip1, avg_shaped, cen_shaped, chroma_shaped, filter_input_8bit, g_kuiABCD,
    hor_filter_input_16bit, hor_shaped, mc_copy, ver_shaped,
};
use crate::safe::plane::{BlockRows, PlaneCursorMut, RefSamples};
use core::arch::x86_64::*;

// ============================================================================
// Block shapes and lane moves
// ============================================================================

/// Rows per window cut. A filter body is well past the unroller's threshold at sixteen
/// rows, so `y * stride` stays symbolic and every per-row bounds check with it; one
/// window per group of four restores the constant offsets. See
/// [`PlaneSpanMut::window_mut`](crate::safe::plane::PlaneSpanMut::window_mut).
#[allow(dead_code)]
const ROW_GROUP: usize = 4;

/// Sixteen bytes of a span row as a vector.
#[allow(dead_code)]
#[target_feature(enable = "sse2")]
fn ld16(r: &[u8; 16]) -> __m128i {
    // SAFETY: `&[u8; 16]` is sixteen readable bytes; the load is unaligned.
    unsafe { _mm_loadu_si128(r.as_ptr() as *const __m128i) }
}

/// Eight bytes of a span row in the low half of a vector.
///
/// Through `i64` rather than `_mm_loadl_epi64` because [`BlockRows::row`] hands a row
/// over **by value**: a `[u8; 8]` is one integer register, and taking its address would
/// put it back on the stack. Where the row does come from memory the `from_le_bytes`
/// folds back into a `movq`.
#[target_feature(enable = "sse2")]
fn ld8(r: &[u8; 8]) -> __m128i {
    _mm_cvtsi64_si128(i64::from_le_bytes(*r))
}

/// Four bytes of a span row in the low quarter of a vector; see [`ld8`].
#[target_feature(enable = "sse2")]
fn ld4(r: &[u8; 4]) -> __m128i {
    _mm_cvtsi32_si128(i32::from_ne_bytes(*r))
}

/// Sixteen bytes of `v` to the start of `out`.
#[allow(dead_code)]
#[target_feature(enable = "sse2")]
fn st16(out: &mut [u8], v: __m128i) {
    // SAFETY: the slicing panics unless `out` holds sixteen writable bytes.
    unsafe { _mm_storeu_si128(out[..16].as_mut_ptr() as *mut __m128i, v) }
}

/// The low eight bytes of `v` to the start of `out`; see [`ld8`].
#[target_feature(enable = "sse2")]
fn st8(out: &mut [u8], v: __m128i) {
    out[..8].copy_from_slice(&_mm_cvtsi128_si64(v).to_le_bytes())
}

/// The low four bytes of `v` to the start of `out`; see [`ld8`].
#[target_feature(enable = "sse2")]
fn st4(out: &mut [u8], v: __m128i) {
    out[..4].copy_from_slice(&_mm_cvtsi128_si32(v).to_le_bytes())
}

/// Eight bytes of a window row widened to eight words.
#[target_feature(enable = "sse2")]
fn w8<R: BlockRows>(r: &R, y: usize, x: usize) -> __m128i {
    _mm_unpacklo_epi8(ld8(&r.row::<8>(y, x)), _mm_setzero_si128())
}

/// Four bytes of a window row widened to four words in the low half.
#[target_feature(enable = "sse2")]
fn w4<R: BlockRows>(r: &R, y: usize, x: usize) -> __m128i {
    _mm_unpacklo_epi8(ld4(&r.row::<4>(y, x)), _mm_setzero_si128())
}

// ============================================================================
// Pixel Averaging (SSE2)
// ============================================================================

/// Rounded pixel average of two rows: `((a + b + 1) >> 1) as u8`, `pavgb`.
///
/// The width is a const parameter so the chunk chain below has a constant trip count: a
/// row loop whose body still contains a loop is one the unroller declines, and with it
/// every per-row bounds check stays. See [`ROW_GROUP`].
#[allow(dead_code)]
#[inline(always)]
fn avg_row<const W: usize>(out: &mut [u8; W], a: &[u8; W], b: &[u8; W]) {
    if W == 16 {
        // SAFETY: W == 16 guarantees slices are 16 bytes.
        unsafe {
            let va = _mm_loadu_si128(a.as_ptr() as *const __m128i);
            let vb = _mm_loadu_si128(b.as_ptr() as *const __m128i);
            _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, _mm_avg_epu8(va, vb));
        }
    } else if W == 8 {
        // SAFETY: W == 8 guarantees slices are 8 bytes.
        unsafe {
            let va = _mm_loadl_epi64(a.as_ptr() as *const __m128i);
            let vb = _mm_loadl_epi64(b.as_ptr() as *const __m128i);
            _mm_storel_epi64(out.as_mut_ptr() as *mut __m128i, _mm_avg_epu8(va, vb));
        }
    } else if W == 4 {
        // SAFETY: W == 4 guarantees slices are 4 bytes.
        unsafe {
            let va = _mm_cvtsi32_si128(i32::from_ne_bytes(*(a.as_ptr() as *const [u8; 4])));
            let vb = _mm_cvtsi32_si128(i32::from_ne_bytes(*(b.as_ptr() as *const [u8; 4])));
            let v = _mm_avg_epu8(va, vb);
            *(out.as_mut_ptr() as *mut [u8; 4]) = _mm_cvtsi128_si32(v).to_ne_bytes();
        }
    } else {
        for j in 0..W {
            out[j] = (((a[j] as u32) + (b[j] as u32) + 1) >> 1) as u8;
        }
    }
}

#[inline(always)]
fn avg_block<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
) {
    let sa = a.span::<W, H>(0, 0);
    let sb = b.span::<W, H>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let (ra, rb) = (sa.row::<W>(y, 0), sb.row::<W>(y, 0));
        let out = d.row_mut::<W>(y, 0);
        for j in 0..W {
            out[j] = (((ra[j] as u32) + (rb[j] as u32) + 1) >> 1) as u8;
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`].
fn avg_any<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            *o = (((a.at(j as isize, dy) as u32) + (b.at(j as isize, dy) as u32) + 1) >> 1) as u8;
        }
    }
}

/// Public safe entry point for SSE2 pixel averaging.
#[inline(always)]
pub fn pixel_avg<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    avg_shaped::<Sse2Leaves, A, B>(dst, a, b, width, height)
}

// ============================================================================
// Chroma MC (SSE2)
// ============================================================================

/// One output row of the bilinear filter at width 8 or 4, over the two one-row
/// windows `r0` and `r1`.
#[allow(dead_code)]
#[target_feature(enable = "sse2")]
fn chroma_row<R: BlockRows, const W: usize>(
    out: &mut [u8; W],
    r0: &R,
    r1: &R,
    vA: __m128i,
    vB: __m128i,
    vC: __m128i,
    vD: __m128i,
) {
    let (p00, p01, p10, p11) = if W == 8 {
        (w8(r0, 0, 0), w8(r0, 0, 1), w8(r1, 0, 0), w8(r1, 0, 1))
    } else {
        (w4(r0, 0, 0), w4(r0, 0, 1), w4(r1, 0, 0), w4(r1, 0, 1))
    };
    let sum = _mm_add_epi16(
        _mm_add_epi16(_mm_mullo_epi16(p00, vA), _mm_mullo_epi16(p01, vB)),
        _mm_add_epi16(_mm_mullo_epi16(p10, vC), _mm_mullo_epi16(p11, vD)),
    );
    let shifted = _mm_srli_epi16(_mm_add_epi16(sum, _mm_set1_epi16(32)), 6);
    let packed = _mm_packus_epi16(shifted, _mm_setzero_si128());
    if W == 8 {
        st8(out, packed);
    } else {
        st4(out, packed);
    }
}

/// The bilinear chroma filter over one const-shape block. Widths 8 and 4 take the
/// lane path; width 2 is the scalar.
#[target_feature(enable = "sse4.1")]
unsafe fn chroma_block<
    S: RefSamples + Copy,
    const W: usize,
    const SW: usize,
    const H: usize,
    const SH: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
) {
    let s = src.span::<SW, SH>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    if W == 8 {
        let coeff_ab = _mm_set1_epi16(((w[1] as i16) << 8) | (w[0] as i16));
        let coeff_cd = _mm_set1_epi16(((w[3] as i16) << 8) | (w[2] as i16));
        let round_32 = _mm_set1_epi16(32);

        let load_interleaved_8 = |y: usize| -> __m128i {
            let r = s.row::<SW>(y, 0);
            let lo = i64::from_ne_bytes(r[..8].try_into().unwrap());
            let hi = r[8] as i64;
            let v_lo = _mm_cvtsi64_si128(lo);
            let v_hi = _mm_cvtsi64_si128(hi);
            let r0 = _mm_unpacklo_epi64(v_lo, v_hi);
            let r1 = _mm_srli_si128::<1>(r0);
            _mm_unpacklo_epi8(r0, r1)
        };

        let mut curr_interleaved = load_interleaved_8(0);

        for y in 0..H {
            let next_interleaved = load_interleaved_8(y + 1);

            let term0 = _mm_maddubs_epi16(curr_interleaved, coeff_ab);
            let term1 = _mm_maddubs_epi16(next_interleaved, coeff_cd);
            let sum = _mm_add_epi16(term0, term1);
            let shifted = _mm_srli_epi16(_mm_add_epi16(sum, round_32), 6);
            let packed = _mm_packus_epi16(shifted, shifted);

            let out = d.row_mut::<8>(y, 0);
            *out = _mm_cvtsi128_si64(packed).to_ne_bytes();

            curr_interleaved = next_interleaved;
        }
    } else if W == 4 {
        let coeff_ab = _mm_set1_epi16(((w[1] as i16) << 8) | (w[0] as i16));
        let coeff_cd = _mm_set1_epi16(((w[3] as i16) << 8) | (w[2] as i16));
        let round_32 = _mm_set1_epi16(32);

        let load_interleaved_4 = |y: usize| -> __m128i {
            let r = s.row::<SW>(y, 0);
            unsafe {
                let r_lo = _mm_cvtsi32_si128(i32::from_ne_bytes(*(r.as_ptr() as *const [u8; 4])));
                let r_hi =
                    _mm_cvtsi32_si128(i32::from_ne_bytes(*(r.as_ptr().add(1) as *const [u8; 4])));
                _mm_unpacklo_epi8(r_lo, r_hi)
            }
        };

        let mut curr_interleaved = load_interleaved_4(0);

        for y in 0..H {
            let next_interleaved = load_interleaved_4(y + 1);

            let term0 = _mm_maddubs_epi16(curr_interleaved, coeff_ab);
            let term1 = _mm_maddubs_epi16(next_interleaved, coeff_cd);
            let sum = _mm_add_epi16(term0, term1);
            let shifted = _mm_srli_epi16(_mm_add_epi16(sum, round_32), 6);
            let packed = _mm_packus_epi16(shifted, shifted);

            let out = d.row_mut::<W>(y, 0);
            unsafe {
                *(out.as_mut_ptr() as *mut [u8; 4]) = _mm_cvtsi128_si32(packed).to_ne_bytes();
            }

            curr_interleaved = next_interleaved;
        }
    } else {
        let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
        for y in 0..H {
            let (r0, r1) = (s.row::<SW>(y, 0), s.row::<SW>(y + 1, 0));
            let out = d.row_mut::<W>(y, 0);
            for j in 0..W {
                out[j] = ((iA * (r0[j] as i32)
                    + iB * (r0[j + 1] as i32)
                    + iC * (r1[j] as i32)
                    + iD * (r1[j + 1] as i32)
                    + 32)
                    >> 6) as u8;
            }
        }
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`].
fn chroma_any<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    w: &[u8; 4],
    width: usize,
    height: usize,
) {
    let (iA, iB, iC, iD) = (w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32);
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (j, o) in out.iter_mut().enumerate() {
            let x = j as isize;
            *o = ((iA * (src.at(x, dy) as i32)
                + iB * (src.at(x + 1, dy) as i32)
                + iC * (src.at(x, dy + 1) as i32)
                + iD * (src.at(x + 1, dy + 1) as i32)
                + 32)
                >> 6) as u8;
        }
    }
}

/// SIMD block copy for whole-pixel motion vectors.
#[inline(always)]
fn copy_block_simd<const W: usize, const H: usize, S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, H>(0, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        *d.row_mut::<W>(y, 0) = s.row::<W>(y, 0);
    }
}

/// Public safe entry point for SSE2 chroma MC.
#[inline(always)]
pub fn mc_chroma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        match (width, height) {
            (8, 8) => copy_block_simd::<8, 8, S>(src, dst),
            (8, 4) => copy_block_simd::<8, 4, S>(src, dst),
            (4, 8) => copy_block_simd::<4, 8, S>(src, dst),
            (4, 4) => copy_block_simd::<4, 4, S>(src, dst),
            (2, 2) => copy_block_simd::<2, 2, S>(src, dst),
            _ => mc_copy(src, dst, width, height),
        }
        return;
    }
    mc_chroma_frac(src, dst, mv_x, mv_y, width, height)
}

/// The fractional half of [`mc_chroma`], out of line so the entry point stays small
/// enough to inline. The whole-sample vector is the common chroma case and a block copy;
/// with the bilinear dispatch in the same body, `mc_copy` lost its constant width and
/// height at that call site.
#[inline(always)]
fn mc_chroma_frac<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if width == 0 {
        return;
    }
    let w = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    chroma_shaped::<Sse2Leaves, S>(src, dst, w, width, height)
}

// ============================================================================
// 6-Tap Filter Helpers (SSE2)
// ============================================================================

/// Vectorized 6-tap Wiener filter on 8 samples:
/// `val = (p0 + p5) - 5 * (p1 + p4) + 20 * (p2 + p3)`
/// `res = WelsClip1((val + 16) >> 5)`
///
/// Implemented using the identity:
/// `x = 4 * (p2 + p3) - (p1 + p4)`
/// `val = (p0 + p5) + x + (x << 2)`
#[allow(dead_code)]
#[target_feature(enable = "sse2")]
fn filter_6tap_8_samples(
    p0: __m128i,
    p1: __m128i,
    p2: __m128i,
    p3: __m128i,
    p4: __m128i,
    p5: __m128i,
) -> __m128i {
    let p14 = _mm_add_epi16(p1, p4);
    let p23 = _mm_add_epi16(p2, p3);
    let x = _mm_sub_epi16(_mm_slli_epi16(p23, 2), p14);
    let p05 = _mm_add_epi16(p0, p5);
    let sum = _mm_add_epi16(p05, _mm_add_epi16(x, _mm_slli_epi16(x, 2)));
    let rounded = _mm_add_epi16(sum, _mm_set1_epi16(16));
    let shifted = _mm_srai_epi16(rounded, 5);
    _mm_packus_epi16(shifted, _mm_setzero_si128())
}

/// Computes the unclipped 16-bit intermediate for 2D filter:
/// `val = (p0 + p5) - 5 * (p1 + p4) + 20 * (p2 + p3)`
#[allow(dead_code)]
#[target_feature(enable = "sse2")]
fn filter_6tap_intermediate_8_samples(
    p0: __m128i,
    p1: __m128i,
    p2: __m128i,
    p3: __m128i,
    p4: __m128i,
    p5: __m128i,
) -> __m128i {
    let p14 = _mm_add_epi16(p1, p4);
    let p23 = _mm_add_epi16(p2, p3);
    let x = _mm_sub_epi16(_mm_slli_epi16(p23, 2), p14);
    let p05 = _mm_add_epi16(p0, p5);
    _mm_add_epi16(p05, _mm_add_epi16(x, _mm_slli_epi16(x, 2)))
}

// ============================================================================
// Horizontal 6-Tap Filter: McHorVer20 (SSE2)
// ============================================================================

/// Vectorized 6-tap Wiener filter on 8 samples using SSSE3 pmaddubsw and pshufb.
#[target_feature(enable = "sse4.1")]
#[inline]
unsafe fn filter_6tap_8px(raw: __m128i) -> __m128i {
    let mask_01 = _mm_setr_epi8(0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8);
    let mask_23 = _mm_setr_epi8(2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10);
    let mask_45 = _mm_setr_epi8(4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12);

    let coeff_01 = _mm_setr_epi8(1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5);
    let coeff_23 = _mm_set1_epi8(20);
    let coeff_45 = _mm_setr_epi8(-5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1, -5, 1);

    let p01 = _mm_shuffle_epi8(raw, mask_01);
    let p23 = _mm_shuffle_epi8(raw, mask_23);
    let p45 = _mm_shuffle_epi8(raw, mask_45);

    let m01 = _mm_maddubs_epi16(p01, coeff_01);
    let m23 = _mm_maddubs_epi16(p23, coeff_23);
    let m45 = _mm_maddubs_epi16(p45, coeff_45);

    _mm_add_epi16(_mm_add_epi16(m01, m45), m23)
}

#[target_feature(enable = "sse4.1")]
#[inline]
unsafe fn hor_row_fast<const W: usize, const SW: usize, const AVG: usize>(
    out: &mut [u8; W],
    src_row: &[u8; SW],
) {
    let mut buf = [0u8; 32];
    buf[..SW].copy_from_slice(src_row);

    unsafe {
        let r0 = _mm_loadu_si128(buf.as_ptr() as *const __m128i);
        let r1 = _mm_loadu_si128(buf.as_ptr().add(16) as *const __m128i);

        let mut col = 0;
        while col + 16 <= W {
            let r_lo = r0;
            let r_hi = _mm_alignr_epi8(r1, r0, 8);
            let sum_lo = filter_6tap_8px(r_lo);
            let sum_hi = filter_6tap_8px(r_hi);
            let shifted_lo = _mm_srai_epi16(_mm_add_epi16(sum_lo, _mm_set1_epi16(16)), 5);
            let shifted_hi = _mm_srai_epi16(_mm_add_epi16(sum_hi, _mm_set1_epi16(16)), 5);
            let mut res16 = _mm_packus_epi16(shifted_lo, shifted_hi);
            if AVG != 0 {
                let tap16 = _mm_loadu_si128(buf.as_ptr().add(col + AVG) as *const __m128i);
                res16 = _mm_avg_epu8(res16, tap16);
            }
            _mm_storeu_si128(out[col..][..16].as_mut_ptr() as *mut __m128i, res16);
            col += 16;
        }
        if col + 8 <= W {
            let sum = filter_6tap_8px(r0);
            let shifted = _mm_srai_epi16(_mm_add_epi16(sum, _mm_set1_epi16(16)), 5);
            let mut res8 = _mm_packus_epi16(shifted, shifted);
            if AVG != 0 {
                let tap8 = _mm_loadl_epi64(buf.as_ptr().add(col + AVG) as *const __m128i);
                res8 = _mm_avg_epu8(res8, tap8);
            }
            _mm_storel_epi64(out[col..][..8].as_mut_ptr() as *mut __m128i, res8);
            col += 8;
        }
        if col + 4 <= W {
            let sum = filter_6tap_8px(r0);
            let shifted = _mm_srai_epi16(_mm_add_epi16(sum, _mm_set1_epi16(16)), 5);
            let mut res4 = _mm_packus_epi16(shifted, shifted);
            if AVG != 0 {
                let tap4 = _mm_cvtsi32_si128(i32::from_ne_bytes(
                    *(buf.as_ptr().add(col + AVG) as *const [u8; 4]),
                ));
                res4 = _mm_avg_epu8(res4, tap4);
            }
            *(out[col..][..4].as_mut_ptr() as *mut [u8; 4]) = _mm_cvtsi128_si32(res4).to_ne_bytes();
            col += 4;
        }
        while col < W {
            let t: [u8; 6] = buf[col..col + 6].try_into().unwrap();
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + buf[col + AVG] as u32 + 1) >> 1) as u8;
            }
            out[col] = v;
            col += 1;
        }
    }
}

/// `McHorVer20` over one const-shape block: one span for the source, one for the
/// destination, walked row by row.
#[target_feature(enable = "sse4.1")]
unsafe fn hor_block<
    S: RefSamples + Copy,
    const W: usize,
    const SW: usize,
    const H: usize,
    const AVG: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<SW, H>(0, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let r = s.row::<SW>(y, 0);
        let out = d.row_mut::<W>(y, 0);
        unsafe { hor_row_fast::<W, SW, AVG>(out, &r) };
    }
}

/// The run-time-shape twin — cold, and scalar; see [`McLeaves`].
fn hor_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| src.at(x as isize + k as isize - 2, dy));
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// Public safe entry point for SSE2 horizontal half-pel filter.
pub fn mc_hor_ver20<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    hor_shaped::<Sse2Leaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// Vertical 6-Tap Filter: McHorVer02 (SSE2)
// ============================================================================

#[target_feature(enable = "sse4.1")]
#[inline]
unsafe fn filter_6tap_vertical_words(
    p0: __m128i,
    p1: __m128i,
    p2: __m128i,
    p3: __m128i,
    p4: __m128i,
    p5: __m128i,
) -> __m128i {
    let p14 = _mm_add_epi16(p1, p4);
    let p23 = _mm_add_epi16(p2, p3);
    let x = _mm_sub_epi16(_mm_slli_epi16(p23, 2), p14);
    let p05 = _mm_add_epi16(p0, p5);
    let sum = _mm_add_epi16(p05, _mm_add_epi16(x, _mm_slli_epi16(x, 2)));
    let rounded = _mm_add_epi16(sum, _mm_set1_epi16(16));
    _mm_srai_epi16(rounded, 5)
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn filter_6tap_vertical_avx2(
    p0: __m256i,
    p1: __m256i,
    p2: __m256i,
    p3: __m256i,
    p4: __m256i,
    p5: __m256i,
) -> __m256i {
    let p14 = _mm256_add_epi16(p1, p4);
    let p23 = _mm256_add_epi16(p2, p3);
    let x = _mm256_sub_epi16(_mm256_slli_epi16(p23, 2), p14);
    let p05 = _mm256_add_epi16(p0, p5);
    let sum = _mm256_add_epi16(p05, _mm256_add_epi16(x, _mm256_slli_epi16(x, 2)));
    let rounded = _mm256_add_epi16(sum, _mm256_set1_epi16(16));
    _mm256_srai_epi16(rounded, 5)
}

#[target_feature(enable = "avx2")]
unsafe fn ver_lanes_avx2_16x<
    S: RefSamples + Copy,
    const H: usize,
    const SH: usize,
    const AVG: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<16, SH>(-2, 0);
    let mut d = dst.span_mut::<16, H>(0, 0);

    let load_row = |y: usize| -> __m256i {
        unsafe {
            let r = s.row::<16>(y, 0);
            let raw = _mm_loadu_si128(r.as_ptr() as *const __m128i);
            _mm256_cvtepu8_epi16(raw)
        }
    };

    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        load_row(0),
        load_row(1),
        load_row(2),
        load_row(3),
        load_row(4),
    );

    for y in 0..H {
        let r5 = load_row(y + 5);
        let out = d.row_mut::<16>(y, 0);

        unsafe {
            let w = filter_6tap_vertical_avx2(r0, r1, r2, r3, r4, r5);
            let lo = _mm256_castsi256_si128(w);
            let hi = _mm256_extracti128_si256(w, 1);
            let mut both = _mm_packus_epi16(lo, hi);
            if AVG != 0 {
                let tap = _mm_loadu_si128(s.row::<16>(y + AVG, 0).as_ptr() as *const __m128i);
                both = _mm_avg_epu8(both, tap);
            }
            _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, both);
        }

        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// The vertical filter at width 16, 8 or 4: the five-row window carried in widened
/// registers and one new row read per output row.
#[target_feature(enable = "sse4.1")]
unsafe fn ver_lanes<
    S: RefSamples + Copy,
    const W: usize,
    const H: usize,
    const SH: usize,
    const AVG: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let zero = _mm_setzero_si128();

    let load_row = |y: usize| -> [__m128i; 2] {
        unsafe {
            if W == 16 {
                let r = s.row::<16>(y, 0);
                let raw = _mm_loadu_si128(r.as_ptr() as *const __m128i);
                [_mm_unpacklo_epi8(raw, zero), _mm_unpackhi_epi8(raw, zero)]
            } else if W == 8 {
                let r = s.row::<8>(y, 0);
                let raw = _mm_loadl_epi64(r.as_ptr() as *const __m128i);
                [_mm_unpacklo_epi8(raw, zero), zero]
            } else {
                let r = s.row::<4>(y, 0);
                let raw = _mm_cvtsi32_si128(i32::from_ne_bytes(r));
                [_mm_unpacklo_epi8(raw, zero), zero]
            }
        }
    };

    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        load_row(0),
        load_row(1),
        load_row(2),
        load_row(3),
        load_row(4),
    );

    for y in 0..H {
        let r5 = load_row(y + 5);
        let out = d.row_mut::<W>(y, 0);

        unsafe {
            if W == 16 {
                let w_lo = filter_6tap_vertical_words(r0[0], r1[0], r2[0], r3[0], r4[0], r5[0]);
                let w_hi = filter_6tap_vertical_words(r0[1], r1[1], r2[1], r3[1], r4[1], r5[1]);
                let mut both = _mm_packus_epi16(w_lo, w_hi);
                if AVG != 0 {
                    let tap = _mm_loadu_si128(s.row::<16>(y + AVG, 0).as_ptr() as *const __m128i);
                    both = _mm_avg_epu8(both, tap);
                }
                _mm_storeu_si128(out.as_mut_ptr() as *mut __m128i, both);
            } else if W == 8 {
                let w_lo = filter_6tap_vertical_words(r0[0], r1[0], r2[0], r3[0], r4[0], r5[0]);
                let mut res = _mm_packus_epi16(w_lo, w_lo);
                if AVG != 0 {
                    let tap = _mm_loadl_epi64(s.row::<8>(y + AVG, 0).as_ptr() as *const __m128i);
                    res = _mm_avg_epu8(res, tap);
                }
                _mm_storel_epi64(out.as_mut_ptr() as *mut __m128i, res);
            } else {
                let w_lo = filter_6tap_vertical_words(r0[0], r1[0], r2[0], r3[0], r4[0], r5[0]);
                let mut res = _mm_packus_epi16(w_lo, w_lo);
                if AVG != 0 {
                    let tap = _mm_cvtsi32_si128(i32::from_ne_bytes(s.row::<4>(y + AVG, 0)));
                    res = _mm_avg_epu8(res, tap);
                }
                *(out.as_mut_ptr() as *mut [u8; 4]) = _mm_cvtsi128_si32(res).to_ne_bytes();
            }
        }

        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// The widths the lane path has no form for: the scalar over the same span.
#[target_feature(enable = "sse2")]
fn ver_odd<
    S: RefSamples + Copy,
    const W: usize,
    const H: usize,
    const SH: usize,
    const AVG: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    let s = src.span::<W, SH>(-2, 0);
    let mut d = dst.span_mut::<W, H>(0, 0);
    for y in 0..H {
        let out = d.row_mut::<W>(y, 0);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| s.row::<1>(y + k, x)[0]);
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// `McHorVer02` over one const-shape block: the width picks the path, and the
/// `match` folds because `W` is a constant.
#[target_feature(enable = "sse4.1")]
unsafe fn ver_block<
    S: RefSamples + Copy,
    const W: usize,
    const H: usize,
    const SH: usize,
    const AVG: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    if W == 16 && crate::simd::has_avx2() {
        unsafe { ver_lanes_avx2_16x::<S, H, SH, AVG>(src, dst) }
    } else {
        match W {
            16 | 8 | 4 => unsafe { ver_lanes::<S, W, H, SH, AVG>(src, dst) },
            _ => ver_odd::<S, W, H, SH, AVG>(src, dst),
        }
    }
}

/// The run-time-shape twin — cold, and scalar; see [`McLeaves`].
fn ver_any<S: RefSamples + Copy, const AVG: usize>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let out = dst.row_mut(dy, 0, width);
        for (x, o) in out.iter_mut().enumerate() {
            let t: [u8; 6] = std::array::from_fn(|k| src.at(x as isize, dy + k as isize - 2));
            let mut v = WelsClip1((filter_input_8bit(&t) + 16) >> 5);
            if AVG != 0 {
                v = ((v as u32 + t[AVG] as u32 + 1) >> 1) as u8;
            }
            *o = v;
        }
    }
}

/// Public safe entry point for SSE2 vertical half-pel filter.
pub fn mc_hor_ver02<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    ver_shaped::<Sse2Leaves, S, 0>(src, dst, width, height)
}

// ============================================================================
// 2D Center 6x6-Tap Filter: McHorVer22 (SSE2)
// ============================================================================

#[target_feature(enable = "sse4.1")]
#[inline]
unsafe fn hor_filter_4px_16bit(v0: __m128i, v1: __m128i) -> __m128i {
    let mask_01 = _mm_setr_epi8(0, 1, 2, 3, 2, 3, 4, 5, 4, 5, 6, 7, 6, 7, 8, 9);
    let mask_23 = _mm_setr_epi8(4, 5, 6, 7, 6, 7, 8, 9, 8, 9, 10, 11, 10, 11, 12, 13);

    let coeff_01 = _mm_setr_epi16(1, -5, 1, -5, 1, -5, 1, -5);
    let coeff_23 = _mm_set1_epi16(20);
    let coeff_45 = _mm_setr_epi16(-5, 1, -5, 1, -5, 1, -5, 1);

    let p01 = _mm_shuffle_epi8(v0, mask_01);
    let p23 = _mm_shuffle_epi8(v0, mask_23);
    let v_align4 = _mm_alignr_epi8(v1, v0, 8);
    let p45 = _mm_shuffle_epi8(v_align4, mask_01);

    let m01 = _mm_madd_epi16(p01, coeff_01);
    let m23 = _mm_madd_epi16(p23, coeff_23);
    let m45 = _mm_madd_epi16(p45, coeff_45);

    let sum = _mm_add_epi32(_mm_add_epi32(m01, m45), m23);
    let rounded = _mm_add_epi32(sum, _mm_set1_epi32(512));
    _mm_srai_epi32(rounded, 10)
}

#[target_feature(enable = "sse4.1")]
#[inline]
unsafe fn hor_filter_8px_16bit(itmp_ptr: *const i16) -> __m128i {
    let v0 = unsafe { _mm_loadu_si128(itmp_ptr as *const __m128i) };
    let v1 = unsafe { _mm_loadu_si128(itmp_ptr.add(8) as *const __m128i) };
    let s0 = unsafe { hor_filter_4px_16bit(v0, v1) };

    let v0_hi = unsafe { _mm_loadu_si128(itmp_ptr.add(4) as *const __m128i) };
    let v1_hi = unsafe { _mm_loadu_si128(itmp_ptr.add(12) as *const __m128i) };
    let s1 = unsafe { hor_filter_4px_16bit(v0_hi, v1_hi) };

    _mm_packs_epi32(s0, s1)
}

/// `McHorVer22` over one const-shape block: the vertical 6-tap into `iTmp` over a
/// sliding 6-row window, then the vectorized horizontal pass over those.
#[target_feature(enable = "sse4.1")]
unsafe fn cen_block<
    S: RefSamples + Copy,
    const W: usize,
    const SW: usize,
    const H: usize,
    const SH: usize,
>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
) {
    const {
        assert!(
            SW <= 17 + 5,
            "mc_hor_ver22 width exceeds the 17 iTmp is sized for"
        )
    };
    let s = src.span::<SW, SH>(-2, -2);
    let mut d = dst.span_mut::<W, H>(0, 0);
    let mut iTmp = [0i16; 32];

    let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
        s.row::<SW>(0, 0),
        s.row::<SW>(1, 0),
        s.row::<SW>(2, 0),
        s.row::<SW>(3, 0),
        s.row::<SW>(4, 0),
    );

    for y in 0..H {
        let r5 = s.row::<SW>(y + 5, 0);

        for j in 0..SW {
            let p05 = r0[j] as i16 + r5[j] as i16;
            let p14 = r1[j] as i16 + r4[j] as i16;
            let p23 = r2[j] as i16 + r3[j] as i16;
            iTmp[j] = p05 - 5 * p14 + 20 * p23;
        }

        let out = d.row_mut::<W>(y, 0);
        let mut col = 0;
        while col + 16 <= W {
            let w_lo = unsafe { hor_filter_8px_16bit(iTmp.as_ptr().add(col)) };
            let w_hi = unsafe { hor_filter_8px_16bit(iTmp.as_ptr().add(col + 8)) };
            let res16 = _mm_packus_epi16(w_lo, w_hi);
            unsafe { _mm_storeu_si128(out[col..][..16].as_mut_ptr() as *mut __m128i, res16) };
            col += 16;
        }
        if col + 8 <= W {
            let w_lo = unsafe { hor_filter_8px_16bit(iTmp.as_ptr().add(col)) };
            let res8 = _mm_packus_epi16(w_lo, w_lo);
            unsafe { _mm_storel_epi64(out[col..][..8].as_mut_ptr() as *mut __m128i, res8) };
            col += 8;
        }
        if col + 4 <= W {
            let v0 = unsafe { _mm_loadu_si128(iTmp.as_ptr().add(col) as *const __m128i) };
            let v1 = unsafe { _mm_loadu_si128(iTmp.as_ptr().add(col + 8) as *const __m128i) };
            let s0 = unsafe { hor_filter_4px_16bit(v0, v1) };
            let w4 = _mm_packs_epi32(s0, s0);
            let res4 = _mm_packus_epi16(w4, w4);
            unsafe {
                *(out[col..][..4].as_mut_ptr() as *mut [u8; 4]) =
                    _mm_cvtsi128_si32(res4).to_ne_bytes();
            }
            col += 4;
        }
        while col < W {
            let t: &[i16; 6] = iTmp[col..col + 6].try_into().unwrap();
            out[col] = WelsClip1((hor_filter_input_16bit(t) + 512) >> 10);
            col += 1;
        }

        (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
    }
}

/// The run-time-shape twin — cold; see [`McLeaves`]. The `width <= 17` contract is what
/// sizes `iTmp`.
fn cen_any<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    assert!(
        width <= 17,
        "mc_hor_ver22 width {width} exceeds the 17 iTmp is sized for"
    );
    let n = width + 5;
    let mut iTmp = [0i16; 17 + 5];
    for dy in 0..height as isize {
        for (j, t) in iTmp[..n].iter_mut().enumerate() {
            let x = j as isize - 2;
            let p: [u8; 6] = std::array::from_fn(|k| src.at(x, dy + k as isize - 2));
            *t = filter_input_8bit(&p) as i16;
        }
        let out = dst.row_mut(dy, 0, width);
        for (o, t) in out.iter_mut().zip(iTmp[..n].windows(6)) {
            *o = WelsClip1((hor_filter_input_16bit(t.try_into().expect("six taps")) + 512) >> 10);
        }
    }
}

/// Public safe entry point for SSE2 center half-pel filter.
pub fn mc_hor_ver22<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    cen_shaped::<Sse2Leaves, S>(src, dst, width, height)
}

// ============================================================================
// Luma Quarter-Pel MC (SSE2)
// ============================================================================

/// The SSE2 leaf set — `McLeaves` with the four kernels in this file. The twelve
/// quarter-pel composites live once, in [`crate::common::mc::McLeaves`].
pub struct Sse2Leaves;

impl McLeaves for Sse2Leaves {
    const FUSED_QPEL: bool = true;
    #[inline(always)]
    fn hor<
        S: RefSamples + Copy,
        const W: usize,
        const SW: usize,
        const H: usize,
        const AVG: usize,
    >(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { hor_block::<S, W, SW, H, AVG>(src, dst) }
    }
    #[inline(always)]
    fn hor_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        hor_any::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn ver<
        S: RefSamples + Copy,
        const W: usize,
        const H: usize,
        const SH: usize,
        const AVG: usize,
    >(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { ver_block::<S, W, H, SH, AVG>(src, dst) }
    }
    #[inline(always)]
    fn ver_any<S: RefSamples + Copy, const AVG: usize>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        ver_any::<S, AVG>(src, dst, width, height)
    }
    #[inline(always)]
    fn cen<
        S: RefSamples + Copy,
        const W: usize,
        const SW: usize,
        const H: usize,
        const SH: usize,
    >(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { cen_block::<S, W, SW, H, SH>(src, dst) }
    }
    #[inline(always)]
    fn cen_any<S: RefSamples + Copy>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        width: usize,
        height: usize,
    ) {
        cen_any::<S>(src, dst, width, height)
    }
    #[inline(always)]
    fn avg<A: RefSamples, B: RefSamples, const W: usize, const H: usize>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
    ) {
        avg_block::<A, B, W, H>(dst, a, b);
    }
    #[inline(always)]
    fn avg_any<A: RefSamples, B: RefSamples>(
        dst: &mut PlaneCursorMut<'_>,
        a: &A,
        b: &B,
        width: usize,
        height: usize,
    ) {
        avg_any::<A, B>(dst, a, b, width, height)
    }
    #[inline(always)]
    fn chroma<
        S: RefSamples + Copy,
        const W: usize,
        const SW: usize,
        const H: usize,
        const SH: usize,
    >(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
    ) {
        // SAFETY: SSE2 is baseline on x86_64.
        unsafe { chroma_block::<S, W, SW, H, SH>(src, dst, w) }
    }
    #[inline(always)]
    fn chroma_any<S: RefSamples + Copy>(
        src: &S,
        dst: &mut PlaneCursorMut<'_>,
        w: &[u8; 4],
        width: usize,
        height: usize,
    ) {
        chroma_any::<S>(src, dst, w, width, height)
    }
}

/// Public safe entry point for SSE2 luma quarter-pel MC.
#[inline(always)]
pub fn mc_luma<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x03) == 0 && (mv_y & 0x03) == 0 {
        match (width, height) {
            (16, 16) => copy_block_simd::<16, 16, S>(src, dst),
            (16, 8) => copy_block_simd::<16, 8, S>(src, dst),
            (8, 16) => copy_block_simd::<8, 16, S>(src, dst),
            (8, 8) => copy_block_simd::<8, 8, S>(src, dst),
            (8, 4) => copy_block_simd::<8, 4, S>(src, dst),
            (4, 8) => copy_block_simd::<4, 8, S>(src, dst),
            (4, 4) => copy_block_simd::<4, 4, S>(src, dst),
            _ => mc_copy(src, dst, width, height),
        }
        return;
    }
    mc_luma_frac(src, dst, mv_x, mv_y, width, height)
}

#[inline(always)]
fn mc_luma_frac<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    mc_luma_with::<Sse2Leaves, S>(src, dst, mv_x, mv_y, width, height)
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::plane::PlaneCursor;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers, which
    // route to the very SSE2 kernels under test.
    use crate::common::mc::McLeaves;
    use crate::common::mc::{
        mc_chroma_with_frag_mv, mc_hor_ver02_c as scalar_hor_ver02,
        mc_hor_ver20_c as scalar_hor_ver20, mc_hor_ver22_c as scalar_hor_ver22,
        mc_luma_c as scalar_luma, pixel_avg_c as scalar_pixel_avg,
    };
    use crate::encoder::rec_view::RecCursor;

    const STRIDE: usize = 64;
    const ROWS: usize = 64;

    fn filled_plane() -> Vec<u8> {
        let mut v = vec![0u8; STRIDE * ROWS];
        let mut s: u32 = 0xdead_beef;
        for b in v.iter_mut() {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *b = (s >> 16) as u8;
        }
        v
    }

    #[test]
    fn test_pixel_avg_parity() {
        let a = filled_plane();
        let mut b = a.clone();
        for x in b.iter_mut() {
            *x = x.wrapping_add(42);
        }

        let ca = PlaneCursor::new(&a, 10 * STRIDE + 8, STRIDE);
        let cb = PlaneCursor::new(&b, 12 * STRIDE + 8, STRIDE);

        for (w, h) in [
            (16, 16),
            (16, 8),
            (8, 16),
            (8, 8),
            (8, 4),
            (4, 8),
            (4, 4),
            (17, 16),
            (9, 8),
            (5, 4),
            (2, 2),
        ] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, 10 * STRIDE + 8, STRIDE);
            scalar_pixel_avg(&mut cur_scalar, &ca, &cb, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, 10 * STRIDE + 8, STRIDE);
            pixel_avg(&mut cur_simd, &ca, &cb, w, h);

            assert_eq!(dst_scalar, dst_simd, "pixel_avg mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_chroma_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)];

        for &(w, h) in &shapes {
            for dy in 0..8i16 {
                for dx in 0..8i16 {
                    let mut dst_scalar = vec![0u8; STRIDE * ROWS];
                    let mut dst_simd = vec![0u8; STRIDE * ROWS];

                    let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
                    if (dx & 7) == 0 && (dy & 7) == 0 {
                        mc_copy(&src, &mut cur_scalar, w, h);
                    } else {
                        mc_chroma_with_frag_mv(&src, &mut cur_scalar, dx, dy, w, h);
                    }

                    let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
                    mc_chroma(&src, &mut cur_simd, dx, dy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_chroma mismatch at {w}x{h} with mv=({dx}, {dy})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_mc_hor_ver20_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16),
            (16, 8),
            (8, 16),
            (8, 8),
            (8, 4),
            (4, 8),
            (4, 4),
            (17, 16),
            (17, 8),
            (9, 16),
            (9, 8),
            // Outside the const tables, so these drive the run-time fallback.
            (5, 8),
            (5, 4),
            (17, 17),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver20(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver20(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver20 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_hor_ver02_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16),
            (16, 8),
            (8, 16),
            (8, 8),
            (8, 4),
            (4, 8),
            (4, 4),
            (16, 17),
            (16, 9),
            (8, 17),
            (8, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (8, 5),
            (4, 5),
            (17, 17),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver02(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver02(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver02 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_hor_ver22_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [
            (16, 16),
            (16, 8),
            (8, 16),
            (8, 8),
            (8, 4),
            (4, 8),
            (4, 4),
            (17, 17),
            (17, 9),
            (9, 17),
            (9, 9),
            // Outside the const tables, so these drive the run-time fallback.
            (9, 5),
            (5, 5),
            (17, 16),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver22(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver22(&src, &mut cur_simd, w, h);

            assert_eq!(dst_scalar, dst_simd, "mc_hor_ver22 mismatch at {w}x{h}");
        }
    }

    #[test]
    fn test_mc_luma_parity() {
        let base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let src = PlaneCursor::new(&base, src_c, STRIDE);

        let shapes = [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)];

        for &(w, h) in &shapes {
            for qy in 0..4i16 {
                for qx in 0..4i16 {
                    let mut dst_scalar = vec![0u8; STRIDE * ROWS];
                    let mut dst_simd = vec![0u8; STRIDE * ROWS];

                    let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
                    scalar_luma(&src, &mut cur_scalar, qx, qy, w, h);

                    let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
                    mc_luma(&src, &mut cur_simd, qx, qy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_luma mismatch at {w}x{h} with qpos=({qx}, {qy})"
                    );
                }
            }
        }
    }
    /// The same kernels reached through the shared cursor, whose rows arrive by
    /// value: the encoder's reference picture is one of these, and it is what the
    /// half-pel refinement filters read at `kiW + 1` by `kiH + 1`.
    #[test]
    fn mc_parity_through_the_shared_cursor() {
        let mut base = filled_plane();
        let src_c = 10 * STRIDE + 10;
        let dst_c = 20 * STRIDE + 10;
        let mut want = vec![0u8; STRIDE * ROWS];
        let mut got = vec![0u8; STRIDE * ROWS];
        macro_rules! pair {
            ($scalar:expr, $simd:expr) => {{
                want.iter_mut().for_each(|b| *b = 0);
                got.iter_mut().for_each(|b| *b = 0);
                {
                    let src = PlaneCursor::new(&base, src_c, STRIDE);
                    let mut d = PlaneCursorMut::new(&mut want, dst_c, STRIDE);
                    #[allow(clippy::redundant_closure_call)]
                    ($scalar)(&src, &mut d);
                }
                {
                    let src = RecCursor::over_owned(&mut base, src_c, STRIDE);
                    let mut d = PlaneCursorMut::new(&mut got, dst_c, STRIDE);
                    #[allow(clippy::redundant_closure_call)]
                    ($simd)(&src, &mut d);
                }
                assert_eq!(want, got);
            }};
        }
        for (qx, qy) in [
            (0i16, 0i16),
            (1, 0),
            (2, 0),
            (3, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (1, 1),
            (2, 2),
            (3, 3),
        ] {
            for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4)] {
                pair!(
                    |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_luma(
                        s, d, qx, qy, w, h
                    ),
                    |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_luma(s, d, qx, qy, w, h)
                );
            }
        }
        // The half-pel refinement shapes, which only ever run over this cursor.
        for (w, h) in [(17, 16), (17, 8), (9, 16), (9, 8)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver20(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver20(s, d, w, h)
            );
        }
        for (w, h) in [(16, 17), (16, 9), (8, 17), (8, 9)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver02(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver02(s, d, w, h)
            );
        }
        for (w, h) in [(17, 17), (17, 9), (9, 17), (9, 9)] {
            pair!(
                |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| scalar_hor_ver22(s, d, w, h),
                |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_hor_ver22(s, d, w, h)
            );
        }
        for (w, h) in [(8, 8), (8, 4), (4, 8), (4, 4), (4, 2), (2, 4), (2, 2)] {
            for (mx, my) in [(0i16, 0i16), (3, 5), (1, 0), (0, 7)] {
                pair!(
                    |s: &PlaneCursor<'_>, d: &mut PlaneCursorMut<'_>| {
                        if (mx & 7) == 0 && (my & 7) == 0 {
                            mc_copy(s, d, w, h)
                        } else {
                            mc_chroma_with_frag_mv(s, d, mx, my, w, h)
                        }
                    },
                    |s: &RecCursor<'_>, d: &mut PlaneCursorMut<'_>| mc_chroma(s, d, mx, my, w, h)
                );
            }
        }
        // `pixel_avg`'s second operand is the reference block itself in four of the
        // quarter-pixel candidates, so it too sees this cursor.
        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8)] {
            let a = filled_plane();
            want.iter_mut().for_each(|b| *b = 0);
            got.iter_mut().for_each(|b| *b = 0);
            {
                let ca = PlaneCursor::new(&a, src_c, STRIDE);
                let cb = PlaneCursor::new(&base, src_c, STRIDE);
                scalar_pixel_avg(
                    &mut PlaneCursorMut::new(&mut want, dst_c, STRIDE),
                    &ca,
                    &cb,
                    w,
                    h,
                );
            }
            {
                let ca = PlaneCursor::new(&a, src_c, STRIDE);
                let cb = RecCursor::over_owned(&mut base, src_c, STRIDE);
                pixel_avg(
                    &mut PlaneCursorMut::new(&mut got, dst_c, STRIDE),
                    &ca,
                    &cb,
                    w,
                    h,
                );
            }
            assert_eq!(want, got, "pixel_avg via RecCursor at {w}x{h}");
        }
    }

    /// The `_AVERAGE_WITH_` forms of the two direct filters agree with the composites
    /// they would replace at quarter-pel `(1, 0)`, `(3, 0)`, `(0, 1)` and `(0, 3)`: the
    /// fused form is a rounded average against one of the filter's own taps, which is
    /// what the composite computes with an averaging pass.
    #[test]
    fn the_fused_quarter_pel_arms_agree_with_the_composites() {
        let base = filled_plane();
        let src = PlaneCursor::new(&base, 10 * STRIDE + 10, STRIDE);
        let dst_c = 20 * STRIDE + 10;
        for (qx, qy, avg) in [(1i16, 0i16, 2usize), (3, 0, 3), (0, 1, 2), (0, 3, 3)] {
            let mut want = vec![0u8; STRIDE * ROWS];
            let mut got = vec![0u8; STRIDE * ROWS];
            mc_luma(
                &src,
                &mut PlaneCursorMut::new(&mut want, dst_c, STRIDE),
                qx,
                qy,
                16,
                16,
            );
            {
                let mut d = PlaneCursorMut::new(&mut got, dst_c, STRIDE);
                match avg {
                    2 if qy == 0 => Sse2Leaves::hor::<_, 16, 21, 16, 2>(&src, &mut d),
                    3 if qy == 0 => Sse2Leaves::hor::<_, 16, 21, 16, 3>(&src, &mut d),
                    2 => Sse2Leaves::ver::<_, 16, 16, 21, 2>(&src, &mut d),
                    _ => Sse2Leaves::ver::<_, 16, 16, 21, 3>(&src, &mut d),
                }
            }
            assert_eq!(
                want, got,
                "fused quarter-pel ({qx}, {qy}) differs from the composite"
            );
        }
    }
}
