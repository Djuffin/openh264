#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_mut
)]

#![deny(unsafe_code)]
// **Phase 5b, T5b.6: this file's `unsafe` is gone and no exception is enumerated.**
// `src/decoder/` carries **three** `#[allow(unsafe_code)]` items in total, and they
// are all in `decoder_context.rs` (`api_alias`/`api_alias_mut`) and `picture.rs` (the
// one Miri provenance test S28 mandates for `data_ptr` — T5b.7 retired the second
// with `data_ptr_ref`). Nothing here is one of them.

use crate::decoder::decoder_context::{
    PicRefs, SRefPic, SliceCtx, SpsRef, active_fmo, active_pps, active_sps, cur_au, pps_of,
    ref_id, slice_split, sps_of,
};
use crate::decoder::pic_queue::RefSlot;
use crate::safe::bits::BsCursor;
use crate::decoder::bit_stream::BsReader;
use std::ffi::c_void;

// ============================================================================
// Constants & Error Codes
// ============================================================================

pub const ERR_NONE: i32 = 0;
pub use crate::decoder::decoder_core::ERR_INFO_REF_COUNT_OVERFLOW;
pub const ERR_INVALID_PARAMETERS: i32 = 1;
pub const ERR_MALLOC_FAILED: i32 = 2;
pub const ERR_API_FAILED: i32 = 3;

pub const ERR_LEVEL_ACCESS_UNIT: i32 = 1;
pub const ERR_LEVEL_NAL_UNIT_HEADER: i32 = 2;
pub const ERR_LEVEL_PREFIX_NAL: i32 = 3;
pub const ERR_LEVEL_PARAM_SETS: i32 = 4;
pub const ERR_LEVEL_SLICE_HEADER: i32 = 5;
pub const ERR_LEVEL_SLICE_DATA: i32 = 6;
pub const ERR_LEVEL_MB_DATA: i32 = 7;

pub const ERR_INFO_COMMON_BASE: i32 = 1;
pub const ERR_INFO_SYNTAX_BASE: i32 = 1001;
pub const ERR_INFO_LOGIC_BASE: i32 = 10001;

pub const ERR_INVALID_INTRA4X4_MODE: i32 = -1;

pub const ERR_INFO_INVALID_QP: i32 = ERR_INFO_SYNTAX_BASE + 24;
pub const ERR_INFO_INVALID_MB_TYPE: i32 = ERR_INFO_SYNTAX_BASE + 32;
pub const ERR_INFO_INVALID_MB_SKIP_RUN: i32 = ERR_INFO_SYNTAX_BASE + 33;
pub const ERR_INFO_INVALID_CBP: i32 = ERR_INFO_SYNTAX_BASE + 40;
pub const ERR_INFO_INVALID_I4x4_PRED_MODE: i32 = ERR_INFO_SYNTAX_BASE + 49;
pub const ERR_INFO_INVALID_I16x16_PRED_MODE: i32 = ERR_INFO_SYNTAX_BASE + 50;
pub const ERR_INFO_INVALID_I_CHROMA_PRED_MODE: i32 = ERR_INFO_SYNTAX_BASE + 51;
pub const ERR_INFO_UNSUPPORTED_ILP: i32 = ERR_INFO_SYNTAX_BASE + 63;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = ERR_INFO_SYNTAX_BASE + 74;

pub const ERR_INFO_WIDTH_MISMATCH: i32 = ERR_INFO_LOGIC_BASE + 5;
pub const ERR_INFO_MB_RECON_FAIL: i32 = ERR_INFO_LOGIC_BASE + 7;
pub const ERR_INFO_MB_NUM_EXCEED_FAIL: i32 = ERR_INFO_LOGIC_BASE + 8;
pub const ERR_INFO_BS_INCOMPLETE: i32 = ERR_INFO_LOGIC_BASE + 9;

pub const dsBitstreamError: i32 = 0x04;

pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;
pub const WELS_LOG_DEBUG: i32 = 4;

#[inline(always)]
pub fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

#[inline(always)]
pub fn WELS_CLIP3(x: i32, min_val: i32, max_val: i32) -> i32 {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline(always)]
pub fn WELS_MAX(x: i32, y: i32) -> i32 {
    if x > y { x } else { y }
}

#[inline(always)]
pub fn WELS_MIN(x: i32, y: i32) -> i32 {
    if x < y { x } else { y }
}

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;
pub const SP_SLICE: i32 = 3;
pub const SI_SLICE: i32 = 4;
pub const UNKNOWN_SLICE: i32 = 5;

// Reference List Indices
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MV_A: usize = 2;

// Macroblock Types
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000080;
pub const MB_TYPE_SKIP: u32 = 0x00000100;
pub const MB_TYPE_INTRA_PCM: u32 = 0x00000200;
pub const MB_TYPE_INTRA_BL: u32 = 0x00000400;
pub const MB_TYPE_DIRECT: u32 = 0x00000800;
pub const MB_TYPE_P0L0: u32 = 0x00001000;
pub const MB_TYPE_P1L0: u32 = 0x00002000;
pub const MB_TYPE_P0L1: u32 = 0x00004000;
pub const MB_TYPE_P1L1: u32 = 0x00008000;
pub const MB_TYPE_L0: u32 = MB_TYPE_P0L0 | MB_TYPE_P1L0;
pub const MB_TYPE_L1: u32 = MB_TYPE_P0L1 | MB_TYPE_P1L1;

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

pub const MB_TYPE_INTRA: u32 = MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;
pub const MB_TYPE_INTER: u32 = MB_TYPE_16x16 | MB_TYPE_16x8 | MB_TYPE_8x16 | MB_TYPE_8x8 | MB_TYPE_8x8_REF0 | MB_TYPE_SKIP | MB_TYPE_DIRECT;

#[inline(always)]
pub fn IS_INTRA4x4(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA4x4
}

#[inline(always)]
pub fn IS_INTRA8x8(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA8x8
}

#[inline(always)]
pub fn IS_INTRANxN(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA4x4 || mb_type == MB_TYPE_INTRA8x8
}

#[inline(always)]
pub fn IS_INTRA16x16(mb_type: u32) -> bool {
    mb_type == MB_TYPE_INTRA16x16
}

#[inline(always)]
pub fn IS_INTRA(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA) != 0
}

#[inline(always)]
pub fn IS_INTER(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTER) != 0
}

#[inline(always)]
pub fn IS_INTER_16x16(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_16x16) != 0
}

#[inline(always)]
pub fn IS_INTER_16x8(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_16x8) != 0
}

#[inline(always)]
pub fn IS_INTER_8x16(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_8x16) != 0
}

#[inline(always)]
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_DIRECT(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_DIRECT) != 0
}

#[inline(always)]
pub fn IS_Inter_8x8(mb_type: u32) -> bool {
    (mb_type & (MB_TYPE_8x8 | MB_TYPE_8x8_REF0)) != 0
}

#[inline(always)]
pub fn IS_TYPE_L0(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_L0) != 0
}

#[inline(always)]
pub fn IS_TYPE_L1(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_L1) != 0
}

#[inline(always)]
pub fn IS_DIR(a: u32, part: usize, list: usize) -> bool {
    (a & (MB_TYPE_P0L0 << (part + 2 * list))) != 0
}

#[inline(always)]
pub fn IS_SUB_8x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x8) != 0
}

#[inline(always)]
pub fn IS_SUB_8x4(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x4) != 0
}

#[inline(always)]
pub fn IS_SUB_4x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_4x8) != 0
}

#[inline(always)]
pub fn IS_SUB_4x4(sub_type: u32) -> bool {
    {
        (sub_type & SUB_MB_TYPE_4x4) != 0
    }
}

// Residual Properties
pub const I16_LUMA_DC: i32 = 1;
pub const I16_LUMA_AC: i32 = 2;
pub const LUMA_DC_AC: i32 = 3;
pub const CHROMA_DC: i32 = 4;
pub const CHROMA_AC: i32 = 5;
pub const LUMA_DC_AC_8: i32 = 6;
pub const CHROMA_DC_U: i32 = 7;
pub const CHROMA_DC_V: i32 = 8;
pub const CHROMA_AC_U: i32 = 9;
pub const CHROMA_AC_V: i32 = 10;
pub const LUMA_DC_AC_INTRA: i32 = 11;
pub const LUMA_DC_AC_INTER: i32 = 12;
pub const CHROMA_DC_U_INTER: i32 = 13;
pub const CHROMA_DC_V_INTER: i32 = 14;
pub const CHROMA_AC_U_INTER: i32 = 15;
pub const CHROMA_AC_V_INTER: i32 = 16;
pub const LUMA_DC_AC_INTRA_8: i32 = 17;
pub const LUMA_DC_AC_INTER_8: i32 = 18;

pub const C_PRED_DC: u8 = 0;
pub const MAX_PRED_MODE_ID_CHROMA: i32 = 3;

// CPU Flags

// ============================================================================
// Lookup Tables
// ============================================================================

pub static g_kuiScan4: [u8; 16] = [
    0, 1, 4, 5,
    2, 3, 6, 7,
    8, 9, 12, 13,
    10, 11, 14, 15,
];

pub static g_kuiScan8: [u8; 24] = [
    9, 10, 17, 18,
    11, 12, 19, 20,
    25, 26, 33, 34,
    27, 28, 35, 36,
    14, 15,
    22, 23,
    38, 39,
    46, 47,
];

pub static g_kCacheNzcScanIdx: [u8; 27] = [
    9, 10, 17, 18,
    11, 12, 19, 20,
    25, 26, 33, 34,
    27, 28, 35, 36,
    14, 15,
    22, 23,
    38, 39,
    46, 47,
    41,
    42, 43,
];

pub static g_kCache30ScanIdx: [u8; 16] = [
    7, 8, 13, 14,
    9, 10, 15, 16,
    19, 20, 25, 26,
    21, 22, 27, 28,
];

pub static g_kuiCache30ScanIdx: [u8; 16] = [
    7, 8, 13, 14,
    9, 10, 15, 16,
    19, 20, 25, 26,
    21, 22, 27, 28,
];

// `common_tables.cpp:49` declares this `[24]`, not `[16]`: the eight chroma
// entries were missing from this module's copy. Nothing here indexes past 15, so
// the truncation was latent -- the same shape as `g_kuiGolombUELength` in Phase
// 4.6, where a short copy did index out of bounds.
pub static g_kuiCache48CountScan4Idx: [u8; 24] = [
    /* Luma */
    9, 10, 17, 18,
    11, 12, 19, 20,
    25, 26, 33, 34,
    27, 28, 35, 36,
    /* Cb */
    14, 15,
    22, 23,
    /* Cr */
    38, 39,
    46, 47,
];

// `wels_common_defs.h:64` declares this `[24]`, not `[16]`: the eight chroma
// entries were missing here. Only indices below 16 are read in this module.
pub static g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5,
    2, 3, 6, 7,
    8, 9, 12, 13,
    10, 11, 14, 15,
    16, 17, 20, 21,
    18, 19, 22, 23,
];

pub static g_kuiZigzagScan: [u8; 16] = [
    0, 1, 4, 8,
    5, 2, 3, 6,
    9, 12, 13, 10,
    7, 11, 14, 15,
];

pub static g_kuiZigzagScan8x8: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10,
    17, 24, 32, 25, 18, 11, 4, 5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

pub static g_kuiLumaDcZigzagScan: [u8; 16] = [
    0, 16, 32, 128,
    48, 64, 80, 96,
    144, 160, 176, 192,
    112, 208, 224, 240,
];

pub static g_kuiChromaDcScan: [u8; 4] = [
    0, 16, 32, 48,
];

pub static g_kuiI16CbpTable: [u8; 6] = [0, 16, 32, 15, 31, 47];

pub static g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37,
    37, 38, 38, 38, 39, 39, 39, 39,
];

pub static g_kuiDequantCoeff: [[u16; 8]; 52] = [
    [10, 13, 10, 13, 13, 16, 13, 16], [11, 14, 11, 14, 14, 18, 14, 18],
    [13, 16, 13, 16, 16, 20, 16, 20], [14, 18, 14, 18, 18, 23, 18, 23],
    [16, 20, 16, 20, 20, 25, 20, 25], [18, 23, 18, 23, 23, 29, 23, 29],
    [20, 26, 20, 26, 26, 32, 26, 32], [22, 28, 22, 28, 28, 36, 28, 36],
    [26, 32, 26, 32, 32, 40, 32, 40], [28, 36, 28, 36, 36, 46, 36, 46],
    [32, 40, 32, 40, 40, 50, 40, 50], [36, 46, 36, 46, 46, 58, 46, 58],
    [40, 52, 40, 52, 52, 64, 52, 64], [44, 56, 44, 56, 56, 72, 56, 72],
    [52, 64, 52, 64, 64, 80, 64, 80], [56, 72, 56, 72, 72, 92, 72, 92],
    [64, 80, 64, 80, 80, 100, 80, 100], [72, 92, 72, 92, 92, 116, 92, 116],
    [80, 104, 80, 104, 104, 128, 104, 128], [88, 112, 88, 112, 112, 144, 112, 144],
    [104, 128, 104, 128, 128, 160, 128, 160], [112, 144, 112, 144, 144, 184, 144, 184],
    [128, 160, 128, 160, 160, 200, 160, 200], [144, 184, 144, 184, 184, 232, 184, 232],
    [160, 208, 160, 208, 208, 256, 208, 256], [176, 224, 176, 224, 224, 288, 224, 288],
    [208, 256, 208, 256, 256, 320, 256, 320], [224, 288, 224, 288, 288, 368, 288, 368],
    [256, 320, 256, 320, 320, 400, 320, 400], [288, 368, 288, 368, 368, 464, 368, 464],
    [320, 416, 320, 416, 416, 512, 416, 512], [352, 448, 352, 448, 448, 576, 448, 576],
    [416, 512, 416, 512, 512, 640, 512, 640], [448, 576, 448, 576, 576, 736, 576, 736],
    [512, 640, 512, 640, 640, 800, 640, 800], [576, 736, 576, 736, 736, 928, 736, 928],
    [640, 832, 640, 832, 832, 1024, 832, 1024], [704, 896, 704, 896, 896, 1152, 896, 1152],
    [832, 1024, 832, 1024, 1024, 1280, 1024, 1280], [896, 1152, 896, 1152, 1152, 1472, 1152, 1472],
    [1024, 1280, 1024, 1280, 1280, 1600, 1280, 1600], [1152, 1472, 1152, 1472, 1472, 1856, 1472, 1856],
    [1280, 1664, 1280, 1664, 1664, 2048, 1664, 2048], [1408, 1792, 1408, 1792, 1792, 2304, 1792, 2304],
    [1664, 2048, 1664, 2048, 2048, 2560, 2048, 2560], [1792, 2304, 1792, 2304, 2304, 2944, 2304, 2944],
    [2048, 2560, 2048, 2560, 2560, 3200, 2560, 3200], [2304, 2944, 2304, 2944, 2944, 3712, 2944, 3712],
    [2560, 3328, 2560, 3328, 3328, 4096, 3328, 4096], [2816, 3584, 2816, 3584, 3584, 4608, 3584, 4608],
    [3328, 4096, 3328, 4096, 4096, 5120, 4096, 5120], [3584, 4608, 3584, 4608, 4608, 5888, 4608, 5888],
];

pub static g_kuiMatrixV: [[[u8; 8]; 8]; 6] = [
    [
        [20, 19, 25, 19, 20, 19, 25, 19],
        [19, 18, 24, 18, 19, 18, 24, 18],
        [25, 24, 32, 24, 25, 24, 32, 24],
        [19, 18, 24, 18, 19, 18, 24, 18],
        [20, 19, 25, 19, 20, 19, 25, 19],
        [19, 18, 24, 18, 19, 18, 24, 18],
        [25, 24, 32, 24, 25, 24, 32, 24],
        [19, 18, 24, 18, 19, 18, 24, 18],
    ],
    [
        [22, 21, 28, 21, 22, 21, 28, 21],
        [21, 19, 26, 19, 21, 19, 26, 19],
        [28, 26, 35, 26, 28, 26, 35, 26],
        [21, 19, 26, 19, 21, 19, 26, 19],
        [22, 21, 28, 21, 22, 21, 28, 21],
        [21, 19, 26, 19, 21, 19, 26, 19],
        [28, 26, 35, 26, 28, 26, 35, 26],
        [21, 19, 26, 19, 21, 19, 26, 19],
    ],
    [
        [26, 24, 33, 24, 26, 24, 33, 24],
        [24, 23, 31, 23, 24, 23, 31, 23],
        [33, 31, 42, 31, 33, 31, 42, 31],
        [24, 23, 31, 23, 24, 23, 31, 23],
        [26, 24, 33, 24, 26, 24, 33, 24],
        [24, 23, 31, 23, 24, 23, 31, 23],
        [33, 31, 42, 31, 33, 31, 42, 31],
        [24, 23, 31, 23, 24, 23, 31, 23],
    ],
    [
        [28, 26, 35, 26, 28, 26, 35, 26],
        [26, 25, 33, 25, 26, 25, 33, 25],
        [35, 33, 45, 33, 35, 33, 45, 33],
        [26, 25, 33, 25, 26, 25, 33, 25],
        [28, 26, 35, 26, 28, 26, 35, 26],
        [26, 25, 33, 25, 26, 25, 33, 25],
        [35, 33, 45, 33, 35, 33, 45, 33],
        [26, 25, 33, 25, 26, 25, 33, 25],
    ],
    [
        [33, 31, 42, 31, 33, 31, 42, 31],
        [31, 29, 39, 29, 31, 29, 39, 29],
        [42, 39, 53, 39, 42, 39, 53, 39],
        [31, 29, 39, 29, 31, 29, 39, 29],
        [33, 31, 42, 31, 33, 31, 42, 31],
        [31, 29, 39, 29, 31, 29, 39, 29],
        [42, 39, 53, 39, 42, 39, 53, 39],
        [31, 29, 39, 29, 31, 29, 39, 29],
    ],
    [
        [36, 34, 45, 34, 36, 34, 45, 34],
        [34, 32, 43, 32, 34, 32, 43, 32],
        [45, 43, 58, 43, 45, 43, 58, 43],
        [34, 32, 43, 32, 34, 32, 43, 32],
        [36, 34, 45, 34, 36, 34, 45, 34],
        [34, 32, 43, 32, 34, 32, 43, 32],
        [45, 43, 58, 43, 45, 43, 58, 43],
        [34, 32, 43, 32, 34, 32, 43, 32],
    ],
];

/// `TagPartMbInfo` — `codec/decoder/core/inc/wels_common_basis.h:235`.
///
/// Single declaration for the decoder. C++ puts this type and its four tables in
/// one header; the port had transliterated them per consumer, so `mv_pred.rs`,
/// `parse_mb_syn_cabac.rs` and `parse_mb_syn_cavlc.rs` each carried a copy and
/// `mv_pred.rs`'s named the first field `iMbType` (unified at T5.A3 — the values
/// were identical, the field name was not).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPartMbInfo {
    pub iType: u32,
    pub iPartCount: i8,
    pub iPartWidth: i8,
}

pub static g_ksInterPMbTypeInfo: [SPartMbInfo; 5] = [
    SPartMbInfo { iType: MB_TYPE_16x16, iPartCount: 1, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_16x8, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_8x8, iPartCount: 4, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x8_REF0, iPartCount: 4, iPartWidth: 4 },
];

pub static g_ksInterBMbTypeInfo: [SPartMbInfo; 23] = [
    SPartMbInfo { iType: MB_TYPE_DIRECT, iPartCount: 1, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_16x16 | MB_TYPE_P0L0, iPartCount: 1, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_16x16 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_16x16 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L1 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L1 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L1 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L1 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L1 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L1 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L0, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_16x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 4 },
    SPartMbInfo { iType: MB_TYPE_8x16 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: MB_TYPE_8x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_P1L0 | MB_TYPE_P1L1, iPartCount: 4, iPartWidth: 4 },
];

/// Table 7.17 — sub-macroblock type values for P slices.
/// `codec/decoder/core/inc/wels_common_basis.h:279`.
pub static g_ksInterPSubMbTypeInfo: [SPartMbInfo; 4] = [
    SPartMbInfo { iType: SUB_MB_TYPE_8x8, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4, iPartCount: 4, iPartWidth: 1 },
];

/// Table 7.18 — sub-macroblock type values for B slices.
/// `codec/decoder/core/inc/wels_common_basis.h:287`.
pub static g_ksInterBSubMbTypeInfo: [SPartMbInfo; 13] = [
    SPartMbInfo { iType: MB_TYPE_DIRECT, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L0, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 1, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L0, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L0, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_8x4 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 2 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 2, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L0, iPartCount: 4, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L1, iPartCount: 4, iPartWidth: 1 },
    SPartMbInfo { iType: SUB_MB_TYPE_4x4 | MB_TYPE_P0L0 | MB_TYPE_P0L1, iPartCount: 4, iPartWidth: 1 },
];

// ============================================================================
// Function Pointer Types & Block Structures
// ============================================================================

/// **T5.P″3: the dispatch carries the slice's two derivations.** The per-macroblock
/// function used to reach the pool through `pCtx` at each of its levels; the slice
/// bracket resolves once and hands the result down, so the type that crosses the
/// loop is where the hoist becomes visible. `PDeblockingFilterMbFunc` gained its
/// `pDec` the same way at T5.P′1, for the same reason.
pub type PWelsDecMbFunc = fn(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32;

// T5.R7: `PFillInfoCacheIntraNxNFunc`, `PMapNxNNeighToSampleFunc` and
// `PMap16x16NeighToSampleFunc` stood here with **no uses at all** — Phase 4b turned
// their three dispatch slots into an enum method and two direct calls and left the
// typedefs behind. S18's straggler class, deleted where it was found.

// T5.X8: two more of that class, and this pair was worse than unused — they were a
// *second* declaration of `PIdctResAddPredFunc` (`decoder_context.rs` holds the one
// the tables are typed by) with a different parameter list, so a reader who found
// this one learned the wrong signature. Neither had a use in this file.

// T4b.3c: `SBlockFunc` was declared **twice** -- here and in `decoder_context.rs`,
// with the same three members and two different `PWelsNonZeroCountFunc` /
// `PWelsBlockZeroFunc` typedef pairs. `WelsInitDecoderFuncs` bridged the two
// definitions with `&mut (*pCtx).sBlockFunc as *mut _ as *mut _`, a double cast
// doing exactly what T4b.3b's pair of reinterpreting calls did: laundering one
// type into an identical one that a second declaration had made incompatible.
//
// Both are deleted. Of the three slots, **one was ever read** -- in this port and
// in the C++: `pWelsSetNonZeroCountFunc`, at `WelsMbInterConstruction` below.
// `pWelsBlockZero16x16Func` and `pWelsBlockZero8x8Func` are installed by
// `decode_slice.cpp:2992-2993` and called from nowhere in the C++ tree either, so
// they and their two `_c` kernels went with the table rather than being kept as
// dead ports of dead code.

// ============================================================================
// Core Decoder Structures
// ============================================================================

pub use crate::decoder::parse_mb_syn_cavlc::SWelsNeighAvail;

pub use crate::decoder::parameter_sets::{SSps, SPps};
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SSlice, EWelsSliceType};

pub use crate::decoder::decoder_core::{
    DqLayerState, SLayerInfo, SPredWeightTable, ERR_INFO_INVALID_PTR, ERR_INFO_INVALID_ACCESS, ERR_INFO_INVALID_PARAM,
};
pub use crate::decoder::nalu::{SNalUnit};




pub use crate::decoder::picture::SPicture;





#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SParam {
    pub bParseOnly: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpsPpsCtx {
    pub bAvcBasedFlag: bool,
}

pub use crate::decoder::decoder_context::SWelsDecoderContext;


// ============================================================================
// Core Utility & Scaling Functions
// ============================================================================

pub fn CheckRefPics(ctx: &SliceCtx<'_>) -> bool {
    let mut listCount = 1;
    if ctx.eSliceType == EWelsSliceType::B_SLICE {
        listCount += 1;
    }

    for list in 0..listCount {
        let shortRefCount = ctx.sRefPic.uiShortRefCount[list];
        let pShortList = &ctx.sRefPic.pShortRefList[list];
        for refIdx in 0..shortRefCount {
            if pShortList[refIdx as usize].is_none() {
                return false;
            }
        }
        let longRefCount = ctx.sRefPic.uiLongRefCount[list];
        let pLongList = &ctx.sRefPic.pLongRefList[list];
        for refIdx in 0..longRefCount {
            if pLongList[refIdx as usize].is_none() {
                return false;
            }
        }
    }
    true
}

pub fn ComputeColocatedTemporalScaling(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pRefs: PicRefs<'_>,
    pDec: Option<&SPicture>,
) -> bool {
    {

        if (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag == 0 {
            let uiRefCount = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount[LIST_0];
            let pic1 = pRefs.resolve(pCtx.ref_id(LIST_1, 0), pDec);
            if let Some(pic1) = pic1 {
                for i in 0..uiRefCount {
                    let pic0 = pRefs.resolve(pCtx.ref_id(LIST_0, i as usize), pDec);
                    if let Some(pic0) = pic0 {
                        let poc0 = pic0.iFramePoc;
                        let poc1 = pic1.iFramePoc;
                        let poc = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
                        let td = WELS_CLIP3(poc1 - poc0, -128, 127);
                        if td == 0 {
                            (*pCurDqLayer).sLayerInfo.sSliceInLayer.iMvScale[LIST_0][i as usize] = 1 << 8;
                        } else {
                            let tb = WELS_CLIP3(poc - poc0, -128, 127);
                            let tx = (16384 + (td.abs() >> 1)) / td;
                            (*pCurDqLayer).sLayerInfo.sSliceInLayer.iMvScale[LIST_0][i as usize] =
                                WELS_CLIP3((tb * tx + 32) >> 6, -1024, 1023) as i16;
                        }
                    }
                }
            }
        }
        true
    }
}

pub fn WelsCalcDeqCoeffScalingList(pCtx: &mut SWelsDecoderContext) -> i32 {
    // **The lists travel as values, not as borrows** (T5.Z1). The loop below writes
    // `pDequant_coeff_buffer*` through the context on every iteration, so a borrow of
    // the parameter sets held across it is the shape this face removes — and the two
    // lists are `[[i8; 16]; 6]` and `[[i8; 64]; 6]`, small enough that copying them
    // out is cheaper than re-resolving the active set 40 thousand times, which is
    // what the per-use spelling did.
    let ps = &(*pCtx).sSpsPpsCtx;
    let (Some(sps), Some(pps)) = (
        active_sps(ps, (*pCtx).active_sps),
        active_pps(ps, (*pCtx).active_pps),
    ) else {
        return ERR_NONE;
    };
    let bPicScaling = pps.bPicScalingMatrixPresentFlag;
    let bAnyScaling = sps.bSeqScalingMatrixPresentFlag || bPicScaling;
    let iPpsId = pps.iPpsId;
    let (kList4x4, kList8x8) = if bPicScaling {
        (pps.iScalingList4x4, pps.iScalingList8x8)
    } else {
        (sps.iScalingList4x4, sps.iScalingList8x8)
    };

    if bAnyScaling {
        (*pCtx).bUseScalingList = true;

        if !(*pCtx).bDequantCoeff4x4Init || (*pCtx).iDequantCoeffPpsid != iPpsId {
            for i in 0..6 {
                // T5.Y1: the two alias stores that stood here (`pDequant_coeff4x4[i]
                // = pDequant_coeff_buffer4x4[i]`'s first row, and the 8x8 twin) are
                // gone with the fields — every reader indexes the buffer by the same
                // `i` the alias was derived from.
                for q in 0..51 {
                    for x in 0..16 {
                        let scale4 = kList4x4[i][x] as u32;
                        (*pCtx).pDequant_coeff_buffer4x4[i][q][x] =
                            (scale4 * (g_kuiDequantCoeff[q][x & 0x07] as u32)) as u16;
                    }
                    for y in 0..64 {
                        let scale8 = kList8x8[i][y] as u32;
                        (*pCtx).pDequant_coeff_buffer8x8[i][q][y] =
                            (scale8 * (g_kuiMatrixV[q % 6][y / 8][y % 8] as u32)) as u16;
                    }
                }
            }
            (*pCtx).bDequantCoeff4x4Init = true;
            (*pCtx).iDequantCoeffPpsid = iPpsId;
        }
    } else {
        (*pCtx).bUseScalingList = false;
    }
    ERR_NONE
}

// ============================================================================
// Inverse Transform & Dequantization Functions
// ============================================================================

// **T5.AC6: `WelsLumaDcDequantIdct` and `WelsChromaDcIdct` stood here and are
// deleted, not converted.** They were this file's copies of the two DC transforms
// and they had **zero callers anywhere in the crate** — `parse_mb_syn_cavlc.rs` and
// `parse_mb_syn_cabac.rs` each call their own. Worse, this pair had *drifted*: its
// luma form clipped the QP and read `g_kuiDequantCoeff` unconditionally, with no
// scaling-list arm at all, so it was the pre-`bUseScalingList` version preserved by
// having no user. S18's shape — a raw duplicate beside the live one — found by the
// signature conversion that would otherwise have had to convert it too.
//
// The surviving pair is still two copies of one C++ function (`decode_slice.cpp`'s
// `WelsLumaDcDequantIdct` and `WelsChromaDcIdct`), which is **F22's class and not
// this phase's**: they are allowlisted in the duplicate census and their C++ home
// is a decoder file both parsers include. Deduplicating them is a Phase 8 item with
// the rest of the census's 59.

// ============================================================================
// Neighbor Availability Mapping
// ============================================================================

/// `bConstainedIntraPredFlag`, as a type. (The misspelling is upstream's, in both
/// the PPS field and the `Constrain0`/`Constrain1` function names; P14 keeps it.)
///
/// **This replaces three `Option<fn>` members of `SWelsDecoderContext`** —
/// `pFillInfoCacheIntraNxNFunc`, `pMapNxNNeighToSampleFunc`,
/// `pMap16x16NeighToSampleFunc` — and their three typedefs. They were never three
/// independent choices: `WelsDecodeSlice` and `WelsDecodeAndConstructSlice` each set
/// all three together, from one `if`, on one flag read out of the slice's PPS. The
/// same *configuration, not dispatch* shape as T4b.1's entropy slots.
///
/// **Why this seam is where the crate's oldest ratchet metric finally moves.** The
/// three typedefs declared
/// `pNeighAvail: *mut c_void` and `extern "C"`; the functions actually stored take
/// `PWelsNeighAvail` / a layer pointer and two of them are not `extern "C"` at all. So
/// *every* install and *every* fallback had to launder the mismatch through
/// `mem::transmute` — **19 of the crate's 21 such calls, in this one family**.
/// (T4b.3b took the remaining two, at `decoder_core.rs`'s expand wrapper. The
/// crate's count is now **zero calls**; the metric's residue is prose only.)
/// Naming the configuration lets the methods take the real types, and the casts at
/// the call sites (`as *mut _ as *mut c_void`) go with them. Nothing is reinterpreted
/// any more; the types simply match.
///
/// `Constrain0 = 0` is load-bearing twice over: `SWelsDecoderContext` is built from a
/// `MaybeUninit::zeroed()` shell (`decoder_context.rs`), so the zero pattern must be a
/// declared variant (S21) — and `Constrain0` is also exactly what every former
/// `unwrap_or_else` fallback named, so a zeroed context dispatches where an
/// uninstalled slot used to.
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum IntraPredConstraint {
    /// `bConstainedIntraPredFlag == false` — `WelsFillCacheConstrain0IntraNxN`,
    /// `WelsMapNxNNeighToSampleNormal`, `WelsMap16x16NeighToSampleNormal`.
    #[default]
    Constrain0 = 0,
    /// `bConstainedIntraPredFlag == true` — the `Constrain1` trio.
    Constrain1 = 1,
}

impl IntraPredConstraint {
    /// `pPps->bConstainedIntraPredFlag`, the one `if` this type replaces.
    #[inline]
    pub fn from_flag(bConstainedIntraPredFlag: bool) -> Self {
        if bConstainedIntraPredFlag {
            IntraPredConstraint::Constrain1
        } else {
            IntraPredConstraint::Constrain0
        }
    }

    /// `pCtx->pFillInfoCacheIntraNxNFunc (…)`.
    ///
    /// # Safety
    /// The pointers must satisfy `WelsFillCacheConstrain0IntraNxN`'s contract.
    #[inline]
    pub fn FillCacheIntraNxN(
        self,
        pNeighAvail: &SWelsNeighAvail,
        pNonZeroCount: &mut [u8; 48],
        pIntraPredMode: &mut [i8; 48],
        pCurDqLayer: &DqLayerState,
    ) {
        match self {
            IntraPredConstraint::Constrain0 => {
                crate::decoder::parse_mb_syn_cavlc::WelsFillCacheConstrain0IntraNxN(
                    pNeighAvail,
                    pNonZeroCount,
                    pIntraPredMode,
                    pCurDqLayer,
                )
            }
            IntraPredConstraint::Constrain1 => {
                crate::decoder::parse_mb_syn_cavlc::WelsFillCacheConstrain1IntraNxN(
                    pNeighAvail,
                    pNonZeroCount,
                    pIntraPredMode,
                    pCurDqLayer,
                )
            }
        }
    }

    /// `pCtx->pMapNxNNeighToSampleFunc (…)`.
    ///
    /// # Safety
    /// Both pointers must be non-null and writable for their element counts.
    #[inline]
    pub fn MapNxNNeighToSample(
        self,
        pNeighAvail: &mut SWelsNeighAvail,
        pSampleAvail: &mut [i32; 30],
    ) {
        match self {
            IntraPredConstraint::Constrain0 => {
                WelsMapNxNNeighToSampleNormal(pNeighAvail, pSampleAvail)
            }
            IntraPredConstraint::Constrain1 => {
                WelsMapNxNNeighToSampleConstrain1(pNeighAvail, pSampleAvail)
            }
        }
    }

    /// `pCtx->pMap16x16NeighToSampleFunc (…)`.
    ///
    /// # Safety
    /// Both pointers must be non-null and writable.
    #[inline]
    pub fn Map16x16NeighToSample(
        self,
        pNeighAvail: &mut SWelsNeighAvail,
        pSampleAvail: &mut u8,
    ) {
        match self {
            IntraPredConstraint::Constrain0 => {
                WelsMap16x16NeighToSampleNormal(pNeighAvail, pSampleAvail)
            }
            IntraPredConstraint::Constrain1 => {
                WelsMap16x16NeighToSampleConstrain1(pNeighAvail, pSampleAvail)
            }
        }
    }
}

pub extern "C" fn WelsMapNxNNeighToSampleNormal(
    pNeighAvail: &mut SWelsNeighAvail,
    pSampleAvail: &mut [i32; 30],
) {
    let avail = &*pNeighAvail;
    if avail.iLeftAvail != 0 {
        pSampleAvail[6] = 1;
        pSampleAvail[12] = 1;
        pSampleAvail[18] = 1;
        pSampleAvail[24] = 1;
    }
    if avail.iLeftTopAvail != 0 {
        pSampleAvail[0] = 1;
    }
    if avail.iTopAvail != 0 {
        pSampleAvail[1] = 1;
        pSampleAvail[2] = 1;
        pSampleAvail[3] = 1;
        pSampleAvail[4] = 1;
    }
    if avail.iRightTopAvail != 0 {
        pSampleAvail[5] = 1;
    }
}

pub extern "C" fn WelsMapNxNNeighToSampleConstrain1(
    pNeighAvail: &mut SWelsNeighAvail,
    pSampleAvail: &mut [i32; 30],
) {
    let avail = &*pNeighAvail;
    if avail.iLeftAvail != 0 && IS_INTRA(avail.iLeftType) {
        pSampleAvail[6] = 1;
        pSampleAvail[12] = 1;
        pSampleAvail[18] = 1;
        pSampleAvail[24] = 1;
    }
    if avail.iLeftTopAvail != 0 && IS_INTRA(avail.iLeftTopType) {
        pSampleAvail[0] = 1;
    }
    if avail.iTopAvail != 0 && IS_INTRA(avail.iTopType) {
        pSampleAvail[1] = 1;
        pSampleAvail[2] = 1;
        pSampleAvail[3] = 1;
        pSampleAvail[4] = 1;
    }
    if avail.iRightTopAvail != 0 && IS_INTRA(avail.iRightTopType) {
        pSampleAvail[5] = 1;
    }
}

pub extern "C" fn WelsMap16x16NeighToSampleNormal(
    pNeighAvail: &mut SWelsNeighAvail,
    pSampleAvail: &mut u8,
) {
    let avail = &*pNeighAvail;
    let mut mask: u8 = 0;
    if avail.iLeftAvail != 0 {
        mask |= 1 << 2;
    }
    if avail.iLeftTopAvail != 0 {
        mask |= 1 << 1;
    }
    if avail.iTopAvail != 0 {
        mask |= 1;
    }
    *pSampleAvail = mask;
}

pub extern "C" fn WelsMap16x16NeighToSampleConstrain1(
    pNeighAvail: &mut SWelsNeighAvail,
    pSampleAvail: &mut u8,
) {
    let avail = &*pNeighAvail;
    let mut mask: u8 = 0;
    if avail.iLeftAvail != 0 && IS_INTRA(avail.iLeftType) {
        mask |= 1 << 2;
    }
    if avail.iLeftTopAvail != 0 && IS_INTRA(avail.iLeftTopType) {
        mask |= 1 << 1;
    }
    if avail.iTopAvail != 0 && IS_INTRA(avail.iTopType) {
        mask |= 1;
    }
    *pSampleAvail = mask;
}

// ============================================================================
// Block Zeroing & NonZero Count Functions -- deleted at T4b.3c
// ============================================================================
//
// `WelsBlockInit`, `WelsBlockZero16x16_c` and `WelsBlockZero8x8_c`
// (`decode_slice.cpp:3021-3036`) were installed into `SBlockFunc`'s two zeroing
// slots and **called from nowhere**, in this port and in the C++ tree alike; they
// went with the table. `WelsBlockFuncInit` (`decode_slice.cpp:2990`) went with
// them: its `iCpu` argument selected between `_c`, `_neon`, `_AArch64_neon` and
// `_sse2` in the C++ and selected nothing here.
//
// This module's third `WelsNonZeroCount_c` went too. The C++ has **one**, in
// `common/src/deblocking_common.cpp:248`; the port had three, and this one was the
// copy that never got Phase 2's conversion -- a hand-written `if *p != 0 { *p = 1 }`
// loop where the other two are shims over the safe `nonzero_count` kernel
// (`(*v != 0) as i8`, the C++'s `!!`). The single reader below now calls
// `common/deblocking_common.rs`'s shim, which is a plain `unsafe fn`: with no
// `Option<fn>` slot to fill, the `extern "C"` this copy carried had nothing left to
// satisfy. `encoder/deblocking.rs` keeps its own `extern "C"` copy for now because
// `pfSetNZCZero` is still a slot -- that is the last member of this family.

// ============================================================================
// Macroblock Reconstruction Functions
// ============================================================================

/// **T5.X8: the residual add takes the picture, not three plane pointers.**
///
/// The three `*mut u8` and two strides were the caller's `data_ptr(i) + (mb << 4)`
/// arithmetic, handed on so the IDCT shims could rebuild a slice out of them. The
/// picture owns its planes, so the cursor is `plane_mut(i).cursor_mut(x, y)` and
/// the block offsets are sample coordinates rather than byte offsets that had to be
/// recomputed whenever a stride changed.
pub fn WelsMbInterSampleConstruction(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: Option<&mut SPicture>,
) -> i32 {
    let Some(pDec) = pDec else {
        return ERR_NONE;
    };
    let ctx = &*pCtx;
    let dq: &mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let (mb_x, mb_y) = ((*dq).iMbX as isize, (*dq).iMbY as isize);

    let pTransformSize8x8 = *(*dq).grid.transform_size8x8_flag.get(iMbXy);
    let pNzc = *(*dq).grid.nzc.get(iMbXy);
    let tcoeff: &[i16; 384] = (*dq).grid.scaled_tcoeff.get(iMbXy);

    if pTransformSize8x8 {
        if let Some(idct8x8) = ctx.pIdctResAddPredFunc8x8 {
            for i in 0..4 {
                let iIndex = g_kuiMbCountScan4Idx[i << 2] as usize;
                if pNzc[iIndex] != 0
                    || pNzc[iIndex + 1] != 0
                    || pNzc[iIndex + 4] != 0
                    || pNzc[iIndex + 5] != 0
                {
                    let (dx, dy) = (((iIndex % 4) << 2) as isize, ((iIndex >> 2) << 2) as isize);
                    let rs: &[i16; 64] = tcoeff[i << 6..][..64].try_into().unwrap();
                    idct8x8(
                        &mut pDec
                            .plane_mut(0)
                            .cursor_mut((mb_x << 4) + dx, (mb_y << 4) + dy),
                        rs,
                    );
                }
            }
        }
    } else {
        if let Some(idct4x4) = ctx.pIdctFourResAddPredFunc {
            for (q, (dx, dy, nz)) in [(0, 0, 0usize), (8, 0, 2), (0, 8, 8), (8, 8, 10)]
                .into_iter()
                .enumerate()
            {
                let rs: &[i16; 64] = tcoeff[q << 6..][..64].try_into().unwrap();
                let nzc: &[i8; 6] = pNzc[nz..][..6].try_into().unwrap();
                idct4x4(
                    &mut pDec
                        .plane_mut(0)
                        .cursor_mut((mb_x << 4) + dx, (mb_y << 4) + dy),
                    rs,
                    nzc,
                );
            }
        }
    }

    if let Some(idct4x4) = ctx.pIdctFourResAddPredFunc {
        for (plane, (coeff, nz)) in [(256usize, 16usize), (320, 18)].into_iter().enumerate() {
            let rs: &[i16; 64] = tcoeff[coeff..][..64].try_into().unwrap();
            let nzc: &[i8; 6] = pNzc[nz..][..6].try_into().unwrap();
            idct4x4(
                &mut pDec
                    .plane_mut(plane + 1)
                    .cursor_mut(mb_x << 3, mb_y << 3),
                rs,
                nzc,
            );
        }
    }

    ERR_NONE
}

// Phase 4a: `BaseMC` calls these directly instead of through `SMcFunc` slots.
// T5b.2: and the safe kernels directly rather than through the raw shims, because
// the callers now hold pictures rather than plane pointers.
use crate::common::mc::{mc_chroma, mc_chroma_same, mc_luma, mc_luma_same};

// ============================================================================
// Inter prediction — `rec_mb.cpp`, and what became of `sMCRefMember`
// ============================================================================
//
// **T5b.2 (face 0 + face 2).** The C++'s MC descriptor is six raw plane cursors,
// two source strides, two destination strides and the picture's dimensions; the port
// built one per macroblock with `mem::zeroed()` and walked its `pDst*` fields with
// pointer arithmetic. Every one of those fields is something a picture already
// carries or a coordinate the walk already knows, so the descriptor is gone and what
// replaces it is [`McDst`] (where the block lands) plus [`McSrc`] (where the samples
// come from). The two `mem::zeroed()` sites die with it — S21's question, "what does
// the all-zero pattern mean now", had no answer here: a zeroed descriptor is six
// null cursors that every path overwrites before use.
//
// **What made this face a design question rather than a spelling pass is F42.** A
// malformed stream can put the picture being decoded into its own reference list, so
// `pSrcY` and `pDstY` can address one allocation — which is why `pDec` was a raw
// pointer in twenty-three signatures, and why converting them needed a *type* for
// the answer. [`PicRefs::classify`] is that type: `RefSlot::Other` is a picture
// disjoint from the destination and runs the two-cursor kernels; `RefSlot::Current`
// resolves to the destination itself and runs `mc_luma_same`/`mc_chroma_same`
// (`common/mc.rs`, T5b.1), which read and write through the one `&mut`.

/// **Where a partition's prediction lands**: the block's sample coordinates in the
/// destination picture's luma and chroma planes.
///
/// `sMCRefMember`'s `pDstY`/`pDstU`/`pDstV`, as the coordinates the pointers were
/// carrying. **Luma and chroma are tracked separately rather than derived from one
/// another**, and `rec_mb.cpp:1014` is the reason: it applies the 8x8 sub-partition
/// offset *twice* to the LIST_1 luma destination of a bi-predicted 4x4 block and
/// once to its chroma. The C's two walks were independent, so the divergence was
/// expressible; deriving `chroma = luma >> 1` would silently repair it, which is a
/// behaviour change on a B-slice path (S6).
#[derive(Clone, Copy)]
struct McDst {
    luma: (isize, isize),
    chroma: (isize, isize),
}

impl McDst {
    /// The macroblock's own top-left sample, in both planes.
    #[inline]
    fn mb(iMbX: i32, iMbY: i32) -> Self {
        Self {
            luma: ((iMbX as isize) << 4, (iMbY as isize) << 4),
            chroma: ((iMbX as isize) << 3, (iMbY as isize) << 3),
        }
    }

    /// A sub-block step both planes take together: luma by `(x, y)`, chroma by half
    /// of each. Every `pDst*.offset(...)` pair in `rec_mb.cpp` is this one except the
    /// case named at the type.
    #[inline]
    fn blk(self, x: isize, y: isize) -> Self {
        Self {
            luma: (self.luma.0 + x, self.luma.1 + y),
            chroma: (self.chroma.0 + (x >> 1), self.chroma.1 + (y >> 1)),
        }
    }

    /// The two planes stepped independently — `rec_mb.cpp:1014`'s case, and only it.
    #[inline]
    fn split_blk(self, lx: isize, ly: isize, cx: isize, cy: isize) -> Self {
        Self {
            luma: (self.luma.0 + lx, self.luma.1 + ly),
            chroma: (self.chroma.0 + cx, self.chroma.1 + cy),
        }
    }
}

/// Where motion compensation reads from — [`PicRefs::classify`]'s answer resolved
/// against *this* destination.
enum McSrc<'a> {
    /// A picture disjoint from the destination: the ordinary case, and the one the
    /// two-cursor kernels are written for.
    Other(&'a SPicture),
    /// The destination picture is its own reference (**F42**): one allocation, and
    /// `mc_luma_same`/`mc_chroma_same` are what run.
    Dst,
}

/// The C's `ERR_INFO_REFERENCE_PIC_LOST`, which all three of `GetRefPic`'s failure
/// arms return.
#[inline]
fn ref_pic_lost() -> i32 {
    GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST)
}

/// `GetRefPic`'s third arm: the C tested the three source cursors for null after
/// filling them, which is exactly "this picture has an unallocated plane".
#[inline]
fn has_planes(pic: &SPicture) -> bool {
    !pic.plane(0).is_empty() && !pic.plane(1).is_empty() && !pic.plane(2).is_empty()
}

/// The reference `iRefIdx` selects in list `listIdx`, for a destination that **is**
/// the picture the bracket holds — the P-slice path and the B path's LIST_0 half.
///
/// Matches `GetRefPic` in `rec_mb.cpp`, whose whole body was filling `sMCRefMember`'s
/// source cursors and strides from the resolved picture. The three failure arms are
/// the C's: a negative index, a handle that resolves to nothing, and a resolved
/// picture with an empty plane.
#[inline]
fn ref_for_current<'a>(
    pRefs: PicRefs<'a>,
    sRefPic: &SRefPic,
    iRefIdx: i8,
    listIdx: usize,
) -> Result<McSrc<'a>, i32> {
    if iRefIdx < 0 {
        return Err(ref_pic_lost());
    }
    match pRefs.classify(ref_id(sRefPic, listIdx, iRefIdx as usize)) {
        RefSlot::Empty => Err(ref_pic_lost()),
        // F42. The C resolved this to `pCtx->pDec` and read on; so does the `Dst`
        // arm, and the destination's own planes are the ones `has_planes` would have
        // tested — the caller holds them mutably, so the test moves there.
        RefSlot::Current => Ok(McSrc::Dst),
        RefSlot::Other(pic) if has_planes(pic) => Ok(McSrc::Other(pic)),
        RefSlot::Other(_) => Err(ref_pic_lost()),
    }
}

/// [`ref_for_current`] for a destination that is **not** the bracket's picture: the
/// B-slice scratch (`pCtx->pTempDec`), where the LIST_1 hypothesis lands.
///
/// F42's arm is an ordinary disjoint reference here — the destination is a different
/// allocation — so `cur` is passed in and the answer never needs the `Dst` form.
#[inline]
fn ref_for_other<'s>(
    pRefs: PicRefs<'s>,
    sRefPic: &SRefPic,
    iRefIdx: i8,
    listIdx: usize,
    cur: &'s SPicture,
) -> Result<McSrc<'s>, i32> {
    if iRefIdx < 0 {
        return Err(ref_pic_lost());
    }
    match pRefs.resolve(ref_id(sRefPic, listIdx, iRefIdx as usize), Some(cur)) {
        Some(pic) if has_planes(pic) => Ok(McSrc::Other(pic)),
        _ => Err(ref_pic_lost()),
    }
}

/// Motion-compensate one block from the reference into the destination.
/// Matches `BaseMC` in `rec_mb.cpp` (single-thread path).
///
/// `geom` is `sMCRefMember`'s `(iPicWidth, iPicHeight)` — the MV clamp's bounds, and
/// the descriptor's only two fields that were data rather than cursors.
fn BaseMC(
    geom: (i32, i32),
    src: &McSrc<'_>,
    dst: &mut SPicture,
    at: McDst,
    iXOffset: i32,
    iYOffset: i32,
    iBlkWidth: i32,
    iBlkHeight: i32,
    iMVs: [i16; 2],
) {
    const PADDING_LENGTH: i32 = 32;
    let mut iFullMVx = (iXOffset << 2) + iMVs[0] as i32;
    let mut iFullMVy = (iYOffset << 2) + iMVs[1] as i32;
    iFullMVx = WELS_CLIP3(
        iFullMVx,
        (-PADDING_LENGTH + 2) * 4,
        (geom.0 + PADDING_LENGTH - 19) * 4,
    );
    iFullMVy = WELS_CLIP3(
        iFullMVy,
        (-PADDING_LENGTH + 2) * 4,
        (geom.1 + PADDING_LENGTH - 19) * 4,
    );

    // The C added `(iFullMVx >> 2) + (iFullMVy >> 2) * iSrcLineLuma` to the source
    // plane's origin; the stride is the plane's own, so what is left is the sample
    // coordinate. Chroma is the same expression at `>> 3`, which is the eighth-pel
    // vector's integer part — *not* half the luma one, because the two shifts round
    // differently on negatives.
    let (sy_luma, sx_luma) = ((iFullMVy >> 2) as isize, (iFullMVx >> 2) as isize);
    let (sy_chroma, sx_chroma) = ((iFullMVy >> 3) as isize, (iFullMVx >> 3) as isize);

    let (bw, bh) = (iBlkWidth as usize, iBlkHeight as usize);
    let (bwc, bhc) = (bw >> 1, bh >> 1);
    let (mvx, mvy) = (iFullMVx as i16, iFullMVy as i16);

    // Phase 4a's direct-dispatch note stands and is recorded at the safe kernels:
    // `pMCFunc` held `McLuma_c`/`McChroma_c` and nothing else, and the block
    // dimensions arrive here as parameters, which is why de-virtualization recovered
    // nothing on this side. What T5b.2 changes is that the shims — `shim_wh`'s
    // `from_raw_parts` pair per call, three per partition — are gone with the plane
    // pointers: the kernels are handed cursors over the pictures themselves.
    match *src {
        McSrc::Other(pic) => {
            mc_luma(
                &pic.plane(0).cursor(sx_luma, sy_luma),
                &mut dst.plane_mut(0).cursor_mut(at.luma.0, at.luma.1),
                mvx,
                mvy,
                bw,
                bh,
            );
            for i in 1..3usize {
                mc_chroma(
                    &pic.plane(i).cursor(sx_chroma, sy_chroma),
                    &mut dst.plane_mut(i).cursor_mut(at.chroma.0, at.chroma.1),
                    mvx,
                    mvy,
                    bwc,
                    bhc,
                );
            }
        }
        // F42's arm: the source anchor is expressed *relative to* the destination's,
        // because one plane has one stride and there is only one borrow to give.
        McSrc::Dst => {
            mc_luma_same(
                &mut dst.plane_mut(0).cursor_mut(at.luma.0, at.luma.1),
                sx_luma - at.luma.0,
                sy_luma - at.luma.1,
                mvx,
                mvy,
                bw,
                bh,
            );
            for i in 1..3usize {
                mc_chroma_same(
                    &mut dst.plane_mut(i).cursor_mut(at.chroma.0, at.chroma.1),
                    sx_chroma - at.chroma.0,
                    sy_chroma - at.chroma.1,
                    mvx,
                    mvy,
                    bwc,
                    bhc,
                );
            }
        }
    }
}

/// Matches `WeightPrediction` in `rec_mb.cpp`.
fn WeightPrediction(
    pwt: Option<&SPredWeightTable>,
    dst: &mut SPicture,
    at: McDst,
    listIdx: usize,
    iRefIdx: i32,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    // T5.X2 — take what you reach. The layer was the parameter and one nullable
    // field of it was the reach; the null test that stood on the field stands on the
    // `Option` here, with the same two arms.
    let Some(pwt) = pwt.filter(|_| iRefIdx >= 0) else {
        return;
    };
    // luma
    let iLog2denom = pwt.uiLumaLog2WeightDenom as i32;
    let iWoc = pwt.sPredList[listIdx].iLumaWeight[iRefIdx as usize];
    let iOoc = pwt.sPredList[listIdx].iLumaOffset[iRefIdx as usize];
    let mut cur = dst.plane_mut(0).cursor_mut(at.luma.0, at.luma.1);
    for i in 0..iBlkHeight as isize {
        let out = cur.row_mut(i, 0, iBlkWidth as usize);
        for p in out.iter_mut() {
            let iPredTemp = if iLog2denom >= 1 {
                ((*p as i32 * iWoc + (1 << (iLog2denom - 1))) >> iLog2denom) + iOoc
            } else {
                *p as i32 * iWoc + iOoc
            };
            *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
        }
    }
    // chroma
    let iLog2denom = pwt.uiChromaLog2WeightDenom as i32;
    for plane in 0..2usize {
        let iWoc = pwt.sPredList[listIdx].iChromaWeight[iRefIdx as usize][plane];
        let iOoc = pwt.sPredList[listIdx].iChromaOffset[iRefIdx as usize][plane];
        let mut cur = dst
            .plane_mut(plane + 1)
            .cursor_mut(at.chroma.0, at.chroma.1);
        for i in 0..(iBlkHeight >> 1) as isize {
            let out = cur.row_mut(i, 0, (iBlkWidth >> 1) as usize);
            for p in out.iter_mut() {
                let iPredTemp = if iLog2denom >= 1 {
                    ((*p as i32 * iWoc + (1 << (iLog2denom - 1))) >> iLog2denom) + iOoc
                } else {
                    *p as i32 * iWoc + iOoc
                };
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }
}

/// Matches `BiWeightPrediction` in `rec_mb.cpp`.
///
/// `tmp` is `pCtx->pTempDec` — a different picture from `dst` at every call site, so
/// the two halves of the blend are two borrows the compiler separates for free.
fn BiWeightPrediction(
    pwt: Option<&SPredWeightTable>,
    dst: &mut SPicture,
    at: McDst,
    tmp: &SPicture,
    tmp_at: McDst,
    iRefIdx1: i32,
    iRefIdx2: i32,
    bWeightedBipredIdcIs1: bool,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    // T5.X2, as `WeightPrediction` above.
    let Some(pwt) = pwt else {
        return;
    };
    let (mut iWoc1, mut iOoc1, mut iWoc2, mut iOoc2) = (0i32, 0i32, 0i32, 0i32);

    // luma
    let mut iLog2denom = pwt.uiLumaLog2WeightDenom as i32;
    if bWeightedBipredIdcIs1 {
        iWoc1 = pwt.sPredList[LIST_0].iLumaWeight[iRefIdx1 as usize];
        iOoc1 = pwt.sPredList[LIST_0].iLumaOffset[iRefIdx1 as usize];
        iWoc2 = pwt.sPredList[LIST_1].iLumaWeight[iRefIdx2 as usize];
        iOoc2 = pwt.sPredList[LIST_1].iLumaOffset[iRefIdx2 as usize];
    } else {
        iWoc1 = pwt.iImplicitWeight[iRefIdx1 as usize][iRefIdx2 as usize];
        iWoc2 = 64 - iWoc1;
    }

    {
        let src = tmp.plane(0).cursor(tmp_at.luma.0, tmp_at.luma.1);
        let mut cur = dst.plane_mut(0).cursor_mut(at.luma.0, at.luma.1);
        for i in 0..iBlkHeight as isize {
            let t = src.row(i, 0, iBlkWidth as usize);
            let out = cur.row_mut(i, 0, iBlkWidth as usize);
            for (p, &t) in out.iter_mut().zip(t) {
                let iPredTemp = ((*p as i32 * iWoc1 + t as i32 * iWoc2 + (1 << iLog2denom))
                    >> (iLog2denom + 1))
                    + ((iOoc1 + iOoc2 + 1) >> 1);
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }

    // UV
    let iBlkWidth = iBlkWidth >> 1;
    let iBlkHeight = iBlkHeight >> 1;
    iLog2denom = pwt.uiChromaLog2WeightDenom as i32;

    for k in 0..2usize {
        if bWeightedBipredIdcIs1 {
            iWoc1 = pwt.sPredList[LIST_0].iChromaWeight[iRefIdx1 as usize][k];
            iOoc1 = pwt.sPredList[LIST_0].iChromaOffset[iRefIdx1 as usize][k];
            iWoc2 = pwt.sPredList[LIST_1].iChromaWeight[iRefIdx2 as usize][k];
            iOoc2 = pwt.sPredList[LIST_1].iChromaOffset[iRefIdx2 as usize][k];
        }
        let src = tmp.plane(k + 1).cursor(tmp_at.chroma.0, tmp_at.chroma.1);
        let mut cur = dst.plane_mut(k + 1).cursor_mut(at.chroma.0, at.chroma.1);
        for i in 0..iBlkHeight as isize {
            let t = src.row(i, 0, iBlkWidth as usize);
            let out = cur.row_mut(i, 0, iBlkWidth as usize);
            for (p, &t) in out.iter_mut().zip(t) {
                let iPredTemp = ((*p as i32 * iWoc1 + t as i32 * iWoc2 + (1 << iLog2denom))
                    >> (iLog2denom + 1))
                    + ((iOoc1 + iOoc2 + 1) >> 1);
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }
}

/// Matches `BiPrediction` in `rec_mb.cpp`.
// T5.X2: the layer parameter was `_pCurDqLayer` — unused since the function was
// written — and it is deleted rather than converted.
fn BiPrediction(
    dst: &mut SPicture,
    at: McDst,
    tmp: &SPicture,
    tmp_at: McDst,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    {
        let src = tmp.plane(0).cursor(tmp_at.luma.0, tmp_at.luma.1);
        let mut cur = dst.plane_mut(0).cursor_mut(at.luma.0, at.luma.1);
        for i in 0..iBlkHeight as isize {
            let t = src.row(i, 0, iBlkWidth as usize);
            let out = cur.row_mut(i, 0, iBlkWidth as usize);
            for (p, &t) in out.iter_mut().zip(t) {
                let iPredTemp = (*p as i32 + t as i32 + 1) >> 1;
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }

    // UV
    let iBlkWidth = iBlkWidth >> 1;
    let iBlkHeight = iBlkHeight >> 1;
    for k in 0..2usize {
        let src = tmp.plane(k + 1).cursor(tmp_at.chroma.0, tmp_at.chroma.1);
        let mut cur = dst.plane_mut(k + 1).cursor_mut(at.chroma.0, at.chroma.1);
        for i in 0..iBlkHeight as isize {
            let t = src.row(i, 0, iBlkWidth as usize);
            let out = cur.row_mut(i, 0, iBlkWidth as usize);
            for (p, &t) in out.iter_mut().zip(t) {
                let iPredTemp = (*p as i32 + t as i32 + 1) >> 1;
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }
}

/// Inter (motion-compensated) prediction of one P-slice macroblock.
/// Matches `GetInterPred` in `rec_mb.cpp`.
pub fn GetInterPred(
    sRefPic: &SRefPic,
    pRefs: PicRefs<'_>,
    pDec: &mut SPicture,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    let iMBXY = pCurDqLayer.iMbXyIndex as usize;
    // Copied, not borrowed: the same picture is written below, and these are the
    // per-macroblock records the C++ hoisted into locals for the same reason.
    let iMBType = *pDec.pMbType.get(iMBXY);
    let mv_mb = *pDec.pMv[0].get(iMBXY);
    let ref_mb = *pDec.pRefIndex[0].get(iMBXY);

    let iMBOffsetX = pCurDqLayer.iMbX << 4;
    let iMBOffsetY = pCurDqLayer.iMbY << 4;

    let sh = &pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let geom = (sh.iMbWidth << 4, sh.iMbHeight << 4);
    let pwt = pCurDqLayer.sPredWeightTable;
    let bWeight = pCurDqLayer.bUseWeightPredictionFlag;
    let mb = McDst::mb(pCurDqLayer.iMbX, pCurDqLayer.iMbY);

    macro_rules! mc {
        ($at:expr, $iref:expr, $x:expr, $y:expr, $w:expr, $h:expr, $mvs:expr) => {{
            let src = match ref_for_current(pRefs, sRefPic, $iref, LIST_0) {
                Ok(src) => src,
                Err(ret) => return ret,
            };
            let at = $at;
            BaseMC(geom, &src, pDec, at, $x, $y, $w, $h, $mvs);
            if bWeight {
                WeightPrediction(pwt.as_ref(), pDec, at, LIST_0, $iref as i32, $w, $h);
            }
        }};
    }

    match iMBType {
        MB_TYPE_SKIP | MB_TYPE_16x16 => {
            mc!(mb, ref_mb[0], iMBOffsetX, iMBOffsetY, 16, 16, mv_mb[0]);
        }
        MB_TYPE_16x8 => {
            mc!(mb, ref_mb[0], iMBOffsetX, iMBOffsetY, 16, 8, mv_mb[0]);
            mc!(mb.blk(0, 8), ref_mb[8], iMBOffsetX, iMBOffsetY + 8, 16, 8, mv_mb[8]);
        }
        MB_TYPE_8x16 => {
            mc!(mb, ref_mb[0], iMBOffsetX, iMBOffsetY, 8, 16, mv_mb[0]);
            mc!(mb.blk(8, 0), ref_mb[2], iMBOffsetX + 8, iMBOffsetY, 8, 16, mv_mb[2]);
        }
        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
            // T5.I1: one window borrow at the loop head, where the C++ hoists
            // `pCurDqLayer->pSubMbType[iMBXY]` into a `uint32_t (*)[4]`.
            let pSubMbType = *pCurDqLayer.grid.sub_mb_type.get(iMBXY);
            for i in 0..4usize {
                let iSubMBType = pSubMbType[i];
                let iBlk8X = ((i & 1) << 3) as i32;
                let iBlk8Y = ((i >> 1) << 3) as i32;
                let iXOffset = iMBOffsetX + iBlk8X;
                let iYOffset = iMBOffsetY + iBlk8Y;

                let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);
                let iRefIndex = ref_mb[iIIdx];
                let blk8 = mb.blk(iBlk8X as isize, iBlk8Y as isize);

                match iSubMBType {
                    SUB_MB_TYPE_8x8 => {
                        mc!(blk8, iRefIndex, iXOffset, iYOffset, 8, 8, mv_mb[iIIdx]);
                    }
                    SUB_MB_TYPE_8x4 => {
                        mc!(blk8, iRefIndex, iXOffset, iYOffset, 8, 4, mv_mb[iIIdx]);
                        mc!(blk8.blk(0, 4), iRefIndex, iXOffset, iYOffset + 4, 8, 4, mv_mb[iIIdx + 4]);
                    }
                    SUB_MB_TYPE_4x8 => {
                        mc!(blk8, iRefIndex, iXOffset, iYOffset, 4, 8, mv_mb[iIIdx]);
                        mc!(blk8.blk(4, 0), iRefIndex, iXOffset + 4, iYOffset, 4, 8, mv_mb[iIIdx + 1]);
                    }
                    SUB_MB_TYPE_4x4 => {
                        for j in 0..4usize {
                            let iJIdx = ((j >> 1) << 2) + (j & 1);
                            let iBlk4X = ((j & 1) << 2) as i32;
                            let iBlk4Y = ((j >> 1) << 2) as i32;
                            mc!(
                                blk8.blk(iBlk4X as isize, iBlk4Y as isize),
                                iRefIndex,
                                iXOffset + iBlk4X,
                                iYOffset + iBlk4Y,
                                4,
                                4,
                                mv_mb[iIIdx + iJIdx]
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    ERR_NONE
}

/// Inter (motion-compensated) prediction of one B-slice macroblock.
/// Matches `GetInterBPred` in `rec_mb.cpp`.
///
/// `pTempDec` receives the LIST_1 prediction so the two hypotheses can be blended in
/// place by [`BiPrediction`] / [`BiWeightPrediction`]. It is `pCtx->pTempDec`, a
/// different picture from `pDec` — which is why F42's arm never reaches the LIST_1
/// half: for that half the current picture is an ordinary disjoint reference
/// ([`ref_for_other`]).
pub fn GetInterBPred(
    sRefPic: &SRefPic,
    pRefs: PicRefs<'_>,
    pDec: &mut SPicture,
    pTempDec: &mut SPicture,
    pCurDqLayer: &mut DqLayerState,
    bWeightedBipredIdcIs1: bool,
) -> i32 {
    let iMBXY = pCurDqLayer.iMbXyIndex as usize;

    let iMBType = *pDec.pMbType.get(iMBXY);
    let mv = [*pDec.pMv[LIST_0].get(iMBXY), *pDec.pMv[LIST_1].get(iMBXY)];
    let rf = [
        *pDec.pRefIndex[LIST_0].get(iMBXY),
        *pDec.pRefIndex[LIST_1].get(iMBXY),
    ];
    let pMv = |list: usize, idx: usize| -> [i16; 2] { mv[list][idx] };
    let pRef = |list: usize, idx: usize| -> i8 { rf[list][idx] };

    let iMBOffsetX = pCurDqLayer.iMbX << 4;
    let iMBOffsetY = pCurDqLayer.iMbY << 4;

    let sh = &pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let geom = (sh.iMbWidth << 4, sh.iMbHeight << 4);
    let pwt = pCurDqLayer.sPredWeightTable;
    let bUseWeightedBiPredIdc = pCurDqLayer.bUseWeightedBiPredIdc;
    let mb = McDst::mb(pCurDqLayer.iMbX, pCurDqLayer.iMbY);

    let mut iMVs = [0i16; 2];
    let mut iRefIndex0: i8 = 0;
    let mut iRefIndex1: i8 = 0;
    let mut iRefIndex: i8 = 0;

    /// LIST_0's hypothesis, into `pDec`.
    macro_rules! mc0 {
        ($at:expr, $list:expr, $iref:expr, $x:expr, $y:expr, $w:expr, $h:expr, $mvs:expr) => {{
            let src = match ref_for_current(pRefs, sRefPic, $iref, $list) {
                Ok(src) => src,
                Err(ret) => return ret,
            };
            BaseMC(geom, &src, pDec, $at, $x, $y, $w, $h, $mvs);
        }};
    }
    /// LIST_1's hypothesis, into `pTempDec`. `pDec` is only read here, so the
    /// shared reborrow coexists with the scratch picture's `&mut`.
    macro_rules! mc1 {
        ($at:expr, $list:expr, $iref:expr, $x:expr, $y:expr, $w:expr, $h:expr, $mvs:expr) => {{
            let src = match ref_for_other(pRefs, sRefPic, $iref, $list, &*pDec) {
                Ok(src) => src,
                Err(ret) => return ret,
            };
            BaseMC(geom, &src, pTempDec, $at, $x, $y, $w, $h, $mvs);
        }};
    }
    macro_rules! blend {
        ($at:expr, $tat:expr, $r0:expr, $r1:expr, $w:expr, $h:expr) => {{
            if bUseWeightedBiPredIdc {
                BiWeightPrediction(
                    pwt.as_ref(),
                    pDec,
                    $at,
                    pTempDec,
                    $tat,
                    $r0 as i32,
                    $r1 as i32,
                    bWeightedBipredIdcIs1,
                    $w,
                    $h,
                );
            } else {
                BiPrediction(pDec, $at, pTempDec, $tat, $w, $h);
            }
        }};
    }

    if IS_INTER_16x16(iMBType) {
        if IS_TYPE_L0(iMBType) && IS_TYPE_L1(iMBType) {
            iMVs = pMv(LIST_0, 0);
            iRefIndex0 = pRef(LIST_0, 0);
            mc0!(mb, LIST_0, iRefIndex0, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);

            iMVs = pMv(LIST_1, 0);
            iRefIndex1 = pRef(LIST_1, 0);
            mc1!(mb, LIST_1, iRefIndex1, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);
            blend!(mb, mb, iRefIndex0, iRefIndex1, 16, 16);
        } else {
            let listIdx = if (iMBType & MB_TYPE_P0L0) != 0 { LIST_0 } else { LIST_1 };
            iMVs = pMv(listIdx, 0);
            iRefIndex = pRef(listIdx, 0);
            mc0!(mb, listIdx, iRefIndex, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);
            if bWeightedBipredIdcIs1 {
                WeightPrediction(pwt.as_ref(), pDec, mb, listIdx, iRefIndex as i32, 16, 16);
            }
        }
    } else if IS_INTER_16x8(iMBType) {
        // **The two destination walks accumulate, and that is not a transcription
        // choice.** `rec_mb.cpp:749` advances `pMCRefMem.pDst*` *inside* the list
        // loop under `if (i)`, and `pMCRefMem` is function-scoped — so a second-half
        // partition predicted from **both** lists advances it **twice**, and the
        // LIST_1 hypothesis of that partition lands 8 rows below where a single
        // advance would put it. Reproduced verbatim (S6); a fixed per-partition
        // coordinate diverges on exactly the bi-predicted 16x8 macroblock, which is
        // what eight B-slice conformance assets caught when this face first landed.
        let mut at = mb;
        let mut tat = mb;
        for i in 0..2usize {
            let iPartIdx = i << 3;
            let mut listCount = 0u32;
            let mut lastListIdx = LIST_0;
            for listIdx in LIST_0..LIST_A {
                if IS_DIR(iMBType, i, listIdx) {
                    lastListIdx = listIdx;
                    iMVs = pMv(listIdx, iPartIdx);
                    iRefIndex = pRef(listIdx, iPartIdx);
                    if i != 0 {
                        at = at.blk(0, 8);
                    }
                    mc0!(at, listIdx, iRefIndex, iMBOffsetX, iMBOffsetY + iPartIdx as i32, 16, 8, iMVs);
                    listCount += 1;
                    if listCount == 2 {
                        iMVs = pMv(LIST_1, iPartIdx);
                        iRefIndex1 = pRef(LIST_1, iPartIdx);
                        if i != 0 {
                            tat = tat.blk(0, 8);
                        }
                        mc1!(tat, LIST_1, iRefIndex1, iMBOffsetX, iMBOffsetY + iPartIdx as i32, 16, 8, iMVs);
                        iRefIndex0 = pRef(LIST_0, iPartIdx);
                        iRefIndex1 = pRef(LIST_1, iPartIdx);
                        blend!(at, tat, iRefIndex0, iRefIndex1, 16, 8);
                    }
                }
            }
            if listCount == 1 && bWeightedBipredIdcIs1 {
                iRefIndex = pRef(lastListIdx, iPartIdx);
                WeightPrediction(pwt.as_ref(), pDec, at, lastListIdx, iRefIndex as i32, 16, 8);
            }
        }
    } else if IS_INTER_8x16(iMBType) {
        // The 16x8 arm's accumulation, in columns (`rec_mb.cpp:794`).
        let mut at = mb;
        let mut tat = mb;
        for i in 0..2usize {
            let iXOffset = iMBOffsetX + if i != 0 { 8 } else { 0 };
            let mut listCount = 0u32;
            let mut lastListIdx = LIST_0;
            for listIdx in LIST_0..LIST_A {
                if IS_DIR(iMBType, i, listIdx) {
                    lastListIdx = listIdx;
                    iMVs = pMv(listIdx, i << 1);
                    iRefIndex = pRef(listIdx, i << 1);
                    if i != 0 {
                        at = at.blk(8, 0);
                    }
                    mc0!(at, listIdx, iRefIndex, iXOffset, iMBOffsetY, 8, 16, iMVs);
                    listCount += 1;
                    if listCount == 2 {
                        iMVs = pMv(LIST_1, i << 1);
                        iRefIndex1 = pRef(LIST_1, i << 1);
                        if i != 0 {
                            tat = tat.blk(8, 0);
                        }
                        mc1!(tat, LIST_1, iRefIndex1, iXOffset, iMBOffsetY, 8, 16, iMVs);
                        iRefIndex0 = pRef(LIST_0, i << 1);
                        iRefIndex1 = pRef(LIST_1, i << 1);
                        blend!(at, tat, iRefIndex0, iRefIndex1, 8, 16);
                    }
                }
            }
            if listCount == 1 && bWeightedBipredIdcIs1 {
                iRefIndex = pRef(lastListIdx, i << 1);
                WeightPrediction(pwt.as_ref(), pDec, at, lastListIdx, iRefIndex as i32, 8, 16);
            }
        }
    } else if IS_Inter_8x8(iMBType) {
        // T5.I1: hoisted as in `GetInterPred`.
        let pSubMbType = *pCurDqLayer.grid.sub_mb_type.get(iMBXY);
        for i in 0..4usize {
            let iSubMBType = pSubMbType[i];
            let iBlk8X = ((i & 1) << 3) as i32;
            let iBlk8Y = ((i >> 1) << 3) as i32;
            let iXOffset = iMBOffsetX + iBlk8X;
            let iYOffset = iMBOffsetY + iBlk8Y;

            let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);
            let blk8 = mb.blk(iBlk8X as isize, iBlk8Y as isize);

            // Both destinations start at the sub-block; the C copies `pMCRefMem` into
            // `pTempMCRefMem` and then re-points only the `pDst*` fields, so the two
            // agree here and diverge only in the 4x4 arm below.
            if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                iRefIndex0 = pRef(LIST_0, iIIdx);
                iRefIndex1 = pRef(LIST_1, iIIdx);
            } else {
                iRefIndex = pRef(if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 }, iIIdx);
            }

            if IS_SUB_8x8(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    iMVs = pMv(LIST_0, iIIdx);
                    mc0!(blk8, LIST_0, iRefIndex0, iXOffset, iYOffset, 8, 8, iMVs);
                    iMVs = pMv(LIST_1, iIIdx);
                    mc1!(blk8, LIST_1, iRefIndex1, iXOffset, iYOffset, 8, 8, iMVs);
                    blend!(blk8, blk8, iRefIndex0, iRefIndex1, 8, 8);
                } else {
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    mc0!(blk8, listIdx, iRefIndex, iXOffset, iYOffset, 8, 8, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pwt.as_ref(), pDec, blk8, listIdx, iRefIndex as i32, 8, 8);
                    }
                }
            } else if IS_SUB_8x4(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    // B_Bi_8x4
                    iMVs = pMv(LIST_0, iIIdx);
                    mc0!(blk8, LIST_0, iRefIndex0, iXOffset, iYOffset, 8, 4, iMVs);
                    iMVs = pMv(LIST_1, iIIdx);
                    mc1!(blk8, LIST_1, iRefIndex1, iXOffset, iYOffset, 8, 4, iMVs);
                    blend!(blk8, blk8, iRefIndex0, iRefIndex1, 8, 4);

                    let lower = blk8.blk(0, 4);
                    iMVs = pMv(LIST_0, iIIdx + 4);
                    mc0!(lower, LIST_0, iRefIndex0, iXOffset, iYOffset + 4, 8, 4, iMVs);
                    iMVs = pMv(LIST_1, iIIdx + 4);
                    mc1!(lower, LIST_1, iRefIndex1, iXOffset, iYOffset + 4, 8, 4, iMVs);
                    blend!(lower, lower, iRefIndex0, iRefIndex1, 8, 4);
                } else {
                    // B_L0_8x4 B_L1_8x4
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    mc0!(blk8, listIdx, iRefIndex, iXOffset, iYOffset, 8, 4, iMVs);
                    let lower = blk8.blk(0, 4);
                    iMVs = pMv(listIdx, iIIdx + 4);
                    mc0!(lower, listIdx, iRefIndex, iXOffset, iYOffset + 4, 8, 4, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pwt.as_ref(), pDec, lower, listIdx, iRefIndex as i32, 8, 4);
                    }
                }
            } else if IS_SUB_4x8(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    // B_Bi_4x8
                    iMVs = pMv(LIST_0, iIIdx);
                    mc0!(blk8, LIST_0, iRefIndex0, iXOffset, iYOffset, 4, 8, iMVs);
                    iMVs = pMv(LIST_1, iIIdx);
                    mc1!(blk8, LIST_1, iRefIndex1, iXOffset, iYOffset, 4, 8, iMVs);
                    blend!(blk8, blk8, iRefIndex0, iRefIndex1, 4, 8);

                    let right = blk8.blk(4, 0);
                    iMVs = pMv(LIST_0, iIIdx + 1);
                    mc0!(right, LIST_0, iRefIndex0, iXOffset + 4, iYOffset, 4, 8, iMVs);
                    iMVs = pMv(LIST_1, iIIdx + 1);
                    mc1!(right, LIST_1, iRefIndex1, iXOffset + 4, iYOffset, 4, 8, iMVs);
                    blend!(right, right, iRefIndex0, iRefIndex1, 4, 8);
                } else {
                    // B_L0_4x8 B_L1_4x8
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    mc0!(blk8, listIdx, iRefIndex, iXOffset, iYOffset, 4, 8, iMVs);
                    let right = blk8.blk(4, 0);
                    iMVs = pMv(listIdx, iIIdx + 1);
                    mc0!(right, listIdx, iRefIndex, iXOffset + 4, iYOffset, 4, 8, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pwt.as_ref(), pDec, right, listIdx, iRefIndex as i32, 4, 8);
                    }
                }
            } else if IS_SUB_4x4(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    for j in 0..4usize {
                        let iJIdx = ((j >> 1) << 2) + (j & 1);
                        let iBlk4X = ((j & 1) << 2) as i32;
                        let iBlk4Y = ((j >> 1) << 2) as i32;

                        let at = blk8.blk(iBlk4X as isize, iBlk4Y as isize);
                        // NOTE: C indexes the LIST_1 *luma* destination with
                        // iBlk8X/iBlk8Y here, not iBlk4X/iBlk4Y, so the 8x8 offset is
                        // applied twice — while its chroma takes the 4x4 offset like
                        // everything else. Kept verbatim; `McDst`'s two coordinate
                        // pairs exist for this line. See rec_mb.cpp:1014.
                        let tat = blk8.split_blk(
                            iBlk8X as isize,
                            iBlk8Y as isize,
                            (iBlk4X >> 1) as isize,
                            (iBlk4Y >> 1) as isize,
                        );

                        iMVs = pMv(LIST_0, iIIdx + iJIdx);
                        mc0!(at, LIST_0, iRefIndex0, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);
                        iMVs = pMv(LIST_1, iIIdx + iJIdx);
                        mc1!(tat, LIST_1, iRefIndex1, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);
                        blend!(at, tat, iRefIndex0, iRefIndex1, 4, 4);
                    }
                } else {
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iRefIndex = pRef(listIdx, iIIdx);
                    for j in 0..4usize {
                        let iJIdx = ((j >> 1) << 2) + (j & 1);
                        let iBlk4X = ((j & 1) << 2) as i32;
                        let iBlk4Y = ((j >> 1) << 2) as i32;
                        let at = blk8.blk(iBlk4X as isize, iBlk4Y as isize);
                        iMVs = pMv(listIdx, iIIdx + iJIdx);
                        mc0!(at, listIdx, iRefIndex, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);
                        if bWeightedBipredIdcIs1 {
                            WeightPrediction(pwt.as_ref(), pDec, at, listIdx, iRefIndex as i32, 4, 4);
                        }
                    }
                }
            }
        }
    }
    ERR_NONE
}

/// `pCtx->pTempDec`, lazily allocated on the first B macroblock — the `else` branch
/// shared by `WelsMbInterConstruction` / `WelsMbInterPrediction` in
/// `decode_slice.cpp`, with the three plane pointers it used to hand back replaced by
/// the picture itself (T5b.2).
///
/// **The borrow is on the field** (S29), so the caller's other reads of the view are
/// disjoint field paths and the compiler separates them.
#[inline]
fn temp_pred_pic<'v>(pCtx: &'v mut SliceCtx<'_>) -> Option<&'v mut SPicture> {
    if pCtx.pTempDec.is_none() {
        let (iMbWidth, iMbHeight) = match pCtx.active_sps() {
            Some(sps) => (sps.iMbWidth, sps.iMbHeight),
            None => return None,
        };
        // T5.P″1: `alloc_picture` hands back the owner, and the field keeps it. The
        // lazy arm's two null tests are the same two states — "not allocated yet" and
        // "the allocation failed" — with `Option` spelling them.
        // T5.W3: `alloc_picture` stopped taking the context, so its `pMemAlign`
        // guard is tested here — the same `None`, from the same condition, at the one
        // caller that holds a context to test it with.
        *pCtx.pTempDec = if !pCtx.bHasMemAlign {
            None
        } else {
            crate::decoder::pic_queue::alloc_picture(
                pCtx.bParseOnly,
                (iMbWidth << 4) as i32,
                (iMbHeight << 4) as i32,
            )
        };
    }
    pCtx.pTempDec.as_deref_mut()
}

pub fn WelsMbInterConstruction(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    let ret = inter_pred(pCtx, pDec, pRefs, pCurDqLayer);
    if ret != ERR_NONE {
        return ret;
    }

    WelsMbInterSampleConstruction(pCtx, pCurDqLayer, Some(pDec));

    // `decode_slice.cpp:240`, the only reader of the former `sBlockFunc` table.
    // The C++ guards this with `GetThreadCount (pCtx) <= 1`; the port's
    // `GetThreadCount` is hard-coded 0 (decoder threading was never ported, T5c),
    // so the guard is always true and is not transcribed.
    //
    // T5b.2: the safe kernel directly. The `WelsNonZeroCount_c` shim it went through
    // existed to turn `*mut i8` into `&mut [i8; 24]`, and the grid already has one.
    crate::common::deblocking_common::nonzero_count(
        pCurDqLayer.grid.nzc.get_mut(pCurDqLayer.iMbXyIndex as usize),
    );

    ERR_NONE
}

/// MC-only reconstruction for inter macroblocks with cbp == 0 (incl. skip).
/// Matches `WelsMbInterPrediction` in `decode_slice.cpp`.
pub fn WelsMbInterPrediction(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    inter_pred(pCtx, pDec, pRefs, pCurDqLayer)
}

/// The prediction step both of the two above open with, and the one place the
/// P-slice and B-slice paths are told apart.
///
/// **The view is read field by field rather than through its methods**, and that is
/// what lets the B path hold `pTempDec` mutably beside `sRefPic`: `(*pCtx).pTempDec`
/// and `(*pCtx).sRefPic` are disjoint places, where `pCtx.ref_id(..)` would borrow
/// the whole view.
#[inline]
fn inter_pred(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    // A shared reference field copies out, so this borrow of the view ends here.
    let sRefPic: &SRefPic = pCtx.sRefPic;
    if pCtx.eSliceType == EWelsSliceType::P_SLICE {
        return GetInterPred(sRefPic, pRefs, pDec, pCurDqLayer);
    }
    let bWeightedBipredIdcIs1 = pCtx
        .pps_of(pCurDqLayer.sLayerInfo.pps_id)
        .is_some_and(|p| p.uiWeightedBipredIdc == 1);
    let Some(pTempDec) = temp_pred_pic(pCtx) else {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_MB_RECON_FAIL);
    };
    GetInterBPred(
        sRefPic,
        pRefs,
        pDec,
        pTempDec,
        pCurDqLayer,
        bWeightedBipredIdcIs1,
    )
}

pub fn WelsFillRecNeededMbInfo(
    pCtx: &mut SliceCtx<'_>,
    pDec: Option<&mut SPicture>,
    bOutput: bool,
    pCurDqLayer: &mut DqLayerState,
) {
    let Some(pCurPic) = pDec else {
        return;
    };
    let iLumaStride = pCurPic.linesize(0);
    let iChromaStride = pCurPic.linesize(1);
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;

    (*pCurDqLayer).iLumaStride = iLumaStride;
    (*pCurDqLayer).iChromaStride = iChromaStride;

    // T5.X8: `pPred[0..3]` was stamped here — the macroblock's top-left sample in
    // each plane, cached as three raw pointers under `bOutput && !data_ptr(0)
    // .is_null()`. It was a cache of `(iMbX, iMbY)`, which this layer already
    // carries, resolved against a picture the reconstruction path already has; the
    // field is deleted and its readers derive `plane_mut(i).cursor_mut(x, y)`. The
    // guard went with it: a null `data_ptr(0)` is an empty plane, and every read of
    // it now goes through `PaddedPlane`, which answers by bounds rather than by a
    // stale pointer.
    let _ = bOutput;
}

// **T5.X8: the intra reconstruction bracket.**
//
// This family reached the picture through `DqLayerState::pPred[3]` — three raw plane
// pointers stamped once per macroblock by `WelsFillRecNeededMbInfo` — and then
// offset them by `iDecBlockOffsetArray`, a 24-entry table of **byte** offsets that
// `WelsDecodeSlice` recomputed every time a stride changed. Both are cache, not
// carrier: the picture owns its planes, and a 4x4 block's position inside a
// macroblock is a pair of sample coordinates that no stride enters. So the family
// takes `pDec` and derives `plane_mut(i).cursor_mut(x, y)` at the block, and the
// dispatch tables hold the safe kernels themselves.
//
// The layer is read for small per-macroblock values (the modes, the availability
// mask, the non-zero counts) which are **copied**, and for the coefficient block,
// which is borrowed shared — the picture is a different object, so writing it
// through `pDec` while holding that borrow is not an aliasing question at all.

/// Sample coordinates of 4x4 block `i` inside its macroblock — `iDecBlockOffsetArray`
/// with the stride factored out (`i4_luma_ichroma_addr_table`'s `(x, y)` before it
/// multiplied `y` by the stride and shifted).
#[inline]
pub(crate) fn blk4_xy(i: usize) -> (isize, isize) {
    let a = g_kuiScan8[i] as usize - g_kuiScan8[0] as usize;
    (((a & 0x07) << 2) as isize, ((a >> 3) << 2) as isize)
}

pub fn RecChroma(
    iMBXY: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    let pIdctFourResAddPredFunc = pCtx.pIdctFourResAddPredFunc;
    let (mb_x, mb_y) = ((*pDqLayer).iMbX as isize, (*pDqLayer).iMbY as isize);
    let uiCbpC = ((*(*pDqLayer).grid.cbp.get(iMBXY as usize)) as u8) >> 4;

    if uiCbpC == 1 || uiCbpC == 2 {
        if let Some(func) = pIdctFourResAddPredFunc {
            let pNzc = *(*pDqLayer).grid.nzc.get(iMBXY as usize);
            let tcoeff: &[i16; 384] = (*pDqLayer).grid.scaled_tcoeff.get(iMBXY as usize);
            for i in 0..2 {
                let rs: &[i16; 64] = tcoeff[256 + (i << 6)..][..64].try_into().unwrap();
                let nzc: &[i8; 6] = pNzc[16 + 2 * i..][..6].try_into().unwrap();
                func(
                    &mut pDec.plane_mut(i + 1).cursor_mut(mb_x << 3, mb_y << 3),
                    rs,
                    nzc,
                );
            }
        }
    }
    ERR_NONE
}

pub fn RecI4x4Luma(
    iMBXY: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    let pIntra4x4PredMode = *(*pDqLayer).grid.intra4x4_final_mode.get(iMBXY as usize);
    let pNzc = *(*pDqLayer).grid.nzc.get(iMBXY as usize);
    let (mb_x, mb_y) = ((*pDqLayer).iMbX as isize, (*pDqLayer).iMbY as isize);
    let tcoeff: &[i16; 384] = (*pDqLayer).grid.scaled_tcoeff.get(iMBXY as usize);
    let pIdctResAddPredFunc = pCtx.pIdctResAddPredFunc;

    for i in 0..16 {
        let (dx, dy) = blk4_xy(i);
        let (x, y) = ((mb_x << 4) + dx, (mb_y << 4) + dy);
        let uiMode = pIntra4x4PredMode[g_kuiMbCountScan4Idx[i] as usize] as usize;

        if let Some(func) = pCtx.pGetI4x4LumaPredFunc[uiMode] {
            func(&mut pDec.plane_mut(0).cursor_mut(x, y));
        }

        let nzc_idx = g_kuiMbCountScan4Idx[i] as usize;
        if pNzc[nzc_idx] != 0 {
            if let Some(idct_func) = pIdctResAddPredFunc {
                let rs: &[i16; 16] = tcoeff[i << 4..][..16].try_into().unwrap();
                idct_func(&mut pDec.plane_mut(0).cursor_mut(x, y), rs);
            }
        }
    }
    ERR_NONE
}

pub fn RecI4x4Chroma(
    iMBXY: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    let iChromaPredMode = *(*pDqLayer).grid.chroma_pred_mode.get(iMBXY as usize) as usize;
    let (mb_x, mb_y) = ((*pDqLayer).iMbX as isize, (*pDqLayer).iMbY as isize);

    if let Some(func) = pCtx.pGetIChromaPredFunc[iChromaPredMode] {
        func(&mut pDec.plane_mut(1).cursor_mut(mb_x << 3, mb_y << 3));
        func(&mut pDec.plane_mut(2).cursor_mut(mb_x << 3, mb_y << 3));
    }

    RecChroma(iMBXY, pCtx, &mut *pDec, pDqLayer)
}

pub fn RecI4x4Mb(
    iMBXY: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    RecI4x4Luma(iMBXY, pCtx, &mut *pDec, pDqLayer);
    RecI4x4Chroma(iMBXY, pCtx, &mut *pDec, pDqLayer);
    ERR_NONE
}

pub fn RecI8x8Luma(
    iMbXy: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    let pIntra8x8PredMode = *(*pDqLayer).grid.intra4x4_final_mode.get(iMbXy as usize);
    let pNzc = *(*pDqLayer).grid.nzc.get(iMbXy as usize);
    let (mb_x, mb_y) = ((*pDqLayer).iMbX as isize, (*pDqLayer).iMbY as isize);
    let tcoeff: &[i16; 384] = (*pDqLayer).grid.scaled_tcoeff.get(iMbXy as usize);
    let pIdctResAddPredFunc = pCtx.pIdctResAddPredFunc8x8;

    let avail = *(*pDqLayer).grid.intra_nxn_avail_flag.get(iMbXy as usize);
    let bTLAvail: [bool; 4] = [
        (avail & 0x02) != 0,
        (avail & 0x01) != 0,
        (avail & 0x04) != 0,
        true,
    ];
    let bTRAvail: [bool; 4] = [
        (avail & 0x01) != 0,
        (avail & 0x08) != 0,
        true,
        false,
    ];

    for i in 0..4 {
        let (dx, dy) = blk4_xy(i << 2);
        let (x, y) = ((mb_x << 4) + dx, (mb_y << 4) + dy);
        let uiMode = pIntra8x8PredMode[g_kuiMbCountScan4Idx[i << 2] as usize] as usize;

        if let Some(func) = pCtx.pGetI8x8LumaPredFunc[uiMode] {
            func(
                &mut pDec.plane_mut(0).cursor_mut(x, y),
                bTLAvail[i],
                bTRAvail[i],
            );
        }

        let iIndex = g_kuiMbCountScan4Idx[i << 2] as usize;
        if pNzc[iIndex] != 0
            || pNzc[iIndex + 1] != 0
            || pNzc[iIndex + 4] != 0
            || pNzc[iIndex + 5] != 0
        {
            if let Some(idct_func) = pIdctResAddPredFunc {
                let rs: &[i16; 64] = tcoeff[i << 6..][..64].try_into().unwrap();
                idct_func(&mut pDec.plane_mut(0).cursor_mut(x, y), rs);
            }
        }
    }
    ERR_NONE
}

pub fn RecI8x8Mb(
    iMbXy: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    RecI8x8Luma(iMbXy, pCtx, &mut *pDec, pDqLayer);
    RecI4x4Chroma(iMbXy, pCtx, &mut *pDec, pDqLayer);
    ERR_NONE
}

pub fn RecI16x16Mb(
    iMBXY: i32,
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pDqLayer: &mut DqLayerState,
) -> i32 {
    let iI16x16PredMode = (*pDqLayer).grid.intra_pred_mode.get(iMBXY as usize)[7] as usize;
    let iChromaPredMode = *(*pDqLayer).grid.chroma_pred_mode.get(iMBXY as usize) as usize;
    let (mb_x, mb_y) = ((*pDqLayer).iMbX as isize, (*pDqLayer).iMbY as isize);
    let pIdctFourResAddPredFunc = pCtx.pIdctFourResAddPredFunc;

    if let Some(func) = pCtx.pGetI16x16LumaPredFunc[iI16x16PredMode] {
        func(&mut pDec.plane_mut(0).cursor_mut(mb_x << 4, mb_y << 4));
    }

    if let Some(idct_func) = pIdctFourResAddPredFunc {
        let pNzc = *(*pDqLayer).grid.nzc.get(iMBXY as usize);
        let tcoeff: &[i16; 384] = (*pDqLayer).grid.scaled_tcoeff.get(iMBXY as usize);
        for (q, (dx, dy, nz)) in [(0isize, 0isize, 0usize), (8, 0, 2), (0, 8, 8), (8, 8, 10)]
            .into_iter()
            .enumerate()
        {
            let rs: &[i16; 64] = tcoeff[q << 6..][..64].try_into().unwrap();
            let nzc: &[i8; 6] = pNzc[nz..][..6].try_into().unwrap();
            idct_func(
                &mut pDec
                    .plane_mut(0)
                    .cursor_mut((mb_x << 4) + dx, (mb_y << 4) + dy),
                rs,
                nzc,
            );
        }
    }

    if let Some(chroma_func) = pCtx.pGetIChromaPredFunc[iChromaPredMode] {
        chroma_func(&mut pDec.plane_mut(1).cursor_mut(mb_x << 3, mb_y << 3));
        chroma_func(&mut pDec.plane_mut(2).cursor_mut(mb_x << 3, mb_y << 3));
    }

    RecChroma(iMBXY, pCtx, &mut *pDec, pDqLayer);
    ERR_NONE
}

pub fn WelsMbIntraPredictionConstruction(
    pCtx: &mut SliceCtx<'_>,
    mut pDec: Option<&mut SPicture>,
    pCurDqLayer: &mut DqLayerState,
    bOutput: bool,
) -> i32 {
    let iMbXy = (*pCurDqLayer).iMbXyIndex;

    WelsFillRecNeededMbInfo(pCtx, pDec.as_deref_mut(), bOutput, pCurDqLayer);

    let Some(pDec) = pDec else {
        return ERR_NONE;
    };
    if pDec.pMbType.as_slice().is_empty() {
        return ERR_NONE;
    }
    let mb_type = *pDec.pMbType.get(iMbXy as usize);

    if IS_INTRA16x16(mb_type) {
        RecI16x16Mb(iMbXy, pCtx, &mut *pDec, pCurDqLayer);
    } else if IS_INTRA8x8(mb_type) {
        RecI8x8Mb(iMbXy, pCtx, &mut *pDec, pCurDqLayer);
    } else if IS_INTRA4x4(mb_type) {
        RecI4x4Mb(iMbXy, pCtx, &mut *pDec, pCurDqLayer);
    }
    ERR_NONE
}

pub fn WelsTargetMbConstruction(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: Option<&mut SPicture>,
    pRefs: PicRefs<'_>,
) -> i32 {
    let iMbXy = pCurDqLayer.iMbXyIndex as usize;

    // The C's `pDec == NULL` arm, as the `Option`.
    let Some(pDec) = pDec else {
        return ERR_NONE;
    };
    if pDec.pMbType.as_slice().is_empty() {
        return ERR_NONE;
    }
    let mb_type = *pDec.pMbType.get(iMbXy);

    if mb_type == MB_TYPE_INTRA_PCM {
        ERR_NONE
    } else if IS_INTRA(mb_type) {
        WelsMbIntraPredictionConstruction(pCtx, Some(pDec), pCurDqLayer, true);
        ERR_NONE
    } else if IS_INTER(mb_type) {
        // T5.H12: a `pCbp.is_null()` guard returning `ERR_INFO_MB_RECON_FAIL` sat
        // here. `WelsTargetMbConstruction` (`decode_slice.cpp:334-355`) has no such
        // test — the port invented it, and it could only fire if the array's
        // allocation had failed, which the C++ answers by dereferencing null. The
        // grid makes it unrepresentable: `cbp` is a `Vec` sized with the layer.
        let cbp = *pCurDqLayer.grid.cbp.get(iMbXy);
        if cbp == 0 {
            if !CheckRefPics(pCtx) {
                return ERR_INFO_MB_RECON_FAIL;
            }
            WelsMbInterPrediction(pCtx, pDec, pRefs, pCurDqLayer)
        } else {
            WelsMbInterConstruction(pCtx, pDec, pRefs, pCurDqLayer);
            ERR_NONE
        }
    } else {
        ERR_INFO_MB_RECON_FAIL
    }
}

pub fn WelsTargetSliceConstruction(pCtx: &mut SWelsDecoderContext, pCurDqLayer: &mut DqLayerState) -> i32 {
    {
        let dq: &mut DqLayerState = pCurDqLayer;

        if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref.is_none() {
            return ERR_NONE;
        }
        // **The split** (T5.Y2), the third of the three: this bracket reconstructs
        // rather than parses, and reaches the context for exactly what the view carries.
        let (pDec, pRefs, mut view, _nal) = slice_split(pCtx, None);
        // T5b.2: the picture is a borrow, so the one place its absence is tested is
        // here rather than at every level below. `DecodeCurrentAccessUnit` prefetches
        // it and returns `ERR_INFO_REF_COUNT_OVERFLOW` when the pool cannot supply
        // one, so this arm is unreachable from the decode path — same condition, same
        // code, stated once.
        let mut pDec = pDec;
        // The view's scope is the macroblock loop; the deblocking tail below it still
        // takes the context, and converts with the rest of `deblocking.rs`.
        let (iCurLayerWidth, iCurLayerHeight) = {
        let pCtx = &mut view;
        let iTotalMbTargetLayer = pCtx
            .sps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref)
            .map_or(0, |sps| sps.uiTotalMbCount as i32);


        let iCurLayerWidth = (*dq).iMbWidth << 4;
        let iCurLayerHeight = (*dq).iMbHeight << 4;

        let mut iNextMbXyIndex = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
        let iTotalNumMb = (*dq).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice;
        let mut iCountNumMb = 0;

        if !pCtx.sSpsPpsCtx.bAvcBasedFlag && iCurLayerWidth != pCtx.iCurSeqIntervalMaxPicWidth {
            return ERR_INFO_WIDTH_MISMATCH;
        }

        if (*dq).iMbWidth > 0 {
            (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
            (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
        }
        (*dq).iMbXyIndex = iNextMbXyIndex;

        if iNextMbXyIndex == 0 {
            if let Some(pDec) = pDec.as_deref_mut() {
                if let Some(sps) = pCtx.active_sps() {
                    pDec.iSpsId = sps.iSpsId;
                }
                if let Some(pps) = pCtx.active_pps() {
                    pDec.iPpsId = pps.iPpsId;
                }
                pDec.uiQualityId = (*dq).sLayerInfo.sNalHeaderExt.uiQualityId;
            }
        }

        loop {
            if iCountNumMb >= iTotalNumMb {
                break;
            }

            let bParseOnly = pCtx.bParseOnly;
            if !bParseOnly {
                let ret = WelsTargetMbConstruction(pCtx, dq, pDec.as_deref_mut(), pRefs);
                if ret != ERR_NONE {
                    return ERR_INFO_MB_RECON_FAIL;
                }
            }

            iCountNumMb += 1;
            let idx = iNextMbXyIndex as usize;
            if !*(*dq).grid.mb_correctly_decoded_flag.get(idx) {
                *(*dq).grid.mb_correctly_decoded_flag.get_mut(idx) = true;
                if *(*dq).grid.mb_ref_concealed_flag.get(idx) {
                    if let Some(pDec) = pDec.as_deref_mut() {
                        pDec.iMbEcedPropNum += 1;
                    }
                }
                *pCtx.iTotalNumMbRec += 1;
            }

            if *pCtx.iTotalNumMbRec > iTotalMbTargetLayer {
                return ERR_INFO_MB_NUM_EXCEED_FAIL;
            }

            if pCtx
                .pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id)
                .is_some_and(|pps| pps.uiNumSliceGroups > 1)
            {
                iNextMbXyIndex = crate::decoder::fmo::FmoNextMb(pCtx.active_fmo(), iNextMbXyIndex);
            } else {
                iNextMbXyIndex += 1;
            }
            if iNextMbXyIndex == -1 || iNextMbXyIndex >= iTotalMbTargetLayer {
                break;
            }
            if (*dq).iMbWidth > 0 {
                (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
                (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
            }
            (*dq).iMbXyIndex = iNextMbXyIndex;
        }
        (iCurLayerWidth, iCurLayerHeight)
        };

        if let Some(pDec) = pDec.as_deref_mut() {
            pDec.iWidthInPixel = iCurLayerWidth;
            pDec.iHeightInPixel = iCurLayerHeight;
        }

        if (*dq).sLayerInfo.sSliceInLayer.eSliceType != EWelsSliceType::I_SLICE as u8
            && (*dq).sLayerInfo.sSliceInLayer.eSliceType != EWelsSliceType::P_SLICE as u8
            && (*dq).sLayerInfo.sSliceInLayer.eSliceType != EWelsSliceType::B_SLICE as u8
        {
            return ERR_NONE;
        }

        if crate::decoder::decoder_context::parse_only(&pCtx.pParam) {
            return ERR_NONE;
        }

        if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc == 1 || (*dq).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice <= 0 {
            return ERR_NONE;
        } else {
            // **The deblocking bracket** (T5.AA2): its own split, taken here rather than
            // inherited, because the reconstruction bracket above closed with the view.
            // The picture is a borrow and the view is what deblocking reads of the
            // context; `None` is no pool or no current picture, and the family below
            // has nothing to filter then.
            let (pDec, view) = crate::decoder::decoder_context::pic_split(pCtx);
            if let Some(pDec) = pDec {
                crate::decoder::deblocking::WelsDeblockingFilterSlice(
                    &view, dq,
                    &mut *pDec,
                    Some(crate::decoder::deblocking::WelsDeblockingMb),
                );
            }
        }

        ERR_NONE
    }
}

// ============================================================================
// Entropy Slice Decoding (CAVLC / CABAC Dispatch)
// ============================================================================

/// Copy an I_PCM macroblock's raw samples from the bitstream into the decoded
/// picture and update QP/NZC state. Shared by the I- and P-slice CAVLC paths.
/// Matches the `25 == uiMbType` branch of `WelsActualDecodeMbCavlcISlice` /
/// `WelsActualDecodeMbCavlcPSlice` in `decode_slice.cpp`.
/// **The reader arrives as parameters — F47's second instance.** This opened by
/// re-deriving `&mut *slice_bit_reader(pCtx)` below callers that already hold the
/// split, which removes their strongly-protected `&mut` argument. The all-I_PCM
/// FMO asset is what reached it: no probe before T5.S2 decoded a PCM macroblock.
fn DecodeMbCavlcPcm(pCtx: &mut SliceCtx<'_>, buf: &[u8], pBs: &mut BsCursor, dq: &mut DqLayerState, pDec: &mut SPicture) -> i32 {
    {
        let iMbX = (*dq).iMbX;
        let iMbY = (*dq).iMbY;
        let iMbXy = (*dq).iMbXyIndex as usize;

        // T5b.6: the macroblock's top-left in plane coordinates. The C++ computes
        // `(iMbX + iMbY * stride) << 4` — one linear offset off `pData[i]` — and the
        // two halves of it are exactly `x = iMbX * 16` and `y = iMbY * 16`, which is
        // what a padded plane is addressed by.
        let (iPcmX, iPcmY) = ((iMbX as isize) << 4, (iMbY as isize) << 4);
        let (iPcmXC, iPcmYC) = ((iMbX as isize) << 3, (iMbY as isize) << 3);

        let iIndex = ((-pBs.left_bits()) >> 3) + 2;

        *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA_PCM;

        // step 1: locate the bit-stream position (must align to an integer byte).
        // `pCurBuf - iIndex` becomes `pos - iIndex`; the C++ computed a pointer here and a
        // negative result was an out-of-bounds pointer with no check, so an underflow is a
        // pre-existing overrun surfacing (plan §2.2.2) — `pos` is `usize` and the slice
        // index below is what reports it.
        let iPcmStart = (pBs.pos() as isize - iIndex as isize) as usize;
        pBs.set_pos(iPcmStart);

        // step 2: copy pixels from the bit-stream into the decoded picture.
        //
        // **The 384 bytes are taken as one window.** The C++ walks a pointer off
        // `pCurBuf - iIndex` and reads 384 bytes with no test at all, so a PCM
        // macroblock announced within `iIndex` of the end of the RBSP reads past the
        // allocation. Here the window either exists or the copy does not run — and
        // the error the C++ eventually reports is unchanged either way, because
        // `InitReadBits` below is handed `iPcmStart + 384` and fails on exactly that
        // arithmetic.
        let bParseOnly = pCtx.bParseOnly;
        if !bParseOnly {
            if let Some(pcm) = buf.get(iPcmStart..iPcmStart + 384) {
                for r in 0..16 {
                    pDec.plane_mut(0)
                        .row_mut(iPcmY + r as isize, iPcmX, 16)
                        .copy_from_slice(&pcm[r * 16..][..16]);
                }
                for r in 0..8 {
                    pDec.plane_mut(1)
                        .row_mut(iPcmYC + r as isize, iPcmXC, 8)
                        .copy_from_slice(&pcm[256 + r * 8..][..8]);
                }
                for r in 0..8 {
                    pDec.plane_mut(2)
                        .row_mut(iPcmYC + r as isize, iPcmXC, 8)
                        .copy_from_slice(&pcm[320 + r * 8..][..8]);
                }
            }
        }

        pBs.set_pos(iPcmStart + 384);

        // step 3: update QP and non-zero counts (Rec. 9.2.1: for PCM, nzc = 16)
        *(*dq).grid.luma_qp.get_mut(iMbXy) = 0;
        (*dq).grid.chroma_qp.get_mut(iMbXy)[0] = 0;
        (*dq).grid.chroma_qp.get_mut(iMbXy)[1] = 0;
        let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
        pNzc.fill(16);
        let ret = crate::decoder::bit_stream::InitReadBits(buf, pBs, 0);
        if ret != 0 {
            return ret;
        }
        ERR_NONE
    }
}

/// Matches `WelsActualDecodeMbCavlcISlice` in `decode_slice.cpp`.
pub fn WelsActualDecodeMbCavlcISlice(pCtx: &mut SliceCtx<'_>, buf: &[u8], pBs: &mut BsCursor, dq: &mut DqLayerState, pDec: &mut SPicture) -> i32 {
    {
        // T5.W4: the table is read-only — no `SVlcTable` field is written outside
        // `InitVlcTable` — so the derivation is a shared borrow and the callees take one.
        let pVlcTable = pCtx.pVlcTable;

        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;

        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut uiCode = 0u32;
        let mut iCode = 0i32;
        let mut uiCbp;
        let mut uiCbpC = 0u32;
        let mut uiCbpL = 0u32;

        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut pNonZeroCount = [0u8; 48];
        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, Some(&*dq), Some(&*pDec));

        // T5.I3's two macroblock windows stood here as `&mut` cursors into `grid.cbp`
        // and `grid.transform_size8x8_flag`, held across the whole parse. **T5.X3**: the
        // layer is a borrow now, so a cursor's span is the layer's span, and every call
        // below that takes the layer conflicts with it. Both are re-derived per use
        // instead — S29's spelling, and the same one expression per access the C++'s
        // `pCurDqLayer->pCbp[iMbXy]` compiles to. Nothing between a write and a read of
        // either field writes it, so the values read are the cursors'.
        *(*dq).grid.residual_pred_flag.get_mut(iMbXy) = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultResidualPredFlag as i8;

        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;

        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        let mut uiMbType = uiCode;
        if uiMbType > 25 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }
        if pCtx.uiChromaFormatIdc() == 0
            && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }

        if 25 == uiMbType {
            return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, &mut *pDec);
        } else if 0 == uiMbType {
            let mut pIntraPredMode = [0i8; 48];
            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
            if pCtx.bTransform8x8ModeFlag() {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = uiCode != 0;
                if uiCode != 0 {
                    *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                    uiMbType = MB_TYPE_INTRA8x8;
                }
            }
            pCtx.eIntraPredConstraint.FillCacheIntraNxN(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                &mut pIntraPredMode,
                dq,
            );
            let ret = if !*(*dq).grid.transform_size8x8_flag.get(iMbXy) {
                ParseIntra4x4Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
            } else {
                ParseIntra8x8Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
            };
            if ret != ERR_NONE {
                return ret;
            }

            let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            uiCbp = uiCode;
            if pCtx.uiChromaFormatIdc() != 0 && uiCbp > 47 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            if pCtx.uiChromaFormatIdc() == 0 && uiCbp > 15 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            uiCbp = if pCtx.uiChromaFormatIdc() != 0 {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
            } else {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
            };
            *(*dq).grid.cbp.get_mut(iMbXy) = uiCbp as i8;
            uiCbpC = uiCbp >> 4;
            uiCbpL = uiCbp & 15;
        } else {
            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
            *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
            *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
            (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
            *(*dq).grid.cbp.get_mut(iMbXy) = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
            uiCbpC = if pCtx.uiChromaFormatIdc() != 0 {
                (*(*dq).grid.cbp.get(iMbXy) as u32) >> 4
            } else {
                0
            };
            uiCbpL = (*(*dq).grid.cbp.get(iMbXy) as u32) & 15;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                Some(&*dq),
            );
            let ret = { ParseIntra16x16Mode(pCtx, &mut *pDec, &mut sNeighAvail, buf, pBs, dq) };
            if ret != ERR_NONE {
                return ret;
            }
        }

        let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
        pNzc.fill(0);

        if *(*dq).grid.cbp.get(iMbXy) == 0 && IS_INTRANxN(*pDec.pMbType.get(iMbXy)) {
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            *(*dq).grid.luma_qp.get_mut(iMbXy) = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp as i8;
            for i in 0..2 {
                let idx = WELS_CLIP3(
                    *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh_chroma_qp_offset[i] as i32,
                    0,
                    51,
                );
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }
        }

        if *(*dq).grid.cbp.get(iMbXy) != 0 || MB_TYPE_INTRA16x16 == *pDec.pMbType.get(iMbXy) {
            let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
            scaled_tcoeff_mb.fill(0);

            let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
            if ret != 0 {
                return ret;
            }
            let iQpDelta = iCode;
            if iQpDelta > 25 || iQpDelta < -26 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_QP);
            }
            let new_qp = ((*dq).sLayerInfo.sSliceInLayer.iLastMbQp + iQpDelta + 52) % 52;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
            (*dq).sLayerInfo.sSliceInLayer.iLastMbQp = new_qp;
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx = WELS_CLIP3(new_qp + pps_sh_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            pBs.start_cavlc();

            let ret = { WelsDecodeMbCavlcResidual(
                pCtx,
                buf,
                pBs,
                dq,
                &mut *pDec,
                pVlcTable,
                &mut pNonZeroCount,
                iScanIdxStart,
                iScanIdxEnd,
                uiCbpL,
                uiCbpC,
            ) };
            if ret != ERR_NONE {
                return ret;
            }

            pBs.end_cavlc(buf);
        }

        ERR_NONE
    }
}

/// Shared CAVLC residual decode for luma + chroma blocks of one macroblock.
/// Matches the residual sections of `WelsActualDecodeMbCavlcISlice` /
/// `WelsActualDecodeMbCavlcPSlice` in `decode_slice.cpp` (after `BsStartCavlc`,
/// up to `BsEndCavlc`).
///
/// **The reader arrives as parameters — F47.** This function used to open with its
/// own `pNalCur.sNalData.sVclNal.sSliceBitsRead.split(pCtx.sRawData)`, and all three of its
/// callers had already taken that same split at their own heads. Two live `&mut`
/// derivations of one `BsCursor` through a raw pointer: the callee's function-entry
/// retag popped the caller's tag, and the caller then used it again —
/// `pBs.end_cavlc(buf)` — which is Undefined Behaviour on the ordinary CAVLC path,
/// every macroblock that carries residual. Threading the split down is the same
/// bracket maneuver W3 used: derive once at the top, pass it, touch the source
/// nowhere below.
fn WelsDecodeMbCavlcResidual(
    pCtx: &mut SliceCtx<'_>,
    buf: &[u8],
    pBs: &mut BsCursor,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pVlcTable: &crate::decoder::parse_mb_syn_cavlc::SVlcTable,
    pNonZeroCount: &mut [u8; 48],
    iScanIdxStart: usize,
    iScanIdxEnd: usize,
    uiCbpL: u32,
    uiCbpC: u32,
) -> i32 {
    let iMbXy = (*dq).iMbXyIndex as usize;
    let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
    let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
    let mb_type = *pDec.pMbType.get(iMbXy);
    let is_intra = IS_INTRA(mb_type);
    // T5.I2: the QP is read once per residual block, so the four sites below sit
    // inside `for iId8x8 { for iId4x4 { } }` and cost up to sixteen checks per
    // macroblock. Nothing here writes the family and `WelsResidualBlockCavlc*` does
    // not reach it, so one shared window serves the whole function — this is the
    // same hoist `pNzc` and `scaled_tcoeff_mb` already have on the two lines above.
    let iLumaQp = (*dq).grid.luma_qp.get(iMbXy);

    // T5.R7: the cache is a `[u8; 48]` and the grid's row is an `[i8; 24]`, so the
    // C's `ST32`/`ST16` writes are four- and two-element copies between two arrays
    // whose elements differ only in signedness. The `as i8` per element is the same
    // reinterpretation the pointer cast was, spelled where it happens.
    let copy4 = |dst: &mut [i8], at: usize, src: &[u8; 48], from: usize| {
        for k in 0..4 {
            dst[at + k] = src[from + k] as i8;
        }
    };
    let copy2 = |dst: &mut [i8], at: usize, src: &[u8; 48], from: usize| {
        for k in 0..2 {
            dst[at + k] = src[from + k] as i8;
        }
    };

    if MB_TYPE_INTRA16x16 == mb_type {
        // step 1: luma DC
        let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc(
            pVlcTable,
            pNonZeroCount,
            buf,
            pBs,
            0,
            16,
            &g_kuiLumaDcZigzagScan,
            I16_LUMA_DC,
            &mut scaled_tcoeff_mb[..],
            *iLumaQp as u8,
            pCtx,
        );
        if ret != ERR_NONE {
            return ret;
        }
        // step 2: luma AC
        if uiCbpL != 0 {
            let max_idx = std::cmp::max(iScanIdxStart, 1);
            for i in 0..16 {
                let len = (iScanIdxEnd as isize - max_idx as isize + 1) as i32;
                let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc(
                    pVlcTable,
                    pNonZeroCount,
                    buf,
                    pBs,
                    i as i32,
                    len,
                    &g_kuiZigzagScan[max_idx..],
                    I16_LUMA_AC,
                    &mut scaled_tcoeff_mb[i << 4..],
                    *iLumaQp as u8,
                    pCtx,
                );
                if ret != ERR_NONE {
                    return ret;
                }
            }
            copy4(pNzc, 0, pNonZeroCount, 1 + 8);
            copy4(pNzc, 4, pNonZeroCount, 1 + 8 * 2);
            copy4(pNzc, 8, pNonZeroCount, 1 + 8 * 3);
            copy4(pNzc, 12, pNonZeroCount, 1 + 8 * 4);
        }
    } else {
        // non-INTRA16x16
        if *(*dq).grid.transform_size8x8_flag.get(iMbXy) {
            let iMbResProperty = if is_intra { LUMA_DC_AC_INTRA_8 } else { LUMA_DC_AC_INTER_8 };
            for iId8x8 in 0..4usize {
                if (uiCbpL & (1 << iId8x8)) != 0 {
                    let mut iIndex = (iId8x8 << 2) as i32;
                    for iId4x4 in 0..4 {
                        let len = (iScanIdxEnd as isize - iScanIdxStart as isize + 1) as i32;
                        let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc8x8(
                            pVlcTable,
                            pNonZeroCount,
                            buf,
                            pBs,
                            iIndex,
                            len,
                            &g_kuiZigzagScan8x8[iScanIdxStart..],
                            iMbResProperty,
                            &mut scaled_tcoeff_mb[iId8x8 << 6..],
                            iId4x4,
                            *iLumaQp as u8,
                            pCtx,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                        iIndex += 1;
                    }
                } else {
                    let idx0 = crate::decoder::parse_mb_syn_cavlc::g_kuiCache48CountScan4Idx[iId8x8 << 2] as usize;
                    let idx2 = crate::decoder::parse_mb_syn_cavlc::g_kuiCache48CountScan4Idx[(iId8x8 << 2) + 2] as usize;
                    pNonZeroCount[idx0] = 0;
                    pNonZeroCount[idx0 + 1] = 0;
                    pNonZeroCount[idx2] = 0;
                    pNonZeroCount[idx2 + 1] = 0;
                }
            }
            copy4(pNzc, 0, pNonZeroCount, 1 + 8);
            copy4(pNzc, 4, pNonZeroCount, 1 + 8 * 2);
            copy4(pNzc, 8, pNonZeroCount, 1 + 8 * 3);
            copy4(pNzc, 12, pNonZeroCount, 1 + 8 * 4);
        } else {
            let iMbResProperty = if is_intra { LUMA_DC_AC_INTRA } else { LUMA_DC_AC_INTER };
            for iId8x8 in 0..4usize {
                if (uiCbpL & (1 << iId8x8)) != 0 {
                    let mut iIndex = (iId8x8 << 2) as i32;
                    for _iId4x4 in 0..4 {
                        let len = (iScanIdxEnd as isize - iScanIdxStart as isize + 1) as i32;
                        let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc(
                            pVlcTable,
                            pNonZeroCount,
                            buf,
                            pBs,
                            iIndex,
                            len,
                            &g_kuiZigzagScan[iScanIdxStart..],
                            iMbResProperty,
                            &mut scaled_tcoeff_mb[(iIndex as usize) << 4..],
                            *iLumaQp as u8,
                            pCtx,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                        iIndex += 1;
                    }
                } else {
                    let idx0 = crate::decoder::parse_mb_syn_cavlc::g_kuiCache48CountScan4Idx[iId8x8 << 2] as usize;
                    let idx2 = crate::decoder::parse_mb_syn_cavlc::g_kuiCache48CountScan4Idx[(iId8x8 << 2) + 2] as usize;
                    pNonZeroCount[idx0] = 0;
                    pNonZeroCount[idx0 + 1] = 0;
                    pNonZeroCount[idx2] = 0;
                    pNonZeroCount[idx2 + 1] = 0;
                }
            }
            copy4(pNzc, 0, pNonZeroCount, 1 + 8);
            copy4(pNzc, 4, pNonZeroCount, 1 + 8 * 2);
            copy4(pNzc, 8, pNonZeroCount, 1 + 8 * 3);
            copy4(pNzc, 12, pNonZeroCount, 1 + 8 * 4);
        }
    }

    // chroma
    // step 1: DC
    if 1 == uiCbpC || 2 == uiCbpC {
        for i in 0..2usize {
            let iMbResProperty = if is_intra {
                if i != 0 { CHROMA_DC_V } else { CHROMA_DC_U }
            } else {
                if i != 0 { CHROMA_DC_V_INTER } else { CHROMA_DC_U_INTER }
            };
            let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc(
                pVlcTable,
                pNonZeroCount,
                buf,
                pBs,
                (16 + (i << 2)) as i32,
                4,
                &g_kuiChromaDcScan,
                iMbResProperty,
                &mut scaled_tcoeff_mb[256 + (i << 6)..],
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                pCtx,
            );
            if ret != ERR_NONE {
                return ret;
            }
        }
    }
    // step 2: AC
    if 2 == uiCbpC {
        let max_idx = std::cmp::max(iScanIdxStart, 1);
        for i in 0..2usize {
            let iMbResProperty = if is_intra {
                if i != 0 { CHROMA_AC_V } else { CHROMA_AC_U }
            } else {
                if i != 0 { CHROMA_AC_V_INTER } else { CHROMA_AC_U_INTER }
            };
            let mut iIndex = 16 + (i << 2);
            for _iId4x4 in 0..4 {
                let len = (iScanIdxEnd as isize - max_idx as isize + 1) as i32;
                let ret = crate::decoder::parse_mb_syn_cavlc::WelsResidualBlockCavlc(
                    pVlcTable,
                    pNonZeroCount,
                    buf,
                    pBs,
                    iIndex as i32,
                    len,
                    &g_kuiZigzagScan[max_idx..],
                    iMbResProperty,
                    &mut scaled_tcoeff_mb[iIndex << 4..],
                    (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                    pCtx,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                iIndex += 1;
            }
        }
        copy2(pNzc, 16, pNonZeroCount, 6 + 8);
        copy2(pNzc, 20, pNonZeroCount, 6 + 8 * 2);
        copy2(pNzc, 18, pNonZeroCount, 6 + 8 * 4);
        copy2(pNzc, 22, pNonZeroCount, 6 + 8 * 5);
    }

    ERR_NONE
}
pub fn WelsDecodeMbCavlcISlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let (buf, pBs) = pNalCur.sNalData.sVclNal.sSliceBitsRead.split(pCtx.sRawData);
        let mut uiCode = 0u32;
        let iBaseModeFlag;
        if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveBaseModeFlag {
            if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
                return ERR_INFO_INVALID_ACCESS;
            }
            iBaseModeFlag = uiCode != 0;
        } else {
            iBaseModeFlag = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultBaseModeFlag;
        }
        if iBaseModeFlag {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_ILP);
        }
        let ret = WelsActualDecodeMbCavlcISlice(pCtx, buf, pBs, dq, &mut *pDec);
        if ret != ERR_NONE {
            return ret;
        }
        let iUsedBits = (pBs.pos() as i32) * 8 - (16 - pBs.left_bits());
        if iUsedBits == (pBs.bits() - 1) && (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun <= 0 {
            if true {
                *uiEosFlag = 1;
            }
        }
        if iUsedBits > (pBs.bits() - 1) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
        }
        ERR_NONE
    }
}

/// Matches `WelsActualDecodeMbCavlcPSlice` in `decode_slice.cpp`.
pub fn WelsActualDecodeMbCavlcPSlice(pCtx: &mut SliceCtx<'_>, buf: &[u8], pBs: &mut BsCursor, dq: &mut DqLayerState, pDec: &mut SPicture, pRefs: PicRefs<'_>) -> i32 {
    {
        // T5.W4: the table is read-only — no `SVlcTable` field is written outside
        // `InitVlcTable` — so the derivation is a shared borrow and the callees take one.
        let pVlcTable = pCtx.pVlcTable;

        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;

        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut uiCode = 0u32;
        let mut iCode = 0i32;
        let mut uiCbp;
        let mut uiCbpC = 0u32;
        let mut uiCbpL = 0u32;

        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut pNonZeroCount = [0u8; 48];
        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, Some(&*dq), Some(&*pDec));

        // T5.I3's two macroblock windows stood here as `&mut` cursors into `grid.cbp`
        // and `grid.transform_size8x8_flag`, held across the whole parse. **T5.X3**: the
        // layer is a borrow now, so a cursor's span is the layer's span, and every call
        // below that takes the layer conflicts with it. Both are re-derived per use
        // instead — S29's spelling, and the same one expression per access the C++'s
        // `pCurDqLayer->pCbp[iMbXy]` compiles to. Nothing between a write and a read of
        // either field writes it, so the values read are the cursors'.

        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        let mut uiMbType = uiCode;
        if uiMbType < 5 {
            // inter MB type
            let mut iMotionVector = [[[0i16; 2]; 30]; 2];
            let mut iRefIndex = [[0i8; 30]; 2];
            *pDec.pMbType.get_mut(iMbXy) = g_ksInterPMbTypeInfo[uiMbType as usize].iType;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInter(
                &sNeighAvail,
                &mut pNonZeroCount,
                &mut iMotionVector,
                &mut iRefIndex,
                &*dq,
                &*pDec,
            );

            let ret = crate::decoder::parse_mb_syn_cavlc::ParseInterInfo(
                pCtx, &mut *dq,
                &mut *pDec,
                pRefs,
                &mut iMotionVector,
                &mut iRefIndex,
                buf,
                pBs,
            );
            if ret != ERR_NONE {
                return ret;
            }

            // T5.I4: one window over the write-then-test — three checks became one.
            let pResidualPredFlag = (*dq).grid.residual_pred_flag.get_mut(iMbXy);
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveResidualPredFlag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *pResidualPredFlag = uiCode as i8;
            } else {
                *pResidualPredFlag = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultResidualPredFlag as i8;
            }

            if *pResidualPredFlag == 0 {
                // T5.H1: the arm's only statement was a write to `pInterPredictionDoneFlag`,
                // which nothing in either tree reads. The `if` stays: its `else` is the error.
            } else {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
            }
        } else {
            // intra MB type
            uiMbType -= 5;
            if uiMbType > 25 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if pCtx.uiChromaFormatIdc() == 0
                && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
            {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }

            if 25 == uiMbType {
                return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, &mut *pDec);
            } else if 0 == uiMbType {
                let mut pIntraPredMode = [0i8; 48];
                *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
                if pCtx.bTransform8x8ModeFlag() {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = uiCode != 0;
                    if uiCode != 0 {
                        *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                    }
                }
                pCtx.eIntraPredConstraint.FillCacheIntraNxN(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                &mut pIntraPredMode,
                dq,
            );
                let ret = if !*(*dq).grid.transform_size8x8_flag.get(iMbXy) {
                    ParseIntra4x4Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
                } else {
                    ParseIntra8x8Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
                };
                if ret != ERR_NONE {
                    return ret;
                }
            } else {
                *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
                *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
                (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
                *(*dq).grid.cbp.get_mut(iMbXy) = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
                uiCbpC = if pCtx.uiChromaFormatIdc() != 0 {
                    (*(*dq).grid.cbp.get(iMbXy) as u32) >> 4
                } else {
                    0
                };
                uiCbpL = (*(*dq).grid.cbp.get(iMbXy) as u32) & 15;
                crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                    &mut sNeighAvail,
                    &mut pNonZeroCount,
                    Some(&*dq),
                );
                let ret = ParseIntra16x16Mode(pCtx, &mut *pDec, &mut sNeighAvail, buf, pBs, dq);
                if ret != ERR_NONE {
                    return ret;
                }
            }
        }

        if MB_TYPE_INTRA16x16 != *pDec.pMbType.get(iMbXy) {
            let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            uiCbp = uiCode;
            if pCtx.uiChromaFormatIdc() != 0 && uiCbp > 47 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            if pCtx.uiChromaFormatIdc() == 0 && uiCbp > 15 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            let mb_type = *pDec.pMbType.get(iMbXy);
            uiCbp = if MB_TYPE_INTRA4x4 == mb_type || MB_TYPE_INTRA8x8 == mb_type {
                if pCtx.uiChromaFormatIdc() != 0 {
                    crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
                } else {
                    crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
                }
            } else {
                if pCtx.uiChromaFormatIdc() != 0 {
                    crate::decoder::dec_golomb::g_kuiInterCbpTable[uiCbp as usize] as u32
                } else {
                    crate::decoder::dec_golomb::g_kuiInterCbpTable400[uiCbp as usize] as u32
                }
            };

            *(*dq).grid.cbp.get_mut(iMbXy) = uiCbp as i8;
            uiCbpC = uiCbp >> 4;
            uiCbpL = uiCbp & 15;

            let mb_type = *pDec.pMbType.get(iMbXy);
            let bNeedParseTransformSize8x8Flag = ((mb_type >= MB_TYPE_16x16 && mb_type <= MB_TYPE_8x16)
                || *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get(iMbXy))
                && mb_type != MB_TYPE_INTRA8x8
                && mb_type != MB_TYPE_INTRA4x4
                && uiCbpL > 0
                && pCtx.bTransform8x8ModeFlag();

            if bNeedParseTransformSize8x8Flag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = uiCode != 0;
            }
        }

        let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
        pNzc.fill(0);

        let mb_type = *pDec.pMbType.get(iMbXy);
        if *(*dq).grid.cbp.get(iMbXy) == 0 && !IS_INTRA16x16(mb_type) && mb_type != MB_TYPE_INTRA_BL {
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            *(*dq).grid.luma_qp.get_mut(iMbXy) = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp as i8;
            for i in 0..2 {
                let idx = WELS_CLIP3(
                    *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh_chroma_qp_offset[i] as i32,
                    0,
                    51,
                );
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }
        }

        if *(*dq).grid.cbp.get(iMbXy) != 0 || MB_TYPE_INTRA16x16 == *pDec.pMbType.get(iMbXy) {
            let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
            scaled_tcoeff_mb.fill(0);

            let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
            if ret != 0 {
                return ret;
            }
            let iQpDelta = iCode;
            if iQpDelta > 25 || iQpDelta < -26 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_QP);
            }
            let new_qp = ((*dq).sLayerInfo.sSliceInLayer.iLastMbQp + iQpDelta + 52) % 52;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
            (*dq).sLayerInfo.sSliceInLayer.iLastMbQp = new_qp;
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx = WELS_CLIP3(new_qp + pps_sh_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            pBs.start_cavlc();

            let ret = WelsDecodeMbCavlcResidual(
                pCtx,
                buf,
                pBs,
                dq,
                &mut *pDec,
                pVlcTable,
                &mut pNonZeroCount,
                iScanIdxStart,
                iScanIdxEnd,
                uiCbpL,
                uiCbpC,
            );
            if ret != ERR_NONE {
                return ret;
            }

            pBs.end_cavlc(buf);
        }

        ERR_NONE
    }
}
pub fn WelsDecodeMbCavlcPSlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let (buf, pBs) = pNalCur.sNalData.sVclNal.sSliceBitsRead.split(pCtx.sRawData);
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut uiCode = 0u32;

        if (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun == -1 {
            if crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode) != 0 {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun = uiCode as i32;
            if (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun == -1 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_PARAM);
            }
        }

        // C++ uses `if (pSlice->iMbSkipRun--)`: a coded macroblock leaves the
        // counter at -1 so the next macroblock parses a fresh mb_skip_run.
        let bSkip = (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun != 0;
        (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun -= 1;
        if bSkip {
            let mut iMv = [0i16; 2];

            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_SKIP;
            (*dq).grid.nzc.get_mut(iMbXy).fill(0);
            pDec.pRefIndex[0].get_mut(iMbXy).fill(0);
            crate::decoder::mv_pred::PredPSkipMvFromNeighbor(&mut *dq, Some(&*pDec), &mut iMv);
            pDec.pMv[0].get_mut(iMbXy).fill(iMv);

            let iLastMbQp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = iLastMbQp as i8;
            let pps_ptr_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            for i in 0..2 {
                let offset = pps_ptr_chroma_qp_offset[i];
                let qp_idx = WELS_CLIP3(iLastMbQp as i32 + offset as i32, 0, 51) as usize;
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[qp_idx] as i8;
            }

            *(*dq).grid.cbp.get_mut(iMbXy) = 0;
        } else {
            let iBaseModeFlag;
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveBaseModeFlag {
                if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
                    return ERR_INFO_INVALID_ACCESS;
                }
                iBaseModeFlag = uiCode != 0;
            } else {
                iBaseModeFlag = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultBaseModeFlag;
            }
            if iBaseModeFlag {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_ILP);
            }
            let ret = WelsActualDecodeMbCavlcPSlice(pCtx, buf, pBs, dq, pDec, pRefs);
            if ret != ERR_NONE {
                return ret;
            }
        }

        let iUsedBits = (pBs.pos() as i32) * 8 - (16 - pBs.left_bits());
        if iUsedBits == (pBs.bits() - 1) && (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun <= 0 {
            if true {
                *uiEosFlag = 1;
            }
        }
        if iUsedBits > (pBs.bits() - 1) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
        }
        ERR_NONE
    }
}

/// Matches `WelsActualDecodeMbCavlcBSlice` in `decode_slice.cpp`.
///
/// Identical to [`WelsActualDecodeMbCavlcPSlice`] apart from the inter/intra
/// `mb_type` split (23 instead of 5), the mb-type table and the motion parser,
/// so the residual half is shared through [`WelsDecodeMbCavlcResidual`].
pub fn WelsActualDecodeMbCavlcBSlice(pCtx: &mut SliceCtx<'_>, buf: &[u8], pBs: &mut BsCursor, dq: &mut DqLayerState, pDec: &mut SPicture, pRefs: PicRefs<'_>) -> i32 {
    {
        // T5.W4: the table is read-only — no `SVlcTable` field is written outside
        // `InitVlcTable` — so the derivation is a shared borrow and the callees take one.
        let pVlcTable = pCtx.pVlcTable;

        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;

        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut uiCode = 0u32;
        let mut iCode = 0i32;
        let mut uiCbp;
        let mut uiCbpC = 0u32;
        let mut uiCbpL = 0u32;

        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut pNonZeroCount = [0u8; 48];
        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, Some(&*dq), Some(&*pDec));

        // T5.I3's two macroblock windows stood here as `&mut` cursors into `grid.cbp`
        // and `grid.transform_size8x8_flag`, held across the whole parse. **T5.X3**: the
        // layer is a borrow now, so a cursor's span is the layer's span, and every call
        // below that takes the layer conflicts with it. Both are re-derived per use
        // instead — S29's spelling, and the same one expression per access the C++'s
        // `pCurDqLayer->pCbp[iMbXy]` compiles to. Nothing between a write and a read of
        // either field writes it, so the values read are the cursors'.

        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        let mut uiMbType = uiCode;
        if uiMbType < 23 {
            // inter MB type
            let mut iMotionVector = [[[0i16; 2]; 30]; 2];
            let mut iRefIndex = [[0i8; 30]; 2];
            *pDec.pMbType.get_mut(iMbXy) = g_ksInterBMbTypeInfo[uiMbType as usize].iType;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInter(
                &sNeighAvail,
                &mut pNonZeroCount,
                &mut iMotionVector,
                &mut iRefIndex,
                &*dq,
                &*pDec,
            );

            let ret = crate::decoder::parse_mb_syn_cavlc::ParseInterBInfo(
                pCtx, &mut *dq,
                &mut *pDec,
                pRefs,
                &mut iMotionVector,
                &mut iRefIndex,
                buf,
                pBs,
            );
            if ret != ERR_NONE {
                return ret;
            }

            // T5.I4: one window over the write-then-test — three checks became one.
            let pResidualPredFlag = (*dq).grid.residual_pred_flag.get_mut(iMbXy);
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveResidualPredFlag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *pResidualPredFlag = uiCode as i8;
            } else {
                *pResidualPredFlag = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultResidualPredFlag as i8;
            }

            if *pResidualPredFlag == 0 {
                // T5.H1: the arm's only statement was a write to `pInterPredictionDoneFlag`,
                // which nothing in either tree reads. The `if` stays: its `else` is the error.
            } else {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
            }
        } else {
            // intra MB type
            uiMbType -= 23;
            if uiMbType > 25 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if pCtx.uiChromaFormatIdc() == 0
                && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
            {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }

            if 25 == uiMbType {
                return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, &mut *pDec);
            } else if 0 == uiMbType {
                let mut pIntraPredMode = [0i8; 48];
                *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
                if pCtx.bTransform8x8ModeFlag() {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = uiCode != 0;
                    if uiCode != 0 {
                        *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                    }
                }
                pCtx.eIntraPredConstraint.FillCacheIntraNxN(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                &mut pIntraPredMode,
                dq,
            );
                let ret = if !*(*dq).grid.transform_size8x8_flag.get(iMbXy) {
                    ParseIntra4x4Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
                } else {
                    ParseIntra8x8Mode(pCtx, &mut *pDec, &mut sNeighAvail, &mut pIntraPredMode, buf, pBs, dq)
                };
                if ret != ERR_NONE {
                    return ret;
                }
            } else {
                *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
                *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
                (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
                *(*dq).grid.cbp.get_mut(iMbXy) = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
                uiCbpC = if pCtx.uiChromaFormatIdc() != 0 {
                    (*(*dq).grid.cbp.get(iMbXy) as u32) >> 4
                } else {
                    0
                };
                uiCbpL = (*(*dq).grid.cbp.get(iMbXy) as u32) & 15;
                crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                    &mut sNeighAvail,
                    &mut pNonZeroCount,
                    Some(&*dq),
                );
                let ret = ParseIntra16x16Mode(pCtx, &mut *pDec, &mut sNeighAvail, buf, pBs, dq);
                if ret != ERR_NONE {
                    return ret;
                }
            }
        }

        if MB_TYPE_INTRA16x16 != *pDec.pMbType.get(iMbXy) {
            let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            uiCbp = uiCode;
            if pCtx.uiChromaFormatIdc() != 0 && uiCbp > 47 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            if pCtx.uiChromaFormatIdc() == 0 && uiCbp > 15 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
            }
            let mb_type = *pDec.pMbType.get(iMbXy);
            uiCbp = if MB_TYPE_INTRA4x4 == mb_type || MB_TYPE_INTRA8x8 == mb_type {
                if pCtx.uiChromaFormatIdc() != 0 {
                    crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
                } else {
                    crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
                }
            } else {
                if pCtx.uiChromaFormatIdc() != 0 {
                    crate::decoder::dec_golomb::g_kuiInterCbpTable[uiCbp as usize] as u32
                } else {
                    crate::decoder::dec_golomb::g_kuiInterCbpTable400[uiCbp as usize] as u32
                }
            };

            *(*dq).grid.cbp.get_mut(iMbXy) = uiCbp as i8;
            uiCbpC = uiCbp >> 4;
            uiCbpL = uiCbp & 15;

            let mb_type = *pDec.pMbType.get(iMbXy);
            let bNeedParseTransformSize8x8Flag = ((mb_type >= MB_TYPE_16x16 && mb_type <= MB_TYPE_8x16)
                || *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get(iMbXy))
                && mb_type != MB_TYPE_INTRA8x8
                && mb_type != MB_TYPE_INTRA4x4
                && uiCbpL > 0
                && pCtx.bTransform8x8ModeFlag();

            if bNeedParseTransformSize8x8Flag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = uiCode != 0;
            }
        }

        let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
        pNzc.fill(0);

        let mb_type = *pDec.pMbType.get(iMbXy);
        if *(*dq).grid.cbp.get(iMbXy) == 0 && !IS_INTRA16x16(mb_type) && mb_type != MB_TYPE_INTRA_BL {
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            *(*dq).grid.luma_qp.get_mut(iMbXy) = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp as i8;
            for i in 0..2 {
                let idx = WELS_CLIP3(
                    *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh_chroma_qp_offset[i] as i32,
                    0,
                    51,
                );
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }
        }

        if *(*dq).grid.cbp.get(iMbXy) != 0 || MB_TYPE_INTRA16x16 == *pDec.pMbType.get(iMbXy) {
            let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
            scaled_tcoeff_mb.fill(0);

            let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
            if ret != 0 {
                return ret;
            }
            let iQpDelta = iCode;
            if iQpDelta > 25 || iQpDelta < -26 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_QP);
            }
            let new_qp = ((*dq).sLayerInfo.sSliceInLayer.iLastMbQp + iQpDelta + 52) % 52;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
            (*dq).sLayerInfo.sSliceInLayer.iLastMbQp = new_qp;
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx = WELS_CLIP3(new_qp + pps_sh_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            pBs.start_cavlc();

            let ret = WelsDecodeMbCavlcResidual(
                pCtx,
                buf,
                pBs,
                dq,
                &mut *pDec,
                pVlcTable,
                &mut pNonZeroCount,
                iScanIdxStart,
                iScanIdxEnd,
                uiCbpL,
                uiCbpC,
            );
            if ret != ERR_NONE {
                return ret;
            }

            pBs.end_cavlc(buf);
        }

        ERR_NONE
    }
}

/// Matches `WelsDecodeMbCavlcBSlice` in `decode_slice.cpp`.
pub fn WelsDecodeMbCavlcBSlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let (buf, pBs) = pNalCur.sNalData.sVclNal.sSliceBitsRead.split(pCtx.sRawData);
        // Resolved to the one flag either is read for. **F42 lets a list entry name
        // `pDec`**, so the shared borrow the resolution needs must not outlive the
        // expression that takes it — the writes below go through the same picture.
        let ppRefPicL0 = pRefs
            .resolve(pCtx.ref_id(LIST_0, 0), Some(&*pDec))
            .map(|p| p.bIsComplete);
        let ppRefPicL1 = pRefs
            .resolve(pCtx.ref_id(LIST_1, 0), Some(&*pDec))
            .map(|p| p.bIsComplete);
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut uiCode = 0u32;

        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;

        if (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun == -1 {
            // mb_skip_run
            if crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode) != 0 {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun = uiCode as i32;
            if (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun == -1 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_SKIP_RUN);
            }
            if (uiCode) > (((*dq).iMbWidth * (*dq).iMbHeight - iMbXy as i32) as u32) {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_SKIP_RUN);
            }
        }

        // C++ uses `if (pSlice->iMbSkipRun--)`: a coded macroblock leaves the
        // counter at -1 so the next macroblock parses a fresh mb_skip_run.
        let bSkip = (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun != 0;
        (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun -= 1;
        if bSkip {
            let mut iMv = [[0i16; 2]; LIST_A];
            let mut iRef = [0i8; LIST_A];

            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_SKIP | MB_TYPE_DIRECT;
            let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
            nzc_mb.fill(0);

            (pDec.pRefIndex[LIST_0].get_mut(iMbXy)).fill(0);
            (pDec.pRefIndex[LIST_1].get_mut(iMbXy)).fill(0);

            let bIsPending = pCtx.iThreadCount > 1;
            let is_complete0 = ppRefPicL0.is_some_and(|c| c || bIsPending);
            let is_complete1 = ppRefPicL1.is_some_and(|c| c || bIsPending);
            *pCtx.bMbRefConcealed =
                pCtx.bRPLRError || *pCtx.bMbRefConcealed || !is_complete0 || !is_complete1;

            // NOTE: unlike the CABAC B path, C keeps the `if (pCtx->bMbRefConcealed)
            // return ERR_INFO_REFERENCE_PIC_LOST` block commented out here.

            // predict iMv
            let mut subMbType: crate::decoder::mv_pred::SubMbType = 0;
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag != 0 {
                // predict direct spatial mv
                let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                    pCtx, &mut *dq,
                    &mut *pDec,
                    pRefs,
                    &mut iMv,
                    &mut iRef,
                    &mut subMbType,
                );
                if ret != ERR_NONE {
                    return ret;
                }
            } else {
                // temporal direct mode
                let ret = crate::decoder::mv_pred::PredBDirectTemporal(
                    pCtx, &mut *dq,
                    &mut *pDec,
                    pRefs,
                    &mut iMv,
                    &mut iRef,
                    &mut subMbType,
                );
                if ret != ERR_NONE {
                    return ret;
                }
            }

            // reset rS
            if !(*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultResidualPredFlag
                || (pNalCur.sNalHeaderExt.uiQualityId == 0
                    && pNalCur.sNalHeaderExt.uiDependencyId == 0)
            {
                let iLastMbQp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
                *(*dq).grid.luma_qp.get_mut(iMbXy) = iLastMbQp as i8;
                let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
                let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
                let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
                for i in 0..2 {
                    let idx = WELS_CLIP3(
                        *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh_chroma_qp_offset[i] as i32,
                        0,
                        51,
                    );
                    (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
                }
            }

            *(*dq).grid.cbp.get_mut(iMbXy) = 0;
        } else {
            let iBaseModeFlag;
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveBaseModeFlag {
                if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
                    return ERR_INFO_INVALID_ACCESS;
                }
                iBaseModeFlag = uiCode != 0;
            } else {
                iBaseModeFlag = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultBaseModeFlag;
            }
            if iBaseModeFlag {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_ILP);
            }
            let ret = WelsActualDecodeMbCavlcBSlice(pCtx, buf, pBs, dq, pDec, pRefs);
            if ret != ERR_NONE {
                return ret;
            }
        }

        // check whether there is left bits to read next time in case multiple slices
        let iUsedBits = (pBs.pos() as i32) * 8 - (16 - pBs.left_bits());
        // sub 1, for stop bit
        if iUsedBits == (pBs.bits() - 1)
            && 0 >= (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun
        {
            // slice boundary
            if true {
                *uiEosFlag = 1;
            }
        }
        if iUsedBits > (pBs.bits() - 1) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
        }
        ERR_NONE
    }
}

pub fn ParseIntra4x4Mode(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pNeighAvail: &mut SWelsNeighAvail,
    pIntraPredMode: &mut [i8; 48],
    buf: &[u8],
    // **T5b.6: a borrow, and F27's reason for the raw spelling is spent.** The
    // hazard was that the CABAC arm re-reached *this* cursor through the context;
    // since W6 step 3 the view carries `rbsp` — a shared slice over `sRawData`,
    // derived once at the bracket top — and the arithmetic engine beside it, so the
    // CABAC arm never touches the NAL node's `BsCursor` at all. The two are
    // different allocations, and the aliasing probes are the instrument that says so.
    pBsAux: &mut BsCursor,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    let dq: &mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut iSampleAvail = [0i32; 30];
    let uiNeighAvail;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    pCtx
        .eIntraPredConstraint
        .MapNxNNeighToSample(pNeighAvail, &mut iSampleAvail);

    uiNeighAvail = ((iSampleAvail[6] << 2) | (iSampleAvail[0] << 1) | (iSampleAvail[1])) as u8;

    let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);

    let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);

    let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
    // T5.I5: the sixteen 4x4 modes are written through one window. Nothing in
    // the loop reaches this family — `ParseIntraPredModeLumaCabac` and
    // `CheckIntraNxNPredMode` do not — and the record is `[i8; 16]`, so the
    // scan-order index inside it is bounded by a constant.
    let pIntra4x4FinalMode = (*dq).grid.intra4x4_final_mode.get_mut(iMbXy);
    for i in 0..16 {
        let iPrevIntra4x4PredMode;
        if pps_entropy {
            let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeLumaCabac(
                pCtx,
                &mut iCode,
            );
            if ret != ERR_NONE {
                return ret;
            }
            iPrevIntra4x4PredMode = iCode;
        } else {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, &mut *pBsAux, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            iPrevIntra4x4PredMode = uiCode as i32;
        }
        let kiPredMode = crate::decoder::parse_mb_syn_cavlc::PredIntra4x4Mode(pIntraPredMode, i);

        let mut iBestMode;
        if pps_entropy {
            if iPrevIntra4x4PredMode == -1 {
                iBestMode = kiPredMode as i8;
            } else {
                iBestMode = (iPrevIntra4x4PredMode
                    + if iPrevIntra4x4PredMode >= kiPredMode { 1 } else { 0 }) as i8;
            }
        } else {
            if iPrevIntra4x4PredMode != 0 {
                iBestMode = kiPredMode as i8;
            } else {
                let ret = crate::decoder::dec_golomb::BsGetBits(buf, &mut *pBsAux, 3, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                iBestMode = (uiCode as i32 + if (uiCode as i32) >= kiPredMode { 1 } else { 0 }) as i8;
            }
        }

        let iFinalMode = crate::decoder::parse_mb_syn_cavlc::CheckIntraNxNPredMode(
            &iSampleAvail,
            &mut iBestMode,
            i,
            false,
        );
        if iFinalMode == GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I4x4_PRED_MODE);
        }

        pIntra4x4FinalMode[g_kuiScan4[i as usize] as usize] = iFinalMode as i8;
        pIntraPredMode[g_kuiScan8[i as usize] as usize] = iBestMode;
        iSampleAvail[g_kCache30ScanIdx[i as usize] as usize] = 1;
    }

    let dst_modes = (*dq).grid.intra_pred_mode.get_mut(iMbXy);
    dst_modes[0] = pIntraPredMode[1 + 8 * 4];
    dst_modes[1] = pIntraPredMode[2 + 8 * 4];
    dst_modes[2] = pIntraPredMode[3 + 8 * 4];
    dst_modes[3] = pIntraPredMode[4 + 8 * 4];
    dst_modes[4] = pIntraPredMode[4 + 8 * 1];
    dst_modes[5] = pIntraPredMode[4 + 8 * 2];
    dst_modes[6] = pIntraPredMode[4 + 8 * 3];

    if pCtx.uiChromaFormatIdc() == 0 {
        return ERR_NONE;
    }

    if pps_entropy {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, dq,
            &*pDec,
            uiNeighAvail,
            &mut iCode,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if iCode > MAX_PRED_MODE_ID_CHROMA {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = iCode as i8;
    } else {
        let ret = crate::decoder::dec_golomb::BsGetUe(buf, &mut *pBsAux, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        if uiCode > MAX_PRED_MODE_ID_CHROMA as u32 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = uiCode as i8;
    }

    // T5.I4: the read and the `&mut i8` argument were two checks on one entry.
    // The window cannot open any earlier — `ParseIntraPredModeChromaCabac` reads
    // this family at the top and left addresses, and `Vec`'s `Index` retags the
    // whole buffer, so an earlier borrow would not survive that call.
    let pChromaPredMode = (*dq).grid.chroma_pred_mode.get_mut(iMbXy);
    if *pChromaPredMode == -1
        || crate::decoder::parse_mb_syn_cavlc::CheckIntraChromaPredMode(
            uiNeighAvail,
            pChromaPredMode,
        ) != 0
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
    }
    ERR_NONE
}

pub fn ParseIntra8x8Mode(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pNeighAvail: &mut SWelsNeighAvail,
    pIntraPredMode: &mut [i8; 48],
    buf: &[u8],
    // **T5b.6: a borrow, and F27's reason for the raw spelling is spent.** The
    // hazard was that the CABAC arm re-reached *this* cursor through the context;
    // since W6 step 3 the view carries `rbsp` — a shared slice over `sRawData`,
    // derived once at the bracket top — and the arithmetic engine beside it, so the
    // CABAC arm never touches the NAL node's `BsCursor` at all. The two are
    // different allocations, and the aliasing probes are the instrument that says so.
    pBsAux: &mut BsCursor,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    let dq: &mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut iSampleAvail = [0i32; 30];
    let uiNeighAvail;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    pCtx
        .eIntraPredConstraint
        .MapNxNNeighToSample(pNeighAvail, &mut iSampleAvail);

    uiNeighAvail = ((iSampleAvail[5] << 3)
        | (iSampleAvail[6] << 2)
        | (iSampleAvail[0] << 1)
        | (iSampleAvail[1])) as u8;
    *(*dq).grid.intra_nxn_avail_flag.get_mut(iMbXy) = uiNeighAvail;

    let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);

    let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);

    let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
    // T5.I5: as in `ParseIntra4x4Mode` — sixteen writes, one check.
    let pIntra4x4FinalMode = (*dq).grid.intra4x4_final_mode.get_mut(iMbXy);
    for i in 0..4usize {
        let iPrevIntra4x4PredMode;
        if pps_entropy {
            let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeLumaCabac(
                pCtx,
                &mut iCode,
            );
            if ret != ERR_NONE {
                return ret;
            }
            iPrevIntra4x4PredMode = iCode;
        } else {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, &mut *pBsAux, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            iPrevIntra4x4PredMode = uiCode as i32;
        }
        let kiPredMode =
            crate::decoder::parse_mb_syn_cavlc::PredIntra4x4Mode(pIntraPredMode, (i << 2) as i32);

        let mut iBestMode;
        if pps_entropy {
            if iPrevIntra4x4PredMode == -1 {
                iBestMode = kiPredMode as i8;
            } else {
                iBestMode = (iPrevIntra4x4PredMode
                    + if iPrevIntra4x4PredMode >= kiPredMode { 1 } else { 0 }) as i8;
            }
        } else {
            if iPrevIntra4x4PredMode != 0 {
                iBestMode = kiPredMode as i8;
            } else {
                let ret = crate::decoder::dec_golomb::BsGetBits(buf, &mut *pBsAux, 3, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                iBestMode = (uiCode as i32 + if (uiCode as i32) >= kiPredMode { 1 } else { 0 }) as i8;
            }
        }

        let iFinalMode = crate::decoder::parse_mb_syn_cavlc::CheckIntraNxNPredMode(
            &iSampleAvail,
            &mut iBestMode,
            (i << 2) as i32,
            true,
        );
        if iFinalMode == GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I4x4_PRED_MODE);
        }

        for j in 0..4usize {
            pIntra4x4FinalMode[g_kuiScan4[(i << 2) + j] as usize] =
                iFinalMode as i8;
            pIntraPredMode[g_kuiScan8[(i << 2) + j] as usize] = iBestMode;
            iSampleAvail[g_kCache30ScanIdx[(i << 2) + j] as usize] = 1;
        }
    }

    // `ST32 (&pIntraPredMode[iMbXy][0], LD32 (&pIntraPredMode[1 + 8 * 4]))` copies
    // four modes, not one; entries 1..3 feed the left-neighbour cache of the next
    // macroblock (WelsFillCacheConstrain0IntraNxN reads [3]).
    let dst_modes = (*dq).grid.intra_pred_mode.get_mut(iMbXy);
    dst_modes[0] = pIntraPredMode[1 + 8 * 4];
    dst_modes[1] = pIntraPredMode[2 + 8 * 4];
    dst_modes[2] = pIntraPredMode[3 + 8 * 4];
    dst_modes[3] = pIntraPredMode[4 + 8 * 4];
    dst_modes[4] = pIntraPredMode[4 + 8 * 1];
    dst_modes[5] = pIntraPredMode[4 + 8 * 2];
    dst_modes[6] = pIntraPredMode[4 + 8 * 3];

    if pCtx.uiChromaFormatIdc() == 0 {
        return ERR_NONE;
    }

    if pps_entropy {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, dq,
            &*pDec,
            uiNeighAvail,
            &mut iCode,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if iCode > MAX_PRED_MODE_ID_CHROMA {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = iCode as i8;
    } else {
        let ret = crate::decoder::dec_golomb::BsGetUe(buf, &mut *pBsAux, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        if uiCode > MAX_PRED_MODE_ID_CHROMA as u32 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = uiCode as i8;
    }

    // T5.I4: the read and the `&mut i8` argument were two checks on one entry.
    // The window cannot open any earlier — `ParseIntraPredModeChromaCabac` reads
    // this family at the top and left addresses, and `Vec`'s `Index` retags the
    // whole buffer, so an earlier borrow would not survive that call.
    let pChromaPredMode = (*dq).grid.chroma_pred_mode.get_mut(iMbXy);
    if *pChromaPredMode == -1
        || crate::decoder::parse_mb_syn_cavlc::CheckIntraChromaPredMode(
            uiNeighAvail,
            pChromaPredMode,
        ) != 0
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
    }
    ERR_NONE
}

pub fn ParseIntra16x16Mode(
    pCtx: &mut SliceCtx<'_>,
    pDec: &mut SPicture,
    pNeighAvail: &mut SWelsNeighAvail,
    buf: &[u8],
    // **T5b.6: a borrow, and F27's reason for the raw spelling is spent.** The
    // hazard was that the CABAC arm re-reached *this* cursor through the context;
    // since W6 step 3 the view carries `rbsp` — a shared slice over `sRawData`,
    // derived once at the bracket top — and the arithmetic engine beside it, so the
    // CABAC arm never touches the NAL node's `BsCursor` at all. The two are
    // different allocations, and the aliasing probes are the instrument that says so.
    pBsAux: &mut BsCursor,
    pCurDqLayer: &mut DqLayerState,
) -> i32 {
    let dq: &mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiNeighAvail = 0u8;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    pCtx
        .eIntraPredConstraint
        .Map16x16NeighToSample(pNeighAvail, &mut uiNeighAvail);

    let pMode = &mut (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7];
    if crate::decoder::parse_mb_syn_cavlc::CheckIntra16x16PredMode(uiNeighAvail, pMode) != 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
    }
    if pCtx.uiChromaFormatIdc() == 0 {
        return ERR_NONE;
    }

    let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);

    let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);

    let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
    if pps_entropy {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, dq,
            &*pDec,
            uiNeighAvail,
            &mut iCode,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if iCode > MAX_PRED_MODE_ID_CHROMA {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = iCode as i8;
    } else {
        let ret = crate::decoder::dec_golomb::BsGetUe(buf, &mut *pBsAux, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        if uiCode > MAX_PRED_MODE_ID_CHROMA as u32 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
        }
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = uiCode as i8;
    }

    // T5.I4: the read and the `&mut i8` argument were two checks on one entry.
    // The window cannot open any earlier — `ParseIntraPredModeChromaCabac` reads
    // this family at the top and left addresses, and `Vec`'s `Index` retags the
    // whole buffer, so an earlier borrow would not survive that call.
    let pChromaPredMode = (*dq).grid.chroma_pred_mode.get_mut(iMbXy);
    if *pChromaPredMode == -1
        || crate::decoder::parse_mb_syn_cavlc::CheckIntraChromaPredMode(
            uiNeighAvail,
            pChromaPredMode,
        ) != 0
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
    }
    ERR_NONE
}

fn WelsDecodeMbCabacIntraModeHelper(
    pCtx: &mut SliceCtx<'_>,
    pNalCur: &mut SNalUnit,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pNeighAvail: &mut SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    pIntraPredMode: &mut [i8; 48],
    uiMbType: u32,
) -> i32 {
    {
        // **Not `split()` here** (F27). `split` hands back `&mut self.cursor`, which this
        // function then passes down as a strongly protected argument — while the CABAC
        // engine underneath reaches the *same* `BsReader` whole through
        // `cabac_rbsp_window`. Two live paths to one object, one of them exclusive —
        // and since T5.M3 both start at the same `slice_bit_reader` derivation rather
        // than at a mirror of it. `addr_of_mut!` creates no reference, so there is no
        // retag to conflict and the CAVLC leaves re-derive per use; S29's spelling.
        let pBsRd: &mut BsReader = &mut pNalCur.sNalData.sVclNal.sSliceBitsRead;
        let buf = pCtx.sRawData.window_from((*pBsRd).start);
        let pBsAux: &mut BsCursor = &mut pBsRd.cursor;
        let iMbXy = (*dq).iMbXyIndex as usize;

        if uiMbType == 0 {
            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
            let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            if pps_transform8x8 {
                // T5.I2 (F34): the callee reads *this array* at the left and top
                // addresses, and `Vec`'s `Index` builds a shared slice over the whole
                // buffer — which removes the strongly-protected `&mut` it was handed.
                // Proved under Miri on a standalone reproduction; unreachable by the
                // aliasing probe, whose stream is one macroblock per frame, so both
                // availability flags are 0 and neither read runs. Keeping the value in
                // a local and storing it after the call has no borrow live across it.
                let mut bTransformSize8x8Flag = false;
                let ret = crate::decoder::parse_mb_syn_cabac::ParseTransformSize8x8FlagCabac(
                    pCtx, &mut *dq,
                    pNeighAvail,
                    &mut bTransformSize8x8Flag,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = bTransformSize8x8Flag;
            }
            pCtx.eIntraPredConstraint.FillCacheIntraNxN(
                pNeighAvail,
                pNonZeroCount,
                pIntraPredMode,
                dq,
            );

            if *(*dq).grid.transform_size8x8_flag.get(iMbXy) {
                *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                ParseIntra8x8Mode(pCtx, &mut *pDec, pNeighAvail, pIntraPredMode, buf, pBsAux, dq)
            } else {
                ParseIntra4x4Mode(pCtx, &mut *pDec, pNeighAvail, pIntraPredMode, buf, pBsAux, dq)
            }
        } else {
            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
            *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
            *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
            (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType as i32 - 1) & 3) as i8;
            *(*dq).grid.cbp.get_mut(iMbXy) = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                pNeighAvail,
                pNonZeroCount,
                Some(&*dq),
            );
            ParseIntra16x16Mode(pCtx, &mut *pDec, pNeighAvail, buf, pBsAux, dq)
        }
    }
}

fn WelsDecodeMbCabacResidualHelper(
    pCtx: &mut SliceCtx<'_>,
    pNalCur: &mut SNalUnit,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pNeighAvail: &mut SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    iScanIdxStart: usize,
    iScanIdxEnd: usize,
) -> i32 {
    {
        // **Not `split()` here** (F27). `split` hands back `&mut self.cursor`, which this
        // function then passes down as a strongly protected argument — while the CABAC
        // engine underneath reaches the *same* `BsReader` whole through
        // `cabac_rbsp_window`. Two live paths to one object, one of them exclusive —
        // and since T5.M3 both start at the same `slice_bit_reader` derivation rather
        // than at a mirror of it. `addr_of_mut!` creates no reference, so there is no
        // retag to conflict and the CAVLC leaves re-derive per use; S29's spelling.
        let pBsRd: &mut BsReader = &mut pNalCur.sNalData.sVclNal.sSliceBitsRead;
        let buf = pCtx.sRawData.window_from((*pBsRd).start);
        let pBsAux: &mut BsCursor = &mut pBsRd.cursor;
        let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
        let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
        let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
        let pps_layer_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
        let pps_layer_entropy = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
        let pps_layer_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
        let iMbXy = (*dq).iMbXyIndex as usize;
        // T5.X1: the two scalars the residual chain reaches, read once here. The chain
        // takes them by value, so the `&mut` the caller holds into `grid.scaled_tcoeff`
        // never has to coexist with a `&mut` to the whole layer.
        let iMbXyIndex = (*dq).iMbXyIndex;
        let iMbWidth = (*dq).iMbWidth;
        let mb_type = *pDec.pMbType.get(iMbXy);
        let mut uiCbp = 0u32;
        let uiCbpLuma;
        let uiCbpChroma;

        // T5.R7: the cache is a `[u8; 48]` and the grid's row is an `[i8; 24]`, so the
        // C's `ST32`/`ST16` writes are four- and two-element copies between two arrays
        // whose elements differ only in signedness. The `as i8` per element is the same
        // reinterpretation the pointer cast was, spelled where it happens.
        let copy4 = |dst: &mut [i8], at: usize, src: &[u8; 48], from: usize| {
            for k in 0..4 {
                dst[at + k] = src[from + k] as i8;
            }
        };
        let copy2 = |dst: &mut [i8], at: usize, src: &[u8; 48], from: usize| {
            for k in 0..2 {
                dst[at + k] = src[from + k] as i8;
            }
        };

        if mb_type != MB_TYPE_INTRA16x16 {
            let ret = crate::decoder::parse_mb_syn_cabac::ParseCbpInfoCabac(
                pCtx,
                pNeighAvail,
                &mut uiCbp,
            );
            if ret != ERR_NONE {
                return ret;
            }
            *(*dq).grid.cbp.get_mut(iMbXy) = uiCbp as i8;
            if uiCbp == 0 {
                (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
            }
            uiCbpChroma = if pCtx.uiChromaFormatIdc() != 0 {
                uiCbp >> 4
            } else {
                0
            };
            uiCbpLuma = uiCbp & 15;
        } else {
            uiCbp = *(*dq).grid.cbp.get(iMbXy) as u32;
            uiCbpChroma = if pCtx.uiChromaFormatIdc() != 0 {
                uiCbp >> 4
            } else {
                0
            };
            uiCbpLuma = uiCbp & 15;
        }

        if uiCbp != 0 || mb_type == MB_TYPE_INTRA16x16 {
            if mb_type != MB_TYPE_INTRA16x16 {
                let bNeedParseTransformSize8x8Flag = (IS_INTER_16x16(mb_type)
                    || IS_DIRECT(mb_type)
                    || IS_INTER_16x8(mb_type)
                    || IS_INTER_8x16(mb_type)
                    || *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get(iMbXy))
                    && mb_type != MB_TYPE_INTRA8x8
                    && mb_type != MB_TYPE_INTRA4x4
                    && (uiCbp & 0x0F) > 0
                    && pps_layer_transform8x8;

                if bNeedParseTransformSize8x8Flag {
                    // T5.I2 (F34) — as above; the callee reads the same array at the
                    // neighbour addresses while holding this borrow.
                    let mut bTransformSize8x8Flag = false;
                    let ret = crate::decoder::parse_mb_syn_cabac::ParseTransformSize8x8FlagCabac(
                        pCtx, &mut *dq,
                        pNeighAvail,
                        &mut bTransformSize8x8Flag,
                    );
                    if ret != ERR_NONE {
                        return ret;
                    }
                    *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = bTransformSize8x8Flag;
                }
            }

            // T5.X3: the zeroing stays where the C++ has it (`memset (pScaledTCoeff, 0,
            // …)` before the delta-QP parse); only the *window* moves below the parse,
            // because `ParseDeltaQpCabac` takes the layer and this is a borrow of it.
            (*dq).grid.scaled_tcoeff.get_mut(iMbXy).fill(0);

            let mut iQpDelta = 0i32;
            let ret = crate::decoder::parse_mb_syn_cabac::ParseDeltaQpCabac(
                pCtx, &mut *dq,
                &mut iQpDelta,
            );
            if ret != ERR_NONE {
                return ret;
            }
            if iQpDelta > 25 || iQpDelta < -26 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_QP);
            }
            let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
            let new_qp = ((*dq).sLayerInfo.sSliceInLayer.iLastMbQp + iQpDelta + 52) % 52;
            // T5.I2: the write opens the window and the four residual reads below —
            // each of them inside `for iId8x8 { for iId4x4 { } }` — read through it.
            // `ParseResidualBlockCabac` reaches the layer but not this family. The
            // `else` arm's write at the tail of the function is this branch's
            // alternative, never the same execution.
            let iLumaQp = (*dq).grid.luma_qp.get_mut(iMbXy);
            *iLumaQp = new_qp as i8;
            (*dq).sLayerInfo.sSliceInLayer.iLastMbQp = new_qp;
            for i in 0..2 {
                let idx =
                    WELS_CLIP3(new_qp + pps_sh_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            if mb_type == MB_TYPE_INTRA16x16 {
                let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                    pNeighAvail,
                    pNonZeroCount,
                    0,
                    16,
                    &g_kuiLumaDcZigzagScan,
                    I16_LUMA_DC,
                    &mut scaled_tcoeff_mb[..],
                    *iLumaQp as u8,
                    pCtx,
                    iMbXyIndex,
                    iMbWidth,
                    &mut (*dq).grid.cbf_dc,
                    &pDec.pMbType,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                if uiCbpLuma != 0 {
                    for i in 0..16 {
                        let max_idx = std::cmp::max(iScanIdxStart, 1);
                        let len = (iScanIdxEnd as isize - max_idx as isize + 1) as i32;
                        let scan_ptr = &g_kuiZigzagScan[max_idx..];
                        let coeff_ptr = &mut scaled_tcoeff_mb[i * 16..];
                        let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                            pNeighAvail,
                            pNonZeroCount,
                                    i as i32,
                            len,
                            scan_ptr,
                            I16_LUMA_AC,
                            coeff_ptr,
                            *iLumaQp as u8,
                            pCtx,
                            iMbXyIndex,
                            iMbWidth,
                            &mut (*dq).grid.cbf_dc,
                            &pDec.pMbType,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                    }
                    // `ST32 (&pNzc[iMbXy][n], LD32 (&pNonZeroCount[1 + 8 * k]))`: each store
                    // copies a whole row of four 4x4 counts, not a single one.
                    let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                    copy4(nzc_mb, 0, pNonZeroCount, 1 + 8);
                    copy4(nzc_mb, 4, pNonZeroCount, 1 + 8 * 2);
                    copy4(nzc_mb, 8, pNonZeroCount, 1 + 8 * 3);
                    copy4(nzc_mb, 12, pNonZeroCount, 1 + 8 * 4);
                } else {
                    // `ST32 (&pNzc[iMbXy][n], 0)` clears four counts per store.
                    let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                    nzc_mb[0..16].fill(0);
                }
            } else {
                let is_intra = IS_INTRA(mb_type);
                if *(*dq).grid.transform_size8x8_flag.get(iMbXy) {
                    for iId8x8 in 0..4 {
                        if (uiCbpLuma & (1 << iId8x8)) != 0 {
                            let iIdx = iId8x8 * 4;
                            let len = (iScanIdxEnd as isize - iScanIdxStart as isize + 1) as i32;
                            let scan_ptr = &g_kuiZigzagScan8x8[iScanIdxStart..];
                            let res_prop = if is_intra {
                                LUMA_DC_AC_INTRA_8
                            } else {
                                LUMA_DC_AC_INTER_8
                            };
                            let coeff_ptr = &mut scaled_tcoeff_mb[iId8x8 * 64..];
                            let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac8x8(
                                pNeighAvail,
                                pNonZeroCount,
                                            iIdx as i32,
                                len,
                                scan_ptr,
                                res_prop,
                                coeff_ptr,
                                *iLumaQp as u8,
                                pCtx,
                            );
                            if ret != ERR_NONE {
                                return ret;
                            }
                        } else {
                            pNonZeroCount[g_kCacheNzcScanIdx[(iId8x8 * 4) as usize] as usize] = 0;
                            pNonZeroCount[g_kCacheNzcScanIdx[(iId8x8 * 4 + 2) as usize] as usize] = 0;
                        }
                    }
                    // `ST32 (&pNzc[iMbXy][n], LD32 (&pNonZeroCount[1 + 8 * k]))`: each store
                    // copies a whole row of four 4x4 counts, not a single one.
                    let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                    copy4(nzc_mb, 0, pNonZeroCount, 1 + 8);
                    copy4(nzc_mb, 4, pNonZeroCount, 1 + 8 * 2);
                    copy4(nzc_mb, 8, pNonZeroCount, 1 + 8 * 3);
                    copy4(nzc_mb, 12, pNonZeroCount, 1 + 8 * 4);
                } else {
                    let res_prop = if is_intra {
                        LUMA_DC_AC_INTRA
                    } else {
                        LUMA_DC_AC_INTER
                    };
                    for iId8x8 in 0..4 {
                        if (uiCbpLuma & (1 << iId8x8)) != 0 {
                            let mut iIdx = iId8x8 * 4;
                            for _ in 0..4 {
                                let len = (iScanIdxEnd as isize - iScanIdxStart as isize + 1) as i32;
                                let scan_ptr = &g_kuiZigzagScan[iScanIdxStart..];
                                let coeff_ptr = &mut scaled_tcoeff_mb[iIdx * 16..];
                                let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                                    pNeighAvail,
                                    pNonZeroCount,
                                                    iIdx as i32,
                                    len,
                                    scan_ptr,
                                    res_prop,
                                    coeff_ptr,
                                    *iLumaQp as u8,
                                    pCtx,
                                    iMbXyIndex,
                                    iMbWidth,
                                    &mut (*dq).grid.cbf_dc,
                                    &pDec.pMbType,
                                );
                                if ret != ERR_NONE {
                                    return ret;
                                }
                                iIdx += 1;
                            }
                        } else {
                            pNonZeroCount[g_kCacheNzcScanIdx[(iId8x8 * 4) as usize] as usize] = 0;
                            pNonZeroCount[g_kCacheNzcScanIdx[(iId8x8 * 4 + 2) as usize] as usize] = 0;
                        }
                    }
                    // `ST32 (&pNzc[iMbXy][n], LD32 (&pNonZeroCount[1 + 8 * k]))`: each store
                    // copies a whole row of four 4x4 counts, not a single one.
                    let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                    copy4(nzc_mb, 0, pNonZeroCount, 1 + 8);
                    copy4(nzc_mb, 4, pNonZeroCount, 1 + 8 * 2);
                    copy4(nzc_mb, 8, pNonZeroCount, 1 + 8 * 3);
                    copy4(nzc_mb, 12, pNonZeroCount, 1 + 8 * 4);
                }
            }

            if uiCbpChroma == 1 || uiCbpChroma == 2 {
                for i in 0..2 {
                    let res_prop = if IS_INTRA(mb_type) {
                        if i != 0 {
                            CHROMA_DC_V
                        } else {
                            CHROMA_DC_U
                        }
                    } else {
                        if i != 0 {
                            CHROMA_DC_V_INTER
                        } else {
                            CHROMA_DC_U_INTER
                        }
                    };
                    let coeff_ptr = &mut scaled_tcoeff_mb[256 + i * 64..];
                    let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                        pNeighAvail,
                        pNonZeroCount,
                            16 + (i as i32 * 4),
                        4,
                        &g_kuiChromaDcScan,
                        res_prop,
                        coeff_ptr,
                        (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                        pCtx,
                        iMbXyIndex,
                        iMbWidth,
                        &mut (*dq).grid.cbf_dc,
                        &pDec.pMbType,
                    );
                    if ret != ERR_NONE {
                        return ret;
                    }
                }
            }

            if uiCbpChroma == 2 {
                for i in 0..2 {
                    let res_prop = if IS_INTRA(mb_type) {
                        if i != 0 {
                            CHROMA_AC_V
                        } else {
                            CHROMA_AC_U
                        }
                    } else {
                        if i != 0 {
                            CHROMA_AC_V_INTER
                        } else {
                            CHROMA_AC_U_INTER
                        }
                    };
                    let mut index = 16 + (i * 4);
                    for _ in 0..4 {
                        let max_idx = std::cmp::max(iScanIdxStart, 1);
                        let len = (iScanIdxEnd as isize - max_idx as isize + 1) as i32;
                        let scan_ptr = &g_kuiZigzagScan[max_idx..];
                        let coeff_ptr = &mut scaled_tcoeff_mb[index * 16..];
                        let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                            pNeighAvail,
                            pNonZeroCount,
                                    index as i32,
                            len,
                            scan_ptr,
                            res_prop,
                            coeff_ptr,
                            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                            pCtx,
                            iMbXyIndex,
                            iMbWidth,
                            &mut (*dq).grid.cbf_dc,
                            &pDec.pMbType,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                        index += 1;
                    }
                }
                // `ST16 (&pNzc[iMbXy][n], LD16 (&pNonZeroCount[6 + 8 * k]))`: two counts each.
                let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                copy2(nzc_mb, 16, pNonZeroCount, 6 + 8);
                copy2(nzc_mb, 20, pNonZeroCount, 6 + 8 * 2);
                copy2(nzc_mb, 18, pNonZeroCount, 6 + 8 * 4);
                copy2(nzc_mb, 22, pNonZeroCount, 6 + 8 * 5);
            } else {
                // `ST16 (&pNzc[iMbXy][n], 0)` clears two counts per store.
                let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
                nzc_mb[16..24].fill(0);
            }
        } else {
            let last_qp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = last_qp as i8;
            let pps_sh_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_sh_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_sh_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx =
                    WELS_CLIP3(last_qp + pps_sh_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }
        }

        ERR_NONE
    }
}
pub fn WelsDecodeMbCabacISliceBaseMode0(
    pCtx: &mut SliceCtx<'_>,
    pNalCur: &mut SNalUnit,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut pNonZeroCount = [0u8; 48];
        let mut pIntraPredMode = [0i8; 48];
        let mut uiMbType = 0u32;

        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
        *(*dq).grid.residual_pred_flag.get_mut(iMbXy) =
            (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultResidualPredFlag as i8;

        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(
            &mut sNeighAvail,
            Some(&*dq),
            Some(&*pDec),
        );
        let mut ret = crate::decoder::parse_mb_syn_cabac::ParseMBTypeISliceCabac(
            pCtx,
            &mut sNeighAvail,
            &mut uiMbType,
        );
        if ret != ERR_NONE {
            return ret;
        }

        if uiMbType > 25 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        } else if pCtx.uiChromaFormatIdc() == 0
            && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        } else if uiMbType == 25 {
            ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, &mut pNalCur.sNalData.sVclNal.sSliceBitsRead, &mut *dq, &mut *pDec);
            if ret != ERR_NONE {
                return ret;
            }
            (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
            ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                pCtx,
                uiEosFlag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            if *uiEosFlag != 0 {
                crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                    &mut *pCtx.sCabacDecEngine,
                    &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
                );
            }
            return ERR_NONE;
        }

        ret = WelsDecodeMbCabacIntraModeHelper(
            pCtx,
            pNalCur,
            dq,
            &mut *pDec,
            &mut sNeighAvail,
            &mut pNonZeroCount,
            &mut pIntraPredMode,
            uiMbType,
        );
        if ret != ERR_NONE {
            return ret;
        }

        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);
        *(*dq).grid.cbf_dc.get_mut(iMbXy) = 0;

        ret = WelsDecodeMbCabacResidualHelper(
            pCtx,
            pNalCur,
            dq,
            &mut *pDec,
            &mut sNeighAvail,
            &mut pNonZeroCount,
            iScanIdxStart,
            iScanIdxEnd,
        );
        if ret != ERR_NONE {
            return ret;
        }

        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if *uiEosFlag != 0 {
            crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                &mut *pCtx.sCabacDecEngine,
                &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
            );
        }
        ERR_NONE
    }
}
pub fn WelsDecodeMbCabacISlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let ret = { WelsDecodeMbCabacISliceBaseMode0(pCtx,
            pNalCur, dq, pDec, pRefs, uiEosFlag) };
        if ret != ERR_NONE {
            return ret;
        }
        ERR_NONE
    }
}
pub fn WelsDecodeMbCabacPSliceBaseMode0(
    pCtx: &mut SliceCtx<'_>,
    pNalCur: &mut SNalUnit,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNeighAvail: &mut SWelsNeighAvail,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut pNonZeroCount = [0u8; 48];
        let mut pIntraPredMode = [0i8; 48];
        let mut uiMbType = 0u32;


        let mut ret = crate::decoder::parse_mb_syn_cabac::ParseMBTypePSliceCabac(
            pCtx,
            pNeighAvail,
            &mut uiMbType,
        );
        if ret != ERR_NONE {
            return ret;
        }

        if uiMbType < 4 {
            let mut pMotionVector = [[[0i16; 2]; 30]; LIST_A];
            let mut pMvdCache = [[[0i16; 2]; 30]; LIST_A];
            let mut pRefIndex = [[0i8; 30]; LIST_A];
            *pDec.pMbType.get_mut(iMbXy) = g_ksInterPMbTypeInfo[uiMbType as usize].iType;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInterCabac(
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pMotionVector,
                &mut pMvdCache,
                &mut pRefIndex,
                &*dq,
                &*pDec,
            );
            ret = crate::decoder::parse_mb_syn_cabac::ParseInterPMotionInfoCabac(
                pCtx, &mut *dq,
                &mut *pDec,
                pRefs,
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pMotionVector,
                &mut pMvdCache,
                &mut pRefIndex,
            );
            if ret != ERR_NONE {
                return ret;
            }
        } else {
            let intra_type = uiMbType - 5;
            if intra_type > 25 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if pCtx.uiChromaFormatIdc() == 0
                && ((intra_type >= 5 && intra_type <= 12) || (intra_type >= 17 && intra_type <= 24))
            {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if intra_type == 25 {
                ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, &mut pNalCur.sNalData.sVclNal.sSliceBitsRead, &mut *dq, &mut *pDec);
                if ret != ERR_NONE {
                    return ret;
                }
                (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
                ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                    pCtx,
                    uiEosFlag,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                if *uiEosFlag != 0 {
                    crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                        &mut *pCtx.sCabacDecEngine,
                        &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
                    );
                }
                return ERR_NONE;
            }

            ret = WelsDecodeMbCabacIntraModeHelper(
                pCtx,
            pNalCur,
                dq,
                &mut *pDec,
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pIntraPredMode,
                intra_type,
            );
            if ret != ERR_NONE {
                return ret;
            }
        }

        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);
        *(*dq).grid.cbf_dc.get_mut(iMbXy) = 0;

        ret = WelsDecodeMbCabacResidualHelper(
            pCtx,
            pNalCur,
            dq,
            &mut *pDec,
            pNeighAvail,
            &mut pNonZeroCount,
            iScanIdxStart,
            iScanIdxEnd,
        );
        if ret != ERR_NONE {
            return ret;
        }

        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if *uiEosFlag != 0 {
            crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                &mut *pCtx.sCabacDecEngine,
                &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
            );
        }
        ERR_NONE
    }
}
pub fn WelsDecodeMbCabacPSlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut uiCode = 0u32;

        *(*dq).grid.cbp.get_mut(iMbXy) = 0;
        *(*dq).grid.cbf_dc.get_mut(iMbXy) = 0;
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = C_PRED_DC as i8;
        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;

        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(
            &mut sNeighAvail,
            Some(&*dq),
            Some(&*pDec),
        );
        let mut ret = crate::decoder::parse_mb_syn_cabac::ParseSkipFlagCabac(
            pCtx,
            &mut sNeighAvail,
            &mut uiCode,
        );
        if ret != ERR_NONE {
            return ret;
        }

        if uiCode != 0 {
            let mut pMv = [0i16; 2];
            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_SKIP;
            let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
            nzc_mb.fill(0);
            let ref_slice = pDec.pRefIndex[LIST_0].get_mut(iMbXy);
            ref_slice.fill(0);

            let bIsPending = pCtx.iThreadCount > 1;
            // F42's clause, as above: resolve and take the flag in one expression.
            let is_complete0 = pRefs
                .resolve(pCtx.ref_id(LIST_0, 0), Some(&*pDec))
                .is_some_and(|p| p.bIsComplete || bIsPending);
            *pCtx.bMbRefConcealed =
                pCtx.bRPLRError || *pCtx.bMbRefConcealed || !is_complete0;

            crate::decoder::mv_pred::PredPSkipMvFromNeighbor(&mut *dq, Some(&*pDec), &mut pMv);
            let mv_slice = pDec.pMv[LIST_0].get_mut(iMbXy);
            let mvd_slice = (*dq).grid.mvd[LIST_0].get_mut(iMbXy);
            for i in 0..16 {
                mv_slice[i] = pMv;
                mvd_slice[i] = [0, 0];
            }

            let last_qp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = last_qp as i8;
            let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx =
                    WELS_CLIP3(last_qp + pps_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;

            ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                pCtx,
                uiEosFlag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            return ERR_NONE;
        }

        WelsDecodeMbCabacPSliceBaseMode0(pCtx,
            pNalCur, dq, pDec, pRefs, &mut sNeighAvail, uiEosFlag)
    }
}
pub fn WelsDecodeMbCabacBSliceBaseMode0(
    pCtx: &mut SliceCtx<'_>,
    pNalCur: &mut SNalUnit,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNeighAvail: &mut SWelsNeighAvail,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let iScanIdxStart = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxStart as usize;
        let iScanIdxEnd = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.uiScanIdxEnd as usize;
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut pNonZeroCount = [0u8; 48];
        let mut pIntraPredMode = [0i8; 48];
        let mut uiMbType = 0u32;


        let mut ret = crate::decoder::parse_mb_syn_cabac::ParseMBTypeBSliceCabac(
            pCtx,
            pNeighAvail,
            &mut uiMbType,
        );
        if ret != ERR_NONE {
            return ret;
        }

        if uiMbType < 23 {
            let mut pMotionVector = [[[0i16; 2]; 30]; LIST_A];
            let mut pMvdCache = [[[0i16; 2]; 30]; LIST_A];
            let mut pRefIndex = [[0i8; 30]; LIST_A];
            let mut pDirect = [0i8; 30];
            *pDec.pMbType.get_mut(iMbXy) = g_ksInterBMbTypeInfo[uiMbType as usize].iType;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInterCabac(
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pMotionVector,
                &mut pMvdCache,
                &mut pRefIndex,
                &*dq,
                &*pDec,
            );
            crate::decoder::parse_mb_syn_cavlc::WelsFillDirectCacheCabac(
                pNeighAvail,
                &mut pDirect,
                &*dq,
            );
            ret = crate::decoder::parse_mb_syn_cabac::ParseInterBMotionInfoCabac(
                pCtx, &mut *dq,
                &mut *pDec,
                pRefs,
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pMotionVector,
                &mut pMvdCache,
                &mut pRefIndex,
                &mut pDirect,
            );
            if ret != ERR_NONE {
                return ret;
            }
        } else {
            let intra_type = uiMbType - 23;
            if intra_type > 25 {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if pCtx.uiChromaFormatIdc() == 0
                && ((intra_type >= 5 && intra_type <= 12) || (intra_type >= 17 && intra_type <= 24))
            {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
            }
            if intra_type == 25 {
                ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, &mut pNalCur.sNalData.sVclNal.sSliceBitsRead, &mut *dq, &mut *pDec);
                if ret != ERR_NONE {
                    return ret;
                }
                (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
                ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                    pCtx,
                    uiEosFlag,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                if *uiEosFlag != 0 {
                    crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                        &mut *pCtx.sCabacDecEngine,
                        &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
                    );
                }
                return ERR_NONE;
            }

            ret = WelsDecodeMbCabacIntraModeHelper(
                pCtx,
            pNalCur,
                dq,
                &mut *pDec,
                pNeighAvail,
                &mut pNonZeroCount,
                &mut pIntraPredMode,
                intra_type,
            );
            if ret != ERR_NONE {
                return ret;
            }
        }

        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);
        *(*dq).grid.cbf_dc.get_mut(iMbXy) = 0;

        ret = WelsDecodeMbCabacResidualHelper(
            pCtx,
            pNalCur,
            dq,
            &mut *pDec,
            pNeighAvail,
            &mut pNonZeroCount,
            iScanIdxStart,
            iScanIdxEnd,
        );
        if ret != ERR_NONE {
            return ret;
        }

        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if *uiEosFlag != 0 {
            crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                &mut *pCtx.sCabacDecEngine,
                &mut pNalCur.sNalData.sVclNal.sSliceBitsRead,
            );
        }
        ERR_NONE
    }
}
pub fn WelsDecodeMbCabacBSlice(
    pCtx: &mut SliceCtx<'_>,
    dq: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    pNalCur: &mut SNalUnit,
    uiEosFlag: &mut u32,
) -> i32 {
    {
        let iMbXy = (*dq).iMbXyIndex as usize;
        let mut sNeighAvail = SWelsNeighAvail::default();
        let mut uiCode = 0u32;

        *(*dq).grid.cbp.get_mut(iMbXy) = 0;
        *(*dq).grid.cbf_dc.get_mut(iMbXy) = 0;
        *(*dq).grid.chroma_pred_mode.get_mut(iMbXy) = C_PRED_DC as i8;
        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;

        crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(
            &mut sNeighAvail,
            Some(&*dq),
            Some(&*pDec),
        );
        let mut ret = crate::decoder::parse_mb_syn_cabac::ParseSkipFlagCabac(
            pCtx,
            &mut sNeighAvail,
            &mut uiCode,
        );
        if ret != ERR_NONE {
            return ret;
        }

        // `memset (pCurDqLayer->pDirect[iMbXy], 0, sizeof (int8_t) * 16)`: pDirect is
        // `*mut [i8; 16]`, so the row index is iMbXy — scaling it by 16 walks 16 rows
        // per macroblock and writes past the allocation into the neighbouring buffers.
        (*dq).grid.direct.get_mut(iMbXy).fill(0);

        let bIsPending = pCtx.iThreadCount > 1;

        if uiCode != 0 {
            let mut pMv = [[0i16; 2]; 2];
            let mut ref_idx = [0i8; 2];
            let mut subMbType = 0u32;

            *pDec.pMbType.get_mut(iMbXy) = MB_TYPE_SKIP | MB_TYPE_DIRECT;
            let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
            nzc_mb.fill(0);
            pDec.pRefIndex[LIST_0].get_mut(iMbXy).fill(0);
            pDec.pRefIndex[LIST_1].get_mut(iMbXy).fill(0);

            // F42's clause, as above.
            let is_complete0 = pRefs
                .resolve(pCtx.ref_id(LIST_0, 0), Some(&*pDec))
                .is_some_and(|p| p.bIsComplete || bIsPending);
            let is_complete1 = pRefs
                .resolve(pCtx.ref_id(LIST_1, 0), Some(&*pDec))
                .is_some_and(|p| p.bIsComplete || bIsPending);
            *pCtx.bMbRefConcealed =
                pCtx.bRPLRError || *pCtx.bMbRefConcealed || !is_complete0 || !is_complete1;

            if *pCtx.bMbRefConcealed {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
            }

            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag != 0 {
                ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                    pCtx, &mut *dq,
                    &mut *pDec,
                    pRefs,
                    &mut pMv,
                    &mut ref_idx,
                    &mut subMbType,
                );
                if ret != ERR_NONE {
                    return ret;
                }
            } else {
                ret = crate::decoder::mv_pred::PredBDirectTemporal(
                    pCtx, &mut *dq,
                    pDec,
                    pRefs,
                    &mut pMv,
                    &mut ref_idx,
                    &mut subMbType,
                );
                if ret != ERR_NONE {
                    return ret;
                }
            }

            let last_qp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = last_qp as i8;
            let pps_chroma_qp_offset = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).map_or([0i32; 2], |p| p.iChromaQpIndexOffset);
            let pps_entropy = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bEntropyCodingModeFlag);
            let pps_transform8x8 = pCtx.pps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id).is_some_and(|p| p.bTransform8x8ModeFlag);
            for i in 0..2 {
                let idx =
                    WELS_CLIP3(last_qp + pps_chroma_qp_offset[i] as i32, 0, 51);
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }

            (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;

            ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                pCtx,
                uiEosFlag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            return ERR_NONE;
        }

        WelsDecodeMbCabacBSliceBaseMode0(pCtx,
            pNalCur, dq, pDec, pRefs, &mut sNeighAvail, uiEosFlag)
    }
}

// ============================================================================
// Top-Level Slice Decoding Orchestrators
// ============================================================================

pub fn WelsDecodeSlice(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: &mut DqLayerState,
    bFirstSliceInLayer: bool,
    nal_idx: Option<usize>,
) -> i32 {
    // **The bracket top** (T5.Y2): the slice header's parameter sets are selected,
    // the CABAC engine is initialized and the scaling lists are computed *before*
    // the split, because the view copies those scalars; below the split the context
    // is not reachable at all and `pDec`/`pRefs` travel as their own parameters.

    // **The loop this function's aliasing probe was written for, and the prediction
    // session D read out of the code is now a Miri finding (T5.E1).** This held
    // `ctx = &mut *pCtx`, `dq = &mut *pCurDqLayer` and two reborrows of `dq` across
    // `pDecMbFunc(pCtx, …)`, which re-enters through `pCtx` and reaches the same layer:
    // the callee's own `&mut *pCtx` invalidated the outer `[0x0..0x8ae00]` retag, and
    // `ctx.bMbRefConcealed = false` on the next iteration wrote through the dead tag.
    // Nothing is a borrow now — `(*pCtx)` / `(*pCurDqLayer)` per use, and the two nested
    // pointers derive from the layer without retagging, so re-entry cannot invalidate
    // them. T5.G1 removed the last of the invalidators: there is no `&mut *pCtx` left in
    // `src/decoder/`, so no callee can retag the context out from under a caller.
    // S25's shape (plan §7.6); S29 is the spelling.

    (*pCurDqLayer).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice = 0;

    let pDecMbFunc: PWelsDecMbFunc = if active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
        .is_some_and(|pps| pps.bEntropyCodingModeFlag)
    {
        if (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCabacPSlice
        } else if (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCabacBSlice
        } else {
            WelsDecodeMbCabacISlice
        }
    } else {
        if (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCavlcPSlice
        } else if (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCavlcBSlice
        } else {
            WelsDecodeMbCavlcISlice
        }
    };

    // `pSliceHeader->pPps` in decode_slice.cpp; the slice header stores it opaquely.
    // T4b.3: the `if` that used to fill three laundered slots *is* the assignment
    // now. A null PPS keeps the `Constrain0` arm the old `else` gave it.
    let bConstrainedIntra = pps_of(
        &(*pCtx).sSpsPpsCtx,
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id,
    )
    .is_some_and(|pps| pps.bConstainedIntraPredFlag);
    (*pCtx).eIntraPredConstraint = IntraPredConstraint::from_flag(bConstrainedIntra);

    (*pCtx).eSliceType = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType;
    if pps_of(&(*pCtx).sSpsPpsCtx, (*pCurDqLayer).sLayerInfo.pps_id)
        .is_some_and(|pps| pps.bEntropyCodingModeFlag)
    {
        let iQp = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iSliceQp;
        let iCabacInitIdc = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iCabacInitIdc;
        crate::decoder::cabac_decoder::WelsCabacContextInit(
            &mut (*pCtx).sWelsCabacContexts,
            &mut (*pCtx).bCabacInited,
            &mut (*pCtx).pCabacCtx,
            (*pCurDqLayer).sLayerInfo.sSliceInLayer.eSliceType,
            iCabacInitIdc,
            iQp,
        );
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
        // T5b.3: the node's own reader, reached the way everything else in this
        // function reaches the access unit — by index, in one expression.
        let err = match nal_idx.and_then(|i| {
            cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(i))
        }) {
            Some(nal) => {
                let reader = &mut nal.sNalData.sVclNal.sSliceBitsRead;
                // Three disjoint fields, split at the context rather than derived
                // through it: the node comes out of `access_unit`, and the engine and
                // the raw buffer are named beside it.
                crate::decoder::cabac_decoder::InitCabacDecEngineFromBS(
                    &mut pCtx.sCabacDecEngine,
                    reader,
                    &pCtx.sRawData,
                )
            }
            None => return ERR_NONE,
        };
        if err != ERR_NONE {
            return err;
        }
    }
    WelsCalcDeqCoeffScalingList(pCtx);

    // **The split.** Everything above this line is the context's; everything below
    // it is the view's, and the pool borrow the two halves of `cur_and_refs` carry
    // coexists with it because `pPicBuff` is not in the view.
    let (pDec, pRefs, mut view, nal) = slice_split(pCtx, nal_idx);
    let Some(pNalCur) = nal else {
        return ERR_NONE;
    };
    // T5b.2: with the picture a borrow, the one place its absence is tested is here
    // rather than at every level below. `DecodeCurrentAccessUnit` prefetches it and
    // returns `ERR_INFO_REF_COUNT_OVERFLOW` when the pool cannot supply one, so this
    // arm is unreachable from the decode path — same condition, same code.
    let Some(pDec) = pDec else {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REF_COUNT_OVERFLOW);
    };
    let pCtx = &mut view;

    let mut iNextMbXyIndex = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    if (*pCurDqLayer).iMbWidth > 0 {
        (*pCurDqLayer).iMbX = iNextMbXyIndex % (*pCurDqLayer).iMbWidth;
        (*pCurDqLayer).iMbY = iNextMbXyIndex / (*pCurDqLayer).iMbWidth;
    }
    (*pCurDqLayer).iMbXyIndex = iNextMbXyIndex;
    (*pCurDqLayer).sLayerInfo.sSliceInLayer.iMbSkipRun = -1;
    let iSliceIdc = ((*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice << 7) + (*pCurDqLayer).uiLayerDqId as i32;

    let kiCountNumMb = pCtx
        .sps_of((*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref)
        .map_or(0, |sps| sps.uiTotalMbCount as i32);

    let mut uiEosFlag: u32 = 0;

    loop {
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
            break;
        }

        *(*pCurDqLayer).grid.slice_idc.get_mut(iNextMbXyIndex as usize) = iSliceIdc;
        *pCtx.bMbRefConcealed = false;
        let iRet = pDecMbFunc(pCtx, pCurDqLayer, &mut *pDec, pRefs, pNalCur, &mut uiEosFlag);
        *(*pCurDqLayer).grid.mb_ref_concealed_flag.get_mut(iNextMbXyIndex as usize) =
            *pCtx.bMbRefConcealed;
        if iRet != ERR_NONE {
            return iRet;
        }

        (*pCurDqLayer).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice += 1;
        if uiEosFlag != 0 {
            break;
        }

        if pCtx.active_pps().is_some_and(|pps| pps.uiNumSliceGroups > 1) {
            iNextMbXyIndex = crate::decoder::fmo::FmoNextMb(pCtx.active_fmo(), iNextMbXyIndex);
        } else {
            iNextMbXyIndex += 1;
        }
        if (*pCurDqLayer).iMbWidth > 0 {
            (*pCurDqLayer).iMbX = iNextMbXyIndex % (*pCurDqLayer).iMbWidth;
            (*pCurDqLayer).iMbY = iNextMbXyIndex / (*pCurDqLayer).iMbWidth;
        }
        (*pCurDqLayer).iMbXyIndex = iNextMbXyIndex;
    }

    ERR_NONE
}

pub fn WelsDecodeAndConstructSlice(pCtx: &mut SWelsDecoderContext, pCurDqLayer: &mut DqLayerState) -> i32 {
    {
        // T5b.3: `pCtx->pNalCur` is an index now, and `slice_split` resolves it in the
        // same statement it splits the context — which is what lets the node's bit
        // reader travel beside the view. The `None` arm is the C's null `pNalCur`; F36
        // owns it as before.
        let Some(iNalCur) = (*pCtx).nal_cur else {
            return ERR_NONE;
        };
        let dq: &mut DqLayerState = pCurDqLayer;

        (*dq).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice = 0;

        let pDecMbFunc: PWelsDecMbFunc = if active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
            .is_some_and(|pps| pps.bEntropyCodingModeFlag)
        {
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
                WelsDecodeMbCabacPSlice
            } else if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
                WelsDecodeMbCabacBSlice
            } else {
                WelsDecodeMbCabacISlice
            }
        } else {
            if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
                WelsDecodeMbCavlcPSlice
            } else if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
                WelsDecodeMbCavlcBSlice
            } else {
                WelsDecodeMbCavlcISlice
            }
        };

        // `pSliceHeader->pPps` in decode_slice.cpp; the slice header stores it opaquely.
        // T4b.3: the `if` that used to fill three laundered slots *is* the assignment
        // now. A null PPS keeps the `Constrain0` arm the old `else` gave it.
        let bConstrainedIntra = pps_of(
            &(*pCtx).sSpsPpsCtx,
            (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id,
        )
        .is_some_and(|pps| pps.bConstainedIntraPredFlag);
        (*pCtx).eIntraPredConstraint = IntraPredConstraint::from_flag(bConstrainedIntra);

        (*pCtx).eSliceType = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.eSliceType;
        WelsCalcDeqCoeffScalingList(pCtx);

        // **The split** — `WelsDecodeSlice`'s, one function down: the pool's two halves
        // and the view come out of the same context in the same statement group, and
        // nothing below the loop reaches the context again.
        let (pDec, pRefs, mut view, nal) = slice_split(pCtx, Some(iNalCur));
        let Some(pNalCur) = nal else {
            return ERR_NONE;
        };
        // `WelsDecodeSlice`'s guard, same reason, same code.
        let Some(pDec) = pDec else {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REF_COUNT_OVERFLOW);
        };
        let pCtx = &mut view;

        let mut iNextMbXyIndex = (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
        if (*dq).iMbWidth > 0 {
            (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
            (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
        }
        (*dq).iMbXyIndex = iNextMbXyIndex;

        let kiCountNumMb = pCtx
            .sps_of((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref)
            .map_or(0, |sps| sps.uiTotalMbCount as i32);

        let mut uiEosFlag: u32 = 0;

        loop {
            if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
                break;
            }

            *pCtx.bMbRefConcealed = false;
            let iRet = pDecMbFunc(pCtx, dq, &mut *pDec, pRefs, pNalCur, &mut uiEosFlag);
            *(*dq).grid.mb_ref_concealed_flag.get_mut(iNextMbXyIndex as usize) = *pCtx.bMbRefConcealed;
            if iRet != ERR_NONE {
                return iRet;
            }

            let ret = WelsTargetMbConstruction(pCtx, dq, Some(&mut *pDec), pRefs);
            if ret != ERR_NONE {
                return ERR_INFO_MB_RECON_FAIL;
            }

            let idx = iNextMbXyIndex as usize;
            if !*(*dq).grid.mb_correctly_decoded_flag.get(idx) {
                *(*dq).grid.mb_correctly_decoded_flag.get_mut(idx) = true;
                if *(*dq).grid.mb_ref_concealed_flag.get(idx) {
                    pDec.iMbEcedPropNum += 1;
                }
                *pCtx.iTotalNumMbRec += 1;
            }

            (*dq).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice += 1;
            if uiEosFlag != 0 {
                break;
            }

            iNextMbXyIndex += 1;
            if (*dq).iMbWidth > 0 {
                (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
                (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
            }
            (*dq).iMbXyIndex = iNextMbXyIndex;
        }

        ERR_NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::mb_grid::MbDims;

    // T5.X3: these three asserted "a null layer returns ERR_NONE". The layer is a
    // `&mut` now, so the null arm is unrepresentable *here* — and the guard did not
    // disappear, it moved to the boundary where a raw layer still arrives
    // (`decoder_core`'s forwarding shims, which own the `as_mut()`). The tests
    // follow the behaviour: the null-context arm is still asserted at these three
    // functions, and the null-*layer* arm is asserted where it now lives.
    // **T5.Z4: the null-*context* arm is unrepresentable everywhere in the decoder
    // now.** T5.Y2 moved it to the three bracket tops that still took a raw context;
    // the flip took the context off them too, so there is no decoder site left where
    // a null context can be constructed to test. The guard did not disappear — it is
    // `api/codec_api.rs`'s, at the five entry points that still hold a pointer, and
    // the api tests below drive it there. What remains testable here is the
    // null-*layer* arm, which `decoder_core`'s three forwarding shims still own.

    #[test]
    fn test_wels_target_slice_construction_null_layer() {
        {
            {
                let mut ctx = SWelsDecoderContext::new_boxed();
                // The missing-layer arm, at the boundary it moved to (T5.X3, T5.AA1):
                // `decoder_core`'s three forwarding shims are where a layer may still be
                // absent, and each answers `None` the way the per-macroblock callees
                // answered a null pointer. The null is a `None` since the alias died.
                assert_eq!(
                    crate::decoder::decoder_core::WelsTargetSliceConstruction(&mut ctx, None),
                    ERR_NONE
                );
                assert_eq!(
                    crate::decoder::decoder_core::WelsDecodeSlice(&mut ctx, None, false, None),
                    ERR_NONE
                );
                assert_eq!(
                    crate::decoder::decoder_core::WelsDecodeAndConstructSlice(&mut ctx, None),
                    ERR_NONE
                );
            }
        }
    }

    #[test]
    fn test_wels_calc_deq_coeff_scaling_list() {
        {
            {
                // T5.R6: the active parameter sets are ids into the context's own
                // buffers, so the fixture fills the buffers rather than pointing the
                // context at two stack locals — which is the aliasing the ids remove.
                let mut ctx = SWelsDecoderContext::new_boxed();
                ctx.sSpsPpsCtx.sSpsBuffer[0].bSeqScalingMatrixPresentFlag = true;
                ctx.sSpsPpsCtx.sSpsBuffer[0].iScalingList4x4[0][0] = 16;
                ctx.sSpsPpsCtx.sPpsBuffer[1].iPpsId = 1;
                ctx.active_sps = Some(SpsRef { id: 0, subset: false });
                ctx.active_pps = Some(1);
                let res = WelsCalcDeqCoeffScalingList(&mut *ctx);
                assert_eq!(res, ERR_NONE);
                assert!(ctx.bUseScalingList);
                assert!(ctx.bDequantCoeff4x4Init);
                assert_eq!(ctx.iDequantCoeffPpsid, 1);
            }
        }
    }

    // T5.Y2: `test_wels_decode_mb_cavlc_slices_null` stood here, asserting that the
    // two CAVLC dispatch entry points answer a null context with `ERR_NONE`. Both
    // take the slice view and a NAL borrow now, so neither argument can be null —
    // the arm is the bracket tops', asserted above.

    // **The one-macroblock clean-stream probe stood here, and was retired at T5.S3.**
    //
    // It decoded `narrow_16x16.264` and asserted a frame came out at 16x16, and it
    // cost **265.3s of the library's 989.2s** under Miri against 0.001s natively —
    // the whole of its cost, and the whole of its unique value, is the aliasing
    // verdict.
    //
    // **The stream is still decoded**, by `test_asset_narrow_16x16` in
    // `decoder_conformance_test.rs`, which checks its full SHA-1 against the C++
    // decoder's — strictly more than this probe asserted. Retiring a probe whose
    // vector another test already runs is the rule that removed it (Eugene,
    // 2026-08-14).
    //
    // **What that costs, stated because it is not nothing.** The conformance suite
    // is not a Miri target, so no aliasing verdict covers this stream any more.
    // Counting decode entries per probe stream shows the gap precisely:
    //
    //     stream                  CavlcPcm CavlcI CavlcP  CabacI CabacP CabacB
    //     narrow_16x16                   0      0      0       3     21      0
    //     narrow_16x16_idr_lost          0      3     21       0      7      0
    //     grid_48x32                     0      0      0       6     18      7
    //     fmo_2groups_64x64             16     16      0       0      0      0
    //
    // CABAC I-slices on a one-macroblock-per-frame picture are now unprobed:
    // `grid_48x32` reaches `CabacI` but at 3x2 with different stride arithmetic, and
    // `narrow_16x16_idr_lost` reaches it **zero** times, because in that stream the
    // CABAC sequence is exactly the one whose IDR was removed.
    //
    // *(An earlier version of this note claimed `narrow_16x16_idr_lost` was a strict
    // superset and that nothing was lost. That was a syntactic argument — profile,
    // entropy mode, slice count — and the table above is what disproved it. Which
    // decode entries run is a property of slice types after damage, not of the
    // parameter sets.)*
    //
    // **Why deleting a probe is the lever and shortening one is not.** The probes
    // cost 268.9s / 265.3s / 257.5s / 196.0s for 36 / 24 / 31 / 16 macroblocks: cost
    // tracks the number of decoder instantiations, not decoding work, because a full
    // `Initialize` of a multi-MiB context under an interpreter dwarfs the
    // macroblocks. Trimming a stream saves nothing.
    //
    // F34's lesson — one macroblock per frame was a ceiling, and a real UB hid under
    // it — is recorded on the grid probe below, which exists because of it.


    /// The same gate over a stream that has **neighbours** — Phase 5 session J,
    /// D-perf-5's "probe first, then flip".
    ///
    /// The test above bounds every Miri verdict this phase issued, and F34 is what
    /// that cost: a real UB in `WelsDecodeMbCabacIntraModeHelper` that Miri
    /// *executed* and returned green on, because `narrow_16x16.264` is one
    /// macroblock per frame — `iLeftAvail` and `iTopAvail` are 0 at every
    /// macroblock in it, so the two lines that make the function UB were
    /// unreachable. S22's law aimed at a stream instead of at a scope list.
    ///
    /// `grid_48x32.264` is 3x2 macroblocks, so MB(1,1) has all four neighbours,
    /// MB(0,1) is missing only its left and MB(2,1) only its top-right: every
    /// availability combination the neighbour paths branch on. It is CABAC, High
    /// profile with `transform_8x8_mode_flag` set, carries I, P **and** B slices,
    /// and its source window pans so the MVs are non-zero. Built by
    /// `rust/tools/make_narrow_assets.py`, which explains at length why this one
    /// asset comes from libx264 and not from the C++ encoder: **OpenH264's encoder
    /// has no `transform_8x8_mode_flag` to write**, and F34 sits behind it.
    ///
    /// **Coverage is proven, not asserted** (the F21 rule): with T5.I2's fix
    /// reverted in a scratch worktree, this test goes red under Miri at
    /// `parse_mb_syn_cabac.rs:1242` — the callee's own left-neighbour read —
    /// while `decode_slice_loop_runs_under_the_aliasing_checker` stays green.
    /// Session J's log records the run.
    ///
    /// The dimension assertion is not decoration. A regenerated asset that
    /// silently came out 16x16 would still pass "a frame came out" while covering
    /// nothing this test exists for, which is exactly how F34 survived.
    #[test]
    fn decode_slice_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker() {
        {
            const GRID_48X32: &[u8] = include_bytes!("../../../../../res/grid_48x32.264");
            let (frames, dims, states) = drive_decoder_over(GRID_48X32);
            assert!(
                frames > 0,
                "no frame came out of grid_48x32.264 — the slice loop was never entered, \
                 so this test is not measuring what it claims to (states = {states:#x})"
            );
            assert_eq!(
                dims,
                Some((48, 32)),
                "grid_48x32.264 must decode as a 3x2 macroblock grid; a stream without \
                 neighbours covers nothing this test exists for"
            );
        }
    }

    // **T5b.6: `drive_decoder_over` moved to `api/codec_api.rs`.** It drives the
    // vtable thunks through a raw `*mut ISVCDecoder` rather than through the
    // `&mut self` convenience methods, deliberately and for F23's reason — which
    // makes its `unsafe` the C ABI's, not this module's. The three probes below are
    // unchanged and call it by path.
    use crate::api::codec_api::abi_test_driver::drive_decoder_over;

    /// **The error-concealment probe** — Phase 5 session S, F43/F44/F45.
    ///
    /// Until T5.S1 this path could not be probed, because it did not run: five stubs
    /// in `decoder_core.rs` shadowed `error_concealment.rs` (F43), `InitErrorCon` had
    /// no caller so `sCopyFunc` was `None` and the copies were silent no-ops (F44),
    /// and `bInstantDecFlag` was never written so the emission gate never fired
    /// (F45). Every Miri verdict this phase issued was on a decoder whose whole
    /// concealment subsystem was unreachable — **a new container, in S29's sense,
    /// arriving at the end of the phase rather than the start**.
    ///
    /// `narrow_16x16_idr_lost.264` is 827 bytes and one macroblock per frame — the
    /// cheapest stream in the tree that actually conceals, which is what makes a
    /// full `Initialize` + 30-frame decode tractable under an interpreter. Its
    /// second sequence has no IDR to open it, so the first P slice finds the
    /// reference lists empty and the concealment path runs for real: measured,
    /// `dsDataErrorConcealed` comes back set. Its output is pinned against the C++
    /// decoder elsewhere (`test_asset_narrow_16x16_idr_lost`, and every truncation
    /// of it in `malformed_parity/narrow_16x16_idr_lost.txt` agrees with `ecref`),
    /// so here we require only that concealment ran — the instrument is Miri's
    /// verdict on the path, not the bytes.
    ///
    /// The `dsDataErrorConcealed` assertion is the point. Without it this test
    /// passes just as well on a decoder where concealment never runs — which is
    /// precisely the state the port was in for five phases.
    ///
    /// **It also carries the retired clean-stream probe's duty** (T5.S3): the
    /// dimension assertion below was that probe's, and this stream is its superset
    /// in every syntactic dimension, so the geometry check moves here rather than
    /// disappearing with it.
    #[test]
    fn error_concealment_runs_under_the_aliasing_checker() {
        {
            const IDR_LOST: &[u8] = include_bytes!("../../../../../res/narrow_16x16_idr_lost.264");
            let (frames, dims, states) = drive_decoder_over(IDR_LOST);
            assert!(frames > 0, "no frame came out of narrow_16x16_idr_lost.264");
            assert_eq!(dims, Some((16, 16)), "this stream is one macroblock per frame");
            assert_ne!(
                states & 0x20,
                0,
                "dsDataErrorConcealed never set: concealment did not run, so this test is \
                 not measuring what it claims to (states = {states:#x})"
            );
        }
    }

    /// **The FMO probe** — Phase 5 session S, F43.
    ///
    /// `fmo.rs` was unreachable in production: `decoder_core.rs`'s `FmoNextMb` stub
    /// returned `iMbIdx + 1` and nothing ever wrote `pCtx->pFmo`. It has therefore
    /// never been under the aliasing checker in the shape a stream drives it —
    /// `FmoParamUpdate` writing the map at paramset activation, `FmoNextMb` reading
    /// it once per macroblock through the context's `sFmoList` entry.
    ///
    /// `fmo_2groups_64x64.264` is built by `rust/tools/make_fmo_asset.py` because no
    /// stream in `res/` has more than one slice group. 16 macroblocks over two
    /// interleaved groups, all I_PCM, one frame — the cheapest thing that makes the
    /// map decide anything.
    #[test]
    fn fmo_slice_group_walk_runs_under_the_aliasing_checker() {
        {
            const FMO: &[u8] = include_bytes!("../../../../../res/fmo_2groups_64x64.264");
            let (frames, dims, states) = drive_decoder_over(FMO);
            assert_eq!(frames, 1, "fmo_2groups_64x64.264 is one frame (states = {states:#x})");
            assert_eq!(dims, Some((64, 64)));
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_NEON, WELS_CPU_SSE2};
pub use crate::decoder::dec_golomb::{g_kuiIntra4x4CbpTable, g_kuiIntra4x4CbpTable400};
