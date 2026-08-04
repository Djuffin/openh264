#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! Master decoder execution context, function dispatch tables, DPB memory pool management,
//! bitstream demuxing, and statistics aggregation.
//!
//! Translated from `codec/decoder/core/inc/decoder_context.h` and `codec/decoder/core/src/decoder.cpp`.

use crate::common::memory_align::CMemoryAlign;
use crate::decoder::bit_stream::SBitStringAux;
use crate::decoder::fmo::{PFmo, SFmo};
use crate::decoder::slice::EWelsSliceType;
use std::ffi::{c_char, c_void};


// ---------------------------------------------------------------------------
// Constants & Defines
// ---------------------------------------------------------------------------

pub const MAX_PRED_MODE_ID_I16x16: usize = 3;
pub const MAX_PRED_MODE_ID_CHROMA: usize = 3;
pub const MAX_PRED_MODE_ID_I4x4: usize = 8;
pub const WELS_QP_MAX: usize = 51;

pub const IMinInt32: i32 = -0x7FFFFFFF;

pub const NEW_CTX_OFFSET_MB_TYPE_I: usize = 3;
pub const NEW_CTX_OFFSET_SKIP: usize = 11;
pub const NEW_CTX_OFFSET_SUBMB_TYPE: usize = 21;
pub const NEW_CTX_OFFSET_B_SUBMB_TYPE: usize = 36;
pub const NEW_CTX_OFFSET_MVD: usize = 40;
pub const NEW_CTX_OFFSET_REF_NO: usize = 54;
pub const NEW_CTX_OFFSET_DELTA_QP: usize = 60;
pub const NEW_CTX_OFFSET_IPR: usize = 68;
pub const NEW_CTX_OFFSET_CIPR: usize = 64;
pub const NEW_CTX_OFFSET_CBP: usize = 73;
pub const NEW_CTX_OFFSET_CBF: usize = 85;
pub const NEW_CTX_OFFSET_MAP: usize = 105;
pub const NEW_CTX_OFFSET_LAST: usize = 166;
pub const NEW_CTX_OFFSET_ONE: usize = 227;
pub const NEW_CTX_OFFSET_ABS: usize = 232;
pub const NEW_CTX_OFFSET_TS_8x8_FLAG: usize = 399;
pub const CTX_NUM_MVD: usize = 7;
pub const CTX_NUM_CBP: usize = 4;
pub const NEW_CTX_OFFSET_TRANSFORM_SIZE_8X8_FLAG: usize = 399;
pub const NEW_CTX_OFFSET_MAP_8x8: usize = 402;
pub const NEW_CTX_OFFSET_LAST_8x8: usize = 417;
pub const NEW_CTX_OFFSET_ONE_8x8: usize = 426;
pub const NEW_CTX_OFFSET_ABS_8x8: usize = 431;

pub const SPS_PPS_BS_SIZE: usize = 128;

pub const OVERWRITE_NONE: i32 = 0;
pub const OVERWRITE_PPS: i32 = 1;
pub const OVERWRITE_SPS: i32 = 1 << 1;
pub const OVERWRITE_SUBSETSPS: i32 = 1 << 2;

pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MIN_REF_PIC_COUNT: usize = 1;
pub const MAX_SHORT_REF_COUNT: usize = 16;
pub const MAX_LONG_REF_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = MAX_REF_PIC_COUNT + 1; // 17
pub const MAX_SPS_COUNT: usize = 32;
pub const MAX_PPS_COUNT: usize = 256;
pub const MAX_LAYER_NUM: usize = 8;
pub const LAYER_NUM_EXCHANGEABLE: usize = 1;
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MV_A: usize = 2;
pub const MB_COEFF_LIST_SIZE: usize = 256 + ((8 * 8) << 1); // 384
pub const MB_PARTITION_SIZE: usize = 4;
pub const MB_SUB_PARTITION_SIZE: usize = 4;
pub const WELS_CONTEXT_COUNT: usize = 460;

// Return & Error codes
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_INVALID_PARAM: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = 2;
pub const ERR_INFO_INVALID_PTR: i32 = 3;

pub const dsBitstreamError: i32 = 0x01;
pub const dsNoParamSets: i32 = 0x02;
pub const dsDataErrorConcealed: i32 = 0x04;
pub const dsOutOfMemory: i32 = 0x08;

// Error Concealment Modes
pub const ERROR_CON_DISABLE: i32 = 0;
pub const ERROR_CON_FRAME_COPY: i32 = 1;
pub const ERROR_CON_SLICE_COPY: i32 = 2;
pub const ERROR_CON_FRAME_COPY_CROSS_IDR: i32 = 3;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR: i32 = 4;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 5;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR: i32 = 6;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 7;

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;
pub const SP_SLICE: i32 = 3;
pub const SI_SLICE: i32 = 4;

// Prediction Mode Index Identifiers
pub const I16_PRED_V: usize = 0;
pub const I16_PRED_H: usize = 1;
pub const I16_PRED_DC: usize = 2;
pub const I16_PRED_P: usize = 3;
pub const I16_PRED_DC_L: usize = 4;
pub const I16_PRED_DC_T: usize = 5;
pub const I16_PRED_DC_128: usize = 6;

pub const I4_PRED_V: usize = 0;
pub const I4_PRED_H: usize = 1;
pub const I4_PRED_DC: usize = 2;
pub const I4_PRED_DDL: usize = 3;
pub const I4_PRED_DDR: usize = 4;
pub const I4_PRED_VR: usize = 5;
pub const I4_PRED_HD: usize = 6;
pub const I4_PRED_VL: usize = 7;
pub const I4_PRED_HU: usize = 8;
pub const I4_PRED_DC_L: usize = 9;
pub const I4_PRED_DC_T: usize = 10;
pub const I4_PRED_DC_128: usize = 11;
pub const I4_PRED_DDL_TOP: usize = 12;
pub const I4_PRED_VL_TOP: usize = 13;

pub const C_PRED_DC: usize = 0;
pub const C_PRED_H: usize = 1;
pub const C_PRED_V: usize = 2;
pub const C_PRED_P: usize = 3;
pub const C_PRED_DC_L: usize = 4;
pub const C_PRED_DC_T: usize = 5;
pub const C_PRED_DC_128: usize = 6;

pub const FEEDBACK_VCL_NAL: i32 = 1;

// ---------------------------------------------------------------------------
// CABAC & Bitstream Data Structures
// ---------------------------------------------------------------------------

pub use crate::decoder::cabac_decoder::{SWelsCabacCtx, PWelsCabacCtx, SWelsCabacDecEngine, PWelsCabacDecEngine};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDataBuffer {
    pub pHead: *mut u8,
    pub pEnd: *mut u8,
    pub pStartPos: *mut u8,
    pub pCurPos: *mut u8,
}

impl Default for SDataBuffer {
    fn default() -> Self {
        Self {
            pHead: std::ptr::null_mut(),
            pEnd: std::ptr::null_mut(),
            pStartPos: std::ptr::null_mut(),
            pCurPos: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSpsBsInfo {
    pub pSpsBsBuf: [u8; SPS_PPS_BS_SIZE],
    pub iSpsId: i32,
    pub uiSpsBsLen: u16,
}

impl Default for SSpsBsInfo {
    fn default() -> Self {
        Self {
            pSpsBsBuf: [0u8; SPS_PPS_BS_SIZE],
            iSpsId: 0,
            uiSpsBsLen: 0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SPpsBsInfo {
    pub pPpsBsBuf: [u8; SPS_PPS_BS_SIZE],
    pub iPpsId: i32,
    pub uiPpsBsLen: u16,
}

impl Default for SPpsBsInfo {
    fn default() -> Self {
        Self {
            pPpsBsBuf: [0u8; SPS_PPS_BS_SIZE],
            iPpsId: 0,
            uiPpsBsLen: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Function Pointer Dispatch Types
// ---------------------------------------------------------------------------

pub type PGetIntraPredFunc = Option<unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32)>;
pub type PIdctResAddPredFunc = Option<unsafe extern "C" fn(pPred: *mut u8, kiStride: i32, pRs: *mut i16)>;
pub type PIdctFourResAddPredFunc =
    Option<unsafe extern "C" fn(pPred: *mut u8, iStride: i32, pRs: *mut i16, pNzc: *const i8)>;
pub type PExpandPictureFunc =
    Option<unsafe extern "C" fn(pDst: *mut u8, kiStride: i32, kiPicWidth: i32, kiPicHeight: i32)>;
pub type PGetIntraPred8x8Func =
    Option<unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32, bTLAvail: bool, bTRAvail: bool)>;
pub type PCopyFunc = Option<unsafe extern "C" fn(pDst: *mut u8, iStrideD: i32, pSrc: *mut u8, iStrideS: i32)>;

pub type PDeblockingFilterMbFunc =
    Option<unsafe extern "C" fn(pCurDqLayer: *mut c_void, filter: *mut SDeblockingFilter, boundry_flag: i32)>;
pub type PLumaDeblockingLT4Func =
    Option<unsafe extern "C" fn(iSampleY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32, iTc: *mut i8)>;
pub type PLumaDeblockingEQ4Func =
    Option<unsafe extern "C" fn(iSampleY: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;
pub type PChromaDeblockingLT4Func = Option<
    unsafe extern "C" fn(
        iSampleCb: *mut u8,
        iSampleCr: *mut u8,
        iStride: i32,
        iAlpha: i32,
        iBeta: i32,
        iTc: *mut i8,
    ),
>;
pub type PChromaDeblockingEQ4Func =
    Option<unsafe extern "C" fn(iSampleCb: *mut u8, iSampleCr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;
pub type PChromaDeblockingLT4Func2 =
    Option<unsafe extern "C" fn(iSampleCbr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32, iTc: *mut i8)>;
pub type PChromaDeblockingEQ4Func2 =
    Option<unsafe extern "C" fn(iSampleCbr: *mut u8, iStride: i32, iAlpha: i32, iBeta: i32)>;

pub type PWelsNonZeroCountFunc = Option<unsafe extern "C" fn(pNonZeroCount: *mut i8)>;
pub type PWelsBlockZeroFunc = Option<unsafe extern "C" fn(block: *mut i16, stride: i32)>;

pub type PWelsFillNeighborMbInfoIntra4x4Func = Option<
    unsafe extern "C" fn(
        pNeighAvail: *mut c_void,
        pNonZeroCount: *mut u8,
        pIntraPredMode: *mut i8,
        pCurDqLayer: *mut c_void,
    ),
>;
pub type PWelsMapNeighToSample = Option<unsafe extern "C" fn(pNeighAvail: *mut c_void, pSampleAvail: *mut i32)>;
pub type PWelsMap16NeighToSample = Option<unsafe extern "C" fn(pNeighAvail: *mut c_void, pSampleAvail: *mut u8)>;
pub type PWelsParseIntra4x4ModeFunc = Option<
    unsafe extern "C" fn(
        pNeighAvail: *mut c_void,
        pIntraPredMode: *mut i8,
        pBs: *mut c_void,
        pCurDqLayer: *mut c_void,
    ) -> i32,
>;
pub type PWelsParseIntra16x16ModeFunc =
    Option<unsafe extern "C" fn(pNeighAvail: *mut c_void, pBs: *mut c_void, pCurDqLayer: *mut c_void) -> i32>;

// ---------------------------------------------------------------------------
// Auxiliary Data Structures
// ---------------------------------------------------------------------------

pub use crate::decoder::error_concealment::SCopyFunc;


#[repr(C)]
#[derive(Copy, Clone)]
pub struct SDeblockingFilter {
    pub pCsData: [*mut u8; 3],
    pub iCsStride: [i32; 2],
    pub eSliceType: i32,
    pub iSliceAlphaC0Offset: i8,
    pub iSliceBetaOffset: i8,
    pub iChromaQP: [i8; 2],
    pub iLumaQP: i8,
    pub pLoopf: *mut SDeblockingFunc,
    pub pRefPics: [*mut *mut Picture; LIST_A],
}
pub type PDeblockingFilter = *mut SDeblockingFilter;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDeblockingFunc {
    pub pfLumaDeblockingLT4Ver: PLumaDeblockingLT4Func,
    pub pfLumaDeblockingEQ4Ver: PLumaDeblockingEQ4Func,
    pub pfLumaDeblockingLT4Hor: PLumaDeblockingLT4Func,
    pub pfLumaDeblockingEQ4Hor: PLumaDeblockingEQ4Func,
    pub pfChromaDeblockingLT4Ver: PChromaDeblockingLT4Func,
    pub pfChromaDeblockingEQ4Ver: PChromaDeblockingEQ4Func,
    pub pfChromaDeblockingLT4Hor: PChromaDeblockingLT4Func,
    pub pfChromaDeblockingEQ4Hor: PChromaDeblockingEQ4Func,
    pub pfChromaDeblockingLT4Ver2: PChromaDeblockingLT4Func2,
    pub pfChromaDeblockingEQ4Ver2: PChromaDeblockingEQ4Func2,
    pub pfChromaDeblockingLT4Hor2: PChromaDeblockingLT4Func2,
    pub pfChromaDeblockingEQ4Hor2: PChromaDeblockingEQ4Func2,
}
pub type PDeblockingFunc = *mut SDeblockingFunc;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SBlockFunc {
    pub pWelsSetNonZeroCountFunc: PWelsNonZeroCountFunc,
    pub pWelsBlockZero16x16Func: PWelsBlockZeroFunc,
    pub pWelsBlockZero8x8Func: PWelsBlockZeroFunc,
}

pub use crate::decoder::parameter_sets::SPosOffset;


pub use crate::decoder::parameter_sets::{SSps, SPps, PSps, PPps, SSubsetSps, PSubsetSps};


pub use crate::decoder::nalu::{
    SNalUnitHeader, SNalUnitHeaderExt, SNalUnit, PNalUnit,
};



pub use crate::decoder::slice::{
    SSliceHeader, PSliceHeader, SSliceHeaderExt, PSliceHeaderExt, SRefBasePicMarking, PRefBasePicMarking,
};


#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsDecoderSpsPpsCTX {
    pub sFrameCrop: SPosOffset,
    pub sSpsBuffer: [SSps; MAX_SPS_COUNT + 1],
    pub sPpsBuffer: [SPps; MAX_PPS_COUNT + 1],
    pub sSubsetSpsBuffer: [SSubsetSps; MAX_SPS_COUNT + 1],
    pub sPrefixNal: SNalUnit,
    pub pActiveLayerSps: [*mut SSps; MAX_LAYER_NUM],
    pub bAvcBasedFlag: bool,
    pub bSpsExistAheadFlag: bool,
    pub bSubspsExistAheadFlag: bool,
    pub bPpsExistAheadFlag: bool,
    pub iSpsErrorIgnored: i32,
    pub iSubSpsErrorIgnored: i32,
    pub iPpsErrorIgnored: i32,
    pub bSpsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bSubspsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bPpsAvailFlags: [bool; MAX_PPS_COUNT],
    pub iPPSLastInvalidId: i32,
    pub iPPSInvalidNum: i32,
    pub iSPSLastInvalidId: i32,
    pub iSPSInvalidNum: i32,
    pub iSubSPSLastInvalidId: i32,
    pub iSubSPSInvalidNum: i32,
    pub iSeqId: i32,
    pub iOverwriteFlags: i32,
}
pub type PWelsDecoderSpsPpsCTX = *mut SWelsDecoderSpsPpsCTX;

impl Default for SWelsDecoderSpsPpsCTX {
    fn default() -> Self {
        Self {
            sFrameCrop: SPosOffset::default(),
            sSpsBuffer: [SSps::default(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::default(); MAX_PPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::default(); MAX_SPS_COUNT + 1],
            sPrefixNal: SNalUnit::default(),
            pActiveLayerSps: [std::ptr::null_mut(); MAX_LAYER_NUM],
            bAvcBasedFlag: true,
            bSpsExistAheadFlag: false,
            bSubspsExistAheadFlag: false,
            bPpsExistAheadFlag: false,
            iSpsErrorIgnored: 0,
            iSubSpsErrorIgnored: 0,
            iPpsErrorIgnored: 0,
            bSpsAvailFlags: [false; MAX_SPS_COUNT],
            bSubspsAvailFlags: [false; MAX_SPS_COUNT],
            bPpsAvailFlags: [false; MAX_PPS_COUNT],
            iPPSLastInvalidId: -1,
            iPPSInvalidNum: 0,
            iSPSLastInvalidId: -1,
            iSPSInvalidNum: 0,
            iSubSPSLastInvalidId: -1,
            iSubSPSInvalidNum: 0,
            iSeqId: -1,
            iOverwriteFlags: 0,
        }
    }
}

pub use crate::decoder::picture::{SPicture, PPicture, SPicture as Picture};



pub use crate::decoder::pic_queue::{TagPicBuff, SPicBuff, PPicBuff};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefPic {
    pub pRefList: [[*mut Picture; MAX_DPB_COUNT]; LIST_A],
    pub pShortRefList: [[*mut Picture; MAX_DPB_COUNT]; LIST_A],
    pub pLongRefList: [[*mut Picture; MAX_DPB_COUNT]; LIST_A],
    pub uiRefCount: [u8; LIST_A],
    pub uiShortRefCount: [u8; LIST_A],
    pub uiLongRefCount: [u8; LIST_A],
    pub iMaxLongTermFrameIdx: i32,
}
pub type PRefPic = *mut SRefPic;

impl Default for SRefPic {
    fn default() -> Self {
        Self {
            pRefList: [[std::ptr::null_mut(); MAX_DPB_COUNT]; LIST_A],
            pShortRefList: [[std::ptr::null_mut(); MAX_DPB_COUNT]; LIST_A],
            pLongRefList: [[std::ptr::null_mut(); MAX_DPB_COUNT]; LIST_A],
            uiRefCount: [0; LIST_A],
            uiShortRefCount: [0; LIST_A],
            uiLongRefCount: [0; LIST_A],
            iMaxLongTermFrameIdx: -1,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsLastDecPicInfo {
    pub sLastNalHdrExt: SNalUnitHeaderExt,
    pub sLastSliceHeader: SSliceHeader,
    pub iPrevPicOrderCntMsb: i32,
    pub iPrevPicOrderCntLsb: i32,
    pub pPreviousDecodedPictureInDpb: *mut Picture,
    pub iPrevFrameNum: i32,
    pub bLastHasMmco5: bool,
    pub uiDecodingTimeStamp: u32,
}
pub type PWelsLastDecPicInfo = *mut SWelsLastDecPicInfo;

impl Default for SWelsLastDecPicInfo {
    fn default() -> Self {
        Self {
            sLastNalHdrExt: SNalUnitHeaderExt::default(),
            sLastSliceHeader: SSliceHeader::default(),
            iPrevPicOrderCntMsb: 0,
            iPrevPicOrderCntLsb: 0,
            pPreviousDecodedPictureInDpb: std::ptr::null_mut(),
            iPrevFrameNum: -1,
            bLastHasMmco5: false,
            uiDecodingTimeStamp: 0,
        }
    }
}

pub use crate::api::codec_api::{SBufferInfo, SSysMEMBuffer};

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPictInfo {
    pub sBufferInfo: SBufferInfo,
    pub iPOC: i32,
    pub iPicBuffIdx: i32,
    pub uiDecodingTimeStamp: u32,
    pub iSeqNum: i32,
}
pub type PPictInfo = *mut SPictInfo;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPictReoderingStatus {
    pub iPictInfoIndex: i32,
    pub iMinSeqNum: i32,
    pub iMinPOC: i32,
    pub iNumOfPicts: i32,
    pub iLastWrittenSeqNum: i32,
    pub iLastWrittenPOC: i32,
    pub iLargestBufferedPicIndex: i32,
    pub bHasBSlice: bool,
}
pub type PPictReoderingStatus = *mut SPictReoderingStatus;

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
        unsafe { std::mem::zeroed() }
    }
}

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
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    pub pfLog: Option<unsafe extern "C" fn(pCtx: *mut c_void, iLevel: i32, szFmt: *const c_char)>,
    pub pLogCtx: *mut c_void,
}

impl Default for SLogContext {
    fn default() -> Self {
        Self {
            pfLog: None,
            pLogCtx: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum VIDEO_BITSTREAM_TYPE {
    #[default]
    VIDEO_BITSTREAM_AVC = 0,
    VIDEO_BITSTREAM_SVC = 1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVideoProperty {
    pub size: u32,
    pub eVideoBsType: VIDEO_BITSTREAM_TYPE,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDecodingParam {
    pub pFileNameRestructed: *mut c_char,
    pub uiCpuLoad: u32,
    pub uiTargetDqLayer: u8,
    pub eEcActiveIdc: crate::decoder::error_concealment::ERROR_CON_IDC,

    pub bParseOnly: bool,
    pub iMultipleThreadIdc: u16,
    pub sVideoProperty: SVideoProperty,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMbCache {
    pub pMbType: [*mut u32; LAYER_NUM_EXCHANGEABLE],
    pub pMv: [[*mut [[i16; MV_A]; MB_BLOCK4x4_NUM]; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pRefIndex: [[*mut [i8; MB_BLOCK4x4_NUM]; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pDirect: [*mut [i8; MB_BLOCK4x4_NUM]; LAYER_NUM_EXCHANGEABLE],
    pub pNoSubMbPartSizeLessThan8x8Flag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pTransformSize8x8Flag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pLumaQp: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pChromaQp: [*mut [i8; 2]; LAYER_NUM_EXCHANGEABLE],
    pub pMvd: [[*mut [[i16; MV_A]; MB_BLOCK4x4_NUM]; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pCbfDc: [*mut u16; LAYER_NUM_EXCHANGEABLE],
    pub pNzc: [*mut [i8; 24]; LAYER_NUM_EXCHANGEABLE],
    pub pNzcRs: [*mut [i8; 24]; LAYER_NUM_EXCHANGEABLE],
    pub pScaledTCoeff: [*mut [i16; MB_COEFF_LIST_SIZE]; LAYER_NUM_EXCHANGEABLE],
    pub pIntraPredMode: [*mut [i8; 8]; LAYER_NUM_EXCHANGEABLE],
    pub pIntra4x4FinalMode: [*mut [i8; MB_BLOCK4x4_NUM]; LAYER_NUM_EXCHANGEABLE],
    pub pIntraNxNAvailFlag: [*mut u8; LAYER_NUM_EXCHANGEABLE],
    pub pChromaPredMode: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pCbp: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pMotionPredFlag: [[*mut [u8; MB_PARTITION_SIZE]; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pSubMbType: [*mut [u32; MB_SUB_PARTITION_SIZE]; LAYER_NUM_EXCHANGEABLE],
    pub pSliceIdc: [*mut i32; LAYER_NUM_EXCHANGEABLE],
    pub pResidualPredFlag: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pInterPredictionDoneFlag: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pMbCorrectlyDecodedFlag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pMbRefConcealedFlag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub iMbWidth: u32,
    pub iMbHeight: u32,
}

impl Default for SMbCache {
    fn default() -> Self {
        Self {
            pMbType: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMv: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pRefIndex: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pDirect: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNoSubMbPartSizeLessThan8x8Flag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pTransformSize8x8Flag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pLumaQp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pChromaQp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMvd: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pCbfDc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNzc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNzcRs: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pScaledTCoeff: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntraPredMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntra4x4FinalMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntraNxNAvailFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pChromaPredMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pCbp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMotionPredFlag: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pSubMbType: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pSliceIdc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pResidualPredFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pInterPredictionDoneFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMbCorrectlyDecodedFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMbRefConcealedFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            iMbWidth: 0,
            iMbHeight: 0,
        }
    }
}

pub use crate::decoder::decoder_core::{SDqLayer, PDqLayer, SLayerInfo};


pub use crate::decoder::nalu::{SAccessUnit, PAccessUnit};


// ---------------------------------------------------------------------------
// Master Decoder Context
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct SWelsDecoderContext {
    pub sLogCtx: SLogContext,
    pub pArgDec: *mut c_void,
    pub sRawData: SDataBuffer,
    pub sSavedData: SDataBuffer,
    pub pParam: *mut SDecodingParam,
    pub uiCpuFlag: u32,
    pub eVideoType: VIDEO_BITSTREAM_TYPE,
    pub bHaveGotMemory: bool,
    pub iImgWidthInPixel: i32,
    pub iImgHeightInPixel: i32,
    pub iLastImgWidthInPixel: i32,
    pub iLastImgHeightInPixel: i32,
    pub bFreezeOutput: bool,
    pub sCurNalHead: SNalUnitHeader,
    pub eSliceType: EWelsSliceType,

    pub bUsedAsRef: bool,
    pub iFrameNum: i32,
    pub iErrorCode: i32,
    pub sFmoList: [SFmo; MAX_PPS_COUNT],
    pub pFmo: PFmo,
    pub iActiveFmoNum: i32,
    pub iDecBlockOffsetArray: [i32; 24],
    pub sMb: SMbCache,
    pub pDec: *mut Picture,
    pub pTempDec: *mut Picture,
    pub sRefPic: SRefPic,
    pub sTmpRefPic: SRefPic,
    pub pVlcTable: *mut c_void,
    pub sBs: SBitStringAux,
    pub iMaxBsBufferSizeInByte: i32,
    pub sSpsPpsCtx: SWelsDecoderSpsPpsCTX,
    pub bHasNewSps: bool,
    pub sFrameCrop: SPosOffset,
    pub pSliceHeader: *mut SSliceHeader,
    pub pPicBuff: *mut SPicBuff,
    pub iPicQueueNumber: i32,
    pub pAccessUnitList: *mut SAccessUnit,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    pub pCurDqLayer: *mut SDqLayer,
    pub pDqLayersList: [*mut SDqLayer; LAYER_NUM_EXCHANGEABLE],
    pub pNalCur: *mut SNalUnit,
    pub uiNalRefIdc: u8,
    pub iPicWidthReq: i32,
    pub iPicHeightReq: i32,
    pub uiTargetDqId: u8,
    pub bEndOfStreamFlag: bool,
    pub bInstantDecFlag: bool,
    pub bInitialDqLayersMem: bool,
    pub bOnlyOneLayerInCurAuFlag: bool,
    pub bReferenceLostAtT0Flag: bool,
    pub iTotalNumMbRec: i32,
    pub bParamSetsLostFlag: bool,
    pub bCurAuContainLtrMarkSeFlag: bool,
    pub iFrameNumOfAuMarkedLtr: i32,
    pub uiCurIdrPicId: u16,
    pub bNewSeqBegin: bool,
    pub bNextNewSeqBegin: bool,
    pub pStreamSeqNum: *mut i32,
    pub iSeqNum: i32,
    pub bFramePending: bool,
    pub bFrameFinish: bool,
    pub iNalNum: i32,
    pub iMaxNalNum: i32,
    pub sSpsBsInfo: [SSpsBsInfo; MAX_SPS_COUNT],
    pub sSubsetSpsBsInfo: [SSpsBsInfo; MAX_PPS_COUNT],
    pub sPpsBsInfo: [SPpsBsInfo; MAX_PPS_COUNT],
    pub pParserBsInfo: *mut SParserBsInfo,
    pub pGetI16x16LumaPredFunc: [PGetIntraPredFunc; 7],
    pub pGetI4x4LumaPredFunc: [PGetIntraPredFunc; 14],
    pub pGetIChromaPredFunc: [PGetIntraPredFunc; 7],
    pub pIdctResAddPredFunc: PIdctResAddPredFunc,
    pub pIdctFourResAddPredFunc: PIdctFourResAddPredFunc,
    pub sMcFunc: crate::decoder::error_concealment::SMcFunc,
    pub pGetI8x8LumaPredFunc: [PGetIntraPred8x8Func; 14],
    pub pIdctResAddPredFunc8x8: PIdctResAddPredFunc,
    pub sCopyFunc: SCopyFunc,
    pub sDeblockingFunc: SDeblockingFunc,
    pub sExpandPicFunc: crate::decoder::decoder_core::SExpandPicFunc,
    pub sBlockFunc: SBlockFunc,
    pub iCurSeqIntervalTargetDependId: i32,
    pub iCurSeqIntervalMaxPicWidth: i32,
    pub iCurSeqIntervalMaxPicHeight: i32,
    pub pFillInfoCacheIntraNxNFunc: PWelsFillNeighborMbInfoIntra4x4Func,
    pub pMapNxNNeighToSampleFunc: PWelsMapNeighToSample,
    pub pMap16x16NeighToSampleFunc: PWelsMap16NeighToSample,
    pub iFeedbackVclNalInAu: i32,
    pub iFeedbackTidInAu: i32,
    pub iFeedbackNalRefIdc: i32,
    pub bAuReadyFlag: bool,
    pub bPrintFrameErrorTraceFlag: bool,
    pub iIgnoredErrorInfoPacketCount: i32,
    pub pTraceHandle: *mut c_void,
    pub pLastDecPicInfo: *mut SWelsLastDecPicInfo,
    pub sWelsCabacContexts: [[[SWelsCabacCtx; WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
    pub bCabacInited: bool,
    pub pCabacCtx: [SWelsCabacCtx; WELS_CONTEXT_COUNT],
    pub pCabacDecEngine: *mut SWelsCabacDecEngine,
    pub dDecTime: f64,
    pub pDecoderStatistics: *mut SDecoderStatistics,
    pub iMbEcedNum: i32,
    pub iMbEcedPropNum: i32,
    pub iMbNum: i32,
    pub bMbRefConcealed: bool,
    pub bRPLRError: bool,
    pub iECMVs: [[i32; 2]; 16],
    pub pECRefPic: [*mut Picture; 16],
    pub uiTimeStamp: u64,
    pub uiDecodingTimeStamp: u32,
    pub pDequant_coeff_buffer4x4: [[[u16; 16]; 52]; 6],
    pub pDequant_coeff_buffer8x8: [[[u16; 64]; 52]; 6],
    pub pDequant_coeff4x4: [*mut [u16; 16]; 6],
    pub pDequant_coeff8x8: [*mut [u16; 64]; 6],
    pub iDequantCoeffPpsid: i32,
    pub bDequantCoeff4x4Init: bool,
    pub bUseScalingList: bool,
    pub pMemAlign: *mut CMemoryAlign,
    pub pThreadCtx: *mut c_void,
    pub pLastThreadCtx: *mut c_void,
    pub pCsDecoder: *mut c_void,
    pub lastReadyHeightOffset: [[i16; MAX_REF_PIC_COUNT]; LIST_A],
    pub pPictInfoList: *mut SPictInfo,
    pub pPictReoderingStatus: *mut SPictReoderingStatus,
}
pub type PWelsDecoderContext = *mut SWelsDecoderContext;

impl Default for SWelsDecoderContext {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Multithreading Structs
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsDecThreadInfo {
    pub sIsBusy: *mut c_void,
    pub sIsActivated: *mut c_void,
    pub sIsIdle: *mut c_void,
    pub sThrHandle: *mut c_void,
    pub uiCommand: u32,
    pub uiThrNum: u32,
    pub uiThrMaxNum: u32,
    pub uiThrStackSize: u32,
    pub pThrProcMain: Option<unsafe extern "C" fn(pArg: *mut c_void) -> *mut c_void>,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsDecoderThreadCTX {
    pub sThreadInfo: SWelsDecThreadInfo,
    pub pCtx: *mut SWelsDecoderContext,
    pub threadCtxOwner: *mut c_void,
    pub kpSrc: *mut u8,
    pub kiSrcLen: i32,
    pub ppDst: *mut *mut u8,
    pub sDstInfo: SBufferInfo,
    pub pDec: *mut Picture,
    pub sImageReady: *mut c_void,
    pub sSliceDecodeStart: *mut c_void,
    pub sSliceDecodeFinish: *mut c_void,
    pub iPicBuffIdx: i32,
}
pub type PWelsDecoderThreadCTX = *mut SWelsDecoderThreadCTX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::decode_mb_aux::IdctResAddPred_c;
    use crate::decoder::pic_queue::{CreatePicBuff, DestroyPicBuff};

    #[test]
    fn test_decoder_constants() {
        assert_eq!(MAX_PRED_MODE_ID_I16x16, 3);
        assert_eq!(MAX_PRED_MODE_ID_CHROMA, 3);
        assert_eq!(MAX_PRED_MODE_ID_I4x4, 8);
        assert_eq!(WELS_QP_MAX, 51);
        assert_eq!(IMinInt32, -0x7FFFFFFF);
        assert_eq!(MAX_DPB_COUNT, 17);
    }

    #[test]
    fn test_idct_res_add_pred_c() {
        let mut pred = [128u8; 16];
        let mut rs = [0i16; 16];
        unsafe {
            IdctResAddPred_c(pred.as_mut_ptr(), 4, rs.as_mut_ptr());
        }
        for &val in &pred {
            assert_eq!(val, 128);
        }
    }

    #[test]
    fn test_sps_pps_defaults() {
        let mut sps_pps_ctx = SWelsDecoderSpsPpsCTX::default();
        sps_pps_ctx.bAvcBasedFlag = true;
        sps_pps_ctx.iPPSLastInvalidId = -1;
        sps_pps_ctx.iSPSLastInvalidId = -1;
        assert!(sps_pps_ctx.bAvcBasedFlag);
        assert_eq!(sps_pps_ctx.iPPSLastInvalidId, -1);
        assert_eq!(sps_pps_ctx.iSPSLastInvalidId, -1);
    }

    #[test]
    fn test_pic_buff_creation_and_destruction() {
        let mut mem_align = CMemoryAlign::new(16);
        let mut ctx = unsafe { Box::<SWelsDecoderContext>::new_zeroed().assume_init() };
        ctx.pMemAlign = &mut mem_align;

        let mut pic_buff: *mut SPicBuff = std::ptr::null_mut();
        unsafe {
            let err = CreatePicBuff(&mut *ctx as *mut _, &mut pic_buff as *mut _, 4, 64, 64);
            assert_eq!(err, ERR_NONE);
            assert!(!pic_buff.is_null());
            assert_eq!((*pic_buff).iCapacity, 4);

            DestroyPicBuff(&mut *ctx as *mut _, &mut pic_buff as *mut _, ctx.pMemAlign);
            assert!(pic_buff.is_null());
        }
    }
}
