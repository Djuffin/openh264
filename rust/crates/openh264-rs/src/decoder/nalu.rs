#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]
// **Phase 5b, T5b.6: this file's `unsafe` is gone and no exception is enumerated.**
// `src/decoder/` carries **two** `#[allow(unsafe_code)]` items in total (T8.A8), and
// both are `picture.rs`'s Miri provenance tests for `data_ptr` — the instruments S28
// mandates for that accessor, not production code. The two that used to sit beside
// them, `decoder_context.rs`'s `api_alias`/`api_alias_mut`, retired with the api-owned
// fields they dereferenced. Nothing here is one of them.

//! # H.264 / AVC and SVC NAL Unit and Access Unit Parser (`nalu.h` & `au_parser.cpp`)
//!
//! Translated from `codec/decoder/core/inc/nalu.h`, `codec/decoder/core/inc/au_parser.h`,
//! and `codec/decoder/core/src/au_parser.cpp`.
//!
//! This module provides:
//! 1. In-memory data structures for H.264 Network Abstraction Layer (NAL) units ([`SNalUnit`])
//!    and Access Units ([`SAccessUnit`]).
//! 2. NAL header parsing and SVC extension unpacking ([`ParseNalHeader`], [`DecodeNalHeaderExt`]).
//!    (Annex B start-code scanning is `split_annexb_units` in `lib.rs`; the unused
//!    `DetectStartCodePrefix` transliteration was deleted dead at T3.3.)
//! 4. Access Unit (AU) boundary detection algorithms ([`CheckAccessUnitBoundary`], [`CheckAccessUnitBoundaryExt`]).
//! 5. Syntactic Parameter Set parsers for Sequence Parameter Sets ([`ParseSps`], [`DecodeSpsSvcExt`]),
//!    Picture Parameter Sets ([`ParsePps`]), Video Usability Information ([`ParseVui`]),
//!    and frequency scaling matrices ([`ParseScalingList`], [`SetScalingListValue`]).
//! 6. Access-unit NAL node storage ([`TagAccessUnits::with_nodes`], [`MemGetNextNal`]).

use std::ffi::c_void;

use crate::decoder::bit_stream::*;
use crate::decoder::cabac_decoder::*;
use crate::decoder::dec_golomb::*;
use crate::decoder::decode_mb_aux::*;
use crate::decoder::decode_slice::*;
use crate::decoder::decoder_context::*;
use crate::decoder::decoder_core::*;
use crate::decoder::error_concealment::*;
use crate::decoder::fmo::*;
use crate::decoder::get_intra_predictor::*;
use crate::decoder::manage_dec_ref::*;
use crate::decoder::mv_pred::*;
use crate::decoder::parameter_sets::*;
use crate::decoder::parse_mb_syn_cabac::*;
use crate::decoder::parse_mb_syn_cavlc::*;
use crate::decoder::pic_queue::*;
use crate::decoder::picture::*;
use crate::decoder::slice::*;

// Explicit imports to resolve glob ambiguities
use crate::decoder::bit_stream::{BsReader, ERR_NONE, ERR_INVALID_PARAMETERS, ERR_INFO_OUT_OF_MEMORY};
use crate::safe::bits::BsCursor;

use crate::decoder::dec_golomb::{BsGetOneBit, BsGetUe, BsGetSe, BsGetBits};
use crate::decoder::decoder_context::{
    SWelsDecoderContext, MAX_LAYER_NUM, SPosOffset, active_pps, active_sps,
    pps_of, sps_of, subset_sps_of, SpsRef,
};
use crate::decoder::parameter_sets::{SSps, SPps, SSubsetSps, SLevelLimits, MAX_SPS_COUNT, MAX_PPS_COUNT, MAX_MB_SIZE, MAX_SLICEGROUP_IDS};
use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SRefBasePicMarking, MMCO_END, MMCO_SHORT2UNUSED, MMCO_LONG2UNUSED, MAX_MMCO_COUNT, MAX_REF_PIC_COUNT};



// ============================================================================
// Constants and Syntax Limits
// ============================================================================

pub const MAX_NAL_UNIT_NUM_IN_AU: usize = 32;
pub const NAL_UNIT_HEADER_EXT_SIZE: usize = 3;
pub const SPS_PPS_BS_SIZE: usize = 128;

pub const SPS_LOG2_MAX_FRAME_NUM_MINUS4_MAX: u32 = 12;
pub const SPS_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4_MAX: u32 = 12;
pub const SPS_NUM_REF_FRAMES_IN_PIC_ORDER_CNT_CYCLE_MAX: u32 = 255;
pub const SPS_MAX_NUM_REF_FRAMES_MAX_VAL: u32 = 16;

pub const PPS_PIC_INIT_QP_QS_MIN: i32 = 0;
pub const PPS_PIC_INIT_QP_QS_MAX: i32 = 51;
pub const PPS_CHROMA_QP_INDEX_OFFSET_MIN: i32 = -12;
pub const PPS_CHROMA_QP_INDEX_OFFSET_MAX: i32 = 12;

pub const SCALING_LIST_DELTA_SCALE_MIN: i32 = -128;
pub const SCALING_LIST_DELTA_SCALE_MAX: i32 = 127;

pub const VUI_MAX_CHROMA_LOG_TYPE_TOP_BOTTOM_FIELD_MAX: u32 = 5;
pub const VUI_NUM_UNITS_IN_TICK_MIN: u32 = 1;
pub const VUI_TIME_SCALE_MIN: u32 = 1;
pub const VUI_MAX_BYTES_PER_PIC_DENOM_MAX: u32 = 16;
pub const VUI_MAX_BITS_PER_MB_DENOM_MAX: u32 = 16;
pub const VUI_LOG2_MAX_MV_LENGTH_HOR_MAX: u32 = 16;
pub const VUI_LOG2_MAX_MV_LENGTH_VER_MAX: u32 = 16;
pub const VUI_MAX_DEC_FRAME_BUFFERING_MAX: u32 = 16;

pub const LOG2_MAX_FRAME_NUM_OFFSET: u32 = 4;
pub const LOG2_MAX_PIC_ORDER_CNT_LSB_OFFSET: i32 = 4;
pub const PIC_WIDTH_IN_MBS_OFFSET: i32 = 1;
pub const PIC_HEIGHT_IN_MAP_UNITS_OFFSET: i32 = 1;
pub const NUM_SLICE_GROUPS_OFFSET: u32 = 1;
pub const RUN_LENGTH_OFFSET: u32 = 1;
pub const NUM_REF_IDX_L0_DEFAULT_ACTIVE_OFFSET: u32 = 1;
pub const NUM_REF_IDX_L1_DEFAULT_ACTIVE_OFFSET: u32 = 1;
pub const PIC_INIT_QP_OFFSET: i32 = 26;
pub const PIC_INIT_QS_OFFSET: i32 = 26;

// Error Code Bitflags & Offsets
pub const ERR_LEVEL_PARAM_SETS: i32 = 1;
pub const ERR_INFO_INVALID_ESS: i32 = 2;
pub const ERR_INFO_UNSUPPORTED_NON_BASELINE: i32 = 3;
pub const ERR_INFO_SPS_ID_OVERFLOW: i32 = 4;
pub const ERR_INFO_INVALID_LOG2_MAX_FRAME_NUM_MINUS4: i32 = 5;
pub const ERR_INFO_INVALID_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4: i32 = 6;
pub const ERR_INFO_INVALID_NUM_REF_FRAME_IN_PIC_ORDER_CNT_CYCLE: i32 = 7;
pub const ERR_INFO_INVALID_POC_TYPE: i32 = 8;
pub const ERR_INFO_INVALID_MAX_MB_SIZE: i32 = 9;
pub const ERR_INFO_INVALID_MAX_NUM_REF_FRAMES: i32 = 10;
pub const ERR_INFO_UNSUPPORTED_MBAFF: i32 = 11;
pub const ERR_INFO_INVALID_CROPPING_DATA: i32 = 12;
pub const ERR_INFO_UNSUPPORTED_VUI_HRD: i32 = 13;
pub const ERR_INFO_PPS_ID_OVERFLOW: i32 = 14;
pub const ERR_INFO_INVALID_SLICEGROUP: i32 = 15;
pub const ERR_INFO_UNSUPPORTED_FMOTYPE: i32 = 16;
pub const ERR_INFO_REF_COUNT_OVERFLOW: i32 = 17;
pub const ERR_INFO_INVALID_PIC_INIT_QP: i32 = 18;
pub const ERR_INFO_INVALID_PIC_INIT_QS: i32 = 19;
pub const ERR_INFO_INVALID_CHROMA_QP_INDEX_OFFSET: i32 = 20;
pub const ERR_INFO_INVALID_SPS_ID: i32 = 21;
pub const ERR_SCALING_LIST_DELTA_SCALE: i32 = 22;

pub const dsBitstreamError: i32 = 0x04;
pub const dsNoParamSets: i32 = 0x10;
pub const dsOutOfMemory: i32 = 0x4000;

// Re-exported so the parser and `WriteBackActiveParameters` agree on the bits
// (`decoder_context.h`: PPS = 1, SPS = 2, SUBSETSPS = 4).
pub use crate::decoder::decoder_core::{OVERWRITE_NONE, OVERWRITE_PPS, OVERWRITE_SPS, OVERWRITE_SUBSETSPS};
pub use crate::decoder::decode_slice::{g_kuiZigzagScan, g_kuiZigzagScan8x8};

pub const EXTENDED_SAR: u8 = 255;
pub const NRI_PRI_LOWEST: u8 = 0;
pub const ERROR_CON_DISABLE: i32 = 0;

#[inline(always)]
pub fn GENERATE_ERROR_NO(level: i32, info: i32) -> i32 {
    (level << 16) | info
}

// ============================================================================
// NAL Unit Enumerations & Structures
// ============================================================================

/// H.264 NAL Unit Types (0 to 31).
///
/// Matches `enum EWelsNalUnitType` in `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsNalUnitType {
    #[default]
    NAL_UNIT_UNSPEC_0 = 0,
    NAL_UNIT_CODED_SLICE = 1,
    NAL_UNIT_CODED_SLICE_DPA = 2,
    NAL_UNIT_CODED_SLICE_DPB = 3,
    NAL_UNIT_CODED_SLICE_DPC = 4,
    NAL_UNIT_CODED_SLICE_IDR = 5,
    NAL_UNIT_SEI = 6,
    NAL_UNIT_SPS = 7,
    NAL_UNIT_PPS = 8,
    NAL_UNIT_AU_DELIMITER = 9,
    NAL_UNIT_END_OF_SEQ = 10,
    NAL_UNIT_END_OF_STR = 11,
    NAL_UNIT_FILER_DATA = 12,
    NAL_UNIT_SPS_EXT = 13,
    NAL_UNIT_PREFIX = 14,
    NAL_UNIT_SUBSET_SPS = 15,
    NAL_UNIT_RESV_16 = 16,
    NAL_UNIT_RESV_17 = 17,
    NAL_UNIT_RESV_18 = 18,
    NAL_UNIT_AUX_CODED_SLICE = 19,
    NAL_UNIT_CODED_SLICE_EXT = 20,
    NAL_UNIT_RESV_21 = 21,
    NAL_UNIT_RESV_22 = 22,
    NAL_UNIT_RESV_23 = 23,
    NAL_UNIT_UNSPEC_24 = 24,
    NAL_UNIT_UNSPEC_25 = 25,
    NAL_UNIT_UNSPEC_26 = 26,
    NAL_UNIT_UNSPEC_27 = 27,
    NAL_UNIT_UNSPEC_28 = 28,
    NAL_UNIT_UNSPEC_29 = 29,
    NAL_UNIT_UNSPEC_30 = 30,
    NAL_UNIT_UNSPEC_31 = 31,
}

#[inline(always)]
pub fn IS_SEI_NAL(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_SEI
}

#[inline(always)]
pub fn IS_SPS_NAL(t: EWelsNalUnitType) -> bool {
    // **F49, T5.U3 — SPS only.** `wels_common_defs.h:146` is
    // `#define IS_SPS_NAL(t) ((t) == NAL_UNIT_SPS)`; the subset-SPS is *not* in
    // it, and `IS_PARAM_SETS_NALS` (`:145`) is the macro that takes all three.
    // The port had the subset-SPS here, which opened the one gate this is used
    // for — `ParseNalHeader`'s "no Sequence Parameter Sets ahead of sequence"
    // check, the sole caller on either side — to a subset-SPS arriving with no
    // SPS before it. The C++ answers `dsNoParamSets` there and the port parsed
    // the subset-SPS and answered `dsErrorFree`.
    t == EWelsNalUnitType::NAL_UNIT_SPS
}

#[inline(always)]
pub fn IS_SUBSET_SPS_NAL(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_SUBSET_SPS
}

#[inline(always)]
pub fn IS_AU_DELIMITER_NAL(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_AU_DELIMITER
}

#[inline(always)]
pub fn IS_PARAM_SETS_NALS(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_SPS
        || t == EWelsNalUnitType::NAL_UNIT_SUBSET_SPS
        || t == EWelsNalUnitType::NAL_UNIT_PPS
}

#[inline(always)]
pub fn IS_VCL_NAL_AVC_BASE(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_CODED_SLICE || t == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
}

#[inline(always)]
pub fn IS_NEW_INTRODUCED_SVC_NAL(t: EWelsNalUnitType) -> bool {
    t == EWelsNalUnitType::NAL_UNIT_PREFIX || t == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
}

/// 1-byte standard NAL unit header.
///
/// Matches `TagNalUnitHeader` / `SNalUnitHeader` in `codec/common/inc/wels_common_basis.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagNalUnitHeader {
    pub uiForbiddenZeroBit: u8,
    pub uiNalRefIdc: u8,
    pub eNalUnitType: EWelsNalUnitType,
}

pub type SNalUnitHeader = TagNalUnitHeader;

/// Extended NAL unit header containing standard AVC and SVC extension parameters.
///
/// Matches `TagNalUnitHeaderExt` / `SNalUnitHeaderExt` in `codec/common/inc/wels_common_basis.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagNalUnitHeaderExt {
    pub sNalUnitHeader: SNalUnitHeader,
    pub bIdrFlag: bool,
    pub uiPriorityId: u8,
    pub bNoInterLayerPredFlag: bool,
    pub uiDependencyId: u8,
    pub uiQualityId: u8,
    pub uiTemporalId: u8,
    pub bUseRefBasePicFlag: bool,
    pub bDiscardableFlag: bool,
    pub bOutputFlag: bool,
    pub uiReservedThree2Bits: u8,
    pub uiLayerDqId: u8,
}

pub type SNalUnitHeaderExt = TagNalUnitHeaderExt;

/// Video Coding Layer (VCL) slice payload representation.
///
/// The payload's identity is [`sSliceBitsRead`](Self::sSliceBitsRead)'s `start`
/// offset into the decoder's `sRawData` since T3.3 — the **RBSP**, which is what the
/// slice reader wants. [`iNalPos`](Self::iNalPos) is the other one: the offset into
/// `sSavedData` of this NAL's **EBSP**, which is what parse-only hands out.
///
/// `pNalPos: *mut u8` was deleted dead at T3.3 (S18), correctly for that tree —
/// nothing wrote it, because the upstream parse-only output path that fills it had
/// not been carried. T8b.B2 carries that path, so the field comes back as an offset
/// rather than a pointer, and `iNalLength` stops being a perpetual 0.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SVclNal {
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub sSliceBitsRead: BsReader,
    /// `pNalPos` (`nalu.h:54`) as an offset into `sSavedData`, written by
    /// [`ParseNalHeader`]'s parse-only arm and read by `DecodeFrameConstruction`'s.
    /// Meaningless outside parse-only mode, where nothing writes `sSavedData`.
    pub iNalPos: usize,
    pub iNalLength: i32,
    pub bSliceHeaderExtFlag: bool,
}

/// Prefix NAL unit syntax elements (NAL type 14).
///
/// Matches `TagPrefixNalUnit` / `SPrefixNalUnit` in `codec/decoder/core/inc/nal_prefix.h`.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct TagPrefixNalUnit {
    pub sRefPicBaseMarking: SRefBasePicMarking,
    pub bStoreRefBasePicFlag: bool,
    pub bPrefixNalUnitAdditionalExtFlag: bool,
    pub bPrefixNalUnitExtFlag: bool,
    pub bPrefixNalCorrectFlag: bool,
}

pub type SPrefixNalUnit = TagPrefixNalUnit;

/// The payload inside [`SNalUnit`] — the C++'s discriminated union, **as a struct**
/// (T5b.3).
///
/// The C declares `union { SVclNal sVclNal; SPrefixNalUnit sPrefixNal; }` and the NAL
/// type is the discriminant, carried out of band in `sNalHeaderExt`. That is the whole
/// of what made reading a node `unsafe`: a union field read is UB unless the arm is
/// the one last written, and no type in the port could say which.
///
/// **Both arms live side by side now**, which costs `size_of::<SPrefixNalUnit>()` — a
/// small-field struct, against `SVclNal`'s slice-header extension — per node, at
/// thirty-two nodes per access unit. Nothing else changes: the paths that write the
/// prefix arm and the paths that write the VCL arm are the same paths, selected by
/// the same `eNalUnitType`, and no site reads one after writing the other. What is
/// bought is that *reading a node is safe*, which is what the access unit's slots
/// needed in order to own (`TagAccessUnits::nal_units`).
///
/// An `enum` would say more, and it is deliberately not that: the two arms are
/// written field-by-field by the parsers, several statements apart, so a sum type
/// would need a builder at every site. This is the change that removes the
/// unsoundness; tightening it further is a design question, not this face's.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SNalData {
    pub sVclNal: SVclNal,
    pub sPrefixNal: SPrefixNalUnit,
}

/// In-memory representation of an individual NAL Unit.
///
/// Matches `TagNalUnit` / `SNalUnit` in `codec/decoder/core/inc/nalu.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TagNalUnit {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub sNalData: SNalData,
    pub uiTimeStamp: u64,
}

pub type SNalUnit = TagNalUnit;

// **T5b.8: `SNalUnit::memset_zero` is gone, because [`Default`] became the C's
// memset.** T5b.6 introduced it for one field — the zeroed image and `Default`'s
// differ in exactly **one** of 6,056 bytes, `sSliceHeader.sps_ref`'s niche byte,
// where `Option<SpsRef>`'s niche in a `bool` makes all-zero read back as
// `Some(SpsRef { id: 0, subset: false })` (F54's class). F56 ruled that `Some` a
// layout artifact rather than a transcription: the C zeroes a `pSps` pointer here
// and `None` is that null. With the overwrite dropped the two constructors
// coincide, and a `memset_zero` beside a `Default` that *is* its zero would state
// the opposite of what is true — so `MemGetNextNal` spells `SNalUnit::default()`.

impl Default for TagNalUnit {
    fn default() -> Self {
        Self {
            sNalHeaderExt: SNalUnitHeaderExt::default(),
            sNalData: SNalData::default(),
            uiTimeStamp: 0,
        }
    }
}

/// Container structure for an entire Access Unit (AU).
///
/// Matches `TagAccessUnits` / `SAccessUnit` in `codec/decoder/core/inc/nalu.h`.
///
/// **Not `#[repr(C)]` and not `Copy` since T5.O4**: `nal_units` is a `Vec` with no C
/// layout, the struct crosses no FFI boundary, and it carries no `assert_size!` or
/// offset pin — T5.N4's reasoning for `SDeblockingFilter`, unchanged.
pub struct TagAccessUnits {
    /// The NAL nodes — replacing `pNalUnitsList: *mut *mut SNalUnit` and the
    /// `uiCountUnitsNum` that was supposed to describe it (T5.O4, F39).
    ///
    /// The count and the contents are one fact now, which is the same move T5.N1 made
    /// for the picture pool and for the same reason: the port had **two** allocators
    /// for this list, of different shapes, and a growth path that mixed them.
    ///
    /// **The slots own** (T5b.3), and what made that possible is that the last stored
    /// alias into a node is gone.
    ///
    /// The paragraph that stood here explained why they could not: *"handing out a
    /// node pointer means `&mut *the_box`, whose Unique retag covers the whole node,
    /// and `ParseSliceHeaderSyntaxs` is already holding a `&mut BsCursor` into that
    /// same node"* — a Miri verdict, and the reason the design copied `PicPool`'s
    /// pre-T5.Q1 shape. Both halves of it have been removed rather than argued away:
    /// `pCtx->pNalCur` is an **index** since T5b.3, and the `&mut BsCursor` the slice
    /// parser holds is derived from *this* container's own borrow through
    /// [`node_mut`](Self::node_mut), so it is one borrow chain instead of two
    /// derivations. `PicPool`'s slots took exactly this step at T5.Q1, for exactly
    /// this reason.
    ///
    /// One node per `Box`, stable across a growth — the C++ moves the nodes
    /// (`ExpandNalUnitList` copies into a new block and frees the old), which dangles
    /// every outstanding `SNalUnit*`; P5's hazard in its original habitat, and a
    /// `Vec<Box<_>>` makes it unrepresentable rather than repaired.
    pub nal_units: Vec<Box<SNalUnit>>,
    pub uiAvailUnitsNum: u32,
    pub uiActualUnitsNum: u32,
    pub uiStartPos: u32,
    pub uiEndPos: u32,
    pub bCompletedAuFlag: bool,
}

pub type SAccessUnit = TagAccessUnits;
// `PAccessUnit = *mut SAccessUnit` is deleted, not deprecated: T5.P1 left it with no
// referent. The context owns the access unit, every consumer reaches it through
// `cur_au`, and a spare alias for the pointer type is how the next hoist gets
// written.

impl Default for TagAccessUnits {
    fn default() -> Self {
        Self {
            nal_units: Vec::new(),
            uiAvailUnitsNum: 0,
            uiActualUnitsNum: 0,
            uiStartPos: 0,
            uiEndPos: 0,
            bCompletedAuFlag: false,
        }
    }
}

impl TagAccessUnits {
    /// An access unit with `count` zeroed NAL nodes — the constructor both of the
    /// port's two `MemInitNalList`s used to be, and since T5.P1 the only one.
    ///
    /// F19: every node here is dropped by this struct's own drop glue, and the struct
    /// by the context's — `SWelsDecoderContext::access_unit` owns it. There is no size
    /// to recompute at the free, which is what the deleted pair got wrong in opposite
    /// directions.
    pub fn with_nodes(count: usize) -> Box<Self> {
        let mut au = Box::new(Self::default());
        au.nal_units.reserve_exact(count);
        for _ in 0..count {
            au.nal_units.push(Box::new(SNalUnit::default()));
        }
        au
    }

    /// Number of nodes — the old `uiCountUnitsNum`, which is now a derived value and
    /// so cannot disagree with the list.
    #[inline]
    pub fn count(&self) -> u32 {
        self.nal_units.len() as u32
    }

    /// The pointer to node `i`.
    ///
    /// Node `i`, mutably — the C's `pNalUnitsList[i]`.
    ///
    /// **T5b.3: a borrow, from the container's own borrow.** This was
    /// `nal(&self) -> *mut SNalUnit`, a *copy of a stored pointer* rather than a
    /// derivation, and the whole design above existed to keep it that way. With
    /// `pCtx->pNalCur` an index there is no second path to a node, so one `&mut` at a
    /// time is all any consumer wants — the slice-header parser's `&mut BsCursor` is
    /// reborrowed *out of this*, which is one chain rather than two.
    ///
    /// # Panics
    /// If `i` is out of range. The C indexed unchecked and the port paired every index
    /// with a hand-written `i < MAX_NAL_UNIT_NUM_IN_AU` test; this is that check, in
    /// one place (P13).
    #[inline]
    pub fn nal(&mut self, i: usize) -> &mut SNalUnit {
        let len = self.nal_units.len();
        assert!(i < len, "NAL unit {i} outside an access unit of {len}");
        &mut self.nal_units[i]
    }

    /// [`nal`](Self::nal)'s shared form, or `None` past the end of the list.
    ///
    /// T5.AC5 introduced this as the safe reader while the slots were raw; it stays
    /// because most consumers only look at a NAL header, and a shared borrow lets two
    /// of them coexist.
    #[inline]
    pub fn node(&self, i: usize) -> Option<&SNalUnit> {
        self.nal_units.get(i).map(|n| &**n)
    }

    /// [`node`](Self::node)'s mutable form.
    #[inline]
    pub fn node_mut(&mut self, i: usize) -> Option<&mut SNalUnit> {
        self.nal_units.get_mut(i).map(|n| &mut **n)
    }
}

// **`Drop` is deleted, not converted** (T5b.3). It existed to `Box::from_raw` every
// slot — F19's answer for the nodes, hand-written because the slots were raw. Owned
// slots make the same answer the compiler's drop glue, and R4's equivalence argument
// ("the port frees exactly what the C++ frees") holds by construction rather than by
// inspection: there is no spelling in which a slot can be dropped without its node.

// ============================================================================
// Lookup Tables
// ============================================================================

/// Global level limits table for H.264 validation.
///
/// Transcribed field-for-field from `g_ksLevelLimits` in
/// `codec/common/src/common_tables.cpp:345`. Note that openh264 does **not** use the
/// H.264 spec's Table A-1 units for three of these columns: `uiMaxDPBMbs` is
/// `MaxDpbMbs` (macroblocks), not the spec's `MaxDPB` in units of 1024 bytes;
/// `iMinVmv`/`iMaxVmv` are `MaxVmvR` in quarter-pel units, not luma samples; and
/// `iMaxMvsPer2Mb` is `0x7fff` (i.e. unlimited) below level 3.0 rather than a
/// sentinel. Taking the spec's columns instead — which an earlier revision of this
/// file did — makes both the decoder MV-range check and the encoder's
/// `WelsCheckRefFrameLimitationLevelIdcFirst` reject conforming input.
pub const g_ksLevelLimits: [SLevelLimits; 17] = [
    SLevelLimits { uiLevelIdc: 10, uiMaxMBPS: 1485, uiMaxFS: 99, uiMaxDPBMbs: 396, uiMaxBR: 64, uiMaxCPB: 175, iMinVmv: -256, iMaxVmv: 255, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 9, uiMaxMBPS: 1485, uiMaxFS: 99, uiMaxDPBMbs: 396, uiMaxBR: 128, uiMaxCPB: 350, iMinVmv: -256, iMaxVmv: 255, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 11, uiMaxMBPS: 3000, uiMaxFS: 396, uiMaxDPBMbs: 900, uiMaxBR: 192, uiMaxCPB: 500, iMinVmv: -512, iMaxVmv: 511, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 12, uiMaxMBPS: 6000, uiMaxFS: 396, uiMaxDPBMbs: 2376, uiMaxBR: 384, uiMaxCPB: 1000, iMinVmv: -512, iMaxVmv: 511, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 13, uiMaxMBPS: 11880, uiMaxFS: 396, uiMaxDPBMbs: 2376, uiMaxBR: 768, uiMaxCPB: 2000, iMinVmv: -512, iMaxVmv: 511, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 20, uiMaxMBPS: 11880, uiMaxFS: 396, uiMaxDPBMbs: 2376, uiMaxBR: 2000, uiMaxCPB: 2000, iMinVmv: -512, iMaxVmv: 511, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 21, uiMaxMBPS: 19800, uiMaxFS: 792, uiMaxDPBMbs: 4752, uiMaxBR: 4000, uiMaxCPB: 4000, iMinVmv: -1024, iMaxVmv: 1023, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 22, uiMaxMBPS: 20250, uiMaxFS: 1620, uiMaxDPBMbs: 8100, uiMaxBR: 4000, uiMaxCPB: 4000, iMinVmv: -1024, iMaxVmv: 1023, uiMinCR: 2, iMaxMvsPer2Mb: 0x7fff },
    SLevelLimits { uiLevelIdc: 30, uiMaxMBPS: 40500, uiMaxFS: 1620, uiMaxDPBMbs: 8100, uiMaxBR: 10000, uiMaxCPB: 10000, iMinVmv: -1024, iMaxVmv: 1023, uiMinCR: 2, iMaxMvsPer2Mb: 32 },
    SLevelLimits { uiLevelIdc: 31, uiMaxMBPS: 108000, uiMaxFS: 3600, uiMaxDPBMbs: 18000, uiMaxBR: 14000, uiMaxCPB: 14000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 4, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 32, uiMaxMBPS: 216000, uiMaxFS: 5120, uiMaxDPBMbs: 20480, uiMaxBR: 20000, uiMaxCPB: 20000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 4, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 40, uiMaxMBPS: 245760, uiMaxFS: 8192, uiMaxDPBMbs: 32768, uiMaxBR: 20000, uiMaxCPB: 25000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 4, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 41, uiMaxMBPS: 245760, uiMaxFS: 8192, uiMaxDPBMbs: 32768, uiMaxBR: 50000, uiMaxCPB: 62500, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 2, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 42, uiMaxMBPS: 522240, uiMaxFS: 8704, uiMaxDPBMbs: 34816, uiMaxBR: 50000, uiMaxCPB: 62500, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 2, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 50, uiMaxMBPS: 589824, uiMaxFS: 22080, uiMaxDPBMbs: 110400, uiMaxBR: 135000, uiMaxCPB: 135000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 2, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 51, uiMaxMBPS: 983040, uiMaxFS: 36864, uiMaxDPBMbs: 184320, uiMaxBR: 240000, uiMaxCPB: 240000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 2, iMaxMvsPer2Mb: 16 },
    SLevelLimits { uiLevelIdc: 52, uiMaxMBPS: 2073600, uiMaxFS: 36864, uiMaxDPBMbs: 184320, uiMaxBR: 240000, uiMaxCPB: 240000, iMinVmv: -2048, iMaxVmv: 2047, uiMinCR: 2, iMaxMvsPer2Mb: 16 },
];

/// Default dequantization scaling list matrix for 4x4 blocks.
pub const g_kuiDequantScaling4x4Default: [[u8; 16]; 2] = [
    [6, 13, 20, 28, 13, 20, 28, 32, 20, 28, 32, 37, 28, 32, 37, 42],
    [10, 14, 20, 24, 14, 20, 24, 27, 20, 24, 27, 30, 24, 27, 30, 34],
];

/// Default dequantization scaling list matrix for 8x8 blocks.
pub const g_kuiDequantScaling8x8Default: [[u8; 64]; 2] = [
    [
        6, 10, 13, 16, 18, 23, 25, 27, 10, 11, 16, 18, 23, 25, 27, 29,
        13, 16, 18, 23, 25, 27, 29, 31, 16, 18, 23, 25, 27, 29, 31, 33,
        18, 23, 25, 27, 29, 31, 33, 36, 23, 25, 27, 29, 31, 33, 36, 38,
        25, 27, 29, 31, 33, 36, 38, 40, 27, 29, 31, 33, 36, 38, 40, 42,
    ],
    [
        9, 13, 15, 17, 19, 21, 22, 24, 13, 13, 17, 19, 21, 22, 24, 25,
        15, 17, 19, 21, 22, 24, 25, 27, 17, 19, 21, 22, 24, 25, 27, 28,
        19, 21, 22, 24, 25, 27, 28, 30, 21, 22, 24, 25, 27, 28, 30, 32,
        22, 24, 25, 27, 28, 30, 32, 33, 24, 25, 27, 28, 30, 32, 33, 35,
    ],
];

// ============================================================================
// Bitstream Parsing & Access Unit Parser Implementation
// ============================================================================

/// Detects the Annex B start code prefix (`0x000001` or `0x00000001`).
///
/// Returns a pointer to the byte immediately following `0x01` (the NAL header byte),
/// or null if no valid start code prefix is found.

/// Equality of two POD parameter-set structs, standing in for the `memcmp`
/// guards in `ParseSps` / `ParsePps` (`au_parser.cpp`).
///
/// **T5.AC9 enumerated this as an exception; T5b.3 retires it instead.** The
/// `unsafe` was a `from_raw_parts` pair reading each struct as `[u8]`, which made
/// *padding* part of the comparison and put "every byte initialized, padding
/// included" on the caller — F31's zeroing and the `MaybeUninit` scratch in
/// `ParseSps` existed to discharge exactly that. The field comparison is what the
/// `memcmp` was for and cannot see padding at all.
fn bytes_equal<T: PartialEq>(a: &T, b: &T) -> bool {
    // **T5b.3: structural, not byte-wise.** The C++ `memcmp`s two parameter sets, and
    // the port reproduced that literally — which is why `ParseSps` had to zero the
    // *padding* of its scratch (F31) and why the scratch had to be a `MaybeUninit`
    // shell. Comparing the fields is what the `memcmp` was for, it cannot see padding
    // at all, and the two agree on every value either side can hold: both operands
    // are written through this module's own copy, out of a zero-initialised scratch.
    a == b
}

fn bytes_copy<T: Copy>(dst: &mut T, src: &T) {
    // The `memcpy`'s value form. `T` is a POD parameter set, so the assignment moves
    // the same bytes the C's did — minus the padding, which nothing reads.
    *dst = *src;
}

// `DetectStartCodePrefix` was deleted dead at T3.3 (S18): it had no callers — the
// Annex B scan is `split_annexb_units` (`lib.rs`), and has been since the port's
// `WelsDecodeBs` was written.

/// Decodes the 3-byte SVC NAL Unit Header Extension.
///
/// T3.3: takes the 3-byte window as a slice — the function had **no length
/// parameter at all** in pointer form; it gains one by construction, and every
/// caller passes exactly [`NAL_UNIT_HEADER_EXT_SIZE`] bytes behind its own
/// `iNalSize` guard.
pub fn DecodeNalHeaderExt(pNal: &mut SNalUnit, src: &[u8]) {
    let pHeaderExt = &mut pNal.sNalHeaderExt;

    let mut uiCurByte = src[0];
    pHeaderExt.bIdrFlag = (uiCurByte & 0x40) != 0;
    pHeaderExt.uiPriorityId = uiCurByte & 0x3F;

    uiCurByte = src[1];
    pHeaderExt.bNoInterLayerPredFlag = (uiCurByte >> 7) != 0;
    pHeaderExt.uiDependencyId = (uiCurByte & 0x70) >> 4;
    pHeaderExt.uiQualityId = uiCurByte & 0x0F;

    uiCurByte = src[2];
    pHeaderExt.uiTemporalId = uiCurByte >> 5;
    pHeaderExt.bUseRefBasePicFlag = (uiCurByte & 0x10) != 0;
    pHeaderExt.bDiscardableFlag = (uiCurByte & 0x08) != 0;
    pHeaderExt.bOutputFlag = (uiCurByte & 0x04) != 0;
    pHeaderExt.uiReservedThree2Bits = uiCurByte & 0x03;
    pHeaderExt.uiLayerDqId = (pHeaderExt.uiDependencyId << 4) | pHeaderExt.uiQualityId;
}

/// The RBSP's size in bits: `(len << 3) - trailing_bits(last byte)`, and **zero for
/// an empty payload** — the fix for [`phase3_findings.md`](../../../docs/phase3_findings.md)
/// §**F15**.
///
/// The three `BsGetTrailingBits(pNal + iNalSize - 1)` sites this replaces computed
/// the index by subtraction, so `iNalSize == 0` gave `pNal + (0 - 1)`: a debug
/// panic — which, unwinding out of an `extern "C"` thunk, **aborted the process** —
/// and in release an out-of-bounds pointer that happened to land on the preceding
/// header byte. That is not exotic input: `ParseNalHeader` strips trailing zero
/// bytes and then consumes the header byte, so any slice NAL whose payload is one
/// non-zero byte followed by zeros arrives here with `iNalSize == 0`, and every
/// conformance stream in T3.0's corpus has ~11 truncations that produce it.
///
/// The guard is a **comparison, not a subtraction** (the seam's arithmetic rule):
/// `size >= 1` is tested before any index is formed. A zero bit size then flows into
/// the caller's existing `DecInitBits` failure branch — `(0 + 7) >> 3 == 0` is
/// rejected as `ERR_INFO_INVALID_ACCESS` — so the NAL is refused through the path
/// the code already had, with `dsBitstreamError` and the same access-unit
/// bookkeeping. That is exactly what the **release** build did for a type-1/5 NAL
/// (its out-of-bounds read landed on an odd header byte, giving 0 trailing bits and
/// a bit size of 0); the fix makes that outcome the *defined* one, for every NAL
/// type and in both profiles, without reading a byte it has no right to read.
///
/// Upstream C++ (`au_parser.cpp:252` and `:396`) shares the expression, so there is
/// no S6 arithmetic parity to preserve here — there is no correct behaviour to be
/// parity *with*.
fn rbsp_bit_size(bytes: &[u8], start: usize, size: i32) -> i32 {
    if size < 1 {
        return 0;
    }
    (size << 3) - crate::safe::bits::trailing_bits(bytes[start + size as usize - 1])
}



/// **Parse-only's SPS cache** (`au_parser.cpp:1173-1190`, T8b.B2) — the escaped SPS
/// NAL, verbatim, with the start code normalised to the four-byte form.
///
/// `sSpsBsInfo` and its two siblings existed on this context from the day it was
/// written and `grep` found nothing but the declarations: this is their first
/// writer. The reader is `DecodeFrameConstruction`'s IDR prepend, which is why the
/// cache exists at all — a parse-only consumer gets the parameter sets in front of
/// every IDR whether or not the source stream repeated them.
fn parse_only_write_sps(pSpsBs: &mut SSpsBsInfo, iSpsId: i32, kpSrcNal: &[u8]) {
    pSpsBs.iSpsId = iSpsId;
    let iActualLen = actual_len_without_trailing_zeros(kpSrcNal);
    let mut uiLen = iActualLen;
    // "unify start code as 0x0001": the caller's window starts `00 00 01`, so the
    // leading zero that makes it `00 00 00 01` is prepended here.
    let iStartDeltaByte =
        usize::from(kpSrcNal.len() >= 3 && kpSrcNal[0] == 0 && kpSrcNal[1] == 0 && kpSrcNal[2] == 1);
    if iStartDeltaByte == 1 {
        pSpsBs.pSpsBsBuf[0] = 0x0;
        uiLen += 1;
    }
    pSpsBs.pSpsBsBuf[iStartDeltaByte..iStartDeltaByte + iActualLen]
        .copy_from_slice(&kpSrcNal[..iActualLen]);
    pSpsBs.uiSpsBsLen = uiLen as u16;
}

/// **Parse-only's subset-SPS rewrite** (`au_parser.cpp:1191-1256`, T8b.B2) — a
/// subset SPS re-encoded as a *plain* Main-profile SPS, because what parse-only hands
/// out is an AVC bitstream and a plain decoder cannot read NAL type 15.
///
/// It pairs with the slice-extension arm of [`parse_only_capture_vcl`], which strips
/// the SVC header off the slices; between them an SVC access unit comes out as AVC.
/// The reference forces `profile_idc` to 77 and drops the VUI, the scaling lists and
/// the SVC extension; only the syntax elements written below survive.
///
/// **One bound the reference does not have.** It sizes the RBSP writer with
/// `pBs->pEndBuf - pBs->pStartBuf` — the length of the *source* SPS's bitstream —
/// while the buffer it hands the writer is `SPS_PPS_BS_SIZE + 4` bytes, so a source
/// SPS longer than 132 bytes lets the writer run off the allocation. Here the writer
/// is bounded by the buffer it writes into, and a rewrite that does not fit is
/// refused (`false`) rather than truncated. See F92.
///
/// **Ruled: D-fid-6 (the user, 2026-08-27, session J).** All three port behaviors
/// stand — the bounded writer, the refusal, and the escaped length (the reference
/// stores the pre-escape RBSP size, `au_parser.cpp:1252`, truncating one byte per
/// inserted `0x03`). The previously-unreached escape arm now has a synthetic
/// referee: `subset_sps_rewrite_reports_the_escaped_length_when_an_escape_is_needed`.
fn parse_only_write_subset_sps(pSpsBs: &mut SSpsBsInfo, pSps: &SSps) -> bool {
    use crate::encoder::vlc_encoder::{
        BsRbspTrailingBits, BsWriteBits, BsWriteOneBit, BsWriteSE, BsWriteUE,
    };
    pSpsBs.iSpsId = pSps.iSpsId;
    pSpsBs.pSpsBsBuf[0] = 0x00;
    pSpsBs.pSpsBsBuf[1] = 0x00;
    pSpsBs.pSpsBsBuf[2] = 0x00;
    pSpsBs.pSpsBsBuf[3] = 0x01;
    pSpsBs.pSpsBsBuf[4] = 0x67; // nal_ref_idc 3, nal_unit_type 7 (SPS)

    // `WelsMallocz (SPS_PPS_BS_SIZE + 4, "Temp buffer for parse only usage.")` — the
    // four bytes are the writer's flush headroom, and it is a stack array here
    // because its lifetime is this function.
    let mut rbsp = [0u8; SPS_PPS_BS_SIZE + 4];
    let mut bs = crate::safe::bits::BsWriter::new();
    let buf = &mut rbsp[..];

    BsWriteBits(buf, &mut bs, 8, 77); // profile_idc, forced to Main
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bConstraintSet0Flag));
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bConstraintSet1Flag));
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bConstraintSet2Flag));
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bConstraintSet3Flag));
    BsWriteBits(buf, &mut bs, 4, 0); // constraint_set4/5, reserved_zero_2bits
    BsWriteBits(buf, &mut bs, 8, pSps.uiLevelIdc as u32);
    BsWriteUE(buf, &mut bs, pSps.iSpsId as u32);
    BsWriteUE(buf, &mut bs, pSps.uiLog2MaxFrameNum.wrapping_sub(4));
    BsWriteUE(buf, &mut bs, pSps.uiPocType);
    if pSps.uiPocType == 0 {
        BsWriteUE(buf, &mut bs, (pSps.iLog2MaxPocLsb - 4) as u32);
    } else if pSps.uiPocType == 1 {
        BsWriteOneBit(buf, &mut bs, u32::from(pSps.bDeltaPicOrderAlwaysZeroFlag));
        BsWriteSE(buf, &mut bs, pSps.iOffsetForNonRefPic);
        BsWriteSE(buf, &mut bs, pSps.iOffsetForTopToBottomField);
        BsWriteUE(buf, &mut bs, pSps.iNumRefFramesInPocCycle as u32);
        for i in 0..pSps.iNumRefFramesInPocCycle as usize {
            BsWriteSE(buf, &mut bs, pSps.iOffsetForRefFrame[i] as i32);
        }
    }
    BsWriteUE(buf, &mut bs, pSps.iNumRefFrames as u32);
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bGapsInFrameNumValueAllowedFlag));
    // `int32_t - 1` in the C, on a value the SPS parse has already refused to leave
    // at zero; `wrapping_sub` is the same bit pattern for the same input.
    BsWriteUE(buf, &mut bs, pSps.iMbWidth.wrapping_sub(1));
    BsWriteUE(buf, &mut bs, pSps.iMbHeight.wrapping_sub(1));
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bFrameMbsOnlyFlag));
    if !pSps.bFrameMbsOnlyFlag {
        BsWriteOneBit(buf, &mut bs, u32::from(pSps.bMbaffFlag));
    }
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bDirect8x8InferenceFlag));
    BsWriteOneBit(buf, &mut bs, u32::from(pSps.bFrameCroppingFlag));
    if pSps.bFrameCroppingFlag {
        BsWriteUE(buf, &mut bs, pSps.sFrameCrop.iLeftOffset as u32);
        BsWriteUE(buf, &mut bs, pSps.sFrameCrop.iRightOffset as u32);
        BsWriteUE(buf, &mut bs, pSps.sFrameCrop.iTopOffset as u32);
        BsWriteUE(buf, &mut bs, pSps.sFrameCrop.iBottomOffset as u32);
    }
    BsWriteOneBit(buf, &mut bs, 0); // vui_parameters_present_flag
    BsRbspTrailingBits(buf, &mut bs);

    let iRbspSize = bs.pos();
    let dst = &mut pSpsBs.pSpsBsBuf[5..];
    let written = crate::decoder::bit_stream::rbsp_to_ebsp(&rbsp[..iRbspSize], dst);
    // `rbsp_to_ebsp` stops at the end of its destination, so a full destination is
    // indistinguishable from a truncated one and both are refused. The reference has
    // no check here at all.
    if written >= dst.len() {
        return false;
    }
    // The reference stores `pCurBuf - pStartBuf + 5` — the **RBSP** size, not the
    // escaped one. It is a defect the port does not inherit: `uiSpsBsLen` is what
    // `DecodeFrameConstruction` copies out, so an escaped subset SPS would be handed
    // to the caller one byte short per inserted `0x03`. See F92.
    pSpsBs.uiSpsBsLen = (written + 5) as u16;
    true
}

/// **Parse-only's PPS cache** (`au_parser.cpp:1478-1493`, T8b.B2). The plain SPS
/// arm's twin; a PPS needs no rewriting, because its syntax is the same in AVC and
/// SVC.
fn parse_only_write_pps(pPpsBs: &mut SPpsBsInfo, uiPpsId: i32, kpSrcNal: &[u8]) {
    pPpsBs.iPpsId = uiPpsId;
    let iActualLen = actual_len_without_trailing_zeros(kpSrcNal);
    let mut uiLen = iActualLen;
    let iStartDeltaByte =
        usize::from(kpSrcNal.len() >= 3 && kpSrcNal[0] == 0 && kpSrcNal[1] == 0 && kpSrcNal[2] == 1);
    if iStartDeltaByte == 1 {
        pPpsBs.pPpsBsBuf[0] = 0x0;
        uiLen += 1;
    }
    pPpsBs.pPpsBsBuf[iStartDeltaByte..iStartDeltaByte + iActualLen]
        .copy_from_slice(&kpSrcNal[..iActualLen]);
    pPpsBs.uiPpsBsLen = uiLen as u16;
}

/// The reference's "remove final trailing 0 bytes" loop (`au_parser.cpp:325-328`,
/// `:361-364`, `:1176-1179`, `:1482-1485` — four copies, one behaviour), bounded.
///
/// The C walks backwards from the last byte with no floor, so an all-zero NAL reads
/// off the front of the caller's buffer. The bound is the only difference; on every
/// input the C survives, the answer is the same.
fn actual_len_without_trailing_zeros(src: &[u8]) -> usize {
    let mut n = src.len();
    while n > 0 && src[n - 1] == 0 {
        n -= 1;
    }
    n
}

/// **Parse-only's VCL capture** (T8b.B2) — `au_parser.cpp:324-357` (the
/// slice-extension arm) and `:359-382` (the plain arm), which had no counterpart in
/// this port: `pNalPos` was deleted dead at T3.3 precisely because this is what
/// would have written it.
///
/// What it produces is the NAL the *caller* gets back: escaped bytes, a four-byte
/// start code, and — for a slice-extension NAL — the SVC three-byte header removed
/// and the NAL type rewritten to 1 or 5, so an SVC slice comes out as the AVC slice
/// a plain decoder can read. Returns `(offset into sSavedData, iNalLength)`.
///
/// **One deliberate divergence, and it is not a behaviour change.** The reference
/// rewrites the NAL type byte **inside the caller's input buffer**
/// (`*(pSrcNal + iCurrStartByte) &= 0xE0` at `:346-351`, through a `const_cast` the
/// decoder made at `decoder.cpp:766`) and then copies that byte out. The port
/// computes the same byte and copies it; the caller's bitstream is left as it was
/// handed in. Nothing downstream reads those bytes again — `sRawData` already holds
/// the de-escaped copy this NAL will be decoded from — so the only thing the C's
/// version changes is the application's buffer. See F91.
///
/// **Ruled: D-fid-5 (the user, 2026-08-27, session J).** The port never mutates
/// caller-owned memory; outputs stay byte-identical (the `sps_subsetsps_bothVUI`
/// golden referees this arm's output on every `cargo test`). A caller feeding the
/// same buffer twice gets identical results where upstream gives different ones.
fn parse_only_capture_vcl(
    saved: &mut crate::decoder::bit_stream::RawDataBuffer,
    kpSrcNal: &[u8],
    bExtensionFlag: bool,
    bIdrFlag: bool,
) -> Option<(usize, i32)> {
    let iActualLen = actual_len_without_trailing_zeros(kpSrcNal);

    if bExtensionFlag {
        // `iCurrStartByte` indexes the NAL header byte: 4 behind `00 00 00 01`, 3
        // behind `00 00 01`. The caller normalises to the three-byte form, so the C's
        // `if` is always taken; it is written out because the C writes it out.
        let iCurrStartByte =
            if kpSrcNal.len() >= 3 && kpSrcNal[0] == 0 && kpSrcNal[1] == 0 && kpSrcNal[2] == 1 {
                3usize
            } else {
                4usize
            };
        let iOffset = iCurrStartByte + 1 + NAL_UNIT_HEADER_EXT_SIZE;
        if iActualLen <= iOffset {
            // Degenerate NAL: the C indexes past `iActualLen` here and copies a
            // negative length. There is nothing to capture.
            return None;
        }
        let mut iNalLength = (iActualLen - NAL_UNIT_HEADER_EXT_SIZE) as i32;
        if iCurrStartByte == 3 {
            iNalLength += 1;
        }
        // `:346-351` — the AVC-ification, computed rather than written back.
        let hdr = (kpSrcNal[iCurrStartByte] & 0xE0) | if bIdrFlag { 0x05 } else { 0x01 };
        let iWriteLen = 5 + (iActualLen - iOffset);
        if !saved.wrap_for(iWriteLen) {
            return None;
        }
        let pos = saved.append_raw(&[0x00, 0x00, 0x00, 0x01, hdr]);
        saved.append_raw(&kpSrcNal[iOffset..iActualLen]);
        Some((pos, iNalLength))
    } else {
        let iStartDeltaByte =
            usize::from(kpSrcNal.len() >= 3 && kpSrcNal[0] == 0 && kpSrcNal[1] == 0 && kpSrcNal[2] == 1);
        if iActualLen == 0 {
            return None;
        }
        let iWriteLen = iStartDeltaByte + iActualLen;
        if !saved.wrap_for(iWriteLen) {
            return None;
        }
        let iNalLength = (iActualLen + iStartDeltaByte) as i32;
        let pos = saved.cur();
        if iStartDeltaByte == 1 {
            saved.append_raw(&[0x00]);
        }
        saved.append_raw(&kpSrcNal[..iActualLen]);
        Some((pos, iNalLength))
    }
}

/// Parses the NAL unit header byte, checks parameter set existence, and routes
/// the NAL unit to the appropriate syntactic decoder.
///
/// T3.3: the payload's identity is an **offset into `sRawData`** (`kiRbspStart`,
/// minted by `RawDataBuffer::append_ebsp_stripped`), and the return is the offset
/// past the consumed headers — `Some(offset)` where the C returned an advanced
/// pointer, `None` where it returned null. Every read below is an index into the
/// owning buffer.
///
/// `kpSrcNal` is the reference's `kpSrcNal`/`kSrcNalLen` pair (`au_parser.cpp:265`):
/// this NAL's **escaped** bytes, start code included and normalised to the three-byte
/// form the C's caller hands it (`pSrcNal - 3`, `decoder.cpp:815`). It is read only
/// by the parse-only capture below — the RBSP in `sRawData` is what everything else
/// reads — and it is a separate borrow from `pCtx` because it is the caller's input
/// buffer, not the decoder's copy. T8b.B2 added it: the parameter had been dropped
/// at T3.3 along with the capture it exists for.
pub fn ParseNalHeader(
    pCtx: &mut SWelsDecoderContext,
    pNalUnitHeader: &mut SNalUnitHeader,
    kiRbspStart: usize,
    iSrcRbspLen: i32,
    kpSrcNal: &[u8],
    pConsumedBytes: &mut i32,
) -> Option<usize> {
    let mut iNal = kiRbspStart;
    let mut iNalSize = iSrcRbspLen;

    pNalUnitHeader.eNalUnitType = EWelsNalUnitType::NAL_UNIT_UNSPEC_0;

    // Remove consecutive ZERO bytes at the end of current NAL in reverse order
    let mut iIndex = iSrcRbspLen - 1;
    while iIndex >= 0 {
        if pCtx.sRawData.bytes()[kiRbspStart + iIndex as usize] == 0 {
            iNalSize -= 1;
            *pConsumedBytes += 1;
            iIndex -= 1;
        } else {
            break;
        }
    }

    pNalUnitHeader.uiForbiddenZeroBit = pCtx.sRawData.bytes()[iNal] >> 7;
    if pNalUnitHeader.uiForbiddenZeroBit != 0 {
        (*pCtx).iErrorCode |= dsBitstreamError;
        return None;
    }

    pNalUnitHeader.uiNalRefIdc = (pCtx.sRawData.bytes()[iNal] >> 5) & 0x03;
    pNalUnitHeader.eNalUnitType = match pCtx.sRawData.bytes()[iNal] & 0x1F {
        0 => EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
        1 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE,
        2 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPA,
        3 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPB,
        4 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPC,
        5 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR,
        6 => EWelsNalUnitType::NAL_UNIT_SEI,
        7 => EWelsNalUnitType::NAL_UNIT_SPS,
        8 => EWelsNalUnitType::NAL_UNIT_PPS,
        9 => EWelsNalUnitType::NAL_UNIT_AU_DELIMITER,
        10 => EWelsNalUnitType::NAL_UNIT_END_OF_SEQ,
        11 => EWelsNalUnitType::NAL_UNIT_END_OF_STR,
        12 => EWelsNalUnitType::NAL_UNIT_FILER_DATA,
        13 => EWelsNalUnitType::NAL_UNIT_SPS_EXT,
        14 => EWelsNalUnitType::NAL_UNIT_PREFIX,
        15 => EWelsNalUnitType::NAL_UNIT_SUBSET_SPS,
        19 => EWelsNalUnitType::NAL_UNIT_AUX_CODED_SLICE,
        20 => EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT,
        other => EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
    };
    (*pCtx).sCurNalHead = *pNalUnitHeader;

    iNal += 1;
    iNalSize -= 1;
    *pConsumedBytes += 1;

    let eType = pNalUnitHeader.eNalUnitType;

    if !(IS_SEI_NAL(eType)
        || IS_SPS_NAL(eType)
        || IS_AU_DELIMITER_NAL(eType)
        || (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag)
    {
        {
            let stat = &mut (*pCtx).pDecoderStatistics;
            stat.iSpsNoExistNalNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return None;
    }

    if !(IS_SEI_NAL(eType)
        || IS_PARAM_SETS_NALS(eType)
        || IS_AU_DELIMITER_NAL(eType)
        || (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag)
    {
        {
            let stat = &mut (*pCtx).pDecoderStatistics;
            stat.iPpsNoExistNalNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return None;
    }

    if (IS_VCL_NAL_AVC_BASE(eType)
        && !((*pCtx).sSpsPpsCtx.bSpsExistAheadFlag || (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag))
        || (IS_NEW_INTRODUCED_SVC_NAL(eType)
            && !((*pCtx).sSpsPpsCtx.bSpsExistAheadFlag
                || (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag
                || (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag))
    {
        {
            let stat = &mut (*pCtx).pDecoderStatistics;
            stat.iSubSpsNoExistNalNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return None;
    }

    match eType {
        EWelsNalUnitType::NAL_UNIT_AU_DELIMITER | EWelsNalUnitType::NAL_UNIT_SEI => {
            mark_au_ready(pCtx);
        }

        EWelsNalUnitType::NAL_UNIT_PREFIX => {
            // T5b.3: the prefix NAL is a *field* of the context, not a node of the
            // access unit, so it is reached as one — `pCtx.sSpsPpsCtx.sPrefixNal`
            // per statement, which is what the raw local was standing in for.
            macro_rules! pCurNal {
                () => {
                    pCtx.sSpsPpsCtx.sPrefixNal
                };
            }
            pCurNal!().uiTimeStamp = (*pCtx).uiTimeStamp;

            if iNalSize < NAL_UNIT_HEADER_EXT_SIZE as i32 {
                mark_au_ready(pCtx);
                pCurNal!().sNalData.sPrefixNal.bPrefixNalCorrectFlag = false;
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

                        let hdr: [u8; NAL_UNIT_HEADER_EXT_SIZE] = pCtx.sRawData.bytes()
                [iNal..iNal + NAL_UNIT_HEADER_EXT_SIZE]
                .try_into()
                .unwrap();
            DecodeNalHeaderExt(&mut pCurNal!(), &hdr);
            if pCurNal!().sNalHeaderExt.uiQualityId != 0
                || pCurNal!().sNalHeaderExt.bUseRefBasePicFlag
            {
                mark_au_ready(pCtx);
                pCurNal!().sNalData.sPrefixNal.bPrefixNalCorrectFlag = false;
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            iNal += NAL_UNIT_HEADER_EXT_SIZE;
            iNalSize -= NAL_UNIT_HEADER_EXT_SIZE as i32;
            *pConsumedBytes += NAL_UNIT_HEADER_EXT_SIZE as i32;

            pCurNal!().sNalHeaderExt.sNalUnitHeader.uiForbiddenZeroBit = pNalUnitHeader.uiForbiddenZeroBit;
            pCurNal!().sNalHeaderExt.sNalUnitHeader.uiNalRefIdc = pNalUnitHeader.uiNalRefIdc;
            pCurNal!().sNalHeaderExt.sNalUnitHeader.eNalUnitType = pNalUnitHeader.eNalUnitType;

            if pNalUnitHeader.uiNalRefIdc != 0 {
                let iBitSize = rbsp_bit_size(pCtx.sRawData.bytes(), iNal, iNalSize);
                let iErr = DecInitBits(&mut (*pCtx).sBs, &(*pCtx).sRawData, iNal, iBitSize);
                if iErr != ERR_NONE {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }
                // The cursor travels as a value and is written back: `sBs` and
                // `sRawData` are two fields of the context the parse takes whole
                // (T5.Z4).
                let (start, mut cursor) = ((*pCtx).sBs.start, (*pCtx).sBs.cursor);
                ParsePrefixNalUnit(pCtx, start, &mut cursor);
                (*pCtx).sBs.cursor = cursor;
            }
            pCurNal!().sNalData.sPrefixNal.bPrefixNalCorrectFlag = true;
        }

        // `case NAL_UNIT_CODED_SLICE_EXT: bExtensionFlag = true;` falls through into
        // the shared slice body in C, so all three NAL types run the same code and an
        // SVC slice-extension NAL reaches ParseSliceHeaderSyntaxs with the flag set.
        // Splitting this into separate match arms leaves type-20 slices unparsed.
        EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
        | EWelsNalUnitType::NAL_UNIT_CODED_SLICE
        | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR => {
            let bExtensionFlag = eType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

            // **T5b.3: the node is an index from here down.** Every write below
            // re-acquires it, and no borrow crosses a call that re-enters the context.
            let Some(cur_idx) = cur_au(&mut pCtx.access_unit).and_then(MemGetNextNal) else {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return None;
            };
            let uiTimeStamp = (*pCtx).uiTimeStamp;
            if let Some(nal) = cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx)) {
                nal.uiTimeStamp = uiTimeStamp;
                nal.sNalHeaderExt.sNalUnitHeader.uiForbiddenZeroBit = pNalUnitHeader.uiForbiddenZeroBit;
                nal.sNalHeaderExt.sNalUnitHeader.uiNalRefIdc = pNalUnitHeader.uiNalRefIdc;
                nal.sNalHeaderExt.sNalUnitHeader.eNalUnitType = pNalUnitHeader.eNalUnitType;
            }

            // The count is a scalar copy, not a borrow: it is the one thing this branch
            // needs to carry across `ParseSliceHeaderSyntaxs`, which derives the access
            // unit itself. `pCurNal` survives too, because a node is its own allocation
            // (T5.O4) — that is what makes an owning `Box` legal at this level at all.
            let uiAvailNalNum = match cur_au(&mut pCtx.access_unit) {
                Some(au) => au.uiAvailUnitsNum,
                None => 0,
            };

            if bExtensionFlag {
                if iNalSize < NAL_UNIT_HEADER_EXT_SIZE as i32 {
                    discard_nal_and_close_au(pCtx, uiAvailNalNum);
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }

                // The header bytes are copied out first: the slice they come from is
                // `sRawData`, a *field* of the context the node also lives in.
                let hdr: [u8; NAL_UNIT_HEADER_EXT_SIZE] = pCtx.sRawData.bytes()
                    [iNal..iNal + NAL_UNIT_HEADER_EXT_SIZE]
                    .try_into()
                    .unwrap();
                let (qid, base_pic) =
                    match cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx)) {
                        Some(nal) => {
                            DecodeNalHeaderExt(nal, &hdr);
                            (nal.sNalHeaderExt.uiQualityId, nal.sNalHeaderExt.bUseRefBasePicFlag)
                        }
                        None => return None,
                    };
                if qid != 0 || base_pic {
                    // MGS not supported.
                    discard_nal_and_close_au(pCtx, uiAvailNalNum);
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }
                iNal += NAL_UNIT_HEADER_EXT_SIZE;
                iNalSize -= NAL_UNIT_HEADER_EXT_SIZE as i32;
                *pConsumedBytes += NAL_UNIT_HEADER_EXT_SIZE as i32;

                // `au_parser.cpp:324-357` — the parse-only capture, T8b.B2. The
                // buffer and the node are two fields of one context, so the write
                // happens first and the node is re-acquired for the two scalars it
                // gets (T5b.3's rule, the same one the header-ext copy above obeys).
                if (*pCtx).pParam.bParseOnly {
                    let captured = parse_only_capture_vcl(
                        &mut pCtx.sSavedData,
                        kpSrcNal,
                        true,
                        cur_au(&mut pCtx.access_unit)
                            .and_then(|au| au.node(cur_idx))
                            .is_some_and(|nal| nal.sNalHeaderExt.bIdrFlag),
                    );
                    if let Some((iNalPos, iNalLength)) = captured {
                        if let Some(nal) =
                            cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx))
                        {
                            nal.sNalData.sVclNal.iNalPos = iNalPos;
                            nal.sNalData.sVclNal.iNalLength = iNalLength;
                        }
                    }
                }
            } else {
                // `au_parser.cpp:359-382` — the plain arm's capture, before the
                // prefix-NAL prefetch, as in the reference.
                if (*pCtx).pParam.bParseOnly {
                    let captured =
                        parse_only_capture_vcl(&mut pCtx.sSavedData, kpSrcNal, false, false);
                    if let Some((iNalPos, iNalLength)) = captured {
                        if let Some(nal) =
                            cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx))
                        {
                            nal.sNalData.sVclNal.iNalPos = iNalPos;
                            nal.sNalData.sVclNal.iNalLength = iNalLength;
                        }
                    }
                }

                if (*pCtx).sSpsPpsCtx.sPrefixNal.sNalHeaderExt.sNalUnitHeader.eNalUnitType
                    == EWelsNalUnitType::NAL_UNIT_PREFIX
                {
                    if (*pCtx).sSpsPpsCtx.sPrefixNal.sNalData.sPrefixNal.bPrefixNalCorrectFlag {
                        // The prefix NAL is copied out first: it is a field of the
                        // context this call takes whole (T5.Z4). `SNalUnit` is plain
                        // data, and the callee only reads the source.
                        let prefix = (*pCtx).sSpsPpsCtx.sPrefixNal;
                        // T5b.3: the destination node is re-acquired by index, and the
                        // source is a *copy* of the context's prefix NAL — so the two
                        // borrows the call needs cannot be the same object.
                        if let Some(dst) =
                            cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx))
                        {
                            prefetch_nal_header_ext(dst, &prefix);
                        }
                    }
                }

                // SHOULD update this flag for AVC if no prefix NAL.
                if let Some(nal) = cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(cur_idx))
                {
                    nal.sNalHeaderExt.bIdrFlag =
                        eType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
                    nal.sNalHeaderExt.bNoInterLayerPredFlag = true;
                }
            }

            // **T5b.3: nothing is held across a call any more.** `p_last_nal` used to be
            // a raw pointer copied out of the list and dereferenced on both sides of
            // `ParseSliceHeaderSyntaxs`, which re-enters the context and therefore the
            // access unit; with owned slots that is a borrow conflict, and S25's fix
            // shape applies — re-acquire at each use, and no borrow outlives one
            // expression. Everything read out of a node here is a scalar.
            let iBitSize = rbsp_bit_size(pCtx.sRawData.bytes(), iNal, iNalSize);
            // `MemGetNextNal` post-increments, so the node it handed back is the last
            // available one — the two indices are one, and this states it once.
            let last = (uiAvailNalNum - 1) as usize;
            debug_assert_eq!(last, cur_idx);
            let iErr = match cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(last)) {
                Some(nal) => {
                    let pBs = &mut nal.sNalData.sVclNal.sSliceBitsRead;
                    crate::decoder::bit_stream::DecInitBits(pBs, &pCtx.sRawData, iNal, iBitSize)
                }
                None => return None,
            };
            if iErr != ERR_NONE {
                discard_nal_and_close_au(pCtx, uiAvailNalNum);
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            // The cursor travels as a value and is written back **into the NAL's own
            // reader**, which is where the slice's bit position lives and where the
            // slice-data parse picks it up (T5.M3, T5.Y2). It is not `pCtx.sBs`: that
            // one is the non-VCL parser's, and writing this back there would leave
            // every slice header re-read from its first bit.
            let Some((start, mut cursor)) = cur_au(&mut pCtx.access_unit)
                .and_then(|au| au.node(last))
                .map(|nal| {
                    let r = &nal.sNalData.sVclNal.sSliceBitsRead;
                    (r.start, r.cursor)
                })
            else {
                return None;
            };
            let iErr = crate::decoder::decoder_core::ParseSliceHeaderSyntaxs(
                pCtx,
                start,
                &mut cursor,
                bExtensionFlag,
            );
            if let Some(nal) = cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(last)) {
                nal.sNalData.sVclNal.sSliceBitsRead.cursor = cursor;
            }
            if iErr != ERR_NONE {
                let bIdr = cur_au(&mut pCtx.access_unit)
                    .and_then(|au| au.node(cur_idx))
                    .is_some_and(|nal| nal.sNalHeaderExt.bIdrFlag);
                if uiAvailNalNum == 1 && bIdr {
                    crate::decoder::decoder_core::ResetActiveSPSForEachLayer(pCtx);
                }
                discard_nal_and_close_au(pCtx, uiAvailNalNum);
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            let last_sps_ref = cur_au(&mut pCtx.access_unit)
                .and_then(|au| au.node(last))
                .and_then(|nal| nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref);
            let p_last_sps = sps_ref_of(&pCtx.sSpsPpsCtx, last_sps_ref);

            // The two predicates read `sSpsPpsCtx` and the nodes, all shared — so the
            // access unit and the parameter-set context are borrowed side by side out of
            // the one context rather than one of them being copied out.
            let new_seq = |pCtx: &SWelsDecoderContext| -> bool {
                match pCtx.access_unit.as_deref().and_then(|au| au.node(cur_idx)) {
                    Some(cur) => CheckNextAuNewSeq(&pCtx.sSpsPpsCtx, cur, p_last_sps),
                    None => false,
                }
            };
            if uiAvailNalNum == 1 && new_seq(pCtx) {
                crate::decoder::decoder_core::ResetActiveSPSForEachLayer(pCtx);
            }
            if uiAvailNalNum > 1 {
                let prev = (uiAvailNalNum - 2) as usize;
                let boundary = match pCtx.access_unit.as_deref() {
                    Some(au) => match (au.node(last), au.node(prev)) {
                        (Some(l), Some(pv)) => {
                            CheckAccessUnitBoundary(&pCtx.sSpsPpsCtx, l, pv, p_last_sps)
                        }
                        _ => return None,
                    },
                    None => return None,
                };
                if boundary {
                    if let Some(au) = cur_au(&mut pCtx.access_unit) {
                        au.uiEndPos = uiAvailNalNum - 2;
                    }
                    (*pCtx).bAuReadyFlag = true;
                    (*pCtx).bNextNewSeqBegin = new_seq(pCtx);
                }
            }
        }

        _ => {}
    }

    Some(iNal)
}

/// Evaluates whether two consecutive VCL NAL units belong to different Access Units.
/// T5.X7: the context was a parameter for one expression — `sps_of(pCtx,
/// pCurSliceHeader.sps_ref)` — so the caller does the lookup and this function is
/// **safe**, which is what the fifteen comparisons below always were.
pub fn CheckAccessUnitBoundaryExt(
    kpSps: Option<&SSps>,
    pLastNalHdrExt: &SNalUnitHeaderExt,
    pCurNalHeaderExt: &SNalUnitHeaderExt,
    pLastSliceHeader: &SSliceHeader,
    pCurSliceHeader: &SSliceHeader,
) -> bool {

    // Subclause 7.1.4.1.1 temporal_id
    if pLastNalHdrExt.uiTemporalId != pCurNalHeaderExt.uiTemporalId {
        return true;
    }
    // Subclause 7.4.1.2.5
    if pLastSliceHeader.iRedundantPicCnt > pCurSliceHeader.iRedundantPicCnt {
        return true;
    }
    // Subclause G.7.4.1.2.4
    if pLastNalHdrExt.uiDependencyId > pCurNalHeaderExt.uiDependencyId {
        return true;
    }
    if pLastNalHdrExt.uiQualityId > pCurNalHeaderExt.uiQualityId {
        return true;
    }
    // Subclause 7.4.1.2.4
    if pLastSliceHeader.iFrameNum != pCurSliceHeader.iFrameNum {
        return true;
    }
    if pLastSliceHeader.iPpsId != pCurSliceHeader.iPpsId {
        return true;
    }
    if pLastSliceHeader.sps_ref.is_some() && pCurSliceHeader.sps_ref.is_some() {
        // The ids *are* the comparison now — and they carry which buffer they index,
        // where the C compared `pSps->iSpsId` and could not tell the two apart.
        if pLastSliceHeader.sps_ref != pCurSliceHeader.sps_ref {
            return true;
        }
    }
    if pLastSliceHeader.bFieldPicFlag != pCurSliceHeader.bFieldPicFlag {
        return true;
    }
    if pLastSliceHeader.bBottomFiledFlag != pCurSliceHeader.bBottomFiledFlag {
        return true;
    }
    if (pLastNalHdrExt.sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
        != (pCurNalHeaderExt.sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
    {
        return true;
    }
    if pLastNalHdrExt.bIdrFlag != pCurNalHeaderExt.bIdrFlag {
        return true;
    }
    if pCurNalHeaderExt.bIdrFlag {
        if pLastSliceHeader.uiIdrPicId != pCurSliceHeader.uiIdrPicId {
            return true;
        }
    }
    if let Some(kpSps) = kpSps {
        if kpSps.uiPocType == 0 {
            if pLastSliceHeader.iPicOrderCntLsb != pCurSliceHeader.iPicOrderCntLsb {
                return true;
            }
            if pLastSliceHeader.iDeltaPicOrderCntBottom != pCurSliceHeader.iDeltaPicOrderCntBottom {
                return true;
            }
        } else if kpSps.uiPocType == 1 {
            if pLastSliceHeader.iDeltaPicOrderCnt[0] != pCurSliceHeader.iDeltaPicOrderCnt[0] {
                return true;
            }
            if pLastSliceHeader.iDeltaPicOrderCnt[1] != pCurSliceHeader.iDeltaPicOrderCnt[1] {
                return true;
            }
        }
    }
    false
}

/// Evaluates whether the current NAL begins a new picture / Access Unit boundary.
// **T5b.3: both predicates take the field they reach, not the context.** They read
// `pCtx->sSpsPpsCtx` and nothing else, and the two nodes they compare are two shared
// borrows out of one access unit — which cannot coexist with a `&mut` of the context
// the access unit lives in. Take-what-you-reach turns a three-way conflict into three
// shared borrows.
pub fn CheckAccessUnitBoundary(
    spsPps: &SWelsDecoderSpsPpsCTX,
    kpCurNal: &SNalUnit,
    kpLastNal: &SNalUnit,
    kpSpsRef: Option<SpsRef>,
) -> bool {
    let kpLastNalHeaderExt = &kpLastNal.sNalHeaderExt;
    let kpCurNalHeaderExt = &kpCurNal.sNalHeaderExt;
    let kpLastSliceHeader = &kpLastNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
    let kpCurSliceHeader = &kpCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;

    let dep_id = kpCurNalHeaderExt.uiDependencyId as usize;
    if dep_id < MAX_LAYER_NUM {
        let active_sps = spsPps.pActiveLayerSps[dep_id];
        if active_sps.is_some() && active_sps != kpSpsRef {
            return true;
        }
    }

    if kpLastNalHeaderExt.uiTemporalId != kpCurNalHeaderExt.uiTemporalId {
        return true;
    }
    if kpLastSliceHeader.iFrameNum != kpCurSliceHeader.iFrameNum {
        return true;
    }
    if kpLastSliceHeader.iRedundantPicCnt > kpCurSliceHeader.iRedundantPicCnt {
        return true;
    }
    if kpLastNalHeaderExt.uiDependencyId > kpCurNalHeaderExt.uiDependencyId {
        return true;
    }
    if kpLastNalHeaderExt.uiDependencyId == kpCurNalHeaderExt.uiDependencyId
        && kpLastSliceHeader.iPpsId != kpCurSliceHeader.iPpsId
    {
        return true;
    }
    if kpLastSliceHeader.bFieldPicFlag != kpCurSliceHeader.bFieldPicFlag {
        return true;
    }
    if kpLastSliceHeader.bBottomFiledFlag != kpCurSliceHeader.bBottomFiledFlag {
        return true;
    }
    if (kpLastNalHeaderExt.sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
        != (kpCurNalHeaderExt.sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
    {
        return true;
    }
    if kpLastNalHeaderExt.bIdrFlag != kpCurNalHeaderExt.bIdrFlag {
        return true;
    }
    if kpCurNalHeaderExt.bIdrFlag {
        if kpLastSliceHeader.uiIdrPicId != kpCurSliceHeader.uiIdrPicId {
            return true;
        }
    }
    if let Some(kpSps) = sps_of(spsPps, kpSpsRef) {
        if kpSps.uiPocType == 0 {
            if kpLastSliceHeader.iPicOrderCntLsb != kpCurSliceHeader.iPicOrderCntLsb {
                return true;
            }
            if kpLastSliceHeader.iDeltaPicOrderCntBottom != kpCurSliceHeader.iDeltaPicOrderCntBottom {
                return true;
            }
        } else if kpSps.uiPocType == 1 {
            if kpLastSliceHeader.iDeltaPicOrderCnt[0] != kpCurSliceHeader.iDeltaPicOrderCnt[0] {
                return true;
            }
            if kpLastSliceHeader.iDeltaPicOrderCnt[1] != kpCurSliceHeader.iDeltaPicOrderCnt[1] {
                return true;
            }
        }
    }

    false
}

/// Checks whether the current NAL begins a brand-new coded video sequence.
pub fn CheckNextAuNewSeq(
    spsPps: &SWelsDecoderSpsPpsCTX,
    kpCurNal: &SNalUnit,
    kpSpsRef: Option<SpsRef>,
) -> bool {
    let kpCurNalHeaderExt = &kpCurNal.sNalHeaderExt;
    let dep_id = kpCurNalHeaderExt.uiDependencyId as usize;
    if dep_id < MAX_LAYER_NUM {
        let active_sps = spsPps.pActiveLayerSps[dep_id];
        if active_sps.is_some() && active_sps != kpSpsRef {
            return true;
        }
    }
    if kpCurNalHeaderExt.bIdrFlag {
        return true;
    }
    false
}

/// Dispatches non-VCL NAL units (SPS, Subset SPS, PPS, SEI) to syntax parsers.
///
/// T3.3: `kiRbspStart` is the payload's offset into `sRawData` (was `pRbsp`). The
/// trailing-bits read is an index, in bounds because the `kiSrcLen <= 0` guard has
/// always preceded it.
///
/// **`kpSrcNal` is back** (T8b.B2). T3.3 deleted the reference's `pSrcNal`/`kSrcNalLen`
/// pair as dead, correctly for that tree — their only readers are the parse-only
/// SPS/PPS bitstream caches (`au_parser.cpp:1168-1200`, `:1480-1492`), and those had
/// not been carried, so the parameter really did reach nothing. Carrying them is what
/// gives `sSpsBsInfo`/`sSubsetSpsBsInfo`/`sPpsBsInfo` their first writer.
pub fn ParseNonVclNal(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    kiSrcLen: i32,
    kpSrcNal: &[u8],
) -> i32 {
    if kiSrcLen <= 0 {
        return ERR_NONE;
    }

    let pBs = &mut (*pCtx).sBs;
    // F15's third instance, the one the finding records as already guarded (the
    // `kiSrcLen <= 0` early return above). It goes through the same helper anyway:
    // one expression, one guard, nothing left that can form the index by
    // subtraction.
    let iBitSize = rbsp_bit_size((*pCtx).sRawData.bytes(), kiRbspStart, kiSrcLen);
    let eNalType = (*pCtx).sCurNalHead.eNalUnitType;
    let mut iPicWidth = 0;
    let mut iPicHeight = 0;
    let mut iErr = ERR_NONE;

    match eNalType {
        EWelsNalUnitType::NAL_UNIT_SPS | EWelsNalUnitType::NAL_UNIT_SUBSET_SPS => {
            if iBitSize > 0 {
                iErr = DecInitBits(pBs, &(*pCtx).sRawData, kiRbspStart, iBitSize);
                if iErr != ERR_NONE {
                    if (*pCtx).pParam.eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                    {
                        (*pCtx).iErrorCode |= dsNoParamSets;
                    } else {
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    }
                    return iErr;
                }
            }
            let (start, mut cursor) = (pBs.start, pBs.cursor);
            // The parse-tree exception's caller side — the argument is at the
            // callee's item (T5.AC9).
            {
                iErr = ParseSps(pCtx, start, &mut cursor, kpSrcNal, &mut iPicWidth, &mut iPicHeight);
            }
            (*pCtx).sBs.cursor = cursor;
            if iErr != ERR_NONE {
                if (*pCtx).pParam.eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                {
                    (*pCtx).iErrorCode |= dsNoParamSets;
                } else {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                }
                return iErr;
            }
            (*pCtx).bHasNewSps = true;
        }

        EWelsNalUnitType::NAL_UNIT_PPS => {
            if iBitSize > 0 {
                iErr = DecInitBits(pBs, &(*pCtx).sRawData, kiRbspStart, iBitSize);
                if iErr != ERR_NONE {
                    if (*pCtx).pParam.eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                    {
                        (*pCtx).iErrorCode |= dsNoParamSets;
                    } else {
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    }
                    return iErr;
                }
            }
            let (start, mut cursor) = (pBs.start, pBs.cursor);
            {
                iErr = ParsePps(pCtx, start, &mut cursor, kpSrcNal);
            }
            (*pCtx).sBs.cursor = cursor;
            if iErr != ERR_NONE {
                if (*pCtx).pParam.eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                {
                    (*pCtx).iErrorCode |= dsNoParamSets;
                } else {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                }
                (*pCtx).bHasNewSps = false;
                return iErr;
            }
            (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag = true;
            (*pCtx).sSpsPpsCtx.iSeqId += 1;
        }

        EWelsNalUnitType::NAL_UNIT_SEI => {
            // Reserved SEI parser hook
        }

        _ => {}
    }

    iErr
}

/// Parses reference base picture marking syntax for SVC temporal/spatial reference layers.
pub fn ParseRefBasePicMarking(
    buf: &[u8],
    pBs: &mut BsCursor,
    pRefBasePicMarking: &mut SRefBasePicMarking,
) -> i32 {
    let mut uiCode: u32 = 0;
    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
        return ERR_INVALID_PARAMETERS;
    }
    let kbAdaptiveMarkingModeFlag = uiCode != 0;
    pRefBasePicMarking.bAdaptiveRefBasePicMarkingModeFlag = kbAdaptiveMarkingModeFlag;

    if kbAdaptiveMarkingModeFlag {
        let mut iIdx = 0;
        loop {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                return ERR_INVALID_PARAMETERS;
            }
            let kuiMmco = uiCode;
            pRefBasePicMarking.mmco_base[iIdx].uiMmcoType = kuiMmco;

            if kuiMmco == MMCO_END {
                break;
            }
            if kuiMmco == MMCO_SHORT2UNUSED {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                    return ERR_INVALID_PARAMETERS;
                }
                pRefBasePicMarking.mmco_base[iIdx].uiDiffOfPicNums = 1 + uiCode;
                pRefBasePicMarking.mmco_base[iIdx].iShortFrameNum = 0;
            } else if kuiMmco == MMCO_LONG2UNUSED {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                    return ERR_INVALID_PARAMETERS;
                }
                pRefBasePicMarking.mmco_base[iIdx].uiLongTermPicNum = uiCode;
            }
            iIdx += 1;
            if iIdx >= MAX_MMCO_COUNT {
                break;
            }
        }
    }
    ERR_NONE
}

/// Parses prefix NAL unit syntax elements (NAL type 14).
pub fn ParsePrefixNalUnit(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
) -> i32 {
    // **T5.Z4: the offset travels, not the slice.** The caller cannot hand a window
    // out of `sRawData` *and* the context, because both are the same object once the
    // context is a `&mut`. The window is derived here from the field, and the
    // borrow ends before the parameter-set activation this function closes with —
    // which is the whole reason the two sub-parsers below stopped taking a context.
    let buf = pCtx.sRawData.window_from(kiRbspStart);
    let pCurNal = &mut (*pCtx).sSpsPpsCtx.sPrefixNal;
    let mut uiCode: u32 = 0;

    if pCurNal.sNalHeaderExt.sNalUnitHeader.uiNalRefIdc != 0 {
        let head_ext = &pCurNal.sNalHeaderExt;
        let sPrefixNal = &mut pCurNal.sNalData.sPrefixNal;

        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
            return ERR_INVALID_PARAMETERS;
        }
        sPrefixNal.bStoreRefBasePicFlag = uiCode != 0;

        if (head_ext.bUseRefBasePicFlag || sPrefixNal.bStoreRefBasePicFlag) && !head_ext.bIdrFlag {
            if ParseRefBasePicMarking(buf, pBs, &mut sPrefixNal.sRefPicBaseMarking) != ERR_NONE {
                return ERR_INVALID_PARAMETERS;
            }
        }

        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
            return ERR_INVALID_PARAMETERS;
        }
        sPrefixNal.bPrefixNalUnitAdditionalExtFlag = uiCode != 0;

        if sPrefixNal.bPrefixNalUnitAdditionalExtFlag {
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                return ERR_INVALID_PARAMETERS;
            }
            sPrefixNal.bPrefixNalUnitExtFlag = uiCode != 0;
        }
    }
    ERR_NONE
}

/// Decodes the SVC extension syntax block within a Subset SPS (`SSubsetSps`).
/// **T5.Z4: the context parameter is deleted.** It was read nowhere in the body,
/// and it is what made the window and the cursor collide in one call (S18).
pub fn DecodeSpsSvcExt(
    pSpsExt: &mut SSubsetSps,
    buf: &[u8],
    pBs: &mut BsCursor,
) -> i32 {
    let pExt = &mut pSpsExt.sSpsSvcExt;
    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pExt.bInterLayerDeblockingFilterCtrlPresentFlag = uiCode != 0;

    if BsGetBits(buf, pBs, 2, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pExt.uiExtendedSpatialScalability = uiCode as u8;
    if pExt.uiExtendedSpatialScalability > 2 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_ESS);
    }

    pExt.uiChromaPhaseXPlus1Flag = 0;
    pExt.uiChromaPhaseYPlus1 = 1;

    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pExt.uiChromaPhaseXPlus1Flag = uiCode as u8;

    if BsGetBits(buf, pBs, 2, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pExt.uiChromaPhaseYPlus1 = uiCode as u8;

    pExt.uiSeqRefLayerChromaPhaseXPlus1Flag = pExt.uiChromaPhaseXPlus1Flag;
    pExt.uiSeqRefLayerChromaPhaseYPlus1 = pExt.uiChromaPhaseYPlus1;
    pExt.sSeqScaledRefLayer = SPosOffset::default();

    if pExt.uiExtendedSpatialScalability == 1 {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pExt.uiSeqRefLayerChromaPhaseXPlus1Flag = uiCode as u8;

        if BsGetBits(buf, pBs, 2, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pExt.uiSeqRefLayerChromaPhaseYPlus1 = uiCode as u8;

        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pExt.sSeqScaledRefLayer.iLeftOffset = iCode;

        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pExt.sSeqScaledRefLayer.iTopOffset = iCode;

        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pExt.sSeqScaledRefLayer.iRightOffset = iCode;

        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pExt.sSeqScaledRefLayer.iBottomOffset = iCode;
    }

    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pExt.bSeqTCoeffLevelPredFlag = uiCode != 0;
    pExt.bAdaptiveTCoeffLevelPredFlag = false;
    if pExt.bSeqTCoeffLevelPredFlag {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pExt.bAdaptiveTCoeffLevelPredFlag = uiCode != 0;
    }

    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pExt.bSliceHeaderRestrictionFlag = uiCode != 0;

    ERR_NONE
}

/// Maps H.264 level indices to the static global lookup table [`g_ksLevelLimits`].
pub fn GetLevelLimits(iLevelIdx: i32, bConstraint3: bool) -> Option<&'static SLevelLimits> {
    match iLevelIdx {
        9 => Some(&g_ksLevelLimits[1]),
        10 => Some(&g_ksLevelLimits[0]),
        11 => {
            if bConstraint3 {
                Some(&g_ksLevelLimits[1])
            } else {
                Some(&g_ksLevelLimits[2])
            }
        }
        12 => Some(&g_ksLevelLimits[3]),
        13 => Some(&g_ksLevelLimits[4]),
        20 => Some(&g_ksLevelLimits[5]),
        21 => Some(&g_ksLevelLimits[6]),
        22 => Some(&g_ksLevelLimits[7]),
        30 => Some(&g_ksLevelLimits[8]),
        31 => Some(&g_ksLevelLimits[9]),
        32 => Some(&g_ksLevelLimits[10]),
        40 => Some(&g_ksLevelLimits[11]),
        41 => Some(&g_ksLevelLimits[12]),
        42 => Some(&g_ksLevelLimits[13]),
        50 => Some(&g_ksLevelLimits[14]),
        51 => Some(&g_ksLevelLimits[15]),
        52 => Some(&g_ksLevelLimits[16]),
        _ => None,
    }
}

/// Checks whether an SPS is actively in use by any layer context.
/// **The ref travels, not the pointer** (T5.Z1). This took `pSps: *const SSps`, and
/// its two callers derived that pointer *from the context they pass beside it* —
/// session Y's first Miri instance, and the one it fixed "by passing the index".
/// With [`SpsRef`] the identity compare below is a value compare and the SPS is
/// resolved inside, where nothing else is borrowed.
pub fn CheckSpsActive(
    pCtx: &mut SWelsDecoderContext,
    r: Option<SpsRef>,
    bUseSubsetFlag: bool,
) -> bool {
    for i in 0..MAX_LAYER_NUM {
        if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] == r && r.is_some() {
            return true;
        }
    }
    let Some(pSps) = sps_of(&(*pCtx).sSpsPpsCtx, r) else {
        return false;
    };
    let (iSpsId, iMbWidth, iMbHeight) = (pSps.iSpsId, pSps.iMbWidth, pSps.iMbHeight);
    let sps_id = iSpsId as usize;
    if sps_id >= MAX_SPS_COUNT {
        return false;
    }

    let avail = if bUseSubsetFlag {
        (*pCtx).sSpsPpsCtx.bSubspsAvailFlags[sps_id]
    } else {
        (*pCtx).sSpsPpsCtx.bSpsAvailFlags[sps_id]
    };
    if iMbWidth > 0 && iMbHeight > 0 && avail {
        if (*pCtx).iTotalNumMbRec > 0 {
            return true;
        }
        // The access unit is its own allocation, so the NAL walk below and the SPS
        // lookups inside it borrow two disjoint things (T5.O4).
        if let Some(pCurAu) = cur_au(&mut pCtx.access_unit) {
            let iNum = pCurAu.uiAvailUnitsNum as usize;
            for i in 0..iNum {
                let Some(pNalUnit) = pCurAu.node(i) else {
                    continue;
                };
                if pNalUnit.sNalData.sVclNal.bSliceHeaderExtFlag != bUseSubsetFlag {
                    continue;
                }
                let next = sps_of(
                    &(*pCtx).sSpsPpsCtx,
                    (*pNalUnit).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref,
                );
                if next.is_some_and(|n| n.iSpsId == iSpsId) {
                    return true;
                }
            }
        }
    }
    false
}

/// Parses Sequence Parameter Sets (SPS and Subset SPS).
pub fn ParseSps(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBsAux: &mut BsCursor,
    kpSrcNal: &[u8],
    pPicWidth: &mut i32,
    pPicHeight: &mut i32,
) -> i32 {
    // **T5.Z4: the offset travels, not the slice.** The caller cannot hand a window
    // out of `sRawData` *and* the context, because both are the same object once the
    // context is a `&mut`. The window is derived here from the field, and the
    // borrow ends before the parameter-set activation this function closes with —
    // which is the whole reason the two sub-parsers below stopped taking a context.
    let buf = pCtx.sRawData.window_from(kiRbspStart);

    // `memset (pSubsetSps, 0, sizeof (SSubsetSps))` in `au_parser.cpp`, as a value
    // (T5b.4). The distinction T5b.3 measured is now carried by the *type* rather than
    // by a byte-writing shell: `SSubsetSps::default()` is **not** all-zero — it sets
    // `uiBitDepthLuma`/`Chroma` to 8 and `bFrameMbsOnlyFlag` to true through `sSps`,
    // which the parse then reads on the paths that do not write them, and substituting
    // it took eleven conformance assets red (`test_scalinglist_jm` among them).
    // [`SSubsetSps::memset_zero`] is the C's start, spelled out field by field, and
    // it is the only thing `write_bytes` was still here for: `bytes_equal` compares
    // fields since T5b.3, so the padding no longer has to be zeroed either.
    let mut sTempSubsetSps = SSubsetSps::memset_zero();
    let pSubsetSps = &mut sTempSubsetSps;

    let kbUseSubsetFlag = IS_SUBSET_SPS_NAL((*pCtx).sCurNalHead.eNalUnitType);

    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;
    let mut bConstraintSetFlags = [false; 6];

    if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    let uiProfileIdc = uiCode as u8;

    if uiProfileIdc != PRO_BASELINE
        && uiProfileIdc != PRO_MAIN
        && uiProfileIdc != PRO_SCALABLE_BASELINE
        && uiProfileIdc != PRO_SCALABLE_HIGH
        && uiProfileIdc != PRO_EXTENDED
        && uiProfileIdc != PRO_HIGH
    {
        // **F50, and it is `ERR_NONE` on purpose.** `au_parser.cpp:947` spells this
        // arm `return false;` inside a function whose every other exit is an error
        // code, so the C++ reports **success** for an unsupported `profile_idc`:
        // `false` converts to 0, which is `ERR_NONE`, so `ParseNonVclNal`'s
        // `if (ERR_NONE != iErr)` does not fire, no `dsNoParamSets`/`dsBitstreamError`
        // is raised, and `bHasNewSps` is set for an SPS that was never stored.
        //
        // The port had transliterated the arm's *intent* (reject) instead of its
        // *value* (0), which is the whole of F50: 24 corpus rows — `hdr1.07` and
        // `hdr2.07` in each of the 12 tables that have both sites — relabel a NAL as
        // an SPS, so `profile_idc` is whatever the borrowed payload starts with
        // (0xee on `narrow_16x16`), and the port answered `dsBitstreamError` where
        // the C++ answered `dsErrorFree`.
        //
        // The whole decoder's C++ has exactly one instance of the shape: every other
        // `return false` under `codec/decoder/core/src/` (21 sites) is in a function
        // that really does return `bool`, checked one by one.
        return ERR_NONE;
    }

    for i in 0..6 {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        bConstraintSetFlags[i] = uiCode != 0;
    }

    if BsGetBits(buf, pBsAux, 2, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    let uiLevelIdc = uiCode as u8;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    if uiCode >= MAX_SPS_COUNT as u32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_SPS_ID_OVERFLOW);
    }
    let iSpsId = uiCode as i32;

    // The lookup stays for its `None` arm, which is live: an unrecognized level is
    // `ERR_INFO_UNSUPPORTED_NON_BASELINE` and the subset SPS is refused. The row it
    // finds is no longer stored — T5b.9 deleted `SSps::pSLevelLimits`, whose only
    // C++ readers are the four `WELS_CHECK_SE_BOTH_WARNING` log sites T5.Y2 already
    // ruled on.
    if GetLevelLimits(uiLevelIdc as i32, bConstraintSetFlags[3]).is_none() {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
    }

    pSubsetSps.sSps.uiChromaFormatIdc = 1;
    pSubsetSps.sSps.uiChromaArrayType = 1;
    pSubsetSps.sSps.uiProfileIdc = uiProfileIdc;
    pSubsetSps.sSps.uiLevelIdc = uiLevelIdc;
    pSubsetSps.sSps.iSpsId = iSpsId;

    if uiProfileIdc == PRO_SCALABLE_BASELINE
        || uiProfileIdc == PRO_SCALABLE_HIGH
        || uiProfileIdc == PRO_HIGH
        || uiProfileIdc == PRO_HIGH10
        || uiProfileIdc == PRO_HIGH422
        || uiProfileIdc == PRO_HIGH444
        || uiProfileIdc == PRO_CAVLC444
        || uiProfileIdc == 44
    {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.uiChromaFormatIdc = uiCode as u8;
        if pSubsetSps.sSps.uiChromaFormatIdc > 1 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSubsetSps.sSps.uiChromaArrayType = pSubsetSps.sSps.uiChromaFormatIdc;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSubsetSps.sSps.uiBitDepthLuma = 8;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSubsetSps.sSps.uiBitDepthChroma = 8;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.bQpPrimeYZeroTransfBypassFlag = uiCode != 0;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.bSeqScalingMatrixPresentFlag = uiCode != 0;

        if pSubsetSps.sSps.bSeqScalingMatrixPresentFlag {
            let src = ScalingListSource::of(&mut pSubsetSps.sSps);
            ParseScalingList(
                &src,
                buf,
                pBsAux,
                false,
                false,
                &mut pSubsetSps.sSps.bSeqScalingListPresentFlag,
                &mut pSubsetSps.sSps.iScalingList4x4,
                &mut pSubsetSps.sSps.iScalingList8x8,
            );
        }
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    if uiCode > SPS_LOG2_MAX_FRAME_NUM_MINUS4_MAX {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_LOG2_MAX_FRAME_NUM_MINUS4);
    }
    pSubsetSps.sSps.uiLog2MaxFrameNum = LOG2_MAX_FRAME_NUM_OFFSET + uiCode;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.uiPocType = uiCode;

    if pSubsetSps.sSps.uiPocType == 0 {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode > SPS_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4);
        }
        pSubsetSps.sSps.iLog2MaxPocLsb = LOG2_MAX_PIC_ORDER_CNT_LSB_OFFSET + uiCode as i32;
    } else if pSubsetSps.sSps.uiPocType == 1 {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.bDeltaPicOrderAlwaysZeroFlag = uiCode != 0;

        if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.iOffsetForNonRefPic = iCode;

        if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.iOffsetForTopToBottomField = iCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode > SPS_NUM_REF_FRAMES_IN_PIC_ORDER_CNT_CYCLE_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_NUM_REF_FRAME_IN_PIC_ORDER_CNT_CYCLE);
        }
        pSubsetSps.sSps.iNumRefFramesInPocCycle = uiCode as i32;

        for i in 0..pSubsetSps.sSps.iNumRefFramesInPocCycle as usize {
            if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pSubsetSps.sSps.iOffsetForRefFrame[i] = iCode as i8;
        }
    }

    if pSubsetSps.sSps.uiPocType > 2 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_POC_TYPE);
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.iNumRefFrames = uiCode as i32;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.bGapsInFrameNumValueAllowedFlag = uiCode != 0;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.iMbWidth = (PIC_WIDTH_IN_MBS_OFFSET + uiCode as i32) as u32;
    if pSubsetSps.sSps.iMbWidth > MAX_MB_SIZE as u32 || pSubsetSps.sSps.iMbWidth == 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_MB_SIZE);
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.iMbHeight = (PIC_HEIGHT_IN_MAP_UNITS_OFFSET + uiCode as i32) as u32;
    if pSubsetSps.sSps.iMbHeight > MAX_MB_SIZE as u32 || pSubsetSps.sSps.iMbHeight == 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_MB_SIZE);
    }

    let uiTmp64 = pSubsetSps.sSps.iMbWidth as u64 * pSubsetSps.sSps.iMbHeight as u64;
    pSubsetSps.sSps.uiTotalMbCount = uiTmp64 as u32;

    if pSubsetSps.sSps.iNumRefFrames as u32 > SPS_MAX_NUM_REF_FRAMES_MAX_VAL {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_NUM_REF_FRAMES);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.bFrameMbsOnlyFlag = uiCode != 0;
    if !pSubsetSps.sSps.bFrameMbsOnlyFlag {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_MBAFF);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.bDirect8x8InferenceFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.bFrameCroppingFlag = uiCode != 0;

    if pSubsetSps.sSps.bFrameCroppingFlag {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.sFrameCrop.iLeftOffset = uiCode as i32;
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.sFrameCrop.iRightOffset = uiCode as i32;
        if (pSubsetSps.sSps.sFrameCrop.iLeftOffset + pSubsetSps.sSps.sFrameCrop.iRightOffset) > (pSubsetSps.sSps.iMbWidth as i32 * 16 / 2) {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CROPPING_DATA);
        }

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.sFrameCrop.iTopOffset = uiCode as i32;
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSubsetSps.sSps.sFrameCrop.iBottomOffset = uiCode as i32;
        if (pSubsetSps.sSps.sFrameCrop.iTopOffset + pSubsetSps.sSps.sFrameCrop.iBottomOffset) > (pSubsetSps.sSps.iMbHeight as i32 * 16 / 2) {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CROPPING_DATA);
        }
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSubsetSps.sSps.bVuiParamPresentFlag = uiCode != 0;
    if pSubsetSps.sSps.bVuiParamPresentFlag {
        // **F46, T5.T3 — the arms were inverted** (`au_parser.cpp:1156`). The C++
        // reads: *if* the VUI failed because it carries HRD, tolerate it — except on a
        // subset SPS, where it is fatal — and **otherwise propagate whatever it
        // returned** (`WELS_READ_VERIFY`). The port propagated the subset-HRD case and
        // swallowed everything else, so a VUI that ran out of bits left `ParseSps`
        // returning `ERR_NONE`: the port **accepted a truncated SPS** the C++ rejects,
        // and answered `dsErrorFree` where the C++ answers `dsBitstreamError`. All 22
        // truncation rows still disagreeing after T5.T2 are this one arm, and closing
        // it takes the corpus to **2318 / 0** on codes.
        let iRetVui = ParseVui(&mut pSubsetSps.sSps, buf, pBsAux);
        if iRetVui == GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_VUI_HRD) {
            // Currently no support for VUI with HRD enabled in a subset SPS.
            if kbUseSubsetFlag {
                return iRetVui;
            }
        } else if iRetVui != ERR_NONE {
            return iRetVui;
        }
    }

    // ------------------------------------------------------------------
    // `au_parser.cpp:1168-1257` — the parse-only SPS caches (T8b.B2), between the VUI
    // and the SVC extension exactly as in the reference: the rewrite below is a
    // *plain* SPS, so it must be built from the syntax elements parsed so far and
    // not from the extension that follows.
    // ------------------------------------------------------------------
    if (*pCtx).pParam.bParseOnly {
        if kpSrcNal.len() >= SPS_PPS_BS_SIZE - 4 {
            crate::decoder::decoder_core::WelsLog(
                std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                crate::decoder::decoder_core::WELS_LOG_WARNING,
                &format!(
                    "sps payload size ({}) too large for parse only ({}), not supported!",
                    kpSrcNal.len(),
                    SPS_PPS_BS_SIZE - 4
                ),
            );
            (*pCtx).iErrorCode |= dsBitstreamError;
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_OUT_OF_MEMORY);
        }
        if !kbUseSubsetFlag {
            if let Some(row) = (*pCtx).sSpsBsInfo.get_mut(iSpsId as usize) {
                parse_only_write_sps(row, iSpsId, kpSrcNal);
            }
        } else {
            // The rewrite reads the SPS being parsed — a local — and writes the
            // context's cache row, so the two borrows are disjoint by construction.
            let sps = pSubsetSps.sSps;
            let ok = match (*pCtx).sSubsetSpsBsInfo.get_mut(iSpsId as usize) {
                Some(row) => parse_only_write_subset_sps(row, &sps),
                None => true,
            };
            if !ok {
                crate::decoder::decoder_core::WelsLog(
                    std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                    crate::decoder::decoder_core::WELS_LOG_ERROR,
                    "subset sps rewrite does not fit the parse-only buffer",
                );
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_OUT_OF_MEMORY);
            }
        }
    }

    if kbUseSubsetFlag && (uiProfileIdc == PRO_SCALABLE_BASELINE || uiProfileIdc == PRO_SCALABLE_HIGH) {
        let iRet = DecodeSpsSvcExt(pSubsetSps, buf, pBsAux);
        if iRet != ERR_NONE {
            return iRet;
        }
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        (*pSubsetSps).bSvcVuiParamPresentFlag = uiCode != 0;
    }

    *pPicWidth = (pSubsetSps.sSps.iMbWidth << 4) as i32;
    *pPicHeight = (pSubsetSps.sSps.iMbHeight << 4) as i32;

    let idx = iSpsId as usize;
    let tmp_ref = Some(SpsRef { id: iSpsId, subset: kbUseSubsetFlag });
    if kbUseSubsetFlag {
        if CheckSpsActive(pCtx, tmp_ref, true) {
            // Overwriting the active subset SPS: only act when it actually changed.
            if !bytes_equal(&(*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx], &*pSubsetSps) {
                if au_has_nals(pCtx) {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT], &*pSubsetSps);
                    mark_au_ready(pCtx);
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SUBSETSPS;
                } else if active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                    .is_some_and(|s| s.iSpsId == (*pSubsetSps).sSps.iSpsId)
                {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT], &*pSubsetSps);
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SUBSETSPS;
                } else {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx], &*pSubsetSps);
                }
            }
        } else {
            bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx], &*pSubsetSps);
            (*pCtx).sSpsPpsCtx.bSubspsAvailFlags[idx] = true;
            (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag = true;
        }
    } else {
        if CheckSpsActive(pCtx, tmp_ref, false) {
            // Overwriting the active SPS: only act when it actually changed.
            if !bytes_equal(&(*pCtx).sSpsPpsCtx.sSpsBuffer[idx], &mut pSubsetSps.sSps) {
                if au_has_nals(pCtx) {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT], &mut pSubsetSps.sSps);
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SPS;
                    mark_au_ready(pCtx);
                } else if active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                    .is_some_and(|s| s.iSpsId == pSubsetSps.sSps.iSpsId)
                {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT], &mut pSubsetSps.sSps);
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SPS;
                } else {
                    bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSpsBuffer[idx], &mut pSubsetSps.sSps);
                }
            }
        } else {
            bytes_copy(&mut (*pCtx).sSpsPpsCtx.sSpsBuffer[idx], &mut pSubsetSps.sSps);
            (*pCtx).sSpsPpsCtx.bSpsAvailFlags[idx] = true;
            (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag = true;
        }
    }

    ERR_NONE
}

/// Parses Picture Parameter Sets (PPS).
pub fn ParsePps(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBsAux: &mut BsCursor,
    kpSrcNal: &[u8],
) -> i32 {
    // **T5.Z4: the offset travels, not the slice.** The caller cannot hand a window
    // out of `sRawData` *and* the context, because both are the same object once the
    // context is a `&mut`. The window is derived here from the field, and the
    // borrow ends before the parameter-set activation this function closes with —
    // which is the whole reason the two sub-parsers below stopped taking a context.
    let buf = pCtx.sRawData.window_from(kiRbspStart);

    // T5.X6: `pPpsList: *mut SPps` stood here — the context's PPS buffer base,
    // passed by every caller and **read nowhere in the body**, which reaches the
    // buffer through `pCtx` instead. Dead since the function was written; deleted
    // rather than converted (S18).
    // `memset (pPps, 0, sizeof (SPps))` in au_parser.cpp, as a value (T5b.4), for the
    // reason `ParseSps` gives: `SPps::default()` sets `uiNumSliceGroups`,
    // `uiNumRefIdxL0Active`/`L1Active` to 1 and `iPicInitQp`/`Qs` to 26, and the C
    // starts from all-zero. The padding clause F31 and T5.R8 argued over is spent —
    // the comparison against the active PPS is field-wise since T5b.3, so nothing
    // downstream depends on the bytes behind the fields.
    let mut sTempPpsStore = SPps::memset_zero();
    let pPps = &mut sTempPpsStore;

    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    let uiPpsId = uiCode;
    if uiPpsId >= MAX_PPS_COUNT as u32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_PPS_ID_OVERFLOW);
    }
    pPps.iPpsId = uiPpsId as i32;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.iSpsId = uiCode as i32;
    if pPps.iSpsId >= MAX_SPS_COUNT as i32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_SPS_ID_OVERFLOW);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bEntropyCodingModeFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bPicOrderPresentFlag = uiCode != 0;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.uiNumSliceGroups = NUM_SLICE_GROUPS_OFFSET + uiCode;

    if pPps.uiNumSliceGroups > MAX_SLICEGROUP_IDS as u32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_SLICEGROUP);
    }

    if pPps.uiNumSliceGroups > 1 {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pPps.uiSliceGroupMapType = uiCode;
        if pPps.uiSliceGroupMapType > 1 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_FMOTYPE);
        }
        if pPps.uiSliceGroupMapType == 0 {
            for iTmp in 0..pPps.uiNumSliceGroups as usize {
                if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
                pPps.uiRunLength[iTmp] = RUN_LENGTH_OFFSET + uiCode;
            }
        }
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.uiNumRefIdxL0Active = NUM_REF_IDX_L0_DEFAULT_ACTIVE_OFFSET + uiCode;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.uiNumRefIdxL1Active = NUM_REF_IDX_L1_DEFAULT_ACTIVE_OFFSET + uiCode;

    if pPps.uiNumRefIdxL0Active > MAX_REF_PIC_COUNT as u32
        || pPps.uiNumRefIdxL1Active > MAX_REF_PIC_COUNT as u32
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_REF_COUNT_OVERFLOW);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bWeightedPredFlag = uiCode != 0;

    if BsGetBits(buf, pBsAux, 2, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pPps.uiWeightedBipredIdc = uiCode as u8;

    if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pPps.iPicInitQp = PIC_INIT_QP_OFFSET + iCode;
    if pPps.iPicInitQp < PPS_PIC_INIT_QP_QS_MIN || pPps.iPicInitQp > PPS_PIC_INIT_QP_QS_MAX {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_PIC_INIT_QP);
    }

    if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pPps.iPicInitQs = PIC_INIT_QS_OFFSET + iCode;
    if pPps.iPicInitQs < PPS_PIC_INIT_QP_QS_MIN || pPps.iPicInitQs > PPS_PIC_INIT_QP_QS_MAX {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_PIC_INIT_QS);
    }

    if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
    pPps.iChromaQpIndexOffset[0] = iCode;
    if pPps.iChromaQpIndexOffset[0] < PPS_CHROMA_QP_INDEX_OFFSET_MIN
        || pPps.iChromaQpIndexOffset[0] > PPS_CHROMA_QP_INDEX_OFFSET_MAX
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CHROMA_QP_INDEX_OFFSET);
    }
    pPps.iChromaQpIndexOffset[1] = pPps.iChromaQpIndexOffset[0];

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bDeblockingFilterControlPresentFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bConstainedIntraPredFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pPps.bRedundantPicCntPresentFlag = uiCode != 0;

    if CheckMoreRBSPData(pBsAux) {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pPps.bTransform8x8ModeFlag = uiCode != 0;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pPps.bPicScalingMatrixPresentFlag = uiCode != 0;

        if pPps.bPicScalingMatrixPresentFlag {
            if (*pCtx).sSpsPpsCtx.bSpsAvailFlags[pPps.iSpsId as usize] {
                let src =
                    ScalingListSource::of(&(*pCtx).sSpsPpsCtx.sSpsBuffer[pPps.iSpsId as usize]);
                ParseScalingList(
                    &src,
                    buf,
                    pBsAux,
                    true,
                    pPps.bTransform8x8ModeFlag,
                    &mut pPps.bPicScalingListPresentFlag,
                    &mut pPps.iScalingList4x4,
                    &mut pPps.iScalingList8x8,
                );
            } else {
                return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_SPS_ID);
            }
        }

        if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pPps.iChromaQpIndexOffset[1] = iCode;
        if pPps.iChromaQpIndexOffset[1] < PPS_CHROMA_QP_INDEX_OFFSET_MIN
            || pPps.iChromaQpIndexOffset[1] > PPS_CHROMA_QP_INDEX_OFFSET_MAX
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CHROMA_QP_INDEX_OFFSET);
        }
    }

    let pps_idx = uiPpsId as usize;
    if active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps).is_some_and(|p| p.iPpsId == pPps.iPpsId) {
        // Re-sent PPS for the active id: only flag an overwrite when it changed.
        // The comparison is against the active entry, resolved once as a value so
        // the borrow ends before the copies below write the same array (T5.Z1).
        let unchanged = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
            .is_some_and(|active| bytes_equal(active, pPps));
        if !unchanged {
            bytes_copy(&mut (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT], pPps);
            (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_PPS;
            mark_au_ready(pCtx);
        }
    } else {
        bytes_copy(&mut (*pCtx).sSpsPpsCtx.sPpsBuffer[pps_idx], pPps);
        (*pCtx).sSpsPpsCtx.bPpsAvailFlags[pps_idx] = true;
    }

    // `au_parser.cpp:1471-1493` — the parse-only PPS cache (T8b.B2), last thing in
    // the function as in the reference.
    if (*pCtx).pParam.bParseOnly {
        if kpSrcNal.len() >= SPS_PPS_BS_SIZE - 4 {
            crate::decoder::decoder_core::WelsLog(
                std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                crate::decoder::decoder_core::WELS_LOG_WARNING,
                &format!(
                    "pps payload size ({}) too large for parse only ({}), not supported!",
                    kpSrcNal.len(),
                    SPS_PPS_BS_SIZE - 4
                ),
            );
            (*pCtx).iErrorCode |= dsBitstreamError;
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_OUT_OF_MEMORY);
        }
        if let Some(row) = (*pCtx).sPpsBsInfo.get_mut(pps_idx) {
            parse_only_write_pps(row, uiPpsId as i32, kpSrcNal);
        }
    }

    ERR_NONE
}

/// Parses Video Usability Information (VUI) parameters inside an SPS.
/// **T5.Z4: the context parameter is deleted** — see `DecodeSpsSvcExt` (S18).
pub fn ParseVui(
    pSps: &mut SSps,
    buf: &[u8],
    pBsAux: &mut BsCursor,
) -> i32 {
    let mut uiCode: u32 = 0;
    let pVui = &mut pSps.sVui;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bAspectRatioInfoPresentFlag = uiCode != 0;
    if pVui.bAspectRatioInfoPresentFlag {
        if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pVui.uiAspectRatioIdc = uiCode;
        if (pVui.uiAspectRatioIdc as usize) < 17 {
            pVui.uiSarWidth = g_ksVuiSampleAspectRatio[pVui.uiAspectRatioIdc as usize].uiWidth;
            pVui.uiSarHeight = g_ksVuiSampleAspectRatio[pVui.uiAspectRatioIdc as usize].uiHeight;
        } else if pVui.uiAspectRatioIdc as u8 == EXTENDED_SAR {
            if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pVui.uiSarWidth = uiCode;
            if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pVui.uiSarHeight = uiCode;
        }
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bOverscanInfoPresentFlag = uiCode != 0;
    if pVui.bOverscanInfoPresentFlag {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.bOverscanAppropriateFlag = uiCode != 0;
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bVideoSignalTypePresentFlag = uiCode != 0;
    if pVui.bVideoSignalTypePresentFlag {
        if BsGetBits(buf, pBsAux, 3, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pVui.uiVideoFormat = uiCode as u8;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.bVideoFullRangeFlag = uiCode != 0;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.bColourDescripPresentFlag = uiCode != 0;
        if pVui.bColourDescripPresentFlag {
            if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pVui.uiColourPrimaries = uiCode as u8;

            if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pVui.uiTransferCharacteristics = uiCode as u8;

            if BsGetBits(buf, pBsAux, 8, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pVui.uiMatrixCoeffs = uiCode as u8;
        }
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bChromaLocInfoPresentFlag = uiCode != 0;
    if pVui.bChromaLocInfoPresentFlag {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiChromaSampleLocTypeTopField = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiChromaSampleLocTypeBottomField = uiCode;
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bTimingInfoPresentFlag = uiCode != 0;
    if pVui.bTimingInfoPresentFlag {
        let mut uiTmp: u32;
        if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        uiTmp = uiCode << 16;
        if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        uiTmp |= uiCode;
        pVui.uiNumUnitsInTick = uiTmp;

        if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        uiTmp = uiCode << 16;
        if BsGetBits(buf, pBsAux, 16, &mut uiCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        uiTmp |= uiCode;
        pVui.uiTimeScale = uiTmp;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.bFixedFrameRateFlag = uiCode != 0;
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bNalHrdParamPresentFlag = uiCode != 0;
    if pVui.bNalHrdParamPresentFlag {
        let mut cpb_cnt_minus1: u32 = 0;
        if BsGetUe(buf, pBsAux, &mut cpb_cnt_minus1) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        let _ = BsGetBits(buf, pBsAux, 4, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 4, &mut uiCode);
        for _ in 0..=(cpb_cnt_minus1 as i32) {
            let _ = BsGetUe(buf, pBsAux, &mut uiCode);
            let _ = BsGetUe(buf, pBsAux, &mut uiCode);
            let _ = BsGetOneBit(buf, pBsAux, &mut uiCode);
        }
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bVclHrdParamPresentFlag = uiCode != 0;
    if pVui.bVclHrdParamPresentFlag {
        let mut cpb_cnt_minus1: u32 = 0;
        if BsGetUe(buf, pBsAux, &mut cpb_cnt_minus1) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        let _ = BsGetBits(buf, pBsAux, 4, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 4, &mut uiCode);
        for _ in 0..=(cpb_cnt_minus1 as i32) {
            let _ = BsGetUe(buf, pBsAux, &mut uiCode);
            let _ = BsGetUe(buf, pBsAux, &mut uiCode);
            let _ = BsGetOneBit(buf, pBsAux, &mut uiCode);
        }
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
        let _ = BsGetBits(buf, pBsAux, 5, &mut uiCode);
    }

    if pVui.bNalHrdParamPresentFlag || pVui.bVclHrdParamPresentFlag {
        let _ = BsGetOneBit(buf, pBsAux, &mut uiCode);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bPicStructPresentFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pVui.bBitstreamRestrictionFlag = uiCode != 0;
    if pVui.bBitstreamRestrictionFlag {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.bMotionVectorsOverPicBoundariesFlag = uiCode != 0;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiMaxBytesPerPicDenom = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiMaxBitsPerMbDenom = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiLog2MaxMvLengthHorizontal = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiLog2MaxMvLengthVertical = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiMaxNumReorderFrames = uiCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pVui.uiMaxDecFrameBuffering = uiCode;
    }

    ERR_NONE
}

/// Reserved SEI message parsing hook.
pub fn ParseSei(_pBsAux: &mut BsCursor) -> i32 {
    ERR_NONE
}

/// Decodes frequency scaling matrix values from signed delta codes.
pub fn SetScalingListValue(
    pScalingList: &mut [u8],
    iScalingListNum: i32,
    bUseDefaultScalingMatrixFlag: &mut bool,
    buf: &[u8],
    pBsAux: &mut BsCursor,
) -> i32 {
    let mut iLastScale: i32 = 8;
    let mut iNextScale: i32 = 8;
    let mut iCode: i32 = 0;

    for j in 0..iScalingListNum as usize {
        if iNextScale != 0 {
            if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE {
                return ERR_INVALID_PARAMETERS;
            }
            if iCode < SCALING_LIST_DELTA_SCALE_MIN || iCode > SCALING_LIST_DELTA_SCALE_MAX {
                return ERR_SCALING_LIST_DELTA_SCALE;
            }
            let iDeltaScale = iCode;
            iNextScale = (iLastScale + iDeltaScale + 256) % 256;
            *bUseDefaultScalingMatrixFlag = j == 0 && iNextScale == 0;
            if *bUseDefaultScalingMatrixFlag {
                break;
            }
        }
        let iIdx = if iScalingListNum == 16 {
            g_kuiZigzagScan[j] as usize
        } else {
            g_kuiZigzagScan8x8[j] as usize
        };
        let val = if iNextScale == 0 {
            iLastScale as u8
        } else {
            iNextScale as u8
        };
        pScalingList[iIdx] = val;
        iLastScale = val as i32;
    }

    ERR_NONE
}

/// What [`ParseScalingList`] reads out of the SPS.
///
/// **T5.X6: copied at the call, because the lists it writes are fields of that same
/// SPS when the caller is `ParseSps`.** As raw pointers the source and the
/// destination could be one object and nothing in the signature said so; as borrows
/// they cannot be. The C++'s own `bInit` is `bPPS && sps->bSeqScalingMatrixPresentFlag`
/// and `ParseSps` passes `bPPS = false`, so the fallbacks are never read on the path
/// where the two would alias — the copy is of four arrays the PPS path reads out of a
/// *different* SPS.
#[derive(Copy, Clone)]
pub struct ScalingListSource {
    pub uiChromaFormatIdc: u8,
    pub bSeqScalingMatrixPresentFlag: bool,
    /// The SPS's 4x4 lists 0 and 3 — the two the C++ names `defaultScaling4x4_*`.
    pub prev4x4: [[u8; 16]; 2],
    /// The SPS's 8x8 lists 0 and 1.
    pub prev8x8: [[u8; 64]; 2],
}

impl ScalingListSource {
    pub fn of(pSps: &SSps) -> Self {
        Self {
            uiChromaFormatIdc: pSps.uiChromaFormatIdc,
            bSeqScalingMatrixPresentFlag: pSps.bSeqScalingMatrixPresentFlag,
            prev4x4: [pSps.iScalingList4x4[0], pSps.iScalingList4x4[3]],
            prev8x8: [pSps.iScalingList8x8[0], pSps.iScalingList8x8[1]],
        }
    }
}

/// Parses 4x4 and 8x8 frequency scaling list matrices.
pub fn ParseScalingList(
    pSps: &ScalingListSource,
    buf: &[u8],
    pBs: &mut BsCursor,
    bPPS: bool,
    kbTrans8x8ModeFlag: bool,
    pScalingListPresentFlag: &mut [bool],
    iScalingList4x4: &mut [[u8; 16]],
    iScalingList8x8: &mut [[u8; 64]],
) -> i32 {
    let mut uiCode: u32 = 0;
    let mut bUseDefaultScalingMatrixFlag4x4 = false;
    let mut bUseDefaultScalingMatrixFlag8x8 = false;

    let uiScalingListNum = if !bPPS {
        if pSps.uiChromaFormatIdc != 3 { 8 } else { 12 }
    } else {
        6 + (kbTrans8x8ModeFlag as usize) * if pSps.uiChromaFormatIdc != 3 { 2 } else { 6 }
    };

    let bInit = if bPPS { pSps.bSeqScalingMatrixPresentFlag } else { false };

    let defaultScaling4x4_0 = if bInit { pSps.prev4x4[0] } else { g_kuiDequantScaling4x4Default[0] };
    let defaultScaling4x4_1 = if bInit { pSps.prev4x4[1] } else { g_kuiDequantScaling4x4Default[1] };
    let defaultScaling8x8_0 = if bInit { pSps.prev8x8[0] } else { g_kuiDequantScaling8x8Default[0] };
    let defaultScaling8x8_1 = if bInit { pSps.prev8x8[1] } else { g_kuiDequantScaling8x8Default[1] };

    for i in 0..uiScalingListNum {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
            return ERR_INVALID_PARAMETERS;
        }
        pScalingListPresentFlag[i] = uiCode != 0;

        if uiCode != 0 {
            if i < 6 {
                SetScalingListValue(
                    &mut iScalingList4x4[i],
                    16,
                    &mut bUseDefaultScalingMatrixFlag4x4,
                    buf,
                    pBs,
                );
                if bUseDefaultScalingMatrixFlag4x4 {
                    bUseDefaultScalingMatrixFlag4x4 = false;
                    iScalingList4x4[i] = g_kuiDequantScaling4x4Default[i / 3];
                }
            } else {
                SetScalingListValue(
                    &mut iScalingList8x8[i - 6],
                    64,
                    &mut bUseDefaultScalingMatrixFlag8x8,
                    buf,
                    pBs,
                );
                if bUseDefaultScalingMatrixFlag8x8 {
                    bUseDefaultScalingMatrixFlag8x8 = false;
                    iScalingList8x8[i - 6] = g_kuiDequantScaling8x8Default[(i - 6) & 1];
                }
            }
        } else {
            if i < 6 {
                if i != 0 && i != 3 {
                    iScalingList4x4[i] = iScalingList4x4[i - 1];
                } else {
                    iScalingList4x4[i] =
                        if i / 3 == 0 { defaultScaling4x4_0 } else { defaultScaling4x4_1 };
                }
            } else {
                if i == 6 || i == 7 {
                    iScalingList8x8[i - 6] =
                        if ((i & 1) + 2) == 2 { defaultScaling8x8_0 } else { defaultScaling8x8_1 };
                } else {
                    iScalingList8x8[i - 6] = iScalingList8x8[i - 8];
                }
            }
        }
    }

    ERR_NONE
}

/// Resets FMO contexts and returns count of active FMO units.
///
/// **F51 (session V): the list reset was missing, and only the counter was here.**
/// `au_parser.cpp:1794` clears every active entry of `sFmoList` —
/// `UninitFmoList (&pCtx->sFmoList[0], MAX_PPS_COUNT, pCtx->iActiveFmoNum, …)` — and
/// *then* zeroes `iActiveFmoNum`. The port zeroed the counter alone, so after a new
/// SPS every entry kept `bActiveFlag = true` with the previous sequence's map. Two
/// consequences, both C++-visible: `FmoParamUpdate`'s re-activation arm
/// (`!bActiveFlag && iActiveFmoNum < MAX_PPS_COUNT`) could never fire again, so
/// `iActiveFmoNum` stayed 0 for the decoder's life and this function returned 0
/// where the C++ returns a real count; and an entry whose slice-group parameters
/// happen to match the new sequence's kept its stale map, because
/// `FmoParamSetsChanged`'s first term — the one that exists to catch exactly this —
/// is `!bActiveFlag`.
pub fn ResetFmoList(pCtx: &mut SWelsDecoderContext) -> i32 {
    let iCountNum = (*pCtx).iActiveFmoNum;
    crate::decoder::fmo::UninitFmoList(&mut (*pCtx).sFmoList, iCountNum);
    (*pCtx).iActiveFmoNum = 0;
    iCountNum
}

// ============================================================================
// Access Unit List Dynamic Memory Management
// ============================================================================

/// Grows the node list, keeping every existing node **at its address**.
///
/// The C++ (`memmgr_nal_unit.cpp:120`) allocates a second contiguous block, `memcpy`s
/// the nodes into it and frees the first — so every outstanding `SNalUnit*` (the
/// context's `pNalCur`, `DecodeCurrentAccessUnit`'s local, the slice header's
/// back-pointers) dangles the moment an access unit outgrows its list. That is P5's
/// hazard in the habitat it was named for. Pushing boxed nodes onto a `Vec` keeps the
/// old nodes exactly where they were, so the growth is invisible to anything holding
/// one.
pub fn ExpandNalUnitList(pAu: &mut SAccessUnit, kiOrgSize: i32, kiExpSize: i32) -> i32 {
    if kiExpSize <= kiOrgSize {
        return ERR_INVALID_PARAMETERS;
    }
    let want = kiExpSize as usize;
    if pAu.nal_units.try_reserve(want - pAu.nal_units.len()).is_err() {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    while pAu.nal_units.len() < want {
        pAu.nal_units.push(Box::new(SNalUnit::default()));
    }
    ERR_NONE
}

/// Retrieves the next available [`SNalUnit`] node from the AU list, expanding capacity if needed.
///
/// The returned pointer is a **copy of a stored node pointer**, so it outlives every
/// later retag of the access unit — which is what lets the caller hold it while the
/// context's `access_unit` is derived again. See [`TagAccessUnits::nal`].
pub fn MemGetNextNal(pAu: &mut SAccessUnit) -> Option<usize> {
    if pAu.uiAvailUnitsNum >= pAu.count() {
        let kuiExpandingSize = pAu.count() + (MAX_NAL_UNIT_NUM_IN_AU as u32 >> 1);
        let org = pAu.count() as i32;
        if ExpandNalUnitList(pAu, org, kuiExpandingSize as i32) != ERR_NONE {
            return None;
        }
        // No re-read of the access unit: growth no longer moves it, which is the whole
        // point of T5.O4's ownership (the C++ replaces the block here).
    }

    let idx = pAu.uiAvailUnitsNum as usize;
    pAu.uiAvailUnitsNum += 1;
    // **T5b.3: the index, not the node.** The caller re-acquires through it, so
    // nothing outlives the expression that took it and the container is free to own.
    // T5b.6: the C's `memset (pNu, 0, sizeof (SNalUnit))` is a value. T5b.8: that
    // value is [`SNalUnit::default`] — measured identical to the zero image on
    // 6,055 of 6,056 bytes, the exception being `sSliceHeader.sps_ref`'s niche,
    // which F56 ruled belongs to `None` (the C zeroes a `pSps` pointer there).
    *pAu.nal(idx) = SNalUnit::default();
    Some(idx)
}

/// Clears the most recently added corrupted NAL unit from the AU list.
pub fn ForceClearCurrentNal(pAu: &mut SAccessUnit) {
    if pAu.uiAvailUnitsNum > 0 {
        pAu.uiAvailUnitsNum -= 1;
    }
}

/// Drops the NAL just queued and ends the access unit one NAL earlier.
///
/// The error tail the slice branch of [`ParseNalHeader`] spells four times, taking
/// the availability count from *before* the failure because that is what the C++'s
/// hoisted `uiAvailNalNum` held. The concealment-disabled arm is what makes the
/// truncated access unit decodable at all.
///
/// The access-unit borrow ends before `pParam` is read: nothing requires that here,
/// and everything is easier to check when a derivation covers one statement.
fn discard_nal_and_close_au(pCtx: &mut SWelsDecoderContext, uiAvailNalNum: u32) {
    if let Some(au) = cur_au(&mut pCtx.access_unit) {
        ForceClearCurrentNal(au);
        if uiAvailNalNum > 1 {
            au.uiEndPos = uiAvailNalNum - 2;
        }
    }
    if uiAvailNalNum > 1
        && (*pCtx).pParam.eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE
    {
        (*pCtx).bAuReadyFlag = true;
    }
}

/// Prefetches and synchronizes prefix NAL header extension parameters into slice headers.
pub fn prefetch_nal_header_ext(kppDst: &mut SNalUnit, kpSrc: &SNalUnit) -> bool {
    let pNalHdrExtD = &mut kppDst.sNalHeaderExt;
    let pNalHdrExtS = &kpSrc.sNalHeaderExt;

    pNalHdrExtD.uiDependencyId = pNalHdrExtS.uiDependencyId;
    pNalHdrExtD.uiQualityId = pNalHdrExtS.uiQualityId;
    pNalHdrExtD.uiTemporalId = pNalHdrExtS.uiTemporalId;
    pNalHdrExtD.uiPriorityId = pNalHdrExtS.uiPriorityId;
    pNalHdrExtD.bIdrFlag = pNalHdrExtS.bIdrFlag;
    pNalHdrExtD.bNoInterLayerPredFlag = pNalHdrExtS.bNoInterLayerPredFlag;

    true
}

/// Resets active SPS pointers for each layer if MB reconstruction has not started.
pub fn ResetActiveSPSForEachLayer(pCtx: &mut SWelsDecoderContext) {
    if (*pCtx).iTotalNumMbRec == 0 {
        for i in 0..MAX_LAYER_NUM {
            (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = None;
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod au_list_tests {
    use super::*;

    /// **F56's red-under-revert test at the NAL node (T5b.8).** `MemGetNextNal`
    /// hands out a node the C has just `memset` to zero, and the C's zero for
    /// `pSliceHeader->pSps` is a null pointer. The port spelled that memset as
    /// `SNalUnit::memset_zero`, which wrote `Some(SpsRef { id: 0, subset: false })`
    /// into `sSliceHeader.sps_ref` — not because anything transcribed it, but
    /// because `Option<SpsRef>` keeps its niche in `SpsRef`'s `bool` and the zero
    /// image reads back that way (F54's class). Restore that overwrite and this
    /// assertion is the one that fails.
    #[test]
    fn a_fresh_nal_node_has_parsed_no_sps() {
        let mut au = SAccessUnit::with_nodes(MAX_NAL_UNIT_NUM_IN_AU);
        // Dirty the slot first, so the assertion is about the reset and not about
        // what `with_nodes` happened to leave there.
        au.nal(0).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref =
            Some(crate::decoder::decoder_context::SpsRef { id: 3, subset: true });
        let idx = MemGetNextNal(&mut au).expect("a node");
        assert_eq!(idx, 0);
        assert!(
            au.nal(idx)
                .sNalData
                .sVclNal
                .sSliceHeaderExt
                .sSliceHeader
                .sps_ref
                .is_none(),
            "MemGetNextNal's memset leaves a null pSps, which is None"
        );
    }

    /// T5.O4/F39: growing the node list keeps every existing node where it was, and
    /// keeps what was in it.
    ///
    /// The C++ this replaces (`memmgr_nal_unit.cpp:120`) allocates a second block,
    /// copies the nodes and frees the first, so every outstanding `SNalUnit*` dangles
    /// — plan P5's hazard, in the code it was named after. Nothing in the battery
    /// reached the growth path before this session, because the port's live allocator
    /// started the list at 1024 entries instead of the C++'s 32; it starts at 32 now,
    /// and this pins the property directly rather than hoping a stream reaches it.
    #[test]
    fn growing_the_nal_list_moves_no_node() {
        {
            let mut au = SAccessUnit::with_nodes(MAX_NAL_UNIT_NUM_IN_AU);
            assert_eq!(au.count(), MAX_NAL_UNIT_NUM_IN_AU as u32);

            // Fill the list through the path the parser uses, stamping each node so a
            // move would be visible, and remember where every node lives.
            // T5b.3: the slots own, so "did a node move" is asked of its *address*,
            // taken through the borrow rather than stored as the answer. T5b.9:
            // `std::ptr::from_ref(..).addr()` is that question spelled without a cast
            // — the `as *const SNalUnit as usize` pair it replaces was the last raw
            // pointer type written in `src/decoder/` that named neither the api
            // boundary nor a kernel argument.
            let mut addrs: Vec<usize> = Vec::new();
            for i in 0..MAX_NAL_UNIT_NUM_IN_AU {
                let idx = MemGetNextNal(&mut au).expect("a free slot");
                let nal = au.nal(idx);
                nal.uiTimeStamp = 1000 + i as u64;
                addrs.push(std::ptr::from_ref(&*nal).addr());
            }
            assert_eq!(au.uiAvailUnitsNum, MAX_NAL_UNIT_NUM_IN_AU as u32);

            // One past the end: this is the growth.
            let grown = MemGetNextNal(&mut au).expect("the growth hands back a slot");
            assert_eq!(
                au.count(),
                (MAX_NAL_UNIT_NUM_IN_AU + (MAX_NAL_UNIT_NUM_IN_AU >> 1)) as u32,
                "expansion size is the C++'s: count + (MAX >> 1)"
            );

            for (i, &p) in addrs.iter().enumerate() {
                let nal = au.nal(i);
                assert_eq!(std::ptr::from_ref(&*nal).addr(), p, "node {i} moved across the growth");
                assert_eq!(nal.uiTimeStamp, 1000 + i as u64, "node {i} lost its contents");
            }
            // `MemGetNextNal` zeroes the node it hands out.
            assert_eq!(au.nal(grown).uiTimeStamp, 0);
        }
    }

    /// The reordering swaps `ResetCurrentAccessUnit`/`ForceResetCurrentAccessUnit` do
    /// used to exchange two entries of a pointer array; they exchange owned nodes now.
    #[test]
    fn swapping_two_nodes_exchanges_their_contents_and_nothing_else() {
        {
            let mut au = SAccessUnit::with_nodes(4);
            for i in 0..4usize {
                (*au.nal(i)).uiTimeStamp = i as u64;
            }
            au.nal_units.swap(0, 3);
            let seen: Vec<u64> = (0..4).map(|i| (*au.nal(i)).uiTimeStamp).collect();
            assert_eq!(seen, vec![3, 1, 2, 0]);
        }
    }

    /// **F51's red-under-revert test** (session V). `ResetFmoList` must clear the FMO
    /// list, not only the counter: `au_parser.cpp:1794` calls `UninitFmoList` over
    /// `sFmoList` and *then* zeroes `iActiveFmoNum`. Delete the `UninitFmoList` call in
    /// `ResetFmoList` and this test fails on its first assertion — the entry keeps
    /// `bActiveFlag` and its previous map — and on the last one, because
    /// `FmoParamUpdate`'s re-activation arm never fires again for an entry that still
    /// claims to be active.
    #[test]
    fn reset_fmo_list_clears_the_entries_the_cpp_clears() {
        {
            let mut ctx = SWelsDecoderContext::new_boxed();
            let pCtx = &mut *ctx;

            // One active entry with a map, exactly as `FmoParamUpdate` leaves it.
            (*pCtx).sFmoList[0].bActiveFlag = true;
            (*pCtx).sFmoList[0].pMbAllocMap = vec![0u8; 16];
            (*pCtx).sFmoList[0].iCountMbNum = 16;
            (*pCtx).sFmoList[0].iSliceGroupCount = 1;
            (*pCtx).sFmoList[0].iSliceGroupType = 0;
            (*pCtx).iActiveFmoNum = 1;

            assert_eq!(ResetFmoList(pCtx), 1, "returns the count it reset");
            assert!(!(*pCtx).sFmoList[0].bActiveFlag, "the entry is deactivated");
            assert!((*pCtx).sFmoList[0].pMbAllocMap.is_empty(), "its map is dropped");
            assert_eq!((*pCtx).sFmoList[0].iCountMbNum, 0);
            assert_eq!((*pCtx).sFmoList[0].iSliceGroupType, -1);
            assert_eq!((*pCtx).iActiveFmoNum, 0);

            // And the consequence that made it worth finding: a cleared entry can be
            // re-activated, so the counter climbs again.
            let mut sps = crate::decoder::parameter_sets::SSps::default();
            sps.iMbWidth = 4;
            sps.iMbHeight = 4;
            let mut pps = crate::decoder::parameter_sets::SPps::default();
            pps.uiNumSliceGroups = 1;
            let ret = crate::decoder::fmo::FmoParamUpdate(
                Some(&mut (*pCtx).sFmoList[0]),
                Some(&sps),
                Some(&pps),
                &mut (*pCtx).iActiveFmoNum,
            );
            assert_eq!(ret, ERR_NONE);
            assert_eq!((*pCtx).iActiveFmoNum, 1, "re-activation counts again");
        }
    }

    /// **F52's coverage** — `CheckAccessUnitBoundaryExt` returns `false` for two NAL
    /// units that agree on every field it compares.
    ///
    /// That is the arm a stub returning `true` unconditionally destroys, and it was
    /// destroyed: `decoder_core.rs` defined a four-parameter
    /// `CheckAccessUnitBoundaryExt { true }` in the module that calls it, so the real
    /// implementation below had no caller at all. This test is **red under
    /// re-stubbing** — replace this function's body with `true` and it fails — which
    /// is the property the deleted stub violated. It does not, and cannot, prove the
    /// *wiring*; the compiler does that, now that only one such function exists.
    #[test]
    fn check_access_unit_boundary_ext_says_no_boundary_when_nothing_differs() {
        let hdr = SNalUnitHeaderExt::default();
        let sh = SSliceHeader::default();
        assert!(
            !CheckAccessUnitBoundaryExt(None, &hdr, &hdr, &sh, &sh),
            "identical headers are the same access unit"
        );

        // And one field at a time is enough to make it a boundary — the fifteen
        // comparisons are what the stub was standing in for.
        let mut cur_hdr = hdr;
        cur_hdr.uiTemporalId = 1;
        assert!(CheckAccessUnitBoundaryExt(None, &hdr, &cur_hdr, &sh, &sh));

        let mut cur_sh = sh.clone();
        cur_sh.iFrameNum = 1;
        assert!(CheckAccessUnitBoundaryExt(None, &hdr, &hdr, &sh, &cur_sh));
    }

    /// **F92's reachability probe (D-fid-6, session J).** No `res/` asset produces a
    /// subset SPS whose plain-SPS rewrite needs an emulation-prevention byte, so the
    /// one behavior where the port deliberately diverges from upstream — reporting
    /// the **escaped** length where `au_parser.cpp:1252` stores the RBSP length,
    /// one byte short per inserted `0x03` — had no referee. This is that referee,
    /// synthetic on purpose: it drives the writer directly with an `SSps` crafted so
    /// the rewrite emits `.. 00 00 02 ..` (`uiLevelIdc = 0` puts two zero bytes after
    /// `profile_idc`; `iSpsId = 63`'s exp-Golomb prefix makes the next byte `0x02`),
    /// which `rbsp_to_ebsp` must escape.
    #[test]
    fn subset_sps_rewrite_reports_the_escaped_length_when_an_escape_is_needed() {
        use crate::decoder::decoder_context::SSpsBsInfo;
        use crate::decoder::parameter_sets::SSps;

        let craft = |level: u8, sps_id: i32| -> SSps {
            let mut sps = SSps::default();
            sps.iSpsId = sps_id;
            sps.uiLevelIdc = level;
            sps.uiLog2MaxFrameNum = 4; // ue(0)
            sps.uiPocType = 0;
            sps.iLog2MaxPocLsb = 4; // ue(0)
            sps.iNumRefFrames = 0;
            sps.iMbWidth = 1; // ue(0)
            sps.iMbHeight = 1;
            sps.bFrameMbsOnlyFlag = true;
            sps
        };

        let verify = |sps: &SSps, want_escapes: usize| {
            let mut row = SSpsBsInfo::default();
            assert!(super::parse_only_write_subset_sps(&mut row, sps), "the rewrite fits");
            let len = row.uiSpsBsLen as usize;
            assert_eq!(&row.pSpsBsBuf[..5], &[0x00, 0x00, 0x00, 0x01, 0x67]);
            // De-escape the payload the length names: every inserted byte is a
            // `03` after `00 00`. The count is the whole disagreement with
            // upstream — its stored length is exactly `escapes` bytes short.
            let payload = &row.pSpsBsBuf[5..len];
            let mut escapes = 0usize;
            let mut i = 0usize;
            while i + 2 < payload.len() {
                if payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 3 {
                    escapes += 1;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            assert_eq!(escapes, want_escapes, "escape count for level {}", sps.uiLevelIdc);
            // The length reaches exactly the end of the written NAL: the RBSP
            // trailing stop bit makes the final byte nonzero, so a length cut
            // short by the escape count (upstream's) could not end here.
            assert_ne!(payload.last().copied(), Some(0), "the length ends on the stop-bit byte");
        };

        // The reached arm: `4D 00 00 02` forces one emulation-prevention byte.
        verify(&craft(0, 63), 1);
        // The control: an ordinary level has no zero pair and the two length
        // formulas agree — which is why every `res/` golden passes under both.
        verify(&craft(30, 0), 0);
    }
}
