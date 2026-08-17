// Copyright (c) 2010-2013, Cisco Systems
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

//! # OpenH264 Video Decoder: In-Loop Deblocking Filter
//!
//! Translated from `codec/decoder/core/inc/deblocking.h` and `codec/decoder/core/src/deblocking.cpp`.
//!
//! Implements the normative H.264/AVC in-loop adaptive deblocking filter for macroblocks,
//! including boundary strength (bS) derivation for P-slices and B-slices, slice-level iteration,
//! macroblock edge availability masks, and SIMD dispatch table initialization.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]


// ============================================================================
// Constants & Configuration Flags
// ============================================================================

pub const NO_SUPPORTED_FILTER_IDX: i32 = -1;
pub const LEFT_FLAG_BIT: i32 = 0;
pub const TOP_FLAG_BIT: i32 = 1;
pub const LEFT_FLAG_MASK: i32 = 0x01;
pub const TOP_FLAG_MASK: i32 = 0x02;

pub const MB_BLOCK4x4_NUM: usize = 16;
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const REF_NOT_IN_LIST: i8 = -1;
pub const MV_A: usize = 2;

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
pub const MB_TYPE_INTRA_BL: u32 = 0x00000400;
pub const MB_TYPE_DIRECT: u32 = 0x00000800;

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

#[inline(always)]
pub fn IS_INTRA(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA) != 0
}

#[inline(always)]
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_INTER_16x16(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_16x16) != 0
}

// CPU Feature Flags

// ============================================================================
// H.264 / AVC Static Deblocking Lookup Tables
// ============================================================================

/// Table 8-16: Alpha table with +12 index offset padding
// See the note in `encoder/deblocking.rs`: these three tables are file-local in the
// C++ and the decoder's are `[52 + 24]` where the encoder's are `[52 + 12]`.
pub static g_kuiAlphaTable: [u8; 52 + 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 4, 4, 5, 6,
    7, 8, 9, 10, 12, 13, 15, 17, 20, 22,
    25, 28, 32, 36, 40, 45, 50, 56, 63, 71,
    80, 90, 101, 113, 127, 144, 162, 182, 203, 226,
    255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];

/// Table 8-16: Beta table with +12 index offset padding
pub static g_kiBetaTable: [i8; 52 + 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 2, 2, 2, 3,
    3, 3, 3, 4, 4, 4, 6, 6, 7, 7,
    8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13, 14, 14, 15, 15, 16, 16, 17, 17,
    18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
];

/// Table 8-17: Tc0 table indexed by (IndexA + 12) and bS (0..3)
pub static g_kiTc0Table: [[i8; 4]; 52 + 24] = [
    [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0],
    [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0],
    [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0],
    [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0],
    [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 0], [-1, 0, 0, 1],
    [-1, 0, 0, 1], [-1, 0, 0, 1], [-1, 0, 0, 1], [-1, 0, 1, 1], [-1, 0, 1, 1], [-1, 1, 1, 1],
    [-1, 1, 1, 1], [-1, 1, 1, 1], [-1, 1, 1, 1], [-1, 1, 1, 2], [-1, 1, 1, 2], [-1, 1, 1, 2],
    [-1, 1, 1, 2], [-1, 1, 2, 3], [-1, 1, 2, 3], [-1, 2, 2, 3], [-1, 2, 2, 4], [-1, 2, 3, 4],
    [-1, 2, 3, 4], [-1, 3, 3, 5], [-1, 3, 4, 6], [-1, 3, 4, 6], [-1, 4, 5, 7], [-1, 4, 5, 8],
    [-1, 4, 6, 9], [-1, 5, 7, 10], [-1, 6, 8, 11], [-1, 6, 8, 13], [-1, 7, 10, 14], [-1, 8, 11, 16],
    [-1, 9, 12, 18], [-1, 10, 13, 20], [-1, 11, 15, 23], [-1, 13, 17, 25],
    [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25],
    [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25], [-1, 13, 17, 25],
];

pub static g_kuiTableBIdx: [[u8; 8]; 2] = [
    [0, 4, 8, 12, 3, 7, 11, 15],
    [0, 1, 2, 3, 12, 13, 14, 15],
];

pub static g_kuiTableB8x8Idx: [[u8; 16]; 2] = [
    [
        0, 1, 4, 5, 8, 9, 12, 13,
        2, 3, 6, 7, 10, 11, 14, 15,
    ],
    [
        0, 1, 4, 5, 2, 3, 6, 7,
        8, 9, 12, 13, 10, 11, 14, 15,
    ],
];

#[inline(always)]
pub fn alpha_table(x: i32) -> u8 {
    let idx = (x + 12) as usize;
    if idx < g_kuiAlphaTable.len() {
        g_kuiAlphaTable[idx]
    } else {
        255
    }
}

#[inline(always)]
pub fn beta_table(x: i32) -> i8 {
    let idx = (x + 12) as usize;
    if idx < g_kiBetaTable.len() {
        g_kiBetaTable[idx]
    } else {
        18
    }
}

#[inline(always)]
pub fn tc0_table(x: i32) -> &'static [i8; 4] {
    let idx = (x + 12) as usize;
    if idx < g_kiTc0Table.len() {
        &g_kiTc0Table[idx]
    } else {
        &g_kiTc0Table[g_kiTc0Table.len() - 1]
    }
}

pub use crate::common::deblocking_common::*;
use crate::safe::mb_grid::MbArray;
pub use crate::decoder::slice::EWelsSliceType;
pub use crate::decoder::picture::{SPicture, PPicture};
pub use crate::decoder::parameter_sets::{SSps, PSps, SPps, PPps};
pub use crate::decoder::slice::{SSliceHeader, PSliceHeader, SSliceHeaderExt, PSliceHeaderExt};
pub use crate::decoder::decoder_core::{SSlice, PSlice, SLayerInfo, DqLayerState, PDqLayer};
pub use crate::decoder::decoder_context::{
    SRefPic, SDeblockingFunc, SDeblockingFilter, PDeblockingFilter, PicId,
    MAX_DPB_COUNT,
    PLumaDeblockingLT4Func, PLumaDeblockingEQ4Func, PChromaDeblockingLT4Func,
    PChromaDeblockingEQ4Func, PChromaDeblockingLT4Func2, PChromaDeblockingEQ4Func2,
};

/// **T5.P′1 added `pDec`.** The layer used to carry the picture being decoded into,
/// so a macroblock filter reached it through `pCurDqLayer->pDec`; the field was a
/// cache of `dec_pic(pCtx)` and died with W2b. The slice loop derives once per
/// macroblock and hands it down, which is the same freshness the cache had and no
/// stored copy of it anywhere.
// T5.W5 (W6, family 10, partial): `pFilter` is `&mut SDeblockingFilter` at eighteen
// signatures — every function in this module that threads the filter except the one
// below, which the fn-pointer type pins.
//
// The flip is bounded and it is entirely local: the filter is a **stack local** of
// `WelsDeblockingFilterSlice` (`let mut pFilter = SDeblockingFilter::default()`),
// threaded down and out of nothing, so no other module names the type and no call
// site outside this file changed. Bodies did not change either — `(*pFilter).field`
// reads the same through a borrow.
//
// **`WelsDeblockingMb` keeps the raw `pFilter`, and that is a dispatch shape**:
// `PDeblockingFilterMbFunc` below is the per-macroblock filter's installed type and
// `WelsDeblockingMb` is what gets installed, so a parameter's type and the typedef
// move together or not at all — W6 step 3's class, with the four `decode_mb_aux.rs`
// `pIdct*Func` shims (family 3). It reborrows `&mut *pFilter` at its eight call sites.
// **Its layer parameter did move**, at T5.W7, because the layer flip forced the
// question and the answer was cheap: one typedef, one implementor, one install site
// (`decode_slice.rs:2567`), all named in `decoder_context.rs`'s own comment.
//
// What still blocks this module: the plane pointers (`pPix`/`pPixCb`/`pPixCr`, the
// eight edge filters — step 3), the layer (`pCurDqLayer`, step 2), `pDec`, and the
// boundary-strength arrays those same filters take by raw const pointer (spelled in
// words, not in the type, so this note does not raise the file's own count — S16).

pub type PDeblockingFilterMbFunc = unsafe extern "C" fn(
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    filter: *mut SDeblockingFilter,
    boundry_flag: i32,
);

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};
use crate::decoder::decoder_context::{active_fmo, pps_of, sps_of};


// ============================================================================
// Boundary Strength Evaluation Macros & Helper Primitives
// ============================================================================

#[inline(always)]
pub fn GET_ALPHA_BETA_FROM_QP(
    iQp: i32,
    iAlphaOffset: i32,
    iBetaOffset: i32,
    iIndexA: &mut i32,
    iAlpha: &mut i32,
    iBeta: &mut i32,
) {
    *iIndexA = iQp + iAlphaOffset;
    *iAlpha = alpha_table(*iIndexA) as i32;
    *iBeta = beta_table(iQp + iBetaOffset) as i32;
}

#[inline(always)]
pub fn TC0_TBL_LOOKUP(tc: &mut [i8; 4], iIndexA: i32, pBS: &[u8], bChroma: i8) {
    let tbl = tc0_table(iIndexA);
    tc[0] = tbl[(pBS[0] & 3) as usize] + bChroma;
    tc[1] = tbl[(pBS[1] & 3) as usize] + bChroma;
    tc[2] = tbl[(pBS[2] & 3) as usize] + bChroma;
    tc[3] = tbl[(pBS[3] & 3) as usize] + bChroma;
}

#[inline(always)]
pub unsafe fn MB_BS_MV(
    pRefPic0: Option<PicId>,
    pRefPic1: Option<PicId>,
    iMotionVector: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMbXy: usize,
    iMbBn: usize,
    iIndex: usize,
    iNeighIndex: usize,
) -> u8 {
    if pRefPic0 != pRefPic1 {
        return 1;
    }
    let mv_curr = iMotionVector.get(iMbXy)[iIndex];
    let mv_neigh = iMotionVector.get(iMbBn)[iNeighIndex];
    if (mv_curr[0] as i32 - mv_neigh[0] as i32).abs() >= 4
        || (mv_curr[1] as i32 - mv_neigh[1] as i32).abs() >= 4
    {
        1
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn ON_MB_BS_MV_DIFF(
    iMV_A: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMV_B: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMbXy: usize,
    iMbBn: usize,
    iIndex: usize,
    iNeighIndex: usize,
) -> bool {
    let mv_a = iMV_A.get(iMbXy)[iIndex];
    let mv_b = iMV_B.get(iMbBn)[iNeighIndex];
    (mv_a[0] as i32 - mv_b[0] as i32).abs() >= 4 || (mv_a[1] as i32 - mv_b[1] as i32).abs() >= 4
}

#[inline(always)]
pub unsafe fn IN_MB_BS_MV_DIFF(
    iMV_A: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMV_B: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMbXy: usize,
    iIndex: usize,
    iNeighIndex: usize,
) -> bool {
    let mv_a = iMV_A.get(iMbXy)[iIndex];
    let mv_b = iMV_B.get(iMbXy)[iNeighIndex];
    (mv_a[0] as i32 - mv_b[0] as i32).abs() >= 4 || (mv_a[1] as i32 - mv_b[1] as i32).abs() >= 4
}

#[inline(always)]
pub unsafe fn ON_MB_BS(
    ref_p0: Option<PicId>,
    ref_q0: Option<PicId>,
    ref_p1: Option<PicId>,
    ref_q1: Option<PicId>,
    mv0: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    mv1: &MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>,
    iMbXy: usize,
    iMbBn: usize,
    iIndex: usize,
    iNeighIndex: usize,
) -> u8 {
    let res = if ref_p0 != ref_p1 {
        if ref_p0 == ref_q0 {
            ON_MB_BS_MV_DIFF(mv0, mv0, iMbXy, iMbBn, iIndex, iNeighIndex)
                || ON_MB_BS_MV_DIFF(mv1, mv1, iMbXy, iMbBn, iIndex, iNeighIndex)
        } else {
            ON_MB_BS_MV_DIFF(mv0, mv1, iMbXy, iMbBn, iIndex, iNeighIndex)
                || ON_MB_BS_MV_DIFF(mv1, mv0, iMbXy, iMbBn, iIndex, iNeighIndex)
        }
    } else {
        (ON_MB_BS_MV_DIFF(mv0, mv0, iMbXy, iMbBn, iIndex, iNeighIndex)
            || ON_MB_BS_MV_DIFF(mv1, mv1, iMbXy, iMbBn, iIndex, iNeighIndex))
            && (ON_MB_BS_MV_DIFF(mv0, mv1, iMbXy, iMbBn, iIndex, iNeighIndex)
                || ON_MB_BS_MV_DIFF(mv1, mv0, iMbXy, iMbBn, iIndex, iNeighIndex))
    };
    if res { 1 } else { 0 }
}

#[inline(always)]
pub unsafe fn SMB_EDGE_MV(
    pRefIds: &[Option<PicId>; MB_BLOCK4x4_NUM],
    iMotionVector: &[[i16; MV_A]; MB_BLOCK4x4_NUM],
    iIndex: usize,
    iNeighIndex: usize,
) -> u8 {
    let p_ref0 = pRefIds[iIndex];
    let p_ref1 = pRefIds[iNeighIndex];
    if p_ref0 != p_ref1 {
        return 1;
    }
    let mv0 = iMotionVector[iIndex];
    let mv1 = iMotionVector[iNeighIndex];
    let diff0 = (mv0[0] as i32 - mv1[0] as i32).abs();
    let diff1 = (mv0[1] as i32 - mv1[1] as i32).abs();
    if ((diff0 & !3) | (diff1 & !3)) != 0 {
        1
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn BS_EDGE(
    bsx1: u8,
    pRefIds: &[Option<PicId>; MB_BLOCK4x4_NUM],
    iMotionVector: &[[i16; MV_A]; MB_BLOCK4x4_NUM],
    iIndex: usize,
    iNeighIndex: usize,
) -> u8 {
    let smb = SMB_EDGE_MV(pRefIds, iMotionVector, iIndex, iNeighIndex);
    (bsx1 | smb) << (if bsx1 != 0 { 1 } else { 0 })
}

#[inline(always)]
pub unsafe fn IN_SMB_EDGE_MV(
    refs: &[[Option<PicId>; MB_BLOCK4x4_NUM]; LIST_A],
    mv: &[&MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>; LIST_A],
    iMbXy: usize,
    iIndex: usize,
    iNeighborIndex: usize,
) -> u8 {
    let cond1 = (refs[LIST_0][iIndex] == refs[LIST_0][iNeighborIndex])
        && (refs[LIST_1][iIndex] == refs[LIST_1][iNeighborIndex]);
    let cond2 = (refs[LIST_0][iIndex] == refs[LIST_1][iNeighborIndex])
        && (refs[LIST_1][iIndex] == refs[LIST_0][iNeighborIndex]);

    if cond1 || cond2 {
        if refs[LIST_0][iIndex] != refs[LIST_1][iIndex] {
            if refs[LIST_0][iIndex] == refs[LIST_0][iNeighborIndex] {
                if IN_MB_BS_MV_DIFF(mv[LIST_0], mv[LIST_0], iMbXy, iIndex, iNeighborIndex)
                    || IN_MB_BS_MV_DIFF(mv[LIST_1], mv[LIST_1], iMbXy, iIndex, iNeighborIndex)
                {
                    1
                } else {
                    0
                }
            } else {
                if IN_MB_BS_MV_DIFF(mv[LIST_0], mv[LIST_1], iMbXy, iIndex, iNeighborIndex)
                    || IN_MB_BS_MV_DIFF(mv[LIST_1], mv[LIST_0], iMbXy, iIndex, iNeighborIndex)
                {
                    1
                } else {
                    0
                }
            }
        } else {
            let a = IN_MB_BS_MV_DIFF(mv[LIST_0], mv[LIST_0], iMbXy, iIndex, iNeighborIndex)
                || IN_MB_BS_MV_DIFF(mv[LIST_1], mv[LIST_1], iMbXy, iIndex, iNeighborIndex);
            let b = IN_MB_BS_MV_DIFF(mv[LIST_0], mv[LIST_1], iMbXy, iIndex, iNeighborIndex)
                || IN_MB_BS_MV_DIFF(mv[LIST_1], mv[LIST_0], iMbXy, iIndex, iNeighborIndex);
            if a && b { 1 } else { 0 }
        }
    } else {
        1
    }
}

#[inline(always)]
pub unsafe fn IN_BS_EDGE(
    bsx1: u8,
    refs: &[[Option<PicId>; MB_BLOCK4x4_NUM]; LIST_A],
    mv: &[&MbArray<[[i16; MV_A]; MB_BLOCK4x4_NUM]>; LIST_A],
    iMbXy: usize,
    iIndex: usize,
    iNeighborIndex: usize,
) -> u8 {
    let smb = IN_SMB_EDGE_MV(refs, mv, iMbXy, iIndex, iNeighborIndex);
    (bsx1 | smb) << (if bsx1 != 0 { 1 } else { 0 })
}

/// The macroblock's non-zero-count row.
///
/// The C++ picks between two sources here — the decoded picture's own `pNzc` array and
/// the dq-layer's — because under decoder multi-threading the picture carries its own
/// copy. That array was only ever allocated behind a thread-count gate that this port
/// never opened, so the picture branch was unreachable for the port's whole life and
/// died with the rest of the MT scaffolding (T5c). Only the layer source remains.
///
/// T5.L1: every one of the eight uses reads, and two of them
/// (`DeblockingBsMarginalMBAvcbase`, `DeblockingBSliceBsMarginalMBAvcbase`) hold the
/// current macroblock's row and its neighbour's **at the same time** — so this is a
/// shared borrow of the owned array rather than a raw bridge. Two `&mut`s would have
/// been F34's shape: the second retag pops the first. Every consumer stays inside the
/// 24-byte record (`from_raw_parts(pNnzTab, 24)` at each of the four, and index tables
/// `g_kuiTableB8x8Idx`/`g_kuiMbCountScan4Idx` whose entries are all < 24), so the
/// per-element derivation reaches everything it is asked to reach.
#[inline(always)]
pub fn GetPNzc(pCurDqLayer: &DqLayerState, iMbXy: i32) -> &[i8; 24] {
    // T5.W14: the record itself, not its first byte. `MbArray<[i8; 24]>` already knew
    // the span the four consumers were re-asserting with `from_raw_parts(…, 24)`.
    (*pCurDqLayer).grid.nzc.get(iMbXy as usize)
}

// ============================================================================
// Internal Boundary Strength Matrix Computations
// ============================================================================

#[inline(always)]
pub unsafe fn DeblockingBSInsideMBAvsbase(
    pNnzTab: &[i8; 24],
    nBS: &mut [[[u8; 4]; 4]; 2],
    iLShiftFactor: i32,
) {
    let shift = iLShiftFactor as u32;

    nBS[0][1][0] = ((pNnzTab[0] as u8) | (pNnzTab[1] as u8)) << shift;
    nBS[0][2][0] = ((pNnzTab[1] as u8) | (pNnzTab[2] as u8)) << shift;
    nBS[0][3][0] = ((pNnzTab[2] as u8) | (pNnzTab[3] as u8)) << shift;

    nBS[0][1][1] = ((pNnzTab[4] as u8) | (pNnzTab[5] as u8)) << shift;
    nBS[0][2][1] = ((pNnzTab[5] as u8) | (pNnzTab[6] as u8)) << shift;
    nBS[0][3][1] = ((pNnzTab[6] as u8) | (pNnzTab[7] as u8)) << shift;

    nBS[1][1][0] = ((pNnzTab[0] as u8) | (pNnzTab[4] as u8)) << shift;
    nBS[1][1][1] = ((pNnzTab[1] as u8) | (pNnzTab[5] as u8)) << shift;
    nBS[1][1][2] = ((pNnzTab[2] as u8) | (pNnzTab[6] as u8)) << shift;
    nBS[1][1][3] = ((pNnzTab[3] as u8) | (pNnzTab[7] as u8)) << shift;

    nBS[0][1][2] = ((pNnzTab[8] as u8) | (pNnzTab[9] as u8)) << shift;
    nBS[0][2][2] = ((pNnzTab[9] as u8) | (pNnzTab[10] as u8)) << shift;
    nBS[0][3][2] = ((pNnzTab[10] as u8) | (pNnzTab[11] as u8)) << shift;

    nBS[1][2][0] = ((pNnzTab[4] as u8) | (pNnzTab[8] as u8)) << shift;
    nBS[1][2][1] = ((pNnzTab[5] as u8) | (pNnzTab[9] as u8)) << shift;
    nBS[1][2][2] = ((pNnzTab[6] as u8) | (pNnzTab[10] as u8)) << shift;
    nBS[1][2][3] = ((pNnzTab[7] as u8) | (pNnzTab[11] as u8)) << shift;

    nBS[0][1][3] = ((pNnzTab[12] as u8) | (pNnzTab[13] as u8)) << shift;
    nBS[0][2][3] = ((pNnzTab[13] as u8) | (pNnzTab[14] as u8)) << shift;
    nBS[0][3][3] = ((pNnzTab[14] as u8) | (pNnzTab[15] as u8)) << shift;

    nBS[1][3][0] = ((pNnzTab[8] as u8) | (pNnzTab[12] as u8)) << shift;
    nBS[1][3][1] = ((pNnzTab[9] as u8) | (pNnzTab[13] as u8)) << shift;
    nBS[1][3][2] = ((pNnzTab[10] as u8) | (pNnzTab[14] as u8)) << shift;
    nBS[1][3][3] = ((pNnzTab[11] as u8) | (pNnzTab[15] as u8)) << shift;
}

#[inline(always)]
pub unsafe fn DeblockingBSInsideMBAvsbase8x8(
    pNnzTab: &[i8; 24],
    nBS: &mut [[[u8; 4]; 4]; 2],
    iLShiftFactor: i32,
) {
    let mut i8x8NnzTab = [0u8; 4];
    for i in 0..4 {
        let iBlkIdx = i << 2;
        i8x8NnzTab[i] = (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx] as usize] as u8)
            | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 1] as usize] as u8)
            | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 2] as usize] as u8)
            | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 3] as usize] as u8);
    }

    let shift = iLShiftFactor as u32;
    // vertical
    let val_v0 = (i8x8NnzTab[0] | i8x8NnzTab[1]) << shift;
    nBS[0][2][0] = val_v0;
    nBS[0][2][1] = val_v0;

    let val_v1 = (i8x8NnzTab[2] | i8x8NnzTab[3]) << shift;
    nBS[0][2][2] = val_v1;
    nBS[0][2][3] = val_v1;

    // horizontal
    let val_h0 = (i8x8NnzTab[0] | i8x8NnzTab[2]) << shift;
    nBS[1][2][0] = val_h0;
    nBS[1][2][1] = val_h0;

    let val_h1 = (i8x8NnzTab[1] | i8x8NnzTab[3]) << shift;
    nBS[1][2][2] = val_h1;
    nBS[1][2][3] = val_h1;
}

#[inline(always)]
pub unsafe fn DeblockingBSInsideMBNormal(
    pFilter: &mut SDeblockingFilter,
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    nBS: &mut [[[u8; 4]; 4]; 2],
    pNnzTab: &[i8; 24],
    iMbXy: i32,
) {
    let iRefIdx = *(*pDec).pRefIndex[LIST_0].get(iMbXy as usize);
    let mut iRefs: [Option<PicId>; MB_BLOCK4x4_NUM] = [None; MB_BLOCK4x4_NUM];
    for i in 0..MB_BLOCK4x4_NUM {
        if iRefIdx[i] > REF_NOT_IN_LIST {
            iRefs[i] = (*pFilter).ref_ids[LIST_0][iRefIdx[i] as usize];
        } else {
            iRefs[i] = None;
        }
    }

    let is_8x8 = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy as usize);
    // T5.P′3: the picture's array is owned, and `BS_EDGE` reads one macroblock's
    // record by index, so the bridge is taken at the record rather than at the base.
    let pMv = (*pDec).pMv[LIST_0].get(iMbXy as usize);

    if is_8x8 {
        let mut i8x8NnzTab = [0u8; 4];
        for i in 0..4 {
            let iBlkIdx = i << 2;
            i8x8NnzTab[i] = (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 1] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 2] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 3] as usize] as u8);
        }

        let val_v0 = BS_EDGE(
            i8x8NnzTab[0] | i8x8NnzTab[1],
            &iRefs,
            pMv,
            g_kuiMbCountScan4Idx[1 << 2] as usize,
            g_kuiMbCountScan4Idx[0] as usize,
        );
        nBS[0][2][0] = val_v0;
        nBS[0][2][1] = val_v0;

        let val_v1 = BS_EDGE(
            i8x8NnzTab[2] | i8x8NnzTab[3],
            &iRefs,
            pMv,
            g_kuiMbCountScan4Idx[3 << 2] as usize,
            g_kuiMbCountScan4Idx[2 << 2] as usize,
        );
        nBS[0][2][2] = val_v1;
        nBS[0][2][3] = val_v1;

        let val_h0 = BS_EDGE(
            i8x8NnzTab[0] | i8x8NnzTab[2],
            &iRefs,
            pMv,
            g_kuiMbCountScan4Idx[2 << 2] as usize,
            g_kuiMbCountScan4Idx[0] as usize,
        );
        nBS[1][2][0] = val_h0;
        nBS[1][2][1] = val_h0;

        let val_h1 = BS_EDGE(
            i8x8NnzTab[1] | i8x8NnzTab[3],
            &iRefs,
            pMv,
            g_kuiMbCountScan4Idx[3 << 2] as usize,
            g_kuiMbCountScan4Idx[1 << 2] as usize,
        );
        nBS[1][2][2] = val_h1;
        nBS[1][2][3] = val_h1;
    } else {
        let mut uiBsx4 = [0u8; 4];

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[i] as u8) | (pNnzTab[i + 1] as u8);
        }
        nBS[0][1][0] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 1, 0);
        nBS[0][2][0] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 2, 1);
        nBS[0][3][0] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 3, 2);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[4 + i] as u8) | (pNnzTab[4 + i + 1] as u8);
        }
        nBS[0][1][1] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 5, 4);
        nBS[0][2][1] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 6, 5);
        nBS[0][3][1] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 7, 6);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[8 + i] as u8) | (pNnzTab[8 + i + 1] as u8);
        }
        nBS[0][1][2] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 9, 8);
        nBS[0][2][2] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 10, 9);
        nBS[0][3][2] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 11, 10);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[12 + i] as u8) | (pNnzTab[12 + i + 1] as u8);
        }
        nBS[0][1][3] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 13, 12);
        nBS[0][2][3] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 14, 13);
        nBS[0][3][3] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 15, 14);

        // horizontal
        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[i] as u8) | (pNnzTab[4 + i] as u8);
        }
        nBS[1][1][0] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 4, 0);
        nBS[1][1][1] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 5, 1);
        nBS[1][1][2] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 6, 2);
        nBS[1][1][3] = BS_EDGE(uiBsx4[3], &iRefs, pMv, 7, 3);

        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[4 + i] as u8) | (pNnzTab[8 + i] as u8);
        }
        nBS[1][2][0] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 8, 4);
        nBS[1][2][1] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 9, 5);
        nBS[1][2][2] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 10, 6);
        nBS[1][2][3] = BS_EDGE(uiBsx4[3], &iRefs, pMv, 11, 7);

        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[8 + i] as u8) | (pNnzTab[12 + i] as u8);
        }
        nBS[1][3][0] = BS_EDGE(uiBsx4[0], &iRefs, pMv, 12, 8);
        nBS[1][3][1] = BS_EDGE(uiBsx4[1], &iRefs, pMv, 13, 9);
        nBS[1][3][2] = BS_EDGE(uiBsx4[2], &iRefs, pMv, 14, 10);
        nBS[1][3][3] = BS_EDGE(uiBsx4[3], &iRefs, pMv, 15, 11);
    }
}

#[inline(always)]
pub unsafe fn DeblockingBSliceBSInsideMBNormal(
    pFilter: &mut SDeblockingFilter,
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    nBS: &mut [[[u8; 4]; 4]; 2],
    pNnzTab: &[i8; 24],
    iMbXy: i32,
) {
    let mut iRefs: [[Option<PicId>; MB_BLOCK4x4_NUM]; LIST_A] = [[None; MB_BLOCK4x4_NUM]; LIST_A];

    for l in 0..LIST_A {
        let iRefIdx = *(*pDec).pRefIndex[l].get(iMbXy as usize);
        for i in 0..MB_BLOCK4x4_NUM {
            if iRefIdx[i] > REF_NOT_IN_LIST {
                iRefs[l][i] = (*pFilter).ref_ids[l][iRefIdx[i] as usize];
            } else {
                iRefs[l][i] = None;
            }
        }
    }

    // T5.P′3: two bridges, one per list, each from its own allocation root (S28) —
    // `IN_BS_EDGE` indexes them by macroblock address.
    let pMv = [&(*pDec).pMv[LIST_0], &(*pDec).pMv[LIST_1]];
    let is_8x8 = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy as usize);

    if is_8x8 {
        let mut i8x8NnzTab = [0u8; 4];
        for i in 0..4 {
            let iBlkIdx = i << 2;
            i8x8NnzTab[i] = (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 1] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 2] as usize] as u8)
                | (pNnzTab[g_kuiMbCountScan4Idx[iBlkIdx + 3] as usize] as u8);
        }

        let iIndex_v0 = g_kuiMbCountScan4Idx[1 << 2] as usize;
        let iNeigborIndex_v0 = g_kuiMbCountScan4Idx[0] as usize;
        let val_v0 = IN_BS_EDGE(
            i8x8NnzTab[0] | i8x8NnzTab[1],
            &iRefs,
            &pMv,
            iMbXy as usize,
            iIndex_v0,
            iNeigborIndex_v0,
        );
        nBS[0][2][0] = val_v0;
        nBS[0][2][1] = val_v0;

        let iIndex_v1 = g_kuiMbCountScan4Idx[3 << 2] as usize;
        let iNeigborIndex_v1 = g_kuiMbCountScan4Idx[2 << 2] as usize;
        let val_v1 = IN_BS_EDGE(
            i8x8NnzTab[2] | i8x8NnzTab[3],
            &iRefs,
            &pMv,
            iMbXy as usize,
            iIndex_v1,
            iNeigborIndex_v1,
        );
        nBS[0][2][2] = val_v1;
        nBS[0][2][3] = val_v1;

        let iIndex_h0 = g_kuiMbCountScan4Idx[2 << 2] as usize;
        let iNeigborIndex_h0 = g_kuiMbCountScan4Idx[0] as usize;
        let val_h0 = IN_BS_EDGE(
            i8x8NnzTab[0] | i8x8NnzTab[2],
            &iRefs,
            &pMv,
            iMbXy as usize,
            iIndex_h0,
            iNeigborIndex_h0,
        );
        nBS[1][2][0] = val_h0;
        nBS[1][2][1] = val_h0;

        let iIndex_h1 = g_kuiMbCountScan4Idx[3 << 2] as usize;
        let iNeigborIndex_h1 = g_kuiMbCountScan4Idx[1 << 2] as usize;
        let val_h1 = IN_BS_EDGE(
            i8x8NnzTab[1] | i8x8NnzTab[3],
            &iRefs,
            &pMv,
            iMbXy as usize,
            iIndex_h1,
            iNeigborIndex_h1,
        );
        nBS[1][2][2] = val_h1;
        nBS[1][2][3] = val_h1;
    } else {
        let mut uiBsx4 = [0u8; 4];

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[i] as u8) | (pNnzTab[i + 1] as u8);
        }
        nBS[0][1][0] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 1, 0);
        nBS[0][2][0] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 2, 1);
        nBS[0][3][0] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 3, 2);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[4 + i] as u8) | (pNnzTab[4 + i + 1] as u8);
        }
        nBS[0][1][1] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 5, 4);
        nBS[0][2][1] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 6, 5);
        nBS[0][3][1] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 7, 6);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[8 + i] as u8) | (pNnzTab[8 + i + 1] as u8);
        }
        nBS[0][1][2] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 9, 8);
        nBS[0][2][2] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 10, 9);
        nBS[0][3][2] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 11, 10);

        for i in 0..3 {
            uiBsx4[i] = (pNnzTab[12 + i] as u8) | (pNnzTab[12 + i + 1] as u8);
        }
        nBS[0][1][3] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 13, 12);
        nBS[0][2][3] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 14, 13);
        nBS[0][3][3] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 15, 14);

        // horizontal
        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[i] as u8) | (pNnzTab[4 + i] as u8);
        }
        nBS[1][1][0] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 4, 0);
        nBS[1][1][1] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 5, 1);
        nBS[1][1][2] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 6, 2);
        nBS[1][1][3] = IN_BS_EDGE(uiBsx4[3], &iRefs, &pMv, iMbXy as usize, 7, 3);

        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[4 + i] as u8) | (pNnzTab[8 + i] as u8);
        }
        nBS[1][2][0] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 8, 4);
        nBS[1][2][1] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 9, 5);
        nBS[1][2][2] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 10, 6);
        nBS[1][2][3] = IN_BS_EDGE(uiBsx4[3], &iRefs, &pMv, iMbXy as usize, 11, 7);

        for i in 0..4 {
            uiBsx4[i] = (pNnzTab[8 + i] as u8) | (pNnzTab[12 + i] as u8);
        }
        nBS[1][3][0] = IN_BS_EDGE(uiBsx4[0], &iRefs, &pMv, iMbXy as usize, 12, 8);
        nBS[1][3][1] = IN_BS_EDGE(uiBsx4[1], &iRefs, &pMv, iMbXy as usize, 13, 9);
        nBS[1][3][2] = IN_BS_EDGE(uiBsx4[2], &iRefs, &pMv, iMbXy as usize, 14, 10);
        nBS[1][3][3] = IN_BS_EDGE(uiBsx4[3], &iRefs, &pMv, iMbXy as usize, 15, 11);
    }
}

// ============================================================================
// Marginal Boundary Strength Calculation Routines
// ============================================================================

pub unsafe fn DeblockingBsMarginalMBAvcbase(
    pFilter: &mut SDeblockingFilter,
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    iEdge: i32,
    iNeighMb: i32,
    iMbXy: i32,
) -> u32 {
    // T5.X5: `uiBSx4` was a `u32` written through a `*mut u8` byte view and
    // returned whole. It is the four bytes it always was; the one word the
    // caller wants is assembled at the `return`, in the same native order the
    // pun had.
    let mut pBS = [0u8; 4];

    let pBIdx = &g_kuiTableBIdx[iEdge as usize][0..4];
    let pBnIdx = &g_kuiTableBIdx[iEdge as usize][4..8];
    let pB8x8Idx = &g_kuiTableB8x8Idx[iEdge as usize][0..8];
    let pBn8x8Idx = &g_kuiTableB8x8Idx[iEdge as usize][8..16];

    let pRefIdxArr = if !pDec.is_null() {
        // T5.P′3: the picture's array joins the layer's on the same bridge.
        &(*pDec).pRefIndex[LIST_0]
    } else {
        // T5.J3: the grid's array, derived from the allocation root (S28) — the
        // consumer indexes it by macroblock address, so it must reach the whole
        // array and a narrowing slice would be UB at the second index.
        &(*pCurDqLayer).grid.ref_index[LIST_0]
    };

    let is_8x8_curr = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy as usize);
    let is_8x8_neigh = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iNeighMb as usize);

    let pMvArr = if !pDec.is_null() {
        &(*pDec).pMv[LIST_0]
    } else {
        // T5.K1: the grid's array, derived from the allocation root (S28) — `MB_BS_MV`
        // indexes it by macroblock address for the current macroblock *and* its
        // neighbour, so a narrowing slice would be UB at the second index. Same shape
        // as `pRefIdxArr` above, and a second bridge in this function is sound for the
        // same reason the first one is: the two `&mut`s are of different fields, whose
        // `Vec`s are different allocations.
        &(*pCurDqLayer).grid.mv[LIST_0]
    };

    let pNzcCurr = GetPNzc(pCurDqLayer, iMbXy);
    let pNzcNeigh = GetPNzc(pCurDqLayer, iNeighMb);

    if is_8x8_curr && is_8x8_neigh {
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                if uiNzc == 0 {
                    uiNzc |= (pNzcCurr[pB8x8Idx[i * 4 + j] as usize] as u8)
                        | (pNzcNeigh[pBn8x8Idx[i * 4 + j] as usize] as u8);
                }
            }
            if uiNzc != 0 {
                pBS[i << 1] = 2;
                pBS[1 + (i << 1)] = 2;
            } else {
                let idx_curr = pB8x8Idx[i * 4] as usize;
                let idx_neigh = pBn8x8Idx[i * 4] as usize;
                let ref_idx0 = pRefIdxArr.get(iMbXy as usize)[idx_curr];
                let ref_idx1 = pRefIdxArr.get(iNeighMb as usize)[idx_neigh];

                let ref0 = if ref_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref_idx0 as usize]
                } else {
                    None
                };
                let ref1 = if ref_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref_idx1 as usize]
                } else {
                    None
                };

                let val = MB_BS_MV(
                    ref0,
                    ref1,
                    pMvArr,
                    iMbXy as usize,
                    iNeighMb as usize,
                    idx_curr,
                    idx_neigh,
                );
                pBS[i << 1] = val;
                pBS[1 + (i << 1)] = val;
            }
        }
    } else if is_8x8_curr {
        let mut bn_idx_pos = 0usize;
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                uiNzc |= (pNzcCurr[pB8x8Idx[i * 4 + j] as usize] as u8);
            }
            for j in 0..2 {
                let bn_idx = pBnIdx[bn_idx_pos] as usize;
                if (uiNzc | (pNzcNeigh[bn_idx] as u8)) != 0 {
                    pBS[j + (i << 1)] = 2;
                } else {
                    let b_idx = pB8x8Idx[i * 4] as usize;
                    let ref_idx0 = pRefIdxArr.get(iMbXy as usize)[b_idx];
                    let ref_idx1 = pRefIdxArr.get(iNeighMb as usize)[bn_idx];

                    let ref0 = if ref_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref_idx0 as usize]
                    } else {
                        None
                    };
                    let ref1 = if ref_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref_idx1 as usize]
                    } else {
                        None
                    };

                    pBS[j + (i << 1)] = MB_BS_MV(
                        ref0,
                        ref1,
                        pMvArr,
                        iMbXy as usize,
                        iNeighMb as usize,
                        b_idx,
                        bn_idx,
                    );
                }
                bn_idx_pos += 1;
            }
        }
    } else if is_8x8_neigh {
        let mut b_idx_pos = 0usize;
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                uiNzc |= (pNzcNeigh[pBn8x8Idx[i * 4 + j] as usize] as u8);
            }
            for j in 0..2 {
                let b_idx = pBIdx[b_idx_pos] as usize;
                if (uiNzc | (pNzcCurr[b_idx] as u8)) != 0 {
                    pBS[j + (i << 1)] = 2;
                } else {
                    let bn_idx = pBn8x8Idx[i * 4] as usize;
                    let ref_idx0 = pRefIdxArr.get(iMbXy as usize)[b_idx];
                    let ref_idx1 = pRefIdxArr.get(iNeighMb as usize)[bn_idx];

                    let ref0 = if ref_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref_idx0 as usize]
                    } else {
                        None
                    };
                    let ref1 = if ref_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref_idx1 as usize]
                    } else {
                        None
                    };

                    pBS[j + (i << 1)] = MB_BS_MV(
                        ref0,
                        ref1,
                        pMvArr,
                        iMbXy as usize,
                        iNeighMb as usize,
                        b_idx,
                        bn_idx,
                    );
                }
                b_idx_pos += 1;
            }
        }
    } else {
        // only 4x4 transform
        for i in 0..4 {
            let b_idx = pBIdx[i] as usize;
            let bn_idx = pBnIdx[i] as usize;
            if ((pNzcCurr[b_idx] as u8) | (pNzcNeigh[bn_idx] as u8)) != 0 {
                pBS[i] = 2;
            } else {
                let ref_idx0 = pRefIdxArr.get(iMbXy as usize)[b_idx];
                let ref_idx1 = pRefIdxArr.get(iNeighMb as usize)[bn_idx];

                let ref0 = if ref_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref_idx0 as usize]
                } else {
                    None
                };
                let ref1 = if ref_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref_idx1 as usize]
                } else {
                    None
                };

                pBS[i] = MB_BS_MV(
                    ref0,
                    ref1,
                    pMvArr,
                    iMbXy as usize,
                    iNeighMb as usize,
                    b_idx,
                    bn_idx,
                );
            }
        }
    }

    u32::from_ne_bytes(pBS)
}

pub unsafe fn DeblockingBSliceBsMarginalMBAvcbase(
    pFilter: &mut SDeblockingFilter,
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    iEdge: i32,
    iNeighMb: i32,
    iMbXy: i32,
) -> u32 {
    // T5.X5: `uiBSx4` was a `u32` written through a `*mut u8` byte view and
    // returned whole. It is the four bytes it always was; the one word the
    // caller wants is assembled at the `return`, in the same native order the
    // pun had.
    let mut pBS = [0u8; 4];

    let pBIdx = &g_kuiTableBIdx[iEdge as usize][0..4];
    let pBnIdx = &g_kuiTableBIdx[iEdge as usize][4..8];
    let pB8x8Idx = &g_kuiTableB8x8Idx[iEdge as usize][0..8];
    let pBn8x8Idx = &g_kuiTableB8x8Idx[iEdge as usize][8..16];

    let iRefIdx0 = &(*pDec).pRefIndex[LIST_0];
    let iRefIdx1 = &(*pDec).pRefIndex[LIST_1];

    let pNzcCurr = GetPNzc(pCurDqLayer, iMbXy);
    let pNzcNeigh = GetPNzc(pCurDqLayer, iNeighMb);

    let is_8x8_curr = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy as usize);
    let is_8x8_neigh = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iNeighMb as usize);

    if is_8x8_curr && is_8x8_neigh {
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                if uiNzc == 0 {
                    uiNzc |= (pNzcCurr[pB8x8Idx[i * 4 + j] as usize] as u8)
                        | (pNzcNeigh[pBn8x8Idx[i * 4 + j] as usize] as u8);
                }
            }
            if uiNzc != 0 {
                pBS[i << 1] = 2;
                pBS[1 + (i << 1)] = 2;
            } else {
                pBS[i << 1] = 1;
                pBS[1 + (i << 1)] = 1;

                let b_idx = pB8x8Idx[i * 4] as usize;
                let bn_idx = pBn8x8Idx[i * 4] as usize;

                let ref0_idx0 = iRefIdx0.get(iMbXy as usize)[b_idx];
                let ref0_idx1 = iRefIdx0.get(iNeighMb as usize)[bn_idx];
                let ref1_idx0 = iRefIdx1.get(iMbXy as usize)[b_idx];
                let ref1_idx1 = iRefIdx1.get(iNeighMb as usize)[bn_idx];

                let ref_p0 = if ref0_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref0_idx0 as usize]
                } else {
                    None
                };
                let ref_q0 = if ref0_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref0_idx1 as usize]
                } else {
                    None
                };
                let ref_p1 = if ref1_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_1][ref1_idx0 as usize]
                } else {
                    None
                };
                let ref_q1 = if ref1_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_1][ref1_idx1 as usize]
                } else {
                    None
                };

                if ((ref_p0 == ref_q0) && (ref_p1 == ref_q1))
                    || ((ref_p0 == ref_q1) && (ref_p1 == ref_q0))
                {
                    let pMv0 = &(*pDec).pMv[LIST_0];
                    let pMv1 = &(*pDec).pMv[LIST_1];
                    let val = ON_MB_BS(
                        ref_p0,
                        ref_q0,
                        ref_p1,
                        ref_q1,
                        pMv0,
                        pMv1,
                        iMbXy as usize,
                        iNeighMb as usize,
                        b_idx,
                        bn_idx,
                    );
                    pBS[i << 1] = val;
                    pBS[1 + (i << 1)] = val;
                }
            }
        }
    } else if is_8x8_curr {
        let mut bn_idx_pos = 0usize;
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                uiNzc |= (pNzcCurr[pB8x8Idx[i * 4 + j] as usize] as u8);
            }
            for j in 0..2 {
                let bn_idx = pBnIdx[bn_idx_pos] as usize;
                if (uiNzc | (pNzcNeigh[bn_idx] as u8)) != 0 {
                    pBS[j + (i << 1)] = 2;
                } else {
                    pBS[j + (i << 1)] = 1;
                    let b_idx = pB8x8Idx[i * 4] as usize;

                    let ref0_idx0 = iRefIdx0.get(iMbXy as usize)[b_idx];
                    let ref0_idx1 = iRefIdx0.get(iNeighMb as usize)[bn_idx];
                    let ref1_idx0 = iRefIdx1.get(iMbXy as usize)[b_idx];
                    let ref1_idx1 = iRefIdx1.get(iNeighMb as usize)[bn_idx];

                    let ref_p0 = if ref0_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref0_idx0 as usize]
                    } else {
                        None
                    };
                    let ref_q0 = if ref0_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref0_idx1 as usize]
                    } else {
                        None
                    };
                    let ref_p1 = if ref1_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_1][ref1_idx0 as usize]
                    } else {
                        None
                    };
                    let ref_q1 = if ref1_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_1][ref1_idx1 as usize]
                    } else {
                        None
                    };

                    if ((ref_p0 == ref_q0) && (ref_p1 == ref_q1))
                        || ((ref_p0 == ref_q1) && (ref_p1 == ref_q0))
                    {
                        let pMv0 = &(*pDec).pMv[LIST_0];
                        let pMv1 = &(*pDec).pMv[LIST_1];
                        pBS[j + (i << 1)] = ON_MB_BS(
                            ref_p0,
                            ref_q0,
                            ref_p1,
                            ref_q1,
                            pMv0,
                            pMv1,
                            iMbXy as usize,
                            iNeighMb as usize,
                            b_idx,
                            bn_idx,
                        );
                    }
                }
                bn_idx_pos += 1;
            }
        }
    } else if is_8x8_neigh {
        let mut b_idx_pos = 0usize;
        for i in 0..2 {
            let mut uiNzc = 0u8;
            for j in 0..4 {
                uiNzc |= (pNzcNeigh[pBn8x8Idx[i * 4 + j] as usize] as u8);
            }
            for j in 0..2 {
                let b_idx = pBIdx[b_idx_pos] as usize;
                if (uiNzc | (pNzcCurr[b_idx] as u8)) != 0 {
                    pBS[j + (i << 1)] = 2;
                } else {
                    pBS[j + (i << 1)] = 1;
                    let bn_idx = pBn8x8Idx[i * 4] as usize;

                    let ref0_idx0 = iRefIdx0.get(iMbXy as usize)[b_idx];
                    let ref0_idx1 = iRefIdx0.get(iNeighMb as usize)[bn_idx];
                    let ref1_idx0 = iRefIdx1.get(iMbXy as usize)[b_idx];
                    let ref1_idx1 = iRefIdx1.get(iNeighMb as usize)[bn_idx];

                    let ref_p0 = if ref0_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref0_idx0 as usize]
                    } else {
                        None
                    };
                    let ref_q0 = if ref0_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_0][ref0_idx1 as usize]
                    } else {
                        None
                    };
                    let ref_p1 = if ref1_idx0 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_1][ref1_idx0 as usize]
                    } else {
                        None
                    };
                    let ref_q1 = if ref1_idx1 > REF_NOT_IN_LIST {
                        (*pFilter).ref_ids[LIST_1][ref1_idx1 as usize]
                    } else {
                        None
                    };

                    if ((ref_p0 == ref_q0) && (ref_p1 == ref_q1))
                        || ((ref_p0 == ref_q1) && (ref_p1 == ref_q0))
                    {
                        let pMv0 = &(*pDec).pMv[LIST_0];
                        let pMv1 = &(*pDec).pMv[LIST_1];
                        pBS[j + (i << 1)] = ON_MB_BS(
                            ref_p0,
                            ref_q0,
                            ref_p1,
                            ref_q1,
                            pMv0,
                            pMv1,
                            iMbXy as usize,
                            iNeighMb as usize,
                            b_idx,
                            bn_idx,
                        );
                    }
                }
                b_idx_pos += 1;
            }
        }
    } else {
        // 4x4 transform
        for i in 0..4 {
            let b_idx = pBIdx[i] as usize;
            let bn_idx = pBnIdx[i] as usize;
            if ((pNzcCurr[b_idx] as u8) | (pNzcNeigh[bn_idx] as u8)) != 0 {
                pBS[i] = 2;
            } else {
                pBS[i] = 1;

                let ref0_idx0 = iRefIdx0.get(iMbXy as usize)[b_idx];
                let ref0_idx1 = iRefIdx0.get(iNeighMb as usize)[bn_idx];
                let ref1_idx0 = iRefIdx1.get(iMbXy as usize)[b_idx];
                let ref1_idx1 = iRefIdx1.get(iNeighMb as usize)[bn_idx];

                let ref_p0 = if ref0_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref0_idx0 as usize]
                } else {
                    None
                };
                let ref_q0 = if ref0_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_0][ref0_idx1 as usize]
                } else {
                    None
                };
                let ref_p1 = if ref1_idx0 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_1][ref1_idx0 as usize]
                } else {
                    None
                };
                let ref_q1 = if ref1_idx1 > REF_NOT_IN_LIST {
                    (*pFilter).ref_ids[LIST_1][ref1_idx1 as usize]
                } else {
                    None
                };

                if ((ref_p0 == ref_q0) && (ref_p1 == ref_q1))
                    || ((ref_p0 == ref_q1) && (ref_p1 == ref_q0))
                {
                    let pMv0 = &(*pDec).pMv[LIST_0];
                    let pMv1 = &(*pDec).pMv[LIST_1];
                    pBS[i] = ON_MB_BS(
                        ref_p0,
                        ref_q0,
                        ref_p1,
                        ref_q1,
                        pMv0,
                        pMv1,
                        iMbXy as usize,
                        iNeighMb as usize,
                        b_idx,
                        bn_idx,
                    );
                }
            }
        }
    }

    u32::from_ne_bytes(pBS)
}

// ============================================================================
// Macroblock Availability
// ============================================================================

#[inline]
pub unsafe fn DeblockingAvailableNoInterlayer(pCurDqLayer: &DqLayerState, iFilterIdc: i32) -> i32 {
    let iMbY = (*pCurDqLayer).iMbY;
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbXy = (*pCurDqLayer).iMbXyIndex;
    let bLeftFlag: bool;
    let bTopFlag: bool;

    if 2 == iFilterIdc {
        // T5.K3: shared indexing rather than a base pointer — this reads the
        // current macroblock beside its left and top neighbours, which is an
        // ordinary borrow of one owned array now.
        let pSliceIdc = &(*pCurDqLayer).grid.slice_idc;
        bLeftFlag = (iMbX > 0) && (*pSliceIdc.get(iMbXy as usize) == *pSliceIdc.get((iMbXy - 1) as usize));
        bTopFlag = (iMbY > 0)
            && (*pSliceIdc.get(iMbXy as usize)
                == *pSliceIdc.get((iMbXy - (*pCurDqLayer).iMbWidth) as usize));
    } else {
        bLeftFlag = iMbX > 0;
        bTopFlag = iMbY > 0;
    }
    ((bLeftFlag as i32) << LEFT_FLAG_BIT) | ((bTopFlag as i32) << TOP_FLAG_BIT)
}

// ============================================================================
// Edge Filtering Dispatch Subroutines
// ============================================================================

#[inline]
pub unsafe fn FilteringEdgeLumaH(
    pFilter: &mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut tc = [0i8; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).iLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIndexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 0);
        DeblockLumaLt4V_c(pPix, iStride, iAlpha, iBeta, tc.as_mut_ptr());
    }
}

#[inline]
pub unsafe fn FilteringEdgeLumaV(
    pFilter: &mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut tc = [0i8; 4];

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).iLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIndexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 0);
        DeblockLumaLt4H_c(pPix, iStride, iAlpha, iBeta, tc.as_mut_ptr());
    }
}

#[inline]
pub unsafe fn FilteringEdgeLumaIntraH(
    pFilter: &mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).iLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIndexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        DeblockLumaEq4V_c(pPix, iStride, iAlpha, iBeta);
    }
}

#[inline]
pub unsafe fn FilteringEdgeLumaIntraV(
    pFilter: &mut SDeblockingFilter,
    pPix: *mut u8,
    iStride: i32,
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).iLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIndexA,
        &mut iAlpha,
        &mut iBeta,
    );

    if (iAlpha | iBeta) != 0 {
        DeblockLumaEq4H_c(pPix, iStride, iAlpha, iBeta);
    }
}

pub unsafe fn FilteringEdgeChromaH(
    pFilter: &mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut tc = [0i8; 4];

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 1);
            DeblockChromaLt4V_c(pPixCb, pPixCr, iStride, iAlpha, iBeta, tc.as_mut_ptr());
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                let pPixCbCr = if i == 0 { pPixCb } else { pPixCr };
                    TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 1);
                DeblockChromaLt4V2_c(pPixCbCr, iStride, iAlpha, iBeta, tc.as_mut_ptr());
            }
        }
    }
}

pub unsafe fn FilteringEdgeChromaV(
    pFilter: &mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
    pBS: &[u8; 4],
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;
    let mut tc = [0i8; 4];

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 1);
            DeblockChromaLt4H_c(pPixCb, pPixCr, iStride, iAlpha, iBeta, tc.as_mut_ptr());
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                let pPixCbCr = if i == 0 { pPixCb } else { pPixCr };
                    TC0_TBL_LOOKUP(&mut tc, iIndexA, pBS, 1);
                DeblockChromaLt4H2_c(pPixCbCr, iStride, iAlpha, iBeta, tc.as_mut_ptr());
            }
        }
    }
}

pub unsafe fn FilteringEdgeChromaIntraH(
    pFilter: &mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            DeblockChromaEq4V_c(pPixCb, pPixCr, iStride, iAlpha, iBeta);
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                let pPixCbCr = if i == 0 { pPixCb } else { pPixCr };
                DeblockChromaEq4V2_c(pPixCbCr, iStride, iAlpha, iBeta);
            }
        }
    }
}

pub unsafe fn FilteringEdgeChromaIntraV(
    pFilter: &mut SDeblockingFilter,
    pPixCb: *mut u8,
    pPixCr: *mut u8,
    iStride: i32,
) {
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            DeblockChromaEq4H_c(pPixCb, pPixCr, iStride, iAlpha, iBeta);
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                let pPixCbCr = if i == 0 { pPixCb } else { pPixCr };
                DeblockChromaEq4H2_c(pPixCbCr, iStride, iAlpha, iBeta);
            }
        }
    }
}

// ============================================================================
// Inter Macroblock Deblocking Sequence
// ============================================================================

unsafe fn DeblockingInterMb(
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    pFilter: &mut SDeblockingFilter,
    nBS: &[[[u8; 4]; 4]; 2],
    iBoundryFlag: i32,
) {
    let iMbXyIndex = (*pCurDqLayer).iMbXyIndex;
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;

    let iCurLumaQp = *(*pCurDqLayer).grid.luma_qp.get(iMbXyIndex as usize) as i32;
    let pCurChromaQp = *(*pCurDqLayer).grid.chroma_qp.get(iMbXyIndex as usize);
    // T5.N3: the picture, not the filter's copy of three of its pointers. See the
    // note at `WelsDeblockingFilterSlice` for why the layer's `pDec` is the route.
    let iLineSize = (*pDec).linesize(0);
    let iLineSizeUV = (*pDec).linesize(1);

    let pDestY = (*pDec).data_ptr(0).add(((iMbY * iLineSize + iMbX) << 4) as usize);
    let pDestCb = (*pDec).data_ptr(1).add(((iMbY * iLineSizeUV + iMbX) << 3) as usize);
    let pDestCr = (*pDec).data_ptr(2).add(((iMbY * iLineSizeUV + iMbX) << 3) as usize);

    // Vertical margin
    if (iBoundryFlag & LEFT_FLAG_MASK) != 0 {
        let iLeftXyIndex = (iMbXyIndex - 1) as usize;
        (*pFilter).iLumaQP =
            ((iCurLumaQp + *(*pCurDqLayer).grid.luma_qp.get(iLeftXyIndex) as i32 + 1) >> 1) as i8;
        for i in 0..2 {
            (*pFilter).iChromaQP[i] = ((pCurChromaQp[i] as i32
                + (*pCurDqLayer).grid.chroma_qp.get(iLeftXyIndex)[i] as i32
                + 1)
                >> 1) as i8;
        }

        if nBS[0][0][0] == 0x04 {
            FilteringEdgeLumaIntraV(pFilter, pDestY, iLineSize);
            FilteringEdgeChromaIntraV(pFilter, pDestCb, pDestCr, iLineSizeUV);
        } else {
            let bs_word = u32::from_ne_bytes(nBS[0][0]);
            if bs_word != 0 {
                FilteringEdgeLumaV(pFilter, pDestY, iLineSize, &nBS[0][0]);
                FilteringEdgeChromaV(pFilter, pDestCb, pDestCr, iLineSizeUV, &nBS[0][0]);
            }
        }
    }

    (*pFilter).iLumaQP = iCurLumaQp as i8;
    (*pFilter).iChromaQP[0] = pCurChromaQp[0];
    (*pFilter).iChromaQP[1] = pCurChromaQp[1];

    let is_8x8 = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXyIndex as usize);

    let bs_01 = u32::from_ne_bytes(nBS[0][1]);
    if bs_01 != 0 && !is_8x8 {
        FilteringEdgeLumaV(pFilter, pDestY.add(1 << 2), iLineSize, &nBS[0][1]);
    }

    let bs_02 = u32::from_ne_bytes(nBS[0][2]);
    if bs_02 != 0 {
        FilteringEdgeLumaV(pFilter, pDestY.add(2 << 2), iLineSize, &nBS[0][2]);
        FilteringEdgeChromaV(
            pFilter,
            pDestCb.add(2 << 1),
            pDestCr.add(2 << 1),
            iLineSizeUV,
            &nBS[0][2],
        );
    }

    let bs_03 = u32::from_ne_bytes(nBS[0][3]);
    if bs_03 != 0 && !is_8x8 {
        FilteringEdgeLumaV(pFilter, pDestY.add(3 << 2), iLineSize, &nBS[0][3]);
    }

    // Horizontal margin
    if (iBoundryFlag & TOP_FLAG_MASK) != 0 {
        let iTopXyIndex = (iMbXyIndex - (*pCurDqLayer).iMbWidth) as usize;
        (*pFilter).iLumaQP =
            ((iCurLumaQp + *(*pCurDqLayer).grid.luma_qp.get(iTopXyIndex) as i32 + 1) >> 1) as i8;
        for i in 0..2 {
            (*pFilter).iChromaQP[i] = ((pCurChromaQp[i] as i32
                + (*pCurDqLayer).grid.chroma_qp.get(iTopXyIndex)[i] as i32
                + 1)
                >> 1) as i8;
        }

        if nBS[1][0][0] == 0x04 {
            FilteringEdgeLumaIntraH(pFilter, pDestY, iLineSize);
            FilteringEdgeChromaIntraH(pFilter, pDestCb, pDestCr, iLineSizeUV);
        } else {
            let bs_word = u32::from_ne_bytes(nBS[1][0]);
            if bs_word != 0 {
                FilteringEdgeLumaH(pFilter, pDestY, iLineSize, &nBS[1][0]);
                FilteringEdgeChromaH(pFilter, pDestCb, pDestCr, iLineSizeUV, &nBS[1][0]);
            }
        }
    }

    (*pFilter).iLumaQP = iCurLumaQp as i8;
    (*pFilter).iChromaQP[0] = pCurChromaQp[0];
    (*pFilter).iChromaQP[1] = pCurChromaQp[1];

    let bs_11 = u32::from_ne_bytes(nBS[1][1]);
    if bs_11 != 0 && !is_8x8 {
        FilteringEdgeLumaH(
            pFilter,
            pDestY.add(((1 << 2) * iLineSize) as usize),
            iLineSize,
            &nBS[1][1],
        );
    }

    let bs_12 = u32::from_ne_bytes(nBS[1][2]);
    if bs_12 != 0 {
        FilteringEdgeLumaH(
            pFilter,
            pDestY.add(((2 << 2) * iLineSize) as usize),
            iLineSize,
            &nBS[1][2],
        );
        FilteringEdgeChromaH(
            pFilter,
            pDestCb.add(((2 << 1) * iLineSizeUV) as usize),
            pDestCr.add(((2 << 1) * iLineSizeUV) as usize),
            iLineSizeUV,
            &nBS[1][2],
        );
    }

    let bs_13 = u32::from_ne_bytes(nBS[1][3]);
    if bs_13 != 0 && !is_8x8 {
        FilteringEdgeLumaH(
            pFilter,
            pDestY.add(((3 << 2) * iLineSize) as usize),
            iLineSize,
            &nBS[1][3],
        );
    }
}

// ============================================================================
// Intra Macroblock Deblocking Pipelines
// ============================================================================

pub unsafe fn FilteringEdgeLumaHV(
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    pFilter: &mut SDeblockingFilter,
    iBoundryFlag: i32,
) {
    let iMbXyIndex = (*pCurDqLayer).iMbXyIndex;
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;
    let iMbWidth = (*pCurDqLayer).iMbWidth;
    let iLineSize = (*pDec).linesize(0);

    let pDestY = (*pDec).data_ptr(0).add(((iMbY * iLineSize + iMbX) << 4) as usize);
    let iCurQp = *(*pCurDqLayer).grid.luma_qp.get(iMbXyIndex as usize) as i32;

    let mut iTc = [0i8; 4];
    let uiBSx4 = [3u8; 4];

    // Luma V
    if (iBoundryFlag & LEFT_FLAG_MASK) != 0 {
        (*pFilter).iLumaQP = ((iCurQp
            + *(*pCurDqLayer).grid.luma_qp.get((iMbXyIndex - 1) as usize) as i32
            + 1)
            >> 1) as i8;
        FilteringEdgeLumaIntraV(pFilter, pDestY, iLineSize);
    }

    (*pFilter).iLumaQP = iCurQp as i8;
    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    GET_ALPHA_BETA_FROM_QP(
        (*pFilter).iLumaQP as i32,
        (*pFilter).iSliceAlphaC0Offset as i32,
        (*pFilter).iSliceBetaOffset as i32,
        &mut iIndexA,
        &mut iAlpha,
        &mut iBeta,
    );

    let is_8x8 = *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXyIndex as usize);

    if (iAlpha | iBeta) != 0 {
        TC0_TBL_LOOKUP(&mut iTc, iIndexA, &uiBSx4, 0);

        if !is_8x8 {
            DeblockLumaLt4H_c(pDestY.add(1 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
        }

        DeblockLumaLt4H_c(pDestY.add(2 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());

        if !is_8x8 {
            DeblockLumaLt4H_c(pDestY.add(3 << 2), iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
        }
    }

    // Luma H
    if (iBoundryFlag & TOP_FLAG_MASK) != 0 {
        (*pFilter).iLumaQP = ((iCurQp
            + *(*pCurDqLayer).grid.luma_qp.get((iMbXyIndex - iMbWidth) as usize) as i32
            + 1)
            >> 1) as i8;
        FilteringEdgeLumaIntraH(pFilter, pDestY, iLineSize);
    }

    (*pFilter).iLumaQP = iCurQp as i8;
    if (iAlpha | iBeta) != 0 {
        if !is_8x8 {
            DeblockLumaLt4V_c(
                pDestY.add(((1 << 2) * iLineSize) as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }

        DeblockLumaLt4V_c(
            pDestY.add(((2 << 2) * iLineSize) as usize),
            iLineSize,
            iAlpha,
            iBeta,
            iTc.as_mut_ptr(),
        );

        if !is_8x8 {
            DeblockLumaLt4V_c(
                pDestY.add(((3 << 2) * iLineSize) as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    }
}

pub unsafe fn FilteringEdgeChromaHV(
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    pFilter: &mut SDeblockingFilter,
    iBoundryFlag: i32,
) {
    let iMbXyIndex = (*pCurDqLayer).iMbXyIndex;
    let iMbX = (*pCurDqLayer).iMbX;
    let iMbY = (*pCurDqLayer).iMbY;
    let iMbWidth = (*pCurDqLayer).iMbWidth;
    let iLineSize = (*pDec).linesize(1);

    let pDestCb = (*pDec).data_ptr(1).add(((iMbY * iLineSize + iMbX) << 3) as usize);
    let pDestCr = (*pDec).data_ptr(2).add(((iMbY * iLineSize + iMbX) << 3) as usize);
    let pCurQp = *(*pCurDqLayer).grid.chroma_qp.get(iMbXyIndex as usize);

    let mut iTc = [0i8; 4];
    let uiBSx4 = [3u8; 4];

    // Chroma V
    if (iBoundryFlag & LEFT_FLAG_MASK) != 0 {
        for i in 0..2 {
            (*pFilter).iChromaQP[i] = ((pCurQp[i] as i32
                + (*pCurDqLayer).grid.chroma_qp.get((iMbXyIndex - 1) as usize)[i] as i32
                + 1)
                >> 1) as i8;
        }
        FilteringEdgeChromaIntraV(pFilter, pDestCb, pDestCr, iLineSize);
    }

    (*pFilter).iChromaQP[0] = pCurQp[0];
    (*pFilter).iChromaQP[1] = pCurQp[1];

    let mut iIndexA = 0i32;
    let mut iAlpha = 0i32;
    let mut iBeta = 0i32;

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            TC0_TBL_LOOKUP(&mut iTc, iIndexA, &uiBSx4, 1);
            DeblockChromaLt4H_c(
                pDestCb.add(2 << 1),
                pDestCr.add(2 << 1),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                let pDestCbCr = if i == 0 {
                    pDestCb.add(2 << 1)
                } else {
                    pDestCr.add(2 << 1)
                };
                TC0_TBL_LOOKUP(&mut iTc, iIndexA, &uiBSx4, 1);
                DeblockChromaLt4H2_c(pDestCbCr, iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
            }
        }
    }

    // Chroma H
    if (iBoundryFlag & TOP_FLAG_MASK) != 0 {
        for i in 0..2 {
            (*pFilter).iChromaQP[i] = ((pCurQp[i] as i32
                + (*pCurDqLayer).grid.chroma_qp.get((iMbXyIndex - iMbWidth) as usize)[i] as i32
                + 1)
                >> 1) as i8;
        }
        FilteringEdgeChromaIntraH(pFilter, pDestCb, pDestCr, iLineSize);
    }

    (*pFilter).iChromaQP[0] = pCurQp[0];
    (*pFilter).iChromaQP[1] = pCurQp[1];

    if (*pFilter).iChromaQP[0] == (*pFilter).iChromaQP[1] {
        GET_ALPHA_BETA_FROM_QP(
            (*pFilter).iChromaQP[0] as i32,
            (*pFilter).iSliceAlphaC0Offset as i32,
            (*pFilter).iSliceBetaOffset as i32,
            &mut iIndexA,
            &mut iAlpha,
            &mut iBeta,
        );
        if (iAlpha | iBeta) != 0 {
            TC0_TBL_LOOKUP(&mut iTc, iIndexA, &uiBSx4, 1);
            DeblockChromaLt4V_c(
                pDestCb.add(((2 << 1) * iLineSize) as usize),
                pDestCr.add(((2 << 1) * iLineSize) as usize),
                iLineSize,
                iAlpha,
                iBeta,
                iTc.as_mut_ptr(),
            );
        }
    } else {
        for i in 0..2 {
            GET_ALPHA_BETA_FROM_QP(
                (*pFilter).iChromaQP[i] as i32,
                (*pFilter).iSliceAlphaC0Offset as i32,
                (*pFilter).iSliceBetaOffset as i32,
                &mut iIndexA,
                &mut iAlpha,
                &mut iBeta,
            );
            if (iAlpha | iBeta) != 0 {
                TC0_TBL_LOOKUP(&mut iTc, iIndexA, &uiBSx4, 1);
                let pDestCbCr = if i == 0 {
                    pDestCb.add(((2 << 1) * iLineSize) as usize)
                } else {
                    pDestCr.add(((2 << 1) * iLineSize) as usize)
                };
                DeblockChromaLt4V2_c(pDestCbCr, iLineSize, iAlpha, iBeta, iTc.as_mut_ptr());
            }
        }
    }
}

#[inline]
unsafe fn DeblockingIntraMb(
    pCurDqLayer: &DqLayerState,
    pDec: PPicture,
    pFilter: &mut SDeblockingFilter,
    iBoundryFlag: i32,
) {
    FilteringEdgeLumaHV(pCurDqLayer, pDec, pFilter, iBoundryFlag);
    FilteringEdgeChromaHV(pCurDqLayer, pDec, pFilter, iBoundryFlag);
}

// ============================================================================
// Macroblock-Level Top-Level Deblocking Dispatcher
// ============================================================================

pub unsafe extern "C" fn WelsDeblockingMb(
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    pFilter: *mut SDeblockingFilter,
    iBoundryFlag: i32,
) {
    let mut nBS = [[[0u8; 4]; 4]; 2];

    let iMbXyIndex = (*pCurDqLayer).iMbXyIndex;
    let iCurMbType = if !pDec.is_null() {
        *(*pDec).pMbType.get(iMbXyIndex as usize)
    } else {
        *(*pCurDqLayer).grid.mb_type.get(iMbXyIndex as usize)
    };

    let pSliceHeader = &(*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let bBSlice = pSliceHeader.eSliceType == EWelsSliceType::B_SLICE;

    match iCurMbType {
        MB_TYPE_INTRA4x4 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA_PCM => {
            DeblockingIntraMb(pCurDqLayer, pDec, &mut *pFilter, iBoundryFlag);
        }
        _ => {
            if (iBoundryFlag & LEFT_FLAG_MASK) != 0 {
                let iMbNb = iMbXyIndex - 1;
                let uiMbType = if !pDec.is_null() {
                    *(*pDec).pMbType.get(iMbNb as usize)
                } else {
                    *(*pCurDqLayer).grid.mb_type.get(iMbNb as usize)
                };

                let val = if IS_INTRA(uiMbType) {
                    0x04040404u32
                } else if bBSlice {
                    DeblockingBSliceBsMarginalMBAvcbase(&mut *pFilter, pCurDqLayer, pDec, 0, iMbNb, iMbXyIndex)
                } else {
                    DeblockingBsMarginalMBAvcbase(&mut *pFilter, pCurDqLayer, pDec, 0, iMbNb, iMbXyIndex)
                };
                nBS[0][0] = val.to_ne_bytes();
            } else {
                nBS[0][0] = [0u8; 4];
            }

            if (iBoundryFlag & TOP_FLAG_MASK) != 0 {
                let iMbNb = iMbXyIndex - (*pCurDqLayer).iMbWidth;
                let uiMbType = if !pDec.is_null() {
                    *(*pDec).pMbType.get(iMbNb as usize)
                } else {
                    *(*pCurDqLayer).grid.mb_type.get(iMbNb as usize)
                };

                let val = if IS_INTRA(uiMbType) {
                    0x04040404u32
                } else if bBSlice {
                    DeblockingBSliceBsMarginalMBAvcbase(&mut *pFilter, pCurDqLayer, pDec, 1, iMbNb, iMbXyIndex)
                } else {
                    DeblockingBsMarginalMBAvcbase(&mut *pFilter, pCurDqLayer, pDec, 1, iMbNb, iMbXyIndex)
                };
                nBS[1][0] = val.to_ne_bytes();
            } else {
                nBS[1][0] = [0u8; 4];
            }

            if IS_SKIP(iCurMbType) {
                nBS[0][1] = [0u8; 4];
                nBS[0][2] = [0u8; 4];
                nBS[0][3] = [0u8; 4];
                nBS[1][1] = [0u8; 4];
                nBS[1][2] = [0u8; 4];
                nBS[1][3] = [0u8; 4];
            } else if IS_INTER_16x16(iCurMbType) {
                let is_8x8 = *(*pCurDqLayer).grid.transform_size8x8_flag.get((*pCurDqLayer).iMbXyIndex as usize);
                if !is_8x8 {
                    DeblockingBSInsideMBAvsbase(GetPNzc(pCurDqLayer, iMbXyIndex), &mut nBS, 1);
                } else {
                    DeblockingBSInsideMBAvsbase8x8(GetPNzc(pCurDqLayer, iMbXyIndex), &mut nBS, 1);
                }
            } else if bBSlice {
                DeblockingBSliceBSInsideMBNormal(
                    &mut *pFilter,
                    pCurDqLayer,
                    pDec,
                    &mut nBS,
                    GetPNzc(pCurDqLayer, iMbXyIndex),
                    iMbXyIndex,
                );
            } else {
                DeblockingBSInsideMBNormal(
                    &mut *pFilter,
                    pCurDqLayer,
                    pDec,
                    &mut nBS,
                    GetPNzc(pCurDqLayer, iMbXyIndex),
                    iMbXyIndex,
                );
            }

            DeblockingInterMb(pCurDqLayer, pDec, &mut *pFilter, &nBS, iBoundryFlag);
        }
    }
}

// ============================================================================
// Slice-Level In-Loop Deblocking Filter Pipelines
// ============================================================================
//
// S25 for this file (T5.C2, enumerated with the conversion as plan §7.6 asks;
// re-enumerated at T5.N3, where the shape it described stopped existing):
// *who else reaches this `SPicture` while a borrow of it is held?*
//
// The borrow used to be a `data_ptr(i)` off the context's picture, taken three times at each of
// the two filter-initialisation sites and stored into `SDeblockingFilter.pCsData` for
// the whole macroblock loop. **There is no stored derivation now**: each reader takes
// the picture as a parameter, derives inside its own body, and the result dies with
// the macroblock. Three answers, and none of them is a hazard:
//
// 1. **The derivations do not invalidate each other.** They address three planes,
//    which after T5.C3 are three separate allocations; the accessor's `&mut self`
//    covers the picture's own fields, not the sample bytes.
// 2. **Nothing else in the loop reaches `pDec`, and after T5.N4 nothing in the loop
//    reaches another picture at all.** The reference lists are `PicId`s snapshotted
//    at filter init, so the loop's use of a reference is a slot comparison and never
//    a dereference; `pMv` and `pRefIndex` are read off the picture the loop passes
//    down, and `DeblockingBSCalc*` reads the motion caches. The question of what
//    happens if a reference list slot holds `pDec` itself does not arise, because
//    holding a slot number is not holding a picture.
// 4. **T5.P′1: the route is `dec_pic(pCtx)`, derived per macroblock in the loop
//    below and passed as an argument.** It was `pCurDqLayer->pDec`, a cache of the
//    same value with one stamp site; W2b deleted the cache rather than converting
//    it. Nothing stores the result — not the filter, not the layer — so the class
//    §2 names has no instance left in this file.
// 3. **The mirror is gone, and it was the decoder's last** (§2's named class, of
//    which `pBitStringAux` was the previous one, T5.M3). A cached plane pointer
//    beside the plane that owns it is the F16/T5 class — two fields that can
//    disagree about one buffer — and `SDeblockingFilter` carried five of them.
//    What replaces them is nothing: the plane is asked each time.

/// The two reference lists as [`PicId`]s — `SDeblockingFilter::ref_ids`'s one writer.
///
/// **T5.P′2 emptied this function of work.** It existed because `sRefPic`'s lists
/// were raw picture pointers and boundary strength needed identities, so it walked 34
/// pointers and stamped each one's slot, asserting on the way that a reference list
/// only ever holds pool pictures. The lists *are* those identities now, so the
/// snapshot is the copy the type does for free and the assert has moved to the door
/// they go in by (`manage_dec_ref::insert_ref`) — one site instead of every reader.
///
/// The snapshot itself is still a snapshot, and for T5.N4's reason: nothing writes a
/// reference list during deblocking, so the loop's `ref_ids` and the context's lists
/// cannot diverge.
#[inline]
fn snapshot_ref_ids(refs: &SRefPic) -> [[Option<PicId>; MAX_DPB_COUNT]; LIST_A] {
    refs.pRefList
}

pub unsafe fn WelsDeblockingFilterSlice(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    pDeblockMb: Option<PDeblockingFilterMbFunc>,
) {
    let pSliceHeaderExt = &(*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt;
    let iMbWidth = (*pCurDqLayer).iMbWidth;
    // A value, not a borrow: the count is constant across the loop below and the loop
    // writes the layer and the picture through the same context (T5.Z1).
    let iTotalMbCount = sps_of(&(*pCtx).sSpsPpsCtx, pSliceHeaderExt.sSliceHeader.sps_ref)
        .map_or(0, |sps| sps.uiTotalMbCount as i32);

    let mut pFilter = SDeblockingFilter::default();
    // T5.Z4: the id, not the entry — the loop below writes the layer and the
    // picture through the same context, and `active_fmo` re-resolves in one
    // expression at the one line that walks the map.
    let fmo_id = (*pCtx).fmo_id;
    let mut iNextMbXyIndex: i32;
    let iTotalNumMb = (*pCurDqLayer).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice;
    let mut iCountNumMb = 0i32;
    let mut iBoundryFlag: i32;
    let iFilterIdc = pSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc as i32;

    // Step 1: Initialize filter parameters.
    //
    // **T5.N3: the five mirrored fields are gone and nothing replaces them.** The
    // three plane pointers and two strides used to be copied out of `pCtx->pDec`
    // here and read for the whole macroblock loop; each reader derives what it needs
    // per use, so no cached copy can disagree with the plane that owns it.
    //
    // T5.M3's lesson, applied rather than restated: *check that the route you
    // replace the mirror with is as fresh as the mirror was.* The mirror's source
    // was `pCtx->pDec` and the route was `pCurDqLayer->pDec`, which is why a
    // `debug_assert!` sat here asserting the two were one picture. **T5.P′1 deleted
    // the assert by deleting the second route**: the layer's copy was a cache of
    // `dec_pic(pCtx)` and it is gone, so the loop below derives the source itself,
    // once per macroblock, and there is nothing left for the two to disagree about.
    pFilter.eSliceType = (*pCurDqLayer).sLayerInfo.sSliceInLayer.eSliceType as i32;

    pFilter.iSliceAlphaC0Offset = pSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset as i8;
    pFilter.iSliceBetaOffset = pSliceHeaderExt.sSliceHeader.iSliceBetaOffset as i8;

    // F38/S29: `addr_of_mut!`, not `&mut` — this pointer is stored into another
    // struct and read for the whole macroblock loop, which is S29's worst class.
    pFilter.ref_ids = snapshot_ref_ids(&pCtx.sRefPic);

    // T5.W7: `pps_id` is the slice header's and constant across the loop, so it is
    // read once here rather than through a shared borrow of the layer held across the
    // loop's mutable uses of it — the bracket maneuver at one scalar.
    let pps_id = pSliceHeaderExt.sSliceHeader.pps_id;

    // Step 2: Macroblock deblocking loop
    if iFilterIdc == 0 || iFilterIdc == 2 {
        iNextMbXyIndex = pSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
        (*pCurDqLayer).iMbX = iNextMbXyIndex % iMbWidth;
        (*pCurDqLayer).iMbY = iNextMbXyIndex / iMbWidth;
        (*pCurDqLayer).iMbXyIndex = iNextMbXyIndex;

        loop {
            iBoundryFlag = DeblockingAvailableNoInterlayer(pCurDqLayer, iFilterIdc);

            if let Some(func) = pDeblockMb {
                func(pCurDqLayer, pDec, &mut pFilter, iBoundryFlag);
            }

            iCountNumMb += 1;
            if iCountNumMb >= iTotalNumMb {
                break;
            }

            if pps_of(&(*pCtx).sSpsPpsCtx, pps_id).is_some_and(|pps| pps.uiNumSliceGroups > 1) {
                // Flexible Macroblock Ordering slice group transition
                iNextMbXyIndex =
                    crate::decoder::fmo::FmoNextMb(active_fmo(&pCtx.sFmoList, fmo_id), iNextMbXyIndex);
            } else {
                iNextMbXyIndex += 1;
            }

            if iNextMbXyIndex == -1 || iNextMbXyIndex >= iTotalMbCount {
                break;
            }

            (*pCurDqLayer).iMbX = iNextMbXyIndex % iMbWidth;
            (*pCurDqLayer).iMbY = iNextMbXyIndex / iMbWidth;
            (*pCurDqLayer).iMbXyIndex = iNextMbXyIndex;
        }
    }
}

pub unsafe fn WelsDeblockingInitFilter(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: &DqLayerState,
    pFilter: &mut SDeblockingFilter,
    iFilterIdc: *mut i32,
) {
    let pSliceHeaderExt = &(*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt;

    *pFilter = SDeblockingFilter::default();
    *iFilterIdc = pSliceHeaderExt.sSliceHeader.uiDisableDeblockingFilterIdc as i32;

    (*pFilter).eSliceType = (*pCurDqLayer).sLayerInfo.sSliceInLayer.eSliceType as i32;

    (*pFilter).iSliceAlphaC0Offset = pSliceHeaderExt.sSliceHeader.iSliceAlphaC0Offset as i8;
    (*pFilter).iSliceBetaOffset = pSliceHeaderExt.sSliceHeader.iSliceBetaOffset as i8;

    // F38/S29, as above.
    (*pFilter).ref_ids = snapshot_ref_ids(&pCtx.sRefPic);
}

pub unsafe fn WelsDeblockingFilterMB(
    pCurDqLayer: &mut DqLayerState,
    pDec: PPicture,
    pFilter: &mut SDeblockingFilter,
    iFilterIdc: i32,
    pDeblockMb: Option<PDeblockingFilterMbFunc>,
) {
    if iFilterIdc == 0 || iFilterIdc == 2 {
        let iBoundryFlag = DeblockingAvailableNoInterlayer(pCurDqLayer, iFilterIdc);
        if let Some(func) = pDeblockMb {
            func(pCurDqLayer, pDec, pFilter, iBoundryFlag);
        }
    }
}

// ============================================================================
// SIMD Function Pointer Dispatch Initialization
// ============================================================================

pub unsafe fn DeblockingInit(pFunc: *mut SDeblockingFunc, iCpu: i32) {
    (*pFunc).pfLumaDeblockingLT4Ver = Some(DeblockLumaLt4V_c);
    (*pFunc).pfLumaDeblockingEQ4Ver = Some(DeblockLumaEq4V_c);
    (*pFunc).pfLumaDeblockingLT4Hor = Some(DeblockLumaLt4H_c);
    (*pFunc).pfLumaDeblockingEQ4Hor = Some(DeblockLumaEq4H_c);

    (*pFunc).pfChromaDeblockingLT4Ver = Some(DeblockChromaLt4V_c);
    (*pFunc).pfChromaDeblockingEQ4Ver = Some(DeblockChromaEq4V_c);
    (*pFunc).pfChromaDeblockingLT4Hor = Some(DeblockChromaLt4H_c);
    (*pFunc).pfChromaDeblockingEQ4Hor = Some(DeblockChromaEq4H_c);

    (*pFunc).pfChromaDeblockingLT4Ver2 = Some(DeblockChromaLt4V2_c);
    (*pFunc).pfChromaDeblockingEQ4Ver2 = Some(DeblockChromaEq4V2_c);
    (*pFunc).pfChromaDeblockingLT4Hor2 = Some(DeblockChromaLt4H2_c);
    (*pFunc).pfChromaDeblockingEQ4Hor2 = Some(DeblockChromaEq4H2_c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe::mb_grid::MbDims;

    // -----------------------------------------------------------------------
    // P3 site 1 of 3 — boundary strength is decided by reference-picture
    // **identity**, never by picture order count.
    //
    // Plan §3 P3 converted `*mut SPicture` to `PicId` at **T5.N4**, and these three
    // tests are on the far side of it. They were written to pin the distinction
    // beforehand: the comparison had to mean "the same picture object", not "a
    // picture with the same POC", and the two differ exactly when the DPB holds two
    // distinct pictures with a duplicate POC, which a stream can produce (an IDR
    // resets the POC counter; MMCO 5 does too).
    //
    // **The POC half of that is now structural rather than tested.** These functions
    // no longer receive a picture, so there is no POC in reach to compare — a rewrite
    // that consulted one could not compile. What is still worth pinning, and is what
    // they pin now, is that the *reference* term is consulted at all and that the MV
    // term does not mask it: each holds the MVs equal and varies only the reference.
    //
    // The two slots come from a `Pool`, because that is the only place a `PicId`
    // comes from. `pic_queue.rs`'s `pooled_pictures_are_identified_by_slot_not_by_poc`
    // is the other end of the same property — that two real pooled pictures with one
    // POC get two slots.
    // -----------------------------------------------------------------------

    /// Two distinct slots, the shape every one of these tests needs.
    fn two_refs() -> (Option<PicId>, Option<PicId>) {
        let p = crate::safe::pool::Pool::new(vec![(), ()]);
        (Some(p.id(0)), Some(p.id(1)))
    }

    /// Two different references read as boundary strength 1 even though every
    /// motion vector agrees; one reference against itself falls through to the MVs.
    #[test]
    fn p3_mb_bs_mv_separates_distinct_references() {
        let (a, b) = two_refs();
        // T5.X5: the helper takes the array, not a base pointer into it, so the
        // fixture is a two-macroblock `MbArray` rather than a `[_; 2]` and a cast.
        let mvs = MbArray::from_vec(vec![[[0i16; MV_A]; MB_BLOCK4x4_NUM]; 2], MbDims::new(2, 1));

        unsafe {
            let mv = &mvs;
            assert_eq!(
                MB_BS_MV(a, b, mv, 0, 1, 0, 0),
                1,
                "two slots must read as different references"
            );
            assert_eq!(
                MB_BS_MV(a, a, mv, 0, 1, 0, 0),
                0,
                "one reference against itself falls through to the MV comparison"
            );
            // A null slot is a reference too, and it is not any picture.
            assert_eq!(MB_BS_MV(None, None, mv, 0, 1, 0, 0), 0);
            assert_eq!(MB_BS_MV(a, None, mv, 0, 1, 0, 0), 1);
        }
    }

    /// The same property one level down, in the 8x8 edge path — which until T5.N4
    /// erased its references to `*mut c_void` to carry them and now carries ids.
    #[test]
    fn p3_smb_edge_mv_separates_distinct_references() {
        let (a, b) = two_refs();
        let mvs = [[0i16; MV_A]; MB_BLOCK4x4_NUM];

        unsafe {
            let mut refs: [Option<PicId>; MB_BLOCK4x4_NUM] = [a; MB_BLOCK4x4_NUM];
            assert_eq!(SMB_EDGE_MV(&refs, &mvs, 0, 1), 0, "one slot, equal MVs");

            refs[1] = b;
            assert_eq!(
                SMB_EDGE_MV(&refs, &mvs, 0, 1),
                1,
                "a second slot is a second reference"
            );
        }
    }

    /// B-slice edge: `ON_MB_BS` picks between the "lists agree" and "lists crossed"
    /// arms by comparing `ref_p0` with `ref_p1` and then `ref_p0` with `ref_q0`.
    /// With all four MV sets equal, the arm chosen is visible in the result, so this
    /// pins that the choice is made on identity.
    #[test]
    fn p3_on_mb_bs_arm_selection_is_by_identity() {
        let (a, b) = two_refs();
        // Chosen so the two arms disagree: straight comparisons (l0 vs l0, l1 vs l1)
        // exceed the 4-quarter-pel threshold, crossed ones (l0 vs l1) do not. That
        // makes the `ref_p0 == ref_p1` arm — an AND of both — false while the
        // `ref_p0 != ref_p1, ref_p0 == ref_q0` arm is true.
        let mut mv_l0 =
            MbArray::from_vec(vec![[[0i16; MV_A]; MB_BLOCK4x4_NUM]; 2], MbDims::new(2, 1));
        let mut mv_l1 =
            MbArray::from_vec(vec![[[0i16; MV_A]; MB_BLOCK4x4_NUM]; 2], MbDims::new(2, 1));
        mv_l0.get_mut(0)[0][0] = 64; // current MB, list 0
        mv_l1.get_mut(1)[0][0] = 64; // neighbour MB, list 1

        unsafe {
            let m0 = &mv_l0;
            let m1 = &mv_l1;

            // All four references are one slot.
            let same = ON_MB_BS(a, a, a, a, m0, m1, 0, 1, 0, 0);

            // p0/q0 are that slot; p1/q1 are a *different* slot.
            let distinct = ON_MB_BS(a, a, b, b, m0, m1, 0, 1, 0, 0);

            assert_eq!(same, 0, "one slot everywhere takes the lists-agree arm");
            assert_eq!(distinct, 1, "a second slot is a different reference");
        }
    }

    #[test]
    fn test_deblocking_init() {
        unsafe {
            let mut func = SDeblockingFunc::default();
            DeblockingInit(&mut func, 0);
            assert!(func.pfLumaDeblockingLT4Ver.is_some());
            assert!(func.pfLumaDeblockingEQ4Ver.is_some());
            assert!(func.pfLumaDeblockingLT4Hor.is_some());
            assert!(func.pfLumaDeblockingEQ4Hor.is_some());
            assert!(func.pfChromaDeblockingLT4Ver.is_some());
            assert!(func.pfChromaDeblockingEQ4Ver.is_some());
            assert!(func.pfChromaDeblockingLT4Hor.is_some());
            assert!(func.pfChromaDeblockingEQ4Hor.is_some());
            assert!(func.pfChromaDeblockingLT4Ver2.is_some());
            assert!(func.pfChromaDeblockingEQ4Ver2.is_some());
            assert!(func.pfChromaDeblockingLT4Hor2.is_some());
            assert!(func.pfChromaDeblockingEQ4Hor2.is_some());
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_MMI, WELS_CPU_MSA, WELS_CPU_NEON, WELS_CPU_SSSE3};
pub use crate::decoder::decode_slice::{g_kuiMbCountScan4Idx};
