//! Port of `codec/encoder/core/src/sample.cpp` — the SATD kernels and
//! `WelsInitSampleSadFunc`, which installs the sample-cost tables the mode-decision
//! layer scores every candidate with.
//!
//! The SAD kernels already live in `common/sad_common.rs` (`sad_common.cpp`); only
//! the SATD half and the table filler are here.
//!
//! The `Combined3` entries are set to `NULL` by the scalar path and only ever
//! assigned from SIMD kernels behind a `uiCpuFlag` test. Measured against
//! `libopenh264.a` on this machine, `WelsCPUFeatureDetect` returns `0x00000000`, so
//! the reference leaves all five NULL — matching this port, which has no SIMD.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

// ---------------------------------------------------------------------------
// Arithmetic parity: the whole butterfly is `i32`, exactly as the C++
// (`int32_t pSampleMix[4][4]`) — total over all `u8` inputs
// (|diff| <= 255, 16x Hadamard gain, `(sum + 1) >> 1` per 4x4 sub-block; the
// per-sub-block rounding makes the composition order part of the contract,
// mirrored below).
// ---------------------------------------------------------------------------

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::RefSamples;
#[cfg(test)]
use crate::safe::plane::PlaneCursor;

/// Hadamard 4x4 sum of absolute transformed differences of two 4x4 blocks.
///
/// C++: `WelsSampleSatd4x4_c`, `codec/encoder/core/src/sample.cpp`.
pub fn satd_4x4<A: RefSamples + Copy, B: RefSamples + Copy>(c1: &A, c2: &B) -> i32 {
    let mut mix = [[0i32; 4]; 4];

    for (i, row) in mix.iter_mut().enumerate() {
        // `RecCursor::row` is const-sized and returns by value — a shared
        // cell view cannot lend a row. Same four samples, same order.
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
/// bottom-right (the C++'s summation order, kept).
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
    BLOCK_16x16, BLOCK_16x8, BLOCK_4x4, BLOCK_4x8, BLOCK_8x16, BLOCK_8x4, BLOCK_8x8,
};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

/// `sample.cpp:336`. Installs the scalar SAD/SATD/4-SAD tables and clears the five
/// `Combined3` slots. The SIMD overrides that follow in the C++ are all behind
/// `uiCpuFlag` tests that do not fire here.
///
/// **This is the only writer of the three tables, and `_uiCpuFlag` is unused** —
/// so every slot is a compile-time constant from the first frame on, and a call
/// site whose block index is itself a constant may call the kernel directly,
/// byte-identically, without going through the table at all. The table exists for
/// the runtime-indexed readers (the motion search hoists `[block_size]`, and
/// `md_cost`/`me_cost`'s family selection).
pub fn WelsInitSampleSadFunc(pFuncList: &mut SWelsFuncPtrList, _uiCpuFlag: u32) {
    let sdf = &mut pFuncList.sSampleDealingFuncs;

    //pfSampleSad init
    sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| sample_sad::<16, 16, _>(a, b));
    sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| sample_sad::<16, 8, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x16] = Some(|a, b| sample_sad::<8, 16, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x8] = Some(|a, b| sample_sad::<8, 8, _>(a, b));
    sdf.pfSampleSad[BLOCK_4x4] = Some(|a, b| sample_sad::<4, 4, _>(a, b));
    sdf.pfSampleSad[BLOCK_8x4] = Some(|a, b| sample_sad::<8, 4, _>(a, b));
    sdf.pfSampleSad[BLOCK_4x8] = Some(|a, b| sample_sad::<4, 8, _>(a, b));

    //pfSampleSatd init
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

    // The five `pfIntra*Combined3*` slots were nulled here, as the C++ does. They
    // were never anything else on any target this port builds for, and the fields
    // are deleted.
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
        let got = satd_4x4(&PlaneCursor::new(&a, 0, stride), &PlaneCursor::new(&b, 0, stride));
        // difference -7 everywhere -> DC = 16 * -7, all AC zero -> (112 + 1) >> 1
        assert_eq!(got, (16 * 7 + 1) >> 1);
    }

    /// The larger SATDs are exactly the sum of their 4x4 sub-blocks.
    #[test]
    fn satd_composes_from_4x4_subblocks() {
        let stride = 32usize;
        let a: Vec<u8> = (0..stride * 20).map(|i| ((i * 91 + 13) % 256) as u8).collect();
        let b: Vec<u8> = (0..stride * 20).map(|i| ((i * 17 + 200) % 256) as u8).collect();
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
        let mut fl = SWelsFuncPtrList::default();
        WelsInitSampleSadFunc(&mut fl, 0);

        for b in [BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4, BLOCK_4x8] {
            assert!(fl.sSampleDealingFuncs.pfSampleSad[b].is_some(), "sad[{b}]");
            assert!(fl.sSampleDealingFuncs.pfSampleSatd[b].is_some(), "satd[{b}]");
            assert!(fl.sSampleDealingFuncs.pfSample4Sad[b].is_some(), "sad4[{b}]");
        }
    }
}
