#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

//! H.264 / AVC In-Loop Adaptive Deblocking Filter Primitives.
//!
//! C++: `codec/common/src/deblocking_common.cpp`.

// ============================================================================
// Arithmetic and Clipping Helpers
// ============================================================================

pub use crate::common::macros::{WELS_ABS, WELS_CLIP3, WelsClip1};

// ============================================================================
// H.264 Deblocking Lookup Tables
// ============================================================================

/// Table 8-16 in H.264/AVC standard: Alpha table with +12 leading and trailing index offset padding.
pub static g_kuiAlphaTable: [u8; 52 + 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 5, 6,
    7, 8, 9, 10, 12, 13, 15, 17, 20, 22, 25, 28, 32, 36, 40, 45, 50, 56, 63, 71, 80, 90, 101, 113,
    127, 144, 162, 182, 203, 226, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255,
];

/// Table 8-16 in H.264/AVC standard: Beta table with +12 leading and trailing index offset padding.
pub static g_kiBetaTable: [i8; 52 + 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 3,
    3, 3, 3, 4, 4, 4, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 15, 16,
    16, 17, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
];

/// Table 8-17 in H.264/AVC standard: Tc0 table indexed by `(IndexA + 12)` and `bS` (`0..=3`).
pub static g_kiTc0Table: [[i8; 4]; 52 + 24] = [
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 0],
    [-1, 0, 0, 1],
    [-1, 0, 0, 1],
    [-1, 0, 0, 1],
    [-1, 0, 0, 1],
    [-1, 0, 1, 1],
    [-1, 0, 1, 1],
    [-1, 1, 1, 1],
    [-1, 1, 1, 1],
    [-1, 1, 1, 1],
    [-1, 1, 1, 1],
    [-1, 1, 1, 2],
    [-1, 1, 1, 2],
    [-1, 1, 1, 2],
    [-1, 1, 1, 2],
    [-1, 1, 2, 3],
    [-1, 1, 2, 3],
    [-1, 2, 2, 3],
    [-1, 2, 2, 4],
    [-1, 2, 3, 4],
    [-1, 2, 3, 4],
    [-1, 3, 3, 5],
    [-1, 3, 4, 6],
    [-1, 3, 4, 6],
    [-1, 4, 5, 7],
    [-1, 4, 5, 8],
    [-1, 4, 6, 9],
    [-1, 5, 7, 10],
    [-1, 6, 8, 11],
    [-1, 6, 8, 13],
    [-1, 7, 10, 14],
    [-1, 8, 11, 16],
    [-1, 9, 12, 18],
    [-1, 10, 13, 20],
    [-1, 11, 15, 23],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
    [-1, 13, 17, 25],
];

#[inline(always)]
pub fn alpha_table(x: i32) -> u8 {
    let idx = (x + 12) as usize;
    if idx < g_kuiAlphaTable.len() {
        g_kuiAlphaTable[idx]
    } else {
        255
    }
}

#[inline(always)]
pub fn beta_table(x: i32) -> i8 {
    let idx = (x + 12) as usize;
    if idx < g_kiBetaTable.len() {
        g_kiBetaTable[idx]
    } else {
        18
    }
}

#[inline(always)]
pub fn tc0_table(x: i32) -> &'static [i8; 4] {
    let idx = (x + 12) as usize;
    if idx < g_kiTc0Table.len() {
        &g_kiTc0Table[idx]
    } else {
        &g_kiTc0Table[g_kiTc0Table.len() - 1]
    }
}

/// Sub-block index mapping table for marginal boundary edges.
///
/// Row `0` is the left (vertical) edge, row `1` the top (horizontal) edge; the low
/// four entries are the current macroblock's 4x4 block indices along that edge and
/// the high four the co-located neighbour indices. Encoder and decoder carried
/// byte-for-byte identical copies of this before it was hoisted here.
pub static g_kuiTableBIdx: [[u8; 8]; 2] =
    [[0, 4, 8, 12, 3, 7, 11, 15], [0, 1, 2, 3, 12, 13, 14, 15]];

// ============================================================================
// Safe kernels
// ============================================================================

// Direction is encoded in the step arguments: every edge kernel takes `(step_x, step_y)`
// in **bytes**, reads its taps at multiples of `step_x` around the cursor's anchor, and
// advances a line by `step_y`, addressing `i * step_y + j * step_x` as a flat byte offset
// through `at(off, 0)` / `set(off, 0, _)`. A V call passes `(1, stride)`, an H call
// `(stride, 1)`.
//
// Arithmetic is `i32` over `u8` samples with `|tc0| <= 26`, so no intermediate can
// leave `i32` range. The bS<4 kernels store `p1 + clip` and `q1 + clip` (range
// `[-26, 281]`) with a plain `as u8`, which wraps. `WelsClip1` is applied to
// `p0'`/`q0'` only.

use crate::safe::plane::PlaneSamples;

/// The kernel set the dispatch sites below call: `simd::x86_64` or `simd::aarch64` by
/// default, `simd::scalar` under `--features scalar` or on a target with neither. Stays
/// module-qualified: the kernels share their names with the scalars here.
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

/// C++: `DeblockChromaLt42_c` — the weak chroma filter on a single plane (8 lines).
/// Reach and gating as [`deblock_chroma_lt4`].
///
/// The decoder reaches this variant, not the two-plane one, whenever Cb and Cr carry
/// different QPs: each plane then has its own `alpha`/`beta`/`tc` and is filtered on its
/// own. Upstream leaves that path in C, but the two-plane kernels are already per-half —
/// Cb in the low eight lanes, Cr in the high eight, `tc[i >> 1]` repeating over each
/// half — so one plane's eight lines run through the same vector kernel.
pub fn deblock_chroma_lt42(
    cbcr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
    tc: &[i8; 4],
) {
    kernels::deblock::deblock_chroma_lt42(cbcr, step_x, step_y, alpha, beta, tc)
}

pub fn deblock_chroma_lt42_scalar(
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

/// C++: `DeblockChromaEq42_c` — the strong chroma filter on a single plane (8 lines).
/// Reached under the same split-QP condition as [`deblock_chroma_lt42`].
pub fn deblock_chroma_eq42(
    cbcr: &mut impl PlaneSamples,
    step_x: isize,
    step_y: isize,
    alpha: i32,
    beta: i32,
) {
    kernels::deblock::deblock_chroma_eq42(cbcr, step_x, step_y, alpha, beta)
}

pub fn deblock_chroma_eq42_scalar(
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
