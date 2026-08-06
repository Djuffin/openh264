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

pub const MB_WIDTH_LUMA: usize = 16;
pub const MB_WIDTH_CHROMA: usize = 8;

pub const LEFT_MB_POS: i32 = 0x02;
pub const TOP_MB_POS: i32 = 0x01;

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
pub const WELS_CPU_SSE2: i32 = 0x00000004;
pub const WELS_CPU_SSSE3: i32 = 0x00000010;
pub const WELS_CPU_NEON: i32 = 0x00000008;
pub const WELS_CPU_MMI: i32 = 0x00000080;
pub const WELS_CPU_MSA: i32 = 0x00000100;
pub const WELS_CPU_LSX: i32 = 0x00000200;

// ============================================================================
// H.264 Deblocking Lookup Tables
// ============================================================================

/// Table 8-16 in H.264/AVC standard: Alpha table indexed by clipped QP + offset (0..51 + padding)
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

/// Active parameters and pointers for macroblock deblocking filtering.
/// Matches `struct TagDeblockingFilter` in `codec/encoder/core/inc/deblocking.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagDeblockingFilter {
    pub pCsData: [*mut u8; 3],     // Pointer to reconstructed picture data (Y, Cb, Cr)
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
            pCsData: [std::ptr::null_mut(); 3],
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

/// Macroblock context structure matching `TagMB` / `SMB` in `svc_enc_macroblock.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagMB {
    pub uiMbType: u32,
    pub uiSubMbType: [u8; 4],
    pub iMbXY: i32,
    pub iMbX: i16,
    pub iMbY: i16,
    pub uiNeighborAvail: u8,
    pub uiCbp: u8,
    pub sMv: *mut SMVUnitXY,
    pub pRefIndex: *mut i8,
    pub pSadCost: *mut i32,
    pub pIntra4x4PredMode: *mut i8,
    pub pNonZeroCount: *mut i8,
    pub sP16x16Mv: SMVUnitXY,
    pub uiLumaQp: u8,
    pub uiChromaQp: u8,
    pub uiSliceIdc: u16,
    pub uiChromPredMode: u32,
    pub iLumaDQp: i32,
    pub sMvd: [SMVUnitXY; 16],
    pub iCbpDc: i32,
}

pub use crate::encoder::svc_encode_slice::SMB;
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

pub type PDeblockingBSCalc = unsafe extern "C" fn(
    pFunc: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    uiBS: *mut [[u8; 4]; 4],
    uiCurMbType: u32,
    iMbStride: i32,
    iLeftFlag: i32,
    iTopFlag: i32,
);

pub type PDeblockingFilterSlice =
    unsafe extern "C" fn(pCurDq: *mut SDqLayer, pFunc: *mut SWelsFuncPtrList, pSlice: *mut SSlice);

pub type PSetNoneZeroCountZeroFunc = unsafe extern "C" fn(pNonZeroCount: *mut i8);

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
pub unsafe fn MB_BS_MV(
    sCurMv: *const SMVUnitXY,
    sNeighMv: *const SMVUnitXY,
    uiBIdx: usize,
    uiBnIdx: usize,
) -> u8 {
    let cur = *sCurMv.add(uiBIdx);
    let neigh = *sNeighMv.add(uiBnIdx);
    if (cur.iMvX as i32 - neigh.iMvX as i32).abs() >= 4
        || (cur.iMvY as i32 - neigh.iMvY as i32).abs() >= 4
    {
        1
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn SMB_EDGE_MV(
    sMotionVector: *const SMVUnitXY,
    uiBIdx: usize,
    uiBnIdx: usize,
) -> u8 {
    let cur = *sMotionVector.add(uiBIdx);
    let neigh = *sMotionVector.add(uiBnIdx);
    let dx = (cur.iMvX as i32 - neigh.iMvX as i32).abs();
    let dy = (cur.iMvY as i32 - neigh.iMvY as i32).abs();
    if ((dx & !3) | (dy & !3)) != 0 {
        1
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn BS_EDGE(
    bsx1: u8,
    sMotionVector: *const SMVUnitXY,
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
pub unsafe fn DeblockingBSInsideMBAvsbase(
    pNnzTab: *const i8,
    uiBS: *mut [[u8; 4]; 4],
    iLShiftFactor: i32,
) {
    let bs_ptr = uiBS as *mut [u8; 4];

    let n0 = *pNnzTab.add(0) as u8;
    let n1 = *pNnzTab.add(1) as u8;
    let n2 = *pNnzTab.add(2) as u8;
    let n3 = *pNnzTab.add(3) as u8;

    let n4 = *pNnzTab.add(4) as u8;
    let n5 = *pNnzTab.add(5) as u8;
    let n6 = *pNnzTab.add(6) as u8;
    let n7 = *pNnzTab.add(7) as u8;

    let n8 = *pNnzTab.add(8) as u8;
    let n9 = *pNnzTab.add(9) as u8;
    let n10 = *pNnzTab.add(10) as u8;
    let n11 = *pNnzTab.add(11) as u8;

    let n12 = *pNnzTab.add(12) as u8;
    let n13 = *pNnzTab.add(13) as u8;
    let n14 = *pNnzTab.add(14) as u8;
    let n15 = *pNnzTab.add(15) as u8;

    // Vertical internal edges (dir = 0)
    (*uiBS.add(0))[1][0] = (n0 | n1) << iLShiftFactor;
    (*uiBS.add(0))[2][0] = (n1 | n2) << iLShiftFactor;
    (*uiBS.add(0))[3][0] = (n2 | n3) << iLShiftFactor;

    (*uiBS.add(0))[1][1] = (n4 | n5) << iLShiftFactor;
    (*uiBS.add(0))[2][1] = (n5 | n6) << iLShiftFactor;
    (*uiBS.add(0))[3][1] = (n6 | n7) << iLShiftFactor;

    (*uiBS.add(0))[1][2] = (n8 | n9) << iLShiftFactor;
    (*uiBS.add(0))[2][2] = (n9 | n10) << iLShiftFactor;
    (*uiBS.add(0))[3][2] = (n10 | n11) << iLShiftFactor;

    (*uiBS.add(0))[1][3] = (n12 | n13) << iLShiftFactor;
    (*uiBS.add(0))[2][3] = (n13 | n14) << iLShiftFactor;
    (*uiBS.add(0))[3][3] = (n14 | n15) << iLShiftFactor;

    // Horizontal internal edges (dir = 1)
    for k in 0..4 {
        (*uiBS.add(1))[1][k] = (*pNnzTab.add(k) as u8 | *pNnzTab.add(4 + k) as u8) << iLShiftFactor;
        (*uiBS.add(1))[2][k] = (*pNnzTab.add(4 + k) as u8 | *pNnzTab.add(8 + k) as u8) << iLShiftFactor;
        (*uiBS.add(1))[3][k] = (*pNnzTab.add(8 + k) as u8 | *pNnzTab.add(12 + k) as u8) << iLShiftFactor;
    }
}

/// Computes internal boundary strength for normal partitioned Inter macroblocks.
#[inline(always)]
pub unsafe fn DeblockingBSInsideMBNormal(
    pCurMb: *mut SMB,
    uiBS: *mut [[u8; 4]; 4],
    pNnzTab: *const i8,
) {
    let sMv = (*pCurMb).sMv;

    // Vertical internal edges (dir = 0)
    for j in 0..4 {
        let base = j * 4;
        let bs0 = *pNnzTab.add(base) as u8 | *pNnzTab.add(base + 1) as u8;
        let bs1 = *pNnzTab.add(base + 1) as u8 | *pNnzTab.add(base + 2) as u8;
        let bs2 = *pNnzTab.add(base + 2) as u8 | *pNnzTab.add(base + 3) as u8;

        (*uiBS.add(0))[1][j] = BS_EDGE(bs0, sMv, base + 1, base);
        (*uiBS.add(0))[2][j] = BS_EDGE(bs1, sMv, base + 2, base + 1);
        (*uiBS.add(0))[3][j] = BS_EDGE(bs2, sMv, base + 3, base + 2);
    }

    // Horizontal internal edges (dir = 1)
    for k in 0..4 {
        let bs0 = *pNnzTab.add(k) as u8 | *pNnzTab.add(4 + k) as u8;
        let bs1 = *pNnzTab.add(4 + k) as u8 | *pNnzTab.add(8 + k) as u8;
        let bs2 = *pNnzTab.add(8 + k) as u8 | *pNnzTab.add(12 + k) as u8;

        (*uiBS.add(1))[1][k] = BS_EDGE(bs0, sMv, 4 + k, k);
        (*uiBS.add(1))[2][k] = BS_EDGE(bs1, sMv, 8 + k, 4 + k);
        (*uiBS.add(1))[3][k] = BS_EDGE(bs2, sMv, 12 + k, 8 + k);
    }
}

/// Computes marginal boundary strength vector for macroblock boundary edges (edge 0).
#[inline(always)]
pub unsafe fn DeblockingBSMarginalMBAvcbase(
    pCurMb: *mut SMB,
    pNeighMb: *mut SMB,
    iEdge: usize,
) -> u32 {
    let mut uiBSx4: [u8; 4] = [0; 4];
    let pBIdx = &g_kuiTableBIdx[iEdge][0..4];
    let pBnIdx = &g_kuiTableBIdx[iEdge][4..8];

    for i in 0..4 {
        let bIdx = pBIdx[i] as usize;
        let bnIdx = pBnIdx[i] as usize;
        let cur_nzc = *(*pCurMb).pNonZeroCount.add(bIdx);
        let neigh_nzc = *(*pNeighMb).pNonZeroCount.add(bnIdx);

        if (cur_nzc | neigh_nzc) != 0 {
            uiBSx4[i] = 2;
        } else {
            uiBSx4[i] = MB_BS_MV((*pCurMb).sMv, (*pNeighMb).sMv, bIdx, bnIdx);
        }
    }

    u32::from_ne_bytes(uiBSx4)
}

/// Reference C implementation of Boundary Strength ($bS$) calculation.
pub unsafe extern "C" fn DeblockingBSCalc_c(
    pFunc: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    uiBS: *mut [[u8; 4]; 4],
    uiCurMbType: u32,
    iMbStride: i32,
    iLeftFlag: i32,
    iTopFlag: i32,
) {
    if iLeftFlag != 0 {
        let leftMb = pCurMb.offset(-1);
        let val = if IS_INTRA((*leftMb).uiMbType) {
            0x04040404u32
        } else {
            DeblockingBSMarginalMBAvcbase(pCurMb, leftMb, 0)
        };
        let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
        slice[0..4].copy_from_slice(&val.to_ne_bytes());
    } else {
        let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
        slice[0..4].fill(0);
    }

    if iTopFlag != 0 {
        let topMb = pCurMb.offset(-(iMbStride as isize));
        let val = if IS_INTRA((*topMb).uiMbType) {
            0x04040404u32
        } else {
            DeblockingBSMarginalMBAvcbase(pCurMb, topMb, 1)
        };
        let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
        slice[16..20].copy_from_slice(&val.to_ne_bytes());
    } else {
        let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
        slice[16..20].fill(0);
    }

    if uiCurMbType != MB_TYPE_SKIP {
        if !pFunc.is_null() {
            if let Some(set_nzc) = (*pFunc).pfSetNZCZero {
                // deblocking.cpp:615 — one argument.
                set_nzc((*pCurMb).pNonZeroCount);
            }
        }
        if uiCurMbType == MB_TYPE_16x16 {
            DeblockingBSInsideMBAvsbase((*pCurMb).pNonZeroCount, uiBS, 1);
        } else {
            DeblockingBSInsideMBNormal(pCurMb, uiBS, (*pCurMb).pNonZeroCount);
        }
    } else {
        let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
        slice[4..16].fill(0);
        slice[20..32].fill(0);
    }
}

// ============================================================================
// C Reference Fallback Edge Filtering Implementations
// ============================================================================

pub unsafe extern "C" fn DeblockLumaLt4_c(
    mut pPix: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    let sx = iStrideX as isize;
    let sy = iStrideY as isize;
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
                    let diff = (p2 + ((p0 + q0 + 1) >> 1) - (p1 * 2)) >> 1;
                    *pPix.offset(-2 * sx) = (p1 + WELS_CLIP3(diff, -iTc0, iTc0)) as u8;
                    iTc += 1;
                }
                if bDetaQ2Q0 {
                    let diff = (q2 + ((p0 + q0 + 1) >> 1) - (q1 * 2)) >> 1;
                    *pPix.offset(sx) = (q1 + WELS_CLIP3(diff, -iTc0, iTc0)) as u8;
                    iTc += 1;
                }
                let iDeta = WELS_CLIP3((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3, -iTc, iTc);
                *pPix.offset(-sx) = WelsClip1(p0 + iDeta);
                *pPix = WelsClip1(q0 - iDeta);
            }
        }
        pPix = pPix.offset(sy);
    }
}

pub unsafe extern "C" fn DeblockLumaEq4_c(
    mut pPix: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    let sx = iStrideX as isize;
    let sy = iStrideY as isize;
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
                    *pPix.offset(-3 * sx) =
                        (((p3 * 2) + p2 + (p2 * 2) + p1 + p0 + q0 + 4) >> 3) as u8;
                } else {
                    *pPix.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
                }
                if bDetaQ2Q0 {
                    let q3 = *pPix.offset(3 * sx) as i32;
                    *pPix = ((p1 + (p0 * 2) + (q0 * 2) + (q1 * 2) + q2 + 4) >> 3) as u8;
                    *pPix.offset(sx) = ((p0 + q0 + q1 + q2 + 2) >> 2) as u8;
                    *pPix.offset(2 * sx) =
                        (((q3 * 2) + q2 + (q2 * 2) + q1 + q0 + p0 + 4) >> 3) as u8;
                } else {
                    *pPix = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
                }
            } else {
                *pPix.offset(-sx) = (((p1 * 2) + p0 + q1 + 2) >> 2) as u8;
                *pPix = (((q1 * 2) + q0 + p1 + 2) >> 2) as u8;
            }
        }
        pPix = pPix.offset(sy);
    }
}

pub unsafe extern "C" fn DeblockLumaLt4V_c(
    pPix: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    tc: *mut i8,
) {
    DeblockLumaLt4_c(pPix, iStride, 1, iAlpha, iBeta, tc);
}

pub unsafe extern "C" fn DeblockLumaLt4H_c(
    pPix: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    tc: *mut i8,
) {
    DeblockLumaLt4_c(pPix, 1, iStride, iAlpha, iBeta, tc);
}

pub unsafe extern "C" fn DeblockLumaEq4V_c(
    pPix: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockLumaEq4_c(pPix, iStride, 1, iAlpha, iBeta);
}

pub unsafe extern "C" fn DeblockLumaEq4H_c(
    pPix: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockLumaEq4_c(pPix, 1, iStride, iAlpha, iBeta);
}

pub unsafe extern "C" fn DeblockChromaLt4_c(
    mut pPixCb: *mut u8,
    mut pPixCr: *mut u8,
    iStrideX: i32,
    iStrideY: i32,
    iAlpha: i32,
    iBeta: i32,
    pTc: *mut i8,
) {
    let sx = iStrideX as isize;
    let sy = iStrideY as isize;
    for i in 0..8 {
        let iTc0 = *pTc.add(i >> 1) as i32;
        if iTc0 > 0 {
            // Cb Plane
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

            // Cr Plane
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

pub unsafe extern "C" fn DeblockChromaEq4_c(
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
        // Cb Plane
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

        // Cr Plane
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

pub unsafe extern "C" fn DeblockChromaLt4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    tc: *mut i8,
) {
    DeblockChromaLt4_c(pPixCb, pPixCr, iStride, 1, iAlpha, iBeta, tc);
}

pub unsafe extern "C" fn DeblockChromaLt4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
    tc: *mut i8,
) {
    DeblockChromaLt4_c(pPixCb, pPixCr, 1, iStride, iAlpha, iBeta, tc);
}

pub unsafe extern "C" fn DeblockChromaEq4V_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockChromaEq4_c(pPixCb, pPixCr, iStride, 1, iAlpha, iBeta);
}

pub unsafe extern "C" fn DeblockChromaEq4H_c(
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    iAlpha: i32,
    iBeta: i32,
) {
    DeblockChromaEq4_c(pPixCb, pPixCr, 1, iStride, iAlpha, iBeta);
}

pub unsafe extern "C" fn WelsNonZeroCount_c(pNonZeroCount: *mut i8) {
    for i in 0..24 {
        let val = *pNonZeroCount.add(i);
        *pNonZeroCount.add(i) = if val != 0 { 1 } else { 0 };
    }
}

// ============================================================================
// Directional Filtering Dispatchers
// ============================================================================

pub unsafe fn FilteringEdgeLumaH(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut iTc: [i8; 4] = [0; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        let bs_slice = std::slice::from_raw_parts(pBS, 4);
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, bs_slice, 0);
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingLT4Ver {
            func(pPix, iStride, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }
}

pub unsafe fn FilteringEdgeLumaV(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut iTc: [i8; 4] = [0; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        let bs_slice = std::slice::from_raw_parts(pBS, 4);
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, bs_slice, 0);
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingLT4Hor {
            func(pPix, iStride, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }
}

pub unsafe fn FilteringEdgeLumaIntraH(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    _pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingEQ4Ver {
            func(pPix, iStride, iAlpha, iBeta);
        }
    }
}

pub unsafe fn FilteringEdgeLumaIntraV(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    _pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingEQ4Hor {
            func(pPix, iStride, iAlpha, iBeta);
        }
    }
}

pub unsafe fn FilteringEdgeChromaH(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut iTc: [i8; 4] = [0; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiChromaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        let bs_slice = std::slice::from_raw_parts(pBS, 4);
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, bs_slice, 1);
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingLT4Ver {
            func(pPixCb, pPixCr, iStride, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }
}

pub unsafe fn FilteringEdgeChromaV(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut iTc: [i8; 4] = [0; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiChromaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        let bs_slice = std::slice::from_raw_parts(pBS, 4);
        TC0_TBL_LOOKUP(&mut iTc, iIdexA, bs_slice, 1);
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingLT4Hor {
            func(pPixCb, pPixCr, iStride, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }
}

pub unsafe fn FilteringEdgeChromaIntraH(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    _pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiChromaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingEQ4Ver {
            func(pPixCb, pPixCr, iStride, iAlpha, iBeta);
        }
    }
}

pub unsafe fn FilteringEdgeChromaIntraV(
    pfDeblocking: *const DeblockingFunc,
    pFilter: *mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    _pBS: *const u8,
) {
    let mut iIdexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).uiChromaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIdexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingEQ4Hor {
            func(pPixCb, pPixCr, iStride, iAlpha, iBeta);
        }
    }
}

// ============================================================================
// Macroblock Deblocking Execution
// ============================================================================

pub unsafe fn DeblockingInterMb(
    pfDeblocking: *const DeblockingFunc,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
    uiBS: *const [[u8; 4]; 4],
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

    let pDestY = (*pFilter).pCsData[0];
    let pDestCb = (*pFilter).pCsData[1];
    let pDestCr = (*pFilter).pCsData[2];

    let bs_slice = std::slice::from_raw_parts(uiBS as *const u8, 32);

    if iLeftFlag {
        (*pFilter).uiLumaQP =
            ((iCurLumaQp as i32 + (*pCurMb.offset(-1)).uiLumaQp as i32 + 1) >> 1) as u8;
        (*pFilter).uiChromaQP =
            ((iCurChromaQp as i32 + (*pCurMb.offset(-1)).uiChromaQp as i32 + 1) >> 1) as u8;

        if bs_slice[0] == 0x04 {
            FilteringEdgeLumaIntraV(pfDeblocking, pFilter, pDestY, iLineSize, std::ptr::null());
            FilteringEdgeChromaIntraV(
                pfDeblocking,
                pFilter,
                pDestCb,
                pDestCr,
                iLineSizeUV,
                std::ptr::null(),
            );
        } else {
            let bs00_u32 = u32::from_ne_bytes(bs_slice[0..4].try_into().unwrap());
            if bs00_u32 != 0 {
                FilteringEdgeLumaV(pfDeblocking, pFilter, pDestY, iLineSize, &bs_slice[0]);
                FilteringEdgeChromaV(
                    pfDeblocking,
                    pFilter,
                    pDestCb,
                    pDestCr,
                    iLineSizeUV,
                    &bs_slice[0],
                );
            }
        }
    }

    (*pFilter).uiLumaQP = iCurLumaQp as u8;
    (*pFilter).uiChromaQP = iCurChromaQp as u8;

    let bs01_u32 = u32::from_ne_bytes(bs_slice[4..8].try_into().unwrap());
    if bs01_u32 != 0 {
        FilteringEdgeLumaV(
            pfDeblocking,
            pFilter,
            pDestY.add(1 << 2),
            iLineSize,
            &bs_slice[4],
        );
    }

    let bs02_u32 = u32::from_ne_bytes(bs_slice[8..12].try_into().unwrap());
    if bs02_u32 != 0 {
        FilteringEdgeLumaV(
            pfDeblocking,
            pFilter,
            pDestY.add(2 << 2),
            iLineSize,
            &bs_slice[8],
        );
        FilteringEdgeChromaV(
            pfDeblocking,
            pFilter,
            pDestCb.add(2 << 1),
            pDestCr.add(2 << 1),
            iLineSizeUV,
            &bs_slice[8],
        );
    }

    let bs03_u32 = u32::from_ne_bytes(bs_slice[12..16].try_into().unwrap());
    if bs03_u32 != 0 {
        FilteringEdgeLumaV(
            pfDeblocking,
            pFilter,
            pDestY.add(3 << 2),
            iLineSize,
            &bs_slice[12],
        );
    }

    if iTopFlag {
        (*pFilter).uiLumaQP =
            ((iCurLumaQp as i32 + (*pCurMb.offset(-iMbStride)).uiLumaQp as i32 + 1) >> 1) as u8;
        (*pFilter).uiChromaQP =
            ((iCurChromaQp as i32 + (*pCurMb.offset(-iMbStride)).uiChromaQp as i32 + 1) >> 1) as u8;

        if bs_slice[16] == 0x04 {
            FilteringEdgeLumaIntraH(pfDeblocking, pFilter, pDestY, iLineSize, std::ptr::null());
            FilteringEdgeChromaIntraH(
                pfDeblocking,
                pFilter,
                pDestCb,
                pDestCr,
                iLineSizeUV,
                std::ptr::null(),
            );
        } else {
            let bs10_u32 = u32::from_ne_bytes(bs_slice[16..20].try_into().unwrap());
            if bs10_u32 != 0 {
                FilteringEdgeLumaH(pfDeblocking, pFilter, pDestY, iLineSize, &bs_slice[16]);
                FilteringEdgeChromaH(
                    pfDeblocking,
                    pFilter,
                    pDestCb,
                    pDestCr,
                    iLineSizeUV,
                    &bs_slice[16],
                );
            }
        }
    }

    (*pFilter).uiLumaQP = iCurLumaQp as u8;
    (*pFilter).uiChromaQP = iCurChromaQp as u8;

    let bs11_u32 = u32::from_ne_bytes(bs_slice[20..24].try_into().unwrap());
    if bs11_u32 != 0 {
        FilteringEdgeLumaH(
            pfDeblocking,
            pFilter,
            pDestY.add((1 << 2) * iLineSize as usize),
            iLineSize,
            &bs_slice[20],
        );
    }

    let bs12_u32 = u32::from_ne_bytes(bs_slice[24..28].try_into().unwrap());
    if bs12_u32 != 0 {
        FilteringEdgeLumaH(
            pfDeblocking,
            pFilter,
            pDestY.add((2 << 2) * iLineSize as usize),
            iLineSize,
            &bs_slice[24],
        );
        FilteringEdgeChromaH(
            pfDeblocking,
            pFilter,
            pDestCb.add((2 << 1) * iLineSizeUV as usize),
            pDestCr.add((2 << 1) * iLineSizeUV as usize),
            iLineSizeUV,
            &bs_slice[24],
        );
    }

    let bs13_u32 = u32::from_ne_bytes(bs_slice[28..32].try_into().unwrap());
    if bs13_u32 != 0 {
        FilteringEdgeLumaH(
            pfDeblocking,
            pFilter,
            pDestY.add((3 << 2) * iLineSize as usize),
            iLineSize,
            &bs_slice[28],
        );
    }
}

pub unsafe fn FilteringEdgeLumaHV(
    pfDeblocking: *const DeblockingFunc,
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

    let pDestY = (*pFilter).pCsData[0];
    let iCurQp = (*pCurMb).uiLumaQp as i8;

    // Luma vertical edges
    if iLeftFlag {
        (*pFilter).uiLumaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-1)).uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraV(pfDeblocking, pFilter, pDestY, iLineSize, std::ptr::null());
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
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingLT4Hor {
            func(pDestY.add(1 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
            func(pDestY.add(2 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
            func(pDestY.add(3 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }

    // Luma horizontal edges
    if iTopFlag {
        (*pFilter).uiLumaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-iMbStride)).uiLumaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeLumaIntraH(pfDeblocking, pFilter, pDestY, iLineSize, std::ptr::null());
    }

    (*pFilter).uiLumaQP = iCurQp as u8;
    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfLumaDeblockingLT4Ver {
            func(
                pDestY.add((1 << 2) * iLineSize as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
            func(
                pDestY.add((2 << 2) * iLineSize as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
            func(
                pDestY.add((3 << 2) * iLineSize as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    }
}

pub unsafe fn FilteringEdgeChromaHV(
    pfDeblocking: *const DeblockingFunc,
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

    let pDestCb = (*pFilter).pCsData[1];
    let pDestCr = (*pFilter).pCsData[2];
    let iCurQp = (*pCurMb).uiChromaQp as i8;

    // Chroma vertical edges
    if iLeftFlag {
        (*pFilter).uiChromaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-1)).uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraV(
            pfDeblocking,
            pFilter,
            pDestCb,
            pDestCr,
            iLineSize,
            std::ptr::null(),
        );
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
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingLT4Hor {
            func(
                pDestCb.add(2 << 1),
                pDestCr.add(2 << 1),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    }

    // Chroma horizontal edges
    if iTopFlag {
        (*pFilter).uiChromaQP =
            ((iCurQp as i32 + (*pCurMb.offset(-iMbStride)).uiChromaQp as i32 + 1) >> 1) as u8;
        FilteringEdgeChromaIntraH(
            pfDeblocking,
            pFilter,
            pDestCb,
            pDestCr,
            iLineSize,
            std::ptr::null(),
        );
    }

    (*pFilter).uiChromaQP = iCurQp as u8;
    if (iAlpha | iBeta) != 0 {
        if let Some(func) = (*pfDeblocking).pfChromaDeblockingLT4Ver {
            func(
                pDestCb.add((2 << 1) * iLineSize as usize),
                pDestCr.add((2 << 1) * iLineSize as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    }
}

#[inline(always)]
pub unsafe fn DeblockingIntraMb(
    pfDeblocking: *const DeblockingFunc,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    FilteringEdgeLumaHV(pfDeblocking, pCurMb, pFilter);
    FilteringEdgeChromaHV(pfDeblocking, pCurMb, pFilter);
}

pub unsafe fn DeblockingMbAvcbase(
    pFunc: *mut SWelsFuncPtrList,
    pCurMb: *mut SMB,
    pFilter: *mut SDeblockingFilter,
) {
    let mut uiBS: [[u8; 4]; 4] = [[0; 4]; 4];
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
            DeblockingIntraMb(pfDeblocking, pCurMb, pFilter);
        }
        _ => {
            if let Some(bs_calc) = (*pfDeblocking).pfDeblockingBSCalc {
                bs_calc(
                    pFunc,
                    pCurMb,
                    &mut uiBS as *mut [[u8; 4]; 4],
                    uiCurMbType,
                    iMbStride as i32,
                    iLeftFlag,
                    iTopFlag,
                );
            }
            DeblockingInterMb(pfDeblocking, pCurMb, pFilter, &uiBS as *const [[u8; 4]; 4]);
        }
    }
}

// ============================================================================
// Frame and Slice Level Traversal
// ============================================================================

pub unsafe fn DeblockingFilterFrameAvcbase(pCurDq: *mut SDqLayer, pFunc: *mut SWelsFuncPtrList) {
    if pCurDq.is_null() || (*pCurDq).ppSliceInLayer.is_null() || (*(*pCurDq).ppSliceInLayer).is_null() || (*pCurDq).sMbDataP.is_null() || (*pCurDq).pDecPic.is_null() {
        return;
    }
    let kiMbWidth = (*pCurDq).iMbWidth;
    let kiMbHeight = (*pCurDq).iMbHeight;
    let mut pCurrentMbBlock = (*pCurDq).sMbDataP;

    let pSlice = *(*pCurDq).ppSliceInLayer;
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

    let pDecPic = (*pCurDq).pDecPic;
    pFilter.iCsStride[0] = (*pDecPic).iLineSize[0];
    pFilter.iCsStride[1] = (*pDecPic).iLineSize[1];
    pFilter.iCsStride[2] = (*pDecPic).iLineSize[2];

    pFilter.iSliceAlphaC0Offset = sSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = sSliceHeaderExt.sSliceHeader.iSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth as i16;

    for j in 0..kiMbHeight {
        pFilter.pCsData[0] = (*pDecPic).pData[0].add(((j as i32 * pFilter.iCsStride[0]) << 4) as usize);
        pFilter.pCsData[1] = (*pDecPic).pData[1].add(((j as i32 * pFilter.iCsStride[1]) << 3) as usize);
        pFilter.pCsData[2] = (*pDecPic).pData[2].add(((j as i32 * pFilter.iCsStride[2]) << 3) as usize);

        for _ in 0..kiMbWidth {
            DeblockingMbAvcbase(pFunc, pCurrentMbBlock, &mut pFilter);
            pCurrentMbBlock = pCurrentMbBlock.add(1);
            pFilter.pCsData[0] = pFilter.pCsData[0].add(MB_WIDTH_LUMA);
            pFilter.pCsData[1] = pFilter.pCsData[1].add(MB_WIDTH_CHROMA);
            pFilter.pCsData[2] = pFilter.pCsData[2].add(MB_WIDTH_CHROMA);
        }
    }
}

pub unsafe fn GetCurrentSliceNum(pCurDq: *const SDqLayer) -> i32 {
    if pCurDq.is_null() {
        return 0;
    }
    1
}

pub unsafe fn WelsGetNextMbOfSlice(pCurDq: *mut SDqLayer, kiMbXY: i32) -> i32 {
    if pCurDq.is_null() {
        return -1;
    }
    let total: i32 = (*pCurDq).iMbWidth as i32 * (*pCurDq).iMbHeight as i32;
    let next = kiMbXY + 1;
    if next >= total {
        -1
    } else {
        next
    }
}

pub unsafe extern "C" fn DeblockingFilterSliceAvcbase(
    pCurDq: *mut SDqLayer,
    pFunc: *mut SWelsFuncPtrList,
    pSlice: *mut SSlice,
) {
    let pMbList = (*pCurDq).sMbDataP;
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

    let pDecPic = (*pCurDq).pDecPic;
    pFilter.iCsStride[0] = (*pDecPic).iLineSize[0];
    pFilter.iCsStride[1] = (*pDecPic).iLineSize[1];
    pFilter.iCsStride[2] = (*pDecPic).iLineSize[2];

    pFilter.iSliceAlphaC0Offset = sSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset;
    pFilter.iSliceBetaOffset = sSliceHeaderExt.sSliceHeader.iSliceBetaOffset;
    pFilter.iMbStride = kiMbWidth as i16;

    let mut iNextMbIdx = sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

    loop {
        let iCurMbIdx = iNextMbIdx;
        let pCurrentMbBlock = pMbList.add(iCurMbIdx as usize);

        let mbX = (*pCurrentMbBlock).iMbX as i32;
        let mbY = (*pCurrentMbBlock).iMbY as i32;

        pFilter.pCsData[0] = (*pDecPic).pData[0].add(((mbX + mbY * pFilter.iCsStride[0]) << 4) as usize);
        pFilter.pCsData[1] = (*pDecPic).pData[1].add(((mbX + mbY * pFilter.iCsStride[1]) << 3) as usize);
        pFilter.pCsData[2] = (*pDecPic).pData[2].add(((mbX + mbY * pFilter.iCsStride[2]) << 3) as usize);

        DeblockingMbAvcbase(pFunc, pCurrentMbBlock, &mut pFilter);

        iNumMbFiltered += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurDq, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbFiltered >= kiTotalNumMb {
            break;
        }
    }
}

pub unsafe extern "C" fn DeblockingFilterSliceAvcbaseNull(
    _pCurDq: *mut SDqLayer,
    _pFunc: *mut SWelsFuncPtrList,
    _pSlice: *mut SSlice,
) {
}

pub unsafe extern "C" fn PerformDeblockingFilter(pEnc: *mut sWelsEncCtx) {
    if pEnc.is_null() {
        return;
    }
    let pCurLayer = (*pEnc).pCurDqLayer;
    if pCurLayer.is_null() {
        return;
    }

    if (*pCurLayer).iLoopFilterDisableIdc == 0 {
        DeblockingFilterFrameAvcbase(pCurLayer, (*pEnc).pFuncList);
    } else if (*pCurLayer).iLoopFilterDisableIdc == 2 {
        let iSliceCount = GetCurrentSliceNum(pCurLayer);
        for iSliceIdx in 0..iSliceCount {
            let pSlice = *(*pCurLayer).ppSliceInLayer.add(iSliceIdx as usize);
            if !pSlice.is_null() {
                DeblockingFilterSliceAvcbase(pCurLayer, (*pEnc).pFuncList, pSlice);
            }
        }
    }
}

// ============================================================================
// Architecture and Dispatch Table Initialization
// ============================================================================

pub unsafe extern "C" fn WelsBlockFuncInit(
    pfSetNZCZero: *mut Option<PSetNoneZeroCountZeroFunc>,
    _iCpu: i32,
) {
    if !pfSetNZCZero.is_null() {
        *pfSetNZCZero = Some(WelsNonZeroCount_c);
    }
}

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
    fn test_non_zero_count_c() {
        let mut nzc: [i8; 24] = [
            0, 5, 0, 12, -3, 0, 0, 1, 0, 0, 0, 4,
            0, 0, 0, 0, 2, 0, 0, 0, 0, 7, 0, 0,
        ];
        unsafe {
            WelsNonZeroCount_c(nzc.as_mut_ptr());
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
