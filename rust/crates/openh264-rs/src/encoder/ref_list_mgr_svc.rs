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
use crate::encoder::encoder_context::{ctx_ltr_at, ctx_param_raw};
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
///
/// **T9.X — `&mut`, and safe.** The parameter was `*mut`-SLTRState with a null
/// guard; all three callers derive it from `ctx_ltr_at`, which — since T9.H3
/// returns the real `&mut SLTRState` — hands them the argument directly. The
/// body only writes fields, so nothing here needs `unsafe`.
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
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let uiDid = pCtx.uiDependencyId as usize;
    // §4.6, reorder: every raw root and scalar first, the reference-shaped
    // borrow last — the order this file's own T9.H3 note asks for.
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
        (*pRefList).pShortRefList[i] = None;
    }
    for i in 0..=(ltrRefNum) {
        if i <= MAX_REF_PIC_COUNT {
            (*pRefList).pLongRefList[i] = None;
        }
    }
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
/// **Narrowed to the list it edits — T9.G3, S54.** It took `*mut`-sWelsEncCtx and
/// used it for exactly one thing: `pCtx.ref_list_mut((*pCtx).uiDependencyId)`.
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
    // **T9.X — F86's class, guarded where upstream's invariant is implicit.**
    // Upstream (`ref_list_mgr_svc.cpp:82`) walks to `uiLongRefCount - 1` and indexes
    // `pLongRefList[k + 1]` unchecked; the array is `[_; 1 + MAX_REF_PIC_COUNT]` and
    // nothing in either tree *enforces* `uiLongRefCount <= 1 + MAX_REF_PIC_COUNT` —
    // it is an emergent property of the marking schedule, not a checked bound. Where
    // that invariant holds, `kLast` is never the binding term and this loop is
    // byte-for-byte the C++ one. Where it fails, upstream reads past the array and
    // this stops at its end instead of panicking. See F172.
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
///
/// Narrowed with [`DeleteLTRFromLongList`] and for the same reason — T9.G3.
pub fn DeleteSTRFromShortList(pRefList: &mut SRefList, iIdx: i32) {
    let count = pRefList.uiShortRefCount as i32;
    // The same guard as [`DeleteLTRFromLongList`], for the same reason and against
    // the same C++ shape (`ref_list_mgr_svc.cpp:93`) — T9.X, F172.
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
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() {
        return;
    }
    // §4.6, reorder: every raw root and scalar first, the reference-shaped
    // borrow last — the order this file's own T9.H3 note asks for.
    let numRef = pCtx.param().iNumRefFrame;
    let uiTemporalId = pCtx.uiTemporalId;
    let bCurFrameMarkedAsSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
    let Some(pRefList) = pCtx.ref_list_mut(pCtx.uiDependencyId as usize) else {
        return;
    };
    let mut i = 0;
    while i < numRef {
        let hit = match (*pRefList).pLongRefList[i as usize] {
            Some(id) => {
                let pRef = (*pRefList).pic(id);
                pRef.bUsedAsRef
                    && pRef.bIsLongRef
                    && (!pRef.bIsSceneLTR)
                    && (uiTemporalId < pRef.uiTemporalId
                        || bCurFrameMarkedAsSceneLtr)
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
pub fn DeleteInvalidLTR(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return;
    };
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // **S10.5a — T9.H3's ordering rule is gone, and its going is the conversion.**
    // That comment described the order this body had to keep so a raw
    // `addr_of_mut!` cursor would survive the later whole-context reborrow: "every
    // raw root first, the `&mut`-shaped LTR borrow last". It was a hand-maintained
    // rule standing in for a guarantee, and F239 is what happens when such a rule
    // is broken silently. `ltr_family_mut` projects the parameter slot, the list
    // and the LTR state from **one** borrow, so there is no order to keep and the
    // compiler enforces the disjointness the comment used to assert.
    let iMaxFrameNumPlus1 = 1 << sps.uiLog2MaxFrameNum;
    let ltr_family = pCtx.ltr_family_mut(uiDid);
    let pParamInternal = ltr_family.param_layer;
    let pLtr = ltr_family.ltr;
    let Some(pRefList) = ltr_family.ref_list else {
        return;
    };

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
pub fn HandleLTRMarkFeedback(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // S10.5a: one borrow for all four — the parameter slot, the VAA the loop
    // stamps, the list and the LTR state. See `DeleteInvalidLTR` for why T9.H3's
    // ordering rule went with the raw root it protected.
    let ltr_family = pCtx.ltr_family_mut(uiDid);
    let pParamInternal = ltr_family.param_layer;
    let mut pVaa = ltr_family.vaa;
    let pLtr = ltr_family.ltr;
    let Some(pRefList) = ltr_family.ref_list else {
        return;
    };

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
                if let Some(pVaa) = pVaa.as_mut() {
                    pVaa.uiValidLongTermPicIdx =
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
pub fn LTRMarkProcess(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() {
        return;
    }
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    // S10.5a: the scalars are still read out first, because they are *reads* of
    // fields this body does not otherwise borrow — that part of T9.H3 was never
    // about raw roots. What is gone is the ordering rule protecting a raw cursor
    // across a reborrow; `ltr_family_mut` projects all five fields at once.
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
        if let Some(id) = (*pRefList).pShortRefList[i] {
            (*pRefList).pic_mut(id).uiRecieveConfirmed = RECIEVE_SUCCESS;
        }
    } else if pLtr.bLTRMarkingFlag {
        if let Some(pVaa) = pVaa.as_mut() {
            pVaa.uiMarkLongTermPicIdx = pLtr.iCurLtrIdx as u8;
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

    if keSliceType == EWelsSliceType::I_SLICE || pLtr.bLTRMarkingFlag {
        if let Some(id) = (*pRefList).pShortRefList[i] {
            let iFrameNum = (*pParamInternal).iFrameNum;
            let pShort = (*pRefList).pic_mut(id);
            pShort.bIsLongRef = true;
            pShort.iLongTermPicNum = pLtr.iCurLtrIdx;
            pShort.iMarkFrameNum = iFrameNum;
        }
    }

    if pLtr.iLTRMarkMode == LTR_MARKING_PROCESS_MODE::LTR_DIRECT_MARK as i32
        && keSliceType != EWelsSliceType::I_SLICE
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
        // The flag lives in a different field of the context from the two
        // borrows above; its root is derived before them (F71's spelling) so the
        // write does not need a second claim on `pCtx`.
        let tid = uiTemporalId;
        if uiDid < MAX_DEPENDENCY_LAYER && tid < MAX_TEMPORAL_LEVEL {
            (*bRefOfCurTidIsLtr)[uiDid][tid] = true;
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
        if (*pRefList).uiLongRefCount as i32 > iLTRRefNum {
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
pub fn LTRMarkProcessScreen(pCtx: &mut sWelsEncCtx) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let Some(idDec) = pCtx.pDecPic else {
        return;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    // §4.6, reorder: the index is read out, the VAA stamp happens through its own
    // raw, and the list is re-borrowed after it — the accessor is a field
    // projection, so re-deriving costs nothing and holds nothing.
    let Some(iLtrIdx) = pCtx.ref_list(uiDid).map(|l| l.pic(idDec).iLongTermPicNum) else {
        return;
    };
    if pCtx.vaa().is_some() {
        pCtx.vaa_mut().expect("the frame's video-analysis block").uiMarkLongTermPicIdx = iLtrIdx as u8;
    }

    let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
        return;
    };
    if iLtrIdx >= 0 && (iLtrIdx as usize) < MAX_REF_PIC_COUNT {
        match (*pRefList).pLongRefList[iLtrIdx as usize] {
            Some(id) => (*pRefList).pic_mut(id).SetUnref(),
            None => (*pRefList).uiLongRefCount += 1,
        }
        (*pRefList).pLongRefList[iLtrIdx as usize] = Some(idDec);
    }
}

/// Pre-allocates destination frame buffer pointer pDecPic for upcoming reconstruction.
pub fn PrefetchNextBuffer(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // §4.6, reorder: the parameter read goes above the reference-shaped borrow.
    let kiNumRef = pCtx.param().iNumRefFrame;
    let Some(pRefList) = pCtx.ref_list_mut((uiDid) as usize) else {
        return;
    };

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
        // **T9.X — this is F86's actual open site**, not the shift the finding names.
        // The abort F86 recorded (`the len is 5 but the index is 5`, then
        // `panic in a function that cannot unwind -> abort`) was raised *here*, at
        // what was `ref_list_mgr_svc.rs:684` when the finding was written: with
        // `uiShortRefCount == 6`, `lastIdx == 5` on a `[_; 1 + MAX_SHORT_REF_COUNT]`
        // = `[_; 5]`. The C++ counterpart is `ref_list_mgr_svc.cpp:343`
        // (`pShortRefList[pRefList->uiShortRefCount - 1]`), an unchecked *read* — not
        // the `pShortRefList[iRefIdx + 1]` write at `cpp:387-391` the finding cites,
        // which is `WelsUpdateRefList`'s insert shift and has been bound-guarded in
        // this port since the raw translation. See F172.
        //
        // Measured this session: across all 583 sweep rows (`st mt def sl ltr ps dl
        // bg`) `uiShortRefCount` never exceeds 2 against a panic threshold of 6, and
        // this branch is never taken at all — the free-buffer loop above always finds
        // one. F86's own trigger was `ForceCodingIDR` being a stub, which T8b.A4
        // fixed. So the guard below is not covering a reachable panic; it makes the
        // unenforced invariant explicit at the one site that historically broke it.
        let lastIdx = (((*pRefList).uiShortRefCount - 1) as usize)
            .min((*pRefList).pShortRefList.len() - 1);
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
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).map_or(true, |l| l.pRef.is_empty()) {
        return false;
    }

    // **T9.H3 — the held cursor is gone.** T9.G7's note stood here: this body held
    // the LTR state raw across `LTRMarkProcess` / `DeleteInvalidLTR` /
    // `HandleLTRMarkFeedback`, each of which re-derives its own `&mut` to the same
    // `SLTRState`. Nothing is held across those calls any more — each branch
    // re-borrows `ctx_ltr_at` *after* its calls return, so "the writes see the
    // state those calls left" is now said in borrows instead of a comment.
    let pParamD = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[uiDid]);
    let kuiTid = pCtx.uiTemporalId;
    let kuiDid = pCtx.uiDependencyId;
    let keSliceType = pCtx.eSliceType;

    // **A3 — the list borrow is scoped, for the same reason the LTR one was.**
    // The tail calls `LTRMarkProcess` / `DeleteInvalidLTR` /
    // `HandleLTRMarkFeedback`, each of which takes the whole context, and a live
    // `&mut SRefList` cannot span them. So the list is bound inside the block that
    // uses it and re-derived after the calls — the same sentence T9.H3 wrote about
    // the LTR state, one field over. Re-deriving is a field projection and an
    // index; it holds nothing.
    if let Some(idDec) = pCtx.pDecPic {
        let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
            return false;
        };
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

            // Re-derived after the three calls above, exactly as the LTR borrow is.
            let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
                return false;
            };
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
        if pCtx.param().bEnableLongTermReference {
            LTRMarkProcess(pCtx);

            let pLtr = ctx_ltr_at(pCtx, uiDid);
            pLtr.iCurLtrIdx = (pLtr.iCurLtrIdx + 1) % LONG_TERM_REF_NUM;
            pLtr.iLTRMarkSuccessNum = 1;
            pLtr.bLTRMarkEnable = true;
            pLtr.uiLtrMarkInterval = 0;

            if pCtx.vaa().is_some() {
                pCtx.vaa_mut().expect("the frame's video-analysis block").uiValidLongTermPicIdx = 0;
                pCtx.vaa_mut().expect("the frame's video-analysis block").uiMarkLongTermPicIdx = 0;
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
pub fn CheckCurMarkFrameNumUsed(pCtx: &mut sWelsEncCtx) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() {
        return false;
    }
    let Some(sps) = ctx_sps_ref(pCtx) else {
        return false;
    };
    let uiDid = pCtx.uiDependencyId as usize;
    // S10.5a: the scalars still come out before the list's `&mut`, because they
    // are reads of fields this body also borrows later — that half of T9.H3 was
    // never about raw roots, and it stays. The raw SPS deref it also covered is
    // gone: `ctx_sps_ref` is the shared twin, and this body reads one field.
    let gopSize = pCtx.param().uiGopSize;
    let iGoPFrameNumInterval = if (gopSize >> 1) > 1 {
        (gopSize >> 1) as i32
    } else {
        1
    };
    let iMaxFrameNumPlus1 = 1 << sps.uiLog2MaxFrameNum;
    // A7, §4.6 reorder: the only field read out of the layer's parameter block is
    // `iFrameNum`, a scalar, so the borrow does not have to span the list's `&mut`.
    let kiParamFrameNum = pCtx.param().sDependencyLayers[uiDid].iFrameNum;
    let (pRefList, pLtr) = pCtx.ref_list_and_ltr_mut(uiDid);
    let Some(pRefList) = pRefList else {
        return false;
    };

    for i in 0..((*pRefList).uiLongRefCount as usize) {
        if let Some(idLong) = (*pRefList).pLongRefList[i] {
            let iFrameNum = (*pRefList).pic(idLong).iFrameNum;
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
    // **The base arrives by value, and it is not a style preference** (T9.E2b,
    // the S29/S54 lineage). Both callers read `ppSliceList[0]`'s marking, and
    // iteration 0 of this loop writes the very bytes that marking lives in: a
    // reference parameter — `&` or `&mut` — is protected for the whole call
    // (F114b), so the write through `slice_in_layer(pCurDq, 0)` would pop it
    // mid-loop. The value cannot be invalidated by a retag, and the copy is
    // byte-identical to the C++'s `memcpy` from the live field: the first
    // store is `base = base`.
    for iSliceIdx in 0..kiCountSliceNum {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx);
        if let Some(pSlice) = pSlice {
            (*pSlice).sSliceHeaderExt.sSliceHeader.sRefMarking = kBaseMarking;
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
    // **T9.H3 — narrowed (S54).** The context parameter is gone: the body read
    // exactly two `ctx_param` scalars through it, and its single caller
    // (`WelsMarkPic`) passes them by value now. The LTR state arrives as a
    // shared borrow — the body only reads it — which the caller's own `&mut`
    // re-borrows for the call, so no second route to the context exists inside
    // and T9.X's protector argument is not needed at all.
    if kiCountSliceNum <= 0 {
        return;
    }
    let Some(pBaseSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, 0) else {
        return;
    };
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
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

    // S11.2c: the marking is copied out before the call, which ends the base
    // slice's borrow — the callee needs the layer `&mut` again to reach the
    // other slices, and the argument was always a by-value copy.
    let kBaseMarking = *pRefPicMark;
    WelsMarkMMCORefInfoWithBase(pCurDq, kBaseMarking, kiCountSliceNum);
}

/// Evaluates LTR marking criteria and populates slice header MMCO commands.
pub fn WelsMarkPic(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // **T9.H3 — the held cursor is gone.** T9.G7's note stood here: the body held
    // the LTR state raw across `CheckCurMarkFrameNumUsed`, which re-derives its
    // own borrow of the same `SLTRState`. Roots and scalars come first now; the
    // marking decision reads the state through one short borrow that ends before
    // that whole-context call, and the writes re-borrow after it, with the C++
    // condition's short-circuit order preserved exactly — the check runs iff both
    // cheap conjuncts held, and it only reads. The tail call reaches the context
    // no second time (`WelsMarkMMCORefInfo` is narrowed to the two scalars it
    // read, S54).
    // A7, §4.6 reorder: the three parameter fields this body reads are scalars, so
    // they come out here rather than as a borrow held across `ctx_ltr_at`'s `&mut`.
    let kbEnableLtr = pCtx.param().bEnableLongTermReference;
    let kiLtrMarkPeriod = pCtx.param().iLtrMarkPeriod;
    let kuiGopSize = pCtx.param().uiGopSize;
    let kuiTid = pCtx.uiTemporalId;
    let kiCountSliceNum = current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum;
    // **S11.11: the hoist is reversed, and the reason inverted with it.** T9.G6
    // derived this *before* the LTR block because the raw form's argument read
    // through the context and the derivation had to precede the `&mut`s below.
    // A `&mut SDqLayer` is the opposite: it must be taken *after* them, since
    // it and `ctx_ltr_at`'s borrow are both of the context. The derivation moves
    // to its one use, at the call.
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

    // The LTR record is `Copy`-read out first so the layer's `&mut` and it are
    // not both live borrows of the context at the call.
    let kLtr = *ctx_ltr_at(pCtx, uiDid);
    WelsMarkMMCORefInfo(
        kuiGopSize,
        kbEnableLtr,
        &kLtr,
        current_layer_mut(pCtx).expect("the frame's current layer is stamped"),
        kiCountSliceNum,
    );
}

/// Evaluates LTR recovery request feedback packets from decoder.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FilterLTRRecoveryRequest(
    pCtx: &mut sWelsEncCtx,
    pLTRRecoverRequest: &mut SLTRRecoverRequest,
) -> i32 {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one.
    // **T9.X**: the request pointer follows it, and the null guard goes with it —
    // upstream (`ref_list_mgr_svc.cpp:517`) has no such guard, dereferencing both
    // `pCtx->pSvcParam` and `pLTRRecoverRequest->iLayerId` unconditionally. The
    // guard was the port's own addition, not a behaviour the reference has.
    if pCtx.param_opt().is_none() {
        return 0;
    }
    if !pCtx.param().bEnableLongTermReference {
        for iDid in 0..(pCtx.param().iSpatialLayerNum as usize) {
            let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[iDid]);
            (*pParamInternal).bEncCurFrmAsIdrFlag = true;
        }
    } else {
        let pRequest = pLTRRecoverRequest;
        let iLayerId = (*pRequest).iLayerId;
        if iLayerId < 0 || iLayerId >= pCtx.param().iSpatialLayerNum {
            return 0;
        }

        // T9.H3 — the derivation order, inverted from T9.G4: scalar and the
        // permanently-raw root first, the LTR borrow last (see `DeleteInvalidLTR`).
        // S11.7: the scalar comes off the shared twin; `ctx_sps_ref` answers
        // `None` where the raw answered null, and the C++ dereferences here
        // unconditionally — so an absent SPS keeps the raw form's shape by
        // contributing the same `1 << 0` this expression would have read from a
        // zeroed record.
        let iMaxFrameNumPlus1 = 1 << ctx_sps_ref(pCtx).map_or(0, |s| s.uiLog2MaxFrameNum);
        let pParamInternal = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[iLayerId as usize]);
        let pLtr = ctx_ltr_at(pCtx, (iLayerId as usize) as usize);

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
pub fn FilterLTRMarkingFeedback(
    pCtx: &mut sWelsEncCtx,
    pLTRMarkingFeedback: &mut SLTRMarkingFeedback,
) {
    // T9.H / T9.X — as [`FilterLTRRecoveryRequest`]: upstream
    // (`ref_list_mgr_svc.cpp:563`) guards neither pointer.
    if pCtx.param_opt().is_none() {
        return;
    }
    let iLayerId = (*pLTRMarkingFeedback).iLayerId;
    if iLayerId < 0 || iLayerId >= pCtx.param().iSpatialLayerNum {
        return;
    }
    // A7, §4.6 reorder: the two parameter reads are scalars and come out before
    // the LTR state's `&mut`. T9.H3 asked for the same order; the flip enforces it.
    let kbEnableLtr = pCtx.param().bEnableLongTermReference;
    let kuiIdrPicId = pCtx.param().sDependencyLayers[iLayerId as usize].uiIdrPicId;
    let pLtr = ctx_ltr_at(pCtx, (iLayerId as usize) as usize);
    if kbEnableLtr {
        if (*pLTRMarkingFeedback).uiIDRPicId == kuiIdrPicId as u32
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
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).is_none() {
        return false;
    }
    // **A3: the list is re-derived at each read, not held.** This body writes
    // `iNumRef0`, `pRefList0` and the layer's `pRefOri` between its reads of the
    // list, and calls `WelsResetRefList` / `ResetLtrState` on the other arm — a
    // live borrow of one field cannot span writes to the others. The accessor is
    // a bounds check and a field projection, and this runs once per frame.
    // T9.H3: the held LTR binding is gone — the state is touched twice in this
    // body (one read in the branch condition, one write on recovery), and each
    // touch borrows inline for its own expression.
    let kiNumRef = pCtx.param().iNumRefFrame;
    let kuiTid = pCtx.uiTemporalId;
    let pParamD = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[uiDid]);

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
                    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefOri[numRef0] = Some(PicRef::Rec(idLong));
                    pCtx.pRefList0[numRef0] = Some(idLong);
                    pCtx.iNumRef0 += 1;
                    ctx_ltr_at(pCtx, (uiDid) as usize).iLastRecoverFrameNum = (*pParamD).iFrameNum;
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
                    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefOri[numRef0] = Some(PicRef::Rec(idRef));
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateBlockStatic(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.vaa().is_none() || pCtx.pVpp.is_none() {
        return;
    }
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
    //
    // **§4.6, reorder, and A5 had to**: `vaa_ext` derives from `&self`, so the
    // pointer it answers is a child of a shared retag of the context and dies at
    // the next `ref_list_mut` in the loop below. The old raw accessor read the
    // `Box`'s slot as a value and so carried the block's own provenance, which
    // survived that (F71/F211). Both fields wanted here are `Copy` — an `i32`
    // and a raw `*mut u8` — so they are read out before the loop and the
    // derivation never spans the `&mut`.
    // S11.3: `None` in this port (F177) — the screen path's best-reference
    // candidates do not exist, so the walk below has nothing to consider.
    let Some(pVaaExt) = pCtx.vaa_ext_ref() else {
        return;
    };
    let iVaaBestRefFrameNum = pVaaExt.iVaaBestRefFrameNum;
    let pVaaBestBlockStaticIdc = pVaaExt.pVaaBestBlockStaticIdc;
    // §4.6, reorder: the roots and scalars first, then the list per iteration,
    // held only across the two reads that need it. **S3.B1**: the vpp is *taken*
    // for the loop — the box moves out of the context, so the per-iteration
    // `ref_list_mut` borrow and the vpp uses are borrows of two different owners,
    // which is the fact the old raw-copy spelling was expressing.
    let uiDid = pCtx.uiDependencyId as usize;
    let pVpp = crate::encoder::encoder_context::ctx_vpp_raw(pCtx);
    let idEnc = pCtx.pEncPic;
    let kiNumRef0 = pCtx.iNumRef0 as usize;
    let pRefList0 = pCtx.pRefList0;
    for idx in 0..kiNumRef0 {
        let Some(idRef) = pRefList0[idx] else {
            continue;
        };
        // Two pools again: the reference is a reconstruction picture, the current one
        // a spatial source picture. Both are copied to geometry before the call.
        let pRefList = pCtx
            .ref_list_mut(uiDid)
            .expect("the dependency layer's reference list");
        let sRef = pRefList.pic_mut(idRef).planes();
        let iFrameNum = pRefList.pic(idRef).iFrameNum;
        if iVaaBestRefFrameNum != iFrameNum {
            let sSrc = idEnc.map(|id| pVpp.m_pSpatialPicPool.get_mut(id).planes());
            pVpp.UpdateBlockIdcForScreen(
                pVaaBestBlockStaticIdc,
                Some(&sRef),
                sSrc.as_ref(),
            );
        }
    }
}

/// Serializes slice header reference picture reordering syntax and marking flags.
pub fn WelsUpdateSliceHeaderSyntax(
    pCtx: &mut sWelsEncCtx,
    iAbsDiffPicNumMinus1: i32,
    pCurDq: &mut SDqLayer,
    uiFrameType: i32,
) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    // T9.E2i (the close's Miri verdict, F114b's shape): the count is read
    // through the parameter — `current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum` was a
    // second, independent path to the same object this function's `&mut`
    // already protects, and the read popped the protector.
    let kiCountSliceNum = pCurDq.iMaxSliceNum;
    let uiDid = pCtx.uiDependencyId as usize;
    // T9.H3: one bool, read once before the loop — the loop writes only slice
    // headers, so the value cannot change while it runs.
    let bLtrMarkingFlag = ctx_ltr_at(pCtx, (uiDid) as usize).bLTRMarkingFlag;

    for iIdx in 0..kiCountSliceNum {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iIdx)
        else {
            continue;
        };
        let pSliceHdr = &mut pSlice.sSliceHeaderExt.sSliceHeader;
        let pRefReorder = &mut pSliceHdr.sRefReordering;
        let pRefPicMark = &mut pSliceHdr.sRefMarking;

        pSliceHdr.uiRefCount = pCtx.iNumRef0 as u8;
        if pCtx.iNumRef0 > 0 {
            // §4.6: the flag is read out, the borrow ends, and the parameter
            // read below takes the context for itself.
            let isLongRef = match pCtx.pRefList0[0] {
                Some(id) => pCtx
                    .ref_list(pCtx.uiDependencyId as usize)
                    .expect("the dependency layer's reference list")
                    .pic(id)
                    .bIsLongRef,
                None => false,
            };
            if !isLongRef || !pCtx.param().bEnableLongTermReference {
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
                                pCtx.ref_list(pCtx.uiDependencyId as usize).expect("the dependency layer's reference list").pic(id).iLongTermPicNum as u16;
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
            pRefPicMark.bLongTermRefFlag = pCtx.param().bEnableLongTermReference;
        } else {
            // SCREEN_CONTENT(dormant: Phase 10)
            if pCtx.param().iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = pCtx.param().bEnableLongTermReference;
            } else {
                pRefPicMark.bAdaptiveRefPicMarkingModeFlag = pCtx.param().bEnableLongTermReference && bLtrMarkingFlag;
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
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return;
    }
    let mut iAbsDiffPicNumMinus1 = -1i32;
    let uiDid = pCtx.uiDependencyId as usize;
    let pParamD = &pCtx.param().sDependencyLayers[uiDid];

    if pCtx.iNumRef0 > 0 {
        let pRefList = pCtx.ref_list(uiDid).expect("the dependency layer's reference list");
        if let Some(id) = pCtx.pRefList0[0] {
            iAbsDiffPicNumMinus1 = pParamD.iFrameNum - (*pRefList).pic(id).iFrameNum - 1;
            if iAbsDiffPicNumMinus1 < 0 {
                // S11.7: the guard is the `Option`, which is the same test.
                if let Some(kpSps) = ctx_sps_ref(pCtx) {
                    iAbsDiffPicNumMinus1 += 1 << kpSps.uiLog2MaxFrameNum;
                }
            }
        }
    }

    if current_layer_ref(pCtx).is_some() {
        // The null arm of the callee's old guard, hoisted with the reborrow.
        // **S11.11: the raw stays here.** `WelsUpdateSliceHeaderSyntax` takes
        // `&mut sWelsEncCtx` *and* `&mut SDqLayer` where the layer lives inside
        // that context — `DynamicAdjustSlicing`'s shape, and the third member of
        // the seam list. F71's slot read keeps the two provenances apart; a
        // `current_layer_mut` here would be a second `&mut` on the context and
        // the borrow checker refuses it, correctly.
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
fn UpdateOriginalPicInfoFromCtx(pCtx: &mut sWelsEncCtx) {
    let (Some(idEnc), Some(idDec)) = (pCtx.pEncPic, pCtx.pDecPic) else {
        return;
    };
    // S10.5a: two owners, said to the compiler instead of to Miri. The comment
    // that stood here described taking the preprocess box out through
    // `ctx_vpp_raw` so borrowck would grant the list borrow beside it; that is a
    // slot read standing in for a disjointness `vpp_and_ref_list_mut` states
    // directly, `pVpp` and `ppRefPicListExt` being two fields of one struct.
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
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if pCtx.pVpp.is_some() && pCtx.vaa().is_some() {
        // S10.5a: the scalars still come out first — they are reads of fields the
        // combined borrow below also claims. What is gone is the "take the vpp
        // through the slot so borrowck grants both" step: `vpp_and_ref_list_mut`
        // projects the two fields from one borrow and says the same thing safely.
        let idEnc = pCtx.pEncPic;
        let uiMarkLongTermPicIdx = pCtx.vaa().expect("the frame's video-analysis block").uiMarkLongTermPicIdx as i32;
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
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let iDIdx = pCtx.uiDependencyId as i32;
    UpdateOriginalPicInfoFromCtx(pCtx);
    PrefetchNextBuffer(pCtx);
    if pCtx.pVpp.is_some() {
        let idEnc = pCtx.pEncPic;
        // S10.5a: `ctx_vpp_raw` was here so the `&mut` receiver and the shared
        // list borrow would be "siblings off two allocations". They are two
        // *fields*, which is a stronger statement and one the compiler can check.
        let (pVpp, pRefList) = pCtx.vpp_and_ref_list_mut(iDIdx as usize);
        let shortCount = pRefList
            .expect("the dependency layer's reference list")
            .uiShortRefCount;
        pVpp.expect("the preprocess object").UpdateSrcList(idEnc, iDIdx, shortCount as u32);
    }
}

/// Screen content specialized reference picture list update.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsUpdateRefListScreen(pCtx: &mut sWelsEncCtx) -> bool {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if current_layer_ref(pCtx).is_none() || pCtx.param_opt().is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    if pCtx.ref_list(uiDid).map_or(true, |l| l.pRef.is_empty()) {
        return false;
    }
    // **T9.H3 — the held cursor is gone.** T9.G7's note stood here: the body held
    // the LTR state raw across `LTRMarkProcessScreen`, which re-derives its own
    // `&mut` to the *same* `SLTRState`. Nothing is held across those calls any
    // more — each branch re-borrows `ctx_ltr_at` *after* its calls return, and the
    // two reads in the `pDecPic` block borrow inline for one expression each. The
    // borrow checker referees every coexistence; this function is dead code today
    // (F192: screen-content mode is rejected at init), so the compiler is the
    // *only* referee and the reshape moves derivations, never logic.
    let pParamD = std::ptr::addr_of_mut!((*ctx_param_raw(pCtx)).sDependencyLayers[uiDid]);
    let kuiTid = pCtx.uiTemporalId;

    if let Some(idDec) = pCtx.pDecPic {
        // A3: as in `WelsUpdateRefList`, the list borrow is scoped to this block
        // and the context scalars it needs are read out first, because the tail
        // calls `DeleteNonSceneLTR` / `LTRMarkProcessScreen`.
        let uiTemporalId = pCtx.uiTemporalId;
        let uiDependencyId = pCtx.uiDependencyId;
        let bIsSceneLTR = ctx_ltr_at(pCtx, uiDid).bLTRMarkingFlag
            || (pCtx.param().bEnableLongTermReference
                && pCtx.eSliceType == EWelsSliceType::I_SLICE);
        let iLongTermPicNum = ctx_ltr_at(pCtx, uiDid).iCurLtrIdx;
        let Some(pRefList) = pCtx.ref_list_mut(uiDid) else {
            return false;
        };
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

        pDecPic.uiTemporalId = uiTemporalId;
        pDecPic.uiSpatialId = uiDependencyId;
        pDecPic.iFrameNum = (*pParamD).iFrameNum;
        pDecPic.iFramePoc = (*pParamD).iPOC;
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
            pCtx.vaa_mut().expect("the frame's video-analysis block").uiValidLongTermPicIdx = 0;
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
    if pCtx.param_opt().is_none() || pCtx.vaa().is_none() || current_layer_ref(pCtx).is_none() {
        return false;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // A7, §4.6 reorder: this body writes `iNumRef0` and the layer's `pRefOri`, so
    // the parameter borrow may not be held across them — both fields it wants are
    // scalars.
    let iNumRef = pCtx.param().iNumRefFrame;
    let iLTRRefNum = pCtx.param().iLTRRefNum;
    // ref_list_mgr_svc.cpp:649 — static_cast<SVAAFrameInfoExt*> (pCtx->pVaa)
    //
    // §4.6, reorder: the count is `Copy` and is read out here, because the loop
    // below calls `GetRefFrameInfo`, which reaches the same block again — and
    // `vaa_ext`'s answer is a child of a shared retag now, not the slot read that
    // outlived every later derivation (F71/F211).
    // S11.3: `None` in this port (F177); zero available screen references is
    // the value every camera preset already computes here.
    let iNumOfAvailableRef = pCtx.vaa_ext_ref().map_or(0, |ext| ext.iNumOfAvailableRef);
    pCtx.iNumRef0 = 0;

    if pCtx.eSliceType != EWelsSliceType::I_SLICE {
        let mut iLtrRefIdx = 0i32;
        // The screen path's `pRefOri` is a **spatial source** picture, where the
        // camera path's is a reconstruction picture — see [`PicRef`].
        let mut pRefOri: Option<SrcPicId> = None;

        for idx in 0..iNumOfAvailableRef {
            // **T9.H2, F192 — this is the site the finding is about.** It called
            // `GetRefFrameInfo` while holding `pCtx: &mut sWelsEncCtx`, and the callee
            // read the context back out of `CWelsPreProcess::m_pEncCtx`: a read through
            // a stored raw of an allocation whose `&mut` is *strongly protected* for the
            // duration of this call, which Miri refuses. The context is an argument now,
            // so there is no second route and nothing to refuse.
            //
            // The scene-LTR flag is read out before the borrow (T9.G6's shape);
            // the object itself is *taken* (S3.B1), so the `&mut self` receiver
            // and the `&mut pCtx` argument are two owners for the call.
            let bSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
            if pCtx.pVpp.is_some() {
                iLtrRefIdx =
                    crate::encoder::encoder_context::ctx_vpp_raw(pCtx).GetRefFrameInfo(pCtx, idx, bSceneLtr, &mut pRefOri);
            }
            let refOri = pRefOri.map(PicRef::Src);
            if iLtrRefIdx >= 0 && iLtrRefIdx <= iLTRRefNum {
                // A3: the list is re-derived per read, not held — this loop
                // writes `iNumRef0`, `pRefList0` and the layer's `pRefOri`.
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
                    current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefOri[num0] = refOri;
                    pCtx.pRefList0[num0] = Some(idRefPic);
                    pCtx.iNumRef0 += 1;
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
                        current_layer_mut(pCtx).expect("the frame's current layer is stamped").pRefOri[num0] = refOri;
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
    // **T9.H3 — narrowed (S54).** The context parameter is gone: the body read
    // exactly two `ctx_param` scalars through it, and its single caller
    // (`WelsMarkPicScreen`) passes them by value now. The LTR state arrives as a
    // shared borrow — the body only reads it — which the caller's own `&mut`
    // re-borrows for the call, so no second route to the context exists inside
    // and T9.X's protector argument is not needed at all.
    if kiCountSliceNum <= 0 {
        return;
    }
    let Some(pBaseSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, 0) else {
        return;
    };
    let pRefPicMark = &mut (*pBaseSlice).sSliceHeaderExt.sSliceHeader.sRefMarking;
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

    // S11.2c: the marking is copied out before the call, which ends the base
    // slice's borrow — the callee needs the layer `&mut` again to reach the
    // other slices, and the argument was always a by-value copy.
    let kBaseMarking = *pRefPicMark;
    WelsMarkMMCORefInfoWithBase(pCurDq, kBaseMarking, kiCountSliceNum);
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMarkPicScreen(pCtx: &mut sWelsEncCtx) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pCtx.param_opt().is_none() || current_layer_ref(pCtx).is_none() {
        return;
    }
    let uiDid = pCtx.uiDependencyId as usize;
    // **T9.H3 — roots and scalars first, the LTR reference last (T9.G4 inverted).**
    // T9.G7's held-raw note stood here. Every whole-context derivation this body
    // needs — the param root, the ref list, the SPS root, the current layer for
    // the MMCO call, and the two context scalars — is taken *before* the LTR
    // state is borrowed, so one `&mut SLTRState` spans the body with only raw
    // derefs and locals beside it, and the tail call reaches the context no
    // second time (`WelsMarkMMCORefInfoScreen` is narrowed to the two scalars it
    // read). Dead code today (F192: screen-content mode is rejected at init) —
    // the borrow checker is the only referee, and the reshape moves derivations,
    // never logic.
    // A7, §4.6 reorder: the parameter block is read into scalars here — the borrow
    // may not span `ref_list_and_ltr_mut` below.
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
    // §4.6: one scalar out of the list, read before the LTR borrow is taken.
    let bIsRefListNotFull = (pCtx
        .ref_list(uiDid)
        .expect("the dependency layer's reference list")
        .uiLongRefCount as i32)
        < iLongRefNum;
    let pSps = ctx_sps(pCtx);
    let kuiTid = pCtx.uiTemporalId;
    let kbSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
    let iSliceNum = current_layer_ref(pCtx).expect("the frame's current layer is stamped").iMaxSliceNum;
    // **S11.11**: the T9.G6 hoist is reversed here as in `WelsMarkPic` — a
    // `&mut SDqLayer` must be taken *after* the context's other borrows, not
    // before them, so it moves to its one use at the call below.
    // A3: the list and the LTR state are read and written in the same branches
    // here, so they come out of one borrow — §4.6's combined accessor.
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
                    if (*pRefList).pLongRefList[i as usize].is_none() {
                        pLtr.iCurLtrIdx = i;
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
                let iMaxFrameNum = 1 << (*pSps).uiLog2MaxFrameNum;

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

    // S11.11: the LTR record is `Copy`-read out so it and the layer's `&mut`
    // are not both live borrows of the context at the call — `WelsMarkPic`'s
    // shape, and the callee only reads it.
    let kLtr = *pLtr;
    WelsMarkMMCORefInfoScreen(
        iNumRef,
        kbEnableLtr,
        &kLtr,
        current_layer_mut(pCtx).expect("the frame's current layer is stamped"),
        iSliceNum,
    );
}

/// Intentional no-op reference list manager callback.
/// Matches `void DoNothing (sWelsEncCtx* pointer)` in `ref_list_mgr_svc.cpp:996`.
pub fn DoNothing(_pCtx: &mut sWelsEncCtx) {}

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
    /// **S10.5a: the `# Safety` clause is gone with the `unsafe`.** All three arms
    /// went safe in this checkpoint, so the obligation this stated — "`pCtx` must be
    /// a live encoder context" — is now the `&mut sWelsEncCtx`'s, which is where it
    /// always belonged. The cascade is what found this: the declaration carried no
    /// raw pointer and its body's last unsafe call went with `UpdateSrcPicList`.
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
    fn test_ref_list_mgr_noop_callback() {
        // T9.H11: the context argument is `&mut` now, so the null goes — see
        // `rc.rs`'s `test_rc_intentional_noop_callbacks` for the reasoning. A
        // `&mut sWelsEncCtx` cannot be null, so the type enforces more than the
        // old `null_mut()` argument asserted.
        let mut ctx = Box::new(sWelsEncCtx::default());
        DoNothing(&mut ctx);
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
    // unsafe-cat: instrument(test)
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
