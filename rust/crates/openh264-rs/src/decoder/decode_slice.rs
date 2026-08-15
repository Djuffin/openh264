#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut
)]

use crate::decoder::decoder_context::{
    PicRefs, SpsRef, active_fmo, active_pps, active_sps, cur_and_refs, pps_of, ref_id, sps_of,
};
use crate::safe::bits::BsCursor;
use crate::decoder::bit_stream::{BsReader, slice_bit_reader};
use std::ffi::c_void;

// ============================================================================
// Constants & Error Codes
// ============================================================================

pub const ERR_NONE: i32 = 0;
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
    (sub_type & SUB_MB_TYPE_4x4) != 0
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
pub type PWelsDecMbFunc = unsafe extern "C" fn(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32;

// T5.R7: `PFillInfoCacheIntraNxNFunc`, `PMapNxNNeighToSampleFunc` and
// `PMap16x16NeighToSampleFunc` stood here with **no uses at all** — Phase 4b turned
// their three dispatch slots into an enum method and two direct calls and left the
// typedefs behind. S18's straggler class, deleted where it was found.

pub type PIdctResAddPredFunc = unsafe extern "C" fn(
    pDst: *mut u8,
    iStride: i32,
    pScaledTCoeff: *mut i16,
    pNzc: *const i8,
);

pub type PIdctResAddPredFunc8x8 = unsafe extern "C" fn(
    pDst: *mut u8,
    iStride: i32,
    pScaledTCoeff: *mut i16,
);

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
    DqLayerState, PDqLayer, SLayerInfo, ERR_INFO_INVALID_PTR, ERR_INFO_INVALID_ACCESS, ERR_INFO_INVALID_PARAM,
    mb_grid_ptr,
};
pub use crate::decoder::nalu::{SNalUnit, PNalUnit};




pub use crate::decoder::picture::{SPicture, PPicture};





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

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};


// ============================================================================
// Core Utility & Scaling Functions
// ============================================================================

pub unsafe fn CheckRefPics(pCtx: *const SWelsDecoderContext) -> bool {
    if pCtx.is_null() {
        return false;
    }
    let ctx = &*pCtx;
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

pub unsafe fn ComputeColocatedTemporalScaling(pCtx: *mut SWelsDecoderContext, pCurDqLayer: *mut DqLayerState, pRefs: PicRefs<'_>) -> bool {
    if pCtx.is_null() {
        return false;
    }
    if pCurDqLayer.is_null() {
        return false;
    }
    let pCurSlice: *mut SSlice = std::ptr::addr_of_mut!((*pCurDqLayer).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pCurSlice).sSliceHeaderExt.sSliceHeader);

    if (*pSliceHeader).iDirectSpatialMvPredFlag == 0 {
        let uiRefCount = (*pSliceHeader).uiRefCount[LIST_0];
        let pic1 = pRefs.get(ref_id(pCtx, LIST_1, 0));
        if !pic1.is_null() {
            for i in 0..uiRefCount {
                let pic0 = pRefs.get(ref_id(pCtx, LIST_0, i as usize));
                if !pic0.is_null() {
                    let poc0 = (*pic0).iFramePoc;
                    let poc1 = (*pic1).iFramePoc;
                    let poc = (*pSliceHeader).iPicOrderCntLsb;
                    let td = WELS_CLIP3(poc1 - poc0, -128, 127);
                    if td == 0 {
                        (*pCurSlice).iMvScale[LIST_0][i as usize] = 1 << 8;
                    } else {
                        let tb = WELS_CLIP3(poc - poc0, -128, 127);
                        let tx = (16384 + (td.abs() >> 1)) / td;
                        (*pCurSlice).iMvScale[LIST_0][i as usize] =
                            WELS_CLIP3((tb * tx + 32) >> 6, -1024, 1023) as i16;
                    }
                }
            }
        }
    }
    true
}

pub unsafe fn WelsCalcDeqCoeffScalingList(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if active_sps(pCtx).is_null() || active_pps(pCtx).is_null() {
        return ERR_NONE;
    }
    if (*active_sps(pCtx)).bSeqScalingMatrixPresentFlag || (*active_pps(pCtx)).bPicScalingMatrixPresentFlag {
        (*pCtx).bUseScalingList = true;

        if !(*pCtx).bDequantCoeff4x4Init || (*pCtx).iDequantCoeffPpsid != (*active_pps(pCtx)).iPpsId {
            for i in 0..6 {
                (*pCtx).pDequant_coeff4x4[i] = (*pCtx).pDequant_coeff_buffer4x4[i].as_mut_ptr();
                (*pCtx).pDequant_coeff8x8[i] = (*pCtx).pDequant_coeff_buffer8x8[i].as_mut_ptr();
                for q in 0..51 {
                    for x in 0..16 {
                        let scale4 = if (*active_pps(pCtx)).bPicScalingMatrixPresentFlag {
                            (*active_pps(pCtx)).iScalingList4x4[i][x] as u32
                        } else {
                            (*active_sps(pCtx)).iScalingList4x4[i][x] as u32
                        };
                        (*pCtx).pDequant_coeff_buffer4x4[i][q][x] =
                            (scale4 * (g_kuiDequantCoeff[q][x & 0x07] as u32)) as u16;
                    }
                    for y in 0..64 {
                        let scale8 = if (*active_pps(pCtx)).bPicScalingMatrixPresentFlag {
                            (*active_pps(pCtx)).iScalingList8x8[i][y] as u32
                        } else {
                            (*active_sps(pCtx)).iScalingList8x8[i][y] as u32
                        };
                        (*pCtx).pDequant_coeff_buffer8x8[i][q][y] =
                            (scale8 * (g_kuiMatrixV[q % 6][y / 8][y % 8] as u32)) as u16;
                    }
                }
            }
            (*pCtx).bDequantCoeff4x4Init = true;
            (*pCtx).iDequantCoeffPpsid = (*active_pps(pCtx)).iPpsId;
        }
    } else {
        (*pCtx).bUseScalingList = false;
    }
    ERR_NONE
}

// ============================================================================
// Inverse Transform & Dequantization Functions
// ============================================================================

pub unsafe fn WelsLumaDcDequantIdct(pBlock: *mut i16, iQp: i32, pCtx: *mut SWelsDecoderContext) {
    if pBlock.is_null() || pCtx.is_null() {
        return;
    }
    let qp = WELS_CLIP3(iQp, 0, 51) as usize;
    let kiQMul: i32 = (g_kuiDequantCoeff[qp][0] as i32) << 4;

    let stride = 16;
    let kiXOffset: [i32; 4] = [0, stride, stride << 2, 5 * stride];
    let kiYOffset: [i32; 4] = [0, stride << 1, stride << 3, 10 * stride];
    let mut iTemp = [0i32; 16];

    for i in 0..4 {
        let kiOffset = kiYOffset[i] as isize;
        let kiX1 = (kiYOffset[i] + kiXOffset[2]) as isize;
        let kiX2 = (stride + kiYOffset[i]) as isize;
        let kiX3 = (kiYOffset[i] + kiXOffset[3]) as isize;
        let kiI4 = i << 2;

        let kiZ0 = (*pBlock.offset(kiOffset) as i32) + (*pBlock.offset(kiX1) as i32);
        let kiZ1 = (*pBlock.offset(kiOffset) as i32) - (*pBlock.offset(kiX1) as i32);
        let kiZ2 = (*pBlock.offset(kiX2) as i32) - (*pBlock.offset(kiX3) as i32);
        let kiZ3 = (*pBlock.offset(kiX2) as i32) + (*pBlock.offset(kiX3) as i32);

        iTemp[kiI4] = kiZ0 + kiZ3;
        iTemp[1 + kiI4] = kiZ1 + kiZ2;
        iTemp[2 + kiI4] = kiZ1 - kiZ2;
        iTemp[3 + kiI4] = kiZ0 - kiZ3;
    }

    for i in 0..4 {
        let kiOffset = kiXOffset[i] as isize;
        let kiI4 = 4 + i;

        let kiZ0 = iTemp[i] + iTemp[4 + kiI4];
        let kiZ1 = iTemp[i] - iTemp[4 + kiI4];
        let kiZ2 = iTemp[kiI4] - iTemp[8 + kiI4];
        let kiZ3 = iTemp[kiI4] + iTemp[8 + kiI4];

        *pBlock.offset(kiOffset) = (((kiZ0 + kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.offset((kiYOffset[1] as isize) + kiOffset) =
            (((kiZ1 + kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.offset((kiYOffset[2] as isize) + kiOffset) =
            (((kiZ1 - kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.offset((kiYOffset[3] as isize) + kiOffset) =
            (((kiZ0 - kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
    }
}

pub unsafe fn WelsChromaDcIdct(pBlock: *mut i16) {
    if pBlock.is_null() {
        return;
    }
    let iStride: isize = 32;
    let iXStride: isize = 16;
    let iStride1: isize = iXStride + iStride;

    let mut iA = *pBlock.offset(0) as i32;
    let mut iB = *pBlock.offset(iXStride) as i32;
    let mut iC = *pBlock.offset(iStride) as i32;
    let iD = *pBlock.offset(iStride1) as i32;

    let iE = iA - iB;
    iA += iB;
    iB = iC - iD;
    iC += iD;

    *pBlock.offset(0) = (iA + iC) as i16;
    *pBlock.offset(iXStride) = (iE + iB) as i16;
    *pBlock.offset(iStride) = (iA - iC) as i16;
    *pBlock.offset(iStride1) = (iE - iB) as i16;
}

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
/// `PWelsNeighAvail` / `PDqLayer` and two of them are not `extern "C"` at all. So
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
    pub unsafe fn FillCacheIntraNxN(
        self,
        pNeighAvail: crate::decoder::parse_mb_syn_cavlc::PWelsNeighAvail,
        pNonZeroCount: &mut [u8; 48],
        pIntraPredMode: *mut i8,
        pCurDqLayer: crate::decoder::decoder_core::PDqLayer,
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
    pub unsafe fn MapNxNNeighToSample(
        self,
        pNeighAvail: *mut SWelsNeighAvail,
        pSampleAvail: *mut i32,
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
    pub unsafe fn Map16x16NeighToSample(
        self,
        pNeighAvail: *mut SWelsNeighAvail,
        pSampleAvail: *mut u8,
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

pub unsafe extern "C" fn WelsMapNxNNeighToSampleNormal(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut i32,
) {
    if pNeighAvail.is_null() || pSampleAvail.is_null() {
        return;
    }
    let avail = &*pNeighAvail;
    if avail.iLeftAvail != 0 {
        *pSampleAvail.add(6) = 1;
        *pSampleAvail.add(12) = 1;
        *pSampleAvail.add(18) = 1;
        *pSampleAvail.add(24) = 1;
    }
    if avail.iLeftTopAvail != 0 {
        *pSampleAvail.add(0) = 1;
    }
    if avail.iTopAvail != 0 {
        *pSampleAvail.add(1) = 1;
        *pSampleAvail.add(2) = 1;
        *pSampleAvail.add(3) = 1;
        *pSampleAvail.add(4) = 1;
    }
    if avail.iRightTopAvail != 0 {
        *pSampleAvail.add(5) = 1;
    }
}

pub unsafe extern "C" fn WelsMapNxNNeighToSampleConstrain1(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut i32,
) {
    if pNeighAvail.is_null() || pSampleAvail.is_null() {
        return;
    }
    let avail = &*pNeighAvail;
    if avail.iLeftAvail != 0 && IS_INTRA(avail.iLeftType) {
        *pSampleAvail.add(6) = 1;
        *pSampleAvail.add(12) = 1;
        *pSampleAvail.add(18) = 1;
        *pSampleAvail.add(24) = 1;
    }
    if avail.iLeftTopAvail != 0 && IS_INTRA(avail.iLeftTopType) {
        *pSampleAvail.add(0) = 1;
    }
    if avail.iTopAvail != 0 && IS_INTRA(avail.iTopType) {
        *pSampleAvail.add(1) = 1;
        *pSampleAvail.add(2) = 1;
        *pSampleAvail.add(3) = 1;
        *pSampleAvail.add(4) = 1;
    }
    if avail.iRightTopAvail != 0 && IS_INTRA(avail.iRightTopType) {
        *pSampleAvail.add(5) = 1;
    }
}

pub unsafe extern "C" fn WelsMap16x16NeighToSampleNormal(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut u8,
) {
    if pNeighAvail.is_null() || pSampleAvail.is_null() {
        return;
    }
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

pub unsafe extern "C" fn WelsMap16x16NeighToSampleConstrain1(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut u8,
) {
    if pNeighAvail.is_null() || pSampleAvail.is_null() {
        return;
    }
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

pub unsafe fn WelsMbInterSampleConstruction(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut DqLayerState,
    pDstY: *mut u8,
    pDstU: *mut u8,
    pDstV: *mut u8,
    iStrideL: i32,
    iStrideC: i32,
) -> i32 {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let ctx = &*pCtx;
    // T5.L5: a raw layer pointer, as every other function in this file spells it.
    // It was `&*pCurDqLayer` while the coefficient array was reached through a
    // pointer *field* — a `Copy` read that a shared borrow allows. Owned, the array
    // is reached with `get_mut`, and taking a `&mut` of a subfield through a live
    // `&DqLayerState` is the shape F28 names.
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;

    let pTransformSize8x8 = *(*dq).grid.transform_size8x8_flag.get(iMbXy);
    let pNzc = *(*dq).grid.nzc.get(iMbXy);
    let pScaledTCoeff = (*dq).grid.scaled_tcoeff.get_mut(iMbXy).as_mut_ptr();


    if pTransformSize8x8 {
        if let Some(idct8x8) = ctx.pIdctResAddPredFunc8x8 {
            for i in 0..4 {
                let iIndex = g_kuiMbCountScan4Idx[i << 2] as usize;
                if pNzc[iIndex] != 0
                    || pNzc[iIndex + 1] != 0
                    || pNzc[iIndex + 4] != 0
                    || pNzc[iIndex + 5] != 0
                {
                    let iOffset =
                        ((iIndex >> 2) << 2) as i32 * iStrideL + ((iIndex % 4) << 2) as i32;
                    idct8x8(pDstY.offset(iOffset as isize), iStrideL, pScaledTCoeff.add(i << 6));
                }
            }
        }
    } else {
        if let Some(idct4x4) = ctx.pIdctFourResAddPredFunc {
            idct4x4(pDstY.offset(0), iStrideL, pScaledTCoeff.add(0), pNzc.as_ptr().add(0));
            idct4x4(pDstY.offset(8), iStrideL, pScaledTCoeff.add(64), pNzc.as_ptr().add(2));
            idct4x4(
                pDstY.offset((8 * iStrideL) as isize),
                iStrideL,
                pScaledTCoeff.add(128),
                pNzc.as_ptr().add(8),
            );
            idct4x4(
                pDstY.offset((8 * iStrideL + 8) as isize),
                iStrideL,
                pScaledTCoeff.add(192),
                pNzc.as_ptr().add(10),
            );
        }
    }

    if let Some(idct4x4) = ctx.pIdctFourResAddPredFunc {
        idct4x4(pDstU, iStrideC, pScaledTCoeff.add(256), pNzc.as_ptr().add(16));
        idct4x4(pDstV, iStrideC, pScaledTCoeff.add(320), pNzc.as_ptr().add(18));
    }

    ERR_NONE
}

use crate::decoder::error_concealment::sMCRefMember;
// Phase 4a: `BaseMC` calls these directly instead of through `SMcFunc` slots.
use crate::common::mc::{McChroma_c, McLuma_c};

/// Fill an `sMCRefMember` with the reference picture selected by `iRefIdx`.
/// Matches `GetRefPic` in `rec_mb.cpp`.
unsafe fn GetRefPic(
    pMCRefMem: &mut sMCRefMember,
    pCtx: *mut SWelsDecoderContext,
    pRefs: PicRefs<'_>,
    iRefIdx: i8,
    listIdx: usize,
) -> i32 {
    if iRefIdx >= 0 {
        let pRefPic = pRefs.get(ref_id(pCtx, listIdx, iRefIdx as usize));
        if !pRefPic.is_null() {
            pMCRefMem.iSrcLineLuma = (*pRefPic).linesize(0);
            pMCRefMem.iSrcLineChroma = (*pRefPic).linesize(1);
            // The three reference-side `data_ptr` calls in the tree, and the reason
            // the `&self` form exists (W3's fact 5): MC reads the source planes and
            // writes into the current picture, so the reference never needs `&mut`.
            pMCRefMem.pSrcY = (*pRefPic).data_ptr_ref(0);
            pMCRefMem.pSrcU = (*pRefPic).data_ptr_ref(1);
            pMCRefMem.pSrcV = (*pRefPic).data_ptr_ref(2);
            if pMCRefMem.pSrcY.is_null() || pMCRefMem.pSrcU.is_null() || pMCRefMem.pSrcV.is_null() {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
            }
            return ERR_NONE;
        }
    }
    GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST)
}

/// Motion-compensate one block from the reference into the destination.
/// Matches `BaseMC` in `rec_mb.cpp` (single-thread path).
unsafe fn BaseMC(
    _pCtx: *mut SWelsDecoderContext,
    pMCRefMem: &mut sMCRefMember,
    _listIdx: usize,
    _iRefIdx: i8,
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
        (pMCRefMem.iPicWidth + PADDING_LENGTH - 19) * 4,
    );
    iFullMVy = WELS_CLIP3(
        iFullMVy,
        (-PADDING_LENGTH + 2) * 4,
        (pMCRefMem.iPicHeight + PADDING_LENGTH - 19) * 4,
    );

    let iSrcPixOffsetLuma = (iFullMVx >> 2) + (iFullMVy >> 2) * pMCRefMem.iSrcLineLuma;
    let iSrcPixOffsetChroma = (iFullMVx >> 3) + (iFullMVy >> 3) * pMCRefMem.iSrcLineChroma;

    let iBlkWidthChroma = iBlkWidth >> 1;
    let iBlkHeightChroma = iBlkHeight >> 1;

    let pSrcY = pMCRefMem.pSrcY.offset(iSrcPixOffsetLuma as isize);
    let pSrcU = pMCRefMem.pSrcU.offset(iSrcPixOffsetChroma as isize);
    let pSrcV = pMCRefMem.pSrcV.offset(iSrcPixOffsetChroma as isize);

    // Phase 4a: direct calls. `pMCFunc` held `McLuma_c`/`McChroma_c` and
    // nothing else — `init_mc_func_ignores_the_cpu_flag` pins that the
    // installer is constant in its CPU flag, and
    // `mc_table_slots_match_the_direct_calls` pins that those slots compute
    // what these symbols compute. The `if let Some` guards are gone with the
    // indirection: `WelsOpenDecoder` runs `WelsInitDecoderFuncs`
    // unconditionally before any frame is decoded, so the slots were never
    // `None` here, and a `None` would have silently left the prediction block
    // unwritten rather than reported anything.
    //
    // **Why this recovers nothing on the decode side, unlike the encoder's.**
    // Direct dispatch bought the encoder ~5% (see `perf_baseline.md` §Phase 4a)
    // and this side ~0. The difference is not the calls, it is where the block
    // dimensions become constant. The encoder's call sites name these shims
    // with literals (`16, 16` / `8, 8`), so inlining folds the shim's span
    // arithmetic and its `from_raw_parts` against them. Here the sizes arrive
    // as `iBlkWidth`/`iBlkHeight` *parameters*: they are constant at `BaseMC`'s
    // ~40 call sites but runtime inside it, and `BaseMC` is ~1300 instructions,
    // so `#[inline]` on it is declined. Measured, not assumed — `#[inline]` on
    // `BaseMC` and `#[inline(always)]` on `McChroma_c` were each built and
    // paired, and both read inside the null floor. Disassembly confirms
    // `McLuma_c` does inline here (the `mc_hor_ver*` kernels are called
    // straight from `BaseMC`) while the two chroma calls stay out of line.
    //
    // What would recover it is making the dimensions constant *at the shim*:
    // const-generic `BaseMC::<W, H>` monomorphised over the seven partition
    // shapes, or Phase 5 converting these callers so the whole path carries
    // typed blocks. That is caller conversion, which is Phase 5's job, and the
    // ledger row is downgraded to say so rather than to keep promising Phase 4.
    McLuma_c(
        pSrcY,
        pMCRefMem.iSrcLineLuma,
        pMCRefMem.pDstY,
        pMCRefMem.iDstLineLuma,
        iFullMVx as i16,
        iFullMVy as i16,
        iBlkWidth,
        iBlkHeight,
    );
    McChroma_c(
        pSrcU,
        pMCRefMem.iSrcLineChroma,
        pMCRefMem.pDstU,
        pMCRefMem.iDstLineChroma,
        iFullMVx as i16,
        iFullMVy as i16,
        iBlkWidthChroma,
        iBlkHeightChroma,
    );
    McChroma_c(
        pSrcV,
        pMCRefMem.iSrcLineChroma,
        pMCRefMem.pDstV,
        pMCRefMem.iDstLineChroma,
        iFullMVx as i16,
        iFullMVy as i16,
        iBlkWidthChroma,
        iBlkHeightChroma,
    );
}

/// Matches `WeightPrediction` in `rec_mb.cpp`.
unsafe fn WeightPrediction(
    pCurDqLayer: *mut DqLayerState,
    pMCRefMem: &mut sMCRefMember,
    listIdx: usize,
    iRefIdx: i32,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    let pwt = (*pCurDqLayer).pPredWeightTable;
    if pwt.is_null() || iRefIdx < 0 {
        return;
    }
    let pwt = &*pwt;
    // luma
    let iLog2denom = pwt.uiLumaLog2WeightDenom as i32;
    let iWoc = pwt.sPredList[listIdx].iLumaWeight[iRefIdx as usize];
    let iOoc = pwt.sPredList[listIdx].iLumaOffset[iRefIdx as usize];
    let iLineStride = pMCRefMem.iDstLineLuma;
    for i in 0..iBlkHeight {
        for j in 0..iBlkWidth {
            let p = pMCRefMem.pDstY.offset((j + i * iLineStride) as isize);
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
    let iLineStride = pMCRefMem.iDstLineChroma;
    for (plane, dst) in [pMCRefMem.pDstU, pMCRefMem.pDstV].into_iter().enumerate() {
        let iWoc = pwt.sPredList[listIdx].iChromaWeight[iRefIdx as usize][plane];
        let iOoc = pwt.sPredList[listIdx].iChromaOffset[iRefIdx as usize][plane];
        for i in 0..(iBlkHeight >> 1) {
            for j in 0..(iBlkWidth >> 1) {
                let p = dst.offset((j + i * iLineStride) as isize);
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
unsafe fn BiWeightPrediction(
    pCurDqLayer: *mut DqLayerState,
    pMCRefMem: &mut sMCRefMember,
    pTempMCRefMem: &sMCRefMember,
    iRefIdx1: i32,
    iRefIdx2: i32,
    bWeightedBipredIdcIs1: bool,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    let pwt = (*pCurDqLayer).pPredWeightTable;
    if pwt.is_null() {
        return;
    }
    let pwt = &*pwt;
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
    let mut iLineStride = pMCRefMem.iDstLineLuma;

    for i in 0..iBlkHeight {
        for j in 0..iBlkWidth {
            let iPixel = (j + i * iLineStride) as isize;
            let p = pMCRefMem.pDstY.offset(iPixel);
            let t = pTempMCRefMem.pDstY.offset(iPixel);
            let iPredTemp = ((*p as i32 * iWoc1 + *t as i32 * iWoc2 + (1 << iLog2denom))
                >> (iLog2denom + 1))
                + ((iOoc1 + iOoc2 + 1) >> 1);
            *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
        }
    }

    // UV
    let iBlkWidth = iBlkWidth >> 1;
    let iBlkHeight = iBlkHeight >> 1;
    iLog2denom = pwt.uiChromaLog2WeightDenom as i32;
    iLineStride = pMCRefMem.iDstLineChroma;

    for k in 0..2usize {
        if bWeightedBipredIdcIs1 {
            iWoc1 = pwt.sPredList[LIST_0].iChromaWeight[iRefIdx1 as usize][k];
            iOoc1 = pwt.sPredList[LIST_0].iChromaOffset[iRefIdx1 as usize][k];
            iWoc2 = pwt.sPredList[LIST_1].iChromaWeight[iRefIdx2 as usize][k];
            iOoc2 = pwt.sPredList[LIST_1].iChromaOffset[iRefIdx2 as usize][k];
        }
        let pDst = if k != 0 { pMCRefMem.pDstV } else { pMCRefMem.pDstU };
        let pTempDst = if k != 0 { pTempMCRefMem.pDstV } else { pTempMCRefMem.pDstU };

        for i in 0..iBlkHeight {
            for j in 0..iBlkWidth {
                let iPixel = (j + i * iLineStride) as isize;
                let p = pDst.offset(iPixel);
                let t = pTempDst.offset(iPixel);
                let iPredTemp = ((*p as i32 * iWoc1 + *t as i32 * iWoc2 + (1 << iLog2denom))
                    >> (iLog2denom + 1))
                    + ((iOoc1 + iOoc2 + 1) >> 1);
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }
}

/// Matches `BiPrediction` in `rec_mb.cpp`.
unsafe fn BiPrediction(
    _pCurDqLayer: *mut DqLayerState,
    pMCRefMem: &mut sMCRefMember,
    pTempMCRefMem: &sMCRefMember,
    iBlkWidth: i32,
    iBlkHeight: i32,
) {
    // luma
    let mut iLineStride = pMCRefMem.iDstLineLuma;
    for i in 0..iBlkHeight {
        for j in 0..iBlkWidth {
            let iPixel = (j + i * iLineStride) as isize;
            let p = pMCRefMem.pDstY.offset(iPixel);
            let t = pTempMCRefMem.pDstY.offset(iPixel);
            let iPredTemp = (*p as i32 + *t as i32 + 1) >> 1;
            *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
        }
    }

    // UV
    let iBlkWidth = iBlkWidth >> 1;
    let iBlkHeight = iBlkHeight >> 1;
    iLineStride = pMCRefMem.iDstLineChroma;

    for k in 0..2usize {
        let pDst = if k != 0 { pMCRefMem.pDstV } else { pMCRefMem.pDstU };
        let pTempDst = if k != 0 { pTempMCRefMem.pDstV } else { pTempMCRefMem.pDstU };
        for i in 0..iBlkHeight {
            for j in 0..iBlkWidth {
                let iPixel = (j + i * iLineStride) as isize;
                let p = pDst.offset(iPixel);
                let t = pTempDst.offset(iPixel);
                let iPredTemp = (*p as i32 + *t as i32 + 1) >> 1;
                *p = WELS_CLIP3(iPredTemp, 0, 255) as u8;
            }
        }
    }
}

/// Inter (motion-compensated) prediction of one P-slice macroblock.
/// Matches `GetInterPred` in `rec_mb.cpp`.
pub unsafe fn GetInterPred(
    pPredY: *mut u8,
    pPredCb: *mut u8,
    pPredCr: *mut u8,
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
) -> i32 {

    let iMBXY = (*pCurDqLayer).iMbXyIndex as usize;
    let iMBType = *(*pDec).pMbType.get(iMBXY);

    let iMBOffsetX = (*pCurDqLayer).iMbX << 4;
    let iMBOffsetY = (*pCurDqLayer).iMbY << 4;

    let iDstLineLuma = (*pDec).linesize(0);
    let iDstLineChroma = (*pDec).linesize(1);

    let sh = &(*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let mut pMCRefMem: sMCRefMember = std::mem::zeroed();
    pMCRefMem.iPicWidth = sh.iMbWidth << 4;
    pMCRefMem.iPicHeight = sh.iMbHeight << 4;
    pMCRefMem.pDstY = pPredY;
    pMCRefMem.pDstU = pPredCb;
    pMCRefMem.pDstV = pPredCr;
    pMCRefMem.iDstLineLuma = iDstLineLuma;
    pMCRefMem.iDstLineChroma = iDstLineChroma;

    let bWeight = (*pCurDqLayer).bUseWeightPredictionFlag;
    let mv_mb = (*pDec).pMv[0].get(iMBXY);
    let ref_mb = (*pDec).pRefIndex[0].get(iMBXY);

    match iMBType {
        MB_TYPE_SKIP | MB_TYPE_16x16 => {
            let iMVs = mv_mb[0];
            let iRefIndex = ref_mb[0];
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);
            if bWeight {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 16, 16);
            }
        }
        MB_TYPE_16x8 => {
            let iMVs = mv_mb[0];
            let iRefIndex = ref_mb[0];
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iMBOffsetX, iMBOffsetY, 16, 8, iMVs);
            if bWeight {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 16, 8);
            }

            let iMVs = mv_mb[8];
            let iRefIndex = ref_mb[8];
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            pMCRefMem.pDstY = pPredY.offset((iDstLineLuma << 3) as isize);
            pMCRefMem.pDstU = pPredCb.offset((iDstLineChroma << 2) as isize);
            pMCRefMem.pDstV = pPredCr.offset((iDstLineChroma << 2) as isize);
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iMBOffsetX, iMBOffsetY + 8, 16, 8, iMVs);
            if bWeight {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 16, 8);
            }
        }
        MB_TYPE_8x16 => {
            let iMVs = mv_mb[0];
            let iRefIndex = ref_mb[0];
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iMBOffsetX, iMBOffsetY, 8, 16, iMVs);
            if bWeight {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 8, 16);
            }

            let iMVs = mv_mb[2];
            let iRefIndex = ref_mb[2];
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            pMCRefMem.pDstY = pPredY.offset(8);
            pMCRefMem.pDstU = pPredCb.offset(4);
            pMCRefMem.pDstV = pPredCr.offset(4);
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iMBOffsetX + 8, iMBOffsetY, 8, 16, iMVs);
            if bWeight {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 8, 16);
            }
        }
        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
            // T5.I1: one window borrow at the loop head, where the C++ hoists
            // `pCurDqLayer->pSubMbType[iMBXY]` into a `uint32_t (*)[4]`. The four
            // partition reads then index a `[u32; 4]` by a `0..4` induction variable.
            let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get(iMBXY);
            for i in 0..4usize {
                let iSubMBType = pSubMbType[i];
                let iBlk8X = ((i & 1) << 3) as i32;
                let iBlk8Y = ((i >> 1) << 3) as i32;
                let iXOffset = iMBOffsetX + iBlk8X;
                let iYOffset = iMBOffsetY + iBlk8Y;

                let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);
                let iRefIndex = ref_mb[iIIdx];
                let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, LIST_0);
                if ret != ERR_NONE {
                    return ret;
                }
                let pDstY = pPredY.offset((iBlk8X + iBlk8Y * iDstLineLuma) as isize);
                let pDstU = pPredCb.offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);
                let pDstV = pPredCr.offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);
                pMCRefMem.pDstY = pDstY;
                pMCRefMem.pDstU = pDstU;
                pMCRefMem.pDstV = pDstV;

                match iSubMBType {
                    SUB_MB_TYPE_8x8 => {
                        let iMVs = mv_mb[iIIdx];
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iXOffset, iYOffset, 8, 8, iMVs);
                        if bWeight {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 8, 8);
                        }
                    }
                    SUB_MB_TYPE_8x4 => {
                        let iMVs = mv_mb[iIIdx];
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iXOffset, iYOffset, 8, 4, iMVs);
                        if bWeight {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 8, 4);
                        }

                        let iMVs = mv_mb[iIIdx + 4];
                        pMCRefMem.pDstY = pMCRefMem.pDstY.offset((iDstLineLuma << 2) as isize);
                        pMCRefMem.pDstU = pMCRefMem.pDstU.offset((iDstLineChroma << 1) as isize);
                        pMCRefMem.pDstV = pMCRefMem.pDstV.offset((iDstLineChroma << 1) as isize);
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iXOffset, iYOffset + 4, 8, 4, iMVs);
                        if bWeight {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 8, 4);
                        }
                    }
                    SUB_MB_TYPE_4x8 => {
                        let iMVs = mv_mb[iIIdx];
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iXOffset, iYOffset, 4, 8, iMVs);
                        if bWeight {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 4, 8);
                        }

                        let iMVs = mv_mb[iIIdx + 1];
                        pMCRefMem.pDstY = pMCRefMem.pDstY.offset(4);
                        pMCRefMem.pDstU = pMCRefMem.pDstU.offset(2);
                        pMCRefMem.pDstV = pMCRefMem.pDstV.offset(2);
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex, iXOffset + 4, iYOffset, 4, 8, iMVs);
                        if bWeight {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 4, 8);
                        }
                    }
                    SUB_MB_TYPE_4x4 => {
                        for j in 0..4usize {
                            let iJIdx = ((j >> 1) << 2) + (j & 1);
                            let iBlk4X = ((j & 1) << 2) as i32;
                            let iBlk4Y = ((j >> 1) << 2) as i32;

                            let iUVLineStride = (iBlk4X >> 1) + (iBlk4Y >> 1) * iDstLineChroma;
                            pMCRefMem.pDstY = pDstY.offset((iBlk4X + iBlk4Y * iDstLineLuma) as isize);
                            pMCRefMem.pDstU = pDstU.offset(iUVLineStride as isize);
                            pMCRefMem.pDstV = pDstV.offset(iUVLineStride as isize);

                            let iMVs = mv_mb[iIIdx + iJIdx];
                            BaseMC(
                                pCtx,
                                &mut pMCRefMem,
                                LIST_0,
                                iRefIndex,
                                iXOffset + iBlk4X,
                                iYOffset + iBlk4Y,
                                4,
                                4,
                                iMVs,
                            );
                            if bWeight {
                                WeightPrediction(pCurDqLayer, &mut pMCRefMem, LIST_0, iRefIndex as i32, 4, 4);
                            }
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
/// `pTempPredYCbCr` receives the LIST_1 prediction so the two hypotheses can be
/// blended in place by [`BiPrediction`] / [`BiWeightPrediction`].
pub unsafe fn GetInterBPred(
    pPredYCbCr: [*mut u8; 3],
    pTempPredYCbCr: [*mut u8; 3],
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
) -> i32 {

    let iMBXY = (*pCurDqLayer).iMbXyIndex as usize;
    let mut iMVs = [0i16; 2];

    let iMBType = *(*pDec).pMbType.get(iMBXY);

    let iMBOffsetX = (*pCurDqLayer).iMbX << 4;
    let iMBOffsetY = (*pCurDqLayer).iMbY << 4;

    let iDstLineLuma = (*pDec).linesize(0);
    let iDstLineChroma = (*pDec).linesize(1);

    let mut pMCRefMem: sMCRefMember = std::mem::zeroed();
    let sh = &(*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    pMCRefMem.iPicWidth = sh.iMbWidth << 4;
    pMCRefMem.iPicHeight = sh.iMbHeight << 4;

    pMCRefMem.pDstY = pPredYCbCr[0];
    pMCRefMem.pDstU = pPredYCbCr[1];
    pMCRefMem.pDstV = pPredYCbCr[2];

    pMCRefMem.iDstLineLuma = iDstLineLuma;
    pMCRefMem.iDstLineChroma = iDstLineChroma;

    let mut pTempMCRefMem = pMCRefMem;
    pTempMCRefMem.pDstY = pTempPredYCbCr[0];
    pTempMCRefMem.pDstU = pTempPredYCbCr[1];
    pTempMCRefMem.pDstV = pTempPredYCbCr[2];

    let mut iRefIndex0: i8 = 0;
    let mut iRefIndex1: i8 = 0;
    let mut iRefIndex: i8 = 0;

    let pPpsB = pps_of(pCtx, (*pCurDqLayer).sLayerInfo.pps_id);
    let bWeightedBipredIdcIs1 = !pPpsB.is_null() && (*pPpsB).uiWeightedBipredIdc == 1;
    let bUseWeightedBiPredIdc = (*pCurDqLayer).bUseWeightedBiPredIdc;

    let pMv = |list: usize, idx: usize| -> [i16; 2] { (*(*pDec).pMv[list].get(iMBXY))[idx] };
    let pRef = |list: usize, idx: usize| -> i8 { (*(*pDec).pRefIndex[list].get(iMBXY))[idx] };

    if IS_INTER_16x16(iMBType) {
        if IS_TYPE_L0(iMBType) && IS_TYPE_L1(iMBType) {
            iMVs = pMv(LIST_0, 0);
            iRefIndex0 = pRef(LIST_0, 0);
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex0, LIST_0);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);

            iMVs = pMv(LIST_1, 0);
            iRefIndex1 = pRef(LIST_1, 0);
            let ret = GetRefPic(&mut pTempMCRefMem, pCtx, pRefs, iRefIndex1, LIST_1);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);
            if bUseWeightedBiPredIdc {
                BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 16, 16);
            } else {
                BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 16, 16);
            }
        } else {
            let listIdx = if (iMBType & MB_TYPE_P0L0) != 0 { LIST_0 } else { LIST_1 };
            iMVs = pMv(listIdx, 0);
            iRefIndex = pRef(listIdx, 0);
            let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, listIdx);
            if ret != ERR_NONE {
                return ret;
            }
            BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iMBOffsetX, iMBOffsetY, 16, 16, iMVs);
            if bWeightedBipredIdcIs1 {
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, listIdx, iRefIndex as i32, 16, 16);
            }
        }
    } else if IS_INTER_16x8(iMBType) {
        for i in 0..2usize {
            let iPartIdx = i << 3;
            let mut listCount = 0u32;
            let mut lastListIdx = LIST_0;
            for listIdx in LIST_0..LIST_A {
                if IS_DIR(iMBType, i, listIdx) {
                    lastListIdx = listIdx;
                    iMVs = pMv(listIdx, iPartIdx);
                    iRefIndex = pRef(listIdx, iPartIdx);
                    let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, listIdx);
                    if ret != ERR_NONE {
                        return ret;
                    }
                    if i != 0 {
                        pMCRefMem.pDstY = pMCRefMem.pDstY.offset((iDstLineLuma << 3) as isize);
                        pMCRefMem.pDstU = pMCRefMem.pDstU.offset((iDstLineChroma << 2) as isize);
                        pMCRefMem.pDstV = pMCRefMem.pDstV.offset((iDstLineChroma << 2) as isize);
                    }
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iMBOffsetX, iMBOffsetY + iPartIdx as i32, 16, 8, iMVs);
                    listCount += 1;
                    if listCount == 2 {
                        iMVs = pMv(LIST_1, iPartIdx);
                        iRefIndex1 = pRef(LIST_1, iPartIdx);
                        let ret = GetRefPic(&mut pTempMCRefMem, pCtx, pRefs, iRefIndex1, LIST_1);
                        if ret != ERR_NONE {
                            return ret;
                        }
                        if i != 0 {
                            pTempMCRefMem.pDstY = pTempMCRefMem.pDstY.offset((iDstLineLuma << 3) as isize);
                            pTempMCRefMem.pDstU = pTempMCRefMem.pDstU.offset((iDstLineChroma << 2) as isize);
                            pTempMCRefMem.pDstV = pTempMCRefMem.pDstV.offset((iDstLineChroma << 2) as isize);
                        }
                        BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iMBOffsetX, iMBOffsetY + iPartIdx as i32, 16, 8, iMVs);
                        if bUseWeightedBiPredIdc {
                            iRefIndex0 = pRef(LIST_0, iPartIdx);
                            iRefIndex1 = pRef(LIST_1, iPartIdx);
                            BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 16, 8);
                        } else {
                            BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 16, 8);
                        }
                    }
                }
            }
            if listCount == 1 && bWeightedBipredIdcIs1 {
                iRefIndex = pRef(lastListIdx, iPartIdx);
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, lastListIdx, iRefIndex as i32, 16, 8);
            }
        }
    } else if IS_INTER_8x16(iMBType) {
        for i in 0..2usize {
            let mut listCount = 0u32;
            let mut lastListIdx = LIST_0;
            for listIdx in LIST_0..LIST_A {
                if IS_DIR(iMBType, i, listIdx) {
                    lastListIdx = listIdx;
                    iMVs = pMv(listIdx, i << 1);
                    iRefIndex = pRef(listIdx, i << 1);
                    let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, listIdx);
                    if ret != ERR_NONE {
                        return ret;
                    }
                    if i != 0 {
                        pMCRefMem.pDstY = pMCRefMem.pDstY.offset(8);
                        pMCRefMem.pDstU = pMCRefMem.pDstU.offset(4);
                        pMCRefMem.pDstV = pMCRefMem.pDstV.offset(4);
                    }
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iMBOffsetX + if i != 0 { 8 } else { 0 }, iMBOffsetY, 8, 16, iMVs);
                    listCount += 1;
                    if listCount == 2 {
                        iMVs = pMv(LIST_1, i << 1);
                        iRefIndex1 = pRef(LIST_1, i << 1);
                        let ret = GetRefPic(&mut pTempMCRefMem, pCtx, pRefs, iRefIndex1, LIST_1);
                        if ret != ERR_NONE {
                            return ret;
                        }
                        if i != 0 {
                            pTempMCRefMem.pDstY = pTempMCRefMem.pDstY.offset(8);
                            pTempMCRefMem.pDstU = pTempMCRefMem.pDstU.offset(4);
                            pTempMCRefMem.pDstV = pTempMCRefMem.pDstV.offset(4);
                        }
                        BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iMBOffsetX + if i != 0 { 8 } else { 0 }, iMBOffsetY, 8, 16, iMVs);
                        if bUseWeightedBiPredIdc {
                            iRefIndex0 = pRef(LIST_0, i << 1);
                            iRefIndex1 = pRef(LIST_1, i << 1);
                            BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 8, 16);
                        } else {
                            BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 8, 16);
                        }
                    }
                }
            }
            if listCount == 1 && bWeightedBipredIdcIs1 {
                iRefIndex = pRef(lastListIdx, i << 1);
                WeightPrediction(pCurDqLayer, &mut pMCRefMem, lastListIdx, iRefIndex as i32, 8, 16);
            }
        }
    } else if IS_Inter_8x8(iMBType) {
        // T5.I1: hoisted as in `GetInterPred`.
        let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get(iMBXY);
        for i in 0..4usize {
            let iSubMBType = pSubMbType[i];
            let iBlk8X = ((i & 1) << 3) as i32;
            let iBlk8Y = ((i >> 1) << 3) as i32;
            let iXOffset = iMBOffsetX + iBlk8X;
            let iYOffset = iMBOffsetY + iBlk8Y;

            let iIIdx = ((i >> 1) << 3) + ((i & 1) << 1);

            let pDstY = pPredYCbCr[0].offset((iBlk8X + iBlk8Y * iDstLineLuma) as isize);
            let pDstU = pPredYCbCr[1].offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);
            let pDstV = pPredYCbCr[2].offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);
            pMCRefMem.pDstY = pDstY;
            pMCRefMem.pDstU = pDstU;
            pMCRefMem.pDstV = pDstV;

            pTempMCRefMem = pMCRefMem;
            let pDstY2 = pTempPredYCbCr[0].offset((iBlk8X + iBlk8Y * iDstLineLuma) as isize);
            let pDstU2 = pTempPredYCbCr[1].offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);
            let pDstV2 = pTempPredYCbCr[2].offset(((iBlk8X >> 1) + (iBlk8Y >> 1) * iDstLineChroma) as isize);

            pTempMCRefMem.pDstY = pDstY2;
            pTempMCRefMem.pDstU = pDstU2;
            pTempMCRefMem.pDstV = pDstV2;

            if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                iRefIndex0 = pRef(LIST_0, iIIdx);
                let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex0, LIST_0);
                if ret != ERR_NONE {
                    return ret;
                }
                iRefIndex1 = pRef(LIST_1, iIIdx);
                let ret = GetRefPic(&mut pTempMCRefMem, pCtx, pRefs, iRefIndex1, LIST_1);
                if ret != ERR_NONE {
                    return ret;
                }
            } else {
                let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                iRefIndex = pRef(listIdx, iIIdx);
                let ret = GetRefPic(&mut pMCRefMem, pCtx, pRefs, iRefIndex, listIdx);
                if ret != ERR_NONE {
                    return ret;
                }
            }

            if IS_SUB_8x8(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    iMVs = pMv(LIST_0, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset, iYOffset, 8, 8, iMVs);

                    iMVs = pMv(LIST_1, iIIdx);
                    BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset, iYOffset, 8, 8, iMVs);

                    if bUseWeightedBiPredIdc {
                        BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 8, 8);
                    } else {
                        BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 8, 8);
                    }
                } else {
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset, iYOffset, 8, 8, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pCurDqLayer, &mut pMCRefMem, listIdx, iRefIndex as i32, 8, 8);
                    }
                }
            } else if IS_SUB_8x4(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    // B_Bi_8x4
                    iMVs = pMv(LIST_0, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset, iYOffset, 8, 4, iMVs);
                    iMVs = pMv(LIST_1, iIIdx);
                    BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset, iYOffset, 8, 4, iMVs);

                    if bUseWeightedBiPredIdc {
                        BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 8, 4);
                    } else {
                        BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 8, 4);
                    }

                    pMCRefMem.pDstY = pMCRefMem.pDstY.offset((iDstLineLuma << 2) as isize);
                    pMCRefMem.pDstU = pMCRefMem.pDstU.offset((iDstLineChroma << 1) as isize);
                    pMCRefMem.pDstV = pMCRefMem.pDstV.offset((iDstLineChroma << 1) as isize);
                    iMVs = pMv(LIST_0, iIIdx + 4);
                    BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset, iYOffset + 4, 8, 4, iMVs);

                    pTempMCRefMem.pDstY = pTempMCRefMem.pDstY.offset((iDstLineLuma << 2) as isize);
                    pTempMCRefMem.pDstU = pTempMCRefMem.pDstU.offset((iDstLineChroma << 1) as isize);
                    pTempMCRefMem.pDstV = pTempMCRefMem.pDstV.offset((iDstLineChroma << 1) as isize);
                    iMVs = pMv(LIST_1, iIIdx + 4);
                    BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset, iYOffset + 4, 8, 4, iMVs);

                    if bUseWeightedBiPredIdc {
                        BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 8, 4);
                    } else {
                        BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 8, 4);
                    }
                } else {
                    // B_L0_8x4 B_L1_8x4
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset, iYOffset, 8, 4, iMVs);
                    pMCRefMem.pDstY = pMCRefMem.pDstY.offset((iDstLineLuma << 2) as isize);
                    pMCRefMem.pDstU = pMCRefMem.pDstU.offset((iDstLineChroma << 1) as isize);
                    pMCRefMem.pDstV = pMCRefMem.pDstV.offset((iDstLineChroma << 1) as isize);
                    iMVs = pMv(listIdx, iIIdx + 4);
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset, iYOffset + 4, 8, 4, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pCurDqLayer, &mut pMCRefMem, listIdx, iRefIndex as i32, 8, 4);
                    }
                }
            } else if IS_SUB_4x8(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    // B_Bi_4x8
                    iMVs = pMv(LIST_0, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset, iYOffset, 4, 8, iMVs);
                    iMVs = pMv(LIST_1, iIIdx);
                    BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset, iYOffset, 4, 8, iMVs);

                    if bUseWeightedBiPredIdc {
                        BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 4, 8);
                    } else {
                        BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 4, 8);
                    }

                    pMCRefMem.pDstY = pMCRefMem.pDstY.offset(4);
                    pMCRefMem.pDstU = pMCRefMem.pDstU.offset(2);
                    pMCRefMem.pDstV = pMCRefMem.pDstV.offset(2);
                    iMVs = pMv(LIST_0, iIIdx + 1);
                    BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset + 4, iYOffset, 4, 8, iMVs);

                    pTempMCRefMem.pDstY = pTempMCRefMem.pDstY.offset(4);
                    pTempMCRefMem.pDstU = pTempMCRefMem.pDstU.offset(2);
                    pTempMCRefMem.pDstV = pTempMCRefMem.pDstV.offset(2);
                    iMVs = pMv(LIST_1, iIIdx + 1);
                    BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset + 4, iYOffset, 4, 8, iMVs);

                    if bUseWeightedBiPredIdc {
                        BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 4, 8);
                    } else {
                        BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 4, 8);
                    }
                } else {
                    // B_L0_4x8 B_L1_4x8
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iMVs = pMv(listIdx, iIIdx);
                    iRefIndex = pRef(listIdx, iIIdx);
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset, iYOffset, 4, 8, iMVs);
                    pMCRefMem.pDstY = pMCRefMem.pDstY.offset(4);
                    pMCRefMem.pDstU = pMCRefMem.pDstU.offset(2);
                    pMCRefMem.pDstV = pMCRefMem.pDstV.offset(2);
                    iMVs = pMv(listIdx, iIIdx + 1);
                    BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset + 4, iYOffset, 4, 8, iMVs);
                    if bWeightedBipredIdcIs1 {
                        WeightPrediction(pCurDqLayer, &mut pMCRefMem, listIdx, iRefIndex as i32, 4, 8);
                    }
                }
            } else if IS_SUB_4x4(iSubMBType) {
                if IS_TYPE_L0(iSubMBType) && IS_TYPE_L1(iSubMBType) {
                    for j in 0..4usize {
                        let iJIdx = ((j >> 1) << 2) + (j & 1);

                        let iBlk4X = ((j & 1) << 2) as i32;
                        let iBlk4Y = ((j >> 1) << 2) as i32;

                        let iUVLineStride = (iBlk4X >> 1) + (iBlk4Y >> 1) * iDstLineChroma;
                        pMCRefMem.pDstY = pDstY.offset((iBlk4X + iBlk4Y * iDstLineLuma) as isize);
                        pMCRefMem.pDstU = pDstU.offset(iUVLineStride as isize);
                        pMCRefMem.pDstV = pDstV.offset(iUVLineStride as isize);

                        iMVs = pMv(LIST_0, iIIdx + iJIdx);
                        BaseMC(pCtx, &mut pMCRefMem, LIST_0, iRefIndex0, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);

                        // NOTE: C indexes the LIST_1 destination with iBlk8X/iBlk8Y here,
                        // not iBlk4X/iBlk4Y, so the 8x8 offset is applied twice (pDstY2
                        // already carries it). Kept verbatim - see rec_mb.cpp:1014.
                        pTempMCRefMem.pDstY = pDstY2.offset((iBlk8X + iBlk8Y * iDstLineLuma) as isize);
                        pTempMCRefMem.pDstU = pDstU2.offset(iUVLineStride as isize);
                        pTempMCRefMem.pDstV = pDstV2.offset(iUVLineStride as isize);

                        iMVs = pMv(LIST_1, iIIdx + iJIdx);
                        BaseMC(pCtx, &mut pTempMCRefMem, LIST_1, iRefIndex1, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);

                        if bUseWeightedBiPredIdc {
                            BiWeightPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, iRefIndex0 as i32, iRefIndex1 as i32, bWeightedBipredIdcIs1, 4, 4);
                        } else {
                            BiPrediction(pCurDqLayer, &mut pMCRefMem, &pTempMCRefMem, 4, 4);
                        }
                    }
                } else {
                    let listIdx = if IS_TYPE_L0(iSubMBType) { LIST_0 } else { LIST_1 };
                    iRefIndex = pRef(listIdx, iIIdx);
                    for j in 0..4usize {
                        let iJIdx = ((j >> 1) << 2) + (j & 1);

                        let iBlk4X = ((j & 1) << 2) as i32;
                        let iBlk4Y = ((j >> 1) << 2) as i32;

                        let iUVLineStride = (iBlk4X >> 1) + (iBlk4Y >> 1) * iDstLineChroma;
                        pMCRefMem.pDstY = pDstY.offset((iBlk4X + iBlk4Y * iDstLineLuma) as isize);
                        pMCRefMem.pDstU = pDstU.offset(iUVLineStride as isize);
                        pMCRefMem.pDstV = pDstV.offset(iUVLineStride as isize);

                        iMVs = pMv(listIdx, iIIdx + iJIdx);
                        BaseMC(pCtx, &mut pMCRefMem, listIdx, iRefIndex, iXOffset + iBlk4X, iYOffset + iBlk4Y, 4, 4, iMVs);
                        if bWeightedBipredIdcIs1 {
                            WeightPrediction(pCurDqLayer, &mut pMCRefMem, listIdx, iRefIndex as i32, 4, 4);
                        }
                    }
                }
            }
        }
    }
    ERR_NONE
}

/// `pCtx->pTempDec` lazily allocated on first B macroblock, then the three
/// plane pointers for this macroblock. Matches the `else` branch shared by
/// `WelsMbInterConstruction` / `WelsMbInterPrediction` in `decode_slice.cpp`.
unsafe fn GetTempPredPlanes(
    pCtx: *mut SWelsDecoderContext,
    iMbX: i32,
    iMbY: i32,
    iLumaStride: i32,
    iChromaStride: i32,
) -> Option<[*mut u8; 3]> {
    if (*pCtx).pTempDec.is_none() {
        if active_sps(pCtx).is_null() {
            return None;
        }
        // T5.P″1: `alloc_picture` hands back the owner, and the field keeps it. The
        // lazy arm's two null tests are the same two states — "not allocated yet" and
        // "the allocation failed" — with `Option` spelling them.
        (*pCtx).pTempDec = crate::decoder::pic_queue::alloc_picture(
            pCtx,
            ((*active_sps(pCtx)).iMbWidth << 4) as i32,
            ((*active_sps(pCtx)).iMbHeight << 4) as i32,
        );
    }
    // The borrow is on the field (S29) and the three plane pointers derive from the
    // picture's own plane allocations (S28, `data_ptr` from the allocation root), so
    // nothing the caller does through `pCtx` can pop them — the only expression that
    // re-derives this picture is this function, one macroblock later.
    let pTempDec = (*pCtx).pTempDec.as_deref_mut()?;
    Some([
        pTempDec.data_ptr(0).offset(((iMbY * iLumaStride + iMbX) << 4) as isize),
        pTempDec.data_ptr(1).offset(((iMbY * iChromaStride + iMbX) << 3) as isize),
        pTempDec.data_ptr(2).offset(((iMbY * iChromaStride + iMbX) << 3) as isize),
    ])
}

pub unsafe fn WelsMbInterConstruction(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pCurDqLayer: *mut DqLayerState,
) -> i32 {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbX = (*dq).iMbX;
    let iMbY = (*dq).iMbY;

    if pDec.is_null() {
        return ERR_NONE;
    }
    let iLumaStride = (*pDec).linesize(0);
    let iChromaStride = (*pDec).linesize(1);

    let pDstY = (*pDec).data_ptr(0).offset(((iMbY * iLumaStride + iMbX) << 4) as isize);
    let pDstCb = (*pDec).data_ptr(1).offset(((iMbY * iChromaStride + iMbX) << 3) as isize);
    let pDstCr = (*pDec).data_ptr(2).offset(((iMbY * iChromaStride + iMbX) << 3) as isize);

    if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
        let ret = GetInterPred(pDstY, pDstCb, pDstCr, pCtx, dq, pDec, pRefs);
        if ret != ERR_NONE {
            return ret;
        }
    } else {
        let Some(pTempDstYCbCr) =
            GetTempPredPlanes(pCtx, iMbX, iMbY, iLumaStride, iChromaStride)
        else {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_MB_RECON_FAIL);
        };
        let ret = GetInterBPred([pDstY, pDstCb, pDstCr], pTempDstYCbCr, pCtx, dq, pDec, pRefs);
        if ret != ERR_NONE {
            return ret;
        }
    }

    WelsMbInterSampleConstruction(pCtx, pCurDqLayer, pDstY, pDstCb, pDstCr, iLumaStride, iChromaStride);

    // `decode_slice.cpp:240`, the only reader of the former `sBlockFunc` table.
    // The C++ guards this with `GetThreadCount (pCtx) <= 1`; the port's
    // `GetThreadCount` is hard-coded 0 (decoder threading was never ported, T5c),
    // so the guard is always true and is not transcribed.
    crate::common::deblocking_common::WelsNonZeroCount_c(
        (*dq).grid.nzc.get_mut((*dq).iMbXyIndex as usize).as_mut_ptr(),
    );

    ERR_NONE
}

/// MC-only reconstruction for inter macroblocks with cbp == 0 (incl. skip).
/// Matches `WelsMbInterPrediction` in `decode_slice.cpp`.
pub unsafe fn WelsMbInterPrediction(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pCurDqLayer: *mut DqLayerState,
) -> i32 {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    if pDec.is_null() {
        return ERR_NONE;
    }
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbX = (*dq).iMbX;
    let iMbY = (*dq).iMbY;
    let iLumaStride = (*pDec).linesize(0);
    let iChromaStride = (*pDec).linesize(1);

    let pDstY = (*pDec).data_ptr(0).offset(((iMbY * iLumaStride + iMbX) << 4) as isize);
    let pDstCb = (*pDec).data_ptr(1).offset(((iMbY * iChromaStride + iMbX) << 3) as isize);
    let pDstCr = (*pDec).data_ptr(2).offset(((iMbY * iChromaStride + iMbX) << 3) as isize);

    if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
        let ret = GetInterPred(pDstY, pDstCb, pDstCr, pCtx, dq, pDec, pRefs);
        if ret != ERR_NONE {
            return ret;
        }
    } else {
        let Some(pTempDstYCbCr) =
            GetTempPredPlanes(pCtx, iMbX, iMbY, iLumaStride, iChromaStride)
        else {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_MB_RECON_FAIL);
        };
        let ret = GetInterBPred([pDstY, pDstCb, pDstCr], pTempDstYCbCr, pCtx, dq, pDec, pRefs);
        if ret != ERR_NONE {
            return ret;
        }
    }
    ERR_NONE
}

pub unsafe fn WelsFillRecNeededMbInfo(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    bOutput: bool,
    pCurDqLayer: *mut DqLayerState,
) {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return;
    }
    let pCurPic = pDec;
    if pCurPic.is_null() {
        return;
    }
    let iLumaStride = (*pCurPic).linesize(0);
    let iChromaStride = (*pCurPic).linesize(1);
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;

    (*pCurDqLayer).iLumaStride = iLumaStride;
    (*pCurDqLayer).iChromaStride = iChromaStride;

    if bOutput && !(*pCurPic).data_ptr(0).is_null() {
        (*pCurDqLayer).pPred[0] = (*pCurPic).data_ptr(0).add(((iMbY * iLumaStride + iMbX) << 4) as usize);
        (*pCurDqLayer).pPred[1] = (*pCurPic).data_ptr(1).add(((iMbY * iChromaStride + iMbX) << 3) as usize);
        (*pCurDqLayer).pPred[2] = (*pCurPic).data_ptr(2).add(((iMbY * iChromaStride + iMbX) << 3) as usize);
    }
}

pub unsafe fn RecChroma(
    iMBXY: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    let iChromaStride = (*pDqLayer).iChromaStride;
    let pIdctFourResAddPredFunc = (*pCtx).pIdctFourResAddPredFunc;

    let uiCbpC = ((*(*pDqLayer).grid.cbp.get(iMBXY as usize)) as u8) >> 4;

    if uiCbpC == 1 || uiCbpC == 2 {
        if let Some(func) = pIdctFourResAddPredFunc {
            for i in 0..2 {
                let pRS = pScoeffLevel.add(256 + (i << 6));
                let pPred = (*pDqLayer).pPred[i + 1];
                let pNzc = (*pDqLayer).grid.nzc.get(iMBXY as usize).as_ptr().add(16 + 2 * i) as *const i8;
                func(pPred, iChromaStride, pRS, pNzc);
            }
        }
    }
    ERR_NONE
}

pub unsafe fn RecI4x4Luma(
    iMBXY: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    let pPred = (*pDqLayer).pPred[0];
    let iLumaStride = (*pDqLayer).iLumaStride;
    let pBlockOffset = (*pCtx).iDecBlockOffsetArray.as_ptr();
    let pIntra4x4PredMode =
        mb_grid_ptr(&mut (*pDqLayer).grid.intra4x4_final_mode, iMBXY as usize) as *mut i8;
    let pRS = pScoeffLevel;
    let pIdctResAddPredFunc = (*pCtx).pIdctResAddPredFunc;

    for i in 0..16 {
        let pPredI4x4 = pPred.add(*pBlockOffset.add(i) as usize);
        let uiMode = *pIntra4x4PredMode.add(g_kuiMbCountScan4Idx[i] as usize) as usize;

        if let Some(func) = (*pCtx).pGetI4x4LumaPredFunc[uiMode] {
            func(pPredI4x4, iLumaStride);
        }

        let nzc_idx = g_kuiMbCountScan4Idx[i] as usize;
        if *((*pDqLayer).grid.nzc.get(iMBXY as usize).as_ptr().add(nzc_idx)) != 0 {
            if let Some(idct_func) = pIdctResAddPredFunc {
                let pRSI4x4 = pRS.add(i << 4);
                idct_func(pPredI4x4, iLumaStride, pRSI4x4);
            }
        }
    }
    ERR_NONE
}

pub unsafe fn RecI4x4Chroma(
    iMBXY: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    let iChromaStride = (*pDqLayer).iChromaStride;
    let iChromaPredMode = *(*pDqLayer).grid.chroma_pred_mode.get(iMBXY as usize) as usize;

    if let Some(func) = (*pCtx).pGetIChromaPredFunc[iChromaPredMode] {
        let pPred1 = (*pDqLayer).pPred[1];
        func(pPred1, iChromaStride);
        let pPred2 = (*pDqLayer).pPred[2];
        func(pPred2, iChromaStride);
    }

    RecChroma(iMBXY, pCtx, pScoeffLevel, pDqLayer)
}

pub unsafe fn RecI4x4Mb(
    iMBXY: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    RecI4x4Luma(iMBXY, pCtx, pScoeffLevel, pDqLayer);
    RecI4x4Chroma(iMBXY, pCtx, pScoeffLevel, pDqLayer);
    ERR_NONE
}

pub unsafe fn RecI8x8Luma(
    iMbXy: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    let pPred = (*pDqLayer).pPred[0];
    let iLumaStride = (*pDqLayer).iLumaStride;
    let pBlockOffset = (*pCtx).iDecBlockOffsetArray.as_ptr();
    let pIntra8x8PredMode =
        mb_grid_ptr(&mut (*pDqLayer).grid.intra4x4_final_mode, iMbXy as usize) as *mut i8;
    let pRS = pScoeffLevel;
    let pIdctResAddPredFunc = (*pCtx).pIdctResAddPredFunc8x8;

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
        let pPredI8x8 = pPred.add(*pBlockOffset.add(i << 2) as usize);
        let uiMode = *pIntra8x8PredMode.add(g_kuiMbCountScan4Idx[i << 2] as usize) as usize;

        if let Some(func) = (*pCtx).pGetI8x8LumaPredFunc[uiMode] {
            func(pPredI8x8, iLumaStride, bTLAvail[i], bTRAvail[i]);
        }

        let iIndex = g_kuiMbCountScan4Idx[i << 2] as usize;
        let nzc_ptr = (*pDqLayer).grid.nzc.get(iMbXy as usize).as_ptr();
        if *nzc_ptr.add(iIndex) != 0
            || *nzc_ptr.add(iIndex + 1) != 0
            || *nzc_ptr.add(iIndex + 4) != 0
            || *nzc_ptr.add(iIndex + 5) != 0
        {
            if let Some(idct_func) = pIdctResAddPredFunc {
                let pRSI8x8 = pRS.add(i << 6);
                idct_func(pPredI8x8, iLumaStride, pRSI8x8);
            }
        }
    }
    ERR_NONE
}

pub unsafe fn RecI8x8Mb(
    iMbXy: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    RecI8x8Luma(iMbXy, pCtx, pScoeffLevel, pDqLayer);
    RecI4x4Chroma(iMbXy, pCtx, pScoeffLevel, pDqLayer);
    ERR_NONE
}

pub unsafe fn RecI16x16Mb(
    iMBXY: i32,
    pCtx: *mut SWelsDecoderContext,
    pScoeffLevel: *mut i16,
    pDqLayer: *mut DqLayerState,
) -> i32 {
    let iI16x16PredMode = (*pDqLayer).grid.intra_pred_mode.get(iMBXY as usize)[7] as usize;
    let iChromaPredMode = *(*pDqLayer).grid.chroma_pred_mode.get(iMBXY as usize) as usize;
    let iUVStride = (*pDqLayer).iChromaStride;
    let iYStride = (*pDqLayer).iLumaStride;
    let pRS = pScoeffLevel;
    let pPredY = (*pDqLayer).pPred[0];
    let pIdctFourResAddPredFunc = (*pCtx).pIdctFourResAddPredFunc;

    if let Some(func) = (*pCtx).pGetI16x16LumaPredFunc[iI16x16PredMode] {
        func(pPredY, iYStride);
    }

    if let Some(idct_func) = pIdctFourResAddPredFunc {
        let pNzc = (*pDqLayer).grid.nzc.get(iMBXY as usize).as_ptr() as *const i8;
        idct_func(pPredY.add(0 * iYStride as usize + 0), iYStride, pRS.add(0 * 64), pNzc.add(0));
        idct_func(pPredY.add(0 * iYStride as usize + 8), iYStride, pRS.add(1 * 64), pNzc.add(2));
        idct_func(pPredY.add(8 * iYStride as usize + 0), iYStride, pRS.add(2 * 64), pNzc.add(8));
        idct_func(pPredY.add(8 * iYStride as usize + 8), iYStride, pRS.add(3 * 64), pNzc.add(10));
    }

    if let Some(chroma_func) = (*pCtx).pGetIChromaPredFunc[iChromaPredMode] {
        chroma_func((*pDqLayer).pPred[1], iUVStride);
        chroma_func((*pDqLayer).pPred[2], iUVStride);
    }

    RecChroma(iMBXY, pCtx, pScoeffLevel, pDqLayer);
    ERR_NONE
}

pub unsafe fn WelsMbIntraPredictionConstruction(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pCurDqLayer: *mut DqLayerState,
    bOutput: bool,
) -> i32 {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let iMbXy = (*pCurDqLayer).iMbXyIndex;

    WelsFillRecNeededMbInfo(pCtx, pDec, bOutput, pCurDqLayer);

    if pDec.is_null() || (*pDec).pMbType.as_slice().is_empty() {
        return ERR_NONE;
    }
    let mb_type = *(*pDec).pMbType.get(iMbXy as usize);
    let pScoeffLevel = (*pCurDqLayer).grid.scaled_tcoeff.get_mut(iMbXy as usize).as_mut_ptr();

    if IS_INTRA16x16(mb_type) {
        RecI16x16Mb(iMbXy, pCtx, pScoeffLevel, pCurDqLayer);
    } else if IS_INTRA8x8(mb_type) {
        RecI8x8Mb(iMbXy, pCtx, pScoeffLevel, pCurDqLayer);
    } else if IS_INTRA4x4(mb_type) {
        RecI4x4Mb(iMbXy, pCtx, pScoeffLevel, pCurDqLayer);
    }
    ERR_NONE
}

pub unsafe fn WelsTargetMbConstruction(pCtx: *mut SWelsDecoderContext, pCurDqLayer: *mut DqLayerState, pDec: PPicture, pRefs: PicRefs<'_>) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;

    if pDec.is_null() || (*pDec).pMbType.as_slice().is_empty() {
        return ERR_NONE;
    }
    let mb_type = *(*pDec).pMbType.get(iMbXy);

    if mb_type == MB_TYPE_INTRA_PCM {
        ERR_NONE
    } else if IS_INTRA(mb_type) {
        WelsMbIntraPredictionConstruction(pCtx, pDec, pCurDqLayer, true);
        ERR_NONE
    } else if IS_INTER(mb_type) {
        // T5.H12: a `pCbp.is_null()` guard returning `ERR_INFO_MB_RECON_FAIL` sat
        // here. `WelsTargetMbConstruction` (`decode_slice.cpp:334-355`) has no such
        // test — the port invented it, and it could only fire if the array's
        // allocation had failed, which the C++ answers by dereferencing null. The
        // grid makes it unrepresentable: `cbp` is a `Vec` sized with the layer.
        let cbp = *(*dq).grid.cbp.get(iMbXy);
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

pub unsafe fn WelsTargetSliceConstruction(pCtx: *mut SWelsDecoderContext, pCurDqLayer: *mut DqLayerState) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq: *mut DqLayerState = pCurDqLayer;
    let (pDec, pRefs) = cur_and_refs(pCtx);
    let pCurSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pCurSlice).sSliceHeaderExt.sSliceHeader);

    if (*pSliceHeader).sps_ref.is_none() {
        return ERR_NONE;
    }
    let iTotalMbTargetLayer = (*(sps_of(pCtx, (*pSliceHeader).sps_ref))).uiTotalMbCount as i32;


    let iCurLayerWidth = (*dq).iMbWidth << 4;
    let iCurLayerHeight = (*dq).iMbHeight << 4;

    let mut iNextMbXyIndex = (*pSliceHeader).iFirstMbInSlice;
    let iTotalNumMb = (*pCurSlice).iTotalMbInCurSlice;
    let mut iCountNumMb = 0;

    if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag && iCurLayerWidth != (*pCtx).iCurSeqIntervalMaxPicWidth {
        return ERR_INFO_WIDTH_MISMATCH;
    }

    if (*dq).iMbWidth > 0 {
        (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
        (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
    }
    (*dq).iMbXyIndex = iNextMbXyIndex;

    if iNextMbXyIndex == 0 && !pDec.is_null() {
        if !active_sps(pCtx).is_null() {
            (*pDec).iSpsId = (*active_sps(pCtx)).iSpsId;
        }
        if !active_pps(pCtx).is_null() {
            (*pDec).iPpsId = (*active_pps(pCtx)).iPpsId;
        }
        (*pDec).uiQualityId = (*dq).sLayerInfo.sNalHeaderExt.uiQualityId;
    }

    loop {
        if iCountNumMb >= iTotalNumMb {
            break;
        }

        let bParseOnly = if !(*pCtx).pParam.is_null() { (*(*pCtx).pParam).bParseOnly } else { false };
        if !bParseOnly {
            let ret = WelsTargetMbConstruction(pCtx, dq, pDec, pRefs);
            if ret != ERR_NONE {
                return ERR_INFO_MB_RECON_FAIL;
            }
        }

        iCountNumMb += 1;
        let idx = iNextMbXyIndex as usize;
        if !*(*dq).grid.mb_correctly_decoded_flag.get(idx) {
            *(*dq).grid.mb_correctly_decoded_flag.get_mut(idx) = true;
            if *(*dq).grid.mb_ref_concealed_flag.get(idx) {
                if !pDec.is_null() {
                    (*pDec).iMbEcedPropNum += 1;
                }
            }
            (*pCtx).iTotalNumMbRec += 1;
        }

        if (*pCtx).iTotalNumMbRec > iTotalMbTargetLayer {
            return ERR_INFO_MB_NUM_EXCEED_FAIL;
        }

        if !(*pSliceHeader).pps_id.is_none() && (*(pps_of(pCtx, (*pSliceHeader).pps_id))).uiNumSliceGroups > 1 {
            iNextMbXyIndex = crate::decoder::fmo::FmoNextMb(active_fmo(pCtx), iNextMbXyIndex);
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

    if !pDec.is_null() {
        (*pDec).iWidthInPixel = iCurLayerWidth;
        (*pDec).iHeightInPixel = iCurLayerHeight;
    }

    if (*pCurSlice).eSliceType != EWelsSliceType::I_SLICE as u8
        && (*pCurSlice).eSliceType != EWelsSliceType::P_SLICE as u8
        && (*pCurSlice).eSliceType != EWelsSliceType::B_SLICE as u8
    {
        return ERR_NONE;
    }

    let bParseOnly = if !(*pCtx).pParam.is_null() { (*(*pCtx).pParam).bParseOnly } else { false };
    if bParseOnly {
        return ERR_NONE;
    }

    if (*pSliceHeader).uiDisableDeblockingFilterIdc == 1 || (*pCurSlice).iTotalMbInCurSlice <= 0 {
        return ERR_NONE;
    } else {
        crate::decoder::deblocking::WelsDeblockingFilterSlice(
            pCtx, pCurDqLayer,
            pDec,
            Some(crate::decoder::deblocking::WelsDeblockingMb),
        );
    }

    ERR_NONE
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
unsafe fn DecodeMbCavlcPcm(pCtx: *mut SWelsDecoderContext, buf: &[u8], pBs: &mut BsCursor, dq: *mut DqLayerState, pDec: PPicture) -> i32 {
    let iMbX = (*dq).iMbX;
    let iMbY = (*dq).iMbY;
    let iMbXy = (*dq).iMbXyIndex as usize;

    let iDecStrideL = (*pDec).linesize(0);
    let iDecStrideC = (*pDec).linesize(1);

    let iOffsetL = ((iMbX + iMbY * iDecStrideL) << 4) as isize;
    let iOffsetC = ((iMbX + iMbY * iDecStrideC) << 3) as isize;

    let mut pDecY = (*pDec).data_ptr(0).offset(iOffsetL);
    let mut pDecU = (*pDec).data_ptr(1).offset(iOffsetC);
    let mut pDecV = (*pDec).data_ptr(2).offset(iOffsetC);

    let iIndex = ((-pBs.left_bits()) >> 3) + 2;

    *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA_PCM;

    // step 1: locate the bit-stream position (must align to an integer byte).
    // `pCurBuf - iIndex` becomes `pos - iIndex`; the C++ computed a pointer here and a
    // negative result was an out-of-bounds pointer with no check, so an underflow is a
    // pre-existing overrun surfacing (plan §2.2.2) — `pos` is `usize` and the slice
    // index below is what reports it.
    let iPcmStart = (pBs.pos() as isize - iIndex as isize) as usize;
    pBs.set_pos(iPcmStart);

    // step 2: copy pixels from the bit-stream into the decoded picture
    let mut pTmpBsBuf = buf[iPcmStart..].as_ptr();
    let bParseOnly = !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly;
    if !bParseOnly {
        for _ in 0..16 {
            std::ptr::copy_nonoverlapping(pTmpBsBuf, pDecY, 16);
            pDecY = pDecY.offset(iDecStrideL as isize);
            pTmpBsBuf = pTmpBsBuf.add(16);
        }
        for _ in 0..8 {
            std::ptr::copy_nonoverlapping(pTmpBsBuf, pDecU, 8);
            pDecU = pDecU.offset(iDecStrideC as isize);
            pTmpBsBuf = pTmpBsBuf.add(8);
        }
        for _ in 0..8 {
            std::ptr::copy_nonoverlapping(pTmpBsBuf, pDecV, 8);
            pDecV = pDecV.offset(iDecStrideC as isize);
            pTmpBsBuf = pTmpBsBuf.add(8);
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

/// Matches `WelsActualDecodeMbCavlcISlice` in `decode_slice.cpp`.
pub unsafe extern "C" fn WelsActualDecodeMbCavlcISlice(pCtx: *mut SWelsDecoderContext, buf: &[u8], pBs: &mut BsCursor, dq: *mut DqLayerState, pDec: PPicture) -> i32 {
    let pVlcTable = (*pCtx).pVlcTable as *mut crate::decoder::parse_mb_syn_cavlc::SVlcTable;
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);

    let iScanIdxStart = (*pSlice).sSliceHeaderExt.uiScanIdxStart as usize;
    let iScanIdxEnd = (*pSlice).sSliceHeaderExt.uiScanIdxEnd as usize;

    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;
    let mut uiCbp;
    let mut uiCbpC = 0u32;
    let mut uiCbpL = 0u32;

    let mut sNeighAvail = SWelsNeighAvail::default();
    let mut pNonZeroCount = [0u8; 48];
    crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, dq, pDec);

    // T5.I3: two windows for the macroblock, opened after the neighbour
    // scan — `GetNeighborAvailMbType` reads `cbp` at the left and top
    // addresses, so it has to run first. Both borrows are dead by the
    // residual call, which reads `transform_size8x8_flag` through its own.
    let pCbp = (*dq).grid.cbp.get_mut(iMbXy);
    let pTransformSize8x8Flag = (*dq).grid.transform_size8x8_flag.get_mut(iMbXy);
    *(*dq).grid.residual_pred_flag.get_mut(iMbXy) = (*pSlice).sSliceHeaderExt.bDefaultResidualPredFlag as i8;

    *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
    *pTransformSize8x8Flag = false;

    let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
    if ret != 0 {
        return ret as i32;
    }
    let mut uiMbType = uiCode;
    if uiMbType > 25 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
    }
    if (*active_sps(pCtx)).uiChromaFormatIdc == 0
        && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
    }

    if 25 == uiMbType {
        return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, pDec);
    } else if 0 == uiMbType {
        let mut pIntraPredMode = [0i8; 48];
        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
        if (*active_pps(pCtx)).bTransform8x8ModeFlag {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            *pTransformSize8x8Flag = uiCode != 0;
            if uiCode != 0 {
                *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                uiMbType = MB_TYPE_INTRA8x8;
            }
        }
        (*pCtx).eIntraPredConstraint.FillCacheIntraNxN(
            &mut sNeighAvail,
            &mut pNonZeroCount,
            pIntraPredMode.as_mut_ptr(),
            dq,
        );
        let ret = if !*pTransformSize8x8Flag {
            ParseIntra4x4Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
        } else {
            ParseIntra8x8Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
        };
        if ret != ERR_NONE {
            return ret;
        }

        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        uiCbp = uiCode;
        if (*active_sps(pCtx)).uiChromaFormatIdc != 0 && uiCbp > 47 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0 && uiCbp > 15 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        uiCbp = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
            crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
        } else {
            crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
        };
        *pCbp = uiCbp as i8;
        uiCbpC = uiCbp >> 4;
        uiCbpL = uiCbp & 15;
    } else {
        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
        *pTransformSize8x8Flag = false;
        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
        *pCbp = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
        uiCbpC = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
            (*pCbp as u32) >> 4
        } else {
            0
        };
        uiCbpL = (*pCbp as u32) & 15;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
            &mut sNeighAvail,
            &mut pNonZeroCount,
            dq,
        );
        let ret = ParseIntra16x16Mode(pCtx, pDec, &mut sNeighAvail, buf, pBs, dq);
        if ret != ERR_NONE {
            return ret;
        }
    }

    let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
    pNzc.fill(0);

    if *pCbp == 0 && IS_INTRANxN(*(*pDec).pMbType.get(iMbXy)) {
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        *(*dq).grid.luma_qp.get_mut(iMbXy) = (*pSlice).iLastMbQp as i8;
        for i in 0..2 {
            let idx = WELS_CLIP3(
                *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh.iChromaQpIndexOffset[i] as i32,
                0,
                51,
            );
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }
    }

    if *pCbp != 0 || MB_TYPE_INTRA16x16 == *(*pDec).pMbType.get(iMbXy) {
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
        let new_qp = ((*pSlice).iLastMbQp + iQpDelta + 52) % 52;
        *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
        (*pSlice).iLastMbQp = new_qp;
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        for i in 0..2 {
            let idx = WELS_CLIP3(new_qp + pps_sh.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        pBs.start_cavlc();

        let ret = WelsDecodeMbCavlcResidual(
            pCtx,
            buf,
            pBs,
            dq,
            pDec,
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

/// Shared CAVLC residual decode for luma + chroma blocks of one macroblock.
/// Matches the residual sections of `WelsActualDecodeMbCavlcISlice` /
/// `WelsActualDecodeMbCavlcPSlice` in `decode_slice.cpp` (after `BsStartCavlc`,
/// up to `BsEndCavlc`).
///
/// **The reader arrives as parameters — F47.** This function used to open with its
/// own `(*slice_bit_reader(pCtx)).split(&(*pCtx).sRawData)`, and all three of its
/// callers had already taken that same split at their own heads. Two live `&mut`
/// derivations of one `BsCursor` through a raw pointer: the callee's function-entry
/// retag popped the caller's tag, and the caller then used it again —
/// `pBs.end_cavlc(buf)` — which is Undefined Behaviour on the ordinary CAVLC path,
/// every macroblock that carries residual. Threading the split down is the same
/// bracket maneuver W3 used: derive once at the top, pass it, touch the source
/// nowhere below.
unsafe fn WelsDecodeMbCavlcResidual(
    pCtx: *mut SWelsDecoderContext,
    buf: &[u8],
    pBs: &mut BsCursor,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pVlcTable: *mut crate::decoder::parse_mb_syn_cavlc::SVlcTable,
    pNonZeroCount: &mut [u8; 48],
    iScanIdxStart: usize,
    iScanIdxEnd: usize,
    uiCbpL: u32,
    uiCbpC: u32,
) -> i32 {
    let iMbXy = (*dq).iMbXyIndex as usize;
    let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
    let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
    let mb_type = *(*pDec).pMbType.get(iMbXy);
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
            g_kuiLumaDcZigzagScan.as_ptr(),
            I16_LUMA_DC,
            scaled_tcoeff_mb.as_mut_ptr(),
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
                    g_kuiZigzagScan.as_ptr().add(max_idx),
                    I16_LUMA_AC,
                    scaled_tcoeff_mb.as_mut_ptr().add(i << 4),
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
                            g_kuiZigzagScan8x8.as_ptr().add(iScanIdxStart),
                            iMbResProperty,
                            scaled_tcoeff_mb.as_mut_ptr().add(iId8x8 << 6),
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
                            g_kuiZigzagScan.as_ptr().add(iScanIdxStart),
                            iMbResProperty,
                            scaled_tcoeff_mb.as_mut_ptr().add((iIndex as usize) << 4),
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
                g_kuiChromaDcScan.as_ptr(),
                iMbResProperty,
                scaled_tcoeff_mb.as_mut_ptr().add(256 + (i << 6)),
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
                    g_kuiZigzagScan.as_ptr().add(max_idx),
                    iMbResProperty,
                    scaled_tcoeff_mb.as_mut_ptr().add(iIndex << 4),
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

pub unsafe extern "C" fn WelsDecodeMbCavlcISlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if dq.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let (buf, pBs) = (*slice_bit_reader(pCtx)).split(&(*pCtx).sRawData);
    let pSliceHeaderExt = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt);
    let mut uiCode = 0u32;
    let iBaseModeFlag;
    if (*pSliceHeaderExt).bAdaptiveBaseModeFlag {
        if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
            return ERR_INFO_INVALID_ACCESS;
        }
        iBaseModeFlag = uiCode != 0;
    } else {
        iBaseModeFlag = (*pSliceHeaderExt).bDefaultBaseModeFlag;
    }
    if iBaseModeFlag {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_ILP);
    }
    let ret = WelsActualDecodeMbCavlcISlice(pCtx, buf, pBs, dq, pDec);
    if ret != ERR_NONE {
        return ret;
    }
    let iUsedBits = (pBs.pos() as i32) * 8 - (16 - pBs.left_bits());
    if iUsedBits == (pBs.bits() - 1) && (*dq).sLayerInfo.sSliceInLayer.iMbSkipRun <= 0 {
        if !uiEosFlag.is_null() {
            *uiEosFlag = 1;
        }
    }
    if iUsedBits > (pBs.bits() - 1) {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
    }
    ERR_NONE
}

/// Matches `WelsActualDecodeMbCavlcPSlice` in `decode_slice.cpp`.
pub unsafe extern "C" fn WelsActualDecodeMbCavlcPSlice(pCtx: *mut SWelsDecoderContext, buf: &[u8], pBs: &mut BsCursor, dq: *mut DqLayerState, pDec: PPicture, pRefs: PicRefs<'_>) -> i32 {
    let pVlcTable = (*pCtx).pVlcTable as *mut crate::decoder::parse_mb_syn_cavlc::SVlcTable;
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);

    let iScanIdxStart = (*pSlice).sSliceHeaderExt.uiScanIdxStart as usize;
    let iScanIdxEnd = (*pSlice).sSliceHeaderExt.uiScanIdxEnd as usize;

    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;
    let mut uiCbp;
    let mut uiCbpC = 0u32;
    let mut uiCbpL = 0u32;

    let mut sNeighAvail = SWelsNeighAvail::default();
    let mut pNonZeroCount = [0u8; 48];
    crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, dq, pDec);

    // T5.I3: two windows for the macroblock, opened after the neighbour
    // scan — `GetNeighborAvailMbType` reads `cbp` at the left and top
    // addresses, so it has to run first. Both borrows are dead by the
    // residual call, which reads `transform_size8x8_flag` through its own.
    let pCbp = (*dq).grid.cbp.get_mut(iMbXy);
    let pTransformSize8x8Flag = (*dq).grid.transform_size8x8_flag.get_mut(iMbXy);

    let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
    if ret != 0 {
        return ret as i32;
    }
    let mut uiMbType = uiCode;
    if uiMbType < 5 {
        // inter MB type
        let mut iMotionVector = [[[0i16; 2]; 30]; 2];
        let mut iRefIndex = [[0i8; 30]; 2];
        *(*pDec).pMbType.get_mut(iMbXy) = g_ksInterPMbTypeInfo[uiMbType as usize].iType;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInter(
            &sNeighAvail,
            &mut pNonZeroCount,
            &mut iMotionVector,
            &mut iRefIndex,
            dq,
            pDec,
        );

        let ret = crate::decoder::parse_mb_syn_cavlc::ParseInterInfo(
            pCtx, dq,
            pDec,
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
        if (*pSlice).sSliceHeaderExt.bAdaptiveResidualPredFlag {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            *pResidualPredFlag = uiCode as i8;
        } else {
            *pResidualPredFlag = (*pSlice).sSliceHeaderExt.bDefaultResidualPredFlag as i8;
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
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0
            && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }

        if 25 == uiMbType {
            return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, pDec);
        } else if 0 == uiMbType {
            let mut pIntraPredMode = [0i8; 48];
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
            if (*active_pps(pCtx)).bTransform8x8ModeFlag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *pTransformSize8x8Flag = uiCode != 0;
                if uiCode != 0 {
                    *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                }
            }
            (*pCtx).eIntraPredConstraint.FillCacheIntraNxN(
            &mut sNeighAvail,
            &mut pNonZeroCount,
            pIntraPredMode.as_mut_ptr(),
            dq,
        );
            let ret = if !*pTransformSize8x8Flag {
                ParseIntra4x4Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
            } else {
                ParseIntra8x8Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
            };
            if ret != ERR_NONE {
                return ret;
            }
        } else {
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
            *pTransformSize8x8Flag = false;
            *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
            (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
            *pCbp = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
            uiCbpC = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                (*pCbp as u32) >> 4
            } else {
                0
            };
            uiCbpL = (*pCbp as u32) & 15;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                dq,
            );
            let ret = ParseIntra16x16Mode(pCtx, pDec, &mut sNeighAvail, buf, pBs, dq);
            if ret != ERR_NONE {
                return ret;
            }
        }
    }

    if MB_TYPE_INTRA16x16 != *(*pDec).pMbType.get(iMbXy) {
        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        uiCbp = uiCode;
        if (*active_sps(pCtx)).uiChromaFormatIdc != 0 && uiCbp > 47 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0 && uiCbp > 15 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        let mb_type = *(*pDec).pMbType.get(iMbXy);
        uiCbp = if MB_TYPE_INTRA4x4 == mb_type || MB_TYPE_INTRA8x8 == mb_type {
            if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
            } else {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
            }
        } else {
            if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                crate::decoder::dec_golomb::g_kuiInterCbpTable[uiCbp as usize] as u32
            } else {
                crate::decoder::dec_golomb::g_kuiInterCbpTable400[uiCbp as usize] as u32
            }
        };

        *pCbp = uiCbp as i8;
        uiCbpC = uiCbp >> 4;
        uiCbpL = uiCbp & 15;

        let mb_type = *(*pDec).pMbType.get(iMbXy);
        let bNeedParseTransformSize8x8Flag = ((mb_type >= MB_TYPE_16x16 && mb_type <= MB_TYPE_8x16)
            || *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get(iMbXy))
            && mb_type != MB_TYPE_INTRA8x8
            && mb_type != MB_TYPE_INTRA4x4
            && uiCbpL > 0
            && (*active_pps(pCtx)).bTransform8x8ModeFlag;

        if bNeedParseTransformSize8x8Flag {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            *pTransformSize8x8Flag = uiCode != 0;
        }
    }

    let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
    pNzc.fill(0);

    let mb_type = *(*pDec).pMbType.get(iMbXy);
    if *pCbp == 0 && !IS_INTRA16x16(mb_type) && mb_type != MB_TYPE_INTRA_BL {
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        *(*dq).grid.luma_qp.get_mut(iMbXy) = (*pSlice).iLastMbQp as i8;
        for i in 0..2 {
            let idx = WELS_CLIP3(
                *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh.iChromaQpIndexOffset[i] as i32,
                0,
                51,
            );
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }
    }

    if *pCbp != 0 || MB_TYPE_INTRA16x16 == *(*pDec).pMbType.get(iMbXy) {
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
        let new_qp = ((*pSlice).iLastMbQp + iQpDelta + 52) % 52;
        *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
        (*pSlice).iLastMbQp = new_qp;
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        for i in 0..2 {
            let idx = WELS_CLIP3(new_qp + pps_sh.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        pBs.start_cavlc();

        let ret = WelsDecodeMbCavlcResidual(
            pCtx,
            buf,
            pBs,
            dq,
            pDec,
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

pub unsafe extern "C" fn WelsDecodeMbCavlcPSlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if dq.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let (buf, pBs) = (*slice_bit_reader(pCtx)).split(&(*pCtx).sRawData);
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeaderExt = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt);
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiCode = 0u32;

    if (*pSlice).iMbSkipRun == -1 {
        if crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode) != 0 {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSlice).iMbSkipRun = uiCode as i32;
        if (*pSlice).iMbSkipRun == -1 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_PARAM);
        }
    }

    // C++ uses `if (pSlice->iMbSkipRun--)`: a coded macroblock leaves the
    // counter at -1 so the next macroblock parses a fresh mb_skip_run.
    let bSkip = (*pSlice).iMbSkipRun != 0;
    (*pSlice).iMbSkipRun -= 1;
    if bSkip {
        let mut iMv = [0i16; 2];

        if !pDec.is_null() {
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_SKIP;
        }
        for j in 0..24 {
            *(*dq).grid.nzc.get_mut(iMbXy).as_mut_ptr().add(j) = 0;
        }
        if !pDec.is_null() {
            for j in 0..16 {
                (*pDec).pRefIndex[0].get_mut(iMbXy)[j] = 0;
            }
        }
        crate::decoder::mv_pred::PredPSkipMvFromNeighbor(dq, pDec, &mut iMv);
        if !pDec.is_null() {
            for j in 0..16 {
                (*pDec).pMv[0].get_mut(iMbXy)[j] = iMv;
            }
        }

        let iLastMbQp = (*pSlice).iLastMbQp;
        *(*dq).grid.luma_qp.get_mut(iMbXy) = iLastMbQp as i8;
        let pps_ptr = pps_of(pCtx, (*pSliceHeaderExt).sSliceHeader.pps_id);
        for i in 0..2 {
            let offset = if !pps_ptr.is_null() {
                (*pps_ptr).iChromaQpIndexOffset[i]
            } else {
                0
            };
            let qp_idx = WELS_CLIP3(iLastMbQp as i32 + offset as i32, 0, 51) as usize;
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[qp_idx] as i8;
        }

        *(*dq).grid.cbp.get_mut(iMbXy) = 0;
    } else {
        let iBaseModeFlag;
        if (*pSliceHeaderExt).bAdaptiveBaseModeFlag {
            if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
                return ERR_INFO_INVALID_ACCESS;
            }
            iBaseModeFlag = uiCode != 0;
        } else {
            iBaseModeFlag = (*pSliceHeaderExt).bDefaultBaseModeFlag;
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
    if iUsedBits == (pBs.bits() - 1) && (*pSlice).iMbSkipRun <= 0 {
        if !uiEosFlag.is_null() {
            *uiEosFlag = 1;
        }
    }
    if iUsedBits > (pBs.bits() - 1) {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
    }
    ERR_NONE
}

/// Matches `WelsActualDecodeMbCavlcBSlice` in `decode_slice.cpp`.
///
/// Identical to [`WelsActualDecodeMbCavlcPSlice`] apart from the inter/intra
/// `mb_type` split (23 instead of 5), the mb-type table and the motion parser,
/// so the residual half is shared through [`WelsDecodeMbCavlcResidual`].
pub unsafe extern "C" fn WelsActualDecodeMbCavlcBSlice(pCtx: *mut SWelsDecoderContext, buf: &[u8], pBs: &mut BsCursor, dq: *mut DqLayerState, pDec: PPicture, pRefs: PicRefs<'_>) -> i32 {
    let pVlcTable = (*pCtx).pVlcTable as *mut crate::decoder::parse_mb_syn_cavlc::SVlcTable;
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);

    let iScanIdxStart = (*pSlice).sSliceHeaderExt.uiScanIdxStart as usize;
    let iScanIdxEnd = (*pSlice).sSliceHeaderExt.uiScanIdxEnd as usize;

    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;
    let mut uiCbp;
    let mut uiCbpC = 0u32;
    let mut uiCbpL = 0u32;

    let mut sNeighAvail = SWelsNeighAvail::default();
    let mut pNonZeroCount = [0u8; 48];
    crate::decoder::parse_mb_syn_cavlc::GetNeighborAvailMbType(&mut sNeighAvail, dq, pDec);

    // T5.I3: two windows for the macroblock, opened after the neighbour
    // scan — `GetNeighborAvailMbType` reads `cbp` at the left and top
    // addresses, so it has to run first. Both borrows are dead by the
    // residual call, which reads `transform_size8x8_flag` through its own.
    let pCbp = (*dq).grid.cbp.get_mut(iMbXy);
    let pTransformSize8x8Flag = (*dq).grid.transform_size8x8_flag.get_mut(iMbXy);

    let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
    if ret != 0 {
        return ret as i32;
    }
    let mut uiMbType = uiCode;
    if uiMbType < 23 {
        // inter MB type
        let mut iMotionVector = [[[0i16; 2]; 30]; 2];
        let mut iRefIndex = [[0i8; 30]; 2];
        *(*pDec).pMbType.get_mut(iMbXy) = g_ksInterBMbTypeInfo[uiMbType as usize].iType;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInter(
            &sNeighAvail,
            &mut pNonZeroCount,
            &mut iMotionVector,
            &mut iRefIndex,
            dq,
            pDec,
        );

        let ret = crate::decoder::parse_mb_syn_cavlc::ParseInterBInfo(
            pCtx, dq,
            pDec,
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
        if (*pSlice).sSliceHeaderExt.bAdaptiveResidualPredFlag {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            *pResidualPredFlag = uiCode as i8;
        } else {
            *pResidualPredFlag = (*pSlice).sSliceHeaderExt.bDefaultResidualPredFlag as i8;
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
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0
            && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }

        if 25 == uiMbType {
            return DecodeMbCavlcPcm(pCtx, buf, pBs, dq, pDec);
        } else if 0 == uiMbType {
            let mut pIntraPredMode = [0i8; 48];
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
            if (*active_pps(pCtx)).bTransform8x8ModeFlag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                *pTransformSize8x8Flag = uiCode != 0;
                if uiCode != 0 {
                    *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
                }
            }
            (*pCtx).eIntraPredConstraint.FillCacheIntraNxN(
            &mut sNeighAvail,
            &mut pNonZeroCount,
            pIntraPredMode.as_mut_ptr(),
            dq,
        );
            let ret = if !*pTransformSize8x8Flag {
                ParseIntra4x4Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
            } else {
                ParseIntra8x8Mode(pCtx, pDec, &mut sNeighAvail, pIntraPredMode.as_mut_ptr(), buf, pBs, dq)
            };
            if ret != ERR_NONE {
                return ret;
            }
        } else {
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
            *pTransformSize8x8Flag = false;
            *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
            (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType - 1) & 3) as i8;
            *pCbp = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
            uiCbpC = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                (*pCbp as u32) >> 4
            } else {
                0
            };
            uiCbpL = (*pCbp as u32) & 15;
            crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
                &mut sNeighAvail,
                &mut pNonZeroCount,
                dq,
            );
            let ret = ParseIntra16x16Mode(pCtx, pDec, &mut sNeighAvail, buf, pBs, dq);
            if ret != ERR_NONE {
                return ret;
            }
        }
    }

    if MB_TYPE_INTRA16x16 != *(*pDec).pMbType.get(iMbXy) {
        let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
        if ret != 0 {
            return ret as i32;
        }
        uiCbp = uiCode;
        if (*active_sps(pCtx)).uiChromaFormatIdc != 0 && uiCbp > 47 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0 && uiCbp > 15 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_CBP);
        }
        let mb_type = *(*pDec).pMbType.get(iMbXy);
        uiCbp = if MB_TYPE_INTRA4x4 == mb_type || MB_TYPE_INTRA8x8 == mb_type {
            if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable[uiCbp as usize] as u32
            } else {
                crate::decoder::dec_golomb::g_kuiIntra4x4CbpTable400[uiCbp as usize] as u32
            }
        } else {
            if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
                crate::decoder::dec_golomb::g_kuiInterCbpTable[uiCbp as usize] as u32
            } else {
                crate::decoder::dec_golomb::g_kuiInterCbpTable400[uiCbp as usize] as u32
            }
        };

        *pCbp = uiCbp as i8;
        uiCbpC = uiCbp >> 4;
        uiCbpL = uiCbp & 15;

        let mb_type = *(*pDec).pMbType.get(iMbXy);
        let bNeedParseTransformSize8x8Flag = ((mb_type >= MB_TYPE_16x16 && mb_type <= MB_TYPE_8x16)
            || *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get(iMbXy))
            && mb_type != MB_TYPE_INTRA8x8
            && mb_type != MB_TYPE_INTRA4x4
            && uiCbpL > 0
            && (*active_pps(pCtx)).bTransform8x8ModeFlag;

        if bNeedParseTransformSize8x8Flag {
            let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            *pTransformSize8x8Flag = uiCode != 0;
        }
    }

    let pNzc = (*dq).grid.nzc.get_mut(iMbXy);
    pNzc.fill(0);

    let mb_type = *(*pDec).pMbType.get(iMbXy);
    if *pCbp == 0 && !IS_INTRA16x16(mb_type) && mb_type != MB_TYPE_INTRA_BL {
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        *(*dq).grid.luma_qp.get_mut(iMbXy) = (*pSlice).iLastMbQp as i8;
        for i in 0..2 {
            let idx = WELS_CLIP3(
                *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh.iChromaQpIndexOffset[i] as i32,
                0,
                51,
            );
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }
    }

    if *pCbp != 0 || MB_TYPE_INTRA16x16 == *(*pDec).pMbType.get(iMbXy) {
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
        let new_qp = ((*pSlice).iLastMbQp + iQpDelta + 52) % 52;
        *(*dq).grid.luma_qp.get_mut(iMbXy) = new_qp as i8;
        (*pSlice).iLastMbQp = new_qp;
        let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
        let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
        for i in 0..2 {
            let idx = WELS_CLIP3(new_qp + pps_sh.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        pBs.start_cavlc();

        let ret = WelsDecodeMbCavlcResidual(
            pCtx,
            buf,
            pBs,
            dq,
            pDec,
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

/// Matches `WelsDecodeMbCavlcBSlice` in `decode_slice.cpp`.
pub unsafe extern "C" fn WelsDecodeMbCavlcBSlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if dq.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let (buf, pBs) = (*slice_bit_reader(pCtx)).split(&(*pCtx).sRawData);
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeader = &(*pSlice).sSliceHeaderExt.sSliceHeader;
    let ppRefPicL0 = pRefs.get(ref_id(pCtx, LIST_0, 0));
    let ppRefPicL1 = pRefs.get(ref_id(pCtx, LIST_1, 0));
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiCode = 0u32;

    *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
    *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;

    if (*pSlice).iMbSkipRun == -1 {
        // mb_skip_run
        if crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode) != 0 {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSlice).iMbSkipRun = uiCode as i32;
        if (*pSlice).iMbSkipRun == -1 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_SKIP_RUN);
        }
        if (uiCode) > (((*dq).iMbWidth * (*dq).iMbHeight - iMbXy as i32) as u32) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_SKIP_RUN);
        }
    }

    // C++ uses `if (pSlice->iMbSkipRun--)`: a coded macroblock leaves the
    // counter at -1 so the next macroblock parses a fresh mb_skip_run.
    let bSkip = (*pSlice).iMbSkipRun != 0;
    (*pSlice).iMbSkipRun -= 1;
    if bSkip {
        let mut iMv = [[0i16; 2]; LIST_A];
        let mut iRef = [0i8; LIST_A];

        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_SKIP | MB_TYPE_DIRECT;
        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);

        ((*pDec).pRefIndex[LIST_0].get_mut(iMbXy)).fill(0);
        ((*pDec).pRefIndex[LIST_1].get_mut(iMbXy)).fill(0);

        let bIsPending = crate::decoder::decoder_core::GetThreadCount(pCtx) > 1;
        let is_complete0 = !ppRefPicL0.is_null() && ((*ppRefPicL0).bIsComplete || bIsPending);
        let is_complete1 = !ppRefPicL1.is_null() && ((*ppRefPicL1).bIsComplete || bIsPending);
        (*pCtx).bMbRefConcealed =
            (*pCtx).bRPLRError || (*pCtx).bMbRefConcealed || !is_complete0 || !is_complete1;

        // NOTE: unlike the CABAC B path, C keeps the `if (pCtx->bMbRefConcealed)
        // return ERR_INFO_REFERENCE_PIC_LOST` block commented out here.

        // predict iMv
        let mut subMbType: crate::decoder::mv_pred::SubMbType = 0;
        if (*pSliceHeader).iDirectSpatialMvPredFlag != 0 {
            // predict direct spatial mv
            let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                pCtx, dq,
                pDec,
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
                pCtx, dq,
                pDec,
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
        if !(*pSlice).sSliceHeaderExt.bDefaultResidualPredFlag
            || (!pNalCur.is_null()
                && (*pNalCur).sNalHeaderExt.uiQualityId == 0
                && (*pNalCur).sNalHeaderExt.uiDependencyId == 0)
        {
            let iLastMbQp = (*pSlice).iLastMbQp;
            *(*dq).grid.luma_qp.get_mut(iMbXy) = iLastMbQp as i8;
            let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
            for i in 0..2 {
                let idx = WELS_CLIP3(
                    *(*dq).grid.luma_qp.get(iMbXy) as i32 + pps_sh.iChromaQpIndexOffset[i] as i32,
                    0,
                    51,
                );
                (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
            }
        }

        *(*dq).grid.cbp.get_mut(iMbXy) = 0;
    } else {
        let iBaseModeFlag;
        if (*pSlice).sSliceHeaderExt.bAdaptiveBaseModeFlag {
            if crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode) != 0 {
                return ERR_INFO_INVALID_ACCESS;
            }
            iBaseModeFlag = uiCode != 0;
        } else {
            iBaseModeFlag = (*pSlice).sSliceHeaderExt.bDefaultBaseModeFlag;
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
        if !uiEosFlag.is_null() {
            *uiEosFlag = 1;
        }
    }
    if iUsedBits > (pBs.bits() - 1) {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_BS_INCOMPLETE);
    }
    ERR_NONE
}

pub unsafe fn ParseIntra4x4Mode(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pNeighAvail: *mut SWelsNeighAvail,
    pIntraPredMode: *mut i8,
    buf: &[u8],
    // `*mut`, not `&mut`: a borrow here is *strongly protected* for the call's
    // whole duration, and the CABAC arm below re-reaches this very cursor through
    // `bit_stream::slice_bit_reader` (F27). Raw in, `&mut` re-derived per use
    // — S29's spelling, S25's rule that no borrow outlives one expression.
    pBsAux: *mut BsCursor,
    pCurDqLayer: *mut DqLayerState,
) -> i32 {
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut iSampleAvail = [0i32; 30];
    let uiNeighAvail;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    (*pCtx)
        .eIntraPredConstraint
        .MapNxNNeighToSample(pNeighAvail, iSampleAvail.as_mut_ptr());

    uiNeighAvail = ((iSampleAvail[6] << 2) | (iSampleAvail[0] << 1) | (iSampleAvail[1])) as u8;

    let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.pps_id));
    // T5.I5: the sixteen 4x4 modes are written through one window. Nothing in
    // the loop reaches this family — `ParseIntraPredModeLumaCabac` and
    // `CheckIntraNxNPredMode` do not — and the record is `[i8; 16]`, so the
    // scan-order index inside it is bounded by a constant.
    let pIntra4x4FinalMode = (*dq).grid.intra4x4_final_mode.get_mut(iMbXy);
    for i in 0..16 {
        let iPrevIntra4x4PredMode;
        if pps.bEntropyCodingModeFlag {
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
        if pps.bEntropyCodingModeFlag {
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
            iSampleAvail.as_ptr(),
            &mut iBestMode,
            i,
            false,
        );
        if iFinalMode == GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE) {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I4x4_PRED_MODE);
        }

        pIntra4x4FinalMode[g_kuiScan4[i as usize] as usize] = iFinalMode as i8;
        *pIntraPredMode.add(g_kuiScan8[i as usize] as usize) = iBestMode;
        iSampleAvail[g_kCache30ScanIdx[i as usize] as usize] = 1;
    }

    let dst_modes = (*dq).grid.intra_pred_mode.get_mut(iMbXy).as_mut_ptr();
    *dst_modes.add(0) = *pIntraPredMode.add(1 + 8 * 4);
    *dst_modes.add(1) = *pIntraPredMode.add(2 + 8 * 4);
    *dst_modes.add(2) = *pIntraPredMode.add(3 + 8 * 4);
    *dst_modes.add(3) = *pIntraPredMode.add(4 + 8 * 4);
    *dst_modes.add(4) = *pIntraPredMode.add(4 + 8 * 1);
    *dst_modes.add(5) = *pIntraPredMode.add(4 + 8 * 2);
    *dst_modes.add(6) = *pIntraPredMode.add(4 + 8 * 3);

    if (*active_sps(pCtx)).uiChromaFormatIdc == 0 {
        return ERR_NONE;
    }

    if pps.bEntropyCodingModeFlag {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, pCurDqLayer,
            pDec,
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

pub unsafe fn ParseIntra8x8Mode(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pNeighAvail: *mut SWelsNeighAvail,
    pIntraPredMode: *mut i8,
    buf: &[u8],
    // `*mut`, not `&mut`: a borrow here is *strongly protected* for the call's
    // whole duration, and the CABAC arm below re-reaches this very cursor through
    // `bit_stream::slice_bit_reader` (F27). Raw in, `&mut` re-derived per use
    // — S29's spelling, S25's rule that no borrow outlives one expression.
    pBsAux: *mut BsCursor,
    pCurDqLayer: *mut DqLayerState,
) -> i32 {
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut iSampleAvail = [0i32; 30];
    let uiNeighAvail;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    (*pCtx)
        .eIntraPredConstraint
        .MapNxNNeighToSample(pNeighAvail, iSampleAvail.as_mut_ptr());

    uiNeighAvail = ((iSampleAvail[5] << 3)
        | (iSampleAvail[6] << 2)
        | (iSampleAvail[0] << 1)
        | (iSampleAvail[1])) as u8;
    *(*dq).grid.intra_nxn_avail_flag.get_mut(iMbXy) = uiNeighAvail;

    let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.pps_id));
    // T5.I5: as in `ParseIntra4x4Mode` — sixteen writes, one check.
    let pIntra4x4FinalMode = (*dq).grid.intra4x4_final_mode.get_mut(iMbXy);
    for i in 0..4usize {
        let iPrevIntra4x4PredMode;
        if pps.bEntropyCodingModeFlag {
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
        if pps.bEntropyCodingModeFlag {
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
            iSampleAvail.as_ptr(),
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
            *pIntraPredMode.add(g_kuiScan8[(i << 2) + j] as usize) = iBestMode;
            iSampleAvail[g_kCache30ScanIdx[(i << 2) + j] as usize] = 1;
        }
    }

    // `ST32 (&pIntraPredMode[iMbXy][0], LD32 (&pIntraPredMode[1 + 8 * 4]))` copies
    // four modes, not one; entries 1..3 feed the left-neighbour cache of the next
    // macroblock (WelsFillCacheConstrain0IntraNxN reads [3]).
    let dst_modes = (*dq).grid.intra_pred_mode.get_mut(iMbXy).as_mut_ptr();
    *dst_modes.add(0) = *pIntraPredMode.add(1 + 8 * 4);
    *dst_modes.add(1) = *pIntraPredMode.add(2 + 8 * 4);
    *dst_modes.add(2) = *pIntraPredMode.add(3 + 8 * 4);
    *dst_modes.add(3) = *pIntraPredMode.add(4 + 8 * 4);
    *dst_modes.add(4) = *pIntraPredMode.add(4 + 8 * 1);
    *dst_modes.add(5) = *pIntraPredMode.add(4 + 8 * 2);
    *dst_modes.add(6) = *pIntraPredMode.add(4 + 8 * 3);

    if (*active_sps(pCtx)).uiChromaFormatIdc == 0 {
        return ERR_NONE;
    }

    if pps.bEntropyCodingModeFlag {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, pCurDqLayer,
            pDec,
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

pub unsafe fn ParseIntra16x16Mode(
    pCtx: *mut SWelsDecoderContext,
    pDec: PPicture,
    pNeighAvail: *mut SWelsNeighAvail,
    buf: &[u8],
    // `*mut`, not `&mut`: a borrow here is *strongly protected* for the call's
    // whole duration, and the CABAC arm below re-reaches this very cursor through
    // `bit_stream::slice_bit_reader` (F27). Raw in, `&mut` re-derived per use
    // — S29's spelling, S25's rule that no borrow outlives one expression.
    pBsAux: *mut BsCursor,
    pCurDqLayer: *mut DqLayerState,
) -> i32 {
    let dq: *mut DqLayerState = pCurDqLayer;
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mut uiNeighAvail = 0u8;
    let mut uiCode = 0u32;
    let mut iCode = 0i32;

    (*pCtx)
        .eIntraPredConstraint
        .Map16x16NeighToSample(pNeighAvail, &mut uiNeighAvail);

    let pMode = (*dq).grid.intra_pred_mode.get_mut(iMbXy).as_mut_ptr().add(7);
    if crate::decoder::parse_mb_syn_cavlc::CheckIntra16x16PredMode(uiNeighAvail, pMode) != 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
    }
    if (*active_sps(pCtx)).uiChromaFormatIdc == 0 {
        return ERR_NONE;
    }

    let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.pps_id));
    if pps.bEntropyCodingModeFlag {
        let ret = crate::decoder::parse_mb_syn_cabac::ParseIntraPredModeChromaCabac(
            pCtx, pCurDqLayer,
            pDec,
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

unsafe fn WelsDecodeMbCabacIntraModeHelper(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pNeighAvail: *mut SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    pIntraPredMode: *mut i8,
    uiMbType: u32,
) -> i32 {
    // **Not `split()` here** (F27). `split` hands back `&mut self.cursor`, which this
    // function then passes down as a strongly protected argument — while the CABAC
    // engine underneath reaches the *same* `BsReader` whole through
    // `cabac_rbsp_window`. Two live paths to one object, one of them exclusive —
    // and since T5.M3 both start at the same `slice_bit_reader` derivation rather
    // than at a mirror of it. `addr_of_mut!` creates no reference, so there is no
    // retag to conflict and the CAVLC leaves re-derive per use; S29's spelling.
    let pBsRd: *mut BsReader = slice_bit_reader(pCtx);
    let buf = (*pCtx).sRawData.window_from((*pBsRd).start);
    let pBsAux: *mut BsCursor = std::ptr::addr_of_mut!((*pBsRd).cursor);
    let iMbXy = (*dq).iMbXyIndex as usize;

    if uiMbType == 0 {
        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA4x4;
        let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.pps_id));
        if pps.bTransform8x8ModeFlag {
            // T5.I2 (F34): the callee reads *this array* at the left and top
            // addresses, and `Vec`'s `Index` builds a shared slice over the whole
            // buffer — which removes the strongly-protected `&mut` it was handed.
            // Proved under Miri on a standalone reproduction; unreachable by the
            // aliasing probe, whose stream is one macroblock per frame, so both
            // availability flags are 0 and neither read runs. Keeping the value in
            // a local and storing it after the call has no borrow live across it.
            let mut bTransformSize8x8Flag = false;
            let ret = crate::decoder::parse_mb_syn_cabac::ParseTransformSize8x8FlagCabac(
                pCtx, dq,
                pNeighAvail,
                &mut bTransformSize8x8Flag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = bTransformSize8x8Flag;
        }
        (*pCtx).eIntraPredConstraint.FillCacheIntraNxN(
            pNeighAvail,
            pNonZeroCount,
            pIntraPredMode,
            dq,
        );

        if *(*dq).grid.transform_size8x8_flag.get(iMbXy) {
            *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA8x8;
            ParseIntra8x8Mode(pCtx, pDec, pNeighAvail, pIntraPredMode, buf, pBsAux, dq)
        } else {
            ParseIntra4x4Mode(pCtx, pDec, pNeighAvail, pIntraPredMode, buf, pBsAux, dq)
        }
    } else {
        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA16x16;
        *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = false;
        *(*dq).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) = true;
        (*dq).grid.intra_pred_mode.get_mut(iMbXy)[7] = ((uiMbType as i32 - 1) & 3) as i8;
        *(*dq).grid.cbp.get_mut(iMbXy) = g_kuiI16CbpTable[((uiMbType - 1) >> 2) as usize] as i8;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheNonZeroCount(
            pNeighAvail,
            pNonZeroCount,
            dq,
        );
        ParseIntra16x16Mode(pCtx, pDec, pNeighAvail, buf, pBsAux, dq)
    }
}

unsafe fn WelsDecodeMbCabacResidualHelper(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pNeighAvail: *mut SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    iScanIdxStart: usize,
    iScanIdxEnd: usize,
) -> i32 {
    // **Not `split()` here** (F27). `split` hands back `&mut self.cursor`, which this
    // function then passes down as a strongly protected argument — while the CABAC
    // engine underneath reaches the *same* `BsReader` whole through
    // `cabac_rbsp_window`. Two live paths to one object, one of them exclusive —
    // and since T5.M3 both start at the same `slice_bit_reader` derivation rather
    // than at a mirror of it. `addr_of_mut!` creates no reference, so there is no
    // retag to conflict and the CAVLC leaves re-derive per use; S29's spelling.
    let pBsRd: *mut BsReader = slice_bit_reader(pCtx);
    let buf = (*pCtx).sRawData.window_from((*pBsRd).start);
    let pBsAux: *mut BsCursor = std::ptr::addr_of_mut!((*pBsRd).cursor);
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt.sSliceHeader);
    let pps_sh = &*(pps_of(pCtx, (*pSliceHeader).pps_id));
    let pps_layer = &*(pps_of(pCtx, (*dq).sLayerInfo.pps_id));
    let iMbXy = (*dq).iMbXyIndex as usize;
    let mb_type = *(*pDec).pMbType.get(iMbXy);
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
            (*pSlice).iLastDeltaQp = 0;
        }
        uiCbpChroma = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
            uiCbp >> 4
        } else {
            0
        };
        uiCbpLuma = uiCbp & 15;
    } else {
        uiCbp = *(*dq).grid.cbp.get(iMbXy) as u32;
        uiCbpChroma = if (*active_sps(pCtx)).uiChromaFormatIdc != 0 {
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
                && pps_layer.bTransform8x8ModeFlag;

            if bNeedParseTransformSize8x8Flag {
                // T5.I2 (F34) — as above; the callee reads the same array at the
                // neighbour addresses while holding this borrow.
                let mut bTransformSize8x8Flag = false;
                let ret = crate::decoder::parse_mb_syn_cabac::ParseTransformSize8x8FlagCabac(
                    pCtx, dq,
                    pNeighAvail,
                    &mut bTransformSize8x8Flag,
                );
                if ret != ERR_NONE {
                    return ret;
                }
                *(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = bTransformSize8x8Flag;
            }
        }

        let scaled_tcoeff_mb = (*dq).grid.scaled_tcoeff.get_mut(iMbXy);
        scaled_tcoeff_mb.fill(0);

        let mut iQpDelta = 0i32;
        let ret = crate::decoder::parse_mb_syn_cabac::ParseDeltaQpCabac(
            pCtx, dq,
            &mut iQpDelta,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if iQpDelta > 25 || iQpDelta < -26 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_QP);
        }
        let new_qp = ((*pSlice).iLastMbQp + iQpDelta + 52) % 52;
        // T5.I2: the write opens the window and the four residual reads below —
        // each of them inside `for iId8x8 { for iId4x4 { } }` — read through it.
        // `ParseResidualBlockCabac` reaches the layer but not this family. The
        // `else` arm's write at the tail of the function is this branch's
        // alternative, never the same execution.
        let iLumaQp = (*dq).grid.luma_qp.get_mut(iMbXy);
        *iLumaQp = new_qp as i8;
        (*pSlice).iLastMbQp = new_qp;
        for i in 0..2 {
            let idx =
                WELS_CLIP3(new_qp + pps_sh.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        if mb_type == MB_TYPE_INTRA16x16 {
            let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                pNeighAvail,
                pNonZeroCount,
                0,
                16,
                g_kuiLumaDcZigzagScan.as_ptr(),
                I16_LUMA_DC,
                scaled_tcoeff_mb.as_mut_ptr(),
                *iLumaQp as u8,
                pCtx, dq,
                pDec,
            );
            if ret != ERR_NONE {
                return ret;
            }
            if uiCbpLuma != 0 {
                for i in 0..16 {
                    let max_idx = std::cmp::max(iScanIdxStart, 1);
                    let len = (iScanIdxEnd as isize - max_idx as isize + 1) as i32;
                    let scan_ptr = g_kuiZigzagScan.as_ptr().add(max_idx);
                    let coeff_ptr = scaled_tcoeff_mb.as_mut_ptr().add(i * 16);
                    let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                        pNeighAvail,
                        pNonZeroCount,
                                i as i32,
                        len,
                        scan_ptr,
                        I16_LUMA_AC,
                        coeff_ptr,
                        *iLumaQp as u8,
                        pCtx, dq,
                        pDec,
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
                        let scan_ptr = g_kuiZigzagScan8x8.as_ptr().add(iScanIdxStart);
                        let res_prop = if is_intra {
                            LUMA_DC_AC_INTRA_8
                        } else {
                            LUMA_DC_AC_INTER_8
                        };
                        let coeff_ptr = scaled_tcoeff_mb.as_mut_ptr().add(iId8x8 * 64);
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
                            let scan_ptr = g_kuiZigzagScan.as_ptr().add(iScanIdxStart);
                            let coeff_ptr = scaled_tcoeff_mb.as_mut_ptr().add(iIdx * 16);
                            let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                                pNeighAvail,
                                pNonZeroCount,
                                                iIdx as i32,
                                len,
                                scan_ptr,
                                res_prop,
                                coeff_ptr,
                                *iLumaQp as u8,
                                pCtx, dq,
                                pDec,
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
                let coeff_ptr = scaled_tcoeff_mb.as_mut_ptr().add(256 + i * 64);
                let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                    pNeighAvail,
                    pNonZeroCount,
                        16 + (i as i32 * 4),
                    4,
                    g_kuiChromaDcScan.as_ptr(),
                    res_prop,
                    coeff_ptr,
                    (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                    pCtx, dq,
                    pDec,
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
                    let scan_ptr = g_kuiZigzagScan.as_ptr().add(max_idx);
                    let coeff_ptr = scaled_tcoeff_mb.as_mut_ptr().add(index * 16);
                    let ret = crate::decoder::parse_mb_syn_cabac::ParseResidualBlockCabac(
                        pNeighAvail,
                        pNonZeroCount,
                                index as i32,
                        len,
                        scan_ptr,
                        res_prop,
                        coeff_ptr,
                        (*dq).grid.chroma_qp.get_mut(iMbXy)[i] as u8,
                        pCtx, dq,
                        pDec,
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
        let pps_sh = &*(pps_of(pCtx, (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id));
        for i in 0..2 {
            let idx =
                WELS_CLIP3(last_qp + pps_sh.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }
    }

    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacISliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    uiEosFlag: *mut u32,
) -> i32 {
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
        dq,
        pDec,
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
    } else if (*active_sps(pCtx)).uiChromaFormatIdc == 0
        && ((uiMbType >= 5 && uiMbType <= 12) || (uiMbType >= 17 && uiMbType <= 24))
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
    } else if uiMbType == 25 {
        ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, dq, pDec);
        if ret != ERR_NONE {
            return ret;
        }
        (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            &mut *uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        if *uiEosFlag != 0 {
            crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
                &mut *slice_bit_reader(pCtx),
            );
        }
        return ERR_NONE;
    }

    ret = WelsDecodeMbCabacIntraModeHelper(
        pCtx,
        dq,
        pDec,
        &mut sNeighAvail,
        &mut pNonZeroCount,
        pIntraPredMode.as_mut_ptr(),
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
        dq,
        pDec,
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
        &mut *uiEosFlag,
    );
    if ret != ERR_NONE {
        return ret;
    }
    if *uiEosFlag != 0 {
        crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            &mut *slice_bit_reader(pCtx),
        );
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacISlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    let ret = WelsDecodeMbCabacISliceBaseMode0(pCtx, dq, pDec, pRefs, uiEosFlag);
    if ret != ERR_NONE {
        return ret;
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacPSliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNeighAvail: *mut SWelsNeighAvail,
    uiEosFlag: *mut u32,
) -> i32 {
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
        *(*pDec).pMbType.get_mut(iMbXy) = g_ksInterPMbTypeInfo[uiMbType as usize].iType;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInterCabac(
            pNeighAvail,
            &mut pNonZeroCount,
            &mut pMotionVector,
            &mut pMvdCache,
            &mut pRefIndex,
            dq,
            pDec,
        );
        ret = crate::decoder::parse_mb_syn_cabac::ParseInterPMotionInfoCabac(
            pCtx, dq,
            pDec,
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
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0
            && ((intra_type >= 5 && intra_type <= 12) || (intra_type >= 17 && intra_type <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }
        if intra_type == 25 {
            ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, dq, pDec);
            if ret != ERR_NONE {
                return ret;
            }
            (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
            ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                pCtx,
                &mut *uiEosFlag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            if *uiEosFlag != 0 {
                crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                    std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
                    &mut *slice_bit_reader(pCtx),
                );
            }
            return ERR_NONE;
        }

        ret = WelsDecodeMbCabacIntraModeHelper(
            pCtx,
            dq,
            pDec,
            pNeighAvail,
            &mut pNonZeroCount,
            pIntraPredMode.as_mut_ptr(),
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
        dq,
        pDec,
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
        &mut *uiEosFlag,
    );
    if ret != ERR_NONE {
        return ret;
    }
    if *uiEosFlag != 0 {
        crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            &mut *slice_bit_reader(pCtx),
        );
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacPSlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
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
        dq,
        pDec,
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
        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_SKIP;
        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);
        let ref_slice = (*pDec).pRefIndex[LIST_0].get_mut(iMbXy);
        ref_slice.fill(0);

        let bIsPending = crate::decoder::decoder_core::GetThreadCount(pCtx) > 1;
        let ppRefPic0 = pRefs.get(ref_id(pCtx, LIST_0, 0));
        let is_complete0 = if !ppRefPic0.is_null() {
            (*ppRefPic0).bIsComplete || bIsPending
        } else {
            false
        };
        (*pCtx).bMbRefConcealed =
            (*pCtx).bRPLRError || (*pCtx).bMbRefConcealed || !is_complete0;

        crate::decoder::mv_pred::PredPSkipMvFromNeighbor(dq, pDec, &mut pMv);
        let mv_slice = (*pDec).pMv[LIST_0].get_mut(iMbXy);
        let mvd_slice = (*dq).grid.mvd[LIST_0].get_mut(iMbXy);
        for i in 0..16 {
            mv_slice[i] = pMv;
            mvd_slice[i] = [0, 0];
        }

        let last_qp = (*dq).sLayerInfo.sSliceInLayer.iLastMbQp;
        *(*dq).grid.luma_qp.get_mut(iMbXy) = last_qp as i8;
        let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id));
        for i in 0..2 {
            let idx =
                WELS_CLIP3(last_qp + pps.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;

        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            &mut *uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        return ERR_NONE;
    }

    WelsDecodeMbCabacPSliceBaseMode0(pCtx, dq, pDec, pRefs, &mut sNeighAvail, uiEosFlag)
}

pub unsafe extern "C" fn WelsDecodeMbCabacBSliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNeighAvail: *mut SWelsNeighAvail,
    uiEosFlag: *mut u32,
) -> i32 {
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
        *(*pDec).pMbType.get_mut(iMbXy) = g_ksInterBMbTypeInfo[uiMbType as usize].iType;
        crate::decoder::parse_mb_syn_cavlc::WelsFillCacheInterCabac(
            pNeighAvail,
            &mut pNonZeroCount,
            &mut pMotionVector,
            &mut pMvdCache,
            &mut pRefIndex,
            dq,
            pDec,
        );
        crate::decoder::parse_mb_syn_cavlc::WelsFillDirectCacheCabac(
            pNeighAvail,
            &mut pDirect,
            dq,
        );
        ret = crate::decoder::parse_mb_syn_cabac::ParseInterBMotionInfoCabac(
            pCtx, dq,
            pDec,
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
        if (*active_sps(pCtx)).uiChromaFormatIdc == 0
            && ((intra_type >= 5 && intra_type <= 12) || (intra_type >= 17 && intra_type <= 24))
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_MB_TYPE);
        }
        if intra_type == 25 {
            ret = crate::decoder::parse_mb_syn_cabac::ParseIPCMInfoCabac(pCtx, dq, pDec);
            if ret != ERR_NONE {
                return ret;
            }
            (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;
            ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
                pCtx,
                &mut *uiEosFlag,
            );
            if ret != ERR_NONE {
                return ret;
            }
            if *uiEosFlag != 0 {
                crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
                    std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
                    &mut *slice_bit_reader(pCtx),
                );
            }
            return ERR_NONE;
        }

        ret = WelsDecodeMbCabacIntraModeHelper(
            pCtx,
            dq,
            pDec,
            pNeighAvail,
            &mut pNonZeroCount,
            pIntraPredMode.as_mut_ptr(),
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
        dq,
        pDec,
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
        &mut *uiEosFlag,
    );
    if ret != ERR_NONE {
        return ret;
    }
    if *uiEosFlag != 0 {
        crate::decoder::cabac_decoder::RestoreCabacDecEngineToBS(
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            &mut *slice_bit_reader(pCtx),
        );
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacBSlice(
    pCtx: *mut SWelsDecoderContext,
    dq: *mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt.sSliceHeader);
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
        dq,
        pDec,
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

    let bIsPending = crate::decoder::decoder_core::GetThreadCount(pCtx) > 1;

    if uiCode != 0 {
        let mut pMv = [[0i16; 2]; 2];
        let mut ref_idx = [0i8; 2];
        let mut subMbType = 0u32;

        *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_SKIP | MB_TYPE_DIRECT;
        let nzc_mb = (*dq).grid.nzc.get_mut(iMbXy);
        nzc_mb.fill(0);
        let ref0_slice = (*pDec).pRefIndex[LIST_0].get_mut(iMbXy);
        let ref1_slice = (*pDec).pRefIndex[LIST_1].get_mut(iMbXy);
        ref0_slice.fill(0);
        ref1_slice.fill(0);

        let ppRefPic0 = pRefs.get(ref_id(pCtx, LIST_0, 0));
        let ppRefPic1 = pRefs.get(ref_id(pCtx, LIST_1, 0));
        let is_complete0 = if !ppRefPic0.is_null() {
            (*ppRefPic0).bIsComplete || bIsPending
        } else {
            false
        };
        let is_complete1 = if !ppRefPic1.is_null() {
            (*ppRefPic1).bIsComplete || bIsPending
        } else {
            false
        };
        (*pCtx).bMbRefConcealed =
            (*pCtx).bRPLRError || (*pCtx).bMbRefConcealed || !is_complete0 || !is_complete1;

        if (*pCtx).bMbRefConcealed {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
        }

        if (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag != 0 {
            ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                pCtx, dq,
                pDec,
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
                pCtx, dq,
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
        let pps = &*(pps_of(pCtx, (*dq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pps_id));
        for i in 0..2 {
            let idx =
                WELS_CLIP3(last_qp + pps.iChromaQpIndexOffset[i] as i32, 0, 51);
            (*dq).grid.chroma_qp.get_mut(iMbXy)[i] = g_kuiChromaQpTable[idx as usize] as i8;
        }

        (*dq).sLayerInfo.sSliceInLayer.iLastDeltaQp = 0;

        ret = crate::decoder::parse_mb_syn_cabac::ParseEndOfSliceCabac(
            pCtx,
            &mut *uiEosFlag,
        );
        if ret != ERR_NONE {
            return ret;
        }
        return ERR_NONE;
    }

    WelsDecodeMbCabacBSliceBaseMode0(pCtx, dq, pDec, pRefs, &mut sNeighAvail, uiEosFlag)
}

// ============================================================================
// Top-Level Slice Decoding Orchestrators
// ============================================================================

pub unsafe fn WelsDecodeSlice(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut DqLayerState,
    bFirstSliceInLayer: bool,
    pNalCur: *mut SNalUnit,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
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
    let pSlice = std::ptr::addr_of_mut!((*pCurDqLayer).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt.sSliceHeader);

    (*pSlice).iTotalMbInCurSlice = 0;

    // The parse-only slice bracket, the same one borrow as the decode one below.
    let (pDec, pRefs) = cur_and_refs(pCtx);

    let pDecMbFunc: PWelsDecMbFunc = if !active_pps(pCtx).is_null()
        && (*active_pps(pCtx)).bEntropyCodingModeFlag
    {
        if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCabacPSlice
        } else if (*pSliceHeader).eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCabacBSlice
        } else {
            WelsDecodeMbCabacISlice
        }
    } else {
        if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCavlcPSlice
        } else if (*pSliceHeader).eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCavlcBSlice
        } else {
            WelsDecodeMbCavlcISlice
        }
    };

    // `pSliceHeader->pPps` in decode_slice.cpp; the slice header stores it opaquely.
    // T4b.3: the `if` that used to fill three laundered slots *is* the assignment
    // now. A null PPS keeps the `Constrain0` arm the old `else` gave it.
    let pPpsForIntra = pps_of(pCtx, (*pSliceHeader).pps_id);
    (*pCtx).eIntraPredConstraint = IntraPredConstraint::from_flag(
        !pPpsForIntra.is_null() && (*pPpsForIntra).bConstainedIntraPredFlag,
    );

    (*pCtx).eSliceType = (*pSliceHeader).eSliceType;
    let pPpsLayer = pps_of(pCtx, (*pCurDqLayer).sLayerInfo.pps_id);
    if !pPpsLayer.is_null() && (*pPpsLayer).bEntropyCodingModeFlag {
        let iQp = (*pSliceHeader).iSliceQp;
        let iCabacInitIdc = (*pSliceHeader).iCabacInitIdc;
        crate::decoder::cabac_decoder::WelsCabacContextInit(
            pCtx,
            (*pSlice).eSliceType,
            iCabacInitIdc,
            iQp,
        );
        (*pSlice).iLastDeltaQp = 0;
        let err = crate::decoder::cabac_decoder::InitCabacDecEngineFromBS(
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            &mut *slice_bit_reader(pCtx),
            &(*pCtx).sRawData,
        );
        if err != ERR_NONE {
            return err;
        }
    }
    WelsCalcDeqCoeffScalingList(pCtx);

    let mut iNextMbXyIndex = (*pSliceHeader).iFirstMbInSlice;
    if (*pCurDqLayer).iMbWidth > 0 {
        (*pCurDqLayer).iMbX = iNextMbXyIndex % (*pCurDqLayer).iMbWidth;
        (*pCurDqLayer).iMbY = iNextMbXyIndex / (*pCurDqLayer).iMbWidth;
    }
    (*pCurDqLayer).iMbXyIndex = iNextMbXyIndex;
    (*pSlice).iMbSkipRun = -1;
    let iSliceIdc = ((*pSliceHeader).iFirstMbInSlice << 7) + (*pCurDqLayer).uiLayerDqId as i32;

    let kiCountNumMb = if !(*pSliceHeader).sps_ref.is_none() {
        (*(sps_of(pCtx, (*pSliceHeader).sps_ref))).uiTotalMbCount as i32
    } else {
        0
    };

    let mut uiEosFlag: u32 = 0;

    loop {
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
            break;
        }

        *(*pCurDqLayer).grid.slice_idc.get_mut(iNextMbXyIndex as usize) = iSliceIdc;
        (*pCtx).bMbRefConcealed = false;
        let iRet = pDecMbFunc(pCtx, pCurDqLayer, pDec, pRefs, pNalCur, &mut uiEosFlag);
        *(*pCurDqLayer).grid.mb_ref_concealed_flag.get_mut(iNextMbXyIndex as usize) =
            (*pCtx).bMbRefConcealed;
        if iRet != ERR_NONE {
            return iRet;
        }

        (*pSlice).iTotalMbInCurSlice += 1;
        if uiEosFlag != 0 {
            break;
        }

        if !active_pps(pCtx).is_null() && (*active_pps(pCtx)).uiNumSliceGroups > 1 {
            iNextMbXyIndex = crate::decoder::fmo::FmoNextMb(active_fmo(pCtx), iNextMbXyIndex);
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

pub unsafe fn WelsDecodeAndConstructSlice(pCtx: *mut SWelsDecoderContext, pCurDqLayer: *mut DqLayerState) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let pNalCur = (*pCtx).pNalCur;
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq: *mut DqLayerState = pCurDqLayer;
    // **The slice bracket** (T5.P″3, split at T5.Q2): the pool is borrowed **once**
    // here — the picture being written as the `&mut` half, every other slot as the
    // view each reference resolution below goes through — and not at all under this
    // loop. The two lines that stood here were two derivations; `mut_and_rest` makes
    // them one, and everything below was already shaped for it.
    let (pDec, pRefs) = cur_and_refs(pCtx);
    let pSlice: *mut SSlice = std::ptr::addr_of_mut!((*dq).sLayerInfo.sSliceInLayer);
    let pSliceHeader = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt.sSliceHeader);

    (*pSlice).iTotalMbInCurSlice = 0;

    let pDecMbFunc: PWelsDecMbFunc = if !active_pps(pCtx).is_null() && (*active_pps(pCtx)).bEntropyCodingModeFlag {
        if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCabacPSlice
        } else if (*pSliceHeader).eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCabacBSlice
        } else {
            WelsDecodeMbCabacISlice
        }
    } else {
        if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCavlcPSlice
        } else if (*pSliceHeader).eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCavlcBSlice
        } else {
            WelsDecodeMbCavlcISlice
        }
    };

    // `pSliceHeader->pPps` in decode_slice.cpp; the slice header stores it opaquely.
    // T4b.3: the `if` that used to fill three laundered slots *is* the assignment
    // now. A null PPS keeps the `Constrain0` arm the old `else` gave it.
    let pPpsForIntra = pps_of(pCtx, (*pSliceHeader).pps_id);
    (*pCtx).eIntraPredConstraint = IntraPredConstraint::from_flag(
        !pPpsForIntra.is_null() && (*pPpsForIntra).bConstainedIntraPredFlag,
    );

    (*pCtx).eSliceType = (*pSliceHeader).eSliceType;
    WelsCalcDeqCoeffScalingList(pCtx);

    let mut iNextMbXyIndex = (*pSliceHeader).iFirstMbInSlice;
    if (*dq).iMbWidth > 0 {
        (*dq).iMbX = iNextMbXyIndex % (*dq).iMbWidth;
        (*dq).iMbY = iNextMbXyIndex / (*dq).iMbWidth;
    }
    (*dq).iMbXyIndex = iNextMbXyIndex;

    let kiCountNumMb = if !(*pSliceHeader).sps_ref.is_none() {
        (*(sps_of(pCtx, (*pSliceHeader).sps_ref))).uiTotalMbCount as i32
    } else {
        0
    };

    let mut uiEosFlag: u32 = 0;

    loop {
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
            break;
        }

        (*pCtx).bMbRefConcealed = false;
        let iRet = pDecMbFunc(pCtx, dq, pDec, pRefs, pNalCur, &mut uiEosFlag);
        *(*dq).grid.mb_ref_concealed_flag.get_mut(iNextMbXyIndex as usize) = (*pCtx).bMbRefConcealed;
        if iRet != ERR_NONE {
            return iRet;
        }

        let ret = WelsTargetMbConstruction(pCtx, dq, pDec, pRefs);
        if ret != ERR_NONE {
            return ERR_INFO_MB_RECON_FAIL;
        }

        let idx = iNextMbXyIndex as usize;
        if !*(*dq).grid.mb_correctly_decoded_flag.get(idx) {
            *(*dq).grid.mb_correctly_decoded_flag.get_mut(idx) = true;
            if *(*dq).grid.mb_ref_concealed_flag.get(idx) {
                if !pDec.is_null() {
                    (*pDec).iMbEcedPropNum += 1;
                }
            }
            (*pCtx).iTotalNumMbRec += 1;
        }

        (*pSlice).iTotalMbInCurSlice += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wels_mb_intra_prediction_construction_null_ptrs() {
        unsafe {
            let res = WelsMbIntraPredictionConstruction(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                true,
            );
            assert_eq!(res, ERR_NONE);
        }
    }

    #[test]
    fn test_wels_target_mb_construction_null_ptrs() {
        unsafe {
            let res = WelsTargetMbConstruction(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                PicRefs::over(None),
            );
            assert_eq!(res, ERR_NONE);
        }
    }

    #[test]
    fn test_wels_target_slice_construction_null_ptrs() {
        unsafe {
            let res = WelsTargetSliceConstruction(std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(res, ERR_NONE);
        }
    }

    #[test]
    fn test_wels_calc_deq_coeff_scaling_list() {
        unsafe {
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

    #[test]
    fn test_wels_decode_mb_cavlc_slices_null() {
        unsafe {
            let res_i = WelsDecodeMbCavlcISlice(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                PicRefs::over(None),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_eq!(res_i, ERR_NONE);
            let res_p = WelsDecodeMbCavlcPSlice(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                PicRefs::over(None),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_eq!(res_p, ERR_NONE);
        }
    }

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
        const GRID_48X32: &[u8] = include_bytes!("../../../../../res/grid_48x32.264");
        let (frames, dims, _) = drive_decoder_over(GRID_48X32);
        assert!(
            frames > 0,
            "no frame came out of grid_48x32.264 — the slice loop was never entered, \
             so this test is not measuring what it claims to"
        );
        assert_eq!(
            dims,
            Some((48, 32)),
            "grid_48x32.264 must decode as a 3x2 macroblock grid; a stream without \
             neighbours covers nothing this test exists for"
        );
    }

    /// Decodes `stream` through the C ABI and returns `(frames out, last frame's
    /// dimensions)`.
    ///
    /// **It calls the vtable thunks through the raw pointer, not the `&mut self`
    /// convenience methods, and that is deliberate — see F23.** The first draft
    /// used `(*p_decoder).Initialize(&param)`, and Miri rejected it before the
    /// decoder was even initialized: `ISVCDecoder::Initialize` takes `&mut self`
    /// over a struct that is **one pointer wide**, and the thunk immediately casts
    /// that to `*mut CWelsDecoderImpl` and writes at offset `0x20` — outside the
    /// eight bytes the borrow covers. That is a real defect on the public API path
    /// and it is not this phase's (`api/codec_api.rs` is T10/§2.2.8, Phase 8's);
    /// spelling the call the way a C caller does keeps these tests measuring the
    /// decoder instead of the ABI shim. `WelsCreateDecoder` hands out
    /// `Box::into_raw(dec) as *mut ISVCDecoder`, which carries provenance for the
    /// whole implementation object, so the raw-pointer spelling is sound.
    /// Returns `(frames, dims, states)`, where `states` is the bitwise OR of every
    /// `DecodeFrame2` return. The third element exists for T5.S1's probes: a
    /// concealment path that does not run looks exactly like one that runs and
    /// changes nothing, and `dsDataErrorConcealed` in the OR is the difference.
    fn drive_decoder_over(stream: &[u8]) -> (usize, Option<(i32, i32)>, i32) {
        use crate::api::codec_api::*;

        unsafe {
            let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
            assert_eq!(
                i64::from(WelsCreateDecoder(&mut p_decoder)),
                CM_RESULT_SUCCESS as i64
            );
            assert!(!p_decoder.is_null());
            let vtbl = (*p_decoder).lpVtbl;

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;
            dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
            dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
            assert_eq!(
                i64::from(((*vtbl).Initialize)(p_decoder, &dec_param as *const SDecodingParam)),
                CM_RESULT_SUCCESS as i64
            );

            let mut frames = 0;
            let mut dims = None;
            let mut states = 0i32;
            for unit in crate::split_annexb_units(stream) {
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let ret = ((*vtbl).DecodeFrame2)(
                    p_decoder,
                    unit.as_ptr(),
                    unit.len() as i32,
                    p_dst.as_mut_ptr(),
                    &mut buf_info,
                );
                states |= ret.0;
                if buf_info.iBufferStatus == 1 {
                    frames += 1;
                    let sys = buf_info.UsrData.sSystemBuffer;
                    dims = Some((sys.iWidth, sys.iHeight));
                }
            }

            // End of stream, then the zero-length call that flushes it — the same
            // tail `decoder_conformance_test.rs` and `malformed_stream_parity.rs`
            // use. T5.S1 added it: without it this helper never drove the flush
            // path at all, and a stream whose only frame arrives there (the FMO
            // asset, any truncated stream) looked to it like a stream that decodes
            // nothing. It cannot cost the two probes above a verdict — they assert
            // `frames > 0` and the dimensions, and a flush only ever adds frames.
            let mut eos_flag = 1i32;
            ((*vtbl).SetOption)(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
            );
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ((*vtbl).DecodeFrame2)(
                p_decoder,
                std::ptr::null(),
                0,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            states |= ret.0;
            if buf_info.iBufferStatus == 1 {
                frames += 1;
                let sys = buf_info.UsrData.sSystemBuffer;
                dims = Some((sys.iWidth, sys.iHeight));
            }

            // …and the drain the flush announces. Leaving it out cost a frame on
            // every stream whose last picture is still buffered at EOS, which read
            // as the port being one frame short of the C++ until the helper was
            // compared against `rust/tools/ecref` rather than against itself.
            let mut remaining = 0i32;
            ((*vtbl).GetOption)(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                &mut remaining as *mut i32 as *mut std::ffi::c_void,
            );
            for _ in 0..remaining.clamp(0, 24) {
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let ret = ((*vtbl).FlushFrame)(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
                states |= ret.0;
                if buf_info.iBufferStatus == 1 {
                    frames += 1;
                    let sys = buf_info.UsrData.sSystemBuffer;
                    dims = Some((sys.iWidth, sys.iHeight));
                }
            }

            ((*vtbl).Uninitialize)(p_decoder);
            WelsDestroyDecoder(p_decoder);
            (frames, dims, states)
        }
    }

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
        const FMO: &[u8] = include_bytes!("../../../../../res/fmo_2groups_64x64.264");
        let (frames, dims, _) = drive_decoder_over(FMO);
        assert_eq!(frames, 1, "fmo_2groups_64x64.264 is one frame");
        assert_eq!(dims, Some((64, 64)));
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_NEON, WELS_CPU_SSE2};
pub use crate::decoder::dec_golomb::{g_kuiIntra4x4CbpTable, g_kuiIntra4x4CbpTable400};
