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

/// C++: `WelsSampleSad4x4_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 4 x 4 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 3 * iStrideN + 4)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 4.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad4x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<4, 4>
    unsafe { shim_sad::<4, 4>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad8x4_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 8 x 4 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 3 * iStrideN + 8)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 8.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad8x4_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<8, 4>
    unsafe { shim_sad::<8, 4>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad4x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 4 x 8 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 7 * iStrideN + 4)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 4.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad4x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<4, 8>
    unsafe { shim_sad::<4, 8>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad8x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 8 x 8 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 7 * iStrideN + 8)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 8.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad8x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<8, 8>
    unsafe { shim_sad::<8, 8>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad16x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 16 x 8 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 7 * iStrideN + 16)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 16.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad16x8_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<16, 8>
    unsafe { shim_sad::<16, 8>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad8x16_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 8 x 16 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 15 * iStrideN + 8)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 8.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad8x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<8, 16>
    unsafe { shim_sad::<8, 16>(pSample1, iStride1, pSample2, iStride2) }
}

/// C++: `WelsSampleSad16x16_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `pSample1` and `pSample2` point at sample `(0, 0)` of a 16 x 16 block in
///   surfaces whose rows are `iStride1` / `iStride2` bytes apart. Reads span
///   `[0, 15 * iStrideN + 16)` from each, and nothing outside the block.
/// * Both strides must be positive and at least 16.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSad16x16_c(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32 {
    // SHIM(phase2) -> sample_sad::<16, 16>
    unsafe { shim_sad::<16, 16>(pSample1, iStride1, pSample2, iStride2) }
}

//=================== 4-Directional Diamond SAD Functions =====================//

/// C++: `WelsSampleSadFour4x4_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 4 x 4 block; reads span
///   `[0, 3 * iStride1 + 4)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 4 * iStride2 + 4)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 4 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour4x4_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<4, 4>
    unsafe { shim_sad_four::<4, 4>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour8x4_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 8 x 4 block; reads span
///   `[0, 3 * iStride1 + 8)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 4 * iStride2 + 8)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 8 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour8x4_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<8, 4>
    unsafe { shim_sad_four::<8, 4>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour4x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 4 x 8 block; reads span
///   `[0, 7 * iStride1 + 4)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 8 * iStride2 + 4)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 4 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour4x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<4, 8>
    unsafe { shim_sad_four::<4, 8>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour8x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 8 x 8 block; reads span
///   `[0, 7 * iStride1 + 8)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 8 * iStride2 + 8)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 8 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour8x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<8, 8>
    unsafe { shim_sad_four::<8, 8>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour16x8_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 16 x 8 block; reads span
///   `[0, 7 * iStride1 + 16)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 8 * iStride2 + 16)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 16 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour16x8_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<16, 8>
    unsafe { shim_sad_four::<16, 8>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour8x16_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 8 x 16 block; reads span
///   `[0, 15 * iStride1 + 8)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 16 * iStride2 + 8)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 8 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour8x16_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<8, 16>
    unsafe { shim_sad_four::<8, 16>(iSample1, iStride1, iSample2, iStride2, pSad) }
}

/// C++: `WelsSampleSadFour16x16_c`, `codec/common/src/sad_common.cpp`.
///
/// # Safety
/// * `iSample1` points at sample `(0, 0)` of a 16 x 16 block; reads span
///   `[0, 15 * iStride1 + 16)`, the block and nothing more.
/// * `iSample2` points at sample `(0, 0)` of the block the diamond is centred on, and
///   the search steps one whole sample in each direction, so reads span
///   **`[-iStride2, 16 * iStride2 + 16)`** — one row above the block and one below
///   it. The left arm's `-1` lies inside `-iStride2` for any stride of 1 or more; the
///   down arm's last row is what sets the far end.
/// * `pSad` points at **four** writable `i32`. Exactly four are written.
/// * Both strides must be positive and at least 16 + 2.
#[inline(always)]
pub unsafe extern "C" fn WelsSampleSadFour16x16_c(
    iSample1: *mut u8,
    iStride1: i32,
    iSample2: *mut u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    // SHIM(phase2) -> sample_sad_four::<16, 16>
    unsafe { shim_sad_four::<16, 16>(iSample1, iStride1, iSample2, iStride2, pSad) }
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

//=================== Shim span arithmetic =====================//

// Two helpers rather than the span arithmetic written out fourteen times. This costs
// the ratchet a handful of `*const`/`*mut` occurrences it would not otherwise have
// (R-f: judge the shape, not the sign — T4 paid the same +2 for `shim_wh`), and buys
// having the reach derivation in one place where it can be read against the contracts.

/// Spans for a single-block SAD shim: both surfaces are read over the nominal block
/// only, `(H - 1) * stride + W` bytes from each sample pointer.
///
/// # Safety
/// Exactly the contract each shim states: both pointers address those spans, both
/// strides are positive, and neither span is aliased by a live `&mut`.
#[inline(always)]
unsafe fn shim_sad<const W: usize, const H: usize>(
    pSample1: *const u8,
    iStride1: i32,
    pSample2: *const u8,
    iStride2: i32,
) -> i32 {
    let (s1, s2) = (iStride1 as usize, iStride2 as usize);
    let (b1, b2) = unsafe {
        (
            std::slice::from_raw_parts(pSample1, (H - 1) * s1 + W),
            std::slice::from_raw_parts(pSample2, (H - 1) * s2 + W),
        )
    };
    sample_sad::<W, H>(&PlaneCursor::new(b1, 0, s1), &PlaneCursor::new(b2, 0, s2))
}

/// Spans for a four-point SAD shim. `pSample1` spans its block; `pSample2` spans
/// `[-iStride2, H * iStride2 + W)` — one row above the block and one below — because
/// the diamond's arms leave it. The `-1` of the left arm is inside `-iStride2` for any
/// stride of 1 or more, so the row above is what sets the near end, and the down arm's
/// last row sets the far end.
///
/// # Safety
/// As above, plus: `pSad` addresses four writable, properly aligned `i32`.
#[inline(always)]
unsafe fn shim_sad_four<const W: usize, const H: usize>(
    pSample1: *const u8,
    iStride1: i32,
    pSample2: *const u8,
    iStride2: i32,
    pSad: *mut i32,
) {
    let (s1, s2) = (iStride1 as usize, iStride2 as usize);
    let (b1, b2) = unsafe {
        (
            std::slice::from_raw_parts(pSample1, (H - 1) * s1 + W),
            std::slice::from_raw_parts(pSample2.sub(s2), (H + 1) * s2 + W),
        )
    };
    // `&mut [i32; 4]` rather than a slice: the safe kernel then cannot write a fifth,
    // and the count stops being a thing the shim has to remember.
    let sad = unsafe { &mut *pSad.cast::<[i32; 4]>() };
    sample_sad_four::<W, H>(
        &PlaneCursor::new(b1, 0, s1),
        &PlaneCursor::new(b2, s2, s2),
        sad,
    );
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
