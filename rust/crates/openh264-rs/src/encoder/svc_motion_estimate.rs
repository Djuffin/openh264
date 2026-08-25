// Copyright (c) 2009-2013, Cisco Systems
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

//! # Motion Estimation (ME) Engine
//!
//! Translated from `codec/encoder/core/inc/svc_motion_estimate.h` and
//! `codec/encoder/core/src/svc_motion_estimate.cpp`.
//!
//! Implements multi-candidate initial point testing, small diamond search (`ME_DIA`),
//! 1D orthogonal cross line full search (`ME_CROSS`), and hash-based feature search (`ME_FME`)
//! for screen content coding and real-time H.264 / AVC video encoding.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    clippy::too_many_arguments
)]

#![deny(unsafe_code)]

use crate::encoder::svc_encode_slice::layer_ref_pic;
use crate::encoder::picture::{RecPicId};
use crate::safe::plane::PlaneCursor;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::picture::SScreenBlockFeatureStorage;
pub use crate::encoder::md::SSampleDealingFunc;
pub use crate::encoder::slice_multi_threading::SSliceCtx;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

// ============================================================================
// Constants, Limits, and Enums
// ============================================================================

pub const CAMERA_STARTMV_RANGE: i32 = 64;
pub const ITERATIVE_TIMES: i32 = 16;
pub const CAMERA_MV_RANGE: i32 = CAMERA_STARTMV_RANGE + ITERATIVE_TIMES; // 80
pub const CAMERA_MVD_RANGE: i32 = (CAMERA_MV_RANGE + 1) << 1; // 162
pub const MB_WIDTH_LUMA: i32 = 16;
pub const BASE_MV_MB_NMB: i32 = (2 * CAMERA_MV_RANGE / MB_WIDTH_LUMA) - 1; // 9
pub const CAMERA_HIGHLAYER_MVD_RANGE: i32 = 243;
pub const EXPANDED_MV_RANGE: i32 = 504;
pub const EXPANDED_MVD_RANGE: i32 = (504 + 1) << 1; // 1010
pub const INTPEL_NEEDED_MARGIN: i32 = 3;
pub const MAX_VERTICAL_MV_RANGE: i32 = 1024;

// Motion Search Method Bitmask Flags
pub const ME_DIA: u32 = 0x01;
pub const ME_CROSS: u32 = 0x02;
pub const ME_FME: u32 = 0x04;
pub const ME_FULL: u32 = 0x10;
pub const ME_DIA_CROSS: u32 = ME_DIA | ME_CROSS; // 0x03
pub const ME_DIA_CROSS_FME: u32 = ME_DIA_CROSS | ME_FME; // 0x07

// Feature Search Hash and Threshold Constants
pub const LIST_SIZE_SUM_16x16: usize = 0x0FF01; // 65281
pub const LIST_SIZE_SUM_8x8: usize = 0x03FC1; // 16321
pub const LIST_SIZE_MSE_16x16: usize = 0x00878; // 2168
pub const LIST_SIZE: i32 = 0x10000; // 65536

pub const FME_DEFAULT_FEATURE_INDEX: i32 = 0;
pub const FMESWITCH_DEFAULT_GOODFRAME_NUM: u8 = 2;
pub const FMESWITCH_MBSAD_THRESHOLD: i32 = 30;
pub const FMESWITCH_MBAVERCOSTSAVING_THRESHOLD: u32 = 2;
pub const FMESWITCH_GOODFRAMECOUNT_MAX: u8 = 5;

// Block Sizes
pub const BLOCK_16x16: usize = 0;
pub const BLOCK_16x8: usize = 1;
pub const BLOCK_8x16: usize = 2;
pub const BLOCK_8x8: usize = 3;
pub const BLOCK_4x4: usize = 4;
pub const BLOCK_8x4: usize = 5;
pub const BLOCK_4x8: usize = 6;
pub const BLOCK_SIZE_ALL: usize = 7;

// Return Codes
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNSUPPORTED_PARA: i32 = 0x02;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;

// CPU Capability Bit Flags

/// Quantization Step Lookup Table ($16 \times Q_{\text{step}}$ for $\text{QP} \in [0, 51]$)
pub static QStepx16ByQp: [i32; 52] = [
    10, 11, 13, 14, 16, 18,
    20, 22, 26, 28, 32, 36,
    40, 44, 52, 56, 64, 72,
    80, 88, 104, 112, 128, 144,
    160, 176, 208, 224, 256, 288,
    320, 352, 416, 448, 512, 576,
    640, 704, 832, 896, 1024, 1152,
    1280, 1408, 1664, 1792, 2048, 2304,
    2560, 2816, 3328, 3584,
];

// ============================================================================
// Core Data Structures
// ============================================================================

/// 2D Motion Vector displacement in integer or 1/4-pel units.

/// Dual-use union storing predicted SAD threshold before search and SATD after search.
#[repr(C)]
#[derive(Copy, Clone)]
pub union SadPredISatdUnit {
    pub uiSadPred: u32,
    pub uiSatd: u32,
}

impl Default for SadPredISatdUnit {
    fn default() -> Self {
        Self { uiSadPred: 0 }
    }
}

/// Reference frame screen block feature storage and hash lookup index.

/// Central working state structure passed across all motion estimation search routines.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsME {
    pub pMvdCost: *mut u16,
    pub uSadPredISatd: SadPredISatdUnit,
    pub uiSadCost: u32,
    pub uiSatdCost: u32,
    pub uiSadCostThreshold: u32,
    pub iCurMeBlockPixX: i32,
    pub iCurMeBlockPixY: i32,
    pub uiBlockSize: u8,
    pub uiReserved: u8,

    pub pEncMb: *mut u8,
    pub pRefMb: *mut u8,
    pub pColoRefMb: *mut u8,

    pub sMvp: SMVUnitXY,
    pub sMvBase: SMVUnitXY,
    pub sDirectionalMv: SMVUnitXY,

    // SCREEN_CONTENT(dormant: Phase 10)
    pub pRefFeatureStorage: *mut SScreenBlockFeatureStorage,

    pub sMv: SMVUnitXY,
}

impl Default for SWelsME {
    fn default() -> Self {
        Self {
            pMvdCost: std::ptr::null_mut(),
            uSadPredISatd: SadPredISatdUnit::default(),
            uiSadCost: 0,
            uiSatdCost: 0,
            uiSadCostThreshold: 0,
            iCurMeBlockPixX: 0,
            iCurMeBlockPixY: 0,
            uiBlockSize: 0,
            uiReserved: 0,
            pEncMb: std::ptr::null_mut(),
            pRefMb: std::ptr::null_mut(),
            pColoRefMb: std::ptr::null_mut(),
            sMvp: SMVUnitXY::default(),
            sMvBase: SMVUnitXY::default(),
            sDirectionalMv: SMVUnitXY::default(),
            pRefFeatureStorage: std::ptr::null_mut(),
            sMv: SMVUnitXY::default(),
        }
    }
}

/// Input configuration block for the hash-based feature search engine.
// SCREEN_CONTENT(dormant: Phase 10)
#[repr(C)]
pub struct SFeatureSearchIn {
    pub pSad: Option<PSampleSadSatdCostFuncRaw>,
    pub pTimesOfFeature: *mut u32,
    pub pQpelLocationOfFeature: *mut *mut u16,
    pub pMvdCostX: *mut u16,
    pub pMvdCostY: *mut u16,
    pub pEnc: *mut u8,
    pub pColoRef: *mut u8,
    pub iEncStride: i32,
    pub iRefStride: i32,
    pub uiSadCostThresh: u16,
    pub iFeatureOfCurrent: i32,
    pub iCurPixX: i32,
    pub iCurPixY: i32,
    pub iCurPixXQpel: i32,
    pub iCurPixYQpel: i32,
    pub iMinQpelX: i32,
    pub iMinQpelY: i32,
    pub iMaxQpelX: i32,
    pub iMaxQpelY: i32,
}

impl Default for SFeatureSearchIn {
    fn default() -> Self {
        Self {
            pSad: None,
            pTimesOfFeature: std::ptr::null_mut(),
            pQpelLocationOfFeature: std::ptr::null_mut(),
            pMvdCostX: std::ptr::null_mut(),
            pMvdCostY: std::ptr::null_mut(),
            pEnc: std::ptr::null_mut(),
            pColoRef: std::ptr::null_mut(),
            iEncStride: 0,
            iRefStride: 0,
            uiSadCostThresh: 0,
            iFeatureOfCurrent: 0,
            iCurPixX: 0,
            iCurPixY: 0,
            iCurPixXQpel: 0,
            iCurPixYQpel: 0,
            iMinQpelX: 0,
            iMinQpelY: 0,
            iMaxQpelX: 0,
            iMaxQpelY: 0,
        }
    }
}

/// Output container populated during feature search passes.
// SCREEN_CONTENT(dormant: Phase 10)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SFeatureSearchOut {
    pub sBestMv: SMVUnitXY,
    pub uiBestSadCost: u32,
    pub pBestRef: *mut u8,
}

impl Default for SFeatureSearchOut {
    fn default() -> Self {
        Self {
            sBestMv: SMVUnitXY::default(),
            uiBestSadCost: 0,
            pBestRef: std::ptr::null_mut(),
        }
    }
}

/// Reconstructed or reference picture frame buffer descriptor.

/// Slice context parameters for motion estimation.


/// Slice context container.

/// Spatial dependency layer representation in Scalable Video Coding.


// ============================================================================
// Function Pointer Types
// ============================================================================

// `PSampleSadSatdCostFunc` was declared here a second time (`md.rs` is the other),
// and `census_allowlist.txt` carried the pair as `alias PSampleSadSatdCostFunc x2`.
// **T9.B25**: one declaration, in `md.rs`, re-exported here — the slot type is
// safe now and a second spelling of it is exactly the divergence the census exists
// to catch (the allowlist entry retires with the duplicate).
pub use crate::encoder::md::{PSampleSadSatdCostFunc, PSampleSadSatdCostFuncRaw};

/// `PSample4SadCostFunc` — the four-candidate SAD the diamond search steps with:
/// `sample1`'s block against `sample2`'s at each whole-sample neighbour, written to
/// `sad[0..4]` in the order **up, down, left, right**
/// (`common/sad_common.rs::sample_sad_four::<W, H>`).
///
/// Safe since T9.B25; see [`PSampleSadSatdCostFunc`] for the rule and
/// [`PSample4SadCostFuncRaw`] for the shape this replaces.
pub type PSample4SadCostFunc = fn(&PlaneCursor<'_>, &PlaneCursor<'_>, &mut [i32; 4]);

/// The raw four-candidate shape — transitional, as [`PSampleSadSatdCostFuncRaw`].
pub type PSample4SadCostFuncRaw = unsafe extern "C" fn(
    pEnc: *mut u8,
    iEncStride: i32,
    pRef: *mut u8,
    iRefStride: i32,
    pSadCosts: *mut i32,
);

pub type PSampleSadHor8Func = unsafe extern "C" fn(
    pEnc: *mut u8,
    iEncStride: i32,
    pRef: *mut u8,
    iRefStride: i32,
    pBaseCost: *mut u16,
    pMinIndex: *mut i32,
) -> u32;

pub type PMotionSearchFunc = unsafe extern "C" fn(
    pFuncList: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
);

pub type PSearchMethodFunc = unsafe extern "C" fn(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    kiEncStride: i32,
    kiRefStride: i32,
);

pub type PCalculateSatdFunc = unsafe extern "C" fn(
    pSatd: Option<PSampleSadSatdCostFuncRaw>,
    pMe: &mut SWelsME,
    kiEncStride: i32,
    kiRefStride: i32,
);

pub type PCheckDirectionalMv = unsafe extern "C" fn(
    pSad: Option<PSampleSadSatdCostFuncRaw>,
    pMe: &mut SWelsME,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    kiEncStride: i32,
    kiRefStride: i32,
    iBestSadCost: &mut i32,
) -> bool;

pub type PLineFullSearchFunc = unsafe extern "C" fn(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pMvdTable: *mut u16,
    kiEncStride: i32,
    kiRefStride: i32,
    kiMinMv: i16,
    kiMaxMv: i16,
    bVerticalSearch: bool,
);

pub type PInitializeHashforFeatureFunc = unsafe extern "C" fn(
    pTimesOfFeatureValue: *mut u32,
    pBuf: *mut u16,
    kiListSize: i32,
    pLocationOfFeature: *mut *mut u16,
    pFeatureValuePointerList: *mut *mut u16,
);

pub type PFillQpelLocationByFeatureValueFunc = unsafe extern "C" fn(
    pFeatureOfBlock: *mut u16,
    kiWidth: i32,
    kiHeight: i32,
    pFeatureValuePointerList: *mut *mut u16,
);

pub type PCalculateBlockFeatureOfFrame = unsafe extern "C" fn(
    pRef: *mut u8,
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: *mut u16,
    pTimesOfFeatureValue: *mut u32,
);

pub type PCalculateSingleBlockFeature =
    unsafe extern "C" fn(pRef: *mut u8, kiRefStride: i32) -> i32;

pub type PUpdateFMESwitch = unsafe extern "C" fn(pCurLayer: *mut SDqLayer);





// ============================================================================
// Helper Macros and Inline Functions
// ============================================================================

/// Calculates MVD rate cost: `table[mx] + table[my]`.
#[inline(always)]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn COST_MVD(table: *const u16, mx: i32, my: i32) -> u32 {
    unsafe { (*table.offset(mx as isize) as u32) + (*table.offset(my as isize) as u32) }
}

#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateMeResults(
    ksBestMv: SMVUnitXY,
    kiBestSadCost: u32,
    pRef: *mut u8,
    pMe: &mut SWelsME,
) {
    unsafe {
        (*pMe).sMv = ksBestMv;
        (*pMe).pRefMb = pRef;
        (*pMe).uiSadCost = kiBestSadCost;
    }
}

#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn MeEndIntepelSearch(pMe: &mut SWelsME) {
    unsafe {
        (*pMe).sMv.iMvX *= 1 << 2;
        (*pMe).sMv.iMvY *= 1 << 2;
        (*pMe).uiSatdCost = (*pMe).uiSadCost;
    }
}

#[inline]
pub fn CheckInRangeCloseOpen(iVal: i16, iMin: i16, iMax: i16) -> bool {
    iVal >= iMin && iVal < iMax
}

#[inline]
pub fn CheckMvInRange(ksCurrentMv: SMVUnitXY, ksMinMv: SMVUnitXY, ksMaxMv: SMVUnitXY) -> bool {
    CheckInRangeCloseOpen(ksCurrentMv.iMvX, ksMinMv.iMvX, ksMaxMv.iMvX)
        && CheckInRangeCloseOpen(ksCurrentMv.iMvY, ksMinMv.iMvY, ksMaxMv.iMvY)
}

#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn SetMvWithinIntegerMvRange(
    kiMbWidth: i32,
    kiMbHeight: i32,
    kiMbX: i32,
    kiMbY: i32,
    kiMaxMvRange: i32,
    pMvMin: &mut SMVUnitXY,
    pMvMax: &mut SMVUnitXY,
) {
    unsafe {
        (*pMvMin).iMvX = ((-1 * ((kiMbX + 1) * (1 << 4)) + INTPEL_NEEDED_MARGIN)).max(-1 * kiMaxMvRange) as i16;
        (*pMvMin).iMvY = ((-1 * ((kiMbY + 1) * (1 << 4)) + INTPEL_NEEDED_MARGIN)).max(-1 * kiMaxMvRange) as i16;
        (*pMvMax).iMvX = (((kiMbWidth - kiMbX) * (1 << 4)) - INTPEL_NEEDED_MARGIN).min(kiMaxMvRange) as i16;
        (*pMvMax).iMvY = (((kiMbHeight - kiMbY) * (1 << 4)) - INTPEL_NEEDED_MARGIN).min(kiMaxMvRange) as i16;
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
#[inline]
pub fn CalcFMESwitchFlag(
    uiFMEGoodFrameCount: u8,
    _iHighFreMbPrecentage: i32,
    iAvgMbSAD: i32,
    bScrollingDetected: bool,
) -> bool {
    bScrollingDetected || (uiFMEGoodFrameCount > 0 && iAvgMbSAD > FMESWITCH_MBSAD_THRESHOLD)
}

#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetCurrentSliceNum(pCurDq: *const SDqLayer) -> i32 {
    if pCurDq.is_null() {
        -1
    } else {
        unsafe { (*pCurDq).sSliceEncCtx.iSliceNumInFrame.load(std::sync::atomic::Ordering::Relaxed) }
    }
}

// ============================================================================
// Initialization and Dispatch
// ============================================================================

/// Populates motion estimation function pointer table based on CPU capabilities and content type.
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsInitMeFunc(
    pFuncList: *mut SWelsFuncPtrList,
    uiCpuFlag: u32,
    bScreenContent: bool,
) {
    if pFuncList.is_null() {
        return;
    }
    unsafe {
        (*pFuncList).pfUpdateFMESwitch = Some(UpdateFMESwitchNull);

        if !bScreenContent {
            (*pFuncList).pfCheckDirectionalMv = Some(CheckDirectionalMvFalse);
            (*pFuncList).pfCalculateBlockFeatureOfFrame[0] = None;
            (*pFuncList).pfCalculateBlockFeatureOfFrame[1] = None;
            (*pFuncList).pfCalculateSingleBlockFeature[0] = None;
            (*pFuncList).pfCalculateSingleBlockFeature[1] = None;
        } else {
            (*pFuncList).pfCheckDirectionalMv = Some(CheckDirectionalMv);

            // Cross Search
            (*pFuncList).pfVerticalFullSearch = Some(LineFullSearch_c);
            (*pFuncList).pfHorizontalFullSearch = Some(LineFullSearch_c);

            // Feature Search
            (*pFuncList).pfInitializeHashforFeature = Some(InitializeHashforFeature_c);
            (*pFuncList).pfFillQpelLocationByFeatureValue = Some(FillQpelLocationByFeatureValue_c);
            (*pFuncList).pfCalculateBlockFeatureOfFrame[0] = Some(SumOf8x8BlockOfFrame_c);
            (*pFuncList).pfCalculateBlockFeatureOfFrame[1] = Some(SumOf16x16BlockOfFrame_c);
            (*pFuncList).pfCalculateSingleBlockFeature[0] = Some(SumOf8x8SingleBlock_c);
            (*pFuncList).pfCalculateSingleBlockFeature[1] = Some(SumOf16x16SingleBlock_c);
        }
    }
}

// ============================================================================
// Top-Level Motion Estimation Search Routines
// ============================================================================

/// Top-level motion estimation search for a macroblock or sub-partition.
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMotionEstimateSearch(
    pFuncList: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
) {
    unsafe {
        let kiStrideEnc = (*pCurDqLayer).iEncStride[0];
        let kiStrideRef = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[0];

        if crate::encoder::dump_enabled(&ME_DUMP, "OH264_MEDUMP") {
            let mut mvc = String::new();
            for di in 0..(*pSlice).uiMvcNum as usize {
                mvc.push_str(&format!(
                    "{}/{},",
                    (*pSlice).sMvc[di].iMvX,
                    (*pSlice).sMvc[di].iMvY
                ));
            }
            let mut enc = String::new();
            let mut rf = String::new();
            let mut rfup = String::new();
            for di in 0..8isize {
                enc.push_str(&format!("{},", *(*pMe).pEncMb.offset(di)));
                rf.push_str(&format!("{},", *(*pMe).pRefMb.offset(di)));
                rfup.push_str(&format!("{},", *(*pMe).pRefMb.offset(di - kiStrideRef as isize)));
            }
            eprintln!(
                "ME bs={} px={},{} mvp={},{} base={},{} sadpred={} mvcn={} min={},{} max={},{} mvc={} enc={} ref={} refup={} mvdc={},{}",
                (*pMe).uiBlockSize,
                (*pMe).iCurMeBlockPixX,
                (*pMe).iCurMeBlockPixY,
                (*pMe).sMvp.iMvX,
                (*pMe).sMvp.iMvY,
                (*pMe).sMvBase.iMvX,
                (*pMe).sMvBase.iMvY,
                (*pMe).uSadPredISatd.uiSadPred,
                (*pSlice).uiMvcNum,
                (*pSlice).sMvStartMin.iMvX,
                (*pSlice).sMvStartMin.iMvY,
                (*pSlice).sMvStartMax.iMvX,
                (*pSlice).sMvStartMax.iMvY,
                mvc,
                enc,
                rf,
                rfup,
                *(*pMe).pMvdCost.offset(0),
                *(*pMe).pMvdCost.offset(4),
            );
        }

        // Step 1: Initial point prediction
        if !WelsMotionEstimateInitialPoint(pFuncList, pMe, pSlice, kiStrideEnc, kiStrideRef) {
            let block_size = (*pMe).uiBlockSize as usize;
            if let Some(search_fn) = (*pFuncList).pfSearchMethod[block_size] {
                search_fn(pFuncList, pMe, pSlice, kiStrideEnc, kiStrideRef);
            }
            MeEndIntepelSearch(pMe);
        }

        let block_size = (*pMe).uiBlockSize as usize;
        if let Some(calc_satd) = (*pFuncList).pfCalculateSatd {
            calc_satd(
                (*pFuncList).sSampleDealingFuncs.pfSampleSatdRaw[block_size],
                pMe,
                kiStrideEnc,
                kiStrideRef,
            );
        }
        if crate::encoder::dump_enabled(&ME_DUMP, "OH264_MEDUMP") {
            eprintln!(
                "ME> mv={},{} sad={} satd={}",
                (*pMe).sMv.iMvX,
                (*pMe).sMv.iMvY,
                (*pMe).uiSadCost,
                (*pMe).uiSatdCost
            );
        }
    }
}

/// Shortcut motion estimation search for static macroblocks (forced MV = (0,0)).
// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMotionEstimateSearchStatic(
    pFuncList: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMe: &mut SWelsME,
    _pSlice: &mut SSlice,
) {
    unsafe {
        let kiStrideEnc = (*pCurDqLayer).iEncStride[0];
        let kiStrideRef = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[0];
        let block_size = (*pMe).uiBlockSize as usize;

        (*pMe).sMv.iMvX = 0;
        (*pMe).sMv.iMvY = 0;

        if let Some(sad_fn) = (*pFuncList).sSampleDealingFuncs.pfSampleSadRaw[block_size] {
            (*pMe).uiSadCost = sad_fn((*pMe).pEncMb, kiStrideEnc, (*pMe).pRefMb, kiStrideRef) as u32;
        }
        (*pMe).uiSadCost += COST_MVD((*pMe).pMvdCost, -((*pMe).sMvp.iMvX as i32), -((*pMe).sMvp.iMvY as i32));

        MeEndIntepelSearch(pMe);

        if let Some(calc_satd) = (*pFuncList).pfCalculateSatd {
            calc_satd(
                (*pFuncList).sSampleDealingFuncs.pfSampleSatdRaw[block_size],
                pMe,
                kiStrideEnc,
                kiStrideRef,
            );
        }
    }
}

/// Shortcut motion estimation search for scrolled macroblocks.
// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMotionEstimateSearchScrolled(
    pFuncList: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMe: &mut SWelsME,
    _pSlice: &mut SSlice,
) {
    unsafe {
        let kiStrideEnc = (*pCurDqLayer).iEncStride[0];
        let kiStrideRef = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[0];
        let block_size = (*pMe).uiBlockSize as usize;

        (*pMe).sMv = (*pMe).sDirectionalMv;
        let mv_x = (*pMe).sMv.iMvX as i32;
        let mv_y = (*pMe).sMv.iMvY as i32;
        (*pMe).pRefMb = (*pMe).pColoRefMb.offset((mv_y * kiStrideRef + mv_x) as isize);

        let mut sad_cost = 0u32;
        if let Some(sad_fn) = (*pFuncList).sSampleDealingFuncs.pfSampleSadRaw[block_size] {
            sad_cost = sad_fn((*pMe).pEncMb, kiStrideEnc, (*pMe).pRefMb, kiStrideRef) as u32;
        }
        sad_cost += COST_MVD(
            (*pMe).pMvdCost,
            (mv_x * 4) - ((*pMe).sMvp.iMvX as i32),
            (mv_y * 4) - ((*pMe).sMvp.iMvY as i32),
        );
        (*pMe).uiSadCost = sad_cost;

        MeEndIntepelSearch(pMe);

        if let Some(calc_satd) = (*pFuncList).pfCalculateSatd {
            calc_satd(
                (*pFuncList).sSampleDealingFuncs.pfSampleSatdRaw[block_size],
                pMe,
                kiStrideEnc,
                kiStrideRef,
            );
        }
    }
}

// ============================================================================
// Initial Candidate Prediction
// ============================================================================

/// Evaluates spatial MVP, MVC candidate list, and directional scrolling vectors.
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMotionEstimateInitialPoint(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    iStrideEnc: i32,
    iStrideRef: i32,
) -> bool {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad = (*pFuncList).sSampleDealingFuncs.pfSampleSadRaw[block_size];
        let kpMvdCost = (*pMe).pMvdCost;
        let kpEncMb = (*pMe).pEncMb;

        let kuiMvcNum = (*pSlice).uiMvcNum as usize;
        let ksMvStartMin = (*pSlice).sMvStartMin;
        let ksMvStartMax = (*pSlice).sMvStartMax;
        let ksMvp = (*pMe).sMvp;

        let mut sMv = SMVUnitXY {
            iMvX: (((2 + ksMvp.iMvX as i32) >> 2).clamp(ksMvStartMin.iMvX as i32, ksMvStartMax.iMvX as i32)) as i16,
            iMvY: (((2 + ksMvp.iMvY as i32) >> 2).clamp(ksMvStartMin.iMvY as i32, ksMvStartMax.iMvY as i32)) as i16,
        };

        let mut pRefMb = (*pMe).pRefMb.offset((sMv.iMvY as i32 * iStrideRef + sMv.iMvX as i32) as isize);

        let mut iBestSadCost: i32 = 0;
        if let Some(sad_fn) = pSad {
            iBestSadCost = sad_fn(kpEncMb, iStrideEnc, pRefMb, iStrideRef);
        }
        iBestSadCost += COST_MVD(
            kpMvdCost,
            (sMv.iMvX as i32 * 4) - ksMvp.iMvX as i32,
            (sMv.iMvY as i32 * 4) - ksMvp.iMvY as i32,
        ) as i32;

        let mut iSadCost: i32 = 0;
        for i in 0..kuiMvcNum {
            let mvc = (*pSlice).sMvc[i];
            let iMvc0 = (((2 + mvc.iMvX as i32) >> 2).clamp(ksMvStartMin.iMvX as i32, ksMvStartMax.iMvX as i32)) as i16;
            let iMvc1 = (((2 + mvc.iMvY as i32) >> 2).clamp(ksMvStartMin.iMvY as i32, ksMvStartMax.iMvY as i32)) as i16;

            if (iMvc0 != sMv.iMvX) || (iMvc1 != sMv.iMvY) {
                let pFref2 = (*pMe).pRefMb.offset((iMvc1 as i32 * iStrideRef + iMvc0 as i32) as isize);
                if let Some(sad_fn) = pSad {
                    iSadCost = sad_fn(kpEncMb, iStrideEnc, pFref2, iStrideRef);
                }
                iSadCost += COST_MVD(
                    kpMvdCost,
                    (iMvc0 as i32 * 4) - ksMvp.iMvX as i32,
                    (iMvc1 as i32 * 4) - ksMvp.iMvY as i32,
                ) as i32;

                if iSadCost < iBestSadCost {
                    sMv.iMvX = iMvc0;
                    sMv.iMvY = iMvc1;
                    pRefMb = pFref2;
                    iBestSadCost = iSadCost;
                }
            }
        }

        if let Some(check_dir) = (*pFuncList).pfCheckDirectionalMv {
            if check_dir(pSad, pMe, ksMvStartMin, ksMvStartMax, iStrideEnc, iStrideRef, &mut iSadCost) {
                sMv = (*pMe).sDirectionalMv;
                pRefMb = (*pMe).pColoRefMb.offset((sMv.iMvY as i32 * iStrideRef + sMv.iMvX as i32) as isize);
                iBestSadCost = iSadCost;
            }
        }

        UpdateMeResults(sMv, iBestSadCost as u32, pRefMb, pMe);

        if iBestSadCost < (*pMe).uSadPredISatd.uiSadPred as i32 {
            MeEndIntepelSearch(pMe);
            return true;
        }

        false
    }
}

// ============================================================================
// SATD Cost Calculation
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn CalculateSatdCost(
    pSatd: Option<PSampleSadSatdCostFuncRaw>,
    pMe: &mut SWelsME,
    kiEncStride: i32,
    kiRefStride: i32,
) {
    unsafe {
        if let Some(satd_fn) = pSatd {
            (*pMe).uSadPredISatd.uiSatd = satd_fn((*pMe).pEncMb, kiEncStride, (*pMe).pRefMb, kiRefStride) as u32;
            (*pMe).uiSatdCost = (*pMe).uSadPredISatd.uiSatd
                + COST_MVD(
                    (*pMe).pMvdCost,
                    ((*pMe).sMv.iMvX - (*pMe).sMvp.iMvX) as i32,
                    ((*pMe).sMv.iMvY - (*pMe).sMvp.iMvY) as i32,
                );
        }
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn NotCalculateSatdCost(
    _pSatd: Option<PSampleSadSatdCostFuncRaw>,
    _pMe: &mut SWelsME,
    _kiEncStride: i32,
    _kiRefStride: i32,
) {
}

// ============================================================================
// Small Diamond Search (ME_DIA)
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMeSadCostSelect(
    iSadCost: *mut i32,
    kpMvdCost: *const u16,
    pBestCost: *mut i32,
    kiDx: i32,
    kiDy: i32,
    pIx: *mut i32,
    pIy: *mut i32,
) -> bool {
    unsafe {
        let iInputSadCost = *pBestCost;
        let mut iTempSadCost = [0i32; 4];
        iTempSadCost[0] = *iSadCost.add(0) + COST_MVD(kpMvdCost, kiDx, kiDy - 4) as i32;
        iTempSadCost[1] = *iSadCost.add(1) + COST_MVD(kpMvdCost, kiDx, kiDy + 4) as i32;
        iTempSadCost[2] = *iSadCost.add(2) + COST_MVD(kpMvdCost, kiDx - 4, kiDy) as i32;
        iTempSadCost[3] = *iSadCost.add(3) + COST_MVD(kpMvdCost, kiDx + 4, kiDy) as i32;

        if iTempSadCost[0] < *pBestCost {
            *pBestCost = iTempSadCost[0];
            *pIx = 0;
            *pIy = 1;
        }
        if iTempSadCost[1] < *pBestCost {
            *pBestCost = iTempSadCost[1];
            *pIx = 0;
            *pIy = -1;
        }
        if iTempSadCost[2] < *pBestCost {
            *pBestCost = iTempSadCost[2];
            *pIx = 1;
            *pIy = 0;
        }
        if iTempSadCost[3] < *pBestCost {
            *pBestCost = iTempSadCost[3];
            *pIx = -1;
            *pIy = 0;
        }

        *pBestCost == iInputSadCost
    }
}

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsDiamondSearch(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    kiStrideEnc: i32,
    kiStrideRef: i32,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad4 = (*pFuncList).sSampleDealingFuncs.pfSample4SadRaw[block_size];
        let pSadSingle = (*pFuncList).sSampleDealingFuncs.pfSampleSadRaw[block_size];

        let pFref = (*pMe).pRefMb;
        let kpEncMb = (*pMe).pEncMb;
        let kpMvdCost = (*pMe).pMvdCost;

        let ksMvStartMin = (*pSlice).sMvStartMin;
        let ksMvStartMax = (*pSlice).sMvStartMax;

        let mut iMvDx = ((*pMe).sMv.iMvX as i32 * 4) - (*pMe).sMvp.iMvX as i32;
        let mut iMvDy = ((*pMe).sMv.iMvY as i32 * 4) - (*pMe).sMvp.iMvY as i32;

        let mut pRefMb = pFref;
        let mut iBestCost = (*pMe).uiSadCost as i32;
        let mut iTimeThreshold = ITERATIVE_TIMES;
        let mut iSadCosts = [0i32; 4];

        while iTimeThreshold > 0 {
            iTimeThreshold -= 1;
            (*pMe).sMv.iMvX = ((iMvDx + (*pMe).sMvp.iMvX as i32) >> 2) as i16;
            (*pMe).sMv.iMvY = ((iMvDy + (*pMe).sMvp.iMvY as i32) >> 2) as i16;

            if !CheckMvInRange((*pMe).sMv, ksMvStartMin, ksMvStartMax) {
                continue;
            }

            if let Some(sad4_fn) = pSad4 {
                sad4_fn(kpEncMb, kiStrideEnc, pRefMb, kiStrideRef, iSadCosts.as_mut_ptr());
            } else if let Some(sad_fn) = pSadSingle {
                iSadCosts[0] = sad_fn(kpEncMb, kiStrideEnc, pRefMb.offset(-(kiStrideRef as isize)), kiStrideRef);
                iSadCosts[1] = sad_fn(kpEncMb, kiStrideEnc, pRefMb.offset(kiStrideRef as isize), kiStrideRef);
                iSadCosts[2] = sad_fn(kpEncMb, kiStrideEnc, pRefMb.offset(-1), kiStrideRef);
                iSadCosts[3] = sad_fn(kpEncMb, kiStrideEnc, pRefMb.offset(1), kiStrideRef);
            }

            let mut iX = 0i32;
            let mut iY = 0i32;
            let kbIsBestCostWorse = WelsMeSadCostSelect(
                iSadCosts.as_mut_ptr(),
                kpMvdCost,
                &mut iBestCost,
                iMvDx,
                iMvDy,
                &mut iX,
                &mut iY,
            );
            if kbIsBestCostWorse {
                break;
            }

            iMvDx -= iX * 4;
            iMvDy -= iY * 4;
            pRefMb = pRefMb.offset(-((iX + iY * kiStrideRef) as isize));
        }

        (*pMe).sMv.iMvX = ((iMvDx + (*pMe).sMvp.iMvX as i32) >> 2) as i16;
        (*pMe).sMv.iMvY = ((iMvDy + (*pMe).sMvp.iMvY as i32) >> 2) as i16;
        (*pMe).uiSadCost = iBestCost as u32;
        (*pMe).uiSatdCost = (*pMe).uiSadCost;
        (*pMe).pRefMb = pRefMb;
    }
}

// ============================================================================
// Directional Scrolling Search
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn CheckDirectionalMv(
    pSad: Option<PSampleSadSatdCostFuncRaw>,
    pMe: &mut SWelsME,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    kiEncStride: i32,
    kiRefStride: i32,
    iBestSadCost: &mut i32,
) -> bool {
    unsafe {
        let kiMvX = (*pMe).sDirectionalMv.iMvX;
        let kiMvY = (*pMe).sDirectionalMv.iMvY;

        if ((*pMe).uiBlockSize as usize != BLOCK_16x16)
            && ((kiMvX != 0) || (kiMvY != 0))
            && CheckMvInRange((*pMe).sDirectionalMv, ksMinMv, ksMaxMv)
        {
            let pRef = (*pMe).pColoRefMb.offset((kiMvY as i32 * kiRefStride + kiMvX as i32) as isize);
            let mut uiCurrentSadCost = 0u32;
            if let Some(sad_fn) = pSad {
                uiCurrentSadCost = sad_fn((*pMe).pEncMb, kiEncStride, pRef, kiRefStride) as u32;
            }
            uiCurrentSadCost += COST_MVD(
                (*pMe).pMvdCost,
                (kiMvX as i32 * 4) - (*pMe).sMvp.iMvX as i32,
                (kiMvY as i32 * 4) - (*pMe).sMvp.iMvY as i32,
            );
            if uiCurrentSadCost < (*pMe).uiSadCost {
                *iBestSadCost = uiCurrentSadCost as i32;
                return true;
            }
        }
        false
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn CheckDirectionalMvFalse(
    _pSad: Option<PSampleSadSatdCostFuncRaw>,
    _pMe: &mut SWelsME,
    _ksMinMv: SMVUnitXY,
    _ksMaxMv: SMVUnitXY,
    _kiEncStride: i32,
    _kiRefStride: i32,
    _iBestSadCost: &mut i32,
) -> bool {
    false
}

// ============================================================================
// 1D Orthogonal Cross Search (ME_CROSS)
// ============================================================================

// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitMeFunc` (`:508-528`) installs
// this body into `pfVerticalFullSearch`/`pfHorizontalFullSearch` **only** in its
// `bScreenContent` arm, and both slots default to `None` (`wels_func_ptr_def.rs:481`),
// so the two call sites (`:1085`, `:1099`) take their `if let Some(..)` never for
// camera content. Its sibling shortcuts `WelsMotionEstimateSearchStatic` and
// `..Scrolled` were already carrying this tag; this one was still filed as `cursor`,
// which read as a Phase 9 conversion still owed. It is not — it is Phase 10's.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn LineFullSearch_c(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pMvdTable: *mut u16,
    kiEncStride: i32,
    kiRefStride: i32,
    iMinMv: i16,
    iMaxMv: i16,
    bVerticalSearch: bool,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad = (*pFuncList).sSampleDealingFuncs.pfSampleSadRaw[block_size];
        let kiCurMeBlockPixX = (*pMe).iCurMeBlockPixX;
        let kiCurMeBlockPixY = (*pMe).iCurMeBlockPixY;

        let iMinPos: i32;
        let iMaxPos: i32;
        let iFixedMvd: i32;
        let iCurMeBlockPix: i32;
        let iStride: i32;
        let mut pMvdCost: *mut u16;

        if bVerticalSearch {
            iMinPos = kiCurMeBlockPixY + iMinMv as i32;
            iMaxPos = kiCurMeBlockPixY + iMaxMv as i32;
            iFixedMvd = *pMvdTable.offset(-((*pMe).sMvp.iMvX as isize)) as i32;
            iCurMeBlockPix = kiCurMeBlockPixY;
            iStride = kiRefStride;
            pMvdCost = pMvdTable.offset(((iMinMv as i32 * 4) - (*pMe).sMvp.iMvY as i32) as isize);
        } else {
            iMinPos = kiCurMeBlockPixX + iMinMv as i32;
            iMaxPos = kiCurMeBlockPixX + iMaxMv as i32;
            iFixedMvd = *pMvdTable.offset(-((*pMe).sMvp.iMvY as isize)) as i32;
            iCurMeBlockPix = kiCurMeBlockPixX;
            iStride = 1;
            pMvdCost = pMvdTable.offset(((iMinMv as i32 * 4) - (*pMe).sMvp.iMvX as i32) as isize);
        }

        let mut pRef = (*pMe).pColoRefMb.offset((iMinMv as i32 * iStride) as isize);
        let mut uiBestCost: u32 = 0xFFFF_FFFF;
        let mut iBestPos: i32 = 0;

        for iTargetPos in iMinPos..iMaxPos {
            let kpEncMb = (*pMe).pEncMb;
            let mut uiSadCost: u32 = 0;
            if let Some(sad_fn) = pSad {
                uiSadCost = sad_fn(kpEncMb, kiEncStride, pRef, kiRefStride) as u32;
            }
            uiSadCost += (iFixedMvd + *pMvdCost as i32) as u32;
            if uiSadCost < uiBestCost {
                uiBestCost = uiSadCost;
                iBestPos = iTargetPos;
            }
            pRef = pRef.offset(iStride as isize);
            pMvdCost = pMvdCost.add(4);
        }

        if uiBestCost < (*pMe).uiSadCost {
            let mut sBestMv = SMVUnitXY::default();
            sBestMv.iMvX = if bVerticalSearch { 0 } else { (iBestPos - iCurMeBlockPix) as i16 };
            sBestMv.iMvY = if bVerticalSearch { (iBestPos - iCurMeBlockPix) as i16 } else { 0 };
            let pBestRef = (*pMe).pColoRefMb.offset((sBestMv.iMvY as i32 * kiRefStride + sBestMv.iMvX as i32) as isize);
            UpdateMeResults(sBestMv, uiBestCost, pBestRef, pMe);
        }
    }
}

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMotionCrossSearch(
    pFuncList: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    kiEncStride: i32,
    kiRefStride: i32,
) {
    unsafe {
        if let Some(vert_fn) = (*pFuncList).pfVerticalFullSearch {
            vert_fn(
                pFuncList,
                pMe,
                (*pMe).pMvdCost,
                kiEncStride,
                kiRefStride,
                (*pSlice).sMvStartMin.iMvY,
                (*pSlice).sMvStartMax.iMvY,
                true,
            );
        }

        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            if let Some(horiz_fn) = (*pFuncList).pfHorizontalFullSearch {
                horiz_fn(
                    pFuncList,
                    pMe,
                    (*pMe).pMvdCost,
                    kiEncStride,
                    kiRefStride,
                    (*pSlice).sMvStartMin.iMvX,
                    (*pSlice).sMvStartMax.iMvX,
                    false,
                );
            }
        }
    }
}

// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsDiamondCrossSearch(
    pFunc: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    kiEncStride: i32,
    kiRefStride: i32,
) {
    unsafe {
        WelsDiamondSearch(pFunc, pMe, pSlice, kiEncStride, kiRefStride);

        if !(*pMe).pRefFeatureStorage.is_null() {
            let block_size = (*pMe).uiBlockSize as usize;
            (*pMe).uiSadCostThreshold = (*(*pMe).pRefFeatureStorage).uiSadCostThreshold[block_size];
        }
        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            WelsMotionCrossSearch(pFunc, pMe, pSlice, kiEncStride, kiRefStride);
        }
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsDiamondCrossFeatureSearch(
    pFunc: *mut SWelsFuncPtrList,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    kiEncStride: i32,
    kiRefStride: i32,
) {
    unsafe {
        WelsDiamondCrossSearch(pFunc, pMe, pSlice, kiEncStride, kiRefStride);

        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            (*pSlice).uiSliceFMECostDown = (*pSlice).uiSliceFMECostDown.wrapping_add((*pMe).uiSadCost);

            let mut sFeatureSearchIn = SFeatureSearchIn::default();
            if SetFeatureSearchIn(
                &*pFunc,
                pMe,
                &*pSlice,
                (*pMe).pRefFeatureStorage,
                kiEncStride,
                kiRefStride,
                &mut sFeatureSearchIn,
            ) {
                MotionEstimateFeatureFullSearch(sFeatureSearchIn, u32::MAX, pMe);
            }
            (*pSlice).uiSliceFMECostDown = (*pSlice).uiSliceFMECostDown.wrapping_sub((*pMe).uiSadCost);
        }
    }
}

// ============================================================================
// Feature Search (FME / Screen Content Coding)
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn SumOf8x8SingleBlock_c(pRef: *mut u8, kiRefStride: i32) -> i32 {
    let mut iSum = 0i32;
    let mut ptr = pRef;
    for _ in 0..8 {
        unsafe {
            iSum += *ptr as i32
                + *ptr.add(1) as i32
                + *ptr.add(2) as i32
                + *ptr.add(3) as i32
                + *ptr.add(4) as i32
                + *ptr.add(5) as i32
                + *ptr.add(6) as i32
                + *ptr.add(7) as i32;
            ptr = ptr.offset(kiRefStride as isize);
        }
    }
    iSum
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn SumOf16x16SingleBlock_c(pRef: *mut u8, kiRefStride: i32) -> i32 {
    let mut iSum = 0i32;
    let mut ptr = pRef;
    for _ in 0..16 {
        unsafe {
            iSum += *ptr as i32
                + *ptr.add(1) as i32
                + *ptr.add(2) as i32
                + *ptr.add(3) as i32
                + *ptr.add(4) as i32
                + *ptr.add(5) as i32
                + *ptr.add(6) as i32
                + *ptr.add(7) as i32
                + *ptr.add(8) as i32
                + *ptr.add(9) as i32
                + *ptr.add(10) as i32
                + *ptr.add(11) as i32
                + *ptr.add(12) as i32
                + *ptr.add(13) as i32
                + *ptr.add(14) as i32
                + *ptr.add(15) as i32;
            ptr = ptr.offset(kiRefStride as isize);
        }
    }
    iSum
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn SumOf8x8BlockOfFrame_c(
    pRefPicture: *mut u8,
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: *mut u16,
    pTimesOfFeatureValue: *mut u32,
) {
    for y in 0..kiHeight {
        unsafe {
            let pRef = pRefPicture.offset((kiRefStride * y) as isize);
            let pBuffer = pFeatureOfBlock.offset((kiWidth * y) as isize);
            for x in 0..kiWidth {
                let iSum = SumOf8x8SingleBlock_c(pRef.offset(x as isize), kiRefStride);
                *pBuffer.offset(x as isize) = iSum as u16;
                *pTimesOfFeatureValue.offset(iSum as isize) += 1;
            }
        }
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn SumOf16x16BlockOfFrame_c(
    pRefPicture: *mut u8,
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: *mut u16,
    pTimesOfFeatureValue: *mut u32,
) {
    for y in 0..kiHeight {
        unsafe {
            let pRef = pRefPicture.offset((kiRefStride * y) as isize);
            let pBuffer = pFeatureOfBlock.offset((kiWidth * y) as isize);
            for x in 0..kiWidth {
                let iSum = SumOf16x16SingleBlock_c(pRef.offset(x as isize), kiRefStride);
                *pBuffer.offset(x as isize) = iSum as u16;
                *pTimesOfFeatureValue.offset(iSum as isize) += 1;
            }
        }
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn InitializeHashforFeature_c(
    pTimesOfFeatureValue: *mut u32,
    pBuf: *mut u16,
    kiListSize: i32,
    pLocationOfFeature: *mut *mut u16,
    pFeatureValuePointerList: *mut *mut u16,
) {
    unsafe {
        let mut pBufPos = pBuf;
        for i in 0..kiListSize as isize {
            *pLocationOfFeature.offset(i) = pBufPos;
            *pFeatureValuePointerList.offset(i) = pBufPos;
            pBufPos = pBufPos.offset((*pTimesOfFeatureValue.offset(i) << 1) as isize);
        }
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn FillQpelLocationByFeatureValue_c(
    pFeatureOfBlock: *mut u16,
    kiWidth: i32,
    kiHeight: i32,
    pFeatureValuePointerList: *mut *mut u16,
) {
    unsafe {
        let mut pSrcPointer = pFeatureOfBlock;
        let mut iQpelY = 0i32;
        for _ in 0..kiHeight {
            for x in 0..kiWidth {
                let uiFeature = *pSrcPointer.offset(x as isize) as isize;
                let target_ptr = *pFeatureValuePointerList.offset(uiFeature);
                *target_ptr.offset(0) = (x << 2) as u16;
                *target_ptr.offset(1) = iQpelY as u16;
                *pFeatureValuePointerList.offset(uiFeature) = target_ptr.add(2);
            }
            iQpelY += 4;
            pSrcPointer = pSrcPointer.offset(kiWidth as isize);
        }
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CalculateFeatureOfBlock(
    pFunc: &SWelsFuncPtrList,
    pRef: &mut SPicture,
    pScreenBlockFeatureStorage: *mut SScreenBlockFeatureStorage,
) -> bool {
    unsafe {
        let pFeatureOfBlock = (*pScreenBlockFeatureStorage).pFeatureOfBlockPointer;
        let pTimesOfFeatureValue = (*pScreenBlockFeatureStorage).pTimesOfFeatureValue;
        let pLocationOfFeature = (*pScreenBlockFeatureStorage).pLocationOfFeature;
        let pBuf = (*pScreenBlockFeatureStorage).pLocationPointer;

        if pFeatureOfBlock.is_null()
            || pTimesOfFeatureValue.is_null()
            || pLocationOfFeature.is_null()
            || pBuf.is_null()
            || pRef.data_ptr(0).is_null()
        {
            return false;
        }

        let pRefData = pRef.data_ptr(0);
        let iRefStride = pRef.stride(0);
        let iIs16x16 = (*pScreenBlockFeatureStorage).iIs16x16 as usize;
        let iEdgeDiscard = if iIs16x16 != 0 { 16 } else { 8 };
        let iWidth = pRef.iWidthInPixel - iEdgeDiscard;
        let kiHeight = pRef.iHeightInPixel - iEdgeDiscard;
        let kiActualListSize = (*pScreenBlockFeatureStorage).iActualListSize;

        std::ptr::write_bytes(pTimesOfFeatureValue as *mut u8, 0, (kiActualListSize as usize) * std::mem::size_of::<u32>());

        if let Some(calc_frame_feature) = pFunc.pfCalculateBlockFeatureOfFrame[iIs16x16] {
            calc_frame_feature(pRefData, iWidth, kiHeight, iRefStride, pFeatureOfBlock, pTimesOfFeatureValue);
        }

        if let Some(init_hash) = pFunc.pfInitializeHashforFeature {
            init_hash(
                pTimesOfFeatureValue,
                pBuf,
                kiActualListSize,
                pLocationOfFeature,
                (*pScreenBlockFeatureStorage).pFeatureValuePointerList,
            );
        }

        if let Some(fill_qpel) = pFunc.pfFillQpelLocationByFeatureValue {
            fill_qpel(
                pFeatureOfBlock,
                iWidth,
                kiHeight,
                (*pScreenBlockFeatureStorage).pFeatureValuePointerList,
            );
        }

        true
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn PerformFMEPreprocess(
    pFunc: &SWelsFuncPtrList,
    pRef: &mut SPicture,
    pFeatureOfBlock: *mut u16,
    pScreenBlockFeatureStorage: *mut SScreenBlockFeatureStorage,
) {
    unsafe {
        (*pScreenBlockFeatureStorage).pFeatureOfBlockPointer = pFeatureOfBlock;
        (*pScreenBlockFeatureStorage).bRefBlockFeatureCalculated =
            CalculateFeatureOfBlock(pFunc, pRef, pScreenBlockFeatureStorage);

        if (*pScreenBlockFeatureStorage).bRefBlockFeatureCalculated {
            let qp_idx = (pRef.iFrameAverageQp).clamp(0, 51) as usize;
            let uiRefPictureAvgQstepx16 = QStepx16ByQp[qp_idx] as u32;
            let uiSadCostThreshold16x16 = (30 * (uiRefPictureAvgQstepx16 + 160)) >> 3;

            (*pScreenBlockFeatureStorage).uiSadCostThreshold[BLOCK_16x16] = uiSadCostThreshold16x16;
            (*pScreenBlockFeatureStorage).uiSadCostThreshold[BLOCK_8x8] = uiSadCostThreshold16x16 >> 2;
            (*pScreenBlockFeatureStorage).uiSadCostThreshold[BLOCK_16x8] = u32::MAX;
            (*pScreenBlockFeatureStorage).uiSadCostThreshold[BLOCK_8x16] = u32::MAX;
            (*pScreenBlockFeatureStorage).uiSadCostThreshold[BLOCK_4x4] = u32::MAX;
        }
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn SetFeatureSearchIn(
    pFunc: &SWelsFuncPtrList,
    sMe: &SWelsME,
    pSlice: &SSlice,
    pRefFeatureStorage: *mut SScreenBlockFeatureStorage,
    kiEncStride: i32,
    kiRefStride: i32,
    pFeatureSearchIn: &mut SFeatureSearchIn,
) -> bool {
    unsafe {
        let block_size = sMe.uiBlockSize as usize;
        pFeatureSearchIn.pSad = pFunc.sSampleDealingFuncs.pfSampleSadRaw[block_size];

        let single_fn_idx = if block_size == BLOCK_16x16 { 1 } else { 0 };
        if let Some(calc_single) = pFunc.pfCalculateSingleBlockFeature[single_fn_idx] {
            pFeatureSearchIn.iFeatureOfCurrent = calc_single(sMe.pEncMb, kiEncStride);
        }

        pFeatureSearchIn.pEnc = sMe.pEncMb;
        pFeatureSearchIn.pColoRef = sMe.pColoRefMb;
        pFeatureSearchIn.iEncStride = kiEncStride;
        pFeatureSearchIn.iRefStride = kiRefStride;
        pFeatureSearchIn.uiSadCostThresh = sMe.uiSadCostThreshold as u16;

        pFeatureSearchIn.iCurPixX = sMe.iCurMeBlockPixX;
        pFeatureSearchIn.iCurPixXQpel = pFeatureSearchIn.iCurPixX << 2;
        pFeatureSearchIn.iCurPixY = sMe.iCurMeBlockPixY;
        pFeatureSearchIn.iCurPixYQpel = pFeatureSearchIn.iCurPixY << 2;

        pFeatureSearchIn.pTimesOfFeature = (*pRefFeatureStorage).pTimesOfFeatureValue;
        pFeatureSearchIn.pQpelLocationOfFeature = (*pRefFeatureStorage).pLocationOfFeature;
        pFeatureSearchIn.pMvdCostX = sMe.pMvdCost.offset(-(pFeatureSearchIn.iCurPixXQpel as isize) - sMe.sMvp.iMvX as isize);
        pFeatureSearchIn.pMvdCostY = sMe.pMvdCost.offset(-(pFeatureSearchIn.iCurPixYQpel as isize) - sMe.sMvp.iMvY as isize);

        pFeatureSearchIn.iMinQpelX = pFeatureSearchIn.iCurPixXQpel + (pSlice.sMvStartMin.iMvX as i32 * 4);
        pFeatureSearchIn.iMinQpelY = pFeatureSearchIn.iCurPixYQpel + (pSlice.sMvStartMin.iMvY as i32 * 4);
        pFeatureSearchIn.iMaxQpelX = pFeatureSearchIn.iCurPixXQpel + (pSlice.sMvStartMax.iMvX as i32 * 4);
        pFeatureSearchIn.iMaxQpelY = pFeatureSearchIn.iCurPixYQpel + (pSlice.sMvStartMax.iMvY as i32 * 4);

        if pFeatureSearchIn.pSad.is_none()
            || pFeatureSearchIn.pTimesOfFeature.is_null()
            || pFeatureSearchIn.pQpelLocationOfFeature.is_null()
        {
            return false;
        }
        true
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn SaveFeatureSearchOut(
    sBestMv: SMVUnitXY,
    uiBestSadCost: u32,
    pRef: *mut u8,
    pFeatureSearchOut: *mut SFeatureSearchOut,
) {
    unsafe {
        (*pFeatureSearchOut).sBestMv = sBestMv;
        (*pFeatureSearchOut).uiBestSadCost = uiBestSadCost;
        (*pFeatureSearchOut).pBestRef = pRef;
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn FeatureSearchOne(
    sFeatureSearchIn: &SFeatureSearchIn,
    iFeatureDifference: i32,
    kuiExpectedSearchTimes: u32,
    pFeatureSearchOut: *mut SFeatureSearchOut,
) -> bool {
    let iFeatureOfRef = sFeatureSearchIn.iFeatureOfCurrent + iFeatureDifference;
    if iFeatureOfRef < 0 || iFeatureOfRef >= LIST_SIZE {
        return true;
    }

    let pSad = sFeatureSearchIn.pSad;
    let pEnc = sFeatureSearchIn.pEnc;
    let pColoRef = sFeatureSearchIn.pColoRef;
    let iEncStride = sFeatureSearchIn.iEncStride;
    let iRefStride = sFeatureSearchIn.iRefStride;
    let uiSadCostThresh = sFeatureSearchIn.uiSadCostThresh as u32;

    let iCurPixX = sFeatureSearchIn.iCurPixX;
    let iCurPixY = sFeatureSearchIn.iCurPixY;
    let iCurPixXQpel = sFeatureSearchIn.iCurPixXQpel;
    let iCurPixYQpel = sFeatureSearchIn.iCurPixYQpel;

    let iMinQpelX = sFeatureSearchIn.iMinQpelX;
    let iMinQpelY = sFeatureSearchIn.iMinQpelY;
    let iMaxQpelX = sFeatureSearchIn.iMaxQpelX;
    let iMaxQpelY = sFeatureSearchIn.iMaxQpelY;

    unsafe {
        let times = *sFeatureSearchIn.pTimesOfFeature.offset(iFeatureOfRef as isize);
        let iSearchTimes = times.min(kuiExpectedSearchTimes) as i32;
        let iSearchTimesx2 = iSearchTimes << 1;
        let pQpelPosition = *sFeatureSearchIn.pQpelLocationOfFeature.offset(iFeatureOfRef as isize);

        let mut sBestMv = (*pFeatureSearchOut).sBestMv;
        let mut uiBestCost = (*pFeatureSearchOut).uiBestSadCost;
        let mut pBestRef = (*pFeatureSearchOut).pBestRef;

        let mut i = 0i32;
        while i < iSearchTimesx2 {
            let iQpelX = *pQpelPosition.offset(i as isize) as i32;
            let iQpelY = *pQpelPosition.offset((i + 1) as isize) as i32;

            if (iQpelX > iMaxQpelX)
                || (iQpelX < iMinQpelX)
                || (iQpelY > iMaxQpelY)
                || (iQpelY < iMinQpelY)
                || (iQpelX == iCurPixXQpel)
                || (iQpelY == iCurPixYQpel)
            {
                i += 2;
                continue;
            }

            let mut uiTmpCost = (*sFeatureSearchIn.pMvdCostX.offset(iQpelX as isize) as u32)
                + (*sFeatureSearchIn.pMvdCostY.offset(iQpelY as isize) as u32);
            if uiTmpCost.wrapping_add(iFeatureDifference as u32) >= uiBestCost {
                i += 2;
                continue;
            }

            let iIntepelX = (iQpelX >> 2) - iCurPixX;
            let iIntepelY = (iQpelY >> 2) - iCurPixY;
            let pCurRef = pColoRef.offset((iIntepelX + iIntepelY * iRefStride) as isize);

            if let Some(sad_fn) = pSad {
                uiTmpCost += sad_fn(pEnc, iEncStride, pCurRef, iRefStride) as u32;
            }

            if uiTmpCost < uiBestCost {
                sBestMv.iMvX = iIntepelX as i16;
                sBestMv.iMvY = iIntepelY as i16;
                uiBestCost = uiTmpCost;
                pBestRef = pCurRef;

                if uiBestCost < uiSadCostThresh {
                    break;
                }
            }

            i += 2;
        }

        SaveFeatureSearchOut(sBestMv, uiBestCost, pBestRef, pFeatureSearchOut);
        i < iSearchTimesx2
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn MotionEstimateFeatureFullSearch(
    sFeatureSearchIn: SFeatureSearchIn,
    kuiMaxSearchPoint: u32,
    pMe: &mut SWelsME,
) {
    unsafe {
        let mut sFeatureSearchOut = SFeatureSearchOut {
            sBestMv: (*pMe).sMv,
            uiBestSadCost: (*pMe).uiSadCost,
            pBestRef: (*pMe).pRefMb,
        };

        let iFeatureDifference = 0i32;
        FeatureSearchOne(&sFeatureSearchIn, iFeatureDifference, kuiMaxSearchPoint, &mut sFeatureSearchOut);

        if sFeatureSearchOut.uiBestSadCost < (*pMe).uiSadCost {
            UpdateMeResults(
                sFeatureSearchOut.sBestMv,
                sFeatureSearchOut.uiBestSadCost,
                sFeatureSearchOut.pBestRef,
                pMe,
            );
        }
    }
}

// ============================================================================
// Adaptive FME Switch Management
// ============================================================================

/// Intentional no-op motion estimation FME switch callback.
/// Matches `void UpdateFMESwitchNull (SDqLayer* pCurLayer)` in `svc_motion_estimate.cpp:1059`.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn UpdateFMESwitchNull(_pCurLayer: *mut SDqLayer) {}

// ============================================================================
// Feature Storage Dynamic Allocation & Deallocation
// ============================================================================

// `RequestFeatureSearchPreparation` / `ReleaseFeatureSearchPreparation` and
// `SFeatureSearchPreparation` itself stood here, with `UpdateFMESwitch`,
// `CountFMECostDown` and `UpdateFMEGoodFrameCount` above. Nothing called any of
// them: `pfUpdateFMESwitch` is unconditionally `UpdateFMESwitchNull`
// (`WelsInitMeFunc`), and the layer field the rest reached through was written only
// with `null_mut()`, under a guard that refuses screen content two lines earlier.
// S18 — deleted, enumerated by strip-and-build (T6.D2).

// SCREEN_CONTENT(dormant: Phase 10)
//
// **`RequestScreenBlockFeatureStorage` and `ReleaseScreenBlockFeatureStorage` stood
// here, and T7.C6 deleted them.** They were the last eight `WelsMallocz`/`WelsFree`
// call sites in `src/encoder` — four allocations
// (`pTimesOfFeatureValue`, `pLocationOfFeature`, `pLocationPointer`,
// `pFeatureValuePointerList`) and their four frees, transliterated from
// `svc_motion_estimate.cpp:683` and `:727`.
//
// **They had no caller, on either side of the port's own boundary.** The C++ calls
// them from the picture constructor (`picture_handle.cpp:115` and `:173`); the port's
// picture constructor refuses `iNeedFeatureStorage != 0` before it can reach them
// (`wels_preprocess.rs`, the `SCREEN_CONTENT(dormant)` note there), so no live path
// allocated any of this and none ever has. T6.H13's census recorded that and fenced
// them; this session's step 2 asks the allocator itself to retire from the encoder,
// and two unreachable functions are not a reason to keep a whole allocator wired into
// the context.
//
// **Deleted rather than converted, and the difference matters.** Converting them
// would have produced four owned buffers behind a struct whose *fields*
// (`SScreenBlockFeatureStorage`'s raw pointers) are Phase 10's to design — a shape
// nobody has decided, on a path nobody runs. Phase 10 ports this family whole, from
// the reference, where both functions are eleven and thirty lines of plain
// allocate-and-null. The struct and its fields are untouched, so the only thing gone
// is a transliteration of an allocation nothing performs.

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_constants_and_tables() {
        assert_eq!(CAMERA_STARTMV_RANGE, 64);
        assert_eq!(ITERATIVE_TIMES, 16);
        assert_eq!(CAMERA_MV_RANGE, 80);
        assert_eq!(CAMERA_MVD_RANGE, 162);
        assert_eq!(BASE_MV_MB_NMB, 9);
        assert_eq!(EXPANDED_MV_RANGE, 504);
        assert_eq!(EXPANDED_MVD_RANGE, 1010);
        assert_eq!(QStepx16ByQp[0], 10);
        assert_eq!(QStepx16ByQp[51], 3584);
    }

    #[test]
    fn test_mv_range_checks() {
        let mv_min = SMVUnitXY::new(-80, -80);
        let mv_max = SMVUnitXY::new(80, 80);

        assert!(CheckMvInRange(SMVUnitXY::new(0, 0), mv_min, mv_max));
        assert!(CheckMvInRange(SMVUnitXY::new(-80, 79), mv_min, mv_max));
        assert!(!CheckMvInRange(SMVUnitXY::new(80, 0), mv_min, mv_max));
        assert!(!CheckMvInRange(SMVUnitXY::new(-81, 0), mv_min, mv_max));
    }

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_cost_mvd_computation() {
        let table_data = [10u16, 20, 30, 40, 50, 60, 70, 80];
        let base_ptr = table_data.as_ptr();
        unsafe {
            let cost = COST_MVD(base_ptr, 2, 5);
            assert_eq!(cost, 30 + 60);
        }
    }

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_single_block_sums() {
        let mut buf8 = [0u8; 64];
        for i in 0..64 {
            buf8[i] = 1;
        }
        unsafe {
            let sum8 = SumOf8x8SingleBlock_c(buf8.as_mut_ptr(), 8);
            assert_eq!(sum8, 64);
        }

        let mut buf16 = [0u8; 256];
        for i in 0..256 {
            buf16[i] = 2;
        }
        unsafe {
            let sum16 = SumOf16x16SingleBlock_c(buf16.as_mut_ptr(), 16);
            assert_eq!(sum16, 512);
        }
    }

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_me_sad_cost_select() {
        let mut sad_costs = [100i32, 50, 120, 80];
        let mvd_cost_table = vec![0u16; 512];
        let mut best_cost = 200i32;
        let mut ix = 0i32;
        let mut iy = 0i32;

        unsafe {
            let stop = WelsMeSadCostSelect(
                sad_costs.as_mut_ptr(),
                mvd_cost_table.as_ptr().add(256),
                &mut best_cost,
                0,
                0,
                &mut ix,
                &mut iy,
            );
            assert!(!stop);
            assert_eq!(best_cost, 50);
            assert_eq!(ix, 0);
            assert_eq!(iy, -1);
        }
    }

    #[test]
    fn test_fme_switch_flag() {
        assert!(CalcFMESwitchFlag(2, 0, 40, false));
        assert!(!CalcFMESwitchFlag(0, 0, 40, false));
        assert!(CalcFMESwitchFlag(0, 0, 10, true));
    }

    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn test_fme_noop_callback() {
        unsafe {
            UpdateFMESwitchNull(std::ptr::null_mut());
        }
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static ME_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSE41};
