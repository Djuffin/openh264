#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut
)]

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

pub const dsBitstreamError: i32 = 0x02;

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
pub const WELS_CPU_SSE2: i32 = 0x00000004;
pub const WELS_CPU_NEON: i32 = 0x00000080;

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

pub static g_kuiCache48CountScan4Idx: [u8; 16] = [
    9, 10, 17, 18,
    11, 12, 19, 20,
    25, 26, 33, 34,
    27, 28, 35, 36,
];

pub static g_kuiMbCountScan4Idx: [u8; 16] = [
    0, 1, 4, 5,
    2, 3, 6, 7,
    8, 9, 12, 13,
    10, 11, 14, 15,
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

pub static g_kuiIntra4x4CbpTable: [u8; 48] = [
    47, 31, 15, 0, 23, 27, 29, 30, 7, 11, 13, 14, 39, 43, 45, 46,
    16, 3, 5, 10, 12, 19, 21, 26, 28, 35, 37, 42, 44, 1, 2, 4,
    8, 17, 18, 20, 24, 6, 9, 22, 25, 32, 33, 34, 36, 40, 38, 41,
];

pub static g_kuiIntra4x4CbpTable400: [u8; 16] = [
    15, 0, 7, 11, 13, 14, 3, 5, 10, 12, 1, 2, 4, 8, 6, 9,
];

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

// ============================================================================
// Function Pointer Types & Block Structures
// ============================================================================

pub type PWelsNonZeroCountFunc = unsafe extern "C" fn(pNonZeroCount: *mut i8);
pub type PWelsBlockZeroFunc = unsafe extern "C" fn(block: *mut i16, stride: i32);
pub type PWelsDecMbFunc = unsafe extern "C" fn(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32;

pub type PFillInfoCacheIntraNxNFunc = unsafe extern "C" fn(
    pNeighAvail: *mut SWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pIntraPredMode: *mut i8,
    pCurDqLayer: *mut SDqLayer,
);

pub type PMapNxNNeighToSampleFunc = unsafe extern "C" fn(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut i32,
);

pub type PMap16x16NeighToSampleFunc = unsafe extern "C" fn(
    pNeighAvail: *mut SWelsNeighAvail,
    pSampleAvail: *mut u8,
);

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

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBlockFunc {
    pub pWelsSetNonZeroCountFunc: Option<PWelsNonZeroCountFunc>,
    pub pWelsBlockZero16x16Func: Option<PWelsBlockZeroFunc>,
    pub pWelsBlockZero8x8Func: Option<PWelsBlockZeroFunc>,
}

impl Default for SBlockFunc {
    fn default() -> Self {
        Self {
            pWelsSetNonZeroCountFunc: None,
            pWelsBlockZero16x16Func: None,
            pWelsBlockZero8x8Func: None,
        }
    }
}

// ============================================================================
// Core Decoder Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsNeighAvail {
    pub iLeftAvail: i32,
    pub iTopAvail: i32,
    pub iLeftTopAvail: i32,
    pub iRightTopAvail: i32,
    pub iLeftType: u32,
    pub iTopType: u32,
    pub iLeftTopType: u32,
    pub iRightTopType: u32,
}

pub use crate::decoder::bit_stream::SBitStringAux;
pub use crate::decoder::parameter_sets::{SSps, SPps};
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SSlice, EWelsSliceType};

pub use crate::decoder::decoder_core::{SDqLayer, PDqLayer, SLayerInfo};
pub use crate::decoder::nalu::{SNalUnit, PNalUnit};




#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SNalUnitHeaderExt {
    pub uiQualityId: u8,
}



pub use crate::decoder::picture::{SPicture, PPicture};





#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefPic {
    pub uiShortRefCount: [i32; 2],
    pub uiLongRefCount: [i32; 2],
    pub pShortRefList: [*mut *mut SPicture; 2],
    pub pLongRefList: [*mut *mut SPicture; 2],
    pub pRefList: [*mut *mut SPicture; 2],
}

impl Default for SRefPic {
    fn default() -> Self {
        Self {
            uiShortRefCount: [0; 2],
            uiLongRefCount: [0; 2],
            pShortRefList: [std::ptr::null_mut(); 2],
            pLongRefList: [std::ptr::null_mut(); 2],
            pRefList: [std::ptr::null_mut(); 2],
        }
    }
}

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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLogContext {
    pub pLogCtx: *mut c_void,
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
            if pShortList[refIdx as usize].is_null() {
                return false;
            }
        }
        let longRefCount = ctx.sRefPic.uiLongRefCount[list];
        let pLongList = &ctx.sRefPic.pLongRefList[list];
        for refIdx in 0..longRefCount {
            if pLongList[refIdx as usize].is_null() {
                return false;
            }
        }
    }
    true
}

pub unsafe fn ComputeColocatedTemporalScaling(pCtx: *mut SWelsDecoderContext) -> bool {
    if pCtx.is_null() {
        return false;
    }
    let ctx = &mut *pCtx;
    let pCurDqLayer = ctx.pCurDqLayer;
    if pCurDqLayer.is_null() {
        return false;
    }
    let pCurSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pCurSlice.sSliceHeaderExt.sSliceHeader;

    if pSliceHeader.iDirectSpatialMvPredFlag == 0 {
        let uiRefCount = pSliceHeader.uiRefCount[LIST_0];
        let pRefList1 = &ctx.sRefPic.pRefList[LIST_1];
        let pRefList0 = &ctx.sRefPic.pRefList[LIST_0];
        if !pRefList1[0].is_null() {
            for i in 0..uiRefCount {
                if !pRefList0[i as usize].is_null() {
                    let poc0 = (*pRefList0[i as usize]).iFramePoc;
                    let poc1 = (*pRefList1[0]).iFramePoc;
                    let poc = pSliceHeader.iPicOrderCntLsb;
                    let td = WELS_CLIP3(poc1 - poc0, -128, 127);
                    if td == 0 {
                        pCurSlice.iMvScale[LIST_0][i as usize] = 1 << 8;
                    } else {
                        let tb = WELS_CLIP3(poc - poc0, -128, 127);
                        let tx = (16384 + (td.abs() >> 1)) / td;
                        pCurSlice.iMvScale[LIST_0][i as usize] =
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
    let ctx = &mut *pCtx;
    if ctx.pSps.is_null() || ctx.pPps.is_null() {
        return ERR_NONE;
    }
    if (*ctx.pSps).bSeqScalingMatrixPresentFlag || (*ctx.pPps).bPicScalingMatrixPresentFlag {
        ctx.bUseScalingList = true;
    } else {
        ctx.bUseScalingList = false;
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
// SIMD Block Zeroing & NonZero Count Functions
// ============================================================================

pub unsafe fn WelsBlockInit(pBlock: *mut i16, iW: i32, iH: i32, iStride: i32, uiVal: u8) {
    if pBlock.is_null() {
        return;
    }
    for i in 0..iH {
        std::ptr::write_bytes(
            pBlock.offset((i * iStride) as isize) as *mut u8,
            uiVal,
            (iW as usize) * std::mem::size_of::<i16>(),
        );
    }
}

pub unsafe extern "C" fn WelsBlockZero16x16_c(pBlock: *mut i16, iStride: i32) {
    WelsBlockInit(pBlock, 16, 16, iStride, 0);
}

pub unsafe extern "C" fn WelsBlockZero8x8_c(pBlock: *mut i16, iStride: i32) {
    WelsBlockInit(pBlock, 8, 8, iStride, 0);
}

pub unsafe extern "C" fn WelsNonZeroCount_c(pNonZeroCount: *mut i8) {
    if pNonZeroCount.is_null() {
        return;
    }
    for i in 0..24 {
        if *pNonZeroCount.add(i) != 0 {
            *pNonZeroCount.add(i) = 1;
        }
    }
}

pub unsafe fn WelsBlockFuncInit(pFunc: *mut SBlockFunc, iCpu: i32) {
    if pFunc.is_null() {
        return;
    }
    (*pFunc).pWelsSetNonZeroCountFunc = Some(WelsNonZeroCount_c);
    (*pFunc).pWelsBlockZero16x16Func = Some(WelsBlockZero16x16_c);
    (*pFunc).pWelsBlockZero8x8Func = Some(WelsBlockZero8x8_c);
}

// ============================================================================
// Macroblock Reconstruction Functions
// ============================================================================

pub unsafe fn WelsMbInterSampleConstruction(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut SDqLayer,
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
    let dq = &*pCurDqLayer;
    let iMbXy = dq.iMbXyIndex as usize;

    let pTransformSize8x8 = *dq.pTransformSize8x8Flag.add(iMbXy);
    let pNzc = *dq.pNzc.add(iMbXy);
    let pScaledTCoeff = dq.pScaledTCoeff.add(iMbXy) as *mut i16;


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

pub unsafe fn WelsMbInterConstruction(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut SDqLayer,
) -> i32 {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let ctx = &mut *pCtx;
    let dq = &mut *pCurDqLayer;
    let iMbX = dq.iMbX;
    let iMbY = dq.iMbY;

    if dq.pDec.is_null() {
        return ERR_NONE;
    }
    let pDec = &mut *dq.pDec;
    let iLumaStride = pDec.iLinesize[0];
    let iChromaStride = pDec.iLinesize[1];

    let pDstY = pDec.pData[0].offset(((iMbY * iLumaStride + iMbX) << 4) as isize);
    let pDstCb = pDec.pData[1].offset(((iMbY * iChromaStride + iMbX) << 3) as isize);
    let pDstCr = pDec.pData[2].offset(((iMbY * iChromaStride + iMbX) << 3) as isize);

    WelsMbInterSampleConstruction(pCtx, pCurDqLayer, pDstY, pDstCb, pDstCr, iLumaStride, iChromaStride);

    if let Some(nzc_func) = ctx.sBlockFunc.pWelsSetNonZeroCountFunc {
        nzc_func((*dq.pNzc.add(dq.iMbXyIndex as usize)).as_mut_ptr());
    }

    ERR_NONE
}

pub unsafe fn WelsMbInterPrediction(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut SDqLayer,
) -> i32 {
    ERR_NONE
}

pub unsafe fn WelsMbIntraPredictionConstruction(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: *mut SDqLayer,
    bOutput: bool,
) -> i32 {
    ERR_NONE
}

pub unsafe fn WelsTargetMbConstruction(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ctx = &mut *pCtx;
    let pCurDqLayer = ctx.pCurDqLayer;
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq = &mut *pCurDqLayer;
    let iMbXy = dq.iMbXyIndex as usize;

    if dq.pDec.is_null() || (*dq.pDec).pMbType.is_null() {
        return ERR_NONE;
    }
    let mb_type = *(*dq.pDec).pMbType.add(iMbXy);

    if mb_type == MB_TYPE_INTRA_PCM {
        ERR_NONE
    } else if IS_INTRA(mb_type) {
        WelsMbIntraPredictionConstruction(pCtx, pCurDqLayer, true)
    } else if IS_INTER(mb_type) {
        let cbp = *dq.pCbp.add(iMbXy);
        if cbp == 0 {
            if !CheckRefPics(pCtx) {
                return ERR_INFO_MB_RECON_FAIL;
            }
            WelsMbInterPrediction(pCtx, pCurDqLayer)
        } else {
            WelsMbInterConstruction(pCtx, pCurDqLayer)
        }
    } else {
        ERR_INFO_MB_RECON_FAIL
    }
}

pub unsafe fn WelsTargetSliceConstruction(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ctx = &mut *pCtx;
    let pCurDqLayer = ctx.pCurDqLayer;
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq = &mut *pCurDqLayer;
    let pCurSlice = &mut dq.sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pCurSlice.sSliceHeaderExt.sSliceHeader;

    if pSliceHeader.pSps.is_null() {
        return ERR_NONE;
    }
    let iTotalMbTargetLayer = (*(pSliceHeader.pSps as *mut SSps)).uiTotalMbCount as i32;


    let iCurLayerWidth = dq.iMbWidth << 4;
    let iCurLayerHeight = dq.iMbHeight << 4;

    let mut iNextMbXyIndex = pSliceHeader.iFirstMbInSlice;
    let iTotalNumMb = pCurSlice.iTotalMbInCurSlice;
    let mut iCountNumMb = 0;

    if !ctx.sSpsPpsCtx.bAvcBasedFlag && iCurLayerWidth != ctx.iCurSeqIntervalMaxPicWidth {
        return ERR_INFO_WIDTH_MISMATCH;
    }

    if dq.iMbWidth > 0 {
        dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
        dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
    }
    dq.iMbXyIndex = iNextMbXyIndex;

    loop {
        if iCountNumMb >= iTotalNumMb {
            break;
        }

        let bParseOnly = if !ctx.pParam.is_null() { (*ctx.pParam).bParseOnly } else { false };
        if !bParseOnly {
            let ret = WelsTargetMbConstruction(pCtx);
            if ret != ERR_NONE {
                return ERR_INFO_MB_RECON_FAIL;
            }
        }

        iCountNumMb += 1;
        let idx = iNextMbXyIndex as usize;
        if !dq.pMbCorrectlyDecodedFlag.is_null() && !*dq.pMbCorrectlyDecodedFlag.add(idx) {
            *dq.pMbCorrectlyDecodedFlag.add(idx) = true;
            if !dq.pMbRefConcealedFlag.is_null() && *dq.pMbRefConcealedFlag.add(idx) {
                if !dq.pDec.is_null() {
                    (*dq.pDec).iMbEcedPropNum += 1;
                }
            }
            ctx.iTotalNumMbRec += 1;
        }

        if ctx.iTotalNumMbRec > iTotalMbTargetLayer {
            return ERR_INFO_MB_NUM_EXCEED_FAIL;
        }

        iNextMbXyIndex += 1;
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= iTotalMbTargetLayer {
            break;
        }
        if dq.iMbWidth > 0 {
            dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
            dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
        }
        dq.iMbXyIndex = iNextMbXyIndex;
    }

    if !dq.pDec.is_null() {
        (*dq.pDec).iWidthInPixel = iCurLayerWidth;
        (*dq.pDec).iHeightInPixel = iCurLayerHeight;
    }

    ERR_NONE
}

// ============================================================================
// Entropy Slice Decoding (CAVLC / CABAC Dispatch)
// ============================================================================

pub unsafe extern "C" fn WelsActualDecodeMbCavlcISlice(pCtx: *mut SWelsDecoderContext) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCavlcISlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ret = WelsActualDecodeMbCavlcISlice(pCtx);
    if ret != ERR_NONE {
        return ret;
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsActualDecodeMbCavlcPSlice(pCtx: *mut SWelsDecoderContext) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCavlcPSlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ret = WelsActualDecodeMbCavlcPSlice(pCtx);
    if ret != ERR_NONE {
        return ret;
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsActualDecodeMbCavlcBSlice(pCtx: *mut SWelsDecoderContext) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCavlcBSlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ret = WelsActualDecodeMbCavlcBSlice(pCtx);
    if ret != ERR_NONE {
        return ret;
    }
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacISliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    uiEosFlag: *mut u32,
) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacISlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    WelsDecodeMbCabacISliceBaseMode0(pCtx, uiEosFlag)
}

pub unsafe extern "C" fn WelsDecodeMbCabacPSliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    pNeighAvail: *mut SWelsNeighAvail,
    uiEosFlag: *mut u32,
) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacPSlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    let mut sNeighAvail = SWelsNeighAvail::default();
    WelsDecodeMbCabacPSliceBaseMode0(pCtx, &mut sNeighAvail, uiEosFlag)
}

pub unsafe extern "C" fn WelsDecodeMbCabacBSliceBaseMode0(
    pCtx: *mut SWelsDecoderContext,
    pNeighAvail: *mut SWelsNeighAvail,
    uiEosFlag: *mut u32,
) -> i32 {
    ERR_NONE
}

pub unsafe extern "C" fn WelsDecodeMbCabacBSlice(
    pCtx: *mut SWelsDecoderContext,
    pNalCur: *mut SNalUnit,
    uiEosFlag: *mut u32,
) -> i32 {
    let mut sNeighAvail = SWelsNeighAvail::default();
    WelsDecodeMbCabacBSliceBaseMode0(pCtx, &mut sNeighAvail, uiEosFlag)
}

// ============================================================================
// Top-Level Slice Decoding Orchestrators
// ============================================================================

pub unsafe fn WelsDecodeSlice(
    pCtx: *mut SWelsDecoderContext,
    bFirstSliceInLayer: bool,
    pNalCur: *mut SNalUnit,
) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ctx = &mut *pCtx;
    let pCurDqLayer = ctx.pCurDqLayer;
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq = &mut *pCurDqLayer;
    let pSlice = &mut dq.sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;

    pSlice.iTotalMbInCurSlice = 0;

    let pDecMbFunc: PWelsDecMbFunc = if !ctx.pPps.is_null() && (*ctx.pPps).bEntropyCodingModeFlag {
        if pSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCabacPSlice
        } else if pSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCabacBSlice
        } else {
            WelsDecodeMbCabacISlice
        }
    } else {
        if pSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCavlcPSlice
        } else if pSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCavlcBSlice
        } else {
            WelsDecodeMbCavlcISlice
        }
    };

    if !ctx.pPps.is_null() && (*ctx.pPps).bConstainedIntraPredFlag {
        ctx.pMapNxNNeighToSampleFunc = std::mem::transmute(WelsMapNxNNeighToSampleConstrain1 as unsafe extern "C" fn(_, _));
        ctx.pMap16x16NeighToSampleFunc = std::mem::transmute(WelsMap16x16NeighToSampleConstrain1 as unsafe extern "C" fn(_, _));
    } else {
        ctx.pMapNxNNeighToSampleFunc = std::mem::transmute(WelsMapNxNNeighToSampleNormal as unsafe extern "C" fn(_, _));
        ctx.pMap16x16NeighToSampleFunc = std::mem::transmute(WelsMap16x16NeighToSampleNormal as unsafe extern "C" fn(_, _));
    }

    ctx.eSliceType = pSliceHeader.eSliceType;
    WelsCalcDeqCoeffScalingList(pCtx);

    let mut iNextMbXyIndex = pSliceHeader.iFirstMbInSlice;
    if dq.iMbWidth > 0 {
        dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
        dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
    }
    dq.iMbXyIndex = iNextMbXyIndex;

    let kiCountNumMb = if !pSliceHeader.pSps.is_null() {
        (*(pSliceHeader.pSps as *mut SSps)).uiTotalMbCount as i32
    } else {
        0
    };

    let mut uiEosFlag: u32 = 0;

    loop {
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
            break;
        }

        ctx.bMbRefConcealed = false;
        let iRet = pDecMbFunc(pCtx, pNalCur, &mut uiEosFlag);
        if !dq.pMbRefConcealedFlag.is_null() {
            *dq.pMbRefConcealedFlag.add(iNextMbXyIndex as usize) = ctx.bMbRefConcealed;
        }
        if iRet != ERR_NONE {
            return iRet;
        }

        pSlice.iTotalMbInCurSlice += 1;
        if uiEosFlag != 0 {
            break;
        }

        iNextMbXyIndex += 1;
        if dq.iMbWidth > 0 {
            dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
            dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
        }
        dq.iMbXyIndex = iNextMbXyIndex;
    }

    ERR_NONE
}

pub unsafe fn WelsDecodeAndConstructSlice(pCtx: *mut SWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_NONE;
    }
    let ctx = &mut *pCtx;
    let pNalCur = ctx.pNalCur;
    let pCurDqLayer = ctx.pCurDqLayer;
    if pCurDqLayer.is_null() {
        return ERR_NONE;
    }
    let dq = &mut *pCurDqLayer;
    let pSlice = &mut dq.sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;

    pSlice.iTotalMbInCurSlice = 0;

    let pDecMbFunc: PWelsDecMbFunc = if !ctx.pPps.is_null() && (*ctx.pPps).bEntropyCodingModeFlag {
        if pSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCabacPSlice
        } else if pSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCabacBSlice
        } else {
            WelsDecodeMbCabacISlice
        }
    } else {
        if pSliceHeader.eSliceType == EWelsSliceType::P_SLICE {
            WelsDecodeMbCavlcPSlice
        } else if pSliceHeader.eSliceType == EWelsSliceType::B_SLICE {
            WelsDecodeMbCavlcBSlice
        } else {
            WelsDecodeMbCavlcISlice
        }
    };

    if !ctx.pPps.is_null() && (*ctx.pPps).bConstainedIntraPredFlag {
        ctx.pMapNxNNeighToSampleFunc = std::mem::transmute(WelsMapNxNNeighToSampleConstrain1 as unsafe extern "C" fn(_, _));
        ctx.pMap16x16NeighToSampleFunc = std::mem::transmute(WelsMap16x16NeighToSampleConstrain1 as unsafe extern "C" fn(_, _));
    } else {
        ctx.pMapNxNNeighToSampleFunc = std::mem::transmute(WelsMapNxNNeighToSampleNormal as unsafe extern "C" fn(_, _));
        ctx.pMap16x16NeighToSampleFunc = std::mem::transmute(WelsMap16x16NeighToSampleNormal as unsafe extern "C" fn(_, _));
    }

    ctx.eSliceType = pSliceHeader.eSliceType;
    WelsCalcDeqCoeffScalingList(pCtx);

    let mut iNextMbXyIndex = pSliceHeader.iFirstMbInSlice;
    if dq.iMbWidth > 0 {
        dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
        dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
    }
    dq.iMbXyIndex = iNextMbXyIndex;

    let kiCountNumMb = if !pSliceHeader.pSps.is_null() {
        (*(pSliceHeader.pSps as *mut SSps)).uiTotalMbCount as i32
    } else {
        0
    };

    let mut uiEosFlag: u32 = 0;

    loop {
        if iNextMbXyIndex < 0 || iNextMbXyIndex >= kiCountNumMb {
            break;
        }

        ctx.bMbRefConcealed = false;
        let iRet = pDecMbFunc(pCtx, pNalCur, &mut uiEosFlag);
        if !dq.pMbRefConcealedFlag.is_null() {
            *dq.pMbRefConcealedFlag.add(iNextMbXyIndex as usize) = ctx.bMbRefConcealed;
        }
        if iRet != ERR_NONE {
            return iRet;
        }

        let ret = WelsTargetMbConstruction(pCtx);
        if ret != ERR_NONE {
            return ERR_INFO_MB_RECON_FAIL;
        }

        let idx = iNextMbXyIndex as usize;
        if !dq.pMbCorrectlyDecodedFlag.is_null() && !*dq.pMbCorrectlyDecodedFlag.add(idx) {
            *dq.pMbCorrectlyDecodedFlag.add(idx) = true;
            if !dq.pMbRefConcealedFlag.is_null() && *dq.pMbRefConcealedFlag.add(idx) {
                if !dq.pDec.is_null() {
                    (*dq.pDec).iMbEcedPropNum += 1;
                }
            }
            ctx.iTotalNumMbRec += 1;
        }

        pSlice.iTotalMbInCurSlice += 1;
        if uiEosFlag != 0 {
            break;
        }

        iNextMbXyIndex += 1;
        if dq.iMbWidth > 0 {
            dq.iMbX = iNextMbXyIndex % dq.iMbWidth;
            dq.iMbY = iNextMbXyIndex / dq.iMbWidth;
        }
        dq.iMbXyIndex = iNextMbXyIndex;
    }

    ERR_NONE
}
