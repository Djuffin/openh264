#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Reference picture buffer management, list construction, reordering, and DPB lifecycle.
//!
//! Translated from `codec/decoder/core/inc/manage_dec_ref.h` and `codec/decoder/core/src/manage_dec_ref.cpp`.

use std::ffi::c_void;
use crate::decoder::decoder_core::DqLayerState;
use crate::decoder::parameter_sets::SSps;
pub use crate::decoder::nalu::{EWelsNalUnitType, EWelsNalUnitType::*};
pub use crate::decoder::slice::{EWelsSliceType, EWelsSliceType::*, MMCO_END, MMCO_SHORT2UNUSED, MMCO_LONG2UNUSED, MMCO_SHORT2LONG, MMCO_SET_MAX_LONG, MMCO_RESET, MMCO_LONG};
pub use crate::decoder::error_concealment::{ERROR_CON_IDC, ERROR_CON_IDC::*};
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = MAX_REF_PIC_COUNT + 1; // 17
pub const MAX_MMCO_COUNT: usize = 66;

pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

// Error Codes matching `codec/decoder/core/inc/error_code.h`
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_COMMON_BASE: i32 = 1;
pub const ERR_INFO_SYNTAX_BASE: i32 = 1001;
pub const ERR_INFO_LOGIC_BASE: i32 = 10001;

pub const ERR_INFO_INVALID_PTR: i32 = ERR_INFO_COMMON_BASE + 2; // 3
pub const ERR_INFO_REF_COUNT_OVERFLOW: i32 = ERR_INFO_SYNTAX_BASE + 9; // 1010
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = ERR_INFO_SYNTAX_BASE + 84; // 1085

pub const ERR_INFO_DUPLICATE_FRAME_NUM: i32 = ERR_INFO_LOGIC_BASE + 4; // 10005
pub const ERR_INFO_INVALID_MMCO_NUM: i32 = ERR_INFO_LOGIC_BASE + 5; // 10006
pub const ERR_INFO_INVALID_MMCO_OPCODE_BASE: i32 = ERR_INFO_LOGIC_BASE + 6; // 10007
pub const ERR_INFO_INVALID_MMCO_SHORT2UNUSED: i32 = ERR_INFO_LOGIC_BASE + 7; // 10008
pub const ERR_INFO_INVALID_MMCO_LONG2UNUSED: i32 = ERR_INFO_LOGIC_BASE + 8; // 10009
pub const ERR_INFO_INVALID_MMCO_SHOART2LONG: i32 = ERR_INFO_LOGIC_BASE + 9; // 10010
pub const ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW: i32 = ERR_INFO_LOGIC_BASE + 10; // 10011
pub const ERR_INFO_INVALID_MMCO_REF_NUM_NOT_ENOUGH: i32 = ERR_INFO_LOGIC_BASE + 11; // 10012
pub const ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX: i32 = ERR_INFO_LOGIC_BASE + 12; // 10013

// Decoding status flags (bitwise error code flags)
pub const dsNoParamSets: i32 = 0x10;
pub const dsDataErrorConcealed: i32 = 0x20;
pub const dsOutOfMemory: i32 = 0x4000;

// Log Levels
pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;

// ============================================================================
// Data Structures
// ============================================================================

pub use crate::decoder::decoder_context::{Picture, SPicture, PPicture};


pub use crate::decoder::decoder_context::{SRefPic, PRefPic};
pub use crate::decoder::slice::{SRefPicListReorderSyn, PRefPicListReorderSyn, SRefPicMarking, PRefPicMarking};


pub use crate::decoder::slice::{SSliceHeader, PSliceHeader, SSliceHeaderExt};


pub use crate::decoder::decoder_context::SLogContext;


pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};


// ============================================================================
// Internal Logging & Picture Helpers
// ============================================================================

#[inline(always)]
pub unsafe fn WelsLog(_pLogCtx: &SLogContext, _iLevel: i32, _msg: &str) {

    // Logging stub for no-std / embedded compatibility
}

// T4b.3b: this file's `ExpandReferencingPicture` was the third of three copies of
// `expand_pic.cpp:388`, and the one that had drifted furthest: its `kiWidthUV >= 16`
// test had no `else`, so the chroma planes of a frame narrower than 32 pixels were
// never expanded at all. See `phase4b_findings.md` F21. The single copy now lives in
// `common/expand_pic.rs` with the C++'s body.

// ============================================================================
// Core Reference Management Implementation
// ============================================================================

/// Shifts `len` entries of one DPB list from `src` to `dst`, overlap-safe.
///
/// **F13's `manage_dec_ref` site, closed here (T5.B2).** Every one of the six
/// list shifts in this module was written
/// `ptr::copy(list.as_ptr().add(a), list.as_mut_ptr().add(b), n)`. Rust evaluates
/// the arguments left to right, so `as_mut_ptr()` re-borrows the array *after*
/// `as_ptr()` handed out a tag, which pops it — and the copy then reads through
/// the dead one. The addresses are right and every build emits what the author
/// meant, which is why only Miri sees it, and why `gates.sh` skipped every
/// `manage_dec_ref` test rather than read a failure it could not fix in Phase 2.
///
/// `copy_within` is the whole fix: one borrow, one expression.
///
/// The `min` is the array's own bound made explicit and is **not** an arithmetic
/// change — on every path where the C's `memmove` stayed inside the list it
/// trims nothing. Where it would not have (`uiShortRefCount` reaching
/// `MAX_DPB_COUNT`, `iMaxRefIdx` likewise — both bitstream-derived and neither
/// clamped on the way in), the C wrote one entry past the end of the list into
/// the field behind it; this drops that entry instead. Bounding those two counts
/// at their source is the F8/F9/F11 class and stays out of scope per §9.
#[inline]
fn shift_dpb_entries(list: &mut [*mut SPicture], src: usize, dst: usize, len: usize) {
    let len = len
        .min(list.len().saturating_sub(src))
        .min(list.len().saturating_sub(dst));
    list.copy_within(src..src + len, dst);
}

/// Unmarks a reconstructed picture as unused for reference and resets its identifiers.
///
/// Matches `static void SetUnRef (PPicture pRef)` in `manage_dec_ref.cpp`.
///
/// **S25, for this whole module.** This function and the four `WelsDel*` helpers
/// take the DPB by raw pointer and re-borrow it `&mut` on entry, and the functions
/// that call them were handed the *same* raw pointers. A caller that binds
/// `let ref_pic = &mut *pRefPic` and then makes one of these calls has two live
/// `&mut` to one `SRefPic`, the inner one not derived from the outer, and reads
/// through the outer afterwards. `SlidingWindow`, `RemainOneBufferInDpbForEC` and
/// `MMCOProcess` were all written that way and are not any more (T5.B2): they name
/// `(*pRefPic)` / `(*pCtx)` at each use, so no borrow outlives one expression.
/// **The same applies to `pCtx`, which is not a separate object** — every caller
/// passes `&mut ctx.sRefPic` as `pRefPic`, so `&mut *pCtx` and `&mut *pRefPic`
/// overlap. Anything added here inherits the rule.
pub unsafe extern "C" fn SetUnRef(pRef: *mut SPicture) {
    if pRef.is_null() {
        return;
    }
    let ref_pic = &mut *pRef;

    if ref_pic.iRefCount <= 0 {
        ref_pic.bUsedAsRef = false;
        ref_pic.bIsLongRef = false;
        ref_pic.iFrameNum = -1;
        ref_pic.iFrameWrapNum = -1;
        ref_pic.iLongTermFrameIdx = -1;
        ref_pic.uiLongTermPicNum = 0;
        ref_pic.uiQualityId = 0xFF; // -1 as u8
        ref_pic.uiTemporalId = 0xFF; // -1 as u8
        ref_pic.uiSpatialId = 0xFF; // -1 as u8
        ref_pic.iSpsId = -1;
        ref_pic.bIsComplete = false;
        ref_pic.iRefCount = 0;
        ref_pic.pSetUnRef = None;

        if ref_pic.eSliceType == EWelsSliceType::I_SLICE {
            return;
        }
        let lists = if ref_pic.eSliceType == EWelsSliceType::P_SLICE { 1 } else { 2 };
        for i in 0..MAX_DPB_COUNT {
            for list in 0..lists {
                ref_pic.pRefPic[list][i] = std::ptr::null_mut();
            }
        }
    } else {
        ref_pic.pSetUnRef = Some(SetUnRef);
    }
}

/// Flushes all reference pictures from short-term and long-term lists and invokes `SetUnRef`.
///
/// Matches `void WelsResetRefPic (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsResetRefPic(pCtx: *mut SWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pRefPic: *mut SRefPic = std::ptr::addr_of_mut!((*pCtx).sRefPic);
    (*pRefPic).uiLongRefCount[LIST_0] = 0;
    (*pRefPic).uiShortRefCount[LIST_0] = 0;

    (*pRefPic).uiRefCount[LIST_0] = 0;
    (*pRefPic).uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        let pPic = (*pRefPic).pShortRefList[LIST_0][i];
        if !pPic.is_null() {
            SetUnRef(pPic);
            (*pRefPic).pShortRefList[LIST_0][i] = std::ptr::null_mut();
        }
    }
    (*pRefPic).uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        let pPic = (*pRefPic).pLongRefList[LIST_0][i];
        if !pPic.is_null() {
            SetUnRef(pPic);
            (*pRefPic).pLongRefList[LIST_0][i] = std::ptr::null_mut();
        }
    }
    (*pRefPic).uiLongRefCount[LIST_0] = 0;
}

/// Clears reference list pointers and counts without invoking `SetUnRef`.
///
/// Matches `void WelsResetRefPicWithoutUnRef (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsResetRefPicWithoutUnRef(pCtx: *mut SWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pRefPic: *mut SRefPic = std::ptr::addr_of_mut!((*pCtx).sRefPic);
    (*pRefPic).uiLongRefCount[LIST_0] = 0;
    (*pRefPic).uiShortRefCount[LIST_0] = 0;

    (*pRefPic).uiRefCount[LIST_0] = 0;
    (*pRefPic).uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        (*pRefPic).pShortRefList[LIST_0][i] = std::ptr::null_mut();
    }
    (*pRefPic).uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        (*pRefPic).pLongRefList[LIST_0][i] = std::ptr::null_mut();
    }
    (*pRefPic).uiLongRefCount[LIST_0] = 0;
}

/// Deletes a short-term reference picture with `iFrameNum` from `pShortRefList[0]`.
///
/// Matches `static PPicture WelsDelShortFromList (PRefPic pRefPic, int32_t iFrameNum)`.
pub unsafe fn WelsDelShortFromList(pRefPic: *mut SRefPic, iFrameNum: i32) -> *mut SPicture {
    if pRefPic.is_null() {
        return std::ptr::null_mut();
    }
    let ref_pic = &mut *pRefPic;
    let count = ref_pic.uiShortRefCount[LIST_0] as usize;

    for i in 0..count {
        let pPic = ref_pic.pShortRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFrameNum == iFrameNum {
            let iMoveSize = count - i - 1;
            let pic = &mut *pPic;
            pic.bUsedAsRef = false;
            ref_pic.pShortRefList[LIST_0][i] = std::ptr::null_mut();

            if iMoveSize > 0 {
                shift_dpb_entries(&mut ref_pic.pShortRefList[LIST_0], i + 1, i, iMoveSize);
            }
            ref_pic.uiShortRefCount[LIST_0] -= 1;
            let new_count = ref_pic.uiShortRefCount[LIST_0] as usize;
            ref_pic.pShortRefList[LIST_0][new_count] = std::ptr::null_mut();
            return pPic;
        }
    }
    std::ptr::null_mut()
}

/// Deletes a short-term reference picture and immediately calls `SetUnRef`.
pub unsafe fn WelsDelShortFromListSetUnref(pRefPic: *mut SRefPic, iFrameNum: i32) -> *mut SPicture {
    let pPic = WelsDelShortFromList(pRefPic, iFrameNum);
    if !pPic.is_null() {
        SetUnRef(pPic);
    }
    pPic
}

/// Deletes a long-term reference picture with `uiLongTermFrameIdx` from `pLongRefList[0]`.
///
/// Matches `static PPicture WelsDelLongFromList (PRefPic pRefPic, uint32_t uiLongTermFrameIdx)`.
pub unsafe fn WelsDelLongFromList(pRefPic: *mut SRefPic, uiLongTermFrameIdx: u32) -> *mut SPicture {
    if pRefPic.is_null() {
        return std::ptr::null_mut();
    }
    let ref_pic = &mut *pRefPic;
    let count = ref_pic.uiLongRefCount[LIST_0] as usize;

    for i in 0..count {
        let pPic = ref_pic.pLongRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iLongTermFrameIdx == uiLongTermFrameIdx as i32 {
            let iMoveSize = count - i - 1;
            let pic = &mut *pPic;
            pic.bUsedAsRef = false;
            pic.bIsLongRef = false;

            if iMoveSize > 0 {
                shift_dpb_entries(&mut ref_pic.pLongRefList[LIST_0], i + 1, i, iMoveSize);
            }
            ref_pic.uiLongRefCount[LIST_0] -= 1;
            let new_count = ref_pic.uiLongRefCount[LIST_0] as usize;
            ref_pic.pLongRefList[LIST_0][new_count] = std::ptr::null_mut();
            return pPic;
        }
    }
    std::ptr::null_mut()
}

/// Deletes a long-term reference picture and immediately calls `SetUnRef`.
pub unsafe fn WelsDelLongFromListSetUnref(
    pRefPic: *mut SRefPic,
    uiLongTermFrameIdx: u32,
) -> *mut SPicture {
    let pPic = WelsDelLongFromList(pRefPic, uiLongTermFrameIdx);
    if !pPic.is_null() {
        SetUnRef(pPic);
    }
    pPic
}

/// Inserts a decoded picture at index 0 of `pShortRefList[0]`.
///
/// Matches `static int32_t AddShortTermToList (PRefPic pRefPic, PPicture pPic)`.
pub unsafe fn AddShortTermToList(pRefPic: *mut SRefPic, pPic: *mut SPicture) -> i32 {
    if pRefPic.is_null() || pPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pic = &mut *pPic;
    pic.bUsedAsRef = true;
    pic.bIsLongRef = false;
    pic.iLongTermFrameIdx = -1;

    let ref_pic = &mut *pRefPic;
    let short_count = ref_pic.uiShortRefCount[LIST_0] as usize;

    if short_count > 0 {
        for iPos in 0..short_count {
            let cur = ref_pic.pShortRefList[LIST_0][iPos];
            if cur.is_null() {
                return ERR_INFO_INVALID_PTR;
            }
            if pic.iFrameNum == (*cur).iFrameNum {
                ref_pic.pShortRefList[LIST_0][iPos] = pPic;
                return ERR_INFO_DUPLICATE_FRAME_NUM;
            }
        }
        shift_dpb_entries(&mut ref_pic.pShortRefList[LIST_0], 0, 1, short_count);
    }
    ref_pic.pShortRefList[LIST_0][0] = pPic;
    ref_pic.uiShortRefCount[LIST_0] += 1;
    ERR_NONE
}

/// Inserts a decoded picture into `pLongRefList[0]`, keeping it sorted in ascending order of `iLongTermFrameIdx`.
///
/// Matches `static int32_t AddLongTermToList (PRefPic pRefPic, PPicture pPic, int32_t iLongTermFrameIdx, uint32_t uiLongTermPicNum)`.
pub unsafe fn AddLongTermToList(
    pRefPic: *mut SRefPic,
    pPic: *mut SPicture,
    iLongTermFrameIdx: i32,
    uiLongTermPicNum: u32,
) -> i32 {
    if pRefPic.is_null() || pPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pic = &mut *pPic;
    pic.bUsedAsRef = true;
    pic.bIsLongRef = true;
    pic.iLongTermFrameIdx = iLongTermFrameIdx;
    pic.uiLongTermPicNum = uiLongTermPicNum;

    let ref_pic = &mut *pRefPic;
    let long_count = ref_pic.uiLongRefCount[LIST_0] as usize;

    if long_count == 0 {
        ref_pic.pLongRefList[LIST_0][0] = pPic;
    } else {
        let mut insert_idx = long_count.min(MAX_REF_PIC_COUNT);
        for i in 0..insert_idx {
            let cur = ref_pic.pLongRefList[LIST_0][i];
            if cur.is_null() {
                return ERR_INFO_INVALID_PTR;
            }
            if (*cur).iLongTermFrameIdx > pic.iLongTermFrameIdx {
                insert_idx = i;
                break;
            }
        }
        let move_count = long_count - insert_idx;
        if move_count > 0 {
            shift_dpb_entries(
                &mut ref_pic.pLongRefList[LIST_0],
                insert_idx,
                insert_idx + 1,
                move_count,
            );
        }
        ref_pic.pLongRefList[LIST_0][insert_idx] = pPic;
    }

    if (ref_pic.uiLongRefCount[LIST_0] as usize) < MAX_REF_PIC_COUNT {
        ref_pic.uiLongRefCount[LIST_0] += 1;
    }
    ERR_NONE
}

/// Converts a short-term reference picture to a long-term reference picture.
pub unsafe fn MarkAsLongTerm(
    pRefPic: *mut SRefPic,
    iFrameNum: i32,
    iLongTermFrameIdx: i32,
    uiLongTermPicNum: u32,
) -> i32 {
    let _ = WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
    if pRefPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut iRet = ERR_NONE;
    let count = (*pRefPic).uiRefCount[LIST_0] as usize;

    for i in 0..count {
        let pPic = (*pRefPic).pRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFrameNum == iFrameNum && !(*pPic).bIsLongRef {
            iRet = AddLongTermToList(pRefPic, pPic, iLongTermFrameIdx, uiLongTermPicNum);
            break;
        }
    }
    iRet
}

/// Locates the long-term frame index corresponding to `iAncLTRFrameNum`.
pub unsafe fn GetLTRFrameIndex(pRefPic: *mut SRefPic, iAncLTRFrameNum: i32) -> i32 {
    if pRefPic.is_null() {
        return -1;
    }
    let ref_pic = &*pRefPic;
    let long_count = ref_pic.uiLongRefCount[LIST_0] as usize;
    for i in 0..long_count {
        let pPic = ref_pic.pLongRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFrameNum == iAncLTRFrameNum {
            return (*pPic).iLongTermFrameIdx;
        }
    }
    -1
}

/// Evaluates short-term frame number wrapping modulo `1 << uiLog2MaxFrameNum`.
///
/// Matches `static void WrapShortRefPicNum (PWelsDecoderContext pCtx)`.
pub unsafe fn WrapShortRefPicNum(pCtx: *mut SWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    if (*pCtx).pCurDqLayer.is_null() {
        return;
    }
    let pCurDqLayer: *mut DqLayerState = (*pCtx).pCurDqLayer;
    let pSliceHeader =
        std::ptr::addr_of_mut!((*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader);
    if (*pSliceHeader).pSps.is_null() {
        return;
    }
    let pSps = &*((*pSliceHeader).pSps as *mut SSps);

    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iShortRefCount = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;

    for i in 0..iShortRefCount {
        let pPic = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() {
            if (*pPic).iFrameNum > (*pSliceHeader).iFrameNum {
                (*pPic).iFrameWrapNum = (*pPic).iFrameNum - iMaxPicNum;
            } else {
                (*pPic).iFrameWrapNum = (*pPic).iFrameNum;
            }
        }
    }
}

/// Evicts the oldest short-term reference picture when DPB reaches capacity.
///
/// Matches `static int32_t SlidingWindow (PWelsDecoderContext pCtx, PRefPic pRefPic)`.
pub unsafe fn SlidingWindow(pCtx: *mut SWelsDecoderContext, pRefPic: *mut SRefPic) -> i32 {
    if pCtx.is_null() || pRefPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    // S25: no borrow of `*pCtx` or `*pRefPic` outlives one expression here. Both
    // `WelsDelShortFromList` and `SetUnRef` re-enter through the raw pointers this
    // function was handed, so a `let ref_pic = &mut *pRefPic` held across them is
    // the shape the rule names — invalidated by the callee's own `&mut`, and read
    // afterwards. See the module note above `SetUnRef`.
    let num_ref_frames = if !(*pCtx).pSps.is_null() {
        (*(*pCtx).pSps).iNumRefFrames as u8
    } else {
        1
    };

    if (*pRefPic).uiShortRefCount[LIST_0] + (*pRefPic).uiLongRefCount[LIST_0] >= num_ref_frames {
        if (*pRefPic).uiShortRefCount[LIST_0] == 0 {
            WelsLog(
                &(*pCtx).sLogCtx,
                WELS_LOG_ERROR,
                "No reference picture in short term list when sliding window",
            );
            return ERR_INFO_INVALID_MMCO_REF_NUM_NOT_ENOUGH;
        }
        let short_count = (*pRefPic).uiShortRefCount[LIST_0] as isize;
        for i in (0..short_count).rev() {
            let pCur = (*pRefPic).pShortRefList[LIST_0][i as usize];
            if !pCur.is_null() {
                let pPic = WelsDelShortFromList(pRefPic, (*pCur).iFrameNum);
                if !pPic.is_null() {
                    SetUnRef(pPic);
                    break;
                } else {
                    return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
                }
            }
        }
    }
    ERR_NONE
}

/// Ensures at least 1 free slot in the DPB for error concealment operations.
///
/// Matches `static int32_t RemainOneBufferInDpbForEC (PWelsDecoderContext pCtx, PRefPic pRefPic)`.
pub unsafe fn RemainOneBufferInDpbForEC(
    pCtx: *mut SWelsDecoderContext,
    pRefPic: *mut SRefPic,
) -> i32 {
    if pCtx.is_null() || pRefPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    // S25, as in `SlidingWindow`: the loop below *depends* on a re-entrant call
    // changing `uiLongRefCount`, so its condition has to read the live field and
    // not a borrow the call invalidated.
    let num_ref_frames = if !(*pCtx).pSps.is_null() {
        (*(*pCtx).pSps).iNumRefFrames as u8
    } else {
        1
    };

    if (*pRefPic).uiShortRefCount[0] + (*pRefPic).uiLongRefCount[0] < num_ref_frames {
        return ERR_NONE;
    }

    let mut iRet = ERR_NONE;
    if (*pRefPic).uiShortRefCount[0] > 0 {
        iRet = SlidingWindow(pCtx, pRefPic);
    } else {
        let mut iLongTermFrameIdx = 0i32;
        let iMaxLongTermFrameIdx = (*pRefPic).iMaxLongTermFrameIdx;
        let iCurrLTRFrameIdx = GetLTRFrameIndex(pRefPic, (*pCtx).iFrameNumOfAuMarkedLtr);

        while ((*pRefPic).uiLongRefCount[0] >= num_ref_frames)
            && (iLongTermFrameIdx <= iMaxLongTermFrameIdx)
        {
            if iLongTermFrameIdx == iCurrLTRFrameIdx {
                iLongTermFrameIdx += 1;
                continue;
            }
            WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
            iLongTermFrameIdx += 1;
        }
    }

    if (*pRefPic).uiShortRefCount[0] + (*pRefPic).uiLongRefCount[0] >= num_ref_frames {
        WelsLog(
            &(*pCtx).sLogCtx,
            WELS_LOG_WARNING,
            "RemainOneBufferInDpbForEC(): empty one DPB failed for EC!",
        );
        iRet = ERR_INFO_REF_COUNT_OVERFLOW;
    }
    iRet
}

/// Detects missing IDR frames during error concealment and constructs a synthetic reference frame.
///
/// Matches `static int32_t WelsCheckAndRecoverForFutureDecoding (PWelsDecoderContext pCtx)`.
pub unsafe fn WelsCheckAndRecoverForFutureDecoding(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    if ((*pCtx).sRefPic.uiShortRefCount[LIST_0] + (*pCtx).sRefPic.uiLongRefCount[LIST_0] <= 0)
        && ((*pCtx).eSliceType != EWelsSliceType::I_SLICE && (*pCtx).eSliceType != EWelsSliceType::SI_SLICE)
    {
        let ec_mode = if !(*pCtx).pParam.is_null() {
            (*(*pCtx).pParam).eEcActiveIdc
        } else {
            crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
        };

        if ec_mode != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE {
            let pRef = crate::decoder::pic_queue::PrefetchPic((*pCtx).pPicBuff);
            if !pRef.is_null() {
                (*pRef).bIsComplete = false;
                if !(*pCtx).pSps.is_null() {
                    (*pRef).iSpsId = (*(*pCtx).pSps).iSpsId;
                }
                if !(*pCtx).pPps.is_null() {
                    (*pRef).iPpsId = (*(*pCtx).pPps).iPpsId;
                }
                if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
                    for list in 0..LIST_A {
                        for i in 0..MAX_DPB_COUNT {
                            (*pRef).pRefPic[list][i] = std::ptr::null_mut();
                        }
                    }
                }
                (*pCtx).iErrorCode |= dsDataErrorConcealed;

                let mut bCopyPrevious = false;
                let prev_pic = if !(*pCtx).pLastDecPicInfo.is_null() {
                    (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb
                } else {
                    std::ptr::null_mut()
                };

                if (ec_mode == ERROR_CON_FRAME_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
                    || ec_mode == ERROR_CON_SLICE_MV_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)
                    && !prev_pic.is_null()
                {
                    bCopyPrevious = (*pRef).iWidthInPixel == (*prev_pic).iWidthInPixel
                        && (*pRef).iHeightInPixel == (*prev_pic).iHeightInPixel;
                }

                if !bCopyPrevious {
                    if !(*pRef).data_ptr(0).is_null() {
                        std::ptr::write_bytes(
                            (*pRef).data_ptr(0),
                            128,
                            ((*pRef).linesize(0) * (*pRef).iHeightInPixel) as usize,
                        );
                    }
                    if !(*pRef).data_ptr(1).is_null() {
                        std::ptr::write_bytes(
                            (*pRef).data_ptr(1),
                            128,
                            ((*pRef).linesize(1) * (*pRef).iHeightInPixel / 2) as usize,
                        );
                    }
                    if !(*pRef).data_ptr(2).is_null() {
                        std::ptr::write_bytes(
                            (*pRef).data_ptr(2),
                            128,
                            ((*pRef).linesize(2) * (*pRef).iHeightInPixel / 2) as usize,
                        );
                    }
                } else if crate::decoder::picture::same_picture(pRef, prev_pic) {
                    WelsLog(
                        &(*pCtx).sLogCtx,
                        WELS_LOG_WARNING,
                        "WelsInitRefList()::EC memcpy overlap.",
                    );
                } else {
                    // S25: `pRef` and `prev_pic` are two slots of the same
                    // `SPicBuff.ppPic`, and the `pRef == prev_pic` arm above is what
                    // makes them provably distinct here — which is also what makes
                    // `copy_nonoverlapping` legal. Both are named through their raw
                    // pointers per use; the `let prev = &*prev_pic` binding that used
                    // to span this block was a borrow held across three writes into
                    // the other picture.
                    if !(*pRef).data_ptr(0).is_null() && !(*prev_pic).data_ptr(0).is_null() {
                        std::ptr::copy_nonoverlapping(
                            (*prev_pic).data_ptr(0),
                            (*pRef).data_ptr(0),
                            ((*pRef).linesize(0) * (*pRef).iHeightInPixel) as usize,
                        );
                    }
                    if !(*pRef).data_ptr(1).is_null() && !(*prev_pic).data_ptr(1).is_null() {
                        std::ptr::copy_nonoverlapping(
                            (*prev_pic).data_ptr(1),
                            (*pRef).data_ptr(1),
                            ((*pRef).linesize(1) * (*pRef).iHeightInPixel / 2) as usize,
                        );
                    }
                    if !(*pRef).data_ptr(2).is_null() && !(*prev_pic).data_ptr(2).is_null() {
                        std::ptr::copy_nonoverlapping(
                            (*prev_pic).data_ptr(2),
                            (*pRef).data_ptr(2),
                            ((*pRef).linesize(2) * (*pRef).iHeightInPixel / 2) as usize,
                        );
                    }
                }
                (*pRef).iFrameNum = 0;
                (*pRef).iFramePoc = 0;
                (*pRef).uiTemporalId = 0;
                (*pRef).uiQualityId = 0;
                (*pRef).eSliceType = (*pCtx).eSliceType;

                crate::common::expand_pic::ExpandReferencingPicture(
                    &[(*pRef).data_ptr(0), (*pRef).data_ptr(1), (*pRef).data_ptr(2)],
                    (*pRef).iWidthInPixel,
                    (*pRef).iHeightInPixel,
                    &[(*pRef).linesize(0), (*pRef).linesize(1), (*pRef).linesize(2)],
                );
                AddShortTermToList(&mut (*pCtx).sRefPic, pRef);
            } else {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_ERROR,
                    "WelsInitRefList()::PrefetchPic for EC errors.",
                );
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return ERR_INFO_REF_COUNT_OVERFLOW;
            }
        }
    }
    ERR_NONE
}

/// Processes an individual MMCO memory management command.
///
/// Matches `static int32_t MMCOProcess (...)` in `manage_dec_ref.cpp`.
pub unsafe fn MMCOProcess(
    pCtx: *mut SWelsDecoderContext,
    pRefPic: *mut SRefPic,
    uiMmcoType: u32,
    iShortFrameNum: i32,
    uiLongTermPicNum: u32,
    iLongTermFrameIdx: i32,
    iMaxLongTermFrameIdx: i32,
) -> i32 {
    if pCtx.is_null() || pRefPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    // S25 again, and this is the widest of the three: every arm below re-enters
    // through `pRefPic` (and `MMCO_RESET` through `pCtx`), `MMCO_SET_MAX_LONG`'s
    // loop *terminates* on a count those calls decrement, and the surviving arms
    // read the context after them.
    let mut iRet = ERR_NONE;

    match uiMmcoType {
        MMCO_SHORT2UNUSED => {
            let pPic = WelsDelShortFromListSetUnref(pRefPic, iShortFrameNum);
            if pPic.is_null() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_SHORT2UNUSED: delete an empty entry from short term list",
                );
            }
        }
        MMCO_LONG2UNUSED => {
            let pPic = WelsDelLongFromListSetUnref(pRefPic, uiLongTermPicNum);
            if pPic.is_null() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2UNUSED: delete an empty entry from long term list",
                );
            }
        }
        MMCO_SHORT2LONG => {
            if iLongTermFrameIdx > (*pRefPic).iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            let pPic = WelsDelShortFromList(pRefPic, iShortFrameNum);
            if pPic.is_null() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2LONG: delete an empty entry from short term list",
                );
            } else {
                WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
                (*pCtx).bCurAuContainLtrMarkSeFlag = true;
                (*pCtx).iFrameNumOfAuMarkedLtr = iShortFrameNum;
                MarkAsLongTerm(pRefPic, iShortFrameNum, iLongTermFrameIdx, uiLongTermPicNum);
            }
        }
        MMCO_SET_MAX_LONG => {
            (*pRefPic).iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
            let mut i = 0;
            while i < ((*pRefPic).uiLongRefCount[LIST_0] as usize) {
                let pCur = (*pRefPic).pLongRefList[LIST_0][i];
                if !pCur.is_null() && (*pCur).iLongTermFrameIdx > (*pRefPic).iMaxLongTermFrameIdx {
                    WelsDelLongFromListSetUnref(pRefPic, (*pCur).iLongTermFrameIdx as u32);
                } else {
                    i += 1;
                }
            }
        }
        MMCO_RESET => {
            WelsResetRefPic(pCtx);
            if !(*pCtx).pLastDecPicInfo.is_null() {
                (*(*pCtx).pLastDecPicInfo).bLastHasMmco5 = true;
            }
        }
        MMCO_LONG => {
            if iLongTermFrameIdx > (*pRefPic).iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
            let num_ref_frames = if !(*pCtx).pSps.is_null() {
                (*(*pCtx).pSps).iNumRefFrames as u8
            } else {
                1
            };
            if (*pRefPic).uiLongRefCount[LIST_0] + (*pRefPic).uiShortRefCount[LIST_0]
                >= num_ref_frames.max(1)
            {
                return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
            }
            (*pCtx).bCurAuContainLtrMarkSeFlag = true;
            (*pCtx).iFrameNumOfAuMarkedLtr = (*pCtx).iFrameNum;
            iRet = AddLongTermToList(pRefPic, (*pCtx).pDec, iLongTermFrameIdx, uiLongTermPicNum);
        }
        _ => {}
    }
    iRet
}

/// Executes all parsed MMCO memory management commands in sequence.
///
/// Matches `static int32_t MMCO (PWelsDecoderContext pCtx, PRefPic pRefPic, PRefPicMarking pRefPicMarking)`.
pub unsafe fn MMCO(
    pCtx: *mut SWelsDecoderContext,
    pRefPic: *mut SRefPic,
    pRefPicMarking: *mut SRefPicMarking,
) -> i32 {
    if pCtx.is_null() || pRefPic.is_null() || pRefPicMarking.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let marking = &*pRefPicMarking;

    let uiLog2MaxFrameNum = if !(*pCtx).pCurDqLayer.is_null()
        && !(*(*pCtx).pCurDqLayer).sLayerInfo.pSps.is_null()
    {
        (*(*(*pCtx).pCurDqLayer).sLayerInfo.pSps).uiLog2MaxFrameNum
    } else if !(*pCtx).pSps.is_null() {
        (*(*pCtx).pSps).uiLog2MaxFrameNum
    } else {
        4
    };

    let mut i = 0usize;
    while i < MAX_MMCO_COUNT && marking.sMmcoRef[i].uiMmcoType != MMCO_END {
        let uiMmcoType = marking.sMmcoRef[i].uiMmcoType;
        let iShortFrameNum =
            ((*pCtx).iFrameNum - marking.sMmcoRef[i].iDiffOfPicNum) & ((1i32 << uiLog2MaxFrameNum) - 1);
        let uiLongTermPicNum = marking.sMmcoRef[i].uiLongTermPicNum;
        let iLongTermFrameIdx = marking.sMmcoRef[i].iLongTermFrameIdx;
        let iMaxLongTermFrameIdx = marking.sMmcoRef[i].iMaxLongTermFrameIdx;

        if uiMmcoType > MMCO_LONG {
            return ERR_INFO_INVALID_MMCO_OPCODE_BASE;
        }
        let iRet = MMCOProcess(
            pCtx,
            pRefPic,
            uiMmcoType,
            iShortFrameNum,
            uiLongTermPicNum,
            iLongTermFrameIdx,
            iMaxLongTermFrameIdx,
        );
        if iRet != ERR_NONE {
            return iRet;
        }
        i += 1;
    }
    if i == MAX_MMCO_COUNT {
        return ERR_INFO_INVALID_MMCO_NUM;
    }
    ERR_NONE
}

/// Populates `pRefList[LIST_0]` for standard P-slices.
///
/// Matches `int32_t WelsInitRefList (PWelsDecoderContext pCtx, int32_t iPoc)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsInitRefList(pCtx: *mut SWelsDecoderContext, _iPoc: i32) -> i32 {
    let err = WelsCheckAndRecoverForFutureDecoding(pCtx);
    if err != ERR_NONE {
        return err;
    }
    WrapShortRefPicNum(pCtx);

    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    for i in 0..MAX_DPB_COUNT {
        (*pCtx).sRefPic.pRefList[LIST_0][i] = std::ptr::null_mut();
    }

    let mut iCount = 0usize;
    let short_count = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;
    for i in 0..short_count {
        if iCount < MAX_REF_PIC_COUNT {
            (*pCtx).sRefPic.pRefList[LIST_0][iCount] = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
            iCount += 1;
        }
    }

    let long_count = (*pCtx).sRefPic.uiLongRefCount[LIST_0] as usize;
    for i in 0..long_count {
        if iCount < MAX_REF_PIC_COUNT {
            (*pCtx).sRefPic.pRefList[LIST_0][iCount] = (*pCtx).sRefPic.pLongRefList[LIST_0][i];
            iCount += 1;
        }
    }
    (*pCtx).sRefPic.uiRefCount[LIST_0] = iCount as u8;
    ERR_NONE
}

/// Populates dual reference picture lists (`pRefList[0]` and `pRefList[1]`) for B-slices.
///
/// Matches `int32_t WelsInitBSliceRefList (PWelsDecoderContext pCtx, int32_t iPoc)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsInitBSliceRefList(pCtx: *mut SWelsDecoderContext, iPoc: i32) -> i32 {
    let err = WelsCheckAndRecoverForFutureDecoding(pCtx);
    if err != ERR_NONE {
        return err;
    }
    WrapShortRefPicNum(pCtx);

    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    for i in 0..MAX_DPB_COUNT {
        (*pCtx).sRefPic.pRefList[LIST_0][i] = std::ptr::null_mut();
        (*pCtx).sRefPic.pRefList[LIST_1][i] = std::ptr::null_mut();
    }

    let mut iLSCurrPocCount = 0usize;
    let mut iLTCurrPocCount = 0usize;
    let mut pLSCurrPocList0: [*mut SPicture; MAX_DPB_COUNT] = [std::ptr::null_mut(); MAX_DPB_COUNT];
    let mut pLTCurrPocList0: [*mut SPicture; MAX_DPB_COUNT] = [std::ptr::null_mut(); MAX_DPB_COUNT];

    let short_count = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;
    for i in 0..short_count {
        let pPic = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFramePoc < iPoc {
            pLSCurrPocList0[iLSCurrPocCount] = pPic;
            iLSCurrPocCount += 1;
        }
    }
    for i in (0..short_count).rev() {
        let pPic = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFramePoc > iPoc {
            pLTCurrPocList0[iLTCurrPocCount] = pPic;
            iLTCurrPocCount += 1;
        }
    }

    let long_count = (*pCtx).sRefPic.uiLongRefCount[LIST_0] as usize;
    if long_count > 1 {
        for i in 0..long_count {
            for j in (i + 1)..long_count {
                let pj = (*pCtx).sRefPic.pLongRefList[LIST_0][j];
                let pi = (*pCtx).sRefPic.pLongRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    (*pCtx).sRefPic.pLongRefList[LIST_0].swap(i, j);
                }
            }
        }
    }

    let iCurrPocCount = iLSCurrPocCount + iLTCurrPocCount;
    let mut iCount = 0usize;

    // LIST_0 assembly
    for i in 0..iLSCurrPocCount {
        (*pCtx).sRefPic.pRefList[LIST_0][iCount] = pLSCurrPocList0[i];
        iCount += 1;
    }
    if iLSCurrPocCount > 1 {
        for i in 0..iLSCurrPocCount {
            for j in (i + 1)..iLSCurrPocCount {
                let pj = (*pCtx).sRefPic.pRefList[LIST_0][j];
                let pi = (*pCtx).sRefPic.pRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc > (*pi).iFramePoc {
                    (*pCtx).sRefPic.pRefList[LIST_0].swap(i, j);
                }
            }
        }
    }
    for i in 0..iLTCurrPocCount {
        (*pCtx).sRefPic.pRefList[LIST_0][iCount] = pLTCurrPocList0[i];
        iCount += 1;
    }
    if iLTCurrPocCount > 1 {
        for i in iLSCurrPocCount..iCurrPocCount {
            for j in (i + 1)..iCurrPocCount {
                let pj = (*pCtx).sRefPic.pRefList[LIST_0][j];
                let pi = (*pCtx).sRefPic.pRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    (*pCtx).sRefPic.pRefList[LIST_0].swap(i, j);
                }
            }
        }
    }
    for i in 0..long_count {
        (*pCtx).sRefPic.pRefList[LIST_0][iCount] = (*pCtx).sRefPic.pLongRefList[LIST_0][i];
        iCount += 1;
    }
    (*pCtx).sRefPic.uiRefCount[LIST_0] = iCount as u8;

    // LIST_1 assembly
    iCount = 0;
    for i in 0..iLTCurrPocCount {
        (*pCtx).sRefPic.pRefList[LIST_1][iCount] = pLTCurrPocList0[i];
        iCount += 1;
    }
    if iLTCurrPocCount > 1 {
        for i in 0..iLTCurrPocCount {
            for j in (i + 1)..iLTCurrPocCount {
                let pj = (*pCtx).sRefPic.pRefList[LIST_1][j];
                let pi = (*pCtx).sRefPic.pRefList[LIST_1][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    (*pCtx).sRefPic.pRefList[LIST_1].swap(i, j);
                }
            }
        }
    }
    for i in 0..iLSCurrPocCount {
        (*pCtx).sRefPic.pRefList[LIST_1][iCount] = pLSCurrPocList0[i];
        iCount += 1;
    }
    if iLSCurrPocCount > 1 {
        for i in iLTCurrPocCount..iCurrPocCount {
            for j in (i + 1)..iCurrPocCount {
                let pj = (*pCtx).sRefPic.pRefList[LIST_1][j];
                let pi = (*pCtx).sRefPic.pRefList[LIST_1][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc > (*pi).iFramePoc {
                    (*pCtx).sRefPic.pRefList[LIST_1].swap(i, j);
                }
            }
        }
    }
    for i in 0..long_count {
        (*pCtx).sRefPic.pRefList[LIST_1][iCount] = (*pCtx).sRefPic.pLongRefList[LIST_0][i];
        iCount += 1;
    }
    (*pCtx).sRefPic.uiRefCount[LIST_1] = iCount as u8;

    ERR_NONE
}

/// Modifies the active reference picture lists based on parsed RPLR commands.
///
/// Matches `int32_t WelsReorderRefList (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsReorderRefList(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    if (*pCtx).eSliceType == I_SLICE || (*pCtx).eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    if (*pCtx).pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pCurDqLayer: *mut DqLayerState = (*pCtx).pCurDqLayer;
    let pRefPicListReorderSyn = (*pCurDqLayer).pRefPicListReordering;
    if pRefPicListReorderSyn.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let reorder_syn = &*pRefPicListReorderSyn;

    let pNalHeaderExt = std::ptr::addr_of!((*pCurDqLayer).sLayerInfo.sNalHeaderExt);
    let pSliceHeader =
        std::ptr::addr_of_mut!((*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader);
    if (*pSliceHeader).pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = &*((*pSliceHeader).pSps as *mut SSps);

    let list_count = if (*pCtx).eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..list_count {
        let iMaxRefIdx = ((*pCtx).iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
        let iRefCount = (*pSliceHeader).uiRefCount[listIdx] as i32;
        let mut iPredFrameNum = (*pSliceHeader).iFrameNum;
        let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
        let mut iReorderingIndex = 0usize;

        if iRefCount <= 0 {
            (*pCtx).iErrorCode = dsNoParamSets;
            return ERR_INFO_REFERENCE_PIC_LOST;
        }

        if reorder_syn.bRefPicListReorderingFlag[listIdx] {
            while iReorderingIndex <= iMaxRefIdx
                && reorder_syn.sReorderingSyn[listIdx][iReorderingIndex].uiReorderingOfPicNumsIdc != 3
            {
                let uiReorderingOfPicNumsIdc = reorder_syn.sReorderingSyn[listIdx][iReorderingIndex]
                    .uiReorderingOfPicNumsIdc;
                let mut found_i = -1isize;

                if uiReorderingOfPicNumsIdc < 2 {
                    let iAbsDiffPicNum = (reorder_syn.sReorderingSyn[listIdx][iReorderingIndex]
                        .uiAbsDiffPicNumMinus1
                        + 1) as i32;
                    if uiReorderingOfPicNumsIdc == 0 {
                        iPredFrameNum -= iAbsDiffPicNum;
                    } else {
                        iPredFrameNum += iAbsDiffPicNum;
                    }
                    iPredFrameNum &= iMaxPicNum - 1;

                    for i in (0..iMaxRefIdx).rev() {
                        let cur = (*pCtx).sRefPic.pRefList[listIdx][i];
                        if !cur.is_null() && (*cur).iFrameNum == iPredFrameNum && !(*cur).bIsLongRef
                        {
                            if (*pNalHeaderExt).uiQualityId == (*cur).uiQualityId
                                && (*pSliceHeader).iSpsId != (*cur).iSpsId
                            {
                                WelsLog(
                                    &(*pCtx).sLogCtx,
                                    WELS_LOG_WARNING,
                                    "WelsReorderRefList()::::BASE LAYER SPS mismatch",
                                );
                                (*pCtx).iErrorCode = dsNoParamSets;
                                return ERR_INFO_REFERENCE_PIC_LOST;
                            } else {
                                found_i = i as isize;
                                break;
                            }
                        }
                    }
                } else if uiReorderingOfPicNumsIdc == 2 {
                    let target_long = reorder_syn.sReorderingSyn[listIdx][iReorderingIndex]
                        .uiLongTermPicNum as i32;
                    for i in (0..iMaxRefIdx).rev() {
                        let cur = (*pCtx).sRefPic.pRefList[listIdx][i];
                        if !cur.is_null()
                            && (*cur).bIsLongRef
                            && (*cur).iLongTermFrameIdx == target_long
                        {
                            if (*pNalHeaderExt).uiQualityId == (*cur).uiQualityId
                                && (*pSliceHeader).iSpsId != (*cur).iSpsId
                            {
                                WelsLog(
                                    &(*pCtx).sLogCtx,
                                    WELS_LOG_WARNING,
                                    "WelsReorderRefList()::::BASE LAYER SPS mismatch",
                                );
                                (*pCtx).iErrorCode = dsNoParamSets;
                                return ERR_INFO_REFERENCE_PIC_LOST;
                            } else {
                                found_i = i as isize;
                                break;
                            }
                        }
                    }
                }

                if found_i < 0 {
                    return ERR_INFO_REFERENCE_PIC_LOST;
                }
                let i_idx = found_i as usize;
                let pPic = (*pCtx).sRefPic.pRefList[listIdx][i_idx];

                // Both arms shift the same span by one; only the length differs
                // (`manage_dec_ref.cpp` spells the memmove out twice).
                if i_idx != iReorderingIndex {
                    let move_len = if i_idx > iReorderingIndex {
                        i_idx - iReorderingIndex
                    } else {
                        iMaxRefIdx - iReorderingIndex
                    };
                    shift_dpb_entries(
                        &mut (*pCtx).sRefPic.pRefList[listIdx],
                        iReorderingIndex,
                        1 + iReorderingIndex,
                        move_len,
                    );
                }
                (*pCtx).sRefPic.pRefList[listIdx][iReorderingIndex] = pPic;
                iReorderingIndex += 1;
            }
        }
    }
    ERR_NONE
}

/// Alternative test implementation of reference picture list reordering.
///
/// Matches `int32_t WelsReorderRefList2 (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsReorderRefList2(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    if (*pCtx).eSliceType == I_SLICE || (*pCtx).eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    if (*pCtx).pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pCurDqLayer: *mut DqLayerState = (*pCtx).pCurDqLayer;
    let pRefPicListReorderSyn = (*pCurDqLayer).pRefPicListReordering;
    if pRefPicListReorderSyn.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let reorder_syn = &*pRefPicListReorderSyn;

    let pSliceHeader =
        std::ptr::addr_of_mut!((*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader);
    if (*pSliceHeader).pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = &*((*pSliceHeader).pSps as *mut SSps);

    let iShortRefCount = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;
    let iLongRefCount = (*pCtx).sRefPic.uiLongRefCount[LIST_0] as usize;
    let iMaxRefIdx = ((*pCtx).iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
    let iCurFrameNum = (*pSliceHeader).iFrameNum;
    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iListCount = if (*pCtx).eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..iListCount {
        let mut iCount = 0usize;
        let iRefCount = (*pSliceHeader).uiRefCount[listIdx] as usize;

        if reorder_syn.bRefPicListReorderingFlag[listIdx] {
            let mut iPredFrameNum = iCurFrameNum;
            let mut i = 0usize;
            while reorder_syn.sReorderingSyn[listIdx][i].uiReorderingOfPicNumsIdc != 3 {
                if iCount >= iMaxRefIdx {
                    break;
                }
                for j in (iCount + 1..=iRefCount).rev() {
                    (*pCtx).sRefPic.pRefList[listIdx][j] = (*pCtx).sRefPic.pRefList[listIdx][j - 1];
                }

                let uiReorderingOfPicNumsIdc =
                    reorder_syn.sReorderingSyn[listIdx][i].uiReorderingOfPicNumsIdc;
                if uiReorderingOfPicNumsIdc < 2 {
                    let iAbsDiffPicNum = (reorder_syn.sReorderingSyn[listIdx][i]
                        .uiAbsDiffPicNumMinus1
                        + 1) as i32;
                    if uiReorderingOfPicNumsIdc == 0 {
                        if iPredFrameNum - iAbsDiffPicNum < 0 {
                            iPredFrameNum -= iAbsDiffPicNum - iMaxPicNum;
                        } else {
                            iPredFrameNum -= iAbsDiffPicNum;
                        }
                    } else {
                        if iPredFrameNum + iAbsDiffPicNum >= iMaxPicNum {
                            iPredFrameNum += iAbsDiffPicNum - iMaxPicNum;
                        } else {
                            iPredFrameNum += iAbsDiffPicNum;
                        }
                    }
                    if iPredFrameNum > iCurFrameNum {
                        iPredFrameNum -= iMaxPicNum;
                    }
                    for j in 0..iShortRefCount {
                        let cur = (*pCtx).sRefPic.pShortRefList[LIST_0][j];
                        if !cur.is_null() && (*cur).iFrameWrapNum == iPredFrameNum {
                            (*pCtx).sRefPic.pRefList[listIdx][iCount] = cur;
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = (*pCtx).sRefPic.pRefList[listIdx][j];
                        if !cur.is_null()
                            && ((*cur).bIsLongRef || (*cur).iFrameWrapNum != iPredFrameNum)
                        {
                            (*pCtx).sRefPic.pRefList[listIdx][k_write] = cur;
                            k_write += 1;
                        }
                    }
                } else {
                    iPredFrameNum =
                        reorder_syn.sReorderingSyn[listIdx][i].uiLongTermPicNum as i32;
                    for j in 0..iLongRefCount {
                        let cur = (*pCtx).sRefPic.pLongRefList[LIST_0][j];
                        if !cur.is_null() && (*cur).uiLongTermPicNum == iPredFrameNum as u32 {
                            (*pCtx).sRefPic.pRefList[listIdx][iCount] = cur;
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = (*pCtx).sRefPic.pRefList[listIdx][j];
                        if !cur.is_null()
                            && (!(*cur).bIsLongRef
                                || (*cur).uiLongTermPicNum != iPredFrameNum as u32)
                        {
                            (*pCtx).sRefPic.pRefList[listIdx][k_write] = cur;
                            k_write += 1;
                        }
                    }
                }
                i += 1;
            }
        }

        let start_fill = (1usize).max(iCount.max((*pCtx).sRefPic.uiRefCount[listIdx] as usize));
        for i in start_fill..iRefCount {
            (*pCtx).sRefPic.pRefList[listIdx][i] = (*pCtx).sRefPic.pRefList[listIdx][i - 1];
        }
        (*pCtx).sRefPic.uiRefCount[listIdx] =
            (iCount.max((*pCtx).sRefPic.uiRefCount[listIdx] as usize)).min(iRefCount) as u8;
    }
    ERR_NONE
}

/// Commits the newly reconstructed picture into the reference picture buffer pool.
///
/// Matches `int32_t WelsMarkAsRef (PWelsDecoderContext pCtx, PPicture pLastDec)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsMarkAsRef(pCtx: *mut SWelsDecoderContext, pLastDec: *mut SPicture) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut isThreadCtx = true;
    let pDec = if !pLastDec.is_null() {
        pLastDec
    } else {
        isThreadCtx = false;
        (*pCtx).pDec
    };

    if pDec.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pRefPic: *mut SRefPic = if isThreadCtx {
        std::ptr::addr_of_mut!((*pCtx).sTmpRefPic)
    } else {
        std::ptr::addr_of_mut!((*pCtx).sRefPic)
    };

    if (*pCtx).pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurDqLayer: *mut DqLayerState = (*pCtx).pCurDqLayer;
    let pRefPicMarking = (*pCurDqLayer).pRefPicMarking;
    if pRefPicMarking.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    (*pDec).uiQualityId = (*pCurDqLayer).sLayerInfo.sNalHeaderExt.uiQualityId;
    (*pDec).uiTemporalId = (*pCurDqLayer).sLayerInfo.sNalHeaderExt.uiTemporalId;
    if !(*pCtx).pSps.is_null() {
        (*pDec).iSpsId = (*(*pCtx).pSps).iSpsId;
    }
    if !(*pCtx).pPps.is_null() {
        (*pDec).iPpsId = (*(*pCtx).pPps).iPpsId;
    }

    let mut bIsIDRAU = false;
    if !(*pCtx).pAccessUnitList.is_null() {
        let au = &*(*pCtx).pAccessUnitList;
        for j in au.uiStartPos..=au.uiEndPos {
            let pNal = *au.pNalUnitsList.add(j as usize);
            if !pNal.is_null() {
                let nal = &*pNal;
                if nal.sNalHeaderExt.sNalUnitHeader.eNalUnitType == NAL_UNIT_CODED_SLICE_IDR
                    || nal.sNalHeaderExt.bIdrFlag
                {
                    bIsIDRAU = true;
                    break;
                }
            }
        }
    }

    let mut iRet = ERR_NONE;
    if bIsIDRAU {
        if (*pRefPicMarking).bLongTermRefFlag {
            (*pRefPic).iMaxLongTermFrameIdx = 0;
            AddLongTermToList(pRefPic, pDec, 0, 0);
        } else {
            (*pRefPic).iMaxLongTermFrameIdx = -1;
        }
    } else {
        if (*pRefPicMarking).bAdaptiveRefPicMarkingModeFlag {
            iRet = MMCO(pCtx, pRefPic, pRefPicMarking);
            if iRet != ERR_NONE {
                let ec_mode = if !(*pCtx).pParam.is_null() {
                    (*(*pCtx).pParam).eEcActiveIdc
                } else {
                    ERROR_CON_DISABLE
                };
                if ec_mode != ERROR_CON_DISABLE {
                    iRet = RemainOneBufferInDpbForEC(pCtx, pRefPic);
                    if iRet != ERR_NONE {
                        return iRet;
                    }
                } else {
                    return iRet;
                }
            }
            if !(*pCtx).pLastDecPicInfo.is_null() && (*(*pCtx).pLastDecPicInfo).bLastHasMmco5 {
                (*pDec).iFrameNum = 0;
                (*pDec).iFramePoc = 0;
            }
        } else {
            iRet = SlidingWindow(pCtx, pRefPic);
            if iRet != ERR_NONE {
                let ec_mode = if !(*pCtx).pParam.is_null() {
                    (*(*pCtx).pParam).eEcActiveIdc
                } else {
                    ERROR_CON_DISABLE
                };
                if ec_mode != ERROR_CON_DISABLE {
                    iRet = RemainOneBufferInDpbForEC(pCtx, pRefPic);
                    if iRet != ERR_NONE {
                        return iRet;
                    }
                } else {
                    return iRet;
                }
            }
        }
    }

    if !(*pDec).bIsLongRef {
        let num_ref_frames = if !(*pCtx).pSps.is_null() {
            (*(*pCtx).pSps).iNumRefFrames as u8
        } else {
            1
        };
        if (*pRefPic).uiLongRefCount[LIST_0] + (*pRefPic).uiShortRefCount[LIST_0]
            >= num_ref_frames.max(1)
        {
            let ec_mode = if !(*pCtx).pParam.is_null() {
                (*(*pCtx).pParam).eEcActiveIdc
            } else {
                ERROR_CON_DISABLE
            };
            if ec_mode != ERROR_CON_DISABLE {
                iRet = RemainOneBufferInDpbForEC(pCtx, pRefPic);
                if iRet != ERR_NONE {
                    return iRet;
                }
            } else {
                return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
            }
        }
        iRet = AddShortTermToList(pRefPic, pDec);
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
    fn test_set_unref_resets_fields() {
        let mut pic = SPicture::default();
        pic.iRefCount = 0;
        pic.bUsedAsRef = true;
        pic.bIsLongRef = true;
        pic.iFrameNum = 42;

        unsafe {
            SetUnRef(&mut pic);
            assert!(!pic.bUsedAsRef);
            assert!(!pic.bIsLongRef);
            assert_eq!(pic.iFrameNum, -1);
            assert_eq!(pic.iFrameWrapNum, -1);
            assert_eq!(pic.iLongTermFrameIdx, -1);
        }
    }

    // T5.B2: these three tests each took `&mut picN as *mut _` a second time, in
    // the assertion, *after* the list already held a pointer to the same picture.
    // The second `&mut` is a Unique retag that pops the tag the list is holding,
    // so the next call reading `(*cur).iFrameNum` reads through a dead one — the
    // list is right, the test is what makes it UB. Miri could not say so while
    // `gates.sh` skipped this module for F13's production site (now closed
    // above), which is F18's shape a second time: **a skipped test is not a
    // passing test, and the backlog behind a skip is not only in the code the
    // skip was written for.** Each picture's address is taken once, before the
    // list is given it, and the assertions compare that value.

    #[test]
    fn test_add_short_term_to_list_and_delete() {
        let mut ref_pic = SRefPic::default();
        let mut pic1 = SPicture::default();
        pic1.iFrameNum = 10;
        let mut pic2 = SPicture::default();
        pic2.iFrameNum = 12;
        let p1: *mut SPicture = &mut pic1;
        let p2: *mut SPicture = &mut pic2;

        unsafe {
            let res1 = AddShortTermToList(&mut ref_pic, p1);
            assert_eq!(res1, ERR_NONE);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], p1);

            let res2 = AddShortTermToList(&mut ref_pic, p2);
            assert_eq!(res2, ERR_NONE);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 2);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], p2);
            assert_eq!(ref_pic.pShortRefList[LIST_0][1], p1);

            let deleted = WelsDelShortFromList(&mut ref_pic, 10);
            assert_eq!(deleted, p1);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], p2);
        }
    }

    #[test]
    fn test_add_long_term_sorted_order() {
        let mut ref_pic = SRefPic::default();
        let mut pic1 = SPicture::default();
        let mut pic2 = SPicture::default();
        let p1: *mut SPicture = &mut pic1;
        let p2: *mut SPicture = &mut pic2;

        unsafe {
            AddLongTermToList(&mut ref_pic, p1, 5, 5);
            AddLongTermToList(&mut ref_pic, p2, 2, 2);

            assert_eq!(ref_pic.uiLongRefCount[LIST_0], 2);
            assert_eq!(ref_pic.pLongRefList[LIST_0][0], p2);
            assert_eq!(ref_pic.pLongRefList[LIST_0][1], p1);
        }
    }

    #[test]
    fn test_wels_reset_ref_pic() {
        let mut ctx = SWelsDecoderContext::new_boxed();

        let mut pic = SPicture::default();
        pic.iFrameNum = 1;
        let p: *mut SPicture = &mut pic;
        unsafe {
            AddShortTermToList(&mut ctx.sRefPic, p);
            assert_eq!(ctx.sRefPic.uiShortRefCount[LIST_0], 1);
            WelsResetRefPic(&mut *ctx);
            assert_eq!(ctx.sRefPic.uiShortRefCount[LIST_0], 0);
            assert_eq!(ctx.sRefPic.pShortRefList[LIST_0][0], std::ptr::null_mut());
        }
    }
}
