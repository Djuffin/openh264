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
pub unsafe fn sad_16x<S: RefSamples, const H: usize>(
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
    // The loop below steps two rows per iteration, so an odd `H` would read and
    // accumulate row `H` — one past the block. Only 16 and 8 are instantiated today;
    // the SSE2 twin (`sad_16x`, `:22`) iterates one row at a time and has no such
    // constraint, and nothing in either signature said so.
    const { assert!(H % 2 == 0, "sad_16x_avx2 filters two rows per step; H must be even") };
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
pub unsafe fn sad_8x<S: RefSamples, const H: usize>(
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
pub unsafe fn sad_4x<S: RefSamples, const H: usize>(
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
pub unsafe fn sample_sad_four_16x<S: RefSamples, const H: usize>(
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
pub unsafe fn sample_sad_four_8x<S: RefSamples, const H: usize>(
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
pub unsafe fn sample_sad_four_4x<S: RefSamples, const H: usize>(
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
pub fn sample_sad_16x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x::<S, 16>(sample1, sample2, 0, 0) }
}

/// # The AVX2 precondition, and where it is established
///
/// `sad_16x_avx2` is `#[target_feature(enable = "avx2")]`: this runs `vpsadbw` with
/// no test of its own and faults on any pre-Haswell Intel or pre-Excavator AMD part.
///
/// The one caller is `encoder::sample::WelsInitSampleSadFunc`, which installs these
/// into `pfSampleSad` only under `uiCpuFlag & WELS_CPU_AVX2 && simd::has_avx2()`.
/// That is the right altitude for the test — it is asked once when the table is
/// built, not on every candidate the mode-decision loop scores — and it is why this
/// is `pub(crate)`: the module boundary is what keeps the set of callers to the one
/// that checks, in place of a branch each call would pay for.
#[inline(always)]
pub(crate) fn sample_sad_16x16_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    // SAFETY: the caller established AVX2 support before installing this; see above.
    unsafe { sad_16x_avx2::<S, 16>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_16x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_16x::<S, 8>(sample1, sample2, 0, 0) }
}

/// # The AVX2 precondition, and where it is established
///
/// `sad_16x_avx2` is `#[target_feature(enable = "avx2")]`: this runs `vpsadbw` with
/// no test of its own and faults on any pre-Haswell Intel or pre-Excavator AMD part.
///
/// The one caller is `encoder::sample::WelsInitSampleSadFunc`, which installs these
/// into `pfSampleSad` only under `uiCpuFlag & WELS_CPU_AVX2 && simd::has_avx2()`.
/// That is the right altitude for the test — it is asked once when the table is
/// built, not on every candidate the mode-decision loop scores — and it is why this
/// is `pub(crate)`: the module boundary is what keeps the set of callers to the one
/// that checks, in place of a branch each call would pay for.
#[inline(always)]
pub(crate) fn sample_sad_16x8_avx2<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    // SAFETY: the caller established AVX2 support before installing this; see above.
    unsafe { sad_16x_avx2::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x16<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 16>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_4x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x::<S, 4>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_8x4<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_8x::<S, 4>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_4x8<S: RefSamples>(sample1: &S, sample2: &S) -> i32 {
    unsafe { sad_4x::<S, 8>(sample1, sample2, 0, 0) }
}

#[inline(always)]
pub fn sample_sad_four_16x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_16x::<S, 16>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_16x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_16x::<S, 8>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x16<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x::<S, 16>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x::<S, 8>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_4x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_4x::<S, 4>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_8x4<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_8x::<S, 4>(sample1, sample2, sad) }
}

#[inline(always)]
pub fn sample_sad_four_4x8<S: RefSamples>(sample1: &S, sample2: &S, sad: &mut [i32; 4]) {
    unsafe { sample_sad_four_4x::<S, 8>(sample1, sample2, sad) }
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
    fn test_sad_parity_all_shapes() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 8 + 8, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 8 + 8, 64);

        assert_eq!(sample_sad_16x16(&c1, &c2), sample_sad::<16, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_16x8(&c1, &c2), sample_sad::<16, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_8x16(&c1, &c2), sample_sad::<8, 16, _>(&c1, &c2));
        assert_eq!(sample_sad_8x8(&c1, &c2), sample_sad::<8, 8, _>(&c1, &c2));
        assert_eq!(sample_sad_4x4(&c1, &c2), sample_sad::<4, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_8x4(&c1, &c2), sample_sad::<8, 4, _>(&c1, &c2));
        assert_eq!(sample_sad_4x8(&c1, &c2), sample_sad::<4, 8, _>(&c1, &c2));
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
    fn test_sample_sad_four_parity() {
        let (p1, p2) = make_test_planes(64, 64);
        let c1 = PlaneCursor::new(&p1, 64 * 16 + 16, 64);
        let c2 = PlaneCursor::new(&p2, 64 * 16 + 16, 64);

        let mut expected = [0i32; 4];
        let mut actual = [0i32; 4];

        sample_sad_four::<16, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x16(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x16 four-point SAD mismatch");

        sample_sad_four::<16, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_16x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "16x8 four-point SAD mismatch");

        sample_sad_four::<8, 16, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x16(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x16 four-point SAD mismatch");

        sample_sad_four::<8, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x8 four-point SAD mismatch");

        sample_sad_four::<4, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x4(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x4 four-point SAD mismatch");

        sample_sad_four::<8, 4, _>(&c1, &c2, &mut expected);
        sample_sad_four_8x4(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "8x4 four-point SAD mismatch");

        sample_sad_four::<4, 8, _>(&c1, &c2, &mut expected);
        sample_sad_four_4x8(&c1, &c2, &mut actual);
        assert_eq!(actual, expected, "4x8 four-point SAD mismatch");
    }

    // ========================================================================
    // Input and anchor coverage.
    //
    // The three tests above reach all sixteen kernels, but each at one anchor over one
    // input pattern, so a kernel wrong at another alignment — or only on inputs a ramp
    // never produces — passes them all.
    //
    // The sweep below runs every kernel over four anchors, one per residue class mod 8,
    // and five distributions. The all-`0xFF`/all-`0x00` pair and the near-identical
    // pair are the ends of the accumulator's range, where a `psadbw` accumulation that
    // widened or saturated wrongly would show.
    // ========================================================================

    /// A 64-bit LCG, so a failing seed is replayable.
    fn lcg(seed: &mut u64) -> u8 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 32) as u8
    }

    /// The five input distributions, each a pair of planes of `stride * height` bytes.
    fn input_pairs(stride: usize, height: usize) -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
        let n = stride * height;
        let mut seed = 0x5DEECE66Du64;
        let noise1: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();
        let noise2: Vec<u8> = (0..n).map(|_| lcg(&mut seed)).collect();

        let mut near = noise1.clone();
        // A handful of differing bytes: the accumulator spends most of its range at 0.
        for (i, b) in near.iter_mut().enumerate() {
            if i % 97 == 0 {
                *b = b.wrapping_add(1);
            }
        }

        let (ramp1, ramp2) = make_test_planes(stride, height);
        vec![
            ("ramps", ramp1, ramp2),
            ("noise", noise1.clone(), noise2),
            ("max-diff", vec![0xFFu8; n], vec![0x00u8; n]),
            ("near-identical", noise1, near),
            ("identical", vec![0x5Au8; n], vec![0x5Au8; n]),
        ]
    }

    /// Four anchors covering every residue mod 8, so the aligned case is not the only
    /// one tested. Each leaves at least 16 rows and 16 columns of margin on all sides.
    const ANCHORS: [usize; 4] = [64 * 16 + 16, 64 * 17 + 19, 64 * 18 + 22, 64 * 19 + 21];

    #[test]
    fn sad_parity_over_anchors_and_distributions() {
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");

                assert_eq!(
                    sample_sad_16x16(&c1, &c2),
                    sample_sad::<16, 16, _>(&c1, &c2),
                    "16x16 {at}"
                );
                assert_eq!(
                    sample_sad_16x8(&c1, &c2),
                    sample_sad::<16, 8, _>(&c1, &c2),
                    "16x8 {at}"
                );
                assert_eq!(
                    sample_sad_8x16(&c1, &c2),
                    sample_sad::<8, 16, _>(&c1, &c2),
                    "8x16 {at}"
                );
                assert_eq!(
                    sample_sad_8x8(&c1, &c2),
                    sample_sad::<8, 8, _>(&c1, &c2),
                    "8x8 {at}"
                );
                assert_eq!(
                    sample_sad_4x4(&c1, &c2),
                    sample_sad::<4, 4, _>(&c1, &c2),
                    "4x4 {at}"
                );
                assert_eq!(
                    sample_sad_8x4(&c1, &c2),
                    sample_sad::<8, 4, _>(&c1, &c2),
                    "8x4 {at}"
                );
                assert_eq!(
                    sample_sad_4x8(&c1, &c2),
                    sample_sad::<4, 8, _>(&c1, &c2),
                    "4x8 {at}"
                );
            }
        }
    }

    #[test]
    fn sample_sad_four_parity_over_anchors_and_distributions() {
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");
                let (mut want, mut got) = ([0i32; 4], [0i32; 4]);

                sample_sad_four::<16, 16, _>(&c1, &c2, &mut want);
                sample_sad_four_16x16(&c1, &c2, &mut got);
                assert_eq!(got, want, "16x16 four-point {at}");

                sample_sad_four::<16, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_16x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "16x8 four-point {at}");

                sample_sad_four::<8, 16, _>(&c1, &c2, &mut want);
                sample_sad_four_8x16(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x16 four-point {at}");

                sample_sad_four::<8, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_8x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x8 four-point {at}");

                sample_sad_four::<4, 4, _>(&c1, &c2, &mut want);
                sample_sad_four_4x4(&c1, &c2, &mut got);
                assert_eq!(got, want, "4x4 four-point {at}");

                sample_sad_four::<8, 4, _>(&c1, &c2, &mut want);
                sample_sad_four_8x4(&c1, &c2, &mut got);
                assert_eq!(got, want, "8x4 four-point {at}");

                sample_sad_four::<4, 8, _>(&c1, &c2, &mut want);
                sample_sad_four_4x8(&c1, &c2, &mut got);
                assert_eq!(got, want, "4x8 four-point {at}");
            }
        }
    }

    /// The AVX2 pair over the same sweep.
    ///
    /// Like `test_avx2_sad_parity` this can only run where the host has AVX2, but it
    /// says so on the way out instead of returning green in silence: a run that
    /// reports "ok" having executed nothing is the failure mode worth avoiding here.
    #[test]
    fn avx2_sad_parity_over_anchors_and_distributions() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!(
                "SKIPPED avx2_sad_parity_over_anchors_and_distributions: \
                 this host has no AVX2, so the two AVX2 kernels were not executed"
            );
            return;
        }
        for (name, p1, p2) in input_pairs(64, 64) {
            for anchor in ANCHORS {
                let c1 = PlaneCursor::new(&p1, anchor, 64);
                let c2 = PlaneCursor::new(&p2, anchor, 64);
                let at = format!("{name} @ {anchor}");
                assert_eq!(
                    sample_sad_16x16_avx2(&c1, &c2),
                    sample_sad::<16, 16, _>(&c1, &c2),
                    "16x16 avx2 {at}"
                );
                assert_eq!(
                    sample_sad_16x8_avx2(&c1, &c2),
                    sample_sad::<16, 8, _>(&c1, &c2),
                    "16x8 avx2 {at}"
                );
            }
        }
    }

    /// The table site is the only thing standing between these kernels and a SIGILL,
    /// so pin what it installs.
    ///
    /// `WelsInitSampleSadFunc` fills `pfSampleSad[BLOCK_16x16]` from the AVX2 kernel
    /// exactly when `uiCpuFlag` asks for AVX2 *and* this CPU has it.
    ///
    /// **What this catches, and where.** The flag half is pinned on every host: drop
    /// the `uiCpuFlag & WELS_CPU_AVX2` test and the `WELS_CPU_SSE2`-only case starts
    /// returning the AVX2 pointer. The hardware half — dropping `has_avx2()` — can only
    /// fail this test on a machine without AVX2, because on one with it both spellings
    /// install the same kernel. That is the machine where it matters, and it is not
    /// this one, so treat a green run here as covering the flag half only.
    ///
    /// Function-pointer identity is the comparison, so the caveat on
    /// `common/mc.rs`'s `init_mc_func_cpu_flags` applies: both addresses come from the
    /// same `WelsInitSampleSadFunc` instantiation, which makes them comparable, but
    /// Miri mints a fresh synthetic address per reification and is excluded.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn init_sample_sad_installs_avx2_only_where_the_cpu_has_it() {
        use crate::common::cpu_core::{WELS_CPU_AVX2, WELS_CPU_SSE2};
        use crate::encoder::svc_mode_decision::BLOCK_16x16;
        use crate::encoder::sample::WelsInitSampleSadFunc;
        use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

        let slot = |flags: u32| {
            let mut fl = SWelsFuncPtrList::default();
            WelsInitSampleSadFunc(&mut fl, flags);
            fl.sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].map(|f| f as usize)
        };

        let baseline = slot(WELS_CPU_SSE2);
        let asked_for_avx2 = slot(WELS_CPU_SSE2 | WELS_CPU_AVX2);
        assert!(baseline.is_some() && asked_for_avx2.is_some());

        // **The oracle is `has_avx2()`, not `is_x86_feature_detected!`.** They are not the
        // same question: the table arm consults the port's probe, which answers from the
        // build as well as the CPU — under `--features scalar` the feature word is `0`, so
        // a host that *has* AVX2 must still get the baseline entry. Asking the CPU
        // directly made this test fail there.
        if crate::simd::has_avx2() {
            assert_ne!(
                asked_for_avx2, baseline,
                "has_avx2() is true, so asking for AVX2 must change the installed kernel"
            );
        } else {
            assert_eq!(
                asked_for_avx2, baseline,
                "has_avx2() is false, so the flag alone must not install an AVX2 kernel"
            );
        }
    }
}
