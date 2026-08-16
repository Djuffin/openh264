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
use crate::safe::plane::PlaneCursorMut;
use crate::decoder::fmo::{PFmo, SFmo};
use crate::decoder::slice::EWelsSliceType;
use crate::decoder::decode_slice::IntraPredConstraint;
use crate::decoder::parse_mb_syn_cavlc::SVlcTable;
use crate::decoder::error_concealment::ERROR_CON_IDC;
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

pub use crate::decoder::cabac_decoder::{SWelsCabacCtx, SWelsCabacDecEngine};

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

// **T5.X8: the four reconstruction dispatch types carry a plane cursor, not a
// pointer and a stride.** Every slot in all four tables was a Phase-2 strangler
// wrapper whose whole body rebuilt `(len, center)` from the stride and called a
// safe kernel — 46 of them, and 42 of the phase's 51 retiring bridges. The kernels
// *are* the table now: the wrappers are deleted, the tables name them directly,
// and the one `from_raw_parts_mut` each performed happens once at the
// reconstruction bracket, where the picture is (`decode_slice.rs`'s `Rec*` family),
// because a plane's slice is what the picture owns.
pub type PGetIntraPredFunc = Option<fn(pred: &mut PlaneCursorMut<'_>)>;
pub type PIdctResAddPredFunc = Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 16])>;
pub type PIdctResAddPred8x8Func = Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64])>;
pub type PIdctFourResAddPredFunc =
    Option<fn(pred: &mut PlaneCursorMut<'_>, rs: &[i16; 64], nzc: &[i8; 6])>;
pub type PGetIntraPred8x8Func =
    Option<fn(pred: &mut PlaneCursorMut<'_>, bTLAvail: bool, bTRAvail: bool)>;
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
// T5.Y3: `PDeblockingFunc` went with the field it named — the fifth dead pointer
// typedef this phase has deleted at its definition (S18).

// `PWelsFillNeighborMbInfoIntra4x4Func`, `PWelsMapNeighToSample` and
// `PWelsMap16NeighToSample` were deleted at T4b.3. All three declared
// `pNeighAvail: *mut c_void` and `extern "C"`, neither of which matched the functions
// stored in them -- so every install and every fallback had to launder the mismatch.
// They are one
// `IntraPredConstraint` now; see that type in `decode_slice.rs`.
pub type PWelsParseIntra4x4ModeFunc = Option<
    unsafe extern "C" fn(
        pNeighAvail: *mut c_void,
        pIntraPredMode: &mut [i8; 48],
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
    // **T5.Y3: `pLoopf` stood here and was written twice and read never.** The C++
    // filter reaches its kernels through it (`pFilter->pLoopf->pfLumaDeblockingLT4Ver`);
    // Phase 2 gave the port direct calls into `common::deblocking_common` and left the
    // slot behind, so both assignments were bookkeeping for a table nothing consulted.
    // S18's straggler class, and its deletion removes an alias into the context that
    // outlives every reborrow of it — F53's shape, found by the inventory the flip
    // owes rather than by a probe.

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
// T5.W11c: `pub type PRefPic = *mut SRefPic;` sat here. Its last code user went when
// `manage_dec_ref.rs` took the set by borrow; the name survives only in doc comments
// quoting the C++ signatures, which is the C's name and not this crate's type. The
// fourth dead pointer typedef of session W (S18, at the definition).

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

/// The parse-only output descriptor, **decoder-side and owned** (T5.R4).
///
/// The C's `SParserBsInfo` is a public API struct and `codec_api.rs` declares the
/// port's copy of it; this one is the *decoder's* private instance, which
/// `InitBsBuffer` used to `WelsMallocz` along with both of its buffers. Nothing
/// hands it across the boundary — `DecodeParser` is a stub in this port and the api
/// layer uses its own type — so the two buffers own their allocations here and the
/// boundary's stamping of raw pointers is Phase 8's, with the rest of the `api/`
/// inventory (F38, F41).
#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct SParserBsInfo {
    pub iNalNum: i32,
    /// Per-NAL lengths, `iNalNum` used of `len()` allocated. `ExpandBsLenBuffer`
    /// grows it; the old `pCtx->iMaxNalNum` was this `Vec`'s length stored beside it
    /// and died with the pointer (F16).
    pub pNalLenInByte: Vec<i32>,
    /// The parse-only destination buffer, `MAX_ACCESS_UNIT_CAPACITY` zeroed bytes.
    /// **Allocation-only since T3.3**: the one `pNalPos`-guarded copy that wrote
    /// through it was deleted dead, so nothing reads or writes it today.
    pub pDstBuff: Vec<u8>,
    pub iSpsWidthInPixel: i32,
    pub iSpsHeightInPixel: i32,
    pub uiInBsTimeStamp: u64,
    pub uiOutBsTimeStamp: u64,
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

/// Which SPS buffer an id names, and the id (T5.R6).
///
/// The port keeps the C's two parameter-set arrays — `sSpsBuffer` for AVC and
/// `sSubsetSpsBuffer` for SVC, whose entries *contain* an `SSps` — and the C picks
/// between them with the NAL's extension flag, then stores the resulting pointer.
/// The pointer is a raw alias into a context array, which is the blocker class this
/// phase removes: the pair below is what the pointer was carrying, and the lookup
/// that rebuilds it is `sps_of`.
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SpsRef {
    pub id: i32,
    /// `true` when the entry is `sSubsetSpsBuffer[id].sSps`.
    pub subset: bool,
}

/// The SPS an [`SpsRef`] names, or null when there is none.
///
/// **The one place an SPS id becomes an address** (T5.R6). `addr_of_mut!` derives it
/// from `pCtx` with no intermediate reference (S29), so the result carries the
/// context's own provenance and two live results cannot conflict — which is the
/// property the stored pointer had *only* because it was made the same way, and which
/// nothing about a stored pointer said out loud.
#[inline]
pub unsafe fn sps_of(pCtx: PWelsDecoderContext, r: Option<SpsRef>) -> *mut SSps {
    let Some(r) = r else {
        return std::ptr::null_mut();
    };
    if pCtx.is_null() || r.id < 0 {
        return std::ptr::null_mut();
    }
    let i = r.id as usize;
    if r.subset {
        if i >= MAX_SPS_COUNT + 1 {
            return std::ptr::null_mut();
        }
        std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[i].sSps)
    } else {
        if i >= MAX_SPS_COUNT + 1 {
            return std::ptr::null_mut();
        }
        std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSpsBuffer[i])
    }
}

/// The PPS an id names, or null when there is none. [`sps_of`]'s shape.
#[inline]
pub unsafe fn pps_of(pCtx: PWelsDecoderContext, id: Option<i32>) -> *mut SPps {
    let Some(id) = id else {
        return std::ptr::null_mut();
    };
    if pCtx.is_null() || id < 0 || id as usize >= MAX_PPS_COUNT + 1 {
        return std::ptr::null_mut();
    }
    std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sPpsBuffer[id as usize])
}

/// The FMO entry a PPS id names, or null when there is none. [`pps_of`]'s shape
/// (T5.S1, F43).
///
/// `sFmoList` is `MAX_PPS_COUNT` entries indexed by PPS id — one FMO state per
/// parameter set, persisting across access units, which is why the entry lives in
/// the context's array rather than being rebuilt per slice.
#[inline]
pub unsafe fn fmo_of(pCtx: PWelsDecoderContext, id: Option<i32>) -> PFmo {
    let Some(id) = id else {
        return std::ptr::null_mut();
    };
    if pCtx.is_null() || id < 0 || id as usize >= MAX_PPS_COUNT {
        return std::ptr::null_mut();
    }
    std::ptr::addr_of_mut!((*pCtx).sFmoList[id as usize])
}

/// The active FMO entry — the context field `pFmo` was, resolved (T5.S1).
#[inline]
pub unsafe fn active_fmo(pCtx: PWelsDecoderContext) -> PFmo {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    fmo_of(pCtx, (*pCtx).fmo_id)
}

/// The subset SPS an id names, or null when there is none. [`sps_of`]'s shape.
#[inline]
pub unsafe fn subset_sps_of(pCtx: PWelsDecoderContext, id: Option<i32>) -> *mut SSubsetSps {
    let Some(id) = id else {
        return std::ptr::null_mut();
    };
    if pCtx.is_null() || id < 0 || id as usize >= MAX_SPS_COUNT + 1 {
        return std::ptr::null_mut();
    }
    std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[id as usize])
}

/// The active SPS — the context field `pSps` was, resolved (T5.R6).
#[inline]
pub unsafe fn active_sps(pCtx: PWelsDecoderContext) -> *mut SSps {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    sps_of(pCtx, (*pCtx).active_sps)
}

/// The active PPS — the context field `pPps` was, resolved (T5.R6).
#[inline]
pub unsafe fn active_pps(pCtx: PWelsDecoderContext) -> *mut SPps {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    pps_of(pCtx, (*pCtx).active_pps)
}

/// The bit reader for the slice being parsed — **the one route to it** (T5.M3).
///
/// **Moved here from `bit_stream.rs` at T5.V3**, which is where it belonged: it is a
/// context accessor, not bitstream code — it reaches through `pCtx` to a NAL field and
/// returns a pointer, exactly like [`cur_dq_layer`] below it. Its old home holds the
/// cursor types and their arithmetic, and with this gone that module has no `unsafe`
/// left at all.
///
/// The NAL unit owns its reader (`sNalData.sVclNal.sSliceBitsRead`, initialized by
/// `DecInitBits` at parse time) and nothing else does. `DqLayerState::pBitStringAux`
/// used to mirror the address beside its owner, which is the class §2 keeps naming;
/// this replaces the mirror with a derivation. (`SDeblockingFilter.pCsData` was the
/// last one left and died the same way at T5.N3 — with nothing, because its readers
/// already had the layer that carries the picture.)
///
/// `pCtx.pNalCur` is re-pointed at **every** slice NAL, in the same statement of
/// `DecodeCurrentAccessUnit` that used to re-point the layer's mirror, so this is
/// exactly as fresh as the mirror was — see the note there for why it had to move.
///
/// **S29**: `addr_of_mut!` all the way down, so no `&mut SNalUnit` is created to
/// retag and the returned pointer carries the NAL allocation's provenance, not a
/// field borrow's.
///
// **T5.Y2: `cabac_ctx_base` and `cabac_rbsp_window` stood here and are retired.**
// The first handed out a base address into `pCabacCtx` that 29 call sites indexed
// with `.add(N)`; the view carries the array itself and the same sites index it with
// the bound checked. The second synthesized an unbounded lifetime out of `pCtx` at
// 18 sites (S25's shape) to hand back the RBSP window; the window is constant across
// a slice — `sRawData[reader.start..][..reader.cursor.len()]`, and neither bound
// moves while the cursor walks it — so the bracket top derives it once and the view
// carries it. **The marker on the second one went with it**: the last of W6 step 3's
// retirements, and it retired the way its own doc said it would, when its callers
// stopped needing a raw derivation.

/// `pCtx->pParam->bParseOnly`, with the null test the five hand-written copies of
/// this chain all carry (T5.W3).
///
/// It is here because it is a context accessor, and it exists because family 5's
/// conversion needed the answer without the context: `alloc_picture` reached two
/// pointers deep for one `bool`, and the callers that hold `pCtx` read it for it now.
///
/// # Safety
/// `pCtx` must be null or point to a live decoder context.
#[inline]
pub unsafe fn parse_only(pCtx: PWelsDecoderContext) -> bool {
    !pCtx.is_null() && !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly
}

/// # Safety
/// `pCtx` must be a live decoder context inside slice decoding, where `pNalCur` is
/// the NAL being parsed — the precondition every caller already relies on, and the
/// same one the deleted field carried.
#[inline(always)]
pub unsafe fn slice_bit_reader(
    pCtx: PWelsDecoderContext,
) -> *mut crate::decoder::bit_stream::BsReader {
    std::ptr::addr_of_mut!((*(*pCtx).pNalCur).sNalData.sVclNal.sSliceBitsRead)
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

/// The parse-only descriptor, or null when the decoder is not in parse-only mode.
///
/// [`cur_dq_layer`]'s shape and [`cur_dq_layer`]'s discipline (T5.R4): one live
/// derivation at a time, and the three sites that take one —
/// `DecodeFrameConstruction`'s two arms and `CheckAndFinishLastPic`'s reset — hold it
/// across `cur_au` and context-field writes only, never across a second derivation of
/// this field.
#[inline]
pub unsafe fn parser_bs(pCtx: PWelsDecoderContext) -> *mut SParserBsInfo {
    if pCtx.is_null() {
        return std::ptr::null_mut();
    }
    match (*pCtx).pParserBsInfo.as_deref_mut() {
        Some(parser) => parser as *mut SParserBsInfo,
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
// The slice's view of the context (T5.Y2, W6 step 1)
// ---------------------------------------------------------------------------

/// Everything the per-macroblock tree reaches in [`SWelsDecoderContext`], as
/// field-precise borrows — **and not the picture pool**.
///
/// **Why a view and not the context.** `WelsDecodeSlice`'s bracket top does
/// `let (pDec, pRefs) = cur_and_refs(pCtx)` and [`PicRefs`] borrows `pPicBuff` for
/// the whole slice, while the per-macroblock dispatch takes the context *whole*
/// beside it. As raw pointers the two coexisted silently; as borrows they cannot,
/// and no local repair removes it — session X measured that with the flip and
/// reverted rather than half-land it. The answer is this struct: the bracket top
/// splits the context once, `pPicBuff` is **not** in the split (`pDec` and `pRefs`
/// already travel as their own parameters), and everything below takes pieces.
///
/// The three groups are the three state machines the slice actually runs — the
/// raw-data reader's owner, the CABAC engine, and the flag/counter set — plus the
/// tables and configuration it reads, plus scalars **copied** where S23 clears
/// them: a copied field is one nothing below the bracket writes, checked per field
/// against every update path rather than assumed.
///
/// **The NAL is not in here either.** The slice's bit cursor lives in
/// `pNalCur->sNalData.sVclNal.sSliceBitsRead` — its own allocation, reached by the
/// dispatch's own `pNalCur` parameter — so a callee that needs both the view and
/// the cursor takes two arguments and the borrows are disjoint by construction.
/// The same reasoning as the pool, one field over.
///
/// Field names are the context's own, so a reader greps one name and finds both
/// sides of every conversion.
pub struct SliceCtx<'a> {
    /// The buffer every window is derived from. Nothing below the bracket writes
    /// it — the appends are `WelsDecodeBs`'s, an access unit earlier.
    pub sRawData: &'a RawDataBuffer,
    /// The RBSP window the CABAC engine reads, derived **once** at the bracket top.
    ///
    /// This is `cabac_rbsp_window`'s retirement (W6 step 3): the window is
    /// `sRawData[reader.start..][..reader.cursor.len()]`, and both bounds are fixed
    /// when the NAL is parsed — the cursor's *position* moves inside it, the window
    /// does not. Held as a shared slice, so reading it costs no borrow of the view
    /// and the engine can be borrowed mutably in the same expression.
    pub rbsp: &'a [u8],
    pub sCabacDecEngine: &'a mut SWelsCabacDecEngine,
    pub pCabacCtx: &'a mut [SWelsCabacCtx; WELS_CONTEXT_COUNT],
    pub bMbRefConcealed: &'a mut bool,
    pub iErrorCode: &'a mut i32,
    pub iTotalNumMbRec: &'a mut i32,
    /// The B-slice temporal-direct scratch picture, lazily allocated at the first B
    /// macroblock — a write, so it is a borrow and not a copy.
    pub pTempDec: &'a mut Option<Box<Picture>>,
    pub sSpsPpsCtx: &'a SWelsDecoderSpsPpsCTX,
    pub sFmoList: &'a [SFmo; MAX_PPS_COUNT],
    pub sRefPic: &'a SRefPic,
    /// `pCtx->pVlcTable` resolved. The table is `CWelsDecoderImpl`'s, filled once by
    /// `InitVlcTable` and never written again, so the view carries the borrow the
    /// three CAVLC derivations used to spell as a cast each.
    pub pVlcTable: &'a SVlcTable,
    pub pDequant_coeff_buffer4x4: &'a [[[u16; 16]; 52]; 6],
    pub pDequant_coeff_buffer8x8: &'a [[[u16; 64]; 52]; 6],
    pub pGetI16x16LumaPredFunc: &'a [PGetIntraPredFunc; 7],
    pub pGetI4x4LumaPredFunc: &'a [PGetIntraPredFunc; 14],
    pub pGetIChromaPredFunc: &'a [PGetIntraPredFunc; 7],
    pub pGetI8x8LumaPredFunc: &'a [PGetIntraPred8x8Func; 14],
    pub pIdctResAddPredFunc: PIdctResAddPredFunc,
    pub pIdctFourResAddPredFunc: PIdctFourResAddPredFunc,
    pub pIdctResAddPredFunc8x8: PIdctResAddPred8x8Func,

    // --- copied scalars, each with the update paths that clear it (S23) ---
    /// Written by `WelsDecodeSlice`'s and `WelsDecodeAndConstructSlice`'s bracket
    /// tops, above this construction, and by nothing below.
    pub eSliceType: EWelsSliceType,
    /// Same two writers, same line (T4b.3's three laundered slots, as one enum).
    pub eIntraPredConstraint: IntraPredConstraint,
    /// Both written by `WelsCalcDeqCoeffScalingList`, which the bracket top calls
    /// before this construction and nothing below calls at all.
    pub bUseScalingList: bool,
    pub bDequantCoeff4x4Init: bool,
    /// Written by `DecodeCurrentAccessUnit` (once per access unit) and by
    /// `manage_dec_ref`'s reference-list construction — both above the bracket.
    pub bRPLRError: bool,
    /// `pParam`'s two questions, answered **inside** the constructor so F41's raw
    /// field never escapes into the tree. Neither answer changes after
    /// `Initialize`.
    ///
    /// `bParseOnly` is [`parse_only`]'s body. `bEcActive` is
    /// `eEcActiveIdc != ERROR_CON_DISABLE`, which is the only comparison any of the
    /// eight consumers made — three of them spelled `pParam.is_null() || …`, which
    /// this keeps, and five dereferenced without the test, which this makes safe.
    pub bParseOnly: bool,
    pub bEcActive: bool,
    /// `pMemAlign != null` — the whole of what `GetTempPredPlanes` asks it, since
    /// `alloc_picture` stopped taking the aligner at T5.W3.
    pub bHasMemAlign: bool,
    pub iCurSeqIntervalMaxPicWidth: i32,
    /// `GetThreadCount`, which is 0 and cannot change mid-slice.
    pub iThreadCount: i32,
    /// The active parameter sets, as the ids the context stores.
    pub active_sps: Option<SpsRef>,
    pub active_pps: Option<i32>,
    pub fmo_id: Option<i32>,
}

impl<'a> SliceCtx<'a> {
    /// [`sps_of`] against the view — same bounds, same `None` for the same ids.
    #[inline]
    pub fn sps_of(&self, r: Option<SpsRef>) -> Option<&SSps> {
        let r = r?;
        if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
            return None;
        }
        let i = r.id as usize;
        Some(if r.subset {
            &self.sSpsPpsCtx.sSubsetSpsBuffer[i].sSps
        } else {
            &self.sSpsPpsCtx.sSpsBuffer[i]
        })
    }

    /// [`pps_of`] against the view.
    #[inline]
    pub fn pps_of(&self, id: Option<i32>) -> Option<&SPps> {
        let id = id?;
        if id < 0 || id as usize >= MAX_PPS_COUNT + 1 {
            return None;
        }
        Some(&self.sSpsPpsCtx.sPpsBuffer[id as usize])
    }

    /// [`active_sps`] against the view.
    #[inline]
    pub fn active_sps(&self) -> Option<&SSps> {
        self.sps_of(self.active_sps)
    }

    /// [`active_pps`] against the view.
    #[inline]
    pub fn active_pps(&self) -> Option<&SPps> {
        self.pps_of(self.active_pps)
    }

    /// [`active_fmo`] against the view. `sFmoList` is `MAX_PPS_COUNT` entries
    /// indexed by PPS id, and the bound is the one `fmo_of` carries.
    #[inline]
    pub fn active_fmo(&self) -> Option<&SFmo> {
        let id = self.fmo_id?;
        if id < 0 || id as usize >= MAX_PPS_COUNT {
            return None;
        }
        Some(&self.sFmoList[id as usize])
    }

    /// [`ref_id`] against the view — the handle, without touching the pool.
    #[inline]
    pub fn ref_id(&self, list: usize, i: usize) -> Option<PicId> {
        self.sRefPic.pRefList[list][i]
    }

    /// `pSps->uiChromaFormatIdc`, the active SPS's, at the 25 sites that read it
    /// per macroblock.
    ///
    /// **0 with no active SPS**, which is monochrome — and a state the slice tree
    /// cannot reach, because a slice header that activates no SPS never gets here.
    /// The spelling this replaces dereferenced the null, so every arm below is
    /// strictly more defined than what it translates.
    #[inline]
    pub fn uiChromaFormatIdc(&self) -> u8 {
        self.active_sps().map_or(0, |sps| sps.uiChromaFormatIdc)
    }

    /// `pPps->bTransform8x8ModeFlag`, the active PPS's. `false` with no active PPS
    /// — [`uiChromaFormatIdc`](Self::uiChromaFormatIdc)'s clause, same reason.
    #[inline]
    pub fn bTransform8x8ModeFlag(&self) -> bool {
        self.active_pps().is_some_and(|pps| pps.bTransform8x8ModeFlag)
    }
}

/// **The bracket top's split** — the one construction of [`SliceCtx`], and the
/// enumerated exception this face leaves behind.
///
/// Every borrow is derived with `addr_of!`/`addr_of_mut!` and dereferenced field by
/// field (S29), so no reference to the context *as a whole* is ever created: the
/// `PicRefs` the caller is holding over `pPicBuff` is derived from the same raw
/// pointer and stays valid across this, which is the property that lets the split
/// happen beside the pool borrow rather than instead of it.
///
/// **The reader arrives as an argument, and Miri is why** (T5.Y2). The first
/// spelling derived the window here from `pCtx->pNalCur`, which is a *second* path
/// to the NAL the caller is already holding as `&mut` — a shared retag through the
/// context's raw field pops the caller's `Unique`, and the next use of the borrow is
/// UB. The probe convicted it at the dispatch call one line below the split. So the
/// window comes from the borrow the tree itself carries: one path to the NAL, and
/// the returned slice borrows `sRawData` rather than the reader, so the argument's
/// borrow ends with this call.
///
/// `None` is a bracket with no NAL in flight — the reconstruction and colocated
/// brackets, which parse nothing — and the view carries an empty window. A read
/// through it fails with `ERR_INFO_READ_OVERFLOW`, which is the disposition
/// `window_from`'s clamp already takes; the alternative is the null dereference
/// `cabac_rbsp_window` would have made, and no caller could survive that.
///
/// # Safety
/// `pCtx` must be a live decoder context inside a slice bracket, with the parameter
/// sets and the dequantisation tables already selected for this slice — the
/// scalars are copies, and a write to one of them behind the view's back is exactly
/// what S23 asks each field to be checked against.
pub unsafe fn slice_ctx<'a>(pCtx: PWelsDecoderContext, reader: Option<&BsReader>) -> SliceCtx<'a> {
    use std::ptr::{addr_of, addr_of_mut};
    let pParam = (*pCtx).pParam;
    let raw: &'a RawDataBuffer = &*addr_of!((*pCtx).sRawData);
    SliceCtx {
        sRawData: raw,
        rbsp: match reader {
            Some(reader) => raw.rbsp_window(reader),
            None => &[],
        },
        sCabacDecEngine: &mut *addr_of_mut!((*pCtx).sCabacDecEngine),
        pCabacCtx: &mut *addr_of_mut!((*pCtx).pCabacCtx),
        bMbRefConcealed: &mut *addr_of_mut!((*pCtx).bMbRefConcealed),
        iErrorCode: &mut *addr_of_mut!((*pCtx).iErrorCode),
        iTotalNumMbRec: &mut *addr_of_mut!((*pCtx).iTotalNumMbRec),
        pTempDec: &mut *addr_of_mut!((*pCtx).pTempDec),
        sSpsPpsCtx: &*addr_of!((*pCtx).sSpsPpsCtx),
        sFmoList: &*addr_of!((*pCtx).sFmoList),
        sRefPic: &*addr_of!((*pCtx).sRefPic),
        pVlcTable: &*((*pCtx).pVlcTable as *const SVlcTable),
        pDequant_coeff_buffer4x4: &*addr_of!((*pCtx).pDequant_coeff_buffer4x4),
        pDequant_coeff_buffer8x8: &*addr_of!((*pCtx).pDequant_coeff_buffer8x8),
        pGetI16x16LumaPredFunc: &*addr_of!((*pCtx).pGetI16x16LumaPredFunc),
        pGetI4x4LumaPredFunc: &*addr_of!((*pCtx).pGetI4x4LumaPredFunc),
        pGetIChromaPredFunc: &*addr_of!((*pCtx).pGetIChromaPredFunc),
        pGetI8x8LumaPredFunc: &*addr_of!((*pCtx).pGetI8x8LumaPredFunc),
        pIdctResAddPredFunc: (*pCtx).pIdctResAddPredFunc,
        pIdctFourResAddPredFunc: (*pCtx).pIdctFourResAddPredFunc,
        pIdctResAddPredFunc8x8: (*pCtx).pIdctResAddPredFunc8x8,
        eSliceType: (*pCtx).eSliceType,
        eIntraPredConstraint: (*pCtx).eIntraPredConstraint,
        bUseScalingList: (*pCtx).bUseScalingList,
        bDequantCoeff4x4Init: (*pCtx).bDequantCoeff4x4Init,
        bRPLRError: (*pCtx).bRPLRError,
        bParseOnly: !pParam.is_null() && (*pParam).bParseOnly,
        bEcActive: pParam.is_null()
            || (*pParam).eEcActiveIdc != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE,
        bHasMemAlign: !(*pCtx).pMemAlign.is_null(),
        iCurSeqIntervalMaxPicWidth: (*pCtx).iCurSeqIntervalMaxPicWidth,
        iThreadCount: crate::decoder::decoder_core::GetThreadCount(pCtx),
        active_sps: (*pCtx).active_sps,
        active_pps: (*pCtx).active_pps,
        fmo_id: (*pCtx).fmo_id,
    }
}

/// A view over a test context, wired the way `Initialize` wires the real one.
///
/// The only wiring [`slice_ctx`] needs and a zeroed context does not have is the VLC
/// table, which lives in `CWelsDecoderImpl` and is installed at
/// `api/codec_api.rs:1509`; the fixture installs it from the caller's own table so
/// the borrow the view hands out has a real owner.
#[cfg(test)]
pub(crate) unsafe fn test_slice_ctx<'a>(
    ctx: &'a mut SWelsDecoderContext,
    vlc: &'a mut SVlcTable,
) -> SliceCtx<'a> {
    ctx.pVlcTable = std::ptr::addr_of_mut!(*vlc).cast::<c_void>();
    slice_ctx(std::ptr::addr_of_mut!(*ctx), None)
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
    /// The active FMO entry, as the **PPS id that selects it** (T5.S1) — the field
    /// `pFmo` was.
    ///
    /// The C writes `pCtx->pFmo = &pCtx->sFmoList[iPpsId]` (`decoder_core.cpp:2651`):
    /// a raw alias into the context's *own* array, which is the blocker class this
    /// phase names once. The id is what the C computed the address from, so storing
    /// it stores strictly more than the pointer did, and [`fmo_of`] derives the
    /// address per use with no borrow taken (S29) — `pps_of`'s shape exactly, and for
    /// the same reason: `sFmoList` is indexed by PPS id and nothing else.
    pub fmo_id: Option<i32>,
    pub iActiveFmoNum: i32,
    // T5.X8: `iDecBlockOffsetArray: [i32; 24]` stood here — the 4x4 blocks'
    // byte offsets inside a macroblock, rebuilt per access unit because they
    // depended on the picture's strides. Sample coordinates carry the same fact
    // with no stride in them: `decode_slice.rs`'s `blk4_xy`.
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
    /// The active parameter sets, as ids rather than aliases into the two buffers
    /// (T5.R6). `None` is the null the two pointers held before the first slice
    /// header; [`active_sps`] and [`active_pps`] are the only readers.
    pub active_sps: Option<SpsRef>,
    pub active_pps: Option<i32>,
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
    // `iMaxNalNum` stood here — the allocated length of the parse-only length buffer,
    // stored beside it and read only by the allocation and free arithmetic. T5.R4's
    // `Vec` knows it (F16, and the same disposition `iMaxBsBufferSizeInByte` got at
    // T3.3).
    pub sSpsBsInfo: [SSpsBsInfo; MAX_SPS_COUNT],
    pub sSubsetSpsBsInfo: [SSpsBsInfo; MAX_PPS_COUNT],
    pub sPpsBsInfo: [SPpsBsInfo; MAX_PPS_COUNT],
    /// The parse-only descriptor, **owned** (T5.R4) and reached through
    /// [`parser_bs`]. `None` outside parse-only mode, which is the state the old null
    /// pointer named — and, unlike the null, it is what F41's flag-dependent free
    /// path can no longer leak: the drop glue does not read `bParseOnly`.
    pub pParserBsInfo: Option<Box<SParserBsInfo>>,
    pub pGetI16x16LumaPredFunc: [PGetIntraPredFunc; 7],
    pub pGetI4x4LumaPredFunc: [PGetIntraPredFunc; 14],
    pub pGetIChromaPredFunc: [PGetIntraPredFunc; 7],
    pub pIdctResAddPredFunc: PIdctResAddPredFunc,
    pub pIdctFourResAddPredFunc: PIdctFourResAddPredFunc,
    pub sMcFunc: crate::decoder::error_concealment::SMcFunc,
    pub pGetI8x8LumaPredFunc: [PGetIntraPred8x8Func; 14],
    pub pIdctResAddPredFunc8x8: PIdctResAddPred8x8Func,
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
    // T5.Y1: `pDequant_coeff4x4` and `pDequant_coeff8x8` stood here — six aliases
    // each into the two buffers directly above, written as
    // `pDequant_coeff4x4[i] = pDequant_coeff_buffer4x4[i].as_mut_ptr()` and read
    // back as `[i][qp]`. Aliases into the context's own array: the blocker class
    // this phase names once, and here in its purest form, because the index the
    // alias was derived from is the index every reader already had. The readers
    // take the buffer row; `bDequantCoeff4x4Init` is the initialized test the
    // null test was (both are written in `WelsCalcDeqCoeffScalingList`'s one
    // block, and nothing else writes either).
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
            // S21, T5.R4: the parse-only descriptor, `None` through the same niche.
            std::ptr::addr_of_mut!((*p).pParserBsInfo).write(None);
            // S21, T5.R3, and the one clause here that is **not** redundant: `TagFmo`
            // owns its map as a `Vec` now, and a zeroed `Vec` is an invalid value with
            // no niche to rescue it. 256 entries, written where the array is.
            let fmo_list = std::ptr::addr_of_mut!((*p).sFmoList) as *mut SFmo;
            for i in 0..MAX_PPS_COUNT {
                fmo_list.add(i).write(SFmo::default());
            }
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
    use crate::decoder::decode_mb_aux::idct_res_add_pred;
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
        let rs = [0i16; 16];
        idct_res_add_pred(&mut PlaneCursorMut::new(&mut pred, 0, 4), &rs);
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
            (*pCtx).pPicBuff = CreatePicBuff(false, 4, 64, 64);
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
