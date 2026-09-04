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
pub fn WelsInitSampleSadFunc(pFuncList: &mut SWelsFuncPtrList, uiCpuFlag: u32) {
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

    #[cfg(target_arch = "x86_64")]
    if (uiCpuFlag & crate::common::cpu_core::WELS_CPU_SSE2) != 0 {
        sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_16x16_sse2(a, b));
        sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_16x8_sse2(a, b));
        sdf.pfSampleSad[BLOCK_8x16] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_8x16_sse2(a, b));
        sdf.pfSampleSad[BLOCK_8x8] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_8x8_sse2(a, b));
        // **The three small shapes upstream leaves scalar on x86, and why they are not.**
        // `BLOCK_4x4` is a gap against upstream, which installs `WelsSampleSad4x4_mmx`
        // here (`sample.cpp`'s `X86_ASM` arm). `BLOCK_8x4` and `BLOCK_4x8` have no x86
        // kernel upstream at all, so filling them puts this port ahead of it — which is
        // safe to do because SAD is an exact integer cost: the parity tests in
        // `simd::x86_64::sad` assert these agree with `sample_sad::<W, H>` bit for bit
        // over five input distributions and four anchors, so mode decision sees the
        // same numbers and picks the same modes. The kernels were already written and
        // tested; only the table entry was missing.
        sdf.pfSampleSad[BLOCK_4x4] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_4x4_sse2(a, b));
        sdf.pfSampleSad[BLOCK_8x4] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_8x4_sse2(a, b));
        sdf.pfSampleSad[BLOCK_4x8] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_4x8_sse2(a, b));

        sdf.pfSample4Sad[BLOCK_16x16] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_16x16_sse2(a, b, sad));
        sdf.pfSample4Sad[BLOCK_16x8] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_16x8_sse2(a, b, sad));
        sdf.pfSample4Sad[BLOCK_8x16] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_8x16_sse2(a, b, sad));
        sdf.pfSample4Sad[BLOCK_8x8] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_8x8_sse2(a, b, sad));
        sdf.pfSample4Sad[BLOCK_4x4] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_4x4_sse2(a, b, sad));
        // No upstream x86 kernel for these two shapes either; see the note above.
        sdf.pfSample4Sad[BLOCK_8x4] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_8x4_sse2(a, b, sad));
        sdf.pfSample4Sad[BLOCK_4x8] = Some(|a, b, sad| crate::simd::x86_64::sad::sample_sad_four_4x8_sse2(a, b, sad));

        sdf.pfSampleSatd[BLOCK_4x4] = Some(|a, b| crate::simd::x86_64::satd::satd_4x4_sse2(a, b));
        sdf.pfSampleSatd[BLOCK_8x8] = Some(|a, b| crate::simd::x86_64::satd::satd_8x8_sse2(a, b));
        sdf.pfSampleSatd[BLOCK_8x16] = Some(|a, b| crate::simd::x86_64::satd::satd_8x16_sse2(a, b));
        sdf.pfSampleSatd[BLOCK_16x8] = Some(|a, b| crate::simd::x86_64::satd::satd_16x8_sse2(a, b));
        sdf.pfSampleSatd[BLOCK_16x16] = Some(|a, b| crate::simd::x86_64::satd::satd_16x16_sse2(a, b));
        // As above: no upstream x86 SATD for 8x4 or 4x8, and SATD is an exact integer
        // cost, so installing the port's own is byte-neutral.
        sdf.pfSampleSatd[BLOCK_8x4] = Some(|a, b| crate::simd::x86_64::satd::satd_8x4_sse2(a, b));
        sdf.pfSampleSatd[BLOCK_4x8] = Some(|a, b| crate::simd::x86_64::satd::satd_4x8_sse2(a, b));
    }

    // **Two conditions, and they are different questions.** `uiCpuFlag` is the host's
    // policy — a caller may restrict it, and `svc_mode_decision.rs:2371` passes `0` —
    // while `has_avx2()` is the hardware fact. The kernels below are
    // `#[target_feature(enable = "avx2")]` underneath and run `vpsadbw` with no test of
    // their own, so the flag alone is not enough to install them: nothing stops a caller
    // passing a word it made up, and the result would be SIGILL on a pre-Haswell part.
    //
    // This is the altitude the test belongs at. It is asked once, here, when the table
    // is built — not on every candidate the mode-decision loop scores.
    #[cfg(target_arch = "x86_64")]
    if (uiCpuFlag & crate::common::cpu_core::WELS_CPU_AVX2) != 0 && crate::simd::has_avx2() {
        sdf.pfSampleSad[BLOCK_16x16] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_16x16_avx2(a, b));
        sdf.pfSampleSad[BLOCK_16x8] = Some(|a, b| crate::simd::x86_64::sad::sample_sad_16x8_avx2(a, b));
    }

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

    /// **Every SSE2 slot is actually SSE2.** A kernel with a green parity test and no
    /// table entry looks exactly like one in use, so nothing else here would notice a
    /// slot quietly handing out the scalar.
    ///
    /// The table is compared against itself: under `WELS_CPU_SSE2` every one of these
    /// slots must hold a *different* function than at `uiCpuFlag == 0`, so a slot that
    /// stops being wired fails by name.
    ///
    /// Function-pointer identity is the comparison, with the caveat on
    /// `common/mc.rs`'s `init_mc_func_cpu_flags`: both addresses come from the same
    /// `WelsInitSampleSadFunc` instantiation, and Miri mints a fresh synthetic address
    /// per reification, so it is excluded.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn every_sse2_sad_and_satd_slot_is_wired() {
        const SHAPES: [(usize, &str); 7] = [
            (BLOCK_16x16, "16x16"),
            (BLOCK_16x8, "16x8"),
            (BLOCK_8x16, "8x16"),
            (BLOCK_8x8, "8x8"),
            (BLOCK_4x4, "4x4"),
            (BLOCK_8x4, "8x4"),
            (BLOCK_4x8, "4x8"),
        ];

        let mut scalar = SWelsFuncPtrList::default();
        WelsInitSampleSadFunc(&mut scalar, 0);
        let mut sse2 = SWelsFuncPtrList::default();
        WelsInitSampleSadFunc(&mut sse2, crate::common::cpu_core::WELS_CPU_SSE2);

        let (a, b) = (&scalar.sSampleDealingFuncs, &sse2.sSampleDealingFuncs);
        for (slot, name) in SHAPES {
            assert_ne!(
                a.pfSampleSad[slot].map(|f| f as usize),
                b.pfSampleSad[slot].map(|f| f as usize),
                "pfSampleSad[{name}] is the same function with and without SSE2"
            );
            assert_ne!(
                a.pfSample4Sad[slot].map(|f| f as usize),
                b.pfSample4Sad[slot].map(|f| f as usize),
                "pfSample4Sad[{name}] is the same function with and without SSE2"
            );
            assert_ne!(
                a.pfSampleSatd[slot].map(|f| f as usize),
                b.pfSampleSatd[slot].map(|f| f as usize),
                "pfSampleSatd[{name}] is the same function with and without SSE2"
            );
        }
    }

    /// Every slot the mode-decision layer indexes must be filled, and the five
    /// `Combined3` slots must be left NULL — `svc_base_layer_md` asserts on that.
    #[test]
    fn init_fills_sad_and_satd_and_clears_combined3() {
        for flags in [
            0,
            crate::common::cpu_core::WELS_CPU_SSE2,
            crate::common::cpu_core::WELS_CPU_SSE2 | crate::common::cpu_core::WELS_CPU_AVX2,
        ] {
            let mut fl = SWelsFuncPtrList::default();
            WelsInitSampleSadFunc(&mut fl, flags);

            for b in [BLOCK_16x16, BLOCK_16x8, BLOCK_8x16, BLOCK_8x8, BLOCK_4x4, BLOCK_8x4, BLOCK_4x8] {
                assert!(fl.sSampleDealingFuncs.pfSampleSad[b].is_some(), "sad[{b}] flags={flags:#x}");
                assert!(fl.sSampleDealingFuncs.pfSampleSatd[b].is_some(), "satd[{b}] flags={flags:#x}");
                assert!(fl.sSampleDealingFuncs.pfSample4Sad[b].is_some(), "sad4[{b}] flags={flags:#x}");
            }
        }
    }
}
