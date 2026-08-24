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

use crate::encoder::decode_mb_aux::{
    idct_four_t4_rec_in_place_view, idct_four_t4_rec_to_view, idct_t4_rec_on_mb_in_place_view,
};
use crate::encoder::encode_mb_aux::{blk_four4x4, blk_mb256};
use std::sync::atomic::{AtomicU16, Ordering};
use crate::encoder::picture::{PicRef, RecPicId, SPicture, SRefPicView, SrcPicId};
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
    ctx_dq_layer, ctx_mvd_cost_origin, ctx_param, ctx_pps_array, ctx_rc_at, ctx_ref_list,
    ctx_sps_array, ctx_subset_array,
    ctx_func_list,
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
    // unsafe-cat: port-raw(Phase 9)
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
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub use crate::common::wels_common_defs::EWelsNalUnitType;
pub use crate::safe::bits::BsWriter;
use crate::safe::mb_grid::{MbArray, MbDims};
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
pub unsafe fn slice_in_layer(pCurLayer: *mut SDqLayer, kiSliceIdx: i32) -> *mut SSlice {
    if pCurLayer.is_null() || kiSliceIdx < 0 {
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

/// The bank's slices as a raw pointer to their **root** — T6.D8, and S28's rule
/// again: `AddSliceBoundary` and `ReOrderSliceInLayer` walk *neighbouring* slices out
/// of the pointer they hold, so the pointer must carry the whole bank's provenance.
/// Answers null for a bank that has not been sized.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn slice_bank_root(pCurLayer: *mut SDqLayer, kiBank: usize) -> *mut SSlice {
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
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn slice_in_bank(pCurLayer: *mut SDqLayer, kiBank: usize, kiOffset: i32) -> *mut SSlice {
    let root = slice_bank_root(pCurLayer, kiBank);
    if root.is_null() || kiOffset < 0 {
        return std::ptr::null_mut();
    }
    root.add(kiOffset as usize)
}

/// The layer's macroblock array as a raw pointer to its **root** — T6.D5, and
/// **S28 verbatim**.
///
/// Every `*mut SMB` consumer in the tree walks out of the macroblock it is handed:
/// `pCurMb.offset(-1)` reaches the left neighbour, `.offset(-iMbStride)` the one
/// above, and the mode-decision and deblocking paths do both for every macroblock
/// of every frame. A pointer taken through a narrowing index
/// (`as_mut_slice()[xy..].as_mut_ptr()`) has the correct *address* and provenance
/// for the tail alone, so the first neighbour read walks out of it — safe code,
/// correct output, and UB that no byte-level gate can see. This derives from the
/// array's own root and every caller offsets from there.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn mb_list_root(pCurLayer: *mut SDqLayer) -> *mut SMB {
    // **F71**, as `slice_bank_root`: every worker asks for this same array.
    let mb = std::ptr::addr_of!((*pCurLayer).sMbDataP);
    (*mb).root_ptr()
}

/// The macroblock at `kiMbXY`, derived from the array's root — see [`mb_list_root`].
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn mb_at(pCurLayer: *mut SDqLayer, kiMbXY: i32) -> *mut SMB {
    mb_list_root(pCurLayer).add(kiMbXY as usize)
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
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn current_layer(pCtx: *mut sWelsEncCtx) -> *mut SDqLayer {
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
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn set_current_layer(pCtx: *mut sWelsEncCtx, kIdx: Option<LayerIdx>) {
    debug_assert!(
        kIdx.is_none_or(|i| i.get() < MAX_DEPENDENCY_LAYER),
        "{kIdx:?} is past the largest list InitDqLayers can build"
    );
    (*pCtx).iCurDqLayer = kIdx;
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_sps(pCtx: *mut sWelsEncCtx, pCurLayer: *const SDqLayer) -> *mut SWelsSPS {
    match (*pCurLayer).sLayerInfo.eSps {
        None => std::ptr::null_mut(),
        Some(LayerSps::Avc(id)) => {
            let arr = ctx_sps_array(pCtx);
            if arr.is_null() {
                return std::ptr::null_mut();
            }
            arr.add(id.get())
        }
        Some(LayerSps::Subset(id)) => {
            let arr = ctx_subset_array(pCtx);
            if arr.is_null() {
                return std::ptr::null_mut();
            }
            std::ptr::addr_of_mut!((*arr.add(id.get())).pSps)
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_subset_sps(
    pCtx: *mut sWelsEncCtx,
    pCurLayer: *const SDqLayer,
) -> *mut SSubsetSps {
    match (*pCurLayer).sLayerInfo.eSps {
        Some(LayerSps::Subset(id)) => {
            let arr = ctx_subset_array(pCtx);
            if arr.is_null() {
                return std::ptr::null_mut();
            }
            arr.add(id.get())
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_pps(pCtx: *mut sWelsEncCtx, pCurLayer: *const SDqLayer) -> *mut SWelsPPS {
    let Some(id) = (*pCurLayer).sLayerInfo.iPps else {
        return std::ptr::null_mut();
    };
    let arr = ctx_pps_array(pCtx);
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    arr.add(id.get())
}

/// The context's **active SPS**, resolved from its position — T6.G3.
///
/// `sWelsEncCtx::pSps` was a pointer into `pSpsArray`; `iSps` is the index, and this
/// answers the same address, including **null in the two cases the pointer was
/// null**: before `WelsInitEncoderExt` names one, and before the array exists. The
/// spelling is S40's — `pSpsArray` is raw, so `.add()` on it forms no reference and
/// repeated calls are independent.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ctx_sps(pCtx: *mut sWelsEncCtx) -> *mut SWelsSPS {
    let Some(id) = (*pCtx).iSps else {
        return std::ptr::null_mut();
    };
    let arr = ctx_sps_array(pCtx);
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    debug_assert!((id.get() as i32) < (*pCtx).iSpsNum.max(1), "iSps past iSpsNum");
    arr.add(id.get())
}

/// The context's **active PPS**, resolved from its position — see [`ctx_sps`].
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ctx_pps(pCtx: *mut sWelsEncCtx) -> *mut SWelsPPS {
    let Some(id) = (*pCtx).iPps else {
        return std::ptr::null_mut();
    };
    let arr = ctx_pps_array(pCtx);
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    debug_assert!((id.get() as i32) < (*pCtx).iPpsNum.max(1), "iPps past iPpsNum");
    arr.add(id.get())
}

/// The context's current reference picture, resolved through the current dependency
/// layer's reference list.
///
/// # Safety
/// `pCtx` must be a live encoder context past `RequestMemorySvc`.
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ctx_ref_pic<'a>(pCtx: *mut sWelsEncCtx) -> Option<&'a SPicture> {
    let id = (*pCtx).pRefPic?;
    let pRefList = ctx_ref_list(pCtx, (*pCtx).uiDependencyId as usize);
    if pRefList.is_null() {
        return None;
    }
    Some((*pRefList).pic(id))
}

/// The picture a [`PicRef`] names — the reconstruction pool through the current
/// dependency layer's reference list, or the spatial source pool through the
/// preprocessor. `SDqLayer::pRefOri` is the one field that holds either; see
/// [`PicRef`].
///
/// # Safety
/// As [`ctx_ref_pic`].
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ctx_pic_ref_mut<'a>(pCtx: *mut sWelsEncCtx, r: PicRef) -> Option<&'a mut SPicture> {
    match r {
        PicRef::Rec(id) => {
            let pRefList = ctx_ref_list(pCtx, (*pCtx).uiDependencyId as usize);
            if pRefList.is_null() {
                None
            } else {
                Some((*pRefList).pic_mut(id))
            }
        }
        PicRef::Src(id) => {
            if (*pCtx).pVpp.is_null() {
                None
            } else {
                Some((*(*pCtx).pVpp).m_pSpatialPicPool.get_mut(id))
            }
        }
    }
}

/// Shared form of [`ctx_pic_ref_mut`].
///
/// # Safety
/// As [`ctx_ref_pic`].
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn ctx_pic_ref<'a>(pCtx: *mut sWelsEncCtx, r: PicRef) -> Option<&'a SPicture> {
    match r {
        PicRef::Rec(id) => {
            let pRefList = ctx_ref_list(pCtx, (*pCtx).uiDependencyId as usize);
            if pRefList.is_null() {
                None
            } else {
                Some((*pRefList).pic(id))
            }
        }
        PicRef::Src(id) => {
            if (*pCtx).pVpp.is_null() {
                None
            } else {
                Some((*(*pCtx).pVpp).src_id(id))
            }
        }
    }
}

/// The reconstruction picture this layer is **referencing**, resolved through the
/// reference list the layer was stamped with — `None` before the first inter frame,
/// or if the layer has not been initialised for a frame yet.
///
/// **S37, and the rule this family exists to keep**: the returned borrow is not tied
/// to `pLayer` (the layer is reached raw, so it cannot be), and a caller must not hold
/// it across a call that resolves *another* handle in the same pool. Every consumer
/// below takes what it needs — a stride, a plane root, one array element — and drops
/// the borrow in the same statement.
///
/// # Safety
/// `pLayer` must be a live layer stamped by `WelsInitCurrentLayer`.
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_ref_pic<'a>(pLayer: *mut SDqLayer) -> Option<&'a SPicture> {
    let id = (*pLayer).pRefPic?;
    let pRefList = (*pLayer).pRefList;
    if pRefList.is_null() {
        return None;
    }
    Some((*pRefList).pic(id))
}

/// The **source** picture this layer encodes from, resolved through the spatial pool
/// the layer was stamped with — the [`layer_ref_pic`] of the source half (T9.B21).
///
/// Shared only, and deliberately so. The source picture is read by every consumer and
/// written by none of them on any path a gate runs, so a shared borrow handed out
/// twice is **two siblings, not a stack** — which is what makes it safe to call this
/// per macroblock where `SPicture::planes()` (a `&mut self` accessor, so a fresh
/// exclusive borrow of the whole picture every time) is F73's retag. There is no
/// `layer_enc_pic_mut` for that reason; if one is ever needed, read the `src` caveat
/// in `phase9_plane_census.md` first.
///
/// # Safety
/// `pLayer` must be a live layer stamped by `WelsInitCurrentLayer`.
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_enc_pic<'a>(pLayer: *mut SDqLayer) -> Option<&'a SPicture> {
    let id = (*pLayer).pEncPic?;
    let pSrcPool = (*pLayer).pSrcPool;
    if pSrcPool.is_null() {
        return None;
    }
    Some((*pSrcPool).get(id))
}

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
/// `pLayer` must be a live layer stamped by `WelsInitCurrentLayer`, and the
/// frame it stamped must still be the frame in progress.
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn layer_rec_view<'a>(
    pLayer: *mut SDqLayer,
) -> Option<&'a crate::encoder::rec_view::RecPicView> {
    (*pLayer).pRecView.as_ref()
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
    pub sSliceBufferInfo: [SSliceBufferInfo; MAX_THREADS_NUM],
    /// One entry per slice in layer order, each naming its bank and its offset
    /// in it — T6.D4. See [`SliceIdx`] and [`slice_in_layer`].
    pub ppSliceInLayer: Vec<SliceIdx>,
    pub sSliceEncCtx: SSliceCtx,
    pub pCsData: [*mut u8; 3],
    pub iCsStride: [i32; 3],

    pub pEncData: [*mut u8; 3],
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
    pub pSrcPool: *mut crate::encoder::picture::SrcPicPool,
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
    /// The two pictures above **as this frame sees them** — plane roots, strides and
    /// picture type, stamped once by `WelsInitCurrentLayer` (T6.F5). The handles are
    /// still the truth; these are the per-macroblock world's read path, and they are
    /// `pEncData`/`pCsData`'s own treatment applied to the other two pictures. See
    /// [`SRefPicView`].
    pub sRefPicView: SRefPicView,
    pub sDecPicView: SRefPicView,
    /// The *source* pictures behind the reference list — slots of the preprocessor's
    /// spatial pool, resolved through `pCtx->pVpp` (both readers hold the context).
    pub pRefOri: [Option<PicRef>; MAX_REF_PIC_COUNT as usize],

    pub bThreadSlcBufferFlag: bool,
    pub bSliceBsBufferFlag: bool,
    pub iMaxSliceNum: i32,
    pub NumSliceCodedOfPartition: [i32; MAX_THREADS_NUM],
    pub LastCodedMbIdxOfPartition: [i32; MAX_THREADS_NUM],
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
            sSliceBufferInfo: std::array::from_fn(|_| SSliceBufferInfo::default()),
            // The slice position array, sized by `InitSliceInLayer` and regrown by
            // `ExtendLayerBuffer`; empty is the raw spelling's null.
            ppSliceInLayer: Vec::new(),
            // Zero here means "no slice segmentation yet"; `InitSlicePEncCtx` sets
            // the mode, the geometry and the map.
            sSliceEncCtx: SSliceCtx::default(),
            // Plane aliases into the reconstructed and source pictures, re-aimed at
            // every frame by `WelsInitCurrentLayer`; null means "no frame started".
            pCsData: [std::ptr::null_mut(); 3],
            iCsStride: [0; 3],
            // The seam, rebuilt per frame beside `pCsData`; `None` is "no frame
            // started", the same thing the null above means.
            pRecView: None,
            pEncData: [std::ptr::null_mut(); 3],
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
            pSrcPool: std::ptr::null_mut(),
            sRefPicView: SRefPicView::default(),
            sDecPicView: SRefPicView::default(),
            pRefOri: [None; MAX_REF_PIC_COUNT as usize],
            // Both are `iMultipleThreadIdc > 1` predicates that `InitSliceInLayer`
            // computes; false is the single-threaded answer and the honest default.
            bThreadSlcBufferFlag: false,
            bSliceBsBufferFlag: false,
            // Summed from the banks by `InitSliceInLayer`.
            iMaxSliceNum: 0,
            // Partition bookkeeping, reset per frame by `InitSliceBoundaryInfo` and
            // `WelsInitCurrentQBLayerMltslc`.
            NumSliceCodedOfPartition: [0; MAX_THREADS_NUM],
            LastCodedMbIdxOfPartition: [0; MAX_THREADS_NUM],
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
pub type PWelsCodingSliceFunc = unsafe extern "C" fn(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32;
pub type PWelsSliceHeaderWriteFunc = unsafe extern "C" fn(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut BsWriter,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateNonZeroCountCache(pMb: &SMB, pMbCache: &mut SMbCache) {
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMbToSliceIdc(pCurDq: *mut SDqLayer, kiMbXY: i32) -> u16 {
    if pCurDq.is_null() {
        return u16::MAX;
    }
    // `&`, T9.C5 — as `WelsGetNextMbOfSlice`: nothing here writes, and this runs
    // per macroblock inside the fork.
    let pSliceCtx = &(*pCurDq).sSliceEncCtx;
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateMbNeighbor(pCurDq: *mut SDqLayer, pMb: &mut SMB, kiMbWidth: i32, uiSliceIdc: u16) {
    // **T9.D9**: `pMb.is_null()` went with the parameter — a reference cannot be
    // absent. `pCurDq` stays raw until the layer family.
    if pCurDq.is_null() {
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
        UpdateMbNeighbor(pCurDq, &mut *pMb, kiMbWidth, WelsMbToSliceIdc(pCurDq, (*pMb).iMbXY));
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderScalExtInit(pCurLayer: *mut SDqLayer, pSlice: *mut SSlice) {
    if pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    // **T7.C3.** `addr_of_mut!`, not `&mut`: this is *layer* state and every worker
    // runs this function, so a `&mut` retag here is a write as far as the data-race
    // checker is concerned — and it is only ever read. See
    // `StampLayerIdrFlagForSliceType` for the family and why it is the last of it.
    let pNalHeadExt = std::ptr::addr_of!((*pCurLayer).sLayerInfo.sNalHeaderExt);

    pSliceHeadExt.bSliceSkipFlag = false;

    if (*pNalHeadExt).uiDependencyId > 0 {
        pSliceHeadExt.bAdaptiveBaseModeFlag = false;
        pSliceHeadExt.bAdaptiveMotionPredFlag = false;
        pSliceHeadExt.bAdaptiveResidualPredFlag = false;

        pSliceHeadExt.bDefaultBaseModeFlag = false;
        pSliceHeadExt.bDefaultMotionPredFlag = false;
        pSliceHeadExt.bDefaultResidualPredFlag = false;
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
    let pParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[uiDid];
    pCurSliceHeader.iFrameNum = pParamInternal.iFrameNum;
    pCurSliceHeader.uiIdrPicId = pParamInternal.uiIdrPicId;

    if let Some(id) = (*pEncCtx).pEncPic {
        pCurSliceHeader.iPicOrderCntLsb = (*(*pEncCtx).pVpp).src_id(id).iFramePoc;
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WriteReferenceReorder(buf: &mut [u8], pBs: *mut BsWriter, sSliceHeader: *mut SSliceHeader) {
    if pBs.is_null() || sSliceHeader.is_null() {
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WriteRefPicMarking(buf: &mut [u8], pBs: *mut BsWriter, pSliceHeader: *mut SSliceHeader, pNalHdrExt: *mut SNalUnitHeaderExt) {
    if pBs.is_null() || pSliceHeader.is_null() || pNalHdrExt.is_null() {
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderWrite(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut BsWriter,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    if pBs.is_null() || pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    // Derived, not threaded: this function is reached through
    // `PWelsSliceHeaderWriteFunc`, and widening that signature is Phase 4b's fence.
    // `pBs` is `slice_writer(pCtx, pSlice)` at the only call site, and
    // `slice_bs_buffer` reads the same one-bit choice to pick the buffer.
    let buf = slice_bs_buffer(pCtx, pSlice);
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
        WriteReferenceReorder(buf, pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, pBs, pSliceHeader, pNalHead);
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsSliceHeaderExtWrite(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut BsWriter,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    if pBs.is_null() || pCurLayer.is_null() || pSlice.is_null() {
        return;
    }
    // Derived, not threaded: this function is reached through
    // `PWelsSliceHeaderWriteFunc`, and widening that signature is Phase 4b's fence.
    // `pBs` is `slice_writer(pCtx, pSlice)` at the only call site, and
    // `slice_bs_buffer` reads the same one-bit choice to pick the buffer.
    let buf = slice_bs_buffer(pCtx, pSlice);
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
        WriteReferenceReorder(buf, pBs, pSliceHeader);
    }

    if (*pNalHead).sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, pBs, pSliceHeader, pNalHead);
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsIMbChromaEncode(pEncCtx: *mut sWelsEncCtx, pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
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
    let view_chroma = layer_rec_view(pCurLayer)
        .expect("the layer's reconstruction view is built for this frame");
    let (kiChrOrgX, kiChrOrgY) = (*pMbCache).SPicData.chroma_origin();

    // This previously ran both DCTs and then both IDCTs, omitting the two
    // `WelsEncRecUV` calls between them. That is the quantise / zigzag /
    // non-zero-count / chroma-CBP step: without it `pCurRS` reached the IDCT holding
    // raw DCT coefficients, `pCurMb->uiCbp` never got its chroma bits and
    // `pNonZeroCount[16..24]` stayed zero, so no chroma residual was ever coded.
    let pFunc = ctx_func_list(pEncCtx);
    let pfDctFourT4 = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");

    //cb
    pfDctFourT4(
        std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>(),
        (*pMbCache).SPicData.pEncMb[1],
        kiEncStride,
        std::ptr::addr_of_mut!((*pMbCache).sMemPredMb).cast::<u8>().add(kiBestPredOff),
        8,
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
        std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>().add(64),
        (*pMbCache).SPicData.pEncMb[2],
        kiEncStride,
        std::ptr::addr_of_mut!((*pMbCache).sMemPredMb).cast::<u8>().add(kiBestPredOff + 64),
        8,
    );
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, pMbCache, 64, 2);
    idct_four_t4_rec_to_view(
        &view_chroma.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        &(*pMbCache).sMemPredMb[kiBestPredOff + 64..],
        8,
        blk_four4x4(&(*pMbCache).sCoeffLevel, 64),
    );
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsPMbChromaEncode(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: &mut SMB) {
    let pCurLayer = current_layer(pEncCtx);
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    // **T9.D6**, as in `WelsIMbChromaEncode` — but note the base: this one starts at
    // `pCoeffLevel + 256` (`svc_encode_slice.cpp:499`) where the intra path starts at
    // 0, which is why `WelsEncRecUV` takes the offset as a parameter rather than
    // deriving it from `iUV`.
    let kiBestPredOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);

    let pFunc = ctx_func_list(pEncCtx);
    let dct = (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset");
    dct(
        std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>().add(256),
        (*pMbCache).SPicData.pEncMb[1],
        kiEncStride,
        std::ptr::addr_of_mut!((*pMbCache).sMemPredMb).cast::<u8>().add(kiBestPredOff),
        8,
    );
    dct(
        std::ptr::addr_of_mut!((*pMbCache).sCoeffLevel).cast::<i16>().add(320),
        (*pMbCache).SPicData.pEncMb[2],
        kiEncStride,
        std::ptr::addr_of_mut!((*pMbCache).sMemPredMb).cast::<u8>().add(kiBestPredOff + 64),
        8,
    );

    // `svc_encode_slice.cpp:WelsPMbChromaEncode` quantises both chroma planes here.
    // Both calls were missing, so a P macroblock's chroma reached the reconstruction
    // holding raw DCT coefficients and never set its chroma CBP bits — the same
    // defect Phase 4.5 found in `WelsIMbChromaEncode`.
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 256, 1);
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 320, 2);
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn OutputPMbWithoutConstructCsRsNoCopy(pCtx: *mut sWelsEncCtx, pDq: *mut SDqLayer, pSlice: *mut SSlice, pMb: &SMB) {
    if pCtx.is_null() || pDq.is_null() || pSlice.is_null() {
        return;
    }
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn UpdateQpForOverflow(pCurMb: &mut SMB, kuiChromaQpIndexOffset: u8) {
    (*pCurMb).uiLumaQp = (*pCurMb).uiLumaQp.wrapping_add(DELTA_QP as u8);
    let clamped_idx = CLIP3_QP_0_51((*pCurMb).uiLumaQp as i32 + kuiChromaQpIndexOffset as i32);
    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[clamped_idx];
}

// ============================================================================
// Macroblock Search & Traversal Loops
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsGetNextMbOfSlice(pCurDq: *mut SDqLayer, kiMbXY: i32) -> i32 {
    if pCurDq.is_null() {
        return -1;
    }
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitInterMDStruc(
    pCurMb: *const SMB,
    pMvdCostTable: *mut u16,
    kiMvdInterTableStride: i32,
    pMd: &mut SWelsMD,
) {
    if pCurMb.is_null() {
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsISliceMdEnc(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pCurLayer = current_layer(pEncCtx);
    if pCurLayer.is_null() || (*pCurLayer).sMbDataP.dims().count() == 0 || (*pCurLayer).iMbWidth <= 0 || (*pCurLayer).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    // S29: raw, not `&mut` — held across the MB loop, whose callees derive their
    // own borrows of the same fields (the encode probe's fourth red, session B).
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pSliceHdExt = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt);
    let pMbList = mb_list_root(pCurLayer);
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

    let kbCabac = (*ctx_param(pEncCtx)).iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }

    loop {
        if !kbCabac {
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                func_list
                    .eEntropyCoder
                    .StashMBStatus(slice_bs_buffer(pEncCtx, pSlice), slice_writer(pEncCtx, pSlice), &mut sDss, pSlice, 0);
            }
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx as *mut _, &mut *pCurMb, pSlice as *mut _);
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            sMd.iLambda = g_kiQpCostTable[(*pCurMb).uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, &mut *pCurMb, &mut *pMbCache);
            UpdateNonZeroCountCache(&*pCurMb, &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, pCurMb);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(slice_bs_buffer(pEncCtx, pSlice), slice_writer(pEncCtx, pSlice), &mut sDss, pSlice);
                }
                UpdateQpForOverflow(&mut *pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                func(pCurLayer, &mut *pCurMb, (*pMbCache).bCollocatedPredFlag, I_SLICE);
            }
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx as *mut _,
                &mut *pCurMb,
                sMd.iCostLuma,
                pSlice as *mut _,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsISliceMdEncDynamic(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pBs = slice_writer(pEncCtx, pSlice);
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, not `&mut` — both of these are held across the macroblock loop,
    // whose callees derive their own borrows of the same fields. `sSliceEncCtx` is
    // the dynamic-slice probe's third red (session D): `WelsGetNextMbOfSlice` takes
    // its own `&mut (*pCurDq).sSliceEncCtx` every iteration, which pops the `Unique`
    // this binding held, and `DynSlcJudgeSliceBoundaryStepBack` then reads through
    // the dead tag. `pMbCache` is the encode probe's fourth red (session B).
    let pSliceCtx = std::ptr::addr_of_mut!((*pCurLayer).sSliceEncCtx);
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pSliceHdExt = std::ptr::addr_of_mut!((*pSlice).sSliceHeaderExt);
    let pMbList = mb_list_root(pCurLayer);
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
    if (*ctx_param(pEncCtx)).iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = dynamic_bs_buffer(pEncCtx, kiPartitionId);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    } else {
        sDss.iStartPos = (*pBs).bits_pos();
    }

    loop {
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list
                .eEntropyCoder
                .StashMBStatus(slice_bs_buffer(pEncCtx, pSlice), slice_writer(pEncCtx, pSlice), &mut sDss, pSlice, 0);
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx as *mut _, &mut *pCurMb, pSlice as *mut _);
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*ctx_rc_at(pEncCtx, (*pEncCtx).uiDependencyId as usize)).iMaxQp;
            (*pCurMb).uiLumaQp = max_qp as u8;
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            sMd.iLambda = g_kiQpCostTable[(*pCurMb).uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, &mut *pCurMb, &mut *pMbCache);
            UpdateNonZeroCountCache(&*pCurMb, &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, pCurMb);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(slice_bs_buffer(pEncCtx, pSlice), slice_writer(pEncCtx, pSlice), &mut sDss, pSlice);
                }
                UpdateQpForOverflow(&mut *pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(slice_writer(pEncCtx, pSlice), pSlice);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            pSliceCtx,
            (*pCurMb).iMbXY,
            &mut sDss,
        ) {
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                func_list
                    .eEntropyCoder
                    .StashPopMBStatus(slice_bs_buffer(pEncCtx, pSlice), slice_writer(pEncCtx, pSlice), &mut sDss, pSlice);
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx - 1;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx as *mut _,
                &mut *pCurMb,
                sMd.iCostLuma,
                pSlice as *mut _,
            );
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
unsafe fn mb_dump(pCurMb: &SMB, pMd: &SWelsMD, pSlice: *const SSlice) {
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbLoop(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pWelsMd: &mut SWelsMD,
    kiSliceFirstMbXY: i32,
) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() || current_layer(pEncCtx).is_null() || (*current_layer(pEncCtx)).sMbDataP.dims().count() == 0 || (*current_layer(pEncCtx)).iMbWidth <= 0 || (*current_layer(pEncCtx)).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pBs = slice_writer(pEncCtx, pSlice);
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, held across the MB loop (see `WelsISliceMdEnc`).
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pMbList = mb_list_root(pCurLayer);
    let mut iNumMbCoded = 0;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = ctx_mvd_cost_origin(pEncCtx);
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*ctx_param(pEncCtx)).iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.pRestoreBuffer = std::ptr::null_mut();
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if !kbCabac {
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                func_list.eEntropyCoder.StashMBStatus(
                    slice_bs_buffer(pEncCtx, pSlice),
                    slice_writer(pEncCtx, pSlice),
                    &mut sDss,
                    pSlice,
                    (*pSlice).iMbSkipRun,
                );
            }
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        //step(1): set QP for the current MB
        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx as *mut _, &mut *pCurMb, pSlice as *mut _);
        }

        //step (2). save some value for future use, initial pWelsMd
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            &mut *pMbCache,
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
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, pSlice, pCurMb);
                }

                //step (4): save from the MD process for future use
                {
                    // Two disjoint fields of one picture (T6.F0 — `pMbCache->pEncSad`
                    // no longer carries either), reached through the seam: the pair of
                    // `&mut Vec` borrows this used to take retagged both arrays whole,
                    // which is the shape no worker may hold under the fork.
                    crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                        layer_rec_view(pCurLayer)
                            .expect("the layer's reconstruction picture is bound"),
                        pCurMb,
                        pMd,
                    );
                }

                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        pCurLayer,
                        &mut *pCurMb,
                        (*pMbCache).bCollocatedPredFlag,
                        ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                    );
                }
                mb_dump(&*pCurMb, pMd, pSlice);
            }
            //step (5): update cache
            UpdateNonZeroCountCache(&*pCurMb, &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, pCurMb);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                    (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        slice_bs_buffer(pEncCtx, pSlice),
                        slice_writer(pEncCtx, pSlice),
                        &mut sDss,
                        pSlice,
                    );
                }
                UpdateQpForOverflow(&mut *pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer, pSlice, &*pCurMb);

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx as *mut _,
                &mut *pCurMb,
                (*pMd).iCostLuma,
                pSlice as *mut _,
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(pCurLayer, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        // Derived at the use, after the loop's own derivations (see `WelsCodeOneSlice`).
        BsWriteUE(slice_bs_buffer(pEncCtx, pSlice), &mut *pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbLoopOverDynamicSlice(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pWelsMd: &mut SWelsMD,
    kiSliceFirstMbXY: i32,
) -> i32 {
    if pEncCtx.is_null() || pSlice.is_null() || current_layer(pEncCtx).is_null() || (*current_layer(pEncCtx)).sMbDataP.dims().count() == 0 || (*current_layer(pEncCtx)).iMbWidth <= 0 || (*current_layer(pEncCtx)).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pBs = slice_writer(pEncCtx, pSlice);
    let pCurLayer = current_layer(pEncCtx);
    // S29, both: held across the MB loop, whose callees re-derive the same fields.
    // See `WelsISliceMdEncDynamic` for `sSliceEncCtx`'s red and its invalidator.
    let pSliceCtx = std::ptr::addr_of_mut!((*pCurLayer).sSliceEncCtx);
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pMbList = mb_list_root(pCurLayer);
    let mut iNumMbCoded = 0;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = ctx_mvd_cost_origin(pEncCtx);
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset
    } else {
        0
    };

    let mut sDss = SDynamicSlicingStack::default();
    if (*ctx_param(pEncCtx)).iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
        sDss.pRestoreBuffer = dynamic_bs_buffer(pEncCtx, kiPartitionId);
    } else {
        sDss.iStartPos = (*pBs).bits_pos();
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list.eEntropyCoder.StashMBStatus(
                slice_bs_buffer(pEncCtx, pSlice),
                slice_writer(pEncCtx, pSlice),
                &mut sDss,
                pSlice,
                (*pSlice).iMbSkipRun,
            );
        }
        iCurMbIdx = iNextMbIdx;
        let pCurMb = pMbList.add(iCurMbIdx as usize);

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx as *mut _, &mut *pCurMb, pSlice as *mut _);
        }

        if (*pSlice).bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*ctx_rc_at(pEncCtx, (*pEncCtx).uiDependencyId as usize)).iMaxQp;
            (*pCurMb).uiLumaQp = max_qp as u8;
            (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }

        // step (2): save some values for future use, initialise pWelsMd. Both of
        // these were missing: WelsMdInterInit is what installs the reference-block
        // pointers in pMbCache, so pfInterMd read a null pSample2.
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            pCurMb,
            &mut *pMbCache,
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
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, pSlice, pCurMb);
                }
            }
            // step (4): save from the MD process for future use
            {
                // As above.
                crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                    layer_rec_view(pCurLayer)
                        .expect("the layer's reconstruction picture is bound"),
                    pCurMb,
                    pMd,
                );
            }
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                if let Some(func) = func_list.pfMdBackgroundInfoUpdate {
                    func(
                        pCurLayer,
                        &mut *pCurMb,
                        (*pMbCache).bCollocatedPredFlag,
                        ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                    );
                }
            }
            UpdateNonZeroCountCache(&*pCurMb, &mut *pMbCache);

            let mut iEncReturn = ENC_RETURN_SUCCESS;
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, pCurMb);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && (*pCurMb).uiLumaQp < 50 {
                if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                    (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        slice_bs_buffer(pEncCtx, pSlice),
                        slice_writer(pEncCtx, pSlice),
                        &mut sDss,
                        pSlice,
                    );
                }
                UpdateQpForOverflow(&mut *pCurMb, kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(slice_writer(pEncCtx, pSlice), pSlice);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            pSliceCtx,
            (*pCurMb).iMbXY,
            &mut sDss,
        ) {
            if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
                (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                    slice_bs_buffer(pEncCtx, pSlice),
                    slice_writer(pEncCtx, pSlice),
                    &mut sDss,
                    pSlice,
                );
            }
            (*pCurLayer).LastCodedMbIdxOfPartition[kiPartitionId] = iCurMbIdx - 1;
            (*pCurLayer).NumSliceCodedOfPartition[kiPartitionId] += 1;
            break;
        }

        (*pCurMb).uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, pCurLayer, pSlice, &*pCurMb);

        if let Some(func_list) = ctx_func_list(pEncCtx).as_ref() {
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx as *mut _,
                &mut *pCurMb,
                (*pMd).iCostLuma,
                pSlice as *mut _,
            );
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
        // Derived at the use, after the loop's own derivations (see `WelsCodeOneSlice`).
        BsWriteUE(slice_bs_buffer(pEncCtx, pSlice), &mut *pBs, (*pSlice).iMbSkipRun as u32);
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
    sMd.bMdUsingSad = (*ctx_param(pEncCtx)).iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoop(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsPSliceMdEncDynamic(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, kbIsHighestDlayerFlag: bool) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:715`. The same assignment was already missing from
    // `WelsPSliceMdEnc` and fixed there; this twin still had the defect, so every
    // dynamic-slice P macroblock costed with SATD where LOW_COMPLEXITY costs with
    // SAD.
    sMd.bMdUsingSad = (*ctx_param(pEncCtx)).iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoopOverDynamicSlice(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsCodePSlice(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    let pCurLayer = current_layer(pEncCtx);
    let kbBaseAvail = (*pCurLayer).bBaseLayerAvailableFlag;
    let kbHighestSpatial = if !ctx_param(pEncCtx).is_null() {
        (*ctx_param(pEncCtx)).iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    // `svc_encode_slice.cpp:733/736`. C++ picks `pfInterMd` per slice; the port never
    // assigned it at all, so every P macroblock ran with whatever the slot held.
    (*ctx_func_list(pEncCtx)).pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };
    WelsPSliceMdEnc(pEncCtx, pSlice, kbHighestSpatial)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsCodePOverDynamicSlice(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    let pCurLayer = current_layer(pEncCtx);
    let kbBaseAvail = (*pCurLayer).bBaseLayerAvailableFlag;
    let kbHighestSpatial = if !ctx_param(pEncCtx).is_null() {
        (*ctx_param(pEncCtx)).iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    // `svc_encode_slice.cpp:750/753`, the dynamic-slicing twin of `WelsCodePSlice`.
    (*ctx_func_list(pEncCtx)).pfInterMd = if kbBaseAvail && kbHighestSpatial {
        Some(crate::encoder::svc_mode_decision::WelsMdInterMbEnhancelayer)
    } else {
        Some(crate::encoder::svc_base_layer_md::WelsMdInterMb)
    };
    WelsPSliceMdEncDynamic(pEncCtx, pSlice, kbHighestSpatial)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsCodePSlice_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsCodePSlice(pCtx, pSlice)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsCodePOverDynamicSlice_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsCodePOverDynamicSlice(pCtx, pSlice)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsISliceMdEnc_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsISliceMdEnc(pCtx, pSlice)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsISliceMdEncDynamic_c(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> i32 {
    WelsISliceMdEncDynamic(pCtx, pSlice)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSliceHeaderWrite_c(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut BsWriter,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
) {
    WelsSliceHeaderWrite(pCtx, pBs, pCurLayer, pSlice, pParametersetStrategy);
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsSliceHeaderExtWrite_c(
    pCtx: *mut sWelsEncCtx,
    pBs: *mut BsWriter,
    pCurLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
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
#[inline]
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn slice_bs_buffer<'a>(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> &'a mut [u8] {
    if (*pSlice).sSliceBs.pBs.is_some() {
        thread_bs_buffer(pEncCtx, pSlice)
    } else {
        &mut (&mut *(*pEncCtx).pOut).sBsBuffer[..]
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
/// `pEncCtx`, `(*pEncCtx).pSliceThreading` and `pSlice` must be live, and the slice
/// must have been claimed into a thread slot (`InitOneSliceInThread`).
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn thread_bs_buffer<'a>(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> &'a mut [u8] {
    // **T7.C5, and the spelling is F71's.** The buffer is owned now, so the root has
    // to come out of a `Vec` — and it comes out through `addr_of!` on *this worker's
    // element*, never through a reborrow of the array or the struct that every worker
    // shares. `as_ptr() as *mut u8` returns the buffer's own provenance without a
    // `Unique` retag on the three-word header, which is the difference between a
    // read two workers can make at once and a race.
    let slot = (*pSlice).uiBufferIdx as usize;
    let v = std::ptr::addr_of!((*(*pEncCtx).pSliceThreading).pThreadBsBuffer[slot]);
    bs_buffer((*v).as_ptr() as *mut u8, (*pSlice).sSliceBs.uiSize)
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
// unsafe-cat: cursor
#[allow(unsafe_code)]
pub unsafe fn dynamic_bs_buffer(pEncCtx: *mut sWelsEncCtx, kiPartitionId: usize) -> *mut u8 {
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
#[inline]
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn slice_writer(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) -> *mut BsWriter {
    if (*pSlice).sSliceBs.pBs.is_some() {
        std::ptr::addr_of_mut!((*pSlice).sSliceBs.sBsWrite)
    } else {
        std::ptr::addr_of_mut!((*(*pEncCtx).pOut).sBsWrite)
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
/// # Safety
/// `pEncCtx` must be a live context whose current layer is set for this frame.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn StampLayerIdrFlagForSliceType(pEncCtx: *mut sWelsEncCtx) {
    if pEncCtx.is_null() || (*pEncCtx).eSliceType != EWelsSliceType::I_SLICE {
        return;
    }
    let pCurLayer = current_layer(pEncCtx);
    if pCurLayer.is_null() {
        return;
    }
    (*pCurLayer).sLayerInfo.sNalHeaderExt.bIdrFlag = true;
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsCodeOneSlice(pEncCtx: *mut sWelsEncCtx, pCurSlice: *mut SSlice, kiNalType: i32) -> i32 {
    if pEncCtx.is_null() || pCurSlice.is_null() {
        return ENC_RETURN_INVALIDINPUT;
    }
    let pCurLayer = current_layer(pEncCtx);
    // S29: raw, not `&mut` — this is held across `g_pWelsWriteSliceHeader`, whose
    // two bodies derive `&mut` to the same field (`:816`, `:902`) and popped it
    // (the encode probe's first red on the walk, Phase 6 session B).
    let pNalHeadExt = std::ptr::addr_of_mut!((*pCurLayer).sLayerInfo.sNalHeaderExt);
    let pBs = slice_writer(pEncCtx, pCurSlice);

    let kiDynamicSliceFlag = if !ctx_param(pEncCtx).is_null() {
        let did = (*pEncCtx).uiDependencyId as usize;
        if (*ctx_param(pEncCtx)).sSpatialLayers[did].sSliceArgument.uiSliceMode == SliceMode::SM_SIZELIMITED_SLICE {
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

    WelsSliceHeaderExtInit(pEncCtx, pCurLayer, pCurSlice);

    //RomRC init slice by slice
    let pWelsSvcRc = ctx_rc_at(pEncCtx, (*pEncCtx).uiDependencyId as usize);
    if !pWelsSvcRc.is_null() && (*pWelsSvcRc).bGomRC {
        crate::encoder::rc::GomRCInitForOneSlice(pCurSlice, (*pWelsSvcRc).iBitsPerMb);
    }

    let ext_hdr_idx = if (*pCurSlice).bSliceHeaderExtFlag { 1 } else { 0 };
    (g_pWelsWriteSliceHeader[ext_hdr_idx])(
        pEncCtx,
        pBs,
        pCurLayer,
        pCurSlice,
        // T6.I1: was guarded on the table being non-null; it is owned now.
        (*ctx_func_list(pEncCtx)).pParametersetStrategy.as_deref(),
    );

    let pic_init_qp = if !layer_pps(pEncCtx, pCurLayer).is_null() {
        (*layer_pps(pEncCtx, pCurLayer)).iPicInitQp
    } else {
        26
    };
    (*pCurSlice).uiLastMbQp =
        (pic_init_qp as i32 + (*pCurSlice).sSliceHeaderExt.sSliceHeader.iSliceQpDelta as i32) as u8;

    let idr_idx = (*pNalHeadExt).bIdrFlag as usize;
    let func = g_pWelsSliceCoding[idr_idx][kiDynamicSliceFlag];
    let iEncReturn = func(pEncCtx, pCurSlice);
    if iEncReturn != ENC_RETURN_SUCCESS {
        return iEncReturn;
    }

    // The buffer is derived here, at its use, and not at the top of the function:
    // `slice_bs_buffer` hands back a `&mut` over the whole frame buffer, and every
    // macroblock write inside `func` above derived its own — S29's boundary
    // clause, only ordering fixes it (the encode probe's fifth red, session B).
    WelsWriteSliceEndSyn(
        slice_bs_buffer(pEncCtx, pCurSlice),
        pBs,
        pCurSlice,
        (*ctx_param(pEncCtx)).iEntropyCodingModeFlag != 0,
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsWriteSliceEndSyn(
    buf: &mut [u8],
    pBs: *mut BsWriter,
    pSlice: *mut SSlice,
    bEntropyCodingModeFlag: bool,
) {
    if bEntropyCodingModeFlag {
        crate::encoder::set_mb_syn_cabac::WelsCabacEncodeFlush(buf, &mut (*pSlice).sCabacCtx);
        // Both coders now count in the same units over the same buffer, so
        // handing the position back is an assignment. This used to be
        // `set_pos(end.offset_from(buf.as_ptr()))` around a pointer the coder
        // had derived from an offset in the first place; `BsWriter::set_pos`
        // existed for this one caller and is deleted with it.
        *pBs = BsWriter::at(crate::encoder::set_mb_syn_cabac::WelsCabacEncodePos(
            &mut (*pSlice).sCabacCtx,
        ));
    } else {
        crate::encoder::vlc_encoder::BsRbspTrailingBits(buf, &mut *pBs);
        crate::encoder::vlc_encoder::BsFlush(buf, &mut *pBs);
    }
}

// ============================================================================
// Dynamic Slicing & Boundary Enforcement
// ============================================================================

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AddSliceBoundary(
    pEncCtx: *mut sWelsEncCtx,
    pCurSlice: *mut SSlice,
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
    if pEncCtx.is_null() || pCurSlice.is_null() || pSliceCtx.is_null() {
        return;
    }
    let pCurLayer = current_layer(pEncCtx);
    let buf_idx = (*pCurSlice).uiBufferIdx as usize;
    let pSliceBuffer = slice_bank_root(pCurLayer, buf_idx);
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

        UpdateMbNeighbourInfoForNextSlice(pCurLayer, mb_list_root(pCurLayer), iFirstMbIdxOfNextSlice, kiLastMbIdxInPartition);
    }
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DynSlcJudgeSliceBoundaryStepBack(
    pEncCtx: *mut sWelsEncCtx,
    pCurSlice: *mut SSlice,
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
        let pSmtMutex = {
            let bMt = !ctx_param(pEncCtx).is_null()
                && (*ctx_param(pEncCtx)).iMultipleThreadIdc > 1
                && !(*pEncCtx).pSliceThreading.is_null();
            if bMt {
                (*(*pEncCtx).pSliceThreading).mutexSliceNumUpdate
            } else {
                std::ptr::null_mut()
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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

        let count: &mut Vec<i32> = &mut (*pCurLayer).pCountMbNumInSlice;
        count[iSliceIdx as usize] = iMbNumInSlice;
        let first: &mut Vec<i32> = &mut (*pCurLayer).pFirstMbIdxOfSlice;
        first[iSliceIdx as usize] = iFirstMBInSlice;
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn SetSliceBoundaryInfo(pCurLayer: *mut SDqLayer, pSlice: *mut SSlice, kiSliceIdx: i32) -> i32 {
    if pCurLayer.is_null()
        || pSlice.is_null()
        || (*pCurLayer).pFirstMbIdxOfSlice.is_empty()
        || (*pCurLayer).pCountMbNumInSlice.is_empty()
    {
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceBsBuffer(
    pSlice: *mut SSlice,
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn FreeSliceBuffer(pDqLayer: *mut SDqLayer, kiBank: usize) {
    let bank: &mut Vec<SSlice> = &mut (*pDqLayer).sSliceBufferInfo[kiBank].pSliceBuffer;
    bank.clear();
    bank.shrink_to_fit();
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceList(
    pDqLayer: *mut SDqLayer,
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

        let iRet = InitSliceBsBuffer(pSlice, bIndependenceBsBuffer, kiMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitAllSlicesInThread(pCtx: *mut sWelsEncCtx) -> i32 {
    let pCurDqLayer = current_layer(pCtx);
    for iSliceIdx in 0..(*pCurDqLayer).iMaxSliceNum {
        let slice_ptr = slice_in_layer(pCurDqLayer, iSliceIdx);
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitOneSliceInThread(
    pCtx: *mut sWelsEncCtx,
    pSlice: *mut *mut SSlice,
    kiSlcBuffIdx: i32,
    kiDlayerIdx: i32,
    kiSliceIdx: i32,
) -> i32 {
    let pCurDq = current_layer(pCtx);
    let slc_ptr = if (*pCurDq).bThreadSlcBufferFlag {
        let kiCodedNumInThread = (*pCurDq).sSliceBufferInfo[kiSlcBuffIdx as usize].iCodedSliceNum;
        slice_in_bank(pCurDq, kiSlcBuffIdx as usize, kiCodedNumInThread)
    } else {
        slice_in_bank(pCurDq, 0, kiSliceIdx)
    };
    if slc_ptr.is_null() {
        return ENC_RETURN_UNEXPECTED;
    }

    *pSlice = slc_ptr;
    (*slc_ptr).iSliceIdx = kiSliceIdx;
    (*slc_ptr).uiBufferIdx = kiSlcBuffIdx as u32;

    (*slc_ptr).sSliceBs.uiBsPos = 0;
    (*slc_ptr).sSliceBs.iNalIndex = 0;
    // The C++ stamped `sSliceBs.pBsBuffer = pThreadBsBuffer[kiSlcBuffIdx]` here;
    // `uiBufferIdx` above already names that slot, and `thread_bs_buffer` reads it.
    (*slc_ptr).sSliceBs.uiSize = (*pCtx).iFrameBsSize as u32;

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceThreadInfo(
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIndex: i32,
) -> i32 {
    let iThreadNum = if !ctx_param(pCtx).is_null() {
        (*ctx_param(pCtx)).iMultipleThreadIdc as i32
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

        let iRet = InitSliceList(
            pDqLayer,
            iIdx,
            iMaxSliceNum,
            (*pCtx).iSliceBufferSize[kiDlayerIndex as usize],
            (*pDqLayer).bSliceBsBufferFlag,
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
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIndex: i32,
) -> i32 {
    // S29, and F13's remaining production site. This was `&mut ...sSliceArgument`,
    // whose Unique retag popped the tag of `InitDqLayers`'s `pDlayer` — a pointer
    // into the *same* layer, derived one call up and read again after this function
    // returns. `addr_of_mut!` creates no reference, so the pointer carries the
    // parameter struct's own provenance and there is no retag to pop anything.
    // Found by the encoder aliasing probe (Phase 6 session A) on its first run,
    // reported at `encoder_ext.rs:822`.
    let pSliceArgument = std::ptr::addr_of_mut!(
        (*ctx_param(pCtx)).sSpatialLayers[kiDlayerIndex as usize].sSliceArgument
    );

    (*pDqLayer).bSliceBsBufferFlag = (*ctx_param(pCtx)).iMultipleThreadIdc > 1
        && (*pSliceArgument).uiSliceMode != SliceMode::SM_SINGLE_SLICE;

    (*pDqLayer).bThreadSlcBufferFlag = (*ctx_param(pCtx)).iMultipleThreadIdc > 1
        && (*pSliceArgument).uiSliceMode == SliceMode::SM_SIZELIMITED_SLICE;

    let iRet = InitSliceThreadInfo(pCtx, pDqLayer, kiDlayerIndex);
    if iRet != ENC_RETURN_SUCCESS {
        return ENC_RETURN_MEMALLOCERR;
    }

    (*pDqLayer).iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        (*pDqLayer).iMaxSliceNum += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    // One `Vec` sized to the layer's slice count; `WelsMallocz` zeroed the block
    // and `SliceIdx::NONE` is that zero's meaning — "no slice at this position yet".
    (*pDqLayer).ppSliceInLayer = vec![SliceIdx::NONE; (*pDqLayer).iMaxSliceNum as usize];

    (*pDqLayer).pFirstMbIdxOfSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];
    (*pDqLayer).pCountMbNumInSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];

    let iRet2 = InitSliceBoundaryInfo(pDqLayer, pSliceArgument, (*pDqLayer).iMaxSliceNum);
    if iRet2 != ENC_RETURN_SUCCESS {
        return iRet2;
    }

    let mut iStartIdx = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        for iSliceIdx in 0..(*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum {
            let slices: &mut Vec<SliceIdx> = &mut (*pDqLayer).ppSliceInLayer;
            slices[(iStartIdx + iSliceIdx) as usize] =
                SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
        }
        iStartIdx += (*pDqLayer).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn InitSliceHeadWithBase(pSlice: *mut SSlice, pBaseSlice: *mut SSlice) {
    if pSlice.is_null() || pBaseSlice.is_null() {
        return;
    }
    let pBaseSHExt = &mut (*pBaseSlice).sSliceHeaderExt;
    let pSHExt = &mut (*pSlice).sSliceHeaderExt;

    (*pSlice).bSliceHeaderExtFlag = (*pBaseSlice).bSliceHeaderExtFlag;
    // T6.G3: the C++ copies each id and then the pointer derived from it
    // (`svc_encode_slice.cpp:1169-1172`). The pointers are gone; the ids they were
    // derived from are these two lines, unchanged.
    pSHExt.sSliceHeader.iPpsId = pBaseSHExt.sSliceHeader.iPpsId;
    pSHExt.sSliceHeader.iSpsId = pBaseSHExt.sSliceHeader.iSpsId;
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ReallocateSliceList(
    pCtx: *mut sWelsEncCtx,
    pSliceArgument: *mut SSliceArgument,
    pDqLayer: *mut SDqLayer,
    kiBank: usize,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    if pDqLayer.is_null() || pSliceArgument.is_null() || kiMaxSliceNumNew < kiMaxSliceNumOld {
        return ENC_RETURN_INVALIDINPUT;
    }

    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let iMaxSliceBufferSize = (*pCtx).iSliceBufferSize[kiCurDid];
    let bIndependenceBsBuffer = (*ctx_param(pCtx)).iMultipleThreadIdc > 1
        && (*pSliceArgument).uiSliceMode != SliceMode::SM_SINGLE_SLICE;

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
    let pBaseSlice = slice_in_bank(pDqLayer, kiBank, 0);

    for iSliceIdx in kiMaxSliceNumOld..kiMaxSliceNumNew {
        let pSlice = slice_in_bank(pDqLayer, kiBank, iSliceIdx);
        (*pSlice).iSliceIdx = -1;
        (*pSlice).uiBufferIdx = 0;
        (*pSlice).iCountMbNumInSlice = 0;
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let mut iRet = InitSliceBsBuffer(pSlice, bIndependenceBsBuffer, iMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        InitSliceHeadWithBase(pSlice, pBaseSlice);
        InitSliceRefInfoWithBase(pSlice, pBaseSlice, (*pCtx).iNumRef0);

        iRet = InitSliceRC(pSlice, (*pCtx).iGlobalQp);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
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
    let pCurLayer = current_layer(pCtx);
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

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ReallocateSliceInThread(
    pCtx: *mut sWelsEncCtx,
    pDqLayer: *mut SDqLayer,
    kiDlayerIdx: i32,
    KiSlcBuffIdx: i32,
) -> i32 {
    let iMaxSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iMaxSliceNum;
    let iCodedSliceNum = (*pDqLayer).sSliceBufferInfo[KiSlcBuffIdx as usize].iCodedSliceNum;
    let mut iMaxSliceNumNew = 0;
    let pLastCodedSlice = slice_in_bank(pDqLayer, KiSlcBuffIdx as usize, iCodedSliceNum - 1);
    // **T7.C5, F71's idiom at the one site the workers still reached it from.**
    // `&mut` here is a `Unique` retag over *shared* parameter state — this function
    // runs on a worker (`EncodeOnePartitionSizeLimited`), every worker resolves the
    // same layer's slice argument, and `ReallocateSliceList` only ever reads it.
    // `addr_of_mut!` creates no reference, so two workers growing their own banks at
    // the same instant no longer race on this borrow.
    let pSliceArgument = std::ptr::addr_of_mut!(
        (*ctx_param(pCtx)).sSpatialLayers[kiDlayerIdx as usize].sSliceArgument
    );

    let mut iRet = CalculateNewSliceNum(pCtx, pLastCodedSlice, iMaxSliceNum, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet = ReallocateSliceList(
        pCtx,
        pSliceArgument,
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
    pCtx: *mut sWelsEncCtx,
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
pub unsafe fn ReallocSliceBuffer(pCtx: *mut sWelsEncCtx) -> i32 {
    let pCurLayer = current_layer(pCtx);
    let iMaxSliceNumOld = (*pCurLayer).sSliceBufferInfo[0].iMaxSliceNum;
    let mut iMaxSliceNumNew = 0;
    let kiCurDid = (*pCtx).uiDependencyId as usize;
    let pLastCodedSlice = slice_in_bank(pCurLayer, 0, iMaxSliceNumOld - 1);
    let pSliceArgument = &mut (*ctx_param(pCtx)).sSpatialLayers[kiCurDid].sSliceArgument;

    let mut iRet = CalculateNewSliceNum(pCtx, pLastCodedSlice, iMaxSliceNumOld, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    iRet = ReallocateSliceList(pCtx, pSliceArgument, pCurLayer, 0, iMaxSliceNumOld, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn CheckAllSliceBuffer(pCurLayer: *mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    for iSliceIdx in 0..kiCodedSliceNum {
        let slice_ptr = slice_in_layer(pCurLayer, iSliceIdx);
        if slice_ptr.is_null() || iSliceIdx != (*slice_ptr).iSliceIdx {
            return ENC_RETURN_UNEXPECTED;
        }
    }
    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn ReOrderSliceInLayer(pCtx: *mut sWelsEncCtx, kuiSliceMode: SliceMode, kiThreadNum: i32) -> i32 {
    let pCurLayer = current_layer(pCtx);
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
            let pSliceBuffer = slice_in_bank(pCurLayer, iSlcBuffIdx as usize, iSliceIdx);
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

    CheckAllSliceBuffer(pCurLayer, iEncodeSliceNum)
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetCurLayerNalCount(pCurDq: *mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    let mut iTotalNalCount = 0;
    for iSliceIdx in 0..kiCodedSliceNum {
        let slice_ptr = slice_in_layer(pCurDq, iSliceIdx);
        if !slice_ptr.is_null() && (*slice_ptr).sSliceBs.uiBsPos > 0 {
            iTotalNalCount += (*slice_ptr).sSliceBs.iNalIndex;
        }
    }
    iTotalNalCount
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetTotalCodedNalCount(pFbi: *mut SFrameBSInfo) -> i32 {
    let mut iTotalCodedNalCount = 0;
    for iNalIdx in 0..MAX_LAYER_NUM_OF_FRAME {
        iTotalCodedNalCount += (*pFbi).sLayerInfo[iNalIdx].iNalCount;
    }
    iTotalCodedNalCount
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn GetCurrentSliceNum(pCurDq: *const SDqLayer) -> i32 {
    if pCurDq.is_null() {
        -1
    } else {
        (*pCurDq).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed)
    }
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
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kiMaxSliceNumOld: i32,
) -> i32 {
    let pOut = &mut *(*pCtx).pOut;
    let mut iCountNals = pOut.sNalList.len() as i32;
    let spatial_layers = if !ctx_param(pCtx).is_null() { (*ctx_param(pCtx)).iSpatialLayerNum } else { 1 };
    iCountNals += kiMaxSliceNumOld * (spatial_layers + if (*pCtx).bNeedPrefixNalFlag { 1 } else { 0 });

    // Was: allocate a bigger block, `copy_nonoverlapping` the old contents in,
    // free the old, store the new — twice, with a null check each. `Vec::resize`
    // is the same three steps and keeps the same guarantee, that the existing
    // `iCountNals` entries survive at their indices and the new tail is zeroed
    // (`WelsMallocz` zeroed it too).
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
    pCtx: *mut sWelsEncCtx,
    pFrameBsInfo: *mut SFrameBSInfo,
    pLayerBsInfo: *mut SLayerBSInfo,
    kuiSliceMode: SliceMode,
) -> i32 {
    let mut iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..(*pCtx).iActiveThreadsNum {
        iMaxSliceNum += (*current_layer(pCtx)).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    if iMaxSliceNum > (*current_layer(pCtx)).iMaxSliceNum {
        let iRet = ExtendLayerBuffer(pCtx, (*current_layer(pCtx)).iMaxSliceNum, iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        (*current_layer(pCtx)).iMaxSliceNum = iMaxSliceNum;
    }

    let mut iRet = ReOrderSliceInLayer(pCtx, kuiSliceMode, (*pCtx).iActiveThreadsNum as i32);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let iCodedSliceNum = GetCurrentSliceNum(current_layer(pCtx));
    (*pLayerBsInfo).iNalCount = GetCurLayerNalCount(current_layer(pCtx), iCodedSliceNum);
    let iCodedNalCount = GetTotalCodedNalCount(pFrameBsInfo);

    if iCodedNalCount > (*(*pCtx).pOut).sNalList.len() as i32 {
        iRet = FrameBsRealloc(pCtx, pFrameBsInfo, pLayerBsInfo, (*current_layer(pCtx)).iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn WelsInitSliceEncodingFuncs(uiCpuFlag: u32) {
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
    #[test]
    fn encode_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker() {
        let kiFrames = if cfg!(miri) { 2 } else { 3 };
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
    /// **Ignored under Miri, and F73 is why — not a skip without a finding.** This
    /// probe was written to make deleting F12's `--skip wels_thread_pool` mean
    /// something, and it has now found four things by being run: F70 (a dead-tag read
    /// in `InitSliceSettings`, fixed), F71 (the root-accessor family taking `&mut` to
    /// shared context state — sixteen accessors at T7.B5, **the last of it at T7.C3**,
    /// closed), and F73.
    ///
    /// **F71 is closed and this attribute outlived it**, which is worth saying
    /// plainly. F71's named residue was one shared *write* —
    /// `WelsCodeOneSlice` stamping `sLayerInfo.sNalHeaderExt` per slice per worker.
    /// T7.C3 hoisted it out of the fork (`StampLayerIdrFlagForSliceType`) and made the
    /// two remaining layer-header derivations raw, and Miri's next answer was a
    /// **different class**: `&mut` retags over the *reconstruction picture*
    /// (`layer_dec_pic_mut` -> `SRefList::pic_mut` -> `&mut SPicture`, and
    /// `SPicture::planes`' `&mut self`), taken by every worker while each writes its
    /// own disjoint macroblock rows. The aliasing is in how the port **reaches** the
    /// picture, not in what it writes, and unpicking it is 32 `planes()` sites and 68
    /// `_mut` picture-accessor calls across the preprocessor, the reference-list
    /// manager and the encode tree — F67's context split, i.e. Phase 9's, not a site.
    /// That is F73, and the attribute retires with it.
    ///
    /// The test runs normally in both profiles and is the only coverage the fork/join
    /// has outside the diffharness.
    #[test]
    #[cfg_attr(miri, ignore)]
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
    /// **Ignored under Miri, and what remains is named rather than shrugged at**
    /// (T9.C6, re-measured T9.C2). `SSliceCtx` was this probe's verdict for two
    /// rounds and is no longer: T9.C2 made `pOverallMbMap` and `iSliceNumInFrame`
    /// atomic (F136), closing F132's round 6. What the probe reports now is
    /// **round 5 — the `&mut SMB` family**, measured at this commit:
    ///
    /// ```text
    /// Data race between (1) non-atomic read on thread `unnamed-3`
    ///                and (2) retag write of type `encoder::md::SMB` on `unnamed-2`
    ///   (2) svc_encode_slice.rs:1464  UpdateMbNeighbor(pCurDq, &mut *pMb, ..)
    ///   (1) deblocking.rs:1190        (*pCurMb).uiSliceIdc
    ///                                   == (*pCurMb.offset(-iMbStride)).uiSliceIdc
    /// ```
    ///
    /// That is deblocking's cross-slice read of a neighbour's `SMB.uiSliceIdc`
    /// against the `&mut SMB` the worker encoding that neighbour holds —
    /// F112/F114b's family and session E's 31 neighbour-bound `*mut SMB`
    /// parameters. **Both encoder fork/join probes now stop on the same thing**,
    /// where before they stopped on two different families.
    ///
    /// It is not the reconstruction write and it is not in this session's lane;
    /// the finding carries the full enumeration. The attribute retires with it,
    /// and this comment is the ratchet.
    #[test]
    #[cfg_attr(miri, ignore)]
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
        let kiFrames = if cfg!(miri) { 2 } else { 3 };
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

    /// **S28's mandated test for [`mb_list_root`]** — Phase 6 session D, face 2.
    ///
    /// S28 says every accessor that hands a raw pointer out of a safe container gets
    /// a Miri test that reads the pointer's **full legal reach in both directions**,
    /// because that class is invisible to every byte-level gate: a pointer derived
    /// through `as_mut_slice()[xy..].as_mut_ptr()` has the right address and
    /// provenance for the tail alone, and nine gates passed on exactly that in Phase
    /// 5 (T5.C3).
    ///
    /// The reach here is the whole macroblock array, and **backwards is the direction
    /// that matters**: the encoder's neighbour reads are `pCurMb.offset(-1)` (left)
    /// and `.offset(-iMbStride)` (above), so a pointer to the macroblock in the
    /// middle of the grid must be able to walk to index 0 and to the last index.
    /// Under the narrowing spelling the first backwards step is UB.
    ///
    /// It also exercises the second half of the accessor's contract: **two
    /// derivations must coexist**. `mb_list_root` is called once per function in
    /// production and `mb_at` calls it again, so the second call must not invalidate
    /// the first one's pointer — which is why it reads the `Vec`'s own stored pointer
    /// rather than reborrowing the buffer through `as_mut_slice()`.
    #[test]
    // unsafe-cat: port-raw(Phase 9)
    #[allow(unsafe_code)]
    fn mb_list_root_reaches_the_whole_array_in_both_directions() {
        use crate::encoder::svc_encode_slice::{mb_at, mb_list_root, SDqLayer};
        use crate::encoder::md::SMB;
        use crate::safe::mb_grid::{MbArray, MbDims};

        let (w, h) = (5usize, 4usize);
        let mut layer = SDqLayer::default();
        layer.sMbDataP = MbArray::new(MbDims::new(w, h), SMB::default());
        layer.iMbWidth = w as i16;
        layer.iMbHeight = h as i16;
        let p_layer: *mut SDqLayer = &mut layer;

        unsafe {
            // Stamp every macroblock through the root, so the writes below have
            // something to disagree with.
            let root = mb_list_root(p_layer);
            for i in 0..(w * h) {
                (*root.add(i)).iMbXY = i as i32;
            }

            // The middle of the grid, and its whole legal reach both ways.
            let kiMid = (2 * w + 2) as i32;
            let p_mid = mb_at(p_layer, kiMid);
            let mut seen = 0i64;
            for back in 1..=kiMid {
                seen += (*p_mid.offset(-(back as isize))).iMbXY as i64;
            }
            for fwd in 1..((w * h) as i32 - kiMid) {
                seen += (*p_mid.offset(fwd as isize)).iMbXY as i64;
            }
            let expected: i64 = (0..(w * h) as i64).sum::<i64>() - kiMid as i64;
            assert_eq!(seen, expected, "the root-derived pointer did not reach every macroblock");

            // The two neighbour derivations the encoder actually makes.
            assert_eq!((*p_mid.offset(-1)).iMbXY, kiMid - 1, "left neighbour");
            assert_eq!((*p_mid.offset(-(w as isize))).iMbXY, kiMid - w as i32, "top neighbour");

            // A second derivation must not invalidate the first: write through the
            // older pointer after taking a newer one, then read it back through the
            // newer one.
            let p_first = mb_list_root(p_layer);
            let p_second = mb_at(p_layer, 7);
            (*p_first.add(7)).iMbXY = -99;
            assert_eq!((*p_second).iMbXY, -99, "the second derivation popped the first");
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
    /// **Two frames under Miri, three everywhere else (D-gate-5, the `scale()`
    /// pattern) — and the geometry does not shrink, because the geometry is the
    /// realloc.** 112x96 is the smallest grid whose IDR codes the 35 slices that
    /// trigger `DynSliceRealloc` (measured above), so a smaller Miri geometry
    /// would silently drop the realloc chain — the buffer moves this probe
    /// exists for, and F60's shape — from the aliasing checker exactly while the
    /// slice family converts under it. What Miri loses instead is frame 2, the
    /// second inter frame: 3 slices against frame 1's 9, the same
    /// `WelsMdInterMbLoopOverDynamicSlice` / stash-rollback / boundary-step-back
    /// paths frame 1 drives. Every surviving assertion runs on frames 0 and 1;
    /// full size on every plain `cargo test`. (F140 measured this probe at
    /// >19 minutes under Miri un-shrunk; the two-frame drive keeps the IDR's 37
    /// slices and the realloc assertion below intact.)
    #[test]
    fn encode_loop_runs_over_size_limited_dynamic_slices_under_the_aliasing_checker() {
        let kiFrames = if cfg!(miri) { 2 } else { 3 };
        let (frames, dims) = drive_encoder_over(
            112,
            96,
            kiFrames,
            EncoderProbeOptions {
                slice_mode: SliceModeEnum::SM_SIZELIMITED_SLICE,
                slice_constraint: 401,
                ..Default::default()
            },
        );

        assert_eq!(
            dims,
            (112, 96),
            "the encoder must be configured for a 7x6 macroblock grid; below 35 \
             macroblocks no frame can code the 35 slices the realloc needs"
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
        // trigger itself.
        assert!(
            slices[0] >= 35,
            "the IDR coded {} slices, under the 35 that make WelsCodeOnePicPartition \
             call DynSliceRealloc -> ReallocSliceBuffer -> ExtendLayerBuffer: the \
             realloc path this probe exists for did not run",
            slices[0]
        );

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
