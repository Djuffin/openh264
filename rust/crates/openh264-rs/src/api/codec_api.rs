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
use crate::decoder::decoder_context::slice_header_of;

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
/// **A bitmask, not an enumeration — F46, T5.T1.** The C++ declares this as an
/// `enum` and then uses it as a flag set: the decoder accumulates into
/// `pCtx->iErrorCode` with `|=` and `DecodeFrame2WithCtx` hands the accumulator
/// back whole (`welsDecoderExt.cpp:892`, `return (DECODING_STATE)pDecContext->iErrorCode`).
/// `dsBitstreamError | dsDataErrorConcealed` = `0x24` is a value this API returns,
/// and it names no variant.
///
/// A Rust `enum` cannot hold `0x24` — the value is invalid and producing one is UB —
/// so the port used to collapse the accumulator to its first set bit in a fixed
/// priority order, which is how 71 of `narrow_16x16`'s 76 code mismatches happened:
/// every row where the C++ said "concealed, and the bitstream was damaged" the port
/// said only "damaged". A transparent newtype is the type the C++ actually has; the
/// `DECODING_STATE::dsErrorFree` spelling at every use site is unchanged.
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

    /// The set bits, named, in the C header's order — so a `{:?}` of a combined
    /// value reads as the C++ log does rather than as a number.
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

impl SBufferInfoUsrData {
    /// The union's one arm, as a value.
    ///
    /// **T5b.6, and it is why this lives in `api/`.** `SBufferInfo` is the drop-in
    /// ABI's own type and its `UsrData` is declared a union because upstream's header
    /// declares one (`codec_app_def.h`) — with exactly one member, so there is no
    /// discriminant question and never was. Reading a union field is `unsafe` wherever
    /// it is spelled, though, so the two spellings live here rather than in
    /// `src/decoder/`, which is the whole of what this accessor is for.
    #[allow(unsafe_code)] // the ABI union — one arm, so every read is the arm last written
    #[inline]
    pub fn sys(&self) -> &SSysMEMBuffer {
        // SAFETY: `SBufferInfoUsrData` declares exactly one variant.
        unsafe { &self.sSystemBuffer }
    }

    /// [`sys`](Self::sys)'s mutable form.
    #[allow(unsafe_code)] // the ABI union — one arm, so every read is the arm last written
    #[inline]
    pub fn sys_mut(&mut self) -> &mut SSysMEMBuffer {
        // SAFETY: `SBufferInfoUsrData` declares exactly one variant.
        unsafe { &mut self.sSystemBuffer }
    }
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

// **F23 — the receiver is a pointer, and it has to be** (T8.A3; the rule is
// `prompts/phase8.md` §1, S28 at the ABI layer).
//
// Every function in this block used to take `&mut self`. `ISVC{Enc,Dec}oder` is the
// C++ class's vtable slot and **nothing else** — eight bytes — while the thunk
// behind each slot casts `this` to the implementation object and writes fields far
// past those eight bytes. A reference receiver therefore hands the thunk a tag whose
// range is `[0x0..0x8]`, and the first write in `decoder_init_c` is at `0x20`: out
// of bounds for the borrow the call was made through, on the library's public entry
// point. `f23_boundary_provenance` is that six-line Miri report, kept as a test.
//
// So these are **associated functions taking `this` as a raw pointer**, not methods.
// The alternative the brief offered — methods on `CWelsDecoderImpl` /
// `CWelsH264SVCEncoderImpl` taking `&mut self` of the *whole* object — is equally
// sound and was rejected on what the callers read: all 104 of them hold a pointer
// to `ISVCDecoder` or to `ISVCEncoder` (that is what the factories hand out and what
// the ABI names), so option (b) would have made every one of them cast to
// an implementation type the public header does not mention, to call a method, on a
// struct they have no business naming. `ISVCDecoder::DecodeFrame2(p, ..)` needs no
// cast and no new type in the caller's line of sight.
//
// This is not a smaller borrow that could be widened: it is the difference between a
// reference and a pointer. Nothing here may take `&self` either — the eight bytes
// are the whole of the reference's provenance whichever way it is spelled.
//
// # Safety — the contract every function below shares
//
// `this` must be a pointer the matching factory returned (`WelsCreateSVCEncoder` /
// `WelsCreateDecoder`), not yet passed to its destroyer, and derived from the whole
// implementation allocation — which is what the factory's `Box::into_raw(..)`, cast
// to the interface type, produces. It must be non-null and unaliased for the call.
// Every pointer
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

    /// Sets runtime encoder option.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime encoder option.
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
    // F23's rule, its `# Safety` contract and why option (a) was chosen: the note
    // above `impl ISVCEncoder`. Both interfaces are the same eight bytes and the
    // same defect; the argument is written once.

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
            ((*(*this).lpVtbl).DecodeFrame)(
                this,
                pSrc,
                iSrcLen,
                ppDst,
                pStride,
                iWidth,
                iHeight,
            )
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

    /// Sets runtime decoder option.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime decoder option.
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
    // **T8.A5: `param` stood here and is deleted — F41.** It was the port's own
    // invention: `CWelsDecoder` has no parameter member in the reference, and the
    // block the decoder reads is the *context's*, allocated by `InitDecoderCtx`
    // (`welsDecoderExt.cpp:426`) and filled by `DecoderConfigParam`. Holding a copy
    // here and pointing `pCtx->pParam` at it gave the teardown's `bParseOnly` arm a
    // field that a second `Initialize` could rewrite without the context knowing,
    // and gave `SetOption` a block the C++ writes one level down.
    pub bEndOfStream: bool,
    // **T8.A7: the ten `CWelsDecoder` members stood here and are the context's now.**
    // They were wired into it by ten `addr_of_mut!` stamps mirroring
    // `InitDecoderCtx`, and read back through `api_alias`/`api_alias_mut` — F38's
    // discipline holding up an arrangement the port did not need. In the reference
    // they are `CWelsDecoder`'s because a **threaded** decoder shares one reordering
    // buffer, one statistics block and one vlc table across N contexts
    // (`welsDecoderExt.cpp:415-422` stamps all N); with one context per decoder,
    // which is what this port has, the context owns them and the stamps have nothing
    // to do. `m_pPicBuff` went with them by deletion rather than by moving — see
    // [`pool_for`] for why it is provably null here.
    //
    // What is left is what the api layer genuinely owns: the vtable it hands out,
    // the context pointer, and the end-of-stream flag `GetOption` reports.
}

unsafe extern "C" fn encoder_init_c(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.Initialize(pParam)
    }
}

unsafe extern "C" fn encoder_init_ext_c(this: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.InitializeExt(pParam)
    }
}

unsafe extern "C" fn encoder_get_default_c(this: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32 {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.GetDefaultParams(pParam)
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
        (*impl_ptr).inner.EncodeFrame(kpSrcPic, pBsInfo)
    }
}

unsafe extern "C" fn encoder_encode_param_c(this: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32 {
    if this.is_null() || pBsInfo.is_null() {
        return CM_INIT_PARA_ERROR;
    }
    unsafe {
        let impl_ptr = this as *mut CWelsH264SVCEncoderImpl;
        (*impl_ptr).inner.EncodeParameterSets(pBsInfo)
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

/// `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)`
/// — `decoder.cpp:654` and `welsDecoderExt.cpp:528`, one function.
///
/// **F76, T8.B1.** The clamp is the reference's answer to a field that is an `int`
/// on the wire and an eight-variant enum in this port. It has to run on the integer:
/// once the value is an `ERROR_CON_IDC` the question is already settled, badly.
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
///
/// The reference normalises on the way into `pCtx->eVideoType` and leaves the
/// caller's block alone. Here it is the caller's block that is normalised, which is
/// the same store because `sVideoProperty.eVideoBsType` has exactly one reader in
/// this tree — the `eVideoType` assignment itself (`DecoderConfigParam`).
fn video_bs_type_from_raw(raw: i32) -> VIDEO_BITSTREAM_TYPE {
    match raw {
        0 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
        1 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC,
        _ => VIDEO_BITSTREAM_DEFAULT,
    }
}

unsafe extern "C" fn decoder_init_c(this: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long {
    if this.is_null() || pParam.is_null() {
        return CM_INIT_PARA_ERROR as c_long;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        // **F76, T8.B1 — the caller's block, read as a C caller may have written
        // it, and why this is eight lines rather than `*pParam`.**
        //
        // `SDecodingParam` has two enum-typed fields, `eEcActiveIdc` and
        // `sVideoProperty.eVideoBsType`. On the C side both are plain `int`s, and
        // the reference sanitises them *after* the copy: `decoder.cpp:654` clamps
        // the first into `[ERROR_CON_DISABLE, …FREEZE_RES_CHANGE]` and `:667`
        // normalises the second to `VIDEO_BITSTREAM_DEFAULT`. In Rust each has a
        // closed set of variants, so `*pParam` — which is what this boundary did —
        // is undefined for exactly the inputs the sanitising exists to handle: the
        // read assumed the property the clamp was there to establish.
        //
        // So the block is copied as bytes, the two fields are read and written at
        // their own offsets as the `i32`s they are on the wire, and only then does
        // it become an `SDecodingParam`. Every other field is a pointer, an integer
        // or a `bool`, and a `bool` holding something other than 0/1 is the caller's
        // own undefined behaviour in both trees.
        let param = {
            let mut buf = std::mem::MaybeUninit::<SDecodingParam>::uninit();
            ptr::copy_nonoverlapping(
                pParam.cast::<u8>(),
                buf.as_mut_ptr().cast::<u8>(),
                std::mem::size_of::<SDecodingParam>(),
            );
            let ec = ptr::addr_of_mut!((*buf.as_mut_ptr()).eEcActiveIdc).cast::<i32>();
            ec.write(ec_idc_from_raw(ec.read()) as i32);
            let bs =
                ptr::addr_of_mut!((*buf.as_mut_ptr()).sVideoProperty.eVideoBsType).cast::<i32>();
            bs.write(video_bs_type_from_raw(bs.read()) as i32);
            buf.assume_init()
        };
        let pParam = &param;
        if (*dec_impl).pCtx.is_null() {
            // In-place heap construction: the context is several MiB, and since T3.3
            // it owns `Vec`s, so neither `Box::default()` (stack round-trip) nor
            // `new_zeroed().assume_init()` (invalid zeroed `Vec`) is usable.
            let mut ctx_box = crate::decoder::decoder_context::SWelsDecoderContext::new_boxed();
            // Mirror CWelsDecoder::InitDecoderCtx (welsDecoderExt.cpp): wire the
            // decoder-owned members into the context, then fill in defaults.
            //
            // **T8.A5–A8: the ten `addr_of_mut!` stamps stood here, and F38 was
            // why they were spelled that way.** Each derived a pointer from a
            // `CWelsDecoderImpl` field and stored it into a struct that outlives
            // the expression — S29's worst class, where `&mut (*dec_impl).field`
            // retags the field's range and the next write through `dec_impl`
            // itself pops the stored tag. `addr_of_mut!` was the fix; owning the
            // fields is the answer, and there is nothing left to stamp.
            //
            // The caller's parameters, before `WelsDecoderDefaults` — the position
            // the stamped alias put them in, kept because everything built below
            // this line may read them. `DecoderConfigParam` writes the same block
            // again at the tail of this function, which is where the C++ has its
            // one copy; the two are the same store.
            ctx_box.pParam = param;
            let p_ctx = Box::into_raw(ctx_box);
            // `CWelsDecoder::InitDecoder` runs this over `m_sLastDecPicInfo` just
            // before it calls `InitDecoderCtx` (`welsDecoderExt.cpp:386`); the field
            // is the context's since T8.A6, so its defaults are set where the context
            // is built. They are **not** zeros — `iPrevFrameNum` starts at -1.
            crate::decoder::decoder_core::WelsDecoderLastDecPicInfoDefaults(
                &mut (*p_ctx).pLastDecPicInfo,
            );
            // `ResetReorderingPictureBuffers (&m_sReoderingStatus, m_sPictInfoList,
            // true)` — the `CWelsDecoder` constructor's full reset
            // (`welsDecoderExt.cpp:169`), which is where a fresh reordering buffer
            // comes from. `IMinInt32` in every slot's `iPOC` is what "empty" is;
            // zeroes are a valid POC.
            let crate::decoder::decoder_core::SWelsDecoderContext {
                pPictReoderingStatus, pPictInfoList, ..
            } = &mut *p_ctx;
            crate::decoder::decoder_core::ResetReorderingPictureBuffers(
                pPictReoderingStatus,
                pPictInfoList,
                true,
            );
            crate::decoder::decoder_core::WelsDecoderDefaults(&mut *p_ctx, ptr::null_mut());
            crate::decoder::decoder_core::WelsDecoderSpsPpsDefaults(&mut (*p_ctx).sSpsPpsCtx);
            let ret = crate::decoder::decoder_core::WelsInitStaticMemory(&mut *p_ctx);
            if ret != 0 {
                drop(Box::from_raw(p_ctx));
                return CM_INIT_PARA_ERROR as c_long;
            }
            (*dec_impl).pCtx = p_ctx as *mut _;
        }

        // **F44, T5.S1.** `InitErrorCon` had no production caller in this port —
        // F43's defect shape (a real body nothing reaches) in a function F43 did not
        // name. The C++ calls it here, from `WelsInitDecoder` (`decoder.cpp:665`),
        // and it does two things nothing else does:
        //
        //  * clears `bFreezeOutput`, which `WelsDecoderDefaults` sets **true**. The
        //    only other site that clears it is the "complete non-ECed IDR" arm of
        //    `DecodeFrameConstruction`, so a stream whose first IDR is missing or
        //    damaged stayed frozen and emitted nothing until a clean IDR arrived.
        //  * installs `sCopyFunc`'s two kernels. `DoErrorConSliceCopy` and
        //    `DoErrorConSliceMVCopy` guard every copy with `if let Some(f)`, so with
        //    the table `None` the slice-copy concealment ran and **copied nothing**.
        //
        // It is placed outside the `pCtx.is_null()` block on purpose: the C++ runs it
        // on every `Initialize`, and the parameters it reads are re-copied above.
        if !(*dec_impl).pCtx.is_null() {
            crate::decoder::decoder_core::DecoderConfigParam(&mut *(*dec_impl).pCtx, pParam);
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
            crate::decoder::decoder_core::WelsEndDecoder(&mut *(*dec_impl).pCtx);
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
                *pStride.offset(0) = buf_info.UsrData.sys().iStride[0];
                *pStride.offset(1) = buf_info.UsrData.sys().iStride[1];
            }
            if !iWidth.is_null() {
                *iWidth = buf_info.UsrData.sys().iWidth;
            }
            if !iHeight.is_null() {
                *iHeight = buf_info.UsrData.sys().iHeight;
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


/// Matches `void CWelsDecoder::BufferingReadyPicture (...)` in `welsDecoderExt.cpp`.
///
/// Moves a just-decoded picture out of `pDstInfo` and into the reordering slot
/// list, clearing `iBufferStatus` so nothing is emitted until a release call
/// picks the picture back up in display order.
/// `pCtx ? pCtx->pPicBuff : m_pPicBuff` — the C++'s pool selection, as a flag.
///
/// **`m_pPicBuff` is provably null in this port** and the field is deleted with it
/// (T8.A7). Its only writers in the reference are in `ThreadDecodeFrameInternal`
/// (`welsDecoderExt.cpp:1312`, `:1333`), the threaded decoder's frame loop, which
/// this port does not have — `GetThreadCount` returns 0 and the partial MT arm is
/// fenced as `DECODER_MT(incomplete: F36)`. `CWelsDecoderImpl::pPicBuff` was
/// therefore written exactly once, `null_mut()` in the factory, and read at these
/// two sites.
///
/// So the C's ternary carries exactly one bit: **`FlushFrame` passes a null context
/// to say "do not touch the live pool"** (`welsDecoderExt.cpp:1103`), and every
/// other caller passes the real one. That bit is `bUsePool`, and spelling it as a
/// bool is what lets the reordering state move into the context — a null context
/// argument cannot carry the state the callee now reads out of it.
#[inline]
unsafe fn pool_for(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
) -> crate::decoder::pic_queue::PPicBuff {
    if !bUsePool || pCtx.is_null() {
        return ptr::null_mut();
    }
    crate::decoder::decoder_context::pic_pool_ptr(&mut (*pCtx).pPicBuff)
        .map_or(ptr::null_mut(), |pool| pool)
}

unsafe fn BufferingReadyPicture(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    _ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) {
    if (*pDstInfo).iBufferStatus == 0 {
        return;
    }
    // The null guard stays here — this is the boundary that still holds a pointer,
    // and `active_sps` takes the parameter-set field by reference now (T5.Z1).
    if !pCtx.is_null() {
        if let Some(sps) =
            crate::decoder::decoder_context::active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
        {
            (*pCtx).bIsBaseline = sps.uiProfileIdc == 66 || sps.uiProfileIdc == 83;
        }
    }
    if !(*pCtx).bIsBaseline {
        // T5b.3: `pCtx->pSliceHeader` was a raw alias into a NAL node; the node is an
        // index now (`slice_hdr_nal`) and this resolves it.
        if slice_header_of(&*pCtx)
            .is_some_and(|sh| sh.eSliceType == crate::decoder::slice::EWelsSliceType::B_SLICE)
        {
            (*pCtx).pPictReoderingStatus.bHasBSlice = true;
        }
    }
    for i in 0..16usize {
        if (*pCtx).pPictInfoList[i].iPOC == crate::decoder::decoder_context::IMinInt32 {
            (*pCtx).pPictInfoList[i].sBufferInfo = *pDstInfo;
            (*pCtx).pPictInfoList[i].iPOC =
                slice_header_of(&*pCtx).map_or(0, |sh| sh.iPicOrderCntLsb);
            (*pCtx).pPictInfoList[i].iSeqNum = (*pCtx).iSeqNum;
            (*pCtx).pPictInfoList[i].uiDecodingTimeStamp = (*pCtx).uiDecodingTimeStamp;
            // T5.P′2: the DPB's "previous picture" is a slot handle now, so the
            // resolve happens here rather than the pointer being stored. `api/` is
            // Phase 8's; this is the field's type change reaching its one consumer
            // outside the decoder, nothing more.
            // The thread count is read before the pool borrow opens: the picture
            // is `pPicBuff`'s and `GetThreadCount` takes the context (T5.Z1).
            let bSingleThreaded = crate::decoder::decoder_core::GetThreadCount(&mut *pCtx) <= 1;
            let prev_id = crate::decoder::decoder_context::prev_dpb_id(&(*pCtx).pLastDecPicInfo);
            if let Some(prev) =
                crate::decoder::decoder_context::prev_dpb_pic_mut(&mut (*pCtx).pPicBuff, prev_id)
            {
                let iPicBuffIdx = prev.iPicBuffIdx;
                if bSingleThreaded {
                    prev.iRefCount += 1;
                }
                (*pCtx).pPictInfoList[i].iPicBuffIdx = iPicBuffIdx;
            }
            (*pCtx).iLastBufferedIdx = i as i32;
            (*pDstInfo).iBufferStatus = 0;
            (*pCtx).pPictReoderingStatus.iNumOfPicts += 1;
            if i as i32 > (*pCtx).pPictReoderingStatus.iLargestBufferedPicIndex {
                (*pCtx).pPictReoderingStatus.iLargestBufferedPicIndex = i as i32;
            }
            break;
        }
    }
}

/// Releases the buffered picture whose slot is referenced by `iPictInfoIndex`,
/// dropping the DPB reference taken in [`BufferingReadyPicture`]. Shared tail of
/// both `ReleaseBufferedReadyPicture*` functions in `welsDecoderExt.cpp`.
unsafe fn EmitBufferedPicture(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    pPicBuff: *mut crate::decoder::pic_queue::SPicBuff,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) {
    let idx = (*pCtx).pPictReoderingStatus.iPictInfoIndex as usize;
    *pDstInfo = (*pCtx).pPictInfoList[idx].sBufferInfo;
    *ppDst.add(0) = (*pDstInfo).pDst[0];
    *ppDst.add(1) = (*pDstInfo).pDst[1];
    *ppDst.add(2) = (*pDstInfo).pDst[2];
    (*pCtx).pPictInfoList[idx].iPOC = crate::decoder::decoder_context::IMinInt32;
    let iPicBuffIdx = (*pCtx).pPictInfoList[idx].iPicBuffIdx;
    if !pPicBuff.is_null() {
        // `slot_at_mut` carries the C's `>= 0 && < iCapacity` test, so the range check
        // and the indexing are one expression instead of two that could disagree.
        //
        // **The flip's one boundary cost** (T5.Q2): with owned slots this release has
        // to *derive* the picture rather than copy a pointer out of the array, so it
        // needs the mutable form — it decrements `iRefCount` and hands the picture to
        // `pSetUnRef`. That is api-shaped work forced by a decoder change; it is not
        // F41/F23's `src/api/` inventory, which stays Phase 8's.
        let pPic = (*pPicBuff).slot_at_mut(iPicBuffIdx);
        if !pPic.is_null() {
            (*pPic).iRefCount -= 1;
            if (*pPic).iRefCount <= 0 {
                if let Some(set_unref) = (*pPic).pSetUnRef {
                    // T5.AC1: the callback takes `&mut SPicture`, so the null test
                    // above is what licenses the borrow — one derivation, for the
                    // length of the call, out of the pointer this boundary holds.
                    set_unref(&mut *pPic);
                }
            }
        }
    }
    (*pCtx).pPictReoderingStatus.iNumOfPicts -= 1;
}

/// Matches `void CWelsDecoder::ReleaseBufferedReadyPictureNoReorder (...)`.
///
/// Picks the buffered picture with the smallest decoding timestamp, i.e. plain
/// decode order. Used when the stream has no B slices, where POC ordering is
/// unreliable in practice.
///
/// DELIBERATE DEVIATION from the C++ reference: on a decoding-timestamp *tie*
/// this emits the lower POC first, where C++ falls back to slot order.
///
/// `uiDecodingTimeStamp` is only bumped by `DecodeFrame2` calls that carry data
/// (`if (kiSrcLen > 0 && kpSrc != NULL)`), so the picture that completes during
/// the `DecodeFrame2 (NULL, 0, ...)` end-of-stream flush inherits the previous
/// call's timestamp and ties with the picture already buffered. C++ then emits
/// them in slot order, which can invert the last two pictures of a stream --
/// visible on res/CABA2_SVA_B.264, where upstream emits POC 32 before POC 30
/// and disagrees with the JVT gold. (`h264dec` avoids it only because
/// `DecodeFrameNoDelay` never produces the tie.) POC is the actual display
/// order key, so it is the correct tiebreaker; this engages on an exact tie
/// only, and non-tied ordering is bit-identical to C++.
unsafe fn ReleaseBufferedReadyPictureNoReorder(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) {
    let mut firstValidIdx: i32 = -1;
    let mut uiDecodingTimeStamp: u32 = 0;
    let mut iChosenPOC: i32 = 0;
    let largest = (*pCtx).pPictReoderingStatus.iLargestBufferedPicIndex;
    for i in 0..=largest {
        if (*pCtx).pPictInfoList[i as usize].iPOC != crate::decoder::decoder_context::IMinInt32 {
            uiDecodingTimeStamp = (*pCtx).pPictInfoList[i as usize].uiDecodingTimeStamp;
            iChosenPOC = (*pCtx).pPictInfoList[i as usize].iPOC;
            (*pCtx).pPictReoderingStatus.iPictInfoIndex = i;
            firstValidIdx = i;
            break;
        }
    }
    for i in 0..=largest {
        if i == firstValidIdx {
            continue;
        }
        let info = (*pCtx).pPictInfoList[i as usize];
        if info.iPOC != crate::decoder::decoder_context::IMinInt32
            && (info.uiDecodingTimeStamp < uiDecodingTimeStamp
                || (info.uiDecodingTimeStamp == uiDecodingTimeStamp && info.iPOC < iChosenPOC))
        {
            uiDecodingTimeStamp = info.uiDecodingTimeStamp;
            iChosenPOC = info.iPOC;
            (*pCtx).pPictReoderingStatus.iPictInfoIndex = i;
        }
    }
    if uiDecodingTimeStamp > 0 {
        let idx = (*pCtx).pPictReoderingStatus.iPictInfoIndex as usize;
        (*pCtx).pPictReoderingStatus.iLastWrittenPOC = (*pCtx).pPictInfoList[idx].iPOC;
        (*pCtx).pPictReoderingStatus.iLastWrittenSeqNum = (*pCtx).pPictInfoList[idx].iSeqNum;
        // `PPicBuff pPicBuff = pCtx ? pCtx->pPicBuff : m_pPicBuff;`
        // (`welsDecoderExt.cpp:1026`), as a flag — see [`pool_for`].
        let pPicBuff = pool_for(pCtx, bUsePool);
        EmitBufferedPicture(pCtx, pPicBuff, ppDst, pDstInfo);
    }
}

/// Matches `void CWelsDecoder::ReleaseBufferedReadyPictureReorder (...)`.
///
/// Picks the buffered picture with the smallest (seqNum, POC) and emits it only
/// once it is safe to do so — either it directly follows the last written POC,
/// or the decoder has moved past it. `isFlush` forces the emit.
unsafe fn ReleaseBufferedReadyPictureReorder(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
    isFlush: bool,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    // `PPicBuff pPicBuff = pCtx ? pCtx->pPicBuff : m_pPicBuff;` (`:1128`), which in
    // the C++ is evaluated *before* `if (!pCtx) pCtx = m_pDecContext;` restores the
    // live context for everything below. Both halves are `bUsePool` now — see
    // [`pool_for`] — and the restore is what makes the flag the whole difference.
    let pPicBuff = pool_for(pCtx, bUsePool);

    if (*pCtx).pPictReoderingStatus.iNumOfPicts > 0 {
        (*pCtx).pPictReoderingStatus.iMinPOC = IMinInt32;
        let mut firstValidIdx: i32 = -1;
        let largest = (*pCtx).pPictReoderingStatus.iLargestBufferedPicIndex;
        for i in 0..=largest {
            let info = (*pCtx).pPictInfoList[i as usize];
            if (*pCtx).pPictReoderingStatus.iMinPOC == IMinInt32 && info.iPOC > IMinInt32 {
                (*pCtx).pPictReoderingStatus.iMinPOC = info.iPOC;
                (*pCtx).pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                (*pCtx).pPictReoderingStatus.iPictInfoIndex = i;
                firstValidIdx = i;
                break;
            }
        }
        for i in 0..=largest {
            if i == firstValidIdx {
                continue;
            }
            let info = (*pCtx).pPictInfoList[i as usize];
            let min_seq = (*pCtx).pPictReoderingStatus.iMinSeqNum;
            let min_poc = (*pCtx).pPictReoderingStatus.iMinPOC;
            if info.iPOC > IMinInt32
                && (if info.iSeqNum == min_seq {
                    info.iPOC < min_poc
                } else {
                    info.iSeqNum.wrapping_sub(min_seq) < 0
                })
            {
                (*pCtx).pPictReoderingStatus.iMinPOC = info.iPOC;
                (*pCtx).pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                (*pCtx).pPictReoderingStatus.iPictInfoIndex = i;
            }
        }
    }

    if (*pCtx).pPictReoderingStatus.iMinPOC > IMinInt32 {
        let mut isReady = true;
        if !isFlush {
            let last_idx = (*pCtx).iLastBufferedIdx as usize;
            let iLastPOC = match if pCtx.is_null() { None } else { slice_header_of(&*pCtx) } {
                Some(sh) => sh.iPicOrderCntLsb,
                None => (*pCtx).pPictInfoList[last_idx].iPOC,
            };
            let iLastSeqNum = if !pCtx.is_null() {
                (*pCtx).iSeqNum
            } else {
                (*pCtx).pPictInfoList[last_idx].iSeqNum
            };
            let st = (*pCtx).pPictReoderingStatus;
            isReady = (st.iLastWrittenPOC > IMinInt32 && st.iMinPOC - st.iLastWrittenPOC <= 1)
                || st.iMinPOC < iLastPOC
                || st.iMinSeqNum.wrapping_sub(iLastSeqNum) < 0;
        }
        if isReady {
            (*pCtx).pPictReoderingStatus.iLastWrittenPOC = (*pCtx).pPictReoderingStatus.iMinPOC;
            (*pCtx).pPictReoderingStatus.iLastWrittenSeqNum =
                (*pCtx).pPictReoderingStatus.iMinSeqNum;
            EmitBufferedPicture(pCtx, pPicBuff, ppDst, pDstInfo);
            (*pCtx).pPictReoderingStatus.iMinPOC = IMinInt32;
        }
    }
}

/// Matches `DECODING_STATE CWelsDecoder::ReorderPicturesInDisplay (...)`.
///
/// Baseline streams never reorder, so the picture passes straight through and
/// the buffer stays empty.
unsafe fn ReorderPicturesInDisplay(
    pCtx: *mut crate::decoder::decoder_core::SWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) {
    // **The null test moves ahead of the lookup** (T5.Z1). It used to sit after it,
    // and only the accessor's own internal `pCtx.is_null()` arm made that safe; with
    // the parameter-set field taken by reference the guard has to precede the call,
    // which is the same order the C++ has.
    if pCtx.is_null() {
        return;
    }
    let Some(profile) =
        crate::decoder::decoder_context::active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
            .map(|sps| sps.uiProfileIdc)
    else {
        return;
    };
    (*pCtx).bIsBaseline = profile == 66 || profile == 83;
    if (*pCtx).bIsBaseline || (*pDstInfo).iBufferStatus != 1 {
        return;
    }
    let sh_poc = slice_header_of(&*pCtx)
        .filter(|sh| sh.eSliceType == crate::decoder::slice::EWelsSliceType::B_SLICE)
        .map(|sh| sh.iPicOrderCntLsb);
    if let Some(sh_poc) = sh_poc {
        let st = (*pCtx).pPictReoderingStatus;
        let follows = if (*pCtx).iSeqNum == st.iLastWrittenSeqNum {
            sh_poc <= st.iLastWrittenPOC + 2
        } else {
            (*pCtx).iSeqNum - st.iLastWrittenSeqNum == 1 && sh_poc == 0
        };
        if follows {
            // issue #3478: B-slice type is a more reliable ordering signal than POC.
            (*pCtx).pPictReoderingStatus.iLastWrittenPOC = sh_poc;
            (*pCtx).pPictReoderingStatus.iLastWrittenSeqNum = (*pCtx).iSeqNum;
            *ppDst.add(0) = (*pDstInfo).pDst[0];
            *ppDst.add(1) = (*pDstInfo).pDst[1];
            *ppDst.add(2) = (*pDstInfo).pDst[2];
            return;
        }
    }
    BufferingReadyPicture(pCtx, ppDst, pDstInfo);
    if !(*pCtx).pPictReoderingStatus.bHasBSlice && (*pCtx).pPictReoderingStatus.iNumOfPicts > 1 {
        ReleaseBufferedReadyPictureNoReorder(pCtx, true, ppDst, pDstInfo);
    } else {
        ReleaseBufferedReadyPictureReorder(pCtx, true, ppDst, pDstInfo, false);
    }
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

        (*p_ctx).iErrorCode = DECODING_STATE::dsErrorFree.0;
        if !kpSrc.is_null() && kiSrcLen > 0 {
            (*p_ctx).bEndOfStreamFlag = false;
            if crate::decoder::decoder_core::GetThreadCount(&mut *p_ctx) <= 0 {
                (*p_ctx).uiDecodeTimeStamp += 1;
                (*p_ctx).uiDecodingTimeStamp = (*p_ctx).uiDecodeTimeStamp;
            }
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                std::slice::from_raw_parts(kpSrc, kiSrcLen as usize),
                kiSrcLen,
                &mut *(ppDst as *mut [*mut u8; 3]),
                &mut *pDstInfo,
                ptr::null_mut(),
            );
        } else if (*dec_impl).bEndOfStream || (*p_ctx).bEndOfStreamFlag || kpSrc.is_null() || kiSrcLen == 0 {
            (*p_ctx).bEndOfStreamFlag = true;
            // **F45, T5.S1.** The C++ sets this on exactly this arm
            // (`welsDecoderExt.cpp:777`) and clears it right after `WelsDecodeBs`
            // (`:814`). Nothing in this port ever wrote it, so it read `false`
            // forever — and `DecodeFrameConstruction` has the reader:
            //
            //     if iTotalNumMbRec != kiTotalNumMbInCurLayer {
            //         bFrameCompleteFlag = false;
            //         if bInstantDecFlag { return ERR_INFO_MB_NUM_INADEQUATE }   // <-- never taken
            //     }
            //
            // With the flag stuck false the early return never fired, so the
            // flush call fell through to the output path and **emitted a frame the
            // C++ does not emit** — one extra frame at end of stream on every
            // truncated stream, and a whole frame out of nothing on a stream cut
            // inside its first slice.
            (*p_ctx).bInstantDecFlag = true;
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                &[],
                0,
                &mut *(ppDst as *mut [*mut u8; 3]),
                &mut *pDstInfo,
                ptr::null_mut(),
            );
            (*p_ctx).bInstantDecFlag = false; // reset no-delay flag
        }
        // `ReorderPicturesInDisplay` at the tail of DecodeFrame2WithCtx.
        ReorderPicturesInDisplay(p_ctx, ppDst, pDstInfo);
        // **F46, T5.T1.** `welsDecoderExt.cpp:892` — the accumulator, whole.
        DECODING_STATE((*p_ctx).iErrorCode)
    }
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
                if pOption.is_null() {
                    return CM_INIT_PARA_ERROR as c_long;
                }
                if (*dec_impl).pCtx.is_null() {
                    return DECODING_STATE::dsInitialOptExpected.0 as c_long;
                }
                // **F76, T8.B1 — the blob is an `int` and the clamp is the C++'s.**
                // `welsDecoderExt.cpp:528` reads `* ((int*)pOption)` and runs it
                // through `WELS_CLIP3 (iVal, ERROR_CON_DISABLE,
                // ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)` before the
                // store. This port read the blob as `*const ERROR_CON_IDC`, which is
                // undefined the moment a caller passes anything outside 0..=7 — and
                // 0..=7 is exactly the range the clamp exists to enforce, so reading
                // the option at the enum's type assumed the property it was there to
                // establish. It is an `i32` here and becomes an `ERROR_CON_IDC` only
                // once it is in range.
                let val = ec_idc_from_raw(pOption.cast::<i32>().read());
                // `welsDecoderExt.cpp:529–533`: parse-only decoding may not turn
                // concealment back on, and the rejection is a return code, not a
                // silent drop.
                if (*(*dec_impl).pCtx).pParam.bParseOnly && val != ERROR_CON_IDC::ERROR_CON_DISABLE {
                    return CM_INIT_PARA_ERROR as c_long;
                }
                // **The context's own copy** (T8.A5, F41) — and it is where the
                // C++ writes it too: `pDecContext->pParam->eEcActiveIdc = iVal`
                // at `welsDecoderExt.cpp:535`, one level below the api object.
                (*(*dec_impl).pCtx).pParam.eEcActiveIdc = val;
                // F44's second call site (`welsDecoderExt.cpp:536`): the mode
                // selects which kernels `sCopyFunc` holds, so changing it without
                // re-running the init leaves the previous mode's table in place.
                crate::decoder::error_concealment::InitErrorCon(&mut *(*dec_impl).pCtx);
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
                // No context, no reordering state, and the count the C++ reads out of
                // `m_sReoderingStatus` with no context is zero for the same reason
                // (T8.A7; see `decoder_flush_frame_c`).
                *(pOption as *mut i32) = if (*dec_impl).pCtx.is_null() {
                    0
                } else {
                    (*(*dec_impl).pCtx).pPictReoderingStatus.iNumOfPicts
                };
            }
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                *(pOption as *mut i32) = if (*dec_impl).bEndOfStream { 1 } else { 0 };
            }
            // `welsDecoderExt.cpp:634–637`. Unwired until T8.B1, which is why F76's
            // two `DecoderConfigParam` statements had no observable: the mode the
            // decoder actually runs with is only visible through this option.
            DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                if (*dec_impl).pCtx.is_null() {
                    return CM_INIT_EXPECTED as c_long;
                }
                pOption.cast::<i32>().write((*(*dec_impl).pCtx).pParam.eEcActiveIdc as i32);
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

/// Matches `DECODING_STATE CWelsDecoder::FlushFrame (...)` in `welsDecoderExt.cpp`:
/// drains the display-reordering buffer only. The decoder core itself is flushed
/// by the caller through `DecodeFrame2 (NULL, 0, ...)` after signalling EOS.
unsafe extern "C" fn decoder_flush_frame_c(this: *mut ISVCDecoder, ppDst: *mut *mut u8, pDstInfo: *mut SBufferInfo) -> DECODING_STATE {
    if this.is_null() {
        return DECODING_STATE::dsInitialOptExpected;
    }
    let dec_impl = this as *mut CWelsDecoderImpl;
    unsafe {
        let p_ctx = (*dec_impl).pCtx;
        // With no context there is no reordering state to drain — the buffers are
        // the context's own fields since T8.A7. The C++ reaches `CWelsDecoder`'s
        // copies here and finds `iNumOfPicts == 0` for the same two reasons: the
        // constructor full-resets them (`welsDecoderExt.cpp:169`) and `DestroyPicBuff`
        // resets them on the way out (F37).
        if p_ctx.is_null() {
            return DECODING_STATE::dsErrorFree;
        }
        if (*p_ctx).bEndOfStreamFlag && (*p_ctx).pPictReoderingStatus.iNumOfPicts > 0 {
            // `false` is the C's `NULL` context argument (`welsDecoderExt.cpp:1103`):
            // drain the slot list without touching the live pool. See `pool_for`.
            if !(*p_ctx).pPictReoderingStatus.bHasBSlice {
                ReleaseBufferedReadyPictureNoReorder(p_ctx, false, ppDst, pDstInfo);
            } else {
                ReleaseBufferedReadyPictureReorder(p_ctx, false, ppDst, pDstInfo, true);
            }
        }
    }
    DECODING_STATE::dsErrorFree
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
        bEndOfStream: false,
    });
    // **T8.A6/A7: three initialisers stood here and all three moved.** They ran over
    // `CWelsDecoderImpl` members at decoder *creation*; the members are the context's
    // fields now, so each runs where the reference runs it — `InitVlcTable` inside
    // `WelsOpenDecoder` (`decoder.cpp:606`), `WelsDecoderLastDecPicInfoDefaults` in
    // `decoder_init_c`'s construction block (`welsDecoderExt.cpp:386`), and the
    // reordering full reset with them. None of the three sets zeros, so *where* they
    // run is a fact and not a formality; what changes is that a re-`Initialize`d
    // decoder now gets them again, which is what `CWelsDecoder::InitDecoder`'s three
    // `memset`s do on every call and what this port never did.
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
                crate::decoder::decoder_core::WelsEndDecoder(&mut *(*dec_impl).pCtx);
                drop(Box::from_raw((*dec_impl).pCtx));
                (*dec_impl).pCtx = ptr::null_mut();
            }
            drop(Box::from_raw(dec_impl));
        }
    }
}


#[cfg(test)]
pub(crate) mod abi_test_driver {
    use super::*;

    /// Decodes `stream` through the C ABI and returns `(frames out, last frame's
    /// dimensions)`.
    ///
    /// **It calls the vtable thunks directly rather than the conveniences, and that
    /// is now a choice rather than a workaround.** The first draft used the
    /// convenience methods, and Miri rejected the call before the decoder was even
    /// initialized — the receiver was `&mut self` over a struct one pointer wide and
    /// the thunk wrote at offset `0x20`. That defect is **F23, fixed at T8.A3**: the
    /// conveniences take `this` as a raw pointer now and either spelling is sound.
    /// This driver keeps the vtable spelling because it is what a C caller compiles
    /// to and because it exercises the slot *table*, which the conveniences resolve
    /// through but do not prove. `f23_boundary_provenance` is the probe that covers
    /// the other spelling. `WelsCreateDecoder` hands out `Box::into_raw(dec)` cast to
    /// the interface type, which carries provenance for the whole implementation
    /// object, so both are calls on the same allocation.
    /// Returns `(frames, dims, states)`, where `states` is the bitwise OR of every
    /// `DecodeFrame2` return. The third element exists for T5.S1's probes: a
    /// concealment path that does not run looks exactly like one that runs and
    /// changes nothing, and `dsDataErrorConcealed` in the OR is the difference.
    pub(crate) fn drive_decoder_over(stream: &[u8]) -> (usize, Option<(i32, i32)>, i32) {
        // SAFETY: `WelsCreateDecoder` hands out `Box::into_raw(dec) as *mut
        // ISVCDecoder`, so the pointer carries provenance for the whole
        // implementation object and every call below is the sequence a C caller
        // makes.
        unsafe {
            {
                let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
                assert_eq!(
                    i64::from(WelsCreateDecoder(&mut p_decoder)),
                    CM_RESULT_SUCCESS as i64
                );
                assert!(!p_decoder.is_null());
                let vtbl = (*p_decoder).lpVtbl;

                let mut dec_param = SDecodingParam::default();
                dec_param.uiTargetDqLayer = u8::MAX;
                dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
                dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
                assert_eq!(
                    i64::from(((*vtbl).Initialize)(p_decoder, &dec_param as *const SDecodingParam)),
                    CM_RESULT_SUCCESS as i64
                );

                let mut frames = 0;
                let mut dims = None;
                let mut states = 0i32;
                for unit in crate::split_annexb_units(stream) {
                    let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
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

                // End of stream, then the zero-length call that flushes it — the same
                // tail `decoder_conformance_test.rs` and `malformed_stream_parity.rs`
                // use. T5.S1 added it: without it this helper never drove the flush
                // path at all, and a stream whose only frame arrives there (the FMO
                // asset, any truncated stream) looked to it like a stream that decodes
                // nothing. It cannot cost the two probes above a verdict — they assert
                // `frames > 0` and the dimensions, and a flush only ever adds frames.
                let mut eos_flag = 1i32;
                ((*vtbl).SetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                    &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
                );
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let ret = ((*vtbl).DecodeFrame2)(
                    p_decoder,
                    std::ptr::null(),
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

                // …and the drain the flush announces. Leaving it out cost a frame on
                // every stream whose last picture is still buffered at EOS, which read
                // as the port being one frame short of the C++ until the helper was
                // compared against `rust/tools/ecref` rather than against itself.
                let mut remaining = 0i32;
                ((*vtbl).GetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                    &mut remaining as *mut i32 as *mut std::ffi::c_void,
                );
                for _ in 0..remaining.clamp(0, 24) {
                    let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
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
    /// how many bytes of NAL it produced, and **how many NALs those bytes came in**.
    ///
    /// The NAL counts are Phase 6 session D's, and `vcl_nals` is the one that
    /// carries a claim. A frame's slices are exactly the NALs of its
    /// `VIDEO_CODING_LAYER` layers (`uiLayerType`), so `vcl_nals` **is** the coded
    /// slice count — where `nals` also counts the parameter sets an IDR carries in
    /// its `NON_VIDEO_CODING_LAYER`. A `SM_SIZELIMITED_SLICE` probe that encodes
    /// one slice covers nothing it exists for, so the split matters: on the IDR,
    /// `nals` is ≥ 2 whatever the slice mode does.
    pub(crate) struct EncodedFrame {
        pub(crate) kind: EVideoFrameType,
        pub(crate) bytes: usize,
        pub(crate) nals: usize,
        pub(crate) vcl_nals: usize,
        pub(crate) frame_size: i32,
    }

    /// Fills `buf` with frame `f` of a synthetic I420 sequence that **moves**.
    ///
    /// Two motions at different velocities, and both halves of that matter. The
    /// whole picture translates by (2, 1) samples per frame, so every macroblock
    /// has a non-zero motion vector; a bright block crosses it at (3, 2), so the
    /// macroblocks it touches disagree with their neighbours and the *predicted*
    /// vector is wrong for them. One velocity everywhere would run the search and
    /// then code `mvd = 0` at every macroblock but the first.
    ///
    /// The texture is a xor/quotient pattern rather than a gradient for the same
    /// reason the motion has to be there at all: **a translated gradient is a
    /// gradient plus a constant**, so the search answers (0, 0) with a DC residual
    /// and nothing about motion estimation is measured.
    fn moving_i420(width: i32, height: i32, f: usize, buf: &mut [u8]) {
        fn texture(u: i32, v: i32) -> u8 {
            let a = (u.wrapping_mul(3) ^ v.wrapping_mul(5)) as u32;
            let b = (u / 7).wrapping_mul(37).wrapping_add((v / 5).wrapping_mul(53)) as u32;
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

    /// The two configuration knobs the encoder probes vary — **Phase 6 session C**.
    ///
    /// Everything else about the driver's configuration is fixed (`rust_enc`'s, with
    /// session A's three determinism departures); these two are separated because each
    /// selects a *different body of code*, not a different parameter value:
    ///
    /// * `cabac` picks the entropy writers — `svc_set_mb_syn_cabac.rs` or
    ///   `svc_set_mb_syn_cavlc.rs`, sixteen conversion sites in session C's face 1.
    /// * `complexity` picks the mode-decision family: `LOW_COMPLEXITY` installs
    ///   `SetFastCodingFunc` (`encoder_ext.rs:2485`, `bFastMode`) and anything else
    ///   runs the fine intra partition search (`WelsMdIntraFinePartition`,
    ///   `WelsMdI4x4`) and the `pMemPredBlk4` ping-pong that session C's face 2 moves.
    ///
    /// * `slice_mode`/`slice_constraint` pick the *slicing* machinery — **Phase 6
    ///   session D**. `SM_SIZELIMITED_SLICE` is the only encode path with a loop of
    ///   its own (`WelsMdInterMbLoopOverDynamicSlice`), the only caller of the
    ///   CAVLC/CABAC stash-and-rollback pair (`StashMBStatus`/`StashPopMBStatus`)
    ///   and of `pDynamicBsBuffer`, and the only reader of
    ///   `CalculateNewSliceNum` → `ReallocSliceBuffer` → `ExtendLayerBuffer` →
    ///   `ReOrderSliceInLayer`. `slice_constraint` is `uiSliceSizeConstraint` in
    ///   bytes and is ignored by every other mode; validation refuses anything
    ///   ≤ `MAX_MACROBLOCK_SIZE_IN_BYTE` (400), and a slice closes at
    ///   `constraint - AVER_MARGIN_BYTES` (100) bytes of payload.
    ///
    /// **All four defaults are what the first probe has always used** (CABAC,
    /// `LOW_COMPLEXITY`, one slice per frame), so `Default::default()` leaves the
    /// three existing probes unchanged.
    #[derive(Debug, Copy, Clone)]
    pub(crate) struct EncoderProbeOptions {
        pub cabac: bool,
        pub complexity: ECOMPLEXITY_MODE,
        pub slice_mode: SliceModeEnum,
        pub slice_constraint: u32,
        /// `iMultipleThreadIdc`. **Above 1 this is the only way any test in this
        /// crate reaches the fork/join** — every probe before T7.B4 hard-coded 1,
        /// which is why deleting F12's Miri skip needed a probe as well as a
        /// deletion. `bUseLoadBalancing` is forced off below, so the path stays
        /// byte-deterministic.
        pub threads: u16,
        /// `uiSliceNum` for `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE`.
        pub slice_num: u32,
        /// `bUseLoadBalancing`. **Default `false`, and every byte-asserting probe
        /// must leave it there** — with it on, and `iMultipleThreadIdc >= uiSliceNum`,
        /// frame N+1's slice boundaries are a function of frame N's measured encode
        /// *times*, so the bitstream stops being a function of the input. `GetDefaultParams`
        /// sets it **on**, which is why the field is forced here rather than inherited.
        ///
        /// The one probe that turns it on is `load_balancing_completes_frames_with_sane_slice_counts`
        /// (T7.C1), which asserts structure and never bytes — F72's expected-divergent
        /// class, the project's second after `CABA2_SVA_B`.
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

    /// Encodes `frames` frames of [`moving_i420`] at `width` x `height` through the
    /// C ABI, and returns what came out frame by frame together with the encoder's
    /// **own** report of the resolution it is configured for.
    ///
    /// **It calls the vtable thunks directly rather than the conveniences**, for the
    /// reason [`drive_decoder_over`] gives. `ISVCEncoder` is one pointer wide exactly
    /// as `ISVCDecoder` is, and every encoder thunk casts `this` to the
    /// implementation type and reaches `inner` past the `base`/`pVtbl` pair at offset
    /// `0x10` — so the old `&mut self` conveniences were F23's encoder twin, and it
    /// is fixed with F23 itself at T8.A3.
    ///
    /// The configuration is `rust_enc`'s — the driver the 341-configuration
    /// diffharness sweeps run — with three deliberate departures, each for
    /// determinism the probe's assertions rest on: scene-change detection off (a
    /// detected cut would make frame 1 an IDR and there would be no inter frame),
    /// frame skip off (a skipped frame emits no NAL), and `uiIntraPeriod = 0` (no
    /// periodic IDR). The profile is `PRO_HIGH`, because a baseline layer forces
    /// CAVLC and the probe has to be able to ask for either writer; entropy coding
    /// and complexity come from [`EncoderProbeOptions`] and default to CABAC over
    /// `LOW_COMPLEXITY` — the CABAC writers were the larger raw surface of the two
    /// (66 raw-pointer occurrences and 30 `unsafe fn` against 35 and 12), which is
    /// why the first probe took them.
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
            let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
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
            // Off unless a probe asks for it, and only one does. With it on and
            // `iMultipleThreadIdc >= uiSliceNum` the encoder takes `AdjustBaseLayer`
            // -> `DynamicAdjustSlicing`, whose slice boundaries for frame N+1 come
            // from frame N's measured encode *times* — so the bitstream stops being a
            // function of the input and any byte assertion stops meaning anything.
            // The diffharness gates it off for the same reason (`cxx_enc.cpp:119`);
            // `GetDefaultParams` turns it **on**, so this line is a force, not a
            // default. See `EncoderProbeOptions::load_balancing`.
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
                    &mut effective as *mut SEncParamExt as *mut std::ffi::c_void,
                ),
                CM_RESULT_SUCCESS
            );

            let luma = (width * height) as usize;
            let mut buf = vec![0u8; luma * 3 / 2];
            let mut out = Vec::with_capacity(frames);
            for f in 0..frames {
                moving_i420(width, height, f, &mut buf);
                // One derivation for all three planes. Three `as_mut_ptr()` calls
                // would each retag and pop the previous one, which is F13's class
                // manufactured by the test — Phase 2 fixed four of exactly this.
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
                let mut nals = 0usize;
                let mut vcl_nals = 0usize;
                for l in 0..info.iLayerNum as usize {
                    let lay = &info.sLayerInfo[l];
                    if lay.pNalLengthInByte.is_null() {
                        continue;
                    }
                    nals += lay.iNalCount as usize;
                    if lay.uiLayerType == LAYER_TYPE::VIDEO_CODING_LAYER as u8 {
                        vcl_nals += lay.iNalCount as usize;
                    }
                    for n in 0..lay.iNalCount as usize {
                        bytes += *lay.pNalLengthInByte.add(n) as usize;
                    }
                }
                out.push(EncodedFrame {
                    kind: info.eFrameType,
                    bytes,
                    nals,
                    vcl_nals,
                    frame_size: info.iFrameSizeInBytes,
                });
            }

            ((*vtbl).Uninitialize)(p_encoder);
            WelsDestroySVCEncoder(p_encoder);
            (out, (effective.iPicWidth, effective.iPicHeight))
        }
    }
}

// ============================================================================
// F23's covering test
// ============================================================================

#[cfg(test)]
mod f23_boundary_provenance {
    use super::*;

    /// **F23, and its encoder twin, as a probe.**
    ///
    /// The consumer conveniences on `ISVCDecoder`/`ISVCEncoder` used to take `&mut
    /// self`. Those two structs are **one pointer wide** — they are the C++ classes'
    /// vtable slot and nothing else — while the thunk behind every slot immediately
    /// casts `this` to a pointer to `CWelsDecoderImpl` / `CWelsH264SVCEncoderImpl`
    /// and writes the implementation object *past* those eight bytes.
    ///
    /// A `&mut ISVCDecoder` carries provenance for eight bytes. `decoder_init_c`
    /// writes `CWelsDecoderImpl::param` at offset `0x20`. That is out of bounds for
    /// the borrow the call was made through, on the public API path, in a library
    /// whose whole purpose is to be called that way — and `abi_test_driver` has
    /// carried a comment saying so since Phase 5, because its first draft tripped
    /// over it and was rewritten to call through the raw vtable instead.
    ///
    /// So this is the shape the finding describes, kept. Written at **T8.A2** it was
    /// **red**, and the message is the finding in one line:
    ///
    /// ```text
    /// error: Undefined Behavior: attempting a write access using <584081> at
    ///        alloc288026[0x20], but that tag does not exist in the borrow stack
    ///   help: <584081> was created by a SharedReadWrite retag at offsets [0x0..0x8]
    /// ```
    ///
    /// It is **green from T8.A3**, where the twelve conveniences became associated
    /// functions taking `this` as a raw pointer. It asserts nothing — Miri is the
    /// assertion. What it must keep doing is *calling a convenience*, on both codecs,
    /// so that a re-introduced `&mut self` receiver is caught by the checker rather
    /// than by a reader.
    ///
    /// Deliberately cheap: no frame is encoded and no stream decoded, because the
    /// defect is at `Initialize` and the `--lib` Miri step already costs 1651s.
    #[test]
    fn conveniences_call_through_the_whole_impl_allocation() {
        unsafe {
            // --- the decoder half -------------------------------------------
            let mut p_decoder = ptr::null_mut();
            assert_eq!(
                i64::from(WelsCreateDecoder(&mut p_decoder)),
                CM_RESULT_SUCCESS as i64
            );
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;
            dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
            dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
            // The write that is out of bounds for an eight-byte borrow: this call
            // stores `*pParam` into `CWelsDecoderImpl::param`.
            assert_eq!(
                i64::from(ISVCDecoder::Initialize(p_decoder, &dec_param)),
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
                i64::from(ISVCDecoder::Uninitialize(p_decoder)),
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
            assert_eq!(ISVCEncoder::Initialize(p_encoder, &enc_param), CM_RESULT_SUCCESS);
            assert_eq!(ISVCEncoder::Uninitialize(p_encoder), CM_RESULT_SUCCESS);
            WelsDestroySVCEncoder(p_encoder);
        }
    }
}
