#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

//! Reference picture list management and Long-Term Reference (LTR) control.
//!
//! Translated from `codec/encoder/core/inc/ref_list_mgr_svc.h` and
//! `codec/encoder/core/src/ref_list_mgr_svc.cpp`.

#![deny(unsafe_code)]

use crate::encoder::picture::{PicRef, RecPicId, SrcPicId};
use crate::*;

// ============================================================================
// Constants
// ============================================================================

pub const STR_ROOM: i32 = 1;
// `MAX_SHORT_REF_COUNT`, `MAX_TEMPORAL_LEVEL` and `MAX_GOP_SIZE` are defined once in
// `encoder_context.rs` from `wels_const.h`.
pub use crate::encoder::encoder_context::{MAX_GOP_SIZE, MAX_SHORT_REF_COUNT, MAX_TEMPORAL_LEVEL};
use crate::encoder::encoder_context::ctx_ltr_at;
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
use crate::encoder::svc_encode_slice::current_layer_ref;
use crate::encoder::svc_encode_slice::current_layer_mut;
use crate::encoder::svc_encode_slice::ctx_sps;
use crate::encoder::svc_encode_slice::ctx_sps_ref;
use crate::encoder::svc_encode_slice::{current_layer_expect, current_layer_expect_mut};
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::encoder_context::SLTRState;
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

// ============================================================================
// Global Reference Picture List Lifecycle Functions
// ============================================================================

/// Reset LTR marking, recovery, and feedback state to defaults.
pub fn ResetLtrState(pLtr: &mut SLTRState) {
    pLtr.bReceivedT0LostFlag = false;
    pLtr.iLastRecoverFrameNum = 0;
    pLtr.iLastCorFrameNumDec = -1;
    pLtr.iCurFrameNumInDec = -1;

    // LTR mark
    pLtr.iLTRMarkMode = LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32;
    pLtr.iLTRMarkSuccessNum = 0;
    pLtr.bLTRMarkingFlag = false;
    pLtr.bLTRMarkEnable = false;
    pLtr.iCurLtrIdx = 0;
    pLtr.iLastLtrIdx = [0; MAX_TEMPORAL_LAYER_NUM];
    pLtr.uiLtrMarkInterval = 0;

    // LTR mark feedback
    pLtr.uiLtrMarkState = NO_LTR_MARKING_FEEDBACK;
    pLtr.iLtrMarkFbFrameNum = -1;
}

/// Reset active reference picture lists for current spatial layer.
pub fn WelsResetRefList(pCtx: &mut sWelsEncCtx) {
    let uiDid = pCtx.uiDependencyId as usize;
    let ltrRefNum = if pCtx.param_opt().is_some() {
        pCtx.param().iLTRRefNum as usize
    } else {
        0
    };
    let numRefFrame = if pCtx.param_opt().is_some() {
        pCtx.param().iNumRefFrame as usize
    } else {
        0
    };
    let Some(pRefList) = pCtx.ref_list_mut((uiDid) as usize) else {
        return;
    };

    for i in 0..=(MAX_SHORT_REF_COUNT) {
        pRefList.pShortRefList[i] = None;
    }
    for i in 0..=(ltrRefNum) {
        if i <= MAX_REF_PIC_COUNT {
            pRefList.pLongRefList[i] = None;
        }
    }
    for i in 0..=(numRefFrame) {
        if i < pRefList.pRef.len() {
            let id = pRefList.pRef.at(i);
            pRefList.pic_mut(id).SetUnref();
        }
    }

    pRefList.uiLongRefCount = 0;
    pRefList.uiShortRefCount = 0;
    pRefList.pNextBuffer = if pRefList.pRef.is_empty() {
        None
    } else {
        Some(pRefList.pRef.at(0))
    };
}

/// Remove a long-term reference entry by index from pLongRefList.
pub fn DeleteLTRFromLongList(pRefList: &mut SRefList, iIdx: i32) {
    let count = pRefList.uiLongRefCount as i32;
    // Upstream (`ref_list_mgr_svc.cpp:82`) walks to `uiLongRefCount - 1` and indexes
    // `pLongRefList[k + 1]` unchecked; the array is `[_; 1 + MAX_REF_PIC_COUNT]` and
    // nothing in either tree *enforces* `uiLongRefCount <= 1 + MAX_REF_PIC_COUNT` —
    // it is an emergent property of the marking schedule, not a checked bound. Where
    // that invariant holds, `kLast` is never the binding term and this loop is
    // byte-for-byte the C++ one. Where it fails, upstream reads past the array and
    // this stops at its end instead of panicking.
    let kLast = pRefList.pLongRefList.len() as i32 - 1;
    let kUpper = (count - 1).min(kLast);
    let mut k = iIdx;
    while k < kUpper {
        pRefList.pLongRefList[k as usize] = pRefList.pLongRefList[(k + 1) as usize];
        k += 1;
    }
    if k >= 0 && (k as usize) <= MAX_REF_PIC_COUNT {
        pRefList.pLongRefList[k as usize] = None;
    }
    if pRefList.uiLongRefCount > 0 {
        pRefList.uiLongRefCount -= 1;
    }
}

/// Remove a short-term reference entry by index from pShortRefList.
pub fn DeleteSTRFromShortList(pRefList: &mut SRefList, iIdx: i32) {
    let count = pRefList.uiShortRefCount as i32;
    // The same guard as [`DeleteLTRFromLongList`], for the same reason and against
    // the same C++ shape (`ref_list_mgr_svc.cpp:93`).
    let kLast = pRefList.pShortRefList.len() as i32 - 1;
    let kUpper = (count - 1).min(kLast);
    let mut k = iIdx;
    while k < kUpper {
        pRefList.pShortRefList[k as usize] = pRefList.pShortRefList[(k + 1) as usize];
        k += 1;
    }
    if k >= 0 && (k as usize) <= MAX_SHORT_REF_COUNT {
        pRefList.pShortRefList[k as usize] = None;
    }
    if pRefList.uiShortRefCount > 0 {
        pRefList.uiShortRefCount -= 1;
    }
}

/// Unreferences non-scene LTR frames when current frame is marked as Scene LTR.
pub fn DeleteNonSceneLTR(pCtx: &mut sWelsEncCtx) {
    if pCtx.param_opt().is_none() {
        return;
    }
    let numRef = pCtx.param().iNumRefFrame;
    let uiTemporalId = pCtx.uiTemporalId;
    let bCurFrameMarkedAsSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
    let Some(pRefList) = pCtx.ref_list_mut(pCtx.uiDependencyId as usize) else {
        return;
    };
    let mut i = 0;
    while i < numRef {
        let hit = match pRefList.pLongRefList[i as usize] {
            Some(id) => {
                let pRef = pRefList.pic(id);
                pRef.bUsedAsRef
                    && pRef.bIsLongRef
                    && (!pRef.bIsSceneLTR)
                    && (uiTemporalId < pRef.uiTemporalId
                        || bCurFrameMarkedAsSceneLtr)
            }
            None => false,
        };
        if hit {
            let id = pRefList.pLongRefList[i as usize].expect("checked just above");
            pRefList.pic_mut(id).SetUnref();
            DeleteLTRFromLongList(&mut *pRefList, i);
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
pub fn DeleteInvalidLTR(pCtx: &mut sWelsEncCtx) {
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return;
    };
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let iMaxFrameNumPlus1 = 1 << sps.uiLog2MaxFrameNum;
    let ltr_family = pCtx.ltr_family_mut(uiDid);
    let pParamInternal = ltr_family.param_layer;
    let pLtr = ltr_family.ltr;
    let Some(pRefList) = ltr_family.ref_list else {
        return;
    };

    for i in 0..LONG_TERM_REF_NUM {
        if let Some(idPic) = pRefList.pLongRefList[i as usize] {
            let pPic = pRefList.pic(idPic);
            let cond1 = CompareFrameNum(pPic.iFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                && ((CompareFrameNum(pPic.iFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0);

            if cond1 {
                pRefList.pic_mut(idPic).SetUnref();
                DeleteLTRFromLongList(&mut *pRefList, i);
                pLtr.bLTRMarkEnable = true;
                if pRefList.uiLongRefCount == 0 {
                    pParamInternal.bEncCurFrmAsIdrFlag = true;
                }
            } else {
                let pPic = pRefList.pic(idPic);
                let cond2 = CompareFrameNum(pPic.iMarkFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                    == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                    && ((CompareFrameNum(pPic.iMarkFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                        & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                        != 0)
                    && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32;

                if cond2 {
                    pRefList.pic_mut(idPic).SetUnref();
                    DeleteLTRFromLongList(&mut *pRefList, i);
                    pLtr.bLTRMarkEnable = true;
                    if pRefList.uiLongRefCount == 0 {
                        pParamInternal.bEncCurFrmAsIdrFlag = true;
                    }
                }
            }
        }
    }
}

/// Handles asynchronous decoder confirmation or failure feedback for LTR marking.
pub fn HandleLTRMarkFeedback(pCtx: &mut sWelsEncCtx) {
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let ltr_family = pCtx.ltr_family_mut(uiDid);
    let pParamInternal = ltr_family.param_layer;
    let mut pVaa = ltr_family.vaa;
    let pLtr = ltr_family.ltr;
    let Some(pRefList) = ltr_family.ref_list else {
        return;
    };

    if pLtr.uiLtrMarkState == LTR_MARKING_SUCCESS {
        for i in 0..(pRefList.uiLongRefCount as i32) {
            let Some(idPic) = pRefList.pLongRefList[i as usize] else {
                continue;
            };
            let pPic = pRefList.pic(idPic);
            if pPic.iFrameNum == pLtr.iLtrMarkFbFrameNum
                && pPic.uiRecieveConfirmed != RECIEVE_SUCCESS
            {
                pRefList.pic_mut(idPic).uiRecieveConfirmed = RECIEVE_SUCCESS;
                if let Some(pVaa) = pVaa.as_mut() {
                    pVaa.uiValidLongTermPicIdx =
                        pRefList.pic(idPic).iLongTermPicNum as u8;
                }
                pLtr.iCurFrameNumInDec = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastRecoverFrameNum = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastCorFrameNumDec = pLtr.iLtrMarkFbFrameNum;

                let mut j = 0;
                while j < pRefList.uiLongRefCount as i32 {
                    let drop = match pRefList.pLongRefList[j as usize] {
                        Some(id) => pRefList.pic(id).iLongTermPicNum != pLtr.iCurLtrIdx,
                        None => false,
                    };
                    if drop {
                        let id = pRefList.pLongRefList[j as usize].expect("checked just above");
                        pRefList.pic_mut(id).SetUnref();
                        DeleteLTRFromLongList(&mut *pRefList, j);
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
        for i in 0..(pRefList.uiLongRefCount as i32) {
            let Some(idPic) = pRefList.pLongRefList[i as usize] else {
                continue;
            };
            if pRefList.pic(idPic).iFrameNum == pLtr.iLtrMarkFbFrameNum {
                pRefList.pic_mut(idPic).SetUnref();
                DeleteLTRFromLongList(&mut *pRefList, i);
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
pub fn LTRMarkProcess(pCtx: &mut sWelsEncCtx) {
    if pCtx.param_opt().is_none() {
        return;
    }
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    let gopSize = pCtx.param().uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << sps.uiLog2MaxFrameNum;
    let mut i = 0usize;
    let mut bMoveLtrFromShortToLong = false;
    let keSliceType = pCtx.eSliceType;
    let iLTRRefNum = pCtx.param().iLTRRefNum;
    let uiTemporalId = pCtx.uiTemporalId as usize;
    let ltr_family = pCtx.ltr_family_mut(uiDid);
    let pParamInternal = ltr_family.param_layer;
    let mut pVaa = ltr_family.vaa;
    let pLtr = ltr_family.ltr;
    let bRefOfCurTidIsLtr = ltr_family.ref_of_cur_tid_is_ltr;
    let Some(pRefList) = ltr_family.ref_list else {
        return;
    };

    if keSliceType == EWelsSliceType::I_SLICE {
        i = 0;
        if let Some(id) = pRefList.pShortRefList[i] {
            pRefList.pic_mut(id).uiRecieveConfirmed = RECIEVE_SUCCESS;
        }
    } else if pLtr.bLTRMarkingFlag {
        if let Some(pVaa) = pVaa.as_mut() {
            pVaa.uiMarkLongTermPicIdx = pLtr.iCurLtrIdx as u8;
        }

        if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 {
            for idx in 0..(pRefList.uiShortRefCount as usize) {
                if let Some(id) = pRefList.pShortRefList[idx] {
                    if CompareFrameNum(
                        pParamInternal.iFrameNum,
                        pRefList.pic(id).iFrameNum + iGoPFrameNumInterval,
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

    if keSliceType == EWelsSliceType::I_SLICE || pLtr.bLTRMarkingFlag {
        if let Some(id) = pRefList.pShortRefList[i] {
            let iFrameNum = pParamInternal.iFrameNum;
            let pShort = pRefList.pic_mut(id);
            pShort.bIsLongRef = true;
            pShort.iLongTermPicNum = pLtr.iCurLtrIdx;
            pShort.iMarkFrameNum = iFrameNum;
        }
    }

    if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32
        && keSliceType != EWelsSliceType::I_SLICE
        && !pLtr.bLTRMarkingFlag
    {
        for j in 0..(pRefList.uiShortRefCount as usize) {
            if let Some(id) = pRefList.pShortRefList[j] {
                if pRefList.pic(id).bIsLongRef {
                    i = j;
                    bMoveLtrFromShortToLong = true;
                    break;
                }
            }
        }
    }

    if (pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 && pLtr.bLTRMarkingFlag)
        || ((pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32) && bMoveLtrFromShortToLong)
    {
        let tid = uiTemporalId;
        if uiDid < MAX_DEPENDENCY_LAYER && tid < MAX_TEMPORAL_LEVEL {
            bRefOfCurTidIsLtr[uiDid][tid] = true;
        }

        let longCount = pRefList.uiLongRefCount as usize;
        if longCount > 0 {
            for k in (1..=longCount).rev() {
                if k <= MAX_REF_PIC_COUNT {
                    pRefList.pLongRefList[k] = pRefList.pLongRefList[k - 1];
                }
            }
        }
        pRefList.pLongRefList[0] = pRefList.pShortRefList[i];
        pRefList.uiLongRefCount += 1;
        if pRefList.uiLongRefCount as i32 > iLTRRefNum {
            let lastIdx = (pRefList.uiLongRefCount - 1) as usize;
            if let Some(id) = pRefList.pLongRefList[lastIdx] {
                pRefList.pic_mut(id).SetUnref();
            }
            DeleteLTRFromLongList(&mut *pRefList, lastIdx as i32);
        }
        DeleteSTRFromShortList(&mut *pRefList, i as i32);
    }
}

/// Executes promotion of screen content references to long-term reference slots.
pub fn LTRMarkProcessScreen(pCtx: &mut sWelsEncCtx) {
    let Some(idDec) = pCtx.pDecPic else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    let Some(iLtrIdx) = pCtx.ref_list(uiDid).map(|l| l.pic(idDec).iLongTermPicNum) else {
        return;
    };
    if pCtx.vaa().is_some() {
        pCtx.vaa_expect_mut().uiMarkLongTermPicIdx = iLtrIdx as u8;
    }

    let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
        return;
    };
    if iLtrIdx >= 0 && (iLtrIdx as usize) < MAX_REF_PIC_COUNT {
        match pRefList.pLongRefList[iLtrIdx as usize] {
            Some(id) => pRefList.pic_mut(id).SetUnref(),
            None => pRefList.uiLongRefCount += 1,
        }
        pRefList.pLongRefList[iLtrIdx as usize] = Some(idDec);
    }
}

/// Pre-allocates destination frame buffer pointer pDecPic for upcoming reconstruction.
pub fn PrefetchNextBuffer(pCtx: &mut sWelsEncCtx) {
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let kiNumRef = pCtx.param().iNumRefFrame;
    let Some(pRefList) = pCtx.ref_list_mut((uiDid) as usize) else {
        return;
    };

    pRefList.pNextBuffer = None;
    for i in 0..=(kiNumRef as usize) {
        if i < pRefList.pRef.len() {
            let id = pRefList.pRef.at(i);
            if !pRefList.pic(id).bUsedAsRef {
                pRefList.pNextBuffer = Some(id);
                break;
            }
        }
    }

    if pRefList.pNextBuffer.is_none() && pRefList.uiShortRefCount > 0 {
        // The C++ counterpart is `ref_list_mgr_svc.cpp:343`
        // (`pShortRefList[pRefList->uiShortRefCount - 1]`), an unchecked *read*.
        let lastIdx = (((pRefList.uiShortRefCount - 1) as usize))
            .min(pRefList.pShortRefList.len() - 1);
        pRefList.pNextBuffer = pRefList.pShortRefList[lastIdx];
        if let Some(id) = pRefList.pNextBuffer {
            pRefList.pic_mut(id).SetUnref();
        }
    }

    pCtx.pDecPic = pRefList.pNextBuffer;
}

/// Updates reference picture list after current frame reconstruction.
pub fn WelsUpdateRefList(pCtx: &mut sWelsEncCtx) -> bool {
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).map_or(true, |l| l.pRef.is_empty()) {
        return false;
    }

    let (kiHighestTid, kiFrameNum, kiPOC) = {
        let pParamD = &pCtx.param().sDependencyLayers[uiDid];
        (pParamD.iHighestTemporalId, pParamD.iFrameNum, pParamD.iPOC)
    };
    let kuiTid = pCtx.uiTemporalId;
    let kuiDid = pCtx.uiDependencyId;
    let keSliceType = pCtx.eSliceType;

    if let Some(idDec) = pCtx.pDecPic {
        let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
            return false;
        };
        // The reconstruction picture, resolved once.
        let pDecPic: &mut SPicture = pRefList.pic_mut(idDec);
        if kiHighestTid == 0 || (kuiTid as i32) < kiHighestTid as i32 {
            // `ref_list_mgr_svc.cpp:375`.
            pDecPic.expand_as_reference();
        }

        if crate::encoder::dump_enabled(&REC_DUMP, "OH264_RECDUMP") {
            let (kiW, kiH) = (pDecPic.iWidthInPixel, pDecPic.iHeightInPixel);
            for pl in 0..3usize {
                let w = if pl != 0 { kiW >> 1 } else { kiW };
                let h = if pl != 0 { kiH >> 1 } else { kiH };
                let (mut sum, mut x) = (0u32, 1u32);
                for y in 0..h {
                    for &b in pDecPic.plane(pl).row(y as isize, 0, w as usize) {
                        x = x.wrapping_mul(31).wrapping_add(b as u32);
                        sum = sum.wrapping_add(x);
                    }
                }
                eprintln!("REC plane={} poc={} sum={}", pl, kiPOC, sum);
            }
        }

        pDecPic.uiTemporalId = kuiTid;
        pDecPic.uiSpatialId = kuiDid;
        pDecPic.iFrameNum = kiFrameNum;
        pDecPic.iFramePoc = kiPOC;
        pDecPic.uiRecieveConfirmed = RECIEVE_UNKOWN;
        pDecPic.bUsedAsRef = true;

        let shortCount = pRefList.uiShortRefCount as usize;
        for iRefIdx in (0..shortCount).rev() {
            if iRefIdx + 1 <= MAX_SHORT_REF_COUNT {
                pRefList.pShortRefList[iRefIdx + 1] = pRefList.pShortRefList[iRefIdx];
            }
        }
        pRefList.pShortRefList[0] = Some(idDec);
        pRefList.uiShortRefCount += 1;
    }

    if keSliceType == EWelsSliceType::P_SLICE {
        if kuiTid == 0 {
            if pCtx.param().bEnableLongTermReference {
                LTRMarkProcess(pCtx);
                DeleteInvalidLTR(pCtx);
                HandleLTRMarkFeedback(pCtx);

                let pLtr = ctx_ltr_at(pCtx, uiDid);
                pLtr.bReceivedT0LostFlag = false;
                pLtr.bLTRMarkingFlag = false;
                pLtr.uiLtrMarkInterval += 1;
            }

            let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
                return false;
            };
            let mut i = (pRefList.uiShortRefCount as i32) - 1;
            while i > 0 {
                if let Some(id) = pRefList.pShortRefList[i as usize] {
                    pRefList.pic_mut(id).SetUnref();
                }
                DeleteSTRFromShortList(&mut *pRefList, i);
                i -= 1;
            }
            if pRefList.uiShortRefCount > 0 {
                let stale = match pRefList.pShortRefList[0] {
                    Some(id) => {
                        let p0 = pRefList.pic(id);
                        p0.uiTemporalId > 0 || p0.iFrameNum != kiFrameNum
                    }
                    None => false,
                };
                if stale {
                    let id = pRefList.pShortRefList[0].expect("checked just above");
                    pRefList.pic_mut(id).SetUnref();
                    DeleteSTRFromShortList(&mut *pRefList, 0);
                }
            }
        }
    } else {
        if pCtx.param().bEnableLongTermReference {
            LTRMarkProcess(pCtx);

            let pLtr = ctx_ltr_at(pCtx, uiDid);
            pLtr.iCurLtrIdx = (pLtr.iCurLtrIdx + 1) % LONG_TERM_REF_NUM;
            pLtr.iLTRMarkSuccessNum = 1;
            pLtr.bLTRMarkEnable = true;
            pLtr.uiLtrMarkInterval = 0;

            if pCtx.vaa().is_some() {
                pCtx.vaa_expect_mut().uiValidLongTermPicIdx = 0;
                pCtx.vaa_expect_mut().uiMarkLongTermPicIdx = 0;
            }
        }
    }

    // C++ dispatches virtually here (ref_list_mgr_svc.cpp:1041/1057/1073 —
    // PrefetchNextBuffer / UpdateSrcPicList /
    // UpdateSrcPicListLosslessScreenRefSelectionWithLtr).
    pCtx.eRefStrategy.EndofUpdateRefList(pCtx);
    true
}

/// Checks whether candidate frame number is already occupied in LTR list.
pub fn CheckCurMarkFrameNumUsed(pCtx: &mut sWelsEncCtx) -> bool {
    if pCtx.param_opt().is_none() {
        return false;
    }
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return false;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    let gopSize = pCtx.param().uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << sps.uiLog2MaxFrameNum;
    let kiParamFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;
    let (pRefList, pLtr) = pCtx.ref_list_and_ltr_mut(uiDid);
    let Some(pRefList) = pRefList else {
        return false;
    };

    for i in 0..(pRefList.uiLongRefCount as usize) {
        if let Some(idLong) = pRefList.pLongRefList[i] {
            let iFrameNum = pRefList.pic(idLong).iFrameNum;
            let cond1 = kiParamFrameNum == iFrameNum
                && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32;
            let cond2 = CompareFrameNum(
                kiParamFrameNum + iGoPFrameNumInterval,
                iFrameNum,
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
pub fn WelsMarkMMCORefInfoWithBase(
    pCurDq: &mut SDqLayer,
    kBaseMarking: SRefPicMarking,
    kiCountSliceNum: i32,
) {
    for iSliceIdx in 0..kiCountSliceNum {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx);
        if let Some(pSlice) = pSlice {
            pSlice.sSliceHeaderExt.sSliceHeader.sRefMarking = kBaseMarking;
        }
    }
}

/// Constructs MMCO reference marking commands for slice headers.
pub fn WelsMarkMMCORefInfo(
    kuiGopSize: u32,
    kbEnableLongTermReference: bool,
    pLtr: &SLTRState,
    pCurDq: &mut SDqLayer,
    kiCountSliceNum: i32,
) {
    if kiCountSliceNum <= 0 {
        return;
    }
    let Some(pBaseSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, 0) else {
        return;
    };
    let pRefPicMark = &mut pBaseSlice.sSliceHeaderExt.sSliceHeader.sRefMarking;
    let iGoPFrameNumInterval = if (kuiGopSize >> 1) > 1 {
        (kuiGopSize >> 1) as i32
    } else {
        1
    };

    *pRefPicMark = SRefPicMarking::default();

    if kbEnableLongTermReference && pLtr.bLTRMarkingFlag {
        if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32 {
            let count0 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count0].iMaxLongTermFrameIdx = LONG_TERM_REF_NUM - 1;
            pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SET_MAX_LONG;
            pRefPicMark.uiMmcoCount += 1;

            let count1 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count1].iDiffOfPicNum = iGoPFrameNumInterval;
            pRefPicMark.SMmcoRef[count1].iMmcoType = MMCO_SHORT2UNUSED;
            pRefPicMark.uiMmcoCount += 1;

            let count2 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count2].iLongTermFrameIdx = pLtr.iCurLtrIdx;
            pRefPicMark.SMmcoRef[count2].iMmcoType = MMCO_LONG;
            pRefPicMark.uiMmcoCount += 1;
        } else if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 {
            let count0 = pRefPicMark.uiMmcoCount as usize;
            pRefPicMark.SMmcoRef[count0].iDiffOfPicNum = iGoPFrameNumInterval;
            pRefPicMark.SMmcoRef[count0].iLongTermFrameIdx = pLtr.iCurLtrIdx;
            pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SHORT2LONG;
            pRefPicMark.uiMmcoCount += 1;
        }
    }

    let kBaseMarking = *pRefPicMark;
    WelsMarkMMCORefInfoWithBase(pCurDq, kBaseMarking, kiCountSliceNum);
}

/// Evaluates LTR marking criteria and populates slice header MMCO commands.
pub fn WelsMarkPic(pCtx: &mut sWelsEncCtx) {
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let kbEnableLtr = pCtx.param().bEnableLongTermReference;
    let kiLtrMarkPeriod = pCtx.param().iLtrMarkPeriod;
    let kuiGopSize = pCtx.param().uiGopSize;
    let kuiTid = pCtx.uiTemporalId;
    let kiCountSliceNum = current_layer_expect(pCtx).iMaxSliceNum;
    if kbEnableLtr && ctx_ltr_at(pCtx, uiDid).bLTRMarkEnable && kuiTid == 0 {
        let pLtr = &*ctx_ltr_at(pCtx, uiDid);
        let bMarkCandidate = !pLtr.bReceivedT0LostFlag
            && pLtr.uiLtrMarkInterval > kiLtrMarkPeriod as u32;
        if bMarkCandidate && CheckCurMarkFrameNumUsed(pCtx) {
            let pLtr = ctx_ltr_at(pCtx, uiDid);
            pLtr.bLTRMarkingFlag = true;
            pLtr.bLTRMarkEnable = false;
            pLtr.uiLtrMarkInterval = 0;
            for i in 0..MAX_TEMPORAL_LAYER_NUM {
                if (kuiTid as usize) < i || kuiTid == 0 {
                    pLtr.iLastLtrIdx[i] = pLtr.iCurLtrIdx;
                }
            }
        } else {
            ctx_ltr_at(pCtx, uiDid).bLTRMarkingFlag = false;
        }
    }

    let kLtr = *ctx_ltr_at(pCtx, uiDid);
    WelsMarkMMCORefInfo(
        kuiGopSize,
        kbEnableLtr,
        &kLtr,
        current_layer_expect_mut(pCtx),
        kiCountSliceNum,
    );
}

/// Evaluates LTR recovery request feedback packets from decoder.
pub fn FilterLTRRecoveryRequest(
    pCtx: &mut sWelsEncCtx,
    pLTRRecoverRequest: &mut SLTRRecoverRequest,
) -> i32 {
    if pCtx.param_opt().is_none() {
        return 0;
    }
    if !pCtx.param().bEnableLongTermReference {
        for iDid in 0..(pCtx.param().iSpatialLayerNum as usize) {
            pCtx.param_mut().sDependencyLayers[iDid].bEncCurFrmAsIdrFlag = true;
        }
    } else {
        let pRequest = pLTRRecoverRequest;
        let iLayerId = pRequest.iLayerId;
        if iLayerId < 0 || iLayerId >= pCtx.param().iSpatialLayerNum {
            return 0;
        }

        // The C++ dereferences here unconditionally; an absent SPS contributes the
        // same `1 << 0` this expression would have read from a zeroed record.
        let iMaxFrameNumPlus1 = 1 << ctx_sps_ref(pCtx).map_or(0, |s| s.uiLog2MaxFrameNum);
        let kuiIdrPicId = pCtx.param().sDependencyLayers[iLayerId as usize].uiIdrPicId;
        let pLtr = ctx_ltr_at(pCtx, (iLayerId as usize) as usize);

        if pRequest.uiFeedbackType == LTR_RECOVERY_REQUEST && pRequest.uiIDRPicId == kuiIdrPicId as u32 {
            if pRequest.iLastCorrectFrameNum == -1 {
                pCtx.param_mut().sDependencyLayers[iLayerId as usize].bEncCurFrmAsIdrFlag = true;
                return 1;
            } else if pRequest.iCurrentFrameNum == -1 {
                pLtr.bReceivedT0LostFlag = true;
                return 1;
            } else {
                let cond1 = (CompareFrameNum(pLtr.iLastRecoverFrameNum, pRequest.iLastCorrectFrameNum, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0;
                let cond2 = ((CompareFrameNum(pLtr.iLastRecoverFrameNum, pRequest.iCurrentFrameNum, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0)
                    && CompareFrameNum(pLtr.iLastRecoverFrameNum, pRequest.iLastCorrectFrameNum, iMaxFrameNumPlus1)
                        == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32;

                if cond1 || cond2 {
                    pLtr.bReceivedT0LostFlag = true;
                    pLtr.iLastCorFrameNumDec = pRequest.iLastCorrectFrameNum;
                    pLtr.iCurFrameNumInDec = pRequest.iCurrentFrameNum;
                }
            }
        }
    }
    1
}

/// Updates LTR marking confirmation or failure feedback from decoder.
pub fn FilterLTRMarkingFeedback(
    pCtx: &mut sWelsEncCtx,
    pLTRMarkingFeedback: &mut SLTRMarkingFeedback,
) {
    if pCtx.param_opt().is_none() {
        return;
    }
    let iLayerId = pLTRMarkingFeedback.iLayerId;
    if iLayerId < 0 || iLayerId >= pCtx.param().iSpatialLayerNum {
        return;
    }
    let kbEnableLtr = pCtx.param().bEnableLongTermReference;
    let kuiIdrPicId = pCtx.param().sDependencyLayers[iLayerId as usize].uiIdrPicId;
    let pLtr = ctx_ltr_at(pCtx, (iLayerId as usize) as usize);
    if kbEnableLtr {
        if pLTRMarkingFeedback.uiIDRPicId == kuiIdrPicId as u32
            && (pLTRMarkingFeedback.uiFeedbackType == LTR_MARKING_SUCCESS
                || pLTRMarkingFeedback.uiFeedbackType == LTR_MARKING_FAILED)
        {
            pLtr.uiLtrMarkState = pLTRMarkingFeedback.uiFeedbackType;
            pLtr.iLtrMarkFbFrameNum = pLTRMarkingFeedback.iLTRFrameNum;
        }
    }
}

/// Builds active reference picture list pRefList0 for motion estimation.
pub fn WelsBuildRefList(
    pCtx: &mut sWelsEncCtx,
    kiPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).is_none() {
        return false;
    }
    let kiNumRef = pCtx.param().iNumRefFrame;
    let kuiTid = pCtx.uiTemporalId;
    let kiFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;

    pCtx.iNumRef0 = 0;
    if pCtx.eSliceType != EWelsSliceType::I_SLICE {
        if pCtx.param().bEnableLongTermReference
            && ctx_ltr_at(pCtx, (uiDid) as usize).bReceivedT0LostFlag
            && pCtx.uiTemporalId == 0
        {
            let longCount = pCtx.ref_list(uiDid).expect("the list was checked at entry").uiLongRefCount as usize;
            for i in 0..longCount {
                let Some(idLong) = pCtx.ref_list(uiDid).expect("the list was checked at entry").pLongRefList[i] else {
                    continue;
                };
                if pCtx.ref_list(uiDid).expect("the list was checked at entry").pic(idLong).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    let numRef0 = pCtx.iNumRef0 as usize;
                    // The camera path puts a *reconstruction* picture in `pRefOri`
                    // where the screen path puts a source picture — see `PicRef`.
                    current_layer_expect_mut(pCtx).pRefOri[numRef0] = Some(PicRef::Rec(idLong));
                    pCtx.pRefList0[numRef0] = Some(idLong);
                    pCtx.iNumRef0 += 1;
                    ctx_ltr_at(pCtx, (uiDid) as usize).iLastRecoverFrameNum = kiFrameNum;
                    break;
                }
            }
        } else {
            let shortCount = pCtx.ref_list(uiDid).expect("the list was checked at entry").uiShortRefCount as usize;
            for i in 0..shortCount {
                let Some(idRef) = pCtx.ref_list(uiDid).expect("the list was checked at entry").pShortRefList[i] else {
                    continue;
                };
                let bTake = {
                    let pRef = pCtx.ref_list(uiDid).expect("the list was checked at entry").pic(idRef);
                    pRef.bUsedAsRef && pRef.iFramePoc >= 0 && pRef.uiTemporalId <= kuiTid
                };
                if bTake {
                    let numRef0 = pCtx.iNumRef0 as usize;
                    current_layer_expect_mut(pCtx).pRefOri[numRef0] = Some(PicRef::Rec(idRef));
                    pCtx.pRefList0[numRef0] = Some(idRef);
                    pCtx.iNumRef0 += 1;
                }
            }
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(ctx_ltr_at(pCtx, (uiDid) as usize));
        for k in 0..MAX_TEMPORAL_LEVEL {
            pCtx.bRefOfCurTidIsLtr[uiDid][k] = false;
        }
        pCtx.pRefList0[0] = None;
    }

    if pCtx.iNumRef0 as i32 > kiNumRef {
        pCtx.iNumRef0 = kiNumRef as u8;
    }
    pCtx.iNumRef0 > 0 || pCtx.eSliceType == EWelsSliceType::I_SLICE
}

/// Invokes VPP UpdateBlockIdcForScreen to update static block map.
pub fn UpdateBlockStatic(pCtx: &mut sWelsEncCtx) {
    if pCtx.vaa().is_none() || pCtx.pVpp.is_none() {
        return;
    }
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
    //
    // `None` for camera content, where no extension exists and the walk
    // below has nothing to consider; `Some` under `SCREEN_CONTENT_REAL_TIME`.
    let Some(pVaaExt) = pCtx.vaa_ext_ref() else {
        return;
    };
    let iVaaBestRefFrameNum = pVaaExt.iVaaBestRefFrameNum;
    let pVaaBestBlockStaticIdc = pVaaExt.pVaaBestBlockStaticIdc;
    let uiDid = pCtx.uiDependencyId as usize;
    let idEnc = pCtx.pEncPic;
    let kiNumRef0 = pCtx.iNumRef0 as usize;
    let pRefList0 = pCtx.pRefList0;
    crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
        for idx in 0..kiNumRef0 {
            let Some(idRef) = pRefList0[idx] else {
                continue;
            };
            let Some(idSrc) = idEnc else {
                continue;
            };
            // **The block-static grid is the *source* picture's**, `(w >> 3) *
            // (h >> 3)` of its aligned size — the same grid `SetBlockStaticIdcToMd`
            // reads back with `kiBlocks = (kiMbWidth << 1) * (kiMbHeight << 1)`, and
            // the same one `DetectSceneChangeScreen` wrote.
            let kiBlocksInFrame = {
                let src_pic = pVpp.m_pSpatialPicPool.get(idSrc);
                ((src_pic.iWidthInPixel >> 3) * (src_pic.iHeightInPixel >> 3)).max(0) as usize
            };

            let (pVaaExtMut, pRefList) = pCtx.vaa_ext_and_ref_list_mut(uiDid);
            let (Some(pVaaExtMut), Some(pRefList)) = (pVaaExtMut, pRefList) else {
                continue;
            };
            let iFrameNum = pRefList.pic(idRef).iFrameNum;
            if iVaaBestRefFrameNum != iFrameNum {
                let ref_y = pRefList.pic(idRef).plane(0);
                // **The `None` here is where the C++ writes through a null row.**
                // `pVaaBestBlockStaticIdc` names no row when the store is
                // unallocated or the selector is past its rows — the state
                // `pVaaExt->pVaaBestBlockStaticIdc == NULL` names — and the C++
                // hands that null to the plugin, which post-increments through it.
                // The port refuses instead, silently, because the C++ discards this
                // call's return value and there is nothing to report it to.
                let Some(row) = pVaaExtMut
                    .pVaaBlockStaticIdc
                    .row_mut(pVaaBestBlockStaticIdc, kiBlocksInFrame)
                else {
                    continue;
                };
                pVpp.UpdateBlockIdcForScreen(
                    pVaaBestBlockStaticIdc,
                    row,
                    &ref_y.as_slice()[ref_y.origin()..],
                    ref_y.stride(),
                    idSrc,
                );
            }
        }
    });
}

/// Serializes slice header reference picture reordering syntax and marking flags.
/// The context values `WelsUpdateSliceHeaderSyntax` reads, resolved once by its
/// caller. None of them can change while that loop runs, because the loop
/// writes only slice headers.
pub struct SliceHeaderSyntaxIn {
    pub iNumRef0: i32,
    pub bEnableLongTermReference: bool,
    pub bScreenContent: bool,
    pub bLtrMarkingFlag: bool,
    /// `pRefList0[0]`'s picture is a long-term reference.
    pub bFirstRefIsLongRef: bool,
    /// `pRefList0[i]`'s long-term picture number, per reordering slot.
    pub iLongTermPicNum: [Option<u16>; MAX_REFERENCE_REORDER_COUNT_NUM],
}

pub fn WelsUpdateSliceHeaderSyntax(
    kSyn: &SliceHeaderSyntaxIn,
    iAbsDiffPicNumMinus1: i32,
    pCurDq: &mut SDqLayer,
    uiFrameType: i32,
) {
    let kiCountSliceNum = pCurDq.iMaxSliceNum;
    let bLtrMarkingFlag = kSyn.bLtrMarkingFlag;

    for iIdx in 0..kiCountSliceNum {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iIdx)
        else {
            continue;
        };
        let pSliceHdr = &mut pSlice.sSliceHeaderExt.sSliceHeader;
        let pRefReorder = &mut pSliceHdr.sRefReordering;
        let pRefPicMark = &mut pSliceHdr.sRefMarking;

        pSliceHdr.uiRefCount = kSyn.iNumRef0 as u8;
        if kSyn.iNumRef0 > 0 {
            if !kSyn.bFirstRefIsLongRef || !kSyn.bEnableLongTermReference {
                pRefReorder.SReorderingSyntax[0].uiReorderingOfPicNumsIdc = 0;
                pRefReorder.SReorderingSyntax[0].uiAbsDiffPicNumMinus1 = iAbsDiffPicNumMinus1 as u32;
                pRefReorder.SReorderingSyntax[1].uiReorderingOfPicNumsIdc = 3;
            } else {
                let mut iRefIdx = 0usize;
                while (iRefIdx as i32) < kSyn.iNumRef0 as i32 {
                    if iRefIdx < MAX_REFERENCE_REORDER_COUNT_NUM {
                        pRefReorder.SReorderingSyntax[iRefIdx].uiReorderingOfPicNumsIdc = 2;
                        if let Some(kiLongTermPicNum) = kSyn.iLongTermPicNum[iRefIdx] {
                            pRefReorder.SReorderingSyntax[iRefIdx].iLongTermPicNum = kiLongTermPicNum;
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
            pRefPicMark.bLongTermRefFlag = kSyn.bEnableLongTermReference;
        } else {
            // This arm drops `bLtrMarkingFlag` from the slice header's
            // adaptive-marking decision.
            if kSyn.bScreenContent {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = kSyn.bEnableLongTermReference;
            } else {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = kSyn.bEnableLongTermReference && bLtrMarkingFlag;
            }
        }
    }
}

/// Updates reference picture syntax and picture number delta in slice headers.
pub fn WelsUpdateRefSyntax(pCtx: &mut sWelsEncCtx, kiPOC: i32, kiFrameType: i32) {
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return;
    }
    let mut iAbsDiffPicNumMinus1 = -1i32;
    let uiDid = pCtx.uiDependencyId as usize;
    let pParamD = &pCtx.param().sDependencyLayers[uiDid];

    if pCtx.iNumRef0 > 0 {
        let pRefList = pCtx.ref_list(uiDid).expect("the dependency layer's reference list");
        if let Some(id) = pCtx.pRefList0[0] {
            iAbsDiffPicNumMinus1 = pParamD.iFrameNum - pRefList.pic(id).iFrameNum - 1;
            if iAbsDiffPicNumMinus1 < 0 {
                if let Some(kpSps) = ctx_sps_ref(pCtx) {
                    iAbsDiffPicNumMinus1 += 1 << kpSps.uiLog2MaxFrameNum;
                }
            }
        }
    }

    if current_layer_ref(pCtx).is_some() {
        let uiDidSh = pCtx.uiDependencyId as usize;
        let kSyn = {
            let kpRefList = pCtx.ref_list(uiDidSh);
            SliceHeaderSyntaxIn {
                iNumRef0: pCtx.iNumRef0 as i32,
                bEnableLongTermReference: pCtx.param().bEnableLongTermReference,
                bScreenContent: pCtx.param().iUsageType
                    == EUsageType::SCREEN_CONTENT_REAL_TIME,
                bLtrMarkingFlag: crate::encoder::encoder_context::ctx_ltr_at_ref(pCtx, uiDidSh)
                    .bLTRMarkingFlag,
                bFirstRefIsLongRef: match pCtx.pRefList0[0] {
                    Some(id) => kpRefList
                        .expect("the dependency layer's reference list")
                        .pic(id)
                        .bIsLongRef,
                    None => false,
                },
                iLongTermPicNum: std::array::from_fn(|i| {
                    pCtx.pRefList0.get(i).copied().flatten().map(|id| {
                        kpRefList
                            .expect("the dependency layer's reference list")
                            .pic(id)
                            .iLongTermPicNum as u16
                    })
                }),
            }
        };
        let sWelsEncCtx { ppDqLayerList, iCurDqLayer, .. } = &mut *pCtx;
        let pCurLayerForSh = iCurDqLayer
            .and_then(|idx| ppDqLayerList.get_mut(idx.get()))
            .and_then(|l| l.as_deref_mut());
        if let Some(pCurLayerForSh) = pCurLayerForSh {
            WelsUpdateSliceHeaderSyntax(
                &kSyn,
                iAbsDiffPicNumMinus1,
                pCurLayerForSh,
                kiFrameType,
            );
        }
    }
}

/// Synchronizes reconstructed picture metadata back to the source input picture.
/// `pOrigPic` is a spatial source picture, `pReconPic` a reconstruction picture.
pub fn UpdateOriginalPicInfo(pOrigPic: &mut SPicture, pReconPic: &SPicture) {
    pOrigPic.iPictureType = pReconPic.iPictureType;
    pOrigPic.iFramePoc = pReconPic.iFramePoc;
    pOrigPic.iFrameNum = pReconPic.iFrameNum;
    pOrigPic.uiSpatialId = pReconPic.uiSpatialId;
    pOrigPic.uiTemporalId = pReconPic.uiTemporalId;
    pOrigPic.iLongTermPicNum = pReconPic.iLongTermPicNum;
    pOrigPic.bUsedAsRef = pReconPic.bUsedAsRef;
    pOrigPic.bIsLongRef = pReconPic.bIsLongRef;
    pOrigPic.bIsSceneLTR = pReconPic.bIsSceneLTR;
    pOrigPic.iFrameAverageQp = pReconPic.iFrameAverageQp;
}

/// `UpdateOriginalPicInfo` over the context's current pair, resolving each handle in
/// its own pool. A no-op if either is unset, as the C++'s null tests are.
fn UpdateOriginalPicInfoFromCtx(pCtx: &mut sWelsEncCtx) {
    let (Some(idEnc), Some(idDec)) = (pCtx.pEncPic, pCtx.pDecPic) else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    let (pVpp, pRefList) = pCtx.vpp_and_ref_list_mut(uiDid);
    let (Some(pVpp), Some(pRefList)) = (pVpp, pRefList) else {
        return;
    };
    let pRecon: &SPicture = pRefList.pic(idDec);
    let pOrig: &mut SPicture = pVpp.m_pSpatialPicPool.get_mut(idEnc);
    UpdateOriginalPicInfo(pOrig, pRecon);
}

pub fn UpdateSrcPicListLosslessScreenRefSelectionWithLtr(pCtx: &mut sWelsEncCtx) {
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if pCtx.pVpp.is_some() && pCtx.vaa().is_some() {
        let idEnc = pCtx.pEncPic;
        let uiMarkLongTermPicIdx = pCtx.vaa_expect().uiMarkLongTermPicIdx as i32;
        let (pVpp, pRefList) = pCtx.vpp_and_ref_list_mut(iDIdx as usize);
        let pVpp = pVpp.expect("the preprocess object");
        let pRefList = pRefList.expect("the dependency layer's reference list");
        // wels_preprocess.h:143 takes const int32_t; the uint8_t field promotes.
        pVpp.UpdateSrcListLosslessScreenRefSelectionWithLtr(
            idEnc,
            iDIdx,
            uiMarkLongTermPicIdx,
            pRefList,
        );
    }
}

pub fn UpdateSrcPicList(pCtx: &mut sWelsEncCtx) {
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if pCtx.pVpp.is_some() {
        let idEnc = pCtx.pEncPic;
        let (pVpp, pRefList) = pCtx.vpp_and_ref_list_mut(iDIdx as usize);
        let shortCount = pRefList
            .expect("the dependency layer's reference list")
            .uiShortRefCount;
        pVpp.expect("the preprocess object").UpdateSrcList(idEnc, iDIdx, shortCount as u32);
    }
}

/// Screen content specialized reference picture list update.
pub fn WelsUpdateRefListScreen(pCtx: &mut sWelsEncCtx) -> bool {
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).map_or(true, |l| l.pRef.is_empty()) {
        return false;
    }
    let (kiHighestTid, kiFrameNum, kiPOC) = {
        let pParamD = &pCtx.param().sDependencyLayers[uiDid];
        (pParamD.iHighestTemporalId, pParamD.iFrameNum, pParamD.iPOC)
    };
    let kuiTid = pCtx.uiTemporalId;

    if let Some(idDec) = pCtx.pDecPic {
        let uiTemporalId = pCtx.uiTemporalId;
        let uiDependencyId = pCtx.uiDependencyId;
        let bIsSceneLTR = ctx_ltr_at(pCtx, uiDid).bLTRMarkingFlag
            || (pCtx.param().bEnableLongTermReference
                && pCtx.eSliceType == EWelsSliceType::I_SLICE);
        let iLongTermPicNum = ctx_ltr_at(pCtx, uiDid).iCurLtrIdx;
        let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
            return false;
        };
        // The reconstruction picture, resolved once.
        let pDecPic: &mut SPicture = pRefList.pic_mut(idDec);
        let sDec = pDecPic.planes();
        if kiHighestTid == 0 || (kuiTid as i32) < kiHighestTid as i32 {
            // `ref_list_mgr_svc.cpp:779`.
            pDecPic.expand_as_reference();
        }

        pDecPic.uiTemporalId = uiTemporalId;
        pDecPic.uiSpatialId = uiDependencyId;
        pDecPic.iFrameNum = kiFrameNum;
        pDecPic.iFramePoc = kiPOC;
        pDecPic.bUsedAsRef = true;
        pDecPic.bIsLongRef = true;
        pDecPic.bIsSceneLTR = bIsSceneLTR;
        pDecPic.iLongTermPicNum = iLongTermPicNum;
    }

    if pCtx.eSliceType == EWelsSliceType::P_SLICE {
        DeleteNonSceneLTR(pCtx);
        LTRMarkProcessScreen(pCtx);
        let pLtr = ctx_ltr_at(pCtx, uiDid);
        pLtr.bLTRMarkingFlag = false;
        pLtr.uiLtrMarkInterval += 1;
    } else {
        LTRMarkProcessScreen(pCtx);
        let pLtr = ctx_ltr_at(pCtx, uiDid);
        pLtr.iCurLtrIdx = 1;
        pLtr.iSceneLtrIdx = 1;
        pLtr.uiLtrMarkInterval = 0;
        if pCtx.vaa().is_some() {
            pCtx.vaa_expect_mut().uiValidLongTermPicIdx = 0;
        }
    }

    pCtx.eRefStrategy.EndofUpdateRefList(pCtx);
    true
}

/// Screen content specialized reference picture list builder.
pub fn WelsBuildRefListScreen(
    pCtx: &mut sWelsEncCtx,
    iPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    if pCtx.param_opt().is_none() || pCtx.vaa().is_none() || current_layer_ref(pCtx).is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let iNumRef = pCtx.param().iNumRefFrame;
    let iLTRRefNum = pCtx.param().iLTRRefNum;
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
    //
    // `None` for camera content, where zero available screen references
    // is the value every camera preset computes here; the extension's own count
    // under `SCREEN_CONTENT_REAL_TIME`.
    let iNumOfAvailableRef = pCtx.vaa_ext_ref().map_or(0, |ext| ext.iNumOfAvailableRef);
    pCtx.iNumRef0 = 0;

    if pCtx.eSliceType != EWelsSliceType::I_SLICE {
        let mut iLtrRefIdx = 0i32;
        // The screen path's `pRefOri` is a **spatial source** picture, where the
        // camera path's is a reconstruction picture — see [`PicRef`].
        let mut pRefOri: Option<SrcPicId> = None;

        for idx in 0..iNumOfAvailableRef {
            let bSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
            if pCtx.pVpp.is_some() {
                iLtrRefIdx = crate::encoder::encoder_context::with_vpp(pCtx, |pVpp, pCtx| {
                    pVpp.GetRefFrameInfo(pCtx, idx, bSceneLtr, &mut pRefOri)
                });
            }
            let refOri = pRefOri.map(PicRef::Src);
            if iLtrRefIdx >= 0 && iLtrRefIdx <= iLTRRefNum {
                let Some(idRefPic) = pCtx.ref_list(uiDid).expect("the dependency layer's reference list").pLongRefList[iLtrRefIdx as usize] else {
                    continue;
                };
                let bTake = {
                    let pRefPic = pCtx.ref_list(uiDid).expect("the dependency layer's reference list").pic(idRefPic);
                    pRefPic.bUsedAsRef
                        && pRefPic.bIsLongRef
                        && pRefPic.uiTemporalId <= pCtx.uiTemporalId
                        && (!pCtx.bCurFrameMarkedAsSceneLtr || pRefPic.bIsSceneLTR)
                };
                if bTake {
                    let num0 = pCtx.iNumRef0 as usize;
                    current_layer_expect_mut(pCtx).pRefOri[num0] = refOri;
                    pCtx.pRefList0[num0] = Some(idRefPic);
                    pCtx.iNumRef0 += 1;
                    // `ref_list_mgr_svc.cpp:829-834`.
                    let (kiRefFrameNum, kuiRefTid, kbRefIsSceneLtr) = {
                        let pRefPic = pCtx
                            .ref_list(uiDid)
                            .expect("the dependency layer's reference list")
                            .pic(idRefPic);
                        (pRefPic.iFrameNum, pRefPic.uiTemporalId, pRefPic.bIsSceneLTR)
                    };
                    let kuiLongRefCount = pCtx
                        .ref_list(uiDid)
                        .expect("the dependency layer's reference list")
                        .uiLongRefCount;
                    // `pParamD->iFrameNum` — the *parameter* layer's frame number
                    // (`ref_list_mgr_svc.cpp:815`), not the DQ layer's.
                    let kiCurFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;
                    let kuiTid = pCtx.uiTemporalId;
                    crate::common::wels_trace::WelsLog(
                        pCtx.sLogCtx,
                        crate::common::wels_trace::WELS_LOG_DEBUG,
                        &format!(
                            "WelsBuildRefListScreen(), current iFrameNum = {}, current Tid = {}, ref iFrameNum = {}, ref uiTemporalId = {}, ref is Scene LTR = {}, LTR count = {},iNumRef = {}",
                            kiCurFrameNum,
                            kuiTid,
                            kiRefFrameNum,
                            kuiRefTid,
                            kbRefIsSceneLtr as i32,
                            kuiLongRefCount,
                            iNumRef
                        ),
                    );
                }
            } else {
                let mut i = iNumRef;
                while i >= 0 {
                    let Some(idLong) = pCtx.ref_list(uiDid).expect("the dependency layer's reference list").pLongRefList[i as usize] else {
                        i -= 1;
                        continue;
                    };
                    let uiTemporalId = pCtx.ref_list(uiDid).expect("the dependency layer's reference list").pic(idLong).uiTemporalId;
                    if uiTemporalId == 0 || uiTemporalId < pCtx.uiTemporalId {
                        let num0 = pCtx.iNumRef0 as usize;
                        current_layer_expect_mut(pCtx).pRefOri[num0] = refOri;
                        pCtx.pRefList0[num0] = Some(idLong);
                        pCtx.iNumRef0 += 1;
                        // `ref_list_mgr_svc.cpp:845-848`. The C++ reads back
                        // `pRefList0[iNumRef0 - 1]->iFrameNum`, which is the slot
                        // just pushed — `idLong`.
                        let kiRefFrameNum = pCtx
                            .ref_list(uiDid)
                            .expect("the dependency layer's reference list")
                            .pic(idLong)
                            .iFrameNum;
                        let kuiLongRefCount = pCtx
                            .ref_list(uiDid)
                            .expect("the dependency layer's reference list")
                            .uiLongRefCount;
                        let kiCurFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;
                        crate::common::wels_trace::WelsLog(
                            pCtx.sLogCtx,
                            crate::common::wels_trace::WELS_LOG_DEBUG,
                            &format!(
                                "WelsBuildRefListScreen(), ref !current iFrameNum = {}, ref iFrameNum = {},LTR number = {}",
                                kiCurFrameNum, kiRefFrameNum, kuiLongRefCount
                            ),
                        );
                        break;
                    }
                    i -= 1;
                }
            }
        }

        // `ref_list_mgr_svc.cpp:853-875` — the reference-list dump, after the walk
        // and still inside the non-I arm. `%d` of a C++ `bool` prints 0/1; the two
        // `uint8_t`s promote to `int` and print as the numbers they are. The `\t`
        // is upstream's literal tab.
        crate::common::wels_trace::WelsLog(
            pCtx.sLogCtx,
            crate::common::wels_trace::WELS_LOG_DEBUG,
            &format!(
                "WelsBuildRefListScreen(), CurrentFramePoc={}, isLTR={}",
                iPOC, pCtx.bCurFrameMarkedAsSceneLtr as i32
            ),
        );
        for j in 0..iNumRef {
            let pARefPicture = pCtx
                .ref_list(uiDid)
                .expect("the dependency layer's reference list")
                .pLongRefList[j as usize];
            let line = match pARefPicture {
                Some(idA) => {
                    let a = pCtx
                        .ref_list(uiDid)
                        .expect("the dependency layer's reference list")
                        .pic(idA);
                    format!(
                        "WelsBuildRefListScreen()\tRefLot[{}]: iPoc={}, iPictureType={}, bUsedAsRef={}, bIsLongRef={}, bIsSceneLTR={}, uiTemporalId={}, iFrameNum={}, iMarkFrameNum={}, iLongTermPicNum={}, uiRecieveConfirmed={}",
                        j,
                        a.iFramePoc,
                        a.iPictureType,
                        a.bUsedAsRef as i32,
                        a.bIsLongRef as i32,
                        a.bIsSceneLTR as i32,
                        a.uiTemporalId,
                        a.iFrameNum,
                        a.iMarkFrameNum,
                        a.iLongTermPicNum,
                        a.uiRecieveConfirmed
                    )
                }
                None => format!("WelsBuildRefListScreen()\tRefLot[{}]: NULL", j),
            };
            crate::common::wels_trace::WelsLog(
                pCtx.sLogCtx,
                crate::common::wels_trace::WELS_LOG_DEBUG,
                &line,
            );
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(ctx_ltr_at(pCtx, (uiDid) as usize));
        pCtx.pRefList0[0] = None;
    }

    if pCtx.iNumRef0 as i32 > iNumRef {
        pCtx.iNumRef0 = iNumRef as u8;
    }
    pCtx.iNumRef0 > 0 || pCtx.eSliceType == EWelsSliceType::I_SLICE
}

pub fn IsValidFrameNum(kiFrameNum: i32) -> bool {
    kiFrameNum < (1 << 30)
}

pub fn WelsMarkMMCORefInfoScreen(
    kiNumRefFrame: i32,
    kbEnableLongTermReference: bool,
    pLtr: &SLTRState,
    pCurDq: &mut SDqLayer,
    kiCountSliceNum: i32,
) {
    if kiCountSliceNum <= 0 {
        return;
    }
    let Some(pBaseSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, 0) else {
        return;
    };
    let pRefPicMark = &mut pBaseSlice.sSliceHeaderExt.sSliceHeader.sRefMarking;
    let iMaxLtrIdx = kiNumRefFrame - STR_ROOM - 1;

    *pRefPicMark = SRefPicMarking::default();
    if kbEnableLongTermReference {
        let count0 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count0].iMaxLongTermFrameIdx = iMaxLtrIdx;
        pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SET_MAX_LONG;
        pRefPicMark.uiMmcoCount += 1;

        let count1 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count1].iLongTermFrameIdx = pLtr.iCurLtrIdx;
        pRefPicMark.SMmcoRef[count1].iMmcoType = MMCO_LONG;
        pRefPicMark.uiMmcoCount += 1;
    }

    let kBaseMarking = *pRefPicMark;
    WelsMarkMMCORefInfoWithBase(pCurDq, kBaseMarking, kiCountSliceNum);
}

pub fn WelsMarkPicScreen(pCtx: &mut sWelsEncCtx) {
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let gopSize = pCtx.param().uiGopSize;
    let kbEnableLtr = pCtx.param().bEnableLongTermReference;
    let iNumRef = pCtx.param().iNumRefFrame;
    let iMaxTid = if gopSize > 0 { (31 - gopSize.leading_zeros()) as i32 } else { 0 };
    let mut iMaxActualLtrIdx = -1i32;
    let kiParamDFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;

    if kbEnableLtr {
        let maxTidAdj = if iMaxTid > 1 { iMaxTid } else { 1 };
        iMaxActualLtrIdx = iNumRef - STR_ROOM - 1 - maxTidAdj;
    }

    let iLongRefNum = iNumRef - STR_ROOM;
    let bIsRefListNotFull = (pCtx
        .ref_list(uiDid)
        .expect("the dependency layer's reference list")
        .uiLongRefCount as i32)
        < iLongRefNum;
    // `None` contributes the same `1 << 0` a zeroed record would have, as in
    // `FilterLTRRecoveryRequest`.
    let kuiLog2MaxFrameNum = ctx_sps_ref(pCtx).map_or(0, |s| s.uiLog2MaxFrameNum);
    let kuiTid = pCtx.uiTemporalId;
    let kbSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
    let iSliceNum = current_layer_expect(pCtx).iMaxSliceNum;
    let (pRefList, pLtr) = pCtx.ref_list_and_ltr_mut(uiDid);
    let Some(pRefList) = pRefList else {
        return;
    };

    if !kbEnableLtr {
        pLtr.iCurLtrIdx = kuiTid as i32;
    } else {
        if iMaxActualLtrIdx != -1 && kuiTid == 0 && kbSceneLtr {
            pLtr.bLTRMarkingFlag = true;
            pLtr.uiLtrMarkInterval = 0;
            pLtr.iCurLtrIdx = pLtr.iSceneLtrIdx % (iMaxActualLtrIdx + 1);
            pLtr.iSceneLtrIdx += 1;
        } else {
            pLtr.bLTRMarkingFlag = false;
            if bIsRefListNotFull {
                for i in 0..iLongRefNum {
                    if pRefList.pLongRefList[i as usize].is_none() {
                        pLtr.iCurLtrIdx = i;
                        break;
                    }
                }
            } else {
                let mut iRefNum_t = [0i32; MAX_TEMPORAL_LAYER_NUM];
                for i in 0..(pRefList.uiLongRefCount as usize) {
                    let Some(idPic) = pRefList.pLongRefList[i] else {
                        continue;
                    };
                    let pPic = pRefList.pic(idPic);
                    if pPic.bUsedAsRef && pPic.bIsLongRef && !pPic.bIsSceneLTR {
                        let tid = pPic.uiTemporalId as usize;
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
                let iMaxFrameNum = 1 << kuiLog2MaxFrameNum;

                for i in 0..(pRefList.uiLongRefCount as usize) {
                    let Some(idPic) = pRefList.pLongRefList[i] else {
                        continue;
                    };
                    let pPic = pRefList.pic(idPic);
                    if pPic.bUsedAsRef
                        && pPic.bIsLongRef
                        && !pPic.bIsSceneLTR
                        && iMaxMultiRefTid == pPic.uiTemporalId as i32
                    {
                        if !IsValidFrameNum(pPic.iFrameNum) {
                            return;
                        }
                        let iDeltaFrameNum = if kiParamDFrameNum >= pPic.iFrameNum {
                            kiParamDFrameNum - pPic.iFrameNum
                        } else {
                            kiParamDFrameNum + iMaxFrameNum - pPic.iFrameNum
                        };

                        if iDeltaFrameNum > iLongestDeltaFrameNum {
                            pLtr.iCurLtrIdx = pPic.iLongTermPicNum;
                            iLongestDeltaFrameNum = iDeltaFrameNum;
                        }
                    }
                }
            }
        }
    }

    for i in 0..MAX_TEMPORAL_LAYER_NUM {
        if (kuiTid as usize) < i || kuiTid == 0 {
            pLtr.iLastLtrIdx[i] = pLtr.iCurLtrIdx;
        }
    }

    let kLtr = *pLtr;
    WelsMarkMMCORefInfoScreen(
        iNumRef,
        kbEnableLtr,
        &kLtr,
        current_layer_expect_mut(pCtx),
        iSliceNum,
    );
}

/// Intentional no-op reference list manager callback.
/// Matches `void DoNothing (sWelsEncCtx* pointer)` in `ref_list_mgr_svc.cpp:996`.
pub fn DoNothing(_pCtx: &mut sWelsEncCtx) {}

// ============================================================================
// Reference strategy
// ============================================================================

/// Which reference-list strategy an encoder runs.
///
/// C++ declares three classes deriving from `IWelsReferenceStrategy`
/// (`ref_list_mgr_svc.h`): `CWelsReference_TemporalLayer`, `CWelsReference_Screen`
/// and `CWelsReference_LosslessWithLtr`.
///
/// `TemporalLayer = 0` and `#[derive(Default)]` on it: `sWelsEncCtx` is
/// `mem::zeroed()`-constructed (`encoder_context.rs:514`), so the all-zero pattern has
/// to be a declared variant.
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum RefStrategyKind {
    /// `CWelsReference_TemporalLayer` — camera content, and every non-screen usage.
    #[default]
    TemporalLayer = 0,
    /// `CWelsReference_Screen` — screen content without LTR.
    Screen = 1,
    /// `CWelsReference_LosslessWithLtr` — screen content with LTR enabled.
    LosslessWithLtr = 2,
}

impl RefStrategyKind {
    /// `IWelsReferenceStrategy::CreateReferenceStrategy`.
    #[inline]
    pub fn Select(keUsageType: EUsageType, kbLtrEnabled: bool) -> Self {
        match keUsageType {
            EUsageType::SCREEN_CONTENT_REAL_TIME => {
                if kbLtrEnabled {
                    RefStrategyKind::LosslessWithLtr
                } else {
                    RefStrategyKind::Screen
                }
            }
            _ => RefStrategyKind::TemporalLayer,
        }
    }

    /// `BuildRefList` — `WelsBuildRefList` for the temporal-layer and screen
    /// strategies, `WelsBuildRefListScreen` for lossless-with-LTR.
    #[inline]
    pub fn BuildRefList(self, pCtx: &mut sWelsEncCtx, iPOC: i32, iBestLtrRefIdx: i32) -> bool {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => {
                WelsBuildRefList(pCtx, iPOC, iBestLtrRefIdx)
            }
            RefStrategyKind::LosslessWithLtr => WelsBuildRefListScreen(pCtx, iPOC, iBestLtrRefIdx),
        }
    }

    /// `MarkPic` — `ref_list_mgr_svc.cpp`'s `WelsMarkPic` / `WelsMarkPicScreen`.
    #[inline]
    pub fn MarkPic(self, pCtx: &mut sWelsEncCtx) {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => WelsMarkPic(pCtx),
            RefStrategyKind::LosslessWithLtr => WelsMarkPicScreen(pCtx),
        }
    }

    /// `UpdateRefList`.
    #[inline]
    pub fn UpdateRefList(self, pCtx: &mut sWelsEncCtx) -> bool {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => WelsUpdateRefList(pCtx),
            RefStrategyKind::LosslessWithLtr => WelsUpdateRefListScreen(pCtx),
        }
    }

    /// `EndofUpdateRefList` — `ref_list_mgr_svc.cpp:1041` / `:1057` / `:1073`. The one
    /// method where all three variants differ.
    #[inline]
    pub fn EndofUpdateRefList(self, pCtx: &mut sWelsEncCtx) {
        match self {
            RefStrategyKind::TemporalLayer => PrefetchNextBuffer(pCtx),
            RefStrategyKind::Screen => UpdateSrcPicList(pCtx),
            RefStrategyKind::LosslessWithLtr => {
                UpdateSrcPicListLosslessScreenRefSelectionWithLtr(pCtx)
            }
        }
    }

    /// `AfterBuildRefList` — `DoNothing` for the temporal-layer strategy,
    /// `UpdateBlockStatic` for both screen ones.
    #[inline]
    pub fn AfterBuildRefList(self, pCtx: &mut sWelsEncCtx) {
        match self {
            RefStrategyKind::TemporalLayer => DoNothing(pCtx),
            RefStrategyKind::Screen | RefStrategyKind::LosslessWithLtr => UpdateBlockStatic(pCtx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_list_mgr_noop_callback() {
        let mut ctx = Box::new(sWelsEncCtx::default());
        DoNothing(&mut ctx);
    }

    /// LTR only discriminates for screen content — a camera-content encoder gets
    /// `TemporalLayer` either way.
    #[test]
    fn ref_strategy_selection_matches_the_factory() {
        use EUsageType::*;
        assert_eq!(
            RefStrategyKind::Select(SCREEN_CONTENT_REAL_TIME, true),
            RefStrategyKind::LosslessWithLtr
        );
        assert_eq!(
            RefStrategyKind::Select(SCREEN_CONTENT_REAL_TIME, false),
            RefStrategyKind::Screen
        );
        for ltr in [true, false] {
            assert_eq!(
                RefStrategyKind::Select(CAMERA_VIDEO_REAL_TIME, ltr),
                RefStrategyKind::TemporalLayer,
                "camera content ignores LTR when picking a strategy (ltr={ltr})"
            );
        }
    }

    /// `sWelsEncCtx` is `mem::zeroed()`-constructed, so the zero discriminant has to
    /// be the variant the old factory's `_ =>` arm produced.
    #[test]
    #[allow(unsafe_code)]
    fn ref_strategy_zero_is_the_default_arm() {
        assert_eq!(RefStrategyKind::default(), RefStrategyKind::TemporalLayer);
        assert_eq!(RefStrategyKind::TemporalLayer as u8, 0);
        let zeroed: RefStrategyKind = unsafe { std::mem::zeroed() };
        assert_eq!(zeroed, RefStrategyKind::TemporalLayer);
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static REC_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
