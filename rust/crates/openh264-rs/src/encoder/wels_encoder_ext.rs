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

use crate::encoder::au_set::{
    WelsWritePpsSyntax, WelsWriteSpsNal, WelsWriteSubsetSpsSyntax,
};
use crate::encoder::nal_encap::EWelsNalRefIdc::NRI_PRI_HIGHEST;
use crate::encoder::paraset_strategy::{
    IWelsParametersetStrategy, PARA_SET_TYPE_AVCSPS, PARA_SET_TYPE_PPS, PARA_SET_TYPE_SUBSETSPS,
};
use crate::encoder::svc_enc_slice_segment::{
    CheckRasterMultiSliceSetting, CheckRowMbMultiSliceSetting,
    SliceArgumentValidationFixedSliceMode,
};
use crate::api::codec_api::SSliceArgument;

use crate::{
    EComplexityMode, EParameterSetStrategy, EUsageType, EVideoFrameType, EncoderOption,
    ISVCEncoderVtbl, OpenH264Version, RCMode, SBitrateInfo, SEncParamBase,
    SEncParamExt, SFrameBSInfo, SLayerBSInfo, SSourcePicture, SSpatialLayerConfig, VideoFormat,
    CM_INIT_EXPECTED, CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS,
    CM_UNKNOWN_REASON, CM_UNSUPPORTED_DATA, MAX_LAYER_NUM_OF_FRAME, MAX_SPATIAL_LAYER_NUM,
    MAX_TEMPORAL_LAYER_NUM,
};
use crate::api::codec_api::ISVCEncoder as ISVCEncoderHandle;
use crate::api::codec_api::{EProfileIdc, ELevelIdc, LAYER_NUM};
use crate::api::codec_api::LAYER_NUM::*;
use crate::api::codec_api::ECOMPLEXITY_MODE::*;
use crate::api::codec_api::EParameterSetStrategy::*;
use crate::api::codec_api::EUsageType::*;
use crate::api::codec_api::RC_MODES::*;
use crate::api::codec_api::SliceModeEnum::*;
use crate::api::codec_api::EProfileIdc::*;
use crate::api::codec_api::ELevelIdc::LEVEL_UNKNOWN;
// g_ksLevelLimits/LEVEL_NUMBER come from codec/common/inc/wels_common_defs.h and are
// shared by both codecs; reuse the decoder's copy rather than declaring a second one.
use crate::decoder::nalu::g_ksLevelLimits;
/// codec/common/inc/wels_common_defs.h:47
pub const LEVEL_NUMBER: usize = 17;
use crate::encoder::au_set::{
    WelsBitRateVerification, WelsCheckRefFrameLimitationLevelIdcFirst,
    WelsCheckRefFrameLimitationNumRefFirst,
};
use crate::encoder::param_svc::GetLogFactor;
use crate::encoder::param_svc::SExistingParasetList;
use crate::encoder::svc_motion_estimate::CheckInRangeCloseOpen;
use crate::encoder::encoder_context::{
    SParaSetOffsetVariable, MAX_DQ_LAYER_NUM, MAX_PPS_COUNT, PARA_SET_TYPE,
};
use crate::encoder::encoder_ext::{
    GetMultipleThreadIdc, WelsInitEncoderExt, WelsUninitEncoderExt,
};
use crate::encoder::rc::WelsRcInitFuncPointers;
use crate::encoder::ref_list_mgr_svc::{FilterLTRMarkingFeedback, FilterLTRRecoveryRequest};

pub const VERSION_NUMBER: &str = "openh264 2.6.0";

// codec/encoder/core/inc/wels_const.h
pub const MAX_DEPENDENCY_LAYER: i32 = 4;
pub const MAX_TEMPORAL_LEVEL: i32 = 4;
/// `1 << (MAX_TEMPORAL_LEVEL - 1)` — wels_const.h:113. Was 64 here, which let
/// `InitializeInternal` accept GOP 16/32/64 and then index `g_kuiRefTemporalIdx`
/// (a `[[u8; 8]; 4]`) with `iTemporalLayerNum` up to 7.
pub const MAX_GOP_SIZE: u32 = 1 << (MAX_TEMPORAL_LEVEL - 1);
/// `MAX_GOP_SIZE >> 1` — wels_const.h:115, *not* 16.
pub const MAX_SHORT_REF_COUNT: i32 = (MAX_GOP_SIZE >> 1) as i32;
pub const MIN_FRAME_RATE: f32 = 1.0;
pub const MAX_FRAME_RATE: f32 = 60.0;
pub const MIN_BIT_RATE: i32 = 1;
pub const MAX_BIT_RATE: i32 = i32::MAX;
pub const MIN_REF_PIC_COUNT: i32 = 1;
pub const AUTO_REF_PIC_COUNT: i32 = -1;
pub const LONG_TERM_REF_NUM: i32 = 2;
pub const LONG_TERM_REF_NUM_SCREEN: i32 = 4;
pub const MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA: i32 = MAX_SHORT_REF_COUNT + LONG_TERM_REF_NUM;
pub const MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN: i32 =
    MAX_SHORT_REF_COUNT + LONG_TERM_REF_NUM_SCREEN;
pub const MAX_MACROBLOCK_SIZE_IN_BYTE: u32 = 400;
pub const NAL_HEADER_ADD_0X30BYTES: u32 = 20;
/// codec/common/inc/utils.h:46
pub const MAX_MBS_PER_FRAME: i32 = 36864;
/// codec/encoder/core/inc/svc_enc_slice_segment.h:61-65
pub const SAVED_NALUNIT_NUM: usize =
    (MAX_SPATIAL_LAYER_NUM * crate::MAX_QUALITY_LAYER_NUM) + 1 + MAX_SPATIAL_LAYER_NUM;
pub const MAX_SLICES_NUM: usize = (crate::MAX_NAL_UNITS_IN_LAYER - SAVED_NALUNIT_NUM) / 3;
pub const MIN_NUM_MB_PER_SLICE: i32 = 48;
/// codec/encoder/core/inc/rc.h:70-80
pub const GOM_MIN_QP_MODE: i32 = 12;
pub const MAX_LOW_BR_QP: i32 = 42;
pub const MIN_SCREEN_QP: i32 = 26;
pub const MAX_SCREEN_QP: i32 = 35;
pub const QP_MAX_VALUE: i32 = 51;
/// codec/api/wels/codec_def.h:128-133
pub const DEBLOCKING_IDC_0: i32 = 0;
pub const DEBLOCKING_IDC_2: i32 = 2;
pub const DEBLOCKING_OFFSET: i32 = 6;
pub const DEBLOCKING_OFFSET_MINUS: i32 = -6;

// `LAYER_TYPE` -- `codec_app_def.h:200` says NON_VIDEO_CODING_LAYER = 0 and
// VIDEO_CODING_LAYER = 1. This module previously declared the two as bare `u8`
// constants with the values **swapped**, so every `SLayerBSInfo::uiLayerType` this
// encoder wrote was mislabelled: parameter-set layers were tagged VCL and slice
// layers non-VCL. Re-exported from the canonical enum in `api/codec_api.rs` instead.
pub const VIDEO_CODING_LAYER: u8 = crate::api::codec_api::LAYER_TYPE::VIDEO_CODING_LAYER as u8;
pub const NON_VIDEO_CODING_LAYER: u8 =
    crate::api::codec_api::LAYER_TYPE::NON_VIDEO_CODING_LAYER as u8;

// SPATIAL_LAYER_* are LAYER_NUM variants in api::codec_api.

// CM_RETURN return status codes matching codec_def.h
pub const cmResultSuccess: i32 = 0;
pub const cmInitParaError: i32 = 1;
pub const cmUnknownReason: i32 = 2;
pub const cmMallocMemeError: i32 = 3;
pub const cmInitExpected: i32 = 4;
pub const cmUnsupportedData: i32 = 5;

// Encoder core return codes — codec/encoder/core/inc/wels_const.h:161-171.
// These are a bit field in C++; the values here previously ran 0..5 densely.
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNSUPPORTED_PARA: i32 = 0x02;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;
pub const ENC_RETURN_CORRECTED: i32 = 0x08;
pub const ENC_RETURN_INVALIDINPUT: i32 = 0x10;
pub const ENC_RETURN_MEMOVERFLOWFOUND: i32 = 0x20;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 0x40;
pub const ENC_RETURN_KNOWN_ISSUE: i32 = 0x80;

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

/// `WELS_MIN` — `macros.h`. A macro in C++; this module hosts the rest of that
/// header's set (`WELS_MAX`, `WELS_CLIP3`, `WELS_ABS`, `WELS_LOG2`).
#[inline(always)]
pub fn WELS_MIN<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a < b {
        a
    } else {
        b
    }
}

/// `(RC_MODES) iValue` — `welsEncoderExt.cpp:957`.
///
/// **Deviation from the C++, and the only one in `SetOption`.** C++ casts the
/// caller's `int32_t` straight into the enum, so an out-of-range value is stored
/// verbatim; `WelsRcInitFuncPointers`'s switch has no `default`, so the dispatch
/// table is then left pointing at the previous mode's callbacks. A Rust enum
/// cannot hold a value outside its variants, so an unrecognised mode is left as
/// `RC_QUALITY_MODE` (`RC_MODES`' `#[default]`, and C++'s value 0). Every value
/// the reference actually accepts round-trips exactly.
#[inline]
fn rc_mode_from_raw(iValue: i32) -> RCMode {
    match iValue {
        0 => RC_QUALITY_MODE,
        1 => RC_BITRATE_MODE,
        2 => RC_BUFFERBASED_MODE,
        3 => RC_TIMESTAMP_MODE,
        4 => RC_BITRATE_MODE_POST_SKIP,
        -1 => RC_OFF_MODE,
        _ => RC_QUALITY_MODE,
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
pub struct SLTRConfig {
    pub bEnableLongTermReference: bool,
    pub iLTRRefNum: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SProfileInfo {
    pub iLayer: i32,
    /// `EProfileIdc uiProfileIdc` — codec_app_def.h:693. Was a bare `i32` here,
    /// which forced every `ENCODER_OPTION_PROFILE` caller to hand
    /// `CheckProfileSetting` an untyped value.
    pub uiProfileIdc: EProfileIdc,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLevelInfo {
    pub iLayer: i32,
    /// `ELevelIdc uiLevelIdc` — codec_app_def.h:702.
    pub uiLevelIdc: ELevelIdc,
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



// SWelsSvcCodingParam is declared once, in param_svc.rs (mirroring
// codec/encoder/core/inc/param_svc.h). The copy that used to live here was a
// strict subset with truncated FillDefault/ParamBaseTranscode/ParamTranscode
// and a DetermineTemporalSettings that ignored the temporal-id table.
pub use crate::encoder::param_svc::SWelsSvcCodingParam;


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
pub use crate::encoder::ref_list_mgr_svc::SLTRMarkingFeedback;
pub use crate::encoder::ref_list_mgr_svc::SLTRRecoverRequest;
pub use crate::encoder::encoder_context::SLogContext;
pub use crate::encoder::param_svc::SSpatialLayerInternal;
pub use crate::encoder::rc::SWelsSvcRc;

// Core encoder functions implementations
//
// The parameter-set writers themselves live in `au_set.rs`, next to the rest of the
// au_set.cpp port; what remains here are the encoder_ext.cpp functions that wrap them
// in NAL units.

/// `WelsWriteOneSPS` — encoder_ext.cpp:2831.
// The three `*Rust` sketch entry points that used to live here -- WelsInitEncoderExtRust,
// WelsUninitEncoderExtRust and WelsEncoderEncodeExtRust -- are deleted. The real
// `encoder_ext.rs` implementations replace them at both C-ABI call sites.
//
// They were not merely redundant: the sketch teardown freed CMemoryAlign allocations
// with Rust's `Box`/`Vec::from_raw_parts`. Once Initialize switched to the real
// WelsInitEncoderExt, that mismatch corrupted the heap at Uninitialize (SIGTRAP in
// libsystem_malloc), which is how it was found.

pub unsafe fn WelsWriteOneSPS(pCtx: *mut sWelsEncCtx, kiSpsIdx: i32, iNalSize: *mut i32) -> i32 {
    let pOut = (*pCtx).pOut;
    let iNal = (*pOut).iNalIndex;
    crate::encoder::nal_encap::WelsLoadNal(
        pOut,
        crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SPS as i32,
        NRI_PRI_HIGHEST as i32,
    );

    WelsWriteSpsNal(
        crate::encoder::nal_encap::bs_buffer((*pOut).pBsBuffer, (*pOut).uiSize),
        (*pCtx).pSpsArray.add(kiSpsIdx as usize),
        &mut (*pOut).sBsWrite,
        IWelsParametersetStrategy::GetSpsIdOffsetList(
            (*(*pCtx).pFuncList).pParametersetStrategy,
            PARA_SET_TYPE_AVCSPS as i32,
        ),
    );
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        (*pOut).sNalList.add(iNal as usize),
        null_mut(),
        // available buffer to be written, so need to subtract the used length
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void,
        iNalSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    (*pCtx).iPosBsBuffer += *iNalSize;
    ENC_RETURN_SUCCESS
}

/// `WelsWriteOnePPS` — encoder_ext.cpp:2849.
pub unsafe fn WelsWriteOnePPS(pCtx: *mut sWelsEncCtx, kiPpsIdx: i32, iNalSize: *mut i32) -> i32 {
    let pOut = (*pCtx).pOut;
    let iNal = (*pOut).iNalIndex;
    /* generate picture parameter set */
    crate::encoder::nal_encap::WelsLoadNal(
        pOut,
        crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_PPS as i32,
        NRI_PRI_HIGHEST as i32,
    );

    WelsWritePpsSyntax(
        crate::encoder::nal_encap::bs_buffer((*pOut).pBsBuffer, (*pOut).uiSize),
        (*pCtx).pPPSArray.add(kiPpsIdx as usize),
        &mut (*pOut).sBsWrite,
        (*(*pCtx).pFuncList).pParametersetStrategy,
    );
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        (*pOut).sNalList.add(iNal as usize),
        null_mut(),
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void,
        iNalSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    (*pCtx).iPosBsBuffer += *iNalSize;
    ENC_RETURN_SUCCESS
}

/// `WelsWriteParameterSets` — encoder_ext.cpp:2874. Writes every SPS, subset SPS and
/// PPS the context holds.
///
/// Note the loops are bounded by `iSpsNum`/`iSubsetSpsNum`/`iPpsNum`, so an
/// unpopulated context writes **nothing** — which is what happens until Phase 4 builds
/// the parameter-set arrays. The previous version of this function substituted a count
/// of 1 when those were zero and swallowed each writer's return value; that turned
/// blocker C into a silent success.
pub unsafe fn WelsWriteParameterSets(
    pCtx: *mut sWelsEncCtx,
    pNalLen: *mut i32,
    pNumNal: *mut i32,
    pTotalLength: *mut i32,
) -> i32 {
    let mut iSize = 0i32;
    let mut iNal: i32;
    let mut iIdx: i32;
    let mut iId: i32;
    let mut iCountNal = 0i32;
    let mut iNalLength = 0i32;
    let mut iReturn;

    if pCtx.is_null()
        || pNalLen.is_null()
        || pNumNal.is_null()
        || (*(*pCtx).pFuncList).pParametersetStrategy.is_null()
    {
        return ENC_RETURN_UNEXPECTED;
    }
    let pParametersetStrategy = (*(*pCtx).pFuncList).pParametersetStrategy;
    let pVtbl = (*pParametersetStrategy).pVtbl;

    *pTotalLength = 0;
    /* write all SPS */
    iIdx = 0;
    while iIdx < (*pCtx).iSpsNum {
        ((*pVtbl).Update)(
            pParametersetStrategy,
            (*(*pCtx).pSpsArray.add(iIdx as usize)).uiSpsId,
            PARA_SET_TYPE_AVCSPS as i32,
        );
        /* generate sequence parameters set */
        iId = ((*pVtbl).GetSpsIdx)(pParametersetStrategy, iIdx);

        WelsWriteOneSPS(pCtx, iId, &mut iNalLength);

        *pNalLen.add(iCountNal as usize) = iNalLength;
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    /* write all Subset SPS */
    iIdx = 0;
    while iIdx < (*pCtx).iSubsetSpsNum {
        iNal = (*(*pCtx).pOut).iNalIndex;

        ((*pVtbl).Update)(
            pParametersetStrategy,
            (*(*pCtx).pSubsetArray.add(iIdx as usize)).pSps.uiSpsId,
            PARA_SET_TYPE_SUBSETSPS as i32,
        );

        iId = iIdx;

        /* generate Subset SPS */
        crate::encoder::nal_encap::WelsLoadNal(
            (*pCtx).pOut,
            crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SUBSET_SPS as i32,
            NRI_PRI_HIGHEST as i32,
        );

        WelsWriteSubsetSpsSyntax(
            crate::encoder::nal_encap::bs_buffer(
                (*(*pCtx).pOut).pBsBuffer,
                (*(*pCtx).pOut).uiSize,
            ),
            (*pCtx).pSubsetArray.add(iId as usize),
            &mut (*(*pCtx).pOut).sBsWrite,
            IWelsParametersetStrategy::GetSpsIdOffsetList(
                pParametersetStrategy,
                PARA_SET_TYPE_SUBSETSPS as i32,
            ),
        );
        crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            (*(*pCtx).pOut).sNalList.add(iNal as usize),
            null_mut(),
            (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
            (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize) as *mut c_void,
            &mut iNalLength,
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        *pNalLen.add(iCountNal as usize) = iNalLength;

        (*pCtx).iPosBsBuffer += iNalLength;
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    ((*pVtbl).UpdatePpsList)(pParametersetStrategy, pCtx);

    iIdx = 0;
    while iIdx < (*pCtx).iPpsNum {
        ((*pVtbl).Update)(
            pParametersetStrategy,
            (*(*pCtx).pPPSArray.add(iIdx as usize)).iPpsId,
            PARA_SET_TYPE_PPS as i32,
        );

        WelsWriteOnePPS(pCtx, iIdx, &mut iNalLength);

        *pNalLen.add(iCountNal as usize) = iNalLength;
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    *pNumNal = iCountNal;
    *pTotalLength = iSize;

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
    // Was `InitBits(&…sBsWrite, …pBsBuffer, …uiSize)`. The buffer and its length stay
    // where they were; the writer is a position, and resetting it is all `InitBits`
    // did that still means anything. Its `kpBuf: *const u8` parameter — stored as
    // `pStartBuf: *mut u8` and written through — is deleted rather than amended
    // (`phase2_findings.md` F13, third site).
    (*(*pCtx).pOut).sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
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
    (*pBsInfo).eFrameType = EVideoFrameType::videoFrameTypeInvalid;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ForceCodingIDR(pCtx: *mut sWelsEncCtx, _iLayerId: i32) -> i32 {
    if pCtx.is_null() {
        return 1;
    }
    0
}

/// `WelsEncoderParamAdjust` — codec/encoder/core/src/encoder_ext.cpp:4182.
///
/// Decides whether the new configuration can be folded into the running encoder
/// or needs a full uninit/init cycle, and does whichever it decides. `pNewParam`
/// is `SWelsSvcCodingParam*` (non-const) in C++ and really is written back — the
/// clip block in the no-reset arm mutates the caller's copy.
pub unsafe fn WelsEncoderParamAdjust(
    ppCtx: *mut *mut sWelsEncCtx,
    pNewParam: *mut SWelsSvcCodingParam,
) -> i32 {
    const EPSN: f32 = 0.000001;
    let mut iReturn;
    let mut iIndexD: i32;
    let mut bNeedReset: bool;
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on chip cache line size in byte
    let mut uiCpuFeatureFlags: u32 = 0;

    if ppCtx.is_null() || (*ppCtx).is_null() || pNewParam.is_null() {
        return 1;
    }

    /* Check validation in new parameters */
    iReturn = ParamValidationExt(&mut (**ppCtx).sLogCtx, pNewParam);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    iReturn = GetMultipleThreadIdc(
        &mut (**ppCtx).sLogCtx,
        pNewParam,
        &mut iSliceNum,
        &mut iCacheLineSize,
        &mut uiCpuFeatureFlags,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    let pOldParam: *mut SWelsSvcCodingParam = (**ppCtx).pSvcParam;

    if (*pOldParam).iUsageType != (*pNewParam).iUsageType {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    /* Decide whether need reset for IDR frame based on adjusting prarameters changed */
    /* Temporal levels, spatial settings and/ or quality settings changed need update parameter sets related. */
    bNeedReset = pOldParam.is_null()
        || ((*pOldParam).bSimulcastAVC != (*pNewParam).bSimulcastAVC)
        || ((*pOldParam).iSpatialLayerNum != (*pNewParam).iSpatialLayerNum)
        || ((*pOldParam).iPicWidth != (*pNewParam).iPicWidth
            || (*pOldParam).iPicHeight != (*pNewParam).iPicHeight)
        || ((*pOldParam).SUsedPicRect.iWidth != (*pNewParam).SUsedPicRect.iWidth
            || (*pOldParam).SUsedPicRect.iHeight != (*pNewParam).SUsedPicRect.iHeight)
        || ((*pOldParam).bEnableLongTermReference != (*pNewParam).bEnableLongTermReference)
        || ((*pOldParam).iLTRRefNum != (*pNewParam).iLTRRefNum)
        || ((*pOldParam).iMultipleThreadIdc != (*pNewParam).iMultipleThreadIdc)
        || ((*pOldParam).bEnableBackgroundDetection != (*pNewParam).bEnableBackgroundDetection)
        || ((*pOldParam).bEnableAdaptiveQuant != (*pNewParam).bEnableAdaptiveQuant)
        || ((*pOldParam).eSpsPpsIdStrategy != (*pNewParam).eSpsPpsIdStrategy);
    if ((*pNewParam).iMaxNumRefFrame > (*pOldParam).iMaxNumRefFrame)
        || ((*pOldParam).iMaxNumRefFrame == 1
            && (*pOldParam).iTemporalLayerNum == 1
            && (*pNewParam).iTemporalLayerNum == 2)
    {
        bNeedReset = true;
    }
    if !bNeedReset {
        // Check its picture resolutions/quality settings respectively in each dependency layer
        iIndexD = 0;
        debug_assert!((*pOldParam).iSpatialLayerNum == (*pNewParam).iSpatialLayerNum);
        loop {
            let d = iIndexD as usize;
            let kpOldDlp = &(*pOldParam).sDependencyLayers[d];
            let kpNewDlp = &(*pNewParam).sDependencyLayers[d];
            let mut fT1: f32 = 0.0;
            let mut fT2: f32 = 0.0;

            // check frame size settings
            if (*pOldParam).sSpatialLayers[d].iVideoWidth != (*pNewParam).sSpatialLayers[d].iVideoWidth
                || (*pOldParam).sSpatialLayers[d].iVideoHeight
                    != (*pNewParam).sSpatialLayers[d].iVideoHeight
                || kpOldDlp.iActualWidth != kpNewDlp.iActualWidth
                || kpOldDlp.iActualHeight != kpNewDlp.iActualHeight
            {
                bNeedReset = true;
                break;
            }

            if (*pOldParam).sSpatialLayers[d].sSliceArgument.uiSliceMode
                != (*pNewParam).sSpatialLayers[d].sSliceArgument.uiSliceMode
                || (*pOldParam).sSpatialLayers[d].sSliceArgument.uiSliceNum
                    != (*pNewParam).sSpatialLayers[d].sSliceArgument.uiSliceNum
            {
                bNeedReset = true;
                break;
            }

            // check frame rate
            // we can not check whether corresponding fFrameRate is equal or not,
            // only need to check d_max/d_min and max_fr/d_max whether it is equal or not
            if kpNewDlp.fInputFrameRate > EPSN && kpOldDlp.fInputFrameRate > EPSN {
                fT1 = kpNewDlp.fOutputFrameRate / kpNewDlp.fInputFrameRate
                    - kpOldDlp.fOutputFrameRate / kpOldDlp.fInputFrameRate;
            }
            if kpNewDlp.fOutputFrameRate > EPSN && kpOldDlp.fOutputFrameRate > EPSN {
                fT2 = (*pNewParam).fMaxFrameRate / kpNewDlp.fOutputFrameRate
                    - (*pOldParam).fMaxFrameRate / kpOldDlp.fOutputFrameRate;
            }
            if fT1 > EPSN || fT1 < -EPSN || fT2 > EPSN || fT2 < -EPSN {
                bNeedReset = true;
                break;
            }
            if (*pOldParam).sSpatialLayers[d].uiProfileIdc
                != (*pNewParam).sSpatialLayers[d].uiProfileIdc
            {
                bNeedReset = true;
                break;
            }
            // check level change, if new level is smaller than old level, don't reset
            // encoder. still use old level.
            if (*pNewParam).sSpatialLayers[d].uiLevelIdc as i32
                > (*pOldParam).sSpatialLayers[d].uiLevelIdc as i32
            {
                bNeedReset = true;
                break;
            }
            iIndexD += 1;
            if iIndexD >= (*pOldParam).iSpatialLayerNum {
                break;
            }
        }
    }

    if bNeedReset {
        let mut sLogCtx = (**ppCtx).sLogCtx;

        let iOldSpsPpsIdStrategy = (*pOldParam).eSpsPpsIdStrategy;
        let mut sTmpPsoVariable: [SParaSetOffsetVariable; PARA_SET_TYPE] = Default::default();
        let mut iTmpPpsIdList: [i32; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT] =
            [0; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT];
        // for LTR or SPS,PPS ID update
        let mut uiMaxIdrPicId: u16 = 0;
        iIndexD = 0;
        while iIndexD < (*pOldParam).iSpatialLayerNum {
            if (*pOldParam).sDependencyLayers[iIndexD as usize].uiIdrPicId > uiMaxIdrPicId {
                uiMaxIdrPicId = (*pOldParam).sDependencyLayers[iIndexD as usize].uiIdrPicId;
            }
            iIndexD += 1;
        }

        // for sEncoderStatistics
        let sTempEncoderStatistics = (**ppCtx).sEncoderStatistics;
        let uiStartTimestamp = (**ppCtx).uiStartTimestamp;
        let iStatisticsLogInterval = (**ppCtx).iStatisticsLogInterval;
        let iLastStatisticsLogTs = (**ppCtx).iLastStatisticsLogTs;
        // for sEncoderStatistics

        let mut sExistingParasetList = SExistingParasetList::default();
        let mut pExistingParasetList: *mut SExistingParasetList = null_mut();

        if iOldSpsPpsIdStrategy != CONSTANT_ID && (*pNewParam).eSpsPpsIdStrategy != CONSTANT_ID {
            let pStrategy = (*(**ppCtx).pFuncList).pParametersetStrategy;
            ((*(*pStrategy).pVtbl).OutputCurrentStructure)(
                pStrategy,
                sTmpPsoVariable.as_mut_ptr(),
                iTmpPpsIdList.as_mut_ptr(),
                *ppCtx,
                &mut sExistingParasetList,
            );

            if (iOldSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
                && ((*pNewParam).eSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
            {
                pExistingParasetList = &mut sExistingParasetList;
            }
        }

        WelsUninitEncoderExt(ppCtx);

        /* Update new parameters */
        if WelsInitEncoderExt(ppCtx, pNewParam, &mut sLogCtx, pExistingParasetList) != 0 {
            return 1;
        }
        // if WelsInitEncoderExt succeed
        // for LTR or SPS,PPS ID update
        iIndexD = 0;
        while iIndexD < (*pNewParam).iSpatialLayerNum {
            (*(**ppCtx).pSvcParam).sDependencyLayers[iIndexD as usize].uiIdrPicId = uiMaxIdrPicId;
            iIndexD += 1;
        }

        // for sEncoderStatistics
        (**ppCtx).sEncoderStatistics = sTempEncoderStatistics;
        (**ppCtx).uiStartTimestamp = uiStartTimestamp;
        (**ppCtx).iStatisticsLogInterval = iStatisticsLogInterval;
        (**ppCtx).iLastStatisticsLogTs = iLastStatisticsLogTs;
        // for sEncoderStatistics

        // load back the needed structure for eSpsPpsIdStrategy
        if (iOldSpsPpsIdStrategy != CONSTANT_ID && (*pNewParam).eSpsPpsIdStrategy != CONSTANT_ID)
            || (iOldSpsPpsIdStrategy == SPS_PPS_LISTING
                && (*pNewParam).eSpsPpsIdStrategy == SPS_PPS_LISTING)
        {
            let pStrategy = (*(**ppCtx).pFuncList).pParametersetStrategy;
            ((*(*pStrategy).pVtbl).LoadPreviousStructure)(
                pStrategy,
                sTmpPsoVariable.as_mut_ptr(),
                iTmpPpsIdList.as_mut_ptr(),
            );
        }
    } else {
        /* maybe adjustment introduced in bitrate or little settings adjustment and so on.. */
        (*pNewParam).iNumRefFrame = WELS_CLIP3(
            (*pNewParam).iNumRefFrame,
            MIN_REF_PIC_COUNT,
            if (*pNewParam).iUsageType == CAMERA_VIDEO_REAL_TIME {
                MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
            } else {
                MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
            },
        );
        (*pNewParam).iLoopFilterDisableIdc = WELS_CLIP3((*pNewParam).iLoopFilterDisableIdc, 0, 6);
        (*pNewParam).iLoopFilterAlphaC0Offset =
            WELS_CLIP3((*pNewParam).iLoopFilterAlphaC0Offset, -6, 6);
        (*pNewParam).iLoopFilterBetaOffset = WELS_CLIP3((*pNewParam).iLoopFilterBetaOffset, -6, 6);
        (*pNewParam).fMaxFrameRate =
            WELS_CLIP3((*pNewParam).fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);

        // we can not use direct struct based memcpy due some fields need keep unchanged as before
        (*pOldParam).fMaxFrameRate = (*pNewParam).fMaxFrameRate;
        (*pOldParam).iComplexityMode = (*pNewParam).iComplexityMode;
        (*pOldParam).uiIntraPeriod = (*pNewParam).uiIntraPeriod;
        (*pOldParam).eSpsPpsIdStrategy = (*pNewParam).eSpsPpsIdStrategy;
        (*pOldParam).bPrefixNalAddingCtrl = (*pNewParam).bPrefixNalAddingCtrl;
        (*pOldParam).iNumRefFrame = (*pNewParam).iNumRefFrame;
        (*pOldParam).uiGopSize = (*pNewParam).uiGopSize;
        if (*pOldParam).iTemporalLayerNum != (*pNewParam).iTemporalLayerNum {
            (*pOldParam).iTemporalLayerNum = (*pNewParam).iTemporalLayerNum;
            for d in 0..MAX_DEPENDENCY_LAYER as usize {
                (*pOldParam).sDependencyLayers[d].iCodingIndex = 0;
            }
        }
        (*pOldParam).iDecompStages = (*pNewParam).iDecompStages;
        /* denoise control */
        (*pOldParam).bEnableDenoise = (*pNewParam).bEnableDenoise;

        /* background detection control */
        (*pOldParam).bEnableBackgroundDetection = (*pNewParam).bEnableBackgroundDetection;

        /* adaptive quantization control */
        (*pOldParam).bEnableAdaptiveQuant = (*pNewParam).bEnableAdaptiveQuant;

        /* int32_t term reference control */
        (*pOldParam).bEnableLongTermReference = (*pNewParam).bEnableLongTermReference;
        (*pOldParam).iLtrMarkPeriod = (*pNewParam).iLtrMarkPeriod;

        // keep below values unchanged as before
        (*pOldParam).bEnableSSEI = (*pNewParam).bEnableSSEI;
        (*pOldParam).bSimulcastAVC = (*pNewParam).bSimulcastAVC;
        (*pOldParam).bEnableFrameCroppingFlag = (*pNewParam).bEnableFrameCroppingFlag;

        /* Motion search */

        /* Deblocking loop filter */
        (*pOldParam).iLoopFilterDisableIdc = (*pNewParam).iLoopFilterDisableIdc;
        (*pOldParam).iLoopFilterAlphaC0Offset = (*pNewParam).iLoopFilterAlphaC0Offset;
        (*pOldParam).iLoopFilterBetaOffset = (*pNewParam).iLoopFilterBetaOffset;

        /* Rate Control */
        (*pOldParam).iRCMode = (*pNewParam).iRCMode;
        (*pOldParam).iTargetBitrate = (*pNewParam).iTargetBitrate;
        (*pOldParam).iPaddingFlag = (*pNewParam).iPaddingFlag;

        /* Layer definition */
        (*pOldParam).bPrefixNalAddingCtrl = (*pNewParam).bPrefixNalAddingCtrl;

        // d
        iIndexD = 0;
        loop {
            let d = iIndexD as usize;
            (*pOldParam).sDependencyLayers[d].fInputFrameRate =
                (*pNewParam).sDependencyLayers[d].fInputFrameRate;
            (*pOldParam).sDependencyLayers[d].fOutputFrameRate =
                (*pNewParam).sDependencyLayers[d].fOutputFrameRate;
            (*pOldParam).sSpatialLayers[d].iSpatialBitrate =
                (*pNewParam).sSpatialLayers[d].iSpatialBitrate;
            (*pOldParam).sSpatialLayers[d].iMaxSpatialBitrate =
                (*pNewParam).sSpatialLayers[d].iMaxSpatialBitrate;
            (*pOldParam).sSpatialLayers[d].uiProfileIdc =
                (*pNewParam).sSpatialLayers[d].uiProfileIdc;
            (*pOldParam).sSpatialLayers[d].iDLayerQp = (*pNewParam).sSpatialLayers[d].iDLayerQp;

            /* Derived variants below */
            (*pOldParam).sDependencyLayers[d].iTemporalResolution =
                (*pNewParam).sDependencyLayers[d].iTemporalResolution;
            (*pOldParam).sDependencyLayers[d].iDecompositionStages =
                (*pNewParam).sDependencyLayers[d].iDecompositionStages;
            (*pOldParam).sDependencyLayers[d].uiCodingIdx2TemporalId =
                (*pNewParam).sDependencyLayers[d].uiCodingIdx2TemporalId;
            iIndexD += 1;
            if iIndexD >= (*pOldParam).iSpatialLayerNum {
                break;
            }
        }
    }

    /* Any else initialization/reset for rate control here? */

    0
}

/// `WelsEncoderApplyFrameRate` — codec/encoder/core/src/encoder_ext.cpp:672.
///
/// Pushes `fMaxFrameRate` down into every dependency layer, keeping each layer's
/// output/input ratio. The clip to [`MIN_FRAME_RATE`, `MAX_FRAME_RATE`] is the
/// *caller's* job in C++ (`SetOption` does it before calling); this function does
/// not clip.
pub unsafe fn WelsEncoderApplyFrameRate(pParam: *mut SWelsSvcCodingParam) {
    const kfEpsn: f32 = 0.000001;
    let kiNumLayer = (*pParam).iSpatialLayerNum;
    let kfMaxFrameRate = (*pParam).fMaxFrameRate;

    // set input frame rate to each layer
    for i in 0..kiNumLayer as usize {
        let pLayerParamInternal = &mut (*pParam).sDependencyLayers[i];
        let fRatio = pLayerParamInternal.fOutputFrameRate / pLayerParamInternal.fInputFrameRate;
        if (kfMaxFrameRate - pLayerParamInternal.fInputFrameRate) > kfEpsn
            || (kfMaxFrameRate - pLayerParamInternal.fInputFrameRate) < -kfEpsn
        {
            pLayerParamInternal.fInputFrameRate = kfMaxFrameRate;
            let fTargetOutputFrameRate = kfMaxFrameRate * fRatio;
            pLayerParamInternal.fOutputFrameRate = if fTargetOutputFrameRate >= 6.0 {
                fTargetOutputFrameRate
            } else {
                pLayerParamInternal.fInputFrameRate
            };
            let fOut = pLayerParamInternal.fOutputFrameRate;
            (*pParam).sSpatialLayers[i].fFrameRate = fOut;
        }
    }
}

/// `WelsEncoderApplyBitRate` — codec/encoder/core/src/encoder_ext.cpp:699.
///
/// `SPATIAL_LAYER_ALL` re-splits `iTargetBitrate` across the layers in the ratio
/// they already held; a single layer id only re-verifies that layer.
pub unsafe fn WelsEncoderApplyBitRate(
    pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    iLayer: i32,
) -> i32 {
    let iNumLayers = (*pParam).iSpatialLayerNum;
    let mut iOrigTotalBitrate = 0i32;
    if iLayer == SPATIAL_LAYER_ALL as i32 {
        // read old BR
        for i in 0..iNumLayers as usize {
            iOrigTotalBitrate += (*pParam).sSpatialLayers[i].iSpatialBitrate;
        }
        // write new BR
        for i in 0..iNumLayers as usize {
            let pLayerParam = &mut (*pParam).sSpatialLayers[i];
            let fRatio = pLayerParam.iSpatialBitrate as f32 / iOrigTotalBitrate as f32;
            pLayerParam.iSpatialBitrate = ((*pParam).iTargetBitrate as f32 * fRatio) as i32;

            if WelsBitRateVerification(pLogCtx, pLayerParam, i as i32) != ENC_RETURN_SUCCESS {
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
        }
    } else {
        return WelsBitRateVerification(
            pLogCtx,
            &mut (*pParam).sSpatialLayers[iLayer as usize],
            iLayer,
        );
    }
    ENC_RETURN_SUCCESS
}

/// `WelsEncoderApplyLTR` — codec/encoder/core/src/encoder_ext.cpp:4479.
///
/// Derives the reference-frame count the requested LTR setting needs, raises
/// `iMaxNumRefFrame`/`iNumRefFrame` to reach it, and re-adjusts the encoder.
pub unsafe fn WelsEncoderApplyLTR(
    pLogCtx: *mut SLogContext,
    ppCtx: *mut *mut sWelsEncCtx,
    pLTRValue: *mut SLTRConfig,
) -> i32 {
    let mut sConfig: SWelsSvcCodingParam = (*(**ppCtx).pSvcParam).clone();
    let mut iNumRefFrame;
    sConfig.bEnableLongTermReference = (*pLTRValue).bEnableLongTermReference;
    sConfig.iLTRRefNum = (*pLTRValue).iLTRRefNum;
    let uiGopSize: i32 = 1 << (sConfig.iTemporalLayerNum - 1);
    if sConfig.iUsageType == SCREEN_CONTENT_REAL_TIME {
        if sConfig.bEnableLongTermReference {
            sConfig.iLTRRefNum = LONG_TERM_REF_NUM_SCREEN;
            iNumRefFrame = WELS_MAX(1, WELS_LOG2(uiGopSize as u32)) + sConfig.iLTRRefNum;
        } else {
            sConfig.iLTRRefNum = 0;
            iNumRefFrame = WELS_MAX(1, uiGopSize >> 1);
        }
    } else {
        if sConfig.bEnableLongTermReference {
            sConfig.iLTRRefNum = LONG_TERM_REF_NUM;
        } else {
            sConfig.iLTRRefNum = 0;
        }
        iNumRefFrame = if (uiGopSize >> 1) > 1 {
            (uiGopSize >> 1) + sConfig.iLTRRefNum
        } else {
            MIN_REF_PIC_COUNT + sConfig.iLTRRefNum
        };
        iNumRefFrame = WELS_CLIP3(
            iNumRefFrame,
            MIN_REF_PIC_COUNT,
            MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
        );
    }
    if iNumRefFrame > sConfig.iMaxNumRefFrame {
        sConfig.iMaxNumRefFrame = iNumRefFrame;
    }

    if sConfig.iNumRefFrame < iNumRefFrame {
        sConfig.iNumRefFrame = iNumRefFrame;
    }
    WelsEncoderParamAdjust(ppCtx, &mut sConfig)
}

/// `ParamValidation` — codec/encoder/core/src/encoder_ext.cpp:264.
///
/// Complete port, including the RC-on bitrate loop and QP-range correction.
pub unsafe fn ParamValidation(pLogCtx: *mut SLogContext, pCfg: *mut SWelsSvcCodingParam) -> i32 {
    const fEpsn: f32 = 0.000001;
    debug_assert!(!pCfg.is_null());

    if !(((*pCfg).iUsageType as i32) < INPUT_CONTENT_TYPE_ALL as i32) {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCfg).iUsageType == SCREEN_CONTENT_REAL_TIME {
        if (*pCfg).iSpatialLayerNum > 1 {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        if (*pCfg).bEnableAdaptiveQuant {
            (*pCfg).bEnableAdaptiveQuant = false;
        }
        if (*pCfg).bEnableBackgroundDetection {
            (*pCfg).bEnableBackgroundDetection = false;
        }
        if !(*pCfg).bEnableSceneChangeDetect {
            (*pCfg).bEnableSceneChangeDetect = true;
        }
    }

    // turn off adaptive quant now, algorithms needs to be refactored
    (*pCfg).bEnableAdaptiveQuant = false;

    if (*pCfg).iSpatialLayerNum > 1 {
        let mut i = (*pCfg).iSpatialLayerNum - 1;
        while i > 0 {
            let fDlpUp = (*pCfg).sSpatialLayers[i as usize];
            let fDlp = (*pCfg).sSpatialLayers[(i - 1) as usize];
            if fDlp.iVideoWidth > fDlpUp.iVideoWidth || fDlp.iVideoHeight > fDlpUp.iVideoHeight {
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
            i -= 1;
        }
    }

    if !CheckInRangeCloseOpen(
        (*pCfg).iLoopFilterDisableIdc as i16,
        DEBLOCKING_IDC_0 as i16,
        (DEBLOCKING_IDC_2 + 1) as i16,
    ) || !CheckInRangeCloseOpen(
        (*pCfg).iLoopFilterAlphaC0Offset as i16,
        DEBLOCKING_OFFSET_MINUS as i16,
        (DEBLOCKING_OFFSET + 1) as i16,
    ) || !CheckInRangeCloseOpen(
        (*pCfg).iLoopFilterBetaOffset as i16,
        DEBLOCKING_OFFSET_MINUS as i16,
        (DEBLOCKING_OFFSET + 1) as i16,
    ) {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    for i in 0..(*pCfg).iSpatialLayerNum as usize {
        let fInput = (*pCfg).sDependencyLayers[i].fInputFrameRate;
        let fOutput = (*pCfg).sDependencyLayers[i].fOutputFrameRate;
        if fOutput > fInput
            || (fInput >= -fEpsn && fInput <= fEpsn)
            || (fOutput >= -fEpsn && fOutput <= fEpsn)
        {
            return ENC_RETURN_INVALIDINPUT;
        }
        if GetLogFactor(fOutput, fInput) == u32::MAX {
            // AUTO CORRECT: output frame rate must be input/2^n
            (*pCfg).sDependencyLayers[i].fOutputFrameRate = fInput;
            (*pCfg).sSpatialLayers[i].fFrameRate = fInput;
        }
    }

    if (*pCfg).iRCMode != RC_OFF_MODE
        && (*pCfg).iRCMode != RC_QUALITY_MODE
        && (*pCfg).iRCMode != RC_BUFFERBASED_MODE
        && (*pCfg).iRCMode != RC_BITRATE_MODE
        && (*pCfg).iRCMode != RC_TIMESTAMP_MODE
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // bitrate setting validation
    if (*pCfg).iRCMode != RC_OFF_MODE {
        if (*pCfg).iTargetBitrate <= 0 {
            return ENC_RETURN_INVALIDINPUT;
        }
        let mut iTotalBitrate = 0i32;
        for i in 0..(*pCfg).iSpatialLayerNum as usize {
            iTotalBitrate += (*pCfg).sSpatialLayers[i].iSpatialBitrate;
            if WelsBitRateVerification(pLogCtx, &mut (*pCfg).sSpatialLayers[i], i as i32)
                != ENC_RETURN_SUCCESS
            {
                return ENC_RETURN_INVALIDINPUT;
            }
        }
        if iTotalBitrate > (*pCfg).iTargetBitrate {
            return ENC_RETURN_INVALIDINPUT;
        }
        if (*pCfg).iMaxQp <= 0 || (*pCfg).iMinQp <= 0 {
            if (*pCfg).iUsageType == SCREEN_CONTENT_REAL_TIME {
                (*pCfg).iMinQp = MIN_SCREEN_QP;
                (*pCfg).iMaxQp = MAX_SCREEN_QP;
            } else {
                (*pCfg).iMinQp = GOM_MIN_QP_MODE;
                (*pCfg).iMaxQp = MAX_LOW_BR_QP;
            }
        }
        (*pCfg).iMinQp = WELS_CLIP3((*pCfg).iMinQp, GOM_MIN_QP_MODE, QP_MAX_VALUE);
        (*pCfg).iMaxQp = WELS_CLIP3((*pCfg).iMaxQp, (*pCfg).iMinQp, QP_MAX_VALUE);
    }

    // ref-frames validation, encoder_ext.cpp:392-398
    let bRefLimitFailed = if (*pCfg).iUsageType == CAMERA_VIDEO_REAL_TIME
        || (*pCfg).iUsageType == SCREEN_CONTENT_REAL_TIME
    {
        WelsCheckRefFrameLimitationNumRefFirst(pLogCtx, pCfg)
    } else {
        WelsCheckRefFrameLimitationLevelIdcFirst(pLogCtx, pCfg)
    };
    if bRefLimitFailed != 0 {
        return ENC_RETURN_INVALIDINPUT;
    }

    ENC_RETURN_SUCCESS
}

/// `ParamValidationExt` — codec/encoder/core/src/encoder_ext.cpp:403.
///
/// Complete, including the tail call to `ParamValidation`. All four slice modes are
/// handled: `SM_FIXEDSLCNUM_SLICE` and `SM_RASTER_SLICE` dispatch to
/// `SliceArgumentValidationFixedSliceMode` / `CheckRowMbMultiSliceSetting` /
/// `CheckRasterMultiSliceSetting` in `svc_enc_slice_segment.rs` (Phase 3.9); they were
/// `todo!()` before that landed.
///
/// The `WelsLog` calls that accompany each rejection in C++ have no counterpart here,
/// as elsewhere in this port — only the control flow and the returned code are
/// reproduced.
pub unsafe fn ParamValidationExt(
    pLogCtx: *mut SLogContext,
    pCodingParam: *mut SWelsSvcCodingParam,
) -> i32 {
    debug_assert!(!pCodingParam.is_null());
    if pCodingParam.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }

    if (*pCodingParam).iUsageType != CAMERA_VIDEO_REAL_TIME
        && (*pCodingParam).iUsageType != SCREEN_CONTENT_REAL_TIME
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCodingParam).iUsageType == SCREEN_CONTENT_REAL_TIME
        && !(*pCodingParam).bIsLosslessLink
        && (*pCodingParam).bEnableLongTermReference
    {
        (*pCodingParam).bEnableLongTermReference = false;
    }
    if (*pCodingParam).iSpatialLayerNum < 1
        || (*pCodingParam).iSpatialLayerNum > MAX_DEPENDENCY_LAYER
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCodingParam).iTemporalLayerNum < 1
        || (*pCodingParam).iTemporalLayerNum > MAX_TEMPORAL_LEVEL
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCodingParam).uiGopSize < 1 || (*pCodingParam).uiGopSize > MAX_GOP_SIZE {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCodingParam).uiIntraPeriod != 0
        && (*pCodingParam).uiIntraPeriod < (*pCodingParam).uiGopSize
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if (*pCodingParam).uiIntraPeriod != 0
        && ((*pCodingParam).uiIntraPeriod & ((*pCodingParam).uiGopSize - 1)) != 0
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // single thread => no parallel deblocking
    (*pCodingParam).bDeblockingParallelFlag = (*pCodingParam).iMultipleThreadIdc != 1;

    // eSpsPpsIdStrategy checkings
    let sps_listing = SPS_LISTING as i32;
    if (*pCodingParam).iSpatialLayerNum > 1
        && !(*pCodingParam).bSimulcastAVC
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        (*pCodingParam).eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if (*pCodingParam).iUsageType == SCREEN_CONTENT_REAL_TIME
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        (*pCodingParam).eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if (*pCodingParam).bSimulcastAVC
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        (*pCodingParam).eSpsPpsIdStrategy = INCREASING_ID;
    }
    if (*pCodingParam).bSimulcastAVC && (*pCodingParam).bPrefixNalAddingCtrl {
        (*pCodingParam).bPrefixNalAddingCtrl = false;
    }

    for i in 0..(*pCodingParam).iSpatialLayerNum {
        let idx = i as usize;
        let mut kiPicWidth = (*pCodingParam).sSpatialLayers[idx].iVideoWidth;
        let mut kiPicHeight = (*pCodingParam).sSpatialLayers[idx].iVideoHeight;

        if (*pCodingParam).iPicWidth > 0
            && (*pCodingParam).iPicHeight > 0
            && kiPicWidth == 0
            && kiPicHeight == 0
            && (*pCodingParam).iSpatialLayerNum == 1
        {
            kiPicWidth = (*pCodingParam).iPicWidth;
            kiPicHeight = (*pCodingParam).iPicHeight;
            (*pCodingParam).sSpatialLayers[idx].iVideoWidth = kiPicWidth;
            (*pCodingParam).sSpatialLayers[idx].iVideoHeight = kiPicHeight;
        }

        if kiPicWidth <= 0
            || kiPicHeight <= 0
            || kiPicWidth * kiPicHeight > (MAX_MBS_PER_FRAME << 8)
        {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        if (kiPicWidth & 0x0F) != 0 || (kiPicHeight & 0x0F) != 0 {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        if (*pCodingParam).sSpatialLayers[idx].sSliceArgument.uiSliceMode as i32
            >= SM_RESERVED as i32
        {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }

        CheckProfileSetting(
            pLogCtx,
            pCodingParam,
            i,
            (*pCodingParam).sSpatialLayers[idx].uiProfileIdc,
        );
        CheckLevelSetting(
            pLogCtx,
            pCodingParam,
            i,
            (*pCodingParam).sSpatialLayers[idx].uiLevelIdc,
        );

        // only one MB => single slice
        if kiPicWidth <= 16 && kiPicHeight <= 16 {
            (*pCodingParam).sSpatialLayers[idx].sSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
        }
        match (*pCodingParam).sSpatialLayers[idx].sSliceArgument.uiSliceMode {
            SM_SINGLE_SLICE => {
                let pSliceArgument = &mut (*pCodingParam).sSpatialLayers[idx].sSliceArgument;
                pSliceArgument.uiSliceNum = 1;
                pSliceArgument.uiSliceSizeConstraint = 0;
                for iIdx in 0..MAX_SLICES_NUM {
                    pSliceArgument.uiSliceMbNum[iIdx] = 0;
                }
            }
            // encoder_ext.cpp:553
            SM_FIXEDSLCNUM_SLICE => {
                let iReturn = SliceArgumentValidationFixedSliceMode(
                    pLogCtx,
                    &mut (*pCodingParam).sSpatialLayers[idx].sSliceArgument,
                    (*pCodingParam).iRCMode,
                    kiPicWidth,
                    kiPicHeight,
                );
                if iReturn != 0 {
                    return ENC_RETURN_UNSUPPORTED_PARA;
                }
            }
            // encoder_ext.cpp:560
            SM_RASTER_SLICE => {
                (*pCodingParam).sSpatialLayers[idx]
                    .sSliceArgument
                    .uiSliceSizeConstraint = 0;

                let iMbWidth = (kiPicWidth + 15) >> 4;
                let iMbHeight = (kiPicHeight + 15) >> 4;
                let iMbNumInFrame = iMbWidth * iMbHeight;
                let iMaxSliceNum = MAX_SLICES_NUM as i32;
                let pSliceArgument =
                    &mut (*pCodingParam).sSpatialLayers[idx].sSliceArgument as *mut SSliceArgument;

                if (*pSliceArgument).uiSliceMbNum[0] == 0 {
                    if iMbHeight > iMaxSliceNum {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    (*pSliceArgument).uiSliceNum = iMbHeight as u32;
                    for j in 0..iMbHeight as usize {
                        (*pSliceArgument).uiSliceMbNum[j] = iMbWidth as u32;
                    }
                    // verify interleave mode settings
                    if !CheckRowMbMultiSliceSetting(iMbWidth, pSliceArgument) {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                } else {
                    // verify interleave mode settings
                    if !CheckRasterMultiSliceSetting(iMbNumInFrame, pSliceArgument) {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    if (*pSliceArgument).uiSliceNum == 0
                        || (*pSliceArgument).uiSliceNum > iMaxSliceNum as u32
                    {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    if (*pSliceArgument).uiSliceNum == 1 {
                        // SM_RASTER_SLICE with one slice is just SM_SINGLE_SLICE
                        (*pSliceArgument).uiSliceMode = SM_SINGLE_SLICE;
                    } else {
                        // C++ logs "GOM based RC do not support SM_RASTER_SLICE" when
                        // iRCMode != RC_OFF_MODE here, but does not fail.
                        //
                        // considering coding efficiency and performance, iCountMbNum is
                        // constrained by MIN_NUM_MB_PER_SLICE for multi-slice mode
                        if iMbNumInFrame <= MIN_NUM_MB_PER_SLICE {
                            (*pSliceArgument).uiSliceMode = SM_SINGLE_SLICE;
                            (*pSliceArgument).uiSliceNum = 1;
                        }
                    }
                }
            }
            SM_SIZELIMITED_SLICE => {
                // encoder_ext.cpp:614-644. iMbWidth/iMbHeight are computed but
                // unused in this arm in the C++ too.
                let uiMaxNalSize = (*pCodingParam).uiMaxNalSize;
                let pSliceArgument = &mut (*pCodingParam).sSpatialLayers[idx].sSliceArgument;
                if pSliceArgument.uiSliceSizeConstraint <= MAX_MACROBLOCK_SIZE_IN_BYTE {
                    return ENC_RETURN_UNSUPPORTED_PARA;
                }
                if uiMaxNalSize > 0 {
                    if uiMaxNalSize < NAL_HEADER_ADD_0X30BYTES + MAX_MACROBLOCK_SIZE_IN_BYTE {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    if pSliceArgument.uiSliceSizeConstraint
                        > uiMaxNalSize - NAL_HEADER_ADD_0X30BYTES
                    {
                        pSliceArgument.uiSliceSizeConstraint =
                            uiMaxNalSize - NAL_HEADER_ADD_0X30BYTES;
                    }
                }
                pSliceArgument.uiSliceSizeConstraint -= NAL_HEADER_ADD_0X30BYTES;
            }
            _ => return ENC_RETURN_UNSUPPORTED_PARA,
        }
    }

    for i in 0..(*pCodingParam).iSpatialLayerNum as usize {
        let uiProfileIdc = (*pCodingParam).sSpatialLayers[i].uiProfileIdc;
        if uiProfileIdc == PRO_BASELINE || uiProfileIdc == PRO_SCALABLE_BASELINE {
            if (*pCodingParam).iEntropyCodingModeFlag != 0 {
                (*pCodingParam).iEntropyCodingModeFlag = 0;
            }
        } else if uiProfileIdc == PRO_UNKNOWN {
            (*pCodingParam).sSpatialLayers[i].uiProfileIdc =
                if i == 0 || (*pCodingParam).bSimulcastAVC {
                    if (*pCodingParam).iEntropyCodingModeFlag != 0 {
                        PRO_HIGH
                    } else {
                        PRO_BASELINE
                    }
                } else {
                    PRO_SCALABLE_BASELINE
                };
        }
    }

    ParamValidation(pLogCtx, pCodingParam)
}

/// `CheckProfileSetting` — codec/encoder/core/src/encoder_ext.cpp:126.
pub unsafe fn CheckProfileSetting(
    _pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    iLayer: i32,
    uiProfileIdc: EProfileIdc,
) {
    let pLayerInfo = &mut (*pParam).sSpatialLayers[iLayer as usize];
    pLayerInfo.uiProfileIdc = uiProfileIdc;
    if (*pParam).bSimulcastAVC {
        if uiProfileIdc != PRO_BASELINE && uiProfileIdc != PRO_MAIN && uiProfileIdc != PRO_HIGH {
            pLayerInfo.uiProfileIdc = PRO_UNKNOWN;
        }
    } else if iLayer == SPATIAL_LAYER_0 as i32 {
        if uiProfileIdc != PRO_BASELINE && uiProfileIdc != PRO_MAIN && uiProfileIdc != PRO_HIGH {
            pLayerInfo.uiProfileIdc = PRO_UNKNOWN;
        }
    } else if uiProfileIdc != PRO_SCALABLE_BASELINE && uiProfileIdc != PRO_SCALABLE_HIGH {
        pLayerInfo.uiProfileIdc = PRO_SCALABLE_BASELINE;
    }
}

/// `CheckLevelSetting` — codec/encoder/core/src/encoder_ext.cpp:151.
/// Accepts `uiLevelIdc` only if it appears in the shared level-limits table,
/// otherwise leaves the layer at `LEVEL_UNKNOWN`.
pub unsafe fn CheckLevelSetting(
    _pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    iLayer: i32,
    uiLevelIdc: ELevelIdc,
) {
    let pLayerInfo = &mut (*pParam).sSpatialLayers[iLayer as usize];
    pLayerInfo.uiLevelIdc = LEVEL_UNKNOWN;
    let mut iLevelIdx = LEVEL_NUMBER as i32 - 1;
    while iLevelIdx >= 0 {
        if g_ksLevelLimits[iLevelIdx as usize].uiLevelIdc as i32 == uiLevelIdc as i32 {
            pLayerInfo.uiLevelIdc = uiLevelIdc;
            break;
        }
        iLevelIdx -= 1;
    }
}

/// `CheckReferenceNumSetting` — codec/encoder/core/src/encoder_ext.cpp:163.
///
/// Out-of-range counts fall back to `AUTO_REF_PIC_COUNT`, not to the clamped
/// value.
pub unsafe fn CheckReferenceNumSetting(
    _pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    iNumRef: i32,
) {
    let iRefUpperBound = if (*pParam).iUsageType == CAMERA_VIDEO_REAL_TIME {
        MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
    } else {
        MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
    };
    (*pParam).iNumRefFrame = iNumRef;
    if iNumRef < MIN_REF_PIC_COUNT || iNumRef > iRefUpperBound {
        (*pParam).iNumRefFrame = AUTO_REF_PIC_COUNT;
    }
}

/// `WelsEncoderApplyBitVaryRang` — codec/encoder/core/src/encoder_ext.cpp:726.
///
/// Lowers each layer's `iMaxSpatialBitrate` to at most `iSpatialBitrate * (1 +
/// iRang/100)`. It does **not** write `iBitsVaryPercentage`; `SetOption` does
/// that (with the clip) before calling.
pub unsafe fn WelsEncoderApplyBitVaryRang(
    pLogCtx: *mut SLogContext,
    pParam: *mut SWelsSvcCodingParam,
    iRang: i32,
) -> i32 {
    let iNumLayers = (*pParam).iSpatialLayerNum;
    for i in 0..iNumLayers as usize {
        let pLayerParam = &mut (*pParam).sSpatialLayers[i];
        pLayerParam.iMaxSpatialBitrate = WELS_MIN(
            (pLayerParam.iSpatialBitrate as f64 * (1.0 + iRang as f64 / 100.0)) as i32,
            pLayerParam.iMaxSpatialBitrate,
        );
        if WelsBitRateVerification(pLogCtx, pLayerParam, i as i32) != ENC_RETURN_SUCCESS {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
    }
    ENC_RETURN_SUCCESS
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
            SWelsSvcCodingParam::FillDefaultExt(&mut *argv);
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
            if sConfig.ParamBaseTranscode(&*argv) != 0 {
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
            if sConfig.ParamTranscode(&*argv) != 0 {
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

            if (*pCfg).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
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

            if crate::encoder::encoder_ext::WelsInitEncoderExt(
                &mut self.m_pEncContext,
                pCfg,
                log_ctx,
                null_mut(),
            ) != 0
            {
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
                crate::encoder::encoder_ext::WelsUninitEncoderExt(&mut self.m_pEncContext);
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
            if (*kpSrcPic).iColorFormat != VideoFormat::videoFormatI420 as i32 {
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
            let kiEncoderReturn =
                crate::encoder::encoder_ext::WelsEncoderEncodeExt(self.m_pEncContext, pBsInfo, pSrcPic);
            let kiCurrentFrameMs = (WelsTime() - kiBeforeFrameUs) / 1000;

            if kiEncoderReturn == ENC_RETURN_MEMALLOCERR
                || kiEncoderReturn == ENC_RETURN_MEMOVERFLOWFOUND
                || kiEncoderReturn == ENC_RETURN_VLCOVERFLOWFOUND
            {
                crate::encoder::encoder_ext::WelsUninitEncoderExt(&mut self.m_pEncContext);
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
                let mut eFrameType = EVideoFrameType::videoFrameTypeSkip;
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
                    eFrameType == EVideoFrameType::videoFrameTypeSkip;
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

                if eFrameType == EVideoFrameType::videoFrameTypeIDR
                    || eFrameType == EVideoFrameType::videoFrameTypeI
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
                EncoderOption::ENCODER_OPTION_INTER_SPATIAL_PRED => {
                    // "this feature not supported at present" — C++ logs and
                    // returns success without touching anything.
                }
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
                    if sConfig.ParamBaseTranscode(&sEncodingParam) != 0 {
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
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // LogStatistics
                    let ts = (*self.m_pEncContext).iLastStatisticsLogTs;
                    self.LogStatistics(ts, 0);
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let sEncodingParam = *(pOption as *const SEncParamExt);
                    // verify number of spatial layer
                    if sEncodingParam.iSpatialLayerNum < 1
                        || sEncodingParam.iSpatialLayerNum > MAX_SPATIAL_LAYER_NUM as i32
                    {
                        return cmInitParaError;
                    }
                    let mut sConfig = SWelsSvcCodingParam::default();
                    if sConfig.ParamTranscode(&sEncodingParam) != 0 {
                        return cmInitParaError;
                    }
                    if sConfig.iSpatialLayerNum < 1 {
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
                    /* Check every field whether there is new request for memory block changed or else */
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // LogStatistics
                    let ts = (*self.m_pEncContext).iLastStatisticsLogTs;
                    self.LogStatistics(ts, sEncodingParam.iSpatialLayerNum - 1);
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
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
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
                    if WelsEncoderApplyBitRate(log_ctx, (*self.m_pEncContext).pSvcParam, pInfo.iLayer as i32)
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
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
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
                    if WelsEncoderApplyBitRate(log_ctx, (*self.m_pEncContext).pSvcParam, pInfo.iLayer as i32)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_RC_MODE => {
                    // 0:quality mode;1:bit-rate mode;2:bitrate limited mode
                    let iValue = *(pOption as *const i32);
                    (*(*self.m_pEncContext).pSvcParam).iRCMode = rc_mode_from_raw(iValue);
                    // Re-point the dispatch table. Setting the field alone leaves
                    // the encoder running the previous mode's callbacks.
                    let iRCMode = (*(*self.m_pEncContext).pSvcParam).iRCMode;
                    WelsRcInitFuncPointers(
                        &mut (*(*self.m_pEncContext).pFuncList).pfRc,
                        iRCMode,
                    );
                }
                EncoderOption::ENCODER_OPTION_RC_FRAME_SKIP => {
                    // 0:FRAME-SKIP disabled;1:FRAME-SKIP enabled
                    let bValue = *(pOption as *const bool);
                    if (*(*self.m_pEncContext).pSvcParam).iRCMode != RC_OFF_MODE {
                        (*(*self.m_pEncContext).pSvcParam).bEnableFrameSkip = bValue;
                    }
                    // rc off: the setting is accepted and ignored, as in C++.
                }
                EncoderOption::ENCODER_PADDING_PADDING => {
                    // 0:disable padding;1:padding
                    let iValue = *(pOption as *const i32);
                    (*(*self.m_pEncContext).pSvcParam).iPaddingFlag = iValue;
                }
                EncoderOption::ENCODER_LTR_RECOVERY_REQUEST => {
                    let pLTR_Recover_Request = pOption as *mut SLTRRecoverRequest;
                    FilterLTRRecoveryRequest(self.m_pEncContext, pLTR_Recover_Request);
                }
                EncoderOption::ENCODER_LTR_MARKING_FEEDBACK => {
                    let fb = pOption as *mut SLTRMarkingFeedback;
                    FilterLTRMarkingFeedback(self.m_pEncContext, fb);
                }
                EncoderOption::ENCODER_LTR_MARKING_PERIOD => {
                    let iValue = *(pOption as *const u32);
                    (*(*self.m_pEncContext).pSvcParam).iLtrMarkPeriod = iValue;
                }
                EncoderOption::ENCODER_OPTION_LTR => {
                    let pLTRValue = pOption as *mut SLTRConfig;
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    if WelsEncoderApplyLTR(log_ctx, &mut self.m_pEncContext, pLTRValue) != 0 {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_ENABLE_SSEI => {
                    let iValue = *(pOption as *const bool);
                    (*(*self.m_pEncContext).pSvcParam).bEnableSSEI = iValue;
                }
                EncoderOption::ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING => {
                    let iValue = *(pOption as *const bool);
                    (*(*self.m_pEncContext).pSvcParam).bPrefixNalAddingCtrl = iValue;
                }
                EncoderOption::ENCODER_OPTION_SPS_PPS_ID_STRATEGY => {
                    let iValue = *(pOption as *const i32);
                    let mut eNewStrategy = CONSTANT_ID;
                    match iValue {
                        0 => eNewStrategy = CONSTANT_ID,
                        0x01 => eNewStrategy = INCREASING_ID,
                        0x02 => eNewStrategy = SPS_LISTING,
                        0x03 => eNewStrategy = SPS_LISTING_AND_PPS_INCREASING,
                        0x06 => eNewStrategy = SPS_PPS_LISTING,
                        // out of range: unchanged, and *not* an error in C++ —
                        // eNewStrategy stays CONSTANT_ID and the code below runs.
                        _ => {}
                    }

                    let eOld = (*(*self.m_pEncContext).pSvcParam).eSpsPpsIdStrategy;
                    if ((eNewStrategy as i32 & SPS_LISTING as i32) != 0
                        || (eOld as i32 & SPS_LISTING as i32) != 0)
                        && eOld != eNewStrategy
                    {
                        // changing in the middle of call is NOT allowed for
                        // eSpsPpsIdStrategy > INCREASING_ID
                        return cmInitParaError;
                    }
                    let mut sConfig: SWelsSvcCodingParam =
                        *(*self.m_pEncContext).pSvcParam;
                    sConfig.eSpsPpsIdStrategy = eNewStrategy;

                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_CURRENT_PATH => {
                    if !(*self.m_pEncContext).pSvcParam.is_null() {
                        let path = pOption as *mut c_char;
                        (*(*self.m_pEncContext).pSvcParam).pCurPath = path;
                    }
                }
                EncoderOption::ENCODER_OPTION_DUMP_FILE => {
                    // The whole body is `#ifdef ENABLE_FRAME_DUMP` in C++, and
                    // ENABLE_FRAME_DUMP is not defined in the build this port
                    // tracks, so the case compiles to an empty success.
                }
                EncoderOption::ENCODER_OPTION_PROFILE => {
                    let pProfileInfo = &*(pOption as *const SProfileInfo);
                    if (pProfileInfo.iLayer as i32) < SPATIAL_LAYER_0 as i32
                        || (pProfileInfo.iLayer as i32) > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    CheckProfileSetting(
                        log_ctx,
                        (*self.m_pEncContext).pSvcParam,
                        pProfileInfo.iLayer as i32,
                        pProfileInfo.uiProfileIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_LEVEL => {
                    let pLevelInfo = &*(pOption as *const SLevelInfo);
                    if (pLevelInfo.iLayer as i32) < SPATIAL_LAYER_0 as i32
                        || (pLevelInfo.iLayer as i32) > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    CheckLevelSetting(
                        log_ctx,
                        (*self.m_pEncContext).pSvcParam,
                        pLevelInfo.iLayer as i32,
                        pLevelInfo.uiLevelIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_NUMBER_REF => {
                    let iValue = *(pOption as *const i32);
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    CheckReferenceNumSetting(log_ctx, (*self.m_pEncContext).pSvcParam, iValue);
                }
                EncoderOption::ENCODER_OPTION_DELIVERY_STATUS => {
                    let pValue = &*(pOption as *const SDeliveryStatus);
                    (*self.m_pEncContext).bDeliveryFlag = pValue.bDeliveryFlag;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    let iValue = *(pOption as *const i32);
                    (*(*self.m_pEncContext).pSvcParam).iComplexityMode = match iValue {
                        0 => EComplexityMode::LOW_COMPLEXITY,
                        1 => EComplexityMode::MEDIUM_COMPLEXITY,
                        _ => EComplexityMode::HIGH_COMPLEXITY,
                    };
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    // "this option is get-only!" — C++ warns and returns success.
                }
                EncoderOption::ENCODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    let iValue = *(pOption as *const i32);
                    (*self.m_pEncContext).iStatisticsLogInterval = iValue;
                }
                EncoderOption::ENCODER_OPTION_IS_LOSSLESS_LINK => {
                    let bValue = *(pOption as *const bool);
                    (*(*self.m_pEncContext).pSvcParam).bIsLosslessLink = bValue;
                }
                EncoderOption::ENCODER_OPTION_BITS_VARY_PERCENTAGE => {
                    let iValue = *(pOption as *const i32);
                    (*(*self.m_pEncContext).pSvcParam).iBitsVaryPercentage =
                        WELS_CLIP3(iValue, 0, 100);
                    let log_ctx = if !self.m_pWelsTrace.is_null() {
                        &mut (*self.m_pWelsTrace).m_sLogCtx
                    } else {
                        null_mut()
                    };
                    let iRang = (*(*self.m_pEncContext).pSvcParam).iBitsVaryPercentage;
                    WelsEncoderApplyBitVaryRang(
                        log_ctx,
                        (*self.m_pEncContext).pSvcParam,
                        iRang,
                    );
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
                // C++ ends with `default: return cmInitParaError`. There is no
                // wildcard arm here on purpose: `SetOption` takes a typed
                // `ENCODER_OPTION`, so every id the reference can be handed is
                // one of the 32 variants above, and leaving the match exhaustive
                // turns "a new option was added and not handled" into a compile
                // error instead of the silent success this replaced.
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
                EncoderOption::ENCODER_OPTION_INTER_SPATIAL_PRED => {
                    // "this feature not supported at present" — log-only in C++,
                    // and still a success return.
                }
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
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
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
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
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
                    pStatistics.fAverageFrameSpeedInMs = pEncStats.fAverageFrameSpeedInMs;

                    // rate control related
                    pStatistics.fAverageFrameRate = pEncStats.fAverageFrameRate;
                    pStatistics.fLatestFrameRate = pEncStats.fLatestFrameRate;
                    pStatistics.uiBitRate = pEncStats.uiBitRate;
                    pStatistics.uiAverageFrameQP = pEncStats.uiAverageFrameQP;

                    pStatistics.uiInputFrameCount = pEncStats.uiInputFrameCount;
                    pStatistics.uiSkippedFrameCount = pEncStats.uiSkippedFrameCount;

                    pStatistics.uiResolutionChangeTimes = pEncStats.uiResolutionChangeTimes;
                    pStatistics.uiIDRReqNum = pEncStats.uiIDRReqNum;
                    pStatistics.uiIDRSentNum = pEncStats.uiIDRSentNum;
                    pStatistics.uiLTRSentNum = pEncStats.uiLTRSentNum;
                }
                EncoderOption::ENCODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    *(pOption as *mut i32) = (*self.m_pEncContext).iStatisticsLogInterval;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    *(pOption as *mut i32) =
                        (*(*self.m_pEncContext).pSvcParam).iComplexityMode as i32;
                }
                // NOTE: C++'s GetOption has **no** ENCODER_OPTION_TRACE_LEVEL case —
                // it is set-only, and a get falls to `default: return cmInitParaError`.
                // This port used to answer it, which accepted a call the reference
                // rejects.
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
    Initialize: ext_Initialize,
    InitializeExt: ext_InitializeExt,
    GetDefaultParams: ext_GetDefaultParams,
    Uninitialize: ext_Uninitialize,
    EncodeFrame: ext_EncodeFrame,
    EncodeParameterSets: ext_EncodeParameterSets,
    ForceIntraFrame: ext_ForceIntraFrame,
    SetOption: ext_SetOption,
    GetOption: ext_GetOption,
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
