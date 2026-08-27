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

use crate::safe::plane::{PaddedPlane, PlaneCursor};
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

    // `pEncMb`/`pRefMb`/`pColoRefMb` stood here — the C++'s three per-block
    // plane cursors. Session F deleted them on B3's coordinate identity,
    // re-verified at this tree (583/583 rows, ~3.09M search entries/exits
    // asserted, zero violations): `pEncMb == encRoot + (y*strideEnc + x)`,
    // `pColoRefMb == refRoot + (y*strideRef + x)`, and `pRefMb == colo + mv`
    // at every read. The search family takes the two planes as parameters and
    // derives each position from `iCurMeBlockPixX/Y` + the MV it is probing —
    // S37's value half: the *coordinates* are the information, and no retag
    // can invalidate them.

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
            sMvp: SMVUnitXY::default(),
            sMvBase: SMVUnitXY::default(),
            sDirectionalMv: SMVUnitXY::default(),
            pRefFeatureStorage: std::ptr::null_mut(),
            sMv: SMVUnitXY::default(),
        }
    }
}

/// Input configuration block for the hash-based feature search engine.
///
/// Session F: `pSad` holds the safe cost slot, and the two block cursors
/// (`pEnc`/`pColoRef`, with their strides) became the two planes plus the
/// `iCurPixX/Y` coordinates the struct already carried — the same identity
/// the search family converted on. The mvd-cost and hash cursors stay raw
/// (the ctx family's and Phase 10's respectively).
// SCREEN_CONTENT(dormant: Phase 10)
pub struct SFeatureSearchIn<'a> {
    pub pSad: Option<PSampleSadSatdCostFunc>,
    pub pTimesOfFeature: *mut u32,
    pub pQpelLocationOfFeature: *mut *mut u16,
    pub pMvdCostX: *mut u16,
    pub pMvdCostY: *mut u16,
    pub pEncPlane: Option<&'a PaddedPlane>,
    pub pRefPlane: Option<&'a PaddedPlane>,
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

impl Default for SFeatureSearchIn<'_> {
    fn default() -> Self {
        Self {
            pSad: None,
            pTimesOfFeature: std::ptr::null_mut(),
            pQpelLocationOfFeature: std::ptr::null_mut(),
            pMvdCostX: std::ptr::null_mut(),
            pMvdCostY: std::ptr::null_mut(),
            pEncPlane: None,
            pRefPlane: None,
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
///
/// Session F: `pBestRef: *mut u8` is deleted — it cached `colo + sBestMv`,
/// which is `sBestMv`'s information (the identity the whole family converted
/// on), and its one consumer wrote it into the deleted `SWelsME::pRefMb`.
// SCREEN_CONTENT(dormant: Phase 10)
#[derive(Copy, Clone, Default)]
pub struct SFeatureSearchOut {
    pub sBestMv: SMVUnitXY,
    pub uiBestSadCost: u32,
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
// to catch (the allowlist entry retires with the duplicate). The `*Raw` alias
// went with the transitional triple (session F).
pub use crate::encoder::md::PSampleSadSatdCostFunc;

/// `PSample4SadCostFunc` — the four-candidate SAD the diamond search steps with:
/// `sample1`'s block against `sample2`'s at each whole-sample neighbour, written to
/// `sad[0..4]` in the order **up, down, left, right**
/// (`common/sad_common.rs::sample_sad_four::<W, H>`).
///
/// Safe since T9.B25; see [`PSampleSadSatdCostFunc`] for the rule and
/// [`PSample4SadCostFuncRaw`] for the shape this replaces.
pub type PSample4SadCostFunc = fn(&PlaneCursor<'_>, &PlaneCursor<'_>, &mut [i32; 4]);

// `PSample4SadCostFuncRaw` — the transitional raw four-candidate shape — went
// with the raw triple (session F).

// `PSampleSadHor8Func` stood here — it typed `pfSampleSadHor8`, the
// screen-content SIMD horizontal-SAD pair, which had zero writers and zero
// readers in the whole tree. Both deleted, S18 (session F step 0).

// **Session F — the five self-referential typedefs de-virtualized** (Phase
// 4a's `pfMdCost` move, applied to the whole family). Each used to take
// `*mut SWelsFuncPtrList` — the table handed back into its own callees so
// they could reach the cost slots and the sub-search slots. The callees now
// take exactly what they reach: `&SMeFuncs` (the sub-search group) and/or
// `&SSampleDealingFunc` (the safe cost tables), plus the two planes the
// deleted `SWelsME` cursors pointed into. Strides travel inside the planes.
// The layer parameter died with the strides: it was read for nothing else.
//
// All remain `unsafe fn` — every body still walks `pMe.pMvdCost`, the raw
// MVD-cost cursor (the ctx family's, G–H) — but nothing in the signatures is
// raw, and under MT every reference parameter is a shared read of pre-fork
// state (the table is written only by `PreprocessSliceCoding`, before the
// fork — F132 round 7's hoist is what makes the `&` lawful).

pub type PMotionSearchFunc = unsafe fn(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
);

pub type PSearchMethodFunc = unsafe fn(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
);

pub type PCalculateSatdFunc = unsafe fn(
    pSatd: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
);

pub type PCheckDirectionalMv = unsafe fn(
    pSad: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
    iBestSadCost: &mut i32,
) -> bool;

pub type PLineFullSearchFunc = unsafe fn(
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pMvdTable: *mut u16,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
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

/// Session F: the slot's one reader is `SetFeatureSearchIn`, whose block
/// origin is a plane cursor now — the raw `SumOf*SingleBlock_c` kernels stay
/// for the frame-feature builders, which walk a whole raw plane, and the slot
/// holds the safe per-block twins below.
pub type PCalculateSingleBlockFeature = fn(cRef: &PlaneCursor<'_>) -> i32;

pub type PUpdateFMESwitch = unsafe extern "C" fn(pCurLayer: *mut SDqLayer);

/// The motion-estimation dispatch group — every slot the search family reaches
/// *through the table it used to be handed back* (session F, the Phase 4a
/// de-virtualization move applied to the five self-referential typedefs).
///
/// The C++ hands each search function `SWelsFuncPtrList*` so it can reach
/// these five surfaces plus `sSampleDealingFuncs`; the port groups the five
/// here and passes `&SMeFuncs` + `&SSampleDealingFunc` instead, so the table
/// parameter dies and the typedefs stop naming the struct that contains them.
/// Membership is the measured reach set: `pfSearchMethod` (the top-level
/// search's per-block dispatch), `pfCalculateSatd` (fast/normal per frame),
/// `pfCheckDirectionalMv` (screen-content arm), the two line-search slots and
/// `pfCalculateSingleBlockFeature` (both FME, dormant). `pfMotionSearch`
/// stays in the table proper: its readers are the mode-decision callers,
/// which hold the whole table lawfully.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMeFuncs {
    pub pfSearchMethod: [Option<PSearchMethodFunc>; BLOCK_SIZE_ALL],
    pub pfCalculateSatd: Option<PCalculateSatdFunc>,
    pub pfCheckDirectionalMv: Option<PCheckDirectionalMv>,
    // SCREEN_CONTENT(dormant: Phase 10) — the cross/feature-search half.
    pub pfVerticalFullSearch: Option<PLineFullSearchFunc>,
    pub pfHorizontalFullSearch: Option<PLineFullSearchFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateSingleBlockFeature: [Option<PCalculateSingleBlockFeature>; 2],
}

impl Default for SMeFuncs {
    fn default() -> Self {
        Self {
            pfSearchMethod: [None; BLOCK_SIZE_ALL],
            pfCalculateSatd: None,
            pfCheckDirectionalMv: None,
            pfVerticalFullSearch: None,
            pfHorizontalFullSearch: None,
            pfCalculateSingleBlockFeature: [None; 2],
        }
    }
}





// ============================================================================
// Helper Macros and Inline Functions
// ============================================================================

/// Calculates MVD rate cost: `table[mx] + table[my]`.
#[inline(always)]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn COST_MVD(table: *const u16, mx: i32, my: i32) -> u32 {
    unsafe { (*table.offset(mx as isize) as u32) + (*table.offset(my as isize) as u32) }
}

/// Session F: the `pRef` argument and the `pRefMb` store are gone — the
/// pointer was `colo + ksBestMv` at every call site (the verified identity),
/// so the MV alone carries the result.
#[inline]
pub fn UpdateMeResults(ksBestMv: SMVUnitXY, kiBestSadCost: u32, pMe: &mut SWelsME) {
    pMe.sMv = ksBestMv;
    pMe.uiSadCost = kiBestSadCost;
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
// unsafe-cat: fork-shared(S63)
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
pub unsafe fn GetCurrentSliceNum(pCurDq: &SDqLayer) -> i32 {
    unsafe { pCurDq.sSliceEncCtx.iSliceNumInFrame.load(std::sync::atomic::Ordering::Relaxed) }
}

// ============================================================================
// Initialization and Dispatch
// ============================================================================

/// Populates motion estimation function pointer table based on CPU capabilities and content type.
///
/// Session F: `&mut` — the init path's exclusive borrow, taken before anything
/// shares the table (the C-ABI init chain is single-threaded); the null
/// tolerance guarded a pointer that no longer exists.
pub fn WelsInitMeFunc(
    pFuncList: &mut SWelsFuncPtrList,
    uiCpuFlag: u32,
    bScreenContent: bool,
) {
    {
        pFuncList.pfUpdateFMESwitch = Some(UpdateFMESwitchNull);

        if !bScreenContent {
            pFuncList.sMeFuncs.pfCheckDirectionalMv = Some(CheckDirectionalMvFalse);
            pFuncList.pfCalculateBlockFeatureOfFrame[0] = None;
            pFuncList.pfCalculateBlockFeatureOfFrame[1] = None;
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[0] = None;
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[1] = None;
        } else {
            pFuncList.sMeFuncs.pfCheckDirectionalMv = Some(CheckDirectionalMv);

            // Cross Search
            pFuncList.sMeFuncs.pfVerticalFullSearch = Some(LineFullSearch_c);
            pFuncList.sMeFuncs.pfHorizontalFullSearch = Some(LineFullSearch_c);

            // Feature Search
            pFuncList.pfInitializeHashforFeature = Some(InitializeHashforFeature_c);
            pFuncList.pfFillQpelLocationByFeatureValue = Some(FillQpelLocationByFeatureValue_c);
            pFuncList.pfCalculateBlockFeatureOfFrame[0] = Some(SumOf8x8BlockOfFrame_c);
            pFuncList.pfCalculateBlockFeatureOfFrame[1] = Some(SumOf16x16BlockOfFrame_c);
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[0] = Some(sum_of_8x8_single_block);
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[1] = Some(sum_of_16x16_single_block);
        }
    }
}

// ============================================================================
// Top-Level Motion Estimation Search Routines
// ============================================================================

/// Top-level motion estimation search for a macroblock or sub-partition.
// unsafe-cat: port-raw(Phase 9) — pMvdCost (the ctx family's raw MVD table)
#[allow(unsafe_code)]
pub unsafe fn WelsMotionEstimateSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        if crate::encoder::dump_enabled(&ME_DUMP, "OH264_MEDUMP") {
            let mut mvc = String::new();
            for di in 0..(*pSlice).uiMvcNum as usize {
                mvc.push_str(&format!(
                    "{}/{},",
                    (*pSlice).sMvc[di].iMvX,
                    (*pSlice).sMvc[di].iMvY
                ));
            }
            let kiX = (*pMe).iCurMeBlockPixX as isize;
            let kiY = (*pMe).iCurMeBlockPixY as isize;
            let cEnc = pEncPlane.cursor(kiX, kiY);
            // Entry state: the reference position is colocated (mv not yet
            // searched), which is what the deleted `pRefMb` held here.
            let cRef = pRefPlane.cursor(kiX, kiY);
            let mut enc = String::new();
            let mut rf = String::new();
            let mut rfup = String::new();
            for di in 0..8isize {
                enc.push_str(&format!("{},", cEnc.row(0, di, 1)[0]));
                rf.push_str(&format!("{},", cRef.row(0, di, 1)[0]));
                rfup.push_str(&format!("{},", cRef.row(-1, di, 1)[0]));
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
        if !WelsMotionEstimateInitialPoint(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane) {
            let block_size = (*pMe).uiBlockSize as usize;
            if let Some(search_fn) = pMeFuncs.pfSearchMethod[block_size] {
                search_fn(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);
            }
            MeEndIntepelSearch(pMe);
        }

        let block_size = (*pMe).uiBlockSize as usize;
        if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
            calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
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
pub unsafe fn WelsMotionEstimateSearchStatic(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    _pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;

        (*pMe).sMv.iMvX = 0;
        (*pMe).sMv.iMvY = 0;

        if let Some(sad_fn) = sdf.pfSampleSad[block_size] {
            (*pMe).uiSadCost =
                sad_fn(&pEncPlane.cursor(kiX, kiY), &pRefPlane.cursor(kiX, kiY)) as u32;
        }
        (*pMe).uiSadCost += COST_MVD((*pMe).pMvdCost, -((*pMe).sMvp.iMvX as i32), -((*pMe).sMvp.iMvY as i32));

        MeEndIntepelSearch(pMe);

        if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
            calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
        }
    }
}

/// Shortcut motion estimation search for scrolled macroblocks.
// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn WelsMotionEstimateSearchScrolled(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    _pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;

        (*pMe).sMv = (*pMe).sDirectionalMv;
        let mv_x = (*pMe).sMv.iMvX as i32;
        let mv_y = (*pMe).sMv.iMvY as i32;

        let mut sad_cost = 0u32;
        if let Some(sad_fn) = sdf.pfSampleSad[block_size] {
            sad_cost = sad_fn(
                &pEncPlane.cursor(kiX, kiY),
                &pRefPlane.cursor(kiX + mv_x as isize, kiY + mv_y as isize),
            ) as u32;
        }
        sad_cost += COST_MVD(
            (*pMe).pMvdCost,
            (mv_x * 4) - ((*pMe).sMvp.iMvX as i32),
            (mv_y * 4) - ((*pMe).sMvp.iMvY as i32),
        );
        (*pMe).uiSadCost = sad_cost;

        MeEndIntepelSearch(pMe);

        if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
            calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
        }
    }
}

// ============================================================================
// Initial Candidate Prediction
// ============================================================================

/// Evaluates spatial MVP, MVC candidate list, and directional scrolling vectors.
// unsafe-cat: port-raw(Phase 9) — pMvdCost (the ctx family's raw MVD table)
#[allow(unsafe_code)]
pub unsafe fn WelsMotionEstimateInitialPoint(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) -> bool {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad = sdf.pfSampleSad[block_size];
        let kpMvdCost = (*pMe).pMvdCost;
        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;
        let cEnc = pEncPlane.cursor(kiX, kiY);

        let kuiMvcNum = (*pSlice).uiMvcNum as usize;
        let ksMvStartMin = (*pSlice).sMvStartMin;
        let ksMvStartMax = (*pSlice).sMvStartMax;
        let ksMvp = (*pMe).sMvp;

        let mut sMv = SMVUnitXY {
            iMvX: (((2 + ksMvp.iMvX as i32) >> 2).clamp(ksMvStartMin.iMvX as i32, ksMvStartMax.iMvX as i32)) as i16,
            iMvY: (((2 + ksMvp.iMvY as i32) >> 2).clamp(ksMvStartMin.iMvY as i32, ksMvStartMax.iMvY as i32)) as i16,
        };

        let mut iBestSadCost: i32 = 0;
        if let Some(sad_fn) = pSad {
            iBestSadCost = sad_fn(
                &cEnc,
                &pRefPlane.cursor(kiX + sMv.iMvX as isize, kiY + sMv.iMvY as isize),
            );
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
                if let Some(sad_fn) = pSad {
                    iSadCost = sad_fn(
                        &cEnc,
                        &pRefPlane.cursor(kiX + iMvc0 as isize, kiY + iMvc1 as isize),
                    );
                }
                iSadCost += COST_MVD(
                    kpMvdCost,
                    (iMvc0 as i32 * 4) - ksMvp.iMvX as i32,
                    (iMvc1 as i32 * 4) - ksMvp.iMvY as i32,
                ) as i32;

                if iSadCost < iBestSadCost {
                    sMv.iMvX = iMvc0;
                    sMv.iMvY = iMvc1;
                    iBestSadCost = iSadCost;
                }
            }
        }

        if let Some(check_dir) = pMeFuncs.pfCheckDirectionalMv {
            if check_dir(pSad, pMe, ksMvStartMin, ksMvStartMax, pEncPlane, pRefPlane, &mut iSadCost) {
                sMv = (*pMe).sDirectionalMv;
                iBestSadCost = iSadCost;
            }
        }

        UpdateMeResults(sMv, iBestSadCost as u32, pMe);

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

/// Runs after `MeEndIntepelSearch`, so `sMv` is quarter-pel and the integer
/// reference position is `colo + (sMv >> 2)` — `MeRefineFracPixel`'s spelling
/// of the same identity.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CalculateSatdCost(
    pSatd: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        if let Some(satd_fn) = pSatd {
            let kiX = (*pMe).iCurMeBlockPixX as isize;
            let kiY = (*pMe).iCurMeBlockPixY as isize;
            let cRef = pRefPlane.cursor(
                kiX + (((*pMe).sMv.iMvX as isize) >> 2),
                kiY + (((*pMe).sMv.iMvY as isize) >> 2),
            );
            (*pMe).uSadPredISatd.uiSatd = satd_fn(&pEncPlane.cursor(kiX, kiY), &cRef) as u32;
            (*pMe).uiSatdCost = (*pMe).uSadPredISatd.uiSatd
                + COST_MVD(
                    (*pMe).pMvdCost,
                    ((*pMe).sMv.iMvX - (*pMe).sMvp.iMvX) as i32,
                    ((*pMe).sMv.iMvY - (*pMe).sMvp.iMvY) as i32,
                );
        }
    }
}

pub fn NotCalculateSatdCost(
    _pSatd: Option<PSampleSadSatdCostFunc>,
    _pMe: &mut SWelsME,
    _pEncPlane: &PaddedPlane,
    _pRefPlane: &PaddedPlane,
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

// unsafe-cat: port-raw(Phase 9) — pMvdCost (the ctx family's raw MVD table)
#[allow(unsafe_code)]
pub unsafe fn WelsDiamondSearch(
    _pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad4 = sdf.pfSample4Sad[block_size];
        let pSadSingle = sdf.pfSampleSad[block_size];

        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;
        let cEnc = pEncPlane.cursor(kiX, kiY);
        let kpMvdCost = (*pMe).pMvdCost;

        let ksMvStartMin = (*pSlice).sMvStartMin;
        let ksMvStartMax = (*pSlice).sMvStartMax;

        let mut iMvDx = ((*pMe).sMv.iMvX as i32 * 4) - (*pMe).sMvp.iMvX as i32;
        let mut iMvDy = ((*pMe).sMv.iMvY as i32 * 4) - (*pMe).sMvp.iMvY as i32;

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

            // The centre of this iteration's probe: colo + the current
            // integer MV — the walking `pRefMb` this loop used to maintain,
            // proven equal by the identity probe (session F step 1).
            let kiRx = kiX + (*pMe).sMv.iMvX as isize;
            let kiRy = kiY + (*pMe).sMv.iMvY as isize;

            if let Some(sad4_fn) = pSad4 {
                sad4_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy), &mut iSadCosts);
            } else if let Some(sad_fn) = pSadSingle {
                iSadCosts[0] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy - 1));
                iSadCosts[1] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy + 1));
                iSadCosts[2] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx - 1, kiRy));
                iSadCosts[3] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx + 1, kiRy));
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
        }

        (*pMe).sMv.iMvX = ((iMvDx + (*pMe).sMvp.iMvX as i32) >> 2) as i16;
        (*pMe).sMv.iMvY = ((iMvDy + (*pMe).sMvp.iMvY as i32) >> 2) as i16;
        (*pMe).uiSadCost = iBestCost as u32;
        (*pMe).uiSatdCost = (*pMe).uiSadCost;
    }
}

// ============================================================================
// Directional Scrolling Search
// ============================================================================

// unsafe-cat: port-raw(Phase 9) — pMvdCost (the ctx family's raw MVD table)
#[allow(unsafe_code)]
pub unsafe fn CheckDirectionalMv(
    pSad: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
    iBestSadCost: &mut i32,
) -> bool {
    unsafe {
        let kiMvX = (*pMe).sDirectionalMv.iMvX;
        let kiMvY = (*pMe).sDirectionalMv.iMvY;

        if ((*pMe).uiBlockSize as usize != BLOCK_16x16)
            && ((kiMvX != 0) || (kiMvY != 0))
            && CheckMvInRange((*pMe).sDirectionalMv, ksMinMv, ksMaxMv)
        {
            let kiX = (*pMe).iCurMeBlockPixX as isize;
            let kiY = (*pMe).iCurMeBlockPixY as isize;
            let mut uiCurrentSadCost = 0u32;
            if let Some(sad_fn) = pSad {
                uiCurrentSadCost = sad_fn(
                    &pEncPlane.cursor(kiX, kiY),
                    &pRefPlane.cursor(kiX + kiMvX as isize, kiY + kiMvY as isize),
                ) as u32;
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

pub fn CheckDirectionalMvFalse(
    _pSad: Option<PSampleSadSatdCostFunc>,
    _pMe: &mut SWelsME,
    _ksMinMv: SMVUnitXY,
    _ksMaxMv: SMVUnitXY,
    _pEncPlane: &PaddedPlane,
    _pRefPlane: &PaddedPlane,
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
pub unsafe fn LineFullSearch_c(
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pMvdTable: *mut u16,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
    iMinMv: i16,
    iMaxMv: i16,
    bVerticalSearch: bool,
) {
    unsafe {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad = sdf.pfSampleSad[block_size];
        let kiCurMeBlockPixX = (*pMe).iCurMeBlockPixX;
        let kiCurMeBlockPixY = (*pMe).iCurMeBlockPixY;
        let kiX = kiCurMeBlockPixX as isize;
        let kiY = kiCurMeBlockPixY as isize;
        let cEnc = pEncPlane.cursor(kiX, kiY);

        let iMinPos: i32;
        let iMaxPos: i32;
        let iFixedMvd: i32;
        let iCurMeBlockPix: i32;
        let mut pMvdCost: *mut u16;

        if bVerticalSearch {
            iMinPos = kiCurMeBlockPixY + iMinMv as i32;
            iMaxPos = kiCurMeBlockPixY + iMaxMv as i32;
            iFixedMvd = *pMvdTable.offset(-((*pMe).sMvp.iMvX as isize)) as i32;
            iCurMeBlockPix = kiCurMeBlockPixY;
            pMvdCost = pMvdTable.offset(((iMinMv as i32 * 4) - (*pMe).sMvp.iMvY as i32) as isize);
        } else {
            iMinPos = kiCurMeBlockPixX + iMinMv as i32;
            iMaxPos = kiCurMeBlockPixX + iMaxMv as i32;
            iFixedMvd = *pMvdTable.offset(-((*pMe).sMvp.iMvY as isize)) as i32;
            iCurMeBlockPix = kiCurMeBlockPixX;
            pMvdCost = pMvdTable.offset(((iMinMv as i32 * 4) - (*pMe).sMvp.iMvX as i32) as isize);
        }

        let mut uiBestCost: u32 = 0xFFFF_FFFF;
        let mut iBestPos: i32 = 0;

        for iTargetPos in iMinPos..iMaxPos {
            // The walking pRef was colo + d rows (vertical) or d columns.
            let d = (iTargetPos - iCurMeBlockPix) as isize;
            let cRef = if bVerticalSearch {
                pRefPlane.cursor(kiX, kiY + d)
            } else {
                pRefPlane.cursor(kiX + d, kiY)
            };
            let mut uiSadCost: u32 = 0;
            if let Some(sad_fn) = pSad {
                uiSadCost = sad_fn(&cEnc, &cRef) as u32;
            }
            uiSadCost += (iFixedMvd + *pMvdCost as i32) as u32;
            if uiSadCost < uiBestCost {
                uiBestCost = uiSadCost;
                iBestPos = iTargetPos;
            }
            pMvdCost = pMvdCost.add(4);
        }

        if uiBestCost < (*pMe).uiSadCost {
            let mut sBestMv = SMVUnitXY::default();
            sBestMv.iMvX = if bVerticalSearch { 0 } else { (iBestPos - iCurMeBlockPix) as i16 };
            sBestMv.iMvY = if bVerticalSearch { (iBestPos - iCurMeBlockPix) as i16 } else { 0 };
            UpdateMeResults(sBestMv, uiBestCost, pMe);
        }
    }
}

// unsafe-cat: port-raw(Phase 9) — pMvdCost (the ctx family's raw MVD table)
#[allow(unsafe_code)]
pub unsafe fn WelsMotionCrossSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        if let Some(vert_fn) = pMeFuncs.pfVerticalFullSearch {
            vert_fn(
                sdf,
                pMe,
                (*pMe).pMvdCost,
                pEncPlane,
                pRefPlane,
                (*pSlice).sMvStartMin.iMvY,
                (*pSlice).sMvStartMax.iMvY,
                true,
            );
        }

        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            if let Some(horiz_fn) = pMeFuncs.pfHorizontalFullSearch {
                horiz_fn(
                    sdf,
                    pMe,
                    (*pMe).pMvdCost,
                    pEncPlane,
                    pRefPlane,
                    (*pSlice).sMvStartMin.iMvX,
                    (*pSlice).sMvStartMax.iMvX,
                    false,
                );
            }
        }
    }
}

// unsafe-cat: port-raw(Phase 9) — pRefFeatureStorage (Phase 10's raw storage)
#[allow(unsafe_code)]
pub unsafe fn WelsDiamondCrossSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        WelsDiamondSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);

        if !(*pMe).pRefFeatureStorage.is_null() {
            let block_size = (*pMe).uiBlockSize as usize;
            (*pMe).uiSadCostThreshold = (*(*pMe).pRefFeatureStorage).uiSadCostThreshold[block_size];
        }
        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            WelsMotionCrossSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);
        }
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn WelsDiamondCrossFeatureSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME,
    pSlice: &mut SSlice,
    pEncPlane: &PaddedPlane,
    pRefPlane: &PaddedPlane,
) {
    unsafe {
        WelsDiamondCrossSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);

        if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
            (*pSlice).uiSliceFMECostDown = (*pSlice).uiSliceFMECostDown.wrapping_add((*pMe).uiSadCost);

            let mut sFeatureSearchIn = SFeatureSearchIn::default();
            if SetFeatureSearchIn(
                pMeFuncs,
                sdf,
                pMe,
                &*pSlice,
                (*pMe).pRefFeatureStorage,
                pEncPlane,
                pRefPlane,
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

/// The safe per-block twin of [`SumOf8x8SingleBlock_c`] — what the
/// `pfCalculateSingleBlockFeature` slot holds since session F. The raw kernel
/// stays for the frame-feature builders, which walk a whole raw plane.
// SCREEN_CONTENT(dormant: Phase 10)
pub fn sum_of_8x8_single_block(cRef: &PlaneCursor<'_>) -> i32 {
    let mut iSum = 0i32;
    for y in 0..8 {
        for &b in cRef.row(y, 0, 8) {
            iSum += b as i32;
        }
    }
    iSum
}

/// As [`sum_of_8x8_single_block`], 16x16.
// SCREEN_CONTENT(dormant: Phase 10)
pub fn sum_of_16x16_single_block(cRef: &PlaneCursor<'_>) -> i32 {
    let mut iSum = 0i32;
    for y in 0..16 {
        for &b in cRef.row(y, 0, 16) {
            iSum += b as i32;
        }
    }
    iSum
}

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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

// unsafe-cat: SCREEN_CONTENT(dormant)
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
pub unsafe fn SetFeatureSearchIn<'a>(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    sMe: &SWelsME,
    pSlice: &SSlice,
    pRefFeatureStorage: *mut SScreenBlockFeatureStorage,
    pEncPlane: &'a PaddedPlane,
    pRefPlane: &'a PaddedPlane,
    pFeatureSearchIn: &mut SFeatureSearchIn<'a>,
) -> bool {
    unsafe {
        let block_size = sMe.uiBlockSize as usize;
        pFeatureSearchIn.pSad = sdf.pfSampleSad[block_size];

        let single_fn_idx = if block_size == BLOCK_16x16 { 1 } else { 0 };
        if let Some(calc_single) = pMeFuncs.pfCalculateSingleBlockFeature[single_fn_idx] {
            pFeatureSearchIn.iFeatureOfCurrent = calc_single(
                &pEncPlane.cursor(sMe.iCurMeBlockPixX as isize, sMe.iCurMeBlockPixY as isize),
            );
        }

        pFeatureSearchIn.pEncPlane = Some(pEncPlane);
        pFeatureSearchIn.pRefPlane = Some(pRefPlane);
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
pub fn SaveFeatureSearchOut(
    sBestMv: SMVUnitXY,
    uiBestSadCost: u32,
    pFeatureSearchOut: &mut SFeatureSearchOut,
) {
    pFeatureSearchOut.sBestMv = sBestMv;
    pFeatureSearchOut.uiBestSadCost = uiBestSadCost;
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn FeatureSearchOne(
    sFeatureSearchIn: &SFeatureSearchIn<'_>,
    iFeatureDifference: i32,
    kuiExpectedSearchTimes: u32,
    pFeatureSearchOut: &mut SFeatureSearchOut,
) -> bool {
    let iFeatureOfRef = sFeatureSearchIn.iFeatureOfCurrent + iFeatureDifference;
    if iFeatureOfRef < 0 || iFeatureOfRef >= LIST_SIZE {
        return true;
    }

    let pSad = sFeatureSearchIn.pSad;
    let pEncPlane = sFeatureSearchIn.pEncPlane.expect("SetFeatureSearchIn bound the planes");
    let pRefPlane = sFeatureSearchIn.pRefPlane.expect("SetFeatureSearchIn bound the planes");
    let uiSadCostThresh = sFeatureSearchIn.uiSadCostThresh as u32;

    let iCurPixX = sFeatureSearchIn.iCurPixX;
    let iCurPixY = sFeatureSearchIn.iCurPixY;
    let iCurPixXQpel = sFeatureSearchIn.iCurPixXQpel;
    let iCurPixYQpel = sFeatureSearchIn.iCurPixYQpel;
    let cEnc = pEncPlane.cursor(iCurPixX as isize, iCurPixY as isize);

    let iMinQpelX = sFeatureSearchIn.iMinQpelX;
    let iMinQpelY = sFeatureSearchIn.iMinQpelY;
    let iMaxQpelX = sFeatureSearchIn.iMaxQpelX;
    let iMaxQpelY = sFeatureSearchIn.iMaxQpelY;

    unsafe {
        let times = *sFeatureSearchIn.pTimesOfFeature.offset(iFeatureOfRef as isize);
        let iSearchTimes = times.min(kuiExpectedSearchTimes) as i32;
        let iSearchTimesx2 = iSearchTimes << 1;
        let pQpelPosition = *sFeatureSearchIn.pQpelLocationOfFeature.offset(iFeatureOfRef as isize);

        let mut sBestMv = pFeatureSearchOut.sBestMv;
        let mut uiBestCost = pFeatureSearchOut.uiBestSadCost;

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

            if let Some(sad_fn) = pSad {
                uiTmpCost += sad_fn(
                    &cEnc,
                    &pRefPlane.cursor(
                        (iCurPixX + iIntepelX) as isize,
                        (iCurPixY + iIntepelY) as isize,
                    ),
                ) as u32;
            }

            if uiTmpCost < uiBestCost {
                sBestMv.iMvX = iIntepelX as i16;
                sBestMv.iMvY = iIntepelY as i16;
                uiBestCost = uiTmpCost;

                if uiBestCost < uiSadCostThresh {
                    break;
                }
            }

            i += 2;
        }

        SaveFeatureSearchOut(sBestMv, uiBestCost, pFeatureSearchOut);
        i < iSearchTimesx2
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn MotionEstimateFeatureFullSearch(
    sFeatureSearchIn: SFeatureSearchIn<'_>,
    kuiMaxSearchPoint: u32,
    pMe: &mut SWelsME,
) {
    unsafe {
        let mut sFeatureSearchOut = SFeatureSearchOut {
            sBestMv: (*pMe).sMv,
            uiBestSadCost: (*pMe).uiSadCost,
        };

        let iFeatureDifference = 0i32;
        FeatureSearchOne(&sFeatureSearchIn, iFeatureDifference, kuiMaxSearchPoint, &mut sFeatureSearchOut);

        if sFeatureSearchOut.uiBestSadCost < (*pMe).uiSadCost {
            UpdateMeResults(
                sFeatureSearchOut.sBestMv,
                sFeatureSearchOut.uiBestSadCost,
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
    // unsafe-cat: instrument(test)
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
    // unsafe-cat: instrument(test)
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
    // unsafe-cat: instrument(test)
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
    // unsafe-cat: instrument(test)
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
