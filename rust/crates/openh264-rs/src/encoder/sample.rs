//! Port of `codec/encoder/core/src/sample.cpp` — the SATD kernels and
//! `WelsInitSampleSadFunc`, which installs the sample-cost tables the mode-decision
//! layer scores every candidate with.
//!
//! The SAD kernels already live in `common/sad_common.rs` (`sad_common.cpp`); only
//! the SATD half and the table filler are new here.
//!
//! **The SATD family is proven-and-parked** (see the safe-kernel banner below):
//! the seven `satd_*` kernels are differentially proven, nothing installs them,
//! and the raw `WelsSampleSatd*_c` bodies stay live until Phase 4a's
//! direct-dispatch checkpoint — the same state as `sad_common.rs`'s SAD family.
//!
//! The `Combined3` entries are set to `NULL` by the scalar path and only ever
//! assigned from SIMD kernels behind a `uiCpuFlag` test. Measured against
//! `libopenh264.a` on this machine, `WelsCPUFeatureDetect` returns `0x00000000`, so
//! the reference leaves all five NULL — matching this port, which has no SIMD.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

// ---------------------------------------------------------------------------
// Safe kernels — PROVEN AND PARKED (D-perf-4; the D-perf-2 verdict applied).
//
// The seven `satd_*` kernels below are differentially proven against the raw
// `WelsSampleSatd*_c` ones and **nothing installs them**: this family has
// T5-sad's call-density profile (tiny blocks, per-candidate calls from motion
// estimation), D-perf-2 measured that class at 1.39-2.97x through the shim
// boundary, and +15-17% on an encoder already ≈+11% cumulative crosses the
// +25% tripwire. So the raw kernels stay live, exactly like
// `common/sad_common.rs`'s, and the re-attempt point is **Phase 4a's
// direct-dispatch checkpoint** (slices-and-offsets is on the table there —
// plan §5 Phase 4a, `perf_baseline.md` §Parked).
//
// Arithmetic parity (R-e): the whole butterfly is `i32`, exactly as the C++
// (`int32_t pSampleMix[4][4]`) and the raw port — total over all `u8` inputs
// (|diff| <= 255, 16x Hadamard gain, `(sum + 1) >> 1` per 4x4 sub-block; the
// per-sub-block rounding makes the composition order part of the contract,
// mirrored below).
// ---------------------------------------------------------------------------

#![deny(unsafe_code)]

use crate::safe::plane::PlaneCursor;

/// Hadamard 4x4 sum of absolute transformed differences of two 4x4 blocks.
///
/// C++: `WelsSampleSatd4x4_c`, `codec/encoder/core/src/sample.cpp`.
pub fn satd_4x4(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    let mut mix = [[0i32; 4]; 4];

    for (i, row) in mix.iter_mut().enumerate() {
        let r1: &[u8; 4] = c1.row(i as isize, 0, 4).try_into().unwrap();
        let r2: &[u8; 4] = c2.row(i as isize, 0, 4).try_into().unwrap();
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
pub fn satd_8x4(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0))
}

/// C++: `WelsSampleSatd4x8_c` — two 4x4s top-to-bottom.
pub fn satd_4x8(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    satd_4x4(c1, c2) + satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4))
}

/// C++: `WelsSampleSatd8x8_c` — four 4x4s: top-left, top-right, bottom-left,
/// bottom-right (the C++'s summation order, kept).
pub fn satd_8x8(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    let mut satd = satd_4x4(c1, c2);
    satd += satd_4x4(&c1.advance(4, 0), &c2.advance(4, 0));
    satd += satd_4x4(&c1.advance(0, 4), &c2.advance(0, 4));
    satd += satd_4x4(&c1.advance(4, 4), &c2.advance(4, 4));
    satd
}

/// C++: `WelsSampleSatd16x8_c` — two 8x8s left-to-right.
pub fn satd_16x8(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0))
}

/// C++: `WelsSampleSatd8x16_c` — two 8x8s top-to-bottom.
pub fn satd_8x16(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    satd_8x8(c1, c2) + satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8))
}

/// C++: `WelsSampleSatd16x16_c` — four 8x8s in the same quadrant order.
pub fn satd_16x16(c1: &PlaneCursor<'_>, c2: &PlaneCursor<'_>) -> i32 {
    let mut satd = satd_8x8(c1, c2);
    satd += satd_8x8(&c1.advance(8, 0), &c2.advance(8, 0));
    satd += satd_8x8(&c1.advance(0, 8), &c2.advance(0, 8));
    satd += satd_8x8(&c1.advance(8, 8), &c2.advance(8, 8));
    satd
}

use crate::common::sad_common::{
    WelsSampleSad16x16_c, WelsSampleSad16x8_c, WelsSampleSad4x4_c, WelsSampleSad4x8_c,
    WelsSampleSad8x16_c, WelsSampleSad8x4_c, WelsSampleSad8x8_c, WelsSampleSadFour16x16_c,
    WelsSampleSadFour16x8_c, WelsSampleSadFour4x4_c, WelsSampleSadFour4x8_c,
    WelsSampleSadFour8x16_c, WelsSampleSadFour8x4_c, WelsSampleSadFour8x8_c,
};
use crate::encoder::svc_mode_decision::{
    BLOCK_16x16, BLOCK_16x8, BLOCK_4x4, BLOCK_4x8, BLOCK_8x16, BLOCK_8x4, BLOCK_8x8,
};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

/// `sample.cpp:48`. Hadamard 4x4 sum of absolute transformed differences.
///
/// # Safety
/// Both sample pointers must be readable for 4 rows at their respective strides.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd4x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSatdSum: i32 = 0;
    let mut pSampleMix = [[0i32; 4]; 4];
    let mut pSrc1 = pSample1;
    let mut pSrc2 = pSample2;

    //step 1: get the difference
    for i in 0..4usize {
        pSampleMix[i][0] = *pSrc1.add(0) as i32 - *pSrc2.add(0) as i32;
        pSampleMix[i][1] = *pSrc1.add(1) as i32 - *pSrc2.add(1) as i32;
        pSampleMix[i][2] = *pSrc1.add(2) as i32 - *pSrc2.add(2) as i32;
        pSampleMix[i][3] = *pSrc1.add(3) as i32 - *pSrc2.add(3) as i32;

        pSrc1 = pSrc1.offset(iStride1 as isize);
        pSrc2 = pSrc2.offset(iStride2 as isize);
    }

    //step 2: horizontal transform
    for i in 0..4usize {
        let iSample0 = pSampleMix[i][0] + pSampleMix[i][2];
        let iSample1 = pSampleMix[i][1] + pSampleMix[i][3];
        let iSample2 = pSampleMix[i][0] - pSampleMix[i][2];
        let iSample3 = pSampleMix[i][1] - pSampleMix[i][3];

        pSampleMix[i][0] = iSample0 + iSample1;
        pSampleMix[i][1] = iSample2 + iSample3;
        pSampleMix[i][2] = iSample2 - iSample3;
        pSampleMix[i][3] = iSample0 - iSample1;
    }

    //step 3: vertical transform and get the sum of SATD
    for i in 0..4usize {
        let iSample0 = pSampleMix[0][i] + pSampleMix[2][i];
        let iSample1 = pSampleMix[1][i] + pSampleMix[3][i];
        let iSample2 = pSampleMix[0][i] - pSampleMix[2][i];
        let iSample3 = pSampleMix[1][i] - pSampleMix[3][i];

        pSampleMix[0][i] = iSample0 + iSample1;
        pSampleMix[1][i] = iSample2 + iSample3;
        pSampleMix[2][i] = iSample2 - iSample3;
        pSampleMix[3][i] = iSample0 - iSample1;

        iSatdSum += pSampleMix[0][i].abs()
            + pSampleMix[1][i].abs()
            + pSampleMix[2][i].abs()
            + pSampleMix[3][i].abs();
    }

    (iSatdSum + 1) >> 1
}

/// `sample.cpp:99`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd8x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    WelsSampleSatd4x4_c(pSample1, iStride1, pSample2, iStride2)
        + WelsSampleSatd4x4_c(pSample1.add(4), iStride1, pSample2.add(4), iStride2)
}

/// `sample.cpp:106`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd4x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    WelsSampleSatd4x4_c(pSample1, iStride1, pSample2, iStride2)
        + WelsSampleSatd4x4_c(
            pSample1.offset((iStride1 << 2) as isize),
            iStride1,
            pSample2.offset((iStride2 << 2) as isize),
            iStride2,
        )
}

/// `sample.cpp:113`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd8x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSatdSum = 0;
    iSatdSum += WelsSampleSatd4x4_c(pSample1, iStride1, pSample2, iStride2);
    iSatdSum += WelsSampleSatd4x4_c(pSample1.add(4), iStride1, pSample2.add(4), iStride2);
    iSatdSum += WelsSampleSatd4x4_c(
        pSample1.offset((iStride1 << 2) as isize),
        iStride1,
        pSample2.offset((iStride2 << 2) as isize),
        iStride2,
    );
    iSatdSum += WelsSampleSatd4x4_c(
        pSample1.offset((iStride1 << 2) as isize).add(4),
        iStride1,
        pSample2.offset((iStride2 << 2) as isize).add(4),
        iStride2,
    );
    iSatdSum
}

/// `sample.cpp:123`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd16x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    WelsSampleSatd8x8_c(pSample1, iStride1, pSample2, iStride2)
        + WelsSampleSatd8x8_c(pSample1.add(8), iStride1, pSample2.add(8), iStride2)
}

/// `sample.cpp:131`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd8x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    WelsSampleSatd8x8_c(pSample1, iStride1, pSample2, iStride2)
        + WelsSampleSatd8x8_c(
            pSample1.offset((iStride1 << 3) as isize),
            iStride1,
            pSample2.offset((iStride2 << 3) as isize),
            iStride2,
        )
}

/// `sample.cpp:139`.
///
/// # Safety
/// See [`WelsSampleSatd4x4_c`].
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSampleSatd16x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSatdSum = 0;
    iSatdSum += WelsSampleSatd8x8_c(pSample1, iStride1, pSample2, iStride2);
    iSatdSum += WelsSampleSatd8x8_c(pSample1.add(8), iStride1, pSample2.add(8), iStride2);
    iSatdSum += WelsSampleSatd8x8_c(
        pSample1.offset((iStride1 << 3) as isize),
        iStride1,
        pSample2.offset((iStride2 << 3) as isize),
        iStride2,
    );
    iSatdSum += WelsSampleSatd8x8_c(
        pSample1.offset((iStride1 << 3) as isize).add(8),
        iStride1,
        pSample2.offset((iStride2 << 3) as isize).add(8),
        iStride2,
    );
    iSatdSum
}

/// `sample.cpp:336`. Installs the scalar SAD/SATD/4-SAD tables and clears the five
/// `Combined3` slots. The SIMD overrides that follow in the C++ are all behind
/// `uiCpuFlag` tests that do not fire here.
///
/// # Safety
/// `pFuncList` must be a valid, writable `SWelsFuncPtrList`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitSampleSadFunc(pFuncList: &mut SWelsFuncPtrList, _uiCpuFlag: u32) {
    let sdf = &mut pFuncList.sSampleDealingFuncs;

    //pfSampleSad init
    sdf.pfSampleSad[BLOCK_16x16] = Some(WelsSampleSad16x16_c);
    sdf.pfSampleSad[BLOCK_16x8] = Some(WelsSampleSad16x8_c);
    sdf.pfSampleSad[BLOCK_8x16] = Some(WelsSampleSad8x16_c);
    sdf.pfSampleSad[BLOCK_8x8] = Some(WelsSampleSad8x8_c);
    sdf.pfSampleSad[BLOCK_4x4] = Some(WelsSampleSad4x4_c);
    sdf.pfSampleSad[BLOCK_8x4] = Some(WelsSampleSad8x4_c);
    sdf.pfSampleSad[BLOCK_4x8] = Some(WelsSampleSad4x8_c);

    //pfSampleSatd init
    sdf.pfSampleSatd[BLOCK_16x16] = Some(WelsSampleSatd16x16_c);
    sdf.pfSampleSatd[BLOCK_16x8] = Some(WelsSampleSatd16x8_c);
    sdf.pfSampleSatd[BLOCK_8x16] = Some(WelsSampleSatd8x16_c);
    sdf.pfSampleSatd[BLOCK_8x8] = Some(WelsSampleSatd8x8_c);
    sdf.pfSampleSatd[BLOCK_4x4] = Some(WelsSampleSatd4x4_c);
    sdf.pfSampleSatd[BLOCK_8x4] = Some(WelsSampleSatd8x4_c);
    sdf.pfSampleSatd[BLOCK_4x8] = Some(WelsSampleSatd4x8_c);

    sdf.pfSample4Sad[BLOCK_16x16] = Some(WelsSampleSadFour16x16_c);
    sdf.pfSample4Sad[BLOCK_16x8] = Some(WelsSampleSadFour16x8_c);
    sdf.pfSample4Sad[BLOCK_8x16] = Some(WelsSampleSadFour8x16_c);
    sdf.pfSample4Sad[BLOCK_8x8] = Some(WelsSampleSadFour8x8_c);
    sdf.pfSample4Sad[BLOCK_4x4] = Some(WelsSampleSadFour4x4_c);
    sdf.pfSample4Sad[BLOCK_8x4] = Some(WelsSampleSadFour8x4_c);
    sdf.pfSample4Sad[BLOCK_4x8] = Some(WelsSampleSadFour4x8_c);

    // The five `pfIntra*Combined3*` slots were nulled here, as the C++ does. They
    // were never anything else on any target this port builds for, and the fields
    // are deleted (S18, Phase 6 session B).
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SATD of a block against itself is zero for every size.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn satd_of_identical_blocks_is_zero() {
        let stride = 24usize;
        let mut a: Vec<u8> = (0..stride * 20).map(|i| ((i * 37) % 256) as u8).collect();
        let mut b = a.clone();
        unsafe {
            let (pa, pb) = (a.as_mut_ptr(), b.as_mut_ptr());
            let s = stride as i32;
            assert_eq!(WelsSampleSatd4x4_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd8x4_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd4x8_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd8x8_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd16x8_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd8x16_c(pa, s, pb, s), 0);
            assert_eq!(WelsSampleSatd16x16_c(pa, s, pb, s), 0);
        }
    }

    /// A constant offset between the two blocks concentrates all the energy in the DC
    /// coefficient: SATD4x4 = |16 * d| / 2 = 8 * |d| after the `(sum + 1) >> 1`.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn satd4x4_of_constant_offset_is_dc_only() {
        let stride = 16usize;
        let mut a = vec![100u8; stride * 8];
        let mut b = vec![107u8; stride * 8];
        unsafe {
            let got = WelsSampleSatd4x4_c(
                a.as_mut_ptr(),
                stride as i32,
                b.as_mut_ptr(),
                stride as i32,
            );
            // difference -7 everywhere -> DC = 16 * -7, all AC zero -> (112 + 1) >> 1
            assert_eq!(got, (16 * 7 + 1) >> 1);
        }
    }

    /// The larger SATDs are exactly the sum of their 4x4 sub-blocks.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn satd_composes_from_4x4_subblocks() {
        let stride = 32usize;
        let mut a: Vec<u8> = (0..stride * 20).map(|i| ((i * 91 + 13) % 256) as u8).collect();
        let mut b: Vec<u8> = (0..stride * 20).map(|i| ((i * 17 + 200) % 256) as u8).collect();
        unsafe {
            let (pa, pb) = (a.as_mut_ptr(), b.as_mut_ptr());
            let s = stride as i32;

            let mut sum8x8 = 0;
            for (dy, dx) in [(0usize, 0usize), (0, 4), (4, 0), (4, 4)] {
                sum8x8 += WelsSampleSatd4x4_c(
                    pa.add(dy * stride + dx),
                    s,
                    pb.add(dy * stride + dx),
                    s,
                );
            }
            assert_eq!(WelsSampleSatd8x8_c(pa, s, pb, s), sum8x8);

            let mut sum16x16 = 0;
            for (dy, dx) in [(0usize, 0usize), (0, 8), (8, 0), (8, 8)] {
                sum16x16 += WelsSampleSatd8x8_c(
                    pa.add(dy * stride + dx),
                    s,
                    pb.add(dy * stride + dx),
                    s,
                );
            }
            assert_eq!(WelsSampleSatd16x16_c(pa, s, pb, s), sum16x16);
        }
    }

    /// Every slot the mode-decision layer indexes must be filled, and the five
    /// `Combined3` slots must be left NULL — `svc_base_layer_md` asserts on that.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn init_fills_sad_and_satd_and_clears_combined3() {
        // Zeroing this table is sound for the reason its own `Default` gives
        // (`wels_func_ptr_def.rs`, S21); session I converts both with the dispatch
        // tables. T6.H12 enumerated it here rather than leaving it to a grep.
        let mut fl = SWelsFuncPtrList::default();
        unsafe { WelsInitSampleSadFunc(&mut fl, 0) };

        for b in [BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4, BLOCK_4x8] {
            assert!(fl.sSampleDealingFuncs.pfSampleSad[b].is_some(), "sad[{b}]");
            assert!(fl.sSampleDealingFuncs.pfSampleSatd[b].is_some(), "satd[{b}]");
            assert!(fl.sSampleDealingFuncs.pfSample4Sad[b].is_some(), "sad4[{b}]");
        }
    }
}
