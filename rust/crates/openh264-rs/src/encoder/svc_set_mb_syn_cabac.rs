// Copyright (c) 2009-2014, Cisco Systems
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

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut
)]

//! Context-based Adaptive Binary Arithmetic Coding (CABAC) Macroblock Syntax Writer.
//!
//! Translated from `codec/encoder/core/src/svc_set_mb_syn_cabac.cpp`,
//! `codec/encoder/core/inc/svc_set_mb_syn.h`, and `codec/encoder/core/inc/set_mb_syn_cabac.h`.

use std::ffi::c_void;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SMVComponentUnit;

// ============================================================================
// Constants & Configuration Limits
// ============================================================================

pub const WELS_CONTEXT_COUNT: usize = 460;
pub const WELS_QP_MAX: i32 = 51;

pub const LEFT_MB_POS: u8 = 0x01;
pub const TOP_MB_POS: u8 = 0x02;
pub const TOPRIGHT_MB_POS: u8 = 0x04;
pub const TOPLEFT_MB_POS: u8 = 0x08;

pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_LUMA_CHROMA_BLOCK4x4_NUM: usize = 24;

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

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

#[inline(always)]
pub fn IS_INTRA(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA) != 0
}

#[inline(always)]
pub fn IS_INTRA4x4(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA4x4
}

#[inline(always)]
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn CLIP3_QP_0_51(qp: i32) -> usize {
    qp.clamp(0, 51) as usize
}

// ============================================================================
// Block Category Enumeration
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ECtxBlockCat {
    LUMA_DC = 0,
    LUMA_AC = 1,
    LUMA_4x4 = 2,
    CHROMA_DC = 3,
    CHROMA_AC = 4,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
    UNKNOWN_SLICE = 5,
}

// ============================================================================
// Context Offset Tables
// ============================================================================

pub const uiSignificantCoeffFlagOffset: [u16; 5] = [0, 15, 29, 44, 47];
pub const uiLastCoeffFlagOffset: [u16; 5] = [0, 15, 29, 44, 47];
pub const uiCoeffAbsLevelMinus1Offset: [u16; 5] = [0, 10, 20, 30, 39];
pub const uiCodecBlockFlagOffset: [u16; 5] = [0, 4, 8, 12, 16];

pub const g_kiClz5Table: [i8; 32] = [
    6, 5, 4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

pub const g_kiMapModeI16x16: [i8; 7] = [0, 1, 2, 3, 2, 2, 2];
pub const g_kiMapModeIntraChroma: [i8; 7] = [0, 1, 2, 3, 0, 0, 0];

pub const g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5,
    2, 3, 6, 7,
    8, 9, 12, 13,
    10, 11, 14, 15,
    16, 17, 20, 21,
    18, 19, 22, 23,
];

pub const g_kuiCache48CountScan4Idx: [u8; 24] = [
    9, 10, 17, 18,
    11, 12, 19, 20,
    25, 26, 33, 34,
    27, 28, 35, 36,
    14, 15,
    22, 23,
    38, 39,
    46, 47,
];

pub const g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37,
    37, 38, 38, 38, 39, 39, 39, 39,
];

pub const g_kuiCabacRangeLps: [[u8; 4]; 64] = [
    [128, 176, 208, 240], [128, 167, 197, 227], [128, 158, 187, 216], [123, 150, 178, 205],
    [116, 142, 169, 195], [111, 135, 160, 185], [105, 128, 152, 175], [100, 122, 144, 166],
    [95, 116, 137, 158],  [90, 110, 130, 150],  [85, 104, 123, 142],  [81, 99, 117, 135],
    [77, 94, 111, 128],   [73, 89, 105, 122],   [69, 85, 100, 116],   [66, 80, 95, 110],
    [62, 76, 90, 104],    [59, 72, 86, 99],     [56, 69, 81, 94],     [53, 65, 77, 89],
    [51, 62, 73, 85],     [48, 59, 69, 80],     [46, 56, 66, 76],     [43, 53, 63, 72],
    [41, 50, 59, 69],     [39, 48, 56, 65],     [37, 45, 54, 62],     [35, 43, 51, 59],
    [33, 41, 48, 56],     [32, 39, 46, 53],     [30, 37, 43, 50],     [29, 35, 41, 48],
    [27, 33, 39, 45],     [26, 31, 37, 43],     [24, 30, 35, 41],     [23, 28, 33, 39],
    [22, 27, 32, 37],     [21, 26, 30, 35],     [20, 24, 29, 33],     [19, 23, 27, 31],
    [18, 22, 26, 30],     [17, 21, 25, 28],     [16, 20, 23, 27],     [15, 19, 22, 25],
    [14, 18, 21, 24],     [14, 17, 20, 23],     [13, 16, 19, 22],     [12, 15, 18, 21],
    [12, 14, 17, 20],     [11, 14, 16, 19],     [11, 13, 15, 18],     [10, 12, 15, 17],
    [10, 12, 14, 16],     [9, 11, 13, 15],      [9, 11, 12, 14],      [8, 10, 12, 14],
    [8, 9, 11, 13],       [7, 9, 11, 12],       [7, 9, 10, 12],       [7, 8, 10, 11],
    [6, 8, 9, 11],        [6, 7, 9, 10],        [6, 7, 8, 9],         [2, 2, 2, 2],
];

pub const g_kuiStateTransTable: [[u8; 2]; 64] = [
    [0, 1],   [0, 2],   [1, 3],   [2, 4],   [2, 5],   [4, 6],   [4, 7],   [5, 8],
    [6, 9],   [7, 10],  [8, 11],  [9, 12],  [9, 13],  [11, 14], [11, 15], [12, 16],
    [13, 17], [13, 18], [15, 19], [15, 20], [16, 21], [16, 22], [18, 23], [18, 24],
    [19, 25], [19, 26], [21, 27], [21, 28], [22, 29], [22, 30], [23, 31], [24, 32],
    [24, 33], [25, 34], [26, 35], [26, 36], [27, 37], [27, 38], [28, 39], [29, 40],
    [29, 41], [30, 42], [30, 43], [30, 44], [31, 45], [32, 46], [32, 47], [33, 48],
    [33, 49], [33, 50], [34, 51], [34, 52], [35, 53], [35, 54], [35, 55], [36, 56],
    [36, 57], [36, 58], [37, 59], [37, 60], [37, 61], [38, 62], [38, 62], [63, 63],
];

// ============================================================================
// Core CABAC & Encoder Data Structures
// ============================================================================

pub type cabac_low_t = u64;
pub const CABAC_LOW_WIDTH: usize = 64;

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SStateCtx {
    pub m_uiStateMps: u8,
}

impl SStateCtx {
    #[inline(always)]
    pub fn Mps(&self) -> u8 {
        self.m_uiStateMps & 1
    }

    #[inline(always)]
    pub fn State(&self) -> u8 {
        self.m_uiStateMps >> 1
    }

    #[inline(always)]
    pub fn Set(&mut self, uiState: u8, uiMps: u8) {
        self.m_uiStateMps = (uiState << 1) | (uiMps & 1);
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SCabacCtx {
    pub m_uiLow: cabac_low_t,
    pub m_iLowBitCnt: i32,
    pub m_iRenormCnt: i32,
    pub m_uiRange: u32,
    pub m_sStateCtx: [SStateCtx; WELS_CONTEXT_COUNT],
    pub m_pBufStart: *mut u8,
    pub m_pBufEnd: *mut u8,
    pub m_pBufCur: *mut u8,
}

impl Default for SCabacCtx {
    fn default() -> Self {
        Self {
            m_uiLow: 0,
            m_iLowBitCnt: 0,
            m_iRenormCnt: 0,
            m_uiRange: 0,
            m_sStateCtx: [SStateCtx::default(); WELS_CONTEXT_COUNT],
            m_pBufStart: std::ptr::null_mut(),
            m_pBufEnd: std::ptr::null_mut(),
            m_pBufCur: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SMbCache {
    pub sMvComponents: SMVComponentUnit,
    pub iNonZeroCoeffCount: [i8; 48],
    pub iIntraPredMode: [i8; 48],
    pub sMbMvp: [SMVUnitXY; 16],
    pub pPrevIntra4x4PredModeFlag: *mut bool,
    pub pRemIntra4x4PredModeFlag: *mut i8,
    pub bMbTypeSkip: [bool; 4],
    pub pDct: *mut SDCTCoeff,
    pub uiLumaI16x16Mode: u8,
    pub uiChmaI8x8Mode: u8,
}

impl Default for SMbCache {
    fn default() -> Self {
        Self {
            sMvComponents: SMVComponentUnit::default(),
            iNonZeroCoeffCount: [0; 48],
            iIntraPredMode: [0; 48],
            sMbMvp: [SMVUnitXY::default(); 16],
            pPrevIntra4x4PredModeFlag: std::ptr::null_mut(),
            pRemIntra4x4PredModeFlag: std::ptr::null_mut(),
            bMbTypeSkip: [false; 4],
            pDct: std::ptr::null_mut(),
            uiLumaI16x16Mode: 0,
            uiChmaI8x8Mode: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SMB {
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

impl Default for SMB {
    fn default() -> Self {
        Self {
            uiMbType: 0,
            uiSubMbType: [0; 4],
            iMbXY: 0,
            iMbX: 0,
            iMbY: 0,
            uiNeighborAvail: 0,
            uiCbp: 0,
            sMv: std::ptr::null_mut(),
            pRefIndex: std::ptr::null_mut(),
            pSadCost: std::ptr::null_mut(),
            pIntra4x4PredMode: std::ptr::null_mut(),
            pNonZeroCount: std::ptr::null_mut(),
            sP16x16Mv: SMVUnitXY::default(),
            uiLumaQp: 0,
            uiChromaQp: 0,
            uiSliceIdc: 0,
            uiChromPredMode: 0,
            iLumaDQp: 0,
            sMvd: [SMVUnitXY::default(); 16],
            iCbpDc: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeader {
    pub iFirstMbInSlice: i32,
    pub iFrameNum: i32,
    pub iPicOrderCntLsb: i32,
    pub eSliceType: EWelsSliceType,
    pub uiNumRefIdxL0Active: u8,
    pub uiRefCount: u8,
    pub uiRefIndex: u8,
    pub iSliceQpDelta: i8,
    pub uiDisableDeblockingFilterIdc: u8,
    pub iSliceAlphaC0Offset: i8,
    pub iSliceBetaOffset: i8,
}

impl Default for SSliceHeader {
    fn default() -> Self {
        Self {
            iFirstMbInSlice: 0,
            iFrameNum: 0,
            iPicOrderCntLsb: 0,
            eSliceType: EWelsSliceType::P_SLICE,
            uiNumRefIdxL0Active: 0,
            uiRefCount: 0,
            uiRefIndex: 0,
            iSliceQpDelta: 0,
            uiDisableDeblockingFilterIdc: 0,
            iSliceAlphaC0Offset: 0,
            iSliceBetaOffset: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
}

pub use crate::common::wels_common_defs::SBitStringAux;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
    pub pSliceBsa: *mut SBitStringAux,
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub uiLastMbQp: u8,
    pub sCabacCtx: SCabacCtx,
    pub iCabacInitIdc: i32,
}

impl Default for SSlice {
    fn default() -> Self {
        Self {
            sMbCacheInfo: SMbCache::default(),
            pSliceBsa: std::ptr::null_mut(),
            sSliceHeaderExt: SSliceHeaderExt::default(),
            uiLastMbQp: 0,
            sCabacCtx: SCabacCtx::default(),
            iCabacInitIdc: 0,
        }
    }
}

// Function pointer list matching OpenH264
#[repr(C)]
pub struct SWelsFuncPtrList {
    pub pfGetNoneZeroCount: Option<unsafe extern "C" fn(*mut i16) -> i32>,
}

#[repr(C)]
pub struct SWelsPps {
    pub uiChromaQpIndexOffset: u32,
}

#[repr(C)]
pub struct SLayerInfo {
    pub pPpsP: *mut SWelsPps,
}

#[repr(C)]
pub struct SDqLayer {
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    pub sLayerInfo: SLayerInfo,
}

// ============================================================================
// Low-Level CABAC Bitstream & Arithmetic Routines
// ============================================================================

#[inline]
pub unsafe fn PropagateCarry(mut pBufCur: *mut u8, pBufStart: *mut u8) {
    unsafe {
        while pBufCur > pBufStart {
            pBufCur = pBufCur.sub(1);
            let prev = *pBufCur;
            *pBufCur = prev.wrapping_add(1);
            if *pBufCur != 0 {
                break;
            }
        }
    }
}

#[inline]
pub unsafe fn WelsCabacEncodeUpdateLowNontrivial_(pCbCtx: *mut SCabacCtx) {
    unsafe {
        let mut iLowBitCnt = (*pCbCtx).m_iLowBitCnt;
        let mut iRenormCnt = (*pCbCtx).m_iRenormCnt;
        let mut uiLow = (*pCbCtx).m_uiLow;

        loop {
            let mut pBufCur = (*pCbCtx).m_pBufCur;
            let kiInc = (CABAC_LOW_WIDTH as i32) - 1 - iLowBitCnt;

            uiLow <<= kiInc;
            if (uiLow & (1u64 << ((CABAC_LOW_WIDTH as i32) - 1))) != 0 {
                PropagateCarry(pBufCur, (*pCbCtx).m_pBufStart);
            }

            if CABAC_LOW_WIDTH > 32 {
                let be32 = ((uiLow >> 31) as u32).to_be_bytes();
                std::ptr::copy_nonoverlapping(be32.as_ptr(), pBufCur, 4);
                pBufCur = pBufCur.add(4);
            }
            *pBufCur = (uiLow >> 23) as u8;
            pBufCur = pBufCur.add(1);
            *pBufCur = (uiLow >> 15) as u8;
            pBufCur = pBufCur.add(1);

            iRenormCnt -= kiInc;
            iLowBitCnt = 15;
            uiLow &= (1u64 << iLowBitCnt) - 1;
            (*pCbCtx).m_pBufCur = pBufCur;

            if iLowBitCnt + iRenormCnt <= (CABAC_LOW_WIDTH as i32) - 1 {
                break;
            }
        }

        (*pCbCtx).m_iLowBitCnt = iLowBitCnt + iRenormCnt;
        (*pCbCtx).m_uiLow = uiLow << iRenormCnt;
    }
}

#[inline(always)]
pub unsafe fn WelsCabacEncodeUpdateLow_(pCbCtx: *mut SCabacCtx) {
    unsafe {
        let low_bit_cnt = (*pCbCtx).m_iLowBitCnt;
        let renorm_cnt = (*pCbCtx).m_iRenormCnt;
        if (low_bit_cnt + renorm_cnt) < (CABAC_LOW_WIDTH as i32) {
            (*pCbCtx).m_iLowBitCnt += renorm_cnt;
            (*pCbCtx).m_uiLow <<= renorm_cnt;
        } else {
            WelsCabacEncodeUpdateLowNontrivial_(pCbCtx);
        }
        (*pCbCtx).m_iRenormCnt = 0;
    }
}

#[inline]
pub unsafe fn WelsCabacEncodeDecisionLps_(pCbCtx: *mut SCabacCtx, iCtx: i32) {
    unsafe {
        let kiState = (*pCbCtx).m_sStateCtx[iCtx as usize].State() as usize;
        let mut uiRange = (*pCbCtx).m_uiRange;
        let uiRangeLps = g_kuiCabacRangeLps[kiState][((uiRange & 0xff) >> 6) as usize] as u32;
        uiRange = uiRange.wrapping_sub(uiRangeLps);

        let mps = (*pCbCtx).m_sStateCtx[iCtx as usize].Mps();
        let next_mps = mps ^ if kiState == 0 { 1 } else { 0 };
        (*pCbCtx).m_sStateCtx[iCtx as usize].Set(g_kuiStateTransTable[kiState][0], next_mps);

        WelsCabacEncodeUpdateLow_(pCbCtx);
        (*pCbCtx).m_uiLow = (*pCbCtx).m_uiLow.wrapping_add(uiRange as u64);

        let kiRenormAmount = g_kiClz5Table[(uiRangeLps >> 3) as usize] as i32;
        (*pCbCtx).m_uiRange = uiRangeLps << kiRenormAmount;
        (*pCbCtx).m_iRenormCnt = kiRenormAmount;
    }
}

#[inline(always)]
pub unsafe fn WelsCabacEncodeDecision(pCbCtx: *mut SCabacCtx, iCtx: i32, uiBin: u32) {
    unsafe {
        if (uiBin as u8) == (*pCbCtx).m_sStateCtx[iCtx as usize].Mps() {
            let kiState = (*pCbCtx).m_sStateCtx[iCtx as usize].State() as usize;
            let mut uiRange = (*pCbCtx).m_uiRange;
            let uiRangeLps = g_kuiCabacRangeLps[kiState][((uiRange & 0xff) >> 6) as usize] as u32;
            uiRange = uiRange.wrapping_sub(uiRangeLps);

            let kiRenormAmount = ((uiRange >> 8) ^ 1) as i32;
            (*pCbCtx).m_uiRange = uiRange << kiRenormAmount;
            (*pCbCtx).m_iRenormCnt += kiRenormAmount;
            (*pCbCtx).m_sStateCtx[iCtx as usize].Set(g_kuiStateTransTable[kiState][1], uiBin as u8);
        } else {
            WelsCabacEncodeDecisionLps_(pCbCtx, iCtx);
        }
    }
}

#[inline(always)]
pub unsafe fn WelsCabacEncodeBypassOne(pCbCtx: *mut SCabacCtx, uiBin: i32) {
    unsafe {
        (*pCbCtx).m_iRenormCnt += 1;
        WelsCabacEncodeUpdateLow_(pCbCtx);
        if uiBin != 0 {
            (*pCbCtx).m_uiLow = (*pCbCtx).m_uiLow.wrapping_add((*pCbCtx).m_uiRange as u64);
        }
    }
}

#[inline]
pub unsafe fn WelsCabacEncodeTerminate(pCbCtx: *mut SCabacCtx, uiBin: u32) {
    unsafe {
        (*pCbCtx).m_uiRange = (*pCbCtx).m_uiRange.wrapping_sub(2);
        if uiBin != 0 {
            WelsCabacEncodeUpdateLow_(pCbCtx);
            (*pCbCtx).m_uiLow = (*pCbCtx).m_uiLow.wrapping_add((*pCbCtx).m_uiRange as u64);

            let kiRenormAmount: i32 = 7;
            (*pCbCtx).m_uiRange = 2 << kiRenormAmount;
            (*pCbCtx).m_iRenormCnt = kiRenormAmount;

            WelsCabacEncodeUpdateLow_(pCbCtx);
            (*pCbCtx).m_uiLow |= 0x80;
        } else {
            let kiRenormAmount = (((*pCbCtx).m_uiRange >> 8) ^ 1) as i32;
            (*pCbCtx).m_uiRange <<= kiRenormAmount;
            (*pCbCtx).m_iRenormCnt += kiRenormAmount;
        }
    }
}

#[inline]
pub unsafe fn WelsCabacEncodeUeBypass(pCbCtx: *mut SCabacCtx, iExpBits: i32, uiVal: u32) {
    unsafe {
        let mut iSufS = uiVal as i32;
        let mut iStopLoop = 0;
        let mut k = iExpBits;
        while iStopLoop == 0 {
            if iSufS >= (1 << k) {
                WelsCabacEncodeBypassOne(pCbCtx, 1);
                iSufS -= 1 << k;
                k += 1;
            } else {
                WelsCabacEncodeBypassOne(pCbCtx, 0);
                while k > 0 {
                    k -= 1;
                    WelsCabacEncodeBypassOne(pCbCtx, (iSufS >> k) & 1);
                }
                iStopLoop = 1;
            }
        }
    }
}

#[inline]
pub unsafe fn WelsCabacEncodeInit(pCbCtx: *mut SCabacCtx, pBuf: *mut u8, pEnd: *mut u8) {
    unsafe {
        (*pCbCtx).m_uiLow = 0;
        (*pCbCtx).m_iLowBitCnt = 9;
        (*pCbCtx).m_iRenormCnt = 0;
        (*pCbCtx).m_uiRange = 510;
        (*pCbCtx).m_pBufStart = pBuf;
        (*pCbCtx).m_pBufEnd = pEnd;
        (*pCbCtx).m_pBufCur = pBuf;
    }
}

#[inline]
pub unsafe fn BsAlign(pBs: *mut SBitStringAux) {
    unsafe {
        if !pBs.is_null() {
            let left = (*pBs).iLeftBits & 7;
            if left != 0 {
                (*pBs).uiCurBits <<= left;
                (*pBs).uiCurBits |= (1 << left) - 1;
                (*pBs).iLeftBits &= !7;
            }
        }
    }
}

// ============================================================================
// Macroblock Header & Mode Serialization
// ============================================================================

pub unsafe fn WelsCabacMbType(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    iMbWidth: i32,
    eSliceType: EWelsSliceType,
) {
    unsafe {
        if eSliceType == EWelsSliceType::I_SLICE {
            let uiNeighborAvail = (*pCurMb).uiNeighborAvail;
            let pLeftMb = pCurMb.offset(-1);
            let pTopMb = pCurMb.offset(-(iMbWidth as isize));
            let mut iCtx = 3;

            if (uiNeighborAvail & LEFT_MB_POS) != 0 && !IS_INTRA4x4((*pLeftMb).uiMbType) {
                iCtx += 1;
            }
            if (uiNeighborAvail & TOP_MB_POS) != 0 && !IS_INTRA4x4((*pTopMb).uiMbType) {
                iCtx += 1;
            }

            if (*pCurMb).uiMbType == MB_TYPE_INTRA4x4 {
                WelsCabacEncodeDecision(pCabacCtx, iCtx, 0);
            } else {
                let iCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
                let iCbpLuma = ((*pCurMb).uiCbp & 15) as i32;
                let iPredMode = g_kiMapModeI16x16[(*pMbCache).uiLumaI16x16Mode as usize] as i32;

                WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
                WelsCabacEncodeTerminate(pCabacCtx, 0);

                if iCbpLuma != 0 {
                    WelsCabacEncodeDecision(pCabacCtx, 6, 1);
                } else {
                    WelsCabacEncodeDecision(pCabacCtx, 6, 0);
                }

                if iCbpChroma == 0 {
                    WelsCabacEncodeDecision(pCabacCtx, 7, 0);
                } else {
                    WelsCabacEncodeDecision(pCabacCtx, 7, 1);
                    WelsCabacEncodeDecision(pCabacCtx, 8, (iCbpChroma >> 1) as u32);
                }

                WelsCabacEncodeDecision(pCabacCtx, 9, (iPredMode >> 1) as u32);
                WelsCabacEncodeDecision(pCabacCtx, 10, (iPredMode & 1) as u32);
            }
        } else if eSliceType == EWelsSliceType::P_SLICE {
            let uiMbType = (*pCurMb).uiMbType;
            if uiMbType == MB_TYPE_16x16 {
                WelsCabacEncodeDecision(pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(pCabacCtx, 15, 0);
                WelsCabacEncodeDecision(pCabacCtx, 16, 0);
            } else if (uiMbType == MB_TYPE_16x8) || (uiMbType == MB_TYPE_8x16) {
                WelsCabacEncodeDecision(pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(pCabacCtx, 15, 1);
                WelsCabacEncodeDecision(pCabacCtx, 17, if uiMbType == MB_TYPE_16x8 { 1 } else { 0 });
            } else if (uiMbType == MB_TYPE_8x8) || (uiMbType == MB_TYPE_8x8_REF0) {
                WelsCabacEncodeDecision(pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(pCabacCtx, 15, 0);
                WelsCabacEncodeDecision(pCabacCtx, 16, 1);
            } else if (*pCurMb).uiMbType == MB_TYPE_INTRA4x4 {
                WelsCabacEncodeDecision(pCabacCtx, 14, 1);
                WelsCabacEncodeDecision(pCabacCtx, 17, 0);
            } else {
                let iCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
                let iCbpLuma = ((*pCurMb).uiCbp & 15) as i32;
                let iPredMode = g_kiMapModeI16x16[(*pMbCache).uiLumaI16x16Mode as usize] as i32;

                // prefix
                WelsCabacEncodeDecision(pCabacCtx, 14, 1);

                // suffix
                WelsCabacEncodeDecision(pCabacCtx, 17, 1);
                WelsCabacEncodeTerminate(pCabacCtx, 0);
                if iCbpLuma != 0 {
                    WelsCabacEncodeDecision(pCabacCtx, 18, 1);
                } else {
                    WelsCabacEncodeDecision(pCabacCtx, 18, 0);
                }

                if iCbpChroma == 0 {
                    WelsCabacEncodeDecision(pCabacCtx, 19, 0);
                } else {
                    WelsCabacEncodeDecision(pCabacCtx, 19, 1);
                    WelsCabacEncodeDecision(pCabacCtx, 19, (iCbpChroma >> 1) as u32);
                }

                WelsCabacEncodeDecision(pCabacCtx, 20, (iPredMode >> 1) as u32);
                WelsCabacEncodeDecision(pCabacCtx, 20, (iPredMode & 1) as u32);
            }
        }
    }
}

pub unsafe fn WelsCabacMbIntra4x4PredMode(pCabacCtx: *mut SCabacCtx, pMbCache: *mut SMbCache) {
    unsafe {
        for iMode in 0..16 {
            let bPredFlag = *(*pMbCache).pPrevIntra4x4PredModeFlag.add(iMode);
            let iRemMode = *(*pMbCache).pRemIntra4x4PredModeFlag.add(iMode) as i32;

            if bPredFlag {
                WelsCabacEncodeDecision(pCabacCtx, 68, 1);
            } else {
                WelsCabacEncodeDecision(pCabacCtx, 68, 0);
                WelsCabacEncodeDecision(pCabacCtx, 69, (iRemMode & 0x01) as u32);
                WelsCabacEncodeDecision(pCabacCtx, 69, ((iRemMode >> 1) & 0x01) as u32);
                WelsCabacEncodeDecision(pCabacCtx, 69, (iRemMode >> 2) as u32);
            }
        }
    }
}

pub unsafe fn WelsCabacMbIntraChromaPredMode(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    iMbWidth: i32,
) {
    unsafe {
        let uiNeighborAvail = (*pCurMb).uiNeighborAvail;
        let pLeftMb = pCurMb.offset(-1);
        let pTopMb = pCurMb.offset(-(iMbWidth as isize));

        let iPredMode = g_kiMapModeIntraChroma[(*pMbCache).uiChmaI8x8Mode as usize] as i32;
        let mut iCtx = 64;
        if (uiNeighborAvail & LEFT_MB_POS) != 0
            && g_kiMapModeIntraChroma[(*pLeftMb).uiChromPredMode as usize] != 0
        {
            iCtx += 1;
        }
        if (uiNeighborAvail & TOP_MB_POS) != 0
            && g_kiMapModeIntraChroma[(*pTopMb).uiChromPredMode as usize] != 0
        {
            iCtx += 1;
        }

        if iPredMode == 0 {
            WelsCabacEncodeDecision(pCabacCtx, iCtx, 0);
        } else if iPredMode == 1 {
            WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(pCabacCtx, 67, 0);
        } else if iPredMode == 2 {
            WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(pCabacCtx, 67, 1);
            WelsCabacEncodeDecision(pCabacCtx, 67, 0);
        } else {
            WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(pCabacCtx, 67, 1);
            WelsCabacEncodeDecision(pCabacCtx, 67, 1);
        }
    }
}

pub unsafe fn WelsCabacMbCbp(pCurMb: *mut SMB, iMbWidth: i32, pCabacCtx: *mut SCabacCtx) {
    unsafe {
        let cbp = (*pCurMb).uiCbp as i32;
        let iCbpBlockLuma: [u32; 4] = [
            (cbp & 1) as u32,
            ((cbp >> 1) & 1) as u32,
            ((cbp >> 2) & 1) as u32,
            ((cbp >> 3) & 1) as u32,
        ];
        let iCbpChroma = cbp >> 4;
        let mut iCbpBlockLeft: [i32; 4] = [0, 0, 0, 0];
        let mut iCbpBlockTop: [i32; 4] = [0, 0, 0, 0];
        let mut iCbpLeftChroma = 0;
        let mut iCbpTopChroma = 0;
        let mut iCtx = 0;
        let uiNeighborAvail = (*pCurMb).uiNeighborAvail;

        if (uiNeighborAvail & LEFT_MB_POS) != 0 {
            let iCbp = (*pCurMb.offset(-1)).uiCbp as i32;
            iCbpBlockLeft[0] = if (iCbp & 1) != 0 { 0 } else { 1 };
            iCbpBlockLeft[1] = if ((iCbp >> 1) & 1) != 0 { 0 } else { 1 };
            iCbpBlockLeft[2] = if ((iCbp >> 2) & 1) != 0 { 0 } else { 1 };
            iCbpBlockLeft[3] = if ((iCbp >> 3) & 1) != 0 { 0 } else { 1 };
            iCbpLeftChroma = iCbp >> 4;
            if iCbpLeftChroma != 0 {
                iCtx += 1;
            }
        }

        if (uiNeighborAvail & TOP_MB_POS) != 0 {
            let iCbp = (*pCurMb.offset(-(iMbWidth as isize))).uiCbp as i32;
            iCbpBlockTop[0] = if (iCbp & 1) != 0 { 0 } else { 1 };
            iCbpBlockTop[1] = if ((iCbp >> 1) & 1) != 0 { 0 } else { 1 };
            iCbpBlockTop[2] = if ((iCbp >> 2) & 1) != 0 { 0 } else { 1 };
            iCbpBlockTop[3] = if ((iCbp >> 3) & 1) != 0 { 0 } else { 1 };
            iCbpTopChroma = iCbp >> 4;
            if iCbpTopChroma != 0 {
                iCtx += 2;
            }
        }

        let not_cbp0 = if iCbpBlockLuma[0] == 0 { 1 } else { 0 };
        let not_cbp1 = if iCbpBlockLuma[1] == 0 { 1 } else { 0 };
        let not_cbp2 = if iCbpBlockLuma[2] == 0 { 1 } else { 0 };

        WelsCabacEncodeDecision(
            pCabacCtx,
            73 + iCbpBlockLeft[1] + iCbpBlockTop[2] * 2,
            iCbpBlockLuma[0],
        );
        WelsCabacEncodeDecision(
            pCabacCtx,
            73 + not_cbp0 + iCbpBlockTop[3] * 2,
            iCbpBlockLuma[1],
        );
        WelsCabacEncodeDecision(
            pCabacCtx,
            73 + iCbpBlockLeft[3] + not_cbp0 * 2,
            iCbpBlockLuma[2],
        );
        WelsCabacEncodeDecision(
            pCabacCtx,
            73 + not_cbp2 + not_cbp1 * 2,
            iCbpBlockLuma[3],
        );

        // Chroma CBP
        if iCbpChroma != 0 {
            WelsCabacEncodeDecision(pCabacCtx, 77 + iCtx, 1);
            WelsCabacEncodeDecision(
                pCabacCtx,
                81 + (iCbpLeftChroma >> 1) + ((iCbpTopChroma >> 1) * 2),
                if iCbpChroma > 1 { 1 } else { 0 },
            );
        } else {
            WelsCabacEncodeDecision(pCabacCtx, 77 + iCtx, 0);
        }
    }
}

pub unsafe fn WelsCabacMbDeltaQp(
    pCurMb: *mut SMB,
    pCabacCtx: *mut SCabacCtx,
    bFirstMbInSlice: bool,
) {
    unsafe {
        let mut iCtx = 0;

        if !bFirstMbInSlice {
            let pPrevMb = pCurMb.offset(-1);
            (*pCurMb).iLumaDQp = ((*pCurMb).uiLumaQp as i32) - ((*pPrevMb).uiLumaQp as i32);

            if IS_SKIP((*pPrevMb).uiMbType)
                || (((*pPrevMb).uiMbType != MB_TYPE_INTRA16x16) && ((*pPrevMb).uiCbp == 0))
                || ((*pPrevMb).iLumaDQp == 0)
            {
                iCtx = 0;
            } else {
                iCtx = 1;
            }
        }

        if (*pCurMb).iLumaDQp != 0 {
            let mut iValue = if (*pCurMb).iLumaDQp < 0 {
                -2 * (*pCurMb).iLumaDQp
            } else {
                2 * (*pCurMb).iLumaDQp - 1
            };

            WelsCabacEncodeDecision(pCabacCtx, 60 + iCtx, 1);
            if iValue == 1 {
                WelsCabacEncodeDecision(pCabacCtx, 60 + 2, 0);
            } else {
                WelsCabacEncodeDecision(pCabacCtx, 60 + 2, 1);
                iValue -= 1;
                while {
                    iValue -= 1;
                    iValue > 0
                } {
                    WelsCabacEncodeDecision(pCabacCtx, 60 + 3, 1);
                }
                WelsCabacEncodeDecision(pCabacCtx, 60 + 3, 0);
            }
        } else {
            WelsCabacEncodeDecision(pCabacCtx, 60 + iCtx, 0);
        }
    }
}

pub unsafe fn WelsMbSkipCabac(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    iMbWidth: i32,
    eSliceType: EWelsSliceType,
    bSkipFlag: i16,
) {
    unsafe {
        let mut iCtx = if eSliceType == EWelsSliceType::P_SLICE { 11 } else { 24 };
        let uiNeighborAvail = (*pCurMb).uiNeighborAvail;

        if (uiNeighborAvail & LEFT_MB_POS) != 0 {
            if !IS_SKIP((*pCurMb.offset(-1)).uiMbType) {
                iCtx += 1;
            }
        }
        if (uiNeighborAvail & TOP_MB_POS) != 0 {
            if !IS_SKIP((*pCurMb.offset(-(iMbWidth as isize))).uiMbType) {
                iCtx += 1;
            }
        }

        WelsCabacEncodeDecision(pCabacCtx, iCtx, bSkipFlag as u32);

        if bSkipFlag != 0 {
            for i in 0..16 {
                (*pCurMb).sMvd[i].iMvX = 0;
                (*pCurMb).sMvd[i].iMvY = 0;
            }
            (*pCurMb).uiCbp = 0;
            (*pCurMb).iCbpDc = 0;
        }
    }
}

pub unsafe fn WelsCabacMbRef(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    iIdx: i16,
) {
    unsafe {
        let pMvComp = &(*pMbCache).sMvComponents;
        let iRefIdxA = pMvComp.iRefIndexCache[(iIdx + 6) as usize] as i16;
        let iRefIdxB = pMvComp.iRefIndexCache[(iIdx + 1) as usize] as i16;
        let mut iRefIdx = pMvComp.iRefIndexCache[(iIdx + 7) as usize] as i16;
        let mut iCtx: i16 = 0;

        if (iRefIdxA > 0) && (!(*pMbCache).bMbTypeSkip[3]) {
            iCtx += 1;
        }
        if (iRefIdxB > 0) && (!(*pMbCache).bMbTypeSkip[1]) {
            iCtx += 2;
        }

        while iRefIdx > 0 {
            WelsCabacEncodeDecision(pCabacCtx, (54 + iCtx) as i32, 1);
            iCtx = (iCtx >> 2) + 4;
            iRefIdx -= 1;
        }
        WelsCabacEncodeDecision(pCabacCtx, (54 + iCtx) as i32, 0);
    }
}

#[inline]
pub unsafe fn WelsCabacMbMvdLx(
    pCabacCtx: *mut SCabacCtx,
    sMvd: i32,
    iCtx: i32,
    iPredMvd: i32,
) {
    unsafe {
        let iAbsMvd = sMvd.abs();
        let mut iCtxInc = 0;
        let iPrefix = core::cmp::min(iAbsMvd, 9);

        if iPredMvd > 32 {
            iCtxInc += 2;
        } else if iPredMvd > 2 {
            iCtxInc += 1;
        }

        if iPrefix != 0 {
            if iPrefix < 9 {
                WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 1);
                iCtxInc = 3;
                for i in 0..(iPrefix - 1) {
                    WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 1);
                    if i < 3 {
                        iCtxInc += 1;
                    }
                }
                WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 0);
                WelsCabacEncodeBypassOne(pCabacCtx, if sMvd < 0 { 1 } else { 0 });
            } else {
                WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 1);
                iCtxInc = 3;
                for i in 0..(9 - 1) {
                    WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 1);
                    if i < 3 {
                        iCtxInc += 1;
                    }
                }
                WelsCabacEncodeUeBypass(pCabacCtx, 3, (iAbsMvd - 9) as u32);
                WelsCabacEncodeBypassOne(pCabacCtx, if sMvd < 0 { 1 } else { 0 });
            }
        } else {
            WelsCabacEncodeDecision(pCabacCtx, iCtx + iCtxInc, 0);
        }
    }
}

pub unsafe fn WelsCabacMbMvd(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    iMbWidth: u32,
    sCurMv: SMVUnitXY,
    sPredMv: SMVUnitXY,
    i4x4ScanIdx: i16,
) -> SMVUnitXY {
    unsafe {
        let uiNeighborAvail = (*pCurMb).uiNeighborAvail;
        let mut sMvd = SMVUnitXY::default();
        let mut sMvdLeft = SMVUnitXY::default();
        let mut sMvdTop = SMVUnitXY::default();

        sMvd.sDeltaMv(sCurMv, sPredMv);

        if (i4x4ScanIdx < 4) && ((uiNeighborAvail & TOP_MB_POS) != 0) {
            let top_mb = pCurMb.offset(-(iMbWidth as isize));
            sMvdTop.sAssignMv((*top_mb).sMvd[(i4x4ScanIdx + 12) as usize]);
        } else if i4x4ScanIdx >= 4 {
            sMvdTop.sAssignMv((*pCurMb).sMvd[(i4x4ScanIdx - 4) as usize]);
        }

        if ((i4x4ScanIdx & 0x03) == 0) && ((uiNeighborAvail & LEFT_MB_POS) != 0) {
            let left_mb = pCurMb.offset(-1);
            sMvdLeft.sAssignMv((*left_mb).sMvd[(i4x4ScanIdx + 3) as usize]);
        } else if (i4x4ScanIdx & 0x03) != 0 {
            sMvdLeft.sAssignMv((*pCurMb).sMvd[(i4x4ScanIdx - 1) as usize]);
        }

        let iAbsMvd0 = (sMvdLeft.iMvX.abs() as i32) + (sMvdTop.iMvX.abs() as i32);
        let iAbsMvd1 = (sMvdLeft.iMvY.abs() as i32) + (sMvdTop.iMvY.abs() as i32);

        WelsCabacMbMvdLx(pCabacCtx, sMvd.iMvX as i32, 40, iAbsMvd0);
        WelsCabacMbMvdLx(pCabacCtx, sMvd.iMvY as i32, 47, iAbsMvd1);

        sMvd
    }
}

pub unsafe fn WelsCabacSubMbType(pCabacCtx: *mut SCabacCtx, pCurMb: *mut SMB) {
    unsafe {
        for i8x8Idx in 0..4 {
            let uiSubMbType = (*pCurMb).uiSubMbType[i8x8Idx] as u32;
            if SUB_MB_TYPE_8x8 == uiSubMbType {
                WelsCabacEncodeDecision(pCabacCtx, 21, 1);
                continue;
            }
            WelsCabacEncodeDecision(pCabacCtx, 21, 0);
            if SUB_MB_TYPE_8x4 == uiSubMbType {
                WelsCabacEncodeDecision(pCabacCtx, 22, 0);
            } else {
                WelsCabacEncodeDecision(pCabacCtx, 22, 1);
                WelsCabacEncodeDecision(
                    pCabacCtx,
                    23,
                    if SUB_MB_TYPE_4x8 == uiSubMbType { 1 } else { 0 },
                );
            }
        }
    }
}

pub unsafe fn WelsCabacSubMbMvd(
    pCabacCtx: *mut SCabacCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    kiMbWidth: i32,
) {
    unsafe {
        for i8x8Idx in 0..4 {
            let uiSubMbType = (*pCurMb).uiSubMbType[i8x8Idx] as u32;
            if SUB_MB_TYPE_8x8 == uiSubMbType {
                let i4x4ScanIdx = g_kuiMbCountScan4Idx[i8x8Idx << 2] as i16;
                let cur_mv = *(*pCurMb).sMv.add(i4x4ScanIdx as usize);
                let pred_mv = (*pMbCache).sMbMvp[i4x4ScanIdx as usize];
                let sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, kiMbWidth as u32, cur_mv, pred_mv, i4x4ScanIdx);

                let idx = i4x4ScanIdx as usize;
                (*pCurMb).sMvd[idx].sAssignMv(sMvd);
                (*pCurMb).sMvd[1 + idx].sAssignMv(sMvd);
                (*pCurMb).sMvd[4 + idx].sAssignMv(sMvd);
                (*pCurMb).sMvd[5 + idx].sAssignMv(sMvd);
            } else if SUB_MB_TYPE_4x4 == uiSubMbType {
                for i4x4Idx in 0..4 {
                    let i4x4ScanIdx = g_kuiMbCountScan4Idx[(i8x8Idx << 2) + i4x4Idx] as i16;
                    let cur_mv = *(*pCurMb).sMv.add(i4x4ScanIdx as usize);
                    let pred_mv = (*pMbCache).sMbMvp[i4x4ScanIdx as usize];
                    let sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, kiMbWidth as u32, cur_mv, pred_mv, i4x4ScanIdx);

                    (*pCurMb).sMvd[i4x4ScanIdx as usize].sAssignMv(sMvd);
                }
            } else if SUB_MB_TYPE_8x4 == uiSubMbType {
                for i8x4Idx in 0..2 {
                    let i4x4ScanIdx = g_kuiMbCountScan4Idx[(i8x8Idx << 2) + (i8x4Idx << 1)] as i16;
                    let cur_mv = *(*pCurMb).sMv.add(i4x4ScanIdx as usize);
                    let pred_mv = (*pMbCache).sMbMvp[i4x4ScanIdx as usize];
                    let sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, kiMbWidth as u32, cur_mv, pred_mv, i4x4ScanIdx);

                    let idx = i4x4ScanIdx as usize;
                    (*pCurMb).sMvd[idx].sAssignMv(sMvd);
                    (*pCurMb).sMvd[1 + idx].sAssignMv(sMvd);
                }
            } else if SUB_MB_TYPE_4x8 == uiSubMbType {
                for i4x8Idx in 0..2 {
                    let i4x4ScanIdx = g_kuiMbCountScan4Idx[(i8x8Idx << 2) + i4x8Idx] as i16;
                    let cur_mv = *(*pCurMb).sMv.add(i4x4ScanIdx as usize);
                    let pred_mv = (*pMbCache).sMbMvp[i4x4ScanIdx as usize];
                    let sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, kiMbWidth as u32, cur_mv, pred_mv, i4x4ScanIdx);

                    let idx = i4x4ScanIdx as usize;
                    (*pCurMb).sMvd[idx].sAssignMv(sMvd);
                    (*pCurMb).sMvd[4 + idx].sAssignMv(sMvd);
                }
            }
        }
    }
}

pub unsafe fn WelsGetMbCtxCabac(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: u32,
    eCtxBlockCat: ECtxBlockCat,
    iIdx: i16,
) -> i16 {
    unsafe {
        let mut iNzA: i16 = -1;
        let mut iNzB: i16 = -1;
        let pNonZeroCoeffCount = (*pMbCache).iNonZeroCoeffCount.as_ptr();
        let bIntra = IS_INTRA((*pCurMb).uiMbType);
        let mut iCtxInc = 0;

        match eCtxBlockCat {
            ECtxBlockCat::LUMA_AC | ECtxBlockCat::CHROMA_AC | ECtxBlockCat::LUMA_4x4 => {
                iNzA = *pNonZeroCoeffCount.offset((iIdx - 1) as isize) as i16;
                iNzB = *pNonZeroCoeffCount.offset((iIdx - 8) as isize) as i16;
            }
            ECtxBlockCat::LUMA_DC | ECtxBlockCat::CHROMA_DC => {
                if ((*pCurMb).uiNeighborAvail & LEFT_MB_POS) != 0 {
                    iNzA = ((*pCurMb.offset(-1)).iCbpDc & (1 << iIdx)) as i16;
                }
                if ((*pCurMb).uiNeighborAvail & TOP_MB_POS) != 0 {
                    iNzB = ((*pCurMb.offset(-(iMbWidth as isize))).iCbpDc & (1 << iIdx)) as i16;
                }
            }
        }

        if ((iNzA == -1) && bIntra) || (iNzA > 0) {
            iCtxInc += 1;
        }
        if ((iNzB == -1) && bIntra) || (iNzB > 0) {
            iCtxInc += 2;
        }

        85 + (uiCodecBlockFlagOffset[eCtxBlockCat as usize] as i16) + iCtxInc
    }
}

pub unsafe fn WelsWriteBlockResidualCabac(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: u32,
    pCabacCtx: *mut SCabacCtx,
    eCtxBlockCat: ECtxBlockCat,
    iIdx: i16,
    iNonZeroCount: i16,
    pBlock: *mut i16,
    iEndIdx: i16,
) {
    unsafe {
        let mut iCtx = WelsGetMbCtxCabac(pMbCache, pCurMb, iMbWidth, eCtxBlockCat, iIdx) as i32;

        if iNonZeroCount != 0 {
            let mut iLevel = [0i16; 16];
            let iCtxSig = 105 + (uiSignificantCoeffFlagOffset[eCtxBlockCat as usize] as i32);
            let iCtxLast = 166 + (uiLastCoeffFlagOffset[eCtxBlockCat as usize] as i32);
            let iCtxLevel = 227 + (uiCoeffAbsLevelMinus1Offset[eCtxBlockCat as usize] as i32);
            let mut iNonZeroIdx: usize = 0;
            let mut i: usize = 0;

            WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
            loop {
                let coeff = *pBlock.add(i);
                if coeff != 0 {
                    iLevel[iNonZeroIdx] = coeff;
                    iNonZeroIdx += 1;

                    WelsCabacEncodeDecision(pCabacCtx, iCtxSig + (i as i32), 1);
                    if (iNonZeroIdx as i16) != iNonZeroCount {
                        WelsCabacEncodeDecision(pCabacCtx, iCtxLast + (i as i32), 0);
                    } else {
                        WelsCabacEncodeDecision(pCabacCtx, iCtxLast + (i as i32), 1);
                        break;
                    }
                } else {
                    WelsCabacEncodeDecision(pCabacCtx, iCtxSig + (i as i32), 0);
                }

                i += 1;
                if (i as i16) == iEndIdx {
                    iLevel[iNonZeroIdx] = *pBlock.add(i);
                    iNonZeroIdx += 1;
                    break;
                }
            }

            let mut iNumAbsLevelGt1: i32 = 0;
            let mut iCtx1: i32 = iCtxLevel + 1;

            loop {
                iNonZeroIdx -= 1;
                let lvl = iLevel[iNonZeroIdx];
                let abs_lvl = (lvl as i32).abs();
                let mut iPrefix = abs_lvl - 1;

                if iPrefix != 0 {
                    iPrefix = core::cmp::min(iPrefix, 14);
                    iCtx = core::cmp::min(iCtxLevel + 4, iCtx1);
                    WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
                    iNumAbsLevelGt1 += 1;

                    let max_shift = 5 - if eCtxBlockCat == ECtxBlockCat::CHROMA_DC { 1 } else { 0 };
                    iCtx = iCtxLevel + 4 + core::cmp::min(max_shift, iNumAbsLevelGt1);

                    for _ in 1..iPrefix {
                        WelsCabacEncodeDecision(pCabacCtx, iCtx, 1);
                    }

                    if abs_lvl < 15 {
                        WelsCabacEncodeDecision(pCabacCtx, iCtx, 0);
                    } else {
                        WelsCabacEncodeUeBypass(pCabacCtx, 0, (abs_lvl - 15) as u32);
                    }
                    iCtx1 = iCtxLevel;
                } else {
                    iCtx = core::cmp::min(iCtxLevel + 4, iCtx1);
                    WelsCabacEncodeDecision(pCabacCtx, iCtx, 0);
                    if iNumAbsLevelGt1 == 0 {
                        iCtx1 += 1;
                    }
                }

                WelsCabacEncodeBypassOne(pCabacCtx, if lvl < 0 { 1 } else { 0 });

                if iNonZeroIdx == 0 {
                    break;
                }
            }
        } else {
            WelsCabacEncodeDecision(pCabacCtx, iCtx, 0);
        }
    }
}

#[inline]
pub unsafe fn WelsCalNonZeroCount2x2Block(pBlock: *const i16) -> i32 {
    unsafe {
        ((*pBlock != 0) as i32)
            + ((*pBlock.add(1) != 0) as i32)
            + ((*pBlock.add(2) != 0) as i32)
            + ((*pBlock.add(3) != 0) as i32)
    }
}

pub unsafe fn WelsWriteMbResidualCabac(
    pFuncList: *mut SWelsFuncPtrList,
    pSlice: *mut SSlice,
    sMbCacheInfo: *mut SMbCache,
    pCurMb: *mut SMB,
    pCabacCtx: *mut SCabacCtx,
    iMbWidth: i16,
    uiChromaQpIndexOffset: u32,
) -> i32 {
    unsafe {
        let uiMbType = (*pCurMb).uiMbType;
        let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
        let pNonZeroCoeffCount = (*pMbCache).iNonZeroCoeffCount.as_ptr();
        let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
        let iSliceFirstMbXY = pSliceHeadExt.sSliceHeader.iFirstMbInSlice;

        (*pCurMb).iCbpDc = 0;
        (*pCurMb).iLumaDQp = 0;

        if ((*pCurMb).uiCbp > 0) || (uiMbType == MB_TYPE_INTRA16x16) {
            let iCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
            let iCbpLuma = ((*pCurMb).uiCbp & 15) as i32;

            (*pCurMb).iLumaDQp = ((*pCurMb).uiLumaQp as i32) - ((*pSlice).uiLastMbQp as i32);
            WelsCabacMbDeltaQp(pCurMb, pCabacCtx, (*pCurMb).iMbXY == iSliceFirstMbXY);
            (*pSlice).uiLastMbQp = (*pCurMb).uiLumaQp;

            let pDct = (*pMbCache).pDct;

            if uiMbType == MB_TYPE_INTRA16x16 {
                let dc_buf = (*pDct).iLumaI16x16Dc.as_mut_ptr();
                let iNonZeroCount = if !pFuncList.is_null()
                    && (*pFuncList).pfGetNoneZeroCount.is_some()
                {
                    ((*pFuncList).pfGetNoneZeroCount.unwrap())(dc_buf)
                } else {
                    (*pDct).iLumaI16x16Dc.iter().filter(|&&x| x != 0).count() as i32
                };

                WelsWriteBlockResidualCabac(
                    pMbCache,
                    pCurMb,
                    iMbWidth as u32,
                    pCabacCtx,
                    ECtxBlockCat::LUMA_DC,
                    0,
                    iNonZeroCount as i16,
                    dc_buf,
                    15,
                );

                if iNonZeroCount != 0 {
                    (*pCurMb).iCbpDc |= 1;
                }

                if iCbpLuma != 0 {
                    for i in 0..16 {
                        let iIdx = g_kuiCache48CountScan4Idx[i] as i16;
                        let nz = *pNonZeroCoeffCount.offset(iIdx as isize) as i16;
                        let block_buf = (*pDct).iLumaBlock[i].as_mut_ptr();

                        WelsWriteBlockResidualCabac(
                            pMbCache,
                            pCurMb,
                            iMbWidth as u32,
                            pCabacCtx,
                            ECtxBlockCat::LUMA_AC,
                            iIdx,
                            nz,
                            block_buf,
                            14,
                        );
                    }
                }
            } else {
                for i in 0..16 {
                    if (iCbpLuma & (1 << (i >> 2))) != 0 {
                        let iIdx = g_kuiCache48CountScan4Idx[i] as i16;
                        let nz = *pNonZeroCoeffCount.offset(iIdx as isize) as i16;
                        let block_buf = (*pDct).iLumaBlock[i].as_mut_ptr();

                        WelsWriteBlockResidualCabac(
                            pMbCache,
                            pCurMb,
                            iMbWidth as u32,
                            pCabacCtx,
                            ECtxBlockCat::LUMA_4x4,
                            iIdx,
                            nz,
                            block_buf,
                            15,
                        );
                    }
                }
            }

            if iCbpChroma != 0 {
                let cb_dc_buf = (*pDct).iChromaDc[0].as_mut_ptr();
                let mut iNonZeroCount = WelsCalNonZeroCount2x2Block(cb_dc_buf);
                if iNonZeroCount != 0 {
                    (*pCurMb).iCbpDc |= 0x2;
                }
                WelsWriteBlockResidualCabac(
                    pMbCache,
                    pCurMb,
                    iMbWidth as u32,
                    pCabacCtx,
                    ECtxBlockCat::CHROMA_DC,
                    1,
                    iNonZeroCount as i16,
                    cb_dc_buf,
                    3,
                );

                let cr_dc_buf = (*pDct).iChromaDc[1].as_mut_ptr();
                iNonZeroCount = WelsCalNonZeroCount2x2Block(cr_dc_buf);
                if iNonZeroCount != 0 {
                    (*pCurMb).iCbpDc |= 0x4;
                }
                WelsWriteBlockResidualCabac(
                    pMbCache,
                    pCurMb,
                    iMbWidth as u32,
                    pCabacCtx,
                    ECtxBlockCat::CHROMA_DC,
                    2,
                    iNonZeroCount as i16,
                    cr_dc_buf,
                    3,
                );

                if (iCbpChroma & 0x02) != 0 {
                    let g_kuiCache48CountScan4Idx_16base = &g_kuiCache48CountScan4Idx[16..];

                    // Cb AC
                    for i in 0..4 {
                        let iIdx = g_kuiCache48CountScan4Idx_16base[i] as i16;
                        let nz = *pNonZeroCoeffCount.offset(iIdx as isize) as i16;
                        let block_buf = (*pDct).iChromaBlock[i].as_mut_ptr();

                        WelsWriteBlockResidualCabac(
                            pMbCache,
                            pCurMb,
                            iMbWidth as u32,
                            pCabacCtx,
                            ECtxBlockCat::CHROMA_AC,
                            iIdx,
                            nz,
                            block_buf,
                            14,
                        );
                    }

                    // Cr AC
                    for i in 0..4 {
                        let iIdx = (24 + g_kuiCache48CountScan4Idx_16base[i]) as i16;
                        let nz = *pNonZeroCoeffCount.offset(iIdx as isize) as i16;
                        let block_buf = (*pDct).iChromaBlock[4 + i].as_mut_ptr();

                        WelsWriteBlockResidualCabac(
                            pMbCache,
                            pCurMb,
                            iMbWidth as u32,
                            pCabacCtx,
                            ECtxBlockCat::CHROMA_AC,
                            iIdx,
                            nz,
                            block_buf,
                            14,
                        );
                    }
                }
            }
        } else {
            (*pCurMb).iLumaDQp = 0;
            (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
            let qp_idx = CLIP3_QP_0_51(((*pCurMb).uiLumaQp as i32) + (uiChromaQpIndexOffset as i32));
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[qp_idx];
        }

        0
    }
}

// ============================================================================
// Top-Level Slice & Macroblock CABAC Entry Points
// ============================================================================

pub unsafe fn WelsInitSliceCabac(
    pEncCtx: *mut crate::encoder::encoder_context::sWelsEncCtx,
    pSlice: *mut SSlice,
) {
    unsafe {
        let pBs = (*pSlice).pSliceBsa;
        BsAlign(pBs);

        WelsCabacEncodeInit(
            &mut (*pSlice).sCabacCtx,
            (*pBs).pCurBuf,
            (*pBs).pEndBuf,
        );
    }
}

pub unsafe fn WelsSpatialWriteMbSynCabac(
    pEncCtx: *mut crate::encoder::encoder_context::sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) -> i32 {
    unsafe {
        let pCabacCtx = &mut (*pSlice).sCabacCtx as *mut SCabacCtx;
        let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
        let uiMbType = (*pCurMb).uiMbType;
        let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
        let uiNumRefIdxL0Active = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;
        let iSliceFirstMbXY = pSliceHeadExt.sSliceHeader.iFirstMbInSlice;
        let iMbWidth = (*(*pEncCtx).pCurDqLayer).iMbWidth as i32;

        let uiChromaQpIndexOffset = (*(*(*pEncCtx).pCurDqLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset;
        let mut sMvd = SMVUnitXY::default();
        let mut iRet = 0;

        if (*pCurMb).iMbXY > iSliceFirstMbXY {
            WelsCabacEncodeTerminate(pCabacCtx, 0);
        }

        if IS_SKIP((*pCurMb).uiMbType) {
            (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
            let qp_idx = CLIP3_QP_0_51(((*pCurMb).uiLumaQp as i32) + (uiChromaQpIndexOffset as i32));
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[qp_idx];
            WelsMbSkipCabac(pCabacCtx, pCurMb, iMbWidth, std::mem::transmute((*pEncCtx).eSliceType as i32), 1);
        } else {
            if (*pEncCtx).eSliceType as i32 != EWelsSliceType::I_SLICE as i32 {
                WelsMbSkipCabac(pCabacCtx, pCurMb, iMbWidth, std::mem::transmute((*pEncCtx).eSliceType as i32), 0);
            }

            WelsCabacMbType(pCabacCtx, pCurMb, pMbCache, iMbWidth, std::mem::transmute((*pEncCtx).eSliceType as i32));

            if IS_INTRA(uiMbType) {
                if uiMbType == MB_TYPE_INTRA4x4 {
                    WelsCabacMbIntra4x4PredMode(pCabacCtx, pMbCache);
                }
                WelsCabacMbIntraChromaPredMode(pCabacCtx, pCurMb, pMbCache, iMbWidth);
                sMvd.iMvX = 0;
                sMvd.iMvY = 0;
                for i in 0..16 {
                    (*pCurMb).sMvd[i].sAssignMv(sMvd);
                }
            } else if uiMbType == MB_TYPE_16x16 {
                if uiNumRefIdxL0Active > 0 {
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 0);
                }
                let cur_mv = *(*pCurMb).sMv;
                let pred_mv = (*pMbCache).sMbMvp[0];
                sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, iMbWidth as u32, cur_mv, pred_mv, 0);

                for i in 0..16 {
                    (*pCurMb).sMvd[i].sAssignMv(sMvd);
                }
            } else if uiMbType == MB_TYPE_16x8 {
                if uiNumRefIdxL0Active > 0 {
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 0);
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 12);
                }
                let cur_mv0 = *(*pCurMb).sMv;
                let pred_mv0 = (*pMbCache).sMbMvp[0];
                sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, iMbWidth as u32, cur_mv0, pred_mv0, 0);
                for i in 0..8 {
                    (*pCurMb).sMvd[i].sAssignMv(sMvd);
                }
                let cur_mv8 = *(*pCurMb).sMv.add(8);
                let pred_mv1 = (*pMbCache).sMbMvp[1];
                sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, iMbWidth as u32, cur_mv8, pred_mv1, 8);
                for i in 8..16 {
                    (*pCurMb).sMvd[i].sAssignMv(sMvd);
                }
            } else if uiMbType == MB_TYPE_8x16 {
                if uiNumRefIdxL0Active > 0 {
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 0);
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 2);
                }
                let cur_mv0 = *(*pCurMb).sMv;
                let pred_mv0 = (*pMbCache).sMbMvp[0];
                sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, iMbWidth as u32, cur_mv0, pred_mv0, 0);
                let mut i = 0;
                while i < 16 {
                    (*pCurMb).sMvd[i].sAssignMv(sMvd);
                    (*pCurMb).sMvd[i + 1].sAssignMv(sMvd);
                    i += 4;
                }
                let cur_mv2 = *(*pCurMb).sMv.add(2);
                let pred_mv1 = (*pMbCache).sMbMvp[1];
                sMvd = WelsCabacMbMvd(pCabacCtx, pCurMb, iMbWidth as u32, cur_mv2, pred_mv1, 2);
                let mut i = 0;
                while i < 16 {
                    (*pCurMb).sMvd[i + 2].sAssignMv(sMvd);
                    (*pCurMb).sMvd[i + 3].sAssignMv(sMvd);
                    i += 4;
                }
            } else if (uiMbType == MB_TYPE_8x8) || (uiMbType == MB_TYPE_8x8_REF0) {
                WelsCabacSubMbType(pCabacCtx, pCurMb);
                if uiNumRefIdxL0Active > 0 {
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 0);
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 2);
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 12);
                    WelsCabacMbRef(pCabacCtx, pCurMb, pMbCache, 14);
                }
                WelsCabacSubMbMvd(pCabacCtx, pCurMb, pMbCache, iMbWidth);
            }

            if uiMbType != MB_TYPE_INTRA16x16 {
                WelsCabacMbCbp(pCurMb, iMbWidth, pCabacCtx);
            }

            let pFuncList = (*pEncCtx).pFuncList as *mut SWelsFuncPtrList;
            iRet = WelsWriteMbResidualCabac(
                pFuncList,
                pSlice,
                pMbCache,
                pCurMb,
                pCabacCtx,
                iMbWidth as i16,
                uiChromaQpIndexOffset as u32,
            );
        }

        if !IS_INTRA((*pCurMb).uiMbType) {
            (*pCurMb).uiChromPredMode = 0;
        }

        iRet
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cal_nonzero_count_2x2() {
        let block_zero = [0i16, 0, 0, 0];
        assert_eq!(unsafe { WelsCalNonZeroCount2x2Block(block_zero.as_ptr()) }, 0);

        let block_mixed = [1i16, 0, -3, 0];
        assert_eq!(unsafe { WelsCalNonZeroCount2x2Block(block_mixed.as_ptr()) }, 2);

        let block_full = [4i16, -2, 5, 1];
        assert_eq!(unsafe { WelsCalNonZeroCount2x2Block(block_full.as_ptr()) }, 4);
    }

    #[test]
    fn test_cabac_state_ctx() {
        let mut state_ctx = SStateCtx::default();
        state_ctx.Set(30, 1);
        assert_eq!(state_ctx.State(), 30);
        assert_eq!(state_ctx.Mps(), 1);

        state_ctx.Set(63, 0);
        assert_eq!(state_ctx.State(), 63);
        assert_eq!(state_ctx.Mps(), 0);
    }

    #[test]
    fn test_cabac_encode_init_and_terminate() {
        let mut buffer = vec![0u8; 128];
        let mut cabac_ctx = SCabacCtx::default();
        unsafe {
            let start = buffer.as_mut_ptr();
            let end = start.add(buffer.len());
            WelsCabacEncodeInit(&mut cabac_ctx, start, end);
            assert_eq!(cabac_ctx.m_uiRange, 510);
            assert_eq!(cabac_ctx.m_iLowBitCnt, 9);

            WelsCabacEncodeTerminate(&mut cabac_ctx, 0);
            assert_eq!(cabac_ctx.m_uiRange, 508);
        }
    }

    #[test]
    fn test_cabac_mb_skip_logic() {
        let mut buffer = vec![0u8; 128];
        let mut cabac_ctx = SCabacCtx::default();
        let mut cur_mb = SMB::default();
        cur_mb.uiMbType = MB_TYPE_SKIP;

        unsafe {
            let start = buffer.as_mut_ptr();
            let end = start.add(buffer.len());
            WelsCabacEncodeInit(&mut cabac_ctx, start, end);

            WelsMbSkipCabac(
                &mut cabac_ctx,
                &mut cur_mb,
                16,
                EWelsSliceType::P_SLICE,
                1,
            );

            assert_eq!(cur_mb.uiCbp, 0);
            assert_eq!(cur_mb.iCbpDc, 0);
            for i in 0..16 {
                assert_eq!(cur_mb.sMvd[i].iMvX, 0);
                assert_eq!(cur_mb.sMvd[i].iMvY, 0);
            }
        }
    }
}
