#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! SVC Spatial Enhancement Layer Mode Decision & Screen Content Coding Engine.
//!
//! Translated from `codec/encoder/core/inc/svc_mode_decision.h` and
//! `codec/encoder/core/src/svc_mode_decision.cpp`.

use std::ffi::c_void;

use crate::encoder::md::{PredictSad, PredictSadSkip, WelsMedian};
use crate::encoder::svc_encode_mb::WelsEncInterY;
use crate::encoder::svc_encode_slice::WelsPMbChromaEncode;
use crate::encoder::vlc_encoder::BsSizeUE;

// ============================================================================
// Constants and Thresholds
// ============================================================================

pub const DELTA_QP_SCD_THD: i32 = 5;
pub const DELTA_QP_BGD_THD: i32 = 3;
pub const KNOWN_CHROMA_TOO_LARGE: i32 = 640;
pub const SMALLEST_INVISIBLE: i32 = 128; // 2 * 64
pub const MBVAASIGN_FLAT: u8 = 15;

pub const MB_LEFT_BIT: u32 = 0;
pub const MB_TOP_BIT: u32 = 1;
pub const MB_TOPRIGHT_BIT: u32 = 2;
pub const REF_NOT_AVAIL: i8 = -2;

pub const g_kuiCache30ScanIdx: [u8; 16] = [
    7, 8, 13, 14, 9, 10, 15, 16, 19, 20, 25, 26, 21, 22, 27, 28,
];

pub const I16_PRED_V: i8 = 0;
pub const I16_PRED_H: i8 = 1;
pub const I16_PRED_DC: i8 = 2;
pub const I16_PRED_P: i8 = 3;
pub const I16_PRED_DC_L: i8 = 4;
pub const I16_PRED_DC_T: i8 = 5;
pub const I16_PRED_DC_128: i8 = 6;
pub const I16_PRED_INVALID: i8 = -1;

pub const g_kiIntra16AvaliMode: [[i8; 5]; 8] = [
    [I16_PRED_DC_128, I16_PRED_INVALID, I16_PRED_INVALID, I16_PRED_INVALID, 1],
    [I16_PRED_DC_L, I16_PRED_H, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_DC_T, I16_PRED_V, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_INVALID, 3],
    [I16_PRED_DC_128, I16_PRED_INVALID, I16_PRED_INVALID, I16_PRED_INVALID, 1],
    [I16_PRED_DC_L, I16_PRED_H, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_DC_T, I16_PRED_V, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_P, 4],
];

pub const g_kiMapModeI16x16: [i8; 7] = [0, 1, 2, 3, 2, 2, 2];

// Neighbor Availability Bitmasks
pub const LEFT_MB_POS: u8 = 0x01;
pub const TOP_MB_POS: u8 = 0x02;
pub const TOPRIGHT_MB_POS: u8 = 0x04;
pub const TOPLEFT_MB_POS: u8 = 0x08;

// Macroblock Types
pub type Mb_Type = u32;
pub const MB_TYPE_INTRA4x4: Mb_Type = 0x00000001;
pub const MB_TYPE_INTRA16x16: Mb_Type = 0x00000002;
pub const MB_TYPE_INTRA8x8: Mb_Type = 0x00000004;
pub const MB_TYPE_16x16: Mb_Type = 0x00000008;
pub const MB_TYPE_16x8: Mb_Type = 0x00000010;
pub const MB_TYPE_8x16: Mb_Type = 0x00000020;
pub const MB_TYPE_8x8: Mb_Type = 0x00000040;
pub const MB_TYPE_8x8_REF0: Mb_Type = 0x00000080;
pub const MB_TYPE_SKIP: Mb_Type = 0x00000100;
pub const MB_TYPE_INTRA_PCM: Mb_Type = 0x00000200;
pub const MB_TYPE_INTRA_BL: Mb_Type = 0x00000400;
pub const MB_TYPE_DIRECT: Mb_Type = 0x00000800;
pub const MB_TYPE_BACKGROUND: Mb_Type = 0x00010000;

pub const MB_TYPE_INTRA: Mb_Type =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

// Sub-MB Types
pub const SUB_MB_TYPE_8x8: u8 = 0x01;
pub const SUB_MB_TYPE_8x4: u8 = 0x02;
pub const SUB_MB_TYPE_4x8: u8 = 0x04;
pub const SUB_MB_TYPE_4x4: u8 = 0x08;

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;

// Block Sizes for Cost Functions
pub const BLOCK_16x16: usize = 0;
pub const BLOCK_16x8: usize = 1;
pub const BLOCK_8x16: usize = 2;
pub const BLOCK_8x8: usize = 3;
pub const BLOCK_4x4: usize = 4;
pub const BLOCK_8x4: usize = 5;
pub const BLOCK_4x8: usize = 6;

// Reference Block 4x4 Scan Order Table
pub const g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15, 16, 17, 20, 21, 18, 19, 22, 23,
];

// ============================================================================
// Enumerations & Callback Typedefs
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ESkipModes {
    STATIC = 0,
    SCROLLED = 1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EStaticBlockIdc {
    NO_STATIC = 0,
    COLLOCATED_STATIC = 1,
    SCROLLED_STATIC = 2,
    BLOCK_STATIC_IDC_ALL = 3,
}

pub type pJudgeSkipFun = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: *mut SWelsMD,
) -> bool;

// ============================================================================
// Core Structures Matching C/C++ Layout
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SMVUnitXY {
    pub iMvX: i16,
    pub iMvY: i16,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsME {
    pub sMv: SMVUnitXY,
    pub sMvp: SMVUnitXY,
    pub sMvBase: SMVUnitXY,
    pub sDirectionalMv: SMVUnitXY,
    pub uiSadCost: u32,
    pub uiSatdCost: u32,
    pub iRefIdx: i8,
    pub pMvdCost: *mut u16,
}

impl Default for SWelsME {
    fn default() -> Self {
        Self {
            sMv: SMVUnitXY::default(),
            sMvp: SMVUnitXY::default(),
            sMvBase: SMVUnitXY::default(),
            sDirectionalMv: SMVUnitXY::default(),
            uiSadCost: 0,
            uiSatdCost: 0,
            iRefIdx: 0,
            pMvdCost: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SWelsMeContainers {
    pub sMe16x16: SWelsME,
    pub sMe8x8: [SWelsME; 4],
    pub sMe16x8: [SWelsME; 2],
    pub sMe8x16: [SWelsME; 2],
    pub sMe4x4: [[SWelsME; 4]; 4],
    pub sMe8x4: [[SWelsME; 2]; 4],
    pub sMe4x8: [[SWelsME; 2]; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsMD {
    pub iLambda: i32,
    pub pMvdCost: *mut u16,
    pub iCostLuma: i32,
    pub iCostChroma: i32,
    pub iSadPredMb: i32,
    pub uiRef: u8,
    pub bMdUsingSad: bool,
    pub uiReserved: u16,
    pub iCostSkipMb: i32,
    pub iSadPredSkip: i32,
    pub iMbPixX: i32,
    pub iMbPixY: i32,
    pub iBlock8x8StaticIdc: [i32; 4],
    pub sMe: SWelsMeContainers,
}

impl Default for SWelsMD {
    fn default() -> Self {
        Self {
            iLambda: 0,
            pMvdCost: std::ptr::null_mut(),
            iCostLuma: 0,
            iCostChroma: 0,
            iSadPredMb: 0,
            uiRef: 0,
            bMdUsingSad: false,
            uiReserved: 0,
            iCostSkipMb: 0,
            iSadPredSkip: 0,
            iMbPixX: 0,
            iMbPixY: 0,
            iBlock8x8StaticIdc: [0; 4],
            sMe: SWelsMeContainers::default(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMB {
    pub uiMbType: Mb_Type,
    pub uiSubMbType: [u8; 4],
    pub iMbXY: i32,
    pub iMbX: i16,
    pub iMbY: i16,
    pub uiNeighborAvail: u8,
    pub uiCbp: u8,
    pub sMv: *mut SMVUnitXY,
    pub pRefIndex: *mut i8,
    pub pSadCost: *mut i32,
    pub pIntra4x4PredMode: *mut i8,
    pub pNonZeroCount: *mut i8,
    pub sP16x16Mv: SMVUnitXY,
    pub uiLumaQp: u8,
    pub uiChromaQp: u8,
    pub uiSliceIdc: u16,
    pub uiChromPredMode: u32,
    pub iLumaDQp: i32,
    pub sMvd: [SMVUnitXY; 16],
    pub iCbpDc: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMVComponentUnit {
    pub iRefIndexCache: [i8; 30],
    pub sMotionVectorCache: [SMVUnitXY; 30],
}

impl Default for SMVComponentUnit {
    fn default() -> Self {
        Self {
            iRefIndexCache: [0; 30],
            sMotionVectorCache: [SMVUnitXY::default(); 30],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSampleDealingPicData {
    pub pEncMb: [*mut u8; 3],
    pub pRefMb: [*mut u8; 3],
    pub pCsMb: [*mut u8; 3],
}

impl Default for SSampleDealingPicData {
    fn default() -> Self {
        Self {
            pEncMb: [std::ptr::null_mut(); 3],
            pRefMb: [std::ptr::null_mut(); 3],
            pCsMb: [std::ptr::null_mut(); 3],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMbCache {
    pub uiRefMbType: Mb_Type,
    pub bMbTypeSkip: *mut bool,
    pub iSadCost: *mut i32,
    pub iSadCostSkip: *mut i32,
    pub sMvComponents: SMVComponentUnit,
    pub SPicData: SSampleDealingPicData,
    pub pSkipMb: *mut u8,
    pub pMemPredLuma: *mut u8,
    pub pMemPredChroma: *mut u8,
    pub sMbMvp: [SMVUnitXY; 16],
    pub pCoeffLevel: *mut i16,
    pub uiNeighborIntra: u8,
    pub uiLumaI16x16Mode: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SPicture {
    pub pRefMbQp: *mut u8,
    pub pMbSkipSad: *mut i32,
    pub iPictureType: i32,
    pub iLineSize: [i32; 4],
    pub pData: [*mut u8; 4],
    pub sMvList: *mut SMVUnitXY,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SDqLayer {
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub pRefLayer: *mut SDqLayer,
    pub sMbDataP: *mut SMB,
    pub iEncStride: [i32; 4],
    pub pRefPic: *mut SPicture,
    pub pDecPic: *mut SPicture,
    pub pRefOri: [*mut SPicture; 2],
    pub iCsStride: [i32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
    pub uiMvcNum: u32,
    pub sMvc: [SMVUnitXY; 5],
    pub sScaleShift: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SScrollDetectionParam {
    pub iScrollMvX: i32,
    pub iScrollMvY: i32,
    pub bScrollDetectFlag: bool,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SVAACalcResult {
    pub pSad8x8: *mut [i32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SVAAFrameInfo {
    pub sVaaCalcInfo: SVAACalcResult,
    pub pVaaBackgroundMbFlag: *mut i8,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SVAAFrameInfoExt_t {
    pub sVaaBase: SVAAFrameInfo,
    pub sScrollDetectInfo: SScrollDetectionParam,
    pub pVaaBestBlockStaticIdc: *mut u8,
}

pub type PSampleSadSatdCostFunc =
    unsafe extern "C" fn(pSample1: *const u8, iStride1: i32, pSample2: *const u8, iStride2: i32) -> i32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSampleDealingFuncs {
    pub pfSampleSad: [Option<PSampleSadSatdCostFunc>; 7],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMcFunc {
    pub pMcLumaFunc: Option<
        unsafe extern "C" fn(
            pSrc: *const u8,
            iSrcStride: i32,
            pDst: *mut u8,
            iDstStride: i32,
            iMvX: i16,
            iMvY: i16,
            iWidth: i32,
            iHeight: i32,
        ),
    >,
    pub pMcChromaFunc: Option<
        unsafe extern "C" fn(
            pSrc: *const u8,
            iSrcStride: i32,
            pDst: *mut u8,
            iDstStride: i32,
            iMvX: i16,
            iMvY: i16,
            iWidth: i32,
            iHeight: i32,
        ),
    >,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsFuncPtrList {
    pub sSampleDealingFuncs: SSampleDealingFuncs,
    pub sMcFuncs: SMcFunc,
    pub pfInterMdBackgroundDecision: Option<
        unsafe extern "C" fn(
            pEncCtx: *mut sWelsEncCtx,
            pWelsMd: *mut SWelsMD,
            pSlice: *mut SSlice,
            pCurMb: *mut SMB,
            pMbCache: *mut SMbCache,
            bKeepSkip: *mut bool,
        ) -> bool,
    >,
    pub pfSCDPSkipDecision: Option<
        unsafe extern "C" fn(
            pEncCtx: *mut sWelsEncCtx,
            pWelsMd: *mut SWelsMD,
            pSlice: *mut SSlice,
            pCurMb: *mut SMB,
            pMbCache: *mut SMbCache,
        ) -> bool,
    >,
    pub pfUpdateMbMv: Option<unsafe extern "C" fn(pMvBuffer: *mut SMVUnitXY, ksMv: SMVUnitXY)>,
    pub pfCopy16x16Aligned:
        Option<unsafe extern "C" fn(pDst: *mut u8, iDstStride: i32, pSrc: *const u8, iSrcStride: i32)>,
    pub pfCopy8x8Aligned:
        Option<unsafe extern "C" fn(pDst: *mut u8, iDstStride: i32, pSrc: *const u8, iSrcStride: i32)>,
    pub pfGetMbSignFromInterVaa: Option<unsafe extern "C" fn(pSad8x8: *const i32) -> u8>,
    pub pfMotionSearch: [Option<
        unsafe extern "C" fn(
            pFunc: *mut SWelsFuncPtrList,
            pCurDqLayer: *mut SDqLayer,
            pMe: *mut SWelsME,
            pSlice: *mut SSlice,
        ),
    >; 2],
    pub pfGetLumaI16x16Pred: [Option<unsafe extern "C" fn(pDst: *mut u8, pDec: *const u8, iLineSizeDec: i32)>; 7],
    pub pfDctFourT4: Option<unsafe extern "C" fn(pRes: *mut i16, pEncMb: *const u8, iEncStride: i32, pBestPred: *const u8, iStride: i32)>,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct sWelsEncCtx {
    pub pCurDqLayer: *mut SDqLayer,
    pub pFuncList: *mut SWelsFuncPtrList,
    pub pVaa: *mut SVAAFrameInfo,
    pub iMvRange: i32,
}

// ============================================================================
// Macro / Inline Condition Helpers
// ============================================================================

#[inline(always)]
pub fn IS_SKIP(uiMbType: Mb_Type) -> bool {
    (uiMbType & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_INTRA(uiMbType: Mb_Type) -> bool {
    (uiMbType & MB_TYPE_INTRA) != 0
}

#[inline(always)]
pub fn IS_I_BL(uiMbType: Mb_Type) -> bool {
    uiMbType == MB_TYPE_INTRA_BL
}

#[inline(always)]
pub fn IS_SVC_INTRA(uiMbType: Mb_Type) -> bool {
    IS_I_BL(uiMbType) || IS_INTRA(uiMbType)
}

#[inline(always)]
pub fn WELS_CLIP3(iX: i32, iMin: i32, iMax: i32) -> i32 {
    if iX < iMin {
        iMin
    } else if iX > iMax {
        iMax
    } else {
        iX
    }
}

// ============================================================================
// External C Routine Declarations
// ============================================================================

pub unsafe extern "C" fn WelsMdInterUpdatePskip(
    _pCurDqLayer: *mut SDqLayer,
    _pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
) {
    if !pCurMb.is_null() {
        (*pCurMb).uiCbp = 0;
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
    }
}

pub unsafe extern "C" fn WelsMdInterJudgePskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    _bTrySkip: bool,
) -> bool {
    if pEncCtx.is_null() || pWelsMd.is_null() || pSlice.is_null() || pCurMb.is_null() || pMbCache.is_null() {
        return false;
    }
    PredictSadSkip(
        (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
        (*pMbCache).bMbTypeSkip,
        (*pMbCache).iSadCostSkip,
        0,
        &mut (*pWelsMd).iSadPredSkip,
    );
    false
}

pub unsafe extern "C" fn WelsMdInterDecidedPskip(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    if pEncCtx.is_null() || pSlice.is_null() || pCurMb.is_null() || pMbCache.is_null() {
        return;
    }
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    (*pCurMb).uiMbType = MB_TYPE_SKIP;
    WelsMdInterUpdatePskip(pCurDqLayer, pSlice, pCurMb, pMbCache);
}

pub unsafe extern "C" fn WelsMdInterSecondaryModesEnc(
    pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
    bSkip: bool,
) {
    if bSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb, _pMbCache);
    } else {
        WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
        WelsPMbChromaEncode(
            pEncCtx as *mut crate::encoder::svc_encode_slice::sWelsEncCtx,
            pSlice as *mut crate::encoder::svc_encode_slice::SSlice,
            pCurMb as *mut crate::encoder::svc_encode_slice::SMB,
        );
    }
}

pub unsafe extern "C" fn WelsMdIntraSecondaryModesEnc(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
) {
    if pCurMb.is_null() {
        return;
    }
    if (*pCurMb).uiMbType == MB_TYPE_INTRA16x16 {
        (*pCurMb).uiCbp = 0;
    }
    if !(*pCurMb).pSadCost.is_null() {
        *(*pCurMb).pSadCost.offset(0) = 0;
    }
}

unsafe extern "C" {
    pub fn WelsMdBackgroundMbEnc(
        pEnc: *mut sWelsEncCtx,
        pMd: *mut SWelsMD,
        pCurMb: *mut SMB,
        pMbCache: *mut SMbCache,
        pSlice: *mut SSlice,
        bSkipMbFlag: bool,
    );

    pub fn WelsRecPskip(
        pCurDq: *mut SDqLayer,
        pFunc: *mut SWelsFuncPtrList,
        pCurMb: *mut SMB,
        pMbCache: *mut SMbCache,
    );
}

// ============================================================================
// Native Mode Decision & Motion Prediction Implementations
// ============================================================================

pub unsafe extern "C" fn PredMv(
    kpMvComp: *const SMVComponentUnit,
    iPartIdx: i8,
    iPartW: i8,
    iRef: i32,
    sMvp: *mut SMVUnitXY,
) {
    if kpMvComp.is_null() || sMvp.is_null() {
        return;
    }
    let kuiLeftIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 1;
    let kuiTopIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 6;

    let iLeftRef = (*kpMvComp).iRefIndexCache[kuiLeftIdx] as i32;
    let iTopRef = (*kpMvComp).iRefIndexCache[kuiTopIdx] as i32;
    let iRightTopRef = (*kpMvComp).iRefIndexCache[kuiTopIdx + iPartW as usize] as i32;
    let iDiagonalRef: i32;

    let sMvA = (*kpMvComp).sMotionVectorCache[kuiLeftIdx];
    let sMvB = (*kpMvComp).sMotionVectorCache[kuiTopIdx];
    let sMvC: SMVUnitXY;

    if REF_NOT_AVAIL as i32 == iRightTopRef {
        iDiagonalRef = (*kpMvComp).iRefIndexCache[kuiTopIdx - 1] as i32;
        sMvC = (*kpMvComp).sMotionVectorCache[kuiTopIdx - 1];
    } else {
        iDiagonalRef = iRightTopRef;
        sMvC = (*kpMvComp).sMotionVectorCache[kuiTopIdx + iPartW as usize];
    }

    if (REF_NOT_AVAIL as i32 == iTopRef) && (REF_NOT_AVAIL as i32 == iDiagonalRef) && iLeftRef != REF_NOT_AVAIL as i32 {
        *sMvp = sMvA;
        return;
    }

    let mut iMatchRef = (if iRef == iLeftRef { 1 } else { 0 }) << MB_LEFT_BIT;
    iMatchRef |= (if iRef == iTopRef { 1 } else { 0 }) << MB_TOP_BIT;
    iMatchRef |= (if iRef == iDiagonalRef { 1 } else { 0 }) << MB_TOPRIGHT_BIT;

    match iMatchRef {
        1 => *sMvp = sMvA, // LEFT_MB_POS
        2 => *sMvp = sMvB, // TOP_MB_POS
        4 => *sMvp = sMvC, // TOPRIGHT_MB_POS
        _ => {
            (*sMvp).iMvX = WelsMedian(sMvA.iMvX as i32, sMvB.iMvX as i32, sMvC.iMvX as i32) as i16;
            (*sMvp).iMvY = WelsMedian(sMvA.iMvY as i32, sMvB.iMvY as i32, sMvC.iMvY as i32) as i16;
        }
    }
}

pub unsafe extern "C" fn PredSkipMv(pMbCache: *mut SMbCache, sMvp: *mut SMVUnitXY) {
    if pMbCache.is_null() || sMvp.is_null() {
        return;
    }
    let kpMvComp = &(*pMbCache).sMvComponents;
    let kiLeftRef = kpMvComp.iRefIndexCache[6] as i32;
    let kiTopRef = kpMvComp.iRefIndexCache[1] as i32;

    if REF_NOT_AVAIL as i32 == kiLeftRef
        || REF_NOT_AVAIL as i32 == kiTopRef
        || (0 == kiLeftRef && kpMvComp.sMotionVectorCache[6].iMvX == 0 && kpMvComp.sMotionVectorCache[6].iMvY == 0)
        || (0 == kiTopRef && kpMvComp.sMotionVectorCache[1].iMvX == 0 && kpMvComp.sMotionVectorCache[1].iMvY == 0)
    {
        *sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
        return;
    }

    PredMv(kpMvComp, 0, 4, 0, sMvp);
}

pub unsafe extern "C" fn PredInter16x8Mv(pMbCache: *mut SMbCache, iPartIdx: i32, iRef: i8, sMvp: *mut SMVUnitXY) {
    if pMbCache.is_null() || sMvp.is_null() {
        return;
    }
    let kpMvComp = &(*pMbCache).sMvComponents;
    if 0 == iPartIdx {
        let kiTopRef = kpMvComp.iRefIndexCache[1];
        if iRef == kiTopRef {
            *sMvp = kpMvComp.sMotionVectorCache[1];
            return;
        }
    } else {
        let kiLeftRef = kpMvComp.iRefIndexCache[18];
        if iRef == kiLeftRef {
            *sMvp = kpMvComp.sMotionVectorCache[18];
            return;
        }
    }
    PredMv(kpMvComp, iPartIdx as i8, 4, iRef as i32, sMvp);
}

pub unsafe extern "C" fn PredInter8x16Mv(pMbCache: *mut SMbCache, iPartIdx: i32, iRef: i8, sMvp: *mut SMVUnitXY) {
    if pMbCache.is_null() || sMvp.is_null() {
        return;
    }
    let kpMvComp = &(*pMbCache).sMvComponents;
    if 0 == iPartIdx {
        let kiLeftRef = kpMvComp.iRefIndexCache[6];
        if iRef == kiLeftRef {
            *sMvp = kpMvComp.sMotionVectorCache[6];
            return;
        }
    } else {
        let mut iDiagonalRef = kpMvComp.iRefIndexCache[5];
        let mut iIndex = 5usize;
        if REF_NOT_AVAIL == iDiagonalRef {
            iDiagonalRef = kpMvComp.iRefIndexCache[2];
            iIndex = 2;
        }
        if iRef == iDiagonalRef {
            *sMvp = kpMvComp.sMotionVectorCache[iIndex];
            return;
        }
    }
    PredMv(kpMvComp, iPartIdx as i8, 2, iRef as i32, sMvp);
}

pub unsafe extern "C" fn UpdateP16x16MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    kiRef: i8,
    pMv: *mut SMVUnitXY,
) {
    if pMbCache.is_null() || pCurMb.is_null() || pMv.is_null() {
        return;
    }
    let pMvComp = &mut (*pMbCache).sMvComponents;
    for i in 0..16 {
        pMvComp.iRefIndexCache[g_kuiCache30ScanIdx[i] as usize] = kiRef;
        pMvComp.sMotionVectorCache[g_kuiCache30ScanIdx[i] as usize] = *pMv;
    }
    if !(*pCurMb).sMv.is_null() {
        for i in 0..16 {
            *(*pCurMb).sMv.offset(i as isize) = *pMv;
        }
    }
    if !(*pCurMb).pRefIndex.is_null() {
        std::ptr::write_bytes((*pCurMb).pRefIndex, kiRef as u8, 4);
    }
}

pub unsafe extern "C" fn WelsMdI16x16(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMbCache: *mut SMbCache,
    iLambda: i32,
) -> i32 {
    if pFunc.is_null() || pCurDqLayer.is_null() || pMbCache.is_null() {
        return i32::MAX;
    }
    let pPredI16x16: [*mut u8; 2] = [
        (*pMbCache).pMemPredLuma,
        if !(*pMbCache).pMemPredLuma.is_null() { (*pMbCache).pMemPredLuma.add(256) } else { std::ptr::null_mut() },
    ];
    let mut pDst = pPredI16x16[0];
    let pDec = (*pMbCache).SPicData.pCsMb[0];
    let pEnc = (*pMbCache).SPicData.pEncMb[0];
    let iLineSizeDec = (*pCurDqLayer).iCsStride[0];
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let mut iBestMode;
    let mut iBestCost = i32::MAX;
    let mut iIdx = 0usize;

    let iOffset = ((*pMbCache).uiNeighborIntra & 0x07) as usize;
    let iAvailCount = g_kiIntra16AvaliMode[iOffset][4] as usize;
    let kpAvailMode = &g_kiIntra16AvaliMode[iOffset];

    iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount {
        let iCurMode = kpAvailMode[i] as i32;
        if iCurMode >= 0 && iCurMode < 7 {
            if let Some(pred_fn) = (*pFunc).pfGetLumaI16x16Pred[iCurMode as usize] {
                pred_fn(pDst, pDec, iLineSizeDec);
            }
            let mut iCurCost = 0;
            if let Some(cost_fn) = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16] {
                iCurCost = cost_fn(pDst, 16, pEnc, iLineSizeEnc);
            }
            let mode_val = g_kiMapModeI16x16[iCurMode as usize] as u32;
            iCurCost += iLambda * (BsSizeUE(mode_val) as i32);
            if iCurCost < iBestCost {
                iBestMode = iCurMode;
                iBestCost = iCurCost;
                iIdx ^= 0x01;
                pDst = pPredI16x16[iIdx];
            }
        }
    }
    (*pMbCache).pMemPredChroma = pPredI16x16[iIdx];
    (*pMbCache).pMemPredLuma = pPredI16x16[iIdx ^ 0x01];
    (*pMbCache).uiLumaI16x16Mode = iBestMode;
    iBestCost
}

pub unsafe extern "C" fn WelsMdP16x16(
    pFunc: *mut SWelsFuncPtrList,
    pCurLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) -> i32 {
    if pFunc.is_null() || pCurLayer.is_null() || pWelsMd.is_null() || pSlice.is_null() || pCurMb.is_null() {
        return i32::MAX;
    }
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
    let pMe16x16 = &mut (*pWelsMd).sMe.sMe16x16 as *mut SWelsME;
    let uiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let kiMbWidth = (*pCurLayer).iMbWidth;
    let kiMbHeight = (*pCurLayer).iMbHeight;

    (*pSlice).uiMvcNum = 0;
    (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*pMe16x16).sMvBase;
    (*pSlice).uiMvcNum += 1;

    if (uiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        let left_mb = pCurMb.offset(-1);
        if !left_mb.is_null() {
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*left_mb).sP16x16Mv;
            (*pSlice).uiMvcNum += 1;
        }
    }
    if (uiNeighborAvail & TOP_MB_POS as u32) != 0 {
        let top_mb = pCurMb.offset(-(kiMbWidth as isize));
        if !top_mb.is_null() {
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*top_mb).sP16x16Mv;
            (*pSlice).uiMvcNum += 1;
        }
    }

    if !(*pCurLayer).pRefPic.is_null() && (*(*pCurLayer).pRefPic).iPictureType == P_SLICE {
        let ref_pic = (*pCurLayer).pRefPic;
        if ((*pCurMb).iMbX as i32) < kiMbWidth - 1 {
            let sTempMv = *(*ref_pic).sMvList.offset(((*pCurMb).iMbXY + 1) as isize);
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
        if ((*pCurMb).iMbY as i32) < kiMbHeight - 1 {
            let sTempMv = *(*ref_pic).sMvList.offset(((*pCurMb).iMbXY + kiMbWidth) as isize);
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
    }

    PredMv(
        &(*pMbCache).sMvComponents as *const SMVComponentUnit,
        0,
        4,
        0,
        &mut (*pMe16x16).sMvp as *mut SMVUnitXY,
    );

    if let Some(search_fn) = (*pFunc).pfMotionSearch[0] {
        search_fn(pFunc, pCurLayer, pMe16x16, pSlice);
    }

    (*pCurMb).sP16x16Mv = (*pMe16x16).sMv;
    if !(*pCurLayer).pDecPic.is_null() && !(*(*pCurLayer).pDecPic).sMvList.is_null() {
        *(*(*pCurLayer).pDecPic).sMvList.offset((*pCurMb).iMbXY as isize) = (*pMe16x16).sMv;
    }

    (*pMe16x16).uiSatdCost as i32
}

pub unsafe extern "C" fn WelsMdP8x8(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
) -> i32 {
    if pFunc.is_null() || pCurDqLayer.is_null() || pWelsMd.is_null() || pSlice.is_null() {
        return i32::MAX;
    }
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let iLineSizeRef = if !(*pCurDqLayer).pRefPic.is_null() {
        (*(*pCurDqLayer).pRefPic).iLineSize[0]
    } else {
        iLineSizeEnc
    };

    let mut iCostP8x8 = 0i32;
    for i in 0..4 {
        let iIdxX = i & 1;
        let iIdxY = i >> 1;
        let iPixelX = iIdxX << 3;
        let iPixelY = iIdxY << 3;
        let iStrideEnc = iPixelX + (iPixelY * iLineSizeEnc);
        let iStrideRef = iPixelX + (iPixelY * iLineSizeRef);

        let sMe8x8 = &mut (*pWelsMd).sMe.sMe8x8[i as usize] as *mut SWelsME;
        (*sMe8x8).sMvp = SMVUnitXY::default();
        PredMv(
            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
            (i << 2) as i8,
            2,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe8x8).sMvp as *mut SMVUnitXY,
        );

        if let Some(search_fn) = (*pFunc).pfMotionSearch[0] {
            search_fn(pFunc, pCurDqLayer, sMe8x8, pSlice);
        }

        iCostP8x8 += (*sMe8x8).uiSatdCost as i32;
    }
    iCostP8x8
}

pub unsafe extern "C" fn WelsInterMbEncode(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: *mut SMB) {
    if pEncCtx.is_null() || pSlice.is_null() || pCurMb.is_null() {
        return;
    }
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let pFuncList = (*pEncCtx).pFuncList;
    if pCurDqLayer.is_null() || pFuncList.is_null() {
        return;
    }

    let pCoeffLevel = (*pMbCache).pCoeffLevel;
    let pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let iEncStride = (*pCurDqLayer).iEncStride[0];
    let pMemPredLuma = (*pMbCache).pMemPredLuma;

    if !pCoeffLevel.is_null() && !pEncMb.is_null() && !pMemPredLuma.is_null() {
        if let Some(dct_fn) = (*pFuncList).pfDctFourT4 {
            dct_fn(pCoeffLevel, pEncMb, iEncStride, pMemPredLuma, 16);
            dct_fn(pCoeffLevel.add(64), pEncMb.add(8), iEncStride, pMemPredLuma.add(8), 16);
            dct_fn(pCoeffLevel.add(128), pEncMb.add((8 * iEncStride) as usize), iEncStride, pMemPredLuma.add(128), 16);
            dct_fn(pCoeffLevel.add(192), pEncMb.add((8 * iEncStride + 8) as usize), iEncStride, pMemPredLuma.add(136), 16);
        }
    }

    WelsEncInterY(
        pFuncList as *mut crate::encoder::svc_encode_mb::SWelsFuncPtrList,
        pCurMb as *mut crate::encoder::svc_encode_mb::SMB,
        pMbCache as *mut crate::encoder::svc_encode_mb::SMbCache,
    );
}

// ============================================================================
// 1. Spatial Enhancement Layer Mode Decision (ILFMD / NoILP)
// ============================================================================

/// Retrieves the collocated base-layer reference macroblock in dyadic SVC downsampling.
#[inline(always)]
pub unsafe extern "C" fn GetRefMb(pCurLayer: *mut SDqLayer, pCurMb: *mut SMB) -> *mut SMB {
    let kpRefLayer = (*pCurLayer).pRefLayer;
    let kiRefMbIdx =
        (((*pCurMb).iMbY as i32 >> 1) * (*kpRefLayer).iMbWidth) + ((*pCurMb).iMbX as i32 >> 1);
    (*kpRefLayer).sMbDataP.offset(kiRefMbIdx as isize)
}

/// Scales base-layer motion vectors by 2x to initialize enhancement-layer candidates.
pub unsafe extern "C" fn SetMvBaseEnhancelayer(
    pMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    kpRefMb: *const SMB,
) {
    let kuiRefMbType = (*kpRefMb).uiMbType;

    if !IS_SVC_INTRA(kuiRefMbType) {
        let iRefMbPartIdx =
            ((((*pCurMb).iMbY as i32 & 0x01) << 1) + ((*pCurMb).iMbX as i32 & 0x01)) as usize;
        let iScan4RefPartIdx = g_kuiMbCountScan4Idx[iRefMbPartIdx << 2] as isize;

        let ref_mv = *(*kpRefMb).sMv.offset(iScan4RefPartIdx);
        let sMv = SMVUnitXY {
            iMvX: ref_mv.iMvX * 2,
            iMvY: ref_mv.iMvY * 2,
        };

        (*pMd).sMe.sMe16x16.sMvBase = sMv;
        (*pMd).sMe.sMe8x8[0].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[1].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[2].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[3].sMvBase = sMv;

        (*pMd).sMe.sMe16x8[0].sMvBase = sMv;
        (*pMd).sMe.sMe16x8[1].sMvBase = sMv;
        (*pMd).sMe.sMe8x16[0].sMvBase = sMv;
        (*pMd).sMe.sMe8x16[1].sMvBase = sMv;
    }
}

/// Core spatial enhancement layer mode decision without Inter-Layer Prediction.
pub unsafe extern "C" fn WelsMdSpatialelInterMbIlfmdNoilp(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    kuiRefMbType: Mb_Type,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;

    let kuiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let kiMbWidth = (*pCurDqLayer).iMbWidth;
    let kpTopMb = pCurMb.offset(-(kiMbWidth as isize));

    let kbMbLeftAvailPskip = if (kuiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        IS_SKIP((*pCurMb.offset(-1)).uiMbType)
    } else {
        false
    };
    let kbMbTopAvailPskip = if (kuiNeighborAvail & TOP_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb).uiMbType)
    } else {
        false
    };
    let kbMbTopLeftAvailPskip = if (kuiNeighborAvail & TOPLEFT_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb.offset(-1)).uiMbType)
    } else {
        false
    };
    let kbMbTopRightAvailPskip = if (kuiNeighborAvail & TOPRIGHT_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb.offset(1)).uiMbType)
    } else {
        false
    };

    let bTrySkip =
        kbMbLeftAvailPskip | kbMbTopAvailPskip | kbMbTopLeftAvailPskip | kbMbTopRightAvailPskip;
    let mut bKeepSkip = kbMbLeftAvailPskip & kbMbTopAvailPskip & kbMbTopRightAvailPskip;
    let bSkip: bool;

    if let Some(pfBgd) = (*(*pEncCtx).pFuncList).pfInterMdBackgroundDecision {
        if pfBgd(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache, &mut bKeepSkip) {
            return;
        }
    }

    // Step 1: Try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache, bTrySkip);

    if bSkip && bKeepSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb, pMbCache);
        return;
    }

    if !IS_SVC_INTRA(kuiRefMbType) {
        if !bSkip {
            PredictSad(
                (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
                (*pMbCache).iSadCost,
                0,
                &mut (*pWelsMd).iSadPredMb,
            );

            // Step 2: P_16x16
            (*pWelsMd).iCostLuma =
                WelsMdP16x16((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice, pCurMb);
            (*pCurMb).uiMbType = MB_TYPE_16x16;
        }

        WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache, bSkip);
    } else {
        // Base layer is Intra (BLMODE == SVC_INTRA)
        let kiCostI16x16 = WelsMdI16x16(
            (*pEncCtx).pFuncList,
            (*pEncCtx).pCurDqLayer,
            pMbCache,
            (*pWelsMd).iLambda,
        );
        if bSkip && ((*pWelsMd).iCostLuma <= kiCostI16x16) {
            WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb, pMbCache);
        } else {
            (*pWelsMd).iCostLuma = kiCostI16x16;
            (*pCurMb).uiMbType = MB_TYPE_INTRA16x16;

            WelsMdIntraSecondaryModesEnc(pEncCtx, pWelsMd, pCurMb, pMbCache);
        }
    }
}

/// Top-level MD entry point for spatial enhancement layer inter MBs.
pub unsafe extern "C" fn WelsMdInterMbEnhancelayer(
    pEncCtx: *mut sWelsEncCtx,
    pMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
) {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let kpInterLayerRefMb = GetRefMb(pCurLayer, pCurMb);
    let kuiInterLayerRefMbType = (*kpInterLayerRefMb).uiMbType;

    SetMvBaseEnhancelayer(pMd, pCurMb, kpInterLayerRefMb);
    WelsMdSpatialelInterMbIlfmdNoilp(pEncCtx, pMd, pSlice, pCurMb, kuiInterLayerRefMbType);
}

// ============================================================================
// 2. Background Detection (BGD) P-Skip Mode Decision & Chroma Verification
// ============================================================================

#[inline(always)]
pub unsafe fn GetChromaCost(
    pCalculateFunc: *const Option<PSampleSadSatdCostFunc>,
    pSrcChroma: *const u8,
    iSrcStride: i32,
    pRefChroma: *const u8,
    iRefStride: i32,
) -> i32 {
    let func = *pCalculateFunc.add(BLOCK_8x8);
    if let Some(f) = func {
        f(pSrcChroma, iSrcStride, pRefChroma, iRefStride)
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn IsCostLessEqualSkipCost(
    iCurCost: i32,
    iPredPskipSad: i32,
    iRefMbType: Mb_Type,
    pRef: *const SPicture,
    iMbXy: i32,
    iSmallestInvisibleTh: i32,
) -> bool {
    (iPredPskipSad > iSmallestInvisibleTh && iCurCost >= iPredPskipSad)
        || ((*pRef).iPictureType == P_SLICE
            && iRefMbType == MB_TYPE_SKIP
            && *(*pRef).pMbSkipSad.offset(iMbXy as isize) > iSmallestInvisibleTh
            && iCurCost >= *(*pRef).pMbSkipSad.offset(iMbXy as isize))
}

pub unsafe fn CheckChromaCost(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pMbCache: *mut SMbCache,
    iCurMbXy: i32,
) -> bool {
    let pSad = (*(*pEncCtx).pFuncList).sSampleDealingFuncs.pfSampleSad.as_ptr();
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;

    let pCbEnc = (*pMbCache).SPicData.pEncMb[1];
    let pCrEnc = (*pMbCache).SPicData.pEncMb[2];
    let pCbRef = (*pMbCache).SPicData.pRefMb[1];
    let pCrRef = (*pMbCache).SPicData.pRefMb[2];

    let iCbEncStride = (*pCurDqLayer).iEncStride[1];
    let iCrEncStride = (*pCurDqLayer).iEncStride[2];
    let iChromaRefStride = (*(*pCurDqLayer).pRefPic).iLineSize[1];

    let iCbSad = GetChromaCost(pSad, pCbEnc, iCbEncStride, pCbRef, iChromaRefStride);
    let iCrSad = GetChromaCost(pSad, pCrEnc, iCrEncStride, pCrRef, iChromaRefStride);

    let bChromaTooLarge = iCbSad > KNOWN_CHROMA_TOO_LARGE || iCrSad > KNOWN_CHROMA_TOO_LARGE;
    let iChromaSad = iCbSad + iCrSad;

    PredictSadSkip(
        (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
        (*pMbCache).bMbTypeSkip,
        (*pMbCache).iSadCostSkip,
        0,
        &mut (*pWelsMd).iSadPredSkip,
    );

    let bChromaCostCannotSkip = IsCostLessEqualSkipCost(
        iChromaSad,
        (*pWelsMd).iSadPredSkip,
        (*pMbCache).uiRefMbType,
        (*pCurDqLayer).pRefPic,
        iCurMbXy,
        SMALLEST_INVISIBLE,
    );

    !bChromaCostCannotSkip && !bChromaTooLarge
}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    bKeepSkip: *mut bool,
) -> bool {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;

    let kiRefMbQp = *(*(*pCurDqLayer).pRefPic).pRefMbQp.offset((*pCurMb).iMbXY as isize) as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;
    let pVaaBgMbFlag = (*(*pEncCtx).pVaa).pVaaBackgroundMbFlag.offset((*pCurMb).iMbXY as isize);

    let kiMbWidth = (*pCurDqLayer).iMbWidth as isize;

    *bKeepSkip = *bKeepSkip
        && (*pVaaBgMbFlag.offset(-1) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth + 1) == 0);

    if *pVaaBgMbFlag != 0
        && !IS_INTRA((*pMbCache).uiRefMbType)
        && ((kiRefMbQp - kiCurMbQp <= DELTA_QP_BGD_THD) || (kiRefMbQp <= 26))
    {
        if CheckChromaCost(pEncCtx, pWelsMd, pMbCache, (*pCurMb).iMbXY) {
            let mut sVaaPredSkipMv = SMVUnitXY::default();
            PredSkipMv(pMbCache, &mut sVaaPredSkipMv);
            let bZeroMv = sVaaPredSkipMv.iMvX == 0 && sVaaPredSkipMv.iMvY == 0;
            WelsMdBackgroundMbEnc(pEncCtx, pWelsMd, pCurMb, pMbCache, pSlice, bZeroMv);
            return true;
        }
    }

    false
}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskipFalse(
    _pCtx: *mut sWelsEncCtx,
    _pMd: *mut SWelsMD,
    _pSlice: *mut SSlice,
    _pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
    _bKeepSkip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfo(
    pCurLayer: *mut SDqLayer,
    pCurMb: *mut SMB,
    bCollocatedPredFlag: bool,
    iRefPictureType: i32,
) {
    let pTargetRefMbQpList = (*(*pCurLayer).pDecPic).pRefMbQp;
    let kiMbXY = (*pCurMb).iMbXY as isize;

    if (*pCurMb).uiCbp != 0 || iRefPictureType == I_SLICE || !bCollocatedPredFlag {
        *pTargetRefMbQpList.offset(kiMbXY) = (*pCurMb).uiLumaQp;
    } else {
        let pRefPicRefMbQpList = (*(*pCurLayer).pRefPic).pRefMbQp;
        *pTargetRefMbQpList.offset(kiMbXY) = *pRefPicRefMbQpList.offset(kiMbXY);
    }

    if (*pCurMb).uiMbType == MB_TYPE_BACKGROUND {
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
    }
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfoNULL(
    pCurLayer: *mut SDqLayer,
    pCurMb: *mut SMB,
    bCollocatedPredFlag: bool,
    iRefPictureType: i32,
) {
    WelsMdUpdateBGDInfo(pCurLayer, pCurMb, bCollocatedPredFlag, iRefPictureType);
}

// ============================================================================
// 3. Screen Content Coding (SCC) & Scene Change Detection (SCD) P-Skip
// ============================================================================

#[inline(always)]
pub unsafe fn IsMbStatic(pBlockType: *const i32, eType: EStaticBlockIdc) -> bool {
    if pBlockType.is_null() {
        return false;
    }
    let target = eType as i32;
    *pBlockType == target
        && *pBlockType.add(1) == target
        && *pBlockType.add(2) == target
        && *pBlockType.add(3) == target
}

#[inline(always)]
pub unsafe fn IsMbCollocatedStatic(pBlockType: *const i32) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::COLLOCATED_STATIC)
}

#[inline(always)]
pub unsafe fn IsMbScrolledStatic(pBlockType: *const i32) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::SCROLLED_STATIC)
}

#[inline(always)]
pub unsafe fn CalUVSadCost(
    pFunc: *mut SWelsFuncPtrList,
    pEncOri: *mut u8,
    iStrideUV: i32,
    pRefOri: *mut u8,
    iRefLineSize: i32,
) -> i32 {
    let f = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_8x8];
    if let Some(sad_func) = f {
        sad_func(pEncOri, iStrideUV, pRefOri, iRefLineSize)
    } else {
        0
    }
}

#[inline(always)]
pub fn CheckBorder(
    iMbX: i32,
    iMbY: i32,
    iScrollMvX: i32,
    iScrollMvY: i32,
    iMbWidth: i32,
    iMbHeight: i32,
) -> bool {
    (iMbX << 4) + iScrollMvX < 0
        || (iMbX << 4) + iScrollMvX > ((iMbWidth - 1) << 4)
        || (iMbY << 4) + iScrollMvY < 0
        || (iMbY << 4) + iScrollMvY > ((iMbHeight - 1) << 4)
}

pub unsafe extern "C" fn JudgeStaticSkip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: *mut SWelsMD,
) -> bool {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;

    let mut bTryStaticSkip = IsMbCollocatedStatic((*pWelsMd).iBlock8x8StaticIdc.as_ptr());
    if bTryStaticSkip {
        let pFunc = (*pEncCtx).pFuncList;
        let pRefOri = (*pCurDqLayer).pRefOri[0];
        if !pRefOri.is_null() {
            let iStrideUV = (*pCurDqLayer).iEncStride[1];
            let iOffsetUV = (kiMbX + kiMbY * iStrideUV) << 3;

            let iSadCostCb = CalUVSadCost(
                pFunc,
                (*pMbCache).SPicData.pEncMb[1],
                iStrideUV,
                (*pRefOri).pData[1].offset(iOffsetUV as isize),
                (*pRefOri).iLineSize[1],
            );
            if iSadCostCb == 0 {
                let iSadCostCr = CalUVSadCost(
                    pFunc,
                    (*pMbCache).SPicData.pEncMb[2],
                    iStrideUV,
                    (*pRefOri).pData[2].offset(iOffsetUV as isize),
                    (*pRefOri).iLineSize[1],
                );
                bTryStaticSkip = iSadCostCr == 0;
            } else {
                bTryStaticSkip = false;
            }
        } else {
            bTryStaticSkip = false;
        }
    }
    bTryStaticSkip
}

pub unsafe extern "C" fn JudgeScrollSkip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: *mut SWelsMD,
) -> bool {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbWidth = (*pCurDqLayer).iMbWidth;
    let kiMbHeight = (*pCurDqLayer).iMbHeight;
    let pVaaExt = (*pEncCtx).pVaa as *mut SVAAFrameInfoExt_t;

    let mut bTryScrollSkip;
    if (*pVaaExt).sScrollDetectInfo.bScrollDetectFlag {
        bTryScrollSkip = IsMbScrolledStatic((*pWelsMd).iBlock8x8StaticIdc.as_ptr());
    } else {
        return false;
    }

    if bTryScrollSkip {
        let pFunc = (*pEncCtx).pFuncList;
        let pRefOri = (*pCurDqLayer).pRefOri[0];
        if !pRefOri.is_null() {
            let iScrollMvX = (*pVaaExt).sScrollDetectInfo.iScrollMvX;
            let iScrollMvY = (*pVaaExt).sScrollDetectInfo.iScrollMvY;
            if CheckBorder(kiMbX, kiMbY, iScrollMvX, iScrollMvY, kiMbWidth, kiMbHeight) {
                bTryScrollSkip = false;
            } else {
                let iStrideUV = (*pCurDqLayer).iEncStride[1];
                let iOffsetUV = (kiMbX << 3)
                    + (iScrollMvX >> 1)
                    + (((kiMbY << 3) + (iScrollMvY >> 1)) * iStrideUV);

                let iSadCostCb = CalUVSadCost(
                    pFunc,
                    (*pMbCache).SPicData.pEncMb[1],
                    iStrideUV,
                    (*pRefOri).pData[1].offset(iOffsetUV as isize),
                    (*pRefOri).iLineSize[1],
                );
                if iSadCostCb == 0 {
                    let iSadCostCr = CalUVSadCost(
                        pFunc,
                        (*pMbCache).SPicData.pEncMb[2],
                        iStrideUV,
                        (*pRefOri).pData[2].offset(iOffsetUV as isize),
                        (*pRefOri).iLineSize[1],
                    );
                    bTryScrollSkip = iSadCostCr == 0;
                } else {
                    bTryScrollSkip = false;
                }
            }
        }
    }
    bTryScrollSkip
}

pub unsafe extern "C" fn SvcMdSCDMbEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    pSlice: *mut SSlice,
    bQpSimilarFlag: bool,
    bMbSkipFlag: bool,
    sCurMbMv: *mut SMVUnitXY,
    eSkipMode: ESkipModes,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let pFunc = (*pEncCtx).pFuncList;
    let skip_idx = eSkipMode as usize;
    let sCandidateMv = *sCurMbMv.add(skip_idx);

    let sMvp = SMVUnitXY {
        iMvX: sCandidateMv.iMvX,
        iMvY: sCandidateMv.iMvY,
    };

    let pRefLuma = (*pMbCache).SPicData.pRefMb[0];
    let pRefCb = (*pMbCache).SPicData.pRefMb[1];
    let pRefCr = (*pMbCache).SPicData.pRefMb[2];
    let iLineSizeY = (*(*pCurDqLayer).pRefPic).iLineSize[0];
    let iLineSizeUV = (*(*pCurDqLayer).pRefPic).iLineSize[1];

    let mut pDstLuma = (*pMbCache).pSkipMb;
    let mut pDstCb = (*pMbCache).pSkipMb.add(256);
    let mut pDstCr = (*pMbCache).pSkipMb.add(256 + 64);

    let iOffsetY = (sCandidateMv.iMvX as i32 >> 2) + (sCandidateMv.iMvY as i32 >> 2) * iLineSizeY;
    let iOffsetUV = (sCandidateMv.iMvX as i32 >> 3) + (sCandidateMv.iMvY as i32 >> 3) * iLineSizeUV;

    if !bQpSimilarFlag || !bMbSkipFlag {
        pDstLuma = (*pMbCache).pMemPredLuma;
        pDstCb = (*pMbCache).pMemPredChroma;
        pDstCr = (*pMbCache).pMemPredChroma.add(64);
    }

    // Motion Compensation
    if let Some(pMcLuma) = (*pFunc).sMcFuncs.pMcLumaFunc {
        pMcLuma(
            pRefLuma.offset(iOffsetY as isize),
            iLineSizeY,
            pDstLuma,
            16,
            0,
            0,
            16,
            16,
        );
    }
    if let Some(pMcChroma) = (*pFunc).sMcFuncs.pMcChromaFunc {
        pMcChroma(
            pRefCb.offset(iOffsetUV as isize),
            iLineSizeUV,
            pDstCb,
            8,
            sMvp.iMvX,
            sMvp.iMvY,
            8,
            8,
        );
        pMcChroma(
            pRefCr.offset(iOffsetUV as isize),
            iLineSizeUV,
            pDstCr,
            8,
            sMvp.iMvX,
            sMvp.iMvY,
            8,
            8,
        );
    }

    (*pCurMb).uiCbp = 0;
    (*pWelsMd).iCostLuma = 0;

    let sad_16x16 = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].unwrap();
    let sad_cost = sad_16x16(
        (*pMbCache).SPicData.pEncMb[0],
        (*pCurDqLayer).iEncStride[0],
        pRefLuma.offset(iOffsetY as isize),
        iLineSizeY,
    );
    *(*pCurMb).pSadCost.offset(0) = sad_cost;
    (*pWelsMd).iCostSkipMb = sad_cost;

    (*pCurMb).sP16x16Mv = sCandidateMv;
    *(*(*pCurDqLayer).pDecPic).sMvList.offset((*pCurMb).iMbXY as isize) = sCandidateMv;

    if bQpSimilarFlag && bMbSkipFlag {
        std::ptr::write_bytes((*pCurMb).pRefIndex, 0, 4);
        if let Some(pfUpdateMbMv) = (*pFunc).pfUpdateMbMv {
            pfUpdateMbMv((*pCurMb).sMv, sMvp);
        }
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
        WelsRecPskip(pCurDqLayer, pFunc, pCurMb, pMbCache);
        WelsMdInterUpdatePskip(pCurDqLayer, pSlice, pCurMb, pMbCache);
        return;
    }

    (*pCurMb).uiMbType = MB_TYPE_16x16;

    (*pWelsMd).sMe.sMe16x16.sMv = sCandidateMv;
    PredMv(
        &(*pMbCache).sMvComponents,
        0,
        4,
        0,
        &mut (*pWelsMd).sMe.sMe16x16.sMvp,
    );
    (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;

    UpdateP16x16MotionInfo(pMbCache, pCurMb, 0, &mut (*pWelsMd).sMe.sMe16x16.sMv);

    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = *(*pCurMb).pSadCost.offset(0);
    } else {
        (*pWelsMd).iCostLuma = sad_16x16(
            (*pMbCache).SPicData.pEncMb[0],
            (*pCurDqLayer).iEncStride[0],
            pRefLuma,
            iLineSizeY,
        );
    }

    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(
        pEncCtx as *mut crate::encoder::svc_encode_slice::sWelsEncCtx,
        pSlice as *mut crate::encoder::svc_encode_slice::SSlice,
        pCurMb as *mut crate::encoder::svc_encode_slice::SMB,
    );

    if let Some(copy16) = (*pFunc).pfCopy16x16Aligned {
        copy16(
            (*pMbCache).SPicData.pCsMb[0],
            (*pCurDqLayer).iCsStride[0],
            (*pMbCache).pMemPredLuma,
            16,
        );
    }
    if let Some(copy8) = (*pFunc).pfCopy8x8Aligned {
        copy8(
            (*pMbCache).SPicData.pCsMb[1],
            (*pCurDqLayer).iCsStride[1],
            (*pMbCache).pMemPredChroma,
            8,
        );
        copy8(
            (*pMbCache).SPicData.pCsMb[2],
            (*pCurDqLayer).iCsStride[1],
            (*pMbCache).pMemPredChroma.add(64),
            8,
        );
    }
}

pub unsafe extern "C" fn MdInterSCDPskipProcess(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    eSkipMode: ESkipModes,
) -> bool {
    let pVaaExt = (*pEncCtx).pVaa as *mut SVAAFrameInfoExt_t;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;

    let kiRefMbQp = *(*(*pCurDqLayer).pRefPic).pRefMbQp.offset((*pCurMb).iMbXY as isize) as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;

    let pJudgeSkip: [pJudgeSkipFun; 2] = [JudgeStaticSkip, JudgeScrollSkip];
    let bSkipFlag = pJudgeSkip[eSkipMode as usize](pEncCtx, pCurMb, pMbCache, pWelsMd);

    if bSkipFlag {
        let bQpSimilarFlag = (kiRefMbQp - kiCurMbQp <= DELTA_QP_SCD_THD) || (kiRefMbQp <= 26);
        let mut sVaaPredSkipMv = SMVUnitXY::default();
        let mut sCurMbMv: [SMVUnitXY; 2] = [SMVUnitXY::default(), SMVUnitXY::default()];
        PredSkipMv(pMbCache, &mut sVaaPredSkipMv);

        if eSkipMode == ESkipModes::SCROLLED {
            sCurMbMv[1].iMvX = (WELS_CLIP3(
                (*pVaaExt).sScrollDetectInfo.iScrollMvX,
                -(*pEncCtx).iMvRange,
                (*pEncCtx).iMvRange,
            ) << 2) as i16;
            sCurMbMv[1].iMvY = (WELS_CLIP3(
                (*pVaaExt).sScrollDetectInfo.iScrollMvY,
                -(*pEncCtx).iMvRange,
                (*pEncCtx).iMvRange,
            ) << 2) as i16;
        }

        let bMbSkipFlag = sVaaPredSkipMv == sCurMbMv[eSkipMode as usize];
        SvcMdSCDMbEnc(
            pEncCtx,
            pWelsMd,
            pCurMb,
            pMbCache,
            pSlice,
            bQpSimilarFlag,
            bMbSkipFlag,
            sCurMbMv.as_mut_ptr(),
            eSkipMode,
        );
        return true;
    }

    false
}

pub unsafe extern "C" fn SetBlockStaticIdcToMd(
    pVaa: *mut c_void,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pDqLayer: *mut SDqLayer,
) {
    let pVaaExt = pVaa as *mut SVAAFrameInfoExt_t;

    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbWidth = (*pDqLayer).iMbWidth;
    let kiWidth = kiMbWidth << 1;

    let kiBlockIndexUp = (kiMbY << 1) * kiWidth + (kiMbX << 1);
    let kiBlockIndexLow = ((kiMbY << 1) + 1) * kiWidth + (kiMbX << 1);

    (*pWelsMd).iBlock8x8StaticIdc[0] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset(kiBlockIndexUp as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[1] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset((kiBlockIndexUp + 1) as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[2] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset(kiBlockIndexLow as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[3] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset((kiBlockIndexLow + 1) as isize) as i32;
}

pub unsafe extern "C" fn WelsMdInterJudgeSCDPskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    SetBlockStaticIdcToMd((*pEncCtx).pVaa as *mut c_void, pWelsMd, pCurMb, pCurDqLayer);

    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, pMbCache, ESkipModes::STATIC) {
        return true;
    }
    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, pMbCache, ESkipModes::SCROLLED) {
        return true;
    }

    false
}

pub unsafe extern "C" fn WelsMdInterJudgeSCDPskipFalse(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut SWelsMD,
    _slice: *mut SSlice,
    _pCurMb: *mut SMB,
    _pMbCache: *mut SMbCache,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsInitSCDPskipFunc(
    pFuncList: *mut SWelsFuncPtrList,
    bScrollingDetection: bool,
) {
    if bScrollingDetection {
        (*pFuncList).pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskip);
    } else {
        (*pFuncList).pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskipFalse);
    }
}

// ============================================================================
// 4. Sub-Macroblock Fine Partitioning & Mode Merging
// ============================================================================

#[inline(always)]
pub unsafe fn MergeSub16Me(sSrcMe0: *const SWelsME, sSrcMe1: *const SWelsME, pTarMe: *mut SWelsME) {
    std::ptr::copy_nonoverlapping(sSrcMe0, pTarMe, 1);
    (*pTarMe).uiSadCost = (*sSrcMe0).uiSadCost + (*sSrcMe1).uiSadCost;
    (*pTarMe).uiSatdCost = (*sSrcMe0).uiSatdCost + (*sSrcMe1).uiSatdCost;
}

#[inline(always)]
pub fn IsSameMv(sMv0: &SMVUnitXY, sMv1: &SMVUnitXY) -> bool {
    sMv0.iMvX == sMv1.iMvX && sMv0.iMvY == sMv1.iMvY
}

pub unsafe fn TryModeMerge(
    pMbCache: *mut SMbCache,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
) -> bool {
    let pMe8x8 = (*pWelsMd).sMe.sMe8x8.as_ptr();

    let bSameMv16x8_0 = IsSameMv(&(*pMe8x8.add(0)).sMv, &(*pMe8x8.add(1)).sMv);
    let bSameMv16x8_1 = IsSameMv(&(*pMe8x8.add(2)).sMv, &(*pMe8x8.add(3)).sMv);

    let bSameMv8x16_0 = IsSameMv(&(*pMe8x8.add(0)).sMv, &(*pMe8x8.add(2)).sMv);
    let bSameMv8x16_1 = IsSameMv(&(*pMe8x8.add(1)).sMv, &(*pMe8x8.add(3)).sMv);

    let bSameRefIdx16x8_0 = true;
    let bSameRefIdx16x8_1 = true;
    let bSameRefIdx8x16_0 = true;
    let bSameRefIdx8x16_1 = true;

    let iSameMv = (((bSameMv16x8_0 && bSameRefIdx16x8_0 && bSameMv16x8_1 && bSameRefIdx16x8_1) as i32)
        << 1)
        | ((bSameMv8x16_0 && bSameRefIdx8x16_0 && bSameMv8x16_1 && bSameRefIdx8x16_1) as i32);

    match iSameMv {
        2 => {
            (*pCurMb).uiMbType = MB_TYPE_16x8;
            MergeSub16Me(pMe8x8.add(0), pMe8x8.add(1), &mut (*pWelsMd).sMe.sMe16x8[0]);
            MergeSub16Me(pMe8x8.add(2), pMe8x8.add(3), &mut (*pWelsMd).sMe.sMe16x8[1]);
            PredInter16x8Mv(pMbCache, 0, 0, &mut (*pWelsMd).sMe.sMe16x8[0].sMvp);
            PredInter16x8Mv(pMbCache, 8, 0, &mut (*pWelsMd).sMe.sMe16x8[1].sMvp);
        }
        1 => {
            (*pCurMb).uiMbType = MB_TYPE_8x16;
            MergeSub16Me(pMe8x8.add(0), pMe8x8.add(2), &mut (*pWelsMd).sMe.sMe8x16[0]);
            MergeSub16Me(pMe8x8.add(1), pMe8x8.add(3), &mut (*pWelsMd).sMe.sMe8x16[1]);
            PredInter8x16Mv(pMbCache, 0, 0, &mut (*pWelsMd).sMe.sMe8x16[0].sMvp);
            PredInter8x16Mv(pMbCache, 4, 0, &mut (*pWelsMd).sMe.sMe8x16[1].sMvp);
        }
        _ => {}
    }

    (*pCurMb).uiMbType != MB_TYPE_8x8
}

pub unsafe extern "C" fn WelsMdInterFinePartitionVaaOnScreen(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    mut iBestCost: i32,
) {
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;

    let pSad8x8_ptr =
        (*(*(*pEncCtx).pVaa).sVaaCalcInfo.pSad8x8.offset((*pCurMb).iMbXY as isize)).as_ptr();
    let get_sign = (*(*pEncCtx).pFuncList).pfGetMbSignFromInterVaa.unwrap();
    let uiMbSign = get_sign(pSad8x8_ptr);

    if uiMbSign == MBVAASIGN_FLAT {
        return;
    }

    let iCostP8x8 = WelsMdP8x8((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
    if iCostP8x8 < iBestCost {
        iBestCost = iCostP8x8;
        (*pCurMb).uiMbType = MB_TYPE_8x8;
        (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];
        TryModeMerge(pMbCache, pWelsMd, pCurMb);
    }
    (*pWelsMd).iCostLuma = iBestCost;
}

// ============================================================================
// 5. Global Scrolling Motion Vector Dispatch
// ============================================================================

pub unsafe extern "C" fn SetScrollingMvToMd(pVaa: *mut SVAAFrameInfo, pWelsMd: *mut SWelsMD) {
    let pVaaExt = pVaa as *mut SVAAFrameInfoExt_t;
    let sTempMv = SMVUnitXY {
        iMvX: (*pVaaExt).sScrollDetectInfo.iScrollMvX as i16,
        iMvY: (*pVaaExt).sScrollDetectInfo.iScrollMvY as i16,
    };

    (*pWelsMd).sMe.sMe16x16.sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[0].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[1].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[2].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[3].sDirectionalMv = sTempMv;
}

/// Intentional no-op mode decision scrolling MV callback.
/// Matches `void SetScrollingMvToMdNull (SVAAFrameInfo* pVaa, SWelsMD* pWelsMd)` in `svc_mode_decision.cpp:689`.
pub unsafe extern "C" fn SetScrollingMvToMdNull(_pVaa: *mut SVAAFrameInfo, _pWelsMd: *mut SWelsMD) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pred_mv_basic_median() {
        unsafe {
            let mut mv_comp = SMVComponentUnit::default();
            // Cache index 6 is Left (kuiLeftIdx), 1 is Top (kuiTopIdx), 5 is RightTop (kuiTopIdx + 4)
            mv_comp.iRefIndexCache[6] = 0;
            mv_comp.iRefIndexCache[1] = 0;
            mv_comp.iRefIndexCache[5] = 0;

            mv_comp.sMotionVectorCache[6] = SMVUnitXY { iMvX: 10, iMvY: 20 };
            mv_comp.sMotionVectorCache[1] = SMVUnitXY { iMvX: 30, iMvY: 40 };
            mv_comp.sMotionVectorCache[5] = SMVUnitXY { iMvX: 20, iMvY: 30 };

            let mut sMvp = SMVUnitXY::default();
            PredMv(&mv_comp as *const SMVComponentUnit, 0, 4, 0, &mut sMvp as *mut SMVUnitXY);

            // Median of (10, 30, 20) is 20; Median of (20, 40, 30) is 30
            assert_eq!(sMvp.iMvX, 20);
            assert_eq!(sMvp.iMvY, 30);
        }
    }

    #[test]
    fn test_pred_skip_mv_zero_ref() {
        unsafe {
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                bMbTypeSkip: std::ptr::null_mut(),
                iSadCost: std::ptr::null_mut(),
                iSadCostSkip: std::ptr::null_mut(),
                sMvComponents: SMVComponentUnit::default(),
                SPicData: SSampleDealingPicData::default(),
                pSkipMb: std::ptr::null_mut(),
                pMemPredLuma: std::ptr::null_mut(),
                pMemPredChroma: std::ptr::null_mut(),
                sMbMvp: [SMVUnitXY::default(); 16],
                pCoeffLevel: std::ptr::null_mut(),
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
            };

            // When left & top ref MVs are (0,0) and ref=0, PredSkipMv returns (0,0)
            mb_cache.sMvComponents.iRefIndexCache[6] = 0;
            mb_cache.sMvComponents.iRefIndexCache[1] = 0;
            mb_cache.sMvComponents.sMotionVectorCache[6] = SMVUnitXY { iMvX: 0, iMvY: 0 };
            mb_cache.sMvComponents.sMotionVectorCache[1] = SMVUnitXY { iMvX: 0, iMvY: 0 };

            let mut sMvp = SMVUnitXY { iMvX: 99, iMvY: 99 };
            PredSkipMv(&mut mb_cache as *mut SMbCache, &mut sMvp as *mut SMVUnitXY);

            assert_eq!(sMvp.iMvX, 0);
            assert_eq!(sMvp.iMvY, 0);
        }
    }

    #[test]
    fn test_pred_inter_16x8_8x16_mv() {
        unsafe {
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                bMbTypeSkip: std::ptr::null_mut(),
                iSadCost: std::ptr::null_mut(),
                iSadCostSkip: std::ptr::null_mut(),
                sMvComponents: SMVComponentUnit::default(),
                SPicData: SSampleDealingPicData::default(),
                pSkipMb: std::ptr::null_mut(),
                pMemPredLuma: std::ptr::null_mut(),
                pMemPredChroma: std::ptr::null_mut(),
                sMbMvp: [SMVUnitXY::default(); 16],
                pCoeffLevel: std::ptr::null_mut(),
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
            };

            mb_cache.sMvComponents.iRefIndexCache[1] = 0; // Top ref for 16x8 part 0
            mb_cache.sMvComponents.sMotionVectorCache[1] = SMVUnitXY { iMvX: 12, iMvY: 34 };

            let mut sMvp = SMVUnitXY::default();
            PredInter16x8Mv(&mut mb_cache as *mut SMbCache, 0, 0, &mut sMvp as *mut SMVUnitXY);
            assert_eq!(sMvp.iMvX, 12);
            assert_eq!(sMvp.iMvY, 34);

            mb_cache.sMvComponents.iRefIndexCache[6] = 0; // Left ref for 8x16 part 0
            mb_cache.sMvComponents.sMotionVectorCache[6] = SMVUnitXY { iMvX: 56, iMvY: 78 };

            let mut sMvp8x16 = SMVUnitXY::default();
            PredInter8x16Mv(&mut mb_cache as *mut SMbCache, 0, 0, &mut sMvp8x16 as *mut SMVUnitXY);
            assert_eq!(sMvp8x16.iMvX, 56);
            assert_eq!(sMvp8x16.iMvY, 78);
        }
    }

    #[test]
    fn test_update_p16x16_motion_info() {
        unsafe {
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                bMbTypeSkip: std::ptr::null_mut(),
                iSadCost: std::ptr::null_mut(),
                iSadCostSkip: std::ptr::null_mut(),
                sMvComponents: SMVComponentUnit::default(),
                SPicData: SSampleDealingPicData::default(),
                pSkipMb: std::ptr::null_mut(),
                pMemPredLuma: std::ptr::null_mut(),
                pMemPredChroma: std::ptr::null_mut(),
                sMbMvp: [SMVUnitXY::default(); 16],
                pCoeffLevel: std::ptr::null_mut(),
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
            };

            let mut mv_arr = [SMVUnitXY::default(); 16];
            let mut ref_arr = [0u8; 4];

            let mut cur_mb = SMB {
                uiMbType: MB_TYPE_16x16,
                uiSubMbType: [0; 4],
                iMbXY: 0,
                iMbX: 0,
                iMbY: 0,
                uiNeighborAvail: 0,
                uiCbp: 0,
                sMv: mv_arr.as_mut_ptr(),
                pRefIndex: ref_arr.as_mut_ptr() as *mut i8,
                pSadCost: std::ptr::null_mut(),
                pIntra4x4PredMode: std::ptr::null_mut(),
                pNonZeroCount: std::ptr::null_mut(),
                sP16x16Mv: SMVUnitXY::default(),
                uiLumaQp: 26,
                uiChromaQp: 26,
                uiSliceIdc: 0,
                uiChromPredMode: 0,
                iLumaDQp: 0,
                sMvd: [SMVUnitXY::default(); 16],
                iCbpDc: 0,
            };

            let mut target_mv = SMVUnitXY { iMvX: 42, iMvY: -15 };
            UpdateP16x16MotionInfo(
                &mut mb_cache as *mut SMbCache,
                &mut cur_mb as *mut SMB,
                0,
                &mut target_mv as *mut SMVUnitXY,
            );

            assert_eq!(mv_arr[0], target_mv);
            assert_eq!(ref_arr[0], 0);
            assert_eq!(
                mb_cache.sMvComponents.sMotionVectorCache[g_kuiCache30ScanIdx[0] as usize],
                target_mv
            );
        }
    }

    #[test]
    fn test_wels_md_i16x16_cost() {
        unsafe {
            let mut func_list = SWelsFuncPtrList {
                sSampleDealingFuncs: SSampleDealingFuncs {
                    pfSampleSad: [None, None, None, None, None, None, None],
                },
                sMcFuncs: SMcFunc {
                    pMcLumaFunc: None,
                    pMcChromaFunc: None,
                },
                pfInterMdBackgroundDecision: None,
                pfSCDPSkipDecision: None,
                pfUpdateMbMv: None,
                pfCopy16x16Aligned: None,
                pfCopy8x8Aligned: None,
                pfGetMbSignFromInterVaa: None,
                pfMotionSearch: [None, None],
                pfGetLumaI16x16Pred: [None; 7],
                pfDctFourT4: None,
            };

            let mut pred_buf = [128u8; 512];
            let mut cs_mb = [128u8; 256];
            let mut enc_mb = [128u8; 256];

            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                bMbTypeSkip: std::ptr::null_mut(),
                iSadCost: std::ptr::null_mut(),
                iSadCostSkip: std::ptr::null_mut(),
                sMvComponents: SMVComponentUnit::default(),
                SPicData: SSampleDealingPicData {
                    pEncMb: [enc_mb.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut()],
                    pRefMb: [std::ptr::null_mut(); 3],
                    pCsMb: [cs_mb.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut()],
                },
                pSkipMb: std::ptr::null_mut(),
                pMemPredLuma: pred_buf.as_mut_ptr(),
                pMemPredChroma: std::ptr::null_mut(),
                sMbMvp: [SMVUnitXY::default(); 16],
                pCoeffLevel: std::ptr::null_mut(),
                uiNeighborIntra: 0x07, // All neighbors available
                uiLumaI16x16Mode: 0,
            };

            let mut dq_layer = SDqLayer {
                iMbWidth: 10,
                iMbHeight: 10,
                pRefLayer: std::ptr::null_mut(),
                sMbDataP: std::ptr::null_mut(),
                iEncStride: [16; 4],
                pRefPic: std::ptr::null_mut(),
                pDecPic: std::ptr::null_mut(),
                pRefOri: [std::ptr::null_mut(); 2],
                iCsStride: [16; 4],
            };

            let cost = WelsMdI16x16(
                &mut func_list as *mut SWelsFuncPtrList,
                &mut dq_layer as *mut SDqLayer,
                &mut mb_cache as *mut SMbCache,
                10,
            );

            assert!(cost < i32::MAX);
        }
    }

    #[test]
    fn test_svc_mode_decision_noop_callback() {
        unsafe {
            SetScrollingMvToMdNull(std::ptr::null_mut(), std::ptr::null_mut());
        }
    }
}
