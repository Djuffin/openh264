#![forbid(unsafe_code)]
// Copyright (c) 2009-2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # In-Loop Adaptive Deblocking Filter Engine
//!
//! Translated from `codec/encoder/core/inc/deblocking.h` and `codec/encoder/core/src/deblocking.cpp`.
//!
//! Provides boundary strength ($bS$) calculation, alpha/beta clipping threshold lookups,
//! luma and chroma 4-sample directional edge filtering, and frame/slice macroblock raster traversal.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

// ============================================================================
// Constants and Dimension Definitions
// ============================================================================

#![deny(unsafe_code)]

pub const MB_WIDTH_LUMA: usize = 16;
pub const MB_WIDTH_CHROMA: usize = 8;

// `wels_common_basis.h:123-124`.
pub const LEFT_MB_POS: i32 = 0x01;
pub const TOP_MB_POS: i32 = 0x02;

// Macroblock Coding Types matching `wels_common_defs.h`
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000080;
pub const MB_TYPE_SKIP: u32 = 0x00000100;
pub const MB_TYPE_INTRA_PCM: u32 = 0x00000200;
pub const MB_TYPE_INTRA_BL: u32 = 0x00000400;
pub const MB_TYPE_DIRECT: u32 = 0x00000800;

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

#[inline(always)]
pub fn IS_INTRA(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA) != 0
}

// ============================================================================
// H.264 Deblocking Lookup Tables
// ============================================================================

/// Table 8-16 in H.264/AVC standard: Alpha table indexed by clipped QP + offset (0..51 + padding)
// `g_kuiAlphaTable`/`g_kiBetaTable`/`g_kiTc0Table` are `static const` **file-local**
// in both codecs and are deliberately different sizes: `codec/encoder/core/src/
// deblocking.cpp:72-92` declares `[52 + 12]`, `codec/decoder/core/src/deblocking.cpp:
// 144-166` declares `[52 + 24]`.
pub static g_kuiAlphaTable: [u8; 52 + 12] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 5, 6, 7, 8, 9, 10, 12, 13, 15, 17, 20,
    22, 25, 28, 32, 36, 40, 45, 50, 56, 63, 71, 80, 90, 101, 113, 127, 144, 162, 182, 203, 226,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];

/// Table 8-16 in H.264/AVC standard: Beta table indexed by clipped QP + offset (0..51 + padding)
pub static g_kiBetaTable: [i8; 52 + 12] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 6, 6, 7, 7, 8,
    8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, 18, 18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18,
];

/// Table 8-17 in H.264/AVC standard: Clipping parameter matrix indexed by IndexA and bS
pub static g_kiTc0Table: [[i8; 4]; 52 + 12] = [
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

/// Sub-block index mapping table for marginal boundary edges
pub static g_kuiTableBIdx: [[u8; 8]; 2] = [
    [0, 4, 8, 12, 3, 7, 11, 15],
    [0, 1, 2, 3, 12, 13, 14, 15],
];

// ============================================================================
// Core Data Structures
// ============================================================================

/// 4-byte motion vector unit $(MV_x, MV_y)$ in quarter-pel precision.
pub use crate::encoder::svc_encode_slice::SMVUnitXY;
use crate::encoder::rec_view::{RecCursor, RecPicView};
use std::sync::atomic::{AtomicU16, Ordering};
use crate::common::deblocking_common::{
    deblock_chroma_eq4, deblock_chroma_lt4, deblock_luma_eq4, deblock_luma_lt4,
};
use crate::safe::mb_grid::{MbSplit, MbWindow};
/// The kernel set this module dispatches through; see [`crate::simd::kernels`].
use crate::simd::kernels;

/// Active parameters and pointers for macroblock deblocking filtering.
/// Matches `struct TagDeblockingFilter` in `codec/encoder/core/inc/deblocking.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagDeblockingFilter {
    pub iCsStride: [i32; 3],       // Reconstruction buffer row pitch in bytes
    pub iMbStride: i16,            // Picture width in macroblocks
    pub iSliceAlphaC0Offset: i8,   // Slice alpha offset parameter
    pub iSliceBetaOffset: i8,      // Slice beta offset parameter
    pub uiLumaQP: u8,              // Luma Quantization Parameter
    pub uiChromaQP: u8,            // Chroma Quantization Parameter
    pub uiFilterIdc: u8,           // Boundary control: 0 = across slices, 1 = within slice
    pub uiReserved: u8,            // Alignment padding byte
}

pub type SDeblockingFilter = TagDeblockingFilter;

impl Default for TagDeblockingFilter {
    fn default() -> Self {
        Self {
            iCsStride: [0; 3],
            iMbStride: 0,
            iSliceAlphaC0Offset: 0,
            iSliceBetaOffset: 0,
            uiLumaQP: 0,
            uiChromaQP: 0,
            uiFilterIdc: 0,
            uiReserved: 0,
        }
    }
}

pub use crate::encoder::svc_encode_slice::SMB;
pub use crate::encoder::md::{MB_BLOCK4x4_NUM, MB_LUMA_CHROMA_BLOCK4x4_NUM};

/// The per-frame slice-walk dispatch — the one deblocking slot that is
/// genuinely two-valued at runtime (`DeblockingFilterSliceAvcbase` when the
/// parallel-deblocking conditions hold, `..Null` otherwise, re-stamped every
/// frame by `PreprocessSliceCoding`).
pub type PDeblockingFilterSlice = extern "C" fn(
    view: &RecPicView,
    pSliceCtx: &crate::encoder::slice_multi_threading::SSliceCtx,
    kiCsStride: &[i32; 3],
    pSlice: &mut SSlice,
    pMbs: &mut MbWindow<'_, SMB>,
);

/// Function pointer dispatch table for deblocking routines.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct tagDeblockingFunc {
    pub pfDeblockingFilterSlice: Option<PDeblockingFilterSlice>,
}

pub type DeblockingFunc = tagDeblockingFunc;

pub use crate::encoder::encoder_context::{SPicture, SWelsFuncPtrList, sWelsEncCtx};
pub use crate::encoder::svc_encode_slice::{SDqLayer, SSlice, current_layer_ref};

// ============================================================================
// Math & Bitwise Inline Macros
// ============================================================================

#[inline(always)]
pub fn CLIP3_QP_0_51(x: i32) -> i32 {
    if x < 0 {
        0
    } else if x > 51 {
        51
    } else {
        x
    }
}

#[inline(always)]
pub fn WELS_CLIP3(x: i32, min_val: i32, max_val: i32) -> i32 {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline(always)]
pub fn WelsClip1(x: i32) -> u8 {
    if x < 0 {
        0
    } else if x > 255 {
        255
    } else {
        x as u8
    }
}

#[inline(always)]
pub fn MB_BS_MV(
    sCurMv: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    sNeighMv: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    uiBIdx: usize,
    uiBnIdx: usize,
) -> u8 {
    let cur = sCurMv[uiBIdx];
    let neigh = sNeighMv[uiBnIdx];
    if (cur.iMvX as i32 - neigh.iMvX as i32).abs() >= 4
        || (cur.iMvY as i32 - neigh.iMvY as i32).abs() >= 4
    {
        1
    } else {
        0
    }
}

#[inline(always)]
pub fn SMB_EDGE_MV(
    sMotionVector: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    uiBIdx: usize,
    uiBnIdx: usize,
) -> u8 {
    let cur = sMotionVector[uiBIdx];
    let neigh = sMotionVector[uiBnIdx];
    let dx = (cur.iMvX as i32 - neigh.iMvX as i32).abs();
    let dy = (cur.iMvY as i32 - neigh.iMvY as i32).abs();
    if ((dx & !3) | (dy & !3)) != 0 {
        1
    } else {
        0
    }
}

#[inline(always)]
pub fn BS_EDGE(
    bsx1: u8,
    sMotionVector: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    uiBIdx: usize,
    uiBnIdx: usize,
) -> u8 {
    let mv_diff = SMB_EDGE_MV(sMotionVector, uiBIdx, uiBnIdx);
    (bsx1 | mv_diff) << (if bsx1 != 0 { 1 } else { 0 })
}

#[inline(always)]
pub fn GET_ALPHA_BETA_FROM_QP(
    qp: i32,
    iAlphaOffset: i32,
    iBetaOffset: i32,
    iIdexA: &mut i32,
    iAlpha: &mut i32,
    iBeta: &mut i32,
) {
    let idxA = CLIP3_QP_0_51(qp + iAlphaOffset);
    *iIdexA = idxA;
    *iAlpha = g_kuiAlphaTable[idxA as usize] as i32;
    *iBeta = g_kiBetaTable[CLIP3_QP_0_51(qp + iBetaOffset) as usize] as i32;
}

#[inline(always)]
pub fn TC0_TBL_LOOKUP(iTc: &mut [i8; 4], iIdexA: i32, pBS: &[u8], bchroma: i8) {
    let tbl = g_kiTc0Table[iIdexA as usize];
    iTc[0] = tbl[pBS[0] as usize] + bchroma;
    iTc[1] = tbl[pBS[1] as usize] + bchroma;
    iTc[2] = tbl[pBS[2] as usize] + bchroma;
    iTc[3] = tbl[pBS[3] as usize] + bchroma;
}

// ============================================================================
// Boundary Strength (bS) Calculation Functions
// ============================================================================

/// Computes internal boundary strength for 16x16 Inter macroblocks.
#[inline(always)]
pub fn DeblockingBSInsideMBAvsbase(
    pNnzTab: &[i8; MB_LUMA_CHROMA_BLOCK4x4_NUM],
    uiBS: &mut [[[u8; 4]; 4]; 2],
    iLShiftFactor: i32,
) {
    let n0 = pNnzTab[0] as u8;
    let n1 = pNnzTab[1] as u8;
    let n2 = pNnzTab[2] as u8;
    let n3 = pNnzTab[3] as u8;

    let n4 = pNnzTab[4] as u8;
    let n5 = pNnzTab[5] as u8;
    let n6 = pNnzTab[6] as u8;
    let n7 = pNnzTab[7] as u8;

    let n8 = pNnzTab[8] as u8;
    let n9 = pNnzTab[9] as u8;
    let n10 = pNnzTab[10] as u8;
    let n11 = pNnzTab[11] as u8;

    let n12 = pNnzTab[12] as u8;
    let n13 = pNnzTab[13] as u8;
    let n14 = pNnzTab[14] as u8;
    let n15 = pNnzTab[15] as u8;

    // Vertical internal edges (dir = 0)
    uiBS[0][1][0] = (n0 | n1) << iLShiftFactor;
    uiBS[0][2][0] = (n1 | n2) << iLShiftFactor;
    uiBS[0][3][0] = (n2 | n3) << iLShiftFactor;

    uiBS[0][1][1] = (n4 | n5) << iLShiftFactor;
    uiBS[0][2][1] = (n5 | n6) << iLShiftFactor;
    uiBS[0][3][1] = (n6 | n7) << iLShiftFactor;

    uiBS[0][1][2] = (n8 | n9) << iLShiftFactor;
    uiBS[0][2][2] = (n9 | n10) << iLShiftFactor;
    uiBS[0][3][2] = (n10 | n11) << iLShiftFactor;

    uiBS[0][1][3] = (n12 | n13) << iLShiftFactor;
    uiBS[0][2][3] = (n13 | n14) << iLShiftFactor;
    uiBS[0][3][3] = (n14 | n15) << iLShiftFactor;

    // Horizontal internal edges (dir = 1)
    for k in 0..4 {
        uiBS[1][1][k] = (pNnzTab[k] as u8 | pNnzTab[4 + k] as u8) << iLShiftFactor;
        uiBS[1][2][k] = (pNnzTab[4 + k] as u8 | pNnzTab[8 + k] as u8) << iLShiftFactor;
        uiBS[1][3][k] = (pNnzTab[8 + k] as u8 | pNnzTab[12 + k] as u8) << iLShiftFactor;
    }
}

/// Computes internal boundary strength for normal partitioned Inter macroblocks.
#[inline(always)]
pub fn DeblockingBSInsideMBNormal(
    sMv: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    uiBS: &mut [[[u8; 4]; 4]; 2],
    pNnzTab: &[i8; MB_LUMA_CHROMA_BLOCK4x4_NUM],
) {

    // Vertical internal edges (dir = 0)
    for j in 0..4 {
        let base = j * 4;
        let bs0 = pNnzTab[base] as u8 | pNnzTab[base + 1] as u8;
        let bs1 = pNnzTab[base + 1] as u8 | pNnzTab[base + 2] as u8;
        let bs2 = pNnzTab[base + 2] as u8 | pNnzTab[base + 3] as u8;

        uiBS[0][1][j] = BS_EDGE(bs0, sMv, base + 1, base);
        uiBS[0][2][j] = BS_EDGE(bs1, sMv, base + 2, base + 1);
        uiBS[0][3][j] = BS_EDGE(bs2, sMv, base + 3, base + 2);
    }

    // Horizontal internal edges (dir = 1)
    for k in 0..4 {
        let bs0 = pNnzTab[k] as u8 | pNnzTab[4 + k] as u8;
        let bs1 = pNnzTab[4 + k] as u8 | pNnzTab[8 + k] as u8;
        let bs2 = pNnzTab[8 + k] as u8 | pNnzTab[12 + k] as u8;

        uiBS[1][1][k] = BS_EDGE(bs0, sMv, 4 + k, k);
        uiBS[1][2][k] = BS_EDGE(bs1, sMv, 8 + k, 4 + k);
        uiBS[1][3][k] = BS_EDGE(bs2, sMv, 12 + k, 8 + k);
    }
}

/// Computes marginal boundary strength vector for macroblock boundary edges (edge 0).
#[inline(always)]
pub fn DeblockingBSMarginalMBAvcbase(pCurMb: &SMB, pNeighMb: &SMB, iEdge: usize) -> u32 {
    let mut uiBSx4: [u8; 4] = [0; 4];
    let pBIdx = &g_kuiTableBIdx[iEdge][0..4];
    let pBnIdx = &g_kuiTableBIdx[iEdge][4..8];

    for i in 0..4 {
        let bIdx = pBIdx[i] as usize;
        let bnIdx = pBnIdx[i] as usize;
        let cur_nzc = pCurMb.iNonZeroCount[bIdx];
        let neigh_nzc = pNeighMb.iNonZeroCount[bnIdx];

        if (cur_nzc | neigh_nzc) != 0 {
            uiBSx4[i] = 2;
        } else {
            uiBSx4[i] = MB_BS_MV(&pCurMb.sMv, &pNeighMb.sMv, bIdx, bnIdx);
        }
    }

    u32::from_ne_bytes(uiBSx4)
}

/// Boundary strength ($bS$) for one macroblock: the kernel, and the one override it
/// cannot make.
///
/// `uiBS[0][0]` is the left MB-boundary edge, `uiBS[1][0]` the top one;
/// `uiBS[dir][1..4]` are the interior edges. The C++ writes the boundary rows
/// through `uint32_t` punning (`*(uint32_t*)uiBS[0][0]`); a 4-byte row
/// assignment is the same store with the type kept.
///
/// This is `DeblockingBSCalc_AArch64_neon` (`deblocking.cpp:578`) rather than
/// `DeblockingBSCalc_c`: the kernel computes all eight edge groups from the three
/// macroblocks' counts and vectors, and the caller then stamps `0x04040404` across a
/// macroblock edge whose neighbour is intra — the one thing that depends on a
/// neighbour's *type* rather than its data, and the only thing upstream's own NEON
/// wrapper does after calling the asm. The `!iLeftFlag` / `!iTopFlag` zeroing the C++
/// also does there is the kernel's `None` case here.
///
/// The counts are normalised in place first, and that write-back is *observable* —
/// `DeblockingBSCalc_c` has always performed it and later readers of
/// `iNonZeroCount` see the normalised values — so it stays, even though the kernel
/// tests non-zero-ness and would not need it.
///
/// A flag set with the neighbour absent from the split is a bug, and
/// [`MbSplit`]'s panic names it.
pub fn DeblockingBSCalc(
    mbs: &mut MbSplit<'_, SMB>,
    uiBS: &mut [[[u8; 4]; 4]; 2],
    uiCurMbType: u32,
    iLeftFlag: bool,
    iTopFlag: bool,
) {
    if uiCurMbType != MB_TYPE_SKIP {
        // deblocking.cpp:615
        WelsNonZeroCount_c(&mut mbs.cur_mut().iNonZeroCount);
    }
    let cur = mbs.cur();
    let leftMb = if iLeftFlag { Some(mbs.left()) } else { None };
    let topMb = if iTopFlag { Some(mbs.top()) } else { None };

    kernels::deblock::bs_calc(
        &cur.iNonZeroCount,
        &cur.sMv,
        leftMb.map(|m| (&m.iNonZeroCount, &m.sMv)),
        topMb.map(|m| (&m.iNonZeroCount, &m.sMv)),
        inside_bs_mask(uiCurMbType),
        uiBS,
    );

    if leftMb.is_some_and(|m| IS_INTRA(m.uiMbType)) {
        uiBS[0][0] = [4; 4];
    }
    if topMb.is_some_and(|m| IS_INTRA(m.uiMbType)) {
        uiBS[1][0] = [4; 4];
    }
}

/// **The boundary-strength calculation on plain data** — what every kernel set's
/// `bs_calc` computes, and the body the scalar set forwards to.
///
/// Factored out of `DeblockingBSCalc_c` so that the strength calculation takes
/// per-macroblock *data* rather than a window into the macroblock array: the NEON
/// kernel reaches its neighbours the same way, and the two can therefore be held
/// against each other directly. See
/// [`kernels::deblock::bs_calc`](crate::simd::kernels) for the contract, which is
/// this function's.
///
/// # How this differs, line by line, from the three functions it replaces
///
/// * The macroblock edges are `DeblockingBSMarginalMBAvcbase` with its table indices
///   spelled out, and are unchanged.
/// * The interior edges are `DeblockingBSInsideMBNormal` with `BS_EDGE`'s
///   `(bs | mv) << (bs != 0)` written as the test it performs — `2` when either block
///   has a coefficient, else the vector term. The two agree exactly on counts already
///   normalised to 0/1, which is what the caller passes, and this spelling also holds
///   for raw counts, where `BS_EDGE`'s shift would not.
/// * `DeblockingBSInsideMBAvsbase` (the `MB_TYPE_16x16` rule, coefficients only) and
///   the skip macroblock's all-zero interior are `inside_mask` — `0x02` and `0x00`
///   against this function's `0xFF` — rather than separate bodies.
pub fn bs_calc_scalar(
    cur_nzc: &[i8; MB_LUMA_CHROMA_BLOCK4x4_NUM],
    cur_mv: &[SMVUnitXY; MB_BLOCK4x4_NUM],
    left: Option<(&[i8; MB_LUMA_CHROMA_BLOCK4x4_NUM], &[SMVUnitXY; MB_BLOCK4x4_NUM])>,
    top: Option<(&[i8; MB_LUMA_CHROMA_BLOCK4x4_NUM], &[SMVUnitXY; MB_BLOCK4x4_NUM])>,
    inside_mask: u8,
    uiBS: &mut [[[u8; 4]; 4]; 2],
) {
    /// One edge: the coefficient test first, the motion-vector test second.
    #[inline(always)]
    fn edge(a_nzc: i8, b_nzc: i8, a_mv: SMVUnitXY, b_mv: SMVUnitXY) -> u8 {
        if (a_nzc | b_nzc) != 0 {
            2
        } else if (a_mv.iMvX as i32 - b_mv.iMvX as i32).abs() >= 4
            || (a_mv.iMvY as i32 - b_mv.iMvY as i32).abs() >= 4
        {
            1
        } else {
            0
        }
    }

    for (dir, nb) in [(0usize, left), (1usize, top)] {
        uiBS[dir][0] = match nb {
            Some((nzc, mv)) => std::array::from_fn(|i| {
                let (b, bn) = (
                    g_kuiTableBIdx[dir][i] as usize,
                    g_kuiTableBIdx[dir][4 + i] as usize,
                );
                edge(cur_nzc[b], nzc[bn], cur_mv[b], mv[bn])
            }),
            None => [0; 4],
        };
    }

    for pos in 0..4 {
        for e in 1..4 {
            // Vertical edge `e` at row `pos` separates blocks `4 * pos + e - 1` and
            // `4 * pos + e`; the horizontal one at column `pos` separates
            // `4 * (e - 1) + pos` and `4 * e + pos`.
            let (v, vp) = (4 * pos + e, 4 * pos + e - 1);
            let (h, hp) = (4 * e + pos, 4 * (e - 1) + pos);
            uiBS[0][e][pos] = edge(cur_nzc[v], cur_nzc[vp], cur_mv[v], cur_mv[vp]) & inside_mask;
            uiBS[1][e][pos] = edge(cur_nzc[h], cur_nzc[hp], cur_mv[h], cur_mv[hp]) & inside_mask;
        }
    }
}

/// The `inside_mask` for a macroblock kind — see [`bs_calc_scalar`].
///
/// `DeblockingBSCalc_c` branches on the same three cases: a skip macroblock's interior
/// edges are set to zero outright, `MB_TYPE_16x16` takes
/// `DeblockingBSInsideMBAvsbase`, whose rule is the coefficient term alone, and
/// everything else takes `DeblockingBSInsideMBNormal`.
#[inline]
pub fn inside_bs_mask(uiCurMbType: u32) -> u8 {
    match uiCurMbType {
        MB_TYPE_SKIP => 0x00,
        MB_TYPE_16x16 => 0x02,
        _ => 0xFF,
    }
}

/// C++: `WelsNonZeroCount_c` — the encoder's copy.
pub fn WelsNonZeroCount_c(pNonZeroCount: &mut [i8; MB_LUMA_CHROMA_BLOCK4x4_NUM]) {
    crate::common::deblocking_common::nonzero_count(pNonZeroCount);
}


// ============================================================================
// Directional Filtering Dispatchers
// ============================================================================

/// This macroblock's three reconstruction cursors.
///
/// Luma is 16 samples per macroblock and chroma 8, which is the whole
/// content of the `<< 4` and `<< 3`.
fn mb_cursors<'a>(
    view: &'a RecPicView,
    iMbX: i32,
    iMbY: i32,
) -> (RecCursor<'a>, RecCursor<'a>, RecCursor<'a>) {
    let (lx, ly) = ((iMbX as isize) << 4, (iMbY as isize) << 4);
    let (cx, cy) = ((iMbX as isize) << 3, (iMbY as isize) << 3);
    (
        view.plane(0).cursor(lx, ly),
        view.plane(1).cursor(cx, cy),
        view.plane(2).cursor(cx, cy),
    )
}

/// The eight directional edge dispatchers.
///
/// `iStride` stays a parameter even though the cursor carries it, because it is
/// not addressing here — it is the kernels' `step_x`/`step_y`, the linear
/// distance between taps. Which of the two gets the stride is the whole
/// difference between a vertical and a horizontal edge, and it is the reason the
/// upstream slot names read backwards against these function names: `…Ver`
/// steps its taps by the stride, which filters a *horizontal* edge.
fn FilteringEdgeLumaH(pFilter: &SDeblockingFilter, pix: &mut RecCursor<'_>, iStride: i32, pBS: &[u8; 4]) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    let mut iTc: [i8; 4] = [0; 4];
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiLumaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, pBS, 0);
        deblock_luma_lt4(pix, iStride as isize, 1, iAlpha, iBeta, &iTc);
    }
}

/// [`FilteringEdgeLumaH`]'s vertical-edge twin: the taps step by one byte.
fn FilteringEdgeLumaV(pFilter: &SDeblockingFilter, pix: &mut RecCursor<'_>, iStride: i32, pBS: &[u8; 4]) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    let mut iTc: [i8; 4] = [0; 4];
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiLumaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, pBS, 0);
        deblock_luma_lt4(pix, 1, iStride as isize, iAlpha, iBeta, &iTc);
    }
}

/// The bS == 4 (intra boundary) strong filter — no `pBS`, because the boundary
/// strength is what selected this function.
fn FilteringEdgeLumaIntraH(pFilter: &SDeblockingFilter, pix: &mut RecCursor<'_>, iStride: i32) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiLumaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        deblock_luma_eq4(pix, iStride as isize, 1, iAlpha, iBeta);
    }
}

/// [`FilteringEdgeLumaIntraH`]'s vertical-edge twin.
fn FilteringEdgeLumaIntraV(pFilter: &SDeblockingFilter, pix: &mut RecCursor<'_>, iStride: i32) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiLumaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        deblock_luma_eq4(pix, 1, iStride as isize, iAlpha, iBeta);
    }
}

/// Chroma takes two cursors, one per plane: the C++ filters Cb and Cr line by
/// line in one call, and `deblock_chroma_lt4` keeps that interleaving.
fn FilteringEdgeChromaH(
    pFilter: &SDeblockingFilter,
    cb: &mut RecCursor<'_>,
    cr: &mut RecCursor<'_>,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    let mut iTc: [i8; 4] = [0; 4];
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiChromaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, pBS, 1);
        deblock_chroma_lt4(cb, cr, iStride as isize, 1, iAlpha, iBeta, &iTc);
    }
}

/// [`FilteringEdgeChromaH`]'s vertical-edge twin.
fn FilteringEdgeChromaV(
    pFilter: &SDeblockingFilter,
    cb: &mut RecCursor<'_>,
    cr: &mut RecCursor<'_>,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    let mut iTc: [i8; 4] = [0; 4];
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiChromaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, pBS, 1);
        deblock_chroma_lt4(cb, cr, 1, iStride as isize, iAlpha, iBeta, &iTc);
    }
}

/// The bS == 4 chroma strong filter.
fn FilteringEdgeChromaIntraH(
    pFilter: &SDeblockingFilter,
    cb: &mut RecCursor<'_>,
    cr: &mut RecCursor<'_>,
    iStride: i32,
) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiChromaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        deblock_chroma_eq4(cb, cr, iStride as isize, 1, iAlpha, iBeta);
    }
}

/// [`FilteringEdgeChromaIntraH`]'s vertical-edge twin.
fn FilteringEdgeChromaIntraV(
    pFilter: &SDeblockingFilter,
    cb: &mut RecCursor<'_>,
    cr: &mut RecCursor<'_>,
    iStride: i32,
) {
    let (mut iIdexA, mut iAlpha, mut iBeta) = (0i32, 0i32, 0i32);
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiChromaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA, &mut iAlpha, &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        deblock_chroma_eq4(cb, cr, 1, iStride as isize, iAlpha, iBeta);
    }
}

// ============================================================================
// Macroblock Deblocking Execution
// ============================================================================

/// The inter macroblock's eight edges.
///
/// The validity flags and the three plane cursors are the caller's — computed once in
/// [`DeblockingMbAvcbase`] where the boundary strengths need them too, rather than
/// re-derived here from the slice map and `mb_cursors` as they were.
pub fn DeblockingInterMb(
    mbs: &MbSplit<'_, SMB>,
    pFilter: &mut SDeblockingFilter,
    uiBS: &[[[u8; 4]; 4]; 2],
    cursors: (RecCursor<'_>, RecCursor<'_>, RecCursor<'_>),
    iLeftFlag: bool,
    iTopFlag: bool,
) {
    let iCurLumaQp = mbs.cur().uiLumaQp as i8;
    let iCurChromaQp = mbs.cur().uiChromaQp as i8;
    let iLineSize = pFilter.iCsStride[0];
    let iLineSizeUV = pFilter.iCsStride[1];

    let (mut pDestY, mut pDestCb, mut pDestCr) = cursors;

    if iLeftFlag {
        let leftMb = mbs.left();
        pFilter.uiLumaQP =
            ((iCurLumaQp as i32 + leftMb.uiLumaQp as i32 + 1) >> 1) as u8;
        pFilter.uiChromaQP =
            ((iCurChromaQp as i32 + leftMb.uiChromaQp as i32 + 1) >> 1) as u8;

        if uiBS[0][0][0] == 0x04 {
            FilteringEdgeLumaIntraV(&*pFilter, &mut pDestY, iLineSize);
            FilteringEdgeChromaIntraV(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSizeUV);
        } else {
            let bs00_u32 = u32::from_ne_bytes(uiBS[0][0]);
            if bs00_u32 != 0 {
                FilteringEdgeLumaV(&*pFilter, &mut pDestY, iLineSize, &uiBS[0][0]);
                FilteringEdgeChromaV(
                    &*pFilter,
                    &mut pDestCb,
                    &mut pDestCr,
                    iLineSizeUV,
                    &uiBS[0][0],
                );
            }
        }
    }

    pFilter.uiLumaQP = iCurLumaQp as u8;
    pFilter.uiChromaQP = iCurChromaQp as u8;

    let bs01_u32 = u32::from_ne_bytes(uiBS[0][1]);
    if bs01_u32 != 0 {
        FilteringEdgeLumaV(
            &*pFilter,
            &mut pDestY.advance(4, 0),
            iLineSize,
            &uiBS[0][1],
        );
    }

    let bs02_u32 = u32::from_ne_bytes(uiBS[0][2]);
    if bs02_u32 != 0 {
        FilteringEdgeLumaV(
            &*pFilter,
            &mut pDestY.advance(8, 0),
            iLineSize,
            &uiBS[0][2],
        );
        FilteringEdgeChromaV(
            &*pFilter,
            &mut pDestCb.advance(4, 0),
            &mut pDestCr.advance(4, 0),
            iLineSizeUV,
            &uiBS[0][2],
        );
    }

    let bs03_u32 = u32::from_ne_bytes(uiBS[0][3]);
    if bs03_u32 != 0 {
        FilteringEdgeLumaV(
            &*pFilter,
            &mut pDestY.advance(12, 0),
            iLineSize,
            &uiBS[0][3],
        );
    }

    if iTopFlag {
        let topMb = mbs.top();
        pFilter.uiLumaQP =
            ((iCurLumaQp as i32 + topMb.uiLumaQp as i32 + 1) >> 1) as u8;
        pFilter.uiChromaQP =
            ((iCurChromaQp as i32 + topMb.uiChromaQp as i32 + 1) >> 1) as u8;

        if uiBS[1][0][0] == 0x04 {
            FilteringEdgeLumaIntraH(&*pFilter, &mut pDestY, iLineSize);
            FilteringEdgeChromaIntraH(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSizeUV);
        } else {
            let bs10_u32 = u32::from_ne_bytes(uiBS[1][0]);
            if bs10_u32 != 0 {
                FilteringEdgeLumaH(&*pFilter, &mut pDestY, iLineSize, &uiBS[1][0]);
                FilteringEdgeChromaH(
                    &*pFilter,
                    &mut pDestCb,
                    &mut pDestCr,
                    iLineSizeUV,
                    &uiBS[1][0],
                );
            }
        }
    }

    pFilter.uiLumaQP = iCurLumaQp as u8;
    pFilter.uiChromaQP = iCurChromaQp as u8;

    let bs11_u32 = u32::from_ne_bytes(uiBS[1][1]);
    if bs11_u32 != 0 {
        FilteringEdgeLumaH(
            &*pFilter,
            &mut pDestY.advance(0, 4),
            iLineSize,
            &uiBS[1][1],
        );
    }

    let bs12_u32 = u32::from_ne_bytes(uiBS[1][2]);
    if bs12_u32 != 0 {
        FilteringEdgeLumaH(
            &*pFilter,
            &mut pDestY.advance(0, 8),
            iLineSize,
            &uiBS[1][2],
        );
        FilteringEdgeChromaH(
            &*pFilter,
            &mut pDestCb.advance(0, 4),
            &mut pDestCr.advance(0, 4),
            iLineSizeUV,
            &uiBS[1][2],
        );
    }

    let bs13_u32 = u32::from_ne_bytes(uiBS[1][3]);
    if bs13_u32 != 0 {
        FilteringEdgeLumaH(
            &*pFilter,
            &mut pDestY.advance(0, 12),
            iLineSize,
            &uiBS[1][3],
        );
    }
}

/// The intra macroblock's luma edges. Flags and cursor from the caller, as in
/// [`DeblockingInterMb`].
pub fn FilteringEdgeLumaHV(
    mbs: &MbSplit<'_, SMB>,
    pFilter: &mut SDeblockingFilter,
    mut pDestY: RecCursor<'_>,
    iLeftFlag: bool,
    iTopFlag: bool,
) {
    let iLineSize = pFilter.iCsStride[0];

    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    let mut iTc: [i8; 4] = [0; 4];
    let uiBSx4: [u8; 4] = [0x03, 0x03, 0x03, 0x03];

    let iCurQp = mbs.cur().uiLumaQp as i8;

    // Luma vertical edges
    if iLeftFlag {
        pFilter.uiLumaQP =
            ((iCurQp as i32 + mbs.left().uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraV(&*pFilter, &mut pDestY, iLineSize);
    }

    pFilter.uiLumaQP = iCurQp as u8;
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiLumaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, &uiBSx4, 0);
                    deblock_luma_lt4(&mut pDestY.advance(4, 0), 1, iLineSize as isize, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(8, 0), 1, iLineSize as isize, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(12, 0), 1, iLineSize as isize, iAlpha, iBeta, &iTc);
    }

    // Luma horizontal edges
    if iTopFlag {
        pFilter.uiLumaQP =
            ((iCurQp as i32 + mbs.top().uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraH(&*pFilter, &mut pDestY, iLineSize);
    }

    pFilter.uiLumaQP = iCurQp as u8;
    if (iAlpha | iBeta) != 0 {
                    deblock_luma_lt4(&mut pDestY.advance(0, 4), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(0, 8), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(0, 12), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
    }
}

/// The intra macroblock's chroma edges. Flags and cursors from the caller.
pub fn FilteringEdgeChromaHV(
    mbs: &MbSplit<'_, SMB>,
    pFilter: &mut SDeblockingFilter,
    mut pDestCb: RecCursor<'_>,
    mut pDestCr: RecCursor<'_>,
    iLeftFlag: bool,
    iTopFlag: bool,
) {
    let iLineSize = pFilter.iCsStride[1];

    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    let mut iTc: [i8; 4] = [0; 4];
    let uiBSx4: [u8; 4] = [0x03, 0x03, 0x03, 0x03];

    let iCurQp = mbs.cur().uiChromaQp as i8;

    // Chroma vertical edges
    if iLeftFlag {
        pFilter.uiChromaQP =
            ((iCurQp as i32 + mbs.left().uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraV(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSize);
    }

    pFilter.uiChromaQP = iCurQp as u8;
    GET_ALPHA_BETA_FROM_QP(
        pFilter.uiChromaQP as i32,
        pFilter.iSliceAlphaC0Offset as i32,
        pFilter.iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );
    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, &uiBSx4, 1);
        deblock_chroma_lt4(
            &mut pDestCb.advance(4, 0),
            &mut pDestCr.advance(4, 0),
            1,
            iLineSize as isize,
            iAlpha,
            iBeta,
            &iTc,
        );
    }

    // Chroma horizontal edges
    if iTopFlag {
        pFilter.uiChromaQP =
            ((iCurQp as i32 + mbs.top().uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraH(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSize);
    }

    pFilter.uiChromaQP = iCurQp as u8;
    if (iAlpha | iBeta) != 0 {
        deblock_chroma_lt4(
            &mut pDestCb.advance(0, 4),
            &mut pDestCr.advance(0, 4),
            iLineSize as isize,
            1,
            iAlpha,
            iBeta,
            &iTc,
        );
    }
}

#[inline(always)]
pub fn DeblockingIntraMb(
    mbs: &MbSplit<'_, SMB>,
    pFilter: &mut SDeblockingFilter,
    cursors: (RecCursor<'_>, RecCursor<'_>, RecCursor<'_>),
    iLeftFlag: bool,
    iTopFlag: bool,
) {
    let (pDestY, pDestCb, pDestCr) = cursors;
    FilteringEdgeLumaHV(mbs, pFilter, pDestY, iLeftFlag, iTopFlag);
    FilteringEdgeChromaHV(mbs, pFilter, pDestCb, pDestCr, iLeftFlag, iTopFlag);
}

/// One macroblock's filter, and **the one place its neighbour flags and plane cursors
/// are computed**.
///
/// Both used to be re-derived by each callee — the flags three times from the slice
/// map, the cursors twice through `mb_cursors`, which is six `SharedPlane::cursor`
/// constructions — for values that are the macroblock's and settled here. The C++ does
/// the same thing in `DeblockingMbAvcbase` and passes `pFilter` down with the
/// pointers already in it.
///
/// The flags are also *lazier* than the array pair they replace: `bLeftBsValid[2]`
/// built both elements, so the slice map was read even at `uiFilterIdc == 0`, which
/// only ever selects element 0. The read is relaxed and side-effect-free, so dropping
/// it changes nothing but the work.
pub fn DeblockingMbAvcbase(
    view: &RecPicView,
    map: &[AtomicU16],
    mbs: &mut MbSplit<'_, SMB>,
    pFilter: &mut SDeblockingFilter,
) {
    // deblocking.cpp:629 — `uint8_t uiBS[2][4][4]`, two 4x4 planes (vertical and
    // horizontal edges).
    let mut uiBS: [[[u8; 4]; 4]; 2] = [[[0; 4]; 4]; 2];
    let cur = mbs.cur();
    let uiCurMbType = cur.uiMbType;
    let iMbStride = pFilter.iMbStride as isize;

    let iMbX = cur.iMbX as i32;
    let iMbY = cur.iMbY as i32;
    let kiMbXY = cur.iMbXY;
    let uiSliceIdc = cur.uiSliceIdc;
    let bWithinSlice = pFilter.uiFilterIdc != 0;

    let iLeftFlag = iMbX > 0
        && (!bWithinSlice
            || uiSliceIdc == map[(kiMbXY - 1) as usize].load(Ordering::Relaxed));
    let iTopFlag = iMbY > 0
        && (!bWithinSlice
            || uiSliceIdc
                == map[(kiMbXY - iMbStride as i32) as usize].load(Ordering::Relaxed));

    let cursors = mb_cursors(view, iMbX, iMbY);

    match uiCurMbType {
        MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA_PCM => {
            DeblockingIntraMb(mbs, pFilter, cursors, iLeftFlag, iTopFlag);
        }
        _ => {
            DeblockingBSCalc(mbs, &mut uiBS, uiCurMbType, iLeftFlag, iTopFlag);
            DeblockingInterMb(mbs, pFilter, &uiBS, cursors, iLeftFlag, iTopFlag);
        }
    }
}

// ============================================================================
// Frame and Slice Level Traversal
// ============================================================================

pub fn DeblockingFilterFrameAvcbase(pCurDq: &mut SDqLayer) {
    if pCurDq.pDecPic.is_none() {
        return;
    }
    let (kuiDisableDeblockingFilterIdc, kiSliceAlphaC0Offset, kiSliceBetaOffset) = {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, 0) else {
            return;
        };
        let sh = &pSlice.sSliceHeaderExt.sSliceHeader;
        (sh.uiDisableDeblockingFilterIdc, sh.iSliceAlphaC0Offset, sh.iSliceBetaOffset)
    };
    let kiMbWidth = pCurDq.iMbWidth;
    let kiMbHeight = pCurDq.iMbHeight;

    if kuiDisableDeblockingFilterIdc == 1 {
        return;
    }

    let mut pFilter = SDeblockingFilter::default();
    pFilter.uiFilterIdc = if kuiDisableDeblockingFilterIdc != 0 { 1 } else { 0 };

    let Some(view) = pCurDq.pRecView.as_ref() else {
        return;
    };
    pFilter.iCsStride[0] = pCurDq.iCsStride[0];
    pFilter.iCsStride[1] = pCurDq.iCsStride[1];
    pFilter.iCsStride[2] = pCurDq.iCsStride[2];

    pFilter.iSliceAlphaC0Offset = kiSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = kiSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth;

    let map: &[AtomicU16] = &pCurDq.sSliceEncCtx.pOverallMbMap;

    // The whole grid as one window: this walk is the single-threaded frame
    // filter, the one deblocking path where the guards' `[0]` mode legitimately
    // reads a neighbour record across a slice boundary — so its window is the
    // grid.
    let mut mbs = MbWindow::whole(&mut pCurDq.sMbDataP, 0);
    for iMbY in 0..kiMbHeight as usize {
        for iMbX in 0..kiMbWidth as usize {
            mbs.set_cur(iMbY * kiMbWidth as usize + iMbX);
            DeblockingMbAvcbase(view, map, &mut mbs.split_cur(), &mut pFilter);
        }
    }
}

// `GetCurrentSliceNum` — svc_encode_slice.cpp.
pub use crate::encoder::svc_encode_slice::GetCurrentSliceNum;

// `WelsGetNextMbOfSlice` — svc_enc_slice_segment.cpp:556.
pub use crate::encoder::svc_encode_slice::WelsGetNextMbOfSlice;

/// The per-slice walker.
pub extern "C" fn DeblockingFilterSliceAvcbase(
    view: &RecPicView,
    pSliceCtx: &crate::encoder::slice_multi_threading::SSliceCtx,
    kiCsStride: &[i32; 3],
    pSlice: &mut SSlice,
    pMbs: &mut MbWindow<'_, SMB>,
) {
    let sSliceHeaderExt = &pSlice.sSliceHeaderExt;

    let kiMbWidth: i32 = pSliceCtx.iMbWidth as i32;
    let kiTotalNumMb: i32 = pSliceCtx.iMbNumInFrame;
    let mut iNumMbFiltered = 0i32;

    if sSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc == 1 {
        return;
    }

    let mut pFilter = SDeblockingFilter::default();
    pFilter.uiFilterIdc = if sSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc != 0 {
        1
    } else {
        0
    };

    pFilter.iCsStride[0] = kiCsStride[0];
    pFilter.iCsStride[1] = kiCsStride[1];
    pFilter.iCsStride[2] = kiCsStride[2];

    pFilter.iSliceAlphaC0Offset = sSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = sSliceHeaderExt.sSliceHeader.iSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth as i16;

    let map: &[AtomicU16] = &pSliceCtx.pOverallMbMap;

    let mut iNextMbIdx = sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

    loop {
        let iCurMbIdx = iNextMbIdx;
        // A walk step outside the caller's window panics with the coordinates.
        pMbs.set_cur(iCurMbIdx as usize);
        DeblockingMbAvcbase(view, map, &mut pMbs.split_cur(), &mut pFilter);

        iNumMbFiltered += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pSliceCtx, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbFiltered >= kiTotalNumMb {
            break;
        }
    }
}

pub extern "C" fn DeblockingFilterSliceAvcbaseNull(
    _view: &RecPicView,
    _pSliceCtx: &crate::encoder::slice_multi_threading::SSliceCtx,
    _kiCsStride: &[i32; 3],
    _pSlice: &mut SSlice,
    _pMbs: &mut MbWindow<'_, SMB>,
) {
}

pub extern "C" fn PerformDeblockingFilter(pEnc: &mut sWelsEncCtx) {
    let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pEnc);

    if pCurDq.iLoopFilterDisableIdc == 0 {
        DeblockingFilterFrameAvcbase(pCurDq);
    } else if pCurDq.iLoopFilterDisableIdc == 2 {
        let iSliceCount = GetCurrentSliceNum(&*pCurDq);
        // The layer's disjoint fields, split once — the grid becomes one
        // whole window and each slice comes out of the banks, all from a single
        // `&mut SDqLayer`.
        let SDqLayer {
            sMbDataP,
            sSliceEncCtx,
            sSliceBufferInfo,
            ppSliceInLayer,
            pRecView,
            iCsStride,
            ..
        } = pCurDq;
        let Some(view) = pRecView.as_ref() else {
            return;
        };
        let mut sMbWindow = MbWindow::whole(sMbDataP, 0);
        for iSliceIdx in 0..iSliceCount {
            let Some(&loc) = ppSliceInLayer.get(iSliceIdx as usize) else {
                continue;
            };
            if loc.offset < 0 {
                continue;
            }
            let Some(pSlice) = sSliceBufferInfo
                .get_mut(loc.bank as usize)
                .and_then(|b| b.pSliceBuffer.get_mut(loc.offset as usize))
            else {
                continue;
            };
            DeblockingFilterSliceAvcbase(view, sSliceEncCtx, iCsStride, pSlice, &mut sMbWindow);
        }
    }
}

// ============================================================================
// Architecture and Dispatch Table Initialization
// ============================================================================

pub fn DeblockingInit(pFunc: &mut DeblockingFunc, _iCpu: i32) {
    pFunc.pfDeblockingFilterSlice = Some(DeblockingFilterSliceAvcbase);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_alpha_beta_table_lookups() {
        let mut idxA = 0i32;
        let mut alpha = 0i32;
        let mut beta = 0i32;

        GET_ALPHA_BETA_FROM_QP(28, 0, 0, &mut idxA, &mut alpha, &mut beta);
        assert_eq!(idxA, 28);
        assert_eq!(alpha, 20);
        assert_eq!(beta, 7);

        GET_ALPHA_BETA_FROM_QP(0, -4, -4, &mut idxA, &mut alpha, &mut beta);
        assert_eq!(idxA, 0);
        assert_eq!(alpha, 0);
        assert_eq!(beta, 0);
    }

    #[test]
    fn test_tc0_table_lookup() {
        let mut iTc: [i8; 4] = [0; 4];
        let bs: [u8; 4] = [1, 2, 3, 0];
        TC0_TBL_LOOKUP(&mut iTc, 28, &bs, 0);
        assert_eq!(iTc[0], 1);
        assert_eq!(iTc[1], 1);
        assert_eq!(iTc[2], 2);
        assert_eq!(iTc[3], -1);
    }


    // ========================================================================
    // Boundary strength
    // ========================================================================

    /// **The three-way scalar this replaced, kept verbatim as the test's oracle.**
    ///
    /// `DeblockingBSCalc_c` as it stood before the strength calculation became one
    /// kernel over plain data: the marginal edges through
    /// `DeblockingBSMarginalMBAvcbase`, then one of three interior bodies chosen by
    /// the macroblock kind, with `WelsNonZeroCount_c` run over the current
    /// macroblock's counts in between. Everything below is held against this.
    fn bs_calc_three_way(
        cur: &mut SMB,
        left: Option<&SMB>,
        top: Option<&SMB>,
        uiBS: &mut [[[u8; 4]; 4]; 2],
    ) {
        uiBS[0][0] = match left {
            Some(m) => {
                let v = if IS_INTRA(m.uiMbType) {
                    0x04040404u32
                } else {
                    DeblockingBSMarginalMBAvcbase(cur, m, 0)
                };
                v.to_ne_bytes()
            }
            None => [0; 4],
        };
        uiBS[1][0] = match top {
            Some(m) => {
                let v = if IS_INTRA(m.uiMbType) {
                    0x04040404u32
                } else {
                    DeblockingBSMarginalMBAvcbase(cur, m, 1)
                };
                v.to_ne_bytes()
            }
            None => [0; 4],
        };
        if cur.uiMbType != MB_TYPE_SKIP {
            WelsNonZeroCount_c(&mut cur.iNonZeroCount);
            if cur.uiMbType == MB_TYPE_16x16 {
                DeblockingBSInsideMBAvsbase(&cur.iNonZeroCount, uiBS, 1);
            } else {
                let mv = cur.sMv;
                DeblockingBSInsideMBNormal(&mv, uiBS, &cur.iNonZeroCount);
            }
        } else {
            for dir in 0..2 {
                for edge in 1..4 {
                    uiBS[dir][edge] = [0; 4];
                }
            }
        }
    }

    /// The path `DeblockingMbAvcbase` runs: normalise, kernel, intra override.
    fn bs_calc_via_kernel(
        cur: &mut SMB,
        left: Option<&SMB>,
        top: Option<&SMB>,
        uiBS: &mut [[[u8; 4]; 4]; 2],
    ) {
        if cur.uiMbType != MB_TYPE_SKIP {
            WelsNonZeroCount_c(&mut cur.iNonZeroCount);
        }
        kernels::deblock::bs_calc(
            &cur.iNonZeroCount,
            &cur.sMv,
            left.map(|m| (&m.iNonZeroCount, &m.sMv)),
            top.map(|m| (&m.iNonZeroCount, &m.sMv)),
            inside_bs_mask(cur.uiMbType),
            uiBS,
        );
        if let Some(m) = left {
            if IS_INTRA(m.uiMbType) {
                uiBS[0][0] = [4; 4];
            }
        }
        if let Some(m) = top {
            if IS_INTRA(m.uiMbType) {
                uiBS[1][0] = [4; 4];
            }
        }
    }

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 32) as u32
        }

        /// A motion vector concentrated on the +/-3 / +/-4 threshold the strength test
        /// turns on, with the occasional extreme so the 16-bit lanes are exercised at
        /// their ends.
        fn mv(&mut self) -> SMVUnitXY {
            let mut c = || match self.next() % 10 {
                0..=6 => (self.next() % 11) as i16 - 5,
                7 | 8 => (self.next() % 2001) as i16 - 1000,
                _ => [i16::MIN, i16::MAX, -32000, 32000][(self.next() % 4) as usize],
            };
            SMVUnitXY { iMvX: c(), iMvY: c() }
        }

        /// A macroblock of the given kind, with counts either raw or already 0/1 —
        /// both occur, because a skip macroblock's are not normalised.
        fn mb(&mut self, uiMbType: u32, raw_counts: bool, uniform_mv: bool) -> SMB {
            let mut mb = SMB { uiMbType, ..Default::default() };
            for n in mb.iNonZeroCount.iter_mut() {
                *n = match self.next() % 4 {
                    0 | 1 => 0,
                    _ if raw_counts => (self.next() % 17) as i8,
                    _ => 1,
                };
            }
            let one = self.mv();
            for m in mb.sMv.iter_mut() {
                *m = if uniform_mv { one } else { self.mv() };
            }
            mb
        }
    }

    /// **The kernel and the three-way scalar agree, byte for byte, over every
    /// combination that reaches them.**
    ///
    /// Both neighbour flags on and off, intra and inter neighbours, all three
    /// macroblock kinds, raw and normalised counts, and motion vectors drawn around
    /// the whole-sample threshold and at the ends of the 16-bit range.
    #[test]
    fn the_boundary_strength_kernel_matches_the_three_way_scalar() {
        let mut r = Lcg(0x5EED_1234_ABCD_0001);
        let kinds = [
            MB_TYPE_16x16,
            MB_TYPE_SKIP,
            MB_TYPE_16x8,
            MB_TYPE_8x16,
            MB_TYPE_8x8,
        ];
        // 400 rounds natively; a tenth of that keeps the Miri filter for this module
        // inside its three-minute budget, and the combinations below are what the
        // coverage rests on rather than the round count.
        for round in 0..if cfg!(miri) { 40 } else { 400 } {
            for &kind in &kinds {
                // A 16x16 or skip macroblock carries one vector in all sixteen
                // blocks; every other kind may differ block to block.
                let uniform = kind == MB_TYPE_16x16 || kind == MB_TYPE_SKIP;
                let raw = kind == MB_TYPE_SKIP;
                let cur = r.mb(kind, raw, uniform);
                for lk in 0..3 {
                    for tk in 0..3 {
                        let nb = |k: usize, r: &mut Lcg| match k {
                            0 => None,
                            1 => Some(r.mb(MB_TYPE_8x8, true, false)),
                            _ => Some(r.mb(MB_TYPE_INTRA16x16, true, false)),
                        };
                        let l = nb(lk, &mut r);
                        let t = nb(tk, &mut r);
                        let (mut a, mut b) = (cur.clone(), cur.clone());
                        let (mut want, mut got) = ([[[0u8; 4]; 4]; 2], [[[0u8; 4]; 4]; 2]);
                        bs_calc_three_way(&mut a, l.as_ref(), t.as_ref(), &mut want);
                        bs_calc_via_kernel(&mut b, l.as_ref(), t.as_ref(), &mut got);
                        assert_eq!(
                            want, got,
                            "round {round} kind {kind:#x} left {lk} top {tk}"
                        );
                        // The count normalisation is observable — later readers of
                        // `iNonZeroCount` see it — so it has to survive too.
                        assert_eq!(
                            a.iNonZeroCount, b.iNonZeroCount,
                            "the write-back of the normalised counts, round {round}"
                        );
                    }
                }
            }
        }
    }

    /// **Why upstream's asm can leave the two kind masks out, and why this port
    /// applies them anyway.**
    ///
    /// `DeblockingBSCalcEnc_AArch64_neon` computes one rule for every interior edge —
    /// the partitioned-inter one — where `DeblockingBSCalc_c` has three. It gets away
    /// with it on the data the encoder produces: a `MB_TYPE_16x16` macroblock has one
    /// motion vector replicated to all sixteen blocks, so the vector term is zero and
    /// the coefficient term is all that is left, which is `DeblockingBSInsideMBAvsbase`;
    /// and a skip macroblock has that *and* no coefficients, so its interior is zero.
    ///
    /// This asserts exactly that — with `0xFF` for `inside`, the unmasked rule already
    /// equals the kind's own — and, by the negative half, that it is a property of the
    /// data and not of the kernel: give a 16x16 macroblock two different vectors and
    /// the masks start to matter.
    #[test]
    fn the_kind_masks_are_no_ops_on_the_data_the_encoder_produces() {
        let mut r = Lcg(0x5EED_1234_ABCD_0002);
        for _ in 0..if cfg!(miri) { 20 } else { 200 } {
            for &kind in &[MB_TYPE_16x16, MB_TYPE_SKIP] {
                let cur = r.mb(kind, false, true);
                let cur = if kind == MB_TYPE_SKIP {
                    SMB { iNonZeroCount: [0; MB_LUMA_CHROMA_BLOCK4x4_NUM], ..cur }
                } else {
                    cur
                };
                let l = r.mb(MB_TYPE_8x8, true, false);
                let (mut masked, mut unmasked) = ([[[0u8; 4]; 4]; 2], [[[0u8; 4]; 4]; 2]);
                for (mask, out) in [(inside_bs_mask(kind), &mut masked), (0xFF, &mut unmasked)] {
                    kernels::deblock::bs_calc(
                        &cur.iNonZeroCount,
                        &cur.sMv,
                        Some((&l.iNonZeroCount, &l.sMv)),
                        None,
                        mask,
                        out,
                    );
                }
                assert_eq!(masked, unmasked, "kind {kind:#x}");
            }
        }

        // The negative half: a 16x16 macroblock whose blocks disagree about the
        // motion vector — which the encoder never builds — is where the mask earns
        // its place, because the unmasked rule would raise those edges to 1.
        let mut cur = SMB { uiMbType: MB_TYPE_16x16, ..Default::default() };
        cur.sMv[5] = SMVUnitXY { iMvX: 64, iMvY: 0 };
        let (mut masked, mut unmasked) = ([[[0u8; 4]; 4]; 2], [[[0u8; 4]; 4]; 2]);
        for (mask, out) in [(inside_bs_mask(MB_TYPE_16x16), &mut masked), (0xFF, &mut unmasked)] {
            kernels::deblock::bs_calc(
                &cur.iNonZeroCount, &cur.sMv, None, None, mask, out,
            );
        }
        assert_ne!(masked, unmasked, "the mask is what makes the 16x16 rule the 16x16 rule");
        assert_eq!(masked, [[[0u8; 4]; 4]; 2], "coefficients are all zero, so the 16x16 rule gives zero");
    }

    #[test]
    fn test_non_zero_count_c() {
        let mut nzc: [i8; 24] = [
            0, 5, 0, 12, -3, 0, 0, 1, 0, 0, 0, 4,
            0, 0, 0, 0, 2, 0, 0, 0, 0, 7, 0, 0,
        ];
        WelsNonZeroCount_c(&mut nzc);
        for (i, &val) in nzc.iter().enumerate() {
            if [1, 3, 4, 7, 11, 16, 21].contains(&i) {
                assert_eq!(val, 1);
            } else {
                assert_eq!(val, 0);
            }
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MSA, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSSE3};
