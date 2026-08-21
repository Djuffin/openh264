#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

//! Reference picture buffer management, list construction, reordering, and DPB lifecycle.
//!
//! Translated from `codec/decoder/core/inc/manage_dec_ref.h` and `codec/decoder/core/src/manage_dec_ref.cpp`.

#![deny(unsafe_code)]
// **Phase 5, T5.AC5 — the lint, allowing nothing.** The module's four raw
// pointers were four different things and none of them was a pointer:
//
//   * `SetUnRef`'s `PPicture` was a **callback type** — the field that stores it
//     is the only reason it could not be a borrow (T5.AC1).
//   * `MMCO`'s `PRefPicMarking` was the **layer's alias into the NAL's slice
//     header**, which the layer had already copied whole (T5.AC2).
//   * `WelsMarkAsRef`'s `pLastDec` and its `pDec` local were **one binding
//     unifying two arms** across `pCtx` re-entry; re-derived per use (T5.AC3).
//   * `pCtx->pParam` / `pCtx->pLastDecPicInfo` were the **api-owned aliases**,
//     which reach this module through `decoder_context`'s two accessors now
//     (T5.AC4) — the enumerated exception is there, not here.
//
// What was left after those was the EC prefetch bracket, and it is the
// concealment maneuver's fourth application: `pic_and_refs_mut` +
// `PicRefs::classify`, where the overlap guard the copy needed becomes
// `RefSlot::Current` instead of a comparison of two addresses (T5.AC5).

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

pub use crate::decoder::decoder_context::{Picture, SPicture};


pub use crate::decoder::decoder_context::SRefPic;
use crate::decoder::decoder_context::{active_pps, active_sps, pic_and_refs_mut, pps_of, ref_set, sps_of};
use crate::decoder::pic_queue::RefSlot;
use crate::decoder::decoder_context::ec_active_idc;
pub use crate::decoder::slice::{SRefPicListReorderSyn, SRefPicMarking};


pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt};


pub use crate::decoder::decoder_context::SLogContext;


pub use crate::decoder::decoder_context::SWelsDecoderContext;
use crate::decoder::decoder_context::{
    cur_au, dec_pic, long_ref_pic, pic_and_refs, pic_pool_mut, pool_pic, pool_pic_mut,
    prev_dpb_id, ref_pic, short_ref_pic, short_ref_pic_mut,
};
pub use crate::decoder::pic_queue::PicId;


// ============================================================================
// Internal Logging & Picture Helpers
// ============================================================================

// **T5.AA4: `insert_ref` is deleted, and the invariant it asserted is now the
// type's.** It answered "what handle does this picture go into a reference list
// as?" for a `*mut SPicture`, with a `debug_assert!` (T5.N4's, moved here at
// T5.P′2) that the picture had a pool slot at all — the check that a list entry
// cannot be a picture the pool does not own. The two doors into the lists take
// `Option<PicId>` now, so a caller has nothing else to pass: the assertion is
// discharged by the parameter type rather than at run time in one profile.

#[inline(always)]
pub fn WelsLog(_pLogCtx: &SLogContext, _iLevel: i32, _msg: &str) {

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
fn shift_dpb_entries(list: &mut [Option<PicId>], src: usize, dst: usize, len: usize) {
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
///
/// **T5.AC1: the picture arrives as a borrow.** The parameter was `*mut SPicture`
/// because the field that stores this function — `SPicture::pSetUnRef` — is a
/// callback type, and the C++ spells a callback's parameter as a pointer. Every
/// one of the seven call sites in this module already holds the picture as an
/// `&mut` out of `pool_pic_mut`, so the null test at the top ran on a pointer no
/// caller could make null; the two indirect sites (`api/codec_api.rs`'s pool
/// release and the reinstall below) each guard before they call. The test is the
/// parameter type now, and this function is safe.
pub extern "C" fn SetUnRef(ref_pic: &mut SPicture) {
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
                ref_pic.pRefPic[list][i] = None;
            }
        }
    } else {
        ref_pic.pSetUnRef = Some(SetUnRef);
    }
}

/// Flushes all reference pictures from short-term and long-term lists and invokes `SetUnRef`.
///
/// Matches `void WelsResetRefPic (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub fn WelsResetRefPic(pCtx: &mut SWelsDecoderContext) {
    let bTmpRefSet = false;
    ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] = 0;
    ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] = 0;

    ref_set(pCtx, bTmpRefSet).uiRefCount[LIST_0] = 0;
    ref_set(pCtx, bTmpRefSet).uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        let entry = ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][i];
        if let Some(pPic) = pool_pic_mut(&mut pCtx.pPicBuff, entry) {
            SetUnRef(pPic);
            ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][i] = None;
        }
    }
    ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        let entry = ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i];
        if let Some(pPic) = pool_pic_mut(&mut pCtx.pPicBuff, entry) {
            SetUnRef(pPic);
            ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i] = None;
        }
    }
    ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] = 0;
}

/// Clears reference list pointers and counts without invoking `SetUnRef`.
///
/// Matches `void WelsResetRefPicWithoutUnRef (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub fn WelsResetRefPicWithoutUnRef(pCtx: &mut SWelsDecoderContext) {
    let bTmpRefSet = false;
    ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] = 0;
    ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] = 0;

    ref_set(pCtx, bTmpRefSet).uiRefCount[LIST_0] = 0;
    ref_set(pCtx, bTmpRefSet).uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][i] = None;
    }
    ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i] = None;
    }
    ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] = 0;
}

/// Deletes a short-term reference picture with `iFrameNum` from `pShortRefList[0]`.
///
/// Matches `static PPicture WelsDelShortFromList (PRefPic pRefPic, int32_t iFrameNum)`.
pub fn WelsDelShortFromList(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    iFrameNum: i32,
) -> Option<PicId> {
    let count = ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] as usize;

    for i in 0..count {
        let slot = ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][i];
        // The mark and the list surgery are two disjoint fields — the picture is
        // the pool's, the list is `pRefPic`'s — so the borrow ends at the mark and
        // the **handle** is what travels out (T5.Z1).
        let matched = match pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
            Some(pic) if pic.iFrameNum == iFrameNum => {
                pic.bUsedAsRef = false;
                true
            }
            _ => false,
        };
        if matched {
            let iMoveSize = count - i - 1;
            let ref_pic = ref_set(pCtx, bTmpRefSet);
            ref_pic.pShortRefList[LIST_0][i] = None;

            if iMoveSize > 0 {
                shift_dpb_entries(&mut ref_pic.pShortRefList[LIST_0], i + 1, i, iMoveSize);
            }
            ref_pic.uiShortRefCount[LIST_0] -= 1;
            let new_count = ref_pic.uiShortRefCount[LIST_0] as usize;
            ref_pic.pShortRefList[LIST_0][new_count] = None;
            return slot;
        }
    }
    None
}

/// Deletes a short-term reference picture and immediately calls `SetUnRef`.
pub fn WelsDelShortFromListSetUnref(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    iFrameNum: i32,
) -> Option<PicId> {
    let slot = WelsDelShortFromList(pCtx, bTmpRefSet, iFrameNum);
    if let Some(pPic) = pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
        SetUnRef(pPic);
    }
    slot
}

/// Deletes a long-term reference picture with `uiLongTermFrameIdx` from `pLongRefList[0]`.
///
/// Matches `static PPicture WelsDelLongFromList (PRefPic pRefPic, uint32_t uiLongTermFrameIdx)`.
pub fn WelsDelLongFromList(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    uiLongTermFrameIdx: u32,
) -> Option<PicId> {
    let count = ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] as usize;

    for i in 0..count {
        let slot = ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i];
        // `WelsDelShortFromList`'s shape — the handle travels out (T5.Z1).
        let matched = match pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
            Some(pic) if pic.iLongTermFrameIdx == uiLongTermFrameIdx as i32 => {
                pic.bUsedAsRef = false;
                pic.bIsLongRef = false;
                true
            }
            _ => false,
        };
        if matched {
            let iMoveSize = count - i - 1;
            let ref_pic = ref_set(pCtx, bTmpRefSet);

            if iMoveSize > 0 {
                shift_dpb_entries(&mut ref_pic.pLongRefList[LIST_0], i + 1, i, iMoveSize);
            }
            ref_pic.uiLongRefCount[LIST_0] -= 1;
            let new_count = ref_pic.uiLongRefCount[LIST_0] as usize;
            ref_pic.pLongRefList[LIST_0][new_count] = None;
            return slot;
        }
    }
    None
}

/// Deletes a long-term reference picture and immediately calls `SetUnRef`.
pub fn WelsDelLongFromListSetUnref(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    uiLongTermFrameIdx: u32,
) -> Option<PicId> {
    let slot = WelsDelLongFromList(pCtx, bTmpRefSet, uiLongTermFrameIdx);
    if let Some(pPic) = pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
        SetUnRef(pPic);
    }
    slot
}

/// Inserts a decoded picture at index 0 of `pShortRefList[0]`.
///
/// Matches `static int32_t AddShortTermToList (PRefPic pRefPic, PPicture pPic)`.
pub fn AddShortTermToList(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    slot: Option<PicId>,
) -> i32 {
    // **T5.AA4: the handle travels, not the picture.** The parameter was a
    // `*mut SPicture` the caller derived from `pCtx.pPicBuff` and passed *beside*
    // `pCtx` — the layer bracket's shape one container over, and the phase's own
    // rule says the answer before the compiler does: aliases become ids. Everything
    // this function did with the picture was name its slot and write four fields,
    // and both are reachable from the handle, one borrow at a time.
    //
    // The ordering note that stood here — take the slot before the borrow, because
    // naming the picture's own slot under a live `&mut` over the same allocation
    // pops it — is discharged by construction: there is no second path to the
    // picture left to order against.
    let Some(_) = slot else {
        return ERR_INFO_INVALID_PTR;
    };
    let Some(pic) = pool_pic_mut(&mut pCtx.pPicBuff, slot) else {
        return ERR_INFO_INVALID_PTR;
    };
    pic.bUsedAsRef = true;
    pic.bIsLongRef = false;
    pic.iLongTermFrameIdx = -1;
    // The comparison value out from under the borrow: the scan below resolves other
    // slots of the same pool, and this one's own slot is exactly what it is looking
    // for (it is the duplicate-frame-num test).
    let iPicFrameNum = pic.iFrameNum;

    // **T5.Z4: the reference set is re-acquired per use, not held.** The scan below
    // alternates between the set (a context field) and the pool (another), so a
    // borrow of either held across the other is the shape this face removes. Each
    // `ref_set` call ends with its expression — S25's fix, and the reason the
    // selector is what travels in the signature.
    let short_count = ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0] as usize;

    if short_count > 0 {
        for iPos in 0..short_count {
            let entry = ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][iPos];
            let Some(iCurFrameNum) = pool_pic(&pCtx.pPicBuff, entry).map(|p| p.iFrameNum) else {
                return ERR_INFO_INVALID_PTR;
            };
            if iPicFrameNum == iCurFrameNum {
                ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][iPos] = slot;
                return ERR_INFO_DUPLICATE_FRAME_NUM;
            }
        }
        let ref_pic = ref_set(pCtx, bTmpRefSet);
        shift_dpb_entries(&mut ref_pic.pShortRefList[LIST_0], 0, 1, short_count);
    }
    let ref_pic = ref_set(pCtx, bTmpRefSet);
    ref_pic.pShortRefList[LIST_0][0] = slot;
    ref_pic.uiShortRefCount[LIST_0] += 1;
    ERR_NONE
}

/// Inserts a decoded picture into `pLongRefList[0]`, keeping it sorted in ascending order of `iLongTermFrameIdx`.
///
/// Matches `static int32_t AddLongTermToList (PRefPic pRefPic, PPicture pPic, int32_t iLongTermFrameIdx, uint32_t uiLongTermPicNum)`.
pub fn AddLongTermToList(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    slot: Option<PicId>,
    iLongTermFrameIdx: i32,
    uiLongTermPicNum: u32,
) -> i32 {
    // The handle travels — see `AddShortTermToList` (T5.AA4).
    let Some(_) = slot else {
        return ERR_INFO_INVALID_PTR;
    };
    let Some(pic) = pool_pic_mut(&mut pCtx.pPicBuff, slot) else {
        return ERR_INFO_INVALID_PTR;
    };
    pic.bUsedAsRef = true;
    pic.bIsLongRef = true;
    pic.iLongTermFrameIdx = iLongTermFrameIdx;
    pic.uiLongTermPicNum = uiLongTermPicNum;
    // The comparison value out from under the borrow, as in `AddShortTermToList`.
    let iPicLongTermFrameIdx = pic.iLongTermFrameIdx;

    // The set is re-acquired per use — see `AddShortTermToList` (T5.Z4).
    let long_count = ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] as usize;

    if long_count == 0 {
        ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][0] = slot;
    } else {
        let mut insert_idx = long_count.min(MAX_REF_PIC_COUNT);
        for i in 0..insert_idx {
            let entry = ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i];
            let Some(iCurLongTermFrameIdx) =
                pool_pic(&pCtx.pPicBuff, entry).map(|p| p.iLongTermFrameIdx)
            else {
                return ERR_INFO_INVALID_PTR;
            };
            if iCurLongTermFrameIdx > iPicLongTermFrameIdx {
                insert_idx = i;
                break;
            }
        }
        let move_count = long_count - insert_idx;
        let ref_pic = ref_set(pCtx, bTmpRefSet);
        if move_count > 0 {
            shift_dpb_entries(
                &mut ref_pic.pLongRefList[LIST_0],
                insert_idx,
                insert_idx + 1,
                move_count,
            );
        }
        ref_pic.pLongRefList[LIST_0][insert_idx] = slot;
    }

    let ref_pic = ref_set(pCtx, bTmpRefSet);
    if (ref_pic.uiLongRefCount[LIST_0] as usize) < MAX_REF_PIC_COUNT {
        ref_pic.uiLongRefCount[LIST_0] += 1;
    }
    ERR_NONE
}

/// Converts a short-term reference picture to a long-term reference picture.
pub fn MarkAsLongTerm(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    iFrameNum: i32,
    iLongTermFrameIdx: i32,
    uiLongTermPicNum: u32,
) -> i32 {
    let _ = WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, iLongTermFrameIdx as u32);
    let mut iRet = ERR_NONE;
    let count = ref_set(pCtx, bTmpRefSet).uiRefCount[LIST_0] as usize;

    for i in 0..count {
        let slot = ref_set(pCtx, bTmpRefSet).pRefList[LIST_0][i];
        let matched = pool_pic(&(*pCtx).pPicBuff, slot)
            .is_some_and(|p| p.iFrameNum == iFrameNum && !p.bIsLongRef);
        if matched {
            // T5.AA4: the handle the scan already found, not a pointer resolved from it.
            iRet = AddLongTermToList(pCtx, bTmpRefSet, slot, iLongTermFrameIdx, uiLongTermPicNum);
            break;
        }
    }
    iRet
}

/// Locates the long-term frame index corresponding to `iAncLTRFrameNum`.
pub fn GetLTRFrameIndex(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    iAncLTRFrameNum: i32,
) -> i32 {
    let long_count = ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] as usize;
    for i in 0..long_count {
        let entry = ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i];
        if let Some(pPic) = pool_pic(&pCtx.pPicBuff, entry) {
            if pPic.iFrameNum == iAncLTRFrameNum {
                return pPic.iLongTermFrameIdx;
            }
        }
    }
    -1
}

/// Evaluates short-term frame number wrapping modulo `1 << uiLog2MaxFrameNum`.
///
/// Matches `static void WrapShortRefPicNum (PWelsDecoderContext pCtx)`.
pub fn WrapShortRefPicNum(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    // The `sps_ref.is_none()` guard that stood here is the lookup's own `else` arm
    // now — and it covers the out-of-range id the old `&*sps_of(…)` dereferenced
    // (T5.Z1).
    let Some(pSps) = sps_of(
        &(*pCtx).sSpsPpsCtx,
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref,
    ) else {
        return;
    };

    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iShortRefCount = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;

    // The slice's frame number is read once, above the loop: it is the NAL's and
    // the pictures are the pool's, so nothing here holds two borrows (T5.Z1).
    let iSliceFrameNum =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFrameNum;
    for i in 0..iShortRefCount {
        let slot = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
        if let Some(pPic) = pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
            pPic.iFrameWrapNum = if pPic.iFrameNum > iSliceFrameNum {
                pPic.iFrameNum - iMaxPicNum
            } else {
                pPic.iFrameNum
            };
        }
    }
}

/// Evicts the oldest short-term reference picture when DPB reaches capacity.
///
/// Matches `static int32_t SlidingWindow (PWelsDecoderContext pCtx, PRefPic pRefPic)`.
pub fn SlidingWindow(pCtx: &mut SWelsDecoderContext, bTmpRefSet: bool) -> i32 {
    // S25: no borrow of `*pCtx` or `*pRefPic` outlives one expression here. Both
    // `WelsDelShortFromList` and `SetUnRef` re-enter through the raw pointers this
    // function was handed, so a `let ref_pic = &mut *pRefPic` held across them is
    // the shape the rule names — invalidated by the callee's own `&mut`, and read
    // afterwards. See the module note above `SetUnRef`.
    let num_ref_frames = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
        .map_or(1, |sps| sps.iNumRefFrames as u8);

    let counts = ref_set(pCtx, bTmpRefSet);
    let (uiShort, uiLong) = (counts.uiShortRefCount[LIST_0], counts.uiLongRefCount[LIST_0]);
    if uiShort + uiLong >= num_ref_frames {
        if uiShort == 0 {
            WelsLog(
                &(*pCtx).sLogCtx,
                WELS_LOG_ERROR,
                "No reference picture in short term list when sliding window",
            );
            return ERR_INFO_INVALID_MMCO_REF_NUM_NOT_ENOUGH;
        }
        let short_count = uiShort as isize;
        for i in (0..short_count).rev() {
            let entry = ref_set(pCtx, bTmpRefSet).pShortRefList[LIST_0][i as usize];
            let iCurFrameNum = pool_pic(&pCtx.pPicBuff, entry).map(|p| p.iFrameNum);
            if let Some(iCurFrameNum) = iCurFrameNum {
                let slot = WelsDelShortFromList(pCtx, bTmpRefSet, iCurFrameNum);
                match pool_pic_mut(&mut (*pCtx).pPicBuff, slot) {
                    Some(pPic) => {
                        SetUnRef(pPic);
                        break;
                    }
                    None => return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW,
                }
            }
        }
    }
    ERR_NONE
}

/// Ensures at least 1 free slot in the DPB for error concealment operations.
///
/// Matches `static int32_t RemainOneBufferInDpbForEC (PWelsDecoderContext pCtx, PRefPic pRefPic)`.
pub fn RemainOneBufferInDpbForEC(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
) -> i32 {
    // S25, as in `SlidingWindow`: the loop below *depends* on a re-entrant call
    // changing `uiLongRefCount`, so its condition has to read the live field and
    // not a borrow the call invalidated.
    let num_ref_frames = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
        .map_or(1, |sps| sps.iNumRefFrames as u8);

    if ref_set(pCtx, bTmpRefSet).uiShortRefCount[0] + ref_set(pCtx, bTmpRefSet).uiLongRefCount[0] < num_ref_frames {
        return ERR_NONE;
    }

    let mut iRet = ERR_NONE;
    if ref_set(pCtx, bTmpRefSet).uiShortRefCount[0] > 0 {
        iRet = SlidingWindow(pCtx, bTmpRefSet);
    } else {
        let mut iLongTermFrameIdx = 0i32;
        let iMaxLongTermFrameIdx = ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx;
        let iCurrLTRFrameIdx = GetLTRFrameIndex(pCtx, bTmpRefSet, (*pCtx).iFrameNumOfAuMarkedLtr);

        while (ref_set(pCtx, bTmpRefSet).uiLongRefCount[0] >= num_ref_frames)
            && (iLongTermFrameIdx <= iMaxLongTermFrameIdx)
        {
            if iLongTermFrameIdx == iCurrLTRFrameIdx {
                iLongTermFrameIdx += 1;
                continue;
            }
            WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, iLongTermFrameIdx as u32);
            iLongTermFrameIdx += 1;
        }
    }

    if ref_set(pCtx, bTmpRefSet).uiShortRefCount[0] + ref_set(pCtx, bTmpRefSet).uiLongRefCount[0] >= num_ref_frames {
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
pub fn WelsCheckAndRecoverForFutureDecoding(pCtx: &mut SWelsDecoderContext) -> i32 {

    if ((*pCtx).sRefPic.uiShortRefCount[LIST_0] + (*pCtx).sRefPic.uiLongRefCount[LIST_0] <= 0)
        && ((*pCtx).eSliceType != EWelsSliceType::I_SLICE && (*pCtx).eSliceType != EWelsSliceType::SI_SLICE)
    {
        let ec_mode = ec_active_idc(&(*pCtx).pParam);

        if ec_mode != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE {
            // **The EC prefetch bracket** (T5.Q2). The region below writes into the
            // slot the prefetch just took and reads the previous DPB picture out of
            // another one, and the "are they the same slot?" guard sits in the middle
            // of it — so both halves have to come out of one borrow, or the guard
            // would be deciding a question the second derivation had already answered
            // by invalidating the first.
            // The two ids are read before the bracket opens: below it the pool is
            // borrowed and the parameter sets cannot be reached through the same
            // context (T5.Z1).
            let sps_id = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps).map(|s| s.iSpsId);
            let pps_id = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps).map(|p| p.iPpsId);
            let ec_slot = match pic_pool_mut(pCtx) {
                Some(pool) => pool.prefetch_free(),
                None => None,
            };
            // **T5.AC5 — the concealment bracket's fourth application** (the slice at
            // Y, the pool at Q, the layer at AA1, `DoErrorConFrameCopy` at AB3). One
            // borrow of the pool, split into the EC picture being built and a view of
            // the rest; the previous DPB picture is *classified* rather than compared,
            // so the `pic_slot(pRef) == pic_slot(prev_pic)` overlap test below is
            // `RefSlot::Current` and its arm cannot also hold a second derivation of
            // the same slot (F42's shape, and why the guard had to come out of one
            // borrow).
            let eSliceType = (*pCtx).eSliceType;
            let prev = prev_dpb_id(&pCtx.pLastDecPicInfo);
            let (pRef, pRefs) = pic_and_refs_mut(&mut pCtx.pPicBuff, ec_slot);
            if let Some(pRef) = pRef {
                pRef.bIsComplete = false;
                if let Some(iSpsId) = sps_id {
                    pRef.iSpsId = iSpsId;
                }
                if let Some(iPpsId) = pps_id {
                    pRef.iPpsId = iPpsId;
                }
                if eSliceType == EWelsSliceType::B_SLICE {
                    for list in 0..LIST_A {
                        for i in 0..MAX_DPB_COUNT {
                            pRef.pRefPic[list][i] = None;
                        }
                    }
                }

                (*pCtx).iErrorCode |= dsDataErrorConcealed;

                let bCrossIdr = ec_mode == ERROR_CON_FRAME_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
                    || ec_mode == ERROR_CON_SLICE_MV_COPY_CROSS_IDR
                    || ec_mode == ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE;

                // The three arms are the pointer form's three, unchanged: gray-fill
                // when there is nothing to copy from, log-and-skip when the source
                // *is* the destination (`Current` — and the dimension test the old
                // form ran there was trivially true), copy otherwise. A `Current`
                // slot outside the cross-IDR modes gray-fills, exactly as a null
                // `prev_pic` did.
                let spans = |pic: &SPicture| {
                    [
                        (0usize, (pic.linesize(0) * pic.iHeightInPixel) as usize),
                        (1, (pic.linesize(1) * pic.iHeightInPixel / 2) as usize),
                        (2, (pic.linesize(2) * pic.iHeightInPixel / 2) as usize),
                    ]
                };
                match pRefs.classify(prev) {
                    RefSlot::Other(prev_pic)
                        if bCrossIdr
                            && pRef.iWidthInPixel == prev_pic.iWidthInPixel
                            && pRef.iHeightInPixel == prev_pic.iHeightInPixel =>
                    {
                        // Disjoint by the classification rather than by a comparison
                        // the compiler cannot see — which is what `copy_nonoverlapping`
                        // needed an argument for (S25).
                        for (i, len) in spans(pRef) {
                            if pRef.plane(i).is_empty() || prev_pic.plane(i).is_empty() {
                                continue;
                            }
                            let (src_base, dst_base) =
                                (prev_pic.plane(i).origin(), pRef.plane(i).origin());
                            let src = &prev_pic.plane(i).as_slice()[src_base..src_base + len];
                            pRef.plane_mut(i).as_mut_slice()[dst_base..dst_base + len]
                                .copy_from_slice(src);
                        }
                    }
                    RefSlot::Current if bCrossIdr => {
                        WelsLog(
                            &(*pCtx).sLogCtx,
                            WELS_LOG_WARNING,
                            "WelsInitRefList()::EC memcpy overlap.",
                        );
                    }
                    _ => {
                        for (i, len) in spans(pRef) {
                            let plane = pRef.plane_mut(i);
                            if plane.is_empty() {
                                continue;
                            }
                            let base = plane.origin();
                            plane.as_mut_slice()[base..base + len].fill(128);
                        }
                    }
                }
                pRef.iFrameNum = 0;
                pRef.iFramePoc = 0;
                pRef.uiTemporalId = 0;
                pRef.uiQualityId = 0;
                pRef.eSliceType = eSliceType;

                pRef.expand_as_reference();
                AddShortTermToList(pCtx, false, ec_slot);
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
pub fn MMCOProcess(
    pCtx: &mut SWelsDecoderContext,
    bTmpRefSet: bool,
    uiMmcoType: u32,
    iShortFrameNum: i32,
    uiLongTermPicNum: u32,
    iLongTermFrameIdx: i32,
    iMaxLongTermFrameIdx: i32,
) -> i32 {
    // S25 again, and this is the widest of the three: every arm below re-enters
    // through `pRefPic` (and `MMCO_RESET` through `pCtx`), `MMCO_SET_MAX_LONG`'s
    // loop *terminates* on a count those calls decrement, and the surviving arms
    // read the context after them.
    let mut iRet = ERR_NONE;

    match uiMmcoType {
        MMCO_SHORT2UNUSED => {
            if WelsDelShortFromListSetUnref(pCtx, bTmpRefSet, iShortFrameNum).is_none() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_SHORT2UNUSED: delete an empty entry from short term list",
                );
            }
        }
        MMCO_LONG2UNUSED => {
            if WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, uiLongTermPicNum).is_none() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2UNUSED: delete an empty entry from long term list",
                );
            }
        }
        MMCO_SHORT2LONG => {
            if iLongTermFrameIdx > ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            if WelsDelShortFromList(pCtx, bTmpRefSet, iShortFrameNum).is_none() {
                WelsLog(
                    &(*pCtx).sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2LONG: delete an empty entry from short term list",
                );
            } else {
                WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, iLongTermFrameIdx as u32);
                (*pCtx).bCurAuContainLtrMarkSeFlag = true;
                (*pCtx).iFrameNumOfAuMarkedLtr = iShortFrameNum;
                MarkAsLongTerm(pCtx, bTmpRefSet, iShortFrameNum, iLongTermFrameIdx, uiLongTermPicNum);
            }
        }
        MMCO_SET_MAX_LONG => {
            ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
            let mut i = 0;
            while i < (ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] as usize) {
                let entry = ref_set(pCtx, bTmpRefSet).pLongRefList[LIST_0][i];
                let iCurLongTermFrameIdx =
                    pool_pic(&pCtx.pPicBuff, entry).map(|p| p.iLongTermFrameIdx);
                match iCurLongTermFrameIdx {
                    Some(idx) if idx > ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx => {
                        WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, idx as u32);
                    }
                    _ => i += 1,
                }
            }
        }
        MMCO_RESET => {
            WelsResetRefPic(pCtx);
            {
                let last = &mut (*pCtx).pLastDecPicInfo;
                last.bLastHasMmco5 = true;
            }
        }
        MMCO_LONG => {
            if iLongTermFrameIdx > ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            WelsDelLongFromListSetUnref(pCtx, bTmpRefSet, iLongTermFrameIdx as u32);
            let num_ref_frames = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                .map_or(1, |sps| sps.iNumRefFrames as u8);
            if ref_set(pCtx, bTmpRefSet).uiLongRefCount[LIST_0] + ref_set(pCtx, bTmpRefSet).uiShortRefCount[LIST_0]
                >= num_ref_frames.max(1)
            {
                return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
            }
            (*pCtx).bCurAuContainLtrMarkSeFlag = true;
            (*pCtx).iFrameNumOfAuMarkedLtr = (*pCtx).iFrameNum;
            iRet = AddLongTermToList(
                pCtx,
                bTmpRefSet,
                (*pCtx).pDec,
                iLongTermFrameIdx,
                uiLongTermPicNum,
            );
        }
        _ => {}
    }
    iRet
}

/// Executes all parsed MMCO memory management commands in sequence.
///
/// Matches `static int32_t MMCO (PWelsDecoderContext pCtx, PRefPic pRefPic, PRefPicMarking pRefPicMarking)`.
pub fn MMCO(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&DqLayerState>,
    bTmpRefSet: bool,
    marking: &SRefPicMarking,
) -> i32 {
    // T5.AC2: the marking arrives as a borrow of the layer's own copy, and the
    // layer arrives shared beside it — this function only reads it, and the two
    // borrows have to coexist at the one call site (`WelsMarkAsRef`). The null
    // test the C++ needs is discharged by the parameter type.

    // A scalar, not a borrow — the MMCO loop below re-enters through `pCtx` on
    // every command (T5.Z1).
    let ps = &(*pCtx).sSpsPpsCtx;
    let uiLog2MaxFrameNum = pCurDqLayer
        .and_then(|layer| sps_of(ps, layer.sLayerInfo.sps_ref))
        .or_else(|| active_sps(ps, (*pCtx).active_sps))
        .map_or(4, |sps| sps.uiLog2MaxFrameNum);

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
            bTmpRefSet,
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
pub fn WelsInitRefList(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>, _iPoc: i32) -> i32 {
    let err = WelsCheckAndRecoverForFutureDecoding(pCtx);
    if err != ERR_NONE {
        return err;
    }
    WrapShortRefPicNum(pCtx, pCurDqLayer);


    for i in 0..MAX_DPB_COUNT {
        (*pCtx).sRefPic.pRefList[LIST_0][i] = None;
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
pub fn WelsInitBSliceRefList(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>, iPoc: i32) -> i32 {
    let err = WelsCheckAndRecoverForFutureDecoding(pCtx);
    if err != ERR_NONE {
        return err;
    }
    WrapShortRefPicNum(pCtx, pCurDqLayer);


    for i in 0..MAX_DPB_COUNT {
        (*pCtx).sRefPic.pRefList[LIST_0][i] = None;
        (*pCtx).sRefPic.pRefList[LIST_1][i] = None;
    }

    let mut iLSCurrPocCount = 0usize;
    let mut iLTCurrPocCount = 0usize;
    let mut pLSCurrPocList0: [Option<PicId>; MAX_DPB_COUNT] = [None; MAX_DPB_COUNT];
    let mut pLTCurrPocList0: [Option<PicId>; MAX_DPB_COUNT] = [None; MAX_DPB_COUNT];

    let short_count = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;
    for i in 0..short_count {
        let poc = short_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, i).map(|p| p.iFramePoc);
        if matches!(poc, Some(poc) if poc < iPoc) {
            pLSCurrPocList0[iLSCurrPocCount] = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
            iLSCurrPocCount += 1;
        }
    }
    for i in (0..short_count).rev() {
        let poc = short_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, i).map(|p| p.iFramePoc);
        if matches!(poc, Some(poc) if poc > iPoc) {
            pLTCurrPocList0[iLTCurrPocCount] = (*pCtx).sRefPic.pShortRefList[LIST_0][i];
            iLTCurrPocCount += 1;
        }
    }

    let long_count = (*pCtx).sRefPic.uiLongRefCount[LIST_0] as usize;
    if long_count > 1 {
        for i in 0..long_count {
            for j in (i + 1)..long_count {
                let poc_j = long_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, j).map(|p| p.iFramePoc);
                let poc_i = long_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, i).map(|p| p.iFramePoc);
                if matches!((poc_j, poc_i), (Some(pj), Some(pi)) if pj < pi) {
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
                let poc_j = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, j).map(|p| p.iFramePoc);
                let poc_i = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, i).map(|p| p.iFramePoc);
                if matches!((poc_j, poc_i), (Some(pj), Some(pi)) if pj > pi) {
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
                let poc_j = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, j).map(|p| p.iFramePoc);
                let poc_i = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, i).map(|p| p.iFramePoc);
                if matches!((poc_j, poc_i), (Some(pj), Some(pi)) if pj < pi) {
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
                let poc_j = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, j).map(|p| p.iFramePoc);
                let poc_i = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, i).map(|p| p.iFramePoc);
                if matches!((poc_j, poc_i), (Some(pj), Some(pi)) if pj < pi) {
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
                let poc_j = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, j).map(|p| p.iFramePoc);
                let poc_i = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, i).map(|p| p.iFramePoc);
                if matches!((poc_j, poc_i), (Some(pj), Some(pi)) if pj > pi) {
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
pub fn WelsReorderRefList(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) -> i32 {
    if (*pCtx).eSliceType == I_SLICE || (*pCtx).eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    let Some(pCurDqLayer) = pCurDqLayer else {
        return ERR_INFO_INVALID_PTR;
    };

    // T5.AC2: the reordering syntax is the layer's own copy, taken at the base-
    // quality slice; the null test the pointer needed is the `Option`.
    let Some(reorder_syn) = (*pCurDqLayer).sRefPicListReordering.as_ref() else {
        return ERR_INFO_INVALID_PTR;
    };

    // The guard and the lookup are one test now — see `WrapShortRefPicNum` (T5.Z1).
    let Some(pSps) = sps_of(
        &(*pCtx).sSpsPpsCtx,
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref,
    ) else {
        return ERR_INFO_INVALID_PTR;
    };

    let list_count = if (*pCtx).eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..list_count {
        let iMaxRefIdx = ((*pCtx).iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
        let iRefCount = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount[listIdx] as i32;
        let mut iPredFrameNum = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFrameNum;
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
                        let cur = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, listIdx, i)
                            .filter(|c| c.iFrameNum == iPredFrameNum && !c.bIsLongRef)
                            .map(|c| (c.uiQualityId, c.iSpsId));
                        if let Some((uiQualityId, iSpsId)) = cur {
                            if (*pCurDqLayer).sLayerInfo.sNalHeaderExt.uiQualityId == uiQualityId
                                && (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iSpsId != iSpsId
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
                        let cur = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, listIdx, i)
                            .filter(|c| c.bIsLongRef && c.iLongTermFrameIdx == target_long)
                            .map(|c| (c.uiQualityId, c.iSpsId));
                        if let Some((uiQualityId, iSpsId)) = cur {
                            if (*pCurDqLayer).sLayerInfo.sNalHeaderExt.uiQualityId == uiQualityId
                                && (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iSpsId != iSpsId
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
                let pPic = (*pCtx).sRefPic.pRefList[listIdx][i_idx];  // a handle: moved, never dereferenced

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
pub fn WelsReorderRefList2(pCtx: &mut SWelsDecoderContext, pCurDqLayer: Option<&mut DqLayerState>) -> i32 {
    if (*pCtx).eSliceType == I_SLICE || (*pCtx).eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    let Some(pCurDqLayer) = pCurDqLayer else {
        return ERR_INFO_INVALID_PTR;
    };

    // T5.AC2: the reordering syntax is the layer's own copy, taken at the base-
    // quality slice; the null test the pointer needed is the `Option`.
    let Some(reorder_syn) = (*pCurDqLayer).sRefPicListReordering.as_ref() else {
        return ERR_INFO_INVALID_PTR;
    };

    // The guard and the lookup are one test now — see `WrapShortRefPicNum` (T5.Z1).
    let Some(pSps) = sps_of(
        &(*pCtx).sSpsPpsCtx,
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref,
    ) else {
        return ERR_INFO_INVALID_PTR;
    };

    let iShortRefCount = (*pCtx).sRefPic.uiShortRefCount[LIST_0] as usize;
    let iLongRefCount = (*pCtx).sRefPic.uiLongRefCount[LIST_0] as usize;
    let iMaxRefIdx = ((*pCtx).iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
    let iCurFrameNum = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFrameNum;
    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iListCount = if (*pCtx).eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..iListCount {
        let mut iCount = 0usize;
        let iRefCount = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount[listIdx] as usize;

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
                        let cur = short_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, j);
                        if cur.is_some_and(|c| c.iFrameWrapNum == iPredFrameNum) {
                            (*pCtx).sRefPic.pRefList[listIdx][iCount] =
                                (*pCtx).sRefPic.pShortRefList[LIST_0][j];
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, listIdx, j);
                        if cur.is_some_and(|c| c.bIsLongRef || c.iFrameWrapNum != iPredFrameNum) {
                            (*pCtx).sRefPic.pRefList[listIdx][k_write] =
                                (*pCtx).sRefPic.pRefList[listIdx][j];
                            k_write += 1;
                        }
                    }
                } else {
                    iPredFrameNum =
                        reorder_syn.sReorderingSyn[listIdx][i].uiLongTermPicNum as i32;
                    for j in 0..iLongRefCount {
                        let cur = long_ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, j);
                        if cur.is_some_and(|c| c.uiLongTermPicNum == iPredFrameNum as u32) {
                            (*pCtx).sRefPic.pRefList[listIdx][iCount] =
                                (*pCtx).sRefPic.pLongRefList[LIST_0][j];
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, listIdx, j);
                        if cur.is_some_and(|c| {
                            !c.bIsLongRef || c.uiLongTermPicNum != iPredFrameNum as u32
                        }) {
                            (*pCtx).sRefPic.pRefList[listIdx][k_write] =
                                (*pCtx).sRefPic.pRefList[listIdx][j];
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
pub fn WelsMarkAsRef(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
    mut pLastDec: Option<&mut SPicture>,
) -> i32 {
    // **T5.AC3: the picture is re-derived at each use, never held.** It was a
    // `*mut SPicture` because the two arms had to unify on one binding and the
    // body below re-enters `pCtx` between every pair of stamps (T5.Z1) — a
    // borrow out of `pCtx.pPicBuff` dies at the next call that takes the context,
    // which is session Y's verdict. `dec!()` is that same derivation written at
    // each use instead of once: the thread arm re-borrows the caller's picture,
    // the pool arm re-resolves the handle, and neither result outlives its own
    // statement. No expression below holds one across a call.
    //
    // `pLastDec` is F36's threading arm and both callers pass `None`; it stays
    // (the non-goal is explicit about the arm), and it is what `isThreadCtx`
    // reads — one `is_some()` instead of the old null test.
    macro_rules! dec {
        () => {
            match pLastDec {
                Some(ref mut p) => Some(&mut **p),
                None => dec_pic(&mut pCtx.pPicBuff, pCtx.pDec),
            }
        };
    }
    let isThreadCtx = pLastDec.is_some();

    if dec!().is_none() {
        return ERR_INFO_INVALID_PTR;
    }

    // **T5.Z4: the pick is a `bool` now, not a borrow.** T5.W11c made it a borrow of
    // one field or the other, which the arms unified on their own; with the context a
    // `&mut` that borrow is a context field held across every call below that takes
    // the context. The selector travels and `ref_set` re-acquires per use (S25).
    let bTmpRefSet = isThreadCtx;

    let Some(pCurDqLayer) = pCurDqLayer else {
        return ERR_INFO_INVALID_PTR;
    };
    // Shared from here down: nothing in this function writes the layer, and the
    // marking below is a borrow *of* it that has to coexist with the `MMCO` call
    // that also takes it (T5.AC2).
    let pCurDqLayer = &*pCurDqLayer;
    let Some(pRefPicMarking) = pCurDqLayer.sRefPicMarking.as_ref() else {
        return ERR_INFO_INVALID_PTR;
    };

    let (uiQualityId, uiTemporalId) = (
        pCurDqLayer.sLayerInfo.sNalHeaderExt.uiQualityId,
        pCurDqLayer.sLayerInfo.sNalHeaderExt.uiTemporalId,
    );
    if let Some(pDec) = dec!() {
        pDec.uiQualityId = uiQualityId;
        pDec.uiTemporalId = uiTemporalId;
    }
    // The ids are read as values above the picture's borrow: `pDec` is the pool's
    // and the parameter sets are the context's, and the two travel together at
    // every one of these stamps (T5.Z1).
    if let Some(iSpsId) = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps).map(|s| s.iSpsId) {
        if let Some(pDec) = dec!() {
            pDec.iSpsId = iSpsId;
        }
    }
    if let Some(iPpsId) = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps).map(|p| p.iPpsId) {
        if let Some(pDec) = dec!() {
            pDec.iPpsId = iPpsId;
        }
    }

    let mut bIsIDRAU = false;
    if let Some(au) = cur_au(&mut pCtx.access_unit) {
        for j in au.uiStartPos..=au.uiEndPos {
            // T5.O4: the list owns its nodes, so `get` is the whole guard — the null
            // test this replaces could only ever fail past the end of the list.
            if let Some(nal) = au.node(j as usize) {
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
        if pRefPicMarking.bLongTermRefFlag {
            ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx = 0;
            AddLongTermToList(pCtx, bTmpRefSet, pCtx.pDec, 0, 0);
        } else {
            ref_set(pCtx, bTmpRefSet).iMaxLongTermFrameIdx = -1;
        }
    } else {
        if pRefPicMarking.bAdaptiveRefPicMarkingModeFlag {
            iRet = MMCO(pCtx, Some(pCurDqLayer), bTmpRefSet, pRefPicMarking);
            if iRet != ERR_NONE {
                let ec_mode = ec_active_idc(&(*pCtx).pParam);
                if ec_mode != ERROR_CON_DISABLE {
                    iRet = RemainOneBufferInDpbForEC(pCtx, bTmpRefSet);
                    if iRet != ERR_NONE {
                        return iRet;
                    }
                } else {
                    return iRet;
                }
            }
            if (*pCtx).pLastDecPicInfo.bLastHasMmco5 {
                if let Some(pDec) = dec!() {
                    pDec.iFrameNum = 0;
                    pDec.iFramePoc = 0;
                }
            }
        } else {
            iRet = SlidingWindow(pCtx, bTmpRefSet);
            if iRet != ERR_NONE {
                let ec_mode = ec_active_idc(&(*pCtx).pParam);
                if ec_mode != ERROR_CON_DISABLE {
                    iRet = RemainOneBufferInDpbForEC(pCtx, bTmpRefSet);
                    if iRet != ERR_NONE {
                        return iRet;
                    }
                } else {
                    return iRet;
                }
            }
        }
    }

    if !dec!().is_some_and(|pDec| pDec.bIsLongRef) {
        let num_ref_frames = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
            .map_or(1, |sps| sps.iNumRefFrames as u8);
        let counts = ref_set(pCtx, bTmpRefSet);
        let bDpbFull =
            counts.uiLongRefCount[LIST_0] + counts.uiShortRefCount[LIST_0] >= num_ref_frames.max(1);
        if bDpbFull {
            let ec_mode = ec_active_idc(&(*pCtx).pParam);
            if ec_mode != ERROR_CON_DISABLE {
                iRet = RemainOneBufferInDpbForEC(pCtx, bTmpRefSet);
                if iRet != ERR_NONE {
                    return iRet;
                }
            } else {
                return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
            }
        }
        iRet = AddShortTermToList(pCtx, bTmpRefSet, pCtx.pDec);
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

        {
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

    /// **T5.P′2 gave these three a pool, and that is the point of the conversion.**
    /// They used to put stack `SPicture`s straight into the list and compare the
    /// pointers back out. A list entry is a `PicId` now, and a picture outside the
    /// pool has none — so a fixture without a pool would store `None`, which is the
    /// C's null slot, and the test would be asserting that the DPB lost its entries.
    /// `PicPool::over` stamps each picture with its slot (T5.N2), exactly as
    /// `CreatePicBuff` does on the live path, and the assertions compare slots.
    #[test]
    fn test_add_short_term_to_list_and_delete() {
        let mut pic1 = SPicture::default();
        pic1.iFrameNum = 10;
        let mut pic2 = SPicture::default();
        pic2.iFrameNum = 12;


        {
            // T5.Q2: the pool owns the pictures, so the fixture hands them over and
            // resolves them back per call — which is the production shape, and it
            // retires the `addr_of_mut!` pair this test needed while the pool held
            // raw pointers into the stack frame.
            let pool = crate::decoder::pic_queue::PicPool::over(vec![
                Some(Box::new(pic1)),
                Some(Box::new(pic2)),
            ]);
            let (s1, s2) = (Some(pool.id(0)), Some(pool.id(1)));
            let mut ctx = SWelsDecoderContext::new_boxed();
            ctx.pPicBuff = Some(pool);
            let pCtx = &mut *ctx;
            let pRefPic = &mut (*pCtx).sRefPic;

            let res1 = AddShortTermToList(pCtx, false, s1);
            assert_eq!(res1, ERR_NONE);
            assert_eq!(ref_set(pCtx, false).uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_set(pCtx, false).pShortRefList[LIST_0][0], s1);

            let res2 = AddShortTermToList(pCtx, false, s2);
            assert_eq!(res2, ERR_NONE);
            assert_eq!(ref_set(pCtx, false).uiShortRefCount[LIST_0], 2);
            assert_eq!(ref_set(pCtx, false).pShortRefList[LIST_0][0], s2);
            assert_eq!(ref_set(pCtx, false).pShortRefList[LIST_0][1], s1);

            let deleted = WelsDelShortFromList(pCtx, false, 10);
            assert_eq!(deleted, s1);
            assert_eq!(ref_set(pCtx, false).uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_set(pCtx, false).pShortRefList[LIST_0][0], s2);
        }
    }

    #[test]
    fn test_add_long_term_sorted_order() {
        let mut pic1 = SPicture::default();
        let mut pic2 = SPicture::default();

        {
            let pool = crate::decoder::pic_queue::PicPool::over(vec![
                Some(Box::new(pic1)),
                Some(Box::new(pic2)),
            ]);
            let (s1, s2) = (Some(pool.id(0)), Some(pool.id(1)));
            let mut ctx = SWelsDecoderContext::new_boxed();
            ctx.pPicBuff = Some(pool);
            let pCtx = &mut *ctx;
            let pRefPic = &mut (*pCtx).sRefPic;

            AddLongTermToList(pCtx, false, s1, 5, 5);
            AddLongTermToList(pCtx, false, s2, 2, 2);

            assert_eq!(ref_set(pCtx, false).uiLongRefCount[LIST_0], 2);
            assert_eq!(ref_set(pCtx, false).pLongRefList[LIST_0][0], s2);
            assert_eq!(ref_set(pCtx, false).pLongRefList[LIST_0][1], s1);
        }
    }

    #[test]
    fn test_wels_reset_ref_pic() {
        let mut pic = SPicture::default();
        pic.iFrameNum = 1;

        {
            let pool = crate::decoder::pic_queue::PicPool::over(vec![Some(Box::new(pic))]);
            let s = Some(pool.id(0));
            let mut ctx = SWelsDecoderContext::new_boxed();
            ctx.pPicBuff = Some(pool);
            let pCtx = &mut *ctx;

            // T5.W11b: the borrow is re-derived at each use, never held across
            // `WelsResetRefPic`. **Miri convicted the previous shape** and it was
            // this session's own doing: with `pRefPic` a raw pointer both
            // derivations were `addr_of_mut!` and neither retagged, so a fixture
            // could hold one across a call that made another. T5.W11 turned the
            // callee's parameter into `&mut SRefPic` and the derivations into field
            // borrows — correct, and it makes the second derivation *invalidate* the
            // first, which is S29's sentence arriving as a test failure rather than
            // as a review comment. The production path is unaffected:
            // `WelsResetRefPic` holds one borrow of `sRefPic` and the calls inside it
            // (`pool_pic_mut`, `SetUnRef`) reach `pPicBuff` and a picture, disjoint
            // fields, which is why the `exit` battery is where this surfaced (S22).
            AddShortTermToList(pCtx, false, s);
            assert_eq!((*pCtx).sRefPic.uiShortRefCount[LIST_0], 1);
            WelsResetRefPic(pCtx);
            assert_eq!((*pCtx).sRefPic.uiShortRefCount[LIST_0], 0);
            assert_eq!((*pCtx).sRefPic.pShortRefList[LIST_0][0], None);
        }
    }
}
