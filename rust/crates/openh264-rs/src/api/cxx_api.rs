//! Self-contained OpenH264 C++ API generated via `cxx` in the top-level namespace (`::`).
//!
//! `pub mod ffi` defines the C++-facing structs and enums so `cxx-build` generates a
//! 100% self-contained `cxx_api.rs.h` header without any external `.h` dependencies.
//! At the bridge boundary, these types are converted to/from the native Rust enums
//! and structs in [`crate::api::types`], keeping the internal Rust codec free of
//! `cxx`'s `.repr` enum wrappers.

#![deny(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_qualifications
)]

use crate::api::c_api::{self, abi_guard};
use crate::api::encoder::Encoder;
use crate::api::types::{
    self as rust_types, CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS,
    CM_UNKNOWN_REASON,
};
use std::ptr;

// Compile-time layout parity assertions between `ffi` C++ types and `rust_types` native types
const _: () = {
    assert!(size_of::<ffi::OpenH264Version>() == size_of::<rust_types::OpenH264Version>());
    assert!(size_of::<ffi::SSliceArgument>() == size_of::<rust_types::SSliceArgument>());
    assert!(size_of::<ffi::SSpatialLayerConfig>() == size_of::<rust_types::SSpatialLayerConfig>());
    assert!(size_of::<ffi::SEncParamBase>() == size_of::<rust_types::SEncParamBase>());
    assert!(size_of::<ffi::SEncParamExt>() == size_of::<rust_types::SEncParamExt>());
    assert!(size_of::<ffi::SSourcePicture>() == size_of::<rust_types::SSourcePicture>());
    assert!(size_of::<ffi::SLayerBSInfo>() == size_of::<rust_types::SLayerBSInfo>());
    assert!(size_of::<ffi::SFrameBSInfo>() == size_of::<rust_types::SFrameBSInfo>());
    assert!(size_of::<ffi::SDecodingParam>() == size_of::<rust_types::SDecodingParam>());
    assert!(size_of::<ffi::SBufferInfo>() == size_of::<rust_types::SBufferInfo>());
    assert!(size_of::<ffi::SParserBsInfo>() == size_of::<rust_types::SParserBsInfo>());
    assert!(size_of::<ffi::SDecoderCapability>() == size_of::<rust_types::SDecoderCapability>());
};

// ---------------------------------------------------------------------------
// Safe Conversions Between C++ Bridge Types (`ffi`) and Native Rust Types (`rust_types`)
// ---------------------------------------------------------------------------

impl From<ffi::EUsageType> for rust_types::EUsageType {
    fn from(v: ffi::EUsageType) -> Self {
        match v.repr {
            0 => Self::CAMERA_VIDEO_REAL_TIME,
            1 => Self::SCREEN_CONTENT_REAL_TIME,
            2 => Self::CAMERA_VIDEO_NON_REAL_TIME,
            3 => Self::SCREEN_CONTENT_NON_REAL_TIME,
            _ => Self::INPUT_CONTENT_TYPE_ALL,
        }
    }
}

impl From<rust_types::EUsageType> for ffi::EUsageType {
    fn from(v: rust_types::EUsageType) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::RC_MODES> for rust_types::RC_MODES {
    fn from(v: ffi::RC_MODES) -> Self {
        match v.repr {
            -1 => Self::RC_OFF_MODE,
            0 => Self::RC_QUALITY_MODE,
            1 => Self::RC_BITRATE_MODE,
            2 => Self::RC_BUFFERBASED_MODE,
            3 => Self::RC_TIMESTAMP_MODE,
            _ => Self::RC_BITRATE_MODE_POST_SKIP,
        }
    }
}

impl From<rust_types::RC_MODES> for ffi::RC_MODES {
    fn from(v: rust_types::RC_MODES) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::SliceModeEnum> for rust_types::SliceModeEnum {
    fn from(v: ffi::SliceModeEnum) -> Self {
        match v.repr {
            0 => Self::SM_SINGLE_SLICE,
            1 => Self::SM_FIXEDSLCNUM_SLICE,
            2 => Self::SM_RASTER_SLICE,
            3 => Self::SM_SIZELIMITED_SLICE,
            _ => Self::SM_RESERVED,
        }
    }
}

impl From<rust_types::SliceModeEnum> for ffi::SliceModeEnum {
    fn from(v: rust_types::SliceModeEnum) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::EProfileIdc> for rust_types::EProfileIdc {
    fn from(v: ffi::EProfileIdc) -> Self {
        match v.repr {
            66 => Self::PRO_BASELINE,
            77 => Self::PRO_MAIN,
            88 => Self::PRO_EXTENDED,
            100 => Self::PRO_HIGH,
            110 => Self::PRO_HIGH10,
            122 => Self::PRO_HIGH422,
            144 => Self::PRO_HIGH444,
            244 => Self::PRO_CAVLC444,
            83 => Self::PRO_SCALABLE_BASELINE,
            86 => Self::PRO_SCALABLE_HIGH,
            _ => Self::PRO_UNKNOWN,
        }
    }
}

impl From<rust_types::EProfileIdc> for ffi::EProfileIdc {
    fn from(v: rust_types::EProfileIdc) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::ELevelIdc> for rust_types::ELevelIdc {
    fn from(v: ffi::ELevelIdc) -> Self {
        match v.repr {
            9 => Self::LEVEL_1_B,
            10 => Self::LEVEL_1_0,
            11 => Self::LEVEL_1_1,
            12 => Self::LEVEL_1_2,
            13 => Self::LEVEL_1_3,
            20 => Self::LEVEL_2_0,
            21 => Self::LEVEL_2_1,
            22 => Self::LEVEL_2_2,
            30 => Self::LEVEL_3_0,
            31 => Self::LEVEL_3_1,
            32 => Self::LEVEL_3_2,
            40 => Self::LEVEL_4_0,
            41 => Self::LEVEL_4_1,
            42 => Self::LEVEL_4_2,
            50 => Self::LEVEL_5_0,
            51 => Self::LEVEL_5_1,
            52 => Self::LEVEL_5_2,
            _ => Self::LEVEL_UNKNOWN,
        }
    }
}

impl From<rust_types::ELevelIdc> for ffi::ELevelIdc {
    fn from(v: rust_types::ELevelIdc) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::ESampleAspectRatio> for rust_types::ESampleAspectRatio {
    fn from(v: ffi::ESampleAspectRatio) -> Self {
        match v.repr {
            1 => Self::ASP_1x1,
            2 => Self::ASP_12x11,
            3 => Self::ASP_10x11,
            4 => Self::ASP_16x11,
            5 => Self::ASP_40x33,
            6 => Self::ASP_24x11,
            7 => Self::ASP_20x11,
            8 => Self::ASP_32x11,
            9 => Self::ASP_80x33,
            10 => Self::ASP_18x11,
            11 => Self::ASP_15x11,
            12 => Self::ASP_64x33,
            13 => Self::ASP_160x99,
            255 => Self::ASP_EXT_SAR,
            _ => Self::ASP_UNSPECIFIED,
        }
    }
}

impl From<rust_types::ESampleAspectRatio> for ffi::ESampleAspectRatio {
    fn from(v: rust_types::ESampleAspectRatio) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::ECOMPLEXITY_MODE> for rust_types::ECOMPLEXITY_MODE {
    fn from(v: ffi::ECOMPLEXITY_MODE) -> Self {
        match v.repr {
            1 => Self::MEDIUM_COMPLEXITY,
            2 => Self::HIGH_COMPLEXITY,
            _ => Self::LOW_COMPLEXITY,
        }
    }
}

impl From<rust_types::ECOMPLEXITY_MODE> for ffi::ECOMPLEXITY_MODE {
    fn from(v: rust_types::ECOMPLEXITY_MODE) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::EParameterSetStrategy> for rust_types::EParameterSetStrategy {
    fn from(v: ffi::EParameterSetStrategy) -> Self {
        match v.repr {
            0 => Self::CONSTANT_ID,
            2 => Self::SPS_LISTING,
            3 => Self::SPS_LISTING_AND_PPS_INCREASING,
            6 => Self::SPS_PPS_LISTING,
            _ => Self::INCREASING_ID,
        }
    }
}

impl From<rust_types::EParameterSetStrategy> for ffi::EParameterSetStrategy {
    fn from(v: rust_types::EParameterSetStrategy) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<rust_types::EVideoFrameType> for ffi::EVideoFrameType {
    fn from(v: rust_types::EVideoFrameType) -> Self {
        Self { repr: v as i32 }
    }
}

impl From<ffi::EVideoFrameType> for rust_types::EVideoFrameType {
    fn from(v: ffi::EVideoFrameType) -> Self {
        match v.repr {
            1 => Self::videoFrameTypeIDR,
            2 => Self::videoFrameTypeI,
            3 => Self::videoFrameTypeP,
            4 => Self::videoFrameTypeSkip,
            5 => Self::videoFrameTypeIPMixed,
            _ => Self::videoFrameTypeInvalid,
        }
    }
}

impl From<ffi::ENCODER_OPTION> for rust_types::ENCODER_OPTION {
    fn from(v: ffi::ENCODER_OPTION) -> Self {
        match v.repr {
            0 => Self::ENCODER_OPTION_DATAFORMAT,
            1 => Self::ENCODER_OPTION_IDR_INTERVAL,
            2 => Self::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE,
            3 => Self::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
            4 => Self::ENCODER_OPTION_FRAME_RATE,
            5 => Self::ENCODER_OPTION_BITRATE,
            6 => Self::ENCODER_OPTION_MAX_BITRATE,
            7 => Self::ENCODER_OPTION_INTER_SPATIAL_PRED,
            8 => Self::ENCODER_OPTION_RC_MODE,
            9 => Self::ENCODER_OPTION_RC_FRAME_SKIP,
            10 => Self::ENCODER_PADDING_PADDING,
            11 => Self::ENCODER_OPTION_PROFILE,
            12 => Self::ENCODER_OPTION_LEVEL,
            13 => Self::ENCODER_OPTION_NUMBER_REF,
            14 => Self::ENCODER_OPTION_DELIVERY_STATUS,
            15 => Self::ENCODER_LTR_RECOVERY_REQUEST,
            16 => Self::ENCODER_LTR_MARKING_FEEDBACK,
            17 => Self::ENCODER_LTR_MARKING_PERIOD,
            18 => Self::ENCODER_OPTION_LTR,
            19 => Self::ENCODER_OPTION_COMPLEXITY,
            20 => Self::ENCODER_OPTION_ENABLE_SSEI,
            21 => Self::ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING,
            22 => Self::ENCODER_OPTION_SPS_PPS_ID_STRATEGY,
            23 => Self::ENCODER_OPTION_CURRENT_PATH,
            24 => Self::ENCODER_OPTION_DUMP_FILE,
            25 => Self::ENCODER_OPTION_TRACE_LEVEL,
            26 => Self::ENCODER_OPTION_TRACE_CALLBACK,
            27 => Self::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT,
            28 => Self::ENCODER_OPTION_GET_STATISTICS,
            29 => Self::ENCODER_OPTION_STATISTICS_LOG_INTERVAL,
            30 => Self::ENCODER_OPTION_IS_LOSSLESS_LINK,
            _ => Self::ENCODER_OPTION_BITS_VARY_PERCENTAGE,
        }
    }
}

impl From<ffi::DECODER_OPTION> for rust_types::DECODER_OPTION {
    fn from(v: ffi::DECODER_OPTION) -> Self {
        match v.repr {
            1 => Self::DECODER_OPTION_END_OF_STREAM,
            2 => Self::DECODER_OPTION_VCL_NAL,
            3 => Self::DECODER_OPTION_TEMPORAL_ID,
            4 => Self::DECODER_OPTION_FRAME_NUM,
            5 => Self::DECODER_OPTION_IDR_PIC_ID,
            6 => Self::DECODER_OPTION_LTR_MARKING_FLAG,
            7 => Self::DECODER_OPTION_LTR_MARKED_FRAME_NUM,
            8 => Self::DECODER_OPTION_ERROR_CON_IDC,
            9 => Self::DECODER_OPTION_TRACE_LEVEL,
            10 => Self::DECODER_OPTION_TRACE_CALLBACK,
            11 => Self::DECODER_OPTION_TRACE_CALLBACK_CONTEXT,
            12 => Self::DECODER_OPTION_GET_STATISTICS,
            13 => Self::DECODER_OPTION_GET_SAR_INFO,
            14 => Self::DECODER_OPTION_PROFILE,
            15 => Self::DECODER_OPTION_LEVEL,
            16 => Self::DECODER_OPTION_STATISTICS_LOG_INTERVAL,
            17 => Self::DECODER_OPTION_IS_REF_PIC,
            18 => Self::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            _ => Self::DECODER_OPTION_NUM_OF_THREADS,
        }
    }
}

impl From<ffi::SSliceArgument> for rust_types::SSliceArgument {
    fn from(v: ffi::SSliceArgument) -> Self {
        Self {
            uiSliceMode: v.uiSliceMode.into(),
            uiSliceNum: v.uiSliceNum,
            uiSliceMbNum: v.uiSliceMbNum,
            uiSliceSizeConstraint: v.uiSliceSizeConstraint,
        }
    }
}

impl From<rust_types::SSliceArgument> for ffi::SSliceArgument {
    fn from(v: rust_types::SSliceArgument) -> Self {
        Self {
            uiSliceMode: v.uiSliceMode.into(),
            uiSliceNum: v.uiSliceNum,
            uiSliceMbNum: v.uiSliceMbNum,
            uiSliceSizeConstraint: v.uiSliceSizeConstraint,
        }
    }
}

impl From<ffi::SSpatialLayerConfig> for rust_types::SSpatialLayerConfig {
    fn from(v: ffi::SSpatialLayerConfig) -> Self {
        Self {
            iVideoWidth: v.iVideoWidth,
            iVideoHeight: v.iVideoHeight,
            fFrameRate: v.fFrameRate,
            iSpatialBitrate: v.iSpatialBitrate,
            iMaxSpatialBitrate: v.iMaxSpatialBitrate,
            uiProfileIdc: v.uiProfileIdc.into(),
            uiLevelIdc: v.uiLevelIdc.into(),
            iDLayerQp: v.iDLayerQp,
            sSliceArgument: v.sSliceArgument.into(),
            bVideoSignalTypePresent: v.bVideoSignalTypePresent,
            uiVideoFormat: v.uiVideoFormat,
            bFullRange: v.bFullRange,
            bColorDescriptionPresent: v.bColorDescriptionPresent,
            uiColorPrimaries: v.uiColorPrimaries,
            uiTransferCharacteristics: v.uiTransferCharacteristics,
            uiColorMatrix: v.uiColorMatrix,
            bAspectRatioPresent: v.bAspectRatioPresent,
            eAspectRatio: v.eAspectRatio.into(),
            sAspectRatioExtWidth: v.sAspectRatioExtWidth,
            sAspectRatioExtHeight: v.sAspectRatioExtHeight,
        }
    }
}

impl From<rust_types::SSpatialLayerConfig> for ffi::SSpatialLayerConfig {
    fn from(v: rust_types::SSpatialLayerConfig) -> Self {
        Self {
            iVideoWidth: v.iVideoWidth,
            iVideoHeight: v.iVideoHeight,
            fFrameRate: v.fFrameRate,
            iSpatialBitrate: v.iSpatialBitrate,
            iMaxSpatialBitrate: v.iMaxSpatialBitrate,
            uiProfileIdc: v.uiProfileIdc.into(),
            uiLevelIdc: v.uiLevelIdc.into(),
            iDLayerQp: v.iDLayerQp,
            sSliceArgument: v.sSliceArgument.into(),
            bVideoSignalTypePresent: v.bVideoSignalTypePresent,
            uiVideoFormat: v.uiVideoFormat,
            bFullRange: v.bFullRange,
            bColorDescriptionPresent: v.bColorDescriptionPresent,
            uiColorPrimaries: v.uiColorPrimaries,
            uiTransferCharacteristics: v.uiTransferCharacteristics,
            uiColorMatrix: v.uiColorMatrix,
            bAspectRatioPresent: v.bAspectRatioPresent,
            eAspectRatio: v.eAspectRatio.into(),
            sAspectRatioExtWidth: v.sAspectRatioExtWidth,
            sAspectRatioExtHeight: v.sAspectRatioExtHeight,
        }
    }
}

impl From<ffi::SEncParamBase> for rust_types::SEncParamBase {
    fn from(v: ffi::SEncParamBase) -> Self {
        Self {
            iUsageType: v.iUsageType.into(),
            iPicWidth: v.iPicWidth,
            iPicHeight: v.iPicHeight,
            iTargetBitrate: v.iTargetBitrate,
            iRCMode: v.iRCMode.into(),
            fMaxFrameRate: v.fMaxFrameRate,
        }
    }
}

impl From<ffi::SEncParamExt> for rust_types::SEncParamExt {
    fn from(v: ffi::SEncParamExt) -> Self {
        Self {
            iUsageType: v.iUsageType.into(),
            iPicWidth: v.iPicWidth,
            iPicHeight: v.iPicHeight,
            iTargetBitrate: v.iTargetBitrate,
            iRCMode: v.iRCMode.into(),
            fMaxFrameRate: v.fMaxFrameRate,
            iTemporalLayerNum: v.iTemporalLayerNum,
            iSpatialLayerNum: v.iSpatialLayerNum,
            sSpatialLayers: v.sSpatialLayers.map(Into::into),
            iComplexityMode: v.iComplexityMode.into(),
            uiIntraPeriod: v.uiIntraPeriod,
            iNumRefFrame: v.iNumRefFrame,
            eSpsPpsIdStrategy: v.eSpsPpsIdStrategy.into(),
            bPrefixNalAddingCtrl: v.bPrefixNalAddingCtrl,
            bEnableSSEI: v.bEnableSSEI,
            bSimulcastAVC: v.bSimulcastAVC,
            iPaddingFlag: v.iPaddingFlag,
            iEntropyCodingModeFlag: v.iEntropyCodingModeFlag,
            bEnableFrameSkip: v.bEnableFrameSkip,
            iMaxBitrate: v.iMaxBitrate,
            iMaxQp: v.iMaxQp,
            iMinQp: v.iMinQp,
            uiMaxNalSize: v.uiMaxNalSize,
            bEnableLongTermReference: v.bEnableLongTermReference,
            iLTRRefNum: v.iLTRRefNum,
            iLtrMarkPeriod: v.iLtrMarkPeriod,
            iMultipleThreadIdc: v.iMultipleThreadIdc,
            bUseLoadBalancing: v.bUseLoadBalancing,
            iLoopFilterDisableIdc: v.iLoopFilterDisableIdc,
            iLoopFilterAlphaC0Offset: v.iLoopFilterAlphaC0Offset,
            iLoopFilterBetaOffset: v.iLoopFilterBetaOffset,
            bEnableDenoise: v.bEnableDenoise,
            bEnableBackgroundDetection: v.bEnableBackgroundDetection,
            bEnableAdaptiveQuant: v.bEnableAdaptiveQuant,
            bEnableFrameCroppingFlag: v.bEnableFrameCroppingFlag,
            bEnableSceneChangeDetect: v.bEnableSceneChangeDetect,
            bIsLosslessLink: v.bIsLosslessLink,
            bFixRCOverShoot: v.bFixRCOverShoot,
            iIdrBitrateRatio: v.iIdrBitrateRatio,
            bPsnrY: v.bPsnrY,
            bPsnrU: v.bPsnrU,
            bPsnrV: v.bPsnrV,
        }
    }
}

impl From<rust_types::SEncParamExt> for ffi::SEncParamExt {
    fn from(v: rust_types::SEncParamExt) -> Self {
        Self {
            iUsageType: v.iUsageType.into(),
            iPicWidth: v.iPicWidth,
            iPicHeight: v.iPicHeight,
            iTargetBitrate: v.iTargetBitrate,
            iRCMode: v.iRCMode.into(),
            fMaxFrameRate: v.fMaxFrameRate,
            iTemporalLayerNum: v.iTemporalLayerNum,
            iSpatialLayerNum: v.iSpatialLayerNum,
            sSpatialLayers: v.sSpatialLayers.map(Into::into),
            iComplexityMode: v.iComplexityMode.into(),
            uiIntraPeriod: v.uiIntraPeriod,
            iNumRefFrame: v.iNumRefFrame,
            eSpsPpsIdStrategy: v.eSpsPpsIdStrategy.into(),
            bPrefixNalAddingCtrl: v.bPrefixNalAddingCtrl,
            bEnableSSEI: v.bEnableSSEI,
            bSimulcastAVC: v.bSimulcastAVC,
            iPaddingFlag: v.iPaddingFlag,
            iEntropyCodingModeFlag: v.iEntropyCodingModeFlag,
            bEnableFrameSkip: v.bEnableFrameSkip,
            iMaxBitrate: v.iMaxBitrate,
            iMaxQp: v.iMaxQp,
            iMinQp: v.iMinQp,
            uiMaxNalSize: v.uiMaxNalSize,
            bEnableLongTermReference: v.bEnableLongTermReference,
            iLTRRefNum: v.iLTRRefNum,
            iLtrMarkPeriod: v.iLtrMarkPeriod,
            iMultipleThreadIdc: v.iMultipleThreadIdc,
            bUseLoadBalancing: v.bUseLoadBalancing,
            iLoopFilterDisableIdc: v.iLoopFilterDisableIdc,
            iLoopFilterAlphaC0Offset: v.iLoopFilterAlphaC0Offset,
            iLoopFilterBetaOffset: v.iLoopFilterBetaOffset,
            bEnableDenoise: v.bEnableDenoise,
            bEnableBackgroundDetection: v.bEnableBackgroundDetection,
            bEnableAdaptiveQuant: v.bEnableAdaptiveQuant,
            bEnableFrameCroppingFlag: v.bEnableFrameCroppingFlag,
            bEnableSceneChangeDetect: v.bEnableSceneChangeDetect,
            bIsLosslessLink: v.bIsLosslessLink,
            bFixRCOverShoot: v.bFixRCOverShoot,
            iIdrBitrateRatio: v.iIdrBitrateRatio,
            bPsnrY: v.bPsnrY,
            bPsnrU: v.bPsnrU,
            bPsnrV: v.bPsnrV,
        }
    }
}

impl From<ffi::SSourcePicture> for rust_types::SSourcePicture {
    fn from(v: ffi::SSourcePicture) -> Self {
        Self {
            iColorFormat: v.iColorFormat,
            iStride: v.iStride,
            pData: v.pData,
            iPicWidth: v.iPicWidth,
            iPicHeight: v.iPicHeight,
            uiTimeStamp: v.uiTimeStamp,
            bPsnrY: v.bPsnrY,
            bPsnrU: v.bPsnrU,
            bPsnrV: v.bPsnrV,
        }
    }
}

impl From<rust_types::SLayerBSInfo> for ffi::SLayerBSInfo {
    fn from(v: rust_types::SLayerBSInfo) -> Self {
        Self {
            uiTemporalId: v.uiTemporalId,
            uiSpatialId: v.uiSpatialId,
            uiQualityId: v.uiQualityId,
            eFrameType: v.eFrameType.into(),
            uiLayerType: v.uiLayerType,
            iSubSeqId: v.iSubSeqId,
            iNalCount: v.iNalCount,
            pNalLengthInByte: v.pNalLengthInByte,
            pBsBuf: v.pBsBuf,
            rPsnr: v.rPsnr,
        }
    }
}

impl From<rust_types::SFrameBSInfo> for ffi::SFrameBSInfo {
    fn from(v: rust_types::SFrameBSInfo) -> Self {
        Self {
            iLayerNum: v.iLayerNum,
            sLayerInfo: v.sLayerInfo.map(Into::into),
            eFrameType: v.eFrameType.into(),
            iFrameSizeInBytes: v.iFrameSizeInBytes,
            uiTimeStamp: v.uiTimeStamp,
        }
    }
}

impl From<rust_types::OpenH264Version> for ffi::OpenH264Version {
    fn from(v: rust_types::OpenH264Version) -> Self {
        Self {
            uMajor: v.uMajor,
            uMinor: v.uMinor,
            uRevision: v.uRevision,
            uReserved: v.uReserved,
        }
    }
}

impl From<rust_types::SDecoderCapability> for ffi::SDecoderCapability {
    fn from(v: rust_types::SDecoderCapability) -> Self {
        Self {
            iProfileIdc: v.iProfileIdc,
            iProfileIop: v.iProfileIop,
            iLevelIdc: v.iLevelIdc,
            iMaxMbps: v.iMaxMbps,
            iMaxFs: v.iMaxFs,
            iMaxCpb: v.iMaxCpb,
            iMaxDpb: v.iMaxDpb,
            iMaxBr: v.iMaxBr,
            bRedPicCap: v.bRedPicCap,
        }
    }
}

/// Opaque type representing `void` pointer payloads across the `cxx` FFI boundary.
#[repr(C)]
pub struct c_void {
    _priv: [u8; 0],
}

/// H.264 / SVC Encoder instance exposed to C++ via `cxx` (`::ISVCEncoder`).
pub struct ISVCEncoder {
    pub(crate) inner: Encoder,
}

impl ISVCEncoder {
    #[inline]
    fn log_ctx(&self) -> Option<crate::common::wels_trace::SLogContext> {
        Some(self.inner.0.m_pWelsTrace.log_context())
    }

    #[allow(unsafe_code)]
    pub unsafe fn initialize(&mut self, param: *const ffi::SEncParamBase) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::Initialize", log, CM_INIT_PARA_ERROR, {
            let param_opt =
                (unsafe { param.as_ref() }).map(|p| rust_types::SEncParamBase::from(*p));
            self.inner.0.Initialize(param_opt.as_ref())
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn initialize_ext(&mut self, param: *const ffi::SEncParamExt) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::InitializeExt", log, CM_INIT_PARA_ERROR, {
            let param_opt = (unsafe { param.as_ref() }).map(|p| rust_types::SEncParamExt::from(*p));
            self.inner.0.InitializeExt(param_opt.as_ref())
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn get_default_params(&mut self, param: *mut ffi::SEncParamExt) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::GetDefaultParams", log, CM_UNKNOWN_REASON, {
            let Some(param_mut) = (unsafe { param.as_mut() }) else {
                return CM_INIT_PARA_ERROR;
            };
            let mut rust_param = rust_types::SEncParamExt::default();
            let rc = self.inner.default_params(&mut rust_param);
            *param_mut = rust_param.into();
            rc
        })
    }

    pub fn uninitialize(&mut self) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::Uninitialize", log, CM_UNKNOWN_REASON, {
            self.inner.uninitialize()
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn encode_frame(
        &mut self,
        src_pic: *const ffi::SSourcePicture,
        bs_info: *mut ffi::SFrameBSInfo,
    ) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::EncodeFrame", log, CM_UNKNOWN_REASON, {
            let (Some(src_ref), Some(bs_mut)) =
                (unsafe { src_pic.as_ref() }, unsafe { bs_info.as_mut() })
            else {
                return CM_INIT_PARA_ERROR;
            };
            let rust_src: rust_types::SSourcePicture = (*src_ref).into();
            let mut rust_bs = rust_types::SFrameBSInfo::default();
            let rc = self.inner.0.EncodeFrame(&rust_src, &mut rust_bs);
            *bs_mut = rust_bs.into();
            rc
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn encode_parameter_sets(&mut self, bs_info: *mut ffi::SFrameBSInfo) -> i32 {
        let log = self.log_ctx();
        abi_guard!(
            "ISVCEncoder::EncodeParameterSets",
            log,
            CM_UNKNOWN_REASON,
            {
                let Some(bs_mut) = (unsafe { bs_info.as_mut() }) else {
                    return CM_INIT_PARA_ERROR;
                };
                let mut rust_bs = rust_types::SFrameBSInfo::default();
                let rc = self.inner.encode_parameter_sets(&mut rust_bs);
                *bs_mut = rust_bs.into();
                rc
            }
        )
    }

    pub fn force_intra_frame(&mut self, idr: bool, layer_id: i32) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::ForceIntraFrame", log, CM_UNKNOWN_REASON, {
            self.inner.force_intra_frame(idr, layer_id)
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn set_option(
        &mut self,
        option_id: ffi::ENCODER_OPTION,
        option: *mut c_void,
    ) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::SetOption", log, CM_UNKNOWN_REASON, {
            unsafe {
                self.inner
                    .set_option_raw(option_id.into(), option as *mut std::ffi::c_void)
            }
        })
    }

    #[allow(unsafe_code)]
    pub unsafe fn get_option(
        &mut self,
        option_id: ffi::ENCODER_OPTION,
        option: *mut c_void,
    ) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::GetOption", log, CM_UNKNOWN_REASON, {
            unsafe {
                self.inner
                    .get_option_raw(option_id.into(), option as *mut std::ffi::c_void)
            }
        })
    }
}

#[allow(unsafe_code)]
pub unsafe fn wels_create_svc_encoder(pp_encoder: *mut *mut ISVCEncoder) -> i32 {
    abi_guard!("WelsCreateSVCEncoder", None, CM_MALLOC_MEM_ERROR, {
        if pp_encoder.is_null() {
            return CM_INIT_PARA_ERROR;
        }
        let enc = Box::new(ISVCEncoder {
            inner: Encoder::new(),
        });
        unsafe {
            *pp_encoder = Box::into_raw(enc);
        }
        CM_RESULT_SUCCESS
    })
}

#[allow(unsafe_code)]
pub unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder) {
    let Some(enc_ref) = (unsafe { p_encoder.as_ref() }) else {
        return;
    };
    let log = enc_ref.log_ctx();
    abi_guard!("WelsDestroySVCEncoder", log, (), {
        unsafe {
            drop(Box::from_raw(p_encoder));
        }
    })
}

/// H.264 / SVC Decoder instance exposed to C++ via `cxx` (`::ISVCDecoder`).
pub struct ISVCDecoder {
    pub(crate) ptr: *mut c_api::ISVCDecoder,
}

#[allow(unsafe_code)]
impl ISVCDecoder {
    pub unsafe fn initialize(&mut self, param: *const ffi::SDecodingParam) -> isize {
        unsafe {
            c_api::ISVCDecoder::Initialize(self.ptr, param as *const rust_types::SDecodingParam)
                as isize
        }
    }

    pub unsafe fn uninitialize(&mut self) -> isize {
        unsafe { c_api::ISVCDecoder::Uninitialize(self.ptr) as isize }
    }

    pub unsafe fn decode_frame(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        stride: *mut i32,
        width: *mut i32,
        height: *mut i32,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::DecodeFrame(self.ptr, src, src_len, dst, stride, width, height).0
        }
    }

    pub unsafe fn decode_frame_no_delay(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::DecodeFrameNoDelay(
                self.ptr,
                src,
                src_len,
                dst,
                dst_info as *mut rust_types::SBufferInfo,
            )
            .0
        }
    }

    pub unsafe fn decode_frame2(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::DecodeFrame2(
                self.ptr,
                src,
                src_len,
                dst,
                dst_info as *mut rust_types::SBufferInfo,
            )
            .0
        }
    }

    pub unsafe fn flush_frame(
        &mut self,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::FlushFrame(self.ptr, dst, dst_info as *mut rust_types::SBufferInfo)
                .0
        }
    }

    pub unsafe fn decode_parser(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst_info: *mut ffi::SParserBsInfo,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::DecodeParser(
                self.ptr,
                src,
                src_len,
                dst_info as *mut rust_types::SParserBsInfo,
            )
            .0
        }
    }

    pub unsafe fn decode_frame_ex(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut u8,
        dst_stride: i32,
        dst_len: *mut i32,
        width: *mut i32,
        height: *mut i32,
        color_format: *mut i32,
    ) -> i32 {
        unsafe {
            c_api::ISVCDecoder::DecodeFrameEx(
                self.ptr,
                src,
                src_len,
                dst,
                dst_stride,
                dst_len,
                width,
                height,
                color_format,
            )
            .0
        }
    }

    pub unsafe fn set_option(
        &mut self,
        option_id: ffi::DECODER_OPTION,
        option: *mut c_void,
    ) -> isize {
        unsafe {
            c_api::ISVCDecoder::SetOption(
                self.ptr,
                option_id.into(),
                option as *mut std::ffi::c_void,
            ) as isize
        }
    }

    pub unsafe fn get_option(
        &mut self,
        option_id: ffi::DECODER_OPTION,
        option: *mut c_void,
    ) -> isize {
        unsafe {
            c_api::ISVCDecoder::GetOption(
                self.ptr,
                option_id.into(),
                option as *mut std::ffi::c_void,
            ) as isize
        }
    }
}

#[allow(unsafe_code)]
pub unsafe fn wels_create_decoder(pp_decoder: *mut *mut ISVCDecoder) -> isize {
    abi_guard!("WelsCreateDecoder", None, CM_MALLOC_MEM_ERROR as isize, {
        if pp_decoder.is_null() {
            return CM_INIT_PARA_ERROR as isize;
        }
        let mut raw_dec: *mut c_api::ISVCDecoder = ptr::null_mut();
        let rc = unsafe { c_api::WelsCreateDecoder(&mut raw_dec) };
        if rc != 0 || raw_dec.is_null() {
            return rc as isize;
        }
        let dec = Box::new(ISVCDecoder { ptr: raw_dec });
        unsafe {
            *pp_decoder = Box::into_raw(dec);
        }
        CM_RESULT_SUCCESS as isize
    })
}

#[allow(unsafe_code)]
pub unsafe fn wels_destroy_decoder(p_decoder: *mut ISVCDecoder) {
    if p_decoder.is_null() {
        return;
    }
    abi_guard!("WelsDestroyDecoder", None, (), {
        let dec = unsafe { Box::from_raw(p_decoder) };
        unsafe {
            c_api::WelsDestroyDecoder(dec.ptr);
        }
    })
}

#[allow(unsafe_code)]
pub unsafe fn wels_get_decoder_capability(p_dec_capability: *mut ffi::SDecoderCapability) -> i32 {
    abi_guard!("WelsGetDecoderCapability", None, CM_INIT_PARA_ERROR, {
        let Some(cap_out) = (unsafe { p_dec_capability.as_mut() }) else {
            return CM_INIT_PARA_ERROR;
        };
        let mut rust_cap = rust_types::SDecoderCapability::default();
        let rc = unsafe { c_api::WelsGetDecoderCapability(&mut rust_cap) };
        *cap_out = rust_cap.into();
        rc
    })
}

pub fn wels_get_codec_version() -> ffi::OpenH264Version {
    crate::api::version::WelsGetCodecVersion().into()
}

#[allow(unsafe_code)]
pub unsafe fn wels_get_codec_version_ex(p_version: *mut ffi::OpenH264Version) {
    if let Some(ver_out) = unsafe { p_version.as_mut() } {
        *ver_out = crate::api::version::WelsGetCodecVersion().into();
    }
}

#[allow(unsafe_code)]
#[cxx::bridge]
pub mod ffi {
    /// Video format types (`EVideoFormatType`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum EVideoFormatType {
        videoFormatRGB = 1,
        videoFormatRGBA = 2,
        videoFormatRGB555 = 3,
        videoFormatRGB565 = 4,
        videoFormatBGR = 5,
        videoFormatBGRA = 6,
        videoFormatABGR = 7,
        videoFormatARGB = 8,
        videoFormatYUY2 = 20,
        videoFormatYVYU = 21,
        videoFormatUYVY = 22,
        videoFormatI420 = 23,
        videoFormatYV12 = 24,
        videoFormatInternal = 25,
        videoFormatNV12 = 26,
        videoFormatVFlip = -0x80000000,
    }

    /// Video frame types (`EVideoFrameType`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum EVideoFrameType {
        videoFrameTypeInvalid = 0,
        videoFrameTypeIDR = 1,
        videoFrameTypeI = 2,
        videoFrameTypeP = 3,
        videoFrameTypeSkip = 4,
        videoFrameTypeIPMixed = 5,
    }

    /// Decoding state bitmask flags (`DECODING_STATE_FLAGS`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum DECODING_STATE_FLAGS {
        dsErrorFree = 0x00,
        dsFramePending = 0x01,
        dsRefLost = 0x02,
        dsBitstreamError = 0x04,
        dsDepLayerLost = 0x08,
        dsNoParamSets = 0x10,
        dsDataErrorConcealed = 0x20,
        dsRefListNullPtrs = 0x40,
        dsInvalidArgument = 0x1000,
        dsInitialOptExpected = 0x2000,
        dsOutOfMemory = 0x4000,
        dsDstBufNeedExpan = 0x8000,
    }

    /// Trace log levels (`WelsLogLevel`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum WelsLogLevel {
        WELS_LOG_QUIET = 0x00,
        WELS_LOG_ERROR = 0x01,
        WELS_LOG_WARNING = 0x02,
        WELS_LOG_INFO = 0x04,
        WELS_LOG_DEBUG = 0x08,
        WELS_LOG_DETAIL = 0x10,
    }

    /// Encoder option identifiers (`ENCODER_OPTION`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum ENCODER_OPTION {
        ENCODER_OPTION_DATAFORMAT = 0,
        ENCODER_OPTION_IDR_INTERVAL = 1,
        ENCODER_OPTION_SVC_ENCODE_PARAM_BASE = 2,
        ENCODER_OPTION_SVC_ENCODE_PARAM_EXT = 3,
        ENCODER_OPTION_FRAME_RATE = 4,
        ENCODER_OPTION_BITRATE = 5,
        ENCODER_OPTION_MAX_BITRATE = 6,
        ENCODER_OPTION_INTER_SPATIAL_PRED = 7,
        ENCODER_OPTION_RC_MODE = 8,
        ENCODER_OPTION_RC_FRAME_SKIP = 9,
        ENCODER_PADDING_PADDING = 10,
        ENCODER_OPTION_PROFILE = 11,
        ENCODER_OPTION_LEVEL = 12,
        ENCODER_OPTION_NUMBER_REF = 13,
        ENCODER_OPTION_DELIVERY_STATUS = 14,
        ENCODER_LTR_RECOVERY_REQUEST = 15,
        ENCODER_LTR_MARKING_FEEDBACK = 16,
        ENCODER_LTR_MARKING_PERIOD = 17,
        ENCODER_OPTION_LTR = 18,
        ENCODER_OPTION_COMPLEXITY = 19,
        ENCODER_OPTION_ENABLE_SSEI = 20,
        ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING = 21,
        ENCODER_OPTION_SPS_PPS_ID_STRATEGY = 22,
        ENCODER_OPTION_CURRENT_PATH = 23,
        ENCODER_OPTION_DUMP_FILE = 24,
        ENCODER_OPTION_TRACE_LEVEL = 25,
        ENCODER_OPTION_TRACE_CALLBACK = 26,
        ENCODER_OPTION_TRACE_CALLBACK_CONTEXT = 27,
        ENCODER_OPTION_GET_STATISTICS = 28,
        ENCODER_OPTION_STATISTICS_LOG_INTERVAL = 29,
        ENCODER_OPTION_IS_LOSSLESS_LINK = 30,
        ENCODER_OPTION_BITS_VARY_PERCENTAGE = 31,
    }

    /// Decoder option identifiers (`DECODER_OPTION`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum DECODER_OPTION {
        DECODER_OPTION_END_OF_STREAM = 1,
        DECODER_OPTION_VCL_NAL = 2,
        DECODER_OPTION_TEMPORAL_ID = 3,
        DECODER_OPTION_FRAME_NUM = 4,
        DECODER_OPTION_IDR_PIC_ID = 5,
        DECODER_OPTION_LTR_MARKING_FLAG = 6,
        DECODER_OPTION_LTR_MARKED_FRAME_NUM = 7,
        DECODER_OPTION_ERROR_CON_IDC = 8,
        DECODER_OPTION_TRACE_LEVEL = 9,
        DECODER_OPTION_TRACE_CALLBACK = 10,
        DECODER_OPTION_TRACE_CALLBACK_CONTEXT = 11,
        DECODER_OPTION_GET_STATISTICS = 12,
        DECODER_OPTION_GET_SAR_INFO = 13,
        DECODER_OPTION_PROFILE = 14,
        DECODER_OPTION_LEVEL = 15,
        DECODER_OPTION_STATISTICS_LOG_INTERVAL = 16,
        DECODER_OPTION_IS_REF_PIC = 17,
        DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER = 18,
        DECODER_OPTION_NUM_OF_THREADS = 19,
    }

    /// Error concealment modes (`ERROR_CON_IDC`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum ERROR_CON_IDC {
        ERROR_CON_DISABLE = 0,
        ERROR_CON_FRAME_COPY = 1,
        ERROR_CON_SLICE_COPY = 2,
        ERROR_CON_FRAME_COPY_CROSS_IDR = 3,
        ERROR_CON_SLICE_COPY_CROSS_IDR = 4,
        ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 5,
        ERROR_CON_SLICE_MV_COPY_CROSS_IDR = 6,
        ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 7,
    }

    /// Video bitstream type (`VIDEO_BITSTREAM_TYPE`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum VIDEO_BITSTREAM_TYPE {
        VIDEO_BITSTREAM_AVC = 0,
        VIDEO_BITSTREAM_SVC = 1,
    }

    /// Rate control modes (`RC_MODES`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum RC_MODES {
        RC_OFF_MODE = -1,
        RC_QUALITY_MODE = 0,
        RC_BITRATE_MODE = 1,
        RC_BUFFERBASED_MODE = 2,
        RC_TIMESTAMP_MODE = 3,
        RC_BITRATE_MODE_POST_SKIP = 4,
    }

    /// Profile IDC enumeration (`EProfileIdc`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum EProfileIdc {
        PRO_UNKNOWN = 0,
        PRO_BASELINE = 66,
        PRO_MAIN = 77,
        PRO_EXTENDED = 88,
        PRO_HIGH = 100,
        PRO_HIGH10 = 110,
        PRO_HIGH422 = 122,
        PRO_HIGH444 = 144,
        PRO_CAVLC444 = 244,
        PRO_SCALABLE_BASELINE = 83,
        PRO_SCALABLE_HIGH = 86,
    }

    /// Level IDC enumeration (`ELevelIdc`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum ELevelIdc {
        LEVEL_UNKNOWN = 0,
        LEVEL_1_0 = 10,
        LEVEL_1_B = 9,
        LEVEL_1_1 = 11,
        LEVEL_1_2 = 12,
        LEVEL_1_3 = 13,
        LEVEL_2_0 = 20,
        LEVEL_2_1 = 21,
        LEVEL_2_2 = 22,
        LEVEL_3_0 = 30,
        LEVEL_3_1 = 31,
        LEVEL_3_2 = 32,
        LEVEL_4_0 = 40,
        LEVEL_4_1 = 41,
        LEVEL_4_2 = 42,
        LEVEL_5_0 = 50,
        LEVEL_5_1 = 51,
        LEVEL_5_2 = 52,
    }

    /// Slicing modes (`SliceModeEnum`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum SliceModeEnum {
        SM_SINGLE_SLICE = 0,
        SM_FIXEDSLCNUM_SLICE = 1,
        SM_RASTER_SLICE = 2,
        SM_SIZELIMITED_SLICE = 3,
        SM_RESERVED = 4,
    }

    /// Sample aspect ratio (`ESampleAspectRatio`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum ESampleAspectRatio {
        ASP_UNSPECIFIED = 0,
        ASP_1x1 = 1,
        ASP_12x11 = 2,
        ASP_10x11 = 3,
        ASP_16x11 = 4,
        ASP_40x33 = 5,
        ASP_24x11 = 6,
        ASP_20x11 = 7,
        ASP_32x11 = 8,
        ASP_80x33 = 9,
        ASP_18x11 = 10,
        ASP_15x11 = 11,
        ASP_64x33 = 12,
        ASP_160x99 = 13,
        ASP_EXT_SAR = 255,
    }

    /// Encoder application scenario / usage type (`EUsageType`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum EUsageType {
        CAMERA_VIDEO_REAL_TIME = 0,
        SCREEN_CONTENT_REAL_TIME = 1,
        CAMERA_VIDEO_NON_REAL_TIME = 2,
        SCREEN_CONTENT_NON_REAL_TIME = 3,
        INPUT_CONTENT_TYPE_ALL = 4,
    }

    /// Encoder complexity modes (`ECOMPLEXITY_MODE`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum ECOMPLEXITY_MODE {
        LOW_COMPLEXITY = 0,
        MEDIUM_COMPLEXITY = 1,
        HIGH_COMPLEXITY = 2,
    }

    /// Parameter set strategy (`EParameterSetStrategy`).
    #[repr(i32)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum EParameterSetStrategy {
        CONSTANT_ID = 0,
        INCREASING_ID = 0x01,
        SPS_LISTING = 0x02,
        SPS_LISTING_AND_PPS_INCREASING = 0x03,
        SPS_PPS_LISTING = 0x06,
    }

    /// OpenH264 version record (`OpenH264Version`).
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub struct OpenH264Version {
        pub uMajor: u32,
        pub uMinor: u32,
        pub uRevision: u32,
        pub uReserved: u32,
    }

    /// Slice configuration (`SSliceArgument`).
    #[derive(Debug, Copy, Clone)]
    pub struct SSliceArgument {
        pub uiSliceMode: SliceModeEnum,
        pub uiSliceNum: u32,
        pub uiSliceMbNum: [u32; 35],
        pub uiSliceSizeConstraint: u32,
    }

    /// Spatial layer configuration (`SSpatialLayerConfig`).
    #[derive(Debug, Copy, Clone)]
    pub struct SSpatialLayerConfig {
        pub iVideoWidth: i32,
        pub iVideoHeight: i32,
        pub fFrameRate: f32,
        pub iSpatialBitrate: i32,
        pub iMaxSpatialBitrate: i32,
        pub uiProfileIdc: EProfileIdc,
        pub uiLevelIdc: ELevelIdc,
        pub iDLayerQp: i32,

        pub sSliceArgument: SSliceArgument,

        pub bVideoSignalTypePresent: bool,
        pub uiVideoFormat: u8,
        pub bFullRange: bool,
        pub bColorDescriptionPresent: bool,
        pub uiColorPrimaries: u8,
        pub uiTransferCharacteristics: u8,
        pub uiColorMatrix: u8,

        pub bAspectRatioPresent: bool,
        pub eAspectRatio: ESampleAspectRatio,
        pub sAspectRatioExtWidth: u16,
        pub sAspectRatioExtHeight: u16,
    }

    /// Basic encoder parameters (`SEncParamBase`).
    #[derive(Debug, Copy, Clone)]
    pub struct SEncParamBase {
        pub iUsageType: EUsageType,
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub iTargetBitrate: i32,
        pub iRCMode: RC_MODES,
        pub fMaxFrameRate: f32,
    }

    /// Extended encoder parameters (`SEncParamExt`).
    #[derive(Debug, Copy, Clone)]
    pub struct SEncParamExt {
        pub iUsageType: EUsageType,
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub iTargetBitrate: i32,
        pub iRCMode: RC_MODES,
        pub fMaxFrameRate: f32,

        pub iTemporalLayerNum: i32,
        pub iSpatialLayerNum: i32,
        pub sSpatialLayers: [SSpatialLayerConfig; 4],

        pub iComplexityMode: ECOMPLEXITY_MODE,
        pub uiIntraPeriod: u32,
        pub iNumRefFrame: i32,
        pub eSpsPpsIdStrategy: EParameterSetStrategy,
        pub bPrefixNalAddingCtrl: bool,
        pub bEnableSSEI: bool,
        pub bSimulcastAVC: bool,
        pub iPaddingFlag: i32,
        pub iEntropyCodingModeFlag: i32,

        pub bEnableFrameSkip: bool,
        pub iMaxBitrate: i32,
        pub iMaxQp: i32,
        pub iMinQp: i32,
        pub uiMaxNalSize: u32,

        pub bEnableLongTermReference: bool,
        pub iLTRRefNum: i32,
        pub iLtrMarkPeriod: u32,

        pub iMultipleThreadIdc: u16,
        pub bUseLoadBalancing: bool,

        pub iLoopFilterDisableIdc: i32,
        pub iLoopFilterAlphaC0Offset: i32,
        pub iLoopFilterBetaOffset: i32,

        pub bEnableDenoise: bool,
        pub bEnableBackgroundDetection: bool,
        pub bEnableAdaptiveQuant: bool,
        pub bEnableFrameCroppingFlag: bool,
        pub bEnableSceneChangeDetect: bool,

        pub bIsLosslessLink: bool,
        pub bFixRCOverShoot: bool,
        pub iIdrBitrateRatio: i32,
        pub bPsnrY: bool,
        pub bPsnrU: bool,
        pub bPsnrV: bool,
    }

    /// Uncompressed input picture (`SSourcePicture`).
    #[derive(Debug, Copy, Clone)]
    pub struct SSourcePicture {
        pub iColorFormat: i32,
        pub iStride: [i32; 4],
        pub pData: [*mut u8; 4],
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub uiTimeStamp: i64,
        pub bPsnrY: bool,
        pub bPsnrU: bool,
        pub bPsnrV: bool,
    }

    /// Bitstream layer output descriptor (`SLayerBSInfo`).
    #[derive(Debug, Copy, Clone)]
    pub struct SLayerBSInfo {
        pub uiTemporalId: u8,
        pub uiSpatialId: u8,
        pub uiQualityId: u8,
        pub eFrameType: EVideoFrameType,
        pub uiLayerType: u8,
        pub iSubSeqId: i32,
        pub iNalCount: i32,
        pub pNalLengthInByte: *mut i32,
        pub pBsBuf: *mut u8,
        pub rPsnr: [f32; 3],
    }

    /// Bitstream frame output descriptor (`SFrameBSInfo`).
    #[derive(Debug, Copy, Clone)]
    pub struct SFrameBSInfo {
        pub iLayerNum: i32,
        pub sLayerInfo: [SLayerBSInfo; 128],
        pub eFrameType: EVideoFrameType,
        pub iFrameSizeInBytes: i32,
        pub uiTimeStamp: i64,
    }

    /// Video bitstream property descriptor (`SVideoProperty`).
    #[derive(Debug, Copy, Clone)]
    pub struct SVideoProperty {
        pub size: u32,
        pub eVideoBsType: VIDEO_BITSTREAM_TYPE,
    }

    /// Decoder initialization parameters (`SDecodingParam`).
    #[derive(Debug, Copy, Clone)]
    pub struct SDecodingParam {
        pub pFileNameRestructed: *mut c_char,
        pub uiCpuLoad: u32,
        pub uiTargetDqLayer: u8,
        pub eEcActiveIdc: ERROR_CON_IDC,
        pub bParseOnly: bool,
        pub sVideoProperty: SVideoProperty,
    }

    /// System memory buffer information (`SSysMEMBuffer`).
    #[derive(Debug, Copy, Clone)]
    pub struct SSysMEMBuffer {
        pub iWidth: i32,
        pub iHeight: i32,
        pub iFormat: i32,
        pub iStride: [i32; 2],
    }

    /// Decoded frame destination buffer information payload (`SBufferInfoUsrData`).
    #[derive(Debug, Copy, Clone)]
    pub struct SBufferInfoUsrData {
        pub sSystemBuffer: SSysMEMBuffer,
    }

    /// Decoded frame destination buffer metadata (`SBufferInfo`).
    #[derive(Debug, Copy, Clone)]
    pub struct SBufferInfo {
        pub iBufferStatus: i32,
        pub uiInBsTimeStamp: u64,
        pub uiOutYuvTimeStamp: u64,
        pub UsrData: SBufferInfoUsrData,
        pub pDst: [*mut u8; 3],
    }

    /// Parsed bitstream output descriptor (`SParserBsInfo`).
    #[derive(Debug, Copy, Clone)]
    pub struct SParserBsInfo {
        pub iNalNum: i32,
        pub pNalLenInByte: *mut i32,
        pub pDstBuff: *mut u8,
        pub iSpsWidthInPixel: i32,
        pub iSpsHeightInPixel: i32,
        pub uiInBsTimeStamp: u64,
        pub uiOutBsTimeStamp: u64,
    }

    /// Decoder capability descriptor (`SDecoderCapability`).
    #[derive(Debug, Copy, Clone)]
    pub struct SDecoderCapability {
        pub iProfileIdc: i32,
        pub iProfileIop: i32,
        pub iLevelIdc: i32,
        pub iMaxMbps: i32,
        pub iMaxFs: i32,
        pub iMaxCpb: i32,
        pub iMaxDpb: i32,
        pub iMaxBr: i32,
        pub bRedPicCap: bool,
    }

    extern "Rust" {
        type c_void;
        type ISVCEncoder;
        type ISVCDecoder;

        // ISVCEncoder methods
        #[cxx_name = "Initialize"]
        unsafe fn initialize(self: &mut ISVCEncoder, param: *const SEncParamBase) -> i32;

        #[cxx_name = "InitializeExt"]
        unsafe fn initialize_ext(self: &mut ISVCEncoder, param: *const SEncParamExt) -> i32;

        #[cxx_name = "GetDefaultParams"]
        unsafe fn get_default_params(self: &mut ISVCEncoder, param: *mut SEncParamExt) -> i32;

        #[cxx_name = "Uninitialize"]
        fn uninitialize(self: &mut ISVCEncoder) -> i32;

        #[cxx_name = "EncodeFrame"]
        unsafe fn encode_frame(
            self: &mut ISVCEncoder,
            src_pic: *const SSourcePicture,
            bs_info: *mut SFrameBSInfo,
        ) -> i32;

        #[cxx_name = "EncodeParameterSets"]
        unsafe fn encode_parameter_sets(self: &mut ISVCEncoder, bs_info: *mut SFrameBSInfo) -> i32;

        #[cxx_name = "ForceIntraFrame"]
        fn force_intra_frame(self: &mut ISVCEncoder, idr: bool, layer_id: i32) -> i32;

        #[cxx_name = "SetOption"]
        unsafe fn set_option(
            self: &mut ISVCEncoder,
            option_id: ENCODER_OPTION,
            option: *mut c_void,
        ) -> i32;

        #[cxx_name = "GetOption"]
        unsafe fn get_option(
            self: &mut ISVCEncoder,
            option_id: ENCODER_OPTION,
            option: *mut c_void,
        ) -> i32;

        // ISVCDecoder methods
        #[cxx_name = "Initialize"]
        unsafe fn initialize(self: &mut ISVCDecoder, param: *const SDecodingParam) -> isize;

        #[cxx_name = "Uninitialize"]
        unsafe fn uninitialize(self: &mut ISVCDecoder) -> isize;

        #[cxx_name = "DecodeFrame"]
        unsafe fn decode_frame(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            stride: *mut i32,
            width: *mut i32,
            height: *mut i32,
        ) -> i32;

        #[cxx_name = "DecodeFrameNoDelay"]
        unsafe fn decode_frame_no_delay(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> i32;

        #[cxx_name = "DecodeFrame2"]
        unsafe fn decode_frame2(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> i32;

        #[cxx_name = "FlushFrame"]
        unsafe fn flush_frame(
            self: &mut ISVCDecoder,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> i32;

        #[cxx_name = "DecodeParser"]
        unsafe fn decode_parser(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst_info: *mut SParserBsInfo,
        ) -> i32;

        #[cxx_name = "DecodeFrameEx"]
        unsafe fn decode_frame_ex(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut u8,
            dst_stride: i32,
            dst_len: *mut i32,
            width: *mut i32,
            height: *mut i32,
            color_format: *mut i32,
        ) -> i32;

        #[cxx_name = "SetOption"]
        unsafe fn set_option(
            self: &mut ISVCDecoder,
            option_id: DECODER_OPTION,
            option: *mut c_void,
        ) -> isize;

        #[cxx_name = "GetOption"]
        unsafe fn get_option(
            self: &mut ISVCDecoder,
            option_id: DECODER_OPTION,
            option: *mut c_void,
        ) -> isize;

        // Global lifecycle & query functions
        #[cxx_name = "WelsCreateSVCEncoder"]
        unsafe fn wels_create_svc_encoder(pp_encoder: *mut *mut ISVCEncoder) -> i32;

        #[cxx_name = "WelsDestroySVCEncoder"]
        unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder);

        #[cxx_name = "WelsCreateDecoder"]
        unsafe fn wels_create_decoder(pp_decoder: *mut *mut ISVCDecoder) -> isize;

        #[cxx_name = "WelsDestroyDecoder"]
        unsafe fn wels_destroy_decoder(p_decoder: *mut ISVCDecoder);

        #[cxx_name = "WelsGetDecoderCapability"]
        unsafe fn wels_get_decoder_capability(p_dec_capability: *mut SDecoderCapability) -> i32;

        #[cxx_name = "WelsGetCodecVersion"]
        fn wels_get_codec_version() -> OpenH264Version;

        #[cxx_name = "WelsGetCodecVersionEx"]
        unsafe fn wels_get_codec_version_ex(p_version: *mut OpenH264Version);
    }
}

impl Default for ffi::SEncParamExt {
    fn default() -> Self {
        rust_types::SEncParamExt::default().into()
    }
}

impl Default for ffi::SSourcePicture {
    fn default() -> Self {
        Self {
            iColorFormat: rust_types::EVideoFormatType::videoFormatI420 as i32,
            iStride: [0; 4],
            pData: [ptr::null_mut(); 4],
            iPicWidth: 0,
            iPicHeight: 0,
            uiTimeStamp: 0,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,
        }
    }
}

impl Default for ffi::SLayerBSInfo {
    fn default() -> Self {
        rust_types::SLayerBSInfo::default().into()
    }
}

impl Default for ffi::SFrameBSInfo {
    fn default() -> Self {
        rust_types::SFrameBSInfo::default().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unsafe_code)]
    fn cxx_encoder_and_decoder_lifecycle() {
        unsafe {
            let mut encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(wels_create_svc_encoder(&mut encoder), CM_RESULT_SUCCESS);
            assert!(!encoder.is_null());

            let enc = &mut *encoder;
            let mut param = ffi::SEncParamExt::default();
            assert_eq!(enc.get_default_params(&mut param), CM_RESULT_SUCCESS);

            param.iPicWidth = 320;
            param.iPicHeight = 192;
            param.iTargetBitrate = 500_000;
            param.fMaxFrameRate = 30.0;
            param.sSpatialLayers[0].iVideoWidth = 320;
            param.sSpatialLayers[0].iVideoHeight = 192;
            param.sSpatialLayers[0].fFrameRate = 30.0;
            param.sSpatialLayers[0].iSpatialBitrate = 500_000;

            assert_eq!(enc.initialize_ext(&param), CM_RESULT_SUCCESS);
            assert_eq!(enc.force_intra_frame(true, -1), CM_RESULT_SUCCESS);

            let width = 320usize;
            let height = 192usize;
            let mut y_plane = vec![128u8; width * height];
            let mut u_plane = vec![128u8; (width / 2) * (height / 2)];
            let mut v_plane = vec![128u8; (width / 2) * (height / 2)];

            let mut src_pic = ffi::SSourcePicture::default();
            src_pic.iPicWidth = width as i32;
            src_pic.iPicHeight = height as i32;
            src_pic.iStride = [width as i32, (width / 2) as i32, (width / 2) as i32, 0];
            src_pic.pData = [
                y_plane.as_mut_ptr(),
                u_plane.as_mut_ptr(),
                v_plane.as_mut_ptr(),
                ptr::null_mut(),
            ];

            let mut bs_info = ffi::SFrameBSInfo::default();
            assert_eq!(enc.encode_frame(&src_pic, &mut bs_info), CM_RESULT_SUCCESS);
            assert!(bs_info.iFrameSizeInBytes > 0);
            assert!(bs_info.iLayerNum > 0);

            assert_eq!(enc.uninitialize(), CM_RESULT_SUCCESS);
            wels_destroy_svc_encoder(encoder);
        }
    }
}
