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

#![deny(unsafe_code)]

pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SMVComponentUnit;

// ============================================================================
// Constants & Configuration Limits
// ============================================================================

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



// ============================================================================
// Context Offset Tables
// ============================================================================

pub const uiSignificantCoeffFlagOffset: [u16; 5] = [0, 15, 29, 44, 47];
pub const uiLastCoeffFlagOffset: [u16; 5] = [0, 15, 29, 44, 47];
pub const uiCoeffAbsLevelMinus1Offset: [u16; 5] = [0, 10, 20, 30, 39];
pub const uiCodecBlockFlagOffset: [u16; 5] = [0, 4, 8, 12, 16];

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












pub use crate::encoder::svc_encode_slice::SSliceHeader;
use crate::encoder::svc_encode_slice::current_layer_ref;
use crate::encoder::svc_encode_slice::current_layer;
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::vlc_encoder::ECtxBlockCat;
pub use crate::encoder::set_mb_syn_cabac::SStateCtx;
pub use crate::encoder::set_mb_syn_cabac::SCabacCtx;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::md::SMB;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;



// `SWelsPps` used to be declared here: a one-field struct holding
// `uiChromaQpIndexOffset: u32`. C++ has no such type -- the real one is
// `param_svc.h`'s `SWelsPPS`, where `uiChromaQpIndexOffset` is a `uint8_t` at offset 10,
// behind `iSpsId`/`iPpsId`/`iPicInitQp`/`iPicInitQs`. Reading it through the fake would
// have returned `iSpsId`. It was dead code, and it is deleted rather than fixed.



// ============================================================================
// Low-Level CABAC Bitstream & Arithmetic Routines
// ============================================================================
//
// There are none here any more, and that is the point. Upstream splits the two
// CABAC files exactly this way: `set_mb_syn_cabac.cpp` owns the arithmetic
// engine, `svc_set_mb_syn_cabac.cpp` owns only the macroblock *syntax* that
// drives it. The port had transliterated the engine a second time into this
// module — nine functions and five tables — and because a module-local item
// beats a `use`, this file's syntax layer silently ran the local copy while
// `WelsWriteSliceEndSyn` flushed through the canonical one. Two engines, one
// `SCabacCtx`, split across a slice.
//
// That exact mechanism has already produced one real defect in this file: see
// the `BsAlign` note below, where a local copy missing its trailing `BsFlush`
// beat the import and corrupted every CABAC slice's first bytes. The engine is
// now imported, once, and the shadowing cannot recur.
pub use crate::encoder::set_mb_syn_cabac::{
    cabac_low_t, g_kiClz5Table, g_kuiCabacRangeLps, g_kuiStateTransTable, PropagateCarry,
    WelsCabacEncodeBypassOne, WelsCabacEncodeDecision, WelsCabacEncodeDecisionLps_,
    WelsCabacEncodeInit, WelsCabacEncodeTerminate, WelsCabacEncodeUeBypass,
    WelsCabacEncodeUpdateLow_, WelsCabacEncodeUpdateLowNontrivial_, CABAC_LOW_WIDTH,
    WELS_CONTEXT_COUNT, WELS_QP_MAX,
};

// `BsAlign` — svc_enc_golomb.h:112. This module used to declare its own copy
// **without the trailing `BsFlush (pBs)`**, and being a local item it beat the
// import in `WelsInitSliceCabac`. Without the flush, `pBs->pCurBuf` still points
// before the pending accumulator word, so `WelsCabacEncodeInit` started the
// arithmetic coder on top of bytes of the slice header that had already been
// written. Use the one faithful copy.
pub use crate::encoder::vlc_encoder::BsAlign;

// ============================================================================
// Macroblock Header & Mode Serialization
// ============================================================================

pub fn WelsCabacMbType(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>,
    pMbCache: &mut SMbCache,
    eSliceType: EWelsSliceType,
) {
    {
        if eSliceType == EWelsSliceType::I_SLICE {
            let uiNeighborAvail = mbs.cur().uiNeighborAvail;
            // F14's shape retired with the window: the neighbour is asked for
            // only under its availability guard, and the ask itself is checked.
            let mut iCtx = 3;

            if (uiNeighborAvail & LEFT_MB_POS) != 0 && !IS_INTRA4x4(mbs.left().uiMbType) {
                iCtx += 1;
            }
            if (uiNeighborAvail & TOP_MB_POS) != 0 && !IS_INTRA4x4(mbs.top().uiMbType) {
                iCtx += 1;
            }

            if mbs.cur().uiMbType == MB_TYPE_INTRA4x4 {
                WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 0);
            } else {
                let iCbpChroma = (mbs.cur().uiCbp >> 4) as i32;
                let iCbpLuma = (mbs.cur().uiCbp & 15) as i32;
                let iPredMode = g_kiMapModeI16x16[(*pMbCache).uiLumaI16x16Mode as usize] as i32;

                WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
                WelsCabacEncodeTerminate(buf, pCabacCtx, 0);

                if iCbpLuma != 0 {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 6, 1);
                } else {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 6, 0);
                }

                if iCbpChroma == 0 {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 7, 0);
                } else {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 7, 1);
                    WelsCabacEncodeDecision(buf, pCabacCtx, 8, (iCbpChroma >> 1) as u32);
                }

                WelsCabacEncodeDecision(buf, pCabacCtx, 9, (iPredMode >> 1) as u32);
                WelsCabacEncodeDecision(buf, pCabacCtx, 10, (iPredMode & 1) as u32);
            }
        } else if eSliceType == EWelsSliceType::P_SLICE {
            let uiMbType = mbs.cur().uiMbType;
            if uiMbType == MB_TYPE_16x16 {
                WelsCabacEncodeDecision(buf, pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(buf, pCabacCtx, 15, 0);
                WelsCabacEncodeDecision(buf, pCabacCtx, 16, 0);
            } else if (uiMbType == MB_TYPE_16x8) || (uiMbType == MB_TYPE_8x16) {
                WelsCabacEncodeDecision(buf, pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(buf, pCabacCtx, 15, 1);
                WelsCabacEncodeDecision(buf, pCabacCtx, 17, if uiMbType == MB_TYPE_16x8 { 1 } else { 0 });
            } else if (uiMbType == MB_TYPE_8x8) || (uiMbType == MB_TYPE_8x8_REF0) {
                WelsCabacEncodeDecision(buf, pCabacCtx, 14, 0);
                WelsCabacEncodeDecision(buf, pCabacCtx, 15, 0);
                WelsCabacEncodeDecision(buf, pCabacCtx, 16, 1);
            } else if mbs.cur().uiMbType == MB_TYPE_INTRA4x4 {
                WelsCabacEncodeDecision(buf, pCabacCtx, 14, 1);
                WelsCabacEncodeDecision(buf, pCabacCtx, 17, 0);
            } else {
                let iCbpChroma = (mbs.cur().uiCbp >> 4) as i32;
                let iCbpLuma = (mbs.cur().uiCbp & 15) as i32;
                let iPredMode = g_kiMapModeI16x16[(*pMbCache).uiLumaI16x16Mode as usize] as i32;

                // prefix
                WelsCabacEncodeDecision(buf, pCabacCtx, 14, 1);

                // suffix
                WelsCabacEncodeDecision(buf, pCabacCtx, 17, 1);
                WelsCabacEncodeTerminate(buf, pCabacCtx, 0);
                if iCbpLuma != 0 {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 18, 1);
                } else {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 18, 0);
                }

                if iCbpChroma == 0 {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 19, 0);
                } else {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 19, 1);
                    WelsCabacEncodeDecision(buf, pCabacCtx, 19, (iCbpChroma >> 1) as u32);
                }

                WelsCabacEncodeDecision(buf, pCabacCtx, 20, (iPredMode >> 1) as u32);
                WelsCabacEncodeDecision(buf, pCabacCtx, 20, (iPredMode & 1) as u32);
            }
        }
    }
}

pub fn WelsCabacMbIntra4x4PredMode(buf: &mut [u8], pCabacCtx: &mut SCabacCtx, pMbCache: &mut SMbCache) {
    for iMode in 0..16 {
        let bPredFlag = (*pMbCache).bPrevIntra4x4PredModeFlag[iMode];
        let iRemMode = (*pMbCache).iRemIntra4x4PredModeFlag[iMode] as i32;

        if bPredFlag {
            WelsCabacEncodeDecision(buf, pCabacCtx, 68, 1);
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, 68, 0);
            WelsCabacEncodeDecision(buf, pCabacCtx, 69, (iRemMode & 0x01) as u32);
            WelsCabacEncodeDecision(buf, pCabacCtx, 69, ((iRemMode >> 1) & 0x01) as u32);
            WelsCabacEncodeDecision(buf, pCabacCtx, 69, (iRemMode >> 2) as u32);
        }
    }
}

pub fn WelsCabacMbIntraChromaPredMode(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>,
    pMbCache: &mut SMbCache,
) {
    {
        let uiNeighborAvail = mbs.cur().uiNeighborAvail;

        let iPredMode = g_kiMapModeIntraChroma[(*pMbCache).uiChmaI8x8Mode as usize] as i32;
        let mut iCtx = 64;
        if (uiNeighborAvail & LEFT_MB_POS) != 0
            && g_kiMapModeIntraChroma[mbs.left().uiChromPredMode as usize] != 0
        {
            iCtx += 1;
        }
        if (uiNeighborAvail & TOP_MB_POS) != 0
            && g_kiMapModeIntraChroma[mbs.top().uiChromPredMode as usize] != 0
        {
            iCtx += 1;
        }

        if iPredMode == 0 {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 0);
        } else if iPredMode == 1 {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(buf, pCabacCtx, 67, 0);
        } else if iPredMode == 2 {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(buf, pCabacCtx, 67, 1);
            WelsCabacEncodeDecision(buf, pCabacCtx, 67, 0);
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
            WelsCabacEncodeDecision(buf, pCabacCtx, 67, 1);
            WelsCabacEncodeDecision(buf, pCabacCtx, 67, 1);
        }
    }
}

pub fn WelsCabacMbCbp(buf: &mut [u8], mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>, pCabacCtx: &mut SCabacCtx) {
    {
        let cbp = mbs.cur().uiCbp as i32;
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
        let uiNeighborAvail = mbs.cur().uiNeighborAvail;

        if (uiNeighborAvail & LEFT_MB_POS) != 0 {
            let iCbp = mbs.left().uiCbp as i32;
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
            let iCbp = mbs.top().uiCbp as i32;
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

        WelsCabacEncodeDecision(buf, 
            pCabacCtx,
            73 + iCbpBlockLeft[1] + iCbpBlockTop[2] * 2,
            iCbpBlockLuma[0],
        );
        WelsCabacEncodeDecision(buf, 
            pCabacCtx,
            73 + not_cbp0 + iCbpBlockTop[3] * 2,
            iCbpBlockLuma[1],
        );
        WelsCabacEncodeDecision(buf, 
            pCabacCtx,
            73 + iCbpBlockLeft[3] + not_cbp0 * 2,
            iCbpBlockLuma[2],
        );
        WelsCabacEncodeDecision(buf, 
            pCabacCtx,
            73 + not_cbp2 + not_cbp1 * 2,
            iCbpBlockLuma[3],
        );

        // Chroma CBP
        if iCbpChroma != 0 {
            WelsCabacEncodeDecision(buf, pCabacCtx, 77 + iCtx, 1);
            WelsCabacEncodeDecision(buf, 
                pCabacCtx,
                81 + (iCbpLeftChroma >> 1) + ((iCbpTopChroma >> 1) * 2),
                if iCbpChroma > 1 { 1 } else { 0 },
            );
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, 77 + iCtx, 0);
        }
    }
}

pub fn WelsCabacMbDeltaQp(
    buf: &mut [u8],
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pCabacCtx: &mut SCabacCtx,
    bFirstMbInSlice: bool,
) {
    {
        let mut iCtx = 0;

        if !bFirstMbInSlice {
            // The previously *coded* record — raster order within the slice,
            // which `prev` answers where `left` would refuse a row start.
            let (prevQp, prevType, prevCbp, prevDQp) = {
                let p = mbs.prev();
                (p.uiLumaQp, p.uiMbType, p.uiCbp, p.iLumaDQp)
            };
            mbs.cur_mut().iLumaDQp = (mbs.cur().uiLumaQp as i32) - (prevQp as i32);

            if IS_SKIP(prevType)
                || ((prevType != MB_TYPE_INTRA16x16) && (prevCbp == 0))
                || (prevDQp == 0)
            {
                iCtx = 0;
            } else {
                iCtx = 1;
            }
        }

        if mbs.cur().iLumaDQp != 0 {
            let mut iValue = if mbs.cur().iLumaDQp < 0 {
                -2 * mbs.cur().iLumaDQp
            } else {
                2 * mbs.cur().iLumaDQp - 1
            };

            WelsCabacEncodeDecision(buf, pCabacCtx, 60 + iCtx, 1);
            if iValue == 1 {
                WelsCabacEncodeDecision(buf, pCabacCtx, 60 + 2, 0);
            } else {
                WelsCabacEncodeDecision(buf, pCabacCtx, 60 + 2, 1);
                iValue -= 1;
                while {
                    iValue -= 1;
                    iValue > 0
                } {
                    WelsCabacEncodeDecision(buf, pCabacCtx, 60 + 3, 1);
                }
                WelsCabacEncodeDecision(buf, pCabacCtx, 60 + 3, 0);
            }
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, 60 + iCtx, 0);
        }
    }
}

pub fn WelsMbSkipCabac(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    eSliceType: EWelsSliceType,
    bSkipFlag: i16,
) {
    {
        let mut iCtx = if eSliceType == EWelsSliceType::P_SLICE { 11 } else { 24 };
        let uiNeighborAvail = mbs.cur().uiNeighborAvail;

        if (uiNeighborAvail & LEFT_MB_POS) != 0 {
            if !IS_SKIP(mbs.left().uiMbType) {
                iCtx += 1;
            }
        }
        if (uiNeighborAvail & TOP_MB_POS) != 0 {
            if !IS_SKIP(mbs.top().uiMbType) {
                iCtx += 1;
            }
        }

        WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, bSkipFlag as u32);

        if bSkipFlag != 0 {
            let cur = mbs.cur_mut();
            for i in 0..16 {
                cur.sMvd[i].iMvX = 0;
                cur.sMvd[i].iMvY = 0;
            }
            cur.uiCbp = 0;
            cur.iCbpDc = 0;
        }
    }
}

pub fn WelsCabacMbRef(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    pMbCache: &mut SMbCache,
    iIdx: i16,
) {
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
        WelsCabacEncodeDecision(buf, pCabacCtx, (54 + iCtx) as i32, 1);
        iCtx = (iCtx >> 2) + 4;
        iRefIdx -= 1;
    }
    WelsCabacEncodeDecision(buf, pCabacCtx, (54 + iCtx) as i32, 0);
}

#[inline]
pub fn WelsCabacMbMvdLx(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    sMvd: i32,
    iCtx: i32,
    iPredMvd: i32,
) {
    {
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
                WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 1);
                iCtxInc = 3;
                for i in 0..(iPrefix - 1) {
                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 1);
                    if i < 3 {
                        iCtxInc += 1;
                    }
                }
                WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 0);
                WelsCabacEncodeBypassOne(buf, pCabacCtx, if sMvd < 0 { 1 } else { 0 });
            } else {
                WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 1);
                iCtxInc = 3;
                for i in 0..(9 - 1) {
                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 1);
                    if i < 3 {
                        iCtxInc += 1;
                    }
                }
                WelsCabacEncodeUeBypass(buf, pCabacCtx, 3, (iAbsMvd - 9) as u32);
                WelsCabacEncodeBypassOne(buf, pCabacCtx, if sMvd < 0 { 1 } else { 0 });
            }
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx + iCtxInc, 0);
        }
    }
}

pub fn WelsCabacMbMvd(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>,
    sCurMv: SMVUnitXY,
    sPredMv: SMVUnitXY,
    i4x4ScanIdx: i16,
) -> SMVUnitXY {
    {
        let uiNeighborAvail = mbs.cur().uiNeighborAvail;
        let mut sMvd = SMVUnitXY::default();
        let mut sMvdLeft = SMVUnitXY::default();
        let mut sMvdTop = SMVUnitXY::default();

        sMvd.sDeltaMv(sCurMv, sPredMv);

        if (i4x4ScanIdx < 4) && ((uiNeighborAvail & TOP_MB_POS) != 0) {
            sMvdTop.sAssignMv(mbs.top().sMvd[(i4x4ScanIdx + 12) as usize]);
        } else if i4x4ScanIdx >= 4 {
            sMvdTop.sAssignMv(mbs.cur().sMvd[(i4x4ScanIdx - 4) as usize]);
        }

        if ((i4x4ScanIdx & 0x03) == 0) && ((uiNeighborAvail & LEFT_MB_POS) != 0) {
            sMvdLeft.sAssignMv(mbs.left().sMvd[(i4x4ScanIdx + 3) as usize]);
        } else if (i4x4ScanIdx & 0x03) != 0 {
            sMvdLeft.sAssignMv(mbs.cur().sMvd[(i4x4ScanIdx - 1) as usize]);
        }

        let iAbsMvd0 = (sMvdLeft.iMvX.abs() as i32) + (sMvdTop.iMvX.abs() as i32);
        let iAbsMvd1 = (sMvdLeft.iMvY.abs() as i32) + (sMvdTop.iMvY.abs() as i32);

        WelsCabacMbMvdLx(buf, pCabacCtx, sMvd.iMvX as i32, 40, iAbsMvd0);
        WelsCabacMbMvdLx(buf, pCabacCtx, sMvd.iMvY as i32, 47, iAbsMvd1);

        sMvd
    }
}

pub fn WelsCabacSubMbType(buf: &mut [u8], pCabacCtx: &mut SCabacCtx, pCurMb: &SMB) {
    {
        for i8x8Idx in 0..4 {
            let uiSubMbType = pCurMb.uiSubMbType[i8x8Idx] as u32;
            if SUB_MB_TYPE_8x8 == uiSubMbType {
                WelsCabacEncodeDecision(buf, pCabacCtx, 21, 1);
                continue;
            }
            // D-dead-2 / F122 — the `_8x4`/`_4x8`/`_4x4` bins (contexts 22 and 23)
            // are gone with the sub-8x8 search that produced their partitions. Every
            // writer of `uiSubMbType` in this encoder sets `SUB_MB_TYPE_8x8`
            // (`svc_base_layer_md.rs:1164`/`:1249`/`:1262`,
            // `svc_mode_decision.rs:2495`); upstream's only other writers are inside
            // `#if 0 //Disable for sub8x8 modes for now`
            // (`svc_mode_decision.cpp:634-661`). A wrong bin here desynchronises the
            // arithmetic coder for the rest of the slice, so this fails loudly.
            unreachable!(
                "sub_mb_type {:#x} — the sub-8x8 search is #if 0 upstream and \
                 unwritten here (D-dead-2/F122)",
                uiSubMbType
            );
        }
    }
}

pub fn WelsCabacSubMbMvd(
    buf: &mut [u8],
    pCabacCtx: &mut SCabacCtx,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pMbCache: &mut SMbCache,
) {
    {
        for i8x8Idx in 0..4 {
            let uiSubMbType = mbs.cur().uiSubMbType[i8x8Idx] as u32;
            if SUB_MB_TYPE_8x8 == uiSubMbType {
                let i4x4ScanIdx = g_kuiMbCountScan4Idx[i8x8Idx << 2] as i16;
                let cur_mv = mbs.cur().sMv[i4x4ScanIdx as usize];
                let pred_mv = (*pMbCache).sMbMvp[i4x4ScanIdx as usize];
                let sMvd = WelsCabacMbMvd(buf, pCabacCtx, mbs, cur_mv, pred_mv, i4x4ScanIdx);

                let idx = i4x4ScanIdx as usize;
                let cur = mbs.cur_mut();
                cur.sMvd[idx].sAssignMv(sMvd);
                cur.sMvd[1 + idx].sAssignMv(sMvd);
                cur.sMvd[4 + idx].sAssignMv(sMvd);
                cur.sMvd[5 + idx].sAssignMv(sMvd);
            } else {
                // D-dead-2 / F122 — the `_4x4`/`_8x4`/`_4x8` motion-vector-difference
                // arms go with the partitions. See `WelsCabacSubMbType` above for the
                // reachability argument.
                unreachable!(
                    "sub_mb_type {:#x} — the sub-8x8 search is #if 0 upstream and \
                     unwritten here (D-dead-2/F122)",
                    uiSubMbType
                );
            }
        }
    }
}

pub fn WelsGetMbCtxCabac(
    kpNonZeroCoeffCount: &[i8; 48],
    mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>,
    eCtxBlockCat: ECtxBlockCat,
    iIdx: i16,
) -> i16 {
    {
        let mut iNzA: i16 = -1;
        let mut iNzB: i16 = -1;
        let bIntra = IS_INTRA(mbs.cur().uiMbType);
        let mut iCtxInc = 0;

        match eCtxBlockCat {
            ECtxBlockCat::LUMA_AC | ECtxBlockCat::CHROMA_AC | ECtxBlockCat::LUMA_4x4 => {
                iNzA = kpNonZeroCoeffCount[(iIdx - 1) as usize] as i16;
                iNzB = kpNonZeroCoeffCount[(iIdx - 8) as usize] as i16;
            }
            ECtxBlockCat::LUMA_DC | ECtxBlockCat::CHROMA_DC => {
                if (mbs.cur().uiNeighborAvail & LEFT_MB_POS) != 0 {
                    iNzA = (mbs.left().iCbpDc & (1 << iIdx)) as i16;
                }
                if (mbs.cur().uiNeighborAvail & TOP_MB_POS) != 0 {
                    iNzB = (mbs.top().iCbpDc & (1 << iIdx)) as i16;
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

pub fn WelsWriteBlockResidualCabac(
    buf: &mut [u8],
    kpNonZeroCoeffCount: &[i8; 48],
    mbs: &crate::safe::mb_grid::MbWindow<'_, SMB>,
    pCabacCtx: &mut SCabacCtx,
    eCtxBlockCat: ECtxBlockCat,
    iIdx: i16,
    iNonZeroCount: i16,
    // S11.45: the residual cursor is the block it walked — every read below is
    // bounds-checked against the caller's array.
    pBlock: &[i16],
    iEndIdx: i16,
) {
    {
        let mut iCtx = WelsGetMbCtxCabac(kpNonZeroCoeffCount, mbs, eCtxBlockCat, iIdx) as i32;

        if iNonZeroCount != 0 {
            let mut iLevel = [0i16; 16];
            let iCtxSig = 105 + (uiSignificantCoeffFlagOffset[eCtxBlockCat as usize] as i32);
            let iCtxLast = 166 + (uiLastCoeffFlagOffset[eCtxBlockCat as usize] as i32);
            let iCtxLevel = 227 + (uiCoeffAbsLevelMinus1Offset[eCtxBlockCat as usize] as i32);
            let mut iNonZeroIdx: usize = 0;
            let mut i: usize = 0;

            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
            loop {
                let coeff = pBlock[i];
                if coeff != 0 {
                    iLevel[iNonZeroIdx] = coeff;
                    iNonZeroIdx += 1;

                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtxSig + (i as i32), 1);
                    if (iNonZeroIdx as i16) != iNonZeroCount {
                        WelsCabacEncodeDecision(buf, pCabacCtx, iCtxLast + (i as i32), 0);
                    } else {
                        WelsCabacEncodeDecision(buf, pCabacCtx, iCtxLast + (i as i32), 1);
                        break;
                    }
                } else {
                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtxSig + (i as i32), 0);
                }

                i += 1;
                if (i as i16) == iEndIdx {
                    iLevel[iNonZeroIdx] = pBlock[i];
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
                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
                    iNumAbsLevelGt1 += 1;

                    let max_shift = 5 - if eCtxBlockCat == ECtxBlockCat::CHROMA_DC { 1 } else { 0 };
                    iCtx = iCtxLevel + 4 + core::cmp::min(max_shift, iNumAbsLevelGt1);

                    for _ in 1..iPrefix {
                        WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 1);
                    }

                    if abs_lvl < 15 {
                        WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 0);
                    } else {
                        WelsCabacEncodeUeBypass(buf, pCabacCtx, 0, (abs_lvl - 15) as u32);
                    }
                    iCtx1 = iCtxLevel;
                } else {
                    iCtx = core::cmp::min(iCtxLevel + 4, iCtx1);
                    WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 0);
                    if iNumAbsLevelGt1 == 0 {
                        iCtx1 += 1;
                    }
                }

                WelsCabacEncodeBypassOne(buf, pCabacCtx, if lvl < 0 { 1 } else { 0 });

                if iNonZeroIdx == 0 {
                    break;
                }
            }
        } else {
            WelsCabacEncodeDecision(buf, pCabacCtx, iCtx, 0);
        }
    }
}

#[inline]
pub fn WelsCalNonZeroCount2x2Block(pBlock: &[i16; 4]) -> i32 {
    ((pBlock[0] != 0) as i32)
        + ((pBlock[1] != 0) as i32)
        + ((pBlock[2] != 0) as i32)
        + ((pBlock[3] != 0) as i32)
}

pub fn WelsWriteMbResidualCabac(
    buf: &mut [u8],
    pFuncList: &SWelsFuncPtrList,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    uiChromaQpIndexOffset: u32,
) -> i32 {
    {
        let uiMbType = mbs.cur().uiMbType;
        // Both of these used to arrive as parameters *alongside* `pSlice`, which is
        // where they come from — and the body reaches `pSlice.uiLastMbQp` between
        // uses of them. Two live paths to one slice is the aliasing bug this session
        // is here to remove, so they are derived here and reborrowed per call: each
        // `&mut *` below is a child of `pSlice`'s tag that dies at the call it is
        // made for, and the `(*pSlice)` accesses in between never overlap one.
        // `sMbCacheInfo` was already dead as a parameter — the old body re-derived
        // `pMbCache` from `pSlice` and never read the argument.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let pCabacCtx = &mut pSlice.sCabacCtx;
        let kpNonZeroCoeffCount = &pMbCache.iNonZeroCoeffCount;
        let pSliceHeadExt = &mut pSlice.sSliceHeaderExt;
        let iSliceFirstMbXY = pSliceHeadExt.sSliceHeader.iFirstMbInSlice;

        {
            let cur = mbs.cur_mut();
            cur.iCbpDc = 0;
            cur.iLumaDQp = 0;
        }

        if (mbs.cur().uiCbp > 0) || (uiMbType == MB_TYPE_INTRA16x16) {
            let iCbpChroma = (mbs.cur().uiCbp >> 4) as i32;
            let iCbpLuma = (mbs.cur().uiCbp & 15) as i32;

            mbs.cur_mut().iLumaDQp = (mbs.cur().uiLumaQp as i32) - (pSlice.uiLastMbQp as i32);
            let bFirstMbOfSlice = mbs.cur().iMbXY == iSliceFirstMbXY;
            WelsCabacMbDeltaQp(buf, mbs, &mut *pCabacCtx, bFirstMbOfSlice);
            pSlice.uiLastMbQp = mbs.cur().uiLumaQp;

            // S11.45: a shared borrow — every block below is read, and the
            // non-zero-count array is a sibling field, so the two coexist by
            // disjointness where the raw had to argue it.
            let pDct = &pMbCache.sDct;

            if uiMbType == MB_TYPE_INTRA16x16 {
                let dc_buf = &pDct.iLumaI16x16Dc[..];
                let iNonZeroCount = if pFuncList.pfGetNoneZeroCount.is_some()
                {
                    (pFuncList.pfGetNoneZeroCount.unwrap())(&pDct.iLumaI16x16Dc)
                } else {
                    pDct.iLumaI16x16Dc.iter().filter(|&&x| x != 0).count() as i32
                };

                WelsWriteBlockResidualCabac(buf, 
                    kpNonZeroCoeffCount,
                    &*mbs,
                    &mut *pCabacCtx,
                    ECtxBlockCat::LUMA_DC,
                    0,
                    iNonZeroCount as i16,
                    dc_buf,
                    15,
                );

                if iNonZeroCount != 0 {
                    mbs.cur_mut().iCbpDc |= 1;
                }

                if iCbpLuma != 0 {
                    for i in 0..16 {
                        let iIdx = g_kuiCache48CountScan4Idx[i] as i16;
                        let nz = kpNonZeroCoeffCount[iIdx as usize] as i16;
                        let block_buf = &pDct.iLumaBlock[i][..];

                        WelsWriteBlockResidualCabac(buf, 
                            kpNonZeroCoeffCount,
                            &*mbs,
                            &mut *pCabacCtx,
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
                        let nz = kpNonZeroCoeffCount[iIdx as usize] as i16;
                        let block_buf = &pDct.iLumaBlock[i][..];

                        WelsWriteBlockResidualCabac(buf, 
                            kpNonZeroCoeffCount,
                            &*mbs,
                            &mut *pCabacCtx,
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
                let mut iNonZeroCount = WelsCalNonZeroCount2x2Block(&pDct.iChromaDc[0]);
                let cb_dc_buf = &pDct.iChromaDc[0][..];
                if iNonZeroCount != 0 {
                    mbs.cur_mut().iCbpDc |= 0x2;
                }
                WelsWriteBlockResidualCabac(buf, 
                    kpNonZeroCoeffCount,
                    &*mbs,
                    &mut *pCabacCtx,
                    ECtxBlockCat::CHROMA_DC,
                    1,
                    iNonZeroCount as i16,
                    cb_dc_buf,
                    3,
                );

                iNonZeroCount = WelsCalNonZeroCount2x2Block(&pDct.iChromaDc[1]);
                let cr_dc_buf = &pDct.iChromaDc[1][..];
                if iNonZeroCount != 0 {
                    mbs.cur_mut().iCbpDc |= 0x4;
                }
                WelsWriteBlockResidualCabac(buf, 
                    kpNonZeroCoeffCount,
                    &*mbs,
                    &mut *pCabacCtx,
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
                        let nz = kpNonZeroCoeffCount[iIdx as usize] as i16;
                        let block_buf = &pDct.iChromaBlock[i][..];

                        WelsWriteBlockResidualCabac(buf, 
                            kpNonZeroCoeffCount,
                            &*mbs,
                            &mut *pCabacCtx,
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
                        let nz = kpNonZeroCoeffCount[iIdx as usize] as i16;
                        let block_buf = &pDct.iChromaBlock[4 + i][..];

                        WelsWriteBlockResidualCabac(buf, 
                            kpNonZeroCoeffCount,
                            &*mbs,
                            &mut *pCabacCtx,
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
            mbs.cur_mut().iLumaDQp = 0;
            mbs.cur_mut().uiLumaQp = pSlice.uiLastMbQp;
            let qp_idx = CLIP3_QP_0_51((mbs.cur().uiLumaQp as i32) + (uiChromaQpIndexOffset as i32));
            mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[qp_idx];
        }

        0
    }
}

// ============================================================================
// Top-Level Slice & Macroblock CABAC Entry Points
// ============================================================================

pub fn WelsInitSliceCabac(
    pEncCtx: &crate::encoder::encoder_context::sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut crate::encoder::vlc_encoder::BsWriter>,
) {
    /* alignment needed */
    let buf = pSliceBsBuf;
    BsAlign(buf, crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs));

    /* init cabac */
    let iCabacInitIdc = (*pSlice).iCabacInitIdc;
    crate::encoder::set_mb_syn_cabac::WelsCabacContextInit(
        // **S7.A2**: the callee takes `&sWelsEncCtx`. This body still holds the
        // context raw, so the shared borrow is formed here — and it is a
        // *whole-context* retag (F239). Nothing in this body derives a
        // field-precise exclusive pointer from `pEncCtx` across it, which the
        // F239 scan confirms tree-wide with its control seen red.
        &*pEncCtx,
        &mut (*pSlice).sCabacCtx,
        iCabacInitIdc,
    );
    // The arithmetic coder's cursor is now three offsets into this same
    // buffer, so the slice's start is the writer's position and nothing
    // becomes a pointer on the way. What used to be
    // `base.add(pBs.pos())` / `base.add(buf.len())` is what it always
    // meant: where this slice begins, and where the buffer ends.
    let end = buf.len();
    let kiBsPos = crate::encoder::svc_encode_slice::slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs).pos();
    WelsCabacEncodeInit(&mut (*pSlice).sCabacCtx, kiBsPos, end);
}

pub fn WelsSpatialWriteMbSynCabac(
    pEncCtx: &crate::encoder::encoder_context::sWelsEncCtx,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pSliceBsBuf: &mut [u8],
    _pCtxOutBs: &mut Option<&mut crate::encoder::vlc_encoder::BsWriter>,
) -> i32 {
    // **S11.1a reverses 4b's fence with the seam it guarded.** The fence
    // kept this signature stable so the entropy dispatch would not carry a
    // buffer; the bitstream pair now threads from the chain's top through
    // that dispatch (F272), and deriving here from the shared context is
    // the exact shape the conversion retires. The CABAC arm spends its bits
    // through `sCabacCtx` and touches no `BsWriter`, so the threaded writer
    // is unused here — the CAVLC arm is its consumer.
    let buf = pSliceBsBuf;
    let pCabacCtx = &mut pSlice.sCabacCtx;
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let uiMbType = mbs.cur().uiMbType;
    let pSliceHeadExt = &mut pSlice.sSliceHeaderExt;
    let uiNumRefIdxL0Active = (pSliceHeadExt.sSliceHeader.uiNumRefIdxL0Active as i32) - 1;
    let iSliceFirstMbXY = pSliceHeadExt.sSliceHeader.iFirstMbInSlice;
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");

    let uiChromaQpIndexOffset = crate::encoder::svc_encode_slice::layer_pps_ref(pEncCtx, &*pCurDqLayer)
        .expect("the layer's PPS is stamped")
        .uiChromaQpIndexOffset;
    let mut sMvd = SMVUnitXY::default();
    let mut iRet = 0;

    if mbs.cur().iMbXY > iSliceFirstMbXY {
        WelsCabacEncodeTerminate(buf, &mut *pCabacCtx, 0);
    }

    if IS_SKIP(mbs.cur().uiMbType) {
        mbs.cur_mut().uiLumaQp = pSlice.uiLastMbQp;
        let qp_idx = CLIP3_QP_0_51((mbs.cur().uiLumaQp as i32) + (uiChromaQpIndexOffset as i32));
        mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[qp_idx];
        WelsMbSkipCabac(buf, &mut *pCabacCtx, mbs, (*pEncCtx).eSliceType, 1);
    } else {
        if (*pEncCtx).eSliceType != EWelsSliceType::I_SLICE {
            WelsMbSkipCabac(buf, &mut *pCabacCtx, mbs, (*pEncCtx).eSliceType, 0);
        }

        WelsCabacMbType(buf, &mut *pCabacCtx, &*mbs, &mut *pMbCache, (*pEncCtx).eSliceType);

        if IS_INTRA(uiMbType) {
            if uiMbType == MB_TYPE_INTRA4x4 {
                WelsCabacMbIntra4x4PredMode(buf, &mut *pCabacCtx, &mut *pMbCache);
            }
            WelsCabacMbIntraChromaPredMode(buf, &mut *pCabacCtx, &*mbs, &mut *pMbCache);
            sMvd.iMvX = 0;
            sMvd.iMvY = 0;
            for i in 0..16 {
                mbs.cur_mut().sMvd[i].sAssignMv(sMvd);
            }
        } else if uiMbType == MB_TYPE_16x16 {
            if uiNumRefIdxL0Active > 0 {
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 0);
            }
            let cur_mv = mbs.cur().sMv[0];
            let pred_mv = (*pMbCache).sMbMvp[0];
            sMvd = WelsCabacMbMvd(buf, &mut *pCabacCtx, &*mbs, cur_mv, pred_mv, 0);

            for i in 0..16 {
                mbs.cur_mut().sMvd[i].sAssignMv(sMvd);
            }
        } else if uiMbType == MB_TYPE_16x8 {
            if uiNumRefIdxL0Active > 0 {
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 0);
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 12);
            }
            let cur_mv0 = mbs.cur().sMv[0];
            let pred_mv0 = (*pMbCache).sMbMvp[0];
            sMvd = WelsCabacMbMvd(buf, &mut *pCabacCtx, &*mbs, cur_mv0, pred_mv0, 0);
            for i in 0..8 {
                mbs.cur_mut().sMvd[i].sAssignMv(sMvd);
            }
            let cur_mv8 = mbs.cur().sMv[8];
            let pred_mv1 = (*pMbCache).sMbMvp[1];
            sMvd = WelsCabacMbMvd(buf, &mut *pCabacCtx, &*mbs, cur_mv8, pred_mv1, 8);
            for i in 8..16 {
                mbs.cur_mut().sMvd[i].sAssignMv(sMvd);
            }
        } else if uiMbType == MB_TYPE_8x16 {
            if uiNumRefIdxL0Active > 0 {
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 0);
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 2);
            }
            let cur_mv0 = mbs.cur().sMv[0];
            let pred_mv0 = (*pMbCache).sMbMvp[0];
            sMvd = WelsCabacMbMvd(buf, &mut *pCabacCtx, &*mbs, cur_mv0, pred_mv0, 0);
            let mut i = 0;
            while i < 16 {
                let cur = mbs.cur_mut();
                cur.sMvd[i].sAssignMv(sMvd);
                cur.sMvd[i + 1].sAssignMv(sMvd);
                i += 4;
            }
            let cur_mv2 = mbs.cur().sMv[2];
            let pred_mv1 = (*pMbCache).sMbMvp[1];
            sMvd = WelsCabacMbMvd(buf, &mut *pCabacCtx, &*mbs, cur_mv2, pred_mv1, 2);
            let mut i = 0;
            while i < 16 {
                let cur = mbs.cur_mut();
                cur.sMvd[i + 2].sAssignMv(sMvd);
                cur.sMvd[i + 3].sAssignMv(sMvd);
                i += 4;
            }
        } else if (uiMbType == MB_TYPE_8x8) || (uiMbType == MB_TYPE_8x8_REF0) {
            WelsCabacSubMbType(buf, &mut *pCabacCtx, mbs.cur());
            if uiNumRefIdxL0Active > 0 {
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 0);
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 2);
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 12);
                WelsCabacMbRef(buf, &mut *pCabacCtx, &mut *pMbCache, 14);
            }
            WelsCabacSubMbMvd(buf, &mut *pCabacCtx, mbs, &mut *pMbCache);
        }

        if uiMbType != MB_TYPE_INTRA16x16 {
            WelsCabacMbCbp(buf, &*mbs, &mut *pCabacCtx);
        }

        let pFuncList = (*pEncCtx).func_list();
        iRet = WelsWriteMbResidualCabac(
            buf,
            &*pFuncList,
            pSlice,
            mbs,
            uiChromaQpIndexOffset as u32,
        );
    }

    if !IS_INTRA(mbs.cur().uiMbType) {
        mbs.cur_mut().uiChromPredMode = 0;
    }

    iRet
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
        assert_eq!(WelsCalNonZeroCount2x2Block(&block_zero), 0);

        let block_mixed = [1i16, 0, -3, 0];
        assert_eq!(WelsCalNonZeroCount2x2Block(&block_mixed), 2);

        let block_full = [4i16, -2, 5, 1];
        assert_eq!(WelsCalNonZeroCount2x2Block(&block_full), 4);
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
        let end = buffer.len();
        WelsCabacEncodeInit(&mut cabac_ctx, 0, end);
        assert_eq!(cabac_ctx.m_uiRange, 510);
        assert_eq!(cabac_ctx.m_iLowBitCnt, 9);

        WelsCabacEncodeTerminate(&mut buffer, &mut cabac_ctx, 0);
        assert_eq!(cabac_ctx.m_uiRange, 508);
    }

    #[test]
    // unsafe-cat: C-ABI(test) — WelsCabacEncodeInit keeps its frozen boundary shape
    #[allow(unsafe_code)]
    fn test_cabac_mb_skip_logic() {
        let mut buffer = vec![0u8; 128];
        let mut cabac_ctx = SCabacCtx::default();
        let mut grid =
            crate::safe::mb_grid::MbArray::from_vec(vec![SMB::default()], crate::safe::mb_grid::MbDims::new(1, 1));
        grid.get_mut(0).uiMbType = MB_TYPE_SKIP;

        let end = buffer.len();
        unsafe { WelsCabacEncodeInit(&mut cabac_ctx, 0, end) };

        let mut mbs = crate::safe::mb_grid::MbWindow::whole(&mut grid, 0);
        WelsMbSkipCabac(
            &mut buffer,
            &mut cabac_ctx,
            &mut mbs,
            EWelsSliceType::P_SLICE,
            1,
        );

        let cur_mb = grid.get(0);
        assert_eq!(cur_mb.uiCbp, 0);
        assert_eq!(cur_mb.iCbpDc, 0);
        for i in 0..16 {
            assert_eq!(cur_mb.sMvd[i].iMvX, 0);
            assert_eq!(cur_mb.sMvd[i].iMvY, 0);
        }
    }
}
