#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Reference picture list management and Long-Term Reference (LTR) control.
//!
//! Translated from `codec/encoder/core/inc/ref_list_mgr_svc.h` and
//! `codec/encoder/core/src/ref_list_mgr_svc.cpp`.

use crate::*;

// ============================================================================
// Constants
// ============================================================================

pub const STR_ROOM: i32 = 1;
// `MAX_SHORT_REF_COUNT`, `MAX_TEMPORAL_LEVEL` and `MAX_GOP_SIZE` are defined once in
// `encoder_context.rs` from `wels_const.h`. This module previously had its own copies
// with MAX_SHORT_REF_COUNT = 16 (C++: 4) and MAX_TEMPORAL_LEVEL = 8 (C++: 4).
pub use crate::encoder::encoder_context::{MAX_GOP_SIZE, MAX_SHORT_REF_COUNT, MAX_TEMPORAL_LEVEL};
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const LONG_TERM_REF_NUM: i32 = 2;
pub const MAX_TEMPORAL_LAYER_NUM: usize = 4;
pub const MAX_DEPENDENCY_LAYER: usize = 4;
pub const MAX_REFERENCE_MMCO_COUNT_NUM: usize = 4;
pub const MAX_REFERENCE_REORDER_COUNT_NUM: usize = 2;

// Key frame & LTR feedback request types (matching codec_app_def.h)
pub const NO_RECOVERY_REQUSET: u32 = 0;
pub const LTR_RECOVERY_REQUEST: u32 = 1;
pub const IDR_RECOVERY_REQUEST: u32 = 2;
pub const NO_LTR_MARKING_FEEDBACK: u32 = 3;
pub const LTR_MARKING_SUCCESS: u32 = 4;
pub const LTR_MARKING_FAILED: u32 = 5;

// Reference picture reception status
pub const RECIEVE_UNKOWN: u8 = 0;
pub const RECIEVE_SUCCESS: u8 = 1;
pub const RECIEVE_FAILED: u8 = 2;

// H.264 MMCO (Memory Management Control Operation) types
pub const MMCO_END: i32 = 0;
pub const MMCO_SHORT2UNUSED: i32 = 1;
pub const MMCO_LONG2UNUSED: i32 = 2;
pub const MMCO_SHORT2LONG: i32 = 3;
pub const MMCO_SET_MAX_LONG: i32 = 4;
pub const MMCO_RESET: i32 = 5;
pub const MMCO_LONG: i32 = 6;

// ============================================================================
// Enumerations
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LTR_MARKING_PROCESS_MODE {
    LTR_DIRECT_MARK = 0,
    LTR_DELAY_MARK = 1,
}
pub use LTR_MARKING_PROCESS_MODE::*;

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum COMPARE_FRAME_NUM {
    FRAME_NUM_EQUAL = 0x01,
    FRAME_NUM_BIGGER = 0x02,
    FRAME_NUM_SMALLER = 0x04,
    FRAME_NUM_OVER_MAX = 0x08,
}
pub use COMPARE_FRAME_NUM::*;

pub use EWelsSliceType::*;
pub use crate::encoder::encoder_context::SRefList;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::picture::SScreenBlockFeatureStorage;
pub use crate::encoder::param_svc::SWelsSPS;
pub use crate::encoder::svc_encode_slice::SSliceHeader;
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::encoder_context::SLTRState;
pub use crate::encoder::encoder_context::SExpandPicFunc;
// ExpandPictureChroma_c is the shared common-layer routine
// (codec/common/src/expand_pic.cpp); the decoder module already holds the port.
use crate::decoder::decoder_core::ExpandPictureChroma_c;
use crate::common::expand_pic::PExpandPictureFunc;
pub use crate::encoder::encoder_context::SLogContext;
pub use crate::encoder::wels_preprocess::SVAAFrameInfoExt;
pub use crate::encoder::wels_preprocess::CWelsPreProcess;
pub use crate::encoder::param_svc::SSpatialLayerInternal;
pub use crate::encoder::wels_preprocess::SVAAFrameInfo;
pub use crate::encoder::param_svc::SWelsSvcCodingParam;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;

// ============================================================================
// Core Data Structures
// ============================================================================

/// Long-Term Reference (LTR) state machine.


/// Feature storage for screen content reference pictures.

/// Reconstructed reference picture representation in the DPB.

/// Reference picture lists for a spatial dependency layer.

/// Reference picture list reordering syntax element.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SReorderingSyntax {
    pub uiAbsDiffPicNumMinus1: u32,
    pub iLongTermPicNum: u16,
    pub uiReorderingOfPicNumsIdc: u16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRefPicListReorderSyntax {
    pub SReorderingSyntax: [SReorderingSyntax; MAX_REFERENCE_REORDER_COUNT_NUM],
}

/// Decoded reference picture marking MMCO command syntax.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMmcoRef {
    pub iMmcoType: i32,
    pub iShortFrameNum: i32,
    pub iDiffOfPicNum: i32,
    pub iLongTermPicNum: i32,
    pub iLongTermFrameIdx: i32,
    pub iMaxLongTermFrameIdx: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRefPicMarking {
    pub SMmcoRef: [SMmcoRef; MAX_REFERENCE_MMCO_COUNT_NUM],
    pub uiMmcoCount: u8,
    pub bNoOutputOfPriorPicsFlag: bool,
    pub bLongTermRefFlag: bool,
    pub bAdaptiveRefPicMarkingModeFlag: bool,
}














#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLTRRecoverRequest {
    pub uiFeedbackType: u32,
    pub uiIDRPicId: u32,
    pub iLastCorrectFrameNum: i32,
    pub iCurrentFrameNum: i32,
    pub iLayerId: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLTRMarkingFeedback {
    pub uiFeedbackType: u32,
    pub uiIDRPicId: u32,
    pub iLTRFrameNum: i32,
    pub iLayerId: i32,
}


/// Master encoder context state for reference list management.

// ============================================================================
// Helper Utilities & Logging
// ============================================================================

#[inline]
/// Expand the borders of a reference picture.
///
/// Matches `ExpandReferencingPicture` in `codec/common/src/expand_pic.cpp:388`.
/// `pExpChrom` is a two-entry table indexed by whether the chroma width is
/// 16-aligned, not a single function: the port previously took one scalar and used
/// it for both planes, which ignored the alignment-specialised variant and the
/// sub-16 fallback below.
pub unsafe fn ExpandReferencingPicture(
    pData: [*mut u8; 3],
    iWidth: i32,
    iHeight: i32,
    iStride: [i32; 3],
    pExpLuma: Option<PExpandPictureFunc>,
    pExpChrom: [Option<PExpandPictureFunc>; 2],
) {
    let pPicY = pData[0];
    let pPicCb = pData[1];
    let pPicCr = pData[2];
    let kiWidthY = iWidth;
    let kiHeightY = iHeight;
    let kiWidthUV = kiWidthY >> 1;
    let kiHeightUV = kiHeightY >> 1;

    if let Some(fLuma) = pExpLuma {
        fLuma(pPicY, iStride[0], kiWidthY, kiHeightY);
    }
    if kiWidthUV >= 16 {
        // fix coding picture size as 16x16
        let kbChrAligned = (kiWidthUV & 0x0F) == 0; // chroma planes: (16+iWidthUV) & 15
        if let Some(fChroma) = pExpChrom[kbChrAligned as usize] {
            fChroma(pPicCb, iStride[1], kiWidthUV, kiHeightUV);
            fChroma(pPicCr, iStride[2], kiWidthUV, kiHeightUV);
        }
    } else {
        // fix coding picture size as 16x16
        ExpandPictureChroma_c(pPicCb, iStride[1], kiWidthUV, kiHeightUV);
        ExpandPictureChroma_c(pPicCr, iStride[2], kiWidthUV, kiHeightUV);
    }
}

// ============================================================================
// Global Reference Picture List Lifecycle Functions
// ============================================================================

/// Reset LTR marking, recovery, and feedback state to defaults.
pub unsafe fn ResetLtrState(pLtr: *mut SLTRState) {
    if pLtr.is_null() {
        return;
    }
    (*pLtr).bReceivedT0LostFlag = false;
    (*pLtr).iLastRecoverFrameNum = 0;
    (*pLtr).iLastCorFrameNumDec = -1;
    (*pLtr).iCurFrameNumInDec = -1;

    // LTR mark
    (*pLtr).iLTRMarkMode = LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32;
    (*pLtr).iLTRMarkSuccessNum = 0;
    (*pLtr).bLTRMarkingFlag = false;
    (*pLtr).bLTRMarkEnable = false;
    (*pLtr).iCurLtrIdx = 0;
    (*pLtr).iLastLtrIdx = [0; MAX_TEMPORAL_LAYER_NUM];
    (*pLtr).uiLtrMarkInterval = 0;

    // LTR mark feedback
    (*pLtr).uiLtrMarkState = NO_LTR_MARKING_FEEDBACK;
    (*pLtr).iLtrMarkFbFrameNum = -1;
}

/// Reset active reference picture lists for current spatial layer.
pub unsafe fn WelsResetRefList(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }

    for i in 0..=(MAX_SHORT_REF_COUNT) {
        (*pRefList).pShortRefList[i] = std::ptr::null_mut();
    }
    let ltrRefNum = if !(*pCtx).pSvcParam.is_null() {
        (*(*pCtx).pSvcParam).iLTRRefNum as usize
    } else {
        0
    };
    for i in 0..=(ltrRefNum) {
        if i <= MAX_REF_PIC_COUNT {
            (*pRefList).pLongRefList[i] = std::ptr::null_mut();
        }
    }
    let numRefFrame = if !(*pCtx).pSvcParam.is_null() {
        (*(*pCtx).pSvcParam).iNumRefFrame as usize
    } else {
        0
    };
    for i in 0..=(numRefFrame) {
        if i <= MAX_REF_PIC_COUNT {
            let pPic = (*pRefList).pRef[i];
            if !pPic.is_null() {
                (*pPic).SetUnref();
            }
        }
    }

    (*pRefList).uiLongRefCount = 0;
    (*pRefList).uiShortRefCount = 0;
    (*pRefList).pNextBuffer = (*pRefList).pRef[0];
}

/// Remove a long-term reference entry by index from pLongRefList.
pub unsafe fn DeleteLTRFromLongList(pCtx: *mut sWelsEncCtx, iIdx: i32) {
    if pCtx.is_null() {
        return;
    }
    let pRefList = *(*pCtx).ppRefPicListExt.add(((*pCtx).uiDependencyId as usize) as usize);
    if pRefList.is_null() {
        return;
    }
    let count = (*pRefList).uiLongRefCount as i32;
    let mut k = iIdx;
    while k < count - 1 {
        (*pRefList).pLongRefList[k as usize] = (*pRefList).pLongRefList[(k + 1) as usize];
        k += 1;
    }
    if k >= 0 && (k as usize) <= MAX_REF_PIC_COUNT {
        (*pRefList).pLongRefList[k as usize] = std::ptr::null_mut();
    }
    if (*pRefList).uiLongRefCount > 0 {
        (*pRefList).uiLongRefCount -= 1;
    }
}

/// Remove a short-term reference entry by index from pShortRefList.
pub unsafe fn DeleteSTRFromShortList(pCtx: *mut sWelsEncCtx, iIdx: i32) {
    if pCtx.is_null() {
        return;
    }
    let pRefList = *(*pCtx).ppRefPicListExt.add(((*pCtx).uiDependencyId as usize) as usize);
    if pRefList.is_null() {
        return;
    }
    let count = (*pRefList).uiShortRefCount as i32;
    let mut k = iIdx;
    while k < count - 1 {
        (*pRefList).pShortRefList[k as usize] = (*pRefList).pShortRefList[(k + 1) as usize];
        k += 1;
    }
    if k >= 0 && (k as usize) <= MAX_SHORT_REF_COUNT {
        (*pRefList).pShortRefList[k as usize] = std::ptr::null_mut();
    }
    if (*pRefList).uiShortRefCount > 0 {
        (*pRefList).uiShortRefCount -= 1;
    }
}

/// Unreferences non-scene LTR frames when current frame is marked as Scene LTR.
pub unsafe fn DeleteNonSceneLTR(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
        return;
    }
    let pRefList = *(*pCtx).ppRefPicListExt.add(((*pCtx).uiDependencyId as usize) as usize);
    if pRefList.is_null() {
        return;
    }
    let numRef = (*(*pCtx).pSvcParam).iNumRefFrame;
    let mut i = 0;
    while i < numRef {
        let pRef = (*pRefList).pLongRefList[i as usize];
        if !pRef.is_null()
            && (*pRef).bUsedAsRef
            && (*pRef).bIsLongRef
            && (!(*pRef).bIsSceneLTR)
            && ((*pCtx).uiTemporalId < (*pRef).uiTemporalId || (*pCtx).bCurFrameMarkedAsSceneLtr)
        {
            (*pRef).SetUnref();
            DeleteLTRFromLongList(pCtx, i);
            i -= 1;
        }
        i += 1;
    }
}

/// Modular frame number distance comparison arithmetic.
pub fn CompareFrameNum(iFrameNumA: i32, iFrameNumB: i32, iMaxFrameNumPlus1: i32) -> i32 {
    if iFrameNumA > iMaxFrameNumPlus1 || iFrameNumB > iMaxFrameNumPlus1 {
        return -2;
    }
    let iDiffAB = (iFrameNumA as i64 - iFrameNumB as i64).abs();
    let iDiffMin = iDiffAB;
    if iDiffMin == 0 {
        return COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32;
    }

    let iNumA = ((iFrameNumA + iMaxFrameNumPlus1) as i64 - iFrameNumB as i64).abs();
    if iNumA == 0 {
        return COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32;
    } else if iDiffMin > iNumA {
        return COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32;
    }

    let iNumB = ((iFrameNumB + iMaxFrameNumPlus1) as i64 - iFrameNumA as i64).abs();
    if iNumB == 0 {
        return COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32;
    } else if iDiffMin > iNumB {
        return COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32;
    }

    if iFrameNumA > iFrameNumB {
        COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
    } else {
        COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32
    }
}

/// Purges unacknowledged or invalid LTR frames based on decoder feedback.
pub unsafe fn DeleteInvalidLTR(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSps.is_null() || (*pCtx).pSvcParam.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let iMaxFrameNumPlus1 = 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;
    let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    for i in 0..LONG_TERM_REF_NUM {
        let pPic = (*pRefList).pLongRefList[i as usize];
        if !pPic.is_null() {
            let cond1 = CompareFrameNum((*pPic).iFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                && ((CompareFrameNum((*pPic).iFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0);

            if cond1 {
                (*pPic).SetUnref();
                DeleteLTRFromLongList(pCtx, i);
                pLtr.bLTRMarkEnable = true;
                if (*pRefList).uiLongRefCount == 0 {
                    pParamInternal.bEncCurFrmAsIdrFlag = true;
                }
            } else {
                let cond2 = CompareFrameNum((*pPic).iMarkFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                    == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                    && ((CompareFrameNum((*pPic).iMarkFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                        & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                        != 0)
                    && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32;

                if cond2 {
                    (*pPic).SetUnref();
                    DeleteLTRFromLongList(pCtx, i);
                    pLtr.bLTRMarkEnable = true;
                    if (*pRefList).uiLongRefCount == 0 {
                        pParamInternal.bEncCurFrmAsIdrFlag = true;
                    }
                }
            }
        }
    }
}

/// Handles asynchronous decoder confirmation or failure feedback for LTR marking.
pub unsafe fn HandleLTRMarkFeedback(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    if pLtr.uiLtrMarkState == LTR_MARKING_SUCCESS {
        for i in 0..((*pRefList).uiLongRefCount as i32) {
            let pPic = (*pRefList).pLongRefList[i as usize];
            if !pPic.is_null()
                && (*pPic).iFrameNum == pLtr.iLtrMarkFbFrameNum
                && (*pPic).uiRecieveConfirmed != RECIEVE_SUCCESS
            {
                (*pPic).uiRecieveConfirmed = RECIEVE_SUCCESS;
                if !(*pCtx).pVaa.is_null() {
                    (*(*pCtx).pVaa).uiValidLongTermPicIdx = (*pPic).iLongTermPicNum as u8;
                }
                pLtr.iCurFrameNumInDec = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastRecoverFrameNum = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastCorFrameNumDec = pLtr.iLtrMarkFbFrameNum;

                let mut j = 0;
                while j < (*pRefList).uiLongRefCount as i32 {
                    let pLong = (*pRefList).pLongRefList[j as usize];
                    if !pLong.is_null() && (*pLong).iLongTermPicNum != pLtr.iCurLtrIdx {
                        (*pLong).SetUnref();
                        DeleteLTRFromLongList(pCtx, j);
                    } else {
                        j += 1;
                    }
                }

                pLtr.iLTRMarkSuccessNum += 1;
                pLtr.iCurLtrIdx = (pLtr.iCurLtrIdx + 1) % LONG_TERM_REF_NUM;
                pLtr.iLTRMarkMode = if pLtr.iLTRMarkSuccessNum >= LONG_TERM_REF_NUM {
                    LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32
                } else {
                    LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32
                };
                pLtr.bLTRMarkEnable = true;
                break;
            }
        }
        pLtr.uiLtrMarkState = NO_LTR_MARKING_FEEDBACK;
    } else if pLtr.uiLtrMarkState == LTR_MARKING_FAILED {
        for i in 0..((*pRefList).uiLongRefCount as i32) {
            let pPic = (*pRefList).pLongRefList[i as usize];
            if !pPic.is_null() && (*pPic).iFrameNum == pLtr.iLtrMarkFbFrameNum {
                (*pPic).SetUnref();
                DeleteLTRFromLongList(pCtx, i);
                break;
            }
        }
        pLtr.uiLtrMarkState = NO_LTR_MARKING_FEEDBACK;
        pLtr.bLTRMarkEnable = true;

        if pLtr.iLTRMarkSuccessNum == 0 {
            pParamInternal.bEncCurFrmAsIdrFlag = true;
        }
    }
}

/// Executes promotion and movement of frames from short-term to long-term lists.
pub unsafe fn LTRMarkProcess(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pSps.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let gopSize = (*(*pCtx).pSvcParam).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;
    let mut i = 0usize;
    let mut bMoveLtrFromShortToLong = false;
    let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    if (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
        i = 0;
        let pShort = (*pRefList).pShortRefList[i];
        if !pShort.is_null() {
            (*pShort).uiRecieveConfirmed = RECIEVE_SUCCESS;
        }
    } else if pLtr.bLTRMarkingFlag {
        if !(*pCtx).pVaa.is_null() {
            (*(*pCtx).pVaa).uiMarkLongTermPicIdx = pLtr.iCurLtrIdx as u8;
        }

        if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 {
            for idx in 0..((*pRefList).uiShortRefCount as usize) {
                let pShort = (*pRefList).pShortRefList[idx];
                if !pShort.is_null() {
                    if CompareFrameNum(
                        pParamInternal.iFrameNum,
                        (*pShort).iFrameNum + iGoPFrameNumInterval,
                        iMaxFrameNumPlus1,
                    ) == COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32
                    {
                        i = idx;
                        break;
                    }
                }
            }
        }
    }

    if (*pCtx).eSliceType == EWelsSliceType::I_SLICE || pLtr.bLTRMarkingFlag {
        let pShort = (*pRefList).pShortRefList[i];
        if !pShort.is_null() {
            (*pShort).bIsLongRef = true;
            (*pShort).iLongTermPicNum = pLtr.iCurLtrIdx;
            (*pShort).iMarkFrameNum = pParamInternal.iFrameNum;
        }
    }

    if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32
        && (*pCtx).eSliceType != EWelsSliceType::I_SLICE
        && !pLtr.bLTRMarkingFlag
    {
        for j in 0..((*pRefList).uiShortRefCount as usize) {
            let pShort = (*pRefList).pShortRefList[j];
            if !pShort.is_null() && (*pShort).bIsLongRef {
                i = j;
                bMoveLtrFromShortToLong = true;
                break;
            }
        }
    }

    if (pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 && pLtr.bLTRMarkingFlag)
        || ((pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32) && bMoveLtrFromShortToLong)
    {
        let tid = (*pCtx).uiTemporalId as usize;
        if uiDid < MAX_DEPENDENCY_LAYER && tid < MAX_TEMPORAL_LEVEL {
            (*pCtx).bRefOfCurTidIsLtr[uiDid][tid] = true;
        }

        let longCount = (*pRefList).uiLongRefCount as usize;
        if longCount > 0 {
            for k in (1..=longCount).rev() {
                if k <= MAX_REF_PIC_COUNT {
                    (*pRefList).pLongRefList[k] = (*pRefList).pLongRefList[k - 1];
                }
            }
        }
        (*pRefList).pLongRefList[0] = (*pRefList).pShortRefList[i];
        (*pRefList).uiLongRefCount += 1;
        if (*pRefList).uiLongRefCount as i32 > (*(*pCtx).pSvcParam).iLTRRefNum {
            let lastIdx = ((*pRefList).uiLongRefCount - 1) as usize;
            let pLast = (*pRefList).pLongRefList[lastIdx];
            if !pLast.is_null() {
                (*pLast).SetUnref();
            }
            DeleteLTRFromLongList(pCtx, lastIdx as i32);
        }
        DeleteSTRFromShortList(pCtx, i as i32);
    }
}

/// Executes promotion of screen content references to long-term reference slots.
pub unsafe fn LTRMarkProcessScreen(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pDecPic.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let iLtrIdx = (*(*pCtx).pDecPic).iLongTermPicNum;
    if !(*pCtx).pVaa.is_null() {
        (*(*pCtx).pVaa).uiMarkLongTermPicIdx = (*(*pCtx).pDecPic).iLongTermPicNum as u8;
    }

    if iLtrIdx >= 0 && (iLtrIdx as usize) < MAX_REF_PIC_COUNT {
        let pLong = (*pRefList).pLongRefList[iLtrIdx as usize];
        if !pLong.is_null() {
            (*pLong).SetUnref();
        } else {
            (*pRefList).uiLongRefCount += 1;
        }
        (*pRefList).pLongRefList[iLtrIdx as usize] = (*pCtx).pDecPic;
    }
}

/// Pre-allocates destination frame buffer pointer pDecPic for upcoming reconstruction.
pub unsafe fn PrefetchNextBuffer(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let kiNumRef = (*(*pCtx).pSvcParam).iNumRefFrame;

    (*pRefList).pNextBuffer = std::ptr::null_mut();
    for i in 0..=(kiNumRef as usize) {
        if i <= MAX_REF_PIC_COUNT {
            let pPic = (*pRefList).pRef[i];
            if !pPic.is_null() && !(*pPic).bUsedAsRef {
                (*pRefList).pNextBuffer = pPic;
                break;
            }
        }
    }

    if (*pRefList).pNextBuffer.is_null() && (*pRefList).uiShortRefCount > 0 {
        let lastIdx = ((*pRefList).uiShortRefCount - 1) as usize;
        (*pRefList).pNextBuffer = (*pRefList).pShortRefList[lastIdx];
        if !(*pRefList).pNextBuffer.is_null() {
            (*(*pRefList).pNextBuffer).SetUnref();
        }
    }

    (*pCtx).pDecPic = (*pRefList).pNextBuffer;
}

/// Updates reference picture list after current frame reconstruction.
pub unsafe fn WelsUpdateRefList(pCtx: *mut sWelsEncCtx) -> bool {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() || (*pCtx).pSvcParam.is_null() {
        return false;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() || (*pRefList).pRef[0].is_null() {
        return false;
    }

    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let pParamD = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];
    let kuiTid = (*pCtx).uiTemporalId;
    let kuiDid = (*pCtx).uiDependencyId;
    let keSliceType = (*pCtx).eSliceType;

    if !(*pCtx).pDecPic.is_null() {
        let pDecPic = (*pCtx).pDecPic;
        if pParamD.iHighestTemporalId == 0 || (kuiTid as i32) < pParamD.iHighestTemporalId as i32 {
            if !(*pCtx).pFuncList.is_null() {
                ExpandReferencingPicture(
                    (*pDecPic).pData,
                    (*pDecPic).iWidthInPixel,
                    (*pDecPic).iHeightInPixel,
                    (*pDecPic).iLineSize,
                    (*(*pCtx).pFuncList).sExpandPicFunc.pfExpandLumaPicture,
                    (*(*pCtx).pFuncList).sExpandPicFunc.pfExpandChromaPicture,
                );
            }
        }

        (*pDecPic).uiTemporalId = kuiTid;
        (*pDecPic).uiSpatialId = kuiDid;
        (*pDecPic).iFrameNum = pParamD.iFrameNum;
        (*pDecPic).iFramePoc = pParamD.iPOC;
        (*pDecPic).uiRecieveConfirmed = RECIEVE_UNKOWN;
        (*pDecPic).bUsedAsRef = true;

        let shortCount = (*pRefList).uiShortRefCount as usize;
        for iRefIdx in (0..shortCount).rev() {
            if iRefIdx + 1 <= MAX_SHORT_REF_COUNT {
                (*pRefList).pShortRefList[iRefIdx + 1] = (*pRefList).pShortRefList[iRefIdx];
            }
        }
        (*pRefList).pShortRefList[0] = pDecPic;
        (*pRefList).uiShortRefCount += 1;
    }

    if keSliceType == EWelsSliceType::P_SLICE {
        if (*pCtx).uiTemporalId == 0 {
            if (*(*pCtx).pSvcParam).bEnableLongTermReference {
                LTRMarkProcess(pCtx);
                DeleteInvalidLTR(pCtx);
                HandleLTRMarkFeedback(pCtx);

                pLtr.bReceivedT0LostFlag = false;
                pLtr.bLTRMarkingFlag = false;
                pLtr.uiLtrMarkInterval += 1;
            }

            let mut i = ((*pRefList).uiShortRefCount as i32) - 1;
            while i > 0 {
                let pShort = (*pRefList).pShortRefList[i as usize];
                if !pShort.is_null() {
                    (*pShort).SetUnref();
                }
                DeleteSTRFromShortList(pCtx, i);
                i -= 1;
            }
            if (*pRefList).uiShortRefCount > 0 {
                let p0 = (*pRefList).pShortRefList[0];
                if !p0.is_null() && ((*p0).uiTemporalId > 0 || (*p0).iFrameNum != pParamD.iFrameNum) {
                    (*p0).SetUnref();
                    DeleteSTRFromShortList(pCtx, 0);
                }
            }
        }
    } else {
        if (*(*pCtx).pSvcParam).bEnableLongTermReference {
            LTRMarkProcess(pCtx);

            pLtr.iCurLtrIdx = (pLtr.iCurLtrIdx + 1) % LONG_TERM_REF_NUM;
            pLtr.iLTRMarkSuccessNum = 1;
            pLtr.bLTRMarkEnable = true;
            pLtr.uiLtrMarkInterval = 0;

            if !(*pCtx).pVaa.is_null() {
                (*(*pCtx).pVaa).uiValidLongTermPicIdx = 0;
                (*(*pCtx).pVaa).uiMarkLongTermPicIdx = 0;
            }
        }
    }

    if !(*pCtx).pReferenceStrategy.is_null() {
        // C++ dispatches virtually here (ref_list_mgr_svc.cpp:1041/1057/1073 —
        // PrefetchNextBuffer / UpdateSrcPicList /
        // UpdateSrcPicListLosslessScreenRefSelectionWithLtr), through the C-style
        // vtable above.
        IWelsReferenceStrategy::EndofUpdateRefList(
            (*pCtx).pReferenceStrategy as *mut IWelsReferenceStrategy,
        );
    }
    true
}

/// Checks whether candidate frame number is already occupied in LTR list.
pub unsafe fn CheckCurMarkFrameNumUsed(pCtx: *mut sWelsEncCtx) -> bool {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pSps.is_null() {
        return false;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pLtr = &*(*pCtx).pLtr.add((uiDid) as usize);
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return false;
    }
    let gopSize = (*(*pCtx).pSvcParam).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;
    let pParamInternal = &(*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    for i in 0..((*pRefList).uiLongRefCount as usize) {
        let pLong = (*pRefList).pLongRefList[i];
        if !pLong.is_null() {
            let cond1 = pParamInternal.iFrameNum == (*pLong).iFrameNum
                && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32;
            let cond2 = CompareFrameNum(
                pParamInternal.iFrameNum + iGoPFrameNumInterval,
                (*pLong).iFrameNum,
                iMaxFrameNumPlus1,
            ) == COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32
                && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32;

            if cond1 || cond2 {
                return false;
            }
        }
    }
    true
}

/// Replicates base slice header reference marking syntax across all slices.
pub unsafe fn WelsMarkMMCORefInfoWithBase(
    ppSliceList: *mut *mut SSlice,
    pBaseSlice: *mut SSlice,
    kiCountSliceNum: i32,
) {
    if ppSliceList.is_null() || pBaseSlice.is_null() {
        return;
    }
    let pBaseMarking = &(*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
    for iSliceIdx in 0..kiCountSliceNum {
        let pSlice = *ppSliceList.add(iSliceIdx as usize);
        if !pSlice.is_null() {
            (*pSlice).sSliceHeaderExt.sSliceHeader.sRefMarking = *pBaseMarking;
        }
    }
}

/// Constructs MMCO reference marking commands for slice headers.
pub unsafe fn WelsMarkMMCORefInfo(
    pCtx: *mut sWelsEncCtx,
    pLtr: *mut SLTRState,
    ppSliceList: *mut *mut SSlice,
    kiCountSliceNum: i32,
) {
    if pCtx.is_null() || pLtr.is_null() || ppSliceList.is_null() || kiCountSliceNum <= 0 {
        return;
    }
    let pBaseSlice = *ppSliceList.add(0);
    if pBaseSlice.is_null() {
        return;
    }
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
    let gopSize = (*(*pCtx).pSvcParam).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };

    *pRefPicMark = SRefPicMarking::default();

    if (*(*pCtx).pSvcParam).bEnableLongTermReference && (*pLtr).bLTRMarkingFlag {
        if (*pLtr).iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32 {
            let count0 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count0].iMaxLongTermFrameIdx = LONG_TERM_REF_NUM - 1;
            pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SET_MAX_LONG;
            pRefPicMark.uiMmcoCount += 1;

            let count1 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count1].iDiffOfPicNum = iGoPFrameNumInterval;
            pRefPicMark.SMmcoRef[count1].iMmcoType = MMCO_SHORT2UNUSED;
            pRefPicMark.uiMmcoCount += 1;

            let count2 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count2].iLongTermFrameIdx = (*pLtr).iCurLtrIdx;
            pRefPicMark.SMmcoRef[count2].iMmcoType = MMCO_LONG;
            pRefPicMark.uiMmcoCount += 1;
        } else if (*pLtr).iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 {
            let count0 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count0].iDiffOfPicNum = iGoPFrameNumInterval;
            pRefPicMark.SMmcoRef[count0].iLongTermFrameIdx = (*pLtr).iCurLtrIdx;
            pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SHORT2LONG;
            pRefPicMark.uiMmcoCount += 1;
        }
    }

    WelsMarkMMCORefInfoWithBase(ppSliceList, pBaseSlice, kiCountSliceNum);
}

/// Evaluates LTR marking criteria and populates slice header MMCO commands.
pub unsafe fn WelsMarkPic(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() || (*pCtx).pSvcParam.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let kiCountSliceNum = (*(*pCtx).pCurDqLayer).iMaxSliceNum;

    if (*(*pCtx).pSvcParam).bEnableLongTermReference && pLtr.bLTRMarkEnable && (*pCtx).uiTemporalId == 0 {
        if !pLtr.bReceivedT0LostFlag
            && pLtr.uiLtrMarkInterval > (*(*pCtx).pSvcParam).iLtrMarkPeriod as u32
            && CheckCurMarkFrameNumUsed(pCtx)
        {
            pLtr.bLTRMarkingFlag = true;
            pLtr.bLTRMarkEnable = false;
            pLtr.uiLtrMarkInterval = 0;
            for i in 0..MAX_TEMPORAL_LAYER_NUM {
                if ((*pCtx).uiTemporalId as usize) < i || (*pCtx).uiTemporalId == 0 {
                    pLtr.iLastLtrIdx[i] = pLtr.iCurLtrIdx;
                }
            }
        } else {
            pLtr.bLTRMarkingFlag = false;
        }
    }

    WelsMarkMMCORefInfo(
        pCtx,
        pLtr,
        (*(*pCtx).pCurDqLayer).ppSliceInLayer,
        kiCountSliceNum,
    );
}

/// Evaluates LTR recovery request feedback packets from decoder.
pub unsafe fn FilterLTRRecoveryRequest(
    pCtx: *mut sWelsEncCtx,
    pLTRRecoverRequest: *mut SLTRRecoverRequest,
) -> i32 {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || pLTRRecoverRequest.is_null() {
        return 0;
    }
    if !(*(*pCtx).pSvcParam).bEnableLongTermReference {
        for iDid in 0..((*(*pCtx).pSvcParam).iSpatialLayerNum as usize) {
            let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[iDid];
            pParamInternal.bEncCurFrmAsIdrFlag = true;
        }
    } else {
        let pRequest = pLTRRecoverRequest;
        let iLayerId = (*pRequest).iLayerId;
        if iLayerId < 0 || iLayerId >= (*(*pCtx).pSvcParam).iSpatialLayerNum {
            return 0;
        }

        let pLtr = &mut *(*pCtx).pLtr.add((iLayerId as usize) as usize);
        let iMaxFrameNumPlus1 = 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;
        let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[iLayerId as usize];

        if (*pRequest).uiFeedbackType == LTR_RECOVERY_REQUEST && (*pRequest).uiIDRPicId == pParamInternal.uiIdrPicId as u32 {
            if (*pRequest).iLastCorrectFrameNum == -1 {
                pParamInternal.bEncCurFrmAsIdrFlag = true;
                return 1;
            } else if (*pRequest).iCurrentFrameNum == -1 {
                pLtr.bReceivedT0LostFlag = true;
                return 1;
            } else {
                let cond1 = (CompareFrameNum(pLtr.iLastRecoverFrameNum, (*pRequest).iLastCorrectFrameNum, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0;
                let cond2 = ((CompareFrameNum(pLtr.iLastRecoverFrameNum, (*pRequest).iCurrentFrameNum, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0)
                    && CompareFrameNum(pLtr.iLastRecoverFrameNum, (*pRequest).iLastCorrectFrameNum, iMaxFrameNumPlus1)
                        == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32;

                if cond1 || cond2 {
                    pLtr.bReceivedT0LostFlag = true;
                    pLtr.iLastCorFrameNumDec = (*pRequest).iLastCorrectFrameNum;
                    pLtr.iCurFrameNumInDec = (*pRequest).iCurrentFrameNum;
                }
            }
        }
    }
    1
}

/// Updates LTR marking confirmation or failure feedback from decoder.
pub unsafe fn FilterLTRMarkingFeedback(
    pCtx: *mut sWelsEncCtx,
    pLTRMarkingFeedback: *mut SLTRMarkingFeedback,
) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || pLTRMarkingFeedback.is_null() {
        return;
    }
    let iLayerId = (*pLTRMarkingFeedback).iLayerId;
    if iLayerId < 0 || iLayerId >= (*(*pCtx).pSvcParam).iSpatialLayerNum {
        return;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((iLayerId as usize) as usize);
    if (*(*pCtx).pSvcParam).bEnableLongTermReference {
        let pParamInternal = &mut (*(*pCtx).pSvcParam).sDependencyLayers[iLayerId as usize];
        if (*pLTRMarkingFeedback).uiIDRPicId == pParamInternal.uiIdrPicId as u32
            && ((*pLTRMarkingFeedback).uiFeedbackType == LTR_MARKING_SUCCESS
                || (*pLTRMarkingFeedback).uiFeedbackType == LTR_MARKING_FAILED)
        {
            pLtr.uiLtrMarkState = (*pLTRMarkingFeedback).uiFeedbackType;
            pLtr.iLtrMarkFbFrameNum = (*pLTRMarkingFeedback).iLTRFrameNum;
        }
    }
}

/// Builds active reference picture list pRefList0 for motion estimation.
pub unsafe fn WelsBuildRefList(
    pCtx: *mut sWelsEncCtx,
    kiPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return false;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() {
        return false;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let kiNumRef = (*(*pCtx).pSvcParam).iNumRefFrame;
    let kuiTid = (*pCtx).uiTemporalId;
    let pParamD = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    (*pCtx).iNumRef0 = 0;
    if (*pCtx).eSliceType != EWelsSliceType::I_SLICE {
        if (*(*pCtx).pSvcParam).bEnableLongTermReference && pLtr.bReceivedT0LostFlag && (*pCtx).uiTemporalId == 0 {
            for i in 0..((*pRefList).uiLongRefCount as usize) {
                let pLong = (*pRefList).pLongRefList[i];
                if !pLong.is_null() && (*pLong).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    let numRef0 = (*pCtx).iNumRef0 as usize;
                    (*(*pCtx).pCurDqLayer).pRefOri[numRef0] = pLong;
                    (*pCtx).pRefList0[numRef0] = pLong;
                    (*pCtx).iNumRef0 += 1;
                    pLtr.iLastRecoverFrameNum = pParamD.iFrameNum;
                    break;
                }
            }
        } else {
            for i in 0..((*pRefList).uiShortRefCount as usize) {
                let pRef = (*pRefList).pShortRefList[i];
                if !pRef.is_null()
                    && (*pRef).bUsedAsRef
                    && (*pRef).iFramePoc >= 0
                    && (*pRef).uiTemporalId <= kuiTid
                {
                    let numRef0 = (*pCtx).iNumRef0 as usize;
                    (*(*pCtx).pCurDqLayer).pRefOri[numRef0] = pRef;
                    (*pCtx).pRefList0[numRef0] = pRef;
                    (*pCtx).iNumRef0 += 1;
                }
            }
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(&mut *(*pCtx).pLtr.add((uiDid) as usize));
        for k in 0..MAX_TEMPORAL_LEVEL {
            (*pCtx).bRefOfCurTidIsLtr[uiDid][k] = false;
        }
        (*pCtx).pRefList0[0] = std::ptr::null_mut();
    }

    if (*pCtx).iNumRef0 as i32 > kiNumRef {
        (*pCtx).iNumRef0 = kiNumRef as u8;
    }
    (*pCtx).iNumRef0 > 0 || (*pCtx).eSliceType == EWelsSliceType::I_SLICE
}

/// Invokes VPP UpdateBlockIdcForScreen to update static block map.
pub unsafe fn UpdateBlockStatic(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pVaa.is_null() || (*pCtx).pVpp.is_null() {
        return;
    }
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
        let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
    for idx in 0..((*pCtx).iNumRef0 as usize) {
        let pRef = (*pCtx).pRefList0[idx];
        if !pRef.is_null() && (*pVaaExt).iVaaBestRefFrameNum != (*pRef).iFrameNum {
            (*(*pCtx).pVpp).UpdateBlockIdcForScreen(
                (*pVaaExt).pVaaBestBlockStaticIdc,
                pRef,
                (*pCtx).pEncPic,
            );
        }
    }
}

/// Serializes slice header reference picture reordering syntax and marking flags.
pub unsafe fn WelsUpdateSliceHeaderSyntax(
    pCtx: *mut sWelsEncCtx,
    iAbsDiffPicNumMinus1: i32,
    ppSliceList: *mut *mut SSlice,
    uiFrameType: i32,
) {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() || ppSliceList.is_null() {
        return;
    }
    let kiCountSliceNum = (*(*pCtx).pCurDqLayer).iMaxSliceNum;
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pLtr = &*(*pCtx).pLtr.add((uiDid) as usize);

    for iIdx in 0..kiCountSliceNum {
        let pSlice = *ppSliceList.add(iIdx as usize);
        if pSlice.is_null() {
            continue;
        }
        let pSliceHdr = &mut (*pSlice).sSliceHeaderExt.sSliceHeader;
        let pRefReorder = &mut pSliceHdr.sRefReordering;
        let pRefPicMark = &mut pSliceHdr.sRefMarking;

        pSliceHdr.uiRefCount = (*pCtx).iNumRef0 as u8;
        if (*pCtx).iNumRef0 > 0 {
            let pRef0 = (*pCtx).pRefList0[0];
            let isLongRef = !pRef0.is_null() && (*pRef0).bIsLongRef;
            if !isLongRef || !(*(*pCtx).pSvcParam).bEnableLongTermReference {
                pRefReorder.SReorderingSyntax[0].uiReorderingOfPicNumsIdc = 0;
                pRefReorder.SReorderingSyntax[0].uiAbsDiffPicNumMinus1 = iAbsDiffPicNumMinus1 as u32;
                pRefReorder.SReorderingSyntax[1].uiReorderingOfPicNumsIdc = 3;
            } else {
                let mut iRefIdx = 0usize;
                while (iRefIdx as i32) < (*pCtx).iNumRef0 as i32 {
                    if iRefIdx < MAX_REFERENCE_REORDER_COUNT_NUM {
                        pRefReorder.SReorderingSyntax[iRefIdx].uiReorderingOfPicNumsIdc = 2;
                        let pR = (*pCtx).pRefList0[iRefIdx];
                        if !pR.is_null() {
                            pRefReorder.SReorderingSyntax[iRefIdx].iLongTermPicNum = (*pR).iLongTermPicNum as u16;
                        }
                    }
                    iRefIdx += 1;
                }
                if iRefIdx < MAX_REFERENCE_REORDER_COUNT_NUM {
                    pRefReorder.SReorderingSyntax[iRefIdx].uiReorderingOfPicNumsIdc = 3;
                }
            }
        }

        if uiFrameType == EVideoFrameType::videoFrameTypeIDR as i32 {
            pRefPicMark.bNoOutputOfPriorPicsFlag = false;
            pRefPicMark.bLongTermRefFlag = (*(*pCtx).pSvcParam).bEnableLongTermReference;
        } else {
            if (*(*pCtx).pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = (*(*pCtx).pSvcParam).bEnableLongTermReference;
            } else {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = (*(*pCtx).pSvcParam).bEnableLongTermReference && pLtr.bLTRMarkingFlag;
            }
        }
    }
}

/// Updates reference picture syntax and picture number delta in slice headers.
pub unsafe fn WelsUpdateRefSyntax(pCtx: *mut sWelsEncCtx, kiPOC: i32, kiFrameType: i32) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }
    let mut iAbsDiffPicNumMinus1 = -1i32;
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pParamD = &(*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    if (*pCtx).iNumRef0 > 0 {
        let pRef0 = (*pCtx).pRefList0[0];
        if !pRef0.is_null() {
            iAbsDiffPicNumMinus1 = pParamD.iFrameNum - (*pRef0).iFrameNum - 1;
            if iAbsDiffPicNumMinus1 < 0 && !(*pCtx).pSps.is_null() {
                iAbsDiffPicNumMinus1 += 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;
            }
        }
    }

    WelsUpdateSliceHeaderSyntax(
        pCtx,
        iAbsDiffPicNumMinus1,
        (*(*pCtx).pCurDqLayer).ppSliceInLayer,
        kiFrameType,
    );
}

/// Synchronizes reconstructed picture metadata back to the source input picture.
pub unsafe fn UpdateOriginalPicInfo(pOrigPic: *mut SPicture, pReconPic: *mut SPicture) {
    if pOrigPic.is_null() || pReconPic.is_null() {
        return;
    }
    (*pOrigPic).iPictureType = (*pReconPic).iPictureType;
    (*pOrigPic).iFramePoc = (*pReconPic).iFramePoc;
    (*pOrigPic).iFrameNum = (*pReconPic).iFrameNum;
    (*pOrigPic).uiSpatialId = (*pReconPic).uiSpatialId;
    (*pOrigPic).uiTemporalId = (*pReconPic).uiTemporalId;
    (*pOrigPic).iLongTermPicNum = (*pReconPic).iLongTermPicNum;
    (*pOrigPic).bUsedAsRef = (*pReconPic).bUsedAsRef;
    (*pOrigPic).bIsLongRef = (*pReconPic).bIsLongRef;
    (*pOrigPic).bIsSceneLTR = (*pReconPic).bIsSceneLTR;
    (*pOrigPic).iFrameAverageQp = (*pReconPic).iFrameAverageQp;
}

pub unsafe fn UpdateSrcPicListLosslessScreenRefSelectionWithLtr(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() {
        return;
    }
    let iDIdx = (*pCtx).uiDependencyId as i32;
    UpdateOriginalPicInfo((*pCtx).pEncPic, (*pCtx).pDecPic);
    PrefetchNextBuffer(pCtx);
    if !(*pCtx).pVpp.is_null() && !(*pCtx).pVaa.is_null() {
        let pLongRefList = (**(*pCtx).ppRefPicListExt.add((iDIdx as usize) as usize)).pLongRefList.as_mut_ptr();
        // wels_preprocess.h:143 takes const int32_t; the uint8_t field promotes.
        (*(*pCtx).pVpp).UpdateSrcListLosslessScreenRefSelectionWithLtr(
            (*pCtx).pEncPic,
            iDIdx,
            (*(*pCtx).pVaa).uiMarkLongTermPicIdx as i32,
            pLongRefList,
        );
    }
}

pub unsafe fn UpdateSrcPicList(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() {
        return;
    }
    let iDIdx = (*pCtx).uiDependencyId as i32;
    UpdateOriginalPicInfo((*pCtx).pEncPic, (*pCtx).pDecPic);
    PrefetchNextBuffer(pCtx);
    if !(*pCtx).pVpp.is_null() {
        let pRefList = *(*pCtx).ppRefPicListExt.add((iDIdx as usize) as usize);
        let pShortRefList = (*pRefList).pShortRefList.as_mut_ptr();
        let shortCount = (*pRefList).uiShortRefCount;
        (*(*pCtx).pVpp).UpdateSrcList(
            (*pCtx).pEncPic,
            iDIdx,
            pShortRefList,
            shortCount as u32,
        );
    }
}

/// Screen content specialized reference picture list update.
pub unsafe fn WelsUpdateRefListScreen(pCtx: *mut sWelsEncCtx) -> bool {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() || (*pCtx).pSvcParam.is_null() {
        return false;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    if pRefList.is_null() || (*pRefList).pRef[0].is_null() {
        return false;
    }
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let pParamD = &mut (*(*pCtx).pSvcParam).sDependencyLayers[uiDid];
    let kuiTid = (*pCtx).uiTemporalId;

    if !(*pCtx).pDecPic.is_null() {
        let pDecPic = (*pCtx).pDecPic;
        if pParamD.iHighestTemporalId == 0 || (kuiTid as i32) < pParamD.iHighestTemporalId as i32 {
            if !(*pCtx).pFuncList.is_null() {
                ExpandReferencingPicture(
                    (*pDecPic).pData,
                    (*pDecPic).iWidthInPixel,
                    (*pDecPic).iHeightInPixel,
                    (*pDecPic).iLineSize,
                    (*(*pCtx).pFuncList).sExpandPicFunc.pfExpandLumaPicture,
                    (*(*pCtx).pFuncList).sExpandPicFunc.pfExpandChromaPicture,
                );
            }
        }

        (*pDecPic).uiTemporalId = (*pCtx).uiTemporalId;
        (*pDecPic).uiSpatialId = (*pCtx).uiDependencyId;
        (*pDecPic).iFrameNum = pParamD.iFrameNum;
        (*pDecPic).iFramePoc = pParamD.iPOC;
        (*pDecPic).bUsedAsRef = true;
        (*pDecPic).bIsLongRef = true;
        (*pDecPic).bIsSceneLTR = pLtr.bLTRMarkingFlag
            || ((*(*pCtx).pSvcParam).bEnableLongTermReference
                && (*pCtx).eSliceType == EWelsSliceType::I_SLICE);
        (*pDecPic).iLongTermPicNum = pLtr.iCurLtrIdx;
    }

    if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
        DeleteNonSceneLTR(pCtx);
        LTRMarkProcessScreen(pCtx);
        pLtr.bLTRMarkingFlag = false;
        pLtr.uiLtrMarkInterval += 1;
    } else {
        LTRMarkProcessScreen(pCtx);
        pLtr.iCurLtrIdx = 1;
        pLtr.iSceneLtrIdx = 1;
        pLtr.uiLtrMarkInterval = 0;
        if !(*pCtx).pVaa.is_null() {
            (*(*pCtx).pVaa).uiValidLongTermPicIdx = 0;
        }
    }

    if !(*pCtx).pReferenceStrategy.is_null() {
        // C++ dispatches virtually here (ref_list_mgr_svc.cpp:1041/1057/1073 —
        // PrefetchNextBuffer / UpdateSrcPicList /
        // UpdateSrcPicListLosslessScreenRefSelectionWithLtr), through the C-style
        // vtable above.
        IWelsReferenceStrategy::EndofUpdateRefList(
            (*pCtx).pReferenceStrategy as *mut IWelsReferenceStrategy,
        );
    }
    true
}

/// Screen content specialized reference picture list builder.
pub unsafe fn WelsBuildRefListScreen(
    pCtx: *mut sWelsEncCtx,
    iPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pVaa.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return false;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    let pParam = (*pCtx).pSvcParam;
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
        let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
    let iNumRef = (*pParam).iNumRefFrame;
    let pParamD = &(*pParam).sDependencyLayers[uiDid];
    (*pCtx).iNumRef0 = 0;

    if (*pCtx).eSliceType != EWelsSliceType::I_SLICE {
        let mut iLtrRefIdx = 0i32;
        let mut pRefOri: *mut SPicture = std::ptr::null_mut();

        for idx in 0..(*pVaaExt).iNumOfAvailableRef {
            if !(*pCtx).pVpp.is_null() {
                iLtrRefIdx = (*(*pCtx).pVpp).GetRefFrameInfo(
                    idx,
                    (*pCtx).bCurFrameMarkedAsSceneLtr,
                    &mut pRefOri,
                );
            }
            if iLtrRefIdx >= 0 && iLtrRefIdx <= (*pParam).iLTRRefNum {
                let pRefPic = (*pRefList).pLongRefList[iLtrRefIdx as usize];
                if !pRefPic.is_null() && (*pRefPic).bUsedAsRef && (*pRefPic).bIsLongRef {
                    if (*pRefPic).uiTemporalId <= (*pCtx).uiTemporalId
                        && (!(*pCtx).bCurFrameMarkedAsSceneLtr || (*pRefPic).bIsSceneLTR)
                    {
                        let num0 = (*pCtx).iNumRef0 as usize;
                        (*(*pCtx).pCurDqLayer).pRefOri[num0] = pRefOri;
                        (*pCtx).pRefList0[num0] = pRefPic;
                        (*pCtx).iNumRef0 += 1;
                    }
                }
            } else {
                let mut i = iNumRef;
                while i >= 0 {
                    let pLong = (*pRefList).pLongRefList[i as usize];
                    if pLong.is_null() {
                        i -= 1;
                        continue;
                    } else if (*pLong).uiTemporalId == 0 || (*pLong).uiTemporalId < (*pCtx).uiTemporalId {
                        let num0 = (*pCtx).iNumRef0 as usize;
                        (*(*pCtx).pCurDqLayer).pRefOri[num0] = pRefOri;
                        (*pCtx).pRefList0[num0] = pLong;
                        (*pCtx).iNumRef0 += 1;
                        break;
                    }
                    i -= 1;
                }
            }
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(&mut *(*pCtx).pLtr.add((uiDid) as usize));
        (*pCtx).pRefList0[0] = std::ptr::null_mut();
    }

    if (*pCtx).iNumRef0 as i32 > iNumRef {
        (*pCtx).iNumRef0 = iNumRef as u8;
    }
    (*pCtx).iNumRef0 > 0 || (*pCtx).eSliceType == EWelsSliceType::I_SLICE
}

pub fn IsValidFrameNum(kiFrameNum: i32) -> bool {
    kiFrameNum < (1 << 30)
}

pub unsafe fn WelsMarkMMCORefInfoScreen(
    pCtx: *mut sWelsEncCtx,
    pLtr: *mut SLTRState,
    ppSliceList: *mut *mut SSlice,
    kiCountSliceNum: i32,
) {
    if pCtx.is_null() || pLtr.is_null() || ppSliceList.is_null() || kiCountSliceNum <= 0 {
        return;
    }
    let pBaseSlice = *ppSliceList.add(0);
    if pBaseSlice.is_null() {
        return;
    }
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
    let iMaxLtrIdx = (*(*pCtx).pSvcParam).iNumRefFrame - STR_ROOM - 1;

    *pRefPicMark = SRefPicMarking::default();
    if (*(*pCtx).pSvcParam).bEnableLongTermReference {
        let count0 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count0].iMaxLongTermFrameIdx = iMaxLtrIdx;
        pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SET_MAX_LONG;
        pRefPicMark.uiMmcoCount += 1;

        let count1 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count1].iLongTermFrameIdx = (*pLtr).iCurLtrIdx;
        pRefPicMark.SMmcoRef[count1].iMmcoType = MMCO_LONG;
        pRefPicMark.uiMmcoCount += 1;
    }

    WelsMarkMMCORefInfoWithBase(ppSliceList, pBaseSlice, kiCountSliceNum);
}

pub unsafe fn WelsMarkPicScreen(pCtx: *mut sWelsEncCtx) {
    if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }
    let uiDid = (*pCtx).uiDependencyId as usize;
    let pLtr = &mut *(*pCtx).pLtr.add((uiDid) as usize);
    let gopSize = (*(*pCtx).pSvcParam).uiGopSize;
    let iMaxTid = if gopSize > 0 { (31 - gopSize.leading_zeros()) as i32 } else { 0 };
    let mut iMaxActualLtrIdx = -1i32;
    let pParamD = &(*(*pCtx).pSvcParam).sDependencyLayers[uiDid];

    if (*(*pCtx).pSvcParam).bEnableLongTermReference {
        let maxTidAdj = if iMaxTid > 1 { iMaxTid } else { 1 };
        iMaxActualLtrIdx = (*(*pCtx).pSvcParam).iNumRefFrame - STR_ROOM - 1 - maxTidAdj;
    }

    let pRefList = *(*pCtx).ppRefPicListExt.add((uiDid) as usize);
    let iNumRef = (*(*pCtx).pSvcParam).iNumRefFrame;
    let iLongRefNum = iNumRef - STR_ROOM;
    let bIsRefListNotFull = ((*pRefList).uiLongRefCount as i32) < iLongRefNum;

    if !(*(*pCtx).pSvcParam).bEnableLongTermReference {
        pLtr.iCurLtrIdx = (*pCtx).uiTemporalId as i32;
    } else {
        if iMaxActualLtrIdx != -1 && (*pCtx).uiTemporalId == 0 && (*pCtx).bCurFrameMarkedAsSceneLtr {
            pLtr.bLTRMarkingFlag = true;
            pLtr.uiLtrMarkInterval = 0;
            pLtr.iCurLtrIdx = pLtr.iSceneLtrIdx % (iMaxActualLtrIdx + 1);
            pLtr.iSceneLtrIdx += 1;
        } else {
            pLtr.bLTRMarkingFlag = false;
            if bIsRefListNotFull {
                for i in 0..iLongRefNum {
                    if (*pRefList).pLongRefList[i as usize].is_null() {
                        pLtr.iCurLtrIdx = i;
                        break;
                    }
                }
            } else {
                let mut iRefNum_t = [0i32; MAX_TEMPORAL_LAYER_NUM];
                for i in 0..((*pRefList).uiLongRefCount as usize) {
                    let pPic = (*pRefList).pLongRefList[i];
                    if !pPic.is_null() && (*pPic).bUsedAsRef && (*pPic).bIsLongRef && !(*pPic).bIsSceneLTR {
                        let tid = (*pPic).uiTemporalId as usize;
                        if tid < MAX_TEMPORAL_LAYER_NUM {
                            iRefNum_t[tid] += 1;
                        }
                    }
                }

                let mut iMaxMultiRefTid = if iMaxTid != 0 { iMaxTid - 1 } else { 0 };
                for i in 0..MAX_TEMPORAL_LAYER_NUM {
                    if iRefNum_t[i] > 1 {
                        iMaxMultiRefTid = i as i32;
                    }
                }

                let mut iLongestDeltaFrameNum = -1i32;
                let iMaxFrameNum = 1 << (*(*pCtx).pSps).uiLog2MaxFrameNum;

                for i in 0..((*pRefList).uiLongRefCount as usize) {
                    let pPic = (*pRefList).pLongRefList[i];
                    if !pPic.is_null()
                        && (*pPic).bUsedAsRef
                        && (*pPic).bIsLongRef
                        && !(*pPic).bIsSceneLTR
                        && iMaxMultiRefTid == (*pPic).uiTemporalId as i32
                    {
                        if !IsValidFrameNum((*pPic).iFrameNum) {
                            return;
                        }
                        let iDeltaFrameNum = if pParamD.iFrameNum >= (*pPic).iFrameNum {
                            pParamD.iFrameNum - (*pPic).iFrameNum
                        } else {
                            pParamD.iFrameNum + iMaxFrameNum - (*pPic).iFrameNum
                        };

                        if iDeltaFrameNum > iLongestDeltaFrameNum {
                            pLtr.iCurLtrIdx = (*pPic).iLongTermPicNum;
                            iLongestDeltaFrameNum = iDeltaFrameNum;
                        }
                    }
                }
            }
        }
    }

    for i in 0..MAX_TEMPORAL_LAYER_NUM {
        if ((*pCtx).uiTemporalId as usize) < i || (*pCtx).uiTemporalId == 0 {
            pLtr.iLastLtrIdx[i] = pLtr.iCurLtrIdx;
        }
    }

    let iSliceNum = (*(*pCtx).pCurDqLayer).iMaxSliceNum;
    WelsMarkMMCORefInfoScreen(
        pCtx,
        pLtr,
        (*(*pCtx).pCurDqLayer).ppSliceInLayer,
        iSliceNum,
    );
}

/// Intentional no-op reference list manager callback.
/// Matches `void DoNothing (sWelsEncCtx* pointer)` in `ref_list_mgr_svc.cpp:996`.
pub unsafe fn DoNothing(_pCtx: *mut sWelsEncCtx) {}

// ============================================================================
// Polymorphic Reference Strategy Hierarchy
// ============================================================================

/// Virtual-function table for `IWelsReferenceStrategy`
/// (`codec/encoder/core/inc/ref_list_mgr_svc.h`). Entries are in C++ declaration
/// order, and every entry takes the object pointer as its first argument, standing in
/// for the implicit `this`.
///
/// This replaces a Rust `trait` + `*mut dyn`: `sWelsEncCtx::pReferenceStrategy` is a
/// plain 8-byte `IWelsReferenceStrategy*` in C++, and a `*mut dyn` is a 16-byte fat
/// pointer, which mis-sized the context. Same mechanism as
/// `paraset_strategy::IWelsParametersetStrategyVtbl`.
#[repr(C)]
pub struct IWelsReferenceStrategyVtbl {
    pub Destroy: unsafe extern "C" fn(*mut IWelsReferenceStrategy),
    pub BuildRefList: unsafe extern "C" fn(*mut IWelsReferenceStrategy, i32, i32) -> bool,
    pub MarkPic: unsafe extern "C" fn(*mut IWelsReferenceStrategy),
    pub UpdateRefList: unsafe extern "C" fn(*mut IWelsReferenceStrategy) -> bool,
    pub EndofUpdateRefList: unsafe extern "C" fn(*mut IWelsReferenceStrategy),
    pub AfterBuildRefList: unsafe extern "C" fn(*mut IWelsReferenceStrategy),
    pub Init: unsafe extern "C" fn(*mut IWelsReferenceStrategy, *mut sWelsEncCtx),
}

/// `IWelsReferenceStrategy` — the abstract base; one pointer wide.
#[repr(C)]
pub struct IWelsReferenceStrategy {
    pub pVtbl: *const IWelsReferenceStrategyVtbl,
}

impl IWelsReferenceStrategy {
    /// # Safety
    /// `pThis` must be a live strategy from [`CreateReferenceStrategy`].
    pub unsafe fn BuildRefList(
        pThis: *mut IWelsReferenceStrategy,
        iPOC: i32,
        iBestLtrRefIdx: i32,
    ) -> bool {
        ((*(*pThis).pVtbl).BuildRefList)(pThis, iPOC, iBestLtrRefIdx)
    }
    /// # Safety
    /// `pThis` must be a live strategy from [`CreateReferenceStrategy`].
    pub unsafe fn MarkPic(pThis: *mut IWelsReferenceStrategy) {
        ((*(*pThis).pVtbl).MarkPic)(pThis)
    }
    /// # Safety
    /// `pThis` must be a live strategy from [`CreateReferenceStrategy`].
    pub unsafe fn UpdateRefList(pThis: *mut IWelsReferenceStrategy) -> bool {
        ((*(*pThis).pVtbl).UpdateRefList)(pThis)
    }
    /// # Safety
    /// `pThis` must be a live strategy from [`CreateReferenceStrategy`].
    pub unsafe fn EndofUpdateRefList(pThis: *mut IWelsReferenceStrategy) {
        ((*(*pThis).pVtbl).EndofUpdateRefList)(pThis)
    }
    /// # Safety
    /// `pThis` must be a live strategy from [`CreateReferenceStrategy`].
    pub unsafe fn AfterBuildRefList(pThis: *mut IWelsReferenceStrategy) {
        ((*(*pThis).pVtbl).AfterBuildRefList)(pThis)
    }
}

/// `CWelsReference_TemporalLayer`, `CWelsReference_Screen` and
/// `CWelsReference_LosslessWithLtr` all carry exactly one data member,
/// `m_pEncoderCtx`, inherited from the first; only their vtables differ.
#[repr(C)]
pub struct CWelsReferenceStrategyObj {
    pub base: IWelsReferenceStrategy,
    pub m_pEncoderCtx: *mut sWelsEncCtx,
}

#[inline]
unsafe fn as_ref_obj(pThis: *mut IWelsReferenceStrategy) -> *mut CWelsReferenceStrategyObj {
    pThis as *mut CWelsReferenceStrategyObj
}

unsafe extern "C" fn Ref_Destroy(pThis: *mut IWelsReferenceStrategy) {
    drop(Box::from_raw(as_ref_obj(pThis)));
}

unsafe extern "C" fn Ref_Init(pThis: *mut IWelsReferenceStrategy, pCtx: *mut sWelsEncCtx) {
    (*as_ref_obj(pThis)).m_pEncoderCtx = pCtx;
}

// --- CWelsReference_TemporalLayer ---
unsafe extern "C" fn TL_BuildRefList(
    pThis: *mut IWelsReferenceStrategy,
    iPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    WelsBuildRefList((*as_ref_obj(pThis)).m_pEncoderCtx, iPOC, iBestLtrRefIdx)
}
unsafe extern "C" fn TL_MarkPic(pThis: *mut IWelsReferenceStrategy) {
    WelsMarkPic((*as_ref_obj(pThis)).m_pEncoderCtx);
}
unsafe extern "C" fn TL_UpdateRefList(pThis: *mut IWelsReferenceStrategy) -> bool {
    WelsUpdateRefList((*as_ref_obj(pThis)).m_pEncoderCtx)
}
unsafe extern "C" fn TL_EndofUpdateRefList(pThis: *mut IWelsReferenceStrategy) {
    PrefetchNextBuffer((*as_ref_obj(pThis)).m_pEncoderCtx);
}
unsafe extern "C" fn TL_AfterBuildRefList(pThis: *mut IWelsReferenceStrategy) {
    DoNothing((*as_ref_obj(pThis)).m_pEncoderCtx);
}

pub static TEMPORAL_LAYER_VTBL: IWelsReferenceStrategyVtbl = IWelsReferenceStrategyVtbl {
    Destroy: Ref_Destroy,
    BuildRefList: TL_BuildRefList,
    MarkPic: TL_MarkPic,
    UpdateRefList: TL_UpdateRefList,
    EndofUpdateRefList: TL_EndofUpdateRefList,
    AfterBuildRefList: TL_AfterBuildRefList,
    Init: Ref_Init,
};

// --- CWelsReference_Screen: same build/mark/update as the base, different tail ---
unsafe extern "C" fn SC_EndofUpdateRefList(pThis: *mut IWelsReferenceStrategy) {
    UpdateSrcPicList((*as_ref_obj(pThis)).m_pEncoderCtx);
}
unsafe extern "C" fn SC_AfterBuildRefList(pThis: *mut IWelsReferenceStrategy) {
    UpdateBlockStatic((*as_ref_obj(pThis)).m_pEncoderCtx);
}

pub static SCREEN_VTBL: IWelsReferenceStrategyVtbl = IWelsReferenceStrategyVtbl {
    Destroy: Ref_Destroy,
    BuildRefList: TL_BuildRefList,
    MarkPic: TL_MarkPic,
    UpdateRefList: TL_UpdateRefList,
    EndofUpdateRefList: SC_EndofUpdateRefList,
    AfterBuildRefList: SC_AfterBuildRefList,
    Init: Ref_Init,
};

// --- CWelsReference_LosslessWithLtr ---
unsafe extern "C" fn LL_BuildRefList(
    pThis: *mut IWelsReferenceStrategy,
    iPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    WelsBuildRefListScreen((*as_ref_obj(pThis)).m_pEncoderCtx, iPOC, iBestLtrRefIdx)
}
unsafe extern "C" fn LL_MarkPic(pThis: *mut IWelsReferenceStrategy) {
    WelsMarkPicScreen((*as_ref_obj(pThis)).m_pEncoderCtx);
}
unsafe extern "C" fn LL_UpdateRefList(pThis: *mut IWelsReferenceStrategy) -> bool {
    WelsUpdateRefListScreen((*as_ref_obj(pThis)).m_pEncoderCtx)
}
unsafe extern "C" fn LL_EndofUpdateRefList(pThis: *mut IWelsReferenceStrategy) {
    UpdateSrcPicListLosslessScreenRefSelectionWithLtr((*as_ref_obj(pThis)).m_pEncoderCtx);
}

pub static LOSSLESS_WITH_LTR_VTBL: IWelsReferenceStrategyVtbl = IWelsReferenceStrategyVtbl {
    Destroy: Ref_Destroy,
    BuildRefList: LL_BuildRefList,
    MarkPic: LL_MarkPic,
    UpdateRefList: LL_UpdateRefList,
    EndofUpdateRefList: LL_EndofUpdateRefList,
    AfterBuildRefList: SC_AfterBuildRefList,
    Init: Ref_Init,
};

/// `IWelsReferenceStrategy::CreateReferenceStrategy`.
///
/// Returns a raw pointer the caller owns; release it with
/// [`DestroyReferenceStrategy`].
///
/// # Safety
/// `pCtx` must outlive the returned strategy.
pub unsafe fn CreateReferenceStrategy(
    pCtx: *mut sWelsEncCtx,
    keUsageType: EUsageType,
    kbLtrEnabled: bool,
) -> *mut IWelsReferenceStrategy {
    let pVtbl: *const IWelsReferenceStrategyVtbl = match keUsageType {
        EUsageType::SCREEN_CONTENT_REAL_TIME => {
            if kbLtrEnabled {
                &LOSSLESS_WITH_LTR_VTBL
            } else {
                &SCREEN_VTBL
            }
        }
        _ => &TEMPORAL_LAYER_VTBL,
    };
    let obj = Box::new(CWelsReferenceStrategyObj {
        base: IWelsReferenceStrategy { pVtbl },
        m_pEncoderCtx: std::ptr::null_mut(),
    });
    let p = Box::into_raw(obj) as *mut IWelsReferenceStrategy;
    ((*pVtbl).Init)(p, pCtx);
    p
}

/// Counterpart to [`CreateReferenceStrategy`].
///
/// # Safety
/// `pStrategy` must have come from [`CreateReferenceStrategy`] and must not be used
/// afterwards.
pub unsafe fn DestroyReferenceStrategy(pStrategy: *mut IWelsReferenceStrategy) {
    if !pStrategy.is_null() {
        ((*(*pStrategy).pVtbl).Destroy)(pStrategy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_list_mgr_noop_callback() {
        unsafe {
            DoNothing(std::ptr::null_mut());
        }
    }
}
