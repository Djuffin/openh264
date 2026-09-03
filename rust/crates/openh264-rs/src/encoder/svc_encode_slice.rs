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

/// `rc.h:77` says **2**. `UpdateQpForOverflow` is the only user.
pub use crate::encoder::rc::DELTA_QP;
pub const MB_COEFF_LIST_SIZE: usize = 384;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_LUMA_CHROMA_BLOCK4x4_NUM: usize = 24;
// wels_const.h:69 says 4.
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

// `wels_common_defs.h:275-285`.
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
#[derive(Debug, Copy, Clone, Default)]
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
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub uiIdrPicId: u16,
    pub bNumRefIdxActiveOverrideFlag: bool,
    pub uiPadding1Bytes: u8,
    pub sRefMarking: SRefPicMarking,
    pub sRefReordering: SRefPicListReorderSyntax,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
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

pub use crate::common::wels_common_defs::EWelsNalUnitType;
use crate::safe::plane::PlaneCursor;
pub use crate::safe::bits::BsWriter;
use crate::safe::mb_grid::{MbArray, MbDims, MbWindow};
use crate::safe::mvd_cost::MvdCostCursor;
pub use crate::encoder::set_mb_syn_cabac::SCabacCtx;
use crate::encoder::paraset_strategy::CWelsParametersetIdStrategyObj;

/// `TagSlice` — `codec/encoder/core/inc/slice.h:170`. 1584 bytes in the C++.
#[repr(C)]
pub struct SSlice {
    pub sMbCacheInfo: SMbCache,
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
    pub fn new() -> Self {
        Self {
            // Per-macroblock scratch: 5600 bytes of inline arrays, and every one of
            // them is written before it is read, per macroblock.
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
#[derive(Debug)]
pub struct SDynamicSlicingStack<'a> {
    pub iStartPos: i32,
    pub iCurrentPos: i32,
    /// The CAVLC rollback snapshot.
    pub sBsStack: BsWriter,
    pub sStoredCabac: crate::encoder::set_mb_syn_cabac::SCabacCtx,
    pub iMbSkipRunStack: i32,
    pub uiLastMbQp: u8,
    /// The CABAC restore scratch, one of `pDynamicBsBuffer`'s per-partition
    /// allocations. `None` for CAVLC dynamic slicing, and every fixed mode.
    pub pRestoreBuffer: Option<&'a mut [u8]>,
}

impl Default for SDynamicSlicingStack<'_> {
    fn default() -> Self {
        Self {
            iStartPos: 0,
            iCurrentPos: 0,
            sBsStack: BsWriter::new(),
            sStoredCabac: crate::encoder::set_mb_syn_cabac::SCabacCtx::default(),
            iMbSkipRunStack: 0,
            uiLastMbQp: 0,
            pRestoreBuffer: None,
        }
    }
}

/// `TagSliceBufferInfo` — `codec/encoder/core/inc/svc_enc_frame.h:71`. 16 bytes in
/// the C++; not `repr(C)`, because `pSliceBuffer` is a `Vec<SSlice>`.
pub struct SSliceBufferInfo {
    /// The bank's slices, **owned**.
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

/// **Which array a layer's active SPS lives in.**
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
/// Not `repr(C)`: `Option<LayerSps>` has no C shape. The C++'s three
/// parameter-set pointers are two fields here, and neither is an address — see
/// [`LayerSps`].
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    /// The layer's active SPS and which array it is in — `pSubsetSpsP` + `pSpsP`.
    pub eSps: Option<LayerSps>,
    /// The layer's active PPS, as a position in `pCtx->pPPSArray` — `pPpsP`.
    pub iPps: Option<PpsId>,
}

impl Default for SLayerInfo {
    /// **Field-wise, and it has to be**: `Option<LayerSps>` and `Option<PpsId>`
    /// have no niche — `LayerSps`'s payloads are plain integers — so the all-zero
    /// image of this struct is `Some(Avc(SpsId(0)))` and `Some(PpsId(0))`, a layer
    /// that already has parameter sets.
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

/// A layer's position in `sWelsEncCtx::ppDqLayerList`.
///
/// The list is `iSpatialLayerNum` entries built once in `InitDqLayers` and freed
/// once in `FreeDqLayer`, and **nothing permutes it**: `WelsSwapDqLayers`
/// reassigns `pCurDqLayer` and stamps the outgoing layer's index, and no
/// `swap`/`rotate`/`retain`/`remove`/`sort`/`drain` touches the list anywhere in
/// the tree. So a position is a stable identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerIdx(pub u8);

impl LayerIdx {
    #[inline(always)]
    pub fn get(self) -> usize {
        self.0 as usize
    }
}

/// A slice's position in the layer's slice **banks**: an entry names
/// (bank, offset).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SliceIdx {
    pub bank: u8,
    pub offset: i32,
}

impl SliceIdx {
    /// The value an unfilled entry holds — `ReOrderSliceInLayer` fills the tail of
    /// the array with the banks' uncoded slices, so "unfilled" only ever means
    /// "before the first fill".
    pub const NONE: SliceIdx = SliceIdx { bank: u8::MAX, offset: -1 };
}

/// The bank's slices as an **exclusive slice**, for the callers that hold the
/// layer `&mut`. `None` for a bank that has not been sized.
#[inline]
pub fn slice_bank_mut(pCurLayer: &mut SDqLayer, kiBank: usize) -> Option<&mut [SSlice]> {
    let bank = pCurLayer.sSliceBufferInfo.get_mut(kiBank)?;
    if bank.pSliceBuffer.is_empty() {
        return None;
    }
    Some(&mut bank.pSliceBuffer)
}

/// The slice at `kiOffset` in bank `kiBank`, exclusively. See [`slice_bank_mut`].
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

/// Slice `kiSliceIdx` of the layer, exclusively. Resolves through
/// `ppSliceInLayer`.
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

/// The current layer as a **shared reference**. `None` when no layer is stamped
/// for the frame.
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
#[inline]
pub fn current_layer_mut(pCtx: &mut sWelsEncCtx) -> Option<&mut SDqLayer> {
    let idx = pCtx.iCurDqLayer?;
    debug_assert!(
        idx.get() < MAX_DEPENDENCY_LAYER,
        "iCurDqLayer = {idx:?} is past the largest list InitDqLayers can build"
    );
    pCtx.ppDqLayerList.get_mut(idx.get())?.as_deref_mut()
}

/// The current layer, for the readers that do not ask — the frame loop's
/// `pCtx->pCurDqLayer`.
///
/// # Panics
/// If no layer is stamped for the frame — `iCurDqLayer` unset, or the list not
/// built by `InitDqLayers`. The callers that *do* ask keep asking, through
/// [`current_layer_ref`].
#[inline]
pub fn current_layer_expect(pCtx: &sWelsEncCtx) -> &SDqLayer {
    current_layer_ref(pCtx).expect("the frame's current layer is stamped")
}

/// [`current_layer_expect`] mutably — the writers that stamp the layer.
///
/// **Single-threaded only, and the type says so.** A `&mut sWelsEncCtx` cannot
/// exist while the fork is live (every worker holds `&sWelsEncCtx`), so this
/// accessor is unavailable in exactly the place a `&mut SDqLayer` would be a
/// race.
///
/// # Panics
/// As [`current_layer_expect`]; the asking callers keep [`current_layer_mut`].
#[inline]
pub fn current_layer_expect_mut(pCtx: &mut sWelsEncCtx) -> &mut SDqLayer {
    current_layer_mut(pCtx).expect("the frame's current layer is stamped")
}

/// Make `kIdx` the current layer — the only writer of
/// `sWelsEncCtx::iCurDqLayer`. `None` un-sets it, which no live path does.
#[inline]
pub fn set_current_layer(pCtx: &mut sWelsEncCtx, kIdx: Option<LayerIdx>) {
    debug_assert!(
        kIdx.is_none_or(|i| i.get() < MAX_DEPENDENCY_LAYER),
        "{kIdx:?} is past the largest list InitDqLayers can build"
    );
    pCtx.iCurDqLayer = kIdx;
}

/// The layer's active PPS **as a shared reference**. The PPS array itself is
/// written only before the fork.
#[inline]
pub fn layer_pps_ref<'a>(pCtx: &'a sWelsEncCtx, pCurLayer: &SDqLayer) -> Option<&'a SWelsPPS> {
    pCtx.pps_array().get(pCurLayer.sLayerInfo.iPps?.get())
}

/// A layer's active SPS **as a shared reference**. Answers `None` when no SPS is
/// named, or the array is empty; the subset arm answers the embedded AVC SPS.
#[inline]
pub fn layer_sps_ref<'a>(pCtx: &'a sWelsEncCtx, pCurLayer: &SDqLayer) -> Option<&'a SWelsSPS> {
    match pCurLayer.sLayerInfo.eSps {
        None => None,
        Some(LayerSps::Avc(id)) => pCtx.sps_array().get(id.get()),
        Some(LayerSps::Subset(id)) => pCtx.subset_array().get(id.get()).map(|s| &s.pSps),
    }
}

/// A layer's subset SPS as a shared reference; `None` on the AVC arm.
#[inline]
pub fn layer_subset_sps_ref<'a>(pCtx: &'a sWelsEncCtx, pCurLayer: &SDqLayer) -> Option<&'a SSubsetSps> {
    match pCurLayer.sLayerInfo.eSps {
        Some(LayerSps::Subset(id)) => pCtx.subset_array().get(id.get()),
        _ => None,
    }
}

/// The context's **active SPS**, resolved from its position. Null in two cases:
/// before `WelsInitEncoderExt` names one, and before the array exists.
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
    // `wrapping_add` computes the address `.add` computed without making the
    // in-bounds claim, which the `debug_assert` above makes instead.
    arr.as_ptr().cast_mut().wrapping_add(id.get())
}

/// The context's active SPS **as a shared reference**. `None` in the two cases
/// [`ctx_sps`] returns null: before `WelsInitEncoderExt` names an SPS, and before
/// the array exists.
#[inline]
pub fn ctx_sps_ref(pCtx: &sWelsEncCtx) -> Option<&SWelsSPS> {
    pCtx.sps_array().get(pCtx.iSps?.get())
}

/// The context's active PPS **as a shared reference**.
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
    arr.as_ptr().cast_mut().wrapping_add(id.get())
}

/// The context's current reference picture, resolved through the current dependency
/// layer's reference list.
#[inline]
pub fn ctx_ref_pic<'a>(pCtx: &'a sWelsEncCtx) -> Option<&'a SPicture> {
    let id = (*pCtx).pRefPic?;
    let pRefList = (*pCtx).ref_list((*pCtx).uiDependencyId as usize)?;
    Some(pRefList.pic(id))
}

/// The picture a [`PicRef`] names — the reconstruction pool through the current
/// dependency layer's reference list, or the spatial source pool through the
/// preprocessor. `SDqLayer::pRefOri` is the one field that holds either; see
/// [`PicRef`].
#[inline]
pub fn ctx_pic_ref<'a>(pCtx: &'a sWelsEncCtx, r: PicRef) -> Option<&'a SPicture> {
    match r {
        PicRef::Rec(id) => (*pCtx)
            .ref_list((*pCtx).uiDependencyId as usize)
            .map(|pRefList| pRefList.pic(id)),
        PicRef::Src(id) => {
            if (*pCtx).pVpp.is_none() {
                None
            } else {
                Some(crate::encoder::encoder_context::ctx_vpp_ref(pCtx).src_id(id))
            }
        }
    }
}

/// The reconstruction picture this layer is **referencing**, resolved through the
/// reference list the layer was stamped with — `None` before the first inter frame,
/// or if the layer has not been initialised for a frame yet.
///
/// A caller must not hold the result across a call that resolves another handle
/// in the same pool. Every consumer takes what it needs — a stride, a plane root,
/// one array element — and drops the borrow in the same statement.
#[inline]
pub fn layer_ref_pic<'a>(
    pCtx: &'a sWelsEncCtx,
    pLayer: &SDqLayer,
) -> Option<&'a SPicture> {
    // Resolved through the context, on the layer's *own* dependency id rather
    // than the context's current one: under multi-layer SVC the frame loop moves
    // `pCtx.uiDependencyId` on, and the stamped list is the one this layer's
    // readers mean.
    let id = pLayer.pRefPic?;
    let did = pLayer.sLayerInfo.sNalHeaderExt.uiDependencyId as usize;
    Some(pCtx.ref_list(did)?.pic(id))
}

/// [`layer_ref_pic`] for the readers that **do not ask** — the motion-search
/// and mode-decision bodies that run only on an inter macroblock, where a
/// reference picture is bound by construction.
///
/// The `'a` is [`layer_ref_pic`]'s and is spelled out for the same reason: the
/// borrow is the **context's**, not the layer's, and elision would retie it to
/// `pCtx` only by accident of argument order.
///
/// # Panics
/// If the layer has no reference picture bound — before the first inter frame,
/// or on a layer not yet stamped for a frame. The callers that *do* ask keep
/// [`layer_ref_pic`].
///
/// # Safety
/// As [`layer_ref_pic`]: the layer must be stamped for the frame in progress,
/// and the caller must not hold the result across a call that resolves another
/// handle in the same pool.
#[inline]
pub fn layer_ref_pic_expect<'a>(
    pCtx: &'a sWelsEncCtx,
    pLayer: &SDqLayer,
) -> &'a SPicture {
    layer_ref_pic(pCtx, pLayer).expect("the layer's reference picture is bound")
}

/// The reference picture's screen-content feature storage, resolved per call: the
/// pointer lives on `SPicture` and this is the one place it is re-derived. `None`
/// where no reference is bound, or there is no list.
///
/// # Safety
/// The layer must be stamped for the frame in progress.
#[inline]
pub fn layer_ref_feature_storage<'a>(
    pCtx: &'a sWelsEncCtx,
    pLayer: &SDqLayer,
) -> Option<&'a crate::encoder::picture::SScreenBlockFeatureStorage> {
    layer_ref_pic(pCtx, pLayer)?.pScreenBlockFeatureStorage.as_deref()
}

/// **The reconstruction seam's route from a layer** — a shared view whose writes
/// go through `&self`. Two workers may hold it at the same time;
/// [`crate::encoder::rec_view`] carries the argument for why it is sound.
///
/// `None` means no frame has started, or the picture is unbound.
///
/// # Safety
/// `pLayer` must be stamped by `WelsInitCurrentLayer`, and the frame it stamped
/// must still be the frame in progress.
#[inline]
pub fn layer_rec_view<'a>(
    pLayer: &'a SDqLayer,
) -> Option<&'a crate::encoder::rec_view::RecPicView> {
    (*pLayer).pRecView.as_ref()
}

/// [`layer_rec_view`] for the readers that **do not ask** — every consumer
/// inside a frame, where `WelsInitCurrentLayer` has already stamped the view.
///
/// # Panics
/// If no frame has started, or the picture is unbound. The callers that *do* ask
/// keep [`layer_rec_view`].
///
/// # Safety
/// As [`layer_rec_view`]: the layer must be stamped by `WelsInitCurrentLayer`,
/// and the frame it stamped must still be the frame in progress.
#[inline]
pub fn layer_rec_view_expect<'a>(
    pLayer: &'a SDqLayer,
) -> &'a crate::encoder::rec_view::RecPicView {
    layer_rec_view(pLayer).expect("the layer's reconstruction view is built for this frame")
}

/// The layer's **reference** planes as a shared view — the read-only twin of
/// [`layer_enc_view`], built on demand rather than stamped.
///
/// The cost kernels this feeds are reached through a **function pointer**
/// (`PSampleSadSatdCostFunc`), which cannot be generic. Its second operand
/// position receives the enc plane, a scratch buffer **and the reference plane**
/// at different call sites, so all three must be one type.
///
/// Unlike `pEncView`/`pRecView` it is not a layer field, because the reference
/// picture is chosen per macroblock (`pRefPic` moves with the reference index)
/// where the source and reconstruction pictures are stamped once per frame. A
/// build is three plane headers — twelve words, no allocation — against a pool
/// resolution the caller was already paying for.
///
/// # Safety
/// As [`layer_ref_pic`]: the layer must be stamped for the frame in progress.
#[inline]
pub fn layer_ref_view(
    pCtx: &sWelsEncCtx,
    pLayer: &SDqLayer,
) -> Option<crate::encoder::rec_view::RoPicView> {
    Some(crate::encoder::rec_view::RoPicView::build(layer_ref_pic(pCtx, pLayer)?))
}

/// [`layer_ref_view`] for its readers, none of which ask — the view feeds a
/// `PSampleSadSatdCostFunc` slot on a path that has already selected an inter
/// macroblock.
///
/// No `'a`: [`layer_ref_view`] returns a value, not a borrow — `RoPicView` is
/// three plane headers built on the spot.
///
/// # Panics
/// If the layer has no reference picture bound.
///
/// # Safety
/// As [`layer_ref_pic`]: the layer must be stamped for the frame in progress.
#[inline]
pub fn layer_ref_view_expect(
    pCtx: &sWelsEncCtx,
    pLayer: &SDqLayer,
) -> crate::encoder::rec_view::RoPicView {
    layer_ref_view(pCtx, pLayer).expect("the layer's reference view is built for this frame")
}

/// The frame's source planes, as `layer_rec_view` is its reconstruction planes.
///
/// `None` on a layer whose frame has not been bound yet.
#[inline]
pub fn layer_enc_view<'a>(
    pLayer: &'a SDqLayer,
) -> Option<&'a crate::encoder::rec_view::RoPicView> {
    (*pLayer).pEncView.as_ref()
}

/// [`layer_enc_view`] for its readers, none of which ask — every call site is
/// inside a frame, past the bind.
///
/// The `'a` is [`layer_enc_view`]'s: the borrow is the **layer's**.
///
/// # Panics
/// If the layer's frame has not been bound yet.
#[inline]
pub fn layer_enc_view_expect<'a>(
    pLayer: &'a SDqLayer,
) -> &'a crate::encoder::rec_view::RoPicView {
    layer_enc_view(pLayer).expect("the layer's source view is built for this frame")
}

/// Not `repr(C)`: `pRefLayer` is an `Option<LayerIdx>`, which has no C shape.
pub struct SDqLayer {
    /// This layer's own position in `ppDqLayerList`, stamped at construction —
    /// `WelsSwapDqLayers` needs the *outgoing* layer's index and holds only its
    /// pointer.
    pub iDqIdx: LayerIdx,

    pub sLayerInfo: SLayerInfo,
    /// **Boxed, and the box is the point.** The banks live in **their own
    /// allocation**: the layer holds one pointer, which the fork only ever reads,
    /// and every bank write lands in the boxed allocation, which no whole-layer
    /// retag reaches — separate allocations do not share a borrow stack.
    ///
    /// `Box<[T; N]>` rather than `Vec<T>` deliberately: the length is
    /// `MAX_THREADS_NUM` by construction, and keeping it in the type means no site
    /// gains a bounds question it did not have.
    pub sSliceBufferInfo: Box<[SSliceBufferInfo; MAX_THREADS_NUM]>,
    /// One entry per slice in layer order, each naming its bank and its offset
    /// in it. See [`SliceIdx`].
    pub ppSliceInLayer: Vec<SliceIdx>,
    pub sSliceEncCtx: SSliceCtx,
    pub iCsStride: [i32; 3],

    pub iEncStride: [i32; 3],

    /// The layer's macroblock records, **owned**: each layer owns its own cut of
    /// exactly `iMbWidth * iMbHeight` records.
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

    /// `SDqLayer::pFeatureSearchPreparation` — `svc_enc_frame.h:126`.
    /// `Some` on the last DQ layer under `SCREEN_CONTENT_REAL_TIME`
    /// (`encoder_ext.cpp:1125-1135`), `None` otherwise; `Drop` is `FreeDqLayer`'s
    /// release (`:973-977`). Written only outside the fork (`PreprocessSliceCoding`
    /// and the post-join FME switch); the workers read it.
    pub pFeatureSearchPreparation:
        Option<Box<crate::encoder::svc_motion_estimate::SFeatureSearchPreparation>>,
    pub pRefPic: Option<RecPicId>,
    pub pDecPic: Option<RecPicId>,
    /// The **source** picture this frame encodes from, as a slot of the spatial
    /// pool, which lives in `pCtx->pVpp` and is otherwise unreachable from a
    /// layer. Stamped by `WelsInitCurrentLayer`.
    pub pEncPic: Option<SrcPicId>,
    /// **The reconstruction seam**, built per frame by `WelsInitCurrentLayer`.
    ///
    /// This is the layer's route to the picture *every worker writes*: three
    /// planes and four per-macroblock side arrays, shared and writable through
    /// `&self`. It sits here rather than on the job because every consumer
    /// already reaches the layer and `SDqLayer` cannot carry a lifetime, so the
    /// view holds captured parts instead — see
    /// [`crate::encoder::rec_view`] for the soundness argument, which this
    /// field is one half of.
    ///
    /// **The stability requirement, in one sentence**: while this is `Some`,
    /// nothing may take `&mut` to the same picture through the pool —
    /// `pic_mut(idDec)` — because that retag makes the captured bases stale and,
    /// under the fork, races on `SRefList` itself. `None` between frames is not
    /// decoration: `WelsInitCurrentLayer` rebuilds it every frame, and nothing
    /// may read a view built for a frame that has ended.
    pub pRecView: Option<crate::encoder::rec_view::RecPicView>,

    /// The frame's **source** planes, as a read-only view — the counterpart to
    /// `pRecView` and the read half of the same seam.
    ///
    /// `PlaneCursor`s taken from it bounds-check against the whole allocation, so
    /// the top and left borders a motion search legally reaches stay reads rather
    /// than becoming fresh panics.
    ///
    /// Rebuilt every frame with `pRecView`, and for the same reason: the pool may
    /// hand the next frame a different slot, so a view is only valid for the frame
    /// that built it.
    pub pEncView: Option<crate::encoder::rec_view::RoPicView>,
    /// The *source* pictures behind the reference list — slots of the preprocessor's
    /// spatial pool, resolved through `pCtx->pVpp` (both readers hold the context).
    pub pRefOri: [Option<PicRef>; MAX_REF_PIC_COUNT as usize],

    pub bThreadSlcBufferFlag: bool,
    pub bSliceBsBufferFlag: bool,
    pub iMaxSliceNum: i32,
    /// Atomic because both arrays live *inline* in the layer and are written
    /// **from inside the encode**: `WelsISliceMdEncDynamic` and
    /// `WelsMdInterMbLoopOverDynamicSlice` stamp `[kiPartitionId]` at six sites
    /// between them, one worker per partition, while sibling bodies hold
    /// `&SDqLayer`.
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
    /// position — **owned**, and grown by `ExtendLayerBuffer`'s `resize`.
    pub pFirstMbIdxOfSlice: Vec<i32>,
    pub pCountMbNumInSlice: Vec<i32>,

    pub bNeedAdjustingSlicing: bool,

    /// The base layer this one predicts from, as a position in `ppDqLayerList`
    /// rather than as an address. `None` when there is no base layer, and it is
    /// **written**, never inherited from a zero image: `Option<LayerIdx>` has no
    /// niche to borrow, so all-zero is not a defined `None`.
    pub pRefLayer: Option<LayerIdx>,
}

impl SDqLayer {
    pub fn new(idx: LayerIdx) -> Self {
        Self {
            // Its own position.
            iDqIdx: idx,
            // `InitDqLayers` fills the whole of this from the parameter sets before
            // the first frame.
            sLayerInfo: SLayerInfo::default(),
            // No bank allocated yet — `InitSliceThreadInfo` fills bank 0 (and, under
            // MT, one per thread) two calls later.
            sSliceBufferInfo: Box::new(std::array::from_fn(|_| SSliceBufferInfo::default())),
            // The slice position array, sized by `InitSliceInLayer` and regrown by
            // `ExtendLayerBuffer`.
            ppSliceInLayer: Vec::new(),
            // Zero here means "no slice segmentation yet"; `InitSlicePEncCtx` sets
            // the mode, the geometry and the map.
            sSliceEncCtx: SSliceCtx::default(),
            // Plane aliases into the reconstructed and source pictures, re-aimed at
            // every frame by `WelsInitCurrentLayer`; null means "no frame started".
            iCsStride: [0; 3],
            // The seam, rebuilt per frame; `None` is "no frame started".
            pRecView: None,
            pEncView: None,
            iEncStride: [0; 3],
            // The macroblock records, sized by `InitMbListD` once the geometry is
            // known.
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
            // Picture slots, aimed per frame; `None` is "no picture bound".
            pFeatureSearchPreparation: None,
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
            // The two per-slice-index arrays, sized by `InitSliceInLayer`.
            pFirstMbIdxOfSlice: Vec::new(),
            pCountMbNumInSlice: Vec::new(),
            // "The slicing does not need re-deriving"; `NeedDynamicAdjust` sets it.
            bNeedAdjustingSlicing: false,
            // No base layer until `WelsSwapDqLayers` names one.
            pRefLayer: None,
        }
    }
}

impl Default for SDqLayer {
    /// The layer at index 0, which is what the two test fixtures that call this
    /// want (`slice_multi_threading.rs`, `wels_task_management.rs`): both build a
    /// single-layer context.
    fn default() -> Self {
        Self::new(LayerIdx(0))
    }
}

pub use crate::encoder::nal_encap::SWelsNalRaw;

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
//
// Both slot types carry the bitstream pair. `pSliceBsBuf` is the buffer the
// slice's writer is positioned in, derived once at the chain's top (the fork job
// or the inline dispatch). `pCtxOutBs` is the frame-output writer for the slice
// that shares `pOut` (`sSliceBs.pBs == None`); it is `None` on the fork side,
// where that arm is main-thread-only. The slice-owned writer arm needs no
// parameter: every body that needs it holds `&mut SSlice` and resolves
// `sSliceBs.sBsWrite` field-precisely at the use (`slice_bs_writer`).
pub type PWelsCodingSliceFunc = extern "C" fn(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    // The macroblock records this slice may write, carried like the bitstream
    // pair above: on the fork side from the pre-fork carve over the slice
    // ranges, on the single-threaded side straight off a `&mut MbArray`. The
    // bodies move the cursor with `set_cur`.
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    // The CABAC restore scratch, threaded like the pair above: the size-limited
    // fork takes the partition's buffer beside its bitstream slot, the
    // single-threaded callers take it around the call. `None` for CAVLC dynamic
    // slicing, and every fixed mode.
    pRestoreBuf: Option<&mut [u8]>,
    // The slot the dynamic boundary writes forward into — the bank record right
    // after the current slice. `AddSliceBoundary` copies the current header here
    // when the size limit fires; `None` on every fixed path (the boundary never
    // fires) and where the next slot does not exist.
    pNextSlice: Option<&mut SSlice>,
) -> i32;
pub type PWelsSliceHeaderWriteFunc = extern "C" fn(
    pCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
);

/// The writer a slice's bits go through.
///
/// The discriminator is per-use: **`sSliceBs.pBs`'s nullness**, the one bit
/// `InitSliceBsBuffer` records when it decides whether the slice writes an
/// independent output buffer — it allocates `pBs` exactly when it aims the slice
/// at its own `sBsWrite`, leaves it `None` exactly when the slice shares
/// `pOut->sBsWrite`, and the bit travels with the struct through
/// `ReallocateSliceList`. Deriving the choice back from `iMultipleThreadIdc`
/// and `uiSliceMode` would re-read parameters that can move between allocation
/// and use; the allocation cannot.
///
/// The slice-owned arm is a field projection of the `&mut SWelsSliceBs` the
/// caller already holds; the shared arm reborrows the frame-output writer
/// threaded from the chain's top.
///
/// The `expect` arm: the `pOut` writer is main-thread-only, so a fork body asking
/// for it — `pBs == None` with no threaded writer — is a state the C++ cannot
/// reach either, and it panics rather than handing a cross-thread writer out.
#[inline]
pub fn slice_bs_writer<'a>(
    sSliceBs: &'a mut SWelsSliceBs,
    pCtxOutBs: &'a mut Option<&mut BsWriter>,
) -> &'a mut BsWriter {
    if sSliceBs.pBs.is_some() {
        &mut sSliceBs.sBsWrite
    } else {
        pCtxOutBs
            .as_deref_mut()
            .expect("F217: a slice sharing pOut's writer is main-thread-only, and the inline dispatch threads that writer")
    }
}

/// The shared-read twin of [`slice_bs_writer`], for the rate controller's
/// position reads (`GetBsPosition`).
#[inline]
pub fn slice_bs_writer_ref<'a>(
    sSliceBs: &'a SWelsSliceBs,
    pCtxOutBs: Option<&'a BsWriter>,
) -> &'a BsWriter {
    if sSliceBs.pBs.is_some() {
        &sSliceBs.sBsWrite
    } else {
        pCtxOutBs.expect("F217: a slice sharing pOut's writer is main-thread-only, and the inline dispatch threads that writer")
    }
}

// ============================================================================
// Bitstream Helper Functions
// ============================================================================

// One writer family, `vlc_encoder.rs`'s, which is the transliteration of the C++
// `codec/common/inc/golomb_common.h`.
pub use crate::encoder::vlc_encoder::{
    BsGetBitsPos, BsWriteBits, BsWriteOneBit, BsWriteSE, BsWriteUE,
};

// ============================================================================
// Macroblock Topology & Cache Operations
// ============================================================================

/// Copies non-zero coefficient counts from `SMB` into the slice's `SMbCache`.
#[inline]
pub fn UpdateNonZeroCountCache(pMb: &SMB, pMbCache: &mut SMbCache) {
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
///
/// Writes through the window its caller owns; `MbWindow::at*` take raster
/// addresses, so a partition-wide window indexes as a per-slice one does.
pub fn UpdateMbNeighbourInfoForNextSlice(
    pSliceCtx: &crate::encoder::slice_multi_threading::SSliceCtx,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    kiFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
) {
    let kiMbWidth = pSliceCtx.iMbWidth as i32;
    let mut iIdx = kiFirstMbIdxOfNextSlice;
    let iNextSliceFirstMbIdxRowStart = if (kiFirstMbIdxOfNextSlice % kiMbWidth) != 0 { 1 } else { 0 };
    let iCountMbUpdate = kiMbWidth + iNextSliceFirstMbIdxRowStart;
    let kiEndMbNeedUpdate = kiFirstMbIdxOfNextSlice + iCountMbUpdate;

    // C++ is a do-while: the first macroblock is always updated, even when
    // `kiFirstMbIdxOfNextSlice > kiLastMbIdxInPartition` -- which happens when the
    // boundary lands on the last macroblock of a partition. A `while` skips it.
    // The window is sized to exactly the records this walk touches — the next
    // slice's first row-and-a-bit, bounded by the caller's own partition.
    loop {
        let kiSliceIdc = WelsMbToSliceIdc(Some(pSliceCtx), pMbs.at(iIdx as usize).iMbXY);
        UpdateMbNeighbor(
            Some(pSliceCtx), pMbs.at_mut(iIdx as usize), kiMbWidth, kiSliceIdc);
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

pub fn WelsSliceHeaderExtInit(pEncCtx: &sWelsEncCtx, pCurLayer: Option<&SDqLayer>, pSlice: &mut SSlice) {
    let Some(pCurLayer) = pCurLayer else {
        return;
    };
    let pCurSliceExt = &mut (*pSlice).sSliceHeaderExt;
    let pCurSliceHeader = &mut pCurSliceExt.sSliceHeader;
    let uiDid = (*pEncCtx).uiDependencyId as usize;

    pCurSliceHeader.eSliceType = (*pEncCtx).eSliceType;
    pCurSliceExt.bStoreRefBasePicFlag = false;

    // svc_encode_slice.cpp:97-98.
    let pParamInternal = &(*pEncCtx).param().sDependencyLayers[uiDid];
    pCurSliceHeader.iFrameNum = pParamInternal.iFrameNum;
    pCurSliceHeader.uiIdrPicId = pParamInternal.uiIdrPicId;

    if let Some(id) = (*pEncCtx).pEncPic {
        pCurSliceHeader.iPicOrderCntLsb =
            crate::encoder::encoder_context::ctx_vpp_ref(pEncCtx).src_id(id).iFramePoc;
    }

    if (*pEncCtx).eSliceType == EWelsSliceType::P_SLICE {
        pCurSliceHeader.uiNumRefIdxL0Active = 1;
        let num_ref = if let Some(sps) = layer_sps_ref(pEncCtx, pCurLayer) {
            sps.iNumRefFrames
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

    let pic_init_qp = layer_pps_ref(pEncCtx, pCurLayer).map_or(26, |p| p.iPicInitQp);
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

pub fn WriteReferenceReorder(buf: &mut [u8], pBs: &mut BsWriter, sSliceHeader: &mut SSliceHeader) {
    let pRefOrdering = &mut sSliceHeader.sRefReordering;
    let eSliceType = sSliceHeader.eSliceType;

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

// `pNalHdrExt` is layer state read by every worker, so it is a shared reference.
pub fn WriteRefPicMarking(buf: &mut [u8], pBs: &mut BsWriter, pSliceHeader: &mut SSliceHeader, pNalHdrExt: &SNalUnitHeaderExt) {
    let sRefMarking = &mut pSliceHeader.sRefMarking;
    let mut n: usize = 0;

    if pNalHdrExt.bIdrFlag {
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

pub fn WelsSliceHeaderWrite(
    pCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    let pBs = slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs);
    let buf = pSliceBsBuf;
    let pSps = layer_sps_ref(pCtx, pCurLayer);
    let pPps = layer_pps_ref(pCtx, pCurLayer);
    let pSliceHeader = &mut (*pSlice).sSliceHeaderExt.sSliceHeader;
    let pNalHead = &pCurLayer.sLayerInfo.sNalHeaderExt;

    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy.
    let pps_id = pPps.map_or(0, |p| p.iPpsId);
    let iPpsIdOffset = pParametersetStrategy.map_or(0, |s| s.GetPpsIdOffset(pps_id as i32));
    BsWriteUE(buf, &mut *pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = pSps.map_or(4, |s| s.uiLog2MaxFrameNum);
    BsWriteBits(buf, &mut *pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if pNalHead.bIdrFlag {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if let Some(sps) = pSps {
        if sps.uiPocType == 0 {
            BsWriteBits(buf, &mut *pBs, sps.iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
        }
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(buf, &mut *pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(buf, &mut *pBs, active);
        }
    }

    if !pNalHead.bIdrFlag {
        WriteReferenceReorder(buf, &mut *pBs, pSliceHeader);
    }

    if pNalHead.sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, &mut *pBs, pSliceHeader, pNalHead);
    }

    if pPps.is_some_and(|p| p.bEntropyCodingModeFlag) && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(buf, &mut *pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(buf, &mut *pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if pPps.is_some_and(|p| p.bDeblockingFilterControlPresentFlag) {
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

pub fn WelsSliceHeaderExtWrite(
    pCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    let pBs = slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs);
    let buf = pSliceBsBuf;
    let pSps = layer_sps_ref(pCtx, pCurLayer);
    let pPps = layer_pps_ref(pCtx, pCurLayer);
    let pSubSps = layer_subset_sps_ref(pCtx, pCurLayer);
    let pSliceHeadExt = &mut (*pSlice).sSliceHeaderExt;
    let pSliceHeader = &mut pSliceHeadExt.sSliceHeader;
    let pNalHead = &pCurLayer.sLayerInfo.sNalHeaderExt;

    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).iFirstMbInSlice as u32);
    BsWriteUE(buf, &mut *pBs, (*pSliceHeader).eSliceType as u32);

    // svc_encode_slice.cpp:285 / :361 — `pPps->iPpsId + pParametersetStrategy->
    // GetPpsIdOffset (pPps->iPpsId)`. The offset is 0 under CONSTANT_ID but not under
    // INCREASING_ID, which is the FillDefault strategy.
    let pps_id = pPps.map_or(0, |p| p.iPpsId);
    let iPpsIdOffset = pParametersetStrategy.map_or(0, |s| s.GetPpsIdOffset(pps_id as i32));
    BsWriteUE(buf, &mut *pBs, pps_id.wrapping_add(iPpsIdOffset as u32));

    let log2_max_frame_num = pSps.map_or(4, |s| s.uiLog2MaxFrameNum);
    BsWriteBits(buf, &mut *pBs, log2_max_frame_num as i32, (*pSliceHeader).iFrameNum as u32);

    if pNalHead.bIdrFlag {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiIdrPicId as u32);
    }

    if let Some(sps) = pSps {
        if sps.uiPocType == 0 {
            BsWriteBits(buf, &mut *pBs, sps.iLog2MaxPocLsb, (*pSliceHeader).iPicOrderCntLsb as u32);
        }
    }

    if (*pSliceHeader).eSliceType == EWelsSliceType::P_SLICE {
        BsWriteOneBit(buf, &mut *pBs, if (*pSliceHeader).bNumRefIdxActiveOverrideFlag { 1 } else { 0 });
        if (*pSliceHeader).bNumRefIdxActiveOverrideFlag {
            let active = WELS_CLIP3((*pSliceHeader).uiNumRefIdxL0Active.saturating_sub(1) as u32, 0, MAX_REF_PIC_COUNT);
            BsWriteUE(buf, &mut *pBs, active);
        }
    }

    if !pNalHead.bIdrFlag {
        WriteReferenceReorder(buf, &mut *pBs, pSliceHeader);
    }

    if pNalHead.sNalUnitHeader.uiNalRefIdc != 0 {
        WriteRefPicMarking(buf, &mut *pBs, pSliceHeader, pNalHead);
        if pSubSps.is_some_and(|s| !s.sSpsSvcExt.bSliceHeaderRestrictionFlag) {
            BsWriteOneBit(buf, &mut *pBs, if pSliceHeadExt.bStoreRefBasePicFlag { 1 } else { 0 });
        }
    }

    if pPps.is_some_and(|p| p.bEntropyCodingModeFlag) && (*pSliceHeader).eSliceType != EWelsSliceType::I_SLICE {
        BsWriteUE(buf, &mut *pBs, (*pSlice).iCabacInitIdc as u32);
    }

    BsWriteSE(buf, &mut *pBs, (*pSliceHeader).iSliceQpDelta as i32);

    if pPps.is_some_and(|p| p.bDeblockingFilterControlPresentFlag) {
        BsWriteUE(buf, &mut *pBs, (*pSliceHeader).uiDisableDeblockingFilterIdc as u32);
        if (*pSliceHeader).uiDisableDeblockingFilterIdc != 1 {
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceAlphaC0Offset as i32) >> 1);
            BsWriteSE(buf, &mut *pBs, ((*pSliceHeader).iSliceBetaOffset as i32) >> 1);
        }
    }

    if pSubSps.is_some_and(|s| !s.sSpsSvcExt.bSliceHeaderRestrictionFlag) {
        BsWriteBits(buf, &mut *pBs, 4, 0);
        BsWriteBits(buf, &mut *pBs, 4, 15);
    }
}

// ============================================================================
// Macroblock Residual & Chroma Reconstruction
// ============================================================================

// `WelsInterMbEncode` lives in `svc_mode_decision.rs`, which is where the C++
// has it (svc_mode_decision.cpp) and where all three call sites resolve.

pub fn WelsIMbChromaEncode(pEncCtx: &sWelsEncCtx, pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    let pCurLayer = current_layer_expect(pEncCtx);
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let kiBestPredOff =
        best_pred_intra_chroma_off((*pMbCache).uiMemPredLumaHalf, (*pMbCache).uiBestPredIntraChromaHalf);
    let view_chroma = layer_rec_view_expect(&*pCurLayer);
    let (kiChrOrgX, kiChrOrgY) = (*pMbCache).SPicData.chroma_origin();

    let encView = layer_enc_view_expect(&*pCurLayer);
    let pFunc = (*pEncCtx).func_list();
    let pfDctFourT4 = (*pFunc).pfDctFourT4;

    //cb
    pfDctFourT4(
        &mut (*pMbCache).sCoeffLevel,
        &(*pMbCache).SPicData.mb_cursor_ro(encView, 1),
        &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, kiBestPredOff, 8),
    );
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, pMbCache, 0, 1);
    // The prediction is `sMemPredMb`'s intra-chroma half at stride 8, an owned
    // arena. Slot bypassed: `pfIDctFourT4` is constant after init.
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

pub fn WelsPMbChromaEncode(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, pCurMb: &mut SMB) {
    let pCurLayer = current_layer_expect(pEncCtx);
    let kiEncStride = (*pCurLayer).iEncStride[1];
    let pMbCache = &mut pSlice.sMbCacheInfo;
    // Note the base: this one starts at `pCoeffLevel + 256`
    // (`svc_encode_slice.cpp:499`) where the intra path starts at 0, which is why
    // `WelsEncRecUV` takes the offset as a parameter rather than deriving it from
    // `iUV`.
    let kiBestPredOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);

    let encView = layer_enc_view_expect(&*pCurLayer);
    let pFunc = (*pEncCtx).func_list();
    let dct = (*pFunc).pfDctFourT4;
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
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 256, 1);
    crate::encoder::svc_encode_mb::WelsEncRecUV(&*pFunc, pCurMb, &mut *pMbCache, 320, 2);
}

pub fn OutputPMbWithoutConstructCsRsNoCopy(pCtx: &sWelsEncCtx, pDq: Option<&SDqLayer>, pSlice: &mut SSlice, pMb: &SMB) {
    let Some(pDq) = pDq else {
        return;
    };
    let mb_type = (*pMb).uiMbType;
    //intra have been reconstructed, NO COPY from CS to pDecPic--
    if (IS_INTER(mb_type) && !IS_SKIP(mb_type)) || IS_I_BL(mb_type) {
        let pMbCache = &mut (*pSlice).sMbCacheInfo;
        // The in-place family: `pRec` *is* `pPred` at all three of these sites.
        // One seam cursor per plane, read and written by value. The view carries
        // the strides — `WelsInitCurrentLayer` stamps `iCsStride[i]` and the
        // view's plane stride from one `SPicture::stride(i)`.
        let view = layer_rec_view_expect(pDq);
        let (lx, ly) = (*pMbCache).SPicData.luma_origin();
        let (cx, cy) = (*pMbCache).SPicData.chroma_origin();

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

pub fn WelsGetNextMbOfSlice(pSliceSeg: &crate::encoder::slice_multi_threading::SSliceCtx, kiMbXY: i32) -> i32 {
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
                // Equality holds only when *both* lookups are `Some`.
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
    // The row bump is `offset` rather than `add` because the table it arrives
    // parked in is already biased to the zero-MVD entry
    // (`MvdCostCursor::origin`'s job).
    if !pMvdCostTable.is_none() {
        (*pMd).pMvdCost = pMvdCostTable.offset(luma_qp as i32 * kiMvdInterTableStride);
    }
    (*pMd).iMbPixX = (pCurMb.iMbX as i32) << 4;
    (*pMd).iMbPixY = (pCurMb.iMbY as i32) << 4;
    (*pMd).iBlock8x8StaticIdc.fill(0);
}

pub fn WelsISliceMdEnc(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let Some(pCurLayer) = current_layer_ref(pEncCtx) else {
        return ENC_RETURN_SUCCESS;
    };
    // The grid-emptiness arm reads the *window*: under the carve (and the
    // single-threaded take-and-restore) the layer's `sMbDataP` slot is
    // legitimately empty while this runs, and reading it here would silently
    // skip the slice.
    if pMbs.stride() == 0 || pCurLayer.iMbWidth <= 0 || pCurLayer.iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let kiSliceFirstMbXY = pSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset =
        layer_pps_ref(pEncCtx, &*pCurLayer).map_or(0, |p| p.uiChromaQpIndexOffset);

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*pEncCtx).param().iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice, &mut *pSliceBsBuf, &mut *pCtxOutBs);
        sDss.pRestoreBuffer = None;
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }

    loop {
        if !kbCabac {
            {
                let func_list = (*pEncCtx).func_list();
                func_list
                    .eEntropyCoder
                    .StashMBStatus(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), &mut sDss, &mut (*pSlice).sCabacCtx, (*pSlice).uiLastMbQp, 0);
            }
        }
        iCurMbIdx = iNextMbIdx;
        pMbs.set_cur(iCurMbIdx as usize);

        {
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, pMbs.cur_mut(), &mut *pSlice, pCtxOutBs.as_deref());
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut *pMbs,
            &mut pSlice.sMbCacheInfo,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            let pMbCache = &mut pSlice.sMbCacheInfo;
            sMd.iLambda = g_kiQpCostTable[pMbs.cur().uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, pMbs.cur_mut(), &mut *pMbCache);
            UpdateNonZeroCountCache(pMbs.cur(), &mut *pMbCache);

            let mut iEncReturn;
            {
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut *pMbs, &mut *pSliceBsBuf, &mut *pCtxOutBs);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && pMbs.cur().uiLumaQp < 50 {
                {
                    let func_list = (*pEncCtx).func_list();
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), &mut sDss, &mut (*pSlice).sCabacCtx);
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(pMbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        pMbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;

        let pMbCache = &mut pSlice.sMbCacheInfo;
        {
            let func_list = (*pEncCtx).func_list();
            (func_list.pfMdBackgroundInfoUpdate)(
                pEncCtx,
                &*pCurLayer,
                pMbs.cur_mut(),
                pMbCache.bCollocatedPredFlag,
                I_SLICE,
            );
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                pMbs.cur_mut(),
                sMd.iCostLuma,
                &mut *pSlice,
                pCtxOutBs.as_deref(),
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(&(*pCurLayer).sSliceEncCtx, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

pub fn WelsISliceMdEncDynamic(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    mut pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);
    let kiSliceFirstMbXY = pSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let kiTotalNumMb: i32 = pCurLayer.iMbWidth as i32 * pCurLayer.iMbHeight as i32;
    let mut iCurMbIdx: i32;
    let mut iNumMbCoded = 0;
    let kiSliceIdx = pSlice.iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset =
        layer_pps_ref(pEncCtx, pCurLayer).map_or(0, |p| p.uiChromaQpIndexOffset);

    let mut sMd = SWelsMD::default();
    let mut sDss = SDynamicSlicingStack::default();
    if (*pEncCtx).param().iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice, &mut *pSliceBsBuf, &mut *pCtxOutBs);
        sDss.pRestoreBuffer = pRestoreBuf;
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    } else {
        sDss.iStartPos = slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs).bits_pos();
    }

    loop {
        iCurMbIdx = iNextMbIdx;
        pMbs.set_cur(iCurMbIdx as usize);

        {
            let func_list = (*pEncCtx).func_list();
            func_list
                .eEntropyCoder
                .StashMBStatus(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), &mut sDss, &mut pSlice.sCabacCtx, pSlice.uiLastMbQp, 0);
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, pMbs.cur_mut(), &mut *pSlice, pCtxOutBs.as_deref());
        }

        if pSlice.bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*pEncCtx).rc_at((*pEncCtx).uiDependencyId as usize).iMaxQp;
            pMbs.cur_mut().uiLumaQp = max_qp as u8;
            pMbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut *pMbs,
            &mut pSlice.sMbCacheInfo,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            let pMbCache = &mut pSlice.sMbCacheInfo;
            sMd.iLambda = g_kiQpCostTable[pMbs.cur().uiLumaQp as usize];
            crate::encoder::svc_base_layer_md::WelsMdIntraMb(pEncCtx, &mut sMd, pMbs.cur_mut(), &mut *pMbCache);
            UpdateNonZeroCountCache(pMbs.cur(), &mut *pMbCache);

            let mut iEncReturn;
            {
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut *pMbs, &mut *pSliceBsBuf, &mut *pCtxOutBs);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && pMbs.cur().uiLumaQp < 50 {
                {
                    let func_list = (*pEncCtx).func_list();
                    func_list
                        .eEntropyCoder
                        .StashPopMBStatus(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), &mut sDss, &mut pSlice.sCabacCtx);
                    pSlice.uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(pMbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        {
            let func_list = (*pEncCtx).func_list();
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(slice_bs_writer_ref(&pSlice.sSliceBs, pCtxOutBs.as_deref()), &pSlice.sCabacCtx);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            &pCurLayer.sSliceEncCtx,
            pMbs.cur().iMbXY,
            &mut sDss,
            pMbs,
            // Reborrowed, not taken: the judge runs per macroblock and fires
            // on one of them — a `take` here would consume the slot on the
            // first (non-firing) call and hand the real boundary `None`.
            pNextSlice.as_deref_mut(),
        ) {
            {
                let func_list = (*pEncCtx).func_list();
                func_list
                    .eEntropyCoder
                    .StashPopMBStatus(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), &mut sDss, &mut pSlice.sCabacCtx);
                pSlice.uiLastMbQp = sDss.uiLastMbQp;
            }
            pCurLayer.LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx - 1, Ordering::Relaxed);
            pCurLayer.NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }

        pMbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;

        {
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                pMbs.cur_mut(),
                sMd.iCostLuma,
                &mut *pSlice,
                pCtxOutBs.as_deref(),
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(&pCurLayer.sSliceEncCtx, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            pSlice.iCountMbNumInSlice = iCurMbIdx - pCurLayer.LastCodedMbIdxOfPartition[kiPartitionId].load(Ordering::Relaxed);
            pCurLayer.LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx, Ordering::Relaxed);
            pCurLayer.NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }
    }

    ENC_RETURN_SUCCESS
}

/// Debug hook matching the `OH264_MBDUMP` block the C++ carries at the same point in
/// `WelsMdInterMbLoop`. Prints the per-macroblock mode-decision state so the two
/// encoders can be diffed line by line. Off unless `OH264_MBDUMP` is set.
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

pub fn WelsMdInterMbLoop<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pSlice: &mut SSlice,
    pWelsMd: &mut SWelsMD<'a>,
    kiSliceFirstMbXY: i32,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    if current_layer_ref(pEncCtx).is_none() || pMbs.stride() == 0 || current_layer_expect(pEncCtx).iMbWidth <= 0 || current_layer_expect(pEncCtx).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pCurLayer = current_layer_expect(pEncCtx);
    let mut iNumMbCoded = 0;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiTotalNumMb: i32 = (*pCurLayer).iMbWidth as i32 * (*pCurLayer).iMbHeight as i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = MvdCostCursor::origin(
        &(&(*pEncCtx).pMvdCostTable)[..],
        (*pEncCtx).iMvdCostTableSize,
    );
    let kiSliceIdx = (*pSlice).iSliceIdx;
    let kuiChromaQpIndexOffset =
        layer_pps_ref(pEncCtx, &*pCurLayer).map_or(0, |p| p.uiChromaQpIndexOffset);

    let mut sDss = SDynamicSlicingStack::default();

    let kbCabac = (*pEncCtx).param().iEntropyCodingModeFlag != 0;
    if kbCabac {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice, &mut *pSliceBsBuf, &mut *pCtxOutBs);
        sDss.pRestoreBuffer = None;
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
    }
    (*pSlice).iMbSkipRun = 0;

    loop {
        if !kbCabac {
            {
                let func_list = (*pEncCtx).func_list();
                func_list.eEntropyCoder.StashMBStatus(
                    &mut *pSliceBsBuf,
                    slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs),
                    &mut sDss,
                    &mut (*pSlice).sCabacCtx,
                    (*pSlice).uiLastMbQp,
                    (*pSlice).iMbSkipRun,
                );
            }
        }
        iCurMbIdx = iNextMbIdx;
        pMbs.set_cur(iCurMbIdx as usize);

        //step(1): set QP for the current MB
        {
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, pMbs.cur_mut(), &mut *pSlice, pCtxOutBs.as_deref());
        }

        //step (2). save some value for future use, initial pWelsMd
        let pMbCache = &mut pSlice.sMbCacheInfo;
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut *pMbs,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            &mut *pMbs,
            kiSliceFirstMbXY,
        );

        loop {
            WelsInitInterMDStruc(pMbs.cur(), pMvdCostTable, kiMvdInterTableStride, pMd);
            {
                let func_list = (*pEncCtx).func_list();
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, &mut *pSlice, &mut *pMbs);
                }
                let pMbCache = &mut pSlice.sMbCacheInfo;

                //step (4): save from the MD process for future use
                {
                    crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                        layer_rec_view_expect(&*pCurLayer),
                        pMbs.cur(),
                        pMd,
                    );
                }

                (func_list.pfMdBackgroundInfoUpdate)(
                    pEncCtx,
                    &*pCurLayer,
                    pMbs.cur_mut(),
                    (*pMbCache).bCollocatedPredFlag,
                    ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                );
                mb_dump(pMbs.cur(), pMd, pSlice);
            }
            //step (5): update cache
            let pMbCache = &mut pSlice.sMbCacheInfo;
            UpdateNonZeroCountCache(pMbs.cur(), &mut *pMbCache);

            let mut iEncReturn;
            {
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut *pMbs, &mut *pSliceBsBuf, &mut *pCtxOutBs);
            }

            if !kbCabac && iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && pMbs.cur().uiLumaQp < 50 {
                {
                    let func_list = (*pEncCtx).func_list();
                    (*pSlice).iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        &mut *pSliceBsBuf,
                        slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs),
                        &mut sDss,
                        &mut (*pSlice).sCabacCtx,
                    );
                    (*pSlice).uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(pMbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        pMbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, Some(pCurLayer), pSlice, pMbs.cur());

        {
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                pMbs.cur_mut(),
                (*pMd).iCostLuma,
                &mut *pSlice,
                pCtxOutBs.as_deref(),
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(&pCurLayer.sSliceEncCtx, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            break;
        }
    }

    if (*pSlice).iMbSkipRun > 0 {
        let kiMbSkipRun = (*pSlice).iMbSkipRun as u32;
        BsWriteUE(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), kiMbSkipRun);
    }

    ENC_RETURN_SUCCESS
}

pub fn WelsMdInterMbLoopOverDynamicSlice<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pSlice: &mut SSlice,
    pWelsMd: &mut SWelsMD<'a>,
    kiSliceFirstMbXY: i32,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    mut pNextSlice: Option<&mut SSlice>,
) -> i32 {
    if current_layer_ref(pEncCtx).is_none() || pMbs.stride() == 0 || current_layer_expect(pEncCtx).iMbWidth <= 0 || current_layer_expect(pEncCtx).iMbHeight <= 0 {
        return ENC_RETURN_SUCCESS;
    }
    let pMd = pWelsMd;
    let pCurLayer = current_layer_expect(pEncCtx);
    let mut iNumMbCoded = 0;
    let kiTotalNumMb: i32 = pCurLayer.iMbWidth as i32 * pCurLayer.iMbHeight as i32;
    let mut iNextMbIdx = kiSliceFirstMbXY;
    let mut iCurMbIdx: i32;
    let kiMvdInterTableStride = (*pEncCtx).iMvdCostTableStride;
    let pMvdCostTable = MvdCostCursor::origin(
        &(&(*pEncCtx).pMvdCostTable)[..],
        (*pEncCtx).iMvdCostTableSize,
    );
    let kiSliceIdx = pSlice.iSliceIdx;
    let kiPartitionId = (kiSliceIdx % ((*pEncCtx).iActiveThreadsNum as i32)) as usize;
    let kuiChromaQpIndexOffset =
        layer_pps_ref(pEncCtx, pCurLayer).map_or(0, |p| p.uiChromaQpIndexOffset);

    let mut sDss = SDynamicSlicingStack::default();
    if (*pEncCtx).param().iEntropyCodingModeFlag != 0 {
        crate::encoder::svc_set_mb_syn_cabac::WelsInitSliceCabac(pEncCtx, pSlice, &mut *pSliceBsBuf, &mut *pCtxOutBs);
        sDss.iStartPos = 0;
        sDss.iCurrentPos = 0;
        sDss.pRestoreBuffer = pRestoreBuf;
    } else {
        sDss.iStartPos = slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs).bits_pos();
    }
    pSlice.iMbSkipRun = 0;

    loop {
        {
            let func_list = (*pEncCtx).func_list();
            func_list.eEntropyCoder.StashMBStatus(
                &mut *pSliceBsBuf,
                slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs),
                &mut sDss,
                &mut pSlice.sCabacCtx,
                pSlice.uiLastMbQp,
                pSlice.iMbSkipRun,
            );
        }
        iCurMbIdx = iNextMbIdx;
        pMbs.set_cur(iCurMbIdx as usize);

        {
            let func_list = (*pEncCtx).func_list();
            func_list
                .pfRc
                .WelsRcMbInit(pEncCtx, pMbs.cur_mut(), &mut *pSlice, pCtxOutBs.as_deref());
        }

        if pSlice.bDynamicSlicingSliceSizeCtrlFlag {
            let max_qp = (*pEncCtx).rc_at((*pEncCtx).uiDependencyId as usize).iMaxQp;
            pMbs.cur_mut().uiLumaQp = max_qp as u8;
            pMbs.cur_mut().uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(max_qp as i32 + kuiChromaQpIndexOffset as i32)];
        }

        // step (2): save some values for future use, initialise pWelsMd.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        crate::encoder::svc_base_layer_md::WelsMdIntraInit(
            pEncCtx,
            &mut *pMbs,
            &mut *pMbCache,
            kiSliceFirstMbXY,
        );
        crate::encoder::svc_base_layer_md::WelsMdInterInit(
            pEncCtx,
            pSlice,
            &mut *pMbs,
            kiSliceFirstMbXY,
        );

        // TRY_REENCODING
        loop {
            WelsInitInterMDStruc(pMbs.cur(), pMvdCostTable, kiMvdInterTableStride, pMd);
            {
                let func_list = (*pEncCtx).func_list();
                if let Some(func) = func_list.pfInterMd {
                    func(pEncCtx, pMd, &mut *pSlice, &mut *pMbs);
                }
            }
            let pMbCache = &mut pSlice.sMbCacheInfo;
            // step (4): save from the MD process for future use
            {
                crate::encoder::svc_base_layer_md::WelsMdInterSaveSadAndRefMbType(
                    layer_rec_view_expect(pCurLayer),
                    pMbs.cur(),
                    pMd,
                );
            }
            {
                let func_list = (*pEncCtx).func_list();
                (func_list.pfMdBackgroundInfoUpdate)(
                    pEncCtx,
                    pCurLayer,
                    pMbs.cur_mut(),
                    (*pMbCache).bCollocatedPredFlag,
                    ctx_ref_pic(pEncCtx).map_or(0, |p| p.iPictureType),
                );
            }
            UpdateNonZeroCountCache(pMbs.cur(), &mut *pMbCache);

            let mut iEncReturn;
            {
                let func_list = (*pEncCtx).func_list();
                iEncReturn = func_list
                    .eEntropyCoder
                    .WelsSpatialWriteMbSyn(pEncCtx, pSlice, &mut *pMbs, &mut *pSliceBsBuf, &mut *pCtxOutBs);
            }

            if iEncReturn == ENC_RETURN_VLCOVERFLOWFOUND && pMbs.cur().uiLumaQp < 50 {
                {
                    let func_list = (*pEncCtx).func_list();
                    pSlice.iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                        &mut *pSliceBsBuf,
                        slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs),
                        &mut sDss,
                        &mut pSlice.sCabacCtx,
                    );
                    pSlice.uiLastMbQp = sDss.uiLastMbQp;
                }
                UpdateQpForOverflow(pMbs.cur_mut(), kuiChromaQpIndexOffset);
                continue;
            }

            if iEncReturn != ENC_RETURN_SUCCESS {
                return iEncReturn;
            }
            break;
        }

        {
            let func_list = (*pEncCtx).func_list();
            sDss.iCurrentPos = func_list.eEntropyCoder.GetBsPosition(slice_bs_writer_ref(&pSlice.sSliceBs, pCtxOutBs.as_deref()), &pSlice.sCabacCtx);
        }

        if DynSlcJudgeSliceBoundaryStepBack(
            pEncCtx,
            pSlice,
            &pCurLayer.sSliceEncCtx,
            pMbs.cur().iMbXY,
            &mut sDss,
            pMbs,
            // Reborrowed, not taken: the judge runs per macroblock and fires
            // on one of them — a `take` here would consume the slot on the
            // first (non-firing) call and hand the real boundary `None`.
            pNextSlice.as_deref_mut(),
        ) {
            {
                let func_list = (*pEncCtx).func_list();
                pSlice.iMbSkipRun = func_list.eEntropyCoder.StashPopMBStatus(
                    &mut *pSliceBsBuf,
                    slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs),
                    &mut sDss,
                    &mut pSlice.sCabacCtx,
                );
                pSlice.uiLastMbQp = sDss.uiLastMbQp;
            }
            pCurLayer.LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx - 1, Ordering::Relaxed);
            pCurLayer.NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }

        pMbs.cur_mut().uiSliceIdc = kiSliceIdx as u16;
        OutputPMbWithoutConstructCsRsNoCopy(pEncCtx, Some(pCurLayer), pSlice, pMbs.cur());

        {
            let func_list = (*pEncCtx).func_list();
            func_list.pfRc.WelsRcMbInfoUpdate(
                pEncCtx,
                pMbs.cur_mut(),
                (*pMd).iCostLuma,
                &mut *pSlice,
                pCtxOutBs.as_deref(),
            );
        }

        iNumMbCoded += 1;
        iNextMbIdx = WelsGetNextMbOfSlice(&pCurLayer.sSliceEncCtx, iCurMbIdx);
        if iNextMbIdx == -1 || iNextMbIdx >= kiTotalNumMb || iNumMbCoded >= kiTotalNumMb {
            pCurLayer.LastCodedMbIdxOfPartition[kiPartitionId].store(iCurMbIdx, Ordering::Relaxed);
            pCurLayer.NumSliceCodedOfPartition[kiPartitionId].fetch_add(1, Ordering::Relaxed);
            break;
        }
    }

    if pSlice.iMbSkipRun > 0 {
        let kiMbSkipRun = pSlice.iMbSkipRun as u32;
        BsWriteUE(&mut *pSliceBsBuf, slice_bs_writer(&mut pSlice.sSliceBs, pCtxOutBs), kiMbSkipRun);
    }

    ENC_RETURN_SUCCESS
}

pub fn WelsPSliceMdEnc(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    kbIsHighestDlayerFlag: bool,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    // C++ leaves `SWelsMD sMd;` uninitialized and only `memset`s `sMd.sMe` when the
    // base layer is unavailable or this is not the highest spatial layer.
    // `Default::default()` zeroes the whole struct, which is that memset plus zeroes
    // for fields every path assigns before reading.
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:698`.
    sMd.bMdUsingSad = (*pEncCtx).param().iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoop(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub fn WelsPSliceMdEncDynamic(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    kbIsHighestDlayerFlag: bool,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let kpShExt = &(*pSlice).sSliceHeaderExt;
    let kiSliceFirstMbXY = kpShExt.sSliceHeader.iFirstMbInSlice;
    let mut sMd = SWelsMD::default();
    sMd.uiRef = kpShExt.sSliceHeader.uiRefIndex;
    // `svc_encode_slice.cpp:715`.
    sMd.bMdUsingSad = (*pEncCtx).param().iComplexityMode
        == crate::api::codec_api::ECOMPLEXITY_MODE::LOW_COMPLEXITY;

    WelsMdInterMbLoopOverDynamicSlice(pEncCtx, pSlice, &mut sMd, kiSliceFirstMbXY, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub fn WelsCodePSlice(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);
    // `svc_encode_slice.cpp:733/736` picks `pfInterMd` HERE, per slice, into the
    // shared function list — which under MT is every worker writing the same
    // bytes with no ordering. The stamp is loop-invariant across a frame's
    // slices, so it lives in `PreprocessSliceCoding`; only the
    // `kbHighestSpatial` the MD callee needs stays.
    let kbHighestSpatial = if (*pEncCtx).param_opt().is_some() {
        (*pEncCtx).param().iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    WelsPSliceMdEnc(pEncCtx, pSlice, kbHighestSpatial, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub fn WelsCodePOverDynamicSlice(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);
    // `svc_encode_slice.cpp:750/753`, the dynamic-slicing twin of
    // `WelsCodePSlice` — same hoist, same reason: the per-slice `pfInterMd`
    // stamp lives in `PreprocessSliceCoding`.
    let kbHighestSpatial = if (*pEncCtx).param_opt().is_some() {
        (*pEncCtx).param().iSpatialLayerNum == ((*pCurLayer).sLayerInfo.sNalHeaderExt.uiDependencyId as i32 + 1)
    } else {
        true
    };
    WelsPSliceMdEncDynamic(pEncCtx, pSlice, kbHighestSpatial, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub extern "C" fn WelsCodePSlice_c(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    WelsCodePSlice(pCtx, pSlice, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub extern "C" fn WelsCodePOverDynamicSlice_c(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    WelsCodePOverDynamicSlice(pCtx, pSlice, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub extern "C" fn WelsISliceMdEnc_c(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    WelsISliceMdEnc(pCtx, pSlice, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub extern "C" fn WelsISliceMdEncDynamic_c(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    WelsISliceMdEncDynamic(pCtx, pSlice, pSliceBsBuf, pCtxOutBs, pMbs, pRestoreBuf, pNextSlice)
}

pub extern "C" fn WelsSliceHeaderWrite_c(
    pCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    WelsSliceHeaderWrite(pCtx, pCurLayer, pSlice, pParametersetStrategy, pSliceBsBuf, pCtxOutBs);
}

pub extern "C" fn WelsSliceHeaderExtWrite_c(
    pCtx: &sWelsEncCtx,
    pCurLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pParametersetStrategy: Option<&CWelsParametersetIdStrategyObj>,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
) {
    WelsSliceHeaderExtWrite(pCtx, pCurLayer, pSlice, pParametersetStrategy, pSliceBsBuf, pCtxOutBs);
}

pub static g_pWelsSliceCoding: [[PWelsCodingSliceFunc; 2]; 2] = [
    [WelsCodePSlice_c, WelsCodePOverDynamicSlice_c],
    [WelsISliceMdEnc_c, WelsISliceMdEncDynamic_c],
];

pub static g_pWelsWriteSliceHeader: [PWelsSliceHeaderWriteFunc; 2] = [
    WelsSliceHeaderWrite_c,
    WelsSliceHeaderExtWrite_c,
];

/// The one write `WelsCodeOneSlice` made into *layer* state rather than slice
/// state, lifted out of the slice encode to the thread that owns the frame.
///
/// `svc_encode_slice.cpp:1655` sets `pNalHeadExt->bIdrFlag = 1` inside
/// `WelsCodeOneSlice`, which every worker runs once per slice — so N workers write
/// the same layer byte concurrently.
///
/// **The write is loop-invariant across the fork**, and that is checkable rather
/// than plausible:
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
pub fn StampLayerIdrFlagForSliceType(pEncCtx: &mut sWelsEncCtx) {
    if pEncCtx.eSliceType != EWelsSliceType::I_SLICE {
        return;
    }
    // This body runs on the calling thread *before* either fork spawns,
    // precisely so the write does not race.
    let Some(pCurLayer) = current_layer_mut(pEncCtx) else {
        return;
    };
    pCurLayer.sLayerInfo.sNalHeaderExt.bIdrFlag = true;
}

pub fn WelsCodeOneSlice(
    pEncCtx: &sWelsEncCtx,
    pCurSlice: &mut SSlice,
    kiNalType: i32,
    pSliceBsBuf: &mut [u8],
    pCtxOutBs: &mut Option<&mut BsWriter>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pRestoreBuf: Option<&mut [u8]>,
    pNextSlice: Option<&mut SSlice>,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);

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
        // The `pNalHeadExt->bIdrFlag = 1` of `svc_encode_slice.cpp:1655` is not
        // here: it is layer state, and every caller runs it one line above this
        // call. `sScaleShift` is the slice's own and stays. The assert is the
        // hoist's contract, checked where the statement used to be.
        debug_assert!(
            pCurLayer.sLayerInfo.sNalHeaderExt.bIdrFlag,
            "StampLayerIdrFlagForSliceType was not run before WelsCodeOneSlice on an I_SLICE"
        );
        (*pCurSlice).sScaleShift = 0;
    } else {
        let kuiTemporalId = pCurLayer.sLayerInfo.sNalHeaderExt.uiTemporalId;
        let ref_temporal = ctx_ref_pic(pEncCtx).map_or(0, |p| p.uiTemporalId);
        (*pCurSlice).sScaleShift = if kuiTemporalId != 0 { kuiTemporalId.saturating_sub(ref_temporal) } else { 0 };
    }

    WelsSliceHeaderExtInit(pEncCtx, Some(pCurLayer), &mut *pCurSlice);

    //RomRC init slice by slice
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
        (*pEncCtx).func_list().pParametersetStrategy.as_deref(),
        &mut *pSliceBsBuf,
        &mut *pCtxOutBs,
    );

    let pic_init_qp = layer_pps_ref(pEncCtx, pCurLayer).map_or(26, |p| p.iPicInitQp);
    (*pCurSlice).uiLastMbQp =
        (pic_init_qp as i32 + (*pCurSlice).sSliceHeaderExt.sSliceHeader.iSliceQpDelta as i32) as u8;

    let idr_idx = pCurLayer.sLayerInfo.sNalHeaderExt.bIdrFlag as usize;
    let func = g_pWelsSliceCoding[idr_idx][kiDynamicSliceFlag];
    let iEncReturn = func(pEncCtx, &mut *pCurSlice, &mut *pSliceBsBuf, &mut *pCtxOutBs, pMbs, pRestoreBuf, pNextSlice);
    if iEncReturn != ENC_RETURN_SUCCESS {
        return iEncReturn;
    }

    let bEntropyCodingModeFlag = (*pEncCtx).param().iEntropyCodingModeFlag != 0;
    WelsWriteSliceEndSyn(
        &mut *pSliceBsBuf,
        slice_bs_writer(&mut pCurSlice.sSliceBs, pCtxOutBs),
        &mut pCurSlice.sCabacCtx,
        bEntropyCodingModeFlag,
    );

    ENC_RETURN_SUCCESS
}

/// `set_mb_syn_cavlc.cpp:279`. Terminates the slice bitstream.
///
/// The CAVLC branch is a `BsRbspTrailingBits` + `BsFlush` pair, which pushes the
/// last partial 32-bit accumulator word out to the buffer.
///
/// The CABAC branch hands the bitstream cursor back to `SBitStringAux` from the
/// arithmetic coder's own buffer pointer -- there is no `BsRbspTrailingBits` /
/// `BsFlush` pair, because `WelsCabacEncodeFlush` has already written the last
/// bytes directly.
///
/// `pBs` must be the slice's writer (`slice_bs_writer`) and `buf` the threaded
/// buffer that writer is positioned in.
pub fn WelsWriteSliceEndSyn(
    buf: &mut [u8],
    pBs: &mut BsWriter,
    pCabacCtx: &mut crate::encoder::set_mb_syn_cabac::SCabacCtx,
    bEntropyCodingModeFlag: bool,
) {
    if bEntropyCodingModeFlag {
        crate::encoder::set_mb_syn_cabac::WelsCabacEncodeFlush(buf, &mut *pCabacCtx);
        // Both coders count in the same units over the same buffer, so handing
        // the position back is an assignment.
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

pub fn AddSliceBoundary(
    pEncCtx: &sWelsEncCtx,
    pCurSlice: &mut SSlice,
    pSliceCtx: &SSliceCtx,
    kiCurMbIdx: i32,
    iFirstMbIdxOfNextSlice: i32,
    kiLastMbIdxInPartition: i32,
    // The worker's own records, threaded down from the md loop.
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    // The forward slot, resolved by the loop that owns the bank; `None` is the
    // slot past the bank's end, and the write below is skipped for it.
    pNextSlice: Option<&mut SSlice>,
) {
    let pCurLayer = current_layer_expect(pEncCtx);
    let iCurMbIdx = kiCurMbIdx;
    let iCurSliceIdc = {
        let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
        map[iCurMbIdx as usize].load(Ordering::Relaxed)
    };
    let kiSliceIdxStep = (*pEncCtx).iActiveThreadsNum;
    let iNextSliceIdc = iCurSliceIdc + kiSliceIdxStep as u16;

    (*pCurSlice).sSliceHeaderExt.uiNumMbsInSlice = (1 + iCurMbIdx - (*pCurSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice) as u32;

    if let Some(pNextSlice) = pNextSlice {
        pNextSlice.bSliceHeaderExtFlag = (*pCurLayer).sLayerInfo.sNalHeaderExt.sNalUnitHeader.eNalUnitType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;
        pNextSlice.sSliceHeaderExt = (*pCurSlice).sSliceHeaderExt;
        pNextSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = iFirstMbIdxOfNextSlice;

        // C++ calls WelsSetMemMultiplebytes_c, whose count is a signed int32_t:
        // the count can be negative when the boundary lands past the end of the
        // partition.
        {
            let map: &[AtomicU16] = &(*pSliceCtx).pOverallMbMap;
            crate::encoder::slice_multi_threading::fill_mb_map(
                map,
                iFirstMbIdxOfNextSlice,
                kiLastMbIdxInPartition - iFirstMbIdxOfNextSlice + 1,
                iNextSliceIdc,
            );
        }

        UpdateMbNeighbourInfoForNextSlice(
            &(*pCurLayer).sSliceEncCtx,
            pMbs,
            iFirstMbIdxOfNextSlice,
            kiLastMbIdxInPartition,
        );
    }
}

pub fn DynSlcJudgeSliceBoundaryStepBack(
    pEncCtx: &sWelsEncCtx,
    pCurSlice: &mut SSlice,
    pSliceCtx: &SSliceCtx,
    kiCurMbIdx: i32,
    pDss: &mut SDynamicSlicingStack<'_>,
    pMbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pNextSlice: Option<&mut SSlice>,
) -> bool {
    let iCurMbIdx = kiCurMbIdx;
    let kiActiveThreadsNum = (*pEncCtx).iActiveThreadsNum;
    let kiPartitionId = ((*pCurSlice).iSliceIdx % (kiActiveThreadsNum as i32)) as usize;
    let kiEndMbIdxOfPartition = current_layer_expect(pEncCtx).EndMbIdxOfPartition[kiPartitionId];

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
        // `svc_encode_slice.cpp:1776-1791` brackets exactly these two statements in
        // `WelsMutexLock(&pSliceThreading->mutexSliceNumUpdate)` when
        // `iMultipleThreadIdc > 1`, with the C++'s own comment on the lock line
        // saying what it is for: "lock the acessing to this variable:
        // pSliceCtx->iSliceNumInFrame".
        //
        // `pSliceCtx` is the **layer's** slice context, shared by every worker on
        // the dynamic path — so the `+= 1` is a read-modify-write racing across
        // threads, and `AddSliceBoundary` writes `pOverallMbMap` and the next
        // slice's header through the same shared parent (the C++ calls it
        // "complex memory operation" on the line above the lock). A lost
        // increment leaves `iEncodeSliceNum != iSliceNumInFrame` in
        // `ReOrderSliceInLayer`, which answers `ENC_RETURN_UNEXPECTED` and the
        // frame comes back **empty**.
        //
        // The null-mutex arm of `with_wels_mutex` runs the closure unlocked, which
        // is the C++'s `iMultipleThreadIdc <= 1` path: `pSliceThreading` is null
        // there, because `RequestMtResource` only runs above 1.
        let pSmtMutex: Option<&std::sync::Mutex<()>> = {
            let bMt = (*pEncCtx).param_opt().is_some()
                && (*pEncCtx).param().iMultipleThreadIdc > 1;
            if bMt {
                pEncCtx.pSliceThreading.as_deref().map(|pSmt| &pSmt.mutexSliceNumUpdate)
            } else {
                None
            }
        };
        crate::encoder::slice_multi_threading::with_wels_mutex(pSmtMutex, || {
            AddSliceBoundary(pEncCtx, pCurSlice, pSliceCtx, iCurMbIdx, iCurMbIdx, kiEndMbIdxOfPartition, pMbs, pNextSlice);
            pSliceCtx.iSliceNumInFrame.fetch_add(1, Ordering::Relaxed);
        });
        return true;
    }

    false
}

// ============================================================================
// Memory Management, Buffer Allocation & Dynamic Expansion
// ============================================================================

pub fn InitSliceBoundaryInfo(
    pCurLayer: &mut SDqLayer,
    pSliceArgument: &SSliceArgument,
    kiSliceNumInFrame: i32,
) -> i32 {
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

/// `bIndependenceBsBuffer` is recorded as `sSliceBs.pBs`'s nullness and nowhere
/// else — `slice_bs_writer` and the chain-top buffer derivations read it back
/// from there.
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

/// Releases one slice bank. Each slice's bitstream buffer is its own
/// `Option<Vec<u8>>`, so dropping the bank drops every one of them.
pub fn FreeSliceBuffer(pDqLayer: &mut SDqLayer, kiBank: usize) {
    let bank: &mut Vec<SSlice> = &mut (*pDqLayer).sSliceBufferInfo[kiBank].pSliceBuffer;
    bank.clear();
    bank.shrink_to_fit();
}

/// Initialises the slices of one bank, which the caller sized to `kiMaxSliceNum`.
pub fn InitSliceList(
    pBank: &mut SSliceBufferInfo,
    kiMaxSliceNum: i32,
    kiMaxSliceBufferSize: i32,
    bIndependenceBsBuffer: bool,
) -> i32 {
    if kiMaxSliceBufferSize <= 0 {
        return ENC_RETURN_UNEXPECTED;
    }

    for iSliceIdx in 0..kiMaxSliceNum {
        let Some(pSlice) = pBank.pSliceBuffer.get_mut(iSliceIdx as usize) else {
            return ENC_RETURN_MEMALLOCERR;
        };

        pSlice.iSliceIdx = iSliceIdx;
        pSlice.uiBufferIdx = 0;
        pSlice.iCountMbNumInSlice = 0;
        pSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let iRet = InitSliceBsBuffer(pSlice, bIndependenceBsBuffer, kiMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

/// Runs on the calling thread before the fork, under `&mut sWelsEncCtx`.
pub fn InitAllSlicesInThread(pCtx: &mut sWelsEncCtx) -> i32 {
    let pCurDqLayer = current_layer_expect_mut(pCtx);
    for iSliceIdx in 0..pCurDqLayer.iMaxSliceNum {
        let Some(pSlice) = slice_in_layer_mut(pCurDqLayer, iSliceIdx) else {
            return ENC_RETURN_UNEXPECTED;
        };
        pSlice.iSliceIdx = -1;
    }

    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        current_layer_expect_mut(pCtx).sSliceBufferInfo[iSlcBuffIdx as usize].iCodedSliceNum = 0;
    }

    ENC_RETURN_SUCCESS
}

pub fn InitOneSliceInThread(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    kiSlcBuffIdx: i32,
    kiSliceIdx: i32,
) {
    pSlice.iSliceIdx = kiSliceIdx;
    pSlice.uiBufferIdx = kiSlcBuffIdx as u32;

    pSlice.sSliceBs.uiBsPos = 0;
    pSlice.sSliceBs.iNalIndex = 0;
    // The C++ stamped `sSliceBs.pBsBuffer = pThreadBsBuffer[kiSlcBuffIdx]` here;
    // `uiBufferIdx` above already names that slot.
    pSlice.sSliceBs.uiSize = pCtx.iFrameBsSize as u32;
}

pub fn InitSliceThreadInfo(
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
        // Field-wise, not built-once-and-cloned — `SSlice` is 6544 bytes of
        // mostly inline scratch and carries no `Clone`, and the compiler can
        // flatten a field-wise constructor into the `Vec`'s storage where a clone
        // would build and copy.
        (*pDqLayer).sSliceBufferInfo[iIdx as usize].pSliceBuffer =
            (0..iMaxSliceNum as usize).map(|_| SSlice::new()).collect();

        let kbSliceBsBufferFlag = (*pDqLayer).bSliceBsBufferFlag;
        let iRet = InitSliceList(
            &mut (*pDqLayer).sSliceBufferInfo[iIdx as usize],
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

pub fn InitSliceInLayer(
    pCtx: &mut sWelsEncCtx,
    pDqLayer: &mut SDqLayer,
    kiDlayerIndex: i32,
) -> i32 {
    // `SSliceArgument` is `Copy` (`codec_api.rs:577`) and this body only reads it,
    // so it is copied out; nothing writes it in between (`InitSliceThreadInfo`
    // reads `iMultipleThreadIdc` and nothing else of the parameter block).
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

    // One `Vec` sized to the layer's slice count; `SliceIdx::NONE` means "no
    // slice at this position yet".
    (*pDqLayer).ppSliceInLayer = vec![SliceIdx::NONE; (*pDqLayer).iMaxSliceNum as usize];

    (*pDqLayer).pFirstMbIdxOfSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];
    (*pDqLayer).pCountMbNumInSlice = vec![0i32; (*pDqLayer).iMaxSliceNum as usize];

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
    // The C++ copies each id and then the pointer derived from it
    // (`svc_encode_slice.cpp:1169-1172`); the ids are these two lines.
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

/// `ReallocateSliceList` — svc_encode_slice.cpp:1206, as a **`resize`**.
///
/// Under `Vec<SSlice>::resize_with` the existing slices *move* into the grown
/// buffer rather than being copied beside a live original, so each `pBs` is held
/// by exactly one `SSlice` at every point, and the error paths return the bank as
/// it stands instead of freeing a list that shares pointers with a live one. The
/// only reachable difference from the C++ is on an error path the gates cannot
/// reach — allocation failure, or a negative global QP — where this leaves the
/// bank grown with an uninitialised tail and the C++ left a double free; both
/// then propagate `ENC_RETURN_*` to the same caller.
///
/// Slice 0 is the template every new slot copies its header and reference info
/// from, and it stays readable across the new slots' writes because
/// `split_at_mut` — taken *after* the resize — makes the old and new halves
/// disjoint halves of one borrow.
pub fn ReallocateSliceList(
    kiMaxSliceBufferSize: i32,
    kbIndependenceBsBuffer: bool,
    kiNumRef0: u8,
    kiGlobalQp: i32,
    pBank: &mut SSliceBufferInfo,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    if kiMaxSliceNumNew < kiMaxSliceNumOld {
        return ENC_RETURN_INVALIDINPUT;
    }

    if pBank.pSliceBuffer.is_empty() {
        return ENC_RETURN_INVALIDINPUT;
    }
    pBank.pSliceBuffer.resize_with(kiMaxSliceNumNew as usize, SSlice::new);

    let (kpHead, pNewSlices) = pBank.pSliceBuffer.split_at_mut(kiMaxSliceNumOld as usize);
    let kpBaseSlice = &kpHead[0];

    for pSlice in pNewSlices.iter_mut() {
        pSlice.iSliceIdx = -1;
        pSlice.uiBufferIdx = 0;
        pSlice.iCountMbNumInSlice = 0;
        pSlice.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = 0;

        let mut iRet = InitSliceBsBuffer(pSlice, kbIndependenceBsBuffer, kiMaxSliceBufferSize);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        InitSliceHeadWithBase(pSlice, kpBaseSlice);
        InitSliceRefInfoWithBase(pSlice, kpBaseSlice, kiNumRef0);

        iRet = InitSliceRC(pSlice, kiGlobalQp);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
    }

    ENC_RETURN_SUCCESS
}

pub fn CalculateNewSliceNum(
    pCtx: &sWelsEncCtx,
    kiLastCodedSliceIdx: i32,
    iMaxSliceNumOld: i32,
    iMaxSliceNumNew: &mut i32,
) -> i32 {
    if iMaxSliceNumOld == 0 {
        return ENC_RETURN_INVALIDINPUT;
    }

    if (*pCtx).iActiveThreadsNum == 1 {
        *iMaxSliceNumNew = iMaxSliceNumOld * SLICE_NUM_EXPAND_COEF;
        return ENC_RETURN_SUCCESS;
    }

    let iPartitionID = (kiLastCodedSliceIdx % ((*pCtx).iActiveThreadsNum as i32)) as usize;
    let pCurLayer = current_layer_expect(pCtx);
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

/// Grows the bank the calling worker owns, taken out of the layer before the
/// fork.
pub fn ReallocateSliceInThread(
    pCtx: &sWelsEncCtx,
    kiDlayerIdx: i32,
    pBank: &mut SSliceBufferInfo,
) -> i32 {
    let iMaxSliceNum = pBank.iMaxSliceNum;
    let iCodedSliceNum = pBank.iCodedSliceNum;
    let mut iMaxSliceNumNew = 0;
    let kuiSliceMode = (*pCtx)
        .param()
        .sSpatialLayers[kiDlayerIdx as usize]
        .sSliceArgument
        .uiSliceMode;

    // The last-coded slice's one wanted field, as a scalar.
    let Some(kiLastCodedSliceIdx) = pBank
        .pSliceBuffer
        .get((iCodedSliceNum - 1) as usize)
        .map(|s| s.iSliceIdx)
    else {
        return ENC_RETURN_INVALIDINPUT;
    };
    let mut iRet = CalculateNewSliceNum(pCtx, kiLastCodedSliceIdx, iMaxSliceNum, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let kiMaxSliceBufferSize = (*pCtx).iSliceBufferSize[(*pCtx).uiDependencyId as usize];
    let kbIndependenceBsBuffer = (*pCtx).param().iMultipleThreadIdc > 1
        && kuiSliceMode != SliceMode::SM_SINGLE_SLICE;
    iRet = ReallocateSliceList(
        kiMaxSliceBufferSize,
        kbIndependenceBsBuffer,
        (*pCtx).iNumRef0 as u8,
        (*pCtx).iGlobalQp,
        pBank,
        iMaxSliceNum,
        iMaxSliceNumNew,
    );
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    pBank.iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

pub fn ExtendLayerBuffer(
    pCtx: &mut sWelsEncCtx,
    kiMaxSliceNumOld: i32,
    kiMaxSliceNumNew: i32,
) -> i32 {
    let Some(pCurLayer) = current_layer_mut(pCtx) else {
        return ENC_RETURN_SUCCESS;
    };

    // The C++ allocated a new pointer array, dropped the old one **without copying
    // it**, and left every entry to `ReallocSliceBuffer`'s fill loop below. `resize`
    // is that, minus the allocation failure: the tail arrives as `SliceIdx::NONE`,
    // which is the zero `WelsMallocz` handed back.
    {
        let slices: &mut Vec<SliceIdx> = &mut pCurLayer.ppSliceInLayer;
        slices.clear();
        slices.resize(kiMaxSliceNumNew as usize, SliceIdx::NONE);
    }

    // The two remaining triples — allocate, `copy_nonoverlapping` the first
    // `kiMaxSliceNumOld` entries, free the old block — are one `resize` each, which
    // keeps exactly the same guarantee: the existing entries survive at their indices
    // and the new tail is zero, as `WelsMallocz` left it.
    {
        let first: &mut Vec<i32> = &mut pCurLayer.pFirstMbIdxOfSlice;
        first.resize(kiMaxSliceNumNew as usize, 0);
        let count: &mut Vec<i32> = &mut pCurLayer.pCountMbNumInSlice;
        count.resize(kiMaxSliceNumNew as usize, 0);
    }
    let _ = kiMaxSliceNumOld;

    ENC_RETURN_SUCCESS
}

/// Runs single-threaded, on the dynamic realloc path. The layer is re-derived
/// after `ExtendLayerBuffer`, which takes the whole `&mut` context.
pub fn ReallocSliceBuffer(pCtx: &mut sWelsEncCtx) -> i32 {
    let kiCurDid = pCtx.uiDependencyId as usize;
    let kuiSliceMode =
        pCtx.param().sSpatialLayers[kiCurDid].sSliceArgument.uiSliceMode;

    let pCurLayer = current_layer_expect_mut(pCtx);
    let iMaxSliceNumOld = pCurLayer.sSliceBufferInfo[0].iMaxSliceNum;
    let mut iMaxSliceNumNew = 0;

    // The last slot's one wanted field, as a scalar.
    let Some(kiLastCodedSliceIdx) = pCurLayer.sSliceBufferInfo[0]
        .pSliceBuffer
        .get((iMaxSliceNumOld - 1) as usize)
        .map(|s| s.iSliceIdx)
    else {
        return ENC_RETURN_INVALIDINPUT;
    };
    let mut iRet = CalculateNewSliceNum(pCtx, kiLastCodedSliceIdx, iMaxSliceNumOld, &mut iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    // The callee's four context inputs are scalars, read before the bank's `&mut`.
    let kiMaxSliceBufferSize = pCtx.iSliceBufferSize[kiCurDid];
    let kbIndependenceBsBuffer = pCtx.param().iMultipleThreadIdc > 1
        && kuiSliceMode != SliceMode::SM_SINGLE_SLICE;
    let (kiNumRef0, kiGlobalQp) = (pCtx.iNumRef0 as u8, pCtx.iGlobalQp);
    let pCurLayer = current_layer_expect_mut(pCtx);
    iRet = ReallocateSliceList(
        kiMaxSliceBufferSize,
        kbIndependenceBsBuffer,
        kiNumRef0,
        kiGlobalQp,
        &mut pCurLayer.sSliceBufferInfo[0],
        iMaxSliceNumOld,
        iMaxSliceNumNew,
    );
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }
    let pCurLayer = current_layer_expect_mut(pCtx);
    pCurLayer.sSliceBufferInfo[0].iMaxSliceNum = iMaxSliceNumNew;

    iMaxSliceNumNew = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        iMaxSliceNumNew += current_layer_expect(pCtx).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    let kiMaxSliceNumOldLayer = current_layer_expect(pCtx).iMaxSliceNum;
    iRet = ExtendLayerBuffer(pCtx, kiMaxSliceNumOldLayer, iMaxSliceNumNew);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let pCurLayer = current_layer_expect_mut(pCtx);
    let SDqLayer { sSliceBufferInfo, ppSliceInLayer, iMaxSliceNum, .. } = &mut *pCurLayer;
    let mut iStartIdx = 0;
    for (iSlcBuffIdx, bank) in sSliceBufferInfo.iter().enumerate() {
        for iSliceIdx in 0..bank.iMaxSliceNum {
            ppSliceInLayer[(iStartIdx + iSliceIdx) as usize] =
                SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
        }
        iStartIdx += bank.iMaxSliceNum;
    }

    *iMaxSliceNum = iMaxSliceNumNew;

    ENC_RETURN_SUCCESS
}

#[inline]
pub fn CheckAllSliceBuffer(pCurLayer: &mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
    for iSliceIdx in 0..kiCodedSliceNum {
        match slice_in_layer_mut(pCurLayer, iSliceIdx) {
            Some(slice) if iSliceIdx == slice.iSliceIdx => {}
            _ => return ENC_RETURN_UNEXPECTED,
        }
    }
    ENC_RETURN_SUCCESS
}

/// Runs post-join, on the calling thread. The two storages the walk writes
/// (`sSliceBufferInfo`'s slices and `ppSliceInLayer`) are disjoint fields of one
/// destructured `&mut SDqLayer`.
pub fn ReOrderSliceInLayer(pCtx: &mut sWelsEncCtx, kuiSliceMode: SliceMode, kiThreadNum: i32) -> i32 {
    let pCurLayer = current_layer_expect_mut(pCtx);
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

    // The two storages the walk writes, split from one `&mut SDqLayer`: the
    // per-slot stamp and the layer-order index are disjoint fields.
    let SDqLayer { sSliceBufferInfo, ppSliceInLayer, iMaxSliceNum, .. } = &mut *pCurLayer;
    for (iSlcBuffIdx, pBank) in sSliceBufferInfo.iter_mut().take(kiThreadNum as usize).enumerate() {
        let iSliceNumInThread = pBank.iMaxSliceNum;
        for iSliceIdx in 0..iSliceNumInThread {
            let Some(pSliceBuffer) = pBank.pSliceBuffer.get_mut(iSliceIdx as usize) else {
                return ENC_RETURN_UNEXPECTED;
            };

            if pSliceBuffer.iSliceIdx != -1 {
                let iPartitionID = pSliceBuffer.iSliceIdx % iPartitionNum;
                let iActualSliceIdx = aiPartitionOffset[iPartitionID as usize] + pSliceBuffer.iSliceIdx / iPartitionNum;
                pSliceBuffer.iSliceIdx = iActualSliceIdx;
                ppSliceInLayer[iActualSliceIdx as usize] =
                    SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
                iUsedSliceNum += 1;
            } else {
                ppSliceInLayer[(iEncodeSliceNum + iNonUsedBufferNum) as usize] =
                    SliceIdx { bank: iSlcBuffIdx as u8, offset: iSliceIdx };
                iNonUsedBufferNum += 1;
            }
        }
    }

    if iUsedSliceNum != iEncodeSliceNum || *iMaxSliceNum != (iNonUsedBufferNum + iUsedSliceNum) {
        return ENC_RETURN_UNEXPECTED;
    }

    CheckAllSliceBuffer(current_layer_expect_mut(pCtx), iEncodeSliceNum)
}

pub fn GetCurLayerNalCount(pCurDq: &mut SDqLayer, kiCodedSliceNum: i32) -> i32 {
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

/// `FrameBsRealloc` — svc_encode_slice.cpp:1562. `iLbi` indexes
/// `pFbi.sLayerInfo`.
pub fn FrameBsRealloc(
    pCtx: &mut sWelsEncCtx,
    pFbi: &mut SFrameBSInfo,
    iLbi: usize,
    kiMaxSliceNumOld: i32,
) -> i32 {
    // The count is a scalar, so the `pOut` borrow ends on this line and the
    // `param()` reads below are free.
    let mut iCountNals =
        pCtx.out().sNalList.len() as i32;
    let spatial_layers = if pCtx.param_opt().is_some() { pCtx.param().iSpatialLayerNum } else { 1 };
    iCountNals += kiMaxSliceNumOld * (spatial_layers + if pCtx.bNeedPrefixNalFlag { 1 } else { 0 });

    // `Vec::resize` keeps the guarantee that the existing `iCountNals` entries
    // survive at their indices and the new tail is zeroed.
    let pOut = pCtx.out_mut();
    pOut.sNalList.resize(iCountNals as usize, SWelsNalRaw::default());
    pOut.sNalLen
        .resize_with(iCountNals as usize, || std::sync::atomic::AtomicI32::new(0));

    // The C++'s closing loop (`svc_encode_slice.cpp:1589`). The resize moves
    // `sNalLen`, so every `sLayerInfo[..].pNalLengthInByte` handed out before it
    // names the freed block from here on; the C++ re-stamps them from the new
    // root, layer by layer, each layer's cursor being the previous layer's plus
    // that layer's own NAL count.
    debug_assert!(
        iLbi < MAX_LAYER_NUM_OF_FRAME,
        "FrameBsRealloc: layer index {iLbi} is outside pFbi.sLayerInfo"
    );
    // **Ascending, and the order is load-bearing.** Each layer's base is the
    // previous one's plus that layer's NAL count, so the walk must accumulate
    // front to back; walking it backwards leaves every layer pointing at the
    // wrong slot. The walk is over indices into `pOut.sNalLen`, which is the
    // array the realloc above just rebuilt and the thing every one of those
    // pointers points into; the ABI pointer is the reslice at each stop. The
    // current layer's base is restored last, so the encoder's own writes
    // continue where they left off.
    let mut kiBase = 0usize;
    for i in 0..=iLbi {
        pOut.iNalLenBase = kiBase;
        pFbi.sLayerInfo[i].pNalLengthInByte = pOut.nal_len_ptr();
        kiBase += pFbi.sLayerInfo[i].iNalCount.max(0) as usize;
    }

    ENC_RETURN_SUCCESS
}

pub fn SliceLayerInfoUpdate(
    pCtx: &mut sWelsEncCtx,
    pFbi: &mut SFrameBSInfo,
    iLbi: usize,
    kuiSliceMode: SliceMode,
) -> i32 {
    let mut iMaxSliceNum = 0;
    for iSlcBuffIdx in 0..pCtx.iActiveThreadsNum {
        iMaxSliceNum += current_layer_expect(pCtx).sSliceBufferInfo[iSlcBuffIdx as usize].iMaxSliceNum;
    }

    if iMaxSliceNum > current_layer_expect(pCtx).iMaxSliceNum {
        let iCurMaxSliceNum = current_layer_expect(pCtx).iMaxSliceNum;
        let iRet = ExtendLayerBuffer(pCtx, iCurMaxSliceNum, iMaxSliceNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }
        current_layer_expect_mut(pCtx).iMaxSliceNum = iMaxSliceNum;
    }

    let iActiveThreadsNum = pCtx.iActiveThreadsNum as i32;
    let mut iRet = ReOrderSliceInLayer(pCtx, kuiSliceMode, iActiveThreadsNum);
    if iRet != ENC_RETURN_SUCCESS {
        return iRet;
    }

    let iCodedSliceNum = GetCurrentSliceNum(current_layer_expect(pCtx));
    pFbi.sLayerInfo[iLbi].iNalCount = GetCurLayerNalCount(current_layer_expect_mut(pCtx), iCodedSliceNum);
    let iCodedNalCount = GetTotalCodedNalCount(pFbi);

    if iCodedNalCount > pCtx.out().sNalList.len() as i32 {
        let iCurMaxSliceNum = current_layer_expect(pCtx).iMaxSliceNum;
        iRet = FrameBsRealloc(pCtx, pFbi, iLbi, iCurMaxSliceNum);
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

    /// Drive-size knob: `small` under the Miri interpreter, `full` on every native
    /// run — and under Miri again when the battery exports `MIRI_FULL=1`. The env
    /// read needs `-Zmiri-disable-isolation`, which the `--lib` step passes.
    fn miri_scaled(full: i32, small: i32) -> i32 {
        if cfg!(miri) && std::env::var_os("MIRI_FULL").is_none() { small } else { full }
    }

    /// **Encoder initialisation under the aliasing checker.**
    ///
    /// `frames = 0` drives create -> `GetDefaultParams` -> `InitializeExt` ->
    /// `GetOption` -> `Uninitialize` -> destroy and stops there. Encoder
    /// initialisation is where the multi-MiB context, the DQ layers, the slice
    /// buffers, the MVD cost table and the parameter sets are all built.
    ///
    /// 48 x 32 is a 3 x 2 macroblock grid, so MB(1, 1) has all four neighbours,
    /// MB(0, 1) is missing only its left and MB(2, 1) only its top-right. A
    /// single-macroblock picture has no neighbour, so no neighbour-dependent
    /// mode-decision or motion-vector-prediction path runs.
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

    /// **The encode loop over a macroblock grid.**
    ///
    /// The `--lib` Miri step runs this with `-Zmiri-disable-isolation`, for
    /// `WelsTime()` (`SystemTime::now()`, the library's one clock site, called by
    /// `EncodeFrameInternal` around every frame; it does not reach the
    /// bitstream). That flag disables host isolation and nothing else.
    ///
    /// Two frames under Miri, three everywhere else. What the third frame adds is
    /// a second inter frame — the same ME/MD/reconstruction paths as frame 1 with
    /// one more picture in the reference list, and the list update itself runs
    /// after frames 0 and 1 alike. Every assertion below is on frames 0 and 1.
    ///
    /// Ignored under Miri, on cost: this probe's distinguishing axes are CABAC
    /// entropy over LOW_COMPLEXITY on a single slice, and under Miri both are
    /// covered more deeply elsewhere — the size-limited probe drives the CABAC
    /// writers *and* their stash/restore arm at LOW_COMPLEXITY (its options
    /// default `cabac: true`), and the CAVLC probe carries the other entropy
    /// family. It runs at full size on every native `cargo test`.
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

    /// **The fork/join under the aliasing checker.**
    ///
    /// What it drives: `SM_FIXEDSLCNUM_SLICE` with two slices at two threads, which
    /// is `EncodeFixedSlicesForked` — two `SliceJobHandle`s moved across two
    /// `thread::scope` spawns, each owning one bs scratch slot, both calling
    /// `WelsCodeOneSlice`, joined by the scope before `AppendSliceToFrameBs` walks
    /// the slices in index order. Miri checks what the byte gate cannot: that the
    /// two workers' derivations of the shared context do not invalidate each
    /// other, and that the assembly reads what they wrote.
    ///
    /// **112x112, and the size is forced rather than chosen.**
    /// `MIN_NUM_MB_PER_SLICE` is 48 (`wels_encoder_ext.rs:106`), and
    /// `SliceArgumentValidationFixedSliceMode` silently rewrites any multi-slice
    /// request on a smaller picture to `SM_SINGLE_SLICE` — which is what the other
    /// probes' 48x32 (a 3x2 grid, six macroblocks) gets. 7x7 = 49 macroblocks is
    /// the smallest grid above the threshold.
    ///
    /// Two frames, not three: an IDR to build the slice banks and one inter frame so
    /// the fork runs with the mode-decision and motion-estimation halves of the tree
    /// live. `bUseLoadBalancing` is off (the probe forces it), so the slice
    /// boundaries are a function of the input and these assertions mean something.
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
        // Two slices means two VCL NALs per frame. One would mean either that the
        // request was rewritten to `SM_SINGLE_SLICE` and the fork never ran, or
        // that the fork ran a single job and the second slice's bytes never
        // reached the frame.
        assert!(
            frames.iter().all(|f| f.vcl_nals >= 2),
            "a frame carried fewer than two VCL NALs, so a slice did not make it out \
             of the fork: {:?}",
            frames.iter().map(|f| (f.kind, f.vcl_nals)).collect::<Vec<_>>()
        );
    }

    /// **The `UpdateMbMapForked` fork, at a size Miri can afford.**
    ///
    /// This probe does not drive the encoder: the aliasing question is about two
    /// workers and one layer, not about encoding, so it builds the layer by hand
    /// and spawns the same shape `UpdateMbMapForked` does — one scoped thread per
    /// slice, each walking its own slice's macroblocks. Under Miri it is the
    /// instrument that refuses a `&mut` to layer state held across the fork;
    /// natively it is a neighbour-map correctness test, and both assertions below
    /// hold either way.
    #[test]
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

        // The grid is carved into per-slice `&mut [SMB]` before the spawn, exactly
        // as `UpdateMbMapForked` does, so two workers naming one record is a
        // program that does not compile. What the probe checks is that the
        // partition arithmetic hands each worker the records it should, and that
        // the neighbour walk respects the slice boundary.
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

    /// **The layer's NAL header, read shared by every worker.**
    ///
    /// The slice-header writers read `pCurLayer.sLayerInfo.sNalHeaderExt` on every
    /// slice, from every worker: `bIdrFlag`, `uiTemporalId`, the ref-marking gate.
    /// What this certifies: N workers may each take a **shared** borrow of the one
    /// header struct and read it concurrently, which is the shape
    /// (`&SNalUnitHeaderExt`) the writer chain takes.
    #[test]
    #[allow(unsafe_code)]
    fn workers_read_the_layer_nal_header_through_shared_borrows() {
        use crate::common::wels_common_defs::SNalUnitHeaderExt;
        const WORKERS: usize = 2;
        const ROUNDS: usize = 8;

        let mut sHdr = SNalUnitHeaderExt::default();
        sHdr.bIdrFlag = true;
        sHdr.uiTemporalId = 1;
        let kHdrAddr = std::ptr::addr_of_mut!(sHdr) as usize;

        std::thread::scope(|s| {
            for _ in 0..WORKERS {
                s.spawn(move || unsafe {
                    let p = kHdrAddr as *mut SNalUnitHeaderExt;
                    for _ in 0..ROUNDS {
                        // A shared reborrow per read, the way `WelsCodeOneSlice`
                        // and both header writers take it.
                        let hdr: &SNalUnitHeaderExt = &*p;
                        assert!(hdr.bIdrFlag);
                        let _ = hdr.uiTemporalId;
                    }
                });
            }
        });

        assert!(sHdr.bIdrFlag, "nothing wrote the header");
    }

    /// **The boxed banks, under two workers.**
    ///
    /// May a body hold a whole-layer shared borrow while a sibling worker writes
    /// its own slice-buffer bank? With `sSliceBufferInfo` *inline* the answer is
    /// no — `ReallocateSliceList` and `ReallocateSliceInThread` would write into
    /// the layer's own bytes, and a sibling's entry retag races them. Boxed, every
    /// bank write lands in the box's allocation, which no retag of the layer
    /// reaches.
    ///
    /// **The spelling is `ReallocateSliceList`'s, deliberately.** The write below
    /// is `&mut (*p).sSliceBufferInfo[w]` — a real `&mut`, not an `addr_of_mut!` —
    /// because that is what the in-fork writer does, and the two are not
    /// equivalent: a probe using `addr_of_mut!` would create no reference and so
    /// would not exercise the retag that matters. It passes because `Box`
    /// place-deref is built into rustc: no `&mut Box<..>` is created for
    /// `..sSliceBufferInfo[w]`, so nothing retags the eight header bytes that do
    /// live inline.
    #[test]
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

    /// **A whole-layer `&SDqLayer` held while workers stamp their own partition
    /// counters.**
    ///
    /// `NumSliceCodedOfPartition` and `LastCodedMbIdxOfPartition` live **inline in
    /// the layer** and are written from inside the encode — six sites across
    /// `WelsISliceMdEncDynamic` and `WelsMdInterMbLoopOverDynamicSlice`, each
    /// stamping `[kiPartitionId]`. A whole-struct shared retag racing a concurrent
    /// write to an inline field is undefined behaviour under Miri's model.
    ///
    /// With the two arrays atomic, the race is gone by construction and a body may
    /// take a whole-layer shared borrow while its siblings write. That is what this
    /// asserts: each worker re-takes `&*p` every round — the entry retag a called
    /// body performs — and stamps only its own partition slot.
    ///
    /// Like the layer probe above, this does not drive the encoder: the question is
    /// about two workers and one struct, not about encoding.
    #[test]
    #[allow(unsafe_code)]
    fn partition_counters_take_a_shared_layer_borrow_across_the_forked_writes() {
        use std::sync::atomic::Ordering;
        use super::SDqLayer;
        const WORKERS: usize = 2;
        // **200, and the number is load-bearing.** Miri reports a data race only
        // when the schedule it runs actually interleaves the two accesses; only at
        // 200 rounds does the sibling retag land inside the write.
        const ROUNDS: i32 = 200;

        let mut dq = SDqLayer::default();
        dq.iMbWidth = 4;
        dq.iMbHeight = 2;

        // The address as an integer, so the test does not add a hand-written
        // `Send` impl.
        let layer_addr = (&mut dq as *mut SDqLayer) as usize;

        std::thread::scope(|s| {
            for w in 0..WORKERS {
                s.spawn(move || unsafe {
                    let p = layer_addr as *mut SDqLayer;
                    for r in 0..ROUNDS {
                        // **The borrow under test, re-taken every round**: the
                        // read-only bodies are *called*, many times per frame, and
                        // each retags the whole layer on entry. A probe that
                        // borrows once at the top and holds it never interleaves
                        // its retag with the other worker's writes.
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

    /// **The MVD cursor, held across a slice, under two workers.**
    ///
    /// `SWelsMD::pMvdCost` is a borrow of the context's `pMvdCostTable`, and the
    /// two `WelsMdInterMbLoop` bodies derive that borrow once and hold it for the
    /// whole macroblock loop. This probe does not drive the encoder, because the
    /// question is about two workers and one table, not about encoding.
    ///
    /// **The claim, in three parts.**
    ///
    /// 1. The `&[u16]` lands in the `Vec`'s *heap buffer*, which is a different
    ///    allocation from the context, so no retag of the context can reach it and
    ///    holding it across the loop's calls is lawful.
    /// 2. The table is written exactly once, by `MvdCostInit` inside
    ///    `WelsInitEncoderExt`, before any slice worker exists. Concurrent *readers*
    ///    of one buffer coexist freely; a concurrent writer would not, and there is
    ///    none.
    /// 3. Deriving it must be **field-precise** — `&(*p).pMvdCostTable`, never a
    ///    `&self` accessor, which would borrow the whole context.
    ///
    /// The per-worker write below is the *class* of concurrent inline-context write
    /// the fork performs, reduced to its smallest form — one disjoint scalar slot per
    /// worker. It is what makes part 3 observable; parts 1 and 2 hold without it.
    #[test]
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

        // The address as an integer, for the reason the layer probe above gives.
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
    /// than 600 so the IDR comes out in nine slices rather than sixteen.
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

    /// **CAVLC and the fine mode-decision family, both knobs flipped together.**
    ///
    /// The probe above is CABAC over `LOW_COMPLEXITY`, and those two choices leave
    /// two bodies of code dark: the CAVLC writers (`svc_set_mb_syn_cavlc.rs`) and
    /// everything `bFastMode` switches off — `WelsMdIntraFinePartition`,
    /// `WelsMdI4x4` and the `pMemPredBlk4` ping-pong (`svc_base_layer_md.rs`).
    ///
    /// The byte gate does not cover the complexity half either: all 341
    /// diffharness configurations set `iComplexityMode = LOW_COMPLEXITY`
    /// (`diffharness/cxx_enc.cpp:81`) — CABAC vs CAVLC is a sweep axis (`kiCabac`)
    /// but complexity is not — so the fine partition search is checked by neither
    /// instrument, and this is the only coverage it has.
    ///
    /// One test rather than two: each Miri probe pays a multi-MiB `Initialize`
    /// under the interpreter, and the two knobs are independent code selections
    /// that a single encode drives together.
    ///
    /// The assertions are the first probe's, for the first probe's reasons — the
    /// 3x2 macroblock grid read back from the encoder, three frames with the
    /// second inter-coded, and an inter frame an order of magnitude above the
    /// all-skip floor. Two frames under Miri, three everywhere else, for the grid
    /// probe's reason: the third frame's marginal coverage is a second inter frame
    /// over paths frame 1 already ran.
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

    /// **The dynamic-slice probe — `SM_SIZELIMITED_SLICE`.**
    ///
    /// The two probes above encode one slice a frame, so an entire encode path is
    /// otherwise dark: `SM_SIZELIMITED_SLICE` is the only mode with a
    /// macroblock loop of its own (`WelsMdInterMbLoopOverDynamicSlice`,
    /// `WelsISliceMdEncDynamic`), the only caller of the stash-and-rollback pair
    /// (`StashMBStatus`/`StashPopMBStatus`, `wels_func_ptr_def.rs`) and of
    /// `pDynamicBsBuffer`, and the only path that reaches
    /// `CalculateNewSliceNum` → `ReallocSliceBuffer` → `ExtendLayerBuffer` →
    /// `ReOrderSliceInLayer`.
    ///
    /// **It is single-threaded, and that is settled by reading rather than by
    /// configuration**: the two flags that put a size-limited encode on the
    /// multi-threaded slice path, `bSliceBsBufferFlag` and `bThreadSlcBufferFlag`,
    /// both require `iMultipleThreadIdc > 1` (`InitSliceInLayer`, this file), and
    /// the driver fixes `iMultipleThreadIdc = 1`.
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
    /// **`bytes == frame_size` is the covering assertion for the NAL-length
    /// re-stamp.** `bytes` is summed through `sLayerInfo[..].pNalLengthInByte`,
    /// which is what `FrameBsRealloc` invalidates and re-stamps;
    /// `iFrameSizeInBytes` is accumulated as the slices are written and survives
    /// independently.
    ///
    /// The remaining assertions are the first probe's, for the first probe's
    /// reasons: the grid read back from the encoder, three frames with the
    /// second inter-coded, and an inter frame an order of magnitude above the
    /// all-skip floor.
    ///
    /// **48x32 x 2 frames under Miri, 112x96 x 3 everywhere else.** Under Miri the
    /// drive is 48x32 at the same 401-byte constraint — measured 3 / 3 / 3 slices
    /// across three frames. Every frame still splits, each frame still closes
    /// slices through `DynSlcJudgeSliceBoundaryStepBack` / `AddSliceBoundary` /
    /// stash-rollback, and `WelsMdInterMbLoopOverDynamicSlice` and the accounting
    /// assertion stay live. What the small drive cannot reach is the realloc chain
    /// itself (`CalculateNewSliceNum` -> `ReallocSliceBuffer` ->
    /// `ExtendLayerBuffer`): its assertion below is gated to the full drive, which
    /// every native `cargo test` runs, and Miri runs wherever the battery exports
    /// `MIRI_FULL=1`.
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
        // trigger itself. Full drive only: the small Miri geometry cannot reach
        // 35 slices by construction.
        if kbFullDrive {
            assert!(
                slices[0] >= 35,
                "the IDR coded {} slices, under the 35 that make WelsCodeOnePicPartition \
                 call DynSliceRealloc -> ReallocSliceBuffer -> ExtendLayerBuffer: the \
                 realloc path this probe exists for did not run",
                slices[0]
            );
        }

        // The NAL-length cursors and the frame size are two independent
        // accountings of the same bytes; FrameBsRealloc moves the array the first
        // one reads through and re-stamps it.
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
