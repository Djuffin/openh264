//! OpenH264 Video Encoder Core Context and State Machine
//!
//! Translated from `codec/encoder/core/inc/encoder_context.h` and
//! `codec/encoder/core/src/encoder.cpp`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
use crate::{
    EUsageType, RCMode, SEncParamExt, SEncoderStatistics, SSliceArgument,
    SSpatialLayerConfig, SSourcePicture, VideoFormat,
    MAX_DEPENDENCY_LAYER, MAX_QUALITY_LAYER_NUM, MAX_TEMPORAL_LAYER_NUM,
};

// ============================================================================
// Core Constants
// ============================================================================

pub const MAX_SHORT_REF_COUNT: usize = 16;
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_TEMPORAL_LEVEL: usize = MAX_TEMPORAL_LAYER_NUM;
pub const MAX_QUALITY_LEVEL: usize = MAX_QUALITY_LAYER_NUM;
pub const WELS_QP_MAX: usize = 51;
pub const WELS_CONTEXT_COUNT: usize = 460;
pub const MAX_THREADS_NUM: usize = 4;
pub const I420_PLANES: usize = 3;
pub const BASE_DEPENDENCY_ID: i32 = 0;
pub const VGOP_SIZE: i32 = 8;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 1;

// CPU Feature Bit Flags
pub const WELS_CPU_MMX: u32 = 0x00000001;
pub const WELS_CPU_MMXEXT: u32 = 0x00000002;
pub const WELS_CPU_SSE: u32 = 0x00000004;
pub const WELS_CPU_SSE2: u32 = 0x00000008;
pub const WELS_CPU_SSE3: u32 = 0x00000010;
pub const WELS_CPU_SSSE3: u32 = 0x00000020;
pub const WELS_CPU_SSE41: u32 = 0x00000040;
pub const WELS_CPU_SSE42: u32 = 0x00000080;
pub const WELS_CPU_AVX: u32 = 0x00000100;
pub const WELS_CPU_FMA: u32 = 0x00000200;
pub const WELS_CPU_AVX2: u32 = 0x00000400;
pub const WELS_CPU_NEON: u32 = 0x00000800;

// Complexity Mode Constants
pub const LOW_COMPLEXITY: i32 = 0;
pub const MEDIUM_COMPLEXITY: i32 = 1;
pub const HIGH_COMPLEXITY: i32 = 2;

// Intra Prediction Mode Count Constants
pub const I16_PRED_DC_A: usize = 7;
pub const I4_PRED_A: usize = 14;
pub const C_PRED_A: usize = 7;
pub const BLOCK_STATIC_IDC_ALL: usize = 5;
pub const BLOCK_SIZE_ALL: usize = 8;

// ============================================================================
// Bit Arithmetic Macros & Helpers
// ============================================================================

/// Calculates 4-byte (32-bit DWORD) aligned row stride for bitmap image buffers.
#[inline(always)]
pub fn CALC_BI_STRIDE(width: i32, bitcount: i32) -> i32 {
    (((width * bitcount + 31) & !31) >> 3)
}

// ============================================================================
// Enums
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
    UNKNOWN_SLICE = 5,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsNalUnitType {
    #[default]
    NAL_UNIT_UNSPEC_0 = 0,
    NAL_UNIT_CODED_SLICE = 1,
    NAL_UNIT_CODED_SLICE_DPA = 2,
    NAL_UNIT_CODED_SLICE_DPB = 3,
    NAL_UNIT_CODED_SLICE_DPC = 4,
    NAL_UNIT_CODED_SLICE_IDR = 5,
    NAL_UNIT_SEI = 6,
    NAL_UNIT_SPS = 7,
    NAL_UNIT_PPS = 8,
    NAL_UNIT_AU_DELIMITER = 9,
    NAL_UNIT_END_OF_SEQ = 10,
    NAL_UNIT_END_OF_STR = 11,
    NAL_UNIT_FILLER_DATA = 12,
    NAL_UNIT_SPS_EXT = 13,
    NAL_UNIT_PREFIX = 14,
    NAL_UNIT_SUBSET_SPS = 15,
    NAL_UNIT_RESV_16 = 16,
    NAL_UNIT_RESV_17 = 17,
    NAL_UNIT_RESV_18 = 18,
    NAL_UNIT_AUX_CODED_SLICE = 19,
    NAL_UNIT_CODED_SLICE_EXT = 20,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsNalRefIdc {
    #[default]
    NRI_PRI_LOWEST = 0,
    NRI_PRI_LOW = 1,
    NRI_PRI_HIGH = 2,
    NRI_PRI_HIGHEST = 3,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ESceneChangeIdc {
    #[default]
    NO_SCENE_CHANGE = 0,
    SIMILAR_SCENE = 1,
    LARGE_CHANGED_SCENE = 2,
}

// Re-export EVideoFrameType from crate root
pub use crate::EVideoFrameType;

// ============================================================================
// Core Supporting Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    pub pfLog: *mut c_void,
    pub pLogCtx: *mut c_void,
    pub pCodecInstance: *mut c_void,
}

impl Default for SLogContext {
    fn default() -> Self {
        Self {
            pfLog: std::ptr::null_mut(),
            pLogCtx: std::ptr::null_mut(),
            pCodecInstance: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMVUnitXY {
    pub iMvX: i16,
    pub iMvY: i16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SCropOffset {
    pub iCropLeft: i32,
    pub iCropRight: i32,
    pub iCropTop: i32,
    pub iCropBottom: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPicture {
    pub pData: [*mut u8; 4],
    pub iLineSize: [i32; 4],
    pub iWidthInPixel: i32,
    pub iHeightInPixel: i32,
    pub bUsedAsRef: bool,
    pub bIsLongRef: bool,
    pub bIsSceneLTR: bool,
    pub iFrameNum: i32,
    pub iPOC: i32,
    pub uiTemporalId: u8,
    pub uiSpatialId: u8,
    pub iMarkFrameNum: i32,
    pub iLongTermPicNum: i32,
}

impl Default for SPicture {
    fn default() -> Self {
        Self {
            pData: [std::ptr::null_mut(); 4],
            iLineSize: [0; 4],
            iWidthInPixel: 0,
            iHeightInPixel: 0,
            bUsedAsRef: false,
            bIsLongRef: false,
            bIsSceneLTR: false,
            iFrameNum: 0,
            iPOC: 0,
            uiTemporalId: 0,
            uiSpatialId: 0,
            iMarkFrameNum: 0,
            iLongTermPicNum: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpatialLayerInternal {
    pub iFrameNum: i32,
    pub iPOC: i32,
    pub iFrameIndex: i32,
    pub iCodingIndex: i32,
    pub bEncCurFrmAsIdrFlag: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsSPS {
    pub uiLog2MaxFrameNum: u32,
    pub iLog2MaxPocLsb: i32,
    pub bFrameCroppingFlag: bool,
    pub sFrameCrop: SCropOffset,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsPPS {
    pub iPicInitQp: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSubsetSps {
    pub pSps: SWelsSPS,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub pSpsP: *mut SWelsSPS,
    pub pSubsetSpsP: *mut SSubsetSps,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        Self {
            pSpsP: std::ptr::null_mut(),
            pSubsetSpsP: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSvcCodingParam {
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iRCMode: RCMode,
    pub fMaxFrameRate: f32,
    pub iTemporalLayerNum: i32,
    pub iSpatialLayerNum: i32,
    pub sSpatialLayers: [SSpatialLayerConfig; MAX_DEPENDENCY_LAYER],
    pub iComplexityMode: i32,
    pub uiIntraPeriod: u32,
    pub iNumRefFrame: i32,
    pub eSpsPpsIdStrategy: i32,
    pub bPrefixNalAddingCtrl: bool,
    pub bEnableSSEI: bool,
    pub bSimulcastAVC: bool,
    pub iPaddingFlag: i32,
    pub iEntropyCodingModeFlag: i32,
    pub bEnableFrameCroppingFlag: bool,
    pub iLoopFilterDisableIdc: i32,
    pub iLoopFilterAlphaC0Offset: i32,
    pub iLoopFilterBetaOffset: i32,
    pub bEnableDenoise: bool,
    pub bEnableSceneChangeDetect: bool,
    pub bEnableBackgroundDetection: bool,
    pub bEnableAdaptiveQuant: bool,
    pub bEnableFrameSkip: bool,
    pub bEnableLongTermReference: bool,
    pub iLtrMarkPeriod: i32,
    pub iMultipleThreadIdc: u16,
    pub bUseLoadBalancing: bool,
    pub iMaxBitrate: i32,
    pub iMinQp: i32,
    pub iMaxQp: i32,
    pub uiMaxNalSize: u32,
    pub bIsLosslessLink: bool,
    pub iLTRRefNum: i32,
    pub sDependencyLayers: [SSpatialLayerInternal; MAX_DEPENDENCY_LAYER],
}

impl Default for SWelsSvcCodingParam {
    fn default() -> Self {
        Self {
            iUsageType: EUsageType::CameraVideoRealTime,
            iPicWidth: 0,
            iPicHeight: 0,
            iTargetBitrate: 0,
            iRCMode: RCMode::RcQualityMode,
            fMaxFrameRate: 30.0,
            iTemporalLayerNum: 1,
            iSpatialLayerNum: 1,
            sSpatialLayers: [SSpatialLayerConfig::default(); MAX_DEPENDENCY_LAYER],
            iComplexityMode: LOW_COMPLEXITY,
            uiIntraPeriod: 0,
            iNumRefFrame: 1,
            eSpsPpsIdStrategy: 1,
            bPrefixNalAddingCtrl: false,
            bEnableSSEI: false,
            bSimulcastAVC: false,
            iPaddingFlag: 0,
            iEntropyCodingModeFlag: 0,
            bEnableFrameCroppingFlag: false,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            bEnableDenoise: false,
            bEnableSceneChangeDetect: true,
            bEnableBackgroundDetection: true,
            bEnableAdaptiveQuant: true,
            bEnableFrameSkip: true,
            bEnableLongTermReference: false,
            iLtrMarkPeriod: 0,
            iMultipleThreadIdc: 1,
            bUseLoadBalancing: false,
            iMaxBitrate: 0,
            iMinQp: 0,
            iMaxQp: 51,
            uiMaxNalSize: 0,
            bIsLosslessLink: false,
            iLTRRefNum: 0,
            sDependencyLayers: [SSpatialLayerInternal::default(); MAX_DEPENDENCY_LAYER],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVAAFrameInfo {
    pub bIdrPeriodFlag: bool,
    pub bSceneChangeFlag: bool,
    pub eSceneChangeIdc: ESceneChangeIdc,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBitStringAux {
    pub pStartBuf: *mut u8,
    pub pEndBuf: *mut u8,
    pub pCurBuf: *mut u8,
    pub uiBufSize: u32,
    pub iBits: i32,
}

impl Default for SBitStringAux {
    fn default() -> Self {
        Self {
            pStartBuf: std::ptr::null_mut(),
            pEndBuf: std::ptr::null_mut(),
            pCurBuf: std::ptr::null_mut(),
            uiBufSize: 0,
            iBits: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsEncoderOutput {
    pub sBsWrite: SBitStringAux,
    pub pBsBuffer: *mut u8,
    pub uiSize: u32,
    pub iNalIndex: i32,
    pub iLayerBsIndex: i32,
}

impl Default for SWelsEncoderOutput {
    fn default() -> Self {
        Self {
            sBsWrite: SBitStringAux::default(),
            pBsBuffer: std::ptr::null_mut(),
            uiSize: 0,
            iNalIndex: 0,
            iLayerBsIndex: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SStateCtx {
    pub uiState: u8,
    pub uiMPS: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SParaSetOffset {
    pub sParaSetOffsetArray: [i32; 32],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDqIdc {
    pub uiDId: u8,
    pub uiQId: u8,
    pub uiTId: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMB {
    pub iMbX: i16,
    pub iMbY: i16,
    pub uiMbType: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSliceThreading {
    pub uiSliceNum: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsSvcRc {
    pub iQp: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SExpandPicFunc {
    pub pfExpandPicLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
    pub pfExpandPicChroma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMcFunc {
    pub pfLumaHalfPelHoriz: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32)>,
    pub pfLumaHalfPelVert: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32)>,
    pub pfChromaInterpolation: Option<unsafe extern "C" fn(*mut u8, i32, *const u8, i32, i32, i32, i32, i32)>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDeblockingFunc {
    pub pfDeblockingLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32, *mut i8)>,
    pub pfDeblockingChroma: Option<unsafe extern "C" fn(*mut u8, *mut u8, i32, i32, i32, *mut i8)>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsFuncPtrList {
    pub sExpandPicFunc: SExpandPicFunc,
    pub sMcFuncs: SMcFunc,
    pub pfDeblocking: SDeblockingFunc,
    pub pfSetMemZeroSize8: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfSetMemZeroSize64Aligned16: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfSetMemZeroSize64: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub pfInterMdBackgroundDecision: Option<
        unsafe extern "C" fn(*mut sWelsEncCtx, *mut c_void, *mut c_void, *mut SMB, *mut c_void, *mut bool) -> bool,
    >,
    pub pfMdBackgroundInfoUpdate: Option<
        unsafe extern "C" fn(*mut SDqLayer, *mut SMB, bool, i32),
    >,
    pub pParametersetStrategy: *mut c_void,
}

impl Default for SWelsFuncPtrList {
    fn default() -> Self {
        Self {
            sExpandPicFunc: SExpandPicFunc::default(),
            sMcFuncs: SMcFunc::default(),
            pfDeblocking: SDeblockingFunc::default(),
            pfSetMemZeroSize8: None,
            pfSetMemZeroSize64Aligned16: None,
            pfSetMemZeroSize64: None,
            pfInterMdBackgroundDecision: None,
            pfMdBackgroundInfoUpdate: None,
            pParametersetStrategy: std::ptr::null_mut(),
        }
    }
}

// ============================================================================
// Primary Encoder Context Data Structures (encoder_context.h)
// ============================================================================

/// Reference picture lists for each spatial dependency/quality layer in SVC.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefList {
    pub pShortRefList: [*mut SPicture; 1 + MAX_SHORT_REF_COUNT],
    pub pLongRefList: [*mut SPicture; 1 + MAX_REF_PIC_COUNT],
    pub pNextBuffer: *mut SPicture,
    pub pRef: [*mut SPicture; 1 + MAX_REF_PIC_COUNT],
    pub uiShortRefCount: u8,
    pub uiLongRefCount: u8,
}

impl Default for SRefList {
    fn default() -> Self {
        Self {
            pShortRefList: [std::ptr::null_mut(); 1 + MAX_SHORT_REF_COUNT],
            pLongRefList: [std::ptr::null_mut(); 1 + MAX_REF_PIC_COUNT],
            pNextBuffer: std::ptr::null_mut(),
            pRef: [std::ptr::null_mut(); 1 + MAX_REF_PIC_COUNT],
            uiShortRefCount: 0,
            uiLongRefCount: 0,
        }
    }
}

/// Long-Term Reference (LTR) State Machine.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLTRState {
    pub uiLtrMarkState: u32,
    pub iLtrMarkFbFrameNum: i32,
    pub iLastRecoverFrameNum: i32,
    pub iLastCorFrameNumDec: i32,
    pub iCurFrameNumInDec: i32,
    pub iLTRMarkMode: i32,
    pub iLTRMarkSuccessNum: i32,
    pub iCurLtrIdx: i32,
    pub iLastLtrIdx: [i32; MAX_TEMPORAL_LAYER_NUM],
    pub iSceneLtrIdx: i32,
    pub uiLtrMarkInterval: u32,
    pub bLTRMarkingFlag: bool,
    pub bLTRMarkEnable: bool,
    pub bReceivedT0LostFlag: bool,
}

impl Default for SLTRState {
    fn default() -> Self {
        Self {
            uiLtrMarkState: 0,
            iLtrMarkFbFrameNum: 0,
            iLastRecoverFrameNum: 0,
            iLastCorFrameNumDec: 0,
            iCurFrameNumInDec: 0,
            iLTRMarkMode: 0,
            iLTRMarkSuccessNum: 0,
            iCurLtrIdx: 0,
            iLastLtrIdx: [0; MAX_TEMPORAL_LAYER_NUM],
            iSceneLtrIdx: 0,
            uiLtrMarkInterval: 0,
            bLTRMarkingFlag: false,
            bLTRMarkEnable: false,
            bReceivedT0LostFlag: false,
        }
    }
}

/// Planar YUV 4:2:0 source picture paired with spatial dependency ID.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSpatialPicIndex {
    pub pSrc: *mut SPicture,
    pub iDid: i32,
}

impl Default for SSpatialPicIndex {
    fn default() -> Self {
        Self {
            pSrc: std::ptr::null_mut(),
            iDid: 0,
        }
    }
}

/// Stride and coordinate lookup tables across spatial dependency layers.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SStrideTables {
    pub pStrideDecBlockOffset: [[*mut i32; 2]; MAX_DEPENDENCY_LAYER],
    pub pStrideEncBlockOffset: [*mut i32; MAX_DEPENDENCY_LAYER],
    pub pMbIndexX: [*mut i16; MAX_DEPENDENCY_LAYER],
    pub pMbIndexY: [*mut i16; MAX_DEPENDENCY_LAYER],
}

impl Default for SStrideTables {
    fn default() -> Self {
        Self {
            pStrideDecBlockOffset: [[std::ptr::null_mut(); 2]; MAX_DEPENDENCY_LAYER],
            pStrideEncBlockOffset: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
            pMbIndexX: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
            pMbIndexY: [std::ptr::null_mut(); MAX_DEPENDENCY_LAYER],
        }
    }
}

/// Master runtime encoder context (`sWelsEncCtx` / `TagWelsEncCtx`).
#[repr(C)]
pub struct sWelsEncCtx {
    pub sLogCtx: SLogContext,
    pub pSvcParam: *mut SWelsSvcCodingParam,
    pub pSadCostMb: *mut i32,
    pub iMvRange: i32,
    pub pMvdCostTable: *mut u16,
    pub iMvdCostTableSize: i32,
    pub iMvdCostTableStride: i32,
    pub pMvUnitBlock4x4: *mut SMVUnitXY,
    pub pRefIndexBlock4x4: *mut i8,
    pub pNonZeroCountBlocks: *mut i8,
    pub pIntra4x4PredModeBlocks: *mut i8,
    pub ppMbListD: *mut *mut SMB,
    pub pStrideTab: *mut SStrideTables,
    pub pFuncList: *mut SWelsFuncPtrList,
    pub pSliceThreading: *mut SSliceThreading,
    pub pTaskManage: *mut c_void,
    pub pReferenceStrategy: *mut c_void,
    pub pEncPic: *mut SPicture,
    pub pDecPic: *mut SPicture,
    pub pRefPic: *mut SPicture,
    pub pCurDqLayer: *mut SDqLayer,
    pub ppDqLayerList: *mut *mut SDqLayer,
    pub ppRefPicListExt: *mut *mut SRefList,
    pub pRefList0: [*mut SPicture; 16],
    pub pLtr: *mut SLTRState,
    pub bCurFrameMarkedAsSceneLtr: bool,
    pub eSliceType: EWelsSliceType,
    pub eNalType: EWelsNalUnitType,
    pub eNalPriority: EWelsNalRefIdc,
    pub eLastNalPriority: [EWelsNalRefIdc; MAX_DEPENDENCY_LAYER],
    pub iNumRef0: u8,
    pub uiDependencyId: u8,
    pub uiTemporalId: u8,
    pub bNeedPrefixNalFlag: bool,
    pub pWelsSvcRc: *mut SWelsSvcRc,
    pub bCheckWindowStatusRefreshFlag: bool,
    pub iCheckWindowStartTs: i64,
    pub iCheckWindowCurrentTs: i64,
    pub iCheckWindowInterval: i32,
    pub iCheckWindowIntervalShift: i32,
    pub bCheckWindowShiftResetFlag: bool,
    pub iGlobalQp: i32,
    pub pVaa: *mut SVAAFrameInfo,
    pub pVpp: *mut c_void,
    pub pSpsArray: *mut SWelsSPS,
    pub pSps: *mut SWelsSPS,
    pub pPPSArray: *mut SWelsPPS,
    pub pPps: *mut SWelsPPS,
    pub pSubsetArray: *mut SSubsetSps,
    pub pSubsetSps: *mut SSubsetSps,
    pub iSpsNum: i32,
    pub iSubsetSpsNum: i32,
    pub iPpsNum: i32,
    pub pOut: *mut SWelsEncoderOutput,
    pub pFrameBs: *mut u8,
    pub iFrameBsSize: i32,
    pub iPosBsBuffer: i32,
    pub sSpatialIndexMap: [SSpatialPicIndex; MAX_DEPENDENCY_LAYER],
    pub iSliceBufferSize: [i32; MAX_DEPENDENCY_LAYER],
    pub bRefOfCurTidIsLtr: [[bool; MAX_TEMPORAL_LEVEL]; MAX_DEPENDENCY_LAYER],
    pub iMaxSliceCount: i32,
    pub iActiveThreadsNum: i16,
    pub pDqIdcMap: *mut SDqIdc,
    pub sPSOVector: SParaSetOffset,
    pub pPSOVector: *mut SParaSetOffset,
    pub pMemAlign: *mut CMemoryAlign,
    pub uiStartTimestamp: i64,
    pub sEncoderStatistics: [SEncoderStatistics; MAX_DEPENDENCY_LAYER],
    pub iStatisticsLogInterval: i32,
    pub iLastStatisticsLogTs: i64,
    pub iEncoderError: i32,
    pub mutexEncoderError: *mut c_void,
    pub bDeliveryFlag: bool,
    pub sWelsCabacContexts: [[[SStateCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub uiLastTimestamp: i64,
    pub pDynamicBsBuffer: [*mut u8; MAX_THREADS_NUM],
}

// ============================================================================
// Background Detection Fallback Callbacks
// ============================================================================

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskip(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut c_void,
    _slice: *mut c_void,
    _pCurMb: *mut SMB,
    _pMbCache: *mut c_void,
    _pKeepPskip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfo(
    _pCurLayer: *mut SDqLayer,
    _pCurMb: *mut SMB,
    _bFlag: bool,
    _kiRefPictureType: i32,
) {}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskipFalse(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: *mut c_void,
    _slice: *mut c_void,
    _pCurMb: *mut SMB,
    _pMbCache: *mut c_void,
    _pKeepPskip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfoNULL(
    _pCurLayer: *mut SDqLayer,
    _pCurMb: *mut SMB,
    _bFlag: bool,
    _kiRefPictureType: i32,
) {}

// ============================================================================
// Core Encoder Functions (encoder.cpp)
// ============================================================================

/// Initializes input source picture geometry, color planes, and line strides.
///
/// # Safety
/// `kpSrc` must point to a valid `SSourcePicture` struct or be null.
pub unsafe fn InitPic(
    kpSrc: *const c_void,
    kiColorspace: i32,
    kiWidth: i32,
    kiHeight: i32,
) -> i32 {
    let pSrcPic = kpSrc as *mut SSourcePicture;

    if pSrcPic.is_null() || kiWidth == 0 || kiHeight == 0 {
        return 1;
    }

    let vflip_mask = VideoFormat::VideoFormatVFlip as i32;
    let base_colorspace = kiColorspace & !vflip_mask;

    (*pSrcPic).iColorFormat = kiColorspace;
    (*pSrcPic).iPicWidth = kiWidth;
    (*pSrcPic).iPicHeight = kiHeight;

    if base_colorspace != VideoFormat::VideoFormatI420 as i32 {
        return 2;
    }

    match base_colorspace {
        cs if cs == VideoFormat::VideoFormatI420 as i32
            || cs == VideoFormat::VideoFormatYv12 as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = kiWidth;
            (*pSrcPic).iStride[1] = kiWidth >> 1;
            (*pSrcPic).iStride[2] = kiWidth >> 1;
            (*pSrcPic).iStride[3] = 0;
        }
        cs if cs == VideoFormat::VideoFormatYuy2 as i32
            || cs == VideoFormat::VideoFormatYvyu as i32
            || cs == VideoFormat::VideoFormatUyvy as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = CALC_BI_STRIDE(kiWidth, 16);
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
        }
        cs if cs == VideoFormat::VideoFormatRgb as i32
            || cs == VideoFormat::VideoFormatBgr as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = CALC_BI_STRIDE(kiWidth, 24);
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                (*pSrcPic).iColorFormat = kiColorspace & !vflip_mask;
            } else {
                (*pSrcPic).iColorFormat = kiColorspace | vflip_mask;
            }
        }
        cs if cs == VideoFormat::VideoFormatBgra as i32
            || cs == VideoFormat::VideoFormatRgba as i32
            || cs == VideoFormat::VideoFormatArgb as i32
            || cs == VideoFormat::VideoFormatAbgr as i32 =>
        {
            (*pSrcPic).pData[0] = std::ptr::null_mut();
            (*pSrcPic).pData[1] = std::ptr::null_mut();
            (*pSrcPic).pData[2] = std::ptr::null_mut();
            (*pSrcPic).pData[3] = std::ptr::null_mut();
            (*pSrcPic).iStride[0] = kiWidth << 2;
            (*pSrcPic).iStride[1] = 0;
            (*pSrcPic).iStride[2] = 0;
            (*pSrcPic).iStride[3] = 0;
            if (kiColorspace & vflip_mask) != 0 {
                (*pSrcPic).iColorFormat = kiColorspace & !vflip_mask;
            } else {
                (*pSrcPic).iColorFormat = kiColorspace | vflip_mask;
            }
        }
        _ => return 2,
    }

    0
}

/// Wires background detection function pointers into the encoder function table.
///
/// # Safety
/// `pFuncList` must be non-null and point to a valid `SWelsFuncPtrList`.
pub unsafe fn WelsInitBGDFunc(
    pFuncList: *mut SWelsFuncPtrList,
    kbEnableBackgroundDetection: bool,
) {
    if pFuncList.is_null() {
        return;
    }
    if kbEnableBackgroundDetection {
        (*pFuncList).pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskip);
        (*pFuncList).pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfo);
    } else {
        (*pFuncList).pfInterMdBackgroundDecision = Some(WelsMdInterJudgeBGDPskipFalse);
        (*pFuncList).pfMdBackgroundInfoUpdate = Some(WelsMdUpdateBGDInfoNULL);
    }
}

/// Initializes encoder compute kernel function pointers.
///
/// # Safety
/// `pEncCtx` and `pParam` must be valid non-null pointers.
pub unsafe fn InitFunctionPointers(
    pEncCtx: *mut sWelsEncCtx,
    pParam: *mut SWelsSvcCodingParam,
    _uiCpuFlag: u32,
) -> i32 {
    if pEncCtx.is_null() || pParam.is_null() || (*pEncCtx).pFuncList.is_null() {
        return ENC_RETURN_SUCCESS;
    }
    let pFuncList = (*pEncCtx).pFuncList;

    (*pFuncList).pfSetMemZeroSize8 = Some(WelsSetMemZero_c_extern);
    (*pFuncList).pfSetMemZeroSize64Aligned16 = Some(WelsSetMemZero_c_extern);
    (*pFuncList).pfSetMemZeroSize64 = Some(WelsSetMemZero_c_extern);

    WelsInitBGDFunc(pFuncList, (*pParam).bEnableBackgroundDetection);

    ENC_RETURN_SUCCESS
}

/// Increments the H.264 slice header `frame_num` syntax element for spatial layer `kiDidx`.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn UpdateFrameNum(pEncCtx: *mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        let max_frame_num_minus1 = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
        if pParamInternal.iFrameNum < max_frame_num_minus1 {
            pParamInternal.iFrameNum += 1;
        } else {
            pParamInternal.iFrameNum = 0;
        }
    }

    (*pEncCtx).eLastNalPriority[kiDidx as usize] = EWelsNalRefIdc::NRI_PRI_LOWEST;
}

/// Rolls back the `frame_num` counter if a reference frame encoding attempt fails.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn LoadBackFrameNum(pEncCtx: *mut sWelsEncCtx, kiDidx: i32) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];
    let mut bNeedFrameNumIncreasing = false;

    if (*pEncCtx).eLastNalPriority[kiDidx as usize] != EWelsNalRefIdc::NRI_PRI_LOWEST {
        bNeedFrameNumIncreasing = true;
    }

    if bNeedFrameNumIncreasing {
        if pParamInternal.iFrameNum != 0 {
            pParamInternal.iFrameNum -= 1;
        } else {
            pParamInternal.iFrameNum = (1 << (*(*pEncCtx).pSps).uiLog2MaxFrameNum) - 1;
        }
    }
}

/// Resets the auxiliary bitstream serializer buffer pointers before frame encoding.
///
/// # Safety
/// `pBs` must point to a valid `SBitStringAux`.
pub unsafe fn InitBits(pBs: *mut SBitStringAux, pBuf: *mut u8, uiSize: u32) {
    if pBs.is_null() {
        return;
    }
    (*pBs).pStartBuf = pBuf;
    (*pBs).pCurBuf = pBuf;
    (*pBs).pEndBuf = if pBuf.is_null() {
        std::ptr::null_mut()
    } else {
        pBuf.add(uiSize as usize)
    };
    (*pBs).uiBufSize = uiSize;
    (*pBs).iBits = 0;
}

/// Reinitializes bitstream buffer write offsets and NAL indices.
///
/// # Safety
/// `pEncCtx` must be non-null and contain a valid `pOut`.
pub unsafe fn InitBitStream(pEncCtx: *mut sWelsEncCtx) {
    if pEncCtx.is_null() || (*pEncCtx).pOut.is_null() {
        return;
    }
    (*pEncCtx).iPosBsBuffer = 0;
    (*(*pEncCtx).pOut).iNalIndex = 0;
    (*(*pEncCtx).pOut).iLayerBsIndex = 0;

    InitBits(
        &mut (*(*pEncCtx).pOut).sBsWrite,
        (*(*pEncCtx).pOut).pBsBuffer,
        (*(*pEncCtx).pOut).uiSize,
    );
}

/// Configures slice types, NAL headers, and Picture Order Count (POC) for the frame.
///
/// # Safety
/// `pEncCtx` must be non-null and properly initialized.
pub unsafe fn InitFrameCoding(
    pEncCtx: *mut sWelsEncCtx,
    keFrameType: EVideoFrameType,
    kiDidx: i32,
) {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() || (*pEncCtx).pSps.is_null() {
        return;
    }
    let pParamInternal = &mut (*(*pEncCtx).pSvcParam).sDependencyLayers[kiDidx as usize];

    if keFrameType == EVideoFrameType::VideoFrameTypeP {
        pParamInternal.iFrameIndex += 1;

        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
        if pParamInternal.iPOC < max_poc_boundary {
            pParamInternal.iPOC += 2;
        } else {
            pParamInternal.iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::P_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGH;
    } else if keFrameType == EVideoFrameType::VideoFrameTypeIDR {
        pParamInternal.iFrameNum = 0;
        pParamInternal.iPOC = 0;
        pParamInternal.bEncCurFrmAsIdrFlag = false;
        pParamInternal.iFrameIndex = 0;

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
        (*pEncCtx).eSliceType = EWelsSliceType::I_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;

        pParamInternal.iCodingIndex = 0;
    } else if keFrameType == EVideoFrameType::VideoFrameTypeI {
        let max_poc_boundary = (1 << (*(*pEncCtx).pSps).iLog2MaxPocLsb) - 2;
        if pParamInternal.iPOC < max_poc_boundary {
            pParamInternal.iPOC += 2;
        } else {
            pParamInternal.iPOC = 0;
        }

        UpdateFrameNum(pEncCtx, kiDidx);

        (*pEncCtx).eNalType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
        (*pEncCtx).eSliceType = EWelsSliceType::I_SLICE;
        (*pEncCtx).eNalPriority = EWelsNalRefIdc::NRI_PRI_HIGHEST;
    }
}

/// Evaluates VAA scene change analysis, LTR feedback, and rate control constraints to classify frame coding type.
///
/// # Safety
/// `pEncCtx` must be non-null and initialized.
pub unsafe fn DecideFrameType(
    pEncCtx: *mut sWelsEncCtx,
    kiSpatialNum: i8,
    kiDidx: i32,
    bSkipFrameFlag: bool,
) -> EVideoFrameType {
    if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() {
        return EVideoFrameType::VideoFrameTypeInvalid;
    }
    let pSvcParam = (*pEncCtx).pSvcParam;
    let pParamInternal = &mut (*pSvcParam).sDependencyLayers[kiDidx as usize];
    let mut iFrameType = EVideoFrameType::VideoFrameTypeInvalid;
    let mut bSceneChangeFlag = false;

    if (*pSvcParam).iUsageType == EUsageType::ScreenContentRealTime {
        let pVaa = (*pEncCtx).pVaa;
        let vaa_idr = !pVaa.is_null() && (*pVaa).bIdrPeriodFlag;

        if !(*pSvcParam).bEnableSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < (*pSvcParam).iSpatialLayerNum)
        {
            bSceneChangeFlag = false;
        } else if !pVaa.is_null() {
            bSceneChangeFlag = (*pVaa).bSceneChangeFlag;
        }

        if vaa_idr
            || pParamInternal.bEncCurFrmAsIdrFlag
            || (!(*pSvcParam).bEnableLongTermReference && bSceneChangeFlag && !bSkipFrameFlag)
        {
            iFrameType = EVideoFrameType::VideoFrameTypeIDR;
        } else if (*pSvcParam).bEnableLongTermReference
            && (bSceneChangeFlag
                || (!pVaa.is_null() && (*pVaa).eSceneChangeIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE))
        {
            let mut iActualLtrcount = 0;
            if !(*pEncCtx).ppRefPicListExt.is_null() {
                let ref_list_0 = *(*pEncCtx).ppRefPicListExt;
                if !ref_list_0.is_null() {
                    let pLongTermRefList = (*ref_list_0).pLongRefList.as_ptr();
                    for i in 0..(*pSvcParam).iLTRRefNum {
                        let pic = *pLongTermRefList.add(i as usize);
                        if !pic.is_null()
                            && (*pic).bUsedAsRef
                            && (*pic).bIsLongRef
                            && (*pic).bIsSceneLTR
                        {
                            iActualLtrcount += 1;
                        }
                    }
                }
            }
            if iActualLtrcount == (*pSvcParam).iLTRRefNum && bSceneChangeFlag {
                iFrameType = EVideoFrameType::VideoFrameTypeIDR;
            } else {
                iFrameType = EVideoFrameType::VideoFrameTypeP;
                (*pEncCtx).bCurFrameMarkedAsSceneLtr = true;
            }
        } else {
            iFrameType = EVideoFrameType::VideoFrameTypeP;
        }

        if iFrameType == EVideoFrameType::VideoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::VideoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::VideoFrameTypeIDR {
            pParamInternal.iCodingIndex = 0;
            (*pEncCtx).bCurFrameMarkedAsSceneLtr = true;
        }
    } else {
        let pVaa = (*pEncCtx).pVaa;
        let vaa_idr = !pVaa.is_null() && (*pVaa).bIdrPeriodFlag;

        if !(*pSvcParam).bEnableSceneChangeDetect
            || vaa_idr
            || ((kiSpatialNum as i32) < (*pSvcParam).iSpatialLayerNum)
            || (pParamInternal.iFrameIndex < (VGOP_SIZE << 1))
        {
            bSceneChangeFlag = false;
        } else if !pVaa.is_null() {
            bSceneChangeFlag = (*pVaa).bSceneChangeFlag;
        }

        iFrameType = if vaa_idr || bSceneChangeFlag || pParamInternal.bEncCurFrmAsIdrFlag {
            EVideoFrameType::VideoFrameTypeIDR
        } else {
            EVideoFrameType::VideoFrameTypeP
        };

        if iFrameType == EVideoFrameType::VideoFrameTypeP && bSkipFrameFlag {
            iFrameType = EVideoFrameType::VideoFrameTypeSkip;
        } else if iFrameType == EVideoFrameType::VideoFrameTypeIDR {
            pParamInternal.iCodingIndex = 0;
        }
    }

    iFrameType
}

/// Portable C/C++ fallback for memory clearing.
///
/// # Safety
/// `pDst` must point to valid writable memory of at least `iSize` bytes.
pub unsafe fn WelsSetMemZero_c(pDst: *mut c_void, iSize: i32) {
    if !pDst.is_null() && iSize > 0 {
        std::ptr::write_bytes(pDst as *mut u8, 0, iSize as usize);
    }
}

pub unsafe extern "C" fn WelsSetMemZero_c_extern(pDst: *mut c_void, iSize: i32) {
    WelsSetMemZero_c(pDst, iSize);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_calc_bi_stride() {
        assert_eq!(CALC_BI_STRIDE(640, 24), 1920);
        assert_eq!(CALC_BI_STRIDE(1920, 16), 3840);
        assert_eq!(CALC_BI_STRIDE(1920, 24), 5760);
    }

    #[test]
    fn test_init_pic() {
        let mut src_pic = SSourcePicture::default();
        let ret = unsafe {
            InitPic(
                &mut src_pic as *mut SSourcePicture as *mut c_void,
                VideoFormat::VideoFormatI420 as i32,
                640,
                480,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(src_pic.iPicWidth, 640);
        assert_eq!(src_pic.iPicHeight, 480);
        assert_eq!(src_pic.iStride[0], 640);
        assert_eq!(src_pic.iStride[1], 320);
        assert_eq!(src_pic.iStride[2], 320);
    }

    #[test]
    fn test_update_and_loadback_framenum() {
        let mut param = SWelsSvcCodingParam::default();
        let mut sps = SWelsSPS {
            uiLog2MaxFrameNum: 4,
            iLog2MaxPocLsb: 4,
            bFrameCroppingFlag: false,
            sFrameCrop: SCropOffset::default(),
        };
        let mut ctx = unsafe { std::mem::zeroed::<sWelsEncCtx>() };
        ctx.pSvcParam = &mut param;
        ctx.pSps = &mut sps;
        ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;

        unsafe {
            UpdateFrameNum(&mut ctx, 0);
            assert_eq!(param.sDependencyLayers[0].iFrameNum, 1);
            assert_eq!(ctx.eLastNalPriority[0], EWelsNalRefIdc::NRI_PRI_LOWEST);

            ctx.eLastNalPriority[0] = EWelsNalRefIdc::NRI_PRI_HIGH;
            LoadBackFrameNum(&mut ctx, 0);
            assert_eq!(param.sDependencyLayers[0].iFrameNum, 0);
        }
    }

    #[test]
    fn test_decide_frame_type() {
        let mut param = SWelsSvcCodingParam::default();
        let mut vaa = SVAAFrameInfo::default();
        let mut ctx = unsafe { std::mem::zeroed::<sWelsEncCtx>() };
        ctx.pSvcParam = &mut param;
        ctx.pVaa = &mut vaa;
        param.sDependencyLayers[0].bEncCurFrmAsIdrFlag = true;

        unsafe {
            let ft = DecideFrameType(&mut ctx, 1, 0, false);
            assert_eq!(ft, EVideoFrameType::VideoFrameTypeIDR);
        }
    }
}
