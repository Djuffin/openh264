//! Low-level C ABI type definitions matching OpenH264 C interface.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_imports,
    unsafe_op_in_unsafe_fn
)]

use std::ffi::c_void;

pub mod common;
pub mod decoder;
pub mod encoder;
pub mod api;


pub const MAX_SPATIAL_LAYER_NUM: usize = 4;
pub const MAX_QUALITY_LAYER_NUM: usize = 4;
pub const MAX_TEMPORAL_LAYER_NUM: usize = 4;
pub const MAX_LAYER_NUM_OF_FRAME: usize = 128;
pub const MAX_NAL_UNITS_IN_LAYER: usize = 128;

// Error & Return Codes
pub const CM_RESULT_SUCCESS: i32 = 0;
pub const CM_INIT_PARA_ERROR: i32 = 1;
pub const CM_UNINITIALIZED_ERROR: i32 = 2;
pub const CM_MALLOC_MEM_ERROR: i32 = 3;
pub const CM_UNSUPPORTED_DATA: i32 = 4;
pub const CM_UNKNOW_REASON: i32 = 5;

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EUsageType {
    #[default]
    CameraVideoRealTime = 0,
    ScreenContentRealTime = 1,
    CameraVideoNonRealTime = 2,
    ScreenContentNonRealTime = 3,
    InputContentTypeAll = 4,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum RCMode {
    #[default]
    RcQualityMode = 0,
    RcBitrateMode = 1,
    RcBufferBasedMode = 2,
    RcTimestampMode = 3,
    RcOffMode = -1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SliceMode {
    #[default]
    SmSingleSlice = 0,
    SmFixedSliceNum = 1,
    SmRasterSlice = 2,
    SmSizeSlice = 3,
    SmAutoSlice = 4,
    SmReserved = 5,
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum VideoFormat {
    VideoFormatRgb = 1,
    VideoFormatRgba = 2,
    VideoFormatRgb555 = 3,
    VideoFormatRgb565 = 4,
    VideoFormatBgr = 5,
    VideoFormatBgra = 6,
    VideoFormatAbgr = 7,
    VideoFormatArgb = 8,
    VideoFormatYuy2 = 20,
    VideoFormatYvyu = 21,
    VideoFormatUyvy = 22,
    #[default]
    VideoFormatI420 = 23,
    VideoFormatYv12 = 24,
    VideoFormatNv12 = 25,
    VideoFormatVFlip = -0x80000000i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EncoderOption {
    #[default]
    EncoderOptionDatFormat = 0,
    EncoderOptionIdrInterval = 1,
    EncoderOptionSvcEncodeParamBase = 2,
    EncoderOptionSvcEncodeParamExt = 3,
    EncoderOptionFrameRate = 4,
    EncoderOptionBitrate = 5,
    EncoderOptionMaxBitrate = 6,
    EncoderOptionComplexity = 7,
    EncoderOptionGetStatistics = 8,
    EncoderOptionTraceCallback = 9,
    EncoderOptionTraceCallbackContext = 10,
    EncoderOptionTraceLevel = 11,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EVideoFrameType {
    #[default]
    VideoFrameTypeInvalid = 0,
    VideoFrameTypeIDR = 1,
    VideoFrameTypeI = 2,
    VideoFrameTypeP = 3,
    VideoFrameTypeSkip = 4,
    VideoFrameTypeIPMixed = 5,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EComplexityMode {
    #[default]
    LowComplexity = 0,
    MediumComplexity = 1,
    HighComplexity = 2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EParameterSetStrategy {
    ConstantId = 0,
    #[default]
    IncreasingId = 1,
    SpsListing = 2,
    SpsListingAndPpsIncreasing = 3,
    SpsPpsBsOverwrite = 4,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceArgument {
    pub uiSliceMode: SliceMode,
    pub uiSliceNum: u32,
    pub uiSliceSizeConstraint: u32,
    pub uiSliceMbNum: [u32; 35],
    pub bSliceNumBoxCount: bool,
}

impl Default for SSliceArgument {
    fn default() -> Self {
        Self {
            uiSliceMode: SliceMode::SmSingleSlice,
            uiSliceNum: 0,
            uiSliceSizeConstraint: 0,
            uiSliceMbNum: [0; 35],
            bSliceNumBoxCount: false,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpatialLayerConfig {
    pub iVideoWidth: i32,
    pub iVideoHeight: i32,
    pub fFrameRate: f32,
    pub iSpatialBitrate: i32,
    pub iMaxSpatialBitrate: i32,
    pub uiProfileIdc: i32,
    pub uiLevelIdc: i32,
    pub iDLayerQp: i32,
    pub sSliceArgument: SSliceArgument,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SEncParamBase {
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iRCMode: RCMode,
    pub fMaxFrameRate: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SEncParamExt {
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iRCMode: RCMode,
    pub fMaxFrameRate: f32,
    pub iTemporalLayerNum: i32,
    pub iSpatialLayerNum: i32,
    pub sSpatialLayers: [SSpatialLayerConfig; MAX_SPATIAL_LAYER_NUM],
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
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSourcePicture {
    pub iColorFormat: i32,
    pub iStride: [i32; 4],
    pub pData: [*mut u8; 4],
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub uiTimeStamp: i64,
}

impl Default for SSourcePicture {
    fn default() -> Self {
        Self {
            iColorFormat: VideoFormat::VideoFormatI420 as i32,
            iStride: [0; 4],
            pData: [std::ptr::null_mut(); 4],
            iPicWidth: 0,
            iPicHeight: 0,
            uiTimeStamp: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerBSInfo {
    pub uiTemporalId: u8,
    pub uiSpatialId: u8,
    pub uiQualityId: u8,
    pub uiLayerType: u8,
    pub iNalCount: i32,
    pub pNalLengthInByte: *mut i32,
    pub pBsBuf: *mut u8,
}

impl Default for SLayerBSInfo {
    fn default() -> Self {
        Self {
            uiTemporalId: 0,
            uiSpatialId: 0,
            uiQualityId: 0,
            uiLayerType: 0,
            iNalCount: 0,
            pNalLengthInByte: std::ptr::null_mut(),
            pBsBuf: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SFrameBSInfo {
    pub iTemporalId: i32,
    pub iSubSeqId: i32,
    pub iLayerNum: i32,
    pub sLayerInfo: [SLayerBSInfo; MAX_LAYER_NUM_OF_FRAME],
    pub eFrameType: i32,
    pub iFrameSizeInBytes: i32,
    pub uiTimeStamp: i64,
}

impl Default for SFrameBSInfo {
    fn default() -> Self {
        Self {
            iTemporalId: 0,
            iSubSeqId: 0,
            iLayerNum: 0,
            sLayerInfo: [SLayerBSInfo::default(); MAX_LAYER_NUM_OF_FRAME],
            eFrameType: 0,
            iFrameSizeInBytes: 0,
            uiTimeStamp: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SEncoderStatistics {
    pub uiWidth: u32,
    pub uiHeight: u32,
    pub fAverageFrameRate: f32,
    pub fLatestFrameRate: f32,
    pub uiBitRate: u32,
    pub uiAverageFrameQP: u32,
    pub uiInputFrameCount: u32,
    pub uiSkippedFrameCount: u32,
    pub uiResolutionChangeTimes: u32,
    pub uiIDRSentNum: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SBitrateInfo {
    pub iLayer: i32,
    pub iBitrate: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct OpenH264Version {
    pub uMajor: u32,
    pub uMinor: u32,
    pub uRevision: u32,
    pub uReserved: u32,
}

pub use crate::api::codec_api::*;

pub fn split_annexb_units(bitstream: &[u8]) -> Vec<&[u8]> {
    let mut start_indices = Vec::new();
    let mut i = 0;
    while i + 3 < bitstream.len() {
        if bitstream[i] == 0 && bitstream[i + 1] == 0 && bitstream[i + 2] == 0 && bitstream[i + 3] == 1 {
            start_indices.push(i);
            i += 4;
        } else if bitstream[i] == 0 && bitstream[i + 1] == 0 && bitstream[i + 2] == 1 {
            start_indices.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }

    let mut units = Vec::new();
    for idx in 0..start_indices.len() {
        let start = start_indices[idx];
        let end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1]
        } else {
            bitstream.len()
        };
        units.push(&bitstream[start..end]);
    }
    units
}

