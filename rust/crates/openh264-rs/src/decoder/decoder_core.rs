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
    unused_unsafe,
    unused_mut
)]

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;

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


// Logging Levels
pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;
pub const WELS_LOG_DEBUG: i32 = 4;

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


pub use crate::decoder::decoder_context::SParserBsInfo;


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
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SSlice, PSlice};



// The duplicate SVclNal/SPrefixNalUnit/SNalData definitions that used to sit here
// were deleted dead at T3.3 (S18): every live `sNalData` access resolves through
// `nalu::SNalUnit`, whose own definitions are the ones the decoder uses.

pub use crate::decoder::nalu::SAccessUnit;
use crate::decoder::decoder_context::{
    au_has_nals, cur_au, cur_and_refs, cur_dq_layer, dec_pic, pic_pool_mut, pic_refs, pool_pic,
    ref_id, ref_pic,
};
use crate::decoder::picture::pic_slot;


#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub sSliceInLayer: SSlice,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    pub pSubsetSps: *mut SSubsetSps,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        Self {
            sNalHeaderExt: SNalUnitHeaderExt::default(),
            sSliceInLayer: SSlice::default(),
            pSps: std::ptr::null_mut(),
            pPps: std::ptr::null_mut(),
            pSubsetSps: std::ptr::null_mut(),
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
    pub pFmo: *mut crate::decoder::fmo::TagFmo,
    // T5.H1: `pNzcRs` (24 bytes per macroblock) and `pInterPredictionDoneFlag`
    // (one byte per macroblock) sat here. Both are dead in **both** trees: `pNzcRs` is allocated, aliased onto
    // the layer (`decoder_core.cpp:2471`) and never read or written by anything;
    // `pInterPredictionDoneFlag` is written `= 0` at 14 sites in `decode_slice.cpp`
    // and read at none. Deleting them costs 2 of the grid's 24 arrays and 2 of its
    // 27 allocations before 5.2 carries either into a safe container.
    pub iLumaStride: i32,
    pub iChromaStride: i32,
    pub pPred: [*mut u8; 3],
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

    pub pRefPicListReordering: *mut SRefPicListReorderSyn,
    pub pPredWeightTable: *mut SPredWeightTable,
    pub pRefPicMarking: *mut SRefPicMarking,
    pub pRefPicBaseMarking: *mut SRefBasePicMarking,

    // **T5.P′1 (W2b) — `pRef` and `pDec` sat here, and both are gone.**
    //
    // `pRef` was dead in this port: zero readers, zero writers, at every commit the
    // grep was taken.
    //
    // `pDec` was a **cache of `dec_pic(pCtx)`**, not a second carrier. One stamp site
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
    /// The shape is `SWelsDecoderContext::new_boxed`'s, from T3.3: zero a
    /// `MaybeUninit` shell, write the owning fields through it with
    /// `addr_of_mut!` (S29 — no `&mut` to a field of a not-yet-valid struct), and
    /// only then materialize. There is no shell left over to delete afterwards,
    /// because the grid is the only owning field and it is written here.
    ///
    /// `Default` is not reinstated on top of this: a layer cannot be constructed
    /// without dimensions any more, and a `Default` that invented some would be the
    /// same lie the wholesale zeroing was.
    pub fn for_grid(dims: MbDims) -> Self {
        let mut shell = std::mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            std::ptr::addr_of_mut!((*shell.as_mut_ptr()).grid).write(MbGrid::new(dims));
            let mut layer = shell.assume_init();
            layer.uiRefLayerDqId = 255;
            layer.uiRefLayerChromaPhaseYPlus1 = 1;
            layer
        }
    }
}

/// `SHIM(phase5)` — a raw pointer at macroblock `mb_xy` of one of [`MbGrid`]'s
/// arrays, for the callers 5.2 has not converted yet.
///
/// The raw bridge lives here, on the consumer side, and **not** in
/// `safe/mb_grid.rs`, which is `#![forbid(unsafe_code)]` and stays that way:
/// `SPicture::data_ptr` (`picture.rs`, T5.C3) set the precedent. It retires as the
/// families that still hand a bare element pointer to a kernel convert; nothing new
/// may call it.
///
/// # The provenance, which is the whole point (S28)
///
/// The pointer is derived from the **allocation root** — the whole `Vec`'s slice —
/// and then moved with `wrapping_add`. It is *not*
/// `a.as_mut_slice()[mb_xy..].as_mut_ptr()`, which produces the identical address,
/// compiles under `forbid(unsafe_code)`, passes every byte gate in the battery, and
/// is UB the moment a kernel walks backwards from it, because slicing narrows
/// provenance to `[mb_xy..]`. That is not hypothetical here: `pCbfDc`'s consumers
/// take the array's base and index it later, and `GetMbType`'s seven callers index
/// the base they are handed at *neighbour* addresses (T5.K2).
///
/// **`GetPNzc` was named here as a third example and it is not one** (checked at
/// T5.L1, when the family actually converted): its callers read the neighbour's
/// counts through a *second* `GetPNzc` call, never by walking off the first
/// pointer, and all four consumers bound themselves with
/// `from_raw_parts(pNnzTab, 24)`. It is a shared borrow now, not a bridge. A
/// scouted claim about a family's reach is a lead until the family converts (S24).
/// Miri is the only instrument that can see the difference — see the full-reach
/// tests in this file's `mod tests`.
///
/// # Panics
///
/// If `mb_xy` is past one-past-the-end of the array. The C computed
/// `base + iMbXy` with no check at all, so this is P13's bargain: a panic here is
/// a port bug that the C would have turned into a silent out-of-bounds write. It
/// is the check F32 did not have — two arrays sized `numMb` and indexed
/// `numMb * 8`.
#[inline]
pub fn mb_grid_ptr<T>(a: &mut MbArray<T>, mb_xy: usize) -> *mut T {
    let len = a.as_slice().len();
    assert!(
        mb_xy <= len,
        "macroblock {mb_xy} outside a per-macroblock array of {len}"
    );
    a.as_mut_slice().as_mut_ptr().wrapping_add(mb_xy)
}

pub use crate::decoder::decoder_context::{SRefPic, PRefPic};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSysMEMBuffer {
    pub iWidth: i32,
    pub iHeight: i32,
    pub iFormat: i32,
    pub iStride: [i32; 2],
}

impl Default for SSysMEMBuffer {
    fn default() -> Self {
        Self {
            iWidth: 0,
            iHeight: 0,
            iFormat: videoFormatI420,
            iStride: [0; 2],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union SUsrData {
    pub sSystemBuffer: SSysMEMBuffer,
}

impl Default for SUsrData {
    fn default() -> Self {
        Self {
            sSystemBuffer: SSysMEMBuffer::default(),
        }
    }
}

pub use crate::api::codec_api::SBufferInfo;

pub use crate::decoder::decoder_context::SDecoderStatistics;


pub use crate::decoder::decoder_context::{SDecodingParam, SLogContext};


pub use crate::decoder::decoder_context::SWelsCabacDecEngine;


#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SFmo {
    pub pSliceGroupMap: *mut u8,
    pub iSliceGroupCount: i32,
}

/// Reference-picture border expansion length (`PADDING_LENGTH` in
/// `codec/common/inc/expand_pic.h`).
pub const PADDING_LENGTH: usize = 32;

/// The one place a mid-plane `pDst` becomes the full allocation slice
/// (R-c: nothing else does this arithmetic). Every caller of the expand
/// functions hands `pData[i]`, which both codecs' `AllocPicture`s place at
/// `pBuffer + (1 + stride) * pad` — i.e. `pad` rows plus `pad` bytes into the
/// allocation (`decoder/pic_queue.rs:177-330`, `encoder/wels_preprocess.rs:
/// 764-806`; chroma divides the same expression by two, which is the same
/// layout at `pad = 16`). The reconstructed span is the padded plane:
/// `(h + 2*pad)` rows of `stride`. The real allocation may be taller (row
/// counts are aligned up); claiming the prefix is exactly what the kernel may
/// touch.
///
/// # Safety
/// `pDst` must point at `(0, 0)` of a picture plane laid out as above, `pad`
/// rows and `pad` bytes into a live allocation of at least
/// `(kiPicH + 2*pad) * kiStride` bytes, with no other live reference to it.
unsafe fn expand_shim_span<'a>(pDst: *mut u8, kiStride: i32, kiPicH: i32, pad: usize) -> &'a mut [u8] {
    let stride = kiStride as usize;
    let h = kiPicH as usize;
    std::slice::from_raw_parts_mut(pDst.sub(pad * stride + pad), (h + 2 * pad) * stride)
}

/// Matches `ExpandPictureLuma_c` in `codec/common/src/expand_pic.cpp`.
///
/// # Safety
/// `pDst`, `kiStride`, `kiPicH` as [`expand_shim_span`] with `pad = 32`
/// (`PADDING_LENGTH` — the luma border); `kiPicW + 64 <= kiStride`; positive
/// width and height.
pub unsafe extern "C" fn ExpandPictureLuma_c(pDst: *mut u8, kiStride: i32, kiPicW: i32, kiPicH: i32) {
    // SHIM(phase2) -> expand_picture
    unsafe {
        let buf = expand_shim_span(pDst, kiStride, kiPicH, PADDING_LENGTH);
        crate::common::expand_pic::expand_picture(
            buf,
            kiStride as usize,
            kiPicW as usize,
            kiPicH as usize,
            PADDING_LENGTH,
        );
    }
}

/// Matches `ExpandPictureChroma_c` in `codec/common/src/expand_pic.cpp`.
///
/// # Safety
/// `pDst`, `kiStride`, `kiPicH` as [`expand_shim_span`] with `pad = 16`
/// (`PADDING_LENGTH >> 1` — the chroma border); `kiPicW + 32 <= kiStride`;
/// positive width and height.
pub unsafe extern "C" fn ExpandPictureChroma_c(pDst: *mut u8, kiStride: i32, kiPicW: i32, kiPicH: i32) {
    // SHIM(phase2) -> expand_picture
    unsafe {
        let buf = expand_shim_span(pDst, kiStride, kiPicH, PADDING_LENGTH >> 1);
        crate::common::expand_pic::expand_picture(
            buf,
            kiStride as usize,
            kiPicW as usize,
            kiPicH as usize,
            PADDING_LENGTH >> 1,
        );
    }
}

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};

pub use crate::decoder::nalu::{SNalUnit, PNalUnit};

/// The C's `typedef SDqLayer* PDqLayer`, kept under its C name deliberately: it is
/// a *raw pointer to* the layer, not the layer, and it is Phase 5's to delete
/// outright (5.5 constructs the layer, 5.6 converts the last callers that spell
/// this). Renaming it alongside the struct at T5.M1 would have dressed a pointer
/// the port is retiring in the name of the safe type that replaced its contents.
pub type PDqLayer = *mut DqLayerState;

pub use crate::decoder::decoder_context::{Picture, SPicture, PPicture, SPicBuff};

pub use crate::decoder::parameter_sets::{PSps, PPps, PSubsetSps};

pub type PSliceHeader = *mut SSliceHeader;
pub type PSliceHeaderExt = *mut SSliceHeaderExt;
pub type PNalUnitHeaderExt = *mut SNalUnitHeaderExt;
pub type PLayerInfo = *mut SLayerInfo;
pub type PRefPicListReorderSyn = *mut SRefPicListReorderSyn;
pub type PRefPicMarking = *mut SRefPicMarking;
pub type PRefBasePicMarking = *mut SRefBasePicMarking;
pub type PPredWeightTable = *mut SPredWeightTable;

// Logging and Bitstream Reading Helpers

pub unsafe fn WelsLog(_pLogCtx: *mut SLogContext, _iLevel: i32, _fmt: &str) {}

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

// Memory Allocation Helper Wrappers

unsafe fn WelsMalloczHelper(pMa: *mut CMemoryAlign, size: usize) -> *mut u8 {
    if !pMa.is_null() {
        let tag = b"WelsMallocz\0".as_ptr() as *const c_char;
        (*pMa).WelsMallocz(size as u32, tag) as *mut u8
    } else {
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap_or(
            std::alloc::Layout::from_size_align(size, 1).unwrap()
        );
        std::alloc::alloc_zeroed(layout)
    }
}

unsafe fn WelsFreeHelper(pMa: *mut CMemoryAlign, ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }
    if !pMa.is_null() {
        let tag = b"WelsFree\0".as_ptr() as *const c_char;
        (*pMa).WelsFree(ptr as *mut c_void, tag);
    } else {
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap_or(
            std::alloc::Layout::from_size_align(size, 1).unwrap()
        );
        std::alloc::dealloc(ptr, layout);
    }
}

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
pub unsafe fn GetThreadCount(_pCtx: PWelsDecoderContext) -> i32 {
    0
}

unsafe fn ResetDecStatNums(pDecStat: *mut SDecoderStatistics) {
    if pDecStat.is_null() {
        return;
    }
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

unsafe fn UpdateDecStatFreezingInfo(idr_flag: bool, pDecStat: *mut SDecoderStatistics) {
    if pDecStat.is_null() {
        return;
    }
    if idr_flag {
        (*pDecStat).uiFreezingIDRNum += 1;
    } else {
        (*pDecStat).uiFreezingNonIDRNum += 1;
    }
}

#[inline]
pub unsafe fn UpdateDecStatNoFreezingInfo(pCtx: PWelsDecoderContext, pCurDq: PDqLayer) {
    if pCtx.is_null()
        || pCurDq.is_null()
        || (*pCtx).pDec.is_none()
        || (*pCtx).pDecoderStatistics.is_null()
    {
        return;
    }
    let pPic = dec_pic(pCtx);
    let pDecStat = (*pCtx).pDecoderStatistics;

    if (*pDecStat).iAvgLumaQp == -1 {
        (*pDecStat).iAvgLumaQp = 0;
    }

    let mut iTotalQp = 0i64;
    let kiMbNum = ((*pCurDq).iMbWidth * (*pCurDq).iMbHeight) as usize;
    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        for iMb in 0..kiMbNum {
            iTotalQp += *(*pCurDq).grid.luma_qp.get(iMb) as i64;
        }
        if kiMbNum > 0 {
            iTotalQp /= kiMbNum as i64;
        }
    } else {
        let mut iCorrectMbNum = 0i64;
        for iMb in 0..kiMbNum {
            let correct = if *(*pCurDq).grid.mb_correctly_decoded_flag.get(iMb) {
                1i64
            } else {
                0i64
            };
            iCorrectMbNum += correct;
            iTotalQp += (*(*pCurDq).grid.luma_qp.get(iMb) as i64) * correct;
        }
        if iCorrectMbNum == 0 {
            iTotalQp = (*pDecStat).iAvgLumaQp as i64;
        } else {
            iTotalQp /= iCorrectMbNum;
        }
    }

    if (*pDecStat).uiDecodedFrameCount == u32::MAX {
        ResetDecStatNums(pDecStat);
        (*pDecStat).iAvgLumaQp = iTotalQp as i32;
    } else {
        let count = (*pDecStat).uiDecodedFrameCount as i64;
        (*pDecStat).iAvgLumaQp =
            (((*pDecStat).iAvgLumaQp as i64 * count + iTotalQp) / (count + 1)) as i32;
    }

    if (*pCurDq).sLayerInfo.sNalHeaderExt.bIdrFlag {
        if (*pPic).bIsComplete {
            (*pDecStat).uiIDRCorrectNum += 1;
        } else if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
            (*pDecStat).uiEcIDRNum += 1;
        }
    }
}

#[inline]
pub unsafe fn UpdateDecStat(pCtx: PWelsDecoderContext, pCurDq: PDqLayer, bOutput: bool) {
    if pCtx.is_null() {
        return;
    }
    if (*pCtx).bFreezeOutput {
        if !pCurDq.is_null() {
            UpdateDecStatFreezingInfo(
                (*pCurDq).sLayerInfo.sNalHeaderExt.bIdrFlag,
                (*pCtx).pDecoderStatistics,
            );
        }
    } else if bOutput {
        UpdateDecStatNoFreezingInfo(pCtx, pCurDq);
    }
}

#[inline]
pub unsafe fn WelsTargetSliceConstruction(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> i32 {
    crate::decoder::decode_slice::WelsTargetSliceConstruction(pCtx, pCurDqLayer)
}

#[inline]
pub unsafe fn WelsDecodeSlice(
    pCtx: PWelsDecoderContext,
    pCurDqLayer: PDqLayer,
    bFreshSlice: bool,
    pCurNal: PNalUnit,
) -> i32 {
    crate::decoder::decode_slice::WelsDecodeSlice(pCtx, pCurDqLayer, bFreshSlice, pCurNal)
}

#[inline]
pub unsafe fn WelsDecodeAndConstructSlice(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> i32 {
    crate::decoder::decode_slice::WelsDecodeAndConstructSlice(pCtx, pCurDqLayer)
}

#[inline]
pub unsafe fn WelsInitRefList(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer, iPoc: i32) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitRefList(pCtx, pCurDqLayer, iPoc)
}

#[inline]
pub unsafe fn WelsInitBSliceRefList(
    pCtx: PWelsDecoderContext,
    pCurDqLayer: PDqLayer,
    iPoc: i32,
) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitBSliceRefList(pCtx, pCurDqLayer, iPoc)
}

#[inline]
pub unsafe fn WelsReorderRefList(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> i32 {
    crate::decoder::manage_dec_ref::WelsReorderRefList(pCtx, pCurDqLayer)
}

#[inline]
pub unsafe fn WelsReorderRefList2(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> i32 {
    crate::decoder::manage_dec_ref::WelsReorderRefList2(pCtx, pCurDqLayer)
}

#[inline]
pub unsafe fn WelsMarkAsRef(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> i32 {
    crate::decoder::manage_dec_ref::WelsMarkAsRef(pCtx, pCurDqLayer, std::ptr::null_mut())
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
pub unsafe fn GetI4LumaIChromaAddrTable(pBlockOffset: *mut i32, iStrideY: i32, iStrideUV: i32) {
    crate::decoder::decode_mb_aux::GetI4LumaIChromaAddrTable(pBlockOffset, iStrideY, iStrideUV);
}

#[inline]
pub unsafe fn ComputeColocatedTemporalScaling(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) {
    let _ = crate::decoder::decode_slice::ComputeColocatedTemporalScaling(
        pCtx,
        pCurDqLayer,
        pic_refs(pCtx),
    );
}

/// Adaptive picture-queue size, `pSps->iNumRefFrames + 2` (the extra two are
/// the EC MV copy exchange buffers).
/// Matches `GetTargetRefListSize` in `decoder.cpp`.
pub unsafe fn GetTargetRefListSize(pCtx: PWelsDecoderContext) -> i32 {
    let mut iNumRefFrames = if pCtx.is_null() || (*pCtx).pSps.is_null() {
        MAX_REF_PIC_COUNT as i32 + 2
    } else {
        let iThreadCount = GetThreadCount(pCtx);
        if iThreadCount > 1 {
            // Thread and reordering buffering need more DPB space.
            MAX_DPB_COUNT as i32 + iThreadCount
        } else {
            (*(*pCtx).pSps).iNumRefFrames + 2
        }
    };
    // LONG_TERM_REF: picture queue size is at least 2.
    if iNumRefFrames < 2 {
        iNumRefFrames = 2;
    }
    iNumRefFrames
}

pub unsafe fn SyncPictureResolutionExt(pCtx: PWelsDecoderContext, iWidth: u32, iHeight: u32) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let iPicWidth = (iWidth << 4) as i32;
    let iPicHeight = (iHeight << 4) as i32;
    let iPicBufSize = GetTargetRefListSize(pCtx);
    (*pCtx).iPicQueueNumber = iPicBufSize;

    if (*pCtx).pPicBuff.is_none() {
        let Some(pool) =
            crate::decoder::pic_queue::CreatePicBuff(pCtx, iPicBufSize, iPicWidth, iPicHeight)
        else {
            return 1;
        };
        (*pCtx).pPicBuff = Some(pool);
    } else {
        // The buffer is not reallocated here, so report its real capacity. The `0` is
        // unreachable — this arm *is* the pool being present — and the borrow ends
        // before the field write, which is the discipline every `pic_pool_mut` call
        // site keeps.
        let capacity = pic_pool_mut(pCtx).map_or(0, |pool| pool.capacity());
        (*pCtx).iPicQueueNumber = capacity;
    }
    let iErr = InitialDqLayersContext(pCtx, iPicWidth, iPicHeight);
    if iErr != ERR_NONE {
        return iErr;
    }
    ERR_NONE
}

#[inline]
pub unsafe fn WelsResetRefPic(pCtx: PWelsDecoderContext) {
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

#[inline]
pub unsafe fn NeedErrorCon(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> bool {
    false
}

#[inline]
pub unsafe fn ImplementErrorCon(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) {}

#[inline]
pub unsafe fn MarkECFrameAsRef(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
/// Matches `ResetActiveSPSForEachLayer` in `decoder_context.h`.
pub unsafe fn ResetActiveSPSForEachLayer(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    if (*pCtx).iTotalNumMbRec == 0 {
        for i in 0..MAX_LAYER_NUM {
            (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = std::ptr::null_mut();
        }
    }
}

#[inline]
pub unsafe fn GetVclNalTemporalId(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn GetPrevFrameNum(pCtx: PWelsDecoderContext) -> i32 {
    0
}

#[inline]
pub unsafe fn CopySpsPps(pSrcCtx: PWelsDecoderContext, pDstCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn FmoParamUpdate(
    pFmo: *mut SFmo,
    pSps: PSps,
    pPps: PPps,
    pActiveNum: *mut i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn FmoNextMb(pFmo: *mut SFmo, iMbIdx: i32) -> i32 {
    iMbIdx + 1
}

#[inline]
pub unsafe fn CheckAccessUnitBoundaryExt(
    pLastNalHdr: *mut SNalUnitHeaderExt,
    pCurNalHdr: *mut SNalUnitHeaderExt,
    pLastSh: *mut SSliceHeader,
    pCurSh: *mut SSliceHeader,
) -> bool {
    true
}

// Core Functions Implemented in `decoder_core.cpp`

pub unsafe fn DecodeFrameConstruction(
    pCtx: PWelsDecoderContext,
    pCurDq: PDqLayer,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    if pCtx.is_null() || ppDst.is_null() || pDstInfo.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pPic = dec_pic(pCtx);
    if pCurDq.is_null() || pPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let kiWidth = (*pCurDq).iMbWidth << 4;
    let kiHeight = (*pCurDq).iMbHeight << 4;
    let kiTotalNumMbInCurLayer = (*pCurDq).iMbWidth * (*pCurDq).iMbHeight;
    let mut bFrameCompleteFlag = true;

    if (*pCtx).bNewSeqBegin {
        let pSps = (*pCurDq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
        if !pSps.is_null() {
            (*pCtx).sFrameCrop = (*pSps).sFrameCrop;
        }
        (*pCtx).bReferenceLostAtT0Flag = false;
        if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
            (*pCtx).bPrintFrameErrorTraceFlag = true;
            (*pCtx).iIgnoredErrorInfoPacketCount = 0;
        }
    }

    let kiActualWidth = kiWidth - ((*pCtx).sFrameCrop.iLeftOffset + (*pCtx).sFrameCrop.iRightOffset) * 2;
    let kiActualHeight = kiHeight - ((*pCtx).sFrameCrop.iTopOffset + (*pCtx).sFrameCrop.iBottomOffset) * 2;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        if !(*pCtx).pDecoderStatistics.is_null() {
            if (*(*pCtx).pDecoderStatistics).uiWidth != kiActualWidth as u32
                || (*(*pCtx).pDecoderStatistics).uiHeight != kiActualHeight as u32
            {
                (*(*pCtx).pDecoderStatistics).uiResolutionChangeTimes += 1;
                (*(*pCtx).pDecoderStatistics).uiWidth = kiActualWidth as u32;
                (*(*pCtx).pDecoderStatistics).uiHeight = kiActualHeight as u32;
            }
        }
        UpdateDecStatNoFreezingInfo(pCtx, pCurDq);
    }

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        if (*pCtx).iErrorCode == dsErrorFree {
            let pParser = (*pCtx).pParserBsInfo;
            // Nothing in this block calls back into the context, so one derivation
            // covers it — the borrow's extent is the check, not a style choice.
            if let Some(pCurAu) = cur_au(pCtx).filter(|_| !pParser.is_null()) {
                let mut iTotalNalLen: i32 = 0;
                for i in 0..(*pParser).iNalNum {
                    if !(*pParser).pNalLenInByte.is_null() {
                        iTotalNalLen += *(*pParser).pNalLenInByte.add(i as usize);
                    }
                }
                let mut pDstBuf = (*pParser).pDstBuff.add(iTotalNalLen as usize);
                let mut iIdx = pCurAu.uiStartPos as i32;
                let iEndIdx = pCurAu.uiEndPos as i32;
                if !(pCurAu.nal(iIdx as usize)).is_null() {
                    (*pParser).uiOutBsTimeStamp = (*(pCurAu.nal(iIdx as usize))).uiTimeStamp;
                }
                if !(*pCtx).pSps.is_null() {
                    let pSps = (*pCtx).pSps as *mut SSps;
                    (*pParser).iSpsWidthInPixel = ((*pSps).iMbWidth as i32) * 16
                        - (((*pSps).sFrameCrop.iLeftOffset
                            + (*pSps).sFrameCrop.iRightOffset)
                            << 1);
                    (*pParser).iSpsHeightInPixel = ((*pSps).iMbHeight as i32) * 16
                        - (((*pSps).sFrameCrop.iTopOffset
                            + (*pSps).sFrameCrop.iBottomOffset)
                            << 1);
                }

                while iIdx <= iEndIdx {
                    let pCurNal = pCurAu.nal(iIdx as usize);
                    if !pCurNal.is_null() {
                        let iNalLen = (*pCurNal).sNalData.sVclNal.iNalLength;
                        if !(*pParser).pNalLenInByte.is_null() {
                            *(*pParser).pNalLenInByte.add((*pParser).iNalNum as usize) = iNalLen;
                            (*pParser).iNalNum += 1;
                        }
                        // The `pNalPos`-guarded copy that sat here never executed:
                        // nothing in this port wrote `pNalPos`, so the guard was
                        // always false. Deleted dead with the field at T3.3 (S18);
                        // the length bookkeeping above is the part that does run.
                    }
                    iIdx += 1;
                }

                if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
                    (*pCtx).iTotalNumMbRec = 0;
                    (*pCtx).bFramePending = false;
                    (*pCtx).bFrameFinish = true;
                } else if (*pCtx).iTotalNumMbRec != 0 {
                    (*pCtx).bFramePending = true;
                    (*pPic).bIsComplete = false;
                    (*pCtx).bFrameFinish = false;
                    (*pCtx).iErrorCode |= dsFramePending;
                    return ERR_INFO_PARSEONLY_PENDING;
                }
            }
        } else {
            let pParser = (*pCtx).pParserBsInfo;
            if !pParser.is_null() {
                (*pParser).uiOutBsTimeStamp = 0;
                (*pParser).iNalNum = 0;
                (*pParser).iSpsWidthInPixel = 0;
                (*pParser).iSpsHeightInPixel = 0;
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
    } else if (*pCurDq).sLayerInfo.sNalHeaderExt.bIdrFlag && (*pCtx).iErrorCode == dsErrorFree {
        // T5.Q2: `dec_pic(pCtx)` stood here and at the parse-only arm above. Both
        // name the picture `pPic` already holds — `pCtx->pDec` is not written
        // anywhere in this function — and under owned slots a second derivation of
        // one slot invalidates the first, which `pPic`'s twenty-odd uses below are.
        // One derivation at the top is what this whole function's region wants.
        (*pPic).bIsComplete = true;
        (*pCtx).bFreezeOutput = false;
    }

    (*pCtx).iTotalNumMbRec = 0;

    (*pDstInfo).uiOutYuvTimeStamp = (*pPic).uiTimeStamp;
    *ppDst.add(0) = (*pPic).data_ptr(0);
    *ppDst.add(1) = (*pPic).data_ptr(1);
    *ppDst.add(2) = (*pPic).data_ptr(2);

    (*pDstInfo).UsrData.sSystemBuffer.iFormat = videoFormatI420;
    (*pDstInfo).UsrData.sSystemBuffer.iWidth = kiActualWidth;
    (*pDstInfo).UsrData.sSystemBuffer.iHeight = kiActualHeight;
    (*pDstInfo).UsrData.sSystemBuffer.iStride[0] = (*pPic).linesize(0);
    (*pDstInfo).UsrData.sSystemBuffer.iStride[1] = (*pPic).linesize(1);

    if !(*ppDst.add(0)).is_null() {
        *ppDst.add(0) = (*ppDst.add(0)).add(
            ((*pCtx).sFrameCrop.iTopOffset * 2 * (*pPic).linesize(0) + (*pCtx).sFrameCrop.iLeftOffset * 2) as usize
        );
    }
    if !(*ppDst.add(1)).is_null() {
        *ppDst.add(1) = (*ppDst.add(1)).add(
            ((*pCtx).sFrameCrop.iTopOffset * (*pPic).linesize(1) + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }
    if !(*ppDst.add(2)).is_null() {
        *ppDst.add(2) = (*ppDst.add(2)).add(
            ((*pCtx).sFrameCrop.iTopOffset * (*pPic).linesize(1) + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }

    for i in 0..3 {
        (*pDstInfo).pDst[i] = *ppDst.add(i);
    }
    (*pDstInfo).iBufferStatus = 1;

    let bOutResChange = (*pCtx).iLastImgWidthInPixel != (*pDstInfo).UsrData.sSystemBuffer.iWidth
        || (*pCtx).iLastImgHeightInPixel != (*pDstInfo).UsrData.sSystemBuffer.iHeight;
    (*pCtx).iLastImgWidthInPixel = (*pDstInfo).UsrData.sSystemBuffer.iWidth;
    (*pCtx).iLastImgHeightInPixel = (*pDstInfo).UsrData.sSystemBuffer.iHeight;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        (*pDstInfo).iBufferStatus = (bFrameCompleteFlag && (*pPic).bIsComplete) as i32;
    } else if !(*pCtx).pParam.is_null()
        && ((*(*pCtx).pParam).eEcActiveIdc
            == ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE
            || (*(*pCtx).pParam).eEcActiveIdc
                == ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)
        && (*pCtx).iErrorCode != dsErrorFree
        && bOutResChange
    {
        (*pCtx).bFreezeOutput = true;
    }

    if (*pDstInfo).iBufferStatus == 0 {
        if !bFrameCompleteFlag {
            (*pCtx).iErrorCode |= dsBitstreamError;
        }
        return ERR_INFO_MB_NUM_INADEQUATE;
    }

    if (*pCtx).bFreezeOutput {
        (*pDstInfo).iBufferStatus = 0;
    }

    (*pCtx).iMbEcedNum = (*pPic).iMbEcedNum;
    (*pCtx).iMbNum = (*pPic).iMbNum;
    (*pCtx).iMbEcedPropNum = (*pPic).iMbEcedPropNum;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
        if (*pDstInfo).iBufferStatus != 0
            && !(*pCtx).pDecoderStatistics.is_null()
            && ((*(*pCtx).pDecoderStatistics).uiWidth != kiActualWidth as u32
                || (*(*pCtx).pDecoderStatistics).uiHeight != kiActualHeight as u32)
        {
            (*(*pCtx).pDecoderStatistics).uiResolutionChangeTimes += 1;
            (*(*pCtx).pDecoderStatistics).uiWidth = kiActualWidth as u32;
            (*(*pCtx).pDecoderStatistics).uiHeight = kiActualHeight as u32;
        }
        UpdateDecStat(pCtx, pCurDq, (*pDstInfo).iBufferStatus != 0);
    }

    ERR_NONE
}

#[inline]
pub fn CheckSliceNeedReconstruct(uiLayerDqId: u8, uiTargetDqId: u8) -> bool {
    uiLayerDqId == uiTargetDqId
}

#[inline]
pub unsafe fn GetTargetDqId(uiTargetDqId: u8, psParam: *mut SDecodingParam) -> u8 {
    let uiRequiredDqId = if !psParam.is_null() {
        (*psParam).uiTargetDqLayer
    } else {
        255
    };
    WELS_MIN(uiTargetDqId, uiRequiredDqId)
}

#[inline]
pub unsafe fn HandleReferenceLostL0(pCtx: PWelsDecoderContext, pCurNal: PNalUnit) {
    if !pCurNal.is_null() && (*pCurNal).sNalHeaderExt.uiTemporalId == 0 {
        (*pCtx).bReferenceLostAtT0Flag = true;
    }
    (*pCtx).iErrorCode |= dsBitstreamError;
}

#[inline]
pub unsafe fn HandleReferenceLost(pCtx: PWelsDecoderContext, pCurNal: PNalUnit) {
    if !pCurNal.is_null()
        && ((*pCurNal).sNalHeaderExt.uiTemporalId == 0 || (*pCurNal).sNalHeaderExt.uiTemporalId == 1)
    {
        (*pCtx).bReferenceLostAtT0Flag = true;
    }
    (*pCtx).iErrorCode |= dsRefLost;
}

#[inline]
pub unsafe fn WelsDecodeConstructSlice(
    pCtx: PWelsDecoderContext,
    pCurDqLayer: PDqLayer,
    pCurNal: PNalUnit,
) -> i32 {
    let iRet = WelsTargetSliceConstruction(pCtx, pCurDqLayer);
    if iRet != ERR_NONE {
        HandleReferenceLostL0(pCtx, pCurNal);
    }
    iRet
}

pub unsafe fn ParsePredWeightedTable(buf: &[u8], pBs: &mut BsCursor, pSh: PSliceHeader) -> i32 {
    if pSh.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode > 7 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_LOG2_WEIGHT_DENOM);
    }
    (*pSh).sPredWeightTable.uiLumaLog2WeightDenom = uiCode;

    let pSps = (*pSh).pSps as *mut SSps;

    if !pSps.is_null() && (*pSps).uiChromaArrayType != 0 {
        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > 7 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_LOG2_WEIGHT_DENOM);
        }
        (*pSh).sPredWeightTable.uiChromaLog2WeightDenom = uiCode;
    }

    if ((*pSh).sPredWeightTable.uiLumaLog2WeightDenom | (*pSh).sPredWeightTable.uiChromaLog2WeightDenom) > 7 {
        return ERR_NONE;
    }

    let mut iList = 0;
    while iList < LIST_A {
        for i in 0..((*pSh).uiRefCount[iList] as usize) {
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
                (*pSh).sPredWeightTable.sPredList[iList].iLumaWeight[i] = iCode;

                if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if iCode < -128 || iCode > 127 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_OFFSET);
                }
                (*pSh).sPredWeightTable.sPredList[iList].iLumaOffset[i] = iCode;
            } else {
                (*pSh).sPredWeightTable.sPredList[iList].iLumaWeight[i] =
                    1 << (*pSh).sPredWeightTable.uiLumaLog2WeightDenom;
                (*pSh).sPredWeightTable.sPredList[iList].iLumaOffset[i] = 0;
            }

            if !pSps.is_null() && (*pSps).uiChromaArrayType == 0 {
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
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaWeight[i][j] = iCode;

                    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if iCode < -128 || iCode > 127 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_OFFSET);
                    }
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = iCode;
                }
            } else {
                for j in 0..2 {
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaWeight[i][j] =
                        1 << (*pSh).sPredWeightTable.uiChromaLog2WeightDenom;
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = 0;
                }
            }
        }
        iList += 1;
        if (*pSh).eSliceType != B_SLICE {
            break;
        }
    }
    ERR_NONE
}

pub unsafe fn CreateImplicitWeightTable(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return;
    }
    let pSliceHeader = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let pPps = (*pSliceHeader).pPps as *mut SPps;
    if pPps.is_null() {
        return;
    }

    if (*pCurDqLayer).bUseWeightedBiPredIdc && (*pPps).uiWeightedBipredIdc == 2 {
        let iPoc = (*pSliceHeader).iPicOrderCntLsb;
        let ref0 = ref_pic(pCtx, LIST_0, 0);
        let ref1 = ref_pic(pCtx, LIST_1, 0);
        if !ref0.is_null() && !ref1.is_null() {
            if (*pSliceHeader).uiRefCount[0] == 1
                && (*pSliceHeader).uiRefCount[1] == 1
                && ((*ref0).iFramePoc as i64 + (*ref1).iFramePoc as i64 == 2 * (iPoc as i64))
            {
                (*pCurDqLayer).bUseWeightedBiPredIdc = false;
                return;
            }
        }

        if !(*pCurDqLayer).pPredWeightTable.is_null() {
            (*(*pCurDqLayer).pPredWeightTable).uiLumaLog2WeightDenom = 5;
            (*(*pCurDqLayer).pPredWeightTable).uiChromaLog2WeightDenom = 5;
            for iRef0 in 0..((*pSliceHeader).uiRefCount[0] as usize) {
                let pRef0 = ref_pic(pCtx, LIST_0, iRef0);
                if !pRef0.is_null() {
                    let iPoc0 = (*pRef0).iFramePoc;
                    let bIsLongRef0 = (*pRef0).bIsLongRef;
                    for iRef1 in 0..((*pSliceHeader).uiRefCount[1] as usize) {
                        let pRef1 = ref_pic(pCtx, LIST_1, iRef1);
                        if !pRef1.is_null() {
                            let iPoc1 = (*pRef1).iFramePoc;
                            let bIsLongRef1 = (*pRef1).bIsLongRef;
                            (*(*pCurDqLayer).pPredWeightTable).iImplicitWeight[iRef0][iRef1] = 32;
                            if !bIsLongRef0 && !bIsLongRef1 {
                                let iTd = WELS_CLIP3(iPoc1 - iPoc0, -128, 127);
                                if iTd != 0 {
                                    let iTb = WELS_CLIP3(iPoc - iPoc0, -128, 127);
                                    let iTx = (16384 + (WELS_ABS(iTd) >> 1)) / iTd;
                                    let iDistScaleFactor = (iTb * iTx + 32) >> 8;
                                    if iDistScaleFactor >= -64 && iDistScaleFactor <= 128 {
                                        (*(*pCurDqLayer).pPredWeightTable).iImplicitWeight[iRef0][iRef1] =
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

pub unsafe fn ParseRefPicListReordering(buf: &[u8], pBs: &mut BsCursor, pSh: PSliceHeader) -> i32 {
    if pSh.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let keSt = (*pSh).eSliceType;
    if keSt == I_SLICE || keSt == SI_SLICE {
        return ERR_NONE;
    }
    let pRefPicListReordering = &mut (*pSh).pRefPicListReordering;
    let pSps = (*pSh).pSps;
    if pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

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
                if iIdx >= (*pSh).uiRefCount[iList] as usize || iIdx >= MAX_REF_PIC_COUNT {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                }
                if kuiIdc == 0 || kuiIdc == 1 {
                    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if uiCode >= (1u32 << (*(pSps as *mut SSps)).uiLog2MaxFrameNum) {
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

pub unsafe fn ParseDecRefPicMarking(
    pCtx: PWelsDecoderContext,
    buf: &[u8],
    pBs: &mut BsCursor,
    pSh: PSliceHeader,
    pSps: PSps,
    kbIdrFlag: bool,
) -> i32 {
    if pSh.is_null() || pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let kpRefMarking = &mut (*pSh).sRefMarking;
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
                    kpRefMarking.sMmcoRef[iIdx].iShortFrameNum = ((*pSh).iFrameNum
                        - kpRefMarking.sMmcoRef[iIdx].iDiffOfPicNum)
                        & (((1 << (*pSps).uiLog2MaxFrameNum) - 1) as i32);
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
                    if iMaxLongTermFrameIdx > (*pSps).iNumRefFrames {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_MARKING);
                    }
                    kpRefMarking.sMmcoRef[iIdx].iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
                } else if kuiMmco == MMCO_RESET {
                    if !bAllowMmco5 || bMmco5Exist {
                        return -1;
                    }
                    bMmco5Exist = true;
                    if !(*pCtx).pLastDecPicInfo.is_null() {
                        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb = 0;
                        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb = 0;
                    }
                    (*pSh).iPicOrderCntLsb = 0;
                    if !(*pCtx).pSliceHeader.is_null() {
                        (*(*pCtx).pSliceHeader).iPicOrderCntLsb = 0;
                    }
                }
                iIdx += 1;
            }
        }
    }
    ERR_NONE
}

pub unsafe fn FillDefaultSliceHeaderExt(
    pShExt: PSliceHeaderExt,
    pNalExt: PNalUnitHeaderExt,
) -> bool {
    if pShExt.is_null() || pNalExt.is_null() {
        return false;
    }
    if (*pNalExt).bNoInterLayerPredFlag || (*pNalExt).uiQualityId > 0 {

        (*pShExt).bBasePredWeightTableFlag = false;
    } else {
        (*pShExt).bBasePredWeightTableFlag = true;
    }
    (*pShExt).uiRefLayerDqId = 255;
    (*pShExt).uiDisableInterLayerDeblockingFilterIdc = 0;
    (*pShExt).iInterLayerSliceAlphaC0Offset = 0;
    (*pShExt).iInterLayerSliceBetaOffset = 0;
    (*pShExt).bConstrainedIntraResamplingFlag = false;
    (*pShExt).uiRefLayerChromaPhaseXPlus1Flag = 0;
    (*pShExt).uiRefLayerChromaPhaseYPlus1 = 1;
    (*pShExt).iScaledRefLayerPicWidthInSampleLuma = (*pShExt).sSliceHeader.iMbWidth << 4;
    (*pShExt).iScaledRefLayerPicHeightInSampleLuma = (*pShExt).sSliceHeader.iMbHeight << 4;
    (*pShExt).bSliceSkipFlag = false;
    (*pShExt).bAdaptiveBaseModeFlag = false;
    (*pShExt).bDefaultBaseModeFlag = false;
    (*pShExt).bAdaptiveMotionPredFlag = false;
    (*pShExt).bDefaultMotionPredFlag = false;
    (*pShExt).bAdaptiveResidualPredFlag = false;
    (*pShExt).bDefaultResidualPredFlag = false;
    (*pShExt).bTCoeffLevelPredFlag = false;
    (*pShExt).uiScanIdxStart = 0;
    (*pShExt).uiScanIdxEnd = 15;
    true
}

pub unsafe fn InitBsBuffer(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pMa = (*pCtx).pMemAlign;
    // `WelsMalloczHelper`'s zeroed allocation, owned: the allocation size *is*
    // `sRawData.len()` — the `iMaxBsBufferSizeInByte` field died with the pointers,
    // since a stored copy of the buffer's length is exactly the kind of extent F16
    // is about.
    match RawDataBuffer::try_new_zeroed(MIN_ACCESS_UNIT_CAPACITY * MAX_BUFFERED_NUM) {
        Ok(raw) => (*pCtx).sRawData = raw,
        Err(()) => return ERR_INFO_OUT_OF_MEMORY,
    }

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        let pParser = WelsMalloczHelper(pMa, std::mem::size_of::<SParserBsInfo>()) as *mut SParserBsInfo;
        if pParser.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pCtx).pParserBsInfo = pParser;
        let dstBuff = WelsMalloczHelper(pMa, MAX_ACCESS_UNIT_CAPACITY);
        if dstBuff.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pParser).pDstBuff = dstBuff;

        match RawDataBuffer::try_new_zeroed((*pCtx).sRawData.len()) {
            Ok(saved) => (*pCtx).sSavedData = saved,
            Err(()) => return ERR_INFO_OUT_OF_MEMORY,
        }

        (*pCtx).iMaxNalNum = (MAX_NAL_UNITS_IN_LAYER + 2) as i32;
        let nalLen = WelsMalloczHelper(pMa, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>()) as *mut i32;
        if nalLen.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pParser).pNalLenInByte = nalLen;
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

pub unsafe fn ExpandBsLenBuffer(pCtx: PWelsDecoderContext, kiCurrLen: i32) -> i32 {
    let pParser = (*pCtx).pParserBsInfo;
    if pParser.is_null() || (*pParser).pNalLenInByte.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    if kiCurrLen >= MAX_MB_SIZE + 2 {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    let mut iNewLen = kiCurrLen << 1;
    iNewLen = WELS_MIN(iNewLen, MAX_MB_SIZE + 2);
    let pMa = (*pCtx).pMemAlign;
    let pNewLenBuffer = WelsMalloczHelper(pMa, (iNewLen as usize) * std::mem::size_of::<i32>()) as *mut i32;
    if pNewLenBuffer.is_null() {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    // F40: `copy_nonoverlapping`'s count is in **elements**; the C++'s `memcpy`
    // (`decoder.cpp`) takes bytes, and the transliteration kept the `* sizeof(int32_t)`
    // — so this copied four times the source and wrote four times the destination.
    // Unreachable today (the port's `DecodeParser` is a stub, so nothing calls this
    // function), which is why nine gates and two Miri probes never saw it.
    std::ptr::copy_nonoverlapping(
        (*pParser).pNalLenInByte,
        pNewLenBuffer,
        (*pCtx).iMaxNalNum as usize,
    );
    WelsFreeHelper(pMa, (*pParser).pNalLenInByte as *mut u8, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>());
    (*pParser).pNalLenInByte = pNewLenBuffer;
    (*pCtx).iMaxNalNum = iNewLen;
    ERR_NONE
}

pub unsafe fn WelsInitDecoderFuncs(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let cpu_flag = (*pCtx).uiCpuFlag;

    // 0. Block helpers. `WelsBlockFuncInit` filled `sBlockFunc` here (`InitDecFuncs`
    // in `decoder.cpp`), through a `*mut _ as *mut _` double cast that bridged the
    // port's two incompatible declarations of one struct. T4b.3c deleted both. The
    // one slot that was ever read -- `pWelsSetNonZeroCountFunc`, which clamps every
    // non-zero coefficient count to 1 after inter reconstruction so that deblocking
    // derives the boundary strengths the C++ derives -- is a direct call at its
    // single use in `decode_slice.rs`.

    // 1. Deblocking Filter
    crate::common::deblocking_common::DeblockingInit(&mut (*pCtx).sDeblockingFunc, cpu_flag as i32);

    // 2. Motion Compensation
    crate::common::mc::InitMcFunc(&mut (*pCtx).sMcFunc, cpu_flag);

    // 2b. Reference picture border expansion installed `sExpandPicFunc` here,
    // three constants that T4b.3b turned into direct calls. Both chroma slots held
    // `ExpandPictureChroma_c`, so the aligned/unaligned index selected between two
    // identical functions -- the 4a shape, in the decoder.

    // 3. IDCT Inverse Transform
    (*pCtx).pIdctResAddPredFunc = Some(crate::decoder::decode_mb_aux::IdctResAddPred_c);
    (*pCtx).pIdctResAddPredFunc8x8 = Some(crate::decoder::decode_mb_aux::IdctResAddPred8x8_c);
    (*pCtx).pIdctFourResAddPredFunc = Some(crate::decoder::decode_mb_aux::IdctFourResAddPred_c);

    // 4. Intra Prediction
    (*pCtx).pGetI4x4LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDL_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDR_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVR_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredHD_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVL_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredHU_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcNA_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDLTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVLTop_c),
    ];

    (*pCtx).pGetI16x16LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredPlane_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcNA_c),
    ];

    (*pCtx).pGetIChromaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredPlane_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcNA_c),
    ];

    (*pCtx).pGetI8x8LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDL_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDR_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVR_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredHD_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVL_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredHU_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcNA_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDLTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVLTop_c),
    ];
}

/// Returns the detected host CPU core count.
/// Matches `int32_t GetCPUCount()` in `decoder.cpp`.
pub fn GetCPUCount() -> i32 {
    1
}

/// Detects SIMD hardware capabilities.
/// Matches `uint32_t WelsCPUFeatureDetect (int32_t* pCPUFlag)` in `decoder.cpp`.
pub unsafe fn WelsCPUFeatureDetect(pCpuCores: *mut i32) -> u32 {
    if !pCpuCores.is_null() {
        *pCpuCores = GetCPUCount();
    }
    0
}

/// Initializes CPU feature detection and decoder function tables.
/// Matches `int32_t WelsOpenDecoder (PWelsDecoderContext pCtx, SLogContext* pLogCtx)` in `decoder.cpp:52`.
/// Fill data fields in default for decoder context.
/// Matches `void WelsDecoderDefaults (PWelsDecoderContext pCtx, SLogContext* pLogCtx)` in `decoder.cpp`.
pub unsafe fn WelsDecoderDefaults(pCtx: PWelsDecoderContext, _pLogCtx: *mut c_void) {
    if pCtx.is_null() {
        return;
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
    if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).iPrevFrameNum = -1;
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
    if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb = None;
    }
    if !(*pCtx).pDecoderStatistics.is_null() {
        (*(*pCtx).pDecoderStatistics).iAvgLumaQp = -1;
        (*(*pCtx).pDecoderStatistics).iStatisticsLogInterval = 1000;
    }
    (*pCtx).bUseScalingList = false;
    (*pCtx).iFeedbackNalRefIdc = -1;
    if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb = 0;
        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb = 0;
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
pub unsafe fn ResetReorderingPictureBuffers(
    pPictReoderingStatus: *mut crate::decoder::decoder_context::SPictReoderingStatus,
    pPictInfo: *mut crate::decoder::decoder_context::SPictInfo,
    fullReset: bool,
) {
    if pPictReoderingStatus.is_null() || pPictInfo.is_null() {
        return;
    }
    let pictInfoListCount = if fullReset {
        16
    } else {
        (*pPictReoderingStatus).iLargestBufferedPicIndex + 1
    };
    (*pPictReoderingStatus).iPictInfoIndex = 0;
    (*pPictReoderingStatus).iMinPOC = crate::decoder::decoder_context::IMinInt32;
    (*pPictReoderingStatus).iNumOfPicts = 0;
    (*pPictReoderingStatus).iLastWrittenPOC = crate::decoder::decoder_context::IMinInt32;
    (*pPictReoderingStatus).iLargestBufferedPicIndex = 0;
    for i in 0..pictInfoListCount {
        (*pPictInfo.add(i as usize)).iPOC = crate::decoder::decoder_context::IMinInt32;
        (*pPictInfo.add(i as usize)).iPicBuffIdx = -1;
    }
    (*pPictInfo).sBufferInfo.iBufferStatus = 0;
    (*pPictReoderingStatus).bHasBSlice = false;
}

pub unsafe fn WelsOpenDecoder(pCtx: PWelsDecoderContext, _pLogCtx: *mut c_void) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut cpu_cores = 0i32;
    (*pCtx).uiCpuFlag = WelsCPUFeatureDetect(&mut cpu_cores) as u32;
    WelsInitDecoderFuncs(pCtx);
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
pub unsafe fn WelsFreeDynamicMemory(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pMa = (*pCtx).pMemAlign;

    UninitialDqLayersContext(pCtx);
    crate::decoder::nalu::ResetFmoList(pCtx);
    WelsResetRefPic(pCtx);

    if (*pCtx).pPicBuff.is_some() {
        // `.take()` is the C's `ppPicBuf` out-parameter: it reads the pool and nulls
        // the field in one expression, so `DestroyPicBuff` cannot return with the
        // context still naming a pool it has freed.
        let pool = (*pCtx).pPicBuff.take();
        crate::decoder::pic_queue::DestroyPicBuff(pCtx, pool, pMa);
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
pub unsafe fn WelsEndDecoder(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    WelsFreeDynamicMemory(pCtx);
    WelsFreeStaticMemory(pCtx);
    (*pCtx).bParamSetsLostFlag = false;
    (*pCtx).bNewSeqBegin = false;
    (*pCtx).bPrintFrameErrorTraceFlag = false;
    (*pCtx).iIgnoredErrorInfoPacketCount = 0;
    (*pCtx).bFrameFinish = false;
}

pub unsafe fn WelsInitStaticMemory(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    WelsOpenDecoder(pCtx, std::ptr::null_mut());
    // F19: freed by the context's drop glue. `MAX_NAL_UNIT_NUM_IN_AU` is still the
    // caller's argument, exactly as `decoder_core.cpp:763` passes it.
    (*pCtx).access_unit = Some(SAccessUnit::with_nodes(MAX_NAL_UNIT_NUM_IN_AU));
    if InitBsBuffer(pCtx) != 0 {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    (*pCtx).uiTargetDqId = 255;
    (*pCtx).bEndOfStreamFlag = false;
    ERR_NONE
}

pub unsafe fn WelsFreeStaticMemory(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pMa = (*pCtx).pMemAlign;
    // R4, first entry: the access unit's free is `Option::take`'s drop, and it happens
    // here rather than in the context's own `Drop` only because this function still
    // exists. Both of its callers run `drop(Box::from_raw(pCtx))` on the next line, so
    // moving it either way is the same program.
    (*pCtx).access_unit = None;

    // The buffers own their allocations now; reset releases them (the WelsFreeHelper
    // free-cascade entries for sRawData/sSavedData died with the pointers).
    (*pCtx).sRawData.reset();

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        (*pCtx).sSavedData.reset();

        if !(*pCtx).pParserBsInfo.is_null() {
            let pParser = (*pCtx).pParserBsInfo;
            if !(*pParser).pNalLenInByte.is_null() {
                WelsFreeHelper(pMa, (*pParser).pNalLenInByte as *mut u8, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>());
                (*pParser).pNalLenInByte = std::ptr::null_mut();
                (*pCtx).iMaxNalNum = 0;
            }
            if !(*pParser).pDstBuff.is_null() {
                WelsFreeHelper(pMa, (*pParser).pDstBuff, MAX_ACCESS_UNIT_CAPACITY);
                (*pParser).pDstBuff = std::ptr::null_mut();
            }
            WelsFreeHelper(pMa, pParser as *mut u8, std::mem::size_of::<SParserBsInfo>());
            (*pCtx).pParserBsInfo = std::ptr::null_mut();
        }
    }
}

// A duplicate `DecodeNalHeaderExt` was deleted dead here at T3.3 (S18): it had no
// callers — both call sites resolve to `nalu::DecodeNalHeaderExt`, which takes the
// 3-byte window as a slice since this seam.

pub unsafe fn UpdateDecoderStatisticsForActiveParaset(
    pDecoderStatistics: *mut SDecoderStatistics,
    pSps: PSps,
    pPps: PPps,
) {
    if pDecoderStatistics.is_null() || pSps.is_null() || pPps.is_null() {
        return;
    }
    let pSps = pSps as *mut SSps;
    let pPps = pPps as *mut SPps;
    (*pDecoderStatistics).iCurrentActiveSpsId = (*pSps).iSpsId;
    (*pDecoderStatistics).iCurrentActivePpsId = (*pPps).iPpsId;
    (*pDecoderStatistics).uiProfile = (*pSps).uiProfileIdc as u32;
    (*pDecoderStatistics).uiLevel = (*pSps).uiLevelIdc as u32;
}

pub unsafe fn ParseSliceHeaderSyntaxs(
    pCtx: PWelsDecoderContext,
    buf: &[u8],
    pBs: &mut BsCursor,
    kbExtensionFlag: bool,
) -> i32 {
    // The access unit is borrowed for exactly these three lines. `kpCurNal` outlives
    // it — it is a copy of a stored node pointer, and the whole 576-line body below
    // runs against the node, not against the list.
    let kpCurNal = match cur_au(pCtx) {
        None => return ERR_INFO_INVALID_PTR,
        Some(au) if au.uiAvailUnitsNum == 0 => return ERR_INFO_OUT_OF_MEMORY,
        Some(au) => au.nal((au.uiAvailUnitsNum - 1) as usize),
    };
    if kpCurNal.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }

    // **S25 (F24) — none of these three is a borrow, and that is the fix.**
    // `sSliceHeader` sits *inside* `sSliceHeaderExt`, so taking `&mut` of each puts the
    // outer's Unique retag on top of the inner's and every later write through the inner
    // is UB — Miri, byte-exact: `[0x28..0xf00]` created here, invalidated by the retag at
    // `[0x28..0x1350]`, caught at the `iFirstMbInSlice` store 13 lines down. `pSliceHead`
    // is used 74 times after that point and `pSliceHeadExt` 50, so the whole 576-line
    // function ran on an invalid borrow stack. `addr_of_mut!` creates no reference: all
    // three pointers carry `kpCurNal`'s own provenance, none can invalidate another, and
    // the raw `bSliceHeaderExtFlag` write below is no longer racing two live borrows.
    // `pNalHeaderExt` is a sibling field and was never part of the defect; it moves with
    // them so the rule reads uniformly. T5.B2's shape (`manage_dec_ref.rs`, `SetUnRef`):
    // no borrow outlives one expression. Anything added here inherits it.
    let pNalHeaderExt: PNalUnitHeaderExt = std::ptr::addr_of_mut!((*kpCurNal).sNalHeaderExt);
    let pSliceHead: PSliceHeader =
        std::ptr::addr_of_mut!((*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader);
    let eNalType = (*pNalHeaderExt).sNalUnitHeader.eNalUnitType;
    let pSliceHeadExt: PSliceHeaderExt =
        std::ptr::addr_of_mut!((*kpCurNal).sNalData.sVclNal.sSliceHeaderExt);

    (*kpCurNal).sNalData.sVclNal.bSliceHeaderExtFlag = kbExtensionFlag;

    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode > 36863 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FIRST_MB_IN_SLICE);
    }
    (*pSliceHead).iFirstMbInSlice = uiCode as i32;

    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
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

    (*pSliceHead).eSliceType = match uiSliceType {
        0 => P_SLICE,
        1 => B_SLICE,
        2 => I_SLICE,
        3 => SP_SLICE,
        _ => SI_SLICE,
    };

    if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode >= MAX_PPS_COUNT as u32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_PPS_ID_OVERFLOW);
    }
    let iPpsId = uiCode as i32;

    if !(*pCtx).sSpsPpsCtx.bPpsAvailFlags[iPpsId as usize] {
        if !(*pCtx).pDecoderStatistics.is_null() {
            (*(*pCtx).pDecoderStatistics).iPpsReportErrorNum += 1;
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
    // does not exist. The stored `(*pSliceHead).pPps` / `pSps` pointers get the
    // context's provenance for free, which is what S28 asks of a pointer that outlives
    // the expression that made it.
    let pPps: PPps = std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sPpsBuffer[iPpsId as usize]);
    if (*pPps).uiNumSliceGroups == 0 {
        (*pCtx).iErrorCode |= dsNoParamSets;
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_NO_PARAM_SETS);
    }

    let pSps: PSps = if kbExtensionFlag {
        std::ptr::addr_of_mut!(
            (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[(*pPps).iSpsId as usize].sSps
        )
    } else {
        std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSpsBuffer[(*pPps).iSpsId as usize])
    };

    if (*pSps).iNumRefFrames == 0
        && (*pSliceHead).eSliceType != I_SLICE
        && (*pSliceHead).eSliceType != SI_SLICE
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
    }

    (*pSliceHead).iPpsId = iPpsId;
    (*pSliceHead).iSpsId = (*pPps).iSpsId;
    (*pSliceHead).pPps = pPps as *mut c_void;
    (*pSliceHead).pSps = pSps as *mut c_void;
    if kbExtensionFlag {
        let pSubsetSps: PSubsetSps =
            std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[(*pPps).iSpsId as usize]);
        (*pSliceHeadExt).pSubsetSps = pSubsetSps as *mut c_void;
    }

    let bIdrFlag = (!kbExtensionFlag && eNalType == NAL_UNIT_CODED_SLICE_IDR)
        || (kbExtensionFlag && (*pNalHeaderExt).bIdrFlag);
    (*pSliceHead).bIdrFlag = bIdrFlag;

    if (*pSps).uiLog2MaxFrameNum == 0 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_NO_PARAM_SETS);
    }
    if ((*pSliceHead).iFirstMbInSlice as u32) > (*pSps).uiTotalMbCount - 1 {
        return GENERATE_ERROR_NO(
            ERR_LEVEL_SLICE_HEADER,
            ERR_INFO_INVALID_FIRST_MB_IN_SLICE,
        );
    }
    if BsGetBits(buf, pBs, (*pSps).uiLog2MaxFrameNum, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    (*pSliceHead).iFrameNum = uiCode as i32;
    if !(*pSps).bFrameMbsOnlyFlag {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MBAFF);
    }
    (*pSliceHead).iMbWidth = (*pSps).iMbWidth as i32;
    (*pSliceHead).iMbHeight = (*pSps).iMbHeight as i32;

    if bIdrFlag {
        if (*pSliceHead).iFrameNum != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FRAME_NUM);
        }
        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > SLICE_HEADER_IDR_PIC_ID_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_IDR_PIC_ID);
        }
        (*pSliceHead).uiIdrPicId = uiCode as u16;
    }

    (*pSliceHead).iDeltaPicOrderCntBottom = 0;
    (*pSliceHead).iDeltaPicOrderCnt[0] = 0;
    (*pSliceHead).iDeltaPicOrderCnt[1] = 0;
    if (*pSps).uiPocType == 0 {
        if BsGetBits(buf, pBs, (*pSps).iLog2MaxPocLsb as u32, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        let iMaxPocLsb = 1 << (*pSps).iLog2MaxPocLsb;
        let pocLsb = uiCode as i32;
        (*pSliceHead).iPicOrderCntLsb = pocLsb;
        if (*pPps).bPicOrderPresentFlag && !(*pSliceHead).bFieldPicFlag {
            if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHead).iDeltaPicOrderCntBottom = iCode;
        }
        let prevLsb = if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb
        } else {
            0
        };
        let prevMsb = if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb
        } else {
            0
        };
        let pocMsb = if pocLsb < prevLsb && (prevLsb - pocLsb) >= (iMaxPocLsb / 2) {
            prevMsb + iMaxPocLsb
        } else if pocLsb > prevLsb && (pocLsb - prevLsb) > (iMaxPocLsb / 2) {
            prevMsb - iMaxPocLsb
        } else {
            prevMsb
        };
        (*pSliceHead).iPicOrderCntLsb = pocMsb + pocLsb;
        if (*pPps).bPicOrderPresentFlag && !(*pSliceHead).bFieldPicFlag {
            (*pSliceHead).iPicOrderCntLsb += (*pSliceHead).iDeltaPicOrderCntBottom;
        }
        if !(*pCtx).pLastDecPicInfo.is_null() && (*pNalHeaderExt).sNalUnitHeader.uiNalRefIdc != 0 {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb = pocLsb;
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb = pocMsb;
        }
    } else if (*pSps).uiPocType == 1 && !(*pSps).bDeltaPicOrderAlwaysZeroFlag {
        if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSliceHead).iDeltaPicOrderCnt[0] = iCode;
        if (*pPps).bPicOrderPresentFlag && !(*pSliceHead).bFieldPicFlag {
            if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHead).iDeltaPicOrderCnt[1] = iCode;
        }
    }

    (*pSliceHead).iRedundantPicCnt = 0;
    if (*pPps).bRedundantPicCntPresentFlag {
        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > SLICE_HEADER_REDUNDANT_PIC_CNT_MAX {
            return GENERATE_ERROR_NO(
                ERR_LEVEL_SLICE_HEADER,
                ERR_INFO_INVALID_REDUNDANT_PIC_CNT,
            );
        }
        (*pSliceHead).iRedundantPicCnt = uiCode as i32;
        if (*pSliceHead).iRedundantPicCnt > 0 {
            return GENERATE_ERROR_NO(
                ERR_LEVEL_SLICE_HEADER,
                ERR_INFO_INVALID_REDUNDANT_PIC_CNT,
            );
        }
    }

    if (*pSliceHead).eSliceType == B_SLICE {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSliceHead).iDirectSpatialMvPredFlag = uiCode as i32;
    }

    (*pSliceHead).uiRefCount[0] = (*pPps).uiNumRefIdxL0Active as i32;
    (*pSliceHead).uiRefCount[1] = (*pPps).uiNumRefIdxL1Active as i32;

    let mut bReadNumRefFlag = (*pSliceHead).eSliceType == P_SLICE
        || (*pSliceHead).eSliceType == B_SLICE;
    if kbExtensionFlag {
        bReadNumRefFlag &= (*pNalHeaderExt).uiQualityId == BASE_QUALITY_ID;
    }
    if bReadNumRefFlag {
        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSliceHead).bNumRefIdxActiveOverrideFlag = uiCode != 0;
        if (*pSliceHead).bNumRefIdxActiveOverrideFlag {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode > MAX_NUM_REF_IDX_L0_ACTIVE_MINUS1 {
                return GENERATE_ERROR_NO(
                    ERR_LEVEL_SLICE_HEADER,
                    ERR_INFO_INVALID_NUM_REF_IDX_L0_ACTIVE_MINUS1,
                );
            }
            (*pSliceHead).uiRefCount[0] = (1 + uiCode) as i32;
            if (*pSliceHead).eSliceType == B_SLICE {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if uiCode > MAX_NUM_REF_IDX_L1_ACTIVE_MINUS1 {
                    return GENERATE_ERROR_NO(
                        ERR_LEVEL_SLICE_HEADER,
                        ERR_INFO_INVALID_NUM_REF_IDX_L1_ACTIVE_MINUS1,
                    );
                }
                (*pSliceHead).uiRefCount[1] = (1 + uiCode) as i32;
            }
        }
    }
    if ((*pSliceHead).uiRefCount[0] as usize) > MAX_REF_PIC_COUNT
        || ((*pSliceHead).uiRefCount[1] as usize) > MAX_REF_PIC_COUNT
    {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_REF_COUNT_OVERFLOW);
    }

    if (*pNalHeaderExt).uiQualityId == BASE_QUALITY_ID {
        let iRet = ParseRefPicListReordering(buf, pBs, pSliceHead);
        if iRet != ERR_NONE {
            return iRet;
        }

        // pred_weight_table(): present for weighted P slices and for B slices when
        // weighted_bipred_idc == 1. Skipping it desynchronises the rest of the
        // slice header (`decoder_core.cpp`).
        if ((*pPps).bWeightedPredFlag && uiSliceType == P_SLICE as u32)
            || ((*pPps).uiWeightedBipredIdc == 1 && uiSliceType == B_SLICE as u32)
        {
            let iRet = ParsePredWeightedTable(buf, pBs, pSliceHead);
            if iRet != ERR_NONE {
                return iRet;
            }
        }

        if kbExtensionFlag {
            (*pSliceHeadExt).bBasePredWeightTableFlag =
                !((*pNalHeaderExt).bNoInterLayerPredFlag || (*pNalHeaderExt).uiQualityId > 0);
        }

        if (*pNalHeaderExt).sNalUnitHeader.uiNalRefIdc != 0 {
            let iRet = ParseDecRefPicMarking(pCtx, buf, pBs, pSliceHead, pSps, bIdrFlag);
            if iRet != ERR_NONE {
                return iRet;
            }
            if kbExtensionFlag {
                let pSubsetSps: PSubsetSps = std::ptr::addr_of_mut!(
                    (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[(*pPps).iSpsId as usize]
                );
                if !(*pSubsetSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
                    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    (*pSliceHeadExt).bStoreRefBasePicFlag = uiCode != 0;
                    if ((*pNalHeaderExt).bUseRefBasePicFlag
                        || (*pSliceHeadExt).bStoreRefBasePicFlag)
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

    if (*pPps).bEntropyCodingModeFlag {
        if (*pSliceHead).eSliceType != I_SLICE && (*pSliceHead).eSliceType != SI_SLICE {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode > SLICE_HEADER_CABAC_INIT_IDC_MAX {
                return ERR_INFO_INVALID_CABAC_INIT_IDC;
            }
            (*pSliceHead).iCabacInitIdc = uiCode as i32;
        } else {
            (*pSliceHead).iCabacInitIdc = 0;
        }
    }

    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    (*pSliceHead).iSliceQpDelta = iCode;
    (*pSliceHead).iSliceQp = (*pPps).iPicInitQp + (*pSliceHead).iSliceQpDelta;
    if (*pSliceHead).iSliceQp < 0 || (*pSliceHead).iSliceQp > 51 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_QP);
    }

    (*pSliceHead).uiDisableDeblockingFilterIdc = 0;
    (*pSliceHead).iSliceAlphaC0Offset = 0;
    (*pSliceHead).iSliceBetaOffset = 0;
    if (*pPps).bDeblockingFilterControlPresentFlag {
        if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        (*pSliceHead).uiDisableDeblockingFilterIdc = uiCode;
        if (*pSliceHead).uiDisableDeblockingFilterIdc > 6 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_DBLOCKING_IDC);
        }
        if (*pSliceHead).uiDisableDeblockingFilterIdc != 1 {
            if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHead).iSliceAlphaC0Offset = iCode * 2;
            if (*pSliceHead).iSliceAlphaC0Offset < SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN
                || (*pSliceHead).iSliceAlphaC0Offset > SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX
            {
                return GENERATE_ERROR_NO(
                    ERR_LEVEL_SLICE_HEADER,
                    ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2,
                );
            }
            if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHead).iSliceBetaOffset = iCode * 2;
            if (*pSliceHead).iSliceBetaOffset < SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN
                || (*pSliceHead).iSliceBetaOffset > SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX
            {
                return GENERATE_ERROR_NO(
                    ERR_LEVEL_SLICE_HEADER,
                    ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2,
                );
            }
        }
    }

    let mut bSgChangeCycleInvolved = (*pPps).uiNumSliceGroups > 1
        && (*pPps).uiSliceGroupMapType >= 3
        && (*pPps).uiSliceGroupMapType <= 5;
    if kbExtensionFlag && bSgChangeCycleInvolved {
        bSgChangeCycleInvolved =
            bSgChangeCycleInvolved && ((*pNalHeaderExt).uiQualityId == BASE_QUALITY_ID);
    }
    if bSgChangeCycleInvolved {
        if (*pPps).uiSliceGroupChangeRate > 0 {
            let kiNumBits = ((1 + (*pPps).uiPicSizeInMapUnits / (*pPps).uiSliceGroupChangeRate)
                as f64)
                .log2()
                .ceil() as u32;
            if BsGetBits(buf, pBs, kiNumBits, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHead).iSliceGroupChangeCycle = uiCode as i32;
        } else {
            (*pSliceHead).iSliceGroupChangeCycle = 0;
        }
    }

    if !kbExtensionFlag {
        FillDefaultSliceHeaderExt(pSliceHeadExt, pNalHeaderExt);
    } else {
        // Extra syntax elements newly introduced (G.7.3.3.4). These bits are part of
        // the slice header, so skipping them desynchronises the slice-data parse.
        let pSubsetSps: PSubsetSps =
            std::ptr::addr_of_mut!((*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[(*pPps).iSpsId as usize]);
        (*pSliceHeadExt).pSubsetSps = pSubsetSps as *mut c_void;

        if !(*pNalHeaderExt).bNoInterLayerPredFlag
            && BASE_QUALITY_ID == (*pNalHeaderExt).uiQualityId
        {
            if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHeadExt).uiRefLayerDqId = uiCode as u8; //ref_layer_dq_id
            if (*pSubsetSps).sSpsSvcExt.bInterLayerDeblockingFilterCtrlPresentFlag {
                if BsGetUe(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                //disable_inter_layer_deblocking_filter_idc
                (*pSliceHeadExt).uiDisableInterLayerDeblockingFilterIdc = uiCode;
                if (*pSliceHeadExt).uiDisableInterLayerDeblockingFilterIdc > 6 {
                    return GENERATE_ERROR_NO(
                        ERR_LEVEL_SLICE_HEADER,
                        ERR_INFO_INVALID_DBLOCKING_IDC,
                    );
                }
                if (*pSliceHeadExt).uiDisableInterLayerDeblockingFilterIdc != 1 {
                    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //inter_layer_slice_alpha_c0_offset_div2
                    (*pSliceHeadExt).iInterLayerSliceAlphaC0Offset = iCode * 2;
                    if (*pSliceHeadExt).iInterLayerSliceAlphaC0Offset
                        < SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN
                        || (*pSliceHeadExt).iInterLayerSliceAlphaC0Offset
                            > SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX
                    {
                        return GENERATE_ERROR_NO(
                            ERR_LEVEL_SLICE_HEADER,
                            ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2,
                        );
                    }
                    if BsGetSe(buf, pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //inter_layer_slice_beta_offset_div2
                    (*pSliceHeadExt).iInterLayerSliceBetaOffset = iCode * 2;
                    if (*pSliceHeadExt).iInterLayerSliceBetaOffset
                        < SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN
                        || (*pSliceHeadExt).iInterLayerSliceBetaOffset
                            > SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX
                    {
                        return GENERATE_ERROR_NO(
                            ERR_LEVEL_SLICE_HEADER,
                            ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2,
                        );
                    }
                }
            }

            (*pSliceHeadExt).uiRefLayerChromaPhaseXPlus1Flag =
                (*pSubsetSps).sSpsSvcExt.uiSeqRefLayerChromaPhaseXPlus1Flag;
            (*pSliceHeadExt).uiRefLayerChromaPhaseYPlus1 =
                (*pSubsetSps).sSpsSvcExt.uiSeqRefLayerChromaPhaseYPlus1;

            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHeadExt).bConstrainedIntraResamplingFlag = uiCode != 0;

            {
                let scaled = &(*pSubsetSps).sSpsSvcExt.sSeqScaledRefLayer;
                let iLeftOffset = scaled.iLeftOffset;
                let iTopOffset = scaled.iTopOffset * (2 - (*pSps).bFrameMbsOnlyFlag as i32);
                let iRightOffset = scaled.iRightOffset;
                let iBottomOffset = scaled.iBottomOffset * (2 - (*pSps).bFrameMbsOnlyFlag as i32);
                (*pSliceHeadExt).iScaledRefLayerPicWidthInSampleLuma =
                    ((*pSliceHead).iMbWidth << 4) - (iLeftOffset + iRightOffset);
                (*pSliceHeadExt).iScaledRefLayerPicHeightInSampleLuma =
                    ((*pSliceHead).iMbHeight << 4)
                        - (iTopOffset + iBottomOffset) / (1 + (*pSliceHead).bFieldPicFlag as i32);
            }
        } else if (*pNalHeaderExt).uiQualityId > BASE_QUALITY_ID {
            // MGS not supported.
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MGS);
        } else {
            (*pSliceHeadExt).uiRefLayerDqId = u8::MAX;
        }

        (*pSliceHeadExt).bSliceSkipFlag = false;
        (*pSliceHeadExt).bAdaptiveBaseModeFlag = false;
        (*pSliceHeadExt).bDefaultBaseModeFlag = false;
        (*pSliceHeadExt).bAdaptiveMotionPredFlag = false;
        (*pSliceHeadExt).bDefaultMotionPredFlag = false;
        (*pSliceHeadExt).bAdaptiveResidualPredFlag = false;
        (*pSliceHeadExt).bDefaultResidualPredFlag = false;
        (*pSliceHeadExt).bTCoeffLevelPredFlag = if (*pNalHeaderExt).bNoInterLayerPredFlag {
            false
        } else {
            (*pSubsetSps).sSpsSvcExt.bSeqTCoeffLevelPredFlag
        };

        if !(*pNalHeaderExt).bNoInterLayerPredFlag {
            if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHeadExt).bSliceSkipFlag = uiCode != 0; //slice_skip_flag
            if (*pSliceHeadExt).bSliceSkipFlag {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_SLICESKIP);
            } else {
                if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                (*pSliceHeadExt).bAdaptiveBaseModeFlag = uiCode != 0; //adaptive_base_mode_flag
                if !(*pSliceHeadExt).bAdaptiveBaseModeFlag {
                    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    (*pSliceHeadExt).bDefaultBaseModeFlag = uiCode != 0; //default_base_mode_flag
                }
                if !(*pSliceHeadExt).bDefaultBaseModeFlag {
                    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //adaptive_motion_prediction_flag
                    (*pSliceHeadExt).bAdaptiveMotionPredFlag = uiCode != 0;
                    if !(*pSliceHeadExt).bAdaptiveMotionPredFlag {
                        if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                            return ERR_INFO_INVALID_ACCESS;
                        }
                        //default_motion_prediction_flag
                        (*pSliceHeadExt).bDefaultMotionPredFlag = uiCode != 0;
                    }
                }

                if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                //adaptive_residual_prediction_flag
                (*pSliceHeadExt).bAdaptiveResidualPredFlag = uiCode != 0;
                if !(*pSliceHeadExt).bAdaptiveResidualPredFlag {
                    if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    //default_residual_prediction_flag
                    (*pSliceHeadExt).bDefaultResidualPredFlag = uiCode != 0;
                }
            }
            if (*pSubsetSps).sSpsSvcExt.bAdaptiveTCoeffLevelPredFlag {
                if BsGetOneBit(buf, pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                //tcoeff_level_prediction_flag
                (*pSliceHeadExt).bTCoeffLevelPredFlag = uiCode != 0;
            }
        }

        if !(*pSubsetSps).sSpsSvcExt.bSliceHeaderRestrictionFlag {
            if BsGetBits(buf, pBs, 4, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHeadExt).uiScanIdxStart = uiCode as u8; //scan_idx_start
            if BsGetBits(buf, pBs, 4, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            (*pSliceHeadExt).uiScanIdxEnd = uiCode as u8; //scan_idx_end
            if (*pSliceHeadExt).uiScanIdxStart != 0 || (*pSliceHeadExt).uiScanIdxEnd != 15 {
                return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_UNSUPPORTED_MGS);
            }
        } else {
            (*pSliceHeadExt).uiScanIdxStart = 0;
            (*pSliceHeadExt).uiScanIdxEnd = 15;
        }
    }

    ERR_NONE
}

pub unsafe fn PrefetchNalHeaderExtSyntax(
    pCtx: PWelsDecoderContext,
    kpDst: PNalUnit,
    kpSrc: *const SNalUnit,
) -> bool {
    if kpDst.is_null() || kpSrc.is_null() {
        return false;
    }
    let pNalHdrExtD = &mut (*kpDst).sNalHeaderExt;
    let pNalHdrExtS = &(*kpSrc).sNalHeaderExt;
    let pShExtD = &mut (*kpDst).sNalData.sVclNal.sSliceHeaderExt;
    let pPrefixS = &(*kpSrc).sNalData.sPrefixNal;

    pNalHdrExtD.uiDependencyId = pNalHdrExtS.uiDependencyId;
    pNalHdrExtD.uiQualityId = pNalHdrExtS.uiQualityId;
    pNalHdrExtD.uiTemporalId = pNalHdrExtS.uiTemporalId;
    pNalHdrExtD.uiPriorityId = pNalHdrExtS.uiPriorityId;
    pNalHdrExtD.bIdrFlag = pNalHdrExtS.bIdrFlag;
    pNalHdrExtD.bNoInterLayerPredFlag = pNalHdrExtS.bNoInterLayerPredFlag;
    pNalHdrExtD.bDiscardableFlag = pNalHdrExtS.bDiscardableFlag;
    pNalHdrExtD.bOutputFlag = pNalHdrExtS.bOutputFlag;
    pNalHdrExtD.bUseRefBasePicFlag = pNalHdrExtS.bUseRefBasePicFlag;
    pNalHdrExtD.uiLayerDqId = pNalHdrExtS.uiLayerDqId;

    (*pShExtD).bStoreRefBasePicFlag = pPrefixS.bStoreRefBasePicFlag;
    (*pShExtD).sRefBasePicMarking = pPrefixS.sRefPicBaseMarking;
    true
}

pub unsafe fn UpdateAccessUnit(pCtx: PWelsDecoderContext) -> i32 {
    let Some(pCurAu) = cur_au(pCtx) else {
        return ERR_INFO_INVALID_PTR;
    };
    let iIdx = pCurAu.uiEndPos as usize;
    let dq_id = if iIdx < pCurAu.count() as usize {
        Some((*pCurAu.nal(iIdx)).sNalHeaderExt.uiLayerDqId)
    } else {
        None
    };
    pCurAu.uiActualUnitsNum = pCurAu.uiEndPos + 1;
    pCurAu.bCompletedAuFlag = true;
    if let Some(dq_id) = dq_id {
        (*pCtx).uiTargetDqId = dq_id;
    }
    ERR_NONE
}

pub unsafe fn InitialDqLayersContext(
    pCtx: PWelsDecoderContext,
    kiMaxWidth: i32,
    kiMaxHeight: i32,
) -> i32 {
    if pCtx.is_null() || kiMaxWidth <= 0 || kiMaxHeight <= 0 {
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

pub unsafe fn UninitialDqLayersContext(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
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

pub unsafe fn ResetCurrentAccessUnit(pCtx: PWelsDecoderContext) {
    let Some(pCurAu) = cur_au(pCtx) else {
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
            pCurAu.nal_units.swap(kuiActualNum as usize + iIdx, iIdx);
        }
        pCurAu.uiActualUnitsNum = kuiLeftNum;
        pCurAu.uiAvailUnitsNum = kuiLeftNum;
    }
}

pub fn ForceResetCurrentAccessUnit(pAu: &mut SAccessUnit) {
    let mut uiSucAuIdx = pAu.uiEndPos + 1;
    let mut uiCurAuIdx = 0;
    while uiSucAuIdx < pAu.uiAvailUnitsNum {
        pAu.nal_units.swap(uiSucAuIdx as usize, uiCurAuIdx as usize);
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

pub unsafe fn ForceResetParaSetStatusAndAUList(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag = false;

    if let Some(pAu) = cur_au(pCtx) {
        pAu.uiAvailUnitsNum = 0;
        pAu.uiActualUnitsNum = 0;
        pAu.uiStartPos = 0;
        pAu.uiEndPos = 0;
        pAu.bCompletedAuFlag = false;
    }
}

pub unsafe fn CheckAvailNalUnitsListContinuity(
    pCtx: PWelsDecoderContext,
    iStartIdx: i32,
    iEndIdx: i32,
) {
    let Some(pCurAu) = cur_au(pCtx) else {
        return;
    };
    let mut uiLastNuDependencyId = (*pCurAu.nal(iStartIdx as usize)).sNalHeaderExt.uiDependencyId;
    let mut uiLastNuLayerDqId = (*pCurAu.nal(iStartIdx as usize)).sNalHeaderExt.uiLayerDqId;
    let mut iCurNalUnitIdx = iStartIdx + 1;

    while iCurNalUnitIdx <= iEndIdx {
        let pNal = pCurAu.nal(iCurNalUnitIdx as usize);
        let uiCurNuDependencyId = (*pNal).sNalHeaderExt.uiDependencyId;
        let uiCurNuQualityId = (*pNal).sNalHeaderExt.uiQualityId;
        let uiCurNuLayerDqId = (*pNal).sNalHeaderExt.uiLayerDqId;
        let uiCurNuRefLayerDqId = (*pNal).sNalData.sVclNal.sSliceHeaderExt.uiRefLayerDqId;

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

pub unsafe fn RefineIdxNoInterLayerPred(pCurAu: &SAccessUnit, pIdxNoInterLayerPred: *mut i32) {
    if pIdxNoInterLayerPred.is_null() {
        return;
    }
    let idx = *pIdxNoInterLayerPred as usize;
    let pNal = pCurAu.nal(idx);
    if pNal.is_null() {
        return;
    }
    let iLastNalDependId = (*pNal).sNalHeaderExt.uiDependencyId;
    let iLastNalQualityId = (*pNal).sNalHeaderExt.uiQualityId;
    let uiLastNalTId = (*pNal).sNalHeaderExt.uiTemporalId;
    let iLastNalFrameNum = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
    let iLastNalPoc = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
    let iLastNalFirstMb = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

    let mut bMultiSliceFind = false;
    let mut iFinalIdxNoInterLayerPred = 0;
    let mut iCurIdx = (*pIdxNoInterLayerPred) - 1;

    while iCurIdx >= 0 {
        let pCurNal = (*pCurAu).nal(iCurIdx as usize);
        if !pCurNal.is_null() && (*pCurNal).sNalHeaderExt.bNoInterLayerPredFlag {
            let iCurNalDependId = (*pCurNal).sNalHeaderExt.uiDependencyId;
            let iCurNalQualityId = (*pCurNal).sNalHeaderExt.uiQualityId;
            let iCurNalTId = (*pCurNal).sNalHeaderExt.uiTemporalId;
            let iCurNalFrameNum = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
            let iCurNalPoc = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
            let iCurNalFirstMb = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

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

pub unsafe fn CheckPocOfCurValidNalUnits(pCurAu: &SAccessUnit, pIdxNoInterLayerPred: i32) -> bool {
    let iEndIdx = pCurAu.uiEndPos as i32;
    let iCurAuPoc = (*pCurAu.nal(pIdxNoInterLayerPred as usize))
        .sNalData
        .sVclNal
        .sSliceHeaderExt
        .sSliceHeader
        .iPicOrderCntLsb;

    for i in (pIdxNoInterLayerPred + 1)..iEndIdx {
        let iTmpPoc = (*pCurAu.nal(i as usize))
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .iPicOrderCntLsb;
        if iTmpPoc != iCurAuPoc {
            return false;
        }
    }
    true
}

pub unsafe fn CheckIntegrityNalUnitsList(pCtx: PWelsDecoderContext) -> bool {
    let Some(pCurAu) = cur_au(pCtx) else {
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

        let Some(pCurAu) = cur_au(pCtx) else {
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

pub unsafe fn CheckOnlyOneLayerInAu(pCtx: PWelsDecoderContext) {
    let Some(pCurAu) = cur_au(pCtx) else {
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

pub unsafe fn WelsDecodeAccessUnitStart(pCtx: PWelsDecoderContext) -> i32 {
    let iRet = UpdateAccessUnit(pCtx);
    if iRet != ERR_NONE {
        return iRet;
    }
    if let Some(au) = cur_au(pCtx) {
        au.uiStartPos = 0;
    }
    if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag && !CheckIntegrityNalUnitsList(pCtx) {
        (*pCtx).iErrorCode |= dsBitstreamError;
        { return dsBitstreamError; }
    }
    if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag {
        CheckOnlyOneLayerInAu(pCtx);
    }
    ERR_NONE
}

pub unsafe fn WelsDecodeAccessUnitEnd(pCtx: PWelsDecoderContext) {
    let Some(pCurAu) = cur_au(pCtx) else {
        return;
    };
    let endIdx = pCurAu.uiEndPos as usize;
    if endIdx < pCurAu.count() as usize {
        let pCurNal = pCurAu.nal(endIdx);
        if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt = (*pCurNal).sNalHeaderExt;
            (*(*pCtx).pLastDecPicInfo).sLastSliceHeader =
                (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
        }
    }
    ResetCurrentAccessUnit(pCtx);
}

pub unsafe fn CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx: PWelsDecoderContext) -> bool {
    let mut bNewSeq = false;
    let mut pTmpLayerSps: [*mut SSps; MAX_LAYER_NUM] = [std::ptr::null_mut(); MAX_LAYER_NUM];

    let Some(pCurAu) = cur_au(pCtx) else {
        return false;
    };
    let start = pCurAu.uiStartPos as usize;
    let end = pCurAu.uiEndPos as usize;
    for i in start..=end {
        if i < pCurAu.count() as usize {
            let pNal = pCurAu.nal(i);
            let uiDid = (*pNal).sNalHeaderExt.uiDependencyId as usize;
            if uiDid < MAX_LAYER_NUM {
                pTmpLayerSps[uiDid] = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
            }
            if (*pNal).sNalHeaderExt.sNalUnitHeader.eNalUnitType == NAL_UNIT_CODED_SLICE_IDR
                || (*pNal).sNalHeaderExt.bIdrFlag
            {
                bNewSeq = true;
            }
        }
    }

    let mut iMaxActiveLayer = 0;
    let mut iMaxCurrentLayer = 0;
    for i in (0..MAX_LAYER_NUM).rev() {
        if !(*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_null() {
            iMaxActiveLayer = i;
            break;
        }
    }
    for i in (0..MAX_LAYER_NUM).rev() {
        if !pTmpLayerSps[i].is_null() {
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
            if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_null() && !pTmpLayerSps[i].is_null() {
                (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = pTmpLayerSps[i];
            }
        }
    } else {
        (*pCtx).sSpsPpsCtx.pActiveLayerSps.copy_from_slice(&pTmpLayerSps);
    }
    bNewSeq
}

pub unsafe fn WriteBackActiveParameters(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
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

pub unsafe fn DecodeFinishUpdate(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    (*pCtx).bNewSeqBegin = false;
    WriteBackActiveParameters(pCtx);
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || (*pCtx).bNextNewSeqBegin;
    (*pCtx).bNextNewSeqBegin = false;
    if (*pCtx).bNewSeqBegin {
        ResetActiveSPSForEachLayer(pCtx);
    }
}

pub unsafe fn WelsDecodeInitAccessUnitStart(
    pCtx: PWelsDecoderContext,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    (*pCtx).bAuReadyFlag = false;
    if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).bLastHasMmco5 = false;
    }
    let bTmpNewSeqBegin = CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx);
    if bTmpNewSeqBegin {
        if !(*pCtx).pStreamSeqNum.is_null() {
            *(*pCtx).pStreamSeqNum += 1;
        } else {
            (*pCtx).iSeqNum += 1;
        }
    }
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || bTmpNewSeqBegin;
    if !(*pCtx).pStreamSeqNum.is_null() {
        (*pCtx).iSeqNum = *(*pCtx).pStreamSeqNum;
    }
    let iErr = WelsDecodeAccessUnitStart(pCtx);
    GetVclNalTemporalId(pCtx);

    if iErr != ERR_NONE {
        if let Some(au) = cur_au(pCtx) {
            ForceResetCurrentAccessUnit(au);
        }
        if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly && !pDstInfo.is_null() {
            (*pDstInfo).iBufferStatus = 0;
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
    let pNal = match cur_au(pCtx) {
        Some(au) if (au.uiStartPos as usize) < au.count() as usize => {
            au.nal(au.uiStartPos as usize)
        }
        _ => std::ptr::null_mut(),
    };
    if !pNal.is_null() {
        (*pCtx).pSps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
        (*pCtx).pPps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pPps as *mut SPps;
    }
    iErr
}

pub unsafe fn AllocPicBuffOnNewSeqBegin(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = if !(*pCtx).pSps.is_null() {
        (*pCtx).pSps
    } else {
        let mut found_sps: *mut SSps = std::ptr::null_mut();
        for sps in (*pCtx).sSpsPpsCtx.sSpsBuffer.iter_mut() {
            if sps.uiTotalMbCount > 0 {
                found_sps = sps as *mut SSps;
                break;
            }
        }
        found_sps
    };

    if pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    (*pCtx).pSps = pSps;

    if GetThreadCount(pCtx) <= 1 {
        WelsResetRefPic(pCtx);
    }
    let iErr = SyncPictureResolutionExt(pCtx, (*pSps).iMbWidth as u32, (*pSps).iMbHeight as u32);
    iErr
}

pub unsafe fn InitConstructAccessUnit(
    pCtx: PWelsDecoderContext,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    let mut iErr = WelsDecodeInitAccessUnitStart(pCtx, pDstInfo);
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

pub unsafe fn ConstructAccessUnit(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
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

    let iErr = DecodeCurrentAccessUnit(pCtx, ppDst, pDstInfo);
    WelsDecodeAccessUnitEnd(pCtx);
    iErr
}

/// Core bitstream decoding loop that demultiplexes Annex B NAL units and decodes them into an access unit.
/// Matches `int32_t WelsDecodeBs (PWelsDecoderContext pCtx, const uint8_t* kpBsBuf, const int32_t kiBsLen, uint8_t** ppDst, SBufferInfo* pDstBufInfo, SParserBsInfo* pDstBsInfo)` in `decoder.cpp:741`.
pub unsafe fn WelsDecodeBs(
    pCtx: PWelsDecoderContext,
    kpBsBuf: *const u8,
    kiBsLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
    _pDstBsInfo: *mut c_void,
) -> i32 {
    if pCtx.is_null() {
        return crate::api::codec_api::DECODING_STATE::dsInitialOptExpected as i32;
    }
    if !pDstInfo.is_null() {
        (*pDstInfo).iBufferStatus = 0;
    }

    if !kpBsBuf.is_null() && kiBsLen > 0 {
        (*pCtx).bEndOfStreamFlag = false;
        let input_slice = std::slice::from_raw_parts(kpBsBuf, kiBsLen as usize);
        let units = crate::split_annexb_units(input_slice);

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
            if payload_slice.is_empty() {
                continue;
            }

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
                    if !(*pCtx).pParam.is_null()
                        && (*(*pCtx).pParam).bParseOnly
                        && (*pCtx).sSavedData.grow_to((*pCtx).sRawData.len()).is_err()
                    {
                        (*pCtx).iErrorCode |= dsOutOfMemory;
                        return (*pCtx).iErrorCode;
                    }
                    (*pCtx).sRawData.rewind();
                }
            }
            let (payload_start, payload_len) = (*pCtx).sRawData.append_ebsp_stripped(payload_slice);

            let mut consumed_bytes = 0i32;
            let mut nal_header = crate::decoder::nalu::SNalUnitHeader::default();
            let p_payload = crate::decoder::nalu::ParseNalHeader(
                pCtx,
                &mut nal_header,
                payload_start,
                payload_len as i32,
                &mut consumed_bytes,
            );

            if let Some(nal_start) = p_payload {
                let nal_type = nal_header.eNalUnitType;
                if crate::decoder::nalu::IS_PARAM_SETS_NALS(nal_type) {
                    crate::decoder::nalu::ParseNonVclNal(
                        pCtx,
                        nal_start,
                        (payload_len as i32) - consumed_bytes,
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
        let bHasPending = match cur_au(pCtx) {
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
/// `dec_pic(pCtx)` — a cache with one stamp site (this one) and no way for a reader
/// to observe it diverging from its source, which is what W2b's S23 check
/// established. The readers derive; the parameter had nothing left to write.
pub unsafe fn InitDqLayerInfo(
    pDqLayer: PDqLayer,
    pLayerInfo: PLayerInfo,
    pNalUnit: PNalUnit,
) {
    if pDqLayer.is_null() || pLayerInfo.is_null() || pNalUnit.is_null() {
        return;
    }
    // F24's shape, third site (T5.E1). `pSh` was a `&mut` *inside* `pShExt`'s `&mut`,
    // so every `(*pShExt)` read below popped it and every later `(*pSh)` read was UB;
    // worse, the four escaping borrows in the `kuiQualityId` block store pointers into
    // the layer that outlive this function, and as `&mut` they died at the next use of
    // their parent. Raw derivations from `pNalUnit` carry the NAL unit's provenance and
    // nothing pops anything.
    let pNalHdrExt: PNalUnitHeaderExt = std::ptr::addr_of_mut!((*pNalUnit).sNalHeaderExt);
    let pShExt: PSliceHeaderExt =
        std::ptr::addr_of_mut!((*pNalUnit).sNalData.sVclNal.sSliceHeaderExt);
    let pSh: PSliceHeader = std::ptr::addr_of_mut!((*pShExt).sSliceHeader);
    let kuiQualityId = (*pNalHdrExt).uiQualityId;

    (*pDqLayer).sLayerInfo = *pLayerInfo;
    (*pDqLayer).iMbWidth = (*pSh).iMbWidth;
    (*pDqLayer).iMbHeight = (*pSh).iMbHeight;
    (*pDqLayer).iSliceIdcBackup = ((*pSh).iFirstMbInSlice << 7)
        | (((*pNalHdrExt).uiDependencyId as i32) << 4)
        | ((*pNalHdrExt).uiQualityId as i32);

    if !(*pLayerInfo).pPps.is_null() {
        (*pDqLayer).uiPpsId = (*(*pLayerInfo).pPps).iPpsId as u32;
    }
    (*pDqLayer).uiDisableInterLayerDeblockingFilterIdc = (*pShExt).uiDisableInterLayerDeblockingFilterIdc;
    (*pDqLayer).iInterLayerSliceAlphaC0Offset = (*pShExt).iInterLayerSliceAlphaC0Offset;
    (*pDqLayer).iInterLayerSliceBetaOffset = (*pShExt).iInterLayerSliceBetaOffset;
    (*pDqLayer).iSliceGroupChangeCycle = (*pSh).iSliceGroupChangeCycle;
    (*pDqLayer).bStoreRefBasePicFlag = (*pShExt).bStoreRefBasePicFlag;
    (*pDqLayer).bTCoeffLevelPredFlag = (*pShExt).bTCoeffLevelPredFlag;
    (*pDqLayer).bConstrainedIntraResamplingFlag = (*pShExt).bConstrainedIntraResamplingFlag;
    (*pDqLayer).uiRefLayerDqId = (*pShExt).uiRefLayerDqId;
    (*pDqLayer).uiRefLayerChromaPhaseXPlus1Flag = (*pShExt).uiRefLayerChromaPhaseXPlus1Flag;
    (*pDqLayer).uiRefLayerChromaPhaseYPlus1 = (*pShExt).uiRefLayerChromaPhaseYPlus1;
    (*pDqLayer).bUseWeightPredictionFlag = false;
    (*pDqLayer).bUseWeightedBiPredIdc = false;

    if kuiQualityId == BASE_QUALITY_ID {
        (*pDqLayer).pRefPicListReordering = std::ptr::addr_of_mut!((*pSh).pRefPicListReordering);
        (*pDqLayer).pRefPicMarking = std::ptr::addr_of_mut!((*pSh).sRefMarking);
        if !(*pSh).pPps.is_null() {
            let pPps = (*pSh).pPps as *mut SPps;
            (*pDqLayer).bUseWeightPredictionFlag = (*pPps).bWeightedPredFlag;
            (*pDqLayer).bUseWeightedBiPredIdc = (*pPps).uiWeightedBipredIdc != 0;
            if (*pPps).bWeightedPredFlag || (*pPps).uiWeightedBipredIdc != 0 {
                (*pDqLayer).pPredWeightTable = std::ptr::addr_of_mut!((*pSh).sPredWeightTable);
            }
        }
        (*pDqLayer).pRefPicBaseMarking = std::ptr::addr_of_mut!((*pShExt).sRefBasePicMarking);
    }
    (*pDqLayer).uiLayerDqId = (*pNalHdrExt).uiLayerDqId;
    (*pDqLayer).bUseRefBasePicFlag = (*pNalHdrExt).bUseRefBasePicFlag;
}

pub unsafe fn WelsDqLayerDecodeStart(
    pCtx: PWelsDecoderContext,
    pCurNal: PNalUnit,
    pSps: PSps,
    pPps: PPps,
) {
    if pCtx.is_null() || pCurNal.is_null() {
        return;
    }
    // F24's shape, fourth site — and the one that escapes furthest: `(*pCtx).pSliceHeader`
    // outlives this call by the whole slice decode, so as a `&mut`-derived pointer it
    // was dead the moment any other borrow of this NAL unit was taken (which
    // `InitDqLayerInfo` does, immediately after, at the same call site).
    let pSh: PSliceHeader =
        std::ptr::addr_of_mut!((*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader);
    (*pCtx).eSliceType = (*pSh).eSliceType;
    (*pCtx).pSliceHeader = pSh;
    (*pCtx).bUsedAsRef = false;
    (*pCtx).iFrameNum = (*pSh).iFrameNum;
    UpdateDecoderStatisticsForActiveParaset((*pCtx).pDecoderStatistics, pSps, pPps);
}

pub unsafe fn InitRefPicList(
    pCtx: PWelsDecoderContext,
    pCurDqLayer: PDqLayer,
    _kuiNRi: u8,
    iPoc: i32,
) -> i32 {
    let mut iRet = if (*pCtx).eSliceType == B_SLICE {
        let ret = WelsInitBSliceRefList(pCtx, pCurDqLayer, iPoc);
        CreateImplicitWeightTable(pCtx, pCurDqLayer);
        ret
    } else {
        WelsInitRefList(pCtx, pCurDqLayer, iPoc)
    };
    if (*pCtx).eSliceType != I_SLICE && (*pCtx).eSliceType != SI_SLICE {
        if !(*pCtx).pSps.is_null()
            && (*(*pCtx).pSps).uiProfileIdc != 66
            && !(*pCtx).pPps.is_null()
            && (*(*pCtx).pPps).bEntropyCodingModeFlag
        {
            iRet = WelsReorderRefList2(pCtx, pCurDqLayer);
        } else {
            iRet = WelsReorderRefList(pCtx, pCurDqLayer);
        }
    }
    iRet
}

pub unsafe fn DecodeCurrentAccessUnit(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    let (mut iIdx, iEndIdx) = match cur_au(pCtx) {
        None => return ERR_INFO_INVALID_PTR,
        Some(au) => (au.uiStartPos as i32, au.uiEndPos as i32),
    };
    let iThreadCount = GetThreadCount(pCtx);
    let mut iRet;
    let mut bAllRefComplete = true;

    let kuiTargetLayerDqId = GetTargetDqId((*pCtx).uiTargetDqId, (*pCtx).pParam);
    let kuiDependencyIdMax = (kuiTargetLayerDqId & 0x7F) >> 4;
    let mut iLastIdD: i16 = -1;
    let mut iLastIdQ: i16 = -1;
    (*pCtx).uiNalRefIdc = 0;
    let mut bFreshSliceAvailable;

    // Node pointers only, one derivation each. The slice loop below calls back into
    // the context on every iteration, so nothing about the access unit may be held
    // across it — but a node is its own allocation and outlives every retag (T5.O4).
    let mut pNalCur = match cur_au(pCtx) {
        Some(au) => au.nal(iIdx as usize),
        None => return ERR_INFO_INVALID_PTR,
    };
    (*pCtx).pNalCur = pNalCur;

    while iIdx <= iEndIdx {
        // **The access-unit bracket** (T5.R1): the one derivation of the layer on the
        // decode path, taken at the top of each iteration and threaded down. The stamp
        // that stood before this loop (`pCurDqLayer = pDqLayersList` under
        // `bInitialDqLayersMem || is_null()`) was a cache write of exactly this value —
        // the list is reallocated only by `InitialDqLayersContext`, which runs in
        // `InitConstructAccessUnit` *before* this function, never under it.
        let dq_cur = cur_dq_layer(pCtx);
        let mut pLayerInfo = SLayerInfo::default();
        let isNewFrame = (*pCtx).pDec.is_none();

        if (*pCtx).pDec.is_none() {
            // The prefetch hands back the slot it landed on, which is what this field
            // holds; `pic_slot(prefetched)` stood here and read the same value back
            // out of the picture's stamp. `None` is the pool being empty or fully
            // held, which is the arm below.
            (*pCtx).pDec = match pic_pool_mut(pCtx) {
                Some(pool) => pool.prefetch_free(),
                None => None,
            };
            if (*pCtx).pDec.is_none() {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return ERR_INFO_REF_COUNT_OVERFLOW;
            }
            (*dec_pic(pCtx)).bNewSeqBegin = (*pCtx).bNewSeqBegin;
        }

        if !pNalCur.is_null() {
            (*dec_pic(pCtx)).uiTimeStamp = (*pNalCur).uiTimeStamp;
        }
        (*dec_pic(pCtx)).uiDecodingTimeStamp = (*pCtx).uiDecodingTimeStamp as u32;


        if (*pCtx).iTotalNumMbRec == 0 {
            // Picture starts to decode: reset per-picture MB state, matching
            // `DecodeCurrentAccessUnit` in `decoder_core.cpp`.
            let iMbCacheNum =
                ((((*pCtx).iPicWidthReq + 15) >> 4) * (((*pCtx).iPicHeightReq + 15) >> 4)) as usize;
            // This re-derived from the list mid-loop where everything around it read
            // the cache; both were this iteration's `dq_cur`, and now it says so.
            let pDq = dq_cur;
            if !pDq.is_null() {
                // `memset(pSliceIdc, 0xff, numMb * sizeof(int32_t))` — 0xff bytes in
                // an `i32` is -1. `iMbCacheNum` is computed from `iPicWidthReq`, which
                // `InitialDqLayersContext` sets to the same `kiMaxWidth` the grid's
                // dimensions come from, so the bound is an identity; spelling it as a
                // slice makes it a checked one (P13), where the C had no bound at all.
                (*pDq).grid.slice_idc.as_mut_slice()[..iMbCacheNum].fill(-1);
            }
            if !(*pCtx).pSps.is_null() {
                let iMbNum = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as usize;
                if !dq_cur.is_null() {
                    (*dq_cur).grid.mb_correctly_decoded_flag.as_mut_slice()
                        [..iMbNum]
                        .fill(false);
                    // The C's `memset(.., 0, iMbWidth * iMbHeight)` over the
                    // **SPS's** dimensions, which are the current sequence's and can
                    // be smaller than the grid's negotiated maximum (T5.E2). As a
                    // slice the bound is checked; as a `write_bytes` it was not.
                    (*dq_cur).grid.mb_ref_concealed_flag.as_mut_slice()[..iMbNum]
                        .fill(false);
                }
                (*dec_pic(pCtx)).iMbNum = iMbNum as i32;
            }
            (*dec_pic(pCtx)).pRefPic[LIST_0] = [None; MAX_DPB_COUNT];
            (*dec_pic(pCtx)).pRefPic[LIST_1] = [None; MAX_DPB_COUNT];
            (*dec_pic(pCtx)).iMbEcedNum = 0;
            (*dec_pic(pCtx)).iMbEcedPropNum = 0;
        }

        (*pCtx).bRPLRError = false;
        if (*pCtx).pDec.is_some() {
            GetI4LumaIChromaAddrTable(
                // F38/S29: `as_mut_ptr()` takes a `&mut [i32; 24]` of a field of a
                // raw-reached struct first; `addr_of_mut!` derives from `pCtx`.
                std::ptr::addr_of_mut!((*pCtx).iDecBlockOffsetArray) as *mut i32,
                (*dec_pic(pCtx)).linesize(0),
                (*dec_pic(pCtx)).linesize(1),
            );
        }

        if !pNalCur.is_null() && (*pNalCur).sNalHeaderExt.uiLayerDqId > kuiTargetLayerDqId {
            break;
        }

        while iIdx <= iEndIdx {
            if pNalCur.is_null() || dq_cur.is_null() {
                break;
            }
            let iCurrIdQ = (*pNalCur).sNalHeaderExt.uiQualityId as i16;
            let iCurrIdD = (*pNalCur).sNalHeaderExt.uiDependencyId as i16;
            // F24's shape, second site — this is the pair Miri reported once the
            // `ParseSliceHeaderSyntaxs` fix let it get this far. Byte-identical
            // diagnosis: `[0x28..0xf00]` created here, invalidated by the retag at
            // `[0x28..0x1350]` on the next line, caught at the `iFrameNum` read below.
            let pSh: PSliceHeader =
                std::ptr::addr_of_mut!((*pNalCur).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader);
            let pShExt: PSliceHeaderExt =
                std::ptr::addr_of_mut!((*pNalCur).sNalData.sVclNal.sSliceHeaderExt);
            (*pCtx).bRPLRError = false;
            let bReconstructSlice = CheckSliceNeedReconstruct((*pNalCur).sNalHeaderExt.uiLayerDqId, kuiTargetLayerDqId);

            pLayerInfo.sNalHeaderExt = (*pNalCur).sNalHeaderExt;
            if (*pCtx).pDec.is_some() {
                (*dec_pic(pCtx)).iFrameNum = (*pSh).iFrameNum;
                (*dec_pic(pCtx)).iFramePoc = (*pSh).iPicOrderCntLsb;
                (*dec_pic(pCtx)).bIdrFlag = (*pNalCur).sNalHeaderExt.bIdrFlag;
                (*dec_pic(pCtx)).eSliceType = (*pSh).eSliceType;
            }

            pLayerInfo.sSliceInLayer.sSliceHeaderExt = *pShExt;
            pLayerInfo.sSliceInLayer.bSliceHeaderExtFlag = (*pNalCur).sNalData.sVclNal.bSliceHeaderExtFlag;
            pLayerInfo.sSliceInLayer.eSliceType = (*pSh).eSliceType as u8;

            pLayerInfo.sSliceInLayer.iLastMbQp = (*pSh).iSliceQp;
            // **T5.M3 — where `dq_cur->pBitStringAux` was written.** The layer mirrored
            // `&pNalCur->sNalData.sVclNal.sSliceBitsRead` here and every reader in
            // `decode_slice.rs` and `parse_mb_syn_cabac.rs` went through the mirror;
            // `bit_stream::slice_bit_reader` derives it from `pNalCur` instead, so the
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
            (*pCtx).pNalCur = pNalCur;

            (*pCtx).uiNalRefIdc = (*pNalCur).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc;
            let iPpsId = (*pSh).iPpsId;
            pLayerInfo.pPps = (*pSh).pPps as *mut SPps;
            pLayerInfo.pSps = (*pSh).pSps as *mut SSps;
            pLayerInfo.pSubsetSps = (*pShExt).pSubsetSps as *mut SSubsetSps;


            bFreshSliceAvailable = iCurrIdD != iLastIdD || iCurrIdQ != iLastIdQ;
            WelsDqLayerDecodeStart(pCtx, pNalCur, pLayerInfo.pSps, pLayerInfo.pPps);

            if iLastIdD < 0 || iLastIdD == iCurrIdD {
                InitDqLayerInfo(dq_cur, &mut pLayerInfo, pNalCur);

                if iCurrIdD == (kuiDependencyIdMax as i16) && iCurrIdQ == (BASE_QUALITY_ID as i16) && isNewFrame {
                    iRet = InitRefPicList(pCtx, dq_cur, (*pCtx).uiNalRefIdc, (*pSh).iPicOrderCntLsb);
                    if iRet != ERR_NONE {
                        (*pCtx).bRPLRError = true;
                        bAllRefComplete = false;
                        HandleReferenceLost(pCtx, pNalCur);
                        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                            if (*pCtx).iTotalNumMbRec == 0 {
                                (*pCtx).pDec = None;
                            }
                            return iRet;
                        }
                    }
                }

                if (*pSh).eSliceType == B_SLICE && (*pSh).iDirectSpatialMvPredFlag == 0 {
                    ComputeColocatedTemporalScaling(pCtx, dq_cur);
                }

                if iThreadCount > 1 {
                    iRet = WelsDecodeAndConstructSlice(pCtx, dq_cur);
                } else {
                    iRet = WelsDecodeSlice(pCtx, dq_cur, bFreshSliceAvailable, pNalCur);
                }

                if iRet != ERR_NONE {
                    bAllRefComplete = false;
                    HandleReferenceLostL0(pCtx, pNalCur);
                    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                        if (*pCtx).iTotalNumMbRec == 0 {
                            (*pCtx).pDec = None;
                        }
                        return iRet;
                    }
                }

                if iThreadCount <= 1 && bReconstructSlice {
                    iRet = WelsDecodeConstructSlice(pCtx, dq_cur, pNalCur);
                    if iRet != ERR_NONE {
                        if (*pCtx).pDec.is_some() {
                            (*dec_pic(pCtx)).bIsComplete = false;
                        }
                        return iRet;
                    }
                }

                if bAllRefComplete && (*pCtx).eSliceType != I_SLICE {
                    if iThreadCount <= 1 {
                        if (*pCtx).sRefPic.uiRefCount[LIST_0] > 0 {
                            bAllRefComplete =
                                bAllRefComplete && CheckRefPicturesComplete(pCtx, dq_cur);
                        } else {
                            bAllRefComplete = false;
                        }
                    }
                }
            }

            iLastIdD = iCurrIdD;
            iLastIdQ = iCurrIdQ;

            iIdx += 1;
            pNalCur = match cur_au(pCtx) {
                Some(au) if iIdx <= iEndIdx => au.nal(iIdx as usize),
                _ => std::ptr::null_mut(),
            };

            if pNalCur.is_null()
                || iLastIdD != ((*pNalCur).sNalHeaderExt.uiDependencyId as i16)
                || iLastIdQ != ((*pNalCur).sNalHeaderExt.uiQualityId as i16)
            {
                break;
            }
        }

        // The C++ code runs the completion/frame-construction block below even
        // when all NAL units are consumed (pNalCur == NULL); only a missing DQ
        // layer aborts here.
        if dq_cur.is_null() {
            break;
        }

        if (*pCtx).pDec.is_some() {
            (*dec_pic(pCtx)).bIsComplete = bAllRefComplete;
            if !(*dec_pic(pCtx)).bIsComplete {
                (*pCtx).iErrorCode |= dsDataErrorConcealed;
            }
        }

        if !dq_cur.is_null() && (*dq_cur).uiLayerDqId == kuiTargetLayerDqId {
            if !(*pCtx).bInstantDecFlag {
                if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly {
                    if NeedErrorCon(pCtx, dq_cur) && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                        ImplementErrorCon(pCtx, dq_cur);
                        if !(*pCtx).pSps.is_null() {
                            (*pCtx).iTotalNumMbRec = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;
                            if (*pCtx).pDec.is_some() {
                                (*dec_pic(pCtx)).iSpsId = (*(*pCtx).pSps).iSpsId;
                            }
                        }
                        if !(*pCtx).pPps.is_null() && (*pCtx).pDec.is_some() {
                            (*dec_pic(pCtx)).iPpsId = (*(*pCtx).pPps).iPpsId;
                        }
                    }
                }
            }

            iRet = DecodeFrameConstruction(pCtx, dq_cur, ppDst, pDstInfo);
            if iRet != ERR_NONE {
                return iRet;
            }

            if !(*pCtx).pLastDecPicInfo.is_null() {
                (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb = (*pCtx).pDec;
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
                    let pDec = dec_pic(pCtx);
                    if !pDec.is_null() {
                        for listIdx in LIST_0..LIST_A {
                            let mut i = 0usize;
                            while i < MAX_DPB_COUNT
                                && (*pCtx).sRefPic.pRefList[listIdx][i].is_some()
                            {
                                (*pDec).pRefPic[listIdx][i] =
                                    (*pCtx).sRefPic.pRefList[listIdx][i];
                                i += 1;
                            }
                        }
                    }
                    iRet = WelsMarkAsRef(pCtx, dq_cur);
                    if iRet != ERR_NONE {
                        if iRet == ERR_INFO_DUPLICATE_FRAME_NUM {
                            (*pCtx).iErrorCode |= dsBitstreamError;
                        }
                        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                            (*pCtx).pDec = None;
                            return iRet;
                        }
                    }
                    if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly && (*pCtx).pDec.is_some() {
                        let pDec = dec_pic(pCtx);
                        crate::common::expand_pic::ExpandReferencingPicture(
                            &[(*pDec).data_ptr(0), (*pDec).data_ptr(1), (*pDec).data_ptr(2)],
                            (*pDec).iWidthInPixel,
                            (*pDec).iHeightInPixel,
                            &[(*pDec).linesize(0), (*pDec).linesize(1), (*pDec).linesize(2)],
                        );
                    }
                }
            }
            (*pCtx).pDec = None;
        }

        if pNalCur.is_null() {
            break;
        }
    }
    ERR_NONE
}

pub unsafe fn CheckAndFinishLastPic(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> bool {
    if pCtx.is_null() || (*pCtx).access_unit.is_none() {
        return false;
    }
    let mut bAuBoundaryFlag = false;

    if IS_VCL_NAL((*pCtx).sCurNalHead.eNalUnitType, 1) {
        let pCurNal = match cur_au(pCtx) {
            Some(au) => au.nal(au.uiEndPos as usize),
            None => return false,
        };
        if !pCurNal.is_null() && !(*pCtx).pLastDecPicInfo.is_null() {
            bAuBoundaryFlag = (*pCtx).iTotalNumMbRec != 0
                && CheckAccessUnitBoundaryExt(
                    std::ptr::addr_of_mut!((*(*pCtx).pLastDecPicInfo).sLastNalHdrExt),
                    std::ptr::addr_of_mut!((*pCurNal).sNalHeaderExt),
                    std::ptr::addr_of_mut!((*(*pCtx).pLastDecPicInfo).sLastSliceHeader),
                    std::ptr::addr_of_mut!(
                        (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader
                    ),
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

    // **The error-concealment bracket** (T5.R1): this runs *between* access units —
    // `ConstructAccessUnit` above may have just returned — so it takes its own
    // derivation of the layer rather than inheriting one. It is the value the deleted
    // cache field held at this point, and the list is what the cache was stamped from.
    let dq_cur = cur_dq_layer(pCtx);

    if bAuBoundaryFlag && (*pCtx).iTotalNumMbRec != 0 && NeedErrorCon(pCtx, dq_cur) {
        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
            ImplementErrorCon(pCtx, dq_cur);
            if !(*pCtx).pSps.is_null() {
                (*pCtx).iTotalNumMbRec = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;
                if (*pCtx).pDec.is_some() {
                    (*dec_pic(pCtx)).iSpsId = (*(*pCtx).pSps).iSpsId;
                }
            }
            if !(*pCtx).pPps.is_null() && (*pCtx).pDec.is_some() {
                (*dec_pic(pCtx)).iPpsId = (*(*pCtx).pPps).iPpsId;
            }
            DecodeFrameConstruction(pCtx, dq_cur, ppDst, pDstInfo);
            if !(*pCtx).pLastDecPicInfo.is_null() {
                (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb = (*pCtx).pDec;
                if (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt.sNalUnitHeader.uiNalRefIdc > 0 {
                    if MarkECFrameAsRef(pCtx) == ERR_INFO_INVALID_PTR {
                        (*pCtx).iErrorCode |= dsRefListNullPtrs;
                        return false;
                    }
                }
            }
        } else if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
            if !(*pCtx).pParserBsInfo.is_null() {
                (*(*pCtx).pParserBsInfo).iNalNum = 0;
            }
            (*pCtx).bFrameFinish = true;
        } else {
            if DecodeFrameConstruction(pCtx, dq_cur, ppDst, pDstInfo) != ERR_NONE {
                if !(*pCtx).pLastDecPicInfo.is_null()
                    && (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt.sNalUnitHeader.uiNalRefIdc > 0
                    && (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt.uiTemporalId == 0
                {
                    (*pCtx).iErrorCode |= dsNoParamSets;
                } else {
                    (*pCtx).iErrorCode |= dsBitstreamError;
                }
                (*pCtx).pDec = None;
                return false;
            }
        }
        (*pCtx).pDec = None;
        // Re-derived: `ConstructAccessUnit` ran above, and it decodes.
        let pStartNal = match cur_au(pCtx) {
            Some(au) if (au.uiStartPos as usize) < au.count() as usize => {
                au.nal(au.uiStartPos as usize)
            }
            _ => std::ptr::null_mut(),
        };
        if !pStartNal.is_null() {
            if (*pStartNal).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc > 0
                && !(*pCtx).pLastDecPicInfo.is_null()
            {
                (*(*pCtx).pLastDecPicInfo).iPrevFrameNum =
                    (*(*pCtx).pLastDecPicInfo).sLastSliceHeader.iFrameNum;
            }
        }
        if !(*pCtx).pLastDecPicInfo.is_null() && (*(*pCtx).pLastDecPicInfo).bLastHasMmco5 {
            (*(*pCtx).pLastDecPicInfo).iPrevFrameNum = 0;
        }
    }
    true
}

pub unsafe fn CheckRefPicturesComplete(pCtx: PWelsDecoderContext, pCurDqLayer: PDqLayer) -> bool {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return true;
    }
    // **A bracket** (T5.Q2): this scan reads the current picture's macroblock types
    // and reference indices *while* resolving the reference-list entries those
    // indices name, and a malformed stream can put the current picture in that list
    // (F42). One borrow, split — the same shape the slice brackets take, over a
    // whole-slice operation rather than a whole slice.
    let (pDec, pRefs) = cur_and_refs(pCtx);
    if pDec.is_null() || (*pDec).pMbType.as_slice().is_empty() {
        return true;
    }
    let mut bAllRefComplete = true;
    let mut iRealMbIdx = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    let totalMb = (*pCurDqLayer).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice;

    for iMbIdx in 0..totalMb {
        let mbType = *(*pDec).pMbType.get(iRealMbIdx as usize);
        match mbType {
            MB_TYPE_SKIP | MB_TYPE_16x16 => {
                let refIdx = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                if refIdx < MAX_REF_PIC_COUNT {
                    let pRef = pRefs.get(ref_id(pCtx, LIST_0, refIdx));
                    if !pRef.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef).bIsComplete;
                    }
                }
            }
            MB_TYPE_16x8 => {
                let refIdx0 = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                let refIdx1 = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[8] as usize;
                if refIdx0 < MAX_REF_PIC_COUNT {
                    let pRef0 = pRefs.get(ref_id(pCtx, LIST_0, refIdx0));
                    if !pRef0.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef0).bIsComplete;
                    }
                }
                if refIdx1 < MAX_REF_PIC_COUNT {
                    let pRef1 = pRefs.get(ref_id(pCtx, LIST_0, refIdx1));
                    if !pRef1.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef1).bIsComplete;
                    }
                }
            }
            MB_TYPE_8x16 => {
                let refIdx0 = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[0] as usize;
                let refIdx1 = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[2] as usize;
                if refIdx0 < MAX_REF_PIC_COUNT {
                    let pRef0 = pRefs.get(ref_id(pCtx, LIST_0, refIdx0));
                    if !pRef0.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef0).bIsComplete;
                    }
                }
                if refIdx1 < MAX_REF_PIC_COUNT {
                    let pRef1 = pRefs.get(ref_id(pCtx, LIST_0, refIdx1));
                    if !pRef1.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef1).bIsComplete;
                    }
                }
            }
            MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
                let indices = [0, 2, 8, 10];
                for &sub in &indices {
                    let refIdx = (*(*pDec).pRefIndex[0].get(iRealMbIdx as usize))[sub] as usize;
                    if refIdx < MAX_REF_PIC_COUNT {
                        let pRef = pRefs.get(ref_id(pCtx, LIST_0, refIdx));
                        if !pRef.is_null() {
                            bAllRefComplete = bAllRefComplete && (*pRef).bIsComplete;
                        }
                    }
                }
            }
            _ => {}
        }
        if !bAllRefComplete {
            break;
        }
        iRealMbIdx = if !(*pCtx).pPps.is_null() && (*(*pCtx).pPps).uiNumSliceGroups > 1 {
            FmoNextMb((*pCtx).pFmo as *mut SFmo, iRealMbIdx)
        } else {
            (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice + iMbIdx + 1
        };
        if iRealMbIdx == -1 {
            return false;
        }
    }
    bAllRefComplete
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

    /// **S28, forwards and backwards.** A pointer taken at the last macroblock
    /// must reach every earlier one — that is what `GetPNzc`'s callers do when
    /// they read the left neighbour's counts, and what `pCbfDc`'s consumers do
    /// when they keep the base and index it later.
    ///
    /// Under Miri this fails against `as_mut_slice()[mb_xy..].as_mut_ptr()` at
    /// the first backwards write and passes against the `wrapping_add` form.
    /// Under the ordinary test runner both pass, which is the whole reason the
    /// rule needed a finding to discover.
    #[test]
    fn mb_grid_ptr_reaches_the_whole_array_in_both_directions() {
        let dims = MbDims::new(11, 9);
        let n = dims.count();
        let mut g = MbGrid::new(dims);

        // From the last macroblock, walk back over every element and forward to
        // the end, writing through the raw pointer the shim handed out.
        let last = n - 1;
        let p = mb_grid_ptr(&mut g.luma_qp, last);
        unsafe {
            for back in 0..=last {
                *p.sub(back) = (back % 128) as i8;
            }
            assert_eq!(*p, 0);
            assert_eq!(*p.sub(last), ((n - 1) % 128) as i8);
        }
        // and the safe view agrees with what the raw writes did
        assert_eq!(*g.luma_qp.get(0), ((n - 1) % 128) as i8);
        assert_eq!(*g.luma_qp.get(last), 0);

        // From macroblock 0 forward.
        let p0 = mb_grid_ptr(&mut g.luma_qp, 0);
        unsafe {
            for fwd in 0..n {
                *p0.add(fwd) = 7;
            }
        }
        assert!(g.luma_qp.as_slice().iter().all(|&q| q == 7));
    }

    /// The same reach on a **composite** element, cast to the element type the
    /// kernels take: a pointer taken at one macroblock and walked over the whole
    /// flattened array is what `mb_grid_ptr` promises, and this is where that
    /// promise is checked.
    ///
    /// **The family it was written for turned out not to need it** (T5.L5):
    /// `pScaledTCoeff`'s consumers spell `.add(iMbXy) as *mut i16` and then index
    /// 0..384, which is the record's *own* length — `pScoeffLevel.add(256 + (i << 6))`
    /// with `i < 2` is the furthest any of them reaches — so that family derives
    /// per-record now and this test outlives it as the composite case of the
    /// accessor's contract. The scouted claim was that those consumers needed the
    /// flattened extent; walking them at conversion time said otherwise (S24).
    #[test]
    fn mb_grid_ptr_reaches_a_composite_arrays_flattened_extent() {
        let dims = MbDims::new(3, 2);
        let n = dims.count();
        let mut g = MbGrid::new(dims);

        let p = mb_grid_ptr(&mut g.scaled_tcoeff, n - 1) as *mut i16;
        unsafe {
            // this macroblock's own 384 coefficients
            for i in 0..384 {
                *p.add(i) = (i as i16) & 0x7f;
            }
            // and backwards into the previous macroblock's, which is where a
            // narrowed provenance dies
            for i in 1..=384 {
                *p.sub(i) = -1;
            }
        }
        assert_eq!(g.scaled_tcoeff.get(n - 1)[383], 383 & 0x7f);
        assert!(g.scaled_tcoeff.get(n - 2).iter().all(|&c| c == -1));

        // `[i8; 24]` likewise: `GetPNzc` hands out `*mut i8` at one macroblock and
        // the deblocking filter reads the neighbour's 24 through it.
        let q = mb_grid_ptr(&mut g.nzc, 1) as *mut i8;
        unsafe {
            for i in 0..24 {
                *q.add(i) = 2;
            }
            for i in 1..=24 {
                *q.sub(i) = 3;
            }
        }
        assert_eq!(g.nzc.get(1), &[2i8; 24]);
        assert_eq!(g.nzc.get(0), &[3i8; 24]);
    }

    /// The reach `pRefIndex`'s one surviving raw consumer actually takes (T5.J3).
    ///
    /// `DeblockingBsMarginalMBAvcbase` keeps the array **base** and then indexes it
    /// by macroblock address for the current macroblock *and* its neighbour —
    /// `(*pRefIdxArr.add(iMbXy))[k]`, `(*pRefIdxArr.add(iNeighMb))[k]` — so the
    /// legal reach of the pointer taken at 0 is every macroblock in the array, and a
    /// derivation narrowed to `[0..]`-of-something-shorter would die at the first
    /// neighbour. It also pins the element type: this bridge stays `*mut [i8; 16]`
    /// rather than flattening, because the consumer indexes inside the record with a
    /// scan-order index of its own.
    #[test]
    fn mb_grid_ptr_reaches_every_macroblock_of_ref_index_from_the_base() {
        let dims = MbDims::new(4, 3);
        let n = dims.count();
        let mut g = MbGrid::new(dims);

        let base = mb_grid_ptr(&mut g.ref_index[LIST_1], 0);
        unsafe {
            for mb in 0..n {
                for k in 0..16 {
                    (*base.add(mb))[k] = ((mb * 16 + k) % 128) as i8;
                }
            }
            // and read back across a neighbour pair, the consumer's own shape
            assert_eq!((*base.add(n - 1))[15], (((n - 1) * 16 + 15) % 128) as i8);
            assert_eq!((*base.add(n - 2))[0], (((n - 2) * 16) % 128) as i8);
        }
        assert_eq!(g.ref_index[LIST_1].get(0)[0], 0);
        assert_eq!(g.ref_index[LIST_1].get(n - 1)[15], (((n - 1) * 16 + 15) % 128) as i8);
        // LIST_0 is untouched: two arrays of one grid are two values, which the raw
        // pair of pointers this replaces could only promise by inspection.
        assert!(g.ref_index[LIST_0].as_slice().iter().all(|mb| mb.iter().all(|&r| r == 0)));
    }

    /// The reach `pMv`'s one surviving raw consumer actually takes (T5.K1).
    ///
    /// `MB_BS_MV` is handed the array **base** and reads
    /// `(*iMotionVector.add(iMbXy))[i]` beside `(*iMotionVector.add(iMbBn))[j]`, so —
    /// exactly as for `pRefIndex` above — the legal reach of the pointer taken at 0
    /// is every macroblock, and the element type must stay `[[i16; 2]; 16]` rather
    /// than flattening, because the consumer indexes inside the record.
    ///
    /// It also pins the **alignment** this family lands on (F35): the `Vec` behind
    /// `MbArray<[[i16; 2]; 16]>` is align **2** where `WelsMallocz` returned 16, so
    /// every 4-byte access into a record through this pointer is unaligned and must
    /// be spelled that way. The `ST32`/`LD32` round trip below is `mv_pred.rs`'s own
    /// spelling and is what Miri checks here.
    #[test]
    fn mb_grid_ptr_reaches_every_macroblock_of_mv_from_the_base() {
        use crate::decoder::mv_pred::{LD32, ST32};

        let dims = MbDims::new(4, 3);
        let n = dims.count();
        let mut g = MbGrid::new(dims);

        let base = mb_grid_ptr(&mut g.mv[LIST_1], 0);
        unsafe {
            for mb in 0..n {
                let row = (*base.add(mb)).as_mut_ptr();
                for k in 0..16 {
                    // the 4-byte-at-a-time write the MV cache helpers do, at the
                    // alignment the flip actually hands them
                    ST32(row.add(k) as *mut i16, (mb as u32) << 16 | k as u32);
                }
            }
            // read back across a neighbour pair, the consumer's own shape
            assert_eq!(LD32((*base.add(n - 1))[15].as_ptr()), ((n as u32 - 1) << 16) | 15);
            assert_eq!(LD32((*base.add(n - 2))[0].as_ptr()), (n as u32 - 2) << 16);
        }
        assert_eq!(g.mv[LIST_1].get(0)[0], [0, 0]);
        // LIST_0 is untouched: two arrays of one grid are two values.
        assert!(g.mv[LIST_0].as_slice().iter().all(|mb| mb.iter().all(|v| v == &[0, 0])));
    }

    /// The reach `pMbType`'s one surviving raw consumer actually takes (T5.K2).
    ///
    /// `GetMbType` hands its caller the array **base**, and every caller then
    /// indexes it at *neighbour* addresses — `*pMbType.add(iLeftXy)`,
    /// `.add(iTopXy)`, `.add(iLeftTopXy)`, `.add(iRightTopXy)` — around a current
    /// macroblock that may be anywhere in the picture. So the pointer taken at 0
    /// must reach every macroblock in both directions, and the element stays a
    /// plain `u32`: this is the one family of the twenty-two whose record *is* a
    /// scalar, so there is nothing inside it to index.
    #[test]
    fn mb_grid_ptr_reaches_every_macroblock_of_mb_type_from_the_base() {
        let dims = MbDims::new(4, 3);
        let n = dims.count();
        let mut g = MbGrid::new(dims);

        let base = mb_grid_ptr(&mut g.mb_type, 0);
        unsafe {
            for mb in 0..n {
                *base.add(mb) = 0xDEAD_0000 | mb as u32;
            }
            // the caller's own shape: a current macroblock and its four neighbours,
            // read backwards from the interior through the pointer taken at 0
            let cur = dims.mb_xy(2, 2);
            assert_eq!(*base.add(cur), 0xDEAD_0000 | cur as u32);
            for nb in [dims.left(cur), dims.top(cur), dims.top_left(cur), dims.top_right(cur)] {
                let nb = nb.expect("interior macroblock has all four neighbours");
                assert_eq!(*base.add(nb), 0xDEAD_0000 | nb as u32);
            }
        }
        assert_eq!(*g.mb_type.get(n - 1), 0xDEAD_0000 | (n as u32 - 1));
    }

    /// One past the end is a pointer you may form and not one you may read —
    /// exactly what `base + numMb` meant in the C. Past *that* is the F32 shape
    /// and it is a panic now.
    #[test]
    fn mb_grid_ptr_allows_one_past_the_end() {
        let dims = MbDims::new(2, 2);
        let mut g = MbGrid::new(dims);
        let end = mb_grid_ptr(&mut g.cbp, dims.count());
        let base = mb_grid_ptr(&mut g.cbp, 0);
        assert_eq!(end as usize - base as usize, dims.count());
    }

    #[test]
    #[should_panic(expected = "outside a per-macroblock array of 4")]
    fn mb_grid_ptr_rejects_an_index_past_one_past_the_end() {
        let mut g = MbGrid::new(MbDims::new(2, 2));
        mb_grid_ptr(&mut g.cbp, 5);
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

    /// The grid is sized from the **allocation's** dimensions, and the layer's
    /// `iMbWidth`/`iMbHeight` are the current slice's — T5.E2's correction, now
    /// structural rather than a comment on a `numMb` expression.
    #[test]
    fn the_grid_outlives_a_narrower_slice() {
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

    #[test]
    fn test_update_dec_stat_null() {
        unsafe {
            UpdateDecStatNoFreezingInfo(std::ptr::null_mut(), std::ptr::null_mut());
            UpdateDecStat(std::ptr::null_mut(), std::ptr::null_mut(), true);
        }
    }

    #[test]
    fn test_update_dec_stat_freezing() {
        unsafe {
            let mut stat = SDecoderStatistics::default();
            UpdateDecStatFreezingInfo(true, &mut stat);
            assert_eq!(stat.uiFreezingIDRNum, 1);
            assert_eq!(stat.uiFreezingNonIDRNum, 0);
            UpdateDecStatFreezingInfo(false, &mut stat);
            assert_eq!(stat.uiFreezingNonIDRNum, 1);
        }
    }

    #[test]
    fn test_reset_dec_stat_nums() {
        unsafe {
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

    #[test]
    fn test_inline_delegation_stubs_null() {
        unsafe {
            assert_eq!(
                WelsTargetSliceConstruction(std::ptr::null_mut(), std::ptr::null_mut()),
                ERR_NONE
            );
            assert_eq!(
                WelsDecodeSlice(std::ptr::null_mut(), std::ptr::null_mut(), true, std::ptr::null_mut()),
                ERR_NONE
            );
            assert_eq!(
                WelsDecodeAndConstructSlice(std::ptr::null_mut(), std::ptr::null_mut()),
                ERR_NONE
            );
            assert_ne!(WelsInitRefList(std::ptr::null_mut(), std::ptr::null_mut(), 0), ERR_NONE);
            assert_ne!(
                WelsInitBSliceRefList(std::ptr::null_mut(), std::ptr::null_mut(), 0),
                ERR_NONE
            );
            assert_ne!(WelsReorderRefList(std::ptr::null_mut(), std::ptr::null_mut()), ERR_NONE);
            assert_ne!(WelsReorderRefList2(std::ptr::null_mut(), std::ptr::null_mut()), ERR_NONE);
            assert_ne!(WelsMarkAsRef(std::ptr::null_mut(), std::ptr::null_mut()), ERR_NONE);
            WelsResetRefPic(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_missing_functions_highlight_translations() {
        unsafe {
            assert_eq!(GetCPUCount(), 1);
            let mut cpu_cores = 0;
            assert_eq!(WelsCPUFeatureDetect(&mut cpu_cores), 0);
            assert_eq!(cpu_cores, 1);
            assert_eq!(
                WelsOpenDecoder(std::ptr::null_mut(), std::ptr::null_mut()),
                ERR_INFO_INVALID_PTR
            );
            WelsEndDecoder(std::ptr::null_mut());
            assert_eq!(
                WelsDecodeBs(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ),
                crate::api::codec_api::DECODING_STATE::dsInitialOptExpected as i32
            );
        }
    }

    #[test]
    fn test_decoder_open_end_and_init_static_memory_state_flags() {
        unsafe {
            let mut ctx = SWelsDecoderContext::new_boxed();
            assert_eq!(WelsOpenDecoder(&mut *ctx as *mut _, std::ptr::null_mut()), ERR_NONE);
            assert!(ctx.bParamSetsLostFlag);
            assert!(ctx.bNewSeqBegin);
            assert!(ctx.bPrintFrameErrorTraceFlag);
            assert_eq!(ctx.iIgnoredErrorInfoPacketCount, 0);
            assert!(ctx.bFrameFinish);
            assert_eq!(ctx.iSeqNum, 0);

            WelsEndDecoder(&mut *ctx as *mut _);
            assert!(!ctx.bParamSetsLostFlag);
            assert!(!ctx.bNewSeqBegin);
            assert!(!ctx.bPrintFrameErrorTraceFlag);
            assert!(!ctx.bFrameFinish);
        }
    }

    #[test]
    fn test_chapter_7_frame_finalization() {
        unsafe {
            assert_eq!(
                CheckAndFinishLastPic(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()),
                false
            );
            assert_eq!(
                DecodeFrameConstruction(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                ERR_INFO_INVALID_PTR
            );
            WelsDecodeAccessUnitEnd(std::ptr::null_mut());

            let mut stat = SDecoderStatistics::default();
            let mut sps = SSps::default();
            sps.iSpsId = 3;
            sps.uiProfileIdc = 66;
            sps.uiLevelIdc = 31;
            let mut pps = SPps::default();
            pps.iPpsId = 5;

            UpdateDecoderStatisticsForActiveParaset(
                &mut stat,
                &mut sps as *mut SSps as PSps,
                &mut pps as *mut SPps as PPps,
            );
            assert_eq!(stat.iCurrentActiveSpsId, 3);
            assert_eq!(stat.iCurrentActivePpsId, 5);
            assert_eq!(stat.uiProfile, 66);
            assert_eq!(stat.uiLevel, 31);
        }
    }

    #[test]
    fn test_parse_slice_header_syntaxs_null() {
        unsafe {
            let mut cursor = crate::safe::bits::BsCursor::default();
            let res = ParseSliceHeaderSyntaxs(std::ptr::null_mut(), &[], &mut cursor, false);
            assert_eq!(res, ERR_INFO_INVALID_PTR);
        }
    }
}
