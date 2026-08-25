//! Port of `codec/encoder/core/src/svc_enc_slice_segment.cpp` — the slice-argument
//! validation group.
//!
//! **Partial by design.** This module holds the six functions
//! `ParamValidationExt` needs, which is what unblocks the `SM_FIXEDSLCNUM_SLICE` and
//! `SM_RASTER_SLICE` arms:
//!
//! - `CheckFixedSliceNumMultiSliceSetting`
//! - `CheckRowMbMultiSliceSetting`
//! - `CheckRasterMultiSliceSetting`
//! - `GomValidCheckSliceNum`
//! - `GomValidCheckSliceMbNum`
//! - `SliceArgumentValidationFixedSliceMode`
//!
//! The rest of `svc_enc_slice_segment.cpp` — `InitSliceSegment`,
//! `AssignMbMapSingleSlice`, `AssignMbMapMultipleSlices`, `GetInitialSliceNum`,
//! `InitSlicePEncCtx`/`UninitSlicePEncCtx`, `WelsGetFirstMbOfSlice`,
//! `WelsGetPrevMbOfSlice`, `WelsGetNumMbInSlice`, `DynamicMaxSliceNumConstraint` —
//! allocates and drives `SSliceCtx::pOverallMbMap` and belongs with the context
//! construction in Phase 4. `WelsMbToSliceIdc`, `WelsGetNextMbOfSlice`,
//! `GetCurrentSliceNum` and `DynamicAdjustSlicePEncCtxAll` already exist elsewhere in
//! the port and are deliberately not re-declared here.
//!
//! Note the header comment in the Phase-3 plan places
//! `SliceArgumentValidationFixedSliceMode` in `svc_enc_slice_segment.cpp`; it is
//! actually defined at `encoder_ext.cpp:174` and declared in
//! `svc_enc_slice_segment.h`. It is kept here, with the functions it calls.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

#![deny(unsafe_code)]

use std::ffi::c_char;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::api::codec_api::SliceModeEnum::{
    SM_FIXEDSLCNUM_SLICE, SM_RASTER_SLICE, SM_SINGLE_SLICE, SM_SIZELIMITED_SLICE,
};
use crate::api::codec_api::RC_MODES::RC_OFF_MODE;
use crate::api::codec_api::{RC_MODES, SSliceArgument};
use crate::encoder::encoder_context::SLogContext;
use crate::encoder::slice_multi_threading::{
    SSliceCtx, WelsSetMemMultiplebytes_c, DEFAULT_MAXPACKETSIZE_CONSTRAINT,
};
use crate::encoder::svc_encode_slice::SDqLayer;
use crate::encoder::rc::{
    WELS_DIV_ROUND, GOM_ROW_MODE0_180P, GOM_ROW_MODE0_360P, GOM_ROW_MODE0_720P, GOM_ROW_MODE0_90P,
    MB_WIDTH_THRESHOLD_180P, MB_WIDTH_THRESHOLD_360P, MB_WIDTH_THRESHOLD_90P,
};
use crate::encoder::slice_multi_threading::{DynamicDetectCpuCores, INT_MULTIPLY, MAX_SLICES_NUM};
use crate::encoder::wels_encoder_ext::{
    ENC_RETURN_SUCCESS, ENC_RETURN_UNSUPPORTED_PARA, MIN_NUM_MB_PER_SLICE,
};

/// `AVERSLICENUM_CONSTRAINT` — `svc_enc_slice_segment.h:63`; equal to
/// `MAX_SLICES_NUM`. Used as the initial slice count in `SM_SIZELIMITED_SLICE`.
pub const AVERSLICENUM_CONSTRAINT: usize = MAX_SLICES_NUM;

/// The GOM size in macroblocks for a frame `kiMbWidth` macroblocks wide.
///
/// Shared prologue of `GomValidCheckSliceNum` (svc_enc_slice_segment.cpp:226) and
/// `GomValidCheckSliceMbNum` (:271), which compute it identically.
///
/// The default RC is bitrate mode, but this has to hold for both: `GOM_ROW_MODE0_?P`
/// is an integer multiple of `GOM_ROW_MODE1_?P` (see `rc.h`), so MODE0 is taken as the
/// initial value because the RC mode can change from outside without refreshing this.
#[inline]
fn GomSizeForMbWidth(kiMbWidth: i32) -> i32 {
    if kiMbWidth <= MB_WIDTH_THRESHOLD_90P {
        kiMbWidth * GOM_ROW_MODE0_90P
    } else if kiMbWidth <= MB_WIDTH_THRESHOLD_180P {
        kiMbWidth * GOM_ROW_MODE0_180P
    } else if kiMbWidth <= MB_WIDTH_THRESHOLD_360P {
        kiMbWidth * GOM_ROW_MODE0_360P
    } else {
        kiMbWidth * GOM_ROW_MODE0_720P
    }
}

/// `CheckFixedSliceNumMultiSliceSetting` — svc_enc_slice_segment.cpp:125.
///
/// Slice parameter check for `SM_FIXEDSLCNUM_SLICE`: divides the frame evenly and puts
/// the remainder in the last slice.
///
/// C++ aliases `uiSliceMbNum` (a `uint32_t[]`) through an `int32_t*`, so the
/// assignments and the `iNumMbLeft <= 0` test are signed; that is reproduced here.
///
/// # Safety
/// `pSliceArg` must be non-null, and `uiSliceNum` must be non-zero and no greater than
/// `uiSliceMbNum`'s length.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CheckFixedSliceNumMultiSliceSetting(
    kiMbNumInFrame: i32,
    pSliceArg: *mut SSliceArgument,
) -> bool {
    let pSlicesAssignList = (*pSliceArg).uiSliceMbNum.as_mut_ptr() as *mut i32;
    let kuiSliceNum = (*pSliceArg).uiSliceNum;
    let mut uiSliceIdx: u32 = 0;
    let kiMbNumPerSlice = kiMbNumInFrame / kuiSliceNum as i32;
    let mut iNumMbLeft = kiMbNumInFrame;

    // C++ null-checks pSlicesAssignList here; the array is inline in the struct, so
    // the pointer can never be null and the check is elided.

    while uiSliceIdx + 1 < kuiSliceNum {
        *pSlicesAssignList.add(uiSliceIdx as usize) = kiMbNumPerSlice;
        iNumMbLeft -= kiMbNumPerSlice;
        uiSliceIdx += 1;
    }

    *pSlicesAssignList.add(uiSliceIdx as usize) = iNumMbLeft;

    if iNumMbLeft <= 0 || kiMbNumPerSlice <= 0 {
        return false;
    }

    true
}

/// `CheckRowMbMultiSliceSetting` — svc_enc_slice_segment.cpp:150.
///
/// Slice parameter check for `SM_ROWMB_SLICE`: one macroblock row per slice.
///
/// # Safety
/// `pSliceArg` must be non-null and `uiSliceNum` no greater than `uiSliceMbNum`'s
/// length.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CheckRowMbMultiSliceSetting(kiMbWidth: i32, pSliceArg: *mut SSliceArgument) -> bool {
    let pSlicesAssignList = (*pSliceArg).uiSliceMbNum.as_mut_ptr() as *mut i32;
    let kuiSliceNum = (*pSliceArg).uiSliceNum;
    let mut uiSliceIdx: u32 = 0;

    while uiSliceIdx < kuiSliceNum {
        *pSlicesAssignList.add(uiSliceIdx as usize) = kiMbWidth;
        uiSliceIdx += 1;
    }
    true
}

/// `CheckRasterMultiSliceSetting` — svc_enc_slice_segment.cpp:166.
///
/// Slice parameter check for `SM_RASTER_SLICE`: walks the caller's per-slice
/// macroblock counts, then corrects the total to exactly `kiMbNumInFrame` and writes
/// back the resulting slice count.
///
/// # Safety
/// `pSliceArg` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CheckRasterMultiSliceSetting(
    kiMbNumInFrame: i32,
    pSliceArg: *mut SSliceArgument,
) -> bool {
    let pSlicesAssignList = (*pSliceArg).uiSliceMbNum.as_mut_ptr() as *mut i32;
    let mut iActualSliceCount: i32 = 0;

    // check mb_num setting
    let mut uiSliceIdx: u32 = 0;
    let mut iCountMb: i32 = 0;

    while (uiSliceIdx < MAX_SLICES_NUM as u32) && (0 < *pSlicesAssignList.add(uiSliceIdx as usize))
    {
        iCountMb += *pSlicesAssignList.add(uiSliceIdx as usize);
        iActualSliceCount = uiSliceIdx as i32 + 1;

        if iCountMb >= kiMbNumInFrame {
            break;
        }

        uiSliceIdx += 1;
    }
    // the break condition above guarantees iActualSliceCount <= MAX_SLICES_NUM here

    // correction if needed
    if iCountMb == kiMbNumInFrame {
        // nothing to do
    } else if iCountMb > kiMbNumInFrame {
        // setting is more than iMbNumInFrame: cut the last uiSliceMbNum, adjust iCountMb
        *pSlicesAssignList.add(iActualSliceCount as usize - 1) -= iCountMb - kiMbNumInFrame;
    } else if iActualSliceCount < MAX_SLICES_NUM as i32 {
        // iCountMb < iMbNumInFrame: make the last uiSliceMbNum the left num
        *pSlicesAssignList.add(iActualSliceCount as usize) = kiMbNumInFrame - iCountMb;
        iActualSliceCount += 1;
    } else {
        // iCountMb < iMbNumInFrame and iActualSliceCount == MAX_SLICES_NUM:
        // no more slices can be added
        return false;
    }

    (*pSliceArg).uiSliceNum = iActualSliceCount as u32;
    true
}

/// `GomValidCheckSliceNum` — svc_enc_slice_segment.cpp:218.
///
/// GOM-based RC decision for `uiSliceNum`, only used at `SM_FIXEDSLCNUM_SLICE`.
/// Returns false — and rewrites `*pSliceNum` — when the requested count does not fit.
///
/// # Safety
/// `pSliceNum` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GomValidCheckSliceNum(
    kiMbWidth: i32,
    kiMbHeight: i32,
    pSliceNum: *mut u32,
) -> bool {
    let kiCountNumMb = kiMbWidth * kiMbHeight;
    let mut iSliceNum: u32 = *pSliceNum;
    let iGomSize = GomSizeForMbWidth(kiMbWidth);

    loop {
        if kiCountNumMb < iGomSize * iSliceNum as i32 {
            iSliceNum -= 1;
            iSliceNum -= iSliceNum & 0x01; // verify even num for the multiple slices case
            if iSliceNum < 2 {
                // for safe
                break;
            }
            continue;
        }
        break;
    }

    if *pSliceNum != iSliceNum {
        *pSliceNum = if 0 != iSliceNum { iSliceNum } else { 1 };
        return false;
    }
    true
}

/// `GomValidCheckSliceMbNum` — svc_enc_slice_segment.cpp:255.
///
/// GOM-based RC decision for `uiSliceMbNum`, only used at `SM_FIXEDSLCNUM_SLICE`.
/// Assigns GOM-aligned macroblock counts to every slice but the last, which takes the
/// remainder.
///
/// # Safety
/// `pSliceArg` must be non-null with a non-zero `uiSliceNum` no greater than
/// `uiSliceMbNum`'s length.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GomValidCheckSliceMbNum(
    kiMbWidth: i32,
    kiMbHeight: i32,
    pSliceArg: *mut SSliceArgument,
) -> bool {
    let pSlicesAssignList = (*pSliceArg).uiSliceMbNum.as_mut_ptr();
    let kuiSliceNum = (*pSliceArg).uiSliceNum;
    let kiMbNumInFrame = kiMbWidth * kiMbHeight;
    let kiMbNumPerSlice = kiMbNumInFrame / kuiSliceNum as i32;
    let mut iNumMbLeft = kiMbNumInFrame;

    let mut iMaximalMbNum: i32; // dynamically assigned later
    let iGomSize = GomSizeForMbWidth(kiMbWidth);

    let mut uiSliceIdx: u32 = 0;

    // GOM boundary aligned
    let iNumMbAssigning =
        WELS_DIV_ROUND(INT_MULTIPLY * kiMbNumPerSlice, iGomSize * INT_MULTIPLY) * iGomSize;
    let mut iCurNumMbAssigning: i32;

    // C++ initialises iMinimalMbNum to kiMbWidth ("in theory we need only 1 SMB, here
    // let it as one SMB row required") and then immediately overwrites it with iGomSize.
    let iMinimalMbNum = iGomSize;
    // Ensure that the minimum macroblock requirement across all slices does not exceed
    // total frame capacity, preventing negative calculations for remaining macroblocks.
    if iMinimalMbNum * kuiSliceNum as i32 > kiMbNumInFrame {
        return false;
    }
    while uiSliceIdx + 1 < kuiSliceNum {
        // get maximal num_mb in the left parts
        iMaximalMbNum = iNumMbLeft - (kuiSliceNum - uiSliceIdx - 1) as i32 * iMinimalMbNum;
        // make sure one GOM at least in each slice for safe
        if iNumMbAssigning < iMinimalMbNum {
            iCurNumMbAssigning = iMinimalMbNum;
        } else if iNumMbAssigning > iMaximalMbNum {
            iCurNumMbAssigning = (iMaximalMbNum / iGomSize) * iGomSize;
        } else {
            iCurNumMbAssigning = iNumMbAssigning;
        }

        if iCurNumMbAssigning <= 0 {
            return false;
        }

        iNumMbLeft -= iCurNumMbAssigning;
        if iNumMbLeft <= 0 {
            return false;
        }

        *pSlicesAssignList.add(uiSliceIdx as usize) = iCurNumMbAssigning as u32;
        uiSliceIdx += 1;
    }
    *pSlicesAssignList.add(uiSliceIdx as usize) = iNumMbLeft as u32;
    if iNumMbLeft < iMinimalMbNum {
        return false;
    }

    true
}

/// `SliceArgumentValidationFixedSliceMode` — `encoder_ext.cpp:174`, declared in
/// `svc_enc_slice_segment.h`.
///
/// Validates and repairs an `SM_FIXEDSLCNUM_SLICE` argument, falling back to
/// `SM_SINGLE_SLICE` where the requested slicing cannot work.
///
/// # Safety
/// `pSliceArgument` must be non-null and writable.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn SliceArgumentValidationFixedSliceMode(
    _pLogCtx: *mut SLogContext,
    pSliceArgument: *mut SSliceArgument,
    kiRCMode: RC_MODES,
    kiPicWidth: i32,
    kiPicHeight: i32,
) -> i32 {
    let mut iCpuCores: i32 = 0;
    let iMbWidth = (kiPicWidth + 15) >> 4;
    let iMbHeight = (kiPicHeight + 15) >> 4;
    let iMbNumInFrame = iMbWidth * iMbHeight;
    let mut bSingleMode = false;

    (*pSliceArgument).uiSliceSizeConstraint = 0;

    if (*pSliceArgument).uiSliceNum == 0 {
        crate::decoder::decoder_core::WelsCPUFeatureDetect(&mut iCpuCores);
        if 0 == iCpuCores {
            // cpuid not supported, or doesn't expose the number of cores: use the
            // high-level system API to detect physical/logical processors
            iCpuCores = DynamicDetectCpuCores();
        }
        (*pSliceArgument).uiSliceNum = iCpuCores as u32;
    }

    if (*pSliceArgument).uiSliceNum <= 1 {
        bSingleMode = true;
    }

    // considering coding efficiency and performance, iCountMbNum is constrained by the
    // MIN_NUM_MB_PER_SLICE condition of the multi-slice mode setting
    if iMbNumInFrame <= MIN_NUM_MB_PER_SLICE {
        bSingleMode = true;
    }

    if bSingleMode {
        (*pSliceArgument).uiSliceMode = SM_SINGLE_SLICE;
        (*pSliceArgument).uiSliceNum = 1;
        for iIdx in 0..MAX_SLICES_NUM {
            (*pSliceArgument).uiSliceMbNum[iIdx] = 0;
        }
        return ENC_RETURN_SUCCESS;
    }

    if (*pSliceArgument).uiSliceNum > MAX_SLICES_NUM as u32 {
        (*pSliceArgument).uiSliceNum = MAX_SLICES_NUM as u32;
    }

    if kiRCMode != RC_OFF_MODE {
        // multiple slices verified with gom
        // check uiSliceNum and set uiSliceMbNum with the current uiSliceNum. C++ only
        // logs when this returns false; uiSliceNum has already been corrected in place.
        GomValidCheckSliceNum(iMbWidth, iMbHeight, &mut (*pSliceArgument).uiSliceNum);

        if (*pSliceArgument).uiSliceNum <= 1
            || !GomValidCheckSliceMbNum(iMbWidth, iMbHeight, pSliceArgument)
        {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
    } else if !CheckFixedSliceNumMultiSliceSetting(iMbNumInFrame, pSliceArgument) {
        // check uiSliceMbNum with the current uiSliceNum
        (*pSliceArgument).uiSliceMode = SM_SINGLE_SLICE;
        (*pSliceArgument).uiSliceNum = 1;
        for iIdx in 0..MAX_SLICES_NUM {
            (*pSliceArgument).uiSliceMbNum[iIdx] = 0;
        }
    }

    ENC_RETURN_SUCCESS
}

/// `AssignMbMapSingleSlice` — svc_enc_slice_segment.cpp:53.
///
/// # Safety
/// `pMbMap` must point to at least `kiCountMbNum * kiMapUnitSize` writable bytes.
/// (`*mut u16` since Phase 6 session B: the C++ takes `void*` and every caller
/// passes `pOverallMbMap`, which is `uint16_t*` at both ends.)
pub fn AssignMbMapSingleSlice(pMbMap: &[AtomicU16], kiCountMbNum: i32) -> i32 {
    if pMbMap.is_empty() || kiCountMbNum <= 0 {
        return 1;
    }

    let n = (kiCountMbNum as usize).min(pMbMap.len());
    for c in &pMbMap[..n] {
        c.store(0, Ordering::Relaxed);
    }

    0
}

/// A zeroed macroblock map of `kiCountMbNum` entries — the port's spelling of the
/// `WelsMallocz` the C++ carves the map with, now that the element is `AtomicU16`
/// and `vec![_; n]` cannot clone it (T9.C2).
fn new_mb_map(kiCountMbNum: i32) -> Vec<AtomicU16> {
    (0..kiCountMbNum.max(0) as usize).map(|_| AtomicU16::new(0)).collect()
}

/// `AssignMbMapMultipleSlices` — svc_enc_slice_segment.cpp:70.
///
/// Note the C++ returns **1** on the normal `SM_RASTER_SLICE`/`SM_FIXEDSLCNUM_SLICE`
/// path — the `return 0` is only in the `uiSliceMbNum[0] == 0` raster special case, and
/// the shared tail falls through to `return 1` with the comment "extention for other
/// multiple slice type in the future". `InitSliceSegment` returns that value directly,
/// so multi-slice `InitSlicePEncCtx` reports failure while still having filled the map.
/// Reproduced verbatim.
///
/// # Safety
/// `pCurDq` must be non-null with `sSliceEncCtx.pOverallMbMap` allocated.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AssignMbMapMultipleSlices(
    pCurDq: &mut SDqLayer,
    kpSliceArgument: *const SSliceArgument,
) -> i32 {
    // T9.E2h: a plain field borrow — the `as *mut` spelling made a raw whose
    // parent temporary expired at the statement (S29's cast clause); under the
    // `&mut` parameter the borrow checker referees the window instead.
    let pSliceSeg = &mut pCurDq.sSliceEncCtx;
    let mut iSliceIdx: i32;
    if (*pSliceSeg).uiSliceMode == SM_SINGLE_SLICE {
        return 1;
    }

    if (*pSliceSeg).uiSliceMode == SM_RASTER_SLICE && 0 == (*kpSliceArgument).uiSliceMbNum[0] {
        let kiMbWidth = (*pSliceSeg).iMbWidth as i32;
        let iSliceNum = (*pSliceSeg).iSliceNumInFrame.load(Ordering::Relaxed);

        iSliceIdx = 0;
        while iSliceIdx < iSliceNum {
            let kiFirstMb = iSliceIdx * kiMbWidth;
            let map: &[AtomicU16] = &(*pSliceSeg).pOverallMbMap;
            crate::encoder::slice_multi_threading::fill_mb_map(
                map,
                kiFirstMb,
                kiMbWidth,
                iSliceIdx as u16,
            );
            iSliceIdx += 1;
        }

        return 0;
    } else if (*pSliceSeg).uiSliceMode == SM_RASTER_SLICE
        || (*pSliceSeg).uiSliceMode == SM_FIXEDSLCNUM_SLICE
    {
        let kpSlicesAssignList = (*kpSliceArgument).uiSliceMbNum.as_ptr() as *const i32;
        let kiCountNumMbInFrame = (*pSliceSeg).iMbNumInFrame;
        let kiCountSliceNumInFrame = (*pSliceSeg).iSliceNumInFrame.load(Ordering::Relaxed);
        let mut iMbIdx: i32 = 0;

        iSliceIdx = 0;
        loop {
            let kiCurRunLength = *kpSlicesAssignList.add(iSliceIdx as usize);
            let mut iRunIdx: i32 = 0;

            // the mb_assign_map has to be validated against the input data here, so
            // this cannot be a memset
            loop {
                let map: &[AtomicU16] = &(*pSliceSeg).pOverallMbMap;
                map[(iMbIdx + iRunIdx) as usize].store(iSliceIdx as u16, Ordering::Relaxed);
                iRunIdx += 1;
                if !(iRunIdx < kiCurRunLength && iMbIdx + iRunIdx < kiCountNumMbInFrame) {
                    break;
                }
            }

            iMbIdx += kiCurRunLength;
            iSliceIdx += 1;
            if !(iSliceIdx < kiCountSliceNumInFrame && iMbIdx < kiCountNumMbInFrame) {
                break;
            }
        }
    } else if (*pSliceSeg).uiSliceMode == SM_SIZELIMITED_SLICE {
        // do nothing, pSliceSeg->pOverallMbMap will be initialised later
    } else {
        // any else uiSliceMode? C++ asserts here.
        debug_assert!(false, "AssignMbMapMultipleSlices: unexpected uiSliceMode");
    }

    // extention for other multiple slice type in the future
    1
}

/// `GetInitialSliceNum` — svc_enc_slice_segment.cpp:325.
///
/// # Safety
/// `pSliceArgument` may be null, which returns -1 as in C++.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetInitialSliceNum(pSliceArgument: *const SSliceArgument) -> i32 {
    if pSliceArgument.is_null() {
        return -1;
    }

    match (*pSliceArgument).uiSliceMode {
        SM_SINGLE_SLICE | SM_FIXEDSLCNUM_SLICE | SM_RASTER_SLICE => {
            (*pSliceArgument).uiSliceNum as i32
        }
        // at the beginning of dynamic slicing, set the uiSliceNum to be 1
        SM_SIZELIMITED_SLICE => AVERSLICENUM_CONSTRAINT as i32,
        _ => -1,
    }
}

/// `InitSliceSegment` — svc_enc_slice_segment.cpp:358.
///
/// # Safety
/// `pCurDq` and `pSliceArgument` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceSegment(
    pCurDq: &mut SDqLayer,
    pSliceArgument: *mut SSliceArgument,
    kiMbWidth: i32,
    kiMbHeight: i32,
) -> i32 {
    // T9.E2h: a plain field borrow — the `as *mut` spelling made a raw whose
    // parent temporary expired at the statement (S29's cast clause); under the
    // `&mut` parameter the borrow checker referees the window instead.
    let pSliceSeg = &mut pCurDq.sSliceEncCtx;
    let kiCountMbNum = kiMbWidth * kiMbHeight;

    if pSliceArgument.is_null() || kiMbWidth == 0 || kiMbHeight == 0 {
        return 1;
    }

    let uiSliceMode = (*pSliceArgument).uiSliceMode;
    if (*pSliceSeg).iMbNumInFrame == kiCountMbNum
        && (*pSliceSeg).iMbWidth as i32 == kiMbWidth
        && (*pSliceSeg).iMbHeight as i32 == kiMbHeight
        && (*pSliceSeg).uiSliceMode == uiSliceMode
        && !(*pSliceSeg).pOverallMbMap.is_empty()
    {
        return 0;
    } else if (*pSliceSeg).iMbNumInFrame != kiCountMbNum {
        (*pSliceSeg).pOverallMbMap = Vec::new();

        // just for safe
        (*pSliceSeg).iSliceNumInFrame.store(0, Ordering::Relaxed);
        (*pSliceSeg).iMbNumInFrame = 0;
        (*pSliceSeg).iMbWidth = 0;
        (*pSliceSeg).iMbHeight = 0;
        (*pSliceSeg).uiSliceMode = SM_SINGLE_SLICE; // single in default
    }

    if SM_SINGLE_SLICE == uiSliceMode {
        (*pSliceSeg).pOverallMbMap = new_mb_map(kiCountMbNum);
        (*pSliceSeg).iSliceNumInFrame.store(1, Ordering::Relaxed);

        (*pSliceSeg).uiSliceMode = uiSliceMode;
        (*pSliceSeg).iMbWidth = kiMbWidth as i16;
        (*pSliceSeg).iMbHeight = kiMbHeight as i16;
        (*pSliceSeg).iMbNumInFrame = kiCountMbNum;

        AssignMbMapSingleSlice(&(*pSliceSeg).pOverallMbMap, kiCountMbNum)
    } else {
        if uiSliceMode != SM_FIXEDSLCNUM_SLICE
            && uiSliceMode != SM_RASTER_SLICE
            && uiSliceMode != SM_SIZELIMITED_SLICE
        {
            return 1;
        }

        // `WelsMallocz` zeroed the block and the `WelsSetMemMultiplebytes_c` that
        // followed zeroed it again; a zeroed map is both.
        (*pSliceSeg).pOverallMbMap = new_mb_map(kiCountMbNum);

        // SM_SIZELIMITED_SLICE: init, set pSliceSeg->iSliceNumInFrame = 1
        (*pSliceSeg)
            .iSliceNumInFrame
            .store(GetInitialSliceNum(pSliceArgument), Ordering::Relaxed);
        if -1 == (*pSliceSeg).iSliceNumInFrame.load(Ordering::Relaxed) {
            return 1;
        }

        (*pSliceSeg).uiSliceMode = (*pSliceArgument).uiSliceMode;

        (*pSliceSeg).iMbWidth = kiMbWidth as i16;
        (*pSliceSeg).iMbHeight = kiMbHeight as i16;
        (*pSliceSeg).iMbNumInFrame = kiCountMbNum;
        if SM_SIZELIMITED_SLICE == (*pSliceArgument).uiSliceMode {
            if 0 < (*pSliceArgument).uiSliceSizeConstraint {
                (*pSliceSeg).uiSliceSizeConstraint = (*pSliceArgument).uiSliceSizeConstraint;
            } else {
                return 1;
            }
        } else {
            (*pSliceSeg).uiSliceSizeConstraint = DEFAULT_MAXPACKETSIZE_CONSTRAINT;
        }
        // "iMaxSliceNumConstraint" is only used in SM_SIZELIMITED_SLICE mode so far and
        // follows NAL_UNIT_CONSTRAINT; it will be adjusted under MT if there is a
        // limitation on iLayerNum.
        (*pSliceSeg).iMaxSliceNumConstraint = MAX_SLICES_NUM as i32;

        AssignMbMapMultipleSlices(pCurDq, pSliceArgument)
    }
}

/// `UninitSliceSegment` — svc_enc_slice_segment.cpp:449.
///
/// # Safety
/// `pCurDq` and `pMa` must be non-null.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UninitSliceSegment(pCurDq: &mut SDqLayer) {
    // T9.E2h: a plain field borrow — the `as *mut` spelling made a raw whose
    // parent temporary expired at the statement (S29's cast clause); under the
    // `&mut` parameter the borrow checker referees the window instead.
    let pSliceSeg = &mut pCurDq.sSliceEncCtx;
    // The map is a `Vec<u16>` since T6.D7 — clearing it releases the storage the
    // explicit `WelsFree` used to, and the layer's `Drop` covers the paths that never
    // reach here at all.
    (*pSliceSeg).pOverallMbMap = Vec::new();

    (*pSliceSeg).uiSliceMode = SM_SINGLE_SLICE; // single in default
    (*pSliceSeg).iMbWidth = 0;
    (*pSliceSeg).iMbHeight = 0;
    (*pSliceSeg).iSliceNumInFrame.store(0, Ordering::Relaxed);
    (*pSliceSeg).iMbNumInFrame = 0;
    (*pSliceSeg).uiSliceSizeConstraint = 0;
    (*pSliceSeg).iMaxSliceNumConstraint = 0;
}

/// `InitSlicePEncCtx` — svc_enc_slice_segment.cpp:482.
///
/// `bFmoUseFlag` and `pPpsArg` are accepted and unused, as in C++, and the return
/// value of `InitSliceSegment` is discarded — C++ returns a literal 0 here.
///
/// # Safety
/// `pCurDq` may be null, which returns 1 as in C++.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSlicePEncCtx(
    pCurDq: &mut SDqLayer,
    _bFmoUseFlag: bool,
    iMbWidth: i32,
    iMbHeight: i32,
    pSliceArgument: *mut SSliceArgument,
) -> i32 {
    InitSliceSegment(pCurDq, pSliceArgument, iMbWidth, iMbHeight);
    0
}

/// `UninitSlicePEncCtx` — svc_enc_slice_segment.cpp:508.
///
/// # Safety
/// `pMa` must be non-null when `pCurDq` is.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UninitSlicePEncCtx(pCurDq: &mut SDqLayer) {
    UninitSliceSegment(pCurDq);
}

/// `WelsGetFirstMbOfSlice` — svc_enc_slice_segment.cpp:540.
///
/// # Safety
/// `pCurLayer` may be null, which returns -1.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsGetFirstMbOfSlice(pCurLayer: &mut SDqLayer, kuiSliceIdc: i32) -> i32 {
    let first: &[i32] = &(*pCurLayer).pFirstMbIdxOfSlice;
    match first.get(kuiSliceIdc as usize) {
        Some(&v) => v,
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::codec_api::SliceModeEnum::SM_FIXEDSLCNUM_SLICE;

    fn arg(uiSliceNum: u32) -> SSliceArgument {
        let mut a = SSliceArgument::default();
        a.uiSliceMode = SM_FIXEDSLCNUM_SLICE;
        a.uiSliceNum = uiSliceNum;
        a
    }

    /// 4 slices over 99 MBs: 24 each for the first three, 27 for the last.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn fixed_slice_num_splits_evenly_with_remainder_last() {
        let mut a = arg(4);
        unsafe {
            assert!(CheckFixedSliceNumMultiSliceSetting(99, &mut a));
        }
        assert_eq!(&a.uiSliceMbNum[0..4], &[24, 24, 24, 27]);
    }

    /// More slices than macroblocks leaves nothing for the last slice.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn fixed_slice_num_rejects_when_a_slice_would_be_empty() {
        let mut a = arg(8);
        unsafe {
            assert!(!CheckFixedSliceNumMultiSliceSetting(4, &mut a));
        }
    }

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn row_mb_assigns_one_row_per_slice() {
        let mut a = arg(3);
        unsafe {
            assert!(CheckRowMbMultiSliceSetting(10, &mut a));
        }
        assert_eq!(&a.uiSliceMbNum[0..3], &[10, 10, 10]);
    }

    /// A short total gets a trailing slice carrying the remainder, and uiSliceNum is
    /// rewritten to the actual count.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn raster_pads_a_short_assignment() {
        let mut a = arg(2);
        a.uiSliceMbNum[0] = 30;
        a.uiSliceMbNum[1] = 30;
        unsafe {
            assert!(CheckRasterMultiSliceSetting(99, &mut a));
        }
        assert_eq!(a.uiSliceNum, 3);
        assert_eq!(&a.uiSliceMbNum[0..3], &[30, 30, 39]);
    }

    /// An over-long total is trimmed on the last used slice.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn raster_trims_an_over_long_assignment() {
        let mut a = arg(2);
        a.uiSliceMbNum[0] = 60;
        a.uiSliceMbNum[1] = 60;
        unsafe {
            assert!(CheckRasterMultiSliceSetting(99, &mut a));
        }
        assert_eq!(a.uiSliceNum, 2);
        assert_eq!(&a.uiSliceMbNum[0..2], &[60, 39]);
    }

    /// 160x96 is 10x6 MBs; MB width 10 <= MB_WIDTH_THRESHOLD_90P so the GOM is
    /// 10 * GOM_ROW_MODE0_90P = 20 MBs, and 60 MBs cannot carry 4 slices of one GOM.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn gom_slice_num_reduces_to_an_even_count_that_fits() {
        let mut uiSliceNum: u32 = 4;
        unsafe {
            assert!(!GomValidCheckSliceNum(10, 6, &mut uiSliceNum));
        }
        assert_eq!(uiSliceNum, 2);
    }

    /// A count that already fits is returned unchanged and reports success.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn gom_slice_num_accepts_a_fitting_count() {
        let mut uiSliceNum: u32 = 2;
        unsafe {
            assert!(GomValidCheckSliceNum(10, 6, &mut uiSliceNum));
        }
        assert_eq!(uiSliceNum, 2);
    }

    /// The minimum-per-slice guard added upstream: 4 slices x 20-MB GOM = 80 > 60 MBs.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn gom_slice_mb_num_rejects_when_minimums_exceed_the_frame() {
        let mut a = arg(4);
        unsafe {
            assert!(!GomValidCheckSliceMbNum(10, 6, &mut a));
        }
    }

    /// 2 slices over 10x12 MBs: GOM is 20 MBs, 120/2 = 60 rounds to 60, leaving 60.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn gom_slice_mb_num_assigns_gom_aligned_counts() {
        let mut a = arg(2);
        unsafe {
            assert!(GomValidCheckSliceMbNum(10, 12, &mut a));
        }
        assert_eq!(&a.uiSliceMbNum[0..2], &[60, 60]);
    }

    /// 128x96 is 8x6 = 48 MBs, exactly MIN_NUM_MB_PER_SLICE, so fixed-slice mode falls
    /// back to a single slice (`encoder_ext.cpp:205`).
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn fixed_slice_mode_falls_back_to_single_slice_at_min_mb_count() {
        let mut a = arg(2);
        unsafe {
            let ret = SliceArgumentValidationFixedSliceMode(
                std::ptr::null_mut(),
                &mut a,
                RC_OFF_MODE,
                128,
                96,
            );
            assert_eq!(ret, ENC_RETURN_SUCCESS);
        }
        assert_eq!(a.uiSliceMode, SM_SINGLE_SLICE);
        assert_eq!(a.uiSliceNum, 1);
    }

    /// uiSliceNum above MAX_SLICES_NUM is clamped rather than rejected
    /// (`encoder_ext.cpp:221`).
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn fixed_slice_mode_clamps_slice_num_to_max() {
        let mut a = arg(MAX_SLICES_NUM as u32 + 10);
        unsafe {
            let ret = SliceArgumentValidationFixedSliceMode(
                std::ptr::null_mut(),
                &mut a,
                RC_OFF_MODE,
                1280,
                720,
            );
            assert_eq!(ret, ENC_RETURN_SUCCESS);
        }
        assert!(a.uiSliceNum <= MAX_SLICES_NUM as u32);
    }
}
