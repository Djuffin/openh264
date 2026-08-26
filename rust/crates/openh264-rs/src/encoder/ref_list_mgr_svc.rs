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

#![deny(unsafe_code)]

use crate::encoder::picture::{PicRef, RecPicId, SrcPicId};
use crate::*;

// ============================================================================
// Constants
// ============================================================================

pub const STR_ROOM: i32 = 1;
// `MAX_SHORT_REF_COUNT`, `MAX_TEMPORAL_LEVEL` and `MAX_GOP_SIZE` are defined once in
// `encoder_context.rs` from `wels_const.h`. This module previously had its own copies
// with MAX_SHORT_REF_COUNT = 16 (C++: 4) and MAX_TEMPORAL_LEVEL = 8 (C++: 4).
pub use crate::encoder::encoder_context::{MAX_GOP_SIZE, MAX_SHORT_REF_COUNT, MAX_TEMPORAL_LEVEL};
use crate::encoder::encoder_context::{ctx_ltr_at, ctx_param, ctx_ref_list, ctx_vaa};
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
use crate::encoder::svc_encode_slice::ctx_sps;
use crate::encoder::svc_encode_slice::current_layer;
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::encoder_context::SLTRState;
// T4b.3b: `ExpandReferencingPicture` was one of three copies of one C++ function
// in this port. `common/expand_pic.rs` now holds the single one, and the
// `SExpandPicFunc` table it used to be handed is deleted.
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

// ============================================================================
// Global Reference Picture List Lifecycle Functions
// ============================================================================

/// Reset LTR marking, recovery, and feedback state to defaults.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsResetRefList(pCtx: &mut sWelsEncCtx) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }

    for i in 0..=(MAX_SHORT_REF_COUNT) {
        (*pRefList).pShortRefList[i] = None;
    }
    let ltrRefNum = if !ctx_param(pCtx).is_null() {
        (*ctx_param(pCtx)).iLTRRefNum as usize
    } else {
        0
    };
    for i in 0..=(ltrRefNum) {
        if i <= MAX_REF_PIC_COUNT {
            (*pRefList).pLongRefList[i] = None;
        }
    }
    let numRefFrame = if !ctx_param(pCtx).is_null() {
        (*ctx_param(pCtx)).iNumRefFrame as usize
    } else {
        0
    };
    for i in 0..=(numRefFrame) {
        if i < (*pRefList).pRef.len() {
            let id = (*pRefList).pRef.at(i);
            (*pRefList).pic_mut(id).SetUnref();
        }
    }

    (*pRefList).uiLongRefCount = 0;
    (*pRefList).uiShortRefCount = 0;
    (*pRefList).pNextBuffer = if (*pRefList).pRef.is_empty() {
        None
    } else {
        Some((*pRefList).pRef.at(0))
    };
}

/// Remove a long-term reference entry by index from pLongRefList.
///
/// **Narrowed to the list it edits — T9.G3, S54.** It took `*mut sWelsEncCtx` and
/// used it for exactly one thing: `ctx_ref_list(pCtx, (*pCtx).uiDependencyId)`.
/// Every one of its five callers already holds that same pointer, derived with the
/// same `uiDid` (`uiDependencyId` is written in one place in the encoder,
/// `encoder_ext.rs:3356`, and never inside this file), and every one has already
/// null-guarded it. So the context parameter carried no information the caller did
/// not have, and carrying it made this call a **whole-context retag in the middle
/// of five loops that hold cursors across it** — 8 of the 131 live hazards
/// (`phase9_ctx_join.py`), and, after the flip, five borrow-checker errors instead.
/// Taking the list retags the list.
pub fn DeleteLTRFromLongList(pRefList: &mut SRefList, iIdx: i32) {
    let count = pRefList.uiLongRefCount as i32;
    let mut k = iIdx;
    while k < count - 1 {
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
///
/// Narrowed with [`DeleteLTRFromLongList`] and for the same reason — T9.G3.
pub fn DeleteSTRFromShortList(pRefList: &mut SRefList, iIdx: i32) {
    let count = pRefList.uiShortRefCount as i32;
    let mut k = iIdx;
    while k < count - 1 {
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DeleteNonSceneLTR(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() {
        return;
    }
    let pRefList = ctx_ref_list(pCtx, pCtx.uiDependencyId as usize);
    if pRefList.is_null() {
        return;
    }
    let numRef = (*ctx_param(pCtx)).iNumRefFrame;
    let mut i = 0;
    while i < numRef {
        let hit = match (*pRefList).pLongRefList[i as usize] {
            Some(id) => {
                let pRef = (*pRefList).pic(id);
                pRef.bUsedAsRef
                    && pRef.bIsLongRef
                    && (!pRef.bIsSceneLTR)
                    && (pCtx.uiTemporalId < pRef.uiTemporalId
                        || pCtx.bCurFrameMarkedAsSceneLtr)
            }
            None => false,
        };
        if hit {
            let id = (*pRefList).pLongRefList[i as usize].expect("checked just above");
            (*pRefList).pic_mut(id).SetUnref();
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DeleteInvalidLTR(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_sps(pCtx).is_null() || ctx_param(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    // **T9.G4 — the derivation order is the fix, and the order is not arbitrary.**
    // Each of these three lines makes a whole-context call, so whatever is derived
    // first is held across the ones after it. Sorted by what the call becomes:
    // `ctx_sps` and `ctx_ltr_at` are ST-flippable and really will retag; `ctx_param`
    // is fork-reachable, so S63 keeps it `*mut` permanently and it never retags at
    // all (`phase9_ctx_join.py` calls that the moot half). So: the scalar first
    // while nothing is live, then the flipping accessor, then the permanent-raw one.
    let iMaxFrameNumPlus1 = 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;
    let pLtr = &mut *ctx_ltr_at(pCtx, (uiDid) as usize);
    let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);

    for i in 0..LONG_TERM_REF_NUM {
        if let Some(idPic) = (*pRefList).pLongRefList[i as usize] {
            let pPic = (*pRefList).pic(idPic);
            let cond1 = CompareFrameNum(pPic.iFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                && ((CompareFrameNum(pPic.iFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                    & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                    != 0);

            if cond1 {
                (*pRefList).pic_mut(idPic).SetUnref();
                DeleteLTRFromLongList(&mut *pRefList, i);
                pLtr.bLTRMarkEnable = true;
                if (*pRefList).uiLongRefCount == 0 {
                    (*pParamInternal).bEncCurFrmAsIdrFlag = true;
                }
            } else {
                let pPic = (*pRefList).pic(idPic);
                let cond2 = CompareFrameNum(pPic.iMarkFrameNum, pLtr.iLastCorFrameNumDec, iMaxFrameNumPlus1)
                    == COMPARE_FRAME_NUM::FRAME_NUM_BIGGER as i32
                    && ((CompareFrameNum(pPic.iMarkFrameNum, pLtr.iCurFrameNumInDec, iMaxFrameNumPlus1)
                        & (COMPARE_FRAME_NUM::FRAME_NUM_EQUAL as i32 | COMPARE_FRAME_NUM::FRAME_NUM_SMALLER as i32))
                        != 0)
                    && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32;

                if cond2 {
                    (*pRefList).pic_mut(idPic).SetUnref();
                    DeleteLTRFromLongList(&mut *pRefList, i);
                    pLtr.bLTRMarkEnable = true;
                    if (*pRefList).uiLongRefCount == 0 {
                        (*pParamInternal).bEncCurFrmAsIdrFlag = true;
                    }
                }
            }
        }
    }
}

/// Handles asynchronous decoder confirmation or failure feedback for LTR marking.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn HandleLTRMarkFeedback(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let pLtr = &mut *ctx_ltr_at(pCtx, (uiDid) as usize);
    let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);

    if pLtr.uiLtrMarkState == LTR_MARKING_SUCCESS {
        for i in 0..((*pRefList).uiLongRefCount as i32) {
            let Some(idPic) = (*pRefList).pLongRefList[i as usize] else {
                continue;
            };
            let pPic = (*pRefList).pic(idPic);
            if pPic.iFrameNum == pLtr.iLtrMarkFbFrameNum
                && pPic.uiRecieveConfirmed != RECIEVE_SUCCESS
            {
                (*pRefList).pic_mut(idPic).uiRecieveConfirmed = RECIEVE_SUCCESS;
                if !ctx_vaa(pCtx).is_null() {
                    (*ctx_vaa(pCtx)).uiValidLongTermPicIdx =
                        (*pRefList).pic(idPic).iLongTermPicNum as u8;
                }
                pLtr.iCurFrameNumInDec = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastRecoverFrameNum = pLtr.iLtrMarkFbFrameNum;
                pLtr.iLastCorFrameNumDec = pLtr.iLtrMarkFbFrameNum;

                let mut j = 0;
                while j < (*pRefList).uiLongRefCount as i32 {
                    let drop = match (*pRefList).pLongRefList[j as usize] {
                        Some(id) => (*pRefList).pic(id).iLongTermPicNum != pLtr.iCurLtrIdx,
                        None => false,
                    };
                    if drop {
                        let id = (*pRefList).pLongRefList[j as usize].expect("checked just above");
                        (*pRefList).pic_mut(id).SetUnref();
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
        for i in 0..((*pRefList).uiLongRefCount as i32) {
            let Some(idPic) = (*pRefList).pLongRefList[i as usize] else {
                continue;
            };
            if (*pRefList).pic(idPic).iFrameNum == pLtr.iLtrMarkFbFrameNum {
                (*pRefList).pic_mut(idPic).SetUnref();
                DeleteLTRFromLongList(&mut *pRefList, i);
                break;
            }
        }
        pLtr.uiLtrMarkState = NO_LTR_MARKING_FEEDBACK;
        pLtr.bLTRMarkEnable = true;

        if pLtr.iLTRMarkSuccessNum == 0 {
            (*pParamInternal).bEncCurFrmAsIdrFlag = true;
        }
    }
}

/// Executes promotion and movement of frames from short-term to long-term lists.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn LTRMarkProcess(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || ctx_sps(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    // T9.G4: derived above `pLtr`, not below it — these are whole-context calls
    // and `pLtr` is a cursor held to the end of the body.
    let gopSize = (*ctx_param(pCtx)).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;
    let pLtr = &mut *ctx_ltr_at(pCtx, (uiDid) as usize);
    let mut i = 0usize;
    let mut bMoveLtrFromShortToLong = false;
    let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);

    if pCtx.eSliceType == EWelsSliceType::I_SLICE {
        i = 0;
        if let Some(id) = (*pRefList).pShortRefList[i] {
            (*pRefList).pic_mut(id).uiRecieveConfirmed = RECIEVE_SUCCESS;
        }
    } else if pLtr.bLTRMarkingFlag {
        if !ctx_vaa(pCtx).is_null() {
            (*ctx_vaa(pCtx)).uiMarkLongTermPicIdx = pLtr.iCurLtrIdx as u8;
        }

        if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DELAY_MARK as i32 {
            for idx in 0..((*pRefList).uiShortRefCount as usize) {
                if let Some(id) = (*pRefList).pShortRefList[idx] {
                    if CompareFrameNum(
                        (*pParamInternal).iFrameNum,
                        (*pRefList).pic(id).iFrameNum + iGoPFrameNumInterval,
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

    if pCtx.eSliceType == EWelsSliceType::I_SLICE || pLtr.bLTRMarkingFlag {
        if let Some(id) = (*pRefList).pShortRefList[i] {
            let iFrameNum = (*pParamInternal).iFrameNum;
            let pShort = (*pRefList).pic_mut(id);
            pShort.bIsLongRef = true;
            pShort.iLongTermPicNum = pLtr.iCurLtrIdx;
            pShort.iMarkFrameNum = iFrameNum;
        }
    }

    if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32
        && pCtx.eSliceType != EWelsSliceType::I_SLICE
        && !pLtr.bLTRMarkingFlag
    {
        for j in 0..((*pRefList).uiShortRefCount as usize) {
            if let Some(id) = (*pRefList).pShortRefList[j] {
                if (*pRefList).pic(id).bIsLongRef {
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
        let tid = pCtx.uiTemporalId as usize;
        if uiDid < MAX_DEPENDENCY_LAYER && tid < MAX_TEMPORAL_LEVEL {
            pCtx.bRefOfCurTidIsLtr[uiDid][tid] = true;
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
        if (*pRefList).uiLongRefCount as i32 > (*ctx_param(pCtx)).iLTRRefNum {
            let lastIdx = ((*pRefList).uiLongRefCount - 1) as usize;
            if let Some(id) = (*pRefList).pLongRefList[lastIdx] {
                (*pRefList).pic_mut(id).SetUnref();
            }
            DeleteLTRFromLongList(&mut *pRefList, lastIdx as i32);
        }
        DeleteSTRFromShortList(&mut *pRefList, i as i32);
    }
}

/// Executes promotion of screen content references to long-term reference slots.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn LTRMarkProcessScreen(pCtx: &mut sWelsEncCtx) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let Some(idDec) = pCtx.pDecPic else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let iLtrIdx = (*pRefList).pic(idDec).iLongTermPicNum;
    if !ctx_vaa(pCtx).is_null() {
        (*ctx_vaa(pCtx)).uiMarkLongTermPicIdx = iLtrIdx as u8;
    }

    if iLtrIdx >= 0 && (iLtrIdx as usize) < MAX_REF_PIC_COUNT {
        match (*pRefList).pLongRefList[iLtrIdx as usize] {
            Some(id) => (*pRefList).pic_mut(id).SetUnref(),
            None => (*pRefList).uiLongRefCount += 1,
        }
        (*pRefList).pLongRefList[iLtrIdx as usize] = Some(idDec);
    }
}

/// Pre-allocates destination frame buffer pointer pDecPic for upcoming reconstruction.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn PrefetchNextBuffer(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return;
    }
    let kiNumRef = (*ctx_param(pCtx)).iNumRefFrame;

    (*pRefList).pNextBuffer = None;
    for i in 0..=(kiNumRef as usize) {
        if i < (*pRefList).pRef.len() {
            let id = (*pRefList).pRef.at(i);
            if !(*pRefList).pic(id).bUsedAsRef {
                (*pRefList).pNextBuffer = Some(id);
                break;
            }
        }
    }

    if (*pRefList).pNextBuffer.is_none() && (*pRefList).uiShortRefCount > 0 {
        let lastIdx = ((*pRefList).uiShortRefCount - 1) as usize;
        (*pRefList).pNextBuffer = (*pRefList).pShortRefList[lastIdx];
        if let Some(id) = (*pRefList).pNextBuffer {
            (*pRefList).pic_mut(id).SetUnref();
        }
    }

    pCtx.pDecPic = (*pRefList).pNextBuffer;
}

/// Updates reference picture list after current frame reconstruction.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUpdateRefList(pCtx: &mut sWelsEncCtx) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if current_layer(pCtx).is_null() || ctx_param(pCtx).is_null() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() || (*pRefList).pRef.is_empty() {
        return false;
    }

    // **T9.G7 — raw, not `&mut`.** This body holds the LTR state across calls that
    // derive their own `&mut` to the *same* `SLTRState` (`LTRMarkProcess` and the
    // rest re-derive `ctx_ltr_at(pCtx, uiDid)` for this same `uiDid`). Two Unique
    // tags from one raw root are siblings, and the second pops the first — so the
    // `&mut` binding was the hazard, not the holding. A raw cursor with a deref at
    // each use is the port's own idiom and is what F66 says is sound here.
    let pLtr = ctx_ltr_at(pCtx, (uiDid) as usize);
    let pParamD = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);
    let kuiTid = pCtx.uiTemporalId;
    let kuiDid = pCtx.uiDependencyId;
    let keSliceType = pCtx.eSliceType;

    if let Some(idDec) = pCtx.pDecPic {
        // The reconstruction picture, resolved once — S37: everything below either
        // reads its geometry or writes its own fields, and the borrow ends before the
        // reference-list shifts that would touch the pool again.
        let pDecPic: &mut SPicture = (*pRefList).pic_mut(idDec);
        let sDec = pDecPic.planes();
        if (*pParamD).iHighestTemporalId == 0 || (kuiTid as i32) < (*pParamD).iHighestTemporalId as i32 {
            // T4b.3b: the `pFuncList` null guard went with the table it guarded.
            // The C++ (`ref_list_mgr_svc.cpp:375`) dereferences `pCtx->pFuncList`
            // here unconditionally, so dropping it is the closer reading.
            // T6.F4: the picture owns its planes, so the expansion is a method on it
            // and `ExpandReferencingPicture`'s three raw origins are gone from the
            // encoder entirely.
            pDecPic.expand_as_reference();
        }

        if crate::encoder::dump_enabled(&REC_DUMP, "OH264_RECDUMP") {
            for pl in 0..3usize {
                let w = if pl != 0 { pDecPic.iWidthInPixel >> 1 } else { pDecPic.iWidthInPixel };
                let h = if pl != 0 { pDecPic.iHeightInPixel >> 1 } else { pDecPic.iHeightInPixel };
                let (mut sum, mut x) = (0u32, 1u32);
                for y in 0..h {
                    for i in 0..w {
                        x = x
                            .wrapping_mul(31)
                            .wrapping_add(*sDec.pData[pl].offset((y * sDec.iLineSize[pl] + i) as isize) as u32);
                        sum = sum.wrapping_add(x);
                    }
                }
                eprintln!("REC plane={} poc={} sum={}", pl, (*pParamD).iPOC, sum);
            }
        }

        pDecPic.uiTemporalId = kuiTid;
        pDecPic.uiSpatialId = kuiDid;
        pDecPic.iFrameNum = (*pParamD).iFrameNum;
        pDecPic.iFramePoc = (*pParamD).iPOC;
        pDecPic.uiRecieveConfirmed = RECIEVE_UNKOWN;
        pDecPic.bUsedAsRef = true;

        let shortCount = (*pRefList).uiShortRefCount as usize;
        for iRefIdx in (0..shortCount).rev() {
            if iRefIdx + 1 <= MAX_SHORT_REF_COUNT {
                (*pRefList).pShortRefList[iRefIdx + 1] = (*pRefList).pShortRefList[iRefIdx];
            }
        }
        (*pRefList).pShortRefList[0] = Some(idDec);
        (*pRefList).uiShortRefCount += 1;
    }

    if keSliceType == EWelsSliceType::P_SLICE {
        if pCtx.uiTemporalId == 0 {
            if (*ctx_param(pCtx)).bEnableLongTermReference {
                LTRMarkProcess(pCtx);
                DeleteInvalidLTR(pCtx);
                HandleLTRMarkFeedback(pCtx);

                (*pLtr).bReceivedT0LostFlag = false;
                (*pLtr).bLTRMarkingFlag = false;
                (*pLtr).uiLtrMarkInterval += 1;
            }

            let mut i = ((*pRefList).uiShortRefCount as i32) - 1;
            while i > 0 {
                if let Some(id) = (*pRefList).pShortRefList[i as usize] {
                    (*pRefList).pic_mut(id).SetUnref();
                }
                DeleteSTRFromShortList(&mut *pRefList, i);
                i -= 1;
            }
            if (*pRefList).uiShortRefCount > 0 {
                let stale = match (*pRefList).pShortRefList[0] {
                    Some(id) => {
                        let p0 = (*pRefList).pic(id);
                        p0.uiTemporalId > 0 || p0.iFrameNum != (*pParamD).iFrameNum
                    }
                    None => false,
                };
                if stale {
                    let id = (*pRefList).pShortRefList[0].expect("checked just above");
                    (*pRefList).pic_mut(id).SetUnref();
                    DeleteSTRFromShortList(&mut *pRefList, 0);
                }
            }
        }
    } else {
        if (*ctx_param(pCtx)).bEnableLongTermReference {
            LTRMarkProcess(pCtx);

            (*pLtr).iCurLtrIdx = ((*pLtr).iCurLtrIdx + 1) % LONG_TERM_REF_NUM;
            (*pLtr).iLTRMarkSuccessNum = 1;
            (*pLtr).bLTRMarkEnable = true;
            (*pLtr).uiLtrMarkInterval = 0;

            if !ctx_vaa(pCtx).is_null() {
                (*ctx_vaa(pCtx)).uiValidLongTermPicIdx = 0;
                (*ctx_vaa(pCtx)).uiMarkLongTermPicIdx = 0;
            }
        }
    }

    // C++ dispatches virtually here (ref_list_mgr_svc.cpp:1041/1057/1073 —
    // PrefetchNextBuffer / UpdateSrcPicList /
    // UpdateSrcPicListLosslessScreenRefSelectionWithLtr).
    //
    // **T4b.2b, and this guard needed an argument rather than a substitution.** The
    // old `if !pReferenceStrategy.is_null()` asked whether a strategy was installed;
    // the enum has no uninstalled state, so the guard is gone. That is equivalent
    // because *this function is only reached through the strategy*:
    // `RefStrategyKind::UpdateRefList` is its one caller, so by the time the body
    // runs the kind has already been read. The guard was re-checking a thing its
    // caller had just dereferenced.
    pCtx.eRefStrategy.EndofUpdateRefList(pCtx);
    true
}

/// Checks whether candidate frame number is already occupied in LTR list.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CheckCurMarkFrameNumUsed(pCtx: &mut sWelsEncCtx) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || ctx_sps(pCtx).is_null() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return false;
    }
    // T9.G4: derived above `pLtr`, not below it — see `DeleteInvalidLTR`.
    let gopSize = (*ctx_param(pCtx)).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;
    let pLtr = &*ctx_ltr_at(pCtx, (uiDid) as usize);
    let pParamInternal = &(*ctx_param(pCtx)).sDependencyLayers[uiDid];

    for i in 0..((*pRefList).uiLongRefCount as usize) {
        if let Some(idLong) = (*pRefList).pLongRefList[i] {
            let iFrameNum = (*pRefList).pic(idLong).iFrameNum;
            let cond1 = pParamInternal.iFrameNum == iFrameNum
                && pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32;
            let cond2 = CompareFrameNum(
                pParamInternal.iFrameNum + iGoPFrameNumInterval,
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkMMCORefInfoWithBase(
    pCurDq: &mut SDqLayer,
    kBaseMarking: SRefPicMarking,
    kiCountSliceNum: i32,
) {
    // **The base arrives by value, and it is not a style preference** (T9.E2b,
    // the S29/S54 lineage). Both callers read `ppSliceList[0]`'s marking, and
    // iteration 0 of this loop writes the very bytes that marking lives in: a
    // reference parameter — `&` or `&mut` — is protected for the whole call
    // (F114b), so the write through `slice_in_layer(pCurDq, 0)` would pop it
    // mid-loop. The value cannot be invalidated by a retag, and the copy is
    // byte-identical to the C++'s `memcpy` from the live field: the first
    // store is `base = base`.
    for iSliceIdx in 0..kiCountSliceNum {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
        if !pSlice.is_null() {
            (*pSlice).sSliceHeaderExt.sSliceHeader.sRefMarking = kBaseMarking;
        }
    }
}

/// Constructs MMCO reference marking commands for slice headers.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkMMCORefInfo(
    pCtx: &mut sWelsEncCtx,
    pLtr: *mut SLTRState,
    pCurDq: &mut SDqLayer,
    kiCountSliceNum: i32,
) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pLtr.is_null() || kiCountSliceNum <= 0 {
        return;
    }
    let pBaseSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, 0);
    if pBaseSlice.is_null() {
        return;
    }
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
    let gopSize = (*ctx_param(pCtx)).uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };

    *pRefPicMark = SRefPicMarking::default();

    if (*ctx_param(pCtx)).bEnableLongTermReference && (*pLtr).bLTRMarkingFlag {
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

    WelsMarkMMCORefInfoWithBase(pCurDq, *pRefPicMark, kiCountSliceNum);
}

/// Evaluates LTR marking criteria and populates slice header MMCO commands.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkPic(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if current_layer(pCtx).is_null() || ctx_param(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // **T9.G7 — raw, not `&mut`.** This body holds the LTR state across calls that
    // derive their own `&mut` to the *same* `SLTRState` (`LTRMarkProcess` and the
    // rest re-derive `ctx_ltr_at(pCtx, uiDid)` for this same `uiDid`). Two Unique
    // tags from one raw root are siblings, and the second pops the first — so the
    // `&mut` binding was the hazard, not the holding. A raw cursor with a deref at
    // each use is the port's own idiom and is what F66 says is sound here.
    let pLtr = ctx_ltr_at(pCtx, (uiDid) as usize);
    let kiCountSliceNum = (*current_layer(pCtx)).iMaxSliceNum;

    if (*ctx_param(pCtx)).bEnableLongTermReference && (*pLtr).bLTRMarkEnable && pCtx.uiTemporalId == 0 {
        if !(*pLtr).bReceivedT0LostFlag
            && (*pLtr).uiLtrMarkInterval > (*ctx_param(pCtx)).iLtrMarkPeriod as u32
            && CheckCurMarkFrameNumUsed(pCtx)
        {
            (*pLtr).bLTRMarkingFlag = true;
            (*pLtr).bLTRMarkEnable = false;
            (*pLtr).uiLtrMarkInterval = 0;
            for i in 0..MAX_TEMPORAL_LAYER_NUM {
                if (pCtx.uiTemporalId as usize) < i || pCtx.uiTemporalId == 0 {
                    (*pLtr).iLastLtrIdx[i] = (*pLtr).iCurLtrIdx;
                }
            }
        } else {
            (*pLtr).bLTRMarkingFlag = false;
        }
    }

    // T9.G6: hoisted — the call takes the context retag and this argument reads
    // through the same context (shape B).
    let pCurLayerForMmco = &mut *current_layer(pCtx);
    WelsMarkMMCORefInfo(
        pCtx,
        pLtr,
        pCurLayerForMmco,
        kiCountSliceNum,
    );
}

/// Evaluates LTR recovery request feedback packets from decoder.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FilterLTRRecoveryRequest(
    pCtx: &mut sWelsEncCtx,
    pLTRRecoverRequest: *mut SLTRRecoverRequest,
) -> i32 {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || pLTRRecoverRequest.is_null() {
        return 0;
    }
    if !(*ctx_param(pCtx)).bEnableLongTermReference {
        for iDid in 0..((*ctx_param(pCtx)).iSpatialLayerNum as usize) {
            let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[iDid]);
            (*pParamInternal).bEncCurFrmAsIdrFlag = true;
        }
    } else {
        let pRequest = pLTRRecoverRequest;
        let iLayerId = (*pRequest).iLayerId;
        if iLayerId < 0 || iLayerId >= (*ctx_param(pCtx)).iSpatialLayerNum {
            return 0;
        }

        // T9.G4 — the derivation order, as in `DeleteInvalidLTR`: scalar, then the
        // ST-flippable accessor, then the permanently-raw fork-reachable one.
        let iMaxFrameNumPlus1 = 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;
        let pLtr = &mut *ctx_ltr_at(pCtx, (iLayerId as usize) as usize);
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[iLayerId as usize]);

        if (*pRequest).uiFeedbackType == LTR_RECOVERY_REQUEST && (*pRequest).uiIDRPicId == (*pParamInternal).uiIdrPicId as u32 {
            if (*pRequest).iLastCorrectFrameNum == -1 {
                (*pParamInternal).bEncCurFrmAsIdrFlag = true;
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FilterLTRMarkingFeedback(
    pCtx: &mut sWelsEncCtx,
    pLTRMarkingFeedback: *mut SLTRMarkingFeedback,
) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || pLTRMarkingFeedback.is_null() {
        return;
    }
    let iLayerId = (*pLTRMarkingFeedback).iLayerId;
    if iLayerId < 0 || iLayerId >= (*ctx_param(pCtx)).iSpatialLayerNum {
        return;
    }
    let pLtr = &mut *ctx_ltr_at(pCtx, (iLayerId as usize) as usize);
    if (*ctx_param(pCtx)).bEnableLongTermReference {
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[iLayerId as usize]);
        if (*pLTRMarkingFeedback).uiIDRPicId == (*pParamInternal).uiIdrPicId as u32
            && ((*pLTRMarkingFeedback).uiFeedbackType == LTR_MARKING_SUCCESS
                || (*pLTRMarkingFeedback).uiFeedbackType == LTR_MARKING_FAILED)
        {
            pLtr.uiLtrMarkState = (*pLTRMarkingFeedback).uiFeedbackType;
            pLtr.iLtrMarkFbFrameNum = (*pLTRMarkingFeedback).iLTRFrameNum;
        }
    }
}

/// Builds active reference picture list pRefList0 for motion estimation.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsBuildRefList(
    pCtx: &mut sWelsEncCtx,
    kiPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || current_layer(pCtx).is_null() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() {
        return false;
    }
    let pLtr = &mut *ctx_ltr_at(pCtx, (uiDid) as usize);
    let kiNumRef = (*ctx_param(pCtx)).iNumRefFrame;
    let kuiTid = pCtx.uiTemporalId;
    let pParamD = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);

    pCtx.iNumRef0 = 0;
    if pCtx.eSliceType != EWelsSliceType::I_SLICE {
        if (*ctx_param(pCtx)).bEnableLongTermReference && pLtr.bReceivedT0LostFlag && pCtx.uiTemporalId == 0 {
            for i in 0..((*pRefList).uiLongRefCount as usize) {
                let Some(idLong) = (*pRefList).pLongRefList[i] else {
                    continue;
                };
                if (*pRefList).pic(idLong).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    let numRef0 = pCtx.iNumRef0 as usize;
                    // The camera path puts a *reconstruction* picture in `pRefOri`
                    // where the screen path puts a source picture — see `PicRef`.
                    (*current_layer(pCtx)).pRefOri[numRef0] = Some(PicRef::Rec(idLong));
                    pCtx.pRefList0[numRef0] = Some(idLong);
                    pCtx.iNumRef0 += 1;
                    pLtr.iLastRecoverFrameNum = (*pParamD).iFrameNum;
                    break;
                }
            }
        } else {
            for i in 0..((*pRefList).uiShortRefCount as usize) {
                let Some(idRef) = (*pRefList).pShortRefList[i] else {
                    continue;
                };
                let pRef = (*pRefList).pic(idRef);
                if pRef.bUsedAsRef && pRef.iFramePoc >= 0 && pRef.uiTemporalId <= kuiTid {
                    let numRef0 = pCtx.iNumRef0 as usize;
                    (*current_layer(pCtx)).pRefOri[numRef0] = Some(PicRef::Rec(idRef));
                    pCtx.pRefList0[numRef0] = Some(idRef);
                    pCtx.iNumRef0 += 1;
                }
            }
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(&mut *ctx_ltr_at(pCtx, (uiDid) as usize));
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateBlockStatic(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_vaa(pCtx).is_null() || pCtx.pVpp.is_null() {
        return;
    }
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
        let pVaaExt = ctx_vaa(pCtx) as *mut SVAAFrameInfoExt;
    let pRefList = ctx_ref_list(pCtx, pCtx.uiDependencyId as usize);
    for idx in 0..(pCtx.iNumRef0 as usize) {
        let Some(idRef) = pCtx.pRefList0[idx] else {
            continue;
        };
        // Two pools again: the reference is a reconstruction picture, the current one
        // a spatial source picture. Both are copied to geometry before the call.
        let sRef = (*pRefList).pic_mut(idRef).planes();
        if (*pVaaExt).iVaaBestRefFrameNum != (*pRefList).pic(idRef).iFrameNum {
            let sSrc = (*pCtx)
                .pEncPic
                .map(|id| (*pCtx.pVpp).m_pSpatialPicPool.get_mut(id).planes());
            (*pCtx.pVpp).UpdateBlockIdcForScreen(
                (*pVaaExt).pVaaBestBlockStaticIdc,
                Some(&sRef),
                sSrc.as_ref(),
            );
        }
    }
}

/// Serializes slice header reference picture reordering syntax and marking flags.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUpdateSliceHeaderSyntax(
    pCtx: &mut sWelsEncCtx,
    iAbsDiffPicNumMinus1: i32,
    pCurDq: &mut SDqLayer,
    uiFrameType: i32,
) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    // T9.E2i (the close's Miri verdict, F114b's shape): the count is read
    // through the parameter — `(*current_layer(pCtx)).iMaxSliceNum` was a
    // second, independent path to the same object this function's `&mut`
    // already protects, and the read popped the protector.
    let kiCountSliceNum = pCurDq.iMaxSliceNum;
    let uiDid = pCtx.uiDependencyId as usize;
    let pLtr = &*ctx_ltr_at(pCtx, (uiDid) as usize);

    for iIdx in 0..kiCountSliceNum {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iIdx);
        if pSlice.is_null() {
            continue;
        }
        let pSliceHdr = &mut (*pSlice).sSliceHeaderExt.sSliceHeader;
        let pRefReorder = &mut pSliceHdr.sRefReordering;
        let pRefPicMark = &mut pSliceHdr.sRefMarking;

        pSliceHdr.uiRefCount = pCtx.iNumRef0 as u8;
        if pCtx.iNumRef0 > 0 {
            let pRefList = ctx_ref_list(pCtx, pCtx.uiDependencyId as usize);
            let isLongRef = match pCtx.pRefList0[0] {
                Some(id) => (*pRefList).pic(id).bIsLongRef,
                None => false,
            };
            if !isLongRef || !(*ctx_param(pCtx)).bEnableLongTermReference {
                pRefReorder.SReorderingSyntax[0].uiReorderingOfPicNumsIdc = 0;
                pRefReorder.SReorderingSyntax[0].uiAbsDiffPicNumMinus1 = iAbsDiffPicNumMinus1 as u32;
                pRefReorder.SReorderingSyntax[1].uiReorderingOfPicNumsIdc = 3;
            } else {
                let mut iRefIdx = 0usize;
                while (iRefIdx as i32) < pCtx.iNumRef0 as i32 {
                    if iRefIdx < MAX_REFERENCE_REORDER_COUNT_NUM {
                        pRefReorder.SReorderingSyntax[iRefIdx].uiReorderingOfPicNumsIdc = 2;
                        if let Some(id) = pCtx.pRefList0[iRefIdx] {
                            pRefReorder.SReorderingSyntax[iRefIdx].iLongTermPicNum =
                                (*pRefList).pic(id).iLongTermPicNum as u16;
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
            pRefPicMark.bLongTermRefFlag = (*ctx_param(pCtx)).bEnableLongTermReference;
        } else {
            // SCREEN_CONTENT(dormant: Phase 10)
            if (*ctx_param(pCtx)).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = (*ctx_param(pCtx)).bEnableLongTermReference;
            } else {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = (*ctx_param(pCtx)).bEnableLongTermReference && pLtr.bLTRMarkingFlag;
            }
        }
    }
}

/// Updates reference picture syntax and picture number delta in slice headers.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUpdateRefSyntax(pCtx: &mut sWelsEncCtx, kiPOC: i32, kiFrameType: i32) {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if ctx_param(pCtx).is_null() || current_layer(pCtx).is_null() {
        return;
    }
    let mut iAbsDiffPicNumMinus1 = -1i32;
    let uiDid = pCtx.uiDependencyId as usize;
    let pParamD = &(*ctx_param(pCtx)).sDependencyLayers[uiDid];

    if pCtx.iNumRef0 > 0 {
        let pRefList = ctx_ref_list(pCtx, uiDid);
        if let Some(id) = pCtx.pRefList0[0] {
            iAbsDiffPicNumMinus1 = pParamD.iFrameNum - (*pRefList).pic(id).iFrameNum - 1;
            if iAbsDiffPicNumMinus1 < 0 && !ctx_sps(pCtx).is_null() {
                iAbsDiffPicNumMinus1 += 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;
            }
        }
    }

    if !current_layer(pCtx).is_null() {
        // The null arm of the callee's old guard, hoisted with the reborrow.
        // T9.G6: hoisted — see `WelsMarkMMCORefInfo` above.
        let pCurLayerForSh = &mut *current_layer(pCtx);
        WelsUpdateSliceHeaderSyntax(
            pCtx,
            iAbsDiffPicNumMinus1,
            pCurLayerForSh,
            kiFrameType,
        );
    }
}

/// Synchronizes reconstructed picture metadata back to the source input picture.
/// **The one place the two pools meet by name** — `pOrigPic` is a spatial source
/// picture, `pReconPic` a reconstruction picture. Two owners, so two references can
/// be live at once, and the two handle types are what keep the arguments the right
/// way round (session B's settlement, and the reason there are two).
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
unsafe fn UpdateOriginalPicInfoFromCtx(pCtx: &mut sWelsEncCtx) {
    let (Some(idEnc), Some(idDec)) = (pCtx.pEncPic, pCtx.pDecPic) else {
        return;
    };
    let pRefList = ctx_ref_list(pCtx, pCtx.uiDependencyId as usize);
    if pRefList.is_null() || pCtx.pVpp.is_null() {
        return;
    }
    // Two owners, two raw parents, so both references are live at once without either
    // borrow overlapping the other.
    let pRecon: &SPicture = (*pRefList).pic(idDec);
    let pOrig: &mut SPicture = (*pCtx.pVpp).m_pSpatialPicPool.get_mut(idEnc);
    UpdateOriginalPicInfo(pOrig, pRecon);
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateSrcPicListLosslessScreenRefSelectionWithLtr(pCtx: &mut sWelsEncCtx) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if !pCtx.pVpp.is_null() && !ctx_vaa(pCtx).is_null() {
        let pRefList = &*(ctx_ref_list(pCtx, iDIdx as usize));
        // wels_preprocess.h:143 takes const int32_t; the uint8_t field promotes.
        (*pCtx.pVpp).UpdateSrcListLosslessScreenRefSelectionWithLtr(
            pCtx.pEncPic,
            iDIdx,
            (*ctx_vaa(pCtx)).uiMarkLongTermPicIdx as i32,
            pRefList,
        );
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateSrcPicList(pCtx: &mut sWelsEncCtx) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if !pCtx.pVpp.is_null() {
        let pRefList = ctx_ref_list(pCtx, (iDIdx as usize) as usize);
        let shortCount = (*pRefList).uiShortRefCount;
        (*pCtx.pVpp).UpdateSrcList(pCtx.pEncPic, iDIdx, shortCount as u32);
    }
}

/// Screen content specialized reference picture list update.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUpdateRefListScreen(pCtx: &mut sWelsEncCtx) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if current_layer(pCtx).is_null() || ctx_param(pCtx).is_null() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    if pRefList.is_null() || (*pRefList).pRef.is_empty() {
        return false;
    }
    // **T9.G7 — raw, not `&mut`.** This body holds the LTR state across calls that
    // derive their own `&mut` to the *same* `SLTRState` (`LTRMarkProcess` and the
    // rest re-derive `ctx_ltr_at(pCtx, uiDid)` for this same `uiDid`). Two Unique
    // tags from one raw root are siblings, and the second pops the first — so the
    // `&mut` binding was the hazard, not the holding. A raw cursor with a deref at
    // each use is the port's own idiom and is what F66 says is sound here.
    let pLtr = ctx_ltr_at(pCtx, (uiDid) as usize);
    let pParamD = std::ptr::addr_of_mut!((*ctx_param(pCtx)).sDependencyLayers[uiDid]);
    let kuiTid = pCtx.uiTemporalId;

    if let Some(idDec) = pCtx.pDecPic {
        // The reconstruction picture, resolved once — S37: everything below either
        // reads its geometry or writes its own fields, and the borrow ends before the
        // reference-list shifts that would touch the pool again.
        let pDecPic: &mut SPicture = (*pRefList).pic_mut(idDec);
        let sDec = pDecPic.planes();
        if (*pParamD).iHighestTemporalId == 0 || (kuiTid as i32) < (*pParamD).iHighestTemporalId as i32 {
            // T4b.3b: as above — `ref_list_mgr_svc.cpp:779`, the second of the
            // encoder's two identical expand sites.
            // T6.F4: the picture owns its planes, so the expansion is a method on it
            // and `ExpandReferencingPicture`'s three raw origins are gone from the
            // encoder entirely.
            pDecPic.expand_as_reference();
        }

        pDecPic.uiTemporalId = pCtx.uiTemporalId;
        pDecPic.uiSpatialId = pCtx.uiDependencyId;
        pDecPic.iFrameNum = (*pParamD).iFrameNum;
        pDecPic.iFramePoc = (*pParamD).iPOC;
        pDecPic.bUsedAsRef = true;
        pDecPic.bIsLongRef = true;
        pDecPic.bIsSceneLTR = (*pLtr).bLTRMarkingFlag
            || ((*ctx_param(pCtx)).bEnableLongTermReference
                && pCtx.eSliceType == EWelsSliceType::I_SLICE);
        pDecPic.iLongTermPicNum = (*pLtr).iCurLtrIdx;
    }

    if pCtx.eSliceType == EWelsSliceType::P_SLICE {
        DeleteNonSceneLTR(pCtx);
        LTRMarkProcessScreen(pCtx);
        (*pLtr).bLTRMarkingFlag = false;
        (*pLtr).uiLtrMarkInterval += 1;
    } else {
        LTRMarkProcessScreen(pCtx);
        (*pLtr).iCurLtrIdx = 1;
        (*pLtr).iSceneLtrIdx = 1;
        (*pLtr).uiLtrMarkInterval = 0;
        if !ctx_vaa(pCtx).is_null() {
            (*ctx_vaa(pCtx)).uiValidLongTermPicIdx = 0;
        }
    }

    // Same dispatch and the same guard argument as `WelsUpdateRefList` above: this
    // body is reached only through `RefStrategyKind::UpdateRefList`.
    pCtx.eRefStrategy.EndofUpdateRefList(pCtx);
    true
}

/// Screen content specialized reference picture list builder.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsBuildRefListScreen(
    pCtx: &mut sWelsEncCtx,
    iPOC: i32,
    iBestLtrRefIdx: i32,
) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || ctx_vaa(pCtx).is_null() || current_layer(pCtx).is_null() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    let pParam = ctx_param(pCtx);
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
        let pVaaExt = ctx_vaa(pCtx) as *mut SVAAFrameInfoExt;
    let iNumRef = (*pParam).iNumRefFrame;
    let pParamD = &(*pParam).sDependencyLayers[uiDid];
    pCtx.iNumRef0 = 0;

    if pCtx.eSliceType != EWelsSliceType::I_SLICE {
        let mut iLtrRefIdx = 0i32;
        // The screen path's `pRefOri` is a **spatial source** picture, where the
        // camera path's is a reconstruction picture — see [`PicRef`].
        let mut pRefOri: Option<SrcPicId> = None;

        for idx in 0..(*pVaaExt).iNumOfAvailableRef {
            if !pCtx.pVpp.is_null() {
                iLtrRefIdx = (*pCtx.pVpp).GetRefFrameInfo(
                    idx,
                    pCtx.bCurFrameMarkedAsSceneLtr,
                    &mut pRefOri,
                );
            }
            let refOri = pRefOri.map(PicRef::Src);
            if iLtrRefIdx >= 0 && iLtrRefIdx <= (*pParam).iLTRRefNum {
                let Some(idRefPic) = (*pRefList).pLongRefList[iLtrRefIdx as usize] else {
                    continue;
                };
                let pRefPic = (*pRefList).pic(idRefPic);
                if pRefPic.bUsedAsRef
                    && pRefPic.bIsLongRef
                    && pRefPic.uiTemporalId <= pCtx.uiTemporalId
                    && (!pCtx.bCurFrameMarkedAsSceneLtr || pRefPic.bIsSceneLTR)
                {
                    let num0 = pCtx.iNumRef0 as usize;
                    (*current_layer(pCtx)).pRefOri[num0] = refOri;
                    pCtx.pRefList0[num0] = Some(idRefPic);
                    pCtx.iNumRef0 += 1;
                }
            } else {
                let mut i = iNumRef;
                while i >= 0 {
                    let Some(idLong) = (*pRefList).pLongRefList[i as usize] else {
                        i -= 1;
                        continue;
                    };
                    let uiTemporalId = (*pRefList).pic(idLong).uiTemporalId;
                    if uiTemporalId == 0 || uiTemporalId < pCtx.uiTemporalId {
                        let num0 = pCtx.iNumRef0 as usize;
                        (*current_layer(pCtx)).pRefOri[num0] = refOri;
                        pCtx.pRefList0[num0] = Some(idLong);
                        pCtx.iNumRef0 += 1;
                        break;
                    }
                    i -= 1;
                }
            }
        }
    } else {
        WelsResetRefList(pCtx);
        ResetLtrState(&mut *ctx_ltr_at(pCtx, (uiDid) as usize));
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkMMCORefInfoScreen(
    pCtx: &mut sWelsEncCtx,
    pLtr: *mut SLTRState,
    pCurDq: &mut SDqLayer,
    kiCountSliceNum: i32,
) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pLtr.is_null() || kiCountSliceNum <= 0 {
        return;
    }
    let pBaseSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, 0);
    if pBaseSlice.is_null() {
        return;
    }
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
    let iMaxLtrIdx = (*ctx_param(pCtx)).iNumRefFrame - STR_ROOM - 1;

    *pRefPicMark = SRefPicMarking::default();
    if (*ctx_param(pCtx)).bEnableLongTermReference {
        let count0 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count0].iMaxLongTermFrameIdx = iMaxLtrIdx;
        pRefPicMark.SMmcoRef[count0].iMmcoType = MMCO_SET_MAX_LONG;
        pRefPicMark.uiMmcoCount += 1;

        let count1 = pRefPicMark.uiMmcoCount as usize;
        pRefPicMark.SMmcoRef[count1].iLongTermFrameIdx = (*pLtr).iCurLtrIdx;
        pRefPicMark.SMmcoRef[count1].iMmcoType = MMCO_LONG;
        pRefPicMark.uiMmcoCount += 1;
    }

    WelsMarkMMCORefInfoWithBase(pCurDq, *pRefPicMark, kiCountSliceNum);
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkPicScreen(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if ctx_param(pCtx).is_null() || current_layer(pCtx).is_null() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // **T9.G7 — raw, not `&mut`.** This body holds the LTR state across calls that
    // derive their own `&mut` to the *same* `SLTRState` (`LTRMarkProcess` and the
    // rest re-derive `ctx_ltr_at(pCtx, uiDid)` for this same `uiDid`). Two Unique
    // tags from one raw root are siblings, and the second pops the first — so the
    // `&mut` binding was the hazard, not the holding. A raw cursor with a deref at
    // each use is the port's own idiom and is what F66 says is sound here.
    let pLtr = ctx_ltr_at(pCtx, (uiDid) as usize);
    let gopSize = (*ctx_param(pCtx)).uiGopSize;
    let iMaxTid = if gopSize > 0 { (31 - gopSize.leading_zeros()) as i32 } else { 0 };
    let mut iMaxActualLtrIdx = -1i32;
    let pParamD = &(*ctx_param(pCtx)).sDependencyLayers[uiDid];

    if (*ctx_param(pCtx)).bEnableLongTermReference {
        let maxTidAdj = if iMaxTid > 1 { iMaxTid } else { 1 };
        iMaxActualLtrIdx = (*ctx_param(pCtx)).iNumRefFrame - STR_ROOM - 1 - maxTidAdj;
    }

    let pRefList = ctx_ref_list(pCtx, (uiDid) as usize);
    let iNumRef = (*ctx_param(pCtx)).iNumRefFrame;
    let iLongRefNum = iNumRef - STR_ROOM;
    let bIsRefListNotFull = ((*pRefList).uiLongRefCount as i32) < iLongRefNum;

    if !(*ctx_param(pCtx)).bEnableLongTermReference {
        (*pLtr).iCurLtrIdx = pCtx.uiTemporalId as i32;
    } else {
        if iMaxActualLtrIdx != -1 && pCtx.uiTemporalId == 0 && pCtx.bCurFrameMarkedAsSceneLtr {
            (*pLtr).bLTRMarkingFlag = true;
            (*pLtr).uiLtrMarkInterval = 0;
            (*pLtr).iCurLtrIdx = (*pLtr).iSceneLtrIdx % (iMaxActualLtrIdx + 1);
            (*pLtr).iSceneLtrIdx += 1;
        } else {
            (*pLtr).bLTRMarkingFlag = false;
            if bIsRefListNotFull {
                for i in 0..iLongRefNum {
                    if (*pRefList).pLongRefList[i as usize].is_none() {
                        (*pLtr).iCurLtrIdx = i;
                        break;
                    }
                }
            } else {
                let mut iRefNum_t = [0i32; MAX_TEMPORAL_LAYER_NUM];
                for i in 0..((*pRefList).uiLongRefCount as usize) {
                    let Some(idPic) = (*pRefList).pLongRefList[i] else {
                        continue;
                    };
                    let pPic = (*pRefList).pic(idPic);
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
                let iMaxFrameNum = 1 << (*ctx_sps(pCtx)).uiLog2MaxFrameNum;

                for i in 0..((*pRefList).uiLongRefCount as usize) {
                    let Some(idPic) = (*pRefList).pLongRefList[i] else {
                        continue;
                    };
                    let pPic = (*pRefList).pic(idPic);
                    if pPic.bUsedAsRef
                        && pPic.bIsLongRef
                        && !pPic.bIsSceneLTR
                        && iMaxMultiRefTid == pPic.uiTemporalId as i32
                    {
                        if !IsValidFrameNum(pPic.iFrameNum) {
                            return;
                        }
                        let iDeltaFrameNum = if pParamD.iFrameNum >= pPic.iFrameNum {
                            pParamD.iFrameNum - pPic.iFrameNum
                        } else {
                            pParamD.iFrameNum + iMaxFrameNum - pPic.iFrameNum
                        };

                        if iDeltaFrameNum > iLongestDeltaFrameNum {
                            (*pLtr).iCurLtrIdx = pPic.iLongTermPicNum;
                            iLongestDeltaFrameNum = iDeltaFrameNum;
                        }
                    }
                }
            }
        }
    }

    for i in 0..MAX_TEMPORAL_LAYER_NUM {
        if (pCtx.uiTemporalId as usize) < i || pCtx.uiTemporalId == 0 {
            (*pLtr).iLastLtrIdx[i] = (*pLtr).iCurLtrIdx;
        }
    }

    let iSliceNum = (*current_layer(pCtx)).iMaxSliceNum;
    // T9.G6: hoisted — see `WelsMarkMMCORefInfo`.
    let pCurLayerForMmco = &mut *current_layer(pCtx);
    WelsMarkMMCORefInfoScreen(
        pCtx,
        pLtr,
        pCurLayerForMmco,
        iSliceNum,
    );
}

/// Intentional no-op reference list manager callback.
/// Matches `void DoNothing (sWelsEncCtx* pointer)` in `ref_list_mgr_svc.cpp:996`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DoNothing(_pCtx: *mut sWelsEncCtx) {}

// ============================================================================
// Reference strategy — T4b.2b
// ============================================================================

/// Which reference-list strategy an encoder runs.
///
/// C++ declares three classes deriving from `IWelsReferenceStrategy`
/// (`ref_list_mgr_svc.h`): `CWelsReference_TemporalLayer`, `CWelsReference_Screen`
/// and `CWelsReference_LosslessWithLtr`. **None of them declares a data member**
/// beyond the `m_pEncoderCtx` back-pointer the first one introduces, so the port
/// already had them as one object struct with three static vtables. What actually
/// varied was the vtable, and this enum is that vtable — three variants, seven
/// methods, five of which `match`.
///
/// **The back-pointer is gone with the object.** Every call site already holds `pCtx`
/// and used to reach the strategy *through* it, only for the strategy to hand `pCtx`
/// straight back from `m_pEncoderCtx`. The methods take it as a parameter instead;
/// `Init` existed only to store it and `Destroy` only to free the box, so both die
/// rather than convert. This is T8's stored-context-back-pointer shape, deleted.
///
/// `TemporalLayer = 0` and `#[derive(Default)]` on it: `sWelsEncCtx` is
/// `mem::zeroed()`-constructed (`encoder_context.rs:514`), so the all-zero pattern has
/// to be a declared variant. It is, and it is also the variant
/// `CreateReferenceStrategy`'s `_ =>` arm picked (S21).
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
    /// `IWelsReferenceStrategy::CreateReferenceStrategy` — the whole factory, which
    /// was only ever a two-level `match` picking one of three vtables.
    ///
    /// **S23**: the selectors are `iUsageType` and `bEnableLongTermReference`, and
    /// neither can change behind this choice. `WelsEncoderParamAdjust` *rejects* a
    /// changed `iUsageType` outright (`wels_encoder_ext.rs:697`,
    /// `ENC_RETURN_UNSUPPORTED_PARA`), and `bEnableLongTermReference` is one of the
    /// fields in its `bNeedReset` predicate, so changing it forces the full
    /// uninit/init that re-runs this selection. There is no lag to preserve and the
    /// field needs no `eInstalled…` name — unlike `SWelsRcFunc`'s `eInstalledMode`,
    /// which is the same question answered the other way (T4b.1b).
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
    ///
    /// # Safety
    /// `pCtx` must be a live encoder context.
    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn BuildRefList(self, pCtx: &mut sWelsEncCtx, iPOC: i32, iBestLtrRefIdx: i32) -> bool {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => {
                WelsBuildRefList(pCtx, iPOC, iBestLtrRefIdx)
            }
            RefStrategyKind::LosslessWithLtr => WelsBuildRefListScreen(pCtx, iPOC, iBestLtrRefIdx),
        }
    }

    /// `MarkPic` — `ref_list_mgr_svc.cpp`'s `WelsMarkPic` / `WelsMarkPicScreen`.
    ///
    /// # Safety
    /// `pCtx` must be a live encoder context.
    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn MarkPic(self, pCtx: &mut sWelsEncCtx) {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => WelsMarkPic(pCtx),
            RefStrategyKind::LosslessWithLtr => WelsMarkPicScreen(pCtx),
        }
    }

    /// `UpdateRefList`.
    ///
    /// # Safety
    /// `pCtx` must be a live encoder context.
    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn UpdateRefList(self, pCtx: &mut sWelsEncCtx) -> bool {
        match self {
            RefStrategyKind::TemporalLayer | RefStrategyKind::Screen => WelsUpdateRefList(pCtx),
            RefStrategyKind::LosslessWithLtr => WelsUpdateRefListScreen(pCtx),
        }
    }

    /// `EndofUpdateRefList` — `ref_list_mgr_svc.cpp:1041` / `:1057` / `:1073`. The one
    /// method where all three variants differ.
    ///
    /// # Safety
    /// `pCtx` must be a live encoder context.
    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn EndofUpdateRefList(self, pCtx: &mut sWelsEncCtx) {
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
    ///
    /// # Safety
    /// `pCtx` must be a live encoder context.
    #[inline]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub unsafe fn AfterBuildRefList(self, pCtx: &mut sWelsEncCtx) {
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
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_ref_list_mgr_noop_callback() {
        unsafe {
            DoNothing(std::ptr::null_mut());
        }
    }

    /// The whole of `CreateReferenceStrategy`, which used to be a two-level `match`
    /// picking one of three static vtables. LTR only discriminates for screen
    /// content — a camera-content encoder gets `TemporalLayer` either way, which is
    /// the arm the old factory's `_ =>` covered.
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
    /// be the variant the old factory's `_ =>` arm produced. S21, as an assertion
    /// rather than a sentence.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
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
