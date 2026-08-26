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

#![deny(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::encoder::au_set::{
    WelsWritePpsSyntax, WelsWriteSpsNal, WelsWriteSubsetSpsSyntax,
};
use crate::encoder::nal_encap::EWelsNalRefIdc::NRI_PRI_HIGHEST;
use crate::encoder::paraset_strategy::{
    ParasetStrategy, PARA_SET_TYPE_AVCSPS, PARA_SET_TYPE_PPS, PARA_SET_TYPE_SUBSETSPS,
};
use crate::encoder::svc_enc_slice_segment::{
    CheckRasterMultiSliceSetting, CheckRowMbMultiSliceSetting,
    SliceArgumentValidationFixedSliceMode,
};
use crate::api::codec_api::SSliceArgument;

use crate::{
    EComplexityMode, EParameterSetStrategy, EUsageType, EVideoFrameType, EncoderOption,
    OpenH264Version, RCMode, SBitrateInfo, SEncParamBase,
    SEncParamExt, SFrameBSInfo, SLayerBSInfo, SSourcePicture, SSpatialLayerConfig, VideoFormat,
    CM_INIT_EXPECTED, CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS,
    CM_UNKNOWN_REASON, CM_UNSUPPORTED_DATA, MAX_LAYER_NUM_OF_FRAME, MAX_SPATIAL_LAYER_NUM,
    MAX_TEMPORAL_LAYER_NUM,
};
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
    ctx_frame_bs, ctx_frame_bs_cur, ctx_ltr, ctx_param, ctx_pps_array, ctx_rc, ctx_rc_at,
    ctx_sps_array, ctx_subset_array,
    SParaSetOffsetVariable, MAX_DQ_LAYER_NUM,
    MAX_PPS_COUNT, PARA_SET_TYPE,
    ctx_func_list,
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
// T8.B6: the six levels are `codec_app_def.h:323`'s and are declared once, beside
// the `WelsLog` that filters on them.
pub use crate::common::wels_trace::{
    WELS_LOG_DEBUG, WELS_LOG_DEFAULT, WELS_LOG_DETAIL, WELS_LOG_ERROR, WELS_LOG_INFO,
    WELS_LOG_QUIET, WELS_LOG_WARNING,
};

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

// **T8.B6: `WelsTraceCallback`, `welsCodecTrace` and `WelsLog` stood here.**
//
// The first was a second declaration of `codec_api.h`'s callback type (the census
// carried it as `alias WelsTraceCallback x2`); the second had a fifth member,
// `m_pCodecInstance`, that `welsCodecTrace.h` does not have and nothing read; and
// the third was a stub — `let _ = (pLogCtx, iLevel, msg);` — so a caller who
// installed a trace callback through `ENCODER_OPTION_TRACE_CALLBACK` was handed
// silence. All three are `common::wels_trace`'s now, one copy for both codecs, and
// `WelsLog` delivers.
pub use crate::common::wels_trace::{WelsLog, WelsTraceCallback, welsCodecTrace};

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

/// `TagDeliveryStatus` — `codec_app_def.h:708`, the payload of
/// `ENCODER_OPTION_DELIVERY_STATUS`.
///
/// **F81 (T8.C4): the two `int` fields were missing.** This struct had one field
/// where the header has three, so it was **1 byte against the ABI's 12** — a
/// truncated declaration of a type a caller passes by pointer. Nothing misread
/// memory, because the one field the option arm reads is at offset 0 in both; what
/// was missing was the *rest of the caller's struct*, so any later read of
/// `iDropFrameType` would have been out of bounds and any by-value copy short. Found
/// by `api/abi_guard.rs`'s new pin, which is the whole reason to pin a size rather
/// than an offset.
///
/// Both are marked "reserved" upstream and neither is read by
/// `welsEncoderExt.cpp:1150-1155`, which takes `bDeliveryFlag` and logs it. They are
/// declared here for the layout, not for a reader.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDeliveryStatus {
    pub bDeliveryFlag: bool,
    /// `iDropFrameType` — the frame type that is dropped; reserved upstream.
    pub iDropFrameType: i32,
    /// `iDropFrameSize` — the frame size that is dropped; reserved upstream.
    pub iDropFrameSize: i32,
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsWriteOneSPS(pCtx: *mut sWelsEncCtx, kiSpsIdx: i32, iNalSize: *mut i32) -> i32 {
    let pOut = (*pCtx).pOut;
    let iNal = (*pOut).iNalIndex;
    crate::encoder::nal_encap::WelsLoadNal(
        pOut,
        crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SPS as i32,
        NRI_PRI_HIGHEST as i32,
    );

    WelsWriteSpsNal(
        &mut (&mut *pOut).sBsBuffer[..],
        &*ctx_sps_array(pCtx).add(kiSpsIdx as usize),
        &mut (*pOut).sBsWrite,
        ParasetStrategy(pCtx).GetSpsIdOffsetList(PARA_SET_TYPE_AVCSPS as i32),
    );
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &(&*pOut).sNalList[iNal as usize],
        &(&*pOut).sBsBuffer[..],
        None,
        ctx_frame_bs_cur(pCtx),
        // available buffer to be written, so need to subtract the used length
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        &mut *iNalSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    (*pCtx).iPosBsBuffer += *iNalSize;
    ENC_RETURN_SUCCESS
}

/// `WelsWriteOnePPS` — encoder_ext.cpp:2849.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
        &mut (&mut *pOut).sBsBuffer[..],
        &*ctx_pps_array(pCtx).add(kiPpsIdx as usize),
        &mut (*pOut).sBsWrite,
        ParasetStrategy(pCtx),
    );
    crate::encoder::nal_encap::WelsUnloadNal(pOut);

    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &(&*pOut).sNalList[iNal as usize],
        &(&*pOut).sBsBuffer[..],
        None,
        ctx_frame_bs_cur(pCtx),
        (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
        &mut *iNalSize,
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
        || (*ctx_func_list(pCtx)).pParametersetStrategy.is_none()
    {
        return ENC_RETURN_UNEXPECTED;
    }
    // Every access below re-acquires. `WelsWriteOneSPS` and `WelsWriteOnePPS` reach
    // this same object through `pCtx->pFuncList`, so the local this function used to
    // keep was an alias of theirs — invisible while it was a `*mut`. T4b.2a.

    *pTotalLength = 0;
    /* write all SPS */
    iIdx = 0;
    while iIdx < (*pCtx).iSpsNum {
        ParasetStrategy(pCtx).Update(
            (*ctx_sps_array(pCtx).add(iIdx as usize)).uiSpsId,
            PARA_SET_TYPE_AVCSPS as i32,
        );
        /* generate sequence parameters set */
        iId = ParasetStrategy(pCtx).GetSpsIdx(iIdx);

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

        ParasetStrategy(pCtx).Update(
            (*ctx_subset_array(pCtx).add(iIdx as usize)).pSps.uiSpsId,
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
            &mut (&mut *(*pCtx).pOut).sBsBuffer[..],
            &*ctx_subset_array(pCtx).add(iId as usize),
            &mut (*(*pCtx).pOut).sBsWrite,
            ParasetStrategy(pCtx).GetSpsIdOffsetList(PARA_SET_TYPE_SUBSETSPS as i32),
        );
        crate::encoder::nal_encap::WelsUnloadNal((*pCtx).pOut);

        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &(&*(*pCtx).pOut).sNalList[iNal as usize],
            &(&*(*pCtx).pOut).sBsBuffer[..],
            None,
            ctx_frame_bs_cur(pCtx),
            (*pCtx).iFrameBsSize - (*pCtx).iPosBsBuffer,
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

    ParasetStrategy(pCtx).UpdatePpsList(pCtx);

    iIdx = 0;
    while iIdx < (*pCtx).iPpsNum {
        ParasetStrategy(pCtx).Update(
            (*ctx_pps_array(pCtx).add(iIdx as usize)).iPpsId,
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




// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderEncodeParameterSetsRust(
    pCtx: &mut sWelsEncCtx,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pBsInfo.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pLayerBsInfo = &mut (*pBsInfo).sLayerInfo[0];
    pLayerBsInfo.pBsBuf = ctx_frame_bs(pCtx);
    pLayerBsInfo.pNalLengthInByte = (*pCtx.pOut).sNalLen.as_mut_ptr();
    // Was `InitBits(&…sBsWrite, …pBsBuffer, …uiSize)`. The buffer and its length stay
    // where they were; the writer is a position, and resetting it is all `InitBits`
    // did that still means anything. Its `kpBuf: *const u8` parameter — stored as
    // `pStartBuf: *mut u8` and written through — is deleted rather than amended
    // (`phase2_findings.md` F13, third site).
    (*pCtx.pOut).sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
    pCtx.iPosBsBuffer = 0;

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

/// `ForceCodingIDR` — `encoder_ext.cpp:3046`.
///
/// **T8b.A4: this was a stub.** It checked `pCtx` for null and returned 0, so
/// `ISVCEncoder::ForceIntraFrame(true)` reported success and *did nothing*. What the
/// caller got instead of an IDR was whatever the normal GOP logic produced next —
/// with the default `iIdrInterval` that is often an IDR anyway, which is why the
/// diffharness, the sweeps and `EncoderOutputTest`'s hashes never saw it: the
/// reference and the port agree on the frames where nothing was forced. `ltr_test.cpp:39`
/// is the assertion that does not agree, and it is about the *frame type reported*,
/// which no byte referee reads.
///
/// The measured shape (`tests/encoder_force_idr_ltr_test.rs`, red before this
/// commit): at IDR interval 1 every frame was IDR and the stub was invisible; at
/// interval 2 frame 1 came back `videoFrameTypeI` (3) instead of
/// `videoFrameTypeIDR` (1).
///
/// The reference's two arms differ only in *which* dependency layers they reset:
/// all of them unless simulcast-AVC is on and the caller named a valid one. Both
/// reset the same five fields and bump the same counter, so the loop below is
/// written once over the layer range each arm selects.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ForceCodingIDR(pCtx: &mut sWelsEncCtx, iLayerId: i32) -> i32 {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.
    let pParam = crate::encoder::encoder_context::ctx_param(pCtx);
    if pParam.is_null() {
        return 1;
    }
    let all_layers = iLayerId < 0
        || iLayerId >= crate::encoder::param_svc::MAX_SPATIAL_LAYER_NUM as i32
        || !(*pParam).bSimulcastAVC;
    let (first, last) = if all_layers {
        (0, (*pParam).iSpatialLayerNum)
    } else {
        (iLayerId, iLayerId + 1)
    };
    for iDid in first..last {
        let pParamInternal = std::ptr::addr_of_mut!((*pParam).sDependencyLayers[iDid as usize]);
        (*pParamInternal).iCodingIndex = 0;
        (*pParamInternal).iFrameIndex = 0;
        (*pParamInternal).iFrameNum = 0;
        (*pParamInternal).iPOC = 0;
        (*pParamInternal).bEncCurFrmAsIdrFlag = true;
        // The reference counts the request against layer **0** in the all-layers arm
        // and against `iLayerId` in the other — `sEncoderStatistics[0]` inside the
        // loop, not `sEncoderStatistics[iDid]`. Kept as it is: it is a statistic, and
        // a "fix" here would diverge from what a consumer reading
        // `ENCODER_OPTION_GET_STATISTICS` sees.
        let stat_idx = if all_layers { 0 } else { iLayerId as usize };
        pCtx.sEncoderStatistics[stat_idx].uiIDRReqNum =
            pCtx.sEncoderStatistics[stat_idx].uiIDRReqNum.wrapping_add(1);
    }
    pCtx.bCheckWindowStatusRefreshFlag = false;
    0
}

/// `WelsEncoderParamAdjust` — codec/encoder/core/src/encoder_ext.cpp:4182.
///
/// Decides whether the new configuration can be folded into the running encoder
/// or needs a full uninit/init cycle, and does whichever it decides. `pNewParam`
/// is `SWelsSvcCodingParam*` (non-const) in C++ and really is written back — the
/// clip block in the no-reset arm mutates the caller's copy.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderParamAdjust(
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pNewParam: *mut SWelsSvcCodingParam,
) -> i32 {
    const EPSN: f32 = 0.000001;
    let mut iReturn;
    let mut iIndexD: i32;
    let mut bNeedReset: bool;
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on chip cache line size in byte
    let mut uiCpuFeatureFlags: u32 = 0;

    if pNewParam.is_null() {
        return 1;
    }
    // **T8.B5 (S42): derived once, from the owner's `Box`.** Everything below that
    // used to spell `*ppCtx` reads through this; the re-initialisation branch
    // re-derives it, because the box it came from is gone by then.
    let mut pCtx: *mut sWelsEncCtx = match ppCtx.as_mut() {
        Some(pEncContext) => std::ptr::addr_of_mut!(**pEncContext),
        None => return 1,
    };

    /* Check validation in new parameters */
    iReturn = ParamValidationExt(&mut (*pCtx).sLogCtx, pNewParam);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    iReturn = GetMultipleThreadIdc(
        &mut (*pCtx).sLogCtx,
        pNewParam,
        &mut iSliceNum,
        &mut iCacheLineSize,
        &mut uiCpuFeatureFlags,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    let pOldParam: *mut SWelsSvcCodingParam = ctx_param(pCtx);

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
        let mut sLogCtx = (*pCtx).sLogCtx;

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
        let sTempEncoderStatistics = (*pCtx).sEncoderStatistics;
        let uiStartTimestamp = (*pCtx).uiStartTimestamp;
        let iStatisticsLogInterval = (*pCtx).iStatisticsLogInterval;
        let iLastStatisticsLogTs = (*pCtx).iLastStatisticsLogTs;
        // for sEncoderStatistics

        let mut sExistingParasetList = SExistingParasetList::default();
        let mut pExistingParasetList: *mut SExistingParasetList = null_mut();

        if iOldSpsPpsIdStrategy != CONSTANT_ID && (*pNewParam).eSpsPpsIdStrategy != CONSTANT_ID {
            ParasetStrategy(pCtx).OutputCurrentStructure(
                sTmpPsoVariable.as_mut_ptr(),
                iTmpPpsIdList.as_mut_ptr(),
                &mut *pCtx,
                &mut sExistingParasetList,
            );

            if (iOldSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
                && ((*pNewParam).eSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
            {
                pExistingParasetList = &mut sExistingParasetList;
            }
        }

        WelsUninitEncoderExt(ppCtx.take());

        /* Update new parameters */
        if WelsInitEncoderExt(ppCtx, pNewParam, &mut sLogCtx, pExistingParasetList) != 0 {
            return 1;
        }
        // The context below this line is a different allocation from the one above
        // it. `encoder_ext.cpp` hides that behind `*ppCtx`; here it is a statement.
        pCtx = match ppCtx.as_mut() {
            Some(pEncContext) => std::ptr::addr_of_mut!(**pEncContext),
            None => return 1,
        };
        // if WelsInitEncoderExt succeed
        // for LTR or SPS,PPS ID update
        iIndexD = 0;
        while iIndexD < (*pNewParam).iSpatialLayerNum {
            (*ctx_param(pCtx)).sDependencyLayers[iIndexD as usize].uiIdrPicId = uiMaxIdrPicId;
            iIndexD += 1;
        }

        // for sEncoderStatistics
        (*pCtx).sEncoderStatistics = sTempEncoderStatistics;
        (*pCtx).uiStartTimestamp = uiStartTimestamp;
        (*pCtx).iStatisticsLogInterval = iStatisticsLogInterval;
        (*pCtx).iLastStatisticsLogTs = iLastStatisticsLogTs;
        // for sEncoderStatistics

        // load back the needed structure for eSpsPpsIdStrategy
        if (iOldSpsPpsIdStrategy != CONSTANT_ID && (*pNewParam).eSpsPpsIdStrategy != CONSTANT_ID)
            || (iOldSpsPpsIdStrategy == SPS_PPS_LISTING
                && (*pNewParam).eSpsPpsIdStrategy == SPS_PPS_LISTING)
        {
            ParasetStrategy(pCtx).LoadPreviousStructure(
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderApplyFrameRate(pParam: *mut SWelsSvcCodingParam) {
    const kfEpsn: f32 = 0.000001;
    let kiNumLayer = (*pParam).iSpatialLayerNum;
    let kfMaxFrameRate = (*pParam).fMaxFrameRate;

    // set input frame rate to each layer
    for i in 0..kiNumLayer as usize {
        let pLayerParamInternal = std::ptr::addr_of_mut!((*pParam).sDependencyLayers[i]);
        let fRatio = (*pLayerParamInternal).fOutputFrameRate / (*pLayerParamInternal).fInputFrameRate;
        if (kfMaxFrameRate - (*pLayerParamInternal).fInputFrameRate) > kfEpsn
            || (kfMaxFrameRate - (*pLayerParamInternal).fInputFrameRate) < -kfEpsn
        {
            (*pLayerParamInternal).fInputFrameRate = kfMaxFrameRate;
            let fTargetOutputFrameRate = kfMaxFrameRate * fRatio;
            (*pLayerParamInternal).fOutputFrameRate = if fTargetOutputFrameRate >= 6.0 {
                fTargetOutputFrameRate
            } else {
                (*pLayerParamInternal).fInputFrameRate
            };
            let fOut = (*pLayerParamInternal).fOutputFrameRate;
            (*pParam).sSpatialLayers[i].fFrameRate = fOut;
        }
    }
}

/// `WelsEncoderApplyBitRate` — codec/encoder/core/src/encoder_ext.cpp:699.
///
/// `SPATIAL_LAYER_ALL` re-splits `iTargetBitrate` across the layers in the ratio
/// they already held; a single layer id only re-verifies that layer.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsEncoderApplyLTR(
    pLogCtx: *mut SLogContext,
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pLTRValue: *mut SLTRConfig,
) -> i32 {
    let mut sConfig: SWelsSvcCodingParam = match ppCtx.as_mut() {
        Some(pEncContext) => (*ctx_param(std::ptr::addr_of_mut!(**pEncContext))).clone(),
        None => return 1,
    };
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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

    // eSpsPpsIdStrategy checkings — `encoder_ext.cpp:466-491`.
    //
    // **The three traces were missing until T8b.B3.** The adjustments themselves were
    // ported; the `WelsLog` beside each was not, so an application that asked for a
    // listing strategy in a configuration that does not support one had it silently
    // replaced. The reference tells it. The messages are the reference's, argument for
    // argument — including the third one's, which prints `eSpsPpsIdStrategy` where it
    // says `bSimulcastAVC` and vice versa (`encoder_ext.cpp:487-489`); reproduced
    // rather than repaired, because a consumer grepping its logs matches on text.
    let sps_listing = SPS_LISTING as i32;
    if (*pCodingParam).iSpatialLayerNum > 1
        && !(*pCodingParam).bSimulcastAVC
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy setting ({}) with multiple svc SpatialLayers ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                (*pCodingParam).eSpsPpsIdStrategy as i32,
                (*pCodingParam).iSpatialLayerNum
            ),
        );
        (*pCodingParam).eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if (*pCodingParam).iUsageType == SCREEN_CONTENT_REAL_TIME
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy setting ({}) with iUsageType ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                (*pCodingParam).eSpsPpsIdStrategy as i32,
                (*pCodingParam).iUsageType as i32
            ),
        );
        (*pCodingParam).eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if (*pCodingParam).bSimulcastAVC
        && (sps_listing & (*pCodingParam).eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_INFO,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy({}) under bSimulcastAVC({}) not supported yet, adjusted to INCREASING_ID",
                (*pCodingParam).eSpsPpsIdStrategy as i32,
                (*pCodingParam).bSimulcastAVC as i32
            ),
        );
        (*pCodingParam).eSpsPpsIdStrategy = INCREASING_ID;
    }
    if (*pCodingParam).bSimulcastAVC && (*pCodingParam).bPrefixNalAddingCtrl {
        (*pCodingParam).bPrefixNalAddingCtrl = false;
    }

    // -----------------------------------------------------------------------
    // -----------------------------------------------------------------------
    // **S48's two refusals are gone — both plugins are ported (Phase 8b session C).**
    //
    // `METHOD_DENOISE` (T8b.C1, `processing/denoise.rs`) and `METHOD_DOWNSAMPLE`
    // (T8b.C2, `processing/downsample.rs`) were untranslated, and *both callers
    // dropped the `RET_NOTSUPPORTED` they returned*, so asking for either
    // succeeded and produced wrong bytes: un-denoised frames, and lower spatial
    // layers encoded from whatever the picture pool last held. S48 made them
    // refuse at the entry point instead — `ENC_RETURN_UNSUPPORTED_PARA` here,
    // `cmInitParaError` out of `InitializeExt` — which cost 17 gtest rows that had
    // been "passing" by never checking bytes. All 17 are back.
    //
    // Kept for the record, because it is the reason the check that stood here was
    // written the way it was: the downsample test was `JudgeNeedOfScaling`'s own,
    // per layer (`wels_preprocess.rs:710`), comparing each layer against
    // `SUsedPicRect` — the *input rect*. Testing `iPicWidth != <top layer>` instead
    // looks equivalent and is not, because layer dimensions are rounded up to a
    // multiple of 16 by `ParamTranscode` while `iPicWidth` is not, so 140x96
    // legitimately becomes a 144x96 layer. Measured, not reasoned:
    // `tests/encoder_force_idr_ltr_test.rs`'s 140x96 row failed at `InitializeExt`
    // against the first version of that check.

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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
    /// **T8.B5 — the boundary object owns the encoder context (S42).**
    ///
    /// `CWelsH264SVCEncoder::m_pEncContext` is a `sWelsEncCtx*` in the reference
    /// because C has no other way to say "mine, or nothing". This is the
    /// allocation root: `WelsInitEncoderExt` fills the slot, `WelsUninitEncoderExt`
    /// takes it by value, and every `*mut sWelsEncCtx` in the tree below is derived
    /// from this `Box` for the duration of one call. The tree itself stays raw and
    /// stays tagged `port-raw(Phase 9)` — what moves here is the *root*, which is
    /// the only place the question "who frees this" was ever open.
    pub m_pEncContext: Option<Box<sWelsEncCtx>>,
    /// The trace object, owned outright. It was a `Box::into_raw` in the
    /// constructor and a `Box::from_raw` in `Drop` with a null test at every use
    /// between them; the reference's `m_pWelsTrace` is a `new`ed object with the
    /// same lifetime as the encoder and no null state after construction.
    pub m_pWelsTrace: Box<welsCodecTrace>,
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
    /// **S42's root, read out.** The one expression in this file that turns the
    /// boundary object's ownership back into the `*mut sWelsEncCtx` the tree below
    /// still speaks — derived from the `Box` for the duration of one call, never
    /// stored. It takes the slot rather than `&mut self` on purpose: a `&mut self`
    /// here would retag the whole encoder object, and the log-context pointers the
    /// call sites hold are derived from a *sibling* field.
    ///
    /// Null exactly when the encoder has no context, which is the state
    /// `m_pEncContext == NULL` used to mean.
    #[inline]
    fn ctx_ptr(slot: &mut Option<Box<sWelsEncCtx>>) -> *mut sWelsEncCtx {
        match slot {
            Some(pEncContext) => std::ptr::addr_of_mut!(**pEncContext),
            None => null_mut(),
        }
    }

    /// `VERSION_NUMBER` — the string `welsEncoderExt.cpp` puts in its trace lines,
    /// built from the version this crate reports through `WelsGetCodecVersion`.
    fn version_number() -> String {
        format!(
            "{}.{}.{}",
            G_ST_CODEC_VERSION.uMajor, G_ST_CODEC_VERSION.uMinor, G_ST_CODEC_VERSION.uRevision
        )
    }

    /// The trace destination for this encoder's own messages.
    #[inline]
    fn log_ctx(&mut self) -> *mut SLogContext {
        std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx)
    }

    /// **T8.B6 — the write-through that replaces the reference's back-pointer.**
    ///
    /// `WelsInitEncoderExt` copies the log context into `sWelsEncCtx::sLogCtx`, and
    /// in C++ that copy stays current because it holds a route to the trace object
    /// rather than the settings. Here it holds the settings, so a `SetOption` that
    /// changes them re-stamps the copy. One line per trace option arm.
    pub(crate) fn sync_log_ctx(&mut self) {
        let CWelsH264SVCEncoder { m_pWelsTrace, m_pEncContext, .. } = self;
        if let Some(pEncContext) = m_pEncContext.as_mut() {
            pEncContext.sLogCtx = m_pWelsTrace.log_context();
        }
    }

    pub fn new() -> Self {
        let mut encoder = Self {
            m_pEncContext: None,
            m_pWelsTrace: Box::new(welsCodecTrace::new()),
            m_iMaxPicWidth: 0,
            m_iMaxPicHeight: 0,
            m_iCspInternal: 0,
            m_bInitialFlag: false,
        };
        encoder.InitEncoder();
        encoder
    }

    pub fn InitEncoder(&mut self) {
        // `welsEncoderExt.cpp:180` — `m_pWelsTrace->SetCodecInstance (this)`, which
        // writes `m_sLogCtx.pCodecInstance`. It is the value `WelsLog` prints in the
        // message tag and nothing else, so it travels as an address (T8.B6).
        let instance = std::ptr::from_mut(self) as usize;
        self.m_pWelsTrace.SetCodecInstance(instance);
    }

    /// T8.B7: the caller's block, for the duration of the call. The thunk owns the
    /// null check and the alignment claim; from here in it is a place.
    pub fn GetDefaultParams(&mut self, argv: &mut SEncParamExt) -> i32 {
        SWelsSvcCodingParam::FillDefaultExt(argv);
        cmResultSuccess
    }

    /// T8.B7: `None` is the reference's `NULL argv`, which is a *reported* error
    /// (`welsEncoderExt.cpp:192`) and not a caller contract, so it survives the
    /// translation as an `Option` rather than being rejected at the thunk.
    pub fn Initialize(&mut self, argv: Option<&SEncParamBase>) -> i32 {
        // `welsEncoderExt.cpp`'s `if (m_pWelsTrace == NULL) return cmMallocMemeError`
        // stood here on both entry points. It guards a `new welsCodecTrace` that can
        // return null in C++; `Box::new` cannot, and T8.B5 makes the member owned,
        // so the arm is unreachable rather than untaken and is deleted.
        // `welsEncoderExt.cpp:188` (T8.B6).
        WelsLog(
            self.log_ctx(),
            WELS_LOG_INFO,
            &format!(
                "CWelsH264SVCEncoder::InitEncoder(), openh264 codec version = {}",
                Self::version_number()
            ),
        );
        let Some(argv) = argv else {
            // `welsEncoderExt.cpp:192`.
            WelsLog(
                self.log_ctx(),
                WELS_LOG_ERROR,
                "CWelsH264SVCEncoder::Initialize(), invalid argv= 0x0",
            );
            return cmInitParaError;
        };
        let mut sConfig = SWelsSvcCodingParam::default();
        if sConfig.ParamBaseTranscode(argv) != 0 {
            self.TraceParamInfo(&sConfig.to_param_ext());
            self.Uninitialize();
            return cmInitParaError;
        }
        self.InitializeInternal(&mut sConfig)
    }

    /// See [`Self::Initialize`] for why `argv` is an `Option` and not a contract.
    pub fn InitializeExt(&mut self, argv: Option<&SEncParamExt>) -> i32 {
        // See `Initialize`: the trace's null guard is unreachable since T8.B5.
        // `welsEncoderExt.cpp:215` (T8.B6).
        WelsLog(
            self.log_ctx(),
            WELS_LOG_INFO,
            &format!(
                "CWelsH264SVCEncoder::InitEncoder(), openh264 codec version = {}",
                Self::version_number()
            ),
        );
        let Some(argv) = argv else {
            // `welsEncoderExt.cpp:219`.
            WelsLog(
                self.log_ctx(),
                WELS_LOG_ERROR,
                "CWelsH264SVCEncoder::InitializeExt(), invalid argv= 0x0",
            );
            return cmInitParaError;
        };
        let mut sConfig = SWelsSvcCodingParam::default();
        if sConfig.ParamTranscode(argv) != 0 {
            self.TraceParamInfo(argv);
            self.Uninitialize();
            return cmInitParaError;
        }
        self.InitializeInternal(&mut sConfig)
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
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
            let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);

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

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn Uninitialize(&mut self) -> i32 {
        if !self.m_bInitialFlag {
            return 0;
        }
        // `welsEncoderExt.cpp:358` (T8.B6).
        WelsLog(
            self.log_ctx(),
            WELS_LOG_INFO,
            &format!(
                "CWelsH264SVCEncoder::Uninitialize(), openh264 codec version = {}.",
                Self::version_number()
            ),
        );
        // T8.B5: `if !is_null { Uninit(&mut p); p = null }` was three statements
        // saying what `take()` says in one, and the null store is the type's now.
        // The obligation this block carries is the tree below the root, not the
        // root: `WelsUninitEncoderExt` takes the context *by value* since T8.B5, so
        // there is no aliasing question left at this call — only the free cascade's
        // own raw walk, which is `port-raw(Phase 9)`.
        unsafe { crate::encoder::encoder_ext::WelsUninitEncoderExt(self.m_pEncContext.take()) };
        self.m_bInitialFlag = false;
        0
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn EncodeFrame(
        &mut self,
        kpSrcPic: &SSourcePicture,
        pBsInfo: &mut SFrameBSInfo,
    ) -> i32 {
        if !self.m_bInitialFlag {
            return cmInitParaError;
        }
        if kpSrcPic.iColorFormat != VideoFormat::videoFormatI420 as i32 {
            return cmInitParaError;
        }
        let kiEncoderReturn = self.EncodeFrameInternal(kpSrcPic, pBsInfo);
        if kiEncoderReturn != cmResultSuccess {
            return kiEncoderReturn;
        }
        kiEncoderReturn
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn EncodeFrameInternal(
        &mut self,
        pSrcPic: &SSourcePicture,
        pBsInfo: &mut SFrameBSInfo,
    ) -> i32 {
        if pSrcPic.iPicWidth < 16 || pSrcPic.iPicHeight < 16 {
            return cmUnsupportedData;
        }
        // **T9.H3 — the flip's top boundary.** The encode root takes
        // `&mut sWelsEncCtx`, so the context reaches it as a borrow of the owning
        // `Box` rather than through `ctx_ptr`'s `addr_of_mut!`. This is the one
        // `&mut` the whole encode tree hangs from: everything below re-derives a
        // raw from it at each use, so there is exactly one `Unique` on the
        // context's allocation and every other derivation is its child.
        //
        // `WelsEncoderEncodeExt`'s own `pCtx.is_null()` guard moved here with it,
        // and it reproduces the whole old null path, not just the return code: a
        // null context made that function answer `ENC_RETURN_MEMALLOCERR`, which
        // fell into the arm below and ran `WelsUninitEncoderExt(take())` before
        // returning `cmMallocMemeError`. `take()` on an unset slot is `None`, so
        // this is the same two statements in the same order.
        unsafe {
            // Back to raw for the tree below the boundary, which is
            // `port-raw(Phase 9)` and takes both blocks as pointers.
            let pSrcPic: *const SSourcePicture = pSrcPic;
            let pBsInfo: *mut SFrameBSInfo = pBsInfo;

            let Some(pCtx) = self.m_pEncContext.as_deref_mut() else {
                crate::encoder::encoder_ext::WelsUninitEncoderExt(None);
                return cmMallocMemeError;
            };

            let kiBeforeFrameUs = WelsTime();
            let kiEncoderReturn =
                crate::encoder::encoder_ext::WelsEncoderEncodeExt(pCtx, pBsInfo, pSrcPic);
            let kiCurrentFrameMs = (WelsTime() - kiBeforeFrameUs) / 1000;

            if kiEncoderReturn == ENC_RETURN_MEMALLOCERR
                || kiEncoderReturn == ENC_RETURN_MEMOVERFLOWFOUND
                || kiEncoderReturn == ENC_RETURN_VLCOVERFLOWFOUND
            {
                crate::encoder::encoder_ext::WelsUninitEncoderExt(self.m_pEncContext.take());
                return cmMallocMemeError;
            } else if kiEncoderReturn == ENC_RETURN_INVALIDINPUT {
                return cmUnsupportedData;
            } else if kiEncoderReturn != ENC_RETURN_SUCCESS
                && kiEncoderReturn == ENC_RETURN_CORRECTED
            {
                return cmUnknownReason;
            }

            self.UpdateStatistics(&*pBsInfo, kiCurrentFrameMs);
        }

        cmResultSuccess
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn EncodeParameterSets(&mut self, pBsInfo: &mut SFrameBSInfo) -> i32 {
        let pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
        if pCtx.is_null() || !self.m_bInitialFlag {
            return cmInitParaError;
        }
        unsafe { WelsEncoderEncodeParameterSetsRust(&mut *pCtx, pBsInfo) }
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn ForceIntraFrame(&mut self, bIDR: bool, iLayerId: i32) -> i32 {
        if bIDR {
            if !self.m_bInitialFlag {
                return 1;
            }
            // T9.H5: `ForceCodingIDR` takes `&mut sWelsEncCtx`, so the context is
            // borrowed from the owning `Box` rather than flattened through
            // `ctx_ptr`. The `pCtx.is_null()` half of the old guard becomes the
            // `else` arm and answers the same `1`; `ctx_ptr` has no side effect,
            // so deriving it inside the `bIDR` arm rather than above it is the
            // same program.
            let Some(pCtx) = self.m_pEncContext.as_deref_mut() else {
                return 1;
            };
            unsafe {
                ForceCodingIDR(pCtx, iLayerId);
            }
        }
        0
    }

    /// `welsEncoderExt.cpp:1197` dumps the whole parameter block at
    /// `WELS_LOG_INFO`. Still a stub here; the encoder's remaining trace call
    /// sites are enumerated in the session log (T8.B6) and owned by Phase 9.
    pub fn TraceParamInfo(&mut self, _pParam: &SEncParamExt) {}

    pub fn LogStatistics(&mut self, _kiCurrentFrameTs: i64, _iMaxDid: i32) {}

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    pub fn UpdateStatistics(&mut self, pBsInfo: &SFrameBSInfo, kiCurrentFrameMs: i64) {
        let pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
        unsafe {
            if pCtx.is_null() || ctx_param(pCtx).is_null() {
                return;
            }
            let kiCurrentFrameTs = pBsInfo.uiTimeStamp;
            (*pCtx).uiLastTimestamp = kiCurrentFrameTs;
            let kiTimeDiff = kiCurrentFrameTs - (*pCtx).iLastStatisticsLogTs;

            let iMaxDid = (*ctx_param(pCtx)).iSpatialLayerNum - 1;
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
                    &mut (*pCtx).sEncoderStatistics[iDid as usize];
                let pSpatialLayerInternalParam =
                    &(*ctx_param(pCtx)).sDependencyLayers[iDid as usize];

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

                if (*pCtx).uiStartTimestamp != 0 {
                    if kiCurrentFrameTs > (*pCtx).uiStartTimestamp + 800 {
                        pStatistics.fAverageFrameRate = (pStatistics.uiInputFrameCount as f32
                            * 1000.0)
                            / ((kiCurrentFrameTs - (*pCtx).uiStartTimestamp) as f32);
                    }
                } else {
                    (*pCtx).uiStartTimestamp = kiCurrentFrameTs;
                }

                pStatistics.uiAverageFrameQP = if !ctx_rc(pCtx).is_null() {
                    (*ctx_rc_at(pCtx, iDid as usize)).iAverageFrameQp as u32
                } else {
                    26
                };

                if eFrameType == EVideoFrameType::videoFrameTypeIDR
                    || eFrameType == EVideoFrameType::videoFrameTypeI
                {
                    pStatistics.uiIDRSentNum += 1;
                }
                let pLtr = ctx_ltr(pCtx);
                if !pLtr.is_null() && (*pLtr).bLTRMarkingFlag {
                    pStatistics.uiLTRSentNum += 1;
                }

                pStatistics.iTotalEncodedBytes += kiCurrentFrameSize as u64;

                let kiDeltaFrames = (pStatistics.uiInputFrameCount
                    - pStatistics.iLastStatisticsFrameCount)
                    as i32;
                if kiDeltaFrames as f32
                    > (*ctx_param(pCtx)).fMaxFrameRate * 2.0
                {
                    if kiTimeDiff >= (*pCtx).iStatisticsLogInterval as i64 {
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
                        (*pCtx).iLastStatisticsLogTs = kiCurrentFrameTs;
                        self.LogStatistics(kiCurrentFrameTs, iMaxDid);
                        pStatistics.iTotalEncodedBytes = 0;
                    }
                }
            }
        }
    }

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    /// `pOption` is **C-ABI** and stays a `c_void` (T8.B10): its type is a function
    /// of `eOptionId` and of nothing else, over thirty-two ids, and no Rust type
    /// states that. `Encoder::set_option_raw` is the safe surface's `unsafe`
    /// spelling of the same obligation.
    pub fn SetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        // Re-derived after every arm that replaces the context — the three
        // `WelsEncoderParamAdjust` arms and `ENCODER_OPTION_LTR`.
        let mut pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
        if (pCtx.is_null() || !self.m_bInitialFlag)
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
                    if iValue == (*ctx_param(pCtx)).uiIntraPeriod as i32 {
                        return cmResultSuccess;
                    }
                    (*ctx_param(pCtx)).uiIntraPeriod = iValue as u32;
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
                    // T8.B5: `WelsEncoderParamAdjust` may replace the context
                    // (`encoder_ext.cpp`'s uninit/init pair), so the pointer derived
                    // at the top of `SetOption` no longer names this encoder's
                    // context. Re-derived from the slot the adjust just filled.
                    pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
                    // LogStatistics
                    let ts = (*pCtx).iLastStatisticsLogTs;
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
                    // T8.B5: `WelsEncoderParamAdjust` may replace the context
                    // (`encoder_ext.cpp`'s uninit/init pair), so the pointer derived
                    // at the top of `SetOption` no longer names this encoder's
                    // context. Re-derived from the slot the adjust just filled.
                    pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
                    // LogStatistics
                    let ts = (*pCtx).iLastStatisticsLogTs;
                    self.LogStatistics(ts, sEncodingParam.iSpatialLayerNum - 1);
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    let iValue = *(pOption as *const f32);
                    if iValue <= 0.0 {
                        return cmInitParaError;
                    }
                    (*ctx_param(pCtx)).fMaxFrameRate =
                        WELS_CLIP3(iValue, MIN_FRAME_RATE, MAX_FRAME_RATE);
                    WelsEncoderApplyFrameRate(ctx_param(pCtx));
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
                            (*ctx_param(pCtx)).iTargetBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
                            (*ctx_param(pCtx)).sSpatialLayers[pInfo.iLayer as usize]
                                .iSpatialBitrate = iBitrate;
                        }
                        _ => return cmInitParaError,
                    }
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    if WelsEncoderApplyBitRate(log_ctx, ctx_param(pCtx), pInfo.iLayer as i32)
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
                            (*ctx_param(pCtx)).iMaxBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
                            (*ctx_param(pCtx)).sSpatialLayers[pInfo.iLayer as usize]
                                .iMaxSpatialBitrate = iBitrate;
                        }
                        _ => return cmInitParaError,
                    }
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    if WelsEncoderApplyBitRate(log_ctx, ctx_param(pCtx), pInfo.iLayer as i32)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_RC_MODE => {
                    // 0:quality mode;1:bit-rate mode;2:bitrate limited mode
                    let iValue = *(pOption as *const i32);
                    (*ctx_param(pCtx)).iRCMode = rc_mode_from_raw(iValue);
                    // Re-point the dispatch table. Setting the field alone leaves
                    // the encoder running the previous mode's callbacks.
                    let iRCMode = (*ctx_param(pCtx)).iRCMode;
                    WelsRcInitFuncPointers(
                        &mut (*ctx_func_list(pCtx)).pfRc,
                        iRCMode,
                    );
                }
                EncoderOption::ENCODER_OPTION_RC_FRAME_SKIP => {
                    // 0:FRAME-SKIP disabled;1:FRAME-SKIP enabled
                    let bValue = *(pOption as *const bool);
                    if (*ctx_param(pCtx)).iRCMode != RC_OFF_MODE {
                        (*ctx_param(pCtx)).bEnableFrameSkip = bValue;
                    }
                    // rc off: the setting is accepted and ignored, as in C++.
                }
                EncoderOption::ENCODER_PADDING_PADDING => {
                    // 0:disable padding;1:padding
                    let iValue = *(pOption as *const i32);
                    (*ctx_param(pCtx)).iPaddingFlag = iValue;
                }
                EncoderOption::ENCODER_LTR_RECOVERY_REQUEST => {
                    let pLTR_Recover_Request = pOption as *mut SLTRRecoverRequest;
                    FilterLTRRecoveryRequest(&mut *pCtx, pLTR_Recover_Request);
                }
                EncoderOption::ENCODER_LTR_MARKING_FEEDBACK => {
                    let fb = pOption as *mut SLTRMarkingFeedback;
                    FilterLTRMarkingFeedback(&mut *pCtx, fb);
                }
                EncoderOption::ENCODER_LTR_MARKING_PERIOD => {
                    let iValue = *(pOption as *const u32);
                    (*ctx_param(pCtx)).iLtrMarkPeriod = iValue;
                }
                EncoderOption::ENCODER_OPTION_LTR => {
                    let pLTRValue = pOption as *mut SLTRConfig;
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    if WelsEncoderApplyLTR(log_ctx, &mut self.m_pEncContext, pLTRValue) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: the context may be a different allocation from here
                    // on — `WelsEncoderApplyLTR` runs the uninit/init pair — and
                    // this arm reads nothing after it, so there is nothing to
                    // re-derive. The stale pointer is unreachable, not tolerated.
                }
                EncoderOption::ENCODER_OPTION_ENABLE_SSEI => {
                    let iValue = *(pOption as *const bool);
                    (*ctx_param(pCtx)).bEnableSSEI = iValue;
                }
                EncoderOption::ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING => {
                    let iValue = *(pOption as *const bool);
                    (*ctx_param(pCtx)).bPrefixNalAddingCtrl = iValue;
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

                    let eOld = (*ctx_param(pCtx)).eSpsPpsIdStrategy;
                    if ((eNewStrategy as i32 & SPS_LISTING as i32) != 0
                        || (eOld as i32 & SPS_LISTING as i32) != 0)
                        && eOld != eNewStrategy
                    {
                        // changing in the middle of call is NOT allowed for
                        // eSpsPpsIdStrategy > INCREASING_ID
                        return cmInitParaError;
                    }
                    let mut sConfig: SWelsSvcCodingParam =
                        *ctx_param(pCtx);
                    sConfig.eSpsPpsIdStrategy = eNewStrategy;

                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: as in `ENCODER_OPTION_LTR` — nothing below reads the
                    // context in this arm, so there is nothing to re-derive.
                }
                EncoderOption::ENCODER_OPTION_CURRENT_PATH => {
                    if !ctx_param(pCtx).is_null() {
                        let path = pOption as *mut c_char;
                        (*ctx_param(pCtx)).pCurPath = path;
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
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    CheckProfileSetting(
                        log_ctx,
                        ctx_param(pCtx),
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
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    CheckLevelSetting(
                        log_ctx,
                        ctx_param(pCtx),
                        pLevelInfo.iLayer as i32,
                        pLevelInfo.uiLevelIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_NUMBER_REF => {
                    let iValue = *(pOption as *const i32);
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    CheckReferenceNumSetting(log_ctx, ctx_param(pCtx), iValue);
                }
                EncoderOption::ENCODER_OPTION_DELIVERY_STATUS => {
                    let pValue = &*(pOption as *const SDeliveryStatus);
                    (*pCtx).bDeliveryFlag = pValue.bDeliveryFlag;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    let iValue = *(pOption as *const i32);
                    (*ctx_param(pCtx)).iComplexityMode = match iValue {
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
                    (*pCtx).iStatisticsLogInterval = iValue;
                }
                EncoderOption::ENCODER_OPTION_IS_LOSSLESS_LINK => {
                    let bValue = *(pOption as *const bool);
                    (*ctx_param(pCtx)).bIsLosslessLink = bValue;
                }
                EncoderOption::ENCODER_OPTION_BITS_VARY_PERCENTAGE => {
                    let iValue = *(pOption as *const i32);
                    (*ctx_param(pCtx)).iBitsVaryPercentage =
                        WELS_CLIP3(iValue, 0, 100);
                    let log_ctx = std::ptr::addr_of_mut!(self.m_pWelsTrace.m_sLogCtx);
                    let iRang = (*ctx_param(pCtx)).iBitsVaryPercentage;
                    WelsEncoderApplyBitVaryRang(
                        log_ctx,
                        ctx_param(pCtx),
                        iRang,
                    );
                }
                EncoderOption::ENCODER_OPTION_TRACE_LEVEL => {
                    let level = pOption.cast::<u32>().read();
                    self.m_pWelsTrace.SetTraceLevel(level);
                    self.sync_log_ctx();
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK => {
                    let callback = pOption.cast::<WelsTraceCallback>().read();
                    self.m_pWelsTrace.SetTraceCallback(callback);
                    self.sync_log_ctx();
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT => {
                    // **C-ABI**: the caller's opaque trace context, kept until it
                    // is replaced and handed back to the callback untouched. Never
                    // dereferenced by this crate.
                    let ctx = pOption.cast::<*mut c_void>().read();
                    self.m_pWelsTrace.SetTraceCallbackContext(ctx);
                    self.sync_log_ctx();
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

    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    /// `pOption` is **C-ABI**, as in [`Self::SetOption`], with the blob written.
    pub fn GetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        let pCtx = Self::ctx_ptr(&mut self.m_pEncContext);
        if pCtx.is_null() || !self.m_bInitialFlag {
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
                        (*ctx_param(pCtx)).uiIntraPeriod as i32;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let param_ext = (*ctx_param(pCtx)).to_param_ext();
                    *(pOption as *mut SEncParamExt) = param_ext;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE => {
                    (*ctx_param(pCtx))
                        .GetBaseParams(&mut *(pOption as *mut SEncParamBase));
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    *(pOption as *mut f32) = (*ctx_param(pCtx)).fMaxFrameRate;
                }
                EncoderOption::ENCODER_OPTION_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = (*ctx_param(pCtx)).iTargetBitrate;
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = (*ctx_param(pCtx)).sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = (*ctx_param(pCtx)).iMaxBitrate;
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = (*ctx_param(pCtx)).sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iMaxSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    let pStatistics = &mut *(pOption as *mut crate::SEncoderStatistics);
                    let iLayerIdx =
                        ((*ctx_param(pCtx)).iSpatialLayerNum - 1) as usize;
                    let pEncStats = &(*pCtx).sEncoderStatistics[iLayerIdx];

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
                    *(pOption as *mut i32) = (*pCtx).iStatisticsLogInterval;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    *(pOption as *mut i32) =
                        (*ctx_param(pCtx)).iComplexityMode as i32;
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
        // T8.B5: the trace's `Box::from_raw` stood here. Both members are owned, so
        // this is the drop glue's work and the body is `Uninitialize`'s alone —
        // which the reference's destructor also calls (`welsEncoderExt.cpp`).
        self.Uninitialize();
    }
}

// **T8.B4 — the second encoder boundary stood here, and it was dead.**
//
// `G_ISVCENCODER_VTBL`, nine `ext_*` thunks duplicating `codec_api.rs`'s nine
// `encoder_*_c` bodies, and `WelsCreateSVCEncoderExt`/`WelsDestroySVCEncoderExt`:
// a whole parallel C-ABI surface over `CWelsH264SVCEncoder` itself, reached by
// casting `*mut ISVCEncoder` straight to `*mut CWelsH264SVCEncoder` because the
// struct opened with a `vptr` slot.
//
// Nothing called any of it. The two factories have no caller in the crate, in the
// tests, in the benches or in the diffharness; they are not among the seven names
// `codec_api.h` declares (there is no `WelsCreateSVCEncoderExt` upstream); and
// `vptr` itself was written once by the constructor and **never read** — the live
// boundary object is `CWelsH264SVCEncoderImpl { base, pVtbl, inner }`, whose
// vtable pointer is `base`'s and whose slots are the `*_c` thunks.
//
// So the encoder had two vtables, two factories and two sets of nine thunk bodies
// under one interface type, and only one of each was reachable. Deleted rather than
// carried into step 2's contracts, where writing a `# Safety` window for a slot
// nothing can call would have documented a fiction. **F78.**

pub static G_ST_CODEC_VERSION: OpenH264Version = OpenH264Version {
    uMajor: 2,
    uMinor: 6,
    uRevision: 0,
    uReserved: 0,
};

#[unsafe(no_mangle)]
// unsafe-cat: C-ABI
#[allow(unsafe_code)]
pub extern "C" fn WelsGetCodecVersion() -> OpenH264Version {
    G_ST_CODEC_VERSION
}

#[unsafe(no_mangle)]
// unsafe-cat: C-ABI
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsGetCodecVersionEx(pVersion: *mut OpenH264Version) {
    if !pVersion.is_null() {
        *pVersion = G_ST_CODEC_VERSION;
    }
}
