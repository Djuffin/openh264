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

#![deny(unsafe_code)]

use crate::safe::bits::BsWriter;
use crate::encoder::set_mb_syn_cabac::SCabacCtx;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::svc_encode_slice::SSliceHeader;
use crate::encoder::svc_encode_slice::layer_pps;
use crate::encoder::svc_encode_slice::current_layer;
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::md::SMB;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;

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
/// `wels_common_defs.h:286` says **0x00000800**, not 0x200 (0x200 is
/// `MB_TYPE_INTRA_PCM`). Nothing in this module reads it, so the wrong value was
/// dead. One definition now.
pub use crate::encoder::deblocking::MB_TYPE_DIRECT;

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;


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

// `g_kuiGolombUELength` is a common-layer table (`common_tables.cpp:886`).
// This module used to declare its own copy; see the canonical definition for
// what the divergent copies got wrong.
pub use crate::common::wels_common_defs::g_kuiGolombUELength;

// `g_kuiEncNcMapTable` is `encoder_data_tables.cpp`'s. This module used to declare
// a byte-identical second copy; one definition now, same as `g_kuiGolombUELength`
// above.
pub use crate::encoder::vlc_encoder::g_kuiEncNcMapTable;

// ============================================================================
// Bitstream Writers
// ============================================================================

// One writer family, `vlc_encoder.rs`'s, which is the transliteration of the C++
// `codec/common/inc/golomb_common.h`. This module used to declare its own copy of
// the five functions below — equivalent to the canonical one on every in-contract
// input, differing only in a hand-rolled four-byte store where the canonical calls
// `WRITE_BE_32`, and in `(1u32 << iLen).wrapping_sub(1)` where the canonical
// subtracts plainly (`1u32 << iLen` is at least 1 for the `iLen` in `0..=31` that
// reaches it, so the two cannot differ). See `phase0_findings.md` F2 for the
// four-copy inventory and the log's session-E entry for what the dedupe decided.
pub use crate::encoder::vlc_encoder::{
    BsWriteBits, BsWriteOneBit, BsWriteSE, BsWriteTE, BsWriteUE,
};

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












// ============================================================================
// CAVLC Parameter Calculation and Residual Writing
// ============================================================================

/// Calculates non-zero count, level, run, and total zero statistics for CAVLC transform blocks.
///
/// **S4.C1**: the four raw parameters became the shapes the callers already hold
/// — `pCoffLevel` the coefficient block as a slice (its length is the extent the
/// caller owns: 16 for luma, 4 for chroma DC), `pRun`/`pLevel` the caller's own
/// sixteen-element locals. The backward scan is unchanged; what changed is that
/// `iLastIndex` is now bounds-checked against the block rather than trusted, and
/// the two output writes index arrays instead of offsetting pointers.
///
/// `iLastIndex` is clamped to the block on entry rather than asserted: the C++
/// passes a compile-time constant (15 or 3) that always matches the block it
/// passes beside it, so a mismatch is unreachable from the port's own call
/// sites, and clamping keeps a future caller from reading past the slice.
pub fn CavlcParamCal_c(
    pCoffLevel: &[i16],
    pRun: &mut [u8; 16],
    pLevel: &mut [i16; 16],
    pTotalCoeff: &mut i32,
    iEndIdx: i32,
) -> i32 {
    let mut iTotalZeros = 0i32;
    let mut iTotalCoeffs = 0i32;
    let mut iLastIndex = iEndIdx.min(pCoffLevel.len() as i32 - 1);

    while iLastIndex >= 0 && pCoffLevel[iLastIndex as usize] == 0 {
        iLastIndex -= 1;
    }

    while iLastIndex >= 0 {
        let mut iCountZero = 0u8;
        pLevel[iTotalCoeffs as usize] = pCoffLevel[iLastIndex as usize];
        iLastIndex -= 1;

        while iLastIndex >= 0 && pCoffLevel[iLastIndex as usize] == 0 {
            iCountZero += 1;
            iLastIndex -= 1;
        }
        iTotalZeros += iCountZero as i32;
        pRun[iTotalCoeffs as usize] = iCountZero;
        iTotalCoeffs += 1;
    }
    *pTotalCoeff = iTotalCoeffs;
    iTotalZeros
}

/// Serializes transform coefficient block residuals into the CAVLC bitstream.
pub fn WriteBlockResidualCavlc(
    pFuncList: &SWelsFuncPtrList,
    pCoffLevel: &[i16],
    iEndIdx: i32,
    iCalRunLevelFlag: i32,
    iResidualProperty: i32,
    iNC: i8,
    buf: &mut [u8],
    pBs: &mut BsWriter,
) -> i32 {
    let mut iLevel = [0i16; 16];
    let mut uiRun = [0u8; 16];

    let mut iTotalCoeffs = 0i32;
    let mut iTrailingOnes = 0i32;
    let mut iTotalZeros = 0i32;
    let mut uiSign = 0u32;

    if iCalRunLevelFlag != 0 {
        let func = if pFuncList.pfCavlcParamCal.is_some() {
            pFuncList.pfCavlcParamCal.unwrap()
        } else {
            CavlcParamCal_c
        };

        iTotalZeros = func(
            pCoffLevel,
            &mut uiRun,
            &mut iLevel,
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
        BsWriteBits(buf, &mut *pBs, n, iValue);
        return ENC_RETURN_SUCCESS;
    }

    // Trailing ones sign bits
    n += iTrailingOnes;
    iValue = (iValue << iTrailingOnes) + uiSign;
    BsWriteBits(buf, &mut *pBs, n, iValue);

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
        BsWriteBits(buf, &mut *pBs, n, iValue);

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
            BsWriteBits(buf, &mut *pBs, n, iValue);
        } else {
            let upTotalZeros = crate::encoder::vlc_encoder::g_kuiVlcTotalZerosChromaDc[(iTotalCoeffs as usize).min(3)][(iTotalZeros as usize).min(3)];
            n = upTotalZeros[1] as i32;
            iValue = upTotalZeros[0] as u32;
            BsWriteBits(buf, &mut *pBs, n, iValue);
        }
    }

    // Run before
    let mut iZerosLeft = iTotalZeros;
    let mut i = 0i32;
    while i + 1 < iTotalCoeffs && iZerosLeft > 0 {
        let uirun = uiRun[i as usize] as usize;
        // `set_mb_syn_cavlc.cpp:223` — `g_kuiZeroLeftMap[iZerosLeft]`, i.e. saturate at
        // 7. This was `(iZerosLeft.min(7) - 1).max(0)`, an invented formula one row off:
        // it selected the run-before VLC for `zeros_left - 1` for every block with a
        // run to code.
        let iZeroLeft = crate::encoder::vlc_encoder::g_kuiZeroLeftMap[(iZerosLeft as usize).min(15)] as usize;
        let upRunBefore = crate::encoder::vlc_encoder::g_kuiVlcRunBefore[iZeroLeft][uirun.min(14)];
        n = upRunBefore[1] as i32;
        iValue = upRunBefore[0] as u32;
        BsWriteBits(buf, &mut *pBs, n, iValue);
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
pub fn WelsSpatialWriteMbPred(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pBs = crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs);
    let buf = pSliceBsBuf;
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let iNumRefIdxl0ActiveMinus1 = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;

    let uiMbType = (*pCurMb).uiMbType;
    let iCbpChroma = ((*pCurMb).uiCbp >> 4) as i32;
    let iCbpLuma = ((*pCurMb).uiCbp & 15) as i32;

    // svc_set_mb_syn_cavlc.cpp:76
    let iMbOffset = match pSliceHeadExt.sSliceHeader.eSliceType {
        EWelsSliceType::I_SLICE => 0,
        EWelsSliceType::P_SLICE => 5,
        _ => return,
    };

    let mut sMvd: [SMVUnitXY; 2] = [SMVUnitXY::default(), SMVUnitXY::default()];

    match uiMbType {
        MB_TYPE_INTRA4x4 => {
            BsWriteUE(buf, &mut *pBs, (iMbOffset + 0) as u32);

            for iMode in 0..crate::encoder::md::MB_BLOCK4x4_NUM {
                let flag = pMbCache.bPrevIntra4x4PredModeFlag[iMode];
                BsWriteOneBit(buf, &mut *pBs, if flag { 1 } else { 0 });
                if !flag {
                    BsWriteBits(buf, &mut *pBs, 3, pMbCache.iRemIntra4x4PredModeFlag[iMode] as u32);
                }
            }

            BsWriteUE(buf, &mut *pBs,
                g_kiMapModeIntraChroma[pMbCache.uiChmaI8x8Mode as usize] as u32,
            );
        }

        MB_TYPE_INTRA16x16 => {
            let val = 1
                + iMbOffset
                + (g_kiMapModeI16x16[pMbCache.uiLumaI16x16Mode as usize] as i32)
                + (iCbpChroma << 2)
                + (if iCbpLuma == 0 { 0 } else { 12 });
            BsWriteUE(buf, &mut *pBs, val as u32);

            BsWriteUE(buf, &mut *pBs,
                g_kiMapModeIntraChroma[pMbCache.uiChmaI8x8Mode as usize] as u32,
            );
        }

        MB_TYPE_16x16 => {
            BsWriteUE(buf, &mut *pBs, 0);
            sMvd[0].sDeltaMv((*pCurMb).sMv[0], pMbCache.sMbMvp[0]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(buf, &mut *pBs,
                    iNumRefIdxl0ActiveMinus1,
                    (*pCurMb).iRefIndex[0] as u32,
                );
            }

            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvX as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvY as i32);
        }

        MB_TYPE_16x8 => {
            BsWriteUE(buf, &mut *pBs, 1);

            sMvd[0].sDeltaMv((*pCurMb).sMv[0], pMbCache.sMbMvp[0]);
            sMvd[1].sDeltaMv((*pCurMb).sMv[8], pMbCache.sMbMvp[1]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(buf, &mut *pBs,
                    iNumRefIdxl0ActiveMinus1,
                    (*pCurMb).iRefIndex[0] as u32,
                );
                BsWriteTE(buf, &mut *pBs,
                    iNumRefIdxl0ActiveMinus1,
                    (*pCurMb).iRefIndex[2] as u32,
                );
            }
            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvX as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvY as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[1].iMvX as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[1].iMvY as i32);
        }

        MB_TYPE_8x16 => {
            BsWriteUE(buf, &mut *pBs, 2);

            sMvd[0].sDeltaMv((*pCurMb).sMv[0], pMbCache.sMbMvp[0]);
            sMvd[1].sDeltaMv((*pCurMb).sMv[2], pMbCache.sMbMvp[1]);

            if iNumRefIdxl0ActiveMinus1 > 0 {
                BsWriteTE(buf, &mut *pBs,
                    iNumRefIdxl0ActiveMinus1,
                    (*pCurMb).iRefIndex[0] as u32,
                );
                BsWriteTE(buf, &mut *pBs,
                    iNumRefIdxl0ActiveMinus1,
                    (*pCurMb).iRefIndex[1] as u32,
                );
            }
            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvX as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[0].iMvY as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[1].iMvX as i32);
            BsWriteSE(buf, &mut *pBs, sMvd[1].iMvY as i32);
        }

        _ => {}
    }
}

/// Encodes 8x8 sub-macroblock prediction headers for CAVLC.
///
/// Matches `void WelsSpatialWriteSubMbPred (sWelsEncCtx* pEncCtx, SSlice* pSlice, SMB* pCurMb)`
pub fn WelsSpatialWriteSubMbPred(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pBs = crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs);
    let buf = pSliceBsBuf;
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;

    let iNumRefIdxl0ActiveMinus1 = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;

    let bSubRef0: bool;
    let mut kpScan4_idx = 0usize;

    // `LD32 (pCurMb->pRefIndex)` — the four 8x8 reference indices as one word.
    let ref_idx_u32 = u32::from_ne_bytes((*pCurMb).iRefIndex.map(|r| r as u8));
    if ref_idx_u32 == 0 {
        BsWriteUE(buf, &mut *pBs, 4);
        bSubRef0 = false;
    } else {
        BsWriteUE(buf, &mut *pBs, 3);
        bSubRef0 = true;
    }

    // Step 1: sub_mb_type
    for i in 0..4 {
        match (*pCurMb).uiSubMbType[i] as u32 {
            SUB_MB_TYPE_8x8 => {
                BsWriteUE(buf, &mut *pBs, 0);
            }
            // D-dead-2 / F122: `sub_mb_type` 1/2/3 (`SUB_MB_TYPE_8x4`/`_4x8`/`_4x4`)
            // deleted. Every writer of `uiSubMbType` in this encoder sets
            // `SUB_MB_TYPE_8x8` (`svc_base_layer_md.rs:1164`/`:1249`/`:1262`,
            // `svc_mode_decision.rs:2495`), and upstream's only other writers are
            // inside `#if 0 //Disable for sub8x8 modes for now`
            // (`svc_mode_decision.cpp:634-661`). Loud rather than silent: emitting
            // nothing for an unexpected partition would desynchronise the whole
            // slice, which is a far worse failure than a panic.
            _ => unreachable!(
                "sub_mb_type {:#x} — the sub-8x8 search is #if 0 upstream and \
                 unwritten here (D-dead-2/F122)",
                (*pCurMb).uiSubMbType[i]
            ),
        }
    }

    // Step 2: get and write uiRefIndex and sMvd
    if iNumRefIdxl0ActiveMinus1 > 0 && bSubRef0 {
        BsWriteTE(buf, &mut *pBs,
            iNumRefIdxl0ActiveMinus1,
            (*pCurMb).iRefIndex[0] as u32,
        );
        BsWriteTE(buf, &mut *pBs,
            iNumRefIdxl0ActiveMinus1,
            (*pCurMb).iRefIndex[1] as u32,
        );
        BsWriteTE(buf, &mut *pBs,
            iNumRefIdxl0ActiveMinus1,
            (*pCurMb).iRefIndex[2] as u32,
        );
        BsWriteTE(buf, &mut *pBs,
            iNumRefIdxl0ActiveMinus1,
            (*pCurMb).iRefIndex[3] as u32,
        );
    }

    // Write sMvd
    for i in 0..4 {
        let uiSubMbType = (*pCurMb).uiSubMbType[i] as u32;
        let s0 = g_kuiMbCountScan4Idx[kpScan4_idx] as usize;
        let s1 = g_kuiMbCountScan4Idx[kpScan4_idx + 1] as usize;
        let s2 = g_kuiMbCountScan4Idx[kpScan4_idx + 2] as usize;
        let s3 = g_kuiMbCountScan4Idx[kpScan4_idx + 3] as usize;

        let cur_mv = &(*pCurMb).sMv;

        if SUB_MB_TYPE_8x8 == uiSubMbType {
            BsWriteSE(buf, &mut *pBs,
                (cur_mv[s0].iMvX - pMbCache.sMbMvp[s0].iMvX) as i32,
            );
            BsWriteSE(buf, &mut *pBs,
                (cur_mv[s0].iMvY - pMbCache.sMbMvp[s0].iMvY) as i32,
            );
        } else {
            // D-dead-2 / F122 — the `_4x4`/`_8x4`/`_4x8` motion-vector-difference
            // arms are gone with the sub-8x8 search that produced them. See the
            // `sub_mb_type` match above for the reachability argument.
            unreachable!(
                "sub_mb_type {:#x} — the sub-8x8 search is #if 0 upstream and \
                 unwritten here (D-dead-2/F122)",
                uiSubMbType
            );
        }
        kpScan4_idx += 4;
    }
}

/// Checks bitstream buffer safety threshold.
///
/// Matches `int32_t CheckBitstreamBuffer (const uint32_t kuiSliceIdx, sWelsEncCtx* pEncCtx, SBitStringAux* pBs)`
///
/// The C++ computes `iLeftLength = pEndBuf - pCurBuf - 1` as a signed pointer
/// difference and compares it twice. Both comparisons are kept in **comparison
/// form** against `pos`/`len` rather than restored as a subtraction: `len - pos - 1`
/// on `usize` wraps to a huge number exactly where the signed original goes
/// negative, which turns an overflow report into a silent pass. That has been the
/// live hazard twice already this phase (T3.2's ladder, T3.3's `size < 1` guard),
/// and it is the reason this reads the way it does.
///
///   `iLeftLength > 0`  <=>  `pos + 1 < len`
///   `iLeftLength < K`  <=>  `len < pos + 1 + K`   (true when `pos >= len`, which
///                                                  is where the signed form is
///                                                  negative — same verdict)
pub fn CheckBitstreamBuffer(
    _kuiSliceIdx: u32,
    _pEncCtx: &sWelsEncCtx,
    buf: &[u8],
    pBs: &BsWriter,
) -> i32 {
    let (pos, len) = (pBs.pos(), buf.len());
    debug_assert!(pos + 1 < len, "the writer is already at or past the buffer end");

    if len < pos + 1 + MAX_MACROBLOCK_SIZE_IN_BYTE_x2 as usize {
        return ENC_RETURN_VLCOVERFLOWFOUND;
    }
    ENC_RETURN_SUCCESS
}

/// Top-level macroblock CAVLC bitstream serialization function.
///
/// Matches `int32_t WelsSpatialWriteMbSyn (sWelsEncCtx* pEncCtx, SSlice* pSlice, SMB* pCurMb)`
///
/// `extern "C"` came off at T4b.1 with the slot that required it — and with the
/// thunk its CABAC twin needed to reach the same slot.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsSpatialWriteMbSyn(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) -> i32 {
    // **The writer is minted at each use-cluster, not once at the top** — the
    // ordering class session B closed (T6.C2's probe found it here first): the
    // pred writers take `&mut *pSlice`, which would pop a slice-resident writer
    // held across them. The buffer needs no such care since S11.1a: it arrives
    // threaded, borrows nothing of the slice or the context, and survives every
    // reborrow below.
    let kuiChromaQpIndexOffset = (*layer_pps(pEncCtx, current_layer(pEncCtx))).uiChromaQpIndexOffset;

    if IS_SKIP(mbs.cur().uiMbType) {
        mbs.cur_mut().uiLumaQp = (*pSlice).uiLastMbQp;
        let idx = CLIP3_QP_0_51((mbs.cur().uiLumaQp as i32) + (kuiChromaQpIndexOffset as i32));
        mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[idx as usize];

        (*pSlice).iMbSkipRun += 1;
        ENC_RETURN_SUCCESS
    } else {
        if (*pEncCtx).eSliceType != EWelsSliceType::I_SLICE {
            let kiMbSkipRun = (*pSlice).iMbSkipRun as u32;
            BsWriteUE(&mut *pSliceBsBuf, crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), kiMbSkipRun);
            (*pSlice).iMbSkipRun = 0;
        }

        // Step 1: write mb type and pred
        if IS_Inter_8x8(mbs.cur().uiMbType) {
            WelsSpatialWriteSubMbPred(pEncCtx, &mut *pSlice, mbs.cur_mut(), &mut *pSliceBsBuf, &mut *pCtxOutBs);
        } else {
            WelsSpatialWriteMbPred(pEncCtx, &mut *pSlice, mbs.cur_mut(), &mut *pSliceBsBuf, &mut *pCtxOutBs);
        }
        // T9.E2f: the writer is re-minted after the pred writers — their
        // `&mut *pSlice` reborrows above end any writer borrow taken before
        // them, and the borrow checker now enforces the placement the comment
        // used to describe.
        let pBs = crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs);

        // Step 2: write coded block pattern
        if IS_INTRA4x4(mbs.cur().uiMbType) {
            let buf = &mut *pSliceBsBuf;
            BsWriteUE(buf, &mut *pBs, g_kuiIntra4x4CbpMap[mbs.cur().uiCbp as usize]);
        } else if !IS_INTRA16x16(mbs.cur().uiMbType) {
            let buf = &mut *pSliceBsBuf;
            BsWriteUE(buf, &mut *pBs, g_kuiInterCbpMap[mbs.cur().uiCbp as usize]);
        }

        // Step 3: write QP and residual
        if mbs.cur().uiCbp > 0 || IS_INTRA16x16(mbs.cur().uiMbType) {
            let kiDeltaQp = (mbs.cur().uiLumaQp as i32) - ((*pSlice).uiLastMbQp as i32);
            (*pSlice).uiLastMbQp = mbs.cur().uiLumaQp;

            BsWriteSE(
                &mut *pSliceBsBuf,
                &mut *pBs,
                kiDeltaQp,
            );
            let pMbCache = &mut pSlice.sMbCacheInfo;
            let buf = &mut *pSliceBsBuf;
            if WelsWriteMbResidual((*pEncCtx).func_list(), &mut *pMbCache, mbs.cur(), buf, &mut *pBs) != 0 {
                return ENC_RETURN_VLCOVERFLOWFOUND;
            }
        } else {
            mbs.cur_mut().uiLumaQp = (*pSlice).uiLastMbQp;
            // `kuiChromaQpIndexOffset`, bound at this function's head from the same
            // expression. The C++ re-reads `pCurLayer->sLayerInfo.pPpsP->…` here
            // (`svc_set_mb_syn_cavlc.cpp`) and so did this port; nothing between the
            // two can change the layer's PPS, and T6.G3 made the re-read cost a
            // resolution rather than a load. Same value, once per macroblock.
            let idx = CLIP3_QP_0_51(
                (mbs.cur().uiLumaQp as i32) + (kuiChromaQpIndexOffset as i32),
            );
            mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[idx as usize];
        }

        // Step 4: Check the left buffer
        CheckBitstreamBuffer(
            (*pSlice).iSliceIdx as u32,
            pEncCtx,
            &mut *pSliceBsBuf,
            &*pBs,
        )
    }
}

/// Serializes all macroblock quantized transform coefficient residuals using CAVLC.
///
/// Matches `int32_t WelsWriteMbResidual (SWelsFuncPtrList* pFuncList, SMbCache* sMbCacheInfo, SMB* pCurMb, SBitStringAux* pBs)`
pub fn WelsWriteMbResidual(
    pFuncList: &SWelsFuncPtrList,
    sMbCacheInfo: &mut SMbCache,
    pCurMb: &SMB,
    buf: &mut [u8],
    pBs: &mut BsWriter,
) -> i32 {
    let uiMbType = pCurMb.uiMbType;
    let kiCbpChroma = (pCurMb.uiCbp >> 4) as i32;
    let kiCbpLuma = (pCurMb.uiCbp & 0x0F) as i32;
    // **T9.D3**: the neighbour non-zero counts are read out of the array at each
    // use, not through a cursor held across the six `dct(sMbCacheInfo)` calls
    // below. `iNonZeroCoeffCount` and `sDct` are different fields, so the hold was
    // never unsound — but it is the shape `q1c.py --type SMbCache` reports, and
    // indexing the root is what S28 asks for anyway. Every index below comes from
    // `g_kuiCache48CountScan4Idx` (max 47) or that table's 16.. tail plus 24 (max
    // 47), against `[i8; 48]`; the lowest is `9 - 8`.

    let mut iA: i8;
    let mut iB: i8;
    let mut iC: i8;

    if IS_INTRA16x16(uiMbType) {
        // DC luma
        iA = (*sMbCacheInfo).iNonZeroCoeffCount[8];
        iB = (*sMbCacheInfo).iNonZeroCoeffCount[1];
        iC = wels_non_zero_count_average(iA, iB);
        if WriteBlockResidualCavlc(
            pFuncList,
            &(*sMbCacheInfo).sDct.iLumaI16x16Dc[..],
            15,
            1,
            LUMA_4x4,
            iC,
            buf,
            pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        // AC Luma
        if kiCbpLuma != 0 {
            // S4.C1: the flat cursor became the block index it was always walking
            // — `pBlock.add(16)` per iteration is block `i`, and the extent the
            // callee may read is exactly that block's sixteen coefficients.
            for i in 0..16 {
                let iIdx = g_kuiCache48CountScan4Idx[i] as usize;
                iA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 1];
                iB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 8];
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    &(*sMbCacheInfo).sDct.iLumaBlock[i][..],
                    14,
                    if (*sMbCacheInfo).iNonZeroCoeffCount[iIdx] > 0 { 1 } else { 0 },
                    LUMA_AC,
                    iC,
                    buf, pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
            }
        }
    } else {
        // Luma DC AC
        if kiCbpLuma != 0 {
            // S4.C1: `pBlock.add(64)` per outer step and `.add(16/32/48)` within
            // it are blocks `i`, `i+1`, `i+2`, `i+3` — `i` advances by four, so the
            // flat cursor's arithmetic and the block index coincide exactly.
            let mut i = 0usize;
            while i < 16 {
                if (kiCbpLuma & (1 << (i >> 2))) != 0 {
                    let iIdx = g_kuiCache48CountScan4Idx[i] as usize;
                    let kiA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx];
                    let kiB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx + 1];
                    let kiC_val = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx + 8];
                    let kiD = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx + 9];

                    iA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 1];
                    iB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 8];
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        &(*sMbCacheInfo).sDct.iLumaBlock[i][..],
                        15,
                        if kiA > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        buf, pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = kiA;
                    iB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 7];
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        &(*sMbCacheInfo).sDct.iLumaBlock[i + 1][..],
                        15,
                        if kiB > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        buf, pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx + 7];
                    iB = kiA;
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        &(*sMbCacheInfo).sDct.iLumaBlock[i + 2][..],
                        15,
                        if kiC_val > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        buf, pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }

                    iA = kiC_val;
                    iB = kiB;
                    iC = wels_non_zero_count_average(iA, iB);
                    if WriteBlockResidualCavlc(
                        pFuncList,
                        &(*sMbCacheInfo).sDct.iLumaBlock[i + 3][..],
                        15,
                        if kiD > 0 { 1 } else { 0 },
                        LUMA_4x4,
                        iC,
                        buf, pBs,
                    ) != 0
                    {
                        return ENC_RETURN_VLCOVERFLOWFOUND;
                    }
                }
                i += 4;
            }
        }
    }

    if kiCbpChroma != 0 {
        // Chroma DC residual present
        // S4.C1: the `.add(4)` that walked from `iChromaDc[0]` into `iChromaDc[1]`
        // is the second row of the array, named as such. Each row is four
        // coefficients and `iEndIdx` is 3, so the slice is the exact extent.
        if WriteBlockResidualCavlc(
            pFuncList,
            &(*sMbCacheInfo).sDct.iChromaDc[0][..],
            3,
            1,
            CHROMA_DC,
            CHROMA_DC_NC_OFFSET,
            buf, pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        if WriteBlockResidualCavlc(
            pFuncList,
            &(*sMbCacheInfo).sDct.iChromaDc[1][..],
            3,
            1,
            CHROMA_DC,
            CHROMA_DC_NC_OFFSET,
            buf, pBs,
        ) != 0
        {
            return ENC_RETURN_VLCOVERFLOWFOUND;
        }

        // Chroma AC residual present
        if (kiCbpChroma & 0x02) != 0 {
            let kCache48CountScan4Idx16base = &g_kuiCache48CountScan4Idx[16..];
            // S4.C1: blocks 0..4 are Cb, 4..8 are Cr — the two loops' flat
            // cursors said the same thing with arithmetic.
            for i in 0..4 {
                let iIdx = kCache48CountScan4Idx16base[i] as usize;
                iA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 1];
                iB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 8];
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    &(*sMbCacheInfo).sDct.iChromaBlock[i][..],
                    14,
                    if (*sMbCacheInfo).iNonZeroCoeffCount[iIdx] > 0 { 1 } else { 0 },
                    CHROMA_AC,
                    iC,
                    buf, pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
            }

            // The Cr half. This cursor is the one S28 recorded as session B's ninth:
            // written as `iChromaBlock[4].as_mut_ptr()` it narrowed the tag to block
            // 4 and the second iteration read outside it, so it had to be derived
            // from the whole array and offset. Indexing `[4 + i]` per call retires
            // the question — there is no cursor to narrow. **Only the CAVLC probe
            // (T6.C2) reaches this line**; the CABAC/LOW probe never did.
            for i in 0..4 {
                let iIdx = 24 + (kCache48CountScan4Idx16base[i] as usize);
                iA = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 1];
                iB = (*sMbCacheInfo).iNonZeroCoeffCount[iIdx - 8];
                iC = wels_non_zero_count_average(iA, iB);
                if WriteBlockResidualCavlc(
                    pFuncList,
                    &(*sMbCacheInfo).sDct.iChromaBlock[4 + i][..],
                    14,
                    if (*sMbCacheInfo).iNonZeroCoeffCount[iIdx] > 0 { 1 } else { 0 },
                    CHROMA_AC,
                    iC,
                    buf, pBs,
                ) != 0
                {
                    return ENC_RETURN_VLCOVERFLOWFOUND;
                }
            }
        }
    }
    0
}

/// The CAVLC stash needs no buffer: a detached cursor is `Copy`, so the whole
/// snapshot is `*pBs`.
///
/// **T4b.1 deleted the `buf` parameter.** It was here because the CABAC variant
/// behind the same `pfStashMBStatus` slot needs one, and one slot meant one
/// signature. [`EntropyCoder`] is a `match` and not a slot, so each arm now names
/// only what it uses — the first thing banked by making this an enum rather than
/// a trait object.
///
/// [`EntropyCoder`]: crate::encoder::wels_func_ptr_def::EntropyCoder
pub fn StashMBStatusCavlc(
    pBs: &mut BsWriter,
    pDss: &mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
    kuiLastMbQp: u8,
    iMbSkipRun: i32,
) {
    // **S5.D1**: both null tests are gone with the pointers. Every one of the
    // family's fourteen call sites was enumerated before they were deleted: `pDss`
    // is `&mut sDss`, a caller local, at all of them, and `pBs` is the writer
    // resolver's result (`slice_bs_writer` since S11.1a), a reference. Neither
    // branch could be taken; a reference says so in the type.
    //
    // Three cursor fields become one value. `BsWriter` is `Copy`, which is the whole
    // point of a detached cursor. The CABAC twin below now reads the same way —
    // `sStoredCabac = *pCtx` — since T3.5 turned its triple into offsets; the two
    // families end symmetric, and the only thing still asymmetric between them is
    // CABAC's byte copy, which exists for `PropagateCarry` and not for the cursor.
    pDss.sBsStack = *pBs;
    pDss.uiLastMbQp = kuiLastMbQp;
    pDss.iMbSkipRunStack = iMbSkipRun;
}

/// See [`StashMBStatusCavlc`] for why this takes no buffer.
///
/// **T9.E6**: the `uiLastMbQp` restore moved to the call sites (the caller owns
/// `sDss` and the slice; this fn no longer names `SSlice` — S54's value rule).
pub fn StashPopMBStatusCavlc(
    pBs: &mut BsWriter,
    pDss: &mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
) -> i32 {
    // S5.D1: as the stash side — the two null tests were unreachable, see there.
    *pBs = pDss.sBsStack;
    pDss.iMbSkipRunStack
}

/// `StashMBStatusCabac` — set_mb_syn_cavlc.cpp:250. (The three CABAC entry
/// points live in *cavlc*.cpp in the reference, next to their CAVLC twins.)
///
/// Saves the whole arithmetic-coder state, and — unlike the CAVLC twin, which
/// only has to remember three bitstream cursor fields — copies out the bytes
/// already emitted, because CABAC renormalisation can rewrite them via
/// `PropagateCarry`.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn StashMBStatusCabac(
    buf: &mut [u8],
    pDss: &mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
    pCabacCtx: &mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
    kuiLastMbQp: u8,
    iMbSkipRun: i32,
) {
    // S5.D1: as `StashMBStatusCavlc` — the null test was unreachable at all
    // fourteen call sites; `pCabacCtx` is `addr_of_mut!((*pSlice).sCabacCtx)`.
    let pCtx = pCabacCtx;
    // `SCabacCtx` is `Copy` and, since T3.5, holds no pointers — so the whole
    // snapshot is this one assignment, the same shape the CAVLC twin above
    // reached in T3.4. What the two still do not share is the byte copy below:
    // CABAC's `PropagateCarry` rewrites bytes it already emitted, so restoring
    // the cursor is not enough to restore the output.
    (*pDss).sStoredCabac = *pCtx;
    if !(*pDss).pRestoreBuffer.is_null() {
        let iPosBitOffset = GetBsPosCabac(pCtx) - pDss.iStartPos;
        let iLen = (iPosBitOffset >> 3) + if (iPosBitOffset & 0x07) != 0 { 1 } else { 0 };
        let start = (*pCtx).m_iBufStart;
        // Sliced, not offset: `buf[start..start + iLen]` is what bounds the
        // read against the output buffer, which the C++ never did. The
        // destination stays a raw pointer — `pRestoreBuffer` is one of the
        // `pDynamicBsBuffer` allocations, and those are Phase 6's.
        let src = &buf[start..start + iLen as usize];
        std::ptr::copy_nonoverlapping(src.as_ptr(), (*pDss).pRestoreBuffer, iLen as usize);
    }
    (*pDss).uiLastMbQp = kuiLastMbQp;
    (*pDss).iMbSkipRunStack = iMbSkipRun;
}

/// `StashPopMBStatusCabac` — set_mb_syn_cavlc.cpp:261.
///
/// Note the offset is recomputed from the *restored* context, so
/// `GetBsPosCabac` is called after `sStoredCabac` has been copied back.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn StashPopMBStatusCabac(
    buf: &mut [u8],
    pDss: &mut crate::encoder::svc_encode_slice::SDynamicSlicingStack,
    pCabacCtx: &mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
) -> i32 {
    // S5.D1: as the stash side — the null test was unreachable, see there.
    let pCtx = pCabacCtx;
    *pCtx = (*pDss).sStoredCabac;
    // Write-extent audit site 3: the one write that is not at the cursor.
    if !(*pDss).pRestoreBuffer.is_null() {
        let iPosBitOffset = GetBsPosCabac(pCtx) - pDss.iStartPos;
        let iLen = (iPosBitOffset >> 3) + if (iPosBitOffset & 0x07) != 0 { 1 } else { 0 };
        let start = (*pCtx).m_iBufStart;
        // Same bound as the stash side, on the write this time — this is the
        // one write in the whole engine that is not at the cursor, and
        // `buf[start..start + iLen]` is what says how far it may reach.
        let dst = &mut buf[start..start + iLen as usize];
        std::ptr::copy_nonoverlapping((*pDss).pRestoreBuffer, dst.as_mut_ptr(), iLen as usize);
    }
    (*pDss).iMbSkipRunStack
}

/// `GetBsPosCabac` — set_mb_syn_cavlc.cpp:275.
///
/// The bit position is derived from the arithmetic coder's own byte cursor, not
/// from `SBitStringAux`: `((m_iBufCur - m_iBufStart) << 3) + (m_iLowBitCnt - 9)`.
/// The `- 9` is load-bearing and the result can legitimately be negative before
/// the first byte is emitted.
///
/// Both cursor fields are offsets into the same buffer, so the difference is
/// plain arithmetic and this function needs no buffer — which is why
/// [`EntropyCoder::GetBsPosition`] takes none on either arm.
///
/// `extern "C"` came off at T4b.1 with the slot that required it.
///
/// [`EntropyCoder::GetBsPosition`]: crate::encoder::wels_func_ptr_def::EntropyCoder::GetBsPosition
pub fn GetBsPosCabac(pCabacCtx: &crate::encoder::set_mb_syn_cabac::SCabacCtx) -> i32 {
    // S5.D1: shared, and the null test goes with the pointer. Three call sites: the
    // `EntropyCoder::GetBsPosition` dispatch, which passes `&(*pSlice).sCabacCtx` —
    // an inline field of a live slice — and the two CABAC stash bodies above, which
    // pass the coder state they were handed.
    let pCtx = pCabacCtx;
    ((pCtx.m_iBufCur - pCtx.m_iBufStart) as i32) * 8 + (pCtx.m_iLowBitCnt - 9)
}

// `WelsSpatialWriteMbSynCabacThunk` was here. It existed for one reason —
// `WelsSpatialWriteMbSynCabac` is a plain Rust `fn` and `pfWelsSpatialWriteMbSyn`
// held an `extern "C"` pointer, so something had to bridge the two, where C++
// assigns the function itself (`set_mb_syn_cavlc.cpp:308`). T4b.1 deleted the
// slot; with no slot there is no slot type, and the thunk was pure deletion.

/// `extern "C"` came off at T4b.1 with the slot that required it.
///
/// Takes the slice's writer (`slice_bs_writer`) rather than the slice: the writer is
/// all this reads, and the slice no longer stores it (Phase 6 session B).
pub fn GetBsPosCavlc(pBs: &BsWriter) -> i32 {
    // S5.D1: shared, not `&mut` — this reads the writer's position and nothing else.
    crate::encoder::vlc_encoder::BsGetBitsPos(pBs)
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

        let total_zeros = CavlcParamCal_c(&coeffs[..], &mut run, &mut level, &mut total_coeffs, 15);
        assert_eq!(total_coeffs, 3);
        assert_eq!(total_zeros, 1);
    }
}
