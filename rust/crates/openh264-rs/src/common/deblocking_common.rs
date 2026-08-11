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
// Shim span arithmetic
// ============================================================================

/// The one place that turns a deblocking kernel's reach into a slice span
/// (R-c: nothing else does this arithmetic).
///
/// A kernel anchored at `pPix` touches byte offsets `j*step_x + i*step_y` for
/// taps `j ∈ [-reach_back, reach_fwd]` and lines `i ∈ [0, lines)`. Both steps
/// are positive at every call site (`(iStride, 1)` or `(1, iStride)`), so the
/// minimum offset is `-reach_back*step_x` and the maximum
/// `reach_fwd*step_x + (lines-1)*step_y`. Returns `(back, len)`: the slice is
/// anchored at `pPix - back` and holds `len` bytes, with the kernel's anchor at
/// index `back`.
#[inline]
fn shim_span(step_x: usize, step_y: usize, reach_back: usize, reach_fwd: usize, lines: usize) -> (usize, usize) {
    let back = reach_back * step_x;
    (back, back + reach_fwd * step_x + (lines - 1) * step_y + 1)
}

// ============================================================================
// Public C ABI wrappers (as declared in deblocking_common.h) — Phase 2 shims
// ============================================================================
//
// The `V`/`H` pairs collapse onto one safe kernel each, exactly as the C++'s
// wrappers collapse onto one `(iStrideX, iStrideY)` body: `V` fixes the steps
// to `(iStride, 1)` (taps across rows — a horizontal edge), `H` to
// `(1, iStride)` (taps across columns — a vertical edge). Each shim
// materialises exactly the span its kernel's reach declares and anchors a
// cursor whose stride is the plane's real stride.
//
// **Why the negative reach is legal — the availability argument, shared by all
// twelve shims and quoted from here in each contract:** deblocking runs on
// decoded picture samples, and the drivers only filter an edge whose p side
// exists. The macroblock-boundary edge (the anchor's own column or row) is
// filtered only when the left/top neighbour MB is present and, under
// `uiFilterIdc == 2`, in the same slice (`decoder/deblocking.rs`
// `DeblockingAvailableNoInterlayer`; `encoder/deblocking.rs` `bLeftBsValid`/
// `bTopBsValid`); interior edges sit 4, 8 or 12 samples into the macroblock.
// So every tap, including the `-reach_back` ones, lands on picture (or left/top
// neighbour MB) samples — the padding border is *not* part of this argument.

/// C++: `DeblockLumaLt4V_c` — bS<4 luma, taps stepping by `iStride`.
///
/// # Safety
/// * `pPixY` points at the first line's `q0` of a 16-line luma edge in a plane
///   of stride `iStride > 0`. With `s = iStride as usize`, the call touches
///   exactly `[pPixY - 3*s, pPixY + 2*s + 15]` — reads `p2..q2` per line,
///   writes `p1..q1` — and the shim materialises that span (`5*s + 16` bytes),
///   which must lie inside one live allocation. The p side exists per the
///   module-level availability argument above.
/// * `pTc` points at 4 readable `i8` group thresholds.
pub unsafe extern "C" fn DeblockLumaLt4V_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_luma_lt4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 3, 2, 16);
        let buf = std::slice::from_raw_parts_mut(pPixY.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_luma_lt4(&mut PlaneCursorMut::new(buf, back, s), s as isize, 1, iAlpha, iBeta, tc);
    }
}

/// C++: `DeblockLumaEq4V_c` — bS==4 luma, taps stepping by `iStride`.
///
/// # Safety
/// * As [`DeblockLumaLt4V_c`], but the strong filter reaches one tap further on
///   both sides: the span is `[pPixY - 4*s, pPixY + 3*s + 15]` (`7*s + 16`
///   bytes) — reads `p3..q3`, writes `p2..q2`.
pub unsafe extern "C" fn DeblockLumaEq4V_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_luma_eq4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 4, 3, 16);
        let buf = std::slice::from_raw_parts_mut(pPixY.sub(back), len);
        deblock_luma_eq4(&mut PlaneCursorMut::new(buf, back, s), s as isize, 1, iAlpha, iBeta);
    }
}

/// C++: `DeblockLumaLt4H_c` — bS<4 luma, taps stepping by 1 byte.
///
/// # Safety
/// * `pPixY` points at the first line's `q0` of a 16-line luma edge in a plane
///   of stride `iStride > 0`. With `s = iStride as usize`, the call touches
///   exactly `[pPixY - 3, pPixY + 15*s + 2]` (`15*s + 6` bytes) — three
///   columns left, two right, sixteen rows down. The p side exists per the
///   module-level availability argument above.
/// * `pTc` points at 4 readable `i8` group thresholds.
pub unsafe extern "C" fn DeblockLumaLt4H_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_luma_lt4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 3, 2, 16);
        let buf = std::slice::from_raw_parts_mut(pPixY.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_luma_lt4(&mut PlaneCursorMut::new(buf, back, s), 1, s as isize, iAlpha, iBeta, tc);
    }
}

/// C++: `DeblockLumaEq4H_c` — bS==4 luma, taps stepping by 1 byte.
///
/// # Safety
/// * As [`DeblockLumaLt4H_c`], one tap further both sides: the span is
///   `[pPixY - 4, pPixY + 15*s + 3]` (`15*s + 8` bytes).
pub unsafe extern "C" fn DeblockLumaEq4H_c(
    pPixY: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_luma_eq4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 4, 3, 16);
        let buf = std::slice::from_raw_parts_mut(pPixY.sub(back), len);
        deblock_luma_eq4(&mut PlaneCursorMut::new(buf, back, s), 1, s as isize, iAlpha, iBeta);
    }
}

/// C++: `DeblockChromaLt4V_c` — bS<4 chroma on separate Cb/Cr planes, taps
/// stepping by `iStride`.
///
/// # Safety
/// * `pPixCb` and `pPixCr` each point at the first line's `q0` of an 8-line
///   chroma edge; the planes share stride `iStride > 0` and must not overlap.
///   With `s = iStride as usize`, each plane's touched span is exactly
///   `[p - 2*s, p + s + 7]` (`3*s + 8` bytes) — reads `p1..q1`, writes
///   `p0`/`q0`. The p side exists per the module-level availability argument.
/// * `pTc` points at 4 readable `i8` group thresholds.
pub unsafe extern "C" fn DeblockChromaLt4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_chroma_lt4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 2, 1, 8);
        let cb = std::slice::from_raw_parts_mut(pPixCb.sub(back), len);
        let cr = std::slice::from_raw_parts_mut(pPixCr.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_chroma_lt4(
            &mut PlaneCursorMut::new(cb, back, s),
            &mut PlaneCursorMut::new(cr, back, s),
            s as isize, 1, iAlpha, iBeta, tc,
        );
    }
}

/// C++: `DeblockChromaEq4V_c` — bS==4 chroma on separate Cb/Cr planes, taps
/// stepping by `iStride`. Same reach and span as [`DeblockChromaLt4V_c`].
///
/// # Safety
/// * As [`DeblockChromaLt4V_c`], without the tc table.
pub unsafe extern "C" fn DeblockChromaEq4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_chroma_eq4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 2, 1, 8);
        let cb = std::slice::from_raw_parts_mut(pPixCb.sub(back), len);
        let cr = std::slice::from_raw_parts_mut(pPixCr.sub(back), len);
        deblock_chroma_eq4(
            &mut PlaneCursorMut::new(cb, back, s),
            &mut PlaneCursorMut::new(cr, back, s),
            s as isize, 1, iAlpha, iBeta,
        );
    }
}

/// C++: `DeblockChromaLt4H_c` — bS<4 chroma on separate Cb/Cr planes, taps
/// stepping by 1 byte.
///
/// # Safety
/// * As [`DeblockChromaLt4V_c`] with the axes swapped: each plane's span is
///   exactly `[p - 2, p + 7*s + 1]` (`7*s + 4` bytes) — two columns left, one
///   right, eight rows down.
pub unsafe extern "C" fn DeblockChromaLt4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_chroma_lt4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 2, 1, 8);
        let cb = std::slice::from_raw_parts_mut(pPixCb.sub(back), len);
        let cr = std::slice::from_raw_parts_mut(pPixCr.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_chroma_lt4(
            &mut PlaneCursorMut::new(cb, back, s),
            &mut PlaneCursorMut::new(cr, back, s),
            1, s as isize, iAlpha, iBeta, tc,
        );
    }
}

/// C++: `DeblockChromaEq4H_c` — bS==4 chroma on separate Cb/Cr planes, taps
/// stepping by 1 byte. Same span as [`DeblockChromaLt4H_c`].
///
/// # Safety
/// * As [`DeblockChromaLt4H_c`], without the tc table.
pub unsafe extern "C" fn DeblockChromaEq4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_chroma_eq4
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 2, 1, 8);
        let cb = std::slice::from_raw_parts_mut(pPixCb.sub(back), len);
        let cr = std::slice::from_raw_parts_mut(pPixCr.sub(back), len);
        deblock_chroma_eq4(
            &mut PlaneCursorMut::new(cb, back, s),
            &mut PlaneCursorMut::new(cr, back, s),
            1, s as isize, iAlpha, iBeta,
        );
    }
}

/// C++: `DeblockChromaLt4V2_c` — bS<4 chroma on one combined CbCr buffer, taps
/// stepping by `iStride`.
///
/// # Safety
/// * `pPixCbCr` points at the first line's `q0`; the touched span is exactly
///   `[pPixCbCr - 2*s, pPixCbCr + s + 7]` (`3*s + 8` bytes), as one plane of
///   [`DeblockChromaLt4V_c`].
/// * `pTc` points at 4 readable `i8` group thresholds.
pub unsafe extern "C" fn DeblockChromaLt4V2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_chroma_lt42
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 2, 1, 8);
        let buf = std::slice::from_raw_parts_mut(pPixCbCr.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_chroma_lt42(&mut PlaneCursorMut::new(buf, back, s), s as isize, 1, iAlpha, iBeta, tc);
    }
}

/// C++: `DeblockChromaEq4V2_c` — bS==4 chroma on one combined CbCr buffer, taps
/// stepping by `iStride`. Same span as [`DeblockChromaLt4V2_c`].
///
/// # Safety
/// * As [`DeblockChromaLt4V2_c`], without the tc table.
pub unsafe extern "C" fn DeblockChromaEq4V2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_chroma_eq42
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(s, 1, 2, 1, 8);
        let buf = std::slice::from_raw_parts_mut(pPixCbCr.sub(back), len);
        deblock_chroma_eq42(&mut PlaneCursorMut::new(buf, back, s), s as isize, 1, iAlpha, iBeta);
    }
}

/// C++: `DeblockChromaLt4H2_c` — bS<4 chroma on one combined CbCr buffer, taps
/// stepping by 1 byte.
///
/// # Safety
/// * `pPixCbCr` points at the first line's `q0`; the touched span is exactly
///   `[pPixCbCr - 2, pPixCbCr + 7*s + 1]` (`7*s + 4` bytes), as one plane of
///   [`DeblockChromaLt4H_c`].
/// * `pTc` points at 4 readable `i8` group thresholds.
pub unsafe extern "C" fn DeblockChromaLt4H2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    // SHIM(phase2) -> deblock_chroma_lt42
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 2, 1, 8);
        let buf = std::slice::from_raw_parts_mut(pPixCbCr.sub(back), len);
        let tc: &[i8; 4] = std::slice::from_raw_parts(pTc, 4).try_into().unwrap();
        deblock_chroma_lt42(&mut PlaneCursorMut::new(buf, back, s), 1, s as isize, iAlpha, iBeta, tc);
    }
}

/// C++: `DeblockChromaEq4H2_c` — bS==4 chroma on one combined CbCr buffer, taps
/// stepping by 1 byte. Same span as [`DeblockChromaLt4H2_c`].
///
/// # Safety
/// * As [`DeblockChromaLt4H2_c`], without the tc table.
pub unsafe extern "C" fn DeblockChromaEq4H2_c(
    pPixCbCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    // SHIM(phase2) -> deblock_chroma_eq42
    unsafe {
        let s = iStride as usize;
        let (back, len) = shim_span(1, s, 2, 1, 8);
        let buf = std::slice::from_raw_parts_mut(pPixCbCr.sub(back), len);
        deblock_chroma_eq42(&mut PlaneCursorMut::new(buf, back, s), 1, s as isize, iAlpha, iBeta);
    }
}

/// C++: `WelsNonZeroCount_c` — in no dispatch table in this module (the decoder
/// installs `decode_slice.rs`'s copy, the encoder `encoder/deblocking.rs`'s), so
/// per the T3 precedent the shim keeps only the Wels name, not the C ABI.
///
/// # Safety
/// * `pNonZeroCount` points at 24 writable `i8` — the per-MB non-zero-count
///   cache (16 luma + 8 chroma entries).
pub unsafe fn WelsNonZeroCount_c(pNonZeroCount: *mut i8) {
    // SHIM(phase2) -> nonzero_count
    unsafe {
        let nzc: &mut [i8; 24] = std::slice::from_raw_parts_mut(pNonZeroCount, 24).try_into().unwrap();
        nonzero_count(nzc);
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

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    /// Plan §5's de-virtualization mitigation for `SDeblockingFunc`:
    /// `DeblockingInit` is a constant function of its CPU argument.
    ///
    /// See `common::mc::tests::init_mc_func_ignores_the_cpu_flag` for why this
    /// compares two installed tables rather than a table against named
    /// functions, and why it is scoped out of Miri. In short: taking a
    /// function's address can mint a fresh instantiation per codegen unit, and
    /// Miri mints one per cast, so only two addresses produced by the *same*
    /// installer are safely comparable. The complementary behavioural half is
    /// `deblocking_table_slots_match_the_direct_calls` in the differential file.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn deblocking_init_ignores_the_cpu_flag() {
        use crate::common::cpu_core::*;
        let flags: [i32; 10] = [
            0, -1,
            WELS_CPU_SSE2 as i32, WELS_CPU_SSE41 as i32, WELS_CPU_SSE42 as i32,
            WELS_CPU_AVX as i32, WELS_CPU_AVX2 as i32, WELS_CPU_NEON as i32,
            WELS_CPU_MMI as i32, WELS_CPU_LSX as i32,
        ];
        let addrs = |t: &SDeblockingFunc| -> [usize; 12] {
            [
                t.pfLumaDeblockingLT4Ver.unwrap() as usize,
                t.pfLumaDeblockingEQ4Ver.unwrap() as usize,
                t.pfLumaDeblockingLT4Hor.unwrap() as usize,
                t.pfLumaDeblockingEQ4Hor.unwrap() as usize,
                t.pfChromaDeblockingLT4Ver.unwrap() as usize,
                t.pfChromaDeblockingEQ4Ver.unwrap() as usize,
                t.pfChromaDeblockingLT4Hor.unwrap() as usize,
                t.pfChromaDeblockingEQ4Hor.unwrap() as usize,
                t.pfChromaDeblockingLT4Ver2.unwrap() as usize,
                t.pfChromaDeblockingEQ4Ver2.unwrap() as usize,
                t.pfChromaDeblockingLT4Hor2.unwrap() as usize,
                t.pfChromaDeblockingEQ4Hor2.unwrap() as usize,
            ]
        };
        const NAMES: [&str; 12] = [
            "LumaLT4Ver", "LumaEQ4Ver", "LumaLT4Hor", "LumaEQ4Hor",
            "ChromaLT4Ver", "ChromaEQ4Ver", "ChromaLT4Hor", "ChromaEQ4Hor",
            "ChromaLT4Ver2", "ChromaEQ4Ver2", "ChromaLT4Hor2", "ChromaEQ4Hor2",
        ];
        let mut base = SDeblockingFunc::default();
        unsafe { DeblockingInit(&mut base, 0) };
        let want = addrs(&base);
        for flag in flags {
            let mut t = SDeblockingFunc::default();
            unsafe { DeblockingInit(&mut t, flag) };
            for (i, (got, expected)) in addrs(&t).into_iter().zip(want).enumerate() {
                assert_eq!(got, expected, "cpu flag {flag:#x} changed slot {}", NAMES[i]);
            }
        }
    }

    /// Every slot is populated after init, so the decoder's former
    /// `if let Some(f) = (*pLoopf).pf...` guards — 22 of them — were never
    /// taken, and unconditional direct calls preserve behaviour.
    ///
    /// A `None` there did not degrade gracefully: it silently skipped filtering
    /// one edge, which is a wrong picture rather than an error. `DeblockingInit`
    /// runs from `WelsInitDecoderFuncs` at open, before any slice is decoded.
    #[test]
    fn deblocking_table_is_fully_populated_after_init() {
        let mut t = SDeblockingFunc::default();
        unsafe { DeblockingInit(&mut t, 0) };
        assert!(
            t.pfLumaDeblockingLT4Ver.is_some() && t.pfLumaDeblockingEQ4Ver.is_some()
                && t.pfLumaDeblockingLT4Hor.is_some() && t.pfLumaDeblockingEQ4Hor.is_some()
                && t.pfChromaDeblockingLT4Ver.is_some() && t.pfChromaDeblockingEQ4Ver.is_some()
                && t.pfChromaDeblockingLT4Hor.is_some() && t.pfChromaDeblockingEQ4Hor.is_some()
                && t.pfChromaDeblockingLT4Ver2.is_some() && t.pfChromaDeblockingEQ4Ver2.is_some()
                && t.pfChromaDeblockingLT4Hor2.is_some() && t.pfChromaDeblockingEQ4Hor2.is_some(),
            "DeblockingInit must leave every slot populated"
        );
    }
}
