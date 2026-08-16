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

//! # Macroblock Syntax Parsing & CAVLC Entropy Decoding
//!
//! Translated from `codec/decoder/core/src/parse_mb_syn_cavlc.cpp` and
//! `codec/decoder/core/inc/parse_mb_syn_cavlc.h`.
//!
//! Implements macroblock-level syntax parsing, context-adaptive variable-length
//! decoding (CAVLC), neighbor availability derivation, intra prediction mode
//! verification, and inter-frame motion information parsing for H.264 / AVC decoding.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use crate::safe::bits::BsCursor;

// ============================================================================
// Constants & Error Codes
// ============================================================================

pub const MAX_LEVEL_PREFIX: i32 = 15;
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MV_A: usize = 2;

pub const REF_NOT_AVAIL: i8 = -2;
pub const REF_NOT_IN_LIST: i8 = -1;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_SLICE_DATA: i32 = 6;
pub const ERR_LEVEL_MB_DATA: i32 = 7;

pub const ERR_INVALID_INTRA4X4_MODE: i32 = -1;
pub const ERR_INFO_INVALID_SUB_MB_TYPE: i32 = 1037;
pub const ERR_INFO_INVALID_REF_INDEX: i32 = 1040;
pub const ERR_INFO_CAVLC_INVALID_LEVEL: i32 = 1044;
pub const ERR_INFO_CAVLC_INVALID_TOTAL_COEFF_OR_TRAILING_ONES: i32 = 1045;
pub const ERR_INFO_CAVLC_INVALID_ZERO_LEFT: i32 = 1046;
pub const ERR_INFO_CAVLC_INVALID_RUN_BEFORE: i32 = 1047;
pub const ERR_INFO_INVALID_I4x4_PRED_MODE: i32 = 1050;
pub const ERR_INFO_INVALID_I16x16_PRED_MODE: i32 = 1051;
pub const ERR_INFO_INVALID_I_CHROMA_PRED_MODE: i32 = 1052;
pub const ERR_INFO_UNSUPPORTED_ILP: i32 = 1064;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = 1075;

pub const dsBitstreamError: i32 = 0x04;
pub const ERROR_CON_DISABLE: i32 = 0;

#[inline(always)]
pub const fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

// AVC Macroblock Types
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
pub const MB_TYPE_INTER: u32 =
    MB_TYPE_16x16 | MB_TYPE_16x8 | MB_TYPE_8x16 | MB_TYPE_8x8 | MB_TYPE_8x8_REF0 | MB_TYPE_SKIP | MB_TYPE_DIRECT;

#[inline(always)]
pub const fn IS_INTRA4x4(t: u32) -> bool {
    t == MB_TYPE_INTRA4x4
}
#[inline(always)]
pub const fn IS_INTRA8x8(t: u32) -> bool {
    t == MB_TYPE_INTRA8x8
}
#[inline(always)]
pub const fn IS_INTRANxN(t: u32) -> bool {
    (t & (MB_TYPE_INTRA4x4 | MB_TYPE_INTRA8x8)) != 0
}
#[inline(always)]
pub const fn IS_INTRA16x16(t: u32) -> bool {
    (t & MB_TYPE_INTRA16x16) != 0
}
#[inline(always)]
pub const fn IS_INTRA(t: u32) -> bool {
    (t & MB_TYPE_INTRA) != 0
}
#[inline(always)]
pub const fn IS_INTER(t: u32) -> bool {
    (t & MB_TYPE_INTER) != 0
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
    (t & (MB_TYPE_8x8 | MB_TYPE_8x8_REF0)) != 0
}
#[inline(always)]
pub const fn IS_DIRECT(t: u32) -> bool {
    (t & MB_TYPE_DIRECT) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_8x4(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_8x4) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x8(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_4x8) != 0
}
#[inline(always)]
pub const fn IS_SUB_4x4(sub_type: u32) -> bool {
    (sub_type & SUB_MB_TYPE_4x4) != 0
}
#[inline(always)]
pub const fn IS_DIR(a: u32, part: usize, list: usize) -> bool {
    (a & (MB_TYPE_P0L0 << (part + 2 * list))) != 0
}

// Intra prediction mode identifiers
pub const I16_PRED_V: i8 = 0;
pub const I16_PRED_H: i8 = 1;
pub const I16_PRED_DC: i8 = 2;
pub const I16_PRED_P: i8 = 3;
pub const I16_PRED_DC_L: i8 = 4;
pub const I16_PRED_DC_T: i8 = 5;
pub const I16_PRED_DC_128: i8 = 6;
pub const MAX_PRED_MODE_ID_I16x16: i8 = 3;

pub const I4_PRED_V: i8 = 0;
pub const I4_PRED_H: i8 = 1;
pub const I4_PRED_DC: i8 = 2;
pub const I4_PRED_DDL: i8 = 3;
pub const I4_PRED_DDR: i8 = 4;
pub const I4_PRED_VR: i8 = 5;
pub const I4_PRED_HD: i8 = 6;
pub const I4_PRED_VL: i8 = 7;
pub const I4_PRED_HU: i8 = 8;
pub const I4_PRED_DC_L: i8 = 9;
pub const I4_PRED_DC_T: i8 = 10;
pub const I4_PRED_DC_128: i8 = 11;
pub const I4_PRED_DDL_TOP: i8 = 12;
pub const I4_PRED_VL_TOP: i8 = 13;
pub const MAX_PRED_MODE_ID_I4x4: i8 = 8;

pub const C_PRED_DC: i8 = 0;
pub const C_PRED_H: i8 = 1;
pub const C_PRED_V: i8 = 2;
pub const C_PRED_P: i8 = 3;
pub const C_PRED_DC_L: i8 = 4;
pub const C_PRED_DC_T: i8 = 5;
pub const C_PRED_DC_128: i8 = 6;

// Residual block properties
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

pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;

// ============================================================================
// Core Data Structures
// ============================================================================

/// 32-bit big-endian bit window cache for CAVLC parsing
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagReadBitsCache {
    pub uiCache32Bit: u32,
    pub uiRemainBits: u8,
    pub pBuf: *mut u8,
}
pub type SReadBitsCache = TagReadBitsCache;

impl Default for TagReadBitsCache {
    fn default() -> Self {
        Self {
            uiCache32Bit: 0,
            uiRemainBits: 0,
            pBuf: std::ptr::null_mut(),
        }
    }
}

/// Neighbor availability tracking descriptor
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsNeighAvail {
    // Field order matches `SWelsNeighAvail` in
    // `codec/decoder/core/inc/mb_cache.h` — this struct is shared (via raw
    // pointer casts) between the CAVLC, CABAC and slice-decode modules.
    pub iTopAvail: i32,
    pub iLeftAvail: i32,
    pub iRightTopAvail: i32,
    pub iLeftTopAvail: i32,

    pub iLeftType: u32,
    pub iTopType: u32,
    pub iLeftTopType: u32,
    pub iRightTopType: u32,

    pub iTopCbp: u8,
    pub iLeftCbp: u8,
    pub iDummy: [u8; 2],
}
// T5.W10: `pub type PWelsNeighAvail = *mut SWelsNeighAvail;` sat here and has no
// user left — the struct is a stack local of `decode_slice.rs` threaded down, so
// every consumer takes a borrow now. S18's shape, found at the definition.

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SI16PredInfo {
    pub iPredMode: i8,
    pub iLeftAvail: i8,
    pub iTopAvail: i8,
    pub iLeftTopAvail: i8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SI4PredInfo {
    pub iPredMode: i8,
    pub iLeftAvail: i8,
    pub iTopAvail: i8,
    pub iLeftTopAvail: i8,
}

/// VLC lookup table pointers.
/// Matches `SVlcTable` in `codec/decoder/core/inc/vlc_decoder.h`:
/// `const uint8_t (*kpCoeffTokenVlcTable[4][8])[2];` etc. Each pointer refers
/// to a table of `[value, bit-count]` pairs of varying length.
#[repr(C)]
pub struct SVlcTable {
    pub kpCoeffTokenVlcTable: [[*const [u8; 2]; 8]; 4],
    pub kpChromaCoeffTokenVlcTable: *const [u8; 2],
    pub kpZeroTable: [*const [u8; 2]; 7],
    pub kpTotalZerosTable: [[*const [u8; 2]; 15]; 2],
}

// T5.W4 (W6, family 8, partial): the CAVLC leaf parameters became borrows.
//
//   * `pBitsCache` is `&mut SReadBitsCache` at nine signatures. The type never
//     leaves this module — it is a local of `WelsResidualBlockCavlc` and
//     `…8x8`, built and passed down — and every call site already spelled the
//     argument `&mut sReadBitsCache`, so the flip changed no call site at all.
//   * `pVlcTable` is `&SVlcTable` at nine. **Shared, because nothing writes one
//     outside `InitVlcTable` below** — grep-verified over the crate — so the three
//     `decode_slice.rs` derivations off `(*pCtx).pVlcTable` are shared borrows now.
//   * `kpZigzagTable` is `&[u8]`, and its span is exact rather than open. The
//     callers pass `&g_kuiZigzagScan[max_idx..]` where they passed
//     `.as_ptr().add(max_idx)`, and the length is provably enough: the callee
//     indexes `0..iMaxNumCoeff` with `iMaxNumCoeff = iScanIdxEnd - max_idx + 1`,
//     and `uiScanIdxEnd` is a **4-bit** slice-header field (`decoder_core.rs:2692`)
//     that the port then rejects unless it reads exactly 15 (`:2696`), every other
//     path assigning the literal 15. So `max_idx + iMaxNumCoeff - 1 <= 15 < 16`,
//     the bound the raw form argued in prose is now the type's, and no input can
//     reach the panic. S9's exact-span trim, at table scale.
//
// **What is left, and why it is not a signature problem**: nine of these functions
// are still `unsafe fn`, and every one of them is unsafe for `SVlcTable`'s own
// fields — `[[*const [u8; 2]; 8]; 4]` and friends, raw because the sub-tables have
// *varying lengths* and the C indexes them by a computed VLC bucket. Converting
// those is a table-representation change, not a parameter flip, and it is the whole
// of what stands between this module and the lint. Named here so the next session
// costs it rather than rediscovering it.

/// Matches `InitVlcTable` in `codec/decoder/core/inc/vlc_decoder.h`.
pub fn InitVlcTable(pVlcTable: &mut SVlcTable) {
    use crate::decoder::vlc_tables::*;
    pVlcTable.kpChromaCoeffTokenVlcTable = g_kuiVlcChromaTable.as_ptr();

    pVlcTable.kpCoeffTokenVlcTable = [[std::ptr::null(); 8]; 4];
    pVlcTable.kpCoeffTokenVlcTable[0][0] = g_kuiVlcTable_0.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[0][1] = g_kuiVlcTable_1.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[0][2] = g_kuiVlcTable_2.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[0][3] = g_kuiVlcTable_3.as_ptr();

    pVlcTable.kpCoeffTokenVlcTable[1][0] = g_kuiVlcTable_0_0.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[1][1] = g_kuiVlcTable_0_1.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[1][2] = g_kuiVlcTable_0_2.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[1][3] = g_kuiVlcTable_0_3.as_ptr();

    pVlcTable.kpCoeffTokenVlcTable[2][0] = g_kuiVlcTable_1_0.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[2][1] = g_kuiVlcTable_1_1.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[2][2] = g_kuiVlcTable_1_2.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[2][3] = g_kuiVlcTable_1_3.as_ptr();

    pVlcTable.kpCoeffTokenVlcTable[3][0] = g_kuiVlcTable_2_0.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][1] = g_kuiVlcTable_2_1.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][2] = g_kuiVlcTable_2_2.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][3] = g_kuiVlcTable_2_3.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][4] = g_kuiVlcTable_2_4.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][5] = g_kuiVlcTable_2_5.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][6] = g_kuiVlcTable_2_6.as_ptr();
    pVlcTable.kpCoeffTokenVlcTable[3][7] = g_kuiVlcTable_2_7.as_ptr();

    pVlcTable.kpZeroTable[0] = g_kuiZeroLeftTable0.as_ptr();
    pVlcTable.kpZeroTable[1] = g_kuiZeroLeftTable1.as_ptr();
    pVlcTable.kpZeroTable[2] = g_kuiZeroLeftTable2.as_ptr();
    pVlcTable.kpZeroTable[3] = g_kuiZeroLeftTable3.as_ptr();
    pVlcTable.kpZeroTable[4] = g_kuiZeroLeftTable4.as_ptr();
    pVlcTable.kpZeroTable[5] = g_kuiZeroLeftTable5.as_ptr();
    pVlcTable.kpZeroTable[6] = g_kuiZeroLeftTable6.as_ptr();

    pVlcTable.kpTotalZerosTable[0][0] = g_kuiTotalZerosTable0.as_ptr();
    pVlcTable.kpTotalZerosTable[0][1] = g_kuiTotalZerosTable1.as_ptr();
    pVlcTable.kpTotalZerosTable[0][2] = g_kuiTotalZerosTable2.as_ptr();
    pVlcTable.kpTotalZerosTable[0][3] = g_kuiTotalZerosTable3.as_ptr();
    pVlcTable.kpTotalZerosTable[0][4] = g_kuiTotalZerosTable4.as_ptr();
    pVlcTable.kpTotalZerosTable[0][5] = g_kuiTotalZerosTable5.as_ptr();
    pVlcTable.kpTotalZerosTable[0][6] = g_kuiTotalZerosTable6.as_ptr();
    pVlcTable.kpTotalZerosTable[0][7] = g_kuiTotalZerosTable7.as_ptr();
    pVlcTable.kpTotalZerosTable[0][8] = g_kuiTotalZerosTable8.as_ptr();
    pVlcTable.kpTotalZerosTable[0][9] = g_kuiTotalZerosTable9.as_ptr();
    pVlcTable.kpTotalZerosTable[0][10] = g_kuiTotalZerosTable10.as_ptr();
    pVlcTable.kpTotalZerosTable[0][11] = g_kuiTotalZerosTable11.as_ptr();
    pVlcTable.kpTotalZerosTable[0][12] = g_kuiTotalZerosTable12.as_ptr();
    pVlcTable.kpTotalZerosTable[0][13] = g_kuiTotalZerosTable13.as_ptr();
    pVlcTable.kpTotalZerosTable[0][14] = g_kuiTotalZerosTable14.as_ptr();
    pVlcTable.kpTotalZerosTable[1][0] = g_kuiTotalZerosChromaTable0.as_ptr();
    pVlcTable.kpTotalZerosTable[1][1] = g_kuiTotalZerosChromaTable1.as_ptr();
    pVlcTable.kpTotalZerosTable[1][2] = g_kuiTotalZerosChromaTable2.as_ptr();
}

// Forward definitions matching OpenH264 decoder C ABI structs
pub use crate::decoder::picture::{SPicture, PPicture};
use crate::decoder::decoder_context::{PicRefs, ref_id};

pub use crate::decoder::parameter_sets::{SLevelLimits, SSps, SPps};
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt};



pub use crate::decoder::decoder_core::{
    DqLayerState, PDqLayer, SWelsDecoderContext, PWelsDecoderContext,
    SSlice, PSlice, SLayerInfo, PLayerInfo,
};
pub use crate::decoder::decode_slice::{SPartMbInfo, g_ksInterPSubMbTypeInfo, g_ksInterBSubMbTypeInfo};
pub use crate::decoder::dec_golomb::{g_kuiPrefix8BitsTable};
pub use crate::decoder::decode_slice::{g_kuiCache30ScanIdx, g_kuiCache48CountScan4Idx, g_kuiDequantCoeff, g_kuiScan4, g_kuiScan8};

// **T5.R5 deleted this file's `LD16`/`ST16`/`LD32`/`ST32`/`LD64`/`ST64`.** Their
// twelve uses were the neighbour caches — four non-zero counts and four intra
// prediction modes at a time — over `[i8; 24]` and `[i8; 8]` arrays whose elements are
// single bytes, so each word move became the element copies it was spelling. `LD64`
// and `ST64` had no use in this file at all.

#[inline(always)]
pub fn POP_BUFFER(pBitsCache: &mut SReadBitsCache, iCount: u32) {
    unsafe {
        (*pBitsCache).uiCache32Bit = (*pBitsCache).uiCache32Bit.wrapping_shl(iCount);
        (*pBitsCache).uiRemainBits = (*pBitsCache).uiRemainBits.wrapping_sub(iCount as u8);
    }
}

#[inline(always)]
pub fn SHIFT_BUFFER(pBitsCache: &mut SReadBitsCache) {
    unsafe {
        // Matches the C++ macro: pBuf is advanced FIRST, so the two bytes
        // shifted in are the original pBuf[4] and pBuf[5].
        (*pBitsCache).pBuf = (*pBitsCache).pBuf.add(2);
        let pBuf = (*pBitsCache).pBuf;
        let b2 = *pBuf.add(2) as u32;
        let b3 = *pBuf.add(3) as u32;
        (*pBitsCache).uiRemainBits = (*pBitsCache).uiRemainBits.wrapping_add(16);
        let shift = 32u32.wrapping_sub((*pBitsCache).uiRemainBits as u32);
        (*pBitsCache).uiCache32Bit |= ((b2 << 8) | b3).wrapping_shl(shift);
    }
}

#[inline(always)]
pub fn GetPrefixBits(mut uiValue: u32) -> u32 {
    let mut iNumBit = 0u32;
    if (uiValue & 0xffff0000) != 0 {
        uiValue >>= 16;
        iNumBit += 16;
    }
    if (uiValue & 0xff00) != 0 {
        uiValue >>= 8;
        iNumBit += 8;
    }
    if (uiValue & 0xf0) != 0 {
        uiValue >>= 4;
        iNumBit += 4;
    }
    iNumBit += g_kuiPrefix8BitsTable[(uiValue & 0x0f) as usize];
    32 - iNumBit
}

#[inline(always)]
pub fn wels_non_zero_count_average(nA: i8, nB: i8) -> i8 {
    let mut nC = (nA as i32) + (nB as i32) + 1;
    let shift = if nA != -1 && nB != -1 { 1 } else { 0 };
    nC >>= shift;
    let add = if nA == -1 && nB == -1 { 1 } else { 0 };
    (nC + add) as i8
}

// ============================================================================
// Static Lookup Tables
// ============================================================================

pub static g_kuiNcMapTable: [u8; 17] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];

pub static g_kuiVlcTableNeedMoreBitsThread: [u8; 3] = [4, 4, 8];
pub static g_kuiVlcTableMoreBitsCount0: [u8; 4] = [8, 2, 1, 1];
pub static g_kuiVlcTableMoreBitsCount1: [u8; 4] = [6, 3, 1, 1];
pub static g_kuiVlcTableMoreBitsCount2: [u8; 8] = [2, 2, 2, 2, 1, 1, 1, 1];

pub static g_kuiTotalZerosBitNumMap: [u8; 15] = [9, 6, 6, 5, 5, 6, 6, 6, 6, 5, 4, 4, 3, 2, 1];
pub static g_kuiTotalZerosBitNumChromaMap: [u8; 3] = [3, 2, 1];
pub static g_kuiZeroLeftBitNumMap: [u8; 16] = [0, 1, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3];

pub static g_ksI16PredInfo: [SI16PredInfo; 4] = [
    SI16PredInfo { iPredMode: I16_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: I16_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: I16_PRED_P, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
];

pub static g_ksChromaPredInfo: [SI16PredInfo; 4] = [
    SI16PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI16PredInfo { iPredMode: C_PRED_P, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
];

pub static g_ksI4PredInfo: [SI4PredInfo; 9] = [
    SI4PredInfo { iPredMode: I4_PRED_V, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_H, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: 0, iLeftAvail: 0, iTopAvail: 0, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_DDL, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_DDR, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_VR, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_HD, iLeftAvail: 1, iTopAvail: 1, iLeftTopAvail: 1 },
    SI4PredInfo { iPredMode: I4_PRED_VL, iLeftAvail: 0, iTopAvail: 1, iLeftTopAvail: 0 },
    SI4PredInfo { iPredMode: I4_PRED_HU, iLeftAvail: 1, iTopAvail: 0, iLeftTopAvail: 0 },
];

pub static g_kuiVlcTrailingOneTotalCoeffTable: [[u8; 2]; 62] = [
    [0, 0], [0, 1], [1, 1], [0, 2], [1, 2], [2, 2], [0, 3], [1, 3], [2, 3], [3, 3],
    [0, 4], [1, 4], [2, 4], [3, 4], [0, 5], [1, 5], [2, 5], [3, 5], [0, 6], [1, 6],
    [2, 6], [3, 6], [0, 7], [1, 7], [2, 7], [3, 7], [0, 8], [1, 8], [2, 8], [3, 8],
    [0, 9], [1, 9], [2, 9], [3, 9], [0, 10], [1, 10], [2, 10], [3, 10], [0, 11], [1, 11],
    [2, 11], [3, 11], [0, 12], [1, 12], [2, 12], [3, 12], [0, 13], [1, 13], [2, 13], [3, 13],
    [0, 14], [1, 14], [2, 14], [3, 14], [0, 15], [1, 15], [2, 15], [3, 15], [0, 16], [1, 16],
    [2, 16], [3, 16],
];

// ============================================================================
// Neighborhood Availability & Context Loading
// ============================================================================

/// Evaluates spatial neighborhood availability (Left, Top, Top-Left, Top-Right)
/// for the current macroblock across slice boundaries.
pub unsafe fn GetNeighborAvailMbType(
    pNeighAvail: &mut SWelsNeighAvail,
    pCurDqLayer: Option<&DqLayerState>,
    pDec: PPicture,
) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    unsafe {
        let dq = &*pCurDqLayer;
        let na = &mut *pNeighAvail;

        let iCurXy = dq.iMbXyIndex;
        let iCurX = dq.iMbX;
        let iCurY = dq.iMbY;
        let iCurSliceIdc = *dq.grid.slice_idc.get(iCurXy as usize);

        let mut iLeftXy = 0;
        let mut iTopXy = 0;
        let mut iLeftTopXy = 0;
        let mut iRightTopXy = 0;

        if iCurX != 0 {
            iLeftXy = iCurXy - 1;
            let iLeftSliceIdc = *dq.grid.slice_idc.get(iLeftXy as usize);
            na.iLeftAvail = if iLeftSliceIdc == iCurSliceIdc { 1 } else { 0 };
            na.iLeftCbp = if na.iLeftAvail != 0 { *dq.grid.cbp.get(iLeftXy as usize) as u8 } else { 0 };
        } else {
            na.iLeftAvail = 0;
            na.iLeftTopAvail = 0;
            na.iLeftCbp = 0;
        }

        if iCurY != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
            let iTopSliceIdc = *dq.grid.slice_idc.get(iTopXy as usize);
            na.iTopAvail = if iTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            na.iTopCbp = if na.iTopAvail != 0 { *dq.grid.cbp.get(iTopXy as usize) as u8 } else { 0 };

            if iCurX != 0 {
                iLeftTopXy = iTopXy - 1;
                let iLeftTopSliceIdc = *dq.grid.slice_idc.get(iLeftTopXy as usize);
                na.iLeftTopAvail = if iLeftTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            } else {
                na.iLeftTopAvail = 0;
            }

            if iCurX != (dq.iMbWidth - 1) {
                iRightTopXy = iTopXy + 1;
                let iRightTopSliceIdc = *dq.grid.slice_idc.get(iRightTopXy as usize);
                na.iRightTopAvail = if iRightTopSliceIdc == iCurSliceIdc { 1 } else { 0 };
            } else {
                na.iRightTopAvail = 0;
            }
        } else {
            na.iTopAvail = 0;
            na.iLeftTopAvail = 0;
            na.iRightTopAvail = 0;
            na.iTopCbp = 0;
        }

        na.iLeftType = if na.iLeftAvail != 0 && !pDec.is_null() {
            *(*pDec).pMbType.get(iLeftXy as usize)
        } else {
            0
        };
        na.iTopType = if na.iTopAvail != 0 && !pDec.is_null() {
            *(*pDec).pMbType.get(iTopXy as usize)
        } else {
            0
        };
        na.iLeftTopType = if na.iLeftTopAvail != 0 && !pDec.is_null() {
            *(*pDec).pMbType.get(iLeftTopXy as usize)
        } else {
            0
        };
        na.iRightTopType = if na.iRightTopAvail != 0 && !pDec.is_null() {
            *(*pDec).pMbType.get(iRightTopXy as usize)
        } else {
            0
        };
    }
}

/// Fills the 48-entry local cache `pNonZeroCount` from neighboring macroblocks.
pub unsafe fn WelsFillCacheNonZeroCount(
    pNeighAvail: &SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    pCurDqLayer: Option<&DqLayerState>,
) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    unsafe {
        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let iTopXy: i32;
        let iLeftXy: i32;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
            // T5.R5: the C's `ST32`/`ST16` moved four and two counts at a time; the
            // counts are bytes in a byte array, so the same bytes in the same order
            // are four and two element copies.
            let pTopNzc = dq.grid.nzc.get(iTopXy as usize).as_ptr();
            for k in 0..4 {
                pNonZeroCount[1 + k] = *pTopNzc.add(12 + k) as u8;
            }
            pNonZeroCount[0] = 0;
            pNonZeroCount[5] = 0;
            pNonZeroCount[29] = 0;
            for k in 0..2 {
                pNonZeroCount[6 + k] = *pTopNzc.add(20 + k) as u8;
                pNonZeroCount[30 + k] = *pTopNzc.add(22 + k) as u8;
            }
        } else {
            for k in 0..4 {
                pNonZeroCount[1 + k] = 0xFF;
            }
            pNonZeroCount[0] = 0xFF;
            pNonZeroCount[5] = 0xFF;
            pNonZeroCount[29] = 0xFF;
            for k in 0..2 {
                pNonZeroCount[6 + k] = 0xFF;
                pNonZeroCount[30 + k] = 0xFF;
            }
        }

        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
            let pLeftNzc = dq.grid.nzc.get(iLeftXy as usize).as_ptr();
            pNonZeroCount[8 * 1] = *pLeftNzc.add(3) as u8;
            pNonZeroCount[8 * 2] = *pLeftNzc.add(7) as u8;
            pNonZeroCount[8 * 3] = *pLeftNzc.add(11) as u8;
            pNonZeroCount[8 * 4] = *pLeftNzc.add(15) as u8;

            pNonZeroCount[5 + 8 * 1] = *pLeftNzc.add(17) as u8;
            pNonZeroCount[5 + 8 * 2] = *pLeftNzc.add(21) as u8;
            pNonZeroCount[5 + 8 * 4] = *pLeftNzc.add(19) as u8;
            pNonZeroCount[5 + 8 * 5] = *pLeftNzc.add(23) as u8;
        } else {
            pNonZeroCount[8 * 1] = 0xFF;
            pNonZeroCount[8 * 2] = 0xFF;
            pNonZeroCount[8 * 3] = 0xFF;
            pNonZeroCount[8 * 4] = 0xFF;

            pNonZeroCount[5 + 8 * 1] = 0xFF;
            pNonZeroCount[5 + 8 * 2] = 0xFF;
            pNonZeroCount[5 + 8 * 4] = 0xFF;
            pNonZeroCount[5 + 8 * 5] = 0xFF;
        }
    }
}

pub unsafe fn WelsFillCacheConstrain1IntraNxN(
    pNeighAvail: &SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    pIntraPredMode: *mut i8,
    pCurDqLayer: &DqLayerState,
) {
    unsafe {
        WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, Some(pCurDqLayer));

        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let mut iTopXy = 0;
        let mut iLeftXy = 0;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
        }
        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
        }

        if na.iTopAvail != 0 && IS_INTRANxN(na.iTopType) {
            // T5.R5: four modes, copied as four modes. The `0x02020202`/`0xffffffff`
            // fills below are the same byte four times, which is what the C's word
            // store was spelling.
            let pTopMode = dq.grid.intra_pred_mode.get(iTopXy as usize).as_ptr();
            for k in 0..4 {
                *pIntraPredMode.add(1 + k) = *pTopMode.add(k);
            }
        } else {
            let iPred: i8 = if IS_INTRA16x16(na.iTopType) || (MB_TYPE_INTRA_PCM == na.iTopType) {
                0x02
            } else {
                -1
            };
            for k in 0..4 {
                *pIntraPredMode.add(1 + k) = iPred;
            }
        }

        if na.iLeftAvail != 0 && IS_INTRANxN(na.iLeftType) {
            let pLeftMode = dq.grid.intra_pred_mode.get(iLeftXy as usize).as_ptr();
            *pIntraPredMode.add(0 + 8) = *pLeftMode.add(4);
            *pIntraPredMode.add(0 + 8 * 2) = *pLeftMode.add(5);
            *pIntraPredMode.add(0 + 8 * 3) = *pLeftMode.add(6);
            *pIntraPredMode.add(0 + 8 * 4) = *pLeftMode.add(3);
        } else {
            let iPred: i8 = if IS_INTRA16x16(na.iLeftType) || (MB_TYPE_INTRA_PCM == na.iLeftType) {
                2
            } else {
                -1
            };
            *pIntraPredMode.add(0 + 8) = iPred;
            *pIntraPredMode.add(0 + 8 * 2) = iPred;
            *pIntraPredMode.add(0 + 8 * 3) = iPred;
            *pIntraPredMode.add(0 + 8 * 4) = iPred;
        }
    }
}

pub unsafe fn WelsFillCacheInterCabac(
    pNeighAvail: &SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    iMvArray: &mut [[[i16; 2]; 30]; LIST_A],
    iMvdCache: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIdxArray: &mut [[i8; 30]; LIST_A],
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
) {
    let na = &*pNeighAvail;
    let dq = &*pCurDqLayer;
    let iCurXy = dq.iMbXyIndex as usize;
    let mut iTopXy = 0usize;
    let mut iLeftXy = 0usize;
    let mut iLeftTopXy = 0usize;
    let mut iRightTopXy = 0usize;

    let pSlice = &dq.sLayerInfo.sSliceInLayer;
    let pSliceHeader = &pSlice.sSliceHeaderExt.sSliceHeader;
    let listCount = if pSliceHeader.eSliceType == crate::decoder::slice::EWelsSliceType::B_SLICE {
        2
    } else {
        1
    };

    WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, Some(pCurDqLayer));

    if na.iTopAvail != 0 {
        iTopXy = iCurXy - dq.iMbWidth as usize;
    }
    if na.iLeftAvail != 0 {
        iLeftXy = iCurXy - 1;
    }
    if na.iLeftTopAvail != 0 {
        iLeftTopXy = iCurXy - 1 - dq.iMbWidth as usize;
    }
    if na.iRightTopAvail != 0 {
        iRightTopXy = iCurXy + 1 - dq.iMbWidth as usize;
    }

    for listIdx in 0..listCount {
        if na.iLeftAvail != 0 && IS_INTER(na.iLeftType) {
            let pMv = (*pDec).pMv[listIdx].get(iLeftXy);
            let pMvd = dq.grid.mvd[listIdx].get(iLeftXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iLeftXy);
            iMvArray[listIdx][6] = pMv[3];
            iMvArray[listIdx][12] = pMv[7];
            iMvArray[listIdx][18] = pMv[11];
            iMvArray[listIdx][24] = pMv[15];

            iMvdCache[listIdx][6] = pMvd[3];
            iMvdCache[listIdx][12] = pMvd[7];
            iMvdCache[listIdx][18] = pMvd[11];
            iMvdCache[listIdx][24] = pMvd[15];

            iRefIdxArray[listIdx][6] = pRef[3];
            iRefIdxArray[listIdx][12] = pRef[7];
            iRefIdxArray[listIdx][18] = pRef[11];
            iRefIdxArray[listIdx][24] = pRef[15];
        } else {
            iMvArray[listIdx][6] = [0, 0];
            iMvArray[listIdx][12] = [0, 0];
            iMvArray[listIdx][18] = [0, 0];
            iMvArray[listIdx][24] = [0, 0];

            iMvdCache[listIdx][6] = [0, 0];
            iMvdCache[listIdx][12] = [0, 0];
            iMvdCache[listIdx][18] = [0, 0];
            iMvdCache[listIdx][24] = [0, 0];

            let val = if na.iLeftAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][6] = val;
            iRefIdxArray[listIdx][12] = val;
            iRefIdxArray[listIdx][18] = val;
            iRefIdxArray[listIdx][24] = val;
        }

        if na.iLeftTopAvail != 0 && IS_INTER(na.iLeftTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iLeftTopXy);
            let pMvd = dq.grid.mvd[listIdx].get(iLeftTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iLeftTopXy);
            iMvArray[listIdx][0] = pMv[15];
            iMvdCache[listIdx][0] = pMvd[15];
            iRefIdxArray[listIdx][0] = pRef[15];
        } else {
            iMvArray[listIdx][0] = [0, 0];
            iMvdCache[listIdx][0] = [0, 0];
            let val = if na.iLeftTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][0] = val;
        }

        if na.iTopAvail != 0 && IS_INTER(na.iTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iTopXy);
            let pMvd = dq.grid.mvd[listIdx].get(iTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iTopXy);
            iMvArray[listIdx][1] = pMv[12];
            iMvArray[listIdx][2] = pMv[13];
            iMvArray[listIdx][3] = pMv[14];
            iMvArray[listIdx][4] = pMv[15];

            iMvdCache[listIdx][1] = pMvd[12];
            iMvdCache[listIdx][2] = pMvd[13];
            iMvdCache[listIdx][3] = pMvd[14];
            iMvdCache[listIdx][4] = pMvd[15];

            iRefIdxArray[listIdx][1] = pRef[12];
            iRefIdxArray[listIdx][2] = pRef[13];
            iRefIdxArray[listIdx][3] = pRef[14];
            iRefIdxArray[listIdx][4] = pRef[15];
        } else {
            iMvArray[listIdx][1] = [0, 0];
            iMvArray[listIdx][2] = [0, 0];
            iMvArray[listIdx][3] = [0, 0];
            iMvArray[listIdx][4] = [0, 0];

            iMvdCache[listIdx][1] = [0, 0];
            iMvdCache[listIdx][2] = [0, 0];
            iMvdCache[listIdx][3] = [0, 0];
            iMvdCache[listIdx][4] = [0, 0];

            let val = if na.iTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][1] = val;
            iRefIdxArray[listIdx][2] = val;
            iRefIdxArray[listIdx][3] = val;
            iRefIdxArray[listIdx][4] = val;
        }

        if na.iRightTopAvail != 0 && IS_INTER(na.iRightTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iRightTopXy);
            let pMvd = dq.grid.mvd[listIdx].get(iRightTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iRightTopXy);
            iMvArray[listIdx][5] = pMv[12];
            iMvdCache[listIdx][5] = pMvd[12];
            iRefIdxArray[listIdx][5] = pRef[12];
        } else {
            iMvArray[listIdx][5] = [0, 0];
            iMvdCache[listIdx][5] = [0, 0];
            let val = if na.iRightTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][5] = val;
        }

        iMvArray[listIdx][9] = [0, 0];
        iMvArray[listIdx][21] = [0, 0];
        iMvArray[listIdx][11] = [0, 0];
        iMvArray[listIdx][17] = [0, 0];
        iMvArray[listIdx][23] = [0, 0];
        iMvdCache[listIdx][9] = [0, 0];
        iMvdCache[listIdx][21] = [0, 0];
        iMvdCache[listIdx][11] = [0, 0];
        iMvdCache[listIdx][17] = [0, 0];
        iMvdCache[listIdx][23] = [0, 0];

        iRefIdxArray[listIdx][9] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][21] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][11] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][17] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][23] = REF_NOT_AVAIL;
    }
}

/// Matches `WelsFillCacheInter` in `parse_mb_syn_cavlc.cpp` (CAVLC variant,
/// same as the CABAC variant but without the mvd cache).
pub unsafe fn WelsFillCacheInter(
    pNeighAvail: &SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    iMvArray: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIdxArray: &mut [[i8; 30]; LIST_A],
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
) {
    let na = &*pNeighAvail;
    let dq = &*pCurDqLayer;
    let iCurXy = dq.iMbXyIndex as usize;
    let mut iTopXy = 0usize;
    let mut iLeftXy = 0usize;
    let mut iLeftTopXy = 0usize;
    let mut iRightTopXy = 0usize;

    let pSlice = &dq.sLayerInfo.sSliceInLayer;
    let pSliceHeader = &pSlice.sSliceHeaderExt.sSliceHeader;
    let listCount = if pSliceHeader.eSliceType == crate::decoder::slice::EWelsSliceType::B_SLICE {
        2
    } else {
        1
    };

    WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, Some(pCurDqLayer));

    if na.iTopAvail != 0 {
        iTopXy = iCurXy - dq.iMbWidth as usize;
    }
    if na.iLeftAvail != 0 {
        iLeftXy = iCurXy - 1;
    }
    if na.iLeftTopAvail != 0 {
        iLeftTopXy = iCurXy - 1 - dq.iMbWidth as usize;
    }
    if na.iRightTopAvail != 0 {
        iRightTopXy = iCurXy + 1 - dq.iMbWidth as usize;
    }

    for listIdx in 0..listCount {
        if na.iLeftAvail != 0 && IS_INTER(na.iLeftType) {
            let pMv = (*pDec).pMv[listIdx].get(iLeftXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iLeftXy);
            iMvArray[listIdx][6] = pMv[3];
            iMvArray[listIdx][12] = pMv[7];
            iMvArray[listIdx][18] = pMv[11];
            iMvArray[listIdx][24] = pMv[15];
            iRefIdxArray[listIdx][6] = pRef[3];
            iRefIdxArray[listIdx][12] = pRef[7];
            iRefIdxArray[listIdx][18] = pRef[11];
            iRefIdxArray[listIdx][24] = pRef[15];
        } else {
            iMvArray[listIdx][6] = [0, 0];
            iMvArray[listIdx][12] = [0, 0];
            iMvArray[listIdx][18] = [0, 0];
            iMvArray[listIdx][24] = [0, 0];
            let val = if na.iLeftAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][6] = val;
            iRefIdxArray[listIdx][12] = val;
            iRefIdxArray[listIdx][18] = val;
            iRefIdxArray[listIdx][24] = val;
        }

        if na.iLeftTopAvail != 0 && IS_INTER(na.iLeftTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iLeftTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iLeftTopXy);
            iMvArray[listIdx][0] = pMv[15];
            iRefIdxArray[listIdx][0] = pRef[15];
        } else {
            iMvArray[listIdx][0] = [0, 0];
            let val = if na.iLeftTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][0] = val;
        }

        if na.iTopAvail != 0 && IS_INTER(na.iTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iTopXy);
            iMvArray[listIdx][1] = pMv[12];
            iMvArray[listIdx][2] = pMv[13];
            iMvArray[listIdx][3] = pMv[14];
            iMvArray[listIdx][4] = pMv[15];
            iRefIdxArray[listIdx][1] = pRef[12];
            iRefIdxArray[listIdx][2] = pRef[13];
            iRefIdxArray[listIdx][3] = pRef[14];
            iRefIdxArray[listIdx][4] = pRef[15];
        } else {
            iMvArray[listIdx][1] = [0, 0];
            iMvArray[listIdx][2] = [0, 0];
            iMvArray[listIdx][3] = [0, 0];
            iMvArray[listIdx][4] = [0, 0];
            let val = if na.iTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][1] = val;
            iRefIdxArray[listIdx][2] = val;
            iRefIdxArray[listIdx][3] = val;
            iRefIdxArray[listIdx][4] = val;
        }

        if na.iRightTopAvail != 0 && IS_INTER(na.iRightTopType) {
            let pMv = (*pDec).pMv[listIdx].get(iRightTopXy);
            let pRef = (*pDec).pRefIndex[listIdx].get(iRightTopXy);
            iMvArray[listIdx][5] = pMv[12];
            iRefIdxArray[listIdx][5] = pRef[12];
        } else {
            iMvArray[listIdx][5] = [0, 0];
            let val = if na.iRightTopAvail == 0 { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
            iRefIdxArray[listIdx][5] = val;
        }

        // right-top 4x4 block unavailable positions
        iMvArray[listIdx][9] = [0, 0];
        iMvArray[listIdx][21] = [0, 0];
        iMvArray[listIdx][11] = [0, 0];
        iMvArray[listIdx][17] = [0, 0];
        iMvArray[listIdx][23] = [0, 0];
        iRefIdxArray[listIdx][9] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][21] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][11] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][17] = REF_NOT_AVAIL;
        iRefIdxArray[listIdx][23] = REF_NOT_AVAIL;
    }
}

/// Matches `ParseInterInfo` in `parse_mb_syn_cavlc.cpp`.
pub unsafe fn ParseInterInfo(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    iMvArray: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIdxArray: &mut [[i8; 30]; LIST_A],
    buf: &[u8],
    pBs: &mut BsCursor,
) -> i32 {
    // T5.W8: these two bindings were held across every call in the function that
    // takes the layer, and **every one of their uses is a read of a slice-header
    // scalar** this function never writes (grep-verified over both bodies). The layer
    // flip made the overlap a compile error where the raw pointer had made it
    // invisible (S25); the fix is the bracket maneuver at four scalars — copy once,
    // use everywhere, borrow nothing.
    let bDefaultMotionPredFlag =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultMotionPredFlag;
    let bAdaptiveMotionPredFlag =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveMotionPredFlag;
    let iDirectSpatialMvPredFlag = (*pCurDqLayer)
        .sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag;
    let uiRefCountHdr =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount;
    let ppRefPic = &(*pCtx).sRefPic.pRefList[0];
    let mut iRefCount = [0i32; 2];
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mut iMotionPredFlag = [if bDefaultMotionPredFlag { 1u32 } else { 0u32 }; 4];
    let mut uiCode = 0u32;
    let mut iCode = 0i32;
    iRefCount[0] = uiRefCountHdr[0];
    iRefCount[1] = uiRefCountHdr[1];

    let bIsPending = crate::decoder::decoder_core::GetThreadCount(pCtx) > 1;
    let ec_active = (*pCtx).pParam.is_null()
        || (*(*pCtx).pParam).eEcActiveIdc != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE;

    let mb_type = *(*pDec).pMbType.get(iMbXy);
    match mb_type {
        MB_TYPE_16x16 => {
            let mut iRefIdx = 0i32;
            if bAdaptiveMotionPredFlag {
                let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                iMotionPredFlag[0] = uiCode;
            }
            if iMotionPredFlag[0] == 0 {
                let ret = crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[0], &mut uiCode);
                if ret != 0 {
                    return ret;
                }
                iRefIdx = uiCode as i32;
                if iRefIdx < 0 || iRefIdx >= iRefCount[0] || ppRefPic[iRefIdx as usize].is_none() {
                    (*pCtx).bMbRefConcealed = true;
                    if ec_active {
                        iRefIdx = 0;
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                }
                let pRefPic = pRefs.get(ppRefPic[iRefIdx as usize]);
                (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                    || (*pCtx).bMbRefConcealed
                    || !(!pRefPic.is_null() && ((*pRefPic).bIsComplete || bIsPending));
            } else {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
            }
            let mut iMv = [0i16; 2];
            crate::decoder::mv_pred::PredMv(&*iMvArray, &*iRefIdxArray, 0, 0, 4, iRefIdx as i8, &mut iMv);

            let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
            if ret != 0 {
                return ret;
            }
            iMv[0] = iMv[0].wrapping_add(iCode as i16);
            let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
            if ret != 0 {
                return ret;
            }
            iMv[1] = iMv[1].wrapping_add(iCode as i16);
            crate::decoder::mv_pred::UpdateP16x16MotionInfo(&mut *pCurDqLayer, pDec, 0, iRefIdx as i8, &iMv);
        }
        MB_TYPE_16x8 => {
            let mut iRefIdx = [0i32; 2];
            for i in 0..2 {
                if bAdaptiveMotionPredFlag {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    iMotionPredFlag[i] = uiCode;
                }
            }
            for i in 0..2 {
                if iMotionPredFlag[i] != 0 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                }
                let ret = crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[0], &mut uiCode);
                if ret != 0 {
                    return ret;
                }
                iRefIdx[i] = uiCode as i32;
                if iRefIdx[i] < 0 || iRefIdx[i] >= iRefCount[0] || ppRefPic[iRefIdx[i] as usize].is_none() {
                    (*pCtx).bMbRefConcealed = true;
                    if ec_active {
                        iRefIdx[i] = 0;
                        (*pCtx).iErrorCode |= dsBitstreamError;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                    }
                }
                let pRefPic = pRefs.get(ppRefPic[iRefIdx[i] as usize]);
                (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                    || (*pCtx).bMbRefConcealed
                    || !(!pRefPic.is_null() && ((*pRefPic).bIsComplete || bIsPending));
            }
            for i in 0..2 {
                let mut iMv = [0i16; 2];
                crate::decoder::mv_pred::PredInter16x8Mv(&*iMvArray, &*iRefIdxArray, 0, i << 3, iRefIdx[i] as i8, &mut iMv);

                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[0] = iMv[0].wrapping_add(iCode as i16);
                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[1] = iMv[1].wrapping_add(iCode as i16);
                crate::decoder::mv_pred::UpdateP16x8MotionInfo(
                    &mut *pCurDqLayer,
                    pDec,
                    iMvArray,
                    iRefIdxArray,
                    0,
                    i << 3,
                    iRefIdx[i] as i8,
                    &iMv,
                );
            }
        }
        MB_TYPE_8x16 => {
            let mut iRefIdx = [0i32; 2];
            for i in 0..2 {
                if bAdaptiveMotionPredFlag {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    iMotionPredFlag[i] = uiCode;
                }
            }
            for i in 0..2 {
                if iMotionPredFlag[i] == 0 {
                    let ret = crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[0], &mut uiCode);
                    if ret != 0 {
                        return ret;
                    }
                    iRefIdx[i] = uiCode as i32;
                    if iRefIdx[i] < 0 || iRefIdx[i] >= iRefCount[0] || ppRefPic[iRefIdx[i] as usize].is_none() {
                        (*pCtx).bMbRefConcealed = true;
                        if ec_active {
                            iRefIdx[i] = 0;
                            (*pCtx).iErrorCode |= dsBitstreamError;
                        } else {
                            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                        }
                    }
                    let pRefPic = pRefs.get(ppRefPic[iRefIdx[i] as usize]);
                    (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                        || (*pCtx).bMbRefConcealed
                        || !(!pRefPic.is_null() && ((*pRefPic).bIsComplete || bIsPending));
                } else {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                }
            }
            for i in 0..2 {
                let mut iMv = [0i16; 2];
                crate::decoder::mv_pred::PredInter8x16Mv(&*iMvArray, &*iRefIdxArray, 0, i << 2, iRefIdx[i] as i8, &mut iMv);

                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[0] = iMv[0].wrapping_add(iCode as i16);
                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[1] = iMv[1].wrapping_add(iCode as i16);
                crate::decoder::mv_pred::UpdateP8x16MotionInfo(
                    &mut *pCurDqLayer,
                    pDec,
                    iMvArray,
                    iRefIdxArray,
                    0,
                    i << 2,
                    iRefIdx[i] as i8,
                    &iMv,
                );
            }
        }
        MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
            let mut iRefIdx = [0i32; 4];
            let mut iSubPartCount = [0i32; 4];
            let mut iPartWidth = [0i32; 4];

            if MB_TYPE_8x8_REF0 == mb_type {
                iRefCount[0] = 1;
                iRefCount[1] = 1;
            }

            // T5.I1: two window borrows for the whole arm. Nothing it calls —
            // `BsGet*`, `PredMv` — reaches either array, and the picture's
            // `pRefIndex`/`pMv` below are a different allocation. Twelve checks
            // across the first loop alone become two.
            let pSubMbType = (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy);
            let pNoSubMbPartSizeLessThan8x8Flag =
                (*pCurDqLayer).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy);

            for i in 0..4 {
                let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
                if ret != 0 {
                    return ret as i32;
                }
                let uiSubMbType = uiCode;
                if uiSubMbType >= 4 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_SUB_MB_TYPE);
                }
                pSubMbType[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iType;
                iSubPartCount[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iPartCount as i32;
                iPartWidth[i] = g_ksInterPSubMbTypeInfo[uiSubMbType as usize].iPartWidth as i32;
                *pNoSubMbPartSizeLessThan8x8Flag =
                    *pNoSubMbPartSizeLessThan8x8Flag && (uiSubMbType == 0);
            }

            if bAdaptiveMotionPredFlag {
                for i in 0..4 {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    iMotionPredFlag[i] = uiCode;
                }
            }

            if MB_TYPE_8x8_REF0 == mb_type {
                let ref_idx_mb = (*pDec).pRefIndex[0].get_mut(iMbXy);
                ref_idx_mb.fill(0);
            } else {
                for i in 0..4 {
                    let iIndex8 = (i as i32) << 2;
                    let uiScan4Idx = g_kuiScan4[iIndex8 as usize] as usize;

                    if iMotionPredFlag[i] == 0 {
                        let ret = crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[0], &mut uiCode);
                        if ret != 0 {
                            return ret;
                        }
                        iRefIdx[i] = uiCode as i32;
                        if iRefIdx[i] < 0 || iRefIdx[i] >= iRefCount[0] || ppRefPic[iRefIdx[i] as usize].is_none() {
                            (*pCtx).bMbRefConcealed = true;
                            if ec_active {
                                iRefIdx[i] = 0;
                                (*pCtx).iErrorCode |= dsBitstreamError;
                            } else {
                                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_REF_INDEX);
                            }
                        }
                        let pRefPic = pRefs.get(ppRefPic[iRefIdx[i] as usize]);
                        (*pCtx).bMbRefConcealed = (*pCtx).bRPLRError
                            || (*pCtx).bMbRefConcealed
                            || !(!pRefPic.is_null() && ((*pRefPic).bIsComplete || bIsPending));

                        let ref_idx_mb = (*pDec).pRefIndex[0].get_mut(iMbXy);
                        ref_idx_mb[uiScan4Idx] = iRefIdx[i] as i8;
                        ref_idx_mb[uiScan4Idx + 1] = iRefIdx[i] as i8;
                        ref_idx_mb[uiScan4Idx + 4] = iRefIdx[i] as i8;
                        ref_idx_mb[uiScan4Idx + 5] = iRefIdx[i] as i8;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                    }
                }
            }

            for i in 0..4 {
                let iPartCount = iSubPartCount[i];
                let uiSubMbType = pSubMbType[i];
                let iBlockWidth = iPartWidth[i];
                let iIdx = (i as i32) << 2;
                let uiIdx4Cache = g_kuiCache30ScanIdx[iIdx as usize] as usize;

                iRefIdxArray[0][uiIdx4Cache] = iRefIdx[i] as i8;
                iRefIdxArray[0][uiIdx4Cache + 1] = iRefIdx[i] as i8;
                iRefIdxArray[0][uiIdx4Cache + 6] = iRefIdx[i] as i8;
                iRefIdxArray[0][uiIdx4Cache + 7] = iRefIdx[i] as i8;

                for j in 0..iPartCount {
                    let iPartIdx = iIdx + j * iBlockWidth;
                    let uiScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
                    let uiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
                    let mut iMv = [0i16; 2];
                    crate::decoder::mv_pred::PredMv(&*iMvArray, &*iRefIdxArray, 0, iPartIdx as usize, iBlockWidth as usize, iRefIdx[i] as i8, &mut iMv);

                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[0] = iMv[0].wrapping_add(iCode as i16);
                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[1] = iMv[1].wrapping_add(iCode as i16);

                    let mv_mb = (*pDec).pMv[0].get_mut(iMbXy);
                    if SUB_MB_TYPE_8x8 == uiSubMbType {
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 1] = iMv;
                        mv_mb[uiScan4Idx + 4] = iMv;
                        mv_mb[uiScan4Idx + 5] = iMv;
                        iMvArray[0][uiCacheIdx] = iMv;
                        iMvArray[0][uiCacheIdx + 1] = iMv;
                        iMvArray[0][uiCacheIdx + 6] = iMv;
                        iMvArray[0][uiCacheIdx + 7] = iMv;
                    } else if SUB_MB_TYPE_8x4 == uiSubMbType {
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 1] = iMv;
                        iMvArray[0][uiCacheIdx] = iMv;
                        iMvArray[0][uiCacheIdx + 1] = iMv;
                    } else if SUB_MB_TYPE_4x8 == uiSubMbType {
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 4] = iMv;
                        iMvArray[0][uiCacheIdx] = iMv;
                        iMvArray[0][uiCacheIdx + 6] = iMv;
                    } else {
                        // SUB_MB_TYPE_4x4
                        mv_mb[uiScan4Idx] = iMv;
                        iMvArray[0][uiCacheIdx] = iMv;
                    }
                }
            }
        }
        _ => {}
    }
    ERR_NONE
}

/// Matches `ParseInterBInfo` in `parse_mb_syn_cavlc.cpp`.
///
/// `WELS_CHECK_SE_BOTH_WARNING` on the vertical mv is warning-only in C (see
/// `dec_golomb.h`), so it has no port here — same as `ParseInterInfo` above.
pub unsafe fn ParseInterBInfo(
    pCtx: *mut SWelsDecoderContext,
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    pRefs: PicRefs<'_>,
    iMvArray: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIdxArray: &mut [[i8; 30]; LIST_A],
    buf: &[u8],
    pBs: &mut BsCursor,
) -> i32 {
    // T5.W8: these two bindings were held across every call in the function that
    // takes the layer, and **every one of their uses is a read of a slice-header
    // scalar** this function never writes (grep-verified over both bodies). The layer
    // flip made the overlap a compile error where the raw pointer had made it
    // invisible (S25); the fix is the bracket maneuver at four scalars — copy once,
    // use everywhere, borrow nothing.
    let bDefaultMotionPredFlag =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bDefaultMotionPredFlag;
    let bAdaptiveMotionPredFlag =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.bAdaptiveMotionPredFlag;
    let iDirectSpatialMvPredFlag = (*pCurDqLayer)
        .sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iDirectSpatialMvPredFlag;
    let uiRefCountHdr =
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    let mut ref_idx_list = [[-1i8; 4]; LIST_A];
    let mut iRef = [0i8; 2];
    let mut iRefCount = [0i32; 2];
    let mut iMotionPredFlag =
        [[if bDefaultMotionPredFlag { 1u8 } else { 0u8 }; 4]; LIST_A];
    let mut iMv = [0i16; 2];
    let mut uiCode = 0u32;
    let mut iCode = 0i32;
    iRefCount[0] = uiRefCountHdr[0];
    iRefCount[1] = uiRefCountHdr[1];

    let bIsPending = crate::decoder::decoder_core::GetThreadCount(pCtx) > 1;
    let ec_active = (*pCtx).pParam.is_null()
        || (*(*pCtx).pParam).eEcActiveIdc != crate::decoder::error_concealment::ERROR_CON_IDC::ERROR_CON_DISABLE;

    /// `pCtx->bMbRefConcealed = pCtx->bRPLRError || pCtx->bMbRefConcealed ||
    ///  !(ppRefPic[list][ref] && (ppRefPic[list][ref]->bIsComplete || bIsPending))`
    macro_rules! note_ref_concealed {
        ($listIdx:expr, $iref:expr) => {{
            let p = pRefs.get(ref_id(pCtx, $listIdx as usize, $iref as usize));
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
            if $iref < 0 || $iref as i32 >= iRefCount[list] || ppRefPic[$iref as usize].is_none() {
                (*pCtx).bMbRefConcealed = true;
                if ec_active {
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

    let mbType = *(*pDec).pMbType.get(iMbXy);
    if IS_DIRECT(mbType) {
        let mut pMvDirect = [[0i16; 2]; LIST_A];
        let mut subMbType: crate::decoder::mv_pred::SubMbType = 0;
        if iDirectSpatialMvPredFlag != 0 {
            // predict direct spatial mv
            let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                pCtx, &mut *pCurDqLayer,
                pDec,
                pRefs,
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
                pCtx, &mut *pCurDqLayer,
                pDec,
                pRefs,
                &mut pMvDirect,
                &mut iRef,
                &mut subMbType,
            );
            if ret != ERR_NONE {
                return ret;
            }
        }
    } else if IS_INTER_16x16(mbType) {
        if bAdaptiveMotionPredFlag {
            for listIdx in LIST_0..LIST_A {
                if IS_DIR(mbType, 0, listIdx) {
                    let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                    if ret != 0 {
                        return ret as i32;
                    }
                    iMotionPredFlag[listIdx][0] = uiCode as u8;
                }
            }
        }
        for listIdx in LIST_0..LIST_A {
            if IS_DIR(mbType, 0, listIdx) {
                if iMotionPredFlag[listIdx][0] == 0 {
                    let ret = crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[listIdx], &mut uiCode);
                    if ret != 0 {
                        return ret;
                    }
                    // C truncates into `int8_t ref_idx_list[LIST_A][4]` here.
                    ref_idx_list[listIdx][0] = uiCode as i8;
                    check_ref_idx!(listIdx, ref_idx_list[listIdx][0]);
                } else {
                    // "inter parse: iMotionPredFlag = 1 not supported."
                    return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                }
            }
        }
        for listIdx in LIST_0..LIST_A {
            if IS_DIR(mbType, 0, listIdx) {
                crate::decoder::mv_pred::PredMv(
                    &*iMvArray,
                    &*iRefIdxArray,
                    listIdx,
                    0,
                    4,
                    ref_idx_list[listIdx][0],
                    &mut iMv,
                );
                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[0] = iMv[0].wrapping_add(iCode as i16);
                let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                if ret != 0 {
                    return ret;
                }
                iMv[1] = iMv[1].wrapping_add(iCode as i16);
            } else {
                iMv[0] = 0;
                iMv[1] = 0;
            }
            crate::decoder::mv_pred::UpdateP16x16MotionInfo(
                &mut *pCurDqLayer,
                pDec,
                listIdx,
                ref_idx_list[listIdx][0],
                &iMv,
            );
        }
    } else if IS_INTER_16x8(mbType) {
        if bAdaptiveMotionPredFlag {
            for listIdx in LIST_0..LIST_A {
                for i in 0..2usize {
                    if IS_DIR(mbType, i, listIdx) {
                        let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                        if ret != 0 {
                            return ret as i32;
                        }
                        iMotionPredFlag[listIdx][i] = uiCode as u8;
                    }
                }
            }
        }
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                if IS_DIR(mbType, i, listIdx) {
                    if iMotionPredFlag[listIdx][i] == 0 {
                        let ret =
                            crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[listIdx], &mut uiCode);
                        if ret != 0 {
                            return ret;
                        }
                        let mut iRefIdx = uiCode as i32;
                        check_ref_idx!(listIdx, iRefIdx);
                        ref_idx_list[listIdx][i] = iRefIdx as i8;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                    }
                }
            }
        }
        // Read mvd_L0 then mvd_L1
        for listIdx in LIST_0..LIST_A {
            // Partitions
            for i in 0..2usize {
                let iPartIdx = (i << 3) as i32;
                let iRefIdx = ref_idx_list[listIdx][i];
                if IS_DIR(mbType, i, listIdx) {
                    crate::decoder::mv_pred::PredInter16x8Mv(
                        &*iMvArray,
                        &*iRefIdxArray,
                        listIdx,
                        iPartIdx as usize,
                        iRefIdx,
                        &mut iMv,
                    );
                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[0] = iMv[0].wrapping_add(iCode as i16);
                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[1] = iMv[1].wrapping_add(iCode as i16);
                } else {
                    iMv[0] = 0;
                    iMv[1] = 0;
                }
                crate::decoder::mv_pred::UpdateP16x8MotionInfo(
                    &mut *pCurDqLayer,
                    pDec,
                    iMvArray,
                    iRefIdxArray,
                    listIdx,
                    iPartIdx as usize,
                    iRefIdx,
                    &iMv,
                );
            }
        }
    } else if IS_INTER_8x16(mbType) {
        if bAdaptiveMotionPredFlag {
            for listIdx in LIST_0..LIST_A {
                for i in 0..2usize {
                    if IS_DIR(mbType, i, listIdx) {
                        let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                        if ret != 0 {
                            return ret as i32;
                        }
                        iMotionPredFlag[listIdx][i] = uiCode as u8;
                    }
                }
            }
        }
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                if IS_DIR(mbType, i, listIdx) {
                    if iMotionPredFlag[listIdx][i] == 0 {
                        let ret =
                            crate::decoder::dec_golomb::BsGetTe0(buf, pBs, iRefCount[listIdx], &mut uiCode);
                        if ret != 0 {
                            return ret;
                        }
                        let mut iRefIdx = uiCode as i32;
                        check_ref_idx!(listIdx, iRefIdx);
                        ref_idx_list[listIdx][i] = iRefIdx as i8;
                    } else {
                        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                    }
                }
            }
        }
        for listIdx in LIST_0..LIST_A {
            for i in 0..2usize {
                let iPartIdx = (i << 2) as i32;
                let iRefIdx = ref_idx_list[listIdx][i];
                if IS_DIR(mbType, i, listIdx) {
                    crate::decoder::mv_pred::PredInter8x16Mv(
                        &*iMvArray,
                        &*iRefIdxArray,
                        listIdx,
                        iPartIdx as usize,
                        iRefIdx,
                        &mut iMv,
                    );
                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[0] = iMv[0].wrapping_add(iCode as i16);
                    let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                    if ret != 0 {
                        return ret;
                    }
                    iMv[1] = iMv[1].wrapping_add(iCode as i16);
                } else {
                    iMv[0] = 0;
                    iMv[1] = 0;
                }
                crate::decoder::mv_pred::UpdateP8x16MotionInfo(
                    &mut *pCurDqLayer,
                    pDec,
                    iMvArray,
                    iRefIdxArray,
                    listIdx,
                    iPartIdx as usize,
                    iRefIdx,
                    &iMv,
                );
            }
        }
    } else if IS_Inter_8x8(mbType) {
        let mut pSubPartCount = [0i8; 4];
        let mut pPartW = [0i8; 4];
        // sub_mb_type, partition
        let mut pMvDirect = [[0i16; 2]; LIST_A];
        if (*pCtx).sRefPic.pRefList[LIST_1][0].is_none() {
            // "Colocated Ref Picture for B-Slice is lost, B-Slice decoding cannot be continued!"
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
        }
        let bIsLongRef = (*pRefs.get(ref_id(pCtx, LIST_1, 0))).bIsLongRef;
        let ref0Count = std::cmp::min(
            uiRefCountHdr[LIST_0],
            (*pCtx).sRefPic.uiRefCount[LIST_0] as i32,
        );
        let mut has_direct_called = false;
        let mut directSubMbType: crate::decoder::mv_pred::SubMbType = 0;

        // T5.W8: T5.I1's loop-level window borrow for the flag **is gone**, and the
        // layer flip is what removed it. It was a `&mut` into
        // `grid.no_sub_mb_part_size_less_than8x8_flag` held across
        // `PredMvBDirectSpatial`/`PredBDirectTemporal`, which take the whole layer
        // mutably — F24/F25/F28's shape, invisible while the layer was a raw pointer
        // and a compile error the moment it was not. The two callees write a
        // *different* grid array (`sub_mb_type`, `mv_pred.rs:1035`/`:1130`), so no
        // write was actually lost; the borrow was still one the type system could not
        // justify. Re-derived per write below, which is exactly the same effect and
        // costs nothing worth measuring: S8's fourth negative result (D-perf-5) is
        // that bounds-check amortisation does not pay per macroblock.

        // uiSubMbType, partition
        for i in 0..4usize {
            let ret = crate::decoder::dec_golomb::BsGetUe(buf, pBs, &mut uiCode);
            if ret != 0 {
                return ret as i32;
            }
            let uiSubMbType = uiCode;
            if uiSubMbType >= 13 {
                // invalid uiSubMbType
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_SUB_MB_TYPE);
            }
            pSubPartCount[i] = g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iPartCount;
            pPartW[i] = g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iPartWidth;

            // Need modification when B picture add in, reference to 7.3.5
            if pSubPartCount[i] > 1 {
                *(*pCurDqLayer).grid.no_sub_mb_part_size_less_than8x8_flag.get_mut(iMbXy) =
                    false;
            }

            if IS_DIRECT(g_ksInterBSubMbTypeInfo[uiSubMbType as usize].iType) {
                if !has_direct_called {
                    if iDirectSpatialMvPredFlag != 0 {
                        let ret = crate::decoder::mv_pred::PredMvBDirectSpatial(
                            pCtx, &mut *pCurDqLayer,
                            pDec,
                            pRefs,
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
                            pCtx, &mut *pCurDqLayer,
                            pDec,
                            pRefs,
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
        // T5.I1's shared window over this family, **copied rather than borrowed since
        // T5.W8**. Its own sentence is what makes the copy exact: the parse loop is
        // done writing, and every remaining reader below — `FillSpatialDirect8x8Mv`,
        // `FillTemporalDirect8x8Mv`, `Update8x8RefIdx`, `PredMv` — reaches the layer
        // but not this array. Under `&mut DqLayerState` the borrow the window took
        // could not coexist with those calls, and four `SubMbType`s are a copy the
        // compiler will sink anyway.
        let pSubMbType = *(*pCurDqLayer).grid.sub_mb_type.get(iMbXy);
        if bAdaptiveMotionPredFlag {
            for listIdx in LIST_0..LIST_A {
                for i in 0..4usize {
                    let is_dir = IS_DIR(pSubMbType[i], 0, listIdx);
                    if is_dir {
                        let ret = crate::decoder::dec_golomb::BsGetOneBit(buf, pBs, &mut uiCode);
                        if ret != 0 {
                            return ret as i32;
                        }
                        iMotionPredFlag[listIdx][i] = uiCode as u8;
                    }
                }
            }
        }
        for i in 0..4usize {
            // Direct 8x8 Ref and mv
            let iIdx8 = (i << 2) as i16;
            if IS_DIRECT(pSubMbType[i]) {
                if iDirectSpatialMvPredFlag != 0 {
                    crate::decoder::mv_pred::FillSpatialDirect8x8Mv(
                        &mut *pCurDqLayer,
                        pDec,
                        iIdx8,
                        pSubPartCount[i],
                        pPartW[i],
                        directSubMbType,
                        bIsLongRef,
                        pMvDirect.as_mut_ptr(),
                        iRef.as_mut_ptr(),
                        Some(iMvArray),
                        // CAVLC has no mvd cache — the C++ passes NULL here too.
                        None,
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
                                pRefs,
                                colocRefIndexL0,
                                ref0Count,
                            );
                        } else {
                            mvColoc = (*pCurDqLayer).iColocMv[LIST_1].as_mut_ptr();
                        }
                    }
                    crate::decoder::mv_pred::Update8x8RefIdx(
                        &mut *pCurDqLayer,
                        pDec,
                        iIdx8,
                        LIST_0,
                        iRef[LIST_0],
                    );
                    crate::decoder::mv_pred::Update8x8RefIdx(
                        &mut *pCurDqLayer,
                        pDec,
                        iIdx8,
                        LIST_1,
                        iRef[LIST_1],
                    );
                    crate::decoder::mv_pred::FillTemporalDirect8x8Mv(
                        &mut *pCurDqLayer,
                        pDec,
                        iIdx8,
                        pSubPartCount[i],
                        pPartW[i],
                        directSubMbType,
                        iRef.as_mut_ptr(),
                        mvColoc,
                        Some(iMvArray),
                        // CAVLC has no mvd cache — the C++ passes NULL here too.
                        None,
                    );
                }
            }
        }
        // ref no-direct
        for listIdx in LIST_0..LIST_A {
            for i in 0..4usize {
                let iIdx8 = (i << 2) as i16;
                let subMbType = pSubMbType[i];
                let mut iref: i8 = REF_NOT_IN_LIST;
                if IS_DIRECT(subMbType) {
                    if iDirectSpatialMvPredFlag != 0 {
                        crate::decoder::mv_pred::Update8x8RefIdx(
                            &mut *pCurDqLayer,
                            pDec,
                            iIdx8,
                            listIdx,
                            iRef[listIdx],
                        );
                        ref_idx_list[listIdx][i] = iRef[listIdx];
                    }
                } else {
                    if IS_DIR(subMbType, 0, listIdx) {
                        if iMotionPredFlag[listIdx][i] == 0 {
                            let ret = crate::decoder::dec_golomb::BsGetTe0(
                                buf,
                                pBs,
                                iRefCount[listIdx],
                                &mut uiCode,
                            );
                            if ret != 0 {
                                return ret;
                            }
                            iref = uiCode as i8;
                            check_ref_idx!(listIdx, iref);
                        } else {
                            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_UNSUPPORTED_ILP);
                        }
                    }
                    crate::decoder::mv_pred::Update8x8RefIdx(
                        &mut *pCurDqLayer,
                        pDec,
                        iIdx8,
                        listIdx,
                        iref,
                    );
                    ref_idx_list[listIdx][i] = iref;
                }
            }
        }
        // mv
        for listIdx in LIST_0..LIST_A {
            for i in 0..4usize {
                let iPartCount = pSubPartCount[i];
                let iBlockW = pPartW[i];

                let uiCacheIdx = g_kuiCache30ScanIdx[i << 2] as usize;

                let iref = ref_idx_list[listIdx][i];
                iRefIdxArray[listIdx][uiCacheIdx] = iref;
                iRefIdxArray[listIdx][uiCacheIdx + 1] = iref;
                iRefIdxArray[listIdx][uiCacheIdx + 6] = iref;
                iRefIdxArray[listIdx][uiCacheIdx + 7] = iref;

                let subMbType = pSubMbType[i];
                if IS_DIRECT(subMbType) {
                    continue;
                }
                let is_dir = IS_DIR(subMbType, 0, listIdx);
                for j in 0..iPartCount as usize {
                    let iPartIdx = (i << 2) + j * iBlockW as usize;
                    let uiScan4Idx = g_kuiScan4[iPartIdx] as usize;
                    let uiCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;
                    if is_dir {
                        crate::decoder::mv_pred::PredMv(
                            &*iMvArray,
                            &*iRefIdxArray,
                            listIdx,
                            iPartIdx as usize,
                            iBlockW as usize,
                            iref,
                            &mut iMv,
                        );
                        let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                        if ret != 0 {
                            return ret;
                        }
                        iMv[0] = iMv[0].wrapping_add(iCode as i16);
                        let ret = crate::decoder::dec_golomb::BsGetSe(buf, pBs, &mut iCode);
                        if ret != 0 {
                            return ret;
                        }
                        iMv[1] = iMv[1].wrapping_add(iCode as i16);
                    } else {
                        iMv[0] = 0;
                        iMv[1] = 0;
                    }

                    let mv_mb = (*pDec).pMv[listIdx].get_mut(iMbXy);
                    if IS_SUB_8x8(subMbType) {
                        // MB_TYPE_8x8
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 1] = iMv;
                        mv_mb[uiScan4Idx + 4] = iMv;
                        mv_mb[uiScan4Idx + 5] = iMv;
                        iMvArray[listIdx][uiCacheIdx] = iMv;
                        iMvArray[listIdx][uiCacheIdx + 1] = iMv;
                        iMvArray[listIdx][uiCacheIdx + 6] = iMv;
                        iMvArray[listIdx][uiCacheIdx + 7] = iMv;
                    } else if IS_SUB_8x4(subMbType) {
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 1] = iMv;
                        iMvArray[listIdx][uiCacheIdx] = iMv;
                        iMvArray[listIdx][uiCacheIdx + 1] = iMv;
                    } else if IS_SUB_4x8(subMbType) {
                        mv_mb[uiScan4Idx] = iMv;
                        mv_mb[uiScan4Idx + 4] = iMv;
                        iMvArray[listIdx][uiCacheIdx] = iMv;
                        iMvArray[listIdx][uiCacheIdx + 6] = iMv;
                    } else {
                        // SUB_MB_TYPE_4x4 == uiSubMbType
                        mv_mb[uiScan4Idx] = iMv;
                        iMvArray[listIdx][uiCacheIdx] = iMv;
                    }
                }
            }
        }
    }
    ERR_NONE
}

pub unsafe fn WelsFillDirectCacheCabac(
    pNeighAvail: &SWelsNeighAvail,
    iDirect: &mut [i8; 30],
    pCurDqLayer: &DqLayerState,
) {
    let na = &*pNeighAvail;
    let dq = &*pCurDqLayer;
    let iCurXy = dq.iMbXyIndex as usize;
    let mut iTopXy = 0usize;
    let mut iLeftXy = 0usize;
    let mut iLeftTopXy = 0usize;
    let mut iRightTopXy = 0usize;

    if na.iTopAvail != 0 {
        iTopXy = iCurXy - dq.iMbWidth as usize;
    }
    if na.iLeftAvail != 0 {
        iLeftXy = iCurXy - 1;
    }
    if na.iLeftTopAvail != 0 {
        iLeftTopXy = iCurXy - 1 - dq.iMbWidth as usize;
    }
    if na.iRightTopAvail != 0 {
        iRightTopXy = iCurXy + 1 - dq.iMbWidth as usize;
    }

    iDirect.fill(0);
    if na.iLeftAvail != 0 && IS_INTER(na.iLeftType) {
        let pDir = dq.grid.direct.get(iLeftXy).as_ptr();
        iDirect[6] = *pDir.add(3);
        iDirect[12] = *pDir.add(7);
        iDirect[18] = *pDir.add(11);
        iDirect[24] = *pDir.add(15);
    }
    if na.iLeftTopAvail != 0 && IS_INTER(na.iLeftTopType) {
        let pDir = dq.grid.direct.get(iLeftTopXy).as_ptr();
        iDirect[0] = *pDir.add(15);
    }
    if na.iTopAvail != 0 && IS_INTER(na.iTopType) {
        let pDir = dq.grid.direct.get(iTopXy).as_ptr();
        iDirect[1] = *pDir.add(12);
        iDirect[2] = *pDir.add(13);
        iDirect[3] = *pDir.add(14);
        iDirect[4] = *pDir.add(15);
    }
    if na.iRightTopAvail != 0 && IS_INTER(na.iRightTopType) {
        let pDir = dq.grid.direct.get(iRightTopXy).as_ptr();
        iDirect[5] = *pDir.add(12);
    }
}

pub unsafe fn WelsFillCacheConstrain0IntraNxN(
    pNeighAvail: &SWelsNeighAvail,
    pNonZeroCount: &mut [u8; 48],
    pIntraPredMode: *mut i8,
    pCurDqLayer: &DqLayerState,
) {
    unsafe {
        WelsFillCacheNonZeroCount(pNeighAvail, pNonZeroCount, Some(pCurDqLayer));

        let na = &*pNeighAvail;
        let dq = &*pCurDqLayer;
        let iCurXy = dq.iMbXyIndex;
        let mut iTopXy = 0;
        let mut iLeftXy = 0;

        if na.iTopAvail != 0 {
            iTopXy = iCurXy - dq.iMbWidth;
        }
        if na.iLeftAvail != 0 {
            iLeftXy = iCurXy - 1;
        }

        if na.iTopAvail != 0 && IS_INTRANxN(na.iTopType) {
            // T5.R5: four modes, copied as four modes. The `0x02020202`/`0xffffffff`
            // fills below are the same byte four times, which is what the C's word
            // store was spelling.
            let pTopMode = dq.grid.intra_pred_mode.get(iTopXy as usize).as_ptr();
            for k in 0..4 {
                *pIntraPredMode.add(1 + k) = *pTopMode.add(k);
            }
        } else {
            let iPred: i8 = if na.iTopAvail != 0 { 0x02 } else { -1 };
            for k in 0..4 {
                *pIntraPredMode.add(1 + k) = iPred;
            }
        }

        if na.iLeftAvail != 0 && IS_INTRANxN(na.iLeftType) {
            let pLeftMode = dq.grid.intra_pred_mode.get(iLeftXy as usize).as_ptr();
            *pIntraPredMode.add(0 + 8 * 1) = *pLeftMode.add(4);
            *pIntraPredMode.add(0 + 8 * 2) = *pLeftMode.add(5);
            *pIntraPredMode.add(0 + 8 * 3) = *pLeftMode.add(6);
            *pIntraPredMode.add(0 + 8 * 4) = *pLeftMode.add(3);
        } else {
            let iPred: i8 = if na.iLeftAvail != 0 { 2 } else { -1 };
            *pIntraPredMode.add(0 + 8 * 1) = iPred;
            *pIntraPredMode.add(0 + 8 * 2) = iPred;
            *pIntraPredMode.add(0 + 8 * 3) = iPred;
            *pIntraPredMode.add(0 + 8 * 4) = iPred;
        }
    }
}

// ============================================================================
// Intra Mode Validation
// ============================================================================

/// Predicts the most probable mode for an Intra 4x4 sub-block.
pub unsafe fn PredIntra4x4Mode(pIntraPredMode: *mut i8, iIdx4: i32) -> i32 {
    unsafe {
        let scan = g_kuiScan8[iIdx4 as usize] as usize;
        let iTopMode = *pIntraPredMode.add(scan - 8);
        let iLeftMode = *pIntraPredMode.add(scan - 1);

        if iLeftMode == -1 || iTopMode == -1 {
            2
        } else {
            (iLeftMode.min(iTopMode)) as i32
        }
    }
}

#[inline(always)]
fn CHECK_I16_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksI16PredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

#[inline(always)]
fn CHECK_CHROMA_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksChromaPredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

#[inline(always)]
fn CHECK_I4_MODE(a: i8, b: i32, c: i32, d: i32) -> bool {
    let info = &g_ksI4PredInfo[a as usize];
    (a == info.iPredMode)
        && (b >= info.iLeftAvail as i32)
        && (c >= info.iTopAvail as i32)
        && (d >= info.iLeftTopAvail as i32)
}

pub unsafe fn CheckIntra16x16PredMode(uiSampleAvail: u8, pMode: *mut i8) -> i32 {
    unsafe {
        let iLeftAvail = (uiSampleAvail & 0x04) as i32;
        let bLeftTopAvail = (uiSampleAvail & 0x02) as i32;
        let iTopAvail = (uiSampleAvail & 0x01) as i32;

        if *pMode < 0 || *pMode > MAX_PRED_MODE_ID_I16x16 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
        }

        if I16_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return ERR_NONE;
            } else if iLeftAvail != 0 {
                *pMode = I16_PRED_DC_L;
            } else if iTopAvail != 0 {
                *pMode = I16_PRED_DC_T;
            } else {
                *pMode = I16_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_I16_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I16x16_PRED_MODE);
            }
        }
        ERR_NONE
    }
}

/// T5.H5's family took this signature with it: `pMode` had exactly three callers,
/// all of them `pChromaPredMode[iMbXy]`, so when that array became a grid entry the
/// parameter's only possible source became a `&mut i8`. The body then needs no
/// `unsafe` at all — it reads and writes one `i8` — so the function stops being an
/// `unsafe fn`.
pub fn CheckIntraChromaPredMode(uiSampleAvail: u8, pMode: &mut i8) -> i32 {
    {
        let iLeftAvail = (uiSampleAvail & 0x04) as i32;
        let bLeftTopAvail = (uiSampleAvail & 0x02) as i32;
        let iTopAvail = (uiSampleAvail & 0x01) as i32;

        if C_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return ERR_NONE;
            } else if iLeftAvail != 0 {
                *pMode = C_PRED_DC_L;
            } else if iTopAvail != 0 {
                *pMode = C_PRED_DC_T;
            } else {
                *pMode = C_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_CHROMA_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_INVALID_I_CHROMA_PRED_MODE);
            }
        }
        ERR_NONE
    }
}

pub unsafe fn CheckIntraNxNPredMode(
    pSampleAvail: *const i32,
    pMode: *mut i8,
    iIndex: i32,
    b8x8: bool,
) -> i32 {
    unsafe {
        let iIdx = g_kuiCache30ScanIdx[iIndex as usize] as isize;

        let iLeftAvail = *pSampleAvail.offset(iIdx - 1);
        let iTopAvail = *pSampleAvail.offset(iIdx - 6);
        let bLeftTopAvail = *pSampleAvail.offset(iIdx - 7);
        let bRightTopAvail = *pSampleAvail.offset(iIdx - if b8x8 { 4 } else { 5 });

        if *pMode < 0 || *pMode > MAX_PRED_MODE_ID_I4x4 {
            return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE);
        }

        let mut iFinalMode: i8;

        if I4_PRED_DC == *pMode {
            if iLeftAvail != 0 && iTopAvail != 0 {
                return *pMode as i32;
            } else if iLeftAvail != 0 {
                iFinalMode = I4_PRED_DC_L;
            } else if iTopAvail != 0 {
                iFinalMode = I4_PRED_DC_T;
            } else {
                iFinalMode = I4_PRED_DC_128;
            }
        } else {
            let bModeAvail = CHECK_I4_MODE(*pMode, iLeftAvail, iTopAvail, bLeftTopAvail);
            if !bModeAvail {
                return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INVALID_INTRA4X4_MODE);
            }

            iFinalMode = *pMode;

            if I4_PRED_DDL == iFinalMode && bRightTopAvail == 0 {
                iFinalMode = I4_PRED_DDL_TOP;
            } else if I4_PRED_VL == iFinalMode && bRightTopAvail == 0 {
                iFinalMode = I4_PRED_VL_TOP;
            }
        }
        iFinalMode as i32
    }
}

// `BsStartCavlc`/`BsEndCavlc` used to live here. They are now
// `BsCursor::start_cavlc`/`end_cavlc` (plan §2.2.2 [P3]) — the arithmetic is
// identical, and the mode additionally makes the stale-accumulator desync a debug
// panic instead of a silent miscode. A frozen transliteration of the C++ pair is kept
// in `tests/safe_bits_differential.rs` as the parity reference.

// ============================================================================
// Inverse Quantization and Transforms (IDCT)
// ============================================================================

pub unsafe fn WelsChromaDcIdct(pBlock: *mut i16) {
    unsafe {
        let iStride: isize = 32;
        let iXStride: isize = 16;
        let iStride1 = iXStride + iStride;
        let pBlk = pBlock;

        let mut iA = *pBlk as i32;
        let mut iB = *pBlk.offset(iXStride) as i32;
        let mut iC = *pBlk.offset(iStride) as i32;
        let iD = *pBlk.offset(iStride1) as i32;

        let iE = iA - iB;
        iA += iB;
        iB = iC - iD;
        iC += iD;

        *pBlk = (iA + iC) as i16;
        *pBlk.offset(iXStride) = (iE + iB) as i16;
        *pBlk.offset(iStride) = (iA - iC) as i16;
        *pBlk.offset(iStride1) = (iE - iB) as i16;
    }
}

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
        I16_LUMA_AC | I16_LUMA_DC => {
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
        // Reference to Table 7-2
        LUMA_DC_AC_INTRA_8 => {
            *pMBproperty = 6;
            *pResidualProperty = LUMA_DC_AC_8;
        }
        LUMA_DC_AC_INTER_8 => {
            *pMBproperty = 7;
            *pResidualProperty = LUMA_DC_AC_8;
        }
        _ => {}
    }
}

pub unsafe fn WelsLumaDcDequantIdct(pBlock: *mut i16, uiQp: u8, pCtx: *mut SWelsDecoderContext) {
    unsafe {
        let kiQMul: i32 = if !pCtx.is_null() && (*pCtx).bUseScalingList && !(*pCtx).pDequant_coeff4x4[0].is_null() {
            (*(*pCtx).pDequant_coeff4x4[0].add(uiQp as usize))[0] as i32
        } else {
            (g_kuiDequantCoeff[uiQp as usize][0] as i32) << 4
        };

        const STRIDE: isize = 16;
        let mut iTemp = [0i32; 16];
        let pBlk = pBlock;
        let kiXOffset: [isize; 4] = [0, STRIDE, STRIDE << 2, 5 * STRIDE];
        let kiYOffset: [isize; 4] = [0, STRIDE << 1, STRIDE << 3, 10 * STRIDE];

        for i in 0..4 {
            let kiOffset = kiYOffset[i];
            let kiX1 = kiOffset + kiXOffset[2];
            let kiX2 = STRIDE + kiOffset;
            let kiX3 = kiOffset + kiXOffset[3];
            let kiI4 = i << 2;
            let kiZ0 = *pBlk.offset(kiOffset) as i32 + *pBlk.offset(kiX1) as i32;
            let kiZ1 = *pBlk.offset(kiOffset) as i32 - *pBlk.offset(kiX1) as i32;
            let kiZ2 = *pBlk.offset(kiX2) as i32 - *pBlk.offset(kiX3) as i32;
            let kiZ3 = *pBlk.offset(kiX2) as i32 + *pBlk.offset(kiX3) as i32;

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

            *pBlk.offset(kiOffset) = (((kiZ0 + kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[1] + kiOffset) = (((kiZ1 + kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[2] + kiOffset) = (((kiZ1 - kiZ2) * kiQMul + (1 << 5)) >> 6) as i16;
            *pBlk.offset(kiYOffset[3] + kiOffset) = (((kiZ0 - kiZ3) * kiQMul + (1 << 5)) >> 6) as i16;
        }
    }
}

// // ============================================================================
// CAVLC Residual Parsing & Decoding Implementation
// ============================================================================

pub unsafe fn CavlcGetTrailingOnesAndTotalCoeff(
    uiTotalCoeff: &mut u8,
    uiTrailingOnes: &mut u8,
    pBitsCache: &mut SReadBitsCache,
    pVlcTable: &SVlcTable,
    bChromaDc: bool,
    nC: i8,
) -> i32 {
    let kpVlcTableMoreBitsCountList: [*const u8; 3] = [
        g_kuiVlcTableMoreBitsCount0.as_ptr(),
        g_kuiVlcTableMoreBitsCount1.as_ptr(),
        g_kuiVlcTableMoreBitsCount2.as_ptr(),
    ];
    let mut iUsedBits: i32 = 0;

    if bChromaDc {
        let uiValue = ((*pBitsCache).uiCache32Bit >> 24) as usize;
        let vlc_entry = *(*pVlcTable).kpChromaCoeffTokenVlcTable.add(uiValue);
        let iIndexVlc = vlc_entry[0] as usize;
        let uiCount = vlc_entry[1] as u32;
        POP_BUFFER(pBitsCache, uiCount);
        iUsedBits += uiCount as i32;
        *uiTrailingOnes = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][0];
        *uiTotalCoeff = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][1];
    } else {
        let iNcMapIdx = g_kuiNcMapTable[nC as usize] as usize;
        if iNcMapIdx <= 2 {
            let uiValue = ((*pBitsCache).uiCache32Bit >> 24) as usize;
            if uiValue < g_kuiVlcTableNeedMoreBitsThread[iNcMapIdx] as usize {
                POP_BUFFER(pBitsCache, 8);
                iUsedBits += 8;
                let more_bits_shift = 32 - *kpVlcTableMoreBitsCountList[iNcMapIdx].add(uiValue) as usize;
                let iIndexValue = ((*pBitsCache).uiCache32Bit >> more_bits_shift) as usize;
                let entry_ptr = (*pVlcTable).kpCoeffTokenVlcTable[iNcMapIdx + 1][uiValue].add(iIndexValue);
                let iIndexVlc = (*entry_ptr)[0] as usize;
                let uiCount = (*entry_ptr)[1] as u32;
                POP_BUFFER(pBitsCache, uiCount);
                iUsedBits += uiCount as i32;
                *uiTrailingOnes = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][0];
                *uiTotalCoeff = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][1];
            } else {
                let entry_ptr = (*pVlcTable).kpCoeffTokenVlcTable[0][iNcMapIdx].add(uiValue);
                let iIndexVlc = (*entry_ptr)[0] as usize;
                let uiCount = (*entry_ptr)[1] as u32;
                POP_BUFFER(pBitsCache, uiCount);
                iUsedBits += uiCount as i32;
                *uiTrailingOnes = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][0];
                *uiTotalCoeff = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][1];
            }
        } else {
            let uiValue = ((*pBitsCache).uiCache32Bit >> (32 - 6)) as usize;
            POP_BUFFER(pBitsCache, 6);
            iUsedBits += 6;
            let entry_ptr = (*pVlcTable).kpCoeffTokenVlcTable[0][3].add(uiValue);
            let iIndexVlc = (*entry_ptr)[0] as usize;
            *uiTrailingOnes = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][0];
            *uiTotalCoeff = g_kuiVlcTrailingOneTotalCoeffTable[iIndexVlc][1];
        }
    }
    iUsedBits
}

pub unsafe fn ParseCoeffToken(
    uiTotalCoeff: &mut u8,
    uiTrailingOnes: &mut u8,
    pBitsCache: &mut SReadBitsCache,
    pVlcTable: &SVlcTable,
    bChromaDc: bool,
    nC: i8,
) -> i32 {
    CavlcGetTrailingOnesAndTotalCoeff(uiTotalCoeff, uiTrailingOnes, pBitsCache, pVlcTable, bChromaDc, nC)
}

pub fn CavlcGetLevelVal(
    iLevel: &mut [i32; 16],
    pBitsCache: &mut SReadBitsCache,
    uiTotalCoeff: u8,
    uiTrailingOnes: u8,
) -> i32 {
    let mut iUsedBits: i32 = 0;
    for i in 0..(uiTrailingOnes as usize) {
        iLevel[i] = 1 - (((*pBitsCache).uiCache32Bit >> (30 - i)) & 0x02) as i32;
    }
    POP_BUFFER(pBitsCache, uiTrailingOnes as u32);
    iUsedBits += uiTrailingOnes as i32;

    let mut iSuffixLength: i32 = if uiTotalCoeff > 10 && uiTrailingOnes < 3 { 1 } else { 0 };

    for i in (uiTrailingOnes as usize)..(uiTotalCoeff as usize) {
        if (*pBitsCache).uiRemainBits <= 16 {
            SHIFT_BUFFER(pBitsCache);
        }
        let iPrefixBits = ((*pBitsCache).uiCache32Bit.leading_zeros() + 1) as i32;
        if iPrefixBits > MAX_LEVEL_PREFIX + 1 {
            return -1;
        }
        POP_BUFFER(pBitsCache, iPrefixBits as u32);
        iUsedBits += iPrefixBits;
        let iLevelPrefix = iPrefixBits - 1;
        let mut iLevelCode = iLevelPrefix << iSuffixLength;
        let mut iSuffixLengthSize = iSuffixLength;

        if iLevelPrefix >= 14 {
            if 14 == iLevelPrefix && 0 == iSuffixLength {
                iSuffixLengthSize = 4;
            } else if 15 == iLevelPrefix {
                iSuffixLengthSize = 12;
                if iSuffixLength == 0 {
                    iLevelCode += 15;
                }
            }
        }

        if iSuffixLengthSize > 0 {
            if (*pBitsCache).uiRemainBits <= iSuffixLengthSize as u8 {
                SHIFT_BUFFER(pBitsCache);
            }
            iLevelCode += ((*pBitsCache).uiCache32Bit >> (32 - iSuffixLengthSize)) as i32;
            POP_BUFFER(pBitsCache, iSuffixLengthSize as u32);
            iUsedBits += iSuffixLengthSize;
        }

        if i == (uiTrailingOnes as usize) && uiTrailingOnes < 3 {
            iLevelCode += 2;
        }
        let mut lev = (iLevelCode + 2) >> 1;
        if (iLevelCode & 0x01) != 0 {
            lev = -lev;
        }
        iLevel[i] = lev;

        if iSuffixLength == 0 {
            iSuffixLength = 1;
        }
        let iThreshold = 3 << (iSuffixLength - 1);
        if (iLevel[i] > iThreshold || iLevel[i] < -iThreshold) && iSuffixLength < 6 {
            iSuffixLength += 1;
        }
    }
    iUsedBits
}

pub unsafe fn CavlcGetTotalZeros(
    iZerosLeft: &mut i32,
    pBitsCache: &mut SReadBitsCache,
    uiTotalCoeff: u8,
    pVlcTable: &SVlcTable,
    bChromaDc: bool,
) -> i32 {
    let mut iUsedBits: i32 = 0;
    let iTotalZeroVlcIdx = uiTotalCoeff as usize;
    let uiTableType: usize = if bChromaDc { 1 } else { 0 };

    let iCount = if bChromaDc {
        g_kuiTotalZerosBitNumChromaMap[iTotalZeroVlcIdx - 1] as usize
    } else {
        g_kuiTotalZerosBitNumMap[iTotalZeroVlcIdx - 1] as usize
    };

    if (*pBitsCache).uiRemainBits < iCount as u8 {
        SHIFT_BUFFER(pBitsCache);
    }
    let uiValue = ((*pBitsCache).uiCache32Bit >> (32 - iCount)) as usize;
    let table_ptr = (*pVlcTable).kpTotalZerosTable[uiTableType][iTotalZeroVlcIdx - 1];
    let entry = *table_ptr.add(uiValue);
    let consumed_bits = entry[1] as u32;
    POP_BUFFER(pBitsCache, consumed_bits);
    iUsedBits += consumed_bits as i32;
    *iZerosLeft = entry[0] as i32;
    iUsedBits
}

pub unsafe fn ParseTotalZeros(
    iZerosLeft: &mut i32,
    pBitsCache: &mut SReadBitsCache,
    uiTotalCoeff: u8,
    pVlcTable: &SVlcTable,
    bChromaDc: bool,
) -> i32 {
    CavlcGetTotalZeros(iZerosLeft, pBitsCache, uiTotalCoeff, pVlcTable, bChromaDc)
}

pub unsafe fn CavlcGetRunBefore(
    iRun: &mut [i32; 16],
    pBitsCache: &mut SReadBitsCache,
    uiTotalCoeff: u8,
    pVlcTable: &SVlcTable,
    mut iZerosLeft: i32,
) -> i32 {
    let mut iUsedBits: i32 = 0;
    let total = uiTotalCoeff as usize;

    for i in 0..(total - 1) {
        if iZerosLeft > 0 {
            let uiCount = g_kuiZeroLeftBitNumMap[iZerosLeft as usize] as u32;
            if (*pBitsCache).uiRemainBits < uiCount as u8 {
                SHIFT_BUFFER(pBitsCache);
            }
            let uiValue = ((*pBitsCache).uiCache32Bit >> (32 - uiCount)) as usize;
            if iZerosLeft < 7 {
                let table_ptr = (*pVlcTable).kpZeroTable[(iZerosLeft - 1) as usize];
                let entry = *table_ptr.add(uiValue);
                let consumed = entry[1] as u32;
                POP_BUFFER(pBitsCache, consumed);
                iUsedBits += consumed as i32;
                iRun[i] = entry[0] as i32;
            } else {
                POP_BUFFER(pBitsCache, uiCount);
                iUsedBits += uiCount as i32;
                let table_ptr = (*pVlcTable).kpZeroTable[6];
                let entry = *table_ptr.add(uiValue);
                if entry[0] < 7 {
                    iRun[i] = entry[0] as i32;
                } else {
                    if (*pBitsCache).uiRemainBits < 16 {
                        SHIFT_BUFFER(pBitsCache);
                    }
                    let iPrefixBits = ((*pBitsCache).uiCache32Bit.leading_zeros() + 1) as i32;
                    iRun[i] = iPrefixBits + 6;
                    if iRun[i] > iZerosLeft {
                        return -1;
                    }
                    POP_BUFFER(pBitsCache, iPrefixBits as u32);
                    iUsedBits += iPrefixBits;
                }
            }
        } else {
            for j in i..total {
                iRun[j] = 0;
            }
            return iUsedBits;
        }
        iZerosLeft -= iRun[i];
    }
    iRun[total - 1] = iZerosLeft;
    iUsedBits
}

pub unsafe fn ParseRunBefore(
    iRun: &mut [i32; 16],
    pBitsCache: &mut SReadBitsCache,
    uiTotalCoeff: u8,
    pVlcTable: &SVlcTable,
    iZerosLeft: i32,
) -> i32 {
    CavlcGetRunBefore(iRun, pBitsCache, uiTotalCoeff, pVlcTable, iZerosLeft)
}

pub unsafe fn WelsResidualBlockCavlc(
    pVlcTable: &SVlcTable,
    pNonZeroCountCache: &mut [u8; 48],
    buf: &[u8],
    pBs: &mut BsCursor,
    iIndex: i32,
    iMaxNumCoeff: i32,
    kpZigzagTable: &[u8],
    mut iResidualProperty: i32,
    pTCoeff: *mut i16,
    uiQp: u8,
    pCtx: *mut SWelsDecoderContext,
) -> i32 {
    let mut iLevel = [0i32; 16];
    let mut iRun = [0i32; 16];
    let mut iMbResProperty: i32 = 0;
    GetMbResProperty(&mut iMbResProperty, &mut iResidualProperty, true);

    // `pCtx->pDequant_coeff4x4[iMbResProperty][uiQp]` in parse_mb_syn_cavlc.cpp: the 4x4
    // table is indexed directly by the MB residual property (0..5); only the 8x8 table
    // is biased by 6.
    let kpDequantCoeff: *const u16 = if !pCtx.is_null() && (*pCtx).bUseScalingList && !(*pCtx).pDequant_coeff4x4[iMbResProperty as usize].is_null() {
        (*(*pCtx).pDequant_coeff4x4[iMbResProperty as usize].add(uiQp as usize)).as_ptr()
    } else {
        g_kuiDequantCoeff[uiQp as usize].as_ptr()
    };

    let mut uiTotalCoeff: u8 = 0;
    let mut uiTrailingOnes: u8 = 0;
    let mut iUsedBits: i32 = 0;
    let iCurIdx = pBs.cavlc_bit_pos() as usize;
    let pBuf = buf.as_ptr().add(iCurIdx >> 3) as *mut u8;
    let bChromaDc = CHROMA_DC == iResidualProperty;
    let bChroma = bChromaDc || CHROMA_AC == iResidualProperty;

    let uiCache32Bit = ((*pBuf.add(0) as u32) << 24)
        | ((*pBuf.add(1) as u32) << 16)
        | ((*pBuf.add(2) as u32) << 8)
        | (*pBuf.add(3) as u32);

    let mut sReadBitsCache = SReadBitsCache {
        uiCache32Bit: uiCache32Bit << (iCurIdx & 0x07),
        uiRemainBits: 32 - (iCurIdx & 0x07) as u8,
        pBuf,
    };

    let iCurNonZeroCacheIdx = g_kuiCache48CountScan4Idx[iIndex as usize] as usize;
    let nA = pNonZeroCountCache[iCurNonZeroCacheIdx - 1] as i8;
    let nB = pNonZeroCountCache[iCurNonZeroCacheIdx - 8] as i8;
    let nC = wels_non_zero_count_average(nA, nB);

    iUsedBits += CavlcGetTrailingOnesAndTotalCoeff(
        &mut uiTotalCoeff,
        &mut uiTrailingOnes,
        &mut sReadBitsCache,
        pVlcTable,
        bChromaDc,
        nC,
    );

    if iResidualProperty != CHROMA_DC && iResidualProperty != I16_LUMA_DC {
        pNonZeroCountCache[iCurNonZeroCacheIdx] = uiTotalCoeff;
    }
    if 0 == uiTotalCoeff {
        pBs.advance_cavlc_bits(iUsedBits as isize);
        return ERR_NONE;
    }
    if uiTrailingOnes > 3 || uiTotalCoeff > 16 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_TOTAL_COEFF_OR_TRAILING_ONES);
    }
    let res = CavlcGetLevelVal(&mut iLevel, &mut sReadBitsCache, uiTotalCoeff, uiTrailingOnes);
    if res == -1 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_LEVEL);
    }
    iUsedBits += res;

    let mut iZerosLeft: i32 = 0;
    if (uiTotalCoeff as i32) < iMaxNumCoeff {
        iUsedBits += CavlcGetTotalZeros(&mut iZerosLeft, &mut sReadBitsCache, uiTotalCoeff, pVlcTable, bChromaDc);
    }

    if iZerosLeft < 0 || (iZerosLeft + uiTotalCoeff as i32) > iMaxNumCoeff {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_ZERO_LEFT);
    }
    let res = CavlcGetRunBefore(&mut iRun, &mut sReadBitsCache, uiTotalCoeff, pVlcTable, iZerosLeft);
    if res == -1 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_RUN_BEFORE);
    }
    iUsedBits += res;
    pBs.advance_cavlc_bits(iUsedBits as isize);
    let mut iCoeffNum: i32 = -1;

    if iResidualProperty == CHROMA_DC {
        for i in (0..(uiTotalCoeff as usize)).rev() {
            iCoeffNum += iRun[i] + 1;
            let j = kpZigzagTable[iCoeffNum as usize] as usize;
            *pTCoeff.add(j) = iLevel[i] as i16;
        }
        WelsChromaDcIdct(pTCoeff);
        if pCtx.is_null() || !(*pCtx).bUseScalingList {
            for j in 0..4 {
                let idx = kpZigzagTable[j] as usize;
                *pTCoeff.add(idx) = ((*pTCoeff.add(idx) as i32 * *kpDequantCoeff as i32) >> 1) as i16;
            }
        } else {
            for j in 0..4 {
                let idx = kpZigzagTable[j] as usize;
                *pTCoeff.add(idx) = (((*pTCoeff.add(idx) as i64) * (*kpDequantCoeff as i64)) >> 5) as i16;
            }
        }
    } else if iResidualProperty == I16_LUMA_DC {
        for i in (0..(uiTotalCoeff as usize)).rev() {
            iCoeffNum += iRun[i] + 1;
            let j = kpZigzagTable[iCoeffNum as usize] as usize;
            *pTCoeff.add(j) = iLevel[i] as i16;
        }
        WelsLumaDcDequantIdct(pTCoeff, uiQp, pCtx);
    } else {
        for i in (0..(uiTotalCoeff as usize)).rev() {
            iCoeffNum += iRun[i] + 1;
            let j = kpZigzagTable[iCoeffNum as usize] as usize;
            if pCtx.is_null() || !(*pCtx).bUseScalingList {
                *pTCoeff.add(j) = (iLevel[i] * (*kpDequantCoeff.add(j & 0x07)) as i32) as i16;
            } else {
                *pTCoeff.add(j) = ((iLevel[i] * (*kpDequantCoeff.add(j)) as i32 + 8) >> 4) as i16;
            }
        }
    }
    ERR_NONE
}

/// Matches `WelsResidualBlockCavlc8x8` in `parse_mb_syn_cavlc.cpp`.
pub unsafe fn WelsResidualBlockCavlc8x8(
    pVlcTable: &SVlcTable,
    pNonZeroCountCache: &mut [u8; 48],
    buf: &[u8],
    pBs: &mut BsCursor,
    iIndex: i32,
    iMaxNumCoeff: i32,
    kpZigzagTable: &[u8],
    mut iResidualProperty: i32,
    pTCoeff: *mut i16,
    iIdx4x4: i32,
    uiQp: u8,
    pCtx: *mut SWelsDecoderContext,
) -> i32 {
    let mut iLevel = [0i32; 16];
    let mut iRun = [0i32; 16];
    let mut iMbResProperty: i32 = 0;
    GetMbResProperty(&mut iMbResProperty, &mut iResidualProperty, true);

    let kpDequantCoeff: *const u16 = if !pCtx.is_null() && (*pCtx).bUseScalingList && !(*pCtx).pDequant_coeff8x8[(iMbResProperty - 6) as usize].is_null() {
        (*(*pCtx).pDequant_coeff8x8[(iMbResProperty - 6) as usize].add(uiQp as usize)).as_ptr()
    } else {
        crate::decoder::parse_mb_syn_cabac::g_kuiDequantCoeff8x8[uiQp as usize].as_ptr()
    };

    let mut uiTotalCoeff: u8 = 0;
    let mut uiTrailingOnes: u8 = 0;
    let mut iUsedBits: i32 = 0;
    let iCurIdx = pBs.cavlc_bit_pos() as usize;
    let pBuf = buf.as_ptr().add(iCurIdx >> 3) as *mut u8;
    let bChromaDc = CHROMA_DC == iResidualProperty;

    let uiCache32Bit = ((*pBuf.add(0) as u32) << 24)
        | ((*pBuf.add(1) as u32) << 16)
        | ((*pBuf.add(2) as u32) << 8)
        | (*pBuf.add(3) as u32);

    let mut sReadBitsCache = SReadBitsCache {
        uiCache32Bit: uiCache32Bit << (iCurIdx & 0x07),
        uiRemainBits: 32 - (iCurIdx & 0x07) as u8,
        pBuf,
    };

    let iCurNonZeroCacheIdx = g_kuiCache48CountScan4Idx[iIndex as usize] as usize;
    let nA = pNonZeroCountCache[iCurNonZeroCacheIdx - 1] as i8;
    let nB = pNonZeroCountCache[iCurNonZeroCacheIdx - 8] as i8;
    let nC = wels_non_zero_count_average(nA, nB);

    iUsedBits += CavlcGetTrailingOnesAndTotalCoeff(
        &mut uiTotalCoeff,
        &mut uiTrailingOnes,
        &mut sReadBitsCache,
        pVlcTable,
        bChromaDc,
        nC,
    );

    if iResidualProperty != CHROMA_DC && iResidualProperty != I16_LUMA_DC {
        pNonZeroCountCache[iCurNonZeroCacheIdx] = uiTotalCoeff;
    }
    if 0 == uiTotalCoeff {
        pBs.advance_cavlc_bits(iUsedBits as isize);
        return ERR_NONE;
    }
    if uiTrailingOnes > 3 || uiTotalCoeff > 16 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_TOTAL_COEFF_OR_TRAILING_ONES);
    }
    let res = CavlcGetLevelVal(&mut iLevel, &mut sReadBitsCache, uiTotalCoeff, uiTrailingOnes);
    if res == -1 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_LEVEL);
    }
    iUsedBits += res;

    let mut iZerosLeft: i32 = 0;
    if (uiTotalCoeff as i32) < iMaxNumCoeff {
        iUsedBits += CavlcGetTotalZeros(&mut iZerosLeft, &mut sReadBitsCache, uiTotalCoeff, pVlcTable, bChromaDc);
    }

    if iZerosLeft < 0 || (iZerosLeft + uiTotalCoeff as i32) > iMaxNumCoeff {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_ZERO_LEFT);
    }
    let res = CavlcGetRunBefore(&mut iRun, &mut sReadBitsCache, uiTotalCoeff, pVlcTable, iZerosLeft);
    if res == -1 {
        return GENERATE_ERROR_NO(ERR_LEVEL_MB_DATA, ERR_INFO_CAVLC_INVALID_RUN_BEFORE);
    }
    iUsedBits += res;
    pBs.advance_cavlc_bits(iUsedBits as isize);
    let mut iCoeffNum: i32 = -1;

    for i in (0..(uiTotalCoeff as usize)).rev() {
        iCoeffNum += iRun[i] + 1;
        let j = (iCoeffNum << 2) + iIdx4x4;
        let j = kpZigzagTable[j as usize] as usize;
        let coeff = if uiQp >= 36 {
            (iLevel[i] * *kpDequantCoeff.add(j) as i32) * (1 << (uiQp as i32 / 6 - 6))
        } else {
            (iLevel[i] * *kpDequantCoeff.add(j) as i32 + (1 << (5 - uiQp as i32 / 6))) >> (6 - uiQp as i32 / 6)
        };
        *pTCoeff.add(j) = coeff as i16;
    }

    ERR_NONE
}

pub unsafe fn WelsParseMbCavlcResidual(
    pVlcTable: &SVlcTable,
    pNonZeroCountCache: &mut [u8; 48],
    buf: &[u8],
    pBs: &mut BsCursor,
    iIndex: i32,
    iMaxNumCoeff: i32,
    kpZigzagTable: &[u8],
    iResidualProperty: i32,
    pTCoeff: *mut i16,
    uiQp: u8,
    pCtx: *mut SWelsDecoderContext,
) -> i32 {
    WelsResidualBlockCavlc(
        pVlcTable,
        pNonZeroCountCache,
        buf,
        pBs,
        iIndex,
        iMaxNumCoeff,
        kpZigzagTable,
        iResidualProperty,
        pTCoeff,
        uiQp,
        pCtx,
    )
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_non_zero_count_average() {
        assert_eq!(wels_non_zero_count_average(3, 2), 3);
        assert_eq!(wels_non_zero_count_average(-1, 4), 4);
        assert_eq!(wels_non_zero_count_average(2, -1), 2);
        assert_eq!(wels_non_zero_count_average(-1, -1), 0);
    }

    #[test]
    fn test_pred_intra_4x4_mode() {
        let mut modes = [0i8; 64];
        // Set top and left modes
        modes[9 - 8] = 3; // Top
        modes[9 - 1] = 1; // Left
        unsafe {
            let res = PredIntra4x4Mode(modes.as_mut_ptr(), 0);
            assert_eq!(res, 1);
        }
    }

    #[test]
    fn test_check_intra16x16_pred_mode() {
        let mut mode: i8 = I16_PRED_DC;
        unsafe {
            let err = CheckIntra16x16PredMode(0x04, &mut mode);
            assert_eq!(err, ERR_NONE);
            assert_eq!(mode, I16_PRED_DC_L);
        }
    }

    #[test]
    fn test_chroma_dc_idct() {
        let mut blk = [0i16; 64];
        blk[0] = 10;
        blk[16] = 2;
        blk[32] = 4;
        blk[48] = 1;

        unsafe {
            WelsChromaDcIdct(blk.as_mut_ptr());
        }

        // iA=10, iB=2, iC=4, iD=1 -> iE=8, iA=12, iB=3, iC=5
        // blk[0] = 12+5 = 17
        // blk[16] = 8+3 = 11
        // blk[32] = 12-5 = 7
        // blk[48] = 8-3 = 5
        assert_eq!(blk[0], 17);
        assert_eq!(blk[16], 11);
        assert_eq!(blk[32], 7);
        assert_eq!(blk[48], 5);
    }

    #[test]
    fn test_cavlc_zero_coeff_block_decoding() {
        let buf = [0u8; 16];
        // The F10-class accommodation that used to be here is **deleted**: it existed
        // because `SBitStringAux` was built from three `as_mut_ptr()` calls, two of
        // which Stacked Borrows had already popped by the time the parser read through
        // them. There are no pointers left to reborrow.
        //
        // The setup is now the production sequence: `init` primes the accumulator, and
        // `start_cavlc` projects it to bit position 0 — `(4 << 3) - (16 - (-16))`. The
        // residual path asserts it is inside a CAVLC region, which is exactly the
        // discipline the mode exists to enforce (plan §2.2.2 [P3]).
        let mut bs = BsCursor::init(&buf, 128).unwrap();
        bs.start_cavlc();
        assert_eq!(bs.cavlc_bit_pos(), 0);
        let mut non_zero_cache = [0u8; 48];
        let mut coeffs = [0i16; 16];
        let zigzag = [0u8; 16];
        let mut vlc_table = SVlcTable {
            kpCoeffTokenVlcTable: [[std::ptr::null(); 8]; 4],
            kpChromaCoeffTokenVlcTable: std::ptr::null(),
            kpZeroTable: [std::ptr::null(); 7],
            kpTotalZerosTable: [[std::ptr::null(); 15]; 2],
        };
        InitVlcTable(&mut vlc_table);

        unsafe {
            let res = WelsParseMbCavlcResidual(
                &mut vlc_table,
                &mut non_zero_cache,
                &buf,
                &mut bs,
                0,
                16,
                &zigzag,
                0,
                coeffs.as_mut_ptr(),
                26,
                std::ptr::null_mut(),
            );
            assert_eq!(res, ERR_NONE);
            assert_eq!(non_zero_cache[g_kuiCache48CountScan4Idx[0] as usize], 0);
        }
    }
}
