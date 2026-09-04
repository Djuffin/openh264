//! x86_64 SSE2 & AVX2 implementations of SAD and 4-point SAD kernels.
#![allow(unsafe_code)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;
use crate::safe::plane::RefSamples;

// ============================================================================
// Internal SSE2 Kernels
// ============================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sad_16x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    dx: isize,
    dy: isize,
) -> i32 {
    unsafe {
        let mut acc = _mm_setzero_si128();
        for y in 0..H {
            let r1 = sample1.row_n::<16>(y as isize, 0);
            let r2 = sample2.row_n::<16>(y as isize + dy, dx);
            let v1 = _mm_loadu_si128(r1.as_ptr() as *const __m128i);
            let v2 = _mm_loadu_si128(r2.as_ptr() as *const __m128i);
            acc = _mm_add_epi64(acc, _mm_sad_epu8(v1, v2));
        }
        let hi = _mm_srli_si128(acc, 8);
        let sum = _mm_add_epi32(acc, hi);
        _mm_cvtsi128_si32(sum)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn sad_16x_avx2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    dx: isize,
    dy: isize,
) -> i32 {
    unsafe {
        let mut acc = _mm256_setzero_si256();
        let mut y = 0;
        while y < H {
            let r1_0 = sample1.row_n::<16>(y as isize, 0);
            let r2_0 = sample2.row_n::<16>(y as isize + dy, dx);
            let r1_1 = sample1.row_n::<16>((y + 1) as isize, 0);
            let r2_1 = sample2.row_n::<16>((y + 1) as isize + dy, dx);

            let v1_0 = _mm_loadu_si128(r1_0.as_ptr() as *const __m128i);
            let v2_0 = _mm_loadu_si128(r2_0.as_ptr() as *const __m128i);
            let v1_1 = _mm_loadu_si128(r1_1.as_ptr() as *const __m128i);
            let v2_1 = _mm_loadu_si128(r2_1.as_ptr() as *const __m128i);

            let v1 = _mm256_set_m128i(v1_1, v1_0);
            let v2 = _mm256_set_m128i(v2_1, v2_0);

            acc = _mm256_add_epi64(acc, _mm256_sad_epu8(v1, v2));
            y += 2;
        }
        let lo = _mm256_castsi256_si128(acc);
        let hi = _mm256_extracti128_si256(acc, 1);
        let sum128 = _mm_add_epi64(lo, hi);
        let sum_hi = _mm_srli_si128(sum128, 8);
        let total = _mm_add_epi32(sum128, sum_hi);
        _mm_cvtsi128_si32(total)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sad_8x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    dx: isize,
    dy: isize,
) -> i32 {
    unsafe {
        let mut acc = _mm_setzero_si128();
        for y in 0..H {
            let r1 = sample1.row_n::<8>(y as isize, 0);
            let r2 = sample2.row_n::<8>(y as isize + dy, dx);
            let v1 = _mm_loadl_epi64(r1.as_ptr() as *const __m128i);
            let v2 = _mm_loadl_epi64(r2.as_ptr() as *const __m128i);
            acc = _mm_add_epi64(acc, _mm_sad_epu8(v1, v2));
        }
        _mm_cvtsi128_si32(acc)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sad_4x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    dx: isize,
    dy: isize,
) -> i32 {
    let mut acc = _mm_setzero_si128();
    for y in 0..H {
        let r1 = sample1.row_n::<4>(y as isize, 0);
        let r2 = sample2.row_n::<4>(y as isize + dy, dx);
        let v1 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1));
        let v2 = _mm_cvtsi32_si128(i32::from_ne_bytes(r2));
        acc = _mm_add_epi64(acc, _mm_sad_epu8(v1, v2));
    }
    _mm_cvtsi128_si32(acc)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sample_sad_four_16x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    unsafe {
        let mut acc0 = _mm_setzero_si128();
        let mut acc1 = _mm_setzero_si128();
        let mut acc2 = _mm_setzero_si128();
        let mut acc3 = _mm_setzero_si128();

        for y in 0..H {
            let y_isize = y as isize;
            let r1 = sample1.row_n::<16>(y_isize, 0);
            let v1 = _mm_loadu_si128(r1.as_ptr() as *const __m128i);

            let r2_up = sample2.row_n::<16>(y_isize - 1, 0);
            let r2_dn = sample2.row_n::<16>(y_isize + 1, 0);
            let r2_lt = sample2.row_n::<16>(y_isize, -1);
            let r2_rt = sample2.row_n::<16>(y_isize, 1);

            let v2_up = _mm_loadu_si128(r2_up.as_ptr() as *const __m128i);
            let v2_dn = _mm_loadu_si128(r2_dn.as_ptr() as *const __m128i);
            let v2_lt = _mm_loadu_si128(r2_lt.as_ptr() as *const __m128i);
            let v2_rt = _mm_loadu_si128(r2_rt.as_ptr() as *const __m128i);

            acc0 = _mm_add_epi64(acc0, _mm_sad_epu8(v1, v2_up));
            acc1 = _mm_add_epi64(acc1, _mm_sad_epu8(v1, v2_dn));
            acc2 = _mm_add_epi64(acc2, _mm_sad_epu8(v1, v2_lt));
            acc3 = _mm_add_epi64(acc3, _mm_sad_epu8(v1, v2_rt));
        }

        #[inline(always)]
        unsafe fn reduce16(acc: __m128i) -> i32 {
            unsafe {
                let hi = _mm_srli_si128(acc, 8);
                let sum = _mm_add_epi32(acc, hi);
                _mm_cvtsi128_si32(sum)
            }
        }

        sad[0] = reduce16(acc0);
        sad[1] = reduce16(acc1);
        sad[2] = reduce16(acc2);
        sad[3] = reduce16(acc3);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sample_sad_four_8x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    unsafe {
        let mut acc0 = _mm_setzero_si128();
        let mut acc1 = _mm_setzero_si128();
        let mut acc2 = _mm_setzero_si128();
        let mut acc3 = _mm_setzero_si128();

        for y in 0..H {
            let y_isize = y as isize;
            let r1 = sample1.row_n::<8>(y_isize, 0);
            let v1 = _mm_loadl_epi64(r1.as_ptr() as *const __m128i);

            let r2_up = sample2.row_n::<8>(y_isize - 1, 0);
            let r2_dn = sample2.row_n::<8>(y_isize + 1, 0);
            let r2_lt = sample2.row_n::<8>(y_isize, -1);
            let r2_rt = sample2.row_n::<8>(y_isize, 1);

            let v2_up = _mm_loadl_epi64(r2_up.as_ptr() as *const __m128i);
            let v2_dn = _mm_loadl_epi64(r2_dn.as_ptr() as *const __m128i);
            let v2_lt = _mm_loadl_epi64(r2_lt.as_ptr() as *const __m128i);
            let v2_rt = _mm_loadl_epi64(r2_rt.as_ptr() as *const __m128i);

            acc0 = _mm_add_epi64(acc0, _mm_sad_epu8(v1, v2_up));
            acc1 = _mm_add_epi64(acc1, _mm_sad_epu8(v1, v2_dn));
            acc2 = _mm_add_epi64(acc2, _mm_sad_epu8(v1, v2_lt));
            acc3 = _mm_add_epi64(acc3, _mm_sad_epu8(v1, v2_rt));
        }

        sad[0] = _mm_cvtsi128_si32(acc0);
        sad[1] = _mm_cvtsi128_si32(acc1);
        sad[2] = _mm_cvtsi128_si32(acc2);
        sad[3] = _mm_cvtsi128_si32(acc3);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub unsafe fn sample_sad_four_4x_sse2<S: RefSamples, const H: usize>(
    sample1: &S,
    sample2: &S,
    sad: &mut [i32; 4],
) {
    let mut acc0 = _mm_setzero_si128();
    let mut acc1 = _mm_setzero_si128();
    let mut acc2 = _mm_setzero_si128();
    let mut acc3 = _mm_setzero_si128();

    for y in 0..H {
        let y_isize = y as isize;
        let r1 = sample1.row_n::<4>(y_isize, 0);
        let v1 = _mm_cvtsi32_si128(i32::from_ne_bytes(r1));

        let r2_up = sample2.row_n::<4>(y_isize - 1, 0);
        let r2_dn = sample2.row_n::<4>(y_isize + 1, 0);
        let r2_lt = sample2.row_n::<4>(y_isize, -1);
        let r2_rt = sample2.row_n::<4>(y_isize, 1);

        let v2_up = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_up));
        let v2_dn = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_dn));
        let v2_lt = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_lt));
        let v2_rt = _mm_cvtsi32_si128(i32::from_ne_bytes(r2_rt));

        acc0 = _mm_add_epi64(acc0, _mm_sad_epu8(v1, v2_up));
        acc1 = _mm_add_epi64(acc1, _mm_sad_epu8(v1, v2_dn));
        acc2 = _mm_add_epi64(acc2, _mm_sad_epu8(v1, v2_lt));
        acc3 = _mm_add_epi64(acc3, _mm_sad_epu8(v1, v2_rt));
    }

    sad[0] = _mm_cvtsi128_si32(acc0);
    sad[1] = _mm_cvtsi128_si32(acc1);
    sad[2] = _mm_cvtsi128_si32(acc2);
    sad[3] = _mm_cvtsi128_si32(acc3);
}

// ============================================================================
// Safe Public Wrappers
// ============================================================================

#[inline(always)]
pub fn sample_sad_16x16_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x_sse2::<S, 16>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x_avx2::<S, 16>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_16x8_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x_sse2::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_16x8_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x_avx2::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x16_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x_sse2::<S, 16>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x8_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x_sse2::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_4x4_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x_sse2::<S, 4>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x4_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x_sse2::<S, 4>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_4x8_sse2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x_sse2::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_four_16x16_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_16x_sse2::<S, 16>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_16x8_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_16x_sse2::<S, 8>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x16_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x_sse2::<S, 16>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x8_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x_sse2::<S, 8>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_4x4_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_4x_sse2::<S, 4>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x4_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x_sse2::<S, 4>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_4x8_sse2<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_4x_sse2::<S, 8>(sample1, sample2, sad) }
}

// ============================================================================
// Unit Tests: Differential Parity Against Scalar Kernels
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::sad_common::{sample_sad, sample_sad_four};
    use crate::safe::plane::PlaneCursor;

    fn make_test_planes(stride: usize, height: usize) -> (Vec<u8>, Vec<u8>) {
        let mut p1 = vec![0u8; stride * height];
        let mut p2 = vec![0u8; stride * height];
        for (i, b) in p1.iter_mut().enumerate() {
            *b = ((i * 17 + 5) & 0xFF) as u8;
        }
        for (i, b) in p2.iter_mut().enumerate() {
            *b = ((i * 31 + 13) & 0xFF) as u8;
        }
        (p1, p2)
    }

    #[test]
    fn test_sse2_sad_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(sample_sad_16x16_sse2(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_16x8_sse2(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_8x16_sse2(&c1, &c2), sample_sad::<8, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_8x8_sse2(&c1, &c2), sample_sad::<8, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_4x4_sse2(&c1, &c2), sample_sad::<4, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_8x4_sse2(&c1, &c2), sample_sad::<8, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_4x8_sse2(&c1, &c2), sample_sad::<4, 8, _>(&c1, &c2));
    }

    #[test]
    fn test_avx2_sad_parity() {
        if !std::is_x86_feature_detected!("avx2") {
            return;
        }
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(sample_sad_16x16_avx2(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_16x8_avx2(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2));
    }

    #[test]
    fn test_sse2_sample_sad_four_parity() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 16 + 16, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 16 + 16, 64);

        let mut expected = [0i32; 4];
        let mut actual = [0i32; 4];

        sample_sad_four::<16, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x16_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x16 four-point SAD mismatch");

        sample_sad_four::<16, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x8_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x8 four-point SAD mismatch");

        sample_sad_four::<8, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x16_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x16 four-point SAD mismatch");

        sample_sad_four::<8, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x8_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x8 four-point SAD mismatch");

        sample_sad_four::<4, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x4_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x4 four-point SAD mismatch");

        sample_sad_four::<8, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x4_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x4 four-point SAD mismatch");

        sample_sad_four::<4, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x8_sse2(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x8 four-point SAD mismatch");
    }
}
