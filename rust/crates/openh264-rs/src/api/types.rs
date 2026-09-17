//! Shared codec types, constants, return codes, options, and enums (`codec_def.h`, `codec_app_def.h`).
//!
//! These types are used across the Rust core, the safe Rust API (`Encoder` / `Decoder`),
//! the `cxx` C++ bridge (`cxx_api`), and the raw C ABI (`c_api`).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use std::ffi::{c_char, c_void};
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
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
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ECOMPLEXITY_MODE {
    #[default]
    LOW_COMPLEXITY = 0,
    MEDIUM_COMPLEXITY = 1,
    HIGH_COMPLEXITY = 2,
}

pub type EComplexityMode = ECOMPLEXITY_MODE;

/// Parameter set strategy (`EParameterSetStrategy`).
#[repr(i32)]
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

pub use crate::api::cxx_api::ffi::{
    SEncParamBase, SEncParamExt, SFrameBSInfo, SLayerBSInfo, SSliceArgument, SSourcePicture,
    SSpatialLayerConfig,
};

pub type PEncParamBase = *mut SEncParamBase;
pub type PLayerBSInfo = *mut SLayerBSInfo;
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
