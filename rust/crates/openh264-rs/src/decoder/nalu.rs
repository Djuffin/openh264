#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

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
//! 6. Dynamic memory management for AU NAL pointer arrays ([`MemInitNalList`], [`MemGetNextNal`]).

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
use crate::common::memory_align::*;

// Explicit imports to resolve glob ambiguities
use crate::decoder::bit_stream::{BsReader, ERR_NONE, ERR_INVALID_PARAMETERS, ERR_INFO_OUT_OF_MEMORY};
use crate::safe::bits::BsCursor;

use crate::decoder::dec_golomb::{BsGetOneBit, BsGetUe, BsGetSe, BsGetBits};
use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext, MAX_LAYER_NUM, SPosOffset};
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
    t == EWelsNalUnitType::NAL_UNIT_SPS || t == EWelsNalUnitType::NAL_UNIT_SUBSET_SPS
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
pub type PNalUnitHeader = *mut SNalUnitHeader;

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
pub type PNalUnitHeaderExt = *mut SNalUnitHeaderExt;

/// Video Coding Layer (VCL) slice payload representation.
///
/// The payload's identity is [`sSliceBitsRead`](Self::sSliceBitsRead)'s `start`
/// offset into the decoder's `sRawData` since T3.3. `pNalPos: *mut u8` was deleted
/// dead there (S18): nothing in this port ever wrote it — the upstream parse-only
/// output path that fills it was never carried — and its one read sat behind an
/// always-true null guard. `iNalLength` stays: the parse-only NAL-length
/// bookkeeping does execute (with its perpetual default of 0).
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SVclNal {
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub sSliceBitsRead: BsReader,
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
pub type PPrefixNalUnit = *mut SPrefixNalUnit;

/// Discriminated payload union inside [`SNalUnit`].
#[repr(C)]
#[derive(Copy, Clone)]
pub union SNalData {
    pub sVclNal: SVclNal,
    pub sPrefixNal: SPrefixNalUnit,
}

impl Default for SNalData {
    fn default() -> Self {
        SNalData {
            sVclNal: SVclNal::default(),
        }
    }
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
pub type PNalUnit = *mut SNalUnit;

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
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TagAccessUnits {
    pub pNalUnitsList: *mut *mut SNalUnit,
    pub uiAvailUnitsNum: u32,
    pub uiActualUnitsNum: u32,
    pub uiCountUnitsNum: u32,
    pub uiStartPos: u32,
    pub uiEndPos: u32,
    pub bCompletedAuFlag: bool,
}

pub type SAccessUnit = TagAccessUnits;
pub type PAccessUnit = *mut SAccessUnit;

impl Default for TagAccessUnits {
    fn default() -> Self {
        Self {
            pNalUnitsList: std::ptr::null_mut(),
            uiAvailUnitsNum: 0,
            uiActualUnitsNum: 0,
            uiCountUnitsNum: 0,
            uiStartPos: 0,
            uiEndPos: 0,
            bCompletedAuFlag: false,
        }
    }
}

// ============================================================================
// Lookup Tables
// ============================================================================

/// 4x4 block residual zig-zag scan order.
pub const g_kuiZigzagScan: [u8; 16] = [
    0, 1, 4, 8,
    5, 2, 3, 6,
    9, 12, 13, 10,
    7, 11, 14, 15,
];

/// 8x8 block residual zig-zag scan order.
pub const g_kuiZigzagScan8x8: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10,
    17, 24, 32, 25, 18, 11, 4, 5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

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

/// Byte-wise equality of two POD parameter-set structs, matching the `memcmp`
/// guards in `ParseSps` / `ParsePps` (`au_parser.cpp`).
unsafe fn bytes_equal<T>(a: *const T, b: *const T) -> bool {
    std::slice::from_raw_parts(a as *const u8, std::mem::size_of::<T>())
        == std::slice::from_raw_parts(b as *const u8, std::mem::size_of::<T>())
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
pub unsafe fn DecodeNalHeaderExt(pNal: *mut SNalUnit, src: &[u8]) {
    let pHeaderExt = &mut (*pNal).sNalHeaderExt;

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

/// Parses the NAL unit header byte, checks parameter set existence, and routes
/// the NAL unit to the appropriate syntactic decoder.
///
/// T3.3: the payload's identity is an **offset into `sRawData`** (`kiRbspStart`,
/// minted by `RawDataBuffer::append_ebsp_stripped`), and the return is the offset
/// past the consumed headers — `Some(offset)` where the C returned an advanced
/// pointer, `None` where it returned null. Every read below is an index into the
/// owning buffer.
pub unsafe fn ParseNalHeader(
    pCtx: *mut SWelsDecoderContext,
    pNalUnitHeader: *mut SNalUnitHeader,
    kiRbspStart: usize,
    iSrcRbspLen: i32,
    pConsumedBytes: *mut i32,
) -> Option<usize> {
    let pCurNal: *mut SNalUnit;
    let bytes = (*pCtx).sRawData.bytes();
    let mut iNal = kiRbspStart;
    let mut iNalSize = iSrcRbspLen;

    (*pNalUnitHeader).eNalUnitType = EWelsNalUnitType::NAL_UNIT_UNSPEC_0;

    // Remove consecutive ZERO bytes at the end of current NAL in reverse order
    let mut iIndex = iSrcRbspLen - 1;
    while iIndex >= 0 {
        if bytes[kiRbspStart + iIndex as usize] == 0 {
            iNalSize -= 1;
            *pConsumedBytes += 1;
            iIndex -= 1;
        } else {
            break;
        }
    }

    (*pNalUnitHeader).uiForbiddenZeroBit = bytes[iNal] >> 7;
    if (*pNalUnitHeader).uiForbiddenZeroBit != 0 {
        (*pCtx).iErrorCode |= dsBitstreamError;
        return None;
    }

    (*pNalUnitHeader).uiNalRefIdc = (bytes[iNal] >> 5) & 0x03;
    (*pNalUnitHeader).eNalUnitType = match bytes[iNal] & 0x1F {
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

    let eType = (*pNalUnitHeader).eNalUnitType;

    if !(IS_SEI_NAL(eType)
        || IS_SPS_NAL(eType)
        || IS_AU_DELIMITER_NAL(eType)
        || (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag)
    {
        if !(*pCtx).pDecoderStatistics.is_null() {
            (*(*pCtx).pDecoderStatistics).iSpsNoExistNalNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return None;
    }

    if !(IS_SEI_NAL(eType)
        || IS_PARAM_SETS_NALS(eType)
        || IS_AU_DELIMITER_NAL(eType)
        || (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag)
    {
        if !(*pCtx).pDecoderStatistics.is_null() {
            (*(*pCtx).pDecoderStatistics).iPpsNoExistNalNum += 1;
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
        if !(*pCtx).pDecoderStatistics.is_null() {
            (*(*pCtx).pDecoderStatistics).iSubSpsNoExistNalNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return None;
    }

    match eType {
        EWelsNalUnitType::NAL_UNIT_AU_DELIMITER | EWelsNalUnitType::NAL_UNIT_SEI => {
            let pCurAu = (*pCtx).pAccessUnitList;
            if !pCurAu.is_null() && (*pCurAu).uiAvailUnitsNum > 0 {
                (*pCurAu).uiEndPos = (*pCurAu).uiAvailUnitsNum - 1;
                (*pCtx).bAuReadyFlag = true;
            }
        }

        EWelsNalUnitType::NAL_UNIT_PREFIX => {
            pCurNal = &mut (*pCtx).sSpsPpsCtx.sPrefixNal;
            (*pCurNal).uiTimeStamp = (*pCtx).uiTimeStamp;

            if iNalSize < NAL_UNIT_HEADER_EXT_SIZE as i32 {
                let pCurAu = (*pCtx).pAccessUnitList;
                if !pCurAu.is_null() {
                    let uiAvailNalNum = (*pCurAu).uiAvailUnitsNum;
                    if uiAvailNalNum > 0 {
                        (*pCurAu).uiEndPos = uiAvailNalNum - 1;
                        (*pCtx).bAuReadyFlag = true;
                    }
                }
                (*pCurNal).sNalData.sPrefixNal.bPrefixNalCorrectFlag = false;
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            DecodeNalHeaderExt(pCurNal, &bytes[iNal..iNal + NAL_UNIT_HEADER_EXT_SIZE]);
            if (*pCurNal).sNalHeaderExt.uiQualityId != 0
                || (*pCurNal).sNalHeaderExt.bUseRefBasePicFlag
            {
                let pCurAu = (*pCtx).pAccessUnitList;
                if !pCurAu.is_null() {
                    let uiAvailNalNum = (*pCurAu).uiAvailUnitsNum;
                    if uiAvailNalNum > 0 {
                        (*pCurAu).uiEndPos = uiAvailNalNum - 1;
                        (*pCtx).bAuReadyFlag = true;
                    }
                }
                (*pCurNal).sNalData.sPrefixNal.bPrefixNalCorrectFlag = false;
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            iNal += NAL_UNIT_HEADER_EXT_SIZE;
            iNalSize -= NAL_UNIT_HEADER_EXT_SIZE as i32;
            *pConsumedBytes += NAL_UNIT_HEADER_EXT_SIZE as i32;

            (*pCurNal).sNalHeaderExt.sNalUnitHeader.uiForbiddenZeroBit = (*pNalUnitHeader).uiForbiddenZeroBit;
            (*pCurNal).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc = (*pNalUnitHeader).uiNalRefIdc;
            (*pCurNal).sNalHeaderExt.sNalUnitHeader.eNalUnitType = (*pNalUnitHeader).eNalUnitType;

            if (*pNalUnitHeader).uiNalRefIdc != 0 {
                // F15, preserved verbatim for one more commit: with iNalSize == 0 the
                // `iNalSize as usize - 1` panics in debug and wraps in release, where
                // the pointer lands on the header byte. The checked form and the
                // golden un-withholding land together in the F15 commit.
                let iBitSize = (iNalSize << 3)
                    - BsGetTrailingBits(bytes.as_ptr().add(iNal).add(iNalSize as usize - 1));
                let iErr = DecInitBits(&mut (*pCtx).sBs, &(*pCtx).sRawData, iNal, iBitSize);
                if iErr != ERR_NONE {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }
                let (buf, cursor) = (*pCtx).sBs.split(&(*pCtx).sRawData);
                ParsePrefixNalUnit(pCtx, buf, cursor);
            }
            (*pCurNal).sNalData.sPrefixNal.bPrefixNalCorrectFlag = true;
        }

        // `case NAL_UNIT_CODED_SLICE_EXT: bExtensionFlag = true;` falls through into
        // the shared slice body in C, so all three NAL types run the same code and an
        // SVC slice-extension NAL reaches ParseSliceHeaderSyntaxs with the flag set.
        // Splitting this into separate match arms leaves type-20 slices unparsed.
        EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
        | EWelsNalUnitType::NAL_UNIT_CODED_SLICE
        | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR => {
            let bExtensionFlag = eType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

            pCurNal = MemGetNextNal(&mut (*pCtx).pAccessUnitList, (*pCtx).pMemAlign);
            if pCurNal.is_null() {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return None;
            }
            (*pCurNal).uiTimeStamp = (*pCtx).uiTimeStamp;
            (*pCurNal).sNalHeaderExt.sNalUnitHeader.uiForbiddenZeroBit = (*pNalUnitHeader).uiForbiddenZeroBit;
            (*pCurNal).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc = (*pNalUnitHeader).uiNalRefIdc;
            (*pCurNal).sNalHeaderExt.sNalUnitHeader.eNalUnitType = (*pNalUnitHeader).eNalUnitType;

            let pCurAu = (*pCtx).pAccessUnitList;
            let uiAvailNalNum = (*pCurAu).uiAvailUnitsNum;

            if bExtensionFlag {
                if iNalSize < NAL_UNIT_HEADER_EXT_SIZE as i32 {
                    ForceClearCurrentNal(pCurAu);
                    if uiAvailNalNum > 1 {
                        (*pCurAu).uiEndPos = uiAvailNalNum - 2;
                        if !(*pCtx).pParam.is_null()
                            && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE
                        {
                            (*pCtx).bAuReadyFlag = true;
                        }
                    }
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }

                DecodeNalHeaderExt(pCurNal, &bytes[iNal..iNal + NAL_UNIT_HEADER_EXT_SIZE]);
                if (*pCurNal).sNalHeaderExt.uiQualityId != 0
                    || (*pCurNal).sNalHeaderExt.bUseRefBasePicFlag
                {
                    // MGS not supported.
                    ForceClearCurrentNal(pCurAu);
                    if uiAvailNalNum > 1 {
                        (*pCurAu).uiEndPos = uiAvailNalNum - 2;
                        if !(*pCtx).pParam.is_null()
                            && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE
                        {
                            (*pCtx).bAuReadyFlag = true;
                        }
                    }
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    return None;
                }
                iNal += NAL_UNIT_HEADER_EXT_SIZE;
                iNalSize -= NAL_UNIT_HEADER_EXT_SIZE as i32;
                *pConsumedBytes += NAL_UNIT_HEADER_EXT_SIZE as i32;
            } else {
                if (*pCtx).sSpsPpsCtx.sPrefixNal.sNalHeaderExt.sNalUnitHeader.eNalUnitType
                    == EWelsNalUnitType::NAL_UNIT_PREFIX
                {
                    if (*pCtx).sSpsPpsCtx.sPrefixNal.sNalData.sPrefixNal.bPrefixNalCorrectFlag {
                        PrefetchNalHeaderExtSyntax(pCtx, pCurNal, &mut (*pCtx).sSpsPpsCtx.sPrefixNal);
                    }
                }

                // SHOULD update this flag for AVC if no prefix NAL.
                (*pCurNal).sNalHeaderExt.bIdrFlag =
                    eType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
                (*pCurNal).sNalHeaderExt.bNoInterLayerPredFlag = true;
            }

            let pBs = &mut (*(*(*pCurAu).pNalUnitsList.add((uiAvailNalNum - 1) as usize)))
                .sNalData
                .sVclNal
                .sSliceBitsRead;
            // F15, preserved verbatim for one more commit: with iNalSize == 0 the
            // `iNalSize as usize - 1` panics in debug and wraps in release, where the
            // pointer lands on the header byte. The checked form and the golden
            // un-withholding land together in the F15 commit.
            let trailing_bits = crate::decoder::dec_golomb::BsGetTrailingBits(
                bytes.as_ptr().add(iNal).add(iNalSize as usize - 1),
            );
            let iBitSize = (iNalSize << 3) - trailing_bits;
            let mut iErr =
                crate::decoder::bit_stream::DecInitBits(pBs, &(*pCtx).sRawData, iNal, iBitSize);
            if iErr != ERR_NONE {
                ForceClearCurrentNal(pCurAu);
                if uiAvailNalNum > 1 {
                    (*pCurAu).uiEndPos = uiAvailNalNum - 2;
                    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE {
                        (*pCtx).bAuReadyFlag = true;
                    }
                }
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            let (buf, cursor) = pBs.split(&(*pCtx).sRawData);
            iErr = crate::decoder::decoder_core::ParseSliceHeaderSyntaxs(pCtx, buf, cursor, bExtensionFlag);
            if iErr != ERR_NONE {
                if uiAvailNalNum == 1 && (*pCurNal).sNalHeaderExt.bIdrFlag {
                    crate::decoder::decoder_core::ResetActiveSPSForEachLayer(pCtx);
                }
                ForceClearCurrentNal(pCurAu);
                if uiAvailNalNum > 1 {
                    (*pCurAu).uiEndPos = uiAvailNalNum - 2;
                    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE {
                        (*pCtx).bAuReadyFlag = true;
                    }
                }
                (*pCtx).iErrorCode |= dsBitstreamError;
                return None;
            }

            let p_last_nal = *(*pCurAu).pNalUnitsList.add((uiAvailNalNum - 1) as usize);
            let p_last_sps = (*p_last_nal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;

            if uiAvailNalNum == 1 && CheckNextAuNewSeq(pCtx, pCurNal, p_last_sps) {
                crate::decoder::decoder_core::ResetActiveSPSForEachLayer(pCtx);
            }
            if uiAvailNalNum > 1 {
                let p_prev_nal = *(*pCurAu).pNalUnitsList.add((uiAvailNalNum - 2) as usize);
                if CheckAccessUnitBoundary(pCtx, p_last_nal, p_prev_nal, p_last_sps) {
                    (*pCurAu).uiEndPos = uiAvailNalNum - 2;
                    (*pCtx).bAuReadyFlag = true;
                    (*pCtx).bNextNewSeqBegin = CheckNextAuNewSeq(pCtx, pCurNal, p_last_sps);
                }
            }
        }

        _ => {}
    }

    Some(iNal)
}

/// Evaluates whether two consecutive VCL NAL units belong to different Access Units.
pub unsafe fn CheckAccessUnitBoundaryExt(
    pLastNalHdrExt: *const SNalUnitHeaderExt,
    pCurNalHeaderExt: *const SNalUnitHeaderExt,
    pLastSliceHeader: *const SSliceHeader,
    pCurSliceHeader: *const SSliceHeader,
) -> bool {
    let kpSps = (*pCurSliceHeader).pSps as *const SSps;

    // Subclause 7.1.4.1.1 temporal_id
    if (*pLastNalHdrExt).uiTemporalId != (*pCurNalHeaderExt).uiTemporalId {
        return true;
    }
    // Subclause 7.4.1.2.5
    if (*pLastSliceHeader).iRedundantPicCnt > (*pCurSliceHeader).iRedundantPicCnt {
        return true;
    }
    // Subclause G.7.4.1.2.4
    if (*pLastNalHdrExt).uiDependencyId > (*pCurNalHeaderExt).uiDependencyId {
        return true;
    }
    if (*pLastNalHdrExt).uiQualityId > (*pCurNalHeaderExt).uiQualityId {
        return true;
    }
    // Subclause 7.4.1.2.4
    if (*pLastSliceHeader).iFrameNum != (*pCurSliceHeader).iFrameNum {
        return true;
    }
    if (*pLastSliceHeader).iPpsId != (*pCurSliceHeader).iPpsId {
        return true;
    }
    if !(*pLastSliceHeader).pSps.is_null() && !(*pCurSliceHeader).pSps.is_null() {
        if (*((*pLastSliceHeader).pSps as *mut SSps)).iSpsId != (*((*pCurSliceHeader).pSps as *mut SSps)).iSpsId {
            return true;
        }
    }
    if (*pLastSliceHeader).bFieldPicFlag != (*pCurSliceHeader).bFieldPicFlag {
        return true;
    }
    if (*pLastSliceHeader).bBottomFiledFlag != (*pCurSliceHeader).bBottomFiledFlag {
        return true;
    }
    if ((*pLastNalHdrExt).sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
        != ((*pCurNalHeaderExt).sNalUnitHeader.uiNalRefIdc != NRI_PRI_LOWEST)
    {
        return true;
    }
    if (*pLastNalHdrExt).bIdrFlag != (*pCurNalHeaderExt).bIdrFlag {
        return true;
    }
    if (*pCurNalHeaderExt).bIdrFlag {
        if (*pLastSliceHeader).uiIdrPicId != (*pCurSliceHeader).uiIdrPicId {
            return true;
        }
    }
    if !kpSps.is_null() {
        if (*kpSps).uiPocType == 0 {
            if (*pLastSliceHeader).iPicOrderCntLsb != (*pCurSliceHeader).iPicOrderCntLsb {
                return true;
            }
            if (*pLastSliceHeader).iDeltaPicOrderCntBottom != (*pCurSliceHeader).iDeltaPicOrderCntBottom {
                return true;
            }
        } else if (*kpSps).uiPocType == 1 {
            if (*pLastSliceHeader).iDeltaPicOrderCnt[0] != (*pCurSliceHeader).iDeltaPicOrderCnt[0] {
                return true;
            }
            if (*pLastSliceHeader).iDeltaPicOrderCnt[1] != (*pCurSliceHeader).iDeltaPicOrderCnt[1] {
                return true;
            }
        }
    }
    false
}

/// Evaluates whether the current NAL begins a new picture / Access Unit boundary.
pub unsafe fn CheckAccessUnitBoundary(
    pCtx: *mut SWelsDecoderContext,
    kpCurNal: *const SNalUnit,
    kpLastNal: *const SNalUnit,
    kpSps: *const SSps,
) -> bool {
    let kpLastNalHeaderExt = &(*kpLastNal).sNalHeaderExt;
    let kpCurNalHeaderExt = &(*kpCurNal).sNalHeaderExt;
    let kpLastSliceHeader = &(*kpLastNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
    let kpCurSliceHeader = &(*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;

    let dep_id = kpCurNalHeaderExt.uiDependencyId as usize;
    if dep_id < MAX_LAYER_NUM {
        let active_sps = (*pCtx).sSpsPpsCtx.pActiveLayerSps[dep_id];
        if !active_sps.is_null() && active_sps as *const _ != kpSps {
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
    if !kpSps.is_null() {
        if (*kpSps).uiPocType == 0 {
            if kpLastSliceHeader.iPicOrderCntLsb != kpCurSliceHeader.iPicOrderCntLsb {
                return true;
            }
            if kpLastSliceHeader.iDeltaPicOrderCntBottom != kpCurSliceHeader.iDeltaPicOrderCntBottom {
                return true;
            }
        } else if (*kpSps).uiPocType == 1 {
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
pub unsafe fn CheckNextAuNewSeq(
    pCtx: *mut SWelsDecoderContext,
    kpCurNal: *const SNalUnit,
    kpSps: *const SSps,
) -> bool {
    let kpCurNalHeaderExt = &(*kpCurNal).sNalHeaderExt;
    let dep_id = kpCurNalHeaderExt.uiDependencyId as usize;
    if dep_id < MAX_LAYER_NUM {
        let active_sps = (*pCtx).sSpsPpsCtx.pActiveLayerSps[dep_id];
        if !active_sps.is_null() && active_sps as *const _ != kpSps {
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
/// T3.3: `kiRbspStart` is the payload's offset into `sRawData` (was `pRbsp`); the
/// dead `pSrcNal`/`kSrcNalLen` pair — unused in this port, upstream's parse-only
/// SPS/PPS caching was never carried — is deleted (S18). The trailing-bits read is
/// an index, in bounds because the `kiSrcLen <= 0` guard has always preceded it.
pub unsafe fn ParseNonVclNal(pCtx: *mut SWelsDecoderContext, kiRbspStart: usize, kiSrcLen: i32) -> i32 {
    if kiSrcLen <= 0 {
        return ERR_NONE;
    }

    let pBs = &mut (*pCtx).sBs;
    let iBitSize = (kiSrcLen << 3)
        - BsGetTrailingBits(&(*pCtx).sRawData.bytes()[kiRbspStart + kiSrcLen as usize - 1]);
    let eNalType = (*pCtx).sCurNalHead.eNalUnitType;
    let mut iPicWidth = 0;
    let mut iPicHeight = 0;
    let mut iErr = ERR_NONE;

    match eNalType {
        EWelsNalUnitType::NAL_UNIT_SPS | EWelsNalUnitType::NAL_UNIT_SUBSET_SPS => {
            if iBitSize > 0 {
                iErr = DecInitBits(pBs, &(*pCtx).sRawData, kiRbspStart, iBitSize);
                if iErr != ERR_NONE {
                    if !(*pCtx).pParam.is_null()
                        && (*(*pCtx).pParam).eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                    {
                        (*pCtx).iErrorCode |= dsNoParamSets;
                    } else {
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    }
                    return iErr;
                }
            }
            let (buf, cursor) = pBs.split(&(*pCtx).sRawData);
            iErr = ParseSps(pCtx, buf, cursor, &mut iPicWidth, &mut iPicHeight);
            if iErr != ERR_NONE {
                if !(*pCtx).pParam.is_null()
                    && (*(*pCtx).pParam).eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
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
                    if !(*pCtx).pParam.is_null()
                        && (*(*pCtx).pParam).eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
                    {
                        (*pCtx).iErrorCode |= dsNoParamSets;
                    } else {
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    }
                    return iErr;
                }
            }
            let (buf, cursor) = pBs.split(&(*pCtx).sRawData);
            iErr = ParsePps(
                pCtx,
                (*pCtx).sSpsPpsCtx.sPpsBuffer.as_mut_ptr(),
                buf,
                cursor,
            );
            if iErr != ERR_NONE {
                if !(*pCtx).pParam.is_null()
                    && (*(*pCtx).pParam).eEcActiveIdc == crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE
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
pub unsafe fn ParseRefBasePicMarking(
    buf: &[u8],
    pBs: &mut BsCursor,
    pRefBasePicMarking: *mut SRefBasePicMarking,
) -> i32 {
    let mut uiCode: u32 = 0;
    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
        return ERR_INVALID_PARAMETERS;
    }
    let kbAdaptiveMarkingModeFlag = uiCode != 0;
    (*pRefBasePicMarking).bAdaptiveRefBasePicMarkingModeFlag = kbAdaptiveMarkingModeFlag;

    if kbAdaptiveMarkingModeFlag {
        let mut iIdx = 0;
        loop {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                return ERR_INVALID_PARAMETERS;
            }
            let kuiMmco = uiCode;
            (*pRefBasePicMarking).mmco_base[iIdx].uiMmcoType = kuiMmco;

            if kuiMmco == MMCO_END {
                break;
            }
            if kuiMmco == MMCO_SHORT2UNUSED {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                    return ERR_INVALID_PARAMETERS;
                }
                (*pRefBasePicMarking).mmco_base[iIdx].uiDiffOfPicNums = 1 + uiCode;
                (*pRefBasePicMarking).mmco_base[iIdx].iShortFrameNum = 0;
            } else if kuiMmco == MMCO_LONG2UNUSED {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
                    return ERR_INVALID_PARAMETERS;
                }
                (*pRefBasePicMarking).mmco_base[iIdx].uiLongTermPicNum = uiCode;
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
pub unsafe fn ParsePrefixNalUnit(
    pCtx: *mut SWelsDecoderContext,
    buf: &[u8],
    pBs: &mut BsCursor,
) -> i32 {
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
pub unsafe fn DecodeSpsSvcExt(
    pCtx: *mut SWelsDecoderContext,
    pSpsExt: *mut SSubsetSps,
    buf: &[u8],
    pBs: &mut BsCursor,
) -> i32 {
    let pExt = &mut (*pSpsExt).sSpsSvcExt;
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
pub unsafe fn CheckSpsActive(
    pCtx: *mut SWelsDecoderContext,
    pSps: *const SSps,
    bUseSubsetFlag: bool,
) -> bool {
    for i in 0..MAX_LAYER_NUM {
        if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] as *const _ == pSps {
            return true;
        }
    }
    if pSps.is_null() {
        return false;
    }
    let sps_id = (*pSps).iSpsId as usize;
    if sps_id >= MAX_SPS_COUNT {
        return false;
    }

    if bUseSubsetFlag {
        if (*pSps).iMbWidth > 0 && (*pSps).iMbHeight > 0 && (*pCtx).sSpsPpsCtx.bSubspsAvailFlags[sps_id] {
            if (*pCtx).iTotalNumMbRec > 0 {
                return true;
            }
            let pCurAu = (*pCtx).pAccessUnitList;
            if !pCurAu.is_null() && (*pCurAu).uiAvailUnitsNum > 0 {
                let iNum = (*pCurAu).uiAvailUnitsNum as usize;
                for i in 0..iNum {
                    let pNalUnit = *(*pCurAu).pNalUnitsList.add(i);
                    if !pNalUnit.is_null() && (*pNalUnit).sNalData.sVclNal.bSliceHeaderExtFlag {
                        let pNextUsedSps = (*pNalUnit).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
                        if !pNextUsedSps.is_null() && (*pNextUsedSps).iSpsId == (*pSps).iSpsId {
                            return true;
                        }
                    }
                }
            }
        }
    } else {
        if (*pSps).iMbWidth > 0 && (*pSps).iMbHeight > 0 && (*pCtx).sSpsPpsCtx.bSpsAvailFlags[sps_id] {
            if (*pCtx).iTotalNumMbRec > 0 {
                return true;
            }
            let pCurAu = (*pCtx).pAccessUnitList;
            if !pCurAu.is_null() && (*pCurAu).uiAvailUnitsNum > 0 {
                let iNum = (*pCurAu).uiAvailUnitsNum as usize;
                for i in 0..iNum {
                    let pNalUnit = *(*pCurAu).pNalUnitsList.add(i);
                    if !pNalUnit.is_null() && !(*pNalUnit).sNalData.sVclNal.bSliceHeaderExtFlag {
                        let pNextUsedSps = (*pNalUnit).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
                        if !pNextUsedSps.is_null() && (*pNextUsedSps).iSpsId == (*pSps).iSpsId {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Parses Sequence Parameter Sets (SPS and Subset SPS).
pub unsafe fn ParseSps(
    pCtx: *mut SWelsDecoderContext,
    buf: &[u8],
    pBsAux: &mut BsCursor,
    pPicWidth: *mut i32,
    pPicHeight: *mut i32,
) -> i32 {
    // `memset (pSubsetSps, 0, sizeof (SSubsetSps))` in au_parser.cpp. Zeroing the
    // raw bytes (rather than using Default) also clears the struct's padding, which
    // the byte-wise comparison against the stored SPS below relies on: leftover
    // padding would otherwise read as a changed SPS and force a spurious new
    // sequence, resetting the DPB mid-stream.
    let mut sTempSubsetSps: SSubsetSps = std::mem::zeroed();
    let pSubsetSps = &mut sTempSubsetSps as *mut SSubsetSps;
    let pSps = unsafe { &mut (*pSubsetSps).sSps };

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
        return ERR_INVALID_PARAMETERS;
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

    let pSLevelLimits = match GetLevelLimits(uiLevelIdc as i32, bConstraintSetFlags[3]) {
        Some(limits) => limits as *const SLevelLimits,
        None => return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE),
    };

    pSps.pSLevelLimits = pSLevelLimits;
    pSps.uiChromaFormatIdc = 1;
    pSps.uiChromaArrayType = 1;
    pSps.uiProfileIdc = uiProfileIdc;
    pSps.uiLevelIdc = uiLevelIdc;
    pSps.iSpsId = iSpsId;

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
        pSps.uiChromaFormatIdc = uiCode as u8;
        if pSps.uiChromaFormatIdc > 1 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSps.uiChromaArrayType = pSps.uiChromaFormatIdc;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSps.uiBitDepthLuma = 8;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_NON_BASELINE);
        }
        pSps.uiBitDepthChroma = 8;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.bQpPrimeYZeroTransfBypassFlag = uiCode != 0;

        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.bSeqScalingMatrixPresentFlag = uiCode != 0;

        if pSps.bSeqScalingMatrixPresentFlag {
            ParseScalingList(
                pSps,
                buf,
                pBsAux,
                false,
                false,
                pSps.bSeqScalingListPresentFlag.as_mut_ptr(),
                pSps.iScalingList4x4.as_mut_ptr(),
                pSps.iScalingList8x8.as_mut_ptr(),
            );
        }
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    if uiCode > SPS_LOG2_MAX_FRAME_NUM_MINUS4_MAX {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_LOG2_MAX_FRAME_NUM_MINUS4);
    }
    pSps.uiLog2MaxFrameNum = LOG2_MAX_FRAME_NUM_OFFSET + uiCode;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.uiPocType = uiCode;

    if pSps.uiPocType == 0 {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode > SPS_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4);
        }
        pSps.iLog2MaxPocLsb = LOG2_MAX_PIC_ORDER_CNT_LSB_OFFSET + uiCode as i32;
    } else if pSps.uiPocType == 1 {
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.bDeltaPicOrderAlwaysZeroFlag = uiCode != 0;

        if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pSps.iOffsetForNonRefPic = iCode;

        if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
        pSps.iOffsetForTopToBottomField = iCode;

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        if uiCode > SPS_NUM_REF_FRAMES_IN_PIC_ORDER_CNT_CYCLE_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_NUM_REF_FRAME_IN_PIC_ORDER_CNT_CYCLE);
        }
        pSps.iNumRefFramesInPocCycle = uiCode as i32;

        for i in 0..pSps.iNumRefFramesInPocCycle as usize {
            if BsGetSe(buf, pBsAux, &mut iCode) != ERR_NONE { return ERR_INVALID_PARAMETERS; }
            pSps.iOffsetForRefFrame[i] = iCode as i8;
        }
    }

    if pSps.uiPocType > 2 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_POC_TYPE);
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.iNumRefFrames = uiCode as i32;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.bGapsInFrameNumValueAllowedFlag = uiCode != 0;

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.iMbWidth = (PIC_WIDTH_IN_MBS_OFFSET + uiCode as i32) as u32;
    if pSps.iMbWidth > MAX_MB_SIZE as u32 || pSps.iMbWidth == 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_MB_SIZE);
    }

    if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.iMbHeight = (PIC_HEIGHT_IN_MAP_UNITS_OFFSET + uiCode as i32) as u32;
    if pSps.iMbHeight > MAX_MB_SIZE as u32 || pSps.iMbHeight == 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_MB_SIZE);
    }

    let uiTmp64 = pSps.iMbWidth as u64 * pSps.iMbHeight as u64;
    pSps.uiTotalMbCount = uiTmp64 as u32;

    if pSps.iNumRefFrames as u32 > SPS_MAX_NUM_REF_FRAMES_MAX_VAL {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_MAX_NUM_REF_FRAMES);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.bFrameMbsOnlyFlag = uiCode != 0;
    if !pSps.bFrameMbsOnlyFlag {
        return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_MBAFF);
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.bDirect8x8InferenceFlag = uiCode != 0;

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.bFrameCroppingFlag = uiCode != 0;

    if pSps.bFrameCroppingFlag {
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.sFrameCrop.iLeftOffset = uiCode as i32;
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.sFrameCrop.iRightOffset = uiCode as i32;
        if (pSps.sFrameCrop.iLeftOffset + pSps.sFrameCrop.iRightOffset) > (pSps.iMbWidth as i32 * 16 / 2) {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CROPPING_DATA);
        }

        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.sFrameCrop.iTopOffset = uiCode as i32;
        if BsGetUe(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        pSps.sFrameCrop.iBottomOffset = uiCode as i32;
        if (pSps.sFrameCrop.iTopOffset + pSps.sFrameCrop.iBottomOffset) > (pSps.iMbHeight as i32 * 16 / 2) {
            return GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_INVALID_CROPPING_DATA);
        }
    }

    if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
    pSps.bVuiParamPresentFlag = uiCode != 0;
    if pSps.bVuiParamPresentFlag {
        let iRetVui = ParseVui(pCtx, pSps, buf, pBsAux);
        if iRetVui != ERR_NONE {
            if kbUseSubsetFlag && iRetVui == GENERATE_ERROR_NO(ERR_LEVEL_PARAM_SETS, ERR_INFO_UNSUPPORTED_VUI_HRD) {
                return iRetVui;
            }
        }
    }

    if kbUseSubsetFlag && (uiProfileIdc == PRO_SCALABLE_BASELINE || uiProfileIdc == PRO_SCALABLE_HIGH) {
        let iRet = DecodeSpsSvcExt(pCtx, pSubsetSps, buf, pBsAux);
        if iRet != ERR_NONE {
            return iRet;
        }
        if BsGetOneBit(buf, pBsAux, &mut uiCode) != ERR_NONE as u32 { return ERR_INVALID_PARAMETERS; }
        (*pSubsetSps).bSvcVuiParamPresentFlag = uiCode != 0;
    }

    *pPicWidth = (pSps.iMbWidth << 4) as i32;
    *pPicHeight = (pSps.iMbHeight << 4) as i32;

    let idx = iSpsId as usize;
    if kbUseSubsetFlag {
        let pTmpSps = &(*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx].sSps;
        if CheckSpsActive(pCtx, pTmpSps, true) {
            // Overwriting the active subset SPS: only act when it actually changed.
            if !bytes_equal(&(*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx], pSubsetSps) {
                if !(*pCtx).pAccessUnitList.is_null() && (*(*pCtx).pAccessUnitList).uiAvailUnitsNum > 0 {
                    std::ptr::copy_nonoverlapping(
                        pSubsetSps,
                        &mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT],
                        1,
                    );
                    (*pCtx).bAuReadyFlag = true;
                    (*(*pCtx).pAccessUnitList).uiEndPos = (*(*pCtx).pAccessUnitList).uiAvailUnitsNum - 1;
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SUBSETSPS;
                } else if !(*pCtx).pSps.is_null() && (*(*pCtx).pSps).iSpsId == (*pSubsetSps).sSps.iSpsId {
                    std::ptr::copy_nonoverlapping(
                        pSubsetSps,
                        &mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT],
                        1,
                    );
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SUBSETSPS;
                } else {
                    std::ptr::copy_nonoverlapping(
                        pSubsetSps,
                        &mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx],
                        1,
                    );
                }
            }
        } else {
            std::ptr::copy_nonoverlapping(
                pSubsetSps,
                &mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[idx],
                1,
            );
            (*pCtx).sSpsPpsCtx.bSubspsAvailFlags[idx] = true;
            (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag = true;
        }
    } else {
        let pTmpSps = &(*pCtx).sSpsPpsCtx.sSpsBuffer[idx];
        if CheckSpsActive(pCtx, pTmpSps, false) {
            // Overwriting the active SPS: only act when it actually changed.
            if !bytes_equal(&(*pCtx).sSpsPpsCtx.sSpsBuffer[idx], pSps) {
                if !(*pCtx).pAccessUnitList.is_null() && (*(*pCtx).pAccessUnitList).uiAvailUnitsNum > 0 {
                    std::ptr::copy_nonoverlapping(
                        pSps,
                        &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT],
                        1,
                    );
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SPS;
                    (*pCtx).bAuReadyFlag = true;
                    (*(*pCtx).pAccessUnitList).uiEndPos = (*(*pCtx).pAccessUnitList).uiAvailUnitsNum - 1;
                } else if !(*pCtx).pSps.is_null() && (*(*pCtx).pSps).iSpsId == (*pSps).iSpsId {
                    std::ptr::copy_nonoverlapping(
                        pSps,
                        &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT],
                        1,
                    );
                    (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_SPS;
                } else {
                    std::ptr::copy_nonoverlapping(pSps, &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[idx], 1);
                }
            }
        } else {
            std::ptr::copy_nonoverlapping(
                pSps,
                &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[idx],
                1,
            );
            (*pCtx).sSpsPpsCtx.bSpsAvailFlags[idx] = true;
            (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag = true;
        }
    }

    ERR_NONE
}

/// Parses Picture Parameter Sets (PPS).
pub unsafe fn ParsePps(
    pCtx: *mut SWelsDecoderContext,
    pPpsList: *mut SPps,
    buf: &[u8],
    pBsAux: &mut BsCursor,
) -> i32 {
    // `memset (pPps, 0, sizeof (SPps))` in au_parser.cpp; zeroing the raw bytes also
    // clears padding, which the byte-wise comparison against the active PPS relies on.
    let mut sTempPps: SPps = std::mem::zeroed();
    let pPps = &mut sTempPps;

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
                ParseScalingList(
                    &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[pPps.iSpsId as usize],
                    buf,
                    pBsAux,
                    true,
                    pPps.bTransform8x8ModeFlag,
                    pPps.bPicScalingListPresentFlag.as_mut_ptr(),
                    pPps.iScalingList4x4.as_mut_ptr(),
                    pPps.iScalingList8x8.as_mut_ptr(),
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
    if !(*pCtx).pPps.is_null() && (*(*pCtx).pPps).iPpsId == pPps.iPpsId {
        // Re-sent PPS for the active id: only flag an overwrite when it changed.
        if !bytes_equal((*pCtx).pPps as *const SPps, pPps) {
            std::ptr::copy_nonoverlapping(pPps, &mut (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT], 1);
            (*pCtx).sSpsPpsCtx.iOverwriteFlags |= OVERWRITE_PPS;
            if !(*pCtx).pAccessUnitList.is_null() && (*(*pCtx).pAccessUnitList).uiAvailUnitsNum > 0 {
                (*pCtx).bAuReadyFlag = true;
                (*(*pCtx).pAccessUnitList).uiEndPos = (*(*pCtx).pAccessUnitList).uiAvailUnitsNum - 1;
            }
        }
    } else {
        std::ptr::copy_nonoverlapping(pPps, &mut (*pCtx).sSpsPpsCtx.sPpsBuffer[pps_idx], 1);
        (*pCtx).sSpsPpsCtx.bPpsAvailFlags[pps_idx] = true;
    }

    ERR_NONE
}

/// Parses Video Usability Information (VUI) parameters inside an SPS.
pub unsafe fn ParseVui(
    pCtx: *mut SWelsDecoderContext,
    pSps: *mut SSps,
    buf: &[u8],
    pBsAux: &mut BsCursor,
) -> i32 {
    let mut uiCode: u32 = 0;
    let pVui = &mut (*pSps).sVui;

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
pub unsafe fn ParseSei(_pSei: *mut c_void, _pBsAux: &mut BsCursor) -> i32 {
    ERR_NONE
}

/// Decodes frequency scaling matrix values from signed delta codes.
pub unsafe fn SetScalingListValue(
    pScalingList: *mut u8,
    iScalingListNum: i32,
    bUseDefaultScalingMatrixFlag: *mut bool,
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
        *pScalingList.add(iIdx) = val;
        iLastScale = val as i32;
    }

    ERR_NONE
}

/// Parses 4x4 and 8x8 frequency scaling list matrices.
pub unsafe fn ParseScalingList(
    pSps: *mut SSps,
    buf: &[u8],
    pBs: &mut BsCursor,
    bPPS: bool,
    kbTrans8x8ModeFlag: bool,
    pScalingListPresentFlag: *mut bool,
    iScalingList4x4: *mut [u8; 16],
    iScalingList8x8: *mut [u8; 64],
) -> i32 {
    let mut uiCode: u32 = 0;
    let mut bUseDefaultScalingMatrixFlag4x4 = false;
    let mut bUseDefaultScalingMatrixFlag8x8 = false;

    let uiScalingListNum = if !bPPS {
        if (*pSps).uiChromaFormatIdc != 3 { 8 } else { 12 }
    } else {
        6 + (kbTrans8x8ModeFlag as usize) * if (*pSps).uiChromaFormatIdc != 3 { 2 } else { 6 }
    };

    let bInit = if bPPS { (*pSps).bSeqScalingMatrixPresentFlag } else { false };

    let defaultScaling4x4_0 = if bInit {
        (*pSps).iScalingList4x4[0].as_ptr()
    } else {
        g_kuiDequantScaling4x4Default[0].as_ptr()
    };
    let defaultScaling4x4_1 = if bInit {
        (*pSps).iScalingList4x4[3].as_ptr()
    } else {
        g_kuiDequantScaling4x4Default[1].as_ptr()
    };
    let defaultScaling8x8_0 = if bInit {
        (*pSps).iScalingList8x8[0].as_ptr()
    } else {
        g_kuiDequantScaling8x8Default[0].as_ptr()
    };
    let defaultScaling8x8_1 = if bInit {
        (*pSps).iScalingList8x8[1].as_ptr()
    } else {
        g_kuiDequantScaling8x8Default[1].as_ptr()
    };

    for i in 0..uiScalingListNum {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE as u32 {
            return ERR_INVALID_PARAMETERS;
        }
        *pScalingListPresentFlag.add(i) = uiCode != 0;

        if uiCode != 0 {
            if i < 6 {
                SetScalingListValue(
                    (*iScalingList4x4.add(i)).as_mut_ptr(),
                    16,
                    &mut bUseDefaultScalingMatrixFlag4x4,
                    buf,
                    pBs,
                );
                if bUseDefaultScalingMatrixFlag4x4 {
                    bUseDefaultScalingMatrixFlag4x4 = false;
                    let src = g_kuiDequantScaling4x4Default[i / 3].as_ptr();
                    std::ptr::copy_nonoverlapping(src, (*iScalingList4x4.add(i)).as_mut_ptr(), 16);
                }
            } else {
                SetScalingListValue(
                    (*iScalingList8x8.add(i - 6)).as_mut_ptr(),
                    64,
                    &mut bUseDefaultScalingMatrixFlag8x8,
                    buf,
                    pBs,
                );
                if bUseDefaultScalingMatrixFlag8x8 {
                    bUseDefaultScalingMatrixFlag8x8 = false;
                    let src = g_kuiDequantScaling8x8Default[(i - 6) & 1].as_ptr();
                    std::ptr::copy_nonoverlapping(src, (*iScalingList8x8.add(i - 6)).as_mut_ptr(), 64);
                }
            }
        } else {
            if i < 6 {
                if i != 0 && i != 3 {
                    std::ptr::copy_nonoverlapping(
                        (*iScalingList4x4.add(i - 1)).as_ptr(),
                        (*iScalingList4x4.add(i)).as_mut_ptr(),
                        16,
                    );
                } else {
                    let src = if i / 3 == 0 { defaultScaling4x4_0 } else { defaultScaling4x4_1 };
                    std::ptr::copy_nonoverlapping(src, (*iScalingList4x4.add(i)).as_mut_ptr(), 16);
                }
            } else {
                if i == 6 || i == 7 {
                    let src = if ((i & 1) + 2) == 2 { defaultScaling8x8_0 } else { defaultScaling8x8_1 };
                    std::ptr::copy_nonoverlapping(src, (*iScalingList8x8.add(i - 6)).as_mut_ptr(), 64);
                } else {
                    std::ptr::copy_nonoverlapping(
                        (*iScalingList8x8.add(i - 8)).as_ptr(),
                        (*iScalingList8x8.add(i - 6)).as_mut_ptr(),
                        64,
                    );
                }
            }
        }
    }

    ERR_NONE
}

/// Resets FMO contexts and returns count of active FMO units.
pub unsafe fn ResetFmoList(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return 0;
    }
    let iCountNum = (*pCtx).iActiveFmoNum;
    (*pCtx).iActiveFmoNum = 0;
    iCountNum
}

// ============================================================================
// Access Unit List Dynamic Memory Management
// ============================================================================

/// Allocates a single contiguous memory buffer holding [`SAccessUnit`],
/// the NAL unit pointer array, and all [`SNalUnit`] storage nodes.
pub unsafe fn MemInitNalList(
    ppAu: *mut *mut SAccessUnit,
    kuiSize: u32,
    _pMa: *mut CMemoryAlign,
) -> i32 {
    if kuiSize == 0 {
        return ERR_INVALID_PARAMETERS;
    }

    let kuiSizeAu = std::mem::size_of::<SAccessUnit>();
    let kuiSizeNalUnitPtr = kuiSize as usize * std::mem::size_of::<*mut SNalUnit>();
    let kuiSizeNalUnit = std::mem::size_of::<SNalUnit>();
    let kuiCountSize = kuiSizeAu + kuiSizeNalUnitPtr + kuiSize as usize * kuiSizeNalUnit;

    let layout = match std::alloc::Layout::from_size_align(kuiCountSize, 16) {
        Ok(l) => l,
        Err(_) => return ERR_INFO_OUT_OF_MEMORY,
    };

    let pBase = std::alloc::alloc_zeroed(layout);
    if pBase.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }

    *ppAu = pBase as *mut SAccessUnit;
    let mut pPtr = pBase.add(kuiSizeAu);
    (**ppAu).pNalUnitsList = pPtr as *mut *mut SNalUnit;
    pPtr = pPtr.add(kuiSizeNalUnitPtr);

    for uiIdx in 0..kuiSize as usize {
        *(**ppAu).pNalUnitsList.add(uiIdx) = pPtr as *mut SNalUnit;
        pPtr = pPtr.add(kuiSizeNalUnit);
    }

    (**ppAu).uiCountUnitsNum = kuiSize;
    (**ppAu).uiAvailUnitsNum = 0;
    (**ppAu).uiActualUnitsNum = 0;
    (**ppAu).uiStartPos = 0;
    (**ppAu).uiEndPos = 0;
    (**ppAu).bCompletedAuFlag = false;

    ERR_NONE
}

/// Frees the contiguous memory buffer allocated for an [`SAccessUnit`].
pub unsafe fn MemFreeNalList(ppAu: *mut *mut SAccessUnit, _pMa: *mut CMemoryAlign) -> i32 {
    if !ppAu.is_null() {
        let pAu = *ppAu;
        if !pAu.is_null() {
            let kuiSize = (*pAu).uiCountUnitsNum;
            let kuiSizeAu = std::mem::size_of::<SAccessUnit>();
            let kuiSizeNalUnitPtr = kuiSize as usize * std::mem::size_of::<*mut SNalUnit>();
            let kuiSizeNalUnit = std::mem::size_of::<SNalUnit>();
            let kuiCountSize = kuiSizeAu + kuiSizeNalUnitPtr + kuiSize as usize * kuiSizeNalUnit;

            if let Ok(layout) = std::alloc::Layout::from_size_align(kuiCountSize, 16) {
                std::alloc::dealloc(pAu as *mut u8, layout);
            }
            *ppAu = std::ptr::null_mut();
        }
    }
    ERR_NONE
}

/// Expands the capacity of an [`SAccessUnit`] list by reallocating the contiguous buffer.
pub unsafe fn ExpandNalUnitList(
    ppAu: *mut *mut SAccessUnit,
    kiOrgSize: i32,
    kiExpSize: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    if kiExpSize <= kiOrgSize {
        return ERR_INVALID_PARAMETERS;
    }

    let mut pTmp: *mut SAccessUnit = std::ptr::null_mut();
    let iRet = MemInitNalList(&mut pTmp, kiExpSize as u32, pMa);
    if iRet != ERR_NONE {
        return iRet;
    }

    for iIdx in 0..kiOrgSize as usize {
        let src = *(*(*ppAu)).pNalUnitsList.add(iIdx);
        let dst = *(*pTmp).pNalUnitsList.add(iIdx);
        std::ptr::copy_nonoverlapping(src, dst, 1);
    }

    (*pTmp).uiCountUnitsNum = kiExpSize as u32;
    (*pTmp).uiAvailUnitsNum = (*(*ppAu)).uiAvailUnitsNum;
    (*pTmp).uiActualUnitsNum = (*(*ppAu)).uiActualUnitsNum;
    (*pTmp).uiEndPos = (*(*ppAu)).uiEndPos;
    (*pTmp).bCompletedAuFlag = (*(*ppAu)).bCompletedAuFlag;

    MemFreeNalList(ppAu, pMa);
    *ppAu = pTmp;

    ERR_NONE
}

/// Retrieves the next available [`SNalUnit`] node from the AU list, expanding capacity if needed.
pub unsafe fn MemGetNextNal(
    ppAu: *mut *mut SAccessUnit,
    pMa: *mut CMemoryAlign,
) -> *mut SNalUnit {
    let mut pAu = *ppAu;

    if (*pAu).uiAvailUnitsNum >= (*pAu).uiCountUnitsNum {
        let kuiExpandingSize = (*pAu).uiCountUnitsNum + (MAX_NAL_UNIT_NUM_IN_AU as u32 >> 1);
        if ExpandNalUnitList(ppAu, (*pAu).uiCountUnitsNum as i32, kuiExpandingSize as i32, pMa) != ERR_NONE {
            return std::ptr::null_mut();
        }
        pAu = *ppAu;
    }

    let idx = (*pAu).uiAvailUnitsNum as usize;
    (*pAu).uiAvailUnitsNum += 1;
    let pNu = *(*pAu).pNalUnitsList.add(idx);

    std::ptr::write_bytes(pNu, 0, 1);
    pNu
}

/// Clears the most recently added corrupted NAL unit from the AU list.
pub unsafe fn ForceClearCurrentNal(pAu: *mut SAccessUnit) {
    if !pAu.is_null() && (*pAu).uiAvailUnitsNum > 0 {
        (*pAu).uiAvailUnitsNum -= 1;
    }
}

/// Prefetches and synchronizes prefix NAL header extension parameters into slice headers.
pub unsafe fn PrefetchNalHeaderExtSyntax(
    pCtx: *mut SWelsDecoderContext,
    kppDst: *mut SNalUnit,
    kpSrc: *mut SNalUnit,
) -> bool {
    if kppDst.is_null() || kpSrc.is_null() {
        return false;
    }

    let pNalHdrExtD = &mut (*kppDst).sNalHeaderExt;
    let pNalHdrExtS = &(*kpSrc).sNalHeaderExt;

    pNalHdrExtD.uiDependencyId = pNalHdrExtS.uiDependencyId;
    pNalHdrExtD.uiQualityId = pNalHdrExtS.uiQualityId;
    pNalHdrExtD.uiTemporalId = pNalHdrExtS.uiTemporalId;
    pNalHdrExtD.uiPriorityId = pNalHdrExtS.uiPriorityId;
    pNalHdrExtD.bIdrFlag = pNalHdrExtS.bIdrFlag;
    pNalHdrExtD.bNoInterLayerPredFlag = pNalHdrExtS.bNoInterLayerPredFlag;

    true
}

/// Resets active SPS pointers for each layer if MB reconstruction has not started.
pub unsafe fn ResetActiveSPSForEachLayer(pCtx: *mut SWelsDecoderContext) {
    if !pCtx.is_null() && (*pCtx).iTotalNumMbRec == 0 {
        for i in 0..MAX_LAYER_NUM {
            (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = std::ptr::null_mut();
        }
    }
}
