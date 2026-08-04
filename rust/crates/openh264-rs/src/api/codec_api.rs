//! OpenH264 Public C/C++ API Architecture (`codec_api.h`).
//!
//! Provides the public API interface definitions, C-compatible vtables,
//! dynamic library export bindings, versioning structures, and factory lifecycles
//! for both the H.264 / SVC video encoder (`ISVCEncoder`) and decoder (`ISVCDecoder`).

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

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

/// Decoding status bitmask / enumeration (`DECODING_STATE`).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum DECODING_STATE {
    #[default]
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
            pData: [std::ptr::null_mut(); 4],
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
            pNalLengthInByte: std::ptr::null_mut(),
            pBsBuf: std::ptr::null_mut(),
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
            pFileNameRestructed: std::ptr::null_mut(),
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
            pDst: [std::ptr::null_mut(); 3],
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
            pNalLenInByte: std::ptr::null_mut(),
            pDstBuff: std::ptr::null_mut(),
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

/// Logging trace callback prototype (`WelsTraceCallback`).
pub type WelsTraceCallback =
    Option<unsafe extern "C" fn(ctx: *mut c_void, level: i32, string: *const c_char)>;

// ============================================================================
// C/C++ Virtual Function Tables & Interface Definitions
// ============================================================================

/// C-compatible virtual function table for `ISVCEncoder`.
#[repr(C)]
pub struct ISVCEncoderVtbl {
    pub Initialize: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        pParam: *const SEncParamBase,
    ) -> i32,
    pub InitializeExt: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        pParam: *const SEncParamExt,
    ) -> i32,
    pub GetDefaultParams: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        pParam: *mut SEncParamExt,
    ) -> i32,
    pub Uninitialize: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
    ) -> i32,
    pub EncodeFrame: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32,
    pub EncodeParameterSets: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32,
    pub ForceIntraFrame: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        bIDR: bool,
    ) -> i32,
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

impl ISVCEncoder {
    /// Initializes the encoder with basic parameters.
    #[inline]
    pub unsafe fn Initialize(&mut self, pParam: *const SEncParamBase) -> i32 {
        unsafe { ((*self.lpVtbl).Initialize)(self, pParam) }
    }

    /// Initializes the encoder with extended SVC parameters.
    #[inline]
    pub unsafe fn InitializeExt(&mut self, pParam: *const SEncParamExt) -> i32 {
        unsafe { ((*self.lpVtbl).InitializeExt)(self, pParam) }
    }

    /// Retrieves default extension encoding parameters.
    #[inline]
    pub unsafe fn GetDefaultParams(&mut self, pParam: *mut SEncParamExt) -> i32 {
        unsafe { ((*self.lpVtbl).GetDefaultParams)(self, pParam) }
    }

    /// Uninitializes and frees encoder session resources.
    #[inline]
    pub unsafe fn Uninitialize(&mut self) -> i32 {
        unsafe { ((*self.lpVtbl).Uninitialize)(self) }
    }

    /// Encodes a single uncompressed frame.
    #[inline]
    pub unsafe fn EncodeFrame(
        &mut self,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32 {
        unsafe { ((*self.lpVtbl).EncodeFrame)(self, kpSrcPic, pBsInfo) }
    }

    /// Serializes out-of-band parameter sets (SPS/PPS).
    #[inline]
    pub unsafe fn EncodeParameterSets(&mut self, pBsInfo: *mut SFrameBSInfo) -> i32 {
        unsafe { ((*self.lpVtbl).EncodeParameterSets)(self, pBsInfo) }
    }

    /// Forces the next frame to be encoded as an IDR keyframe.
    #[inline]
    pub unsafe fn ForceIntraFrame(&mut self, bIDR: bool) -> i32 {
        unsafe { ((*self.lpVtbl).ForceIntraFrame)(self, bIDR) }
    }

    /// Sets runtime encoder option.
    #[inline]
    pub unsafe fn SetOption(&mut self, eOptionId: ENCODER_OPTION, pOption: *mut c_void) -> i32 {
        unsafe { ((*self.lpVtbl).SetOption)(self, eOptionId, pOption) }
    }

    /// Queries runtime encoder option.
    #[inline]
    pub unsafe fn GetOption(&mut self, eOptionId: ENCODER_OPTION, pOption: *mut c_void) -> i32 {
        unsafe { ((*self.lpVtbl).GetOption)(self, eOptionId, pOption) }
    }
}

/// C-compatible virtual function table for `ISVCDecoder`.
#[repr(C)]
pub struct ISVCDecoderVtbl {
    pub Initialize: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pParam: *const SDecodingParam,
    ) -> c_long,
    pub Uninitialize: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
    ) -> c_long,
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

impl ISVCDecoder {
    /// Initializes the decoder context.
    #[inline]
    pub unsafe fn Initialize(&mut self, pParam: *const SDecodingParam) -> c_long {
        unsafe { ((*self.lpVtbl).Initialize)(self, pParam) }
    }

    /// Uninitializes the decoder context.
    #[inline]
    pub unsafe fn Uninitialize(&mut self) -> c_long {
        unsafe { ((*self.lpVtbl).Uninitialize)(self) }
    }

    /// Decodes a single frame.
    #[inline]
    pub unsafe fn DecodeFrame(
        &mut self,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pStride: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
    ) -> DECODING_STATE {
        unsafe { ((*self.lpVtbl).DecodeFrame)(self, pSrc, iSrcLen, ppDst, pStride, iWidth, iHeight) }
    }

    /// Zero-latency frame decoding (recommended real-time decoder API).
    #[inline]
    pub unsafe fn DecodeFrameNoDelay(
        &mut self,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*self.lpVtbl).DecodeFrameNoDelay)(self, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Multi-slice frame assembly decoding entry point.
    #[inline]
    pub unsafe fn DecodeFrame2(
        &mut self,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*self.lpVtbl).DecodeFrame2)(self, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Flushes remaining decoded reference frames from the DPB.
    #[inline]
    pub unsafe fn FlushFrame(
        &mut self,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*self.lpVtbl).FlushFrame)(self, ppDst, pDstInfo) }
    }

    /// Parses input bitstream headers only without pixel reconstruction.
    #[inline]
    pub unsafe fn DecodeParser(
        &mut self,
        pSrc: *const u8,
        iSrcLen: i32,
        pDstInfo: *mut SParserBsInfo,
    ) -> DECODING_STATE {
        unsafe { ((*self.lpVtbl).DecodeParser)(self, pSrc, iSrcLen, pDstInfo) }
    }

    /// Decodes to arbitrary destination format buffer.
    #[inline]
    pub unsafe fn DecodeFrameEx(
        &mut self,
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
            ((*self.lpVtbl).DecodeFrameEx)(
                self,
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

    /// Sets runtime decoder option.
    #[inline]
    pub unsafe fn SetOption(&mut self, eOptionId: DECODER_OPTION, pOption: *mut c_void) -> c_long {
        unsafe { ((*self.lpVtbl).SetOption)(self, eOptionId, pOption) }
    }

    /// Queries runtime decoder option.
    #[inline]
    pub unsafe fn GetOption(&mut self, eOptionId: DECODER_OPTION, pOption: *mut c_void) -> c_long {
        unsafe { ((*self.lpVtbl).GetOption)(self, eOptionId, pOption) }
    }
}

// ============================================================================
// Global Dynamic Library Export Lifecycle Bindings
// ============================================================================

#[repr(C)]
pub struct CWelsH264SVCEncoderImpl {
    pub base: ISVCEncoder,
    pub pVtbl: Box<ISVCEncoderVtbl>,
    pub inner: crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder,
}

#[repr(C)]
pub struct CWelsDecoderImpl {
    pub base: ISVCDecoder,
    pub pVtbl: Box<ISVCDecoderVtbl>,
    pub pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    pub align: crate::common::CMemoryAlign,
    pub param: crate::SDecodingParam,
    pub bEndOfStream: bool,
}

unsafe extern "C" fn encoder_init_c(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.Initialize(pParam as *const crate::SEncParamBase)
    }
}

unsafe extern "C" fn encoder_init_ext_c(this: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.InitializeExt(pParam as *const crate::SEncParamExt)
    }
}

unsafe extern "C" fn encoder_get_default_c(this: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.GetDefaultParams(pParam as *mut crate::SEncParamExt)
    }
}

unsafe extern "C" fn encoder_uninit_c(this: *mut ISVCEncoder) -> i32 {
    if this.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.Uninitialize()
    }
}

unsafe extern "C" fn encoder_encode_frame_c(this: *mut ISVCEncoder, kpSrcPic: *const SSourcePicture, pBsInfo: *mut SFrameBSInfo) -> i32 {
    if this.is_null() || kpSrcPic.is_null() || pBsInfo.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.EncodeFrame(kpSrcPic as *const crate::SSourcePicture, pBsInfo as *mut crate::SFrameBSInfo)
    }
}

unsafe extern "C" fn encoder_encode_param_c(this: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32 {
    if this.is_null() || pBsInfo.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.EncodeParameterSets(pBsInfo as *mut crate::SFrameBSInfo)
    }
}

unsafe extern "C" fn encoder_force_intra_c(this: *mut ISVCEncoder, bIDR: bool) -> i32 {
    if this.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.ForceIntraFrame(bIDR, -1)
    }
}

unsafe extern "C" fn encoder_set_opt_c(this: *mut ISVCEncoder, eOptionId: ENCODER_OPTION, pOption: *mut c_void) -> i32 {
    if this.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.SetOption(eOptionId, pOption)
    }
}

unsafe extern "C" fn encoder_get_opt_c(this: *mut ISVCEncoder, eOptionId: ENCODER_OPTION, pOption: *mut c_void) -> i32 {
    if this.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.GetOption(eOptionId, pOption)
    }
}

unsafe extern "C" fn decoder_init_c(this: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR as c_long;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        (*dec_impl).param = *pParam;

        if (*dec_impl).pCtx.is_null() {
            let mut ctx_box: Box<crate::decoder::decoder_context::SWelsDecoderContext> = Box::default();
            ctx_box.pMemAlign = &mut (*dec_impl).align;
            ctx_box.pParam = &mut (*dec_impl).param as *mut _ as *mut _;
            let p_ctx = Box::into_raw(ctx_box);
            let ret = crate::decoder::decoder_core::WelsInitStaticMemory(p_ctx as *mut _);
            if ret != 0 {
                drop(Box::from_raw(p_ctx));
                return CM_INIT_PARA_ERROR as c_long;
            }
            (*dec_impl).pCtx = p_ctx as *mut _;
        }
    }
    CM_RESULT_SUCCESS as c_long
}

unsafe extern "C" fn decoder_uninit_c(this: *mut ISVCDecoder) -> c_long {
    if this.is_null() {
        return CM_INIT_PARA_ERROR as c_long;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        if !(*dec_impl).pCtx.is_null() {
            crate::decoder::decoder_core::WelsEndDecoder((*dec_impl).pCtx as *mut _);
            drop(Box::from_raw((*dec_impl).pCtx as *mut crate::decoder::decoder_context::SWelsDecoderContext));
            (*dec_impl).pCtx = ptr::null_mut();
        }
    }
    CM_RESULT_SUCCESS as c_long
}

unsafe extern "C" fn decoder_decode_frame_c(
    this: *mut ISVCDecoder,
    pSrc: *const u8,
    iSrcLen: i32,
    ppDst: *mut *mut u8,
    pStride: *mut i32,
    iWidth: *mut i32,
    iHeight: *mut i32,
) -> DECODING_STATE {
    let mut buf_info = SBufferInfo::default();
    let state = decoder_decode_frame2_c(this, pSrc, iSrcLen, ppDst, &mut buf_info);
    if buf_info.iBufferStatus == 1 {
        unsafe {
            if !pStride.is_null() {
                *pStride.offset(0) = buf_info.UsrData.sSystemBuffer.iStride[0];
                *pStride.offset(1) = buf_info.UsrData.sSystemBuffer.iStride[1];
            }
            if !iWidth.is_null() {
                *iWidth = buf_info.UsrData.sSystemBuffer.iWidth;
            }
            if !iHeight.is_null() {
                *iHeight = buf_info.UsrData.sSystemBuffer.iHeight;
            }
        }
    }
    state
}

unsafe extern "C" fn decoder_decode_frame_nodelay_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    decoder_decode_frame2_c(this, kpSrc, kiSrcLen, ppDst, pDstInfo)
}

unsafe extern "C" fn decoder_decode_frame2_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    if this.is_null() {
        return DECODING_STATE::dsInitialOptExpected;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        let p_ctx = (*dec_impl).pCtx;
        if p_ctx.is_null() {
            return DECODING_STATE::dsInitialOptExpected;
        }

        if !kpSrc.is_null() && kiSrcLen > 0 {
            (*p_ctx).bEndOfStreamFlag = false;
            crate::decoder::decoder_core::WelsDecodeBs(
                p_ctx as *mut _,
                kpSrc,
                kiSrcLen,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        } else if (*dec_impl).bEndOfStream || (*p_ctx).bEndOfStreamFlag || kpSrc.is_null() || kiSrcLen == 0 {
            (*p_ctx).bEndOfStreamFlag = true;
            crate::decoder::decoder_core::WelsDecodeBs(
                p_ctx as *mut _,
                kpSrc,
                0,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        }
    }
    DECODING_STATE::dsErrorFree
}

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
    DECODING_STATE::dsErrorFree
}

unsafe extern "C" fn decoder_set_opt_c(this: *mut ISVCDecoder, eOptionId: DECODER_OPTION, pOption: *mut c_void) -> c_long {
    if this.is_null() {
        return CM_INIT_PARA_ERROR as c_long;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        match eOptionId {
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                if !pOption.is_null() {
                    let val = *(pOption as *const i32);
                    (*dec_impl).bEndOfStream = val != 0;
                    if !(*dec_impl).pCtx.is_null() {
                        (*(*dec_impl).pCtx).bEndOfStreamFlag = val != 0;
                    }
                }
            }
            DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                if !pOption.is_null() && !(*dec_impl).pCtx.is_null() {
                    let val = *(pOption as *const ERROR_CON_IDC);
                    (*dec_impl).param.eEcActiveIdc = val;
                }
            }
            _ => {}
        }
    }
    CM_RESULT_SUCCESS as c_long
}

unsafe extern "C" fn decoder_get_opt_c(this: *mut ISVCDecoder, eOptionId: DECODER_OPTION, pOption: *mut c_void) -> c_long {
    if this.is_null() || pOption.is_null() {
        return CM_INIT_PARA_ERROR as c_long;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        match eOptionId {
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER => {
                *(pOption as *mut i32) = 0;
            }
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                *(pOption as *mut i32) = if (*dec_impl).bEndOfStream { 1 } else { 0 };
            }
            _ => {}
        }
    }
    CM_RESULT_SUCCESS as c_long
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateSVCEncoder(ppEncoder: *mut *mut ISVCEncoder) -> i32 {
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
        base: ISVCEncoder { lpVtbl: ptr::null() },
        pVtbl: vtbl,
        inner: crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder::new(),
    });
    enc.base.lpVtbl = &*enc.pVtbl as *const ISVCEncoderVtbl;
    *ppEncoder = Box::into_raw(enc) as *mut ISVCEncoder;
    CM_RESULT_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroySVCEncoder(pEncoder: *mut ISVCEncoder) {
    if !pEncoder.is_null() {
        unsafe {
            drop(Box::from_raw(pEncoder as *mut CWelsH264SVCEncoderImpl));
        }
    }
}

unsafe extern "C" fn decoder_flush_frame_c(this: *mut ISVCDecoder, ppDst: *mut *mut u8, pDstInfo: *mut SBufferInfo) -> DECODING_STATE {
    decoder_decode_frame2_c(this, ptr::null(), 0, ppDst, pDstInfo)
}

unsafe extern "C" fn decoder_decode_parser_c(_this: *mut ISVCDecoder, _pSrc: *const u8, _iSrcLen: i32, _pDstInfo: *mut SParserBsInfo) -> DECODING_STATE {
    DECODING_STATE::dsErrorFree
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateDecoder(ppDecoder: *mut *mut ISVCDecoder) -> c_long {
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
        base: ISVCDecoder { lpVtbl: ptr::null() },
        pVtbl: vtbl,
        pCtx: ptr::null_mut(),
        align: crate::common::CMemoryAlign::new(16),
        param: crate::SDecodingParam::default(),
        bEndOfStream: false,
    });
    dec.base.lpVtbl = &*dec.pVtbl as *const ISVCDecoderVtbl;
    *ppDecoder = Box::into_raw(dec) as *mut ISVCDecoder;
    CM_RESULT_SUCCESS as c_long
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsGetDecoderCapability(pDecCapability: *mut SDecoderCapability) -> i32 {
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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroyDecoder(pDecoder: *mut ISVCDecoder) {
    if !pDecoder.is_null() {
        unsafe {
            let dec_impl = pDecoder as *mut CWelsDecoderImpl;
            if !(*dec_impl).pCtx.is_null() {
                crate::decoder::decoder_core::WelsEndDecoder((*dec_impl).pCtx);
                drop(Box::from_raw((*dec_impl).pCtx));
                (*dec_impl).pCtx = ptr::null_mut();
            }
            drop(Box::from_raw(dec_impl));
        }
    }
}
