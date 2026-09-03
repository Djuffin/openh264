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

//! # Motion Estimation (ME) Engine
//!
//! Translated from `codec/encoder/core/inc/svc_motion_estimate.h` and
//! `codec/encoder/core/src/svc_motion_estimate.cpp`.
//!
//! Implements multi-candidate initial point testing, small diamond search (`ME_DIA`),
//! 1D orthogonal cross line full search (`ME_CROSS`), and hash-based feature search (`ME_FME`)
//! for screen content coding and real-time H.264 / AVC video encoding.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    clippy::too_many_arguments
)]

#![forbid(unsafe_code)]

use crate::encoder::rec_view::SharedPlane;
use crate::encoder::rec_view::RecCursor;
use crate::safe::plane::{PaddedPlane, PlaneCursor};
use crate::safe::mvd_cost::MvdCostCursor;
pub use crate::encoder::encoder_context::SMVUnitXY;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::picture::SScreenBlockFeatureStorage;
pub use crate::encoder::md::SSampleDealingFunc;
pub use crate::encoder::slice_multi_threading::SSliceCtx;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

// ============================================================================
// Constants, Limits, and Enums
// ============================================================================

pub const CAMERA_STARTMV_RANGE: i32 = 64;
pub const ITERATIVE_TIMES: i32 = 16;
pub const CAMERA_MV_RANGE: i32 = CAMERA_STARTMV_RANGE + ITERATIVE_TIMES; // 80
pub const CAMERA_MVD_RANGE: i32 = (CAMERA_MV_RANGE + 1) << 1; // 162
pub const MB_WIDTH_LUMA: i32 = 16;
pub const BASE_MV_MB_NMB: i32 = (2 * CAMERA_MV_RANGE / MB_WIDTH_LUMA) - 1; // 9
pub const CAMERA_HIGHLAYER_MVD_RANGE: i32 = 243;
pub const EXPANDED_MV_RANGE: i32 = 504;
pub const EXPANDED_MVD_RANGE: i32 = (504 + 1) << 1; // 1010
pub const INTPEL_NEEDED_MARGIN: i32 = 3;
pub const MAX_VERTICAL_MV_RANGE: i32 = 1024;

// Motion Search Method Bitmask Flags
pub const ME_DIA: u32 = 0x01;
pub const ME_CROSS: u32 = 0x02;
pub const ME_FME: u32 = 0x04;
pub const ME_FULL: u32 = 0x10;
pub const ME_DIA_CROSS: u32 = ME_DIA | ME_CROSS; // 0x03
pub const ME_DIA_CROSS_FME: u32 = ME_DIA_CROSS | ME_FME; // 0x07

// Feature Search Hash and Threshold Constants
pub const LIST_SIZE_SUM_16x16: usize = 0x0FF01; // 65281
pub const LIST_SIZE_SUM_8x8: usize = 0x03FC1; // 16321
pub const LIST_SIZE_MSE_16x16: usize = 0x00878; // 2168
pub const LIST_SIZE: i32 = 0x10000; // 65536

pub const FME_DEFAULT_FEATURE_INDEX: i32 = 0;
pub const FMESWITCH_DEFAULT_GOODFRAME_NUM: u8 = 2;
pub const FMESWITCH_MBSAD_THRESHOLD: i32 = 30;
pub const FMESWITCH_MBAVERCOSTSAVING_THRESHOLD: u32 = 2;
pub const FMESWITCH_GOODFRAMECOUNT_MAX: u8 = 5;

// Block Sizes
pub const BLOCK_16x16: usize = 0;
pub const BLOCK_16x8: usize = 1;
pub const BLOCK_8x16: usize = 2;
pub const BLOCK_8x8: usize = 3;
pub const BLOCK_4x4: usize = 4;
pub const BLOCK_8x4: usize = 5;
pub const BLOCK_4x8: usize = 6;
pub const BLOCK_SIZE_ALL: usize = 7;

// Return Codes
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNSUPPORTED_PARA: i32 = 0x02;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;

// CPU Capability Bit Flags

/// Quantization Step Lookup Table ($16 \times Q_{\text{step}}$ for $\text{QP} \in [0, 51]$)
pub static QStepx16ByQp: [i32; 52] = [
    10, 11, 13, 14, 16, 18,
    20, 22, 26, 28, 32, 36,
    40, 44, 52, 56, 64, 72,
    80, 88, 104, 112, 128, 144,
    160, 176, 208, 224, 256, 288,
    320, 352, 416, 448, 512, 576,
    640, 704, 832, 896, 1024, 1152,
    1280, 1408, 1664, 1792, 2048, 2304,
    2560, 2816, 3328, 3584,
];

// ============================================================================
// Core Data Structures
// ============================================================================

/// 2D Motion Vector displacement in integer or 1/4-pel units.

/// Dual-use scalar storing the predicted-SAD threshold before the search and
/// the SATD after it.
///
/// **S11.28: was `union { uiSadPred: u32, uiSatd: u32 }`** — two names for the
/// same 32 bits, so every access was an unsafe union read of whatever the other
/// name last stored, and the `unsafe` said nothing a `u32` read does not. One
/// field at the same offset, size and alignment (`repr(C)` both ways); the
/// phase naming the C++ union carried lives here instead: writers before the
/// search store the SAD prediction, `CalculateSatdCost` overwrites it with the
/// SATD, and each reader knows its phase exactly as it had to before.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SadPredISatdUnit {
    pub uiValue: u32,
}

/// Reference frame screen block feature storage and hash lookup index.

/// Central working state structure passed across all motion estimation search routines.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsME<'a> {
    /// **S5.C4b**: was `*mut u16` into `pCtx->pMvdCostTable`. The lifetime this
    /// introduces is the whole reason the struct has one: see
    /// [`MvdCostCursor`](crate::safe::mvd_cost::MvdCostCursor) for why the biased
    /// pointer cannot be a plain slice, and `SWelsMD` for how far the parameter
    /// travels (three structs, all of them slice-encode locals — it reaches no
    /// context field).
    pub pMvdCost: MvdCostCursor<'a>,
    pub uSadPredISatd: SadPredISatdUnit,
    pub uiSadCost: u32,
    pub uiSatdCost: u32,
    pub uiSadCostThreshold: u32,
    pub iCurMeBlockPixX: i32,
    pub iCurMeBlockPixY: i32,
    pub uiBlockSize: u8,
    pub uiReserved: u8,

    // `pEncMb`/`pRefMb`/`pColoRefMb` stood here — the C++'s three per-block
    // plane cursors. Session F deleted them on B3's coordinate identity,
    // re-verified at this tree (583/583 rows, ~3.09M search entries/exits
    // asserted, zero violations): `pEncMb == encRoot + (y*strideEnc + x)`,
    // `pColoRefMb == refRoot + (y*strideRef + x)`, and `pRefMb == colo + mv`
    // at every read. The search family takes the two planes as parameters and
    // derives each position from `iCurMeBlockPixX/Y` + the MV it is probing —
    // S37's value half: the *coordinates* are the information, and no retag
    // can invalidate them.

    pub sMvp: SMVUnitXY,
    pub sMvBase: SMVUnitXY,
    pub sDirectionalMv: SMVUnitXY,

    /// **S5.C6d**: was `*mut SScreenBlockFeatureStorage`. It is resolved from
    /// `SPicture::pScreenBlockFeatureStorage`, which C6c made an `Option<Box<..>>` that
    /// nothing ever fills, so this is `None` on every path the port can take. The
    /// lifetime is the one `pMvdCost` already introduced (C4b) — a search block borrows
    /// the frame's tables for the slice encode and no longer.
    // SCREEN_CONTENT(dormant: Phase 10)
    pub pRefFeatureStorage: Option<&'a SScreenBlockFeatureStorage>,

    pub sMv: SMVUnitXY,
}

impl Default for SWelsME<'_> {
    fn default() -> Self {
        Self {
            pMvdCost: MvdCostCursor::none(),
            uSadPredISatd: SadPredISatdUnit::default(),
            uiSadCost: 0,
            uiSatdCost: 0,
            uiSadCostThreshold: 0,
            iCurMeBlockPixX: 0,
            iCurMeBlockPixY: 0,
            uiBlockSize: 0,
            uiReserved: 0,
            sMvp: SMVUnitXY::default(),
            sMvBase: SMVUnitXY::default(),
            sDirectionalMv: SMVUnitXY::default(),
            pRefFeatureStorage: None,
            sMv: SMVUnitXY::default(),
        }
    }
}

/// Input configuration block for the hash-based feature search engine.
///
/// Session F: `pSad` holds the safe cost slot, and the two block cursors
/// (`pEnc`/`pColoRef`, with their strides) became the two planes plus the
/// `iCurPixX/Y` coordinates the struct already carried — the same identity
/// the search family converted on. The mvd-cost and hash cursors stay raw
/// (the ctx family's and Phase 10's respectively).
// SCREEN_CONTENT(dormant: Phase 10)
pub struct SFeatureSearchIn<'a> {
    pub pSad: Option<PSampleSadSatdCostFunc>,
    /// **S6.B1**: the storage's three read-side buffers, borrowed rather than
    /// pointed at. `pQpelLocationOfFeature` held per-value *addresses* into the arena;
    /// it holds per-value offsets now, so the arena itself has to travel with it —
    /// hence `pLocationPointer`, which the pointer spelling did not need to name.
    pub pTimesOfFeature: &'a [u32],
    pub pQpelLocationOfFeature: &'a [usize],
    pub pLocationPointer: &'a [u16],
    pub pMvdCostX: MvdCostCursor<'a>,
    pub pMvdCostY: MvdCostCursor<'a>,
    pub pEncPlane: Option<&'a SharedPlane>,
    pub pRefPlane: Option<&'a SharedPlane>,
    pub uiSadCostThresh: u16,
    pub iFeatureOfCurrent: i32,
    pub iCurPixX: i32,
    pub iCurPixY: i32,
    pub iCurPixXQpel: i32,
    pub iCurPixYQpel: i32,
    pub iMinQpelX: i32,
    pub iMinQpelY: i32,
    pub iMaxQpelX: i32,
    pub iMaxQpelY: i32,
}

impl Default for SFeatureSearchIn<'_> {
    fn default() -> Self {
        Self {
            pSad: None,
            // Empty, where the pointers were null — and the guard at the end of
            // `SetFeatureSearchIn` asks `is_empty()` for what it asked `is_null()`.
            pTimesOfFeature: &[],
            pQpelLocationOfFeature: &[],
            pLocationPointer: &[],
            pMvdCostX: MvdCostCursor::none(),
            pMvdCostY: MvdCostCursor::none(),
            pEncPlane: None,
            pRefPlane: None,
            uiSadCostThresh: 0,
            iFeatureOfCurrent: 0,
            iCurPixX: 0,
            iCurPixY: 0,
            iCurPixXQpel: 0,
            iCurPixYQpel: 0,
            iMinQpelX: 0,
            iMinQpelY: 0,
            iMaxQpelX: 0,
            iMaxQpelY: 0,
        }
    }
}

/// Output container populated during feature search passes.
///
/// Session F: `pBestRef: *mut u8` is deleted — it cached `colo + sBestMv`,
/// which is `sBestMv`'s information (the identity the whole family converted
/// on), and its one consumer wrote it into the deleted `SWelsME::pRefMb`.
// SCREEN_CONTENT(dormant: Phase 10)
#[derive(Copy, Clone, Default)]
pub struct SFeatureSearchOut {
    pub sBestMv: SMVUnitXY,
    pub uiBestSadCost: u32,
}

/// Reconstructed or reference picture frame buffer descriptor.

/// Slice context parameters for motion estimation.


/// Slice context container.

/// Spatial dependency layer representation in Scalable Video Coding.


// ============================================================================
// Function Pointer Types
// ============================================================================

// `PSampleSadSatdCostFunc` was declared here a second time (`md.rs` is the other),
// and `census_allowlist.txt` carried the pair as `alias PSampleSadSatdCostFunc x2`.
// **T9.B25**: one declaration, in `md.rs`, re-exported here — the slot type is
// safe now and a second spelling of it is exactly the divergence the census exists
// to catch (the allowlist entry retires with the duplicate). The `*Raw` alias
// went with the transitional triple (session F).
pub use crate::encoder::md::PSampleSadSatdCostFunc;

/// `PSample4SadCostFunc` — the four-candidate SAD the diamond search steps with:
/// `sample1`'s block against `sample2`'s at each whole-sample neighbour, written to
/// `sad[0..4]` in the order **up, down, left, right**
/// (`common/sad_common.rs::sample_sad_four::<W, H>`).
///
/// Safe since T9.B25; see [`PSampleSadSatdCostFunc`] for the rule and
/// [`PSample4SadCostFuncRaw`] for the shape this replaces.
pub type PSample4SadCostFunc = fn(&RecCursor<'_>, &RecCursor<'_>, &mut [i32; 4]);

// `PSample4SadCostFuncRaw` — the transitional raw four-candidate shape — went
// with the raw triple (session F).

// `PSampleSadHor8Func` stood here — it typed `pfSampleSadHor8`, the
// screen-content SIMD horizontal-SAD pair, which had zero writers and zero
// readers in the whole tree. Both deleted, S18 (session F step 0).

// **Session F — the five self-referential typedefs de-virtualized** (Phase
// 4a's `pfMdCost` move, applied to the whole family). Each used to take
// `*mut SWelsFuncPtrList` — the table handed back into its own callees so
// they could reach the cost slots and the sub-search slots. The callees now
// take exactly what they reach: `&SMeFuncs` (the sub-search group) and/or
// `&SSampleDealingFunc` (the safe cost tables), plus the two planes the
// deleted `SWelsME` cursors pointed into. Strides travel inside the planes.
// The layer parameter died with the strides: it was read for nothing else.
//
// All remain `unsafe fn` — every body still walks `pMe.pMvdCost`, the raw
// MVD-cost cursor (the ctx family's, G–H) — but nothing in the signatures is
// raw, and under MT every reference parameter is a shared read of pre-fork
// state (the table is written only by `PreprocessSliceCoding`, before the
// fork — F132 round 7's hoist is what makes the `&` lawful).

pub type PMotionSearchFunc = fn(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
);

pub type PSearchMethodFunc = fn(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
);

pub type PCalculateSatdFunc = fn(
    pSatd: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME<'_>,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
);

pub type PCheckDirectionalMv = fn(
    pSad: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME<'_>,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
    iBestSadCost: &mut i32,
) -> bool;

pub type PLineFullSearchFunc = fn(
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pMvdTable: MvdCostCursor<'_>,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
    kiMinMv: i16,
    kiMaxMv: i16,
    bVerticalSearch: bool,
);

/// **S6.B1**: safe, and `pBuf` is gone with the pointers. It was the arena's base
/// address, which every entry of the two tables was an offset from; the tables hold
/// those offsets directly now, so the base is the constant 0 and needs no parameter.
pub type PInitializeHashforFeatureFunc = fn(
    pTimesOfFeatureValue: &[u32],
    kiListSize: i32,
    pLocationOfFeature: &mut [usize],
    pFeatureValuePointerList: &mut [usize],
);

/// **S6.B1**: safe, and the arena arrives as its own slice — the cursors used to be
/// pointers into it, so it did not need naming; they are indices now, so it does.
pub type PFillQpelLocationByFeatureValueFunc = fn(
    pFeatureOfBlock: &[u16],
    kiWidth: i32,
    kiHeight: i32,
    pLocationPointer: &mut [u16],
    pFeatureValuePointerList: &mut [usize],
);

/// **S6.B1**: the two storage buffers are slices; `pRef` stays raw, because it is a
/// *plane* root and the plane family is not this checkpoint's.
pub type PCalculateBlockFeatureOfFrame = fn(
    kpRef: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
);

/// Session F: the slot's one reader is `SetFeatureSearchIn`, whose block
/// origin is a plane cursor now — the raw `SumOf*SingleBlock_c` kernels stay
/// for the frame-feature builders, which walk a whole raw plane, and the slot
/// holds the safe per-block twins below.
pub type PCalculateSingleBlockFeature = fn(cRef: &RecCursor<'_>) -> i32;

/// **D-scc-13**: the layer arrives exclusively, and `Option` is gone from the
/// parameter. `UpdateFMESwitch` writes `uiFMEGoodFrameCount` on the layer's
/// `pFeatureSearchPreparation` — the C++'s `SDqLayer*` is not const — so the
/// slot's shared borrow could not carry the real body. The **table's** wrapper
/// stays (`SWelsFuncPtrList::pfUpdateFMESwitch: Option<PUpdateFMESwitch>`): that
/// one says "unset", which is a different question. The slot's one call site is
/// after the join (`WelsEncoderEncodeExt`, "update scc related"), where a
/// `&mut sWelsEncCtx` exists again, so nothing fork-reachable is retyped here.
pub type PUpdateFMESwitch = fn(pCurLayer: &mut SDqLayer);

/// The motion-estimation dispatch group — every slot the search family reaches
/// *through the table it used to be handed back* (session F, the Phase 4a
/// de-virtualization move applied to the five self-referential typedefs).
///
/// The C++ hands each search function `SWelsFuncPtrList*` so it can reach
/// these five surfaces plus `sSampleDealingFuncs`; the port groups the five
/// here and passes `&SMeFuncs` + `&SSampleDealingFunc` instead, so the table
/// parameter dies and the typedefs stop naming the struct that contains them.
/// Membership is the measured reach set: `pfSearchMethod` (the top-level
/// search's per-block dispatch), `pfCalculateSatd` (fast/normal per frame),
/// `pfCheckDirectionalMv` (screen-content arm), the two line-search slots and
/// `pfCalculateSingleBlockFeature` (both FME, dormant). `pfMotionSearch`
/// stays in the table proper: its readers are the mode-decision callers,
/// which hold the whole table lawfully.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SMeFuncs {
    pub pfSearchMethod: [Option<PSearchMethodFunc>; BLOCK_SIZE_ALL],
    pub pfCalculateSatd: Option<PCalculateSatdFunc>,
    pub pfCheckDirectionalMv: Option<PCheckDirectionalMv>,
    // SCREEN_CONTENT(dormant: Phase 10) — the cross/feature-search half.
    pub pfVerticalFullSearch: Option<PLineFullSearchFunc>,
    pub pfHorizontalFullSearch: Option<PLineFullSearchFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateSingleBlockFeature: [Option<PCalculateSingleBlockFeature>; 2],
}

impl Default for SMeFuncs {
    fn default() -> Self {
        Self {
            pfSearchMethod: [None; BLOCK_SIZE_ALL],
            pfCalculateSatd: None,
            pfCheckDirectionalMv: None,
            pfVerticalFullSearch: None,
            pfHorizontalFullSearch: None,
            pfCalculateSingleBlockFeature: [None; 2],
        }
    }
}





// ============================================================================
// Helper Macros and Inline Functions
// ============================================================================

/// Calculates MVD rate cost: `table[mx] + table[my]`.
///
/// **S5.C4b**: safe. The `*const u16` this took was the encoder's biased cursor
/// into `pMvdCostTable`; it is a [`MvdCostCursor`] now, and with it every body in
/// the search family that was `unsafe` *only* because of this call.
#[inline(always)]
pub fn COST_MVD(table: MvdCostCursor<'_>, mx: i32, my: i32) -> u32 {
    (table.at(mx) as u32) + (table.at(my) as u32)
}

/// Session F: the `pRef` argument and the `pRefMb` store are gone — the
/// pointer was `colo + ksBestMv` at every call site (the verified identity),
/// so the MV alone carries the result.
#[inline]
pub fn UpdateMeResults(ksBestMv: SMVUnitXY, kiBestSadCost: u32, pMe: &mut SWelsME<'_>) {
    pMe.sMv = ksBestMv;
    pMe.uiSadCost = kiBestSadCost;
}

#[inline]
pub fn MeEndIntepelSearch(pMe: &mut SWelsME<'_>) {
    {
        (*pMe).sMv.iMvX *= 1 << 2;
        (*pMe).sMv.iMvY *= 1 << 2;
        (*pMe).uiSatdCost = (*pMe).uiSadCost;
    }
}

#[inline]
pub fn CheckInRangeCloseOpen(iVal: i16, iMin: i16, iMax: i16) -> bool {
    iVal >= iMin && iVal < iMax
}

#[inline]
pub fn CheckMvInRange(ksCurrentMv: SMVUnitXY, ksMinMv: SMVUnitXY, ksMaxMv: SMVUnitXY) -> bool {
    CheckInRangeCloseOpen(ksCurrentMv.iMvX, ksMinMv.iMvX, ksMaxMv.iMvX)
        && CheckInRangeCloseOpen(ksCurrentMv.iMvY, ksMinMv.iMvY, ksMaxMv.iMvY)
}

#[inline]
pub fn SetMvWithinIntegerMvRange(
    kiMbWidth: i32,
    kiMbHeight: i32,
    kiMbX: i32,
    kiMbY: i32,
    kiMaxMvRange: i32,
    pMvMin: &mut SMVUnitXY,
    pMvMax: &mut SMVUnitXY,
) {
    {
        (*pMvMin).iMvX = ((-1 * ((kiMbX + 1) * (1 << 4)) + INTPEL_NEEDED_MARGIN)).max(-1 * kiMaxMvRange) as i16;
        (*pMvMin).iMvY = ((-1 * ((kiMbY + 1) * (1 << 4)) + INTPEL_NEEDED_MARGIN)).max(-1 * kiMaxMvRange) as i16;
        (*pMvMax).iMvX = (((kiMbWidth - kiMbX) * (1 << 4)) - INTPEL_NEEDED_MARGIN).min(kiMaxMvRange) as i16;
        (*pMvMax).iMvY = (((kiMbHeight - kiMbY) * (1 << 4)) - INTPEL_NEEDED_MARGIN).min(kiMaxMvRange) as i16;
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
#[inline]
pub fn CalcFMESwitchFlag(
    uiFMEGoodFrameCount: u8,
    _iHighFreMbPrecentage: i32,
    iAvgMbSAD: i32,
    bScrollingDetected: bool,
) -> bool {
    bScrollingDetected || (uiFMEGoodFrameCount > 0 && iAvgMbSAD > FMESWITCH_MBSAD_THRESHOLD)
}

#[inline]
pub fn GetCurrentSliceNum(pCurDq: &SDqLayer) -> i32 {
    pCurDq.sSliceEncCtx.iSliceNumInFrame.load(std::sync::atomic::Ordering::Relaxed)
}

// ============================================================================
// Initialization and Dispatch
// ============================================================================

/// Populates motion estimation function pointer table based on CPU capabilities and content type.
///
/// Session F: `&mut` — the init path's exclusive borrow, taken before anything
/// shares the table (the C-ABI init chain is single-threaded); the null
/// tolerance guarded a pointer that no longer exists.
pub fn WelsInitMeFunc(
    pFuncList: &mut SWelsFuncPtrList,
    uiCpuFlag: u32,
    bScreenContent: bool,
) {
    {
        pFuncList.pfUpdateFMESwitch = Some(UpdateFMESwitchNull);

        if !bScreenContent {
            pFuncList.sMeFuncs.pfCheckDirectionalMv = Some(CheckDirectionalMvFalse);
            pFuncList.pfCalculateBlockFeatureOfFrame[0] = None;
            pFuncList.pfCalculateBlockFeatureOfFrame[1] = None;
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[0] = None;
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[1] = None;
        } else {
            pFuncList.sMeFuncs.pfCheckDirectionalMv = Some(CheckDirectionalMv);

            // Cross Search
            pFuncList.sMeFuncs.pfVerticalFullSearch = Some(LineFullSearch_c);
            pFuncList.sMeFuncs.pfHorizontalFullSearch = Some(LineFullSearch_c);

            // Feature Search
            pFuncList.pfInitializeHashforFeature = Some(InitializeHashforFeature_c);
            pFuncList.pfFillQpelLocationByFeatureValue = Some(FillQpelLocationByFeatureValue_c);
            pFuncList.pfCalculateBlockFeatureOfFrame[0] = Some(SumOf8x8BlockOfFrame_c);
            pFuncList.pfCalculateBlockFeatureOfFrame[1] = Some(SumOf16x16BlockOfFrame_c);
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[0] = Some(sum_of_8x8_single_block);
            pFuncList.sMeFuncs.pfCalculateSingleBlockFeature[1] = Some(sum_of_16x16_single_block);
        }
    }
}

// ============================================================================
// Top-Level Motion Estimation Search Routines
// ============================================================================

/// Top-level motion estimation search for a macroblock or sub-partition.
// **S5.C4b re-tagged this.** It read `port-raw(Phase 9) — pMvdCost`, and pMvdCost is
// a `MvdCostCursor` now. What is left unsafe here is the `ME_DUMP` arm: a read of
// the `uSadPredISatd` union and two calls through the `unsafe fn` cost-function
// pointers. Neither is this checkpoint's family.
pub fn WelsMotionEstimateSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    if crate::encoder::dump_enabled(&ME_DUMP, "OH264_MEDUMP") {
        let mut mvc = String::new();
        for di in 0..(*pSlice).uiMvcNum as usize {
            mvc.push_str(&format!(
                "{}/{},",
                (*pSlice).sMvc[di].iMvX,
                (*pSlice).sMvc[di].iMvY
            ));
        }
        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;
        let cEnc = pEncPlane.cursor(kiX, kiY);
        // Entry state: the reference position is colocated (mv not yet
        // searched), which is what the deleted `pRefMb` held here.
        let cRef = pRefPlane.cursor(kiX, kiY);
        let mut enc = String::new();
        let mut rf = String::new();
        let mut rfup = String::new();
        for di in 0..8isize {
            enc.push_str(&format!("{},", cEnc.at(di, 0)));
            rf.push_str(&format!("{},", cRef.at(di, 0)));
            rfup.push_str(&format!("{},", cRef.at(di, -1)));
        }
        eprintln!(
            "ME bs={} px={},{} mvp={},{} base={},{} sadpred={} mvcn={} min={},{} max={},{} mvc={} enc={} ref={} refup={} mvdc={},{}",
            (*pMe).uiBlockSize,
            (*pMe).iCurMeBlockPixX,
            (*pMe).iCurMeBlockPixY,
            (*pMe).sMvp.iMvX,
            (*pMe).sMvp.iMvY,
            (*pMe).sMvBase.iMvX,
            (*pMe).sMvBase.iMvY,
            (*pMe).uSadPredISatd.uiValue,
            (*pSlice).uiMvcNum,
            (*pSlice).sMvStartMin.iMvX,
            (*pSlice).sMvStartMin.iMvY,
            (*pSlice).sMvStartMax.iMvX,
            (*pSlice).sMvStartMax.iMvY,
            mvc,
            enc,
            rf,
            rfup,
            (*pMe).pMvdCost.at(0),
            (*pMe).pMvdCost.at(4),
        );
    }

    // Step 1: Initial point prediction
    if !WelsMotionEstimateInitialPoint(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane) {
        let block_size = (*pMe).uiBlockSize as usize;
        if let Some(search_fn) = pMeFuncs.pfSearchMethod[block_size] {
            search_fn(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);
        }
        MeEndIntepelSearch(pMe);
    }

    let block_size = (*pMe).uiBlockSize as usize;
    if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
        calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
    }
    if crate::encoder::dump_enabled(&ME_DUMP, "OH264_MEDUMP") {
        eprintln!(
            "ME> mv={},{} sad={} satd={}",
            (*pMe).sMv.iMvX,
            (*pMe).sMv.iMvY,
            (*pMe).uiSadCost,
            (*pMe).uiSatdCost
        );
    }
}

/// Shortcut motion estimation search for static macroblocks (forced MV = (0,0)).
// SCREEN_CONTENT(dormant: Phase 10)
pub fn WelsMotionEstimateSearchStatic(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    _pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    let block_size = (*pMe).uiBlockSize as usize;
    let kiX = (*pMe).iCurMeBlockPixX as isize;
    let kiY = (*pMe).iCurMeBlockPixY as isize;

    (*pMe).sMv.iMvX = 0;
    (*pMe).sMv.iMvY = 0;

    if let Some(sad_fn) = sdf.pfSampleSad[block_size] {
        (*pMe).uiSadCost =
            sad_fn(&pEncPlane.cursor(kiX, kiY), &pRefPlane.cursor(kiX, kiY)) as u32;
    }
    (*pMe).uiSadCost += COST_MVD((*pMe).pMvdCost, -((*pMe).sMvp.iMvX as i32), -((*pMe).sMvp.iMvY as i32));

    MeEndIntepelSearch(pMe);

    if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
        calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
    }
}

/// Shortcut motion estimation search for scrolled macroblocks.
// SCREEN_CONTENT(dormant: Phase 10)
pub fn WelsMotionEstimateSearchScrolled(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    _pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    let block_size = (*pMe).uiBlockSize as usize;
    let kiX = (*pMe).iCurMeBlockPixX as isize;
    let kiY = (*pMe).iCurMeBlockPixY as isize;

    (*pMe).sMv = (*pMe).sDirectionalMv;
    let mv_x = (*pMe).sMv.iMvX as i32;
    let mv_y = (*pMe).sMv.iMvY as i32;

    let mut sad_cost = 0u32;
    if let Some(sad_fn) = sdf.pfSampleSad[block_size] {
        sad_cost = sad_fn(
            &pEncPlane.cursor(kiX, kiY),
            &pRefPlane.cursor(kiX + mv_x as isize, kiY + mv_y as isize),
        ) as u32;
    }
    sad_cost += COST_MVD(
        (*pMe).pMvdCost,
        (mv_x * 4) - ((*pMe).sMvp.iMvX as i32),
        (mv_y * 4) - ((*pMe).sMvp.iMvY as i32),
    );
    (*pMe).uiSadCost = sad_cost;

    MeEndIntepelSearch(pMe);

    if let Some(calc_satd) = pMeFuncs.pfCalculateSatd {
        calc_satd(sdf.pfSampleSatd[block_size], pMe, pEncPlane, pRefPlane);
    }
}

// ============================================================================
// Initial Candidate Prediction
// ============================================================================

/// Evaluates spatial MVP, MVC candidate list, and directional scrolling vectors.
pub fn WelsMotionEstimateInitialPoint(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) -> bool {
    let block_size = (*pMe).uiBlockSize as usize;
    let pSad = sdf.pfSampleSad[block_size];
    let kpMvdCost = (*pMe).pMvdCost;
    let kiX = (*pMe).iCurMeBlockPixX as isize;
    let kiY = (*pMe).iCurMeBlockPixY as isize;
    let cEnc = pEncPlane.cursor(kiX, kiY);

    let kuiMvcNum = (*pSlice).uiMvcNum as usize;
    let ksMvStartMin = (*pSlice).sMvStartMin;
    let ksMvStartMax = (*pSlice).sMvStartMax;
    let ksMvp = (*pMe).sMvp;

    let mut sMv = SMVUnitXY {
        iMvX: (((2 + ksMvp.iMvX as i32) >> 2).clamp(ksMvStartMin.iMvX as i32, ksMvStartMax.iMvX as i32)) as i16,
        iMvY: (((2 + ksMvp.iMvY as i32) >> 2).clamp(ksMvStartMin.iMvY as i32, ksMvStartMax.iMvY as i32)) as i16,
    };

    let mut iBestSadCost: i32 = 0;
    if let Some(sad_fn) = pSad {
        iBestSadCost = sad_fn(
            &cEnc,
            &pRefPlane.cursor(kiX + sMv.iMvX as isize, kiY + sMv.iMvY as isize),
        );
    }
    iBestSadCost += COST_MVD(
        kpMvdCost,
        (sMv.iMvX as i32 * 4) - ksMvp.iMvX as i32,
        (sMv.iMvY as i32 * 4) - ksMvp.iMvY as i32,
    ) as i32;

    let mut iSadCost: i32 = 0;
    for i in 0..kuiMvcNum {
        let mvc = (*pSlice).sMvc[i];
        let iMvc0 = (((2 + mvc.iMvX as i32) >> 2).clamp(ksMvStartMin.iMvX as i32, ksMvStartMax.iMvX as i32)) as i16;
        let iMvc1 = (((2 + mvc.iMvY as i32) >> 2).clamp(ksMvStartMin.iMvY as i32, ksMvStartMax.iMvY as i32)) as i16;

        if (iMvc0 != sMv.iMvX) || (iMvc1 != sMv.iMvY) {
            if let Some(sad_fn) = pSad {
                iSadCost = sad_fn(
                    &cEnc,
                    &pRefPlane.cursor(kiX + iMvc0 as isize, kiY + iMvc1 as isize),
                );
            }
            iSadCost += COST_MVD(
                kpMvdCost,
                (iMvc0 as i32 * 4) - ksMvp.iMvX as i32,
                (iMvc1 as i32 * 4) - ksMvp.iMvY as i32,
            ) as i32;

            if iSadCost < iBestSadCost {
                sMv.iMvX = iMvc0;
                sMv.iMvY = iMvc1;
                iBestSadCost = iSadCost;
            }
        }
    }

    if let Some(check_dir) = pMeFuncs.pfCheckDirectionalMv {
        if check_dir(pSad, pMe, ksMvStartMin, ksMvStartMax, pEncPlane, pRefPlane, &mut iSadCost) {
            sMv = (*pMe).sDirectionalMv;
            iBestSadCost = iSadCost;
        }
    }

    UpdateMeResults(sMv, iBestSadCost as u32, pMe);

    if iBestSadCost < (*pMe).uSadPredISatd.uiValue as i32 {
        MeEndIntepelSearch(pMe);
        return true;
    }

    false
}

// ============================================================================
// SATD Cost Calculation
// ============================================================================

/// Runs after `MeEndIntepelSearch`, so `sMv` is quarter-pel and the integer
/// reference position is `colo + (sMv >> 2)` — `MeRefineFracPixel`'s spelling
/// of the same identity.
pub fn CalculateSatdCost(
    pSatd: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME<'_>,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    if let Some(satd_fn) = pSatd {
        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;
        let cRef = pRefPlane.cursor(
            kiX + (((*pMe).sMv.iMvX as isize) >> 2),
            kiY + (((*pMe).sMv.iMvY as isize) >> 2),
        );
        (*pMe).uSadPredISatd.uiValue = satd_fn(&pEncPlane.cursor(kiX, kiY), &cRef) as u32;
        (*pMe).uiSatdCost = (*pMe).uSadPredISatd.uiValue
            + COST_MVD(
                (*pMe).pMvdCost,
                ((*pMe).sMv.iMvX - (*pMe).sMvp.iMvX) as i32,
                ((*pMe).sMv.iMvY - (*pMe).sMvp.iMvY) as i32,
            );
    }
}

pub fn NotCalculateSatdCost(
    _pSatd: Option<PSampleSadSatdCostFunc>,
    _pMe: &mut SWelsME<'_>,
    _pEncPlane: &SharedPlane,
    _pRefPlane: &SharedPlane,
) {
}

// ============================================================================
// Small Diamond Search (ME_DIA)
// ============================================================================

/// **S5.C4b**: safe, and no longer `extern "C"`. Both call sites are direct
/// (`WelsDiamondSearch` and this file's own test) — the body was never installed in
/// a dispatch table and no C consumer names it — so the four raw out-parameters
/// become the `&mut`s the call sites already had in hand, and the four-SAD array
/// becomes the `[i32; 4]` both sites already declared.
#[inline]
pub fn WelsMeSadCostSelect(
    iSadCost: &[i32; 4],
    kpMvdCost: MvdCostCursor<'_>,
    pBestCost: &mut i32,
    kiDx: i32,
    kiDy: i32,
    pIx: &mut i32,
    pIy: &mut i32,
) -> bool {
    {
        let iInputSadCost = *pBestCost;
        let mut iTempSadCost = [0i32; 4];
        iTempSadCost[0] = iSadCost[0] + COST_MVD(kpMvdCost, kiDx, kiDy - 4) as i32;
        iTempSadCost[1] = iSadCost[1] + COST_MVD(kpMvdCost, kiDx, kiDy + 4) as i32;
        iTempSadCost[2] = iSadCost[2] + COST_MVD(kpMvdCost, kiDx - 4, kiDy) as i32;
        iTempSadCost[3] = iSadCost[3] + COST_MVD(kpMvdCost, kiDx + 4, kiDy) as i32;

        if iTempSadCost[0] < *pBestCost {
            *pBestCost = iTempSadCost[0];
            *pIx = 0;
            *pIy = 1;
        }
        if iTempSadCost[1] < *pBestCost {
            *pBestCost = iTempSadCost[1];
            *pIx = 0;
            *pIy = -1;
        }
        if iTempSadCost[2] < *pBestCost {
            *pBestCost = iTempSadCost[2];
            *pIx = 1;
            *pIy = 0;
        }
        if iTempSadCost[3] < *pBestCost {
            *pBestCost = iTempSadCost[3];
            *pIx = -1;
            *pIy = 0;
        }

        *pBestCost == iInputSadCost
    }
}

pub fn WelsDiamondSearch(
    _pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    {
        let block_size = (*pMe).uiBlockSize as usize;
        let pSad4 = sdf.pfSample4Sad[block_size];
        let pSadSingle = sdf.pfSampleSad[block_size];

        let kiX = (*pMe).iCurMeBlockPixX as isize;
        let kiY = (*pMe).iCurMeBlockPixY as isize;
        let cEnc = pEncPlane.cursor(kiX, kiY);
        let kpMvdCost = (*pMe).pMvdCost;

        let ksMvStartMin = (*pSlice).sMvStartMin;
        let ksMvStartMax = (*pSlice).sMvStartMax;

        let mut iMvDx = ((*pMe).sMv.iMvX as i32 * 4) - (*pMe).sMvp.iMvX as i32;
        let mut iMvDy = ((*pMe).sMv.iMvY as i32 * 4) - (*pMe).sMvp.iMvY as i32;

        let mut iBestCost = (*pMe).uiSadCost as i32;
        let mut iTimeThreshold = ITERATIVE_TIMES;
        let mut iSadCosts = [0i32; 4];

        while iTimeThreshold > 0 {
            iTimeThreshold -= 1;
            (*pMe).sMv.iMvX = ((iMvDx + (*pMe).sMvp.iMvX as i32) >> 2) as i16;
            (*pMe).sMv.iMvY = ((iMvDy + (*pMe).sMvp.iMvY as i32) >> 2) as i16;

            if !CheckMvInRange((*pMe).sMv, ksMvStartMin, ksMvStartMax) {
                continue;
            }

            // The centre of this iteration's probe: colo + the current
            // integer MV — the walking `pRefMb` this loop used to maintain,
            // proven equal by the identity probe (session F step 1).
            let kiRx = kiX + (*pMe).sMv.iMvX as isize;
            let kiRy = kiY + (*pMe).sMv.iMvY as isize;

            if let Some(sad4_fn) = pSad4 {
                sad4_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy), &mut iSadCosts);
            } else if let Some(sad_fn) = pSadSingle {
                iSadCosts[0] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy - 1));
                iSadCosts[1] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx, kiRy + 1));
                iSadCosts[2] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx - 1, kiRy));
                iSadCosts[3] = sad_fn(&cEnc, &pRefPlane.cursor(kiRx + 1, kiRy));
            }

            let mut iX = 0i32;
            let mut iY = 0i32;
            let kbIsBestCostWorse = WelsMeSadCostSelect(
                &iSadCosts,
                kpMvdCost,
                &mut iBestCost,
                iMvDx,
                iMvDy,
                &mut iX,
                &mut iY,
            );
            if kbIsBestCostWorse {
                break;
            }

            iMvDx -= iX * 4;
            iMvDy -= iY * 4;
        }

        (*pMe).sMv.iMvX = ((iMvDx + (*pMe).sMvp.iMvX as i32) >> 2) as i16;
        (*pMe).sMv.iMvY = ((iMvDy + (*pMe).sMvp.iMvY as i32) >> 2) as i16;
        (*pMe).uiSadCost = iBestCost as u32;
        (*pMe).uiSatdCost = (*pMe).uiSadCost;
    }
}

// ============================================================================
// Directional Scrolling Search
// ============================================================================

pub fn CheckDirectionalMv(
    pSad: Option<PSampleSadSatdCostFunc>,
    pMe: &mut SWelsME<'_>,
    ksMinMv: SMVUnitXY,
    ksMaxMv: SMVUnitXY,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
    iBestSadCost: &mut i32,
) -> bool {
    {
        let kiMvX = (*pMe).sDirectionalMv.iMvX;
        let kiMvY = (*pMe).sDirectionalMv.iMvY;

        if ((*pMe).uiBlockSize as usize != BLOCK_16x16)
            && ((kiMvX != 0) || (kiMvY != 0))
            && CheckMvInRange((*pMe).sDirectionalMv, ksMinMv, ksMaxMv)
        {
            let kiX = (*pMe).iCurMeBlockPixX as isize;
            let kiY = (*pMe).iCurMeBlockPixY as isize;
            let mut uiCurrentSadCost = 0u32;
            if let Some(sad_fn) = pSad {
                uiCurrentSadCost = sad_fn(
                    &pEncPlane.cursor(kiX, kiY),
                    &pRefPlane.cursor(kiX + kiMvX as isize, kiY + kiMvY as isize),
                ) as u32;
            }
            uiCurrentSadCost += COST_MVD(
                (*pMe).pMvdCost,
                (kiMvX as i32 * 4) - (*pMe).sMvp.iMvX as i32,
                (kiMvY as i32 * 4) - (*pMe).sMvp.iMvY as i32,
            );
            if uiCurrentSadCost < (*pMe).uiSadCost {
                *iBestSadCost = uiCurrentSadCost as i32;
                return true;
            }
        }
        false
    }
}

pub fn CheckDirectionalMvFalse(
    _pSad: Option<PSampleSadSatdCostFunc>,
    _pMe: &mut SWelsME<'_>,
    _ksMinMv: SMVUnitXY,
    _ksMaxMv: SMVUnitXY,
    _pEncPlane: &SharedPlane,
    _pRefPlane: &SharedPlane,
    _iBestSadCost: &mut i32,
) -> bool {
    false
}

// ============================================================================
// 1D Orthogonal Cross Search (ME_CROSS)
// ============================================================================

// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitMeFunc` (`:508-528`) installs
// this body into `pfVerticalFullSearch`/`pfHorizontalFullSearch` **only** in its
// `bScreenContent` arm, and both slots default to `None` (`wels_func_ptr_def.rs:481`),
// so the two call sites (`:1085`, `:1099`) take their `if let Some(..)` never for
// camera content. Its sibling shortcuts `WelsMotionEstimateSearchStatic` and
// `..Scrolled` were already carrying this tag; this one was still filed as `cursor`,
// which read as a Phase 9 conversion still owed. It is not — it is Phase 10's.
pub fn LineFullSearch_c(
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pMvdTable: MvdCostCursor<'_>,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
    iMinMv: i16,
    iMaxMv: i16,
    bVerticalSearch: bool,
) {
    let block_size = (*pMe).uiBlockSize as usize;
    let pSad = sdf.pfSampleSad[block_size];
    let kiCurMeBlockPixX = (*pMe).iCurMeBlockPixX;
    let kiCurMeBlockPixY = (*pMe).iCurMeBlockPixY;
    let kiX = kiCurMeBlockPixX as isize;
    let kiY = kiCurMeBlockPixY as isize;
    let cEnc = pEncPlane.cursor(kiX, kiY);

    let iMinPos: i32;
    let iMaxPos: i32;
    let iFixedMvd: i32;
    let iCurMeBlockPix: i32;
    let mut pMvdCost: MvdCostCursor<'_>;

    if bVerticalSearch {
        iMinPos = kiCurMeBlockPixY + iMinMv as i32;
        iMaxPos = kiCurMeBlockPixY + iMaxMv as i32;
        iFixedMvd = pMvdTable.at(-((*pMe).sMvp.iMvX as i32)) as i32;
        iCurMeBlockPix = kiCurMeBlockPixY;
        pMvdCost = pMvdTable.offset((iMinMv as i32 * 4) - (*pMe).sMvp.iMvY as i32);
    } else {
        iMinPos = kiCurMeBlockPixX + iMinMv as i32;
        iMaxPos = kiCurMeBlockPixX + iMaxMv as i32;
        iFixedMvd = pMvdTable.at(-((*pMe).sMvp.iMvY as i32)) as i32;
        iCurMeBlockPix = kiCurMeBlockPixX;
        pMvdCost = pMvdTable.offset((iMinMv as i32 * 4) - (*pMe).sMvp.iMvX as i32);
    }

    let mut uiBestCost: u32 = 0xFFFF_FFFF;
    let mut iBestPos: i32 = 0;

    for iTargetPos in iMinPos..iMaxPos {
        // The walking pRef was colo + d rows (vertical) or d columns.
        let d = (iTargetPos - iCurMeBlockPix) as isize;
        let cRef = if bVerticalSearch {
            pRefPlane.cursor(kiX, kiY + d)
        } else {
            pRefPlane.cursor(kiX + d, kiY)
        };
        let mut uiSadCost: u32 = 0;
        if let Some(sad_fn) = pSad {
            uiSadCost = sad_fn(&cEnc, &cRef) as u32;
        }
        uiSadCost += (iFixedMvd + pMvdCost.at(0) as i32) as u32;
        if uiSadCost < uiBestCost {
            uiBestCost = uiSadCost;
            iBestPos = iTargetPos;
        }
        pMvdCost = pMvdCost.offset(4);
    }

    if uiBestCost < (*pMe).uiSadCost {
        let mut sBestMv = SMVUnitXY::default();
        sBestMv.iMvX = if bVerticalSearch { 0 } else { (iBestPos - iCurMeBlockPix) as i16 };
        sBestMv.iMvY = if bVerticalSearch { (iBestPos - iCurMeBlockPix) as i16 } else { 0 };
        UpdateMeResults(sBestMv, uiBestCost, pMe);
    }
}

pub fn WelsMotionCrossSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    if let Some(vert_fn) = pMeFuncs.pfVerticalFullSearch {
        vert_fn(
            sdf,
            pMe,
            (*pMe).pMvdCost,
            pEncPlane,
            pRefPlane,
            (*pSlice).sMvStartMin.iMvY,
            (*pSlice).sMvStartMax.iMvY,
            true,
        );
    }

    if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
        if let Some(horiz_fn) = pMeFuncs.pfHorizontalFullSearch {
            horiz_fn(
                sdf,
                pMe,
                (*pMe).pMvdCost,
                pEncPlane,
                pRefPlane,
                (*pSlice).sMvStartMin.iMvX,
                (*pSlice).sMvStartMax.iMvX,
                false,
            );
        }
    }
}

pub fn WelsDiamondCrossSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    WelsDiamondSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);

    if let Some(storage) = (*pMe).pRefFeatureStorage {
        let block_size = (*pMe).uiBlockSize as usize;
        (*pMe).uiSadCostThreshold = storage.uiSadCostThreshold[block_size];
    }
    if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
        WelsMotionCrossSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
pub fn WelsDiamondCrossFeatureSearch(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    pMe: &mut SWelsME<'_>,
    pSlice: &mut SSlice,
    pEncPlane: &SharedPlane,
    pRefPlane: &SharedPlane,
) {
    WelsDiamondCrossSearch(pMeFuncs, sdf, pMe, pSlice, pEncPlane, pRefPlane);

    if (*pMe).uiSadCost >= (*pMe).uiSadCostThreshold {
        (*pSlice).uiSliceFMECostDown = (*pSlice).uiSliceFMECostDown.wrapping_add((*pMe).uiSadCost);

        let mut sFeatureSearchIn = SFeatureSearchIn::default();
        if SetFeatureSearchIn(
            pMeFuncs,
            sdf,
            pMe,
            &*pSlice,
            (*pMe).pRefFeatureStorage,
            pEncPlane,
            pRefPlane,
            &mut sFeatureSearchIn,
        ) {
            MotionEstimateFeatureFullSearch(sFeatureSearchIn, u32::MAX, pMe);
        }
        (*pSlice).uiSliceFMECostDown = (*pSlice).uiSliceFMECostDown.wrapping_sub((*pMe).uiSadCost);
    }
}

/// `SetMeMethod` — `encoder_ext.cpp:2639-2662`. Aims one `pfSearchMethod` slot at a
/// search family; `false` means the request was not honoured and the slot holds the
/// diamond search (`ME_FULL` and every unknown value).
///
/// **Re-ported at P10.3.D1.** S18, session F deleted it as callerless: the C++'s only
/// caller is `PreprocessSliceCoding`'s SCREEN_CONTENT block, which the port did not
/// translate until P10.3.D4. The slot is `&mut Option<PSearchMethodFunc>` where the
/// C++ takes `PSearchMethodFunc&` — the table's entries are `Option`s here, so the
/// out-parameter is the option, not the pointer inside it.
pub fn SetMeMethod(uiMethod: u32, pSearchMethodFunc: &mut Option<PSearchMethodFunc>) -> bool {
    match uiMethod {
        ME_DIA => {
            *pSearchMethodFunc = Some(WelsDiamondSearch);
            true
        }
        ME_CROSS => {
            *pSearchMethodFunc = Some(WelsMotionCrossSearch);
            true
        }
        ME_DIA_CROSS => {
            *pSearchMethodFunc = Some(WelsDiamondCrossSearch);
            true
        }
        ME_DIA_CROSS_FME => {
            *pSearchMethodFunc = Some(WelsDiamondCrossFeatureSearch);
            true
        }
        // `ME_FULL` and the C++'s `default:` arm — one match arm because the two
        // C++ cases have identical bodies (`WelsDiamondSearch`, `return false`).
        _ => {
            *pSearchMethodFunc = Some(WelsDiamondSearch);
            false
        }
    }
}

// ============================================================================
// Feature Search (FME / Screen Content Coding)
// ============================================================================

/// The safe per-block twin of [`SumOf8x8SingleBlock_c`] — what the
/// `pfCalculateSingleBlockFeature` slot holds since session F. The raw kernel
/// stays for the frame-feature builders, which walk a whole raw plane.
// SCREEN_CONTENT(dormant: Phase 10)
pub fn sum_of_8x8_single_block(cRef: &RecCursor<'_>) -> i32 {
    let mut iSum = 0i32;
    for y in 0..8 {
        for b in cRef.row::<8>(y, 0) {
            iSum += b as i32;
        }
    }
    iSum
}

/// As [`sum_of_8x8_single_block`], 16x16.
// SCREEN_CONTENT(dormant: Phase 10)
pub fn sum_of_16x16_single_block(cRef: &RecCursor<'_>) -> i32 {
    let mut iSum = 0i32;
    for y in 0..16 {
        for b in cRef.row::<16>(y, 0) {
            iSum += b as i32;
        }
    }
    iSum
}

pub fn SumOf8x8SingleBlock_c(kpRef: &[u8], kiRefStride: i32) -> i32 {
    // S11.6: the pointer walk becomes an offset walk over the caller's slice.
    // The rows are `kiRefStride` apart and `8` wide, exactly as the raw form
    // read them, and every access is bounds-checked — where the raw version
    // would have walked off a short plane silently, this panics.
    let mut iSum = 0i32;
    for r in 0..8 {
        let kiOff = r * kiRefStride as usize;
        iSum += kpRef[kiOff] as i32
            + kpRef[kiOff + 1] as i32
            + kpRef[kiOff + 2] as i32
            + kpRef[kiOff + 3] as i32
            + kpRef[kiOff + 4] as i32
            + kpRef[kiOff + 5] as i32
            + kpRef[kiOff + 6] as i32
            + kpRef[kiOff + 7] as i32;
    }
    iSum
}

pub fn SumOf16x16SingleBlock_c(kpRef: &[u8], kiRefStride: i32) -> i32 {
    // S11.6: the pointer walk becomes an offset walk over the caller's slice.
    // The rows are `kiRefStride` apart and `16` wide, exactly as the raw form
    // read them, and every access is bounds-checked — where the raw version
    // would have walked off a short plane silently, this panics.
    let mut iSum = 0i32;
    for r in 0..16 {
        let kiOff = r * kiRefStride as usize;
        iSum += kpRef[kiOff] as i32
            + kpRef[kiOff + 1] as i32
            + kpRef[kiOff + 2] as i32
            + kpRef[kiOff + 3] as i32
            + kpRef[kiOff + 4] as i32
            + kpRef[kiOff + 5] as i32
            + kpRef[kiOff + 6] as i32
            + kpRef[kiOff + 7] as i32
            + kpRef[kiOff + 8] as i32
            + kpRef[kiOff + 9] as i32
            + kpRef[kiOff + 10] as i32
            + kpRef[kiOff + 11] as i32
            + kpRef[kiOff + 12] as i32
            + kpRef[kiOff + 13] as i32
            + kpRef[kiOff + 14] as i32
            + kpRef[kiOff + 15] as i32;
    }
    iSum
}

pub fn SumOf8x8BlockOfFrame_c(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    for y in 0..kiHeight {
        // **S6.B1**: the plane walk stays raw and the two writes are indexed. `iSum` is
        // bounded by the kernel's own arithmetic — a sum of 8x8 bytes — and
        // `pTimesOfFeatureValue` is `iActualListSize` long, which the allocator sizes
        // from exactly that bound, so the index is in range for every well-formed
        // storage. Out of range now panics where it used to write past the buffer.
        let row = (kiWidth * y) as usize;
        // S11.6: the plane walk is an offset now. The row base is the same
        // `kiRefStride * y`, and each block starts `x` bytes into it.
        let kiRowBase = (kiRefStride * y) as usize;
        for x in 0..kiWidth {
            let iSum =
                SumOf8x8SingleBlock_c(&kpRefPicture[kiRowBase + x as usize..], kiRefStride);
            pFeatureOfBlock[row + x as usize] = iSum as u16;
            pTimesOfFeatureValue[iSum as usize] += 1;
        }
    }
}

pub fn SumOf16x16BlockOfFrame_c(
    kpRefPicture: &[u8],
    kiWidth: i32,
    kiHeight: i32,
    kiRefStride: i32,
    pFeatureOfBlock: &mut [u16],
    pTimesOfFeatureValue: &mut [u32],
) {
    for y in 0..kiHeight {
        // **S6.B1**: the plane walk stays raw and the two writes are indexed. `iSum` is
        // bounded by the kernel's own arithmetic — a sum of 16x16 bytes — and
        // `pTimesOfFeatureValue` is `iActualListSize` long, which the allocator sizes
        // from exactly that bound, so the index is in range for every well-formed
        // storage. Out of range now panics where it used to write past the buffer.
        let row = (kiWidth * y) as usize;
        // S11.6: the plane walk is an offset now. The row base is the same
        // `kiRefStride * y`, and each block starts `x` bytes into it.
        let kiRowBase = (kiRefStride * y) as usize;
        for x in 0..kiWidth {
            let iSum =
                SumOf16x16SingleBlock_c(&kpRefPicture[kiRowBase + x as usize..], kiRefStride);
            pFeatureOfBlock[row + x as usize] = iSum as u16;
            pTimesOfFeatureValue[iSum as usize] += 1;
        }
    }
}

pub fn InitializeHashforFeature_c(
    pTimesOfFeatureValue: &[u32],
    kiListSize: i32,
    pLocationOfFeature: &mut [usize],
    pFeatureValuePointerList: &mut [usize],
) {
    // **S6.B1**, and this is the loop the whole shape follows from: `pBufPos` was a
    // cursor walking the arena, laying each feature value's group base and giving that
    // value's write cursor the same start. It is the running offset now — the identical
    // arithmetic, `times << 1` per value because each position is an (x, y) pair.
    let mut pBufPos = 0usize;
    for i in 0..kiListSize as usize {
        pLocationOfFeature[i] = pBufPos;
        pFeatureValuePointerList[i] = pBufPos;
        pBufPos += (pTimesOfFeatureValue[i] as usize) << 1;
    }
}

pub fn FillQpelLocationByFeatureValue_c(
    pFeatureOfBlock: &[u16],
    kiWidth: i32,
    kiHeight: i32,
    pLocationPointer: &mut [u16],
    pFeatureValuePointerList: &mut [usize],
) {
    // **S6.B1**: `target_ptr` was the roving cursor for this feature value and
    // `target_ptr.add(2)` its advance; `target` is that cursor as an arena offset and
    // `+ 2` the same advance. Each value's cursor starts at its group base
    // (`InitializeHashforFeature_c`) and is advanced once per position carrying that
    // value, so the writes exactly fill `2 * times[value]` slots — the invariant the
    // family's test asserts, because no gate reaches this line.
    let mut pSrcPointer = 0usize;
    let mut iQpelY = 0i32;
    for _ in 0..kiHeight {
        for x in 0..kiWidth {
            let uiFeature = pFeatureOfBlock[pSrcPointer + x as usize] as usize;
            let target = pFeatureValuePointerList[uiFeature];
            pLocationPointer[target] = (x << 2) as u16;
            pLocationPointer[target + 1] = iQpelY as u16;
            pFeatureValuePointerList[uiFeature] = target + 2;
        }
        iQpelY += 4;
        pSrcPointer += kiWidth as usize;
    }
}

/// The three dispatch slots `CalculateFeatureOfBlock` reads, copied out of the
/// table so the caller can hold the reference picture and the table apart (they
/// are `Copy` fn pointers; P10.3's caller in `PreprocessSliceCoding` needs the table
/// `&mut` at the same time as the reference list). P10.1.B5 (D-scc-3).
#[derive(Clone, Copy)]
pub struct FmeKernels {
    pub calc_frame: [Option<PCalculateBlockFeatureOfFrame>; 2],
    pub init_hash: Option<PInitializeHashforFeatureFunc>,
    pub fill_qpel: Option<PFillQpelLocationByFeatureValueFunc>,
}

impl FmeKernels {
    /// The three reads off the table — `pfCalculateBlockFeatureOfFrame[2]`,
    /// `pfInitializeHashforFeature`, `pfFillQpelLocationByFeatureValue`.
    pub fn of(pFunc: &SWelsFuncPtrList) -> Self {
        Self {
            calc_frame: pFunc.pfCalculateBlockFeatureOfFrame,
            init_hash: pFunc.pfInitializeHashforFeature,
            fill_qpel: pFunc.pfFillQpelLocationByFeatureValue,
        }
    }
}

/// `CalculateFeatureOfBlock` — `svc_motion_estimate.cpp:843-878`.
///
/// **P10.1.B5 (D-scc-3)**: `pFeatureOfBlock` is the layer's scratch
/// (`SFeatureSearchPreparation::pFeatureOfBlock`), which the C++ reaches through
/// the address `PerformFMEPreprocess` stored in the storage; it arrives as a slice.
/// `storage` is a separate parameter, not reached through `pRef`, on purpose:
/// P10.3's caller takes the box out of the reference picture (`Option::take`), runs
/// this with the picture's planes, and puts it back — the only way the picture's
/// plane and its own storage can be borrowed together without a split accessor.
/// Under LTR the planes come from a different picture anyway (`pRefOri[0]`). `pRef`
/// supplies what the C++ reads off it: `pData[0]`/`iLineSize[0]` (`plane(0)`,
/// `stride(0)`), `iWidthInPixel`, `iHeightInPixel`.
pub fn CalculateFeatureOfBlock(
    kernels: &FmeKernels,
    pRef: &SPicture,
    pFeatureOfBlock: &mut [u16],
    pScreenBlockFeatureStorage: &mut SScreenBlockFeatureStorage,
) -> bool {
    // **S6.B1**: the four `is_null()` arms became four `is_empty()` arms. An unbuilt
    // storage answers `false` here exactly as a storage with null buffers did — the
    // allocator either sized all four or none, so no arm changes which inputs are
    // rejected. The C++'s fifth arm, `NULL == pRef->pData[0]`, has no subject: a
    // pool picture always has its three planes (`SPicture::new` builds them), so
    // there is nothing to test (S37).
    let SScreenBlockFeatureStorage {
        pTimesOfFeatureValue,
        pLocationOfFeature,
        pLocationPointer: pBuf,
        pFeatureValuePointerList,
        iIs16x16,
        iActualListSize: kiActualListSize,
        ..
    } = pScreenBlockFeatureStorage;
    // Destructured, not field-by-field: the three dispatch calls below need two or
    // three of these buffers borrowed at once, and a `&mut` per field through the
    // struct would be a fresh whole-struct claim each time.

    if pFeatureOfBlock.is_empty()
        || pTimesOfFeatureValue.is_empty()
        || pLocationOfFeature.is_empty()
        || pBuf.is_empty()
    {
        return false;
    }

    let iIs16x16 = *iIs16x16 as usize;
    let kiActualListSize = *kiActualListSize;
    let iEdgeDiscard = if iIs16x16 != 0 { 16 } else { 8 };
    let iWidth = pRef.iWidthInPixel - iEdgeDiscard;
    let kiHeight = pRef.iHeightInPixel - iEdgeDiscard;

    // `write_bytes(.., 0, kiActualListSize * size_of::<u32>())` — the same
    // `kiActualListSize` prefix, zeroed elementwise.
    pTimesOfFeatureValue[..kiActualListSize as usize].fill(0);

    if let Some(calc_frame_feature) = kernels.calc_frame[iIs16x16] {
        // **S11.6: the plane arrives as a slice from its logical origin**, which
        // is the same address `data_ptr(0)` returned — `as_slice()[origin()..]`
        // is that accessor's own arithmetic, with the buffer's end now known to
        // the callee. The slot and both kernels behind it are safe fns.
        let iRefStride = pRef.stride(0);
        let plane = pRef.plane(0);
        let kpRefData = &plane.as_slice()[plane.origin()..];
        calc_frame_feature(kpRefData, iWidth, kiHeight, iRefStride, pFeatureOfBlock, pTimesOfFeatureValue);
    }

    if let Some(init_hash) = kernels.init_hash {
        init_hash(
            pTimesOfFeatureValue,
            kiActualListSize,
            pLocationOfFeature,
            pFeatureValuePointerList,
        );
    }

    if let Some(fill_qpel) = kernels.fill_qpel {
        fill_qpel(pFeatureOfBlock, iWidth, kiHeight, pBuf, pFeatureValuePointerList);
    }

    true
}

// SCREEN_CONTENT(dormant: Phase 10) — no caller after P10.1: P10.3 installs the
// call from `PreprocessSliceCoding`'s screen block (`encoder_ext.cpp:2745-2749`).
/// `PerformFMEPreprocess` — `svc_motion_estimate.cpp:880-893`.
///
/// **P10.1.B5 (D-scc-3)**: the C++ stores its caller's `pFeatureOfBlock` pointer
/// into the storage (`:882`) and `CalculateFeatureOfBlock` reads it back — one
/// owner, two names. The scratch stays the layer's and is passed through instead.
pub fn PerformFMEPreprocess(
    kernels: &FmeKernels,
    pRef: &SPicture,
    pFeatureOfBlock: &mut [u16],
    pScreenBlockFeatureStorage: &mut SScreenBlockFeatureStorage,
) {
    pScreenBlockFeatureStorage.bRefBlockFeatureCalculated =
        CalculateFeatureOfBlock(kernels, pRef, pFeatureOfBlock, pScreenBlockFeatureStorage);

    if pScreenBlockFeatureStorage.bRefBlockFeatureCalculated {
        let qp_idx = (pRef.iFrameAverageQp).clamp(0, 51) as usize;
        let uiRefPictureAvgQstepx16 = QStepx16ByQp[qp_idx] as u32;
        let uiSadCostThreshold16x16 = (30 * (uiRefPictureAvgQstepx16 + 160)) >> 3;

        pScreenBlockFeatureStorage.uiSadCostThreshold[BLOCK_16x16] = uiSadCostThreshold16x16;
        pScreenBlockFeatureStorage.uiSadCostThreshold[BLOCK_8x8] = uiSadCostThreshold16x16 >> 2;
        pScreenBlockFeatureStorage.uiSadCostThreshold[BLOCK_16x8] = u32::MAX;
        pScreenBlockFeatureStorage.uiSadCostThreshold[BLOCK_8x16] = u32::MAX;
        pScreenBlockFeatureStorage.uiSadCostThreshold[BLOCK_4x4] = u32::MAX;
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
pub fn SetFeatureSearchIn<'a>(
    pMeFuncs: &SMeFuncs,
    sdf: &SSampleDealingFunc,
    sMe: &SWelsME<'a>,
    pSlice: &SSlice,
    pRefFeatureStorage: Option<&'a SScreenBlockFeatureStorage>,
    pEncPlane: &'a SharedPlane,
    pRefPlane: &'a SharedPlane,
    pFeatureSearchIn: &mut SFeatureSearchIn<'a>,
) -> bool {
    let block_size = sMe.uiBlockSize as usize;
    pFeatureSearchIn.pSad = sdf.pfSampleSad[block_size];

    let single_fn_idx = if block_size == BLOCK_16x16 { 1 } else { 0 };
    if let Some(calc_single) = pMeFuncs.pfCalculateSingleBlockFeature[single_fn_idx] {
        pFeatureSearchIn.iFeatureOfCurrent = calc_single(
            &pEncPlane.cursor(sMe.iCurMeBlockPixX as isize, sMe.iCurMeBlockPixY as isize),
        );
    }

    pFeatureSearchIn.pEncPlane = Some(pEncPlane);
    pFeatureSearchIn.pRefPlane = Some(pRefPlane);
    pFeatureSearchIn.uiSadCostThresh = sMe.uiSadCostThreshold as u16;

    pFeatureSearchIn.iCurPixX = sMe.iCurMeBlockPixX;
    pFeatureSearchIn.iCurPixXQpel = pFeatureSearchIn.iCurPixX << 2;
    pFeatureSearchIn.iCurPixY = sMe.iCurMeBlockPixY;
    pFeatureSearchIn.iCurPixYQpel = pFeatureSearchIn.iCurPixY << 2;

    // **S5.C6d.** Placed here, not at the top: every line above still runs exactly
    // as it did, and only the point where the old code would have dereferenced a
    // null `pRefFeatureStorage` becomes a defined `false`. That is the same answer
    // its sibling `CalculateFeatureOfBlock` already gives when its own pointers are
    // null, and it replaces undefined behaviour rather than defined behaviour: with
    // `SPicture::pScreenBlockFeatureStorage` never filled (F229), a caller setting
    // `SCREEN_CONTENT_REAL_TIME` reached this line with a null and read through it.
    let Some(pRefFeatureStorage) = pRefFeatureStorage else {
        return false;
    };
    pFeatureSearchIn.pTimesOfFeature = &pRefFeatureStorage.pTimesOfFeatureValue;
    pFeatureSearchIn.pQpelLocationOfFeature = &pRefFeatureStorage.pLocationOfFeature;
    pFeatureSearchIn.pLocationPointer = &pRefFeatureStorage.pLocationPointer;
    pFeatureSearchIn.pMvdCostX = sMe.pMvdCost.offset(-pFeatureSearchIn.iCurPixXQpel - sMe.sMvp.iMvX as i32);
    pFeatureSearchIn.pMvdCostY = sMe.pMvdCost.offset(-pFeatureSearchIn.iCurPixYQpel - sMe.sMvp.iMvY as i32);

    pFeatureSearchIn.iMinQpelX = pFeatureSearchIn.iCurPixXQpel + (pSlice.sMvStartMin.iMvX as i32 * 4);
    pFeatureSearchIn.iMinQpelY = pFeatureSearchIn.iCurPixYQpel + (pSlice.sMvStartMin.iMvY as i32 * 4);
    pFeatureSearchIn.iMaxQpelX = pFeatureSearchIn.iCurPixXQpel + (pSlice.sMvStartMax.iMvX as i32 * 4);
    pFeatureSearchIn.iMaxQpelY = pFeatureSearchIn.iCurPixYQpel + (pSlice.sMvStartMax.iMvY as i32 * 4);

    if pFeatureSearchIn.pSad.is_none()
        || pFeatureSearchIn.pTimesOfFeature.is_empty()
        || pFeatureSearchIn.pQpelLocationOfFeature.is_empty()
    {
        return false;
    }
    true
}

// SCREEN_CONTENT(dormant: Phase 10)
pub fn SaveFeatureSearchOut(
    sBestMv: SMVUnitXY,
    uiBestSadCost: u32,
    pFeatureSearchOut: &mut SFeatureSearchOut,
) {
    pFeatureSearchOut.sBestMv = sBestMv;
    pFeatureSearchOut.uiBestSadCost = uiBestSadCost;
}

// SCREEN_CONTENT(dormant: Phase 10)
pub fn FeatureSearchOne(
    sFeatureSearchIn: &SFeatureSearchIn<'_>,
    iFeatureDifference: i32,
    kuiExpectedSearchTimes: u32,
    pFeatureSearchOut: &mut SFeatureSearchOut,
) -> bool {
    let iFeatureOfRef = sFeatureSearchIn.iFeatureOfCurrent + iFeatureDifference;
    if iFeatureOfRef < 0 || iFeatureOfRef >= LIST_SIZE {
        return true;
    }

    let pSad = sFeatureSearchIn.pSad;
    let pEncPlane = sFeatureSearchIn.pEncPlane.expect("SetFeatureSearchIn bound the planes");
    let pRefPlane = sFeatureSearchIn.pRefPlane.expect("SetFeatureSearchIn bound the planes");
    let uiSadCostThresh = sFeatureSearchIn.uiSadCostThresh as u32;

    let iCurPixX = sFeatureSearchIn.iCurPixX;
    let iCurPixY = sFeatureSearchIn.iCurPixY;
    let iCurPixXQpel = sFeatureSearchIn.iCurPixXQpel;
    let iCurPixYQpel = sFeatureSearchIn.iCurPixYQpel;
    let cEnc = pEncPlane.cursor(iCurPixX as isize, iCurPixY as isize);

    let iMinQpelX = sFeatureSearchIn.iMinQpelX;
    let iMinQpelY = sFeatureSearchIn.iMinQpelY;
    let iMaxQpelX = sFeatureSearchIn.iMaxQpelX;
    let iMaxQpelY = sFeatureSearchIn.iMaxQpelY;

    {
        // **S6.B1**: two indexed reads where there were two pointer loads. `times` is
        // the histogram entry for this feature value and `pQpelPosition` was the
        // address of that value's group in the arena — the group's offset now, which
        // the walk below adds to.
        let times = sFeatureSearchIn.pTimesOfFeature[iFeatureOfRef as usize];
        let iSearchTimes = times.min(kuiExpectedSearchTimes) as i32;
        let iSearchTimesx2 = iSearchTimes << 1;
        let pQpelPosition = sFeatureSearchIn.pQpelLocationOfFeature[iFeatureOfRef as usize];
        let arena = sFeatureSearchIn.pLocationPointer;

        let mut sBestMv = pFeatureSearchOut.sBestMv;
        let mut uiBestCost = pFeatureSearchOut.uiBestSadCost;

        let mut i = 0i32;
        while i < iSearchTimesx2 {
            let iQpelX = arena[pQpelPosition + i as usize] as i32;
            let iQpelY = arena[pQpelPosition + (i + 1) as usize] as i32;

            if (iQpelX > iMaxQpelX)
                || (iQpelX < iMinQpelX)
                || (iQpelY > iMaxQpelY)
                || (iQpelY < iMinQpelY)
                || (iQpelX == iCurPixXQpel)
                || (iQpelY == iCurPixYQpel)
            {
                i += 2;
                continue;
            }

            let mut uiTmpCost = (sFeatureSearchIn.pMvdCostX.at(iQpelX) as u32)
                + (sFeatureSearchIn.pMvdCostY.at(iQpelY) as u32);
            if uiTmpCost.wrapping_add(iFeatureDifference as u32) >= uiBestCost {
                i += 2;
                continue;
            }

            let iIntepelX = (iQpelX >> 2) - iCurPixX;
            let iIntepelY = (iQpelY >> 2) - iCurPixY;

            if let Some(sad_fn) = pSad {
                uiTmpCost += sad_fn(
                    &cEnc,
                    &pRefPlane.cursor(
                        (iCurPixX + iIntepelX) as isize,
                        (iCurPixY + iIntepelY) as isize,
                    ),
                ) as u32;
            }

            if uiTmpCost < uiBestCost {
                sBestMv.iMvX = iIntepelX as i16;
                sBestMv.iMvY = iIntepelY as i16;
                uiBestCost = uiTmpCost;

                if uiBestCost < uiSadCostThresh {
                    break;
                }
            }

            i += 2;
        }

        SaveFeatureSearchOut(sBestMv, uiBestCost, pFeatureSearchOut);
        i < iSearchTimesx2
    }
}

// SCREEN_CONTENT(dormant: Phase 10)
pub fn MotionEstimateFeatureFullSearch(
    sFeatureSearchIn: SFeatureSearchIn<'_>,
    kuiMaxSearchPoint: u32,
    pMe: &mut SWelsME<'_>,
) {
    {
        let mut sFeatureSearchOut = SFeatureSearchOut {
            sBestMv: (*pMe).sMv,
            uiBestSadCost: (*pMe).uiSadCost,
        };

        let iFeatureDifference = 0i32;
        FeatureSearchOne(&sFeatureSearchIn, iFeatureDifference, kuiMaxSearchPoint, &mut sFeatureSearchOut);

        if sFeatureSearchOut.uiBestSadCost < (*pMe).uiSadCost {
            UpdateMeResults(
                sFeatureSearchOut.sBestMv,
                sFeatureSearchOut.uiBestSadCost,
                pMe,
            );
        }
    }
}

// ============================================================================
// Adaptive FME Switch Management
// ============================================================================

/// `CountFMECostDown` — `svc_motion_estimate.cpp:1027-1041`: the sum of every
/// coded slice's `uiSliceFMECostDown`.
///
/// **Re-ported at P10.3.D2**, with `UpdateFMEGoodFrameCount` and
/// `UpdateFMESwitch`. T6.D2 deleted all three (S18) while the screen-content
/// dispatch that installs them was untranslated; D4 translates it.
///
/// `&mut` where the C++ takes `const SDqLayer*`, and nothing here is written:
/// the slice-bank family has only exclusive accessors, because S11.35 deleted
/// `slice_in_layer`'s shared twin for having no callers. The C++'s dead first
/// `pSlice` read (`:1031`) has no subject — the loop overwrites it before any
/// use.
fn CountFMECostDown(pCurLayer: &mut SDqLayer) -> u32 {
    let kiSliceCount = GetCurrentSliceNum(pCurLayer);
    let mut uiCostDownSum: u32 = 0;
    if kiSliceCount >= 1 {
        for iSliceIndex in 0..kiSliceCount {
            if let Some(pSlice) =
                crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurLayer, iSliceIndex)
            {
                // `uint32_t +=`: the C++ wraps, so this does. `uiSliceFMECostDown`
                // is itself a wrapping `+=`/`-=` pair in
                // `WelsDiamondCrossFeatureSearch`, and **nothing ever resets it**
                // (its three mentions in the whole reference are that pair and the
                // sum here) — it accumulates for the life of the slice object.
                uiCostDownSum = uiCostDownSum.wrapping_add(pSlice.uiSliceFMECostDown);
            }
        }
    }
    uiCostDownSum
}

/// `UpdateFMEGoodFrameCount` — `svc_motion_estimate.cpp:1043-1052`.
/// `uiFMEGoodFrameCount` lies in `[0, FMESWITCH_GOODFRAMECOUNT_MAX]`, which is
/// what the two guards are for; neither `+= 1` nor `-= 1` can overflow behind
/// them, so they are plain arithmetic here as in the C++.
fn UpdateFMEGoodFrameCount(iAvMBNormalizedRDcostDown: u32, uiFMEGoodFrameCount: &mut u8) {
    //this strategy may be changed, here the number is derived from empirical-numbers
    if iAvMBNormalizedRDcostDown > FMESWITCH_MBAVERCOSTSAVING_THRESHOLD {
        if *uiFMEGoodFrameCount < FMESWITCH_GOODFRAMECOUNT_MAX {
            *uiFMEGoodFrameCount += 1;
        }
    } else if *uiFMEGoodFrameCount > 0 {
        *uiFMEGoodFrameCount -= 1;
    }
}

/// `UpdateFMESwitch` — `svc_motion_estimate.cpp:1054-1058`. Called through
/// `pfUpdateFMESwitch` after the fork joins, on the frame thread.
///
/// The C++ dereferences `pFeatureSearchPreparation` unconditionally and may do
/// so safely: this body is installed only from inside
/// `PreprocessSliceCoding`'s `if (pFeatureSearchPreparation)` (D4), so a layer
/// reaching it without one does not exist. The port asks anyway, which costs a
/// branch and removes a null dereference from the reachable set.
///
/// `kiMbNum` is never zero on any path that installs this — the layer's
/// macroblock grid is sized in `InitDqLayers`, before any slice is coded — so
/// the division mirrors the C++'s, which would be undefined at zero.
pub fn UpdateFMESwitch(pCurLayer: &mut SDqLayer) {
    let iFMECost = CountFMECostDown(pCurLayer);
    // The C++ multiplies two `int32_t`s and divides a `uint32_t` by the product,
    // so the division is unsigned. `iMbWidth`/`iMbHeight` are `int16_t` here and
    // both are positive, so the product is the same number.
    let kiMbNum = (pCurLayer.iMbWidth as i32 * pCurLayer.iMbHeight as i32) as u32;
    let iAvMBNormalizedRDcostDown = iFMECost / kiMbNum;
    if let Some(prep) = pCurLayer.pFeatureSearchPreparation.as_deref_mut() {
        UpdateFMEGoodFrameCount(iAvMBNormalizedRDcostDown, &mut prep.uiFMEGoodFrameCount);
    }
}

/// Intentional no-op motion estimation FME switch callback.
/// Matches `void UpdateFMESwitchNull (SDqLayer* pCurLayer)` in `svc_motion_estimate.cpp:1059`.
pub fn UpdateFMESwitchNull(_pCurLayer: &mut SDqLayer) {}

// ============================================================================
// Feature Storage Dynamic Allocation & Deallocation
// ============================================================================

/// `SFeatureSearchPreparation` — `svc_enc_frame.h:59-69`. One per encoder, on the
/// last DQ layer (`encoder_ext.cpp:1125-1135`), screen content only.
///
/// **Re-ported at P10.1.B4.** T6.D2 deleted it with `RequestFeatureSearchPreparation`
/// / `ReleaseFeatureSearchPreparation` (S18: nothing called them while `InitDqLayers`
/// refused screen content two lines earlier); `InitDqLayers` builds it again now,
/// and `Drop` is the release (`encoder_ext.cpp:973-977`).
///
/// `pRefBlockFeature` is not carried: the C++ writes it (`encoder_ext.cpp:2743`)
/// and nothing reads it. `pFeatureOfBlock` is the per-frame scratch that every
/// reference's `CalculateFeatureOfBlock` fills (D-scc-3) — the C++ stores its
/// *address* into the reference's storage (`pFeatureOfBlockPointer`) and reads it
/// back only inside that function; here it travels as `&mut [u16]` at the call.
///
/// `UpdateFMESwitch`, `CountFMECostDown` and `UpdateFMEGoodFrameCount`, deleted at
/// the same time, are P10.3's, with the dispatch block that installs them.
#[derive(Debug)]
pub struct SFeatureSearchPreparation {
    /// Feature of every block (8x8), begin with the point — `svc_enc_frame.h:62`.
    pub pFeatureOfBlock: Vec<u16>,
    /// index of hash strategy
    pub uiFeatureStrategyIndex: u8,
    /* for FME frame-level switch */
    pub bFMESwitchFlag: bool,
    pub uiFMEGoodFrameCount: u8,
    pub iHighFreMbCount: i32,
}

impl SFeatureSearchPreparation {
    /// `RequestFeatureSearchPreparation` — `svc_motion_estimate.cpp:648-672`, with
    /// the `WelsMallocz` as a `Vec`.
    pub fn new(kiFrameWidth: i32, kiFrameHeight: i32, iNeedFeatureStorage: i32) -> Self {
        let kiFeatureStrategyIndex = (iNeedFeatureStorage >> 16) as u8;
        let bFme8x8 = (iNeedFeatureStorage & 0x0000FF & ME_FME as i32) == ME_FME as i32;
        let kiMarginSize = if bFme8x8 { 8 } else { 16 };
        let w = (kiFrameWidth - kiMarginSize).max(0) as usize;
        let h = (kiFrameHeight - kiMarginSize).max(0) as usize;
        // The C++ sizes in bytes: `sizeof(uint16_t) * kiFrameSize` for strategy 0,
        // plus `(kiFrameWidth - kiMarginSize) * sizeof(uint32_t) + kiFrameWidth * 8`
        // for any other strategy (never taken: `FME_DEFAULT_FEATURE_INDEX` is 0).
        // Same lengths, in `u16`s.
        let len = if kiFeatureStrategyIndex == 0 {
            w * h
        } else {
            w * h + 2 * w + 4 * kiFrameWidth.max(0) as usize
        };
        Self {
            pFeatureOfBlock: vec![0u16; len],
            uiFeatureStrategyIndex: kiFeatureStrategyIndex,
            bFMESwitchFlag: true,
            uiFMEGoodFrameCount: FMESWITCH_DEFAULT_GOODFRAME_NUM,
            iHighFreMbCount: 0,
        }
    }
}

// `RequestScreenBlockFeatureStorage` / `ReleaseScreenBlockFeatureStorage`
// (`svc_motion_estimate.cpp:683-754`) stood here until T7.C6 deleted them as the
// last `WelsMallocz`/`WelsFree` sites in `src/encoder`. The allocator is
// `SScreenBlockFeatureStorage::for_frame` (`picture.rs`); its call site is
// `AllocPicture` (`wels_preprocess.rs`), as `picture_handle.cpp:115` calls it, from
// P10.1.B5; the release is `Drop`.

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_constants_and_tables() {
        assert_eq!(CAMERA_STARTMV_RANGE, 64);
        assert_eq!(ITERATIVE_TIMES, 16);
        assert_eq!(CAMERA_MV_RANGE, 80);
        assert_eq!(CAMERA_MVD_RANGE, 162);
        assert_eq!(BASE_MV_MB_NMB, 9);
        assert_eq!(EXPANDED_MV_RANGE, 504);
        assert_eq!(EXPANDED_MVD_RANGE, 1010);
        assert_eq!(QStepx16ByQp[0], 10);
        assert_eq!(QStepx16ByQp[51], 3584);
    }

    #[test]
    fn test_mv_range_checks() {
        let mv_min = SMVUnitXY::new(-80, -80);
        let mv_max = SMVUnitXY::new(80, 80);

        assert!(CheckMvInRange(SMVUnitXY::new(0, 0), mv_min, mv_max));
        assert!(CheckMvInRange(SMVUnitXY::new(-80, 79), mv_min, mv_max));
        assert!(!CheckMvInRange(SMVUnitXY::new(80, 0), mv_min, mv_max));
        assert!(!CheckMvInRange(SMVUnitXY::new(-81, 0), mv_min, mv_max));
    }

    #[test]
    fn test_cost_mvd_computation() {
        let table_data = [10u16, 20, 30, 40, 50, 60, 70, 80];
        // S5.C4b: the raw base pointer is a cursor parked at index 0 — the same
        // two entries the raw form read, now without the `unsafe`.
        let cost = COST_MVD(MvdCostCursor::new(&table_data, 0), 2, 5);
        assert_eq!(cost, 30 + 60);
    }

    #[test]
    fn test_single_block_sums() {
        // S11.6: the kernels take slices, so this instrument needs no `unsafe`
        // and its allow retires with them.
        let buf8 = [1u8; 64];
        assert_eq!(SumOf8x8SingleBlock_c(&buf8, 8), 64);

        let buf16 = [2u8; 256];
        assert_eq!(SumOf16x16SingleBlock_c(&buf16, 16), 512);
    }

    #[test]
    fn test_me_sad_cost_select() {
        let sad_costs = [100i32, 50, 120, 80];
        let mvd_cost_table = vec![0u16; 512];
        let mut best_cost = 200i32;
        let mut ix = 0i32;
        let mut iy = 0i32;

        {
            let stop = WelsMeSadCostSelect(
                &sad_costs,
                MvdCostCursor::new(&mvd_cost_table, 256),
                &mut best_cost,
                0,
                0,
                &mut ix,
                &mut iy,
            );
            assert!(!stop);
            assert_eq!(best_cost, 50);
            assert_eq!(ix, 0);
            assert_eq!(iy, -1);
        }
    }

    /// **The screen-content family's first test, and its only referee (S6.B1).**
    ///
    /// No sweep row reaches a line of this code: `SPicture::pScreenBlockFeatureStorage`
    /// is never filled (F229), `AllocPicture` refuses `iNeedFeatureStorage != 0`, and
    /// `PerformFMEPreprocess` has no call site. So the byte gate proved nothing about
    /// the S6.B1 conversion, and this is what stands in for it — the storage is
    /// constructible by hand now, which is the point of owning its buffers.
    ///
    /// What it asserts is the arena's three structural invariants, which are exactly
    /// the properties the pointer-to-index rewrite could have broken:
    ///
    /// 1. the histogram sums to the number of block positions — every block counted once;
    /// 2. each value's write cursor ends exactly `2 * times[value]` past its base —
    ///    the groups tile the arena with no overlap and no gap, which is what
    ///    `InitializeHashforFeature_c`'s running offset and
    ///    `FillQpelLocationByFeatureValue_c`'s `+ 2` advance have to agree on;
    /// 3. every written position is inside the arena and inside the frame, in qpel units.
    #[test]
    fn feature_storage_arena_invariants_hold_over_a_synthetic_frame() {
        const W: i32 = 48;
        const H: i32 = 32;
        const MARGIN: i32 = 8; // the 8x8 feature mode's edge discard

        let mut func_list = SWelsFuncPtrList::default();
        WelsInitMeFunc(&mut func_list, 0, true);

        // A deterministic, non-flat luma: a flat one puts every block in bucket 0 and
        // would satisfy all three invariants without ever exercising a second group.
        let mut pic = SPicture::new(W, H, false);
        {
            let luma = pic.plane_mut(0);
            for y in 0..H as isize {
                for x in 0..W as isize {
                    luma.set(x, y, (((x * 7) ^ (y * 13)) & 0xFF) as u8);
                }
            }
        }

        let bw = (W - MARGIN) as usize;
        let bh = (H - MARGIN) as usize;
        let mut storage = SScreenBlockFeatureStorage::for_frame(W, H, true, 0);
        // D-scc-3: the layer's scratch, passed through rather than aliased.
        let mut feature_of_block = vec![0u16; bw * bh];

        assert!(CalculateFeatureOfBlock(
            &FmeKernels::of(&func_list),
            &pic,
            &mut feature_of_block,
            &mut storage
        ));

        let list_size = storage.iActualListSize as usize;
        assert_eq!(storage.pTimesOfFeatureValue.len(), list_size);
        assert_eq!(storage.pLocationOfFeature.len(), list_size);
        assert_eq!(storage.pLocationPointer.len(), 2 * bw * bh);

        // (1) every block position counted exactly once
        let counted: u64 = storage.pTimesOfFeatureValue.iter().map(|&n| n as u64).sum();
        assert_eq!(counted, (bw * bh) as u64, "histogram must sum to the block count");

        // the frame is not flat, so more than one bucket is populated — otherwise (2)
        // and (3) would hold vacuously for a single group starting at 0
        let populated = storage.pTimesOfFeatureValue.iter().filter(|&&n| n > 0).count();
        assert!(populated > 1, "synthetic frame collapsed to {populated} bucket(s)");

        // (2) the groups tile the arena: base_{v+1} == base_v + 2*times_v, and each
        //     cursor finished exactly at its own group's end
        let mut expect_base = 0usize;
        for v in 0..list_size {
            let base = storage.pLocationOfFeature[v];
            let times = storage.pTimesOfFeatureValue[v] as usize;
            assert_eq!(base, expect_base, "group {v} does not start where {} ended", v.wrapping_sub(1));
            assert_eq!(
                storage.pFeatureValuePointerList[v], base + 2 * times,
                "group {v}'s cursor did not end 2*{times} past its base",
            );
            expect_base = base + 2 * times;
        }
        assert_eq!(expect_base, 2 * bw * bh, "the groups must fill the arena exactly");

        // (3) every written position is a legal qpel coordinate of this frame
        for v in 0..list_size {
            let base = storage.pLocationOfFeature[v];
            let times = storage.pTimesOfFeatureValue[v] as usize;
            for k in 0..times {
                let qx = storage.pLocationPointer[base + 2 * k] as i32;
                let qy = storage.pLocationPointer[base + 2 * k + 1] as i32;
                assert_eq!(qx & 3, 0, "x qpel {qx} is not a whole pixel (x << 2)");
                assert_eq!(qy & 3, 0, "y qpel {qy} is not a whole pixel");
                assert!(qx >> 2 < bw as i32, "x {} outside {bw} block columns", qx >> 2);
                assert!(qy >> 2 < bh as i32, "y {} outside {bh} block rows", qy >> 2);
            }
        }

        // and the feature each position was filed under is the one the block carries
        for y in 0..bh {
            for x in 0..bw {
                let v = feature_of_block[y * bw + x] as usize;
                assert!(v < list_size, "feature {v} outside a list of {list_size}");
            }
        }
    }

    /// `RequestFeatureSearchPreparation` at the harness geometry with the screen
    /// content's fixed `kiNeedFeatureStorage` (`0x0307`: FME on 8x8, so margin 8).
    #[test]
    fn feature_search_preparation_sizes_and_flags_match_the_cpp() {
        let prep = SFeatureSearchPreparation::new(320, 192, 0x0307);
        assert_eq!(prep.pFeatureOfBlock.len(), 312 * 184);
        assert_eq!(prep.uiFeatureStrategyIndex, 0);
        assert!(prep.bFMESwitchFlag);
        assert_eq!(prep.uiFMEGoodFrameCount, FMESWITCH_DEFAULT_GOODFRAME_NUM);
        assert_eq!(prep.uiFMEGoodFrameCount, 2);
        assert_eq!(prep.iHighFreMbCount, 0);
        // 16x16 FME instead: margin 16.
        let prep16 = SFeatureSearchPreparation::new(320, 192, 0x0703);
        assert_eq!(prep16.pFeatureOfBlock.len(), 304 * 176);
    }

    #[test]
    fn test_fme_switch_flag() {
        assert!(CalcFMESwitchFlag(2, 0, 40, false));
        assert!(!CalcFMESwitchFlag(0, 0, 40, false));
        assert!(CalcFMESwitchFlag(0, 0, 10, true));
    }

    /// `SetMeMethod` — every one of the C++'s five cases, by function address.
    #[test]
    fn set_me_method_aims_the_slot_at_the_named_family() {
        let eq = |slot: &Option<PSearchMethodFunc>, want: PSearchMethodFunc| {
            std::ptr::fn_addr_eq(slot.expect("the slot is filled on every arm"), want)
        };
        let mut slot: Option<PSearchMethodFunc> = None;

        assert!(SetMeMethod(ME_DIA, &mut slot));
        assert!(eq(&slot, WelsDiamondSearch as PSearchMethodFunc));
        assert!(SetMeMethod(ME_CROSS, &mut slot));
        assert!(eq(&slot, WelsMotionCrossSearch as PSearchMethodFunc));
        assert!(SetMeMethod(ME_DIA_CROSS, &mut slot));
        assert!(eq(&slot, WelsDiamondCrossSearch as PSearchMethodFunc));
        assert!(SetMeMethod(ME_DIA_CROSS_FME, &mut slot));
        assert!(eq(&slot, WelsDiamondCrossFeatureSearch as PSearchMethodFunc));

        // The two `false` cases: `ME_FULL`, and anything else. Both still fill
        // the slot — with the diamond search — which is why the C++'s callers
        // only log a warning.
        slot = None;
        assert!(!SetMeMethod(ME_FULL, &mut slot));
        assert!(eq(&slot, WelsDiamondSearch as PSearchMethodFunc));
        slot = None;
        assert!(!SetMeMethod(ME_FME, &mut slot));
        assert!(eq(&slot, WelsDiamondSearch as PSearchMethodFunc));
    }

    #[test]
    fn test_fme_noop_callback() {
        // **S6.A1**: the callback took `*mut SDqLayer` and this test passed null to
        // say "the no-op ignores its layer". The parameter is `&SDqLayer` now, so
        // null is unrepresentable and the test says the same thing with a real
        // layer — that the body is empty, which is the whole claim. S11.29: the
        // callee is a safe `fn`, so the block and its instrument allow retire.
        // **D-scc-13**: the parameter is `&mut SDqLayer` now, so the claim is
        // the same one with an exclusive borrow — the body writes nothing.
        let mut layer = crate::encoder::svc_encode_slice::SDqLayer::default();
        UpdateFMESwitchNull(&mut layer);
    }

    /// A layer with two coded slices of known `uiSliceFMECostDown`, a 4x3
    /// macroblock grid and a preparation — the shape `UpdateFMESwitch` reads.
    fn fme_switch_layer(
        costs: &[u32],
        uiFMEGoodFrameCount: u8,
    ) -> crate::encoder::svc_encode_slice::SDqLayer {
        use crate::encoder::svc_encode_slice::{SDqLayer, SSlice, SliceIdx};
        let mut layer = SDqLayer::default();
        layer.iMbWidth = 4;
        layer.iMbHeight = 3;
        layer.sSliceBufferInfo[0].pSliceBuffer =
            costs.iter().map(|_| SSlice::default()).collect();
        for (i, &c) in costs.iter().enumerate() {
            layer.sSliceBufferInfo[0].pSliceBuffer[i].uiSliceFMECostDown = c;
            layer.ppSliceInLayer.push(SliceIdx { bank: 0, offset: i as i32 });
        }
        layer
            .sSliceEncCtx
            .iSliceNumInFrame
            .store(costs.len() as i32, std::sync::atomic::Ordering::Relaxed);
        let mut prep = SFeatureSearchPreparation::new(64, 48, 0x0307);
        prep.uiFMEGoodFrameCount = uiFMEGoodFrameCount;
        layer.pFeatureSearchPreparation = Some(Box::new(prep));
        layer
    }

    /// `UpdateFMEGoodFrameCount` — `svc_motion_estimate.cpp:1043-1052`: both ends
    /// of `[0, 5]` and both sides of the threshold (2).
    #[test]
    fn update_fme_good_frame_count_saturates_at_both_ends() {
        let step = |cost: u32, from: u8| {
            let mut n = from;
            UpdateFMEGoodFrameCount(cost, &mut n);
            n
        };
        // strictly above the threshold: up, and stuck at the maximum
        assert_eq!(step(3, 2), 3);
        assert_eq!(step(3, FMESWITCH_GOODFRAMECOUNT_MAX), FMESWITCH_GOODFRAMECOUNT_MAX);
        assert_eq!(step(u32::MAX, 4), 5);
        // at or below it: down, and stuck at zero (no `u8` underflow)
        assert_eq!(step(FMESWITCH_MBAVERCOSTSAVING_THRESHOLD, 2), 1);
        assert_eq!(step(0, 1), 0);
        assert_eq!(step(0, 0), 0);
    }

    /// `UpdateFMESwitch` — `svc_motion_estimate.cpp:1054-1058`: the sum over the
    /// layer's coded slices, divided by the macroblock count, decides the step.
    #[test]
    fn update_fme_switch_sums_the_slices_and_steps_the_count() {
        // 12 macroblocks; 20 + 25 = 45; 45 / 12 = 3 > 2, so the count goes up.
        let mut layer = fme_switch_layer(&[20, 25], 2);
        UpdateFMESwitch(&mut layer);
        assert_eq!(
            layer.pFeatureSearchPreparation.as_ref().unwrap().uiFMEGoodFrameCount,
            3
        );

        // 12 + 12 = 24; 24 / 12 = 2, which is *not* `> 2` — the count goes down.
        let mut layer = fme_switch_layer(&[12, 12], 2);
        UpdateFMESwitch(&mut layer);
        assert_eq!(
            layer.pFeatureSearchPreparation.as_ref().unwrap().uiFMEGoodFrameCount,
            1
        );

        // `iSliceNumInFrame` is the bound, not the bank's length: a third slice
        // the frame has not coded is not summed. 20 + 25 = 45 again.
        let mut layer = fme_switch_layer(&[20, 25, 1000], 2);
        layer
            .sSliceEncCtx
            .iSliceNumInFrame
            .store(2, std::sync::atomic::Ordering::Relaxed);
        UpdateFMESwitch(&mut layer);
        assert_eq!(
            layer.pFeatureSearchPreparation.as_ref().unwrap().uiFMEGoodFrameCount,
            3
        );
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static ME_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

// WELS_CPU_* flags: one definition, in `common/cpu_core.rs`. The copies that
// used to live in this module disagreed with cpu_core.h and with each other --
// WELS_CPU_NEON alone had seven distinct values across eight modules.
pub use crate::common::cpu_core::{WELS_CPU_LSX, WELS_CPU_NEON, WELS_CPU_SSE2, WELS_CPU_SSE41};
