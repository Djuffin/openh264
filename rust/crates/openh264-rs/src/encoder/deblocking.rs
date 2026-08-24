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
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

// ============================================================================
// Constants and Dimension Definitions
// ============================================================================

#![deny(unsafe_code)]

pub const MB_WIDTH_LUMA: usize = 16;
pub const MB_WIDTH_CHROMA: usize = 8;

// `wels_common_basis.h:123-124`. These were declared here swapped (LEFT 0x02, TOP
// 0x01). Nothing in this module reads them — in C++ they appear only inside the
// `HAVE_NEON && SINGLE_REF_FRAME` boundary-flag argument to DeblockingBSCalcEnc_neon,
// which this port does not dispatch — but the values were still wrong.
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

// CPU Capability Flags

// ============================================================================
// H.264 Deblocking Lookup Tables
// ============================================================================

/// Table 8-16 in H.264/AVC standard: Alpha table indexed by clipped QP + offset (0..51 + padding)
// `g_kuiAlphaTable`/`g_kiBetaTable`/`g_kiTc0Table` are `static const` **file-local**
// in both codecs and are deliberately different sizes: `codec/encoder/core/src/
// deblocking.cpp:72-92` declares `[52 + 12]`, `codec/decoder/core/src/deblocking.cpp:
// 144-166` declares `[52 + 24]`. Two definitions here is correct.
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
use crate::common::deblocking_common::{
    deblock_chroma_eq4, deblock_chroma_lt4, deblock_luma_eq4, deblock_luma_lt4,
};
use crate::encoder::svc_encode_slice::current_layer;

/// Active parameters and pointers for macroblock deblocking filtering.
/// Matches `struct TagDeblockingFilter` in `codec/encoder/core/inc/deblocking.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagDeblockingFilter {
    // `pCsData: [*mut u8; 3]` stood here — three raw roots into the reconstruction
    // picture, re-advanced per macroblock by both drivers. T9.C2 replaced them with
    // `mb_cursors`, which derives the same three addresses from the seam's view and
    // the macroblock's own coordinates.
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

// `TagMB` was declared here: a field-for-field second copy of `SMB` -- five raw
// pointers included -- with no reference anywhere in the crate and the real type
// re-exported on the next line. A census that matches on names could not see it
// (F43's class, under a different name), and T6.C1 would have left it declaring
// fields no live struct has. **S18: deleted, not converted.**
pub use crate::encoder::svc_encode_slice::SMB;
pub use crate::encoder::md::{MB_BLOCK4x4_NUM, MB_LUMA_CHROMA_BLOCK4x4_NUM};
pub type PMb = *mut SMB;

// Function Pointer Typedefs
pub type PLumaDeblockingLT4Func =
    unsafe extern "C" fn(pPixY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32, pTc: *mut i8);

pub type PLumaDeblockingEQ4Func =
    unsafe extern "C" fn(pPixY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32);

pub type PChromaDeblockingLT4Func = unsafe extern "C" fn(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
);

pub type PChromaDeblockingEQ4Func = unsafe extern "C" fn(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
);

// `uiBS` carries its real C++ type end-to-end: `uint8_t uiBS[2][4][4]`
// (`deblocking.cpp:629`) — two 4x4 planes, `[dir][edge][blk]`, dir 0 = vertical
// edges, dir 1 = horizontal. It was previously `*mut [[u8; 4]; 4]` — one plane —
// with the second plane reached through 32-byte `from_raw_parts_mut` casts, which
// is exactly the size relationship whose collapse caused the F1 release segfault
// (`phase0_findings.md`). The F1 surgery (Phase 2 T6) made the type say it.
pub type PDeblockingBSCalc = unsafe extern "C" fn(
    pFunc: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    uiBS: *mut [[[u8; 4]; 4]; 2],
    uiCurMbType: u32,
    iMbStride: i32,
    iLeftFlag: i32,
    iTopFlag: i32,
);

pub type PDeblockingFilterSlice =
    unsafe extern "C" fn(pCurDq: *mut SDqLayer, pFunc: *mut SWelsFuncPtrList, pSlice: *mut SSlice);

/// **T6.C1**: the slot took `int8_t*` because the C++ passes `pCurMb->pNonZeroCount`,
/// a pointer into a context-wide array. The array is inline in `SMB` now and every
/// caller has the whole 24-entry row, so the slot takes the row.
pub type PSetNoneZeroCountZeroFunc = fn(pNonZeroCount: &mut [i8; MB_LUMA_CHROMA_BLOCK4x4_NUM]);

/// Function pointer dispatch table for deblocking routines.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct tagDeblockingFunc {
    pub pfLumaDeblockingLT4Ver: Option<PLumaDeblockingLT4Func>,
    pub pfLumaDeblockingEQ4Ver: Option<PLumaDeblockingEQ4Func>,
    pub pfLumaDeblockingLT4Hor: Option<PLumaDeblockingLT4Func>,
    pub pfLumaDeblockingEQ4Hor: Option<PLumaDeblockingEQ4Func>,
    pub pfChromaDeblockingLT4Ver: Option<PChromaDeblockingLT4Func>,
    pub pfChromaDeblockingEQ4Ver: Option<PChromaDeblockingEQ4Func>,
    pub pfChromaDeblockingLT4Hor: Option<PChromaDeblockingLT4Func>,
    pub pfChromaDeblockingEQ4Hor: Option<PChromaDeblockingEQ4Func>,
    pub pfDeblockingBSCalc: Option<PDeblockingBSCalc>,
    pub pfDeblockingFilterSlice: Option<PDeblockingFilterSlice>,
}

pub type DeblockingFunc = tagDeblockingFunc;

pub use crate::encoder::encoder_context::{SPicture, SWelsFuncPtrList, sWelsEncCtx};
use crate::encoder::encoder_context::ctx_func_list;
pub use crate::encoder::svc_encode_slice::{SDqLayer, SSlice};

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
///
/// **T6.C1**: took `pCurMb: *mut SMB` and read one field of it. It takes the field.
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

/// Reference C implementation of Boundary Strength ($bS$) calculation.
///
/// `uiBS[0][0]` is the left MB-boundary edge, `uiBS[1][0]` the top one;
/// `uiBS[dir][1..4]` are the interior edges. The C++ writes the boundary rows
/// through `uint32_t` punning (`*(uint32_t*)uiBS[0][0]`); a 4-byte row
/// assignment is the same store with the type kept.
///
/// # Safety
/// `pCurMb` must be a valid MB pointer with in-bounds left/top neighbours for
/// whichever of `iLeftFlag`/`iTopFlag` is set, `pFunc` null or valid, and
/// `uiBS` must point at a writable `[2][4][4]` boundary-strength array.
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn DeblockingBSCalc_c(
    pFunc: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    uiBS: *mut [[[u8; 4]; 4]; 2],
    uiCurMbType: u32,
    iMbStride: i32,
    iLeftFlag: i32,
    iTopFlag: i32,
) {
    let uiBS = &mut *uiBS;
    if iLeftFlag != 0 {
        let leftMb = pCurMb.offset(-1);
        let val = if IS_INTRA((*leftMb).uiMbType) {
            0x04040404u32
        } else {
            DeblockingBSMarginalMBAvcbase(&*pCurMb, &*leftMb, 0)
        };
        uiBS[0][0] = val.to_ne_bytes();
    } else {
        uiBS[0][0] = [0; 4];
    }

    if iTopFlag != 0 {
        let topMb = pCurMb.offset(-(iMbStride as isize));
        let val = if IS_INTRA((*topMb).uiMbType) {
            0x04040404u32
        } else {
            DeblockingBSMarginalMBAvcbase(&*pCurMb, &*topMb, 1)
        };
        uiBS[1][0] = val.to_ne_bytes();
    } else {
        uiBS[1][0] = [0; 4];
    }

    if uiCurMbType != MB_TYPE_SKIP {
        if !pFunc.is_null() {
            if let Some(set_nzc) = (*pFunc).pfSetNZCZero {
                // deblocking.cpp:615 — one argument.
                set_nzc(&mut (*pCurMb).iNonZeroCount);
            }
        }
        if uiCurMbType == MB_TYPE_16x16 {
            DeblockingBSInsideMBAvsbase(&(*pCurMb).iNonZeroCount, uiBS, 1);
        } else {
            DeblockingBSInsideMBNormal(&(*pCurMb).sMv, uiBS, &(*pCurMb).iNonZeroCount);
        }
    } else {
        for dir in 0..2 {
            for edge in 1..4 {
                uiBS[dir][edge] = [0; 4];
            }
        }
    }
}

// ============================================================================
// Edge filtering — the same kernels the decoder uses (T9 straggler, G-2)
// ============================================================================
//
// This module used to carry its own copies of the eight `Deblock*V_c`/`*H_c`
// ABI wrappers, their four inner kernels, and `WelsNonZeroCount_c`, duplicating
// `common/deblocking_common.rs` line for line. T6 converted that module and the
// decoder picked the conversion up by re-exporting it; the encoder kept the
// duplicates, so half the family stayed raw on the encoder's mainline path
// until T9's straggler sweep found it.
//
// The eight wrappers are now the common module's shims, re-exported rather than
// re-implemented. That is a deduplication, not a unification of two things that
// merely share a name: the bodies were proven byte-for-byte equivalent over
// `ALPHAS` x `BETAS` x three strides x V/H before this commit
// (`encoder_deblock_*_kernels_match_the_common_safe_ones`, commit A), and the
// signatures are identical. The name-collision discipline that keeps the three
// `WelsI4x4LumaPredV_c`s apart says never unify functions that *differ*; these
// do not.
//
// The common module's availability argument already speaks for this caller — it
// names `encoder/deblocking.rs`'s `bLeftBsValid`/`bTopBsValid` beside the
// decoder's gate — so the contracts move across unchanged.
//
// `DeblockingInit` below installs these names exactly as before; no dispatch
// table changes here (that is Phase 4a's).
pub use crate::common::deblocking_common::{
    DeblockChromaEq4H_c, DeblockChromaEq4V_c, DeblockChromaLt4H_c, DeblockChromaLt4V_c,
    DeblockLumaEq4H_c, DeblockLumaEq4V_c, DeblockLumaLt4H_c, DeblockLumaLt4V_c,
};

/// C++: `WelsNonZeroCount_c` — the encoder's copy, installed into `pfSetNZCZero`.
/// The common module's shim still takes a raw pointer (the decoder's callers hold
/// one), so this stays a distinct function; it is the safe kernel's one-line
/// forwarder since T6.C1 took the raw pointer out of the slot.
pub fn WelsNonZeroCount_c(pNonZeroCount: &mut [i8; MB_LUMA_CHROMA_BLOCK4x4_NUM]) {
    crate::common::deblocking_common::nonzero_count(pNonZeroCount);
}


// ============================================================================
// Directional Filtering Dispatchers
// ============================================================================

/// This macroblock's three reconstruction cursors.
///
/// **T9.C2.** `SDeblockingFilter` used to carry `pCsData: [*mut u8; 3]`, three
/// raw plane roots that the slice and frame drivers re-advanced per macroblock;
/// the arithmetic is here instead, against the seam's view, and the drivers carry
/// nothing. Luma is 16 samples per macroblock and chroma 8, which is the whole
/// content of the `<< 4` and `<< 3` the drivers used to do.
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

/// The eight directional edge dispatchers — **safe since T9.C2**.
///
/// Each was `(pPix: *mut u8, iStride: i32)` into the reconstruction plane and a
/// `pfDeblocking` slot call. The destination is the seam's cursor now, and the
/// kernel is called directly: `DeblockingInit` installs all ten slots
/// unconditionally and nothing rewrites them, so a fixed-size site may bypass the
/// slot byte-identically (F118).
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DeblockingInterMb(
    view: &RecPicView,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
    uiBS: &[[[u8; 4]; 4]; 2],
) {
    let iCurLumaQp = (*pCurMb).uiLumaQp as i8;
    let iCurChromaQp = (*pCurMb).uiChromaQp as i8;
    let iLineSize = (*pFilter).iCsStride[0];
    let iLineSizeUV = (*pFilter).iCsStride[1];
    let iMbStride = (*pFilter).iMbStride as isize;

    let iMbX = (*pCurMb).iMbX as i32;
    let iMbY = (*pCurMb).iMbY as i32;

    let bLeftBsValid = [
        iMbX > 0,
        iMbX > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-1)).uiSliceIdc),
    ];
    let bTopBsValid = [
        iMbY > 0,
        iMbY > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-iMbStride)).uiSliceIdc),
    ];

    let iLeftFlag = bLeftBsValid[(*pFilter).uiFilterIdc as usize];
    let iTopFlag = bTopBsValid[(*pFilter).uiFilterIdc as usize];

    // **T9.C2**: the three raw roots `pCsData[i]`, advanced to this macroblock by
    // the slice driver, are the seam's three cursors at the same coordinates.
    // Deblocking is the family F108 measured running *inside* the fork, so this is
    // the last per-macroblock raw route into the reconstruction picture.
    let (mut pDestY, mut pDestCb, mut pDestCr) = mb_cursors(view, iMbX, iMbY);

    if iLeftFlag {
        (*pFilter).uiLumaQP =
            ((iCurLumaQp as i32 + (*pCurMb.offset(-1)).uiLumaQp as i32 + 1) >> 1) as u8;
        (*pFilter).uiChromaQP =
            ((iCurChromaQp as i32 + (*pCurMb.offset(-1)).uiChromaQp as i32 + 1) >> 1) as u8;

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

    (*pFilter).uiLumaQP = iCurLumaQp as u8;
    (*pFilter).uiChromaQP = iCurChromaQp as u8;

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
        (*pFilter).uiLumaQP =
            ((iCurLumaQp as i32 + (*pCurMb.offset(-iMbStride)).uiLumaQp as i32 + 1) >> 1) as u8;
        (*pFilter).uiChromaQP =
            ((iCurChromaQp as i32 + (*pCurMb.offset(-iMbStride)).uiChromaQp as i32 + 1) >> 1) as u8;

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

    (*pFilter).uiLumaQP = iCurLumaQp as u8;
    (*pFilter).uiChromaQP = iCurChromaQp as u8;

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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FilteringEdgeLumaHV(
    view: &RecPicView,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    let iLineSize = (*pFilter).iCsStride[0];
    let iMbStride = (*pFilter).iMbStride as isize;

    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    let iMbX = (*pCurMb).iMbX as i32;
    let iMbY = (*pCurMb).iMbY as i32;

    let bLeftBsValid = [
        iMbX > 0,
        iMbX > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-1)).uiSliceIdc),
    ];
    let bTopBsValid = [
        iMbY > 0,
        iMbY > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-iMbStride)).uiSliceIdc),
    ];

    let iLeftFlag = bLeftBsValid[(*pFilter).uiFilterIdc as usize];
    let iTopFlag = bTopBsValid[(*pFilter).uiFilterIdc as usize];

    let mut iTc: [i8; 4] = [0; 4];
    let uiBSx4: [u8; 4] = [0x03, 0x03, 0x03, 0x03];

    let (mut pDestY, _, _) = mb_cursors(view, iMbX, iMbY);
    let iCurQp = (*pCurMb).uiLumaQp as i8;

    // Luma vertical edges
    if iLeftFlag {
        (*pFilter).uiLumaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-1)).uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraV(&*pFilter, &mut pDestY, iLineSize);
    }

    (*pFilter).uiLumaQP = iCurQp as u8;
    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
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
        (*pFilter).uiLumaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-iMbStride)).uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraH(&*pFilter, &mut pDestY, iLineSize);
    }

    (*pFilter).uiLumaQP = iCurQp as u8;
    if (iAlpha | iBeta) != 0 {
                    deblock_luma_lt4(&mut pDestY.advance(0, 4), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(0, 8), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
            deblock_luma_lt4(&mut pDestY.advance(0, 12), iLineSize as isize, 1, iAlpha, iBeta, &iTc);
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FilteringEdgeChromaHV(
    view: &RecPicView,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    let iLineSize = (*pFilter).iCsStride[1];
    let iMbStride = (*pFilter).iMbStride as isize;

    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    let iMbX = (*pCurMb).iMbX as i32;
    let iMbY = (*pCurMb).iMbY as i32;

    let bLeftBsValid = [
        iMbX > 0,
        iMbX > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-1)).uiSliceIdc),
    ];
    let bTopBsValid = [
        iMbY > 0,
        iMbY > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-iMbStride)).uiSliceIdc),
    ];

    let iLeftFlag = bLeftBsValid[(*pFilter).uiFilterIdc as usize];
    let iTopFlag = bTopBsValid[(*pFilter).uiFilterIdc as usize];

    let mut iTc: [i8; 4] = [0; 4];
    let uiBSx4: [u8; 4] = [0x03, 0x03, 0x03, 0x03];

    let (_, mut pDestCb, mut pDestCr) = mb_cursors(view, iMbX, iMbY);
    let iCurQp = (*pCurMb).uiChromaQp as i8;

    // Chroma vertical edges
    if iLeftFlag {
        (*pFilter).uiChromaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-1)).uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraV(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSize);
    }

    (*pFilter).uiChromaQP = iCurQp as u8;
    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiChromaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
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
        (*pFilter).uiChromaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-iMbStride)).uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraH(&*pFilter, &mut &mut pDestCb, &mut &mut pDestCr, iLineSize);
    }

    (*pFilter).uiChromaQP = iCurQp as u8;
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DeblockingIntraMb(
    view: &RecPicView,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    FilteringEdgeLumaHV(view, pCurMb, pFilter);
    FilteringEdgeChromaHV(view, pCurMb, pFilter);
}

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn DeblockingMbAvcbase(
    pFunc: *mut SWelsFuncPtrList,
    view: &RecPicView,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    // deblocking.cpp:629 — `uint8_t uiBS[2][4][4]`, two 4x4 planes (vertical and
    // horizontal edges). Since the F1 surgery the callees take exactly this
    // type, so the 16-vs-32-byte relationship that caused the release segfault
    // is carried by the signatures instead of by five raw casts.
    let mut uiBS: [[[u8; 4]; 4]; 2] = [[[0; 4]; 4]; 2];
    let uiCurMbType = (*pCurMb).uiMbType;
    let iMbStride = (*pFilter).iMbStride as isize;

    let iMbX = (*pCurMb).iMbX as i32;
    let iMbY = (*pCurMb).iMbY as i32;

    let bLeftBsValid = [
        iMbX > 0,
        iMbX > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-1)).uiSliceIdc),
    ];
    let bTopBsValid = [
        iMbY > 0,
        iMbY > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-iMbStride)).uiSliceIdc),
    ];

    let iLeftFlag = bLeftBsValid[(*pFilter).uiFilterIdc as usize] as i32;
    let iTopFlag = bTopBsValid[(*pFilter).uiFilterIdc as usize] as i32;

    let pfDeblocking = &(*pFunc).pfDeblocking as *const DeblockingFunc;

    match uiCurMbType {
        MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA_PCM => {
            DeblockingIntraMb(view, pCurMb, pFilter);
        }
        _ => {
            if let Some(bs_calc) = (*pfDeblocking).pfDeblockingBSCalc {
                bs_calc(
                    pFunc,
                    pCurMb,
                    &mut uiBS,
                    uiCurMbType,
                    iMbStride as i32,
                    iLeftFlag,
                    iTopFlag,
                );
            }
            DeblockingInterMb(view, pCurMb, pFilter, &uiBS);
        }
    }
}

// ============================================================================
// Frame and Slice Level Traversal
// ============================================================================

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn DeblockingFilterFrameAvcbase(pCurDq: *mut SDqLayer, pFunc: *mut SWelsFuncPtrList) {
    if pCurDq.is_null() || (*pCurDq).pDecPic.is_none() {
        return;
    }
    let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, 0);
    if pSlice.is_null() {
        return;
    }
    let kiMbWidth = (*pCurDq).iMbWidth;
    let kiMbHeight = (*pCurDq).iMbHeight;
    let mut pCurrentMbBlock = crate::encoder::svc_encode_slice::mb_list_root(pCurDq);

    let sSliceHeaderExt = &(*pSlice).sSliceHeaderExt;

    if sSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc == 1 {
        return;
    }

    let mut pFilter = SDeblockingFilter::default();
    pFilter.uiFilterIdc = if sSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc != 0 {
        1
    } else {
        0
    };

    // **T9.C4**: this resolved the reconstruction picture to its plane roots with
    // `layer_dec_pic_mut(..).planes()` — a whole-picture `&mut` retag, and F108
    // measured that under multi-threading this filter runs *inside* the fork, so
    // two workers took it at once. The layer already carries those three roots
    // and their three strides: `WelsInitCurrentLayer` stamps `pCsData`/`iCsStride`
    // from the same `planes()` call, before anything spawns. Same numbers, same
    // addresses, one derivation instead of two.
    //
    // The guard is the old `None` arm's two conditions — the layer's handle
    // (tested at the top of this function) and a bound reference list — plus a
    // null root, which the old spelling would have carried into a null deref.
    // **T9.C2**: the three roots and the per-macroblock advance are gone with
    // `pCsData` — `mb_cursors` derives each macroblock's three cursors from the
    // seam view and the macroblock's own `(iMbX, iMbY)`, which is the same
    // arithmetic one level down and cannot drift out of step with the loop.
    if (*pCurDq).pRefList.is_null() {
        return;
    }
    let Some(view) = crate::encoder::svc_encode_slice::layer_rec_view(pCurDq) else {
        return;
    };
    pFilter.iCsStride[0] = (*pCurDq).iCsStride[0];
    pFilter.iCsStride[1] = (*pCurDq).iCsStride[1];
    pFilter.iCsStride[2] = (*pCurDq).iCsStride[2];

    pFilter.iSliceAlphaC0Offset = sSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = sSliceHeaderExt.sSliceHeader.iSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth as i16;

    for _ in 0..kiMbHeight {
        for _ in 0..kiMbWidth {
            DeblockingMbAvcbase(pFunc, view, pCurrentMbBlock, &mut pFilter);
            pCurrentMbBlock = pCurrentMbBlock.add(1);
        }
    }
}

// `GetCurrentSliceNum` — svc_encode_slice.cpp. This module used to declare a copy
// that returned a hardcoded `1`, and `WelsDeblockingFilterMbAvcbase`'s slice loop
// below reads it (`deblocking.cpp:754`), so deblocking only ever filtered slice 0.
// Indistinguishable from correct at one slice per frame; wrong for every other
// slice mode.
pub use crate::encoder::svc_encode_slice::GetCurrentSliceNum;

// `WelsGetNextMbOfSlice` — svc_enc_slice_segment.cpp:556, and `deblocking.cpp:733`
// calls that one. This module used to declare a truncated copy that returned
// `kiMbXY + 1` bounded only by the frame, ignoring `sSliceEncCtx` and
// `pOverallMbMap` entirely. It agrees with the real one for SM_SINGLE_SLICE and
// walks straight across slice boundaries for every other slice mode.
pub use crate::encoder::svc_encode_slice::WelsGetNextMbOfSlice;

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn DeblockingFilterSliceAvcbase(
    pCurDq: *mut SDqLayer,
    pFunc: *mut SWelsFuncPtrList,
    pSlice: *mut SSlice,
) {
    let pMbList = crate::encoder::svc_encode_slice::mb_list_root(pCurDq);
    let sSliceHeaderExt = &(*pSlice).sSliceHeaderExt;

    let kiMbWidth: i32 = (*pCurDq).iMbWidth as i32;
    let kiMbHeight: i32 = (*pCurDq).iMbHeight as i32;
    let kiTotalNumMb: i32 = kiMbWidth * kiMbHeight;
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

    // **T9.C4**: this resolved the reconstruction picture to its plane roots with
    // `layer_dec_pic_mut(..).planes()` — a whole-picture `&mut` retag, and F108
    // measured that under multi-threading this filter runs *inside* the fork, so
    // two workers took it at once. The layer already carries those three roots
    // and their three strides: `WelsInitCurrentLayer` stamps `pCsData`/`iCsStride`
    // from the same `planes()` call, before anything spawns. Same numbers, same
    // addresses, one derivation instead of two.
    //
    // The guard is the old `None` arm's two conditions — the layer's handle
    // (tested at the top of this function) and a bound reference list — plus a
    // null root, which the old spelling would have carried into a null deref.
    if (*pCurDq).pRefList.is_null() {
        return;
    }
    let Some(view) = crate::encoder::svc_encode_slice::layer_rec_view(pCurDq) else {
        return;
    };
    pFilter.iCsStride[0] = (*pCurDq).iCsStride[0];
    pFilter.iCsStride[1] = (*pCurDq).iCsStride[1];
    pFilter.iCsStride[2] = (*pCurDq).iCsStride[2];

    pFilter.iSliceAlphaC0Offset = sSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = sSliceHeaderExt.sSliceHeader.iSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth as i16;

    let mut iNextMbIdx = sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

    loop {
        let iCurMbIdx = iNextMbIdx;
        let pCurrentMbBlock = pMbList.add(iCurMbIdx as usize);

        DeblockingMbAvcbase(pFunc, view, pCurrentMbBlock, &mut pFilter);

        iNumMbFiltered += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurDq, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbFiltered >= kiTotalNumMb {
            break;
        }
    }
}

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn DeblockingFilterSliceAvcbaseNull(
    _pCurDq: *mut SDqLayer,
    _pFunc: *mut SWelsFuncPtrList,
    _pSlice: *mut SSlice,
) {
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn PerformDeblockingFilter(pEnc: *mut sWelsEncCtx) {
    if pEnc.is_null() {
        return;
    }
    let pCurLayer = current_layer(pEnc);
    if pCurLayer.is_null() {
        return;
    }

    if (*pCurLayer).iLoopFilterDisableIdc == 0 {
        DeblockingFilterFrameAvcbase(pCurLayer, ctx_func_list(pEnc));
    } else if (*pCurLayer).iLoopFilterDisableIdc == 2 {
        let iSliceCount = GetCurrentSliceNum(pCurLayer);
        for iSliceIdx in 0..iSliceCount {
            let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurLayer, iSliceIdx);
            if !pSlice.is_null() {
                DeblockingFilterSliceAvcbase(pCurLayer, ctx_func_list(pEnc), pSlice);
            }
        }
    }
}

// ============================================================================
// Architecture and Dispatch Table Initialization
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsBlockFuncInit(
    pfSetNZCZero: *mut Option<PSetNoneZeroCountZeroFunc>,
    _iCpu: i32,
) {
    if !pfSetNZCZero.is_null() {
        *pfSetNZCZero = Some(WelsNonZeroCount_c);
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn DeblockingInit(pFunc: *mut DeblockingFunc, _iCpu: i32) {
    if pFunc.is_null() {
        return;
    }

    (*pFunc).pfLumaDeblockingLT4Ver = Some(DeblockLumaLt4V_c);
    (*pFunc).pfLumaDeblockingEQ4Ver = Some(DeblockLumaEq4V_c);
    (*pFunc).pfLumaDeblockingLT4Hor = Some(DeblockLumaLt4H_c);
    (*pFunc).pfLumaDeblockingEQ4Hor = Some(DeblockLumaEq4H_c);

    (*pFunc).pfChromaDeblockingLT4Ver = Some(DeblockChromaLt4V_c);
    (*pFunc).pfChromaDeblockingEQ4Ver = Some(DeblockChromaEq4V_c);
    (*pFunc).pfChromaDeblockingLT4Hor = Some(DeblockChromaLt4H_c);
    (*pFunc).pfChromaDeblockingEQ4Hor = Some(DeblockChromaEq4H_c);

    (*pFunc).pfDeblockingBSCalc = Some(DeblockingBSCalc_c);
    (*pFunc).pfDeblockingFilterSlice = Some(DeblockingFilterSliceAvcbase);
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

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_non_zero_count_c() {
        let mut nzc: [i8; 24] = [
            0, 5, 0, 12, -3, 0, 0, 1, 0, 0, 0, 4,
            0, 0, 0, 0, 2, 0, 0, 0, 0, 7, 0, 0,
        ];
        unsafe {
            WelsNonZeroCount_c(&mut nzc);
        }
        for (i, &val) in nzc.iter().enumerate() {
            if [1, 3, 4, 7, 11, 16, 21].contains(&i) {
                assert_eq!(val, 1);
            } else {
                assert_eq!(val, 0);
            }
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MSA, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSSE3};
