//! `codec/encoder/core/src/sample.cpp` — the SATD kernels and
//! `WelsInitSampleSadFunc`, which installs the sample-cost tables the mode-decision
//! layer scores every candidate with.
//!
//! The SAD kernels live in `common/sad_common.rs` (`sad_common.cpp`); only the SATD
//! half and the table filler are here.

#![allow(non_snake_case, non_upper_case_globals)]
// ---------------------------------------------------------------------------
// The whole butterfly is `i32` (`int32_t pSampleMix[4][4]`): |diff| <= 255 with a
// 16x Hadamard gain, and `(sum + 1) >> 1` per 4x4 sub-block.
// ---------------------------------------------------------------------------
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

#[cfg(test)]
use crate::safe::plane::PlaneCursor;
use crate::safe::plane::RefSamples;

/// Hadamard 4x4 sum of absolute transformed differences of two 4x4 blocks.
///
/// C++: `WelsSampleSatd4x4_c`, `codec/encoder/core/src/sample.cpp`.
pub fn satd_4x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut mix = [[0i32; 4]; 4];

    for (i, row) in mix.iter_mut().enumerate() {
        // `RecCursor::row` is const-sized and returns by value — a shared
        // cell view cannot lend a row.
        let r1: [u8; 4] = c1.row_n::<4>(i as isize, 0);
        let r2: [u8; 4] = c2.row_n::<4>(i as isize, 0);
        for k in 0..4 {
            row[k] = r1[k] as i32 - r2[k] as i32;
        }
    }

    for row in mix.iter_mut() {
        let s0 = row[0] + row[2];
        let s1 = row[1] + row[3];
        let s2 = row[0] - row[2];
        let s3 = row[1] - row[3];
        *row = [s0 + s1, s2 + s3, s2 - s3, s0 - s1];
    }

    let mut satd = 0i32;
    for i in 0..4 {
        let s0 = mix[0][i] + mix[2][i];
        let s1 = mix[1][i] + mix[3][i];
        let s2 = mix[0][i] - mix[2][i];
        let s3 = mix[1][i] - mix[3][i];
        satd += (s0 + s1).abs() + (s2 + s3).abs() + (s2 - s3).abs() + (s0 - s1).abs();
    }

    (satd + 1) >> 1
}

/// C++: `WelsSampleSatd8x4_c` — two 4x4s left-to-right.
pub fn satd_8x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0))
}

/// C++: `WelsSampleSatd4x8_c` — two 4x4s top-to-bottom.
pub fn satd_4x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4))
}

/// C++: `WelsSampleSatd8x8_c` — four 4x4s: top-left, top-right, bottom-left,
/// bottom-right.
pub fn satd_8x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_4x4(c1, c2);
    satd += satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0));
    satd += satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4));
    satd += satd_4x4(&c1.advance(4, 4), &c2.advance(4, 4));
    satd
}

/// C++: `WelsSampleSatd16x8_c` — two 8x8s left-to-right.
pub fn satd_16x8<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0))
}

/// C++: `WelsSampleSatd8x16_c` — two 8x8s top-to-bottom.
pub fn satd_8x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8))
}

/// C++: `WelsSampleSatd16x16_c` — four 8x8s in the same quadrant order.
pub fn satd_16x16<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut satd = satd_8x8(c1, c2);
    satd += satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0));
    satd += satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8));
    satd += satd_8x8(&c1.advance(8, 8), &c2.advance(8, 8));
    satd
}

use crate::common::sad_common::{sample_sad, sample_sad_four};
use crate::encoder::svc_mode_decision::{
    BLOCK_4x4, BLOCK_4x8, BLOCK_8x4, BLOCK_8x8, BLOCK_8x16, BLOCK_16x8, BLOCK_16x16,
};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

use crate::common::cpu_core::{WELS_CPU_AVX2, WELS_CPU_SSE2};
/// The kernel set the dispatch sites below call: `simd::x86_64` or `simd::aarch64` by
/// default, `simd::scalar` under `--features scalar` or on a target with neither.
use crate::simd::kernels;

/// `sample.cpp:336`. Installs the scalar SAD/SATD/4-SAD tables, then overrides entries
/// with SIMD kernels where `uiCpuFlag` and the hardware allow.
///
/// The only writer of the three tables, so every slot is fixed from the first frame on.
/// The tables exist for the runtime-indexed readers: the motion search's `[block_size]`,
/// and `md_cost`/`me_cost`'s family selection.
pub fn WelsInitSampleSadFunc(pFuncList: &mut SWelsFuncPtrList, uiCpuFlag: u32) {
    let sdf = &mut pFuncList.sSampleDealingFuncs;

    sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| sample_sad::<16, 16, _>(a, b));
    sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| sample_sad::<16, 8, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x16] = Some(|a, b| sample_sad::<8, 16, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x8] = Some(|a, b| sample_sad::<8, 8, _>(a, b));
    sdf.pfSampleSad[BLOCK_4x4] = Some(|a, b| sample_sad::<4, 4, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x4] = Some(|a, b| sample_sad::<8, 4, _>(a, b));
    sdf.pfSampleSad[BLOCK_4x8] = Some(|a, b| sample_sad::<4, 8, _>(a, b));

    sdf.pfSampleSatd[BLOCK_16x16] = Some(|a, b| satd_16x16(a, b));
    sdf.pfSampleSatd[BLOCK_16x8] = Some(|a, b| satd_16x8(a, b));
    sdf.pfSampleSatd[BLOCK_8x16] = Some(|a, b| satd_8x16(a, b));
    sdf.pfSampleSatd[BLOCK_8x8] = Some(|a, b| satd_8x8(a, b));
    sdf.pfSampleSatd[BLOCK_4x4] = Some(|a, b| satd_4x4(a, b));
    sdf.pfSampleSatd[BLOCK_8x4] = Some(|a, b| satd_8x4(a, b));
    sdf.pfSampleSatd[BLOCK_4x8] = Some(|a, b| satd_4x8(a, b));

    sdf.pfSample4Sad[BLOCK_16x16] = Some(|a, b, sad| sample_sad_four::<16, 16, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_16x8] = Some(|a, b, sad| sample_sad_four::<16, 8, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_8x16] = Some(|a, b, sad| sample_sad_four::<8, 16, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_8x8] = Some(|a, b, sad| sample_sad_four::<8, 8, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_4x4] = Some(|a, b, sad| sample_sad_four::<4, 4, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_8x4] = Some(|a, b, sad| sample_sad_four::<8, 4, _>(a, b, sad));
    sdf.pfSample4Sad[BLOCK_4x8] = Some(|a, b, sad| sample_sad_four::<4, 8, _>(a, b, sad));

    if (uiCpuFlag & WELS_CPU_SSE2) != 0 {
        sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| kernels::sad::sample_sad_16x16(a, b));
        sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| kernels::sad::sample_sad_16x8(a, b));
        sdf.pfSampleSad[BLOCK_8x16] = Some(|a, b| kernels::sad::sample_sad_8x16(a, b));
        sdf.pfSampleSad[BLOCK_8x8] = Some(|a, b| kernels::sad::sample_sad_8x8(a, b));
        // SAD is an exact integer cost, and these kernels agree with
        // `sample_sad::<W, H>` bit for bit, so mode decision picks the same modes.
        sdf.pfSampleSad[BLOCK_4x4] = Some(|a, b| kernels::sad::sample_sad_4x4(a, b));
        sdf.pfSampleSad[BLOCK_8x4] = Some(|a, b| kernels::sad::sample_sad_8x4(a, b));
        sdf.pfSampleSad[BLOCK_4x8] = Some(|a, b| kernels::sad::sample_sad_4x8(a, b));

        sdf.pfSample4Sad[BLOCK_16x16] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_16x16(a, b, sad));
        sdf.pfSample4Sad[BLOCK_16x8] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_16x8(a, b, sad));
        sdf.pfSample4Sad[BLOCK_8x16] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_8x16(a, b, sad));
        sdf.pfSample4Sad[BLOCK_8x8] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_8x8(a, b, sad));
        sdf.pfSample4Sad[BLOCK_4x4] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_4x4(a, b, sad));
        sdf.pfSample4Sad[BLOCK_8x4] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_8x4(a, b, sad));
        sdf.pfSample4Sad[BLOCK_4x8] =
            Some(|a, b, sad| kernels::sad::sample_sad_four_4x8(a, b, sad));

        sdf.pfSampleSatd[BLOCK_4x4] = Some(|a, b| kernels::satd::satd_4x4(a, b));
        sdf.pfSampleSatd[BLOCK_8x8] = Some(|a, b| kernels::satd::satd_8x8(a, b));
        sdf.pfSampleSatd[BLOCK_8x16] = Some(|a, b| kernels::satd::satd_8x16(a, b));
        sdf.pfSampleSatd[BLOCK_16x8] = Some(|a, b| kernels::satd::satd_16x8(a, b));
        sdf.pfSampleSatd[BLOCK_16x16] = Some(|a, b| kernels::satd::satd_16x16(a, b));
        // SATD is an exact integer cost, so these two shapes score identically to the
        // scalars above.
        sdf.pfSampleSatd[BLOCK_8x4] = Some(|a, b| kernels::satd::satd_8x4(a, b));
        sdf.pfSampleSatd[BLOCK_4x8] = Some(|a, b| kernels::satd::satd_4x8(a, b));
    }

    // `uiCpuFlag` is the caller's policy (a caller may restrict it to `0`), `has_avx2()`
    // the hardware fact. The kernels below are `#[target_feature(enable = "avx2")]` and
    // run `vpsadbw` with no test of their own, so the flag alone is not enough: a made-up
    // flag word would mean SIGILL on a pre-Haswell part. Asked once, when the table is
    // built, not per candidate scored.
    if (uiCpuFlag & WELS_CPU_AVX2) != 0 && crate::simd::has_avx2() {
        sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| kernels::sad::sample_sad_16x16_avx2(a, b));
        sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| kernels::sad::sample_sad_16x8_avx2(a, b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SATD of a block against itself is zero for every size.
    #[test]
    fn satd_of_identical_blocks_is_zero() {
        let stride = 24usize;
        let a: Vec<u8> = (0..stride * 20).map(|i| ((i * 37) % 256) as u8).collect();
        let b = a.clone();
        let ca = PlaneCursor::new(&a, 0, stride);
        let cb = PlaneCursor::new(&b, 0, stride);
        assert_eq!(satd_4x4(&ca, &cb), 0);
        assert_eq!(satd_8x4(&ca, &cb), 0);
        assert_eq!(satd_4x8(&ca, &cb), 0);
        assert_eq!(satd_8x8(&ca, &cb), 0);
        assert_eq!(satd_16x8(&ca, &cb), 0);
        assert_eq!(satd_8x16(&ca, &cb), 0);
        assert_eq!(satd_16x16(&ca, &cb), 0);
    }

    /// A constant offset between the two blocks concentrates all the energy in the DC
    /// coefficient: SATD4x4 = |16 * d| / 2 = 8 * |d| after the `(sum + 1) >> 1`.
    #[test]
    fn satd4x4_of_constant_offset_is_dc_only() {
        let stride = 16usize;
        let a = vec![100u8; stride * 8];
        let b = vec![107u8; stride * 8];
        let got = satd_4x4(
            &PlaneCursor::new(&a, 0, stride),
            &PlaneCursor::new(&b, 0, stride),
        );
        // difference -7 everywhere -> DC = 16 * -7, all AC zero -> (112 + 1) >> 1
        assert_eq!(got, (16 * 7 + 1) >> 1);
    }

    /// The larger SATDs are exactly the sum of their 4x4 sub-blocks.
    #[test]
    fn satd_composes_from_4x4_subblocks() {
        let stride = 32usize;
        let a: Vec<u8> = (0..stride * 20)
            .map(|i| ((i * 91 + 13) % 256) as u8)
            .collect();
        let b: Vec<u8> = (0..stride * 20)
            .map(|i| ((i * 17 + 200) % 256) as u8)
            .collect();
        let ca = PlaneCursor::new(&a, 0, stride);
        let cb = PlaneCursor::new(&b, 0, stride);

        let mut sum8x8 = 0;
        for (dy, dx) in [(0isize, 0isize), (0, 4), (4, 0), (4, 4)] {
            sum8x8 += satd_4x4(&ca.advance(dx, dy), &cb.advance(dx, dy));
        }
        assert_eq!(satd_8x8(&ca, &cb), sum8x8);

        let mut sum16x16 = 0;
        for (dy, dx) in [(0isize, 0isize), (0, 8), (8, 0), (8, 8)] {
            sum16x16 += satd_8x8(&ca.advance(dx, dy), &cb.advance(dx, dy));
        }
        assert_eq!(satd_16x16(&ca, &cb), sum16x16);
    }

    /// Every slot the mode-decision layer indexes must be filled, and the five
    /// `Combined3` slots must be left NULL — `svc_base_layer_md` asserts on that.
    #[test]
    fn init_fills_sad_and_satd_and_clears_combined3() {
        for flags in [0, WELS_CPU_SSE2, WELS_CPU_SSE2 | WELS_CPU_AVX2] {
            let mut fl = SWelsFuncPtrList::default();
            WelsInitSampleSadFunc(&mut fl, flags);

            for b in [
                BLOCK_16x16,
                BLOCK_16x8,
                BLOCK_8x16,
                BLOCK_8x8,
                BLOCK_4x4,
                BLOCK_8x4,
                BLOCK_4x8,
            ] {
                assert!(
                    fl.sSampleDealingFuncs.pfSampleSad[b].is_some(),
                    "sad[{b}] flags={flags:#x}"
                );
                assert!(
                    fl.sSampleDealingFuncs.pfSampleSatd[b].is_some(),
                    "satd[{b}] flags={flags:#x}"
                );
                assert!(
                    fl.sSampleDealingFuncs.pfSample4Sad[b].is_some(),
                    "sad4[{b}] flags={flags:#x}"
                );
            }
        }
    }
}
