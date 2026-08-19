//! Port of `codec/encoder/core/src/svc_base_layer_md.cpp` — the base-layer
//! mode-decision layer.
//!
//! This module carries the **intra (I-slice) half**: the tables, the neighbour-mode
//! predictor, and the `WelsMdIntraInit` -> `WelsMdIntraMb` chain that
//! `WelsISliceMdEnc` (`svc_encode_slice.cpp:562`/`:566`) drives, plus
//! `WelsMdInterInit`, `WelsMdInterMbRefinement` and `WelsMdFirstIntraMode`.
//!
//! The rest of the C++ file's inter half is ported too, but lives elsewhere:
//! `WelsMdP16x16`, `WelsMdBackgroundMbEnc` and `WelsMdInterSecondaryModesEnc` in
//! `svc_mode_decision.rs`, and `WelsMdInterMbLoop` in `svc_encode_slice.rs`. All of
//! it is exercised — P slices are byte-exact across every sweep. (This header said
//! "the inter half of the file is still unported" until Phase 5.4; that was stale.)
//!
//! ## Deviation: the `Combined3` SIMD fast paths are not translated
//!
//! `WelsMdI16x16`, `WelsMdI4x4` and `WelsMdIntraChroma` each open with a branch taken
//! only when the corresponding `sSampleDealingFuncs.pfIntra*Combined3` slot is
//! non-null. Those slots are set exclusively from SIMD kernels in `sample.cpp`
//! (`_sse*`, `_neon`, `_AArch64_neon`, `_mmi`, `_lasx`), all of them behind a
//! `uiCpuFlag` test. Measured on this machine against `libopenh264.a`:
//! `WelsCPUFeatureDetect` returns `0x00000000`, so `WelsInitSampleSadFunc` leaves all
//! five `pfIntra*Combined3*` pointers NULL and the C++ reference takes the scalar
//! branch. This port has no SIMD kernels at all, so the slots are always NULL here
//! too. The scalar branches below are therefore the ones that decide output bytes.
//!
//! Rather than silently ignore the fast branch, each of the three functions asserts
//! the slot is null and panics with an explicit message if it ever is not — see
//! `assert_no_combined3`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code)]

// Phase 4a: MC is called directly, not via `sMcFuncs`.
use crate::common::mc::{McChroma_c, McLuma_c};
use crate::encoder::encoder_context::{sWelsEncCtx, SMVComponentUnit, SMVUnitXY};
use crate::encoder::md::{
    FillNeighborCacheIntra, InitMeRefinePointer, MdIntraAnalysisVaaInfo, MeRefineFracPixel, SMB,
    SMbCache, SMeRefinePointer, SWelsMD, BsSizeUE, MB_TYPE_16x16, MB_TYPE_16x8, MB_TYPE_8x16,
    MB_TYPE_8x8, MB_TYPE_INTRA16x16, MB_TYPE_INTRA4x4, MB_TYPE_SKIP,
    ME_REFINE_BUF_STRIDE_BLK4, ME_REFINE_BUF_STRIDE_BLK8, ME_REFINE_BUF_WIDTH_BLK4,
    ME_REFINE_BUF_WIDTH_BLK8, PredictSad,
};
use crate::encoder::svc_encode_mb::{WelsDctMb, WelsEncRecI4x4Y, WelsTryPUVskip, WelsTryPYskip};
use crate::encoder::svc_encode_slice::{SDqLayer, SSlice};
use crate::encoder::svc_mode_decision::{
    g_kiIntra16AvaliMode, g_kiMapModeI16x16, g_kuiMbCountScan4Idx, update_P8x16_motion_info,
    InitMe, PredInter16x8Mv, PredInter8x16Mv, PredMv, PredSkipMv, UpdateP16x16MotionInfo,
    UpdateP16x8Motion2Cache, UpdateP16x8MotionInfo, UpdateP4x4Motion2Cache, UpdateP4x4MotionInfo,
    UpdateP4x8Motion2Cache, UpdateP4x8MotionInfo, UpdateP8x16Motion2Cache, UpdateP8x4Motion2Cache,
    UpdateP8x4MotionInfo, UpdateP8x8MotionInfo, WelsMdInterDecidedPskip, WelsMdInterJudgePskip,
    WelsMdInterSecondaryModesEnc, WelsMdIntraSecondaryModesEnc, BLOCK_16x16, BLOCK_16x8,
    BLOCK_4x4, BLOCK_4x8, BLOCK_8x16, BLOCK_8x4, BLOCK_8x8, IS_SKIP, MB_TYPE_BACKGROUND,
    REF_NOT_AVAIL, SUB_MB_TYPE_4x4, SUB_MB_TYPE_4x8, SUB_MB_TYPE_8x4, SUB_MB_TYPE_8x8,
};
use crate::encoder::svc_motion_estimate::{SetMvWithinIntegerMvRange, SWelsME};
use crate::encoder::svc_set_mb_syn_cavlc::{g_kuiCache48CountScan4Idx, IS_INTRA16x16};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::common::wels_common_defs::EWelsSliceType;
use crate::encoder::md::{LEFT_MB_POS, TOPLEFT_MB_POS, TOPRIGHT_MB_POS, TOP_MB_POS};
use crate::encoder::picture::SScreenBlockFeatureStorage;

/// `wels_const.h:112` — `MB_WIDTH_LUMA`/`MB_WIDTH_CHROMA`, the per-macroblock advance
/// applied to the cached plane pointers when walking right along a macroblock row.
const MB_WIDTH_LUMA: usize = 16;
const MB_WIDTH_CHROMA: usize = 8;

// ============================================================================
// Intra prediction mode ids — `wels_common_defs.h:329-370`
// ============================================================================

pub const I4_PRED_INVALID: i8 = 0;
pub const I4_PRED_V: i8 = 0;
pub const I4_PRED_H: i8 = 1;
pub const I4_PRED_DC: i8 = 2;
pub const I4_PRED_DDL: i8 = 3;
pub const I4_PRED_DDR: i8 = 4;
pub const I4_PRED_VR: i8 = 5;
pub const I4_PRED_HD: i8 = 6;
pub const I4_PRED_VL: i8 = 7;
pub const I4_PRED_HU: i8 = 8;
pub const I4_PRED_DC_L: i8 = 9;
pub const I4_PRED_DC_T: i8 = 10;
pub const I4_PRED_DC_128: i8 = 11;
pub const I4_PRED_DDL_TOP: i8 = 12;
pub const I4_PRED_VL_TOP: i8 = 13;

pub const C_PRED_INVALID: i8 = -1;
pub const C_PRED_DC: i8 = 0;
pub const C_PRED_H: i8 = 1;
pub const C_PRED_V: i8 = 2;
pub const C_PRED_P: i8 = 3;
pub const C_PRED_DC_L: i8 = 4;
pub const C_PRED_DC_T: i8 = 5;
pub const C_PRED_DC_128: i8 = 6;

// ============================================================================
// Tables — `svc_base_layer_md.cpp:48-244`
// ============================================================================

/// `svc_base_layer_md.cpp:59`. `I4_PRED_MODE_EXTEND` is never defined anywhere in
/// `codec/` — it appears only in the `#ifndef` guards — so the `#ifndef` arm is the
/// live one here and in `g_kiIntra4AvailMode` below.
pub const g_kiIntra4AvailCount: [u8; 16] =
    [1, 3, 2, 4, 1, 3, 2, 7, 1, 3, 4, 6, 1, 3, 4, 9];

/// `svc_base_layer_md.cpp:68`. Indexed by
/// `left_avail | (top_avail<<1) | (left_top_avail<<2) | (right_top_avail<<3)`.
pub const g_kiIntra4AvailMode: [[i8; 16]; 16] = [
    // 0000
    [
        I4_PRED_DC_128, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0001
    [
        I4_PRED_DC_L, I4_PRED_H, I4_PRED_HU, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0010
    [
        I4_PRED_DC_T, I4_PRED_V, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0011
    [
        I4_PRED_DC, I4_PRED_H, I4_PRED_V, I4_PRED_HU,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0100
    [
        I4_PRED_DC_128, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0101
    [
        I4_PRED_DC_L, I4_PRED_H, I4_PRED_HU, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0110
    [
        I4_PRED_DC_T, I4_PRED_V, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 0111
    [
        I4_PRED_DC, I4_PRED_H, I4_PRED_V, I4_PRED_HU,
        I4_PRED_DDR, I4_PRED_VR, I4_PRED_HD, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1000
    [
        I4_PRED_DC_128, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1001
    [
        I4_PRED_DC_L, I4_PRED_H, I4_PRED_HU, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1010
    [
        I4_PRED_DC_T, I4_PRED_V, I4_PRED_DDL, I4_PRED_VL,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1011
    [
        I4_PRED_DC, I4_PRED_H, I4_PRED_V, I4_PRED_HU,
        I4_PRED_DDL, I4_PRED_VL, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1100
    [
        I4_PRED_DC_128, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1101
    [
        I4_PRED_DC_L, I4_PRED_H, I4_PRED_HU, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1110
    [
        I4_PRED_DC_T, I4_PRED_V, I4_PRED_DDL, I4_PRED_VL,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
    // 1111
    [
        I4_PRED_DC, I4_PRED_H, I4_PRED_V, I4_PRED_HU,
        I4_PRED_DDL, I4_PRED_VL, I4_PRED_DDR, I4_PRED_VR,
        I4_PRED_HD, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
        I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID, I4_PRED_INVALID,
    ],
];

/// `svc_base_layer_md.cpp:200`.
pub const g_kiIntraChromaAvailMode: [[i8; 5]; 8] = [
    [C_PRED_DC_128, C_PRED_INVALID, C_PRED_INVALID, C_PRED_INVALID, 1],
    [C_PRED_DC_L, C_PRED_H, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_DC_T, C_PRED_V, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_V, C_PRED_H, C_PRED_DC, C_PRED_INVALID, 3],
    [C_PRED_DC_128, C_PRED_INVALID, C_PRED_INVALID, C_PRED_INVALID, 1],
    [C_PRED_DC_L, C_PRED_H, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_DC_T, C_PRED_V, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_V, C_PRED_H, C_PRED_DC, C_PRED_P, 4],
];

/// `svc_base_layer_md.cpp:212`.
pub const g_kiCoordinateIdx4x4X: [i8; 16] =
    [0, 4, 0, 4, 8, 12, 8, 12, 0, 4, 0, 4, 8, 12, 8, 12];

/// `svc_base_layer_md.cpp:218`.
pub const g_kiCoordinateIdx4x4Y: [i8; 16] =
    [0, 0, 4, 4, 0, 0, 4, 4, 8, 8, 12, 12, 8, 8, 12, 12];

/// `svc_base_layer_md.cpp:223`. Maps `uiNeighborIntra` and the 4x4 block index to the
/// availability code that indexes `g_kiIntra4AvailCount` / `g_kiIntra4AvailMode`.
pub const g_kiNeighborIntraToI4x4: [[i8; 16]; 16] = [
    [0, 1, 10, 7, 1, 1, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [1, 1, 15, 7, 1, 1, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [10, 15, 10, 7, 15, 7, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [11, 15, 15, 7, 15, 7, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [4, 1, 10, 7, 1, 1, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [5, 1, 15, 7, 1, 1, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [14, 15, 10, 7, 15, 7, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [15, 15, 15, 7, 15, 7, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [0, 1, 10, 7, 1, 9, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [1, 1, 15, 7, 1, 9, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [10, 15, 10, 7, 15, 15, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [11, 15, 15, 7, 15, 15, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [4, 1, 10, 7, 1, 9, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [5, 1, 15, 7, 1, 9, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
    [14, 15, 10, 7, 15, 15, 15, 7, 10, 15, 10, 7, 15, 7, 15, 7],
    [15, 15, 15, 7, 15, 15, 15, 7, 15, 15, 15, 7, 15, 7, 15, 7],
];

/// `svc_base_layer_md.cpp:242`. Folds the six "restricted" I4x4 mode ids
/// (`DC_L`, `DC_T`, `DC_128`, `DDL_TOP`, `VL_TOP`) back onto the nine coded ones.
pub const g_kiMapModeI4x4: [i8; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 2, 2, 2, 3, 7];

// ============================================================================
// Helpers
// ============================================================================

// `assert_no_combined3` was here, guarding the three untranslated `Combined3` SIMD
// fast paths against a slot that was never non-null: the reference leaves them NULL
// on every target this port builds for and the port never assigned them. The eight
// `*mut c_void` fields are deleted (S18, Phase 6 session B) and there is nothing
// left to guard — the scalar branch each guard protected is now unconditional.

/// `svc_base_layer_md.cpp:246`.
pub unsafe fn PredIntra4x4Mode(pIntraPredMode: *const i8, iIdx4: i32) -> i32 {
    let iTopMode = *pIntraPredMode.offset(iIdx4 as isize - 8);
    let iLeftMode = *pIntraPredMode.offset(iIdx4 as isize - 1);

    let iBestMode: i8 = if -1 == iLeftMode || -1 == iTopMode {
        2
    } else {
        iLeftMode.min(iTopMode)
    };
    iBestMode as i32
}

// ============================================================================
// Intra mode decision
// ============================================================================

/// `svc_base_layer_md.cpp:259`. Re-points the cached per-macroblock plane pointers and
/// reloads the intra neighbour cache. Called once per macroblock by `WelsISliceMdEnc`
/// *before* the re-encoding loop, so it must not depend on the QP.
///
/// # Safety
/// `pEncCtx`, `pCurMb` and `pMbCache` must be valid, and `pEncCtx->pCurDqLayer` must
/// have `pDecPic` and the `pEncData`/`pCsData` planes installed.
pub unsafe fn WelsMdIntraInit(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = (*pEncCtx).pCurDqLayer;

    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbXY = (*pCurMb).iMbXY;

    // step 3. locating current pEnc and pDec
    // unroll loops here
    if 0 == kiMbX || iSliceFirstMbXY == kiMbXY {
        let mut iStrideY = (*pCurLayer).iEncStride[0];
        let mut iStrideUV = (*pCurLayer).iEncStride[1];
        let mut iOffsetY = (kiMbX + kiMbY * iStrideY) << 4;
        let mut iOffsetUV = (kiMbX + kiMbY * iStrideUV) << 3;
        (*pMbCache).SPicData.pEncMb[0] = (*pCurLayer).pEncData[0].offset(iOffsetY as isize);
        (*pMbCache).SPicData.pEncMb[1] = (*pCurLayer).pEncData[1].offset(iOffsetUV as isize);
        (*pMbCache).SPicData.pEncMb[2] = (*pCurLayer).pEncData[2].offset(iOffsetUV as isize);

        iStrideY = (*pCurLayer).iCsStride[0];
        iStrideUV = (*pCurLayer).iCsStride[1];
        iOffsetY = (kiMbX + kiMbY * iStrideY) << 4;
        iOffsetUV = (kiMbX + kiMbY * iStrideUV) << 3;
        (*pMbCache).SPicData.pCsMb[0] = (*pCurLayer).pCsData[0].offset(iOffsetY as isize);
        (*pMbCache).SPicData.pCsMb[1] = (*pCurLayer).pCsData[1].offset(iOffsetUV as isize);
        (*pMbCache).SPicData.pCsMb[2] = (*pCurLayer).pCsData[2].offset(iOffsetUV as isize);

        let pDecPic = (*pCurLayer).pDecPic;
        iStrideY = (*pDecPic).iLineSize[0];
        iStrideUV = (*pDecPic).iLineSize[1];
        iOffsetY = (kiMbX + kiMbY * iStrideY) << 4;
        iOffsetUV = (kiMbX + kiMbY * iStrideUV) << 3;
        (*pMbCache).SPicData.pDecMb[0] = (*pDecPic).pData[0].offset(iOffsetY as isize);
        (*pMbCache).SPicData.pDecMb[1] = (*pDecPic).pData[1].offset(iOffsetUV as isize);
        (*pMbCache).SPicData.pDecMb[2] = (*pDecPic).pData[2].offset(iOffsetUV as isize);
    } else {
        (*pMbCache).SPicData.pEncMb[0] = (*pMbCache).SPicData.pEncMb[0].add(MB_WIDTH_LUMA);
        (*pMbCache).SPicData.pEncMb[1] = (*pMbCache).SPicData.pEncMb[1].add(MB_WIDTH_CHROMA);
        (*pMbCache).SPicData.pEncMb[2] = (*pMbCache).SPicData.pEncMb[2].add(MB_WIDTH_CHROMA);

        (*pMbCache).SPicData.pDecMb[0] = (*pMbCache).SPicData.pDecMb[0].add(MB_WIDTH_LUMA);
        (*pMbCache).SPicData.pDecMb[1] = (*pMbCache).SPicData.pDecMb[1].add(MB_WIDTH_CHROMA);
        (*pMbCache).SPicData.pDecMb[2] = (*pMbCache).SPicData.pDecMb[2].add(MB_WIDTH_CHROMA);

        (*pMbCache).SPicData.pCsMb[0] = (*pMbCache).SPicData.pCsMb[0].add(MB_WIDTH_LUMA);
        (*pMbCache).SPicData.pCsMb[1] = (*pMbCache).SPicData.pCsMb[1].add(MB_WIDTH_CHROMA);
        (*pMbCache).SPicData.pCsMb[2] = (*pMbCache).SPicData.pCsMb[2].add(MB_WIDTH_CHROMA);
    }

    //step 2. initial pWelsMd
    (*pCurMb).uiCbp = 0;

    //step 4: locating scaled_tcoeff

    //step 1. load neighbor cache
    FillNeighborCacheIntra(pMbCache, pCurMb, (*pCurLayer).iMbWidth as i32);
    // in WelsMdI16x16() will be changed, so re-init here!
    (*pMbCache).pMemPredLuma = (*pMbCache).pMemPredMb;
    // Init with default, maybe change in WelsMdI16x16 and svc_md_i16x16_sad
    (*pMbCache).pMemPredChroma = (*pMbCache).pMemPredMb.add(256);
}

/// `svc_base_layer_md.cpp:418`. The full 16-mode-per-block I4x4 search, used on the
/// non-`LOW_COMPLEXITY` path via [`WelsMdIntraFinePartition`].
///
/// # Safety
/// See [`WelsMdIntraInit`]; additionally `pMbCache` must have `pMemPredBlk4`,
/// `pPrevIntra4x4PredModeFlag` and `pRemIntra4x4PredModeFlag` allocated.
pub unsafe extern "C" fn WelsMdI4x4(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> i32 {
    let pFunc = (*pEncCtx).pFuncList;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let iLambda = (*pWelsMd).iLambda;
    let iBestCostLuma = (*pWelsMd).iCostLuma;
    let pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let pDecMb = (*pMbCache).SPicData.pCsMb[0];
    let kiLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let kiLineSizeDec = (*pCurDqLayer).iCsStride[0];

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let mut pPrevIntra4x4PredModeFlag = (*pMbCache).pPrevIntra4x4PredModeFlag;
    let mut pRemIntra4x4PredModeFlag = (*pMbCache).pRemIntra4x4PredModeFlag;
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[(*pMbCache).uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pfSatd4x4 = (*pFunc).sSampleDealingFuncs.pfSampleSatd[BLOCK_4x4].unwrap();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let iIdxStrideEnc = iCoordinateY * kiLineSizeEnc + iCoordinateX;
        let pCurEnc = pEncMb.offset(iIdxStrideEnc as isize);
        let iIdxStrideDec = iCoordinateY * kiLineSizeDec + iCoordinateX;
        let pCurDec = pDecMb.offset(iIdxStrideDec as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            (*pMbCache).iIntraPredMode.as_ptr(),
            g_kuiCache48CountScan4Idx[i] as i32,
        );

        //step 3: collect candidates of iPredMode
        let iAvailCount = g_kiIntra4AvailCount[kiOffset] as usize;
        let kpAvailMode = &g_kiIntra4AvailMode[kiOffset];

        //step 4: gain the best pred mode
        let mut iBestCost = i32::MAX;
        let mut iBestMode = kpAvailMode[0] as i32;

        for j in 0..iAvailCount {
            let iCurMode = kpAvailMode[j] as i32;
            debug_assert!((0..14).contains(&iCurMode));

            let pDst = (*pMbCache).pMemPredBlk4.offset(((1 - iBestPredBufferNum) << 4) as isize);

            (*pFunc).pfGetLumaI4x4Pred[iCurMode as usize].unwrap()(pDst, pCurDec, kiLineSizeDec);
            let iCurCost = pfSatd4x4(pDst, 4, pCurEnc, kiLineSizeEnc)
                + lambda[(iPredMode == g_kiMapModeI4x4[iCurMode as usize] as i32) as usize];

            if iCurCost < iBestCost {
                iBestMode = iCurMode;
                iBestCost = iCurCost;
                iBestPredBufferNum = 1 - iBestPredBufferNum;
            }
        }

        (*pMbCache).pBestPredI4x4Blk4 =
            (*pMbCache).pMemPredBlk4.offset((iBestPredBufferNum << 4) as isize);
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize] as i32;
        if iPredMode == iFinalMode {
            *pPrevIntra4x4PredModeFlag = true;
        } else {
            *pPrevIntra4x4PredModeFlag = false;
            *pRemIntra4x4PredModeFlag =
                (if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 }) as i8;
        }
        pPrevIntra4x4PredModeFlag = pPrevIntra4x4PredModeFlag.add(1);
        pRemIntra4x4PredModeFlag = pRemIntra4x4PredModeFlag.add(1);
        (*pMbCache).iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode as i8;

        //step 6: encoding I_4x4
        WelsEncRecI4x4Y(pEncCtx, pCurMb, pMbCache, i as u8);
    }

    StoreIntra4x4PredModeToMb(pCurMb, pMbCache);
    iCosti4x4 += (iLambda << 4) + (iLambda << 3); //4*6*lambda from JVT SATD0
    iCosti4x4
}

/// The `ST32`/`LD32` tail shared verbatim by `WelsMdI4x4` (`svc_base_layer_md.cpp:540`)
/// and `WelsMdI4x4Fast` (`:859`): publish the four right-column and three
/// bottom-row I4x4 prediction modes into the macroblock so the *next* macroblock's
/// `FillNeighborCacheIntra` can read them.
#[inline]
unsafe fn StoreIntra4x4PredModeToMb(pCurMb: *mut SMB, pMbCache: *mut SMbCache) {
    // ST32 (pCurMb->pIntra4x4PredMode, LD32 (&pMbCache->iIntraPredMode[33]));
    let pMbMode = &mut (*pCurMb).iIntra4x4PredMode;
    let pCacheMode = &(*pMbCache).iIntraPredMode;
    pMbMode[0..4].copy_from_slice(&pCacheMode[33..37]);
    (*pCurMb).iIntra4x4PredMode[4] = (*pMbCache).iIntraPredMode[12];
    (*pCurMb).iIntra4x4PredMode[5] = (*pMbCache).iIntraPredMode[20];
    (*pCurMb).iIntra4x4PredMode[6] = (*pMbCache).iIntraPredMode[28];
}

/// `svc_base_layer_md.cpp:548`. The `LOW_COMPLEXITY` I4x4 search: instead of scoring
/// every available mode it scores DC/H/V, then follows whichever of the vertical or
/// horizontal families won into at most four more modes.
///
/// # Safety
/// Same as [`WelsMdI4x4`].
pub unsafe extern "C" fn WelsMdI4x4Fast(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> i32 {
    let pFunc = (*pEncCtx).pFuncList;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let iLambda = (*pWelsMd).iLambda;
    let iBestCostLuma = (*pWelsMd).iCostLuma;
    let pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let pDecMb = (*pMbCache).SPicData.pCsMb[0];
    let kiLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let kiLineSizeDec = (*pCurDqLayer).iCsStride[0];

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let mut pPrevIntra4x4PredModeFlag = (*pMbCache).pPrevIntra4x4PredModeFlag;
    let mut pRemIntra4x4PredModeFlag = (*pMbCache).pRemIntra4x4PredModeFlag;
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[(*pMbCache).uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pfMdCost4x4 = (*pFunc).sSampleDealingFuncs.md_cost(BLOCK_4x4).unwrap();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let iIdxStrideEnc = iCoordinateY * kiLineSizeEnc + iCoordinateX;
        let pCurEnc = pEncMb.offset(iIdxStrideEnc as isize);
        let iIdxStrideDec = iCoordinateY * kiLineSizeDec + iCoordinateX;
        let pCurDec = pDecMb.offset(iIdxStrideDec as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            (*pMbCache).iIntraPredMode.as_ptr(),
            g_kuiCache48CountScan4Idx[i] as i32,
        ) as i8;
        //step 3: collect candidates of iPredMode
        let iAvailCount = g_kiIntra4AvailCount[kiOffset] as i32;
        let kpAvailMode = &g_kiIntra4AvailMode[kiOffset];

        let mut iBestMode: i8;
        let mut iBestCost: i32;

        // `lambda[iPredMode == g_kiMapModeI4x4[iCurMode]]`, hoisted so the mode-scoring
        // below reads like the C++ one-liner it is translating.
        macro_rules! score {
            ($mode:expr, $dst:expr) => {{
                let m: i8 = $mode;
                (*pFunc).pfGetLumaI4x4Pred[m as usize].unwrap()($dst, pCurDec, kiLineSizeDec);
                pfMdCost4x4($dst, 4, pCurEnc, kiLineSizeEnc)
                    + lambda[(iPredMode == g_kiMapModeI4x4[m as usize]) as usize]
            }};
        }
        macro_rules! alt_buf {
            () => {
                (*pMbCache).pMemPredBlk4.offset(((1 - iBestPredBufferNum) << 4) as isize)
            };
        }
        // `if (iCurCost < iBestCost) { best = cur; iBestPredBufferNum = 1 - …; }`
        macro_rules! take_if_better {
            ($mode:expr, $cost:expr) => {
                if $cost < iBestCost {
                    iBestMode = $mode;
                    iBestCost = $cost;
                    iBestPredBufferNum = 1 - iBestPredBufferNum;
                }
            };
        }

        if iAvailCount == 9 || iAvailCount == 7 {
            //I4_PRED_DC(2)
            iBestMode = I4_PRED_DC;
            let pDst = (*pMbCache).pMemPredBlk4.offset((iBestPredBufferNum << 4) as isize);
            iBestCost = score!(I4_PRED_DC, pDst);

            //I4_PRED_H(1)
            let iCostH = score!(I4_PRED_H, alt_buf!());
            take_if_better!(I4_PRED_H, iCostH);

            //I4_PRED_V(0)
            let iCostV = score!(I4_PRED_V, alt_buf!());
            take_if_better!(I4_PRED_V, iCostV);

            if iCostV < iCostH {
                if iAvailCount == 9 {
                    //indicating whether V is the best fake mode
                    let mut iBestModeFake = true;

                    //I4_PRED_VR(5) and I4_PRED_VL(7)
                    let iCostVR = score!(I4_PRED_VR, alt_buf!());
                    take_if_better!(I4_PRED_VR, iCostVR);
                    if iCostVR < iCostV {
                        iBestModeFake = false;
                    }

                    let iCostVL = score!(I4_PRED_VL, alt_buf!());
                    take_if_better!(I4_PRED_VL, iCostVL);
                    if iCostVL < iCostV {
                        iBestModeFake = false;
                    }

                    //Vertical Early Determination
                    if !iBestModeFake {
                        //Vertical is not the best, go on checking...
                        //select the best one from VL and VR
                        if iCostVR < iCostVL {
                            //I4_PRED_DDR(4)
                            let iCurCost = score!(I4_PRED_DDR, alt_buf!());
                            take_if_better!(I4_PRED_DDR, iCurCost);
                        } else {
                            //I4_PRED_DDL(3)
                            let iCurCost = score!(I4_PRED_DDL, alt_buf!());
                            take_if_better!(I4_PRED_DDL, iCurCost);
                        }
                    }
                } else if iAvailCount == 7 {
                    let iCurCost = score!(I4_PRED_DDR, alt_buf!());
                    take_if_better!(I4_PRED_DDR, iCurCost);

                    let iCurCost = score!(I4_PRED_VR, alt_buf!());
                    take_if_better!(I4_PRED_VR, iCurCost);
                }
            } else {
                //indicating whether H is the best fake mode
                let mut iBestModeFake = true;

                //I4_PRED_HD(6) and I4_PRED_HU(8)
                let iCostHD = score!(I4_PRED_HD, alt_buf!());
                take_if_better!(I4_PRED_HD, iCostHD);
                if iCostHD < iCostH {
                    iBestModeFake = false;
                }

                let iCostHU = score!(I4_PRED_HU, alt_buf!());
                take_if_better!(I4_PRED_HU, iCostHU);
                if iCostHU < iCostH {
                    iBestModeFake = false;
                }

                if !iBestModeFake {
                    //Horizontal is not the best, go on checking...
                    //select the best one from VL and VR
                    if iCostHD < iCostHU {
                        //I4_PRED_DDR(4)
                        let iCurCost = score!(I4_PRED_DDR, alt_buf!());
                        take_if_better!(I4_PRED_DDR, iCurCost);
                    } else if iAvailCount == 9 {
                        //I4_PRED_DDL(3)
                        let iCurCost = score!(I4_PRED_DDL, alt_buf!());
                        take_if_better!(I4_PRED_DDL, iCurCost);
                    }
                }
            }
        } else {
            iBestCost = i32::MAX;
            iBestMode = I4_PRED_INVALID;
            for j in 0..iAvailCount as usize {
                let iCurMode = kpAvailMode[j];
                let iCurCost = score!(iCurMode, alt_buf!());
                take_if_better!(iCurMode, iCurCost);
            }
        }

        (*pMbCache).pBestPredI4x4Blk4 =
            (*pMbCache).pMemPredBlk4.offset((iBestPredBufferNum << 4) as isize);
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize];
        if iPredMode == iFinalMode {
            *pPrevIntra4x4PredModeFlag = true;
        } else {
            *pPrevIntra4x4PredModeFlag = false;
            *pRemIntra4x4PredModeFlag =
                if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 };
        }
        pPrevIntra4x4PredModeFlag = pPrevIntra4x4PredModeFlag.add(1);
        pRemIntra4x4PredModeFlag = pRemIntra4x4PredModeFlag.add(1);
        (*pMbCache).iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode;
        //step 6: encoding I_4x4
        WelsEncRecI4x4Y(pEncCtx, pCurMb, pMbCache, i as u8);
    }

    StoreIntra4x4PredModeToMb(pCurMb, pMbCache);
    iCosti4x4 += (iLambda << 4) + (iLambda << 3); //4*6*lambda from JVT SATD0
    iCosti4x4
}

/// `svc_base_layer_md.cpp:867`. Picks the 8x8 chroma prediction mode over Cb and Cr
/// jointly and leaves the winning prediction in `pBestPredIntraChroma`.
///
/// # Safety
/// `pFunc`, `pCurDqLayer` and `pMbCache` must be valid, and `pMbCache->pMemPredChroma`
/// must point at the 256-byte ping-pong buffer `WelsMdI16x16` selected.
pub unsafe extern "C" fn WelsMdIntraChroma(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMbCache: *mut SMbCache,
    iLambda: i32,
) -> i32 {
    let mut iChmaIdx: usize = 0;
    let pPredIntraChma: [*mut u8; 2] =
        [(*pMbCache).pMemPredChroma, (*pMbCache).pMemPredChroma.add(128)];
    let mut pDstChma = pPredIntraChma[0];
    let pEncCb = (*pMbCache).SPicData.pEncMb[1];
    let pEncCr = (*pMbCache).SPicData.pEncMb[2];
    let pDecCb = (*pMbCache).SPicData.pCsMb[1];
    let pDecCr = (*pMbCache).SPicData.pCsMb[2];
    let kiLineSizeEnc = (*pCurDqLayer).iEncStride[1];
    let kiLineSizeDec = (*pCurDqLayer).iCsStride[1];

    let mut iBestCost = i32::MAX;

    let iOffset = ((*pMbCache).uiNeighborIntra & 0x07) as usize;
    let iAvailCount = g_kiIntraChromaAvailMode[iOffset][4] as i32;
    let kpAvailMode = &g_kiIntraChromaAvailMode[iOffset];

    let pfMdCost8x8 = (*pFunc).sSampleDealingFuncs.md_cost(BLOCK_8x8).unwrap();

    let mut iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount as usize {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        let pfChromaPred = (*pFunc).pfGetChromaPred[iCurMode as usize].unwrap();
        pfChromaPred(pDstChma, pDecCb, kiLineSizeDec); //Cb
        let mut iCurCost = pfMdCost8x8(pDstChma, 8, pEncCb, kiLineSizeEnc);

        pfChromaPred(pDstChma.add(64), pDecCr, kiLineSizeDec); //Cr
        iCurCost += pfMdCost8x8(pDstChma.add(64), 8, pEncCr, kiLineSizeEnc)
            + iLambda * BsSizeUE(crate::encoder::md::g_kiMapModeIntraChroma[iCurMode as usize] as u32) as i32;
        if iCurCost < iBestCost {
            iBestMode = iCurMode;
            iBestCost = iCurCost;
            iChmaIdx ^= 0x01;
            pDstChma = pPredIntraChma[iChmaIdx];
        }
    }

    (*pMbCache).pBestPredIntraChroma = pPredIntraChma[iChmaIdx ^ 0x01];
    (*pMbCache).uiChmaI8x8Mode = iBestMode as u8;
    iBestCost
}

/// `svc_base_layer_md.cpp:932`. The non-`LOW_COMPLEXITY` `pfIntraFineMd`.
///
/// # Safety
/// Same as [`WelsMdI4x4`].
pub unsafe extern "C" fn WelsMdIntraFinePartition(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> i32 {
    let iCosti4x4 = WelsMdI4x4(pEncCtx, pWelsMd, pCurMb, pMbCache);

    if iCosti4x4 < (*pWelsMd).iCostLuma {
        (*pCurMb).uiMbType = MB_TYPE_INTRA4x4;
        (*pWelsMd).iCostLuma = iCosti4x4;
    }
    (*pWelsMd).iCostLuma
}

/// `svc_base_layer_md.cpp:942`. The `LOW_COMPLEXITY` `pfIntraFineMd`, and the one the
/// Phase-5 gate configuration takes. Skips the I4x4 search entirely for macroblocks
/// whose intra variance is below `INTRA_VARIANCE_SAD_THRESHOLD`.
///
/// # Safety
/// Same as [`WelsMdI4x4Fast`].
pub unsafe extern "C" fn WelsMdIntraFinePartitionVaa(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> i32 {
    if MdIntraAnalysisVaaInfo(pEncCtx, (*pMbCache).SPicData.pEncMb[0]) {
        let iCosti4x4 = WelsMdI4x4Fast(pEncCtx, pWelsMd, pCurMb, pMbCache);

        if iCosti4x4 < (*pWelsMd).iCostLuma {
            (*pCurMb).uiMbType = MB_TYPE_INTRA4x4;
            (*pWelsMd).iCostLuma = iCosti4x4;
        }
    }

    (*pWelsMd).iCostLuma
}

/// `svc_base_layer_md.cpp:956`. The whole intra mode decision for one macroblock:
/// score I16x16, then let `WelsMdIntraSecondaryModesEnc` try I4x4 and chroma and
/// reconstruct whichever won.
///
/// # Safety
/// `pEncCtx`, `pWelsMd`, `pCurMb` and `pMbCache` must be valid, and
/// [`WelsMdIntraInit`] must have run for this macroblock.
pub unsafe fn WelsMdIntraMb(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    //initial prediction memory for I_16x16
    (*pWelsMd).iCostLuma = crate::encoder::svc_mode_decision::WelsMdI16x16(
        (*pEncCtx).pFuncList,
        (*pEncCtx).pCurDqLayer,
        pMbCache,
        (*pWelsMd).iLambda,
    );
    (*pCurMb).uiMbType = MB_TYPE_INTRA16x16;

    WelsMdIntraSecondaryModesEnc(pEncCtx, pWelsMd, pCurMb, pMbCache);
}

// ============================================================================
// The inter (P-slice) half of `svc_base_layer_md.cpp`
// ============================================================================

/// `encoder_data_tables.cpp:41`, declared in `mb_cache.h:59`. Byte offset of each
/// 4x4 block inside a 16x16 prediction buffer, in raster-scan-of-4x4 order.
pub const g_kuiSmb4AddrIn256: [u8; 16] = [
    0,          4,           16 * 4,      16 * 4 + 4,
    8,          12,          16 * 4 + 8,  16 * 4 + 12,
    16 * 8,     16 * 8 + 4,  16 * 12,     16 * 12 + 4,
    16 * 8 + 8, 16 * 8 + 12, 16 * 12 + 8, 16 * 12 + 12,
];

/// `svc_base_layer_md.cpp:1543`.
pub const g_kiPixStrideIdx8x8: [i32; 4] = [
    0,
    ME_REFINE_BUF_WIDTH_BLK8,
    ME_REFINE_BUF_STRIDE_BLK8,
    ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK8,
];

/// `svc_base_layer_md.cpp:1546`.
pub const g_kiPixStrideIdx4x4: [[i32; 4]; 4] = [
    [
        0,
        ME_REFINE_BUF_WIDTH_BLK4,
        ME_REFINE_BUF_STRIDE_BLK4,
        ME_REFINE_BUF_WIDTH_BLK4 + ME_REFINE_BUF_STRIDE_BLK4,
    ],
    [
        ME_REFINE_BUF_WIDTH_BLK8,
        ME_REFINE_BUF_WIDTH_BLK8 + ME_REFINE_BUF_WIDTH_BLK4,
        ME_REFINE_BUF_WIDTH_BLK8 + ME_REFINE_BUF_STRIDE_BLK4,
        ME_REFINE_BUF_WIDTH_BLK8 + ME_REFINE_BUF_WIDTH_BLK4 + ME_REFINE_BUF_STRIDE_BLK4,
    ],
    [
        ME_REFINE_BUF_STRIDE_BLK8,
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK4,
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_STRIDE_BLK4,
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK4 + ME_REFINE_BUF_STRIDE_BLK4,
    ],
    [
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK8,
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK8 + ME_REFINE_BUF_WIDTH_BLK4,
        ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK8 + ME_REFINE_BUF_STRIDE_BLK4,
        ME_REFINE_BUF_STRIDE_BLK8
            + ME_REFINE_BUF_WIDTH_BLK8
            + ME_REFINE_BUF_WIDTH_BLK4
            + ME_REFINE_BUF_STRIDE_BLK4,
    ],
];

/// `svc_base_layer_md.cpp:321`. Per-macroblock inter setup: neighbour cache, the
/// reference-plane pointers, and the integer MV clamp for this macroblock position.
///
/// # Safety
/// `pEncCtx`, `pSlice` and `pCurMb` must be valid; `pCurDqLayer->pRefPic` and
/// `pEncCtx->pVaa->pVaaBackgroundMbFlag` must be assigned.
pub unsafe fn WelsMdInterInit(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbXY = (*pCurMb).iMbXY;
    let kiMbWidth = (*pCurLayer).iMbWidth as i32;
    let kiMbHeight = (*pCurLayer).iMbHeight as i32;

    (*pMbCache).pEncSad = (*(*pCurLayer).pDecPic).pMbSkipSad.offset(kiMbXY as isize);

    //step 1. load neighbor cache
    (*(*pEncCtx).pFuncList)
        .pfFillInterNeighborCache
        .expect("pfFillInterNeighborCache unset")(
        pMbCache,
        pCurMb,
        kiMbWidth,
        (*(*pEncCtx).pVaa).pVaaBackgroundMbFlag.offset(kiMbXY as isize),
    ); //BGD spatial pFunc

    //step 4. locating current p_ref
    // merge loops
    if 0 == kiMbX || iSliceFirstMbXY == kiMbXY {
        let kiRefStrideY = (*(*pCurLayer).pRefPic).iLineSize[0];
        let kiRefStrideUV = (*(*pCurLayer).pRefPic).iLineSize[1];
        let kiCurStrideY = (kiMbX + kiMbY * kiRefStrideY) << 4;
        let kiCurStrideUV = (kiMbX + kiMbY * kiRefStrideUV) << 3;
        (*pMbCache).SPicData.pRefMb[0] =
            (*(*pCurLayer).pRefPic).pData[0].offset(kiCurStrideY as isize);
        (*pMbCache).SPicData.pRefMb[1] =
            (*(*pCurLayer).pRefPic).pData[1].offset(kiCurStrideUV as isize);
        (*pMbCache).SPicData.pRefMb[2] =
            (*(*pCurLayer).pRefPic).pData[2].offset(kiCurStrideUV as isize);
    } else {
        (*pMbCache).SPicData.pRefMb[0] = (*pMbCache).SPicData.pRefMb[0].add(MB_WIDTH_LUMA);
        (*pMbCache).SPicData.pRefMb[1] = (*pMbCache).SPicData.pRefMb[1].add(MB_WIDTH_CHROMA);
        (*pMbCache).SPicData.pRefMb[2] = (*pMbCache).SPicData.pRefMb[2].add(MB_WIDTH_CHROMA);
    }

    (*pMbCache).uiRefMbType = *(*(*pCurLayer).pRefPic).uiRefMbType.offset(kiMbXY as isize);
    (*pMbCache).bCollocatedPredFlag = false;

    //comment: sometimes, mode decision process may skip the md_p16x16 and md_pskip function,
    (*pCurMb).sP16x16Mv = SMVUnitXY { iMvX: 0, iMvY: 0 };
    *(*(*pCurLayer).pDecPic).sMvList.offset(kiMbXY as isize) = SMVUnitXY { iMvX: 0, iMvY: 0 };

    SetMvWithinIntegerMvRange(
        kiMbWidth,
        kiMbHeight,
        kiMbX,
        kiMbY,
        (*pEncCtx).iMvRange,
        &mut (*pSlice).sMvStartMin as *mut SMVUnitXY,
        &mut (*pSlice).sMvStartMax as *mut SMVUnitXY,
    );
}

/// `svc_base_layer_md.cpp:1023`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
pub unsafe extern "C" fn WelsMdP16x8(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let iStrideEnc = (*pCurDqLayer).iEncStride[0];
    let iStrideRef = (*(*pCurDqLayer).pRefPic).iLineSize[0];
    let mut iCostP16x8 = 0i32;
    for i in 0..2i32 {
        let sMe16x8 = &mut (*pWelsMd).sMe.sMe16x8[i as usize] as *mut SWelsME;
        let iPixelY = i << 3;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_16x8 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset((iPixelY * iStrideEnc) as isize),
            (*pMbCache).SPicData.pRefMb[0].offset((iPixelY * iStrideRef) as isize),
            (*(*pCurDqLayer).pRefPic).pScreenBlockFeatureStorage,
            sMe16x8,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        (*sMe16x8).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe16x8).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 1) as u32;

        (*pSlice).sMvc[0] = (*sMe16x8).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredInter16x8Mv(pMbCache, i << 3, 0, &mut (*sMe16x8).sMvp as *mut SMVUnitXY);
        (*pFunc).pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
            pFunc,
            pCurDqLayer,
            sMe16x8,
            pSlice,
        );
        UpdateP16x8Motion2Cache(
            pMbCache,
            i << 3,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe16x8).sMv as *mut SMVUnitXY,
        );
        iCostP16x8 += (*sMe16x8).uiSatdCost as i32;
    }
    iCostP16x8
}

/// `svc_base_layer_md.cpp:1053`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
pub unsafe extern "C" fn WelsMdP8x16(
    pFunc: *mut SWelsFuncPtrList,
    pCurLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let mut iCostP8x16 = 0i32;
    for i in 0..2i32 {
        let iPixelX = i << 3;
        let sMe8x16 = &mut (*pWelsMd).sMe.sMe8x16[i as usize] as *mut SWelsME;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_8x16 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset(iPixelX as isize),
            (*pMbCache).SPicData.pRefMb[0].offset(iPixelX as isize),
            (*(*pCurLayer).pRefPic).pScreenBlockFeatureStorage,
            sMe8x16,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        (*sMe8x16).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe8x16).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 1) as u32;

        (*pSlice).sMvc[0] = (*sMe8x16).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredInter8x16Mv(pMbCache, i << 2, 0, &mut (*sMe8x16).sMvp as *mut SMVUnitXY);
        (*pFunc).pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
            pFunc,
            pCurLayer,
            sMe8x16,
            pSlice,
        );
        UpdateP8x16Motion2Cache(
            pMbCache,
            i << 2,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe8x16).sMv as *mut SMVUnitXY,
        );
        iCostP8x16 += (*sMe8x16).uiSatdCost as i32;
    }
    iCostP8x16
}

/// `svc_base_layer_md.cpp:1120`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
pub unsafe extern "C" fn WelsMdP4x4(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    ki8x8Idx: i32,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let iLineSizeRef = (*(*pCurDqLayer).pRefPic).iLineSize[0];
    let mut iCostP4x4 = 0i32;
    for i4x4Idx in 0..4i32 {
        let iPartIdx = (ki8x8Idx << 2) + i4x4Idx;
        let iIdxX = ((ki8x8Idx & 1) << 1) + (i4x4Idx & 1);
        let iIdxY = ((ki8x8Idx >> 1) << 1) + (i4x4Idx >> 1);
        let iPixelX = iIdxX << 2;
        let iPixelY = iIdxY << 2;
        let iStrideEnc = iPixelX + (iPixelY * iLineSizeEnc);
        let iStrideRef = iPixelX + (iPixelY * iLineSizeRef);

        let sMe4x4 =
            &mut (*pWelsMd).sMe.sMe4x4[ki8x8Idx as usize][i4x4Idx as usize] as *mut SWelsME;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_4x4 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset(iStrideEnc as isize),
            (*pMbCache).SPicData.pRefMb[0].offset(iStrideRef as isize),
            (*(*pCurDqLayer).pRefPic).pScreenBlockFeatureStorage,
            sMe4x4,
        );
        //not putting these three lines below into InitMe to avoid judging mode in InitMe
        (*sMe4x4).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe4x4).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe4x4).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 2) as u32;

        (*pSlice).sMvc[0] = (*sMe4x4).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredMv(
            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
            iPartIdx as i8,
            1,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe4x4).sMvp as *mut SMVUnitXY,
        );
        (*pFunc).pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
            pFunc,
            pCurDqLayer,
            sMe4x4,
            pSlice,
        );
        UpdateP4x4Motion2Cache(
            pMbCache,
            iPartIdx,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe4x4).sMv as *mut SMVUnitXY,
        );
        iCostP4x4 += (*sMe4x4).uiSatdCost as i32;
    }
    iCostP4x4
}

/// `svc_base_layer_md.cpp:1159`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
pub unsafe extern "C" fn WelsMdP8x4(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    ki8x8Idx: i32,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let iLineSizeRef = (*(*pCurDqLayer).pRefPic).iLineSize[0];
    let mut iCostP8x4 = 0i32;
    for i8x4Idx in 0..2i32 {
        let iPartIdx = (ki8x8Idx << 2) + (i8x4Idx << 1);
        let iIdxX = (ki8x8Idx & 1) << 1;
        let iIdxY = ((ki8x8Idx >> 1) << 1) + i8x4Idx;
        let iPixelX = iIdxX << 2;
        let iPixelY = iIdxY << 2;
        let iStrideEnc = iPixelX + (iPixelY * iLineSizeEnc);
        let iStrideRef = iPixelX + (iPixelY * iLineSizeRef);

        let sMe8x4 =
            &mut (*pWelsMd).sMe.sMe8x4[ki8x8Idx as usize][i8x4Idx as usize] as *mut SWelsME;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_8x4 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset(iStrideEnc as isize),
            (*pMbCache).SPicData.pRefMb[0].offset(iStrideRef as isize),
            (*(*pCurDqLayer).pRefPic).pScreenBlockFeatureStorage,
            sMe8x4,
        );
        //not putting these three lines below into InitMe to avoid judging mode in InitMe
        (*sMe8x4).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe8x4).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe8x4).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 2) as u32;

        (*pSlice).sMvc[0] = (*sMe8x4).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredMv(
            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
            iPartIdx as i8,
            2,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe8x4).sMvp as *mut SMVUnitXY,
        );
        (*pFunc).pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
            pFunc,
            pCurDqLayer,
            sMe8x4,
            pSlice,
        );
        UpdateP8x4Motion2Cache(
            pMbCache,
            iPartIdx,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe8x4).sMv as *mut SMVUnitXY,
        );
        iCostP8x4 += (*sMe8x4).uiSatdCost as i32;
    }
    iCostP8x4
}

/// `svc_base_layer_md.cpp:1198`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
pub unsafe extern "C" fn WelsMdP4x8(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    ki8x8Idx: i32,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let iLineSizeRef = (*(*pCurDqLayer).pRefPic).iLineSize[0];
    let mut iCostP4x8 = 0i32;
    for i4x8Idx in 0..2i32 {
        let iPartIdx = (ki8x8Idx << 2) + i4x8Idx;
        let iIdxX = ((ki8x8Idx & 1) << 1) + i4x8Idx;
        let iIdxY = (ki8x8Idx >> 1) << 1;
        let iPixelX = iIdxX << 2;
        let iPixelY = iIdxY << 2;
        let iStrideEnc = iPixelX + (iPixelY * iLineSizeEnc);
        let iStrideRef = iPixelX + (iPixelY * iLineSizeRef);

        let sMe4x8 =
            &mut (*pWelsMd).sMe.sMe4x8[ki8x8Idx as usize][i4x8Idx as usize] as *mut SWelsME;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_4x8 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset(iStrideEnc as isize),
            (*pMbCache).SPicData.pRefMb[0].offset(iStrideRef as isize),
            (*(*pCurDqLayer).pRefPic).pScreenBlockFeatureStorage,
            sMe4x8,
        );
        //not putting these three lines below into InitMe to avoid judging mode in InitMe
        (*sMe4x8).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe4x8).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe4x8).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 2) as u32;

        (*pSlice).sMvc[0] = (*sMe4x8).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredMv(
            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
            iPartIdx as i8,
            1,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe4x8).sMvp as *mut SMVUnitXY,
        );
        (*pFunc).pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
            pFunc,
            pCurDqLayer,
            sMe4x8,
            pSlice,
        );
        UpdateP4x8Motion2Cache(
            pMbCache,
            iPartIdx,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe4x8).sMv as *mut SMVUnitXY,
        );
        iCostP4x8 += (*sMe4x8).uiSatdCost as i32;
    }
    iCostP4x8
}

/// `svc_base_layer_md.cpp:1238`. The non-VAA (`!LOW_COMPLEXITY`) fine partition search.
///
/// # Safety
/// All pointers must be valid.
pub unsafe extern "C" fn WelsMdInterFinePartition(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    iBestCost: i32,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let mut iCost = crate::encoder::svc_mode_decision::WelsMdP8x8(
        (*pEncCtx).pFuncList,
        pCurDqLayer,
        pWelsMd,
        pSlice,
    );

    if iCost < iBestCost {
        (*pCurMb).uiMbType = MB_TYPE_8x8;
        (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];

        let mut iCostPart = WelsMdP16x8((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            iCost = iCostPart;
            (*pCurMb).uiMbType = MB_TYPE_16x8;
        }

        iCostPart = WelsMdP8x16((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            (*pCurMb).uiMbType = MB_TYPE_8x16;
        }
    }
}

/// `svc_base_layer_md.cpp:1270`. The VAA-guided fine partition search — the
/// `LOW_COMPLEXITY` path the gate configuration takes.
///
/// # Safety
/// All pointers must be valid; `pEncCtx->pVaa->sVaaCalcInfo.pSad8x8` must be
/// populated and `pfGetMbSignFromInterVaa` assigned.
pub unsafe extern "C" fn WelsMdInterFinePartitionVaa(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    iBestCostIn: i32,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let mut iBestCost = iBestCostIn;
    let uiMbSign = (*(*pEncCtx).pFuncList)
        .pfGetMbSignFromInterVaa
        .expect("pfGetMbSignFromInterVaa unset")(
        (*(*pEncCtx).pVaa)
            .sVaaCalcInfo
            .pSad8x8
            .offset((*pCurMb).iMbXY as isize) as *mut i32,
    );

    if crate::encoder::dump_enabled(&FP_DUMP, "OH264_FPDUMP") {
        let sad = *(*(*pEncCtx).pVaa)
            .sVaaCalcInfo
            .pSad8x8
            .offset((*pCurMb).iMbXY as isize);
        eprintln!(
            "FP mb={:3} sign={:2} best={:7} sad8x8={},{},{},{}",
            (*pCurMb).iMbXY,
            uiMbSign,
            iBestCost,
            sad[0],
            sad[1],
            sad[2],
            sad[3]
        );
    }

    if uiMbSign == 15 {
        return;
    }

    match uiMbSign {
        3 | 12 => {
            let iCostP16x8 = WelsMdP16x8((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
            if iCostP16x8 < iBestCost {
                iBestCost = iCostP16x8;
                (*pCurMb).uiMbType = MB_TYPE_16x8;
            }
        }
        5 | 10 => {
            let iCostP8x16 = WelsMdP8x16((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x16 < iBestCost {
                iBestCost = iCostP8x16;
                (*pCurMb).uiMbType = MB_TYPE_8x16;
            }
        }
        6 | 9 => {
            let iCostP8x8 = crate::encoder::svc_mode_decision::WelsMdP8x8(
                (*pEncCtx).pFuncList,
                pCurDqLayer,
                pWelsMd,
                pSlice,
            );
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                (*pCurMb).uiMbType = MB_TYPE_8x8;
                (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];
            }
        }
        _ => {
            let iCostP8x8 = crate::encoder::svc_mode_decision::WelsMdP8x8(
                (*pEncCtx).pFuncList,
                pCurDqLayer,
                pWelsMd,
                pSlice,
            );
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                (*pCurMb).uiMbType = MB_TYPE_8x8;
                (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];

                let iCostP16x8 = WelsMdP16x8((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
                if iCostP16x8 <= iBestCost {
                    iBestCost = iCostP16x8;
                    (*pCurMb).uiMbType = MB_TYPE_16x8;
                }

                let iCostP8x16 = WelsMdP8x16((*pEncCtx).pFuncList, pCurDqLayer, pWelsMd, pSlice);
                if iCostP8x16 <= iBestCost {
                    iBestCost = iCostP8x16;
                    (*pCurMb).uiMbType = MB_TYPE_8x16;
                }
            }
        }
    }
    (*pWelsMd).iCostLuma = iBestCost;
}

/// `svc_base_layer_md.cpp:1423`. Motion-compensates the P_SKIP predictor and decides
/// whether the macroblock can be coded as P_SKIP.
///
/// # Safety
/// All four pointers must be valid; `sMcFuncs`, `pfSampleSad`/`pfSampleSatd`,
/// `pfDctFourT4` and `pfUpdateMbMv` must be assigned.
pub unsafe fn WelsMdPSkipEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pFunc = (*pEncCtx).pFuncList;

    let mut pRefLuma = (*pMbCache).SPicData.pRefMb[0];
    let mut pRefCb = (*pMbCache).SPicData.pRefMb[1];
    let mut pRefCr = (*pMbCache).SPicData.pRefMb[2];
    let iLineSizeY = (*(*pCurLayer).pRefPic).iLineSize[0];
    let iLineSizeUV = (*(*pCurLayer).pRefPic).iLineSize[1];

    let pDstLuma = (*pMbCache).pSkipMb;
    let pDstCb = (*pMbCache).pSkipMb.add(256);
    let pDstCr = (*pMbCache).pSkipMb.add(256 + 64);

    let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
    let mut n: i32;

    let mut iEncStride = (*pCurLayer).iEncStride[0];
    let mut pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let pStrideEncBlockOffset =
        (*(*pEncCtx).pStrideTab).pStrideEncBlockOffset[(*pEncCtx).uiDependencyId as usize];
    let mut pEncBlockOffset: *mut i32;

    let iSadCostLuma: i32;
    let mut iSadCostChroma: i32;
    let iSadCostMb: i32;

    PredSkipMv(pMbCache, &mut sMvp as *mut SMVUnitXY);

    // Special case, need to clip the vector //
    let sQpelMvp = SMVUnitXY {
        iMvX: (sMvp.iMvX >> 2) as i16,
        iMvY: (sMvp.iMvY >> 2) as i16,
    };
    n = (((*pCurMb).iMbX as i32) << 4) + sQpelMvp.iMvX as i32;
    if n < -29 {
        return false;
    } else if n > ((((*pCurLayer).iMbWidth as i32) << 4) + 12) {
        return false;
    }

    n = (((*pCurMb).iMbY as i32) << 4) + sQpelMvp.iMvY as i32;
    if n < -29 {
        return false;
    } else if n > ((((*pCurLayer).iMbHeight as i32) << 4) + 12) {
        return false;
    }

    //luma
    pRefLuma = pRefLuma.offset((sQpelMvp.iMvY as i32 * iLineSizeY + sQpelMvp.iMvX as i32) as isize);
    McLuma_c(
        pRefLuma, iLineSizeY, pDstLuma, 16, sMvp.iMvX, sMvp.iMvY, 16, 16,
    );
    iSadCostLuma = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16]
        .expect("pfSampleSad[BLOCK_16x16] unset")(
        (*pMbCache).SPicData.pEncMb[0],
        (*pCurLayer).iEncStride[0],
        pDstLuma,
        16,
    );

    let iStrideUV = (sQpelMvp.iMvY as i32 >> 1) * iLineSizeUV + (sQpelMvp.iMvX as i32 >> 1);
    let pfSad8x8 = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_8x8]
        .expect("pfSampleSad[BLOCK_8x8] unset");
    pRefCb = pRefCb.offset(iStrideUV as isize);
    McChroma_c(pRefCb, iLineSizeUV, pDstCb, 8, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cb
    iSadCostChroma = pfSad8x8(
        (*pMbCache).SPicData.pEncMb[1],
        (*pCurLayer).iEncStride[1],
        pDstCb,
        8,
    );

    pRefCr = pRefCr.offset(iStrideUV as isize);
    McChroma_c(pRefCr, iLineSizeUV, pDstCr, 8, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cr
    iSadCostChroma += pfSad8x8(
        (*pMbCache).SPicData.pEncMb[2],
        (*pCurLayer).iEncStride[2],
        pDstCr,
        8,
    );

    iSadCostMb = iSadCostLuma + iSadCostChroma;

    if iSadCostMb == 0
        || iSadCostMb < (*pWelsMd).iSadPredSkip
        || ((*(*pCurLayer).pRefPic).iPictureType == EWelsSliceType::P_SLICE as i32
            && (*pMbCache).uiRefMbType == MB_TYPE_SKIP
            && iSadCostMb < *(*(*pCurLayer).pRefPic).pMbSkipSad.offset((*pCurMb).iMbXY as isize))
    {
        //update motion info to current MB
        AcceptPskip(pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb, pDstLuma);
        return true;
    }

    WelsDctMb(
        (*pMbCache).pCoeffLevel,
        pEncMb,
        iEncStride,
        pDstLuma,
        (*(*pEncCtx).pFuncList).pfDctFourT4,
    );

    if WelsTryPYskip(pEncCtx, pCurMb, pMbCache) {
        iEncStride = (*(*pEncCtx).pCurDqLayer).iEncStride[1];
        pEncMb = (*pMbCache).SPicData.pEncMb[1];
        pEncBlockOffset = pStrideEncBlockOffset.add(16);
        (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset")(
            (*pMbCache).pCoeffLevel.add(256),
            pEncMb.offset(*pEncBlockOffset as isize),
            iEncStride,
            (*pMbCache).pSkipMb.add(256),
            8,
        );
        if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 1) {
            pEncMb = (*pMbCache).SPicData.pEncMb[2];
            pEncBlockOffset = pStrideEncBlockOffset.add(20);
            (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset")(
                (*pMbCache).pCoeffLevel.add(320),
                pEncMb.offset(*pEncBlockOffset as isize),
                iEncStride,
                (*pMbCache).pSkipMb.add(320),
                8,
            );
            if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 2) {
                //update motion info to current MB
                AcceptPskip(
                    pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb, pDstLuma,
                );
                return true;
            }
        }
    }
    false
}

/// The block `WelsMdPSkipEnc` runs verbatim at both of its `return true` sites
/// (`svc_base_layer_md.cpp:1489` and `:1521`).
///
/// # Safety
/// As [`WelsMdPSkipEnc`].
#[inline]
unsafe fn AcceptPskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    sMvp: &SMVUnitXY,
    iSadCostLuma: i32,
    iSadCostMb: i32,
    pDstLuma: *mut u8,
) {
    let pCurLayer = (*pEncCtx).pCurDqLayer;
    let pFunc = (*pEncCtx).pFuncList;

    // ST32 (pCurMb->pRefIndex, 0)
    (*pCurMb).iRefIndex = [0; crate::encoder::md::MB_BLOCK8x8_NUM];
    (*pFunc).pfUpdateMbMv.expect("pfUpdateMbMv unset")(&mut (*pCurMb).sMv, *sMvp);

    if (*pWelsMd).bMdUsingSad {
        (*pCurMb).iSadCost = iSadCostLuma;
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        (*pWelsMd).iCostLuma = (*pFunc).sSampleDealingFuncs.pfSampleSatd[BLOCK_16x16]
            .expect("pfSampleSatd[BLOCK_16x16] unset")(
            (*pMbCache).SPicData.pEncMb[0],
            (*pCurLayer).iEncStride[0],
            pDstLuma,
            16,
        );
    }

    (*pWelsMd).iCostSkipMb = iSadCostMb;

    (*pCurMb).sP16x16Mv = *sMvp;
    *(*(*pCurLayer).pDecPic)
        .sMvList
        .offset((*pCurMb).iMbXY as isize) = *sMvp;
}

/// `svc_base_layer_md.cpp:1573`. Quarter-pel refinement of whichever partitioning the
/// integer search chose, plus the chroma motion compensation for each partition.
///
/// # Safety
/// All four pointers must be valid; the `pfCopy*` slots and `sMcFuncs.pMcChromaFunc`
/// must be assigned.
pub unsafe fn WelsMdInterMbRefinement(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let pFunc = (*pEncCtx).pFuncList;
    let pTmpRefCb: *mut u8;
    let pTmpRefCr: *mut u8;
    let mut iBestSadCost = 0i32;
    let mut iBestSatdCost = 0i32;
    let mut sMeRefine = SMeRefinePointer::default();

    let pRefCb = (*pMbCache).SPicData.pRefMb[1];
    let pRefCr = (*pMbCache).SPicData.pRefMb[2];
    let pDstCb = (*pMbCache).pMemPredChroma;
    let pDstCr = (*pMbCache).pMemPredChroma.add(64);
    let pDstLuma = (*pMbCache).pMemPredLuma;

    let iLineSizeRefUV = (*(*pCurDqLayer).pRefPic).iLineSize[1];

    match (*pCurMb).uiMbType {
        MB_TYPE_16x16 => {
            //luma
            InitMeRefinePointer(&mut sMeRefine as *mut SMeRefinePointer, pMbCache, 0);
            sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy16x16NotAligned;
            MeRefineFracPixel(
                pEncCtx,
                pDstLuma,
                &mut (*pWelsMd).sMe.sMe16x16 as *mut SWelsME,
                &mut sMeRefine as *mut SMeRefinePointer,
                16,
                16,
            );
            UpdateP16x16MotionInfo(
                pMbCache,
                pCurMb,
                (*pWelsMd).uiRef as i8,
                &mut (*pWelsMd).sMe.sMe16x16.sMv as *mut SMVUnitXY,
            );

            (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;
            //save the best cost of final mode
            iBestSadCost = (*pWelsMd).sMe.sMe16x16.uiSadCost as i32;
            iBestSatdCost = (*pWelsMd).sMe.sMe16x16.uiSatdCost as i32;

            //chroma
            let pMv = &mut (*pWelsMd).sMe.sMe16x16.sMv as *mut SMVUnitXY;
            let iMvStride =
                ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV + ((*pMv).iMvX as i32 >> 3);
            pTmpRefCb = pRefCb.offset(iMvStride as isize);
            pTmpRefCr = pRefCr.offset(iMvStride as isize);
            McChroma_c(pTmpRefCb, iLineSizeRefUV, pDstCb, 8, (*pMv).iMvX, (*pMv).iMvY, 8, 8); //Cb
            McChroma_c(pTmpRefCr, iLineSizeRefUV, pDstCr, 8, (*pMv).iMvX, (*pMv).iMvY, 8, 8); //Cr

            let sdf = &(*pFunc).sSampleDealingFuncs;
            (*pWelsMd).iCostSkipMb = sdf.pfSampleSad[BLOCK_16x16].expect("pfSampleSad unset")(
                (*pMbCache).SPicData.pEncMb[0],
                (*pCurDqLayer).iEncStride[0],
                pDstLuma,
                16,
            );
            (*pWelsMd).iCostSkipMb += sdf.pfSampleSad[BLOCK_8x8].expect("pfSampleSad unset")(
                (*pMbCache).SPicData.pEncMb[1],
                (*pCurDqLayer).iEncStride[1],
                pDstCb,
                8,
            );
            (*pWelsMd).iCostSkipMb += sdf.pfSampleSad[BLOCK_8x8].expect("pfSampleSad unset")(
                (*pMbCache).SPicData.pEncMb[2],
                (*pCurDqLayer).iEncStride[2],
                pDstCr,
                8,
            );
        }

        MB_TYPE_16x8 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy16x8NotAligned;
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 3;
                InitMeRefinePointer(&mut sMeRefine as *mut SMeRefinePointer, pMbCache, iPixStride);
                iPixStride += ME_REFINE_BUF_STRIDE_BLK8;
                PredInter16x8Mv(
                    pMbCache,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe16x8[i].sMvp as *mut SMVUnitXY,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    pDstLuma.add(g_kuiSmb4AddrIn256[iIdx as usize] as usize),
                    &mut (*pWelsMd).sMe.sMe16x8[i] as *mut SWelsME,
                    &mut sMeRefine as *mut SMeRefinePointer,
                    16,
                    8,
                );
                UpdateP16x8MotionInfo(
                    pMbCache,
                    pCurMb,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe16x8[i].sMv as *mut SMVUnitXY,
                );
                (*pMbCache).sMbMvp[i] = (*pWelsMd).sMe.sMe16x8[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += (*pWelsMd).sMe.sMe16x8[i].uiSadCost as i32;
                iBestSatdCost += (*pWelsMd).sMe.sMe16x8[i].uiSatdCost as i32;

                //chroma
                let iRefBlk4Stride = ((i as i32) << 2) * iLineSizeRefUV;
                let iDstBlk4Stride = (i as i32) << 5; // 4*8
                let pMv = &mut (*pWelsMd).sMe.sMe16x8[i].sMv as *mut SMVUnitXY;
                let iMvStride =
                    ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV + ((*pMv).iMvX as i32 >> 3);
                let pTmpRefCb = pRefCb.offset((iRefBlk4Stride + iMvStride) as isize);
                let pTmpRefCr = pRefCr.offset((iRefBlk4Stride + iMvStride) as isize);
                let pTmpDstCb = pDstCb.offset(iDstBlk4Stride as isize);
                let pTmpDstCr = pDstCr.offset(iDstBlk4Stride as isize);
                McChroma_c(pTmpRefCb, iLineSizeRefUV, pTmpDstCb, 8, (*pMv).iMvX, (*pMv).iMvY, 8, 4); //Cb
                McChroma_c(pTmpRefCr, iLineSizeRefUV, pTmpDstCr, 8, (*pMv).iMvX, (*pMv).iMvY, 8, 4); //Cr
            }
        }

        MB_TYPE_8x16 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy8x16Aligned;
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 2;
                InitMeRefinePointer(&mut sMeRefine as *mut SMeRefinePointer, pMbCache, iPixStride);
                iPixStride += ME_REFINE_BUF_WIDTH_BLK8;
                PredInter8x16Mv(
                    pMbCache,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe8x16[i].sMvp as *mut SMVUnitXY,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    pDstLuma.add(g_kuiSmb4AddrIn256[iIdx as usize] as usize),
                    &mut (*pWelsMd).sMe.sMe8x16[i] as *mut SWelsME,
                    &mut sMeRefine as *mut SMeRefinePointer,
                    8,
                    16,
                );
                update_P8x16_motion_info(
                    pMbCache,
                    pCurMb,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe8x16[i].sMv as *mut SMVUnitXY,
                );
                (*pMbCache).sMbMvp[i] = (*pWelsMd).sMe.sMe8x16[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += (*pWelsMd).sMe.sMe8x16[i].uiSadCost as i32;
                iBestSatdCost += (*pWelsMd).sMe.sMe8x16[i].uiSatdCost as i32;

                //chroma
                let iRefBlk4Stride = iIdx; //4
                let pMv = &mut (*pWelsMd).sMe.sMe8x16[i].sMv as *mut SMVUnitXY;
                let iMvStride =
                    ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV + ((*pMv).iMvX as i32 >> 3);
                let pTmpRefCb = pRefCb.offset((iRefBlk4Stride + iMvStride) as isize);
                let pTmpRefCr = pRefCr.offset((iRefBlk4Stride + iMvStride) as isize);
                let pTmpDstCb = pDstCb.offset(iRefBlk4Stride as isize);
                let pTmpDstCr = pDstCr.offset(iRefBlk4Stride as isize);
                McChroma_c(pTmpRefCb, iLineSizeRefUV, pTmpDstCb, 8, (*pMv).iMvX, (*pMv).iMvY, 4, 8); //Cb
                McChroma_c(pTmpRefCr, iLineSizeRefUV, pTmpDstCr, 8, (*pMv).iMvX, (*pMv).iMvY, 4, 8); //Cr
            }
        }

        MB_TYPE_8x8 => {
            (*pMbCache).sMvComponents.iRefIndexCache[9] = REF_NOT_AVAIL;
            (*pMbCache).sMvComponents.iRefIndexCache[21] = REF_NOT_AVAIL;
            for i in 0..4usize {
                let iBlk8Idx = (i as i32) << 2; //0, 4, 8, 12

                (*pCurMb).iRefIndex[i] = (*pWelsMd).uiRef as i8;
                match (*pCurMb).uiSubMbType[i] {
                    SUB_MB_TYPE_8x8 => {
                        sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy8x8Aligned;
                        //luma
                        InitMeRefinePointer(
                            &mut sMeRefine as *mut SMeRefinePointer,
                            pMbCache,
                            g_kiPixStrideIdx8x8[i],
                        );
                        PredMv(
                            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
                            iBlk8Idx as i8,
                            2,
                            (*pWelsMd).uiRef as i32,
                            &mut (*pWelsMd).sMe.sMe8x8[i].sMvp as *mut SMVUnitXY,
                        );
                        MeRefineFracPixel(
                            pEncCtx,
                            pDstLuma.add(g_kuiSmb4AddrIn256[iBlk8Idx as usize] as usize),
                            &mut (*pWelsMd).sMe.sMe8x8[i] as *mut SWelsME,
                            &mut sMeRefine as *mut SMeRefinePointer,
                            8,
                            8,
                        );
                        UpdateP8x8MotionInfo(
                            pMbCache,
                            pCurMb,
                            iBlk8Idx,
                            (*pWelsMd).uiRef as i8,
                            &mut (*pWelsMd).sMe.sMe8x8[i].sMv as *mut SMVUnitXY,
                        );
                        (*pMbCache).sMbMvp[g_kuiMbCountScan4Idx[iBlk8Idx as usize] as usize] =
                            (*pWelsMd).sMe.sMe8x8[i].sMvp;
                        iBestSadCost += (*pWelsMd).sMe.sMe8x8[i].uiSadCost as i32;
                        iBestSatdCost += (*pWelsMd).sMe.sMe8x8[i].uiSatdCost as i32;

                        //chroma
                        let pMv = &mut (*pWelsMd).sMe.sMe8x8[i].sMv as *mut SMVUnitXY;
                        let iMvStride =
                            ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV + ((*pMv).iMvX as i32 >> 3);

                        let iBlk4X = ((i as i32) & 1) << 2;
                        let iBlk4Y = ((i as i32) >> 1) << 2;
                        let iRefBlk4Stride = iBlk4Y * iLineSizeRefUV + iBlk4X;
                        let iDstBlk4Stride = (iBlk4Y << 3) + iBlk4X;

                        let pTmpRefCb = pRefCb.offset(iRefBlk4Stride as isize);
                        let pTmpDstCb = pDstCb.offset(iDstBlk4Stride as isize);
                        let pTmpRefCr = pRefCr.offset(iRefBlk4Stride as isize);
                        let pTmpDstCr = pDstCr.offset(iDstBlk4Stride as isize);
                        McChroma_c(
                            pTmpRefCb.offset(iMvStride as isize),
                            iLineSizeRefUV,
                            pTmpDstCb,
                            8,
                            (*pMv).iMvX,
                            (*pMv).iMvY,
                            4,
                            4,
                        ); //Cb
                        McChroma_c(
                            pTmpRefCr.offset(iMvStride as isize),
                            iLineSizeRefUV,
                            pTmpDstCr,
                            8,
                            (*pMv).iMvX,
                            (*pMv).iMvY,
                            4,
                            4,
                        ); //Cr
                    }
                    SUB_MB_TYPE_4x4 => {
                        sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy4x4;
                        //luma
                        for j in 0..4usize {
                            let iBlk4x4Idx = iBlk8Idx + j as i32;
                            InitMeRefinePointer(
                                &mut sMeRefine as *mut SMeRefinePointer,
                                pMbCache,
                                g_kiPixStrideIdx4x4[i][j],
                            );
                            PredMv(
                                &(*pMbCache).sMvComponents as *const SMVComponentUnit,
                                iBlk4x4Idx as i8,
                                1,
                                (*pWelsMd).uiRef as i32,
                                &mut (*pWelsMd).sMe.sMe4x4[i][j].sMvp as *mut SMVUnitXY,
                            );
                            MeRefineFracPixel(
                                pEncCtx,
                                pDstLuma.add(g_kuiSmb4AddrIn256[iBlk4x4Idx as usize] as usize),
                                &mut (*pWelsMd).sMe.sMe4x4[i][j] as *mut SWelsME,
                                &mut sMeRefine as *mut SMeRefinePointer,
                                4,
                                4,
                            );
                            UpdateP4x4MotionInfo(
                                pMbCache,
                                pCurMb,
                                iBlk4x4Idx,
                                (*pWelsMd).uiRef as i8,
                                &mut (*pWelsMd).sMe.sMe4x4[i][j].sMv as *mut SMVUnitXY,
                            );
                            (*pMbCache).sMbMvp
                                [g_kuiMbCountScan4Idx[iBlk4x4Idx as usize] as usize] =
                                (*pWelsMd).sMe.sMe4x4[i][j].sMvp;
                            iBestSadCost += (*pWelsMd).sMe.sMe4x4[i][j].uiSadCost as i32;
                            iBestSatdCost += (*pWelsMd).sMe.sMe4x4[i][j].uiSatdCost as i32;

                            //chroma
                            let pMv = &mut (*pWelsMd).sMe.sMe4x4[i][j].sMv as *mut SMVUnitXY;
                            let iMvStride = ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV
                                + ((*pMv).iMvX as i32 >> 3);

                            let iBlk4X = (((((i as i32) & 1) << 1) + (j as i32 & 1)) as i32) << 1;
                            let iBlk4Y = (((((i as i32) >> 1) << 1) + (j as i32 >> 1)) as i32) << 1;
                            let iRefBlk4Stride = iBlk4Y * iLineSizeRefUV + iBlk4X;
                            let iDstBlk4Stride = (iBlk4Y << 3) + iBlk4X;

                            let pTmpRefCb = pRefCb.offset(iRefBlk4Stride as isize);
                            let pTmpDstCb = pDstCb.offset(iDstBlk4Stride as isize);
                            let pTmpRefCr = pRefCr.offset(iRefBlk4Stride as isize);
                            let pTmpDstCr = pDstCr.offset(iDstBlk4Stride as isize);
                            McChroma_c(
                                pTmpRefCb.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCb,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                2,
                                2,
                            ); //Cb
                            McChroma_c(
                                pTmpRefCr.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCr,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                2,
                                2,
                            ); //Cr
                        }
                    }
                    SUB_MB_TYPE_8x4 => {
                        sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy8x4;
                        //luma
                        for j in 0..2usize {
                            let iBlk4x4Idx = iBlk8Idx + ((j as i32) << 1);
                            InitMeRefinePointer(
                                &mut sMeRefine as *mut SMeRefinePointer,
                                pMbCache,
                                g_kiPixStrideIdx4x4[i][j << 1],
                            );
                            PredMv(
                                &(*pMbCache).sMvComponents as *const SMVComponentUnit,
                                iBlk4x4Idx as i8,
                                2,
                                (*pWelsMd).uiRef as i32,
                                &mut (*pWelsMd).sMe.sMe8x4[i][j].sMvp as *mut SMVUnitXY,
                            );
                            MeRefineFracPixel(
                                pEncCtx,
                                pDstLuma.add(g_kuiSmb4AddrIn256[iBlk4x4Idx as usize] as usize),
                                &mut (*pWelsMd).sMe.sMe8x4[i][j] as *mut SWelsME,
                                &mut sMeRefine as *mut SMeRefinePointer,
                                8,
                                4,
                            );
                            UpdateP8x4MotionInfo(
                                pMbCache,
                                pCurMb,
                                iBlk4x4Idx,
                                (*pWelsMd).uiRef as i8,
                                &mut (*pWelsMd).sMe.sMe8x4[i][j].sMv as *mut SMVUnitXY,
                            );
                            (*pMbCache).sMbMvp
                                [g_kuiMbCountScan4Idx[iBlk4x4Idx as usize] as usize] =
                                (*pWelsMd).sMe.sMe8x4[i][j].sMvp;
                            iBestSadCost += (*pWelsMd).sMe.sMe8x4[i][j].uiSadCost as i32;
                            iBestSatdCost += (*pWelsMd).sMe.sMe8x4[i][j].uiSatdCost as i32;

                            //chroma
                            let pMv = &mut (*pWelsMd).sMe.sMe8x4[i][j].sMv as *mut SMVUnitXY;
                            let iMvStride = ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV
                                + ((*pMv).iMvX as i32 >> 3);

                            let iBlk4X = ((((i as i32) & 1) << 1) as i32) << 1;
                            let iBlk4Y = (((((i as i32) >> 1) << 1) + j as i32) as i32) << 1;
                            let iRefBlk4Stride = iBlk4Y * iLineSizeRefUV + iBlk4X;
                            let iDstBlk4Stride = (iBlk4Y << 3) + iBlk4X;

                            let pTmpRefCb = pRefCb.offset(iRefBlk4Stride as isize);
                            let pTmpDstCb = pDstCb.offset(iDstBlk4Stride as isize);
                            let pTmpRefCr = pRefCr.offset(iRefBlk4Stride as isize);
                            let pTmpDstCr = pDstCr.offset(iDstBlk4Stride as isize);
                            McChroma_c(
                                pTmpRefCb.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCb,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                4,
                                2,
                            ); //Cb
                            McChroma_c(
                                pTmpRefCr.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCr,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                4,
                                2,
                            ); //Cr
                        }
                    }
                    SUB_MB_TYPE_4x8 => {
                        sMeRefine.pfCopyBlockByMode = (*pFunc).pfCopy4x8;
                        //luma
                        for j in 0..2usize {
                            let iBlk4x4Idx = iBlk8Idx + j as i32;
                            InitMeRefinePointer(
                                &mut sMeRefine as *mut SMeRefinePointer,
                                pMbCache,
                                g_kiPixStrideIdx4x4[i][j],
                            );
                            PredMv(
                                &(*pMbCache).sMvComponents as *const SMVComponentUnit,
                                iBlk4x4Idx as i8,
                                1,
                                (*pWelsMd).uiRef as i32,
                                &mut (*pWelsMd).sMe.sMe4x8[i][j].sMvp as *mut SMVUnitXY,
                            );
                            MeRefineFracPixel(
                                pEncCtx,
                                pDstLuma.add(g_kuiSmb4AddrIn256[iBlk4x4Idx as usize] as usize),
                                &mut (*pWelsMd).sMe.sMe4x8[i][j] as *mut SWelsME,
                                &mut sMeRefine as *mut SMeRefinePointer,
                                4,
                                8,
                            );
                            UpdateP4x8MotionInfo(
                                pMbCache,
                                pCurMb,
                                iBlk4x4Idx,
                                (*pWelsMd).uiRef as i8,
                                &mut (*pWelsMd).sMe.sMe4x8[i][j].sMv as *mut SMVUnitXY,
                            );
                            (*pMbCache).sMbMvp
                                [g_kuiMbCountScan4Idx[iBlk4x4Idx as usize] as usize] =
                                (*pWelsMd).sMe.sMe4x8[i][j].sMvp;
                            iBestSadCost += (*pWelsMd).sMe.sMe4x8[i][j].uiSadCost as i32;
                            iBestSatdCost += (*pWelsMd).sMe.sMe4x8[i][j].uiSatdCost as i32;

                            //chroma
                            let pMv = &mut (*pWelsMd).sMe.sMe4x8[i][j].sMv as *mut SMVUnitXY;
                            let iMvStride = ((*pMv).iMvY as i32 >> 3) * iLineSizeRefUV
                                + ((*pMv).iMvX as i32 >> 3);

                            let iBlk4X = (((((i as i32) & 1) << 1) + j as i32) as i32) << 1;
                            let iBlk4Y = ((((i as i32) >> 1) << 1) as i32) << 1;
                            let iRefBlk4Stride = iBlk4Y * iLineSizeRefUV + iBlk4X;
                            let iDstBlk4Stride = (iBlk4Y << 3) + iBlk4X;

                            let pTmpRefCb = pRefCb.offset(iRefBlk4Stride as isize);
                            let pTmpDstCb = pDstCb.offset(iDstBlk4Stride as isize);
                            let pTmpRefCr = pRefCr.offset(iRefBlk4Stride as isize);
                            let pTmpDstCr = pDstCr.offset(iDstBlk4Stride as isize);
                            McChroma_c(
                                pTmpRefCb.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCb,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                2,
                                4,
                            ); //Cb
                            McChroma_c(
                                pTmpRefCr.offset(iMvStride as isize),
                                iLineSizeRefUV,
                                pTmpDstCr,
                                8,
                                (*pMv).iMvX,
                                (*pMv).iMvY,
                                2,
                                4,
                            ); //Cr
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    (*pCurMb).iSadCost = iBestSadCost;
    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = iBestSadCost;
    } else {
        (*pWelsMd).iCostLuma = iBestSatdCost;
    }
}

/// `svc_base_layer_md.cpp:1829`. Costs I16x16 against the current inter cost and, if
/// intra wins, runs the whole intra encode for this macroblock.
///
/// # Safety
/// All four pointers must be valid and `pfIntraFineMd` assigned.
pub unsafe extern "C" fn WelsMdFirstIntraMode(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool {
    let pFunc = (*pEncCtx).pFuncList;

    let iCostI16x16 = crate::encoder::svc_mode_decision::WelsMdI16x16(
        pFunc,
        (*pEncCtx).pCurDqLayer,
        pMbCache,
        (*pWelsMd).iLambda,
    );

    //compare cost_p16x16 with cost_i16x16
    if iCostI16x16 < (*pWelsMd).iCostLuma {
        (*pCurMb).uiMbType = MB_TYPE_INTRA16x16;
        (*pWelsMd).iCostLuma = iCostI16x16;

        (*pFunc).pfIntraFineMd.expect("pfIntraFineMd unset")(pEncCtx, pWelsMd, pCurMb, pMbCache);

        //add pEnc&rec to MD--2010.3.15
        if IS_INTRA16x16((*pCurMb).uiMbType) {
            (*pCurMb).uiCbp = 0;
            crate::encoder::svc_encode_mb::WelsEncRecI16x16Y(pEncCtx, pCurMb, pMbCache);
        }

        //chroma
        (*pWelsMd).iCostChroma =
            WelsMdIntraChroma(pFunc, (*pEncCtx).pCurDqLayer, pMbCache, (*pWelsMd).iLambda);
        crate::encoder::svc_encode_slice::WelsIMbChromaEncode(pEncCtx, pCurMb, pMbCache); //add pEnc&rec to MD--2010.3.15
        (*pCurMb).uiChromPredMode = (*pMbCache).uiChmaI8x8Mode as u32;
        (*pCurMb).iSadCost = 0;
        return true; //intra_mb_type is best
    }

    false
}

/// `svc_base_layer_md.cpp:1858`. The P-slice per-macroblock entry point; C++ assigns
/// it to `pfInterMd` in `WelsCodePSlice` (`svc_encode_slice.cpp:736`).
///
/// The trailing `pUnused` parameter is `SMbCache*` in the C++ signature and is
/// genuinely unread there — the body re-derives the cache from `pSlice`.
///
/// # Safety
/// All pointers except `pUnused` must be valid; `pfInterMdBackgroundDecision` and
/// `pfSCDPSkipDecision` must be assigned (`WelsInitBGDFunc` / `WelsInitSCDPskipFunc`).
pub unsafe extern "C" fn WelsMdInterMb(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    _pUnused: *mut SMbCache,
) {
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let kuiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let kiMbWidth = (*pCurDqLayer).iMbWidth as i32;
    // F14's class: formed before the availability guards below, read only under them.
    let top_mb = pCurMb.wrapping_offset(-(kiMbWidth as isize));
    let bMbLeftAvailPskip = if (kuiNeighborAvail & LEFT_MB_POS) != 0 {
        IS_SKIP((*pCurMb.offset(-1)).uiMbType)
    } else {
        false
    };
    let bMbTopAvailPskip = if (kuiNeighborAvail & TOP_MB_POS) != 0 {
        IS_SKIP((*top_mb).uiMbType)
    } else {
        false
    };
    let bMbTopLeftAvailPskip = if (kuiNeighborAvail & TOPLEFT_MB_POS) != 0 {
        IS_SKIP((*top_mb.offset(-1)).uiMbType)
    } else {
        false
    };
    let bMbTopRightAvailPskip = if (kuiNeighborAvail & TOPRIGHT_MB_POS) != 0 {
        IS_SKIP((*top_mb.offset(1)).uiMbType)
    } else {
        false
    };
    let bTrySkip =
        bMbLeftAvailPskip || bMbTopAvailPskip || bMbTopLeftAvailPskip || bMbTopRightAvailPskip;
    let mut bKeepSkip = bMbLeftAvailPskip && bMbTopAvailPskip && bMbTopRightAvailPskip;
    let bSkip;

    //try BGD skip
    if (*(*pEncCtx).pFuncList)
        .pfInterMdBackgroundDecision
        .expect("pfInterMdBackgroundDecision unset")(
        pEncCtx,
        pWelsMd,
        pSlice,
        pCurMb,
        pMbCache,
        &mut bKeepSkip as *mut bool,
    ) {
        return;
    }

    //try static or scrolled Pskip
    if (*(*pEncCtx).pFuncList)
        .pfSCDPSkipDecision
        .expect("pfSCDPSkipDecision unset")(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache)
    {
        return;
    }

    //step 1: try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache, bTrySkip);

    if bSkip {
        if bKeepSkip {
            WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb, pMbCache);
            return;
        }
    } else {
        PredictSad(
            (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
            (*pMbCache).iSadCost.as_mut_ptr(),
            0,
            &mut (*pWelsMd).iSadPredMb,
        );

        //step 2: P_16x16
        (*pWelsMd).iCostLuma = crate::encoder::svc_mode_decision::WelsMdP16x16(
            (*pEncCtx).pFuncList,
            pCurDqLayer,
            pWelsMd,
            pSlice,
            pCurMb,
        );
        (*pCurMb).uiMbType = MB_TYPE_16x16;
    }

    WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, pCurMb, pMbCache, bSkip);
}

/// `svc_base_layer_md.cpp:1937`. Re-classifies a zero-CBP 16x16 as P_SKIP when its MV
/// equals the skip predictor.
///
/// # Safety
/// `pCurMb` and `pMbCache` must be valid.
pub unsafe fn WelsMdInterDoubleCheckPskip(pCurMb: *mut SMB, pMbCache: *mut SMbCache) {
    if MB_TYPE_16x16 == (*pCurMb).uiMbType && 0 == (*pCurMb).uiCbp {
        if 0 == (*pCurMb).iRefIndex[0] {
            let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };

            PredSkipMv(pMbCache, &mut sMvp as *mut SMVUnitXY);
            if LD32_MV_PUB(&sMvp) == LD32_MV_PUB(&(*pCurMb).sMv[0]) {
                (*pCurMb).uiMbType = MB_TYPE_SKIP;
            }
        }
        (*pMbCache).bCollocatedPredFlag = LD32_MV_PUB(&(*pCurMb).sMv[0]) == 0;
    }
}

/// `LD32` on a motion vector — the 32-bit word an `SMVUnitXY` occupies. T6.C1 spells
/// the pun as the two halves it is, rather than reading a `u32` through the pair.
#[inline]
fn LD32_MV_PUB(pMv: &SMVUnitXY) -> u32 {
    let x = pMv.iMvX.to_ne_bytes();
    let y = pMv.iMvY.to_ne_bytes();
    u32::from_ne_bytes([x[0], x[1], y[0], y[1]])
}

/// `svc_base_layer_md.cpp:1964`. Transforms, quantises and reconstructs the chosen
/// inter macroblock, then copies the prediction into the CS planes.
///
/// # Safety
/// All four pointers must be valid; `pfCopy16x16Aligned`/`pfCopy8x8Aligned` assigned.
pub unsafe fn WelsMdInterEncode(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) {
    let pFunc = (*pEncCtx).pFuncList;
    let pCurDqLayer = (*pEncCtx).pCurDqLayer;

    //add pEnc&rec to MD--2010.3.15
    let kiCsStrideY = (*pCurDqLayer).iCsStride[0];
    let kiCsStrideUV = (*pCurDqLayer).iCsStride[1];

    //add pEnc&rec to MD--2010.3.15
    (*pCurMb).uiCbp = 0;
    crate::encoder::svc_mode_decision::WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    crate::encoder::svc_encode_slice::WelsPMbChromaEncode(pEncCtx, pSlice, pCurMb);

    (*pFunc).pfCopy16x16Aligned.expect("pfCopy16x16Aligned unset")(
        (*pMbCache).SPicData.pCsMb[0],
        kiCsStrideY,
        (*pMbCache).pMemPredLuma,
        16,
    );
    let copy8 = (*pFunc).pfCopy8x8Aligned.expect("pfCopy8x8Aligned unset");
    copy8(
        (*pMbCache).SPicData.pCsMb[1],
        kiCsStrideUV,
        (*pMbCache).pMemPredChroma,
        8,
    );
    copy8(
        (*pMbCache).SPicData.pCsMb[2],
        kiCsStrideUV,
        (*pMbCache).pMemPredChroma.add(64),
        8,
    );
}

/// `svc_base_layer_md.cpp:1987`. Records the skip SAD and the coded macroblock type
/// for the next frame's predictors.
///
/// # Safety
/// `pRefMbtypeList` must have room for `pCurMb->iMbXY`; `pMbCache->pEncSad` must be
/// assigned (`WelsMdInterInit` does this).
pub unsafe fn WelsMdInterSaveSadAndRefMbType(
    pRefMbtypeList: *mut u32,
    pMbCache: *mut SMbCache,
    pCurMb: *const SMB,
    pMd: *const SWelsMD,
) {
    let kmtCurMbtype = (*pCurMb).uiMbType;

    //sad
    *(*pMbCache).pEncSad.add(0) = if kmtCurMbtype == MB_TYPE_SKIP {
        (*pMd).iCostSkipMb
    } else {
        0
    };
    //uiMbType
    *pRefMbtypeList.offset((*pCurMb).iMbXY as isize) = kmtCurMbtype;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `g_kiIntra4AvailMode` row must list exactly `g_kiIntra4AvailCount` modes
    /// before its `I4_PRED_INVALID` padding; `WelsMdI4x4` walks the row by the count.
    /// `I4_PRED_INVALID` and `I4_PRED_V` are both 0 in C++, so the check is on the
    /// count agreeing with the transcription, not on scanning for a sentinel.
    #[test]
    fn intra4_avail_count_matches_mode_table() {
        // Rows whose count is 1 are the DC_128-only rows.
        for (idx, &count) in g_kiIntra4AvailCount.iter().enumerate() {
            assert!(count as usize <= 16, "row {idx} count out of range");
            if count == 1 {
                assert_eq!(g_kiIntra4AvailMode[idx][0], I4_PRED_DC_128, "row {idx}");
            }
        }
        // Spot-check the two rows the gate configuration spends most of its time in:
        // 0000 (first MB of the frame, nothing available) and 1111 (interior).
        assert_eq!(g_kiIntra4AvailCount[0], 1);
        assert_eq!(g_kiIntra4AvailCount[15], 9);
        assert_eq!(
            &g_kiIntra4AvailMode[15][..9],
            &[
                I4_PRED_DC, I4_PRED_H, I4_PRED_V, I4_PRED_HU, I4_PRED_DDL, I4_PRED_VL,
                I4_PRED_DDR, I4_PRED_VR, I4_PRED_HD
            ]
        );
    }

    /// `g_kiMapModeI4x4` must fold every extended mode onto a coded one in 0..9.
    #[test]
    fn map_mode_i4x4_folds_into_coded_range() {
        for (i, &m) in g_kiMapModeI4x4.iter().enumerate() {
            assert!((0..9).contains(&m), "g_kiMapModeI4x4[{i}] = {m}");
        }
        assert_eq!(g_kiMapModeI4x4[I4_PRED_DC_L as usize], I4_PRED_DC);
        assert_eq!(g_kiMapModeI4x4[I4_PRED_DC_T as usize], I4_PRED_DC);
        assert_eq!(g_kiMapModeI4x4[I4_PRED_DC_128 as usize], I4_PRED_DC);
        assert_eq!(g_kiMapModeI4x4[I4_PRED_DDL_TOP as usize], I4_PRED_DDL);
        assert_eq!(g_kiMapModeI4x4[I4_PRED_VL_TOP as usize], I4_PRED_VL);
    }

    /// `PredIntra4x4Mode` returns 2 (DC) when either neighbour is unavailable, and the
    /// smaller of the two mode ids otherwise (`svc_base_layer_md.cpp:246`).
    #[test]
    fn pred_intra4x4_mode_matches_reference() {
        let mut modes = [0i8; 48];
        let idx = 12usize; // any index with both idx-8 and idx-1 in range

        unsafe {
            modes[idx - 8] = 5;
            modes[idx - 1] = 3;
            assert_eq!(PredIntra4x4Mode(modes.as_ptr(), idx as i32), 3);

            modes[idx - 8] = 1;
            assert_eq!(PredIntra4x4Mode(modes.as_ptr(), idx as i32), 1);

            modes[idx - 1] = -1;
            assert_eq!(PredIntra4x4Mode(modes.as_ptr(), idx as i32), 2);

            modes[idx - 1] = 4;
            modes[idx - 8] = -1;
            assert_eq!(PredIntra4x4Mode(modes.as_ptr(), idx as i32), 2);
        }
    }

    /// The neighbour-to-availability table indexes `g_kiIntra4AvailCount`, so every
    /// entry must be a valid index into it.
    #[test]
    fn neighbor_intra_to_i4x4_indexes_are_in_range() {
        for (r, row) in g_kiNeighborIntraToI4x4.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                assert!(
                    (0..16).contains(&v),
                    "g_kiNeighborIntraToI4x4[{r}][{c}] = {v} out of range"
                );
            }
        }
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static FP_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
