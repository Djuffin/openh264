//! SSE2 implementations of Motion Compensation (MC) kernels:
//! - Pixel averaging (`pixel_avg_sse2`)
//! - Chroma motion compensation (`mc_chroma_sse2`)
//! - Horizontal 6-tap Wiener filter (`mc_hor_ver20_sse2`)
//! - Vertical 6-tap Wiener filter (`mc_hor_ver02_sse2`)
//! - 2D center 6x6-tap Wiener filter (`mc_hor_ver22_sse2`)
//! - Luma quarter-pel motion compensation (`mc_luma_sse2`)
#![allow(unsafe_code)]

use core::arch::x86_64::*;
use crate::common::mc::{
    filter_input_8bit, g_kuiABCD, hor_filter_input_16bit, mc_copy, WelsClip1,
};
use crate::safe::plane::{PlaneCursor, PlaneCursorMut, RefSamples};

// ============================================================================
// Pixel Averaging (SSE2)
// ============================================================================

/// Rounded pixel average of two surfaces: `((a + b + 1) >> 1) as u8`.
///
/// Matches `_mm_avg_epu8` (`pavgb`) exactly.
#[target_feature(enable = "sse2")]
unsafe fn pixel_avg_row_sse2(
    dst: *mut u8,
    a: *const u8,
    b: *const u8,
    width: usize,
) {
    unsafe {
        let mut x = 0;
        while x + 16 <= width {
            let va = _mm_loadu_si128(a.add(x) as *const __m128i);
            let vb = _mm_loadu_si128(b.add(x) as *const __m128i);
            let avg = _mm_avg_epu8(va, vb);
            _mm_storeu_si128(dst.add(x) as *mut __m128i, avg);
            x += 16;
        }
        if x + 8 <= width {
            let va = _mm_loadl_epi64(a.add(x) as *const __m128i);
            let vb = _mm_loadl_epi64(b.add(x) as *const __m128i);
            let avg = _mm_avg_epu8(va, vb);
            _mm_storel_epi64(dst.add(x) as *mut __m128i, avg);
            x += 8;
        }
        if x + 4 <= width {
            let va = _mm_cvtsi32_si128((a.add(x) as *const i32).read_unaligned());
            let vb = _mm_cvtsi32_si128((b.add(x) as *const i32).read_unaligned());
            let avg = _mm_avg_epu8(va, vb);
            (dst.add(x) as *mut i32).write_unaligned(_mm_cvtsi128_si32(avg));
            x += 4;
        }
        while x < width {
            let pa = *a.add(x);
            let pb = *b.add(x);
            *dst.add(x) = (((pa as u32) + (pb as u32) + 1) >> 1) as u8;
            x += 1;
        }
    }
}

/// Public safe entry point for SSE2 pixel averaging.
pub fn pixel_avg_sse2<A: RefSamples, B: RefSamples>(
    dst: &mut PlaneCursorMut<'_>,
    a: &A,
    b: &B,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let ra = a.row_view(dy, 0, width);
        let rb = b.row_view(dy, 0, width);
        let out = dst.row_mut(dy, 0, width);
        unsafe {
            pixel_avg_row_sse2(out.as_mut_ptr(), ra.as_ptr(), rb.as_ptr(), width);
        }
    }
}

// ============================================================================
// Chroma MC (SSE2)
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn mc_chroma_row_w8_sse2(
    dst: *mut u8,
    r0: *const u8,
    r1: *const u8,
    vA: __m128i,
    vB: __m128i,
    vC: __m128i,
    vD: __m128i,
) {
    unsafe {
        let zero = _mm_setzero_si128();
        let v32 = _mm_set1_epi16(32);

        let r0_0 = _mm_loadl_epi64(r0 as *const __m128i);
        let r0_1 = _mm_loadl_epi64(r0.add(1) as *const __m128i);
        let r1_0 = _mm_loadl_epi64(r1 as *const __m128i);
        let r1_1 = _mm_loadl_epi64(r1.add(1) as *const __m128i);

        let r0_lo = _mm_unpacklo_epi8(r0_0, zero);
        let r0_hi = _mm_unpacklo_epi8(r0_1, zero);
        let r1_lo = _mm_unpacklo_epi8(r1_0, zero);
        let r1_hi = _mm_unpacklo_epi8(r1_1, zero);

        let s0 = _mm_mullo_epi16(r0_lo, vA);
        let s1 = _mm_mullo_epi16(r0_hi, vB);
        let s2 = _mm_mullo_epi16(r1_lo, vC);
        let s3 = _mm_mullo_epi16(r1_hi, vD);

        let sum = _mm_add_epi16(_mm_add_epi16(s0, s1), _mm_add_epi16(s2, s3));
        let rounded = _mm_add_epi16(sum, v32);
        let shifted = _mm_srli_epi16(rounded, 6);
        let packed = _mm_packus_epi16(shifted, zero);
        _mm_storel_epi64(dst as *mut __m128i, packed);
    }
}

#[target_feature(enable = "sse2")]
unsafe fn mc_chroma_row_w4_sse2(
    dst: *mut u8,
    r0: *const u8,
    r1: *const u8,
    vA: __m128i,
    vB: __m128i,
    vC: __m128i,
    vD: __m128i,
) {
    unsafe {
        let zero = _mm_setzero_si128();
        let v32 = _mm_set1_epi16(32);

        let r0_0 = _mm_cvtsi32_si128((r0 as *const i32).read_unaligned());
        let r0_1 = _mm_cvtsi32_si128((r0.add(1) as *const i32).read_unaligned());
        let r1_0 = _mm_cvtsi32_si128((r1 as *const i32).read_unaligned());
        let r1_1 = _mm_cvtsi32_si128((r1.add(1) as *const i32).read_unaligned());

        let r0_lo = _mm_unpacklo_epi8(r0_0, zero);
        let r0_hi = _mm_unpacklo_epi8(r0_1, zero);
        let r1_lo = _mm_unpacklo_epi8(r1_0, zero);
        let r1_hi = _mm_unpacklo_epi8(r1_1, zero);

        let s0 = _mm_mullo_epi16(r0_lo, vA);
        let s1 = _mm_mullo_epi16(r0_hi, vB);
        let s2 = _mm_mullo_epi16(r1_lo, vC);
        let s3 = _mm_mullo_epi16(r1_hi, vD);

        let sum = _mm_add_epi16(_mm_add_epi16(s0, s1), _mm_add_epi16(s2, s3));
        let rounded = _mm_add_epi16(sum, v32);
        let shifted = _mm_srli_epi16(rounded, 6);
        let packed = _mm_packus_epi16(shifted, zero);
        (dst as *mut i32).write_unaligned(_mm_cvtsi128_si32(packed));
    }
}

/// Public safe entry point for SSE2 chroma MC.
pub fn mc_chroma_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    if (mv_x & 0x07) == 0 && (mv_y & 0x07) == 0 {
        mc_copy(src, dst, width, height);
        return;
    }
    if width == 0 {
        return;
    }

    let pABCD = &g_kuiABCD[(mv_y & 0x07) as usize][(mv_x & 0x07) as usize];
    let iA = pABCD[0] as i16;
    let iB = pABCD[1] as i16;
    let iC = pABCD[2] as i16;
    let iD = pABCD[3] as i16;

    if width == 8 {
        unsafe {
            let vA = _mm_set1_epi16(iA);
            let vB = _mm_set1_epi16(iB);
            let vC = _mm_set1_epi16(iC);
            let vD = _mm_set1_epi16(iD);
            for dy in 0..height as isize {
                let r0 = src.row_view(dy, 0, 9);
                let r1 = src.row_view(dy + 1, 0, 9);
                let out = dst.row_mut(dy, 0, 8);
                mc_chroma_row_w8_sse2(
                    out.as_mut_ptr(),
                    r0.as_ptr(),
                    r1.as_ptr(),
                    vA,
                    vB,
                    vC,
                    vD,
                );
            }
        }
    } else if width == 4 {
        unsafe {
            let vA = _mm_set1_epi16(iA);
            let vB = _mm_set1_epi16(iB);
            let vC = _mm_set1_epi16(iC);
            let vD = _mm_set1_epi16(iD);
            for dy in 0..height as isize {
                let r0 = src.row_view(dy, 0, 5);
                let r1 = src.row_view(dy + 1, 0, 5);
                let out = dst.row_mut(dy, 0, 4);
                mc_chroma_row_w4_sse2(
                    out.as_mut_ptr(),
                    r0.as_ptr(),
                    r1.as_ptr(),
                    vA,
                    vB,
                    vC,
                    vD,
                );
            }
        }
    } else {
        // Scalar fallback for width 2 or arbitrary widths
        for dy in 0..height as isize {
            let r0 = src.row_view(dy, 0, width + 1);
            let r1 = src.row_view(dy + 1, 0, width + 1);
            let out = dst.row_mut(dy, 0, width);
            for j in 0..width {
                out[j] = (((iA as i32) * (r0[j] as i32)
                    + (iB as i32) * (r0[j + 1] as i32)
                    + (iC as i32) * (r1[j] as i32)
                    + (iD as i32) * (r1[j + 1] as i32)
                    + 32)
                    >> 6) as u8;
            }
        }
    }
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
#[target_feature(enable = "sse2")]
unsafe fn filter_6tap_8_samples(
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
#[target_feature(enable = "sse2")]
unsafe fn filter_6tap_intermediate_8_samples(
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

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver20_chunk8_sse2(out: *mut u8, row: *const u8) {
    unsafe {
        let zero = _mm_setzero_si128();
        let p0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row as *const __m128i), zero);
        let p1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.add(1) as *const __m128i), zero);
        let p2 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.add(2) as *const __m128i), zero);
        let p3 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.add(3) as *const __m128i), zero);
        let p4 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.add(4) as *const __m128i), zero);
        let p5 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.add(5) as *const __m128i), zero);

        let res = filter_6tap_8_samples(p0, p1, p2, p3, p4, p5);
        _mm_storel_epi64(out as *mut __m128i, res);
    }
}

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver20_chunk4_sse2(out: *mut u8, row: *const u8) {
    unsafe {
        let zero = _mm_setzero_si128();
        let p0 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row as *const i32).read_unaligned()), zero);
        let p1 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.add(1) as *const i32).read_unaligned()), zero);
        let p2 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.add(2) as *const i32).read_unaligned()), zero);
        let p3 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.add(3) as *const i32).read_unaligned()), zero);
        let p4 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.add(4) as *const i32).read_unaligned()), zero);
        let p5 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.add(5) as *const i32).read_unaligned()), zero);

        let res = filter_6tap_8_samples(p0, p1, p2, p3, p4, p5);
        (out as *mut i32).write_unaligned(_mm_cvtsi128_si32(res));
    }
}

/// Public safe entry point for SSE2 horizontal half-pel filter.
pub fn mc_hor_ver20_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    for dy in 0..height as isize {
        let row = src.row_view(dy, -2, width + 5);
        let out = dst.row_mut(dy, 0, width);
        let row_ptr = row.as_ptr();
        let out_ptr = out.as_mut_ptr();

        let mut col = 0;
        while col + 8 <= width {
            unsafe {
                mc_hor_ver20_chunk8_sse2(out_ptr.add(col), row_ptr.add(col));
            }
            col += 8;
        }
        if col + 4 <= width {
            unsafe {
                mc_hor_ver20_chunk4_sse2(out_ptr.add(col), row_ptr.add(col));
            }
            col += 4;
        }
        while col < width {
            let w = [
                row[col],
                row[col + 1],
                row[col + 2],
                row[col + 3],
                row[col + 4],
                row[col + 5],
            ];
            out[col] = WelsClip1((filter_input_8bit(&w) + 16) >> 5);
            col += 1;
        }
    }
}

// ============================================================================
// Vertical 6-Tap Filter: McHorVer02 (SSE2)
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver02_w16_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    unsafe {
        let zero = _mm_setzero_si128();

        let row_m2 = src.row_view(-2, 0, 16);
        let mut r0_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m2.as_ptr() as *const __m128i), zero);
        let mut r0_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m2.as_ptr().add(8) as *const __m128i), zero);

        let row_m1 = src.row_view(-1, 0, 16);
        let mut r1_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m1.as_ptr() as *const __m128i), zero);
        let mut r1_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m1.as_ptr().add(8) as *const __m128i), zero);

        let row_0 = src.row_view(0, 0, 16);
        let mut r2_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_0.as_ptr() as *const __m128i), zero);
        let mut r2_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_0.as_ptr().add(8) as *const __m128i), zero);

        let row_1 = src.row_view(1, 0, 16);
        let mut r3_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_1.as_ptr() as *const __m128i), zero);
        let mut r3_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_1.as_ptr().add(8) as *const __m128i), zero);

        let row_2 = src.row_view(2, 0, 16);
        let mut r4_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_2.as_ptr() as *const __m128i), zero);
        let mut r4_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_2.as_ptr().add(8) as *const __m128i), zero);

        for dy in 0..height as isize {
            let row = src.row_view(dy + 3, 0, 16);
            let r5_0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.as_ptr() as *const __m128i), zero);
            let r5_1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.as_ptr().add(8) as *const __m128i), zero);

            let out0 = filter_6tap_8_samples(r0_0, r1_0, r2_0, r3_0, r4_0, r5_0);
            let out1 = filter_6tap_8_samples(r0_1, r1_1, r2_1, r3_1, r4_1, r5_1);

            let out = dst.row_mut(dy, 0, 16);
            _mm_storel_epi64(out.as_mut_ptr() as *mut __m128i, out0);
            _mm_storel_epi64(out.as_mut_ptr().add(8) as *mut __m128i, out1);

            r0_0 = r1_0; r1_0 = r2_0; r2_0 = r3_0; r3_0 = r4_0; r4_0 = r5_0;
            r0_1 = r1_1; r1_1 = r2_1; r2_1 = r3_1; r3_1 = r4_1; r4_1 = r5_1;
        }
    }
}

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver02_w8_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    unsafe {
        let zero = _mm_setzero_si128();

        let row_m2 = src.row_view(-2, 0, 8);
        let mut r0 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m2.as_ptr() as *const __m128i), zero);

        let row_m1 = src.row_view(-1, 0, 8);
        let mut r1 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_m1.as_ptr() as *const __m128i), zero);

        let row_0 = src.row_view(0, 0, 8);
        let mut r2 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_0.as_ptr() as *const __m128i), zero);

        let row_1 = src.row_view(1, 0, 8);
        let mut r3 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_1.as_ptr() as *const __m128i), zero);

        let row_2 = src.row_view(2, 0, 8);
        let mut r4 = _mm_unpacklo_epi8(_mm_loadl_epi64(row_2.as_ptr() as *const __m128i), zero);

        for dy in 0..height as isize {
            let row = src.row_view(dy + 3, 0, 8);
            let r5 = _mm_unpacklo_epi8(_mm_loadl_epi64(row.as_ptr() as *const __m128i), zero);

            let out_v = filter_6tap_8_samples(r0, r1, r2, r3, r4, r5);

            let out = dst.row_mut(dy, 0, 8);
            _mm_storel_epi64(out.as_mut_ptr() as *mut __m128i, out_v);

            r0 = r1; r1 = r2; r2 = r3; r3 = r4; r4 = r5;
        }
    }
}

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver02_w4_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    height: usize,
) {
    unsafe {
        let zero = _mm_setzero_si128();

        let row_m2 = src.row_view(-2, 0, 4);
        let mut r0 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row_m2.as_ptr() as *const i32).read_unaligned()), zero);

        let row_m1 = src.row_view(-1, 0, 4);
        let mut r1 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row_m1.as_ptr() as *const i32).read_unaligned()), zero);

        let row_0 = src.row_view(0, 0, 4);
        let mut r2 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row_0.as_ptr() as *const i32).read_unaligned()), zero);

        let row_1 = src.row_view(1, 0, 4);
        let mut r3 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row_1.as_ptr() as *const i32).read_unaligned()), zero);

        let row_2 = src.row_view(2, 0, 4);
        let mut r4 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row_2.as_ptr() as *const i32).read_unaligned()), zero);

        for dy in 0..height as isize {
            let row = src.row_view(dy + 3, 0, 4);
            let r5 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((row.as_ptr() as *const i32).read_unaligned()), zero);

            let out_v = filter_6tap_8_samples(r0, r1, r2, r3, r4, r5);

            let out = dst.row_mut(dy, 0, 4);
            (out.as_mut_ptr() as *mut i32).write_unaligned(_mm_cvtsi128_si32(out_v));

            r0 = r1; r1 = r2; r2 = r3; r3 = r4; r4 = r5;
        }
    }
}

/// Public safe entry point for SSE2 vertical half-pel filter.
pub fn mc_hor_ver02_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    if width == 16 {
        unsafe {
            mc_hor_ver02_w16_sse2(src, dst, height);
        }
    } else if width == 8 {
        unsafe {
            mc_hor_ver02_w8_sse2(src, dst, height);
        }
    } else if width == 4 {
        unsafe {
            mc_hor_ver02_w4_sse2(src, dst, height);
        }
    } else {
        // Scalar fallback for non-standard widths
        let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
            src.row_view(-2, 0, width),
            src.row_view(-1, 0, width),
            src.row_view(0, 0, width),
            src.row_view(1, 0, width),
            src.row_view(2, 0, width),
        );
        for dy in 0..height as isize {
            let r5 = src.row_view(dy + 3, 0, width);
            let out = dst.row_mut(dy, 0, width);
            for ((((((o, &a), &b), &c), &d), &e), &f) in out
                .iter_mut()
                .zip(r0.iter())
                .zip(r1.iter())
                .zip(r2.iter())
                .zip(r3.iter())
                .zip(r4.iter())
                .zip(r5.iter())
            {
                *o = WelsClip1((filter_input_8bit(&[a, b, c, d, e, f]) + 16) >> 5);
            }
            (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);
        }
    }
}

// ============================================================================
// 2D Center 6x6-Tap Filter: McHorVer22 (SSE2)
// ============================================================================

#[target_feature(enable = "sse2")]
unsafe fn mc_hor_ver22_inner_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    unsafe {
        let mut iTmp = [0i16; 17 + 5];
        let n = width + 5;
        let zero = _mm_setzero_si128();

        let (mut r0, mut r1, mut r2, mut r3, mut r4) = (
            src.row_view(-2, -2, n),
            src.row_view(-1, -2, n),
            src.row_view(0, -2, n),
            src.row_view(1, -2, n),
            src.row_view(2, -2, n),
        );

        for dy in 0..height as isize {
            let r5 = src.row_view(dy + 3, -2, n);

            // Step 1: Vertical 6-tap filter into iTmp
            let mut j = 0;
            while j + 8 <= n {
                let p0 = _mm_unpacklo_epi8(_mm_loadl_epi64(r0.as_ptr().add(j) as *const __m128i), zero);
                let p1 = _mm_unpacklo_epi8(_mm_loadl_epi64(r1.as_ptr().add(j) as *const __m128i), zero);
                let p2 = _mm_unpacklo_epi8(_mm_loadl_epi64(r2.as_ptr().add(j) as *const __m128i), zero);
                let p3 = _mm_unpacklo_epi8(_mm_loadl_epi64(r3.as_ptr().add(j) as *const __m128i), zero);
                let p4 = _mm_unpacklo_epi8(_mm_loadl_epi64(r4.as_ptr().add(j) as *const __m128i), zero);
                let p5 = _mm_unpacklo_epi8(_mm_loadl_epi64(r5.as_ptr().add(j) as *const __m128i), zero);

                let res = filter_6tap_intermediate_8_samples(p0, p1, p2, p3, p4, p5);
                _mm_storeu_si128(iTmp.as_mut_ptr().add(j) as *mut __m128i, res);
                j += 8;
            }
            if j + 4 <= n {
                let p0 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r0.as_ptr().add(j) as *const i32).read_unaligned()), zero);
                let p1 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r1.as_ptr().add(j) as *const i32).read_unaligned()), zero);
                let p2 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r2.as_ptr().add(j) as *const i32).read_unaligned()), zero);
                let p3 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r3.as_ptr().add(j) as *const i32).read_unaligned()), zero);
                let p4 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r4.as_ptr().add(j) as *const i32).read_unaligned()), zero);
                let p5 = _mm_unpacklo_epi8(_mm_cvtsi32_si128((r5.as_ptr().add(j) as *const i32).read_unaligned()), zero);

                let res = filter_6tap_intermediate_8_samples(p0, p1, p2, p3, p4, p5);
                _mm_storel_epi64(iTmp.as_mut_ptr().add(j) as *mut __m128i, res);
                j += 4;
            }
            while j < n {
                iTmp[j] = filter_input_8bit(&[r0[j], r1[j], r2[j], r3[j], r4[j], r5[j]]) as i16;
                j += 1;
            }

            (r0, r1, r2, r3, r4) = (r1, r2, r3, r4, r5);

            // Step 2: Horizontal 6-tap filter over 16-bit intermediate iTmp
            let out = dst.row_mut(dy, 0, width);
            for (o, w) in out.iter_mut().zip(iTmp[..n].windows(6)) {
                *o = WelsClip1((hor_filter_input_16bit(w.try_into().unwrap()) + 512) >> 10);
            }
        }
    }
}

/// Public safe entry point for SSE2 center half-pel filter.
pub fn mc_hor_ver22_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    unsafe {
        mc_hor_ver22_inner_sse2(src, dst, width, height);
    }
}

// ============================================================================
// Luma Quarter-Pel MC (SSE2)
// ============================================================================

#[inline(always)]
fn scratch() -> [u8; 256] {
    [0u8; 256]
}

#[inline(never)]
pub fn mc_hor_ver01_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02_sse2(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_sse2(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

#[inline(never)]
pub fn mc_hor_ver03_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver02_sse2(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &src.advance(0, 1),
        &PlaneCursor::new(&tmp, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver10_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut tmp = scratch();
    mc_hor_ver20_sse2(src, &mut PlaneCursorMut::new(&mut tmp, 0, 16), width, height);
    pixel_avg_sse2(dst, src, &PlaneCursor::new(&tmp, 0, 16), width, height);
}

#[inline(never)]
pub fn mc_hor_ver11_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_sse2(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02_sse2(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver12_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02_sse2(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    mc_hor_ver22_sse2(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver13_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_sse2(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02_sse2(src, &mut PlaneCursorMut::new(&mut ver, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver21_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20_sse2(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver22_sse2(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver23_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ctr = scratch();
    mc_hor_ver20_sse2(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver22_sse2(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver30_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    mc_hor_ver20_sse2(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &src.advance(1, 0),
        &PlaneCursor::new(&hor, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver31_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_sse2(src, &mut PlaneCursorMut::new(&mut hor, 0, 16), width, height);
    mc_hor_ver02_sse2(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver32_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut ver = scratch();
    let mut ctr = scratch();
    mc_hor_ver02_sse2(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    mc_hor_ver22_sse2(src, &mut PlaneCursorMut::new(&mut ctr, 0, 16), width, height);
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&ver, 0, 16),
        &PlaneCursor::new(&ctr, 0, 16),
        width,
        height,
    );
}

#[inline(never)]
pub fn mc_hor_ver33_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    width: usize,
    height: usize,
) {
    let mut hor = scratch();
    let mut ver = scratch();
    mc_hor_ver20_sse2(
        &src.advance(0, 1),
        &mut PlaneCursorMut::new(&mut hor, 0, 16),
        width,
        height,
    );
    mc_hor_ver02_sse2(
        &src.advance(1, 0),
        &mut PlaneCursorMut::new(&mut ver, 0, 16),
        width,
        height,
    );
    pixel_avg_sse2(
        dst,
        &PlaneCursor::new(&hor, 0, 16),
        &PlaneCursor::new(&ver, 0, 16),
        width,
        height,
    );
}

/// Public safe entry point for SSE2 luma quarter-pel MC.
pub fn mc_luma_sse2<S: RefSamples + Copy>(
    src: &S,
    dst: &mut PlaneCursorMut<'_>,
    mv_x: i16,
    mv_y: i16,
    width: usize,
    height: usize,
) {
    match ((mv_x & 0x03) as u8, (mv_y & 0x03) as u8) {
        (0, 0) => mc_copy(src, dst, width, height),
        (0, 1) => mc_hor_ver01_sse2(src, dst, width, height),
        (0, 2) => mc_hor_ver02_sse2(src, dst, width, height),
        (0, 3) => mc_hor_ver03_sse2(src, dst, width, height),
        (1, 0) => mc_hor_ver10_sse2(src, dst, width, height),
        (1, 1) => mc_hor_ver11_sse2(src, dst, width, height),
        (1, 2) => mc_hor_ver12_sse2(src, dst, width, height),
        (1, 3) => mc_hor_ver13_sse2(src, dst, width, height),
        (2, 0) => mc_hor_ver20_sse2(src, dst, width, height),
        (2, 1) => mc_hor_ver21_sse2(src, dst, width, height),
        (2, 2) => mc_hor_ver22_sse2(src, dst, width, height),
        (2, 3) => mc_hor_ver23_sse2(src, dst, width, height),
        (3, 0) => mc_hor_ver30_sse2(src, dst, width, height),
        (3, 1) => mc_hor_ver31_sse2(src, dst, width, height),
        (3, 2) => mc_hor_ver32_sse2(src, dst, width, height),
        _ => mc_hor_ver33_sse2(src, dst, width, height),
    }
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    // These MUST be the `_c` scalar kernels, not the same-named dispatchers:
    // the dispatchers route to the very SSE2 kernels under test, which would
    // make every assertion below a tautology.
    use crate::common::mc::{
        mc_chroma_with_frag_mv, mc_hor_ver02_c as scalar_hor_ver02,
        mc_hor_ver20_c as scalar_hor_ver20, mc_hor_ver22_c as scalar_hor_ver22,
        mc_luma_c as scalar_luma, pixel_avg_c as scalar_pixel_avg,
    };

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

        for (w, h) in [(16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4), (17, 16), (9, 8), (5, 4)] {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, 10 * STRIDE + 8, STRIDE);
            scalar_pixel_avg(&mut cur_scalar, &ca, &cb, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, 10 * STRIDE + 8, STRIDE);
            pixel_avg_sse2(&mut cur_simd, &ca, &cb, w, h);

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
                    mc_chroma_sse2(&src, &mut cur_simd, dx, dy, w, h);

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
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 16), (9, 8), (5, 4),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver20(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver20_sse2(&src, &mut cur_simd, w, h);

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
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (16, 17), (8, 9), (4, 5),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver02(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver02_sse2(&src, &mut cur_simd, w, h);

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
            (16, 16), (16, 8), (8, 16), (8, 8), (8, 4), (4, 8), (4, 4),
            (17, 17), (9, 9), (5, 5),
        ];

        for &(w, h) in &shapes {
            let mut dst_scalar = vec![0u8; STRIDE * ROWS];
            let mut dst_simd = vec![0u8; STRIDE * ROWS];

            let mut cur_scalar = PlaneCursorMut::new(&mut dst_scalar, dst_c, STRIDE);
            scalar_hor_ver22(&src, &mut cur_scalar, w, h);

            let mut cur_simd = PlaneCursorMut::new(&mut dst_simd, dst_c, STRIDE);
            mc_hor_ver22_sse2(&src, &mut cur_simd, w, h);

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
                    mc_luma_sse2(&src, &mut cur_simd, qx, qy, w, h);

                    assert_eq!(
                        dst_scalar, dst_simd,
                        "mc_luma mismatch at {w}x{h} with qpos=({qx}, {qy})"
                    );
                }
            }
        }
    }
}
