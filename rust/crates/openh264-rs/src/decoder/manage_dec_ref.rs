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

/// Function pointer callback type for picture expansion.
pub type PExpandPictureFunc = unsafe extern "C" fn(
    pDst: *mut u8,
    kiStride: i32,
    kiPicWidth: i32,
    kiPicHeight: i32,
);

pub use crate::decoder::decoder_context::{Picture, SPicture, PPicture};


pub use crate::decoder::decoder_context::{SRefPic, PRefPic};
pub use crate::decoder::slice::{SRefPicListReorderSyn, PRefPicListReorderSyn, SRefPicMarking, PRefPicMarking};


pub type PSps = *mut SSps;


/// Picture Parameter Set representation (`SPps` / `PPps`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPps {
    pub iPpsId: i32,
    pub iSpsId: i32,
}

pub type PPps = *mut SPps;

impl Default for SPps {
    fn default() -> Self {
        Self {
            iPpsId: 0,
            iSpsId: 0,
        }
    }
}

pub use crate::decoder::slice::{SSliceHeader, PSliceHeader, SSliceHeaderExt};


#[repr(C)]
#[derive(Default)]
pub struct SSliceInLayer {
    pub sSliceHeaderExt: SSliceHeaderExt,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SNalUnitHeader {
    pub eNalUnitType: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SNalUnitHeaderExt {
    pub sNalUnitHeader: SNalUnitHeader,
    pub uiQualityId: u8,
    pub uiTemporalId: u8,
    pub bIdrFlag: bool,
}

#[repr(C)]
#[derive(Default)]
pub struct SLayerInfo {
    pub sSliceInLayer: SSliceInLayer,
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
}

#[repr(C)]
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
    pub pRefPicListReordering: *mut SRefPicListReorderSyn,
    pub pRefPicMarking: *mut SRefPicMarking,
}

pub type PDqLayer = *mut SDqLayer;

#[repr(C)]
pub struct SNalUnit {
    pub sNalHeaderExt: SNalUnitHeaderExt,
}

#[repr(C)]
pub struct SAccessUnit {
    pub uiStartPos: u32,
    pub uiEndPos: u32,
    pub pNalUnitsList: [*mut SNalUnit; 128],
}

pub type PAccessUnit = *mut SAccessUnit;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDecoderParam {
    pub eEcActiveIdc: i32,
}

#[repr(C)]
pub struct SLastDecPicInfo {
    pub pPreviousDecodedPictureInDpb: *mut SPicture,
    pub bLastHasMmco5: bool,
}

pub use crate::decoder::decoder_core::SExpandPicFunc;

pub use crate::decoder::decoder_context::SLogContext;


pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};


// ============================================================================
// Internal Logging & Picture Helpers
// ============================================================================

#[inline(always)]
pub unsafe fn WelsLog(_pLogCtx: &SLogContext, _iLevel: i32, _msg: &str) {

    // Logging stub for no-std / embedded compatibility
}

/// Fallback picture border expansion if dynamic assembly function pointers are not set.
pub unsafe fn ExpandReferencingPicture(
    pData: [*mut u8; 4],
    iWidth: i32,
    iHeight: i32,
    iStride: [i32; 4],
    pfExpLuma: Option<PExpandPictureFunc>,
    pfExpChrom: [Option<PExpandPictureFunc>; 2],
) {
    let pPicY = pData[0];
    let pPicCb = pData[1];
    let pPicCr = pData[2];
    let kiWidthY = iWidth;
    let kiHeightY = iHeight;
    let kiWidthUV = kiWidthY >> 1;
    let kiHeightUV = kiHeightY >> 1;

    if let Some(exp_luma) = pfExpLuma {
        if !pPicY.is_null() {
            exp_luma(pPicY, iStride[0], kiWidthY, kiHeightY);
        }
    }
    if kiWidthUV >= 16 {
        let kbChrAligned = (kiWidthUV & 0x0F) == 0;
        let idx = if kbChrAligned { 1 } else { 0 };
        if let Some(exp_chrom) = pfExpChrom[idx] {
            if !pPicCb.is_null() {
                exp_chrom(pPicCb, iStride[1], kiWidthUV, kiHeightUV);
            }
            if !pPicCr.is_null() {
                exp_chrom(pPicCr, iStride[2], kiWidthUV, kiHeightUV);
            }
        }
    }
}

// ============================================================================
// Core Reference Management Implementation
// ============================================================================

/// Unmarks a reconstructed picture as unused for reference and resets its identifiers.
///
/// Matches `static void SetUnRef (PPicture pRef)` in `manage_dec_ref.cpp`.
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
    let ctx = &mut *pCtx;
    let pRefPic = &mut ctx.sRefPic;
    pRefPic.uiLongRefCount[LIST_0] = 0;
    pRefPic.uiShortRefCount[LIST_0] = 0;

    pRefPic.uiRefCount[LIST_0] = 0;
    pRefPic.uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        let pPic = pRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() {
            SetUnRef(pPic);
            pRefPic.pShortRefList[LIST_0][i] = std::ptr::null_mut();
        }
    }
    pRefPic.uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        let pPic = pRefPic.pLongRefList[LIST_0][i];
        if !pPic.is_null() {
            SetUnRef(pPic);
            pRefPic.pLongRefList[LIST_0][i] = std::ptr::null_mut();
        }
    }
    pRefPic.uiLongRefCount[LIST_0] = 0;
}

/// Clears reference list pointers and counts without invoking `SetUnRef`.
///
/// Matches `void WelsResetRefPicWithoutUnRef (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsResetRefPicWithoutUnRef(pCtx: *mut SWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let ctx = &mut *pCtx;
    let pRefPic = &mut ctx.sRefPic;
    pRefPic.uiLongRefCount[LIST_0] = 0;
    pRefPic.uiShortRefCount[LIST_0] = 0;

    pRefPic.uiRefCount[LIST_0] = 0;
    pRefPic.uiRefCount[LIST_1] = 0;

    for i in 0..MAX_DPB_COUNT {
        pRefPic.pShortRefList[LIST_0][i] = std::ptr::null_mut();
    }
    pRefPic.uiShortRefCount[LIST_0] = 0;

    for i in 0..MAX_DPB_COUNT {
        pRefPic.pLongRefList[LIST_0][i] = std::ptr::null_mut();
    }
    pRefPic.uiLongRefCount[LIST_0] = 0;
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
                std::ptr::copy(
                    ref_pic.pShortRefList[LIST_0].as_ptr().add(i + 1),
                    ref_pic.pShortRefList[LIST_0].as_mut_ptr().add(i),
                    iMoveSize,
                );
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
                std::ptr::copy(
                    ref_pic.pLongRefList[LIST_0].as_ptr().add(i + 1),
                    ref_pic.pLongRefList[LIST_0].as_mut_ptr().add(i),
                    iMoveSize,
                );
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
        std::ptr::copy(
            ref_pic.pShortRefList[LIST_0].as_ptr(),
            ref_pic.pShortRefList[LIST_0].as_mut_ptr().add(1),
            short_count,
        );
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
            std::ptr::copy(
                ref_pic.pLongRefList[LIST_0].as_ptr().add(insert_idx),
                ref_pic.pLongRefList[LIST_0].as_mut_ptr().add(insert_idx + 1),
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
    let ref_pic = &mut *pRefPic;
    let mut iRet = ERR_NONE;
    let count = ref_pic.uiRefCount[LIST_0] as usize;

    for i in 0..count {
        let pPic = ref_pic.pRefList[LIST_0][i];
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
    let ctx = &mut *pCtx;
    if ctx.pCurDqLayer.is_null() {
        return;
    }
    let pCurDqLayer = &mut *ctx.pCurDqLayer;
    let pSliceHeader = &mut pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    if pSliceHeader.pSps.is_null() {
        return;
    }
    let pSps = &*(pSliceHeader.pSps as *mut SSps);

    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iShortRefCount = ctx.sRefPic.uiShortRefCount[LIST_0] as usize;

    for i in 0..iShortRefCount {
        let pPic = ctx.sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() {
            let pic = &mut *pPic;
            if pic.iFrameNum > pSliceHeader.iFrameNum {
                pic.iFrameWrapNum = pic.iFrameNum - iMaxPicNum;
            } else {
                pic.iFrameWrapNum = pic.iFrameNum;
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
    let ctx = &mut *pCtx;
    let ref_pic = &mut *pRefPic;
    let num_ref_frames = if !ctx.pSps.is_null() {
        (*ctx.pSps).iNumRefFrames as u8
    } else {
        1
    };

    if ref_pic.uiShortRefCount[LIST_0] + ref_pic.uiLongRefCount[LIST_0] >= num_ref_frames {
        if ref_pic.uiShortRefCount[LIST_0] == 0 {
            WelsLog(
                &ctx.sLogCtx,
                WELS_LOG_ERROR,
                "No reference picture in short term list when sliding window",
            );
            return ERR_INFO_INVALID_MMCO_REF_NUM_NOT_ENOUGH;
        }
        let short_count = ref_pic.uiShortRefCount[LIST_0] as isize;
        for i in (0..short_count).rev() {
            let pCur = ref_pic.pShortRefList[LIST_0][i as usize];
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
    let ctx = &mut *pCtx;
    let ref_pic = &mut *pRefPic;
    let num_ref_frames = if !ctx.pSps.is_null() {
        (*ctx.pSps).iNumRefFrames as u8
    } else {
        1
    };

    if ref_pic.uiShortRefCount[0] + ref_pic.uiLongRefCount[0] < num_ref_frames {
        return ERR_NONE;
    }

    let mut iRet = ERR_NONE;
    if ref_pic.uiShortRefCount[0] > 0 {
        iRet = SlidingWindow(pCtx, pRefPic);
    } else {
        let mut iLongTermFrameIdx = 0i32;
        let iMaxLongTermFrameIdx = ref_pic.iMaxLongTermFrameIdx;
        let iCurrLTRFrameIdx = GetLTRFrameIndex(pRefPic, ctx.iFrameNumOfAuMarkedLtr);

        while (ref_pic.uiLongRefCount[0] >= num_ref_frames)
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

    if ref_pic.uiShortRefCount[0] + ref_pic.uiLongRefCount[0] >= num_ref_frames {
        WelsLog(
            &ctx.sLogCtx,
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
    let ctx = &mut *pCtx;

    if (ctx.sRefPic.uiShortRefCount[LIST_0] + ctx.sRefPic.uiLongRefCount[LIST_0] <= 0)
        && (ctx.eSliceType != EWelsSliceType::I_SLICE && ctx.eSliceType != EWelsSliceType::SI_SLICE)
    {
        let ec_mode = if !ctx.pParam.is_null() {
            (*ctx.pParam).eEcActiveIdc
        } else {
            crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
        };

        if ec_mode != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE {
            let pRef = ctx.pDec;
            if !pRef.is_null() {
                let ref_pic = &mut *pRef;
                ref_pic.bIsComplete = false;
                if !ctx.pSps.is_null() {
                    ref_pic.iSpsId = (*ctx.pSps).iSpsId;
                }
                if !ctx.pPps.is_null() {
                    ref_pic.iPpsId = (*ctx.pPps).iPpsId;
                }
                if ctx.eSliceType == EWelsSliceType::B_SLICE {
                    for list in 0..LIST_A {
                        for i in 0..MAX_DPB_COUNT {
                            ref_pic.pRefPic[list][i] = std::ptr::null_mut();
                        }
                    }
                }
                ctx.iErrorCode |= dsDataErrorConcealed;

                let mut bCopyPrevious = false;
                let prev_pic = if !ctx.pLastDecPicInfo.is_null() {
                    (*ctx.pLastDecPicInfo).pPreviousDecodedPictureInDpb
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
                    let prev = &*prev_pic;
                    bCopyPrevious = ref_pic.iWidthInPixel == prev.iWidthInPixel
                        && ref_pic.iHeightInPixel == prev.iHeightInPixel;
                }

                if !bCopyPrevious {
                    if !ref_pic.pData[0].is_null() {
                        std::ptr::write_bytes(
                            ref_pic.pData[0],
                            128,
                            (ref_pic.iLinesize[0] * ref_pic.iHeightInPixel) as usize,
                        );
                    }
                    if !ref_pic.pData[1].is_null() {
                        std::ptr::write_bytes(
                            ref_pic.pData[1],
                            128,
                            (ref_pic.iLinesize[1] * ref_pic.iHeightInPixel / 2) as usize,
                        );
                    }
                    if !ref_pic.pData[2].is_null() {
                        std::ptr::write_bytes(
                            ref_pic.pData[2],
                            128,
                            (ref_pic.iLinesize[2] * ref_pic.iHeightInPixel / 2) as usize,
                        );
                    }
                } else if pRef == prev_pic {
                    WelsLog(
                        &ctx.sLogCtx,
                        WELS_LOG_WARNING,
                        "WelsInitRefList()::EC memcpy overlap.",
                    );
                } else {
                    let prev = &*prev_pic;
                    if !ref_pic.pData[0].is_null() && !prev.pData[0].is_null() {
                        std::ptr::copy_nonoverlapping(
                            prev.pData[0],
                            ref_pic.pData[0],
                            (ref_pic.iLinesize[0] * ref_pic.iHeightInPixel) as usize,
                        );
                    }
                    if !ref_pic.pData[1].is_null() && !prev.pData[1].is_null() {
                        std::ptr::copy_nonoverlapping(
                            prev.pData[1],
                            ref_pic.pData[1],
                            (ref_pic.iLinesize[1] * ref_pic.iHeightInPixel / 2) as usize,
                        );
                    }
                    if !ref_pic.pData[2].is_null() && !prev.pData[2].is_null() {
                        std::ptr::copy_nonoverlapping(
                            prev.pData[2],
                            ref_pic.pData[2],
                            (ref_pic.iLinesize[2] * ref_pic.iHeightInPixel / 2) as usize,
                        );
                    }
                }
                ref_pic.iFrameNum = 0;
                ref_pic.iFramePoc = 0;
                ref_pic.uiTemporalId = 0;
                ref_pic.uiQualityId = 0;
                ref_pic.eSliceType = ctx.eSliceType;

                ExpandReferencingPicture(
                    ref_pic.pData,
                    ref_pic.iWidthInPixel,
                    ref_pic.iHeightInPixel,
                    ref_pic.iLinesize,
                    ctx.sExpandPicFunc.pfExpandLumaPicture,
                    ctx.sExpandPicFunc.pfExpandChromaPicture,
                );
                AddShortTermToList(&mut ctx.sRefPic, pRef);
            } else {
                WelsLog(
                    &ctx.sLogCtx,
                    WELS_LOG_ERROR,
                    "WelsInitRefList()::PrefetchPic for EC errors.",
                );
                ctx.iErrorCode |= dsOutOfMemory;
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
    let ctx = &mut *pCtx;
    let ref_pic = &mut *pRefPic;
    let mut iRet = ERR_NONE;

    match uiMmcoType {
        MMCO_SHORT2UNUSED => {
            let pPic = WelsDelShortFromListSetUnref(pRefPic, iShortFrameNum);
            if pPic.is_null() {
                WelsLog(
                    &ctx.sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_SHORT2UNUSED: delete an empty entry from short term list",
                );
            }
        }
        MMCO_LONG2UNUSED => {
            let pPic = WelsDelLongFromListSetUnref(pRefPic, uiLongTermPicNum);
            if pPic.is_null() {
                WelsLog(
                    &ctx.sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2UNUSED: delete an empty entry from long term list",
                );
            }
        }
        MMCO_SHORT2LONG => {
            if iLongTermFrameIdx > ref_pic.iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            let pPic = WelsDelShortFromList(pRefPic, iShortFrameNum);
            if pPic.is_null() {
                WelsLog(
                    &ctx.sLogCtx,
                    WELS_LOG_WARNING,
                    "MMCO_LONG2LONG: delete an empty entry from short term list",
                );
            } else {
                WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
                ctx.bCurAuContainLtrMarkSeFlag = true;
                ctx.iFrameNumOfAuMarkedLtr = iShortFrameNum;
                MarkAsLongTerm(pRefPic, iShortFrameNum, iLongTermFrameIdx, uiLongTermPicNum);
            }
        }
        MMCO_SET_MAX_LONG => {
            ref_pic.iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
            let mut i = 0;
            while i < (ref_pic.uiLongRefCount[LIST_0] as usize) {
                let pCur = ref_pic.pLongRefList[LIST_0][i];
                if !pCur.is_null() && (*pCur).iLongTermFrameIdx > ref_pic.iMaxLongTermFrameIdx {
                    WelsDelLongFromListSetUnref(pRefPic, (*pCur).iLongTermFrameIdx as u32);
                } else {
                    i += 1;
                }
            }
        }
        MMCO_RESET => {
            WelsResetRefPic(pCtx);
            if !ctx.pLastDecPicInfo.is_null() {
                (*ctx.pLastDecPicInfo).bLastHasMmco5 = true;
            }
        }
        MMCO_LONG => {
            if iLongTermFrameIdx > ref_pic.iMaxLongTermFrameIdx {
                return ERR_INFO_INVALID_MMCO_LONG_TERM_IDX_EXCEED_MAX;
            }
            WelsDelLongFromListSetUnref(pRefPic, iLongTermFrameIdx as u32);
            let num_ref_frames = if !ctx.pSps.is_null() {
                (*ctx.pSps).iNumRefFrames as u8
            } else {
                1
            };
            if ref_pic.uiLongRefCount[LIST_0] + ref_pic.uiShortRefCount[LIST_0]
                >= num_ref_frames.max(1)
            {
                return ERR_INFO_INVALID_MMCO_REF_NUM_OVERFLOW;
            }
            ctx.bCurAuContainLtrMarkSeFlag = true;
            ctx.iFrameNumOfAuMarkedLtr = ctx.iFrameNum;
            iRet = AddLongTermToList(pRefPic, ctx.pDec, iLongTermFrameIdx, uiLongTermPicNum);
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
    let ctx = &mut *pCtx;
    let marking = &*pRefPicMarking;

    let uiLog2MaxFrameNum = if !ctx.pCurDqLayer.is_null()
        && !(*ctx.pCurDqLayer).sLayerInfo.pSps.is_null()
    {
        (*(*ctx.pCurDqLayer).sLayerInfo.pSps).uiLog2MaxFrameNum
    } else if !ctx.pSps.is_null() {
        (*ctx.pSps).uiLog2MaxFrameNum
    } else {
        4
    };

    let mut i = 0usize;
    while i < MAX_MMCO_COUNT && marking.sMmcoRef[i].uiMmcoType != MMCO_END {
        let uiMmcoType = marking.sMmcoRef[i].uiMmcoType;
        let iShortFrameNum =
            (ctx.iFrameNum - marking.sMmcoRef[i].iDiffOfPicNum) & ((1i32 << uiLog2MaxFrameNum) - 1);
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
    let ctx = &mut *pCtx;

    for i in 0..MAX_DPB_COUNT {
        ctx.sRefPic.pRefList[LIST_0][i] = std::ptr::null_mut();
    }

    let mut iCount = 0usize;
    let short_count = ctx.sRefPic.uiShortRefCount[LIST_0] as usize;
    for i in 0..short_count {
        if iCount < MAX_REF_PIC_COUNT {
            ctx.sRefPic.pRefList[LIST_0][iCount] = ctx.sRefPic.pShortRefList[LIST_0][i];
            iCount += 1;
        }
    }

    let long_count = ctx.sRefPic.uiLongRefCount[LIST_0] as usize;
    for i in 0..long_count {
        if iCount < MAX_REF_PIC_COUNT {
            ctx.sRefPic.pRefList[LIST_0][iCount] = ctx.sRefPic.pLongRefList[LIST_0][i];
            iCount += 1;
        }
    }
    ctx.sRefPic.uiRefCount[LIST_0] = iCount as u8;
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
    let ctx = &mut *pCtx;

    for i in 0..MAX_DPB_COUNT {
        ctx.sRefPic.pRefList[LIST_0][i] = std::ptr::null_mut();
        ctx.sRefPic.pRefList[LIST_1][i] = std::ptr::null_mut();
    }

    let mut iLSCurrPocCount = 0usize;
    let mut iLTCurrPocCount = 0usize;
    let mut pLSCurrPocList0: [*mut SPicture; MAX_DPB_COUNT] = [std::ptr::null_mut(); MAX_DPB_COUNT];
    let mut pLTCurrPocList0: [*mut SPicture; MAX_DPB_COUNT] = [std::ptr::null_mut(); MAX_DPB_COUNT];

    let short_count = ctx.sRefPic.uiShortRefCount[LIST_0] as usize;
    for i in 0..short_count {
        let pPic = ctx.sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFramePoc < iPoc {
            pLSCurrPocList0[iLSCurrPocCount] = pPic;
            iLSCurrPocCount += 1;
        }
    }
    for i in (0..short_count).rev() {
        let pPic = ctx.sRefPic.pShortRefList[LIST_0][i];
        if !pPic.is_null() && (*pPic).iFramePoc > iPoc {
            pLTCurrPocList0[iLTCurrPocCount] = pPic;
            iLTCurrPocCount += 1;
        }
    }

    let long_count = ctx.sRefPic.uiLongRefCount[LIST_0] as usize;
    if long_count > 1 {
        for i in 0..long_count {
            for j in (i + 1)..long_count {
                let pj = ctx.sRefPic.pLongRefList[LIST_0][j];
                let pi = ctx.sRefPic.pLongRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    ctx.sRefPic.pLongRefList[LIST_0].swap(i, j);
                }
            }
        }
    }

    let iCurrPocCount = iLSCurrPocCount + iLTCurrPocCount;
    let mut iCount = 0usize;

    // LIST_0 assembly
    for i in 0..iLSCurrPocCount {
        ctx.sRefPic.pRefList[LIST_0][iCount] = pLSCurrPocList0[i];
        iCount += 1;
    }
    if iLSCurrPocCount > 1 {
        for i in 0..iLSCurrPocCount {
            for j in (i + 1)..iLSCurrPocCount {
                let pj = ctx.sRefPic.pRefList[LIST_0][j];
                let pi = ctx.sRefPic.pRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc > (*pi).iFramePoc {
                    ctx.sRefPic.pRefList[LIST_0].swap(i, j);
                }
            }
        }
    }
    for i in 0..iLTCurrPocCount {
        ctx.sRefPic.pRefList[LIST_0][iCount] = pLTCurrPocList0[i];
        iCount += 1;
    }
    if iLTCurrPocCount > 1 {
        for i in iLSCurrPocCount..iCurrPocCount {
            for j in (i + 1)..iCurrPocCount {
                let pj = ctx.sRefPic.pRefList[LIST_0][j];
                let pi = ctx.sRefPic.pRefList[LIST_0][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    ctx.sRefPic.pRefList[LIST_0].swap(i, j);
                }
            }
        }
    }
    for i in 0..long_count {
        ctx.sRefPic.pRefList[LIST_0][iCount] = ctx.sRefPic.pLongRefList[LIST_0][i];
        iCount += 1;
    }
    ctx.sRefPic.uiRefCount[LIST_0] = iCount as u8;

    // LIST_1 assembly
    iCount = 0;
    for i in 0..iLTCurrPocCount {
        ctx.sRefPic.pRefList[LIST_1][iCount] = pLTCurrPocList0[i];
        iCount += 1;
    }
    if iLTCurrPocCount > 1 {
        for i in 0..iLTCurrPocCount {
            for j in (i + 1)..iLTCurrPocCount {
                let pj = ctx.sRefPic.pRefList[LIST_1][j];
                let pi = ctx.sRefPic.pRefList[LIST_1][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc < (*pi).iFramePoc {
                    ctx.sRefPic.pRefList[LIST_1].swap(i, j);
                }
            }
        }
    }
    for i in 0..iLSCurrPocCount {
        ctx.sRefPic.pRefList[LIST_1][iCount] = pLSCurrPocList0[i];
        iCount += 1;
    }
    if iLSCurrPocCount > 1 {
        for i in iLTCurrPocCount..iCurrPocCount {
            for j in (i + 1)..iCurrPocCount {
                let pj = ctx.sRefPic.pRefList[LIST_1][j];
                let pi = ctx.sRefPic.pRefList[LIST_1][i];
                if !pj.is_null() && !pi.is_null() && (*pj).iFramePoc > (*pi).iFramePoc {
                    ctx.sRefPic.pRefList[LIST_1].swap(i, j);
                }
            }
        }
    }
    for i in 0..long_count {
        ctx.sRefPic.pRefList[LIST_1][iCount] = ctx.sRefPic.pLongRefList[LIST_0][i];
        iCount += 1;
    }
    ctx.sRefPic.uiRefCount[LIST_1] = iCount as u8;

    ERR_NONE
}

/// Modifies the active reference picture lists based on parsed RPLR commands.
///
/// Matches `int32_t WelsReorderRefList (PWelsDecoderContext pCtx)` in `manage_dec_ref.cpp`.
pub unsafe fn WelsReorderRefList(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let ctx = &mut *pCtx;
    if ctx.eSliceType == I_SLICE || ctx.eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    if ctx.pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pCurDqLayer = &mut *ctx.pCurDqLayer;
    let pRefPicListReorderSyn = pCurDqLayer.pRefPicListReordering;
    if pRefPicListReorderSyn.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let reorder_syn = &*pRefPicListReorderSyn;

    let pNalHeaderExt = &pCurDqLayer.sLayerInfo.sNalHeaderExt;
    let pSliceHeader = &mut pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    if pSliceHeader.pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = &*(pSliceHeader.pSps as *mut SSps);

    let list_count = if ctx.eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..list_count {
        let iMaxRefIdx = (ctx.iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
        let iRefCount = pSliceHeader.uiRefCount[listIdx] as i32;
        let mut iPredFrameNum = pSliceHeader.iFrameNum;
        let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
        let mut iReorderingIndex = 0usize;

        if iRefCount <= 0 {
            ctx.iErrorCode = dsNoParamSets;
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
                        let cur = ctx.sRefPic.pRefList[listIdx][i];
                        if !cur.is_null() && (*cur).iFrameNum == iPredFrameNum && !(*cur).bIsLongRef
                        {
                            if pNalHeaderExt.uiQualityId == (*cur).uiQualityId
                                && pSliceHeader.iSpsId != (*cur).iSpsId
                            {
                                WelsLog(
                                    &ctx.sLogCtx,
                                    WELS_LOG_WARNING,
                                    "WelsReorderRefList()::::BASE LAYER SPS mismatch",
                                );
                                ctx.iErrorCode = dsNoParamSets;
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
                        let cur = ctx.sRefPic.pRefList[listIdx][i];
                        if !cur.is_null()
                            && (*cur).bIsLongRef
                            && (*cur).iLongTermFrameIdx == target_long
                        {
                            if pNalHeaderExt.uiQualityId == (*cur).uiQualityId
                                && pSliceHeader.iSpsId != (*cur).iSpsId
                            {
                                WelsLog(
                                    &ctx.sLogCtx,
                                    WELS_LOG_WARNING,
                                    "WelsReorderRefList()::::BASE LAYER SPS mismatch",
                                );
                                ctx.iErrorCode = dsNoParamSets;
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
                let pPic = ctx.sRefPic.pRefList[listIdx][i_idx];

                if i_idx > iReorderingIndex {
                    let move_len = i_idx - iReorderingIndex;
                    std::ptr::copy(
                        ctx.sRefPic.pRefList[listIdx]
                            .as_ptr()
                            .add(iReorderingIndex),
                        ctx.sRefPic.pRefList[listIdx]
                            .as_mut_ptr()
                            .add(1 + iReorderingIndex),
                        move_len,
                    );
                } else if i_idx < iReorderingIndex {
                    let move_len = iMaxRefIdx - iReorderingIndex;
                    std::ptr::copy(
                        ctx.sRefPic.pRefList[listIdx]
                            .as_ptr()
                            .add(iReorderingIndex),
                        ctx.sRefPic.pRefList[listIdx]
                            .as_mut_ptr()
                            .add(1 + iReorderingIndex),
                        move_len,
                    );
                }
                ctx.sRefPic.pRefList[listIdx][iReorderingIndex] = pPic;
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
    let ctx = &mut *pCtx;
    if ctx.eSliceType == I_SLICE || ctx.eSliceType == SI_SLICE {
        return ERR_NONE;
    }
    if ctx.pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pCurDqLayer = &mut *ctx.pCurDqLayer;
    let pRefPicListReorderSyn = pCurDqLayer.pRefPicListReordering;
    if pRefPicListReorderSyn.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let reorder_syn = &*pRefPicListReorderSyn;

    let pSliceHeader = &mut pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    if pSliceHeader.pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = &*(pSliceHeader.pSps as *mut SSps);

    let iShortRefCount = ctx.sRefPic.uiShortRefCount[LIST_0] as usize;
    let iLongRefCount = ctx.sRefPic.uiLongRefCount[LIST_0] as usize;
    let iMaxRefIdx = (ctx.iPicQueueNumber as usize).min(MAX_REF_PIC_COUNT);
    let iCurFrameNum = pSliceHeader.iFrameNum;
    let iMaxPicNum = 1i32 << pSps.uiLog2MaxFrameNum;
    let iListCount = if ctx.eSliceType == B_SLICE { 2 } else { 1 };

    for listIdx in 0..iListCount {
        let mut iCount = 0usize;
        let iRefCount = pSliceHeader.uiRefCount[listIdx] as usize;

        if reorder_syn.bRefPicListReorderingFlag[listIdx] {
            let mut iPredFrameNum = iCurFrameNum;
            let mut i = 0usize;
            while reorder_syn.sReorderingSyn[listIdx][i].uiReorderingOfPicNumsIdc != 3 {
                if iCount >= iMaxRefIdx {
                    break;
                }
                for j in (iCount + 1..=iRefCount).rev() {
                    ctx.sRefPic.pRefList[listIdx][j] = ctx.sRefPic.pRefList[listIdx][j - 1];
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
                        let cur = ctx.sRefPic.pShortRefList[LIST_0][j];
                        if !cur.is_null() && (*cur).iFrameWrapNum == iPredFrameNum {
                            ctx.sRefPic.pRefList[listIdx][iCount] = cur;
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = ctx.sRefPic.pRefList[listIdx][j];
                        if !cur.is_null()
                            && ((*cur).bIsLongRef || (*cur).iFrameWrapNum != iPredFrameNum)
                        {
                            ctx.sRefPic.pRefList[listIdx][k_write] = cur;
                            k_write += 1;
                        }
                    }
                } else {
                    iPredFrameNum =
                        reorder_syn.sReorderingSyn[listIdx][i].uiLongTermPicNum as i32;
                    for j in 0..iLongRefCount {
                        let cur = ctx.sRefPic.pLongRefList[LIST_0][j];
                        if !cur.is_null() && (*cur).uiLongTermPicNum == iPredFrameNum as u32 {
                            ctx.sRefPic.pRefList[listIdx][iCount] = cur;
                            iCount += 1;
                            break;
                        }
                    }
                    let k = iCount;
                    let mut k_write = k;
                    for j in k..=iRefCount {
                        let cur = ctx.sRefPic.pRefList[listIdx][j];
                        if !cur.is_null()
                            && (!(*cur).bIsLongRef
                                || (*cur).uiLongTermPicNum != iPredFrameNum as u32)
                        {
                            ctx.sRefPic.pRefList[listIdx][k_write] = cur;
                            k_write += 1;
                        }
                    }
                }
                i += 1;
            }
        }

        let start_fill = (1usize).max(iCount.max(ctx.sRefPic.uiRefCount[listIdx] as usize));
        for i in start_fill..iRefCount {
            ctx.sRefPic.pRefList[listIdx][i] = ctx.sRefPic.pRefList[listIdx][i - 1];
        }
        ctx.sRefPic.uiRefCount[listIdx] =
            (iCount.max(ctx.sRefPic.uiRefCount[listIdx] as usize)).min(iRefCount) as u8;
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
    let ctx = &mut *pCtx;
    let mut isThreadCtx = true;
    let pDec = if !pLastDec.is_null() {
        pLastDec
    } else {
        isThreadCtx = false;
        ctx.pDec
    };

    if pDec.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let pRefPic = if isThreadCtx {
        &mut ctx.sTmpRefPic as *mut SRefPic
    } else {
        &mut ctx.sRefPic as *mut SRefPic
    };

    if ctx.pCurDqLayer.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurDqLayer = &mut *ctx.pCurDqLayer;
    let pRefPicMarking = pCurDqLayer.pRefPicMarking;
    if pRefPicMarking.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let marking = &mut *pRefPicMarking;

    let dec = &mut *pDec;
    dec.uiQualityId = pCurDqLayer.sLayerInfo.sNalHeaderExt.uiQualityId;
    dec.uiTemporalId = pCurDqLayer.sLayerInfo.sNalHeaderExt.uiTemporalId;
    if !ctx.pSps.is_null() {
        dec.iSpsId = (*ctx.pSps).iSpsId;
    }
    if !ctx.pPps.is_null() {
        dec.iPpsId = (*ctx.pPps).iPpsId;
    }

    let mut bIsIDRAU = false;
    if !ctx.pAccessUnitList.is_null() {
        let au = &*ctx.pAccessUnitList;
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
        if marking.bLongTermRefFlag {
            (*pRefPic).iMaxLongTermFrameIdx = 0;
            AddLongTermToList(pRefPic, pDec, 0, 0);
        } else {
            (*pRefPic).iMaxLongTermFrameIdx = -1;
        }
    } else {
        if marking.bAdaptiveRefPicMarkingModeFlag {
            iRet = MMCO(pCtx, pRefPic, pRefPicMarking);
            if iRet != ERR_NONE {
                let ec_mode = if !ctx.pParam.is_null() {
                    (*ctx.pParam).eEcActiveIdc
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
            if !ctx.pLastDecPicInfo.is_null() && (*ctx.pLastDecPicInfo).bLastHasMmco5 {
                dec.iFrameNum = 0;
                dec.iFramePoc = 0;
            }
        } else {
            iRet = SlidingWindow(pCtx, pRefPic);
            if iRet != ERR_NONE {
                let ec_mode = if !ctx.pParam.is_null() {
                    (*ctx.pParam).eEcActiveIdc
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

    if !dec.bIsLongRef {
        let num_ref_frames = if !ctx.pSps.is_null() {
            (*ctx.pSps).iNumRefFrames as u8
        } else {
            1
        };
        if (*pRefPic).uiLongRefCount[LIST_0] + (*pRefPic).uiShortRefCount[LIST_0]
            >= num_ref_frames.max(1)
        {
            let ec_mode = if !ctx.pParam.is_null() {
                (*ctx.pParam).eEcActiveIdc
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

    #[test]
    fn test_add_short_term_to_list_and_delete() {
        let mut ref_pic = SRefPic::default();
        let mut pic1 = SPicture::default();
        pic1.iFrameNum = 10;
        let mut pic2 = SPicture::default();
        pic2.iFrameNum = 12;

        unsafe {
            let res1 = AddShortTermToList(&mut ref_pic, &mut pic1);
            assert_eq!(res1, ERR_NONE);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], &mut pic1 as *mut _);

            let res2 = AddShortTermToList(&mut ref_pic, &mut pic2);
            assert_eq!(res2, ERR_NONE);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 2);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], &mut pic2 as *mut _);
            assert_eq!(ref_pic.pShortRefList[LIST_0][1], &mut pic1 as *mut _);

            let deleted = WelsDelShortFromList(&mut ref_pic, 10);
            assert_eq!(deleted, &mut pic1 as *mut _);
            assert_eq!(ref_pic.uiShortRefCount[LIST_0], 1);
            assert_eq!(ref_pic.pShortRefList[LIST_0][0], &mut pic2 as *mut _);
        }
    }

    #[test]
    fn test_add_long_term_sorted_order() {
        let mut ref_pic = SRefPic::default();
        let mut pic1 = SPicture::default();
        let mut pic2 = SPicture::default();

        unsafe {
            AddLongTermToList(&mut ref_pic, &mut pic1, 5, 5);
            AddLongTermToList(&mut ref_pic, &mut pic2, 2, 2);

            assert_eq!(ref_pic.uiLongRefCount[LIST_0], 2);
            assert_eq!(ref_pic.pLongRefList[LIST_0][0], &mut pic2 as *mut _);
            assert_eq!(ref_pic.pLongRefList[LIST_0][1], &mut pic1 as *mut _);
        }
    }

    #[test]
    fn test_wels_reset_ref_pic() {
        let mut ctx = SWelsDecoderContext::default();

        let mut pic = SPicture::default();
        pic.iFrameNum = 1;
        unsafe {
            AddShortTermToList(&mut ctx.sRefPic, &mut pic);
            assert_eq!(ctx.sRefPic.uiShortRefCount[LIST_0], 1);
            WelsResetRefPic(&mut ctx);
            assert_eq!(ctx.sRefPic.uiShortRefCount[LIST_0], 0);
            assert_eq!(ctx.sRefPic.pShortRefList[LIST_0][0], std::ptr::null_mut());
        }
    }
}
