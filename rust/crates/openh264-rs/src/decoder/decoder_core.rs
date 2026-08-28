#![forbid(unsafe_code)]
/*
 * \copy
 *     Copyright (c)  2013, Cisco Systems
 *     All rights reserved.
 *
 *     Redistribution and use in source and binary forms, with or without
 *     modification, are permitted provided that the following conditions
 *     are met:
 *
 *        * Redistributions of source code must retain the above copyright
 *          notice, this list of conditions and the following disclaimer.
 *
 *        * Redistributions in binary form must reproduce the above copyright
 *          notice, this list of conditions and the following disclaimer in
 *          the documentation and/or other materials provided with the
 *          distribution.
 *
 *     THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 *     "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 *     LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 *     FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 *     COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 *     INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 *     BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 *     LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 *     CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 *     LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 *     ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 *     POSSIBILITY OF SUCH DAMAGE.
 *
 *      decoder_core.rs: Wels decoder framework core implementation in Rust
 */

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_mut
)]

#![deny(unsafe_code)]
// **Phase 5b, T5b.6: the five enumerated families are gone, and so is the
// enumeration.** They retired in this order — `PPicture survivor (F42)` at T5b.1
// (the arm became an identity), `sMCRefMember` with it, `parse tree` at T5b.3 (the
// access unit's slots own; F55/S34 is what that cost), the two zeroed shells at
// T5b.5, and `api boundary` here: `ppDst`, `pDstInfo` and `kpBsBuf` are a plane
// array, a struct and a slice, and **the api-owned context fields are the context's
// own** since T8.A5–A8 — including the reordering buffers, which were
// `CWelsDecoderImpl`'s `[SPictInfo; 16]` reached through a stamped pointer.
//
// `src/decoder/` carries **two** `#[allow(unsafe_code)]` items in total, neither of
// them here: `picture.rs`'s pair of Miri provenance tests for `data_ptr`. What is
// still owed is `data_ptr` itself — its one production use is
// `DecodeFrameConstruction`'s three `ppDst[i]` writes across the C ABI, which is a
// boundary write and belongs with the boundary.

use std::ffi::{c_char, c_void};

// Constants
pub const MIN_ACCESS_UNIT_CAPACITY: usize = 262144;
pub const MAX_ACCESS_UNIT_CAPACITY: usize = 4194304;
pub const MAX_BUFFERED_NUM: usize = 8;
// `MAX_NAL_UNIT_NUM_IN_AU` was declared **here** as 1024 and in `nalu.rs` as the
// C++'s 32 (`wels_const.h:59`), and the two allocators that disagreed about it are
// gone (T5.O4). The C++'s value is re-exported: it is the argument
// `WelsInitStaticMemory` passes, exactly as `decoder_core.cpp:763` passes it, and the
// growth path `MemGetNextNal` owns is what covers an access unit that needs more.
pub use crate::decoder::nalu::MAX_NAL_UNIT_NUM_IN_AU;
pub const MAX_NAL_UNITS_IN_LAYER: usize = 128;
pub const MAX_MB_SIZE: i32 = 36864;
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = 17;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_COEFF_LIST_SIZE: usize = 384;
pub const MB_PARTITION_SIZE: usize = 4;
pub const MAX_MMCO_COUNT: usize = 66;
pub const MAX_PPS_COUNT: usize = 256;
pub const MAX_SPS_COUNT: usize = 32;
pub const MAX_LAYER_NUM: usize = 8;
pub const MAX_SLICEGROUP_IDS: usize = 8;
pub const BASE_QUALITY_ID: u8 = 0;
pub const MV_A: usize = 2;

pub const SLICE_HEADER_IDR_PIC_ID_MAX: u32 = 65535;
pub const SLICE_HEADER_REDUNDANT_PIC_CNT_MAX: u32 = 127;
pub const SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN: i32 = -12;
pub const SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX: i32 = 12;
pub const SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN: i32 = -12;
pub const SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX: i32 = 12;
pub const MAX_NUM_REF_IDX_L0_ACTIVE_MINUS1: u32 = 15;
pub const MAX_NUM_REF_IDX_L1_ACTIVE_MINUS1: u32 = 15;
pub const SLICE_HEADER_CABAC_INIT_IDC_MAX: u32 = 2;

pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

// Macroblock Types -- `wels_common_defs.h:276-283`. This module used to declare
// its own set shifted one bit down (16x16 = 0x2 where the header says 0x8, and so
// on through SKIP = 0x40 where the header says 0x100), and the match in
// `CheckRefPicturesComplete` below reads them, so every macroblock there was
// classified against the wrong mask. Error-free conformance streams never take
// that path, which is why 53/53 stayed green. Twelve other modules already had
// the header's values; use them.
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub use crate::decoder::decode_slice::{
    MB_TYPE_16x16, MB_TYPE_16x8, MB_TYPE_8x16, MB_TYPE_8x8, MB_TYPE_8x8_REF0, MB_TYPE_SKIP,
};

// Error Codes
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_INVALID_PTR: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = 2;
pub const ERR_INFO_INVALID_ACCESS: i32 = 3;
pub const ERR_INFO_INVALID_PARAM: i32 = 4;
pub const ERR_INFO_MB_NUM_INADEQUATE: i32 = 5;
pub const ERR_INFO_PARSEONLY_PENDING: i32 = 6;
pub const ERR_INFO_PARSEONLY_ERROR: i32 = 7;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = 8;
pub const ERR_INFO_DUPLICATE_FRAME_NUM: i32 = 9;
pub const ERR_LEVEL_SLICE_HEADER: i32 = 0x0001;
pub const ERR_INFO_INVALID_FIRST_MB_IN_SLICE: i32 = 10;
pub const ERR_INFO_INVALID_SLICE_TYPE: i32 = 11;
pub const ERR_INFO_PPS_ID_OVERFLOW: i32 = 12;
pub const ERR_INFO_INVALID_PPS_ID: i32 = 13;
pub const ERR_INFO_NO_PARAM_SETS: i32 = 14;
pub const ERR_INFO_INVALID_SPS_ID: i32 = 15;
pub const ERR_INFO_UNSUPPORTED_MBAFF: i32 = 16;
pub const ERR_INFO_INVALID_FRAME_NUM: i32 = 17;
pub const ERR_INFO_INVALID_IDR_PIC_ID: i32 = 18;
pub const ERR_INFO_INVALID_REDUNDANT_PIC_CNT: i32 = 19;
pub const ERR_INFO_INVALID_NUM_REF_IDX_L0_ACTIVE_MINUS1: i32 = 20;
pub const ERR_INFO_INVALID_NUM_REF_IDX_L1_ACTIVE_MINUS1: i32 = 21;
pub const ERR_INFO_REF_COUNT_OVERFLOW: i32 = 22;
pub const ERR_INFO_INVALID_REF_REORDERING: i32 = 23;
pub const ERR_INFO_INVALID_REF_MARKING: i32 = 24;
pub const ERR_INFO_INVALID_CABAC_INIT_IDC: i32 = 25;
pub const ERR_INFO_INVALID_QP: i32 = 26;
pub const ERR_INFO_UNSUPPORTED_SPSI: i32 = 27;
pub const ERR_INFO_INVALID_DBLOCKING_IDC: i32 = 28;
pub const ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2: i32 = 29;
pub const ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2: i32 = 30;
pub const ERR_INFO_UNSUPPORTED_ILP: i32 = 31;
pub const ERR_INFO_UNSUPPORTED_MGS: i32 = 32;
pub const ERR_INFO_UNSUPPORTED_SLICESKIP: i32 = 33;
pub const ERR_INFO_FMO_INIT_FAIL: i32 = 34;
pub const ERR_INFO_INVALID_LUMA_LOG2_WEIGHT_DENOM: i32 = 35;
pub const ERR_INFO_INVALID_CHROMA_LOG2_WEIGHT_DENOM: i32 = 36;
pub const ERR_INFO_INVALID_LUMA_WEIGHT: i32 = 37;
pub const ERR_INFO_INVALID_LUMA_OFFSET: i32 = 38;
pub const ERR_INFO_INVALID_CHROMA_WEIGHT: i32 = 39;
pub const ERR_INFO_INVALID_CHROMA_OFFSET: i32 = 40;

// Bitmask Error Status Flags
pub const dsErrorFree: i32 = 0x00;
pub const dsFramePending: i32 = 0x01;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDepLayerLost: i32 = 0x08;
pub const dsNoParamSets: i32 = 0x10;
pub const dsDataErrorConcealed: i32 = 0x20;
pub const dsRefListNullPtrs: i32 = 0x40;
pub const dsOutOfMemory: i32 = 0x4000;

// MMCO Types
pub const MMCO_END: u32 = 0;
pub const MMCO_SHORT2UNUSED: u32 = 1;
pub const MMCO_LONG2UNUSED: u32 = 2;
pub const MMCO_SHORT2LONG: u32 = 3;
pub const MMCO_SET_MAX_LONG: u32 = 4;
pub const MMCO_RESET: u32 = 5;
pub const MMCO_LONG: u32 = 6;

// Overwrite Flags
pub const OVERWRITE_NONE: i32 = 0;
pub const OVERWRITE_PPS: i32 = 1;
pub const OVERWRITE_SPS: i32 = 2;
pub const OVERWRITE_SUBSETSPS: i32 = 4;

pub use crate::decoder::error_concealment::{ERROR_CON_IDC, ERROR_CON_IDC::*};


// Log levels — **re-exported, not redeclared** (D-fid-4, 2026-08-26, from F184).
// This file used to carry its own copy; all five copies were consecutive integers
// where `codec_app_def.h:323-331` is a bit mask, and the level reaches the caller's
// own callback. A re-export cannot diverge from the canonical block again.
pub use crate::common::wels_trace::{WELS_LOG_DEBUG, WELS_LOG_ERROR, WELS_LOG_INFO, WELS_LOG_WARNING};

pub const videoFormatI420: i32 = 23;

#[inline]
pub fn GENERATE_ERROR_NO(level: i32, info: i32) -> i32 {
    (level << 16) | info
}

#[inline]
pub fn WELS_MAX<T: PartialOrd>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

#[inline]
pub fn WELS_MIN<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

#[inline]
pub fn WELS_CLIP3(x: i32, min_val: i32, max_val: i32) -> i32 {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline]
pub fn WELS_ABS(x: i32) -> i32 {
    x.abs()
}

#[inline]
pub fn IS_VCL_NAL(eNalType: EWelsNalUnitType, _unused: i32) -> bool {
    matches!(
        eNalType,
        EWelsNalUnitType::NAL_UNIT_CODED_SLICE
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPA
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPB
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPC
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
    )
}

pub use crate::decoder::slice::EWelsSliceType;
pub use crate::decoder::slice::EWelsSliceType::*;



pub use crate::decoder::nalu::EWelsNalUnitType;
pub use crate::decoder::nalu::EWelsNalUnitType::*;


// Data Structures Matching C/C++ Layout

pub use crate::decoder::decoder_context::SPosOffset;
use crate::decoder::decoder_context::ec_active_idc;


pub use crate::decoder::decoder_context::ParseOnlyBsBuffers;


#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVui {
    pub bAspectRatioInfoPresentFlag: bool,
    pub uiAspectRatioIdc: u8,
    pub uiSarWidth: u32,
    pub uiSarHeight: u32,
    pub bOverscanInfoPresentFlag: bool,
    pub bOverscanAppropriateFlag: bool,
    pub bVideoSignalTypePresentFlag: bool,
    pub uiVideoFormat: u8,
    pub bVideoFullRangeFlag: bool,
    pub bColourDescriptionPresentFlag: bool,
    pub bColourDescripPresentFlag: bool,
    pub uiColourPrimaries: u8,
    pub uiTransferCharacteristics: u8,
    pub uiMatrixCoefficients: u8,
    pub uiMatrixCoeffs: u8,
    pub bChromaLocInfoPresentFlag: bool,
    pub uiChromaSampleLocTypeTopField: u8,
    pub uiChromaSampleLocTypeBottomField: u8,
    pub bTimingInfoPresentFlag: bool,
    pub uiNumUnitsInTick: u32,
    pub uiTimeScale: u32,
    pub bFixedFrameRateFlag: bool,
    pub bNalHrdParamPresentFlag: bool,
    pub bVclHrdParamPresentFlag: bool,
    pub bPicStructPresentFlag: bool,
    pub bBitstreamRestrictionFlag: bool,
    pub bMotionVectorsOverPicBoundariesFlag: bool,
    pub uiMaxBytesPerPicDenom: u32,
    pub uiMaxBitsPerMbDenom: u32,
    pub uiLog2MaxMvLengthHorizontal: u32,
    pub uiLog2MaxMvLengthVertical: u32,
    pub uiMaxNumReorderFrames: u32,
    pub uiMaxDecFrameBuffering: u32,
}

pub use crate::decoder::parameter_sets::SLevelLimits;


pub use crate::decoder::parameter_sets::{SSps, SPps, SSubsetSps, SSpsSvcExt};
pub use crate::decoder::decoder_context::{SWelsDecoderSpsPpsCTX as SSpsPpsCtx};


pub use crate::decoder::slice::{SPredWeightTable, SPredList};



pub use crate::decoder::slice::{SRefPicListReorderSyn, SRefPicMarking, SReorderingSyntax, SRefBasePicMarking};


pub use crate::decoder::bit_stream::{BsReader, RawDataBuffer};
use crate::safe::bits::BsCursor;
pub use crate::safe::mb_grid::{MbArray, MbDims, MbGrid, LIST_COUNT};
pub use crate::decoder::decoder_context::{SNalUnitHeader, SNalUnitHeaderExt};
pub use crate::decoder::decoder_context::{FEEDBACK_NON_VCL_NAL, FEEDBACK_UNKNOWN_NAL, FEEDBACK_VCL_NAL};
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SSlice};



// The duplicate SVclNal/SPrefixNalUnit/SNalData definitions that used to sit here
// were deleted dead at T3.3 (S18): every live `sNalData` access resolves through
// `nalu::SNalUnit`, whose own definitions are the ones the decoder uses.

pub use crate::decoder::nalu::SAccessUnit;
use crate::decoder::decoder_context::{
    active_fmo, active_pps, active_sps, au_has_nals, cur_au, cur_and_refs, cur_dq_layer, dec_pic,
    fmo_of_mut, parser_bs, pic_pool_mut, pic_refs, pool_pic, pps_of, ref_id, ref_pic, sps_of,
    sps_ref_of, subset_sps_of, SpsRef,
};
// T5.W1: `use crate::decoder::picture::pic_slot;` sat here with **no user** — the
// last one went at T5.P″1 (`:3691`'s comment records the deletion), and the import
// outlived it because `unused_imports` is allowed crate-wide (`lib.rs`). Found by
// family 2's caller grep, deleted here.


#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub sSliceInLayer: SSlice,
    /// T5.R6: the layer's copies of the slice header's parameter-set ids.
    pub sps_ref: Option<SpsRef>,
    pub pps_id: Option<i32>,
    pub subset_sps_id: Option<i32>,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        Self {
            sNalHeaderExt: SNalUnitHeaderExt::default(),
            sSliceInLayer: SSlice::default(),
            sps_ref: None,
            pps_id: None,
            subset_sps_id: None,
        }
    }
}



/// The decoder's DQ-layer state.
///
/// **Named `SDqLayer` until T5.M1**, when the last of the 22 per-macroblock array
/// families flipped onto [`MbGrid`] and the struct stopped holding a per-macroblock
/// pointer at all. The `S`-prefix said "this is the C's `dec_frame.h:50` struct,
/// field for field"; it is not one any more — it owns its heap, it is not `Copy`,
/// and its per-macroblock state is one bounds-checked container rather than 22
/// bare pointers. The encoder's namesake keeps the C's name and the C's shape.
///
/// `Copy` came off at **T5.H3**, for the reason it came off `SPicture` at T5.C3:
/// the struct owns heap now. `#[repr(C)]` stays because every other field is still
/// the C's, but nothing pins this struct's layout — the `assert_size!(SDqLayer,
/// 512)` in `encoder/abi_guard.rs` pins the **encoder's** same-named struct and is
/// unaffected by this rename. The compiler was asked first: nothing in the crate
/// copied a decoder layer by value.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct DqLayerState {
    /// **The grid** (5.2). Every per-macroblock array the layer owns, with one set
    /// of dimensions — the **allocation's**, fixed when the layer is constructed
    /// (T5.E2) — and indexing that panics rather than running off the end.
    ///
    /// Sized once at [`InitialDqLayersContext`] and dropped with the layer. **All 22
    /// families are on it** (T5.H4–T5.L7); every one of their `WelsMallocz`/`WelsFree`
    /// pairs died with its accesses, and the rename above is what that completion
    /// bought.
    pub grid: MbGrid,
    pub sLayerInfo: SLayerInfo,
    // T5.M3: `pBitStringAux: *mut BsReader` sat here, mirroring
    // `&pNalCur->sNalData.sVclNal.sSliceBitsRead` beside its owner for 43 readers.
    // `bit_stream::slice_bit_reader(pCtx)` derives it instead; the NAL unit owns the
    // reader and nothing else holds its address.
    // T5.S1: `pFmo: *mut TagFmo` sat here, mirroring `dec_frame.h:65`'s field. It is
    // dead in **both** trees — zero readers and zero writers here, and every C++
    // `->pFmo` is `pCtx->pFmo` — so it deletes the way `DqLayerState::pRef` did
    // (T5.P′1) rather than converting. The FMO state a slice needs is the context's
    // `sFmoList` entry, reached through `active_fmo`.
    // T5.H1: `pNzcRs` (24 bytes per macroblock) and `pInterPredictionDoneFlag`
    // (one byte per macroblock) sat here. Both are dead in **both** trees: `pNzcRs` is allocated, aliased onto
    // the layer (`decoder_core.cpp:2471`) and never read or written by anything;
    // `pInterPredictionDoneFlag` is written `= 0` at 14 sites in `decode_slice.cpp`
    // and read at none. Deleting them costs 2 of the grid's 24 arrays and 2 of its
    // 27 allocations before 5.2 carries either into a safe container.
    pub iLumaStride: i32,
    pub iChromaStride: i32,
    // T5.X8: `pPred: [*mut u8; 3]` stood here — the current macroblock's top-left
    // sample in each plane, stamped once per macroblock by
    // `WelsFillRecNeededMbInfo`. It was a cache of `(iMbX, iMbY)` resolved against
    // the picture, and both are parameters of every function that read it.
    pub iMbX: i32,
    pub iMbY: i32,
    pub iMbXyIndex: i32,
    pub iMbWidth: i32,
    pub iMbHeight: i32,

    /* Common syntax elements across all slices of a DQLayer */
    pub iSliceIdcBackup: i32,
    pub uiSpsId: u32,
    pub uiPpsId: u32,
    pub uiDisableInterLayerDeblockingFilterIdc: u32,
    pub iInterLayerSliceAlphaC0Offset: i32,
    pub iInterLayerSliceBetaOffset: i32,
    pub iSliceGroupChangeCycle: i32,

    // **T5.AC2 — the layer's four aliases into the NAL's slice header, converted
    // and one of them deleted.** Each was `addr_of_mut!` of a field of the slice
    // header the layer had *already copied whole* one statement earlier
    // (`sLayerInfo.sSliceInLayer.sSliceHeaderExt`), so the pointer carried no
    // datum the layer did not own — it carried a **retention rule**, which is the
    // part a naive read of the copy would have lost: all four are stamped only
    // when `uiQualityId == BASE_QUALITY_ID`, and at an SVC quality-enhancement
    // slice they keep the base slice's values while `sLayerInfo` is overwritten
    // with the enhancement slice's. That is exactly what an `Option` written in
    // the same block reproduces: the field is assigned nowhere else, so it
    // retains, and `None` is the null the readers already tested for.
    //
    // `pRefPicBaseMarking` is **deleted rather than converted** — one stamp
    // (`:3619`), zero readers anywhere in the crate, the same shape as
    // `DqLayerState::pRef` at T5.P′1.
    pub sRefPicListReordering: Option<SRefPicListReorderSyn>,
    pub sPredWeightTable: Option<SPredWeightTable>,
    pub sRefPicMarking: Option<SRefPicMarking>,

    // **T5.P′1 (W2b) — `pRef` and `pDec` sat here, and both are gone.**
    //
    // `pRef` was dead in this port: zero readers, zero writers, at every commit the
    // grep was taken.
    //
    // `pDec` was a **cache of `dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec)`**, not a second carrier. One stamp site
    // in the whole decoder (`InitDqLayerInfo`, from `DecodeCurrentAccessUnit`'s inner
    // loop), and S23's question — *can the source change behind the cache?* — is
    // answered no by an invariant that is worth keeping written down:
    //
    //   Every `(*pCtx).pDec = None` in `DecodeCurrentAccessUnit` either fires only
    //   when `iTotalNumMbRec == 0` (`:3716`, `:3738`) or runs after
    //   `DecodeFrameConstruction` has just zeroed it (`:3847`, `:3862`), and the one
    //   reader reachable outside the decode window — `GetAvilInfoFromCorrectMb`, via
    //   `CheckAndFinishLastPic` → `ImplementErrorCon` — is gated on
    //   `iTotalNumMbRec != 0`. So "the context has no picture" and "a layer read can
    //   run" are mutually exclusive, and no reader could ever observe the cache
    //   holding a picture the context had dropped.
    //
    // Readers that hold `pCtx` derive; the layer-only leaves take the picture as a
    // parameter from their `pCtx`-holding caller (never storing it); the one identity
    // comparison is `PicId` equality with no dereference on either side. 160 sites
    // over seven files, and the layer never advances mid-access-unit — the cache field
    // that made that claim readable is gone (T5.R1) and the layer itself is threaded
    // from the access-unit loop's own derivation.
    pub iColocMv: [[[i16; 2]; 16]; 2],
    pub iColocRefIndex: [[i8; 16]; 2],
    pub iColocIntra: [i8; 16],

    pub bUseWeightPredictionFlag: bool,
    pub bUseWeightedBiPredIdc: bool,
    pub bStoreRefBasePicFlag: bool,
    pub bTCoeffLevelPredFlag: bool,
    pub bConstrainedIntraResamplingFlag: bool,
    pub uiRefLayerDqId: u8,
    pub uiRefLayerChromaPhaseXPlus1Flag: u8,
    pub uiRefLayerChromaPhaseYPlus1: u8,
    pub uiLayerDqId: u8,
    pub bUseRefBasePicFlag: bool,
}

impl DqLayerState {
    /// A layer whose [`grid`](Self::grid) covers `dims`, and whose every other field
    /// is what `WelsMallocz`'s zeroing left it — plus the two the C++ constructor
    /// overwrites (`uiRefLayerDqId = 255`, `uiRefLayerChromaPhaseYPlus1 = 1`).
    ///
    /// # S21, and why `Default` is gone
    ///
    /// `impl Default for DqLayerState` zeroed the whole struct through the intrinsic, and
    /// `InitialDqLayersContext` reached the same state the other way, through
    /// `WelsMallocz`. Both stopped being legal the moment the struct owned a `Vec`:
    /// a zeroed `Vec` is a null pointer where a dangling-aligned one is required,
    /// and the value is invalid before anything reads it. T5.F2 made the allocator
    /// carry provenance, which is what lets Miri *see* that heap — it does not make
    /// a zeroed `Vec` valid.
    ///
    /// **T5b.5: the shell is gone and the constructor says what the zeroing meant.**
    /// It used to be `SWelsDecoderContext::new_boxed`'s shape (zero a `MaybeUninit`,
    /// write the owning field through it with `addr_of_mut!`, materialize) — a
    /// spelling that put every *other* field's initial value beyond reading. Written
    /// out, the C's `WelsMallocz` zero is a value like any other, and the two the C++
    /// constructor overwrites stop being a fixup after the fact.
    ///
    /// `Default` is not reinstated on top of this: a layer cannot be constructed
    /// without dimensions any more, and a `Default` that invented some would be the
    /// same lie the wholesale zeroing was.
    pub fn for_grid(dims: MbDims) -> Self {
        Self {
            // The one owning field, and the reason the shell existed: a zeroed `Vec`
            // is a null pointer where a dangling-aligned one is required.
            grid: MbGrid::new(dims),
            // `SLayerInfo`'s `Default` is `WelsMallocz`'s zero field for field, with
            // one deliberate difference and it is T5.Z1's: `Option<SpsRef>` keeps its
            // niche in a `bool`, so the all-zero pattern reads back as
            // `Some(SpsRef { id: 0, subset: false })` where the C's memset leaves a
            // null `pSps`. `None` is the faithful spelling; the zero pattern was not.
            sLayerInfo: SLayerInfo::default(),
            iLumaStride: 0,
            iChromaStride: 0,
            iMbX: 0,
            iMbY: 0,
            iMbXyIndex: 0,
            // Not the allocation's dimensions: these are the *current slice's*, and
            // the C leaves them zero until `InitDqLayerInfo` writes them (T5.E2).
            iMbWidth: 0,
            iMbHeight: 0,
            iSliceIdcBackup: 0,
            uiSpsId: 0,
            uiPpsId: 0,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            iSliceGroupChangeCycle: 0,
            // The three the C++ carries as pointers into the slice header; `None` is
            // the null it memsets them to.
            sRefPicListReordering: None,
            sPredWeightTable: None,
            sRefPicMarking: None,
            iColocMv: [[[0; 2]; 16]; 2],
            iColocRefIndex: [[0; 16]; 2],
            iColocIntra: [0; 16],
            bUseWeightPredictionFlag: false,
            bUseWeightedBiPredIdc: false,
            bStoreRefBasePicFlag: false,
            bTCoeffLevelPredFlag: false,
            bConstrainedIntraResamplingFlag: false,
            // The two the C++ constructor overwrites after its own zeroing.
            uiRefLayerDqId: 255,
            uiRefLayerChromaPhaseXPlus1Flag: 0,
            uiRefLayerChromaPhaseYPlus1: 1,
            uiLayerDqId: 0,
            bUseRefBasePicFlag: false,
        }
    }
}

// **T5.AA3: `mb_grid_ptr` is deleted.** It was the phase's last raw bridge into
// [`MbGrid`]'s per-macroblock arrays — the base as a pointer, for callers that
// index at *neighbour* addresses and so could not take a narrowing slice (S28).
// Its final production caller was `ParseIntraPredModeChromaCabac`'s two neighbour
// reads of the picture's `pMbType`, and `MbArray::get` reaches both with a bound.
// The seven Miri provenance tests that pinned its reach go with it: the accessor
// they instrument no longer exists. `SPicture::data_ptr` is the phase's one
// remaining bridge of this shape, and it is the named survivor (Phase 8's).


pub use crate::decoder::decoder_context::SRefPic;

// **T8.A1 — a second `SSysMEMBuffer` and a second `SUsrData` stood here, and the
// first of them disagreed with the ABI's.** Neither had a consumer: `SUsrData` had
// none at all, and `SSysMEMBuffer` had exactly one — `SUsrData`'s member. The live
// union is `api/codec_api.rs`'s `SBufferInfoUsrData`, re-exported with `SBufferInfo`
// just below, and the live `SSysMEMBuffer` is the api's, re-exported through
// `decoder_context.rs`.
//
// The divergence they carried is worth naming, because it is the reason a dead
// duplicate is still a defect: this copy's `Default` set `iFormat` to
// `videoFormatI420` (23) where the api's derives it to **0**. One `use` line
// resolving the other way — the shape F43 turned on — and every `SBufferInfo`
// default in the port would have announced a colour format the C++ leaves zero.
pub use crate::api::codec_api::SBufferInfo;

pub use crate::decoder::decoder_context::SDecoderStatistics;


pub use crate::decoder::decoder_context::{SDecodingParam, SLogContext};


pub use crate::decoder::decoder_context::SWelsCabacDecEngine;


// **F43, T5.S1: a second, structurally different `SFmo` stood here** —
// `pSliceGroupMap`/`iSliceGroupCount` against `fmo.rs`'s
// `pMbAllocMap`/`iCountMbNum`/`iSliceGroupCount`/`iSliceGroupType`/`bActiveFlag`.
// It existed only to type this file's two FMO stubs, and its one use cast the
// context's real `TagFmo` to it. The name resolves to `fmo.rs`'s now, which is the
// type the context's `sFmoList` has always held.
pub use crate::decoder::fmo::SFmo;

/// Reference-picture border expansion length (`PADDING_LENGTH` in
/// `codec/common/inc/expand_pic.h`).
pub const PADDING_LENGTH: usize = 32;

// **T5.AA3: `expand_shim_span`, `ExpandPictureLuma_c` and `ExpandPictureChroma_c`
// moved to `common/expand_pic.rs`.** They were phase-2 shim wrappers living in
// `src/decoder/` whose only consumer is `common::expand_pic::ExpandReferencingPicture`
// — a `common/` function importing from the decoder, the dependency the wrong way
// round, and the reason two of the decoder's six shim markers were not the
// decoder's work at all. Both codecs call them through that one consumer, so the
// move touches no encoder line (F12/P10) and Phase 6 deletes them with the rest of
// the raw entry points.

pub use crate::decoder::decoder_context::SWelsDecoderContext;

pub use crate::decoder::nalu::{SNalUnit};

// `PDqLayer` stood here — the C's `typedef SDqLayer*`, and the last raw alias into
// the layer. **T5.AA1 deleted it**: the two brackets that derived it move the layer
// out of the context for the loop instead, so every function that took it now takes
// `Option<&mut DqLayerState>` and the compiler checks what the round trip through a
// pointer was hiding.

pub use crate::decoder::decoder_context::{Picture, SPicture, SPicBuff};



// Logging and Bitstream Reading Helpers

/// The C++'s logging entry point, stubbed. It keeps `unsafe` although the body is
/// empty: the parameter is a raw log context and any real implementation
/// dereferences it, so the contract is the signature's, not today's body's.
// **T8.B6: the decoder's `WelsLog` stub stood here**, the twin of the encoder's,
// and this side already had seventeen call sites feeding it — every one of them
// dropped on the floor. One implementation now, in `common::wels_trace`, and it
// delivers.
pub use crate::common::wels_trace::WelsLog;

#[inline]
pub fn BsGetBits(buf: &[u8], pBs: &mut BsCursor, n: u32, pOut: &mut u32) -> i32 {
    crate::decoder::dec_golomb::BsGetBits(buf, pBs, n as i32, pOut)
}

#[inline]
pub fn BsGetOneBit(buf: &[u8], pBs: &mut BsCursor, pOut: &mut u32) -> i32 {
    crate::decoder::dec_golomb::BsGetBits(buf, pBs, 1, pOut)
}

#[inline]
pub fn BsGetUe(buf: &[u8], pBs: &mut BsCursor, pOut: &mut u32) -> i32 {
    crate::decoder::dec_golomb::BsGetUe(buf, pBs, pOut) as i32
}

#[inline]
pub fn BsGetSe(buf: &[u8], pBs: &mut BsCursor, pOut: &mut i32) -> i32 {
    crate::decoder::dec_golomb::BsGetSe(buf, pBs, pOut)
}

// `WelsMalloczHelper` and `WelsFreeHelper` stood here — the decoder's allocator
// wrappers, each with a `pMa`-or-global arm, and after T5.R4 they had no callers: the
// last three were the parse-only descriptor and its two buffers. **W3's done-test is
// this**: `WelsMallocz|WelsFree` in `src/decoder/` is now prose only, and every
// allocation the decoder makes is an owner with drop glue.

// External and Internal Helper Stubs

/// Number of decoding threads. Always **0**: the C++ decoder's multi-threading was
/// never ported, and `SWelsDecoderContext::pThreadCtx` — the field this used to read
/// through — was declared and read but never once assigned (T5c).
///
/// Matches `GetThreadCount` in `decoder_context.h`, and the function is kept rather
/// than inlined away so its ~10 call sites stay honest about the question they are
/// asking. **The literal must be `0`, not `1`**: every other caller tests `> 1` or
/// `<= 1` and cannot tell the two apart, but `api/codec_api.rs:1831` branches on
/// `GetThreadCount(p_ctx) <= 0` to increment `uiDecodeTimeStamp`, so a `1` here would
/// silently stop that branch running and change the decoding timestamp.
#[inline]
/// **T5.Z4: it takes a shared borrow now, and the parameter is still unused.**
/// F36 owns whether decoder threading returns at all; until then this is `0`. As a
/// `&mut` it was worse than useless — a `Unique` function-entry retag over the whole
/// context at every call site, which pops every live derivation from it. That is
/// what the flip's one Miri failure was: `slice_ctx` derived `sRawData`, then read
/// this, then built the struct, and the field borrow was already dead.
pub fn GetThreadCount(_pCtx: &SWelsDecoderContext) -> i32 {
    0
}

pub fn ResetDecStatNums(pDecStat: &mut SDecoderStatistics) {
    let width = (*pDecStat).uiWidth;
    let height = (*pDecStat).uiHeight;
    let avg_luma_qp = (*pDecStat).iAvgLumaQp;
    let profile = (*pDecStat).uiProfile;
    let level = (*pDecStat).uiLevel;
    *pDecStat = SDecoderStatistics::default();
    (*pDecStat).uiWidth = width;
    (*pDecStat).uiHeight = height;
    (*pDecStat).iAvgLumaQp = avg_luma_qp;
    (*pDecStat).uiProfile = profile;
    (*pDecStat).uiLevel = level;
}

fn UpdateDecStatFreezingInfo(idr_flag: bool, pDecStat: &mut SDecoderStatistics) {
    if idr_flag {
        (*pDecStat).uiFreezingIDRNum += 1;
    } else {
        (*pDecStat).uiFreezingNonIDRNum += 1;
    }
}

#[inline]
pub fn UpdateDecStatNoFreezingInfo(pCtx: &mut SWelsDecoderContext, pCurDq: Option<&DqLayerState>) {
    let Some(pCurDq) = pCurDq else {
        return;
    };
    // The two facts this body needs from *other* context fields, taken as values
    // before the statistics borrow opens: `pParam` and `pPicBuff` are disjoint from
    // `pDecoderStatistics`, and reading them out is cheaper than proving it at four
    // interleaved sites.
    let bEcDisabled =
        pCtx.pParam.eEcActiveIdc == ERROR_CON_DISABLE;
    if pCtx.pDec.is_none() {
        return;
    }
    let Some(bIsComplete) = dec_pic(&mut pCtx.pPicBuff, pCtx.pDec).map(|p| p.bIsComplete) else {
        return;
    };
    // The C++'s `if (NULL == pCtx->pDecoderStatistics) return;`.
    let pDecStat = &mut pCtx.pDecoderStatistics;

    if pDecStat.iAvgLumaQp == -1 {
        pDecStat.iAvgLumaQp = 0;
    }

    let mut iTotalQp = 0i64;
    let kiMbNum = (pCurDq.iMbWidth * pCurDq.iMbHeight) as usize;
    if bEcDisabled {
        for iMb in 0..kiMbNum {
            iTotalQp += *pCurDq.grid.luma_qp.get(iMb) as i64;
        }
        if kiMbNum > 0 {
            iTotalQp /= kiMbNum as i64;
        }
    } else {
        let mut iCorrectMbNum = 0i64;
        for iMb in 0..kiMbNum {
            let correct = if *pCurDq.grid.mb_correctly_decoded_flag.get(iMb) {
                1i64
            } else {
                0i64
            };
            iCorrectMbNum += correct;
            iTotalQp += (*pCurDq.grid.luma_qp.get(iMb) as i64) * correct;
        }
        if iCorrectMbNum == 0 {
            iTotalQp = pDecStat.iAvgLumaQp as i64;
        } else {
            iTotalQp /= iCorrectMbNum;
        }
    }

    if pDecStat.uiDecodedFrameCount == u32::MAX {
        ResetDecStatNums(pDecStat);
        pDecStat.iAvgLumaQp = iTotalQp as i32;
    } else {
        let count = pDecStat.uiDecodedFrameCount as i64;
        pDecStat.iAvgLumaQp =
            ((pDecStat.iAvgLumaQp as i64 * count + iTotalQp) / (count + 1)) as i32;
    }

    if pCurDq.sLayerInfo.sNalHeaderExt.bIdrFlag {
        if bIsComplete {
            pDecStat.uiIDRCorrectNum += 1;
        } else if !bEcDisabled {
            pDecStat.uiEcIDRNum += 1;
        }
    }
}

#[inline]
pub fn UpdateDecStat(pCtx: &mut SWelsDecoderContext, pCurDq: Option<&DqLayerState>, bOutput: bool) {
    {
        if (*pCtx).bFreezeOutput {
            if let Some(pCurDq) = pCurDq {
                {
                    let stat = &mut (*pCtx).pDecoderStatistics;
                    UpdateDecStatFreezingInfo(pCurDq.sLayerInfo.sNalHeaderExt.bIdrFlag, stat);
                }
            }
        } else if bOutput {
            { UpdateDecStatNoFreezingInfo(pCtx, pCurDq) };
        }
    }
}

#[inline]
pub fn WelsTargetSliceConstruction(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) -> i32 {
    {
        match pCurDqLayer {
            Some(dq) => { crate::decoder::decode_slice::WelsTargetSliceConstruction(pCtx, dq) },
            None => ERR_NONE,
        }
    }
}

#[inline]
pub fn WelsDecodeSlice(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
    bFreshSlice: bool,
    pCurNal: Option<usize>,
) -> i32 {
    {
        match pCurDqLayer {
            Some(dq) => {
                crate::decoder::decode_slice::WelsDecodeSlice(pCtx, dq, bFreshSlice, pCurNal)
            },
            None => ERR_NONE,
        }
    }
}

#[inline]
pub fn WelsDecodeAndConstructSlice(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) -> i32 {
    {
        match pCurDqLayer {
            Some(dq) => { crate::decoder::decode_slice::WelsDecodeAndConstructSlice(pCtx, dq) },
            None => ERR_NONE,
        }
    }
}

#[inline]
pub fn WelsInitRefList(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
    iPoc: i32,
) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitRefList(pCtx, pCurDqLayer, iPoc)
}

#[inline]
pub fn WelsInitBSliceRefList(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
    iPoc: i32,
) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitBSliceRefList(pCtx, pCurDqLayer, iPoc)
}

#[inline]
pub fn WelsReorderRefList(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) -> i32 {
    {
        crate::decoder::manage_dec_ref::WelsReorderRefList(pCtx, pCurDqLayer)
    }
}

#[inline]
pub fn WelsReorderRefList2(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) -> i32 {
    {
        crate::decoder::manage_dec_ref::WelsReorderRefList2(pCtx, pCurDqLayer)
    }
}

#[inline]
pub fn WelsMarkAsRef(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) -> i32 {
    {
        crate::decoder::manage_dec_ref::WelsMarkAsRef(pCtx, pCurDqLayer, None)
    }
}

// T4b.3b: a forwarding `ExpandReferencingPicture` stood here, taking the two
// function-pointer slots as an inline `extern "C"` type and handing them to
// `error_concealment`'s copy through **the crate's last two `mem::transmute`
// calls** -- one of them over an entire `[Option<fn>; 2]` array. All four
// `PExpandPictureFunc` typedefs in play were the same type,
// `unsafe extern "C" fn(*mut u8, i32, i32, i32)`, so the two calls reinterpreted
// a type into itself and bridged nothing at all. **The crate now contains zero
// such calls**; every remaining ratchet match, including this comment, is prose.
// Callers use `common/expand_pic.rs` directly.


#[inline]
pub fn ComputeColocatedTemporalScaling(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) {
    {
        if let Some(dq) = pCurDqLayer {
            // T5.Y2: a slice bracket of its own — the pool view and the context view
            // come out of the same context, and the two are disjoint by construction.
            let (pDec, pRefs, mut view, _nal) =
                crate::decoder::decoder_context::slice_split(pCtx, None);
            // The picture travels as a shared borrow: this scan only reads POCs, and
            // F42 lets a list entry name it (T5b.2).
            let pDec = pDec.map(|p| &*p);
            let _ = crate::decoder::decode_slice::ComputeColocatedTemporalScaling(
                &mut view,
                dq,
                pRefs,
                pDec,
            );
        }
    }
}

/// Adaptive picture-queue size, `pSps->iNumRefFrames + 2` (the extra two are
/// the EC MV copy exchange buffers).
/// Matches `GetTargetRefListSize` in `decoder.cpp`.
pub fn GetTargetRefListSize(pCtx: &mut SWelsDecoderContext) -> i32 {
    let kiNumRefFrames =
        active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps).map(|sps| sps.iNumRefFrames);
    let mut iNumRefFrames = match kiNumRefFrames {
        None => MAX_REF_PIC_COUNT as i32 + 2,
        Some(kiNumRefFrames) => {
            let iThreadCount = GetThreadCount(pCtx);
            if iThreadCount > 1 {
                // Thread and reordering buffering need more DPB space.
                MAX_DPB_COUNT as i32 + iThreadCount
            } else {
                kiNumRefFrames + 2
            }
        }
    };
    // LONG_TERM_REF: picture queue size is at least 2.
    if iNumRefFrames < 2 {
        iNumRefFrames = 2;
    }
    iNumRefFrames
}

pub fn SyncPictureResolutionExt(pCtx: &mut SWelsDecoderContext, iWidth: u32, iHeight: u32) -> i32 {
    {
        let iPicWidth = (iWidth << 4) as i32;
        let iPicHeight = (iHeight << 4) as i32;
        let iPicBufSize = GetTargetRefListSize(pCtx);
        (*pCtx).iPicQueueNumber = iPicBufSize;

        // **F77 (T8.C1): the resolution-change arm.** `WelsRequestMem`
        // (`decoder.cpp:464–545`) is what this function's first half is; the port had
        // only its *first-allocation* case, so the pool built for the opening sequence
        // survived every later one. `InitialDqLayersContext` below re-sizes the layer
        // from the new SPS unconditionally, so a mid-stream resolution change left the
        // **layer** at the new macroblock count and the **pictures** at the old one —
        // `res/Error_I_P.264` goes 352x288 → 640x480 → 352x288, and the first switch
        // put `WelsActualDecodeMbCavlcISlice` at `iMbXy` 396 in a 396-entry
        // `pDec.pMbType`. Through an `extern "C"` thunk that panic is a process abort,
        // which is plan §P13's line. The C++ logs the three transitions it makes here
        // ("memory re-alloc for resolution change, size change from 352 * 288 to
        // 640 * 480") and emits five frames; the port emitted two and died.
        //
        // The C's early return is `bHaveGotMemory && same size && !bNeedChangePicQueue`.
        // **T8b.C3 supplies its `bNeedChangePicQueue` half** —
        // `IncreasePicBuff`/`DecreasePicBuff`, F80/F87 — so all three of
        // `WelsRequestMem`'s arms are here now. `WelsResetRefPic` is the caller's
        // (`AllocPicBuffOnNewSeqBegin`, matching `decoder.cpp:489`'s placement relative
        // to this work), so the reference lists are already clear of the pool being
        // dropped or reordered here.
        let size_changed = (*pCtx).bHaveGotMemory
            && (iPicWidth != (*pCtx).iImgWidthInPixel || iPicHeight != (*pCtx).iImgHeightInPixel);
        if size_changed {
            // `decoder.cpp:518–528`: destroy, forget the DPB back-reference into the
            // pool being freed, rebuild at the new size. `.take()` is the C's
            // `ppPicBuf` out-parameter — it reads the pool and nulls the field in one
            // expression, the form `WelsFreeDynamicMemory` already uses.
            let pool = (*pCtx).pPicBuff.take();
            crate::decoder::pic_queue::DestroyPicBuff(pCtx, pool);
            (*pCtx).pLastDecPicInfo.pPreviousDecodedPictureInDpb = None;
        }

        if (*pCtx).pPicBuff.is_none() {
            // **T8.A8: the `pMemAlign` guard is gone with the allocator.** `CreatePicBuff`'s
            // C++ opens `if (NULL == pCtx || NULL == pCtx->pMemAlign) return 1;` — a
            // null test on the aligned allocator it is about to call. This port's pool
            // is a `Vec` and the only failure it can report is the `None` the `else`
            // arm below already takes, so the guard tested a field whose one remaining
            // job was to be non-null.
            let Some(pool) = crate::decoder::pic_queue::CreatePicBuff(
                crate::decoder::decoder_context::parse_only(&pCtx.pParam),
                iPicBufSize,
                iPicWidth,
                iPicHeight,
            ) else {
                return 1;
            };
            (*pCtx).pPicBuff = Some(pool);
            // `decoder.cpp:534–540`, and only on the arm that actually allocated:
            // the size the pictures were built at, and `pDec = NULL` because "need
            // prefetch a new pic due to spatial size changed" — the id the field holds
            // names a slot of the pool that was just dropped.
            (*pCtx).iImgWidthInPixel = iPicWidth;
            (*pCtx).iImgHeightInPixel = iPicHeight;
            (*pCtx).bHaveGotMemory = true;
            (*pCtx).pDec = None;
        } else {
            // **The third arm (F80/F87): same resolution, different queue size.**
            // `decoder.cpp:493-509`. A stream that changes `num_ref_frames` without
            // changing resolution lands here; the port had no branch for it and
            // returned `dsOutOfMemory` from the switch onward —
            // `tests/data/f80/num_ref_change_320x192.264` decoded 34 of 48 frames.
            //
            // The reference logs the transition, and that log line is ported with the
            // behaviour it describes rather than ahead of it.
            let capacity = pic_pool_mut(pCtx).map_or(0, |pool| pool.capacity());
            if capacity != iPicBufSize {
                WelsLog(
                    &mut (*pCtx).sLogCtx,
                    WELS_LOG_INFO,
                    &format!(
                        "WelsRequestMem(): memory re-alloc for no resolution change (size = {} * {}), ref list size change from {} to {}",
                        iPicWidth, iPicHeight, capacity, iPicBufSize
                    ),
                );
                let iErr = if capacity < iPicBufSize {
                    let parse_only =
                        crate::decoder::decoder_context::parse_only(&pCtx.pParam);
                    let Some(pool) = pCtx.pPicBuff.as_deref_mut() else {
                        return ERR_INFO_INVALID_PARAM;
                    };
                    crate::decoder::pic_queue::IncreasePicBuff(
                        pool,
                        parse_only,
                        capacity,
                        iPicWidth,
                        iPicHeight,
                        iPicBufSize,
                    )
                } else {
                    crate::decoder::pic_queue::DecreasePicBuff(
                        pCtx,
                        capacity,
                        iPicWidth,
                        iPicHeight,
                        iPicBufSize,
                    )
                };
                if iErr != ERR_NONE {
                    return iErr;
                }
                // `decoder.cpp:534-540`, which the C++ reaches from this arm as well
                // as from the reallocating one: the pool's pictures were built at
                // this size, and `pDec` names a slot whose occupant the resize may
                // have moved or dropped.
                (*pCtx).iImgWidthInPixel = iPicWidth;
                (*pCtx).iImgHeightInPixel = iPicHeight;
                (*pCtx).bHaveGotMemory = true;
                (*pCtx).pDec = None;
            }
            // Report the pool's real capacity, resized or not. The borrow ends before
            // the field write, which is the discipline every `pic_pool_mut` call site
            // keeps.
            let capacity = pic_pool_mut(pCtx).map_or(0, |pool| pool.capacity());
            (*pCtx).iPicQueueNumber = capacity;
        }
        let iErr = InitialDqLayersContext(pCtx, iPicWidth, iPicHeight);
        if iErr != ERR_NONE {
            return iErr;
        }
        ERR_NONE
    }
}

#[inline]
pub fn WelsResetRefPic(pCtx: &mut SWelsDecoderContext) {
    crate::decoder::manage_dec_ref::WelsResetRefPic(pCtx)
}

pub use crate::decoder::pic_queue::PrefetchLastPicForThread;

// `MemInitNalList` and `MemFreeNalList` were **duplicated** here, in a different
// shape from `nalu.rs`'s: three `WelsMallocz` blocks against one `alloc_zeroed`, and
// `MAX_NAL_UNIT_NUM_IN_AU` = 1024 against 32. This file's pair was the live one
// (`WelsInitStaticMemory`/`WelsFreeStaticMemory` call it), and `nalu.rs`'s
// `MemGetNextNal` grew *that* allocation through `nalu.rs`'s free — F39. T5.O4
// deleted the duplicate and unified the survivor over an owned `Vec<Box<SNalUnit>>`;
// **T5.P1 deleted the survivor too.** Once the context's field owns the access unit,
// the allocator is `SAccessUnit::with_nodes` and the deallocator is drop glue, so
// neither has a name to call and the F39 shape cannot be rewritten.

// **F43, T5.S1: three stubs stood here and shadowed `error_concealment.rs`.**
// `NeedErrorCon` returned `false`, `ImplementErrorCon` did nothing, and
// `MarkECFrameAsRef` returned `ERR_NONE` — declared in the same module that calls
// them, so a local item beat every path to the real bodies and the port's whole
// error-concealment subsystem was reachable only from its own unit tests. The
// imports below are what the call sites resolve to now; they are `use`s and not a
// glob on purpose, because the defect was name resolution and an explicit import
// is the form that shows which body runs.
use crate::decoder::error_concealment::{ImplementErrorCon, MarkECFrameAsRef, NeedErrorCon};

#[inline]
/// Matches `ResetActiveSPSForEachLayer` in `decoder_context.h`.
pub fn ResetActiveSPSForEachLayer(pCtx: &mut SWelsDecoderContext) {
    if (*pCtx).iTotalNumMbRec == 0 {
        for i in 0..MAX_LAYER_NUM {
            (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = None;
        }
    }
}

/// `decoder.cpp:716-724`. The three feedback fields `DECODER_OPTION_VCL_NAL`,
/// `DECODER_OPTION_TEMPORAL_ID` and `DECODER_OPTION_IS_REF_PIC` read, taken from the
/// access unit's **first** NAL (`uiStartPos`, not the last — the AU is ordered and the
/// base layer leads it).
///
/// **T8b.A3: this was `{}`.** Four phases of an empty body, and 21 of the 44 remaining
/// `test/api` rows behind it — `iFeedbackVclNalInAu` never left the caller's stack
/// garbage, `iFeedbackTidInAu` never left −1. `find_stub_bodies.py` could not see it
/// because it diffs *call sets* and this C++ body makes no calls (S46, closed in
/// T8b.A2's empty-body rule).
///
/// The reference has no null guard here — `pAccessUnitList` and its `pNalUnitsList`
/// entry are both live at the one call site (`decoder_core.cpp:2274`, right after
/// `WelsDecodeAccessUnitStart`). The port's accessors return `Option`, and a `None`
/// leaves the three fields at their per-call reset, which is what a caller reading
/// them before any AU already sees.
pub fn GetVclNalTemporalId(pCtx: &mut SWelsDecoderContext) {
    let Some(pAccessUnit) = cur_au(&mut pCtx.access_unit) else { return };
    let idx = pAccessUnit.uiStartPos as usize;
    let Some(nal) = pAccessUnit.node(idx) else { return };
    let uiTemporalId = nal.sNalHeaderExt.uiTemporalId;
    let uiNalRefIdc = nal.sNalHeaderExt.sNalUnitHeader.uiNalRefIdc;
    (*pCtx).iFeedbackVclNalInAu = FEEDBACK_VCL_NAL;
    (*pCtx).iFeedbackTidInAu = i32::from(uiTemporalId);
    (*pCtx).iFeedbackNalRefIdc = i32::from(uiNalRefIdc);
}

// **T8b.A3: `GetPrevFrameNum` deleted dead (S18).** It returned 0 — a stub of the
// multi-threaded detour's helper, which walks the other decoding contexts for the
// most recent frame number. This port's single call site reads
// `pLastDecPicInfo.iPrevFrameNum` directly (`:4356`), which is exactly what the
// reference's single-threaded path does. Fixing it would have been porting the
// threaded decoder; deleting it is what the plan says (D3).

// **T5b.9: `CopySpsPps` deleted dead (S18, D3).** The C++ calls it only from the
// threaded decoder (`decoder_core.cpp:2875`, `welsDecoderExt.cpp:1308`) — the
// subsystem this port stubbed out and the plan deletes. Here it was an empty body
// with zero callers, and the last thing in `src/decoder/` that spelled
// `PWelsDecoderContext` in code; the typedef goes with it.

// **F43, T5.S1: the other two stubs.** `FmoParamUpdate` returned `ERR_NONE`
// without building a map and `FmoNextMb` returned `iMbIdx + 1` — raster order, the
// answer FMO exists to *not* give — and both took the second `SFmo` deleted at the
// top of this file, so the one call site cast the context's real `TagFmo` to a
// structurally different type to reach them. `fmo.rs` is the resolution now.
use crate::decoder::fmo::{FmoNextMb, FmoParamUpdate};

// **F52 (T5.X7): a sixth F43-class stub stood here** — `CheckAccessUnitBoundaryExt`
// returning `true` unconditionally, in the module that calls it, shadowing the real
// 80-line implementation in `nalu.rs` (which had **no caller at all**). A local item
// beats every import, so `CheckAndFinishLastPic`'s access-unit boundary test read
// `iTotalNumMbRec != 0 && true` where the C++ compares fifteen header fields.
//
// The F43 sweep (`tools/find_shadowing_stubs.py`) reported clean over this: it counts
// a function's statements as the lines between the `fn` line and the closing brace,
// so a **multi-line signature** is counted as statements and a four-parameter stub
// scores 6 — `SUBSTANTIAL`, not `TRIVIAL`. The instrument is fixed with the finding
// (S33's corollary: an instrument that disagrees with a hand count is wrong until
// proven otherwise). Callers use `nalu.rs` directly.

// Core Functions Implemented in `decoder_core.cpp`
pub fn DecodeFrameConstruction(
    pCtx: &mut SWelsDecoderContext,
    pCurDq: Option<&DqLayerState>,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) -> i32 {
    let Some(pCurDq) = pCurDq else {
        return ERR_INFO_INVALID_PTR;
    };
    // **T5.Z4: the picture is re-derived where it is written, not held.** T5.Q2 took
    // one derivation for the whole function because a second one would invalidate the
    // first; with the context a `&mut` the *statistics* calls in between take it too,
    // so the borrow cannot span them. `pCtx->pDec` is not written anywhere in here,
    // so every re-derivation names the same slot — which is what made one derivation
    // safe in the first place, said the other way round.
    if dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec).is_none() {
        return ERR_INFO_INVALID_PTR;
    }
    macro_rules! pic {
        () => {
            dec_pic(&mut pCtx.pPicBuff, pCtx.pDec).unwrap()
        };
    }

    let kiWidth = pCurDq.iMbWidth << 4;
    let kiHeight = pCurDq.iMbHeight << 4;
    let kiTotalNumMbInCurLayer = pCurDq.iMbWidth * pCurDq.iMbHeight;
    let mut bFrameCompleteFlag = true;

    if (*pCtx).bNewSeqBegin {
        // A value: the destination is another field of the same context (T5.Z1).
        let sFrameCrop = sps_of(
            &(*pCtx).sSpsPpsCtx,
            pCurDq.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.sps_ref,
        )
        .map(|sps| sps.sFrameCrop);
        if let Some(sFrameCrop) = sFrameCrop {
            (*pCtx).sFrameCrop = sFrameCrop;
        }
        // **F46, T5.T2 — the wrong side of the `#ifdef`.** `LONG_TERM_REF` is defined
        // (`decoder_context.h:67`), so `decoder_core.cpp:60` clears
        // **`bParamSetsLostFlag`** here and leaves `bReferenceLostAtT0Flag` alone; the
        // port took the `#else` line. It cost nothing while nothing read the flag —
        // `bReferenceLostAtT0Flag` has no reader in either tree under this `#ifdef` —
        // and it costs a return code now that `UpdateAccessUnit`'s mosaic-avoidance
        // block reads `bParamSetsLostFlag`: without the clear the port would raise
        // `dsRefLost` on every non-IDR access unit for the life of the decoder, which
        // is not "references lost", it is "we have never seen a key frame".
        (*pCtx).bParamSetsLostFlag = false;
        if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
            (*pCtx).bPrintFrameErrorTraceFlag = true;
            (*pCtx).iIgnoredErrorInfoPacketCount = 0;
        }
    }

    let kiActualWidth = kiWidth - ((*pCtx).sFrameCrop.iLeftOffset + (*pCtx).sFrameCrop.iRightOffset) * 2;
    let kiActualHeight = kiHeight - ((*pCtx).sFrameCrop.iTopOffset + (*pCtx).sFrameCrop.iBottomOffset) * 2;

    if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
        {
            let stat = &mut (*pCtx).pDecoderStatistics;
            if stat.uiWidth != kiActualWidth as u32
                || stat.uiHeight != kiActualHeight as u32
            {
                stat.uiResolutionChangeTimes += 1;
                stat.uiWidth = kiActualWidth as u32;
                stat.uiHeight = kiActualHeight as u32;
            }
        }
        UpdateDecStatNoFreezingInfo(pCtx, Some(pCurDq));
    }

    if (*pCtx).pParam.bParseOnly {
        if (*pCtx).iErrorCode == dsErrorFree {
            // The cropped dimensions as values, above the descriptor's borrow: the
            // parse-only descriptor is the context's own `Box` and the SPS is the
            // context's array, so the two cannot be borrowed through it at once
            // once the context is a reference (T5.Z1).
            let sps_dims = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps).map(|sps| {
                (
                    (sps.iMbWidth as i32) * 16
                        - ((sps.sFrameCrop.iLeftOffset + sps.sFrameCrop.iRightOffset) << 1),
                    (sps.iMbHeight as i32) * 16
                        - ((sps.sFrameCrop.iTopOffset + sps.sFrameCrop.iBottomOffset) << 1),
                )
            });
            // ------------------------------------------------------------------
            // **T8b.B2 — the byte copies.** Until now this arm kept the reference's
            // length bookkeeping and *none* of its `memcpy`s: T3.3 deleted them with
            // `pNalPos`, correctly for that tree (nothing wrote the field), and the
            // note that stood here said the next writer of `pDstBuff` would need the
            // running total. This is that writer. `decoder_core.cpp:88-175`, whole:
            // the IDR SPS/PPS prepend, the two capacity checks, `ExpandBsLenBuffer`,
            // and the per-NAL copy out of `sSavedData`.
            // ------------------------------------------------------------------

            // Everything `ExpandBsLenBuffer` needs is read here, *above* the
            // destructure: it takes the whole context, and the copies below hold
            // three of its fields at once. The reference interleaves the two checks
            // with the copies; hoisting them is the same growth on the same inputs,
            // because both are functions of `iNalNum` and the access unit's index
            // range and neither copy changes those.
            let (iIdx0, iEndIdx0) = match cur_au(&mut pCtx.access_unit) {
                Some(au) => (au.uiStartPos as i32, au.uiEndPos as i32),
                None => (0, -1),
            };
            let (bIdrFlag, bSubSps) = match cur_au(&mut pCtx.access_unit)
                .and_then(|au| au.node(iIdx0 as usize))
            {
                Some(nal) => (
                    nal.sNalHeaderExt.bIdrFlag,
                    nal.sNalHeaderExt.sNalUnitHeader.eNalUnitType
                        == crate::decoder::nalu::EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT,
                ),
                None => (false, false),
            };
            let bDoPrepend = bIdrFlag && (*pCtx).bFrameFinish;
            let iSpsId = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                .map(|sps| sps.iSpsId)
                .unwrap_or(-1);
            let iPpsId = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
                .map(|pps| pps.iPpsId)
                .unwrap_or(-1);
            let mut iNalNumAfter = parser_bs(&mut (*pCtx).pParserBsInfo)
                .map(|p| p.iNalNum)
                .unwrap_or(0);
            let iMaxNalNum = |pCtx: &SWelsDecoderContext| -> i32 {
                pCtx.pParserBsInfo
                    .as_deref()
                    .map(|p| p.pNalLenInByte.len() as i32)
                    .unwrap_or(0)
            };
            if bDoPrepend {
                // "2 reserved for sps+pps" — `decoder_core.cpp:113`.
                if iNalNumAfter > iMaxNalNum(pCtx) - 2 {
                    WelsLog(
                        std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                        WELS_LOG_INFO,
                        &format!(
                            "DecodeFrameConstruction(): current NAL num ({}) plus sps & pps exceeds permitted num ({}). Will expand",
                            iNalNumAfter,
                            iMaxNalNum(pCtx)
                        ),
                    );
                    if ExpandBsLenBuffer(pCtx, iNalNumAfter + 2) != ERR_NONE {
                        return ERR_INFO_OUT_OF_MEMORY;
                    }
                }
                iNalNumAfter += 2;
            }
            let iNalTotal = iNalNumAfter + iEndIdx0 - iIdx0 + 1;
            if iNalTotal > iMaxNalNum(pCtx) {
                WelsLog(
                    std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                    WELS_LOG_INFO,
                    &format!(
                        "DecodeFrameConstruction(): current NAL num ({}) exceeds permitted num ({}). Will expand",
                        iNalTotal,
                        iMaxNalNum(pCtx)
                    ),
                );
                if ExpandBsLenBuffer(pCtx, iNalTotal) != ERR_NONE {
                    return ERR_INFO_OUT_OF_MEMORY;
                }
            }

            // Five field borrows, disjoint: the access unit is `access_unit`'s own
            // allocation, the descriptor is `pParserBsInfo`'s (T5.Z3), and the three
            // sources — the saved EBSP and the two parameter-set caches — are their
            // own arrays. Nothing in this block calls back into the context, so one
            // derivation of each covers it; the borrow's extent is the check, not a
            // style choice.
            let SWelsDecoderContext {
                access_unit,
                pParserBsInfo,
                sSavedData,
                sSpsBsInfo,
                sSubsetSpsBsInfo,
                sPpsBsInfo,
                bFrameFinish,
                bParamSetsLostFlag,
                iErrorCode,
                ..
            } = &mut *pCtx;
            if let (Some(pCurAu), Some(pParser)) =
                (access_unit.as_deref_mut(), parser_bs(pParserBsInfo))
            {
                let mut iTotalNalLen: i32 = 0;
                for i in 0..(*pParser).iNalNum {
                    if let Some(len) = (*pParser).pNalLenInByte.as_slice().get(i as usize) {
                        iTotalNalLen += *len;
                    }
                }
                // `uint8_t* pDstBuf = pParser->pDstBuff + iTotalNalLen;` as an offset.
                let mut iDstPos = iTotalNalLen.max(0) as usize;
                let mut iIdx = pCurAu.uiStartPos as i32;
                let iEndIdx = pCurAu.uiEndPos as i32;
                if let Some(nal) = pCurAu.node(iIdx as usize) {
                    (*pParser).uiOutBsTimeStamp = nal.uiTimeStamp;
                }
                if let Some((w, h)) = sps_dims {
                    (*pParser).iSpsWidthInPixel = w;
                    (*pParser).iSpsHeightInPixel = h;
                }

                // `decoder_core.cpp:110-140` — an IDR that opens a frame gets the
                // active SPS and PPS written in front of it, from the caches
                // `ParseSps`/`ParsePps` fill, whether or not the source stream
                // repeated them. This is what makes the parse-only output
                // independently decodable.
                if bDoPrepend {
                    *bParamSetsLostFlag = false;
                    let sps_row = if bSubSps {
                        sSubsetSpsBsInfo.get(iSpsId.max(0) as usize)
                    } else {
                        sSpsBsInfo.get(iSpsId.max(0) as usize)
                    };
                    let pps_row = sPpsBsInfo.get(iPpsId.max(0) as usize);
                    if let (Some(sps_row), Some(pps_row)) = (sps_row, pps_row) {
                        let kSpsLen = sps_row.uiSpsBsLen as usize;
                        let kPpsLen = pps_row.uiPpsBsLen as usize;
                        if iDstPos + kSpsLen + kPpsLen >= MAX_ACCESS_UNIT_CAPACITY {
                            *iErrorCode |= dsOutOfMemory;
                            (*pParser).iNalNum = 0;
                            return ERR_INFO_OUT_OF_MEMORY;
                        }
                        (*pParser).pDstBuff[iDstPos..iDstPos + kSpsLen]
                            .copy_from_slice(&sps_row.pSpsBsBuf[..kSpsLen]);
                        let iSlot = (*pParser).iNalNum as usize;
                        if let Some(slot) = (*pParser).pNalLenInByte.get_mut(iSlot) {
                            *slot = kSpsLen as i32;
                            (*pParser).iNalNum += 1;
                        }
                        iDstPos += kSpsLen;
                        (*pParser).pDstBuff[iDstPos..iDstPos + kPpsLen]
                            .copy_from_slice(&pps_row.pPpsBsBuf[..kPpsLen]);
                        let iSlot = (*pParser).iNalNum as usize;
                        if let Some(slot) = (*pParser).pNalLenInByte.get_mut(iSlot) {
                            *slot = kPpsLen as i32;
                            (*pParser).iNalNum += 1;
                        }
                        iDstPos += kPpsLen;
                    }
                    *bFrameFinish = false;
                }

                while iIdx <= iEndIdx {
                    let pCurNal = pCurAu.node(iIdx as usize);
                    if let Some(pCurNal) = pCurNal {
                        let iNalLen = pCurNal.sNalData.sVclNal.iNalLength;
                        let iNalPos = pCurNal.sNalData.sVclNal.iNalPos;
                        let iSlot = (*pParser).iNalNum as usize;
                        let lens = (*pParser).pNalLenInByte.as_mut_slice();
                        if let Some(slot) = lens.get_mut(iSlot) {
                            *slot = iNalLen;
                            (*pParser).iNalNum += 1;
                        }
                        // `decoder_core.cpp:155-172` — the copy the port has been
                        // missing. The source is `sSavedData`, the **EBSP** copy
                        // `ParseNalHeader` made; `sRawData` holds the de-escaped RBSP
                        // and is the wrong bytes to hand out.
                        if iDstPos + iNalLen.max(0) as usize >= MAX_ACCESS_UNIT_CAPACITY {
                            *iErrorCode |= dsOutOfMemory;
                            (*pParser).iNalNum = 0;
                            return ERR_INFO_OUT_OF_MEMORY;
                        }
                        if iNalLen > 0 {
                            let kLen = iNalLen as usize;
                            if let Some(src) = sSavedData.bytes().get(iNalPos..iNalPos + kLen) {
                                (*pParser).pDstBuff[iDstPos..iDstPos + kLen].copy_from_slice(src);
                                iDstPos += kLen;
                            }
                        }
                    }
                    iIdx += 1;
                }

                if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
                    (*pCtx).iTotalNumMbRec = 0;
                    (*pCtx).bFramePending = false;
                    (*pCtx).bFrameFinish = true;
                } else if (*pCtx).iTotalNumMbRec != 0 {
                    (*pCtx).bFramePending = true;
                    pic!().bIsComplete = false;
                    (*pCtx).bFrameFinish = false;
                    (*pCtx).iErrorCode |= dsFramePending;
                    return ERR_INFO_PARSEONLY_PENDING;
                }
            }
        } else {
            if let Some(pParser) = parser_bs(&mut (*pCtx).pParserBsInfo) {
                pParser.uiOutBsTimeStamp = 0;
                pParser.iNalNum = 0;
                pParser.iSpsWidthInPixel = 0;
                pParser.iSpsHeightInPixel = 0;
            }
            return ERR_INFO_PARSEONLY_ERROR;
        }
        return ERR_NONE;
    }

    if (*pCtx).iTotalNumMbRec != kiTotalNumMbInCurLayer {
        bFrameCompleteFlag = false;
        if (*pCtx).bInstantDecFlag {
            return ERR_INFO_MB_NUM_INADEQUATE;
        }
    } else if pCurDq.sLayerInfo.sNalHeaderExt.bIdrFlag && (*pCtx).iErrorCode == dsErrorFree {
        // T5.Q2: `dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec)` stood here and at the parse-only arm above. Both
        // name the picture `pPic` already holds — `pCtx->pDec` is not written
        // anywhere in this function — and under owned slots a second derivation of
        // one slot invalidates the first, which `pPic`'s twenty-odd uses below are.
        // One derivation at the top is what this whole function's region wants.
        pic!().bIsComplete = true;
        (*pCtx).bFreezeOutput = false;
    }

    (*pCtx).iTotalNumMbRec = 0;

    pDstInfo.uiOutYuvTimeStamp = pic!().uiTimeStamp;
    ppDst[0] = pic!().data_ptr(0);
    ppDst[1] = pic!().data_ptr(1);
    ppDst[2] = pic!().data_ptr(2);

    pDstInfo.UsrData.sys_mut().iFormat = videoFormatI420;
    pDstInfo.UsrData.sys_mut().iWidth = kiActualWidth;
    pDstInfo.UsrData.sys_mut().iHeight = kiActualHeight;
    pDstInfo.UsrData.sys_mut().iStride[0] = pic!().linesize(0);
    pDstInfo.UsrData.sys_mut().iStride[1] = pic!().linesize(1);

    if !(ppDst[0]).is_null() {
        ppDst[0] = ppDst[0].wrapping_add(
            ((*pCtx).sFrameCrop.iTopOffset * 2 * pic!().linesize(0) + (*pCtx).sFrameCrop.iLeftOffset * 2) as usize
        );
    }
    if !(ppDst[1]).is_null() {
        ppDst[1] = ppDst[1].wrapping_add(
            ((*pCtx).sFrameCrop.iTopOffset * pic!().linesize(1) + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }
    if !(ppDst[2]).is_null() {
        ppDst[2] = ppDst[2].wrapping_add(
            ((*pCtx).sFrameCrop.iTopOffset * pic!().linesize(1) + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }

    for i in 0..3 {
        pDstInfo.pDst[i] = ppDst[i];
    }
    pDstInfo.iBufferStatus = 1;

    let bOutResChange = (*pCtx).iLastImgWidthInPixel != pDstInfo.UsrData.sys().iWidth
        || (*pCtx).iLastImgHeightInPixel != pDstInfo.UsrData.sys().iHeight;
    (*pCtx).iLastImgWidthInPixel = pDstInfo.UsrData.sys().iWidth;
    (*pCtx).iLastImgHeightInPixel = pDstInfo.UsrData.sys().iHeight;

    if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
        pDstInfo.iBufferStatus = (bFrameCompleteFlag && pic!().bIsComplete) as i32;
    } else if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        || (*pCtx).pParam.eEcActiveIdc == ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE
        && (*pCtx).iErrorCode != dsErrorFree
        && bOutResChange
    {
        (*pCtx).bFreezeOutput = true;
    }

    if pDstInfo.iBufferStatus == 0 {
        if !bFrameCompleteFlag {
            (*pCtx).iErrorCode |= dsBitstreamError;
        }
        return ERR_INFO_MB_NUM_INADEQUATE;
    }

    if (*pCtx).bFreezeOutput {
        pDstInfo.iBufferStatus = 0;
    }

    (*pCtx).iMbEcedNum = pic!().iMbEcedNum;
    (*pCtx).iMbNum = pic!().iMbNum;
    (*pCtx).iMbEcedPropNum = pic!().iMbEcedPropNum;

    if (*pCtx).pParam.eEcActiveIdc != ERROR_CON_DISABLE {
        if pDstInfo.iBufferStatus != 0 {
            {
                let stat = &mut (*pCtx).pDecoderStatistics;
                if stat.uiWidth != kiActualWidth as u32 || stat.uiHeight != kiActualHeight as u32 {
                    stat.uiResolutionChangeTimes += 1;
                    stat.uiWidth = kiActualWidth as u32;
                    stat.uiHeight = kiActualHeight as u32;
                }
            }
        }
        UpdateDecStat(pCtx, Some(pCurDq), pDstInfo.iBufferStatus != 0);
    }

    ERR_NONE
}

#[inline]
pub fn CheckSliceNeedReconstruct(uiLayerDqId: u8, uiTargetDqId: u8) -> bool {
    uiLayerDqId == uiTargetDqId
}

#[inline]
pub fn GetTargetDqId(uiTargetDqId: u8, psParam: &SDecodingParam) -> u8 {
    // T8.A5: the parameter block is the context's own field (F41), so the `Option`
    // and its 255 null-default are gone — the C's `pCtx->pParam->uiTargetDqLayer`
    // is a field read, exactly as `decoder_core.cpp:1049` spells it.
    WELS_MIN(uiTargetDqId, psParam.uiTargetDqLayer)
}

/// The header of NAL `i` of the access unit under construction, or `None` — the
/// `!pNalCur.is_null()` test, once (T5b.3).
#[inline]
fn nal_hdr(pCtx: &SWelsDecoderContext, i: Option<usize>) -> Option<&SNalUnitHeaderExt> {
    let i = i?;
    pCtx.access_unit
        .as_deref()
        .and_then(|au| au.node(i))
        .map(|nal| &nal.sNalHeaderExt)
}

#[inline]
pub fn HandleReferenceLostL0(pCtx: &mut SWelsDecoderContext, pCurNal: Option<&SNalUnitHeaderExt>) {
    {
        if pCurNal.is_some_and(|h| h.uiTemporalId == 0) {
            (*pCtx).bReferenceLostAtT0Flag = true;
        }
        (*pCtx).iErrorCode |= dsBitstreamError;
    }
}

#[inline]
pub fn HandleReferenceLost(pCtx: &mut SWelsDecoderContext, pCurNal: Option<&SNalUnitHeaderExt>) {
    {
        if pCurNal
            .is_some_and(|h| h.uiTemporalId == 0 || h.uiTemporalId == 1)
        {
            (*pCtx).bReferenceLostAtT0Flag = true;
        }
        (*pCtx).iErrorCode |= dsRefLost;
    }
}

#[inline]
pub fn WelsDecodeConstructSlice(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
    pCurNal: Option<usize>,
) -> i32 {
    {
        let iRet = WelsTargetSliceConstruction(pCtx, pCurDqLayer);
        if iRet != ERR_NONE {
            let h = nal_hdr(pCtx, pCurNal).copied();
            HandleReferenceLostL0(pCtx, h.as_ref());
        }
        iRet
    }
}

pub fn ParsePredWeightedTable(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
    pSh: &mut SSliceHeader,
) -> i32 {
    {
        // The offset travels, not the slice — see `ParseSps` (T5.Z4).
        let buf = pCtx.sRawData.window_from(kiRbspStart);
        let mut uiCode: u32 = 0;
        let mut iCode: i32 = 0;

        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > 7 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_LOG2_WEIGHT_DENOM);
        }
        pSh.sPredWeightTable.uiLumaLog2WeightDenom = uiCode;

        // `Option`, not a defaulted scalar: the two arms below distinguish "no SPS" from
        // "monochrome SPS" and they distinguish them *in opposite directions* — the
        // first parses nothing without an SPS, the second parses chroma weights
        // *because* there is none. A `map_or(0, …)` collapses them and desynchronises
        // the slice header (T5.Z1).
        let uiChromaArrayType =
            sps_of(&(*pCtx).sSpsPpsCtx, pSh.sps_ref).map(|sps| sps.uiChromaArrayType);

        if uiChromaArrayType.is_some_and(|t| t != 0) {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode > 7 {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_LOG2_WEIGHT_DENOM);
            }
            pSh.sPredWeightTable.uiChromaLog2WeightDenom = uiCode;
        }

        if (pSh.sPredWeightTable.uiLumaLog2WeightDenom | pSh.sPredWeightTable.uiChromaLog2WeightDenom) > 7 {
            return ERR_NONE;
        }

        let mut iList = 0;
        while iList < LIST_A {
            for i in 0..(pSh.uiRefCount[iList] as usize) {
                if i >= MAX_REF_PIC_COUNT {
                    break;
                }
                if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if uiCode != 0 {
                    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if iCode < -128 || iCode > 127 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_WEIGHT);
                    }
                    pSh.sPredWeightTable.sPredList[iList].iLumaWeight[i] = iCode;

                    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if iCode < -128 || iCode > 127 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_OFFSET);
                    }
                    pSh.sPredWeightTable.sPredList[iList].iLumaOffset[i] = iCode;
                } else {
                    pSh.sPredWeightTable.sPredList[iList].iLumaWeight[i] =
                        1 << pSh.sPredWeightTable.uiLumaLog2WeightDenom;
                    pSh.sPredWeightTable.sPredList[iList].iLumaOffset[i] = 0;
                }

                if uiChromaArrayType == Some(0) {
                    continue;
                }

                if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if uiCode != 0 {
                    for j in 0..2 {
                        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        if iCode < -128 || iCode > 127 {
                            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_WEIGHT);
                        }
                        pSh.sPredWeightTable.sPredList[iList].iChromaWeight[i][j] = iCode;

                        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        if iCode < -128 || iCode > 127 {
                            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_OFFSET);
                        }
                        pSh.sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = iCode;
                    }
                } else {
                    for j in 0..2 {
                        pSh.sPredWeightTable.sPredList[iList].iChromaWeight[i][j] =
                            1 << pSh.sPredWeightTable.uiChromaLog2WeightDenom;
                        pSh.sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = 0;
                    }
                }
            }
            iList += 1;
            if pSh.eSliceType != B_SLICE {
                break;
            }
        }
        ERR_NONE
    }
}

pub fn CreateImplicitWeightTable(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&mut DqLayerState>,
) {
    let Some(pCurDqLayer) = pCurDqLayer else {
        return;
    };
    // **The header's three reads travel as values** (T5.Z1's shape, one container
    // in): `pSliceHeader` was a `&mut` *into* the layer held across writes to the
    // layer itself — legal only while the layer arrived as a raw pointer. Nothing
    // below writes the header, so the reads are copies and the writes keep the
    // borrow.
    let (pps_id, iPoc, uiRefCount) = {
        let pSliceHeader = &pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
        (
            pSliceHeader.pps_id,
            pSliceHeader.iPicOrderCntLsb,
            pSliceHeader.uiRefCount,
        )
    };
    // A value: the pool bracket below reaches the context for every reference it
    // resolves (T5.Z1).
    let Some(uiWeightedBipredIdc) =
        pps_of(&(*pCtx).sSpsPpsCtx, pps_id).map(|pps| pps.uiWeightedBipredIdc)
    else {
        return;
    };

    if pCurDqLayer.bUseWeightedBiPredIdc && uiWeightedBipredIdc == 2 {
        let ref0 = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, 0);
        let ref1 = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, 0);
        if let (Some(ref0), Some(ref1)) = (ref0, ref1) {
            if uiRefCount[0] == 1
                && uiRefCount[1] == 1
                && (ref0.iFramePoc as i64 + ref1.iFramePoc as i64 == 2 * (iPoc as i64))
            {
                pCurDqLayer.bUseWeightedBiPredIdc = false;
                return;
            }
        }

        // T5.AC2: the table is the layer's own now, so the null test is the
        // `Option` and the writes below are field writes. The `&mut` is taken once
        // for the whole block and every reference resolution inside it reads the
        // *context*'s pool — a disjoint object, which is why the borrow can span
        // them (the layer arrives as its own `&mut` parameter).
        if let Some(pred_weight_table) = pCurDqLayer.sPredWeightTable.as_mut() {
            pred_weight_table.uiLumaLog2WeightDenom = 5;
            pred_weight_table.uiChromaLog2WeightDenom = 5;
            for iRef0 in 0..(uiRefCount[0] as usize) {
                // The two poc/long-ref pairs are read as **values**, so the
                // reference borrow ends before the inner loop resolves the second
                // list — two live shared results of one pool are legal, but the
                // values make the reach obvious (T5.Z1).
                let ref0_poc = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_0, iRef0)
                    .map(|p| (p.iFramePoc, p.bIsLongRef));
                if let Some((iPoc0, bIsLongRef0)) = ref0_poc {
                    for iRef1 in 0..(uiRefCount[1] as usize) {
                        let ref1_poc = ref_pic(&(*pCtx).pPicBuff, &(*pCtx).sRefPic, LIST_1, iRef1)
                            .map(|p| (p.iFramePoc, p.bIsLongRef));
                        if let Some((iPoc1, bIsLongRef1)) = ref1_poc {
                            pred_weight_table.iImplicitWeight[iRef0][iRef1] = 32;
                            if !bIsLongRef0 && !bIsLongRef1 {
                                let iTd = WELS_CLIP3(iPoc1 - iPoc0, -128, 127);
                                if iTd != 0 {
                                    let iTb = WELS_CLIP3(iPoc - iPoc0, -128, 127);
                                    let iTx = (16384 + (WELS_ABS(iTd) >> 1)) / iTd;
                                    let iDistScaleFactor = (iTb * iTx + 32) >> 8;
                                    if iDistScaleFactor >= -64 && iDistScaleFactor <= 128 {
                                        pred_weight_table.iImplicitWeight[iRef0][iRef1] =
                                            64 - iDistScaleFactor;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn ParseRefPicListReordering(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
    pSh: &mut SSliceHeader,
) -> i32 {
    {
        // The offset travels, not the slice — see `ParseSps` (T5.Z4).
        let buf = pCtx.sRawData.window_from(kiRbspStart);
        let keSt = pSh.eSliceType;
        if keSt == I_SLICE || keSt == SI_SLICE {
            return ERR_NONE;
        }
        let pRefPicListReordering = &mut pSh.pRefPicListReordering;
        let Some(pSps) = sps_of(&(*pCtx).sSpsPpsCtx, pSh.sps_ref) else {
            return ERR_INFO_INVALID_PTR;
        };

        let mut iList = 0;
        let mut uiCode: u32 = 0;
        loop {
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            pRefPicListReordering.bRefPicListReorderingFlag[iList] = uiCode != 0;

            if pRefPicListReordering.bRefPicListReorderingFlag[iList] {
                let mut iIdx = 0;
                loop {
                    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    let kuiIdc = uiCode;
                    if (iIdx >= MAX_REF_PIC_COUNT && kuiIdc != 3) || kuiIdc > 3 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                    }
                    pRefPicListReordering.sReorderingSyn[iList][iIdx].uiReorderingOfPicNumsIdc = kuiIdc as _;

                    if kuiIdc == 3 {
                        break;
                    }
                    if iIdx >= pSh.uiRefCount[iList] as usize || iIdx >= MAX_REF_PIC_COUNT {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                    }
                    if kuiIdc == 0 || kuiIdc == 1 {
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        if uiCode >= (1u32 << (*pSps).uiLog2MaxFrameNum) {
                            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                        }
                        pRefPicListReordering.sReorderingSyn[iList][iIdx].uiAbsDiffPicNumMinus1 = uiCode;
                    } else if kuiIdc == 2 {
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        pRefPicListReordering.sReorderingSyn[iList][iIdx].uiLongTermPicNum = uiCode as u16;

                    }
                    iIdx += 1;
                }
            }
            if keSt != B_SLICE {
                break;
            }
            iList += 1;
            if iList >= LIST_A {
                break;
            }
        }
        ERR_NONE
    }
}

pub fn ParseDecRefPicMarking(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
    pSh: &mut SSliceHeader,
    sps_ref: Option<SpsRef>,
    kbIdrFlag: bool,
) -> i32 {
    {
        // The offset travels, not the slice — see `ParseSps` (T5.Z4).
        let buf = pCtx.sRawData.window_from(kiRbspStart);
        // **And the SPS travels as a ref, not a pointer.** Its two scalars are read out
        // here, above every use of the context: derived from `pCtx` and passed beside
        // it, the pointer dies at this function's own entry retag — Y's verdict, arriving
        // from a *caller-derived* alias rather than from an accessor.
        let Some((uiLog2MaxFrameNum, iNumRefFrames)) = sps_of(&pCtx.sSpsPpsCtx, sps_ref)
            .map(|sps| (sps.uiLog2MaxFrameNum, sps.iNumRefFrames))
        else {
            return ERR_INFO_INVALID_PTR;
        };
        let kpRefMarking = &mut pSh.sRefMarking;
        let mut uiCode: u32 = 0;

        if kbIdrFlag {
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            kpRefMarking.bNoOutputOfPriorPicsFlag = uiCode != 0;
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            kpRefMarking.bLongTermRefFlag = uiCode != 0;
        } else {
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            kpRefMarking.bAdaptiveRefPicMarkingModeFlag = uiCode != 0;
            if kpRefMarking.bAdaptiveRefPicMarkingModeFlag {
                let mut iIdx = 0;
                let mut bAllowMmco5 = true;
                let mut bMmco4Exist = false;
                let mut bMmco5Exist = false;
                let mut bMmco6Exist = false;

                while iIdx < MAX_MMCO_COUNT {
                    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    let kuiMmco = uiCode;
                    kpRefMarking.sMmcoRef[iIdx].uiMmcoType = kuiMmco;
                    if kuiMmco == MMCO_END {
                        break;
                    }
                    if kuiMmco == MMCO_SHORT2UNUSED || kuiMmco == MMCO_SHORT2LONG {
                        bAllowMmco5 = false;
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        kpRefMarking.sMmcoRef[iIdx].iDiffOfPicNum = 1 + (uiCode as i32);
                        kpRefMarking.sMmcoRef[iIdx].iShortFrameNum = (pSh.iFrameNum
                            - kpRefMarking.sMmcoRef[iIdx].iDiffOfPicNum)
                            & (((1 << uiLog2MaxFrameNum) - 1) as i32);
                    } else if kuiMmco == MMCO_LONG2UNUSED {
                        bAllowMmco5 = false;
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        kpRefMarking.sMmcoRef[iIdx].uiLongTermPicNum = uiCode;
                    }
                    if kuiMmco == MMCO_SHORT2LONG || kuiMmco == MMCO_LONG {
                        if kuiMmco == MMCO_LONG {
                            if bMmco6Exist {
                                return -1;
                            }
                            bMmco6Exist = true;
                        }
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        kpRefMarking.sMmcoRef[iIdx].iLongTermFrameIdx = uiCode as i32;
                    } else if kuiMmco == MMCO_SET_MAX_LONG {
                        if bMmco4Exist {
                            return -1;
                        }
                        bMmco4Exist = true;
                        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        let iMaxLongTermFrameIdx = -1 + (uiCode as i32);
                        if iMaxLongTermFrameIdx > iNumRefFrames {
                            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_MARKING);
                        }
                        kpRefMarking.sMmcoRef[iIdx].iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
                    } else if kuiMmco == MMCO_RESET {
                        if !bAllowMmco5 || bMmco5Exist {
                            return -1;
                        }
                        bMmco5Exist = true;
                        {
                            let info = &mut pCtx.pLastDecPicInfo;
                            info.iPrevPicOrderCntLsb = 0;
                            info.iPrevPicOrderCntMsb = 0;
                        }
                        pSh.iPicOrderCntLsb = 0;
                        // **T5b.3: through the index, not a stored alias.**
                        // `pCtx->pSliceHeader` was a raw pointer *into* the current
                        // NAL — the second of the parse tree's two stored aliases, and
                        // the one that escaped furthest (it outlived the call that set
                        // it by a whole slice decode). It is deleted; the NAL under
                        // decode is `pCtx->nal_cur`, and this — its only reader —
                        // resolves it. `pSh` above is the *layer's* copy and is a
                        // different object, which is why both writes are here.
                        let nal_cur = (*pCtx).slice_hdr_nal;
                        if let Some(nal) = nal_cur
                            .and_then(|i| cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(i)))
                        {
                            nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb = 0;
                        }
                    }
                    iIdx += 1;
                }
            }
        }
        ERR_NONE
    }
}

pub fn FillDefaultSliceHeaderExt(
    pShExt: &mut SSliceHeaderExt,
    pNalExt: &SNalUnitHeaderExt,
) -> bool {
    {
        if pNalExt.bNoInterLayerPredFlag || pNalExt.uiQualityId > 0 {

            pShExt.bBasePredWeightTableFlag = false;
        } else {
            pShExt.bBasePredWeightTableFlag = true;
        }
        pShExt.uiRefLayerDqId = 255;
        pShExt.uiDisableInterLayerDeblockingFilterIdc = 0;
        pShExt.iInterLayerSliceAlphaC0Offset = 0;
        pShExt.iInterLayerSliceBetaOffset = 0;
        pShExt.bConstrainedIntraResamplingFlag = false;
        pShExt.uiRefLayerChromaPhaseXPlus1Flag = 0;
        pShExt.uiRefLayerChromaPhaseYPlus1 = 1;
        pShExt.iScaledRefLayerPicWidthInSampleLuma = pShExt.sSliceHeader.iMbWidth << 4;
        pShExt.iScaledRefLayerPicHeightInSampleLuma = pShExt.sSliceHeader.iMbHeight << 4;
        pShExt.bSliceSkipFlag = false;
        pShExt.bAdaptiveBaseModeFlag = false;
        pShExt.bDefaultBaseModeFlag = false;
        pShExt.bAdaptiveMotionPredFlag = false;
        pShExt.bDefaultMotionPredFlag = false;
        pShExt.bAdaptiveResidualPredFlag = false;
        pShExt.bDefaultResidualPredFlag = false;
        pShExt.bTCoeffLevelPredFlag = false;
        pShExt.uiScanIdxStart = 0;
        pShExt.uiScanIdxEnd = 15;
        true
    }
}

pub fn InitBsBuffer(pCtx: &mut SWelsDecoderContext) -> i32 {
    // `WelsMalloczHelper`'s zeroed allocation, owned: the allocation size *is*
    // `sRawData.len()` — the `iMaxBsBufferSizeInByte` field died with the pointers,
    // since a stored copy of the buffer's length is exactly the kind of extent F16
    // is about.
    match RawDataBuffer::try_new_zeroed(MIN_ACCESS_UNIT_CAPACITY * MAX_BUFFERED_NUM) {
        Ok(raw) => (*pCtx).sRawData = raw,
        Err(()) => return ERR_INFO_OUT_OF_MEMORY,
    }

    if (*pCtx).pParam.bParseOnly {
        // T5.R4: three `WelsMallocz` blocks — the descriptor and its two buffers —
        // become one owned value. The `ERR_INFO_OUT_OF_MEMORY` arms go with the null
        // returns they tested for; `RawDataBuffer::try_new_zeroed` beside them keeps
        // its arm because it really can fail (T3.3's fallible reserve).
        (*pCtx).pParserBsInfo = Some(Box::new(ParseOnlyBsBuffers {
            pDstBuff: vec![0u8; MAX_ACCESS_UNIT_CAPACITY],
            pNalLenInByte: vec![0i32; MAX_NAL_UNITS_IN_LAYER + 2],
            ..Default::default()
        }));

        match RawDataBuffer::try_new_zeroed((*pCtx).sRawData.len()) {
            Ok(saved) => (*pCtx).sSavedData = saved,
            Err(()) => return ERR_INFO_OUT_OF_MEMORY,
        }
    }
    ERR_NONE
}

// `ExpandBsBuffer` was deleted at T3.3, not converted. Its growth policy (when to
// grow is `WelsDecodeBs`'s check; by how much is `max(srcLen * MAX_BUFFERED_NUM,
// len << 1)`) lives in [`RawDataBuffer::grow`]. Everything else it did was pointer
// maintenance that offsets made meaningless: the `sRawData`/`sSavedData` rebases
// (offsets survive a reallocation by definition — plan §2.2.2, P5) and the
// per-pending-NAL `sSliceBitsRead` rebase with its stale-`avail` repair (F16's
// second instance) — under the derive-don't-store rule there is nothing left to go
// stale, so the hazard is unrepresentable rather than repaired. `CheckBsBuffer`
// (upstream's per-frame growth trigger) had no caller in this port and died with
// it; the port's only trigger is the single-NAL-bigger-than-the-buffer check in
// `WelsDecodeBs`, unchanged.

pub fn ExpandBsLenBuffer(pCtx: &mut SWelsDecoderContext, kiCurrLen: i32) -> i32 {
    if !parser_bs(&mut (*pCtx).pParserBsInfo)
        .is_some_and(|p| !p.pNalLenInByte.as_slice().is_empty())
    {
        return ERR_INFO_INVALID_ACCESS;
    }
    if kiCurrLen >= MAX_MB_SIZE + 2 {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    let Some(pParser) = parser_bs(&mut (*pCtx).pParserBsInfo) else {
        return ERR_INFO_INVALID_ACCESS;
    };
    let mut iNewLen = kiCurrLen << 1;
    iNewLen = WELS_MIN(iNewLen, MAX_MB_SIZE + 2);
    // **F40 is unrepresentable here now.** The allocate-copy-free triple this was —
    // and whose `copy_nonoverlapping` counted bytes where the count is in elements, so
    // it copied four times the source — is `Vec::resize`, which knows both the element
    // size and the old length. The old length was `pCtx->iMaxNalNum`, a stored extent
    // that went with the pointer (F16).
    (*pParser).pNalLenInByte.resize(iNewLen as usize, 0);
    ERR_NONE
}

pub fn WelsInitDecoderFuncs(pCtx: &mut SWelsDecoderContext) {
    {
        let cpu_flag = (*pCtx).uiCpuFlag;

        // 0. Block helpers. `WelsBlockFuncInit` filled `sBlockFunc` here (`InitDecFuncs`
        // in `decoder.cpp`), through a `*mut _ as *mut _` double cast that bridged the
        // port's two incompatible declarations of one struct. T4b.3c deleted both. The
        // one slot that was ever read -- `pWelsSetNonZeroCountFunc`, which clamps every
        // non-zero coefficient count to 1 after inter reconstruction so that deblocking
        // derives the boundary strengths the C++ derives -- is a direct call at its
        // single use in `decode_slice.rs`.

        // 1. Deblocking Filter — the `DeblockingInit` install stood here. **S18,
        // S4.D4**: it filled a twelve-slot table nothing read (T5.Y3 had already
        // deleted `pLoopf`, the alias the C++ dispatches through), and it ignored
        // its own `cpu_flag` besides. A future SIMD pass builds its dispatch from
        // the safe kernels' signatures, not from these C-ABI shims.

        // 2. Motion Compensation
        crate::common::mc::InitMcFunc(&mut (*pCtx).sMcFunc, cpu_flag);

        // 2b. Reference picture border expansion installed `sExpandPicFunc` here,
        // three constants that T4b.3b turned into direct calls. Both chroma slots held
        // `ExpandPictureChroma_c`, so the aligned/unaligned index selected between two
        // identical functions -- the 4a shape, in the decoder.

        // 3. IDCT Inverse Transform
        (*pCtx).pIdctResAddPredFunc = Some(crate::decoder::decode_mb_aux::idct_res_add_pred);
        (*pCtx).pIdctResAddPredFunc8x8 = Some(crate::decoder::decode_mb_aux::idct_res_add_pred8x8);
        (*pCtx).pIdctFourResAddPredFunc = Some(crate::decoder::decode_mb_aux::idct_four_res_add_pred);

        // 4. Intra Prediction
        (*pCtx).pGetI4x4LumaPredFunc = [
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_v),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_h),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_dc),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_ddl),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_ddr),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_vr),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_hd),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_vl),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_hu),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_dc_left),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_dc_top),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_dc_na),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_ddl_top),
            Some(crate::decoder::get_intra_predictor::i4x4_luma_pred_vl_top),
        ];

        (*pCtx).pGetI16x16LumaPredFunc = [
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_v),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_h),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_dc),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_plane),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_dc_left),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_dc_top),
            Some(crate::decoder::get_intra_predictor::i16x16_luma_pred_dc_na),
        ];

        (*pCtx).pGetIChromaPredFunc = [
            Some(crate::decoder::get_intra_predictor::chroma_pred_dc),
            Some(crate::decoder::get_intra_predictor::chroma_pred_h),
            Some(crate::decoder::get_intra_predictor::chroma_pred_v),
            Some(crate::decoder::get_intra_predictor::chroma_pred_plane),
            Some(crate::decoder::get_intra_predictor::chroma_pred_dc_left),
            Some(crate::decoder::get_intra_predictor::chroma_pred_dc_top),
            Some(crate::decoder::get_intra_predictor::chroma_pred_dc_na),
        ];

        (*pCtx).pGetI8x8LumaPredFunc = [
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_v),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_h),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_dc),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_ddl),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_ddr),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_vr),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_hd),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_vl),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_hu),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_dc_left),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_dc_top),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_dc_na),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_ddl_top),
            Some(crate::decoder::get_intra_predictor::i8x8_luma_pred_vl_top),
        ];
    }
}

/// Returns the detected host CPU core count.
/// Matches `int32_t GetCPUCount()` in `decoder.cpp`.
pub fn GetCPUCount() -> i32 {
    1
}

/// Detects SIMD hardware capabilities.
/// Matches `uint32_t WelsCPUFeatureDetect (int32_t* pCPUFlag)` in `decoder.cpp`.
pub fn WelsCPUFeatureDetect(pCpuCores: &mut i32) -> u32 {
    *pCpuCores = GetCPUCount();
    0
}

/// Initializes CPU feature detection and decoder function tables.
/// Matches `int32_t WelsOpenDecoder (PWelsDecoderContext pCtx, SLogContext* pLogCtx)` in `decoder.cpp:52`.
/// Fill data fields in default for decoder context.
/// Matches `void WelsDecoderDefaults (PWelsDecoderContext pCtx, SLogContext* pLogCtx)` in `decoder.cpp`.
pub fn WelsDecoderDefaults(pCtx: &mut SWelsDecoderContext, pLogCtx: Option<&SLogContext>) {
    // `decoder.cpp:340` — `pCtx->sLogCtx = *pLogCtx`. The parameter was `_pLogCtx:
    // *mut c_void` and ignored until T8.B6, which is one half of why the decoder's
    // seventeen `WelsLog` call sites delivered nothing: the context they log
    // through had never been given a destination.
    if let Some(pLogCtx) = pLogCtx {
        pCtx.sLogCtx = *pLogCtx;
    }
    let mut iCpuCores = 1i32;
    (*pCtx).pArgDec = std::ptr::null_mut();
    (*pCtx).bHaveGotMemory = false;
    (*pCtx).uiCpuFlag = 0;
    (*pCtx).bAuReadyFlag = false;
    (*pCtx).bCabacInited = false;
    (*pCtx).uiCpuFlag = WelsCPUFeatureDetect(&mut iCpuCores) as u32;
    (*pCtx).iImgWidthInPixel = 0;
    (*pCtx).iImgHeightInPixel = 0;
    (*pCtx).iLastImgWidthInPixel = 0;
    (*pCtx).iLastImgHeightInPixel = 0;
    (*pCtx).bFreezeOutput = true;
    (*pCtx).iFrameNum = -1;
    {
        let info = &mut pCtx.pLastDecPicInfo;
        info.iPrevFrameNum = -1;
    }
    (*pCtx).iErrorCode = ERR_NONE;
    (*pCtx).pDec = None;
    // T5.P″1: both were `= null_mut()` and are now `= None`, which *drops* what they
    // held. Checked rather than assumed (S23's question, aimed at a lifecycle): this
    // function is `WelsDecoderDefaults`, called from exactly one place —
    // `codec_api.rs:1454`, on the line after `Box::into_raw(ctx_box)` — so it runs on
    // a context that has never decoded, both fields are already `None`, and the drop
    // is of nothing. The C leaked here for the same reason it could not free: it had
    // only a pointer's zero to write.
    (*pCtx).pTempDec = None;
    WelsResetRefPic(pCtx);
    (*pCtx).iActiveFmoNum = 0;
    (*pCtx).pPicBuff = None;
    {
        let info = &mut pCtx.pLastDecPicInfo;
        info.pPreviousDecodedPictureInDpb = None;
    }
    {
        let stat = &mut (*pCtx).pDecoderStatistics;
        stat.iAvgLumaQp = -1;
        stat.iStatisticsLogInterval = 1000;
    }
    (*pCtx).bUseScalingList = false;
    (*pCtx).iFeedbackNalRefIdc = -1;
    {
        let info = &mut pCtx.pLastDecPicInfo;
        info.iPrevPicOrderCntMsb = 0;
        info.iPrevPicOrderCntLsb = 0;
    }
}

/// Fill data fields in SPS and PPS default for decoder context.
/// Matches `void WelsDecoderSpsPpsDefaults (SWelsDecoderSpsPpsCTX& sSpsPpsCtx)` in `decoder.cpp`.
pub fn WelsDecoderSpsPpsDefaults(sSpsPpsCtx: &mut crate::decoder::decoder_context::SWelsDecoderSpsPpsCTX) {
    sSpsPpsCtx.bSpsExistAheadFlag = false;
    sSpsPpsCtx.bSubspsExistAheadFlag = false;
    sSpsPpsCtx.bPpsExistAheadFlag = false;
    sSpsPpsCtx.bAvcBasedFlag = true;
    sSpsPpsCtx.iSpsErrorIgnored = 0;
    sSpsPpsCtx.iSubSpsErrorIgnored = 0;
    sSpsPpsCtx.iPpsErrorIgnored = 0;
    sSpsPpsCtx.iPPSInvalidNum = 0;
    sSpsPpsCtx.iPPSLastInvalidId = -1;
    sSpsPpsCtx.iSPSInvalidNum = 0;
    sSpsPpsCtx.iSPSLastInvalidId = -1;
    sSpsPpsCtx.iSubSPSInvalidNum = 0;
    sSpsPpsCtx.iSubSPSLastInvalidId = -1;
    sSpsPpsCtx.iSeqId = -1;
}

/// Fill last decoded picture info defaults.
/// Matches `void WelsDecoderLastDecPicInfoDefaults (SWelsLastDecPicInfo& sLastDecPicInfo)` in `decoder.cpp`.
pub fn WelsDecoderLastDecPicInfoDefaults(sLastDecPicInfo: &mut crate::decoder::decoder_context::SWelsLastDecPicInfo) {
    sLastDecPicInfo.iPrevPicOrderCntMsb = 0;
    sLastDecPicInfo.iPrevPicOrderCntLsb = 0;
    sLastDecPicInfo.pPreviousDecodedPictureInDpb = None;
    sLastDecPicInfo.iPrevFrameNum = -1;
    sLastDecPicInfo.bLastHasMmco5 = false;
    sLastDecPicInfo.uiDecodingTimeStamp = 0;
}

/// Reset picture reordering buffer list.
/// Matches `void ResetReorderingPictureBuffers (...)` in `decoder.cpp`.
/// The list is the context's own `[SPictInfo; 16]` since T8.A7 (it was
/// `CWelsDecoderImpl::sPictInfoList`, reached through a stamped pointer), so the
/// parameter is that array: the `16` the `fullReset` arm writes was the C's `sizeof`
/// and is now the type's own length, and the `pPictInfo.add(i)` walk is an index.
/// `iLargestBufferedPicIndex + 1` is clamped to it — the C++ trusts the field.
pub fn ResetReorderingPictureBuffers(
    pPictReoderingStatus: &mut crate::decoder::decoder_context::SPictReoderingStatus,
    pPictInfo: &mut [crate::decoder::decoder_context::SPictInfo; 16],
    fullReset: bool,
) {
    let pictInfoListCount = if fullReset {
        pPictInfo.len()
    } else {
        ((pPictReoderingStatus.iLargestBufferedPicIndex + 1).max(0) as usize).min(pPictInfo.len())
    };
    pPictReoderingStatus.iPictInfoIndex = 0;
    pPictReoderingStatus.iMinPOC = crate::decoder::decoder_context::IMinInt32;
    pPictReoderingStatus.iNumOfPicts = 0;
    pPictReoderingStatus.iLastWrittenPOC = crate::decoder::decoder_context::IMinInt32;
    pPictReoderingStatus.iLargestBufferedPicIndex = 0;
    for info in pPictInfo.iter_mut().take(pictInfoListCount) {
        info.iPOC = crate::decoder::decoder_context::IMinInt32;
        info.iPicBuffIdx = -1;
    }
    pPictInfo[0].sBufferInfo.iBufferStatus = 0;
    pPictReoderingStatus.bHasBSlice = false;
}

/// Matches `int32_t DecoderConfigParam (PWelsDecoderContext pCtx, const SDecodingParam* kpParam)`
/// in `decoder.cpp:649`.
///
/// **The C++'s own name for "the caller's parameters become the context's"**, and
/// T8.A5 gives the port the function rather than the two lines it had scattered
/// across the boundary. The copy is `decoder.cpp:653`'s `memcpy` — into the block
/// the context owns, which is the whole of **F41**: the port used to point
/// `pCtx->pParam` at `CWelsDecoderImpl::param` and overwrite *that* on every
/// `Initialize`, so a field the teardown reads lived in an object with its own
/// lifetime and its own writers.
///
/// `InitErrorCon` is the C++'s next statement and F44's first call site: the mode
/// just copied selects which kernels `sCopyFunc` holds, and it also clears
/// `bFreezeOutput`, which `WelsDecoderDefaults` sets **true**.
///
/// **F76, T8.B1 — the three statements this function used to be missing are here.**
/// The range clamp (`:654`) is not: it has to run on the `int` the caller wrote,
/// before the value is an `ERROR_CON_IDC` at all, so it lives at the boundary in
/// `api::codec_api::read_decoding_param` and the block that arrives here is already
/// in range. The other two — the parse-only concealment disable (`:663`) and the
/// `pCtx->eVideoType` assignment (`:667`) — are written below in the reference's
/// order, and the second is what makes `eVideoType` a field with a writer: it was
/// set once by the context constructor and never again, so its one reader (the
/// key-frame-loss arm of `DecodeFrame2`'s error block) would have fired on every
/// stream rather than on AVC ones.
/// `void CWelsDecoder::OutputStatisticsLog (SDecoderStatistics&)` —
/// `welsDecoderExt.cpp:947`.
///
/// **F76, T8.B3.** One line every `iStatisticsLogInterval` decoded frames (1000 by
/// default, `WelsDecoderDefaults`), and the reason `uiDecodedFrameCount` has to be
/// counted on *both* of `DecodeFrame2`'s tails rather than only the error one: the
/// interval is a modulus over it, and a counter that never moves prints nothing and
/// divides by zero in `DECODER_OPTION_GET_STATISTICS`'s two speed fields.
pub fn OutputStatisticsLog(pCtx: &mut SWelsDecoderContext) {
    let s = pCtx.pDecoderStatistics;
    if s.uiDecodedFrameCount == 0
        || s.iStatisticsLogInterval == 0
        || s.uiDecodedFrameCount % s.iStatisticsLogInterval != 0
    {
        return;
    }
    WelsLog(
        std::ptr::addr_of_mut!(pCtx.sLogCtx),
        WELS_LOG_INFO,
        &format!(
            "DecoderStatistics: uiWidth={}, uiHeight={}, fAverageFrameSpeedInMs={:.1}, \
             fActualAverageFrameSpeedInMs={:.1}, uiDecodedFrameCount={}, \
             uiResolutionChangeTimes={}, uiIDRCorrectNum={}, uiAvgEcRatio={}, \
             uiAvgEcPropRatio={}, uiEcIDRNum={}, uiEcFrameNum={}, uiIDRLostNum={}, \
             uiFreezingIDRNum={}, uiFreezingNonIDRNum={}, iAvgLumaQp={}, \
             iSpsReportErrorNum={}, iSubSpsReportErrorNum={}, iPpsReportErrorNum={}, \
             iSpsNoExistNalNum={}, iSubSpsNoExistNalNum={}, iPpsNoExistNalNum={}, \
             uiProfile={}, uiLevel={}, iCurrentActiveSpsId={}, iCurrentActivePpsId={},",
            s.uiWidth,
            s.uiHeight,
            s.fAverageFrameSpeedInMs,
            s.fActualAverageFrameSpeedInMs,
            s.uiDecodedFrameCount,
            s.uiResolutionChangeTimes,
            s.uiIDRCorrectNum,
            s.uiAvgEcRatio,
            s.uiAvgEcPropRatio,
            s.uiEcIDRNum,
            s.uiEcFrameNum,
            s.uiIDRLostNum,
            s.uiFreezingIDRNum,
            s.uiFreezingNonIDRNum,
            s.iAvgLumaQp,
            s.iSpsReportErrorNum,
            s.iSubSpsReportErrorNum,
            s.iPpsReportErrorNum,
            s.iSpsNoExistNalNum,
            s.iSubSpsNoExistNalNum,
            s.iPpsNoExistNalNum,
            s.uiProfile,
            s.uiLevel,
            s.iCurrentActiveSpsId,
            s.iCurrentActivePpsId,
        ),
    );
}

pub fn DecoderConfigParam(pCtx: &mut SWelsDecoderContext, kpParam: &SDecodingParam) {
    pCtx.pParam = *kpParam;
    // `decoder.cpp:663` — parse-only decoding disables concealment. Inert on output
    // today only because `DecodeParser` is a stub; not inert as configuration,
    // because the mode selects `sCopyFunc`'s kernels one line below.
    if pCtx.pParam.bParseOnly {
        pCtx.pParam.eEcActiveIdc = ERROR_CON_DISABLE;
    }
    crate::decoder::error_concealment::InitErrorCon(pCtx);
    // `decoder.cpp:667–671`. The out-of-range `else` is `read_decoding_param`'s, for
    // the same reason the clamp is: `VIDEO_BITSTREAM_TYPE` has two variants and the
    // wire has 2^32 values.
    pCtx.eVideoType = pCtx.pParam.sVideoProperty.eVideoBsType;
    WelsLog(
        std::ptr::addr_of_mut!(pCtx.sLogCtx),
        WELS_LOG_INFO,
        &format!("eVideoType: {}", pCtx.eVideoType as i32),
    );
}

pub fn WelsOpenDecoder(pCtx: &mut SWelsDecoderContext, _pLogCtx: *mut c_void) -> i32 {
    let mut cpu_cores = 0i32;
    (*pCtx).uiCpuFlag = { WelsCPUFeatureDetect(&mut cpu_cores) } as u32;
    { WelsInitDecoderFuncs(pCtx) };
    // `decoder.cpp:606` — the vlc tables, right after the function pointers. T8.A6
    // moved this here from `WelsCreateDecoder`, which had it because the table was a
    // `CWelsDecoderImpl` member rather than the context's field.
    crate::decoder::parse_mb_syn_cavlc::InitVlcTable(&mut pCtx.pVlcTable);
    (*pCtx).bParamSetsLostFlag = true;
    (*pCtx).bNewSeqBegin = true;
    (*pCtx).bPrintFrameErrorTraceFlag = true;
    (*pCtx).iIgnoredErrorInfoPacketCount = 0;
    (*pCtx).bFrameFinish = true;
    (*pCtx).iSeqNum = 0;
    ERR_NONE
}

/// Frees dynamically-grown decoder memory (DQ layers, FMO, reference
/// pictures, picture buffer, CABAC engine).
/// Matches `void WelsFreeDynamicMemory (PWelsDecoderContext pCtx)` in `decoder.cpp`.
pub fn WelsFreeDynamicMemory(pCtx: &mut SWelsDecoderContext) {

    UninitialDqLayersContext(pCtx);
    crate::decoder::nalu::ResetFmoList(pCtx);
    WelsResetRefPic(pCtx);

    if (*pCtx).pPicBuff.is_some() {
        // `.take()` is the C's `ppPicBuf` out-parameter: it reads the pool and nulls
        // the field in one expression, so `DestroyPicBuff` cannot return with the
        // context still naming a pool it has freed.
        let pool = (*pCtx).pPicBuff.take();
        crate::decoder::pic_queue::DestroyPicBuff(pCtx, pool);
    }

    // T5.P″1: `FreePicture((*pCtx).pTempDec, pMa)` followed by a null store. One
    // `= None` is both, and F19's question — which line frees this? — is answered by
    // the type: this line, or the context's drop glue if this line never runs (R4).
    (*pCtx).pTempDec = None;

    // T5.O2: the CABAC engine's free block stood here. The engine is a field now, so
    // there is no allocation to release and no pointer to null — and, unlike the
    // pointer, the field cannot be read after this function has run.

    (*pCtx).iImgWidthInPixel = 0;
    (*pCtx).iImgHeightInPixel = 0;
    (*pCtx).iLastImgWidthInPixel = 0;
    (*pCtx).iLastImgHeightInPixel = 0;
    (*pCtx).bFreezeOutput = true;
    (*pCtx).bHaveGotMemory = false;
}

/// Terminates decoder worker threads and cleans up internal decoding context.
/// Matches `void WelsEndDecoder (PWelsDecoderContext pCtx)` in `decoder.cpp:711`.
pub fn WelsEndDecoder(pCtx: &mut SWelsDecoderContext) {
    {
        WelsFreeDynamicMemory(pCtx);
        { WelsFreeStaticMemory(pCtx) };
        (*pCtx).bParamSetsLostFlag = false;
        (*pCtx).bNewSeqBegin = false;
        (*pCtx).bPrintFrameErrorTraceFlag = false;
        (*pCtx).iIgnoredErrorInfoPacketCount = 0;
        (*pCtx).bFrameFinish = false;
    }
}

pub fn WelsInitStaticMemory(pCtx: &mut SWelsDecoderContext) -> i32 {
    {
        WelsOpenDecoder(pCtx, std::ptr::null_mut());
        // F19: freed by the context's drop glue. `MAX_NAL_UNIT_NUM_IN_AU` is still the
        // caller's argument, exactly as `decoder_core.cpp:763` passes it.
        (*pCtx).access_unit = Some(SAccessUnit::with_nodes(MAX_NAL_UNIT_NUM_IN_AU));
        if { InitBsBuffer(pCtx) } != 0 {
            (*pCtx).iErrorCode |= dsOutOfMemory;
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pCtx).uiTargetDqId = 255;
        (*pCtx).bEndOfStreamFlag = false;
        ERR_NONE
    }
}

pub fn WelsFreeStaticMemory(pCtx: &mut SWelsDecoderContext) {
    // R4, first entry: the access unit's free is `Option::take`'s drop, and it happens
    // here rather than in the context's own `Drop` only because this function still
    // exists. Both of its callers run `drop(Box::from_raw(pCtx))` on the next line, so
    // moving it either way is the same program.
    (*pCtx).access_unit = None;

    // The buffers own their allocations now; reset releases them (the WelsFreeHelper
    // free-cascade entries for sRawData/sSavedData died with the pointers).
    (*pCtx).sRawData.reset();

    if (*pCtx).pParam.bParseOnly {
        (*pCtx).sSavedData.reset();
    }
    // **Outside the `bParseOnly` arm on purpose (T5.R4, and it is half of F41).** The
    // three free calls that stood inside it — the two buffers and the descriptor —
    // were reachable only when the flag still read `true` at teardown, and the flag
    // lives in the API object where a second `Initialize` can flip it. As an owned
    // field the release is unconditional, so those three blocks can no longer leak;
    // what remains of F41 is `sSavedData` above and the `pParam` alias itself, which
    // is Phase 8's.
    (*pCtx).pParserBsInfo = None;
}

// A duplicate `DecodeNalHeaderExt` was deleted dead here at T3.3 (S18): it had no
// callers — both call sites resolve to `nalu::DecodeNalHeaderExt`, which takes the
// 3-byte window as a slice since this seam.

pub fn UpdateDecoderStatisticsForActiveParaset(
    pDecoderStatistics: Option<&mut SDecoderStatistics>,
    pSps: Option<&SSps>,
    pPps: Option<&SPps>,
) {
    let (Some(pDecoderStatistics), Some(pSps), Some(pPps)) = (pDecoderStatistics, pSps, pPps)
    else {
        return;
    };
    pDecoderStatistics.iCurrentActiveSpsId = pSps.iSpsId;
    pDecoderStatistics.iCurrentActivePpsId = pPps.iPpsId;
    pDecoderStatistics.uiProfile = pSps.uiProfileIdc as u32;
    pDecoderStatistics.uiLevel = pSps.uiLevelIdc as u32;
}

/// **T5b.3: the header is parsed into a scratch and written back once.**
///
/// The body below took three `addr_of_mut!` derivations into the access unit's last
/// node and held them across its whole 576 lines — which is sound only while the
/// container's slots are raw, and they own now. A local scratch, copied in and copied
/// back at the single exit, is behaviour-identical: the C++ parses in place, and
/// nothing between the two copies reads that node (the sub-parsers take the scratch;
/// `ParseDecRefPicMarking`'s second POC write targets a *different* NAL, the decode
/// loop's, through `slice_hdr_nal`). What it buys is that the parse can keep taking
/// the context — which it does five times, for `sRawData`, `sSpsPpsCtx`,
/// `iErrorCode`, `pLastDecPicInfo` and `pDecoderStatistics`.
pub fn ParseSliceHeaderSyntaxs(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
    kbExtensionFlag: bool,
) -> i32 {
    let last = match pCtx.access_unit.as_deref() {
        None => return ERR_INFO_INVALID_PTR,
        Some(au) if au.uiAvailUnitsNum == 0 => return ERR_INFO_OUT_OF_MEMORY,
        Some(au) => (au.uiAvailUnitsNum - 1) as usize,
    };
    let (mut ext, mut hdr) = match cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(last)) {
        Some(nal) => {
            nal.sNalData.sVclNal.bSliceHeaderExtFlag = kbExtensionFlag;
            (nal.sNalData.sVclNal.sSliceHeaderExt, nal.sNalHeaderExt)
        }
        None => return ERR_INFO_INVALID_PTR,
    };
    let iRet = parse_slice_header_into(pCtx, kiRbspStart, pBs, kbExtensionFlag, &mut ext, &mut hdr);
    // The single exit's copy-back. It is unconditional on `iRet` because the in-place
    // parse it replaces left every field it had written behind on an error return, and
    // `WelsParseOneNal`'s error arms read the node afterwards.
    if let Some(nal) = cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(last)) {
        nal.sNalData.sVclNal.sSliceHeaderExt = ext;
        nal.sNalHeaderExt = hdr;
    }
    iRet
}

fn parse_slice_header_into(
    pCtx: &mut SWelsDecoderContext,
    kiRbspStart: usize,
    pBs: &mut BsCursor,
    kbExtensionFlag: bool,
    pSliceHeadExt: &mut SSliceHeaderExt,
    pNalHeaderExt: &mut SNalUnitHeaderExt,
) -> i32 {
    {
        // **The window is re-derived per read, not bound** (T5.Z4). This function
        // interleaves reads with calls that take the context — the three sub-parsers and
        // the parameter-set lookups — so a `let win!() = …` at the top is a shared borrow of
        // `sRawData` live across a `&mut` of its parent. Each `win!()` ends with its
        // expression; S25's shape, and the same one `ref_set` takes for the DPB.
        macro_rules! win {
            () => {
                pCtx.sRawData.window_from(kiRbspStart)
            };
        }

        let eNalType = pNalHeaderExt.sNalUnitHeader.eNalUnitType;

        let mut uiCode: u32 = 0;
        let mut iCode: i32 = 0;

        if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > 36863 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FIRST_MB_IN_SLICE);
        }
        pSliceHeadExt.sSliceHeader.iFirstMbInSlice = uiCode as i32;

        if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        let mut uiSliceType = uiCode;
        if uiSliceType > 9 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
        }
        if uiSliceType > 4 {
            uiSliceType -= 5;
        }
        if eNalType == NAL_UNIT_CODED_SLICE_IDR && uiSliceType != 2 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
        }
        if kbExtensionFlag && uiSliceType > 2 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
        }

        pSliceHeadExt.sSliceHeader.eSliceType = match uiSliceType {
            0 => P_SLICE,
            1 => B_SLICE,
            2 => I_SLICE,
            3 => SP_SLICE,
            _ => SI_SLICE,
        };

        if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode >= MAX_PPS_COUNT as u32 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_PPS_ID_OVERFLOW);
        }
        let iPpsId = uiCode as i32;

        if !(*pCtx).sSpsPpsCtx.bPpsAvailFlags[iPpsId as usize] {
            {
                let stat = &mut (*pCtx).pDecoderStatistics;
                stat.iPpsReportErrorNum += 1;
            }
            (*pCtx).iErrorCode |= dsNoParamSets;
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_PPS_ID);
        }

        // The same rule, for the paramset side — a second live instance of F24's shape,
        // not a precaution. `pSps` was `&mut (*pSubsetSps).sSps`, nested inside a `&mut` of
        // the subset SPS entry, and the three later `let pSubsetSps` bindings each took
        // `&mut` of that *same* entry again — popping the nested tag, after which every
        // `(*pSps)` read is UB (`uiLog2MaxFrameNum`, `uiTotalMbCount`, `bFrameMbsOnlyFlag`,
        // the POC block, and on). Every `kbExtensionFlag` path, which means every SVC
        // stream. **No gate reaches it**: `bExtensionFlag` is `eType == NAL_UNIT_CODED_SLICE_EXT`
        // (`nalu.rs:684`) and the probe decodes AVC, so it was found by reading while fixing
        // the pair above and dies in the same commit rather than waiting for a test that
        // does not exist. The stored `pSliceHeadExt.sSliceHeader.pps_id` / `pSps` pointers get the
        // context's provenance for free, which is what S28 asks of a pointer that outlives
        // the expression that made it.
        // T5b.3: an index, not an address. The `addr_of_mut!` was S29's spelling for a
        // *raw* parent; the parse has the context as a borrow and re-resolves per read,
        // exactly as `win!()` and `sps!()` below do.
        let pps_idx = iPpsId as usize;
        macro_rules! pps {
            () => {
                pCtx.sSpsPpsCtx.sPpsBuffer[pps_idx]
            };
        }
        if pps!().uiNumSliceGroups == 0 {
            (*pCtx).iErrorCode |= dsNoParamSets;
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_NO_PARAM_SETS);
        }

        // **T5.Z4: the ref, not the address.** This derived a raw `SSps` alias out of
        // the context and read it across two dozen calls that take the context — the
        // same class the accessors shed at T5.Z1, arriving from a hand-written
        // derivation. `sps!()` re-resolves in one expression, exactly as `win!()` does
        // for the bitstream window above.
        let sps_ref = Some(SpsRef { id: pps!().iSpsId, subset: kbExtensionFlag });
        macro_rules! sps {
            () => {
                match sps_of(&pCtx.sSpsPpsCtx, sps_ref) {
                    Some(sps) => sps,
                    None => return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SPS_ID),
                }
            };
        }

        if sps!().iNumRefFrames == 0
            && pSliceHeadExt.sSliceHeader.eSliceType != I_SLICE
            && pSliceHeadExt.sSliceHeader.eSliceType != SI_SLICE
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
        }

        pSliceHeadExt.sSliceHeader.iPpsId = iPpsId;
        pSliceHeadExt.sSliceHeader.iSpsId = pps!().iSpsId;
        // T5.R6: what the two stored pointers carried — an id and, for the SPS, which of
        // the two buffers it indexes. `kbExtensionFlag` is that choice, made three lines
        // above where `pSps` was derived.
        pSliceHeadExt.sSliceHeader.pps_id = Some(iPpsId);
        pSliceHeadExt.sSliceHeader.sps_ref = Some(SpsRef { id: pps!().iSpsId, subset: kbExtensionFlag });
        if kbExtensionFlag {
            pSliceHeadExt.subset_sps_id = Some(pps!().iSpsId);
        }

        let bIdrFlag = (!kbExtensionFlag && eNalType == NAL_UNIT_CODED_SLICE_IDR)
            || (kbExtensionFlag && pNalHeaderExt.bIdrFlag);
        pSliceHeadExt.sSliceHeader.bIdrFlag = bIdrFlag;

        if sps!().uiLog2MaxFrameNum == 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_NO_PARAM_SETS);
        }
        if (pSliceHeadExt.sSliceHeader.iFirstMbInSlice as u32) > sps!().uiTotalMbCount - 1 {
            return GENERATE_ERROR_NO(
                ERR_LEVEL_SLICE_HEADER,
                ERR_INFO_INVALID_FIRST_MB_IN_SLICE,
            );
        }
        if BsGetBits(win!(), pBs, sps!().uiLog2MaxFrameNum, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        pSliceHeadExt.sSliceHeader.iFrameNum = uiCode as i32;
        if !sps!().bFrameMbsOnlyFlag {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MBAFF);
        }
        pSliceHeadExt.sSliceHeader.iMbWidth = sps!().iMbWidth as i32;
        pSliceHeadExt.sSliceHeader.iMbHeight = sps!().iMbHeight as i32;

        if bIdrFlag {
            if pSliceHeadExt.sSliceHeader.iFrameNum != 0 {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FRAME_NUM);
            }
            if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode > SLICE_HEADER_IDR_PIC_ID_MAX {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_IDR_PIC_ID);
            }
            pSliceHeadExt.sSliceHeader.uiIdrPicId = uiCode as u16;
            // `decoder_core.cpp:1060-1062`, under `LONG_TERM_REF` (which
            // `decoder_context.h:67` defines, so it is on in every reference build).
            //
            // **T8b.A3: `uiCurIdrPicId` was declared, reset, and never written.**
            // F77's signature exactly — and it is the only feeder for
            // `DECODER_OPTION_IDR_PIC_ID`, so `decode_api_test.cpp:132`'s three rows
            // read a field that could only ever be 0. Written here, at the reference's
            // point: the *parse* of the IDR's slice header, not its reconstruction,
            // which is why the test reads the same value on both `DecodeFrame2` calls.
            pCtx.uiCurIdrPicId = uiCode as u16;
        }

        pSliceHeadExt.sSliceHeader.iDeltaPicOrderCntBottom = 0;
        pSliceHeadExt.sSliceHeader.iDeltaPicOrderCnt[0] = 0;
        pSliceHeadExt.sSliceHeader.iDeltaPicOrderCnt[1] = 0;
        if sps!().uiPocType == 0 {
            if BsGetBits(win!(), pBs, sps!().iLog2MaxPocLsb as u32, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            let iMaxPocLsb = 1 << sps!().iLog2MaxPocLsb;
            let pocLsb = uiCode as i32;
            pSliceHeadExt.sSliceHeader.iPicOrderCntLsb = pocLsb;
            if pps!().bPicOrderPresentFlag && !pSliceHeadExt.sSliceHeader.bFieldPicFlag {
                if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.sSliceHeader.iDeltaPicOrderCntBottom = iCode;
            }
            // The api-owned alias family, through its accessor (T5.AC4).
            let prevLsb = pCtx.pLastDecPicInfo.iPrevPicOrderCntLsb;
            let prevMsb = pCtx.pLastDecPicInfo.iPrevPicOrderCntMsb;
            let pocMsb = if pocLsb < prevLsb && (prevLsb - pocLsb) >= (iMaxPocLsb / 2) {
                prevMsb + iMaxPocLsb
            } else if pocLsb > prevLsb && (pocLsb - prevLsb) > (iMaxPocLsb / 2) {
                prevMsb - iMaxPocLsb
            } else {
                prevMsb
            };
            pSliceHeadExt.sSliceHeader.iPicOrderCntLsb = pocMsb + pocLsb;
            if pps!().bPicOrderPresentFlag && !pSliceHeadExt.sSliceHeader.bFieldPicFlag {
                pSliceHeadExt.sSliceHeader.iPicOrderCntLsb += pSliceHeadExt.sSliceHeader.iDeltaPicOrderCntBottom;
            }
            if pNalHeaderExt.sNalUnitHeader.uiNalRefIdc != 0 {
                {
                    let info = &mut pCtx.pLastDecPicInfo;
                    info.iPrevPicOrderCntLsb = pocLsb;
                    info.iPrevPicOrderCntMsb = pocMsb;
                }
            }
        } else if sps!().uiPocType == 1 && !sps!().bDeltaPicOrderAlwaysZeroFlag {
            if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            pSliceHeadExt.sSliceHeader.iDeltaPicOrderCnt[0] = iCode;
            if pps!().bPicOrderPresentFlag && !pSliceHeadExt.sSliceHeader.bFieldPicFlag {
                if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.sSliceHeader.iDeltaPicOrderCnt[1] = iCode;
            }
        }

        pSliceHeadExt.sSliceHeader.iRedundantPicCnt = 0;
        if pps!().bRedundantPicCntPresentFlag {
            if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode > SLICE_HEADER_REDUNDANT_PIC_CNT_MAX {
                return GENERATE_ERROR_NO(
                    ERR_LEVEL_SLICE_HEADER,
                    ERR_INFO_INVALID_REDUNDANT_PIC_CNT,
                );
            }
            pSliceHeadExt.sSliceHeader.iRedundantPicCnt = uiCode as i32;
            if pSliceHeadExt.sSliceHeader.iRedundantPicCnt > 0 {
                return GENERATE_ERROR_NO(
                    ERR_LEVEL_SLICE_HEADER,
                    ERR_INFO_INVALID_REDUNDANT_PIC_CNT,
                );
            }
        }

        if pSliceHeadExt.sSliceHeader.eSliceType == B_SLICE {
            if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            pSliceHeadExt.sSliceHeader.iDirectSpatialMvPredFlag = uiCode as i32;
        }

        pSliceHeadExt.sSliceHeader.uiRefCount[0] = pps!().uiNumRefIdxL0Active as i32;
        pSliceHeadExt.sSliceHeader.uiRefCount[1] = pps!().uiNumRefIdxL1Active as i32;

        let mut bReadNumRefFlag = pSliceHeadExt.sSliceHeader.eSliceType == P_SLICE
            || pSliceHeadExt.sSliceHeader.eSliceType == B_SLICE;
        if kbExtensionFlag {
            bReadNumRefFlag &= pNalHeaderExt.uiQualityId == BASE_QUALITY_ID;
        }
        if bReadNumRefFlag {
            if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            pSliceHeadExt.sSliceHeader.bNumRefIdxActiveOverrideFlag = uiCode != 0;
            if pSliceHeadExt.sSliceHeader.bNumRefIdxActiveOverrideFlag {
                if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if uiCode > MAX_NUM_REF_IDX_L0_ACTIVE_MINUS1 {
                    return GENERATE_ERROR_NO(
                        ERR_LEVEL_SLICE_HEADER,
                        ERR_INFO_INVALID_NUM_REF_IDX_L0_ACTIVE_MINUS1,
                    );
                }
                pSliceHeadExt.sSliceHeader.uiRefCount[0] = (1 + uiCode) as i32;
                if pSliceHeadExt.sSliceHeader.eSliceType == B_SLICE {
                    if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if uiCode > MAX_NUM_REF_IDX_L1_ACTIVE_MINUS1 {
                        return GENERATE_ERROR_NO(
                            ERR_LEVEL_SLICE_HEADER,
                            ERR_INFO_INVALID_NUM_REF_IDX_L1_ACTIVE_MINUS1,
                        );
                    }
                    pSliceHeadExt.sSliceHeader.uiRefCount[1] = (1 + uiCode) as i32;
                }
            }
        }
        if (pSliceHeadExt.sSliceHeader.uiRefCount[0] as usize) > MAX_REF_PIC_COUNT
            || (pSliceHeadExt.sSliceHeader.uiRefCount[1] as usize) > MAX_REF_PIC_COUNT
        {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_REF_COUNT_OVERFLOW);
        }

        if pNalHeaderExt.uiQualityId == BASE_QUALITY_ID {
            let iRet = ParseRefPicListReordering(pCtx, kiRbspStart, pBs, &mut pSliceHeadExt.sSliceHeader);
            if iRet != ERR_NONE {
                return iRet;
            }

            // pred_weight_table(): present for weighted P slices and for B slices when
            // weighted_bipred_idc == 1. Skipping it desynchronises the rest of the
            // slice header (`decoder_core.cpp`).
            if (pps!().bWeightedPredFlag && uiSliceType == P_SLICE as u32)
                || (pps!().uiWeightedBipredIdc == 1 && uiSliceType == B_SLICE as u32)
            {
                let iRet = ParsePredWeightedTable(pCtx, kiRbspStart, pBs, &mut pSliceHeadExt.sSliceHeader);
                if iRet != ERR_NONE {
                    return iRet;
                }
            }

            if kbExtensionFlag {
                pSliceHeadExt.bBasePredWeightTableFlag =
                    !(pNalHeaderExt.bNoInterLayerPredFlag || pNalHeaderExt.uiQualityId > 0);
            }

            if pNalHeaderExt.sNalUnitHeader.uiNalRefIdc != 0 {
                let iRet = ParseDecRefPicMarking(pCtx, kiRbspStart, pBs, &mut pSliceHeadExt.sSliceHeader, sps_ref, bIdrFlag);
                if iRet != ERR_NONE {
                    return iRet;
                }
                if kbExtensionFlag {
                    let subset_idx = pps!().iSpsId as usize;
                    if !pCtx.sSpsPpsCtx.sSubsetSpsBuffer[subset_idx]
                        .sSpsSvcExt
                        .bSliceHeaderRestrictionFlag
                    {
                        if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        pSliceHeadExt.bStoreRefBasePicFlag = uiCode != 0;
                        if (pNalHeaderExt.bUseRefBasePicFlag
                            || pSliceHeadExt.bStoreRefBasePicFlag)
                            && !bIdrFlag
                        {
                            return GENERATE_ERROR_NO(
                                ERR_LEVEL_SLICE_HEADER,
                                ERR_INFO_UNSUPPORTED_ILP,
                            );
                        }
                    }
                }
            }
        }

        if pps!().bEntropyCodingModeFlag {
            if pSliceHeadExt.sSliceHeader.eSliceType != I_SLICE && pSliceHeadExt.sSliceHeader.eSliceType != SI_SLICE {
                if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if uiCode > SLICE_HEADER_CABAC_INIT_IDC_MAX {
                    return ERR_INFO_INVALID_CABAC_INIT_IDC;
                }
                pSliceHeadExt.sSliceHeader.iCabacInitIdc = uiCode as i32;
            } else {
                pSliceHeadExt.sSliceHeader.iCabacInitIdc = 0;
            }
        }

        if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        pSliceHeadExt.sSliceHeader.iSliceQpDelta = iCode;
        pSliceHeadExt.sSliceHeader.iSliceQp = pps!().iPicInitQp + pSliceHeadExt.sSliceHeader.iSliceQpDelta;
        if pSliceHeadExt.sSliceHeader.iSliceQp < 0 || pSliceHeadExt.sSliceHeader.iSliceQp > 51 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_QP);
        }

        pSliceHeadExt.sSliceHeader.uiDisableDeblockingFilterIdc = 0;
        pSliceHeadExt.sSliceHeader.iSliceAlphaC0Offset = 0;
        pSliceHeadExt.sSliceHeader.iSliceBetaOffset = 0;
        if pps!().bDeblockingFilterControlPresentFlag {
            if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            pSliceHeadExt.sSliceHeader.uiDisableDeblockingFilterIdc = uiCode;
            if pSliceHeadExt.sSliceHeader.uiDisableDeblockingFilterIdc > 6 {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_DBLOCKING_IDC);
            }
            if pSliceHeadExt.sSliceHeader.uiDisableDeblockingFilterIdc != 1 {
                if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.sSliceHeader.iSliceAlphaC0Offset = iCode * 2;
                if pSliceHeadExt.sSliceHeader.iSliceAlphaC0Offset < SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN
                    || pSliceHeadExt.sSliceHeader.iSliceAlphaC0Offset > SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX
                {
                    return GENERATE_ERROR_NO(
                        ERR_LEVEL_SLICE_HEADER,
                        ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2,
                    );
                }
                if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.sSliceHeader.iSliceBetaOffset = iCode * 2;
                if pSliceHeadExt.sSliceHeader.iSliceBetaOffset < SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN
                    || pSliceHeadExt.sSliceHeader.iSliceBetaOffset > SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX
                {
                    return GENERATE_ERROR_NO(
                        ERR_LEVEL_SLICE_HEADER,
                        ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2,
                    );
                }
            }
        }

        let mut bSgChangeCycleInvolved = pps!().uiNumSliceGroups > 1
            && pps!().uiSliceGroupMapType >= 3
            && pps!().uiSliceGroupMapType <= 5;
        if kbExtensionFlag && bSgChangeCycleInvolved {
            bSgChangeCycleInvolved =
                bSgChangeCycleInvolved && (pNalHeaderExt.uiQualityId == BASE_QUALITY_ID);
        }
        if bSgChangeCycleInvolved {
            if pps!().uiSliceGroupChangeRate > 0 {
                let kiNumBits = ((1 + pps!().uiPicSizeInMapUnits / pps!().uiSliceGroupChangeRate)
                    as f64)
                    .log2()
                    .ceil() as u32;
                if BsGetBits(win!(), pBs, kiNumBits, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.sSliceHeader.iSliceGroupChangeCycle = uiCode as i32;
            } else {
                pSliceHeadExt.sSliceHeader.iSliceGroupChangeCycle = 0;
            }
        }

        if !kbExtensionFlag {
            FillDefaultSliceHeaderExt(pSliceHeadExt, pNalHeaderExt);
        } else {
            // Extra syntax elements newly introduced (G.7.3.3.4). These bits are part of
            // the slice header, so skipping them desynchronises the slice-data parse.
            pSliceHeadExt.subset_sps_id = Some(pps!().iSpsId);
            // The borrow lives across the whole SVC extension parse below, which reads
            // `win!()`/`pBs` and writes `pSliceHeadExt` — three allocations, none of them
            // the context — so nothing here can invalidate it (T5.Z1). The `None` arm is
            // unreachable: the id is the PPS's own `iSpsId`, which `ParsePps` bounds to
            // `< MAX_SPS_COUNT` before the PPS is ever stored.
            let Some(pSubsetSps) = subset_sps_of(&(*pCtx).sSpsPpsCtx, pSliceHeadExt.subset_sps_id)
            else {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SPS_ID);
            };

            if !pNalHeaderExt.bNoInterLayerPredFlag
                && BASE_QUALITY_ID == pNalHeaderExt.uiQualityId
            {
                if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.uiRefLayerDqId = uiCode as u8; //ref_layer_dq_id
                if (*pSubsetSps).sSpsSvcExt.bInterLayerDeblockingFilterCtrlPresentFlag {
                    if BsGetUe(win!(), pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //disable_inter_layer_deblocking_filter_idc
                    pSliceHeadExt.uiDisableInterLayerDeblockingFilterIdc = uiCode;
                    if pSliceHeadExt.uiDisableInterLayerDeblockingFilterIdc > 6 {
                        return GENERATE_ERROR_NO(
                            ERR_LEVEL_SLICE_HEADER,
                            ERR_INFO_INVALID_DBLOCKING_IDC,
                        );
                    }
                    if pSliceHeadExt.uiDisableInterLayerDeblockingFilterIdc != 1 {
                        if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        //inter_layer_slice_alpha_c0_offset_div2
                        pSliceHeadExt.iInterLayerSliceAlphaC0Offset = iCode * 2;
                        if pSliceHeadExt.iInterLayerSliceAlphaC0Offset
                            < SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN
                            || pSliceHeadExt.iInterLayerSliceAlphaC0Offset
                                > SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX
                        {
                            return GENERATE_ERROR_NO(
                                ERR_LEVEL_SLICE_HEADER,
                                ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2,
                            );
                        }
                        if BsGetSe(win!(), pBs, &mut iCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        //inter_layer_slice_beta_offset_div2
                        pSliceHeadExt.iInterLayerSliceBetaOffset = iCode * 2;
                        if pSliceHeadExt.iInterLayerSliceBetaOffset
                            < SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN
                            || pSliceHeadExt.iInterLayerSliceBetaOffset
                                > SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX
                        {
                            return GENERATE_ERROR_NO(
                                ERR_LEVEL_SLICE_HEADER,
                                ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2,
                            );
                        }
                    }
                }

                pSliceHeadExt.uiRefLayerChromaPhaseXPlus1Flag =
                    (*pSubsetSps).sSpsSvcExt.uiSeqRefLayerChromaPhaseXPlus1Flag;
                pSliceHeadExt.uiRefLayerChromaPhaseYPlus1 =
                    (*pSubsetSps).sSpsSvcExt.uiSeqRefLayerChromaPhaseYPlus1;

                if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.bConstrainedIntraResamplingFlag = uiCode != 0;

                {
                    let scaled = &(*pSubsetSps).sSpsSvcExt.sSeqScaledRefLayer;
                    let iLeftOffset = scaled.iLeftOffset;
                    let iTopOffset = scaled.iTopOffset * (2 - sps!().bFrameMbsOnlyFlag as i32);
                    let iRightOffset = scaled.iRightOffset;
                    let iBottomOffset = scaled.iBottomOffset * (2 - sps!().bFrameMbsOnlyFlag as i32);
                    pSliceHeadExt.iScaledRefLayerPicWidthInSampleLuma =
                        (pSliceHeadExt.sSliceHeader.iMbWidth << 4) - (iLeftOffset + iRightOffset);
                    pSliceHeadExt.iScaledRefLayerPicHeightInSampleLuma =
                        (pSliceHeadExt.sSliceHeader.iMbHeight << 4)
                            - (iTopOffset + iBottomOffset) / (1 + pSliceHeadExt.sSliceHeader.bFieldPicFlag as i32);
                }
            } else if pNalHeaderExt.uiQualityId > BASE_QUALITY_ID {
                // MGS not supported.
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MGS);
            } else {
                pSliceHeadExt.uiRefLayerDqId = u8::MAX;
            }

            pSliceHeadExt.bSliceSkipFlag = false;
            pSliceHeadExt.bAdaptiveBaseModeFlag = false;
            pSliceHeadExt.bDefaultBaseModeFlag = false;
            pSliceHeadExt.bAdaptiveMotionPredFlag = false;
            pSliceHeadExt.bDefaultMotionPredFlag = false;
            pSliceHeadExt.bAdaptiveResidualPredFlag = false;
            pSliceHeadExt.bDefaultResidualPredFlag = false;
            pSliceHeadExt.bTCoeffLevelPredFlag = if pNalHeaderExt.bNoInterLayerPredFlag {
                false
            } else {
                (*pSubsetSps).sSpsSvcExt.bSeqTCoeffLevelPredFlag
            };

            if !pNalHeaderExt.bNoInterLayerPredFlag {
                if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.bSliceSkipFlag = uiCode != 0; //slice_skip_flag
                if pSliceHeadExt.bSliceSkipFlag {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_SLICESKIP);
                } else {
                    if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    pSliceHeadExt.bAdaptiveBaseModeFlag = uiCode != 0; //adaptive_base_mode_flag
                    if !pSliceHeadExt.bAdaptiveBaseModeFlag {
                        if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        pSliceHeadExt.bDefaultBaseModeFlag = uiCode != 0; //default_base_mode_flag
                    }
                    if !pSliceHeadExt.bDefaultBaseModeFlag {
                        if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        //adaptive_motion_prediction_flag
                        pSliceHeadExt.bAdaptiveMotionPredFlag = uiCode != 0;
                        if !pSliceHeadExt.bAdaptiveMotionPredFlag {
                            if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                                return ERR_INFO_INVALID_ACCESS;
                            }
                            //default_motion_prediction_flag
                            pSliceHeadExt.bDefaultMotionPredFlag = uiCode != 0;
                        }
                    }

                    if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //adaptive_residual_prediction_flag
                    pSliceHeadExt.bAdaptiveResidualPredFlag = uiCode != 0;
                    if !pSliceHeadExt.bAdaptiveResidualPredFlag {
                        if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        //default_residual_prediction_flag
                        pSliceHeadExt.bDefaultResidualPredFlag = uiCode != 0;
                    }
                }
                if (*pSubsetSps).sSpsSvcExt.bAdaptiveTCoeffLevelPredFlag {
                    if BsGetOneBit(win!(), pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //tcoeff_level_prediction_flag
                    pSliceHeadExt.bTCoeffLevelPredFlag = uiCode != 0;
                }
            }

            if !(*pSubsetSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
                if BsGetBits(win!(), pBs, 4, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.uiScanIdxStart = uiCode as u8; //scan_idx_start
                if BsGetBits(win!(), pBs, 4, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                pSliceHeadExt.uiScanIdxEnd = uiCode as u8; //scan_idx_end
                if pSliceHeadExt.uiScanIdxStart != 0 || pSliceHeadExt.uiScanIdxEnd != 15 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MGS);
                }
            } else {
                pSliceHeadExt.uiScanIdxStart = 0;
                pSliceHeadExt.uiScanIdxEnd = 15;
            }
        }

        ERR_NONE
    }
}

pub fn PrefetchNalHeaderExtSyntax(
    pCtx: &mut SWelsDecoderContext,
    dst_idx: usize,
    kpSrc: &SNalUnit,
) -> bool {
    let Some(kpDst) = cur_au(&mut pCtx.access_unit).and_then(|au| au.node_mut(dst_idx)) else {
        return false;
    };
    let pNalHdrExtS = kpSrc.sNalHeaderExt;
    let pPrefixS = kpSrc.sNalData.sPrefixNal;

    kpDst.sNalHeaderExt.uiDependencyId = pNalHdrExtS.uiDependencyId;
    kpDst.sNalHeaderExt.uiQualityId = pNalHdrExtS.uiQualityId;
    kpDst.sNalHeaderExt.uiTemporalId = pNalHdrExtS.uiTemporalId;
    kpDst.sNalHeaderExt.uiPriorityId = pNalHdrExtS.uiPriorityId;
    kpDst.sNalHeaderExt.bIdrFlag = pNalHdrExtS.bIdrFlag;
    kpDst.sNalHeaderExt.bNoInterLayerPredFlag = pNalHdrExtS.bNoInterLayerPredFlag;
    kpDst.sNalHeaderExt.bDiscardableFlag = pNalHdrExtS.bDiscardableFlag;
    kpDst.sNalHeaderExt.bOutputFlag = pNalHdrExtS.bOutputFlag;
    kpDst.sNalHeaderExt.bUseRefBasePicFlag = pNalHdrExtS.bUseRefBasePicFlag;
    kpDst.sNalHeaderExt.uiLayerDqId = pNalHdrExtS.uiLayerDqId;

    kpDst.sNalData.sVclNal.sSliceHeaderExt.bStoreRefBasePicFlag = pPrefixS.bStoreRefBasePicFlag;
    kpDst.sNalData.sVclNal.sSliceHeaderExt.sRefBasePicMarking = pPrefixS.sRefPicBaseMarking;
    true
}

pub fn UpdateAccessUnit(pCtx: &mut SWelsDecoderContext) -> i32 {
    {
        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return ERR_INFO_INVALID_PTR;
        };
        let iIdx = pCurAu.uiEndPos as usize;
        let dq_id = if iIdx < pCurAu.count() as usize {
            pCurAu.node(iIdx).map(|n| n.sNalHeaderExt.uiLayerDqId)
        } else {
            None
        };
        pCurAu.uiActualUnitsNum = pCurAu.uiEndPos + 1;
        pCurAu.bCompletedAuFlag = true;
        if let Some(dq_id) = dq_id {
            (*pCtx).uiTargetDqId = dq_id;
        }

        // **F46, T5.T2 — the mosaic-avoidance block, never ported** (`decoder_core.cpp:1454`).
        // "Added for mosaic avoidance, 11/19/2009": an access unit that arrives while the
        // decoder is still waiting for a key frame, and that contains no IDR, means the
        // references this AU predicts from are gone — `dsRefLost`. The port stopped at the
        // three assignments above, so **the whole block was absent** and `dsRefLost` had no
        // producer anywhere in the decoder: `iErrorCode |= dsRefLost` appears once
        // (`HandleReferenceLost`) against the C++'s four, and this is the site the corpus
        // reaches. Measured: this block plus the `bParamSetsLostFlag` clear it depends on
        // take the corpus's disagreeing truncation rows from **399 to 188** — 211 rows,
        // one bit on one call.
        //
        // `LONG_TERM_REF` is defined (`decoder_context.h:67`), so the guard is
        // `bParamSetsLostFlag || bNewSeqBegin`. Both trees leave `bParamSetsLostFlag` true
        // from `WelsOpenDecoder` on every non-parse-only path — the only clear is inside
        // `DecodeFrameConstruction`'s `bParseOnly` arm — so the guard is not what was
        // missing and is written here as the C++ writes it rather than folded away.
        let waiting_for_key = (*pCtx).bParamSetsLostFlag || (*pCtx).bNewSeqBegin;
        if waiting_for_key {
            let mut uiActualIdx = 0u32;
            while uiActualIdx < pCurAu.uiActualUnitsNum {
                let Some(nal) = pCurAu.node(uiActualIdx as usize) else {
                    break;
                };
                let hdr = &nal.sNalHeaderExt;
                if hdr.sNalUnitHeader.eNalUnitType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
                    || hdr.bIdrFlag
                {
                    break;
                }
                uiActualIdx += 1;
            }
            if uiActualIdx == pCurAu.uiActualUnitsNum {
                // No IDR in this access unit.
                {
                    let stat = &mut (*pCtx).pDecoderStatistics;
                    stat.uiIDRLostNum += 1;
                }
                if !(*pCtx).bParamSetsLostFlag {
                    WelsLog(
                        std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                        WELS_LOG_WARNING,
                        "UpdateAccessUnit():::::Key frame lost.....CAN NOT find IDR from current AU.",
                    );
                }
                (*pCtx).iErrorCode |= dsRefLost;
                if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
                    (*pCtx).iErrorCode |= dsNoParamSets;
                    return dsNoParamSets;
                }
            }
        }

        ERR_NONE
    }
}

pub fn InitialDqLayersContext(
    pCtx: &mut SWelsDecoderContext,
    kiMaxWidth: i32,
    kiMaxHeight: i32,
) -> i32 {
    {
        if kiMaxWidth <= 0 || kiMaxHeight <= 0 {
            return ERR_INFO_INVALID_PARAM;
        }

        if (*pCtx).bInitialDqLayersMem
            && kiMaxWidth <= (*pCtx).iPicWidthReq
            && kiMaxHeight <= (*pCtx).iPicHeightReq
        {
            return ERR_NONE;
        }

        UninitialDqLayersContext(pCtx);

        // The **allocation's** dimensions, from the negotiated maximum — the layer's
        // `iMbWidth`/`iMbHeight` are the current slice's and are smaller on any stream
        // decoding below it (T5.E2). `pMa` and a separate `numMb` were still declared
        // here for the 25 raw array allocations that died at T5.H3; the grid is the only
        // consumer of this arithmetic now.
        let dims = MbDims::new(
            ((kiMaxWidth + 15) >> 4) as usize,
            ((kiMaxHeight + 15) >> 4) as usize,
        );

        // One layer, and these 27 arrays are now allocated *into* it. They used to be
        // allocated into `SWelsDecoderContext::sMb` and re-aliased onto the layer once per
        // picture by `InitCurDqLayerData`; the alias carried every real access (316 against
        // the cache's 130, of which 129 were lifecycle), so the cache was an owner with no
        // readers and it is gone. The block scopes `pDq`; it was a
        // `for i in 0..LAYER_NUM_EXCHANGEABLE` loop, and that constant was 1.
        // T5.H3: the layer is heap-*constructed*, not zero-allocated — it owns a `MbGrid`
        // and a zeroed `Vec` is an invalid value (S21). The allocation cannot fail by
        // returning null, so the `ERR_INFO_OUT_OF_MEMORY` arm that guarded `WelsMallocz` is
        // gone with it; the 25 array allocations below still go through the C's allocator
        // and are still checked where they were.
        //
        // **T5.R2: the context owns it.** `Box::into_raw` and the `Box::from_raw` in
        // `UninitialDqLayersContext` were F19's last pair in `src/decoder/` — the
        // assignment *is* the ownership transfer now, and the old block scope existed only
        // to bound the raw `pDq`.
        (*pCtx).pDqLayersList = Some(Box::new(DqLayerState::for_grid(dims)));

        (*pCtx).bInitialDqLayersMem = true;
        (*pCtx).iPicWidthReq = kiMaxWidth;
        (*pCtx).iPicHeightReq = kiMaxHeight;
        ERR_NONE
    }
}

pub fn UninitialDqLayersContext(pCtx: &mut SWelsDecoderContext) {
    // T5.E2's `numMb` — the free path's copy of the allocation's dimensions, and the
    // one the closure got wrong — is gone with the raw arrays it sized. The layer's
    // drop glue needs no size at all, which is the whole argument for owning it, and
    // at T5.R2 the drop *is* the assignment: the grid's 22 `Vec`s go with the field's
    // own glue, there is no way to forget one and no size to get wrong.
    (*pCtx).pDqLayersList = None;
    (*pCtx).iPicWidthReq = 0;
    (*pCtx).iPicHeightReq = 0;
    (*pCtx).bInitialDqLayersMem = false;
}

/// **The rotation moves nodes, so the indices that name nodes move with it** (F55).
///
/// `pCtx->pNalCur` and `pCtx->pSliceHeader` were raw pointers *into* a node, and the
/// C's rotation exchanges two entries of a **pointer array** — the node objects do not
/// move, so a pointer that named one before the rotation still names it after. The
/// port's slots own their nodes and the rotation is a `Vec::swap`, which likewise
/// leaves the boxed node where it is; what it does move is the node's **index**, and an
/// index is what [`SWelsDecoderContext::nal_cur`] and
/// [`SWelsDecoderContext::slice_hdr_nal`] hold. Carrying them through the swap is what
/// makes the index spelling mean what the pointer spelling meant.
#[inline]
fn swap_au_nodes(
    pAu: &mut SAccessUnit,
    nal_cur: &mut Option<usize>,
    slice_hdr_nal: &mut Option<usize>,
    a: usize,
    b: usize,
) {
    pAu.nal_units.swap(a, b);
    for idx in [nal_cur, slice_hdr_nal] {
        match *idx {
            Some(i) if i == a => *idx = Some(b),
            Some(i) if i == b => *idx = Some(a),
            _ => {}
        }
    }
}

pub fn ResetCurrentAccessUnit(pCtx: &mut SWelsDecoderContext) {
    // The three fields are disjoint, and spelling them as a split is what lets the
    // rotation carry the two indices (T5.Z1's shape, `swap_au_nodes`' reason).
    let SWelsDecoderContext { access_unit, nal_cur, slice_hdr_nal, .. } = pCtx;
    let Some(pCurAu) = cur_au(access_unit) else {
        return;
    };
    pCurAu.uiStartPos = 0;
    pCurAu.uiEndPos = 0;
    pCurAu.bCompletedAuFlag = false;
    if pCurAu.uiActualUnitsNum > 0 {
        let kuiActualNum = pCurAu.uiActualUnitsNum;
        let kuiAvailNum = pCurAu.uiAvailUnitsNum;
        let kuiLeftNum = if kuiAvailNum > kuiActualNum { kuiAvailNum - kuiActualNum } else { 0 };
        for iIdx in 0..kuiLeftNum as usize {
            // The C swapped two entries of the pointer array; the nodes are owned
            // now, so the same rotation is a `Vec` swap and no node moves.
            swap_au_nodes(
                pCurAu,
                nal_cur,
                slice_hdr_nal,
                kuiActualNum as usize + iIdx,
                iIdx,
            );
        }
        pCurAu.uiActualUnitsNum = kuiLeftNum;
        pCurAu.uiAvailUnitsNum = kuiLeftNum;
    }
}

/// **The two indices travel with the rotation** — `swap_au_nodes`' reason, and why
/// this takes them rather than the access unit alone.
pub fn ForceResetCurrentAccessUnit(
    pAu: &mut SAccessUnit,
    nal_cur: &mut Option<usize>,
    slice_hdr_nal: &mut Option<usize>,
) {
    let mut uiSucAuIdx = pAu.uiEndPos + 1;
    let mut uiCurAuIdx = 0;
    while uiSucAuIdx < pAu.uiAvailUnitsNum {
        swap_au_nodes(
            pAu,
            nal_cur,
            slice_hdr_nal,
            uiSucAuIdx as usize,
            uiCurAuIdx as usize,
        );
        uiSucAuIdx += 1;
        uiCurAuIdx += 1;
    }
    if pAu.uiAvailUnitsNum > pAu.uiEndPos {
        pAu.uiAvailUnitsNum -= pAu.uiEndPos + 1;
    } else {
        pAu.uiAvailUnitsNum = 0;
    }
    pAu.uiActualUnitsNum = 0;
    pAu.uiStartPos = 0;
    pAu.uiEndPos = 0;
    pAu.bCompletedAuFlag = false;
}

// `ForceClearCurrentNal` was declared **here as well as in `nalu.rs`**, and this copy
// had no caller — the second rival pair the access-unit code carried (F39 was the
// first, and that one was live in both directions). It is deleted rather than
// re-exported: the surviving copy takes `&mut SAccessUnit`, so the shapes had already
// diverged and a re-export would have been a new fact rather than a preserved one.

pub fn ForceResetParaSetStatusAndAUList(pCtx: &mut SWelsDecoderContext) {
    (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag = false;

    if let Some(pAu) = cur_au(&mut pCtx.access_unit) {
        pAu.uiAvailUnitsNum = 0;
        pAu.uiActualUnitsNum = 0;
        pAu.uiStartPos = 0;
        pAu.uiEndPos = 0;
        pAu.bCompletedAuFlag = false;
    }
}

pub fn CheckAvailNalUnitsListContinuity(
    pCtx: &mut SWelsDecoderContext,
    iStartIdx: i32,
    iEndIdx: i32,
) {
    {
        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return;
        };
        let mut uiLastNuDependencyId = (*pCurAu.nal(iStartIdx as usize)).sNalHeaderExt.uiDependencyId;
        let mut uiLastNuLayerDqId = (*pCurAu.nal(iStartIdx as usize)).sNalHeaderExt.uiLayerDqId;
        let mut iCurNalUnitIdx = iStartIdx + 1;

        while iCurNalUnitIdx <= iEndIdx {
            let Some(pNal) = pCurAu.node(iCurNalUnitIdx as usize) else {
                return;
            };
            let uiCurNuDependencyId = pNal.sNalHeaderExt.uiDependencyId;
            let uiCurNuQualityId = pNal.sNalHeaderExt.uiQualityId;
            let uiCurNuLayerDqId = pNal.sNalHeaderExt.uiLayerDqId;
            let uiCurNuRefLayerDqId = pNal.sNalData.sVclNal.sSliceHeaderExt.uiRefLayerDqId;

            if uiCurNuDependencyId == uiLastNuDependencyId {
                uiLastNuLayerDqId = uiCurNuLayerDqId;
                iCurNalUnitIdx += 1;
            } else {
                if uiCurNuQualityId == 0 {
                    uiLastNuDependencyId = uiCurNuDependencyId;
                    if uiCurNuRefLayerDqId == uiLastNuLayerDqId {
                        uiLastNuLayerDqId = uiCurNuLayerDqId;
                        iCurNalUnitIdx += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        iCurNalUnitIdx -= 1;
        pCurAu.uiEndPos = iCurNalUnitIdx as u32;
        let dq_id = (*pCurAu.nal(iCurNalUnitIdx as usize)).sNalHeaderExt.uiLayerDqId;
        (*pCtx).uiTargetDqId = dq_id;
    }
}
pub fn RefineIdxNoInterLayerPred(pCurAu: &SAccessUnit, pIdxNoInterLayerPred: &mut i32) {
    {
        // T5.AC11: the out-parameter is a borrow and the node arrives through the AU's
        // shared reader, so this whole function is safe (T5.AC5's `node`).
        let idx = *pIdxNoInterLayerPred as usize;
        let Some(pNal) = pCurAu.node(idx) else {
            return;
        };
        let iLastNalDependId = pNal.sNalHeaderExt.uiDependencyId;
        let iLastNalQualityId = pNal.sNalHeaderExt.uiQualityId;
        let uiLastNalTId = pNal.sNalHeaderExt.uiTemporalId;
        let iLastNalFrameNum = pNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
        let iLastNalPoc = pNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
        let iLastNalFirstMb = pNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

        let mut bMultiSliceFind = false;
        let mut iFinalIdxNoInterLayerPred = 0;
        let mut iCurIdx = (*pIdxNoInterLayerPred) - 1;

        while iCurIdx >= 0 {
            let pCurNal = pCurAu.node(iCurIdx as usize);
            if let Some(pCurNal) = pCurNal.filter(|n| n.sNalHeaderExt.bNoInterLayerPredFlag) {
                let iCurNalDependId = pCurNal.sNalHeaderExt.uiDependencyId;
                let iCurNalQualityId = pCurNal.sNalHeaderExt.uiQualityId;
                let iCurNalTId = pCurNal.sNalHeaderExt.uiTemporalId;
                let iCurNalFrameNum = pCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
                let iCurNalPoc = pCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
                let iCurNalFirstMb = pCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

                if iCurNalDependId == iLastNalDependId
                    && iCurNalQualityId == iLastNalQualityId
                    && iCurNalTId == uiLastNalTId
                    && iCurNalFrameNum == iLastNalFrameNum
                    && iCurNalPoc == iLastNalPoc
                    && iCurNalFirstMb != iLastNalFirstMb
                {
                    bMultiSliceFind = true;
                    iFinalIdxNoInterLayerPred = iCurIdx;
                    iCurIdx -= 1;
                    continue;
                } else {
                    break;
                }
            }
            iCurIdx -= 1;
        }

        if bMultiSliceFind && *pIdxNoInterLayerPred != iFinalIdxNoInterLayerPred {
            *pIdxNoInterLayerPred = iFinalIdxNoInterLayerPred;
        }
    }
}

pub fn CheckPocOfCurValidNalUnits(pCurAu: &SAccessUnit, pIdxNoInterLayerPred: i32) -> bool {
    {
        let iEndIdx = pCurAu.uiEndPos as i32;
        let Some(iCurAuPoc) = pCurAu
            .node(pIdxNoInterLayerPred as usize)
            .map(|n| n.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb)
        else {
            return true;
        };

        for i in (pIdxNoInterLayerPred + 1)..iEndIdx {
            let Some(iTmpPoc) = pCurAu
                .node(i as usize)
                .map(|n| n.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb)
            else {
                continue;
            };
            if iTmpPoc != iCurAuPoc {
                return false;
            }
        }
        true
    }
}

pub fn CheckIntegrityNalUnitsList(pCtx: &mut SWelsDecoderContext) -> bool {
    {
        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return false;
        };
        let kiEndPos = pCurAu.uiEndPos as i32;

        if !pCurAu.bCompletedAuFlag {
            return false;
        }

        if (*pCtx).bNewSeqBegin {
            pCurAu.uiStartPos = 0;
            let mut iIdxNoInterLayerPred = kiEndPos;
            while iIdxNoInterLayerPred >= 0 {
                if (*pCurAu.nal(iIdxNoInterLayerPred as usize)).sNalHeaderExt.bNoInterLayerPredFlag {
                    break;
                }
                iIdxNoInterLayerPred -= 1;
            }
            if iIdxNoInterLayerPred < 0 {
                return false;
            }
            RefineIdxNoInterLayerPred(pCurAu, &mut iIdxNoInterLayerPred);
            pCurAu.uiStartPos = iIdxNoInterLayerPred as u32;

            // `CheckAvailNalUnitsListContinuity` derives the access unit itself and writes
            // `uiEndPos` through its own borrow, so everything below re-derives. This is
            // the shape the raw field made invisible: the old `pCurAu` stayed usable
            // afterwards precisely because nothing owned the thing it pointed at.
            CheckAvailNalUnitsListContinuity(pCtx, iIdxNoInterLayerPred, kiEndPos);

            let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
                return false;
            };
            if !CheckPocOfCurValidNalUnits(pCurAu, iIdxNoInterLayerPred) {
                return false;
            }
            let endIdx = pCurAu.uiEndPos as usize;
            let pEndNal = pCurAu.nal(endIdx);
            (*pCtx).iCurSeqIntervalTargetDependId = (*pEndNal).sNalHeaderExt.uiDependencyId as i32;
            (*pCtx).iCurSeqIntervalMaxPicWidth = (*pEndNal)
                .sNalData
                .sVclNal
                .sSliceHeaderExt
                .sSliceHeader
                .iMbWidth
                << 4;
            (*pCtx).iCurSeqIntervalMaxPicHeight = (*pEndNal)
                .sNalData
                .sVclNal
                .sSliceHeaderExt
                .sSliceHeader
                .iMbHeight
                << 4;
        }
        true
    }
}

pub fn CheckOnlyOneLayerInAu(pCtx: &mut SWelsDecoderContext) {
    {
        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return;
        };
        let iEndIdx = pCurAu.uiEndPos as usize;
        let mut iCurIdx = pCurAu.uiStartPos as usize;
        let uiDId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiDependencyId;
        let uiQId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiQualityId;
        let uiTId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiTemporalId;

        (*pCtx).bOnlyOneLayerInCurAuFlag = true;
        if iEndIdx == iCurIdx {
            return;
        }
        iCurIdx += 1;
        while iCurIdx <= iEndIdx {
            let uiCurDId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiDependencyId;
            let uiCurQId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiQualityId;
            let uiCurTId = (*pCurAu.nal(iCurIdx)).sNalHeaderExt.uiTemporalId;
            if uiDId != uiCurDId || uiQId != uiCurQId || uiTId != uiCurTId {
                (*pCtx).bOnlyOneLayerInCurAuFlag = false;
                return;
            }
            iCurIdx += 1;
        }
    }
}

pub fn WelsDecodeAccessUnitStart(pCtx: &mut SWelsDecoderContext) -> i32 {
    {
        let iRet = { UpdateAccessUnit(pCtx) };
        if iRet != ERR_NONE {
            return iRet;
        }
        if let Some(au) = cur_au(&mut pCtx.access_unit) {
            au.uiStartPos = 0;
        }
        if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag && !{ CheckIntegrityNalUnitsList(pCtx) } {
            (*pCtx).iErrorCode |= dsBitstreamError;
            { return dsBitstreamError; }
        }
        if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag {
            { CheckOnlyOneLayerInAu(pCtx) };
        }
        ERR_NONE
    }
}

pub fn WelsDecodeAccessUnitEnd(pCtx: &mut SWelsDecoderContext) {
    {
        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return;
        };
        let endIdx = pCurAu.uiEndPos as usize;
        if endIdx < pCurAu.count() as usize {
            let pCurNal = pCurAu.nal(endIdx);
            let last = (
                pCurNal.sNalHeaderExt,
                pCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader,
            );
            {
                let info = &mut pCtx.pLastDecPicInfo;
                info.sLastNalHdrExt = last.0;
                info.sLastSliceHeader = last.1;
            }
        }
        ResetCurrentAccessUnit(pCtx);
    }
}

pub fn CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx: &mut SWelsDecoderContext) -> bool {
    {
        let mut bNewSeq = false;
        let mut pTmpLayerSps: [Option<SpsRef>; MAX_LAYER_NUM] = [None; MAX_LAYER_NUM];

        let Some(pCurAu) = cur_au(&mut pCtx.access_unit) else {
            return false;
        };
        let start = pCurAu.uiStartPos as usize;
        let end = pCurAu.uiEndPos as usize;
        for i in start..=end {
            if let Some(pNal) = pCurAu.node(i) {
                let uiDid = pNal.sNalHeaderExt.uiDependencyId as usize;
                if uiDid < MAX_LAYER_NUM {
                    pTmpLayerSps[uiDid] = sps_ref_of(
                        &(*pCtx).sSpsPpsCtx,
                        pNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref,
                    );
                }
                if pNal.sNalHeaderExt.sNalUnitHeader.eNalUnitType == NAL_UNIT_CODED_SLICE_IDR
                    || pNal.sNalHeaderExt.bIdrFlag
                {
                    bNewSeq = true;
                }
            }
        }

        let mut iMaxActiveLayer = 0;
        let mut iMaxCurrentLayer = 0;
        for i in (0..MAX_LAYER_NUM).rev() {
            if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_some() {
                iMaxActiveLayer = i;
                break;
            }
        }
        for i in (0..MAX_LAYER_NUM).rev() {
            if pTmpLayerSps[i].is_some() {
                iMaxCurrentLayer = i;
                break;
            }
        }
        if iMaxCurrentLayer != iMaxActiveLayer
            || pTmpLayerSps[iMaxCurrentLayer] != (*pCtx).sSpsPpsCtx.pActiveLayerSps[iMaxActiveLayer]
        {
            bNewSeq = true;
        }
        if !bNewSeq {
            for i in 0..MAX_LAYER_NUM {
                if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_none() && pTmpLayerSps[i].is_some() {
                    (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = pTmpLayerSps[i];
                }
            }
        } else {
            (*pCtx).sSpsPpsCtx.pActiveLayerSps.copy_from_slice(&pTmpLayerSps);
        }
        bNewSeq
    }
}

pub fn WriteBackActiveParameters(pCtx: &mut SWelsDecoderContext) {
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_PPS) != 0 {
        let ppsId = (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT].iPpsId as usize;
        if ppsId < MAX_PPS_COUNT {
            (*pCtx).sSpsPpsCtx.sPpsBuffer[ppsId] = (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT];
        }
    }
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SPS) != 0 {
        let spsId = (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT].iSpsId as usize;
        if spsId < MAX_SPS_COUNT {
            (*pCtx).sSpsPpsCtx.sSpsBuffer[spsId] = (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT];
            (*pCtx).bNewSeqBegin = true;
        }
    }
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SUBSETSPS) != 0 {
        let spsId = (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT].sSps.iSpsId as usize;
        if spsId < MAX_SPS_COUNT {
            (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[spsId] = (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT];
            (*pCtx).bNewSeqBegin = true;
        }
    }
    (*pCtx).sSpsPpsCtx.iOverwriteFlags = OVERWRITE_NONE;
}

pub fn DecodeFinishUpdate(pCtx: &mut SWelsDecoderContext) {
    (*pCtx).bNewSeqBegin = false;
    WriteBackActiveParameters(pCtx);
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || (*pCtx).bNextNewSeqBegin;
    (*pCtx).bNextNewSeqBegin = false;
    if (*pCtx).bNewSeqBegin {
        ResetActiveSPSForEachLayer(pCtx);
    }
}

pub fn WelsDecodeInitAccessUnitStart(
    pCtx: &mut SWelsDecoderContext,
    pDstInfo: &mut SBufferInfo,
) -> i32 {
    (*pCtx).bAuReadyFlag = false;
    {
        let info = &mut pCtx.pLastDecPicInfo;
        info.bLastHasMmco5 = false;
    }
    let bTmpNewSeqBegin = CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx);
    if bTmpNewSeqBegin {
        // `decoder_core.cpp:2265`'s `if (pCtx->pStreamSeqNum) (*pCtx->pStreamSeqNum)++;
        // else pCtx->iSeqNum++;`. **T8.A6 retires the else arm with the pointer**: the
        // counter is the context's own field now, so the null the C tested for cannot
        // occur and the arm it guarded is unreachable code, not a fallback.
        pCtx.pStreamSeqNum += 1;
    }
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || bTmpNewSeqBegin;
    (*pCtx).iSeqNum = pCtx.pStreamSeqNum;
    let iErr = WelsDecodeAccessUnitStart(pCtx);
    GetVclNalTemporalId(pCtx);

    if iErr != ERR_NONE {
        let SWelsDecoderContext { access_unit, nal_cur, slice_hdr_nal, .. } = &mut *pCtx;
        if let Some(au) = cur_au(access_unit) {
            ForceResetCurrentAccessUnit(au, nal_cur, slice_hdr_nal);
        }
        if !pCtx.pParam.bParseOnly {
            pDstInfo.iBufferStatus = 0;
        }
        (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || (*pCtx).bNextNewSeqBegin;
        (*pCtx).bNextNewSeqBegin = false;
        if (*pCtx).bNewSeqBegin {
            ResetActiveSPSForEachLayer(pCtx);
        }
        return iErr;
    }

    // Derived here, not at the head: `CheckNewSeqBeginAndUpdateActiveLayerSps` and
    // `WelsDecodeAccessUnitStart` both derive the access unit in between, and the
    // second of them moves `uiStartPos`. Hoisting this was legal only while the field
    // was a raw pointer into memory nothing owned.
    let pNal = match cur_au(&mut pCtx.access_unit) {
        Some(au) => au.node(au.uiStartPos as usize).map(|nal| {
            (
                nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref,
                nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pps_id,
            )
        }),
        _ => None,
    };
    if let Some((sps_ref, pps_id)) = pNal {
        (*pCtx).active_sps = sps_ref;
        (*pCtx).active_pps = pps_id;
    }
    iErr
}

pub fn AllocPicBuffOnNewSeqBegin(pCtx: &mut SWelsDecoderContext) -> i32 {
    {
        // T5.R6: the fallback scan now yields the *id* of the first initialized entry —
        // the same entry its `sps as *mut SSps` named, without the alias.
        let active = if (*pCtx).active_sps.is_some() {
            (*pCtx).active_sps
        } else {
            (*pCtx)
                .sSpsPpsCtx
                .sSpsBuffer
                .iter()
                .position(|sps| sps.uiTotalMbCount > 0)
                .map(|i| SpsRef { id: i as i32, subset: false })
        };

        // The dimensions as values: `WelsResetRefPic` and `SyncPictureResolutionExt`
        // both re-enter through the context (T5.Z1).
        let Some((iMbWidth, iMbHeight)) =
            sps_of(&(*pCtx).sSpsPpsCtx, active).map(|sps| (sps.iMbWidth, sps.iMbHeight))
        else {
            return ERR_INFO_INVALID_PTR;
        };
        (*pCtx).active_sps = active;

        if GetThreadCount(pCtx) <= 1 {
            WelsResetRefPic(pCtx);
        }
        let iErr = SyncPictureResolutionExt(pCtx, iMbWidth as u32, iMbHeight as u32);
        iErr
    }
}

pub fn InitConstructAccessUnit(
    pCtx: &mut SWelsDecoderContext,
    pDstInfo: &mut SBufferInfo,
) -> i32 {
    let mut iErr = { WelsDecodeInitAccessUnitStart(pCtx, pDstInfo) };
    if iErr != ERR_NONE {
        return iErr;
    }
    if (*pCtx).bNewSeqBegin {
        iErr = AllocPicBuffOnNewSeqBegin(pCtx);
        if iErr != ERR_NONE {
            return iErr;
        }
    }
    iErr
}

pub fn ConstructAccessUnit(
    pCtx: &mut SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) -> i32 {
    if GetThreadCount(pCtx) <= 1 {
        let iErr = InitConstructAccessUnit(pCtx, pDstInfo);
        if iErr != ERR_NONE {
            return iErr;
        }
    }
    // T5.O2: the CABAC engine's lazy allocation stood here — allocate on the first
    // access unit, null-check twice, carry an out-of-memory arm. The engine is a
    // field now, zeroed with the context, which is the state that allocation
    // produced on its one execution.

    let iErr = { DecodeCurrentAccessUnit(pCtx, ppDst, pDstInfo) };
    { WelsDecodeAccessUnitEnd(pCtx) };
    iErr
}

/// Core bitstream decoding loop that demultiplexes Annex B NAL units and decodes them into an access unit.
/// Matches `int32_t WelsDecodeBs (PWelsDecoderContext pCtx, const uint8_t* kpBsBuf, const int32_t kiBsLen, uint8_t** ppDst, SBufferInfo* pDstBufInfo, SParserBsInfo* pDstBsInfo)` in `decoder.cpp:741`.
pub fn WelsDecodeBs(
    pCtx: &mut SWelsDecoderContext,
    kpBsBuf: &[u8],
    kiBsLen: i32,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
    _pDstBsInfo: *mut c_void,
) -> i32 {
    pDstInfo.iBufferStatus = 0;

    if !kpBsBuf.is_empty() && kiBsLen > 0 {
        (*pCtx).bEndOfStreamFlag = false;
        let input_slice = &kpBsBuf[..kiBsLen as usize];
        let units = crate::split_annexb_units(input_slice);

        // **F48, T5.U3 — a buffer with no start code is an error, not an empty
        // loop.** The C++ opens with `DetectStartCodePrefix`, and its *return
        // value* is a verdict, not just an offset:
        //
        //     if (NULL == DetectStartCodePrefix (kpBsBuf, &iOffset, kiBsLen)) {
        //       pCtx->iErrorCode |= dsBitstreamError;
        //       return dsBitstreamError;      // decoder.cpp:760
        //     }
        //
        // The port reached the same *decoding* by iterating `split_annexb_units`,
        // which yields nothing for such a buffer — so the loop body ran zero times
        // and the function fell out reporting `dsErrorFree`. The transliteration
        // deleted `DetectStartCodePrefix` as dead at T3.3 (S18: it had no callers)
        // and the error signal went with it, because the caller only ever used the
        // offset. Nothing could see it: the four rows it moves are `raw.*` feeds of
        // one to three bytes, which is the family that got its first C++ referee in
        // this commit.
        if units.is_empty() {
            (*pCtx).iErrorCode |= dsBitstreamError;
            return dsBitstreamError;
        }

        // The raw-data buffer can be rewound once no pending NAL units
        // reference it (slices stay queued until their access unit completes).
        if !au_has_nals(pCtx) {
            (*pCtx).sRawData.rewind();
        }

        for (_u_i, unit) in units.iter().enumerate() {
            let mut payload_slice = *unit;
            if payload_slice.starts_with(&[0, 0, 0, 1]) {
                payload_slice = &payload_slice[4..];
            } else if payload_slice.starts_with(&[0, 0, 1]) {
                payload_slice = &payload_slice[3..];
            }
            // **`kpSrcNal`, T8b.B2** — the *escaped* NAL, start code included, which
            // is what parse-only hands back to its caller and the one thing
            // `sRawData` cannot supply (it holds the RBSP). The reference passes
            // `pSrcNal - 3` with length `iSrcIdx + 3` (`decoder.cpp:815`, `:877`):
            // the three-byte start-code form, whichever form the stream used, which
            // is why both parse-only writers open by prepending the missing `0x00`.
            // Trailing zeros belonging to the next start code are inside this window
            // in the reference too, and both trees trim them the same way.
            let src_nal: &[u8] = if unit.starts_with(&[0, 0, 0, 1]) { &unit[1..] } else { unit };
            // **F46, T5.T3 — an empty NAL is a NAL.** This used to `continue`. The C++
            // has no such skip: a start code with nothing behind it reaches
            // `ParseNalHeader` with `iSrcRbspLen == 0`, whose header byte then reads
            // out of the **four reserved zero bytes** the scanner writes at the write
            // position before every parse (`decoder.cpp:875`, `:874`). That is
            // `nal_unit_type` 0 — `NAL_UNIT_UNSPEC_0` — with no SPS ahead of it, so
            // the C++ answers `dsNoParamSets` where the port answered `dsErrorFree`.
            // The four zeroes are written below rather than assumed: they are also the
            // guard bytes the refill predicate is allowed to touch past an RBSP end
            // (F4/P6), and `sRawData` is reused across access units, so "the buffer
            // starts zeroed" stops being true after the first rewind.

            // Copy the NAL into the persistent raw-data buffer, stripping
            // emulation-prevention bytes (00 00 03 -> 00 00), as the C++
            // WelsDecodeBs start-code scanner does.
            if (*pCtx).sRawData.remaining() < payload_slice.len() + 4 {
                // Wrap to the buffer head like the C++ scanner; the buffer is
                // sized for several access units, so pending NAL data (near
                // the current write position) is not overwritten.
                (*pCtx).sRawData.rewind();
                if (*pCtx).sRawData.len() < payload_slice.len() + 4 {
                    // ExpandBsBuffer's policy, now RawDataBuffer::grow. Offsets —
                    // the write position, every pending reader's `start`/cursor —
                    // survive the reallocation by definition, so the pointer
                    // rebasing block is gone rather than converted, and the saved
                    // buffer keeps `sRawData`'s size as before.
                    if (*pCtx).sRawData.grow(payload_slice.len()).is_err() {
                        (*pCtx).iErrorCode |= dsOutOfMemory;
                        return (*pCtx).iErrorCode;
                    }
                    if (*pCtx).pParam.bParseOnly
                        && (*pCtx).sSavedData.grow_to((*pCtx).sRawData.len()).is_err()
                    {
                        (*pCtx).iErrorCode |= dsOutOfMemory;
                        return (*pCtx).iErrorCode;
                    }
                    (*pCtx).sRawData.rewind();
                }
            }
            let (payload_start, payload_len) = (*pCtx).sRawData.append_ebsp_stripped(payload_slice);
            (*pCtx).sRawData.zero_reserved(payload_start + payload_len);

            let mut consumed_bytes = 0i32;
            let mut nal_header = crate::decoder::nalu::SNalUnitHeader::default();
            let p_payload = crate::decoder::nalu::ParseNalHeader(
                pCtx,
                &mut nal_header,
                payload_start,
                payload_len as i32,
                src_nal,
                &mut consumed_bytes,
            );

            if let Some(nal_start) = p_payload {
                let nal_type = nal_header.eNalUnitType;
                if crate::decoder::nalu::IS_PARAM_SETS_NALS(nal_type) {
                    crate::decoder::nalu::ParseNonVclNal(
                        pCtx,
                        nal_start,
                        (payload_len as i32) - consumed_bytes,
                        src_nal,
                    );
                }
                CheckAndFinishLastPic(pCtx, ppDst, pDstInfo);
                // Decode a completed access unit as soon as the parser marks
                // the boundary, matching `WelsDecodeBs` in `decoder_core.cpp`.
                // (`ConstructAccessUnit` runs frame construction internally.)
                if (*pCtx).bAuReadyFlag && au_has_nals(pCtx) {
                    ConstructAccessUnit(pCtx, ppDst, pDstInfo);
                }
            }
            DecodeFinishUpdate(pCtx);
        }
    } else if (*pCtx).bEndOfStreamFlag {
        // End of stream: flush the pending (final) access unit.
        // Not `mark_au_ready`: the flush ends the access unit without setting
        // `bAuReadyFlag`, because it is about to decode it here rather than wait for
        // the parser to say so.
        let bHasPending = match cur_au(&mut pCtx.access_unit) {
            Some(au) if au.uiAvailUnitsNum > 0 => {
                au.uiEndPos = au.uiAvailUnitsNum - 1;
                true
            }
            _ => false,
        };
        if bHasPending {
            ConstructAccessUnit(pCtx, ppDst, pDstInfo);
        }
        DecodeFinishUpdate(pCtx);
    }
    (*pCtx).iErrorCode
}

/// **T5.P′1 dropped `pPicDec`.** It wrote `pDqLayer->pDec`, the layer's copy of
/// `dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec)` — a cache with one stamp site (this one) and no way for a reader
/// to observe it diverging from its source, which is what W2b's S23 check
/// established. The readers derive; the parameter had nothing left to write.
pub fn InitDqLayerInfo(
    pCtx: &mut SWelsDecoderContext,
    pDqLayer: Option<&mut DqLayerState>,
    pLayerInfo: &SLayerInfo,
    pNalUnit: Option<&SNalUnit>,
) {
    {
        let Some(pDqLayer) = pDqLayer else {
            return;
        };
        // F24's shape, third site (T5.E1). `pSh` was a `&mut` *inside* `pShExt`'s `&mut`,
        // so every `(*pShExt)` read below popped it and every later `(*pSh)` read was UB;
        // worse, the four escaping borrows in the `kuiQualityId` block store pointers into
        // the layer that outlive this function, and as `&mut` they died at the next use of
        // their parent. Raw derivations from `pNalUnit` carry the NAL unit's provenance and
        // nothing pops anything.
        // T5b.3: the node is a borrow, so the three `addr_of_mut!` chains — S29's
        // spelling for a *raw* parent — are ordinary field paths, and the four
        // "escaping borrows" the note above worried about are the layer's own copies.
        let Some(pNalUnit) = pNalUnit else {
            return;
        };
        let pNalHdrExt = &pNalUnit.sNalHeaderExt;
        let pShExt = &pNalUnit.sNalData.sVclNal.sSliceHeaderExt;
        let pSh = &pShExt.sSliceHeader;
        let kuiQualityId = pNalHdrExt.uiQualityId;

        pDqLayer.sLayerInfo = *pLayerInfo;
        pDqLayer.iMbWidth = pSh.iMbWidth;
        pDqLayer.iMbHeight = pSh.iMbHeight;
        pDqLayer.iSliceIdcBackup = (pSh.iFirstMbInSlice << 7)
            | ((pNalHdrExt.uiDependencyId as i32) << 4)
            | (pNalHdrExt.uiQualityId as i32);

        if let Some(iPpsId) = pLayerInfo.pps_id {
            // T5.R6: the id *is* what the pointer was carried for here.
            pDqLayer.uiPpsId = iPpsId as u32;
        }
        pDqLayer.uiDisableInterLayerDeblockingFilterIdc = pShExt.uiDisableInterLayerDeblockingFilterIdc;
        pDqLayer.iInterLayerSliceAlphaC0Offset = pShExt.iInterLayerSliceAlphaC0Offset;
        pDqLayer.iInterLayerSliceBetaOffset = pShExt.iInterLayerSliceBetaOffset;
        pDqLayer.iSliceGroupChangeCycle = pSh.iSliceGroupChangeCycle;
        pDqLayer.bStoreRefBasePicFlag = pShExt.bStoreRefBasePicFlag;
        pDqLayer.bTCoeffLevelPredFlag = pShExt.bTCoeffLevelPredFlag;
        pDqLayer.bConstrainedIntraResamplingFlag = pShExt.bConstrainedIntraResamplingFlag;
        pDqLayer.uiRefLayerDqId = pShExt.uiRefLayerDqId;
        pDqLayer.uiRefLayerChromaPhaseXPlus1Flag = pShExt.uiRefLayerChromaPhaseXPlus1Flag;
        pDqLayer.uiRefLayerChromaPhaseYPlus1 = pShExt.uiRefLayerChromaPhaseYPlus1;
        pDqLayer.bUseWeightPredictionFlag = false;
        pDqLayer.bUseWeightedBiPredIdc = false;

        if kuiQualityId == BASE_QUALITY_ID {
            // **T5.AC2: the four `addr_of_mut!`s are three copies and a deletion.** Each
            // addressed a field of `(*pSh)` / `(*pShExt)` — the very slice header
            // `pDqLayer.sLayerInfo = *pLayerInfo` copied whole at the top of this
            // function — so the value is taken here instead of the address. The
            // assignment stays inside this `kuiQualityId` block, which is what keeps
            // the retention rule the pointers carried: at a quality-enhancement slice
            // nothing writes these three and they still read the base slice's.
            //
            // Nothing wrote *through* two of the three (the marking and the
            // reordering syntax are read-only below this point); the third,
            // `sPredWeightTable`, is written by `CreateImplicitWeightTable`, whose
            // writes now land in the layer's own copy and are read back through it —
            // the NAL's copy had no other reader.
            pDqLayer.sRefPicListReordering = Some(pSh.pRefPicListReordering);
            pDqLayer.sRefPicMarking = Some(pSh.sRefMarking);
            if let Some((bWeightedPredFlag, uiWeightedBipredIdc)) =
                pps_of(&(*pCtx).sSpsPpsCtx, pSh.pps_id)
                    .map(|pps| (pps.bWeightedPredFlag, pps.uiWeightedBipredIdc))
            {
                pDqLayer.bUseWeightPredictionFlag = bWeightedPredFlag;
                pDqLayer.bUseWeightedBiPredIdc = uiWeightedBipredIdc != 0;
                if bWeightedPredFlag || uiWeightedBipredIdc != 0 {
                    pDqLayer.sPredWeightTable = Some(pSh.sPredWeightTable);
                }
            }
        }
        pDqLayer.uiLayerDqId = pNalHdrExt.uiLayerDqId;
        pDqLayer.bUseRefBasePicFlag = pNalHdrExt.bUseRefBasePicFlag;
    }
}

/// **The parameter sets arrive as ids** (T5.Z1). They used to arrive as pointers
/// the caller derived from the context it passes beside them, which is the shape
/// this face removes; resolved here, at the one line that reads them, nothing else
/// of the context is borrowed.
pub fn WelsDqLayerDecodeStart(
    pCtx: &mut SWelsDecoderContext,
    nal_idx: Option<usize>,
    sps_ref: Option<SpsRef>,
    pps_id: Option<i32>,
) {
    // T5b.3: the two scalars this lifted out of the slice header travel as values.
    // `pCtx->pSliceHeader` — the raw alias it used to stamp — is deleted; the NAL
    // under decode is `pCtx->nal_cur`, an index, and its one reader resolves it.
    let Some(nal_idx) = nal_idx else {
        return;
    };
    let Some((eSliceType, iFrameNum)) = pCtx
        .access_unit
        .as_deref()
        .and_then(|au| au.node(nal_idx))
        .map(|nal| {
            let sh = &nal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
            (sh.eSliceType, sh.iFrameNum)
        })
    else {
        return;
    };
    pCtx.eSliceType = eSliceType;
    pCtx.bUsedAsRef = false;
    pCtx.iFrameNum = iFrameNum;
    // `pCtx->pSliceHeader = pSh`'s replacement: the node, by index.
    pCtx.slice_hdr_nal = Some(nal_idx);
    // The three arguments are three disjoint fields of the context: the statistics,
    // owned since T8.A6, and both parameter sets out of `sSpsPpsCtx`.
    let SWelsDecoderContext { pDecoderStatistics, sSpsPpsCtx, .. } = &mut *pCtx;
    UpdateDecoderStatisticsForActiveParaset(
        Some(pDecoderStatistics),
        sps_of(sSpsPpsCtx, sps_ref),
        pps_of(sSpsPpsCtx, pps_id),
    );
}

pub fn InitRefPicList(
    pCtx: &mut SWelsDecoderContext,
    mut pCurDqLayer: Option<&mut DqLayerState>,
    _kuiNRi: u8,
    iPoc: i32,
) -> i32 {
    {
        let mut iRet = if (*pCtx).eSliceType == B_SLICE {
            let ret = WelsInitBSliceRefList(pCtx, pCurDqLayer.as_deref_mut(), iPoc);
            { CreateImplicitWeightTable(pCtx, pCurDqLayer.as_deref_mut()) };
            ret
        } else {
            WelsInitRefList(pCtx, pCurDqLayer.as_deref_mut(), iPoc)
        };
        if (*pCtx).eSliceType != I_SLICE && (*pCtx).eSliceType != SI_SLICE {
            if active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                .is_some_and(|sps| sps.uiProfileIdc != 66)
                && active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
                    .is_some_and(|pps| pps.bEntropyCodingModeFlag)
            {
                iRet = WelsReorderRefList2(pCtx, pCurDqLayer.as_deref_mut());
            } else {
                iRet = WelsReorderRefList(pCtx, pCurDqLayer.as_deref_mut());
            }
        }
        iRet
    }
}

pub fn DecodeCurrentAccessUnit(
    pCtx: &mut SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) -> i32 {
    let (iIdx, iEndIdx) = match cur_au(&mut pCtx.access_unit) {
        None => return ERR_INFO_INVALID_PTR,
        Some(au) => (au.uiStartPos as i32, au.uiEndPos as i32),
    };
    let iThreadCount = GetThreadCount(pCtx);

    let kuiTargetLayerDqId = GetTargetDqId((*pCtx).uiTargetDqId, &(*pCtx).pParam);
    let kuiDependencyIdMax = (kuiTargetLayerDqId & 0x7F) >> 4;
    (*pCtx).uiNalRefIdc = 0;

    // Node pointers only, one derivation each. The slice loop below calls back into
    // the context on every iteration, so nothing about the access unit may be held
    // across it — but a node is its own allocation and outlives every retag (T5.O4).
    // **T5b.3: the index travels, not the node.** This local was a raw pointer held
    // across the whole slice loop, which calls back into the context — the second half
    // of what kept the access unit's slots raw. It is `Some(iIdx)` while `iIdx` names a
    // NAL and `None` past the end, which is what the null tests below were reading.
    if cur_au(&mut pCtx.access_unit).is_none() {
        return ERR_INFO_INVALID_PTR;
    }
    let pNalCur: Option<usize> = Some(iIdx as usize);
    (*pCtx).nal_cur = pNalCur;

    // **The access-unit bracket owns the layer** (T5.AA1). It is `slice_split`'s
    // maneuver one container over, and it takes the other of the two available
    // shapes: instead of splitting the borrow, the bracket *moves* the layer out of
    // the context for the call and puts it back below. That is what makes the dozen
    // calls under it — every one of which takes `pCtx` beside the layer — safe code
    // with no round trip through a pointer.
    //
    // The move is invisible to everything under here, and the check is a grep rather
    // than an argument: `pDqLayersList` has exactly two readers in the decoder, this
    // bracket and `CheckAndFinishLastPic`'s, and one writer, `InitialDqLayersContext`,
    // which runs in `InitConstructAccessUnit` *above* this function and never under
    // it.
    //
    // **The labelled block is the restore's guarantee**, and it is why this is one
    // function rather than two: every path out of the loop — the five `break 'au`s
    // that were `return`s — lands on the line that puts the layer back, and no new
    // signature has to repeat the two output parameters to get there.
    let mut owned_layer = pCtx.pDqLayersList.take();
    let mut dq_cur = owned_layer.as_deref_mut();
    let mut pNalCur = pNalCur;
    let mut iIdx = iIdx;
    let iRet = 'au: {
    let mut iRet;
    let mut bAllRefComplete = true;
    let mut iLastIdD: i16 = -1;
    let mut iLastIdQ: i16 = -1;
    let mut iLastSliceFrameNum: i32 = 0;
    let mut bFreshSliceAvailable;

    while iIdx <= iEndIdx {
        let mut pLayerInfo = SLayerInfo::default();
        // **F96.** `decoder_core.cpp:2538-2541`:
        //
        // ```c
        // bool isNewFrame = true;
        // if (iThreadCount > 1) {
        //   isNewFrame = pCtx->pDec == NULL;
        // }
        // ```
        //
        // The port had only the assignment, without the `iThreadCount > 1` that
        // guards it — so on a continuing frame (`pDec` already prefetched) it read
        // `false` where the reference reads `true`, and the one thing this flag
        // gates, `InitRefPicList` at `:4627`, was **skipped**.
        //
        // That is the whole of F96. On `res/Error_I_P.264` under `ERROR_CON_DISABLE`
        // the reference calls `InitRefPicList` on two P-slice access units whose
        // reference list is empty; `WelsReorderRefList`'s search finds nothing,
        // falls out at `i < 0` and returns `ERR_INFO_REFERENCE_PIC_LOST`
        // (`manage_dec_ref.cpp:462`) — the one of its four failure returns that sets
        // no `iErrorCode`, which is why the reference reports a bare `dsRefLost`
        // (0x2) there. The arm below then returns early under DISABLE. The port
        // never entered it, decoded the slice instead, and failed it with
        // `dsBitstreamError` (0x4).
        //
        // `GetThreadCount` returns 0 here (F36 fences the threaded decoder), so this
        // reads `true` in every configuration the port can be in today. It is
        // written as the C++ writes it because the condition is the fact, and a
        // `true` literal would lose why.
        let isNewFrame = if iThreadCount > 1 { (*pCtx).pDec.is_none() } else { true };

        if (*pCtx).pDec.is_none() {
            // The prefetch hands back the slot it landed on, which is what this field
            // holds; `pic_slot(prefetched)` stood here and read the same value back
            // out of the picture's stamp. `None` is the pool being empty or fully
            // held, which is the arm below.
            (*pCtx).pDec = match pic_pool_mut(pCtx) {
                Some(pool) => pool.prefetch_free(),
                None => None,
            };
            // `decoder_core.cpp:2568-2569` — a fresh picture starts from zero
            // recorded macroblocks, and the zeroing precedes the null check because
            // the C's does. Without it a count left over from a dropped access unit
            // (EC disabled, refs lost) accumulates across frames, and
            // `ResetActiveSPSForEachLayer` — gated on `iTotalNumMbRec == 0` in both
            // trees — never fires again (F93's root).
            if (*pCtx).iTotalNumMbRec != 0 {
                (*pCtx).iTotalNumMbRec = 0;
            }
            if (*pCtx).pDec.is_none() {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return ERR_INFO_REF_COUNT_OVERFLOW;
            }
            let bNewSeqBegin = (*pCtx).bNewSeqBegin;
            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                pDec.bNewSeqBegin = bNewSeqBegin;
            }
        } else if (*pCtx).iTotalNumMbRec == 0 {
            // `decoder_core.cpp:2588-2590` — a picture already prefetched but not yet
            // started re-takes the flag ("pDec != NULL, already start").
            let bNewSeqBegin = (*pCtx).bNewSeqBegin;
            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                pDec.bNewSeqBegin = bNewSeqBegin;
            }
        }

        // The two stamps travel as values: the picture is the pool's borrow and
        // both sources are read before it opens (T5.Z1).
        let uiTimeStamp = pNalCur
            .and_then(|i| pCtx.access_unit.as_deref().and_then(|au| au.node(i)))
            .map(|nal| nal.uiTimeStamp);
        let uiDecodingTimeStamp = (*pCtx).uiDecodingTimeStamp as u32;
        if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
            if let Some(uiTimeStamp) = uiTimeStamp {
                pDec.uiTimeStamp = uiTimeStamp;
            }
            pDec.uiDecodingTimeStamp = uiDecodingTimeStamp;
        }


        if (*pCtx).iTotalNumMbRec == 0 {
            // Picture starts to decode: reset per-picture MB state, matching
            // `DecodeCurrentAccessUnit` in `decoder_core.cpp`.
            let iMbCacheNum =
                ((((*pCtx).iPicWidthReq + 15) >> 4) * (((*pCtx).iPicHeightReq + 15) >> 4)) as usize;
            // This re-derived from the list mid-loop where everything around it read
            // the cache; both were this iteration's `dq_cur`, and now it says so.
            if let Some(pDq) = dq_cur.as_deref_mut() {
                // `memset(pSliceIdc, 0xff, numMb * sizeof(int32_t))` — 0xff bytes in
                // an `i32` is -1. `iMbCacheNum` is computed from `iPicWidthReq`, which
                // `InitialDqLayersContext` sets to the same `kiMaxWidth` the grid's
                // dimensions come from, so the bound is an identity; spelling it as a
                // slice makes it a checked one (P13), where the C had no bound at all.
                pDq.grid.slice_idc.as_mut_slice()[..iMbCacheNum].fill(-1);
            }
            if let Some(iMbNum) = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                .map(|sps| (sps.iMbWidth * sps.iMbHeight) as usize)
            {
                if let Some(pDq) = dq_cur.as_deref_mut() {
                    pDq.grid.mb_correctly_decoded_flag.as_mut_slice()
                        [..iMbNum]
                        .fill(false);
                    // The C's `memset(.., 0, iMbWidth * iMbHeight)` over the
                    // **SPS's** dimensions, which are the current sequence's and can
                    // be smaller than the grid's negotiated maximum (T5.E2). As a
                    // slice the bound is checked; as a `write_bytes` it was not.
                    pDq.grid.mb_ref_concealed_flag.as_mut_slice()[..iMbNum]
                        .fill(false);
                }
                if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                    pDec.iMbNum = iMbNum as i32;
                }
            }
            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                pDec.pRefPic[LIST_0] = [None; MAX_DPB_COUNT];
                pDec.pRefPic[LIST_1] = [None; MAX_DPB_COUNT];
                pDec.iMbEcedNum = 0;
                pDec.iMbEcedPropNum = 0;
            }
        }

        (*pCtx).bRPLRError = false;
        // T5.X8: `GetI4LumaIChromaAddrTable` filled `iDecBlockOffsetArray` here,
        // once per access unit, because the 24 byte offsets it held depended on the
        // picture's two strides. The reconstruction path addresses blocks by sample
        // coordinate now (`decode_slice.rs`'s `blk4_xy`), so there is no table.

        if nal_hdr(pCtx, pNalCur).is_some_and(|h| h.uiLayerDqId > kuiTargetLayerDqId) {
            break;
        }

        while iIdx <= iEndIdx {
            if pNalCur.is_none() || dq_cur.is_none() {
                break;
            }
            // **T5b.3: the node's fields are copied out once, and nothing is held.**
            // The C++ declares `pSh`/`pShExt` at function scope and re-derives them
            // per iteration; the port spelled that with `addr_of_mut!` because the
            // node was raw. It is a borrow now, so the shape that works is the one
            // T5.Z1 already uses everywhere else — read what this iteration needs, in
            // one expression, then let the borrow end before the calls that re-enter
            // the context.
            let Some(nal) = pNalCur
                .and_then(|i| pCtx.access_unit.as_deref().and_then(|au| au.node(i)))
            else {
                break;
            };
            let hdr_ext = nal.sNalHeaderExt;
            let pShExt = nal.sNalData.sVclNal.sSliceHeaderExt;
            let pSh = pShExt.sSliceHeader;
            let bSliceHeaderExtFlag = nal.sNalData.sVclNal.bSliceHeaderExtFlag;

            let iCurrIdQ = hdr_ext.uiQualityId as i16;
            let iCurrIdD = hdr_ext.uiDependencyId as i16;
            // The C++'s `pSh` outlives the slice loop and names the *last* slice
            // header, which is what the frame_num update below wants; the one field
            // that outlives the iteration is carried out by value.
            iLastSliceFrameNum = pSh.iFrameNum;
            (*pCtx).bRPLRError = false;
            let bReconstructSlice =
                CheckSliceNeedReconstruct(hdr_ext.uiLayerDqId, kuiTargetLayerDqId);

            pLayerInfo.sNalHeaderExt = hdr_ext;
            let stamp = (pSh.iFrameNum, pSh.iPicOrderCntLsb, hdr_ext.bIdrFlag, pSh.eSliceType);
            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                pDec.iFrameNum = stamp.0;
                pDec.iFramePoc = stamp.1;
                pDec.bIdrFlag = stamp.2;
                pDec.eSliceType = stamp.3;
            }

            pLayerInfo.sSliceInLayer.sSliceHeaderExt = pShExt;
            pLayerInfo.sSliceInLayer.bSliceHeaderExtFlag = bSliceHeaderExtFlag;
            pLayerInfo.sSliceInLayer.eSliceType = pSh.eSliceType as u8;

            pLayerInfo.sSliceInLayer.iLastMbQp = pSh.iSliceQp;
            // **T5.M3 — where `dq_cur->pBitStringAux` was written.** The layer mirrored
            // `&pNalCur->sNalData.sVclNal.sSliceBitsRead` here and every reader in
            // `decode_slice.rs` and `parse_mb_syn_cabac.rs` went through the mirror;
            // `decoder_context::slice_bit_reader` derives it from `pNalCur` instead, so the
            // one thing that has to be true is that **this** field is as fresh as the
            // mirror was.
            //
            // It was not. `(*pCtx).pNalCur` was written **once**, before the loop
            // (`:3573`), while `pNalCur` itself is re-read at the bottom of the inner
            // loop — so from the second slice NAL of any access unit onward the
            // context's copy pointed at the first NAL while the layer's mirror pointed
            // at the right one. Nothing read the stale copy (its one reader is
            // `WelsDecodeAndConstructSlice`, F36's dead `iThreadCount > 1` arm), which
            // is why five phases of byte-exact gates never saw it. The write moves
            // here, to the statement the mirror occupied, and the field is now correct
            // by the same construction the mirror was.
            //
            // The C++ has no counterpart to correct: `decoder_core.cpp:2491` sets
            // `pCtx->pNalCur = NULL` and never writes it again, so its own
            // `decode_slice.cpp:1621` reader — the same dead MT arm — reads NULL. The
            // port already diverged by writing the first NAL; this makes the divergence
            // *useful* instead of merely different, and F36 owns the arm either way.
            (*pCtx).nal_cur = Some(iIdx as usize);

            (*pCtx).uiNalRefIdc = hdr_ext.sNalUnitHeader.uiNalRefIdc;
            let iPpsId = pSh.iPpsId;
            pLayerInfo.pps_id = pSh.pps_id;
            pLayerInfo.sps_ref = pSh.sps_ref;
            pLayerInfo.subset_sps_id = pShExt.subset_sps_id;

            // **FMO activation** (`decoder_core.cpp:2651-2663`), restored at T5.S1.
            // Nothing wrote `pFmo` in this port — F43's other half — so `fmo.rs` was
            // unreachable in production and every multi-slice-group stream decoded in
            // raster order through the deleted `FmoNextMb` stub. The id is what the C
            // indexes `sFmoList` with, and the slice header parse has already rejected
            // `iPpsId >= MAX_PPS_COUNT` (`:2155`), so the entry always exists.
            //
            // `FmoParamUpdate` rebuilds the map only when the PPS's slice-group
            // parameters changed (`FmoParamSetsChanged`), which is why the state is
            // per-PPS and kept across access units rather than per slice.
            (*pCtx).fmo_id = Some(iPpsId);
            // **Three disjoint fields of one context, in one call** (T5.Z1). This
            // is the site session Y's Miri verdict could not repair site-locally:
            // two accessor results derived from `pCtx` and passed together, each
            // popping the other's derivation. Spelled as the fields they actually
            // reach, the disjointness is the compiler's to check.
            let SWelsDecoderContext { sFmoList, sSpsPpsCtx, iActiveFmoNum, .. } = &mut *pCtx;
            iRet = FmoParamUpdate(
                fmo_of_mut(sFmoList, Some(iPpsId)),
                sps_of(sSpsPpsCtx, pLayerInfo.sps_ref),
                pps_of(sSpsPpsCtx, pLayerInfo.pps_id),
                iActiveFmoNum,
            );
            if iRet != ERR_NONE {
                if iRet == ERR_INFO_OUT_OF_MEMORY {
                    (*pCtx).iErrorCode |= dsOutOfMemory;
                    WelsLog(
                        std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                        WELS_LOG_ERROR,
                        "DecodeCurrentAccessUnit(), Fmo param alloc failed",
                    );
                } else {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                    WelsLog(
                        std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                        WELS_LOG_WARNING,
                        "DecodeCurrentAccessUnit(), FmoParamUpdate failed",
                    );
                }
                break 'au GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_FMO_INIT_FAIL);
            }

            bFreshSliceAvailable = iCurrIdD != iLastIdD || iCurrIdQ != iLastIdQ;
            WelsDqLayerDecodeStart(pCtx, pNalCur, pLayerInfo.sps_ref, pLayerInfo.pps_id);

            if iLastIdD < 0 || iLastIdD == iCurrIdD {
                let nal_copy = pNalCur
                    .and_then(|i| pCtx.access_unit.as_deref().and_then(|au| au.node(i)))
                    .copied();
                InitDqLayerInfo(pCtx, dq_cur.as_deref_mut(), &mut pLayerInfo, nal_copy.as_ref());

                // **F46, T5.T2 — subclause 8.2.5.2, gaps in `frame_num`**
                // (`decoder_core.cpp:2675`), the second of the C++'s four `dsRefLost`
                // sites and also never ported. A non-IDR slice whose `frame_num` is
                // neither the previous one nor its successor means frames went missing
                // in transmission, so the pictures this one predicts from are gone.
                // This is the site `BA_MW_D_P_LOST` reaches — the stream is a lost P
                // frame, which is precisely a `frame_num` gap.
                // Two values, read above the block that logs and writes through the
                // context (T5.Z1) — and the layer's three with them, because the block
                // both reads the layer's NAL header and writes the context.
                let dq_layer_info = dq_cur.as_deref().map(|dq| {
                    (
                        dq.sLayerInfo.sps_ref,
                        dq.sLayerInfo.sNalHeaderExt.bIdrFlag,
                        dq.sLayerInfo.sNalHeaderExt.sNalUnitHeader.eNalUnitType,
                    )
                });
                let dq_sps = dq_layer_info
                    .and_then(|(sps_ref, _, _)| sps_of(&(*pCtx).sSpsPpsCtx, sps_ref))
                    .map(|sps| (sps.bGapsInFrameNumValueAllowedFlag, sps.uiLog2MaxFrameNum));
                if let (Some((false, uiLog2MaxFrameNum)), Some((_, bIdrFlag, eNalUnitType))) =
                    (dq_sps, dq_layer_info)
                {
                    let kbIdrFlag = bIdrFlag
                        || eNalUnitType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
                    // `pLastThreadCtx` is the multi-threaded arm's `GetPrevFrameNum`
                    // detour; `GetThreadCount` is identically 0 here (F36 owns the
                    // threading gap), so the C++'s single-threaded read is the whole
                    // of it.
                    let iPrevFrameNum =
                        pCtx.pLastDecPicInfo.iPrevFrameNum;
                    let wrap = (1i32 << uiLog2MaxFrameNum) - 1;
                    if !kbIdrFlag
                        && pSh.iFrameNum != iPrevFrameNum
                        && pSh.iFrameNum != ((iPrevFrameNum + 1) & wrap)
                    {
                        WelsLog(
                            std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                            WELS_LOG_WARNING,
                            "referencing pictures lost due frame gaps exist",
                        );
                        bAllRefComplete = false;
                        (*pCtx).iErrorCode |= dsRefLost;
                        if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE
                        {
                            (*pCtx).bParamSetsLostFlag = true;
                            break 'au GENERATE_ERROR_NO(
                                ERR_LEVEL_SLICE_HEADER,
                                ERR_INFO_REFERENCE_PIC_LOST,
                            );
                        }
                    }
                }

                if iCurrIdD == (kuiDependencyIdMax as i16) && iCurrIdQ == (BASE_QUALITY_ID as i16) && isNewFrame {
                    iRet = InitRefPicList(
                        pCtx,
                        dq_cur.as_deref_mut(),
                        (*pCtx).uiNalRefIdc,
                        pSh.iPicOrderCntLsb,
                    );
                    if iRet != ERR_NONE {
                        (*pCtx).bRPLRError = true;
                        bAllRefComplete = false;
                        let h = nal_hdr(pCtx, pNalCur).copied();
                        HandleReferenceLost(pCtx, h.as_ref());
                        // `decoder_core.cpp:2713`, ported with the fix it explains
                        // rather than ahead of it (F96): it is a behaviour change on
                        // the trace surface, and it is the line that named this arm
                        // when the reference was run at `--trace=8`.
                        WelsLog(
                            std::ptr::addr_of_mut!((*pCtx).sLogCtx),
                            WELS_LOG_DEBUG,
                            &format!(
                                "reference picture introduced by this frame is lost during transmission! uiTId: {}",
                                h.map_or(0, |hdr| hdr.uiTemporalId)
                            ),
                        );
                        if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
                            if (*pCtx).iTotalNumMbRec == 0 {
                                (*pCtx).pDec = None;
                            }
                            break 'au iRet;
                        }
                    }
                }

                if pSh.eSliceType == B_SLICE && pSh.iDirectSpatialMvPredFlag == 0 {
                    ComputeColocatedTemporalScaling(pCtx, dq_cur.as_deref_mut());
                }

                // `DECODER_MT(incomplete: F36)` — this arm is unreachable
                // (`GetThreadCount` returns 0) and its callee is a partial translation
                // of the reference. See `decode_slice::WelsDecodeAndConstructSlice` for
                // the list of statements that are missing and what turning this on
                // would require. T7.C7 left it and fenced it.
                if iThreadCount > 1 {
                    iRet = WelsDecodeAndConstructSlice(pCtx, dq_cur.as_deref_mut());
                } else {
                    iRet = WelsDecodeSlice(pCtx, dq_cur.as_deref_mut(), bFreshSliceAvailable, pNalCur);
                }

                if iRet != ERR_NONE {
                    bAllRefComplete = false;
                    let h = nal_hdr(pCtx, pNalCur).copied();
                    HandleReferenceLostL0(pCtx, h.as_ref());
                    if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
                        if (*pCtx).iTotalNumMbRec == 0 {
                            (*pCtx).pDec = None;
                        }
                        break 'au iRet;
                    }
                }

                if iThreadCount <= 1 && bReconstructSlice {
                    iRet = WelsDecodeConstructSlice(pCtx, dq_cur.as_deref_mut(), pNalCur);
                    if iRet != ERR_NONE {
                        if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                            pDec.bIsComplete = false;
                        }
                        break 'au iRet;
                    }
                }

                if bAllRefComplete && (*pCtx).eSliceType != I_SLICE {
                    if iThreadCount <= 1 {
                        if (*pCtx).sRefPic.uiRefCount[LIST_0] > 0 {
                            bAllRefComplete =
                                bAllRefComplete && CheckRefPicturesComplete(pCtx, dq_cur.as_deref());
                        } else {
                            bAllRefComplete = false;
                        }
                    }
                }
            }

            iLastIdD = iCurrIdD;
            iLastIdQ = iCurrIdQ;

            iIdx += 1;
            pNalCur = if iIdx <= iEndIdx { Some(iIdx as usize) } else { None };

            match nal_hdr(pCtx, pNalCur) {
                Some(h)
                    if iLastIdD == (h.uiDependencyId as i16)
                        && iLastIdQ == (h.uiQualityId as i16) => {}
                _ => break,
            }
        }

        // The C++ code runs the completion/frame-construction block below even
        // when all NAL units are consumed (pNalCur == NULL); only a missing DQ
        // layer aborts here.
        if dq_cur.is_none() {
            break;
        }

        if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
            pDec.bIsComplete = bAllRefComplete;
        }
        if (*pCtx).pDec.is_some() && !bAllRefComplete {
            (*pCtx).iErrorCode |= dsDataErrorConcealed;
        }

        if dq_cur.as_deref().is_some_and(|dq| dq.uiLayerDqId == kuiTargetLayerDqId) {
            if !(*pCtx).bInstantDecFlag {
                if !(*pCtx).pParam.bParseOnly {
                    if NeedErrorCon(pCtx, dq_cur.as_deref_mut())
                        && ec_active_idc(&(*pCtx).pParam) != ERROR_CON_DISABLE
                    {
                        ImplementErrorCon(pCtx, dq_cur.as_deref_mut());
                        // Values first, then the picture: `dec_pic` borrows the
                        // pool and the ids are the parameter sets' (T5.Z1).
                        let sps_dims_id = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                            .map(|sps| (sps.iMbWidth, sps.iMbHeight, sps.iSpsId));
                        let pps_id = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
                            .map(|pps| pps.iPpsId);
                        if let Some((iMbWidth, iMbHeight, iSpsId)) = sps_dims_id {
                            (*pCtx).iTotalNumMbRec = (iMbWidth * iMbHeight) as i32;
                            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                                pDec.iSpsId = iSpsId;
                            }
                        }
                        if let Some(iPpsId) = pps_id {
                            if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                                pDec.iPpsId = iPpsId;
                            }
                        }
                    }
                }
            }

            iRet = DecodeFrameConstruction(pCtx, dq_cur.as_deref(), ppDst, pDstInfo);
            if iRet != ERR_NONE {
                break 'au iRet;
            }

            let dec = pCtx.pDec;
            {
                let info = &mut pCtx.pLastDecPicInfo;
                info.pPreviousDecodedPictureInDpb = dec;
            }
            (*pCtx).bUsedAsRef = (*pCtx).uiNalRefIdc > 0;
            if iThreadCount <= 1 {
                if (*pCtx).bUsedAsRef {
                    // Snapshot this picture's own reference lists onto the picture.
                    // MapColToList0 reads them back off the colocated picture when a
                    // later B slice uses temporal direct mode; without this the lookup
                    // always misses and every mapped ref index collapses to 0.
                    //
                    // **T5.P′2: a handle-to-handle copy.** Both sides are `Option<PicId>`
                    // now, so the snapshot that used to duplicate up to 34 raw aliases
                    // into the pool — onto a *pooled picture*, for as long as it stays a
                    // reference — reaches the pool exactly once, for `pDec` itself.
                    // The handles are copied out of `sRefPic` first (T5.Z1): the
                    // destination borrows `pPicBuff` and the source is another field
                    // of the same context, so the copy is between two values.
                    let kpRefList = (*pCtx).sRefPic.pRefList;
                    if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                        for listIdx in LIST_0..LIST_A {
                            let mut i = 0usize;
                            while i < MAX_DPB_COUNT && kpRefList[listIdx][i].is_some() {
                                pDec.pRefPic[listIdx][i] = kpRefList[listIdx][i];
                                i += 1;
                            }
                        }
                    }
                    iRet = WelsMarkAsRef(pCtx, dq_cur.as_deref_mut());
                    if iRet != ERR_NONE {
                        if iRet == ERR_INFO_DUPLICATE_FRAME_NUM {
                            (*pCtx).iErrorCode |= dsBitstreamError;
                        }
                        if (*pCtx).pParam.eEcActiveIdc == ERROR_CON_DISABLE {
                            (*pCtx).pDec = None;
                            break 'au iRet;
                        }
                    }
                    if !(*pCtx).pParam.bParseOnly && (*pCtx).pDec.is_some() {
                        if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                            pDec.expand_as_reference();
                        }
                    }
                }
            }
            (*pCtx).pDec = None;

            // **F46, T5.T2 — "need update frame_num due current frame is well
            // decoded"** (`decoder_core.cpp:2864`). The port had the *other* writer of
            // `iPrevFrameNum` — `CheckAndFinishLastPic`'s, off `sLastSliceHeader` — and
            // not this one, which is the writer on the ordinary path where an access
            // unit decodes to completion. With only half the state machine the frame
            // number the gap test compares against went stale after the first frame,
            // so the gap test above would report a gap on **every** subsequent access
            // unit rather than on the one that actually skipped a frame.
            let bStartNalIsRef = cur_au(&mut pCtx.access_unit)
                .and_then(|au| au.node(au.uiStartPos as usize))
                .is_some_and(|nal| nal.sNalHeaderExt.sNalUnitHeader.uiNalRefIdc > 0);
            if bStartNalIsRef {
                {
                    let info = &mut pCtx.pLastDecPicInfo;
                    info.iPrevFrameNum = iLastSliceFrameNum;
                }
            }
            {
                let info = &mut pCtx.pLastDecPicInfo;
                if info.bLastHasMmco5 {
                    info.iPrevFrameNum = 0;
                }
            }
        }

        if pNalCur.is_none() {
            break;
        }
    }
    ERR_NONE
    };
    pCtx.pDqLayersList = owned_layer;
    iRet
}

pub fn CheckAndFinishLastPic(
    pCtx: &mut SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) -> bool {
    if (*pCtx).access_unit.is_none() {
        return false;
    }
    let mut bAuBoundaryFlag = false;

    if IS_VCL_NAL((*pCtx).sCurNalHead.eNalUnitType, 1) {
        let sps_ref = match pCtx.access_unit.as_deref() {
            Some(au) => au
                .node(au.uiEndPos as usize)
                .and_then(|n| n.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.sps_ref),
            None => return false,
        };
        let cur_nal = pCtx
            .access_unit
            .as_deref()
            .and_then(|au| au.node(au.uiEndPos as usize))
            .copied();
        // The last-picture halves are copied out first: the field borrow and
        // `sps_of`'s borrow of `sSpsPpsCtx` are both arguments to one call.
        let last = Some((pCtx.pLastDecPicInfo.sLastNalHdrExt, pCtx.pLastDecPicInfo.sLastSliceHeader));
        if let (Some(pCurNal), Some((last_hdr, last_sh))) = (cur_nal.as_ref(), last) {
            bAuBoundaryFlag = (*pCtx).iTotalNumMbRec != 0
                && crate::decoder::nalu::CheckAccessUnitBoundaryExt(
                    sps_of(&(*pCtx).sSpsPpsCtx, sps_ref),
                    &last_hdr,
                    &pCurNal.sNalHeaderExt,
                    &last_sh,
                    &pCurNal.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader,
                );
        }
    } else {
        if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_AU_DELIMITER
            || (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SEI
        {
            bAuBoundaryFlag = true;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SPS) != 0;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SUBSET_SPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SUBSETSPS) != 0;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_PPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_PPS) != 0;
        }
        if bAuBoundaryFlag && au_has_nals(pCtx) {
            ConstructAccessUnit(pCtx, ppDst, pDstInfo);
        }
    }

    // **The error-concealment bracket** (T5.R1, T5.AA1): this runs *between* access
    // units — `ConstructAccessUnit` above may have just returned — so it takes its own
    // derivation of the layer rather than inheriting one. It is the value the deleted
    // cache field held at this point, and the list is what the cache was stamped from.
    // The layer is *moved out* of the context for the block, exactly as
    // `DecodeCurrentAccessUnit`'s bracket moves it, because every call below takes
    // `pCtx` beside it; the restore is at the single exit.
    let mut owned_layer = pCtx.pDqLayersList.take();
    let mut dq_cur = owned_layer.as_deref_mut();
    let bRet = 'ec: {
    if bAuBoundaryFlag && (*pCtx).iTotalNumMbRec != 0 && NeedErrorCon(pCtx, dq_cur.as_deref_mut()) {
        if (*pCtx).pParam.eEcActiveIdc != ERROR_CON_DISABLE {
            ImplementErrorCon(pCtx, dq_cur.as_deref_mut());
            // Values first, then the picture — see `ConstructAccessUnit` (T5.Z1).
            let sps_dims_id = active_sps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_sps)
                .map(|sps| (sps.iMbWidth, sps.iMbHeight, sps.iSpsId));
            let pps_id = active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps).map(|pps| pps.iPpsId);
            if let Some((iMbWidth, iMbHeight, iSpsId)) = sps_dims_id {
                (*pCtx).iTotalNumMbRec = (iMbWidth * iMbHeight) as i32;
                if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                    pDec.iSpsId = iSpsId;
                }
            }
            if let Some(iPpsId) = pps_id {
                if let Some(pDec) = dec_pic(&mut (*pCtx).pPicBuff, (*pCtx).pDec) {
                    pDec.iPpsId = iPpsId;
                }
            }
            DecodeFrameConstruction(pCtx, dq_cur.as_deref(), ppDst, pDstInfo);
            let dec = pCtx.pDec;
            {
                let info = &mut pCtx.pLastDecPicInfo;
                info.pPreviousDecodedPictureInDpb = dec;
                if info.sLastNalHdrExt.sNalUnitHeader.uiNalRefIdc > 0 {
                    if MarkECFrameAsRef(pCtx, dq_cur.as_deref_mut()) == ERR_INFO_INVALID_PTR {
                        (*pCtx).iErrorCode |= dsRefListNullPtrs;
                        break 'ec false;
                    }
                }
            }
        } else if (*pCtx).pParam.bParseOnly {
            let pParser = parser_bs(&mut (*pCtx).pParserBsInfo);
            if let Some(pParser) = pParser {
                pParser.iNalNum = 0;
            }
            (*pCtx).bFrameFinish = true;
        } else {
            if DecodeFrameConstruction(pCtx, dq_cur.as_deref(), ppDst, pDstInfo) != ERR_NONE {
                if {
                    pCtx.pLastDecPicInfo.sLastNalHdrExt.sNalUnitHeader.uiNalRefIdc > 0
                        && pCtx.pLastDecPicInfo.sLastNalHdrExt.uiTemporalId == 0
                }
                {
                    (*pCtx).iErrorCode |= dsNoParamSets;
                } else {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                }
                (*pCtx).pDec = None;
                break 'ec false;
            }
        }
        (*pCtx).pDec = None;
        // Re-derived: `ConstructAccessUnit` ran above, and it decodes.
        let bStartNalIsRef = cur_au(&mut pCtx.access_unit)
            .and_then(|au| au.node(au.uiStartPos as usize))
            .is_some_and(|nal| nal.sNalHeaderExt.sNalUnitHeader.uiNalRefIdc > 0);
        if bStartNalIsRef {
            {
                let info = &mut pCtx.pLastDecPicInfo;
                info.iPrevFrameNum = info.sLastSliceHeader.iFrameNum;
            }
        }
        {
            let info = &mut pCtx.pLastDecPicInfo;
            if info.bLastHasMmco5 {
                info.iPrevFrameNum = 0;
            }
        }
    }
    true
    };
    pCtx.pDqLayersList = owned_layer;
    bRet
}

pub fn CheckRefPicturesComplete(
    pCtx: &mut SWelsDecoderContext,
    pCurDqLayer: Option<&DqLayerState>,
) -> bool {
    {
        let Some(pCurDqLayer) = pCurDqLayer else {
            return true;
        };
        // **A bracket** (T5.Q2): this scan reads the current picture's macroblock types
        // and reference indices *while* resolving the reference-list entries those
        // indices name, and a malformed stream can put the current picture in that list
        // (F42). One borrow, split — the same shape the slice brackets take, over a
        // whole-slice operation rather than a whole slice.
        let (pDec, pRefs) = cur_and_refs(&mut pCtx.pPicBuff, pCtx.pDec);
        // T5b.2: the picture is a borrow, and the whole scan reads it — so F42's arm
        // resolves through `PicRefs::resolve` and the two shared borrows coexist.
        let Some(pDec) = pDec.map(|p| &*p) else {
            return true;
        };
        if pDec.pMbType.as_slice().is_empty() {
            return true;
        }
        let mut bAllRefComplete = true;
        let mut iRealMbIdx = pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
        let totalMb = pCurDqLayer.sLayerInfo.sSliceInLayer.iTotalMbInCurSlice;

        for iMbIdx in 0..totalMb {
            let mbType = *pDec.pMbType.get(iRealMbIdx as usize);
            match mbType {
                MB_TYPE_SKIP | MB_TYPE_16x16 => {
                    let refIdx = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                    if refIdx < MAX_REF_PIC_COUNT {
                        if let Some(pRef) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx), Some(pDec)) {
                            bAllRefComplete = bAllRefComplete && pRef.bIsComplete;
                        }
                    }
                }
                MB_TYPE_16x8 => {
                    let refIdx0 = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                    let refIdx1 = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[8] as usize;
                    if refIdx0 < MAX_REF_PIC_COUNT {
                        if let Some(pRef0) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx0), Some(pDec)) {
                            bAllRefComplete = bAllRefComplete && pRef0.bIsComplete;
                        }
                    }
                    if refIdx1 < MAX_REF_PIC_COUNT {
                        if let Some(pRef1) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx1), Some(pDec)) {
                            bAllRefComplete = bAllRefComplete && pRef1.bIsComplete;
                        }
                    }
                }
                MB_TYPE_8x16 => {
                    let refIdx0 = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                    let refIdx1 = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[2] as usize;
                    if refIdx0 < MAX_REF_PIC_COUNT {
                        if let Some(pRef0) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx0), Some(pDec)) {
                            bAllRefComplete = bAllRefComplete && pRef0.bIsComplete;
                        }
                    }
                    if refIdx1 < MAX_REF_PIC_COUNT {
                        if let Some(pRef1) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx1), Some(pDec)) {
                            bAllRefComplete = bAllRefComplete && pRef1.bIsComplete;
                        }
                    }
                }
                MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
                    let indices = [0, 2, 8, 10];
                    for &sub in &indices {
                        let refIdx = (*pDec.pRefIndex[0].get(iRealMbIdx as usize))[sub] as usize;
                        if refIdx < MAX_REF_PIC_COUNT {
                            if let Some(pRef) = pRefs.resolve(ref_id(&pCtx.sRefPic, LIST_0, refIdx), Some(pDec)) {
                                bAllRefComplete = bAllRefComplete && pRef.bIsComplete;
                            }
                        }
                    }
                }
                _ => {}
            }
            if !bAllRefComplete {
                break;
            }
            iRealMbIdx = if active_pps(&(*pCtx).sSpsPpsCtx, (*pCtx).active_pps)
                .is_some_and(|pps| pps.uiNumSliceGroups > 1)
            {
                FmoNextMb(active_fmo(&(*pCtx).sFmoList, (*pCtx).fmo_id), iRealMbIdx)
            } else {
                pCurDqLayer.sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice + iMbIdx + 1
            };
            if iRealMbIdx == -1 {
                return false;
            }
        }
        bAllRefComplete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // T5.H2 — `MbGrid`'s raw bridge, and S28's instrument.
    //
    // These are the tests S28 exists for, and the only ones in the battery that
    // can fail on the defect it names. Byte-exactness cannot: the wrong
    // derivation produces the identical address. Run under Miri they are the
    // proof that a converted family may keep handing pointers to a kernel.
    // -----------------------------------------------------------------------

    /// The grid's per-list arrays and `decoder_context::LIST_A` are the same
    /// number. `safe/mb_grid.rs` depends on nothing, so it declares its own
    /// `LIST_COUNT`; this is the one place both names are in scope, which makes
    /// it the place the identity is checked rather than assumed.
    #[test]
    fn mb_grid_list_count_matches_list_a() {
        assert_eq!(LIST_COUNT, LIST_A);
        let g = MbGrid::new(MbDims::new(2, 2));
        assert_eq!(g.mv.len(), LIST_A);
        assert_eq!(g.mvd.len(), LIST_A);
        assert_eq!(g.ref_index.len(), LIST_A);
    }

    /// **S21's discharge for T5.H3.** The layer's construction is a zeroed shell
    /// with the one owning field written through it, so: every C-defaulted field
    /// still reads zero, the two the C++ constructor overwrites read their
    /// overwritten values, and the grid is a real grid rather than 22 null `Vec`s.
    ///
    /// Under Miri this is also the test that would fail if the shell were ever
    /// materialized before the grid was written into it.
    #[test]
    fn for_grid_constructs_a_layer_whose_grid_is_valid_and_whose_rest_is_zero() {
        {
            let dims = MbDims::new(5, 3);
            let layer = DqLayerState::for_grid(dims);

            // the owned field
            assert_eq!(layer.grid.dims(), dims);
            assert_eq!(layer.grid.mb_type.as_slice().len(), dims.count());
            assert!(layer.grid.scaled_tcoeff.as_slice().iter().all(|mb| mb.iter().all(|&c| c == 0)));

            // the two the C++ constructor overwrites
            assert_eq!(layer.uiRefLayerDqId, 255);
            assert_eq!(layer.uiRefLayerChromaPhaseYPlus1, 1);

            // and a sample of what `WelsMallocz`'s zeroing used to leave behind
            assert_eq!(layer.iMbWidth, 0);
            assert_eq!(layer.iMbHeight, 0);
            assert!(!layer.bUseWeightPredictionFlag);
            assert_eq!(layer.uiRefLayerChromaPhaseXPlus1Flag, 0);
        }
    }

    /// The grid is sized from the **allocation's** dimensions, and the layer's
    /// `iMbWidth`/`iMbHeight` are the current slice's — T5.E2's correction, now
    /// structural rather than a comment on a `numMb` expression.
    #[test]
    fn the_grid_outlives_a_narrower_slice() {
        {
            let mut layer = DqLayerState::for_grid(MbDims::from_pixels(1920, 1080));
            assert_eq!(layer.grid.dims().count(), 120 * 68);
            // a stream decoding below the negotiated maximum
            layer.iMbWidth = 11;
            layer.iMbHeight = 9;
            assert_eq!(
                layer.grid.mb_type.as_slice().len(),
                120 * 68,
                "the grid is the allocation's, not the slice's"
            );
        }
    }

    #[test]
    fn test_update_dec_stat_null_layer() {
        {
            {
                // T5.Z4: the null-*context* arm is gone with the raw parameter; the
                // null-*layer* arm is what these two still answer.
                let mut ctx = SWelsDecoderContext::new_boxed();
                UpdateDecStatNoFreezingInfo(&mut ctx, None);
                UpdateDecStat(&mut ctx, None, true);
            }
        }
    }

    #[test]
    fn test_update_dec_stat_freezing() {
        {
            {
                let mut stat = SDecoderStatistics::default();
                UpdateDecStatFreezingInfo(true, &mut stat);
                assert_eq!(stat.uiFreezingIDRNum, 1);
                assert_eq!(stat.uiFreezingNonIDRNum, 0);
                UpdateDecStatFreezingInfo(false, &mut stat);
                assert_eq!(stat.uiFreezingNonIDRNum, 1);
            }
        }
    }

    #[test]
    fn test_reset_dec_stat_nums() {
        {
            {
                let mut stat = SDecoderStatistics::default();
                stat.uiWidth = 1920;
                stat.uiHeight = 1080;
                stat.iAvgLumaQp = 26;
                stat.uiProfile = 66;
                stat.uiLevel = 31;
                stat.uiDecodedFrameCount = 100;
                stat.uiIDRCorrectNum = 5;
                ResetDecStatNums(&mut stat);
                assert_eq!(stat.uiWidth, 1920);
                assert_eq!(stat.uiHeight, 1080);
                assert_eq!(stat.iAvgLumaQp, 26);
                assert_eq!(stat.uiProfile, 66);
                assert_eq!(stat.uiLevel, 31);
                assert_eq!(stat.uiDecodedFrameCount, 0);
                assert_eq!(stat.uiIDRCorrectNum, 0);
            }
        }
    }

    #[test]
    fn test_inline_delegation_stubs_null_layer() {
        {
            {
                let mut ctx = SWelsDecoderContext::new_boxed();
                let pCtx = &mut *ctx;
                assert_eq!(
                    WelsTargetSliceConstruction(pCtx, None),
                    ERR_NONE
                );
                assert_eq!(
                    WelsDecodeSlice(pCtx, None, true, None),
                    ERR_NONE
                );
                assert_eq!(
                    WelsDecodeAndConstructSlice(pCtx, None),
                    ERR_NONE
                );
                // **T5.Z4: these five asserted the null-*context* guard, not the
                // null-layer one.** With the context a `&mut` the guard is gone and the
                // remaining arm is the layer's, which every one of them answers by doing
                // nothing — the values below are what the tree returns, read off the
                // run rather than predicted (S33).
                assert_eq!(WelsInitRefList(pCtx, None, 0), ERR_NONE);
                assert_eq!(WelsInitBSliceRefList(pCtx, None, 0), ERR_NONE);
                // `manage_dec_ref`'s `ERR_INFO_INVALID_PTR` is 3 where this module's own
                // constant is 1 — the three shims below forward into the former, which is
                // why the value is spelled through its owner.
                const MDR_INVALID_PTR: i32 = crate::decoder::manage_dec_ref::ERR_INFO_INVALID_PTR;
                assert_eq!(WelsReorderRefList(pCtx, None), MDR_INVALID_PTR);
                assert_eq!(WelsReorderRefList2(pCtx, None), MDR_INVALID_PTR);
                assert_eq!(WelsMarkAsRef(pCtx, None), MDR_INVALID_PTR);
                WelsResetRefPic(pCtx);
            }
        }
    }

    #[test]
    fn test_missing_functions_highlight_translations() {
        {
            {
                assert_eq!(GetCPUCount(), 1);
                let mut cpu_cores = 0;
                assert_eq!(WelsCPUFeatureDetect(&mut cpu_cores), 0);
                assert_eq!(cpu_cores, 1);
                // T5.Z4: this asserted the null-context guard too. `WelsOpenDecoder`
                // on a real context is the success path `Initialize` takes.
                assert_eq!(
                    WelsOpenDecoder(&mut SWelsDecoderContext::new_boxed(), std::ptr::null_mut()),
                    ERR_NONE
                );
                WelsEndDecoder(&mut SWelsDecoderContext::new_boxed());
                // T5.Z4: the null-context arm is gone; a zero-length source on a real
                // context is the flush path, which succeeds.
                // T5b.6: the three null pointers were the api's three arguments and
                // are the three empty values now — an empty source, the caller's own
                // plane array, and the buffer descriptor it fills.
                let mut ppDst = [std::ptr::null_mut(); 3];
                let mut dst_info = SBufferInfo::default();
                assert_eq!(
                    WelsDecodeBs(
                        &mut SWelsDecoderContext::new_boxed(),
                        &[],
                        0,
                        &mut ppDst,
                        &mut dst_info,
                        std::ptr::null_mut(),
                    ),
                    ERR_NONE
                );
            }
        }
    }

    #[test]
    fn test_decoder_open_end_and_init_static_memory_state_flags() {
        {
            {
                let mut ctx = SWelsDecoderContext::new_boxed();
                assert_eq!(WelsOpenDecoder(&mut *ctx, std::ptr::null_mut()), ERR_NONE);
                assert!(ctx.bParamSetsLostFlag);
                assert!(ctx.bNewSeqBegin);
                assert!(ctx.bPrintFrameErrorTraceFlag);
                assert_eq!(ctx.iIgnoredErrorInfoPacketCount, 0);
                assert!(ctx.bFrameFinish);
                assert_eq!(ctx.iSeqNum, 0);

                WelsEndDecoder(&mut *ctx);
                assert!(!ctx.bParamSetsLostFlag);
                assert!(!ctx.bNewSeqBegin);
                assert!(!ctx.bPrintFrameErrorTraceFlag);
                assert!(!ctx.bFrameFinish);
            }
        }
    }

    #[test]
    fn test_chapter_7_frame_finalization() {
        {
            {
                let mut ppDst = [std::ptr::null_mut(); 3];
                let mut dst_info = SBufferInfo::default();
                assert_eq!(
                    CheckAndFinishLastPic(
                        &mut SWelsDecoderContext::new_boxed(),
                        &mut ppDst,
                        &mut dst_info
                    ),
                    false
                );
                // **The arm this asserted is now the type's.** It read
                // `ERR_INFO_INVALID_PTR` from `DecodeFrameConstruction`'s two null
                // tests; with `ppDst` and `pDstInfo` borrows there is no null to test,
                // and the absent layer is what the function refuses on.
                assert_eq!(
                    DecodeFrameConstruction(
                        &mut SWelsDecoderContext::new_boxed(),
                        None,
                        &mut ppDst,
                        &mut dst_info
                    ),
                    ERR_INFO_INVALID_PTR
                );
                WelsDecodeAccessUnitEnd(&mut SWelsDecoderContext::new_boxed());

                let mut stat = SDecoderStatistics::default();
                let mut sps = SSps::default();
                sps.iSpsId = 3;
                sps.uiProfileIdc = 66;
                sps.uiLevelIdc = 31;
                let mut pps = SPps::default();
                pps.iPpsId = 5;

                UpdateDecoderStatisticsForActiveParaset(Some(&mut stat), Some(&sps), Some(&pps));
                assert_eq!(stat.iCurrentActiveSpsId, 3);
                assert_eq!(stat.iCurrentActivePpsId, 5);
                assert_eq!(stat.uiProfile, 66);
                assert_eq!(stat.uiLevel, 31);
            }
        }
    }

    /// **F55's red-under-revert test.** `ResetCurrentAccessUnit`'s rotation moves the
    /// access unit's slots; the C rotates a *pointer array*, so `pCtx->pSliceHeader` —
    /// a pointer *into* a node — still names the same slice header afterwards. The port
    /// holds that reference as an index, so the rotation has to carry it: without
    /// `swap_au_nodes`' remap the index keeps naming slot 0, which the rotation has just
    /// filled with the **next** access unit's NAL, and every reader downstream — the
    /// api's three reordering sites among them — reads a slice header one access unit
    /// ahead of the picture it is describing.
    ///
    /// The shape is the measured one: one decoded slice at slot 0, the successor AU's
    /// already-parsed NAL at slot 1, `uiActualUnitsNum` 1 and `uiAvailUnitsNum` 2.
    /// Revert `swap_au_nodes` to a bare `nal_units.swap` and the first assertion reads
    /// `P_SLICE`/`99`.
    #[test]
    fn the_au_rotation_carries_the_two_node_indices_with_it() {
        use crate::decoder::decoder_context::slice_header_of;
        use crate::decoder::slice::EWelsSliceType;

        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.access_unit = Some(SAccessUnit::with_nodes(4));
        {
            let au = cur_au(&mut ctx.access_unit).unwrap();
            let decoded = &mut au.node_mut(0).unwrap().sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
            decoded.eSliceType = EWelsSliceType::B_SLICE;
            decoded.iPicOrderCntLsb = 41;
            let successor =
                &mut au.node_mut(1).unwrap().sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
            successor.eSliceType = EWelsSliceType::P_SLICE;
            successor.iPicOrderCntLsb = 99;
            au.uiActualUnitsNum = 1;
            au.uiAvailUnitsNum = 2;
        }
        ctx.nal_cur = Some(0);
        ctx.slice_hdr_nal = Some(0);

        ResetCurrentAccessUnit(&mut ctx);

        assert_eq!(
            slice_header_of(&ctx).map(|sh| (sh.eSliceType, sh.iPicOrderCntLsb)),
            Some((EWelsSliceType::B_SLICE, 41)),
            "the decoded slice's header, not the successor AU's"
        );
        assert_eq!(ctx.slice_hdr_nal, Some(1), "the index followed its node");
        assert_eq!(ctx.nal_cur, Some(1), "and so did the NAL under decode");
        assert_eq!(
            cur_au(&mut ctx.access_unit)
                .and_then(|au| au.node(0))
                .map(|n| n.sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb),
            Some(99),
            "slot 0 is the successor's, which is the point of the rotation"
        );
    }

    /// [`ForceResetCurrentAccessUnit`]'s half of the same rule — the error path's
    /// rotation, driven from `uiEndPos` instead of `uiActualUnitsNum`.
    #[test]
    fn the_error_path_rotation_carries_them_too() {
        let mut ctx = SWelsDecoderContext::new_boxed();
        ctx.access_unit = Some(SAccessUnit::with_nodes(4));
        {
            let au = cur_au(&mut ctx.access_unit).unwrap();
            au.node_mut(0).unwrap().uiTimeStamp = 10;
            au.node_mut(2).unwrap().uiTimeStamp = 12;
            au.uiEndPos = 1;
            au.uiAvailUnitsNum = 3;
        }
        ctx.nal_cur = Some(2);
        ctx.slice_hdr_nal = Some(0);

        let SWelsDecoderContext { access_unit, nal_cur, slice_hdr_nal, .. } = &mut *ctx;
        ForceResetCurrentAccessUnit(cur_au(access_unit).unwrap(), nal_cur, slice_hdr_nal);

        // The one swap is (2, 0): `uiSucAuIdx` starts at `uiEndPos + 1` = 2 and
        // `uiCurAuIdx` at 0, and the loop stops at `uiAvailUnitsNum` = 3.
        assert_eq!(ctx.nal_cur, Some(0));
        assert_eq!(ctx.slice_hdr_nal, Some(2));
        assert_eq!(
            cur_au(&mut ctx.access_unit).and_then(|au| au.node(0)).map(|n| n.uiTimeStamp),
            Some(12)
        );
    }

    /// **The arm F56 unblocked (T5b.8).** `AllocPicBuffOnNewSeqBegin` opens with
    /// "the active SPS, or else the first initialized entry of `sSpsBuffer`". While
    /// `active_sps` was born `Some(SpsRef { id: 0 })` — the `Option<SpsRef>` niche
    /// reading out of the zeroed shell, not anything the C wrote — the `else` could
    /// not run on a first access unit, and the scan had no test because it had no
    /// caller.
    ///
    /// **No stream in either gate reaches it even now**, and this test says so
    /// rather than implying otherwise: `WelsDecodeInitAccessUnitStart` writes
    /// `active_sps` from the start NAL's slice header before this runs, so on the
    /// corpus and the conformance set the `if` arm is always the one taken. The
    /// scan is exercised here directly, which is the only way it is exercised.
    ///
    /// **What it selects, and against what reference.** The scan is the *port's*
    /// guard, not a transcription: the C++ reads `pCtx->pSps->iMbWidth` with no null
    /// test, so this state is a null dereference there. What the port has always
    /// done — before T5.R6 by taking the entry's address, after it by taking the
    /// entry's id — is select the first `sSpsBuffer` entry with
    /// `uiTotalMbCount > 0`. That equivalence is what is asserted.
    #[test]
    fn the_fallback_scan_takes_the_first_initialized_sps() {
        use crate::decoder::decoder_context::SpsRef;

        let mut ctx = SWelsDecoderContext::new_boxed();
        assert!(ctx.active_sps.is_none(), "F56: the context is born with no active SPS");

        // Entries 0 and 1 are the zeroed buffer; 2 is the first one a `ParseSps` has
        // filled. `uiTotalMbCount > 0` is the initialized test the null test was.
        ctx.sSpsPpsCtx.sSpsBuffer[2].iSpsId = 2;
        ctx.sSpsPpsCtx.sSpsBuffer[2].iMbWidth = 5;
        ctx.sSpsPpsCtx.sSpsBuffer[2].iMbHeight = 3;
        ctx.sSpsPpsCtx.sSpsBuffer[2].uiTotalMbCount = 15;
        // A later entry, to show the scan stops at the first rather than the last.
        ctx.sSpsPpsCtx.sSpsBuffer[7].iSpsId = 7;
        ctx.sSpsPpsCtx.sSpsBuffer[7].uiTotalMbCount = 99;

        // `pMemAlign` is null on a bare context, so `SyncPictureResolutionExt`
        // returns 1 at its own guard — after the scan has run and stored its answer,
        // which is the step under test.
        let _ = AllocPicBuffOnNewSeqBegin(&mut ctx);
        assert_eq!(ctx.active_sps, Some(SpsRef { id: 2, subset: false }));

        // And with nothing initialized the scan finds nothing, which is the C++'s
        // null `pSps` reached without dereferencing it.
        let mut empty = SWelsDecoderContext::new_boxed();
        assert_eq!(AllocPicBuffOnNewSeqBegin(&mut empty), ERR_INFO_INVALID_PTR);
        assert!(empty.active_sps.is_none());
    }

    #[test]
    fn test_parse_slice_header_syntaxs_no_access_unit() {
        {
            {
                // T5.Z4: the null-*context* arm is gone with the raw parameter; what this
                // still covers is the arm the body actually takes first — no access unit
                // in flight, so no NAL to read a header out of.
                let mut cursor = crate::safe::bits::BsCursor::default();
                let mut ctx = SWelsDecoderContext::new_boxed();
                let res = ParseSliceHeaderSyntaxs(&mut ctx, 0, &mut cursor, false);
                assert_eq!(res, ERR_INFO_INVALID_PTR);
            }
        }
    }
}
