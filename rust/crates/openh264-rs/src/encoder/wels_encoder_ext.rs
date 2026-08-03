#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! C++ SVC Encoder Facade and Lifecycle Controller (`CWelsH264SVCEncoder`).
//!
//! Translated from `codec/encoder/plus/inc/welsEncoderExt.h` and `codec/encoder/plus/src/welsEncoderExt.cpp`.

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    EComplexityMode, EParameterSetStrategy, EUsageType, EVideoFrameType, EncoderOption,
    ISVCEncoderVtbl, OpenH264Version, RCMode, SBitrateInfo, SEncParamBase,
    SEncParamExt, SFrameBSInfo, SLayerBSInfo, SSourcePicture, SSpatialLayerConfig, VideoFormat,
    CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS, CM_UNINITIALIZED_ERROR,
    CM_UNKNOWN_REASON, CM_UNSUPPORTED_DATA, MAX_LAYER_NUM_OF_FRAME, MAX_SPATIAL_LAYER_NUM,
    MAX_TEMPORAL_LAYER_NUM,
};
use crate::api::codec_api::ISVCEncoder as ISVCEncoderHandle;

pub const VERSION_NUMBER: &str = "openh264 2.6.0";

pub const MAX_DEPENDENCY_LAYER: i32 = 4;
pub const MAX_TEMPORAL_LEVEL: i32 = 4;
pub const MAX_GOP_SIZE: u32 = 64;
pub const MIN_FRAME_RATE: f32 = 1.0;
pub const MAX_FRAME_RATE: f32 = 60.0;
pub const MIN_BIT_RATE: i32 = 1;
pub const MAX_BIT_RATE: i32 = i32::MAX;
pub const MIN_REF_PIC_COUNT: i32 = 1;
pub const MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA: i32 = 16;
pub const AUTO_REF_PIC_COUNT: i32 = -1;
pub const LONG_TERM_REF_NUM: i32 = 1;
pub const LONG_TERM_REF_NUM_SCREEN: i32 = 2;

pub const VIDEO_CODING_LAYER: u8 = 0;
pub const NON_VIDEO_CODING_LAYER: u8 = 1;

pub const SPATIAL_LAYER_ALL: i32 = -1;
pub const SPATIAL_LAYER_0: i32 = 0;
pub const SPATIAL_LAYER_1: i32 = 1;
pub const SPATIAL_LAYER_2: i32 = 2;
pub const SPATIAL_LAYER_3: i32 = 3;

// CM_RETURN return status codes matching codec_def.h
pub const cmResultSuccess: i32 = 0;
pub const cmInitParaError: i32 = 1;
pub const cmUnknownReason: i32 = 2;
pub const cmMallocMemeError: i32 = 3;
pub const cmInitExpected: i32 = 4;
pub const cmUnsupportedData: i32 = 5;

// Encoder Core return codes
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 1;
pub const ENC_RETURN_INVALIDINPUT: i32 = 2;
pub const ENC_RETURN_MEMOVERFLOWFOUND: i32 = 3;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 4;
pub const ENC_RETURN_CORRECTED: i32 = 5;

// Log levels matching WELS_LOG_LEVEL
pub const WELS_LOG_QUIET: i32 = 0;
pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;
pub const WELS_LOG_DEBUG: i32 = 4;
pub const WELS_LOG_DETAIL: i32 = 5;

#[inline(always)]
pub fn WELS_LOG2(mut v: u32) -> i32 {
    let mut r = 0;
    while {
        v >>= 1;
        v != 0
    } {
        r += 1;
    }
    r
}

#[inline(always)]
pub fn WELS_CLIP3<T: PartialOrd + Copy>(v: T, min_val: T, max_val: T) -> T {
    if v < min_val {
        min_val
    } else if v > max_val {
        max_val
    } else {
        v
    }
}

#[inline(always)]
pub fn WELS_MAX<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a > b {
        a
    } else {
        b
    }
}

#[inline(always)]
pub fn WELS_ABS(a: f32) -> f32 {
    a.abs()
}

#[inline(always)]
pub fn WELS_POWER2_IF(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

pub fn WelsTime() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_micros() as i64,
        Err(_) => 0,
    }
}

pub type WelsTraceCallback =
    Option<unsafe extern "C" fn(pCtx: *mut c_void, iLevel: i32, pStr: *const c_char)>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    pub pLogCtx: *mut c_void,
}

impl Default for SLogContext {
    fn default() -> Self {
        Self {
            pLogCtx: null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct welsCodecTrace {
    pub m_iTraceLevel: i32,
    pub m_fpTrace: WelsTraceCallback,
    pub m_pTraceCtx: *mut c_void,
    pub m_sLogCtx: SLogContext,
    pub m_pCodecInstance: *mut c_void,
}

impl Default for welsCodecTrace {
    fn default() -> Self {
        Self {
            m_iTraceLevel: WELS_LOG_ERROR,
            m_fpTrace: None,
            m_pTraceCtx: null_mut(),
            m_sLogCtx: SLogContext::default(),
            m_pCodecInstance: null_mut(),
        }
    }
}

impl welsCodecTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn SetCodecInstance(&mut self, pCodecInstance: *mut c_void) {
        self.m_pCodecInstance = pCodecInstance;
    }

    pub fn SetTraceLevel(&mut self, kiLevel: u32) {
        self.m_iTraceLevel = kiLevel as i32;
    }

    pub fn GetTraceLevel(&self) -> i32 {
        self.m_iTraceLevel
    }

    pub fn SetTraceCallback(&mut self, func: WelsTraceCallback) {
        self.m_fpTrace = func;
    }

    pub fn SetTraceCallbackContext(&mut self, pCtx: *mut c_void) {
        self.m_pTraceCtx = pCtx;
    }
}

pub fn WelsLog(pLogCtx: *mut SLogContext, iLevel: i32, msg: &str) {
    // Basic diagnostic logging helper matching OpenH264 trace logging
    let _ = (pLogCtx, iLevel, msg);
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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLTRConfig {
    pub bEnableLongTermReference: bool,
    pub iLTRRefNum: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SProfileInfo {
    pub iLayer: i32,
    pub uiProfileIdc: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLevelInfo {
    pub iLayer: i32,
    pub uiLevelIdc: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDeliveryStatus {
    pub bDeliveryFlag: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDumpLayer {
    pub iLayer: i32,
    pub pFileName: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSpatialLayerInternal {
    pub iActualWidth: i32,
    pub iActualHeight: i32,
    pub sRecFileName: [u8; 256],
    pub iCodingIndex: i32,
    pub iPOC: i32,
    pub uiIdrPicId: u32,
    pub iFrameNum: i32,
    pub bEncCurFrmAsIdrFlag: bool,
    pub iDecompositionStages: i32,
    pub iHighestTemporalId: i32,
    pub iFrameIndex: i32,
}

impl Default for SSpatialLayerInternal {
    fn default() -> Self {
        Self {
            iActualWidth: 0,
            iActualHeight: 0,
            sRecFileName: [0; 256],
            iCodingIndex: 0,
            iPOC: 0,
            uiIdrPicId: 0,
            iFrameNum: 0,
            bEncCurFrmAsIdrFlag: false,
            iDecompositionStages: 0,
            iHighestTemporalId: 0,
            iFrameIndex: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSvcCodingParam {
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iMaxBitrate: i32,
    pub iRCMode: RCMode,
    pub fMaxFrameRate: f32,
    pub iTemporalLayerNum: i32,
    pub iSpatialLayerNum: i32,
    pub sSpatialLayers: [SSpatialLayerConfig; MAX_SPATIAL_LAYER_NUM],
    pub sDependencyLayers: [SSpatialLayerInternal; MAX_SPATIAL_LAYER_NUM],
    pub uiGopSize: u32,
    pub uiIntraPeriod: u32,
    pub iNumRefFrame: i32,
    pub eSpsPpsIdStrategy: EParameterSetStrategy,
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
    pub iLtrMarkPeriod: u32,
    pub iMultipleThreadIdc: u16,
    pub bUseLoadBalancing: bool,
    pub iMinQp: i32,
    pub iMaxQp: i32,
    pub uiMaxNalSize: u32,
    pub bIsLosslessLink: bool,
    pub iComplexityMode: EComplexityMode,
    pub iLTRRefNum: i32,
    pub pCurPath: *mut c_char,
    pub iBitsVaryPercentage: i32,
}

impl Default for SWelsSvcCodingParam {
    fn default() -> Self {
        Self {
            iUsageType: EUsageType::CameraVideoRealTime,
            iPicWidth: 0,
            iPicHeight: 0,
            iTargetBitrate: 0,
            iMaxBitrate: 0,
            iRCMode: RCMode::RcQualityMode,
            fMaxFrameRate: 30.0,
            iTemporalLayerNum: 1,
            iSpatialLayerNum: 1,
            sSpatialLayers: [SSpatialLayerConfig::default(); MAX_SPATIAL_LAYER_NUM],
            sDependencyLayers: [SSpatialLayerInternal::default(); MAX_SPATIAL_LAYER_NUM],
            uiGopSize: 1,
            uiIntraPeriod: 0,
            iNumRefFrame: AUTO_REF_PIC_COUNT,
            eSpsPpsIdStrategy: EParameterSetStrategy::ConstantId,
            bPrefixNalAddingCtrl: false,
            bEnableSSEI: false,
            bSimulcastAVC: false,
            iPaddingFlag: 0,
            iEntropyCodingModeFlag: 0,
            bEnableFrameCroppingFlag: true,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            bEnableDenoise: false,
            bEnableSceneChangeDetect: true,
            bEnableBackgroundDetection: true,
            bEnableAdaptiveQuant: true,
            bEnableFrameSkip: true,
            bEnableLongTermReference: false,
            iLtrMarkPeriod: 30,
            iMultipleThreadIdc: 1,
            bUseLoadBalancing: true,
            iMinQp: 0,
            iMaxQp: 51,
            uiMaxNalSize: 0,
            bIsLosslessLink: false,
            iComplexityMode: EComplexityMode::LowComplexity,
            iLTRRefNum: 0,
            pCurPath: null_mut(),
            iBitsVaryPercentage: 0,
        }
    }
}

impl SWelsSvcCodingParam {
    pub fn FillDefault(argv: &mut SEncParamExt) {
        argv.iUsageType = EUsageType::CameraVideoRealTime;
        argv.iPicWidth = 0;
        argv.iPicHeight = 0;
        argv.iTargetBitrate = 0;
        argv.iRCMode = RCMode::RcQualityMode;
        argv.fMaxFrameRate = 30.0;
        argv.iTemporalLayerNum = 1;
        argv.iSpatialLayerNum = 1;
        argv.uiIntraPeriod = 0;
        argv.iNumRefFrame = AUTO_REF_PIC_COUNT;
        argv.bEnableFrameSkip = true;
        argv.iComplexityMode = 0;
    }

    pub fn ParamBaseTranscode(&mut self, argv: SEncParamBase) -> i32 {
        if argv.iPicWidth <= 0 || argv.iPicHeight <= 0 {
            return cmInitParaError;
        }
        self.iUsageType = argv.iUsageType;
        self.iPicWidth = argv.iPicWidth;
        self.iPicHeight = argv.iPicHeight;
        self.iTargetBitrate = argv.iTargetBitrate;
        self.iRCMode = argv.iRCMode;
        self.fMaxFrameRate = argv.fMaxFrameRate;
        self.iSpatialLayerNum = 1;

        self.sSpatialLayers[0].iVideoWidth = argv.iPicWidth;
        self.sSpatialLayers[0].iVideoHeight = argv.iPicHeight;
        self.sSpatialLayers[0].fFrameRate = argv.fMaxFrameRate;
        self.sSpatialLayers[0].iSpatialBitrate = argv.iTargetBitrate;
        self.sSpatialLayers[0].iMaxSpatialBitrate = argv.iTargetBitrate;

        self.sDependencyLayers[0].iActualWidth = argv.iPicWidth;
        self.sDependencyLayers[0].iActualHeight = argv.iPicHeight;
        if self.uiGopSize == 0 {
            self.uiGopSize = 1;
        }
        0
    }

    pub fn ParamTranscode(&mut self, argv: SEncParamExt) -> i32 {
        if argv.iPicWidth <= 0 || argv.iPicHeight <= 0 {
            return cmInitParaError;
        }
        self.iUsageType = argv.iUsageType;
        self.iPicWidth = argv.iPicWidth;
        self.iPicHeight = argv.iPicHeight;
        self.iTargetBitrate = argv.iTargetBitrate;
        self.iRCMode = argv.iRCMode;
        self.fMaxFrameRate = argv.fMaxFrameRate;
        self.iTemporalLayerNum = argv.iTemporalLayerNum;
        self.iSpatialLayerNum = argv.iSpatialLayerNum;
        self.sSpatialLayers = argv.sSpatialLayers;
        self.uiIntraPeriod = argv.uiIntraPeriod;
        self.iNumRefFrame = argv.iNumRefFrame;
        self.bPrefixNalAddingCtrl = argv.bPrefixNalAddingCtrl;
        self.bEnableSSEI = argv.bEnableSSEI;
        self.bSimulcastAVC = argv.bSimulcastAVC;
        self.iPaddingFlag = argv.iPaddingFlag;
        self.iEntropyCodingModeFlag = argv.iEntropyCodingModeFlag;
        self.bEnableFrameCroppingFlag = argv.bEnableFrameCroppingFlag;
        self.iLoopFilterDisableIdc = argv.iLoopFilterDisableIdc;
        self.iLoopFilterAlphaC0Offset = argv.iLoopFilterAlphaC0Offset;
        self.iLoopFilterBetaOffset = argv.iLoopFilterBetaOffset;
        self.bEnableDenoise = argv.bEnableDenoise;
        self.bEnableSceneChangeDetect = argv.bEnableSceneChangeDetect;
        self.bEnableBackgroundDetection = argv.bEnableBackgroundDetection;
        self.bEnableAdaptiveQuant = argv.bEnableAdaptiveQuant;
        self.bEnableFrameSkip = argv.bEnableFrameSkip;
        self.bEnableLongTermReference = argv.bEnableLongTermReference;
        self.iLtrMarkPeriod = argv.iLtrMarkPeriod as u32;
        self.iMultipleThreadIdc = argv.iMultipleThreadIdc;
        self.bUseLoadBalancing = argv.bUseLoadBalancing;
        self.iMaxBitrate = argv.iMaxBitrate;
        self.iMinQp = argv.iMinQp;
        self.iMaxQp = argv.iMaxQp;
        self.uiMaxNalSize = argv.uiMaxNalSize;
        self.bIsLosslessLink = argv.bIsLosslessLink;

        if self.uiGopSize == 0 {
            self.uiGopSize = 1;
        }

        let num_layers = WELS_CLIP3(argv.iSpatialLayerNum, 1, MAX_DEPENDENCY_LAYER) as usize;
        for i in 0..num_layers {
            self.sDependencyLayers[i].iActualWidth = argv.sSpatialLayers[i].iVideoWidth;
            self.sDependencyLayers[i].iActualHeight = argv.sSpatialLayers[i].iVideoHeight;
        }
        0
    }

    pub fn DetermineTemporalSettings(&mut self) -> i32 {
        let kiDecStages = WELS_LOG2(self.uiGopSize);
        self.iTemporalLayerNum = 1 + kiDecStages;
        0
    }

    pub fn GetBaseParams(&self, pParam: &mut SEncParamBase) {
        pParam.iUsageType = self.iUsageType;
        pParam.iPicWidth = self.iPicWidth;
        pParam.iPicHeight = self.iPicHeight;
        pParam.iTargetBitrate = self.iTargetBitrate;
        pParam.iRCMode = self.iRCMode;
        pParam.fMaxFrameRate = self.fMaxFrameRate;
    }

    pub fn to_param_ext(&self) -> SEncParamExt {
        SEncParamExt {
            iUsageType: self.iUsageType,
            iPicWidth: self.iPicWidth,
            iPicHeight: self.iPicHeight,
            iTargetBitrate: self.iTargetBitrate,
            iRCMode: self.iRCMode,
            fMaxFrameRate: self.fMaxFrameRate,
            iTemporalLayerNum: self.iTemporalLayerNum,
            iSpatialLayerNum: self.iSpatialLayerNum,
            sSpatialLayers: self.sSpatialLayers,
            iComplexityMode: self.iComplexityMode as i32,
            uiIntraPeriod: self.uiIntraPeriod,
            iNumRefFrame: self.iNumRefFrame,
            eSpsPpsIdStrategy: self.eSpsPpsIdStrategy as i32,
            bPrefixNalAddingCtrl: self.bPrefixNalAddingCtrl,
            bEnableSSEI: self.bEnableSSEI,
            bSimulcastAVC: self.bSimulcastAVC,
            iPaddingFlag: self.iPaddingFlag,
            iEntropyCodingModeFlag: self.iEntropyCodingModeFlag,
            bEnableFrameCroppingFlag: self.bEnableFrameCroppingFlag,
            iLoopFilterDisableIdc: self.iLoopFilterDisableIdc,
            iLoopFilterAlphaC0Offset: self.iLoopFilterAlphaC0Offset,
            iLoopFilterBetaOffset: self.iLoopFilterBetaOffset,
            bEnableDenoise: self.bEnableDenoise,
            bEnableSceneChangeDetect: self.bEnableSceneChangeDetect,
            bEnableBackgroundDetection: self.bEnableBackgroundDetection,
            bEnableAdaptiveQuant: self.bEnableAdaptiveQuant,
            bEnableFrameSkip: self.bEnableFrameSkip,
            bEnableLongTermReference: self.bEnableLongTermReference,
            iLtrMarkPeriod: self.iLtrMarkPeriod as i32,
            iMultipleThreadIdc: self.iMultipleThreadIdc,
            bUseLoadBalancing: self.bUseLoadBalancing,
            iMaxBitrate: self.iMaxBitrate,
            iMinQp: self.iMinQp,
            iMaxQp: self.iMaxQp,
            uiMaxNalSize: self.uiMaxNalSize,
            bIsLosslessLink: self.bIsLosslessLink,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsSvcRc {
    pub iAverageFrameQp: u32,
    pub fLatestFrameRate: f32,
    pub iActualBitRate: u32,
}

pub use crate::encoder::encoder_context::SLTRState;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagVideoEncoderStatistics {
    pub uiWidth: u32,
    pub uiHeight: u32,
    pub fAverageFrameSpeedInMs: f32,
    pub fAverageFrameRate: f32,
    pub fLatestFrameRate: f32,
    pub uiBitRate: u32,
    pub uiAverageFrameQP: u32,
    pub uiInputFrameCount: u32,
    pub uiSkippedFrameCount: u32,
    pub uiResolutionChangeTimes: u32,
    pub uiIDRReqNum: u32,
    pub uiIDRSentNum: u32,
    pub uiLTRSentNum: u32,
    pub iStatisticsTs: i64,
    pub iTotalEncodedBytes: u64,
    pub iLastStatisticsBytes: u64,
    pub iLastStatisticsFrameCount: u32,
}

impl Default for TagVideoEncoderStatistics {
    fn default() -> Self {
        Self {
            uiWidth: 0,
            uiHeight: 0,
            fAverageFrameSpeedInMs: 0.0,
            fAverageFrameRate: 0.0,
            fLatestFrameRate: 0.0,
            uiBitRate: 0,
            uiAverageFrameQP: 0,
            uiInputFrameCount: 0,
            uiSkippedFrameCount: 0,
            uiResolutionChangeTimes: 0,
            uiIDRReqNum: 0,
            uiIDRSentNum: 0,
            uiLTRSentNum: 0,
            iStatisticsTs: 0,
            iTotalEncodedBytes: 0,
            iLastStatisticsBytes: 0,
            iLastStatisticsFrameCount: 0,
        }
    }
}

pub use crate::encoder::encoder_context::sWelsEncCtx;

// Core encoder functions implementations / fallbacks
pub unsafe fn WelsWriteSpsSyntax(
    pSps: *const crate::encoder::param_svc::SWelsSPS,
    pBs: *mut crate::encoder::svc_encode_slice::SBitStringAux,
    _bBaseLayer: bool,
) -> i32 {
    if pSps.is_null() || pBs.is_null() {
        return 1;
    }
    let sps = &*pSps;
    crate::encoder::svc_encode_slice::BsWriteBits(pBs, 8, sps.uiProfileIdc as u32);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bConstraintSet0Flag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bConstraintSet1Flag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bConstraintSet2Flag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bConstraintSet3Flag { 1 } else { 0 });
    if sps.uiProfileIdc == 77 || sps.uiProfileIdc == 88 || sps.uiProfileIdc == 100 {
        crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 1);
        crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 1);
        crate::encoder::svc_encode_slice::BsWriteBits(pBs, 2, 0);
    } else {
        crate::encoder::svc_encode_slice::BsWriteBits(pBs, 4, 0);
    }
    crate::encoder::svc_encode_slice::BsWriteBits(pBs, 8, sps.iLevelIdc as u32);
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.uiSpsId);

    if sps.uiProfileIdc == 83
        || sps.uiProfileIdc == 86
        || sps.uiProfileIdc == 100
        || sps.uiProfileIdc == 110
        || sps.uiProfileIdc == 122
        || sps.uiProfileIdc == 244
        || sps.uiProfileIdc == 44
    {
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, 1);
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, 0);
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, 0);
        crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
        crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    }

    crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.uiLog2MaxFrameNum.saturating_sub(4));
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.uiPocType);
    if sps.uiPocType == 0 {
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, (sps.iLog2MaxPocLsb - 4).max(0) as u32);
    }

    crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.iNumRefFrames as u32);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bGapsInFrameNumValueAllowedFlag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, (sps.iMbWidth - 1).max(0) as u32);
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, (sps.iMbHeight - 1).max(0) as u32);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 1);

    let d8x8 = if sps.iLevelIdc >= 30 { 1 } else { 0 };
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, d8x8);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if sps.bFrameCroppingFlag { 1 } else { 0 });
    if sps.bFrameCroppingFlag {
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.sFrameCrop.iCropLeft as u32);
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.sFrameCrop.iCropRight as u32);
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.sFrameCrop.iCropTop as u32);
        crate::encoder::svc_encode_slice::BsWriteUE(pBs, sps.sFrameCrop.iCropBottom as u32);
    }

    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    0
}

pub unsafe fn WelsWriteSpsNal(
    pSps: *const crate::encoder::param_svc::SWelsSPS,
    pBs: *mut crate::encoder::svc_encode_slice::SBitStringAux,
) -> i32 {
    WelsWriteSpsSyntax(pSps, pBs, true);
    crate::encoder::nal_encap::BsRbspTrailingBits(pBs);
    0
}

pub unsafe fn WelsWritePpsSyntax(
    pPps: *const crate::encoder::param_svc::SWelsPPS,
    pBs: *mut crate::encoder::svc_encode_slice::SBitStringAux,
) -> i32 {
    if pPps.is_null() || pBs.is_null() {
        return 1;
    }
    let pps = &*pPps;
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, pps.iPpsId);
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, pps.iSpsId);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if pps.bEntropyCodingModeFlag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, pps.uiNumSliceGroups.saturating_sub(1));
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, 0);
    crate::encoder::svc_encode_slice::BsWriteUE(pBs, 0);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    crate::encoder::svc_encode_slice::BsWriteBits(pBs, 2, 0);
    crate::encoder::svc_encode_slice::BsWriteSE(pBs, pps.iPicInitQp as i32 - 26);
    crate::encoder::svc_encode_slice::BsWriteSE(pBs, pps.iPicInitQs as i32 - 26);
    crate::encoder::svc_encode_slice::BsWriteSE(pBs, pps.uiChromaQpIndexOffset as i32);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, if pps.bDeblockingFilterControlPresentFlag { 1 } else { 0 });
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    crate::encoder::svc_encode_slice::BsWriteOneBit(pBs, 0);
    crate::encoder::nal_encap::BsRbspTrailingBits(pBs);
    0
}

pub unsafe fn WelsWriteOneSPS(pCtx: *mut sWelsEncCtx, kiSpsIdx: usize, pNalSize: *mut i32) -> i32 {
    let pOut = (*pCtx).pOut;
    if pOut.is_null() {
        return 1;
    }
    let iNal = (*pOut).iNalIndex;
    crate::encoder::nal_encap::WelsLoadNal(pOut, crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SPS as i32, 3);

    let pSps = if !(*pCtx).pSpsArray.is_null() {
        (*pCtx).pSpsArray.add(kiSpsIdx) as *const crate::encoder::param_svc::SWelsSPS
    } else {
        return 1;
    };
    WelsWriteSpsNal(pSps, &mut (*pOut).sBsWrite);
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let pRawNal = (*pOut).sNalList.add(iNal as usize);
    let avail_len = (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer;
    let pDst = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void;

    let ret = crate::encoder::nal_encap::WelsEncodeNal(pRawNal, null_mut(), avail_len, pDst, pNalSize);
    if ret == ENC_RETURN_SUCCESS {
        (*pCtx).iPosBsBuffer += *pNalSize;
    }
    ret
}

pub unsafe fn WelsWriteOnePPS(pCtx: *mut sWelsEncCtx, kiPpsIdx: usize, pNalSize: *mut i32) -> i32 {
    let pOut = (*pCtx).pOut;
    if pOut.is_null() {
        return 1;
    }
    let iNal = (*pOut).iNalIndex;
    crate::encoder::nal_encap::WelsLoadNal(pOut, crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_PPS as i32, 3);

    let pPps = if !(*pCtx).pPPSArray.is_null() {
        (*pCtx).pPPSArray.add(kiPpsIdx) as *const crate::encoder::param_svc::SWelsPPS
    } else {
        return 1;
    };
    WelsWritePpsSyntax(pPps, &mut (*pOut).sBsWrite);
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let pRawNal = (*pOut).sNalList.add(iNal as usize);
    let avail_len = (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer;
    let pDst = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void;

    let ret = crate::encoder::nal_encap::WelsEncodeNal(pRawNal, null_mut(), avail_len, pDst, pNalSize);
    if ret == ENC_RETURN_SUCCESS {
        (*pCtx).iPosBsBuffer += *pNalSize;
    }
    ret
}

pub unsafe fn WelsWriteParameterSets(
    pCtx: *mut sWelsEncCtx,
    pNalLen: *mut i32,
    pNumNal: *mut i32,
    pTotalLength: *mut i32,
) -> i32 {
    let mut iSize = 0i32;
    let mut iCountNal = 0i32;
    let mut iNalLength = 0i32;

    let sps_count = if (*pCtx).iSpsNum > 0 { (*pCtx).iSpsNum as usize } else { 1 };
    for iIdx in 0..sps_count {
        if WelsWriteOneSPS(pCtx, iIdx, &mut iNalLength) == ENC_RETURN_SUCCESS {
            *pNalLen.add(iCountNal as usize) = iNalLength;
            iSize += iNalLength;
            iCountNal += 1;
        }
    }

    let pps_count = if (*pCtx).iPpsNum > 0 { (*pCtx).iPpsNum as usize } else { 1 };
    for iIdx in 0..pps_count {
        if WelsWriteOnePPS(pCtx, iIdx, &mut iNalLength) == ENC_RETURN_SUCCESS {
            *pNalLen.add(iCountNal as usize) = iNalLength;
            iSize += iNalLength;
            iCountNal += 1;
        }
    }

    *pNumNal = iCountNal;
    *pTotalLength = iSize;
    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsInitEncoderExtRust(
    ppCtx: *mut *mut sWelsEncCtx,
    pCfg: *const SWelsSvcCodingParam,
    _pLogCtx: *mut SLogContext,
    _pReserved: *mut c_void,
) -> i32 {
    if ppCtx.is_null() || pCfg.is_null() {
        return 1;
    }
    let mut ctx = Box::new(sWelsEncCtx::default());
    let cfg_clone = Box::new(*pCfg);
    ctx.pSvcParam = Box::into_raw(cfg_clone);
    let ltr = Box::new(SLTRState::default());
    ctx.pLtr = Box::into_raw(ltr);

    let buf_size = 1024 * 1024 * 4;
    ctx.iFrameBsSize = buf_size as i32;
    ctx.pFrameBs = vec![0u8; buf_size].leak().as_mut_ptr();

    let mut out = Box::new(crate::encoder::encoder_context::SWelsEncoderOutput::default());
    out.pBsBuffer = vec![0u8; buf_size].leak().as_mut_ptr();
    let max_nals = 64usize;
    let nal_list = vec![crate::encoder::svc_encode_slice::SWelsNalRaw::default(); max_nals].leak().as_mut_ptr();
    let nal_len = vec![0i32; max_nals].leak().as_mut_ptr();
    out.sNalList = nal_list;
    out.pNalLen = nal_len;
    out.iCountNals = max_nals as i32;

    let out_ptr = Box::into_raw(out);
    ctx.pOut = out_ptr;

    let func_list = Box::new(crate::encoder::encoder_context::SWelsFuncPtrList::default());
    ctx.pFuncList = Box::into_raw(func_list);

    let ctx_ptr = Box::into_raw(ctx);
    let _ = crate::encoder::encoder_context::InitFunctionPointers(
        ctx_ptr,
        (*ctx_ptr).pSvcParam,
        0,
    );

    *ppCtx = ctx_ptr;
    0
}

pub unsafe fn WelsUninitEncoderExtRust(ppCtx: *mut *mut sWelsEncCtx) {
    if ppCtx.is_null() || (*ppCtx).is_null() {
        return;
    }
    let ctx = Box::from_raw(*ppCtx);
    if !ctx.pSvcParam.is_null() {
        let _ = Box::from_raw(ctx.pSvcParam);
    }
    if !ctx.pLtr.is_null() {
        let _ = Box::from_raw(ctx.pLtr);
    }
    if !ctx.pOut.is_null() {
        let out = Box::from_raw(ctx.pOut);
        if !out.pBsBuffer.is_null() {
            let _ = Vec::from_raw_parts(out.pBsBuffer, out.uiSize as usize, out.uiSize as usize);
        }
    }
    if !ctx.pFrameBs.is_null() {
        let _ = Vec::from_raw_parts(ctx.pFrameBs, ctx.iFrameBsSize as usize, ctx.iFrameBsSize as usize);
    }
    if !ctx.pSpsArray.is_null() {
        let _ = Box::from_raw(ctx.pSpsArray as *mut crate::encoder::param_svc::SWelsSPS);
    }
    if !ctx.pPPSArray.is_null() {
        let _ = Box::from_raw(ctx.pPPSArray as *mut crate::encoder::param_svc::SWelsPPS);
    }
    *ppCtx = null_mut();
}

pub unsafe fn WelsEncoderEncodeExtRust(
    pCtx: *mut sWelsEncCtx,
    pFbi: *mut SFrameBSInfo,
    pSrcPic: *const SSourcePicture,
) -> i32 {
    if pCtx.is_null() || pFbi.is_null() || pSrcPic.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }

    let pLayerBsInfo = &mut (*pFbi).sLayerInfo[0];
    (*pFbi).eFrameType = EVideoFrameType::VideoFrameTypeIDR as i32;
    (*pFbi).iLayerNum = 0;
    (*pFbi).uiTimeStamp = (*pSrcPic).uiTimeStamp;

    pLayerBsInfo.pBsBuf = (*pCtx).pFrameBs;
    pLayerBsInfo.pNalLengthInByte = (*(*pCtx).pOut).pNalLen;
    crate::encoder::encoder_context::InitBits(&mut (*(*pCtx).pOut).sBsWrite, (*(*pCtx).pOut).pBsBuffer, (*(*pCtx).pOut).uiSize as u32);
    (*pCtx).iPosBsBuffer = 0;

    let mut iCountNal = 0;
    let mut iTotalLength = 0;
    let ret = WelsWriteParameterSets(pCtx, pLayerBsInfo.pNalLengthInByte, &mut iCountNal, &mut iTotalLength);
    if ret != ENC_RETURN_SUCCESS {
        return ret;
    }

    pLayerBsInfo.uiSpatialId = 0;
    pLayerBsInfo.uiTemporalId = 0;
    pLayerBsInfo.uiQualityId = 0;
    pLayerBsInfo.uiLayerType = NON_VIDEO_CODING_LAYER;
    pLayerBsInfo.iNalCount = iCountNal;
    (*pFbi).iLayerNum = 1;

    let eNalType = crate::encoder::encoder_context::EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
    let eNalRefIdc = crate::encoder::encoder_context::EWelsNalRefIdc::NRI_PRI_HIGHEST;
    (*pCtx).eNalType = eNalType;
    (*pCtx).eNalPriority = eNalRefIdc;

    if !(*pCtx).pCurDqLayer.is_null() {
        crate::encoder::nal_encap::WelsLoadNal((*pCtx).pOut, eNalType as i32, eNalRefIdc as i32);
        let pCurSlice = (*(*pCtx).pCurDqLayer).sSliceBufferInfo[0].pSliceBuffer;
        crate::encoder::svc_encode_slice::SetSliceBoundaryInfo((*pCtx).pCurDqLayer, pCurSlice, 0);

        let slice_ret = crate::encoder::svc_encode_slice::WelsCodeOneSlice(pCtx, pCurSlice, eNalType as i32);
        if slice_ret == ENC_RETURN_SUCCESS {
            crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);
            let mut iSliceSize = 0i32;
            let pRawNal = (*(*pCtx).pOut).sNalList.add((*(*pCtx).pOut).iNalIndex as usize - 1);
            let pNalHeaderExt = null_mut();
            let avail_len = (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer;
            let pDst = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void;

            let enc_nal_ret = crate::encoder::nal_encap::WelsEncodeNal(pRawNal, pNalHeaderExt, avail_len, pDst, &mut iSliceSize);
            if enc_nal_ret == ENC_RETURN_SUCCESS && iSliceSize > 0 {
                let vcl_layer = &mut (*pFbi).sLayerInfo[1];
                vcl_layer.pBsBuf = (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize);
                vcl_layer.pNalLengthInByte = (*(*pCtx).pOut).pNalLen.add(iCountNal as usize);
                *vcl_layer.pNalLengthInByte = iSliceSize;
                vcl_layer.uiSpatialId = 0;
                vcl_layer.uiTemporalId = 0;
                vcl_layer.uiQualityId = 0;
                vcl_layer.uiLayerType = VIDEO_CODING_LAYER;
                vcl_layer.iNalCount = 1;
                (*pFbi).iLayerNum = 2;
                (*pCtx).iPosBsBuffer += iSliceSize;
            }
        }
    }

    crate::encoder::deblocking::PerformDeblockingFilter(pCtx);

    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsEncoderEncodeParameterSetsRust(
    pCtx: *mut sWelsEncCtx,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    if pCtx.is_null() || pBsInfo.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pLayerBsInfo = &mut (*pBsInfo).sLayerInfo[0];
    pLayerBsInfo.pBsBuf = (*pCtx).pFrameBs;
    pLayerBsInfo.pNalLengthInByte = (*(*pCtx).pOut).pNalLen;
    crate::encoder::encoder_context::InitBits(&mut (*(*pCtx).pOut).sBsWrite, (*(*pCtx).pOut).pBsBuffer, (*(*pCtx).pOut).uiSize as u32);
    (*pCtx).iPosBsBuffer = 0;

    let mut iCountNal = 0;
    let mut iTotalLength = 0;
    let ret = WelsWriteParameterSets(pCtx, pLayerBsInfo.pNalLengthInByte, &mut iCountNal, &mut iTotalLength);
    if ret != ENC_RETURN_SUCCESS {
        return ret;
    }

    pLayerBsInfo.uiSpatialId = 0;
    pLayerBsInfo.uiTemporalId = 0;
    pLayerBsInfo.uiQualityId = 0;
    pLayerBsInfo.uiLayerType = NON_VIDEO_CODING_LAYER;
    pLayerBsInfo.iNalCount = iCountNal;
    (*pBsInfo).iLayerNum = 1;
    (*pBsInfo).eFrameType = EVideoFrameType::VideoFrameTypeInvalid as i32;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ForceCodingIDR(pCtx: *mut sWelsEncCtx, _iLayerId: i32) -> i32 {
    if pCtx.is_null() {
        return 1;
    }
    0
}

pub unsafe fn WelsEncoderParamAdjust(
    ppCtx: *mut *mut sWelsEncCtx,
    pCfg: *const SWelsSvcCodingParam,
) -> i32 {
    if ppCtx.is_null() || (*ppCtx).is_null() || pCfg.is_null() {
        return 1;
    }
    let ctx = &mut **ppCtx;
    if !ctx.pSvcParam.is_null() {
        *ctx.pSvcParam = *pCfg;
    }
    0
}

pub unsafe fn WelsEncoderApplyFrameRate(pCfg: *mut SWelsSvcCodingParam) {
    if !pCfg.is_null() {
        (*pCfg).fMaxFrameRate = WELS_CLIP3((*pCfg).fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);
    }
}

pub unsafe fn WelsEncoderApplyBitRate(
    _pLogCtx: *mut SLogContext,
    pCfg: *mut SWelsSvcCodingParam,
    iLayer: i32,
) -> i32 {
    if pCfg.is_null() {
        return 1;
    }
    0
}

pub unsafe fn WelsRcInitFuncPointers(pCtx: *mut sWelsEncCtx, iRCMode: RCMode) {
    if !pCtx.is_null() && !(*pCtx).pSvcParam.is_null() {
        (*(*pCtx).pSvcParam).iRCMode = iRCMode;
    }
}

pub unsafe fn FilterLTRRecoveryRequest(pCtx: *mut sWelsEncCtx, _pReq: *mut SLTRRecoverRequest) {
    let _ = pCtx;
}

pub unsafe fn FilterLTRMarkingFeedback(pCtx: *mut sWelsEncCtx, _pFb: *mut SLTRMarkingFeedback) {
    let _ = pCtx;
}

pub unsafe fn WelsEncoderApplyLTR(
    _pLogCtx: *mut SLogContext,
    ppCtx: *mut *mut sWelsEncCtx,
    pLTRValue: *mut SLTRConfig,
) -> i32 {
    if ppCtx.is_null() || (*ppCtx).is_null() || pLTRValue.is_null() {
        return 1;
    }
    let ctx = &mut **ppCtx;
    if !ctx.pSvcParam.is_null() {
        (*ctx.pSvcParam).bEnableLongTermReference = (*pLTRValue).bEnableLongTermReference;
        (*ctx.pSvcParam).iLTRRefNum = (*pLTRValue).iLTRRefNum;
    }
    0
}

pub unsafe fn CheckProfileSetting(
    _pLogCtx: *mut SLogContext,
    pCfg: *mut SWelsSvcCodingParam,
    iLayer: i32,
    uiProfileIdc: i32,
) {
    if !pCfg.is_null() && iLayer >= 0 && iLayer < MAX_DEPENDENCY_LAYER {
        (*pCfg).sSpatialLayers[iLayer as usize].uiProfileIdc = uiProfileIdc;
    }
}

pub unsafe fn CheckLevelSetting(
    _pLogCtx: *mut SLogContext,
    pCfg: *mut SWelsSvcCodingParam,
    iLayer: i32,
    uiLevelIdc: i32,
) {
    if !pCfg.is_null() && iLayer >= 0 && iLayer < MAX_DEPENDENCY_LAYER {
        (*pCfg).sSpatialLayers[iLayer as usize].uiLevelIdc = uiLevelIdc;
    }
}

pub unsafe fn CheckReferenceNumSetting(
    _pLogCtx: *mut SLogContext,
    pCfg: *mut SWelsSvcCodingParam,
    iValue: i32,
) {
    if !pCfg.is_null() {
        (*pCfg).iNumRefFrame = iValue;
    }
}

pub unsafe fn WelsEncoderApplyBitVaryRang(
    _pLogCtx: *mut SLogContext,
    pCfg: *mut SWelsSvcCodingParam,
    iBitsVaryPercentage: i32,
) {
    if !pCfg.is_null() {
        (*pCfg).iBitsVaryPercentage = WELS_CLIP3(iBitsVaryPercentage, 0, 100);
    }
}

/// CWelsH264SVCEncoder class implementation
#[repr(C)]
pub struct CWelsH264SVCEncoder {
    pub vptr: *const ISVCEncoderVtbl,
    pub m_pEncContext: *mut sWelsEncCtx,
    pub m_pWelsTrace: *mut welsCodecTrace,
    pub m_iMaxPicWidth: i32,
    pub m_iMaxPicHeight: i32,
    pub m_iCspInternal: i32,
    pub m_bInitialFlag: bool,
}

impl Default for CWelsH264SVCEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl CWelsH264SVCEncoder {
    pub fn new() -> Self {
        let trace = Box::into_raw(Box::new(welsCodecTrace::new()));
        let mut encoder = Self {
            vptr: &G_ISVCENCODER_VTBL,
            m_pEncContext: null_mut(),
            m_pWelsTrace: trace,
            m_iMaxPicWidth: 0,
            m_iMaxPicHeight: 0,
            m_iCspInternal: 0,
            m_bInitialFlag: false,
        };
        encoder.InitEncoder();
        encoder
    }

    pub fn InitEncoder(&mut self) {
        if !self.m_pWelsTrace.is_null() {
            unsafe {
                (*self.m_pWelsTrace).SetCodecInstance(self as *mut Self as *mut c_void);
            }
        }
    }

    pub fn GetDefaultParams(&mut self, argv: *mut SEncParamExt) -> i32 {
        if argv.is_null() {
            return cmInitParaError;
        }
        unsafe {
            SWelsSvcCodingParam::FillDefault(&mut *argv);
        }
        cmResultSuccess
    }

    pub fn Initialize(&mut self, argv: *const SEncParamBase) -> i32 {
        if self.m_pWelsTrace.is_null() {
            return cmMallocMemeError;
        }
        if argv.is_null() {
            return cmInitParaError;
        }
        let mut sConfig = SWelsSvcCodingParam::default();
        unsafe {
            if sConfig.ParamBaseTranscode(*argv) != 0 {
                self.TraceParamInfo(&sConfig.to_param_ext());
                self.Uninitialize();
                return cmInitParaError;
            }
        }
        self.InitializeInternal(&mut sConfig)
    }

    pub fn InitializeExt(&mut self, argv: *const SEncParamExt) -> i32 {
        if self.m_pWelsTrace.is_null() {
            return cmMallocMemeError;
        }
        if argv.is_null() {
            return cmInitParaError;
        }
        let mut sConfig = SWelsSvcCodingParam::default();
        unsafe {
            if sConfig.ParamTranscode(*argv) != 0 {
                self.TraceParamInfo(argv);
                self.Uninitialize();
                return cmInitParaError;
            }
        }
        self.InitializeInternal(&mut sConfig)
    }

    pub fn InitializeInternal(&mut self, pCfg: *mut SWelsSvcCodingParam) -> i32 {
        if pCfg.is_null() {
            return cmInitParaError;
        }

        if self.m_bInitialFlag {
            self.Uninitialize();
        }

        unsafe {
            let iNumOfLayers = (*pCfg).iSpatialLayerNum;
            if iNumOfLayers < 1 || iNumOfLayers > MAX_DEPENDENCY_LAYER {
                self.Uninitialize();
                return cmInitParaError;
            }
            if (*pCfg).iTemporalLayerNum < 1 {
                (*pCfg).iTemporalLayerNum = 1;
            }
            if (*pCfg).iTemporalLayerNum > MAX_TEMPORAL_LEVEL {
                self.Uninitialize();
                return cmInitParaError;
            }

            if (*pCfg).uiGopSize < 1 || (*pCfg).uiGopSize > MAX_GOP_SIZE {
                self.Uninitialize();
                return cmInitParaError;
            }

            if !WELS_POWER2_IF((*pCfg).uiGopSize) {
                self.Uninitialize();
                return cmInitParaError;
            }

            if (*pCfg).uiIntraPeriod != 0 && (*pCfg).uiIntraPeriod < (*pCfg).uiGopSize {
                self.Uninitialize();
                return cmInitParaError;
            }

            if (*pCfg).uiIntraPeriod != 0 && ((*pCfg).uiIntraPeriod & ((*pCfg).uiGopSize - 1)) != 0
            {
                self.Uninitialize();
                return cmInitParaError;
            }

            if (*pCfg).iUsageType == EUsageType::ScreenContentRealTime {
                if (*pCfg).bEnableLongTermReference {
                    (*pCfg).iLTRRefNum = LONG_TERM_REF_NUM_SCREEN;
                    if (*pCfg).iNumRefFrame == AUTO_REF_PIC_COUNT {
                        (*pCfg).iNumRefFrame =
                            WELS_MAX(1, WELS_LOG2((*pCfg).uiGopSize)) + (*pCfg).iLTRRefNum;
                    }
                } else {
                    (*pCfg).iLTRRefNum = 0;
                    if (*pCfg).iNumRefFrame == AUTO_REF_PIC_COUNT {
                        (*pCfg).iNumRefFrame = WELS_MAX(1, ((*pCfg).uiGopSize >> 1) as i32);
                    }
                }
            } else {
                (*pCfg).iLTRRefNum = if (*pCfg).bEnableLongTermReference {
                    LONG_TERM_REF_NUM
                } else {
                    0
                };
                if (*pCfg).iNumRefFrame == AUTO_REF_PIC_COUNT {
                    let ref_calc = if ((*pCfg).uiGopSize >> 1) > 1 {
                        ((*pCfg).uiGopSize >> 1) as i32 + (*pCfg).iLTRRefNum
                    } else {
                        MIN_REF_PIC_COUNT + (*pCfg).iLTRRefNum
                    };
                    (*pCfg).iNumRefFrame = WELS_CLIP3(
                        ref_calc,
                        MIN_REF_PIC_COUNT,
                        MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
                    );
                }
            }

            if (*pCfg).iLtrMarkPeriod == 0 {
                (*pCfg).iLtrMarkPeriod = 30;
            }

            let kiDecStages = WELS_LOG2((*pCfg).uiGopSize);
            (*pCfg).iTemporalLayerNum = 1 + kiDecStages;
            (*pCfg).iLoopFilterAlphaC0Offset =
                WELS_CLIP3((*pCfg).iLoopFilterAlphaC0Offset, -6, 6);
            (*pCfg).iLoopFilterBetaOffset = WELS_CLIP3((*pCfg).iLoopFilterBetaOffset, -6, 6);

            self.m_iMaxPicWidth = (*pCfg).iPicWidth;
            self.m_iMaxPicHeight = (*pCfg).iPicHeight;

            self.TraceParamInfo(&mut (*pCfg).to_param_ext());
            let log_ctx = if !self.m_pWelsTrace.is_null() {
                &mut (*self.m_pWelsTrace).m_sLogCtx
            } else {
                null_mut()
            };

            if WelsInitEncoderExtRust(&mut self.m_pEncContext, pCfg, log_ctx, null_mut()) != 0 {
                self.Uninitialize();
                return cmInitParaError;
            }

            self.m_bInitialFlag = true;
        }

        cmResultSuccess
    }

    pub fn Uninitialize(&mut self) -> i32 {
        if !self.m_bInitialFlag {
            return 0;
        }
        unsafe {
            if !self.m_pEncContext.is_null() {
                WelsUninitEncoderExtRust(&mut self.m_pEncContext);
                self.m_pEncContext = null_mut();
            }
        }
        self.m_bInitialFlag = false;
        0
    }

    pub fn EncodeFrame(
        &mut self,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32 {
        if kpSrcPic.is_null() || !self.m_bInitialFlag || pBsInfo.is_null() {
            return cmInitParaError;
        }
        unsafe {
            if (*kpSrcPic).iColorFormat != VideoFormat::VideoFormatI420 as i32 {
                return cmInitParaError;
            }
        }
        let kiEncoderReturn = self.EncodeFrameInternal(kpSrcPic, pBsInfo);
        if kiEncoderReturn != cmResultSuccess {
            return kiEncoderReturn;
        }
        kiEncoderReturn
    }

    pub fn EncodeFrameInternal(
        &mut self,
        pSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32 {
        unsafe {
            if (*pSrcPic).iPicWidth < 16 || (*pSrcPic).iPicHeight < 16 {
                return cmUnsupportedData;
            }

            let kiBeforeFrameUs = WelsTime();
            let kiEncoderReturn = WelsEncoderEncodeExtRust(self.m_pEncContext, pBsInfo, pSrcPic);
            let kiCurrentFrameMs = (WelsTime() - kiBeforeFrameUs) / 1000;

            if kiEncoderReturn == ENC_RETURN_MEMALLOCERR
                || kiEncoderReturn == ENC_RETURN_MEMOVERFLOWFOUND
                || kiEncoderReturn == ENC_RETURN_VLCOVERFLOWFOUND
            {
                WelsUninitEncoderExtRust(&mut self.m_pEncContext);
                return cmMallocMemeError;
            } else if kiEncoderReturn == ENC_RETURN_INVALIDINPUT {
                return cmUnsupportedData;
            } else if kiEncoderReturn != ENC_RETURN_SUCCESS
                && kiEncoderReturn == ENC_RETURN_CORRECTED
            {
                return cmUnknownReason;
            }

            self.UpdateStatistics(pBsInfo, kiCurrentFrameMs);
        }

        cmResultSuccess
    }

    pub fn EncodeParameterSets(&mut self, pBsInfo: *mut SFrameBSInfo) -> i32 {
        if self.m_pEncContext.is_null() || !self.m_bInitialFlag || pBsInfo.is_null() {
            return cmInitParaError;
        }
        unsafe { WelsEncoderEncodeParameterSetsRust(self.m_pEncContext, pBsInfo) }
    }

    pub fn ForceIntraFrame(&mut self, bIDR: bool, iLayerId: i32) -> i32 {
        if bIDR {
            if self.m_pEncContext.is_null() || !self.m_bInitialFlag {
                return 1;
            }
            unsafe {
                ForceCodingIDR(self.m_pEncContext, iLayerId);
            }
        }
        0
    }

    pub fn TraceParamInfo(&mut self, _pParam: *const SEncParamExt) {}

    pub fn LogStatistics(&mut self, _kiCurrentFrameTs: i64, _iMaxDid: i32) {}

    pub fn UpdateStatistics(&mut self, pBsInfo: *mut SFrameBSInfo, kiCurrentFrameMs: i64) {
        unsafe {
            if self.m_pEncContext.is_null() || (*self.m_pEncContext).pSvcParam.is_null() || pBsInfo.is_null() {
                return;
            }
            let kiCurrentFrameTs = (*pBsInfo).uiTimeStamp;
            (*self.m_pEncContext).uiLastTimestamp = kiCurrentFrameTs;
            let kiTimeDiff = kiCurrentFrameTs - (*self.m_pEncContext).iLastStatisticsLogTs;

            let iMaxDid = (*(*self.m_pEncContext).pSvcParam).iSpatialLayerNum - 1;
            for iDid in 0..=iMaxDid {
                let mut eFrameType = EVideoFrameType::VideoFrameTypeSkip as i32;
                let mut kiCurrentFrameSize = 0;
                for iLayerNum in 0..((*pBsInfo).iLayerNum as usize).min(MAX_LAYER_NUM_OF_FRAME as usize) {
                    let pLayerInfo = &(*pBsInfo).sLayerInfo[iLayerNum];
                    if pLayerInfo.uiLayerType == VIDEO_CODING_LAYER
                        && pLayerInfo.uiSpatialId as i32 == iDid
                    {
                        eFrameType = (*pBsInfo).eFrameType;
                        if !pLayerInfo.pNalLengthInByte.is_null() {
                            for iNalIdx in 0..pLayerInfo.iNalCount {
                                kiCurrentFrameSize += *pLayerInfo.pNalLengthInByte.offset(iNalIdx as isize);
                            }
                        }
                    }
                }

                let pStatistics =
                    &mut (*self.m_pEncContext).sEncoderStatistics[iDid as usize];
                let pSpatialLayerInternalParam =
                    &(*(*self.m_pEncContext).pSvcParam).sDependencyLayers[iDid as usize];

                if pStatistics.uiWidth != 0
                    && pStatistics.uiHeight != 0
                    && (pStatistics.uiWidth != pSpatialLayerInternalParam.iActualWidth as u32
                        || pStatistics.uiHeight != pSpatialLayerInternalParam.iActualHeight as u32)
                {
                    pStatistics.uiResolutionChangeTimes += 1;
                }
                pStatistics.uiWidth = pSpatialLayerInternalParam.iActualWidth as u32;
                pStatistics.uiHeight = pSpatialLayerInternalParam.iActualHeight as u32;

                let kbCurrentFrameSkipped =
                    eFrameType == EVideoFrameType::VideoFrameTypeSkip as i32;
                pStatistics.uiInputFrameCount += 1;
                if kbCurrentFrameSkipped {
                    pStatistics.uiSkippedFrameCount += 1;
                }
                let iProcessedFrameCount =
                    (pStatistics.uiInputFrameCount - pStatistics.uiSkippedFrameCount) as i32;
                if !kbCurrentFrameSkipped && iProcessedFrameCount != 0 {
                    pStatistics.fAverageFrameSpeedInMs += (kiCurrentFrameMs as f32
                        - pStatistics.fAverageFrameSpeedInMs)
                        / (iProcessedFrameCount as f32);
                }

                if (*self.m_pEncContext).uiStartTimestamp != 0 {
                    if kiCurrentFrameTs > (*self.m_pEncContext).uiStartTimestamp + 800 {
                        pStatistics.fAverageFrameRate = (pStatistics.uiInputFrameCount as f32
                            * 1000.0)
                            / ((kiCurrentFrameTs - (*self.m_pEncContext).uiStartTimestamp) as f32);
                    }
                } else {
                    (*self.m_pEncContext).uiStartTimestamp = kiCurrentFrameTs;
                }

                pStatistics.uiAverageFrameQP = if !(*self.m_pEncContext).pWelsSvcRc.is_null() {
                    (*(*self.m_pEncContext).pWelsSvcRc.add(iDid as usize)).iAverageFrameQp as u32
                } else {
                    26
                };

                if eFrameType == EVideoFrameType::VideoFrameTypeIDR as i32
                    || eFrameType == EVideoFrameType::VideoFrameTypeI as i32
                {
                    pStatistics.uiIDRSentNum += 1;
                }
                if !(*self.m_pEncContext).pLtr.is_null()
                    && (*(*self.m_pEncContext).pLtr).bLTRMarkingFlag
                {
                    pStatistics.uiLTRSentNum += 1;
                }

                pStatistics.iTotalEncodedBytes += kiCurrentFrameSize as u64;

                let kiDeltaFrames = (pStatistics.uiInputFrameCount
                    - pStatistics.iLastStatisticsFrameCount)
                    as i32;
                if kiDeltaFrames as f32
                    > (*(*self.m_pEncContext).pSvcParam).fMaxFrameRate * 2.0
                {
                    if kiTimeDiff >= (*self.m_pEncContext).iStatisticsLogInterval as i64 {
                        let fTimeDiffSec = kiTimeDiff as f32 / 1000.0;
                        if fTimeDiffSec > 0.0 {
                            pStatistics.fLatestFrameRate = (pStatistics.uiInputFrameCount
                                - pStatistics.iLastStatisticsFrameCount)
                                as f32
                                / fTimeDiffSec;
                            pStatistics.uiBitRate =
                                ((pStatistics.iTotalEncodedBytes as f32) * 8.0 / fTimeDiffSec)
                                    as u32;
                        }
                        pStatistics.iLastStatisticsBytes = pStatistics.iTotalEncodedBytes;
                        pStatistics.iLastStatisticsFrameCount = pStatistics.uiInputFrameCount;
                        (*self.m_pEncContext).iLastStatisticsLogTs = kiCurrentFrameTs;
                        self.LogStatistics(kiCurrentFrameTs, iMaxDid);
                        pStatistics.iTotalEncodedBytes = 0;
                    }
                }
            }
        }
    }

    pub fn SetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        if (self.m_pEncContext.is_null() || !self.m_bInitialFlag)
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_LEVEL
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_CALLBACK
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT
        {
            return cmInitExpected;
        }

        unsafe {
            match eOptionId {
                EncoderOption::ENCODER_OPTION_DATAFORMAT => {
                    let iValue = *(pOption as *const i32);
                    if iValue == 0 {
                        return cmInitParaError;
                    }
                    self.m_iCspInternal = iValue;
                }
                EncoderOption::ENCODER_OPTION_IDR_INTERVAL => {
                    let mut iValue = *(pOption as *const i32);
                    if iValue <= -1 {
                        iValue = 0;
                    }
                    if iValue == (*(*self.m_pEncContext).pSvcParam).uiIntraPeriod as i32 {
                        return cmResultSuccess;
                    }
                    (*(*self.m_pEncContext).pSvcParam).uiIntraPeriod = iValue as u32;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE => {
                    let sEncodingParam = *(pOption as *const SEncParamBase);
                    let mut sConfig = SWelsSvcCodingParam::default();
                    if sConfig.ParamBaseTranscode(sEncodingParam) != 0 {
                        return cmInitParaError;
                    }
                    let iTargetWidth = sConfig.iPicWidth;
                    let iTargetHeight = sConfig.iPicHeight;
                    if self.m_iMaxPicWidth != iTargetWidth
                        || self.m_iMaxPicHeight != iTargetHeight
                    {
                        self.m_iMaxPicWidth = iTargetWidth;
                        self.m_iMaxPicHeight = iTargetHeight;
                    }
                    if sConfig.DetermineTemporalSettings() != 0 {
                        return cmInitParaError;
                    }
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &sConfig) != 0 {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let sEncodingParam = *(pOption as *const SEncParamExt);
                    if sEncodingParam.iSpatialLayerNum < 1
                        || sEncodingParam.iSpatialLayerNum > MAX_DEPENDENCY_LAYER
                    {
                        return cmInitParaError;
                    }
                    let mut sConfig = SWelsSvcCodingParam::default();
                    if sConfig.ParamTranscode(sEncodingParam) != 0 {
                        return cmInitParaError;
                    }
                    if sConfig.DetermineTemporalSettings() != 0 {
                        return cmInitParaError;
                    }
                    let iTargetWidth = sConfig.iPicWidth;
                    let iTargetHeight = sConfig.iPicHeight;
                    if self.m_iMaxPicWidth != iTargetWidth
                        || self.m_iMaxPicHeight != iTargetHeight
                    {
                        self.m_iMaxPicWidth = iTargetWidth;
                        self.m_iMaxPicHeight = iTargetHeight;
                    }
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &sConfig) != 0 {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    let iValue = *(pOption as *const f32);
                    if iValue <= 0.0 {
                        return cmInitParaError;
                    }
                    (*(*self.m_pEncContext).pSvcParam).fMaxFrameRate =
                        WELS_CLIP3(iValue, MIN_FRAME_RATE, MAX_FRAME_RATE);
                    WelsEncoderApplyFrameRate((*self.m_pEncContext).pSvcParam);
                }
                EncoderOption::ENCODER_OPTION_BITRATE => {
                    let pInfo = &*(pOption as *const SBitrateInfo);
                    let mut iBitrate = pInfo.iBitrate;
                    if iBitrate <= 0 {
                        return cmInitParaError;
                    }
                    iBitrate = WELS_CLIP3(iBitrate, MIN_BIT_RATE, MAX_BIT_RATE);
                    match pInfo.iLayer {
                        SPATIAL_LAYER_ALL => {
                            (*(*self.m_pEncContext).pSvcParam).iTargetBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0..=SPATIAL_LAYER_3 => {
                            (*(*self.m_pEncContext).pSvcParam).sSpatialLayers[pInfo.iLayer as usize]
                                .iSpatialBitrate = iBitrate;
                        }
                        _ => return cmInitParaError,
                    }
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    if WelsEncoderApplyBitRate(log_ctx, (*self.m_pEncContext).pSvcParam, pInfo.iLayer)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let pInfo = &*(pOption as *const SBitrateInfo);
                    let mut iBitrate = pInfo.iBitrate;
                    if iBitrate <= 0 {
                        return cmInitParaError;
                    }
                    iBitrate = WELS_CLIP3(iBitrate, MIN_BIT_RATE, MAX_BIT_RATE);
                    match pInfo.iLayer {
                        SPATIAL_LAYER_ALL => {
                            (*(*self.m_pEncContext).pSvcParam).iMaxBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0..=SPATIAL_LAYER_3 => {
                            (*(*self.m_pEncContext).pSvcParam).sSpatialLayers[pInfo.iLayer as usize]
                                .iMaxSpatialBitrate = iBitrate;
                        }
                        _ => return cmInitParaError,
                    }
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    if WelsEncoderApplyBitRate(log_ctx, (*self.m_pEncContext).pSvcParam, pInfo.iLayer)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    let iValue = *(pOption as *const i32);
                    (*(*self.m_pEncContext).pSvcParam).iComplexityMode = match iValue {
                        0 => EComplexityMode::LowComplexity,
                        1 => EComplexityMode::MediumComplexity,
                        _ => EComplexityMode::HighComplexity,
                    };
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    // Get only option
                }
                EncoderOption::ENCODER_OPTION_TRACE_LEVEL => {
                    if !self.m_pWelsTrace.is_null() {
                        let level = *(pOption as *const u32);
                        (*self.m_pWelsTrace).SetTraceLevel(level);
                    }
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK => {
                    if !self.m_pWelsTrace.is_null() {
                        let callback = *(pOption as *const WelsTraceCallback);
                        (*self.m_pWelsTrace).SetTraceCallback(callback);
                    }
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT => {
                    if !self.m_pWelsTrace.is_null() {
                        let ctx = *(pOption as *const *mut c_void);
                        (*self.m_pWelsTrace).SetTraceCallbackContext(ctx);
                    }
                }
                _ => {}
            }
        }
        0
    }

    pub fn GetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        if self.m_pEncContext.is_null() || !self.m_bInitialFlag {
            return cmInitExpected;
        }

        unsafe {
            match eOptionId {
                EncoderOption::ENCODER_OPTION_DATAFORMAT => {
                    *(pOption as *mut i32) = self.m_iCspInternal;
                }
                EncoderOption::ENCODER_OPTION_IDR_INTERVAL => {
                    *(pOption as *mut i32) =
                        (*(*self.m_pEncContext).pSvcParam).uiIntraPeriod as i32;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let param_ext = (*(*self.m_pEncContext).pSvcParam).to_param_ext();
                    *(pOption as *mut SEncParamExt) = param_ext;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE => {
                    (*(*self.m_pEncContext).pSvcParam)
                        .GetBaseParams(&mut *(pOption as *mut SEncParamBase));
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    *(pOption as *mut f32) = (*(*self.m_pEncContext).pSvcParam).fMaxFrameRate;
                }
                EncoderOption::ENCODER_OPTION_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = (*(*self.m_pEncContext).pSvcParam).iTargetBitrate;
                    } else if pInfo.iLayer >= 0 && pInfo.iLayer < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = (*(*self.m_pEncContext).pSvcParam).sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = (*(*self.m_pEncContext).pSvcParam).iMaxBitrate;
                    } else if pInfo.iLayer >= 0 && pInfo.iLayer < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = (*(*self.m_pEncContext).pSvcParam).sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iMaxSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    let pStatistics = &mut *(pOption as *mut crate::SEncoderStatistics);
                    let iLayerIdx =
                        ((*(*self.m_pEncContext).pSvcParam).iSpatialLayerNum - 1) as usize;
                    let pEncStats = &(*self.m_pEncContext).sEncoderStatistics[iLayerIdx];

                    pStatistics.uiWidth = pEncStats.uiWidth;
                    pStatistics.uiHeight = pEncStats.uiHeight;
                    pStatistics.fAverageFrameRate = pEncStats.fAverageFrameRate;
                    pStatistics.fLatestFrameRate = pEncStats.fLatestFrameRate;
                    pStatistics.uiBitRate = pEncStats.uiBitRate;
                    pStatistics.uiAverageFrameQP = pEncStats.uiAverageFrameQP;
                    pStatistics.uiInputFrameCount = pEncStats.uiInputFrameCount;
                    pStatistics.uiSkippedFrameCount = pEncStats.uiSkippedFrameCount;
                    pStatistics.uiResolutionChangeTimes = pEncStats.uiResolutionChangeTimes;
                    pStatistics.uiIDRSentNum = pEncStats.uiIDRSentNum;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    *(pOption as *mut i32) =
                        (*(*self.m_pEncContext).pSvcParam).iComplexityMode as i32;
                }
                EncoderOption::ENCODER_OPTION_TRACE_LEVEL => {
                    if !self.m_pWelsTrace.is_null() {
                        *(pOption as *mut i32) = (*self.m_pWelsTrace).GetTraceLevel();
                    }
                }
                _ => return cmInitParaError,
            }
        }
        0
    }
}

impl Drop for CWelsH264SVCEncoder {
    fn drop(&mut self) {
        self.Uninitialize();
        if !self.m_pWelsTrace.is_null() {
            unsafe {
                let _ = Box::from_raw(self.m_pWelsTrace);
                self.m_pWelsTrace = null_mut();
            }
        }
    }
}

// C-Vtable Thunk Callbacks for ISVCEncoder
unsafe extern "C" fn ext_Initialize(
    p: *mut ISVCEncoderHandle,
    argv: *const SEncParamBase,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.Initialize(argv)
}

unsafe extern "C" fn ext_InitializeExt(
    p: *mut ISVCEncoderHandle,
    argv: *const SEncParamExt,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.InitializeExt(argv)
}

unsafe extern "C" fn ext_GetDefaultParams(
    p: *mut ISVCEncoderHandle,
    argv: *mut SEncParamExt,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.GetDefaultParams(argv)
}

unsafe extern "C" fn ext_Uninitialize(p: *mut ISVCEncoderHandle) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.Uninitialize()
}

unsafe extern "C" fn ext_EncodeFrame(
    p: *mut ISVCEncoderHandle,
    kpSrcPic: *const SSourcePicture,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.EncodeFrame(kpSrcPic, pBsInfo)
}

unsafe extern "C" fn ext_EncodeParameterSets(
    p: *mut ISVCEncoderHandle,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.EncodeParameterSets(pBsInfo)
}

unsafe extern "C" fn ext_ForceIntraFrame(p: *mut ISVCEncoderHandle, bIDR: bool) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.ForceIntraFrame(bIDR, -1)
}

unsafe extern "C" fn ext_SetOption(
    p: *mut ISVCEncoderHandle,
    opt_id: EncoderOption,
    option: *mut c_void,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.SetOption(opt_id, option)
}

unsafe extern "C" fn ext_GetOption(
    p: *mut ISVCEncoderHandle,
    opt_id: EncoderOption,
    option: *mut c_void,
) -> i32 {
    let enc = &mut *(p as *mut CWelsH264SVCEncoder);
    enc.GetOption(opt_id, option)
}

pub static G_ISVCENCODER_VTBL: ISVCEncoderVtbl = ISVCEncoderVtbl {
    Initialize: unsafe { std::mem::transmute(ext_Initialize as *const ()) },
    InitializeExt: unsafe { std::mem::transmute(ext_InitializeExt as *const ()) },
    GetDefaultParams: unsafe { std::mem::transmute(ext_GetDefaultParams as *const ()) },
    Uninitialize: unsafe { std::mem::transmute(ext_Uninitialize as *const ()) },
    EncodeFrame: unsafe { std::mem::transmute(ext_EncodeFrame as *const ()) },
    EncodeParameterSets: unsafe { std::mem::transmute(ext_EncodeParameterSets as *const ()) },
    ForceIntraFrame: unsafe { std::mem::transmute(ext_ForceIntraFrame as *const ()) },
    SetOption: unsafe { std::mem::transmute(ext_SetOption as *const ()) },
    GetOption: unsafe { std::mem::transmute(ext_GetOption as *const ()) },
};

pub unsafe extern "C" fn WelsCreateSVCEncoderExt(ppEncoder: *mut *mut ISVCEncoderHandle) -> i32 {
    if ppEncoder.is_null() {
        return 1;
    }
    let encoder = Box::new(CWelsH264SVCEncoder::new());
    *ppEncoder = Box::into_raw(encoder) as *mut ISVCEncoderHandle;
    0
}

pub unsafe extern "C" fn WelsDestroySVCEncoderExt(pEncoder: *mut ISVCEncoderHandle) {
    if !pEncoder.is_null() {
        let _ = Box::from_raw(pEncoder as *mut CWelsH264SVCEncoder);
    }
}

pub static G_ST_CODEC_VERSION: OpenH264Version = OpenH264Version {
    uMajor: 2,
    uMinor: 6,
    uRevision: 0,
    uReserved: 0,
};

#[unsafe(no_mangle)]
pub extern "C" fn WelsGetCodecVersion() -> OpenH264Version {
    G_ST_CODEC_VERSION
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsGetCodecVersionEx(pVersion: *mut OpenH264Version) {
    if !pVersion.is_null() {
        *pVersion = G_ST_CODEC_VERSION;
    }
}
