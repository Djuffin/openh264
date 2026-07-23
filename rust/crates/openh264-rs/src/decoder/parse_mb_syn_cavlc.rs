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

//! # Macroblock Syntax Parsing & CAVLC Entropy Decoding
//!
//! Translated from `codec/decoder/core/src/parse_mb_syn_cavlc.cpp` and
//! `codec/decoder/core/inc/parse_mb_syn_cavlc.h`.
//!
//! Implements macroblock-level syntax parsing, context-adaptive variable-length
//! decoding (CAVLC), neighbor availability derivation, intra prediction mode
//! verification, and inter-frame motion information parsing for H.264 / AVC decoding.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use super::bit_stream::SBitStringAux;

// ============================================================================
// Constants & Error Codes
// ============================================================================

pub const MAX_LEVEL_PREFIX: i32 = 15;
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MV_A: usize = 2;

pub const REF_NOT_AVAIL: i8 = -2;
pub const REF_NOT_IN_LIST: i8 = -1;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_SLICE_DATA: i32 = 6;
pub const ERR_LEVEL_MB_DATA: i32 = 7;

pub const ERR_INVALID_INTRA4X4_MODE: i32 = -1;
pub const ERR_INFO_INVALID_SUB_MB_TYPE: i32 = 1037;
pub const ERR_INFO_INVALID_REF_INDEX: i32 = 1040;
pub const ERR_INFO_CAVLC_INVALID_LEVEL: i32 = 1044;
pub const ERR_INFO_CAVLC_INVALID_TOTAL_COEFF_OR_TRAILING_ONES: i32 = 1045;
pub const ERR_INFO_CAVLC_INVALID_ZERO_LEFT: i32 = 1046;
pub const ERR_INFO_CAVLC_INVALID_RUN_BEFORE: i32 = 1047;
pub const ERR_INFO_INVALID_I4x4_PRED_MODE: i32 = 1050;
pub const ERR_INFO_INVALID_I16x16_PRED_MODE: i32 = 1051;
pub const ERR_INFO_INVALID_I_CHROMA_PRED_MODE: i32 = 1052;
pub const ERR_INFO_UNSUPPORTED_ILP: i32 = 1064;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = 1075;

pub const dsBitstreamError: i32 = 0x02;
pub const ERROR_CON_DISABLE: i32 = 0;

#[inline(always)]
pub const fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

// AVC Macroblock Types
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
pub const MB_TYPE_DIRECT: u32 = 0x00000800;
pub const MB_TYPE_P0L0: u32 = 0x00001000;
pub const MB_TYPE_P1L0: u32 = 0x00002000;
pub const MB_TYPE_P0L1: u32 = 0x00004000;
pub const MB_TYPE_P1L1: u32 = 0x00008000;

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;
pub const MB_TYPE_INTER: u32 =
    MB_TYPE_16x16 | MB_TYPE_16x8 | MB_TYPE_8x16 | MB_TYPE_8x8 | MB_TYPE_8x8_REF0 | MB_TYPE_SKIP | MB_TYPE_DIRECT;

#[inline(always)]
pub const fn IS_INTRA4x4(t: u32) -> bool {
    t == MB_TYPE_INTRA4x4
}
#[inline(always)]
pub const fn IS_INTRA8x8(t: u32) -> bool {
    t == MB_TYPE_INTRA8x8
}
#[inline(always)]
pub const fn IS_INTRANxN(t: u32) -> bool {
    (t & (MB_TYPE_INTRA4x4 | MB_TYPE_INTRA8x8)) != 0
}
#[inline(always)]
pub const fn IS_INTRA16x16(t: u32) -> bool {
    (t & MB_TYPE_INTRA16x16) != 0
}
#[inline(always)]
pub const fn IS_INTRA(t: u32) -> bool {
    (t & MB_TYPE_INTRA) != 0
}
#[inline(always)]
pub const fn IS_INTER(t: u32) -> bool {
    (t & MB_TYPE_INTER) != 0
}
#[inline(always)]
pub const fn IS_INTER_16x16(t: u32) -> bool {
    (t & MB_TYPE_16x16) != 0
}
#[inline(always)]
pub const fn IS_INTER_16x8(t: u32) -> bool {
    (t & MB_TYPE_16x8) != 0
}
#[inline(always)]
pub const fn IS_INTER_8x16(t: u32) -> bool {
    (t & MB_TYPE_8x16) != 0
}
#[inline(always)]
pub const fn IS_Inter_8x8(t: u32) -> bool {
    (t & (MB_TYPE_8x8 | MB_TYPE_8x8_REF0)) != 0
}
#[inline(always)]
pub const fn IS_DIRECT(t: u32) -> bool {
    (t & MB_TYPE_DIRECT) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x4(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x4) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_4x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x4(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_4x4) != 0
}
#[inline(always)]
pub const fn IS_DIR(a: u32, part: usize, list: usize) -> bool {
    (a & (MB_TYPE_P0L0 << (part + 2 * list))) != 0
}

// Intra prediction mode identifiers
pub const I16_PRED_V: i8 = 0;
pub const I16_PRED_H: i8 = 1;
pub const I16_PRED_DC: i8 = 2;
pub const I16_PRED_P: i8 = 3;
pub const I16_PRED_DC_L: i8 = 4;
pub const I16_PRED_DC_T: i8 = 5;
pub const I16_PRED_DC_128: i8 = 6;
pub const MAX_PRED_MODE_ID_I16x16: i8 = 3;

pub const I4_PRED_V: i8 = 0;
pub const I4_PRED_H: i8 = 1;
pub const I4_PRED_DC: i8 = 2;
pub const I4_PRED_DDL: i8 = 3;
pub const I4_PRED_DDR: i8 = 4;
pub const I4_PRED_VR: i8 = 5;
pub const I4_PRED_HD: i8 = 6;
pub const I4_PRED_VL: i8 = 7;
pub const I4_PRED_HU: i8 = 8;
pub const I4_PRED_DC_L: i8 = 9;
pub const I4_PRED_DC_T: i8 = 10;
pub const I4_PRED_DC_128: i8 = 11;
pub const I4_PRED_DDL_TOP: i8 = 12;
pub const I4_PRED_VL_TOP: i8 = 13;
pub const MAX_PRED_MODE_ID_I4x4: i8 = 8;

pub const C_PRED_DC: i8 = 0;
pub const C_PRED_H: i8 = 1;
pub const C_PRED_V: i8 = 2;
pub const C_PRED_P: i8 = 3;
pub const C_PRED_DC_L: i8 = 4;
pub const C_PRED_DC_T: i8 = 5;
pub const C_PRED_DC_128: i8 = 6;

// Residual block properties
pub const I16_LUMA_DC: i32 = 1;
pub const I16_LUMA_AC: i32 = 2;
pub const LUMA_DC_AC: i32 = 3;
pub const CHROMA_DC: i32 = 4;
pub const CHROMA_AC: i32 = 5;
pub const LUMA_DC_AC_8: i32 = 6;
pub const CHROMA_DC_U: i32 = 7;
pub const CHROMA_DC_V: i32 = 8;
pub const CHROMA_AC_U: i32 = 9;
pub const CHROMA_AC_V: i32 = 10;
pub const LUMA_DC_AC_INTRA: i32 = 11;
pub const LUMA_DC_AC_INTER: i32 = 12;
pub const CHROMA_DC_U_INTER: i32 = 13;
pub const CHROMA_DC_V_INTER: i32 = 14;
pub const CHROMA_AC_U_INTER: i32 = 15;
pub const CHROMA_AC_V_INTER: i32 = 16;
pub const LUMA_DC_AC_INTRA_8: i32 = 17;
pub const LUMA_DC_AC_INTER_8: i32 = 18;

pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;

// ============================================================================
// Core Data Structures
// ============================================================================

/// 32-bit big-endian bit window cache for CAVLC parsing
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagReadBitsCache {
    pub uiCache32Bit: u32,
    pub uiRemainBits: u8,
    pub pBuf: *mut u8,
}
pub type SReadBitsCache = TagReadBitsCache;

impl Default for TagReadBitsCache {
    fn default() -> Self {
        Self {
            uiCache32Bit: 0,
            uiRemainBits: 0,
            pBuf: std::ptr::null_mut(),
        }
    }
}

/// Neighbor availability tracking descriptor
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsNeighAvail {
    pub iTopAvail: i32,
    pub iLeftAvail: i32,
    pub iRightTopAvail: i32,
    pub iLeftTopAvail: i32,

    pub iLeftCbp: i32,
    pub iTopCbp: i32,

    pub iLeftType: u32,
    pub iTopType: u32,
    pub iRightTopType: u32,
    pub iLeftTopType: u32,
}
pub type PWelsNeighAvail = *mut SWelsNeighAvail;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SI16PredInfo {
    pub iPredMode: i8,
    pub iLeftAvail: i8,
    pub iTopAvail: i8,
    pub iLeftTopAvail: i8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SI4PredInfo {
    pub iPredMode: i8,
    pub iLeftAvail: i8,
    pub iTopAvail: i8,
    pub iLeftTopAvail: i8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPartMbInfo {
    pub iType: u32,
    pub iPartCount: i8,
    pub iPartWidth: i8,
}

#[repr(C)]
pub struct SVlcTable {
    pub kpCoeffTokenVlcTable: [*const [[u8; 2]; 256]; 4],
    pub kpChromaCoeffTokenVlcTable: *const [u8; 2],
    pub kpZeroTable: [*const [u8; 2]; 7],
    pub kpTotalZerosTable: [[*const [u8; 2]; 15]; 2],
}

// Forward definitions matching OpenH264 decoder C ABI structs
#[repr(C)]
pub struct SPicture {
    pub pData: [*mut u8; 3],
    pub iLinesize: [i32; 3],
    pub pMbType: *mut u32,
    pub pMv: [*mut [[i16; 2]; 16]; 2],
    pub pRefIndex: [*mut [i8; 16]; 2],
    pub bIsComplete: bool,
    pub bIsLongRef: bool,
}

pub type PPicture = *mut SPicture;

#[repr(C)]
pub struct SLevelLimits {
    pub iMinVmv: i16,
    pub iMaxVmv: i16,
}

#[repr(C)]
pub struct SSps {
    pub pSLevelLimits: *mut SLevelLimits,
    pub iMbWidth: i32,
    pub iMbHeight: i32,
}

#[repr(C)]
pub struct SSliceHeader {
    pub eSliceType: i32,
    pub uiRefCount: [i32; 2],
    pub pSps: *mut SSps,
    pub iDirectSpatialMvPredFlag: i32,
}

#[repr(C)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    pub bDefaultMotionPredFlag: bool,
    pub bAdaptiveMotionPredFlag: bool,
}

#[repr(C)]
pub struct SSlice {
    pub sSliceHeaderExt: SSliceHeaderExt,
}

#[repr(C)]
pub struct SLayerInfo {
    pub sSliceInLayer: SSlice,
}

#[repr(C)]
pub struct SDqLayer {
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub iMbX: i32,
    pub iMbY: i32,
    pub iMbXyIndex: i32,
    pub pSliceIdc: *mut i32,
    pub pMbType: *mut u32,
    pub pCbp: *mut i32,
    pub pNzc: *mut [u8; 24],
    pub pIntraPredMode: *mut [i8; 8],
    pub pDec: PPicture,
    pub pMvd: [*mut [[i16; 2]; 16]; 2],
    pub pDirect: *mut [i8; 16],
    pub pSubMbType: *mut [u32; 4],
    pub pNoSubMbPartSizeLessThan8x8Flag: *mut bool,
    pub sLayerInfo: SLayerInfo,
    pub iColocMv: [*mut [i16; 2]; 2],
    pub iColocIntra: *mut bool,
    pub iColocRefIndex: [*mut i8; 2],
    pub pScaledTCoeff: *mut [i16; 256],
}
pub type PDqLayer = *mut SDqLayer;

#[repr(C)]
pub struct SRefPic {
    pub pRefList: [*mut PPicture; 2],
    pub uiRefCount: [i32; 2],
}

#[repr(C)]
pub struct SParam {
    pub eEcActiveIdc: i32,
}

#[repr(C)]
pub struct SLogContext {
    pub pLogCtx: *mut std::ffi::c_void,
}

#[repr(C)]
pub struct SWelsDecoderContext {
    pub sRefPic: SRefPic,
    pub pCurDqLayer: PDqLayer,
    pub pParam: *mut SParam,
    pub sLogCtx: SLogContext,
    pub bMbRefConcealed: bool,
    pub bRPLRError: bool,
    pub iErrorCode: i32,
    pub bUseScalingList: bool,
    pub pDequant_coeff4x4: [*const [u16; 8]; 6],
    pub pDequant_coeff8x8: [*const [u16; 64]; 6],
    pub pDec: PPicture,
    pub pTempDec: PPicture,
    pub pSps: *mut SSps,
    pub eSliceType: i32,
}
pub type PWelsDecoderContext = *mut SWelsDecoderContext;

// ============================================================================
// Raw Memory Access Helpers
// ============================================================================

#[inline(always)]
pub unsafe fn LD16(ptr: *const u8) -> u16 {
    unsafe { (ptr as *const u16).read_unaligned() }
}

#[inline(always)]
pub unsafe fn ST16(ptr: *mut u8, val: u16) {
    unsafe { (ptr as *mut u16).write_unaligned(val) }
}

#[inline(always)]
pub unsafe fn LD32(ptr: *const u8) -> u32 {
    unsafe { (ptr as *const u32).read_unaligned() }
}

#[inline(always)]
pub unsafe fn ST32(ptr: *mut u8, val: u32) {
    unsafe { (ptr as *mut u32).write_unaligned(val) }
}

#[inline(always)]
pub unsafe fn LD64(ptr: *const u8) -> u64 {
    unsafe { (ptr as *const u64).read_unaligned() }
}

#[inline(always)]
pub unsafe fn ST64(ptr: *mut u8, val: u64) {
    unsafe { (ptr as *mut u64).write_unaligned(val) }
}

#[inline(always)]
pub unsafe fn POP_BUFFER(pBitsCache: *mut SReadBitsCache, iCount: u32) {
    unsafe {
        (*pBitsCache).uiCache32Bit = (*pBitsCache).uiCache32Bit.wrapping_shl(iCount);
        (*pBitsCache).uiRemainBits = (*pBitsCache).uiRemainBits.wrapping_sub(iCount as u8);
    }
}

#[inline(always)]
pub unsafe fn SHIFT_BUFFER(pBitsCache: *mut SReadBitsCache) {
    unsafe {
        let pBuf = (*pBitsCache).pBuf;
        let b2 = *pBuf.add(2) as u32;
        let b3 = *pBuf.add(3) as u32;
        (*pBitsCache).pBuf = pBuf.add(2);
        (*pBitsCache).uiRemainBits = (*pBitsCache).uiRemainBits.wrapping_add(16);
        let shift = 32u32.wrapping_sub((*pBitsCache).uiRemainBits as u32);
        (*pBitsCache).uiCache32Bit |= ((b2 << 8) | b3).wrapping_shl(shift);
    }
}

#[inline(always)]
pub fn GetPrefixBits(mut uiValue: u32) -> u32 {
    let mut iNumBit = 0u32;
    if (uiValue & 0xffff0000) != 0 {
        uiValue >>= 16;
        iNumBit += 16;
    }
    if (uiValue & 0xff00) != 0 {
        uiValue >>= 8;
        iNumBit += 8;
    }
    if (uiValue & 0xf0) != 0 {
        uiValue >>= 4;
        iNumBit += 4;
    }
    iNumBit += g_kuiPrefix8BitsTable[(uiValue & 0x0f) as usize];
    32 - iNumBit
}

#[inline(always)]
pub fn wels_non_zero_count_average(nA: i8, nB: i8) -> i8 {
    let mut nC = (nA as i32) + (nB as i32) + 1;
    let shift = if nA != -1 && nB != -1 { 1 } else { 0 };
    nC >>= shift;
    let add = if nA == -1 && nB == -1 { 1 } else { 0 };
    (nC + add) as i8
}

// ============================================================================
// Static Lookup Tables
// ============================================================================

pub static g_kuiPrefix8BitsTable: [u32; 16] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3];

pub static g_kuiCache30ScanIdx: [u8; 16] = [
    7, 8, 13, 14,
    9, 10, 15, 16,
    19, 20, 25, 26,
    21, 22, 27, 28,
];

pub static g_kuiCache48CountScan4Idx: [u8; 24] = [
    9, 10, 17, 18, 11, 12, 19, 20, 25, 26, 33, 34, 27, 28, 35, 36,
    14, 15, 22, 23, 38, 39, 46, 47,
];

pub static g_kuiScan4: [u8; 16] = [
    0, 1, 4, 5,
    2, 3, 6, 7,
    8, 9, 12, 13,
    10, 11, 14, 15,
];

pub static g_kuiScan8: [u8; 24] = [
    9, 10, 17, 18, 11, 12, 19, 20, 25, 26, 33, 34, 27, 28, 35, 36,
    14, 15, 22, 23, 38, 39, 46, 47,
];

pub static g_kuiNcMapTable: [u8; 17] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];

pub static g_kuiVlcTableNeedMoreBitsThread: [u8; 3] = [0, 0, 0];
pub static g_kuiVlcTableMoreBitsCount0: [u8; 4] = [0, 0, 0, 0];
pub static g_kuiVlcTableMoreBitsCount1: [u8; 4] = [6, 3, 1, 1];
pub static g_kuiVlcTableMoreBitsCount2: [u8; 8] = [2, 2, 2, 2, 1, 1, 1, 1];

pub static g_kuiTotalZerosBitNumMap: [u8; 15] = [1, 2, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9];
pub static g_kuiTotalZerosBitNumChromaMap: [u8; 3] = [1, 2, 3];
pub static g_kuiZeroLeftBitNumMap: [u8; 16] = [0, 1, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3];

pub static g_ksI16PredInfo: [SI16PredInfo; 4] = [
    SI16PredInfo { iPredMode: I16_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: I16_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: I16_PRED_P, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
];

pub static g_ksChromaPredInfo: [SI16PredInfo; 4] = [
    SI16PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_P, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
];

pub static g_ksI4PredInfo: [SI4PredInfo; 9] = [
    SI4PredInfo { iPredMode: I4_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_DDL, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_DDR, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_VR, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_HD, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_VL, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_HU, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
];

pub static g_ksInterPSubMbTypeInfo: [SPartMbInfo; 4] = [
    SPartMbInfo { iType: SUB_MB_TYPE_8x8, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4, iPartCount: 4, iPartWidth: 1 },
];

pub static g_ksInterBSubMbTypeInfo: [SPartMbInfo; 13] = [
    SPartMbInfo { iType: MB_TYPE_DIRECT, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L0, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L0, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L0, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L0, iPartCount: 4, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L1, iPartCount: 4, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 4, iPartWidth: 1 },
];

pub static g_kuiVlcTrailingOneTotalCoeffTable: [[u8; 2]; 62] = [
    [0, 0], [0, 1], [1, 1], [0, 2], [1, 2], [2, 2], [0, 3], [1, 3], [2, 3], [3, 3],
    [0, 4], [1, 4], [2, 4], [3, 4], [0, 5], [1, 5], [2, 5], [3, 5], [0, 6], [1, 6],
    [2, 6], [3, 6], [0, 7], [1, 7], [2, 7], [3, 7], [0, 8], [1, 8], [2, 8], [3, 8],
    [0, 9], [1, 9], [2, 9], [3, 9], [0, 10], [1, 10], [2, 10], [3, 10], [0, 11], [1, 11],
    [2, 11], [3, 11], [0, 12], [1, 12], [2, 12], [3, 12], [0, 13], [1, 13], [2, 13], [3, 13],
    [0, 14], [1, 14], [2, 14], [3, 14], [0, 15], [1, 15], [2, 15], [3, 15], [0, 16], [1, 16],
    [2, 16], [3, 16],
];

pub static g_kuiDequantCoeff: [[u16; 8]; 52] = [
    [10, 13, 10, 13, 13, 16, 13, 16], [11, 14, 11, 14, 14, 18, 14, 18],
    [13, 16, 13, 16, 16, 20, 16, 20], [14, 18, 14, 18, 18, 23, 18, 23],
    [16, 20, 16, 20, 20, 25, 20, 25], [18, 23, 18, 23, 23, 29, 23, 29],
    [20, 26, 20, 26, 26, 32, 26, 32], [22, 28, 22, 28, 28, 36, 28, 36],
    [26, 32, 26, 32, 32, 40, 32, 40], [28, 36, 28, 36, 36, 46, 36, 46],
    [32, 40, 32, 40, 40, 50, 40, 50], [36, 46, 36, 46, 46, 58, 46, 58],
    [40, 52, 40, 52, 52, 64, 52, 64], [44, 56, 44, 56, 56, 72, 56, 72],
    [52, 64, 52, 64, 64, 80, 64, 80], [56, 72, 56, 72, 72, 92, 72, 92],
    [64, 80, 64, 80, 80, 100, 80, 100], [72, 92, 72, 92, 92, 116, 92, 116],
    [80, 104, 80, 104, 104, 128, 104, 128], [88, 112, 88, 112, 112, 144, 112, 144],
    [104, 128, 104, 128, 128, 160, 128, 160], [112, 144, 112, 144, 144, 184, 144, 184],
    [128, 160, 128, 160, 160, 200, 160, 200], [144, 184, 144, 184, 184, 232, 184, 232],
    [160, 208, 160, 208, 208, 256, 208, 256], [176, 224, 176, 224, 224, 288, 224, 288],
    [208, 256, 208, 256, 256, 320, 256, 320], [224, 288, 224, 288, 288, 368, 288, 368],
    [256, 320, 256, 320, 320, 400, 320, 400], [288, 368, 288, 368, 368, 464, 368, 464],
    [320, 416, 320, 416, 416, 512, 416, 512], [352, 448, 352, 448, 448, 576, 448, 576],
    [416, 512, 416, 512, 512, 640, 512, 640], [448, 576, 448, 576, 576, 736, 576, 736],
    [512, 640, 512, 640, 640, 800, 640, 800], [576, 736, 576, 736, 736, 928, 736, 928],
    [640, 832, 640, 832, 832, 1024, 832, 1024], [704, 896, 704, 896, 896, 1152, 896, 1152],
    [832, 1024, 832, 1024, 1024, 1280, 1024, 1280], [896, 1152, 896, 1152, 1152, 1472, 1152, 1472],
    [1024, 1280, 1024, 1280, 1280, 1600, 1280, 1600], [1152, 1472, 1152, 1472, 1472, 1856, 1472, 1856],
    [1280, 1664, 1280, 1664, 1664, 2048, 1664, 2048], [1408, 1792, 1408, 1792, 1792, 2304, 1792, 2304],
    [1664, 2048, 1664, 2048, 2048, 2560, 2048, 2560], [1792, 2304, 1792, 2304, 2304, 2944, 2304, 2944],
    [2048, 2560, 2048, 2560, 2560, 3200, 2560, 3200], [2304, 2944, 2304, 2944, 2944, 3712, 2944, 3712],
    [2560, 3328, 2560, 3328, 3328, 4096, 3328, 4096], [2816, 3584, 2816, 3584, 3584, 4608, 3584, 4608],
    [3328, 4096, 3328, 4096, 4096, 5120, 4096, 5120], [3584, 4608, 3584, 4608, 4608, 5888, 4608, 5888],
];

// ============================================================================
// Neighborhood Availability & Context Loading
// ============================================================================

/// Evaluates spatial neighborhood availability (Left, Top, Top-Left, Top-Right)
/// for the current macroblock across slice boundaries.
pub unsafe fn GetNeighborAvailMbType(pNeighAvail: PWelsNeighAvail, pCurDqLayer: PDqLayer) {
    if pNeighAvail.is_null() || pCurDqLayer.is_null() {
        return;
    }
    unsafe {
        let dq = &*pCurDqLayer;
        let na = &mut *pNeighAvail;

        let iCurXy = dq.iMbXyIndex;
        let iCurX = dq.iMbX;
        let iCurY = dq.iMbY;
        let iCurSliceIdc = *dq.pSliceIdc.add(iCurXy as usize);

        let mut iLeftXy = 0;
        let mut iTopXy = 0;
        let mut iLeftTopXy = 0;
        let mut iRightTopXy = 0;

        if iCurX != 0 {
            iLeftXy = iCurXy - 1;
            let iLeftSliceIdc = *dq.pSliceIdc.add(iLeftXy as usize);
            na.iLeftAvail = if iLeftSliceIdc == iCurSliceIdc { 1 } else { 0 };
            na.iLeftCbp = if na.iLeftAvail != 0 { *dq.pCbp.add(iLeftXy as usize) } else { 0 };
        } else {
            na.iLeftAvail = 0;
            na.iLeftTopAvail = 0;
            na.iLeftCbp = 0;
        }

        if iCurY != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
            let iTopSliceIdc = *dq.pSliceIdc.add(iTopXy as usize);
            na.iTopAvail = if iTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            na.iTopCbp = if na.iTopAvail != 0 { *dq.pCbp.add(iTopXy as usize) } else { 0 };

            if iCurX != 0 {
                iLeftTopXy = iTopXy - 1;
                let iLeftTopSliceIdc = *dq.pSliceIdc.add(iLeftTopXy as usize);
                na.iLeftTopAvail = if iLeftTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            } else {
                na.iLeftTopAvail = 0;
            }

            if iCurX != (dq.iMbWidth - 1) {
                iRightTopXy = iTopXy + 1;
                let iRightTopSliceIdc = *dq.pSliceIdc.add(iRightTopXy as usize);
                na.iRightTopAvail = if iRightTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            } else {
                na.iRightTopAvail = 0;
            }
        } else {
            na.iTopAvail = 0;
            na.iLeftTopAvail = 0;
            na.iRightTopAvail = 0;
            na.iTopCbp = 0;
        }

        na.iLeftType = if na.iLeftAvail != 0 && !dq.pDec.is_null() {
            *(*dq.pDec).pMbType.add(iLeftXy as usize)
        } else {
            0
        };
        na.iTopType = if na.iTopAvail != 0 && !dq.pDec.is_null() {
            *(*dq.pDec).pMbType.add(iTopXy as usize)
        } else {
            0
        };
        na.iLeftTopType = if na.iLeftTopAvail != 0 && !dq.pDec.is_null() {
            *(*dq.pDec).pMbType.add(iLeftTopXy as usize)
        } else {
            0
        };
        na.iRightTopType = if na.iRightTopAvail != 0 && !dq.pDec.is_null() {
            *(*dq.pDec).pMbType.add(iRightTopXy as usize)
        } else {
            0
        };
    }
}

/// Fills the 48-entry local cache `pNonZeroCount` from neighboring macroblocks.
pub unsafe fn WelsFillCacheNonZeroCount(
    pNeighAvail: PWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pCurDqLayer: PDqLayer,
) {
    if pNeighAvail.is_null() || pNonZeroCount.is_null() || pCurDqLayer.is_null() {
        return;
    }
    unsafe {
        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let mut iTopXy = 0;
        let mut iLeftXy = 0;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
            let pTopNzc = (*dq.pNzc.add(iTopXy as usize)).as_ptr();
            ST32(pNonZeroCount.add(1), LD32(pTopNzc.add(12)));
            *pNonZeroCount.add(0) = 0;
            *pNonZeroCount.add(5) = 0;
            *pNonZeroCount.add(29) = 0;
            ST16(pNonZeroCount.add(6), LD16(pTopNzc.add(20)));
            ST16(pNonZeroCount.add(30), LD16(pTopNzc.add(22)));
        } else {
            ST32(pNonZeroCount.add(1), 0xFFFFFFFF);
            *pNonZeroCount.add(0) = 0xFF;
            *pNonZeroCount.add(5) = 0xFF;
            *pNonZeroCount.add(29) = 0xFF;
            ST16(pNonZeroCount.add(6), 0xFFFF);
            ST16(pNonZeroCount.add(30), 0xFFFF);
        }

        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
            let pLeftNzc = (*dq.pNzc.add(iLeftXy as usize)).as_ptr();
            *pNonZeroCount.add(8 * 1) = *pLeftNzc.add(3);
            *pNonZeroCount.add(8 * 2) = *pLeftNzc.add(7);
            *pNonZeroCount.add(8 * 3) = *pLeftNzc.add(11);
            *pNonZeroCount.add(8 * 4) = *pLeftNzc.add(15);

            *pNonZeroCount.add(5 + 8 * 1) = *pLeftNzc.add(17);
            *pNonZeroCount.add(5 + 8 * 2) = *pLeftNzc.add(21);
            *pNonZeroCount.add(5 + 8 * 4) = *pLeftNzc.add(19);
            *pNonZeroCount.add(5 + 8 * 5) = *pLeftNzc.add(23);
        } else {
            *pNonZeroCount.add(8 * 1) = 0xFF;
            *pNonZeroCount.add(8 * 2) = 0xFF;
            *pNonZeroCount.add(8 * 3) = 0xFF;
            *pNonZeroCount.add(8 * 4) = 0xFF;

            *pNonZeroCount.add(5 + 8 * 1) = 0xFF;
            *pNonZeroCount.add(5 + 8 * 2) = 0xFF;
            *pNonZeroCount.add(5 + 8 * 4) = 0xFF;
            *pNonZeroCount.add(5 + 8 * 5) = 0xFF;
        }
    }
}

pub unsafe fn WelsFillCacheConstrain1IntraNxN(
    pNeighAvail: PWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pIntraPredMode: *mut i8,
    pCurDqLayer: PDqLayer,
) {
    unsafe {
        WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, pCurDqLayer);

        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let mut iTopXy = 0;
        let mut iLeftXy = 0;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
        }
        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
        }

        if na.iTopAvail != 0 && IS_INTRANxN(na.iTopType) {
            let pTopMode = (*dq.pIntraPredMode.add(iTopXy as usize)).as_ptr();
            ST32(pIntraPredMode.add(1) as *mut u8, LD32(pTopMode as *const u8));
        } else {
            let iPred: u32 = if IS_INTRA16x16(na.iTopType) || (MB_TYPE_INTRA_PCM == na.iTopType) {
                0x02020202
            } else {
                0xffffffff
            };
            ST32(pIntraPredMode.add(1) as *mut u8, iPred);
        }

        if na.iLeftAvail != 0 && IS_INTRANxN(na.iLeftType) {
            let pLeftMode = (*dq.pIntraPredMode.add(iLeftXy as usize)).as_ptr();
            *pIntraPredMode.add(0 + 8) = *pLeftMode.add(4);
            *pIntraPredMode.add(0 + 8 * 2) = *pLeftMode.add(5);
            *pIntraPredMode.add(0 + 8 * 3) = *pLeftMode.add(6);
            *pIntraPredMode.add(0 + 8 * 4) = *pLeftMode.add(3);
        } else {
            let iPred: i8 = if IS_INTRA16x16(na.iLeftType) || (MB_TYPE_INTRA_PCM == na.iLeftType) {
                2
            } else {
                -1
            };
            *pIntraPredMode.add(0 + 8) = iPred;
            *pIntraPredMode.add(0 + 8 * 2) = iPred;
            *pIntraPredMode.add(0 + 8 * 3) = iPred;
            *pIntraPredMode.add(0 + 8 * 4) = iPred;
        }
    }
}

pub unsafe fn WelsFillCacheConstrain0IntraNxN(
    pNeighAvail: PWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pIntraPredMode: *mut i8,
    pCurDqLayer: PDqLayer,
) {
    unsafe {
        WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, pCurDqLayer);

        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let mut iTopXy = 0;
        let mut iLeftXy = 0;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
        }
        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
        }

        if na.iTopAvail != 0 && IS_INTRANxN(na.iTopType) {
            let pTopMode = (*dq.pIntraPredMode.add(iTopXy as usize)).as_ptr();
            ST32(pIntraPredMode.add(1) as *mut u8, LD32(pTopMode as *const u8));
        } else {
            let iPred: u32 = if na.iTopAvail != 0 { 0x02020202 } else { 0xffffffff };
            ST32(pIntraPredMode.add(1) as *mut u8, iPred);
        }

        if na.iLeftAvail != 0 && IS_INTRANxN(na.iLeftType) {
            let pLeftMode = (*dq.pIntraPredMode.add(iLeftXy as usize)).as_ptr();
            *pIntraPredMode.add(0 + 8 * 1) = *pLeftMode.add(4);
            *pIntraPredMode.add(0 + 8 * 2) = *pLeftMode.add(5);
            *pIntraPredMode.add(0 + 8 * 3) = *pLeftMode.add(6);
            *pIntraPredMode.add(0 + 8 * 4) = *pLeftMode.add(3);
        } else {
            let iPred: i8 = if na.iLeftAvail != 0 { 2 } else { -1 };
            *pIntraPredMode.add(0 + 8 * 1) = iPred;
            *pIntraPredMode.add(0 + 8 * 2) = iPred;
            *pIntraPredMode.add(0 + 8 * 3) = iPred;
            *pIntraPredMode.add(0 + 8 * 4) = iPred;
        }
    }
}

// ============================================================================
// Intra Mode Validation
// ============================================================================

/// Predicts the most probable mode for an Intra 4x4 sub-block.
pub unsafe fn PredIntra4x4Mode(pIntraPredMode: *mut i8, iIdx4: i32) -> i32 {
    unsafe {
        let scan = g_kuiScan8[iIdx4 as usize] as usize;
        let iTopMode = *pIntraPredMode.add(scan - 8);
        let iLeftMode = *pIntraPredMode.add(scan - 1);

        if iLeftMode == -1 || iTopMode == -1 {
            2
        } else {
            (iLeftMode.min(iTopMode)) as i32
        }
    }
}

#[inline(always)]
fn CHECK_I16_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksI16PredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

#[inline(always)]
fn CHECK_CHROMA_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksChromaPredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

#[inline(always)]
fn CHECK_I4_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksI4PredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

pub unsafe fn CheckIntra16x16PredMode(uiSampleAvail: u8, pMode: *mut i8) -> i32 {
    unsafe {
        let iLeftAvail = (uiSampleAvail & 0x04) as i32;
        let bLeftTopAvail = (uiSampleAvail & 0x02) as i32;
        let iTopAvail = (uiSampleAvail & 0x01) as i32;

        if *pMode < 0 || *pMode > MAX_PRED_MODE_ID_I16x16 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
        }

        if I16_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return ERR_NONE;
            } else if iLeftAvail != 0 {
                *pMode = I16_PRED_DC_L;
            } else if iTopAvail != 0 {
                *pMode = I16_PRED_DC_T;
            } else {
                *pMode = I16_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_I16_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
            }
        }
        ERR_NONE
    }
}

pub unsafe fn CheckIntraChromaPredMode(uiSampleAvail: u8, pMode: *mut i8) -> i32 {
    unsafe {
        let iLeftAvail = (uiSampleAvail & 0x04) as i32;
        let bLeftTopAvail = (uiSampleAvail & 0x02) as i32;
        let iTopAvail = (uiSampleAvail & 0x01) as i32;

        if C_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return ERR_NONE;
            } else if iLeftAvail != 0 {
                *pMode = C_PRED_DC_L;
            } else if iTopAvail != 0 {
                *pMode = C_PRED_DC_T;
            } else {
                *pMode = C_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_CHROMA_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
            }
        }
        ERR_NONE
    }
}

pub unsafe fn CheckIntraNxNPredMode(
    pSampleAvail: *const i32,
    pMode: *mut i8,
    iIndex: i32,
    b8x8: bool,
) -> i32 {
    unsafe {
        let iIdx = g_kuiCache30ScanIdx[iIndex as usize] as isize;

        let iLeftAvail = *pSampleAvail.offset(iIdx - 1);
        let iTopAvail = *pSampleAvail.offset(iIdx - 6);
        let bLeftTopAvail = *pSampleAvail.offset(iIdx - 7);
        let bRightTopAvail = *pSampleAvail.offset(iIdx - if b8x8 { 4 } else { 5 });

        if *pMode < 0 || *pMode > MAX_PRED_MODE_ID_I4x4 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE);
        }

        let mut iFinalMode: i8;

        if I4_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return *pMode as i32;
            } else if iLeftAvail != 0 {
                iFinalMode = I4_PRED_DC_L;
            } else if iTopAvail != 0 {
                iFinalMode = I4_PRED_DC_T;
            } else {
                iFinalMode = I4_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_I4_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE);
            }

            iFinalMode = *pMode;

            if I4_PRED_DDL == iFinalMode && bRightTopAvail == 0 {
                iFinalMode = I4_PRED_DDL_TOP;
            } else if I4_PRED_VL == iFinalMode && bRightTopAvail == 0 {
                iFinalMode = I4_PRED_VL_TOP;
            }
        }
        iFinalMode as i32
    }
}

// ============================================================================
// Bitstream CAVLC Synchronization
// ============================================================================

pub unsafe fn BsStartCavlc(pBs: *mut SBitStringAux) {
    unsafe {
        let bs = &mut *pBs;
        bs.iIndex = ((bs.pCurBuf.offset_from(bs.pStartBuf)) << 3) - (16 - bs.iLeftBits as isize);
    }
}

pub unsafe fn BsEndCavlc(pBs: *mut SBitStringAux) {
    unsafe {
        let bs = &mut *pBs;
        bs.pCurBuf = bs.pStartBuf.offset(bs.iIndex >> 3);
        let b0 = *bs.pCurBuf as u32;
        let b1 = *bs.pCurBuf.add(1) as u32;
        let b2 = *bs.pCurBuf.add(2) as u32;
        let b3 = *bs.pCurBuf.add(3) as u32;
        let uiCache32Bit = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
        bs.uiCurBits = uiCache32Bit << ((bs.iIndex & 0x07) as u32);
        bs.pCurBuf = bs.pCurBuf.add(4);
        bs.iLeftBits = -16 + ((bs.iIndex & 0x07) as i32);
    }
}

// ============================================================================
// Inverse Quantization and Transforms (IDCT)
// ============================================================================

pub unsafe fn WelsChromaDcIdct(pBlock: *mut i16) {
    unsafe {
        let iStride: isize = 32;
        let iXStride: isize = 16;
        let iStride1 = iXStride + iStride;
        let pBlk = pBlock;

        let mut iA = *pBlk as i32;
        let mut iB = *pBlk.offset(iXStride) as i32;
        let mut iC = *pBlk.offset(iStride) as i32;
        let iD = *pBlk.offset(iStride1) as i32;

        let iE = iA - iB;
        iA += iB;
        iB = iC - iD;
        iC += iD;

        *pBlk = (iA + iC) as i16;
        *pBlk.offset(iXStride) = (iE + iB) as i16;
        *pBlk.offset(iStride) = (iA - iC) as i16;
        *pBlk.offset(iStride1) = (iE - iB) as i16;
    }
}

#[inline(always)]
pub fn GetMbResProperty(pMBproperty: &mut i32, pResidualProperty: &mut i32, bCavlc: bool) {
    match *pResidualProperty {
        CHROMA_AC_U => {
            *pMBproperty = 1;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_U };
        }
        CHROMA_AC_V => {
            *pMBproperty = 2;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_V };
        }
        LUMA_DC_AC_INTRA => {
            *pMBproperty = 0;
            *pResidualProperty = LUMA_DC_AC;
        }
        CHROMA_DC_U => {
            *pMBproperty = 1;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_U };
        }
        CHROMA_DC_V => {
            *pMBproperty = 2;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_V };
        }
        I16_LUMA_AC | I16_LUMA_DC => {
            *pMBproperty = 0;
        }
        LUMA_DC_AC_INTER => {
            *pMBproperty = 3;
            *pResidualProperty = LUMA_DC_AC;
        }
        CHROMA_DC_U_INTER => {
            *pMBproperty = 4;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_U };
        }
        CHROMA_DC_V_INTER => {
            *pMBproperty = 5;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_V };
        }
        CHROMA_AC_U_INTER => {
            *pMBproperty = 4;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_U };
        }
        CHROMA_AC_V_INTER => {
            *pMBproperty = 5;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_V };
        }
        _ => {}
    }
}

pub unsafe fn WelsLumaDcDequantIdct(pBlock: *mut i16, uiQp: u8, pCtx: *mut SWelsDecoderContext) {
    unsafe {
        let kiQMul: i32 = if !pCtx.is_null() && (*pCtx).bUseScalingList && !(*pCtx).pDequant_coeff4x4[0].is_null() {
            (*(*pCtx).pDequant_coeff4x4[0].add(uiQp as usize))[0] as i32
        } else {
            (g_kuiDequantCoeff[uiQp as usize][0] as i32) << 4
        };

        const STRIDE: isize = 16;
        let mut iTemp = [0i32; 16];
        let pBlk = pBlock;
        let kiXOffset: [isize; 4] = [0, STRIDE, STRIDE << 2, 5 * STRIDE];
        let kiYOffset: [isize; 4] = [0, STRIDE << 1, STRIDE << 3, 10 * STRIDE];

        for i in 0..4 {
            let kiOffset = kiYOffset[i];
            let kiX1 = kiOffset + kiXOffset[2];
            let kiX2 = STRIDE + kiOffset;
            let kiX3 = kiOffset + kiXOffset[3];
            let kiI4 = i << 2;
            let kiZ0 = *pBlk.offset(kiOffset) as i32 + *pBlk.offset(kiX1) as i32;
            let kiZ1 = *pBlk.offset(kiOffset) as i32 - *pBlk.offset(kiX1) as i32;
            let kiZ2 = *pBlk.offset(kiX2) as i32 - *pBlk.offset(kiX3) as i32;
            let kiZ3 = *pBlk.offset(kiX2) as i32 + *pBlk.offset(kiX3) as i32;

            iTemp[kiI4] = kiZ0 + kiZ3;
            iTemp[1 + kiI4] = kiZ1 + kiZ2;
            iTemp[2 + kiI4] = kiZ1 - kiZ2;
            iTemp[3 + kiI4] = kiZ0 - kiZ3;
        }

        for i in 0..4 {
            let kiOffset = kiXOffset[i];
            let kiI4 = 4 + i;
            let kiZ0 = iTemp[i] + iTemp[4 + kiI4];
            let kiZ1 = iTemp[i] - iTemp[4 + kiI4];
            let kiZ2 = iTemp[kiI4] - iTemp[8 + kiI4];
            let kiZ3 = iTemp[kiI4] + iTemp[8 + kiI4];

            *pBlk.offset(kiOffset) = (((kiZ0 + kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[1] + kiOffset) = (((kiZ1 + kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[2] + kiOffset) = (((kiZ1 - kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[3] + kiOffset) = (((kiZ0 - kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_non_zero_count_average() {
        assert_eq!(wels_non_zero_count_average(3, 2), 3);
        assert_eq!(wels_non_zero_count_average(-1, 4), 4);
        assert_eq!(wels_non_zero_count_average(2, -1), 2);
        assert_eq!(wels_non_zero_count_average(-1, -1), 0);
    }

    #[test]
    fn test_pred_intra_4x4_mode() {
        let mut modes = [0i8; 64];
        // Set top and left modes
        modes[9 - 8] = 3; // Top
        modes[9 - 1] = 1; // Left
        unsafe {
            let res = PredIntra4x4Mode(modes.as_mut_ptr(), 0);
            assert_eq!(res, 1);
        }
    }

    #[test]
    fn test_check_intra16x16_pred_mode() {
        let mut mode: i8 = I16_PRED_DC;
        unsafe {
            let err = CheckIntra16x16PredMode(0x04, &mut mode);
            assert_eq!(err, ERR_NONE);
            assert_eq!(mode, I16_PRED_DC_L);
        }
    }

    #[test]
    fn test_chroma_dc_idct() {
        let mut blk = [0i16; 64];
        blk[0] = 10;
        blk[16] = 2;
        blk[32] = 4;
        blk[48] = 1;

        unsafe {
            WelsChromaDcIdct(blk.as_mut_ptr());
        }

        // iA=10, iB=2, iC=4, iD=1 -> iE=8, iA=12, iB=3, iC=5
        // blk[0] = 12+5 = 17
        // blk[16] = 8+3 = 11
        // blk[32] = 12-5 = 7
        // blk[48] = 8-3 = 5
        assert_eq!(blk[0], 17);
        assert_eq!(blk[16], 11);
        assert_eq!(blk[32], 7);
        assert_eq!(blk[48], 5);
    }
}
