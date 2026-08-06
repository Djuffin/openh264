#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! CAVLC Macroblock Syntax Elements Serialization and Residual Bitstream Encoding.
//!
//! Translated from `codec/encoder/core/src/svc_set_mb_syn_cavlc.cpp` and
//! `codec/encoder/core/inc/svc_set_mb_syn.h`.

use crate::decoder::bit_stream::SBitStringAux;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;

// ============================================================================
// Macroblock Type & Sub-MB Type Constants
// ============================================================================

pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000080;
pub const MB_TYPE_SKIP: u32 = 0x00000100;
pub const MB_TYPE_DIRECT: u32 = 0x00000200;

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

pub const I_SLICE: i32 = 0;
pub const P_SLICE: i32 = 1;

pub const LUMA_DC: i32 = 0;
pub const LUMA_AC: i32 = 1;
pub const LUMA_4x4: i32 = 2;
pub const CHROMA_DC: i32 = 3;
pub const CHROMA_AC: i32 = 4;

pub const CHROMA_DC_NC_OFFSET: i8 = 17;

pub const MAX_MACROBLOCK_SIZE_IN_BYTE: usize = 400;
pub const MAX_MACROBLOCK_SIZE_IN_BYTE_x2: usize = MAX_MACROBLOCK_SIZE_IN_BYTE << 1; // 800

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 0x40;

#[inline(always)]
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_INTRA4x4(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA4x4
}

#[inline(always)]
pub fn IS_INTRA16x16(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA16x16
}

#[inline(always)]
pub fn IS_Inter_8x8(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_8x8) != 0
}

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
pub fn wels_non_zero_count_average(nA: i8, nB: i8) -> i8 {
    let mut nC = (nA as i32) + (nB as i32) + 1;
    let shift = if nA != -1 && nB != -1 { 1 } else { 0 };
    nC >>= shift;
    let add = if nA == -1 && nB == -1 { 1 } else { 0 };
    nC += add;
    nC as i8
}

// ============================================================================
// Global Lookup Tables
// ============================================================================

pub const g_kuiIntra4x4CbpMap: [u32; 48] = [
    3, 29, 30, 17, 31, 18, 37, 8, 32, 38, 19, 9, 20, 10, 11, 2, // 15
    16, 33, 34, 21, 35, 22, 39, 4, 36, 40, 23, 5, 24, 6, 7, 1, // 31
    41, 42, 43, 25, 44, 26, 46, 12, 45, 47, 27, 13, 28, 14, 15, 0, // 47
];

pub const g_kuiInterCbpMap: [u32; 48] = [
    0, 2, 3, 7, 4, 8, 17, 13, 5, 18, 9, 14, 10, 15, 16, 11, // 15
    1, 32, 33, 36, 34, 37, 44, 40, 35, 45, 38, 41, 39, 42, 43, 19, // 31
    6, 24, 25, 20, 26, 21, 46, 28, 27, 47, 22, 29, 23, 30, 31, 12, // 47
];

pub const g_kiMapModeI16x16: [i8; 7] = [0, 1, 2, 3, 2, 2, 2];

pub const g_kiMapModeIntraChroma: [i8; 7] = [0, 1, 2, 3, 0, 0, 0];

pub const g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15, 16, 17, 20, 21, 18, 19, 22, 23,
];

pub const g_kuiCache48CountScan4Idx: [u8; 24] = [
    9, 10, 17, 18, 11, 12, 19, 20, 25, 26, 33, 34, 27, 28, 35, 36, 14, 15, 22, 23, 38, 39, 46,
    47,
];

pub const g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, 39,
    39, 39, 39,
];

pub const g_kuiGolombUELength: &[u32] = &[
    1, 3, 3, 5, 5, 5, 5, 7, 7, 7, 7, 7, 7, 7, 7, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    9, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 17,
];

pub const g_kuiEncNcMapTable: [u8; 18] = [
    0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4,
];

// ============================================================================
// Bitstream Writers
// ============================================================================

#[inline(always)]
pub unsafe fn BsWriteBits(pBitString: *mut SBitStringAux, mut iLen: i32, kuiValue: u32) -> i32 {
    let bs = &mut *pBitString;
    if iLen < bs.iLeftBits {
        bs.uiCurBits = (bs.uiCurBits << iLen) | kuiValue;
        bs.iLeftBits -= iLen;
    } else {
        iLen -= bs.iLeftBits;
        bs.uiCurBits = (bs.uiCurBits << bs.iLeftBits) | (kuiValue >> iLen);
        let ptr = bs.pCurBuf;
        *ptr = (bs.uiCurBits >> 24) as u8;
        *ptr.add(1) = (bs.uiCurBits >> 16) as u8;
        *ptr.add(2) = (bs.uiCurBits >> 8) as u8;
        *ptr.add(3) = bs.uiCurBits as u8;
        bs.pCurBuf = bs.pCurBuf.add(4);
        bs.uiCurBits = kuiValue & ((1u32 << iLen).wrapping_sub(1));
        bs.iLeftBits = 32 - iLen;
    }
    0
}

#[inline(always)]
pub unsafe fn BsWriteOneBit(pBitString: *mut SBitStringAux, kuiValue: u32) -> i32 {
    BsWriteBits(pBitString, 1, kuiValue)
}

#[inline(always)]
pub unsafe fn BsWriteUE(pBitString: *mut SBitStringAux, kuiValue: u32) -> i32 {
    let iTmpValue = kuiValue + 1;
    if kuiValue < 256 {
        BsWriteBits(
            pBitString,
            g_kuiGolombUELength[kuiValue as usize] as i32,
            kuiValue + 1,
        );
    } else {
        let mut n = 0u32;
        let mut tmp = iTmpValue;
        if (tmp & 0xffff0000) != 0 {
            tmp >>= 16;
            n += 16;
        }
        if (tmp & 0xff00) != 0 {
            tmp >>= 8;
            n += 8;
        }
        n += g_kuiGolombUELength[(tmp - 1) as usize] >> 1;
        BsWriteBits(pBitString, ((n << 1) + 1) as i32, kuiValue + 1);
    }
    0
}

#[inline(always)]
pub unsafe fn BsWriteSE(pBitString: *mut SBitStringAux, kiValue: i32) -> i32 {
    if kiValue == 0 {
        BsWriteOneBit(pBitString, 1);
    } else if kiValue > 0 {
        let iTmpValue = ((kiValue as u32) << 1) - 1;
        BsWriteUE(pBitString, iTmpValue);
    } else {
        let iTmpValue = ((-kiValue) as u32) << 1;
        BsWriteUE(pBitString, iTmpValue);
    }
    0
}

#[inline(always)]
pub unsafe fn BsWriteTE(pBitString: *mut SBitStringAux, kiX: i32, kuiValue: u32) {
    if kiX == 1 {
        BsWriteOneBit(pBitString, if kuiValue == 0 { 1 } else { 0 });
    } else {
        BsWriteUE(pBitString, kuiValue);
    }
}

// ============================================================================
// Core C-compatible Data Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagMVComponentUnit {
    pub sMotionVectorCache: [SMVUnitXY; 29],
    pub iRefIndexCache: [i8; 30],
}

impl Default for TagMVComponentUnit {
    fn default() -> Self {
        Self {
            sMotionVectorCache: [SMVUnitXY::default(); 29],
            iRefIndexCache: [0; 30],
        }
    }
}

#[repr(C)]
pub struct SMbCache {
    pub sMvComponents: TagMVComponentUnit,
    pub iNonZeroCoeffCount: [i8; 48],
    pub iIntraPredMode: [i8; 48],
    pub iSadCost: [i32; 4],
    pub sMbMvp: [SMVUnitXY; 16],
    pub pCoeffLevel: *mut i16,
    pub pSkipMb: *mut u8,
    pub pMemPredMb: *mut u8,
    pub pMemPredLuma: *mut u8,
    pub pMemPredChroma: *mut u8,
    pub pBestPredIntraChroma: *mut u8,
    pub pMemPredBlk4: *mut u8,
    pub pBestPredI4x4Blk4: *mut u8,
    pub pBufferInterPredMe: *mut u8,
    pub pPrevIntra4x4PredModeFlag: *mut bool,
    pub pRemIntra4x4PredModeFlag: *mut i8,
    pub iSadCostSkip: [i32; 4],
    pub bMbTypeSkip: [bool; 4],
    pub pEncSad: *mut i32,
    pub pDct: *mut SDCTCoeff,
    pub uiNeighborIntra: u8,
    pub uiLumaI16x16Mode: u8,
    pub uiChmaI8x8Mode: u8,
    pub bCollocatedPredFlag: bool,
    pub uiRefMbType: u32,
}

impl Default for SMbCache {
    fn default() -> Self {
        Self {
            sMvComponents: TagMVComponentUnit::default(),
            iNonZeroCoeffCount: [0; 48],
            iIntraPredMode: [0; 48],
            iSadCost: [0; 4],
            sMbMvp: [SMVUnitXY::default(); 16],
            pCoeffLevel: std::ptr::null_mut(),
            pSkipMb: std::ptr::null_mut(),
            pMemPredMb: std::ptr::null_mut(),
            pMemPredLuma: std::ptr::null_mut(),
            pMemPredChroma: std::ptr::null_mut(),
            pBestPredIntraChroma: std::ptr::null_mut(),
            pMemPredBlk4: std::ptr::null_mut(),
            pBestPredI4x4Blk4: std::ptr::null_mut(),
            pBufferInterPredMe: std::ptr::null_mut(),
            pPrevIntra4x4PredModeFlag: std::ptr::null_mut(),
            pRemIntra4x4PredModeFlag: std::ptr::null_mut(),
            iSadCostSkip: [0; 4],
            bMbTypeSkip: [false; 4],
            pEncSad: std::ptr::null_mut(),
            pDct: std::ptr::null_mut(),
            uiNeighborIntra: 0,
            uiLumaI16x16Mode: 0,
            uiChmaI8x8Mode: 0,
            bCollocatedPredFlag: false,
            uiRefMbType: 0,
        }
    }
}

#[repr(C)]
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
#[derive(Debug, Copy, Clone, Default)]
pub struct SSliceHeader {
    pub eSliceType: i32,
    pub uiNumRefIdxL0Active: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
}

#[repr(C)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
    pub pSliceBsa: *mut SBitStringAux,
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub iSliceIdx: i32,
    pub uiBufferIdx: u32,
    pub bSliceHeaderExtFlag: bool,
    pub uiLastMbQp: u8,
    pub iMbSkipRun: i32,
}

impl Default for SSlice {
    fn default() -> Self {
        Self {
            sMbCacheInfo: SMbCache::default(),
            pSliceBsa: std::ptr::null_mut(),
            sSliceHeaderExt: SSliceHeaderExt::default(),
            iSliceIdx: 0,
            uiBufferIdx: 0,
            bSliceHeaderExtFlag: false,
            uiLastMbQp: 0,
            iMbSkipRun: 0,
        }
    }
}

#[repr(C)]
pub struct SWelsFuncPtrList {
    pub pfCavlcParamCal:
        Option<unsafe extern "C" fn(*mut i16, *mut u8, *mut i16, *mut i32, i32) -> i32>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPps {
    pub uiChromaQpIndexOffset: u8,
}

#[repr(C)]
pub struct SDqLayerInfo {
    pub pPpsP: *mut SPps,
}

#[repr(C)]
pub struct SDqLayer {
    pub sLayerInfo: SDqLayerInfo,
}

#[repr(C)]
pub struct sWelsEncCtx {
    pub pFuncList: *mut SWelsFuncPtrList,
    pub pCurDqLayer: *mut SDqLayer,
    pub eSliceType: i32,
}

// ============================================================================
// CAVLC Parameter Calculation and Residual Writing
// ============================================================================

/// Calculates non-zero count, level, run, and total zero statistics for CAVLC transform blocks.
pub unsafe extern "C" fn CavlcParamCal_c(
    pCoffLevel: *mut i16,
    pRun: *mut u8,
    pLevel: *mut i16,
    pTotalCoeff: *mut i32,
    mut iLastIndex: i32,
) -> i32 {
    let mut iTotalZeros = 0i32;
    let mut iTotalCoeffs = 0i32;

    while iLastIndex >= 0 && *pCoffLevel.add(iLastIndex as usize) == 0 {
        iLastIndex -= 1;
    }

    while iLastIndex >= 0 {
        let mut iCountZero = 0u8;
        *pLevel.add(iTotalCoeffs as usize) = *pCoffLevel.add(iLastIndex as usize);
        iLastIndex -= 1;

        while iLastIndex >= 0 && *pCoffLevel.add(iLastIndex as usize) == 0 {
            iCountZero += 1;
            iLastIndex -= 1;
        }
        iTotalZeros += iCountZero as i32;
        *pRun.add(iTotalCoeffs as usize) = iCountZero;
        iTotalCoeffs += 1;
    }
    *pTotalCoeff = iTotalCoeffs;
    iTotalZeros
}

/// Serializes transform coefficient block residuals into the CAVLC bitstream.
pub unsafe fn WriteBlockResidualCavlc(
    pFuncList: *mut SWelsFuncPtrList,
    pCoffLevel: *mut i16,
    iEndIdx: i32,
    iCalRunLevelFlag: i32,
    iResidualProperty: i32,
    iNC: i8,
    pBs: *mut SBitStringAux,
) -> i32 {
    let mut iLevel = [0i16; 16];
    let mut uiRun = [0u8; 16];

    let mut iTotalCoeffs = 0i32;
    let mut iTrailingOnes = 0i32;
    let mut iTotalZeros = 0i32;
    let mut uiSign = 0u32;

    if iCalRunLevelFlag != 0 {
        let func = if !pFuncList.is_null() && (*pFuncList).pfCavlcParamCal.is_some() {
            (*pFuncList).pfCavlcParamCal.unwrap()
        } else {
            CavlcParamCal_c
        };

        iTotalZeros = func(
            pCoffLevel,
            uiRun.as_mut_ptr(),
            iLevel.as_mut_ptr(),
            &mut iTotalCoeffs,
            iEndIdx,
        );

        let iCount = if iTotalCoeffs > 3 { 3 } else { iTotalCoeffs };
        for i in 0..iCount {
            if iLevel[i as usize].abs() == 1 {
                iTrailingOnes += 1;
                uiSign <<= 1;
                if iLevel[i as usize] < 0 {
                    uiSign |= 1;
                }
            } else {
                break;
            }
        }
    }

    let nc_idx = g_kuiEncNcMapTable[iNC.clamp(0, 17) as usize] as usize;
    let total_coeffs_idx = (iTotalCoeffs as usize).min(16);
    let trailing_ones_idx = (iTrailingOnes as usize).min(3);

    // Coeff token
    let upCoeffToken = crate::encoder::vlc_encoder::g_kuiVlcCoeffToken[nc_idx][total_coeffs_idx][trailing_ones_idx];
    let mut iValue = upCoeffToken[0] as u32;
    let mut n = upCoeffToken[1] as i32;

    if iTotalCoeffs == 0 {
        BsWriteBits(pBs, n, iValue);
        return ENC_RETURN_SUCCESS;
    }

    // Trailing ones sign bits
    n += iTrailingOnes;
    iValue = (iValue << iTrailingOnes) + uiSign;
    BsWriteBits(pBs, n, iValue);

    // Levels
    let mut uiSuffixLength = if iTotalCoeffs > 10 && iTrailingOnes < 3 {
        1i32
    } else {
        0i32
    };

    for i in iTrailingOnes..iTotalCoeffs {
        let iVal = iLevel[i as usize] as i32;
        let mut iLevelCode = (iVal - 1) * 2;
        let sign = (iLevelCode >> 31) as u32;
        iLevelCode = (iLevelCode ^ (sign as i32)) + ((sign as i32) << 1);
        if i == iTrailingOnes && iTrailingOnes < 3 {
            iLevelCode -= 2;
        }

        let mut iLevelPrefix = iLevelCode >> uiSuffixLength;
        let mut iLevelSuffixSize = uiSuffixLength;
        let mut iLevelSuffix = iLevelCode - (iLevelPrefix << uiSuffixLength);

        if iLevelPrefix >= 14 && iLevelPrefix < 30 && uiSuffixLength == 0 {
            iLevelPrefix = 14;
            iLevelSuffix = iLevelCode - iLevelPrefix;
            iLevelSuffixSize = 4;
        } else if iLevelPrefix >= 15 {
            iLevelPrefix = 15;
            iLevelSuffix = iLevelCode - (iLevelPrefix << uiSuffixLength);
            if (iLevelSuffix >> 11) != 0 {
                return ENC_RETURN_VLCOVERFLOWFOUND;
            }
            if uiSuffixLength == 0 {
                iLevelSuffix -= 15;
            }
            iLevelSuffixSize = 12;
        }

        n = iLevelPrefix + 1 + iLevelSuffixSize;
        iValue = (1u32 << iLevelSuffixSize) | (iLevelSuffix as u32);
        BsWriteBits(pBs, n, iValue);

        if uiSuffixLength == 0 {
            uiSuffixLength += 1;
        }
        let iThreshold = 3 << (uiSuffixLength - 1);
        if (iVal > iThreshold || iVal < -iThreshold) && uiSuffixLength < 6 {
            uiSuffixLength += 1;
        }
    }

    // Total zeros
    if iTotalCoeffs < iEndIdx + 1 {
        if CHROMA_DC != iResidualProperty {
            let upTotalZeros = crate::encoder::vlc_encoder::g_kuiVlcTotalZeros[(iTotalCoeffs as usize).min(15)][(iTotalZeros as usize).min(15)];
            n = upTotalZeros[1] as i32;
            iValue = upTotalZeros[0] as u32;
            BsWriteBits(pBs, n, iValue);
        } else {
            let upTotalZeros = crate::encoder::vlc_encoder::g_kuiVlcTotalZerosChromaDc[(iTotalCoeffs as usize).min(3)][(iTotalZeros as usize).min(3)];
            n = upTotalZeros[1] as i32;
            iValue = upTotalZeros[0] as u32;
            BsWriteBits(pBs, n, iValue);
        }
    }

    // Run before
    let mut iZerosLeft = iTotalZeros;
    let mut i = 0i32;
    while i + 1 < iTotalCoeffs && iZerosLeft > 0 {
        let uirun = uiRun[i as usize] as usize;
        let iZeroLeft = (iZerosLeft.min(7) - 1).max(0) as usize;
        let upRunBefore = crate::encoder::vlc_encoder::g_kuiVlcRunBefore[iZeroLeft][uirun.min(14)];
        n = upRunBefore[1] as i32;
        iValue = upRunBefore[0] as u32;
        BsWriteBits(pBs, n, iValue);
        iZerosLeft -= uirun as i32;
        i += 1;
    }

    ENC_RETURN_SUCCESS
}

// ============================================================================
// Core CAVLC Macroblock Serialization Functions
// ============================================================================

/// Encodes macroblock prediction headers (macroblock type, intra modes, MVDs) for CAVLC.
///
/// Matches `void WelsSpatialWriteMbPred (sWelsEncCtx* pEncCtx, SSlice* pSlice, SMB* pCurMb)`
pub unsafe fn WelsSpatialWriteMbPred(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) {
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pBs = (*pSlice).pSliceBsa;
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let iNumRefIdxl0ActiveMinus1 = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;

    let uiMbType = (*pCurMb).uiMbType;
    let iCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
    let iCbpLuma = ((*pCurMb).uiCbp & 15) as i32;

    let iMbOffset = match pSliceHeadExt.sSliceHeader.eSliceType {
        I_SLICE => 0,
        P_SLICE => 5,
        _ => return,
    };

    let mut sMvd: [SMVUnitXY; 2] = [SMVUnitXY::default(), SMVUnitXY::default()];

    match uiMbType {
        MB_TYPE_INTRA4x4 => {
            BsWriteUE(pBs, (iMbOffset + 0) as u32);

            let mut pPredFlag = pMbCache.pPrevIntra4x4PredModeFlag;
            let mut pRemMode = pMbCache.pRemIntra4x4PredModeFlag;
            for _ in 0..16 {
                let flag = *pPredFlag;
                BsWriteOneBit(pBs, if flag { 1 } else { 0 });
                if !flag {
                    BsWriteBits(pBs, 3, *pRemMode as u32);
                }
                pPredFlag = pPredFlag.add(1);
                pRemMode = pRemMode.add(1);
            }

            BsWriteUE(
                pBs,
                g_kiMapModeIntraChroma[pMbCache.uiChmaI8x8Mode as usize] as u32,
            );
        }

        MB_TYPE_INTRA16x16 => {
            let val = 1
                + iMbOffset
                + (g_kiMapModeI16x16[pMbCache.uiLumaI16x16Mode as usize] as i32)
                + (iCbpChroma << 2)
                + (if iCbpLuma == 0 { 0 } else { 12 });
            BsWriteUE(pBs, val as u32);

            BsWriteUE(
                pBs,
                g_kiMapModeIntraChroma[pMbCache.uiChmaI8x8Mode as usize] as u32,
            );
        }

        MB_TYPE_16x16 => {
            BsWriteUE(pBs, 0);
            sMvd[0].sDeltaMv(*(*pCurMb).sMv.add(0), pMbCache.sMbMvp[0]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(
                    pBs,
                    iNumRefIdxl0ActiveMinus1,
                    *(*pCurMb).pRefIndex.add(0) as u32,
                );
            }

            BsWriteSE(pBs, sMvd[0].iMvX as i32);
            BsWriteSE(pBs, sMvd[0].iMvY as i32);
        }

        MB_TYPE_16x8 => {
            BsWriteUE(pBs, 1);

            sMvd[0].sDeltaMv(*(*pCurMb).sMv.add(0), pMbCache.sMbMvp[0]);
            sMvd[1].sDeltaMv(*(*pCurMb).sMv.add(8), pMbCache.sMbMvp[1]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(
                    pBs,
                    iNumRefIdxl0ActiveMinus1,
                    *(*pCurMb).pRefIndex.add(0) as u32,
                );
                BsWriteTE(
                    pBs,
                    iNumRefIdxl0ActiveMinus1,
                    *(*pCurMb).pRefIndex.add(2) as u32,
                );
            }
            BsWriteSE(pBs, sMvd[0].iMvX as i32);
            BsWriteSE(pBs, sMvd[0].iMvY as i32);
            BsWriteSE(pBs, sMvd[1].iMvX as i32);
            BsWriteSE(pBs, sMvd[1].iMvY as i32);
        }

        MB_TYPE_8x16 => {
            BsWriteUE(pBs, 2);

            sMvd[0].sDeltaMv(*(*pCurMb).sMv.add(0), pMbCache.sMbMvp[0]);
            sMvd[1].sDeltaMv(*(*pCurMb).sMv.add(2), pMbCache.sMbMvp[1]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(
                    pBs,
                    iNumRefIdxl0ActiveMinus1,
                    *(*pCurMb).pRefIndex.add(0) as u32,
                );
                BsWriteTE(
                    pBs,
                    iNumRefIdxl0ActiveMinus1,
                    *(*pCurMb).pRefIndex.add(1) as u32,
                );
            }
            BsWriteSE(pBs, sMvd[0].iMvX as i32);
            BsWriteSE(pBs, sMvd[0].iMvY as i32);
            BsWriteSE(pBs, sMvd[1].iMvX as i32);
            BsWriteSE(pBs, sMvd[1].iMvY as i32);
        }

        _ => {}
    }
}

/// Encodes 8x8 sub-macroblock prediction headers for CAVLC.
///
/// Matches `void WelsSpatialWriteSubMbPred (sWelsEncCtx* pEncCtx, SSlice* pSlice, SMB* pCurMb)`
pub unsafe fn WelsSpatialWriteSubMbPred(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) {
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pBs = (*pSlice).pSliceBsa;
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;

    let iNumRefIdxl0ActiveMinus1 = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;

    let bSubRef0: bool;
    let mut kpScan4_idx = 0usize;

    let ref_idx_u32 = std::ptr::read_unaligned((*pCurMb).pRefIndex as *const u32);
    if ref_idx_u32 == 0 {
        BsWriteUE(pBs, 4);
        bSubRef0 = false;
    } else {
        BsWriteUE(pBs, 3);
        bSubRef0 = true;
    }

    // Step 1: sub_mb_type
    for i in 0..4 {
        match (*pCurMb).uiSubMbType[i] as u32 {
            SUB_MB_TYPE_8x8 => {
                BsWriteUE(pBs, 0);
            }
            SUB_MB_TYPE_8x4 => {
                BsWriteUE(pBs, 1);
            }
            SUB_MB_TYPE_4x8 => {
                BsWriteUE(pBs, 2);
            }
            SUB_MB_TYPE_4x4 => {
                BsWriteUE(pBs, 3);
            }
            _ => {}
        }
    }

    // Step 2: get and write uiRefIndex and sMvd
    if iNumRefIdxl0ActiveMinus1 > 0 && bSubRef0 {
        BsWriteTE(
            pBs,
            iNumRefIdxl0ActiveMinus1,
            *(*pCurMb).pRefIndex.add(0) as u32,
        );
        BsWriteTE(
            pBs,
            iNumRefIdxl0ActiveMinus1,
            *(*pCurMb).pRefIndex.add(1) as u32,
        );
        BsWriteTE(
            pBs,
            iNumRefIdxl0ActiveMinus1,
            *(*pCurMb).pRefIndex.add(2) as u32,
        );
        BsWriteTE(
            pBs,
            iNumRefIdxl0ActiveMinus1,
            *(*pCurMb).pRefIndex.add(3) as u32,
        );
    }

    // Write sMvd
    for i in 0..4 {
        let uiSubMbType = (*pCurMb).uiSubMbType[i] as u32;
        let s0 = g_kuiMbCountScan4Idx[kpScan4_idx] as usize;
        let s1 = g_kuiMbCountScan4Idx[kpScan4_idx + 1] as usize;
        let s2 = g_kuiMbCountScan4Idx[kpScan4_idx + 2] as usize;
        let s3 = g_kuiMbCountScan4Idx[kpScan4_idx + 3] as usize;

        let cur_mv = (*pCurMb).sMv;

        if SUB_MB_TYPE_8x8 == uiSubMbType {
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvX - pMbCache.sMbMvp[s0].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvY - pMbCache.sMbMvp[s0].iMvY) as i32,
            );
        } else if SUB_MB_TYPE_4x4 == uiSubMbType {
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvX - pMbCache.sMbMvp[s0].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvY - pMbCache.sMbMvp[s0].iMvY) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s1)).iMvX - pMbCache.sMbMvp[s1].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s1)).iMvY - pMbCache.sMbMvp[s1].iMvY) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s2)).iMvX - pMbCache.sMbMvp[s2].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s2)).iMvY - pMbCache.sMbMvp[s2].iMvY) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s3)).iMvX - pMbCache.sMbMvp[s3].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s3)).iMvY - pMbCache.sMbMvp[s3].iMvY) as i32,
            );
        } else if SUB_MB_TYPE_8x4 == uiSubMbType {
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvX - pMbCache.sMbMvp[s0].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvY - pMbCache.sMbMvp[s0].iMvY) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s2)).iMvX - pMbCache.sMbMvp[s2].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s2)).iMvY - pMbCache.sMbMvp[s2].iMvY) as i32,
            );
        } else if SUB_MB_TYPE_4x8 == uiSubMbType {
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvX - pMbCache.sMbMvp[s0].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s0)).iMvY - pMbCache.sMbMvp[s0].iMvY) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s1)).iMvX - pMbCache.sMbMvp[s1].iMvX) as i32,
            );
            BsWriteSE(
                pBs,
                ((*cur_mv.add(s1)).iMvY - pMbCache.sMbMvp[s1].iMvY) as i32,
            );
        }
        kpScan4_idx += 4;
    }
}

/// Checks bitstream buffer safety threshold.
///
/// Matches `int32_t CheckBitstreamBuffer (const uint32_t kuiSliceIdx, sWelsEncCtx* pEncCtx, SBitStringAux* pBs)`
pub unsafe fn CheckBitstreamBuffer(
    _kuiSliceIdx: u32,
    _pEncCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
) -> i32 {
    let bs = &*pBs;
    let iLeftLength = (bs.pEndBuf as isize) - (bs.pCurBuf as isize) - 1;
    debug_assert!(iLeftLength > 0);

    if iLeftLength < MAX_MACROBLOCK_SIZE_IN_BYTE_x2 as isize {
        return ENC_RETURN_VLCOVERFLOWFOUND;
    }
    ENC_RETURN_SUCCESS
}

/// Top-level macroblock CAVLC bitstream serialization function.
///
/// Matches `int32_t WelsSpatialWriteMbSyn (sWelsEncCtx* pEncCtx, SSlice* pSlice, SMB* pCurMb)`
pub unsafe fn WelsSpatialWriteMbSyn(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) -> i32 {
    let pBs = (*pSlice).pSliceBsa;
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let kuiChromaQpIndexOffset = (*(*(*pEncCtx).pCurDqLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset;

    if IS_SKIP((*pCurMb).uiMbType) {
        (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
        let idx = CLIP3_QP_0_51(((*pCurMb).uiLumaQp as i32) + (kuiChromaQpIndexOffset as i32));
        (*pCurMb).uiChromaQp = g_kuiChromaQpTable[idx as usize];

        (*pSlice).iMbSkipRun += 1;
        ENC_RETURN_SUCCESS
    } else {
        if (*pEncCtx).eSliceType != I_SLICE {
            BsWriteUE(pBs, (*pSlice).iMbSkipRun as u32);
            (*pSlice).iMbSkipRun = 0;
        }

        // Step 1: write mb type and pred
        if IS_Inter_8x8((*pCurMb).uiMbType) {
            WelsSpatialWriteSubMbPred(pEncCtx, pSlice, pCurMb);
        } else {
            WelsSpatialWriteMbPred(pEncCtx, pSlice, pCurMb);
        }

        // Step 2: write coded block pattern
        if IS_INTRA4x4((*pCurMb).uiMbType) {
            BsWriteUE(pBs, g_kuiIntra4x4CbpMap[(*pCurMb).uiCbp as usize]);
        } else if !IS_INTRA16x16((*pCurMb).uiMbType) {
            BsWriteUE(pBs, g_kuiInterCbpMap[(*pCurMb).uiCbp as usize]);
        }

        // Step 3: write QP and residual
        if (*pCurMb).uiCbp > 0 || IS_INTRA16x16((*pCurMb).uiMbType) {
            let kiDeltaQp = ((*pCurMb).uiLumaQp as i32) - ((*pSlice).uiLastMbQp as i32);
            (*pSlice).uiLastMbQp = (*pCurMb).uiLumaQp;

            BsWriteSE(pBs, kiDeltaQp);
            if WelsWriteMbResidual((*pEncCtx).pFuncList, pMbCache, pCurMb, pBs) != 0 {
                return ENC_RETURN_VLCOVERFLOWFOUND;
            }
        } else {
            (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
            let idx = CLIP3_QP_0_51(
                ((*pCurMb).uiLumaQp as i32)
                    + ((*(*(*pEncCtx).pCurDqLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset as i32),
            );
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[idx as usize];
        }

        // Step 4: Check the left buffer
        CheckBitstreamBuffer((*pSlice).iSliceIdx as u32, pEncCtx, pBs)
    }
}

/// Serializes all macroblock quantized transform coefficient residuals using CAVLC.
///
/// Matches `int32_t WelsWriteMbResidual (SWelsFuncPtrList* pFuncList, SMbCache* sMbCacheInfo, SMB* pCurMb, SBitStringAux* pBs)`
pub unsafe fn WelsWriteMbResidual(
    pFuncList: *mut SWelsFuncPtrList,
    sMbCacheInfo: *mut SMbCache,
    pCurMb: *mut SMB,
    pBs: *mut SBitStringAux,
) -> i32 {
    let uiMbType = (*pCurMb).uiMbType;
    let kiCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
    let kiCbpLuma = ((*pCurMb).uiCbp & 0x0F) as i32;
    let pNonZeroCoeffCount = (*sMbCacheInfo).iNonZeroCoeffCount.as_mut_ptr();

    let mut pBlock: *mut i16;
    let mut iA: i8;
    let mut iB: i8;
    let mut iC: i8;

    if IS_INTRA16x16(uiMbType) {
        // DC luma
        iA = *pNonZeroCoeffCount.add(8);
        iB = *pNonZeroCoeffCount.add(1);
        iC = wels_non_zero_count_average(iA, iB);
        if WriteBlockResidualCavlc(
            pFuncList,
            (*(*sMbCacheInfo).pDct).iLumaI16x16Dc.as_mut_ptr(),
            15,
            1,
            LUMA_4x4,
            iC,
            pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        // AC Luma
        if kiCbpLuma != 0 {
            pBlock = (*(*sMbCacheInfo).pDct).iLumaBlock[0].as_mut_ptr();

            for i in 0..16 {
                let iIdx = g_kuiCache48CountScan4Idx[i] as usize;
                iA = *pNonZeroCoeffCount.add(iIdx - 1);
                iB = *pNonZeroCoeffCount.add(iIdx - 8);
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    pBlock,
                    14,
                    if *pNonZeroCoeffCount.add(iIdx) > 0 { 1 } else { 0 },
                    LUMA_AC,
                    iC,
                    pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
                pBlock = pBlock.add(16);
            }
        }
    } else {
        // Luma DC AC
        if kiCbpLuma != 0 {
            pBlock = (*(*sMbCacheInfo).pDct).iLumaBlock[0].as_mut_ptr();

            let mut i = 0usize;
            while i < 16 {
                if (kiCbpLuma & (1 << (i >> 2))) != 0 {
                    let iIdx = g_kuiCache48CountScan4Idx[i] as usize;
                    let kiA = *pNonZeroCoeffCount.add(iIdx);
                    let kiB = *pNonZeroCoeffCount.add(iIdx + 1);
                    let kiC_val = *pNonZeroCoeffCount.add(iIdx + 8);
                    let kiD = *pNonZeroCoeffCount.add(iIdx + 9);

                    iA = *pNonZeroCoeffCount.add(iIdx - 1);
                    iB = *pNonZeroCoeffCount.add(iIdx - 8);
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        pBlock,
                        15,
                        if kiA > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = kiA;
                    iB = *pNonZeroCoeffCount.add(iIdx - 7);
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        pBlock.add(16),
                        15,
                        if kiB > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = *pNonZeroCoeffCount.add(iIdx + 7);
                    iB = kiA;
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        pBlock.add(32),
                        15,
                        if kiC_val > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = kiC_val;
                    iB = kiB;
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        pBlock.add(48),
                        15,
                        if kiD > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }
                }
                pBlock = pBlock.add(64);
                i += 4;
            }
        }
    }

    if kiCbpChroma != 0 {
        // Chroma DC residual present
        pBlock = (*(*sMbCacheInfo).pDct).iChromaDc[0].as_mut_ptr(); // Cb
        if WriteBlockResidualCavlc(
            pFuncList,
            pBlock,
            3,
            1,
            CHROMA_DC,
            CHROMA_DC_NC_OFFSET,
            pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        pBlock = pBlock.add(4); // Cr
        if WriteBlockResidualCavlc(
            pFuncList,
            pBlock,
            3,
            1,
            CHROMA_DC,
            CHROMA_DC_NC_OFFSET,
            pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        // Chroma AC residual present
        if (kiCbpChroma & 0x02) != 0 {
            let kCache48CountScan4Idx16base = &g_kuiCache48CountScan4Idx[16..];
            pBlock = (*(*sMbCacheInfo).pDct).iChromaBlock[0].as_mut_ptr(); // Cb

            for i in 0..4 {
                let iIdx = kCache48CountScan4Idx16base[i] as usize;
                iA = *pNonZeroCoeffCount.add(iIdx - 1);
                iB = *pNonZeroCoeffCount.add(iIdx - 8);
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    pBlock,
                    14,
                    if *pNonZeroCoeffCount.add(iIdx) > 0 { 1 } else { 0 },
                    CHROMA_AC,
                    iC,
                    pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
                pBlock = pBlock.add(16);
            }

            pBlock = (*(*sMbCacheInfo).pDct).iChromaBlock[4].as_mut_ptr(); // Cr

            for i in 0..4 {
                let iIdx = 24 + (kCache48CountScan4Idx16base[i] as usize);
                iA = *pNonZeroCoeffCount.add(iIdx - 1);
                iB = *pNonZeroCoeffCount.add(iIdx - 8);
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    pBlock,
                    14,
                    if *pNonZeroCoeffCount.add(iIdx) > 0 { 1 } else { 0 },
                    CHROMA_AC,
                    iC,
                    pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
                pBlock = pBlock.add(16);
            }
        }
    }
    0
}

pub unsafe extern "C" fn StashMBStatusCavlc(
    pDss: *mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
    pSlice: *mut SSlice,
    iMbSkipRun: i32,
) {
    if pDss.is_null() || pSlice.is_null() {
        return;
    }
    let pBs = (*pSlice).pSliceBsa;
    if !pBs.is_null() {
        (*pDss).pBsStackBufPtr = (*pBs).pCurBuf;
        (*pDss).uiBsStackCurBits = (*pBs).uiCurBits;
        (*pDss).iBsStackLeftBits = (*pBs).iLeftBits;
    }
    (*pDss).uiLastMbQp = (*pSlice).uiLastMbQp as u8;
    (*pDss).iMbSkipRunStack = iMbSkipRun;
}

pub unsafe extern "C" fn StashPopMBStatusCavlc(
    pDss: *mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
    pSlice: *mut SSlice,
) -> i32 {
    if pDss.is_null() || pSlice.is_null() {
        return 0;
    }
    let pBs = (*pSlice).pSliceBsa;
    if !pBs.is_null() {
        (*pBs).pCurBuf = (*pDss).pBsStackBufPtr;
        (*pBs).uiCurBits = (*pDss).uiBsStackCurBits;
        (*pBs).iLeftBits = (*pDss).iBsStackLeftBits;
    }
    (*pSlice).uiLastMbQp = (*pDss).uiLastMbQp;
    (*pDss).iMbSkipRunStack
}

pub unsafe extern "C" fn GetBsPosCavlc(pSlice: *mut SSlice) -> i32 {
    if pSlice.is_null() || (*pSlice).pSliceBsa.is_null() {
        return 0;
    }
    crate::encoder::vlc_encoder::BsGetBitsPos(
        (*pSlice).pSliceBsa as *const crate::encoder::vlc_encoder::SBitStringAux
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cbp_lookup_tables() {
        assert_eq!(g_kuiIntra4x4CbpMap[0], 3);
        assert_eq!(g_kuiIntra4x4CbpMap[47], 0);
        assert_eq!(g_kuiInterCbpMap[0], 0);
        assert_eq!(g_kuiInterCbpMap[47], 12);
    }

    #[test]
    fn test_non_zero_count_average() {
        assert_eq!(wels_non_zero_count_average(2, 4), 3);
        assert_eq!(wels_non_zero_count_average(2, -1), 2);
        assert_eq!(wels_non_zero_count_average(-1, 4), 4);
        assert_eq!(wels_non_zero_count_average(-1, -1), 0);
    }

    #[test]
    fn test_clip3_qp() {
        assert_eq!(CLIP3_QP_0_51(-5), 0);
        assert_eq!(CLIP3_QP_0_51(26), 26);
        assert_eq!(CLIP3_QP_0_51(60), 51);
    }

    #[test]
    fn test_cavlc_param_cal() {
        let mut coeffs = [0i16; 16];
        coeffs[0] = 5;
        coeffs[2] = -1;
        coeffs[3] = 1;

        let mut run = [0u8; 16];
        let mut level = [0i16; 16];
        let mut total_coeffs = 0i32;

        unsafe {
            let total_zeros =
                CavlcParamCal_c(coeffs.as_mut_ptr(), run.as_mut_ptr(), level.as_mut_ptr(), &mut total_coeffs, 15);
            assert_eq!(total_coeffs, 3);
            assert_eq!(total_zeros, 1);
        }
    }
}
