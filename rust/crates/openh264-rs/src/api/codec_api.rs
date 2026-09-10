//! OpenH264 Public C/C++ API Architecture (`codec_api.h`).
//!
//! Provides the public API interface definitions, C-compatible vtables,
//! dynamic library export bindings, versioning structures, and factory lifecycles
//! for both the H.264 / SVC video encoder (`ISVCEncoder`) and decoder (`ISVCDecoder`).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use crate::decoder::decoder_context::{
    LIST_0, MAX_DPB_COUNT, PICT_INFO_LIST_SIZE, parser_bs, pic_pool_ptr, prev_dpb_id,
    prev_dpb_pic_mut, slice_header_of,
};
use crate::decoder::decoder_core::{
    ERR_NONE, OutputStatisticsLog, ResetDecStatNums, WelsDecoderLastDecPicInfoDefaults,
    WelsDecoderSpsPpsDefaults, WelsInitStaticMemory,
};
use crate::decoder::nalu::{EWelsNalUnitType, IS_PARAM_SETS_NALS};
use crate::decoder::pic_queue::SPicBuff;
use std::ffi::{c_char, c_long, c_void};
use std::ptr;

pub const MAX_TEMPORAL_LAYER_NUM: usize = 4;
pub const MAX_SPATIAL_LAYER_NUM: usize = 4;
pub const MAX_QUALITY_LAYER_NUM: usize = 4;

pub const MAX_LAYER_NUM_OF_FRAME: usize = 128;
pub const MAX_NAL_UNITS_IN_LAYER: usize = 128;

pub const MAX_RTP_PAYLOAD_LEN: usize = 1000;
pub const AVERAGE_RTP_PAYLOAD_LEN: usize = 800;

pub const SAVED_NALUNIT_NUM_TMP: usize =
    (MAX_SPATIAL_LAYER_NUM * MAX_QUALITY_LAYER_NUM) + 1 + MAX_SPATIAL_LAYER_NUM;
pub const MAX_SLICES_NUM_TMP: usize = (MAX_NAL_UNITS_IN_LAYER - SAVED_NALUNIT_NUM_TMP) / 3;

pub const AUTO_REF_PIC_COUNT: i32 = -1;
pub const UNSPECIFIED_BIT_RATE: i32 = 0;

pub const FRAME_NUM_PARAM_SET: i32 = -1;
pub const FRAME_NUM_IDR: i32 = 0;

// Error & Return Codes
pub const CM_RESULT_SUCCESS: i32 = 0;
pub const CM_INIT_PARA_ERROR: i32 = 1;
pub const CM_UNKNOWN_REASON: i32 = 2;
pub const CM_MALLOC_MEM_ERROR: i32 = 3;
pub const CM_INIT_EXPECTED: i32 = 4;
pub const CM_UNSUPPORTED_DATA: i32 = 5;

/// Return codes enumeration (`CM_RETURN`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum CM_RETURN {
    #[default]
    cmResultSuccess = 0,
    cmInitParaError = 1,
    cmUnknownReason = 2,
    cmMallocMemeError = 3,
    cmInitExpected = 4,
    cmUnsupportedData = 5,
}

/// Enumerate video format types (`EVideoFormatType`).
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
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
    #[default]
    videoFormatI420 = 23,
    videoFormatYV12 = 24,
    videoFormatInternal = 25,
    videoFormatNV12 = 26,
    videoFormatVFlip = -0x80000000i32,
}

pub type VideoFormat = EVideoFormatType;

/// Enumerate video frame types (`EVideoFrameType`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EVideoFrameType {
    #[default]
    videoFrameTypeInvalid = 0,
    videoFrameTypeIDR = 1,
    videoFrameTypeI = 2,
    videoFrameTypeP = 3,
    videoFrameTypeSkip = 4,
    videoFrameTypeIPMixed = 5,
}

/// NAL unit types (`ENalUnitType`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ENalUnitType {
    #[default]
    NAL_UNKNOWN = 0,
    NAL_SLICE = 1,
    NAL_SLICE_DPA = 2,
    NAL_SLICE_DPB = 3,
    NAL_SLICE_DPC = 4,
    NAL_SLICE_IDR = 5,
    NAL_SEI = 6,
    NAL_SPS = 7,
    NAL_PPS = 8,
}

/// NAL reference priority (`ENalPriority`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ENalPriority {
    #[default]
    NAL_PRIORITY_DISPOSABLE = 0,
    NAL_PRIORITY_LOW = 1,
    NAL_PRIORITY_HIGH = 2,
    NAL_PRIORITY_HIGHEST = 3,
}

/// Decoding status bitmask (`DECODING_STATE`).
///
/// A bitmask, not an enumeration: the decoder accumulates into `iErrorCode` with
/// `|=` and returns the accumulator whole, so combined values such as
/// `dsBitstreamError | dsDataErrorConcealed` (`0x24`) name no variant.
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Default)]
pub struct DECODING_STATE(pub i32);

#[allow(non_upper_case_globals)]
impl DECODING_STATE {
    pub const dsErrorFree: Self = Self(0x00);
    pub const dsFramePending: Self = Self(0x01);
    pub const dsRefLost: Self = Self(0x02);
    pub const dsBitstreamError: Self = Self(0x04);
    pub const dsDepLayerLost: Self = Self(0x08);
    pub const dsNoParamSets: Self = Self(0x10);
    pub const dsDataErrorConcealed: Self = Self(0x20);
    pub const dsRefListNullPtrs: Self = Self(0x40);

    pub const dsInvalidArgument: Self = Self(0x1000);
    pub const dsInitialOptExpected: Self = Self(0x2000);
    pub const dsOutOfMemory: Self = Self(0x4000);
    pub const dsDstBufNeedExpan: Self = Self(0x8000);

    /// The set bits, named, in header order, so `{:?}` of a combined value reads as
    /// names rather than a number.
    const NAMES: [(i32, &'static str); 12] = [
        (0x01, "dsFramePending"),
        (0x02, "dsRefLost"),
        (0x04, "dsBitstreamError"),
        (0x08, "dsDepLayerLost"),
        (0x10, "dsNoParamSets"),
        (0x20, "dsDataErrorConcealed"),
        (0x40, "dsRefListNullPtrs"),
        (0x1000, "dsInvalidArgument"),
        (0x2000, "dsInitialOptExpected"),
        (0x4000, "dsOutOfMemory"),
        (0x8000, "dsDstBufNeedExpan"),
        (0, ""),
    ];
}

impl core::fmt::Debug for DECODING_STATE {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == 0 {
            return f.write_str("dsErrorFree");
        }
        let mut first = true;
        let mut rest = self.0;
        for (bit, name) in Self::NAMES {
            if bit != 0 && self.0 & bit != 0 {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
                rest &= !bit;
            }
        }
        if rest != 0 {
            if !first {
                f.write_str("|")?;
            }
            write!(f, "{rest:#x}")?;
        }
        Ok(())
    }
}

/// Encoder option identifiers (`ENCODER_OPTION`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ENCODER_OPTION {
    #[default]
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

pub type EncoderOption = ENCODER_OPTION;

/// Decoder option identifiers (`DECODER_OPTION`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum DECODER_OPTION {
    #[default]
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

pub type DecoderOption = DECODER_OPTION;

/// Error concealment modes (`ERROR_CON_IDC`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ERROR_CON_IDC {
    #[default]
    ERROR_CON_DISABLE = 0,
    ERROR_CON_FRAME_COPY = 1,
    ERROR_CON_SLICE_COPY = 2,
    ERROR_CON_FRAME_COPY_CROSS_IDR = 3,
    ERROR_CON_SLICE_COPY_CROSS_IDR = 4,
    ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 5,
    ERROR_CON_SLICE_MV_COPY_CROSS_IDR = 6,
    ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE = 7,
}

/// Feedback VCL NAL state (`FEEDBACK_VCL_NAL_IN_AU`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum FEEDBACK_VCL_NAL_IN_AU {
    #[default]
    FEEDBACK_NON_VCL_NAL = 0,
    FEEDBACK_VCL_NAL = 1,
    FEEDBACK_UNKNOWN_NAL = 2,
}

/// Layer type being encoded (`LAYER_TYPE`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LAYER_TYPE {
    #[default]
    NON_VIDEO_CODING_LAYER = 0,
    VIDEO_CODING_LAYER = 1,
}

/// Spatial layer enumeration (`LAYER_NUM`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LAYER_NUM {
    #[default]
    SPATIAL_LAYER_0 = 0,
    SPATIAL_LAYER_1 = 1,
    SPATIAL_LAYER_2 = 2,
    SPATIAL_LAYER_3 = 3,
    SPATIAL_LAYER_ALL = 4,
}

/// Video bitstream type (`VIDEO_BITSTREAM_TYPE`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum VIDEO_BITSTREAM_TYPE {
    VIDEO_BITSTREAM_AVC = 0,
    #[default]
    VIDEO_BITSTREAM_SVC = 1,
}

pub const VIDEO_BITSTREAM_DEFAULT: VIDEO_BITSTREAM_TYPE = VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC;

/// Keyframe request type (`KEY_FRAME_REQUEST_TYPE`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum KEY_FRAME_REQUEST_TYPE {
    #[default]
    NO_RECOVERY_REQUSET = 0,
    LTR_RECOVERY_REQUEST = 1,
    IDR_RECOVERY_REQUEST = 2,
    NO_LTR_MARKING_FEEDBACK = 3,
    LTR_MARKING_SUCCESS = 4,
    LTR_MARKING_FAILED = 5,
}

/// Rate control modes (`RC_MODES`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum RC_MODES {
    #[default]
    RC_QUALITY_MODE = 0,
    RC_BITRATE_MODE = 1,
    RC_BUFFERBASED_MODE = 2,
    RC_TIMESTAMP_MODE = 3,
    RC_BITRATE_MODE_POST_SKIP = 4,
    RC_OFF_MODE = -1,
}

pub type RCMode = RC_MODES;

/// Profile IDC enumeration (`EProfileIdc`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EProfileIdc {
    #[default]
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
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ELevelIdc {
    #[default]
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
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SliceModeEnum {
    #[default]
    SM_SINGLE_SLICE = 0,
    SM_FIXEDSLCNUM_SLICE = 1,
    SM_RASTER_SLICE = 2,
    SM_SIZELIMITED_SLICE = 3,
    SM_RESERVED = 4,
}

pub type SliceMode = SliceModeEnum;

/// Video format in SPS VUI (`EVideoFormatSPS`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EVideoFormatSPS {
    #[default]
    VF_COMPONENT = 0,
    VF_PAL = 1,
    VF_NTSC = 2,
    VF_SECAM = 3,
    VF_MAC = 4,
    VF_UNDEF = 5,
    VF_NUM_ENUM = 6,
}

/// Color primaries (`EColorPrimaries`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EColorPrimaries {
    #[default]
    CP_RESERVED0 = 0,
    CP_BT709 = 1,
    CP_UNDEF = 2,
    CP_RESERVED3 = 3,
    CP_BT470M = 4,
    CP_BT470BG = 5,
    CP_SMPTE170M = 6,
    CP_SMPTE240M = 7,
    CP_FILM = 8,
    CP_BT2020 = 9,
    CP_NUM_ENUM = 10,
}

/// Transfer characteristics (`ETransferCharacteristics`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ETransferCharacteristics {
    #[default]
    TRC_RESERVED0 = 0,
    TRC_BT709 = 1,
    TRC_UNDEF = 2,
    TRC_RESERVED3 = 3,
    TRC_BT470M = 4,
    TRC_BT470BG = 5,
    TRC_SMPTE170M = 6,
    TRC_SMPTE240M = 7,
    TRC_LINEAR = 8,
    TRC_LOG100 = 9,
    TRC_LOG316 = 10,
    TRC_IEC61966_2_4 = 11,
    TRC_BT1361E = 12,
    TRC_IEC61966_2_1 = 13,
    TRC_BT2020_10 = 14,
    TRC_BT2020_12 = 15,
    TRC_NUM_ENUM = 16,
}

/// Color matrix (`EColorMatrix`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EColorMatrix {
    #[default]
    CM_GBR = 0,
    CM_BT709 = 1,
    CM_UNDEF = 2,
    CM_RESERVED3 = 3,
    CM_FCC = 4,
    CM_BT470BG = 5,
    CM_SMPTE170M = 6,
    CM_SMPTE240M = 7,
    CM_YCGCO = 8,
    CM_BT2020NC = 9,
    CM_BT2020C = 10,
    CM_NUM_ENUM = 11,
}

/// Sample aspect ratio (`ESampleAspectRatio`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ESampleAspectRatio {
    #[default]
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
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EUsageType {
    #[default]
    CAMERA_VIDEO_REAL_TIME = 0,
    SCREEN_CONTENT_REAL_TIME = 1,
    CAMERA_VIDEO_NON_REAL_TIME = 2,
    SCREEN_CONTENT_NON_REAL_TIME = 3,
    INPUT_CONTENT_TYPE_ALL = 4,
}

/// Encoder complexity modes (`ECOMPLEXITY_MODE`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ECOMPLEXITY_MODE {
    #[default]
    LOW_COMPLEXITY = 0,
    MEDIUM_COMPLEXITY = 1,
    HIGH_COMPLEXITY = 2,
}

pub type EComplexityMode = ECOMPLEXITY_MODE;

/// Parameter set strategy (`EParameterSetStrategy`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EParameterSetStrategy {
    CONSTANT_ID = 0,
    #[default]
    INCREASING_ID = 0x01,
    SPS_LISTING = 0x02,
    SPS_LISTING_AND_PPS_INCREASING = 0x03,
    SPS_PPS_LISTING = 0x06,
}

/// OpenH264 version record (`OpenH264Version`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct OpenH264Version {
    pub uMajor: u32,
    pub uMinor: u32,
    pub uRevision: u32,
    pub uReserved: u32,
}

/// Slice configuration structure (`SSliceArgument`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceArgument {
    pub uiSliceMode: SliceModeEnum,
    pub uiSliceNum: u32,
    pub uiSliceMbNum: [u32; MAX_SLICES_NUM_TMP],
    pub uiSliceSizeConstraint: u32,
}

impl Default for SSliceArgument {
    fn default() -> Self {
        Self {
            uiSliceMode: SliceModeEnum::SM_SINGLE_SLICE,
            uiSliceNum: 0,
            uiSliceMbNum: [0; MAX_SLICES_NUM_TMP],
            uiSliceSizeConstraint: 0,
        }
    }
}

/// Spatial layer configuration (`SSpatialLayerConfig`).
#[repr(C)]
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

impl Default for SSpatialLayerConfig {
    fn default() -> Self {
        Self {
            iVideoWidth: 0,
            iVideoHeight: 0,
            fFrameRate: 0.0,
            iSpatialBitrate: 0,
            iMaxSpatialBitrate: 0,
            uiProfileIdc: EProfileIdc::PRO_UNKNOWN,
            uiLevelIdc: ELevelIdc::LEVEL_UNKNOWN,
            iDLayerQp: 0,
            sSliceArgument: SSliceArgument::default(),
            bVideoSignalTypePresent: false,
            uiVideoFormat: 0,
            bFullRange: false,
            bColorDescriptionPresent: false,
            uiColorPrimaries: 0,
            uiTransferCharacteristics: 0,
            uiColorMatrix: 0,
            bAspectRatioPresent: false,
            eAspectRatio: ESampleAspectRatio::ASP_UNSPECIFIED,
            sAspectRatioExtWidth: 0,
            sAspectRatioExtHeight: 0,
        }
    }
}

/// Basic encoder parameter structure (`SEncParamBase`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SEncParamBase {
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iRCMode: RC_MODES,
    pub fMaxFrameRate: f32,
}

pub type PEncParamBase = *mut SEncParamBase;

/// Extended encoder parameter structure (`SEncParamExt`).
#[repr(C)]
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
    pub sSpatialLayers: [SSpatialLayerConfig; MAX_SPATIAL_LAYER_NUM],

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

impl Default for SEncParamExt {
    fn default() -> Self {
        Self {
            iUsageType: EUsageType::CAMERA_VIDEO_REAL_TIME,
            iPicWidth: 0,
            iPicHeight: 0,
            iTargetBitrate: 0,
            iRCMode: RC_MODES::RC_QUALITY_MODE,
            fMaxFrameRate: 0.0,
            iTemporalLayerNum: 1,
            iSpatialLayerNum: 1,
            sSpatialLayers: [SSpatialLayerConfig::default(); MAX_SPATIAL_LAYER_NUM],
            iComplexityMode: ECOMPLEXITY_MODE::LOW_COMPLEXITY,
            uiIntraPeriod: 0,
            iNumRefFrame: 1,
            eSpsPpsIdStrategy: EParameterSetStrategy::INCREASING_ID,
            bPrefixNalAddingCtrl: false,
            bEnableSSEI: false,
            bSimulcastAVC: false,
            iPaddingFlag: 0,
            iEntropyCodingModeFlag: 0,
            bEnableFrameSkip: false,
            iMaxBitrate: UNSPECIFIED_BIT_RATE,
            iMaxQp: 51,
            iMinQp: 0,
            uiMaxNalSize: 0,
            bEnableLongTermReference: false,
            iLTRRefNum: 0,
            iLtrMarkPeriod: 0,
            iMultipleThreadIdc: 1,
            bUseLoadBalancing: false,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            bEnableDenoise: false,
            bEnableBackgroundDetection: true,
            bEnableAdaptiveQuant: true,
            bEnableFrameCroppingFlag: true,
            bEnableSceneChangeDetect: true,
            bIsLosslessLink: false,
            bFixRCOverShoot: false,
            iIdrBitrateRatio: 0,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,
        }
    }
}

/// Uncompressed input picture description (`SSourcePicture`).
#[repr(C)]
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

impl Default for SSourcePicture {
    fn default() -> Self {
        Self {
            iColorFormat: EVideoFormatType::videoFormatI420 as i32,
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

/// Coded layer bitstream metadata (`SLayerBSInfo`).
#[repr(C)]
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

impl Default for SLayerBSInfo {
    fn default() -> Self {
        Self {
            uiTemporalId: 0,
            uiSpatialId: 0,
            uiQualityId: 0,
            eFrameType: EVideoFrameType::videoFrameTypeInvalid,
            uiLayerType: 0,
            iSubSeqId: 0,
            iNalCount: 0,
            pNalLengthInByte: ptr::null_mut(),
            pBsBuf: ptr::null_mut(),
            rPsnr: [0.0; 3],
        }
    }
}

pub type PLayerBSInfo = *mut SLayerBSInfo;

/// Encoded frame bitstream container (`SFrameBSInfo`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SFrameBSInfo {
    pub iLayerNum: i32,
    pub sLayerInfo: [SLayerBSInfo; MAX_LAYER_NUM_OF_FRAME],
    pub eFrameType: EVideoFrameType,
    pub iFrameSizeInBytes: i32,
    pub uiTimeStamp: i64,
}

impl Default for SFrameBSInfo {
    fn default() -> Self {
        Self {
            iLayerNum: 0,
            sLayerInfo: [SLayerBSInfo::default(); MAX_LAYER_NUM_OF_FRAME],
            eFrameType: EVideoFrameType::videoFrameTypeInvalid,
            iFrameSizeInBytes: 0,
            uiTimeStamp: 0,
        }
    }
}

pub type PFrameBSInfo = *mut SFrameBSInfo;

/// Video bitstream property descriptor (`SVideoProperty`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVideoProperty {
    pub size: u32,
    pub eVideoBsType: VIDEO_BITSTREAM_TYPE,
}

/// Decoder initialization parameters (`SDecodingParam`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDecodingParam {
    pub pFileNameRestructed: *mut c_char,
    pub uiCpuLoad: u32,
    pub uiTargetDqLayer: u8,
    pub eEcActiveIdc: ERROR_CON_IDC,
    pub bParseOnly: bool,
    pub sVideoProperty: SVideoProperty,
}

impl Default for SDecodingParam {
    fn default() -> Self {
        Self {
            pFileNameRestructed: ptr::null_mut(),
            uiCpuLoad: 0,
            uiTargetDqLayer: 0,
            eEcActiveIdc: ERROR_CON_IDC::ERROR_CON_DISABLE,
            bParseOnly: false,
            sVideoProperty: SVideoProperty::default(),
        }
    }
}

pub type PDecodingParam = *mut SDecodingParam;

/// System memory buffer information (`SSysMEMBuffer`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSysMEMBuffer {
    pub iWidth: i32,
    pub iHeight: i32,
    pub iFormat: i32,
    pub iStride: [i32; 2],
}

/// Decoded frame destination buffer information union payload.
#[repr(C)]
#[derive(Copy, Clone)]
pub union SBufferInfoUsrData {
    pub sSystemBuffer: SSysMEMBuffer,
}

impl SBufferInfoUsrData {
    /// The union's one arm, as a value.
    #[allow(unsafe_code)]
    #[inline]
    pub fn sys(&self) -> &SSysMEMBuffer {
        // SAFETY: `SBufferInfoUsrData` declares exactly one variant.
        unsafe { &self.sSystemBuffer }
    }

    /// [`sys`](Self::sys)'s mutable form.
    #[allow(unsafe_code)]
    #[inline]
    pub fn sys_mut(&mut self) -> &mut SSysMEMBuffer {
        // SAFETY: `SBufferInfoUsrData` declares exactly one variant.
        unsafe { &mut self.sSystemBuffer }
    }
}

#[allow(unsafe_code)]
impl std::fmt::Debug for SBufferInfoUsrData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe { write!(f, "SBufferInfoUsrData({:?})", self.sSystemBuffer) }
    }
}

/// Decoded frame destination buffer metadata (`SBufferInfo`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBufferInfo {
    pub iBufferStatus: i32,
    pub uiInBsTimeStamp: u64,
    pub uiOutYuvTimeStamp: u64,
    pub UsrData: SBufferInfoUsrData,
    pub pDst: [*mut u8; 3],
}

impl Default for SBufferInfo {
    fn default() -> Self {
        Self {
            iBufferStatus: 0,
            uiInBsTimeStamp: 0,
            uiOutYuvTimeStamp: 0,
            UsrData: SBufferInfoUsrData {
                sSystemBuffer: SSysMEMBuffer::default(),
            },
            pDst: [ptr::null_mut(); 3],
        }
    }
}

/// Parsed bitstream output descriptor (`SParserBsInfo`).
#[repr(C)]
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

impl Default for SParserBsInfo {
    fn default() -> Self {
        Self {
            iNalNum: 0,
            pNalLenInByte: ptr::null_mut(),
            pDstBuff: ptr::null_mut(),
            iSpsWidthInPixel: 0,
            iSpsHeightInPixel: 0,
            uiInBsTimeStamp: 0,
            uiOutBsTimeStamp: 0,
        }
    }
}

pub type PParserBsInfo = *mut SParserBsInfo;

/// Decoder capability descriptor (`SDecoderCapability`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
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

/// Video encoder runtime statistics (`SEncoderStatistics`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SEncoderStatistics {
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
    pub iTotalEncodedBytes: std::ffi::c_ulong,
    pub iLastStatisticsBytes: std::ffi::c_ulong,
    pub iLastStatisticsFrameCount: std::ffi::c_ulong,
}

/// Video decoder runtime statistics (`SDecoderStatistics`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDecoderStatistics {
    pub uiWidth: u32,
    pub uiHeight: u32,
    pub fAverageFrameSpeedInMs: f32,
    pub fActualAverageFrameSpeedInMs: f32,
    pub uiDecodedFrameCount: u32,
    pub uiResolutionChangeTimes: u32,
    pub uiIDRCorrectNum: u32,
    pub uiAvgEcRatio: u32,
    pub uiAvgEcPropRatio: u32,
    pub uiEcIDRNum: u32,
    pub uiEcFrameNum: u32,
    pub uiIDRLostNum: u32,
    pub uiFreezingIDRNum: u32,
    pub uiFreezingNonIDRNum: u32,
    pub iAvgLumaQp: i32,
    pub iSpsReportErrorNum: i32,
    pub iSubSpsReportErrorNum: i32,
    pub iPpsReportErrorNum: i32,
    pub iSpsNoExistNalNum: i32,
    pub iSubSpsNoExistNalNum: i32,
    pub iPpsNoExistNalNum: i32,
    pub uiProfile: u32,
    pub uiLevel: u32,
    pub iCurrentActiveSpsId: i32,
    pub iCurrentActivePpsId: i32,
    pub iStatisticsLogInterval: u32,
}

/// VUI sample aspect ratio metadata (`SVuiSarInfo`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVuiSarInfo {
    pub uiSarWidth: u32,
    pub uiSarHeight: u32,
    pub bOverscanAppropriateFlag: bool,
}

pub type PVuiSarInfo = *mut SVuiSarInfo;

/// Bitrate info per layer (`SBitrateInfo`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SBitrateInfo {
    pub iLayer: LAYER_NUM,
    pub iBitrate: i32,
}

/// Logging trace callback prototype (`WelsTraceCallback`) — `codec_api.h:129`.
///
/// `ctx` is the caller's own opaque context, installed through
/// `ENCODER_OPTION_TRACE_CALLBACK_CONTEXT` or the decoder's equivalent and handed
/// back untouched. This crate never dereferences it.
pub type WelsTraceCallback =
    Option<unsafe extern "C" fn(ctx: *mut c_void, level: i32, string: *const c_char)>;

/// The caller's opaque trace handle. Never dereferenced here; `deliver` hands it
/// straight back to the caller untouched.
///
/// Stored as its address: `usize` is `Sync` and `Send`, and `repr(transparent)` over
/// a pointer-width integer keeps `SLogContext`'s byte image identical. The round trip
/// uses the strict-provenance pair, `expose_provenance` in and
/// `with_exposed_provenance_mut` out, because the pointer it rebuilds goes straight
/// to a C callback that will use it.
///
/// The sink is only reachable from inside the crate:
///
/// ```compile_fail,E0624
/// # unsafe extern "C" fn sink(_: *mut std::ffi::c_void, _: i32, _: *const std::ffi::c_char) {}
/// openh264_rs::api::codec_api::TraceUserCtx::default().deliver(sink, 1, c"x");
/// ```
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct TraceUserCtx(usize);

impl Default for TraceUserCtx {
    /// No handle installed.
    fn default() -> Self {
        Self(0)
    }
}

impl TraceUserCtx {
    /// Takes in whatever the caller installed through
    /// `ENCODER_OPTION_TRACE_CALLBACK_CONTEXT` or the decoder's equivalent.
    #[inline]
    pub(crate) fn from_abi(p: *mut c_void) -> Self {
        // `expose_provenance`, not `as usize`: the pointer `deliver` rebuilds must be
        // usable, not merely numerically equal.
        Self(p.expose_provenance())
    }

    /// Invokes the caller's sink.
    ///
    /// `pfLog` and the handle were installed together through `SetOption`; neither can
    /// carry a lifetime across the C ABI, so their validity is the caller's contract.
    ///
    /// Sound only because it is unreachable from safe code: this method, `from_abi`
    /// and `SLogContext`'s two callback fields are all `pub(crate)`, so the pair can
    /// only have arrived through the `unsafe` installers that took on that contract.
    /// Widening any one of those visibilities makes this unsound.
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn deliver(
        self,
        pfLog: unsafe extern "C" fn(ctx: *mut c_void, level: i32, string: *const c_char),
        level: i32,
        line: &std::ffi::CStr,
    ) {
        let ctx: *mut c_void = ptr::with_exposed_provenance_mut(self.0);
        unsafe { pfLog(ctx, level, line.as_ptr()) };
    }
}

/// The default trace sink — `welsCodecTrace.cpp`'s `welsStderrTrace`.
///
/// # Safety
/// `string` is the NUL-terminated buffer [`crate::common::wels_trace::WelsLog`] just
/// formatted; `ctx` is whatever was installed beside the callback and is not read here.
#[allow(unsafe_code)]
pub unsafe extern "C" fn welsStderrTrace(_ctx: *mut c_void, _level: i32, string: *const c_char) {
    if string.is_null() {
        return;
    }
    // Written through `std::io::stderr()` so it interleaves with the rest of this
    // process's stderr.
    let bytes = unsafe { std::ffi::CStr::from_ptr(string) }.to_bytes();
    use std::io::Write as _;
    let out = std::io::stderr();
    let mut lock = out.lock();
    let _ = lock.write_all(bytes);
    let _ = lock.write_all(b"\n");
}

// ============================================================================
// C/C++ Virtual Function Tables & Interface Definitions
// ============================================================================

/// C-compatible virtual function table for `ISVCEncoder`.
#[repr(C)]
pub struct ISVCEncoderVtbl {
    pub Initialize:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32,
    pub InitializeExt:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32,
    pub GetDefaultParams:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32,
    pub Uninitialize: unsafe extern "C" fn(pThis: *mut ISVCEncoder) -> i32,
    pub EncodeFrame: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32,
    pub EncodeParameterSets:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32,
    pub ForceIntraFrame: unsafe extern "C" fn(pThis: *mut ISVCEncoder, bIDR: bool) -> i32,
    /// `pOption`'s type is a function of `eOptionId` (`codec_api.h:245`). See
    /// [`encoder_set_opt_c`]'s contract.
    pub SetOption: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32,
    pub GetOption: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32,
}

/// Opaque H.264 / SVC Encoder class instance representation (`ISVCEncoder`).
#[repr(C)]
pub struct ISVCEncoder {
    pub lpVtbl: *const ISVCEncoderVtbl,
}

#[allow(unsafe_code)]
// # Safety — the contract every function below shares
//
// `this` must be a pointer the matching factory returned (`WelsCreateSVCEncoder` /
// `WelsCreateDecoder`), not yet passed to its destroyer, derived from the whole
// implementation allocation, non-null, and unaliased for the call. Every pointer
// argument carries the C header's own contract (`codec_api.h`), unchanged.
impl ISVCEncoder {
    /// Initializes the encoder with basic parameters.
    #[inline]
    pub unsafe fn Initialize(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
        unsafe { ((*(*this).lpVtbl).Initialize)(this, pParam) }
    }

    /// Initializes the encoder with extended SVC parameters.
    #[inline]
    pub unsafe fn InitializeExt(this: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32 {
        unsafe { ((*(*this).lpVtbl).InitializeExt)(this, pParam) }
    }

    /// Retrieves default extension encoding parameters.
    #[inline]
    pub unsafe fn GetDefaultParams(this: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32 {
        unsafe { ((*(*this).lpVtbl).GetDefaultParams)(this, pParam) }
    }

    /// Uninitializes and frees encoder session resources.
    #[inline]
    pub unsafe fn Uninitialize(this: *mut ISVCEncoder) -> i32 {
        unsafe { ((*(*this).lpVtbl).Uninitialize)(this) }
    }

    /// Encodes a single uncompressed frame.
    #[inline]
    pub unsafe fn EncodeFrame(
        this: *mut ISVCEncoder,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).EncodeFrame)(this, kpSrcPic, pBsInfo) }
    }

    /// Serializes out-of-band parameter sets (SPS/PPS).
    #[inline]
    pub unsafe fn EncodeParameterSets(this: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32 {
        unsafe { ((*(*this).lpVtbl).EncodeParameterSets)(this, pBsInfo) }
    }

    /// Forces the next frame to be encoded as an IDR keyframe.
    #[inline]
    pub unsafe fn ForceIntraFrame(this: *mut ISVCEncoder, bIDR: bool) -> i32 {
        unsafe { ((*(*this).lpVtbl).ForceIntraFrame)(this, bIDR) }
    }

    /// Sets runtime encoder option. `pOption` is raw — see the vtable slot.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime encoder option. `pOption` is raw.
    #[inline]
    pub unsafe fn GetOption(
        this: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).GetOption)(this, eOptionId, pOption) }
    }
}

/// C-compatible virtual function table for `ISVCDecoder`.
#[repr(C)]
pub struct ISVCDecoderVtbl {
    pub Initialize:
        unsafe extern "C" fn(pThis: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long,
    pub Uninitialize: unsafe extern "C" fn(pThis: *mut ISVCDecoder) -> c_long,
    pub DecodeFrame: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pStride: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
    ) -> DECODING_STATE,
    pub DecodeFrameNoDelay: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub DecodeFrame2: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub FlushFrame: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub DecodeParser: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDstInfo: *mut SParserBsInfo,
    ) -> DECODING_STATE,
    pub DecodeFrameEx: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDst: *mut u8,
        iDstStride: i32,
        iDstLen: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
        iColorFormat: *mut i32,
    ) -> DECODING_STATE,
    /// As `ISVCEncoderVtbl::SetOption` — `codec_api.h:518`'s slot.
    pub SetOption: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long,
    pub GetOption: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long,
}

/// Opaque H.264 / SVC Decoder class instance representation (`ISVCDecoder`).
#[repr(C)]
pub struct ISVCDecoder {
    pub lpVtbl: *const ISVCDecoderVtbl,
}

#[allow(unsafe_code)]
impl ISVCDecoder {
    // The `# Safety` contract: the note above `impl ISVCEncoder`.

    /// Initializes the decoder context.
    #[inline]
    pub unsafe fn Initialize(this: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long {
        unsafe { ((*(*this).lpVtbl).Initialize)(this, pParam) }
    }

    /// Uninitializes the decoder context.
    #[inline]
    pub unsafe fn Uninitialize(this: *mut ISVCDecoder) -> c_long {
        unsafe { ((*(*this).lpVtbl).Uninitialize)(this) }
    }

    /// Decodes a single frame.
    #[inline]
    pub unsafe fn DecodeFrame(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pStride: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            ((*(*this).lpVtbl).DecodeFrame)(this, pSrc, iSrcLen, ppDst, pStride, iWidth, iHeight)
        }
    }

    /// Zero-latency frame decoding (recommended real-time decoder API).
    #[inline]
    pub unsafe fn DecodeFrameNoDelay(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeFrameNoDelay)(this, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Multi-slice frame assembly decoding entry point.
    #[inline]
    pub unsafe fn DecodeFrame2(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeFrame2)(this, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Flushes remaining decoded reference frames from the DPB.
    #[inline]
    pub unsafe fn FlushFrame(
        this: *mut ISVCDecoder,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).FlushFrame)(this, ppDst, pDstInfo) }
    }

    /// Parses input bitstream headers only without pixel reconstruction.
    #[inline]
    pub unsafe fn DecodeParser(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDstInfo: *mut SParserBsInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeParser)(this, pSrc, iSrcLen, pDstInfo) }
    }

    /// Decodes to arbitrary destination format buffer.
    #[inline]
    pub unsafe fn DecodeFrameEx(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDst: *mut u8,
        iDstStride: i32,
        iDstLen: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
        iColorFormat: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            ((*(*this).lpVtbl).DecodeFrameEx)(
                this,
                pSrc,
                iSrcLen,
                pDst,
                iDstStride,
                iDstLen,
                iWidth,
                iHeight,
                iColorFormat,
            )
        }
    }

    /// Sets runtime decoder option. `pOption` is raw — see the vtable slot.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime decoder option. `pOption` is raw.
    #[inline]
    pub unsafe fn GetOption(
        this: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long {
        unsafe { ((*(*this).lpVtbl).GetOption)(this, eOptionId, pOption) }
    }
}

// ============================================================================
// Global Dynamic Library Export Lifecycle Bindings
// ============================================================================

// ===========================================================================
// The safe cores.
//
// `CWelsDecoderImpl` and `CWelsH264SVCEncoderImpl` are C-ABI shells: a vtable
// pointer at offset zero, the vtable it points at, and the thing that does the work.
//
// `Decoder` and `Encoder` are the same objects with the C ABI removed: they own
// their contexts, their methods are safe, and their arguments are references and
// slices. The shells hold one each.
//
// They are newtypes and not re-exports, so that the members shaped by the C ABI —
// `SetOption`'s type-erased blob above all — do not become part of the safe surface
// by accident.
// ===========================================================================

/// The H.264 encoder, as a Rust type.
///
/// Wraps `CWelsH264SVCEncoder`. Every method is safe except the two option calls,
/// which say so.
pub struct Encoder(pub(crate) crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder);

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unsafe_code)]
impl Encoder {
    pub fn new() -> Self {
        Self(crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder::new())
    }

    /// Returns `cmResultSuccess` or one of `codec_def.h`'s `CM_*` codes.
    pub fn initialize(&mut self, param: &SEncParamBase) -> i32 {
        self.0.Initialize(Some(param))
    }

    pub fn initialize_ext(&mut self, param: &SEncParamExt) -> i32 {
        self.0.InitializeExt(Some(param))
    }

    /// Fills `param` with the encoder's defaults. Every field is written.
    pub fn default_params(&mut self, param: &mut SEncParamExt) -> i32 {
        self.0.GetDefaultParams(param)
    }

    pub fn uninitialize(&mut self) -> i32 {
        self.0.Uninitialize()
    }

    /// Encodes one frame.
    ///
    /// `src`'s `pData` planes are the caller's and are read during this call only.
    /// On success `bs`'s layer buffers name memory owned by this encoder, valid
    /// until the next call on it — which is why this returns pointers, not slices.
    pub fn encode_frame(&mut self, src: &SSourcePicture, bs: &mut SFrameBSInfo) -> i32 {
        self.0.EncodeFrame(src, bs)
    }

    /// Emits SPS/PPS into `bs`, with the same output window as [`Self::encode_frame`].
    pub fn encode_parameter_sets(&mut self, bs: &mut SFrameBSInfo) -> i32 {
        self.0.EncodeParameterSets(bs)
    }

    pub fn force_intra_frame(&mut self, idr: bool) -> i32 {
        self.0.ForceIntraFrame(idr, -1)
    }

    /// The trace destination, as three typed setters rather than an option blob.
    pub fn set_trace_level(&mut self, level: u32) {
        self.0.m_pWelsTrace.SetTraceLevel(level);
        self.0.sync_log_ctx();
    }

    /// # Safety
    ///
    /// `callback` is entered on every delivered message until it is replaced or
    /// this encoder is dropped, with the context installed beside it by
    /// [`Self::set_trace_callback_context`]; it must be sound to enter for that
    /// whole window. Neither half can carry a lifetime across the C ABI.
    ///
    /// ```compile_fail,E0133
    /// # unsafe extern "C" fn sink(_: *mut std::ffi::c_void, _: i32, _: *const std::ffi::c_char) {}
    /// let mut e = openh264_rs::api::codec_api::Encoder::new();
    /// e.set_trace_callback(Some(sink));
    /// ```
    pub unsafe fn set_trace_callback(&mut self, callback: WelsTraceCallback) {
        self.0.m_pWelsTrace.SetTraceCallback(callback);
        self.0.sync_log_ctx();
    }

    /// # Safety
    ///
    /// `ctx` is handed back to the trace callback on every message until it is
    /// replaced or this encoder is dropped, so it must stay valid for that long.
    /// It is the caller's, and this crate never dereferences it.
    pub unsafe fn set_trace_callback_context(&mut self, ctx: *mut c_void) {
        self.0
            .m_pWelsTrace
            .SetTraceCallbackContext(TraceUserCtx::from_abi(ctx));
        self.0.sync_log_ctx();
    }

    /// `ENCODER_OPTION_*`, the type-erased pair.
    ///
    /// # Safety
    ///
    /// `option` must point at a readable, aligned object of the type `id` names,
    /// for the duration of the call — see [`encoder_set_opt_c`]'s contract.
    pub unsafe fn set_option_raw(&mut self, id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        unsafe { self.0.SetOption(id, option) }
    }

    /// # Safety
    ///
    /// As [`Self::set_option_raw`], with `option` written.
    pub unsafe fn get_option_raw(&mut self, id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        unsafe { self.0.GetOption(id, option) }
    }
}

#[repr(C)]
pub struct CWelsH264SVCEncoderImpl {
    pub base: ISVCEncoder,
    pub pVtbl: Box<ISVCEncoderVtbl>,
    pub inner: Encoder,
}

/// The H.264 decoder, as a Rust type.
///
/// Owns the decoder context and the trace object; every method below is safe and
/// takes references and slices.
///
/// The one thing that cannot be a Rust type is the output window: a decoded frame
/// is handed back as three plane pointers into this decoder's own picture buffer,
/// valid until the next call on it. That is `codec_api.h`'s contract and the
/// methods that return it say so.
pub struct Decoder {
    /// The allocation root for the decoder context; every teardown site is `take()`.
    /// One context per decoder, so the context owns the reordering buffer, the
    /// statistics block and the vlc table.
    pub(crate) ctx: Option<Box<crate::decoder::decoder_core::SWelsDecoderContext>>,
    /// The trace object the context's own `sLogCtx` names.
    pub(crate) trace: Box<crate::common::wels_trace::welsCodecTrace>,
    /// This object's own record of `DECODER_OPTION_END_OF_STREAM`, which `GetOption`
    /// reads back.
    pub(crate) end_of_stream: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
pub struct CWelsDecoderImpl {
    pub base: ISVCDecoder,
    pub pVtbl: Box<ISVCDecoderVtbl>,
    /// The safe core this C-ABI shell wraps.
    pub core: Decoder,
}

// ===========================================================================
// No panic crosses the ABI.
//
// A `panic!` inside an `extern "C" fn` is not an unwind a C caller can ignore: since
// Rust 1.81 the runtime aborts the process. Every entry point below therefore catches.
//
// The guard rests on `panic = "unwind"`, Cargo's default in every profile this crate
// builds under. A consumer who rebuilds it with `panic = "abort"` gets no window here.
//
// A caught panic is reported at `WELS_LOG_ERROR` so it is visible, and the code it
// maps to is the slot's own failure code so a caller's error handling works.
//
// `AssertUnwindSafe` is asserted, not proved. The impl object is `&mut`-reachable
// across the window, so a panic can leave a codec context half-updated. The claim is
// narrower than `UnwindSafe`'s: after a caught panic the object is memory-safe to drop
// and to call again, and the call that panicked reports failure. The codec's state is
// not claimed to be coherent — a consumer that gets one of these codes should destroy
// the object.
// ===========================================================================

#[allow(unsafe_code)]
/// The trace settings to report a caught panic through, read from the impl object
/// before the guarded body runs: afterwards the object is in whatever state the unwind
/// left it.
///
/// # Safety
///
/// `this` is either null or a pointer to a live `CWelsDecoderImpl` — the same
/// contract every decoder slot states for its own `this`.
unsafe fn decoder_log(this: *mut ISVCDecoder) -> Option<crate::common::wels_trace::SLogContext> {
    if this.is_null() {
        return None;
    }
    unsafe { Some((*(this as *mut CWelsDecoderImpl)).core.trace.log_context()) }
}

#[allow(unsafe_code)]
/// [`decoder_log`] for the encoder side.
///
/// # Safety
///
/// `this` is either null or a pointer to a live `CWelsH264SVCEncoderImpl`.
unsafe fn encoder_log(this: *mut ISVCEncoder) -> Option<crate::common::wels_trace::SLogContext> {
    if this.is_null() {
        return None;
    }
    unsafe {
        Some(
            (*(this as *mut CWelsH264SVCEncoderImpl))
                .inner
                .0
                .m_pWelsTrace
                .log_context(),
        )
    }
}

/// Reports a caught panic through the trace at `WELS_LOG_ERROR`.
///
/// The message is included when the payload is `&'static str` or `String`; any other
/// payload is named as such.
fn report_abi_panic(
    slot: &str,
    payload: Box<dyn std::any::Any + Send>,
    log: Option<crate::common::wels_trace::SLogContext>,
) {
    let what = payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>");
    // `WelsLog` does nothing when there is no sink, which is the whole of the `None`
    // case.
    crate::common::wels_trace::WelsLog(
        log.unwrap_or_default(),
        crate::common::wels_trace::WELS_LOG_ERROR,
        &format!(
            "{slot}: a panic was caught at the C-ABI boundary and reported as a failure code instead of aborting the process. Panic message: {what}"
        ),
    );
}

/// One `catch_unwind` window per `extern "C"` entry point.
///
/// `$slot` names the entry for the log, `$log` is its [`decoder_log`]/[`encoder_log`]
/// read, `$fail` is the code this slot returns when it cannot do its job, and `$body`
/// is the slot's own body.
///
/// Every `return` inside `$body` returns from the closure, so it is the value of the
/// whole expression.
macro_rules! abi_guard {
    ($slot:literal, $log:expr, $fail:expr, $body:block) => {{
        let __log = $log;
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(move || $body)) {
            Ok(v) => v,
            Err(payload) => {
                report_abi_panic($slot, payload, __log);
                $fail
            }
        }
    }};
}

#[cfg(test)]
thread_local! {
    /// The guard's covering-test hook, compiled only under `cfg(test)`.
    ///
    /// Thread-local rather than global: other unit tests drive `DecodeFrame2` and
    /// `EncodeFrame` in parallel in one process, and a global switch would fire in
    /// whichever test happened to be inside a thunk at the time.
    pub(crate) static PANIC_PROBE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `PANIC_PROBE` values. `0` is off.
#[cfg(test)]
pub(crate) const PROBE_DECODE_FRAME2: u32 = 1;
#[cfg(test)]
pub(crate) const PROBE_ENCODE_FRAME: u32 = 2;

/// Panics if this thread armed `PANIC_PROBE` for `$which`. Expands to nothing
/// outside `cfg(test)`.
macro_rules! panic_probe {
    ($which:expr) => {
        #[cfg(test)]
        if PANIC_PROBE.with(|p| p.get()) == $which {
            panic!("P13 covering test: a deliberate panic inside the guarded body");
        }
    };
}

// ===========================================================================
// The encoder's nine vtable slots.
//
// `codec_api.h` hands a C caller a vtable of nine `extern "C"` functions over an
// opaque `ISVCEncoder*`. Everything that arrives here is a raw pointer with a
// validity window the caller guarantees, and the window is not the same for every
// slot: `pParam` is read for the duration of one call, `pBsInfo` is written during
// one call and read by the caller until the next one, and `pOption`'s size is a
// function of the option id. Each slot below states its own.
//
// `this` is cast to the whole impl allocation and never borrowed as `ISVCEncoder`,
// which is one pointer wide.
// ===========================================================================

#[allow(unsafe_code)]
/// `ISVCEncoder::Initialize` — `codec_api.h:196`.
///
/// # Safety
///
/// * `this` is either null or a pointer to a live `CWelsH264SVCEncoderImpl`
///   produced by [`WelsCreateSVCEncoder`] and not yet destroyed. It is cast to the
///   whole impl allocation, never borrowed as the one-pointer-wide `ISVCEncoder`.
/// * `pParam` is either null or a readable, aligned `SEncParamBase` for the duration
///   of this call. Nothing in the encoder retains it: `Initialize` transcodes it into
///   its own `SWelsSvcCodingParam` before returning.
unsafe extern "C" fn encoder_init_c(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
    abi_guard!(
        "ISVCEncoder::Initialize",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            // The impl reports a null `pParam`; only `this` has to be checked before
            // the cast.
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.0.Initialize(pParam.as_ref())
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::InitializeExt` — `codec_api.h:203`.
///
/// # Safety
///
/// As [`encoder_init_c`], with `SEncParamExt` in place of `SEncParamBase`.
unsafe extern "C" fn encoder_init_ext_c(
    this: *mut ISVCEncoder,
    pParam: *const SEncParamExt,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::InitializeExt",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.0.InitializeExt(pParam.as_ref())
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::GetDefaultParams` — `codec_api.h:210`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pParam` must be null or a writable, aligned `SEncParamExt` for the duration
///   of this call. It is an out parameter: every field is overwritten and none is
///   read first, so its prior contents may be anything, including uninitialised.
unsafe extern "C" fn encoder_get_default_c(
    this: *mut ISVCEncoder,
    pParam: *mut SEncParamExt,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::GetDefaultParams",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let Some(pParam) = pParam.as_mut() else {
                    return CM_INIT_PARA_ERROR;
                };
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.default_params(pParam)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::Uninitialize` — `codec_api.h:216`.
///
/// # Safety
///
/// `this` as in [`encoder_init_c`]. Nothing else crosses.
unsafe extern "C" fn encoder_uninit_c(this: *mut ISVCEncoder) -> i32 {
    abi_guard!(
        "ISVCEncoder::Uninitialize",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.uninitialize()
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::EncodeFrame` — `codec_api.h:224`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `kpSrcPic` must be null or a readable, aligned `SSourcePicture` for the
///   duration of this call. Its `pData[0..3]` stay raw: they are the caller's plane
///   pointers, each readable for `iStride[i] * height` bytes in the caller's own
///   layout, for this call only. The encoder copies what it needs before returning
///   and retains none of them.
/// * `pBsInfo` must be null or a writable, aligned `SFrameBSInfo` for the duration
///   of this call. On success its `sLayerInfo[].pBsBuf` pointers name memory owned
///   by the encoder, valid until the next call on this encoder — which is why this
///   cannot be a `&mut [u8]`.
unsafe extern "C" fn encoder_encode_frame_c(
    this: *mut ISVCEncoder,
    kpSrcPic: *const SSourcePicture,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::EncodeFrame",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            panic_probe!(PROBE_ENCODE_FRAME);
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let (Some(kpSrcPic), Some(pBsInfo)) = (kpSrcPic.as_ref(), pBsInfo.as_mut()) else {
                    return CM_INIT_PARA_ERROR;
                };
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.encode_frame(kpSrcPic, pBsInfo)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::EncodeParameterSets` — `codec_api.h:231`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pBsInfo` as in [`encoder_encode_frame_c`], including the output window: the
///   SPS/PPS bytes it names are the encoder's, valid until the next call.
unsafe extern "C" fn encoder_encode_param_c(
    this: *mut ISVCEncoder,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::EncodeParameterSets",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let Some(pBsInfo) = pBsInfo.as_mut() else {
                    return CM_INIT_PARA_ERROR;
                };
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.encode_parameter_sets(pBsInfo)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::ForceIntraFrame` — `codec_api.h:238`.
///
/// # Safety
///
/// `this` as in [`encoder_init_c`]. `bIDR` must hold 0 or 1, as the C++ ABI requires
/// of `bool`.
unsafe extern "C" fn encoder_force_intra_c(this: *mut ISVCEncoder, bIDR: bool) -> i32 {
    abi_guard!(
        "ISVCEncoder::ForceIntraFrame",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.force_intra_frame(bIDR)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::SetOption` — `codec_api.h:245`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pOption` stays raw, because its type is a function of `eOptionId` and of
///   nothing else: `ENCODER_OPTION_TRACE_LEVEL` reads a `u32` through it,
///   `ENCODER_OPTION_TRACE_CALLBACK` a `WelsTraceCallback`,
///   `ENCODER_OPTION_SVC_ENCODE_PARAM_EXT` an `SEncParamExt`, and so on for
///   thirty-two ids. The caller must point it at a readable, aligned object of the
///   type that id names, for the duration of this call.
unsafe extern "C" fn encoder_set_opt_c(
    this: *mut ISVCEncoder,
    eOptionId: ENCODER_OPTION,
    pOption: *mut c_void,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::SetOption",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.set_option_raw(eOptionId, pOption)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::GetOption` — `codec_api.h:252`.
///
/// # Safety
///
/// As [`encoder_set_opt_c`], with the blob written rather than read: the caller must
/// point `pOption` at a writable, aligned object of the type `eOptionId` names, for
/// the duration of this call.
unsafe extern "C" fn encoder_get_opt_c(
    this: *mut ISVCEncoder,
    eOptionId: ENCODER_OPTION,
    pOption: *mut c_void,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::GetOption",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            unsafe {
                let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
                (*impl_ptr).inner.get_option_raw(eOptionId, pOption)
            }
        }
    )
}

/// `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)`
/// — `decoder.cpp:654` and `welsDecoderExt.cpp:528`, one function.
///
/// The clamp runs on the wire integer, which is an `int` there and an eight-variant
/// enum here.
fn ec_idc_from_raw(raw: i32) -> ERROR_CON_IDC {
    match raw.clamp(
        ERROR_CON_IDC::ERROR_CON_DISABLE as i32,
        ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE as i32,
    ) {
        0 => ERROR_CON_IDC::ERROR_CON_DISABLE,
        1 => ERROR_CON_IDC::ERROR_CON_FRAME_COPY,
        2 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY,
        3 => ERROR_CON_IDC::ERROR_CON_FRAME_COPY_CROSS_IDR,
        4 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR,
        5 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
        6 => ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR,
        _ => ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
    }
}

/// `VIDEO_BITSTREAM_SVC`/`VIDEO_BITSTREAM_AVC` pass; anything else is
/// `VIDEO_BITSTREAM_DEFAULT` — `decoder.cpp:667–671`'s `else`.
fn video_bs_type_from_raw(raw: i32) -> VIDEO_BITSTREAM_TYPE {
    match raw {
        0 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
        1 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC,
        _ => VIDEO_BITSTREAM_DEFAULT,
    }
}

#[allow(unsafe_code)]
impl Decoder {
    /// `CWelsDecoder`'s constructor — `welsDecoderExt.cpp:155`.
    pub fn new() -> Self {
        Self {
            ctx: None,
            trace: Box::new(crate::common::wels_trace::welsCodecTrace::new()),
            end_of_stream: false,
        }
    }

    /// `CWelsDecoder::Initialize` — `welsDecoderExt.cpp:260`.
    ///
    /// Returns `cmResultSuccess` or a `CM_*` code. The caller's block has already
    /// been sanitised by the time it gets here: the range clamp and the bitstream
    /// type's normalisation run on the wire values, at the thunk.
    pub fn initialize(&mut self, pParam: &SDecodingParam) -> c_long {
        // A second `Initialize` on a live decoder rebuilds: the previous session's
        // reordering buffer, statistics, last decoded-picture record and decode
        // timestamps do not survive the teardown.
        if let Some(mut pCtx) = self.ctx.take() {
            crate::decoder::decoder_core::WelsEndDecoder(&mut pCtx);
        }
        {
            // In-place heap construction: the context is several MiB and owns `Vec`s,
            // so neither `Box::default()` (stack round-trip) nor
            // `new_zeroed().assume_init()` (invalid zeroed `Vec`) is usable.
            let mut ctx_box = crate::decoder::decoder_context::SWelsDecoderContext::new_boxed();
            // The caller's parameters go in before `WelsDecoderDefaults`, because
            // everything built below this line may read them.
            ctx_box.pParam = *pParam;
            // These defaults are not zeros: `iPrevFrameNum` starts at -1.
            WelsDecoderLastDecPicInfoDefaults(&mut ctx_box.pLastDecPicInfo);
            // A fresh reordering buffer. `IMinInt32` in every slot's `iPOC` is what
            // "empty" is; zeroes are a valid POC.
            let crate::decoder::decoder_core::SWelsDecoderContext {
                pPictReoderingStatus,
                pPictInfoList,
                ..
            } = &mut *ctx_box;
            crate::decoder::decoder_core::ResetReorderingPictureBuffers(
                pPictReoderingStatus,
                pPictInfoList,
                true,
            );
            let log_ctx = self.trace.log_context();
            crate::decoder::decoder_core::WelsDecoderDefaults(&mut ctx_box, Some(&log_ctx));
            WelsDecoderSpsPpsDefaults(&mut ctx_box.sSpsPpsCtx);
            if WelsInitStaticMemory(&mut ctx_box) != 0 {
                // The failure path is the `Box` going out of scope.
                return CM_INIT_PARA_ERROR as c_long;
            }
            self.ctx = Some(ctx_box);
        }

        // `DecoderConfigParam` runs on every `Initialize`, and its `InitErrorCon` does
        // two things nothing else does:
        //
        //  * clears `bFreezeOutput`, which `WelsDecoderDefaults` sets true. The only
        //    other site that clears it is the "complete non-ECed IDR" arm of
        //    `DecodeFrameConstruction`, so without it a stream whose first IDR is
        //    missing or damaged stays frozen and emits nothing until a clean IDR
        //    arrives.
        //  * installs `sCopyFunc`'s two kernels. `DoErrorConSliceCopy` and
        //    `DoErrorConSliceMVCopy` guard every copy with `if let Some(f)`, so with
        //    the table `None` the slice-copy concealment runs and copies nothing.
        if let Some(pCtx) = self.ctx.as_mut() {
            crate::decoder::decoder_core::DecoderConfigParam(pCtx, pParam);
        }

        CM_RESULT_SUCCESS as c_long
    }

    /// `CWelsDecoder::Uninitialize` — `welsDecoderExt.cpp:279`.
    pub fn uninitialize(&mut self) -> c_long {
        if let Some(mut pCtx) = self.ctx.take() {
            crate::decoder::decoder_core::WelsEndDecoder(&mut pCtx);
        }
        CM_RESULT_SUCCESS as c_long
    }

    /// The number of pictures the display-reordering buffer is holding —
    /// `DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER`.
    pub fn frames_remaining(&self) -> i32 {
        self.ctx
            .as_ref()
            .map_or(0, |pCtx| pCtx.pPictReoderingStatus.iNumOfPicts)
    }

    /// `DECODER_OPTION_END_OF_STREAM`, both ways.
    pub fn end_of_stream(&self) -> bool {
        self.end_of_stream
    }

    pub fn set_end_of_stream(&mut self, eos: bool) {
        self.end_of_stream = eos;
        if let Some(pCtx) = self.ctx.as_mut() {
            pCtx.bEndOfStreamFlag = eos;
        }
    }

    /// The concealment mode the decoder is actually running with — the context's
    /// own `pParam`, which is the block `SetOption` writes.
    pub fn error_concealment(&self) -> Option<ERROR_CON_IDC> {
        self.ctx.as_ref().map(|pCtx| pCtx.pParam.eEcActiveIdc)
    }

    /// `welsDecoderExt.cpp:521–539`: clamp the caller's `int`, refuse it outright
    /// when parse-only is on, then store it and re-run `InitErrorCon` — the mode
    /// selects which kernels `sCopyFunc` holds.
    pub fn set_error_concealment(&mut self, raw: i32) -> c_long {
        let val = ec_idc_from_raw(raw);
        let Some(pCtx) = self.ctx.as_mut() else {
            return DECODING_STATE::dsInitialOptExpected.0 as c_long;
        };
        if pCtx.pParam.bParseOnly && val != ERROR_CON_IDC::ERROR_CON_DISABLE {
            return CM_INIT_PARA_ERROR as c_long;
        }
        pCtx.pParam.eEcActiveIdc = val;
        crate::decoder::error_concealment::InitErrorCon(pCtx);
        CM_RESULT_SUCCESS as c_long
    }

    /// `DECODER_OPTION_GET_STATISTICS` — `welsDecoderExt.cpp:639`. The two speed
    /// fields are computed at read time, as there.
    pub fn statistics(&self) -> Option<SDecoderStatistics> {
        let pCtx = self.ctx.as_ref()?;
        let mut out = pCtx.pDecoderStatistics;
        if out.uiDecodedFrameCount != 0 {
            out.fAverageFrameSpeedInMs =
                (pCtx.dDecTime / f64::from(out.uiDecodedFrameCount)) as f32;
            out.fActualAverageFrameSpeedInMs = (pCtx.dDecTime
                / f64::from(
                    out.uiDecodedFrameCount
                        .wrapping_add(out.uiFreezingIDRNum)
                        .wrapping_add(out.uiFreezingNonIDRNum),
                )) as f32;
        }
        Some(out)
    }

    /// The trace destination, as typed setters rather than an option blob. Each
    /// pushes the settings into the context's copy of the log context.
    pub fn set_trace_level(&mut self, level: u32) {
        self.trace.SetTraceLevel(level);
        self.sync_log_ctx();
    }

    /// # Safety
    ///
    /// As [`Encoder::set_trace_callback`], for this decoder's lifetime.
    pub unsafe fn set_trace_callback(&mut self, callback: WelsTraceCallback) {
        self.trace.SetTraceCallback(callback);
        self.sync_log_ctx();
        crate::common::wels_trace::WelsLog(
            self.trace.m_sLogCtx,
            crate::common::wels_trace::WELS_LOG_INFO,
            "CWelsDecoder::SetOption():DECODER_OPTION_TRACE_CALLBACK callback set.",
        );
    }

    // -----------------------------------------------------------------------
    // `CWelsDecoder::GetOption`'s 16 arms — `welsDecoderExt.cpp:584-695`.
    //
    // The option-id dispatch, the pointer types and the error codes stay in the
    // thunk, which is where the C interface's rules live.
    // -----------------------------------------------------------------------

    /// Whether the context exists yet. `GetOption` answers `cmInitExpected` for
    /// every id but `NUM_OF_THREADS` when it does not (`welsDecoderExt.cpp:589`).
    pub fn has_ctx(&self) -> bool {
        self.ctx.is_some()
    }

    /// `DECODER_OPTION_VCL_NAL` — `:621-624`.
    pub fn feedback_vcl_nal(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFeedbackVclNalInAu)
    }

    /// `DECODER_OPTION_TEMPORAL_ID` — `:625-628`.
    pub fn feedback_temporal_id(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFeedbackTidInAu)
    }

    /// `DECODER_OPTION_IS_REF_PIC` — `:629-634`. The stored `nal_ref_idc` is clamped
    /// to 0/1 on the way out, but −1 is left alone.
    pub fn feedback_is_ref_pic(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| {
            let iVal = pCtx.iFeedbackNalRefIdc;
            if iVal > 0 { 1 } else { iVal }
        })
    }

    /// `DECODER_OPTION_FRAME_NUM` — `:608-611`, under `LONG_TERM_REF`.
    pub fn frame_num(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFrameNum)
    }

    /// `DECODER_OPTION_IDR_PIC_ID` — `:603-607`, under `LONG_TERM_REF`. The field is
    /// a `uint16_t` and crosses as the `int` the option id names.
    pub fn cur_idr_pic_id(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| i32::from(pCtx.uiCurIdrPicId))
    }

    /// `DECODER_OPTION_LTR_MARKING_FLAG` — `:612-615`.
    pub fn ltr_marking_flag(&self) -> Option<i32> {
        self.ctx
            .as_ref()
            .map(|pCtx| i32::from(pCtx.bCurAuContainLtrMarkSeFlag))
    }

    /// `DECODER_OPTION_LTR_MARKED_FRAME_NUM` — `:616-619`.
    pub fn ltr_marked_frame_num(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFrameNumOfAuMarkedLtr)
    }

    /// `DECODER_OPTION_PROFILE` / `DECODER_OPTION_LEVEL` — `:673-687`. Both return
    /// `cmInitExpected` when no SPS has been activated yet, which the outer `Option`
    /// carries; the inner one is the context.
    pub fn active_sps_profile(&self) -> Option<Option<i32>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
                .map(|sps| i32::from(sps.uiProfileIdc))
        })
    }

    pub fn active_sps_level(&self) -> Option<Option<i32>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
                .map(|sps| i32::from(sps.uiLevelIdc))
        })
    }

    /// `DECODER_OPTION_GET_SAR_INFO` — `:664-672`. The caller's struct is zeroed and
    /// then filled from the active SPS's VUI, so a stream whose VUI carries no aspect
    /// ratio reads back zeros rather than the previous stream's.
    pub fn sar_info(&self) -> Option<Option<SVuiSarInfo>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).map(
                |sps| SVuiSarInfo {
                    uiSarWidth: sps.sVui.uiSarWidth,
                    uiSarHeight: sps.sVui.uiSarHeight,
                    bOverscanAppropriateFlag: sps.sVui.bOverscanAppropriateFlag,
                },
            )
        })
    }

    /// `DECODER_OPTION_STATISTICS_LOG_INTERVAL`, both ways — `:653-659` and
    /// `:571-577`.
    pub fn statistics_log_interval(&self) -> Option<u32> {
        self.ctx
            .as_ref()
            .map(|pCtx| pCtx.pDecoderStatistics.iStatisticsLogInterval)
    }

    pub fn set_statistics_log_interval(&mut self, interval: u32) -> bool {
        match self.ctx.as_mut() {
            Some(pCtx) => {
                pCtx.pDecoderStatistics.iStatisticsLogInterval = interval;
                true
            }
            None => false,
        }
    }

    /// `CWelsDecoder::Initialize`'s null-parameter arm — `welsDecoderExt.cpp:266-268`.
    ///
    /// On the impl because the message needs the trace object; the thunk only has to
    /// notice the null.
    pub(crate) fn report_init_null_param(&mut self) -> c_long {
        crate::common::wels_trace::WelsLog(
            self.trace.m_sLogCtx,
            crate::common::wels_trace::WELS_LOG_ERROR,
            "CWelsDecoder::Initialize(), invalid input argument.",
        );
        CM_INIT_PARA_ERROR as c_long
    }

    /// # Safety
    ///
    /// `ctx` is handed back to the trace callback on every message until it is
    /// replaced or this decoder is dropped, so it must stay valid for that long.
    /// It is the caller's, and this crate never dereferences it.
    pub unsafe fn set_trace_callback_context(&mut self, ctx: *mut c_void) {
        self.trace
            .SetTraceCallbackContext(TraceUserCtx::from_abi(ctx));
        self.sync_log_ctx();
    }

    /// `CWelsDecoder::DecodeFrame2WithCtx` — `welsDecoderExt.cpp:735`.
    ///
    /// `src` is one access unit, or `None` for the end-of-stream flush that
    /// `(NULL, 0)` means on the C interface.
    ///
    /// The output window: when `pDstInfo.iBufferStatus == 1`, `ppDst[0..3]` and
    /// `pDstInfo.pDst[0..3]` name planes inside this decoder's picture buffer with
    /// the strides `pDstInfo.UsrData` reports. They stay valid until the next call on
    /// this decoder and are not the caller's to free, which is why they are pointers
    /// here and not slices.
    pub fn decode(
        &mut self,
        src: Option<&[u8]>,
        ppDst: &mut [*mut u8; 3],
        pDstInfo: &mut SBufferInfo,
    ) -> DECODING_STATE {
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            return DECODING_STATE::dsInitialOptExpected;
        };
        p_ctx.iErrorCode = DECODING_STATE::dsErrorFree.0;
        // `dDecTime` is a millisecond accumulator; its one reader is
        // `DECODER_OPTION_GET_STATISTICS`'s two speed fields.
        let dec_started = std::time::Instant::now();

        // ------------------------------------------------------------------
        // The per-call reset block — `welsDecoderExt.cpp:784-811`. Every field here is
        // read back through `GetOption`.
        ppDst[0] = ptr::null_mut();
        ppDst[1] = ptr::null_mut();
        ppDst[2] = ptr::null_mut();
        p_ctx.iFeedbackVclNalInAu = crate::decoder::decoder_core::FEEDBACK_UNKNOWN_NAL;
        // `:789-793`: the whole `SBufferInfo` is zeroed and only `uiInBsTimeStamp`
        // survives, being the caller's input on this slot.
        let uiInBsTimeStamp = pDstInfo.uiInBsTimeStamp;
        *pDstInfo = SBufferInfo::default();
        pDstInfo.uiInBsTimeStamp = uiInBsTimeStamp;
        // `:795-800`, under `LONG_TERM_REF`.
        p_ctx.bReferenceLostAtT0Flag = false;
        p_ctx.bCurAuContainLtrMarkSeFlag = false;
        p_ctx.iFrameNumOfAuMarkedLtr = 0;
        p_ctx.iFrameNum = -1;
        // `:804-805`.
        p_ctx.iFeedbackTidInAu = -1;
        p_ctx.iFeedbackNalRefIdc = -1;
        // `:807-811`. `pDstInfo` is a reference here: `decoder_decode_frame2_c` has
        // already returned `dsInitialOptExpected` for a null.
        pDstInfo.uiOutYuvTimeStamp = 0;
        p_ctx.uiTimeStamp = uiInBsTimeStamp;

        if let Some(src) = src {
            p_ctx.bEndOfStreamFlag = false;
            if crate::decoder::decoder_core::GetThreadCount(&*p_ctx) <= 0 {
                p_ctx.uiDecodeTimeStamp += 1;
                p_ctx.uiDecodingTimeStamp = p_ctx.uiDecodeTimeStamp;
            }
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                src,
                src.len() as i32,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        } else {
            // `WelsDecodeBs` runs on both arms, so `DecodeFrame2 (NULL, 0, …)` always
            // reconstructs and this arm is not gated on end of stream:
            // `DecodeFrameNoDelay`'s second call is a null call made before it.
            p_ctx.bEndOfStreamFlag = true;
            // `DecodeFrameConstruction` reads this flag: with it set, an incomplete
            // frame returns `ERR_INFO_MB_NUM_INADEQUATE` instead of falling through to
            // the output path and emitting a partial frame.
            p_ctx.bInstantDecFlag = true;
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                &[],
                0,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        }
        p_ctx.bInstantDecFlag = false; // reset no-delay flag

        // ------------------------------------------------------------------
        // The error-reporting block — `welsDecoderExt.cpp:815–891`.
        // ------------------------------------------------------------------
        if p_ctx.iErrorCode != 0 {
            let eNalType = p_ctx.sCurNalHead.eNalUnitType;

            // `:820–831` — the two reset arms, which differ only in the code they
            // report. `ResetDecoder` (`:439`) saves the parameter block and rebuilds
            // the context over it; the rebuilt buffers are already at their
            // constructor defaults.
            let reset_code = if p_ctx.iErrorCode & crate::decoder::decoder_core::dsOutOfMemory != 0
            {
                Some(DECODING_STATE::dsOutOfMemory)
            } else if p_ctx.iErrorCode & crate::decoder::decoder_core::dsRefListNullPtrs != 0 {
                Some(DECODING_STATE::dsRefListNullPtrs)
            } else {
                None
            };
            if let Some(code) = reset_code {
                let sPrevParam = p_ctx.pParam;
                crate::decoder::decoder_core::WelsLog(
                    p_ctx.sLogCtx,
                    crate::decoder::decoder_core::WELS_LOG_INFO,
                    &format!("ResetDecoder(), context error code is {}", p_ctx.iErrorCode),
                );
                let _ = self.initialize(&sPrevParam);
                pDstInfo.iBufferStatus = 0;
                return code;
            }

            // `:833–842` — on an AVC bitstream, or on an error in a parameter set or
            // IDR NAL, the upper layer is notified of key frame loss. This is
            // `eVideoType`'s one reader, and `bParamSetsLostFlag` is what
            // `UpdateAccessUnit`'s mosaic-avoidance block reads.
            if IS_PARAM_SETS_NALS(eNalType)
                || eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
                || p_ctx.eVideoType == VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC
            {
                if p_ctx.pParam.eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE {
                    p_ctx.bParamSetsLostFlag = true;
                }
            }

            // `:844–854` — the trace throttle. One line per error burst, then a
            // counter, so a stream that fails on every access unit does not fill
            // the caller's log. `bPrintFrameErrorTraceFlag` is re-armed by
            // `DecodeFrameConstruction` on a complete frame.
            if p_ctx.bPrintFrameErrorTraceFlag {
                crate::decoder::decoder_core::WelsLog(
                    p_ctx.sLogCtx,
                    crate::decoder::decoder_core::WELS_LOG_INFO,
                    &format!("decode failed, failure type:{} \n", p_ctx.iErrorCode),
                );
                p_ctx.bPrintFrameErrorTraceFlag = false;
            } else {
                p_ctx.iIgnoredErrorInfoPacketCount =
                    p_ctx.iIgnoredErrorInfoPacketCount.wrapping_add(1);
                if p_ctx.iIgnoredErrorInfoPacketCount == i32::MAX {
                    crate::decoder::decoder_core::WelsLog(
                        p_ctx.sLogCtx,
                        crate::decoder::decoder_core::WELS_LOG_WARNING,
                        "continuous error reached INT_MAX! Restart as 0.",
                    );
                    p_ctx.iIgnoredErrorInfoPacketCount = 0;
                }
            }

            // `:856–882` — concealment happened and the frame came out anyway. The
            // four counters behind it are what `DECODER_OPTION_GET_STATISTICS`
            // reports.
            if p_ctx.pParam.eEcActiveIdc != ERROR_CON_IDC::ERROR_CON_DISABLE
                && pDstInfo.iBufferStatus == 1
            {
                p_ctx.iErrorCode |= DECODING_STATE::dsDataErrorConcealed.0;

                let iMbConcealedNum = p_ctx.iMbEcedNum.wrapping_add(p_ctx.iMbEcedPropNum);
                let iMbNum = p_ctx.iMbNum;
                let iMbEcedPropNum = p_ctx.iMbEcedPropNum;
                let stat = &mut p_ctx.pDecoderStatistics;

                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                if stat.uiDecodedFrameCount == 0 {
                    // exceeded the max value of uint32_t
                    ResetDecStatNums(stat);
                    stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                }
                // The running average is de-normalised by the frame count, the new
                // frame's percentage added, and the whole re-normalised below.
                stat.uiAvgEcRatio = if iMbNum == 0 {
                    stat.uiAvgEcRatio.wrapping_mul(stat.uiEcFrameNum)
                } else {
                    stat.uiAvgEcRatio
                        .wrapping_mul(stat.uiEcFrameNum)
                        .wrapping_add((iMbConcealedNum.wrapping_mul(100) / iMbNum) as u32)
                };
                stat.uiAvgEcPropRatio = if iMbNum == 0 {
                    stat.uiAvgEcPropRatio.wrapping_mul(stat.uiEcFrameNum)
                } else {
                    stat.uiAvgEcPropRatio
                        .wrapping_mul(stat.uiEcFrameNum)
                        .wrapping_add((iMbEcedPropNum.wrapping_mul(100) / iMbNum) as u32)
                };
                stat.uiEcFrameNum = stat
                    .uiEcFrameNum
                    .wrapping_add(u32::from(iMbConcealedNum != 0));
                stat.uiAvgEcRatio = if stat.uiEcFrameNum == 0 {
                    0
                } else {
                    stat.uiAvgEcRatio / stat.uiEcFrameNum
                };
                stat.uiAvgEcPropRatio = if stat.uiEcFrameNum == 0 {
                    0
                } else {
                    stat.uiAvgEcPropRatio / stat.uiEcFrameNum
                };
            }
            p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
            OutputStatisticsLog(&mut *p_ctx);
            // `:885–890`.
            ReorderPicturesInDisplay(&mut *p_ctx, ppDst, pDstInfo);
            // The accumulated error code, whole.
            return DECODING_STATE(p_ctx.iErrorCode);
        }

        // `:894–905` — error free. This frame counter is the divisor
        // `DECODER_OPTION_GET_STATISTICS` reports its two speeds by.
        if pDstInfo.iBufferStatus == 1 {
            let stat = &mut p_ctx.pDecoderStatistics;
            stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
            if stat.uiDecodedFrameCount == 0 {
                ResetDecStatNums(stat);
                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
            }
            OutputStatisticsLog(&mut *p_ctx);
        }
        p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
        ReorderPicturesInDisplay(&mut *p_ctx, ppDst, pDstInfo);

        DECODING_STATE::dsErrorFree
    }

    /// `CWelsDecoder::DecodeParser` — `welsDecoderExt.cpp:1180-1262`.
    ///
    /// `src` is one access unit, or `None` for the `(NULL, 0)` end-of-stream call.
    ///
    /// The output window: on the call that completes a frame, `pDstInfo`'s
    /// `pNalLenInByte` and `pDstBuff` name this decoder's parse-only buffers —
    /// `iNalNum` lengths and their concatenated bytes — valid until the next call on
    /// this decoder and not the caller's to free, as with [`Self::decode`]'s planes.
    pub fn decode_parser(
        &mut self,
        src: Option<&[u8]>,
        pDstInfo: &mut SParserBsInfo,
    ) -> DECODING_STATE {
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_ERROR,
                "Call DecodeParser without Initialize.",
            );
            return DECODING_STATE::dsInitialOptExpected;
        };
        // `:1189-1193` — the mode check.
        if !p_ctx.pParam.bParseOnly {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_ERROR,
                "bParseOnly should be true for this API calling! \n",
            );
            p_ctx.iErrorCode |= DECODING_STATE::dsInvalidArgument.0;
            return DECODING_STATE::dsInvalidArgument;
        }
        let dec_started = std::time::Instant::now();

        if src.is_some() {
            p_ctx.bEndOfStreamFlag = false;
        } else {
            // "for CONSOLE MODE, when decoding LAST AU, kiSrcLen==0 && kpSrc==NULL"
            p_ctx.bEndOfStreamFlag = true;
            p_ctx.bInstantDecFlag = true;
        }

        p_ctx.iErrorCode = DECODING_STATE::dsErrorFree.0;
        // "add protection to disable EC here" (`:1216`).
        p_ctx.pParam.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_DISABLE;
        p_ctx.iFeedbackNalRefIdc = -1;
        if !p_ctx.bFramePending {
            // `:1219-1220`. Every slot is written by `pNalLenInByte[iNalNum++] = …`
            // before anything reads it, and the one reader sums `0..iNalNum`.
            if let Some(p) = parser_bs(&mut p_ctx.pParserBsInfo) {
                p.iNalNum = 0;
                p.pNalLenInByte.fill(0);
            }
        }
        pDstInfo.iNalNum = 0;
        pDstInfo.iSpsWidthInPixel = 0;
        pDstInfo.iSpsHeightInPixel = 0;
        p_ctx.uiTimeStamp = pDstInfo.uiInBsTimeStamp;
        pDstInfo.uiOutBsTimeStamp = 0;

        // `:1230`. The picture-output parameters are never reached in parse-only mode:
        // `DecodeFrameConstruction` returns out of its `bParseOnly` arm before the
        // reconstruction path touches either. Locals here; nothing reads them back.
        let mut ppDstUnused: [*mut u8; 3] = [ptr::null_mut(); 3];
        let mut sDstInfoUnused = SBufferInfo::default();
        let (bs, len): (&[u8], i32) = match src {
            Some(s) => (s, s.len() as i32),
            None => (&[], 0),
        };
        crate::decoder::decoder_core::WelsDecodeBs(
            &mut *p_ctx,
            bs,
            len,
            &mut ppDstUnused,
            &mut sDstInfoUnused,
            ptr::null_mut(),
        );

        // `:1231-1236` — out of memory rebuilds the decoder over the saved parameter
        // block, the rebuild being the recovery.
        if p_ctx.iErrorCode & crate::decoder::decoder_core::dsOutOfMemory != 0 {
            let sPrevParam = p_ctx.pParam;
            let _ = self.initialize(&sPrevParam);
            return DECODING_STATE::dsOutOfMemory;
        }

        // `:1238-1249` — the copy-out, field by field; the two raw pointers are minted
        // from the `Vec`s that own the bytes.
        let bFrameDone = !p_ctx.bFramePending;
        if bFrameDone {
            let filled = match parser_bs(&mut p_ctx.pParserBsInfo) {
                Some(p) if p.iNalNum != 0 => {
                    pDstInfo.iNalNum = p.iNalNum;
                    pDstInfo.pNalLenInByte = p.pNalLenInByte.as_mut_ptr();
                    pDstInfo.pDstBuff = p.pDstBuff.as_mut_ptr();
                    pDstInfo.iSpsWidthInPixel = p.iSpsWidthInPixel;
                    pDstInfo.iSpsHeightInPixel = p.iSpsHeightInPixel;
                    // Nothing writes the decoder-side `uiInBsTimeStamp`, so the
                    // caller's input timestamp is overwritten with zero on every
                    // completed frame.
                    pDstInfo.uiInBsTimeStamp = p.uiInBsTimeStamp;
                    pDstInfo.uiOutBsTimeStamp = p.uiOutBsTimeStamp;
                    true
                }
                _ => false,
            };
            if filled && p_ctx.iErrorCode == ERR_NONE {
                let stat = &mut p_ctx.pDecoderStatistics;
                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                if stat.uiDecodedFrameCount == 0 {
                    ResetDecStatNums(stat);
                    stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                }
            }
        }

        p_ctx.bInstantDecFlag = false; // reset no-delay flag

        if p_ctx.iErrorCode != 0 && p_ctx.bPrintFrameErrorTraceFlag {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_INFO,
                &format!("decode failed, failure type:{} \n", p_ctx.iErrorCode),
            );
            p_ctx.bPrintFrameErrorTraceFlag = false;
        }
        p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
        DECODING_STATE(p_ctx.iErrorCode)
    }

    /// `CWelsDecoder::FlushFrame` — `welsDecoderExt.cpp:1094`: drains the display
    /// reordering buffer only. The decoder core itself is flushed by the caller
    /// through [`Self::decode`] with `None` after signalling end of stream.
    ///
    /// Same output window as [`Self::decode`].
    pub fn flush(
        &mut self,
        ppDst: &mut [*mut u8; 3],
        pDstInfo: &mut SBufferInfo,
    ) -> DECODING_STATE {
        // With no context there is no reordering state to drain: the buffers are the
        // context's own fields.
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            return DECODING_STATE::dsErrorFree;
        };
        if p_ctx.bEndOfStreamFlag && p_ctx.pPictReoderingStatus.iNumOfPicts > 0 {
            // `false`: drain the slot list without touching the live pool. See
            // `pool_for`.
            ReleaseBufferedReadyPictureReorder(&mut *p_ctx, false, ppDst, pDstInfo, true);
        }
        DECODING_STATE::dsErrorFree
    }

    fn sync_log_ctx(&mut self) {
        let log_ctx = self.trace.log_context();
        if let Some(pCtx) = self.ctx.as_mut() {
            pCtx.sLogCtx = log_ctx;
        }
    }
}

// ===========================================================================
// The decoder's ten vtable slots. The window that matters most is `DecodeFrame2`'s
// output: `ppDst` comes back naming the decoder's planes, valid until the next call
// on this decoder.
// ===========================================================================

/// `ISVCDecoder::Initialize` — `codec_api.h:452`.
///
/// # Safety
///
/// * `this` is either null or a pointer to a live `CWelsDecoderImpl` from
///   [`WelsCreateDecoder`], cast to the whole impl allocation.
/// * `pParam` must be null or point to a readable, aligned `SDecodingParam`-sized
///   object for the duration of this call.
///
/// `pParam` stays a raw pointer: two of its fields are C `int`s whose wire domain is
/// wider than the Rust enums they are typed as, so `&*pParam` would be undefined for
/// exactly the inputs the clamp exists to handle. It is read as bytes and sanitised
/// field-wise before it becomes an `SDecodingParam`.
#[allow(unsafe_code)]
unsafe extern "C" fn decoder_init_c(
    this: *mut ISVCDecoder,
    pParam: *const SDecodingParam,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::Initialize",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            let dec_impl = this as *mut CWelsDecoderImpl;
            // The null is reported by the impl, which has the trace object, rather
            // than short-circuited here.
            if pParam.is_null() {
                return unsafe { (*dec_impl).core.report_init_null_param() };
            }
            unsafe {
                // The caller's block, read as a C caller may have written it.
                //
                // `SDecodingParam`'s two enum-typed fields, `eEcActiveIdc` and
                // `sVideoProperty.eVideoBsType`, are plain `int`s on the C side and are
                // sanitised after the copy: the first is clamped into
                // `[ERROR_CON_DISABLE, …FREEZE_RES_CHANGE]` (`decoder.cpp:654`), the
                // second normalised to `VIDEO_BITSTREAM_DEFAULT` (`:667`). Each has a
                // closed set of variants in Rust, so `*pParam` would be undefined for
                // exactly the inputs the sanitising exists to handle.
                //
                // So the block is copied as bytes, those two fields are read and written
                // at their own offsets as the `i32`s they are on the wire, and only then
                // does it become an `SDecodingParam`. Every other field is a pointer, an
                // integer or a `bool`.
                let param = {
                    let mut buf = std::mem::MaybeUninit::<SDecodingParam>::uninit();
                    ptr::copy_nonoverlapping(
                        pParam.cast::<u8>(),
                        buf.as_mut_ptr().cast::<u8>(),
                        size_of::<SDecodingParam>(),
                    );
                    let ec = ptr::addr_of_mut!((*buf.as_mut_ptr()).eEcActiveIdc).cast::<i32>();
                    ec.write(ec_idc_from_raw(ec.read()) as i32);
                    let bs = ptr::addr_of_mut!((*buf.as_mut_ptr()).sVideoProperty.eVideoBsType)
                        .cast::<i32>();
                    bs.write(video_bs_type_from_raw(bs.read()) as i32);
                    buf.assume_init()
                };
                (*dec_impl).core.initialize(&param)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::Uninitialize` — `codec_api.h:458`.
///
/// # Safety
///
/// `this` as in [`decoder_init_c`]. Nothing else crosses. After this call the
/// planes any previous `DecodeFrame2` handed out are freed with the context and
/// must not be read.
unsafe extern "C" fn decoder_uninit_c(this: *mut ISVCDecoder) -> c_long {
    abi_guard!(
        "ISVCDecoder::Uninitialize",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            unsafe { (*(this as *mut CWelsDecoderImpl)).core.uninitialize() }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrame` — `codec_api.h:466`. Deprecated upstream.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pSrc` / `iSrcLen`: either `pSrc` is null or `iSrcLen <= 0` (the flush call),
///   or `pSrc` names `iSrcLen` readable bytes for the duration of this call. The
///   decoder copies what it keeps; nothing of the caller's buffer is retained.
/// * `ppDst` must name three writable plane-pointer slots. They come back pointing
///   into the decoder's own picture buffer, readable until the next call on this
///   decoder, which is why they cannot be a `&mut [u8]`.
/// * `pStride` must be null or name two writable `i32`s; `iWidth`, `iHeight` null or
///   one each. All three are out parameters and are written only when a frame was
///   emitted.
unsafe extern "C" fn decoder_decode_frame_c(
    this: *mut ISVCDecoder,
    pSrc: *const u8,
    iSrcLen: i32,
    ppDst: *mut *mut u8,
    pStride: *mut i32,
    iWidth: *mut i32,
    iHeight: *mut i32,
) -> DECODING_STATE {
    unsafe {
        abi_guard!(
            "ISVCDecoder::DecodeFrame",
            decoder_log(this),
            DECODING_STATE::dsBitstreamError,
            {
                let mut buf_info = SBufferInfo::default();
                let state = decoder_decode_frame2_c(this, pSrc, iSrcLen, ppDst, &mut buf_info);
                if buf_info.iBufferStatus != 1 {
                    return state;
                }
                // Each of the three is optional and written only on the frame-emitted
                // path; `pStride` is two `i32`s.
                let sys = buf_info.UsrData.sys();
                if let Some(pStride) = pStride.cast::<[i32; 2]>().as_mut() {
                    pStride[0] = sys.iStride[0];
                    pStride[1] = sys.iStride[1];
                }
                if let Some(iWidth) = iWidth.as_mut() {
                    *iWidth = sys.iWidth;
                }
                if let Some(iHeight) = iHeight.as_mut() {
                    *iHeight = sys.iHeight;
                }
                state
            }
        )
    }
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrameNoDelay` — `codec_api.h:479`.
///
/// # Safety
///
/// As [`decoder_decode_frame2_c`], which this calls twice. Both calls write `ppDst`
/// and `pDstInfo`, and the second call's values are the ones the caller sees.
///
/// # What "no delay" is
///
/// The second call — `DecodeFrame2 (NULL, 0, …)`, `welsDecoderExt.cpp:720–725` —
/// forces reconstruction, so a caller gets the frame on the call that fed the access
/// unit rather than on the next one.
///
/// The out-parameters are deliberately not restored: if the first call emits a picture
/// and the second does not, the second call's `iBufferStatus = 0` overwrites it and
/// the frame is lost to that caller.
unsafe extern "C" fn decoder_decode_frame_nodelay_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    unsafe {
        abi_guard!(
            "ISVCDecoder::DecodeFrameNoDelay",
            decoder_log(this),
            DECODING_STATE::dsBitstreamError,
            {
                // `iRet |=` on `DECODING_STATE`, which is a bitset of `ds*` flags — the two
                // calls' states are ORed, not replaced, so an error in either half survives.
                let first = decoder_decode_frame2_c(this, kpSrc, kiSrcLen, ppDst, pDstInfo);
                let second = decoder_decode_frame2_c(this, ptr::null(), 0, ppDst, pDstInfo);
                DECODING_STATE(first.0 | second.0)
            }
        )
    }
}

/// `pCtx ? pCtx->pPicBuff : m_pPicBuff` — the C++'s pool selection, as a flag.
///
/// The ternary carries exactly one bit: `FlushFrame` passes a null context to say "do
/// not touch the live pool" (`welsDecoderExt.cpp:1103`), and every other caller passes
/// the real one. That bit is `bUsePool`, and spelling it as a bool is what lets the
/// reordering state live in the context — a null context argument cannot carry the
/// state the callee reads out of it.
#[inline]
fn pool_for(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
) -> Option<&mut SPicBuff> {
    if !bUsePool {
        return None;
    }
    pic_pool_ptr(&mut pCtx.pPicBuff)
}

/// Matches `void CWelsDecoder::UpdateReorderingParameters (...)`.
///
/// Refreshes, from the active SPS, the three things the display layer needs to put
/// pictures into output order: whether this stream reorders at all, how many frames
/// its DPB holds, and what its VUI promises about reordering. `false` when no SPS is
/// active yet — the C's `if (pDecContext->pSps != NULL)` guard.
fn UpdateReorderingParameters(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
) -> bool {
    let Some(sps) =
        crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).copied()
    else {
        return false;
    };
    pCtx.bIsBaseline = sps.uiProfileIdc == 66 || sps.uiProfileIdc == 83;
    let st = &mut pCtx.pPictReoderingStatus;
    st.bReorderPictures = crate::decoder::decoder_core::NeedsPictureReordering(&sps);
    st.iDpbSize = crate::decoder::decoder_core::GetDpbSize(&sps);
    st.iMaxNumReorderFrames = if sps.sVui.bBitstreamRestrictionFlag {
        sps.sVui.uiMaxNumReorderFrames as i32
    } else {
        -1
    };
    true
}

/// Matches `int32_t CWelsDecoder::GetDpbFullness (...)`.
///
/// DPB fullness in frame buffers, C.4.1: `|R| + |W \ R|`, where `R` is the set of
/// pictures the core holds as reference right now and `W` the set waiting here for
/// output. A picture that is both is one frame buffer, not two; `iPicBuffIdx` names
/// the pool slot on both sides.
///
/// Marking for the current picture has already run when this is reached —
/// `DecodeCurrentAccessUnit` calls `DecodeFrameConstruction` and then `WelsMarkAsRef`
/// before returning to this layer — so `R` already holds the current picture if it
/// is a reference, and `W` holds it because it has just been buffered. The count is
/// therefore one more than the fullness C.4.5.3 tests before storing the current
/// picture, which is why the caller's test is `> iDpbSize` and not `>=`.
fn GetDpbFullness(pCtx: &crate::decoder::decoder_core::SWelsDecoderContext) -> i32 {
    // The distinct pool slots held as reference, by `iPicBuffIdx`. Short and long
    // lists can name the same picture, so they are unioned rather than added.
    let mut aiRefBuffIdx = [-1i32; 2 * MAX_DPB_COUNT];
    let mut iRefs = 0usize;
    let pPicBuff = pCtx.pPicBuff.as_deref();
    for iList in 0..2 {
        let (pList, kuiCount) = if iList == 0 {
            (
                &pCtx.sRefPic.pShortRefList[LIST_0],
                pCtx.sRefPic.uiShortRefCount[LIST_0],
            )
        } else {
            (
                &pCtx.sRefPic.pLongRefList[LIST_0],
                pCtx.sRefPic.uiLongRefCount[LIST_0],
            )
        };
        for slot in pList.iter().take((kuiCount as usize).min(MAX_DPB_COUNT)) {
            let Some(id) = *slot else { continue };
            let Some(iPicBuffIdx) = pPicBuff.and_then(|p| p.slot(id)).map(|pic| pic.iPicBuffIdx)
            else {
                continue;
            };
            if iPicBuffIdx < 0 || aiRefBuffIdx[..iRefs].contains(&iPicBuffIdx) {
                continue;
            }
            if iRefs < aiRefBuffIdx.len() {
                aiRefBuffIdx[iRefs] = iPicBuffIdx;
                iRefs += 1;
            }
        }
    }
    let mut iWaiting = 0i32;
    let largest = pCtx.pPictReoderingStatus.iLargestBufferedPicIndex;
    for i in 0..=largest {
        let info = pCtx.pPictInfoList[i as usize];
        if info.iPOC == crate::decoder::decoder_context::IMinInt32 {
            continue;
        }
        if !aiRefBuffIdx[..iRefs].contains(&info.iPicBuffIdx) {
            iWaiting += 1;
        }
    }
    iRefs as i32 + iWaiting
}

/// Matches `void CWelsDecoder::StoreReadyPicture (...)`.
///
/// Moves the current picture into the first free slot of the picture list and takes
/// the DPB reference that keeps its planes alive until it is emitted. Leaves
/// `iBufferStatus` alone — i.e. the picture where it is — if the list is full; see
/// [`EmitOnFullPictInfoList`].
fn StoreReadyPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    pDstInfo: &mut SBufferInfo,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    let Some(i) = (0..PICT_INFO_LIST_SIZE).find(|&i| pCtx.pPictInfoList[i].iPOC == IMinInt32)
    else {
        return;
    };
    pCtx.pPictInfoList[i].sBufferInfo = *pDstInfo;
    // The DPB's "previous picture" is a slot handle, so it is resolved here. The
    // thread count is read before the pool borrow opens, since `GetThreadCount`
    // takes the context.
    let bSingleThreaded = crate::decoder::decoder_core::GetThreadCount(&*pCtx) <= 1;
    let prev_id = prev_dpb_id(&pCtx.pLastDecPicInfo);
    let mut sPrev: Option<(i32, i32)> = None;
    if let Some(prev) = prev_dpb_pic_mut(&mut pCtx.pPicBuff, prev_id) {
        sPrev = Some((prev.iFramePoc, prev.iPicBuffIdx));
        if bSingleThreaded {
            prev.iRefCount += 1;
        }
    }
    // The decoded picture's own POC, not the slice header's: the two agree except
    // after an MMCO 5, where marking zeroes the picture's POC as 8.2.1 requires and
    // the slice header still carries what was coded.
    pCtx.pPictInfoList[i].iPOC = match sPrev {
        Some((iFramePoc, iPicBuffIdx)) => {
            pCtx.pPictInfoList[i].iPicBuffIdx = iPicBuffIdx;
            iFramePoc
        }
        None => slice_header_of(&*pCtx).map_or(0, |sh| sh.iPicOrderCntLsb),
    };
    pCtx.pPictInfoList[i].iSeqNum = pCtx.pPictReoderingStatus.iOutputSeqNum;
    pCtx.pPictInfoList[i].uiDecodingTimeStamp = pCtx.uiDecodingTimeStamp;
    pDstInfo.iBufferStatus = 0;
    pCtx.pPictReoderingStatus.iNumOfPicts += 1;
    if i as i32 > pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        pCtx.pPictReoderingStatus.iLargestBufferedPicIndex = i as i32;
    }
}

/// Matches `void CWelsDecoder::EmitOnFullPictInfoList (...)`.
///
/// The picture list is full: emit whichever of the current picture and the smallest
/// waiting picture comes first in output order, so that the output order still holds
/// and neither is dropped.
///
/// [`PICT_INFO_LIST_SIZE`] is twice the largest DPB Table A-1 allows, and the backlog
/// this layer can build is bounded below that, so this is a guarantee that a picture
/// is never dropped rather than a path a conforming stream reaches.
fn EmitOnFullPictInfoList(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    let kiCurPoc = match prev_dpb_id(&pCtx.pLastDecPicInfo)
        .and_then(|id| pCtx.pPicBuff.as_deref().and_then(|p| p.slot(id)))
    {
        Some(pic) => pic.iFramePoc,
        None => slice_header_of(&*pCtx).map_or(0, |sh| sh.iPicOrderCntLsb),
    };
    let mut iMinIdx: Option<usize> = None;
    for i in 0..=pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        let info = pCtx.pPictInfoList[i as usize];
        if info.iPOC == IMinInt32 {
            continue;
        }
        let bSmaller = match iMinIdx {
            None => true,
            Some(m) => {
                let cur = pCtx.pPictInfoList[m];
                if info.iSeqNum == cur.iSeqNum {
                    info.iPOC < cur.iPOC
                } else {
                    info.iSeqNum.wrapping_sub(cur.iSeqNum) < 0
                }
            }
        };
        if bSmaller {
            iMinIdx = Some(i as usize);
        }
    }
    let kbCurrentIsFirst = match iMinIdx {
        None => true,
        Some(m) => {
            let min = pCtx.pPictInfoList[m];
            let iOutputSeqNum = pCtx.pPictReoderingStatus.iOutputSeqNum;
            if min.iSeqNum == iOutputSeqNum {
                kiCurPoc <= min.iPOC
            } else {
                iOutputSeqNum.wrapping_sub(min.iSeqNum) < 0
            }
        }
    };
    if kbCurrentIsFirst {
        // Straight out, the way C.4.5.2 outputs a picture no waiting one precedes.
        ppDst[0] = pDstInfo.pDst[0];
        ppDst[1] = pDstInfo.pDst[1];
        ppDst[2] = pDstInfo.pDst[2];
        return;
    }
    // Force the smallest waiting picture out, which frees its slot, buffer the
    // current picture into it, and hand the freed picture to the caller.
    let sCurrent = *pDstInfo;
    ReleaseBufferedReadyPictureReorder(pCtx, true, ppDst, pDstInfo, true);
    let sEmitted = *pDstInfo;
    let pEmitted = *ppDst;
    *pDstInfo = sCurrent;
    StoreReadyPicture(pCtx, pDstInfo);
    *pDstInfo = sEmitted;
    *ppDst = pEmitted;
}

/// Matches `void CWelsDecoder::BufferingReadyPicture (...)` in `welsDecoderExt.cpp`.
///
/// Moves a just-decoded picture out of `pDstInfo` and into the reordering slot list,
/// clearing `iBufferStatus` so nothing is emitted until a release call picks the
/// picture back up in display order, and closes off the coded video sequence the
/// picture ends, if it ends one.
fn BufferingReadyPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    _ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    if pDstInfo.iBufferStatus == 0 {
        return;
    }
    UpdateReorderingParameters(pCtx);
    // A coded video sequence ends at an IDR or an SPS change — which is what the
    // core's own `iSeqNum` counts — and at a memory_management_control_operation
    // equal to 5, which 8.2.1 makes the current picture's POC zero and C.4.4 makes a
    // point past which nothing earlier may still be waiting. Marking has already run,
    // so `bLastHasMmco5` is this picture's flag.
    let kbNewSequence = pCtx.pPictReoderingStatus.iPrevCoreSeqNum != pCtx.iSeqNum
        || pCtx.pLastDecPicInfo.bLastHasMmco5;
    if kbNewSequence {
        pCtx.pPictReoderingStatus.iOutputSeqNum += 1;
        pCtx.pPictReoderingStatus.iPrevCoreSeqNum = pCtx.iSeqNum;
        // C.4.4: an IDR that asks for it discards the pictures of the sequence before
        // it instead of outputting them.
        let bNoOutputOfPriorPics = slice_header_of(&*pCtx)
            .is_some_and(|sh| sh.bIdrFlag && sh.sRefMarking.bNoOutputOfPriorPicsFlag);
        if bNoOutputOfPriorPics {
            DiscardBufferedPictures(pCtx);
        }
    }
    StoreReadyPicture(pCtx, pDstInfo);
}

/// The `no_output_of_prior_pics_flag` arm of C.4.4: every waiting picture is
/// released without being output.
fn DiscardBufferedPictures(pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    for i in 0..=pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        let idx = i as usize;
        if pCtx.pPictInfoList[idx].iPOC == IMinInt32 {
            continue;
        }
        pCtx.pPictInfoList[idx].iPOC = IMinInt32;
        let iPicBuffIdx = pCtx.pPictInfoList[idx].iPicBuffIdx;
        if let Some(pPicBuff) = pic_pool_ptr(&mut pCtx.pPicBuff) {
            if let Some(pPic) = pPicBuff.slot_at_mut(iPicBuffIdx) {
                pPic.iRefCount -= 1;
                if pPic.iRefCount <= 0 {
                    if let Some(set_unref) = pPic.pSetUnRef {
                        set_unref(pPic);
                    }
                }
            }
        }
        pCtx.pPictReoderingStatus.iNumOfPicts -= 1;
    }
}

/// Releases the buffered picture whose slot is referenced by `iPictInfoIndex`,
/// dropping the DPB reference taken in [`StoreReadyPicture`]. Shared tail of
/// `ReleaseBufferedReadyPictureReorder` and `EmitOnFullPictInfoList` in
/// `welsDecoderExt.cpp`.
fn EmitBufferedPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    let idx = pCtx.pPictReoderingStatus.iPictInfoIndex as usize;
    *pDstInfo = pCtx.pPictInfoList[idx].sBufferInfo;
    ppDst[0] = pDstInfo.pDst[0];
    ppDst[1] = pDstInfo.pDst[1];
    ppDst[2] = pDstInfo.pDst[2];
    pCtx.pPictInfoList[idx].iPOC = crate::decoder::decoder_context::IMinInt32;
    let iPicBuffIdx = pCtx.pPictInfoList[idx].iPicBuffIdx;
    // The pool is resolved here, not by the caller: the flag comes down instead and
    // the borrow is taken — and ended — inside this block.
    if let Some(pPicBuff) = pool_for(pCtx, bUsePool) {
        // `slot_at_mut` carries the `>= 0 && < iCapacity` range test, so a failed test
        // is its `None`.
        if let Some(pPic) = pPicBuff.slot_at_mut(iPicBuffIdx) {
            pPic.iRefCount -= 1;
            if pPic.iRefCount <= 0 {
                if let Some(set_unref) = pPic.pSetUnRef {
                    set_unref(pPic);
                }
            }
        }
    }
    pCtx.pPictReoderingStatus.iNumOfPicts -= 1;
}

/// Matches `void CWelsDecoder::ReleaseBufferedReadyPictureReorder (...)`.
///
/// The bumping process of C.4.5.3, one output per completed picture.
///
/// Picks the buffered picture with the smallest `(sequence, POC)` — the next one in
/// output order, since output order is POC order within a coded video sequence and
/// sequences follow one another in decoding order — and emits it when the DPB says
/// nothing still to come can precede it:
///
/// * `isFlush`: end of stream, everything left goes out in order;
/// * the picture is from an older sequence than the one being decoded. C.4.4 empties
///   the DPB across a sequence boundary, so nothing that follows can precede it;
/// * the DPB has no empty frame buffer (C.4.5.3). [`GetDpbFullness`] counts one more
///   than the fullness the specification tests, because the current picture is
///   already stored here and already marked there, hence `> iDpbSize`;
/// * the VUI carries `max_num_reorder_frames` and more than that many pictures are
///   waiting (E.2.1): the smallest of them cannot be preceded by a picture that has
///   not been decoded yet. This is the term that keeps the latency of a stream with
///   a VUI down to what its encoder promised.
fn ReleaseBufferedReadyPictureReorder(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
    isFlush: bool,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    // The pool resolution happens down in [`EmitBufferedPicture`]: held here it would
    // be a borrow of `pCtx` spanning the whole body below, which reads `pCtx`
    // throughout. Nothing between this point and the call writes `pCtx.pPicBuff`.

    if pCtx.pPictReoderingStatus.iNumOfPicts > 0 {
        pCtx.pPictReoderingStatus.iMinPOC = IMinInt32;
        let mut firstValidIdx: i32 = -1;
        let largest = pCtx.pPictReoderingStatus.iLargestBufferedPicIndex;
        for i in 0..=largest {
            let info = pCtx.pPictInfoList[i as usize];
            if pCtx.pPictReoderingStatus.iMinPOC == IMinInt32 && info.iPOC > IMinInt32 {
                pCtx.pPictReoderingStatus.iMinPOC = info.iPOC;
                pCtx.pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                pCtx.pPictReoderingStatus.iPictInfoIndex = i;
                firstValidIdx = i;
                break;
            }
        }
        for i in 0..=largest {
            if i == firstValidIdx {
                continue;
            }
            let info = pCtx.pPictInfoList[i as usize];
            let min_seq = pCtx.pPictReoderingStatus.iMinSeqNum;
            let min_poc = pCtx.pPictReoderingStatus.iMinPOC;
            if info.iPOC > IMinInt32
                && (if info.iSeqNum == min_seq {
                    info.iPOC < min_poc
                } else {
                    info.iSeqNum.wrapping_sub(min_seq) < 0
                })
            {
                pCtx.pPictReoderingStatus.iMinPOC = info.iPOC;
                pCtx.pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                pCtx.pPictReoderingStatus.iPictInfoIndex = i;
            }
        }
    }

    if pCtx.pPictReoderingStatus.iMinPOC > IMinInt32 {
        let mut isReady = true;
        if !isFlush {
            let st = pCtx.pPictReoderingStatus;
            isReady = st.iMinSeqNum.wrapping_sub(st.iOutputSeqNum) < 0
                || GetDpbFullness(&*pCtx) > st.iDpbSize
                || (st.iMaxNumReorderFrames >= 0 && st.iNumOfPicts > st.iMaxNumReorderFrames);
        }
        if isReady {
            pCtx.pPictReoderingStatus.iLastWrittenPOC = pCtx.pPictReoderingStatus.iMinPOC;
            pCtx.pPictReoderingStatus.iLastWrittenSeqNum = pCtx.pPictReoderingStatus.iMinSeqNum;
            EmitBufferedPicture(pCtx, bUsePool, ppDst, pDstInfo);
            pCtx.pPictReoderingStatus.iMinPOC = IMinInt32;
        }
    }
}

/// Matches `DECODING_STATE CWelsDecoder::ReorderPicturesInDisplay (...)`.
///
/// A stream that cannot reorder — baseline, a POC type this decoder derives no POC
/// for, or a VUI that promises output order is decoding order — is handed its
/// picture the moment it is decoded. Everything else goes into the buffer and comes
/// out by the bumping process.
fn ReorderPicturesInDisplay(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    if !UpdateReorderingParameters(pCtx) {
        return;
    }
    if !pCtx.pPictReoderingStatus.bReorderPictures || pDstInfo.iBufferStatus != 1 {
        return;
    }
    BufferingReadyPicture(pCtx, ppDst, pDstInfo);
    if pDstInfo.iBufferStatus == 1 {
        // The picture list was full, so the picture is still here; it is sized so that
        // this cannot happen.
        EmitOnFullPictInfoList(pCtx, ppDst, pDstInfo);
    } else {
        ReleaseBufferedReadyPictureReorder(pCtx, true, ppDst, pDstInfo, false);
    }
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrame2` — `codec_api.h:490`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `kpSrc` / `kiSrcLen` as in [`decoder_decode_frame_c`]: null-or-zero is the
///   end-of-stream flush, otherwise `kiSrcLen` readable bytes for this call.
/// * `ppDst` must name three writable plane-pointer slots, and `pDstInfo` a writable,
///   aligned `SBufferInfo`. Both are out parameters.
/// * When `pDstInfo->iBufferStatus == 1`, `ppDst[0..3]` and `pDstInfo->pDst[0..3]`
///   name planes inside the decoder's picture buffer with the strides
///   `pDstInfo->UsrData.sSystemBuffer.iStride` reports. They are valid until the next
///   call on this decoder — the next `DecodeFrame*`, `FlushFrame`, `Uninitialize` or
///   `WelsDestroyDecoder` — and are not the caller's to free, which is why this slot
///   hands back pointers instead of slices.
unsafe extern "C" fn decoder_decode_frame2_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrame2",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            panic_probe!(PROBE_DECODE_FRAME2);
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            let dec_impl = this as *mut CWelsDecoderImpl;
            unsafe {
                // The caller's access unit, or `None` for the end-of-stream flush that
                // `(NULL, 0)` means on this slot.
                let src: Option<&[u8]> = if kpSrc.is_null() || kiSrcLen <= 0 {
                    None
                } else {
                    Some(std::slice::from_raw_parts(kpSrc, kiSrcLen as usize))
                };
                let Some(ppDst) = (ppDst as *mut [*mut u8; 3]).as_mut() else {
                    return DECODING_STATE::dsInitialOptExpected;
                };
                let Some(pDstInfo) = pDstInfo.as_mut() else {
                    return DECODING_STATE::dsInitialOptExpected;
                };
                (*dec_impl).core.decode(src, ppDst, pDstInfo)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrameEx` — `codec_api.h:503`.
///
/// # Safety
///
/// `this` as in [`decoder_init_c`]; every other argument is unread.
///
/// A stub: returns `dsErrorFree` without touching anything. The slot exists so the
/// vtable's shape and slot order match `codec_api.h`.
unsafe extern "C" fn decoder_decode_frame_ex_c(
    _this: *mut ISVCDecoder,
    _pSrc: *const u8,
    _iSrcLen: i32,
    _pDst: *mut u8,
    _iDstStride: i32,
    _iDstLen: *mut i32,
    _iWidth: *mut i32,
    _iHeight: *mut i32,
    _iColorFormat: *mut i32,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrameEx",
        unsafe { decoder_log(_this) },
        DECODING_STATE::dsBitstreamError,
        { DECODING_STATE::dsErrorFree }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::SetOption` — `codec_api.h:518`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pOption` stays raw, for the same reason as
///   [`encoder_set_opt_c`]: its type is a function of `eOptionId`. The caller must
///   point it at a readable, aligned object of the type that id names, for the
///   duration of this call — an `i32` for `END_OF_STREAM` and `ERROR_CON_IDC`, a
///   `u32` for `TRACE_LEVEL`, a `WelsTraceCallback` for `TRACE_CALLBACK`, a `void*`
///   for `TRACE_CALLBACK_CONTEXT`.
/// * The pointer installed by `DECODER_OPTION_TRACE_CALLBACK_CONTEXT` is kept and
///   handed back to the callback on every message until it is replaced or the decoder
///   is destroyed. It is the one value on this interface whose window outlives the
///   call, and it is the caller's to keep alive.
unsafe extern "C" fn decoder_set_opt_c(
    this: *mut ISVCDecoder,
    eOptionId: DECODER_OPTION,
    pOption: *mut c_void,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::SetOption",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            // The blob's type is the option id's: every arm reads it at that type and
            // hands the value to a safe method.
            unsafe {
                let core = &mut (*(this as *mut CWelsDecoderImpl)).core;

                // `welsDecoderExt.cpp:479-584`. Nine arms, and two head clauses:
                //
                //   1. `NUM_OF_THREADS` first, and it succeeds whether or not the decoder
                //      has a context — it is the object's field;
                //   2. then, for every other id except the three trace ones, a missing
                //      context is `dsInitialOptExpected`.
                if eOptionId == DECODER_OPTION::DECODER_OPTION_NUM_OF_THREADS {
                    // `:481-501`. Single-threaded here, so the count clamps to 0.
                    // Success on any input, including a null.
                    return CM_RESULT_SUCCESS as c_long;
                }
                let ctx_needed = !matches!(
                    eOptionId,
                    DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL
                        | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK
                        | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT
                );
                if ctx_needed && !core.has_ctx() {
                    return DECODING_STATE::dsInitialOptExpected.0 as c_long;
                }

                match eOptionId {
                    // `pOption` is tested per arm, and the arms disagree about what a
                    // null means: `END_OF_STREAM` and `ERROR_CON_IDC` reject it, the
                    // trace ones dereference it, `STATISTICS_LOG_INTERVAL` reaches the
                    // trailing `cmInitParaError`.
                    DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                        if pOption.is_null() {
                            return CM_INIT_PARA_ERROR as c_long;
                        }
                        core.set_end_of_stream(pOption.cast::<i32>().read() != 0);
                        CM_RESULT_SUCCESS as c_long
                    }
                    DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                        if pOption.is_null() {
                            return CM_INIT_PARA_ERROR as c_long;
                        }
                        // The blob is an `int`, run through
                        // `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, …FREEZE_RES_CHANGE)`
                        // before the store (`welsDecoderExt.cpp:528`). Reading it as
                        // `*const ERROR_CON_IDC` would be undefined for anything outside
                        // 0..=7, which is exactly the range the clamp enforces, so it
                        // crosses as an `i32` and becomes an `ERROR_CON_IDC` only once
                        // `Decoder::set_error_concealment` has clamped it — which is also
                        // where the parse-only refusal (`:531-536`) lives.
                        core.set_error_concealment(pOption.cast::<i32>().read())
                    }
                    // The three trace options — `welsDecoderExt.cpp:541-561`. These are
                    // the three ids that work without a context.
                    DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL
                    | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK
                    | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT => {
                        if pOption.is_null() {
                            return CM_INIT_PARA_ERROR as c_long;
                        }
                        match eOptionId {
                            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL => {
                                core.set_trace_level(pOption.cast::<u32>().read());
                            }
                            DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK => {
                                core.set_trace_callback(pOption.cast::<WelsTraceCallback>().read());
                            }
                            // The one value whose window outlives the call — see the
                            // contract.
                            _ => core
                                .set_trace_callback_context(pOption.cast::<*mut c_void>().read()),
                        }
                        CM_RESULT_SUCCESS as c_long
                    }
                    // `welsDecoderExt.cpp:562` and `:578` — get-only, and both say so.
                    DECODER_OPTION::DECODER_OPTION_GET_STATISTICS
                    | DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO => CM_INIT_PARA_ERROR as c_long,
                    // `:571-577`. A null `pOption` is `cmInitParaError`.
                    DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL => {
                        if pOption.is_null() {
                            return CM_INIT_PARA_ERROR as c_long;
                        }
                        if core.set_statistics_log_interval(pOption.cast::<u32>().read()) {
                            CM_RESULT_SUCCESS as c_long
                        } else {
                            DECODING_STATE::dsInitialOptExpected.0 as c_long
                        }
                    }
                    // `:583` — an id with no arm is an error.
                    _ => CM_INIT_PARA_ERROR as c_long,
                }
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::GetOption` — `codec_api.h:525`.
///
/// # Safety
///
/// As [`decoder_set_opt_c`], with the blob written: `DECODER_OPTION_GET_STATISTICS`
/// writes a whole `SDecoderStatistics` through it, so a caller who passes an `i32`
/// for that id overflows its own object. `pOption` must be a writable, aligned
/// object of the type `eOptionId` names, for the duration of this call.
unsafe extern "C" fn decoder_get_opt_c(
    this: *mut ISVCDecoder,
    eOptionId: DECODER_OPTION,
    pOption: *mut c_void,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::GetOption",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            // Translate-out: each arm asks the core for a value and writes it at the type
            // the option id names.
            unsafe {
                let core = &(*(this as *mut CWelsDecoderImpl)).core;

                // `welsDecoderExt.cpp:584-695`. Three head clauses, in this order:
                //
                //   1. `NUM_OF_THREADS` is answered before the context is looked at —
                //      it is the object's field, not the context's, and it is the one id
                //      that works on an uninitialized decoder;
                //   2. then `pDecContext == NULL` -> `cmInitExpected`;
                //   3. then `pOption == NULL` -> `cmInitParaError`.
                //
                // So a null `pOption` on an uninitialized decoder reports
                // `cmInitExpected`, not `cmInitParaError`.
                if eOptionId == DECODER_OPTION::DECODER_OPTION_NUM_OF_THREADS {
                    if pOption.is_null() {
                        return CM_INIT_PARA_ERROR as c_long;
                    }
                    // `m_iThreadCount`: single-threaded, and `SetOption`'s arm clamps
                    // every request to it.
                    pOption.cast::<i32>().write(0);
                    return CM_RESULT_SUCCESS as c_long;
                }
                if !core.has_ctx() {
                    return CM_INIT_EXPECTED as c_long;
                }
                if pOption.is_null() {
                    return CM_INIT_PARA_ERROR as c_long;
                }

                // Past the head clauses every arm below has a context, so each accessor's
                // `Option` is `Some`.
                macro_rules! write_i32 {
                    ($v:expr) => {{
                        let Some(v) = $v else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        pOption.cast::<i32>().write(v);
                        return CM_RESULT_SUCCESS as c_long;
                    }};
                }

                match eOptionId {
                    DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                        write_i32!(Some(i32::from(core.end_of_stream())))
                    }
                    // `:603-619`, the four `LONG_TERM_REF` arms.
                    DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID => write_i32!(core.cur_idr_pic_id()),
                    DECODER_OPTION::DECODER_OPTION_FRAME_NUM => write_i32!(core.frame_num()),
                    DECODER_OPTION::DECODER_OPTION_LTR_MARKING_FLAG => {
                        write_i32!(core.ltr_marking_flag())
                    }
                    DECODER_OPTION::DECODER_OPTION_LTR_MARKED_FRAME_NUM => {
                        write_i32!(core.ltr_marked_frame_num())
                    }
                    DECODER_OPTION::DECODER_OPTION_VCL_NAL => write_i32!(core.feedback_vcl_nal()),
                    DECODER_OPTION::DECODER_OPTION_TEMPORAL_ID => {
                        write_i32!(core.feedback_temporal_id())
                    }
                    DECODER_OPTION::DECODER_OPTION_IS_REF_PIC => {
                        write_i32!(core.feedback_is_ref_pic())
                    }
                    // `welsDecoderExt.cpp:634-637`. The mode the decoder actually runs
                    // with is only visible through this option.
                    DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                        write_i32!(core.error_concealment().map(|idc| idc as i32))
                    }
                    // `welsDecoderExt.cpp:639-651`.
                    DECODER_OPTION::DECODER_OPTION_GET_STATISTICS => {
                        let Some(stats) = core.statistics() else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        pOption.cast::<SDecoderStatistics>().write(stats);
                        return CM_RESULT_SUCCESS as c_long;
                    }
                    // `:653-659`. An `unsigned int` on this id, in both directions.
                    DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL => {
                        let Some(v) = core.statistics_log_interval() else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        pOption.cast::<u32>().write(v);
                        return CM_RESULT_SUCCESS as c_long;
                    }
                    // `:664-672`. The caller's struct is zeroed before the SPS check, so
                    // a refusal still leaves zeros rather than the caller's stack.
                    DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO => {
                        pOption.cast::<SVuiSarInfo>().write(SVuiSarInfo::default());
                        let Some(sar) = core.sar_info() else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        let Some(sar) = sar else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        pOption.cast::<SVuiSarInfo>().write(sar);
                        return CM_RESULT_SUCCESS as c_long;
                    }
                    DECODER_OPTION::DECODER_OPTION_PROFILE => {
                        let Some(v) = core.active_sps_profile() else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        write_i32!(v)
                    }
                    DECODER_OPTION::DECODER_OPTION_LEVEL => {
                        let Some(v) = core.active_sps_level() else {
                            return CM_INIT_EXPECTED as c_long;
                        };
                        write_i32!(v)
                    }
                    DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER => {
                        // `:688-694`. The count is the context's own.
                        pOption.cast::<i32>().write(core.frames_remaining());
                        return CM_RESULT_SUCCESS as c_long;
                    }
                    // `:696` — an id with no arm is an error, not a silent success.
                    _ => return CM_INIT_PARA_ERROR as c_long,
                }
            }
        }
    )
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateSVCEncoder(ppEncoder: *mut *mut ISVCEncoder) -> i32 {
    unsafe {
        abi_guard!("WelsCreateSVCEncoder", None, CM_MALLOC_MEM_ERROR, {
            if ppEncoder.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let vtbl = Box::new(ISVCEncoderVtbl {
                Initialize: encoder_init_c,
                InitializeExt: encoder_init_ext_c,
                GetDefaultParams: encoder_get_default_c,
                Uninitialize: encoder_uninit_c,
                EncodeFrame: encoder_encode_frame_c,
                EncodeParameterSets: encoder_encode_param_c,
                ForceIntraFrame: encoder_force_intra_c,
                SetOption: encoder_set_opt_c,
                GetOption: encoder_get_opt_c,
            });
            let mut enc = Box::new(CWelsH264SVCEncoderImpl {
                base: ISVCEncoder {
                    lpVtbl: ptr::null(),
                },
                pVtbl: vtbl,
                inner: Encoder::new(),
            });
            enc.base.lpVtbl = &*enc.pVtbl as *const ISVCEncoderVtbl;
            *ppEncoder = Box::into_raw(enc) as *mut ISVCEncoder;
            CM_RESULT_SUCCESS
        })
    }
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroySVCEncoder(pEncoder: *mut ISVCEncoder) {
    abi_guard!(
        "WelsDestroySVCEncoder",
        unsafe { encoder_log(pEncoder) },
        (),
        {
            if !pEncoder.is_null() {
                unsafe {
                    drop(Box::from_raw(pEncoder as *mut CWelsH264SVCEncoderImpl));
                }
            }
        }
    )
}

#[allow(unsafe_code)]
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `ppDst` and `pDstInfo` as in [`decoder_decode_frame2_c`], including the
///   output window: a picture released from the reordering buffer is the
///   decoder's, valid until the next call on this decoder.
unsafe extern "C" fn decoder_flush_frame_c(
    this: *mut ISVCDecoder,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::FlushFrame",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            unsafe {
                // A caller that hands either out-parameter null gets the drain skipped
                // rather than a write through null.
                let (Some(ppDst), Some(pDstInfo)) =
                    ((ppDst as *mut [*mut u8; 3]).as_mut(), pDstInfo.as_mut())
                else {
                    return DECODING_STATE::dsErrorFree;
                };
                (*(this as *mut CWelsDecoderImpl))
                    .core
                    .flush(ppDst, pDstInfo)
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeParser` — `codec_api.h:511`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pSrc` / `iSrcLen` as in [`decoder_decode_frame2_c`]: null-or-zero is the
///   end-of-stream flush.
/// * `pDstInfo` must name a writable, aligned `SParserBsInfo`. It is an in-out
///   parameter: `uiInBsTimeStamp` is read, everything else is written.
/// * When `iNalNum > 0`, `pNalLenInByte` and `pDstBuff` point into this decoder's
///   parse-only buffers and are valid until the next call on this decoder — the plane
///   contract [`decoder_decode_frame2_c`] states, for bytes instead of planes.
unsafe extern "C" fn decoder_decode_parser_c(
    this: *mut ISVCDecoder,
    pSrc: *const u8,
    iSrcLen: i32,
    pDstInfo: *mut SParserBsInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeParser",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            let dec_impl = this as *mut CWelsDecoderImpl;
            unsafe {
                let src: Option<&[u8]> = if pSrc.is_null() || iSrcLen <= 0 {
                    None
                } else {
                    Some(std::slice::from_raw_parts(pSrc, iSrcLen as usize))
                };
                // A null `pDstInfo` is refused rather than written through.
                let Some(pDstInfo) = pDstInfo.as_mut() else {
                    return DECODING_STATE::dsInitialOptExpected;
                };
                (*dec_impl).core.decode_parser(src, pDstInfo)
            }
        }
    )
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateDecoder(ppDecoder: *mut *mut ISVCDecoder) -> c_long {
    unsafe {
        abi_guard!("WelsCreateDecoder", None, CM_MALLOC_MEM_ERROR as c_long, {
            if ppDecoder.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            let vtbl = Box::new(ISVCDecoderVtbl {
                Initialize: decoder_init_c,
                Uninitialize: decoder_uninit_c,
                DecodeFrame: decoder_decode_frame_c,
                DecodeFrameNoDelay: decoder_decode_frame_nodelay_c,
                DecodeFrame2: decoder_decode_frame2_c,
                FlushFrame: decoder_flush_frame_c,
                DecodeParser: decoder_decode_parser_c,
                DecodeFrameEx: decoder_decode_frame_ex_c,
                SetOption: decoder_set_opt_c,
                GetOption: decoder_get_opt_c,
            });
            let mut dec = Box::new(CWelsDecoderImpl {
                base: ISVCDecoder {
                    lpVtbl: ptr::null(),
                },
                pVtbl: vtbl,
                core: Decoder::new(),
            });
            dec.base.lpVtbl = &*dec.pVtbl as *const ISVCDecoderVtbl;
            let dec = Box::into_raw(dec);
            // Taken after the object has its final address. It is the `this = 0x…` of
            // every trace line and nothing else, which is why it travels as an address.
            (*dec).core.trace.SetCodecInstance(dec as usize);
            // The trace object's constructor sets `WELS_LOG_WARNING`, the encoder's
            // default; the decoder's own default is `WELS_LOG_ERROR`.
            (*dec)
                .core
                .trace
                .SetTraceLevel(crate::common::wels_trace::WELS_LOG_ERROR as u32);
            *ppDecoder = dec as *mut ISVCDecoder;
            CM_RESULT_SUCCESS as c_long
        })
    }
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsGetDecoderCapability(pDecCapability: *mut SDecoderCapability) -> i32 {
    abi_guard!("WelsGetDecoderCapability", None, CM_INIT_PARA_ERROR, {
        if pDecCapability.is_null() {
            return CM_INIT_PARA_ERROR;
        }
        unsafe {
            (*pDecCapability).iProfileIdc = 66;
            (*pDecCapability).iProfileIop = 0xE0;
            (*pDecCapability).iLevelIdc = 32;
            (*pDecCapability).iMaxMbps = 216000;
            (*pDecCapability).iMaxFs = 5120;
            (*pDecCapability).iMaxCpb = 20000;
            (*pDecCapability).iMaxDpb = 20480;
            (*pDecCapability).iMaxBr = 20000;
            (*pDecCapability).bRedPicCap = false;
        }
        CM_RESULT_SUCCESS
    })
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroyDecoder(pDecoder: *mut ISVCDecoder) {
    abi_guard!(
        "WelsDestroyDecoder",
        unsafe { decoder_log(pDecoder) },
        (),
        {
            if !pDecoder.is_null() {
                unsafe {
                    let dec_impl = pDecoder as *mut CWelsDecoderImpl;
                    // Order matters: the dynamic memory goes before the context does.
                    (*dec_impl).core.uninitialize();
                    drop(Box::from_raw(dec_impl));
                }
            }
        }
    )
}

#[cfg(test)]
pub(crate) mod abi_test_driver {
    use super::*;

    #[allow(unsafe_code)]
    /// Decodes `stream` through the C ABI and returns `(frames, last frame's
    /// dimensions, states)`, where `states` is the bitwise OR of every `DecodeFrame2`
    /// return.
    ///
    /// Calls the vtable thunks directly rather than the conveniences, which is what a
    /// C caller compiles to and exercises the slot table itself. `states` matters
    /// because a concealment path that does not run looks exactly like one that runs
    /// and changes nothing; `dsDataErrorConcealed` in the OR is the difference.
    pub(crate) fn drive_decoder_over(stream: &[u8]) -> (usize, Option<(i32, i32)>, i32) {
        // SAFETY: `WelsCreateDecoder` hands out `Box::into_raw(dec) as *mut
        // ISVCDecoder`, so the pointer carries provenance for the whole
        // implementation object and every call below is the sequence a C caller
        // makes.
        unsafe {
            {
                let mut p_decoder: *mut ISVCDecoder = ptr::null_mut();
                assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS as i64);
                assert!(!p_decoder.is_null());
                let vtbl = (*p_decoder).lpVtbl;

                let mut dec_param = SDecodingParam::default();
                dec_param.uiTargetDqLayer = u8::MAX;
                dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
                dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
                assert_eq!(
                    ((*vtbl).Initialize)(p_decoder, &dec_param as *const SDecodingParam),
                    CM_RESULT_SUCCESS as i64
                );

                let mut frames = 0;
                let mut dims = None;
                let mut states = 0i32;
                for unit in crate::split_annexb_units(stream) {
                    let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                    let mut buf_info = SBufferInfo::default();
                    let ret = ((*vtbl).DecodeFrame2)(
                        p_decoder,
                        unit.as_ptr(),
                        unit.len() as i32,
                        p_dst.as_mut_ptr(),
                        &mut buf_info,
                    );
                    states |= ret.0;
                    if buf_info.iBufferStatus == 1 {
                        frames += 1;
                        let sys = *buf_info.UsrData.sys();
                        dims = Some((sys.iWidth, sys.iHeight));
                    }
                }

                // End of stream, then the zero-length call that flushes it. Without it a
                // stream whose only frame arrives there looks like one that decodes
                // nothing.
                let mut eos_flag = 1i32;
                ((*vtbl).SetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                    &mut eos_flag as *mut i32 as *mut c_void,
                );
                let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let ret = ((*vtbl).DecodeFrame2)(
                    p_decoder,
                    ptr::null(),
                    0,
                    p_dst.as_mut_ptr(),
                    &mut buf_info,
                );
                states |= ret.0;
                if buf_info.iBufferStatus == 1 {
                    frames += 1;
                    let sys = *buf_info.UsrData.sys();
                    dims = Some((sys.iWidth, sys.iHeight));
                }

                // …and the drain the flush announces. Leaving it out costs a frame on
                // every stream whose last picture is still buffered at EOS.
                let mut remaining = 0i32;
                ((*vtbl).GetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                    &mut remaining as *mut i32 as *mut c_void,
                );
                for _ in 0..remaining.clamp(0, 24) {
                    let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                    let mut buf_info = SBufferInfo::default();
                    let ret = ((*vtbl).FlushFrame)(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
                    states |= ret.0;
                    if buf_info.iBufferStatus == 1 {
                        frames += 1;
                        let sys = *buf_info.UsrData.sys();
                        dims = Some((sys.iWidth, sys.iHeight));
                    }
                }

                ((*vtbl).Uninitialize)(p_decoder);
                WelsDestroyDecoder(p_decoder);
                (frames, dims, states)
            }
        }
    }

    /// One encoded frame, as the probe sees it: what the encoder called the frame,
    /// how many bytes of NAL it produced, and how many NALs those bytes came in.
    ///
    /// A frame's slices are exactly the NALs of its `VIDEO_CODING_LAYER` layers
    /// (`uiLayerType`), so `vcl_nals` is the coded slice count — which a count over
    /// every layer would not be, since an IDR also carries its parameter sets in a
    /// `NON_VIDEO_CODING_LAYER`.
    pub(crate) struct EncodedFrame {
        pub(crate) kind: EVideoFrameType,
        pub(crate) bytes: usize,
        pub(crate) vcl_nals: usize,
        pub(crate) frame_size: i32,
        /// `first_mb_in_slice` of every VCL NAL of this frame, in emission order.
        ///
        /// Read straight off the slice header as `ue(v)`; the value cannot need
        /// two zero bytes of prefix at any picture size this crate's probes use,
        /// so emulation prevention never falls inside it.
        pub(crate) first_mbs: Vec<u32>,
    }

    /// Fills `buf` with frame `f` of a synthetic I420 sequence that moves.
    ///
    /// Two motions at different velocities. The whole picture translates by (2, 1)
    /// samples per frame, so every macroblock has a non-zero motion vector; a bright
    /// block crosses it at (3, 2), so the macroblocks it touches disagree with their
    /// neighbours and the predicted vector is wrong for them. One velocity everywhere
    /// would run the search and then code `mvd = 0` at every macroblock but the first.
    ///
    /// The texture is a xor/quotient pattern rather than a gradient because a
    /// translated gradient is a gradient plus a constant, so the search would answer
    /// (0, 0) with a DC residual and nothing about motion estimation is measured.
    fn moving_i420(width: i32, height: i32, f: usize, buf: &mut [u8]) {
        fn texture(u: i32, v: i32) -> u8 {
            let a = (u.wrapping_mul(3) ^ v.wrapping_mul(5)) as u32;
            let b = (u / 7)
                .wrapping_mul(37)
                .wrapping_add((v / 5).wrapping_mul(53)) as u32;
            (16 + ((a ^ b) & 0x7f)) as u8
        }
        let (w, h) = (width as usize, height as usize);
        let (dx, dy) = (2 * f as i32, f as i32);
        // The bright block's own track, wrapped so it stays inside the picture.
        let (bw, bh) = (20.min(w / 2) as i32, 12.min(h / 2) as i32);
        let bx = (3 * f as i32) % (width - bw).max(1);
        let by = (2 * f as i32) % (height - bh).max(1);

        for y in 0..h {
            for x in 0..w {
                let inside = (x as i32) >= bx
                    && (x as i32) < bx + bw
                    && (y as i32) >= by
                    && (y as i32) < by + bh;
                buf[y * w + x] = if inside {
                    235
                } else {
                    texture(x as i32 + dx, y as i32 + dy)
                };
            }
        }
        let luma = w * h;
        let (cw, ch) = (w / 2, h / 2);
        for y in 0..ch {
            for x in 0..cw {
                let t = texture(2 * x as i32 + dx, 2 * y as i32 + dy);
                buf[luma + y * cw + x] = 112u8.wrapping_add(t & 0x1f);
                buf[luma + cw * ch + y * cw + x] = 144u8.wrapping_sub(t & 0x1f);
            }
        }
    }

    /// The configuration knobs the encoder probes vary.
    ///
    /// Each of these selects a different body of code, not a different parameter
    /// value:
    ///
    /// * `cabac` picks the entropy writers — `svc_set_mb_syn_cabac.rs` or
    ///   `svc_set_mb_syn_cavlc.rs`.
    /// * `complexity` picks the mode-decision family: `LOW_COMPLEXITY` installs
    ///   `SetFastCodingFunc` (`encoder_ext.rs:2485`, `bFastMode`) and anything else
    ///   runs the fine intra partition search (`WelsMdIntraFinePartition`,
    ///   `WelsMdI4x4`) and the `pMemPredBlk4` ping-pong.
    ///
    /// * `slice_mode`/`slice_constraint` pick the slicing machinery.
    ///   `SM_SIZELIMITED_SLICE` is the only encode path with a loop of
    ///   its own (`WelsMdInterMbLoopOverDynamicSlice`), the only caller of the
    ///   CAVLC/CABAC stash-and-rollback pair (`StashMBStatus`/`StashPopMBStatus`)
    ///   and of `pDynamicBsBuffer`, and the only reader of
    ///   `CalculateNewSliceNum` → `ReallocSliceBuffer` → `ExtendLayerBuffer` →
    ///   `ReOrderSliceInLayer`. `slice_constraint` is `uiSliceSizeConstraint` in
    ///   bytes and is ignored by every other mode; validation refuses anything
    ///   ≤ `MAX_MACROBLOCK_SIZE_IN_BYTE` (400), and a slice closes at
    ///   `constraint - AVER_MARGIN_BYTES` (100) bytes of payload.
    #[derive(Debug, Copy, Clone)]
    pub(crate) struct EncoderProbeOptions {
        pub cabac: bool,
        pub complexity: ECOMPLEXITY_MODE,
        pub slice_mode: SliceModeEnum,
        pub slice_constraint: u32,
        /// `iMultipleThreadIdc`. Above 1 the encode takes the fork/join path;
        /// `bUseLoadBalancing` is forced off below, so it stays byte-deterministic.
        pub threads: u16,
        /// `uiSliceNum` for `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE`.
        pub slice_num: u32,
        /// `bUseLoadBalancing`. Default `false`, and every byte-asserting probe must
        /// leave it there: with it on, and `iMultipleThreadIdc >= uiSliceNum`, frame
        /// N+1's slice boundaries are a function of frame N's measured encode times, so
        /// the bitstream stops being a function of the input. `GetDefaultParams` sets it
        /// on, which is why the field is forced here rather than inherited.
        pub load_balancing: bool,
    }

    impl Default for EncoderProbeOptions {
        fn default() -> Self {
            Self {
                cabac: true,
                complexity: ECOMPLEXITY_MODE::LOW_COMPLEXITY,
                slice_mode: SliceModeEnum::SM_SINGLE_SLICE,
                slice_constraint: 0,
                threads: 1,
                slice_num: 1,
                load_balancing: false,
            }
        }
    }

    /// `first_mb_in_slice` of one Annex-B VCL NAL — the leading `ue(v)` of the
    /// slice header, and nothing else of it.
    ///
    /// Returns `None` for a NAL with no start code or no payload byte, which is
    /// what a caller should see rather than a panic: the counts beside it are
    /// still meaningful.
    fn first_mb_in_slice(nal: &[u8]) -> Option<u32> {
        // Skip the start code (3 or 4 bytes) and the one-byte NAL header.
        let body = if nal.starts_with(&[0, 0, 0, 1]) {
            &nal[4..]
        } else if nal.starts_with(&[0, 0, 1]) {
            &nal[3..]
        } else {
            nal
        };
        // Refuse rather than guess on the SVC extension types. `eNalUnitType` 14
        // (`NAL_UNIT_PREFIX`) and 20 (`NAL_UNIT_CODED_SLICE_EXT`) carry a three-byte
        // `SNalUnitHeaderExt` between the NAL header and the slice header, so skipping
        // one byte would read the extension as a `ue(v)` and return a plausible wrong
        // number.
        let nal_type = body.first()? & 0x1F;
        if nal_type == 14 || nal_type == 20 {
            return None;
        }
        let rbsp = body.get(1..)?;
        // `ue(v)`: count leading zero bits, then read that many more.
        let bit =
            |i: usize| -> Option<u32> { Some(((*rbsp.get(i / 8)? >> (7 - (i % 8))) & 1) as u32) };
        let mut lead = 0usize;
        while bit(lead)? == 0 {
            lead += 1;
            // 32 leading zeros is not a slice header; refuse rather than loop.
            if lead > 31 {
                return None;
            }
        }
        let mut v: u32 = 1;
        for k in 1..=lead {
            v = (v << 1) | bit(lead + k)?;
        }
        Some(v - 1)
    }

    #[allow(unsafe_code)]
    /// Encodes `frames` frames of [`moving_i420`] at `width` x `height` through the
    /// C ABI, and returns what came out frame by frame together with the encoder's
    /// own report of the resolution it is configured for.
    ///
    /// Calls the vtable thunks directly rather than the conveniences, for the reason
    /// [`drive_decoder_over`] gives.
    ///
    /// Three settings are fixed for the determinism the probe's assertions rest on:
    /// scene-change detection off (a detected cut would make frame 1 an IDR and there
    /// would be no inter frame), frame skip off (a skipped frame emits no NAL), and
    /// `uiIntraPeriod = 0` (no periodic IDR). The profile is `PRO_HIGH`, because a
    /// baseline layer forces CAVLC and the probe has to be able to ask for either
    /// writer; entropy coding and complexity come from [`EncoderProbeOptions`] and
    /// default to CABAC over `LOW_COMPLEXITY`.
    pub(crate) fn drive_encoder_over(
        width: i32,
        height: i32,
        frames: usize,
        opts: EncoderProbeOptions,
    ) -> (Vec<EncodedFrame>, (i32, i32)) {
        assert!(
            width % 16 == 0 && height % 16 == 0 && width >= 16 && height >= 16,
            "the driver synthesises whole macroblocks: {width}x{height} is not one"
        );
        // SAFETY: `WelsCreateSVCEncoder` hands out `Box::into_raw(enc) as *mut
        // ISVCEncoder`, so the pointer carries provenance for the whole
        // implementation object, and every call below is the sequence a C caller
        // makes.
        unsafe {
            let mut p_encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut p_encoder), CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());
            let vtbl = (*p_encoder).lpVtbl;

            let mut param = SEncParamExt::default();
            assert_eq!(
                ((*vtbl).GetDefaultParams)(p_encoder, &mut param),
                CM_RESULT_SUCCESS
            );
            param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            param.iPicWidth = width;
            param.iPicHeight = height;
            param.iTargetBitrate = 500_000;
            param.iMaxBitrate = UNSPECIFIED_BIT_RATE;
            param.iRCMode = RC_MODES::RC_QUALITY_MODE;
            param.fMaxFrameRate = 30.0;
            param.iTemporalLayerNum = 1;
            param.iSpatialLayerNum = 1;
            param.iComplexityMode = opts.complexity;
            param.uiIntraPeriod = 0;
            param.iNumRefFrame = AUTO_REF_PIC_COUNT;
            param.eSpsPpsIdStrategy = EParameterSetStrategy::CONSTANT_ID;
            param.iEntropyCodingModeFlag = if opts.cabac { 1 } else { 0 };
            param.bEnableFrameSkip = false;
            param.iMaxQp = 51;
            param.iMinQp = 0;
            param.iMultipleThreadIdc = opts.threads;
            // Off unless a probe asks for it. With it on and
            // `iMultipleThreadIdc >= uiSliceNum` the encoder takes `AdjustBaseLayer`
            // -> `DynamicAdjustSlicing`, whose slice boundaries for frame N+1 come
            // from frame N's measured encode times — so the bitstream stops being a
            // function of the input and any byte assertion stops meaning anything.
            // `GetDefaultParams` turns it on, so this line is a force, not a default.
            param.bUseLoadBalancing = opts.load_balancing;
            param.bEnableDenoise = false;
            param.bEnableBackgroundDetection = false;
            param.bEnableAdaptiveQuant = false;
            param.bEnableSceneChangeDetect = false;
            param.bEnableLongTermReference = false;
            param.bEnableFrameCroppingFlag = true;
            param.iLoopFilterDisableIdc = 0;
            param.sSpatialLayers[0].uiProfileIdc = EProfileIdc::PRO_HIGH;
            param.sSpatialLayers[0].uiLevelIdc = ELevelIdc::LEVEL_UNKNOWN;
            param.sSpatialLayers[0].iVideoWidth = width;
            param.sSpatialLayers[0].iVideoHeight = height;
            param.sSpatialLayers[0].fFrameRate = 30.0;
            param.sSpatialLayers[0].iSpatialBitrate = 500_000;
            param.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
            param.sSpatialLayers[0].sSliceArgument.uiSliceMode = opts.slice_mode;
            param.sSpatialLayers[0].sSliceArgument.uiSliceNum = opts.slice_num;
            param.sSpatialLayers[0].sSliceArgument.uiSliceSizeConstraint = opts.slice_constraint;
            assert_eq!(
                ((*vtbl).InitializeExt)(p_encoder, &param as *const SEncParamExt),
                CM_RESULT_SUCCESS
            );

            // The encoder's own answer for the geometry it is configured for, rather
            // than the geometry we asked for: the grid assertion has to be the
            // encoder's report or it asserts the test's own argument.
            let mut effective = SEncParamExt::default();
            assert_eq!(
                ((*vtbl).GetOption)(
                    p_encoder,
                    ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
                    &mut effective as *mut SEncParamExt as *mut c_void,
                ),
                CM_RESULT_SUCCESS
            );

            let luma = (width * height) as usize;
            let mut buf = vec![0u8; luma * 3 / 2];
            let mut out = Vec::with_capacity(frames);
            for f in 0..frames {
                moving_i420(width, height, f, &mut buf);
                // One derivation for all three planes. Three `as_mut_ptr()` calls
                // would each retag and pop the previous one.
                let base = buf.as_mut_ptr();
                let mut pic = SSourcePicture::default();
                pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
                pic.iPicWidth = width;
                pic.iPicHeight = height;
                pic.iStride[0] = width;
                pic.iStride[1] = width / 2;
                pic.iStride[2] = width / 2;
                pic.pData[0] = base;
                pic.pData[1] = base.add(luma);
                pic.pData[2] = base.add(luma + luma / 4);
                pic.uiTimeStamp = (f as i64) * 1000 / 30;

                let mut info = SFrameBSInfo::default();
                assert_eq!(
                    ((*vtbl).EncodeFrame)(p_encoder, &pic as *const SSourcePicture, &mut info),
                    CM_RESULT_SUCCESS,
                    "EncodeFrame failed at frame {f}"
                );
                let mut bytes = 0usize;
                let mut vcl_nals = 0usize;
                let mut first_mbs: Vec<u32> = Vec::new();
                for l in 0..info.iLayerNum as usize {
                    let lay = &info.sLayerInfo[l];
                    if lay.pNalLengthInByte.is_null() {
                        continue;
                    }
                    let is_vcl = lay.uiLayerType == LAYER_TYPE::VIDEO_CODING_LAYER as u8;
                    if is_vcl {
                        vcl_nals += lay.iNalCount as usize;
                    }
                    let mut at = 0usize;
                    for n in 0..lay.iNalCount as usize {
                        let len = *lay.pNalLengthInByte.add(n) as usize;
                        bytes += len;
                        if is_vcl && !lay.pBsBuf.is_null() {
                            let nal = std::slice::from_raw_parts(lay.pBsBuf.add(at), len);
                            if let Some(v) = first_mb_in_slice(nal) {
                                first_mbs.push(v);
                            }
                        }
                        at += len;
                    }
                }
                out.push(EncodedFrame {
                    kind: info.eFrameType,
                    bytes,
                    vcl_nals,
                    frame_size: info.iFrameSizeInBytes,
                    first_mbs,
                });
            }

            ((*vtbl).Uninitialize)(p_encoder);
            WelsDestroySVCEncoder(p_encoder);
            (out, (effective.iPicWidth, effective.iPicHeight))
        }
    }
}

// ============================================================================
// Boundary provenance covering test
// ============================================================================

#[cfg(test)]
mod f23_boundary_provenance {
    use super::*;

    #[allow(unsafe_code)]
    /// The consumer conveniences on `ISVCDecoder`/`ISVCEncoder` take `this` as a raw
    /// pointer rather than `&mut self`: those two structs are one pointer wide, while
    /// the thunk behind every slot casts `this` to a pointer to `CWelsDecoderImpl` /
    /// `CWelsH264SVCEncoderImpl` and writes the implementation object past those eight
    /// bytes — `decoder_init_c` writes at offset `0x20`.
    ///
    /// Miri is the assertion. What this must keep doing is calling a convenience, on
    /// both codecs, so that a re-introduced `&mut self` receiver is caught by the
    /// checker.
    #[test]
    fn conveniences_call_through_the_whole_impl_allocation() {
        unsafe {
            // --- the decoder half -------------------------------------------
            let mut p_decoder = ptr::null_mut();
            assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;
            dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
            dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
            // The write that is out of bounds for an eight-byte borrow: this call
            // stores `*pParam` into `CWelsDecoderImpl::param`.
            assert_eq!(
                ISVCDecoder::Initialize(p_decoder, &dec_param),
                CM_RESULT_SUCCESS as i64
            );

            // `bEndOfStream` lives further out still, and this pair writes then reads
            // it — so the probe covers a round trip and not only the init.
            let mut eos = 1i32;
            ISVCDecoder::SetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                ptr::from_mut(&mut eos).cast(),
            );
            let mut eos_back = 0i32;
            ISVCDecoder::GetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                ptr::from_mut(&mut eos_back).cast(),
            );
            assert_eq!(eos_back, 1, "END_OF_STREAM did not round-trip");

            assert_eq!(
                ISVCDecoder::Uninitialize(p_decoder),
                CM_RESULT_SUCCESS as i64
            );
            WelsDestroyDecoder(p_decoder);

            // --- the encoder half -------------------------------------------
            let mut p_encoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut p_encoder), CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());

            let mut enc_param = SEncParamBase::default();
            enc_param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            enc_param.iPicWidth = 64;
            enc_param.iPicHeight = 64;
            enc_param.fMaxFrameRate = 30.0;
            enc_param.iTargetBitrate = 64000;
            assert_eq!(
                ISVCEncoder::Initialize(p_encoder, &enc_param),
                CM_RESULT_SUCCESS
            );
            assert_eq!(ISVCEncoder::Uninitialize(p_encoder), CM_RESULT_SUCCESS);
            WelsDestroySVCEncoder(p_encoder);
        }
    }
}

// ===========================================================================
// The cores' `Send` verdict.
// ===========================================================================

#[cfg(test)]
mod send_verdict {
    use super::*;

    /// `true` iff the named type is `Send`, decided at compile time and reported rather
    /// than enforced.
    ///
    /// A plain `fn assert_send<T: Send>() {}` states a verdict only when the verdict is
    /// yes; when it is no the file stops compiling. This is the
    /// inherent-method-beats-trait-method trick, and it has to be a macro rather than a
    /// generic function: inside `fn is_send<T>()` the method resolves at definition
    /// time, where `T` is not known to be `Send`, so the answer would be `false` for
    /// everything. Expanded at a concrete type it resolves at the use site.
    macro_rules! is_send {
        ($t:ty) => {{
            struct Probe<T>(std::marker::PhantomData<T>);
            // Each expansion uses exactly one of the two arms and leaves the other
            // dead, so both carry the allow: the trait arm is dead when `$t` is `Send`,
            // the inherent one when it is not.
            #[allow(dead_code)]
            trait NotSend {
                fn probe(&self) -> bool {
                    false
                }
            }
            impl<T> NotSend for Probe<T> {}
            impl<T: Send> Probe<T> {
                #[allow(dead_code)]
                fn probe(&self) -> bool {
                    true
                }
            }
            Probe::<$t>(std::marker::PhantomData).probe()
        }};
    }

    /// Neither core is `Send`, for the same reason: `SWelsDecoderContext` and
    /// `sWelsEncCtx` each carry `*mut`/`*const` members below the boundary, and a raw
    /// pointer is `!Send` by construction, so `Decoder` and `Encoder` are too.
    ///
    /// The encoder's own threading does not depend on this: it forks onto its worker
    /// pool through a `scope` shaped like `std::thread::scope`, and the workers take
    /// what they reach by static partition. `Send` on the whole encoder is the property
    /// a consumer would need to move a codec between threads.
    #[test]
    fn the_cores_are_not_send_yet_and_this_is_the_inventory() {
        assert!(
            !is_send!(Decoder),
            "Decoder became Send — rewrite the verdict in this test and in the \
             session log, with what changed"
        );
        assert!(
            !is_send!(Encoder),
            "Encoder became Send — rewrite the verdict in this test and in the \
             session log, with what changed"
        );
        // The probe itself has to be able to say yes, or the assertions above are
        // vacuous.
        assert!(is_send!(u32), "the Send probe reports false for u32");
        assert!(
            !is_send!(*mut u8),
            "the Send probe reports true for a raw pointer"
        );
    }
}

// ===========================================================================
// The panic guard's covering tests.
// ===========================================================================

#[cfg(test)]
mod abi_panic_guard {
    use super::*;

    #[allow(unsafe_code)]
    /// A panic inside a decoder entry comes back as that entry's failure code, and
    /// the process is still here to assert it.
    ///
    /// Without `abi_guard!` this would be a non-unwinding panic and a SIGABRT that
    /// takes every other test in the binary with it.
    #[test]
    fn a_panic_inside_a_decoder_thunk_becomes_dsbitstreamerror() {
        unsafe {
            let mut decoder: *mut ISVCDecoder = ptr::null_mut();
            assert_eq!(WelsCreateDecoder(&mut decoder), CM_RESULT_SUCCESS as i64);
            let param = SDecodingParam {
                uiTargetDqLayer: u8::MAX,
                ..SDecodingParam::default()
            };
            assert_eq!(
                ISVCDecoder::Initialize(decoder, &param as *const SDecodingParam),
                CM_RESULT_SUCCESS as i64
            );

            let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let bytes = [0u8, 0, 0, 1, 0x67];

            PANIC_PROBE.with(|p| p.set(PROBE_DECODE_FRAME2));
            let state = ISVCDecoder::DecodeFrame2(
                decoder,
                bytes.as_ptr(),
                bytes.len() as i32,
                p_dst.as_mut_ptr(),
                &mut info,
            );
            PANIC_PROBE.with(|p| p.set(0));

            assert_eq!(
                state,
                DECODING_STATE::dsBitstreamError,
                "a caught panic must be reported as this slot's failure code"
            );

            // Alive, and the object is still usable enough to tear down — the narrow
            // half of the `AssertUnwindSafe` claim.
            ISVCDecoder::Uninitialize(decoder);
            WelsDestroyDecoder(decoder);
        }
    }

    #[allow(unsafe_code)]
    /// The encoder half: `cmUnknownReason`, which is what an encode entry has for
    /// "something went wrong and it was not your parameters".
    #[test]
    fn a_panic_inside_an_encoder_thunk_becomes_cm_unknown_reason() {
        unsafe {
            let mut encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut encoder), CM_RESULT_SUCCESS);
            let mut base = SEncParamBase {
                iPicWidth: 176,
                iPicHeight: 144,
                iTargetBitrate: 128_000,
                fMaxFrameRate: 30.0,
                ..SEncParamBase::default()
            };
            assert_eq!(
                ISVCEncoder::Initialize(encoder, &mut base as *const SEncParamBase),
                CM_RESULT_SUCCESS
            );

            let mut plane = vec![0u8; 176 * 144 * 3 / 2];
            let mut pic = SSourcePicture::default();
            pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            pic.iPicWidth = 176;
            pic.iPicHeight = 144;
            pic.iStride = [176, 88, 88, 0];
            pic.pData = [
                plane.as_mut_ptr(),
                plane.as_mut_ptr().add(176 * 144),
                plane.as_mut_ptr().add(176 * 144 * 5 / 4),
                ptr::null_mut(),
            ];
            let mut bs = SFrameBSInfo::default();

            PANIC_PROBE.with(|p| p.set(PROBE_ENCODE_FRAME));
            let rc = ISVCEncoder::EncodeFrame(encoder, &pic as *const SSourcePicture, &mut bs);
            PANIC_PROBE.with(|p| p.set(0));

            assert_eq!(
                rc, CM_UNKNOWN_REASON,
                "a caught panic must be reported as this slot's failure code"
            );

            ISVCEncoder::Uninitialize(encoder);
            WelsDestroySVCEncoder(encoder);
        }
    }

    /// The probe is this thread's, so a thunk called from another thread runs the real
    /// body and the switch cannot fire in tests running in parallel with these two.
    #[test]
    fn the_probe_does_not_leak_to_other_threads() {
        PANIC_PROBE.with(|p| p.set(PROBE_DECODE_FRAME2));
        let seen = std::thread::spawn(|| PANIC_PROBE.with(|p| p.get()))
            .join()
            .unwrap();
        PANIC_PROBE.with(|p| p.set(0));
        assert_eq!(seen, 0);
    }
}
