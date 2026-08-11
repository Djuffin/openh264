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

//! # OpenH264 Video Encoder: Slice Encoding Subsystem
//!
//! Translated from `codec/encoder/core/inc/svc_encode_slice.h` and
//! `codec/encoder/core/src/svc_encode_slice.cpp`.
//!
//! Handles slice-level macroblock traversal, rate-control target quantization parameters,
//! slice header serialization (AVC Base and SVC Extension), intra/inter macroblock encoding loops,
//! dynamic MTU slice boundary enforcement and rollback, multithreaded slice memory buffer
//! reallocation, and NAL index buffer resizing.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut
)]

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;
use crate::{
    SliceMode, SFrameBSInfo, SLayerBSInfo, SSliceArgument,
    MAX_LAYER_NUM_OF_FRAME, MAX_SPATIAL_LAYER_NUM, MAX_QUALITY_LAYER_NUM, MAX_NAL_UNITS_IN_LAYER,
};

// ============================================================================
// Constants and Definitions
// ============================================================================

pub use crate::encoder::encoder_context::EWelsSliceType;

pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;
pub const SP_SLICE: i32 = 3;
pub const SI_SLICE: i32 = 4;

pub const LEFT_MB_POS: u8 = 0x01;
pub const TOP_MB_POS: u8 = 0x02;
pub const TOPRIGHT_MB_POS: u8 = 0x04;
pub const TOPLEFT_MB_POS: u8 = 0x08;

/// `rc.h:77` says **2**, not 1. `UpdateQpForOverflow` is the only user, so a
/// macroblock that overflowed the CAVLC level suffix was re-encoded one QP step
/// higher than the reference chose. Only reachable at very low QP, which is why
/// it survived every sweep except qp=0. Re-exported from the one definition.
pub use crate::encoder::rc::DELTA_QP;
pub const MB_COEFF_LIST_SIZE: usize = 384;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_LUMA_CHROMA_BLOCK4x4_NUM: usize = 24;
// wels_const.h:69 says 4. This module had 8, which over-sized SDqLayer's
// sSliceBufferInfo and its four partition arrays by 128 bytes in total.
pub use crate::encoder::encoder_context::MAX_THREADS_NUM;
pub const MAX_REF_PIC_COUNT: u32 = 16;
pub const INT_MULTIPLY: i32 = 100;
pub const SLICE_NUM_EXPAND_COEF: i32 = 2;
pub const AVER_MARGIN_BYTES: u32 = 100;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNSUPPORTED_PARA: i32 = 0x02;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;
pub const ENC_RETURN_CORRECTED: i32 = 0x08;
pub const ENC_RETURN_INVALIDINPUT: i32 = 0x10;
pub const ENC_RETURN_MEMOVERFLOWFOUND: i32 = 0x20;
pub const ENC_RETURN_VLCOVERFLOWFOUND: i32 = 0x40;
pub const ENC_RETURN_KNOWN_ISSUE: i32 = 0x80;

// `wels_common_defs.h:275-285`. MB_TYPE_INTRA_BL and MB_TYPE_SKIP were both wrong
// here: 0x04 is MB_TYPE_INTRA8x8 and 0x80 is MB_TYPE_8x8_REF0. Both are live, in
// IS_SVC_INTER and IS_SKIP below.
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_SKIP: u32 = 0x00000100;
pub const MB_TYPE_INTRA_BL: u32 = 0x00000400;

pub const g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37,
    37, 38, 38, 38, 39, 39, 39, 39,
];

pub const g_kiQpCostTable: [i32; 52] = [
    1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1,
    1, 1, 1, 1, 2, 2, 2, 2,
    3, 3, 3, 4, 4, 4, 5, 6,
    6, 7, 8, 9, 10, 11, 13, 14,
    16, 18, 20, 23, 25, 29, 32, 36,
    40, 45, 51, 57, 64, 72, 81, 91,
];

// `g_kuiGolombUELength` is a common-layer table (`common_tables.cpp:886`).
// This module used to declare its own copy; see the canonical definition for
// what the divergent copies got wrong.
pub use crate::common::wels_common_defs::g_kuiGolombUELength;

#[inline]
pub fn CLIP3_QP_0_51(x: i32) -> usize {
    if x < 0 {
        0
    } else if x > 51 {
        51
    } else {
        x as usize
    }
}

#[inline]
pub fn WELS_CLIP3<T: Ord + Copy>(x: T, min_val: T, max_val: T) -> T {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline]
pub fn JUMPPACKETSIZE_CONSTRAINT(max_byte: u32) -> u32 {
    if max_byte >= AVER_MARGIN_BYTES {
        max_byte - AVER_MARGIN_BYTES
    } else {
        0
    }
}

#[inline]
pub fn JUMPPACKETSIZE_JUDGE(len: u32, _mb_idx: i32, max_byte: u32) -> bool {
    len > JUMPPACKETSIZE_CONSTRAINT(max_byte)
}

#[inline]
pub fn IS_INTER(mb_type: u32) -> bool {
    (mb_type & (MB_TYPE_16x16 | MB_TYPE_16x8 | MB_TYPE_8x16 | MB_TYPE_8x8 | MB_TYPE_SKIP)) != 0
}

#[inline]
pub fn IS_SKIP(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_SKIP) != 0
}

#[inline]
pub fn IS_I_BL(mb_type: u32) -> bool {
    (mb_type & MB_TYPE_INTRA_BL) != 0
}

#[inline]
pub fn WELS_CEILLOG2(v: u32) -> i32 {
    if v <= 1 {
        0
    } else {
        32 - (v - 1).leading_zeros() as i32
    }
}

// ============================================================================
// Core Macroblock, Cache, and Slice Data Structures
// ============================================================================













#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeader {
    pub iFirstMbInSlice: i32,
    pub iFrameNum: i32,
    pub iPicOrderCntLsb: i32,
    pub eSliceType: EWelsSliceType,
    pub uiNumRefIdxL0Active: u8,
    pub uiRefCount: u8,
    pub uiRefIndex: u8,
    pub iSliceQpDelta: i8,
    pub uiDisableDeblockingFilterIdc: u8,
    pub iSliceAlphaC0Offset: i8,
    pub iSliceBetaOffset: i8,
    pub pSps: *mut SWelsSPS,
    pub pPps: *mut SWelsPPS,
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub uiIdrPicId: u16,
    pub bNumRefIdxActiveOverrideFlag: bool,
    pub uiPadding1Bytes: u8,
    pub sRefMarking: SRefPicMarking,
    pub sRefReordering: SRefPicListReorderSyntax,
}

impl Default for SSliceHeader {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    pub pSubsetSps: *mut SSubsetSps,
    pub uiNumMbsInSlice: u32,
    pub bStoreRefBasePicFlag: bool,
    pub bConstrainedIntraResamplingFlag: bool,
    pub bSliceSkipFlag: bool,
    pub bAdaptiveBaseModeFlag: bool,
    pub bDefaultBaseModeFlag: bool,
    pub bAdaptiveMotionPredFlag: bool,
    pub bDefaultMotionPredFlag: bool,
    pub bAdaptiveResidualPredFlag: bool,
    pub bDefaultResidualPredFlag: bool,
    pub bTcoeffLevelPredFlag: bool,
    pub uiDisableInterLayerDeblockingFilterIdc: u8,
}

impl Default for SSliceHeaderExt {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::common::wels_common_defs::{EWelsNalUnitType, SBitStringAux};
pub use crate::encoder::set_mb_syn_cabac::SCabacCtx;
use crate::encoder::paraset_strategy::IWelsParametersetStrategy;
use crate::encoder::svc_motion_estimate::SFeatureSearchPreparation;


/// `TagSlice` — `codec/encoder/core/inc/slice.h:170`. 1584 bytes.
#[repr(C)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
    pub pSliceBsa: *mut SBitStringAux,
    pub sSliceBs: SWelsSliceBs,
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub sMvStartMin: SMVUnitXY,
    pub sMvStartMax: SMVUnitXY,
    pub sMvc: [SMVUnitXY; 5],
    pub uiMvcNum: u8,
    pub sScaleShift: u8,
    pub iSliceIdx: i32,
    pub uiBufferIdx: u32,
    pub bSliceHeaderExtFlag: bool,
    pub uiLastMbQp: u8,
    pub bDynamicSlicingSliceSizeCtrlFlag: bool,
    pub uiAssumeLog2BytePerMb: u8,
    pub uiSliceFMECostDown: u32,
    pub uiReservedFillByte: u8,
    pub sCabacCtx: SCabacCtx,
    pub iCabacInitIdc: i32,
    pub iMbSkipRun: i32,
    pub iCountMbNumInSlice: i32,
    pub uiSliceConsumeTime: u32,
    pub iSliceComplexRatio: i32,
    pub sSlicingOverRc: SRCSlicing,
}

impl Default for SSlice {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}



#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDynamicSlicingStack {
    pub iStartPos: i32,
    pub iCurrentPos: i32,
    pub pBsStackBufPtr: *mut u8,
    pub uiBsStackCurBits: u32,
    pub iBsStackLeftBits: i32,
    pub sStoredCabac: crate::encoder::set_mb_syn_cabac::SCabacCtx,
    pub iMbSkipRunStack: i32,
    pub uiLastMbQp: u8,
    pub pRestoreBuffer: *mut u8,
}

impl Default for SDynamicSlicingStack {
    fn default() -> Self {
        Self {
            iStartPos: 0,
            iCurrentPos: 0,
            pBsStackBufPtr: std::ptr::null_mut(),
            uiBsStackCurBits: 0,
            iBsStackLeftBits: 0,
            sStoredCabac: crate::encoder::set_mb_syn_cabac::SCabacCtx::default(),
            iMbSkipRunStack: 0,
            uiLastMbQp: 0,
            pRestoreBuffer: std::ptr::null_mut(),
        }
    }
}



#[repr(C)]
/// `TagSliceBufferInfo` — `codec/encoder/core/inc/svc_enc_frame.h:71`. 16 bytes.
pub struct SSliceBufferInfo {
    pub pSliceBuffer: *mut SSlice,
    pub iMaxSliceNum: i32,
    pub iCodedSliceNum: i32,
}

impl Default for SSliceBufferInfo {
    fn default() -> Self {
        Self {
            iMaxSliceNum: 0,
            iCodedSliceNum: 0,
            pSliceBuffer: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
/// `TagLayerInfo` — `codec/encoder/core/inc/svc_enc_frame.h:77`. 48 bytes.
/// Field order follows C++: pSubsetSpsP precedes pSpsP and pPpsP.
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub pSubsetSpsP: *mut SSubsetSps,
    pub pSpsP: *mut SWelsSPS,
    pub pPpsP: *mut SWelsPPS,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::encoder::encoder_context::SPicture;

/// `TagDqLayer` — `codec/encoder/core/inc/svc_enc_frame.h:84`. 512 bytes.
///
/// This was previously spread over eleven partial copies. The least-truncated one
/// (`slice_multi_threading.rs`) still stood `sSliceBufferInfo` in for
/// `[SSliceBufferInfo; MAX_THREADS_NUM]` with `[u8; 64 * MAX_THREADS_NUM]` — four
/// times the real 64 bytes — and left four pointers as `*mut c_void`.
#[repr(C)]
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
    pub sSliceBufferInfo: [SSliceBufferInfo; MAX_THREADS_NUM],
    pub ppSliceInLayer: *mut *mut SSlice,
    pub sSliceEncCtx: SSliceCtx,
    pub pCsData: [*mut u8; 3],
    pub iCsStride: [i32; 3],

    pub pEncData: [*mut u8; 3],
    pub iEncStride: [i32; 3],

    pub sMbDataP: *mut SMB,
    pub iMbWidth: i16,
    pub iMbHeight: i16,

    pub bBaseLayerAvailableFlag: bool,
    pub bSatdInMdFlag: bool,

    pub iLoopFilterDisableIdc: u8,
    pub iLoopFilterAlphaC0Offset: i8,
    pub iLoopFilterBetaOffset: i8,
    pub uiDisableInterLayerDeblockingFilterIdc: u8,
    pub iInterLayerSliceAlphaC0Offset: i8,
    pub iInterLayerSliceBetaOffset: i8,
    pub bDeblockingParallelFlag: bool,

    pub pRefPic: *mut SPicture,
    pub pDecPic: *mut SPicture,
    pub pRefOri: [*mut SPicture; MAX_REF_PIC_COUNT as usize],

    pub bThreadSlcBufferFlag: bool,
    pub bSliceBsBufferFlag: bool,
    pub iMaxSliceNum: i32,
    pub NumSliceCodedOfPartition: [i32; MAX_THREADS_NUM],
    pub LastCodedMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub FirstMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub EndMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub pFirstMbIdxOfSlice: *mut i32,
    pub pCountMbNumInSlice: *mut i32,

    pub bNeedAdjustingSlicing: bool,

    pub pFeatureSearchPreparation: *mut SFeatureSearchPreparation,

    pub pRefLayer: *mut SDqLayer,
}

impl Default for SDqLayer {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::encoder::nal_encap::SWelsNalRaw;

#[repr(C)]
pub struct SWelsOut {
    pub sBsWrite: SBitStringAux,
    pub sNalList: *mut SWelsNalRaw,
    pub pNalLen: *mut i32,
    pub iCountNals: i32,
}

impl Default for SWelsOut {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::encoder::encoder_context::{SWelsFuncPtrList, SWelsRcFunc};

pub use crate::encoder::rc::SWelsSvcRc;
pub use crate::encoder::wels_encoder_ext::SSpatialLayerInternal;



pub use crate::encoder::encoder_context::sWelsEncCtx;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::encoder_context::SMVComponentUnit;
pub use crate::encoder::nal_encap::SNalUnitHeaderExt;
pub use crate::encoder::nal_encap::SNalUnitHeader;
pub use crate::encoder::nal_encap::SWelsSliceBs;
pub use crate::encoder::param_svc::SWelsSPS;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::param_svc::SSubsetSps;
pub use crate::encoder::param_svc::SSpsSvcExt;
pub use crate::encoder::ref_list_mgr_svc::SMmcoRef;
pub use crate::encoder::ref_list_mgr_svc::SReorderingSyntax;
pub use crate::encoder::ref_list_mgr_svc::SRefPicMarking;
pub use crate::encoder::ref_list_mgr_svc::SRefPicListReorderSyntax;
pub use crate::encoder::rc::SRCSlicing;
pub use crate::encoder::md::SWelsMD;
pub use crate::encoder::slice_multi_threading::SSliceThreading;
pub use crate::encoder::slice_multi_threading::SSliceCtx;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::md::SMB;

// Function pointer dispatch table types
pub type PWelsCodingSliceFunc = unsafe extern "C" fn(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32;
pub type PWelsSliceHeaderWriteFunc = unsafe extern "C" fn(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
);

// ============================================================================
// Bitstream Helper Functions
// ============================================================================

// One writer family, `vlc_encoder.rs`'s, which is the transliteration of the C++
// `codec/common/inc/golomb_common.h`. This module used to declare its own copy of
// the five functions below, and it was the one copy of the four that **diverged**
// (`phase0_findings.md` F2's fourth row). Four divergences died with it, all of
// them defensive additions the C++ does not have:
//
//   * `pBs.is_null() || iLen <= 0` early-returns, where the canonical would
//     dereference null or shift by a non-positive amount. Every call site here
//     writes a length known positive at the call: `uiLog2MaxFrameNum` and
//     `iLog2MaxPocLsb` are both at least 4, the two literals are 4, and the
//     Exp-Golomb lengths are at least 1 by construction.
//   * a **pre-mask** of the value to `iLen` bits, which the canonical does not
//     do: the canonical ORs the value into the accumulator whole. This is the
//     divergence with real teeth — a `iFrameNum` or `iPicOrderCntLsb` carrying
//     bits above its field width would be truncated by the old copy here and
//     would corrupt the neighbouring syntax elements under the canonical. The
//     encoder keeps both counters reduced modulo their field width, and the
//     sweeps are the referee for that claim: 341/341 both profiles, every RC
//     mode, GOP length and slice mode, 18-20 frames each, so the wrap is
//     exercised rather than assumed.
//   * an inverted branch sense (`iLeftBits >= iLen` with a flush-when-empty tail,
//     against the canonical's `iLen < iLeftBits`). The two converge on the
//     `iLen == iLeftBits` boundary — both end with the word stored, `uiCurBits`
//     zero and `iLeftBits` 32 — which F2 checked by hand and this deletes.
//   * `wrapping_add(1)` in `BsWriteUE` and `& 1` in `BsWriteOneBit`, where the
//     canonical adds and passes plainly. `u32::MAX` never reaches `BsWriteUE`,
//     and every `BsWriteOneBit` here passes a literal 0/1 or a `bool` cast.
//
// F5 (`phase1_findings.md`) is deliberately NOT fixed here: the canonical writer
// still shifts a full accumulator by 32 on a 32-bit write and still panics in a
// debug build, and a Phase 1 differential test pins that. Arithmetic parity, not
// repair (S6).
pub use crate::encoder::vlc_encoder::{
    BsGetBitsPos, BsWriteBits, BsWriteOneBit, BsWriteSE, BsWriteUE,
};

// ============================================================================
// Macroblock Topology & Cache Operations
// ============================================================================

/// Copies non-zero coefficient counts from `SMB` into the slice's `SMbCache`.
#[inline]
pub unsafe fn UpdateNonZeroCountCache(pMb: *mut SMB, pMbCache: *mut SMbCache) {
    if pMb.is_null() || pMbCache.is_null() {
        return;
    }
    let mb_nz = (*pMb).pNonZeroCount;
    if mb_nz.is_null() {
        return;
    }
    let cache_nz = (*pMbCache).iNonZeroCoeffCount.as_mut_ptr();

    std::ptr::copy_nonoverlapping(mb_nz.add(0), cache_nz.add(9), 4);
    std::ptr::copy_nonoverlapping(mb_nz.add(4), cache_nz.add(17), 4);
    std::ptr::copy_nonoverlapping(mb_nz.add(8), cache_nz.add(25), 4);
    std::ptr::copy_nonoverlapping(mb_nz.add(12), cache_nz.add(33), 4);

    std::ptr::copy_nonoverlapping(mb_nz.add(16), cache_nz.add(14), 2);
    std::ptr::copy_nonoverlapping(mb_nz.add(18), cache_nz.add(38), 2);
    std::ptr::copy_nonoverlapping(mb_nz.add(20), cache_nz.add(22), 2);
    std::ptr::copy_nonoverlapping(mb_nz.add(22), cache_nz.add(46), 2);
}

/// Computes the virtual slice identifier `uiSliceIdc` for a given macroblock linear index.
#[inline]
pub unsafe fn WelsMbToSliceIdc(pCurDq: *mut SDqLayer, kiMbXY: i32) -> u16 {
    if pCurDq.is_null() {
        return u16::MAX;
    }
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    if kiMbXY >= 0 && kiMbXY < (*pSliceCtx).iMbNumInFrame && !(*pSliceCtx).pOverallMbMap.is_null() {
        *(*pSliceCtx).pOverallMbMap.add(kiMbXY as usize)
    } else {
        u16::MAX
    }
}

/// Evaluates spatial neighbor availability masks for intra prediction and motion vector prediction.
pub unsafe fn UpdateMbNeighbor(pCurDq: *mut SDqLayer, pMb: *mut SMB, kiMbWidth: i32, uiSliceIdc: u16) {
    if pCurDq.is_null() || pMb.is_null() {
        return;
    }
    let mut uiNeighborAvailFlag: u32 = 0;
    let kiMbXY = (*pMb).iMbXY;
    let kiMbX = (*pMb).iMbX as i32;
    let kiMbY = (*pMb).iMbY as i32;

    (*pMb).uiSliceIdc = uiSliceIdc;
    let iLeftXY = kiMbXY - 1;
    let iTopXY = kiMbXY - kiMbWidth;
    let iLeftTopXY = iTopXY - 1;
    let iRightTopXY = iTopXY + 1;

    let bLeft = (kiMbX > 0) && (uiSliceIdc == WelsMbToSliceIdc(pCurDq, iLeftXY));
    let bTop = (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(pCurDq, iTopXY));
    let bLeftTop = (kiMbX > 0) && (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(pCurDq, iLeftTopXY));
    let bRightTop = (kiMbX < (kiMbWidth - 1)) && (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(pCurDq, iRightTopXY));

    if bLeft {
        uiNeighborAvailFlag |= LEFT_MB_POS as u32;
    }
    if bTop {
        uiNeighborAvailFlag |= TOP_MB_POS as u32;
    }
    if bLeftTop {
        uiNeighborAvailFlag |= TOPLEFT_MB_POS as u32;
    }
    if bRightTop {
        uiNeighborAvailFlag |= TOPRIGHT_MB_POS as u32;
    }

    (*pMb).uiNeighborAvail = uiNeighborAvailFlag as u8;
}

/// Updates neighbor availability information across dynamic slicing boundaries.
pub unsafe fn UpdateMbNeighbourInfoForNextSlice(
    pCurDq: *mut SDqLayer,
    pMbList: *mut SMB,
    kiFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
) {
    if pCurDq.is_null() || pMbList.is_null() {
        return;
    }
    let kiMbWidth = (*pCurDq).sSliceEncCtx.iMbWidth as i32;
    let mut iIdx = kiFirstMbIdxOfNextSlice;
    let iNextSliceFirstMbIdxRowStart = if (kiFirstMbIdxOfNextSlice % kiMbWidth) != 0 { 1 } else { 0 };
    let iCountMbUpdate = kiMbWidth + iNextSliceFirstMbIdxRowStart;
    let kiEndMbNeedUpdate = kiFirstMbIdxOfNextSlice + iCountMbUpdate;

    // C++ is a do-while: the first macroblock is always updated, even when
    // `kiFirstMbIdxOfNextSlice > kiLastMbIdxInPartition` -- which happens when the
    // boundary lands on the last macroblock of a partition. A `while` skips it.
    let mut pMb = pMbList.add(iIdx as usize);
    loop {
        UpdateMbNeighbor(pCurDq, pMb, kiMbWidth, WelsMbToSliceIdc(pCurDq, (*pMb).iMbXY));
        pMb = pMb.add(1);
        iIdx += 1;
        if !((iIdx < kiEndMbNeedUpdate) && (iIdx <= kiLastMbIdxInPartition)) {
            break;
        }
    }
}

// ============================================================================
// Slice Header Initialization & Serialization
// ============================================================================

pub unsafe fn WelsSliceHeaderScalExtInit(pCurLayer: *mut SDqLayer, pSlice: *mut SSlice) {
    if pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let pNalHeadExt = &mut (*pCurLayer).sLayerInfo.sNalHeaderExt;

    pSliceHeadExt.bSliceSkipFlag = false;

    if pNalHeadExt.uiDependencyId > 0 {
        pSliceHeadExt.bAdaptiveBaseModeFlag = false;
        pSliceHeadExt.bAdaptiveMotionPredFlag = false;
        pSliceHeadExt.bAdaptiveResidualPredFlag = false;

        pSliceHeadExt.bDefaultBaseModeFlag = false;
        pSliceHeadExt.bDefaultMotionPredFlag = false;
        pSliceHeadExt.bDefaultResidualPredFlag = false;
    }
}

pub unsafe fn WelsSliceHeaderExtInit(pEncCtx: *mut sWelsEncCtx, pCurLayer: *mut SDqLayer, pSlice: *mut SSlice) {
    if pEncCtx.is_null() || pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    let pCurSliceExt = &mut (*pSlice).sSliceHeaderExt;
    let pCurSliceHeader = &mut pCurSliceExt.sSliceHeader;
    let uiDid = (*pEncCtx).uiDependencyId as usize;

    pCurSliceHeader.eSliceType = (*pEncCtx).eSliceType;
    pCurSliceExt.bStoreRefBasePicFlag = false;

    // svc_encode_slice.cpp:97-98. Both of these were missing: `iFrameNum` stayed 0 for
    // every frame, and `uiIdrPicId` stayed 0 where WriteSsvcParaset has already
    // incremented the layer's counter to 1, so the IDR slice header wrote ue(0) (1 bit)
    // where the C++ writes ue(1) (3 bits) -- the 2-bit offset that shifted the whole
    // slice payload.
    let pParamInternal = &(*(*pEncCtx).pSvcParam).sDependencyLayers[uiDid];
    pCurSliceHeader.iFrameNum = pParamInternal.iFrameNum;
    pCurSliceHeader.uiIdrPicId = pParamInternal.uiIdrPicId;

    if !(*pEncCtx).pEncPic.is_null() {
        pCurSliceHeader.iPicOrderCntLsb = (*(*pEncCtx).pEncPic).iFramePoc;
    }

    if (*pEncCtx).eSliceType == EWelsSliceType::P_SLICE {
        pCurSliceHeader.uiNumRefIdxL0Active = 1;
        let num_ref = if !(*pCurLayer).sLayerInfo.pSpsP.is_null() {
            (*(*pCurLayer).sLayerInfo.pSpsP).iNumRefFrames
        } else {
            1
        };
        if pCurSliceHeader.uiRefCount > 0 && (pCurSliceHeader.uiRefCount as i32) <= num_ref as i32 {
            pCurSliceHeader.bNumRefIdxActiveOverrideFlag = true;
            pCurSliceHeader.uiNumRefIdxL0Active = pCurSliceHeader.uiRefCount;
        } else {
            pCurSliceHeader.bNumRefIdxActiveOverrideFlag = false;
        }
    }

    let pic_init_qp = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).iPicInitQp
    } else {
        26
    };
    pCurSliceHeader.iSliceQpDelta = ((*pEncCtx).iGlobalQp - pic_init_qp as i32) as i8;

    pCurSliceHeader.uiDisableDeblockingFilterIdc = (*pCurLayer).iLoopFilterDisableIdc;
    pCurSliceHeader.iSliceAlphaC0Offset = (*pCurLayer).iLoopFilterAlphaC0Offset;
    pCurSliceHeader.iSliceBetaOffset = (*pCurLayer).iLoopFilterBetaOffset;
    pCurSliceExt.uiDisableInterLayerDeblockingFilterIdc = (*pCurLayer).uiDisableInterLayerDeblockingFilterIdc;

    if (*pSlice).bSliceHeaderExtFlag {
        WelsSliceHeaderScalExtInit(pCurLayer, pSlice);
    } else {
        pCurSliceExt.bAdaptiveBaseModeFlag = false;
        pCurSliceExt.bAdaptiveMotionPredFlag = false;
        pCurSliceExt.bAdaptiveResidualPredFlag = false;
        pCurSliceExt.bDefaultBaseModeFlag = false;
        pCurSliceExt.bDefaultMotionPredFlag = false;
        pCurSliceExt.bDefaultResidualPredFlag = false;
    }
}

pub unsafe fn WriteReferenceReorder(pBs: *mut SBitStringAux, sSliceHeader: *mut SSliceHeader) {
    if pBs.is_null() || sSliceHeader.is_null() {
        return;
    }
    let pRefOrdering = &mut (*sSliceHeader).sRefReordering;
    let eSliceType = (*sSliceHeader).eSliceType;

    if eSliceType != EWelsSliceType::I_SLICE && eSliceType != EWelsSliceType::SI_SLICE {
        BsWriteOneBit(pBs, 1);
        let mut n: usize = 0;
        loop {
            let uiReorderingOfPicNumsIdc = pRefOrdering.SReorderingSyntax[n].uiReorderingOfPicNumsIdc;
            BsWriteUE(pBs, uiReorderingOfPicNumsIdc as u32);
            if uiReorderingOfPicNumsIdc == 0 || uiReorderingOfPicNumsIdc == 1 {
                BsWriteUE(pBs, pRefOrdering.SReorderingSyntax[n].uiAbsDiffPicNumMinus1);
            } else if uiReorderingOfPicNumsIdc == 2 {
                BsWriteUE(pBs, pRefOrdering.SReorderingSyntax[n].iLongTermPicNum as u32);
            }
            n += 1;
            if uiReorderingOfPicNumsIdc == 3 || n >= 32 {
                break;
            }
        }
    }
}

pub unsafe fn WriteRefPicMarking(pBs: *mut SBitStringAux, pSliceHeader: *mut SSliceHeader, pNalHdrExt: *mut SNalUnitHeaderExt) {
    if pBs.is_null() || pSliceHeader.is_null() || pNalHdrExt.is_null() {
        return;
    }
    let sRefMarking = &mut (*pSliceHeader).sRefMarking;
    let mut n: usize = 0;

    if (*pNalHdrExt).bIdrFlag {
        BsWriteOneBit(pBs, if sRefMarking.bNoOutputOfPriorPicsFlag { 1 } else { 0 });
        BsWriteOneBit(pBs, if sRefMarking.bLongTermRefFlag { 1 } else { 0 });
    } else {
        BsWriteOneBit(pBs, if sRefMarking.bAdaptiveRefPicMarkingModeFlag { 1 } else { 0 });
        if sRefMarking.bAdaptiveRefPicMarkingModeFlag {
            loop {
                let iMmcoType = sRefMarking.SMmcoRef[n].iMmcoType;
                BsWriteUE(pBs, iMmcoType as u32);
                if iMmcoType == 1 || iMmcoType == 3 {
                    BsWriteUE(pBs, (sRefMarking.SMmcoRef[n].iDiffOfPicNum - 1) as u32);
                }
                if iMmcoType == 2 {
                    BsWriteUE(pBs, sRefMarking.SMmcoRef[n].iLongTermPicNum as u32);
                }
                if iMmcoType == 3 || iMmcoType == 6 {
                    BsWriteUE(pBs, sRefMarking.SMmcoRef[n].iLongTermFrameIdx as u32);
                }
                if iMmcoType == 4 {
                    BsWriteUE(pBs, (sRefMarking.SMmcoRef[n].iMaxLongTermFrameIdx + 1) as u32);
                }
                n += 1;
                if iMmcoType == 0 || n >= 32 {
                    break;
                }
            }
        }
    }
}

pub unsafe fn WelsSliceHeaderWrite(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
) {
    if pBs.is_null() || pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    let pSps = (*pCurLayer).sLayerInfo.pSpsP;
    let pPps = (*pCurLayer).sLayerInfo.pPpsP;
    let pSliceHeader = &mut (*pSlice).sSliceHeaderExt.sSliceHeader;
    let pNalHead = &mut (*pCurLayer).sLayerInfo.sNalHeaderExt;

    BsWriteUE(pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy. C++ dereferences both pointers
    // unconditionally; the null guards here follow the surrounding style in this port.
    let pps_id = if !pPps.is_null() { (*pPps).iPpsId } else { 0 };
    let iPpsIdOffset = if !pParametersetStrategy.is_null() {
        IWelsParametersetStrategy::GetPpsIdOffset(pParametersetStrategy, pps_id as i32)
    } else {
        0
    };
    BsWriteUE(pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = if !pSps.is_null() { (*pSps).uiLog2MaxFrameNum } else { 4 };
    BsWriteBits(pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if (*pNalHead).bIdrFlag {
        BsWriteUE(pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if !pSps.is_null() && (*pSps).uiPocType == 0 {
        BsWriteBits(pBs, (*pSps).iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(pBs, active);
        }
    }

    if !(*pNalHead).bIdrFlag {
        WriteReferenceReorder(pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(pBs, pSliceHeader, pNalHead);
    }

    if !pPps.is_null() && (*pPps).bEntropyCodingModeFlag && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if !pPps.is_null() && (*pPps).bDeblockingFilterControlPresentFlag {
        match (*pSliceHeader).uiDisableDeblockingFilterIdc {
            0 | 3 | 4 | 6 => {
                BsWriteUE(pBs, 0);
            }
            1 => {
                BsWriteUE(pBs, 1);
            }
            2 | 5 => {
                BsWriteUE(pBs, 2);
            }
            _ => {
                BsWriteUE(pBs, 0);
            }
        }
        if (*pSliceHeader).uiDisableDeblockingFilterIdc != 1 {
            BsWriteSE(pBs, ((*pSliceHeader).iSliceAlphaC0Offset as i32) >> 1);
            BsWriteSE(pBs, ((*pSliceHeader).iSliceBetaOffset as i32) >> 1);
        }
    }
}

pub unsafe fn WelsSliceHeaderExtWrite(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
) {
    if pBs.is_null() || pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    let pSps = (*pCurLayer).sLayerInfo.pSpsP;
    let pPps = (*pCurLayer).sLayerInfo.pPpsP;
    let pSubSps = (*pCurLayer).sLayerInfo.pSubsetSpsP;
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let pSliceHeader = &mut pSliceHeadExt.sSliceHeader;
    let pNalHead = &mut (*pCurLayer).sLayerInfo.sNalHeaderExt;

    BsWriteUE(pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy. C++ dereferences both pointers
    // unconditionally; the null guards here follow the surrounding style in this port.
    let pps_id = if !pPps.is_null() { (*pPps).iPpsId } else { 0 };
    let iPpsIdOffset = if !pParametersetStrategy.is_null() {
        IWelsParametersetStrategy::GetPpsIdOffset(pParametersetStrategy, pps_id as i32)
    } else {
        0
    };
    BsWriteUE(pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = if !pSps.is_null() { (*pSps).uiLog2MaxFrameNum } else { 4 };
    BsWriteBits(pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if (*pNalHead).bIdrFlag {
        BsWriteUE(pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if !pSps.is_null() && (*pSps).uiPocType == 0 {
        BsWriteBits(pBs, (*pSps).iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(pBs, active);
        }
    }

    if !(*pNalHead).bIdrFlag {
        WriteReferenceReorder(pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(pBs, pSliceHeader, pNalHead);
        if !pSubSps.is_null() && !(*pSubSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
            BsWriteOneBit(pBs, if pSliceHeadExt.bStoreRefBasePicFlag { 1 } else { 0 });
        }
    }

    if !pPps.is_null() && (*pPps).bEntropyCodingModeFlag && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if !pPps.is_null() && (*pPps).bDeblockingFilterControlPresentFlag {
        BsWriteUE(pBs, (*pSliceHeader).uiDisableDeblockingFilterIdc as u32);
        if (*pSliceHeader).uiDisableDeblockingFilterIdc != 1 {
            BsWriteSE(pBs, ((*pSliceHeader).iSliceAlphaC0Offset as i32) >> 1);
            BsWriteSE(pBs, ((*pSliceHeader).iSliceBetaOffset as i32) >> 1);
        }
    }

    if !pSubSps.is_null() && !(*pSubSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
        BsWriteBits(pBs, 4, 0);
        BsWriteBits(pBs, 4, 15);
    }
}

// ============================================================================
// Macroblock Residual & Chroma Reconstruction
// ============================================================================

// `WelsInterMbEncode` lives in `svc_mode_decision.rs`, which is where the C++
// has it (svc_mode_decision.cpp) and where all three call sites resolve. A
// truncated second copy used to sit here: it did the DCT and dropped
// quantisation and reconstruction entirely. It was dead, but one unqualified
// call in this file would have silently activated it.

pub unsafe fn WelsIMbChromaEncode(pEncCtx: *mut sWelsEncCtx, pCurMb: *mut SMB, pMbCache: *mut SMbCache) {
    if pEncCtx.is_null() || pCurMb.is_null() || pMbCache.is_null() {
        return;
    }
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let kiCsStride = (*pCurLayer).iCsStride[1];
    let pCurRS = (*pMbCache).pCoeffLevel;
    let pBestPred = (*pMbCache).pBestPredIntraChroma;
    let pCsCb = (*pMbCache).SPicData.pCsMb[1];
    let pCsCr = (*pMbCache).SPicData.pCsMb[2];

    // This previously ran both DCTs and then both IDCTs, omitting the two
    // `WelsEncRecUV` calls between them. That is the quantise / zigzag /
    // non-zero-count / chroma-CBP step: without it `pCurRS` reached the IDCT holding
    // raw DCT coefficients, `pCurMb->uiCbp` never got its chroma bits and
    // `pNonZeroCount[16..24]` stayed zero, so no chroma residual was ever coded.
    let pFunc = (*pEncCtx).pFuncList;
    let pfDctFourT4 = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");
    let pfIDctFourT4 = (*pFunc).pfIDctFourT4.expect("pfIDctFourT4 unset");

    //cb
    pfDctFourT4(pCurRS, (*pMbCache).SPicData.pEncMb[1], kiEncStride, pBestPred, 8);
    crate::encoder::svc_encode_mb::WelsEncRecUV(pFunc, pCurMb, pMbCache, pCurRS, 1);
    pfIDctFourT4(pCsCb, kiCsStride, pBestPred, 8, pCurRS);

    //cr
    pfDctFourT4(
        pCurRS.add(64),
        (*pMbCache).SPicData.pEncMb[2],
        kiEncStride,
        pBestPred.add(64),
        8,
    );
    crate::encoder::svc_encode_mb::WelsEncRecUV(pFunc, pCurMb, pMbCache, pCurRS.add(64), 2);
    pfIDctFourT4(pCsCr, kiCsStride, pBestPred.add(64), 8, pCurRS.add(64));
}

pub unsafe fn WelsPMbChromaEncode(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: *mut SMB) {
    if pEncCtx.is_null() || pSlice.is_null() || pCurMb.is_null() {
        return;
    }
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let pMbCache = &mut (*pSlice).sMbCacheInfo as *mut SMbCache;
    let pCurRS = (*pMbCache).pCoeffLevel.add(256);
    let pBestPred = (*pMbCache).pMemPredChroma;

    let pFunc = (*pEncCtx).pFuncList;
    let dct = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");
    dct(pCurRS, (*pMbCache).SPicData.pEncMb[1], kiEncStride, pBestPred, 8);
    dct(pCurRS.add(64), (*pMbCache).SPicData.pEncMb[2], kiEncStride, pBestPred.add(64), 8);

    // `svc_encode_slice.cpp:WelsPMbChromaEncode` quantises both chroma planes here.
    // Both calls were missing, so a P macroblock's chroma reached the reconstruction
    // holding raw DCT coefficients and never set its chroma CBP bits — the same
    // defect Phase 4.5 found in `WelsIMbChromaEncode`.
    crate::encoder::svc_encode_mb::WelsEncRecUV(pFunc, pCurMb, pMbCache, pCurRS, 1);
    crate::encoder::svc_encode_mb::WelsEncRecUV(pFunc, pCurMb, pMbCache, pCurRS.add(64), 2);
}

pub unsafe fn OutputPMbWithoutConstructCsRsNoCopy(pCtx: *mut sWelsEncCtx, pDq: *mut SDqLayer, pSlice: *mut SSlice, pMb: *mut SMB) {
    if pCtx.is_null() || pDq.is_null() || pSlice.is_null() || pMb.is_null() {
        return;
    }
    let mb_type = (*pMb).uiMbType;
    //intra have been reconstructed, NO COPY from CS to pDecPic--
    if (IS_INTER(mb_type) && !IS_SKIP(mb_type)) || IS_I_BL(mb_type) {
        let pMbCache = &mut (*pSlice).sMbCacheInfo;
        let pDecY = (*pMbCache).SPicData.pDecMb[0];
        let pDecU = (*pMbCache).SPicData.pDecMb[1];
        let pDecV = (*pMbCache).SPicData.pDecMb[2];
        let pScaledTcoeff = (*pMbCache).pCoeffLevel;
        let kiDecStrideLuma = (*(*pDq).pDecPic).iLineSize[0];
        let kiDecStrideChroma = (*(*pDq).pDecPic).iLineSize[1];
        let pfIdctFour4x4 = (*(*pCtx).pFuncList).pfIDctFourT4.expect("pfIDctFourT4 unset");

        // The luma half of this function was missing: no `pDecY`, no
        // `WelsIDctT4RecOnMb`. Every inter macroblock's luma residual was therefore
        // never added back into the reconstruction, so the encoder's reference frame
        // diverged from what a decoder produces from its own (correct) bitstream —
        // invisible until a second P frame referenced it.
        crate::encoder::decode_mb_aux::WelsIDctT4RecOnMb(
            pDecY,
            kiDecStrideLuma,
            pDecY,
            kiDecStrideLuma,
            pScaledTcoeff,
            pfIdctFour4x4,
        );
        pfIdctFour4x4(pDecU, kiDecStrideChroma, pDecU, kiDecStrideChroma, pScaledTcoeff.add(256));
        pfIdctFour4x4(pDecV, kiDecStrideChroma, pDecV, kiDecStrideChroma, pScaledTcoeff.add(320));
    }
}

pub unsafe fn UpdateQpForOverflow(pCurMb: *mut SMB, kuiChromaQpIndexOffset: u8) {
    if pCurMb.is_null() {
        return;
    }
    (*pCurMb).uiLumaQp = (*pCurMb).uiLumaQp.wrapping_add(DELTA_QP as u8);
    let clamped_idx = CLIP3_QP_0_51((*pCurMb).uiLumaQp as i32 + kuiChromaQpIndexOffset as i32);
    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[clamped_idx];
}

// ============================================================================
// Macroblock Search & Traversal Loops
// ============================================================================

pub unsafe fn WelsGetNextMbOfSlice(pCurDq: *mut SDqLayer, kiMbXY: i32) -> i32 {
    if pCurDq.is_null() {
        return -1;
    }
    let pSliceSeg = &mut (*pCurDq).sSliceEncCtx;
    if kiMbXY < 0 || kiMbXY >= pSliceSeg.iMbNumInFrame {
        return -1;
    }
    if pSliceSeg.uiSliceMode == SliceMode::SM_SINGLE_SLICE {
        let iNextMbIdx = kiMbXY + 1;
        if iNextMbIdx >= pSliceSeg.iMbNumInFrame {
            -1
        } else {
            iNextMbIdx
        }
    } else if pSliceSeg.uiSliceMode != SliceMode::SM_RESERVED {
        let iNextMbIdx = kiMbXY + 1;
        if iNextMbIdx < pSliceSeg.iMbNumInFrame
            && !pSliceSeg.pOverallMbMap.is_null()
            && *pSliceSeg.pOverallMbMap.add(iNextMbIdx as usize)
                == *pSliceSeg.pOverallMbMap.add(kiMbXY as usize)
        {
            iNextMbIdx
        } else {
            -1
        }
    } else {
        -1
    }
}

pub unsafe fn WelsInitInterMDStruc(
    pCurMb: *const SMB,
    pMvdCostTable: *mut u16,
    kiMvdInterTableStride: i32,
    pMd: *mut SWelsMD,
) {
    if pCurMb.is_null() || pMd.is_null() {
        return;
    }
    let luma_qp = (*pCurMb).uiLumaQp as usize;
    (*pMd).iLambda = g_kiQpCostTable[luma_qp];
    if !pMvdCostTable.is_null() {
        (*pMd).pMvdCost = pMvdCostTable.add(luma_qp * kiMvdInterTableStride as usize);
    }
    (*pMd).iMbPixX = ((*pCurMb).iMbX as i32) << 4;
    (*pMd).iMbPixY = ((*pCurMb).iMbY as i32) << 4;
    (*pMd).iBlock8x8StaticIdc.fill(0);
}

pub unsafe fn WelsISliceMdEnc(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    if pCurLayer.is_null() || (*pCurLayer).sMbDataP.is_null() || (*pCurLayer).iMbWidth <= 0 || (*pCurLayer).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pSliceHdExt = &mut (*pSlice).sSliceHeaderExt;
    let pMbList = (*pCurLayer).sMbDataP;
    let kiSliceFirstMbXY = pSliceHdExt.sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*(*pEncCtx).pSvcParam).iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }

    loop {
        if !kbCabac {
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfStashMBStatus {
                    func(&mut sDss, pSlice, 0);
                }
            }
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInit {
                func(pEncCtx as *mut _, pCurMb as *mut _, pSlice as *mut _);
            }
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            pMbCache,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            sMd.iLambda = g_kiQpCostTable[(*pCurMb).uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, pCurMb, pMbCache);
            UpdateNonZeroCountCache(pCurMb, pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfWelsSpatialWriteMbSyn {
                    iEncReturn = func(pEncCtx, pSlice, pCurMb);
                }
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                    if let Some(func) = func_list.pfStashPopMBStatus {
                        func(&mut sDss, pSlice);
                    }
                }
                UpdateQpForOverflow(pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                func(pCurLayer, pCurMb, (*pMbCache).bCollocatedPredFlag, I_SLICE);
            }
            if let Some(func) = func_list.pfRc.pfWelsRcMbInfoUpdate {
                func(pEncCtx as *mut _, pCurMb as *mut _, sMd.iCostLuma, pSlice as *mut _);
            }
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsISliceMdEncDynamic(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pBs = (*pSlice).pSliceBsa;
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pSliceCtx = &mut (*pCurLayer).sSliceEncCtx;
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pSliceHdExt = &mut (*pSlice).sSliceHeaderExt;
    let pMbList = (*pCurLayer).sMbDataP;
    let kiSliceFirstMbXY = pSliceHdExt.sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();
    if (*(*pEncCtx).pSvcParam).iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = (*pEncCtx).pDynamicBsBuffer[kiPartitionId];
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    } else {
        sDss.iStartPos = BsGetBitsPos(pBs);
    }

    loop {
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfStashMBStatus {
                func(&mut sDss, pSlice, 0);
            }
            if let Some(func) = func_list.pfRc.pfWelsRcMbInit {
                func(pEncCtx as *mut _, pCurMb as *mut _, pSlice as *mut _);
            }
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*(*pEncCtx).pWelsSvcRc.add((*pEncCtx).uiDependencyId as usize)).iMaxQp;
            (*pCurMb).uiLumaQp = max_qp as u8;
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            pMbCache,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            sMd.iLambda = g_kiQpCostTable[(*pCurMb).uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, pCurMb, pMbCache);
            UpdateNonZeroCountCache(pCurMb, pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfWelsSpatialWriteMbSyn {
                    iEncReturn = func(pEncCtx, pSlice, pCurMb);
                }
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                    if let Some(func) = func_list.pfStashPopMBStatus {
                        func(&mut sDss, pSlice);
                    }
                }
                UpdateQpForOverflow(pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfGetBsPosition {
                sDss.iCurrentPos = func(pSlice);
            }
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx as *mut c_void,
            pSlice as *mut c_void,
            pSliceCtx,
            pCurMb,
            &mut sDss,
        ) {
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfStashPopMBStatus {
                    func(&mut sDss, pSlice);
                }
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx - 1;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInfoUpdate {
                func(pEncCtx as *mut _, pCurMb as *mut _, sMd.iCostLuma, pSlice as *mut _);
            }
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            (*pSlice).iCountMbNumInSlice = iCurMbIdx - (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId];
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

/// Debug hook matching the `OH264_MBDUMP` block the C++ carries at the same point in
/// `WelsMdInterMbLoop`. Prints the per-macroblock mode-decision state so the two
/// encoders can be diffed line by line. Off unless `OH264_MBDUMP` is set.
///
/// # Safety
/// All three pointers must be valid, with `pCurMb`'s side arrays allocated.
unsafe fn mb_dump(pCurMb: *mut SMB, pMd: *const SWelsMD, pSlice: *const SSlice) {
    if !crate::encoder::dump_enabled(&MB_DUMP, "OH264_MBDUMP") {
        return;
    }
    let mut nzc = String::new();
    for di in 0..24 {
        nzc.push_str(&format!("{},", *(*pCurMb).pNonZeroCount.add(di)));
    }
    eprintln!(
        "MB {:3} type={:08x} cbp={:02x} qp={:2} cqp={:2} sub={},{},{},{} \
         cl={:7} cc={:7} skip={:7} sad={:7} mv={},{} ri={} nzc={} mv0={},{} skiprun={}",
        (*pCurMb).iMbXY,
        (*pCurMb).uiMbType,
        (*pCurMb).uiCbp,
        (*pCurMb).uiLumaQp,
        (*pCurMb).uiChromaQp,
        (*pCurMb).uiSubMbType[0],
        (*pCurMb).uiSubMbType[1],
        (*pCurMb).uiSubMbType[2],
        (*pCurMb).uiSubMbType[3],
        (*pMd).iCostLuma,
        (*pMd).iCostChroma,
        (*pMd).iCostSkipMb,
        *(*pCurMb).pSadCost.add(0),
        (*pCurMb).sP16x16Mv.iMvX,
        (*pCurMb).sP16x16Mv.iMvY,
        *(*pCurMb).pRefIndex.add(0),
        nzc,
        (*(*pCurMb).sMv.add(0)).iMvX,
        (*(*pCurMb).sMv.add(0)).iMvY,
        (*pSlice).iMbSkipRun,
    );
}

pub unsafe fn WelsMdInterMbLoop(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pWelsMd: *mut c_void,
    kiSliceFirstMbXY: i32,
) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() || pWelsMd.is_null() || (*pEncCtx).pCurDqLayer.is_null() || (*(*pEncCtx).pCurDqLayer).sMbDataP.is_null() || (*(*pEncCtx).pCurDqLayer).iMbWidth <= 0 || (*(*pEncCtx).pCurDqLayer).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd as *mut SWelsMD;
    let pBs = (*pSlice).pSliceBsa;
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pMbList = (*pCurLayer).sMbDataP;
    let mut iNumMbCoded = 0;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = if !(*pEncCtx).pMvdCostTable.is_null() {
        (*pEncCtx).pMvdCostTable.add((*pEncCtx).iMvdCostTableSize as usize)
    } else {
        std::ptr::null_mut()
    };
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*(*pEncCtx).pSvcParam).iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if !kbCabac {
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfStashMBStatus {
                    func(&mut sDss, pSlice, (*pSlice).iMbSkipRun);
                }
            }
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        //step(1): set QP for the current MB
        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInit {
                func(pEncCtx as *mut _, pCurMb as *mut _, pSlice as *mut _);
            }
        }

        //step (2). save some value for future use, initial pWelsMd
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            pCurMb,
            kiSliceFirstMbXY,
        );

        loop {
            WelsInitInterMDStruc(pCurMb, pMvdCostTable, kiMvdInterTableStride, pMd);
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, pSlice, pCurMb, pMbCache);
                }

                //step (4): save from the MD process for future use
                crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                    (*(*pCurLayer).pDecPic).uiRefMbType,
                    pMbCache,
                    pCurMb,
                    pMd,
                );

                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        pCurLayer,
                        pCurMb,
                        (*pMbCache).bCollocatedPredFlag,
                        if !(*pEncCtx).pRefPic.is_null() { (*(*pEncCtx).pRefPic).iPictureType } else { 0 },
                    );
                }
                mb_dump(pCurMb, pMd, pSlice);
            }
            //step (5): update cache
            UpdateNonZeroCountCache(pCurMb, pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfWelsSpatialWriteMbSyn {
                    iEncReturn = func(pEncCtx, pSlice, pCurMb);
                }
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                    if let Some(func) = func_list.pfStashPopMBStatus {
                        (*pSlice).iMbSkipRun = func(&mut sDss, pSlice);
                    }
                }
                UpdateQpForOverflow(pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer, pSlice, pCurMb);

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInfoUpdate {
                func(pEncCtx as *mut _, pCurMb as *mut _, (*pMd).iCostLuma, pSlice as *mut _);
            }
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        BsWriteUE(pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsMdInterMbLoopOverDynamicSlice(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pWelsMd: *mut c_void,
    kiSliceFirstMbXY: i32,
) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() || pWelsMd.is_null() || (*pEncCtx).pCurDqLayer.is_null() || (*(*pEncCtx).pCurDqLayer).sMbDataP.is_null() || (*(*pEncCtx).pCurDqLayer).iMbWidth <= 0 || (*(*pEncCtx).pCurDqLayer).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd as *mut SWelsMD;
    let pBs = (*pSlice).pSliceBsa;
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pSliceCtx = &mut (*pCurLayer).sSliceEncCtx;
    let pMbCache = &mut (*pSlice).sMbCacheInfo;
    let pMbList = (*pCurLayer).sMbDataP;
    let mut iNumMbCoded = 0;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = if !(*pEncCtx).pMvdCostTable.is_null() {
        (*pEncCtx).pMvdCostTable.add((*pEncCtx).iMvdCostTableSize as usize)
    } else {
        std::ptr::null_mut()
    };
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();
    if (*(*pEncCtx).pSvcParam).iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
        sDss.pRestoreBuffer = (*pEncCtx).pDynamicBsBuffer[kiPartitionId];
    } else {
        sDss.iStartPos = BsGetBitsPos(pBs);
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfStashMBStatus {
                func(&mut sDss, pSlice, (*pSlice).iMbSkipRun);
            }
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInit {
                func(pEncCtx as *mut _, pCurMb as *mut _, pSlice as *mut _);
            }
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*(*pEncCtx).pWelsSvcRc.add((*pEncCtx).uiDependencyId as usize)).iMaxQp;
            (*pCurMb).uiLumaQp = max_qp as u8;
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }

        // step (2): save some values for future use, initialise pWelsMd. Both of
        // these were missing: WelsMdInterInit is what installs the reference-block
        // pointers in pMbCache, so pfInterMd read a null pSample2.
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            pCurMb,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            WelsInitInterMDStruc(pCurMb, pMvdCostTable, kiMvdInterTableStride, pMd);
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, pSlice, pCurMb, pMbCache);
                }
            }
            // step (4): save from the MD process for future use
            crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                (*(*pCurLayer).pDecPic).uiRefMbType,
                pMbCache,
                pCurMb,
                pMd,
            );
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        pCurLayer,
                        pCurMb,
                        (*pMbCache).bCollocatedPredFlag,
                        if !(*pEncCtx).pRefPic.is_null() { (*(*pEncCtx).pRefPic).iPictureType } else { 0 },
                    );
                }
            }
            UpdateNonZeroCountCache(pCurMb, pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfWelsSpatialWriteMbSyn {
                    iEncReturn = func(pEncCtx, pSlice, pCurMb);
                }
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                    if let Some(func) = func_list.pfStashPopMBStatus {
                        (*pSlice).iMbSkipRun = func(&mut sDss, pSlice);
                    }
                }
                UpdateQpForOverflow(pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfGetBsPosition {
                sDss.iCurrentPos = func(pSlice);
            }
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx as *mut c_void,
            pSlice as *mut c_void,
            pSliceCtx,
            pCurMb,
            &mut sDss,
        ) {
            if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
                if let Some(func) = func_list.pfStashPopMBStatus {
                    (*pSlice).iMbSkipRun = func(&mut sDss, pSlice);
                }
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx - 1;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer, pSlice, pCurMb);

        if let Some(func_list) = (*pEncCtx).pFuncList.as_ref() {
            if let Some(func) = func_list.pfRc.pfWelsRcMbInfoUpdate {
                func(pEncCtx as *mut _, pCurMb as *mut _, (*pMd).iCostLuma, pSlice as *mut _);
            }
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        BsWriteUE(pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsPSliceMdEnc(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, kbIsHighestDlayerFlag: bool) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    // C++ leaves `SWelsMD sMd;` uninitialized and only `memset`s `sMd.sMe` when the
    // base layer is unavailable or this is not the highest spatial layer.
    // `Default::default()` zeroes the whole struct, which is that memset plus zeroes
    // for fields every path assigns before reading.
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:698`. This assignment was missing, so `bMdUsingSad` was
    // always false and every skip/refinement cost was taken from SATD where the gate
    // configuration (LOW_COMPLEXITY) costs with SAD.
    sMd.bMdUsingSad = (*(*pEncCtx).pSvcParam).iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoop(pEncCtx, pSlice, &mut sMd as *mut SWelsMD as *mut c_void, kiSliceFirstMbXY)
}

pub unsafe fn WelsPSliceMdEncDynamic(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, kbIsHighestDlayerFlag: bool) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:715`. The same assignment was already missing from
    // `WelsPSliceMdEnc` and fixed there; this twin still had the defect, so every
    // dynamic-slice P macroblock costed with SATD where LOW_COMPLEXITY costs with
    // SAD.
    sMd.bMdUsingSad = (*(*pEncCtx).pSvcParam).iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoopOverDynamicSlice(pEncCtx, pSlice, &mut sMd as *mut SWelsMD as *mut c_void, kiSliceFirstMbXY)
}

pub unsafe fn WelsCodePSlice(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let kbBaseAvail = (*pCurLayer).bBaseLayerAvailableFlag;
    let kbHighestSpatial = if !(*pEncCtx).pSvcParam.is_null() {
        (*(*pEncCtx).pSvcParam).iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    // `svc_encode_slice.cpp:733/736`. C++ picks `pfInterMd` per slice; the port never
    // assigned it at all, so every P macroblock ran with whatever the slot held.
    (*(*pEncCtx).pFuncList).pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };
    WelsPSliceMdEnc(pEncCtx, pSlice, kbHighestSpatial)
}

pub unsafe fn WelsCodePOverDynamicSlice(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let kbBaseAvail = (*pCurLayer).bBaseLayerAvailableFlag;
    let kbHighestSpatial = if !(*pEncCtx).pSvcParam.is_null() {
        (*(*pEncCtx).pSvcParam).iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    // `svc_encode_slice.cpp:750/753`, the dynamic-slicing twin of `WelsCodePSlice`.
    (*(*pEncCtx).pFuncList).pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };
    WelsPSliceMdEncDynamic(pEncCtx, pSlice, kbHighestSpatial)
}

pub unsafe extern "C" fn WelsCodePSlice_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsCodePSlice(pCtx, pSlice)
}

pub unsafe extern "C" fn WelsCodePOverDynamicSlice_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsCodePOverDynamicSlice(pCtx, pSlice)
}

pub unsafe extern "C" fn WelsISliceMdEnc_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsISliceMdEnc(pCtx, pSlice)
}

pub unsafe extern "C" fn WelsISliceMdEncDynamic_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsISliceMdEncDynamic(pCtx, pSlice)
}

pub unsafe extern "C" fn WelsSliceHeaderWrite_c(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
) {
    WelsSliceHeaderWrite(pCtx, pBs, pCurLayer, pSlice, pParametersetStrategy);
}

pub unsafe extern "C" fn WelsSliceHeaderExtWrite_c(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut SBitStringAux,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: *mut IWelsParametersetStrategy,
) {
    WelsSliceHeaderExtWrite(pCtx, pBs, pCurLayer, pSlice, pParametersetStrategy);
}

pub static g_pWelsSliceCoding: [[PWelsCodingSliceFunc; 2]; 2] = [
    [WelsCodePSlice_c, WelsCodePOverDynamicSlice_c],
    [WelsISliceMdEnc_c, WelsISliceMdEncDynamic_c],
];

pub static g_pWelsWriteSliceHeader: [PWelsSliceHeaderWriteFunc; 2] = [
    WelsSliceHeaderWrite_c,
    WelsSliceHeaderExtWrite_c,
];

pub unsafe fn WelsCodeOneSlice(pEncCtx: *mut sWelsEncCtx, pCurSlice: *mut SSlice, kiNalType: i32) -> i32 {
    if pEncCtx.is_null() || pCurSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pNalHeadExt = &mut (*pCurLayer).sLayerInfo.sNalHeaderExt;
    let pBs = (*pCurSlice).pSliceBsa;

    let kiDynamicSliceFlag = if !(*pEncCtx).pSvcParam.is_null() {
        let did = (*pEncCtx).uiDependencyId as usize;
        if (*(*pEncCtx).pSvcParam).sSpatialLayers[did].sSliceArgument.uiSliceMode == SliceMode::SM_SIZELIMITED_SLICE {
            1
        } else {
            0
        }
    } else {
        0
    };

    if (*pEncCtx).eSliceType == EWelsSliceType::I_SLICE {
        pNalHeadExt.bIdrFlag = true;
        (*pCurSlice).sScaleShift = 0;
    } else {
        let kuiTemporalId = pNalHeadExt.uiTemporalId;
        let ref_temporal = if !(*pEncCtx).pRefPic.is_null() { (*(*pEncCtx).pRefPic).uiTemporalId } else { 0 };
        (*pCurSlice).sScaleShift = if kuiTemporalId != 0 { kuiTemporalId.saturating_sub(ref_temporal) } else { 0 };
    }

    WelsSliceHeaderExtInit(pEncCtx, pCurLayer, pCurSlice);

    //RomRC init slice by slice
    let pWelsSvcRc = (*pEncCtx).pWelsSvcRc.add((*pEncCtx).uiDependencyId as usize);
    if !pWelsSvcRc.is_null() && (*pWelsSvcRc).bGomRC {
        crate::encoder::rc::GomRCInitForOneSlice(pCurSlice, (*pWelsSvcRc).iBitsPerMb);
    }

    let ext_hdr_idx = if (*pCurSlice).bSliceHeaderExtFlag { 1 } else { 0 };
    (g_pWelsWriteSliceHeader[ext_hdr_idx])(
        pEncCtx,
        pBs,
        pCurLayer,
        pCurSlice,
        if !(*pEncCtx).pFuncList.is_null() { (*(*pEncCtx).pFuncList).pParametersetStrategy } else { std::ptr::null_mut() },
    );

    let pic_init_qp = if !(*pCurLayer).sLayerInfo.pPpsP.is_null() {
        (*(*pCurLayer).sLayerInfo.pPpsP).iPicInitQp
    } else {
        26
    };
    (*pCurSlice).uiLastMbQp =
        (pic_init_qp as i32 + (*pCurSlice).sSliceHeaderExt.sSliceHeader.iSliceQpDelta as i32) as u8;

    let idr_idx = pNalHeadExt.bIdrFlag as usize;
    let func = g_pWelsSliceCoding[idr_idx][kiDynamicSliceFlag];
    let iEncReturn = func(pEncCtx, pCurSlice);
    if iEncReturn != ENC_RETURN_SUCCESS {
        return iEncReturn;
    }

    WelsWriteSliceEndSyn(pCurSlice, (*(*pEncCtx).pSvcParam).iEntropyCodingModeFlag != 0);

    ENC_RETURN_SUCCESS
}

/// `set_mb_syn_cavlc.cpp:279`. Terminates the slice bitstream.
///
/// This was missing entirely, and with it the `BsRbspTrailingBits` + `BsFlush` pair
/// that pushes the last partial 32-bit accumulator word out to the buffer -- so every
/// slice lost its final byte.
///
/// The CABAC branch hands the bitstream cursor back to `SBitStringAux` from the
/// arithmetic coder's own buffer pointer -- there is no `BsRbspTrailingBits` /
/// `BsFlush` pair, because `WelsCabacEncodeFlush` has already written the last
/// bytes directly.
///
/// # Safety
/// `pSlice` must be valid and have `pSliceBsa` installed.
pub unsafe fn WelsWriteSliceEndSyn(pSlice: *mut SSlice, bEntropyCodingModeFlag: bool) {
    let pBs = (*pSlice).pSliceBsa;
    if bEntropyCodingModeFlag {
        crate::encoder::set_mb_syn_cabac::WelsCabacEncodeFlush(&mut (*pSlice).sCabacCtx);
        (*pBs).pCurBuf =
            crate::encoder::set_mb_syn_cabac::WelsCabacEncodeGetPtr(&mut (*pSlice).sCabacCtx);
    } else {
        crate::encoder::vlc_encoder::BsRbspTrailingBits(pBs);
        crate::encoder::vlc_encoder::BsFlush(pBs);
    }
}

// ============================================================================
// Dynamic Slicing & Boundary Enforcement
// ============================================================================

pub unsafe fn AddSliceBoundary(
    pEncCtx: *mut sWelsEncCtx,
    pCurSlice: *mut SSlice,
    pSliceCtx: *mut SSliceCtx,
    pCurMb: *mut SMB,
    iFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
) {
    if pEncCtx.is_null() || pCurSlice.is_null() || pSliceCtx.is_null() || pCurMb.is_null() {
        return;
    }
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let buf_idx = (*pCurSlice).uiBufferIdx as usize;
    let pSliceBuffer = (*pCurLayer).sSliceBufferInfo[buf_idx].pSliceBuffer;
    let iCodedSliceNum = (*pCurLayer).sSliceBufferInfo[buf_idx].iCodedSliceNum;
    let iCurMbIdx = (*pCurMb).iMbXY;
    let iCurSliceIdc = *(*pSliceCtx).pOverallMbMap.add(iCurMbIdx as usize);
    let kiSliceIdxStep = (*pEncCtx).iActiveThreadsNum;
    let iNextSliceIdc = iCurSliceIdc + kiSliceIdxStep as u16;

    (*pCurSlice).sSliceHeaderExt.uiNumMbsInSlice = (1 + iCurMbIdx - (*pCurSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice) as u32;

    let pNextSlice = if (*pEncCtx).iActiveThreadsNum > 1 {
        pSliceBuffer.add((iCodedSliceNum + 1) as usize)
    } else {
        pSliceBuffer.add(iNextSliceIdc as usize)
    };

    if !pNextSlice.is_null() {
        (*pNextSlice).bSliceHeaderExtFlag = (*pCurLayer).sLayerInfo.sNalHeaderExt.sNalUnitHeader.eNalUnitType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;
        std::ptr::copy_nonoverlapping(&(*pCurSlice).sSliceHeaderExt, &mut (*pNextSlice).sSliceHeaderExt, 1);
        (*pNextSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = iFirstMbIdxOfNextSlice;

        // C++ calls WelsSetMemMultiplebytes_c, whose count is a signed int32_t; the
        // open-coded `for i in 0..count as usize` here wrapped to ~2^64 iterations
        // when the boundary landed past the end of the partition.
        crate::encoder::slice_multi_threading::WelsSetMemMultiplebytes_c(
            (*pSliceCtx).pOverallMbMap.add(iFirstMbIdxOfNextSlice as usize) as *mut c_void,
            iNextSliceIdc as u32,
            kiLastMbIdxInPartition - iFirstMbIdxOfNextSlice + 1,
            std::mem::size_of::<u16>() as i32,
        );

        UpdateMbNeighbourInfoForNextSlice(pCurLayer, (*pCurLayer).sMbDataP, iFirstMbIdxOfNextSlice, kiLastMbIdxInPartition);
    }
}

pub unsafe fn DynSlcJudgeSliceBoundaryStepBack(
    pCtx: *mut c_void,
    pSlice: *mut c_void,
    pSliceCtx: *mut SSliceCtx,
    pCurMb: *mut SMB,
    pDss: *mut SDynamicSlicingStack,
) -> bool {
    let pEncCtx = pCtx as *mut sWelsEncCtx;
    let pCurSlice = pSlice as *mut SSlice;
    let iCurMbIdx = (*pCurMb).iMbXY;
    let kiActiveThreadsNum = (*pEncCtx).iActiveThreadsNum;
    let kiPartitionId = ((*pCurSlice).iSliceIdx % (kiActiveThreadsNum as i32)) as usize;
    let kiEndMbIdxOfPartition = (*(*pEncCtx).pCurDqLayer).EndMbIdxOfPartition[kiPartitionId];

    let kbCurMbNotFirstMbOfCurSlice = (iCurMbIdx > 0)
        && (*(*pSliceCtx).pOverallMbMap.add(iCurMbIdx as usize) == *(*pSliceCtx).pOverallMbMap.add((iCurMbIdx - 1) as usize));
    let kbCurMbNotLastMbOfCurPartition = iCurMbIdx < kiEndMbIdxOfPartition;

    if (*pCurSlice).bDynamicSlicingSliceSizeCtrlFlag {
        return false;
    }

    let iPosBitOffset = (*pDss).iCurrentPos - (*pDss).iStartPos;
    let uiLen = ((iPosBitOffset >> 3) + if (iPosBitOffset & 0x07) != 0 { 1 } else { 0 }) as u32;

    if kbCurMbNotFirstMbOfCurSlice
        && JUMPPACKETSIZE_JUDGE(uiLen, iCurMbIdx, (*pSliceCtx).uiSliceSizeConstraint)
        && kbCurMbNotLastMbOfCurPartition
    {
        AddSliceBoundary(pEncCtx, pCurSlice, pSliceCtx, pCurMb, iCurMbIdx, kiEndMbIdxOfPartition);
        (*pSliceCtx).iSliceNumInFrame += 1;
        return true;
    }

    false
}

// ============================================================================
// Memory Management, Buffer Allocation & Dynamic Expansion
// ============================================================================

pub unsafe fn AllocMbCacheAligned(pMbCache: *mut SMbCache, pMa: *mut CMemoryAlign) -> i32 {
    if pMbCache.is_null() || pMa.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let tag = c"SMbCache_alloc".as_ptr();

    // C++: `WelsMallocz(2 * 256)` (`svc_encode_slice.cpp`). The `+ 16` is this
    // port's, and it is a soundness accommodation rather than a size the codec
    // uses — F14, `phase2_findings.md`.
    //
    // The two 256-byte halves are the I16x16 prediction ping-pong
    // (`pMemPredLuma` / `pMemPredChroma`, split at +256 in
    // `svc_base_layer_md.rs:371-373`). `WelsMdI16x16` scores a candidate in the
    // upper half with `WelsSampleSad16x16_c`, whose last 8x8 sub-block starts at
    // +392 and reads through byte 511 — *exactly* in bounds. But the raw kernel
    // is a C transliteration that bumps its row pointer after the final row
    // (`sad_common.rs:158`), computing `base + 520` and never dereferencing it.
    // Forming that pointer is UB in Rust and in C alike; nothing observable ever
    // came of it, which is why it survived the port.
    //
    // This is S12's rule met in production rather than in a test: a raw kernel's
    // pointer footprint is one stride larger than its read footprint. The `+ 16`
    // is one luma row at the ping-pong's stride of 16 — the smallest thing that
    // makes the arithmetic legal. It cannot change any encoded byte: the extra
    // bytes are never read, never written, and never addressed except by the
    // one-past bump this exists to keep in bounds.
    //
    // It goes away on its own if `common/sad_common.rs` re-lands from its park:
    // the safe kernels take exact spans and stop at the last row. Until then,
    // deleting this `+ 16` restores the UB and Miri's `--lib` gate catches it in
    // `svc_mode_decision::tests::test_wels_md_i16x16_cost`.
    (*pMbCache).pMemPredMb = (*pMa).WelsMallocz((2 * 256 + 16) as u32, tag) as *mut u8;
    if (*pMbCache).pMemPredMb.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pCoeffLevel = (*pMa).WelsMallocz((MB_COEFF_LIST_SIZE * std::mem::size_of::<i16>()) as u32, tag) as *mut i16;
    if (*pMbCache).pCoeffLevel.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pSkipMb = (*pMa).WelsMallocz(384, tag) as *mut u8;
    if (*pMbCache).pSkipMb.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pMemPredBlk4 = (*pMa).WelsMallocz((2 * 16) as u32, tag) as *mut u8;
    if (*pMbCache).pMemPredBlk4.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pBufferInterPredMe = (*pMa).WelsMallocz((4 * 640) as u32, tag) as *mut u8;
    if (*pMbCache).pBufferInterPredMe.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pPrevIntra4x4PredModeFlag = (*pMa).WelsMallocz(16 * std::mem::size_of::<bool>() as u32, tag) as *mut bool;
    if (*pMbCache).pPrevIntra4x4PredModeFlag.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pRemIntra4x4PredModeFlag = (*pMa).WelsMallocz(16, tag) as *mut i8;
    if (*pMbCache).pRemIntra4x4PredModeFlag.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pMbCache).pDct = (*pMa).WelsMallocz(std::mem::size_of::<SDCTCoeff>() as u32, tag) as *mut SDCTCoeff;
    if (*pMbCache).pDct.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn FreeMbCache(pMbCache: *mut SMbCache, pMa: *mut CMemoryAlign) {
    if pMbCache.is_null() || pMa.is_null() {
        return;
    }
    let tag = c"SMbCache_free".as_ptr();

    if !(*pMbCache).pCoeffLevel.is_null() {
        (*pMa).WelsFree((*pMbCache).pCoeffLevel as *mut c_void, tag);
        (*pMbCache).pCoeffLevel = std::ptr::null_mut();
    }
    if !(*pMbCache).pMemPredMb.is_null() {
        (*pMa).WelsFree((*pMbCache).pMemPredMb as *mut c_void, tag);
        (*pMbCache).pMemPredMb = std::ptr::null_mut();
    }
    if !(*pMbCache).pSkipMb.is_null() {
        (*pMa).WelsFree((*pMbCache).pSkipMb as *mut c_void, tag);
        (*pMbCache).pSkipMb = std::ptr::null_mut();
    }
    if !(*pMbCache).pMemPredBlk4.is_null() {
        (*pMa).WelsFree((*pMbCache).pMemPredBlk4 as *mut c_void, tag);
        (*pMbCache).pMemPredBlk4 = std::ptr::null_mut();
    }
    if !(*pMbCache).pBufferInterPredMe.is_null() {
        (*pMa).WelsFree((*pMbCache).pBufferInterPredMe as *mut c_void, tag);
        (*pMbCache).pBufferInterPredMe = std::ptr::null_mut();
    }
    if !(*pMbCache).pPrevIntra4x4PredModeFlag.is_null() {
        (*pMa).WelsFree((*pMbCache).pPrevIntra4x4PredModeFlag as *mut c_void, tag);
        (*pMbCache).pPrevIntra4x4PredModeFlag = std::ptr::null_mut();
    }
    if !(*pMbCache).pRemIntra4x4PredModeFlag.is_null() {
        (*pMa).WelsFree((*pMbCache).pRemIntra4x4PredModeFlag as *mut c_void, tag);
        (*pMbCache).pRemIntra4x4PredModeFlag = std::ptr::null_mut();
    }
    if !(*pMbCache).pDct.is_null() {
        (*pMa).WelsFree((*pMbCache).pDct as *mut c_void, tag);
        (*pMbCache).pDct = std::ptr::null_mut();
    }
}

pub unsafe fn InitSliceBoundaryInfo(
    pCurLayer: *mut SDqLayer,
    pSliceArgument: *mut SSliceArgument,
    kiSliceNumInFrame: i32,
) -> i32 {
    if pCurLayer.is_null() || pSliceArgument.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let kiMBWidth: i32 = (*pCurLayer).iMbWidth as i32;
    let kiMBHeight: i32 = (*pCurLayer).iMbHeight as i32;
    let kiCountNumMbInFrame: i32 = kiMBWidth * kiMBHeight;

    for iSliceIdx in 0..kiSliceNumInFrame {
        let mut iFirstMBInSlice: i32;
        let mut iMbNumInSlice: i32;

        match (*pSliceArgument).uiSliceMode {
            SliceMode::SM_SINGLE_SLICE => {
                iFirstMBInSlice = 0;
                iMbNumInSlice = kiCountNumMbInFrame;
            }
            SliceMode::SM_RASTER_SLICE if (*pSliceArgument).uiSliceMbNum[0] == 0 => {
                iFirstMBInSlice = iSliceIdx * kiMBWidth;
                iMbNumInSlice = kiMBWidth;
            }
            SliceMode::SM_RASTER_SLICE | SliceMode::SM_FIXEDSLCNUM_SLICE => {
                let mut iMbIdx = 0;
                for i in 0..iSliceIdx {
                    iMbIdx += (*pSliceArgument).uiSliceMbNum[i as usize] as i32;
                }
                if iMbIdx >= kiCountNumMbInFrame {
                    return ENC_RETURN_UNEXPECTED;
                }
                iFirstMBInSlice = iMbIdx;
                iMbNumInSlice = (*pSliceArgument).uiSliceMbNum[iSliceIdx as usize] as i32;
            }
            SliceMode::SM_SIZELIMITED_SLICE => {
                iFirstMBInSlice = 0;
                iMbNumInSlice = kiCountNumMbInFrame;
            }
            _ => {
                iFirstMBInSlice = 0;
                iMbNumInSlice = kiCountNumMbInFrame;
            }
        }

        *(*pCurLayer).pCountMbNumInSlice.add(iSliceIdx as usize) = iMbNumInSlice;
        *(*pCurLayer).pFirstMbIdxOfSlice.add(iSliceIdx as usize) = iFirstMBInSlice;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn SetSliceBoundaryInfo(pCurLayer: *mut SDqLayer, pSlice: *mut SSlice, kiSliceIdx: i32) -> i32 {
    if pCurLayer.is_null()
        || pSlice.is_null()
        || (*pCurLayer).pFirstMbIdxOfSlice.is_null()
        || (*pCurLayer).pCountMbNumInSlice.is_null()
    {
        return ENC_RETURN_UNEXPECTED;
    }

    (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = *(*pCurLayer).pFirstMbIdxOfSlice.add(kiSliceIdx as usize);
    (*pSlice).iCountMbNumInSlice = *(*pCurLayer).pCountMbNumInSlice.add(kiSliceIdx as usize);

    ENC_RETURN_SUCCESS
}

pub unsafe fn AllocateSliceMBBuffer(pSlice: *mut SSlice, pMa: *mut CMemoryAlign) -> i32 {
    if AllocMbCacheAligned(&mut (*pSlice).sMbCacheInfo, pMa) != 0 {
        ENC_RETURN_MEMALLOCERR
    } else {
        ENC_RETURN_SUCCESS
    }
}

pub unsafe fn InitSliceBsBuffer(
    pSlice: *mut SSlice,
    pBsWrite: *mut SBitStringAux,
    bIndependenceBsBuffer: bool,
    iMaxSliceBufferSize: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    (*pSlice).sSliceBs.uiSize = iMaxSliceBufferSize as u32;
    (*pSlice).sSliceBs.uiBsPos = 0;

    if bIndependenceBsBuffer {
        let tag = c"sSliceBs.pBs".as_ptr();
        (*pSlice).pSliceBsa = &mut (*pSlice).sSliceBs.sBsWrite;
        (*pSlice).sSliceBs.pBs = (*pMa).WelsMallocz(iMaxSliceBufferSize as u32, tag) as *mut u8;
        if (*pSlice).sSliceBs.pBs.is_null() {
            (*pSlice).sSliceBs.uiBsSize = 0;
            return ENC_RETURN_MEMALLOCERR;
        }
        (*pSlice).sSliceBs.uiBsSize = iMaxSliceBufferSize as u32;
    } else {
        (*pSlice).pSliceBsa = pBsWrite;
        (*pSlice).sSliceBs.pBs = std::ptr::null_mut();
        (*pSlice).sSliceBs.uiBsSize = 0;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn FreeSliceBuffer(pSliceList: *mut *mut SSlice, kiMaxSliceNum: i32, pMa: *mut CMemoryAlign, kpTag: *const c_char) {
    if !pSliceList.is_null() && !(*pSliceList).is_null() {
        let slice_array = *pSliceList;
        for iSliceIdx in 0..kiMaxSliceNum {
            let pSlice = slice_array.add(iSliceIdx as usize);
            FreeMbCache(&mut (*pSlice).sMbCacheInfo, pMa);
            if !(*pSlice).sSliceBs.pBs.is_null() {
                (*pMa).WelsFree((*pSlice).sSliceBs.pBs as *mut c_void, c"sSliceBs.pBs".as_ptr());
                (*pSlice).sSliceBs.pBs = std::ptr::null_mut();
            }
        }
        (*pMa).WelsFree(slice_array as *mut c_void, kpTag);
        *pSliceList = std::ptr::null_mut();
    }
}

pub unsafe fn InitSliceList(
    pSliceList: *mut SSlice,
    pBsWrite: *mut SBitStringAux,
    kiMaxSliceNum: i32,
    kiMaxSliceBufferSize: i32,
    bIndependenceBsBuffer: bool,
    pMa: *mut CMemoryAlign,
) -> i32 {
    if kiMaxSliceBufferSize <= 0 {
        return ENC_RETURN_UNEXPECTED;
    }

    for iSliceIdx in 0..kiMaxSliceNum {
        let pSlice = pSliceList.add(iSliceIdx as usize);
        if pSlice.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }

        (*pSlice).iSliceIdx = iSliceIdx;
        (*pSlice).uiBufferIdx = 0;
        (*pSlice).iCountMbNumInSlice = 0;
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let iRet = InitSliceBsBuffer(pSlice, pBsWrite, bIndependenceBsBuffer, kiMaxSliceBufferSize, pMa);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        let iRet2 = AllocateSliceMBBuffer(pSlice, pMa);
        if iRet2 != ENC_RETURN_SUCCESS {
            return iRet2;
        }
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn InitAllSlicesInThread(pCtx: *mut sWelsEncCtx) -> i32 {
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    for iSliceIdx in 0..(*pCurDqLayer).iMaxSliceNum {
        let slice_ptr = *(*pCurDqLayer).ppSliceInLayer.add(iSliceIdx as usize);
        if slice_ptr.is_null() {
            return ENC_RETURN_UNEXPECTED;
        }
        (*slice_ptr).iSliceIdx = -1;
    }

    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        (*pCurDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iCodedSliceNum = 0;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn InitOneSliceInThread(
    pCtx: *mut sWelsEncCtx,
    pSlice: *mut *mut SSlice,
    kiSlcBuffIdx: i32,
    kiDlayerIdx: i32,
    kiSliceIdx: i32,
) -> i32 {
    let pCurDq = (*pCtx).pCurDqLayer;
    let slc_ptr = if (*pCurDq).bThreadSlcBufferFlag {
        let kiCodedNumInThread = (*pCurDq).sSliceBufferInfo[kiSlcBuffIdx as usize].iCodedSliceNum;
        (*pCurDq).sSliceBufferInfo[kiSlcBuffIdx as usize].pSliceBuffer.add(kiCodedNumInThread as usize)
    } else {
        (*pCurDq).sSliceBufferInfo[0].pSliceBuffer.add(kiSliceIdx as usize)
    };

    *pSlice = slc_ptr;
    (*slc_ptr).iSliceIdx = kiSliceIdx;
    (*slc_ptr).uiBufferIdx = kiSlcBuffIdx as u32;

    (*slc_ptr).sSliceBs.uiBsPos = 0;
    (*slc_ptr).sSliceBs.iNalIndex = 0;
    if !(*pCtx).pSliceThreading.is_null() {
        (*slc_ptr).sSliceBs.pBsBuffer = (*(*pCtx).pSliceThreading).pThreadBsBuffer[kiSlcBuffIdx as usize];
    }
    (*slc_ptr).sSliceBs.uiSize = (*pCtx).iFrameBsSize as u32;

    ENC_RETURN_SUCCESS
}

pub unsafe fn InitSliceThreadInfo(
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIndex: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    let iThreadNum = if !(*pCtx).pSvcParam.is_null() {
        (*(*pCtx).pSvcParam).iMultipleThreadIdc as i32
    } else {
        1
    };

    let (iMaxSliceNum, iSlcBufferNum) = if (*pDqLayer).bThreadSlcBufferFlag {
        ((*pDqLayer).iMaxSliceNum / iThreadNum + 1, iThreadNum)
    } else {
        ((*pDqLayer).iMaxSliceNum, 1)
    };

    let mut iIdx = 0;
    while iIdx < iSlcBufferNum {
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iMaxSliceNum = iMaxSliceNum;
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iCodedSliceNum = 0;
        let pBuf = (*pMa).WelsMallocz((std::mem::size_of::<SSlice>() * iMaxSliceNum as usize) as u32, c"pSliceBuffer".as_ptr()) as *mut SSlice;
        if pBuf.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].pSliceBuffer = pBuf;

        let iRet = InitSliceList(
            pBuf,
            &mut (*(*pCtx).pOut).sBsWrite,
            iMaxSliceNum,
            (*pCtx).iSliceBufferSize[kiDlayerIndex as usize],
            (*pDqLayer).bSliceBsBufferFlag,
            pMa,
        );
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        iIdx += 1;
    }

    while (iIdx as usize) < MAX_THREADS_NUM {
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iMaxSliceNum = 0;
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iCodedSliceNum = 0;
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].pSliceBuffer = std::ptr::null_mut();
        iIdx += 1;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn InitSliceInLayer(
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIndex: i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    let pSliceArgument = &mut (*(*pCtx).pSvcParam).sSpatialLayers[kiDlayerIndex as usize].sSliceArgument;

    (*pDqLayer).bSliceBsBufferFlag = (*(*pCtx).pSvcParam).iMultipleThreadIdc > 1
        && pSliceArgument.uiSliceMode != SliceMode::SM_SINGLE_SLICE;

    (*pDqLayer).bThreadSlcBufferFlag = (*(*pCtx).pSvcParam).iMultipleThreadIdc > 1
        && pSliceArgument.uiSliceMode == SliceMode::SM_SIZELIMITED_SLICE;

    let iRet = InitSliceThreadInfo(pCtx, pDqLayer, kiDlayerIndex, pMa);
    if iRet != ENC_RETURN_SUCCESS {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pDqLayer).iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        (*pDqLayer).iMaxSliceNum += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    let ppSlice = (*pMa).WelsMallocz((std::mem::size_of::<*mut SSlice>() * (*pDqLayer).iMaxSliceNum as usize) as u32, c"ppSliceInLayer".as_ptr()) as *mut *mut SSlice;
    if ppSlice.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    (*pDqLayer).ppSliceInLayer = ppSlice;

    let pFirstMb = (*pMa).WelsMallocz((std::mem::size_of::<i32>() * (*pDqLayer).iMaxSliceNum as usize) as u32, c"pFirstMbIdxOfSlice".as_ptr()) as *mut i32;
    if pFirstMb.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    (*pDqLayer).pFirstMbIdxOfSlice = pFirstMb;

    let pCountMb = (*pMa).WelsMallocz((std::mem::size_of::<i32>() * (*pDqLayer).iMaxSliceNum as usize) as u32, c"pCountMbNumInSlice".as_ptr()) as *mut i32;
    if pCountMb.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    (*pDqLayer).pCountMbNumInSlice = pCountMb;

    let iRet2 = InitSliceBoundaryInfo(pDqLayer, pSliceArgument, (*pDqLayer).iMaxSliceNum);
    if iRet2 != ENC_RETURN_SUCCESS {
        return iRet2;
    }

    let mut iStartIdx = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        for iSliceIdx in 0..(*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum {
            *(*pDqLayer).ppSliceInLayer.add((iStartIdx + iSliceIdx) as usize) =
                (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].pSliceBuffer.add(iSliceIdx as usize);
        }
        iStartIdx += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn InitSliceHeadWithBase(pSlice: *mut SSlice, pBaseSlice: *mut SSlice) {
    if pSlice.is_null() || pBaseSlice.is_null() {
        return;
    }
    let pBaseSHExt = &mut (*pBaseSlice).sSliceHeaderExt;
    let pSHExt = &mut (*pSlice).sSliceHeaderExt;

    (*pSlice).bSliceHeaderExtFlag = (*pBaseSlice).bSliceHeaderExtFlag;
    pSHExt.sSliceHeader.iPpsId = pBaseSHExt.sSliceHeader.iPpsId;
    pSHExt.sSliceHeader.pPps = pBaseSHExt.sSliceHeader.pPps;
    pSHExt.sSliceHeader.iSpsId = pBaseSHExt.sSliceHeader.iSpsId;
    pSHExt.sSliceHeader.pSps = pBaseSHExt.sSliceHeader.pSps;
}

pub unsafe fn InitSliceRefInfoWithBase(pSlice: *mut SSlice, pBaseSlice: *mut SSlice, kuiRefCount: u8) {
    if pSlice.is_null() || pBaseSlice.is_null() {
        return;
    }
    let pBaseSHExt = &mut (*pBaseSlice).sSliceHeaderExt;
    let pSHExt = &mut (*pSlice).sSliceHeaderExt;

    pSHExt.sSliceHeader.uiRefCount = kuiRefCount;
    pSHExt.sSliceHeader.sRefMarking = pBaseSHExt.sSliceHeader.sRefMarking;
    pSHExt.sSliceHeader.sRefReordering = pBaseSHExt.sSliceHeader.sRefReordering;
}

#[inline]
pub unsafe fn InitSliceRC(pSlice: *mut SSlice, kiGlobalQp: i32) -> i32 {
    if pSlice.is_null() || kiGlobalQp < 0 {
        return ENC_RETURN_INVALIDINPUT;
    }
    (*pSlice).sSlicingOverRc.iComplexityIndexSlice = 0;
    (*pSlice).sSlicingOverRc.iCalculatedQpSlice = kiGlobalQp;
    (*pSlice).sSlicingOverRc.iTotalQpSlice = 0;
    (*pSlice).sSlicingOverRc.iTotalMbSlice = 0;
    (*pSlice).sSlicingOverRc.iTargetBitsSlice = 0;
    (*pSlice).sSlicingOverRc.iFrameBitsSlice = 0;
    (*pSlice).sSlicingOverRc.iGomBitsSlice = 0;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ReallocateSliceList(
    pCtx: *mut sWelsEncCtx,
    pSliceArgument: *mut SSliceArgument,
    pSliceList: *mut *mut SSlice,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    let pMA = (*pCtx).pMemAlign;
    if pSliceList.is_null() || (*pSliceList).is_null() || pSliceArgument.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }

    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let iMaxSliceBufferSize = (*pCtx).iSliceBufferSize[kiCurDid];
    let bIndependenceBsBuffer = (*(*pCtx).pSvcParam).iMultipleThreadIdc > 1
        && (*pSliceArgument).uiSliceMode != SliceMode::SM_SINGLE_SLICE;

    let pNewSliceList = (*pMA).WelsMallocz(
        (std::mem::size_of::<SSlice>() * kiMaxSliceNumNew as usize) as u32,
        c"pSliceBuffer".as_ptr(),
    ) as *mut SSlice;
    if pNewSliceList.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }

    std::ptr::copy_nonoverlapping(*pSliceList, pNewSliceList, kiMaxSliceNumOld as usize);

    for iSliceIdx in 0..kiMaxSliceNumOld {
        let pSlice = pNewSliceList.add(iSliceIdx as usize);
        if bIndependenceBsBuffer {
            (*pSlice).pSliceBsa = &mut (*pSlice).sSliceBs.sBsWrite;
        }
    }

    let pBaseSlice = *pSliceList;

    for iSliceIdx in kiMaxSliceNumOld..kiMaxSliceNumNew {
        let pSlice = pNewSliceList.add(iSliceIdx as usize);
        (*pSlice).iSliceIdx = -1;
        (*pSlice).uiBufferIdx = 0;
        (*pSlice).iCountMbNumInSlice = 0;
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let mut iRet = InitSliceBsBuffer(pSlice, &mut (*(*pCtx).pOut).sBsWrite, bIndependenceBsBuffer, iMaxSliceBufferSize, pMA);
        if iRet != ENC_RETURN_SUCCESS {
            let mut tmp = pNewSliceList;
            FreeSliceBuffer(&mut tmp, kiMaxSliceNumNew, pMA, c"pSliceBuffer".as_ptr());
            return iRet;
        }

        iRet = AllocateSliceMBBuffer(pSlice, pMA);
        if iRet != ENC_RETURN_SUCCESS {
            let mut tmp = pNewSliceList;
            FreeSliceBuffer(&mut tmp, kiMaxSliceNumNew, pMA, c"pSliceBuffer".as_ptr());
            return iRet;
        }

        InitSliceHeadWithBase(pSlice, pBaseSlice);
        InitSliceRefInfoWithBase(pSlice, pBaseSlice, (*pCtx).iNumRef0);

        iRet = InitSliceRC(pSlice, (*pCtx).iGlobalQp);
        if iRet != ENC_RETURN_SUCCESS {
            let mut tmp = pNewSliceList;
            FreeSliceBuffer(&mut tmp, kiMaxSliceNumNew, pMA, c"pSliceBuffer".as_ptr());
            return iRet;
        }
    }

    (*pMA).WelsFree(*pSliceList as *mut c_void, c"pSliceBuffer".as_ptr());
    *pSliceList = pNewSliceList;

    ENC_RETURN_SUCCESS
}

pub unsafe fn CalculateNewSliceNum(
    pCtx: *mut sWelsEncCtx,
    pLastCodedSlice: *mut SSlice,
    iMaxSliceNumOld: i32,
    iMaxSliceNumNew: *mut i32,
) -> i32 {
    if pCtx.is_null() || pLastCodedSlice.is_null() || iMaxSliceNumOld == 0 || iMaxSliceNumNew.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }

    if (*pCtx).iActiveThreadsNum == 1 {
        *iMaxSliceNumNew = iMaxSliceNumOld * SLICE_NUM_EXPAND_COEF;
        return ENC_RETURN_SUCCESS;
    }

    let iPartitionID = ((*pLastCodedSlice).iSliceIdx % ((*pCtx).iActiveThreadsNum as i32)) as usize;
    let pCurLayer = (*pCtx).pCurDqLayer;
    let iMBNumInPartition = (*pCurLayer).EndMbIdxOfPartition[iPartitionID] - (*pCurLayer).FirstMbIdxOfPartition[iPartitionID] + 1;
    let iLeftMBNum = (*pCurLayer).EndMbIdxOfPartition[iPartitionID] - (*pCurLayer).LastCodedMbIdxOfPartition[iPartitionID] + 1;

    let mut iIncreaseSliceNum = if iMBNumInPartition > 0 {
        (iLeftMBNum * INT_MULTIPLY / iMBNumInPartition) * iMaxSliceNumOld
    } else {
        0
    };

    iIncreaseSliceNum = if (iIncreaseSliceNum / INT_MULTIPLY) == 0 { 1 } else { iIncreaseSliceNum / INT_MULTIPLY };
    iIncreaseSliceNum = if iIncreaseSliceNum < (iMaxSliceNumOld / 2) { iMaxSliceNumOld / 2 } else { iIncreaseSliceNum };

    *iMaxSliceNumNew = iMaxSliceNumOld + iIncreaseSliceNum;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ReallocateSliceInThread(
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIdx: i32,
    KiSlcBuffIdx: i32,
) -> i32 {
    let iMaxSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iMaxSliceNum;
    let iCodedSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iCodedSliceNum;
    let mut iMaxSliceNumNew = 0;
    let pLastCodedSlice = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].pSliceBuffer.add((iCodedSliceNum - 1) as usize);
    let pSliceArgument = &mut (*(*pCtx).pSvcParam).sSpatialLayers[kiDlayerIdx as usize].sSliceArgument;

    let mut iRet = CalculateNewSliceNum(pCtx, pLastCodedSlice, iMaxSliceNum, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let mut slice_buf_ptr = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].pSliceBuffer;
    iRet = ReallocateSliceList(pCtx, pSliceArgument, &mut slice_buf_ptr, iMaxSliceNum, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].pSliceBuffer = slice_buf_ptr;
    (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ExtendLayerBuffer(
    pCtx: *mut sWelsEncCtx,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    let pMA = (*pCtx).pMemAlign;
    let pCurLayer = (*pCtx).pCurDqLayer;

    let ppSlice = (*pMA).WelsMallocz((std::mem::size_of::<*mut SSlice>() * kiMaxSliceNumNew as usize) as u32, c"ppSliceInLayer".as_ptr()) as *mut *mut SSlice;
    if ppSlice.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    (*pMA).WelsFree((*pCurLayer).ppSliceInLayer as *mut c_void, c"ppSliceInLayer".as_ptr());
    (*pCurLayer).ppSliceInLayer = ppSlice;

    let pFirstMbIdxOfSlice = (*pMA).WelsMallocz((std::mem::size_of::<i32>() * kiMaxSliceNumNew as usize) as u32, c"pFirstMbIdxOfSlice".as_ptr()) as *mut i32;
    if pFirstMbIdxOfSlice.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    std::ptr::copy_nonoverlapping((*pCurLayer).pFirstMbIdxOfSlice, pFirstMbIdxOfSlice, kiMaxSliceNumOld as usize);
    (*pMA).WelsFree((*pCurLayer).pFirstMbIdxOfSlice as *mut c_void, c"pFirstMbIdxOfSlice".as_ptr());
    (*pCurLayer).pFirstMbIdxOfSlice = pFirstMbIdxOfSlice;

    let pCountMbNumInSlice = (*pMA).WelsMallocz((std::mem::size_of::<i32>() * kiMaxSliceNumNew as usize) as u32, c"pCountMbNumInSlice".as_ptr()) as *mut i32;
    if pCountMbNumInSlice.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    std::ptr::copy_nonoverlapping((*pCurLayer).pCountMbNumInSlice, pCountMbNumInSlice, kiMaxSliceNumOld as usize);
    (*pMA).WelsFree((*pCurLayer).pCountMbNumInSlice as *mut c_void, c"pCountMbNumInSlice".as_ptr());
    (*pCurLayer).pCountMbNumInSlice = pCountMbNumInSlice;

    ENC_RETURN_SUCCESS
}

pub unsafe fn ReallocSliceBuffer(pCtx: *mut sWelsEncCtx) -> i32 {
    let pCurLayer = (*pCtx).pCurDqLayer;
    let iMaxSliceNumOld = (*pCurLayer).sSliceBufferInfo[0].iMaxSliceNum;
    let mut iMaxSliceNumNew = 0;
    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let pLastCodedSlice = (*pCurLayer).sSliceBufferInfo[0].pSliceBuffer.add((iMaxSliceNumOld - 1) as usize);
    let pSliceArgument = &mut (*(*pCtx).pSvcParam).sSpatialLayers[kiCurDid].sSliceArgument;

    let mut iRet = CalculateNewSliceNum(pCtx, pLastCodedSlice, iMaxSliceNumOld, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let mut slice_buf_ptr = (*pCurLayer).sSliceBufferInfo[0].pSliceBuffer;
    iRet = ReallocateSliceList(pCtx, pSliceArgument, &mut slice_buf_ptr, iMaxSliceNumOld, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    (*pCurLayer).sSliceBufferInfo[0].pSliceBuffer = slice_buf_ptr;
    (*pCurLayer).sSliceBufferInfo[0].iMaxSliceNum = iMaxSliceNumNew;

    iMaxSliceNumNew = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        iMaxSliceNumNew += (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    iRet = ExtendLayerBuffer(pCtx, (*pCurLayer).iMaxSliceNum, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let mut iStartIdx = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        for iSliceIdx in 0..(*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum {
            *(*pCurLayer).ppSliceInLayer.add((iStartIdx + iSliceIdx) as usize) =
                (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].pSliceBuffer.add(iSliceIdx as usize);
        }
        iStartIdx += (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    (*pCurLayer).iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

#[inline]
pub unsafe fn CheckAllSliceBuffer(pCurLayer: *mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    for iSliceIdx in 0..kiCodedSliceNum {
        let slice_ptr = *(*pCurLayer).ppSliceInLayer.add(iSliceIdx as usize);
        if slice_ptr.is_null() || iSliceIdx != (*slice_ptr).iSliceIdx {
            return ENC_RETURN_UNEXPECTED;
        }
    }
    ENC_RETURN_SUCCESS
}

pub unsafe fn ReOrderSliceInLayer(pCtx: *mut sWelsEncCtx, kuiSliceMode: SliceMode, kiThreadNum: i32) -> i32 {
    let pCurLayer = (*pCtx).pCurDqLayer;
    let mut iEncodeSliceNum = 0;
    let mut iUsedSliceNum = 0;
    let mut iNonUsedBufferNum = 0;
    let mut aiPartitionOffset = [0i32; MAX_THREADS_NUM];

    let iPartitionNum = if kuiSliceMode == SliceMode::SM_SIZELIMITED_SLICE { kiThreadNum } else { 1 };
    for iPartitionIdx in 0..iPartitionNum {
        aiPartitionOffset[iPartitionIdx as usize] = iEncodeSliceNum;
        if kuiSliceMode == SliceMode::SM_SIZELIMITED_SLICE {
            iEncodeSliceNum += (*pCurLayer).NumSliceCodedOfPartition[iPartitionIdx as usize];
        } else {
            iEncodeSliceNum = (*pCurLayer).sSliceEncCtx.iSliceNumInFrame;
        }
    }

    if iEncodeSliceNum != (*pCurLayer).sSliceEncCtx.iSliceNumInFrame {
        return ENC_RETURN_UNEXPECTED;
    }

    for iSlcBuffIdx in 0..kiThreadNum {
        let iSliceNumInThread = (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
        for iSliceIdx in 0..iSliceNumInThread {
            let pSliceBuffer = (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].pSliceBuffer.add(iSliceIdx as usize);
            if pSliceBuffer.is_null() {
                return ENC_RETURN_UNEXPECTED;
            }

            if (*pSliceBuffer).iSliceIdx != -1 {
                let iPartitionID = (*pSliceBuffer).iSliceIdx % iPartitionNum;
                let iActualSliceIdx = aiPartitionOffset[iPartitionID as usize] + (*pSliceBuffer).iSliceIdx / iPartitionNum;
                (*pSliceBuffer).iSliceIdx = iActualSliceIdx;
                *(*pCurLayer).ppSliceInLayer.add(iActualSliceIdx as usize) = pSliceBuffer;
                iUsedSliceNum += 1;
            } else {
                *(*pCurLayer).ppSliceInLayer.add((iEncodeSliceNum + iNonUsedBufferNum) as usize) = pSliceBuffer;
                iNonUsedBufferNum += 1;
            }
        }
    }

    if iUsedSliceNum != iEncodeSliceNum || (*pCurLayer).iMaxSliceNum != (iNonUsedBufferNum + iUsedSliceNum) {
        return ENC_RETURN_UNEXPECTED;
    }

    CheckAllSliceBuffer(pCurLayer, iEncodeSliceNum)
}

pub unsafe fn GetCurLayerNalCount(pCurDq: *const SDqLayer, kiCodedSliceNum: i32) -> i32 {
    let mut iTotalNalCount = 0;
    for iSliceIdx in 0..kiCodedSliceNum {
        let slice_ptr = *(*pCurDq).ppSliceInLayer.add(iSliceIdx as usize);
        if !slice_ptr.is_null() && (*slice_ptr).sSliceBs.uiBsPos > 0 {
            iTotalNalCount += (*slice_ptr).sSliceBs.iNalIndex;
        }
    }
    iTotalNalCount
}

pub unsafe fn GetTotalCodedNalCount(pFbi: *mut SFrameBSInfo) -> i32 {
    let mut iTotalCodedNalCount = 0;
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME {
        iTotalCodedNalCount += (*pFbi).sLayerInfo[iNalIdx].iNalCount;
    }
    iTotalCodedNalCount
}

pub unsafe fn GetCurrentSliceNum(pCurDq: *const SDqLayer) -> i32 {
    if pCurDq.is_null() {
        -1
    } else {
        (*pCurDq).sSliceEncCtx.iSliceNumInFrame
    }
}

pub unsafe fn FrameBsRealloc(
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kiMaxSliceNumOld: i32,
) -> i32 {
    let pMA = (*pCtx).pMemAlign;
    let mut iCountNals = (*(*pCtx).pOut).iCountNals;
    let spatial_layers = if !(*pCtx).pSvcParam.is_null() { (*(*pCtx).pSvcParam).iSpatialLayerNum } else { 1 };
    iCountNals += kiMaxSliceNumOld * (spatial_layers + if (*pCtx).bNeedPrefixNalFlag { 1 } else { 0 });

    let pNalList = (*pMA).WelsMallocz(
        (iCountNals as usize * std::mem::size_of::<SWelsNalRaw>()) as u32,
        c"pOut->sNalList".as_ptr(),
    ) as *mut SWelsNalRaw;
    if pNalList.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    std::ptr::copy_nonoverlapping((*(*pCtx).pOut).sNalList, pNalList, (*(*pCtx).pOut).iCountNals as usize);
    (*pMA).WelsFree((*(*pCtx).pOut).sNalList as *mut c_void, c"pOut->sNalList".as_ptr());
    (*(*pCtx).pOut).sNalList = pNalList;

    let pNalLen = (*pMA).WelsMallocz(
        (iCountNals as usize * std::mem::size_of::<i32>()) as u32,
        c"pOut->pNalLen".as_ptr(),
    ) as *mut i32;
    if pNalLen.is_null() {
        return ENC_RETURN_MEMALLOCERR;
    }
    std::ptr::copy_nonoverlapping((*(*pCtx).pOut).pNalLen, pNalLen, (*(*pCtx).pOut).iCountNals as usize);
    (*pMA).WelsFree((*(*pCtx).pOut).pNalLen as *mut c_void, c"pOut->pNalLen".as_ptr());
    (*(*pCtx).pOut).pNalLen = pNalLen;

    (*(*pCtx).pOut).iCountNals = iCountNals;

    ENC_RETURN_SUCCESS
}

pub unsafe fn SliceLayerInfoUpdate(
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kuiSliceMode: SliceMode,
) -> i32 {
    let mut iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        iMaxSliceNum += (*(*pCtx).pCurDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    if iMaxSliceNum > (*(*pCtx).pCurDqLayer).iMaxSliceNum {
        let iRet = ExtendLayerBuffer(pCtx, (*(*pCtx).pCurDqLayer).iMaxSliceNum, iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        (*(*pCtx).pCurDqLayer).iMaxSliceNum = iMaxSliceNum;
    }

    let mut iRet = ReOrderSliceInLayer(pCtx, kuiSliceMode, (*pCtx).iActiveThreadsNum as i32);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let iCodedSliceNum = GetCurrentSliceNum((*pCtx).pCurDqLayer);
    (*pLayerBsInfo).iNalCount = GetCurLayerNalCount((*pCtx).pCurDqLayer, iCodedSliceNum);
    let iCodedNalCount = GetTotalCodedNalCount(pFrameBsInfo);

    if iCodedNalCount > (*(*pCtx).pOut).iCountNals {
        iRet = FrameBsRealloc(pCtx, pFrameBsInfo, pLayerBsInfo, (*(*pCtx).pCurDqLayer).iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

pub unsafe fn WelsInitSliceEncodingFuncs(uiCpuFlag: u32) {
    // Dynamically wires CPU architecture flags if SIMD variants are enabled
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static MB_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
