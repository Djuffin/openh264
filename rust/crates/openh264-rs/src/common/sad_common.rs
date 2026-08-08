#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

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

//=================== Single-Block SAD Functions =====================//

/// Computes the Sum of Absolute Differences for a 4x4 block.
///
/// Matches `int32_t WelsSampleSad4x4_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// `pSample1` and `pSample2` must point to valid readable pixel buffers of at least 4 rows with
/// the specified strides.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad4x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    let mut pSrc1 = pSample1;
    let mut pSrc2 = pSample2;

    unsafe {
        for _ in 0..4 {
            iSadSum += (*pSrc1.add(0) as i32 - *pSrc2.add(0) as i32).abs();
            iSadSum += (*pSrc1.add(1) as i32 - *pSrc2.add(1) as i32).abs();
            iSadSum += (*pSrc1.add(2) as i32 - *pSrc2.add(2) as i32).abs();
            iSadSum += (*pSrc1.add(3) as i32 - *pSrc2.add(3) as i32).abs();

            pSrc1 = pSrc1.offset(iStride1 as isize);
            pSrc2 = pSrc2.offset(iStride2 as isize);
        }
    }

    iSadSum
}

/// Computes the Sum of Absolute Differences for an 8x4 block.
///
/// Matches `int32_t WelsSampleSad8x4_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad8x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    unsafe {
        iSadSum += WelsSampleSad4x4_c(pSample1, iStride1, pSample2, iStride2);
        iSadSum += WelsSampleSad4x4_c(pSample1.add(4), iStride1, pSample2.add(4), iStride2);
    }
    iSadSum
}

/// Computes the Sum of Absolute Differences for a 4x8 block.
///
/// Matches `int32_t WelsSampleSad4x8_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad4x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    unsafe {
        iSadSum += WelsSampleSad4x4_c(pSample1, iStride1, pSample2, iStride2);
        iSadSum += WelsSampleSad4x4_c(
            pSample1.offset((iStride1 << 2) as isize),
            iStride1,
            pSample2.offset((iStride2 << 2) as isize),
            iStride2,
        );
    }
    iSadSum
}

/// Computes the Sum of Absolute Differences for an 8x8 block.
///
/// Matches `int32_t WelsSampleSad8x8_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad8x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    let mut pSrc1 = pSample1;
    let mut pSrc2 = pSample2;

    unsafe {
        for _ in 0..8 {
            iSadSum += (*pSrc1.add(0) as i32 - *pSrc2.add(0) as i32).abs();
            iSadSum += (*pSrc1.add(1) as i32 - *pSrc2.add(1) as i32).abs();
            iSadSum += (*pSrc1.add(2) as i32 - *pSrc2.add(2) as i32).abs();
            iSadSum += (*pSrc1.add(3) as i32 - *pSrc2.add(3) as i32).abs();
            iSadSum += (*pSrc1.add(4) as i32 - *pSrc2.add(4) as i32).abs();
            iSadSum += (*pSrc1.add(5) as i32 - *pSrc2.add(5) as i32).abs();
            iSadSum += (*pSrc1.add(6) as i32 - *pSrc2.add(6) as i32).abs();
            iSadSum += (*pSrc1.add(7) as i32 - *pSrc2.add(7) as i32).abs();

            pSrc1 = pSrc1.offset(iStride1 as isize);
            pSrc2 = pSrc2.offset(iStride2 as isize);
        }
    }

    iSadSum
}

/// Computes the Sum of Absolute Differences for a 16x8 block.
///
/// Matches `int32_t WelsSampleSad16x8_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad16x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    unsafe {
        iSadSum += WelsSampleSad8x8_c(pSample1, iStride1, pSample2, iStride2);
        iSadSum += WelsSampleSad8x8_c(pSample1.add(8), iStride1, pSample2.add(8), iStride2);
    }
    iSadSum
}

/// Computes the Sum of Absolute Differences for an 8x16 block.
///
/// Matches `int32_t WelsSampleSad8x16_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad8x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    unsafe {
        iSadSum += WelsSampleSad8x8_c(pSample1, iStride1, pSample2, iStride2);
        iSadSum += WelsSampleSad8x8_c(
            pSample1.offset((iStride1 << 3) as isize),
            iStride1,
            pSample2.offset((iStride2 << 3) as isize),
            iStride2,
        );
    }
    iSadSum
}

/// Computes the Sum of Absolute Differences for a 16x16 macroblock.
///
/// Matches `int32_t WelsSampleSad16x16_c (uint8_t* pSample1, int32_t iStride1, uint8_t* pSample2, int32_t iStride2)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid readable pointers for `pSample1` and `pSample2`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSad16x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    let mut iSadSum: i32 = 0;
    unsafe {
        iSadSum += WelsSampleSad8x8_c(pSample1, iStride1, pSample2, iStride2);
        iSadSum += WelsSampleSad8x8_c(pSample1.add(8), iStride1, pSample2.add(8), iStride2);
        iSadSum += WelsSampleSad8x8_c(
            pSample1.offset((iStride1 << 3) as isize),
            iStride1,
            pSample2.offset((iStride2 << 3) as isize),
            iStride2,
        );
        iSadSum += WelsSampleSad8x8_c(
            pSample1.offset((iStride1 << 3) as isize).add(8),
            iStride1,
            pSample2.offset((iStride2 << 3) as isize).add(8),
            iStride2,
        );
    }
    iSadSum
}

//=================== 4-Directional Diamond SAD Functions =====================//

/// Computes 4-point directional diamond search SAD for 16x16 macroblocks (Top, Bottom, Left, Right).
///
/// Matches `void WelsSampleSadFour16x16_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// `iSample1`, `iSample2`, and `pSad` (must have at least 4 elements) must be valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour16x16_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad16x16_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad16x16_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad16x16_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad16x16_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 16x8 blocks.
///
/// Matches `void WelsSampleSadFour16x8_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour16x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad16x8_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad16x8_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad16x8_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad16x8_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 8x16 blocks.
///
/// Matches `void WelsSampleSadFour8x16_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour8x16_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad8x16_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad8x16_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad8x16_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad8x16_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 8x8 blocks.
///
/// Matches `void WelsSampleSadFour8x8_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour8x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad8x8_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad8x8_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad8x8_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad8x8_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 4x4 blocks.
///
/// Matches `void WelsSampleSadFour4x4_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour4x4_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad4x4_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad4x4_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad4x4_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad4x4_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 8x4 blocks.
///
/// Matches `void WelsSampleSadFour8x4_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour8x4_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad8x4_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad8x4_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad8x4_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad8x4_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

/// Computes 4-point directional diamond search SAD for 4x8 blocks.
///
/// Matches `void WelsSampleSadFour4x8_c (uint8_t* iSample1, int32_t iStride1, uint8_t* iSample2, int32_t iStride2, int32_t* pSad)`
/// in `sad_common.cpp`.
///
/// # Safety
/// Requires valid pointers for `iSample1`, `iSample2`, and `pSad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsSampleSadFour4x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    unsafe {
        *pSad.add(0) = WelsSampleSad4x8_c(iSample1, iStride1, iSample2.offset(-(iStride2 as isize)), iStride2);
        *pSad.add(1) = WelsSampleSad4x8_c(iSample1, iStride1, iSample2.offset(iStride2 as isize), iStride2);
        *pSad.add(2) = WelsSampleSad4x8_c(iSample1, iStride1, iSample2.offset(-1), iStride2);
        *pSad.add(3) = WelsSampleSad4x8_c(iSample1, iStride1, iSample2.offset(1), iStride2);
    }
}

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
    for y in 0..H as isize {
        // Statically-sized row windows on both sides: the bounds check lands per row,
        // and the inner loop's trip count is a constant, so it vectorises.
        let a: &[u8; W] = sample1.row(y, 0, W).try_into().unwrap();
        let b: &[u8; W] = sample2.row(y + dy, dx, W).try_into().unwrap();
        for (p, q) in a.iter().zip(b.iter()) {
            sum += (*p as i32 - *q as i32).abs();
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

    #[test]
    fn test_sample_sad_4x4_identical() {
        let mut buf = [42u8; 64];
        unsafe {
            let sad = WelsSampleSad4x4_c(buf.as_mut_ptr(), 8, buf.as_mut_ptr(), 8);
            assert_eq!(sad, 0);
        }
    }

    #[test]
    fn test_sample_sad_4x4_diff() {
        let mut buf1 = [10u8; 16];
        let mut buf2 = [20u8; 16];
        unsafe {
            let sad = WelsSampleSad4x4_c(buf1.as_mut_ptr(), 4, buf2.as_mut_ptr(), 4);
            assert_eq!(sad, 16 * 10);
        }
    }

    #[test]
    fn test_sample_sad_8x8_diff() {
        let mut buf1 = [5u8; 64];
        let mut buf2 = [15u8; 64];
        unsafe {
            let sad = WelsSampleSad8x8_c(buf1.as_mut_ptr(), 8, buf2.as_mut_ptr(), 8);
            assert_eq!(sad, 64 * 10);
        }
    }

    #[test]
    fn test_sample_sad_16x16_diff() {
        let mut buf1 = [0u8; 256];
        let mut buf2 = [2u8; 256];
        unsafe {
            let sad = WelsSampleSad16x16_c(buf1.as_mut_ptr(), 16, buf2.as_mut_ptr(), 16);
            assert_eq!(sad, 256 * 2);
        }
    }

    #[test]
    fn test_sample_sad_partitions() {
        let mut buf1: Vec<u8> = (0..512).map(|x| (x % 255) as u8).collect();
        let mut buf2: Vec<u8> = (0..512).map(|x| ((x + 5) % 255) as u8).collect();

        unsafe {
            let s8x4 = WelsSampleSad8x4_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32);
            let s4x8 = WelsSampleSad4x8_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32);
            let s16x8 = WelsSampleSad16x8_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32);
            let s8x16 = WelsSampleSad8x16_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32);
            let s16x16 = WelsSampleSad16x16_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32);

            assert!(s8x4 > 0);
            assert!(s4x8 > 0);
            assert_eq!(
                s16x8,
                WelsSampleSad8x8_c(buf1.as_mut_ptr(), 32, buf2.as_mut_ptr(), 32)
                    + WelsSampleSad8x8_c(buf1.as_mut_ptr().add(8), 32, buf2.as_mut_ptr().add(8), 32)
            );
        }
    }

    #[test]
    fn test_sample_sad_four_16x16() {
        let stride = 64;
        let mut buf1 = vec![100u8; stride * 32];
        let mut buf2 = vec![100u8; stride * 32];

        let center_offset = stride * 10 + 10;
        let p_center = unsafe { buf2.as_mut_ptr().add(center_offset) };

        let mut sad_results = [0i32; 4];
        unsafe {
            WelsSampleSadFour16x16_c(buf1.as_mut_ptr(), stride as i32, p_center, stride as i32, sad_results.as_mut_ptr());
        }
        assert_eq!(sad_results, [0, 0, 0, 0]);
    }
}
