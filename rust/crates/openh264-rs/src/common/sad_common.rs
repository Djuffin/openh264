#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! Sum of Absolute Differences (SAD) distortion calculation engine.
//!
//! Translated from `codec/common/inc/sad_common.h` and `codec/common/src/sad_common.cpp`.

// `PSampleSadSatdCostFunc` and `PSample4SadCostFunc` were declared here too, the
// former with `*const u8` parameters where `wels_func_ptr_def.h:127` says `uint8_t*`
// -- a fifth identity for that alias, and a distinct function type from the one the
// SSampleDealingFunc tables hold. The canonical declarations are
// `encoder::md::PSampleSadSatdCostFunc` and
// `encoder::svc_motion_estimate::PSample4SadCostFunc`; re-exported rather than
// redeclared so there stays one definition each.
pub use crate::encoder::md::PSampleSadSatdCostFunc;
pub use crate::encoder::svc_motion_estimate::PSample4SadCostFunc;

/// Block partition types matching OpenH264's `Sub_Block_Multiple_T`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SubBlockMultiple {
    BLOCK_16x16 = 0,
    BLOCK_16x8 = 1,
    BLOCK_8x16 = 2,
    BLOCK_8x8 = 3,
    BLOCK_4x4 = 4,
    BLOCK_8x4 = 5,
    BLOCK_4x8 = 6,
    BLOCK_SIZE_ALL = 7,
}

/// Absolute difference helper matching `WELS_ABS` in `macros.h`.
#[inline(always)]
pub fn WELS_ABS(iX: i32) -> i32 {
    iX.abs()
}

// **T8.C3 — the fourteen `#[unsafe(no_mangle)]` attributes in this file are deleted.**
//
// They were transliteration residue: the C++ has these names because it links one
// object file per translation unit, and nothing in this crate ever reached them by
// symbol. Every caller is a Rust path (`sad::WelsSampleSad4x4_c` in
// `tests/kernels_differential_phase2.rs` and `benches/sad_bodies_bench.rs`), and the
// dispatch tables hold function *items*.
//
// With `crate-type = ["cdylib", ...]` they stopped being harmless: a Rust cdylib
// exports exactly its `#[no_mangle] pub extern` items, so these fourteen plus
// `set_mb_syn_cabac.rs`'s three appeared in `nm -gU` beside the seven upstream names
// — **and they are the same names `libopenh264` exports**, so a consumer that loaded
// both libraries would have had one interpose on the other. `tools/abi_exports.sh` is
// the gate that will not let them come back.

// The fourteen raw `WelsSampleSad*_c` / `WelsSampleSadFour*_c` shims stood
// here (the "Single-Block" and "4-Directional Diamond" sections) — the parked
// raw half of the SAD family, installed only into T9.B25's transitional raw
// tables after B2-B4 flipped the production tables safe. Session F converted
// the tables' last readers (the ME family, the chroma P-skip check, and the
// dormant SCD sites) and deleted the tables, so the raw bodies went per the
// park's own charter: `sample_sad::<W, H>` / `sample_sad_four::<W, H>` below
// are the family, refereed by the 583-row byte gates on every commit. This
// file carries `#![deny(unsafe_code)]` with zero allows from this commit on.

//=================== Safe kernels =====================//

// The three `sample_sad_4x4/8x8/16x16` wrappers that used to sit here were a second,
// unused SAD API — a `(&[u8], stride)` pair per surface, three fixed shapes, no
// four-point form, and no caller anywhere in the tree. They are absorbed into the two
// const-generic kernels below rather than left beside them: one safe SAD API, not two.
//
// **Why the composites flatten.** The raw kernels build the larger shapes out of the
// smaller ones — `WelsSampleSad16x16_c` sums four 8x8 quadrants, `WelsSampleSad8x4_c`
// sums two 4x4 halves — and the safe side computes each shape in one pass instead.
// That is not an approximation. The summands are the same set of `|a - b|` terms and
// `i32` addition is associative, so the only way regrouping could change the result is
// overflow; the largest shape sums 16 x 16 terms of at most 255, i.e. **65 280**, four
// orders of magnitude inside `i32`. No grouping of these operands can overflow, which
// is what makes the flattening exact rather than merely close (contrast the 8x8 IDCT,
// `phase2_findings.md` F8, where the intermediates *can* overflow and the port
// therefore reproduces the old grouping operation for operation).

use crate::safe::plane::PlaneCursor;

/// Sum of absolute differences between a `W` x `H` block at `sample1` and one at
/// `sample2` displaced by `(dx, dy)`.
///
/// The displacement is what the four-point kernels need and is a parameter rather than
/// four rebased cursors because `PlaneCursor::advance` re-runs the anchor assertion:
/// folding the offset into the row lookup keeps the four probes at one bounds check
/// per row each, which is where this family's checks are meant to land (plan §7.4).
#[inline(always)]
fn sad_at<const W: usize, const H: usize>(
    sample1: &PlaneCursor<'_>,
    sample2: &PlaneCursor<'_>,
    dx: isize,
    dy: isize,
) -> i32 {
    let mut sum: i32 = 0;
    // One bounds check per block per side, not two per row per side. Through a shim
    // neither the stride nor the buffer length is a compile-time value, so a per-row
    // `row()` walk cannot fold its checks and a 16x8 emits 32 branches before reading
    // a sample — see `PlaneCursor::row_windows` for the measurement and for why this
    // is the right call here and the wrong one in `mc.rs`.
    let rows1 = sample1.row_windows::<W>(0, 0, H);
    let rows2 = sample2.row_windows::<W>(dy, dx, H);
    for (a, b) in rows1.zip(rows2) {
        for (p, q) in a.iter().zip(b.iter()) {
            sum += p.abs_diff(*q) as i32;
        }
    }
    sum
}

/// C++: `WelsSampleSad<W>x<H>_c`, `codec/common/src/sad_common.cpp` — the seven
/// single-block SAD shapes, which differ only in `W` and `H`.
///
/// Reads `x` in `0 .. W` and `y` in `0 .. H` from both cursors, and nothing else.
#[inline(always)]
pub fn sample_sad<const W: usize, const H: usize>(
    sample1: &PlaneCursor<'_>,
    sample2: &PlaneCursor<'_>,
) -> i32 {
    sad_at::<W, H>(sample1, sample2, 0, 0)
}

/// C++: `WelsSampleSadFour<W>x<H>_c`, `codec/common/src/sad_common.cpp` — the SAD of
/// `sample1`'s block against `sample2`'s at each of the four whole-sample neighbours
/// the diamond search steps to, in the order the caller indexes them: **up, down,
/// left, right**.
///
/// `sample1` is read over its nominal block only. `sample2` is read one row above and
/// one row below it, and one column either side: `x` in `-1 .. W + 1`, `y` in
/// `-1 .. H + 1`. That reach is the whole reason this kernel takes a plane cursor and
/// not a block slice — the diamond's arms leave the block.
#[inline(always)]
pub fn sample_sad_four<const W: usize, const H: usize>(
    sample1: &PlaneCursor<'_>,
    sample2: &PlaneCursor<'_>,
    sad: &mut [i32; 4],
) {
    sad[0] = sad_at::<W, H>(sample1, sample2, 0, -1);
    sad[1] = sad_at::<W, H>(sample1, sample2, 0, 1);
    sad[2] = sad_at::<W, H>(sample1, sample2, -1, 0);
    sad[3] = sad_at::<W, H>(sample1, sample2, 1, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wels_abs() {
        assert_eq!(WELS_ABS(-10), 10);
        assert_eq!(WELS_ABS(15), 15);
        assert_eq!(WELS_ABS(0), 0);
    }

    // Session F: these value tests drove the raw `WelsSampleSad*_c` shims
    // (deleted with T9.B25's transitional tables); the same values now pin the
    // safe kernels — which also ends F10's third instance for good: the raw
    // bodies' trailing `pSrc += iStride` needed `(h + 1) * stride` buffers,
    // and the safe kernels read exactly `W` x `H`.
    #[test]
    fn test_sample_sad_4x4_identical() {
        let buf = [42u8; 64];
        let c = PlaneCursor::new(&buf, 0, 8);
        assert_eq!(sample_sad::<4, 4>(&c, &c), 0);
    }

    #[test]
    fn test_sample_sad_4x4_diff() {
        let buf1 = [10u8; 16];
        let buf2 = [20u8; 16];
        let sad = sample_sad::<4, 4>(&PlaneCursor::new(&buf1, 0, 4), &PlaneCursor::new(&buf2, 0, 4));
        assert_eq!(sad, 16 * 10);
    }

    #[test]
    fn test_sample_sad_8x8_diff() {
        let buf1 = [5u8; 64];
        let buf2 = [15u8; 64];
        let sad = sample_sad::<8, 8>(&PlaneCursor::new(&buf1, 0, 8), &PlaneCursor::new(&buf2, 0, 8));
        assert_eq!(sad, 64 * 10);
    }

    #[test]
    fn test_sample_sad_16x16_diff() {
        let buf1 = [0u8; 16 * 16];
        let buf2 = [2u8; 16 * 16];
        let sad =
            sample_sad::<16, 16>(&PlaneCursor::new(&buf1, 0, 16), &PlaneCursor::new(&buf2, 0, 16));
        assert_eq!(sad, 256 * 2);
    }

    #[test]
    fn test_sample_sad_partitions() {
        let buf1: Vec<u8> = (0..16 * 32).map(|x| (x % 255) as u8).collect();
        let buf2: Vec<u8> = (0..16 * 32).map(|x| ((x + 5) % 255) as u8).collect();
        let c1 = PlaneCursor::new(&buf1, 0, 32);
        let c2 = PlaneCursor::new(&buf2, 0, 32);

        assert!(sample_sad::<8, 4>(&c1, &c2) > 0);
        assert!(sample_sad::<4, 8>(&c1, &c2) > 0);
        assert_eq!(
            sample_sad::<16, 8>(&c1, &c2),
            sample_sad::<8, 8>(&c1, &c2)
                + sample_sad::<8, 8>(&c1.advance(8, 0), &c2.advance(8, 0))
        );
    }

    #[test]
    fn test_sample_sad_four_16x16() {
        let stride = 64;
        let buf1 = vec![100u8; stride * 32];
        let buf2 = vec![100u8; stride * 32];

        let mut sad_results = [0i32; 4];
        sample_sad_four::<16, 16>(
            &PlaneCursor::new(&buf1, 0, stride),
            &PlaneCursor::new(&buf2, stride * 10 + 10, stride),
            &mut sad_results,
        );
        assert_eq!(sad_results, [0, 0, 0, 0]);
    }
}
