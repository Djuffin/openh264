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
use crate::decoder::bit_stream::BsReader;
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

pub const dsErrorFree: i32 = 0x00;
pub const dsFramePending: i32 = 0x01;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDepLayerLost: i32 = 0x08;
pub const dsNoParamSets: i32 = 0x10;
pub const dsDataErrorConcealed: i32 = 0x20;
pub const dsRefListNullPtrs: i32 = 0x40;
pub const dsInvalidArgument: i32 = 0x1000;
pub const dsInitialOptExpected: i32 = 0x2000;
pub const dsOutOfMemory: i32 = 0x4000;
pub const dsDstBufNeedExpan: i32 = 0x8000;

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

// `SDataBuffer { pHead, pEnd, pStartPos, pCurPos }` died at T3.3: the raw bitstream
// buffer is an owned [`RawDataBuffer`] (`decoder::bit_stream`), and positions are
// offsets that survive its growth by definition.
pub use crate::decoder::bit_stream::RawDataBuffer;

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
pub type PGetIntraPred8x8Func =
    Option<unsafe extern "C" fn(pPred: *mut u8, kiLumaStride: i32, bTLAvail: bool, bTRAvail: bool)>;
pub type PCopyFunc = Option<unsafe extern "C" fn(pDst: *mut u8, iStrideD: i32, pSrc: *mut u8, iStrideS: i32)>;

pub type PDeblockingFilterMbFunc =
    Option<unsafe extern "C" fn(pCurDqLayer: *mut c_void, filter: *mut SDeblockingFilter, boundry_flag: i32)>;
// The six deblocking kernel-pointer types and the table that holds them
// (`SDeblockingFunc`, C++ `decoder_context.h:196-209`) are declared **once**, in
// `common/deblocking_common.rs`, where the decoder's `DeblockingInit`
// (`decoder/deblocking.cpp:1338`) was ported alongside its kernels. This module
// re-exports them; it used to redeclare all seven with different parameter names,
// which made two structurally identical types the compiler treated as distinct.
// The encoder's same-named table is a **different** C++ type
// (`encoder/deblocking.cpp:793`, non-`Option` slots) and stays in
// `encoder/deblocking.rs`.
pub use crate::common::deblocking_common::{
    PLumaDeblockingLT4Func, PLumaDeblockingEQ4Func,
    PChromaDeblockingLT4Func, PChromaDeblockingEQ4Func,
    PChromaDeblockingLT4Func2, PChromaDeblockingEQ4Func2,
    SDeblockingFunc,
};
pub type PDeblockingFunc = *mut SDeblockingFunc;

// `PWelsFillNeighborMbInfoIntra4x4Func`, `PWelsMapNeighToSample` and
// `PWelsMap16NeighToSample` were deleted at T4b.3. All three declared
// `pNeighAvail: *mut c_void` and `extern "C"`, neither of which matched the functions
// stored in them -- so every install and every fallback had to launder the mismatch.
// They are one
// `IntraPredConstraint` now; see that type in `decode_slice.rs`.
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


/// The per-slice deblocking scratch — C++ `SDeblockingFilter`
/// (`decoder_context.h:214-223`).
///
/// **Not `#[repr(C)]` since T5.N4**, on T5.C3's reasoning: `ref_ids` is a Rust type
/// with no C layout, the struct crosses no FFI boundary (`PDeblockingFilterMbFunc` is
/// an in-crate fn pointer), and it carries no `assert_size!` or offset pin. A `repr(C)`
/// claim it cannot honour is worse than no claim.
#[derive(Copy, Clone, Default)]
pub struct SDeblockingFilter {
    // T5.N3: `pCsData` and `iCsStride` sat here — three raw plane
    // pointers and two strides copied out of `pCtx->pDec` at filter init and read for
    // the whole macroblock loop. They were the decoder's last plane-pointer mirror
    // (the `pBitStringAux` class, T5.M3), and nothing replaces them: every reader
    // takes `pCurDqLayer`, which carries the picture, so the plane that owns the bytes
    // is the only thing that says where they are. The encoder's same-named struct
    // (`encoder/deblocking.rs:191`) keeps its own `pCsData`; it is a different struct
    // with a different lifecycle and 6.4's to convert.
    pub eSliceType: i32,
    pub iSliceAlphaC0Offset: i8,
    pub iSliceBetaOffset: i8,
    pub iChromaQP: [i8; 2],
    pub iLumaQP: i8,
    pub pLoopf: *mut SDeblockingFunc,

    /// The two reference lists as **identities**, snapshotted at filter init.
    ///
    /// **T5.N4 replaced `pRefPics`**, one raw pointer per list aimed *into*
    /// `pCtx->sRefPic.pRefList` and held for the whole macroblock loop —
    /// F28's shape (something reachable from `pCtx`, stored across calls) with the
    /// borrow spelled as a pointer so nothing had to face it.
    ///
    /// Boundary strength asks one question of these entries and it is plan P3's:
    /// *are these two the same reference picture?* A [`PicId`] answers it directly, so
    /// the erased `c_void` slot the 4x4 paths used to carry them in is gone as well.
    ///
    /// `None` is the C's null slot. Every non-null entry of a reference list is a pool
    /// picture and therefore has a slot — `WelsInitRefList` fills these lists from
    /// `pPicBuff` and from nowhere else — so `Option<PicId>` equality is exactly the
    /// pointer equality it replaces. The snapshot asserts it rather than assuming it.
    ///
    /// Snapshotting is safe because nothing writes a reference list during deblocking:
    /// the macroblock loop calls `WelsDeblockingMb` and nothing else, and list
    /// construction (`InitRefPicList`) ran before the slice decode.
    pub ref_ids: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
}
pub type PDeblockingFilter = *mut SDeblockingFilter;

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



pub use crate::decoder::pic_queue::{PicPool, PicId, SPicBuff, PPicBuff};

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
    pub iStatisticsLogInterval: u32,
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

// The decoder context points at the caller's `SDecodingParam`, so it must be
// the very same type as the public API struct (`codec_app_def.h`).
pub use crate::api::codec_api::SDecodingParam;


pub use crate::decoder::decoder_core::{DqLayerState, PDqLayer, SLayerInfo};


pub use crate::decoder::nalu::{SAccessUnit, PAccessUnit};


// ---------------------------------------------------------------------------
// Master Decoder Context
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct SWelsDecoderContext {
    pub sLogCtx: SLogContext,
    pub pArgDec: *mut c_void,
    pub sRawData: RawDataBuffer,
    pub sSavedData: RawDataBuffer,
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
    pub pDec: *mut Picture,
    pub pTempDec: *mut Picture,
    pub sRefPic: SRefPic,
    pub sTmpRefPic: SRefPic,
    pub pVlcTable: *mut c_void,
    pub sBs: BsReader,
    pub sSpsPpsCtx: SWelsDecoderSpsPpsCTX,
    pub bHasNewSps: bool,
    pub sFrameCrop: SPosOffset,
    pub pSliceHeader: *mut SSliceHeader,
    pub pPicBuff: *mut SPicBuff,
    pub iPicQueueNumber: i32,
    pub pAccessUnitList: *mut SAccessUnit,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    pub pCurDqLayer: *mut DqLayerState,
    pub pDqLayersList: *mut DqLayerState,
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
    // T4b.3b: `sExpandPicFunc: SExpandPicFunc` sat here. Three constant slots,
    // both chroma entries the same function; `common/expand_pic.rs` names the
    // kernels directly now. This struct has no `assert_size!` and no offset pins,
    // so nothing moves with it.
    // T4b.3c: `sBlockFunc: SBlockFunc` sat here -- three slots of which the port
    // and the C++ both read exactly one. See `decode_slice.rs`'s note.
    pub iCurSeqIntervalTargetDependId: i32,
    pub iCurSeqIntervalMaxPicWidth: i32,
    pub iCurSeqIntervalMaxPicHeight: i32,
    /// The three C++ slots `pFillInfoCacheIntraNxNFunc`, `pMapNxNNeighToSampleFunc`
    /// and `pMap16x16NeighToSampleFunc`, which were always set together from one
    /// flag. **T4b.3**; see [`IntraPredConstraint`](crate::decoder::decode_slice::IntraPredConstraint).
    pub eIntraPredConstraint: crate::decoder::decode_slice::IntraPredConstraint,
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
    /// The arithmetic decoding engine, **by value** (T5.O2).
    ///
    /// It was a lazily `WelsMallocz`'d 32-byte allocation of four scalars, made on
    /// the first access unit, freed by `WelsFreeDynamicMemory`, and null-checked at
    /// every one of its ~45 derivation sites. Nothing about it was dynamic: one per
    /// decoder, for the decoder's whole life, sized at compile time. F19's question —
    /// *which line frees this?* — has the best possible answer when there is no
    /// allocation to free.
    ///
    /// Its zero is its initial state in both spellings: `WelsMallocz` zeroed the
    /// block, the context's shell zeroes the field, and the lazy arm re-zeroed
    /// nothing after the first AU either. Consumers take `*mut SWelsCabacDecEngine`
    /// and derive it per use with `addr_of_mut!` (S29), so no borrow of it is ever
    /// live across a call.
    pub sCabacDecEngine: SWelsCabacDecEngine,
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
    pub lastReadyHeightOffset: [[i16; MAX_REF_PIC_COUNT]; LIST_A],
    pub pPictInfoList: *mut SPictInfo,
    pub pPictReoderingStatus: *mut SPictReoderingStatus,
}
pub type PWelsDecoderContext = *mut SWelsDecoderContext;

impl Default for SWelsDecoderContext {
    fn default() -> Self {
        // The context has always been created zeroed (`WelsMallocz` semantics, and
        // every value field's zero is its C default). Since T3.3 two fields own heap
        // allocations, and a zeroed `Vec` is not a valid `Vec` — so those are written
        // through the uninitialized shell before the value materializes, and no
        // invalid value ever exists.
        let mut shell = std::mem::MaybeUninit::<Self>::zeroed();
        Self::make_zeroed_shell_valid(shell.as_mut_ptr());
        unsafe { shell.assume_init() }
    }
}

impl SWelsDecoderContext {
    /// Writes valid values into the `Vec`-bearing fields of a zeroed, not yet
    /// materialized context. Everything else's zero is its C default.
    fn make_zeroed_shell_valid(p: *mut Self) {
        unsafe {
            std::ptr::addr_of_mut!((*p).sRawData).write(RawDataBuffer::default());
            std::ptr::addr_of_mut!((*p).sSavedData).write(RawDataBuffer::default());
        }
    }

    /// A zeroed context constructed **on the heap**: the struct is several MiB, so
    /// `Box::default()`'s by-value path overflows a 2 MiB test-thread stack. This is
    /// the replacement for the `Box::new_zeroed().assume_init()` idiom, which stopped
    /// being legal at T3.3 (a zeroed `Vec` is an invalid value).
    pub fn new_boxed() -> Box<Self> {
        let mut shell = Box::<Self>::new_zeroed();
        Self::make_zeroed_shell_valid(shell.as_mut_ptr());
        unsafe { shell.assume_init() }
    }
}

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
        // `new_zeroed().assume_init()` stopped being legal at T3.3: the context now
        // owns `Vec`s, and a zeroed `Vec` is an invalid value. `Default` constructs
        // the same zeroed context with those fields written properly.
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.pMemAlign = &mut mem_align;

        let mut pic_buff: *mut SPicBuff = std::ptr::null_mut();
        unsafe {
            let err = CreatePicBuff(&mut *ctx as *mut _, &mut pic_buff as *mut _, 4, 64, 64);
            assert_eq!(err, ERR_NONE);
            assert!(!pic_buff.is_null());
            assert_eq!((*pic_buff).capacity(), 4);

            DestroyPicBuff(&mut *ctx as *mut _, &mut pic_buff as *mut _, ctx.pMemAlign);
            assert!(pic_buff.is_null());
        }
    }
}
