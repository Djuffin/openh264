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

//! # Motion Vector Prediction and Motion Info Caching (`mv_pred.h` / `mv_pred.cpp`)
//!
//! Translated from `codec/decoder/core/inc/mv_pred.h` and `codec/decoder/core/src/mv_pred.cpp`.
//!
//! Implements motion vector prediction (MVP), directional match selection, component-wise
//! median calculation, P-skip and B-direct mode derivations (Spatial Direct and Temporal Direct),
//! collocated macroblock synchronization, POC reference remapping, and macroblock motion
//! vector and reference index cache propagation.

#![deny(unsafe_code)]
// **Phase 5, T5.AB1 — `PPicture`'s first module, and the survivor named at its
// items (face 0).** The module carried the lint with **three exceptions, allowed
// by name**, and every one of them held `PicRefs` beside the picture being decoded.
// **T5b.1 closed all three** — the arm stopped being an address and became an
// identity (`PicRefs::resolve`) — so this module allows nothing today, and T5b.9
// deleted the last dead `PPicture` re-export it carried.
//
// The twelve signatures that carry the picture *without* a reference view take a
// borrow — `Option<&SPicture>` where they read, `Option<&mut SPicture>` where they
// write, `&mut SPicture` for the one whose C++ dereferences unconditionally — and
// the writes into them became indexing, because `MbArray::get_mut` already hands
// back the macroblock's own `[T; 16]` and the raw base was that array all along.
//
// The three that keep `PPicture` are the phase's **second enumerated survivor**
// (steward, at `6b6dd9a3`; `phase5_session_ab.md` §0), and the reason is written at
// each item.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_mut,
    unused_assignments
)]

use std::ffi::c_void;
use crate::safe::mb_grid::MbArray;

// ============================================================================
// Constants & Error Codes
// ============================================================================

pub const REF_NOT_AVAIL: i8 = -2;
pub const REF_NOT_IN_LIST: i8 = -1;

pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;
pub const MV_A: usize = 2;

pub const MB_BLOCK4x4_NUM: usize = 16;

pub const ERR_NONE: i32 = 0;
pub const ERR_LEVEL_SLICE_DATA: i32 = 6;
pub const ERR_LEVEL_MB_DATA: i32 = 7;
pub const ERR_INFO_SYNTAX_BASE: i32 = 1001;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = ERR_INFO_SYNTAX_BASE + 74; // 1075 or 175 depending on enum
pub const ERR_INFO_INVALID_REF_INDEX: i32 = ERR_INFO_SYNTAX_BASE + 39;

pub const dsRefLost: i32 = 0x02;

#[inline(always)]
pub fn GENERATE_ERROR_NO(iErrLevel: i32, iErrInfo: i32) -> i32 {
    (iErrLevel << 16) | (iErrInfo & 0xFFFF)
}

// Macroblock type bitmasks
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

pub const MB_TYPE_INTRA: u32 =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;
pub const MB_TYPE_INTER: u32 = MB_TYPE_16x16
    | MB_TYPE_16x8
    | MB_TYPE_8x16
    | MB_TYPE_8x8
    | MB_TYPE_8x8_REF0
    | MB_TYPE_SKIP
    | MB_TYPE_DIRECT;

pub type MbType = u32;
pub type SubMbType = u32;

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
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_DIRECT(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_DIRECT) != 0
}

#[inline(always)]
pub fn IS_Inter_8x8(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_8x8) != 0
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
pub fn IS_SUB_8x8(sub_mb_type: u32) -> bool {
    (sub_mb_type & SUB_MB_TYPE_8x8) != 0
}

#[inline(always)]
pub fn IS_SUB_4x4(sub_mb_type: u32) -> bool {
    (sub_mb_type & SUB_MB_TYPE_4x4) != 0
}

// ============================================================================
// Lookup Tables
// ============================================================================

// ============================================================================
// Data Structures matching C++ Dec Core
// ============================================================================

pub use crate::decoder::picture::SPicture;

pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt};


pub use crate::decoder::decoder_core::{SSlice, SLayerInfo, DqLayerState};


pub use crate::decoder::decoder_context::{SRefPic};
use crate::decoder::decoder_context::{SliceCtx, active_pps, active_sps, pps_of, sps_of};
// The real decoder context and SPS, not local stand-ins: these are reached through
// raw pointers from decode_slice, so the layouts must be the genuine ones.
pub use crate::decoder::decoder_context::{
    SWelsDecoderContext, PicRefs, ref_id,
};
pub use crate::decoder::parameter_sets::SSps;
pub use crate::decoder::decode_slice::{SPartMbInfo, g_ksInterBSubMbTypeInfo};
pub use crate::decoder::decode_slice::{g_kuiCache30ScanIdx, g_kuiScan4};


// ============================================================================
// Block Fill/Copy Primitives, in the grid's own units
// ============================================================================

// **T5.R5 deleted this file's `LD32`/`ST32`/`LD16`/`ST16`/`LD64`/`ST64` and the two
// byte-pointer block helpers `SetRectBlock`/`CopyRectBlock4Cols`.** They were the C's
// packed-word idiom transliterated — read/write two `int16_t` as one `int32_t`, fill a
// rectangle through a byte pointer with a width dispatch — and every one of their 207
// uses in this file has become an assignment of the value the C was moving: an
// `[i16; 2]` motion vector, an `i8` reference index, a `[[i16; 2]; 16]` block. F35's
// alignment precondition is gone with them rather than satisfied, and S6's
// never-widen rule now holds by construction: nothing here is wider than what it
// moves. What survives is the *arithmetic* two of those uses depended on, spelled
// where it can be read:

/// Fills the 2x2 4x4-block square whose top-left is `origin` with one reference index,
/// **including the C's sign-extension quirk** (T5.R5).
///
/// `SetRectBlock`'s 1-byte path broadcasts through `val * 0x0101` in `uint32_t` and
/// truncates to 16 bits, and every caller here passes a sign-extended `int8_t`: for
/// `val = -1` the product is `0xFFFFFEFF`, so the pair written is `{-1, -2}`, not
/// `{-1, -1}`. The arithmetic is kept exactly (S6) — repairing it here would disagree
/// with the reference decoder — and it is spelled once, where it can be read, instead
/// of hiding inside a byte-pointer helper's width dispatch. The two
/// `(uint8_t)REF_NOT_IN_LIST` sites are *not* this: C casts to unsigned itself there,
/// so those are plain fills.
#[inline(always)]
pub fn set_rect_ref(block: &mut [i8; 16], origin: usize, val: i8) {
    let broadcast = (val as i32 as u32).wrapping_mul(0x0101);
    let pair = [broadcast as u8 as i8, (broadcast >> 8) as u8 as i8];
    block[origin] = pair[0];
    block[origin + 1] = pair[1];
    block[origin + 4] = pair[0];
    block[origin + 5] = pair[1];
}

/// Fills the 2x2 4x4-block square whose top-left is `origin` with one motion vector.
///
/// **T5.R5: `SetRectBlock`'s only surviving decoder shape, typed.** The C macro took a
/// byte pointer, a byte stride and an element size, and every caller here passed
/// `(2, 2, 16, LD32(mv), 4)` — a 2x2 block of 4-byte elements over a 16-byte row —
/// which in the grid's own units is this. The MV is copied as an `[i16; 2]`, so no
/// operation is wider than the value it moves (S6) and F35's alignment precondition
/// is gone rather than satisfied by accident.
#[inline(always)]
pub fn set_rect_mv(block: &mut [[i16; 2]; 16], origin: usize, val: [i16; 2]) {
    block[origin] = val;
    block[origin + 1] = val;
    block[origin + 4] = val;
    block[origin + 5] = val;
}

// ============================================================================
// Mathematical Helper Functions
// ============================================================================

/// Calculates component-wise median of three signed 16-bit integers.
#[inline(always)]
pub fn WelsMedian(a: i16, b: i16, c: i16) -> i16 {
    let a32 = a as i32;
    let b32 = b as i32;
    let c32 = c as i32;
    let min_ab = std::cmp::min(a32, b32);
    let min_abc = std::cmp::min(min_ab, c32);
    let max_ab = std::cmp::max(a32, b32);
    let max_abc = std::cmp::max(max_ab, c32);
    (a32 + b32 + c32 - min_abc - max_abc) as i16
}

/// Returns the minimum positive reference index (>= 0), or the other value if negative.
#[inline(always)]
pub fn WELS_MIN_POSITIVE(a: i8, b: i8) -> i8 {
    if a < 0 {
        b
    } else if b < 0 {
        a
    } else {
        std::cmp::min(a, b)
    }
}

// **T5.X4: `SetRectBlock` and `CopyRectBlock4Cols` are deleted, not converted.**
// T5.R5's note above says every one of their 207 uses became an assignment of the
// value the C was moving — and it was right: the two definitions had **zero
// callers anywhere in the crate** from that commit onwards. They stood here as
// 82 raw-pointer casts (65 `*mut u32` among them) and a width dispatch that
// nothing dispatched to. S18's straggler class, deleted where it was found. F35's
// record is `phase5_findings.md`'s; the alignment precondition it named went with
// the last use, not with this deletion.

// ============================================================================
// Macroblock Type Accessor
// ============================================================================

/// The macroblock-type array this layer reads, **whole** — the picture's when there
/// is a picture, the layer's grid otherwise.
///
/// T5.X4: this handed back `mb_grid_ptr(.., 0)`, the array's base as a raw pointer,
/// because every caller indexes it at *neighbour* addresses (left, top, top-left,
/// top-right) and a narrowing slice would be UB at the first one (S28). Handing back
/// the array itself says the same thing with no pointer written and no reach to get
/// wrong: `MbArray::get` bounds-checks against the allocation, which is exactly the
/// reach the raw base had. Shared, because all seven read sites read — the two that
/// write go through [`SetMbType`] below, one expression each, as they already did.
///
/// T5.AB1: the picture arrives as `Option<&SPicture>`, and the null test the raw
/// parameter carried is the `Option`. Two *shared* borrows of one picture are legal
/// however they alias, which is what makes this side of the family convertible
/// while the three `PicRefs` items below are not (§0's option 3).
#[inline(always)]
pub fn GetMbType<'a>(pCurDqLayer: &'a DqLayerState, pDec: Option<&'a SPicture>) -> &'a MbArray<u32> {
    match pDec {
        Some(pic) => &pic.pMbType,
        None => &pCurDqLayer.grid.mb_type,
    }
}

/// [`GetMbType`]'s write half — the same dual path, one macroblock.
#[inline(always)]
pub fn SetMbType(pCurDqLayer: &mut DqLayerState, pDec: Option<&mut SPicture>, mb_xy: usize, val: u32) {
    match pDec {
        Some(pic) => *pic.pMbType.get_mut(mb_xy) = val,
        None => *pCurDqLayer.grid.mb_type.get_mut(mb_xy) = val,
    }
}

// ============================================================================
// Motion Vector Predictor Implementations
// ============================================================================

/// Calculates the predicted motion vector for a P_SKIP macroblock from its spatial neighbors.
pub fn PredPSkipMvFromNeighbor(
    pCurDqLayer: &mut DqLayerState,
    pDec: Option<&SPicture>,
    iMvp: &mut [i16; 2],
) {
    let mut bTopAvail = false;
    let mut bLeftTopAvail = false;
    let mut bRightTopAvail = false;
    let mut bLeftAvail = false;

    let iCurXy = (*pCurDqLayer).iMbXyIndex;
    let iCurX = (*pCurDqLayer).iMbX;
    let iCurY = (*pCurDqLayer).iMbY;
    let iCurSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iCurXy as usize);

    let mut iLeftXy = 0;
    let mut iTopXy = 0;
    let mut iLeftTopXy = 0;
    let mut iRightTopXy = 0;

    if iCurX != 0 {
        iLeftXy = iCurXy - 1;
        let iLeftSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iLeftXy as usize);
        bLeftAvail = iLeftSliceIdc == iCurSliceIdc;
    } else {
        bLeftAvail = false;
        bLeftTopAvail = false;
    }

    if iCurY != 0 {
        iTopXy = iCurXy - (*pCurDqLayer).iMbWidth;
        let iTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iTopXy as usize);
        bTopAvail = iTopSliceIdc == iCurSliceIdc;
        if iCurX != 0 {
            iLeftTopXy = iTopXy - 1;
            let iLeftTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iLeftTopXy as usize);
            bLeftTopAvail = iLeftTopSliceIdc == iCurSliceIdc;
        } else {
            bLeftTopAvail = false;
        }
        if iCurX != ((*pCurDqLayer).iMbWidth - 1) {
            iRightTopXy = iTopXy + 1;
            let iRightTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iRightTopXy as usize);
            bRightTopAvail = iRightTopSliceIdc == iCurSliceIdc;
        } else {
            bRightTopAvail = false;
        }
    } else {
        bTopAvail = false;
        bLeftTopAvail = false;
        bRightTopAvail = false;
    }

    let pMbType = GetMbType(pCurDqLayer, pDec);
    let iLeftType = if iCurX != 0 && bLeftAvail { *pMbType.get(iLeftXy as usize) } else { 0 };
    let iTopType = if iCurY != 0 && bTopAvail { *pMbType.get(iTopXy as usize) } else { 0 };
    let iLeftTopType = if iCurX != 0 && iCurY != 0 && bLeftTopAvail { *pMbType.get(iLeftTopXy as usize) } else { 0 };
    let iRightTopType = if iCurX != ((*pCurDqLayer).iMbWidth - 1) && iCurY != 0 && bRightTopAvail { *pMbType.get(iRightTopXy as usize) } else { 0 };

    let mut iMvA = [0i16; 2];
    let mut iMvB = [0i16; 2];
    let mut iMvC = [0i16; 2];
    let mut iMvD = [0i16; 2];
    let mut iLeftRef: i8;
    let mut iTopRef: i8;
    let mut iRightTopRef: i8;
    let mut iLeftTopRef: i8;


    // left
    if bLeftAvail && IS_INTER(iLeftType) {
        if let Some(pic) = pDec {
            iMvA = pic.pMv[0].get(iLeftXy as usize)[3];
            iLeftRef = pic.pRefIndex[0].get(iLeftXy as usize)[3];
        } else {
            iMvA = (*pCurDqLayer).grid.mv[0].get(iLeftXy as usize)[3];
            iLeftRef = (*pCurDqLayer).grid.ref_index[0].get(iLeftXy as usize)[3];
        }
    } else {
        iMvA = [0, 0];
        iLeftRef = if !bLeftAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }
    if iLeftRef == REF_NOT_AVAIL || (iLeftRef == 0 && (iMvA[0] == 0 && iMvA[1] == 0)) {
        *iMvp = [0, 0];
        return;
    }

    // top
    if bTopAvail && IS_INTER(iTopType) {
        if let Some(pic) = pDec {
            iMvB = pic.pMv[0].get(iTopXy as usize)[12];
            iTopRef = pic.pRefIndex[0].get(iTopXy as usize)[12];
        } else {
            iMvB = (*pCurDqLayer).grid.mv[0].get(iTopXy as usize)[12];
            iTopRef = (*pCurDqLayer).grid.ref_index[0].get(iTopXy as usize)[12];
        }
    } else {
        iMvB = [0, 0];
        iTopRef = if !bTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }
    if iTopRef == REF_NOT_AVAIL || (iTopRef == 0 && (iMvB[0] == 0 && iMvB[1] == 0)) {
        *iMvp = [0, 0];
        return;
    }

    // right_top
    if bRightTopAvail && IS_INTER(iRightTopType) {
        if let Some(pic) = pDec {
            iMvC = pic.pMv[0].get(iRightTopXy as usize)[12];
            iRightTopRef = pic.pRefIndex[0].get(iRightTopXy as usize)[12];
        } else {
            iMvC = (*pCurDqLayer).grid.mv[0].get(iRightTopXy as usize)[12];
            iRightTopRef = (*pCurDqLayer).grid.ref_index[0].get(iRightTopXy as usize)[12];
        }
    } else {
        iMvC = [0, 0];
        iRightTopRef = if !bRightTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }

    // left_top
    if bLeftTopAvail && IS_INTER(iLeftTopType) {
        if let Some(pic) = pDec {
            iMvD = pic.pMv[0].get(iLeftTopXy as usize)[15];
            iLeftTopRef = pic.pRefIndex[0].get(iLeftTopXy as usize)[15];
        } else {
            iMvD = (*pCurDqLayer).grid.mv[0].get(iLeftTopXy as usize)[15];
            iLeftTopRef = (*pCurDqLayer).grid.ref_index[0].get(iLeftTopXy as usize)[15];
        }
    } else {
        iMvD = [0, 0];
        iLeftTopRef = if !bLeftTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }

    let mut iDiagonalRef = iRightTopRef;
    if iDiagonalRef == REF_NOT_AVAIL {
        iDiagonalRef = iLeftTopRef;
        iMvC = iMvD;
    }

    if iTopRef == REF_NOT_AVAIL && iDiagonalRef == REF_NOT_AVAIL && iLeftRef >= REF_NOT_IN_LIST {
        *iMvp = iMvA;
        return;
    }

    let iMatchRef = (0 == iLeftRef) as i32 + (0 == iTopRef) as i32 + (0 == iDiagonalRef) as i32;
    if 1 == iMatchRef {
        if 0 == iLeftRef {
            *iMvp = iMvA;
        } else if 0 == iTopRef {
            *iMvp = iMvB;
        } else {
            *iMvp = iMvC;
        }
    } else {
        iMvp[0] = WelsMedian(iMvA[0], iMvB[0], iMvC[0]);
        iMvp[1] = WelsMedian(iMvA[1], iMvB[1], iMvC[1]);
    }
}

/// General median motion vector prediction kernel for 4x4, 8x8, or 16x16 block partitions.
pub fn PredMv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: usize,
    iPartIdx: usize,
    iPartWidth: usize,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    let kuiLeftIdx = (g_kuiCache30ScanIdx[iPartIdx] - 1) as usize;
    let kuiTopIdx = (g_kuiCache30ScanIdx[iPartIdx] - 6) as usize;
    let kuiRightTopIdx = kuiTopIdx + iPartWidth;
    let kuiLeftTopIdx = kuiTopIdx - 1;

    let kiLeftRef = iRefIndex[listIdx][kuiLeftIdx];
    let kiTopRef = iRefIndex[listIdx][kuiTopIdx];
    let kiRightTopRef = iRefIndex[listIdx][kuiRightTopIdx];
    let kiLeftTopRef = iRefIndex[listIdx][kuiLeftTopIdx];
    let mut iDiagonalRef = kiRightTopRef;

    let mut iAMV = [0i16; 2];
    let mut iBMV = [0i16; 2];
    let mut iCMV = [0i16; 2];

    iAMV = iMotionVector[listIdx][kuiLeftIdx];
    iBMV = iMotionVector[listIdx][kuiTopIdx];
    iCMV = iMotionVector[listIdx][kuiRightTopIdx];

    if REF_NOT_AVAIL == iDiagonalRef {
        iDiagonalRef = kiLeftTopRef;
        iCMV = iMotionVector[listIdx][kuiLeftTopIdx];
    }

    let iMatchRef = (iRef == kiLeftRef) as i32 + (iRef == kiTopRef) as i32 + (iRef == iDiagonalRef) as i32;

    if REF_NOT_AVAIL == kiTopRef && REF_NOT_AVAIL == iDiagonalRef && kiLeftRef >= REF_NOT_IN_LIST {
        *iMVP = iAMV;
        return;
    }

    if 1 == iMatchRef {
        if iRef == kiLeftRef {
            *iMVP = iAMV;
        } else if iRef == kiTopRef {
            *iMVP = iBMV;
        } else {
            *iMVP = iCMV;
        }
    } else {
        iMVP[0] = WelsMedian(iAMV[0], iBMV[0], iCMV[0]);
        iMVP[1] = WelsMedian(iAMV[1], iBMV[1], iCMV[1]);
    }
}

/// Motion vector predictor for 8x16 macroblock partitions.
pub fn PredInter8x16Mv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: usize,
    iPartIdx: usize,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    if 0 == iPartIdx {
        let kiLeftRef = iRefIndex[listIdx][6];
        if iRef == kiLeftRef {
            *iMVP = iMotionVector[listIdx][6];
            return;
        }
    } else {
        let mut iDiagonalRef = iRefIndex[listIdx][5];
        let mut index = 5;
        if REF_NOT_AVAIL == iDiagonalRef {
            iDiagonalRef = iRefIndex[listIdx][2];
            index = 2;
        }
        if iRef == iDiagonalRef {
            *iMVP = iMotionVector[listIdx][index];
            return;
        }
    }

    PredMv(iMotionVector, iRefIndex, listIdx, iPartIdx, 2, iRef, iMVP);
}

/// Motion vector predictor for 16x8 macroblock partitions.
pub fn PredInter16x8Mv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: usize,
    iPartIdx: usize,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    if 0 == iPartIdx {
        let kiTopRef = iRefIndex[listIdx][1];
        if iRef == kiTopRef {
            *iMVP = iMotionVector[listIdx][1];
            return;
        }
    } else {
        let kiLeftRef = iRefIndex[listIdx][18];
        if iRef == kiLeftRef {
            *iMVP = iMotionVector[listIdx][18];
            return;
        }
    }

    PredMv(iMotionVector, iRefIndex, listIdx, iPartIdx, 4, iRef, iMVP);
}

// ============================================================================
// B-Slice Direct Mode Implementations
// ============================================================================

/// Retrieves collocated macroblock parameters for spatial and temporal direct modes.
pub fn GetColocatedMb(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: Option<&SPicture>,
    colocPic: Option<&SPicture>,
    mbType: &mut MbType,
    subMbType: &mut SubMbType,
) -> i32 {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    let pMbType = GetMbType(pCurDqLayer, pDec);
    let curMbType = *pMbType.get(iMbXy);
    let is8x8 = IS_Inter_8x8(curMbType);
    *mbType = curMbType;

    let Some(colocPic) = colocPic else {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
    };

    // **T5.N5's `debug_assert!` stood here and is deleted, because F42 disproved it.**
    // It said "the colocated picture is never the picture being decoded", on the
    // argument that `PrefetchPic` hands out only unreferenced slots while list 1 holds
    // references. A malformed stream breaks that: `pRefList[i]` is filled from a
    // `ref_idx` the bitstream chooses, so it can name the picture being decoded, which
    // is exactly what F42 found and what `PicRefs::get` answers by resolving that slot
    // through the mutable half's own pointer. Keeping the assert would abort in debug
    // on the input class the flip deliberately kept decodable.
    //
    // What replaces it is a type, not a check (S25's question answered by the
    // signature): the current picture arrives as `PPicture` and the colocated one as
    // `*const SPicture`, both resolved by the caller's bracket, and this function
    // reaches no container at all. Whether the two alias is decided once, in
    // `PicRefs::get`, where one tag covers both.

    let mut coloc_mbType = *colocPic.pMbType.get(iMbXy);
    if coloc_mbType == MB_TYPE_SKIP {
        coloc_mbType |= MB_TYPE_16x16 | MB_TYPE_P0L0 | MB_TYPE_P1L0;
    }

    let bDirect8x8InferenceFlag = if let Some(sps) = pCtx.active_sps() {
        sps.bDirect8x8InferenceFlag
    } else {
        false
    };

    if IS_Inter_8x8(coloc_mbType) && !bDirect8x8InferenceFlag {
        *subMbType = SUB_MB_TYPE_4x4 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_DIRECT;
        *mbType |= MB_TYPE_8x8 | MB_TYPE_L0 | MB_TYPE_L1;
    } else if !is8x8 && (IS_INTER_16x16(coloc_mbType) || IS_INTRA(coloc_mbType)) {
        *subMbType = SUB_MB_TYPE_8x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_DIRECT;
        *mbType |= MB_TYPE_16x16 | MB_TYPE_L0 | MB_TYPE_L1;
    } else {
        *subMbType = SUB_MB_TYPE_8x8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_DIRECT;
        *mbType |= MB_TYPE_8x8 | MB_TYPE_L0 | MB_TYPE_L1;
    }

    // `SetRectBlock(p, 4, 4, 4, v, 1)` is a 4x4 block of 1-byte elements over a
    // 4-byte row — the whole 16-entry array, filled with `v` broadcast through
    // `v * 0x01010101`. In the array's own units it is this (T5.R5).
    if IS_INTRA(coloc_mbType) {
        (*pCurDqLayer).iColocIntra.fill(1);
        return ERR_NONE;
    }
    (*pCurDqLayer).iColocIntra.fill(0);

    if IS_INTER_16x16(*mbType) {
        let pMv = if IS_TYPE_L1(coloc_mbType) {
            colocPic.pMv[LIST_1].get(iMbXy)[0]
        } else {
            [0i16; 2]
        };
        (*pCurDqLayer).iColocMv[LIST_0][0] = colocPic.pMv[LIST_0].get(iMbXy)[0];
        (*pCurDqLayer).iColocMv[LIST_1][0] = pMv;
        (*pCurDqLayer).iColocRefIndex[LIST_0][0] = colocPic.pRefIndex[LIST_0].get(iMbXy)[0];
        (*pCurDqLayer).iColocRefIndex[LIST_1][0] = if IS_TYPE_L1(coloc_mbType) {
            colocPic.pRefIndex[LIST_1].get(iMbXy)[0]
        } else {
            REF_NOT_IN_LIST
        };
    } else {
        if !bDirect8x8InferenceFlag {
            // Each `CopyRectBlock4Cols` here is four rows of a full row's width — the
            // whole 16-entry array — so in the arrays' own units both are one copy,
            // and the MV one moves `[i16; 2]` values rather than 16 bytes at a time
            // (F35's alignment precondition, deleted rather than met).
            (*pCurDqLayer).iColocMv[LIST_0] = *colocPic.pMv[LIST_0].get(iMbXy);
            (*pCurDqLayer).iColocRefIndex[LIST_0] = *colocPic.pRefIndex[LIST_0].get(iMbXy);
            if IS_TYPE_L1(coloc_mbType) {
                (*pCurDqLayer).iColocMv[LIST_1] = *colocPic.pMv[LIST_1].get(iMbXy);
                (*pCurDqLayer).iColocRefIndex[LIST_1] =
                    *colocPic.pRefIndex[LIST_1].get(iMbXy);
            } else {
                // The C casts to `uint8_t` here, so this fill is the plain value.
                (*pCurDqLayer).iColocRefIndex[LIST_1].fill(REF_NOT_IN_LIST);
            }
        } else {
            let maxList = 1 + (if (coloc_mbType & MB_TYPE_L1) != 0 { 1 } else { 0 });
            for listIdx in 0..maxList {
                let colocMvPtr = *colocPic.pMv[listIdx].get(iMbXy);
                set_rect_mv(&mut (*pCurDqLayer).iColocMv[listIdx], 0, colocMvPtr[0]);
                set_rect_mv(&mut (*pCurDqLayer).iColocMv[listIdx], 2, colocMvPtr[3]);
                set_rect_mv(&mut (*pCurDqLayer).iColocMv[listIdx], 8, colocMvPtr[12]);
                set_rect_mv(&mut (*pCurDqLayer).iColocMv[listIdx], 10, colocMvPtr[15]);

                // C passes the raw `int8_t` into SetRectBlock's `uint32_t val`, so a
                // negative ref index sign-extends (-1 -> 0xFFFFFFFF) and the `val *
                // 0x0101` fill then writes {-1, -2} rather than {-1, -1}. Zero-extending
                // here would silently disagree with the reference decoder, so keep the
                // sign extension. (Contrast the two `(uint8_t)REF_NOT_IN_LIST` sites,
                // where C casts to unsigned itself.)
                let colocRefPtr = *colocPic.pRefIndex[listIdx].get(iMbXy);
                set_rect_ref(&mut (*pCurDqLayer).iColocRefIndex[listIdx], 0, colocRefPtr[0]);
                set_rect_ref(&mut (*pCurDqLayer).iColocRefIndex[listIdx], 2, colocRefPtr[3]);
                set_rect_ref(&mut (*pCurDqLayer).iColocRefIndex[listIdx], 8, colocRefPtr[12]);
                set_rect_ref(&mut (*pCurDqLayer).iColocRefIndex[listIdx], 10, colocRefPtr[15]);
            }
            if (coloc_mbType & MB_TYPE_L1) == 0 {
                (*pCurDqLayer).iColocRefIndex[1].fill(REF_NOT_IN_LIST);
            }
        }
    }

    ERR_NONE
}

/// Derives motion predictors and reference indices for B-slice spatial direct mode.
/// **T5b.2 — F42 costs a parameter here and nothing else.** Everything this reads
/// through a reference is a POC, a flag or the colocated macroblock's motion, so the
/// current picture is just another *shared* source: [`PicRefs::resolve`] answers the
/// `Current` arm with the caller's own borrow, and two shared borrows of one picture
/// coexist. What forced the raw alias was `PicRefs::get`, which had to hand back an
/// address.
pub fn PredMvBDirectSpatial(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    iMvp: &mut [[i16; 2]; 2],
    ref_idx: &mut [i8; 2],
    subMbType: &mut SubMbType,
) -> i32 {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pMbType = GetMbType(pCurDqLayer, Some(&*pDec));
    let curMbType = *pMbType.get(iMbXy);
    let bSkipOrDirect = IS_SKIP(curMbType) || IS_DIRECT(curMbType);

    let mut mbType: MbType = 0;
    let colocPic = pRefs.resolve(pCtx.ref_id(LIST_1, 0), Some(&*pDec));
    let ret = GetColocatedMb(pCtx, pCurDqLayer, Some(&*pDec), colocPic, &mut mbType, subMbType);
    if ret != ERR_NONE {
        return ret;
    }

    let mut bTopAvail = false;
    let mut bLeftTopAvail = false;
    let mut bRightTopAvail = false;
    let mut bLeftAvail = false;

    let iCurXy = (*pCurDqLayer).iMbXyIndex;
    let iCurX = (*pCurDqLayer).iMbX;
    let iCurY = (*pCurDqLayer).iMbY;
    let iCurSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iCurXy as usize);

    let mut iLeftXy = 0;
    let mut iTopXy = 0;
    let mut iLeftTopXy = 0;
    let mut iRightTopXy = 0;

    if iCurX != 0 {
        iLeftXy = iCurXy - 1;
        let iLeftSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iLeftXy as usize);
        bLeftAvail = iLeftSliceIdc == iCurSliceIdc;
    }

    if iCurY != 0 {
        iTopXy = iCurXy - (*pCurDqLayer).iMbWidth;
        let iTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iTopXy as usize);
        bTopAvail = iTopSliceIdc == iCurSliceIdc;
        if iCurX != 0 {
            iLeftTopXy = iTopXy - 1;
            let iLeftTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iLeftTopXy as usize);
            bLeftTopAvail = iLeftTopSliceIdc == iCurSliceIdc;
        }
        if iCurX != ((*pCurDqLayer).iMbWidth - 1) {
            iRightTopXy = iTopXy + 1;
            let iRightTopSliceIdc = *(*pCurDqLayer).grid.slice_idc.get(iRightTopXy as usize);
            bRightTopAvail = iRightTopSliceIdc == iCurSliceIdc;
        }
    }

    let pMbTypePtr = GetMbType(pCurDqLayer, Some(&*pDec));
    let iLeftType = if iCurX != 0 && bLeftAvail { *pMbTypePtr.get(iLeftXy as usize) } else { 0 };
    let iTopType = if iCurY != 0 && bTopAvail { *pMbTypePtr.get(iTopXy as usize) } else { 0 };
    let iLeftTopType = if iCurX != 0 && iCurY != 0 && bLeftTopAvail { *pMbTypePtr.get(iLeftTopXy as usize) } else { 0 };
    let iRightTopType = if iCurX != ((*pCurDqLayer).iMbWidth - 1) && iCurY != 0 && bRightTopAvail { *pMbTypePtr.get(iRightTopXy as usize) } else { 0 };

    let mut iLeftRef = [0i8; 2];
    let mut iTopRef = [0i8; 2];
    let mut iRightTopRef = [0i8; 2];
    let mut iLeftTopRef = [0i8; 2];
    let mut iDiagonalRef = [0i8; 2];
    let mut iMvA = [[0i16; 2]; 2];
    let mut iMvB = [[0i16; 2]; 2];
    let mut iMvC = [[0i16; 2]; 2];
    let mut iMvD = [[0i16; 2]; 2];


    // **T5b.2 deleted a dead arm here, and named it rather than keeping it.** Each of
    // the four neighbour reads had a `pDec.is_null()` fallback onto
    // `pCurDqLayer.grid.mv` / `.ref_index` — a *different* pair of arrays, so it was
    // not a spelling of the same read. With the picture a borrow the arm is
    // unreachable: `WelsDecodeSlice` and `WelsDecodeAndConstructSlice` establish
    // `Some` at their brackets, above every caller of this function. S18's
    // deleted-dead disposition.
    for listIdx in 0..2 {
        if bLeftAvail && IS_INTER(iLeftType) {
            iMvA[listIdx] = (*pDec.pMv[listIdx].get(iLeftXy as usize))[3];
            iLeftRef[listIdx] = (*pDec.pRefIndex[listIdx].get(iLeftXy as usize))[3];
        } else {
            iMvA[listIdx] = [0, 0];
            iLeftRef[listIdx] = if !bLeftAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bTopAvail && IS_INTER(iTopType) {
            iMvB[listIdx] = (*pDec.pMv[listIdx].get(iTopXy as usize))[12];
            iTopRef[listIdx] = (*pDec.pRefIndex[listIdx].get(iTopXy as usize))[12];
        } else {
            iMvB[listIdx] = [0, 0];
            iTopRef[listIdx] = if !bTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bRightTopAvail && IS_INTER(iRightTopType) {
            iMvC[listIdx] = (*pDec.pMv[listIdx].get(iRightTopXy as usize))[12];
            iRightTopRef[listIdx] = (*pDec.pRefIndex[listIdx].get(iRightTopXy as usize))[12];
        } else {
            iMvC[listIdx] = [0, 0];
            iRightTopRef[listIdx] = if !bRightTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bLeftTopAvail && IS_INTER(iLeftTopType) {
            iMvD[listIdx] = (*pDec.pMv[listIdx].get(iLeftTopXy as usize))[15];
            iLeftTopRef[listIdx] = (*pDec.pRefIndex[listIdx].get(iLeftTopXy as usize))[15];
        } else {
            iMvD[listIdx] = [0, 0];
            iLeftTopRef[listIdx] = if !bLeftTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        iDiagonalRef[listIdx] = iRightTopRef[listIdx];
        if REF_NOT_AVAIL == iDiagonalRef[listIdx] {
            iDiagonalRef[listIdx] = iLeftTopRef[listIdx];
            iMvC[listIdx] = iMvD[listIdx];
        }

        let ref_temp = WELS_MIN_POSITIVE(iTopRef[listIdx], iDiagonalRef[listIdx]);
        ref_idx[listIdx] = WELS_MIN_POSITIVE(iLeftRef[listIdx], ref_temp);

        if ref_idx[listIdx] >= 0 {
            let match_count = (iLeftRef[listIdx] == ref_idx[listIdx]) as u32
                + (iTopRef[listIdx] == ref_idx[listIdx]) as u32
                + (iDiagonalRef[listIdx] == ref_idx[listIdx]) as u32;
            if match_count == 1 {
                if iLeftRef[listIdx] == ref_idx[listIdx] {
                    iMvp[listIdx] = iMvA[listIdx];
                } else if iTopRef[listIdx] == ref_idx[listIdx] {
                    iMvp[listIdx] = iMvB[listIdx];
                } else {
                    iMvp[listIdx] = iMvC[listIdx];
                }
            } else {
                iMvp[listIdx][0] = WelsMedian(iMvA[listIdx][0], iMvB[listIdx][0], iMvC[listIdx][0]);
                iMvp[listIdx][1] = WelsMedian(iMvA[listIdx][1], iMvB[listIdx][1], iMvC[listIdx][1]);
            }
        } else {
            iMvp[listIdx][0] = 0;
            iMvp[listIdx][1] = 0;
            ref_idx[listIdx] = REF_NOT_IN_LIST;
        }
    }

    if ref_idx[LIST_0] <= REF_NOT_IN_LIST && ref_idx[LIST_1] <= REF_NOT_IN_LIST {
        ref_idx[LIST_0] = 0;
        ref_idx[LIST_1] = 0;
    } else if ref_idx[LIST_1] < 0 {
        mbType &= !MB_TYPE_L1;
        *subMbType &= !MB_TYPE_L1;
    } else if ref_idx[LIST_0] < 0 {
        mbType &= !MB_TYPE_L0;
        *subMbType &= !MB_TYPE_L0;
    }
    SetMbType(pCurDqLayer, Some(&mut *pDec), iMbXy, mbType);

    let pMvd = [0i16; 2]; // T5.W12: the callee reads two; the other two were never read
    let bIsLongRef = pRefs
        .resolve(pCtx.ref_id(LIST_1, 0), Some(&*pDec))
        .is_some_and(|p| p.bIsLongRef);

    if IS_INTER_16x16(mbType) {
        if iMvp[LIST_0] != [0, 0] || iMvp[LIST_1] != [0, 0] {
            if 0 == (*pCurDqLayer).iColocIntra[0]
                && !bIsLongRef
                && (((*pCurDqLayer).iColocRefIndex[LIST_0][0] == 0
                    && ((*pCurDqLayer).iColocMv[LIST_0][0][0] + 1) as u32 <= 2
                    && ((*pCurDqLayer).iColocMv[LIST_0][0][1] + 1) as u32 <= 2)
                    || ((*pCurDqLayer).iColocRefIndex[LIST_0][0] < 0
                        && (*pCurDqLayer).iColocRefIndex[LIST_1][0] == 0
                        && ((*pCurDqLayer).iColocMv[LIST_1][0][0] + 1) as u32 <= 2
                        && ((*pCurDqLayer).iColocMv[LIST_1][0][1] + 1) as u32 <= 2))
            {
                if 0 >= ref_idx[0] {
                    iMvp[LIST_0] = [0, 0];
                }
                if 0 >= ref_idx[1] {
                    iMvp[LIST_1] = [0, 0];
                }
            }
        }
        UpdateP16x16DirectCabac(pCurDqLayer);
        for listIdx in 0..2 {
            UpdateP16x16MotionInfo(pCurDqLayer, Some(&mut *pDec), listIdx, ref_idx[listIdx as usize], &iMvp[listIdx as usize]);
            UpdateP16x16MvdCabac(pCurDqLayer, &pMvd, listIdx as i32);
        }
    } else {
        if bSkipOrDirect {
            let mut pSubPartCount = [0i8; 4];
            let mut pPartW = [0i8; 4];
            for i in 0..4 {
                let iIdx8 = (i << 2) as i16;
                (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy)[i as usize] = *subMbType;
                UpdateP8x8RefIdxCabac(pCurDqLayer, &mut *pDec, iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
                UpdateP8x8RefIdxCabac(pCurDqLayer, &mut *pDec, iIdx8 as i32, ref_idx[LIST_1], LIST_1 as i8);
                UpdateP8x8DirectCabac(pCurDqLayer, iIdx8 as i32);

                pSubPartCount[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartCount;
                pPartW[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartWidth;

                if IS_SUB_4x4(*subMbType) {
                    pSubPartCount[i as usize] = 4;
                    pPartW[i as usize] = 1;
                }
                FillSpatialDirect8x8Mv(
                    pCurDqLayer,
                    Some(&mut *pDec),
                    iIdx8,
                    pSubPartCount[i as usize],
                    pPartW[i as usize],
                    *subMbType,
                    bIsLongRef,
                    iMvp,
                    ref_idx,
                    None,
                    None,
                );
            }
        }
    }

    ret
}

/// Derives motion predictors for B-slice temporal direct mode using POC distance scaling.
/// **T5b.2 — F42 costs a parameter here and nothing else.** Everything this reads
/// through a reference is a POC, a flag or the colocated macroblock's motion, so the
/// current picture is just another *shared* source: [`PicRefs::resolve`] answers the
/// `Current` arm with the caller's own borrow, and two shared borrows of one picture
/// coexist. What forced the raw alias was `PicRefs::get`, which had to hand back an
/// address.
pub fn PredBDirectTemporal(
    pCtx: &mut SliceCtx<'_>,
    pCurDqLayer: &mut DqLayerState,
    pDec: &mut SPicture,
    pRefs: PicRefs<'_>,
    iMvp: &mut [[i16; 2]; 2],
    ref_idx: &mut [i8; 2],
    subMbType: &mut SubMbType,
) -> i32 {
    let mut ret = ERR_NONE;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pMbType = GetMbType(pCurDqLayer, Some(&*pDec));
    let curMbType = *pMbType.get(iMbXy);
    let bSkipOrDirect = IS_SKIP(curMbType) || IS_DIRECT(curMbType);

    let mut mbType: MbType = 0;
    let colocPicForMb = pRefs.resolve(pCtx.ref_id(LIST_1, 0), Some(&*pDec));
    ret = GetColocatedMb(pCtx, pCurDqLayer, Some(&*pDec), colocPicForMb, &mut mbType, subMbType);
    if ret != ERR_NONE {
        return ret;
    }

    SetMbType(pCurDqLayer, Some(&mut *pDec), iMbXy, mbType);
    // T5.W6: the two `&mut` bindings that stood here were held across sixteen calls
    // that take the layer, and **both of their uses are reads** — a ref count copied
    // out on the next line, and one `iMvScale` entry read at `:1160`. As raw pointers
    // the overlap was invisible; as borrows the compiler names it, which is S25's law
    // arriving on schedule. The fix is S25's too: no borrow outlives one expression,
    // so the count is copied here and the scale is re-read at its use.
    let pMvd = [0i16; 2]; // T5.W12: the callee reads two; the other two were never read
    let ref0Count = std::cmp::min(
        (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.uiRefCount[LIST_0],
        pCtx.sRefPic.uiRefCount[LIST_0] as i32,
    );

    if IS_INTER_16x16(mbType) {
        ref_idx[LIST_0] = 0;
        ref_idx[LIST_1] = 0;
        UpdateP16x16DirectCabac(pCurDqLayer);
        UpdateP16x16RefIdx(pCurDqLayer, Some(&mut *pDec), LIST_1 as i32, ref_idx[LIST_1]);
        *iMvp = [[0, 0]; 2];
        if (*pCurDqLayer).iColocIntra[0] != 0 {
            UpdateP16x16MotionOnly(pCurDqLayer, Some(&mut *pDec), LIST_0 as i32, &iMvp[LIST_0]);
            UpdateP16x16MotionOnly(pCurDqLayer, Some(&mut *pDec), LIST_1 as i32, &iMvp[LIST_1]);
            UpdateP16x16RefIdx(pCurDqLayer, Some(&mut *pDec), LIST_0 as i32, ref_idx[LIST_0]);
        } else {
            ref_idx[LIST_0] = 0;
            // T5.X4: the selection is which *list*, and the value read out of it is
            // one `[i16; 2]`. It stood here as a `*mut i16` re-pointed in the `else`
            // arm; the copy is taken after the selection instead, which is the same
            // two loads and no pointer.
            let colocRefIndexL0 = (*pCurDqLayer).iColocRefIndex[LIST_0][0];
            let colocList = if colocRefIndexL0 >= 0 {
                ref_idx[LIST_0] = MapColToList0(pCtx, pRefs, Some(&*pDec), colocRefIndexL0, ref0Count);
                LIST_0
            } else {
                LIST_1
            };
            let mv = (*pCurDqLayer).iColocMv[colocList][0];
            UpdateP16x16RefIdx(pCurDqLayer, Some(&mut *pDec), LIST_0 as i32, ref_idx[LIST_0]);

            let scale = (*pCurDqLayer).sLayerInfo.sSliceInLayer.iMvScale[LIST_0]
                [ref_idx[LIST_0] as usize] as i32;
            iMvp[LIST_0][0] = ((scale * (mv[0] as i32) + 128) >> 8) as i16;
            iMvp[LIST_0][1] = ((scale * (mv[1] as i32) + 128) >> 8) as i16;
            UpdateP16x16MotionOnly(pCurDqLayer, Some(&mut *pDec), LIST_0 as i32, &iMvp[LIST_0]);
            iMvp[LIST_1][0] = iMvp[LIST_0][0] - mv[0];
            iMvp[LIST_1][1] = iMvp[LIST_0][1] - mv[1];
            UpdateP16x16MotionOnly(pCurDqLayer, Some(&mut *pDec), LIST_1 as i32, &iMvp[LIST_1]);
        }
        UpdateP16x16MvdCabac(pCurDqLayer, &pMvd, LIST_0 as i32);
        UpdateP16x16MvdCabac(pCurDqLayer, &pMvd, LIST_1 as i32);
    } else {
        if bSkipOrDirect {
            let mut pSubPartCount = [0i8; 4];
            let mut pPartW = [0i8; 4];
            for i in 0..4 {
                let iIdx8 = (i << 2) as i16;
                let iScan4Idx = g_kuiScan4[iIdx8 as usize] as usize;
                (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy)[i as usize] = *subMbType;
                let mut colocList = LIST_0;

                ref_idx[LIST_1] = 0;
                UpdateP8x8RefIdxCabac(pCurDqLayer, &mut *pDec, iIdx8 as i32, ref_idx[LIST_1], LIST_1 as i8);
                if (*pCurDqLayer).iColocIntra[iScan4Idx] != 0 {
                    ref_idx[LIST_0] = 0;
                    UpdateP8x8RefIdxCabac(pCurDqLayer, &mut *pDec, iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
                    *iMvp = [[0, 0]; 2];
                } else {
                    ref_idx[LIST_0] = 0;
                    let colocRefIndexL0 = (*pCurDqLayer).iColocRefIndex[LIST_0][iScan4Idx];
                    if colocRefIndexL0 >= 0 {
                        ref_idx[LIST_0] = MapColToList0(pCtx, pRefs, Some(&*pDec), colocRefIndexL0, ref0Count);
                    } else {
                        colocList = LIST_1;
                    }
                    UpdateP8x8RefIdxCabac(pCurDqLayer, &mut *pDec, iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
                }
                UpdateP8x8DirectCabac(pCurDqLayer, iIdx8 as i32);

                pSubPartCount[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartCount;
                pPartW[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartWidth;

                if IS_SUB_4x4(*subMbType) {
                    pSubPartCount[i as usize] = 4;
                    pPartW[i as usize] = 1;
                }
                FillTemporalDirect8x8Mv(
                    pCurDqLayer,
                    Some(&mut *pDec),
                    iIdx8,
                    pSubPartCount[i as usize],
                    pPartW[i as usize],
                    *subMbType,
                    ref_idx,
                    colocList,
                    None,
                    None,
                );
            }
        }
    }
    ret
}

/// Maps collocated reference picture list 0 index into the current picture's List 0 reference list.
/// **T5b.2 — F42 costs a parameter here and nothing else.** Everything this reads
/// through a reference is a POC, a flag or the colocated macroblock's motion, so the
/// current picture is just another *shared* source: [`PicRefs::resolve`] answers the
/// `Current` arm with the caller's own borrow, and two shared borrows of one picture
/// coexist. What forced the raw alias was `PicRefs::get`, which had to hand back an
/// address.
pub fn MapColToList0(
    pCtx: &mut SliceCtx<'_>,
    pRefs: PicRefs<'_>,
    pDec: Option<&SPicture>,
    colocRefIndexL0: i8,
    ref0Count: i32,
) -> i8 {
    if (*pCtx.iErrorCode & dsRefLost) == dsRefLost {
        return 0;
    }
    let pic1 = pRefs.resolve(pCtx.ref_id(LIST_1, 0), pDec);
    if let Some(pic1) = pic1.filter(|_| (colocRefIndexL0 as usize) < 17) {
        // The one resolution in the decode path whose handle comes out of another
        // *picture* rather than out of the context: the colocated picture's own
        // list-0 entry. `pRefs` resolves it exactly as `pool_pic` did.
        let ref_pic = pRefs.resolve(pic1.pRefPic[LIST_0][colocRefIndexL0 as usize], pDec);
        if let Some(ref_pic) = ref_pic {
            let iFramePoc = ref_pic.iFramePoc;
            for i in 0..ref0Count {
                let ref0 = pRefs.resolve(pCtx.ref_id(LIST_0, i as usize), pDec);
                if ref0.is_some_and(|r| r.iFramePoc == iFramePoc) {
                    return i as i8;
                }
            }
        }
    }
    0
}

// ============================================================================
// Macroblock Motion Cache & Buffer Update Routines
// ============================================================================

/// Updates motion vector and reference index cache for a 16x16 macroblock.
pub fn UpdateP16x16MotionInfo(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    listIdx: usize,
    iRef: i8,
    iMVs: &[i16; 2],
) {
    let kiMV = *iMVs;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    for i in (0..16).step_by(4) {
        let kuiScan4Idx = g_kuiScan4[i] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;

        if let Some(pic) = pDec.as_deref_mut() {
            let ref_ptr = pic.pRefIndex[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = pic.pMv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        }
    }
}

/// Updates reference index cache for a 16x16 macroblock.
pub fn UpdateP16x16RefIdx(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    listIdx: i32,
    iRef: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    if let Some(pic) = pDec.as_deref_mut() {
        let ref_ptr = pic.pRefIndex[listIdx as usize].get_mut(iMbXy);
        for i in (0..16).step_by(4) {
            let kuiScan4Idx = g_kuiScan4[i] as usize;
            let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;
        }
    }
}

/// Updates motion vector only cache for a 16x16 macroblock.
pub fn UpdateP16x16MotionOnly(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    listIdx: i32,
    iMVs: &[i16; 2],
) {
    let kiMV = *iMVs;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    for i in (0..16).step_by(4) {
        let kuiScan4Idx = g_kuiScan4[i] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;

        if let Some(pic) = pDec.as_deref_mut() {
            let mv_ptr = pic.pMv[listIdx as usize].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        } else {
            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx as usize].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        }
    }
}

/// Updates reference index and motion vector caches for a 16x8 macroblock partition.
pub fn UpdateP16x8MotionInfo(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    iMotionVector: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIndex: &mut [[i8; 30]; LIST_A],
    listIdx: usize,
    mut iPartIdx: usize,
    iRef: i8,
    iMVs: &[i16; 2],
) {
    let kiMV = *iMVs;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    for _ in 0..2 {
        let kuiScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;
        let kuiCacheIdxPlus6 = 6 + kuiCacheIdx;

        if let Some(pic) = pDec.as_deref_mut() {
            let ref_ptr = pic.pRefIndex[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = pic.pMv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        }

        {
            let ref_cache_ptr = &mut iRefIndex[listIdx];
            ref_cache_ptr[kuiCacheIdx] = iRef;
            ref_cache_ptr[kuiCacheIdx + 1] = iRef;
            ref_cache_ptr[kuiCacheIdxPlus6] = iRef;
            ref_cache_ptr[kuiCacheIdxPlus6 + 1] = iRef;
        }

        {
            let mv_cache_ptr = &mut iMotionVector[listIdx];
            mv_cache_ptr[kuiCacheIdx] = kiMV;
            mv_cache_ptr[1 + kuiCacheIdx] = kiMV;
            mv_cache_ptr[kuiCacheIdxPlus6] = kiMV;
            mv_cache_ptr[1 + kuiCacheIdxPlus6] = kiMV;
        }

        iPartIdx += 4;
    }
}

/// Updates reference index and motion vector caches for an 8x16 macroblock partition.
pub fn UpdateP8x16MotionInfo(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    iMotionVector: &mut [[[i16; 2]; 30]; LIST_A],
    iRefIndex: &mut [[i8; 30]; LIST_A],
    listIdx: usize,
    mut iPartIdx: usize,
    iRef: i8,
    iMVs: &[i16; 2],
) {
    let kiMV = *iMVs;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    for _ in 0..2 {
        let kuiScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
        let kuiCacheIdxPlus6 = 6 + kuiCacheIdx;

        if let Some(pic) = pDec.as_deref_mut() {
            let ref_ptr = pic.pRefIndex[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = pic.pMv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx].get_mut(iMbXy);
            ref_ptr[kuiScan4Idx] = iRef;
            ref_ptr[kuiScan4Idx + 1] = iRef;
            ref_ptr[kuiScan4IdxPlus4] = iRef;
            ref_ptr[kuiScan4IdxPlus4 + 1] = iRef;

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx].get_mut(iMbXy);
            mv_ptr[kuiScan4Idx] = kiMV;
            mv_ptr[1 + kuiScan4Idx] = kiMV;
            mv_ptr[kuiScan4IdxPlus4] = kiMV;
            mv_ptr[1 + kuiScan4IdxPlus4] = kiMV;
        }

        {
            let ref_cache_ptr = &mut iRefIndex[listIdx];
            ref_cache_ptr[kuiCacheIdx] = iRef;
            ref_cache_ptr[kuiCacheIdx + 1] = iRef;
            ref_cache_ptr[kuiCacheIdxPlus6] = iRef;
            ref_cache_ptr[kuiCacheIdxPlus6 + 1] = iRef;
        }

        {
            let mv_cache_ptr = &mut iMotionVector[listIdx];
            mv_cache_ptr[kuiCacheIdx] = kiMV;
            mv_cache_ptr[1 + kuiCacheIdx] = kiMV;
            mv_cache_ptr[kuiCacheIdxPlus6] = kiMV;
            mv_cache_ptr[1 + kuiCacheIdxPlus6] = kiMV;
        }

        iPartIdx += 8;
    }
}

/// Updates reference index cache for an 8x8 macroblock partition.
pub fn Update8x8RefIdx(
    pCurDqLayer: &mut DqLayerState,
    pDec: &mut SPicture,
    iPartIdx: i16,
    listIdx: usize,
    iRef: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    // **No `pDec` guard, and this is the direction F22 runs the other way** (T5.M4):
    // `mv_pred.cpp:1175` dereferences unconditionally and the CABAC copy was the
    // faithful one; the guard here was the port's addition. T5.D1 proved `pDec`
    // cannot be null on this path in either tree.
    let pDecRef = pDec.pRefIndex[listIdx].get_mut(iMbXy);
    pDecRef[iScan4Idx] = iRef;
    pDecRef[iScan4Idx + 1] = iRef;
    pDecRef[iScan4Idx + 4] = iRef;
    pDecRef[iScan4Idx + 5] = iRef;
}

// ============================================================================
// CABAC Cache Update Helpers
// ============================================================================

pub use crate::decoder::parse_mb_syn_cabac::UpdateP8x8RefIdxCabac;

// T5.M4 (F22): `UpdateP8x8RefIdxCabac` was re-translated here. The C++ declares it
// once, in `parse_mb_syn_cabac.cpp:141`, and `mv_pred.cpp` is the *caller* (`:595`,
// `:596`, `:674`, `:677`, `:687`) — so the import below is the correspondence, and
// this copy's two added guards (`pDec`, then the ref-index list) went with it.

#[inline(always)]
pub fn UpdateP8x8DirectCabac(pCurDqLayer: &mut DqLayerState, iPartIdx: i32) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    {
        let direct_ptr = (*pCurDqLayer).grid.direct.get_mut(iMbXy);
        direct_ptr[iScan4Idx] = 1;
        direct_ptr[iScan4Idx + 1] = 1;
        direct_ptr[iScan4Idx + 4] = 1;
        direct_ptr[iScan4Idx + 5] = 1;
    }
}

#[inline(always)]
pub fn UpdateP16x16DirectCabac(pCurDqLayer: &mut DqLayerState) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let direct: u16 = (1 << 8) | 1;
    {
        let direct_ptr = (*pCurDqLayer).grid.direct.get_mut(iMbXy);
        for i in (0..16).step_by(4) {
            let kuiScan4Idx = g_kuiScan4[i] as usize;
            let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
            direct_ptr[kuiScan4Idx] = 1;
            direct_ptr[kuiScan4Idx + 1] = 1;
            direct_ptr[kuiScan4IdxPlus4] = 1;
            direct_ptr[kuiScan4IdxPlus4 + 1] = 1;
        }
    }
}

#[inline(always)]
pub fn UpdateP16x16MvdCabac(pCurDqLayer: &mut DqLayerState, pMvd: &[i16; 2], iListIdx: i32) {
    let kMvd = *pMvd;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mvd = (*pCurDqLayer).grid.mvd[iListIdx as usize].get_mut(iMbXy);
    for i in 0..16 {
        mvd[i] = kMvd;
    }
}

// ============================================================================
// Direct 8x8 Motion Vector Fill Routines
// ============================================================================

/// Populates motion vectors and clears MVDs for spatial direct 8x8 and 4x4 sub-partitions.
pub fn FillSpatialDirect8x8Mv(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    iIdx8: i16,
    iPartCount: i8,
    iPartW: i8,
    subMbType: SubMbType,
    bIsLongRef: bool,
    // T5.X4: two `[i16; 2]` and two `i8`, one per list — indexed at `LIST_0`/`LIST_1`
    // and never written, which is what the two raw out-params were carrying.
    pMvDirect: &[[i16; 2]; 2],
    iRef: &[i8; 2],
    mut pMotionVector: Option<&mut [[[i16; 2]; 30]; LIST_A]>,
    mut pMvdCache: Option<&mut [[[i16; 2]; 30]; LIST_A]>,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    for j in 0..iPartCount as i32 {
        let iPartIdx = (iIdx8 as i32 + j * iPartW as i32) as usize;
        let iScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let iColocIdx = g_kuiScan4[iPartIdx] as usize;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;

        let mut pMV = [[0i16; 2]; 2];
        if IS_SUB_8x8(subMbType) {
            pMV[0] = pMvDirect[LIST_0];
            pMV[1] = pMV[0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                dec_mv_l0[iScan4Idx] = pMV[0];
                dec_mv_l0[iScan4Idx + 1] = pMV[1];
                dec_mv_l0[iScan4Idx + 4] = pMV[0];
                dec_mv_l0[iScan4Idx + 4 + 1] = pMV[1];
            }
            {
                let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                mvd_l0[iScan4Idx] = [0, 0];
                mvd_l0[iScan4Idx + 1] = [0, 0];
                mvd_l0[iScan4Idx + 4] = [0, 0];
                mvd_l0[iScan4Idx + 4 + 1] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l0 = &mut pMotionVector[LIST_0];
                mv_cache_l0[iCacheIdx] = pMV[0];
                mv_cache_l0[iCacheIdx + 1] = pMV[1];
                mv_cache_l0[iCacheIdx + 6] = pMV[0];
                mv_cache_l0[iCacheIdx + 6 + 1] = pMV[1];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                mvd_cache_l0[iCacheIdx] = [0, 0];
                mvd_cache_l0[iCacheIdx + 1] = [0, 0];
                mvd_cache_l0[iCacheIdx + 6] = [0, 0];
                mvd_cache_l0[iCacheIdx + 6 + 1] = [0, 0];
            }

            pMV[0] = pMvDirect[LIST_1];
            pMV[1] = pMV[0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                dec_mv_l1[iScan4Idx] = pMV[0];
                dec_mv_l1[iScan4Idx + 1] = pMV[1];
                dec_mv_l1[iScan4Idx + 4] = pMV[0];
                dec_mv_l1[iScan4Idx + 4 + 1] = pMV[1];
            }
            {
                let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                mvd_l1[iScan4Idx] = [0, 0];
                mvd_l1[iScan4Idx + 1] = [0, 0];
                mvd_l1[iScan4Idx + 4] = [0, 0];
                mvd_l1[iScan4Idx + 4 + 1] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l1 = &mut pMotionVector[LIST_1];
                mv_cache_l1[iCacheIdx] = pMV[0];
                mv_cache_l1[iCacheIdx + 1] = pMV[1];
                mv_cache_l1[iCacheIdx + 6] = pMV[0];
                mv_cache_l1[iCacheIdx + 6 + 1] = pMV[1];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                mvd_cache_l1[iCacheIdx] = [0, 0];
                mvd_cache_l1[iCacheIdx + 1] = [0, 0];
                mvd_cache_l1[iCacheIdx + 6] = [0, 0];
                mvd_cache_l1[iCacheIdx + 6 + 1] = [0, 0];
            }
        } else {
            pMV[0] = pMvDirect[LIST_0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                dec_mv_l0[iScan4Idx] = pMV[0];
            }
            {
                let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                mvd_l0[iScan4Idx] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l0 = &mut pMotionVector[LIST_0];
                mv_cache_l0[iCacheIdx] = pMV[0];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                mvd_cache_l0[iCacheIdx] = [0, 0];
            }

            pMV[0] = pMvDirect[LIST_1];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                dec_mv_l1[iScan4Idx] = pMV[0];
            }
            {
                let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                mvd_l1[iScan4Idx] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l1 = &mut pMotionVector[LIST_1];
                mv_cache_l1[iCacheIdx] = pMV[0];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                mvd_cache_l1[iCacheIdx] = [0, 0];
            }
        }

        if pMvDirect[LIST_0] != [0, 0] || pMvDirect[LIST_1] != [0, 0] {
            let uiColZeroFlag = (0 == (*pCurDqLayer).iColocIntra[iColocIdx]) && !bIsLongRef &&
                ((*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] == 0 ||
                 ((*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] < 0 && (*pCurDqLayer).iColocRefIndex[LIST_1][iColocIdx] == 0));

            // T5.X4: the same list selection as `FillTemporalDirect8x8Mv`'s, spelled
            // as the index it always was rather than as a pointer into the field.
            let colocList = if 0 == (*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] {
                LIST_0
            } else {
                LIST_1
            };
            let mv = (*pCurDqLayer).iColocMv[colocList][iColocIdx];

            if IS_SUB_8x8(subMbType) {
                if uiColZeroFlag && ((mv[0] + 1) as u32 <= 2 && (mv[1] + 1) as u32 <= 2) {
                    if iRef[LIST_0] == 0 {
                        if let Some(pic) = pDec.as_deref_mut() {
                            let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                            dec_mv_l0[iScan4Idx] = [0, 0];
                            dec_mv_l0[iScan4Idx + 1] = [0, 0];
                            dec_mv_l0[iScan4Idx + 4] = [0, 0];
                            dec_mv_l0[iScan4Idx + 4 + 1] = [0, 0];
                        }
                        {
                            let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                            mvd_l0[iScan4Idx] = [0, 0];
                            mvd_l0[iScan4Idx + 1] = [0, 0];
                            mvd_l0[iScan4Idx + 4] = [0, 0];
                            mvd_l0[iScan4Idx + 4 + 1] = [0, 0];
                        }
                        if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                            let mv_cache_l0 = &mut pMotionVector[LIST_0];
                            mv_cache_l0[iCacheIdx] = [0, 0];
                            mv_cache_l0[iCacheIdx + 1] = [0, 0];
                            mv_cache_l0[iCacheIdx + 6] = [0, 0];
                            mv_cache_l0[iCacheIdx + 6 + 1] = [0, 0];
                        }
                        if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                            let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                            mvd_cache_l0[iCacheIdx] = [0, 0];
                            mvd_cache_l0[iCacheIdx + 1] = [0, 0];
                            mvd_cache_l0[iCacheIdx + 6] = [0, 0];
                            mvd_cache_l0[iCacheIdx + 6 + 1] = [0, 0];
                        }
                    }

                    if iRef[LIST_1] == 0 {
                        if let Some(pic) = pDec.as_deref_mut() {
                            let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                            dec_mv_l1[iScan4Idx] = [0, 0];
                            dec_mv_l1[iScan4Idx + 1] = [0, 0];
                            dec_mv_l1[iScan4Idx + 4] = [0, 0];
                            dec_mv_l1[iScan4Idx + 4 + 1] = [0, 0];
                        }
                        {
                            let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                            mvd_l1[iScan4Idx] = [0, 0];
                            mvd_l1[iScan4Idx + 1] = [0, 0];
                            mvd_l1[iScan4Idx + 4] = [0, 0];
                            mvd_l1[iScan4Idx + 4 + 1] = [0, 0];
                        }
                        if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                            let mv_cache_l1 = &mut pMotionVector[LIST_1];
                            mv_cache_l1[iCacheIdx] = [0, 0];
                            mv_cache_l1[iCacheIdx + 1] = [0, 0];
                            mv_cache_l1[iCacheIdx + 6] = [0, 0];
                            mv_cache_l1[iCacheIdx + 6 + 1] = [0, 0];
                        }
                        if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                            let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                            mvd_cache_l1[iCacheIdx] = [0, 0];
                            mvd_cache_l1[iCacheIdx + 1] = [0, 0];
                            mvd_cache_l1[iCacheIdx + 6] = [0, 0];
                            mvd_cache_l1[iCacheIdx + 6 + 1] = [0, 0];
                        }
                    }
                }
            } else {
                if uiColZeroFlag && ((mv[0] + 1) as u32 <= 2 && (mv[1] + 1) as u32 <= 2) {
                    if iRef[LIST_0] == 0 {
                        if let Some(pic) = pDec.as_deref_mut() {
                            let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                            dec_mv_l0[iScan4Idx] = [0, 0];
                        }
                        {
                            let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                            mvd_l0[iScan4Idx] = [0, 0];
                        }
                        if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                            let mv_cache_l0 = &mut pMotionVector[LIST_0];
                            mv_cache_l0[iCacheIdx] = [0, 0];
                        }
                        if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                            let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                            mvd_cache_l0[iCacheIdx] = [0, 0];
                        }
                    }
                    if iRef[LIST_1] == 0 {
                        if let Some(pic) = pDec.as_deref_mut() {
                            let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                            dec_mv_l1[iScan4Idx] = [0, 0];
                        }
                        {
                            let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                            mvd_l1[iScan4Idx] = [0, 0];
                        }
                        if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                            let mv_cache_l1 = &mut pMotionVector[LIST_1];
                            mv_cache_l1[iCacheIdx] = [0, 0];
                        }
                        if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                            let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                            mvd_cache_l1[iCacheIdx] = [0, 0];
                        }
                    }
                }
            }
        }
    }
}

/// Calculates and populates temporal direct motion vectors for 8x8 or 4x4 direct sub-partitions.
pub fn FillTemporalDirect8x8Mv(
    pCurDqLayer: &mut DqLayerState,
    mut pDec: Option<&mut SPicture>,
    iIdx8: i16,
    iPartCount: i8,
    iPartW: i8,
    subMbType: SubMbType,
    iRef: &[i8; 2],
    // T5.X4: `mvColoc` was `(*pCurDqLayer).iColocMv[l].as_mut_ptr()` at every one of
    // its four call sites — a raw alias of a field this function already reaches
    // through `pCurDqLayer`, whose only variable was **which list**. So the list is
    // what crosses now, and the array is read where it lives.
    colocList: usize,
    mut pMotionVector: Option<&mut [[[i16; 2]; 30]; LIST_A]>,
    mut pMvdCache: Option<&mut [[[i16; 2]; 30]; LIST_A]>,
) {
    let pSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mut pMvDirect = [[0i16; 2]; 2];

    for j in 0..iPartCount as i32 {
        let iPartIdx = (iIdx8 as i32 + j * iPartW as i32) as usize;
        let iScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let iColocIdx = g_kuiScan4[iPartIdx] as usize;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;

        let mv = (*pCurDqLayer).iColocMv[colocList][iColocIdx];

        let mut pMV = [[0i16; 2]; 2];
        if IS_SUB_8x8(subMbType) {
            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                let ref0 = iRef[LIST_0] as usize;
                let scale = pSlice.iMvScale[LIST_0][ref0] as i32;
                pMvDirect[LIST_0][0] = ((scale * (mv[0] as i32) + 128) >> 8) as i16;
                pMvDirect[LIST_0][1] = ((scale * (mv[1] as i32) + 128) >> 8) as i16;
            }
            pMV[0] = pMvDirect[LIST_0];
            pMV[1] = pMV[0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                dec_mv_l0[iScan4Idx] = pMV[0];
                dec_mv_l0[iScan4Idx + 1] = pMV[1];
                dec_mv_l0[iScan4Idx + 4] = pMV[0];
                dec_mv_l0[iScan4Idx + 4 + 1] = pMV[1];
            }
            {
                let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                mvd_l0[iScan4Idx] = [0, 0];
                mvd_l0[iScan4Idx + 1] = [0, 0];
                mvd_l0[iScan4Idx + 4] = [0, 0];
                mvd_l0[iScan4Idx + 4 + 1] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l0 = &mut pMotionVector[LIST_0];
                mv_cache_l0[iCacheIdx] = pMV[0];
                mv_cache_l0[iCacheIdx + 1] = pMV[1];
                mv_cache_l0[iCacheIdx + 6] = pMV[0];
                mv_cache_l0[iCacheIdx + 6 + 1] = pMV[1];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                mvd_cache_l0[iCacheIdx] = [0, 0];
                mvd_cache_l0[iCacheIdx + 1] = [0, 0];
                mvd_cache_l0[iCacheIdx + 6] = [0, 0];
                mvd_cache_l0[iCacheIdx + 6 + 1] = [0, 0];
            }

            if (*pCurDqLayer).iColocIntra[g_kuiScan4[iIdx8 as usize] as usize] == 0 {
                pMvDirect[LIST_1][0] = pMvDirect[LIST_0][0] - mv[0];
                pMvDirect[LIST_1][1] = pMvDirect[LIST_0][1] - mv[1];
            }
            pMV[0] = pMvDirect[LIST_1];
            pMV[1] = pMV[0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                dec_mv_l1[iScan4Idx] = pMV[0];
                dec_mv_l1[iScan4Idx + 1] = pMV[1];
                dec_mv_l1[iScan4Idx + 4] = pMV[0];
                dec_mv_l1[iScan4Idx + 4 + 1] = pMV[1];
            }
            {
                let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                mvd_l1[iScan4Idx] = [0, 0];
                mvd_l1[iScan4Idx + 1] = [0, 0];
                mvd_l1[iScan4Idx + 4] = [0, 0];
                mvd_l1[iScan4Idx + 4 + 1] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l1 = &mut pMotionVector[LIST_1];
                mv_cache_l1[iCacheIdx] = pMV[0];
                mv_cache_l1[iCacheIdx + 1] = pMV[1];
                mv_cache_l1[iCacheIdx + 6] = pMV[0];
                mv_cache_l1[iCacheIdx + 6 + 1] = pMV[1];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                mvd_cache_l1[iCacheIdx] = [0, 0];
                mvd_cache_l1[iCacheIdx + 1] = [0, 0];
                mvd_cache_l1[iCacheIdx + 6] = [0, 0];
                mvd_cache_l1[iCacheIdx + 6 + 1] = [0, 0];
            }
        } else {
            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                let ref0 = iRef[LIST_0] as usize;
                let scale = pSlice.iMvScale[LIST_0][ref0] as i32;
                pMvDirect[LIST_0][0] = ((scale * (mv[0] as i32) + 128) >> 8) as i16;
                pMvDirect[LIST_0][1] = ((scale * (mv[1] as i32) + 128) >> 8) as i16;
            }
            pMV[0] = pMvDirect[LIST_0];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l0 = pic.pMv[LIST_0].get_mut(iMbXy);
                dec_mv_l0[iScan4Idx] = pMV[0];
            }
            {
                let mvd_l0 = (*pCurDqLayer).grid.mvd[LIST_0].get_mut(iMbXy);
                mvd_l0[iScan4Idx] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l0 = &mut pMotionVector[LIST_0];
                mv_cache_l0[iCacheIdx] = pMV[0];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l0 = &mut pMvdCache[LIST_0];
                mvd_cache_l0[iCacheIdx] = [0, 0];
            }

            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                pMvDirect[LIST_1][0] = pMvDirect[LIST_0][0] - mv[0];
                pMvDirect[LIST_1][1] = pMvDirect[LIST_0][1] - mv[1];
            }
            pMV[0] = pMvDirect[LIST_1];
            if let Some(pic) = pDec.as_deref_mut() {
                let dec_mv_l1 = pic.pMv[LIST_1].get_mut(iMbXy);
                dec_mv_l1[iScan4Idx] = pMV[0];
            }
            {
                let mvd_l1 = (*pCurDqLayer).grid.mvd[LIST_1].get_mut(iMbXy);
                mvd_l1[iScan4Idx] = [0, 0];
            }
            if let Some(pMotionVector) = pMotionVector.as_deref_mut() {
                let mv_cache_l1 = &mut pMotionVector[LIST_1];
                mv_cache_l1[iCacheIdx] = pMV[0];
            }
            if let Some(pMvdCache) = pMvdCache.as_deref_mut() {
                let mvd_cache_l1 = &mut pMvdCache[LIST_1];
                mvd_cache_l1[iCacheIdx] = [0, 0];
            }
        }
    }
}
