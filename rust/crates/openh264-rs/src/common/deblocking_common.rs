#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

//! H.264 / AVC In-Loop Adaptive Deblocking Filter Primitives.
//!
//! C++: `codec/common/src/deblocking_common.cpp`.

// ============================================================================
// Arithmetic and Clipping Helpers
// ============================================================================

#[inline(always)]
pub fn WELS_ABS(iX: i32) -> i32 {
    if iX > 0 { iX } else { -iX }
}

#[inline(always)]
pub fn WELS_CLIP3(iX: i32, iY: i32, iZ: i32) -> i32 {
    if iX < iY {
        iY
    } else if iX > iZ {
        iZ
    } else {
        iX
    }
}

#[inline(always)]
pub fn WelsClip1(iX: i32) -> u8 {
    if (iX & !255) != 0 {
        ((-iX) >> 31) as u8
    } else {
        iX as u8
    }
}

// ============================================================================
// Safe kernels
// ============================================================================

// Direction is encoded in the step arguments: every edge kernel takes `(step_x, step_y)`
// in **bytes**, reads its taps at multiples of `step_x` around the cursor's anchor, and
// advances a line by `step_y`, addressing `i * step_y + j * step_x` as a flat byte offset
// through `at(off, 0)` / `set(off, 0, _)`. A V call passes `(stride, 1)`, an H call
// `(1, stride)`.
//
// Arithmetic is `i32` over `u8` samples with `|tc0| <= 26`, so no intermediate can
// leave `i32` range. The bS<4 kernels store `p1 + clip` and `q1 + clip` (range
// `[-26, 281]`) with a plain `as u8`, which wraps. `WelsClip1` is applied to
// `p0'`/`q0'` only.

use crate::safe::plane::PlaneSamples;

/// The kernel set the dispatch sites below call: `simd::x86_64` or `simd::aarch64` by
/// default, `simd::wide` under `--features wide`. Stays module-qualified: the kernels
/// share their names with the scalars here.
use crate::simd::kernels;

/// C++: `DeblockLumaLt4_c` — the normal/weak (bS < 4) luma filter across 16 lines of
/// one macroblock edge.
///
/// `pix` is anchored at the first line's `q0`. Taps `j ∈ [-3, 2]` (`p2..q2`) are
/// read at `j * step_x`; `p1..q1` may be written; lines advance by `step_y`.
/// `tc[i >> 2]` gates each line: negative means the line's 4-sample group is not
/// filtered at all.
pub fn deblock_luma_lt4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    kernels::deblock::deblock_luma_lt4(pix, step_x, step_y, alpha, beta, tc)
}

pub fn deblock_luma_lt4_scalar(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    for i in 0..16isize {
        let b = i * step_y;
        let tc0 = tc[(i >> 2) as usize] as i32;
        if tc0 >= 0 {
            let p0 = pix.at(b - step_x, 0) as i32;
            let p1 = pix.at(b - 2 * step_x, 0) as i32;
            let p2 = pix.at(b - 3 * step_x, 0) as i32;
            let q0 = pix.at(b, 0) as i32;
            let q1 = pix.at(b + step_x, 0) as i32;
            let q2 = pix.at(b + 2 * step_x, 0) as i32;

            let deta_p0q0 = (p0 - q0).abs() < alpha;
            let deta_p1p0 = (p1 - p0).abs() < beta;
            let deta_q1q0 = (q1 - q0).abs() < beta;
            let mut tc_i = tc0;
            if deta_p0q0 && deta_p1p0 && deta_q1q0 {
                let deta_p2p0 = (p2 - p0).abs() < beta;
                let deta_q2q0 = (q2 - q0).abs() < beta;
                if deta_p2p0 {
                    let clip = WELS_CLIP3((p2 + ((p0 + q0 + 1) >> 1) - (p1 * 2)) >> 1, -tc0, tc0);
                    pix.set(b - 2 * step_x, 0, (p1 + clip) as u8);
                    tc_i += 1;
                }
                if deta_q2q0 {
                    let clip = WELS_CLIP3((q2 + ((p0 + q0 + 1) >> 1) - (q1 * 2)) >> 1, -tc0, tc0);
                    pix.set(b + step_x, 0, (q1 + clip) as u8);
                    tc_i += 1;
                }
                let deta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -tc_i, tc_i);
                pix.set(b - step_x, 0, WelsClip1(p0 + deta));
                pix.set(b, 0, WelsClip1(q0 - deta));
            }
        }
    }
}

/// C++: `DeblockLumaEq4_c` — the strong (bS == 4, intra boundary) luma filter across
/// 16 lines of one macroblock edge.
///
/// `pix` is anchored at the first line's `q0`. Taps `j ∈ [-4, 3]` (`p3..q3`) may
/// be read at `j * step_x` (`p3`/`q3` only on the strong-filter branch); `p2..q2`
/// may be written; lines advance by `step_y`.
pub fn deblock_luma_eq4(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    kernels::deblock::deblock_luma_eq4(pix, step_x, step_y, alpha, beta)
}

pub fn deblock_luma_eq4_scalar(
    pix: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    for i in 0..16isize {
        let b = i * step_y;
        let p0 = pix.at(b - step_x, 0) as i32;
        let p1 = pix.at(b - 2 * step_x, 0) as i32;
        let p2 = pix.at(b - 3 * step_x, 0) as i32;
        let q0 = pix.at(b, 0) as i32;
        let q1 = pix.at(b + step_x, 0) as i32;
        let q2 = pix.at(b + 2 * step_x, 0) as i32;

        let deta_p0q0 = (p0 - q0).abs();
        let deta_p1p0 = (p1 - p0).abs() < beta;
        let deta_q1q0 = (q1 - q0).abs() < beta;

        if (deta_p0q0 < alpha) && deta_p1p0 && deta_q1q0 {
            if deta_p0q0 < ((alpha >> 2) + 2) {
                let deta_p2p0 = (p2 - p0).abs() < beta;
                let deta_q2q0 = (q2 - q0).abs() < beta;
                if deta_p2p0 {
                    let p3 = pix.at(b - 4 * step_x, 0) as i32;
                    pix.set(
                        b - step_x,
                        0,
                        ((p2 + (p1 * 2) + (p0 * 2) + (q0 * 2) + q1 + 4) >> 3) as u8,
                    );
                    pix.set(b - 2 * step_x, 0, ((p2 + p1 + p0 + q0 + 2) >> 2) as u8);
                    pix.set(
                        b - 3 * step_x,
                        0,
                        (((p3 * 2) + p2 + (p2 * 2) + p1 + p0 + q0 + 4) >> 3) as u8,
                    );
                } else {
                    pix.set(b - step_x, 0, (((p1 * 2) + p0 + q1 + 2) >> 2) as u8);
                }
                if deta_q2q0 {
                    let q3 = pix.at(b + 3 * step_x, 0) as i32;
                    pix.set(
                        b,
                        0,
                        ((p1 + (p0 * 2) + (q0 * 2) + (q1 * 2) + q2 + 4) >> 3) as u8,
                    );
                    pix.set(b + step_x, 0, ((p0 + q0 + q1 + q2 + 2) >> 2) as u8);
                    pix.set(
                        b + 2 * step_x,
                        0,
                        (((q3 * 2) + q2 + (q2 * 2) + q1 + q0 + p0 + 4) >> 3) as u8,
                    );
                } else {
                    pix.set(b, 0, (((q1 * 2) + q0 + p1 + 2) >> 2) as u8);
                }
            } else {
                pix.set(b - step_x, 0, (((p1 * 2) + p0 + q1 + 2) >> 2) as u8);
                pix.set(b, 0, (((q1 * 2) + q0 + p1 + 2) >> 2) as u8);
            }
        }
    }
}

/// One line of the weak chroma filter, shared by the two-plane and single-plane
/// (`*2_c`) variants.
#[inline(always)]
fn chroma_lt4_line(
    pix: &mut impl PlaneSamples,
    b: isize,
    step_x: isize,
    alpha: i32,
    beta: i32,
    tc0: i32,
) {
    let p0 = pix.at(b - step_x, 0) as i32;
    let p1 = pix.at(b - 2 * step_x, 0) as i32;
    let q0 = pix.at(b, 0) as i32;
    let q1 = pix.at(b + step_x, 0) as i32;

    let deta_p0q0 = (p0 - q0).abs() < alpha;
    let deta_p1p0 = (p1 - p0).abs() < beta;
    let deta_q1q0 = (q1 - q0).abs() < beta;
    if deta_p0q0 && deta_p1p0 && deta_q1q0 {
        let deta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -tc0, tc0);
        pix.set(b - step_x, 0, WelsClip1(p0 + deta));
        pix.set(b, 0, WelsClip1(q0 - deta));
    }
}

/// One line of the strong chroma filter, shared the same way.
#[inline(always)]
fn chroma_eq4_line(pix: &mut impl PlaneSamples, b: isize, step_x: isize, alpha: i32, beta: i32) {
    let p0 = pix.at(b - step_x, 0) as i32;
    let p1 = pix.at(b - 2 * step_x, 0) as i32;
    let q0 = pix.at(b, 0) as i32;
    let q1 = pix.at(b + step_x, 0) as i32;
    let deta_p0q0 = (p0 - q0).abs() < alpha;
    let deta_p1p0 = (p1 - p0).abs() < beta;
    let deta_q1q0 = (q1 - q0).abs() < beta;
    if deta_p0q0 && deta_p1p0 && deta_q1q0 {
        pix.set(b - step_x, 0, (((p1 * 2) + p0 + q1 + 2) >> 2) as u8);
        pix.set(b, 0, (((q1 * 2) + q0 + p1 + 2) >> 2) as u8);
    }
}

/// C++: `DeblockChromaLt4_c` — the weak (bS < 4) chroma filter across 8 lines, on
/// separate Cb and Cr planes.
///
/// Both cursors are anchored at their plane's first-line `q0`. Taps
/// `j ∈ [-2, 1]` are read at `j * step_x`; `p0`/`q0` may be written; lines
/// advance by `step_y`. `tc[i >> 1]` gates each line: `> 0` here, where the luma
/// gate is `>= 0`.
pub fn deblock_chroma_lt4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    kernels::deblock::deblock_chroma_lt4(cb, cr, step_x, step_y, alpha, beta, tc)
}

pub fn deblock_chroma_lt4_scalar(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    for i in 0..8isize {
        let b = i * step_y;
        let tc0 = tc[(i >> 1) as usize] as i32;
        if tc0 > 0 {
            chroma_lt4_line(cb, b, step_x, alpha, beta, tc0);
            chroma_lt4_line(cr, b, step_x, alpha, beta, tc0);
        }
    }
}

/// C++: `DeblockChromaEq4_c` — the strong (bS == 4) chroma filter across 8 lines, on
/// separate Cb and Cr planes. Reach as [`deblock_chroma_lt4`].
pub fn deblock_chroma_eq4(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    kernels::deblock::deblock_chroma_eq4(cb, cr, step_x, step_y, alpha, beta)
}

pub fn deblock_chroma_eq4_scalar(
    cb: &mut impl PlaneSamples,
    cr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    for i in 0..8isize {
        let b = i * step_y;
        chroma_eq4_line(cb, b, step_x, alpha, beta);
        chroma_eq4_line(cr, b, step_x, alpha, beta);
    }
}

/// C++: `DeblockChromaLt42_c` — the weak chroma filter on a single combined CbCr buffer
/// (one plane, 8 lines). Reach and gating as [`deblock_chroma_lt4`].
pub fn deblock_chroma_lt42(
    cbcr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    for i in 0..8isize {
        let b = i * step_y;
        let tc0 = tc[(i >> 1) as usize] as i32;
        if tc0 > 0 {
            chroma_lt4_line(cbcr, b, step_x, alpha, beta, tc0);
        }
    }
}

/// C++: `DeblockChromaEq42_c` — the strong chroma filter on a single combined CbCr
/// buffer (one plane, 8 lines).
pub fn deblock_chroma_eq42(
    cbcr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    for i in 0..8isize {
        let b = i * step_y;
        chroma_eq4_line(cbcr, b, step_x, alpha, beta);
    }
}

/// C++: `WelsNonZeroCount_c` — normalises the 24-entry non-zero-count cache to 0/1.
pub fn nonzero_count(nzc: &mut [i8; 24]) {
    for v in nzc.iter_mut() {
        *v = (*v != 0) as i8;
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wels_clip1() {
        assert_eq!(WelsClip1(-10), 0);
        assert_eq!(WelsClip1(0), 0);
        assert_eq!(WelsClip1(128), 128);
        assert_eq!(WelsClip1(255), 255);
        assert_eq!(WelsClip1(300), 255);
    }
}

#[cfg(test)]
mod dispatch_tests {}
