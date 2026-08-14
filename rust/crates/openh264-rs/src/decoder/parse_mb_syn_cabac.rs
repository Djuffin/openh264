// Copyright (c) 2009-2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # CABAC Macroblock Syntax Parsing Engine
//!
//! Translated from `codec/decoder/core/src/parse_mb_syn_cabac.cpp` and
//! `codec/decoder/core/inc/parse_mb_syn_cabac.h`.
//!
//! Implements the macroblock and sub-macroblock layer entropy parsing algorithms for
//! H.264 / AVC Context-Based Adaptive Binary Arithmetic Coding (CABAC), adhering strictly to
//! ISO/IEC 14496-10 (ITU-T H.264) Section 7.3.5 and Section 9.3.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use crate::decoder::decoder_context::{dec_pic, pool_pic, ref_pic};
use crate::decoder::picture::PPicture;
use std::ptr;


use super::bit_stream::InitReadBits;
use crate::safe::bits::BsCursor;
use super::cabac_decoder::{
    DecodeBinCabac, DecodeBypassCabac, DecodeTerminateCabac, DecodeUEGLevelCabac, DecodeUEGMvCabac,
    DecodeUnaryBinCabac, InitCabacDecEngineFromBS, RestoreCabacDecEngineToBS, PWelsCabacCtx,
    cabac_rbsp_window,
    cabac_ctx_base,
    PWelsCabacDecEngine, SWelsCabacCtx, SWelsCabacDecEngine,
};

// ============================================================================
// Constants, Macros & Error Codes
// ============================================================================

pub const IDX_UNUSED: i16 = -1;

pub const NEW_CTX_OFFSET_MB_TYPE_I: i32 = 3;
pub const NEW_CTX_OFFSET_SKIP: i32 = 11;
pub const NEW_CTX_OFFSET_SUBMB_TYPE: i32 = 21;
pub const NEW_CTX_OFFSET_B_SUBMB_TYPE: i32 = 36;
pub const NEW_CTX_OFFSET_MVD: i32 = 40;
pub const NEW_CTX_OFFSET_REF_NO: i32 = 54;
pub const NEW_CTX_OFFSET_DELTA_QP: i32 = 60;
pub const NEW_CTX_OFFSET_CIPR: i32 = 64;
pub const NEW_CTX_OFFSET_IPR: i32 = 68;
pub const NEW_CTX_OFFSET_CBP: i32 = 73;
pub const NEW_CTX_OFFSET_CBF: i32 = 85;
pub const NEW_CTX_OFFSET_MAP: i32 = 105;
pub const NEW_CTX_OFFSET_LAST: i32 = 166;
pub const NEW_CTX_OFFSET_ONE: i32 = 227;
pub const NEW_CTX_OFFSET_ABS: i32 = 232;
pub const NEW_CTX_OFFSET_TS_8x8_FLAG: i32 = 399;
pub const NEW_CTX_OFFSET_MAP_8x8: i32 = 402;
pub const NEW_CTX_OFFSET_LAST_8x8: i32 = 417;
pub const NEW_CTX_OFFSET_ONE_8x8: i32 = 426;
pub const NEW_CTX_OFFSET_ABS_8x8: i32 = 431;

pub const CTX_NUM_MVD: i32 = 7;
pub const CTX_NUM_CBP: i32 = 4;

pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MV_A: usize = 2;

pub const REF_NOT_AVAIL: i8 = -2;
pub const REF_NOT_IN_LIST: i8 = -1;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_SLICE_DATA: i32 = 6;
pub const ERR_LEVEL_MB_DATA: i32 = 7;

pub const ERR_INFO_INVALID_SUB_MB_TYPE: i32 = 1037;
pub const ERR_INFO_INVALID_REF_INDEX: i32 = 1040;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = 1075;
pub const ERR_CABAC_NO_BS_TO_READ: i32 = 201;

pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const ERROR_CON_DISABLE: crate::decoder::error_concealment::ERROR_CON_IDC = crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE;

#[inline(always)]
pub const fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

// Slice Types
pub const P_SLICE: u8 = 0;
pub const B_SLICE: u8 = 1;
pub const I_SLICE: u8 = 2;

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
pub const MB_TYPE_DIRECT: u32 = 0x00000800;
pub const MB_TYPE_P0L0: u32 = 0x00001000;
pub const MB_TYPE_P1L0: u32 = 0x00002000;
pub const MB_TYPE_P0L1: u32 = 0x00004000;
pub const MB_TYPE_P1L1: u32 = 0x00008000;

pub const SUB_MB_TYPE_8x8: u32 = 0x00000001;
pub const SUB_MB_TYPE_8x4: u32 = 0x00000002;
pub const SUB_MB_TYPE_4x8: u32 = 0x00000004;
pub const SUB_MB_TYPE_4x4: u32 = 0x00000008;

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

#[inline(always)]
pub const fn IS_INTRA(t: u32) -> bool {
    (t & MB_TYPE_INTRA) != 0
}
#[inline(always)]
pub const fn IS_SKIP(t: u32) -> bool {
    (t & MB_TYPE_SKIP) != 0
}
#[inline(always)]
pub const fn IS_DIRECT(t: u32) -> bool {
    (t & MB_TYPE_DIRECT) != 0
}
#[inline(always)]
pub const fn IS_INTER_16x16(t: u32) -> bool {
    (t & MB_TYPE_16x16) != 0
}
#[inline(always)]
pub const fn IS_INTER_16x8(t: u32) -> bool {
    (t & MB_TYPE_16x8) != 0
}
#[inline(always)]
pub const fn IS_INTER_8x16(t: u32) -> bool {
    (t & MB_TYPE_8x16) != 0
}
#[inline(always)]
pub const fn IS_Inter_8x8(t: u32) -> bool {
    (t & MB_TYPE_8x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x8(t: u32) -> bool {
    (t & SUB_MB_TYPE_8x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x4(t: u32) -> bool {
    (t & SUB_MB_TYPE_8x4) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x8(t: u32) -> bool {
    (t & SUB_MB_TYPE_4x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x4(t: u32) -> bool {
    (t & SUB_MB_TYPE_4x4) != 0
}
#[inline(always)]
pub const fn IS_DIR(a: u32, part: usize, list: usize) -> bool {
    (a & (MB_TYPE_P0L0 << (part + 2 * list))) != 0
}

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

// ============================================================================
// Global Lookup Tables
// ============================================================================

pub const g_kMaxPos: [i16; 11] = [IDX_UNUSED, 15, 14, 15, 3, 14, 63, 3, 3, 14, 14];
pub const g_kMaxC2: [i16; 11] = [IDX_UNUSED, 4, 4, 4, 3, 4, 4, 3, 3, 4, 4];
pub const g_kBlockCat2CtxOffsetCBF: [i16; 11] = [IDX_UNUSED, 0, 4, 8, 12, 16, 0, 12, 12, 16, 16];
pub const g_kBlockCat2CtxOffsetMap: [i16; 11] = [IDX_UNUSED, 0, 15, 29, 44, 47, 0, 44, 44, 47, 47];
pub const g_kBlockCat2CtxOffsetLast: [i16; 11] = [IDX_UNUSED, 0, 15, 29, 44, 47, 0, 44, 44, 47, 47];
pub const g_kBlockCat2CtxOffsetOne: [i16; 11] = [IDX_UNUSED, 0, 10, 20, 30, 39, 0, 30, 30, 39, 39];
pub const g_kBlockCat2CtxOffsetAbs: [i16; 11] = [IDX_UNUSED, 0, 10, 20, 30, 39, 0, 30, 30, 39, 39];

pub const g_kTopBlkInsideMb: [u8; 24] = [
    0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1,
];

pub const g_kLeftBlkInsideMb: [u8; 24] = [
    0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1,
];

pub const g_kuiIdx2CtxSignificantCoeffFlag8x8: [u8; 64] = [
    0, 1, 2, 3, 4, 5, 5, 4, 4, 3, 3, 4, 4, 4, 5, 5, 4, 4, 4, 4, 3, 3, 6, 7, 7, 7, 8, 9, 10, 9, 8,
    7, 7, 6, 11, 12, 13, 11, 6, 7, 8, 9, 14, 10, 9, 8, 6, 11, 12, 13, 11, 6, 9, 14, 10, 9, 11,
    12, 13, 11, 14, 10, 12, 14,
];

pub const g_kuiIdx2CtxLastSignificantCoeffFlag8x8: [u8; 64] = [
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8,
    8, 8,
];

/// `g_kuiDequantCoeff8x8` from `common_tables.cpp`. Each QP has its own 64-entry
/// matrix: the six `qp % 6` phases differ, so the table cannot be derived by
/// scaling a single base matrix.
pub const g_kuiDequantCoeff8x8: [[u16; 64]; 52] = [
    /* QP  0 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP  1 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP  2 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP  3 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP  4 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP  5 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP  6 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP  7 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP  8 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP  9 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 10 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 11 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 12 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 13 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 14 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 15 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 16 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 17 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 18 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 19 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 20 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 21 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 22 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 23 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 24 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 25 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 26 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 27 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 28 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 29 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 30 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 31 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 32 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 33 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 34 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 35 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 36 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 37 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 38 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 39 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 40 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 41 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 42 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 43 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 44 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 45 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
    /* QP 46 */ [
        512, 480, 640, 480, 512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608,
        816, 608, 640, 608, 816, 608, 480, 448, 608, 448, 480, 448, 608, 448, 512, 480, 640, 480,
        512, 480, 640, 480, 480, 448, 608, 448, 480, 448, 608, 448, 640, 608, 816, 608, 640, 608,
        816, 608, 480, 448, 608, 448, 480, 448, 608, 448
    ],
    /* QP 47 */ [
        576, 544, 736, 544, 576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688,
        928, 688, 736, 688, 928, 688, 544, 512, 688, 512, 544, 512, 688, 512, 576, 544, 736, 544,
        576, 544, 736, 544, 544, 512, 688, 512, 544, 512, 688, 512, 736, 688, 928, 688, 736, 688,
        928, 688, 544, 512, 688, 512, 544, 512, 688, 512
    ],
    /* QP 48 */ [
        320, 304, 400, 304, 320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384,
        512, 384, 400, 384, 512, 384, 304, 288, 384, 288, 304, 288, 384, 288, 320, 304, 400, 304,
        320, 304, 400, 304, 304, 288, 384, 288, 304, 288, 384, 288, 400, 384, 512, 384, 400, 384,
        512, 384, 304, 288, 384, 288, 304, 288, 384, 288
    ],
    /* QP 49 */ [
        352, 336, 448, 336, 352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416,
        560, 416, 448, 416, 560, 416, 336, 304, 416, 304, 336, 304, 416, 304, 352, 336, 448, 336,
        352, 336, 448, 336, 336, 304, 416, 304, 336, 304, 416, 304, 448, 416, 560, 416, 448, 416,
        560, 416, 336, 304, 416, 304, 336, 304, 416, 304
    ],
    /* QP 50 */ [
        416, 384, 528, 384, 416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496,
        672, 496, 528, 496, 672, 496, 384, 368, 496, 368, 384, 368, 496, 368, 416, 384, 528, 384,
        416, 384, 528, 384, 384, 368, 496, 368, 384, 368, 496, 368, 528, 496, 672, 496, 528, 496,
        672, 496, 384, 368, 496, 368, 384, 368, 496, 368
    ],
    /* QP 51 */ [
        448, 416, 560, 416, 448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528,
        720, 528, 560, 528, 720, 528, 416, 400, 528, 400, 416, 400, 528, 400, 448, 416, 560, 416,
        448, 416, 560, 416, 416, 400, 528, 400, 416, 400, 528, 400, 560, 528, 720, 528, 560, 528,
        720, 528, 416, 400, 528, 400, 416, 400, 528, 400
    ],
];

// ============================================================================
// Core Context Structures & Type Definitions
// ============================================================================

pub use crate::decoder::parse_mb_syn_cavlc::{SWelsNeighAvail, PWelsNeighAvail};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSubPictureLimits {
    pub iMinVmv: i16,
    pub iMaxVmv: i16,
}

pub use crate::decoder::parameter_sets::SSps;
pub use crate::decoder::slice::{SSlice, SSliceHeader, SSliceHeaderExt, EWelsSliceType};
pub use crate::decoder::decoder_core::{DqLayerState, PDqLayer, SLayerInfo};
pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext, SPicture, Picture, SRefPic, SLogContext, SDecodingParam as SDecoderParam};


// ============================================================================
// Helper Utilities & Math Primitives
// ============================================================================

#[inline(always)]
pub fn WelsMedian(a: i16, b: i16, c: i16) -> i16 {
    let mut min = a;
    let mut max = a;
    if b < min {
        min = b;
    }
    if b > max {
        max = b;
    }
    if c < min {
        min = c;
    }
    if c > max {
        max = c;
    }
    (a as i32 + b as i32 + c as i32 - min as i32 - max as i32) as i16
}

pub use crate::decoder::decoder_core::GetThreadCount;
// Used by the B-slice motion-info branches ported from ParseInterBMotionInfoCabac.
pub use crate::decoder::mv_pred::{
    SubMbType, FillSpatialDirect8x8Mv, FillTemporalDirect8x8Mv,
    // T5.M4 (F22): the seven that used to be re-translated below. `mv_pred.cpp` is
    // the C++'s home for each; this module calls them there, as its C++ counterpart
    // does.
    PredMv, PredInter16x8Mv, PredInter8x16Mv, UpdateP16x16MotionInfo,
    UpdateP16x8MotionInfo, UpdateP8x16MotionInfo, Update8x8RefIdx,
};
pub use crate::decoder::decode_slice::WELS_MIN;
pub use crate::decoder::decode_slice::{SPartMbInfo, g_ksInterPSubMbTypeInfo, g_ksInterBSubMbTypeInfo};
pub use crate::decoder::decode_slice::{g_kCacheNzcScanIdx, g_kuiCache30ScanIdx, g_kuiDequantCoeff, g_kuiScan4};

#[inline(always)]
pub fn GetMbResProperty(pMBproperty: &mut i32, pResidualProperty: &mut i32, bCavlc: bool) {
    match *pResidualProperty {
        CHROMA_AC_U => {
            *pMBproperty = 1;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_U };
        }
        CHROMA_AC_V => {
            *pMBproperty = 2;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_V };
        }
        LUMA_DC_AC_INTRA => {
            *pMBproperty = 0;
            *pResidualProperty = LUMA_DC_AC;
        }
        CHROMA_DC_U => {
            *pMBproperty = 1;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_U };
        }
        CHROMA_DC_V => {
            *pMBproperty = 2;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_V };
        }
        I16_LUMA_AC => {
            *pMBproperty = 0;
        }
        I16_LUMA_DC => {
            *pMBproperty = 0;
        }
        LUMA_DC_AC_INTER => {
            *pMBproperty = 3;
            *pResidualProperty = LUMA_DC_AC;
        }
        CHROMA_DC_U_INTER => {
            *pMBproperty = 4;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_U };
        }
        CHROMA_DC_V_INTER => {
            *pMBproperty = 5;
            *pResidualProperty = if bCavlc { CHROMA_DC } else { CHROMA_DC_V };
        }
        CHROMA_AC_U_INTER => {
            *pMBproperty = 4;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_U };
        }
        CHROMA_AC_V_INTER => {
            *pMBproperty = 5;
            *pResidualProperty = if bCavlc { CHROMA_AC } else { CHROMA_AC_V };
        }
        LUMA_DC_AC_INTRA_8 => {
            *pMBproperty = 6;
            *pResidualProperty = LUMA_DC_AC_8;
        }
        LUMA_DC_AC_INTER_8 => {
            *pMBproperty = 7;
            *pResidualProperty = LUMA_DC_AC_8;
        }
        _ => {
            *pMBproperty = *pResidualProperty;
        }
    }
}

// ============================================================================
// IDCT Dequantization Primitives
// ============================================================================

pub unsafe fn WelsLumaDcDequantIdct(pBlock: *mut i16, iQp: i32, pCtx: PWelsDecoderContext) {
    let kiQMul = if (*pCtx).bUseScalingList && !(*pCtx).pDequant_coeff4x4[0].is_null() {
        (*(*pCtx).pDequant_coeff4x4[0].add(iQp as usize))[0] as i32
    } else {
        (g_kuiDequantCoeff[iQp as usize][0] as i32) << 4
    };

    let mut iTemp = [0i32; 16];
    let kiXOffset = [0usize, 16, 64, 80];
    let kiYOffset = [0usize, 32, 128, 160];

    for i in 0..4 {
        let kiOffset = kiYOffset[i];
        let kiX1 = kiOffset + kiXOffset[2];
        let kiX2 = 16 + kiOffset;
        let kiX3 = kiOffset + kiXOffset[3];
        let kiI4 = i << 2;
        let kiZ0 = *pBlock.add(kiOffset) as i32 + *pBlock.add(kiX1) as i32;
        let kiZ1 = *pBlock.add(kiOffset) as i32 - *pBlock.add(kiX1) as i32;
        let kiZ2 = *pBlock.add(kiX2) as i32 - *pBlock.add(kiX3) as i32;
        let kiZ3 = *pBlock.add(kiX2) as i32 + *pBlock.add(kiX3) as i32;

        iTemp[kiI4] = kiZ0 + kiZ3;
        iTemp[1 + kiI4] = kiZ1 + kiZ2;
        iTemp[2 + kiI4] = kiZ1 - kiZ2;
        iTemp[3 + kiI4] = kiZ0 - kiZ3;
    }

    for i in 0..4 {
        let kiOffset = kiXOffset[i];
        let kiI4 = 4 + i;
        let kiZ0 = iTemp[i] + iTemp[4 + kiI4];
        let kiZ1 = iTemp[i] - iTemp[4 + kiI4];
        let kiZ2 = iTemp[kiI4] - iTemp[8 + kiI4];
        let kiZ3 = iTemp[kiI4] + iTemp[8 + kiI4];

        *pBlock.add(kiOffset) = (((kiZ0 + kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.add(kiYOffset[1] + kiOffset) = (((kiZ1 + kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.add(kiYOffset[2] + kiOffset) = (((kiZ1 - kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
        *pBlock.add(kiYOffset[3] + kiOffset) = (((kiZ0 - kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
    }
}

pub unsafe fn WelsChromaDcIdct(pBlock: *mut i16) {
    let iA = *pBlock as i32;
    let iB = *pBlock.add(16) as i32;
    let iC = *pBlock.add(32) as i32;
    let iD = *pBlock.add(48) as i32;

    let iE = iA - iB;
    let iA_sum = iA + iB;
    let iB_sub = iC - iD;
    let iC_sum = iC + iD;

    *pBlock = (iA_sum + iC_sum) as i16;
    *pBlock.add(16) = (iE + iB_sub) as i16;
    *pBlock.add(32) = (iA_sum - iC_sum) as i16;
    *pBlock.add(48) = (iE - iB_sub) as i16;
}

// ============================================================================
// Cache Update & Spatial Availability Helpers
// ============================================================================

pub unsafe fn DecodeCabacIntraMbType(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    ctx_base: i32,
) -> u32 {
    let mut uiCode: u32 = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(ctx_base as usize);

    if DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx, &mut uiCode) != ERR_NONE {
        return 0;
    }
    if uiCode == 0 {
        return 0; // I4x4
    }

    if DecodeTerminateCabac(cabac_win, pCabacDecEngine, &mut uiCode) != ERR_NONE {
        return 0;
    }
    if uiCode != 0 {
        return 25; // PCM
    }

    let mut uiMbType: u32 = 1; // I16x16
    DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.add(1), &mut uiCode);
    uiMbType += 12 * uiCode;

    DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.add(2), &mut uiCode);
    if uiCode != 0 {
        DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.add(2), &mut uiCode);
        uiMbType += 4 + 4 * uiCode;
    }
    DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.add(3), &mut uiCode);
    uiMbType += 2 * uiCode;
    DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.add(3), &mut uiCode);
    uiMbType += uiCode;
    uiMbType
}

pub unsafe fn UpdateP16x8RefIdxCabac(
    pCurDqLayer: PDqLayer,
    pDec: PPicture,
    pRefIndex: &mut [[i8; 30]; LIST_A],
    iPartIdx: i32,
    iRef: i8,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    let iScan4Idx4 = 4 + iScan4Idx;
    let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    let iCacheIdx6 = 6 + iCacheIdx;

    let pDecRef = (*pDec).pRefIndex[iListIdx as usize].get_mut(iMbXy);
    for offset in 0..4 {
        pDecRef[iScan4Idx + offset] = iRef;
        pDecRef[iScan4Idx4 + offset] = iRef;
        pRefIndex[iListIdx as usize][iCacheIdx + offset] = iRef;
        pRefIndex[iListIdx as usize][iCacheIdx6 + offset] = iRef;
    }
}

pub unsafe fn UpdateP8x16RefIdxCabac(
    pCurDqLayer: PDqLayer,
    pDec: PPicture,
    pRefIndex: &mut [[i8; 30]; LIST_A],
    mut iPartIdx: i32,
    iRef: i8,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    for _ in 0..2 {
        let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        let iScan4Idx4 = 4 + iScan4Idx;
        let iCacheIdx6 = 6 + iCacheIdx;

        let pDecRef = (*pDec).pRefIndex[iListIdx as usize].get_mut(iMbXy);
        for offset in 0..2 {
            pDecRef[iScan4Idx + offset] = iRef;
            pDecRef[iScan4Idx4 + offset] = iRef;
            pRefIndex[iListIdx as usize][iCacheIdx + offset] = iRef;
            pRefIndex[iListIdx as usize][iCacheIdx6 + offset] = iRef;
        }
        iPartIdx += 8;
    }
}

pub unsafe fn UpdateP8x8RefIdxCabac(
    pCurDqLayer: PDqLayer,
    pDec: PPicture,
    iPartIdx: i32,
    iRef: i8,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    let pDecRef = (*pDec).pRefIndex[iListIdx as usize].get_mut(iMbXy);
    pDecRef[iScan4Idx] = iRef;
    pDecRef[iScan4Idx + 1] = iRef;
    pDecRef[iScan4Idx + 4] = iRef;
    pDecRef[iScan4Idx + 5] = iRef;
}

pub unsafe fn UpdateP8x8DirectCabac(pCurDqLayer: PDqLayer, iPartIdx: i32) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    let pDirect = (*pCurDqLayer).grid.direct.get_mut(iMbXy).as_mut_ptr();
    *pDirect.add(iScan4Idx) = 1;
    *pDirect.add(iScan4Idx + 1) = 1;
    *pDirect.add(iScan4Idx + 4) = 1;
    *pDirect.add(iScan4Idx + 5) = 1;
}

pub unsafe fn UpdateP16x16DirectCabac(pCurDqLayer: PDqLayer) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDirect = (*pCurDqLayer).grid.direct.get_mut(iMbXy).as_mut_ptr();
    for i in (0..16).step_by(4) {
        let kuiScan4Idx = g_kuiScan4[i] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
        *pDirect.add(kuiScan4Idx) = 1;
        *pDirect.add(kuiScan4Idx + 1) = 1;
        *pDirect.add(kuiScan4IdxPlus4) = 1;
        *pDirect.add(kuiScan4IdxPlus4 + 1) = 1;
    }
}

pub unsafe fn UpdateP16x16MvdCabac(pCurDqLayer: *mut DqLayerState, pMvd: *const i16, iListIdx: i8) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mvd_x = *pMvd;
    let mvd_y = *pMvd.add(1);
    let pMvdTarget = (*pCurDqLayer).grid.mvd[iListIdx as usize].get_mut(iMbXy);
    for i in 0..16 {
        pMvdTarget[i] = [mvd_x, mvd_y];
    }
}

pub unsafe fn UpdateP16x8MvdCabac(
    pCurDqLayer: *mut DqLayerState,
    pMvdCache: &mut [[[i16; 2]; 30]; LIST_A],
    mut iPartIdx: i32,
    pMvd: *const i16,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mvd_pair = [*pMvd, *pMvd.add(1)];
    for _ in 0..2 {
        let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
        let iScan4Idx4 = 4 + iScan4Idx;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        let iCacheIdx6 = 6 + iCacheIdx;

        let pMvdTarget = (*pCurDqLayer).grid.mvd[iListIdx as usize].get_mut(iMbXy);
        for off in 0..2 {
            pMvdTarget[iScan4Idx + off] = mvd_pair;
            pMvdTarget[iScan4Idx4 + off] = mvd_pair;
            pMvdCache[iListIdx as usize][iCacheIdx + off] = mvd_pair;
            pMvdCache[iListIdx as usize][iCacheIdx6 + off] = mvd_pair;
        }
        iPartIdx += 4;
    }
}

pub unsafe fn UpdateP8x16MvdCabac(
    pCurDqLayer: *mut DqLayerState,
    pMvdCache: &mut [[[i16; 2]; 30]; LIST_A],
    mut iPartIdx: i32,
    pMvd: *const i16,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mvd_pair = [*pMvd, *pMvd.add(1)];
    for _ in 0..2 {
        let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
        let iScan4Idx4 = 4 + iScan4Idx;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        let iCacheIdx6 = 6 + iCacheIdx;

        let pMvdTarget = (*pCurDqLayer).grid.mvd[iListIdx as usize].get_mut(iMbXy);
        for off in 0..2 {
            pMvdTarget[iScan4Idx + off] = mvd_pair;
            pMvdTarget[iScan4Idx4 + off] = mvd_pair;
            pMvdCache[iListIdx as usize][iCacheIdx + off] = mvd_pair;
            pMvdCache[iListIdx as usize][iCacheIdx6 + off] = mvd_pair;
        }
        iPartIdx += 8;
    }
}

pub unsafe fn UpdateP8x8RefCacheIdxCabac(
    pRefIndex: &mut [[i8; 30]; LIST_A],
    iPartIdx: i16,
    listIdx: i32,
    iRef: i8,
) {
    let uiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    pRefIndex[listIdx as usize][uiCacheIdx] = iRef;
    pRefIndex[listIdx as usize][uiCacheIdx + 1] = iRef;
    pRefIndex[listIdx as usize][uiCacheIdx + 6] = iRef;
    pRefIndex[listIdx as usize][uiCacheIdx + 7] = iRef;
}

// ============================================================================
// Macroblock Header & Syntax Parsing Functions
// ============================================================================

pub unsafe fn ParseEndOfSliceCabac(pCtx: PWelsDecoderContext, uiBinVal: &mut u32) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    *uiBinVal = 0;
    let err = DecodeTerminateCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), uiBinVal);
    if err != ERR_NONE {
        return err;
    }
    ERR_NONE
}

pub unsafe fn ParseSkipFlagCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    uiSkip: &mut u32,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    *uiSkip = 0;
    let mut iCtxInc: i32 = NEW_CTX_OFFSET_SKIP;
    iCtxInc += (((*pNeighAvail).iLeftAvail != 0 && !IS_SKIP((*pNeighAvail).iLeftType as u32))
        as i32)
        + (((*pNeighAvail).iTopAvail != 0 && !IS_SKIP((*pNeighAvail).iTopType as u32)) as i32);
    if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
        iCtxInc += 13;
    }
    let pBinCtx = cabac_ctx_base(pCtx).add(iCtxInc as usize);
    let err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pBinCtx, uiSkip);
    if err != ERR_NONE {
        return err;
    }
    ERR_NONE
}

pub unsafe fn ParseMBTypeISliceCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    uiBinVal: &mut u32,
) -> i32 {
    let mut uiCode: u32 = 0;
    *uiBinVal = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_MB_TYPE_I as usize);

    let iIdxA = ((*pNeighAvail).iLeftAvail != 0
        && ((*pNeighAvail).iLeftType as u32 != MB_TYPE_INTRA4x4
            && (*pNeighAvail).iLeftType as u32 != MB_TYPE_INTRA8x8)) as i32;
    let iIdxB = ((*pNeighAvail).iTopAvail != 0
        && ((*pNeighAvail).iTopType as u32 != MB_TYPE_INTRA4x4
            && (*pNeighAvail).iTopType as u32 != MB_TYPE_INTRA8x8)) as i32;
    let iCtxInc = iIdxA + iIdxB;

    let mut err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(iCtxInc as isize), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    *uiBinVal = uiCode;

    if *uiBinVal != 0 {
        err = DecodeTerminateCabac(cabac_win, pCabacDecEngine, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode == 1 {
            *uiBinVal = 25; // I_PCM
        } else {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiBinVal = 1 + uiCode * 12;

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(4), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
                if err != ERR_NONE {
                    return err;
                }
                *uiBinVal += 4;
                if uiCode != 0 {
                    *uiBinVal += 4;
                }
            }

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(6), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiBinVal += uiCode << 1;

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(7), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiBinVal += uiCode;
        }
    }
    ERR_NONE
}

pub unsafe fn ParseMBTypePSliceCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    uiMbType: &mut u32,
) -> i32 {
    let mut uiCode: u32 = 0;
    *uiMbType = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_SKIP as usize);

    let mut err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }

    if uiCode != 0 {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(6), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            err = DecodeTerminateCabac(cabac_win, pCabacDecEngine, &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                *uiMbType = 30; // MB_TYPE_INTRA_PCM
                return ERR_NONE;
            }

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(7), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType = 6 + uiCode * 12;

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(8), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                *uiMbType += 4;
                err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(8), &mut uiCode);
                if err != ERR_NONE {
                    return err;
                }
                if uiCode != 0 {
                    *uiMbType += 4;
                }
            }

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(9), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType += uiCode << 1;

            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(9), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType += uiCode;
        } else {
            *uiMbType = 5; // Intra 4x4
        }
    } else {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(4), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(6), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                *uiMbType = 1;
            } else {
                *uiMbType = 2;
            }
        } else {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                *uiMbType = 3;
            } else {
                *uiMbType = 0;
            }
        }
    }
    ERR_NONE
}

pub unsafe fn ParseMBTypeBSliceCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    uiMbType: &mut u32,
) -> i32 {
    let mut uiCode: u32 = 0;
    *uiMbType = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(27);

    let iIdxA = ((*pNeighAvail).iLeftAvail != 0 && !IS_DIRECT((*pNeighAvail).iLeftType as u32)) as i32;
    let iIdxB = ((*pNeighAvail).iTopAvail != 0 && !IS_DIRECT((*pNeighAvail).iTopType as u32)) as i32;
    let iCtxInc = iIdxA + iIdxB;

    let mut err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(iCtxInc as isize), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }

    if uiCode == 0 {
        *uiMbType = 0; // Bi_Direct
    } else {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode == 0 {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType = 1 + uiCode; // 16x16 L0L1
        } else {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(4), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType = uiCode << 3;
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType |= uiCode << 2;
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType |= uiCode << 1;
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType |= uiCode;

            if *uiMbType < 8 {
                *uiMbType += 3;
                return ERR_NONE;
            } else if *uiMbType == 13 {
                *uiMbType = DecodeCabacIntraMbType(pCtx, pNeighAvail, 32) + 23;
                return ERR_NONE;
            } else if *uiMbType == 14 {
                *uiMbType = 11; // Bi8x16
                return ERR_NONE;
            } else if *uiMbType == 15 {
                *uiMbType = 22; // 8x8
                return ERR_NONE;
            }

            *uiMbType <<= 1;
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(5), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiMbType |= uiCode;
            *uiMbType -= 4;
        }
    }
    ERR_NONE
}

pub unsafe fn ParseTransformSize8x8FlagCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    bTransformSize8x8Flag: &mut bool,
) -> i32 {
    let mut uiCode: u32 = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_TS_8x8_FLAG as usize);
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iMbWidth = (*pCurDqLayer).iMbWidth as usize;

    let iIdxA = if (*pNeighAvail).iLeftAvail != 0 {
        *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy - 1) as i32
    } else {
        0
    };
    let iIdxB = if (*pNeighAvail).iTopAvail != 0 {
        *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy - iMbWidth) as i32
    } else {
        0
    };
    let iCtxInc = iIdxA + iIdxB;

    let err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(iCtxInc as isize), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    *bTransformSize8x8Flag = uiCode != 0;
    ERR_NONE
}

pub unsafe fn ParseSubMBTypeCabac(
    pCtx: PWelsDecoderContext,
    _pNeighAvail: *const SWelsNeighAvail,
    uiSubMbType: &mut u32,
) -> i32 {
    let mut uiCode: u32 = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_SUBMB_TYPE as usize);

    let mut err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx, &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode != 0 {
        *uiSubMbType = 0;
    } else {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(1), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(2), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiSubMbType = 3 - uiCode;
        } else {
            *uiSubMbType = 1;
        }
    }
    ERR_NONE
}

pub unsafe fn ParseBSubMBTypeCabac(
    pCtx: PWelsDecoderContext,
    _pNeighAvail: *const SWelsNeighAvail,
    uiSubMbType: &mut u32,
) -> i32 {
    let mut uiCode: u32 = 0;
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let cabac_win = cabac_rbsp_window(pCtx);
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_B_SUBMB_TYPE as usize);

    let mut err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx, &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode == 0 {
        *uiSubMbType = 0; // B_Direct_8x8
        return ERR_NONE;
    }

    err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(1), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode == 0 {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        *uiSubMbType = 1 + uiCode; // B_L0_8x8, B_L1_8x8
        return ERR_NONE;
    }

    *uiSubMbType = 3;
    err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(2), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode != 0 {
        err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *uiSubMbType = 11 + uiCode; // B_L1_4x4, B_Bi_4x4
            return ERR_NONE;
        }
        *uiSubMbType += 4;
    }

    err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    *uiSubMbType += 2 * uiCode;

    err = DecodeBinCabac(cabac_win, pCabacDecEngine, pBinCtx.offset(3), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    *uiSubMbType += uiCode;
    ERR_NONE
}

pub unsafe fn ParseIntraPredModeLumaCabac(pCtx: PWelsDecoderContext, iBinVal: &mut i32) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    *iBinVal = 0;
    let mut err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_IPR as usize),
        &mut uiCode,
    );
    if err != ERR_NONE {
        return err;
    }
    if uiCode == 1 {
        *iBinVal = -1;
    } else {
        let pCtx1 = cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_IPR + 1) as usize);
        err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pCtx1, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        *iBinVal |= uiCode as i32;

        err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pCtx1, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        *iBinVal |= (uiCode as i32) << 1;

        err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pCtx1, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        *iBinVal |= (uiCode as i32) << 2;
    }
    ERR_NONE
}

pub unsafe fn ParseIntraPredModeChromaCabac(
    pCtx: PWelsDecoderContext,
    uiNeighAvail: u8,
    iBinVal: &mut i32,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pMbType = crate::decoder::decoder_core::mb_grid_ptr(&mut (*dec_pic(pCtx)).pMbType, 0);
    let iLeftAvail = uiNeighAvail & 0x04;
    let iTopAvail = uiNeighAvail & 0x01;
    let iMbXy = (*pCurDqLayer).iMbXyIndex;

    *iBinVal = 0;

    let iIdxB = if iTopAvail != 0 {
        let top_idx = (iMbXy - (*pCurDqLayer).iMbWidth) as usize;
        let mode = *(*pCurDqLayer).grid.chroma_pred_mode.get(top_idx);
        (mode > 0 && mode <= 3 && *pMbType.add(top_idx) != MB_TYPE_INTRA_PCM) as i32
    } else {
        0
    };
    let iIdxA = if iLeftAvail != 0 {
        let left_idx = (iMbXy - 1) as usize;
        let mode = *(*pCurDqLayer).grid.chroma_pred_mode.get(left_idx);
        (mode > 0 && mode <= 3 && *pMbType.add(left_idx) != MB_TYPE_INTRA_PCM) as i32
    } else {
        0
    };
    let iCtxInc = iIdxA + iIdxB;

    let mut err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CIPR + iCtxInc) as usize),
        &mut uiCode,
    );
    if err != ERR_NONE {
        return err;
    }
    *iBinVal = uiCode as i32;

    if *iBinVal != 0 {
        let mut iSym: u32 = 0;
        err = DecodeBinCabac(cabac_win, 
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CIPR + 3) as usize),
            &mut iSym,
        );
        if err != ERR_NONE {
            return err;
        }
        if iSym == 0 {
            *iBinVal = (iSym + 1) as i32;
            return ERR_NONE;
        }
        iSym = 0;
        loop {
            err = DecodeBinCabac(cabac_win, 
                std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
                cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CIPR + 3) as usize),
                &mut uiCode,
            );
            if err != ERR_NONE {
                return err;
            }
            iSym += 1;
            if uiCode == 0 || iSym >= 1 {
                break;
            }
        }
        if uiCode != 0 && iSym == 1 {
            iSym += 1;
        }
        *iBinVal = (iSym + 1) as i32;
    }
    ERR_NONE
}

// ============================================================================
// Inter Motion Vector & Reference Index Parsing
// ============================================================================

pub unsafe fn ParseRefIdxCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    _nzc: *mut u8,
    ref_idx: &mut [[i8; 30]; LIST_A],
    direct: Option<&[i8; 30]>,
    iListIdx: i32,
    iZOrderIdx: i32,
    iActiveRefNum: i32,
    _b8mode: i32,
    iRefIdxVal: &mut i8,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    if iActiveRefNum == 1 {
        *iRefIdxVal = 0;
        return ERR_NONE;
    }

    let mut uiCode: u32 = 0;
    let iIdxA: i32;
    let iIdxB: i32;
    let mut iCtxInc: i32 = 0;
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pRefIdxInMB = *(*dec_pic(pCtx)).pRefIndex[iListIdx as usize].get(iMbXy);
    let pDirect = (*pCurDqLayer).grid.direct.get_mut(iMbXy).as_mut_ptr();

    let scan_cache = g_kuiCache30ScanIdx[iZOrderIdx as usize] as usize;

    if iZOrderIdx == 0 {
        iIdxB = ((*pNeighAvail).iTopAvail != 0
            && (*pNeighAvail).iTopType != MB_TYPE_INTRA_PCM
            && ref_idx[iListIdx as usize][scan_cache - 6] > 0) as i32;
        iIdxA = ((*pNeighAvail).iLeftAvail != 0
            && (*pNeighAvail).iLeftType != MB_TYPE_INTRA_PCM
            && ref_idx[iListIdx as usize][scan_cache - 1] > 0) as i32;
        if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
            if iIdxB > 0 && direct.is_some_and(|d| d[scan_cache - 6] == 0) {
                iCtxInc += 2;
            }
            if iIdxA > 0 && direct.is_some_and(|d| d[scan_cache - 1] == 0) {
                iCtxInc += 1;
            }
        }
    } else if iZOrderIdx == 4 {
        iIdxB = ((*pNeighAvail).iTopAvail != 0
            && (*pNeighAvail).iTopType != MB_TYPE_INTRA_PCM
            && ref_idx[iListIdx as usize][scan_cache - 6] > 0) as i32;
        iIdxA = (pRefIdxInMB[g_kuiScan4[iZOrderIdx as usize] as usize - 1] > 0) as i32;
        if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
            if iIdxB > 0 && direct.is_some_and(|d| d[scan_cache - 6] == 0) {
                iCtxInc += 2;
            }
            if iIdxA > 0 && *pDirect.add(g_kuiScan4[iZOrderIdx as usize] as usize - 1) == 0 {
                iCtxInc += 1;
            }
        }
    } else if iZOrderIdx == 8 {
        iIdxB = (pRefIdxInMB[g_kuiScan4[iZOrderIdx as usize] as usize - 4] > 0) as i32;
        iIdxA = ((*pNeighAvail).iLeftAvail != 0
            && (*pNeighAvail).iLeftType != MB_TYPE_INTRA_PCM
            && ref_idx[iListIdx as usize][scan_cache - 1] > 0) as i32;
        if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
            if iIdxB > 0 && *pDirect.add(g_kuiScan4[iZOrderIdx as usize] as usize - 4) == 0 {
                iCtxInc += 2;
            }
            if iIdxA > 0 && direct.is_some_and(|d| d[scan_cache - 1] == 0) {
                iCtxInc += 1;
            }
        }
    } else {
        iIdxB = (pRefIdxInMB[g_kuiScan4[iZOrderIdx as usize] as usize - 4] > 0) as i32;
        iIdxA = (pRefIdxInMB[g_kuiScan4[iZOrderIdx as usize] as usize - 1] > 0) as i32;
        if (*pCtx).eSliceType == EWelsSliceType::B_SLICE {
            if iIdxB > 0 && *pDirect.add(g_kuiScan4[iZOrderIdx as usize] as usize - 4) == 0 {
                iCtxInc += 2;
            }
            if iIdxA > 0 && *pDirect.add(g_kuiScan4[iZOrderIdx as usize] as usize - 1) == 0 {
                iCtxInc += 1;
            }
        }
    }

    if (*pCtx).eSliceType != EWelsSliceType::B_SLICE {
        iCtxInc = iIdxA + (iIdxB << 1);
    }

    let mut err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_REF_NO + iCtxInc) as usize),
        &mut uiCode,
    );
    if err != ERR_NONE {
        return err;
    }
    if uiCode != 0 {
        err = DecodeUnaryBinCabac(cabac_win, 
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_REF_NO + 4) as usize),
            1,
            &mut uiCode,
        );
        if err != ERR_NONE {
            return err;
        }
        uiCode += 1;
    }
    *iRefIdxVal = uiCode as i8;
    ERR_NONE
}

pub unsafe fn ParseMvdInfoCabac(
    pCtx: PWelsDecoderContext,
    _pNeighAvail: *const SWelsNeighAvail,
    pRefIndex: &[[i8; 30]; LIST_A],
    pMvdCache: &[[[i16; 2]; 30]; LIST_A],
    index: i32,
    iListIdx: i8,
    iMvComp: i8,
    iMvdVal: &mut i16,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    let mut iIdxA: i32 = 0;
    let pBinCtx = cabac_ctx_base(pCtx).add(
        (NEW_CTX_OFFSET_MVD + (iMvComp as i32) * CTX_NUM_MVD) as usize,
    );
    *iMvdVal = 0;

    let cache_idx = g_kuiCache30ScanIdx[index as usize] as usize;
    if pRefIndex[iListIdx as usize][cache_idx - 6] >= 0 {
        iIdxA = (pMvdCache[iListIdx as usize][cache_idx - 6][iMvComp as usize] as i32).abs();
    }
    if pRefIndex[iListIdx as usize][cache_idx - 1] >= 0 {
        iIdxA += (pMvdCache[iListIdx as usize][cache_idx - 1][iMvComp as usize] as i32).abs();
    }

    let mut iCtxInc = 0;
    if iIdxA >= 3 {
        iCtxInc = 1 + (iIdxA > 32) as i32;
    }

    let mut err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pBinCtx.add(iCtxInc as usize), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode != 0 {
        err = DecodeUEGMvCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pBinCtx.add(3), 3, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        *iMvdVal = (uiCode + 1) as i16;
        err = DecodeBypassCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            *iMvdVal = -*iMvdVal;
        }
    } else {
        *iMvdVal = 0;
    }
    ERR_NONE
}

// ============================================================================
// T5.M4 — F22's unification. Seven functions were re-translated here from
// `mv_pred.cpp`, which is where the C++ declares each of them exactly once and
// from which `parse_mb_syn_cabac.cpp` merely *calls* them (`:562`, `:592`,
// `:797`, `:841`). The local copies are deleted; this module imports them, and a
// local item no longer shadows the import.
//
//   PredMv, PredInter16x8Mv, PredInter8x16Mv   — never touch `pDec`, no guard
//                                                question, bodies agreed
//   UpdateP16x16MotionInfo                     — `mv_pred.rs`'s guard wins
//   UpdateP16x8MotionInfo, UpdateP8x16MotionInfo — `mv_pred.rs`'s guard wins
//   Update8x8RefIdx                            — **this copy won**: the C++ has
//                                                no guard and `mv_pred.rs` had
//                                                added one
//
// `UpdateP8x8RefIdxCabac` stays here, because `parse_mb_syn_cabac.cpp:141` is
// where the C++ declares *it* and `mv_pred.cpp` is the caller.
// ============================================================================


pub unsafe fn ParseInterPMotionInfoCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pMotionVector: &mut [[[i16; 2]; 30]; LIST_A],
    pMvdCache: &mut [[[i16; 2]; 30]; LIST_A],
    pRefIndex: &mut [[i8; 30]; LIST_A],
) -> i32 {
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;
    let ppRefPic = &(*pCtx).sRefPic.pRefList[LIST_0];
    let pRefCount0 = pSliceHeader.uiRefCount[0];
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mut pMv = [0i16; 2];
    let mut pMvd = [0i16; 2];
    let mut iRef = [0i8; 2];

    let pSps = pSliceHeader.pSps as *mut SSps;
    let iMinVmv = (*(*pSps).pSLevelLimits).iMinVmv;
    let iMaxVmv = (*(*pSps).pSLevelLimits).iMaxVmv;

    let bIsPending = GetThreadCount(pCtx) > 1;
    let pDec = dec_pic(pCtx);
    let mbType = *(*pDec).pMbType.get(iMbXy);

    match mbType {
        MB_TYPE_16x16 => {
            let iPartIdx = 0;
            let err = ParseRefIdxCabac(
                pCtx,
                pNeighAvail,
                pNonZeroCount,
                pRefIndex,
                // P slices have no direct cache; the C++ passes NULL here too.
                None,
                LIST_0 as i32,
                iPartIdx,
                pRefCount0,
                0,
                &mut iRef[0],
            );
            if err != ERR_NONE {
                return err;
            }
            if iRef[0] < 0 || iRef[0] as i32 >= pRefCount0 || ppRefPic[iRef[0] as usize].is_none() {
                (*pCtx).bMbRefConcealed = true;
                if (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                    iRef[0] = 0;
                    (*pCtx).iErrorCode |= dsBitstreamError;
                } else {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                }
            }
            let pPic0 = pool_pic(pCtx, ppRefPic[iRef[0] as usize]);
            (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                || (*pCtx).bMbRefConcealed
                || !(pPic0.is_null() || (*pPic0).bIsComplete || bIsPending);

            PredMv(pMotionVector, pRefIndex, LIST_0, 0, 4, iRef[0], &mut pMv);
            let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 0, &mut pMvd[0]);
            if err != ERR_NONE { return err; }
            err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 1, &mut pMvd[1]);
            if err != ERR_NONE { return err; }

            pMv[0] += pMvd[0];
            pMv[1] += pMvd[1];

            UpdateP16x16MotionInfo(pCurDqLayer, pDec, LIST_0, iRef[0], pMv.as_ptr());
            UpdateP16x16MvdCabac(pCurDqLayer, pMvd.as_ptr(), LIST_0 as i8);
        }
        MB_TYPE_16x8 => {
            for i in 0..2 {
                let iPartIdx = i << 3;
                let err = ParseRefIdxCabac(
                    pCtx,
                    pNeighAvail,
                    pNonZeroCount,
                    pRefIndex,
                    // P slices have no direct cache; the C++ passes NULL here too.
                    None,
                    LIST_0 as i32,
                    iPartIdx,
                    pRefCount0,
                    0,
                    &mut iRef[i as usize],
                );
                if err != ERR_NONE {
                    return err;
                }
                if iRef[i as usize] < 0 || iRef[i as usize] as i32 >= pRefCount0 || ppRefPic[iRef[i as usize] as usize].is_none() {
                    (*pCtx).bMbRefConcealed = true;
                    if (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                        iRef[i as usize] = 0;
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                }
                let pPic = pool_pic(pCtx, ppRefPic[iRef[i as usize] as usize]);
                (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                    || (*pCtx).bMbRefConcealed
                    || !(pPic.is_null() || (*pPic).bIsComplete || bIsPending);
                UpdateP16x8RefIdxCabac(pCurDqLayer, pDec, pRefIndex, iPartIdx, iRef[i as usize], LIST_0 as i8);
            }
            for i in 0..2 {
                let iPartIdx = i << 3;
                PredInter16x8Mv(pMotionVector, pRefIndex, LIST_0, iPartIdx, iRef[i as usize], &mut pMv);
                let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 0, &mut pMvd[0]);
                if err != ERR_NONE { return err; }
                err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 1, &mut pMvd[1]);
                if err != ERR_NONE { return err; }

                pMv[0] += pMvd[0];
                pMv[1] += pMvd[1];

                UpdateP16x8MotionInfo(pCurDqLayer, pDec, pMotionVector, pRefIndex, LIST_0, iPartIdx, iRef[i as usize], pMv.as_ptr());
                UpdateP16x8MvdCabac(pCurDqLayer, pMvdCache, iPartIdx as i32, pMvd.as_ptr(), LIST_0 as i8);
            }
        }
        MB_TYPE_8x16 => {
            for i in 0..2 {
                let iPartIdx = i << 2;
                let err = ParseRefIdxCabac(
                    pCtx,
                    pNeighAvail,
                    pNonZeroCount,
                    pRefIndex,
                    // P slices have no direct cache; the C++ passes NULL here too.
                    None,
                    LIST_0 as i32,
                    iPartIdx,
                    pRefCount0,
                    0,
                    &mut iRef[i as usize],
                );
                if err != ERR_NONE {
                    return err;
                }
                if iRef[i as usize] < 0 || iRef[i as usize] as i32 >= pRefCount0 || ppRefPic[iRef[i as usize] as usize].is_none() {
                    (*pCtx).bMbRefConcealed = true;
                    if (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                        iRef[i as usize] = 0;
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                }
                let pPic = pool_pic(pCtx, ppRefPic[iRef[i as usize] as usize]);
                (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                    || (*pCtx).bMbRefConcealed
                    || !(pPic.is_null() || (*pPic).bIsComplete || bIsPending);
                UpdateP8x16RefIdxCabac(pCurDqLayer, pDec, pRefIndex, iPartIdx, iRef[i as usize], LIST_0 as i8);
            }
            for i in 0..2 {
                let iPartIdx = i << 2;
                PredInter8x16Mv(pMotionVector, pRefIndex, LIST_0, (i << 2) as usize, iRef[i as usize], &mut pMv);
                let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 0, &mut pMvd[0]);
                if err != ERR_NONE { return err; }
                err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 1, &mut pMvd[1]);
                if err != ERR_NONE { return err; }

                pMv[0] += pMvd[0];
                pMv[1] += pMvd[1];

                UpdateP8x16MotionInfo(pCurDqLayer, pDec, pMotionVector, pRefIndex, LIST_0, iPartIdx, iRef[i as usize], pMv.as_ptr());
                UpdateP8x16MvdCabac(pCurDqLayer, pMvdCache, iPartIdx as i32, pMvd.as_ptr(), LIST_0 as i8);
            }
        }
        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
            let mut pRefIdx = [0i8; 4];
            let mut pSubPartCount = [0i8; 4];
            let mut pPartW = [0i8; 4];
            let mut uiSubMbType: u32 = 0;

            // T5.I1: two window borrows for the whole arm. `ParseSubMBTypeCabac`,
            // `ParseRefIdxCabac` and `UpdateP8x8RefIdxCabac` reach the layer but
            // neither of these arrays; the eight per-partition checks below become
            // two.
            let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy);
            let pNoSubMbPartSizeLessThan8x8Flag = (*pCurDqLayer)
                .grid
                .no_sub_mb_part_size_less_than8x8_flag
                .get_mut(iMbXy);

            for i in 0..4 {
                let err = ParseSubMBTypeCabac(pCtx, pNeighAvail, &mut uiSubMbType);
                if err != ERR_NONE {
                    return err;
                }
                if uiSubMbType >= 4 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_SUB_MB_TYPE);
                }
                pSubMbType[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iType;
                pSubPartCount[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iPartCount;
                pPartW[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iPartWidth;

                *pNoSubMbPartSizeLessThan8x8Flag =
                    *pNoSubMbPartSizeLessThan8x8Flag && (uiSubMbType == 0);
            }

            for i in 0..4 {
                let iIdx8 = (i << 2) as i32;
                let err = ParseRefIdxCabac(
                    pCtx,
                    pNeighAvail,
                    pNonZeroCount,
                    pRefIndex,
                    // P slices have no direct cache; the C++ passes NULL here too.
                    None,
                    LIST_0 as i32,
                    iIdx8,
                    pRefCount0,
                    1,
                    &mut pRefIdx[i],
                );
                if err != ERR_NONE {
                    return err;
                }
                if pRefIdx[i] < 0 || pRefIdx[i] as i32 >= pRefCount0 || ppRefPic[pRefIdx[i] as usize].is_none() {
                    (*pCtx).bMbRefConcealed = true;
                    if (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                        pRefIdx[i] = 0;
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                }
                let pPic = pool_pic(pCtx, ppRefPic[pRefIdx[i] as usize]);
                (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                    || (*pCtx).bMbRefConcealed
                    || !(pPic.is_null() || (*pPic).bIsComplete || bIsPending);
                UpdateP8x8RefIdxCabac(pCurDqLayer, pDec, iIdx8, pRefIdx[i], LIST_0 as i8);
            }

            for i in 0..4 {
                let iPartCount = pSubPartCount[i] as usize;
                uiSubMbType = pSubMbType[i];
                let iBlockW = pPartW[i] as usize;
                let mut iCacheIdx = g_kuiCache30ScanIdx[i << 2] as usize;

                pRefIndex[0][iCacheIdx] = pRefIdx[i];
                pRefIndex[0][iCacheIdx + 1] = pRefIdx[i];
                pRefIndex[0][iCacheIdx + 6] = pRefIdx[i];
                pRefIndex[0][iCacheIdx + 7] = pRefIdx[i];

                for j in 0..iPartCount {
                    let iPartIdx = (i << 2) + j * iBlockW;
                    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
                    iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;

                    PredMv(pMotionVector, pRefIndex, LIST_0, iPartIdx, iBlockW, pRefIdx[i], &mut pMv);
                    let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 0, &mut pMvd[0]);
                    if err != ERR_NONE { return err; }
                    err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, LIST_0 as i8, 1, &mut pMvd[1]);
                    if err != ERR_NONE { return err; }

                    pMv[0] += pMvd[0];
                    pMv[1] += pMvd[1];

                    let pDecMv = (*dec_pic(pCtx)).pMv[0].get_mut(iMbXy);
                    let pMvdTarget = (*pCurDqLayer).grid.mvd[0].get_mut(iMbXy);

                    if SUB_MB_TYPE_8x8 == uiSubMbType {
                        pDecMv[iScan4Idx] = pMv;
                        pDecMv[iScan4Idx + 1] = pMv;
                        pDecMv[iScan4Idx + 4] = pMv;
                        pDecMv[iScan4Idx + 5] = pMv;

                        pMvdTarget[iScan4Idx] = pMvd;
                        pMvdTarget[iScan4Idx + 1] = pMvd;
                        pMvdTarget[iScan4Idx + 4] = pMvd;
                        pMvdTarget[iScan4Idx + 5] = pMvd;

                        pMotionVector[0][iCacheIdx] = pMv;
                        pMotionVector[0][iCacheIdx + 1] = pMv;
                        pMotionVector[0][iCacheIdx + 6] = pMv;
                        pMotionVector[0][iCacheIdx + 7] = pMv;

                        pMvdCache[0][iCacheIdx] = pMvd;
                        pMvdCache[0][iCacheIdx + 1] = pMvd;
                        pMvdCache[0][iCacheIdx + 6] = pMvd;
                        pMvdCache[0][iCacheIdx + 7] = pMvd;
                    } else if SUB_MB_TYPE_8x4 == uiSubMbType {
                        pDecMv[iScan4Idx] = pMv;
                        pDecMv[iScan4Idx + 1] = pMv;
                        pMvdTarget[iScan4Idx] = pMvd;
                        pMvdTarget[iScan4Idx + 1] = pMvd;

                        pMotionVector[0][iCacheIdx] = pMv;
                        pMotionVector[0][iCacheIdx + 1] = pMv;
                        pMvdCache[0][iCacheIdx] = pMvd;
                        pMvdCache[0][iCacheIdx + 1] = pMvd;
                    } else if SUB_MB_TYPE_4x8 == uiSubMbType {
                        pDecMv[iScan4Idx] = pMv;
                        pDecMv[iScan4Idx + 4] = pMv;
                        pMvdTarget[iScan4Idx] = pMvd;
                        pMvdTarget[iScan4Idx + 4] = pMvd;

                        pMotionVector[0][iCacheIdx] = pMv;
                        pMotionVector[0][iCacheIdx + 6] = pMv;
                        pMvdCache[0][iCacheIdx] = pMvd;
                        pMvdCache[0][iCacheIdx + 6] = pMvd;
                    } else {
                        pDecMv[iScan4Idx] = pMv;
                        pMvdTarget[iScan4Idx] = pMvd;
                        pMotionVector[0][iCacheIdx] = pMv;
                        pMvdCache[0][iCacheIdx] = pMvd;
                    }
                }
            }
        }
        _ => {}
    }
    ERR_NONE
}

pub unsafe fn ParseInterBMotionInfoCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    pNonZeroCount: *mut u8,
    pMotionVector: &mut [[[i16; 2]; 30]; LIST_A],
    pMvdCache: &mut [[[i16; 2]; 30]; LIST_A],
    pRefIndex: &mut [[i8; 30]; LIST_A],
    pDirect: &mut [i8; 30],
) -> i32 {
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    let pRefCount = pSliceHeader.uiRefCount;
    let pDec = dec_pic(pCtx);
    let mbType = *(*pDec).pMbType.get(iMbXy);

    // C keeps pMv[4]/pMvd[4]: the 8x8 path duplicates the low pair into the high
    // pair (`ST32 (pMv + 2, LD32 (pMv))`) so it can store 8 bytes at once.
    let mut pMv = [0i16; 4];
    let mut pMvd = [0i16; 4];
    let mut iRef = [0i8; LIST_A];

    let bIsPending = GetThreadCount(pCtx) > 1;

    /// `pCtx->bMbRefConcealed = pCtx->bRPLRError || pCtx->bMbRefConcealed ||
    ///  !(pRefList[ref] && (pRefList[ref]->bIsComplete || bIsPending))`
    macro_rules! note_ref_concealed {
        ($listIdx:expr, $iref:expr) => {{
            let p = ref_pic(pCtx, $listIdx as usize, $iref as usize);
            (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                || (*pCtx).bMbRefConcealed
                || !(!p.is_null() && ((*p).bIsComplete || bIsPending));
        }};
    }

    /// Shared `ref_idx` validation: `RETURN_ERR_IF_NULL` on the concealed path.
    macro_rules! check_ref_idx {
        ($listIdx:expr, $iref:expr) => {{
            let list = $listIdx as usize;
            let ppRefPic = &(*pCtx).sRefPic.pRefList[list];
            if $iref < 0
                || $iref as i32 >= pRefCount[list]
                || ppRefPic[$iref as usize].is_none()
            {
                (*pCtx).bMbRefConcealed = true;
                if (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                    $iref = 0;
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    if ppRefPic[$iref as usize].is_none() {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                } else {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                }
            }
            note_ref_concealed!(list, $iref);
        }};
    }

    if IS_DIRECT(mbType) {
        let mut pMvDirect = [[0i16; 2]; LIST_A];
        let mut subMbType: SubMbType = 0;
        if pSliceHeader.iDirectSpatialMvPredFlag != 0 {
            // predict direct spatial mv
            let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                pCtx,
                &mut pMvDirect,
                &mut iRef,
                &mut subMbType,
            );
            if ret != ERR_NONE {
                return ret;
            }
        } else {
            // temporal direct 16x16 mode
            let ret = crate::decoder::mv_pred::PredBDirectTemporal(
                pCtx,
                &mut pMvDirect,
                &mut iRef,
                &mut subMbType,
            );
            if ret != ERR_NONE {
                return ret;
            }
        }
    } else if IS_INTER_16x16(mbType) {
        let iPartIdx = 0;
        for listIdx in LIST_0..LIST_A {
            iRef[listIdx] = REF_NOT_IN_LIST;
            if IS_DIR(mbType, 0, listIdx) {
                let err = ParseRefIdxCabac(
                    pCtx,
                    pNeighAvail,
                    pNonZeroCount,
                    pRefIndex,
                    Some(pDirect),
                    listIdx as i32,
                    iPartIdx,
                    pRefCount[listIdx],
                    0,
                    &mut iRef[listIdx],
                );
                if err != ERR_NONE {
                    return err;
                }
                check_ref_idx!(listIdx, iRef[listIdx]);
            }
        }
        for listIdx in LIST_0..LIST_A {
            if IS_DIR(mbType, 0, listIdx) {
                PredMv(pMotionVector, pRefIndex, listIdx, 0, 4, iRef[listIdx], (&mut pMv[..2]).try_into().unwrap());
                let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 0, &mut pMvd[0]);
                if err != ERR_NONE { return err; }
                err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 1, &mut pMvd[1]);
                if err != ERR_NONE { return err; }
                pMv[0] += pMvd[0];
                pMv[1] += pMvd[1];
            } else {
                pMv[0] = 0; pMv[1] = 0;
                pMvd[0] = 0; pMvd[1] = 0;
            }
            let mv2: [i16; 2] = [pMv[0], pMv[1]];
            UpdateP16x16MotionInfo(pCurDqLayer, pDec, listIdx, iRef[listIdx], mv2.as_ptr());
            UpdateP16x16MvdCabac(pCurDqLayer, pMvd.as_ptr(), listIdx as i8);
        }
    } else if IS_INTER_16x8(mbType) {
        let mut ref_idx_list = [[REF_NOT_IN_LIST; 2]; LIST_A];
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                let iPartIdx = (i << 3) as i32;
                let mut ref_idx: i8 = REF_NOT_IN_LIST;
                if IS_DIR(mbType, i, listIdx) {
                    let err = ParseRefIdxCabac(
                        pCtx,
                        pNeighAvail,
                        pNonZeroCount,
                        pRefIndex,
                        Some(pDirect),
                        listIdx as i32,
                        iPartIdx,
                        pRefCount[listIdx],
                        0,
                        &mut ref_idx,
                    );
                    if err != ERR_NONE {
                        return err;
                    }
                    check_ref_idx!(listIdx, ref_idx);
                }
                UpdateP16x8RefIdxCabac(pCurDqLayer, pDec, pRefIndex, iPartIdx, ref_idx, listIdx as i8);
                ref_idx_list[listIdx][i] = ref_idx;
            }
        }
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                let iPartIdx = (i << 3) as i32;
                let ref_idx = ref_idx_list[listIdx][i];
                if IS_DIR(mbType, i, listIdx) {
                    let mut mvp: [i16; 2] = [0, 0];
                    PredInter16x8Mv(pMotionVector, pRefIndex, listIdx, iPartIdx as usize, ref_idx, &mut mvp);
                    pMv[0] = mvp[0];
                    pMv[1] = mvp[1];
                    let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 0, &mut pMvd[0]);
                    if err != ERR_NONE { return err; }
                    err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 1, &mut pMvd[1]);
                    if err != ERR_NONE { return err; }
                    pMv[0] += pMvd[0];
                    pMv[1] += pMvd[1];
                } else {
                    pMv[0] = 0; pMv[1] = 0;
                    pMvd[0] = 0; pMvd[1] = 0;
                }
                let mv2: [i16; 2] = [pMv[0], pMv[1]];
                UpdateP16x8MotionInfo(pCurDqLayer, pDec, pMotionVector, pRefIndex, listIdx, iPartIdx as usize, ref_idx, mv2.as_ptr());
                UpdateP16x8MvdCabac(pCurDqLayer, pMvdCache, iPartIdx as i32, pMvd.as_ptr(), listIdx as i8);
            }
        }
    } else if IS_INTER_8x16(mbType) {
        let mut ref_idx_list = [[REF_NOT_IN_LIST; 2]; LIST_A];
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                let iPartIdx = (i << 2) as i32;
                let mut ref_idx: i8 = REF_NOT_IN_LIST;
                if IS_DIR(mbType, i, listIdx) {
                    let err = ParseRefIdxCabac(
                        pCtx,
                        pNeighAvail,
                        pNonZeroCount,
                        pRefIndex,
                        Some(pDirect),
                        listIdx as i32,
                        iPartIdx,
                        pRefCount[listIdx],
                        0,
                        &mut ref_idx,
                    );
                    if err != ERR_NONE {
                        return err;
                    }
                    check_ref_idx!(listIdx, ref_idx);
                }
                UpdateP8x16RefIdxCabac(pCurDqLayer, pDec, pRefIndex, iPartIdx, ref_idx, listIdx as i8);
                ref_idx_list[listIdx][i] = ref_idx;
            }
        }
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                let iPartIdx = (i << 2) as i32;
                let ref_idx = ref_idx_list[listIdx][i];
                if IS_DIR(mbType, i, listIdx) {
                    let mut mvp: [i16; 2] = [0, 0];
                    PredInter8x16Mv(pMotionVector, pRefIndex, listIdx, iPartIdx as usize, ref_idx, &mut mvp);
                    pMv[0] = mvp[0];
                    pMv[1] = mvp[1];
                    let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 0, &mut pMvd[0]);
                    if err != ERR_NONE { return err; }
                    err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 1, &mut pMvd[1]);
                    if err != ERR_NONE { return err; }
                    pMv[0] += pMvd[0];
                    pMv[1] += pMvd[1];
                } else {
                    pMv[0] = 0; pMv[1] = 0;
                    pMvd[0] = 0; pMvd[1] = 0;
                }
                let mv2: [i16; 2] = [pMv[0], pMv[1]];
                UpdateP8x16MotionInfo(pCurDqLayer, pDec, pMotionVector, pRefIndex, listIdx, iPartIdx as usize, ref_idx, mv2.as_ptr());
                UpdateP8x16MvdCabac(pCurDqLayer, pMvdCache, iPartIdx as i32, pMvd.as_ptr(), listIdx as i8);
            }
        }
    } else if IS_Inter_8x8(mbType) {
        let mut pSubPartCount = [0i8; 4];
        let mut pPartW = [0i8; 4];
        let mut uiSubMbType: u32 = 0;
        // sub_mb_type, partition
        let mut pMvDirect = [[0i16; 2]; LIST_A];
        if (*pCtx).sRefPic.pRefList[LIST_1][0].is_none() {
            // "Colocated Ref Picture for B-Slice is lost, B-Slice decoding cannot be continued!"
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
        }
        let bIsLongRef = (*ref_pic(pCtx, LIST_1, 0)).bIsLongRef;
        let ref0Count = WELS_MIN(
            pSliceHeader.uiRefCount[LIST_0],
            (*pCtx).sRefPic.uiRefCount[LIST_0] as i32,
        );
        let mut has_direct_called = false;
        let mut directSubMbType: SubMbType = 0;

        // T5.I1: one window borrow for the flag across the parse loop. `pSubMbType`
        // cannot take one here — `PredMvBDirectSpatial` and `PredBDirectTemporal`
        // write `grid.sub_mb_type[iMbXy]` themselves (`mv_pred.rs:1035`, `:1130`) —
        // so its window is per iteration below, and one shared window covers the
        // read-only loops after.
        let pNoSubMbPartSizeLessThan8x8Flag = (*pCurDqLayer)
            .grid
            .no_sub_mb_part_size_less_than8x8_flag
            .get_mut(iMbXy);

        for i in 0..4usize {
            let err = ParseBSubMBTypeCabac(pCtx, pNeighAvail, &mut uiSubMbType);
            if err != ERR_NONE {
                return err;
            }
            if uiSubMbType >= 13 {
                // invalid sub_mb_type
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_SUB_MB_TYPE);
            }
            pSubPartCount[i] = g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iPartCount;
            pPartW[i] = g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iPartWidth;

            // Need modification when B picture add in, reference to 7.3.5
            if pSubPartCount[i] > 1 {
                *pNoSubMbPartSizeLessThan8x8Flag = false;
            }

            if IS_DIRECT(g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iType) {
                if !has_direct_called {
                    if pSliceHeader.iDirectSpatialMvPredFlag != 0 {
                        let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                            pCtx,
                            &mut pMvDirect,
                            &mut iRef,
                            &mut directSubMbType,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                    } else {
                        // temporal direct mode
                        let ret = crate::decoder::mv_pred::PredBDirectTemporal(
                            pCtx,
                            &mut pMvDirect,
                            &mut iRef,
                            &mut directSubMbType,
                        );
                        if ret != ERR_NONE {
                            return ret;
                        }
                    }
                    has_direct_called = true;
                }
                let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy);
                pSubMbType[i] = directSubMbType;
                if IS_SUB_4x4(pSubMbType[i]) {
                    pSubPartCount[i] = 4;
                    pPartW[i] = 1;
                }
            } else {
                (*(*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy))[i] =
                    g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iType;
            }
        }

        // T5.I1: nothing below writes this family, and `FillSpatialDirect8x8Mv`,
        // `FillTemporalDirect8x8Mv`, `Update8x8RefIdx` and `PredMv` reach the layer
        // but not this array. One shared window for the three loops that follow.
        let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get(iMbXy);

        for i in 0..4usize {
            // Direct 8x8 Ref and mv
            let iIdx8 = (i << 2) as i16;
            if IS_DIRECT(pSubMbType[i]) {
                if pSliceHeader.iDirectSpatialMvPredFlag != 0 {
                    FillSpatialDirect8x8Mv(
                        pCurDqLayer,
                        pDec,
                        iIdx8,
                        pSubPartCount[i],
                        pPartW[i],
                        directSubMbType,
                        bIsLongRef,
                        pMvDirect.as_mut_ptr(),
                        iRef.as_mut_ptr(),
                        Some(&mut *pMotionVector),
                        Some(&mut *pMvdCache),
                    );
                } else {
                    let mut mvColoc = (*pCurDqLayer).iColocMv[LIST_0].as_mut_ptr();
                    iRef[LIST_1] = 0;
                    iRef[LIST_0] = 0;
                    let uiColoc4Idx = g_kuiScan4[iIdx8 as usize] as usize;
                    if (*pCurDqLayer).iColocIntra[uiColoc4Idx] == 0 {
                        iRef[LIST_0] = 0;
                        let colocRefIndexL0 = (*pCurDqLayer).iColocRefIndex[LIST_0][uiColoc4Idx];
                        if colocRefIndexL0 >= 0 {
                            iRef[LIST_0] = crate::decoder::mv_pred::MapColToList0(
                                pCtx,
                                colocRefIndexL0,
                                ref0Count,
                            );
                        } else {
                            mvColoc = (*pCurDqLayer).iColocMv[LIST_1].as_mut_ptr();
                        }
                    }
                    Update8x8RefIdx(pCurDqLayer, pDec, iIdx8, LIST_0, iRef[LIST_0]);
                    Update8x8RefIdx(pCurDqLayer, pDec, iIdx8, LIST_1, iRef[LIST_1]);
                    UpdateP8x8RefCacheIdxCabac(pRefIndex, iIdx8, LIST_0 as i32, iRef[LIST_0]);
                    UpdateP8x8RefCacheIdxCabac(pRefIndex, iIdx8, LIST_1 as i32, iRef[LIST_1]);
                    FillTemporalDirect8x8Mv(
                        pCurDqLayer,
                        pDec,
                        iIdx8,
                        pSubPartCount[i],
                        pPartW[i],
                        directSubMbType,
                        iRef.as_mut_ptr(),
                        mvColoc,
                        Some(&mut *pMotionVector),
                        Some(&mut *pMvdCache),
                    );
                }
            }
        }

        // ref no-direct
        let mut ref_idx_list = [[REF_NOT_IN_LIST; 4]; LIST_A];
        for listIdx in LIST_0..LIST_A {
            for i in 0..4usize {
                let iIdx8 = (i << 2) as i16;
                let subMbType = pSubMbType[i];
                let mut iref: i8 = REF_NOT_IN_LIST;
                if IS_DIRECT(subMbType) {
                    if pSliceHeader.iDirectSpatialMvPredFlag != 0 {
                        Update8x8RefIdx(pCurDqLayer, pDec, iIdx8, listIdx, iRef[listIdx]);
                        ref_idx_list[listIdx][i] = iRef[listIdx];
                    }
                    UpdateP8x8DirectCabac(pCurDqLayer, iIdx8 as i32);
                } else {
                    if IS_DIR(subMbType, 0, listIdx) {
                        let err = ParseRefIdxCabac(
                            pCtx,
                            pNeighAvail,
                            pNonZeroCount,
                            pRefIndex,
                            Some(pDirect),
                            listIdx as i32,
                            iIdx8 as i32,
                            pRefCount[listIdx],
                            1,
                            &mut iref,
                        );
                        if err != ERR_NONE {
                            return err;
                        }
                        check_ref_idx!(listIdx, iref);
                    }
                    Update8x8RefIdx(pCurDqLayer, pDec, iIdx8, listIdx, iref);
                    ref_idx_list[listIdx][i] = iref;
                }
            }
        }

        // mv
        for listIdx in LIST_0..LIST_A {
            for i in 0..4usize {
                let iIdx8 = (i << 2) as i16;
                let subMbType = pSubMbType[i];
                if IS_DIRECT(subMbType) && pSliceHeader.iDirectSpatialMvPredFlag == 0 {
                    continue;
                }
                let iref = ref_idx_list[listIdx][i];
                UpdateP8x8RefCacheIdxCabac(pRefIndex, iIdx8, listIdx as i32, iref);

                if IS_DIRECT(subMbType) {
                    continue;
                }

                let is_dir = IS_DIR(subMbType, 0, listIdx);
                let iPartCount = pSubPartCount[i];
                let iBlockW = pPartW[i];
                for j in 0..iPartCount as usize {
                    let iPartIdx = (i << 2) + j * iBlockW as usize;
                    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
                    let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
                    if is_dir {
                        let mut mvp: [i16; 2] = [0, 0];
                        PredMv(pMotionVector, pRefIndex, listIdx, iPartIdx as usize, iBlockW as usize, iref, &mut mvp);
                        pMv[0] = mvp[0];
                        pMv[1] = mvp[1];
                        let mut err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 0, &mut pMvd[0]);
                        if err != ERR_NONE { return err; }
                        err = ParseMvdInfoCabac(pCtx, pNeighAvail, pRefIndex, pMvdCache, iPartIdx as i32, listIdx as i8, 1, &mut pMvd[1]);
                        if err != ERR_NONE { return err; }
                        pMv[0] += pMvd[0];
                        pMv[1] += pMvd[1];
                    } else {
                        pMv[0] = 0; pMv[1] = 0;
                        pMvd[0] = 0; pMvd[1] = 0;
                    }

                    let pDecMv = (*dec_pic(pCtx)).pMv[listIdx].get_mut(iMbXy).as_mut_ptr();
                    let pLayerMvd = (*pCurDqLayer).grid.mvd[listIdx].get_mut(iMbXy).as_mut_ptr();
                    let mv2: [i16; 2] = [pMv[0], pMv[1]];
                    let mvd2: [i16; 2] = [pMvd[0], pMvd[1]];

                    if IS_SUB_8x8(subMbType) {
                        // MB_TYPE_8x8: duplicate the pair, then store 8 bytes (two blocks)
                        pMv[2] = pMv[0]; pMv[3] = pMv[1];
                        pMvd[2] = pMvd[0]; pMvd[3] = pMvd[1];
                        *pDecMv.add(iScan4Idx) = mv2;
                        *pDecMv.add(iScan4Idx + 1) = mv2;
                        *pDecMv.add(iScan4Idx + 4) = mv2;
                        *pDecMv.add(iScan4Idx + 5) = mv2;
                        *pLayerMvd.add(iScan4Idx) = mvd2;
                        *pLayerMvd.add(iScan4Idx + 1) = mvd2;
                        *pLayerMvd.add(iScan4Idx + 4) = mvd2;
                        *pLayerMvd.add(iScan4Idx + 5) = mvd2;
                        pMotionVector[listIdx][iCacheIdx] = mv2;
                        pMotionVector[listIdx][iCacheIdx + 1] = mv2;
                        pMotionVector[listIdx][iCacheIdx + 6] = mv2;
                        pMotionVector[listIdx][iCacheIdx + 7] = mv2;
                        pMvdCache[listIdx][iCacheIdx] = mvd2;
                        pMvdCache[listIdx][iCacheIdx + 1] = mvd2;
                        pMvdCache[listIdx][iCacheIdx + 6] = mvd2;
                        pMvdCache[listIdx][iCacheIdx + 7] = mvd2;
                    } else if IS_SUB_4x4(subMbType) {
                        // MB_TYPE_4x4
                        *pDecMv.add(iScan4Idx) = mv2;
                        *pLayerMvd.add(iScan4Idx) = mvd2;
                        pMotionVector[listIdx][iCacheIdx] = mv2;
                        pMvdCache[listIdx][iCacheIdx] = mvd2;
                    } else if IS_SUB_4x8(subMbType) {
                        // MB_TYPE_4x8 5, 7, 9
                        *pDecMv.add(iScan4Idx) = mv2;
                        *pDecMv.add(iScan4Idx + 4) = mv2;
                        *pLayerMvd.add(iScan4Idx) = mvd2;
                        *pLayerMvd.add(iScan4Idx + 4) = mvd2;
                        pMotionVector[listIdx][iCacheIdx] = mv2;
                        pMotionVector[listIdx][iCacheIdx + 6] = mv2;
                        pMvdCache[listIdx][iCacheIdx] = mvd2;
                        pMvdCache[listIdx][iCacheIdx + 6] = mvd2;
                    } else {
                        // MB_TYPE_8x4 4, 6, 8
                        pMv[2] = pMv[0]; pMv[3] = pMv[1];
                        pMvd[2] = pMvd[0]; pMvd[3] = pMvd[1];
                        *pDecMv.add(iScan4Idx) = mv2;
                        *pDecMv.add(iScan4Idx + 1) = mv2;
                        *pLayerMvd.add(iScan4Idx) = mvd2;
                        *pLayerMvd.add(iScan4Idx + 1) = mvd2;
                        pMotionVector[listIdx][iCacheIdx] = mv2;
                        pMotionVector[listIdx][iCacheIdx + 1] = mv2;
                        pMvdCache[listIdx][iCacheIdx] = mvd2;
                        pMvdCache[listIdx][iCacheIdx + 1] = mvd2;
                    }
                }
            }
        }
    }
    ERR_NONE
}

// ============================================================================
// Coded Block Pattern & Delta QP Parsing
// ============================================================================

pub unsafe fn ParseCbpInfoCabac(
    pCtx: PWelsDecoderContext,
    pNeighAvail: *const SWelsNeighAvail,
    uiCbp: &mut u32,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut iIdxA: i32;
    let mut iIdxB: i32;
    let mut pALeftMb = [0i32; 2];
    let mut pBTopMb = [0i32; 2];
    *uiCbp = 0;
    let mut pCbpBit = [0u32; 6];
    let mut iCtxInc: i32;

    pBTopMb[0] = ((*pNeighAvail).iTopAvail != 0
        && (*pNeighAvail).iTopType != MB_TYPE_INTRA_PCM
        && (((*pNeighAvail).iTopCbp & (1 << 2)) == 0)) as i32;
    pBTopMb[1] = ((*pNeighAvail).iTopAvail != 0
        && (*pNeighAvail).iTopType != MB_TYPE_INTRA_PCM
        && (((*pNeighAvail).iTopCbp & (1 << 3)) == 0)) as i32;
    pALeftMb[0] = ((*pNeighAvail).iLeftAvail != 0
        && (*pNeighAvail).iLeftType != MB_TYPE_INTRA_PCM
        && (((*pNeighAvail).iLeftCbp & (1 << 1)) == 0)) as i32;
    pALeftMb[1] = ((*pNeighAvail).iLeftAvail != 0
        && (*pNeighAvail).iLeftType != MB_TYPE_INTRA_PCM
        && (((*pNeighAvail).iLeftCbp & (1 << 3)) == 0)) as i32;

    // left_top 8x8 block
    iCtxInc = pALeftMb[0] + (pBTopMb[0] << 1);
    let mut err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + iCtxInc) as usize),
        &mut pCbpBit[0],
    );
    if err != ERR_NONE {
        return err;
    }
    if pCbpBit[0] != 0 {
        *uiCbp += 0x01;
    }

    // right_top 8x8 block
    iIdxA = (pCbpBit[0] == 0) as i32;
    iCtxInc = iIdxA + (pBTopMb[1] << 1);
    err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + iCtxInc) as usize),
        &mut pCbpBit[1],
    );
    if err != ERR_NONE {
        return err;
    }
    if pCbpBit[1] != 0 {
        *uiCbp += 0x02;
    }

    // left_bottom 8x8 block
    iIdxB = (pCbpBit[0] == 0) as i32;
    iCtxInc = pALeftMb[1] + (iIdxB << 1);
    err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + iCtxInc) as usize),
        &mut pCbpBit[2],
    );
    if err != ERR_NONE {
        return err;
    }
    if pCbpBit[2] != 0 {
        *uiCbp += 0x04;
    }

    // right_bottom 8x8 block
    iIdxB = (pCbpBit[1] == 0) as i32;
    iIdxA = (pCbpBit[2] == 0) as i32;
    iCtxInc = iIdxA + (iIdxB << 1);
    err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + iCtxInc) as usize),
        &mut pCbpBit[3],
    );
    if err != ERR_NONE {
        return err;
    }
    if pCbpBit[3] != 0 {
        *uiCbp += 0x08;
    }

    if (*(*pCtx).pSps).uiChromaFormatIdc == 0 {
        return ERR_NONE;
    }

    // Chroma
    iIdxB = ((*pNeighAvail).iTopAvail != 0
        && ((*pNeighAvail).iTopType == MB_TYPE_INTRA_PCM
            || (((*pNeighAvail).iTopCbp >> 4) != 0))) as i32;
    iIdxA = ((*pNeighAvail).iLeftAvail != 0
        && ((*pNeighAvail).iLeftType == MB_TYPE_INTRA_PCM
            || (((*pNeighAvail).iLeftCbp >> 4) != 0))) as i32;

    iCtxInc = iIdxA + (iIdxB << 1);
    err = DecodeBinCabac(cabac_win, 
        std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
        cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + CTX_NUM_CBP + iCtxInc) as usize),
        &mut pCbpBit[4],
    );
    if err != ERR_NONE {
        return err;
    }

    if pCbpBit[4] != 0 {
        iIdxB = ((*pNeighAvail).iTopAvail != 0
            && ((*pNeighAvail).iTopType == MB_TYPE_INTRA_PCM
                || (((*pNeighAvail).iTopCbp >> 4) == 2))) as i32;
        iIdxA = ((*pNeighAvail).iLeftAvail != 0
            && ((*pNeighAvail).iLeftType == MB_TYPE_INTRA_PCM
                || (((*pNeighAvail).iLeftCbp >> 4) == 2))) as i32;
        iCtxInc = iIdxA + (iIdxB << 1);
        err = DecodeBinCabac(cabac_win, 
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            cabac_ctx_base(pCtx).add((NEW_CTX_OFFSET_CBP + 2 * CTX_NUM_CBP + iCtxInc) as usize),
            &mut pCbpBit[5],
        );
        if err != ERR_NONE {
            return err;
        }
        *uiCbp += 1 << (4 + pCbpBit[5]);
    }

    ERR_NONE
}

pub unsafe fn ParseDeltaQpCabac(pCtx: PWelsDecoderContext, iQpDelta: &mut i32) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    let pCurrSlice = &mut (*(*pCtx).pCurDqLayer).sLayerInfo.sSliceInLayer;
    *iQpDelta = 0;
    let pBinCtx = cabac_ctx_base(pCtx).add(NEW_CTX_OFFSET_DELTA_QP as usize);
    let iCtxInc = (pCurrSlice.iLastDeltaQp != 0) as i32;

    let mut err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pBinCtx.add(iCtxInc as usize), &mut uiCode);
    if err != ERR_NONE {
        return err;
    }
    if uiCode != 0 {
        err = DecodeUnaryBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pBinCtx.add(2), 1, &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        uiCode += 1;
        *iQpDelta = ((uiCode + 1) >> 1) as i32;
        if (uiCode & 1) == 0 {
            *iQpDelta = -*iQpDelta;
        }
    }
    pCurrSlice.iLastDeltaQp = *iQpDelta;
    ERR_NONE
}

/// T5.H8: `pCbfDc: *mut u16` and `pMbType: *const u32` were parameters here, and
/// both were shadowed four lines into the body by locals re-deriving the same two
/// expressions the caller had just evaluated to pass them. Dead since the function
/// was written. `pCbfDc`'s had to go with the flip — its only source is a grid
/// array now — and `pMbType`'s went with it because it is the same dead expression
/// at the same call.
pub unsafe fn ParseCbfInfoCabac(
    pNeighAvail: *const SWelsNeighAvail,
    pNzcCache: *const u8,
    pCtx: PWelsDecoderContext,
    iZIndex: i32,
    iResProperty: i32,
    uiCbfBit: &mut u32,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let iCurrBlkXy = (*(*pCtx).pCurDqLayer).iMbXyIndex;
    let mut iTopBlkXy = iCurrBlkXy - (*(*pCtx).pCurDqLayer).iMbWidth;
    let mut iLeftBlkXy = iCurrBlkXy - 1;
    let pMbType = crate::decoder::decoder_core::mb_grid_ptr(&mut (*dec_pic(pCtx)).pMbType, 0);
    *uiCbfBit = 0;
    let mut nA: i8 = IS_INTRA(*pMbType.add(iCurrBlkXy as usize)) as i8;
    let mut nB: i8 = nA;

    if iResProperty == I16_LUMA_DC || iResProperty == CHROMA_DC_U || iResProperty == CHROMA_DC_V {
        if (*pNeighAvail).iTopAvail != 0 {
            nB = (*pMbType.add(iTopBlkXy as usize) == MB_TYPE_INTRA_PCM
                || ((*(*(*pCtx).pCurDqLayer).grid.cbf_dc.get(iTopBlkXy as usize) >> iResProperty) & 1) != 0)
                as i8;
        }
        if (*pNeighAvail).iLeftAvail != 0 {
            nA = (*pMbType.add(iLeftBlkXy as usize) == MB_TYPE_INTRA_PCM
                || ((*(*(*pCtx).pCurDqLayer).grid.cbf_dc.get(iLeftBlkXy as usize) >> iResProperty) & 1) != 0)
                as i8;
        }
        let iCtxInc = (nA as i32) + ((nB as i32) << 1);
        let ctx_offset = NEW_CTX_OFFSET_CBF + g_kBlockCat2CtxOffsetCBF[iResProperty as usize] as i32 + iCtxInc;
        let err = DecodeBinCabac(cabac_win, 
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            cabac_ctx_base(pCtx).add(ctx_offset as usize),
            uiCbfBit,
        );
        if err != ERR_NONE {
            return err;
        }
        if *uiCbfBit != 0 {
            *(*(*pCtx).pCurDqLayer).grid.cbf_dc.get_mut(iCurrBlkXy as usize) |= 1 << iResProperty;
        }
    } else {
        let top_nzc_idx = (g_kCacheNzcScanIdx[iZIndex as usize] - 8) as usize;
        if *pNzcCache.add(top_nzc_idx) != 0xff {
            if g_kTopBlkInsideMb[iZIndex as usize] != 0 {
                iTopBlkXy = iCurrBlkXy;
            }
            nB = (*pNzcCache.add(top_nzc_idx) != 0 || *pMbType.add(iTopBlkXy as usize) == MB_TYPE_INTRA_PCM) as i8;
        }
        let left_nzc_idx = (g_kCacheNzcScanIdx[iZIndex as usize] - 1) as usize;
        if *pNzcCache.add(left_nzc_idx) != 0xff {
            if g_kLeftBlkInsideMb[iZIndex as usize] != 0 {
                iLeftBlkXy = iCurrBlkXy;
            }
            nA = (*pNzcCache.add(left_nzc_idx) != 0 || *pMbType.add(iLeftBlkXy as usize) == MB_TYPE_INTRA_PCM) as i8;
        }
        let iCtxInc = (nA as i32) + ((nB as i32) << 1);
        let ctx_offset = NEW_CTX_OFFSET_CBF + g_kBlockCat2CtxOffsetCBF[iResProperty as usize] as i32 + iCtxInc;
        let err = DecodeBinCabac(cabac_win, 
            std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine),
            cabac_ctx_base(pCtx).add(ctx_offset as usize),
            uiCbfBit,
        );
        if err != ERR_NONE {
            return err;
        }
    }
    ERR_NONE
}

pub unsafe fn ParseSignificantMapCabac(
    mut pSignificantMap: *mut i32,
    iResProperty: i32,
    pCtx: PWelsDecoderContext,
    uiCoeffNum: &mut u32,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    let map_base = if iResProperty == LUMA_DC_AC_8 {
        NEW_CTX_OFFSET_MAP_8x8
    } else {
        NEW_CTX_OFFSET_MAP
    };
    let last_base = if iResProperty == LUMA_DC_AC_8 {
        NEW_CTX_OFFSET_LAST_8x8
    } else {
        NEW_CTX_OFFSET_LAST
    };

    let pMapCtx = cabac_ctx_base(pCtx)
        .add((map_base + g_kBlockCat2CtxOffsetMap[iResProperty as usize] as i32) as usize);
    let pLastCtx = cabac_ctx_base(pCtx)
        .add((last_base + g_kBlockCat2CtxOffsetLast[iResProperty as usize] as i32) as usize);

    *uiCoeffNum = 0;
    let i1 = g_kMaxPos[iResProperty as usize] as i32;

    for i in 0..i1 {
        let iCtx = if iResProperty == LUMA_DC_AC_8 {
            g_kuiIdx2CtxSignificantCoeffFlag8x8[i as usize] as i32
        } else {
            i
        };

        let mut err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pMapCtx.offset(iCtx as isize), &mut uiCode);
        if err != ERR_NONE {
            return err;
        }
        if uiCode != 0 {
            *pSignificantMap = 1;
            pSignificantMap = pSignificantMap.add(1);
            *uiCoeffNum += 1;

            let iLastCtx = if iResProperty == LUMA_DC_AC_8 {
                g_kuiIdx2CtxLastSignificantCoeffFlag8x8[i as usize] as i32
            } else {
                i
            };
            err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pLastCtx.offset(iLastCtx as isize), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                ptr::write_bytes(pSignificantMap, 0, (i1 - i) as usize);
                return ERR_NONE;
            }
        } else {
            *pSignificantMap = 0;
            pSignificantMap = pSignificantMap.add(1);
        }
    }

    *pSignificantMap = 1;
    *uiCoeffNum += 1;

    ERR_NONE
}

pub unsafe fn ParseSignificantCoeffCabac(
    pSignificant: *mut i32,
    iResProperty: i32,
    pCtx: PWelsDecoderContext,
) -> i32 {
    let cabac_win = cabac_rbsp_window(pCtx);
    let mut uiCode: u32 = 0;
    let one_base = if iResProperty == LUMA_DC_AC_8 {
        NEW_CTX_OFFSET_ONE_8x8
    } else {
        NEW_CTX_OFFSET_ONE
    };
    let abs_base = if iResProperty == LUMA_DC_AC_8 {
        NEW_CTX_OFFSET_ABS_8x8
    } else {
        NEW_CTX_OFFSET_ABS
    };

    let pOneCtx = cabac_ctx_base(pCtx)
        .add((one_base + g_kBlockCat2CtxOffsetOne[iResProperty as usize] as i32) as usize);
    let pAbsCtx = cabac_ctx_base(pCtx)
        .add((abs_base + g_kBlockCat2CtxOffsetAbs[iResProperty as usize] as i32) as usize);

    let iMaxType = g_kMaxC2[iResProperty as usize] as i32;
    let mut i = g_kMaxPos[iResProperty as usize] as i32;
    let mut pCoff = pSignificant.offset(i as isize);
    let mut c1: i32 = 1;
    let mut c2: i32 = 0;

    while i >= 0 {
        if *pCoff != 0 {
            let mut err = DecodeBinCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pOneCtx.offset(c1 as isize), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            *pCoff += uiCode as i32;
            if *pCoff == 2 {
                let err = DecodeUEGLevelCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), pAbsCtx.offset(c2 as isize), &mut uiCode);
                if err != (ERR_NONE as u32) {
                    return err as i32;
                }
                *pCoff += uiCode as i32;
                c2 += 1;
                if c2 > iMaxType {
                    c2 = iMaxType;
                }
                c1 = 0;
            } else if c1 != 0 {
                c1 += 1;
                if c1 > 4 {
                    c1 = 4;
                }
            }
            err = DecodeBypassCabac(cabac_win, std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine), &mut uiCode);
            if err != ERR_NONE {
                return err;
            }
            if uiCode != 0 {
                *pCoff = -*pCoff;
            }
        }
        // `wrapping_offset`, not `offset` (F30). The C++ is `pCoff--`
        // (`parse_mb_syn_cabac.cpp:1394`) and its last iteration lands one element
        // *before* `pSignificant`, which `i` then ends the loop on without ever
        // dereferencing. In C that is a benign idiom; in Rust `offset` past the start
        // is UB by the arithmetic alone — F7's class, the one T3.3 deleted from
        // `InitReadBits`. `wrapping_offset` computes the same address and removes the
        // UB, so this is S6 parity and not a repair.
        pCoff = pCoff.wrapping_offset(-1);
        i -= 1;
    }
    ERR_NONE
}

pub unsafe fn ParseResidualBlockCabac8x8(
    _pNeighAvail: *const SWelsNeighAvail,
    pNonZeroCountCache: *mut u8,
    iIndex: i32,
    _iMaxNumCoeff: i32,
    pScanTable: *const u8,
    iResProperty: i32,
    sTCoeff: *mut i16,
    uiQp: u8,
    pCtx: PWelsDecoderContext,
) -> i32 {
    let mut uiTotalCoeffNum: u32 = 0;
    let mut pSignificantMap = [0i32; 64];

    let mut iMbResProperty: i32 = 0;
    let mut iResProp = iResProperty;
    GetMbResProperty(&mut iMbResProperty, &mut iResProp, false);

    let pDeQuantMul: *const u16 = if (*pCtx).bUseScalingList {
        (*pCtx).pDequant_coeff8x8[(iMbResProperty - 6) as usize]
            .add(uiQp as usize) as *const u16
    } else {
        g_kuiDequantCoeff8x8[uiQp as usize].as_ptr()
    };

    let mut err = ParseSignificantMapCabac(pSignificantMap.as_mut_ptr(), iResProp, pCtx, &mut uiTotalCoeffNum);
    if err != ERR_NONE {
        return err;
    }
    err = ParseSignificantCoeffCabac(pSignificantMap.as_mut_ptr(), iResProp, pCtx);
    if err != ERR_NONE {
        return err;
    }

    *pNonZeroCountCache.add(g_kCacheNzcScanIdx[iIndex as usize] as usize) = uiTotalCoeffNum as u8;
    *pNonZeroCountCache.add(g_kCacheNzcScanIdx[(iIndex + 1) as usize] as usize) = uiTotalCoeffNum as u8;
    *pNonZeroCountCache.add(g_kCacheNzcScanIdx[(iIndex + 2) as usize] as usize) = uiTotalCoeffNum as u8;
    *pNonZeroCountCache.add(g_kCacheNzcScanIdx[(iIndex + 3) as usize] as usize) = uiTotalCoeffNum as u8;

    if uiTotalCoeffNum == 0 {
        return ERR_NONE;
    }

    if iResProp == LUMA_DC_AC_8 {
        let qp_shift = uiQp / 6;
        for j in 0..64 {
            if pSignificantMap[j] != 0 {
                let i = *pScanTable.add(j) as usize;
                let dequant_val = *pDeQuantMul.add(i) as i32;
                let sig_val = pSignificantMap[j];
                *sTCoeff.add(i) = if uiQp >= 36 {
                    ((sig_val * dequant_val) * (1 << (qp_shift - 6))) as i16
                } else {
                    ((sig_val * dequant_val + (1 << (5 - qp_shift))) >> (6 - qp_shift)) as i16
                };
            }
        }
    }

    ERR_NONE
}

pub unsafe fn ParseResidualBlockCabac(
    pNeighAvail: *const SWelsNeighAvail,
    pNonZeroCountCache: *mut u8,
    iIndex: i32,
    _iMaxNumCoeff: i32,
    pScanTable: *const u8,
    iResProperty: i32,
    sTCoeff: *mut i16,
    uiQp: u8,
    pCtx: PWelsDecoderContext,
) -> i32 {
    let mut uiTotalCoeffNum: u32 = 0;
    let mut uiCbpBit: u32 = 0;
    let mut pSignificantMap = [0i32; 16];

    let mut iMbResProperty: i32 = 0;
    let mut iResProp = iResProperty;
    GetMbResProperty(&mut iMbResProperty, &mut iResProp, false);

    let pDeQuantMul: *const u16 = if (*pCtx).bUseScalingList {
        (*pCtx).pDequant_coeff4x4[iMbResProperty as usize]
            .add(uiQp as usize) as *const u16
    } else {
        g_kuiDequantCoeff[uiQp as usize].as_ptr()
    };

    let mut err = ParseCbfInfoCabac(pNeighAvail, pNonZeroCountCache, pCtx, iIndex, iResProp, &mut uiCbpBit);
    if err != ERR_NONE {
        return err;
    }
    if uiCbpBit != 0 {
        err = ParseSignificantMapCabac(pSignificantMap.as_mut_ptr(), iResProp, pCtx, &mut uiTotalCoeffNum);
        if err != ERR_NONE {
            return err;
        }
        err = ParseSignificantCoeffCabac(pSignificantMap.as_mut_ptr(), iResProp, pCtx);
        if err != ERR_NONE {
            return err;
        }
    }

    let iCurNzCacheIdx = g_kCacheNzcScanIdx[iIndex as usize] as usize;
    *pNonZeroCountCache.add(iCurNzCacheIdx) = uiTotalCoeffNum as u8;
    if uiTotalCoeffNum == 0 {
        return ERR_NONE;
    }

    if iResProp == I16_LUMA_DC {
        for j in 0..16 {
            let scan_idx = *pScanTable.add(j) as usize;
            *sTCoeff.add(scan_idx) = pSignificantMap[j] as i16;
        }
        WelsLumaDcDequantIdct(sTCoeff, uiQp as i32, pCtx);
    } else if iResProp == CHROMA_DC_U || iResProp == CHROMA_DC_V {
        for j in 0..4 {
            let scan_idx = *pScanTable.add(j) as usize;
            *sTCoeff.add(scan_idx) = pSignificantMap[j] as i16;
        }
        WelsChromaDcIdct(sTCoeff);
        let dequant_mul0 = *pDeQuantMul as i64;
        if !(*pCtx).bUseScalingList {
            for j in 0..4 {
                let scan_idx = *pScanTable.add(j) as usize;
                let val = *sTCoeff.add(scan_idx) as i64;
                *sTCoeff.add(scan_idx) = ((val * dequant_mul0) >> 1) as i16;
            }
        } else {
            for j in 0..4 {
                let scan_idx = *pScanTable.add(j) as usize;
                let val = *sTCoeff.add(scan_idx) as i64;
                *sTCoeff.add(scan_idx) = ((val * dequant_mul0) >> 5) as i16;
            }
        }
    } else {
        for j in 0..16 {
            if pSignificantMap[j] != 0 {
                let scan_idx = *pScanTable.add(j) as usize;
                let sig_val = pSignificantMap[j] as i64;
                if !(*pCtx).bUseScalingList {
                    let mul = *pDeQuantMul.add(scan_idx & 0x07) as i32;
                    *sTCoeff.add(scan_idx) = (pSignificantMap[j] * mul) as i16;
                } else {
                    let mul = *pDeQuantMul.add(scan_idx) as i64;
                    *sTCoeff.add(scan_idx) = (((sig_val * mul) + 8) >> 4) as i16;
                }
            }
        }
    }
    ERR_NONE
}

pub unsafe fn ParseIPCMInfoCabac(pCtx: PWelsDecoderContext) -> i32 {
    let pCabacDecEngine = std::ptr::addr_of_mut!((*pCtx).sCabacDecEngine);
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pBsAux = &mut *crate::decoder::bit_stream::slice_bit_reader(pCtx);
    let pDec = dec_pic(pCtx);
    let iDstStrideLuma = (*pDec).linesize(0);
    let iDstStrideChroma = (*pDec).linesize(1);
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    let iMbOffsetLuma = (iMbX + iMbY * iDstStrideLuma) << 4;
    let iMbOffsetChroma = (iMbX + iMbY * iDstStrideChroma) << 3;

    let mut pMbDstY = (*dec_pic(pCtx)).data_ptr(0).add(iMbOffsetLuma as usize);
    let mut pMbDstU = (*dec_pic(pCtx)).data_ptr(1).add(iMbOffsetChroma as usize);
    let mut pMbDstV = (*dec_pic(pCtx)).data_ptr(2).add(iMbOffsetChroma as usize);

    *(*pDec).pMbType.get_mut(iMbXy) = MB_TYPE_INTRA_PCM;
    RestoreCabacDecEngineToBS(pCabacDecEngine, pBsAux);

    // `pEndBuf - pCurBuf` becomes `len - pos`. F4's off-by-ones are load-bearing, so
    // the comparison keeps its exact shape.
    let iBytesLeft = pBsAux.cursor.len() as isize - pBsAux.cursor.pos() as isize;
    if iBytesLeft < 384 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_CABAC_NO_BS_TO_READ);
    }
    let iPcmStart = pBsAux.cursor.pos();
    let mut pPtrSrc = (*pCtx).sRawData.window_from(pBsAux.start)[iPcmStart..].as_ptr();
    if !(*(*pCtx).pParam).bParseOnly {
        for _ in 0..16 {
            ptr::copy_nonoverlapping(pPtrSrc, pMbDstY, 16);
            pMbDstY = pMbDstY.add(iDstStrideLuma as usize);
            pPtrSrc = pPtrSrc.add(16);
        }
        for _ in 0..8 {
            ptr::copy_nonoverlapping(pPtrSrc, pMbDstU, 8);
            pMbDstU = pMbDstU.add(iDstStrideChroma as usize);
            pPtrSrc = pPtrSrc.add(8);
        }
        for _ in 0..8 {
            ptr::copy_nonoverlapping(pPtrSrc, pMbDstV, 8);
            pMbDstV = pMbDstV.add(iDstStrideChroma as usize);
            pPtrSrc = pPtrSrc.add(8);
        }
    }

    pBsAux.cursor.set_pos(iPcmStart + 384);

    *(*pCurDqLayer).grid.luma_qp.get_mut(iMbXy) = 0;
    let pChromaQp = (*pCurDqLayer).grid.chroma_qp.get_mut(iMbXy);
    pChromaQp[0] = 0;
    pChromaQp[1] = 0;
    (*pCurDqLayer).grid.nzc.get_mut(iMbXy).fill(16);

    let (buf, cursor) = pBsAux.split(&(*pCtx).sRawData);
    let mut err = InitReadBits(buf, cursor, 1);
    if err != ERR_NONE {
        return err;
    }
    err = InitCabacDecEngineFromBS(pCabacDecEngine, pBsAux, &(*pCtx).sRawData);
    if err != ERR_NONE {
        return err;
    }

    ERR_NONE
}
