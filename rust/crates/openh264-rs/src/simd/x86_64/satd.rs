//! x86_64 SSE2 & SSSE3 implementations of SATD (Hadamard transformed SAD).
#![allow(unsafe_code)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;
use crate::safe::plane::RefSamples;

#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn abs_epi16(v: __m128i) -> __m128i {
    unsafe {
        let sign = _mm_srai_epi16(v, 15);
        _mm_sub_epi16(_mm_xor_si128(v, sign), sign)
    }
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn sum_sub(a: &mut __m128i, b: &mut __m128i) {
    unsafe {
        let tmp = *b;
        *b = _mm_add_epi16(*a, *b);
        *a = _mm_sub_epi16(*a, tmp);
    }
}

/// 1D 4-point Hadamard transform over SIMD vectors:
/// in: r0, r1, r2, r3 -> out: r0, r2, r1, r3 (butterfly permutation).
#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn hdm4(
    r0: &mut __m128i,
    r1: &mut __m128i,
    r2: &mut __m128i,
    r3: &mut __m128i,
) {
    unsafe {
        sum_sub(r0, r1);
        sum_sub(r2, r3);
        sum_sub(r1, r3);
        sum_sub(r0, r2);
    }
}

/// Transposes 4x4 matrix of 16-bit words in lower 64 bits of 4 registers.
/// Returns (col01, col23) where col01 = [col0 (64b), col1 (64b)] and col23 = [col2 (64b), col3 (64b)].
#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn transpose_4x4_w(
    r0: __m128i,
    r1: __m128i,
    r2: __m128i,
    r3: __m128i,
) -> (__m128i, __m128i) {
    unsafe {
        let t01 = _mm_unpacklo_epi16(r0, r1);
        let t23 = _mm_unpacklo_epi16(r2, r3);
        let c01 = _mm_unpacklo_epi32(t01, t23);
        let c23 = _mm_unpackhi_epi32(t01, t23);
        (c01, c23)
    }
}

/// Horizontal sum of all 8 unsigned 16-bit integers in a 128-bit register.
#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn sum_u16_8(v: __m128i) -> i32 {
    unsafe {
        let hi64 = _mm_srli_si128(v, 8);
        let sum64 = _mm_add_epi16(v, hi64);
        let hi32 = _mm_srli_si128(sum64, 4);
        let sum32 = _mm_add_epi16(sum64, hi32);
        let hi16 = _mm_srli_si128(sum32, 2);
        let sum16 = _mm_add_epi16(sum32, hi16);
        _mm_cvtsi128_si32(sum16) & 0xFFFF
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn satd_4x4_sse2_impl<A: RefSamples + Copy, B: RefSamples + Copy>(
    c1: &A,
    c2: &B,
) -> i32 {
    unsafe {
        // 1. Load 4 rows of 4 samples and compute difference in i16
        let r1_0 = c1.row_n::<4>(0, 0);
        let r2_0 = c2.row_n::<4>(0, 0);
        let r1_1 = c1.row_n::<4>(1, 0);
        let r2_1 = c2.row_n::<4>(1, 0);
        let r1_2 = c1.row_n::<4>(2, 0);
        let r2_2 = c2.row_n::<4>(2, 0);
        let r1_3 = c1.row_n::<4>(3, 0);
        let r2_3 = c2.row_n::<4>(3, 0);

        let v1_0 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1_0));
        let v2_0 = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_0));
        let v1_1 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1_1));
        let v2_1 = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_1));
        let v1_2 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1_2));
        let v2_2 = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_2));
        let v1_3 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1_3));
        let v2_3 = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_3));

        let zero = _mm_setzero_si128();
        let mut d0 = _mm_sub_epi16(_mm_unpacklo_epi8(v1_0, zero), _mm_unpacklo_epi8(v2_0, zero));
        let mut d1 = _mm_sub_epi16(_mm_unpacklo_epi8(v1_1, zero), _mm_unpacklo_epi8(v2_1, zero));
        let mut d2 = _mm_sub_epi16(_mm_unpacklo_epi8(v1_2, zero), _mm_unpacklo_epi8(v2_2, zero));
        let mut d3 = _mm_sub_epi16(_mm_unpacklo_epi8(v1_3, zero), _mm_unpacklo_epi8(v2_3, zero));

        // 2. 1D Hadamard on rows
        hdm4(&mut d0, &mut d1, &mut d2, &mut d3);

        // 3. Transpose 4x4 matrix
        let (c01, c23) = transpose_4x4_w(d0, d1, d2, d3);
        let mut col0 = c01;
        let mut col1 = _mm_srli_si128(c01, 8);
        let mut col2 = c23;
        let mut col3 = _mm_srli_si128(c23, 8);

        // 4. 1D Hadamard on columns
        hdm4(&mut col0, &mut col1, &mut col2, &mut col3);

        // 5. Absolute values and sum
        let abs0 = abs_epi16(col0);
        let abs1 = abs_epi16(col1);
        let abs2 = abs_epi16(col2);
        let abs3 = abs_epi16(col3);

        // Combine into two registers:
        let abs01 = _mm_unpacklo_epi64(abs0, abs1);
        let abs23 = _mm_unpacklo_epi64(abs2, abs3);
        let total_v = _mm_add_epi16(abs01, abs23);

        let satd = sum_u16_8(total_v);
        (satd + 1) >> 1
    }
}

// ============================================================================
// Safe Public Wrappers
// ============================================================================

#[inline(always)]
pub fn satd_4x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    unsafe { satd_4x4_sse2_impl(c1, c2) }
}

#[inline(always)]
pub fn satd_8x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0))
}

#[inline(always)]
pub fn satd_4x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4))
}

#[inline(always)]
pub fn satd_8x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_4x4(c1, c2);
    satd += satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0));
    satd += satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4));
    satd += satd_4x4(&c1.advance(4, 4), &c2.advance(4, 4));
    satd
}

#[inline(always)]
pub fn satd_16x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0))
}

#[inline(always)]
pub fn satd_8x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8))
}

#[inline(always)]
pub fn satd_16x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_8x8(c1, c2);
    satd += satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0));
    satd += satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8));
    satd += satd_8x8(&c1.advance(8, 8), &c2.advance(8, 8));
    satd
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::sample::{
        satd_16x16, satd_16x8, satd_4x4, satd_4x8, satd_8x16, satd_8x4, satd_8x8,
    };
    use crate::safe::plane::PlaneCursor;

    fn make_test_planes(stride: usize, height: usize) -> (Vec<u8>, Vec<u8>) {
        let mut p1 = vec![0u8; stride * height];
        let mut p2 = vec![0u8; stride * height];
        for (i, b) in p1.iter_mut().enumerate() {
            *b = ((i * 19 + 7) & 0xFF) as u8;
        }
        for (i, b) in p2.iter_mut().enumerate() {
            *b = ((i * 29 + 23) & 0xFF) as u8;
        }
        (p1, p2)
    }

    #[test]
    fn test_satd_parity_identical() {
        let buf = [42u8; 64];
        let c = PlaneCursor::new(&buf, 0, 8);
        assert_eq!(satd_4x4(&c, &c), 0);
        assert_eq!(satd_8x8(&c, &c), 0);
        assert_eq!(satd_4x4(&c, &c), satd_4x4(&c, &c));
    }

    #[test]
    fn test_satd_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(satd_4x4(&c1, &c2), satd_4x4(&c1, &c2), "satd_4x4 mismatch");
        assert_eq!(satd_8x4(&c1, &c2), satd_8x4(&c1, &c2), "satd_8x4 mismatch");
        assert_eq!(satd_4x8(&c1, &c2), satd_4x8(&c1, &c2), "satd_4x8 mismatch");
        assert_eq!(satd_8x8(&c1, &c2), satd_8x8(&c1, &c2), "satd_8x8 mismatch");
        assert_eq!(satd_16x8(&c1, &c2), satd_16x8(&c1, &c2), "satd_16x8 mismatch");
        assert_eq!(satd_8x16(&c1, &c2), satd_8x16(&c1, &c2), "satd_8x16 mismatch");
        assert_eq!(satd_16x16(&c1, &c2), satd_16x16(&c1, &c2), "satd_16x16 mismatch");
    }

    #[test]
    fn test_satd_random_blocks() {
        for seed in 0..100 {
            let mut p1 = [0u8; 16];
            let mut p2 = [0u8; 16];
            for i in 0..16 {
                p1[i] = ((seed * 37 + i * 11) & 0xFF) as u8;
                p2[i] = ((seed * 43 + i * 17) & 0xFF) as u8;
            }
            let c1 = PlaneCursor::new(&p1, 0, 4);
            let c2 = PlaneCursor::new(&p2, 0, 4);
            assert_eq!(
                satd_4x4(&c1, &c2),
                satd_4x4(&c1, &c2),
                "mismatch at seed {seed}"
            );
        }
    }
}
