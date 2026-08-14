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



pub use crate::decoder::pic_queue::{PicPool, PicId, PicRefs, SPicBuff, PPicBuff};

/// The decoder picture buffer's three lists, as **slot handles**.
///
/// **T5.P′2 (W2b) turned all three from raw picture pointers into `Option<PicId>`**, the
/// shape T5.N4 gave `SDeblockingFilter::ref_ids` and T5.P2 gave `pDec`/`pECRefPic`.
/// These were the last raw aliases *into* the pool, and the ordering rule
/// (phase5.md) is what makes them W3's precondition: a safe container may not lend
/// while raw aliases into it are live, so `Pool<Box<SPicture>>` could not exist
/// while sixteen entries per list held slot addresses across a whole access unit.
///
/// `None` is the C's null slot. Every non-null entry is a pool picture — the lists
/// are filled from `pPicBuff` and from nowhere else, which is the invariant T5.N4's
/// `snapshot_ref_ids` asserted before this conversion and
/// [`insert_ref`](crate::decoder::manage_dec_ref::insert_ref) asserts after it — so
/// `Option<PicId>` equality is exactly the pointer equality it replaces.
///
/// **`#[repr(C)]` came off with the pointers**, on T5.N4's reasoning at
/// `SDeblockingFilter`: `Option<PicId>` is a Rust type with no C layout, the struct
/// crosses no FFI boundary, and it carries no `assert_size!`. A layout claim the
/// struct cannot honour is worse than no claim.
#[derive(Debug, Copy, Clone)]
pub struct SRefPic {
    pub pRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub pShortRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub pLongRefList: [[Option<PicId>; MAX_DPB_COUNT]; LIST_A],
    pub uiRefCount: [u8; LIST_A],
    pub uiShortRefCount: [u8; LIST_A],
    pub uiLongRefCount: [u8; LIST_A],
    pub iMaxLongTermFrameIdx: i32,
}
pub type PRefPic = *mut SRefPic;

impl Default for SRefPic {
    fn default() -> Self {
        Self {
            pRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pShortRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pLongRefList: [[None; MAX_DPB_COUNT]; LIST_A],
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
    /// The last picture handed to the DPB, as a slot handle (**T5.P′2**). It is
    /// always `(*pCtx).pDec` at the moment of the write, so it converts with the
    /// field it copies.
    pub pPreviousDecodedPictureInDpb: Option<PicId>,
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
            pPreviousDecodedPictureInDpb: None,
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


pub use crate::decoder::nalu::SAccessUnit;

/// The access unit under construction, borrowed for the expression that asks.
///
/// The one way to reach [`SWelsDecoderContext::access_unit`], and the reason it is
/// a function rather than a field read: every call derives a fresh `&mut` through
/// the owning `Box`, which retags the access unit and pops whatever the last
/// derivation handed out. Bind the result only across code that cannot derive
/// again — which in practice means "across no call that takes `pCtx`".
///
/// The autoref is on the *field*, so the retag covers one word of the context and
/// not the context (S29); writes to other fields through `pCtx` while this borrow
/// is live are what the field-precise spelling exists to allow.
///
/// `None` before `WelsInitStaticMemory` and after `WelsFreeStaticMemory` — the
/// two states the old `.is_null()` guards were testing for.
#[inline]
pub unsafe fn cur_au<'a>(pCtx: PWelsDecoderContext) -> Option<&'a mut SAccessUnit> {
    if pCtx.is_null() {
        return None;
    }
    (*pCtx).access_unit.as_deref_mut()
}

/// The picture a slot handle names, or null when there is none.
///
/// **The one place the decoder turns an id back into a picture** (T5.P′2 collected
/// it; T5.P2 wrote the first two copies of the body). Every field that used to hold
/// a raw picture pointer into the pool now holds one of these handles, and every one of
/// them resolves here.
///
/// **T5.Q2: it derives, and the derivation is shared.** Until the flip this copied a
/// stored slot pointer, so the result carried `AllocPicture`'s provenance and no
/// later borrow of the pool could touch it. The pool owns now, so every resolution
/// borrows: two live results for *one* slot are two derivations of one allocation and
/// the later invalidates the earlier.
///
/// Shared is the default because shared results **coexist** — the reference-side
/// readers hold several at once (`CreateImplicitWeightTable` compares a LIST_0 entry
/// against a LIST_1 entry, and the two can legally be one picture) and none of them
/// writes. The paths that do write take [`pool_pic_mut`] below, and the compiler
/// enumerates them: that is the discriminator W3's settlement asks each of the write
/// sites to be read against, applied by the type system instead of by eye.
#[inline]
pub unsafe fn pool_pic(pCtx: PWelsDecoderContext, slot: Option<PicId>) -> *const SPicture {
    match (slot, (*pCtx).pPicBuff.as_deref()) {
        (Some(id), Some(pool)) => pool.slot(id),
        _ => std::ptr::null(),
    }
}

/// [`pool_pic`]'s mutable form, for the paths that write through what they resolve.
///
/// **One live result at a time**, and it is the caller's job to keep it that way: a
/// result that outlives the expression it was taken in, across another resolution of
/// the same slot, is the conflict the flip is about. Where a scope needs one picture
/// mutably *and* others readably, the answer is a bracket
/// ([`cur_and_refs`]), not two calls here.
#[inline]
pub unsafe fn pool_pic_mut(pCtx: PWelsDecoderContext, slot: Option<PicId>) -> PPicture {
    match (slot, (*pCtx).pPicBuff.as_deref_mut()) {
        (Some(id), Some(pool)) => pool.slot_mut(id),
        _ => std::ptr::null_mut(),
    }
}

/// The pool itself, for the four operations that act on the *container* rather than
/// on one picture: both prefetch scans, the capacity read, and the api layer's
/// buffered-picture release.
///
/// The autoref is on the field (S29), so the borrow covers one word of the context
/// and ends with the caller's expression — which is the whole discipline the owned
/// field asks for, and the reason [`pool_pic`] above takes its own shared borrow
/// rather than being handed one.
#[inline]
pub unsafe fn pic_pool_mut<'a>(pCtx: PWelsDecoderContext) -> Option<&'a mut SPicBuff> {
    if pCtx.is_null() {
        return None;
    }
    (*pCtx).pPicBuff.as_deref_mut()
}

/// The layer the access unit is decoding, or null when there is none.
///
/// **The one way to reach [`SWelsDecoderContext::pDqLayersList`]** (T5.R2), and the
/// shape [`pic_pool_ptr`] below already takes for the pool: the autoref is on the
/// *field*, so the retag covers one word of the context rather than the context
/// (S29), and the `&mut` it derives is discarded into a pointer the whole
/// macroblock tree can carry as a parameter.
///
/// The S29 condition this site has to meet is that **no second derivation happens
/// while a result is live**, and T5.R1 is what makes that checkable: the layer is
/// threaded from three bracket tops — `DecodeCurrentAccessUnit`'s loop,
/// `CheckAndFinishLastPic`'s concealment block, and `InitialDqLayersContext`'s own
/// construction — and nothing below a bracket reads the field at all. The brackets
/// do not nest: `CheckAndFinishLastPic` derives *after* the `ConstructAccessUnit`
/// call that contains the loop returns.
#[inline]
pub unsafe fn cur_dq_layer(pCtx: PWelsDecoderContext) -> *mut DqLayerState {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    match (*pCtx).pDqLayersList.as_deref_mut() {
        Some(layer) => layer as *mut DqLayerState,
        None => std::ptr::null_mut(),
    }
}

/// [`pic_pool_mut`] as a raw pointer, for the api layer's two release paths.
///
/// `CWelsDecoder::ReleaseBufferedReadyPicture*` evaluate `pCtx ? pCtx->pPicBuff :
/// m_pPicBuff` into one local and pass it on, and `m_pPicBuff` is a raw field of
/// `CWelsDecoderImpl` that Phase 8 owns — so the local is a pointer or it is two
/// shapes at once. The reference this derives from covers the whole `PicPool`
/// allocation and nothing re-derives the pool between the two (the code in between
/// reads `sPictInfoList`), which is the S29 condition this site has to meet.
#[inline]
pub unsafe fn pic_pool_ptr(pCtx: PWelsDecoderContext) -> PPicBuff {
    match pic_pool_mut(pCtx) {
        Some(pool) => pool as *mut SPicBuff,
        None => std::ptr::null_mut(),
    }
}

/// The bracket top's pool borrow, handed down as a [`PicRefs`] (T5.P″2).
///
/// Every scope that resolves more than one handle takes this once at its top and
/// threads it; below a bracket top, `pRefs.get(id)` is the only way back to a
/// picture and `(*pCtx).pPicBuff` is not read at all. That invariant is what makes
/// the flip a change of two lines per bracket instead of a change at every use.
#[inline]
pub unsafe fn pic_refs<'a>(pCtx: PWelsDecoderContext) -> PicRefs<'a> {
    PicRefs::over(if pCtx.is_null() {
        None
    } else {
        (*pCtx).pPicBuff.as_deref()
    })
}

/// **The bracket top, both halves from one borrow** (T5.Q2).
///
/// The two lines this replaces — `dec_pic(pCtx)` then `pic_refs(pCtx)` — were two
/// derivations of the same pool, which under `PPicture` slots was two pointer copies
/// and under owned slots is a `&mut` into one picture beside a shared borrow that can
/// reach the same one. [`PicPool::cur_and_rest`] splits the slot span instead, so the
/// current picture and the view of every other slot are the two halves of a single
/// borrow and the disjointness is proved rather than assumed.
///
/// The `None` arms are the states that were reachable before the flip and stay
/// reachable after it: no pool (before `CreatePicBuff`, after `DestroyPicBuff`) and no
/// current picture (a bracket opened before the AU loop prefetched one). Both answer
/// exactly as `dec_pic`/`pic_refs` did — a null picture and a view that resolves
/// everything to null or to the whole pool.
#[inline]
pub unsafe fn cur_and_refs<'a>(pCtx: PWelsDecoderContext) -> (PPicture, PicRefs<'a>) {
    pic_and_refs(pCtx, if pCtx.is_null() { None } else { (*pCtx).pDec })
}

/// [`cur_and_refs`] for a bracket whose mutable half is **not** `pCtx->pDec`: the
/// error-concealment prefetch, which writes into the slot it just took from the pool
/// while reading the previous DPB picture out of another one.
#[inline]
pub unsafe fn pic_and_refs<'a>(
    pCtx: PWelsDecoderContext,
    slot: Option<PicId>,
) -> (PPicture, PicRefs<'a>) {
    if pCtx.is_null() {
        return (std::ptr::null_mut(), PicRefs::over(None));
    }
    match ((*pCtx).pPicBuff.as_deref_mut(), slot) {
        (Some(pool), Some(id)) => pool.cur_and_rest(id),
        (Some(pool), None) => (std::ptr::null_mut(), pool.refs()),
        (None, _) => (std::ptr::null_mut(), PicRefs::over(None)),
    }
}

/// Entry `i` of reference list `list` — the **handle**, without touching the pool.
///
/// The lists live in the context, not in the pool, so reading one below a bracket
/// top is not a pool access: `pRefs.get(ref_id(pCtx, list, i))` is [`ref_pic`] split
/// at exactly the line the flip moves.
#[inline]
pub unsafe fn ref_id(pCtx: PWelsDecoderContext, list: usize, i: usize) -> Option<PicId> {
    (*pCtx).sRefPic.pRefList[list][i]
}

/// The picture being decoded into — **the write target**, so this one is mutable.
#[inline]
pub unsafe fn dec_pic(pCtx: PWelsDecoderContext) -> PPicture {
    pool_pic_mut(pCtx, (*pCtx).pDec)
}

/// Entry `i` of reference list `list` — `sRefPic.pRefList[list][i]` resolved.
#[inline]
pub unsafe fn ref_pic(pCtx: PWelsDecoderContext, list: usize, i: usize) -> *const SPicture {
    pool_pic(pCtx, (*pCtx).sRefPic.pRefList[list][i])
}

/// Entry `i` of the short-term list — `sRefPic.pShortRefList[LIST_0][i]` resolved.
#[inline]
pub unsafe fn short_ref_pic(pCtx: PWelsDecoderContext, i: usize) -> *const SPicture {
    pool_pic(pCtx, (*pCtx).sRefPic.pShortRefList[LIST_0][i])
}

/// [`short_ref_pic`]'s mutable form — `WrapShortRefPicNum`'s per-entry stamp.
#[inline]
pub unsafe fn short_ref_pic_mut(pCtx: PWelsDecoderContext, i: usize) -> PPicture {
    pool_pic_mut(pCtx, (*pCtx).sRefPic.pShortRefList[LIST_0][i])
}

/// Entry `i` of the long-term list — `sRefPic.pLongRefList[LIST_0][i]` resolved.
#[inline]
pub unsafe fn long_ref_pic(pCtx: PWelsDecoderContext, i: usize) -> *const SPicture {
    pool_pic(pCtx, (*pCtx).sRefPic.pLongRefList[LIST_0][i])
}

/// The previous decoded picture's **handle**, without touching the pool — the
/// `ref_id`-shaped half of [`prev_dpb_pic`], for the error-concealment brackets that
/// resolve it through their own [`PicRefs`].
#[inline]
pub unsafe fn prev_dpb_id(pCtx: PWelsDecoderContext) -> Option<PicId> {
    if pCtx.is_null() || (*pCtx).pLastDecPicInfo.is_null() {
        return None;
    }
    (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb
}

/// [`prev_dpb_pic`]'s mutable form — the api layer's buffering path, which takes a
/// DPB reference on it (`iRefCount += 1`).
#[inline]
pub unsafe fn prev_dpb_pic_mut(pCtx: PWelsDecoderContext) -> PPicture {
    if (*pCtx).pLastDecPicInfo.is_null() {
        return std::ptr::null_mut();
    }
    pool_pic_mut(pCtx, (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb)
}

/// Whether an access unit exists and has at least one NAL queued in it.
///
/// The `!pCurAu.is_null() && pCurAu->uiAvailUnitsNum > 0` guard, which the port
/// spelled at eight sites and which is the actual question every one of them asks.
#[inline]
pub unsafe fn au_has_nals(pCtx: PWelsDecoderContext) -> bool {
    matches!(cur_au(pCtx), Some(au) if au.uiAvailUnitsNum > 0)
}

/// Ends the access unit at the last NAL parsed and flags it ready for decode.
///
/// The C++ writes `pCurAu->uiEndPos = pCurAu->uiAvailUnitsNum - 1; pCtx->bAuReadyFlag
/// = true;` at five sites in `nalu.rs` alone, each behind its own spelling of
/// [`au_has_nals`]. Returns whether there was an access unit to end.
///
/// The two stores are to disjoint fields with nothing read between them, so their
/// order is not observable; this one writes the access unit first, which is the
/// ordering discipline T5.O8 cost a Miri round trip to learn.
#[inline]
pub unsafe fn mark_au_ready(pCtx: PWelsDecoderContext) -> bool {
    match cur_au(pCtx) {
        Some(au) if au.uiAvailUnitsNum > 0 => {
            au.uiEndPos = au.uiAvailUnitsNum - 1;
            (*pCtx).bAuReadyFlag = true;
            true
        }
        _ => false,
    }
}


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
    /// The pool slot being decoded into — a [`PicId`] since T5.P2, reached with
    /// [`dec_pic`].
    ///
    /// It was `*mut Picture`, and it was the alias plan §2.2.3's split-borrow API has
    /// been waiting on since session N: a pointer *into* a pool slot, live for a whole
    /// access unit, so a pool that owned its pictures could never lend one. A slot
    /// handle is not an alias, and `dec_pic` copies the stored pointer at the use.
    ///
    /// `None` is the C's null — "no picture in flight" — and it is the same three-state
    /// control flow (`is_null` tests before a prefetch, after a construction failure,
    /// at reset) with the state named instead of encoded.
    pub pDec: Option<PicId>,
    /// The B-slice temporal-direct scratch picture — **owned since T5.P″1**.
    ///
    /// **Not a pool slot**: it is allocated on its own at `decode_slice.rs`'s first B
    /// macroblock and released by `WelsFreeDynamicMemory`, so it has no `PicId` and
    /// cannot take one. That is exactly why it is W1's shape rather than W3's: one
    /// `Box::into_raw`/`from_raw` pair with no second carrier, and
    /// [`pic_queue::alloc_picture`](crate::decoder::pic_queue::alloc_picture) is its
    /// constructor.
    ///
    /// F19: dropped by `WelsFreeDynamicMemory`'s `= None` and, failing that, by the
    /// context's own drop glue — which is R4's equivalence, and the reason the
    /// explicit `FreePicture` call could go.
    ///
    /// `None` is the C's null: "not allocated yet", the state the lazy allocation
    /// tests for.
    pub pTempDec: Option<Box<Picture>>,
    pub sRefPic: SRefPic,
    pub sTmpRefPic: SRefPic,
    pub pVlcTable: *mut c_void,
    pub sBs: BsReader,
    pub sSpsPpsCtx: SWelsDecoderSpsPpsCTX,
    pub bHasNewSps: bool,
    pub sFrameCrop: SPosOffset,
    pub pSliceHeader: *mut SSliceHeader,
    /// The decoded-picture pool — **owned since T5.P″1**.
    ///
    /// It was `*mut SPicBuff`, one `Box::into_raw` in `CreatePicBuff` reclaimed by one
    /// `Box::from_raw` in `DestroyPicBuff`. Owning it here is safe **before** W3's flip
    /// and not after, which is the ordering the phase runs on: while the slots are
    /// `PPicture`, a [`pool_pic`] result carries `AllocPicture`'s provenance and not
    /// this `Box`'s (T5.N1's invariant), so the pool borrow this field now takes ends
    /// inside the accessor and no picture pointer descends from it.
    ///
    /// F19: dropped by the context's drop glue; `WelsFreeDynamicMemory` still calls
    /// `DestroyPicBuff` because that function also frees the pictures the pool
    /// addresses and runs F37's reordering reset.
    pub pPicBuff: Option<Box<SPicBuff>>,
    pub iPicQueueNumber: i32,
    /// The access unit under construction — owned since T5.P1.
    ///
    /// **Reach it with [`cur_au`], never by hoisting.** The `Box` means each
    /// derivation retags the access unit, so a borrow (or a pointer taken from
    /// one) held across a second derivation is popped: T5.O7's conviction, one
    /// level up from where it convicted. The nodes are unaffected — they are
    /// their own allocations since T5.O4, which is the only reason this field
    /// could own at all while `pNalCur` and the slice parser's `&mut BsCursor`
    /// still point into them.
    ///
    /// F19: dropped by the context's own drop glue. `WelsFreeStaticMemory`'s
    /// explicit `MemFreeNalList` is gone with the raw pointer, and R4's
    /// equivalence argument holds by construction — that cascade ran on the
    /// line before `drop(Box::from_raw(pCtx))` at both of its call sites.
    pub access_unit: Option<Box<SAccessUnit>>,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    /// The one layer, **owned** (T5.R2), and reached only through [`cur_dq_layer`].
    ///
    /// `pCurDqLayer` was this field's cache and died at T5.R1: it had one production
    /// stamp (`= pDqLayersList`, under `bInitialDqLayersMem || is_null()`) and
    /// `LAYER_NUM_EXCHANGEABLE` is 1, so the two always named one layer — except in
    /// the window `UninitialDqLayersContext` opened, where the list was nulled and the
    /// cache was left dangling. That deletion is what lets this field own: a stored
    /// derivation through an owning `Box` is invalidated by the next derivation, and
    /// there are now **three** derivations on the whole decode path — the access-unit
    /// loop's, `CheckAndFinishLastPic`'s concealment block, and the allocator's own —
    /// none of them nested inside another.
    ///
    /// `None` is the C's null list: before `InitialDqLayersContext` and after
    /// `UninitialDqLayersContext`.
    pub pDqLayersList: Option<Box<DqLayerState>>,
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
    /// nothing after the first AU either. Consumers still take the engine by raw
    /// pointer and derive it per use with `addr_of_mut!` (S29), so no borrow of it is
    /// ever live across a call — which matters more now that it lives inside the
    /// context and the CABAC path reaches other fields between engine writes.
    ///
    /// (S16: that sentence names no pointer type on purpose. The field declaration
    /// this replaced was one `raw_ptr` occurrence, and a comment describing it would
    /// have put the occurrence straight back — the tenth time this phase has collected
    /// the prose floor, and the third in this session.)
    pub sCabacDecEngine: SWelsCabacDecEngine,
    pub dDecTime: f64,
    pub pDecoderStatistics: *mut SDecoderStatistics,
    pub iMbEcedNum: i32,
    pub iMbEcedPropNum: i32,
    pub iMbNum: i32,
    pub bMbRefConcealed: bool,
    pub bRPLRError: bool,
    pub iECMVs: [[i32; 2]; 16],
    /// The concealment reference list — pool slots, so [`PicId`]s since T5.P2 for the
    /// same reason as [`pDec`](Self::pDec): every entry is a picture from
    /// `sRefPic.pRefList`, and holding pointers to sixteen of them is sixteen more
    /// aliases the pool would have to outlive.
    pub pECRefPic: [Option<PicId>; 16],
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
            // S21, T5.P1. `Option<Box<T>>`'s `None` *is* all-zero via the null-pointer
            // niche, so this write is redundant today — and it is here anyway, because
            // the alternative is a field whose validity rests on a layout guarantee
            // that no test states and no reader of the shell can see.
            std::ptr::addr_of_mut!((*p).access_unit).write(None);
            // S21, T5.P″1: the same clause for the two owned pictures/pool. Each was a
            // raw pointer whose zero *was* its null; the write says so rather than
            // relying on it.
            std::ptr::addr_of_mut!((*p).pTempDec).write(None);
            std::ptr::addr_of_mut!((*p).pPicBuff).write(None);
            // S21, T5.R2: the layer joins them. Its zero was a null `PDqLayer` and is
            // now `None` through the same niche; the write states it either way.
            std::ptr::addr_of_mut!((*p).pDqLayersList).write(None);
        }
    }

    /// A zeroed context constructed **on the heap**: the struct is several MiB, so
    /// `Box::default()`'s by-value path overflows a 2 MiB test-thread stack. This is
    /// the replacement for the `Box::new_zeroed().assume_init()` idiom, which stopped
    /// being legal at T3.3 (a zeroed `Vec` is an invalid value).
    ///
    /// **This is the context's real constructor, and 5.5 does not replace it** (session
    /// O's closure). The shell is here for the *size*, not for the owned fields —
    /// `make_zeroed_shell_valid` writes two of 114 — so a by-value constructor is not
    /// an option at any point in the phase, and "retire the shell" was never a step
    /// anything could perform. What 5.5 owes is to keep the shell honest per owned
    /// field (S21), which is what T5.O3 did when the CABAC engine moved in.
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

        unsafe {
            let pCtx: *mut SWelsDecoderContext = &mut *ctx;
            (*pCtx).pPicBuff = CreatePicBuff(pCtx, 4, 64, 64);
            assert!((*pCtx).pPicBuff.is_some());
            assert_eq!(pic_pool_mut(pCtx).map(|pool| pool.capacity()), Some(4));

            // T5.P″1: the field *is* the out-parameter. `take()` reads the pool and
            // leaves the context naming nothing, which is what the C's
            // `*ppPicBuf = NULL` was for.
            let pool = (*pCtx).pPicBuff.take();
            DestroyPicBuff(pCtx, pool, (*pCtx).pMemAlign);
            assert!((*pCtx).pPicBuff.is_none());
        }
    }
}
