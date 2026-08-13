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

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut,
    unused_assignments
)]

use std::ffi::c_void;

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

pub use crate::decoder::picture::{SPicture, PPicture};

pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt};


pub use crate::decoder::decoder_core::{SSlice, SLayerInfo, SDqLayer, PDqLayer};


pub use crate::decoder::decoder_context::{SRefPic, PRefPic};
// The real decoder context and SPS, not local stand-ins: these are reached through
// raw pointers from decode_slice, so the layouts must be the genuine ones.
pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};
pub use crate::decoder::parameter_sets::SSps;
pub use crate::decoder::decode_slice::{SPartMbInfo, g_ksInterBSubMbTypeInfo};
pub use crate::decoder::decode_slice::{g_kuiCache30ScanIdx, g_kuiScan4};


// ============================================================================
// Low-Level Packed Load/Store Primitives
// ============================================================================

#[inline(always)]
pub unsafe fn LD32(ptr: *const i16) -> u32 {
    (ptr as *const u32).read_unaligned()
}

#[inline(always)]
pub unsafe fn ST32(dst: *mut i16, val: u32) {
    (dst as *mut u32).write_unaligned(val);
}

#[inline(always)]
pub unsafe fn ST16(dst: *mut i8, val: u16) {
    (dst as *mut u16).write_unaligned(val);
}

#[inline(always)]
pub unsafe fn LD64(ptr: *const i16) -> u64 {
    (ptr as *const u64).read_unaligned()
}

#[inline(always)]
pub unsafe fn ST64(dst: *mut i16, val: u64) {
    (dst as *mut u64).write_unaligned(val);
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

// ============================================================================
// Memory & Block Manipulation Primitives
// ============================================================================
//
// **Every wide access below is unaligned, and that is F35** (Phase 5 session J).
// These two helpers take byte pointers and write 2 and 4 bytes at a time into
// arrays of `i8` and `i16`. That was legal only because every one of those arrays
// came from `WelsMallocz`, which returns 16-byte-aligned memory — an accident of
// the allocator, not a property of the data. `pRefIndex` is `[i8; 16]` (align 1)
// and `pMv`/`pMvd` are `[[i16; 2]; 16]` (align 2), so the moment 5.2 moves them
// into the grid's `Vec`s the allocation's alignment becomes the element's and
// every one of these becomes UB.
//
// `read_unaligned`/`write_unaligned` is the same load or store on both targets
// this project builds; what it drops is the alignment *precondition*, which is
// the only thing the aligned spelling was buying. The same rewrite fixed the 13
// direct-mode sites that were already UB — those punned stack `[i16; 2]`s, where
// no allocator was rounding the address up.

#[inline(always)]
pub unsafe fn SetRectBlock(
    vp: *mut u8,
    mut w: i32,
    h: i32,
    stride: i32,
    val: u32,
    size: i32,
) {
    let p = vp;
    w *= size;
    let v16 = if size == 4 {
        val as u16
    } else {
        // C computes this in uint32_t and truncates; callers may pass a
        // sign-extended int8_t, so the multiply has to wrap rather than panic.
        val.wrapping_mul(0x0101) as u16
    };
    let v32 = if size == 4 {
        val
    } else {
        val.wrapping_mul(0x01010101)
    };

    if w == 1 && h == 4 {
        let v8 = val as u8;
        *p.offset(0 * stride as isize) = v8;
        *p.offset(1 * stride as isize) = v8;
        *p.offset(2 * stride as isize) = v8;
        *p.offset(3 * stride as isize) = v8;
    } else if w == 2 && h == 2 {
        (p.offset(0 * stride as isize) as *mut u16).write_unaligned(v16);
        (p.offset(1 * stride as isize) as *mut u16).write_unaligned(v16);
    } else if w == 2 && h == 4 {
        (p.offset(0 * stride as isize) as *mut u16).write_unaligned(v16);
        (p.offset(1 * stride as isize) as *mut u16).write_unaligned(v16);
        (p.offset(2 * stride as isize) as *mut u16).write_unaligned(v16);
        (p.offset(3 * stride as isize) as *mut u16).write_unaligned(v16);
    } else if w == 4 && h == 2 {
        (p.offset(0 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize) as *mut u32).write_unaligned(v32);
    } else if w == 4 && h == 4 {
        (p.offset(0 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize) as *mut u32).write_unaligned(v32);
    } else if w == 8 && h == 1 {
        (p.offset(0 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
    } else if w == 8 && h == 2 {
        (p.offset(0 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 4) as *mut u32).write_unaligned(v32);
    } else if w == 8 && h == 4 {
        (p.offset(0 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize + 4) as *mut u32).write_unaligned(v32);
    } else if w == 16 && h == 2 {
        (p.offset(0 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 12) as *mut u32).write_unaligned(v32);
    } else if w == 16 && h == 3 {
        (p.offset(0 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 12) as *mut u32).write_unaligned(v32);
    } else if w == 16 && h == 4 {
        (p.offset(0 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(0 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(1 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(2 * stride as isize + 12) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize + 0) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize + 4) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize + 8) as *mut u32).write_unaligned(v32);
        (p.offset(3 * stride as isize + 12) as *mut u32).write_unaligned(v32);
    }
}

#[inline(always)]
pub unsafe fn CopyRectBlock4Cols(
    vdst: *mut u8,
    vsrc: *const u8,
    stride_dst: i32,
    stride_src: i32,
    mut w: i32,
    size: i32,
) {
    let dst = vdst;
    let src = vsrc;
    w *= size;
    if w == 1 {
        *dst.offset(stride_dst as isize * 0) = *src.offset(stride_src as isize * 0);
        *dst.offset(stride_dst as isize * 1) = *src.offset(stride_src as isize * 1);
        *dst.offset(stride_dst as isize * 2) = *src.offset(stride_src as isize * 2);
        *dst.offset(stride_dst as isize * 3) = *src.offset(stride_src as isize * 3);
    } else if w == 2 {
        (dst.offset(stride_dst as isize * 0) as *mut u16)
            .write_unaligned((src.offset(stride_src as isize * 0) as *const u16).read_unaligned());
        (dst.offset(stride_dst as isize * 1) as *mut u16)
            .write_unaligned((src.offset(stride_src as isize * 1) as *const u16).read_unaligned());
        (dst.offset(stride_dst as isize * 2) as *mut u16)
            .write_unaligned((src.offset(stride_src as isize * 2) as *const u16).read_unaligned());
        (dst.offset(stride_dst as isize * 3) as *mut u16)
            .write_unaligned((src.offset(stride_src as isize * 3) as *const u16).read_unaligned());
    } else if w == 4 {
        (dst.offset(stride_dst as isize * 0) as *mut u32)
            .write_unaligned((src.offset(stride_src as isize * 0) as *const u32).read_unaligned());
        (dst.offset(stride_dst as isize * 1) as *mut u32)
            .write_unaligned((src.offset(stride_src as isize * 1) as *const u32).read_unaligned());
        (dst.offset(stride_dst as isize * 2) as *mut u32)
            .write_unaligned((src.offset(stride_src as isize * 2) as *const u32).read_unaligned());
        (dst.offset(stride_dst as isize * 3) as *mut u32)
            .write_unaligned((src.offset(stride_src as isize * 3) as *const u32).read_unaligned());
    } else if w == 16 {
        std::ptr::copy_nonoverlapping(src.offset(stride_src as isize * 0), dst.offset(stride_dst as isize * 0), 16);
        std::ptr::copy_nonoverlapping(src.offset(stride_src as isize * 1), dst.offset(stride_dst as isize * 1), 16);
        std::ptr::copy_nonoverlapping(src.offset(stride_src as isize * 2), dst.offset(stride_dst as isize * 2), 16);
        std::ptr::copy_nonoverlapping(src.offset(stride_src as isize * 3), dst.offset(stride_dst as isize * 3), 16);
    }
}

// ============================================================================
// Macroblock Type Accessor
// ============================================================================

#[inline(always)]
pub unsafe fn GetMbType(pCurDqLayer: *mut SDqLayer) -> *mut u32 {
    if !(*pCurDqLayer).pDec.is_null() {
        (*(*pCurDqLayer).pDec).pMbType
    } else {
        (*pCurDqLayer).pMbType
    }
}

// ============================================================================
// Motion Vector Predictor Implementations
// ============================================================================

/// Calculates the predicted motion vector for a P_SKIP macroblock from its spatial neighbors.
pub unsafe fn PredPSkipMvFromNeighbor(pCurDqLayer: *mut SDqLayer, iMvp: &mut [i16; 2]) {
    let mut bTopAvail = false;
    let mut bLeftTopAvail = false;
    let mut bRightTopAvail = false;
    let mut bLeftAvail = false;

    let iCurXy = (*pCurDqLayer).iMbXyIndex;
    let iCurX = (*pCurDqLayer).iMbX;
    let iCurY = (*pCurDqLayer).iMbY;
    let iCurSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iCurXy as usize);

    let mut iLeftXy = 0;
    let mut iTopXy = 0;
    let mut iLeftTopXy = 0;
    let mut iRightTopXy = 0;

    if iCurX != 0 {
        iLeftXy = iCurXy - 1;
        let iLeftSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iLeftXy as usize);
        bLeftAvail = iLeftSliceIdc == iCurSliceIdc;
    } else {
        bLeftAvail = false;
        bLeftTopAvail = false;
    }

    if iCurY != 0 {
        iTopXy = iCurXy - (*pCurDqLayer).iMbWidth;
        let iTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iTopXy as usize);
        bTopAvail = iTopSliceIdc == iCurSliceIdc;
        if iCurX != 0 {
            iLeftTopXy = iTopXy - 1;
            let iLeftTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iLeftTopXy as usize);
            bLeftTopAvail = iLeftTopSliceIdc == iCurSliceIdc;
        } else {
            bLeftTopAvail = false;
        }
        if iCurX != ((*pCurDqLayer).iMbWidth - 1) {
            iRightTopXy = iTopXy + 1;
            let iRightTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iRightTopXy as usize);
            bRightTopAvail = iRightTopSliceIdc == iCurSliceIdc;
        } else {
            bRightTopAvail = false;
        }
    } else {
        bTopAvail = false;
        bLeftTopAvail = false;
        bRightTopAvail = false;
    }

    let pMbType = GetMbType(pCurDqLayer);
    let iLeftType = if iCurX != 0 && bLeftAvail { *pMbType.add(iLeftXy as usize) } else { 0 };
    let iTopType = if iCurY != 0 && bTopAvail { *pMbType.add(iTopXy as usize) } else { 0 };
    let iLeftTopType = if iCurX != 0 && iCurY != 0 && bLeftTopAvail { *pMbType.add(iLeftTopXy as usize) } else { 0 };
    let iRightTopType = if iCurX != ((*pCurDqLayer).iMbWidth - 1) && iCurY != 0 && bRightTopAvail { *pMbType.add(iRightTopXy as usize) } else { 0 };

    let mut iMvA = [0i16; 2];
    let mut iMvB = [0i16; 2];
    let mut iMvC = [0i16; 2];
    let mut iMvD = [0i16; 2];
    let mut iLeftRef: i8;
    let mut iTopRef: i8;
    let mut iRightTopRef: i8;
    let mut iLeftTopRef: i8;

    let pDec = (*pCurDqLayer).pDec;

    // left
    if bLeftAvail && IS_INTER(iLeftType) {
        if !pDec.is_null() {
            let mv_ptr = (*(*pDec).pMv[0].add(iLeftXy as usize))[3].as_ptr();
            ST32(iMvA.as_mut_ptr(), LD32(mv_ptr));
            iLeftRef = (*(*pDec).pRefIndex[0].add(iLeftXy as usize))[3];
        } else {
            iMvA = (*pCurDqLayer).grid.mv[0].get(iLeftXy as usize)[3];
            iLeftRef = (*pCurDqLayer).grid.ref_index[0].get(iLeftXy as usize)[3];
        }
    } else {
        ST32(iMvA.as_mut_ptr(), 0);
        iLeftRef = if !bLeftAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }
    if iLeftRef == REF_NOT_AVAIL || (iLeftRef == 0 && (iMvA[0] == 0 && iMvA[1] == 0)) {
        ST32(iMvp.as_mut_ptr(), 0);
        return;
    }

    // top
    if bTopAvail && IS_INTER(iTopType) {
        if !pDec.is_null() {
            let mv_ptr = (*(*pDec).pMv[0].add(iTopXy as usize))[12].as_ptr();
            ST32(iMvB.as_mut_ptr(), LD32(mv_ptr));
            iTopRef = (*(*pDec).pRefIndex[0].add(iTopXy as usize))[12];
        } else {
            iMvB = (*pCurDqLayer).grid.mv[0].get(iTopXy as usize)[12];
            iTopRef = (*pCurDqLayer).grid.ref_index[0].get(iTopXy as usize)[12];
        }
    } else {
        ST32(iMvB.as_mut_ptr(), 0);
        iTopRef = if !bTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }
    if iTopRef == REF_NOT_AVAIL || (iTopRef == 0 && (iMvB[0] == 0 && iMvB[1] == 0)) {
        ST32(iMvp.as_mut_ptr(), 0);
        return;
    }

    // right_top
    if bRightTopAvail && IS_INTER(iRightTopType) {
        if !pDec.is_null() {
            let mv_ptr = (*(*pDec).pMv[0].add(iRightTopXy as usize))[12].as_ptr();
            ST32(iMvC.as_mut_ptr(), LD32(mv_ptr));
            iRightTopRef = (*(*pDec).pRefIndex[0].add(iRightTopXy as usize))[12];
        } else {
            iMvC = (*pCurDqLayer).grid.mv[0].get(iRightTopXy as usize)[12];
            iRightTopRef = (*pCurDqLayer).grid.ref_index[0].get(iRightTopXy as usize)[12];
        }
    } else {
        ST32(iMvC.as_mut_ptr(), 0);
        iRightTopRef = if !bRightTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }

    // left_top
    if bLeftTopAvail && IS_INTER(iLeftTopType) {
        if !pDec.is_null() {
            let mv_ptr = (*(*pDec).pMv[0].add(iLeftTopXy as usize))[15].as_ptr();
            ST32(iMvD.as_mut_ptr(), LD32(mv_ptr));
            iLeftTopRef = (*(*pDec).pRefIndex[0].add(iLeftTopXy as usize))[15];
        } else {
            iMvD = (*pCurDqLayer).grid.mv[0].get(iLeftTopXy as usize)[15];
            iLeftTopRef = (*pCurDqLayer).grid.ref_index[0].get(iLeftTopXy as usize)[15];
        }
    } else {
        ST32(iMvD.as_mut_ptr(), 0);
        iLeftTopRef = if !bLeftTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
    }

    let mut iDiagonalRef = iRightTopRef;
    if iDiagonalRef == REF_NOT_AVAIL {
        iDiagonalRef = iLeftTopRef;
        ST32(iMvC.as_mut_ptr(), LD32(iMvD.as_ptr()));
    }

    if iTopRef == REF_NOT_AVAIL && iDiagonalRef == REF_NOT_AVAIL && iLeftRef >= REF_NOT_IN_LIST {
        ST32(iMvp.as_mut_ptr(), LD32(iMvA.as_ptr()));
        return;
    }

    let iMatchRef = (0 == iLeftRef) as i32 + (0 == iTopRef) as i32 + (0 == iDiagonalRef) as i32;
    if 1 == iMatchRef {
        if 0 == iLeftRef {
            ST32(iMvp.as_mut_ptr(), LD32(iMvA.as_ptr()));
        } else if 0 == iTopRef {
            ST32(iMvp.as_mut_ptr(), LD32(iMvB.as_ptr()));
        } else {
            ST32(iMvp.as_mut_ptr(), LD32(iMvC.as_ptr()));
        }
    } else {
        iMvp[0] = WelsMedian(iMvA[0], iMvB[0], iMvC[0]);
        iMvp[1] = WelsMedian(iMvA[1], iMvB[1], iMvC[1]);
    }
}

/// General median motion vector prediction kernel for 4x4, 8x8, or 16x16 block partitions.
pub unsafe fn PredMv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: i32,
    iPartIdx: i32,
    iPartWidth: i32,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    let kuiLeftIdx = (g_kuiCache30ScanIdx[iPartIdx as usize] - 1) as usize;
    let kuiTopIdx = (g_kuiCache30ScanIdx[iPartIdx as usize] - 6) as usize;
    let kuiRightTopIdx = kuiTopIdx + iPartWidth as usize;
    let kuiLeftTopIdx = kuiTopIdx - 1;

    let kiLeftRef = iRefIndex[listIdx as usize][kuiLeftIdx];
    let kiTopRef = iRefIndex[listIdx as usize][kuiTopIdx];
    let kiRightTopRef = iRefIndex[listIdx as usize][kuiRightTopIdx];
    let kiLeftTopRef = iRefIndex[listIdx as usize][kuiLeftTopIdx];
    let mut iDiagonalRef = kiRightTopRef;

    let mut iAMV = [0i16; 2];
    let mut iBMV = [0i16; 2];
    let mut iCMV = [0i16; 2];

    ST32(iAMV.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][kuiLeftIdx].as_ptr()));
    ST32(iBMV.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][kuiTopIdx].as_ptr()));
    ST32(iCMV.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][kuiRightTopIdx].as_ptr()));

    if REF_NOT_AVAIL == iDiagonalRef {
        iDiagonalRef = kiLeftTopRef;
        ST32(iCMV.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][kuiLeftTopIdx].as_ptr()));
    }

    let iMatchRef = (iRef == kiLeftRef) as i32 + (iRef == kiTopRef) as i32 + (iRef == iDiagonalRef) as i32;

    if REF_NOT_AVAIL == kiTopRef && REF_NOT_AVAIL == iDiagonalRef && kiLeftRef >= REF_NOT_IN_LIST {
        ST32(iMVP.as_mut_ptr(), LD32(iAMV.as_ptr()));
        return;
    }

    if 1 == iMatchRef {
        if iRef == kiLeftRef {
            ST32(iMVP.as_mut_ptr(), LD32(iAMV.as_ptr()));
        } else if iRef == kiTopRef {
            ST32(iMVP.as_mut_ptr(), LD32(iBMV.as_ptr()));
        } else {
            ST32(iMVP.as_mut_ptr(), LD32(iCMV.as_ptr()));
        }
    } else {
        iMVP[0] = WelsMedian(iAMV[0], iBMV[0], iCMV[0]);
        iMVP[1] = WelsMedian(iAMV[1], iBMV[1], iCMV[1]);
    }
}

/// Motion vector predictor for 8x16 macroblock partitions.
pub unsafe fn PredInter8x16Mv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: i32,
    iPartIdx: i32,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    if 0 == iPartIdx {
        let kiLeftRef = iRefIndex[listIdx as usize][6];
        if iRef == kiLeftRef {
            ST32(iMVP.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][6].as_ptr()));
            return;
        }
    } else {
        let mut iDiagonalRef = iRefIndex[listIdx as usize][5];
        let mut index = 5;
        if REF_NOT_AVAIL == iDiagonalRef {
            iDiagonalRef = iRefIndex[listIdx as usize][2];
            index = 2;
        }
        if iRef == iDiagonalRef {
            ST32(iMVP.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][index].as_ptr()));
            return;
        }
    }

    PredMv(iMotionVector, iRefIndex, listIdx, iPartIdx, 2, iRef, iMVP);
}

/// Motion vector predictor for 16x8 macroblock partitions.
pub unsafe fn PredInter16x8Mv(
    iMotionVector: &[[[i16; 2]; 30]; 2],
    iRefIndex: &[[i8; 30]; 2],
    listIdx: i32,
    iPartIdx: i32,
    iRef: i8,
    iMVP: &mut [i16; 2],
) {
    if 0 == iPartIdx {
        let kiTopRef = iRefIndex[listIdx as usize][1];
        if iRef == kiTopRef {
            ST32(iMVP.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][1].as_ptr()));
            return;
        }
    } else {
        let kiLeftRef = iRefIndex[listIdx as usize][18];
        if iRef == kiLeftRef {
            ST32(iMVP.as_mut_ptr(), LD32(iMotionVector[listIdx as usize][18].as_ptr()));
            return;
        }
    }

    PredMv(iMotionVector, iRefIndex, listIdx, iPartIdx, 4, iRef, iMVP);
}

// ============================================================================
// B-Slice Direct Mode Implementations
// ============================================================================

/// Retrieves collocated macroblock parameters for spatial and temporal direct modes.
pub unsafe fn GetColocatedMb(
    pCtx: *mut SWelsDecoderContext,
    mbType: &mut MbType,
    subMbType: &mut SubMbType,
) -> i32 {
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;

    let pMbType = GetMbType(pCurDqLayer);
    let curMbType = *pMbType.add(iMbXy);
    let is8x8 = IS_Inter_8x8(curMbType);
    *mbType = curMbType;

    let colocPic = (*pCtx).sRefPic.pRefList[LIST_1][0];
    if colocPic.is_null() {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_DATA, ERR_INFO_REFERENCE_PIC_LOST);
    }

    let mut coloc_mbType = *(*colocPic).pMbType.add(iMbXy);
    if coloc_mbType == MB_TYPE_SKIP {
        coloc_mbType |= MB_TYPE_16x16 | MB_TYPE_P0L0 | MB_TYPE_P1L0;
    }

    let bDirect8x8InferenceFlag = if !(*pCtx).pSps.is_null() {
        (*(*pCtx).pSps).bDirect8x8InferenceFlag
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

    if IS_INTRA(coloc_mbType) {
        SetRectBlock((*pCurDqLayer).iColocIntra.as_mut_ptr() as *mut u8, 4, 4, 4, 1, 1);
        return ERR_NONE;
    }
    SetRectBlock((*pCurDqLayer).iColocIntra.as_mut_ptr() as *mut u8, 4, 4, 4, 0, 1);

    if IS_INTER_16x16(*mbType) {
        let iMVZero = [0i16; 2];
        let pMv = if IS_TYPE_L1(coloc_mbType) {
            (*(*colocPic).pMv[LIST_1].add(iMbXy))[0].as_ptr()
        } else {
            iMVZero.as_ptr()
        };
        ST32((*pCurDqLayer).iColocMv[LIST_0][0].as_mut_ptr(), LD32((*(*colocPic).pMv[LIST_0].add(iMbXy))[0].as_ptr()));
        ST32((*pCurDqLayer).iColocMv[LIST_1][0].as_mut_ptr(), LD32(pMv));
        (*pCurDqLayer).iColocRefIndex[LIST_0][0] = (*(*colocPic).pRefIndex[LIST_0].add(iMbXy))[0];
        (*pCurDqLayer).iColocRefIndex[LIST_1][0] = if IS_TYPE_L1(coloc_mbType) {
            (*(*colocPic).pRefIndex[LIST_1].add(iMbXy))[0]
        } else {
            REF_NOT_IN_LIST
        };
    } else {
        if !bDirect8x8InferenceFlag {
            CopyRectBlock4Cols(
                (*pCurDqLayer).iColocMv[LIST_0].as_mut_ptr() as *mut u8,
                (*(*colocPic).pMv[LIST_0].add(iMbXy)).as_ptr() as *const u8,
                16, 16, 4, 4,
            );
            CopyRectBlock4Cols(
                (*pCurDqLayer).iColocRefIndex[LIST_0].as_mut_ptr() as *mut u8,
                (*(*colocPic).pRefIndex[LIST_0].add(iMbXy)).as_ptr() as *const u8,
                4, 4, 4, 1,
            );
            if IS_TYPE_L1(coloc_mbType) {
                CopyRectBlock4Cols(
                    (*pCurDqLayer).iColocMv[LIST_1].as_mut_ptr() as *mut u8,
                    (*(*colocPic).pMv[LIST_1].add(iMbXy)).as_ptr() as *const u8,
                    16, 16, 4, 4,
                );
                CopyRectBlock4Cols(
                    (*pCurDqLayer).iColocRefIndex[LIST_1].as_mut_ptr() as *mut u8,
                    (*(*colocPic).pRefIndex[LIST_1].add(iMbXy)).as_ptr() as *const u8,
                    4, 4, 4, 1,
                );
            } else {
                SetRectBlock(
                    (*pCurDqLayer).iColocRefIndex[LIST_1].as_mut_ptr() as *mut u8,
                    4, 4, 4, REF_NOT_IN_LIST as u8 as u32, 1,
                );
            }
        } else {
            let maxList = 1 + (if (coloc_mbType & MB_TYPE_L1) != 0 { 1 } else { 0 });
            for listIdx in 0..maxList {
                let colocMvPtr = *(*colocPic).pMv[listIdx].add(iMbXy);
                SetRectBlock((*pCurDqLayer).iColocMv[listIdx][0].as_mut_ptr() as *mut u8, 2, 2, 16, LD32(colocMvPtr[0].as_ptr()), 4);
                SetRectBlock((*pCurDqLayer).iColocMv[listIdx][2].as_mut_ptr() as *mut u8, 2, 2, 16, LD32(colocMvPtr[3].as_ptr()), 4);
                SetRectBlock((*pCurDqLayer).iColocMv[listIdx][8].as_mut_ptr() as *mut u8, 2, 2, 16, LD32(colocMvPtr[12].as_ptr()), 4);
                SetRectBlock((*pCurDqLayer).iColocMv[listIdx][10].as_mut_ptr() as *mut u8, 2, 2, 16, LD32(colocMvPtr[15].as_ptr()), 4);

                // C passes the raw `int8_t` into SetRectBlock's `uint32_t val`, so a
                // negative ref index sign-extends (-1 -> 0xFFFFFFFF) and the `val *
                // 0x0101` fill then writes {-1, -2} rather than {-1, -1}. Zero-extending
                // here would silently disagree with the reference decoder, so keep the
                // sign extension. (Contrast the two `(uint8_t)REF_NOT_IN_LIST` sites,
                // where C casts to unsigned itself.)
                let colocRefPtr = *(*colocPic).pRefIndex[listIdx].add(iMbXy);
                SetRectBlock((*pCurDqLayer).iColocRefIndex[listIdx].as_mut_ptr().add(0) as *mut u8, 2, 2, 4, colocRefPtr[0] as i32 as u32, 1);
                SetRectBlock((*pCurDqLayer).iColocRefIndex[listIdx].as_mut_ptr().add(2) as *mut u8, 2, 2, 4, colocRefPtr[3] as i32 as u32, 1);
                SetRectBlock((*pCurDqLayer).iColocRefIndex[listIdx].as_mut_ptr().add(8) as *mut u8, 2, 2, 4, colocRefPtr[12] as i32 as u32, 1);
                SetRectBlock((*pCurDqLayer).iColocRefIndex[listIdx].as_mut_ptr().add(10) as *mut u8, 2, 2, 4, colocRefPtr[15] as i32 as u32, 1);
            }
            if (coloc_mbType & MB_TYPE_L1) == 0 {
                SetRectBlock((*pCurDqLayer).iColocRefIndex[1].as_mut_ptr() as *mut u8, 4, 4, 4, REF_NOT_IN_LIST as u8 as u32, 1);
            }
        }
    }

    ERR_NONE
}

/// Derives motion predictors and reference indices for B-slice spatial direct mode.
pub unsafe fn PredMvBDirectSpatial(
    pCtx: *mut SWelsDecoderContext,
    iMvp: &mut [[i16; 2]; 2],
    ref_idx: &mut [i8; 2],
    subMbType: &mut SubMbType,
) -> i32 {
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pMbType = GetMbType(pCurDqLayer);
    let curMbType = *pMbType.add(iMbXy);
    let bSkipOrDirect = IS_SKIP(curMbType) || IS_DIRECT(curMbType);

    let mut mbType: MbType = 0;
    let ret = GetColocatedMb(pCtx, &mut mbType, subMbType);
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
    let iCurSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iCurXy as usize);

    let mut iLeftXy = 0;
    let mut iTopXy = 0;
    let mut iLeftTopXy = 0;
    let mut iRightTopXy = 0;

    if iCurX != 0 {
        iLeftXy = iCurXy - 1;
        let iLeftSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iLeftXy as usize);
        bLeftAvail = iLeftSliceIdc == iCurSliceIdc;
    }

    if iCurY != 0 {
        iTopXy = iCurXy - (*pCurDqLayer).iMbWidth;
        let iTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iTopXy as usize);
        bTopAvail = iTopSliceIdc == iCurSliceIdc;
        if iCurX != 0 {
            iLeftTopXy = iTopXy - 1;
            let iLeftTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iLeftTopXy as usize);
            bLeftTopAvail = iLeftTopSliceIdc == iCurSliceIdc;
        }
        if iCurX != ((*pCurDqLayer).iMbWidth - 1) {
            iRightTopXy = iTopXy + 1;
            let iRightTopSliceIdc = *(*pCurDqLayer).pSliceIdc.add(iRightTopXy as usize);
            bRightTopAvail = iRightTopSliceIdc == iCurSliceIdc;
        }
    }

    let pMbTypePtr = GetMbType(pCurDqLayer);
    let iLeftType = if iCurX != 0 && bLeftAvail { *pMbTypePtr.add(iLeftXy as usize) } else { 0 };
    let iTopType = if iCurY != 0 && bTopAvail { *pMbTypePtr.add(iTopXy as usize) } else { 0 };
    let iLeftTopType = if iCurX != 0 && iCurY != 0 && bLeftTopAvail { *pMbTypePtr.add(iLeftTopXy as usize) } else { 0 };
    let iRightTopType = if iCurX != ((*pCurDqLayer).iMbWidth - 1) && iCurY != 0 && bRightTopAvail { *pMbTypePtr.add(iRightTopXy as usize) } else { 0 };

    let mut iLeftRef = [0i8; 2];
    let mut iTopRef = [0i8; 2];
    let mut iRightTopRef = [0i8; 2];
    let mut iLeftTopRef = [0i8; 2];
    let mut iDiagonalRef = [0i8; 2];
    let mut iMvA = [[0i16; 2]; 2];
    let mut iMvB = [[0i16; 2]; 2];
    let mut iMvC = [[0i16; 2]; 2];
    let mut iMvD = [[0i16; 2]; 2];

    let pDec = (*pCurDqLayer).pDec;

    for listIdx in 0..2 {
        if bLeftAvail && IS_INTER(iLeftType) {
            if !pDec.is_null() {
                let mv_ptr = (*(*pDec).pMv[listIdx].add(iLeftXy as usize))[3].as_ptr();
                ST32(iMvA[listIdx].as_mut_ptr(), LD32(mv_ptr));
                iLeftRef[listIdx] = (*(*pDec).pRefIndex[listIdx].add(iLeftXy as usize))[3];
            } else {
                iMvA[listIdx] = (*pCurDqLayer).grid.mv[listIdx].get(iLeftXy as usize)[3];
                iLeftRef[listIdx] = (*pCurDqLayer).grid.ref_index[listIdx].get(iLeftXy as usize)[3];
            }
        } else {
            ST32(iMvA[listIdx].as_mut_ptr(), 0);
            iLeftRef[listIdx] = if !bLeftAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bTopAvail && IS_INTER(iTopType) {
            if !pDec.is_null() {
                let mv_ptr = (*(*pDec).pMv[listIdx].add(iTopXy as usize))[12].as_ptr();
                ST32(iMvB[listIdx].as_mut_ptr(), LD32(mv_ptr));
                iTopRef[listIdx] = (*(*pDec).pRefIndex[listIdx].add(iTopXy as usize))[12];
            } else {
                iMvB[listIdx] = (*pCurDqLayer).grid.mv[listIdx].get(iTopXy as usize)[12];
                iTopRef[listIdx] = (*pCurDqLayer).grid.ref_index[listIdx].get(iTopXy as usize)[12];
            }
        } else {
            ST32(iMvB[listIdx].as_mut_ptr(), 0);
            iTopRef[listIdx] = if !bTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bRightTopAvail && IS_INTER(iRightTopType) {
            if !pDec.is_null() {
                let mv_ptr = (*(*pDec).pMv[listIdx].add(iRightTopXy as usize))[12].as_ptr();
                ST32(iMvC[listIdx].as_mut_ptr(), LD32(mv_ptr));
                iRightTopRef[listIdx] = (*(*pDec).pRefIndex[listIdx].add(iRightTopXy as usize))[12];
            } else {
                iMvC[listIdx] = (*pCurDqLayer).grid.mv[listIdx].get(iRightTopXy as usize)[12];
                iRightTopRef[listIdx] = (*pCurDqLayer).grid.ref_index[listIdx].get(iRightTopXy as usize)[12];
            }
        } else {
            ST32(iMvC[listIdx].as_mut_ptr(), 0);
            iRightTopRef[listIdx] = if !bRightTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        if bLeftTopAvail && IS_INTER(iLeftTopType) {
            if !pDec.is_null() {
                let mv_ptr = (*(*pDec).pMv[listIdx].add(iLeftTopXy as usize))[15].as_ptr();
                ST32(iMvD[listIdx].as_mut_ptr(), LD32(mv_ptr));
                iLeftTopRef[listIdx] = (*(*pDec).pRefIndex[listIdx].add(iLeftTopXy as usize))[15];
            } else {
                iMvD[listIdx] = (*pCurDqLayer).grid.mv[listIdx].get(iLeftTopXy as usize)[15];
                iLeftTopRef[listIdx] = (*pCurDqLayer).grid.ref_index[listIdx].get(iLeftTopXy as usize)[15];
            }
        } else {
            ST32(iMvD[listIdx].as_mut_ptr(), 0);
            iLeftTopRef[listIdx] = if !bLeftTopAvail { REF_NOT_AVAIL } else { REF_NOT_IN_LIST };
        }

        iDiagonalRef[listIdx] = iRightTopRef[listIdx];
        if REF_NOT_AVAIL == iDiagonalRef[listIdx] {
            iDiagonalRef[listIdx] = iLeftTopRef[listIdx];
            ST32(iMvC[listIdx].as_mut_ptr(), LD32(iMvD[listIdx].as_ptr()));
        }

        let ref_temp = WELS_MIN_POSITIVE(iTopRef[listIdx], iDiagonalRef[listIdx]);
        ref_idx[listIdx] = WELS_MIN_POSITIVE(iLeftRef[listIdx], ref_temp);

        if ref_idx[listIdx] >= 0 {
            let match_count = (iLeftRef[listIdx] == ref_idx[listIdx]) as u32
                + (iTopRef[listIdx] == ref_idx[listIdx]) as u32
                + (iDiagonalRef[listIdx] == ref_idx[listIdx]) as u32;
            if match_count == 1 {
                if iLeftRef[listIdx] == ref_idx[listIdx] {
                    ST32(iMvp[listIdx].as_mut_ptr(), LD32(iMvA[listIdx].as_ptr()));
                } else if iTopRef[listIdx] == ref_idx[listIdx] {
                    ST32(iMvp[listIdx].as_mut_ptr(), LD32(iMvB[listIdx].as_ptr()));
                } else {
                    ST32(iMvp[listIdx].as_mut_ptr(), LD32(iMvC[listIdx].as_ptr()));
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
    *GetMbType(pCurDqLayer).add(iMbXy) = mbType;

    let pMvd = [0i16; 4];
    let colocPic = (*pCtx).sRefPic.pRefList[LIST_1][0];
    let bIsLongRef = if !colocPic.is_null() { (*colocPic).bIsLongRef } else { false };

    if IS_INTER_16x16(mbType) {
        if (LD32(iMvp[LIST_0].as_ptr()) | LD32(iMvp[LIST_1].as_ptr())) != 0 {
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
                    ST32(iMvp[LIST_0].as_mut_ptr(), 0);
                }
                if 0 >= ref_idx[1] {
                    ST32(iMvp[LIST_1].as_mut_ptr(), 0);
                }
            }
        }
        UpdateP16x16DirectCabac(pCurDqLayer);
        for listIdx in 0..2 {
            UpdateP16x16MotionInfo(pCurDqLayer, listIdx, ref_idx[listIdx as usize], iMvp[listIdx as usize].as_mut_ptr());
            UpdateP16x16MvdCabac(pCurDqLayer, pMvd.as_ptr(), listIdx);
        }
    } else {
        if bSkipOrDirect {
            let mut pSubPartCount = [0i8; 4];
            let mut pPartW = [0i8; 4];
            for i in 0..4 {
                let iIdx8 = (i << 2) as i16;
                (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy)[i as usize] = *subMbType;
                UpdateP8x8RefIdxCabac(pCurDqLayer, std::ptr::null_mut(), iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
                UpdateP8x8RefIdxCabac(pCurDqLayer, std::ptr::null_mut(), iIdx8 as i32, ref_idx[LIST_1], LIST_1 as i8);
                UpdateP8x8DirectCabac(pCurDqLayer, iIdx8 as i32);

                pSubPartCount[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartCount;
                pPartW[i as usize] = g_ksInterBSubMbTypeInfo[0].iPartWidth;

                if IS_SUB_4x4(*subMbType) {
                    pSubPartCount[i as usize] = 4;
                    pPartW[i as usize] = 1;
                }
                FillSpatialDirect8x8Mv(
                    pCurDqLayer,
                    iIdx8,
                    pSubPartCount[i as usize],
                    pPartW[i as usize],
                    *subMbType,
                    bIsLongRef,
                    iMvp.as_mut_ptr() as *mut [i16; 2],
                    ref_idx.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
    }

    ret
}

/// Derives motion predictors for B-slice temporal direct mode using POC distance scaling.
pub unsafe fn PredBDirectTemporal(
    pCtx: *mut SWelsDecoderContext,
    iMvp: &mut [[i16; 2]; 2],
    ref_idx: &mut [i8; 2],
    subMbType: &mut SubMbType,
) -> i32 {
    let mut ret = ERR_NONE;
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pMbType = GetMbType(pCurDqLayer);
    let curMbType = *pMbType.add(iMbXy);
    let bSkipOrDirect = IS_SKIP(curMbType) || IS_DIRECT(curMbType);

    let mut mbType: MbType = 0;
    ret = GetColocatedMb(pCtx, &mut mbType, subMbType);
    if ret != ERR_NONE {
        return ret;
    }

    *GetMbType(pCurDqLayer).add(iMbXy) = mbType;
    let pSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;
    let pMvd = [0i16; 4];
    let ref0Count = std::cmp::min(pSliceHeader.uiRefCount[LIST_0], (*pCtx).sRefPic.uiRefCount[LIST_0] as i32);

    if IS_INTER_16x16(mbType) {
        ref_idx[LIST_0] = 0;
        ref_idx[LIST_1] = 0;
        UpdateP16x16DirectCabac(pCurDqLayer);
        UpdateP16x16RefIdx(pCurDqLayer, LIST_1 as i32, ref_idx[LIST_1]);
        ST64(iMvp.as_mut_ptr() as *mut i16, 0);
        if (*pCurDqLayer).iColocIntra[0] != 0 {
            UpdateP16x16MotionOnly(pCurDqLayer, LIST_0 as i32, iMvp[LIST_0].as_mut_ptr());
            UpdateP16x16MotionOnly(pCurDqLayer, LIST_1 as i32, iMvp[LIST_1].as_mut_ptr());
            UpdateP16x16RefIdx(pCurDqLayer, LIST_0 as i32, ref_idx[LIST_0]);
        } else {
            ref_idx[LIST_0] = 0;
            let mut mv = (*pCurDqLayer).iColocMv[LIST_0][0].as_mut_ptr();
            let colocRefIndexL0 = (*pCurDqLayer).iColocRefIndex[LIST_0][0];
            if colocRefIndexL0 >= 0 {
                ref_idx[LIST_0] = MapColToList0(pCtx, colocRefIndexL0, ref0Count);
            } else {
                mv = (*pCurDqLayer).iColocMv[LIST_1][0].as_mut_ptr();
            }
            UpdateP16x16RefIdx(pCurDqLayer, LIST_0 as i32, ref_idx[LIST_0]);

            let scale = pSlice.iMvScale[LIST_0][ref_idx[LIST_0] as usize] as i32;
            iMvp[LIST_0][0] = ((scale * (*mv.add(0) as i32) + 128) >> 8) as i16;
            iMvp[LIST_0][1] = ((scale * (*mv.add(1) as i32) + 128) >> 8) as i16;
            UpdateP16x16MotionOnly(pCurDqLayer, LIST_0 as i32, iMvp[LIST_0].as_mut_ptr());
            iMvp[LIST_1][0] = iMvp[LIST_0][0] - *mv.add(0);
            iMvp[LIST_1][1] = iMvp[LIST_0][1] - *mv.add(1);
            UpdateP16x16MotionOnly(pCurDqLayer, LIST_1 as i32, iMvp[LIST_1].as_mut_ptr());
        }
        UpdateP16x16MvdCabac(pCurDqLayer, pMvd.as_ptr(), LIST_0 as i32);
        UpdateP16x16MvdCabac(pCurDqLayer, pMvd.as_ptr(), LIST_1 as i32);
    } else {
        if bSkipOrDirect {
            let mut pSubPartCount = [0i8; 4];
            let mut pPartW = [0i8; 4];
            for i in 0..4 {
                let iIdx8 = (i << 2) as i16;
                let iScan4Idx = g_kuiScan4[iIdx8 as usize] as usize;
                (*pCurDqLayer).grid.sub_mb_type.get_mut(iMbXy)[i as usize] = *subMbType;
                let mut mvColoc = (*pCurDqLayer).iColocMv[LIST_0].as_mut_ptr();

                ref_idx[LIST_1] = 0;
                UpdateP8x8RefIdxCabac(pCurDqLayer, std::ptr::null_mut(), iIdx8 as i32, ref_idx[LIST_1], LIST_1 as i8);
                if (*pCurDqLayer).iColocIntra[iScan4Idx] != 0 {
                    ref_idx[LIST_0] = 0;
                    UpdateP8x8RefIdxCabac(pCurDqLayer, std::ptr::null_mut(), iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
                    ST64(iMvp.as_mut_ptr() as *mut i16, 0);
                } else {
                    ref_idx[LIST_0] = 0;
                    let colocRefIndexL0 = (*pCurDqLayer).iColocRefIndex[LIST_0][iScan4Idx];
                    if colocRefIndexL0 >= 0 {
                        ref_idx[LIST_0] = MapColToList0(pCtx, colocRefIndexL0, ref0Count);
                    } else {
                        mvColoc = (*pCurDqLayer).iColocMv[LIST_1].as_mut_ptr();
                    }
                    UpdateP8x8RefIdxCabac(pCurDqLayer, std::ptr::null_mut(), iIdx8 as i32, ref_idx[LIST_0], LIST_0 as i8);
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
                    iIdx8,
                    pSubPartCount[i as usize],
                    pPartW[i as usize],
                    *subMbType,
                    ref_idx.as_mut_ptr(),
                    mvColoc as *mut [i16; 2],
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
    }
    ret
}

/// Maps collocated reference picture list 0 index into the current picture's List 0 reference list.
pub unsafe fn MapColToList0(
    pCtx: *mut SWelsDecoderContext,
    colocRefIndexL0: i8,
    ref0Count: i32,
) -> i8 {
    if ((*pCtx).iErrorCode & dsRefLost) == dsRefLost {
        return 0;
    }
    let pic1 = (*pCtx).sRefPic.pRefList[LIST_1][0];
    if !pic1.is_null() && (colocRefIndexL0 as usize) < 17 {
        let ref_pic_ptr = (*pic1).pRefPic[LIST_0][colocRefIndexL0 as usize];
        if !ref_pic_ptr.is_null() {
            let iFramePoc = (*ref_pic_ptr).iFramePoc;
            for i in 0..ref0Count {
                let ref0 = (*pCtx).sRefPic.pRefList[LIST_0][i as usize];
                if !ref0.is_null() && (*ref0).iFramePoc == iFramePoc {
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
pub unsafe fn UpdateP16x16MotionInfo(
    pCurDqLayer: *mut SDqLayer,
    listIdx: i32,
    iRef: i8,
    iMVs: *const i16,
) {
    let kiRef2 = ((iRef as u8 as u16) << 8) | (iRef as u8 as u16);
    let kiMV32 = LD32(iMVs);
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    for i in (0..16).step_by(4) {
        let kuiScan4Idx = g_kuiScan4[i] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;

        if !pDec.is_null() {
            let ref_ptr = (*(*pDec).pRefIndex[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*(*pDec).pMv[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        }
    }
}

/// Updates reference index cache for a 16x16 macroblock.
pub unsafe fn UpdateP16x16RefIdx(
    pCurDqLayer: *mut SDqLayer,
    listIdx: i32,
    iRef: i8,
) {
    let kiRef2 = ((iRef as u8 as u16) << 8) | (iRef as u8 as u16);
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    if !pDec.is_null() {
        let ref_ptr = (*(*pDec).pRefIndex[listIdx as usize].add(iMbXy)).as_mut_ptr();
        for i in (0..16).step_by(4) {
            let kuiScan4Idx = g_kuiScan4[i] as usize;
            let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);
        }
    }
}

/// Updates motion vector only cache for a 16x16 macroblock.
pub unsafe fn UpdateP16x16MotionOnly(
    pCurDqLayer: *mut SDqLayer,
    listIdx: i32,
    iMVs: *const i16,
) {
    let kiMV32 = LD32(iMVs);
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    for i in (0..16).step_by(4) {
        let kuiScan4Idx = g_kuiScan4[i] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;

        if !pDec.is_null() {
            let mv_ptr = (*(*pDec).pMv[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        } else {
            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        }
    }
}

/// Updates reference index and motion vector caches for a 16x8 macroblock partition.
pub unsafe fn UpdateP16x8MotionInfo(
    pCurDqLayer: *mut SDqLayer,
    iMotionVector: *mut [[i16; 2]; 30],
    iRefIndex: *mut [i8; 30],
    listIdx: i32,
    mut iPartIdx: i32,
    iRef: i8,
    iMVs: *const i16,
) {
    let kiRef2 = ((iRef as u8 as u16) << 8) | (iRef as u8 as u16);
    let kiMV32 = LD32(iMVs);
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    for _ in 0..2 {
        let kuiScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        let kuiCacheIdxPlus6 = 6 + kuiCacheIdx;

        if !pDec.is_null() {
            let ref_ptr = (*(*pDec).pRefIndex[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*(*pDec).pMv[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        }

        if !iRefIndex.is_null() {
            let ref_cache_ptr = (*iRefIndex.add(listIdx as usize)).as_mut_ptr();
            ST16(ref_cache_ptr.add(kuiCacheIdx), kiRef2);
            ST16(ref_cache_ptr.add(kuiCacheIdxPlus6), kiRef2);
        }

        if !iMotionVector.is_null() {
            let mv_cache_ptr = (*iMotionVector.add(listIdx as usize)).as_mut_ptr();
            ST32(mv_cache_ptr.add(kuiCacheIdx) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(1 + kuiCacheIdx) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(kuiCacheIdxPlus6) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(1 + kuiCacheIdxPlus6) as *mut i16, kiMV32);
        }

        iPartIdx += 4;
    }
}

/// Updates reference index and motion vector caches for an 8x16 macroblock partition.
pub unsafe fn UpdateP8x16MotionInfo(
    pCurDqLayer: *mut SDqLayer,
    iMotionVector: *mut [[i16; 2]; 30],
    iRefIndex: *mut [i8; 30],
    listIdx: i32,
    mut iPartIdx: i32,
    iRef: i8,
    iMVs: *const i16,
) {
    let kiRef2 = ((iRef as u8 as u16) << 8) | (iRef as u8 as u16);
    let kiMV32 = LD32(iMVs);
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    for _ in 0..2 {
        let kuiScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
        let kuiCacheIdxPlus6 = 6 + kuiCacheIdx;

        if !pDec.is_null() {
            let ref_ptr = (*(*pDec).pRefIndex[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*(*pDec).pMv[listIdx as usize].add(iMbXy)).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        } else {
            let ref_ptr = (*pCurDqLayer).grid.ref_index[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST16(ref_ptr.add(kuiScan4Idx), kiRef2);
            ST16(ref_ptr.add(kuiScan4IdxPlus4), kiRef2);

            let mv_ptr = (*pCurDqLayer).grid.mv[listIdx as usize].get_mut(iMbXy).as_mut_ptr();
            ST32(mv_ptr.add(kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4Idx) as *mut i16, kiMV32);
            ST32(mv_ptr.add(kuiScan4IdxPlus4) as *mut i16, kiMV32);
            ST32(mv_ptr.add(1 + kuiScan4IdxPlus4) as *mut i16, kiMV32);
        }

        if !iRefIndex.is_null() {
            let ref_cache_ptr = (*iRefIndex.add(listIdx as usize)).as_mut_ptr();
            ST16(ref_cache_ptr.add(kuiCacheIdx), kiRef2);
            ST16(ref_cache_ptr.add(kuiCacheIdxPlus6), kiRef2);
        }

        if !iMotionVector.is_null() {
            let mv_cache_ptr = (*iMotionVector.add(listIdx as usize)).as_mut_ptr();
            ST32(mv_cache_ptr.add(kuiCacheIdx) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(1 + kuiCacheIdx) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(kuiCacheIdxPlus6) as *mut i16, kiMV32);
            ST32(mv_cache_ptr.add(1 + kuiCacheIdxPlus6) as *mut i16, kiMV32);
        }

        iPartIdx += 8;
    }
}

/// Updates reference index cache for an 8x8 macroblock partition.
pub unsafe fn Update8x8RefIdx(
    pCurDqLayer: *mut SDqLayer,
    iPartIdx: i16,
    listIdx: i32,
    iRef: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    let pDec = (*pCurDqLayer).pDec;
    if !pDec.is_null() {
        let ref_ptr = (*(*pDec).pRefIndex[listIdx as usize].add(iMbXy)).as_mut_ptr();
        *ref_ptr.add(iScan4Idx) = iRef;
        *ref_ptr.add(iScan4Idx + 1) = iRef;
        *ref_ptr.add(iScan4Idx + 4) = iRef;
        *ref_ptr.add(iScan4Idx + 5) = iRef;
    }
}

// ============================================================================
// CABAC Cache Update Helpers
// ============================================================================

#[inline(always)]
pub unsafe fn UpdateP8x8RefIdxCabac(
    pCurDqLayer: *mut SDqLayer,
    _pRefIndex: *mut [i8; 30],
    iPartIdx: i32,
    iRef: i8,
    iListIdx: i8,
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    let pDec = (*pCurDqLayer).pDec;
    if !pDec.is_null() {
        let pRefIdxList = (*pDec).pRefIndex[iListIdx as usize];
        if !pRefIdxList.is_null() {
            let ref_ptr = (*pRefIdxList.add(iMbXy)).as_mut_ptr();
            *ref_ptr.add(iScan4Idx) = iRef;
            *ref_ptr.add(iScan4Idx + 1) = iRef;
            *ref_ptr.add(iScan4Idx + 4) = iRef;
            *ref_ptr.add(iScan4Idx + 5) = iRef;
        }
    }
}

#[inline(always)]
pub unsafe fn UpdateP8x8DirectCabac(pCurDqLayer: *mut SDqLayer, iPartIdx: i32) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let iScan4Idx = g_kuiScan4[iPartIdx as usize] as usize;
    if !(*pCurDqLayer).pDirect.is_null() {
        let direct_ptr = (*(*pCurDqLayer).pDirect.add(iMbXy)).as_mut_ptr();
        *direct_ptr.add(iScan4Idx) = 1;
        *direct_ptr.add(iScan4Idx + 1) = 1;
        *direct_ptr.add(iScan4Idx + 4) = 1;
        *direct_ptr.add(iScan4Idx + 5) = 1;
    }
}

#[inline(always)]
pub unsafe fn UpdateP16x16DirectCabac(pCurDqLayer: *mut SDqLayer) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let direct: u16 = (1 << 8) | 1;
    if !(*pCurDqLayer).pDirect.is_null() {
        let direct_ptr = (*(*pCurDqLayer).pDirect.add(iMbXy)).as_mut_ptr();
        for i in (0..16).step_by(4) {
            let kuiScan4Idx = g_kuiScan4[i] as usize;
            let kuiScan4IdxPlus4 = 4 + kuiScan4Idx;
            ST16(direct_ptr.add(kuiScan4Idx), direct);
            ST16(direct_ptr.add(kuiScan4IdxPlus4), direct);
        }
    }
}

#[inline(always)]
pub unsafe fn UpdateP16x16MvdCabac(pCurDqLayer: *mut SDqLayer, pMvd: *const i16, iListIdx: i32) {
    let mut pMvd32 = [0i32; 2];
    ST32(pMvd32.as_mut_ptr() as *mut i16, LD32(pMvd));
    ST32((pMvd32.as_mut_ptr() as *mut i16).add(2), LD32(pMvd));
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pMvdTarget = (*pCurDqLayer).pMvd[iListIdx as usize];
    if !pMvdTarget.is_null() {
        let mvd_ptr = (*pMvdTarget.add(iMbXy)).as_mut_ptr();
        for i in (0..16).step_by(2) {
            ST64(mvd_ptr.add(i) as *mut i16, LD64(pMvd32.as_ptr() as *const i16));
        }
    }
}

// ============================================================================
// Direct 8x8 Motion Vector Fill Routines
// ============================================================================

/// Populates motion vectors and clears MVDs for spatial direct 8x8 and 4x4 sub-partitions.
pub unsafe fn FillSpatialDirect8x8Mv(
    pCurDqLayer: *mut SDqLayer,
    iIdx8: i16,
    iPartCount: i8,
    iPartW: i8,
    subMbType: SubMbType,
    bIsLongRef: bool,
    pMvDirect: *mut [i16; 2],
    iRef: *mut i8,
    pMotionVector: *mut [[i16; 2]; 30],
    pMvdCache: *mut [[i16; 2]; 30],
) {
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let pDec = (*pCurDqLayer).pDec;

    for j in 0..iPartCount as i32 {
        let iPartIdx = (iIdx8 as i32 + j * iPartW as i32) as usize;
        let iScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let iColocIdx = g_kuiScan4[iPartIdx] as usize;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;

        let mut pMV = [0i16; 4];
        if IS_SUB_8x8(subMbType) {
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect.add(LIST_0) as *const i16));
            ST32(pMV.as_mut_ptr().add(2), LD32(pMV.as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                ST64(dec_mv_l0.add(iScan4Idx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(dec_mv_l0.add(iScan4Idx + 4) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                ST64(mvd_l0.add(iScan4Idx) as *mut i16, 0);
                ST64(mvd_l0.add(iScan4Idx + 4) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                ST64(mv_cache_l0.add(iCacheIdx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(mv_cache_l0.add(iCacheIdx + 6) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                ST64(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
                ST64(mvd_cache_l0.add(iCacheIdx + 6) as *mut i16, 0);
            }

            ST32(pMV.as_mut_ptr(), LD32(pMvDirect.add(LIST_1) as *const i16));
            ST32(pMV.as_mut_ptr().add(2), LD32(pMV.as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                ST64(dec_mv_l1.add(iScan4Idx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(dec_mv_l1.add(iScan4Idx + 4) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                ST64(mvd_l1.add(iScan4Idx) as *mut i16, 0);
                ST64(mvd_l1.add(iScan4Idx + 4) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                ST64(mv_cache_l1.add(iCacheIdx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(mv_cache_l1.add(iCacheIdx + 6) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                ST64(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
                ST64(mvd_cache_l1.add(iCacheIdx + 6) as *mut i16, 0);
            }
        } else {
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect.add(LIST_0) as *const i16));
            if !pDec.is_null() {
                let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                ST32(dec_mv_l0.add(iScan4Idx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                ST32(mvd_l0.add(iScan4Idx) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                ST32(mv_cache_l0.add(iCacheIdx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                ST32(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
            }

            ST32(pMV.as_mut_ptr(), LD32(pMvDirect.add(LIST_1) as *const i16));
            if !pDec.is_null() {
                let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                ST32(dec_mv_l1.add(iScan4Idx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                ST32(mvd_l1.add(iScan4Idx) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                ST32(mv_cache_l1.add(iCacheIdx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                ST32(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
            }
        }

        if (LD32(pMvDirect.add(LIST_0) as *const i16) | LD32(pMvDirect.add(LIST_1) as *const i16)) != 0 {
            let uiColZeroFlag = (0 == (*pCurDqLayer).iColocIntra[iColocIdx]) && !bIsLongRef &&
                ((*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] == 0 ||
                 ((*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] < 0 && (*pCurDqLayer).iColocRefIndex[LIST_1][iColocIdx] == 0));

            let mvColoc = if 0 == (*pCurDqLayer).iColocRefIndex[LIST_0][iColocIdx] {
                (*pCurDqLayer).iColocMv[LIST_0].as_ptr()
            } else {
                (*pCurDqLayer).iColocMv[LIST_1].as_ptr()
            };
            let mv = (*mvColoc.add(iColocIdx)).as_ptr();

            if IS_SUB_8x8(subMbType) {
                if uiColZeroFlag && ((*mv.add(0) + 1) as u32 <= 2 && (*mv.add(1) + 1) as u32 <= 2) {
                    if *iRef.add(LIST_0) == 0 {
                        if !pDec.is_null() {
                            let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                            ST64(dec_mv_l0.add(iScan4Idx) as *mut i16, 0);
                            ST64(dec_mv_l0.add(iScan4Idx + 4) as *mut i16, 0);
                        }
                        if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                            let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                            ST64(mvd_l0.add(iScan4Idx) as *mut i16, 0);
                            ST64(mvd_l0.add(iScan4Idx + 4) as *mut i16, 0);
                        }
                        if !pMotionVector.is_null() {
                            let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                            ST64(mv_cache_l0.add(iCacheIdx) as *mut i16, 0);
                            ST64(mv_cache_l0.add(iCacheIdx + 6) as *mut i16, 0);
                        }
                        if !pMvdCache.is_null() {
                            let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                            ST64(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
                            ST64(mvd_cache_l0.add(iCacheIdx + 6) as *mut i16, 0);
                        }
                    }

                    if *iRef.add(LIST_1) == 0 {
                        if !pDec.is_null() {
                            let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                            ST64(dec_mv_l1.add(iScan4Idx) as *mut i16, 0);
                            ST64(dec_mv_l1.add(iScan4Idx + 4) as *mut i16, 0);
                        }
                        if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                            let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                            ST64(mvd_l1.add(iScan4Idx) as *mut i16, 0);
                            ST64(mvd_l1.add(iScan4Idx + 4) as *mut i16, 0);
                        }
                        if !pMotionVector.is_null() {
                            let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                            ST64(mv_cache_l1.add(iCacheIdx) as *mut i16, 0);
                            ST64(mv_cache_l1.add(iCacheIdx + 6) as *mut i16, 0);
                        }
                        if !pMvdCache.is_null() {
                            let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                            ST64(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
                            ST64(mvd_cache_l1.add(iCacheIdx + 6) as *mut i16, 0);
                        }
                    }
                }
            } else {
                if uiColZeroFlag && ((*mv.add(0) + 1) as u32 <= 2 && (*mv.add(1) + 1) as u32 <= 2) {
                    if *iRef.add(LIST_0) == 0 {
                        if !pDec.is_null() {
                            let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                            ST32(dec_mv_l0.add(iScan4Idx) as *mut i16, 0);
                        }
                        if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                            let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                            ST32(mvd_l0.add(iScan4Idx) as *mut i16, 0);
                        }
                        if !pMotionVector.is_null() {
                            let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                            ST32(mv_cache_l0.add(iCacheIdx) as *mut i16, 0);
                        }
                        if !pMvdCache.is_null() {
                            let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                            ST32(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
                        }
                    }
                    if *iRef.add(LIST_1) == 0 {
                        if !pDec.is_null() {
                            let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                            ST32(dec_mv_l1.add(iScan4Idx) as *mut i16, 0);
                        }
                        if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                            let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                            ST32(mvd_l1.add(iScan4Idx) as *mut i16, 0);
                        }
                        if !pMotionVector.is_null() {
                            let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                            ST32(mv_cache_l1.add(iCacheIdx) as *mut i16, 0);
                        }
                        if !pMvdCache.is_null() {
                            let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                            ST32(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
                        }
                    }
                }
            }
        }
    }
}

/// Calculates and populates temporal direct motion vectors for 8x8 or 4x4 direct sub-partitions.
pub unsafe fn FillTemporalDirect8x8Mv(
    pCurDqLayer: *mut SDqLayer,
    iIdx8: i16,
    iPartCount: i8,
    iPartW: i8,
    subMbType: SubMbType,
    iRef: *mut i8,
    mvColoc: *mut [i16; 2],
    pMotionVector: *mut [[i16; 2]; 30],
    pMvdCache: *mut [[i16; 2]; 30],
) {
    let pSlice = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer;
    let iMbXy = (*pCurDqLayer).iMbXyIndex as usize;
    let mut pMvDirect = [[0i16; 2]; 2];
    let pDec = (*pCurDqLayer).pDec;

    for j in 0..iPartCount as i32 {
        let iPartIdx = (iIdx8 as i32 + j * iPartW as i32) as usize;
        let iScan4Idx = g_kuiScan4[iPartIdx] as usize;
        let iColocIdx = g_kuiScan4[iPartIdx] as usize;
        let iCacheIdx = g_kuiCache30ScanIdx[iPartIdx] as usize;

        let mv = (*mvColoc.add(iColocIdx)).as_ptr();

        let mut pMV = [0i16; 4];
        if IS_SUB_8x8(subMbType) {
            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                let ref0 = *iRef.add(LIST_0) as usize;
                let scale = pSlice.iMvScale[LIST_0][ref0] as i32;
                pMvDirect[LIST_0][0] = ((scale * (*mv.add(0) as i32) + 128) >> 8) as i16;
                pMvDirect[LIST_0][1] = ((scale * (*mv.add(1) as i32) + 128) >> 8) as i16;
            }
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect[LIST_0].as_ptr()));
            ST32(pMV.as_mut_ptr().add(2), LD32(pMV.as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                ST64(dec_mv_l0.add(iScan4Idx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(dec_mv_l0.add(iScan4Idx + 4) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                ST64(mvd_l0.add(iScan4Idx) as *mut i16, 0);
                ST64(mvd_l0.add(iScan4Idx + 4) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                ST64(mv_cache_l0.add(iCacheIdx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(mv_cache_l0.add(iCacheIdx + 6) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                ST64(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
                ST64(mvd_cache_l0.add(iCacheIdx + 6) as *mut i16, 0);
            }

            if (*pCurDqLayer).iColocIntra[g_kuiScan4[iIdx8 as usize] as usize] == 0 {
                pMvDirect[LIST_1][0] = pMvDirect[LIST_0][0] - *mv.add(0);
                pMvDirect[LIST_1][1] = pMvDirect[LIST_0][1] - *mv.add(1);
            }
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect[LIST_1].as_ptr()));
            ST32(pMV.as_mut_ptr().add(2), LD32(pMV.as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                ST64(dec_mv_l1.add(iScan4Idx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(dec_mv_l1.add(iScan4Idx + 4) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                ST64(mvd_l1.add(iScan4Idx) as *mut i16, 0);
                ST64(mvd_l1.add(iScan4Idx + 4) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                ST64(mv_cache_l1.add(iCacheIdx) as *mut i16, LD64(pMV.as_ptr()));
                ST64(mv_cache_l1.add(iCacheIdx + 6) as *mut i16, LD64(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                ST64(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
                ST64(mvd_cache_l1.add(iCacheIdx + 6) as *mut i16, 0);
            }
        } else {
            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                let ref0 = *iRef.add(LIST_0) as usize;
                let scale = pSlice.iMvScale[LIST_0][ref0] as i32;
                pMvDirect[LIST_0][0] = ((scale * (*mv.add(0) as i32) + 128) >> 8) as i16;
                pMvDirect[LIST_0][1] = ((scale * (*mv.add(1) as i32) + 128) >> 8) as i16;
            }
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect[LIST_0].as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l0 = (*(*pDec).pMv[LIST_0].add(iMbXy)).as_mut_ptr();
                ST32(dec_mv_l0.add(iScan4Idx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_0].is_null() {
                let mvd_l0 = (*(*pCurDqLayer).pMvd[LIST_0].add(iMbXy)).as_mut_ptr();
                ST32(mvd_l0.add(iScan4Idx) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l0 = (*pMotionVector.add(LIST_0)).as_mut_ptr();
                ST32(mv_cache_l0.add(iCacheIdx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l0 = (*pMvdCache.add(LIST_0)).as_mut_ptr();
                ST32(mvd_cache_l0.add(iCacheIdx) as *mut i16, 0);
            }

            if (*pCurDqLayer).iColocIntra[iColocIdx] == 0 {
                pMvDirect[LIST_1][0] = pMvDirect[LIST_0][0] - *mv.add(0);
                pMvDirect[LIST_1][1] = pMvDirect[LIST_0][1] - *mv.add(1);
            }
            ST32(pMV.as_mut_ptr(), LD32(pMvDirect[LIST_1].as_ptr()));
            if !pDec.is_null() {
                let dec_mv_l1 = (*(*pDec).pMv[LIST_1].add(iMbXy)).as_mut_ptr();
                ST32(dec_mv_l1.add(iScan4Idx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !(*pCurDqLayer).pMvd[LIST_1].is_null() {
                let mvd_l1 = (*(*pCurDqLayer).pMvd[LIST_1].add(iMbXy)).as_mut_ptr();
                ST32(mvd_l1.add(iScan4Idx) as *mut i16, 0);
            }
            if !pMotionVector.is_null() {
                let mv_cache_l1 = (*pMotionVector.add(LIST_1)).as_mut_ptr();
                ST32(mv_cache_l1.add(iCacheIdx) as *mut i16, LD32(pMV.as_ptr()));
            }
            if !pMvdCache.is_null() {
                let mvd_cache_l1 = (*pMvdCache.add(LIST_1)).as_mut_ptr();
                ST32(mvd_cache_l1.add(iCacheIdx) as *mut i16, 0);
            }
        }
    }
}
