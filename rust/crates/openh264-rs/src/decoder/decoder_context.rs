#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#![deny(unsafe_code)]
// **Phase 5, T5.AC10 — the lint on the module the brief called the odd one.** Its
// 36 raw-pointer occurrences against five `unsafe fn` said what §1 predicted:
// *"most of those are type aliases and API-boundary spellings, not conversions."*
// Measured, they were three things and only one was a conversion —
//
//   * **Six dead pointer typedefs**, deleted at their definitions, plus a *third*
//     declaration of `PCopyFunc` that no side imported (S18, the tombstones below).
//   * **The twelve api-owned context fields** — `pParam`, `pLastDecPicInfo`,
//     `pDecoderStatistics`, `pMemAlign`, `pVlcTable`, `pSliceHeader`, `pNalCur`,
//     `pStreamSeqNum`, `pPictInfoList`, `pPictReoderingStatus`, `pTraceHandle`,
//     `pArgDec`. They were declarations of a boundary Phase 5 did not own, reached
//     through `api_alias`/`api_alias_mut` (T5.AC4). **Phase 8 session A owns them
//     now and they are the context's own fields** (T8.A5–A8); `pMemAlign` is
//     deleted with the allocator, `pSliceHeader`/`pNalCur` became indices at T5b.3,
//     and `pTraceHandle`/`pArgDec` are `void*`s this port never reads.
//   * **The four exceptions below**: the three per-slice view constructors, which
//     W6's settlement named as the enumerated exception before they were written,
//     and the zeroed shell, which is session O's standing constraint.
//
// **All four are gone** (T5b.5/T5b.6): the views take borrows and the shell is a
// field-wise constructor. `api_alias`/`api_alias_mut` stood as this file's two
// enumerated items until T8.A8; **this file now allows nothing**, and the whole of
// `src/decoder/` is `picture.rs`'s two Miri tests for `data_ptr`.

//! Master decoder execution context, function dispatch tables, DPB memory pool management,
//! bitstream demuxing, and statistics aggregation.
//!
//! Translated from `codec/decoder/core/inc/decoder_context.h` and `codec/decoder/core/src/decoder.cpp`.

use crate::decoder::bit_stream::BsReader;
use crate::safe::plane::PlaneCursorMut;
use crate::decoder::fmo::{SFmo};
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
// **T5.AC10: `PCopyFunc` stood here and is deleted — a *third* declaration of one
// typedef, and the only one in `src/decoder/`.** The other two are
// `encoder/encode_mb_aux.rs:200` and `encoder/md.rs:228`, and it is the first that
// `encoder/wels_func_ptr_def.rs` and `encoder/md.rs` actually import; this copy had
// zero users on either side. No encoder site is touched and no encoder file is
// edited — the duplicate that goes is the decoder's, which is what makes this S18
// rather than F22, and the encoder's remaining pair is Phase 6's.

// **T5b.9: the second `PDeblockingFilterMbFunc` declaration is deleted (S18/F22's
// class).** It was the C-callback shape — `Option<unsafe extern "C" fn(pCurDqLayer,
// filter, boundry_flag)>` over two raw pointers — and the census allowlisted the
// duplicate as "a REAL type divergence" against `deblocking.rs`'s. That divergence
// resolved itself when T5.AB2 made the live declaration a safe `fn` taking the
// layer and the filter by reference: nothing has imported this copy since, and the
// two call sites in `deblocking.rs` resolve to that module's own.
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
// **T5.AC10 — six dead pointer typedefs, deleted at their definitions (S18).**
// Each declared a raw pointer and had **zero users** anywhere in the crate at the
// commit the grep was taken; two of them were function-pointer types for a dispatch
// that no longer exists.
//
//   `PWelsDecoderSpsPpsCTX`, `PWelsLastDecPicInfo`, `PPictInfo`,
//   `PPictReoderingStatus` — the four `*mut` spellings of context-adjacent structs,
//   outlived by the accessors that reach those structs by field.
//
//   `PWelsParseIntra4x4ModeFunc`, `PWelsParseIntra16x16ModeFunc` — the last two of
//   the intra-mode dispatch typedefs. Their three siblings went at T4b.3 and the
//   note above records why: they declared `*mut c_void` and `extern "C"`, neither
//   of which matched what was stored in them. `IntraPredConstraint` replaced the
//   dispatch; these two outlived it because nothing named them.
//
// This is the phase's sixth through eleventh dead typedef, and the sixth time the
// count moved because a *definition* went rather than a use (S16).

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

pub use crate::decoder::parameter_sets::SPosOffset;


pub use crate::decoder::parameter_sets::{SSps, SPps, SSubsetSps};


pub use crate::decoder::nalu::{
    SNalUnitHeader, SNalUnitHeaderExt, SNalUnit, 
};



pub use crate::decoder::slice::{
    SSliceHeader, SSliceHeaderExt, SRefBasePicMarking, 
};


#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsDecoderSpsPpsCTX {
    pub sFrameCrop: SPosOffset,
    pub sSpsBuffer: [SSps; MAX_SPS_COUNT + 1],
    pub sPpsBuffer: [SPps; MAX_PPS_COUNT + 1],
    pub sSubsetSpsBuffer: [SSubsetSps; MAX_SPS_COUNT + 1],
    pub sPrefixNal: SNalUnit,
    /// Which SPS each dependency layer activated — **an id, not an address**
    /// (T5.Z1). This held `*mut SSps` aliases into the two buffers above it, and
    /// every one of its six readers used them for *identity* only: "is the SPS this
    /// layer activated still the one this NAL names?". A raw alias into a container
    /// blocks that container, so the alias becomes the id it was resolved from —
    /// [`SpsRef`] equality is address equality, because [`sps_of`] maps distinct
    /// refs to distinct slots.
    pub pActiveLayerSps: [Option<SpsRef>; MAX_LAYER_NUM],
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

impl SWelsDecoderSpsPpsCTX {
    /// The parameter-set context the decoder context is *born* with — `WelsMallocz`'s
    /// zero, field by field (S21/F54).
    ///
    /// **Not [`SWelsDecoderSpsPpsCTX::default`]**, which is `WelsDecoderDefaults`'
    /// value and disagrees on eight fields: `bAvcBasedFlag` is `true` there and the
    /// five `i*LastInvalidId`/`iSeqId` fields are `-1`. Both values are real and the
    /// decoder reaches them in that order.
    pub fn memset_zero() -> Self {
        Self {
            sFrameCrop: SPosOffset::default(),
            sSpsBuffer: [SSps::memset_zero(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::memset_zero(); MAX_PPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::memset_zero(); MAX_SPS_COUNT + 1],
            // The prefix NAL's *prefix* arm is the only one this field's five readers
            // touch (`nalu.rs`, `ParsePrefixNalUnit` and `WelsParseNalHeaderExt`);
            // its VCL arm — and so the `Option<SpsRef>` buried in that arm's slice
            // header — is written by nothing and read by nothing. That made this the
            // third site of F56 and the only one whose value no path could observe;
            // T5b.8 rules `None` faithful at all three, so it is now the same answer
            // the other two give rather than an unobservable divergence from them.
            sPrefixNal: SNalUnit::default(),
            // T5.Z1, and the reason this one write predates this constructor:
            // `Option<SpsRef>` keeps its niche in a `bool`, so all-zero reads back as
            // `Some(SpsRef { id: 0, subset: false })` — "SPS 0 is already active"
            // before one has ever been parsed, which takes every stream to zero
            // frames with `dsNoParamSets`.
            pActiveLayerSps: [None; MAX_LAYER_NUM],
            // `true` in `Default`.
            bAvcBasedFlag: false,
            bSpsExistAheadFlag: false,
            bSubspsExistAheadFlag: false,
            bPpsExistAheadFlag: false,
            iSpsErrorIgnored: 0,
            iSubSpsErrorIgnored: 0,
            iPpsErrorIgnored: 0,
            bSpsAvailFlags: [false; MAX_SPS_COUNT],
            bSubspsAvailFlags: [false; MAX_SPS_COUNT],
            bPpsAvailFlags: [false; MAX_PPS_COUNT],
            // The six `-1`s of `Default`.
            iPPSLastInvalidId: 0,
            iPPSInvalidNum: 0,
            iSPSLastInvalidId: 0,
            iSPSInvalidNum: 0,
            iSubSPSLastInvalidId: 0,
            iSubSPSInvalidNum: 0,
            iSeqId: 0,
            iOverwriteFlags: 0,
        }
    }
}

impl Default for SWelsDecoderSpsPpsCTX {
    fn default() -> Self {
        Self {
            sFrameCrop: SPosOffset::default(),
            sSpsBuffer: [SSps::default(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::default(); MAX_PPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::default(); MAX_SPS_COUNT + 1],
            sPrefixNal: SNalUnit::default(),
            pActiveLayerSps: [None; MAX_LAYER_NUM],
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

pub use crate::decoder::picture::{SPicture, SPicture as Picture};



pub use crate::decoder::pic_queue::{PicPool, PicId, PicRefs, SPicBuff};

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

impl SRefPic {
    /// The reference-list set `WelsMallocz`'s zeroing leaves inside the context —
    /// **not** [`SRefPic::default`], whose `iMaxLongTermFrameIdx` is `-1` because that
    /// is what `WelsResetRefPic` writes when it initialises the set for use.
    pub fn memset_zero() -> Self {
        Self {
            pRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pShortRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            pLongRefList: [[None; MAX_DPB_COUNT]; LIST_A],
            uiRefCount: [0; LIST_A],
            uiShortRefCount: [0; LIST_A],
            uiLongRefCount: [0; LIST_A],
            iMaxLongTermFrameIdx: 0,
        }
    }
}

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

// **T8.A1 — one declaration, and it is the ABI's.** `SDecoderStatistics` stood
// declared here *and* in `api/codec_api.rs`, field-for-field identical: F21/F22's
// exact shape, invisible for seven phases because no instrument read `src/api`.
// It is one of the twelve fields the api layer stamps into this context, so the
// two copies met at `pDecoderStatistics` and agreed only by luck. The public
// header owns the layout (`codec_api.h`'s `SDecoderStatistics`, handed out by
// `DECODER_OPTION_GET_STATISTICS`), so the api's is the declaration and this is a
// re-export.
pub use crate::api::codec_api::SDecoderStatistics;

// **T8.B6: the decoder's own `SLogContext` stood here** — the second of the
// census's `type SLogContext x2`, and the one that had already typed `pfLog` as a
// function rather than a `*mut c_void`. One declaration now, in
// `common::wels_trace`, with the instance address and the level travelling beside
// the callback (see that module for why).
pub use crate::common::wels_trace::SLogContext;

// **T8.A1 — and this pair had *diverged*.** Both names were declared here and in
// `api/codec_api.rs`, and the two `VIDEO_BITSTREAM_TYPE`s carry their `#[default]`
// on **different variants**: AVC here, SVC there — where the C++'s
// `VIDEO_BITSTREAM_DEFAULT` is SVC (`codec_app_def.h`). `SVideoProperty::default()`
// derives from it, so a `SDecodingParam` built through this copy would have named a
// different bitstream type than one built through the api's, under one name, with
// nothing to say so. The api's is the declaration; both are re-exported.
//
// Nothing moves today: this copy's only live consumer is the context's own
// `eVideoType`, whose one write names its variant explicitly (see the field). That
// the unification is behaviour-neutral is a fact about *this* tree, not about the
// shape — see F76 for what `eVideoType` is still missing.
pub use crate::api::codec_api::{SVideoProperty, VIDEO_BITSTREAM_TYPE};

// The decoder context points at the caller's `SDecodingParam`, so it must be
// the very same type as the public API struct (`codec_app_def.h`).
pub use crate::api::codec_api::SDecodingParam;


pub use crate::decoder::decoder_core::{DqLayerState, SLayerInfo};


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
pub fn cur_au(au: &mut Option<Box<SAccessUnit>>) -> Option<&mut SAccessUnit> {
    au.as_deref_mut()
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
///
/// **T5.Z1: the parameter is the pool field, not the context.** The result is a
/// borrow of one slot's picture; derived from the context whole, it would die at
/// the next call that takes the context (session Y's verdict), and it would
/// conflict with every disjoint field a caller touches beside it. `pPicBuff` is
/// the field it actually reaches, and every caller already spells it.
#[inline]
pub fn pool_pic(pool: &Option<Box<SPicBuff>>, slot: Option<PicId>) -> Option<&SPicture> {
    pool.as_deref()?.slot(slot?)
}

/// [`pool_pic`]'s mutable form, for the paths that write through what they resolve.
///
/// **One live result at a time**, and it is the caller's job to keep it that way: a
/// result that outlives the expression it was taken in, across another resolution of
/// the same slot, is the conflict the flip is about. Where a scope needs one picture
/// mutably *and* others readably, the answer is a bracket
/// ([`cur_and_refs`]), not two calls here.
#[inline]
pub fn pool_pic_mut(pool: &mut Option<Box<SPicBuff>>, slot: Option<PicId>) -> Option<&mut SPicture> {
    pool.as_deref_mut()?.slot_mut(slot?)
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
pub fn pic_pool_mut(pCtx: &mut SWelsDecoderContext) -> Option<&mut SPicBuff> {
    pCtx.pPicBuff.as_deref_mut()
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

/// The SPS an [`SpsRef`] names, or `None` when there is none.
///
/// **The one place an SPS id becomes a borrow** (T5.R6; T5.Z1 made it one).
///
/// **Why the parameter is the field and not the context** (T5.Z1). This returned
/// `*mut SSps` derived from `pCtx` until session Z. That was sound while the context
/// was a pointer and undefined the moment it became a `&mut`: a `Unique`
/// function-entry retag on the context pops every derivation through it, so the next
/// read of a stored result is UB — session Y measured three instances in one probe
/// run and reverted the flip on them. Returning a borrow makes the rule a compile
/// error instead of a Miri verdict, and taking **`sSpsPpsCtx` rather than the whole
/// context** is what keeps that error honest: a whole-context borrow would conflict
/// with every disjoint field a caller touches beside it, and the one call the verdict
/// could not repair site-locally — `FmoParamUpdate(fmo_of(…), sps_of(…), …)` — is
/// exactly two disjoint fields. Same reasoning as [`SliceCtx`], one level up.
#[inline]
pub fn sps_of(ps: &SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<&SSps> {
    let r = r?;
    if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    let i = r.id as usize;
    Some(if r.subset {
        &ps.sSubsetSpsBuffer[i].sSps
    } else {
        &ps.sSpsBuffer[i]
    })
}

/// The [`SpsRef`] an id resolves to, or `None` where [`sps_of`] answers `None`.
///
/// The identity `pActiveLayerSps` used to carry as an address, normalized the way
/// the address was: an out-of-range ref resolved to null, so it stores as `None`
/// rather than as itself (T5.Z1).
#[inline]
pub fn sps_ref_of(ps: &SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<SpsRef> {
    if sps_of(ps, r).is_some() { r } else { None }
}

/// [`sps_of`]'s mutable form, for the parse paths that fill a buffer entry.
#[inline]
pub fn sps_of_mut(ps: &mut SWelsDecoderSpsPpsCTX, r: Option<SpsRef>) -> Option<&mut SSps> {
    let r = r?;
    if r.id < 0 || r.id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    let i = r.id as usize;
    Some(if r.subset {
        &mut ps.sSubsetSpsBuffer[i].sSps
    } else {
        &mut ps.sSpsBuffer[i]
    })
}

/// The PPS an id names, or `None` when there is none. [`sps_of`]'s shape.
#[inline]
pub fn pps_of(ps: &SWelsDecoderSpsPpsCTX, id: Option<i32>) -> Option<&SPps> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT + 1 {
        return None;
    }
    Some(&ps.sPpsBuffer[id as usize])
}

/// The FMO entry a PPS id names, or `None` when there is none. [`pps_of`]'s shape
/// (T5.S1, F43; a borrow since T5.Z1).
///
/// `sFmoList` is `MAX_PPS_COUNT` entries indexed by PPS id — one FMO state per
/// parameter set, persisting across access units, which is why the entry lives in
/// the context's array rather than being rebuilt per slice.
///
/// **This is the accessor whose old spelling had no site-local repair.**
/// `FmoParamUpdate(fmo_of(pCtx, …), sps_of(pCtx, …), …)` derived two raw pointers
/// from one context and passed them in one call, each invalidating the other under
/// a `&mut` context — session Y's third Miri instance. Taking `sFmoList` and
/// `sSpsPpsCtx` as the separate fields they are makes that call two disjoint
/// borrows and a third for `iActiveFmoNum`, which is what the whole family's
/// parameter choice is for.
#[inline]
pub fn fmo_of(list: &[SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&SFmo> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT {
        return None;
    }
    Some(&list[id as usize])
}

/// [`fmo_of`]'s mutable form — `FmoParamUpdate`'s, which rebuilds the map.
#[inline]
pub fn fmo_of_mut(list: &mut [SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&mut SFmo> {
    let id = id?;
    if id < 0 || id as usize >= MAX_PPS_COUNT {
        return None;
    }
    Some(&mut list[id as usize])
}

/// The active FMO entry — the context field `pFmo` was, resolved (T5.S1). The id
/// travels beside the array for [`active_sps`]'s reason.
#[inline]
pub fn active_fmo(list: &[SFmo; MAX_PPS_COUNT], id: Option<i32>) -> Option<&SFmo> {
    fmo_of(list, id)
}

/// The subset SPS an id names, or `None` when there is none. [`sps_of`]'s shape.
#[inline]
pub fn subset_sps_of(ps: &SWelsDecoderSpsPpsCTX, id: Option<i32>) -> Option<&SSubsetSps> {
    let id = id?;
    if id < 0 || id as usize >= MAX_SPS_COUNT + 1 {
        return None;
    }
    Some(&ps.sSubsetSpsBuffer[id as usize])
}

/// The active SPS — the context field `pSps` was, resolved (T5.R6).
///
/// The id travels as a value beside the array because it lives in a **different**
/// field of the context: `pCtx.active_sps` is `Copy`, so reading it takes no borrow
/// and the returned reference borrows `sSpsPpsCtx` alone (T5.Z1).
#[inline]
pub fn active_sps(ps: &SWelsDecoderSpsPpsCTX, active: Option<SpsRef>) -> Option<&SSps> {
    sps_of(ps, active)
}

/// The active PPS — the context field `pPps` was, resolved (T5.R6).
#[inline]
pub fn active_pps(ps: &SWelsDecoderSpsPpsCTX, active: Option<i32>) -> Option<&SPps> {
    pps_of(ps, active)
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
/// **T8.A5: the parameter block is the context's own, so this is a field read.**
/// It was `api_alias(..).is_some_and(..)` over a raw alias into `CWelsDecoderImpl`
/// — F41 — and the `Option` it produced was the C's null test on a block the
/// context in fact owns (`welsDecoderExt.cpp:426` allocates it, `DecoderConfigParam`
/// fills it). There is no null to test any more, and no unsafe read to argue.
#[inline]
pub fn parse_only(pParam: &SDecodingParam) -> bool {
    pParam.bParseOnly
}

/// `pCtx->pParam->eEcActiveIdc`.
///
/// **T8.A5 folded the two spellings into one, and it is not S6's never-widen case.**
/// This used to carry a *null-is-disabled* default while a dozen sites spelled the
/// other shape — `!pParam.is_null() && eEcActiveIdc == X`, where a null block makes
/// the whole test **false** rather than "disabled" — and the note here said the two
/// defaults disagree so no site may be moved between them. They disagreed only about
/// a null, and F41's fix is that the block is the context's own field: there is no
/// null, both shapes are the same read, and the divergence retires with the pointer.
#[inline]
pub fn ec_active_idc(pParam: &SDecodingParam) -> ERROR_CON_IDC {
    pParam.eEcActiveIdc
}

// **T5.Z3: `slice_bit_reader` stood here and is deleted.** It reached the slice's
// bit cursor through `pCtx->pNalCur` and returned a raw pointer; T5.Y2's split gave
// the tree one path to the NAL — the dispatch's own `pNalCur` parameter — and the
// last call went with `cabac_rbsp_window`. Found by S18's sweep with zero callers
// and one dead import.

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
///
/// **T5.Z1's parameter rule** (T5.Z3): the field. The two bracket tops that take
/// one hold it across calls that take the context, which is face 1's shape and not
/// this accessor's — spelled as `pDqLayersList` the borrow is disjoint from every
/// other field those calls touch.
#[inline]
pub fn cur_dq_layer(list: &mut Option<Box<DqLayerState>>) -> Option<&mut DqLayerState> {
    list.as_deref_mut()
}

/// The parse-only descriptor, or null when the decoder is not in parse-only mode.
///
/// [`cur_dq_layer`]'s shape and [`cur_dq_layer`]'s discipline (T5.R4): one live
/// derivation at a time, and the three sites that take one —
/// `DecodeFrameConstruction`'s two arms and `CheckAndFinishLastPic`'s reset — hold it
/// across `cur_au` and context-field writes only, never across a second derivation of
/// this field.
///
/// **T5.Z1's parameter rule** (T5.Z3): the field, not the context — the four
/// consumers all write the descriptor beside reads of other context fields.
#[inline]
pub fn parser_bs(bs: &mut Option<Box<SParserBsInfo>>) -> Option<&mut SParserBsInfo> {
    bs.as_deref_mut()
}

/// The pool for the api layer's two release paths, **from the field** (T5.Z3).
///
/// `CWelsDecoder::ReleaseBufferedReadyPicture*` evaluate `pCtx ? pCtx->pPicBuff :
/// m_pPicBuff` into one local and pass it on. This accessor hands back the borrow and
/// the api site turns it into a pointer at the one line where the two sources meet —
/// `pool_for`, which since T8.A7 is also where the C's ternary lives, its
/// `m_pPicBuff` arm having been shown to be provably null in this port.
#[inline]
pub fn pic_pool_ptr(pool: &mut Option<Box<SPicBuff>>) -> Option<&mut SPicBuff> {
    pool.as_deref_mut()
}

/// The bracket top's pool borrow, handed down as a [`PicRefs`] (T5.P″2).
///
/// Every scope that resolves more than one handle takes this once at its top and
/// threads it; below a bracket top, `pRefs.get(id)` is the only way back to a
/// picture and `(*pCtx).pPicBuff` is not read at all. That invariant is what makes
/// the flip a change of two lines per bracket instead of a change at every use.
#[inline]
pub fn pic_refs(pool: &Option<Box<SPicBuff>>) -> PicRefs<'_> {
    PicRefs::over(pool.as_deref())
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
pub fn cur_and_refs(
    pool: &mut Option<Box<SPicBuff>>,
    cur: Option<PicId>,
) -> (Option<&mut SPicture>, PicRefs<'_>) {
    pic_and_refs_mut(pool, cur)
}

/// [`cur_and_refs`] for a bracket whose mutable half is **not** `pCtx->pDec`: the
/// error-concealment prefetch, which writes into the slot it just took from the pool
/// while reading the previous DPB picture out of another one.
#[inline]
pub fn pic_and_refs(
    pool: &mut Option<Box<SPicBuff>>,
    slot: Option<PicId>,
) -> (Option<&mut SPicture>, PicRefs<'_>) {
    match (pool.as_deref_mut(), slot) {
        (Some(pool), Some(id)) => pool.cur_and_rest_mut(id),
        (Some(pool), None) => (None, pool.refs()),
        (None, _) => (None, PicRefs::over(None)),
    }
}

/// [`pic_and_refs`]'s former borrow-only name. The pointer bracket and the borrow
/// bracket were the same split with two contracts (T5.AB3); **T5b.2 retired the
/// pointer one**, so there is one bracket and this is an alias.
pub use self::pic_and_refs as pic_and_refs_mut;

/// The slice header the decode loop last started on — `pCtx->pSliceHeader`'s
/// replacement (T5b.3), resolved from [`SWelsDecoderContext::slice_hdr_nal`].
#[inline]
pub fn slice_header_of(pCtx: &SWelsDecoderContext) -> Option<&SSliceHeader> {
    let i = pCtx.slice_hdr_nal?;
    pCtx.access_unit
        .as_deref()
        .and_then(|au| au.node(i))
        .map(|nal| &nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader)
}

/// Entry `i` of reference list `list` — the **handle**, without touching the pool.
///
/// The lists live in the context, not in the pool, so reading one below a bracket
/// top is not a pool access: `pRefs.get(ref_id(pCtx, list, i))` is [`ref_pic`] split
/// at exactly the line the flip moves.
#[inline]
pub fn ref_id(refs: &SRefPic, list: usize, i: usize) -> Option<PicId> {
    refs.pRefList[list][i]
}

/// The picture being decoded into — **the write target**, so this one is mutable.
///
/// The pool and the handle are two fields (T5.Z1): `pDec` is `Copy`, so reading it
/// takes no borrow and the result borrows `pPicBuff` alone.
#[inline]
pub fn dec_pic(pool: &mut Option<Box<SPicBuff>>, cur: Option<PicId>) -> Option<&mut SPicture> {
    pool_pic_mut(pool, cur)
}

/// Entry `i` of reference list `list` — `sRefPic.pRefList[list][i]` resolved.
///
/// The lists and the pool are two disjoint fields of the context, so they arrive as
/// two arguments and the result borrows only the pool (T5.Z1).
#[inline]
pub fn ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    list: usize,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pRefList[list][i])
}

/// Entry `i` of the short-term list — `sRefPic.pShortRefList[LIST_0][i]` resolved.
#[inline]
pub fn short_ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pShortRefList[LIST_0][i])
}

/// [`short_ref_pic`]'s mutable form — `WrapShortRefPicNum`'s per-entry stamp.
#[inline]
pub fn short_ref_pic_mut<'a>(
    pool: &'a mut Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a mut SPicture> {
    pool_pic_mut(pool, refs.pShortRefList[LIST_0][i])
}

/// Entry `i` of the long-term list — `sRefPic.pLongRefList[LIST_0][i]` resolved.
#[inline]
pub fn long_ref_pic<'a>(
    pool: &'a Option<Box<SPicBuff>>,
    refs: &SRefPic,
    i: usize,
) -> Option<&'a SPicture> {
    pool_pic(pool, refs.pLongRefList[LIST_0][i])
}

// ============================================================================
// The api-owned aliases — retired (T8.A4–A7)
// ============================================================================
//
// **This section held `api_alias` and `api_alias_mut`, and the argument for them.**
// The context used to hold nine pointers to objects it did not own — `pParam`,
// `pLastDecPicInfo`, `pDecoderStatistics`, `pStreamSeqNum`, `pMemAlign`,
// `pVlcTable`, `pPictInfoList`, `pPictReoderingStatus` and `pArgDec` — every one
// stamped in `api/codec_api.rs` from a field of `CWelsDecoderImpl`. They were the
// reason `deny(unsafe_code)` could not go on eight decoder modules: not one of the
// ~200 use sites owned a pointer, they all *dereferenced the context's*, and a
// dereference is what the lint forbids. Two accessors concentrated that into two
// `unsafe` items with one written obligation, and the note ended by saying whose
// the fields were: **Phase 8's**, "the fix is for `CWelsDecoderImpl` to hand the
// context borrows or owned values at construction".
//
// That is what happened. Eight of the nine are the context's own fields; `pMemAlign`
// is deleted with the allocator it named; `pArgDec` is a `void*` the port never
// reads. The `Option`s the accessors returned were the C's null tests on blocks the
// context in fact owns, so they retire with the pointers rather than being kept as
// guards — the one place that cost a decision is `WelsDecodeInitAccessUnitStart`'s
// `else` arm for a null `pStreamSeqNum` (`decoder_core.cpp:2265`), which is
// unreachable code once the counter is a field.
//
// `src/decoder/` is down to **one** `#[allow(unsafe_code)]` item — `SPicture`'s
// `data_ptr` in `picture.rs`, whose one production use writes `ppDst` across the
// ABI.

/// The previous decoded picture's **handle**, without touching the pool — the
/// `ref_id`-shaped half of [`prev_dpb_pic`], for the error-concealment brackets that
/// resolve it through their own [`PicRefs`].
#[inline]
pub fn prev_dpb_id(pLastDecPicInfo: &SWelsLastDecPicInfo) -> Option<PicId> {
    pLastDecPicInfo.pPreviousDecodedPictureInDpb
}

/// [`prev_dpb_pic`]'s mutable form — the api layer's buffering path, which takes a
/// DPB reference on it (`iRefCount += 1`).
///
/// The handle comes from [`prev_dpb_id`], which is the caller's read of a raw field
/// the api layer owns; the pool is the field this resolves in (T5.Z1).
#[inline]
pub fn prev_dpb_pic_mut(
    pool: &mut Option<Box<SPicBuff>>,
    prev: Option<PicId>,
) -> Option<&mut SPicture> {
    pool_pic_mut(pool, prev)
}

/// Whether an access unit exists and has at least one NAL queued in it.
///
/// The `!pCurAu.is_null() && pCurAu->uiAvailUnitsNum > 0` guard, which the port
/// spelled at eight sites and which is the actual question every one of them asks.
#[inline]
pub fn au_has_nals(pCtx: &mut SWelsDecoderContext) -> bool {
    matches!(cur_au(&mut pCtx.access_unit), Some(au) if au.uiAvailUnitsNum > 0)
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
pub fn mark_au_ready(pCtx: &mut SWelsDecoderContext) -> bool {
    // Two disjoint fields, in the order T5.O8 cost a Miri round trip to learn.
    match cur_au(&mut pCtx.access_unit) {
        Some(au) if au.uiAvailUnitsNum > 0 => {
            au.uiEndPos = au.uiAvailUnitsNum - 1;
            pCtx.bAuReadyFlag = true;
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
/// **T5b.2: the view is an ordinary disjoint-field split, and the macro is why it
/// can be one.** Borrowing several *distinct* fields of one struct at once is safe
/// Rust — but only inside a single function body, which is exactly the property a
/// helper taking `&mut SWelsDecoderContext` destroys. `slice_split` needs
/// `pPicBuff` mutably *beside* this construction, so the construction has to be
/// expanded into its caller rather than called; a macro is the spelling for that.
/// `addr_of_mut!` was the previous answer to the same problem and it needed the
/// lint's permission to say what the borrow checker now checks.
///
/// The invariants each field rests on are S23's and unchanged — they are recorded
/// at the field declarations in [`SliceCtx`].
macro_rules! slice_view {
    ($ctx:expr, $reader:expr, $threads:expr) => {{
        // The `sRawData` borrow is named because two fields of the view are derived
        // from it and the second one (`rbsp`) is a window into the first.
        let raw: &RawDataBuffer = &$ctx.sRawData;
        SliceCtx {
            sRawData: raw,
            rbsp: match $reader {
                Some(reader) => raw.rbsp_window(reader),
                None => &[],
            },
            sCabacDecEngine: &mut $ctx.sCabacDecEngine,
            pCabacCtx: &mut $ctx.pCabacCtx,
            bMbRefConcealed: &mut $ctx.bMbRefConcealed,
            iErrorCode: &mut $ctx.iErrorCode,
            iTotalNumMbRec: &mut $ctx.iTotalNumMbRec,
            pTempDec: &mut $ctx.pTempDec,
            sSpsPpsCtx: &$ctx.sSpsPpsCtx,
            sFmoList: &$ctx.sFmoList,
            sRefPic: &$ctx.sRefPic,
            // **T8.A6: a field, so there is nothing to resolve and nothing to
            // panic about.** This was `api_alias(..).expect(..)` over a raw alias
            // into `CWelsDecoderImpl::sVlcTable`, and the `expect` argued that
            // `Initialize` installs it before any slice reaches a bracket.
            // `WelsOpenDecoder` fills it now, where `decoder.cpp:606` fills it.
            pVlcTable: &$ctx.pVlcTable,
            pDequant_coeff_buffer4x4: &$ctx.pDequant_coeff_buffer4x4,
            pDequant_coeff_buffer8x8: &$ctx.pDequant_coeff_buffer8x8,
            pGetI16x16LumaPredFunc: &$ctx.pGetI16x16LumaPredFunc,
            pGetI4x4LumaPredFunc: &$ctx.pGetI4x4LumaPredFunc,
            pGetIChromaPredFunc: &$ctx.pGetIChromaPredFunc,
            pGetI8x8LumaPredFunc: &$ctx.pGetI8x8LumaPredFunc,
            pIdctResAddPredFunc: $ctx.pIdctResAddPredFunc,
            pIdctFourResAddPredFunc: $ctx.pIdctFourResAddPredFunc,
            pIdctResAddPredFunc8x8: $ctx.pIdctResAddPredFunc8x8,
            eSliceType: $ctx.eSliceType,
            eIntraPredConstraint: $ctx.eIntraPredConstraint,
            bUseScalingList: $ctx.bUseScalingList,
            bDequantCoeff4x4Init: $ctx.bDequantCoeff4x4Init,
            bRPLRError: $ctx.bRPLRError,
            bParseOnly: parse_only(&$ctx.pParam),
            bEcActive: ec_active_idc(&$ctx.pParam)
                != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE,
            iCurSeqIntervalMaxPicWidth: $ctx.iCurSeqIntervalMaxPicWidth,
            iThreadCount: $threads,
            active_sps: $ctx.active_sps,
            active_pps: $ctx.active_pps,
            fmo_id: $ctx.fmo_id,
        }
    }};
}

/// The view alone, for the brackets that hold no picture.
#[inline]
pub fn slice_ctx<'a>(pCtx: &'a mut SWelsDecoderContext, reader: Option<&BsReader>) -> SliceCtx<'a> {
    // Everything reaching the context *as a whole* happens before the first field
    // borrow — T5.O8's ordering lesson, now a compile error rather than a comment.
    let iThreadCount = crate::decoder::decoder_core::GetThreadCount(pCtx);
    slice_view!(pCtx, reader, iThreadCount)
}

/// **The bracket top's split** (T5.Z4) — the pool's two halves *and* the view, out
/// of one borrow of the context.
///
/// Session Y measured why this has to be one function: with the context a `&mut`,
/// `cur_and_refs(pCtx)` followed by `slice_ctx(pCtx, …)` is two mutable borrows of
/// one object, and no ordering of the two calls fixes it. Inside one function the
/// disjointness is the compiler's business — `pPicBuff` goes to the pool half and
/// every other field to the view, no field twice — which is why [`SliceCtx`] was
/// built without the pool in it (T5.Y2's own clause).
///
/// The six bracket tops are `WelsDecodeSlice`, `WelsDecodeAndConstructSlice`,
/// `WelsTargetSliceConstruction`, `ComputeColocatedTemporalScaling`,
/// `CheckRefPicturesComplete` and `DoErrorConSliceMVCopy`.
///
/// **T5b.2: safe, and the pool half is a borrow.** What forced the raw spelling was
/// `PicRefs::get`'s F42 arm — the view held an address into the current picture, so
/// a `&mut` on that picture had to not exist. With F42 answered by *identity*
/// (`RefSlot::Current`, `PicRefs::resolve`, `mc_luma_same`) the picture is an
/// ordinary `&mut` and this is an ordinary disjoint-field split: `pPicBuff` to the
/// pool half, every other field to the view, no field twice, and the compiler
/// checks it rather than a comment claiming it.
/// **T5b.3: the NAL node comes out of the same split.** The slice bracket needs the
/// node's own bit reader beside the view, and the node lives in `access_unit` — a
/// *field*, disjoint from every field the view takes and from `pPicBuff`. Handing it
/// back here is what lets the caller hold all three at once; deriving it outside would
/// be a second borrow of the context.
#[inline]
pub fn slice_split<'a>(
    pCtx: &'a mut SWelsDecoderContext,
    nal: Option<usize>,
) -> (
    Option<&'a mut SPicture>,
    PicRefs<'a>,
    SliceCtx<'a>,
    Option<&'a mut SNalUnit>,
) {
    // Everything reaching the context *as a whole* happens before the first field
    // borrow (T5.O8's ordering lesson, now enforced by the borrow checker rather
    // than by care).
    let iThreadCount = crate::decoder::decoder_core::GetThreadCount(pCtx);
    let cur = pCtx.pDec;
    let (pDec, pRefs) = pic_and_refs(&mut pCtx.pPicBuff, cur);
    let node = nal.and_then(|i| pCtx.access_unit.as_deref_mut().and_then(|au| au.node_mut(i)));
    // The window the view carries is derived from the reader's *position*, and the
    // slice borrows `sRawData` rather than the node — so the reader travels by value
    // and the node's borrow is free to leave with it.
    let reader = node.as_deref().map(|n| n.sNalData.sVclNal.sSliceBitsRead);
    let view = slice_view!(pCtx, reader.as_ref(), iThreadCount);
    (pDec, pRefs, view, node)
}

/// **The bracket top's split for a scope that writes the picture and reads no
/// references** (T5.AA2) — the current picture as a *borrow*, beside the view.
///
/// [`slice_split`] hands the picture back as a `PPicture` and must: the decode
/// bracket resolves reference lists beside it, and a malformed stream can put the
/// picture being decoded into one (**F42**), so the two have to share a tag. The
/// deblocking bracket resolves no references at all — it snapshots the reference
/// *ids* and reads only the current picture's planes and per-macroblock arrays — so
/// here the picture is a real `&mut` and the whole family below it converts.
///
/// `None` is the state the null `PPicture` stood for: no pool, or no current
/// picture. The one caller skips deblocking there, which is what the null arms
/// inside the family used to stand in for and never reached — the first read after
/// them dereferenced the same null.
///
/// # Safety
/// [`slice_ctx`]'s contract, unchanged.
#[inline]
/// **Enumerated exception — the per-slice view constructors** (W6's settlement,
/// steward at `a158183c`: *"the constructor is the enumerated exception, and it
/// does not live in `decode_slice.rs`"*). All three build field-precise borrows out
/// of one context by `addr_of_mut!`, which is the only way to hand out disjoint
/// borrows the compiler cannot prove disjoint through a method; `slice_split` also
/// carries the `PPicture` survivor's producer half (F42). **Phase 8's**, with the
/// `PPicture` option-1/2 revisit — everything below them is safe because they are
/// not.
pub fn pic_split<'a>(
    pCtx: &'a mut SWelsDecoderContext,
) -> (Option<&'a mut SPicture>, SliceCtx<'a>) {
    // `slice_split`'s construction verbatim, with the reference half dropped.
    let (pDec, _refs, view, _nal) = slice_split(pCtx, None);
    (pDec, view)
}

/// The reference set a DPB operation acts on — **the selector travels, not the
/// borrow** (T5.Z4).
///
/// The marking family took `pRefPic: &mut SRefPic` beside `pCtx`, which is one of
/// the context's own fields held across a borrow of its parent. The caller passes
/// `bTmpRefSet` instead and every use re-acquires here, so no borrow outlives one
/// expression — S25's fix shape, applied to the one family that needed it.
///
/// `sTmpRefPic` is the threading arm's set (F36 owns whether it survives at all);
/// `sRefPic` is every other caller's.
#[inline]
pub fn ref_set(pCtx: &mut SWelsDecoderContext, tmp: bool) -> &mut SRefPic {
    if tmp { &mut pCtx.sTmpRefPic } else { &mut pCtx.sRefPic }
}

/// A view over a test context, wired the way `Initialize` wires the real one.
///
/// The only wiring [`slice_ctx`] needs and a fresh context does not have is the VLC
/// table, which `WelsOpenDecoder` fills in production (`decoder.cpp:606`); the
/// fixture copies the caller's own table into the context's field. It took the
/// table by `&mut` and stamped a pointer until T8.A6 made the field owned.
#[cfg(test)]
pub(crate) fn test_slice_ctx<'a>(
    ctx: &'a mut SWelsDecoderContext,
    vlc: &SVlcTable,
) -> SliceCtx<'a> {
    ctx.pVlcTable = *vlc;
    slice_ctx(ctx, None)
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
    /// The decoding parameters, **owned** (T8.A5, F41).
    ///
    /// The C++ context owns this block too — `InitDecoderCtx` allocates it
    /// (`welsDecoderExt.cpp:426`) and `DecoderConfigParam` `memcpy`s the caller's
    /// values into it — and `CWelsDecoder` has no parameter member of its own. The
    /// port had invented one, `CWelsDecoderImpl::param`, and pointed this field at
    /// it: an alias into another object, overwritten on every `Initialize` before
    /// the existing-context test, and read by the teardown's `bParseOnly` arm. That
    /// is **F41**, and the fix is the C++'s own arrangement rather than a guard.
    ///
    /// Its one writer is [`DecoderConfigParam`](crate::decoder::decoder_core::DecoderConfigParam),
    /// from `Initialize`; `SetOption(DECODER_OPTION_ERROR_CON_IDC)` writes the one
    /// field the C++ lets it write, in the same place the C++ writes it
    /// (`welsDecoderExt.cpp:535`).
    pub pParam: SDecodingParam,
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
    /// `CWelsDecoderImpl::sVlcTable`, **owned** (T8.A6). The C++ declares the
    /// context's slot `void*` and points it at a `CWelsDecoder` member; the port
    /// named the type at T5b.2 and owns the value now — every entry is a
    /// `&'static` table, so the struct is four fat pointers and the aliasing the C
    /// used to avoid copying it bought nothing. `WelsOpenDecoder` fills it, where
    /// `decoder.cpp:606` calls `InitVlcTable (pCtx->pVlcTable)`.
    pub pVlcTable: SVlcTable,
    pub sBs: BsReader,
    pub sSpsPpsCtx: SWelsDecoderSpsPpsCTX,
    pub bHasNewSps: bool,
    pub sFrameCrop: SPosOffset,
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
    /// **The NAL under decode, as its index in the access unit** (T5b.3).
    ///
    /// It was `*mut SNalUnit` — a stored alias *into* a node, and the reason the
    /// access unit's slots have to stay raw (`TagAccessUnits::nal_units`' note: a
    /// container that lends `&mut` to a node invalidates every live alias into it).
    /// An index aliases nothing, so the field costs its one reader an `au.node(i)`
    /// and buys the container the right to own.
    ///
    /// `None` is the C's `pCtx->pNalCur = NULL`, which is also the state that reader
    /// sees in the C++ — `decoder_core.cpp:2491` nulls it and never writes it again
    /// (T5.M3's note, and F36 owns the arm that reads it).
    pub nal_cur: Option<usize>,
    /// The NAL whose slice header the *decode* loop last started on — `pSliceHeader`'s
    /// index (T5b.3).
    ///
    /// `pCtx->pSliceHeader` was a raw pointer into that node, and its one reader is
    /// `ParseDecRefPicMarking`'s MMCO_RESET arm, which zeroes the POC of the header the
    /// decode loop is on *as well as* the one being parsed. The two are different NALs,
    /// so this is a separate field from [`nal_cur`](Self::nal_cur) rather than a second
    /// name for it: `nal_cur` is stamped when the access-unit loop picks a NAL up, this
    /// one when `WelsDqLayerDecodeStart` starts decoding it, and before the first slice
    /// decode it is `None` — the null the pointer held.
    pub slice_hdr_nal: Option<usize>,
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
    /// `CWelsDecoderImpl::iStreamSeqNum`, **owned** (T8.A6).
    pub pStreamSeqNum: i32,
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
    /// `CWelsDecoderImpl::sLastDecPicInfo`, **owned** (T8.A6). `decoder_init_c`
    /// runs `WelsDecoderLastDecPicInfoDefaults` over it at context construction,
    /// where `CWelsDecoder::InitDecoder` runs it (`welsDecoderExt.cpp:386`) — the
    /// defaults are not zeros.
    pub pLastDecPicInfo: SWelsLastDecPicInfo,
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
    /// `CWelsDecoderImpl::sDecoderStatistics`, **owned** (T8.A6). Handed out whole
    /// by `DECODER_OPTION_GET_STATISTICS`, which is why its one declaration is the
    /// ABI's (T8.A1).
    pub pDecoderStatistics: SDecoderStatistics,
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
    pub lastReadyHeightOffset: [[i16; MAX_REF_PIC_COUNT]; LIST_A],
    /// `CWelsDecoderImpl::sPictInfoList`, **owned** (T8.A7).
    ///
    /// It held `*mut SPictInfo` — the C's decayed first-element pointer — until
    /// T5b.6 named the array, and the array until now was `CWelsDecoderImpl`'s with
    /// this field pointing at it. In the reference it is a `CWelsDecoder` member
    /// because a *threaded* decoder shares one reordering buffer across N contexts;
    /// with one context per decoder the context is where it belongs.
    pub pPictInfoList: [SPictInfo; 16],
    /// `CWelsDecoderImpl::sReoderingStatus`, **owned** (T8.A7).
    pub pPictReoderingStatus: SPictReoderingStatus,
    /// `CWelsDecoder::m_bIsBaseline` — reordering is bypassed entirely for baseline
    /// profiles. One of the three `CWelsDecoderImpl` scalars the reordering path
    /// carried beside the two buffers; they moved together at T8.A7, because a
    /// function that reads the buffers out of the context and the scalars out of the
    /// api object would need both pointers to say one thing.
    pub bIsBaseline: bool,
    /// `CWelsDecoder::m_iLastBufferedIdx`.
    pub iLastBufferedIdx: i32,
    /// `CWelsDecoder::m_uiDecodeTimeStamp` — the monotonic counter stamped onto each
    /// decoded picture as [`uiDecodingTimeStamp`](Self::uiDecodingTimeStamp); the
    /// no-reorder release path orders buffered pictures by it.
    pub uiDecodeTimeStamp: u32,
}
// **T5b.9: `PWelsDecoderContext` deleted (S18).** Every use in `src/decoder/` was
// a re-export chain or a doc comment quoting the C++'s signature; the one code use
// was `CopySpsPps`, an empty stub of the threaded decoder D3 deletes. No function
// in the port takes the context by pointer — T5.G1 closed that inventory — so the
// typedef named a shape nothing has.

/// **T5b.5 — the shell is retired and the constructor is the answer to S21's
/// question.** The context was built from `MaybeUninit::zeroed()` with the owning
/// fields written through the raw shell, because the struct "is several MiB" and a
/// by-value constructor was said not to be an option at any point in the phase.
/// Measured at T5b.5's open, it was **573,576 bytes** — Phase 5 moved the bulk into
/// owned containers, and the premise expired with them. It is **572,784** at T5b.9,
/// which deleted `SSps::pSLevelLimits` and so 8 bytes from each of the 99 parameter
/// sets the context carries.
///
/// What the shell cost was not the `unsafe`. It was that *every field's initial value
/// was unreadable*: 109 fields whose meaning was "whatever all-zero happens to be for
/// this type", which is how `pActiveLayerSps` spent a session reading back as
/// `Some(SpsRef { id: 0 })` (T5.Z1) and how [`SRefPic`], [`SCopyFunc`],
/// [`SDeblockingFunc`] and [`SWelsDecoderSpsPpsCTX`] each acquired a `Default` that is
/// deliberately *not* their zero. Each of those now has a `memset_zero` beside its
/// `Default`, and this constructor names which of the two the context is born with.
impl Default for SWelsDecoderContext {
    fn default() -> Self {
        Self {
            // `SLogContext::default()` is a null handler and a null cookie.
            sLogCtx: SLogContext::default(),
            pArgDec: std::ptr::null_mut(),
            // The two owning buffers. A zeroed `Vec` is a null pointer where a
            // dangling-aligned one is required — this is the pair the shell existed
            // to write, and the whole of what it could not express as a value.
            sRawData: RawDataBuffer::default(),
            sSavedData: RawDataBuffer::default(),
            pParam: SDecodingParam::default(),
            uiCpuFlag: 0,
            eVideoType: VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
            bHaveGotMemory: false,
            iImgWidthInPixel: 0,
            iImgHeightInPixel: 0,
            iLastImgWidthInPixel: 0,
            iLastImgHeightInPixel: 0,
            bFreezeOutput: false,
            sCurNalHead: SNalUnitHeader::default(),
            eSliceType: EWelsSliceType::P_SLICE,
            bUsedAsRef: false,
            iFrameNum: 0,
            iErrorCode: 0,
            // **Not the zero pattern, and it never was**: `TagFmo::default`'s
            // `iSliceGroupType` is `-1` (T5.R3), and the shell wrote this array
            // element by element for exactly that reason — the map is a `Vec` and a
            // zeroed `Vec` is invalid. `SFmo` is not `Copy`, so this is 256 calls
            // rather than a repeat expression.
            sFmoList: std::array::from_fn(|_| SFmo::default()),
            fmo_id: None,
            iActiveFmoNum: 0,
            pDec: None,
            pTempDec: None,
            // `WelsResetRefPic`'s `-1` belongs to `Default`, not to the zeroing.
            sRefPic: SRefPic::memset_zero(),
            sTmpRefPic: SRefPic::memset_zero(),
            // The shell `WelsCreateDecoder` used to build: `SVlcTable`'s sub-tables
            // are `&'static` slices, so a zeroed value is invalid and the empty slice
            // is what "not yet initialised" spells. `WelsOpenDecoder` overwrites it.
            pVlcTable: SVlcTable {
                kpCoeffTokenVlcTable: [[&[]; 8]; 4],
                kpChromaCoeffTokenVlcTable: &[],
                kpZeroTable: [&[]; 7],
                kpTotalZerosTable: [[&[]; 15]; 2],
            },
            sBs: BsReader::default(),
            sSpsPpsCtx: SWelsDecoderSpsPpsCTX::memset_zero(),
            bHasNewSps: false,
            sFrameCrop: SPosOffset::default(),
            pPicBuff: None,
            iPicQueueNumber: 0,
            access_unit: None,
            // **T5b.8, F56's ruling: `None` is the faithful value, and the `Some` the
            // zeroed shell produced here was a layout artifact.** The C memsets an
            // `SSps*` to NULL; the port's spelling of that null is `None`. What the
            // shell wrote instead was `Some(SpsRef { id: 0, subset: false })`, because
            // `Option<SpsRef>` keeps its niche in `SpsRef`'s `bool` and `0` is a valid
            // `false` — F54's class, on a field T5.Z1 did not reach. Nothing
            // transcribed that `Some`; the layout algorithm chose it.
            //
            // What the correction buys is a live arm: `AllocPicBuffOnNewSeqBegin`'s
            // `else` scan — T5.R6's replacement for the C++'s pointer-valued
            // fallback — could not run while this field was never `None`. It can now.
            // Gated on the full malformed corpus and the conformance set, both
            // unmoved — see the commit and F56's close.
            active_sps: None,
            active_pps: None,
            pDqLayersList: None,
            nal_cur: None,
            slice_hdr_nal: None,
            uiNalRefIdc: 0,
            iPicWidthReq: 0,
            iPicHeightReq: 0,
            uiTargetDqId: 0,
            bEndOfStreamFlag: false,
            bInstantDecFlag: false,
            bInitialDqLayersMem: false,
            bOnlyOneLayerInCurAuFlag: false,
            bReferenceLostAtT0Flag: false,
            iTotalNumMbRec: 0,
            bParamSetsLostFlag: false,
            bCurAuContainLtrMarkSeFlag: false,
            iFrameNumOfAuMarkedLtr: 0,
            uiCurIdrPicId: 0,
            bNewSeqBegin: false,
            bNextNewSeqBegin: false,
            pStreamSeqNum: 0,
            iSeqNum: 0,
            bFramePending: false,
            bFrameFinish: false,
            iNalNum: 0,
            sSpsBsInfo: [SSpsBsInfo::default(); MAX_SPS_COUNT],
            sSubsetSpsBsInfo: [SSpsBsInfo::default(); MAX_PPS_COUNT],
            sPpsBsInfo: [SPpsBsInfo::default(); MAX_PPS_COUNT],
            pParserBsInfo: None,
            // The dispatch tables are uninstalled until `WelsInitDecoderFuncs`; the
            // C's zero is a null function pointer and `None` is that null.
            pGetI16x16LumaPredFunc: [None; 7],
            pGetI4x4LumaPredFunc: [None; 14],
            pGetIChromaPredFunc: [None; 7],
            pIdctResAddPredFunc: None,
            pIdctFourResAddPredFunc: None,
            sMcFunc: crate::decoder::error_concealment::SMcFunc::default(),
            pGetI8x8LumaPredFunc: [None; 14],
            pIdctResAddPredFunc8x8: None,
            sCopyFunc: SCopyFunc::memset_zero(),
            sDeblockingFunc: SDeblockingFunc::memset_zero(),
            iCurSeqIntervalTargetDependId: 0,
            iCurSeqIntervalMaxPicWidth: 0,
            iCurSeqIntervalMaxPicHeight: 0,
            // `Constrain0 = 0` is the zero pattern *and* the fallback every former
            // uninstalled slot named — `decode_slice.rs`'s note is the authority.
            eIntraPredConstraint: crate::decoder::decode_slice::IntraPredConstraint::Constrain0,
            iFeedbackVclNalInAu: 0,
            iFeedbackTidInAu: 0,
            iFeedbackNalRefIdc: 0,
            bAuReadyFlag: false,
            bPrintFrameErrorTraceFlag: false,
            iIgnoredErrorInfoPacketCount: 0,
            pTraceHandle: std::ptr::null_mut(),
            pLastDecPicInfo: SWelsLastDecPicInfo::default(),
            // 191,360 bytes of it, and `WelsCabacGlobalInit` overwrites every entry on
            // the first CABAC access unit — `bCabacInited` below is the guard.
            sWelsCabacContexts: [[[SWelsCabacCtx::default(); WELS_CONTEXT_COUNT]; WELS_QP_MAX + 1]; 4],
            bCabacInited: false,
            pCabacCtx: [SWelsCabacCtx::default(); WELS_CONTEXT_COUNT],
            // A zeroed engine is inert rather than null-pointered: `pos = 0` against an
            // empty window takes the ladder's error arm (T5.O2).
            sCabacDecEngine: SWelsCabacDecEngine::default(),
            dDecTime: 0.0,
            pDecoderStatistics: SDecoderStatistics::default(),
            iMbEcedNum: 0,
            iMbEcedPropNum: 0,
            iMbNum: 0,
            bMbRefConcealed: false,
            bRPLRError: false,
            iECMVs: [[0; 2]; 16],
            pECRefPic: [None; 16],
            uiTimeStamp: 0,
            uiDecodingTimeStamp: 0,
            pDequant_coeff_buffer4x4: [[[0; 16]; 52]; 6],
            pDequant_coeff_buffer8x8: [[[0; 64]; 52]; 6],
            iDequantCoeffPpsid: 0,
            // `WelsCalcDeqCoeffScalingList` writes both buffers and this flag in one
            // block; the flag is the initialized test the null test used to be.
            bDequantCoeff4x4Init: false,
            bUseScalingList: false,
            lastReadyHeightOffset: [[0; MAX_REF_PIC_COUNT]; LIST_A],
            pPictInfoList: [SPictInfo::default(); 16],
            pPictReoderingStatus: SPictReoderingStatus::default(),
            bIsBaseline: false,
            iLastBufferedIdx: 0,
            uiDecodeTimeStamp: 0,
        }
    }
}

impl SWelsDecoderContext {
    /// The context on the heap, which is where every caller wants it: 572,784 bytes is
    /// under a test thread's 2 MiB stack but not by a margin worth spending, and
    /// `Box::new` of a struct literal is the one shape the optimizer can build in
    /// place.
    ///
    /// **This is the context's real constructor** and the name every call site already
    /// spells; only its body changed at T5b.5.
    pub fn new_boxed() -> Box<Self> {
        Box::new(Self::default())
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

    /// **F56's red-under-revert test (T5b.8).** The context is born with no active
    /// SPS: the C memsets an `SSps*` to NULL and `None` is that null. What made this
    /// worth a test is that the wrong answer was *invisible* — `Option<SpsRef>` keeps
    /// its niche in `SpsRef`'s `bool`, so the zeroed shell read back as
    /// `Some(SpsRef { id: 0, subset: false })` and every reader saw "SPS 0 is
    /// already active" before a stream had sent one. Restore that `Some` in
    /// [`SWelsDecoderContext::default`] and this assertion is the one that fails.
    #[test]
    fn the_context_is_born_with_no_active_sps() {
        let ctx = SWelsDecoderContext::new_boxed();
        assert!(
            ctx.active_sps.is_none(),
            "active_sps was {:?}; the C's memset leaves a null SSps*",
            ctx.active_sps
        );
        // The third site F56 names — the prefix NAL's VCL arm, which nothing
        // writes and nothing reads, now agrees with the two that are read.
        assert!(ctx
            .sSpsPpsCtx
            .sPrefixNal
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .sps_ref
            .is_none());
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
        // `new_zeroed().assume_init()` stopped being legal at T3.3: the context now
        // owns `Vec`s, and a zeroed `Vec` is an invalid value. `Default` constructs
        // the same zeroed context with those fields written properly.
        let mut ctx = SWelsDecoderContext::new_boxed();

        {
            let pCtx = &mut *ctx;
            (*pCtx).pPicBuff = CreatePicBuff(false, 4, 64, 64);
            assert!((*pCtx).pPicBuff.is_some());
            assert_eq!(pic_pool_mut(pCtx).map(|pool| pool.capacity()), Some(4));

            // T5.P″1: the field *is* the out-parameter. `take()` reads the pool and
            // leaves the context naming nothing, which is what the C's
            // `*ppPicBuf = NULL` was for.
            let pool = (*pCtx).pPicBuff.take();
            DestroyPicBuff(pCtx, pool);
            assert!((*pCtx).pPicBuff.is_none());
        }
    }
}



