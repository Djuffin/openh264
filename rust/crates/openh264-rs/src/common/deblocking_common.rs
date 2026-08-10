#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused_variables, unused_unsafe)]

//! H.264 / AVC In-Loop Adaptive Deblocking Filter Primitives.
//!
//! Translated from `codec/common/inc/deblocking_common.h` and
//! `codec/common/src/deblocking_common.cpp`.

// ============================================================================
// Arithmetic and Clipping Helpers
// ============================================================================

#[inline(always)]
pub fn WELS_ABS(iX: i32) -> i32 {
    if iX > 0 {
        iX
    } else {
        -iX
    }
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

// The C++ encodes vertical-vs-horizontal filtering in its argument order: every
// edge kernel takes `(iStrideX, iStrideY)`, reads its taps at multiples of
// `iStrideX`, and advances to the next line by `iStrideY`; the `V` wrappers pass
// `(iStride, 1)` and the `H` wrappers `(1, iStride)`. The safe kernels port that
// trick honestly rather than splitting into per-direction bodies: they take
// `(step_x, step_y)` in **bytes** and index `i * step_y + j * step_x` around the
// cursor's anchor through checked math. The cursor's own stride is the plane's
// real stride (it is `step_x` for a V call and `step_y` for an H call); the
// kernels do all their addressing in flat byte offsets via `at(off, 0)` /
// `set(off, 0, _)`, exactly as the C++ does around its `pPix`.
//
// Arithmetic parity (R-e): everything is `i32` over `u8` samples and `|tc0| <= 26`,
// so no intermediate can leave `i32` range — no F8-class exposure. One
// truncation is load-bearing and faithful: the bS<4 kernels store `p1 + clip`
// and `q1 + clip` (range `[-26, 281]`) with a plain `as u8`, which wraps exactly
// like the C++'s implicit int-to-uint8_t conversion. `WelsClip1` is applied only
// where the C++ applies it (p0'/q0').

use crate::safe::plane::PlaneCursorMut;

/// C++: `DeblockLumaLt4_c`, `codec/common/src/deblocking_common.cpp` — the
/// normal/weak (bS < 4) luma filter across 16 lines of one macroblock edge.
///
/// `pix` is anchored at the first line's `q0`. Taps `j ∈ [-3, 2]` (`p2..q2`) are
/// read at `j * step_x`; `p1..q1` may be written; lines advance by `step_y`.
/// `tc[i >> 2]` gates each line: negative means the line's 4-sample group is not
/// filtered at all.
pub fn deblock_luma_lt4(
    pix: &mut PlaneCursorMut<'_>,
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

/// C++: `DeblockLumaEq4_c`, `codec/common/src/deblocking_common.cpp` — the strong
/// (bS == 4, intra boundary) luma filter across 16 lines of one macroblock edge.
///
/// `pix` is anchored at the first line's `q0`. Taps `j ∈ [-4, 3]` (`p3..q3`) may
/// be read at `j * step_x` (`p3`/`q3` only on the strong-filter branch); `p2..q2`
/// may be written; lines advance by `step_y`.
pub fn deblock_luma_eq4(
    pix: &mut PlaneCursorMut<'_>,
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
                    pix.set(b - step_x, 0, ((p2 + (p1 * 2) + (p0 * 2) + (q0 * 2) + q1 + 4) >> 3) as u8);
                    pix.set(b - 2 * step_x, 0, ((p2 + p1 + p0 + q0 + 2) >> 2) as u8);
                    pix.set(b - 3 * step_x, 0, (((p3 * 2) + p2 + (p2 * 2) + p1 + p0 + q0 + 4) >> 3) as u8);
                } else {
                    pix.set(b - step_x, 0, (((p1 * 2) + p0 + q1 + 2) >> 2) as u8);
                }
                if deta_q2q0 {
                    let q3 = pix.at(b + 3 * step_x, 0) as i32;
                    pix.set(b, 0, ((p1 + (p0 * 2) + (q0 * 2) + (q1 * 2) + q2 + 4) >> 3) as u8);
                    pix.set(b + step_x, 0, ((p0 + q0 + q1 + q2 + 2) >> 2) as u8);
                    pix.set(b + 2 * step_x, 0, (((q3 * 2) + q2 + (q2 * 2) + q1 + q0 + p0 + 4) >> 3) as u8);
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
/// (`*2_c`) variants — the body the C++ repeats verbatim for Cb, Cr and CbCr.
#[inline(always)]
fn chroma_lt4_line(pix: &mut PlaneCursorMut<'_>, b: isize, step_x: isize, alpha: i32, beta: i32, tc0: i32) {
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
fn chroma_eq4_line(pix: &mut PlaneCursorMut<'_>, b: isize, step_x: isize, alpha: i32, beta: i32) {
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

/// C++: `DeblockChromaLt4_c`, `codec/common/src/deblocking_common.cpp` — the
/// weak (bS < 4) chroma filter across 8 lines, on separate Cb and Cr planes.
///
/// Both cursors are anchored at their plane's first-line `q0`. Taps
/// `j ∈ [-2, 1]` are read at `j * step_x`; `p0`/`q0` may be written; lines
/// advance by `step_y`. `tc[i >> 1]` gates each line — note `> 0` here where the
/// luma gate is `>= 0`, faithful to the C++.
pub fn deblock_chroma_lt4(
    cb: &mut PlaneCursorMut<'_>,
    cr: &mut PlaneCursorMut<'_>,
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

/// C++: `DeblockChromaEq4_c`, `codec/common/src/deblocking_common.cpp` — the
/// strong (bS == 4) chroma filter across 8 lines, on separate Cb and Cr planes.
/// Reach as [`deblock_chroma_lt4`].
pub fn deblock_chroma_eq4(
    cb: &mut PlaneCursorMut<'_>,
    cr: &mut PlaneCursorMut<'_>,
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

/// C++: `DeblockChromaLt42_c`, `codec/common/src/deblocking_common.cpp` — the
/// weak chroma filter on a single combined CbCr buffer (one plane, 8 lines).
/// Reach and gating as [`deblock_chroma_lt4`].
pub fn deblock_chroma_lt42(
    cbcr: &mut PlaneCursorMut<'_>,
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

/// C++: `DeblockChromaEq42_c`, `codec/common/src/deblocking_common.cpp` — the
/// strong chroma filter on a single combined CbCr buffer (one plane, 8 lines).
pub fn deblock_chroma_eq42(
    cbcr: &mut PlaneCursorMut<'_>,
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

/// C++: `WelsNonZeroCount_c`, `codec/common/src/deblocking_common.cpp` —
/// normalises the 24-entry non-zero-count cache to 0/1 (`!!nzc[i]`).
pub fn nonzero_count(nzc: &mut [i8; 24]) {
    for v in nzc.iter_mut() {
        *v = (*v != 0) as i8;
    }
}

// ============================================================================
// Pure C Core Deblocking Filter Algorithms
// ============================================================================

/// Normal/Weak Luma Deblocking Kernel (bS < 4)
///
/// Filters across 16 lines along a 4x4/16x16 macroblock boundary.
#[inline(always)]
pub unsafe fn DeblockLumaLt4_c(
    mut pPix: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *const i8,
) {
    let sx = iStrideX as isize;
    for i in 0..16 {
        let iTc0 = *pTc.add(i >> 2) as i32;
        if iTc0 >= 0 {
            let p0 = *pPix.offset(-sx) as i32;
            let p1 = *pPix.offset(-2 * sx) as i32;
            let p2 = *pPix.offset(-3 * sx) as i32;
            let q0 = *pPix as i32;
            let q1 = *pPix.offset(sx) as i32;
            let q2 = *pPix.offset(2 * sx) as i32;

            let bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
            let bDetaP1P0 = (p1 - p0).abs() < iBeta;
            let bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
            let mut iTc = iTc0;
            if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
                let bDetaP2P0 = (p2 - p0).abs() < iBeta;
                let bDetaQ2Q0 = (q2 - q0).abs() < iBeta;
                if bDetaP2P0 {
                    let clip_val = WELS_CLIP3((p2 + ((p0 + q0 + 1) >> 1) - (p1 * 2)) >> 1, -iTc0, iTc0);
                    *pPix.offset(-2 * sx) = (p1 + clip_val) as u8;
                    iTc += 1;
                }
                if bDetaQ2Q0 {
                    let clip_val = WELS_CLIP3((q2 + ((p0 + q0 + 1) >> 1) - (q1 * 2)) >> 1, -iTc0, iTc0);
                    *pPix.offset(sx) = (q1 + clip_val) as u8;
                    iTc += 1;
                }
                let iDeta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -iTc, iTc);
                *pPix.offset(-sx) = WelsClip1(p0 + iDeta);
                *pPix = WelsClip1(q0 - iDeta);
            }
        }
        pPix = pPix.offset(iStrideY as isize);
    }
}

/// Strong Intra Luma Deblocking Kernel (bS == 4)
///
/// Filters across 16 lines along a macroblock boundary between Intra-coded blocks.
#[inline(always)]
pub unsafe fn DeblockLumaEq4_c(
    mut pPix: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    let sx = iStrideX as isize;
    for _ in 0..16 {
        let p0 = *pPix.offset(-sx) as i32;
        let p1 = *pPix.offset(-2 * sx) as i32;
        let p2 = *pPix.offset(-3 * sx) as i32;
        let q0 = *pPix as i32;
        let q1 = *pPix.offset(sx) as i32;
        let q2 = *pPix.offset(2 * sx) as i32;

        let iDetaP0Q0 = (p0 - q0).abs();
        let bDetaP1P0 = (p1 - p0).abs() < iBeta;
        let bDetaQ1Q0 = (q1 - q0).abs() < iBeta;

        if (iDetaP0Q0 < iAlpha) && bDetaP1P0 && bDetaQ1Q0 {
            if iDetaP0Q0 < ((iAlpha >> 2) + 2) {
                let bDetaP2P0 = (p2 - p0).abs() < iBeta;
                let bDetaQ2Q0 = (q2 - q0).abs() < iBeta;
                if bDetaP2P0 {
                    let p3 = *pPix.offset(-4 * sx) as i32;
                    *pPix.offset(-sx) = ((p2 + (p1 * 2) + (p0 * 2) + (q0 * 2) + q1 + 4) >> 3) as u8;
                    *pPix.offset(-2 * sx) = ((p2 + p1 + p0 + q0 + 2) >> 2) as u8;
                    *pPix.offset(-3 * sx) = (((p3 * 2) + p2 + (p2 * 2) + p1 + p0 + q0 + 4) >> 3) as u8;
                } else {
                    *pPix.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
                }
                if bDetaQ2Q0 {
                    let q3 = *pPix.offset(3 * sx) as i32;
                    *pPix = ((p1 + (p0 * 2) + (q0 * 2) + (q1 * 2) + q2 + 4) >> 3) as u8;
                    *pPix.offset(sx) = ((p0 + q0 + q1 + q2 + 2) >> 2) as u8;
                    *pPix.offset(2 * sx) = (((q3 * 2) + q2 + (q2 * 2) + q1 + q0 + p0 + 4) >> 3) as u8;
                } else {
                    *pPix = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
                }
            } else {
                *pPix.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
                *pPix = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
            }
        }
        pPix = pPix.offset(iStrideY as isize);
    }
}

/// Normal/Weak Chroma Deblocking Kernel (bS < 4) for separate Cb and Cr planes.
#[inline(always)]
pub unsafe fn DeblockChromaLt4_c(
    mut pPixCb: *mut u8,
    mut pPixCr: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *const i8,
) {
    let sx = iStrideX as isize;
    let sy = iStrideY as isize;
    for i in 0..8 {
        let iTc0 = *pTc.add(i >> 1) as i32;
        if iTc0 > 0 {
            // Cb plane
            let mut p0 = *pPixCb.offset(-sx) as i32;
            let mut p1 = *pPixCb.offset(-2 * sx) as i32;
            let mut q0 = *pPixCb as i32;
            let mut q1 = *pPixCb.offset(sx) as i32;

            let mut bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
            let mut bDetaP1P0 = (p1 - p0).abs() < iBeta;
            let mut bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
            if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
                let iDeta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -iTc0, iTc0);
                *pPixCb.offset(-sx) = WelsClip1(p0 + iDeta);
                *pPixCb = WelsClip1(q0 - iDeta);
            }

            // Cr plane
            p0 = *pPixCr.offset(-sx) as i32;
            p1 = *pPixCr.offset(-2 * sx) as i32;
            q0 = *pPixCr as i32;
            q1 = *pPixCr.offset(sx) as i32;

            bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
            bDetaP1P0 = (p1 - p0).abs() < iBeta;
            bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
            if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
                let iDeta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -iTc0, iTc0);
                *pPixCr.offset(-sx) = WelsClip1(p0 + iDeta);
                *pPixCr = WelsClip1(q0 - iDeta);
            }
        }
        pPixCb = pPixCb.offset(sy);
        pPixCr = pPixCr.offset(sy);
    }
}

/// Strong Chroma Deblocking Kernel (bS == 4) for separate Cb and Cr planes.
#[inline(always)]
pub unsafe fn DeblockChromaEq4_c(
    mut pPixCb: *mut u8,
    mut pPixCr: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    let sx = iStrideX as isize;
    let sy = iStrideY as isize;
    for _ in 0..8 {
        // Cb
        let mut p0 = *pPixCb.offset(-sx) as i32;
        let mut p1 = *pPixCb.offset(-2 * sx) as i32;
        let mut q0 = *pPixCb as i32;
        let mut q1 = *pPixCb.offset(sx) as i32;
        let mut bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
        let mut bDetaP1P0 = (p1 - p0).abs() < iBeta;
        let mut bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
        if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
            *pPixCb.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
            *pPixCb = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
        }

        // Cr
        p0 = *pPixCr.offset(-sx) as i32;
        p1 = *pPixCr.offset(-2 * sx) as i32;
        q0 = *pPixCr as i32;
        q1 = *pPixCr.offset(sx) as i32;
        bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
        bDetaP1P0 = (p1 - p0).abs() < iBeta;
        bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
        if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
            *pPixCr.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
            *pPixCr = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
        }

        pPixCb = pPixCb.offset(sy);
        pPixCr = pPixCr.offset(sy);
    }
}

/// Normal/Weak Chroma Deblocking Kernel (bS < 4) for single-buffer interleaved/sequential CbCr.
#[inline(always)]
pub unsafe fn DeblockChromaLt42_c(
    mut pPixCbCr: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *const i8,
) {
    let sx = iStrideX as isize;
    for i in 0..8 {
        let iTc0 = *pTc.add(i >> 1) as i32;
        if iTc0 > 0 {
            let p0 = *pPixCbCr.offset(-sx) as i32;
            let p1 = *pPixCbCr.offset(-2 * sx) as i32;
            let q0 = *pPixCbCr as i32;
            let q1 = *pPixCbCr.offset(sx) as i32;

            let bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
            let bDetaP1P0 = (p1 - p0).abs() < iBeta;
            let bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
            if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
                let iDeta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -iTc0, iTc0);
                *pPixCbCr.offset(-sx) = WelsClip1(p0 + iDeta);
                *pPixCbCr = WelsClip1(q0 - iDeta);
            }
        }
        pPixCbCr = pPixCbCr.offset(iStrideY as isize);
    }
}

/// Strong Chroma Deblocking Kernel (bS == 4) for single-buffer interleaved/sequential CbCr.
#[inline(always)]
pub unsafe fn DeblockChromaEq42_c(
    mut pPixCbCr: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    let sx = iStrideX as isize;
    for _ in 0..8 {
        let p0 = *pPixCbCr.offset(-sx) as i32;
        let p1 = *pPixCbCr.offset(-2 * sx) as i32;
        let q0 = *pPixCbCr as i32;
        let q1 = *pPixCbCr.offset(sx) as i32;

        let bDetaP0Q0 = (p0 - q0).abs() < iAlpha;
        let bDetaP1P0 = (p1 - p0).abs() < iBeta;
        let bDetaQ1Q0 = (q1 - q0).abs() < iBeta;
        if bDetaP0Q0 && bDetaP1P0 && bDetaQ1Q0 {
            *pPixCbCr.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
            *pPixCbCr = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
        }
        pPixCbCr = pPixCbCr.offset(iStrideY as isize);
    }
}

// ============================================================================
// Public C ABI Exported Functions (as declared in deblocking_common.h)
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockLumaLt4V_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    DeblockLumaLt4_c(pPixY, iStride, 1, iAlpha, iBeta, pTc);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockLumaEq4V_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockLumaEq4_c(pPixY, iStride, 1, iAlpha, iBeta);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockLumaLt4H_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    DeblockLumaLt4_c(pPixY, 1, iStride, iAlpha, iBeta, pTc);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockLumaEq4H_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockLumaEq4_c(pPixY, 1, iStride, iAlpha, iBeta);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaLt4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    DeblockChromaLt4_c(pPixCb, pPixCr, iStride, 1, iAlpha, iBeta, pTc);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaEq4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockChromaEq4_c(pPixCb, pPixCr, iStride, 1, iAlpha, iBeta);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaLt4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    DeblockChromaLt4_c(pPixCb, pPixCr, 1, iStride, iAlpha, iBeta, pTc);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaEq4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    unsafe {
        DeblockChromaEq4_c(pPixCb, pPixCr, 1, iStride, iAlpha, iBeta);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaLt4V2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    unsafe {
        DeblockChromaLt42_c(pPixCbCr, iStride, 1, iAlpha, iBeta, pTc);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaEq4V2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    unsafe {
        DeblockChromaEq42_c(pPixCbCr, iStride, 1, iAlpha, iBeta);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaLt4H2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    unsafe {
        DeblockChromaLt42_c(pPixCbCr, 1, iStride, iAlpha, iBeta, pTc);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockChromaEq4H2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    unsafe {
        DeblockChromaEq42_c(pPixCbCr, 1, iStride, iAlpha, iBeta);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsNonZeroCount_c(pNonZeroCount: *mut i8) {
    unsafe {
        for i in 0..24 {
            let val = *pNonZeroCount.add(i);
            *pNonZeroCount.add(i) = if val != 0 { 1 } else { 0 };
        }
    }
}

// ============================================================================
// Function Pointer Types & SDeblockingFunc Table
// ============================================================================

pub type PLumaDeblockingLT4Func =
    Option<unsafe extern "C" fn(pPixY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32, pTc: *mut i8)>;

pub type PLumaDeblockingEQ4Func =
    Option<unsafe extern "C" fn(pPixY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;

pub type PChromaDeblockingLT4Func = Option<
    unsafe extern "C" fn(
        pPixCb: *mut u8,
        pPixCr: *mut u8,
        iStride: i32,
        iAlpha: i32,
        iBeta: i32,
        pTc: *mut i8,
    ),
>;

pub type PChromaDeblockingEQ4Func =
    Option<unsafe extern "C" fn(pPixCb: *mut u8, pPixCr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;

pub type PChromaDeblockingLT4Func2 =
    Option<unsafe extern "C" fn(pPixCbCr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32, pTc: *mut i8)>;

pub type PChromaDeblockingEQ4Func2 =
    Option<unsafe extern "C" fn(pPixCbCr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;

pub type PWelsNonZeroCountFunc = Option<unsafe extern "C" fn(pNonZeroCount: *mut i8)>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDeblockingFunc {
    pub pfLumaDeblockingLT4Ver: PLumaDeblockingLT4Func,
    pub pfLumaDeblockingEQ4Ver: PLumaDeblockingEQ4Func,
    pub pfLumaDeblockingLT4Hor: PLumaDeblockingLT4Func,
    pub pfLumaDeblockingEQ4Hor: PLumaDeblockingEQ4Func,

    pub pfChromaDeblockingLT4Ver: PChromaDeblockingLT4Func,
    pub pfChromaDeblockingEQ4Ver: PChromaDeblockingEQ4Func,
    pub pfChromaDeblockingLT4Hor: PChromaDeblockingLT4Func,
    pub pfChromaDeblockingEQ4Hor: PChromaDeblockingEQ4Func,

    pub pfChromaDeblockingLT4Ver2: PChromaDeblockingLT4Func2,
    pub pfChromaDeblockingEQ4Ver2: PChromaDeblockingEQ4Func2,
    pub pfChromaDeblockingLT4Hor2: PChromaDeblockingLT4Func2,
    pub pfChromaDeblockingEQ4Hor2: PChromaDeblockingEQ4Func2,
}

impl Default for SDeblockingFunc {
    fn default() -> Self {
        Self {
            pfLumaDeblockingLT4Ver: Some(DeblockLumaLt4V_c),
            pfLumaDeblockingEQ4Ver: Some(DeblockLumaEq4V_c),
            pfLumaDeblockingLT4Hor: Some(DeblockLumaLt4H_c),
            pfLumaDeblockingEQ4Hor: Some(DeblockLumaEq4H_c),

            pfChromaDeblockingLT4Ver: Some(DeblockChromaLt4V_c),
            pfChromaDeblockingEQ4Ver: Some(DeblockChromaEq4V_c),
            pfChromaDeblockingLT4Hor: Some(DeblockChromaLt4H_c),
            pfChromaDeblockingEQ4Hor: Some(DeblockChromaEq4H_c),

            pfChromaDeblockingLT4Ver2: Some(DeblockChromaLt4V2_c),
            pfChromaDeblockingEQ4Ver2: Some(DeblockChromaEq4V2_c),
            pfChromaDeblockingLT4Hor2: Some(DeblockChromaLt4H2_c),
            pfChromaDeblockingEQ4Hor2: Some(DeblockChromaEq4H2_c),
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn DeblockingInit(pFunc: *mut SDeblockingFunc, _iCpu: i32) {
    if !pFunc.is_null() {
        unsafe {
            *pFunc = SDeblockingFunc::default();
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wels_non_zero_count() {
        let mut counts: [i8; 24] = [
            0, 3, -1, 0, 5, 0, 0, 12,
            0, 0, 1, -4, 0, 0, 0, 0,
            2, 0, 0, 7, 0, -9, 0, 0,
        ];
        unsafe {
            WelsNonZeroCount_c(counts.as_mut_ptr());
        }
        for (i, &val) in counts.iter().enumerate() {
            let expected = match i {
                1 | 2 | 4 | 7 | 10 | 11 | 16 | 19 | 21 => 1,
                _ => 0,
            };
            assert_eq!(val, expected, "Mismatch at index {}", i);
        }
    }

    #[test]
    fn test_wels_clip1() {
        assert_eq!(WelsClip1(-10), 0);
        assert_eq!(WelsClip1(0), 0);
        assert_eq!(WelsClip1(128), 128);
        assert_eq!(WelsClip1(255), 255);
        assert_eq!(WelsClip1(300), 255);
    }

    #[test]
    fn test_deblock_luma_lt4_v() {
        let mut buf = [128u8; 16 * 16];
        let stride = 16i32;
        let mut tc = [2i8; 4];
        unsafe {
            DeblockLumaLt4V_c(buf.as_mut_ptr().add(4 * 16), stride, 20, 10, tc.as_mut_ptr());
        }
        // Flat buffer remains unmodified
        assert_eq!(buf[4 * 16], 128);
    }
}
