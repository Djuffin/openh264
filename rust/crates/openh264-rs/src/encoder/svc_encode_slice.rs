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

#![deny(unsafe_code)]

use crate::encoder::rec_view::RecCursor;
use crate::encoder::decode_mb_aux::{
    idct_four_t4_rec_in_place_view, idct_four_t4_rec_to_view, idct_t4_rec_on_mb_in_place_view,
};
use crate::encoder::encode_mb_aux::{blk_four4x4, blk_mb256};
use std::sync::atomic::{AtomicI32, AtomicU16, Ordering};
use crate::encoder::picture::{PicRef, RecPicId, SPicture, SrcPicId};
use std::ffi::c_char;
use crate::{
    SliceMode, SFrameBSInfo, SLayerBSInfo, SSliceArgument,
    MAX_LAYER_NUM_OF_FRAME, MAX_SPATIAL_LAYER_NUM, MAX_QUALITY_LAYER_NUM, MAX_NAL_UNITS_IN_LAYER,
};

// ============================================================================
// Constants and Definitions
// ============================================================================

pub use crate::encoder::encoder_context::EWelsSliceType;
use crate::encoder::encoder_context::{
    ctx_dq_layer,
};

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
pub use crate::encoder::encoder_context::MAX_DEPENDENCY_LAYER;
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
    // **`pSps` and `pPps` stood here and are deleted, not converted** (T6.G3).
    // Their replacement was already in the struct: `iSpsId`/`iPpsId`, immediately
    // below, are the same two numbers, written from the same locals in the same
    // statement block (`WelsInitCurrentLayer`) and copied by the same function
    // (`InitSliceHeadWithBase`). Converting the pointers to ids would have produced
    // a second dead copy of a number already stored two lines down.
    //
    // Dead is measured, not assumed: grepped tree-wide at this step, both fields are
    // **written and never read**. The C++ does read `pSliceHeader->pPps->iPpsId`
    // (`svc_encode_slice.cpp:285`/`:361`) — this port reads `sLayerInfo.pPpsP`
    // there instead, and says so at both sites. So does `iPpsId` below, which is
    // write-only for the same reason; it stays because it is a syntax element and
    // not a pointer, and naming it here is cheaper than finding it again.
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub uiIdrPicId: u16,
    pub bNumRefIdxActiveOverrideFlag: bool,
    pub uiPadding1Bytes: u8,
    pub sRefMarking: SRefPicMarking,
    pub sRefReordering: SRefPicListReorderSyntax,
}

impl Default for SSliceHeader {
    /// **Zeroed, and it stays zeroed** — T6.H12's rule applied: a type gets a
    /// field-wise constructor only if it holds an owned or `Option` field, and this
    /// one holds neither. Every member is an integer, a `bool`, or a POD sub-struct
    /// of those (`SRefPicMarking`, `SRefPicListReorderSyntax`), plus `eSliceType`,
    /// whose zero discriminant `P_SLICE` is a declared variant — the same audit that
    /// keeps `sWelsEncCtx::new`'s `eSliceType: P_SLICE` honest. The C++ memsets the
    /// slice header before `InitSliceHeadWithBase` fills it, and this is that memset.
    // unsafe-cat: fork-shared(S63)
    #[allow(unsafe_code)]
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    // **`pSubsetSps` stood here and is deleted** (T6.G3): declared by the C++,
    // transcribed by this port, and never written or read by either. The layer's
    // `sLayerInfo` is where every subset consumer looks.
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
    /// Zeroed, and it stays — see [`SSliceHeader::default`]. It is that struct plus
    /// eleven `bool`s and two integers.
    // unsafe-cat: fork-shared(S63)
    #[allow(unsafe_code)]
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::common::wels_common_defs::EWelsNalUnitType;
use crate::safe::plane::PlaneCursor;
pub use crate::safe::bits::BsWriter;
use crate::safe::mb_grid::{MbArray, MbDims, MbWindow};
use crate::safe::mvd_cost::MvdCostCursor;
pub use crate::encoder::set_mb_syn_cabac::SCabacCtx;
use crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj;


/// `TagSlice` — `codec/encoder/core/inc/slice.h:170`. 1584 bytes in the C++; the
/// port's is not pinned (`abi_guard.rs`) and measures 1520 at Phase 6 session B,
/// after `pSliceBsa`, `sSliceBs.pBsBuffer` and the NAL records' `pRawData` went.
#[repr(C)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
    // `pSliceBsa: *mut BsWriter` was here — the C++ `SBitStringAux*` that aimed at
    // either `sSliceBs.sBsWrite` just below or the frame's `pOut->sBsWrite`. It was a
    // cache of one bit that `sSliceBs.pBs`'s nullness already records, and
    // `InitBitStream` replacing `pOut->sBsWrite` every frame killed every slice's
    // copy of it (the encoder probe's eleventh finding, session A). `slice_writer`
    // derives the choice fresh at each use; nothing stores it. Phase 6 session B.
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

impl SSlice {
    /// A slice exactly as `InitSliceThreadInfo` used to find it the moment
    /// `WelsMallocz` returned — **T6.D8**, and it exists for the same reason the
    /// layer's constructor does: the bank is a `Vec<SSlice>` now, so the slices are
    /// built rather than handed a zeroed block.
    ///
    /// **Every field below is that block's zero**, and the port's own audit of the
    /// pieces says so: `SMbCache::default()` is all-zero field by field (T6.C3),
    /// `SCabacCtx::default()` likewise, and `SSliceHeaderExt`/`SRCSlicing`/`SMVUnitXY`
    /// are plain POD. **The one exception is named**: `SWelsSliceBs::default()`
    /// carries `BsWriter::default()`, whose `left_bits` is **32** where a zeroed
    /// `SBitStringAux`'s `iLeftBits` is 0 — a difference `safe/bits.rs` already
    /// settled in writing ("the C++ never uses that struct before calling
    /// `InitBits`"), and `InitBitStream`/`WelsInitSliceCabac` establish the writer
    /// before any slice writes a bit. The sweeps are the measurement.
    pub fn new() -> Self {
        Self {
            // Per-macroblock scratch: 5600 bytes of inline arrays since T6.C3, and
            // every one of them is written before it is read, per macroblock.
            sMbCacheInfo: SMbCache::default(),
            // The slice's own bitstream: `InitSliceBsBuffer` sets `uiSize` and either
            // allocates `pBs` or leaves it null (the frame writer's slot).
            sSliceBs: SWelsSliceBs::default(),
            // Filled from the base slice by `InitSliceHeadWithBase` every frame.
            sSliceHeaderExt: SSliceHeaderExt::default(),
            // Motion-vector search bounds, recomputed per slice by `WelsSliceMdEnc`.
            sMvStartMin: SMVUnitXY::default(),
            sMvStartMax: SMVUnitXY::default(),
            sMvc: [SMVUnitXY::default(); 5],
            uiMvcNum: 0,
            sScaleShift: 0,
            // `InitSliceList` stamps the index; -1 means "not coded this frame" and is
            // written there, not here, exactly as the C++ does after its memset.
            iSliceIdx: 0,
            uiBufferIdx: 0,
            bSliceHeaderExtFlag: false,
            uiLastMbQp: 0,
            bDynamicSlicingSliceSizeCtrlFlag: false,
            uiAssumeLog2BytePerMb: 0,
            uiSliceFMECostDown: 0,
            uiReservedFillByte: 0,
            // CABAC state, re-initialised per slice by `WelsInitSliceCabac`.
            sCabacCtx: SCabacCtx::default(),
            iCabacInitIdc: 0,
            iMbSkipRun: 0,
            iCountMbNumInSlice: 0,
            uiSliceConsumeTime: 0,
            iSliceComplexRatio: 0,
            sSlicingOverRc: SRCSlicing::default(),
        }
    }
}

impl Default for SSlice {
    fn default() -> Self {
        Self::new()
    }
}



#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDynamicSlicingStack {
    pub iStartPos: i32,
    pub iCurrentPos: i32,
    /// The CAVLC rollback snapshot. Was `pBsStackBufPtr`/`uiBsStackCurBits`/
    /// `iBsStackLeftBits` — a pointer and the two accumulator fields, restored
    /// one by one. A detached cursor is `Copy`, so the snapshot is the value.
    pub sBsStack: BsWriter,
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
            sBsStack: BsWriter::new(),
            sStoredCabac: crate::encoder::set_mb_syn_cabac::SCabacCtx::default(),
            iMbSkipRunStack: 0,
            uiLastMbQp: 0,
            pRestoreBuffer: std::ptr::null_mut(),
        }
    }
}



/// `TagSliceBufferInfo` — `codec/encoder/core/inc/svc_enc_frame.h:71`. 16 bytes in
/// the C++; not `repr(C)` since **T6.D8**, because `pSliceBuffer` is a `Vec<SSlice>`.
pub struct SSliceBufferInfo {
    /// The bank's slices, **owned since T6.D8**. `slice_in_layer` resolves a
    /// `SliceIdx` against this, deriving from the bank's root (S28).
    pub pSliceBuffer: Vec<SSlice>,
    pub iMaxSliceNum: i32,
    pub iCodedSliceNum: i32,
}

impl Default for SSliceBufferInfo {
    fn default() -> Self {
        Self {
            iMaxSliceNum: 0,
            iCodedSliceNum: 0,
            pSliceBuffer: Vec::new(),
        }
    }
}

/// **Which array a layer's active SPS lives in** — T6.G3.
///
/// `SLayerInfo` carried two pointers for this, `pSubsetSpsP` and `pSpsP`, and the
/// choice between them was encoded in whether the first was null:
/// `WelsInitCurrentLayer`'s SVC arm aims `pSubsetSpsP` at `pSubsetArray[id]` and then
/// aims `pSpsP` *inside it*, at `SSubsetSps::pSps`; its AVC arm nulls the first and
/// aims `pSpsP` at `pSpsArray[id]`. So `pSpsP` was **not** an index into one array —
/// it named a position in either of two, and the discriminator was a null.
///
/// That is a tagged union with the tag spelled as a null pointer, and this is the
/// same statement with the tag spelled as a tag. It is why the two fields become one:
/// an `Option<SpsId>` plus an `Option<SubsetSpsId>` would leave three states the
/// encoder cannot be in, and one of them (both `Some`, disagreeing) is the bug the
/// null spelling could actually produce.
///
/// The two ids are *different spaces* — `pSpsArray` and `pSubsetArray` are different
/// allocations with different lengths — which is why the arms carry different types
/// even though `WelsInitCurrentLayer` reaches both with the same local.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LayerSps {
    /// `pCtx->pSpsArray[id]` — every simulcast-AVC layer, and the base layer always.
    Avc(SpsId),
    /// `pCtx->pSubsetArray[id].pSps` — an SVC enhancement layer, whose SPS is
    /// embedded in the subset SPS rather than standing alone.
    Subset(SubsetSpsId),
}

/// `TagLayerInfo` — `codec/encoder/core/inc/svc_enc_frame.h:77`. 48 bytes in the C++.
///
/// **Not `repr(C)` since T6.G3**, for `SDqLayer`'s reason one level down:
/// `Option<LayerSps>` has no C shape. The C++'s three parameter-set pointers are two
/// fields here, and neither is an address — see [`LayerSps`], and
/// [`layer_sps`]/[`layer_pps`]/[`layer_subset_sps`] for the resolution.
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    /// The layer's active SPS and which array it is in — `pSubsetSpsP` + `pSpsP`.
    pub eSps: Option<LayerSps>,
    /// The layer's active PPS, as a position in `pCtx->pPPSArray` — `pPpsP`.
    pub iPps: Option<PpsId>,
}

impl Default for SLayerInfo {
    /// **Field-wise, and it has to be** (F56/S21): `Option<LayerSps>` and
    /// `Option<PpsId>` have no niche — `LayerSps`'s payloads are plain integers — so
    /// the all-zero image of this struct is `Some(Avc(SpsId(0)))` and
    /// `Some(PpsId(0))`, a layer that already has parameter sets. `mem::zeroed()`
    /// stood here and would still have compiled.
    fn default() -> Self {
        Self {
            // All scalars, and its own `Default` is its zero; the header is stamped
            // per layer by `WelsInitCurrentLayer` before any NAL is written.
            sNalHeaderExt: SNalUnitHeaderExt::default(),
            eSps: None,
            iPps: None,
        }
    }
}

pub use crate::encoder::encoder_context::SRefList;

/// `TagDqLayer` — `codec/encoder/core/inc/svc_enc_frame.h:84`. 512 bytes.
///
/// This was previously spread over eleven partial copies. The least-truncated one
/// (`slice_multi_threading.rs`) still stood `sSliceBufferInfo` in for
/// `[SSliceBufferInfo; MAX_THREADS_NUM]` with `[u8; 64 * MAX_THREADS_NUM]` — four
/// times the real 64 bytes — and left four pointers as `*mut c_void`.
/// A layer's position in `sWelsEncCtx::ppDqLayerList` — Phase 6 session D.
///
/// The list is `iSpatialLayerNum` entries built once in `InitDqLayers` and freed
/// once in `FreeDqLayer`, and **nothing permutes it**: `WelsSwapDqLayers`
/// reassigns `pCurDqLayer` and stamps the outgoing layer's index, and no
/// `swap`/`rotate`/`retain`/`remove`/`sort`/`drain` touches the list anywhere in
/// the tree (S34, grepped at this face's open — 28 occurrences, zero hits). So a
/// position is a stable identity and an index is faithful where the raw
/// `*mut SDqLayer` was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerIdx(pub u8);

impl LayerIdx {
    #[inline(always)]
    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// A slice's position in the layer's slice **banks** — Phase 6 session D.
///
/// `ppSliceInLayer` was `*mut *mut SSlice`: one pointer per slice, into
/// `sSliceBufferInfo[bank].pSliceBuffer`. Two things invalidate such a pointer and
/// neither invalidates a position:
///
/// * **`ReallocateSliceList` grows a bank** by allocating a new block, copying into
///   it and freeing the old one — every `ppSliceInLayer` entry into that bank is
///   dangling until something re-stamps it. The single-threaded path does re-stamp
///   (`ReallocSliceBuffer` -> `ExtendLayerBuffer` -> the fill loop); the
///   multi-threaded one does not (`ReallocateSliceInThread` updates
///   `sSliceBufferInfo[..].pSliceBuffer` and nothing else), and **the C++ has the
///   same shape** — `phase6_findings.md` F61, Phase 7's.
/// * **`ReOrderSliceInLayer` permutes the pointer array** while the banks stay put.
///
/// The position spelling removes the class rather than the instance: an entry names
/// (bank, offset) and stays true across both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SliceIdx {
    pub bank: u8,
    pub offset: i32,
}

impl SliceIdx {
    /// The value an unfilled entry holds — `ReOrderSliceInLayer` fills the tail of
    /// the array with the banks' uncoded slices, so "unfilled" only ever means
    /// "before the first fill", where the pointer spelling held null.
    pub const NONE: SliceIdx = SliceIdx { bank: u8::MAX, offset: -1 };
}

/// The slice at layer-order position `kiSliceIdx`, resolved against its bank.
///
/// **Derived from the bank's root** (S28): the address is the bank's own
/// `pSliceBuffer` plus the entry's offset, never a pointer narrowed to one slice,
/// because callers walk a slice's inline scratch and `ReOrderSliceInLayer` walks the
/// bank itself. Answers null exactly where the pointer spelling held null — an
/// out-of-range position, an unfilled entry, or a bank that was never allocated.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn slice_in_layer(pCurLayer: Option<&SDqLayer>, kiSliceIdx: i32) -> *mut SSlice {
    let Some(pCurLayer) = pCurLayer else {
        return std::ptr::null_mut();
    };
    if kiSliceIdx < 0 {
        return std::ptr::null_mut();
    }
    // An explicit `&` rather than `(*p).vec[i]`: indexing a `Vec` through a raw
    // parent is an implicit autoref, which rustc denies by default
    // (`dangerous_implicit_autorefs`) and session C met fifteen times.
    let slices: &[SliceIdx] = &(*pCurLayer).ppSliceInLayer;
    let Some(&s) = slices.get(kiSliceIdx as usize) else {
        return std::ptr::null_mut();
    };
    if s.offset < 0 || s.bank as usize >= MAX_THREADS_NUM {
        return std::ptr::null_mut();
    }
    slice_in_bank(pCurLayer, s.bank as usize, s.offset)
}

/// The bank's slices as an **exclusive slice** — the safe twin of
/// [`slice_bank_root`], for the callers that hold the layer `&mut`.
///
/// # Why this can exist, and what F71 actually said
///
/// [`slice_bank_root`] reads the buffer pointer out with `addr_of!` + `as_ptr()`
/// because `&mut Vec<SSlice>` is a `Unique` retag over the `Vec` header, and in
/// every fixed slice mode **all workers resolve bank 0** — so two of them
/// retagging it at once is a data race even though neither writes the `Vec`.
/// That is true, and it is true of **workers**.
///
/// It is not true of a caller holding `&mut SDqLayer`, which by construction has
/// no sibling: a `&mut` to the layer cannot exist while the fork is live, because
/// every worker holds `&SDqLayer`. Measured at S10.3d, **29 of the 38 call sites**
/// in the `slice_bank_root` / `slice_in_bank` / `slice_in_layer` family sit in
/// bodies whose receiver is `&mut sWelsEncCtx` or `&mut SDqLayer`. They were
/// paying the fork's price for single-threaded access; this is what they take.
///
/// `None` for a bank that has not been sized — the state the raw form answered
/// null for.
#[inline]
pub fn slice_bank_mut(pCurLayer: &mut SDqLayer, kiBank: usize) -> Option<&mut [SSlice]> {
    let bank = pCurLayer.sSliceBufferInfo.get_mut(kiBank)?;
    if bank.pSliceBuffer.is_empty() {
        return None;
    }
    Some(&mut bank.pSliceBuffer)
}

/// The slice at `kiOffset` in bank `kiBank`, exclusively — [`slice_in_bank`]'s
/// safe twin. See [`slice_bank_mut`].
#[inline]
pub fn slice_in_bank_mut(
    pCurLayer: &mut SDqLayer,
    kiBank: usize,
    kiOffset: i32,
) -> Option<&mut SSlice> {
    if kiOffset < 0 {
        return None;
    }
    slice_bank_mut(pCurLayer, kiBank)?.get_mut(kiOffset as usize)
}

/// Slice `kiSliceIdx` of the layer, exclusively — [`slice_in_layer`]'s safe twin.
/// Resolves through `ppSliceInLayer` exactly as the raw form does.
#[inline]
pub fn slice_in_layer_mut(pCurLayer: &mut SDqLayer, kiSliceIdx: i32) -> Option<&mut SSlice> {
    if kiSliceIdx < 0 {
        return None;
    }
    let &s = pCurLayer.ppSliceInLayer.get(kiSliceIdx as usize)?;
    if s.offset < 0 || s.bank as usize >= MAX_THREADS_NUM {
        return None;
    }
    slice_in_bank_mut(pCurLayer, s.bank as usize, s.offset)
}

/// The bank's slices as a raw pointer to their **root** — T6.D8, and S28's rule
/// again: `AddSliceBoundary` and `ReOrderSliceInLayer` walk *neighbouring* slices out
/// of the pointer they hold, so the pointer must carry the whole bank's provenance.
/// Answers null for a bank that has not been sized.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn slice_bank_root(pCurLayer: &SDqLayer, kiBank: usize) -> *mut SSlice {
    // **F71.** `&mut Vec<SSlice>` + `as_mut_ptr()` is a `Unique` retag over the
    // three-word `Vec`, and for every fixed slice mode **all** workers resolve bank
    // 0 — so two of them retagging it at once is a data race even though neither
    // writes the `Vec` itself. `addr_of!` + `as_ptr()` reads the buffer pointer out
    // instead: the `Vec` is only ever read, and the pointer carries the buffer's own
    // provenance, so the slices behind it stay writable. S28's rule is unchanged —
    // this is still the bank's root, not a narrowed cursor.
    let bank = std::ptr::addr_of!((*pCurLayer).sSliceBufferInfo[kiBank].pSliceBuffer);
    if (*bank).is_empty() {
        return std::ptr::null_mut();
    }
    (*bank).as_ptr() as *mut SSlice
}

/// The slice at `kiOffset` in bank `kiBank`, derived from the bank's root.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn slice_in_bank(pCurLayer: &SDqLayer, kiBank: usize, kiOffset: i32) -> *mut SSlice {
    let root = slice_bank_root(pCurLayer, kiBank);
    if root.is_null() || kiOffset < 0 {
        return std::ptr::null_mut();
    }
    root.add(kiOffset as usize)
}

// `mb_list_root` (T6.D5, S28's root hand-out) and `mb_at` stood here — the raw
// per-record mints every neighbour walker offset out of. The grid conversion
// (Phase 9 E3) moved all 34 walker parameters and the four list walkers onto
// [`mb_window`] below, and the pair's last callers went with them (whole-tree
// read grep at deletion: zero mentions beyond the definitions). Their S28/S40
// covering test is re-aimed at the mint —
// `mb_window_reaches_its_range_and_disjoint_windows_coexist` — rather than
// deleted (S36). Two `cursor` tags retire.

/// A per-call window over records `[kiFirstMb .. kiFirstMb + kiCount)` of the
/// layer's macroblock grid, current record at `kiCurMb` — **the grid family's
/// mint** (Phase 9 E3), replacing the per-record raw hand-outs that stood above
/// for the neighbour-walker family.
///
/// The layer is shared under the fork (S63, and `&SDqLayer` since S6.A1), so the window is minted *from the
/// raw layer per call*, and the window is the safe object the walkers take.
/// Derivation is S28/S40/F71 verbatim: the array root is read out of the
/// container's header with no reference formed, so concurrent workers minting
/// disjoint windows are sibling derivations, and the exclusive range each window
/// covers is exactly the records its caller owns — never the array struct, never
/// another worker's records. [`MbWindow`]'s own asserts turn any out-of-window
/// access into a coordinate-naming panic (F77) instead of a cross-worker read.
///
/// # Safety
/// `sMbDataP` must be allocated — liveness is the reference's since **S6.A1**,
/// which is why this parameter is no longer raw. The caller must own
/// records `[kiFirstMb .. kiFirstMb + kiCount)` exclusively for the window's
/// lifetime — its own slice's or partition's under the fork, any range
/// single-threaded — and must not use another pointer into that range while the
/// window lives.
#[inline]
// unsafe-cat: fork-shared(S63) — the `&mut [SMB]` it mints, not the layer parameter:
// **S6.A1** made that a `&SDqLayer`, and the window is still minted per call from a
// shared layer so concurrent workers stay sibling derivations (S28/S40/F71).
#[allow(unsafe_code)]
pub unsafe fn mb_window<'a>(
    pCurLayer: &SDqLayer,
    kiFirstMb: i32,
    kiCount: i32,
    kiCurMb: i32,
) -> MbWindow<'a, SMB> {
    let mb = std::ptr::addr_of!((*pCurLayer).sMbDataP);
    let dims = (*mb).dims();
    assert!(
        kiFirstMb >= 0 && kiCount > 0 && (kiFirstMb as usize) + (kiCount as usize) <= dims.count(),
        "mb window [{kiFirstMb}..{}) outside a grid of {}",
        kiFirstMb as i64 + kiCount as i64,
        dims.count()
    );
    let root = (*mb).root_ptr();
    let mbs = std::slice::from_raw_parts_mut(root.add(kiFirstMb as usize), kiCount as usize);
    MbWindow::new(mbs, kiFirstMb as usize, dims.mb_width(), kiCurMb as usize)
}

/// The layer the context is currently working on — **T6.G2's resolution accessor,
/// and the only reader of `sWelsEncCtx::iCurDqLayer`.**
///
/// `pCurDqLayer` used to be a `*mut SDqLayer` alias into `ppDqLayerList`. It is now
/// the *position*, and this resolves it back to the same raw cursor the ~150
/// consumers were already holding: they bind it once at the top of a function
/// (`let pCurDqLayer = current_layer(pEncCtx);`) and offset out of it exactly as
/// before. Nothing downstream changed, which is the point — the identity moved, the
/// idiom did not.
///
/// **The spelling is S40's.** T6.H8 made the list a `Vec<Option<Box<SDqLayer>>>`, and
/// [`ctx_dq_layer`] reads the `Box`'s address without forming a reference to the
/// layer it points at — so repeated calls are still sibling derivations that cannot
/// pop each other's results. That property is what lets a caller keep the cursor
/// across another call to this function, which is what the frame loop does, twice
/// per spatial layer.
///
/// Answers **null** exactly where the old field was null: before any layer is
/// current (`iCurDqLayer == None`), and before the list exists. Every `is_null()`
/// guard in the tree therefore still asks the question it was written to ask.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn current_layer(pCtx: &sWelsEncCtx) -> *mut SDqLayer {
    let Some(idx) = (*pCtx).iCurDqLayer else {
        return std::ptr::null_mut();
    };
    // A `Some` index with no list is a programming error, not a state: every writer
    // names a layer the list already holds. It cannot arise on a live path — the
    // field starts `None` and the list only empties at teardown — so it is asserted
    // rather than handled, and answers null in release for the same reason the
    // field's null answered there.
    debug_assert!(
        !(*pCtx).ppDqLayerList.is_empty(),
        "iCurDqLayer = {idx:?} with no ppDqLayerList"
    );
    debug_assert!(
        idx.get() < MAX_DEPENDENCY_LAYER,
        "iCurDqLayer = {idx:?} is past the largest list InitDqLayers can build"
    );
    ctx_dq_layer(pCtx, idx.get())
}

/// The current layer as a **shared reference** — F240's companion to
/// [`current_layer`], and expressible with no `unsafe` at all.
///
/// F240 named this the cheap piece that moves `current_layer`'s callers and priced
/// it exactly: `iCurDqLayer?` then `ppDqLayerList.get(idx)?.as_deref()`. Every field
/// it touches is already owned and safe, so the whole body is three `?`s.
///
/// **Why a shared borrow is sound where `ctx_dq_layer`'s raw was needed.** F71 wrote
/// that accessor to avoid *retagging* the layer, because two workers resolve the same
/// one per call and an exclusive claim would race. A shared reference makes no
/// exclusive claim — sibling `&SDqLayer`s coexist by construction, which is the whole
/// premise of the S6 flip that put `&SDqLayer` in 28 signatures. Nothing writes
/// `ppDqLayerList` inside the fork; the list is built at `InitDqLayers` and emptied at
/// teardown.
///
/// `None` where `current_layer` answers null, so the two agree on the unset state.
#[inline]
pub fn current_layer_ref(pCtx: &sWelsEncCtx) -> Option<&SDqLayer> {
    let idx = pCtx.iCurDqLayer?;
    debug_assert!(
        idx.get() < MAX_DEPENDENCY_LAYER,
        "iCurDqLayer = {idx:?} is past the largest list InitDqLayers can build"
    );
    pCtx.ppDqLayerList.get(idx.get())?.as_deref()
}

/// [`current_layer_ref`] mutably — the layer the frame loop is stamping.
///
/// **Single-threaded only, and the type says so.** A `&mut sWelsEncCtx` cannot
/// exist while the fork is live (every worker holds `&sWelsEncCtx`), so this
/// accessor is unavailable in exactly the place a `&mut SDqLayer` would be a race.
/// That is the whole difference between it and [`current_layer`]'s raw, which is
/// reachable from anywhere and therefore carries the S63 tag.
#[inline]
pub fn current_layer_mut(pCtx: &mut sWelsEncCtx) -> Option<&mut SDqLayer> {
    let idx = pCtx.iCurDqLayer?;
    debug_assert!(
        idx.get() < MAX_DEPENDENCY_LAYER,
        "iCurDqLayer = {idx:?} is past the largest list InitDqLayers can build"
    );
    pCtx.ppDqLayerList.get_mut(idx.get())?.as_deref_mut()
}

/// Make `kIdx` the current layer — the setter half of [`current_layer`], and the
/// only writer of `sWelsEncCtx::iCurDqLayer`.
///
/// The C++ assigns `pCtx->pCurDqLayer = pCtx->ppDqLayerList[iDid]` at each of its
/// three sites, so each already had the index in hand and was converting it to an
/// address; this keeps the index. `None` un-sets it, which no live path does — it
/// exists so the field's zero has a name.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
pub fn set_current_layer(pCtx: &mut sWelsEncCtx, kIdx: Option<LayerIdx>) {
    debug_assert!(
        kIdx.is_none_or(|i| i.get() < MAX_DEPENDENCY_LAYER),
        "{kIdx:?} is past the largest list InitDqLayers can build"
    );
    pCtx.iCurDqLayer = kIdx;
}

/// A layer's **active SPS**, resolved from [`LayerSps`] — T6.G3.
///
/// This is `sLayerInfo.pSpsP`, and the pointer it replaces pointed into one of two
/// allocations: `pSpsArray[id]`, or the `pSps` *embedded inside* `pSubsetArray[id]`.
/// Both arms are reproduced exactly, including the inner one's spelling: the subset
/// arm takes `addr_of_mut!` of the field rather than a reference to it, so the
/// returned cursor carries the whole `SSubsetSps`'s provenance the way the C++'s
/// `&pSubsetSpsP->pSps` does (S29).
///
/// Null when the layer has no SPS yet, or the array it names is not allocated —
/// the two cases `pSpsP` was null in.
///
/// # Safety
/// `pCtx` must point to a live encoder context and `pCurLayer` to one of its layers.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn layer_sps(pCtx: &sWelsEncCtx, pCurLayer: *const SDqLayer) -> *mut SWelsSPS {
    match (*pCurLayer).sLayerInfo.eSps {
        None => std::ptr::null_mut(),
        Some(LayerSps::Avc(id)) => {
            // A4: the array is the safe reader now; the raw comes back out of it
            // because the far end is `SDqLayer::sLayerInfo`, stage C's. Nothing in
            // the fork writes a parameter set (whole-tree grep at the conversion),
            // so a shared reborrow is the right root for this cursor.
            let arr = (*pCtx).sps_array();
            if arr.is_empty() {
                return std::ptr::null_mut();
            }
            arr.as_ptr().cast_mut().add(id.get())
        }
        Some(LayerSps::Subset(id)) => {
            let arr = (*pCtx).subset_array();
            if arr.is_empty() {
                return std::ptr::null_mut();
            }
            std::ptr::addr_of_mut!((*arr.as_ptr().cast_mut().add(id.get())).pSps)
        }
    }
}

/// A layer's **subset SPS**, or null when the layer is not on the SVC arm — T6.G3.
///
/// `pSubsetSpsP` was null in exactly the AVC case, and this answers null there for
/// the same reason: [`LayerSps::Avc`] has no subset SPS to name.
///
/// # Safety
/// As [`layer_sps`].
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn layer_subset_sps(
    pCtx: &sWelsEncCtx,
    pCurLayer: *const SDqLayer,
) -> *mut SSubsetSps {
    match (*pCurLayer).sLayerInfo.eSps {
        Some(LayerSps::Subset(id)) => {
            let arr = (*pCtx).subset_array();
            if arr.is_empty() {
                return std::ptr::null_mut();
            }
            arr.as_ptr().cast_mut().add(id.get())
        }
        _ => std::ptr::null_mut(),
    }
}

/// A layer's **active PPS**, resolved from its position in `pCtx->pPPSArray` — the
/// `sLayerInfo.pPpsP` of T6.G3, and the most-read of this family (26 sites).
///
/// # Safety
/// As [`layer_sps`].
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn layer_pps(pCtx: &sWelsEncCtx, pCurLayer: *const SDqLayer) -> *mut SWelsPPS {
    let Some(id) = (*pCurLayer).sLayerInfo.iPps else {
        return std::ptr::null_mut();
    };
    let arr = (*pCtx).pps_array();
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    arr.as_ptr().cast_mut().add(id.get())
}

/// The layer's active PPS **as a shared reference** — [`layer_pps`]'s safe twin.
///
/// Same argument as [`ctx_sps_ref`], one indirection out: the callers that read a
/// single field (`uiChromaQpIndexOffset`, three times in `rc.rs`) never hold the
/// pointer, so the raw return buys them nothing.
///
/// **Fork-safe, and refereed rather than asserted.** The `&SDqLayer` this takes is
/// a *shared* borrow, which the two probes in this file
/// (`partition_counters_take_a_shared_layer_borrow_across_the_forked_writes` and
/// `slice_banks_take_a_shared_layer_borrow_across_the_forked_writes`) exist to
/// certify may be held while sibling workers write the layer. The PPS array itself
/// is written only before the fork.
#[inline]
pub fn layer_pps_ref<'a>(pCtx: &'a sWelsEncCtx, pCurLayer: &SDqLayer) -> Option<&'a SWelsPPS> {
    pCtx.pps_array().get(pCurLayer.sLayerInfo.iPps?.get())
}

/// The context's **active SPS**, resolved from its position — T6.G3.
///
/// `sWelsEncCtx::pSps` was a pointer into `pSpsArray`; `iSps` is the index, and this
/// answers the same address, including **null in the two cases the pointer was
/// null**: before `WelsInitEncoderExt` names one, and before the array exists. The
/// spelling is S40's — the array reader hands out the buffer's own address, so
/// repeated calls are independent.
#[inline]
pub fn ctx_sps(pCtx: &sWelsEncCtx) -> *mut SWelsSPS {
    let Some(id) = pCtx.iSps else {
        return std::ptr::null_mut();
    };
    let arr = pCtx.sps_array();
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    debug_assert!((id.get() as i32) < pCtx.iSpsNum.max(1), "iSps past iSpsNum");
    // A4: a **safe fn with a raw return** — forming a pointer needs no `unsafe`,
    // only dereferencing one does, and the dereference belongs to the twenty-two
    // consumers that read a field off it. `wrapping_add` computes the address
    // `.add` computed without making the in-bounds claim, which the
    // `debug_assert` above has always made instead. The return stays raw because
    // every caller holds it beside other reaches into the same context; a
    // reference would borrow the context for its whole lifetime and A7's split is
    // where that gets settled.
    arr.as_ptr().cast_mut().wrapping_add(id.get())
}

/// The context's active SPS **as a shared reference** — [`ctx_sps`]'s safe twin.
///
/// `ctx_sps` hands out a raw pointer for a stated reason: "every caller holds it
/// beside other reaches into the same context; a reference would borrow the
/// context for its whole lifetime". That is true of the callers that *keep* it.
/// It is not true of the callers that read one field on one line and never look
/// again — for those the borrow dies at the semicolon, and the raw deref they
/// perform buys nothing. This is the accessor those callers take.
///
/// `None` in the two cases `ctx_sps` returns null: before `WelsInitEncoderExt`
/// names an SPS, and before the array exists.
#[inline]
pub fn ctx_sps_ref(pCtx: &sWelsEncCtx) -> Option<&SWelsSPS> {
    pCtx.sps_array().get(pCtx.iSps?.get())
}

/// The context's active PPS **as a shared reference** — [`ctx_pps`]'s safe twin,
/// and [`ctx_sps_ref`]'s sibling. Same argument: the callers that read one field
/// and never look again do not need the raw return.
#[inline]
pub fn ctx_pps_ref(pCtx: &sWelsEncCtx) -> Option<&SWelsPPS> {
    pCtx.pps_array().get(pCtx.iPps?.get())
}

/// The context's **active PPS**, resolved from its position — see [`ctx_sps`].
#[inline]
pub fn ctx_pps(pCtx: &sWelsEncCtx) -> *mut SWelsPPS {
    let Some(id) = pCtx.iPps else {
        return std::ptr::null_mut();
    };
    let arr = pCtx.pps_array();
    if arr.is_empty() {
        return std::ptr::null_mut();
    }
    debug_assert!((id.get() as i32) < pCtx.iPpsNum.max(1), "iPps past iPpsNum");
    // Safe fn, raw return — see [`ctx_sps`].
    arr.as_ptr().cast_mut().wrapping_add(id.get())
}

/// The context's current reference picture, resolved through the current dependency
/// layer's reference list.
///
/// # Safety
/// `pCtx` must be a live encoder context past `RequestMemorySvc`.
#[inline]
pub fn ctx_ref_pic<'a>(pCtx: &'a sWelsEncCtx) -> Option<&'a SPicture> {
    let id = (*pCtx).pRefPic?;
    let pRefList = (*pCtx).ref_list((*pCtx).uiDependencyId as usize)?;
    Some(pRefList.pic(id))
}

// `ctx_pic_ref_mut` stood here — the exclusive form of the `PicRef` resolution.
// Its last callers were `JudgeStaticSkip`/`JudgeScrollSkip`'s two
// `ctx_pic_ref_mut(..).planes()` whole-picture retags (F121's live-as-code,
// dark-as-behaviour pair), which session F replaced with the shared route; the
// accessor has had **zero callers anywhere in the tree** since (whole-tree read
// grep at deletion). S18, with E3's tag sweep. One `cursor` tag retires.

/// The picture a [`PicRef`] names — the reconstruction pool through the current
/// dependency layer's reference list, or the spatial source pool through the
/// preprocessor. `SDqLayer::pRefOri` is the one field that holds either; see
/// [`PicRef`]. Shared only: the exclusive form is deleted (see above).
///
/// # Safety
/// As [`ctx_ref_pic`].
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_pic_ref<'a>(pCtx: &'a sWelsEncCtx, r: PicRef) -> Option<&'a SPicture> {
    match r {
        PicRef::Rec(id) => (*pCtx)
            .ref_list((*pCtx).uiDependencyId as usize)
            .map(|pRefList| pRefList.pic(id)),
        PicRef::Src(id) => {
            if (*pCtx).pVpp.is_none() {
                None
            } else {
                // S3.B1: the slot read, and the **shared** half of the pair — this
                // body is fork-reachable, so `ctx_vpp_raw`'s `&mut` would be a
                // data race the MT probes refuse. See `ctx_vpp_ref`.
                Some(crate::encoder::encoder_context::ctx_vpp_ref(pCtx).src_id(id))
            }
        }
    }
}

/// The reconstruction picture this layer is **referencing**, resolved through the
/// reference list the layer was stamped with — `None` before the first inter frame,
/// or if the layer has not been initialised for a frame yet.
///
/// **S37, and the rule this family exists to keep** — half of it enforced now.
/// **S6.A1** tied the returned borrow to `pLayer`, which is a `&'a SDqLayer`, so the
/// compiler holds the layer still for as long as the result lives; the raw parameter
/// was the only reason it could not. What the compiler still cannot see is the
/// *pool*: a caller must not hold the result across a call that resolves another
/// handle in the same pool. Every consumer below takes what it needs — a stride, a
/// plane root, one array element — and drops the borrow in the same statement.
///
/// # Safety
/// `pLayer` must be stamped by `WelsInitCurrentLayer`. Liveness is the reference's
/// to guarantee now; "stamped for this frame" is still the caller's.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn layer_ref_pic<'a>(pLayer: &'a SDqLayer) -> Option<&'a SPicture> {
    let id = (*pLayer).pRefPic?;
    let pRefList = (*pLayer).pRefList;
    if pRefList.is_null() {
        return None;
    }
    Some((*pRefList).pic(id))
}

/// The reference picture's screen-content feature storage, resolved per call —
/// **the dormant Phase-10 pointer's home** after the `sRefPicView` harvest
/// (E3): the pointer lives on `SPicture` and this is the one place the
/// per-frame stamped copy of it is re-derived. Null exactly where the stamped
/// default was null: no reference bound, or no list.
///
/// # Safety
/// `pLayer` is a reference since **S6.A1**, so liveness is no longer the caller's;
/// what remains is that the layer be stamped for the frame in progress.
#[inline]
// unsafe-cat: SCREEN_CONTENT(dormant: Phase 10) — the pointer it hands out; the raw
// layer parameter is the S63 seam (G's)
#[allow(unsafe_code)]
pub unsafe fn layer_ref_feature_storage<'a>(
    pLayer: &'a SDqLayer,
) -> Option<&'a crate::encoder::picture::SScreenBlockFeatureStorage> {
    // **S5.C6c**: the picture owns the storage as an `Option<Box<..>>` now, so the
    // pointer is derived from it rather than copied out of it. The far end —
    // `SWelsME::pRefFeatureStorage` — is still raw and is C5's to convert; this
    // bridges the two without changing which address either side sees. In this tree
    // the answer is always null, because nothing ever fills the `Option` (F229).
    // **S5.C6d**: an `Option<&..>` end to end now — the raw bridge C6c needed is gone
    // with `SWelsME::pRefFeatureStorage`, the far end that had required it.
    layer_ref_pic(pLayer)?.pScreenBlockFeatureStorage.as_deref()
}

// `layer_enc_pic` stood here — "the **source** picture this layer encodes from,
// resolved through the spatial pool the layer was stamped with" (T9.B21).
//
// **S10.2, deleted: it has no caller.** Its twenty-one call sites were the ones
// F254 named — each formed a `PlaneCursor` over the whole plane allocation of a
// picture the fork *writes* (`VaaBackgroundMbDataUpdate`, F117, with
// `bEnableBackgroundDetection` true by default), which is a shared claim on every
// byte racing a concurrent write to any of them. They all read the source through
// [`layer_enc_view`] now, and the S10.1 probe in `svc_mode_decision.rs` is what
// keeps them there: its control is exactly this function's old shape, and it is
// red.
//
// The `unsafe fn` goes with it. Nothing else in the tree resolves a source
// picture by hand.

// `layer_ref_pic_mut` stood here — the `&mut` form of [`layer_ref_pic`], for handing
// out plane roots from the owning buffer. **S18, deleted in T9.B21**: it had no caller
// anywhere in the tree, only four stale imports. It is also the wrong shape for this
// family now — the exclusive borrow it hands out is a fresh whole-picture retag at
// every call (F73), which is precisely what [`layer_enc_pic`] exists to avoid.

// `layer_dec_pic` stood here — the shared form, "the reconstruction picture this
// layer is decoding into". **S18, deleted in T9.C3**: its one caller was the
// `sMvList.is_empty()` test at `svc_mode_decision.rs:1304`, which now asks the
// seam's own `mv_list()` instead, so the shared read route into the
// reconstruction picture has no caller at all. [`layer_rec_view`] below replaces
// it; what is left of the `_mut` form is four plane sites and a deletion.

/// **The reconstruction seam's route from a layer** — D-mt-3 option A, and the
/// replacement for every in-frame use of [`layer_dec_pic_mut`].
///
/// Where `layer_dec_pic_mut` hands out `&mut SPicture` — a fresh whole-picture
/// retag at every call, which is F73 and which Miri reports as a data race on
/// `SRefList` itself when two workers do it at once — this hands out a shared
/// view whose writes go through `&self`. Two workers may hold it at the same
/// time; that is the whole point, and
/// [`crate::encoder::rec_view`] carries the argument for why it is sound.
///
/// `None` means no frame has started (or the picture is unbound), which is what
/// `layer_dec_pic_mut`'s `None` meant.
///
/// # Safety
/// `pLayer` must be stamped by `WelsInitCurrentLayer`, and the frame it stamped
/// must still be the frame in progress. Liveness is the reference's since
/// **S6.A1**; both remaining obligations are the caller's.
#[inline]
pub fn layer_rec_view<'a>(
    pLayer: &'a SDqLayer,
) -> Option<&'a crate::encoder::rec_view::RecPicView> {
    (*pLayer).pRecView.as_ref()
}

/// The layer's **reference** planes as a shared view — the read-only twin of
/// [`layer_enc_view`], built on demand rather than stamped.
///
/// # Why this exists, and why it is built per call
///
/// S10.2 moved the source-plane readers onto the seam, and the cost kernels they
/// feed are reached through a **function pointer**
/// (`PSampleSadSatdCostFunc`), which cannot be generic. Its second operand
/// position receives the enc plane, a scratch buffer **and the reference plane**
/// at different call sites, so all three must be one type; the moment the source
/// operand became `RecCursor` the reference operand had to follow. This is that
/// route (F259).
///
/// Unlike `pEncView`/`pRecView` it is not a layer field, because the reference
/// picture is chosen per macroblock (`pRefPic` moves with the reference index)
/// where the source and reconstruction pictures are stamped once per frame. A
/// build is three plane headers — twelve words, no allocation — against a pool
/// resolution the caller was already paying for.
///
/// Sound for the same reason [`RoPicView::build`] is: it makes no exclusive
/// claim. It uses the seam rather than a `&[u8]` because the slot's single
/// operand type says so — and because arguing about *which* writer might touch a
/// plane is the reasoning S9.0a got wrong.
///
/// # Safety
/// As [`layer_ref_pic`]: the layer must be stamped for the frame in progress.
#[inline]
// unsafe-cat: fork-shared(S63) — inherited from `layer_ref_pic`, whose pool
// resolution this wraps; nothing raw is introduced here.
#[allow(unsafe_code)]
pub unsafe fn layer_ref_view(
    pLayer: &SDqLayer,
) -> Option<crate::encoder::rec_view::RoPicView> {
    Some(crate::encoder::rec_view::RoPicView::build(layer_ref_pic(pLayer)?))
}

/// The frame's source planes, as `layer_rec_view` is its reconstruction planes.
///
/// `None` on a layer whose frame has not been bound yet — the same state
/// `pEncData`'s null roots stood for.
#[inline]
pub fn layer_enc_view<'a>(
    pLayer: &'a SDqLayer,
) -> Option<&'a crate::encoder::rec_view::RoPicView> {
    (*pLayer).pEncView.as_ref()
}

// `layer_dec_pic_mut` stood here, and **F73 was its name**. It handed out
// `&mut SPicture` — a fresh whole-picture retag at every call — and the frame
// loop called it fifteen times per macroblock's worth of work, inside the fork.
// Miri's verdict on the ignored fork/join probe named it directly: "Data race
// detected between (1) retag write on thread `unnamed-2` and (2) retag write of
// type `SRefList` on thread `unnamed-3`", `WelsMdIntraInit` ->
// `layer_dec_pic_mut` -> `SRefList::pic_mut` -> `Pool::get_mut`.
//
// **S18, deleted in T9.C4.** Ten of its call sites became `layer_rec_view`
// (T9.C3), three were second derivations of numbers already on the layer, and
// one — `pDecMb` — was a second derivation of `pCsMb`, proved and deleted. The
// route into the reconstruction picture from inside a frame is now the seam and
// nothing else.

/// Not `repr(C)`: `pRefLayer` is an `Option<LayerIdx>`, which has no C shape.
/// `assert_size!(SDqLayer, ...)` is re-pinned to the measured size in the same
/// commit (phase6.md §5).
pub struct SDqLayer {
    /// This layer's own position in `ppDqLayerList`, stamped at construction —
    /// `WelsSwapDqLayers` needs the *outgoing* layer's index and holds only its
    /// pointer.
    pub iDqIdx: LayerIdx,

    pub sLayerInfo: SLayerInfo,
    /// **S5.D2b — boxed, and the box is the point.**
    ///
    /// This array was *inline* in the layer, and the fork writes into it:
    /// `ReallocateSliceList` takes `&mut ..[kiBank].pSliceBuffer` and
    /// `ReallocateSliceInThread` stores `..[idx].iMaxSliceNum`, each on its own bank.
    /// While those run, a sibling body holding `&SDqLayer` retags the whole layer —
    /// and a whole-struct shared retag racing a write to an inline field is undefined
    /// behaviour (F228 for the context, D2a's probe for this struct). That is the
    /// second and last reason the 37 read-only `*mut SDqLayer` bodies cannot take
    /// `&SDqLayer`; D2a removed the first.
    ///
    /// A `Box` moves the banks to **their own allocation**. The layer then holds one
    /// pointer, which the fork only ever reads; every bank write lands in the boxed
    /// allocation, which no whole-layer retag reaches. It is the same argument F163
    /// makes for `Vec` buffers and C4b relies on for the MVD table — separate
    /// allocations do not share a borrow stack.
    ///
    /// `Box<[T; N]>` rather than `Vec<T>` deliberately: the length is
    /// `MAX_THREADS_NUM` by construction, and keeping it in the type means no site
    /// gains a bounds question it did not have.
    pub sSliceBufferInfo: Box<[SSliceBufferInfo; MAX_THREADS_NUM]>,
    /// One entry per slice in layer order, each naming its bank and its offset
    /// in it — T6.D4. See [`SliceIdx`] and [`slice_in_layer`].
    pub ppSliceInLayer: Vec<SliceIdx>,
    pub sSliceEncCtx: SSliceCtx,
    // `pCsData: [*mut u8; 3]` stood here — the reconstruction picture's three
    // plane roots, stamped by `WelsInitCurrentLayer` every frame.
    //
    // **S10.6, deleted: write-only, and its last three readers were dead
    // bindings.** T9.C2 moved the reconstruction reads onto `pRecView`'s
    // `RecPicView` and left `let pPred = ..mb_cursor(&layer.pCsData, ..)` behind in
    // `WelsEncRecI16x16Y` and `WelsEncRecI4x4Y`, plus a `pPredI4x4` derived from
    // one of them. Nothing read any of the three. `unused_variables` is allowed
    // crate-wide *and* in `svc_encode_mb.rs`'s own module header, so the compiler
    // never said so (F268).
    pub iCsStride: [i32; 3],

    pub iEncStride: [i32; 3],

    /// The layer's macroblock records, **owned** since T6.D5. `InitMbListD` used to
    /// cut one flat `WelsMallocz` block across every layer and hand each its slice,
    /// storing the same pointers a second time in the context's `ppMbListD`; neither
    /// field was a carrier — they were one allocation, cut disjointly and
    /// contiguously, each cut exactly `iMbWidth * iMbHeight`. So each layer owns its
    /// own cut, and the second copy is gone.
    pub sMbDataP: MbArray<SMB>,
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

    /// This layer's reference list — the **owner** of the reconstruction pool that
    /// [`pRefPic`](Self::pRefPic) and [`pDecPic`](Self::pDecPic) name slots in.
    ///
    /// Raw, and for the same reason `sWelsEncCtx::ppRefPicListExt` is raw (session
    /// F's boundary list): the pointee owns, the pointer does not. It exists because
    /// the per-macroblock mode-decision family reaches the reference picture through
    /// `pCurDqLayer` alone — `WelsMdP16x16`, `WelsMdUpdateBGDInfo` and the motion
    /// search take no context — so without it a handle here would name a pool nothing
    /// in scope could open. Stamped by `WelsInitCurrentLayer` from
    /// `ppRefPicListExt[uiDependencyId]`, which is the list these handles belong to.
    pub pRefList: *mut crate::encoder::encoder_context::SRefList,

    pub pRefPic: Option<RecPicId>,
    pub pDecPic: Option<RecPicId>,
    /// The **source** picture this frame encodes from, and the pool it is a slot
    /// of — `pEncData`'s three raw roots said as a handle (T9.B21).
    ///
    /// The pair exists for exactly the reason [`pRefList`](Self::pRefList) does,
    /// one picture over: ten mode-decision consumers reach the source through
    /// `pCurDqLayer` and take no context (`WelsMdI16x16`, `WelsMdIntraChroma`,
    /// `WelsMdP16x16`, `WelsMdP16x8`, `WelsMdP8x16`, `WelsMdP8x8`,
    /// `WelsRecPskip`, and the three F115 names dead in the port), so a handle
    /// here without the pool beside it would name a pool nothing in scope could
    /// open. The reference half solves this with `pRefList`; this is the same
    /// solution for the spatial pool, which lives in `pCtx->pVpp` and is
    /// otherwise unreachable from a layer.
    ///
    /// Raw for `pRefList`'s reason as well: the pointee owns, the pointer does
    /// not. Both are stamped by `WelsInitCurrentLayer` in the same statement
    /// that stamps `pEncData`, from the same already-resolved `idEnc`.
    pub pEncPic: Option<SrcPicId>,
    // `pSrcPool: *mut SrcPicPool` stood here — the spatial picture pool the layer
    // resolves `pEncPic` against, stamped per frame by `WelsInitCurrentLayer`.
    //
    // **S10.7, deleted: write-only.** Its one reader was `layer_enc_pic`, and that
    // accessor went in S10.2 when the last of F254's twenty-one source-plane sites
    // moved onto `pEncView`. Third of `SDqLayer`'s four Sync blockers to go, and
    // the third in a row that a conversion had already emptied without anyone
    // noticing the field was now dead (F268).
    /// **The reconstruction seam** — D-mt-3 option A, built per frame by
    /// `WelsInitCurrentLayer` in the same statement that stamps
    /// [`pCsData`](Self::pCsData), from the same `&mut SPicture`.
    ///
    /// This is the layer's route to the picture *every worker writes*: three
    /// planes and four per-macroblock side arrays, shared and writable through
    /// `&self`. It sits here rather than on the job because every consumer
    /// already reaches the layer and `SDqLayer` cannot carry a lifetime, so the
    /// view holds captured parts instead — see
    /// [`crate::encoder::rec_view`] for the soundness argument, which this
    /// field is one half of.
    ///
    /// **The stability requirement, in one sentence** (F109's shape): while
    /// this is `Some`, nothing may take `&mut` to the same picture through the
    /// pool — `pic_mut(idDec)`, `layer_dec_pic_mut` — because that retag makes
    /// the captured bases stale and, under the fork, races on `SRefList`
    /// itself. `None` between frames is not decoration: `WelsInitCurrentLayer`
    /// rebuilds it every frame, and nothing may read a view built for a frame
    /// that has ended.
    pub pRecView: Option<crate::encoder::rec_view::RecPicView>,

    /// The frame's **source** planes, as a read-only view — the counterpart to
    /// `pRecView` and the read half of the same seam (S9.0).
    ///
    /// `pEncData`'s three raw roots stand for exactly these bytes; this is the
    /// safe spelling of them, and `PlaneCursor`s taken from it bounds-check
    /// against the whole allocation, so the top and left borders a motion search
    /// legally reaches stay reads rather than becoming fresh panics.
    ///
    /// Rebuilt every frame with `pRecView`, and for the same reason: the pool may
    /// hand the next frame a different slot, so a view is only valid for the frame
    /// that built it.
    pub pEncView: Option<crate::encoder::rec_view::RoPicView>,
    // `sRefPicView` and `sDecPicView` stood here — T6.F5's per-frame stamped
    // copies of the two pictures' plane roots/strides/type. Phase 9 E3's harvest:
    // `sDecPicView` had **zero readers** (write-only since the seam took the
    // reconstruction writes, T9.C2); `sRefPicView`'s readers resolve the picture
    // per call through [`layer_ref_pic`] instead (`SPicture::data_ptr_shared`,
    // `stride`, `iPictureType`, and [`layer_ref_feature_storage`] for the
    // dormant Phase-10 pointer). The stamp (`StampLayerPictureViews`) died with
    // the fields.
    /// The *source* pictures behind the reference list — slots of the preprocessor's
    /// spatial pool, resolved through `pCtx->pVpp` (both readers hold the context).
    pub pRefOri: [Option<PicRef>; MAX_REF_PIC_COUNT as usize],

    pub bThreadSlcBufferFlag: bool,
    pub bSliceBsBufferFlag: bool,
    pub iMaxSliceNum: i32,
    /// **S5.D2a — atomics, and the reason is the `&SDqLayer` flip D2/D3 needs.**
    ///
    /// Both arrays live *inline* in the layer and are written **from inside the
    /// encode**: `WelsISliceMdEncDynamic` and `WelsMdInterMbLoopOverDynamicSlice`
    /// stamp `[kiPartitionId]` at six sites between them, one worker per partition.
    /// While that happens, a body holding `&SDqLayer` would have a shared borrow of
    /// the *whole* struct — and a whole-struct shared retag racing a concurrent write
    /// to an inline field is undefined behaviour under Miri's model, which is F228's
    /// finding about the context restated about the layer. As plain `i32` these two
    /// arrays are the reason all 37 read-only `*mut SDqLayer` bodies must stay raw.
    ///
    /// `Relaxed` is the right ordering and the access pattern is why: **every slot has
    /// exactly one writer** — the worker that owns that partition — and every in-fork
    /// read is that same worker reading its own slot back
    /// (`svc_encode_slice.rs:2372`, `slice_multi_threading.rs:1800`,
    /// `CalculateNewSliceNum`). The only cross-partition reads are
    /// `ReOrderSliceInLayer` and `WelsCodeOnePicPartition`, both after the join. No
    /// slot is ever a channel between two threads, so there is nothing for a stronger
    /// ordering to publish.
    ///
    /// `AtomicI32` is `i32`-sized and `i32`-aligned, so the layer's layout is
    /// unchanged.
    pub NumSliceCodedOfPartition: [AtomicI32; MAX_THREADS_NUM],
    pub LastCodedMbIdxOfPartition: [AtomicI32; MAX_THREADS_NUM],
    pub FirstMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub EndMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    /// The first macroblock and the macroblock count of each slice, by layer-order
    /// position — **owned since T6.D6**, and grown by `ExtendLayerBuffer`'s `resize`
    /// where the C++ ran a malloc/copy/free triple for each.
    pub pFirstMbIdxOfSlice: Vec<i32>,
    pub pCountMbNumInSlice: Vec<i32>,

    pub bNeedAdjustingSlicing: bool,

    // `pFeatureSearchPreparation` stood here. Its only writer was `null_mut()`, two
    // lines under the guard that returns `ENC_RETURN_UNSUPPORTED_PARA` for the only
    // configuration that would have allocated it, so `RequestFeatureSearchPreparation`
    // and `UpdateFMESwitch` had no reachable caller at all — S18, deleted rather than
    // converted (T6.D2).
    /// The base layer this one predicts from, as a position in `ppDqLayerList`
    /// rather than as an address — T6.D3, the ordering rule's first application on
    /// this struct. `None` is the raw spelling's null, and it is **written**, never
    /// inherited from a zero image: `Option<LayerIdx>` has no niche to borrow, so
    /// all-zero is not a defined `None` (F56's trap, read the other way), which is
    /// why this conversion lands in the same commit as the constructor below.
    pub pRefLayer: Option<LayerIdx>,
}

impl SDqLayer {
    /// A layer exactly as `InitDqLayers` used to find it the moment `WelsMallocz`
    /// returned — every field is that allocation's zero, with what the zero *meant*
    /// written beside it (T5b's shells recipe).
    ///
    /// **Why there is a constructor at all**: the layer is now `Box`-built, which is
    /// what lets it own containers a `WelsMallocz`'d block may not (S21, read the
    /// way T3.6's `pOut` reads it). A zeroed `Vec` field is UB the moment it drops,
    /// so the construction changes before the ownership does.
    pub fn new(idx: LayerIdx) -> Self {
        Self {
            // Its own position, and the only field here that is not a zero: the
            // C++ never needed it because it compared addresses.
            iDqIdx: idx,
            // `InitDqLayers` fills the whole of this from the parameter sets before
            // the first frame; its `Default` is the same zero block the C++ memset.
            sLayerInfo: SLayerInfo::default(),
            // No bank allocated yet — `InitSliceThreadInfo` fills bank 0 (and, under
            // MT, one per thread) two calls later.
            sSliceBufferInfo: Box::new(std::array::from_fn(|_| SSliceBufferInfo::default())),
            // The slice position array, sized by `InitSliceInLayer` and regrown by
            // `ExtendLayerBuffer`; empty is the raw spelling's null.
            ppSliceInLayer: Vec::new(),
            // Zero here means "no slice segmentation yet"; `InitSlicePEncCtx` sets
            // the mode, the geometry and the map.
            sSliceEncCtx: SSliceCtx::default(),
            // Plane aliases into the reconstructed and source pictures, re-aimed at
            // every frame by `WelsInitCurrentLayer`; null means "no frame started".
            iCsStride: [0; 3],
            // The seam, rebuilt per frame beside `pCsData`; `None` is "no frame
            // started", the same thing the null above means.
            pRecView: None,
            pEncView: None,
            iEncStride: [0; 3],
            // The macroblock records, sized by `InitMbListD` once the geometry is
            // known; empty is the raw spelling's null.
            sMbDataP: MbArray::empty(),
            // Overwritten by the caller on the next two lines; zero is "unsized".
            iMbWidth: 0,
            iMbHeight: 0,
            // `WelsInitCurrentLayer` recomputes this from `pRefLayer` every frame.
            bBaseLayerAvailableFlag: false,
            // Set per frame from the mode-decision configuration.
            bSatdInMdFlag: false,
            // Deblocking parameters, all set by `InitDqLayers` immediately below the
            // allocation; zero is the C++'s "filter on, no offsets".
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            bDeblockingParallelFlag: false,
            // Picture slots, aimed per frame; `None` is "no picture bound", and the
            // list they name slots in is stamped with them (T6.F1).
            pRefList: std::ptr::null_mut(),
            pRefPic: None,
            pDecPic: None,
            pEncPic: None,
            pRefOri: [None; MAX_REF_PIC_COUNT as usize],
            // Both are `iMultipleThreadIdc > 1` predicates that `InitSliceInLayer`
            // computes; false is the single-threaded answer and the honest default.
            bThreadSlcBufferFlag: false,
            bSliceBsBufferFlag: false,
            // Summed from the banks by `InitSliceInLayer`.
            iMaxSliceNum: 0,
            // Partition bookkeeping, reset per frame by `InitSliceBoundaryInfo` and
            // `WelsInitCurrentQBLayerMltslc`.
            NumSliceCodedOfPartition: [const { AtomicI32::new(0) }; MAX_THREADS_NUM],
            LastCodedMbIdxOfPartition: [const { AtomicI32::new(0) }; MAX_THREADS_NUM],
            FirstMbIdxOfPartition: [0; MAX_THREADS_NUM],
            EndMbIdxOfPartition: [0; MAX_THREADS_NUM],
            // The two per-slice-index arrays, sized by `InitSliceInLayer`; empty is
            // the raw spelling's null.
            pFirstMbIdxOfSlice: Vec::new(),
            pCountMbNumInSlice: Vec::new(),
            // "The slicing does not need re-deriving"; `NeedDynamicAdjust` sets it.
            bNeedAdjustingSlicing: false,
            // No base layer until `WelsSwapDqLayers` names one — the raw spelling's
            // null, written rather than inherited.
            pRefLayer: None,
        }
    }
}

impl Default for SDqLayer {
    /// The layer at index 0, which is what the two test fixtures that call this
    /// want (`slice_multi_threading.rs`, `wels_task_management.rs`): both build a
    /// single-layer context. It is no longer `mem::zeroed()` — see `new`.
    fn default() -> Self {
        Self::new(LayerIdx(0))
    }
}

pub use crate::encoder::nal_encap::SWelsNalRaw;

// **`SWelsOut` stood here and is deleted, not converted** (T6.H12). It was a
// four-field struct with a `Default` that zeroed it, and the tree held exactly two
// references to it: its own declaration and that `Default`. Nothing constructed one,
// nothing named it as a field or a parameter, and `nal_encap.rs`'s
// `SWelsEncoderOutput` is what the encoder actually writes NALs through. A type that
// is only ever a declaration is not a zeroed `Default` to audit; it is a type to
// remove — the same reading that deleted `SPicture::unref` at T5.B2 and six context
// fields at T6.G3.

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
pub use crate::encoder::nal_encap::{bs_buffer, SWelsSliceBs};
pub use crate::encoder::param_svc::SWelsSPS;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::param_svc::SSubsetSps;
pub use crate::encoder::param_svc::{PpsId, SpsId, SubsetSpsId};
pub use crate::encoder::param_svc::SSpsSvcExt;
pub use crate::encoder::ref_list_mgr_svc::SMmcoRef;
pub use crate::encoder::ref_list_mgr_svc::SReorderingSyntax;
pub use crate::encoder::ref_list_mgr_svc::SRefPicMarking;
pub use crate::encoder::ref_list_mgr_svc::SRefPicListReorderSyntax;
pub use crate::encoder::rc::SRCSlicing;
pub use crate::encoder::md::SWelsMD;
use crate::encoder::md::{best_pred_intra_chroma_off, mem_pred_chroma_off};
pub use crate::encoder::slice_multi_threading::SSliceThreading;
pub use crate::encoder::slice_multi_threading::SSliceCtx;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::md::SMB;

// Function pointer dispatch table types
pub type PWelsCodingSliceFunc = unsafe extern "C" fn(pCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32;
pub type PWelsSliceHeaderWriteFunc = unsafe extern "C" fn(
    pCtx: &sWelsEncCtx,
    pCurLayer: *mut SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
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
pub fn UpdateNonZeroCountCache(pMb: &SMB, pMbCache: &mut SMbCache) {
    // The `mb_nz.is_null()` guard that stood here was the port's own; the row is an
    // inline array and cannot be absent.
    let mb_nz = &(*pMb).iNonZeroCount;
    let cache_nz = &mut (*pMbCache).iNonZeroCoeffCount;

    cache_nz[9..13].copy_from_slice(&mb_nz[0..4]);
    cache_nz[17..21].copy_from_slice(&mb_nz[4..8]);
    cache_nz[25..29].copy_from_slice(&mb_nz[8..12]);
    cache_nz[33..37].copy_from_slice(&mb_nz[12..16]);

    cache_nz[14..16].copy_from_slice(&mb_nz[16..18]);
    cache_nz[38..40].copy_from_slice(&mb_nz[18..20]);
    cache_nz[22..24].copy_from_slice(&mb_nz[20..22]);
    cache_nz[46..48].copy_from_slice(&mb_nz[22..24]);
}

/// Computes the virtual slice identifier `uiSliceIdc` for a given macroblock linear index.
#[inline]
pub fn WelsMbToSliceIdc(pSliceCtx: Option<&SSliceCtx>, kiMbXY: i32) -> u16 {
    // **S10.3c: the slice context, not the whole layer.** This body reads
    // `sSliceEncCtx` and nothing else, and taking the layer meant a *whole-layer*
    // shared borrow — which is what stopped a caller from holding `&mut` on the
    // macroblock grid (a different field) at the same time, and so is what kept
    // `mb_window`'s raw mint alive on the single-threaded path. Narrowing the
    // parameter to what the body actually reads lets those two borrows coexist.
    let Some(pSliceCtx) = pSliceCtx else {
        return u16::MAX;
    };
    let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
    if kiMbXY >= 0 && kiMbXY < (*pSliceCtx).iMbNumInFrame {
        match map.get(kiMbXY as usize) {
            Some(c) => c.load(Ordering::Relaxed),
            None => u16::MAX,
        }
    } else {
        u16::MAX
    }
}

/// Evaluates spatial neighbor availability masks for intra prediction and motion vector prediction.
pub fn UpdateMbNeighbor(
    pSliceCtx: Option<&SSliceCtx>,
    pMb: &mut SMB,
    kiMbWidth: i32,
    uiSliceIdc: u16,
) {
    // **T9.D9**: `pMb.is_null()` went with the parameter — a reference cannot be
    // absent. **S6.A1**: the layer followed, and its null guard came with it — the
    // absent layer is the `None` arm now, so the obligation stayed in the callee
    // rather than moving to the ~147 call sites. **S10.3c**: and the layer
    // narrowed to `sSliceEncCtx`, the only field this body and `WelsMbToSliceIdc`
    // read — see there for why the width of the borrow mattered.
    let Some(pSliceCtx) = pSliceCtx else {
        return;
    };
    let mut uiNeighborAvailFlag: u32 = 0;
    let kiMbXY = (*pMb).iMbXY;
    let kiMbX = (*pMb).iMbX as i32;
    let kiMbY = (*pMb).iMbY as i32;

    (*pMb).uiSliceIdc = uiSliceIdc;
    let iLeftXY = kiMbXY - 1;
    let iTopXY = kiMbXY - kiMbWidth;
    let iLeftTopXY = iTopXY - 1;
    let iRightTopXY = iTopXY + 1;

    let bLeft = (kiMbX > 0) && (uiSliceIdc == WelsMbToSliceIdc(Some(pSliceCtx), iLeftXY));
    let bTop = (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(Some(pSliceCtx), iTopXY));
    let bLeftTop = (kiMbX > 0) && (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(Some(pSliceCtx), iLeftTopXY));
    let bRightTop = (kiMbX < (kiMbWidth - 1)) && (kiMbY > 0) && (uiSliceIdc == WelsMbToSliceIdc(Some(pSliceCtx), iRightTopXY));

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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn UpdateMbNeighbourInfoForNextSlice(
    pCurDq: Option<&SDqLayer>,
    kiFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
) {
    let Some(pCurDq) = pCurDq else {
        return;
    };
    let kiMbWidth = (*pCurDq).sSliceEncCtx.iMbWidth as i32;
    let mut iIdx = kiFirstMbIdxOfNextSlice;
    let iNextSliceFirstMbIdxRowStart = if (kiFirstMbIdxOfNextSlice % kiMbWidth) != 0 { 1 } else { 0 };
    let iCountMbUpdate = kiMbWidth + iNextSliceFirstMbIdxRowStart;
    let kiEndMbNeedUpdate = kiFirstMbIdxOfNextSlice + iCountMbUpdate;

    // C++ is a do-while: the first macroblock is always updated, even when
    // `kiFirstMbIdxOfNextSlice > kiLastMbIdxInPartition` -- which happens when the
    // boundary lands on the last macroblock of a partition. A `while` skips it.
    // The window is sized to exactly the records this walk touches — the next
    // slice's first row-and-a-bit, bounded by the caller's own partition (the
    // fork-disjointness argument: a worker updates only its partition's records).
    let kiEnd = kiEndMbNeedUpdate
        .min(kiLastMbIdxInPartition + 1)
        .max(kiFirstMbIdxOfNextSlice + 1);
    let mut mbs = mb_window(
        pCurDq,
        kiFirstMbIdxOfNextSlice,
        kiEnd - kiFirstMbIdxOfNextSlice,
        kiFirstMbIdxOfNextSlice,
    );
    loop {
        // T9.E2h (F66's shape B, session J's fix): the idc is read BEFORE the
        // call — once `UpdateMbNeighbor` takes `&mut SDqLayer`, its argument
        // retag would kill a same-call read through the raw. Nothing between
        // the read and the call reallocates.
        let kiSliceIdc = WelsMbToSliceIdc(Some(&pCurDq.sSliceEncCtx), mbs.at(iIdx as usize).iMbXY);
        UpdateMbNeighbor(
            Some(&pCurDq.sSliceEncCtx), mbs.at_mut(iIdx as usize), kiMbWidth, kiSliceIdc);
        iIdx += 1;
        if !((iIdx < kiEndMbNeedUpdate) && (iIdx <= kiLastMbIdxInPartition)) {
            break;
        }
    }
}

// ============================================================================
// Slice Header Initialization & Serialization
// ============================================================================

pub fn WelsSliceHeaderScalExtInit(pCurLayer: Option<&SDqLayer>, pSlice: &mut SSlice) {
    let Some(pCurLayer) = pCurLayer else {
        return;
    };
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    // **T7.C3.** `addr_of_mut!`, not `&mut`: this is *layer* state and every worker
    // runs this function, so a `&mut` retag here is a write as far as the data-race
    // checker is concerned — and it is only ever read. See
    // `StampLayerIdrFlagForSliceType` for the family and why it is the last of it.
    // S10.3d: a shared borrow, not an `addr_of!` — the parameter is already
    // `&SDqLayer` and every use below is a read.
    let pNalHeadExt = &pCurLayer.sLayerInfo.sNalHeaderExt;

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

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderExtInit(pEncCtx: &sWelsEncCtx, pCurLayer: Option<&SDqLayer>, pSlice: &mut SSlice) {
    // **S7.A5**: the `is_null()` guard and its early return retire with the
    // parameter — every context reaching this body comes from a `&mut sWelsEncCtx`
    // held by one of the three fork entry points or the frame loop, never a null.
    let Some(pCurLayer) = pCurLayer else {
        return;
    };
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
    let pParamInternal = &(*pEncCtx).param().sDependencyLayers[uiDid];
    pCurSliceHeader.iFrameNum = pParamInternal.iFrameNum;
    pCurSliceHeader.uiIdrPicId = pParamInternal.uiIdrPicId;

    if let Some(id) = (*pEncCtx).pEncPic {
        // S3.B1: the slot read, and the **shared** half of the pair. This exact
        // line is where Miri refused the first draft's `Box` route (a retag that
        // popped `pSrcPool`) and then the mid-row MT probe refused the second
        // draft's `&mut` route (a retag write raced by every worker). See
        // `ctx_vpp_ref`; the body is fork-reachable and reads one `Copy` field.
        pCurSliceHeader.iPicOrderCntLsb =
            crate::encoder::encoder_context::ctx_vpp_ref(pEncCtx).src_id(id).iFramePoc;
    }

    if (*pEncCtx).eSliceType == EWelsSliceType::P_SLICE {
        pCurSliceHeader.uiNumRefIdxL0Active = 1;
        let num_ref = if !layer_sps(pEncCtx, pCurLayer).is_null() {
            (*layer_sps(pEncCtx, pCurLayer)).iNumRefFrames
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

    let pic_init_qp = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).iPicInitQp
    } else {
        26
    };
    pCurSliceHeader.iSliceQpDelta = ((*pEncCtx).iGlobalQp - pic_init_qp as i32) as i8;

    pCurSliceHeader.uiDisableDeblockingFilterIdc = (*pCurLayer).iLoopFilterDisableIdc;
    pCurSliceHeader.iSliceAlphaC0Offset = (*pCurLayer).iLoopFilterAlphaC0Offset;
    pCurSliceHeader.iSliceBetaOffset = (*pCurLayer).iLoopFilterBetaOffset;
    pCurSliceExt.uiDisableInterLayerDeblockingFilterIdc = (*pCurLayer).uiDisableInterLayerDeblockingFilterIdc;

    if (*pSlice).bSliceHeaderExtFlag {
        WelsSliceHeaderScalExtInit(Some(pCurLayer), pSlice);
    } else {
        let pCurSliceExt = &mut (*pSlice).sSliceHeaderExt;
        pCurSliceExt.bAdaptiveBaseModeFlag = false;
        pCurSliceExt.bAdaptiveMotionPredFlag = false;
        pCurSliceExt.bAdaptiveResidualPredFlag = false;
        pCurSliceExt.bDefaultBaseModeFlag = false;
        pCurSliceExt.bDefaultMotionPredFlag = false;
        pCurSliceExt.bDefaultResidualPredFlag = false;
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WriteReferenceReorder(buf: &mut [u8], pBs: &mut BsWriter, sSliceHeader: *mut SSliceHeader) {
    // **S8.2**: the `pBs.is_null()` arm went with the parameter. It was already
    // unreachable — every caller's writer comes from `slice_writer`, whose two arms
    // both hand back `addr_of_mut!` of a live field and neither of which can be null.
    if sSliceHeader.is_null() {
        return;
    }
    let pRefOrdering = &mut (*sSliceHeader).sRefReordering;
    let eSliceType = (*sSliceHeader).eSliceType;

    if eSliceType != EWelsSliceType::I_SLICE && eSliceType != EWelsSliceType::SI_SLICE {
        BsWriteOneBit(buf, &mut *pBs, 1);
        let mut n: usize = 0;
        loop {
            let uiReorderingOfPicNumsIdc = pRefOrdering.SReorderingSyntax[n].uiReorderingOfPicNumsIdc;
            BsWriteUE(buf, &mut *pBs, uiReorderingOfPicNumsIdc as u32);
            if uiReorderingOfPicNumsIdc == 0 || uiReorderingOfPicNumsIdc == 1 {
                BsWriteUE(buf, &mut *pBs, pRefOrdering.SReorderingSyntax[n].uiAbsDiffPicNumMinus1);
            } else if uiReorderingOfPicNumsIdc == 2 {
                BsWriteUE(buf, &mut *pBs, pRefOrdering.SReorderingSyntax[n].iLongTermPicNum as u32);
            }
            n += 1;
            if uiReorderingOfPicNumsIdc == 3 || n >= 32 {
                break;
            }
        }
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WriteRefPicMarking(buf: &mut [u8], pBs: &mut BsWriter, pSliceHeader: *mut SSliceHeader, pNalHdrExt: *mut SNalUnitHeaderExt) {
    // S8.2, as `WriteReferenceReorder` above: the writer arm was unreachable.
    if pSliceHeader.is_null() || pNalHdrExt.is_null() {
        return;
    }
    let sRefMarking = &mut (*pSliceHeader).sRefMarking;
    let mut n: usize = 0;

    if (*pNalHdrExt).bIdrFlag {
        BsWriteOneBit(buf, &mut *pBs, if sRefMarking.bNoOutputOfPriorPicsFlag { 1 } else { 0 });
        BsWriteOneBit(buf, &mut *pBs, if sRefMarking.bLongTermRefFlag { 1 } else { 0 });
    } else {
        BsWriteOneBit(buf, &mut *pBs, if sRefMarking.bAdaptiveRefPicMarkingModeFlag { 1 } else { 0 });
        if sRefMarking.bAdaptiveRefPicMarkingModeFlag {
            loop {
                let iMmcoType = sRefMarking.SMmcoRef[n].iMmcoType;
                BsWriteUE(buf, &mut *pBs, iMmcoType as u32);
                if iMmcoType == 1 || iMmcoType == 3 {
                    BsWriteUE(buf, &mut *pBs, (sRefMarking.SMmcoRef[n].iDiffOfPicNum - 1) as u32);
                }
                if iMmcoType == 2 {
                    BsWriteUE(buf, &mut *pBs, sRefMarking.SMmcoRef[n].iLongTermPicNum as u32);
                }
                if iMmcoType == 3 || iMmcoType == 6 {
                    BsWriteUE(buf, &mut *pBs, sRefMarking.SMmcoRef[n].iLongTermFrameIdx as u32);
                }
                if iMmcoType == 4 {
                    BsWriteUE(buf, &mut *pBs, (sRefMarking.SMmcoRef[n].iMaxLongTermFrameIdx + 1) as u32);
                }
                n += 1;
                if iMmcoType == 0 || n >= 32 {
                    break;
                }
            }
        }
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderWrite(
    pCtx: &sWelsEncCtx,
    pCurLayer: *mut SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    if pCurLayer.is_null() {
        return;
    }
    // Both halves derived, not threaded (T9.E2b): with `pSlice` a `&mut`, a
    // caller cannot pass a writer cursor *beside* the slice — the argument
    // reborrow would pop it (F114a's mechanism at whole-struct width) — and
    // `pBs` was `slice_writer(pCtx, &slice.sSliceBs)` at the only call site
    // (S54), so both come from the parameter here, sibling raws into disjoint
    // fields. `slice_bs_buffer` reads the same one-bit choice to pick the buffer.
    let pBs = slice_writer(pCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
    let buf = slice_bs_buffer(pCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize);
    let pSps = layer_sps(pCtx, pCurLayer);
    let pPps = layer_pps(pCtx, pCurLayer);
    let pSliceHeader = &mut (*pSlice).sSliceHeaderExt.sSliceHeader;
    // T7.C3: `addr_of_mut!`, not `&mut` — layer state, read-only here, and every
    // worker runs this. `WriteRefPicMarking` takes the raw pointer unchanged.
    let pNalHead = std::ptr::addr_of_mut!((*pCurLayer).sLayerInfo.sNalHeaderExt);

    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy. C++ dereferences both pointers
    // unconditionally; the null guards here follow the surrounding style in this port.
    let pps_id = if !pPps.is_null() { (*pPps).iPpsId } else { 0 };
    let iPpsIdOffset = pParametersetStrategy.map_or(0, |s| s.GetPpsIdOffset(pps_id as i32));
    BsWriteUE(buf, &mut *pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = if !pSps.is_null() { (*pSps).uiLog2MaxFrameNum } else { 4 };
    BsWriteBits(buf, &mut *pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if (*pNalHead).bIdrFlag {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if !pSps.is_null() && (*pSps).uiPocType == 0 {
        BsWriteBits(buf, &mut *pBs, (*pSps).iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(buf, &mut *pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(buf, &mut *pBs, active);
        }
    }

    if !(*pNalHead).bIdrFlag {
        WriteReferenceReorder(buf, &mut *pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, &mut *pBs, pSliceHeader, pNalHead);
    }

    if !pPps.is_null() && (*pPps).bEntropyCodingModeFlag && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(buf, &mut *pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(buf, &mut *pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if !pPps.is_null() && (*pPps).bDeblockingFilterControlPresentFlag {
        match (*pSliceHeader).uiDisableDeblockingFilterIdc {
            0 | 3 | 4 | 6 => {
                BsWriteUE(buf, &mut *pBs, 0);
            }
            1 => {
                BsWriteUE(buf, &mut *pBs, 1);
            }
            2 | 5 => {
                BsWriteUE(buf, &mut *pBs, 2);
            }
            _ => {
                BsWriteUE(buf, &mut *pBs, 0);
            }
        }
        if (*pSliceHeader).uiDisableDeblockingFilterIdc != 1 {
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceAlphaC0Offset as i32) >> 1);
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceBetaOffset as i32) >> 1);
        }
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderExtWrite(
    pCtx: &sWelsEncCtx,
    pCurLayer: *mut SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    if pCurLayer.is_null() {
        return;
    }
    // Both halves derived, not threaded (T9.E2b): with `pSlice` a `&mut`, a
    // caller cannot pass a writer cursor *beside* the slice — the argument
    // reborrow would pop it (F114a's mechanism at whole-struct width) — and
    // `pBs` was `slice_writer(pCtx, &slice.sSliceBs)` at the only call site
    // (S54), so both come from the parameter here, sibling raws into disjoint
    // fields. `slice_bs_buffer` reads the same one-bit choice to pick the buffer.
    let pBs = slice_writer(pCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
    let buf = slice_bs_buffer(pCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize);
    let pSps = layer_sps(pCtx, pCurLayer);
    let pPps = layer_pps(pCtx, pCurLayer);
    let pSubSps = layer_subset_sps(pCtx, pCurLayer);
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let pSliceHeader = &mut pSliceHeadExt.sSliceHeader;
    // T7.C3: `addr_of_mut!`, not `&mut` — layer state, read-only here, and every
    // worker runs this. `WriteRefPicMarking` takes the raw pointer unchanged.
    let pNalHead = std::ptr::addr_of_mut!((*pCurLayer).sLayerInfo.sNalHeaderExt);

    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy. C++ dereferences both pointers
    // unconditionally; the null guards here follow the surrounding style in this port.
    let pps_id = if !pPps.is_null() { (*pPps).iPpsId } else { 0 };
    let iPpsIdOffset = pParametersetStrategy.map_or(0, |s| s.GetPpsIdOffset(pps_id as i32));
    BsWriteUE(buf, &mut *pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = if !pSps.is_null() { (*pSps).uiLog2MaxFrameNum } else { 4 };
    BsWriteBits(buf, &mut *pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if (*pNalHead).bIdrFlag {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if !pSps.is_null() && (*pSps).uiPocType == 0 {
        BsWriteBits(buf, &mut *pBs, (*pSps).iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(buf, &mut *pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(buf, &mut *pBs, active);
        }
    }

    if !(*pNalHead).bIdrFlag {
        WriteReferenceReorder(buf, &mut *pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, &mut *pBs, pSliceHeader, pNalHead);
        if !pSubSps.is_null() && !(*pSubSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
            BsWriteOneBit(buf, &mut *pBs, if pSliceHeadExt.bStoreRefBasePicFlag { 1 } else { 0 });
        }
    }

    if !pPps.is_null() && (*pPps).bEntropyCodingModeFlag && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(buf, &mut *pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(buf, &mut *pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if !pPps.is_null() && (*pPps).bDeblockingFilterControlPresentFlag {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiDisableDeblockingFilterIdc as u32);
        if (*pSliceHeader).uiDisableDeblockingFilterIdc != 1 {
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceAlphaC0Offset as i32) >> 1);
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceBetaOffset as i32) >> 1);
        }
    }

    if !pSubSps.is_null() && !(*pSubSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
        BsWriteBits(buf, &mut *pBs, 4, 0);
        BsWriteBits(buf, &mut *pBs, 4, 15);
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

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsIMbChromaEncode(pEncCtx: &sWelsEncCtx, pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    let pCurLayer = current_layer(pEncCtx);
    let kiEncStride = (*pCurLayer).iEncStride[1];
    // `kiCsStride` stood here: the reconstruction stride, read for the two
    // `pfIDctFourT4` calls alone. T9.C2 gave those calls the seam's cursor, which
    // carries the stride, so the binding has no reader left.
    // **T9.D6**: both cursors are re-derived at each use rather than held across the
    // two `WelsEncRecUV` calls, which retag the whole arena once their parameter is a
    // reference (F66). The chroma prediction's *half* is snapshotted here instead —
    // S53: what has to survive the call is the flag's value, not a pointer — and
    // nothing between here and the last use writes either half flag.
    let kiBestPredOff =
        best_pred_intra_chroma_off((*pMbCache).uiMemPredLumaHalf, (*pMbCache).uiBestPredIntraChromaHalf);
    // **T9.C2**: `pCsCb`/`pCsCr` — two raw cursors into the reconstruction
    // chroma planes — are the seam's two plane views plus this macroblock's
    // chroma origin, which `SPicData`'s `iMbX`/`iMbY` carrier already holds.
    let view_chroma = layer_rec_view(&*pCurLayer)
        .expect("the layer's reconstruction view is built for this frame");
    let (kiChrOrgX, kiChrOrgY) = (*pMbCache).SPicData.chroma_origin();

    // This previously ran both DCTs and then both IDCTs, omitting the two
    // `WelsEncRecUV` calls between them. That is the quantise / zigzag /
    // non-zero-count / chroma-CBP step: without it `pCurRS` reached the IDCT holding
    // raw DCT coefficients, `pCurMb->uiCbp` never got its chroma bits and
    // `pNonZeroCount[16..24]` stayed zero, so no chroma residual was ever coded.
    // S9.0: the source planes through the frame's read-only view; the strides the
    // raw form passed alongside now ride inside the cursors.
    let encView = layer_enc_view(&*pCurLayer)
        .expect("the frame's source view is stamped with pEncData");
    let pFunc = (*pEncCtx).func_list();
    let pfDctFourT4 = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");

    //cb
    pfDctFourT4(
        &mut (*pMbCache).sCoeffLevel,
        &(*pMbCache).SPicData.mb_cursor_ro(encView, 1),
        &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, kiBestPredOff, 8),
    );
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, pMbCache, 0, 1);
    // **T9.C2.** `pCsCb` is the reconstruction Cb plane at this macroblock's
    // chroma origin; the prediction is `sMemPredMb`'s intra-chroma half at stride
    // 8, an owned arena. Slot bypassed per F118 — `pfIDctFourT4` is constant
    // after init, and `kiCsStride` leaves the call because the view carries it.
    idct_four_t4_rec_to_view(
        &view_chroma.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        &(*pMbCache).sMemPredMb[kiBestPredOff..],
        8,
        blk_four4x4(&(*pMbCache).sCoeffLevel, 0),
    );

    //cr
    pfDctFourT4(
        &mut (*pMbCache).sCoeffLevel[64..],
        &(*pMbCache).SPicData.mb_cursor_ro(encView, 2),
        &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, kiBestPredOff + 64, 8),
    );
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, pMbCache, 64, 2);
    idct_four_t4_rec_to_view(
        &view_chroma.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        &(*pMbCache).sMemPredMb[kiBestPredOff + 64..],
        8,
        blk_four4x4(&(*pMbCache).sCoeffLevel, 64),
    );
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsPMbChromaEncode(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, pCurMb: &mut SMB) {
    let pCurLayer = current_layer(pEncCtx);
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let pMbCache = &mut pSlice.sMbCacheInfo;
    // **T9.D6**, as in `WelsIMbChromaEncode` — but note the base: this one starts at
    // `pCoeffLevel + 256` (`svc_encode_slice.cpp:499`) where the intra path starts at
    // 0, which is why `WelsEncRecUV` takes the offset as a parameter rather than
    // deriving it from `iUV`.
    let kiBestPredOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);

    // S9.0: the source planes through the frame's read-only view; the strides the
    // raw form passed alongside now ride inside the cursors.
    let encView = layer_enc_view(&*pCurLayer)
        .expect("the frame's source view is stamped with pEncData");
    let pFunc = (*pEncCtx).func_list();
    let dct = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");
    dct(
        &mut (*pMbCache).sCoeffLevel[256..],
        &(*pMbCache).SPicData.mb_cursor_ro(encView, 1),
        &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, kiBestPredOff, 8),
    );
    dct(
        &mut (*pMbCache).sCoeffLevel[320..],
        &(*pMbCache).SPicData.mb_cursor_ro(encView, 2),
        &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, kiBestPredOff + 64, 8),
    );

    // `svc_encode_slice.cpp:WelsPMbChromaEncode` quantises both chroma planes here.
    // Both calls were missing, so a P macroblock's chroma reached the reconstruction
    // holding raw DCT coefficients and never set its chroma CBP bits — the same
    // defect Phase 4.5 found in `WelsIMbChromaEncode`.
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 256, 1);
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 320, 2);
}

pub fn OutputPMbWithoutConstructCsRsNoCopy(pCtx: &sWelsEncCtx, pDq: Option<&SDqLayer>, pSlice: &mut SSlice, pMb: &SMB) {
    // **S7.A5**: the `is_null()` guard and its early return retire with the
    // parameter — every context reaching this body comes from a `&mut sWelsEncCtx`
    // held by one of the three fork entry points or the frame loop, never a null.
    let Some(pDq) = pDq else {
        return;
    };
    let mb_type = (*pMb).uiMbType;
    //intra have been reconstructed, NO COPY from CS to pDecPic--
    if (IS_INTER(mb_type) && !IS_SKIP(mb_type)) || IS_I_BL(mb_type) {
        let pMbCache = &mut (*pSlice).sMbCacheInfo;
        // **T9.C2 — the in-place family, and the reason `idct_*_in_place` exists
        // at all (F59).** `pRec` *is* `pPred` at all three of these sites: the
        // raw form passed `pDecY`/`pDecU`/`pDecV` twice each, with two identical
        // strides, to spell an aliasing pair Rust cannot build from two
        // references. One seam cursor per plane, read and written by value, says
        // the same thing and says it soundly.
        //
        // The strides go with them: T9.C4 had already replaced a whole-picture
        // retag with `iCsStride`, and the view carries those same numbers —
        // `WelsInitCurrentLayer` stamps `iCsStride[i]` and the view's plane
        // stride from one `SPicture::stride(i)`.
        let view = layer_rec_view(pDq)
            .expect("the layer's reconstruction view is built for this frame");
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let (cx, cy) = (*pMbCache).SPicData.chroma_origin();

        // The luma half of this function was missing: no `pDecY`, no
        // `WelsIDctT4RecOnMb`. Every inter macroblock's luma residual was therefore
        // never added back into the reconstruction, so the encoder's reference frame
        // diverged from what a decoder produces from its own (correct) bitstream —
        // invisible until a second P frame referenced it.
        idct_t4_rec_on_mb_in_place_view(
            &view.plane(0).cursor(lx, ly),
            blk_mb256(&(*pMbCache).sCoeffLevel, 0),
        );
        idct_four_t4_rec_in_place_view(
            &view.plane(1).cursor(cx, cy),
            blk_four4x4(&(*pMbCache).sCoeffLevel, 256),
        );
        idct_four_t4_rec_in_place_view(
            &view.plane(2).cursor(cx, cy),
            blk_four4x4(&(*pMbCache).sCoeffLevel, 320),
        );
    }
}

pub fn UpdateQpForOverflow(pCurMb: &mut SMB, kuiChromaQpIndexOffset: u8) {
    (*pCurMb).uiLumaQp = (*pCurMb).uiLumaQp.wrapping_add(DELTA_QP as u8);
    let clamped_idx = CLIP3_QP_0_51((*pCurMb).uiLumaQp as i32 + kuiChromaQpIndexOffset as i32);
    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[clamped_idx];
}

// ============================================================================
// Macroblock Search & Traversal Loops
// ============================================================================

pub fn WelsGetNextMbOfSlice(pCurDq: Option<&SDqLayer>, kiMbXY: i32) -> i32 {
    let Some(pCurDq) = pCurDq else {
        return -1;
    };
    // **`&`, T9.C5.** Nothing below writes; the `&mut` was a transliteration of the
    // C++'s `SSliceCtx*`, and under multi-threading every worker walks its own
    // slice through this function per macroblock, so it was a whole-`SSliceCtx`
    // retag taken concurrently — Miri's fourth verdict on the fork/join probe,
    // and the same shape as the stride tables' (T9.C4).
    let pSliceSeg = &(*pCurDq).sSliceEncCtx;
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
            && {
                let map: &[AtomicU16] = &pSliceSeg.pOverallMbMap;
                // Was `Option<&u16>` equality, which is false unless *both* are
                // `Some`; the pair match keeps that exactly.
                match (map.get(iNextMbIdx as usize), map.get(kiMbXY as usize)) {
                    (Some(a), Some(b)) => {
                        a.load(Ordering::Relaxed) == b.load(Ordering::Relaxed)
                    }
                    _ => false,
                }
            }
        {
            iNextMbIdx
        } else {
            -1
        }
    } else {
        -1
    }
}

pub fn WelsInitInterMDStruc<'a>(
    pCurMb: &SMB,
    pMvdCostTable: MvdCostCursor<'a>,
    kiMvdInterTableStride: i32,
    pMd: &mut SWelsMD<'a>,
) {
    let luma_qp = (*pCurMb).uiLumaQp as usize;
    (*pMd).iLambda = g_kiQpCostTable[luma_qp];
    // S5.C4b: `!pMvdCostTable.is_null()` — the cursor spells the same test, and the
    // row bump is `offset` rather than `add` because the table it arrives parked in
    // is already biased to the zero-MVD entry (`MvdCostCursor::origin`'s job).
    if !pMvdCostTable.is_none() {
        (*pMd).pMvdCost = pMvdCostTable.offset(luma_qp as i32 * kiMvdInterTableStride);
    }
    (*pMd).iMbPixX = (pCurMb.iMbX as i32) << 4;
    (*pMd).iMbPixY = (pCurMb.iMbY as i32) << 4;
    (*pMd).iBlock8x8StaticIdc.fill(0);
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsISliceMdEnc(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    // **S7.A5**: the `is_null()` guard and its early return retire with the
    // parameter — every context reaching this body comes from a `&mut sWelsEncCtx`
    // held by one of the three fork entry points or the frame loop, never a null.
    let pCurLayer = current_layer(pEncCtx);
    if pCurLayer.is_null() || (*pCurLayer).sMbDataP.dims().count() == 0 || (*pCurLayer).iMbWidth <= 0 || (*pCurLayer).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    // S29 and the D-session playbook (T9.E7): the arena root is derived per
    // use-cluster inside the loop, never held across a slice-taking call —
    // the callees re-derive their own borrows of the same fields (the encode
    // probe's fourth red, session B), and after the flip each window is what
    // keeps the derivation alive.
    let pSliceHdExt = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt);
    let kiSliceFirstMbXY = (*pSliceHdExt).sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*pEncCtx).param().iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }

    loop {
        if !kbCabac {
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                func_list
                    .eEntropyCoder
                    .StashMBStatus(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &mut sDss, &mut (*pSlice).sCabacCtx, (*pSlice).uiLastMbQp, 0);
            }
        }
        iCurMbIdx = iNextMbIdx;
        let mut mbs = mb_window(
            &*pCurLayer,
            kiSliceFirstMbXY,
            iCurMbIdx - kiSliceFirstMbXY + 1,
            iCurMbIdx,
        );

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, mbs.cur_mut(), &mut *pSlice);
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut mbs,
            &mut pSlice.sMbCacheInfo,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            // T9.E2c: the cache borrow lives per loop iteration — the entropy
            // writer below takes the whole slice, and a borrow held across it
            // would not compile (which is the point of F112's one step).
            let pMbCache = &mut pSlice.sMbCacheInfo;
            sMd.iLambda = g_kiQpCostTable[mbs.cur().uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, mbs.cur_mut(), &mut *pMbCache);
            UpdateNonZeroCountCache(mbs.cur(), &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut mbs);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && mbs.cur().uiLumaQp < 50 {
                {  // A6: the block is the shared borrow's scope (F191/F212)
                    let func_list = (*pEncCtx).func_list();
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &mut sDss, &mut (*pSlice).sCabacCtx);
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(mbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        mbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;

        let pMbCache = &mut pSlice.sMbCacheInfo;
        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                func(&*pCurLayer, mbs.cur_mut(), pMbCache.bCollocatedPredFlag, I_SLICE);
            }
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                mbs.cur_mut(),
                sMd.iCostLuma,
                &mut *pSlice,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer.as_ref(), iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsISliceMdEncDynamic(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    // **S7.A5**: the `is_null()` guard and its early return retire with the
    // parameter — every context reaching this body comes from a `&mut sWelsEncCtx`
    // held by one of the three fork entry points or the frame loop, never a null.
    let pBs = slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, not `&mut` — both of these are held across the macroblock loop,
    // whose callees derive their own borrows of the same fields. `sSliceEncCtx` is
    // the dynamic-slice probe's third red (session D): `WelsGetNextMbOfSlice` takes
    // its own `&mut (*pCurDq).sSliceEncCtx` every iteration, which pops the `Unique`
    // this binding held, and `DynSlcJudgeSliceBoundaryStepBack` then reads through
    // the dead tag. `pMbCache` is the encode probe's fourth red (session B).
    let pSliceHdExt = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt);
    let kiSliceFirstMbXY = (*pSliceHdExt).sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();
    if (*pEncCtx).param().iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = dynamic_bs_buffer(pEncCtx, kiPartitionId);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    } else {
        sDss.iStartPos = (*pBs).bits_pos();
    }

    loop {
        iCurMbIdx = iNextMbIdx;
        let mut mbs = mb_window(
            &*pCurLayer,
            kiSliceFirstMbXY,
            iCurMbIdx - kiSliceFirstMbXY + 1,
            iCurMbIdx,
        );

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list
                .eEntropyCoder
                .StashMBStatus(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &mut sDss, &mut (*pSlice).sCabacCtx, (*pSlice).uiLastMbQp, 0);
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, mbs.cur_mut(), &mut *pSlice);
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*pEncCtx).rc_at((*pEncCtx).uiDependencyId as usize).iMaxQp;
            mbs.cur_mut().uiLumaQp = max_qp as u8;
            mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut mbs,
            &mut pSlice.sMbCacheInfo,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            // T9.E2c: the cache borrow lives per loop iteration — the entropy
            // writer below takes the whole slice, and a borrow held across it
            // would not compile (which is the point of F112's one step).
            let pMbCache = &mut pSlice.sMbCacheInfo;
            sMd.iLambda = g_kiQpCostTable[mbs.cur().uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, mbs.cur_mut(), &mut *pMbCache);
            UpdateNonZeroCountCache(mbs.cur(), &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut mbs);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && mbs.cur().uiLumaQp < 50 {
                {  // A6: the block is the shared borrow's scope (F191/F212)
                    let func_list = (*pEncCtx).func_list();
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &mut sDss, &mut (*pSlice).sCabacCtx);
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(mbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(&*slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &(*pSlice).sCabacCtx);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            // **S6.A1 / F239**: derived here, not 100 lines up. A `&SDqLayer` argument
            // anywhere above is a shared retag over the *whole* layer, which pops a
            // field-precise `addr_of_mut!` held across it — the defect Miri found in
            // `WelsInitCurrentLayer`. Deriving at the use keeps this a fresh sibling.
            std::ptr::addr_of_mut!((*pCurLayer).sSliceEncCtx),
            mbs.cur().iMbXY,
            &mut sDss,
        ) {
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                func_list
                    .eEntropyCoder
                    .StashPopMBStatus(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &mut sDss, &mut (*pSlice).sCabacCtx);
                (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx - 1, Ordering::Relaxed);
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }

        mbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                mbs.cur_mut(),
                sMd.iCostLuma,
                &mut *pSlice,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer.as_ref(), iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            (*pSlice).iCountMbNumInSlice = iCurMbIdx - (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId].load(Ordering::Relaxed);
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx, Ordering::Relaxed);
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
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
fn mb_dump(pCurMb: &SMB, pMd: &SWelsMD<'_>, pSlice: & SSlice) {
    if !crate::encoder::dump_enabled(&MB_DUMP, "OH264_MBDUMP") {
        return;
    }
    let mut nzc = String::new();
    for di in 0..24 {
        nzc.push_str(&format!("{},", (*pCurMb).iNonZeroCount[di]));
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
        (*pCurMb).iSadCost,
        (*pCurMb).sP16x16Mv.iMvX,
        (*pCurMb).sP16x16Mv.iMvY,
        (*pCurMb).iRefIndex[0],
        nzc,
        ((*pCurMb).sMv[0]).iMvX,
        ((*pCurMb).sMv[0]).iMvY,
        (*pSlice).iMbSkipRun,
    );
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbLoop<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pSlice: &mut SSlice,
    pWelsMd: &mut SWelsMD<'a>,
    kiSliceFirstMbXY: i32,
) -> i32 {
    // **S7.A5**: the first arm retires with the parameter; the other four are live.
    if current_layer(pEncCtx).is_null() || (*current_layer(pEncCtx)).sMbDataP.dims().count() == 0 || (*current_layer(pEncCtx)).iMbWidth <= 0 || (*current_layer(pEncCtx)).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, held across the MB loop (see `WelsISliceMdEnc`).
    let mut iNumMbCoded = 0;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    // **S5.C4b.** Field-precise on purpose — `&(*pEncCtx).pMvdCostTable` retags the
    // `Vec` header and nothing else, where a `&self` accessor would retag the whole
    // context. Read `MvdCostCursor::origin` for why that difference decides whether
    // this borrow may be held across the macroblock loop it is held across.
    let pMvdCostTable = MvdCostCursor::origin(
        &(&(*pEncCtx).pMvdCostTable)[..],
        (*pEncCtx).iMvdCostTableSize,
    );
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*pEncCtx).param().iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if !kbCabac {
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                func_list.eEntropyCoder.StashMBStatus(
                    slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize),
                    &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)),
                    &mut sDss,
                    &mut (*pSlice).sCabacCtx,
                    (*pSlice).uiLastMbQp,
                    (*pSlice).iMbSkipRun,
                );
            }
        }
        iCurMbIdx = iNextMbIdx;
        let mut mbs = mb_window(
            &*pCurLayer,
            kiSliceFirstMbXY,
            iCurMbIdx - kiSliceFirstMbXY + 1,
            iCurMbIdx,
        );

        //step(1): set QP for the current MB
        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, mbs.cur_mut(), &mut *pSlice);
        }

        //step (2). save some value for future use, initial pWelsMd
        let pMbCache = &mut pSlice.sMbCacheInfo;
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut mbs,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            &mut mbs,
            kiSliceFirstMbXY,
        );

        loop {
            WelsInitInterMDStruc(mbs.cur(), pMvdCostTable, kiMvdInterTableStride, pMd);
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, &mut *pSlice, &mut mbs);
                }
                // T9.E7: fresh window — `pfInterMd` goes through the dispatch
                // slot (q1c cannot attribute it, F111's second limit) and its
                // type carries `*mut SSlice`, so it is a crossing like any
                // named callee.
                let pMbCache = &mut pSlice.sMbCacheInfo;

                //step (4): save from the MD process for future use
                {
                    // Two disjoint fields of one picture (T6.F0 — `pMbCache->pEncSad`
                    // no longer carries either), reached through the seam: the pair of
                    // `&mut Vec` borrows this used to take retagged both arrays whole,
                    // which is the shape no worker may hold under the fork.
                    crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                        layer_rec_view(&*pCurLayer)
                            .expect("the layer's reconstruction picture is bound"),
                        mbs.cur(),
                        pMd,
                    );
                }

                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        &*pCurLayer,
                        mbs.cur_mut(),
                        (*pMbCache).bCollocatedPredFlag,
                        ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                    );
                }
                mb_dump(mbs.cur(), pMd, pSlice);
            }
            //step (5): update cache
            let pMbCache = &mut pSlice.sMbCacheInfo;
            UpdateNonZeroCountCache(mbs.cur(), &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut mbs);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && mbs.cur().uiLumaQp < 50 {
                {  // A6: the block is the shared borrow's scope (F191/F212)
                    let func_list = (*pEncCtx).func_list();
                    (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize),
                        &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)),
                        &mut sDss,
                        &mut (*pSlice).sCabacCtx,
                    );
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(mbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        mbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer.as_ref(), pSlice, mbs.cur());

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                mbs.cur_mut(),
                (*pMd).iCostLuma,
                &mut *pSlice,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer.as_ref(), iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        // Derived at the use, after the loop's own derivations (see `WelsCodeOneSlice`).
        // T9.E2e: the writer is minted here, after the loop — the loop's slot and
        // entropy calls reborrow the whole slice, and a writer held from the top
        // would be popped by the first of them (WelsCodeOneSlice's shape).
        let pBs = slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
        BsWriteUE(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbLoopOverDynamicSlice<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pSlice: &mut SSlice,
    pWelsMd: &mut SWelsMD<'a>,
    kiSliceFirstMbXY: i32,
) -> i32 {
    // **S7.A5**: the first arm retires with the parameter; the other four are live.
    if current_layer(pEncCtx).is_null() || (*current_layer(pEncCtx)).sMbDataP.dims().count() == 0 || (*current_layer(pEncCtx)).iMbWidth <= 0 || (*current_layer(pEncCtx)).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pCurLayer = current_layer(pEncCtx);
    // S29, both: held across the MB loop, whose callees re-derive the same fields.
    // See `WelsISliceMdEncDynamic` for `sSliceEncCtx`'s red and its invalidator.
    let mut iNumMbCoded = 0;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    // **S5.C4b.** Field-precise on purpose — `&(*pEncCtx).pMvdCostTable` retags the
    // `Vec` header and nothing else, where a `&self` accessor would retag the whole
    // context. Read `MvdCostCursor::origin` for why that difference decides whether
    // this borrow may be held across the macroblock loop it is held across.
    let pMvdCostTable = MvdCostCursor::origin(
        &(&(*pEncCtx).pMvdCostTable)[..],
        (*pEncCtx).iMvdCostTableSize,
    );
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();
    if (*pEncCtx).param().iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
        sDss.pRestoreBuffer = dynamic_bs_buffer(pEncCtx, kiPartitionId);
    } else {
        // Minted at the use (T9.E2e): before the loop, so nothing has
        // reborrowed the slice yet; the post-loop use mints its own.
        let pBs = slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
        sDss.iStartPos = (*pBs).bits_pos();
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list.eEntropyCoder.StashMBStatus(
                slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize),
                &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)),
                &mut sDss,
                &mut (*pSlice).sCabacCtx,
                (*pSlice).uiLastMbQp,
                (*pSlice).iMbSkipRun,
            );
        }
        iCurMbIdx = iNextMbIdx;
        let mut mbs = mb_window(
            &*pCurLayer,
            kiSliceFirstMbXY,
            iCurMbIdx - kiSliceFirstMbXY + 1,
            iCurMbIdx,
        );

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, mbs.cur_mut(), &mut *pSlice);
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*pEncCtx).rc_at((*pEncCtx).uiDependencyId as usize).iMaxQp;
            mbs.cur_mut().uiLumaQp = max_qp as u8;
            mbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }

        // step (2): save some values for future use, initialise pWelsMd. Both of
        // these were missing: WelsMdInterInit is what installs the reference-block
        // pointers in pMbCache, so pfInterMd read a null pSample2.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut mbs,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            &mut mbs,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            WelsInitInterMDStruc(mbs.cur(), pMvdCostTable, kiMvdInterTableStride, pMd);
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, &mut *pSlice, &mut mbs);
                }
            }
            // T9.E7: fresh window — `pfInterMd` goes through the dispatch slot
            // (q1c cannot attribute it, F111's second limit) and its type
            // carries `*mut SSlice`, so it is a crossing like any named callee.
            let pMbCache = &mut pSlice.sMbCacheInfo;
            // step (4): save from the MD process for future use
            {
                // As above.
                crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                    layer_rec_view(&*pCurLayer)
                        .expect("the layer's reconstruction picture is bound"),
                    mbs.cur(),
                    pMd,
                );
            }
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        &*pCurLayer,
                        mbs.cur_mut(),
                        (*pMbCache).bCollocatedPredFlag,
                        ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                    );
                }
            }
            UpdateNonZeroCountCache(mbs.cur(), &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut mbs);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && mbs.cur().uiLumaQp < 50 {
                {  // A6: the block is the shared borrow's scope (F191/F212)
                    let func_list = (*pEncCtx).func_list();
                    (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize),
                        &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)),
                        &mut sDss,
                        &mut (*pSlice).sCabacCtx,
                    );
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(mbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(&*slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)), &(*pSlice).sCabacCtx);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            // **S6.A1 / F239**: derived here, not 100 lines up. A `&SDqLayer` argument
            // anywhere above is a shared retag over the *whole* layer, which pops a
            // field-precise `addr_of_mut!` held across it — the defect Miri found in
            // `WelsInitCurrentLayer`. Deriving at the use keeps this a fresh sibling.
            std::ptr::addr_of_mut!((*pCurLayer).sSliceEncCtx),
            mbs.cur().iMbXY,
            &mut sDss,
        ) {
            {  // A6: the block is the shared borrow's scope (F191/F212)
                let func_list = (*pEncCtx).func_list();
                (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                    slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize),
                    &mut *slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs)),
                    &mut sDss,
                    &mut (*pSlice).sCabacCtx,
                );
                (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx - 1, Ordering::Relaxed);
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }

        mbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer.as_ref(), pSlice, mbs.cur());

        {  // A6: the block is the shared borrow's scope (F191/F212)
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                mbs.cur_mut(),
                (*pMd).iCostLuma,
                &mut *pSlice,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer.as_ref(), iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx, Ordering::Relaxed);
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        // Derived at the use, after the loop's own derivations (see `WelsCodeOneSlice`).
        // T9.E2e: the writer is minted here, after the loop — the loop's slot and
        // entropy calls reborrow the whole slice, and a writer held from the top
        // would be popped by the first of them (WelsCodeOneSlice's shape).
        let pBs = slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs));
        BsWriteUE(slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pSlice).sSliceBs), (*pSlice).uiBufferIdx as usize), &mut *pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsPSliceMdEnc(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, kbIsHighestDlayerFlag: bool) -> i32 {
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
    sMd.bMdUsingSad = (*pEncCtx).param().iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoop(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsPSliceMdEncDynamic(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, kbIsHighestDlayerFlag: bool) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:715`. The same assignment was already missing from
    // `WelsPSliceMdEnc` and fixed there; this twin still had the defect, so every
    // dynamic-slice P macroblock costed with SATD where LOW_COMPLEXITY costs with
    // SAD.
    sMd.bMdUsingSad = (*pEncCtx).param().iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoopOverDynamicSlice(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsCodePSlice(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    let pCurLayer = current_layer(pEncCtx);
    // `svc_encode_slice.cpp:733/736` picks `pfInterMd` HERE, per slice, into the
    // shared function list — which under MT is every worker writing the same
    // bytes with no ordering (F132 round 7, the fixed-slice probe's verdict once
    // round 5 stopped aborting first). The stamp is loop-invariant across a
    // frame's slices, so it lives in `PreprocessSliceCoding` now (F71's
    // pattern); only the `kbHighestSpatial` the MD callee needs stays.
    let kbHighestSpatial = if (*pEncCtx).param_opt().is_some() {
        (*pEncCtx).param().iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    WelsPSliceMdEnc(pEncCtx, pSlice, kbHighestSpatial)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsCodePOverDynamicSlice(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    let pCurLayer = current_layer(pEncCtx);
    // `svc_encode_slice.cpp:750/753`, the dynamic-slicing twin of
    // `WelsCodePSlice` — same hoist, same reason (F132 round 7): the per-slice
    // `pfInterMd` stamp lives in `PreprocessSliceCoding` now.
    let kbHighestSpatial = if (*pEncCtx).param_opt().is_some() {
        (*pEncCtx).param().iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    WelsPSliceMdEncDynamic(pEncCtx, pSlice, kbHighestSpatial)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsCodePSlice_c(pCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    WelsCodePSlice(pCtx, pSlice)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsCodePOverDynamicSlice_c(pCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    WelsCodePOverDynamicSlice(pCtx, pSlice)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsISliceMdEnc_c(pCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    WelsISliceMdEnc(pCtx, pSlice)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsISliceMdEncDynamic_c(pCtx: &sWelsEncCtx, pSlice: &mut SSlice) -> i32 {
    WelsISliceMdEncDynamic(pCtx, pSlice)
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSliceHeaderWrite_c(
    pCtx: &sWelsEncCtx,
    pCurLayer: *mut SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    WelsSliceHeaderWrite(pCtx, pCurLayer, pSlice, pParametersetStrategy);
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSliceHeaderExtWrite_c(
    pCtx: &sWelsEncCtx,
    pCurLayer: *mut SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    WelsSliceHeaderExtWrite(pCtx, pCurLayer, pSlice, pParametersetStrategy);
}

pub static g_pWelsSliceCoding: [[PWelsCodingSliceFunc; 2]; 2] = [
    [WelsCodePSlice_c, WelsCodePOverDynamicSlice_c],
    [WelsISliceMdEnc_c, WelsISliceMdEncDynamic_c],
];

pub static g_pWelsWriteSliceHeader: [PWelsSliceHeaderWriteFunc; 2] = [
    WelsSliceHeaderWrite_c,
    WelsSliceHeaderExtWrite_c,
];

/// The buffer a slice's writer is positioned in.
///
/// SHIM(phase3) -> **the thread pool's own bitstream buffers only.** T3.6 made the
/// frame output owned, so this function's second branch is a plain borrow of a
/// `Vec`; the first resolves the pool slot the slice was claimed into
/// (`thread_bs_buffer`) and goes through `bs_buffer` for the slice. What is left
/// here retires with `pThreadBsBuffer` itself, in **Phase 7** (F12/P10).
///
/// A `BsWriter` is a position and carries no buffer, so every write has to be told
/// which one. A slice writes into exactly one of two: the thread buffer it was
/// claimed into when `InitSliceBsBuffer` gave it an independent output buffer, or
/// the frame-level `pOut->sBsBuffer` when it shares. **The discriminator is
/// `sSliceBs.pBs`'s nullness** — `InitSliceBsBuffer` allocates `pBs` exactly when
/// it gives the slice its own writer and leaves it null exactly when the slice
/// shares `pOut->sBsWrite`, so the one bit the deleted `pSliceBsa` pointer used to
/// carry is already recorded, and it travels with the struct through
/// `ReallocateSliceList`'s `copy_nonoverlapping` where the pointer had to be
/// re-stamped. Deriving it back from `iMultipleThreadIdc` and `uiSliceMode` would
/// re-read parameters that can move between allocation and use; the allocation
/// cannot. If Phase 7 makes `pBs` an `Option<Vec<u8>>`, this reads `is_some()` and
/// nothing else moves. See [`slice_writer`] for the writer half of the same choice.
///
/// **T3.5 added the CABAC callers.** `WelsSpatialWriteMbSynCabac` and
/// `WelsInitSliceCabac` derive their buffer here rather than gaining a
/// parameter. That kept the entropy dispatch signature untouched through Phase
/// 3, and T4b.1 vindicated it: with both arms deriving the buffer themselves,
/// `EntropyCoder::WelsSpatialWriteMbSyn` needed no `buf` at all. The arithmetic
/// coder no longer reaches the output any other way.
/// **T9.E6 narrowed the slice half to the field it touches** (the D-session
/// playbook, step 3 of the slice family): the parameter is the `SWelsSliceBs`
/// field and the bank slot, not the slice — so entering this function retags
/// nothing of `SSlice`, and the 8 callers that hold an `sMbCacheInfo` cursor
/// across this call stop being F66 hazards structurally. The ctx half stays
/// raw for G–H (the `pOut` arm is context state).
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn slice_bs_buffer<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pSliceBs: *mut SWelsSliceBs,
    kiBufferIdx: usize,
) -> &'a mut [u8] {
    if (*pSliceBs).pBs.is_some() {
        thread_bs_buffer(pEncCtx, kiBufferIdx, (*pSliceBs).uiSize)
    } else {
        // **F217, measured in S3.B1**: this arm is main-thread-only. The fence
        // (`bIndependenceBsBuffer = iMultipleThreadIdc > 1 && mode != SM_SINGLE_SLICE`)
        // guarantees `pBs = Some` for every slice of a forked layer, and the frame
        // dispatch peels `SM_SINGLE_SLICE` onto the inline path before either fork
        // branch. Probed, not argued: a thread-identity assert here survived the
        // eight MT+single-slice configs and the full 895-case debug sweep.
        // (S3.B1: `pOut` is an `Option<Box<_>>` now, resolved through the slot read
        // so the returned slice carries the output block's own provenance rather
        // than a child of a context retag — F71, and prohibition 2's rule for a
        // body whose context parameter is raw.)
        let pOut = crate::encoder::encoder_context::ctx_out_raw(pEncCtx);
        // The slice comes off the `Vec`'s own buffer (`addr_of_mut!` + `as_mut_ptr`,
        // F71's spelling) rather than through an autoref of `*pOut`, which would be
        // a `Unique` retag over the whole output block.
        let v = std::ptr::addr_of_mut!((*pOut).sBsBuffer);
        std::slice::from_raw_parts_mut((*v).as_mut_ptr(), (*v).len())
    }
}

/// The thread bitstream buffer a slice was claimed into:
/// `pSliceThreading->pThreadBsBuffer[uiBufferIdx]`, `uiSize` bytes.
///
/// This replaces `SWelsSliceBs.pBsBuffer`, which was a cache of exactly this:
/// both C++ stamp sites (`InitOneSliceInThread`, `SetOneSliceBsBufferUnderMultithread`)
/// wrote `pThreadBsBuffer[idx]` with the same `idx` the first stores in
/// `uiBufferIdx`. Resolved at each use, the pool's slot is named by index and no
/// struct aliases its allocation. The pool itself is **Phase 7's** (F12/P10) — see
/// `bs_buffer`, whose one remaining job this is.
///
/// # Safety
/// `pEncCtx` and `(*pEncCtx).pSliceThreading` must be live, and `kiSlot` must be a
/// thread slot a slice was claimed into (`InitOneSliceInThread`), with `kuiSize`
/// that slice's `sSliceBs.uiSize`.
///
/// **T9.E6**: the two slice reads became the two values they read — the slot and
/// the size — so this function's parameters no longer name `SSlice` at all
/// (the D-session playbook's `usize`-offset rule, S54's shape).
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn thread_bs_buffer<'a>(pEncCtx: &'a sWelsEncCtx, kiSlot: usize, kuiSize: u32) -> &'a mut [u8] {
    // **T7.C5, and the spelling is F71's.** The buffer is owned now, so the root has
    // to come out of a `Vec` — and it comes out through `addr_of!` on *this worker's
    // element*, never through a reborrow of the array or the struct that every worker
    // shares. `as_ptr() as *mut u8` returns the buffer's own provenance without a
    // `Unique` retag on the three-word header, which is the difference between a
    // read two workers can make at once and a race.
    // (S3.B1: the block resolves through `ctx_slice_threading_raw` — the slot
    // read keeps the answer's provenance the heap block's own, so this worker's
    // element cursor survives whatever happens to the context.)
    let pSmt = crate::encoder::slice_multi_threading::ctx_slice_threading_raw(pEncCtx);
    let v = std::ptr::addr_of!((*pSmt).pThreadBsBuffer[kiSlot]);
    bs_buffer((*v).as_ptr() as *mut u8, kuiSize)
}

/// The CABAC restore buffer for one picture partition —
/// `sWelsEncCtx::pDynamicBsBuffer[kiPartitionId]`, **owned since T7.C5**.
///
/// Under `SM_SIZELIMITED_SLICE` with CABAC, renormalisation can rewrite bytes already
/// emitted, so stepping back over a slice boundary has to restore the bytes as well as
/// the coder state; this is the scratch it restores from
/// (`SDynamicSlicingStack::pRestoreBuffer`). One per partition, and a partition is a
/// worker, so the buffers are disjoint by the same static partition the bs slots are.
///
/// Null when the encoder was not built for that combination — which is what the raw
/// array's null entry meant, and `svc_set_mb_syn_cavlc.rs` tests for it at both ends
/// of the stash/restore pair.
///
/// The derivation is `thread_bs_buffer`'s and for the same reason: `addr_of!` on this
/// worker's element, never a reborrow of the array every worker shares (F71).
///
/// # Safety
/// `pEncCtx` must be live, and `kiPartitionId` within `MAX_THREADS_NUM`.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn dynamic_bs_buffer(pEncCtx: &sWelsEncCtx, kiPartitionId: usize) -> *mut u8 {
    let v = std::ptr::addr_of!((*pEncCtx).pDynamicBsBuffer[kiPartitionId]);
    if (*v).is_empty() {
        return std::ptr::null_mut();
    }
    (*v).as_ptr() as *mut u8
}

/// The writer a slice's bitstream is positioned by — the other half of
/// [`slice_bs_buffer`]'s choice, and the same discriminator.
///
/// This replaces `SSlice.pSliceBsa`, the C++ `SBitStringAux*` that `InitSliceBsBuffer`
/// aimed at the slice's own `sSliceBs.sBsWrite` (independent buffer, `pBs` allocated)
/// or at the frame's `pOut->sBsWrite` (shared, `pBs` null). Storing that pointer was
/// the encoder probe's eleventh finding (session A): `InitBitStream` replaces
/// `pOut->sBsWrite` wholesale every frame, and a write through the parent kills every
/// slice's cached copy — S29's boundary clause, which no spelling fixes. Derived
/// fresh at every use there is nothing to kill: `addr_of_mut!` on the raw parent, so
/// the pointer carries the parent's tag and no retag sits between them.
///
/// **Not a `bool`.** A second copy of the choice would be the deleted defect in a new
/// spelling; `pBs`'s nullness is the one place the choice lives.
///
/// # Safety
/// `pSlice` must be live, and `pEncCtx` and `(*pEncCtx).pOut` must be live when
/// `pBs` is null.
/// **T9.E6 narrowed the slice half to the field it touches** — see
/// [`slice_bs_buffer`]: the parameter is the `SWelsSliceBs` field, so this call
/// retags nothing of `SSlice` and the 9 callers holding slice cursors across it
/// stop being F66 hazards. The ctx half stays raw for G–H (the `pOut` arm).
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn slice_writer(pEncCtx: &sWelsEncCtx, pSliceBs: *mut SWelsSliceBs) -> *mut BsWriter {
    if (*pSliceBs).pBs.is_some() {
        std::ptr::addr_of_mut!((*pSliceBs).sBsWrite)
    } else {
        // S3.B1: as `slice_bs_buffer`'s arm — the same fence, the same slot read.
        let pOut = crate::encoder::encoder_context::ctx_out_raw(pEncCtx);
        std::ptr::addr_of_mut!((*pOut).sBsWrite)
    }
}

/// **F71's residue, closed — T7.C3.** The one write `WelsCodeOneSlice` made into
/// *layer* state rather than slice state, lifted out of the slice encode to the
/// thread that owns the frame.
///
/// `svc_encode_slice.cpp:1655` sets `pNalHeadExt->bIdrFlag = 1` inside
/// `WelsCodeOneSlice`, which every worker runs once per slice — so N workers write
/// the same layer byte concurrently. The C++ makes that write too and brackets it in
/// nothing, so it was never a port divergence; it was the last shared write the
/// fork/join carried, the reason the MT Miri probe was `#[cfg_attr(miri, ignore)]`,
/// and the thing F71 handed to Phase 9.
///
/// **It did not need Phase 9, because the write is loop-invariant across the fork**,
/// and that is checkable rather than plausible:
///
/// * the condition is `pEncCtx->eSliceType == I_SLICE`, a **frame**-level value fixed
///   before the fork — every worker takes the same arm;
/// * the value written is the constant `true` — no worker can observe a different one;
/// * **no worker reads `bIdrFlag` before its own write.** The only code a worker runs
///   ahead of `WelsCodeOneSlice` is `InitOneSliceInThread`, `SetSliceBoundaryInfo` and
///   `WritePrefixNalForSlice`, and none of the three touches the layer header — the
///   prefix NAL's own idr argument is derived from `eNalType`, not from this field
///   (`nal_encap.rs`, `_kbIdrFlag`, unused). Every read that matters —
///   `WelsSliceHeaderExtInit`, both `g_pWelsWriteSliceHeader` bodies, the
///   `g_pWelsSliceCoding` index — is downstream of the write **in the same worker**,
///   and `AppendSliceToFrameBs`'s read is after the join.
///
/// So running it once on the calling thread immediately before the fork produces
/// byte-for-byte what N workers racing to write the same constant produced, and the
/// race is gone rather than serialised. Placed *at the fork*, deliberately, and not
/// merged into `WelsInitCurrentLayer`'s frame-level stamp
/// (`encoder_ext.rs`, `pNalHdExt.bIdrFlag = ...`): that stamp is hundreds of lines
/// upstream, the two disagree whenever `eSliceType == I_SLICE` with `iFrameNum != 0`,
/// and moving the write across everything in between would be a behaviour change
/// rather than a hoist. Each single-threaded caller keeps it exactly where the
/// statement stood, one line above its own `WelsCodeOneSlice`.
///
/// **S10.3a: safe.** The `# Safety` clause that stood here required "a live context
/// whose current layer is set for this frame"; both halves are now carried by the
/// types — the `&mut sWelsEncCtx` and `current_layer_mut`'s `Option`.
pub fn StampLayerIdrFlagForSliceType(pEncCtx: &mut sWelsEncCtx) {
    // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if pEncCtx.eSliceType != EWelsSliceType::I_SLICE {
        return;
    }
    // **S10.3a.** This body runs on the calling thread *before* either fork
    // spawns — T7.C3 hoisted it there precisely so the write would not race — so
    // the `&mut sWelsEncCtx` it already holds can reach the layer directly.
    // `current_layer`'s raw was carrying a fork-shared tag for a body that is by
    // construction not in the fork.
    let Some(pCurLayer) = current_layer_mut(pEncCtx) else {
        return;
    };
    pCurLayer.sLayerInfo.sNalHeaderExt.bIdrFlag = true;
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsCodeOneSlice(pEncCtx: &sWelsEncCtx, pCurSlice: &mut SSlice, kiNalType: i32) -> i32 {
    // **S7.A5**: the `is_null()` guard and its early return retire with the
    // parameter — every context reaching this body comes from a `&mut sWelsEncCtx`
    // held by one of the three fork entry points or the frame loop, never a null.
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, not `&mut` — this is held across `g_pWelsWriteSliceHeader`, whose
    // two bodies derive `&mut` to the same field (`:816`, `:902`) and popped it
    // (the encode probe's first red on the walk, Phase 6 session B).
    //
    // **S10.3d: that premise looks expired and is not acted on here.** Both of
    // those bodies use `addr_of_mut!` now, not `&mut` — a raw derivation creates
    // no reference and so cannot pop a shared parent — and neither *writes*
    // through it (checked: zero writes through `pNalHead` in either). A shared
    // `&pCurLayer.sLayerInfo.sNalHeaderExt` here would very likely stand. The
    // referee is the two full-encode Miri probes S29's red came from, not a
    // reading of the code, so this is left for a checkpoint that runs them.
    let pNalHeadExt = std::ptr::addr_of_mut!((*pCurLayer).sLayerInfo.sNalHeaderExt);

    let kiDynamicSliceFlag = if (*pEncCtx).param_opt().is_some() {
        let did = (*pEncCtx).uiDependencyId as usize;
        if (*pEncCtx).param().sSpatialLayers[did].sSliceArgument.uiSliceMode == SliceMode::SM_SIZELIMITED_SLICE {
            1
        } else {
            0
        }
    } else {
        0
    };

    if (*pEncCtx).eSliceType == EWelsSliceType::I_SLICE {
        // The `pNalHeadExt->bIdrFlag = 1` of `svc_encode_slice.cpp:1655` is not here:
        // it is layer state, every caller runs it one line above this call, and
        // T7.C3 explains why moving it is byte-neutral. `sScaleShift` is the slice's
        // own and stays. The assert is the hoist's contract, checked where the
        // statement used to be.
        debug_assert!(
            (*pNalHeadExt).bIdrFlag,
            "StampLayerIdrFlagForSliceType was not run before WelsCodeOneSlice on an I_SLICE"
        );
        (*pCurSlice).sScaleShift = 0;
    } else {
        let kuiTemporalId = (*pNalHeadExt).uiTemporalId;
        let ref_temporal = ctx_ref_pic(pEncCtx).map_or(0, |p| p.uiTemporalId);
        (*pCurSlice).sScaleShift = if kuiTemporalId != 0 { kuiTemporalId.saturating_sub(ref_temporal) } else { 0 };
    }

    WelsSliceHeaderExtInit(pEncCtx, pCurLayer.as_ref(), &mut *pCurSlice);

    //RomRC init slice by slice
    // A2: the raw accessor answered null on an empty array and this is the one
    // caller that asked. `rc_at` panics instead (T9.H3's ruling for `ctx_ltr_at`),
    // so the emptiness question moves back out to the array, where it was.
    if !(*pEncCtx).rc().is_empty() {
        let pWelsSvcRc = (*pEncCtx).rc_at((*pEncCtx).uiDependencyId as usize);
        if pWelsSvcRc.bGomRC {
            crate::encoder::rc::GomRCInitForOneSlice(&mut *pCurSlice, pWelsSvcRc.iBitsPerMb);
        }
    }

    let ext_hdr_idx = if (*pCurSlice).bSliceHeaderExtFlag { 1 } else { 0 };
    (g_pWelsWriteSliceHeader[ext_hdr_idx])(
        pEncCtx,
        pCurLayer,
        &mut *pCurSlice,
        // T6.I1: was guarded on the table being non-null; it is owned now.
        (*pEncCtx).func_list().pParametersetStrategy.as_deref(),
    );

    let pic_init_qp = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).iPicInitQp
    } else {
        26
    };
    (*pCurSlice).uiLastMbQp =
        (pic_init_qp as i32 + (*pCurSlice).sSliceHeaderExt.sSliceHeader.iSliceQpDelta as i32) as u8;

    // **S6.A1 / F239**: re-derived. `WelsSliceHeaderExtInit(.., pCurLayer.as_ref(), ..)`
    // above is a whole-layer shared retag that pops the `addr_of_mut!` bound at the top
    // of this function; the two uses before it are still on the live tag, this one is not.
    let idr_idx = (*std::ptr::addr_of!((*pCurLayer).sLayerInfo.sNalHeaderExt)).bIdrFlag as usize;
    let func = g_pWelsSliceCoding[idr_idx][kiDynamicSliceFlag];
    let iEncReturn = func(pEncCtx, &mut *pCurSlice);
    if iEncReturn != ENC_RETURN_SUCCESS {
        return iEncReturn;
    }

    // The buffer and the writer are derived here, at their use, and not at the
    // top of the function: `slice_bs_buffer` hands back a `&mut` over the whole
    // frame buffer, and every macroblock write inside `func` above derived its
    // own — S29's boundary clause, only ordering fixes it (the encode probe's
    // fifth red, session B). The writer moved down with T9.E2b: the `&mut
    // *pCurSlice` argument reborrows above pop a slice-resident writer cursor,
    // so it is minted after the last of them.
    let pBs = slice_writer(pEncCtx, std::ptr::addr_of_mut!((*pCurSlice).sSliceBs));
    WelsWriteSliceEndSyn(
        slice_bs_buffer(pEncCtx, std::ptr::addr_of_mut!((*pCurSlice).sSliceBs), (*pCurSlice).uiBufferIdx as usize),
        &mut *pBs,
        std::ptr::addr_of_mut!((*pCurSlice).sCabacCtx),
        (*pEncCtx).param().iEntropyCodingModeFlag != 0,
    );

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
/// `pSlice` must be valid; `pBs` must be its writer (`slice_writer`) and `buf` the
/// buffer that writer is positioned in (`slice_bs_buffer`).
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsWriteSliceEndSyn(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    pCabacCtx: *mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
    bEntropyCodingModeFlag: bool,
) {
    if bEntropyCodingModeFlag {
        crate::encoder::set_mb_syn_cabac::WelsCabacEncodeFlush(buf, &mut *pCabacCtx);
        // Both coders now count in the same units over the same buffer, so
        // handing the position back is an assignment. This used to be
        // `set_pos(end.offset_from(buf.as_ptr()))` around a pointer the coder
        // had derived from an offset in the first place; `BsWriter::set_pos`
        // existed for this one caller and is deleted with it.
        *pBs = BsWriter::at(crate::encoder::set_mb_syn_cabac::WelsCabacEncodePos(
            &mut *pCabacCtx,
        ));
    } else {
        crate::encoder::vlc_encoder::BsRbspTrailingBits(buf, &mut *pBs);
        crate::encoder::vlc_encoder::BsFlush(buf, &mut *pBs);
    }
}

// ============================================================================
// Dynamic Slicing & Boundary Enforcement
// ============================================================================

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn AddSliceBoundary(
    pEncCtx: &sWelsEncCtx,
    pCurSlice: &mut SSlice,
    pSliceCtx: *mut SSliceCtx,
    // **T9.D11**: this was `pCurMb: &SMB`, and the reference — not the read — was the
    // defect. A `&`/`&mut` *argument* is **strongly protected** for the whole call, and
    // this one covers a macroblock that `UpdateMbNeighbourInfoForNextSlice` re-borrows
    // mutably while walking the MB list one frame below (`UpdateMbNeighbor(.., &mut
    // *pMb, ..)`). A `*mut SMB` parameter carried no protector, so the conversion in
    // T9.D9 created the conflict rather than exposing one. The body wanted a single
    // `i32`; it takes the `i32` (F114).
    kiCurMbIdx: i32,
    iFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
) {
    // **S7.A5**: the context arm retires with the parameter; `pSliceCtx` is still raw.
    if pSliceCtx.is_null() {
        return;
    }
    let pCurLayer = current_layer(pEncCtx);
    let buf_idx = (*pCurSlice).uiBufferIdx as usize;
    let pSliceBuffer = slice_bank_root(&*pCurLayer, buf_idx);
    let iCodedSliceNum = (*pCurLayer).sSliceBufferInfo[buf_idx].iCodedSliceNum;
    let iCurMbIdx = kiCurMbIdx;
    let iCurSliceIdc = {
        let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
        map[iCurMbIdx as usize].load(Ordering::Relaxed)
    };
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
        {
            let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
            crate::encoder::slice_multi_threading::fill_mb_map(
                map,
                iFirstMbIdxOfNextSlice,
                kiLastMbIdxInPartition - iFirstMbIdxOfNextSlice + 1,
                iNextSliceIdc,
            );
        }

        UpdateMbNeighbourInfoForNextSlice(pCurLayer.as_ref(), iFirstMbIdxOfNextSlice, kiLastMbIdxInPartition);
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn DynSlcJudgeSliceBoundaryStepBack(
    pEncCtx: &sWelsEncCtx,
    pCurSlice: &mut SSlice,
    pSliceCtx: *mut SSliceCtx,
    // **T9.D11**, as `AddSliceBoundary` above: the body reads one field of the
    // macroblock, and a reference parameter would protect it across the
    // `AddSliceBoundary` call below.
    kiCurMbIdx: i32,
    pDss: *mut SDynamicSlicingStack,
) -> bool {
    let iCurMbIdx = kiCurMbIdx;
    let kiActiveThreadsNum = (*pEncCtx).iActiveThreadsNum;
    let kiPartitionId = ((*pCurSlice).iSliceIdx % (kiActiveThreadsNum as i32)) as usize;
    let kiEndMbIdxOfPartition = (*current_layer(pEncCtx)).EndMbIdxOfPartition[kiPartitionId];

    let kbCurMbNotFirstMbOfCurSlice = (iCurMbIdx > 0)
        && {
            let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
            map[iCurMbIdx as usize].load(Ordering::Relaxed)
                == map[(iCurMbIdx - 1) as usize].load(Ordering::Relaxed)
        };
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
        // **F69 — the lock the raw translation dropped, restored.**
        // `svc_encode_slice.cpp:1776-1791` brackets exactly these two statements in
        // `WelsMutexLock(&pSliceThreading->mutexSliceNumUpdate)` when
        // `iMultipleThreadIdc > 1`, with the C++'s own comment on the lock line
        // saying what it is for: "lock the acessing to this variable:
        // pSliceCtx->iSliceNumInFrame". `68c4f6a5 "Raw translation"` kept the two
        // statements and neither lock, and nothing has locked
        // `mutexSliceNumUpdate` in this crate since — the field was initialised in
        // `RequestMtResource`, destroyed in `ReleaseMtResource`, and never used.
        //
        // `pSliceCtx` is `addr_of_mut!((*pCurLayer).sSliceEncCtx)` — the **layer's**
        // slice context, shared by every worker on the dynamic path — so the `+= 1`
        // is a read-modify-write racing across threads, and `AddSliceBoundary`
        // writes `pOverallMbMap` and the next slice's header through the same
        // shared parent (the C++ calls it "complex memory operation" on the line
        // above the lock). A lost increment leaves
        // `iEncodeSliceNum != iSliceNumInFrame` in `ReOrderSliceInLayer`, which
        // answers `ENC_RETURN_UNEXPECTED` and the frame comes back **empty** —
        // F3's shape in 18 of the 25 hits of its before-arm.
        //
        // The null-mutex arm of `with_wels_mutex` runs the closure unlocked, which
        // is the C++'s `iMultipleThreadIdc <= 1` path: `pSliceThreading` is null
        // there, because `RequestMtResource` only runs above 1.
        let pSmtMutex: Option<&std::sync::Mutex<()>> = {
            let bMt = (*pEncCtx).param_opt().is_some()
                && (*pEncCtx).param().iMultipleThreadIdc > 1;
            if bMt {
                // S3.B1: the block resolves through the slot read and the shared
                // reference is formed *into its own allocation* — every worker may
                // hold one at once, and locking takes `&self`.
                let pSmt =
                    crate::encoder::slice_multi_threading::ctx_slice_threading_raw(pEncCtx);
                if pSmt.is_null() { None } else { Some(&(*pSmt).mutexSliceNumUpdate) }
            } else {
                None
            }
        };
        crate::encoder::slice_multi_threading::with_wels_mutex(pSmtMutex, || {
            AddSliceBoundary(pEncCtx, pCurSlice, pSliceCtx, iCurMbIdx, iCurMbIdx, kiEndMbIdxOfPartition);
            (*pSliceCtx).iSliceNumInFrame.fetch_add(1, Ordering::Relaxed);
        });
        return true;
    }

    false
}

// ============================================================================
// Memory Management, Buffer Allocation & Dynamic Expansion
// ============================================================================

// `AllocMbCacheAligned` and `FreeMbCache` stood here: eight `WelsMallocz` calls per
// slice and their eight `WelsFree`s, for scratch the slice always owned alone.
// **T6.C3** made all eight inline arrays of `SMbCache`, so the slice's block carries
// them and there is nothing to allocate, nothing to fail, and nothing to free —
// including on `ReallocateSliceList`'s error paths, which freed the *new* list while
// the first `kiMaxSliceNumOld` entries still held the old list's copied pointers.
// The `+ 16` accommodation (F14) and its reasoning moved onto `SMbCache::sMemPredMb`
// with the buffer.

pub fn InitSliceBoundaryInfo(
    pCurLayer: &mut SDqLayer,
    pSliceArgument: &SSliceArgument,
    kiSliceNumInFrame: i32,
) -> i32 {
    // A7: the parameter is a reference now, so its null arm is gone with the raw.
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

        let count: &mut Vec<i32> = &mut (*pCurLayer).pCountMbNumInSlice;
        count[iSliceIdx as usize] = iMbNumInSlice;
        let first: &mut Vec<i32> = &mut (*pCurLayer).pFirstMbIdxOfSlice;
        first[iSliceIdx as usize] = iFirstMBInSlice;
    }

    ENC_RETURN_SUCCESS
}

pub fn SetSliceBoundaryInfo(pCurLayer: Option<&SDqLayer>, pSlice: &mut SSlice, kiSliceIdx: i32) -> i32 {
    let Some(pCurLayer) = pCurLayer else {
        return ENC_RETURN_UNEXPECTED;
    };
    if (*pCurLayer).pFirstMbIdxOfSlice.is_empty() || (*pCurLayer).pCountMbNumInSlice.is_empty() {
        return ENC_RETURN_UNEXPECTED;
    }

    let first: &[i32] = &(*pCurLayer).pFirstMbIdxOfSlice;
    let count: &[i32] = &(*pCurLayer).pCountMbNumInSlice;
    (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = first[kiSliceIdx as usize];
    (*pSlice).iCountMbNumInSlice = count[kiSliceIdx as usize];

    ENC_RETURN_SUCCESS
}

// `AllocateSliceMBBuffer` stood here and forwarded to `AllocMbCacheAligned`. With the
// cache's eight buffers inline it had an empty body and two callers that checked its
// return, so it went with them (S18).

/// `bIndependenceBsBuffer` is recorded as `sSliceBs.pBs`'s nullness and nowhere
/// else — `slice_writer` and `slice_bs_buffer` read it back from there. The C++'s
/// `pBsWrite` parameter (the frame writer this stamped into `pSliceBsa` in the
/// shared arm) is gone with the field.
/// **T7.C4 — the slice owns its bitstream.** The `CMemoryAlign` block and the
/// `ENC_RETURN_MEMALLOCERR` arm behind it are gone: `vec![0; n]` is the `WelsMallocz`
/// this replaces, zeros included, and its failure is a panic-on-OOM — the same trade
/// every owned buffer in this port has made since T3.6. `pMa` goes with the
/// allocation.
pub fn InitSliceBsBuffer(
    pSlice: &mut SSlice,
    bIndependenceBsBuffer: bool,
    iMaxSliceBufferSize: i32,
) -> i32 {
    (*pSlice).sSliceBs.uiSize = iMaxSliceBufferSize as u32;
    (*pSlice).sSliceBs.uiBsPos = 0;

    if bIndependenceBsBuffer {
        (*pSlice).sSliceBs.pBs = Some(vec![0u8; iMaxSliceBufferSize.max(0) as usize]);
        (*pSlice).sSliceBs.uiBsSize = iMaxSliceBufferSize as u32;
    } else {
        (*pSlice).sSliceBs.pBs = None;
        (*pSlice).sSliceBs.uiBsSize = 0;
    }

    ENC_RETURN_SUCCESS
}

/// Releases one slice bank — **T6.D8, finished at T7.C4.**
///
/// The bank has been a `Vec<SSlice>` since T6.D8, so `clear()` was already the old
/// `WelsFree(slice_array)`. What still needed walking was `sSliceBs.pBs`, one
/// `CMemoryAlign` block per slice held by raw pointer — and **that walk is gone with
/// the pointer**: the buffer is the slice's own `Option<Vec<u8>>`, so dropping the
/// bank drops every one of them, in the same order, with nothing to null out and
/// nothing to get wrong on an error path. `pMa` goes with the walk, and this is the
/// last thing `FreeDqLayer` had to release by hand.
pub fn FreeSliceBuffer(pDqLayer: &mut SDqLayer, kiBank: usize) {
    let bank: &mut Vec<SSlice> = &mut (*pDqLayer).sSliceBufferInfo[kiBank].pSliceBuffer;
    bank.clear();
    bank.shrink_to_fit();
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceList(
    pDqLayer: &mut SDqLayer,
    kiBank: i32,
    kiMaxSliceNum: i32,
    kiMaxSliceBufferSize: i32,
    bIndependenceBsBuffer: bool,
) -> i32 {
    if kiMaxSliceBufferSize <= 0 {
        return ENC_RETURN_UNEXPECTED;
    }

    for iSliceIdx in 0..kiMaxSliceNum {
        let pSlice = slice_in_bank(pDqLayer, kiBank as usize, iSliceIdx);
        if pSlice.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }

        (*pSlice).iSliceIdx = iSliceIdx;
        (*pSlice).uiBufferIdx = 0;
        (*pSlice).iCountMbNumInSlice = 0;
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let iRet = InitSliceBsBuffer(&mut *pSlice, bIndependenceBsBuffer, kiMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitAllSlicesInThread(pCtx: &mut sWelsEncCtx) -> i32 {
    let pCurDqLayer = current_layer(pCtx);
    for iSliceIdx in 0..(*pCurDqLayer).iMaxSliceNum {
        let slice_ptr = slice_in_layer(pCurDqLayer.as_ref(), iSliceIdx);
        if slice_ptr.is_null() {
            return ENC_RETURN_UNEXPECTED;
        }
        (*slice_ptr).iSliceIdx = -1;
    }

    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        (*pCurDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iCodedSliceNum = 0;
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn InitOneSliceInThread<'a>(
    pCtx: &'a sWelsEncCtx,
    kiSlcBuffIdx: i32,
    kiDlayerIdx: i32,
    kiSliceIdx: i32,
) -> Option<&'a mut SSlice> {
    // **S10.3a: the out-parameter is gone and the slice comes back by reference.**
    // It was `pSlice: *mut *mut SSlice` — the C's out-pointer idiom — and every
    // caller then wrote `&mut *pSlice` and `addr_of_mut!((*pSlice).sSliceBs)` for
    // the rest of its body. `Option<&mut SSlice>` says the same two outcomes (a
    // slice, or `ENC_RETURN_UNEXPECTED`'s null bank) and hands the callers a
    // reference they can use without a single raw operation.
    //
    // The `unsafe` stays here, and only here: the bank is still reached through
    // [`slice_in_bank`]'s raw root, which is F71's shape and step 3's remaining
    // subject. What this moves is the *boundary* — one audited derivation instead
    // of one per caller body.
    let pCurDq = current_layer(pCtx);
    let slc_ptr = if (*pCurDq).bThreadSlcBufferFlag {
        let kiCodedNumInThread = (*pCurDq).sSliceBufferInfo[kiSlcBuffIdx as usize].iCodedSliceNum;
        slice_in_bank(&*pCurDq, kiSlcBuffIdx as usize, kiCodedNumInThread)
    } else {
        slice_in_bank(&*pCurDq, 0, kiSliceIdx)
    };
    if slc_ptr.is_null() {
        return None;
    }

    let slice = &mut *slc_ptr;
    slice.iSliceIdx = kiSliceIdx;
    slice.uiBufferIdx = kiSlcBuffIdx as u32;

    slice.sSliceBs.uiBsPos = 0;
    slice.sSliceBs.iNalIndex = 0;
    // The C++ stamped `sSliceBs.pBsBuffer = pThreadBsBuffer[kiSlcBuffIdx]` here;
    // `uiBufferIdx` above already names that slot, and `thread_bs_buffer` reads it.
    slice.sSliceBs.uiSize = (*pCtx).iFrameBsSize as u32;

    Some(slice)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceThreadInfo(
    pCtx: &mut sWelsEncCtx,
    pDqLayer: &mut SDqLayer,
    kiDlayerIndex: i32,
) -> i32 {
    let iThreadNum = if pCtx.param_opt().is_some() {
        pCtx.param().iMultipleThreadIdc as i32
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
        // Built rather than handed a zeroed block: `SSlice::new` writes every field
        // that block's zero stood for (T6.D8). Field-wise, not built-once-and-cloned —
        // `SSlice` is 6544 bytes of mostly inline scratch since T6.C3 and carries no
        // `Clone`, and the compiler can flatten a field-wise constructor into the
        // `Vec`'s storage where a clone would build and copy.
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].pSliceBuffer =
            (0..iMaxSliceNum as usize).map(|_| SSlice::new()).collect();

        // T9.E2h, shape B as above: the flag is read before the call.
        let kbSliceBsBufferFlag = (*pDqLayer).bSliceBsBufferFlag;
        let iRet = InitSliceList(
            pDqLayer,
            iIdx,
            iMaxSliceNum,
            pCtx.iSliceBufferSize[kiDlayerIndex as usize],
            kbSliceBsBufferFlag,
        );
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        iIdx += 1;
    }

    while (iIdx as usize) < MAX_THREADS_NUM {
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iMaxSliceNum = 0;
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].iCodedSliceNum = 0;
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].pSliceBuffer = Vec::new();
        iIdx += 1;
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceInLayer(
    pCtx: &mut sWelsEncCtx,
    pDqLayer: &mut SDqLayer,
    kiDlayerIndex: i32,
) -> i32 {
    // S29, and F13's production site: this was `&mut ...sSliceArgument`, whose
    // `Unique` retag popped the tag of `InitDqLayers`'s `pDlayer` — a pointer into
    // the *same* layer, derived one call up and read again after this function
    // returns. Found by the encoder aliasing probe (Phase 6 session A) on its first
    // run. It became `addr_of_mut!` then, and **A7 retires the cursor entirely**:
    // `SSliceArgument` is `Copy` (`codec_api.rs:577`) and this body only reads it,
    // so §4.6's fourth shape applies — copy it out. Nothing writes it in between
    // (`InitSliceThreadInfo` reads `iMultipleThreadIdc` and nothing else of the
    // parameter block), so the copy is the re-read. `InitSliceBoundaryInfo` takes
    // `&SSliceArgument` now, and `ReallocateSliceList` takes the slice *mode* — the
    // one field it ever read.
    let sSliceArgument = pCtx.param().sSpatialLayers[kiDlayerIndex as usize].sSliceArgument;
    let kuiSliceMode = sSliceArgument.uiSliceMode;

    (*pDqLayer).bSliceBsBufferFlag = pCtx.param().iMultipleThreadIdc > 1
        && kuiSliceMode != SliceMode::SM_SINGLE_SLICE;

    (*pDqLayer).bThreadSlcBufferFlag = pCtx.param().iMultipleThreadIdc > 1
        && kuiSliceMode == SliceMode::SM_SIZELIMITED_SLICE;

    let iRet = InitSliceThreadInfo(pCtx, pDqLayer, kiDlayerIndex);
    if iRet != ENC_RETURN_SUCCESS {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pDqLayer).iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        (*pDqLayer).iMaxSliceNum += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    // One `Vec` sized to the layer's slice count; `WelsMallocz` zeroed the block
    // and `SliceIdx::NONE` is that zero's meaning — "no slice at this position yet".
    (*pDqLayer).ppSliceInLayer = vec![SliceIdx::NONE; (*pDqLayer).iMaxSliceNum as usize];

    (*pDqLayer).pFirstMbIdxOfSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];
    (*pDqLayer).pCountMbNumInSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];

    // T9.E2h, shape B as above: the count is read before the call.
    let kiMaxSliceNum = (*pDqLayer).iMaxSliceNum;
    let iRet2 = InitSliceBoundaryInfo(pDqLayer, &sSliceArgument, kiMaxSliceNum);
    if iRet2 != ENC_RETURN_SUCCESS {
        return iRet2;
    }

    let mut iStartIdx = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        for iSliceIdx in 0..(*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum {
            let slices: &mut Vec<SliceIdx> = &mut (*pDqLayer).ppSliceInLayer;
            slices[(iStartIdx + iSliceIdx) as usize] =
                SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
        }
        iStartIdx += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    ENC_RETURN_SUCCESS
}

pub fn InitSliceHeadWithBase(pSlice: &mut SSlice, pBaseSlice: &SSlice) {
    let pBaseSHExt = &(*pBaseSlice).sSliceHeaderExt;
    let pSHExt = &mut (*pSlice).sSliceHeaderExt;

    (*pSlice).bSliceHeaderExtFlag = (*pBaseSlice).bSliceHeaderExtFlag;
    // T6.G3: the C++ copies each id and then the pointer derived from it
    // (`svc_encode_slice.cpp:1169-1172`). The pointers are gone; the ids they were
    // derived from are these two lines, unchanged.
    pSHExt.sSliceHeader.iPpsId = pBaseSHExt.sSliceHeader.iPpsId;
    pSHExt.sSliceHeader.iSpsId = pBaseSHExt.sSliceHeader.iSpsId;
}

pub fn InitSliceRefInfoWithBase(pSlice: &mut SSlice, pBaseSlice: &SSlice, kuiRefCount: u8) {
    let pBaseSHExt = &(*pBaseSlice).sSliceHeaderExt;
    let pSHExt = &mut (*pSlice).sSliceHeaderExt;

    pSHExt.sSliceHeader.uiRefCount = kuiRefCount;
    pSHExt.sSliceHeader.sRefMarking = pBaseSHExt.sSliceHeader.sRefMarking;
    pSHExt.sSliceHeader.sRefReordering = pBaseSHExt.sSliceHeader.sRefReordering;
}

#[inline]
pub fn InitSliceRC(pSlice: &mut SSlice, kiGlobalQp: i32) -> i32 {
    if kiGlobalQp < 0 {
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

/// `ReallocateSliceList` — svc_encode_slice.cpp:1206, as a **`resize`** since
/// **T6.D8**, and a real defect closes with it.
///
/// **The defect** (handed over by session C, whose face 2 closed the `SMbCache`
/// half of the same aliasing): the C++ allocated a new block,
/// `copy_nonoverlapping`d the old slices into it — **their raw `sSliceBs.pBs`
/// included** — and then, on any of three error paths, `FreeSliceBuffer`d the *new*
/// list. Freeing the new list walks all `kiMaxSliceNumNew` entries and frees each
/// `pBs`; the first `kiMaxSliceNumOld` of those pointers are the *old* list's, and
/// the old list is still live and still owns them. Every one of those is a double
/// free waiting for the caller to release the old bank.
///
/// **Under `Vec<SSlice>::resize_with` there is no second owner to free.** The
/// existing slices *move* into the grown buffer rather than being copied beside a
/// live original, so each `pBs` is held by exactly one `SSlice` at every point, and
/// the error paths return the bank as it stands instead of freeing a list that
/// shares pointers with a live one. The only reachable difference from the C++ is on
/// an error path the gates cannot reach — allocation failure, or a negative global
/// QP — where this leaves the bank grown with an uninitialised tail and the C++ left
/// a double free; both then propagate `ENC_RETURN_*` to the same caller.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ReallocateSliceList(
    pCtx: &sWelsEncCtx,
    kuiSliceMode: SliceMode,
    pDqLayer: *mut SDqLayer,
    kiBank: usize,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    // A7 / S54: the parameter was a `*mut SSliceArgument` this body only ever read
    // one field of. It is that field now — a `Copy` enum — so there is no cursor to
    // keep alive across the callers' whole-context calls, and its null arm goes with
    // it (a value cannot be null).
    if pDqLayer.is_null() || kiMaxSliceNumNew < kiMaxSliceNumOld {
        return ENC_RETURN_INVALIDINPUT;
    }

    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let iMaxSliceBufferSize = (*pCtx).iSliceBufferSize[kiCurDid];
    let bIndependenceBsBuffer = (*pCtx).param().iMultipleThreadIdc > 1
        && kuiSliceMode != SliceMode::SM_SINGLE_SLICE;

    {
        let bank: &mut Vec<SSlice> = &mut (*pDqLayer).sSliceBufferInfo[kiBank].pSliceBuffer;
        if bank.is_empty() {
            return ENC_RETURN_INVALIDINPUT;
        }
        bank.resize_with(kiMaxSliceNumNew as usize, SSlice::new);
    }

    // Both are re-derived from the bank's root *after* the resize, because the resize
    // is what moves it (S28, and the reason the pointer spelling needed re-stamping
    // at all).
    let pBaseSlice = slice_in_bank(&*pDqLayer, kiBank, 0);

    for iSliceIdx in kiMaxSliceNumOld..kiMaxSliceNumNew {
        let pSlice = slice_in_bank(&*pDqLayer, kiBank, iSliceIdx);
        (*pSlice).iSliceIdx = -1;
        (*pSlice).uiBufferIdx = 0;
        (*pSlice).iCountMbNumInSlice = 0;
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let mut iRet = InitSliceBsBuffer(&mut *pSlice, bIndependenceBsBuffer, iMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        InitSliceHeadWithBase(&mut *pSlice, &*pBaseSlice);
        InitSliceRefInfoWithBase(&mut *pSlice, &*pBaseSlice, (*pCtx).iNumRef0);

        iRet = InitSliceRC(&mut *pSlice, (*pCtx).iGlobalQp);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn CalculateNewSliceNum(
    pCtx: &sWelsEncCtx,
    pLastCodedSlice: &mut SSlice,
    iMaxSliceNumOld: i32,
    iMaxSliceNumNew: *mut i32,
) -> i32 {
    // **S7.A5**: the context arm retires; the other two are live.
    if iMaxSliceNumOld == 0 || iMaxSliceNumNew.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }

    if (*pCtx).iActiveThreadsNum == 1 {
        *iMaxSliceNumNew = iMaxSliceNumOld * SLICE_NUM_EXPAND_COEF;
        return ENC_RETURN_SUCCESS;
    }

    let iPartitionID = ((*pLastCodedSlice).iSliceIdx % ((*pCtx).iActiveThreadsNum as i32)) as usize;
    let pCurLayer = current_layer(pCtx);
    let iMBNumInPartition = (*pCurLayer).EndMbIdxOfPartition[iPartitionID] - (*pCurLayer).FirstMbIdxOfPartition[iPartitionID] + 1;
    let iLeftMBNum = (*pCurLayer).EndMbIdxOfPartition[iPartitionID] - (*pCurLayer).LastCodedMbIdxOfPartition[iPartitionID].load(Ordering::Relaxed) + 1;

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

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ReallocateSliceInThread(
    pCtx: &sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIdx: i32,
    KiSlcBuffIdx: i32,
) -> i32 {
    let iMaxSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iMaxSliceNum;
    let iCodedSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iCodedSliceNum;
    let mut iMaxSliceNumNew = 0;
    let pLastCodedSlice = slice_in_bank(&*pDqLayer, KiSlcBuffIdx as usize, iCodedSliceNum - 1);
    // **T7.C5, F71's idiom at the one site the workers still reached it from.**
    // `&mut` here is a `Unique` retag over *shared* parameter state — this function
    // runs on a worker (`EncodeOnePartitionSizeLimited`), every worker resolves the
    // same layer's slice argument, and `ReallocateSliceList` only ever reads it.
    // `addr_of_mut!` creates no reference, so two workers growing their own banks at
    // the same instant no longer race on this borrow.
    let kuiSliceMode = (*pCtx)
        .param()
        .sSpatialLayers[kiDlayerIdx as usize]
        .sSliceArgument
        .uiSliceMode;

    if pLastCodedSlice.is_null() {
        // CalculateNewSliceNum's own null arm, hoisted with the parameter (T9.E2b).
        return ENC_RETURN_INVALIDINPUT;
    }
    let mut iRet = CalculateNewSliceNum(pCtx, &mut *pLastCodedSlice, iMaxSliceNum, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet = ReallocateSliceList(
        pCtx,
        kuiSliceMode,
        pDqLayer,
        KiSlcBuffIdx as usize,
        iMaxSliceNum,
        iMaxSliceNumNew,
    );
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ExtendLayerBuffer(
    pCtx: &mut sWelsEncCtx,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    let pCurLayer = current_layer(pCtx);

    // The C++ allocated a new pointer array, dropped the old one **without copying
    // it**, and left every entry to `ReallocSliceBuffer`'s fill loop below. `resize`
    // is that, minus the allocation failure: the tail arrives as `SliceIdx::NONE`,
    // which is the zero `WelsMallocz` handed back.
    {
        let slices: &mut Vec<SliceIdx> = &mut (*pCurLayer).ppSliceInLayer;
        slices.clear();
        slices.resize(kiMaxSliceNumNew as usize, SliceIdx::NONE);
    }

    // The two remaining triples — allocate, `copy_nonoverlapping` the first
    // `kiMaxSliceNumOld` entries, free the old block — are one `resize` each, which
    // keeps exactly the same guarantee: the existing entries survive at their indices
    // and the new tail is zero, as `WelsMallocz` left it.
    {
        let first: &mut Vec<i32> = &mut (*pCurLayer).pFirstMbIdxOfSlice;
        first.resize(kiMaxSliceNumNew as usize, 0);
        let count: &mut Vec<i32> = &mut (*pCurLayer).pCountMbNumInSlice;
        count.resize(kiMaxSliceNumNew as usize, 0);
    }
    let _ = kiMaxSliceNumOld;

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ReallocSliceBuffer(pCtx: &mut sWelsEncCtx) -> i32 {
    let pCurLayer = current_layer(pCtx);
    let iMaxSliceNumOld = (*pCurLayer).sSliceBufferInfo[0].iMaxSliceNum;
    let mut iMaxSliceNumNew = 0;
    let kiCurDid = pCtx.uiDependencyId as usize;
    let pLastCodedSlice = slice_in_bank(&*pCurLayer, 0, iMaxSliceNumOld - 1);
    // A7: as `InitSliceInLayer` — the mode, not a cursor.
    let kuiSliceMode =
        pCtx.param().sSpatialLayers[kiCurDid].sSliceArgument.uiSliceMode;

    if pLastCodedSlice.is_null() {
        // CalculateNewSliceNum's own null arm, hoisted with the parameter (T9.E2b).
        return ENC_RETURN_INVALIDINPUT;
    }
    let mut iRet = CalculateNewSliceNum(pCtx, &mut *pLastCodedSlice, iMaxSliceNumOld, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet = ReallocateSliceList(pCtx, kuiSliceMode, pCurLayer, 0, iMaxSliceNumOld, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    (*pCurLayer).sSliceBufferInfo[0].iMaxSliceNum = iMaxSliceNumNew;

    iMaxSliceNumNew = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        iMaxSliceNumNew += (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    iRet = ExtendLayerBuffer(pCtx, (*pCurLayer).iMaxSliceNum, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let mut iStartIdx = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        for iSliceIdx in 0..(*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum {
            let slices: &mut Vec<SliceIdx> = &mut (*pCurLayer).ppSliceInLayer;
            slices[(iStartIdx + iSliceIdx) as usize] =
                SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
        }
        iStartIdx += (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    (*pCurLayer).iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

#[inline]
pub fn CheckAllSliceBuffer(pCurLayer: &mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    // S10.3d: `&mut SDqLayer` proves there is no fork, so the bank is reached
    // safely. The `is_null()` arm is `None`.
    for iSliceIdx in 0..kiCodedSliceNum {
        match slice_in_layer_mut(pCurLayer, iSliceIdx) {
            Some(slice) if iSliceIdx == slice.iSliceIdx => {}
            _ => return ENC_RETURN_UNEXPECTED,
        }
    }
    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ReOrderSliceInLayer(pCtx: &mut sWelsEncCtx, kuiSliceMode: SliceMode, kiThreadNum: i32) -> i32 {
    let pCurLayer = current_layer(pCtx);
    let mut iEncodeSliceNum = 0;
    let mut iUsedSliceNum = 0;
    let mut iNonUsedBufferNum = 0;
    let mut aiPartitionOffset = [0i32; MAX_THREADS_NUM];

    let iPartitionNum = if kuiSliceMode == SliceMode::SM_SIZELIMITED_SLICE { kiThreadNum } else { 1 };
    for iPartitionIdx in 0..iPartitionNum {
        aiPartitionOffset[iPartitionIdx as usize] = iEncodeSliceNum;
        if kuiSliceMode == SliceMode::SM_SIZELIMITED_SLICE {
            iEncodeSliceNum += (*pCurLayer).NumSliceCodedOfPartition[iPartitionIdx as usize].load(Ordering::Relaxed);
        } else {
            iEncodeSliceNum =
                (*pCurLayer).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
        }
    }

    if iEncodeSliceNum != (*pCurLayer).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed) {
        return ENC_RETURN_UNEXPECTED;
    }

    for iSlcBuffIdx in 0..kiThreadNum {
        let iSliceNumInThread = (*pCurLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
        for iSliceIdx in 0..iSliceNumInThread {
            let pSliceBuffer = slice_in_bank(&*pCurLayer, iSlcBuffIdx as usize, iSliceIdx);
            if pSliceBuffer.is_null() {
                return ENC_RETURN_UNEXPECTED;
            }

            if (*pSliceBuffer).iSliceIdx != -1 {
                let iPartitionID = (*pSliceBuffer).iSliceIdx % iPartitionNum;
                let iActualSliceIdx = aiPartitionOffset[iPartitionID as usize] + (*pSliceBuffer).iSliceIdx / iPartitionNum;
                (*pSliceBuffer).iSliceIdx = iActualSliceIdx;
                let slices: &mut Vec<SliceIdx> = &mut (*pCurLayer).ppSliceInLayer;
                slices[iActualSliceIdx as usize] =
                    SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
                iUsedSliceNum += 1;
            } else {
                let slices: &mut Vec<SliceIdx> = &mut (*pCurLayer).ppSliceInLayer;
                slices[(iEncodeSliceNum + iNonUsedBufferNum) as usize] =
                    SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
                iNonUsedBufferNum += 1;
            }
        }
    }

    if iUsedSliceNum != iEncodeSliceNum || (*pCurLayer).iMaxSliceNum != (iNonUsedBufferNum + iUsedSliceNum) {
        return ENC_RETURN_UNEXPECTED;
    }

    CheckAllSliceBuffer(&mut *pCurLayer, iEncodeSliceNum)
}

pub fn GetCurLayerNalCount(pCurDq: &mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    // S10.3d, as `CheckAllSliceBuffer`.
    let mut iTotalNalCount = 0;
    for iSliceIdx in 0..kiCodedSliceNum {
        if let Some(slice) = slice_in_layer_mut(pCurDq, iSliceIdx) {
            if slice.sSliceBs.uiBsPos > 0 {
                iTotalNalCount += slice.sSliceBs.iNalIndex;
            }
        }
    }
    iTotalNalCount
}

pub fn GetTotalCodedNalCount(pFbi: &mut SFrameBSInfo) -> i32 {
    let mut iTotalCodedNalCount = 0;
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME {
        iTotalCodedNalCount += pFbi.sLayerInfo[iNalIdx].iNalCount;
    }
    iTotalCodedNalCount
}

pub fn GetCurrentSliceNum(pCurDq: &SDqLayer) -> i32 {
    pCurDq.sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed)
}

/// `FrameBsRealloc` — svc_encode_slice.cpp:1562.
///
/// # Safety
/// `pCtx` must be a context built by `WelsInitEncoderExt`; `pLayerBsInfo` must be
/// one of `(*pFrameBsInfo).sLayerInfo`'s entries, which is what every caller
/// passes and what the C++'s own `while (pLBI1 != pLayerBsInfo)` assumes.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FrameBsRealloc(
    pCtx: &mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kiMaxSliceNumOld: i32,
) -> i32 {
    // (S3.B1: the S67/H2 audit note that stood here is borrowck's job now — this
    // is a field-scoped borrow of an owned box, not a context retag.)
    // §4.6's reorder: the count is a scalar, so the `pOut` borrow ends on this line
    // and the `param()` reads below are free.
    let mut iCountNals =
        pCtx.pOut.as_deref().expect("pOut lives").sNalList.len() as i32;
    let spatial_layers = if pCtx.param_opt().is_some() { pCtx.param().iSpatialLayerNum } else { 1 };
    iCountNals += kiMaxSliceNumOld * (spatial_layers + if pCtx.bNeedPrefixNalFlag { 1 } else { 0 });

    // Was: allocate a bigger block, `copy_nonoverlapping` the old contents in,
    // free the old, store the new — twice, with a null check each. `Vec::resize`
    // is the same three steps and keeps the same guarantee, that the existing
    // `iCountNals` entries survive at their indices and the new tail is zeroed
    // (`WelsMallocz` zeroed it too).
    let pOut = pCtx.pOut.as_deref_mut().expect("pOut lives");
    pOut.sNalList.resize(iCountNals as usize, SWelsNalRaw::default());
    pOut.sNalLen.resize(iCountNals as usize, 0);
    let pNalLen = pOut.sNalLen.as_mut_ptr();

    // **F60**, and the C++'s closing loop is the fix (`svc_encode_slice.cpp:1589`).
    // The resize moves `sNalLen`, so every `sLayerInfo[..].pNalLengthInByte` handed
    // out before it — `wels_encoder_ext.rs:617`, `encoder_ext.rs:2077`/`:3113`, and
    // every `.add(iCountNal)` derived from those — names the freed block from here
    // on. The C++ re-stamps them from the new root, layer by layer, each layer's
    // cursor being the previous layer's plus that layer's own NAL count, and the
    // port dropped the loop when the two allocations became `Vec`s.
    //
    // **Every write below goes through `pLayerBsInfo`, not through
    // `pFrameBsInfo`** — the probe's fourth red, and the first spelling of this fix
    // caused it. `WelsEncoderEncodeExt` builds its layer cursor once, as
    // `(*pFbi).sLayerInfo.as_mut_ptr()` (`encoder_ext.rs`), which retags the whole
    // array; `addr_of_mut!((*pFrameBsInfo).sLayerInfo[i])` creates no retag at all,
    // so it writes with the *parent's* tag and pops that array-wide child — and the
    // caller then reads `pNalLengthInByte` through it. Walking from `pLayerBsInfo`
    // instead keeps every store inside the tag the caller is still holding. S28's
    // rule in its other direction: derive from the root the consumers share.
    let pFirstLayer = std::ptr::addr_of_mut!((*pFrameBsInfo).sLayerInfo).cast::<SLayerBSInfo>();
    let kiLayersBefore = pLayerBsInfo.offset_from(pFirstLayer);
    debug_assert!(
        kiLayersBefore >= 0 && (kiLayersBefore as usize) < MAX_LAYER_NUM_OF_FRAME,
        "FrameBsRealloc: pLayerBsInfo is not one of pFrameBsInfo's layers"
    );
    let mut cursor = pNalLen;
    for iBack in (0..=kiLayersBefore).rev() {
        let pLBI = pLayerBsInfo.offset(-iBack);
        (*pLBI).pNalLengthInByte = cursor;
        cursor = cursor.add((*pLBI).iNalCount as usize);
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn SliceLayerInfoUpdate(
    pCtx: &mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kuiSliceMode: SliceMode,
) -> i32 {
    let mut iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        iMaxSliceNum += (*current_layer(pCtx)).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    if iMaxSliceNum > (*current_layer(pCtx)).iMaxSliceNum {
        // T9.G6: hoisted (shape B).
        let iCurMaxSliceNum = (*current_layer(pCtx)).iMaxSliceNum;
        let iRet = ExtendLayerBuffer(pCtx, iCurMaxSliceNum, iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        (*current_layer(pCtx)).iMaxSliceNum = iMaxSliceNum;
    }

    // T9.G6: hoisted (shape B).
    let iActiveThreadsNum = pCtx.iActiveThreadsNum as i32;
    let mut iRet = ReOrderSliceInLayer(pCtx, kuiSliceMode, iActiveThreadsNum);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let iCodedSliceNum = GetCurrentSliceNum(&*current_layer(pCtx));
    (*pLayerBsInfo).iNalCount = GetCurLayerNalCount(&mut *current_layer(pCtx), iCodedSliceNum);
    let iCodedNalCount = GetTotalCodedNalCount(&mut *pFrameBsInfo);

    if iCodedNalCount > pCtx.pOut.as_deref().expect("pOut lives").sNalList.len() as i32 {
        // T9.G6: hoisted (shape B).
        let iCurMaxSliceNum = (*current_layer(pCtx)).iMaxSliceNum;
        iRet = FrameBsRealloc(pCtx, pFrameBsInfo, pLayerBsInfo, iCurMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

pub fn WelsInitSliceEncodingFuncs(uiCpuFlag: u32) {
    // Dynamically wires CPU architecture flags if SIMD variants are enabled
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static MB_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use crate::api::codec_api::EVideoFrameType;
    use crate::api::codec_api::ECOMPLEXITY_MODE;
    use crate::api::codec_api::SliceModeEnum;
    use crate::api::codec_api::abi_test_driver::{EncoderProbeOptions, drive_encoder_over};

    /// D-gate-6's scaling knob (the user, 2026-08-24: the session gate is capped at
    /// 15 minutes, coverage yields). `small` under the Miri interpreter, `full` on
    /// every native run — and under Miri again when the battery exports
    /// `MIRI_FULL=1`, which is how the phase-exit tiers restore the full drive
    /// (`gates.sh` sets it for `full`/`exit`; the env read needs
    /// `-Zmiri-disable-isolation`, which the `--lib` step has always passed).
    fn miri_scaled(full: i32, small: i32) -> i32 {
        if cfg!(miri) && std::env::var_os("MIRI_FULL").is_none() { small } else { full }
    }

    /// **The encoder's aliasing probe** — Phase 6 session A, and the first Miri
    /// coverage the encode path has ever had.
    ///
    /// Before this test the only encoder-adjacent Miri test in the tree installed a
    /// deblocking table and was `#[cfg_attr(miri, ignore)]` besides, so **no
    /// encoder line had ever been executed under the aliasing checker**. F47 is
    /// what that costs: real UB on the *ordinary* CAVLC path survived five phases
    /// of green gates because no probe drove it, and the probe that finally did
    /// found it on its first execution.
    ///
    /// **Every assertion below carries the reason it exists** — the decoder probes'
    /// pattern (`decoder/decode_slice.rs`), and for the decoder's reason: an encode
    /// that merely "produced bytes" can cover almost nothing.
    ///
    /// * **48 x 32, read back from the encoder rather than from this test's own
    ///   argument.** That is a 3 x 2 macroblock grid, so MB(1, 1) has all four
    ///   neighbours, MB(0, 1) is missing only its left and MB(2, 1) only its
    ///   top-right. **F34's lesson, on the encoder side**: a single-macroblock
    ///   picture has no neighbour, so no neighbour-dependent mode-decision or
    ///   motion-vector-prediction path runs, and a probe over one can return green
    ///   on UB that is simply unreachable in it.
    /// * **At least two frames and the second inter-coded.** An all-intra sequence
    ///   executes no motion estimation and no inter mode decision at all — the
    ///   larger half of what this phase converts.
    /// * **The second frame's payload is far above the all-skip floor.** Two
    ///   identical input pictures encode to all-skip macroblocks and motion
    ///   estimation does essentially nothing, so the driver synthesises a sequence
    ///   that moves. **Measured on this exact configuration**, by running it both
    ///   ways: a static source encodes frame 1 to **12** bytes and this one encodes
    ///   it to **618** — 51x — so the 200-byte assertion sits an order of magnitude
    ///   above any all-skip frame and a third below the real one. (The three frames
    ///   read 728 / 618 / 549 moving and 728 / 12 / 94 static.)
    ///
    /// **Coverage is proven, not asserted** (the F21 rule): with F57's
    /// `+ kuiMvdCostTableOvershoot` deleted, the live probe goes red under Miri at
    /// `md.rs:1544` — "attempting to offset pointer by 1042 bytes ... only 11 bytes
    /// from the end" — and green when it is restored (measured, session A, both
    /// directions). Each of the other nine defects it found was observed red before
    /// its fix and green after, which is the same evidence taken the natural way.
    ///
    /// **The live half of the encoder probe: initialisation, under the checker.**
    ///
    /// `frames = 0` drives create -> `GetDefaultParams` -> `InitializeExt` ->
    /// `GetOption` -> `Uninitialize` -> destroy and stops there. That is not a
    /// consolation prize: encoder initialisation is where the multi-MiB context,
    /// the DQ layers, the slice buffers, the MVD cost table and the parameter sets
    /// are all built, and **every defect this probe found on that path is fixed**,
    /// so this test is what keeps them fixed. It is the encoder's one *live* Miri
    /// probe until 6.4 unblocks the encode loop below.
    ///
    /// What it covers, measured rather than claimed — each of these was red here
    /// before its fix and green after: F13's remaining production site
    /// (`InitDqLayers`), **F57** (`MvdCostInit`'s cursor leaving the table), and the
    /// `sSpatialLayers` / `sDependencyLayers` `&mut`-through-a-raw-parent family.
    #[test]
    fn encoder_initialisation_runs_under_the_aliasing_checker() {
        let (frames, dims) = drive_encoder_over(48, 32, 0, EncoderProbeOptions::default());
        assert!(frames.is_empty(), "frames = 0 encodes nothing; this drives init only");
        assert_eq!(
            dims,
            (48, 32),
            "the encoder must come up configured for a 3x2 macroblock grid, read back \
             from the encoder rather than from this test's own argument"
        );
    }

    /// **The encode loop under the aliasing checker — live in the `--lib` Miri step
    /// since Phase 6 session B.**
    ///
    /// Session A left this `#[cfg_attr(miri, ignore)]`, blocked at its first frame:
    /// `InitSliceBsBuffer` cached the *shared* writer in every slice at init
    /// (`SSlice.pSliceBsa = &pOut->sBsWrite`) and `InitBitStream` replaced that
    /// writer wholesale every frame, so `WelsSliceHeaderWrite` read through a dead
    /// pointer — a write through the parent, which no spelling fixes. Session B's
    /// settlement (T6.B3): the pointer was a cache of one bit that `sSliceBs.pBs`'s
    /// nullness already records, and `slice_writer` derives it fresh at each use.
    /// The two neighbouring caches went with it — `SWelsSliceBs.pBsBuffer`
    /// (T6.B5, `pThreadBsBuffer[uiBufferIdx]`) and `SWelsNalRaw.pRawData` (T6.B4,
    /// `iStartPos`) — and the attribute came off. Every red the walk then found is
    /// recorded in the session B log entry, red-before and green-after observed.
    ///
    /// The `--lib` step runs this with `-Zmiri-disable-isolation`, for `WelsTime()`
    /// (`SystemTime::now()`, the library's one clock site, called by
    /// `EncodeFrameInternal` around every frame; it does not reach the bitstream).
    /// That flag disables host isolation and nothing else — aliasing and validity
    /// checking are exactly what they were.
    ///
    /// **Two frames under Miri, three everywhere else (D-gate-5, the `scale()`
    /// pattern).** The seam made every reconstruction access a bounds-checked
    /// `Cell` index and Miri interprets each one (F140: 96.5 s → 258.5 s for this
    /// probe, uniform, no hot spot). What the third frame added was a second
    /// inter frame — the same ME/MD/reconstruction paths as frame 1 with one more
    /// picture in the reference list, and the list update itself runs after
    /// frames 0 and 1 alike. Every assertion below is on frames 0 and 1, and the
    /// 3x2 neighbour grid (F34) is untouched. Full size on every plain
    /// `cargo test`.
    ///
    /// **Ignored under Miri — D-gate-7 (the user, 2026-08-24), a cost scope,
    /// not a defect skip.** This probe's distinguishing axes are CABAC entropy
    /// over LOW_COMPLEXITY on a single slice; under Miri both are covered more
    /// deeply elsewhere — the size-limited probe drives the CABAC writers *and*
    /// their stash/restore arm at LOW_COMPLEXITY (its options default `cabac:
    /// true`), the fork probes drive CABAC multi-slice at `full`/`exit`, and
    /// the CAVLC probe carries the other entropy family — so its marginal Miri
    /// coverage no longer pays its ~4 minutes in every encoder run. It runs at
    /// full size on every native `cargo test`, where its measured anchors
    /// (F34's grid, the 618-byte inter floor) keep their teeth.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn encode_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker() {
        let kiFrames = miri_scaled(3, 2) as usize;
        let (frames, dims) = drive_encoder_over(48, 32, kiFrames, EncoderProbeOptions::default());

        assert_eq!(
            dims,
            (48, 32),
            "the encoder must be configured for a 3x2 macroblock grid; a picture \
             without neighbours covers nothing this test exists for"
        );
        assert_eq!(frames.len(), kiFrames, "the encode loop did not run to the end");
        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        assert_eq!(
            frames[0].kind,
            EVideoFrameType::videoFrameTypeIDR,
            "the sequence must open on an IDR"
        );
        assert_eq!(
            frames[1].kind,
            EVideoFrameType::videoFrameTypeP,
            "the second frame must be inter-coded, or no ME/MD path executes at all"
        );
        assert!(
            frames[1].bytes > 200,
            "the inter frame coded {} bytes, which is at the all-skip floor: the \
             source did not move, so motion estimation did nothing",
            frames[1].bytes
        );
    }

    /// **The fork/join under the aliasing checker** — T7.B4, and the reason F12's
    /// Miri skip could be deleted rather than merely renamed.
    ///
    /// Before this test, **no test in this crate had ever set `iMultipleThreadIdc`
    /// above 1**: every probe hard-coded 1, the unit tests build contexts they never
    /// dispatch through, and the multi-threaded path's only coverage was the
    /// diffharness, which is a byte instrument and cannot see aliasing at all. So the
    /// `--skip wels_thread_pool` line in `gates.sh` was hiding a module *and* the
    /// path that module served, and deleting the line without adding this would only
    /// have stopped naming the gap.
    ///
    /// What it drives: `SM_FIXEDSLCNUM_SLICE` with two slices at two threads, which
    /// is `EncodeFixedSlicesForked` — two `SliceJobHandle`s moved across two
    /// `thread::scope` spawns, each owning one bs scratch slot, both calling
    /// `WelsCodeOneSlice` through the same raw context pointer, joined by the scope
    /// before `AppendSliceToFrameBs` walks the slices in index order. Miri checks
    /// what the byte gate cannot: that the two workers' derivations of the shared
    /// context do not invalidate each other, and that the assembly reads what they
    /// wrote.
    ///
    /// **112x112, and the size is forced rather than chosen.** `MIN_NUM_MB_PER_SLICE`
    /// is 48 (`wels_encoder_ext.rs:106`), and
    /// `SliceArgumentValidationFixedSliceMode` silently rewrites any multi-slice
    /// request on a smaller picture to `SM_SINGLE_SLICE` — which is what the other
    /// probes' 48x32 (a 3x2 grid, six macroblocks) gets. The first version of this
    /// test asked for two slices at 48x32, got one VCL NAL per frame, and would have
    /// passed every assertion that did not count NALs while driving **exactly the
    /// single-threaded path it exists to avoid**. 7x7 = 49 macroblocks is the
    /// smallest grid above the threshold, and Miri's clock is the reason it is the
    /// smallest rather than a comfortable one.
    ///
    /// Two frames, not three: an IDR to build the slice banks and one inter frame so
    /// the fork runs with the mode-decision and motion-estimation halves of the tree
    /// live. `bUseLoadBalancing` is off (the probe forces it), so the slice
    /// boundaries are a function of the input and these assertions mean something.
    ///
    /// **Live under Miri since T9.E8 — the ignore attribute retired with the last
    /// fork race.** It stood from T7.B4 to session E as a work queue, not a shrug:
    /// each run aborts at the first undefined behaviour it meets, a session fixed
    /// that family, and the next run reached deeper — F70, F71, F73/F107 (the
    /// reconstruction seam), F132's rounds 1-6, then this session's round 5
    /// (deblocking's cross-slice `uiSliceIdc` reads, closed by the map
    /// substitution, T9.E4) and the rounds it had been masking: the per-slice
    /// `pfInterMd` stamp into the shared function list (hoisted, T9.E7) and the
    /// in-fork `as_mut_ptr` autoref mints on shared state (re-spelled on
    /// `addr_of!`, T9.E7; F143 carries the enumeration).
    ///
    /// **First green run: 3356 s under Miri, 2026-08-24** — a complete two-frame
    /// two-worker encode with zero reports. The cost is why the session-level
    /// gate skips both fork probes BY NAME (D-gate-6's 15-minute cap; see
    /// gates.sh): they run at `full`/`exit` and by the explicit command in that
    /// block, and any session that touches the fork, the slice structures, or
    /// deblocking owes them one explicit run at its close.
    ///
    /// The test runs normally in both profiles and is the only coverage the fork/join
    /// has outside the diffharness.
    #[test]
    fn fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker() {
        let (frames, dims) = drive_encoder_over(
            112,
            112,
            2,
            EncoderProbeOptions {
                slice_mode: SliceModeEnum::SM_FIXEDSLCNUM_SLICE,
                slice_num: 2,
                threads: 2,
                ..EncoderProbeOptions::default()
            },
        );

        assert_eq!(dims, (112, 112), "the encoder must be configured for a 7x7 grid");
        assert_eq!(frames.len(), 2, "the encode loop did not run to the end");
        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes, which is what a lost slice looks like \
             from here: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        assert_eq!(
            frames[0].kind,
            EVideoFrameType::videoFrameTypeIDR,
            "the sequence must open on an IDR"
        );
        assert_eq!(
            frames[1].kind,
            EVideoFrameType::videoFrameTypeP,
            "the second frame must be inter-coded, or the fork runs over an all-intra \
             frame and the mode-decision half of the tree stays dark"
        );
        // The assertion that made the size matter. Two slices means two VCL NALs; one
        // means the request was rewritten to `SM_SINGLE_SLICE` and the fork never ran.
        // Without it this test is green on the path it was written to leave.

        // Two slices means two VCL NALs per frame. One would mean the fork ran a
        // single job and the second slice's bytes never reached the frame — exactly
        // the failure `AppendSliceToFrameBs` exists to make impossible, and worth
        // asserting where a thread is involved.
        assert!(
            frames.iter().all(|f| f.vcl_nals >= 2),
            "a frame carried fewer than two VCL NALs, so a slice did not make it out \
             of the fork: {:?}",
            frames.iter().map(|f| (f.kind, f.vcl_nals)).collect::<Vec<_>>()
        );
    }

    /// **The mid-row fork/join probe — F107's second acceptance, and the one the
    /// existing probe cannot be.**
    ///
    /// The probe above drives `SM_FIXEDSLCNUM_SLICE` with RC on, which F107 §1
    /// measured as the **one** row-aligned configuration of the four
    /// multi-threaded ones: its slice runs are whole macroblock rows, so a design
    /// that only works on row-aligned slices passes it. Three modes out of four
    /// put a boundary *mid-row*, and that is the case every `&mut [u8]` band, every
    /// per-macroblock `PlaneCursorMut` and every per-worker `&mut SPicture` fails
    /// on — so the seam is not tested until one of them runs.
    ///
    /// **F226's referee — the `UpdateMbMapForked` fork, at a size Miri can afford.**
    ///
    /// `UpdateMbListNeighborParallel` held `&mut (*pCurDq).sSliceEncCtx` for one
    /// scalar read while every worker in `UpdateMbMapForked`'s scope called it on
    /// the same layer. That is F223's third rule — in-fork a `&mut` retag is a
    /// write — and **nothing in this project could see it**: the fork needs
    /// `bUseLoadBalancing`, which both diffharness drivers and both §4.7 MT probes
    /// pin off, and `load_balancing_completes_frames_with_sane_slice_counts` below
    /// is the only test that reaches the path and is `#[cfg_attr(miri, ignore)]`.
    /// Its doc says "the aliasing question this path raises is the fork/join's, and
    /// that probe answers it", and for this site that is false — the fork probe
    /// forces the flag off and never enters this fork at all.
    ///
    /// **This probe does not drive the encoder**, which is what makes it
    /// affordable where that test is not: the aliasing question is about two
    /// workers and one layer, not about encoding, so it builds the layer by hand
    /// and spawns the same shape `UpdateMbMapForked` does — one scoped thread per
    /// slice, each walking its own slice's macroblocks. Under Miri it is the
    /// instrument that refuses the `&mut`; natively it is a neighbour-map
    /// correctness test, and both assertions below hold either way.
    ///
    /// **It has teeth, checked rather than assumed**: restoring the `&mut` binding
    /// in `UpdateMbListNeighborParallel` makes this fail under Miri with "Data race
    /// detected between (1) retag write on thread `<unnamed>` and (2) retag write
    /// ... on thread `<unnamed>`", and removing it makes it pass.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn update_mb_map_forked_workers_share_the_layer_without_racing() {
        use crate::safe::mb_grid::{MbArray, MbDims};
        use crate::encoder::md::SMB;
        use crate::encoder::deblocking::{LEFT_MB_POS, TOP_MB_POS};
        use crate::encoder::slice_multi_threading::UpdateMbListNeighborParallel;
        use super::SDqLayer;
        use std::sync::atomic::AtomicU16;

        // Two slices over a 4x2 grid, four macroblocks each — the smallest shape
        // that gives each worker its own contiguous run and still has a slice
        // boundary for the neighbour walk to respect.
        const MB_W: usize = 4;
        const MB_H: usize = 2;
        const SLICES: usize = 2;
        let dims = MbDims::new(MB_W, MB_H);
        let total = (MB_W * MB_H) as i32;

        let mut dq = SDqLayer::default();
        dq.iMbWidth = MB_W as i16;
        dq.iMbHeight = MB_H as i16;
        dq.sMbDataP = MbArray::new(dims, SMB::default());
        // Each record must know its own coordinates: `UpdateMbNeighbor` reads
        // `iMbXY` off the window's records to locate their neighbours.
        for xy in 0..total {
            let mb = dq.sMbDataP.as_mut_slice();
            mb[xy as usize].iMbXY = xy;
            mb[xy as usize].iMbX = (xy % MB_W as i32) as i16;
            mb[xy as usize].iMbY = (xy / MB_W as i32) as i16;
        }

        dq.sSliceEncCtx.iMbWidth = MB_W as i16;
        dq.sSliceEncCtx.iMbHeight = MB_H as i16;
        dq.sSliceEncCtx.iMbNumInFrame = total;
        // Row 0 is slice 0, row 1 is slice 1 — the map `WelsMbToSliceIdc` reads.
        dq.sSliceEncCtx.pOverallMbMap = (0..total)
            .map(|xy| AtomicU16::new((xy / MB_W as i32) as u16))
            .collect();
        dq.pFirstMbIdxOfSlice = (0..SLICES).map(|s| (s * MB_W) as i32).collect();
        dq.pCountMbNumInSlice = (0..SLICES).map(|_| MB_W as i32).collect();

        // **S10.4: the address-as-integer is gone, and so is the argument it
        // carried.** This probe used to hand every worker the layer's address so
        // each could resolve the grid itself — the production shape at the time —
        // and its job was to keep honest the claim that "each writes only its own
        // slice's macroblock records, disjoint by `pFirstMbIdxOfSlice` /
        // `pCountMbNumInSlice`".
        //
        // That claim is not asserted any more; it is *constructed*. The grid is
        // carved into per-slice `&mut [SMB]` before the spawn, exactly as
        // `UpdateMbMapForked` now does, so two workers naming one record is not a
        // race this test could catch — it is a program that does not compile.
        // What the probe still checks is the half that could still be wrong: that
        // the partition arithmetic hands each worker the records it should, and
        // that the neighbour walk respects the slice boundary.
        let SDqLayer { sMbDataP, sSliceEncCtx, pFirstMbIdxOfSlice, pCountMbNumInSlice, .. } =
            &mut dq;
        let kiGridWidth = sMbDataP.dims().mb_width();
        let mut rest: &mut [SMB] = sMbDataP.as_mut_slice();
        let mut cursor = 0i32;
        let mut chunks: Vec<(i32, i32, i32, &mut [SMB])> = Vec::new();
        for idc in 0..SLICES as i32 {
            let first = pFirstMbIdxOfSlice[idc as usize];
            let count = pCountMbNumInSlice[idc as usize];
            let (_gap, tail) = rest.split_at_mut((first - cursor) as usize);
            let (chunk, tail) = tail.split_at_mut(count as usize);
            chunks.push((idc, first, count, chunk));
            rest = tail;
            cursor = first + count;
        }
        let pSliceCtx = &*sSliceEncCtx;

        std::thread::scope(|s| {
            for (idc, first, count, chunk) in chunks {
                s.spawn(move || {
                    let mut mbs = crate::safe::mb_grid::MbWindow::new(
                        chunk,
                        first as usize,
                        kiGridWidth,
                        first as usize,
                    );
                    UpdateMbListNeighborParallel(
                        &mut mbs,
                        pSliceCtx,
                        MB_W as i32,
                        idc,
                        first,
                        count,
                    );
                });
            }
        });

        // Every macroblock was visited (the walk sets a neighbour mask on each).
        // Row 0's first macroblock has neither a left nor a top neighbour; row 1's
        // second has both a left neighbour and a top one, but the top is in the
        // other slice, so `UpdateMbNeighbor` must not mark it available.
        let mbs = dq.sMbDataP.as_slice();
        assert_eq!(
            mbs[0].uiNeighborAvail & ((LEFT_MB_POS | TOP_MB_POS) as u8),
            0,
            "the first macroblock of slice 0 has no left or top neighbour"
        );
        assert_ne!(
            mbs[MB_W + 1].uiNeighborAvail & (LEFT_MB_POS as u8),
            0,
            "the second macroblock of slice 1 has a left neighbour in its own slice"
        );
        assert_eq!(
            mbs[MB_W + 1].uiNeighborAvail & (TOP_MB_POS as u8),
            0,
            "its top neighbour is in slice 0, so it must not be available"
        );
    }

    /// **S5.D2b's referee — the boxed banks, under the same two workers.**
    ///
    /// The second half of what the `&SDqLayer` flip needs, and the same question D2a
    /// asked of the partition counters: may a body hold a whole-layer shared borrow
    /// while a sibling worker writes its own slice-buffer bank? With
    /// `sSliceBufferInfo` *inline* the answer was no — `ReallocateSliceList` and
    /// `ReallocateSliceInThread` write into the layer's own bytes, and a sibling's
    /// entry retag races them. Boxed, every bank write lands in the box's allocation,
    /// which no retag of the layer reaches: F163's argument, the one C4b relies on for
    /// the MVD table.
    ///
    /// **The spelling is `ReallocateSliceList`'s, deliberately.** The write below is
    /// `&mut (*p).sSliceBufferInfo[w]` — a real `&mut`, not an `addr_of_mut!` — because
    /// that is what the in-fork writer does, and the two are not equivalent: a probe
    /// using `addr_of_mut!` would create no reference and so would not exercise the
    /// retag that matters. It passes because `Box` place-deref is built into rustc: no
    /// `&mut Box<..>` is created for `..sSliceBufferInfo[w]`, so nothing retags the
    /// eight header bytes that do live inline.
    ///
    /// **Teeth, checked (F234).** Putting the array back inline —
    /// `[SSliceBufferInfo; MAX_THREADS_NUM]` as a field — makes this fail under Miri
    /// with "Data race detected between (1) **retag write** on thread `unnamed-2` and
    /// (2) retag read of type `SDqLayer` on thread `unnamed-3`". Note *retag* write,
    /// not non-atomic write: with the array inline the `&mut` itself is the conflicting
    /// access, which is why boxing rather than atomics is the fix for this half. The
    /// round count and the per-round re-borrow are load-bearing exactly as in D2a's
    /// probe.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn slice_banks_take_a_shared_layer_borrow_across_the_forked_writes() {
        use super::{SDqLayer, SSliceBufferInfo};
        const WORKERS: usize = 2;
        const ROUNDS: i32 = 200;

        let mut dq = SDqLayer::default();
        for w in 0..WORKERS {
            dq.sSliceBufferInfo[w].iMaxSliceNum = 0;
            dq.sSliceBufferInfo[w].iCodedSliceNum = 0;
        }
        let layer_addr = (&mut dq as *mut SDqLayer) as usize;

        std::thread::scope(|s| {
            for w in 0..WORKERS {
                s.spawn(move || unsafe {
                    let p = layer_addr as *mut SDqLayer;
                    for r in 0..ROUNDS {
                        // the entry retag a flipped read-only body performs
                        let layer: &SDqLayer = &*p;
                        let _ = layer.iMbWidth;
                        // ... while this worker writes its **own** bank, which lives
                        // in the box rather than in the layer.
                        // EXACT SPELLING of ReallocateSliceList: a `&mut` through the
                        // field, which must DerefMut the Box header inline in the layer.
                        let bank: &mut SSliceBufferInfo = &mut (*p).sSliceBufferInfo[w];
                        bank.iMaxSliceNum = r;
                        bank.iCodedSliceNum = r + 1;
                    }
                });
            }
        });

        for w in 0..WORKERS {
            assert_eq!(dq.sSliceBufferInfo[w].iMaxSliceNum, ROUNDS - 1);
            assert_eq!(dq.sSliceBufferInfo[w].iCodedSliceNum, ROUNDS);
        }
    }

    /// **S5.D2a's referee — a whole-layer `&SDqLayer` held while workers stamp their
    /// own partition counters.**
    ///
    /// This is the property D2/D3's flip is bought with, so it is asked of Miri
    /// directly rather than inferred. The brief's account of why the 37 read-only
    /// `*mut SDqLayer` bodies cannot simply take `&SDqLayer` is that
    /// `NumSliceCodedOfPartition` and `LastCodedMbIdxOfPartition` live **inline in the
    /// layer** and are written from inside the encode — six sites across
    /// `WelsISliceMdEncDynamic` and `WelsMdInterMbLoopOverDynamicSlice`, each stamping
    /// `[kiPartitionId]`. A whole-struct shared retag racing a concurrent write to an
    /// inline field is undefined behaviour under Miri's model; that is F228's finding
    /// about the context, restated about the layer.
    ///
    /// With the two arrays atomic, the race is gone by construction and a body may take
    /// a whole-layer shared borrow while its siblings write. That is what this asserts:
    /// each worker re-takes `&*p` every round — the entry retag a called body performs
    /// — and stamps only its own partition slot.
    ///
    /// **It has teeth, checked rather than assumed.** Pointing the same two workers at
    /// a *non-atomic* inline field instead — `iMaxSliceNum`, written through a raw
    /// place while the other worker holds its `&SDqLayer` — makes this fail under Miri
    /// with "Data race detected between (1) non-atomic write on thread `unnamed-1` and
    /// (2) retag read of type `SDqLayer` on thread `unnamed-2`". That is precisely the
    /// diagnosis this checkpoint exists to remove, and it is why
    /// `sSliceBufferInfo` — the other inline field the fork writes — still blocks the
    /// flip and is the next checkpoint's subject.
    ///
    /// Like the layer probe above, this does not drive the encoder: the question is
    /// about two workers and one struct, not about encoding.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn partition_counters_take_a_shared_layer_borrow_across_the_forked_writes() {
        use std::sync::atomic::Ordering;
        use super::SDqLayer;
        const WORKERS: usize = 2;
        // **200, and the number is load-bearing.** Miri reports a data race only when
        // the schedule it runs actually interleaves the two accesses. At 8 rounds this
        // probe's teeth check came back *green* — the non-atomic control was silent —
        // and only at 200 does the sibling retag land inside the write. A round count
        // too small does not make a weak referee, it makes a blind one that reads as a
        // passing test (F234).
        const ROUNDS: i32 = 200;

        let mut dq = SDqLayer::default();
        dq.iMbWidth = 4;
        dq.iMbHeight = 2;

        // The address as an integer, for the reason the probe above gives: D1 pins the
        // tree's hand-written `Send` impls at two and a test may not spend that pin.
        let layer_addr = (&mut dq as *mut SDqLayer) as usize;

        std::thread::scope(|s| {
            for w in 0..WORKERS {
                s.spawn(move || unsafe {
                    let p = layer_addr as *mut SDqLayer;
                    for r in 0..ROUNDS {
                        // **The borrow under test, re-taken every round** — because
                        // that is the shape of the thing being bought. The 37
                        // read-only bodies are *called*, many times per frame, and
                        // each retags the whole layer on entry. A probe that borrows
                        // once at the top and holds it never interleaves its retag
                        // with the other worker's writes, and so proves nothing: the
                        // first draft of this probe did exactly that and its teeth
                        // check came back green, which is how the shape was found.
                        let layer: &SDqLayer = &*p;
                        let _ = layer.EndMbIdxOfPartition[w];
                        layer.LastCodedMbIdxOfPartition[w].store(r, Ordering::Relaxed);
                        layer.NumSliceCodedOfPartition[w].fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        for w in 0..WORKERS {
            assert_eq!(
                dq.NumSliceCodedOfPartition[w].load(Ordering::Relaxed),
                ROUNDS,
                "worker {w} counted only its own partition's slices"
            );
            assert_eq!(
                dq.LastCodedMbIdxOfPartition[w].load(Ordering::Relaxed),
                ROUNDS - 1,
                "worker {w} left its own partition's last macroblock index"
            );
        }
    }

    /// **S5.C4b's referee — the MVD cursor, held across a slice, under two workers.**
    ///
    /// C4b turned `SWelsMD::pMvdCost` from a `*mut u16` into a borrow of the
    /// context's `pMvdCostTable`, and the two `WelsMdInterMbLoop` bodies derive that
    /// borrow once and hold it for the whole macroblock loop. Under the fork that is
    /// a claim about shared data, and no byte gate can see it: the differential sweep
    /// certifies output, and a stale-tag read produces the same bytes right up until
    /// the optimiser decides otherwise. So the claim is asked of Miri directly, at a
    /// size Miri can afford — this probe does not drive the encoder, because the
    /// question is about two workers and one table, not about encoding.
    ///
    /// **The claim, in three parts.**
    ///
    /// 1. The `&[u16]` lands in the `Vec`'s *heap buffer*, which is a different
    ///    allocation from the context — F163's argument, the one the accessor-sibling
    ///    test above already turns on. So no retag of the context can reach it, and
    ///    holding it across the loop's calls is lawful.
    /// 2. The table is written exactly once, by `MvdCostInit` inside
    ///    `WelsInitEncoderExt`, before any slice worker exists. Concurrent *readers*
    ///    of one buffer coexist freely; a concurrent writer would not, and there is
    ///    none.
    /// 3. Deriving it must be **field-precise** — `&(*p).pMvdCostTable`, never a
    ///    `&self` accessor. This is the part with teeth, and the part that decided
    ///    the shape of `MvdCostCursor::origin`.
    ///
    /// **It has teeth, checked rather than assumed.** Respelling the derivation
    /// below as the whole-context borrow a `&self` accessor would make —
    /// `let c: &sWelsEncCtx = &*p; MvdCostCursor::origin(&c.pMvdCostTable[..], n)` —
    /// makes this fail under Miri with "Data race detected between (1) non-atomic
    /// write on thread `unnamed-1` and (2) retag read of type `sWelsEncCtx` on
    /// thread `unnamed-2`". That is F228, and it is why the raw
    /// `sWelsEncCtx::mvd_cost_origin` this replaced could not simply start returning
    /// a reference: its `&self` was harmless only because the borrow died on the
    /// next line.
    ///
    /// The per-worker write below is the *class* of concurrent inline-context write
    /// the fork performs, reduced to its smallest form — one disjoint scalar slot per
    /// worker. It is what makes part 3 observable; parts 1 and 2 hold without it.
    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn mvd_cursor_survives_a_slice_held_across_the_forked_workers() {
        use crate::safe::mvd_cost::MvdCostCursor;
        use crate::encoder::encoder_context::sWelsEncCtx;

        const SIZE: i32 = 32;                 // the zero-MVD entry's index
        const LEN: usize = 2 * SIZE as usize + 1;
        const WORKERS: usize = 2;

        let mut ctx = Box::new(sWelsEncCtx::new());
        // One QP row, filled so a read's *value* identifies the index it came from.
        ctx.pMvdCostTable = (0..LEN as u16).collect();
        ctx.iMvdCostTableSize = SIZE;
        ctx.iMvdCostTableStride = LEN as i32;
        // Two disjoint scalar slots, one per worker — see the doc's last paragraph.
        ctx.iActiveThreadsNum = 0;
        ctx.iMaxSliceCount = 0;

        // The address as an integer, for the reason the layer probe above gives:
        // D1 pins the tree's hand-written `Send` impls at two, and a test may not
        // spend that pin.
        let ctx_addr = (&mut *ctx as *mut sWelsEncCtx) as usize;

        std::thread::scope(|s| {
            for w in 0..WORKERS {
                s.spawn(move || unsafe {
                    let p = ctx_addr as *mut sWelsEncCtx;
                    // **The derivation under test** — field-precise, taken once, and
                    // held for the whole of this worker's body, exactly as
                    // `WelsMdInterMbLoop` holds it across its macroblock loop.
                    let cursor = MvdCostCursor::origin(
                        &(&(*p).pMvdCostTable)[..],
                        (*p).iMvdCostTableSize,
                    );
                    for _ in 0..8 {
                        // Read through it with signed indices of both signs, which is
                        // the whole reason the cursor is not a plain slice.
                        assert_eq!(cursor.at(0), SIZE as u16);
                        assert_eq!(cursor.at(-SIZE), 0);
                        assert_eq!(cursor.at(SIZE), (LEN - 1) as u16);
                        // ... while the other worker writes the context. Each
                        // branch is one worker's own slot, reached as a raw place so
                        // that nothing here forms a borrow of the context itself.
                        if w == 0 {
                            *std::ptr::addr_of_mut!((*p).iActiveThreadsNum) += 1;
                        } else {
                            *std::ptr::addr_of_mut!((*p).iMaxSliceCount) += 1;
                        }
                    }
                });
            }
        });

        assert_eq!(ctx.iActiveThreadsNum, 8i16, "worker 0 wrote only its own slot");
        assert_eq!(ctx.iMaxSliceCount, 8i32, "worker 1 wrote only its own slot");
    }

    /// **`SM_SIZELIMITED_SLICE` at two threads, and the boundary is asserted
    /// rather than assumed** (which is the whole point of
    /// `EncodedFrame::first_mbs`). This mode reaches
    /// `EncodeSizeLimitedSlicesForked` — a third fork, distinct from the one the
    /// probe above drives — where the workers are *partitions* rather than
    /// slices: `UpdateSlicepEncCtxWithPartition` cuts the 49-macroblock frame in
    /// two at macroblock 24, and 24 is not a multiple of the 7-macroblock row.
    /// Measured at this commit, three frames' slice starts were
    /// `[0, 6, 12, 17, 22, 24, 30, 36, 42]`, `[0, 15, 24, 45]` and `[0, 24]`; the
    /// assertion below fails if that ever becomes a row grid, because then this
    /// test would be the row-aligned one twice.
    ///
    /// 112x112 for `MIN_NUM_MB_PER_SLICE`'s reason (see the probe above), and
    /// `uiSliceSizeConstraint` above `MAX_MACROBLOCK_SIZE_IN_BYTE` because
    /// `SliceArgumentValidation` refuses anything at or below it — 1000 rather
    /// than 600 so the IDR comes out in nine slices rather than sixteen, which is
    /// Miri's clock talking.
    ///
    /// **Live under Miri since T9.E8** — round 5 (the cross-slice `uiSliceIdc`
    /// read this comment used to quote) closed when deblocking's guards moved to
    /// `pOverallMbMap` (T9.E4, F142), and what round 5 had been masking on this
    /// probe's path — the `sLayerInfo` array-autoref mint the size-limited
    /// branch's sibling write popped — closed with it (T9.E7, F143).
    ///
    /// **First green run: 3449 s under Miri, 2026-08-24** — two frames, two
    /// workers, a mid-row boundary asserted from the bitstream, zero reports.
    /// Session-scope cost policy is the fixed-slice probe's (D-gate-6): skipped
    /// by name in the session lane, live at `full`/`exit` and by explicit run.
    #[test]
    fn fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row() {
        let (frames, dims) = drive_encoder_over(
            112,
            112,
            2,
            EncoderProbeOptions {
                slice_mode: SliceModeEnum::SM_SIZELIMITED_SLICE,
                slice_constraint: 1000,
                threads: 2,
                ..EncoderProbeOptions::default()
            },
        );

        assert_eq!(dims, (112, 112), "the encoder must be configured for a 7x7 grid");
        let kiMbWidth = dims.0 / 16;
        assert_eq!(frames.len(), 2, "the encode loop did not run to the end");
        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        assert_eq!(frames[0].kind, EVideoFrameType::videoFrameTypeIDR);
        assert_eq!(
            frames[1].kind,
            EVideoFrameType::videoFrameTypeP,
            "the second frame must be inter-coded, or the fork runs over an all-intra \
             frame and the mode-decision half of the tree stays dark"
        );
        assert!(
            frames.iter().all(|f| f.vcl_nals >= 2),
            "a frame carried fewer than two VCL NALs, so the size limit never split it \
             and the fork ran one job: {:?}",
            frames.iter().map(|f| (f.kind, f.vcl_nals)).collect::<Vec<_>>()
        );

        // **The assertion this probe exists for.** A slice that starts at a
        // macroblock which is not the start of a row is a slice whose first row is
        // shared with the previous slice — the case no `&mut [u8]` over the plane
        // can express.
        for f in &frames {
            assert!(
                f.first_mbs.iter().any(|&m| m != 0 && m % kiMbWidth as u32 != 0),
                "every slice of this frame starts on a row boundary, so the probe is \
                 driving the row-aligned case the other probe already covers: \
                 first_mb_in_slice = {:?} at {kiMbWidth} macroblocks per row",
                f.first_mbs
            );
        }
    }

    /// **The second encode probe — CAVLC and the fine mode-decision family, both
    /// knobs flipped together** (Phase 6 session C, face 1).
    ///
    /// The probe above is CABAC over `LOW_COMPLEXITY`, and those two choices leave
    /// two bodies of code dark under Miri: the CAVLC writers
    /// (`svc_set_mb_syn_cavlc.rs`) and everything `bFastMode` switches off —
    /// `WelsMdIntraFinePartition`, `WelsMdI4x4` and the `pMemPredBlk4` ping-pong
    /// (`svc_base_layer_md.rs`). Session C converts sites in both, and **F47 is what
    /// a probe gap costs**: real UB on the ordinary CAVLC path survived five phases
    /// of green gates because no probe drove it.
    ///
    /// **The byte gate does not cover the complexity half either**, which is the
    /// stronger reason this test exists: all 341 diffharness configurations set
    /// `iComplexityMode = LOW_COMPLEXITY` (`diffharness/cxx_enc.cpp:81`) — CABAC vs
    /// CAVLC is a sweep axis (`kiCabac`) but complexity is not — so the fine
    /// partition search is checked by *neither* instrument today. This is the only
    /// coverage it has.
    ///
    /// One test rather than two, per S32: each Miri probe pays a multi-MiB
    /// `Initialize` under the interpreter, and the two knobs are independent code
    /// selections that a single encode drives together. A third probe needs a number
    /// behind it; the size-limited dynamic-slice path
    /// (`WelsMdInterMbLoopOverDynamicSlice`) is named for session D with the slice
    /// structures it converts.
    ///
    /// The assertions are the first probe's, for the first probe's reasons — the
    /// 3x2 macroblock grid read back from the encoder (F34), three frames with the
    /// second inter-coded, and an inter frame an order of magnitude above the
    /// all-skip floor.
    ///
    /// **Two frames under Miri, three everywhere else — the grid probe's shrink,
    /// for the grid probe's reason (F141).** D-gate-5 and F140 name "the two
    /// full-encode Miri probes"; this is a third, in the same cost class
    /// (216.5 s at session E's start against the grid probe's 258.5 s — measured,
    /// not assumed), because it drives the same encode loop through the same
    /// seam. The third frame's marginal coverage is the grid probe's: a second
    /// inter frame over paths frame 1 already ran.
    #[test]
    fn encode_loop_runs_with_cavlc_and_fine_mode_decision_under_the_aliasing_checker() {
        let kiFrames = miri_scaled(3, 2) as usize;
        let (frames, dims) = drive_encoder_over(
            48,
            32,
            kiFrames,
            EncoderProbeOptions {
                cabac: false,
                complexity: ECOMPLEXITY_MODE::MEDIUM_COMPLEXITY,
                ..Default::default()
            },
        );

        assert_eq!(
            dims,
            (48, 32),
            "the encoder must be configured for a 3x2 macroblock grid; a picture \
             without neighbours covers nothing this test exists for"
        );
        assert_eq!(frames.len(), kiFrames, "the encode loop did not run to the end");
        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        assert_eq!(
            frames[0].kind,
            EVideoFrameType::videoFrameTypeIDR,
            "the sequence must open on an IDR"
        );
        assert_eq!(
            frames[1].kind,
            EVideoFrameType::videoFrameTypeP,
            "the second frame must be inter-coded, or no ME/MD path executes at all"
        );
        assert!(
            frames[1].bytes > 200,
            "the inter frame coded {} bytes, which is at the all-skip floor: the \
             source did not move, so motion estimation did nothing",
            frames[1].bytes
        );
    }

    /// **The mint's instrument, re-aimed (S36).** Its predecessor
    /// (`mb_list_root_reaches_the_whole_array_in_both_directions`) proved two
    /// properties of the raw root hand-out: whole-array provenance, and that a
    /// second derivation does not pop the first. The window mint restates both
    /// structurally — a window's reach *is* its range, and an out-of-range access
    /// panics instead of walking — so what is left to pin under Miri is the pair
    /// of properties the fork relies on: a whole-grid window really reaches every
    /// record in both directions from the middle, and **two disjoint windows of
    /// one grid coexist under interleaved writes** (each worker's mint is a
    /// sibling derivation from the array root, F71; overlapping live windows are
    /// the shape the design forbids and does not need).
    #[test]
    // unsafe-cat: instrument(test) — the mint under test takes the raw layer
    #[allow(unsafe_code)]
    fn mb_window_reaches_its_range_and_disjoint_windows_coexist() {
        use crate::encoder::svc_encode_slice::{mb_window, SDqLayer};
        use crate::encoder::md::SMB;
        use crate::safe::mb_grid::{MbArray, MbDims};

        let (w, h) = (5usize, 4usize);
        let mut layer = SDqLayer::default();
        layer.sMbDataP = MbArray::new(MbDims::new(w, h), SMB::default());
        layer.iMbWidth = w as i16;
        layer.iMbHeight = h as i16;
        let p_layer: *mut SDqLayer = &mut layer;

        unsafe {
            // Whole-grid window: stamp every record, then read the whole grid
            // back both ways out from the middle.
            let mut whole = mb_window(&*p_layer, 0, (w * h) as i32, 0);
            for i in 0..(w * h) {
                whole.at_mut(i).iMbXY = i as i32;
            }
            let kiMid = 2 * w + 2;
            whole.set_cur(kiMid);
            let mut seen = 0i64;
            for back in 1..=kiMid {
                seen += whole.at(kiMid - back).iMbXY as i64;
            }
            for fwd in 1..(w * h - kiMid) {
                seen += whole.at(kiMid + fwd).iMbXY as i64;
            }
            let expected: i64 = (0..(w * h) as i64).sum::<i64>() - kiMid as i64;
            assert_eq!(seen, expected, "the whole-grid window did not reach every record");

            // The two neighbour derivations the encoder actually makes.
            assert_eq!(whole.left().iMbXY, (kiMid - 1) as i32, "left neighbour");
            assert_eq!(whole.top().iMbXY, (kiMid - w) as i32, "top neighbour");

            // Two disjoint windows — the fork's shape: worker A's slice and
            // worker B's. Interleave writes and read A's back through A after
            // B has been minted and used: sibling derivations, nothing popped.
            let mut a = mb_window(&*p_layer, 0, 7, 0);
            a.at_mut(3).iMbXY = -3;
            let mut b = mb_window(&*p_layer, 7, (w * h - 7) as i32, 7);
            b.at_mut(9).iMbXY = -9;
            a.at_mut(5).iMbXY = -5;
            assert_eq!(a.at(3).iMbXY, -3, "window B's mint popped window A");
            assert_eq!(b.at(9).iMbXY, -9);
            assert_eq!(a.at(5).iMbXY, -5);
        }
    }

    /// **The dynamic-slice probe — `SM_SIZELIMITED_SLICE`, Phase 6 session D,
    /// face 0.**
    ///
    /// The two probes above encode one slice a frame, so an entire encode path was
    /// dark under Miri: `SM_SIZELIMITED_SLICE` is the only mode with a
    /// macroblock loop of its own (`WelsMdInterMbLoopOverDynamicSlice`,
    /// `WelsISliceMdEncDynamic`), the only caller of the stash-and-rollback pair
    /// (`StashMBStatus`/`StashPopMBStatus`, `wels_func_ptr_def.rs`) and of
    /// `pDynamicBsBuffer`, and the only path that reaches
    /// `CalculateNewSliceNum` → `ReallocSliceBuffer` → `ExtendLayerBuffer` →
    /// `ReOrderSliceInLayer` — **the machinery this session's faces 2 to 4
    /// rewrite**. It found `F60` on its first execution.
    ///
    /// **It is single-threaded, and that is settled by reading rather than by
    /// configuration**: the two flags that put a size-limited encode on Phase 7's
    /// multi-threaded slice path, `bSliceBsBufferFlag` and `bThreadSlcBufferFlag`,
    /// both require `iMultipleThreadIdc > 1` (`InitSliceInLayer`, this file), and
    /// the driver fixes `iMultipleThreadIdc = 1`. The `st` sweep preset already
    /// encodes `sm=3` at constraints 1500 and 600 with one thread.
    ///
    /// **112x96 and a 401-byte constraint, and both numbers are measured.** A slice
    /// closes when its payload passes `uiSliceSizeConstraint - AVER_MARGIN_BYTES`
    /// (100 bytes), and validation refuses any constraint at or below
    /// `MAX_MACROBLOCK_SIZE_IN_BYTE` (400) — so 401 is the finest split the API
    /// allows. At that constraint this source encodes **37 / 9 / 3** slices in its
    /// three frames, against **1 / 1 / 1** at the 1500-byte constraint the sweep
    /// runs, so the multi-slice half is non-vacuous by measurement rather than by
    /// assumption.
    ///
    /// **The geometry is what reaches the realloc, and that is the whole reason it
    /// is not 48x32 like the probes above.** `GetInitialSliceNum` answers
    /// `AVERSLICENUM_CONSTRAINT` = `MAX_SLICES_NUM` = **35** for this mode, so the
    /// layer opens with `iMaxSliceNum = 35`, and `WelsCodeOnePicPartition` calls
    /// `DynSliceRealloc` when `iSliceIdx >= iMaxSliceNum - iActiveThreadsNum`,
    /// i.e. at the 35th slice. A frame therefore has to code **at least 35 slices**
    /// to reach it, which needs at least 35 macroblocks. Measured at the 401-byte
    /// constraint on this source: 48x32 (6 MB) codes 3 slices, 96x64 (24 MB) 21,
    /// 96x96 (36 MB) 31, and **112x96 (42 MB) 37 — the smallest geometry on the
    /// grid that crosses**. `frames[0].vcl_nals >= 35` is the assertion, and it is
    /// exactly the realloc's own trigger condition rather than a proxy for it.
    ///
    /// **`bytes == frame_size` is F60's covering assertion.** `bytes` is summed
    /// through `sLayerInfo[..].pNalLengthInByte`, which is what `FrameBsRealloc`
    /// invalidates and re-stamps; `iFrameSizeInBytes` is accumulated as the slices
    /// are written and survives independently. With the re-stamp deleted this test
    /// reads **686,479,506 bytes against a frame_size of 10,244** at 128x128 and
    /// then dies — measured in both directions.
    ///
    /// The remaining assertions are the first probe's, for the first probe's
    /// reasons: the grid read back from the encoder (F34), three frames with the
    /// second inter-coded, and an inter frame an order of magnitude above the
    /// all-skip floor.
    ///
    /// **48x32 x 2 frames under Miri, 112x96 x 3 everywhere else (D-gate-6).**
    /// D-gate-5's first cut kept 112x96 under Miri because that geometry *is* the
    /// realloc (35 slices trigger `DynSliceRealloc`, and 112x96 is the smallest
    /// grid whose IDR codes 35 — measured above); at the seam's interpreter cost
    /// that made this one test ~13 minutes and the session gate ~40, and the
    /// user capped the gate at 15 (2026-08-24). So under Miri the drive is now
    /// 48x32 at the same 401-byte constraint — measured **3 / 3 / 3** slices
    /// across three frames (64x48 gives 9/4/4 and measured 534 s in the sharded
    /// lane — still the critical path, so the smaller grid it is). Every frame
    /// still splits, each frame still closes slices through
    /// `DynSlcJudgeSliceBoundaryStepBack` / `AddSliceBoundary` / stash-rollback,
    /// and `WelsMdInterMbLoopOverDynamicSlice` and the F60 accounting assertion
    /// stay live. **What the aliasing checker loses at session scope is the
    /// realloc chain itself**
    /// (`CalculateNewSliceNum` -> `ReallocSliceBuffer` -> `ExtendLayerBuffer`):
    /// its assertion below is gated to the full drive, which every native
    /// `cargo test` still runs — and Miri runs it again wherever the battery
    /// exports `MIRI_FULL=1` (the `full`/`exit` tiers), which is where that
    /// coverage now lives. Named plainly per D-cov-1: a session-scope Miri run
    /// no longer sees slice-buffer moves.
    #[test]
    fn encode_loop_runs_over_size_limited_dynamic_slices_under_the_aliasing_checker() {
        let kiFrames = miri_scaled(3, 2) as usize;
        let kiWidth = miri_scaled(112, 48);
        let kiHeight = miri_scaled(96, 32);
        let kbFullDrive = kiWidth == 112;
        let (frames, dims) = drive_encoder_over(
            kiWidth,
            kiHeight,
            kiFrames,
            EncoderProbeOptions {
                slice_mode: SliceModeEnum::SM_SIZELIMITED_SLICE,
                slice_constraint: 401,
                ..Default::default()
            },
        );

        assert_eq!(
            dims,
            (kiWidth, kiHeight),
            "the encoder must be configured for the geometry the drive asked for; \
             at full size that is the 7x6 grid below which no frame can code the \
             35 slices the realloc needs"
        );
        assert_eq!(frames.len(), kiFrames, "the encode loop did not run to the end");
        assert_eq!(
            frames[0].kind,
            EVideoFrameType::videoFrameTypeIDR,
            "the sequence must open on an IDR"
        );
        assert_eq!(
            frames[1].kind,
            EVideoFrameType::videoFrameTypeP,
            "the second frame must be inter-coded, or WelsMdInterMbLoopOverDynamicSlice \
             never runs and this probe covers only the I-slice half"
        );

        // Non-vacuity. A size-limited probe that codes one slice a frame drives the
        // ordinary single-slice path under a different name.
        let slices: Vec<usize> = frames.iter().map(|f| f.vcl_nals).collect();
        assert!(
            slices.iter().all(|&n| n >= 2),
            "every frame must split: {slices:?} slices (measured 37/9/3 at a 401-byte \
             constraint; 1/1/1 at the 1500-byte constraint the sweep runs)"
        );

        // The realloc ran. `iMaxSliceNum` opens at GetInitialSliceNum's answer for
        // this mode (AVERSLICENUM_CONSTRAINT = MAX_SLICES_NUM = 35) and
        // WelsCodeOnePicPartition reallocates before coding slice index
        // `iMaxSliceNum - iActiveThreadsNum` = 34, so >= 35 coded slices is the
        // trigger itself. Full drive only (D-gate-6): the 64x48 Miri geometry
        // cannot reach 35 slices by construction, so at session scope this
        // assertion would only ever restate the geometry choice.
        if kbFullDrive {
            assert!(
                slices[0] >= 35,
                "the IDR coded {} slices, under the 35 that make WelsCodeOnePicPartition \
                 call DynSliceRealloc -> ReallocSliceBuffer -> ExtendLayerBuffer: the \
                 realloc path this probe exists for did not run",
                slices[0]
            );
        }

        // F60. The NAL-length cursors and the frame size are two independent
        // accountings of the same bytes; FrameBsRealloc moves the array the first
        // one reads through, and until this session did not re-stamp it.
        for (i, f) in frames.iter().enumerate() {
            assert_eq!(
                f.bytes as i32, f.frame_size,
                "frame {i}: the NAL lengths sum to {} where the encoder reports a \
                 frame of {} bytes — sLayerInfo[..].pNalLengthInByte is stale (F60)",
                f.bytes, f.frame_size
            );
        }

        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        assert!(
            frames[1].bytes > 200,
            "the inter frame coded {} bytes, which is at the all-skip floor: the \
             source did not move, so motion estimation did nothing",
            frames[1].bytes
        );
    }

}
