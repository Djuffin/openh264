/*
 * \copy
 *     Copyright (c)  2009-2013, Cisco Systems
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
 * \file    md.rs
 * \brief   Macroblock Mode Decision & Sub-Pixel Refinement Engine
 */

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::encoder_context::SMVComponentUnit;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::svc_motion_estimate::SWelsME;

// Sub-pixel refinement buffer geometry constants
pub const ME_REFINE_BUF_STRIDE: i32 = 32;
pub const ME_REFINE_BUF_WIDTH_BLK4: i32 = 8;
pub const ME_REFINE_BUF_WIDTH_BLK8: i32 = 16;
pub const ME_REFINE_BUF_STRIDE_BLK4: i32 = 160;
pub const ME_REFINE_BUF_STRIDE_BLK8: i32 = 320;

// Half-pixel search offsets
pub const REFINE_ME_NO_BEST_HALF_PIXEL: i32 = 0; // ( 0,  0)
pub const REFINE_ME_HALF_PIXEL_TOP: i32 = 1;     // ( 0, -2) in 1/4-pel units
pub const REFINE_ME_HALF_PIXEL_BOTTOM: i32 = 2;  // ( 0,  2) in 1/4-pel units
pub const REFINE_ME_HALF_PIXEL_LEFT: i32 = 3;    // (-2,  0) in 1/4-pel units
pub const REFINE_ME_HALF_PIXEL_RIGHT: i32 = 4;   // ( 2,  0) in 1/4-pel units

// Quarter-pixel search offsets
pub const ME_NO_BEST_QUAR_PIXEL: i32 = 1; // ( 0,  0) or best half pixel
pub const ME_QUAR_PIXEL_LEFT: i32 = 2;    // (-1,  0) in 1/4-pel units
pub const ME_QUAR_PIXEL_RIGHT: i32 = 3;   // ( 1,  0) in 1/4-pel units
pub const ME_QUAR_PIXEL_TOP: i32 = 4;     // ( 0, -1) in 1/4-pel units
pub const ME_QUAR_PIXEL_BOTTOM: i32 = 5;  // ( 0,  1) in 1/4-pel units

pub const NO_BEST_FRAC_PIX: i32 = 1; // REFINE_ME_NO_BEST_HALF_PIXEL + ME_NO_BEST_QUAR_PIXEL

// Video Analysis Assessment (VAA) texture signatures
pub const MBVAASIGN_FLAT: u8 = 15;
pub const MBVAASIGN_HOR1: u8 = 3;
pub const MBVAASIGN_HOR2: u8 = 12;
pub const MBVAASIGN_VER1: u8 = 5;
pub const MBVAASIGN_VER2: u8 = 10;
pub const MBVAASIGN_CMPX1: u8 = 6;
pub const MBVAASIGN_CMPX2: u8 = 9;

// Internal VAA thresholds
pub const INTRA_VARIANCE_SAD_THRESHOLD: i32 = 150;
pub const INTER_VARIANCE_SAD_THRESHOLD: i32 = 20;

// Neighbor availability bitmasks
pub const LEFT_MB_POS: u32 = 0x01;
pub const TOP_MB_POS: u32 = 0x02;
// `wels_common_basis.h:125-126`: TOPRIGHT is C (0x04) and TOPLEFT is D (0x08).
// These two were swapped here. Both are live: FillNeighborCacheIntra below tests
// `uiNeighborAvail & TOPLEFT_MB_POS` to set uiNeighborIntra bit 0x04, which indexes
// g_kiIntra16AvaliMode / g_kiNeighborIntraToI4x4, and FillNeighborCacheInter* test
// them for the top-left/top-right MV and ref-index caches.
// The mb_cache.h:118 comment describes the *encoded* uiNeighborIntra bit layout
// (TOPLEFT 0x04, TOPRIGHT 0x08), which is deliberately the reverse of the
// uiNeighborAvail layout; do not "reconcile" the two.
pub const TOPRIGHT_MB_POS: u32 = 0x04;
pub const TOPLEFT_MB_POS: u32 = 0x08;

pub const MB_LEFT_BIT: u32 = 0;
pub const MB_TOP_BIT: u32 = 1;
pub const MB_TOPRIGHT_BIT: u32 = 2;

// Reference index availability constants
pub const REF_NOT_IN_LIST: i8 = -1;
pub const REF_NOT_AVAIL: i8 = -2;

// Macroblock sizing & type constants
/// `MB_BLOCK8x8_NUM` — `wels_const_common.h:58`, the width of `SMB::iRefIndex`.
/// Re-exported rather than re-declared: `encoder_ext.rs:96` already owns the copy.
pub use crate::encoder::encoder_ext::MB_BLOCK8x8_NUM;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_LUMA_CHROMA_BLOCK4x4_NUM: usize = 24;
/// `INTRA_4x4_MODE_NUM` — `wels_const.h:48`. **8**, not 16; this is the per-macroblock
/// stride of `sWelsEncCtx::pIntra4x4PredModeBlocks`, so the wrong value made
/// `pIntra4x4PredMode.offset(-INTRA_4x4_MODE_NUM)` at md.rs:563 step back two
/// macroblocks instead of one.
pub const INTRA_4x4_MODE_NUM: usize = 8;
pub const MB_WIDTH_LUMA: i32 = 16;

// `wels_common_defs.h:275-283`. This block was transcribed as a dense 0x01..0x80
// ladder starting at MB_TYPE_SKIP; every one of the eight was wrong. The live
// consequence was in FillNeighborCacheIntra/Inter below, which test
// `uiMbType & MB_TYPE_INTRA4x4` (0x40 here, 0x01 in C++) to decide whether a
// neighbour contributes its I4x4 prediction modes, and `== MB_TYPE_SKIP`
// (0x01 here, 0x100 in C++) for the inter neighbour caches.
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_INTRA16x16: u32 = 0x00000002;
pub const MB_TYPE_INTRA8x8: u32 = 0x00000004;
pub const MB_TYPE_16x16: u32 = 0x00000008;
pub const MB_TYPE_16x8: u32 = 0x00000010;
pub const MB_TYPE_8x16: u32 = 0x00000020;
pub const MB_TYPE_8x8: u32 = 0x00000040;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000080;
pub const MB_TYPE_SKIP: u32 = 0x00000100;

// CPU feature flags

// Global Lookup Tables
pub const g_kiQpCostTable: [i32; 52] = [
    1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1,
    1, 1, 1, 1, 2, 2, 2, 2,
    3, 3, 3, 4, 4, 4, 5, 6,
    6, 7, 8, 9, 10, 11, 13, 14,
    16, 18, 20, 23, 25, 29, 32, 36,
    40, 45, 51, 57, 64, 72, 81, 91,
];

pub const g_kiMapModeI16x16: [i8; 7] = [0, 1, 2, 3, 2, 2, 2];
pub const g_kiMapModeIntraChroma: [i8; 7] = [0, 1, 2, 3, 0, 0, 0];

pub const G_KUI_GOLOMB_UE_LENGTH: [u32; 256] = [
    1, 3, 3, 5, 5, 5, 5, 7, 7, 7, 7, 7, 7, 7, 7,
    9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    17,
];

// Data Structures

pub use crate::encoder::svc_motion_estimate::SadPredISatdUnit;



#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SWelsMD_sMe {
    pub sMe16x16: SWelsME,
    pub sMe8x8: [SWelsME; 4],
    pub sMe16x8: [SWelsME; 2],
    pub sMe8x16: [SWelsME; 2],
    pub sMe4x4: [[SWelsME; 4]; 4],
    pub sMe8x4: [[SWelsME; 2]; 4],
    pub sMe4x8: [[SWelsME; 2]; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsMD {
    pub iLambda: i32,
    pub pMvdCost: *mut u16,
    pub iCostLuma: i32,
    pub iCostChroma: i32,
    pub iSadPredMb: i32,
    pub uiRef: u8,
    pub bMdUsingSad: bool,
    pub uiReserved: u16,
    pub iCostSkipMb: i32,
    pub iSadPredSkip: i32,
    pub iMbPixX: i32,
    pub iMbPixY: i32,
    pub iBlock8x8StaticIdc: [i32; 4],
    pub sMe: SWelsMD_sMe,
}

impl Default for SWelsMD {
    fn default() -> Self {
        Self {
            iLambda: 0,
            pMvdCost: std::ptr::null_mut(),
            iCostLuma: 0,
            iCostChroma: 0,
            iSadPredMb: 0,
            uiRef: 0,
            bMdUsingSad: false,
            uiReserved: 0,
            iCostSkipMb: 0,
            iSadPredSkip: 0,
            iMbPixX: 0,
            iMbPixY: 0,
            iBlock8x8StaticIdc: [0; 4],
            sMe: SWelsMD_sMe::default(),
        }
    }
}

pub type PCopyFunc = unsafe extern "C" fn(pDst: *mut u8, iStrideD: i32, pSrc: *mut u8, iStrideS: i32);
pub type PWelsSampleAveragingFunc = unsafe extern "C" fn(
    pDst: *mut u8,
    iDstStride: i32,
    pSrcA: *const u8,
    iStrideA: i32,
    pSrcB: *const u8,
    iStrideB: i32,
    iWidth: i32,
    iHeight: i32,
);
pub type PWelsLumaHalfpelMcFunc = unsafe extern "C" fn(
    pSrc: *const u8,
    iSrcStride: i32,
    pDst: *mut u8,
    iDstStride: i32,
    iWidth: i32,
    iHeight: i32,
);
pub type PSampleSadSatdCostFunc = unsafe extern "C" fn(
    pSample1: *mut u8,
    iStride1: i32,
    pSample2: *mut u8,
    iStride2: i32,
) -> i32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMeRefinePointer {
    pub pHalfPixH: *mut u8,
    pub pHalfPixV: *mut u8,
    pub pHalfPixHV: *mut u8,
    pub pQuarPixBest: *mut u8,
    pub pQuarPixTmp: *mut u8,
    pub pfCopyBlockByMode: Option<PCopyFunc>,
}

impl Default for SMeRefinePointer {
    fn default() -> Self {
        Self {
            pHalfPixH: std::ptr::null_mut(),
            pHalfPixV: std::ptr::null_mut(),
            pHalfPixHV: std::ptr::null_mut(),
            pQuarPixBest: std::ptr::null_mut(),
            pQuarPixTmp: std::ptr::null_mut(),
            pfCopyBlockByMode: None,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SQuarRefineParams {
    pub iBestCost: i32,
    pub iBestHalfPix: i32,
    pub iStrideA: i32,
    pub iStrideB: i32,
    pub pRef: *mut u8,
    pub pSrcB: [*mut u8; 4],
    pub pSrcA: [*mut u8; 4],
    pub iLms: [i32; 4],
    pub iBestQuarPix: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SMB {
    pub uiMbType: u32,
    pub uiSubMbType: [u8; 4],
    pub iMbXY: i32,
    pub iMbX: i16,
    pub iMbY: i16,
    pub uiNeighborAvail: u8,
    pub uiCbp: u8,
    /// `SMVUnitXY* sMv` in the C++, pointed at a slot of the context-wide
    /// `pMvUnitBlock4x4` bank. **T6.C1** made it the storage: the `SMB` list is
    /// `WelsMallocz`'d, so an all-integer inline array starts at MV (0,0) exactly
    /// as the zeroed bank did (S21), and every macroblock owns its own row —
    /// which is what the two parity banks were buying.
    pub sMv: [SMVUnitXY; MB_BLOCK4x4_NUM],
    /// `int8_t* pRefIndex` — one entry per 8x8 partition.
    pub iRefIndex: [i8; MB_BLOCK8x8_NUM],
    /// `int32_t* pSadCost` — the C++ points this at one `int32_t` per macroblock
    /// (`pSadCostMb + iMbXY`) and every site reads `[0]`, so the inline form is a
    /// scalar rather than an array.
    pub iSadCost: i32,
    /// `int8_t* pIntra4x4PredMode` — 8 published modes (4 right column, 3 bottom
    /// row, 1 unused) that the *next* macroblock's `FillNeighborCacheIntra` reads.
    pub iIntra4x4PredMode: [i8; INTRA_4x4_MODE_NUM],
    /// `int8_t* pNonZeroCount` — 16 luma + 8 chroma 4x4 blocks.
    pub iNonZeroCount: [i8; MB_LUMA_CHROMA_BLOCK4x4_NUM],
    pub sP16x16Mv: SMVUnitXY,
    pub uiLumaQp: u8,
    pub uiChromaQp: u8,
    pub uiSliceIdc: u16,
    pub uiChromPredMode: u32,
    pub iLumaDQp: i32,
    pub sMvd: [SMVUnitXY; MB_BLOCK4x4_NUM],
    pub iCbpDc: i32,
}

impl Default for SMB {
    fn default() -> Self {
        Self {
            uiMbType: 0,
            uiSubMbType: [0; 4],
            iMbXY: 0,
            iMbX: 0,
            iMbY: 0,
            uiNeighborAvail: 0,
            uiCbp: 0,
            sMv: [SMVUnitXY::default(); MB_BLOCK4x4_NUM],
            iRefIndex: [0; MB_BLOCK8x8_NUM],
            iSadCost: 0,
            iIntra4x4PredMode: [0; INTRA_4x4_MODE_NUM],
            iNonZeroCount: [0; MB_LUMA_CHROMA_BLOCK4x4_NUM],
            sP16x16Mv: SMVUnitXY::default(),
            uiLumaQp: 0,
            uiChromaQp: 0,
            uiSliceIdc: 0,
            uiChromPredMode: 0,
            iLumaDQp: 0,
            sMvd: [SMVUnitXY::default(); MB_BLOCK4x4_NUM],
            iCbpDc: 0,
        }
    }
}

/// `TagMbCache` — `codec/encoder/core/inc/mb_cache.h:72`. 576 bytes.
///
/// C++ declares the first three members with `ALIGNED_DECLARE (..., 16)`, which
/// aligns each *variable* to 16 bytes and gives the struct 16-byte alignment. Rust
/// has no per-field alignment attribute, so that is reproduced here as
/// `#[repr(C, align(16))]` plus the 14 bytes of padding the C++ compiler inserts
/// after `sMvComponents` (146 bytes) to bring `iNonZeroCoeffCount` to offset 160.
/// The two `[i8; 48]` arrays are already 16-multiples and need no further padding.
#[repr(C, align(16))]
#[derive(Debug, Copy, Clone)]
pub struct SMbCache {
    pub sMvComponents: SMVComponentUnit,
    pub _pad_after_sMvComponents: [u8; 14],
    pub iNonZeroCoeffCount: [i8; 48],
    pub iIntraPredMode: [i8; 48],
    pub iSadCost: [i32; 4],
    pub sMbMvp: [SMVUnitXY; MB_BLOCK4x4_NUM],
    pub pCoeffLevel: *mut i16,
    pub pSkipMb: *mut u8,
    pub pMemPredMb: *mut u8,
    pub pMemPredLuma: *mut u8,
    pub pMemPredChroma: *mut u8,
    pub pBestPredIntraChroma: *mut u8,
    pub pMemPredBlk4: *mut u8,
    pub pBestPredI4x4Blk4: *mut u8,
    pub pBufferInterPredMe: *mut u8,
    pub pPrevIntra4x4PredModeFlag: *mut bool,
    pub pRemIntra4x4PredModeFlag: *mut i8,
    pub iSadCostSkip: [i32; 4],
    pub bMbTypeSkip: [bool; 4],
    pub pEncSad: *mut i32,
    pub pDct: *mut SDCTCoeff,
    pub uiNeighborIntra: u8,
    pub uiLumaI16x16Mode: u8,
    pub uiChmaI8x8Mode: u8,
    pub bCollocatedPredFlag: bool,
    pub uiRefMbType: u32,
    // mb_cache.h:124 — an anonymous struct member literally named SPicData, not four
    // flattened arrays.
    pub SPicData: SPicData,
}

impl Default for SMbCache {
    fn default() -> Self {
        Self {
            sMvComponents: SMVComponentUnit::default(),
            _pad_after_sMvComponents: [0; 14],
            iNonZeroCoeffCount: [0; 48],
            iIntraPredMode: [0; 48],
            iSadCost: [0; 4],
            sMbMvp: [SMVUnitXY::default(); MB_BLOCK4x4_NUM],
            pCoeffLevel: std::ptr::null_mut(),
            pSkipMb: std::ptr::null_mut(),
            pMemPredMb: std::ptr::null_mut(),
            pMemPredLuma: std::ptr::null_mut(),
            pMemPredChroma: std::ptr::null_mut(),
            pBestPredIntraChroma: std::ptr::null_mut(),
            pMemPredBlk4: std::ptr::null_mut(),
            pBestPredI4x4Blk4: std::ptr::null_mut(),
            pBufferInterPredMe: std::ptr::null_mut(),
            pPrevIntra4x4PredModeFlag: std::ptr::null_mut(),
            pRemIntra4x4PredModeFlag: std::ptr::null_mut(),
            iSadCostSkip: [0; 4],
            bMbTypeSkip: [false; 4],
            pEncSad: std::ptr::null_mut(),
            pDct: std::ptr::null_mut(),
            uiNeighborIntra: 0,
            uiLumaI16x16Mode: 0,
            uiChmaI8x8Mode: 0,
            bCollocatedPredFlag: false,
            uiRefMbType: 0,
            SPicData: SPicData::default(),
        }
    }
}

pub type PFillInterNeighborCacheFunc = unsafe extern "C" fn(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: i32,
    pVaaBgMbFlag: *mut i8,
);
pub type PGetVarianceFromIntraVaaFunc = unsafe extern "C" fn(pDataY: *mut u8, kiLineSize: i32) -> i32;
pub type PGetMbSignFromInterVaaFunc = unsafe extern "C" fn(pSad8x8: *mut i32) -> u8;
/// **T6.C1**: the slot took `SMVUnitXY*` because the C++ hands it
/// `pCurMb->sMv`, a pointer into the context-wide MV bank. The bank is an inline
/// row of `SMB` now, and the kernel writes all sixteen entries, so it takes the row.
pub type PUpdateMbMvFunc = fn(pMvBuffer: &mut [SMVUnitXY; MB_BLOCK4x4_NUM], ksMv: SMVUnitXY);

// SMcFunc is a common-layer type (codec/common/inc/mc.h:46); common/mc.rs already
// had it with correctly typed pointers where this copy used *mut c_void.
pub use crate::common::mc::SMcFunc;
// Phase 4a: MC and the half-pel filters are called directly, not via `sMcFuncs`.
use crate::common::mc::{McHorVer02_c, McHorVer20_c, McHorVer22_c, PixelAvg_c};
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::encoder_context::SDCTCoeff;
pub use crate::encoder::encoder_context::BLOCK_SIZE_ALL;
pub use crate::encoder::svc_motion_estimate::PSample4SadCostFunc;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;

/// Which sibling cost array a selector in [`SSampleDealingFunc`] names.
///
/// Phase 4a, §2.2.5 tier 2: config dispatch becomes an `enum` + `match`. The
/// C++ (`wels_func_ptr_def.h`) stores a `PSampleSadSatdCostFunc*` and points it
/// at one of the two arrays in the same struct; the port copied that literally,
/// which is F13's fourth site. The selection is made once per layer from
/// encoder config and never varies per macroblock, so a tag carries every bit
/// of information the pointer did — and unlike the pointer, taking `&mut` on
/// the enclosing struct cannot invalidate it.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CostFamily {
    /// No cost function selected — the old null pointer. The C++ leaves the
    /// slot unset outside the configurations that use it, where reading it was
    /// already a null deref.
    #[default]
    Unset,
    /// `pfSampleSad` — sum of absolute differences.
    Sad,
    /// `pfSampleSatd` — Hadamard-transformed absolute differences.
    Satd,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSampleDealingFunc {
    // wels_func_ptr_def.h:163 — these are [MAX_BLOCK_TYPE], and MAX_BLOCK_TYPE is
    // BLOCK_SIZE_ALL = 7 (wels_func_ptr_def.h:161, wels_const.h:147). They were [_; 8].
    pub pfSampleSad: [Option<PSampleSadSatdCostFunc>; BLOCK_SIZE_ALL],
    pub pfSampleSatd: [Option<PSampleSadSatdCostFunc>; BLOCK_SIZE_ALL],
    pub pfSample4Sad: [Option<PSample4SadCostFunc>; BLOCK_SIZE_ALL],
    // The five `pfIntra*Combined3*Satd`/`Sad` slots and the three
    // `pfIntra*Combined3` they were copied into were here, all eight
    // `*mut c_void`. The C++ leaves them NULL on every target this port builds
    // for and the port never assigned them, so `assert_no_combined3` guarded
    // three reads and the scalar branch was the only live one — S18: deleted
    // with the guard and its call sites (Phase 6 session B).
    /// Which of the two sibling cost arrays mode decision reads. **Was
    /// `*mut Option<PSampleSadSatdCostFunc>` pointing into this same struct**,
    /// which is F13's fourth site: `SetFastCodingFunc` stored
    /// `pfSampleSad.as_mut_ptr()` here, so every later `&mut SWelsFuncPtrList`
    /// — and the encoder takes one constantly — reborrowed the whole struct and
    /// popped that interior pointer's tag, making the next read UB under
    /// Stacked Borrows. A tag has nothing to invalidate. Read it through
    /// [`SSampleDealingFunc::md_cost`].
    pub pfMdCost: CostFamily,
    /// As [`Self::pfMdCost`], for motion estimation. `CostFamily::Unset` is the
    /// old null: the C++ leaves this unset outside the ME-capable
    /// configurations, and the port reproduced that with a null pointer.
    pub pfMeCost: CostFamily,
}

impl Default for SSampleDealingFunc {
    fn default() -> Self {
        Self {
            pfSampleSad: [None; BLOCK_SIZE_ALL],
            pfSampleSatd: [None; BLOCK_SIZE_ALL],
            pfSample4Sad: [None; BLOCK_SIZE_ALL],
            pfMdCost: CostFamily::Unset,
            pfMeCost: CostFamily::Unset,
        }
    }
}

impl SSampleDealingFunc {
    /// The mode-decision cost function for `block`, or `None` if unselected.
    ///
    /// Replaces `(*sdf.pfMdCost.add(block)).unwrap()`. The old expression read
    /// through a pointer into this struct's own `pfSampleSad`/`pfSampleSatd`
    /// array, which any intervening `&mut SWelsFuncPtrList` had already
    /// invalidated (F13). Same selection, same array, no interior pointer.
    #[inline(always)]
    pub fn md_cost(&self, block: usize) -> Option<PSampleSadSatdCostFunc> {
        match self.pfMdCost {
            CostFamily::Sad => self.pfSampleSad[block],
            CostFamily::Satd => self.pfSampleSatd[block],
            CostFamily::Unset => None,
        }
    }

    /// The motion-estimation cost function for `block`, or `None` if unselected.
    ///
    /// As [`Self::md_cost`]. `CostFamily::Unset` is the old null pointer, which
    /// the encoder installs outside the ME-capable configurations; a caller
    /// that reached it used to null-deref and now unwraps a `None`, which is
    /// the same bug reported at the same place with a legible message.
    #[inline(always)]
    pub fn me_cost(&self, block: usize) -> Option<PSampleSadSatdCostFunc> {
        match self.pfMeCost {
            CostFamily::Sad => self.pfSampleSad[block],
            CostFamily::Satd => self.pfSampleSatd[block],
            CostFamily::Unset => None,
        }
    }
}




// Mathematical Helper Functions & Macros
#[inline(always)]
pub fn BsSizeUE(kiValue: u32) -> u32 {
    if kiValue < 256 {
        G_KUI_GOLOMB_UE_LENGTH[kiValue as usize]
    } else {
        let mut n = 0u32;
        let mut iTmpValue = kiValue + 1;
        if (iTmpValue & 0xffff0000) != 0 {
            iTmpValue >>= 16;
            n += 16;
        }
        if (iTmpValue & 0xff00) != 0 {
            iTmpValue >>= 8;
            n += 8;
        }
        n += G_KUI_GOLOMB_UE_LENGTH[(iTmpValue - 1) as usize] >> 1;
        (n << 1) + 1
    }
}

#[inline(always)]
pub fn BsSizeSE(kiValue: i32) -> u32 {
    if kiValue == 0 {
        1
    } else if kiValue > 0 {
        let iTmpValue = ((kiValue as u32) << 1) - 1;
        BsSizeUE(iTmpValue)
    } else {
        let iTmpValue = (-kiValue as u32) << 1;
        BsSizeUE(iTmpValue)
    }
}

#[inline(always)]
pub unsafe fn COST_MVD(pMvdCost: *const u16, iMvdX: i32, iMvdY: i32) -> i32 {
    let x = *pMvdCost.offset(iMvdX as isize) as i32;
    let y = *pMvdCost.offset(iMvdY as isize) as i32;
    x + y
}

#[inline(always)]
pub fn REPLACE_SAD_MULTIPLY(x: i32) -> i32 {
    x - (x >> 3) + (x >> 5)
}

#[inline(always)]
pub fn WelsMedian(iA: i32, iB: i32, iC: i32) -> i32 {
    let mut min = iA;
    let mut max = iA;
    if iB < min {
        min = iB;
    }
    if iB > max {
        max = iB;
    }
    if iC < min {
        min = iC;
    }
    if iC > max {
        max = iC;
    }
    iA + iB + iC - min - max
}

#[inline(always)]
pub fn IS_SVC_INTER(uiMbType: u32) -> bool {
    (uiMbType & (MB_TYPE_16x16 | MB_TYPE_16x8 | MB_TYPE_8x16 | MB_TYPE_8x8 | MB_TYPE_8x8_REF0 | MB_TYPE_SKIP)) != 0
}

// Function Implementations
pub unsafe extern "C" fn FillNeighborCacheIntra(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: i32,
) {
    let uiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let mut uiNeighborIntra: u32 = 0;

    if (uiNeighborAvail & LEFT_MB_POS) != 0 {
        // C++ reaches the left macroblock's rows by stepping back one stride in the
        // flat context arrays (`pNonZeroCount - MB_LUMA_CHROMA_BLOCK4x4_NUM`,
        // `pIntra4x4PredMode - INTRA_4x4_MODE_NUM`). T6.C1: the arrays are inline, so
        // the same values come from the left macroblock's own struct -- which is the
        // one `pCurMb - 1` names, exactly as the branch below already assumed.
        let pLeftMb = pCurMb.offset(-1);
        (*pMbCache).iNonZeroCoeffCount[8] = (*pLeftMb).iNonZeroCount[3];
        (*pMbCache).iNonZeroCoeffCount[16] = (*pLeftMb).iNonZeroCount[7];
        (*pMbCache).iNonZeroCoeffCount[24] = (*pLeftMb).iNonZeroCount[11];
        (*pMbCache).iNonZeroCoeffCount[32] = (*pLeftMb).iNonZeroCount[15];

        (*pMbCache).iNonZeroCoeffCount[13] = (*pLeftMb).iNonZeroCount[17];
        (*pMbCache).iNonZeroCoeffCount[21] = (*pLeftMb).iNonZeroCount[21];
        (*pMbCache).iNonZeroCoeffCount[37] = (*pLeftMb).iNonZeroCount[19];
        (*pMbCache).iNonZeroCoeffCount[45] = (*pLeftMb).iNonZeroCount[23];

        uiNeighborIntra |= LEFT_MB_POS;

        if ((*pLeftMb).uiMbType & MB_TYPE_INTRA4x4) != 0 {
            (*pMbCache).iIntraPredMode[8] = (*pLeftMb).iIntra4x4PredMode[4];
            (*pMbCache).iIntraPredMode[16] = (*pLeftMb).iIntra4x4PredMode[5];
            (*pMbCache).iIntraPredMode[24] = (*pLeftMb).iIntra4x4PredMode[6];
            (*pMbCache).iIntraPredMode[32] = (*pLeftMb).iIntra4x4PredMode[3];
        } else {
            (*pMbCache).iIntraPredMode[8] = 2;
            (*pMbCache).iIntraPredMode[16] = 2;
            (*pMbCache).iIntraPredMode[24] = 2;
            (*pMbCache).iIntraPredMode[32] = 2;
        }
    } else {
        (*pMbCache).iNonZeroCoeffCount[8] = -1;
        (*pMbCache).iNonZeroCoeffCount[16] = -1;
        (*pMbCache).iNonZeroCoeffCount[24] = -1;
        (*pMbCache).iNonZeroCoeffCount[32] = -1;
        (*pMbCache).iNonZeroCoeffCount[13] = -1;
        (*pMbCache).iNonZeroCoeffCount[21] = -1;
        (*pMbCache).iNonZeroCoeffCount[37] = -1;
        (*pMbCache).iNonZeroCoeffCount[45] = -1;

        (*pMbCache).iIntraPredMode[8] = -1;
        (*pMbCache).iIntraPredMode[16] = -1;
        (*pMbCache).iIntraPredMode[24] = -1;
        (*pMbCache).iIntraPredMode[32] = -1;
    }

    if (uiNeighborAvail & TOP_MB_POS) != 0 {
        let pTopMb = pCurMb.offset(-(iMbWidth as isize));
        // One reborrow of each row rather than one per statement: the destination is
        // the cache's array and the source is the top macroblock's, two allocations.
        let pCacheNzc = &mut (*pMbCache).iNonZeroCoeffCount;
        let pTopNzc = &(*pTopMb).iNonZeroCount;
        pCacheNzc[1..5].copy_from_slice(&pTopNzc[12..16]);
        pCacheNzc[6..8].copy_from_slice(&pTopNzc[20..22]);
        pCacheNzc[30..32].copy_from_slice(&pTopNzc[22..24]);

        uiNeighborIntra |= TOP_MB_POS;

        if ((*pTopMb).uiMbType & MB_TYPE_INTRA4x4) != 0 {
            let pCacheMode = &mut (*pMbCache).iIntraPredMode;
            let pTopMode = &(*pTopMb).iIntra4x4PredMode;
            pCacheMode[1..5].copy_from_slice(&pTopMode[0..4]);
        } else {
            (*pMbCache).iIntraPredMode[1] = 2;
            (*pMbCache).iIntraPredMode[2] = 2;
            (*pMbCache).iIntraPredMode[3] = 2;
            (*pMbCache).iIntraPredMode[4] = 2;
        }
    } else {
        std::ptr::write_bytes((*pMbCache).iIntraPredMode.as_mut_ptr().add(1), 0xff, 4);
        std::ptr::write_bytes((*pMbCache).iNonZeroCoeffCount.as_mut_ptr().add(1), 0xff, 4);
        std::ptr::write_bytes((*pMbCache).iNonZeroCoeffCount.as_mut_ptr().add(6), 0xff, 2);
        std::ptr::write_bytes((*pMbCache).iNonZeroCoeffCount.as_mut_ptr().add(30), 0xff, 2);
    }

    if (uiNeighborAvail & TOPLEFT_MB_POS) != 0 {
        uiNeighborIntra |= 0x04;
    }
    if (uiNeighborAvail & TOPRIGHT_MB_POS) != 0 {
        uiNeighborIntra |= 0x08;
    }
    (*pMbCache).uiNeighborIntra = uiNeighborIntra as u8;
}

pub unsafe extern "C" fn FillNeighborCacheInterWithoutBGD(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: i32,
    _pVaaBgMbFlag: *mut i8,
) {
    let uiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    // F14's class: a neighbour pointer formed before its availability guard, dereferenced only under it — `wrapping_offset` keeps the arithmetic defined off the edge of the MB array (the encode probe, Phase 6 session B).
    let pLeftMb = pCurMb.wrapping_offset(-1);
    let pTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize));
    let pLeftTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize) - 1);
    let iRightTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize) + 1);
    let pMvComp = &mut (*pMbCache).sMvComponents;

    if (uiNeighborAvail & LEFT_MB_POS) != 0 && IS_SVC_INTER((*pLeftMb).uiMbType) {
        pMvComp.sMotionVectorCache[6] = (*pLeftMb).sMv[3];
        pMvComp.sMotionVectorCache[12] = (*pLeftMb).sMv[7];
        pMvComp.sMotionVectorCache[18] = (*pLeftMb).sMv[11];
        pMvComp.sMotionVectorCache[24] = (*pLeftMb).sMv[15];
        pMvComp.iRefIndexCache[6] = (*pLeftMb).iRefIndex[1];
        pMvComp.iRefIndexCache[12] = (*pLeftMb).iRefIndex[1];
        pMvComp.iRefIndexCache[18] = (*pLeftMb).iRefIndex[3];
        pMvComp.iRefIndexCache[24] = (*pLeftMb).iRefIndex[3];
        (*pMbCache).iSadCost[3] = (*pLeftMb).iSadCost;

        if (*pLeftMb).uiMbType == MB_TYPE_SKIP {
            (*pMbCache).bMbTypeSkip[3] = true;
            (*pMbCache).iSadCostSkip[3] = *(*pMbCache).pEncSad.offset(-1);
        } else {
            (*pMbCache).bMbTypeSkip[3] = false;
            (*pMbCache).iSadCostSkip[3] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[6] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[12] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[18] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[24] = SMVUnitXY::default();
        let ref_val = if (uiNeighborAvail & LEFT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        pMvComp.iRefIndexCache[6] = ref_val;
        pMvComp.iRefIndexCache[12] = ref_val;
        pMvComp.iRefIndexCache[18] = ref_val;
        pMvComp.iRefIndexCache[24] = ref_val;
        (*pMbCache).iSadCost[3] = 0;
        (*pMbCache).bMbTypeSkip[3] = false;
        (*pMbCache).iSadCostSkip[3] = 0;
    }

    if (uiNeighborAvail & TOP_MB_POS) != 0 && IS_SVC_INTER((*pTopMb).uiMbType) {
        let pTopMv = &(*pTopMb).sMv;
        pMvComp.sMotionVectorCache[1..3].copy_from_slice(&pTopMv[12..14]);
        pMvComp.sMotionVectorCache[3..5].copy_from_slice(&pTopMv[14..16]);
        pMvComp.iRefIndexCache[1] = (*pTopMb).iRefIndex[2];
        pMvComp.iRefIndexCache[2] = (*pTopMb).iRefIndex[2];
        pMvComp.iRefIndexCache[3] = (*pTopMb).iRefIndex[3];
        pMvComp.iRefIndexCache[4] = (*pTopMb).iRefIndex[3];
        (*pMbCache).iSadCost[1] = (*pTopMb).iSadCost;

        if (*pTopMb).uiMbType == MB_TYPE_SKIP {
            (*pMbCache).bMbTypeSkip[1] = true;
            (*pMbCache).iSadCostSkip[1] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize));
        } else {
            (*pMbCache).bMbTypeSkip[1] = false;
            (*pMbCache).iSadCostSkip[1] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[1] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[2] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[3] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[4] = SMVUnitXY::default();
        let ref_val = if (uiNeighborAvail & TOP_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        pMvComp.iRefIndexCache[1] = ref_val;
        pMvComp.iRefIndexCache[2] = ref_val;
        pMvComp.iRefIndexCache[3] = ref_val;
        pMvComp.iRefIndexCache[4] = ref_val;
        (*pMbCache).iSadCost[1] = 0;
        (*pMbCache).bMbTypeSkip[1] = false;
        (*pMbCache).iSadCostSkip[1] = 0;
    }

    if (uiNeighborAvail & TOPLEFT_MB_POS) != 0 && IS_SVC_INTER((*pLeftTopMb).uiMbType) {
        pMvComp.sMotionVectorCache[0] = (*pLeftTopMb).sMv[15];
        pMvComp.iRefIndexCache[0] = (*pLeftTopMb).iRefIndex[3];
        (*pMbCache).iSadCost[0] = (*pLeftTopMb).iSadCost;

        if (*pLeftTopMb).uiMbType == MB_TYPE_SKIP {
            (*pMbCache).bMbTypeSkip[0] = true;
            (*pMbCache).iSadCostSkip[0] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize) - 1);
        } else {
            (*pMbCache).bMbTypeSkip[0] = false;
            (*pMbCache).iSadCostSkip[0] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[0] = SMVUnitXY::default();
        pMvComp.iRefIndexCache[0] = if (uiNeighborAvail & TOPLEFT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        (*pMbCache).iSadCost[0] = 0;
        (*pMbCache).bMbTypeSkip[0] = false;
        (*pMbCache).iSadCostSkip[0] = 0;
    }

    if (uiNeighborAvail & TOPRIGHT_MB_POS) != 0 && IS_SVC_INTER((*iRightTopMb).uiMbType) {
        pMvComp.sMotionVectorCache[5] = (*iRightTopMb).sMv[12];
        pMvComp.iRefIndexCache[5] = (*iRightTopMb).iRefIndex[2];
        (*pMbCache).iSadCost[2] = (*iRightTopMb).iSadCost;

        if (*iRightTopMb).uiMbType == MB_TYPE_SKIP {
            (*pMbCache).bMbTypeSkip[2] = true;
            (*pMbCache).iSadCostSkip[2] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize) + 1);
        } else {
            (*pMbCache).bMbTypeSkip[2] = false;
            (*pMbCache).iSadCostSkip[2] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[5] = SMVUnitXY::default();
        pMvComp.iRefIndexCache[5] = if (uiNeighborAvail & TOPRIGHT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        (*pMbCache).iSadCost[2] = 0;
        (*pMbCache).bMbTypeSkip[2] = false;
        (*pMbCache).iSadCostSkip[2] = 0;
    }

    pMvComp.sMotionVectorCache[9] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[21] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[11] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[17] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[23] = SMVUnitXY::default();
    pMvComp.iRefIndexCache[9] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[11] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[17] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[21] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[23] = REF_NOT_AVAIL;
}

pub unsafe extern "C" fn FillNeighborCacheInterWithBGD(
    pMbCache: *mut SMbCache,
    pCurMb: *mut SMB,
    iMbWidth: i32,
    pVaaBgMbFlag: *mut i8,
) {
    let uiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    // F14's class: a neighbour pointer formed before its availability guard, dereferenced only under it — `wrapping_offset` keeps the arithmetic defined off the edge of the MB array (the encode probe, Phase 6 session B).
    let pLeftMb = pCurMb.wrapping_offset(-1);
    let pTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize));
    let pLeftTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize) - 1);
    let iRightTopMb = pCurMb.wrapping_offset(-(iMbWidth as isize) + 1);
    let pMvComp = &mut (*pMbCache).sMvComponents;

    if (uiNeighborAvail & LEFT_MB_POS) != 0 && IS_SVC_INTER((*pLeftMb).uiMbType) {
        pMvComp.sMotionVectorCache[6] = (*pLeftMb).sMv[3];
        pMvComp.sMotionVectorCache[12] = (*pLeftMb).sMv[7];
        pMvComp.sMotionVectorCache[18] = (*pLeftMb).sMv[11];
        pMvComp.sMotionVectorCache[24] = (*pLeftMb).sMv[15];
        pMvComp.iRefIndexCache[6] = (*pLeftMb).iRefIndex[1];
        pMvComp.iRefIndexCache[12] = (*pLeftMb).iRefIndex[1];
        pMvComp.iRefIndexCache[18] = (*pLeftMb).iRefIndex[3];
        pMvComp.iRefIndexCache[24] = (*pLeftMb).iRefIndex[3];
        (*pMbCache).iSadCost[3] = (*pLeftMb).iSadCost;

        if (*pLeftMb).uiMbType == MB_TYPE_SKIP && *pVaaBgMbFlag.offset(-1) == 0 {
            (*pMbCache).bMbTypeSkip[3] = true;
            (*pMbCache).iSadCostSkip[3] = *(*pMbCache).pEncSad.offset(-1);
        } else {
            (*pMbCache).bMbTypeSkip[3] = false;
            (*pMbCache).iSadCostSkip[3] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[6] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[12] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[18] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[24] = SMVUnitXY::default();
        let ref_val = if (uiNeighborAvail & LEFT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        pMvComp.iRefIndexCache[6] = ref_val;
        pMvComp.iRefIndexCache[12] = ref_val;
        pMvComp.iRefIndexCache[18] = ref_val;
        pMvComp.iRefIndexCache[24] = ref_val;
        (*pMbCache).iSadCost[3] = 0;
        (*pMbCache).bMbTypeSkip[3] = false;
        (*pMbCache).iSadCostSkip[3] = 0;
    }

    if (uiNeighborAvail & TOP_MB_POS) != 0 && IS_SVC_INTER((*pTopMb).uiMbType) {
        let pTopMv = &(*pTopMb).sMv;
        pMvComp.sMotionVectorCache[1..3].copy_from_slice(&pTopMv[12..14]);
        pMvComp.sMotionVectorCache[3..5].copy_from_slice(&pTopMv[14..16]);
        pMvComp.iRefIndexCache[1] = (*pTopMb).iRefIndex[2];
        pMvComp.iRefIndexCache[2] = (*pTopMb).iRefIndex[2];
        pMvComp.iRefIndexCache[3] = (*pTopMb).iRefIndex[3];
        pMvComp.iRefIndexCache[4] = (*pTopMb).iRefIndex[3];
        (*pMbCache).iSadCost[1] = (*pTopMb).iSadCost;

        if (*pTopMb).uiMbType == MB_TYPE_SKIP && *pVaaBgMbFlag.offset(-(iMbWidth as isize)) == 0 {
            (*pMbCache).bMbTypeSkip[1] = true;
            (*pMbCache).iSadCostSkip[1] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize));
        } else {
            (*pMbCache).bMbTypeSkip[1] = false;
            (*pMbCache).iSadCostSkip[1] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[1] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[2] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[3] = SMVUnitXY::default();
        pMvComp.sMotionVectorCache[4] = SMVUnitXY::default();
        let ref_val = if (uiNeighborAvail & TOP_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        pMvComp.iRefIndexCache[1] = ref_val;
        pMvComp.iRefIndexCache[2] = ref_val;
        pMvComp.iRefIndexCache[3] = ref_val;
        pMvComp.iRefIndexCache[4] = ref_val;
        (*pMbCache).iSadCost[1] = 0;
        (*pMbCache).bMbTypeSkip[1] = false;
        (*pMbCache).iSadCostSkip[1] = 0;
    }

    if (uiNeighborAvail & TOPLEFT_MB_POS) != 0 && IS_SVC_INTER((*pLeftTopMb).uiMbType) {
        pMvComp.sMotionVectorCache[0] = (*pLeftTopMb).sMv[15];
        pMvComp.iRefIndexCache[0] = (*pLeftTopMb).iRefIndex[3];
        (*pMbCache).iSadCost[0] = (*pLeftTopMb).iSadCost;

        if (*pLeftTopMb).uiMbType == MB_TYPE_SKIP && *pVaaBgMbFlag.offset(-(iMbWidth as isize) - 1) == 0 {
            (*pMbCache).bMbTypeSkip[0] = true;
            (*pMbCache).iSadCostSkip[0] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize) - 1);
        } else {
            (*pMbCache).bMbTypeSkip[0] = false;
            (*pMbCache).iSadCostSkip[0] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[0] = SMVUnitXY::default();
        pMvComp.iRefIndexCache[0] = if (uiNeighborAvail & TOPLEFT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        (*pMbCache).iSadCost[0] = 0;
        (*pMbCache).bMbTypeSkip[0] = false;
        (*pMbCache).iSadCostSkip[0] = 0;
    }

    if (uiNeighborAvail & TOPRIGHT_MB_POS) != 0 && IS_SVC_INTER((*iRightTopMb).uiMbType) {
        pMvComp.sMotionVectorCache[5] = (*iRightTopMb).sMv[12];
        pMvComp.iRefIndexCache[5] = (*iRightTopMb).iRefIndex[2];
        (*pMbCache).iSadCost[2] = (*iRightTopMb).iSadCost;

        if (*iRightTopMb).uiMbType == MB_TYPE_SKIP && *pVaaBgMbFlag.offset(-(iMbWidth as isize) + 1) == 0 {
            (*pMbCache).bMbTypeSkip[2] = true;
            (*pMbCache).iSadCostSkip[2] = *(*pMbCache).pEncSad.offset(-(iMbWidth as isize) + 1);
        } else {
            (*pMbCache).bMbTypeSkip[2] = false;
            (*pMbCache).iSadCostSkip[2] = 0;
        }
    } else {
        pMvComp.sMotionVectorCache[5] = SMVUnitXY::default();
        pMvComp.iRefIndexCache[5] = if (uiNeighborAvail & TOPRIGHT_MB_POS) != 0 { REF_NOT_IN_LIST } else { REF_NOT_AVAIL };
        (*pMbCache).iSadCost[2] = 0;
        (*pMbCache).bMbTypeSkip[2] = false;
        (*pMbCache).iSadCostSkip[2] = 0;
    }

    pMvComp.sMotionVectorCache[9] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[21] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[11] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[17] = SMVUnitXY::default();
    pMvComp.sMotionVectorCache[23] = SMVUnitXY::default();
    pMvComp.iRefIndexCache[9] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[11] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[17] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[21] = REF_NOT_AVAIL;
    pMvComp.iRefIndexCache[23] = REF_NOT_AVAIL;
}

pub unsafe extern "C" fn InitFillNeighborCacheInterFunc(
    pFuncList: *mut SWelsFuncPtrList,
    kiFlag: i32,
) {
    (*pFuncList).pfFillInterNeighborCache = if kiFlag != 0 {
        Some(FillNeighborCacheInterWithBGD)
    } else {
        Some(FillNeighborCacheInterWithoutBGD)
    };
}

pub fn UpdateMbMv_c(pMvBuffer: &mut [SMVUnitXY; MB_BLOCK4x4_NUM], ksMv: SMVUnitXY) {
    for k in (0..MB_BLOCK4x4_NUM).step_by(4) {
        pMvBuffer[k] = ksMv;
        pMvBuffer[k + 1] = ksMv;
        pMvBuffer[k + 2] = ksMv;
        pMvBuffer[k + 3] = ksMv;
    }
}

pub unsafe extern "C" fn MdInterAnalysisVaaInfo_c(pSad8x8: *mut i32) -> u8 {
    let mut iSadBlock = [0i32; 4];
    let mut iAverageSadBlock = [0i32; 4];

    iSadBlock[0] = *pSad8x8.add(0);
    let mut iAverageSad = iSadBlock[0];

    iSadBlock[1] = *pSad8x8.add(1);
    iAverageSad += iSadBlock[1];

    iSadBlock[2] = *pSad8x8.add(2);
    iAverageSad += iSadBlock[2];

    iSadBlock[3] = *pSad8x8.add(3);
    iAverageSad += iSadBlock[3];

    iAverageSad >>= 2;

    iAverageSadBlock[0] = (iSadBlock[0] >> 6) - (iAverageSad >> 6);
    let mut iVarianceSad = iAverageSadBlock[0] * iAverageSadBlock[0];

    iAverageSadBlock[1] = (iSadBlock[1] >> 6) - (iAverageSad >> 6);
    iVarianceSad += iAverageSadBlock[1] * iAverageSadBlock[1];

    iAverageSadBlock[2] = (iSadBlock[2] >> 6) - (iAverageSad >> 6);
    iVarianceSad += iAverageSadBlock[2] * iAverageSadBlock[2];

    iAverageSadBlock[3] = (iSadBlock[3] >> 6) - (iAverageSad >> 6);
    iVarianceSad += iAverageSadBlock[3] * iAverageSadBlock[3];

    if iVarianceSad < INTER_VARIANCE_SAD_THRESHOLD {
        return 15;
    }

    let mut uiMbSign: u8 = 0;
    if iSadBlock[0] > iAverageSad {
        uiMbSign |= 0x08;
    }
    if iSadBlock[1] > iAverageSad {
        uiMbSign |= 0x04;
    }
    if iSadBlock[2] > iAverageSad {
        uiMbSign |= 0x02;
    }
    if iSadBlock[3] > iAverageSad {
        uiMbSign |= 0x01;
    }
    uiMbSign
}

pub unsafe extern "C" fn AnalysisVaaInfoIntra_c(pDataY: *mut u8, kiLineSize: i32) -> i32 {
    let mut uiAvgBlock = [0u16; 16];
    let mut pEncData = pDataY;
    let kiLineSize2 = kiLineSize << 1;
    let kiLineSize3 = kiLineSize + kiLineSize2;
    let kiLineSize4 = kiLineSize << 2;

    let mut blk_idx = 0usize;
    for _j in (0..16).step_by(4) {
        for i in (0..16).step_by(4) {
            let mut sum: u32 = *pEncData.add(i) as u32
                + *pEncData.add(i + 1) as u32
                + *pEncData.add(i + 2) as u32
                + *pEncData.add(i + 3) as u32;

            sum += *pEncData.offset(kiLineSize as isize + i as isize) as u32
                + *pEncData.offset(kiLineSize as isize + i as isize + 1) as u32
                + *pEncData.offset(kiLineSize as isize + i as isize + 2) as u32
                + *pEncData.offset(kiLineSize as isize + i as isize + 3) as u32;

            sum += *pEncData.offset(kiLineSize2 as isize + i as isize) as u32
                + *pEncData.offset(kiLineSize2 as isize + i as isize + 1) as u32
                + *pEncData.offset(kiLineSize2 as isize + i as isize + 2) as u32
                + *pEncData.offset(kiLineSize2 as isize + i as isize + 3) as u32;

            sum += *pEncData.offset(kiLineSize3 as isize + i as isize) as u32
                + *pEncData.offset(kiLineSize3 as isize + i as isize + 1) as u32
                + *pEncData.offset(kiLineSize3 as isize + i as isize + 2) as u32
                + *pEncData.offset(kiLineSize3 as isize + i as isize + 3) as u32;

            uiAvgBlock[blk_idx] = (sum >> 4) as u16;
            blk_idx += 1;
        }
        pEncData = pEncData.offset(kiLineSize4 as isize);
    }

    let mut iSumAvg: i32 = 0;
    let mut iSumSqr: i32 = 0;

    for i in (0..16).step_by(4) {
        let b0 = uiAvgBlock[i] as i32;
        let b1 = uiAvgBlock[i + 1] as i32;
        let b2 = uiAvgBlock[i + 2] as i32;
        let b3 = uiAvgBlock[i + 3] as i32;

        iSumAvg += b0 + b1 + b2 + b3;
        iSumSqr += b0 * b0 + b1 * b1 + b2 * b2 + b3 * b3;
    }

    iSumSqr - ((iSumAvg * iSumAvg) >> 4)
}

pub unsafe extern "C" fn InitIntraAnalysisVaaInfo(
    pFuncList: *mut SWelsFuncPtrList,
    _kuiCpuFlag: u32,
) {
    (*pFuncList).pfGetVarianceFromIntraVaa = Some(AnalysisVaaInfoIntra_c);
    (*pFuncList).pfGetMbSignFromInterVaa = Some(MdInterAnalysisVaaInfo_c);
    (*pFuncList).pfUpdateMbMv = Some(UpdateMbMv_c);
}

pub unsafe extern "C" fn MdIntraAnalysisVaaInfo(
    pEncCtx: *mut sWelsEncCtx,
    pEncMb: *mut u8,
) -> bool {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let kiLineSize = (*pCurDqLayer).iEncStride[0];
    let pfGetVariance = (*(*pEncCtx).pFuncList).pfGetVarianceFromIntraVaa.unwrap();
    let kiVariance = pfGetVariance(pEncMb, kiLineSize);
    kiVariance >= INTRA_VARIANCE_SAD_THRESHOLD
}

pub unsafe extern "C" fn InitMeRefinePointer(
    pMeRefine: *mut SMeRefinePointer,
    pMbCache: *mut SMbCache,
    iStride: i32,
) {
    (*pMeRefine).pHalfPixH = (*pMbCache).pBufferInterPredMe.add(iStride as usize);
    (*pMeRefine).pHalfPixV = (*pMbCache).pBufferInterPredMe.add(640 + iStride as usize);
    (*pMeRefine).pQuarPixBest = (*pMbCache).pBufferInterPredMe.add(1280 + iStride as usize);
    (*pMeRefine).pQuarPixTmp = (*pMbCache).pBufferInterPredMe.add(1920 + iStride as usize);
}

#[inline(always)]
pub unsafe fn MeRefineQuarPixel(
    pFunc: *mut SWelsFuncPtrList,
    pMe: *mut SWelsME,
    pMeRefine: *mut SMeRefinePointer,
    kiWidth: i32,
    kiHeight: i32,
    pParams: *mut SQuarRefineParams,
    iStrideEnc: i32,
) {
    let pEncMb = (*pMe).pEncMb;
    let kuiPixel = (*pMe).uiBlockSize as usize;
    let pfMeCost = (*pFunc).sSampleDealingFuncs.me_cost(kuiPixel).unwrap();

    // =========================(0, -1) [TOP] =========================
    PixelAvg_c(
        (*pMeRefine).pQuarPixTmp,
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcA[0],
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcB[0],
        (*pParams).iStrideA,
        kiWidth,
        kiHeight,
    );

    let mut iCurCost = pfMeCost(pEncMb, iStrideEnc, (*pMeRefine).pQuarPixTmp, ME_REFINE_BUF_STRIDE) + (*pParams).iLms[0];
    if iCurCost < (*pParams).iBestCost {
        (*pParams).iBestCost = iCurCost;
        (*pParams).iBestQuarPix = ME_QUAR_PIXEL_TOP;
        std::mem::swap(&mut (*pMeRefine).pQuarPixBest, &mut (*pMeRefine).pQuarPixTmp);
    }

    // =========================(0, 1) [BOTTOM] =======================
    PixelAvg_c(
        (*pMeRefine).pQuarPixTmp,
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcA[1],
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcB[1],
        (*pParams).iStrideA,
        kiWidth,
        kiHeight,
    );

    iCurCost = pfMeCost(pEncMb, iStrideEnc, (*pMeRefine).pQuarPixTmp, ME_REFINE_BUF_STRIDE) + (*pParams).iLms[1];
    if iCurCost < (*pParams).iBestCost {
        (*pParams).iBestCost = iCurCost;
        (*pParams).iBestQuarPix = ME_QUAR_PIXEL_BOTTOM;
        std::mem::swap(&mut (*pMeRefine).pQuarPixBest, &mut (*pMeRefine).pQuarPixTmp);
    }

    // =========================(-1, 0) [LEFT] ========================
    PixelAvg_c(
        (*pMeRefine).pQuarPixTmp,
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcA[2],
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcB[2],
        (*pParams).iStrideB,
        kiWidth,
        kiHeight,
    );

    iCurCost = pfMeCost(pEncMb, iStrideEnc, (*pMeRefine).pQuarPixTmp, ME_REFINE_BUF_STRIDE) + (*pParams).iLms[2];
    if iCurCost < (*pParams).iBestCost {
        (*pParams).iBestCost = iCurCost;
        (*pParams).iBestQuarPix = ME_QUAR_PIXEL_LEFT;
        std::mem::swap(&mut (*pMeRefine).pQuarPixBest, &mut (*pMeRefine).pQuarPixTmp);
    }

    // =========================(1, 0) [RIGHT] ========================
    PixelAvg_c(
        (*pMeRefine).pQuarPixTmp,
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcA[3],
        ME_REFINE_BUF_STRIDE,
        (*pParams).pSrcB[3],
        (*pParams).iStrideB,
        kiWidth,
        kiHeight,
    );

    iCurCost = pfMeCost(pEncMb, iStrideEnc, (*pMeRefine).pQuarPixTmp, ME_REFINE_BUF_STRIDE) + (*pParams).iLms[3];
    if iCurCost < (*pParams).iBestCost {
        (*pParams).iBestCost = iCurCost;
        (*pParams).iBestQuarPix = ME_QUAR_PIXEL_RIGHT;
        std::mem::swap(&mut (*pMeRefine).pQuarPixBest, &mut (*pMeRefine).pQuarPixTmp);
    }
}

pub unsafe extern "C" fn MeRefineFracPixel(
    pEncCtx: *mut sWelsEncCtx,
    pMemPredInterMb: *mut u8,
    pMe: *mut SWelsME,
    pMeRefine: *mut SMeRefinePointer,
    iWidth: i32,
    iHeight: i32,
) {
    let pFunc = (*pEncCtx).pFuncList;
    let iMvx = (*pMe).sMv.iMvX;
    let iMvy = (*pMe).sMv.iMvY;

    let mut iHalfMvx = iMvx;
    let mut iHalfMvy = iMvy;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let kiStrideEnc = (*pCurDqLayer).iEncStride[0];
    let kiStrideRef = (*(*pCurDqLayer).pRefPic).iLineSize[0];

    let pEncData = (*pMe).pEncMb;
    let pRef = (*pMe).pRefMb;

    let mut sParams = SQuarRefineParams {
        iBestCost: 0,
        iBestHalfPix: 0,
        iStrideA: 0,
        iStrideB: 0,
        pRef: std::ptr::null_mut(),
        pSrcB: [std::ptr::null_mut(); 4],
        pSrcA: [std::ptr::null_mut(); 4],
        iLms: [0; 4],
        iBestQuarPix: ME_NO_BEST_QUAR_PIXEL,
    };

    let iMvQuarAddX: [i32; 10] = [0, 0, -1, 1, 0, 0, 0, -1, 1, 0];
    let pMvQuarAddY = &iMvQuarAddX[3..];
    let mut pBestPredInter = pRef;
    let mut iInterBlk4Stride = ME_REFINE_BUF_STRIDE;

    let mut iBestCost: i32;
    let mut iCurCost: i32;
    let mut iBestHalfPix: i32;

    let pfMeCost = (*pFunc).sSampleDealingFuncs.me_cost((*pMe).uiBlockSize as usize).unwrap();

    if (*pCurDqLayer).bSatdInMdFlag {
        iBestCost = (*pMe).uSadPredISatd.uiSatd as i32
            + COST_MVD((*pMe).pMvdCost, (iMvx - (*pMe).sMvp.iMvX) as i32, (iMvy - (*pMe).sMvp.iMvY) as i32);
    } else {
        iBestCost = pfMeCost(pEncData, kiStrideEnc, pRef, kiStrideRef)
            + COST_MVD((*pMe).pMvdCost, (iMvx - (*pMe).sMvp.iMvX) as i32, (iMvy - (*pMe).sMvp.iMvY) as i32);
    }

    iBestHalfPix = REFINE_ME_NO_BEST_HALF_PIXEL;

    McHorVer02_c(
        pRef.offset(-(kiStrideRef as isize)),
        kiStrideRef,
        (*pMeRefine).pHalfPixV,
        ME_REFINE_BUF_STRIDE,
        iWidth,
        iHeight + 1,
    );

    // step 1: vertical filter
    // (0, -2) [TOP]
    iCurCost = pfMeCost(pEncData, kiStrideEnc, (*pMeRefine).pHalfPixV, ME_REFINE_BUF_STRIDE)
        + COST_MVD((*pMe).pMvdCost, (iMvx - (*pMe).sMvp.iMvX) as i32, (iMvy - 2 - (*pMe).sMvp.iMvY) as i32);
    if iCurCost < iBestCost {
        iBestCost = iCurCost;
        iBestHalfPix = REFINE_ME_HALF_PIXEL_TOP;
        pBestPredInter = (*pMeRefine).pHalfPixV;
    }

    // (0, 2) [BOTTOM]
    iCurCost = pfMeCost(
        pEncData,
        kiStrideEnc,
        (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize),
        ME_REFINE_BUF_STRIDE,
    ) + COST_MVD((*pMe).pMvdCost, (iMvx - (*pMe).sMvp.iMvX) as i32, (iMvy + 2 - (*pMe).sMvp.iMvY) as i32);
    if iCurCost < iBestCost {
        iBestCost = iCurCost;
        iBestHalfPix = REFINE_ME_HALF_PIXEL_BOTTOM;
        pBestPredInter = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
    }

    McHorVer20_c(
        pRef.offset(-1),
        kiStrideRef,
        (*pMeRefine).pHalfPixH,
        ME_REFINE_BUF_STRIDE,
        iWidth + 1,
        iHeight,
    );

    // step 2: horizontal filter
    // (-2, 0) [LEFT]
    iCurCost = pfMeCost(pEncData, kiStrideEnc, (*pMeRefine).pHalfPixH, ME_REFINE_BUF_STRIDE)
        + COST_MVD((*pMe).pMvdCost, (iMvx - 2 - (*pMe).sMvp.iMvX) as i32, (iMvy - (*pMe).sMvp.iMvY) as i32);
    if iCurCost < iBestCost {
        iBestCost = iCurCost;
        iBestHalfPix = REFINE_ME_HALF_PIXEL_LEFT;
        pBestPredInter = (*pMeRefine).pHalfPixH;
    }

    // (2, 0) [RIGHT]
    iCurCost = pfMeCost(
        pEncData,
        kiStrideEnc,
        (*pMeRefine).pHalfPixH.add(1),
        ME_REFINE_BUF_STRIDE,
    ) + COST_MVD((*pMe).pMvdCost, (iMvx + 2 - (*pMe).sMvp.iMvX) as i32, (iMvy - (*pMe).sMvp.iMvY) as i32);
    if iCurCost < iBestCost {
        iBestCost = iCurCost;
        iBestHalfPix = REFINE_ME_HALF_PIXEL_RIGHT;
        pBestPredInter = (*pMeRefine).pHalfPixH.add(1);
    }

    sParams.iBestCost = iBestCost;
    sParams.iBestHalfPix = iBestHalfPix;
    sParams.pRef = pRef;
    sParams.iBestQuarPix = ME_NO_BEST_QUAR_PIXEL;


    if REFINE_ME_NO_BEST_HALF_PIXEL == iBestHalfPix {
        sParams.iStrideA = kiStrideRef;
        sParams.iStrideB = kiStrideRef;
        sParams.pSrcA[0] = (*pMeRefine).pHalfPixV;
        sParams.pSrcA[1] = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
        sParams.pSrcA[2] = (*pMeRefine).pHalfPixH;
        sParams.pSrcA[3] = (*pMeRefine).pHalfPixH.add(1);

        sParams.pSrcB[0] = pRef;
        sParams.pSrcB[1] = pRef;
        sParams.pSrcB[2] = pRef;
        sParams.pSrcB[3] = pRef;

        sParams.iLms[0] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - 1 - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[1] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - (*pMe).sMvp.iMvX) as i32, (iHalfMvy + 1 - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[2] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - 1 - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[3] = COST_MVD((*pMe).pMvdCost, (iHalfMvx + 1 - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - (*pMe).sMvp.iMvY) as i32);
    } else {
        match iBestHalfPix {
            REFINE_ME_HALF_PIXEL_LEFT => {
                (*pMeRefine).pHalfPixHV = (*pMeRefine).pHalfPixV;
                McHorVer22_c(
                    pRef.offset(-1 - kiStrideRef as isize),
                    kiStrideRef,
                    (*pMeRefine).pHalfPixHV,
                    ME_REFINE_BUF_STRIDE,
                    iWidth + 1,
                    iHeight + 1,
                );

                iHalfMvx -= 2;
                sParams.iStrideA = ME_REFINE_BUF_STRIDE;
                sParams.iStrideB = kiStrideRef;
                sParams.pSrcA[0] = (*pMeRefine).pHalfPixH;
                sParams.pSrcA[1] = (*pMeRefine).pHalfPixH;
                sParams.pSrcA[2] = (*pMeRefine).pHalfPixH;
                sParams.pSrcA[3] = (*pMeRefine).pHalfPixH;
                sParams.pSrcB[0] = (*pMeRefine).pHalfPixHV;
                sParams.pSrcB[1] = (*pMeRefine).pHalfPixHV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcB[2] = pRef.offset(-1);
                sParams.pSrcB[3] = pRef;
            }
            REFINE_ME_HALF_PIXEL_RIGHT => {
                (*pMeRefine).pHalfPixHV = (*pMeRefine).pHalfPixV;
                McHorVer22_c(
                    pRef.offset(-1 - kiStrideRef as isize),
                    kiStrideRef,
                    (*pMeRefine).pHalfPixHV,
                    ME_REFINE_BUF_STRIDE,
                    iWidth + 1,
                    iHeight + 1,
                );

                iHalfMvx += 2;
                sParams.iStrideA = ME_REFINE_BUF_STRIDE;
                sParams.iStrideB = kiStrideRef;
                sParams.pSrcA[0] = (*pMeRefine).pHalfPixH.add(1);
                sParams.pSrcA[1] = (*pMeRefine).pHalfPixH.add(1);
                sParams.pSrcA[2] = (*pMeRefine).pHalfPixH.add(1);
                sParams.pSrcA[3] = (*pMeRefine).pHalfPixH.add(1);
                sParams.pSrcB[0] = (*pMeRefine).pHalfPixHV.add(1);
                sParams.pSrcB[1] = (*pMeRefine).pHalfPixHV.add(1 + ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcB[2] = pRef;
                sParams.pSrcB[3] = pRef.add(1);
            }
            REFINE_ME_HALF_PIXEL_TOP => {
                (*pMeRefine).pHalfPixHV = (*pMeRefine).pHalfPixH;
                McHorVer22_c(
                    pRef.offset(-1 - kiStrideRef as isize),
                    kiStrideRef,
                    (*pMeRefine).pHalfPixHV,
                    ME_REFINE_BUF_STRIDE,
                    iWidth + 1,
                    iHeight + 1,
                );

                iHalfMvy -= 2;
                sParams.iStrideA = kiStrideRef;
                sParams.iStrideB = ME_REFINE_BUF_STRIDE;
                sParams.pSrcA[0] = (*pMeRefine).pHalfPixV;
                sParams.pSrcA[1] = (*pMeRefine).pHalfPixV;
                sParams.pSrcA[2] = (*pMeRefine).pHalfPixV;
                sParams.pSrcA[3] = (*pMeRefine).pHalfPixV;
                sParams.pSrcB[0] = pRef.offset(-(kiStrideRef as isize));
                sParams.pSrcB[1] = pRef;
                sParams.pSrcB[2] = (*pMeRefine).pHalfPixHV;
                sParams.pSrcB[3] = (*pMeRefine).pHalfPixHV.add(1);
            }
            REFINE_ME_HALF_PIXEL_BOTTOM => {
                (*pMeRefine).pHalfPixHV = (*pMeRefine).pHalfPixH;
                McHorVer22_c(
                    pRef.offset(-1 - kiStrideRef as isize),
                    kiStrideRef,
                    (*pMeRefine).pHalfPixHV,
                    ME_REFINE_BUF_STRIDE,
                    iWidth + 1,
                    iHeight + 1,
                );

                iHalfMvy += 2;
                sParams.iStrideA = kiStrideRef;
                sParams.iStrideB = ME_REFINE_BUF_STRIDE;
                sParams.pSrcA[0] = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcA[1] = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcA[2] = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcA[3] = (*pMeRefine).pHalfPixV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcB[0] = pRef;
                sParams.pSrcB[1] = pRef.offset(kiStrideRef as isize);
                sParams.pSrcB[2] = (*pMeRefine).pHalfPixHV.add(ME_REFINE_BUF_STRIDE as usize);
                sParams.pSrcB[3] = (*pMeRefine).pHalfPixHV.add(ME_REFINE_BUF_STRIDE as usize + 1);
            }
            _ => {}
        }

        sParams.iLms[0] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - 1 - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[1] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - (*pMe).sMvp.iMvX) as i32, (iHalfMvy + 1 - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[2] = COST_MVD((*pMe).pMvdCost, (iHalfMvx - 1 - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - (*pMe).sMvp.iMvY) as i32);
        sParams.iLms[3] = COST_MVD((*pMe).pMvdCost, (iHalfMvx + 1 - (*pMe).sMvp.iMvX) as i32, (iHalfMvy - (*pMe).sMvp.iMvY) as i32);
    }

    MeRefineQuarPixel(pFunc, pMe, pMeRefine, iWidth, iHeight, &mut sParams, kiStrideEnc);

    if iBestCost > sParams.iBestCost {
        pBestPredInter = (*pMeRefine).pQuarPixBest;
        iBestCost = sParams.iBestCost;
    }
    let iBestQuarPix = sParams.iBestQuarPix;

    (*pMe).sMv.iMvX = iHalfMvx + iMvQuarAddX[iBestQuarPix as usize] as i16;
    (*pMe).sMv.iMvY = iHalfMvy + pMvQuarAddY[iBestQuarPix as usize] as i16;
    (*pMe).uiSatdCost = iBestCost as u32;

    if iBestHalfPix + iBestQuarPix == NO_BEST_FRAC_PIX {
        pBestPredInter = pRef;
        iInterBlk4Stride = kiStrideRef;
    }

    let pfCopyBlockByMode = (*pMeRefine).pfCopyBlockByMode.unwrap();
    pfCopyBlockByMode(pMemPredInterMb, MB_WIDTH_LUMA, pBestPredInter, iInterBlk4Stride);
}

pub unsafe extern "C" fn InitBlkStrideWithRef(pBlkStride: *mut i32, kiStrideRef: i32) {
    const KUI_STRIDE_X: [u8; 16] = [
        0, 4, 0, 4,
        8, 12, 8, 12,
        0, 4, 0, 4,
        8, 12, 8, 12,
    ];
    const KUI_STRIDE_Y: [u8; 16] = [
        0, 0, 4, 4,
        0, 0, 4, 4,
        8, 8, 12, 12,
        8, 8, 12, 12,
    ];

    for i in (0..16).step_by(4) {
        *pBlkStride.add(i) = KUI_STRIDE_X[i] as i32 + KUI_STRIDE_Y[i] as i32 * kiStrideRef;
        *pBlkStride.add(i + 1) = KUI_STRIDE_X[i + 1] as i32 + KUI_STRIDE_Y[i + 1] as i32 * kiStrideRef;
        *pBlkStride.add(i + 2) = KUI_STRIDE_X[i + 2] as i32 + KUI_STRIDE_Y[i + 2] as i32 * kiStrideRef;
        *pBlkStride.add(i + 3) = KUI_STRIDE_X[i + 3] as i32 + KUI_STRIDE_Y[i + 3] as i32 * kiStrideRef;
    }
}

pub unsafe extern "C" fn MvdCostInit(pMvdCostInter: *mut u16, kiMvdSz: i32) {
    let kiSz = kiMvdSz >> 1;
    let mut pNegMvd = pMvdCostInter;
    let mut pPosMvd = pMvdCostInter.offset((kiSz + 1) as isize);
    let kpQpLambda = g_kiQpCostTable.as_ptr();

    for i in 0..52 {
        let kiLambda = *kpQpLambda.add(i) as u16;
        let mut iNegSe = -kiSz;
        let mut iPosSe = 1i32;

        let mut j = 0;
        while j < kiSz {
            *pNegMvd = kiLambda.wrapping_mul(BsSizeSE(iNegSe) as u16);
            pNegMvd = pNegMvd.add(1);
            iNegSe += 1;

            *pNegMvd = kiLambda.wrapping_mul(BsSizeSE(iNegSe) as u16);
            pNegMvd = pNegMvd.add(1);
            iNegSe += 1;

            *pNegMvd = kiLambda.wrapping_mul(BsSizeSE(iNegSe) as u16);
            pNegMvd = pNegMvd.add(1);
            iNegSe += 1;

            *pNegMvd = kiLambda.wrapping_mul(BsSizeSE(iNegSe) as u16);
            pNegMvd = pNegMvd.add(1);
            iNegSe += 1;

            *pPosMvd = kiLambda.wrapping_mul(BsSizeSE(iPosSe) as u16);
            pPosMvd = pPosMvd.add(1);
            iPosSe += 1;

            *pPosMvd = kiLambda.wrapping_mul(BsSizeSE(iPosSe) as u16);
            pPosMvd = pPosMvd.add(1);
            iPosSe += 1;

            *pPosMvd = kiLambda.wrapping_mul(BsSizeSE(iPosSe) as u16);
            pPosMvd = pPosMvd.add(1);
            iPosSe += 1;

            *pPosMvd = kiLambda.wrapping_mul(BsSizeSE(iPosSe) as u16);
            pPosMvd = pPosMvd.add(1);
            iPosSe += 1;

            j += 4;
        }

        *pNegMvd = kiLambda;
        pNegMvd = pNegMvd.offset((kiSz + 1) as isize);
        pPosMvd = pPosMvd.offset((kiSz + 1) as isize);
    }
}

pub unsafe extern "C" fn PredictSad(
    pRefIndexCache: *mut i8,
    pSadCostCache: *mut i32,
    uiRef: i32,
    pSadPred: *mut i32,
) {
    let kiRefB = *pRefIndexCache.add(1) as i32;
    let mut iRefC = *pRefIndexCache.add(5) as i32;
    let kiRefA = *pRefIndexCache.add(6) as i32;
    let kiSadB = *pSadCostCache.add(1);
    let mut iSadC = *pSadCostCache.add(2);
    let kiSadA = *pSadCostCache.add(3);

    if iRefC == REF_NOT_AVAIL as i32 {
        iRefC = *pRefIndexCache.add(0) as i32;
        iSadC = *pSadCostCache.add(0);
    }

    if kiRefB == REF_NOT_AVAIL as i32 && iRefC == REF_NOT_AVAIL as i32 && kiRefA != REF_NOT_AVAIL as i32 {
        *pSadPred = kiSadA;
    } else {
        let mut iCount = ((uiRef == kiRefA) as i32) << MB_LEFT_BIT;
        iCount |= ((uiRef == kiRefB) as i32) << MB_TOP_BIT;
        iCount |= ((uiRef == iRefC) as i32) << MB_TOPRIGHT_BIT;
        match iCount as u32 {
            LEFT_MB_POS => {
                *pSadPred = kiSadA;
            }
            TOP_MB_POS => {
                *pSadPred = kiSadB;
            }
            TOPRIGHT_MB_POS => {
                *pSadPred = iSadC;
            }
            _ => {
                *pSadPred = WelsMedian(kiSadA, kiSadB, iSadC);
            }
        }
    }

    let iCount = (*pSadPred) << 6;
    *pSadPred = (REPLACE_SAD_MULTIPLY(iCount) + 32) >> 6;
}

pub unsafe extern "C" fn PredictSadSkip(
    pRefIndexCache: *mut i8,
    pMbSkipCache: *mut bool,
    pSadCostCache: *mut i32,
    uiRef: i32,
    iSadPredSkip: *mut i32,
) {
    let kiRefB = *pRefIndexCache.add(1) as i32;
    let mut iRefC = *pRefIndexCache.add(5) as i32;
    let kiRefA = *pRefIndexCache.add(6) as i32;
    let kiSadB = if *pMbSkipCache.add(1) { *pSadCostCache.add(1) } else { 0 };
    let mut iSadC = if *pMbSkipCache.add(2) { *pSadCostCache.add(2) } else { 0 };
    let kiSadA = if *pMbSkipCache.add(3) { *pSadCostCache.add(3) } else { 0 };
    let mut iRefSkip = *pMbSkipCache.add(2);

    if iRefC == REF_NOT_AVAIL as i32 {
        iRefC = *pRefIndexCache.add(0) as i32;
        iSadC = if *pMbSkipCache.add(0) { *pSadCostCache.add(0) } else { 0 };
        iRefSkip = *pMbSkipCache.add(0);
    }

    if kiRefB == REF_NOT_AVAIL as i32 && iRefC == REF_NOT_AVAIL as i32 && kiRefA != REF_NOT_AVAIL as i32 {
        *iSadPredSkip = kiSadA;
    } else {
        let mut iCount = (((uiRef == kiRefA) && *pMbSkipCache.add(3)) as i32) << MB_LEFT_BIT;
        iCount |= (((uiRef == kiRefB) && *pMbSkipCache.add(1)) as i32) << MB_TOP_BIT;
        iCount |= (((uiRef == iRefC) && iRefSkip) as i32) << MB_TOPRIGHT_BIT;
        match iCount as u32 {
            LEFT_MB_POS => {
                *iSadPredSkip = kiSadA;
            }
            TOP_MB_POS => {
                *iSadPredSkip = kiSadB;
            }
            TOPRIGHT_MB_POS => {
                *iSadPredSkip = iSadC;
            }
            _ => {
                *iSadPredSkip = WelsMedian(kiSadA, kiSadB, iSadC);
            }
        }
    }
}

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_SSE2, WELS_CPU_SSE41, WELS_CPU_SSSE3};
