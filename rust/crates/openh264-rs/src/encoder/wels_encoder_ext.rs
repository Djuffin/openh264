#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

//! C++ SVC Encoder Facade and Lifecycle Controller (`CWelsH264SVCEncoder`).
//!
//! Translated from `codec/encoder/plus/inc/welsEncoderExt.h` and `codec/encoder/plus/src/welsEncoderExt.cpp`.

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

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
    SParaSetOffsetVariable, MAX_DQ_LAYER_NUM,
    MAX_PPS_COUNT, PARA_SET_TYPE,
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
/// `1 << (MAX_TEMPORAL_LEVEL - 1)` — wels_const.h:113.
pub const MAX_GOP_SIZE: u32 = 1 << (MAX_TEMPORAL_LEVEL - 1);
/// `MAX_GOP_SIZE >> 1` — wels_const.h:115.
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
// VIDEO_CODING_LAYER = 1.
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
// These are a bit field in C++.
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
// The six levels are `codec_app_def.h:323`'s.
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
/// **Deviation from the C++.** C++ casts the
/// caller's `int32_t` straight into the enum, so an out-of-range value is stored
/// verbatim; `WelsRcInitFuncPointers`'s switch has no `default`, so the dispatch
/// table is then left pointing at the previous mode's callbacks. A Rust enum
/// cannot hold a value outside its variants, so an unrecognised mode is left as
/// `RC_QUALITY_MODE` (`RC_MODES`' `#[default]`, and C++'s value 0). Every value
/// the reference actually accepts round-trips exactly.
#[inline]
pub(crate) fn rc_mode_from_raw(iValue: i32) -> RCMode {
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
    /// `EProfileIdc uiProfileIdc` — codec_app_def.h:693.
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
// codec/encoder/core/inc/param_svc.h).
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
pub fn WelsWriteOneSPS(pCtx: &mut sWelsEncCtx, kiSpsIdx: i32, iNalSize: &mut i32) -> i32 {
    let iNal = pCtx.out().iNalIndex;
    crate::encoder::nal_encap::WelsLoadNal(
        pCtx.out_mut(),
        crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SPS as i32,
        NRI_PRI_HIGHEST as i32,
    );

    let sSps = pCtx.sps_array()[kiSpsIdx as usize];
    {
        let (strategy, pOut) = crate::encoder::paraset_strategy::ctx_strategy_and_out(pCtx);
        let pSpsIdOffsetList = strategy.GetSpsIdOffsetList(PARA_SET_TYPE_AVCSPS as i32);
        WelsWriteSpsNal(
            &mut pOut.sBsBuffer[..],
            &sSps,
            &mut pOut.sBsWrite,
            pSpsIdOffsetList,
        );
    }
    crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

    let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
    let kpOut = pOut.as_deref().expect("pOut lives");
    let kiPos = *iPosBsBuffer as usize;
    let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &kpOut.sNalList[iNal as usize],
        &kpOut.sBsBuffer[..],
        None,
        pDstTail,
        &mut *iNalSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    pCtx.iPosBsBuffer += *iNalSize;
    ENC_RETURN_SUCCESS
}

/// `WelsWriteOnePPS` — encoder_ext.cpp:2849.
pub fn WelsWriteOnePPS(pCtx: &mut sWelsEncCtx, kiPpsIdx: i32, iNalSize: &mut i32) -> i32 {
    let iNal = pCtx.out().iNalIndex;
    /* generate picture parameter set */
    crate::encoder::nal_encap::WelsLoadNal(
        pCtx.out_mut(),
        crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_PPS as i32,
        NRI_PRI_HIGHEST as i32,
    );

    let sPps = pCtx.pps_array()[kiPpsIdx as usize];
    {
        let (pStrategy, pOut) = crate::encoder::paraset_strategy::ctx_strategy_and_out(pCtx);
        WelsWritePpsSyntax(
            &mut pOut.sBsBuffer[..],
            &sPps,
            &mut pOut.sBsWrite,
            pStrategy,
        );
    }
    crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

    let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
    let kpOut = pOut.as_deref().expect("pOut lives");
    let kiPos = *iPosBsBuffer as usize;
    let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
    let iReturn = crate::encoder::nal_encap::WelsEncodeNal(
        &kpOut.sNalList[iNal as usize],
        &kpOut.sBsBuffer[..],
        None,
        pDstTail,
        &mut *iNalSize,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    pCtx.iPosBsBuffer += *iNalSize;
    ENC_RETURN_SUCCESS
}

/// `WelsWriteParameterSets` — encoder_ext.cpp:2874. Writes every SPS, subset SPS and
/// PPS the context holds.
///
/// Note the loops are bounded by `iSpsNum`/`iSubsetSpsNum`/`iPpsNum`, so an
/// unpopulated context writes **nothing**.
pub fn WelsWriteParameterSets(
    pCtx: &mut sWelsEncCtx,
    pNumNal: &mut i32,
    pTotalLength: &mut i32,
) -> i32 {
    let mut iSize = 0i32;
    let mut iNal: i32;
    let mut iIdx: i32;
    let mut iId: i32;
    let mut iCountNal = 0i32;
    let mut iNalLength = 0i32;
    let mut iReturn;

    if pCtx.func_list().pParametersetStrategy.is_none() {
        return ENC_RETURN_UNEXPECTED;
    }

    *pTotalLength = 0;
    /* write all SPS */
    iIdx = 0;
    while iIdx < pCtx.iSpsNum {
        let uiSpsId = pCtx.sps_array()[iIdx as usize].uiSpsId;
        ParasetStrategy(pCtx).Update(uiSpsId, PARA_SET_TYPE_AVCSPS as i32);
        /* generate sequence parameters set */
        iId = ParasetStrategy(pCtx).GetSpsIdx(iIdx);

        WelsWriteOneSPS(pCtx, iId, &mut iNalLength);

        pCtx.out().set_nal_len_at(iCountNal as usize, iNalLength);
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    /* write all Subset SPS */
    iIdx = 0;
    while iIdx < pCtx.iSubsetSpsNum {
        iNal = pCtx.out().iNalIndex;

        let uiSpsId = pCtx.subset_array()[iIdx as usize].pSps.uiSpsId;
        ParasetStrategy(pCtx).Update(uiSpsId, PARA_SET_TYPE_SUBSETSPS as i32);

        iId = iIdx;

        /* generate Subset SPS */
        crate::encoder::nal_encap::WelsLoadNal(
            pCtx.out_mut(),
            crate::encoder::nal_encap::EWelsNalUnitType::NAL_UNIT_SUBSET_SPS as i32,
            NRI_PRI_HIGHEST as i32,
        );

        let sSubsetSps = pCtx.subset_array()[iId as usize];
        {
            let (strategy, pOut) = crate::encoder::paraset_strategy::ctx_strategy_and_out(pCtx);
            let pSpsIdOffsetList = strategy.GetSpsIdOffsetList(PARA_SET_TYPE_SUBSETSPS as i32);
            WelsWriteSubsetSpsSyntax(
                &mut pOut.sBsBuffer[..],
                &sSubsetSps,
                &mut pOut.sBsWrite,
                pSpsIdOffsetList,
            );
        }
        crate::encoder::nal_encap::WelsUnloadNal(pCtx.out_mut());

        let sWelsEncCtx { pOut, pFrameBs, iPosBsBuffer, .. } = &mut *pCtx;
        let kpOut = pOut.as_deref().expect("pOut lives");
        let kiPos = *iPosBsBuffer as usize;
        let pDstTail = (kiPos <= pFrameBs.len()).then(|| &mut pFrameBs[kiPos..]);
        iReturn = crate::encoder::nal_encap::WelsEncodeNal(
            &kpOut.sNalList[iNal as usize],
            &kpOut.sBsBuffer[..],
            None,
            pDstTail,
            &mut iNalLength,
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        pCtx.out().set_nal_len_at(iCountNal as usize, iNalLength);

        pCtx.iPosBsBuffer += iNalLength;
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    {
        let (strategy, pps, pPpsNum) =
            crate::encoder::paraset_strategy::ctx_strategy_and_pps(pCtx);
        strategy.UpdatePpsList(pps, pPpsNum);
    }

    iIdx = 0;
    while iIdx < pCtx.iPpsNum {
        let iPpsId = pCtx.pps_array()[iIdx as usize].iPpsId;
        ParasetStrategy(pCtx).Update(iPpsId, PARA_SET_TYPE_PPS as i32);

        WelsWriteOnePPS(pCtx, iIdx, &mut iNalLength);

        pCtx.out().set_nal_len_at(iCountNal as usize, iNalLength);
        iSize += iNalLength;

        iIdx += 1;
        iCountNal += 1;
    }

    *pNumNal = iCountNal;
    *pTotalLength = iSize;

    ENC_RETURN_SUCCESS
}

pub fn WelsEncoderEncodeParameterSetsRust(
    pCtx: &mut sWelsEncCtx,
    pBsInfo: &mut SFrameBSInfo,
) -> i32 {
    let pLayerBsInfo = &mut pBsInfo.sLayerInfo[0];
    pLayerBsInfo.pBsBuf = pCtx.frame_bs();
    {
        // The frame's first layer starts at entry 0 of `pOut.sNalLen`;
        // the ABI pointer is that position, resliced.
        let pOut = pCtx.out_mut();
        pOut.iNalLenBase = 0;
        pLayerBsInfo.pNalLengthInByte = pOut.nal_len_ptr();
    }
    pCtx.out_mut().sBsWrite = crate::encoder::vlc_encoder::BsWriter::new();
    pCtx.iPosBsBuffer = 0;

    let mut iCountNal = 0;
    let mut iTotalLength = 0;
    let ret = WelsWriteParameterSets(pCtx, &mut iCountNal, &mut iTotalLength);
    if ret != ENC_RETURN_SUCCESS {
        return ret;
    }

    pLayerBsInfo.uiSpatialId = 0;
    pLayerBsInfo.uiTemporalId = 0;
    pLayerBsInfo.uiQualityId = 0;
    pLayerBsInfo.uiLayerType = NON_VIDEO_CODING_LAYER;
    pLayerBsInfo.iNalCount = iCountNal;
    pBsInfo.iLayerNum = 1;
    pBsInfo.eFrameType = EVideoFrameType::videoFrameTypeInvalid;

    ENC_RETURN_SUCCESS
}

/// `ForceCodingIDR` — `encoder_ext.cpp:3046`.
///
/// The reference's two arms differ only in *which* dependency layers they reset:
/// all of them unless simulcast-AVC is on and the caller named a valid one. Both
/// reset the same five fields and bump the same counter, so the loop below is
/// written once over the layer range each arm selects.
pub fn ForceCodingIDR(pCtx: &mut sWelsEncCtx, iLayerId: i32) -> i32 {
    let Some((bSimulcastAVC, iSpatialLayerNum)) = pCtx
        .param_opt()
        .map(|p| (p.bSimulcastAVC, p.iSpatialLayerNum))
    else {
        return 1;
    };
    let all_layers = iLayerId < 0
        || iLayerId >= crate::encoder::param_svc::MAX_SPATIAL_LAYER_NUM as i32
        || !bSimulcastAVC;
    let (first, last) = if all_layers {
        (0, iSpatialLayerNum)
    } else {
        (iLayerId, iLayerId + 1)
    };
    for iDid in first..last {
        {
            let pParamInternal = &mut pCtx.param_mut().sDependencyLayers[iDid as usize];
            pParamInternal.iCodingIndex = 0;
            pParamInternal.iFrameIndex = 0;
            pParamInternal.iFrameNum = 0;
            pParamInternal.iPOC = 0;
            pParamInternal.bEncCurFrmAsIdrFlag = true;
        }
        // The reference counts the request against layer **0** in the all-layers arm
        // and against `iLayerId` in the other — `sEncoderStatistics[0]` inside the
        // loop, not `sEncoderStatistics[iDid]`.
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
pub fn WelsEncoderParamAdjust(
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pNewParam: &mut SWelsSvcCodingParam,
) -> i32 {
    const EPSN: f32 = 0.000001;
    let mut iReturn;
    let mut iIndexD: i32;
    let mut bNeedReset: bool;
    let mut iSliceNum: i16 = 1; // number of slices used
    let mut iCacheLineSize: i32 = 16; // on chip cache line size in byte
    let mut uiCpuFeatureFlags: u32 = 0;

    let ctx = match ppCtx.as_deref_mut() {
        Some(pEncContext) => pEncContext,
        None => return 1,
    };

    /* Check validation in new parameters */
    iReturn = ParamValidationExt(ctx.sLogCtx, &mut *pNewParam);
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    iReturn = GetMultipleThreadIdc(
        ctx.sLogCtx,
        pNewParam,
        &mut iSliceNum,
        &mut iCacheLineSize,
        &mut uiCpuFeatureFlags,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return iReturn;
    }

    let pOldParam: &mut SWelsSvcCodingParam = ctx.param_mut();

    if pOldParam.iUsageType != pNewParam.iUsageType {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    /* Decide whether need reset for IDR frame based on adjusting prarameters changed */
    /* Temporal levels, spatial settings and/ or quality settings changed need update parameter sets related. */
    bNeedReset = (pOldParam.bSimulcastAVC != pNewParam.bSimulcastAVC)
        || (pOldParam.iSpatialLayerNum != pNewParam.iSpatialLayerNum)
        || (pOldParam.iPicWidth != pNewParam.iPicWidth
            || pOldParam.iPicHeight != pNewParam.iPicHeight)
        || (pOldParam.SUsedPicRect.iWidth != pNewParam.SUsedPicRect.iWidth
            || pOldParam.SUsedPicRect.iHeight != pNewParam.SUsedPicRect.iHeight)
        || (pOldParam.bEnableLongTermReference != pNewParam.bEnableLongTermReference)
        || (pOldParam.iLTRRefNum != pNewParam.iLTRRefNum)
        || (pOldParam.iMultipleThreadIdc != pNewParam.iMultipleThreadIdc)
        || (pOldParam.bEnableBackgroundDetection != pNewParam.bEnableBackgroundDetection)
        || (pOldParam.bEnableAdaptiveQuant != pNewParam.bEnableAdaptiveQuant)
        || (pOldParam.eSpsPpsIdStrategy != pNewParam.eSpsPpsIdStrategy);
    if (pNewParam.iMaxNumRefFrame > pOldParam.iMaxNumRefFrame)
        || (pOldParam.iMaxNumRefFrame == 1
            && pOldParam.iTemporalLayerNum == 1
            && pNewParam.iTemporalLayerNum == 2)
    {
        bNeedReset = true;
    }
    if !bNeedReset {
        // Check its picture resolutions/quality settings respectively in each dependency layer
        iIndexD = 0;
        debug_assert!(pOldParam.iSpatialLayerNum == pNewParam.iSpatialLayerNum);
        loop {
            let d = iIndexD as usize;
            let kpOldDlp = &pOldParam.sDependencyLayers[d];
            let kpNewDlp = &pNewParam.sDependencyLayers[d];
            let mut fT1: f32 = 0.0;
            let mut fT2: f32 = 0.0;

            // check frame size settings
            if pOldParam.sSpatialLayers[d].iVideoWidth != pNewParam.sSpatialLayers[d].iVideoWidth
                || pOldParam.sSpatialLayers[d].iVideoHeight
                    != pNewParam.sSpatialLayers[d].iVideoHeight
                || kpOldDlp.iActualWidth != kpNewDlp.iActualWidth
                || kpOldDlp.iActualHeight != kpNewDlp.iActualHeight
            {
                bNeedReset = true;
                break;
            }

            if pOldParam.sSpatialLayers[d].sSliceArgument.uiSliceMode
                != pNewParam.sSpatialLayers[d].sSliceArgument.uiSliceMode
                || pOldParam.sSpatialLayers[d].sSliceArgument.uiSliceNum
                    != pNewParam.sSpatialLayers[d].sSliceArgument.uiSliceNum
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
                fT2 = pNewParam.fMaxFrameRate / kpNewDlp.fOutputFrameRate
                    - pOldParam.fMaxFrameRate / kpOldDlp.fOutputFrameRate;
            }
            if fT1 > EPSN || fT1 < -EPSN || fT2 > EPSN || fT2 < -EPSN {
                bNeedReset = true;
                break;
            }
            if pOldParam.sSpatialLayers[d].uiProfileIdc
                != pNewParam.sSpatialLayers[d].uiProfileIdc
            {
                bNeedReset = true;
                break;
            }
            // check level change, if new level is smaller than old level, don't reset
            // encoder. still use old level.
            if pNewParam.sSpatialLayers[d].uiLevelIdc as i32
                > pOldParam.sSpatialLayers[d].uiLevelIdc as i32
            {
                bNeedReset = true;
                break;
            }
            iIndexD += 1;
            if iIndexD >= pOldParam.iSpatialLayerNum {
                break;
            }
        }
    }

    if bNeedReset {
        let iOldSpsPpsIdStrategy = pOldParam.eSpsPpsIdStrategy;
        let mut sTmpPsoVariable: [SParaSetOffsetVariable; PARA_SET_TYPE] = Default::default();
        let mut iTmpPpsIdList: [i32; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT] =
            [0; MAX_DQ_LAYER_NUM * MAX_PPS_COUNT];
        // for LTR or SPS,PPS ID update
        let mut uiMaxIdrPicId: u16 = 0;
        iIndexD = 0;
        while iIndexD < pOldParam.iSpatialLayerNum {
            if pOldParam.sDependencyLayers[iIndexD as usize].uiIdrPicId > uiMaxIdrPicId {
                uiMaxIdrPicId = pOldParam.sDependencyLayers[iIndexD as usize].uiIdrPicId;
            }
            iIndexD += 1;
        }

        let sLogCtx = ctx.sLogCtx;

        // for sEncoderStatistics
        let sTempEncoderStatistics = ctx.sEncoderStatistics;
        let uiStartTimestamp = ctx.uiStartTimestamp;
        let iStatisticsLogInterval = ctx.iStatisticsLogInterval;
        let iLastStatisticsLogTs = ctx.iLastStatisticsLogTs;
        // for sEncoderStatistics

        let mut sExistingParasetList = SExistingParasetList::default();
        let mut bHaveExistingParasetList = false;

        if iOldSpsPpsIdStrategy != CONSTANT_ID && pNewParam.eSpsPpsIdStrategy != CONSTANT_ID {
            let (strategy, pSpsArray, pSubsetArray, pPpsArray) =
                crate::encoder::paraset_strategy::ctx_strategy_and_paraset_arrays(ctx);
            strategy.OutputCurrentStructure(
                &mut sTmpPsoVariable,
                &mut iTmpPpsIdList,
                pSpsArray,
                pSubsetArray,
                pPpsArray,
                Some(&mut sExistingParasetList),
            );

            if (iOldSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
                && (pNewParam.eSpsPpsIdStrategy as i32 & SPS_LISTING as i32) != 0
            {
                bHaveExistingParasetList = true;
            }
        }

        WelsUninitEncoderExt(ppCtx.take());

        /* Update new parameters */
        let pExistingParasetList =
            bHaveExistingParasetList.then_some(&sExistingParasetList);
        if WelsInitEncoderExt(ppCtx, pNewParam, sLogCtx, pExistingParasetList) != 0 {
            return 1;
        }
        // The context below this line is a different allocation from the one above
        // it.
        let ctx = match ppCtx.as_deref_mut() {
            Some(pEncContext) => pEncContext,
            None => return 1,
        };
        // if WelsInitEncoderExt succeed
        // for LTR or SPS,PPS ID update
        iIndexD = 0;
        while iIndexD < pNewParam.iSpatialLayerNum {
            ctx.param_mut().sDependencyLayers[iIndexD as usize].uiIdrPicId = uiMaxIdrPicId;
            iIndexD += 1;
        }

        // for sEncoderStatistics
        ctx.sEncoderStatistics = sTempEncoderStatistics;
        ctx.uiStartTimestamp = uiStartTimestamp;
        ctx.iStatisticsLogInterval = iStatisticsLogInterval;
        ctx.iLastStatisticsLogTs = iLastStatisticsLogTs;
        // for sEncoderStatistics

        // load back the needed structure for eSpsPpsIdStrategy
        if (iOldSpsPpsIdStrategy != CONSTANT_ID && pNewParam.eSpsPpsIdStrategy != CONSTANT_ID)
            || (iOldSpsPpsIdStrategy == SPS_PPS_LISTING
                && pNewParam.eSpsPpsIdStrategy == SPS_PPS_LISTING)
        {
            ParasetStrategy(ctx).LoadPreviousStructure(
                &sTmpPsoVariable,
                &mut iTmpPpsIdList,
            );
        }
    } else {
        /* maybe adjustment introduced in bitrate or little settings adjustment and so on.. */
        pNewParam.iNumRefFrame = WELS_CLIP3(
            pNewParam.iNumRefFrame,
            MIN_REF_PIC_COUNT,
            if pNewParam.iUsageType == CAMERA_VIDEO_REAL_TIME {
                MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
            } else {
                MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
            },
        );
        pNewParam.iLoopFilterDisableIdc = WELS_CLIP3(pNewParam.iLoopFilterDisableIdc, 0, 6);
        pNewParam.iLoopFilterAlphaC0Offset =
            WELS_CLIP3(pNewParam.iLoopFilterAlphaC0Offset, -6, 6);
        pNewParam.iLoopFilterBetaOffset = WELS_CLIP3(pNewParam.iLoopFilterBetaOffset, -6, 6);
        pNewParam.fMaxFrameRate =
            WELS_CLIP3(pNewParam.fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);

        // we can not use direct struct based memcpy due some fields need keep unchanged as before
        pOldParam.fMaxFrameRate = pNewParam.fMaxFrameRate;
        pOldParam.iComplexityMode = pNewParam.iComplexityMode;
        pOldParam.uiIntraPeriod = pNewParam.uiIntraPeriod;
        pOldParam.eSpsPpsIdStrategy = pNewParam.eSpsPpsIdStrategy;
        pOldParam.bPrefixNalAddingCtrl = pNewParam.bPrefixNalAddingCtrl;
        pOldParam.iNumRefFrame = pNewParam.iNumRefFrame;
        pOldParam.uiGopSize = pNewParam.uiGopSize;
        if pOldParam.iTemporalLayerNum != pNewParam.iTemporalLayerNum {
            pOldParam.iTemporalLayerNum = pNewParam.iTemporalLayerNum;
            for d in 0..MAX_DEPENDENCY_LAYER as usize {
                pOldParam.sDependencyLayers[d].iCodingIndex = 0;
            }
        }
        pOldParam.iDecompStages = pNewParam.iDecompStages;
        /* denoise control */
        pOldParam.bEnableDenoise = pNewParam.bEnableDenoise;

        /* background detection control */
        pOldParam.bEnableBackgroundDetection = pNewParam.bEnableBackgroundDetection;

        /* adaptive quantization control */
        pOldParam.bEnableAdaptiveQuant = pNewParam.bEnableAdaptiveQuant;

        /* int32_t term reference control */
        pOldParam.bEnableLongTermReference = pNewParam.bEnableLongTermReference;
        pOldParam.iLtrMarkPeriod = pNewParam.iLtrMarkPeriod;

        // keep below values unchanged as before
        pOldParam.bEnableSSEI = pNewParam.bEnableSSEI;
        pOldParam.bSimulcastAVC = pNewParam.bSimulcastAVC;
        pOldParam.bEnableFrameCroppingFlag = pNewParam.bEnableFrameCroppingFlag;

        /* Motion search */

        /* Deblocking loop filter */
        pOldParam.iLoopFilterDisableIdc = pNewParam.iLoopFilterDisableIdc;
        pOldParam.iLoopFilterAlphaC0Offset = pNewParam.iLoopFilterAlphaC0Offset;
        pOldParam.iLoopFilterBetaOffset = pNewParam.iLoopFilterBetaOffset;

        /* Rate Control */
        pOldParam.iRCMode = pNewParam.iRCMode;
        pOldParam.iTargetBitrate = pNewParam.iTargetBitrate;
        pOldParam.iPaddingFlag = pNewParam.iPaddingFlag;

        /* Layer definition */
        pOldParam.bPrefixNalAddingCtrl = pNewParam.bPrefixNalAddingCtrl;

        // d
        iIndexD = 0;
        loop {
            let d = iIndexD as usize;
            pOldParam.sDependencyLayers[d].fInputFrameRate =
                pNewParam.sDependencyLayers[d].fInputFrameRate;
            pOldParam.sDependencyLayers[d].fOutputFrameRate =
                pNewParam.sDependencyLayers[d].fOutputFrameRate;
            pOldParam.sSpatialLayers[d].iSpatialBitrate =
                pNewParam.sSpatialLayers[d].iSpatialBitrate;
            pOldParam.sSpatialLayers[d].iMaxSpatialBitrate =
                pNewParam.sSpatialLayers[d].iMaxSpatialBitrate;
            pOldParam.sSpatialLayers[d].uiProfileIdc =
                pNewParam.sSpatialLayers[d].uiProfileIdc;
            pOldParam.sSpatialLayers[d].iDLayerQp = pNewParam.sSpatialLayers[d].iDLayerQp;

            /* Derived variants below */
            pOldParam.sDependencyLayers[d].iTemporalResolution =
                pNewParam.sDependencyLayers[d].iTemporalResolution;
            pOldParam.sDependencyLayers[d].iDecompositionStages =
                pNewParam.sDependencyLayers[d].iDecompositionStages;
            pOldParam.sDependencyLayers[d].uiCodingIdx2TemporalId =
                pNewParam.sDependencyLayers[d].uiCodingIdx2TemporalId;
            iIndexD += 1;
            if iIndexD >= pOldParam.iSpatialLayerNum {
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
pub fn WelsEncoderApplyFrameRate(pParam: &mut SWelsSvcCodingParam) {
    const kfEpsn: f32 = 0.000001;
    let kiNumLayer = pParam.iSpatialLayerNum;
    let kfMaxFrameRate = pParam.fMaxFrameRate;

    // set input frame rate to each layer
    for i in 0..kiNumLayer as usize {
        let pLayerParamInternal = &mut pParam.sDependencyLayers[i];
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
            pParam.sSpatialLayers[i].fFrameRate = fOut;
        }
    }
}

/// `WelsEncoderApplyBitRate` — codec/encoder/core/src/encoder_ext.cpp:699.
///
/// `SPATIAL_LAYER_ALL` re-splits `iTargetBitrate` across the layers in the ratio
/// they already held; a single layer id only re-verifies that layer.
pub fn WelsEncoderApplyBitRate(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    iLayer: i32,
) -> i32 {
    let iNumLayers = pParam.iSpatialLayerNum;
    let mut iOrigTotalBitrate = 0i32;
    if iLayer == SPATIAL_LAYER_ALL as i32 {
        // read old BR
        for i in 0..iNumLayers as usize {
            iOrigTotalBitrate += pParam.sSpatialLayers[i].iSpatialBitrate;
        }
        // write new BR
        for i in 0..iNumLayers as usize {
            let pLayerParam = &mut pParam.sSpatialLayers[i];
            let fRatio = pLayerParam.iSpatialBitrate as f32 / iOrigTotalBitrate as f32;
            pLayerParam.iSpatialBitrate = (pParam.iTargetBitrate as f32 * fRatio) as i32;

            if WelsBitRateVerification(pLogCtx, pLayerParam, i as i32) != ENC_RETURN_SUCCESS {
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
        }
    } else {
        return WelsBitRateVerification(
            pLogCtx,
            &mut pParam.sSpatialLayers[iLayer as usize],
            iLayer,
        );
    }
    ENC_RETURN_SUCCESS
}

/// `WelsEncoderApplyLTR` — codec/encoder/core/src/encoder_ext.cpp:4479.
///
/// Derives the reference-frame count the requested LTR setting needs, raises
/// `iMaxNumRefFrame`/`iNumRefFrame` to reach it, and re-adjusts the encoder.
pub fn WelsEncoderApplyLTR(
    pLogCtx: SLogContext,
    ppCtx: &mut Option<Box<sWelsEncCtx>>,
    pLTRValue: &mut SLTRConfig,
) -> i32 {
    let mut sConfig: SWelsSvcCodingParam = match ppCtx.as_mut() {
        Some(pEncContext) => pEncContext.param().clone(),
        None => return 1,
    };
    let mut iNumRefFrame;
    sConfig.bEnableLongTermReference = pLTRValue.bEnableLongTermReference;
    sConfig.iLTRRefNum = pLTRValue.iLTRRefNum;
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
pub fn ParamValidation(pLogCtx: SLogContext, pCfg: &mut SWelsSvcCodingParam) -> i32 {
    const fEpsn: f32 = 0.000001;

    if !((pCfg.iUsageType as i32) < INPUT_CONTENT_TYPE_ALL as i32) {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCfg.iUsageType == SCREEN_CONTENT_REAL_TIME {
        if pCfg.iSpatialLayerNum > 1 {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }
        if pCfg.bEnableAdaptiveQuant {
            pCfg.bEnableAdaptiveQuant = false;
        }
        if pCfg.bEnableBackgroundDetection {
            pCfg.bEnableBackgroundDetection = false;
        }
        if !pCfg.bEnableSceneChangeDetect {
            pCfg.bEnableSceneChangeDetect = true;
        }
    }

    // turn off adaptive quant now, algorithms needs to be refactored
    pCfg.bEnableAdaptiveQuant = false;

    if pCfg.iSpatialLayerNum > 1 {
        let mut i = pCfg.iSpatialLayerNum - 1;
        while i > 0 {
            let fDlpUp = pCfg.sSpatialLayers[i as usize];
            let fDlp = pCfg.sSpatialLayers[(i - 1) as usize];
            if fDlp.iVideoWidth > fDlpUp.iVideoWidth || fDlp.iVideoHeight > fDlpUp.iVideoHeight {
                return ENC_RETURN_UNSUPPORTED_PARA;
            }
            i -= 1;
        }
    }

    if !CheckInRangeCloseOpen(
        pCfg.iLoopFilterDisableIdc as i16,
        DEBLOCKING_IDC_0 as i16,
        (DEBLOCKING_IDC_2 + 1) as i16,
    ) || !CheckInRangeCloseOpen(
        pCfg.iLoopFilterAlphaC0Offset as i16,
        DEBLOCKING_OFFSET_MINUS as i16,
        (DEBLOCKING_OFFSET + 1) as i16,
    ) || !CheckInRangeCloseOpen(
        pCfg.iLoopFilterBetaOffset as i16,
        DEBLOCKING_OFFSET_MINUS as i16,
        (DEBLOCKING_OFFSET + 1) as i16,
    ) {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    for i in 0..pCfg.iSpatialLayerNum as usize {
        let fInput = pCfg.sDependencyLayers[i].fInputFrameRate;
        let fOutput = pCfg.sDependencyLayers[i].fOutputFrameRate;
        if fOutput > fInput
            || (fInput >= -fEpsn && fInput <= fEpsn)
            || (fOutput >= -fEpsn && fOutput <= fEpsn)
        {
            return ENC_RETURN_INVALIDINPUT;
        }
        if GetLogFactor(fOutput, fInput) == u32::MAX {
            // AUTO CORRECT: output frame rate must be input/2^n
            pCfg.sDependencyLayers[i].fOutputFrameRate = fInput;
            pCfg.sSpatialLayers[i].fFrameRate = fInput;
        }
    }

    if pCfg.iRCMode != RC_OFF_MODE
        && pCfg.iRCMode != RC_QUALITY_MODE
        && pCfg.iRCMode != RC_BUFFERBASED_MODE
        && pCfg.iRCMode != RC_BITRATE_MODE
        && pCfg.iRCMode != RC_TIMESTAMP_MODE
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // bitrate setting validation
    if pCfg.iRCMode != RC_OFF_MODE {
        if pCfg.iTargetBitrate <= 0 {
            return ENC_RETURN_INVALIDINPUT;
        }
        let mut iTotalBitrate = 0i32;
        for i in 0..pCfg.iSpatialLayerNum as usize {
            iTotalBitrate += pCfg.sSpatialLayers[i].iSpatialBitrate;
            if WelsBitRateVerification(pLogCtx, &mut pCfg.sSpatialLayers[i], i as i32)
                != ENC_RETURN_SUCCESS
            {
                return ENC_RETURN_INVALIDINPUT;
            }
        }
        if iTotalBitrate > pCfg.iTargetBitrate {
            return ENC_RETURN_INVALIDINPUT;
        }
        // `encoder_ext.cpp:370-374` — log-only: the bitrate cannot actually be
        // held without frame skipping.
        if (pCfg.iRCMode == RC_QUALITY_MODE
            || pCfg.iRCMode == RC_BITRATE_MODE
            || pCfg.iRCMode == RC_TIMESTAMP_MODE)
            && !pCfg.bEnableFrameSkip
        {
            WelsLog(
                pLogCtx,
                WELS_LOG_WARNING,
                &format!(
                    "bEnableFrameSkip = {},bitrate can't be controlled for RC_QUALITY_MODE,RC_BITRATE_MODE and RC_TIMESTAMP_MODE without enabling skip frame.",
                    pCfg.bEnableFrameSkip as i32
                ),
            );
        }
        if pCfg.iMaxQp <= 0 || pCfg.iMinQp <= 0 {
            if pCfg.iUsageType == SCREEN_CONTENT_REAL_TIME {
                WelsLog(
                    pLogCtx,
                    WELS_LOG_INFO,
                    &format!(
                        "Change QP Range from({},{}) to ({},{})",
                        pCfg.iMinQp, pCfg.iMaxQp, MIN_SCREEN_QP, MAX_SCREEN_QP
                    ),
                );
                pCfg.iMinQp = MIN_SCREEN_QP;
                pCfg.iMaxQp = MAX_SCREEN_QP;
            } else {
                WelsLog(
                    pLogCtx,
                    WELS_LOG_INFO,
                    &format!(
                        "Change QP Range from({},{}) to ({},{})",
                        pCfg.iMinQp, pCfg.iMaxQp, GOM_MIN_QP_MODE, MAX_LOW_BR_QP
                    ),
                );
                pCfg.iMinQp = GOM_MIN_QP_MODE;
                pCfg.iMaxQp = MAX_LOW_BR_QP;
            }
        }
        pCfg.iMinQp = WELS_CLIP3(pCfg.iMinQp, GOM_MIN_QP_MODE, QP_MAX_VALUE);
        pCfg.iMaxQp = WELS_CLIP3(pCfg.iMaxQp, pCfg.iMinQp, QP_MAX_VALUE);
    }

    // ref-frames validation, encoder_ext.cpp:392-398
    let bRefLimitFailed = if pCfg.iUsageType == CAMERA_VIDEO_REAL_TIME
        || pCfg.iUsageType == SCREEN_CONTENT_REAL_TIME
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
/// All four slice modes are handled: `SM_FIXEDSLCNUM_SLICE` and `SM_RASTER_SLICE`
/// dispatch to `SliceArgumentValidationFixedSliceMode` /
/// `CheckRowMbMultiSliceSetting` / `CheckRasterMultiSliceSetting` in
/// `svc_enc_slice_segment.rs`.
///
/// The `WelsLog` calls that accompany each rejection in C++ have no counterpart
/// here — only the control flow and the returned code are reproduced.
pub fn ParamValidationExt(
    pLogCtx: SLogContext,
    pCodingParam: &mut SWelsSvcCodingParam,
) -> i32 {
    if pCodingParam.iUsageType != CAMERA_VIDEO_REAL_TIME
        && pCodingParam.iUsageType != SCREEN_CONTENT_REAL_TIME
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCodingParam.iUsageType == SCREEN_CONTENT_REAL_TIME
        && !pCodingParam.bIsLosslessLink
        && pCodingParam.bEnableLongTermReference
    {
        pCodingParam.bEnableLongTermReference = false;
    }
    if pCodingParam.iSpatialLayerNum < 1
        || pCodingParam.iSpatialLayerNum > MAX_DEPENDENCY_LAYER
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCodingParam.iTemporalLayerNum < 1
        || pCodingParam.iTemporalLayerNum > MAX_TEMPORAL_LEVEL
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCodingParam.uiGopSize < 1 || pCodingParam.uiGopSize > MAX_GOP_SIZE {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCodingParam.uiIntraPeriod != 0
        && pCodingParam.uiIntraPeriod < pCodingParam.uiGopSize
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }
    if pCodingParam.uiIntraPeriod != 0
        && (pCodingParam.uiIntraPeriod & (pCodingParam.uiGopSize - 1)) != 0
    {
        return ENC_RETURN_UNSUPPORTED_PARA;
    }

    // single thread => no parallel deblocking
    pCodingParam.bDeblockingParallelFlag = pCodingParam.iMultipleThreadIdc != 1;

    // eSpsPpsIdStrategy checkings — `encoder_ext.cpp:466-491`.
    //
    // The messages are the reference's, argument for argument — including the third
    // one's, which prints `eSpsPpsIdStrategy` where it says `bSimulcastAVC` and vice
    // versa (`encoder_ext.cpp:487-489`); reproduced rather than repaired, because a
    // consumer grepping its logs matches on text.
    let sps_listing = SPS_LISTING as i32;
    if pCodingParam.iSpatialLayerNum > 1
        && !pCodingParam.bSimulcastAVC
        && (sps_listing & pCodingParam.eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy setting ({}) with multiple svc SpatialLayers ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                pCodingParam.eSpsPpsIdStrategy as i32,
                pCodingParam.iSpatialLayerNum
            ),
        );
        pCodingParam.eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if pCodingParam.iUsageType == SCREEN_CONTENT_REAL_TIME
        && (sps_listing & pCodingParam.eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy setting ({}) with iUsageType ({}) not supported! eSpsPpsIdStrategy adjusted to CONSTANT_ID",
                pCodingParam.eSpsPpsIdStrategy as i32,
                pCodingParam.iUsageType as i32
            ),
        );
        pCodingParam.eSpsPpsIdStrategy = CONSTANT_ID;
    }
    if pCodingParam.bSimulcastAVC
        && (sps_listing & pCodingParam.eSpsPpsIdStrategy as i32) != 0
    {
        WelsLog(
            pLogCtx,
            WELS_LOG_INFO,
            &format!(
                "ParamValidationExt(), eSpsPpsIdStrategy({}) under bSimulcastAVC({}) not supported yet, adjusted to INCREASING_ID",
                pCodingParam.eSpsPpsIdStrategy as i32,
                pCodingParam.bSimulcastAVC as i32
            ),
        );
        pCodingParam.eSpsPpsIdStrategy = INCREASING_ID;
    }
    if pCodingParam.bSimulcastAVC && pCodingParam.bPrefixNalAddingCtrl {
        pCodingParam.bPrefixNalAddingCtrl = false;
    }

    for i in 0..pCodingParam.iSpatialLayerNum {
        let idx = i as usize;
        let mut kiPicWidth = pCodingParam.sSpatialLayers[idx].iVideoWidth;
        let mut kiPicHeight = pCodingParam.sSpatialLayers[idx].iVideoHeight;

        if pCodingParam.iPicWidth > 0
            && pCodingParam.iPicHeight > 0
            && kiPicWidth == 0
            && kiPicHeight == 0
            && pCodingParam.iSpatialLayerNum == 1
        {
            kiPicWidth = pCodingParam.iPicWidth;
            kiPicHeight = pCodingParam.iPicHeight;
            pCodingParam.sSpatialLayers[idx].iVideoWidth = kiPicWidth;
            pCodingParam.sSpatialLayers[idx].iVideoHeight = kiPicHeight;
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
        if pCodingParam.sSpatialLayers[idx].sSliceArgument.uiSliceMode as i32
            >= SM_RESERVED as i32
        {
            return ENC_RETURN_UNSUPPORTED_PARA;
        }

        let uiProfileIdc = pCodingParam.sSpatialLayers[idx].uiProfileIdc;
        let uiLevelIdc = pCodingParam.sSpatialLayers[idx].uiLevelIdc;
        CheckProfileSetting(pLogCtx, &mut *pCodingParam, i, uiProfileIdc);
        CheckLevelSetting(pLogCtx, &mut *pCodingParam, i, uiLevelIdc);

        // only one MB => single slice
        if kiPicWidth <= 16 && kiPicHeight <= 16 {
            pCodingParam.sSpatialLayers[idx].sSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
        }
        match pCodingParam.sSpatialLayers[idx].sSliceArgument.uiSliceMode {
            SM_SINGLE_SLICE => {
                let pSliceArgument = &mut pCodingParam.sSpatialLayers[idx].sSliceArgument;
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
                    &mut pCodingParam.sSpatialLayers[idx].sSliceArgument,
                    pCodingParam.iRCMode,
                    kiPicWidth,
                    kiPicHeight,
                );
                if iReturn != 0 {
                    return ENC_RETURN_UNSUPPORTED_PARA;
                }
            }
            // encoder_ext.cpp:560
            SM_RASTER_SLICE => {
                pCodingParam.sSpatialLayers[idx]
                    .sSliceArgument
                    .uiSliceSizeConstraint = 0;

                let iMbWidth = (kiPicWidth + 15) >> 4;
                let iMbHeight = (kiPicHeight + 15) >> 4;
                let iMbNumInFrame = iMbWidth * iMbHeight;
                let iMaxSliceNum = MAX_SLICES_NUM as i32;
                let pSliceArgument = &mut pCodingParam.sSpatialLayers[idx].sSliceArgument;

                if pSliceArgument.uiSliceMbNum[0] == 0 {
                    if iMbHeight > iMaxSliceNum {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    pSliceArgument.uiSliceNum = iMbHeight as u32;
                    for j in 0..iMbHeight as usize {
                        pSliceArgument.uiSliceMbNum[j] = iMbWidth as u32;
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
                    if pSliceArgument.uiSliceNum == 0
                        || pSliceArgument.uiSliceNum > iMaxSliceNum as u32
                    {
                        return ENC_RETURN_UNSUPPORTED_PARA;
                    }
                    if pSliceArgument.uiSliceNum == 1 {
                        // SM_RASTER_SLICE with one slice is just SM_SINGLE_SLICE
                        pSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
                    } else {
                        // C++ logs "GOM based RC do not support SM_RASTER_SLICE" when
                        // iRCMode != RC_OFF_MODE here, but does not fail.
                        //
                        // considering coding efficiency and performance, iCountMbNum is
                        // constrained by MIN_NUM_MB_PER_SLICE for multi-slice mode
                        if iMbNumInFrame <= MIN_NUM_MB_PER_SLICE {
                            pSliceArgument.uiSliceMode = SM_SINGLE_SLICE;
                            pSliceArgument.uiSliceNum = 1;
                        }
                    }
                }
            }
            SM_SIZELIMITED_SLICE => {
                // encoder_ext.cpp:614-644. iMbWidth/iMbHeight are computed but
                // unused in this arm in the C++ too.
                let uiMaxNalSize = pCodingParam.uiMaxNalSize;
                let pSliceArgument = &mut pCodingParam.sSpatialLayers[idx].sSliceArgument;
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

    for i in 0..pCodingParam.iSpatialLayerNum as usize {
        let uiProfileIdc = pCodingParam.sSpatialLayers[i].uiProfileIdc;
        if uiProfileIdc == PRO_BASELINE || uiProfileIdc == PRO_SCALABLE_BASELINE {
            if pCodingParam.iEntropyCodingModeFlag != 0 {
                pCodingParam.iEntropyCodingModeFlag = 0;
                WelsLog(
                    pLogCtx,
                    WELS_LOG_WARNING,
                    &format!("layerId({}) Profile is baseline, Change CABAC to CAVLC", i),
                );
            }
        } else if uiProfileIdc == PRO_UNKNOWN {
            pCodingParam.sSpatialLayers[i].uiProfileIdc =
                if i == 0 || pCodingParam.bSimulcastAVC {
                    if pCodingParam.iEntropyCodingModeFlag != 0 {
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
pub fn CheckProfileSetting(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    iLayer: i32,
    uiProfileIdc: EProfileIdc,
) {
    let pLayerInfo = &mut pParam.sSpatialLayers[iLayer as usize];
    pLayerInfo.uiProfileIdc = uiProfileIdc;
    if pParam.bSimulcastAVC {
        if uiProfileIdc != PRO_BASELINE && uiProfileIdc != PRO_MAIN && uiProfileIdc != PRO_HIGH {
            WelsLog(
                pLogCtx,
                WELS_LOG_WARNING,
                &format!(
                    "layerId({}) doesn't support profile({}), change to UNSPECIFIC profile",
                    iLayer, uiProfileIdc as i32
                ),
            );
            pLayerInfo.uiProfileIdc = PRO_UNKNOWN;
        }
    } else if iLayer == SPATIAL_LAYER_0 as i32 {
        if uiProfileIdc != PRO_BASELINE && uiProfileIdc != PRO_MAIN && uiProfileIdc != PRO_HIGH {
            WelsLog(
                pLogCtx,
                WELS_LOG_WARNING,
                &format!(
                    "layerId({}) doesn't support profile({}), change to UNSPECIFIC profile",
                    iLayer, uiProfileIdc as i32
                ),
            );
            pLayerInfo.uiProfileIdc = PRO_UNKNOWN;
        }
    } else if uiProfileIdc != PRO_SCALABLE_BASELINE && uiProfileIdc != PRO_SCALABLE_HIGH {
        pLayerInfo.uiProfileIdc = PRO_SCALABLE_BASELINE;
        WelsLog(
            pLogCtx,
            WELS_LOG_WARNING,
            &format!(
                "layerId({}) doesn't support profile({}), change to scalable baseline profile",
                iLayer, uiProfileIdc as i32
            ),
        );
    }
}

/// `CheckLevelSetting` — codec/encoder/core/src/encoder_ext.cpp:151.
/// Accepts `uiLevelIdc` only if it appears in the shared level-limits table,
/// otherwise leaves the layer at `LEVEL_UNKNOWN`.
pub fn CheckLevelSetting(
    _pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    iLayer: i32,
    uiLevelIdc: ELevelIdc,
) {
    let pLayerInfo = &mut pParam.sSpatialLayers[iLayer as usize];
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
pub fn CheckReferenceNumSetting(
    _pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    iNumRef: i32,
) {
    let iRefUpperBound = if pParam.iUsageType == CAMERA_VIDEO_REAL_TIME {
        MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA
    } else {
        MAX_REFERENCE_PICTURE_COUNT_NUM_SCREEN
    };
    pParam.iNumRefFrame = iNumRef;
    if iNumRef < MIN_REF_PIC_COUNT || iNumRef > iRefUpperBound {
        pParam.iNumRefFrame = AUTO_REF_PIC_COUNT;
    }
}

/// `WelsEncoderApplyBitVaryRang` — codec/encoder/core/src/encoder_ext.cpp:726.
///
/// Lowers each layer's `iMaxSpatialBitrate` to at most `iSpatialBitrate * (1 +
/// iRang/100)`. It does **not** write `iBitsVaryPercentage`; `SetOption` does
/// that (with the clip) before calling.
pub fn WelsEncoderApplyBitVaryRang(
    pLogCtx: SLogContext,
    pParam: &mut SWelsSvcCodingParam,
    iRang: i32,
) -> i32 {
    let iNumLayers = pParam.iSpatialLayerNum;
    for i in 0..iNumLayers as usize {
        let pLayerParam = &mut pParam.sSpatialLayers[i];
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
    /// This is the allocation root: `WelsInitEncoderExt` fills the slot,
    /// `WelsUninitEncoderExt` takes it by value.
    pub m_pEncContext: Option<Box<sWelsEncCtx>>,
    /// The trace object, owned outright.
    pub(crate) m_pWelsTrace: Box<welsCodecTrace>,
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
    pub(crate) fn log_ctx(&mut self) -> SLogContext {
        self.m_pWelsTrace.m_sLogCtx
    }

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
        // message tag and nothing else, so it travels as an address.
        let instance = std::ptr::from_mut(self) as usize;
        self.m_pWelsTrace.SetCodecInstance(instance);
    }

    pub fn GetDefaultParams(&mut self, argv: &mut SEncParamExt) -> i32 {
        SWelsSvcCodingParam::FillDefaultExt(argv);
        cmResultSuccess
    }

    /// `None` is the reference's `NULL argv`, which is a *reported* error
    /// (`welsEncoderExt.cpp:192`) and not a caller contract, so it survives the
    /// translation as an `Option` rather than being rejected at the thunk.
    pub fn Initialize(&mut self, argv: Option<&SEncParamBase>) -> i32 {
        // `welsEncoderExt.cpp:188`.
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
        // `welsEncoderExt.cpp:215`.
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

    pub fn InitializeInternal(&mut self, pCfg: &mut SWelsSvcCodingParam) -> i32 {

        if self.m_bInitialFlag {
            self.Uninitialize();
        }

        let iNumOfLayers = pCfg.iSpatialLayerNum;
        if iNumOfLayers < 1 || iNumOfLayers > MAX_DEPENDENCY_LAYER {
            self.Uninitialize();
            return cmInitParaError;
        }
        if pCfg.iTemporalLayerNum < 1 {
            pCfg.iTemporalLayerNum = 1;
        }
        if pCfg.iTemporalLayerNum > MAX_TEMPORAL_LEVEL {
            self.Uninitialize();
            return cmInitParaError;
        }

        if pCfg.uiGopSize < 1 || pCfg.uiGopSize > MAX_GOP_SIZE {
            self.Uninitialize();
            return cmInitParaError;
        }

        if !WELS_POWER2_IF(pCfg.uiGopSize) {
            self.Uninitialize();
            return cmInitParaError;
        }

        if pCfg.uiIntraPeriod != 0 && pCfg.uiIntraPeriod < pCfg.uiGopSize {
            self.Uninitialize();
            return cmInitParaError;
        }

        if pCfg.uiIntraPeriod != 0 && (pCfg.uiIntraPeriod & (pCfg.uiGopSize - 1)) != 0
        {
            self.Uninitialize();
            return cmInitParaError;
        }

        if pCfg.iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            if pCfg.bEnableLongTermReference {
                pCfg.iLTRRefNum = LONG_TERM_REF_NUM_SCREEN;
                if pCfg.iNumRefFrame == AUTO_REF_PIC_COUNT {
                    pCfg.iNumRefFrame =
                        WELS_MAX(1, WELS_LOG2(pCfg.uiGopSize)) + pCfg.iLTRRefNum;
                }
            } else {
                pCfg.iLTRRefNum = 0;
                if pCfg.iNumRefFrame == AUTO_REF_PIC_COUNT {
                    pCfg.iNumRefFrame = WELS_MAX(1, (pCfg.uiGopSize >> 1) as i32);
                }
            }
        } else {
            pCfg.iLTRRefNum = if pCfg.bEnableLongTermReference {
                LONG_TERM_REF_NUM
            } else {
                0
            };
            if pCfg.iNumRefFrame == AUTO_REF_PIC_COUNT {
                let ref_calc = if (pCfg.uiGopSize >> 1) > 1 {
                    (pCfg.uiGopSize >> 1) as i32 + pCfg.iLTRRefNum
                } else {
                    MIN_REF_PIC_COUNT + pCfg.iLTRRefNum
                };
                pCfg.iNumRefFrame = WELS_CLIP3(
                    ref_calc,
                    MIN_REF_PIC_COUNT,
                    MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA,
                );
            }
        }

        if pCfg.iLtrMarkPeriod == 0 {
            pCfg.iLtrMarkPeriod = 30;
        }

        let kiDecStages = WELS_LOG2(pCfg.uiGopSize);
        pCfg.iTemporalLayerNum = 1 + kiDecStages;
        pCfg.iLoopFilterAlphaC0Offset =
            WELS_CLIP3(pCfg.iLoopFilterAlphaC0Offset, -6, 6);
        pCfg.iLoopFilterBetaOffset = WELS_CLIP3(pCfg.iLoopFilterBetaOffset, -6, 6);

        self.m_iMaxPicWidth = pCfg.iPicWidth;
        self.m_iMaxPicHeight = pCfg.iPicHeight;

        self.TraceParamInfo(&mut pCfg.to_param_ext());
        let log_ctx = self.m_pWelsTrace.m_sLogCtx;

        if crate::encoder::encoder_ext::WelsInitEncoderExt(
            &mut self.m_pEncContext,
            pCfg,
            log_ctx,
            None,
        ) != 0
        {
            self.Uninitialize();
            return cmInitParaError;
        }

        self.m_bInitialFlag = true;

        cmResultSuccess
    }

    pub fn Uninitialize(&mut self) -> i32 {
        if !self.m_bInitialFlag {
            return 0;
        }
        // `welsEncoderExt.cpp:358`.
        WelsLog(
            self.log_ctx(),
            WELS_LOG_INFO,
            &format!(
                "CWelsH264SVCEncoder::Uninitialize(), openh264 codec version = {}.",
                Self::version_number()
            ),
        );
        crate::encoder::encoder_ext::WelsUninitEncoderExt(self.m_pEncContext.take());
        self.m_bInitialFlag = false;
        0
    }

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

    pub fn EncodeFrameInternal(
        &mut self,
        pSrcPic: &SSourcePicture,
        pBsInfo: &mut SFrameBSInfo,
    ) -> i32 {
        if pSrcPic.iPicWidth < 16 || pSrcPic.iPicHeight < 16 {
            return cmUnsupportedData;
        }
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

        self.UpdateStatistics(pBsInfo, kiCurrentFrameMs);

        cmResultSuccess
    }

    pub fn EncodeParameterSets(&mut self, pBsInfo: &mut SFrameBSInfo) -> i32 {
        if !self.m_bInitialFlag {
            return cmInitParaError;
        }
        let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
            return cmInitParaError;
        };
        WelsEncoderEncodeParameterSetsRust(ctx, pBsInfo)
    }

    pub fn ForceIntraFrame(&mut self, bIDR: bool, iLayerId: i32) -> i32 {
        if bIDR {
            if !self.m_bInitialFlag {
                return 1;
            }
            let Some(pCtx) = self.m_pEncContext.as_deref_mut() else {
                return 1;
            };
            ForceCodingIDR(pCtx, iLayerId);
        }
        0
    }

    /// `CWelsH264SVCEncoder::TraceParamInfo` — **`welsEncoderExt.cpp:505`**.
    ///
    /// One `WELS_LOG_INFO` line carrying the whole parameter block, then one more
    /// per spatial layer.
    ///
    /// **Every unsigned field printed through `%d` is cast to `i32` here, and that
    /// is not a lint — it is the reference's output.** C's `%d` on a `uint32_t`
    /// reinterprets the bits as signed, so upstream prints `uiIntraPeriod= -1`
    /// where the stored value is `0xFFFFFFFF`; Rust's `{}` on a `u32` prints
    /// `4294967295`.
    ///
    /// **Format fidelity is the whole contract**, so the odd spellings below are
    /// deliberate transcriptions and not typos: `fFrameRate= %.6ff` really does
    /// print a trailing `f` after six decimals, the block prints `iComplexityMode`
    /// **twice** (once mid-line and once inside the parenthesised tail), and the
    /// parenthesis opened at `iLoopFilterDisableIdc (offset(alpha/beta):` is never
    /// closed until the tail's own `)`. Every `bool` reaches C's `%d` as 0 or 1.
    pub fn TraceParamInfo(&mut self, pParam: &SEncParamExt) {
        let b = |v: bool| if v { 1 } else { 0 };
        WelsLog(
            self.log_ctx(),
            WELS_LOG_INFO,
            &format!(
                "iUsageType = {},iPicWidth= {};iPicHeight= {};iTargetBitrate= {};iMaxBitrate= {};iRCMode= {};iPaddingFlag= {};iTemporalLayerNum= {};iSpatialLayerNum= {};fFrameRate= {:.6}f;uiIntraPeriod= {};\
eSpsPpsIdStrategy = {};bPrefixNalAddingCtrl = {};bSimulcastAVC={};bEnableDenoise= {};bEnableBackgroundDetection= {};bEnableSceneChangeDetect = {};bEnableAdaptiveQuant= {};bEnableFrameSkip= {};bEnableLongTermReference= {};iLtrMarkPeriod= {}, bIsLosslessLink={};\
iComplexityMode = {};iNumRefFrame = {};iEntropyCodingModeFlag = {};uiMaxNalSize = {};iLTRRefNum = {};iMultipleThreadIdc = {};iLoopFilterDisableIdc = {} (offset(alpha/beta): {},{};iComplexityMode = {},iMaxQp = {};iMinQp = {})",
                pParam.iUsageType as i32,
                pParam.iPicWidth,
                pParam.iPicHeight,
                pParam.iTargetBitrate,
                pParam.iMaxBitrate,
                pParam.iRCMode as i32,
                pParam.iPaddingFlag,
                pParam.iTemporalLayerNum,
                pParam.iSpatialLayerNum,
                pParam.fMaxFrameRate,
                pParam.uiIntraPeriod as i32,
                pParam.eSpsPpsIdStrategy as i32,
                b(pParam.bPrefixNalAddingCtrl),
                b(pParam.bSimulcastAVC),
                b(pParam.bEnableDenoise),
                b(pParam.bEnableBackgroundDetection),
                b(pParam.bEnableSceneChangeDetect),
                b(pParam.bEnableAdaptiveQuant),
                b(pParam.bEnableFrameSkip),
                b(pParam.bEnableLongTermReference),
                pParam.iLtrMarkPeriod as i32,
                b(pParam.bIsLosslessLink),
                pParam.iComplexityMode as i32,
                pParam.iNumRefFrame,
                pParam.iEntropyCodingModeFlag,
                pParam.uiMaxNalSize as i32,
                pParam.iLTRRefNum,
                pParam.iMultipleThreadIdc as i32,
                pParam.iLoopFilterDisableIdc,
                pParam.iLoopFilterAlphaC0Offset,
                pParam.iLoopFilterBetaOffset,
                pParam.iComplexityMode as i32,
                pParam.iMaxQp,
                pParam.iMinQp
            ),
        );
        // `while (i < iSpatialLayers)` with the same clamp the reference applies —
        // a caller may name more layers than the array holds.
        let iSpatialLayers = if (pParam.iSpatialLayerNum as usize) < MAX_SPATIAL_LAYER_NUM {
            pParam.iSpatialLayerNum as usize
        } else {
            MAX_SPATIAL_LAYER_NUM
        };
        for i in 0..iSpatialLayers {
            let pSpatialCfg = &pParam.sSpatialLayers[i];
            WelsLog(
                self.log_ctx(),
                WELS_LOG_INFO,
                &format!(
                    "sSpatialLayers[{}]: .iVideoWidth= {}; .iVideoHeight= {}; .fFrameRate= {:.6}f; .iSpatialBitrate= {}; .iMaxSpatialBitrate= {}; .sSliceArgument.uiSliceMode= {}; .sSliceArgument.iSliceNum= {}; .sSliceArgument.uiSliceSizeConstraint= {};\
uiProfileIdc = {};uiLevelIdc = {};iDLayerQp = {}",
                    i,
                    pSpatialCfg.iVideoWidth,
                    pSpatialCfg.iVideoHeight,
                    pSpatialCfg.fFrameRate,
                    pSpatialCfg.iSpatialBitrate,
                    pSpatialCfg.iMaxSpatialBitrate,
                    pSpatialCfg.sSliceArgument.uiSliceMode as i32,
                    pSpatialCfg.sSliceArgument.uiSliceNum as i32,
                    pSpatialCfg.sSliceArgument.uiSliceSizeConstraint as i32,
                    pSpatialCfg.uiProfileIdc as i32,
                    pSpatialCfg.uiLevelIdc as i32,
                    pSpatialCfg.iDLayerQp
                ),
            );
        }
    }

    /// `CWelsH264SVCEncoder::LogStatistics` — `welsEncoderExt.cpp:569`.
    ///
    /// One `WELS_LOG_INFO` line per dependency id in `[0, iMaxDid]`, read straight
    /// out of `sEncoderStatistics[iDid]`.
    ///
    /// `uLTRSentNum=NA` is a literal in the reference: the field exists
    /// (`uiLTRSentNum`) and the line does not print it.
    pub fn LogStatistics(&mut self, kiCurrentFrameTs: i64, iMaxDid: i32) {
        for iDid in 0..=iMaxDid {
            // Copied out (`SEncoderStatistics` is `Copy`) so the context borrow ends
            // before `log_ctx()` takes `&mut self` for the trace object beside it.
            let Some(pStatistics) = self
                .m_pEncContext
                .as_deref()
                .map(|pCtx| pCtx.sEncoderStatistics[iDid as usize])
            else {
                return;
            };
            WelsLog(
                self.log_ctx(),
                WELS_LOG_INFO,
                &format!(
                    "EncoderStatistics: SpatialId = {},{}x{}, SpeedInMs: {}, fAverageFrameRate={}, \
LastFrameRate={}, LatestBitRate={}, LastFrameQP={}, uiInputFrameCount={}, uiSkippedFrameCount={}, \
uiResolutionChangeTimes={}, uIDRReqNum={}, uIDRSentNum={}, uLTRSentNum=NA, iTotalEncodedBytes={} at Ts = {}",
                    iDid,
                    pStatistics.uiWidth as i32,
                    pStatistics.uiHeight as i32,
                    format!("{:.6}", pStatistics.fAverageFrameSpeedInMs),
                    format!("{:.6}", pStatistics.fAverageFrameRate),
                    format!("{:.6}", pStatistics.fLatestFrameRate),
                    pStatistics.uiBitRate as i32,
                    pStatistics.uiAverageFrameQP as i32,
                    pStatistics.uiInputFrameCount as i32,
                    pStatistics.uiSkippedFrameCount as i32,
                    pStatistics.uiResolutionChangeTimes as i32,
                    pStatistics.uiIDRReqNum as i32,
                    pStatistics.uiIDRSentNum as i32,
                    pStatistics.iTotalEncodedBytes,
                    kiCurrentFrameTs
                ),
            );
        }
    }

    pub fn UpdateStatistics(&mut self, pBsInfo: &SFrameBSInfo, kiCurrentFrameMs: i64) {
        let kiCurrentFrameTs = pBsInfo.uiTimeStamp;
        let (kiTimeDiff, iMaxDid) = {
            let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                return;
            };
            if ctx.param_opt().is_none() {
                return;
            }
            ctx.uiLastTimestamp = kiCurrentFrameTs;
            (
                kiCurrentFrameTs - ctx.iLastStatisticsLogTs,
                ctx.param().iSpatialLayerNum - 1,
            )
        };
        for iDid in 0..=iMaxDid {
            let mut eFrameType = EVideoFrameType::videoFrameTypeSkip;
            let mut kiCurrentFrameSize = 0;
            // Each layer's `pNalLengthInByte` is the previous one's advanced by
            // the previous one's `iNalCount` — so the running sum below *is* the
            // pointer chain, in the units the storage is made of, and this walk
            // visits the layers in the order that chain was built.
            let kpNalLen: &[std::sync::atomic::AtomicI32] = match self.m_pEncContext.as_deref() {
                Some(ctx) => match ctx.pOut.as_deref() {
                    Some(pOut) => &pOut.sNalLen,
                    None => &[],
                },
                None => &[],
            };
            let mut kiBase = 0usize;
            for iLayerNum in 0..(pBsInfo.iLayerNum as usize).min(MAX_LAYER_NUM_OF_FRAME as usize) {
                let pLayerInfo = &pBsInfo.sLayerInfo[iLayerNum];
                let kiCount = pLayerInfo.iNalCount.max(0) as usize;
                if pLayerInfo.uiLayerType == VIDEO_CODING_LAYER
                    && pLayerInfo.uiSpatialId as i32 == iDid
                {
                    eFrameType = pBsInfo.eFrameType;
                    if kiBase + kiCount <= kpNalLen.len() {
                        kiCurrentFrameSize += kpNalLen[kiBase..][..kiCount]
                            .iter()
                            .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                            .sum::<i32>();
                    }
                }
                kiBase += kiCount;
            }

            let mut bLogStatisticsNow = false;
            {
            let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                return;
            };
            let bLtrMarkingFlag = ctx
                .pLtr
                .first()
                .map_or(false, |pLtr| pLtr.bLTRMarkingFlag);
            let uiAverageFrameQP = if !ctx.rc().is_empty() {
                ctx.rc_at(iDid as usize).iAverageFrameQp as u32
            } else {
                26
            };
            let kiActualWidth =
                ctx.param().sDependencyLayers[iDid as usize].iActualWidth;
            let kiActualHeight =
                ctx.param().sDependencyLayers[iDid as usize].iActualHeight;
            let kfMaxFrameRate = ctx.param().fMaxFrameRate;
            let kiStatisticsLogInterval = ctx.iStatisticsLogInterval;
            let pStatistics =
                &mut ctx.sEncoderStatistics[iDid as usize];

            if pStatistics.uiWidth != 0
                && pStatistics.uiHeight != 0
                && (pStatistics.uiWidth != kiActualWidth as u32
                    || pStatistics.uiHeight != kiActualHeight as u32)
            {
                pStatistics.uiResolutionChangeTimes += 1;
            }
            pStatistics.uiWidth = kiActualWidth as u32;
            pStatistics.uiHeight = kiActualHeight as u32;

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

            if ctx.uiStartTimestamp != 0 {
                if kiCurrentFrameTs > ctx.uiStartTimestamp + 800 {
                    pStatistics.fAverageFrameRate = (pStatistics.uiInputFrameCount as f32
                        * 1000.0)
                        / ((kiCurrentFrameTs - ctx.uiStartTimestamp) as f32);
                }
            } else {
                ctx.uiStartTimestamp = kiCurrentFrameTs;
            }

            pStatistics.uiAverageFrameQP = uiAverageFrameQP;

            if eFrameType == EVideoFrameType::videoFrameTypeIDR
                || eFrameType == EVideoFrameType::videoFrameTypeI
            {
                pStatistics.uiIDRSentNum += 1;
            }
            if bLtrMarkingFlag {
                pStatistics.uiLTRSentNum += 1;
            }

            pStatistics.iTotalEncodedBytes += kiCurrentFrameSize as u64;

            let kiDeltaFrames = (pStatistics.uiInputFrameCount
                - pStatistics.iLastStatisticsFrameCount)
                as i32;
            if kiDeltaFrames as f32 > kfMaxFrameRate * 2.0 {
                if kiTimeDiff >= kiStatisticsLogInterval as i64 {
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
                    ctx.iLastStatisticsLogTs = kiCurrentFrameTs;
                    // `LogStatistics` takes `&mut self` and the reset writes
                    // back into the statistics this scope is holding, so both
                    // move below the borrow. The C++ order — log, *then* reset
                    // `iTotalEncodedBytes` — is preserved exactly.
                    bLogStatisticsNow = true;
                }
            }
            }
            if bLogStatisticsNow {
                self.LogStatistics(kiCurrentFrameTs, iMaxDid);
                let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                    return;
                };
                ctx.sEncoderStatistics[iDid as usize].iTotalEncodedBytes = 0;
            }
        }
    }
}

impl Drop for CWelsH264SVCEncoder {
    fn drop(&mut self) {
        // `welsEncoderExt.cpp:136` — the destructor announces itself first, then
        // uninitializes, so the two lines land in the reference's order.
        crate::common::wels_trace::WelsLog(
            self.m_pWelsTrace.m_sLogCtx,
            crate::common::wels_trace::WELS_LOG_INFO,
            "CWelsH264SVCEncoder::~CWelsH264SVCEncoder()",
        );
        self.Uninitialize();
    }
}

pub use crate::api::version::G_ST_CODEC_VERSION;
