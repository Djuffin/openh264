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
//! `svc_mode_decision.rs`, and `WelsMdInterMbLoop` in `svc_encode_slice.rs`.
//!
//! ## Deviation: the `Combined3` SIMD fast paths are not translated
//!
//! `WelsMdI16x16`, `WelsMdI4x4` and `WelsMdIntraChroma` each open with a branch taken
//! only when the corresponding `sSampleDealingFuncs.pfIntra*Combined3` slot is
//! non-null. Those slots are set exclusively from SIMD kernels in `sample.cpp`
//! (`_sse*`, `_neon`, `_AArch64_neon`, `_mmi`, `_lasx`), all of them behind a
//! `uiCpuFlag` test. This port has no SIMD kernels at all, so the slots are always
//! NULL here too. The scalar branches below are therefore the ones that decide
//! output bytes.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code)]

#![forbid(unsafe_code)]
use crate::encoder::rec_view::RecCursor;
use crate::encoder::rec_view::copy_block_to_view;
use crate::encoder::svc_encode_slice::{
    layer_enc_view, layer_rec_view, layer_ref_pic, layer_ref_view,
    current_layer_ref,
};
use crate::encoder::picture::{RecPicId, SrcPicId};
use crate::common::mc::{mc_chroma, mc_luma};
use crate::common::copy_mb::{copy_16x16, copy_16x8, copy_8x16, copy_8x8};
use crate::common::sad_common::sample_sad;
use crate::encoder::sample::{satd_16x16, satd_4x4};
use crate::safe::plane::{PlaneCursor, PlaneCursorMut};
use crate::encoder::encoder_context::{sWelsEncCtx, SMVComponentUnit, SMVUnitXY, SPicData};
use crate::encoder::md::{mem_pred_chroma_off, mem_pred_luma_off};
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
    UpdateP16x8Motion2Cache, UpdateP16x8MotionInfo, UpdateP8x16Motion2Cache,
    UpdateP8x8MotionInfo, WelsMdInterDecidedPskip, WelsMdInterJudgePskip,
    WelsMdInterSecondaryModesEnc, WelsMdIntraSecondaryModesEnc, BLOCK_16x16, BLOCK_16x8,
    BLOCK_4x4, BLOCK_4x8, BLOCK_8x16, BLOCK_8x4, BLOCK_8x8, IS_SKIP, MB_TYPE_BACKGROUND,
    REF_NOT_AVAIL, SUB_MB_TYPE_8x8,
};
use crate::encoder::svc_motion_estimate::{SetMvWithinIntegerMvRange, SWelsME};
use crate::encoder::svc_set_mb_syn_cavlc::{g_kuiCache48CountScan4Idx, IS_INTRA16x16};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::common::wels_common_defs::EWelsSliceType;
use crate::encoder::md::{LEFT_MB_POS, TOPLEFT_MB_POS, TOPRIGHT_MB_POS, TOP_MB_POS};
use crate::encoder::picture::SScreenBlockFeatureStorage;
use crate::encoder::svc_encode_slice::{current_layer_expect, layer_rec_view_expect, layer_ref_pic_expect, layer_ref_view_expect};
use crate::encoder::svc_encode_slice::layer_enc_view_expect;

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

/// `svc_base_layer_md.cpp:246`.
pub fn PredIntra4x4Mode(pIntraPredMode: &[i8; 48], iIdx4: i32) -> i32 {
    let iTopMode = pIntraPredMode[(iIdx4 - 8) as usize];
    let iLeftMode = pIntraPredMode[(iIdx4 - 1) as usize];

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
pub fn WelsMdIntraInit(
    pEncCtx: &sWelsEncCtx,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pMbCache: &mut SMbCache,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = current_layer_expect(pEncCtx);

    let kiMbX = mbs.cur().iMbX as i32;
    let kiMbY = mbs.cur().iMbY as i32;
    let kiMbXY = mbs.cur().iMbXY;

    // step 3. locating current pEnc and pDec
    pMbCache.SPicData.iMbX = kiMbX;
    pMbCache.SPicData.iMbY = kiMbY;

    //step 2. initial pWelsMd
    mbs.cur_mut().uiCbp = 0;

    //step 4: locating scaled_tcoeff

    //step 1. load neighbor cache
    FillNeighborCacheIntra(pMbCache, mbs);
    // in WelsMdI16x16() will be changed, so re-init here!
    // Init with default, maybe change in WelsMdI16x16 and svc_md_i16x16_sad:
    // luma is the first 256-byte half of `sMemPredMb` and chroma the second.
    pMbCache.uiMemPredLumaHalf = 0;
}

/// `svc_base_layer_md.cpp:418`. The full 16-mode-per-block I4x4 search, used on the
/// non-`LOW_COMPLEXITY` path via [`WelsMdIntraFinePartition`].
pub extern "C" fn WelsMdI4x4(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pFunc = pEncCtx.func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let iLambda = pWelsMd.iLambda;
    let iBestCostLuma = pWelsMd.iCostLuma;
    let view = layer_rec_view_expect(&*pCurDqLayer);

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[pMbCache.uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pEncPicture = layer_enc_view_expect(&*pCurDqLayer);
    let (kiMbOrgX, kiMbOrgY) = pMbCache.SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let pCurDec = view
            .plane(0)
            .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            &pMbCache.iIntraPredMode,
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

            let kiDstOff = ((1 - iBestPredBufferNum) << 4) as usize;
            let pDst: &mut [u8; 16] = (&mut pMbCache.sMemPredBlk4[kiDstOff..kiDstOff + 16])
                .try_into()
                .expect("a packed 4x4 prediction block is 16 bytes");
            pFunc.pfGetLumaI4x4Pred[iCurMode as usize].unwrap()(pDst, &pCurDec);
            let iCurCost = {
                let cPred = RecCursor::over_owned(
                    &mut pMbCache.sMemPredBlk4[((1 - iBestPredBufferNum) << 4) as usize..][..16],
                    0,
                    4,
                );
                let cEnc = pEncPicture
                    .plane(0)
                    .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize);
                satd_4x4(&cPred, &cEnc)
            } + lambda[(iPredMode == g_kiMapModeI4x4[iCurMode as usize] as i32) as usize];

            if iCurCost < iBestCost {
                iBestMode = iCurMode;
                iBestCost = iCurCost;
                iBestPredBufferNum = 1 - iBestPredBufferNum;
            }
        }

        pMbCache.uiBestPredI4x4Blk4Half = iBestPredBufferNum as u8;
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize] as i32;
        if iPredMode == iFinalMode {
            pMbCache.bPrevIntra4x4PredModeFlag[i] = true;
        } else {
            pMbCache.bPrevIntra4x4PredModeFlag[i] = false;
            pMbCache.iRemIntra4x4PredModeFlag[i] =
                (if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 }) as i8;
        }
        pMbCache.iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode as i8;

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
fn StoreIntra4x4PredModeToMb(pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    // ST32 (pCurMb->pIntra4x4PredMode, LD32 (&pMbCache->iIntraPredMode[33]));
    let pMbMode = &mut pCurMb.iIntra4x4PredMode;
    let pCacheMode = &pMbCache.iIntraPredMode;
    pMbMode[0..4].copy_from_slice(&pCacheMode[33..37]);
    pCurMb.iIntra4x4PredMode[4] = pMbCache.iIntraPredMode[12];
    pCurMb.iIntra4x4PredMode[5] = pMbCache.iIntraPredMode[20];
    pCurMb.iIntra4x4PredMode[6] = pMbCache.iIntraPredMode[28];
}

/// `svc_base_layer_md.cpp:548`. The `LOW_COMPLEXITY` I4x4 search: instead of scoring
/// every available mode it scores DC/H/V, then follows whichever of the vertical or
/// horizontal families won into at most four more modes.
pub extern "C" fn WelsMdI4x4Fast(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pFunc = pEncCtx.func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let iLambda = pWelsMd.iLambda;
    let iBestCostLuma = pWelsMd.iCostLuma;
    let view = layer_rec_view_expect(&*pCurDqLayer);

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[pMbCache.uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pfMdCost4x4 = pFunc.sSampleDealingFuncs.md_cost(BLOCK_4x4).unwrap();
    let pEncPicture = layer_enc_view_expect(&*pCurDqLayer);
    let (kiMbOrgX, kiMbOrgY) = pMbCache.SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let pCurDec = view
            .plane(0)
            .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            &pMbCache.iIntraPredMode,
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
            ($mode:expr, $dst_off:expr) => {{
                let m: i8 = $mode;
                let off: usize = $dst_off;
                pFunc.pfGetLumaI4x4Pred[m as usize].unwrap()(
                    (&mut pMbCache.sMemPredBlk4[off..off + 16])
                        .try_into()
                        .expect("a packed 4x4 prediction block is 16 bytes"),
                    &pCurDec,
                );
                pfMdCost4x4(
                    &RecCursor::over_owned(&mut pMbCache.sMemPredBlk4[off..][..16], 0, 4),
                    &pEncPicture
                        .plane(0)
                        .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize),
                ) + lambda[(iPredMode == g_kiMapModeI4x4[m as usize]) as usize]
            }};
        }
        macro_rules! alt_buf {
            () => {
                ((1 - iBestPredBufferNum) << 4) as usize
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
            iBestCost = score!(I4_PRED_DC, (iBestPredBufferNum << 4) as usize);

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

        pMbCache.uiBestPredI4x4Blk4Half = iBestPredBufferNum as u8;
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize];
        if iPredMode == iFinalMode {
            pMbCache.bPrevIntra4x4PredModeFlag[i] = true;
        } else {
            pMbCache.bPrevIntra4x4PredModeFlag[i] = false;
            pMbCache.iRemIntra4x4PredModeFlag[i] =
                if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 };
        }
        pMbCache.iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode;
        //step 6: encoding I_4x4
        WelsEncRecI4x4Y(pEncCtx, pCurMb, pMbCache, i as u8);
    }

    StoreIntra4x4PredModeToMb(pCurMb, pMbCache);
    iCosti4x4 += (iLambda << 4) + (iLambda << 3); //4*6*lambda from JVT SATD0
    iCosti4x4
}

/// `svc_base_layer_md.cpp:867`. Picks the 8x8 chroma prediction mode over Cb and Cr
/// jointly and leaves the winning prediction in `pBestPredIntraChroma`.
pub extern "C" fn WelsMdIntraChroma(
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: &SDqLayer,
    pMbCache: &mut SMbCache,
    iLambda: i32,
) -> i32 {
    let mut iChmaIdx: usize = 0;
    let view = layer_rec_view_expect(pCurDqLayer);

    let mut iBestCost = i32::MAX;

    let iOffset = (pMbCache.uiNeighborIntra & 0x07) as usize;
    let iAvailCount = g_kiIntraChromaAvailMode[iOffset][4] as i32;
    let kpAvailMode = &g_kiIntraChromaAvailMode[iOffset];

    let pfMdCost8x8 = pFunc.sSampleDealingFuncs.md_cost(BLOCK_8x8).unwrap();
    let pEncPicture = layer_enc_view_expect(pCurDqLayer);
    let (kiChrOrgX, kiChrOrgY) = pMbCache.SPicData.chroma_origin();
    let kiPredOff = mem_pred_chroma_off(pMbCache.uiMemPredLumaHalf);

    let mut iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount as usize {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        let pfChromaPred = pFunc.pfGetChromaPred[iCurMode as usize].unwrap();
        // `pDstChma` is `sMemPredMb` at the chroma half's `iChmaIdx` 128-byte side;
        // as an offset it is that side's start, and the Cr block sits 64 beyond it.
        let kiDstOff = kiPredOff + 128 * iChmaIdx;
        pfChromaPred(
            (&mut pMbCache.sMemPredMb[kiDstOff..kiDstOff + 64])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cb
        let mut iCurCost = pfMdCost8x8(
            &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiDstOff..][..64], 0, 8),
            &pEncPicture.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        );

        pfChromaPred(
            (&mut pMbCache.sMemPredMb[kiDstOff + 64..kiDstOff + 128])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cr
        iCurCost += pfMdCost8x8(
            &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiDstOff + 64..][..64], 0, 8),
            &pEncPicture.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ) + iLambda * BsSizeUE(crate::encoder::md::g_kiMapModeIntraChroma[iCurMode as usize] as u32) as i32;
        if iCurCost < iBestCost {
            iBestMode = iCurMode;
            iBestCost = iCurCost;
            iChmaIdx ^= 0x01;
        }
    }

    pMbCache.uiBestPredIntraChromaHalf = (iChmaIdx ^ 0x01) as u8;
    pMbCache.uiChmaI8x8Mode = iBestMode as u8;
    iBestCost
}

/// `svc_base_layer_md.cpp:932`. The non-`LOW_COMPLEXITY` `pfIntraFineMd`.
pub fn WelsMdIntraFinePartition(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let iCosti4x4 = WelsMdI4x4(pEncCtx, pWelsMd, pCurMb, pMbCache);

    if iCosti4x4 < pWelsMd.iCostLuma {
        pCurMb.uiMbType = MB_TYPE_INTRA4x4;
        pWelsMd.iCostLuma = iCosti4x4;
    }
    pWelsMd.iCostLuma
}

/// `svc_base_layer_md.cpp:942`. The `LOW_COMPLEXITY` `pfIntraFineMd`. Skips the I4x4
/// search entirely for macroblocks whose intra variance is below
/// `INTRA_VARIANCE_SAD_THRESHOLD`.
pub fn WelsMdIntraFinePartitionVaa(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);
    let encView = crate::encoder::svc_encode_slice::layer_enc_view_expect(&*pCurLayer);
    let cEncMb = pMbCache.SPicData.mb_cursor_ro(encView, 0);
    if MdIntraAnalysisVaaInfo(pEncCtx, &cEncMb) {
        let iCosti4x4 = WelsMdI4x4Fast(pEncCtx, pWelsMd, pCurMb, pMbCache);

        if iCosti4x4 < pWelsMd.iCostLuma {
            pCurMb.uiMbType = MB_TYPE_INTRA4x4;
            pWelsMd.iCostLuma = iCosti4x4;
        }
    }

    pWelsMd.iCostLuma
}

/// `svc_base_layer_md.cpp:956`. The whole intra mode decision for one macroblock:
/// score I16x16, then let `WelsMdIntraSecondaryModesEnc` try I4x4 and chroma and
/// reconstruct whichever won.
///
/// [`WelsMdIntraInit`] must have run for this macroblock.
pub fn WelsMdIntraMb(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    //initial prediction memory for I_16x16
    pWelsMd.iCostLuma = crate::encoder::svc_mode_decision::WelsMdI16x16(
        pEncCtx.func_list(),
        current_layer_ref(pEncCtx),
        pMbCache,
        pWelsMd.iLambda,
    );
    pCurMb.uiMbType = MB_TYPE_INTRA16x16;

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

/// `svc_base_layer_md.cpp:321`. Per-macroblock inter setup: neighbour cache, the
/// reference-plane pointers, and the integer MV clamp for this macroblock position.
pub fn WelsMdInterInit(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = current_layer_expect(pEncCtx);
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let kiMbX = mbs.cur().iMbX as i32;
    let kiMbY = mbs.cur().iMbY as i32;
    let kiMbXY = mbs.cur().iMbXY;
    let kiMbWidth = pCurLayer.iMbWidth as i32;
    let kiMbHeight = pCurLayer.iMbHeight as i32;

    //step 1. load neighbor cache
    (pEncCtx.func_list().pfFillInterNeighborCache)(
        &mut *pMbCache,
        &*mbs,
        &pEncCtx
            .vaa_expect().pVaaBackgroundMbFlag[..],
        layer_rec_view_expect(&*pCurLayer)
            .mb_skip_sad(),
    ); //BGD spatial pFunc

    //step 4. locating current p_ref
    pMbCache.SPicData.iMbX = kiMbX;
    pMbCache.SPicData.iMbY = kiMbY;

    pMbCache.uiRefMbType = (&layer_ref_pic_expect(pEncCtx, &*pCurLayer).uiRefMbType)[kiMbXY as usize];
    pMbCache.bCollocatedPredFlag = false;

    //comment: sometimes, mode decision process may skip the md_p16x16 and md_pskip function,
    mbs.cur_mut().sP16x16Mv = SMVUnitXY { iMvX: 0, iMvY: 0 };
    layer_rec_view_expect(&*pCurLayer)
        .mv_list()
        .set(kiMbXY as usize, SMVUnitXY { iMvX: 0, iMvY: 0 });

    SetMvWithinIntegerMvRange(
        kiMbWidth,
        kiMbHeight,
        kiMbX,
        kiMbY,
        pEncCtx.iMvRange,
        &mut pSlice.sMvStartMin,
        &mut pSlice.sMvStartMax,
    );
}

/// `svc_base_layer_md.cpp:1023`.
pub extern "C" fn WelsMdP16x8<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: &'a SDqLayer,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
) -> i32 {
    let mut iCostP16x8 = 0i32;
    for i in 0..2i32 {
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let sMe16x8 = &mut pWelsMd.sMe.sMe16x8[i as usize];
        let iPixelY = i << 3;
        InitMe(
            pWelsMd.iMbPixX,
            pWelsMd.iMbPixY,
            pWelsMd.pMvdCost,
            BLOCK_16x8 as i32,
            crate::encoder::svc_encode_slice::layer_ref_feature_storage(pEncCtx, &*pCurDqLayer),
            sMe16x8,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        sMe16x8.iCurMeBlockPixY = pWelsMd.iMbPixY + iPixelY;
        sMe16x8.uSadPredISatd.uiValue = (pWelsMd.iSadPredMb >> 1) as u32;

        pSlice.sMvc[0] = sMe16x8.sMvBase;
        pSlice.uiMvcNum = 1;

        PredInter16x8Mv(&pMbCache.sMvComponents, i << 3, 0, &mut sMe16x8.sMvp);
        {
            let pEncPicture = layer_enc_view_expect(pCurDqLayer);
            let pRefPicture = layer_ref_view_expect(pEncCtx, &*pCurDqLayer);
            pFunc.pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
                &pFunc.sMeFuncs,
                &pFunc.sSampleDealingFuncs,
                sMe16x8,
                &mut *pSlice,
                pEncPicture.plane(0),
                pRefPicture.plane(0),
            );
        }
        let pMbCache = &mut pSlice.sMbCacheInfo;
        UpdateP16x8Motion2Cache(
            &mut pMbCache.sMvComponents,
            i << 3,
            pWelsMd.uiRef as i8,
            &mut sMe16x8.sMv,
        );
        iCostP16x8 += sMe16x8.uiSatdCost as i32;
    }
    iCostP16x8
}

/// `svc_base_layer_md.cpp:1053`.
pub extern "C" fn WelsMdP8x16<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pFunc: &SWelsFuncPtrList,
    pCurLayer: &'a SDqLayer,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
) -> i32 {
    let mut iCostP8x16 = 0i32;
    for i in 0..2i32 {
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let iPixelX = i << 3;
        let sMe8x16 = &mut pWelsMd.sMe.sMe8x16[i as usize];
        InitMe(
            pWelsMd.iMbPixX,
            pWelsMd.iMbPixY,
            pWelsMd.pMvdCost,
            BLOCK_8x16 as i32,
            crate::encoder::svc_encode_slice::layer_ref_feature_storage(pEncCtx, &*pCurLayer),
            sMe8x16,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        sMe8x16.iCurMeBlockPixX = pWelsMd.iMbPixX + iPixelX;
        sMe8x16.uSadPredISatd.uiValue = (pWelsMd.iSadPredMb >> 1) as u32;

        pSlice.sMvc[0] = sMe8x16.sMvBase;
        pSlice.uiMvcNum = 1;

        PredInter8x16Mv(&pMbCache.sMvComponents, i << 2, 0, &mut sMe8x16.sMvp);
        {
            let pEncPicture = layer_enc_view_expect(pCurLayer);
            let pRefPicture = layer_ref_view_expect(pEncCtx, &*pCurLayer);
            pFunc.pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
                &pFunc.sMeFuncs,
                &pFunc.sSampleDealingFuncs,
                sMe8x16,
                &mut *pSlice,
                pEncPicture.plane(0),
                pRefPicture.plane(0),
            );
        }
        let pMbCache = &mut pSlice.sMbCacheInfo;
        UpdateP8x16Motion2Cache(
            &mut pMbCache.sMvComponents,
            i << 2,
            pWelsMd.uiRef as i8,
            &mut sMe8x16.sMv,
        );
        iCostP8x16 += sMe8x16.uiSatdCost as i32;
    }
    iCostP8x16
}

/// `svc_base_layer_md.cpp:1238`. The non-VAA (`!LOW_COMPLEXITY`) fine partition search.
pub fn WelsMdInterFinePartition<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    iBestCost: i32,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let mut iCost = crate::encoder::svc_mode_decision::WelsMdP8x8(
        pEncCtx,
        pEncCtx.func_list(),
        &*pCurDqLayer,
        pWelsMd,
        pSlice,
    );

    if iCost < iBestCost {
        pCurMb.uiMbType = MB_TYPE_8x8;
        pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];

        let mut iCostPart = WelsMdP16x8(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            iCost = iCostPart;
            pCurMb.uiMbType = MB_TYPE_16x8;
        }

        iCostPart = WelsMdP8x16(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            pCurMb.uiMbType = MB_TYPE_8x16;
        }
    }
}

/// `svc_base_layer_md.cpp:1270`. The VAA-guided fine partition search — the
/// `LOW_COMPLEXITY` path the gate configuration takes.
///
/// `pEncCtx->pVaa->sVaaCalcInfo.pSad8x8` must be populated and
/// `pfGetMbSignFromInterVaa` assigned.
pub fn WelsMdInterFinePartitionVaa<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    iBestCostIn: i32,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let mut iBestCost = iBestCostIn;
    let uiMbSign = (pEncCtx.func_list().pfGetMbSignFromInterVaa)(
        &pEncCtx.vaa_expect().sVaaCalcInfo.pSad8x8
            [pCurMb.iMbXY as usize],
    );

    if crate::encoder::dump_enabled(&FP_DUMP, "OH264_FPDUMP") {
        let sad = (&pEncCtx.vaa_expect().sVaaCalcInfo.pSad8x8)[pCurMb.iMbXY as usize];
        eprintln!(
            "FP mb={:3} sign={:2} best={:7} sad8x8={},{},{},{}",
            pCurMb.iMbXY,
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
            let iCostP16x8 = WelsMdP16x8(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
            if iCostP16x8 < iBestCost {
                iBestCost = iCostP16x8;
                pCurMb.uiMbType = MB_TYPE_16x8;
            }
        }
        5 | 10 => {
            let iCostP8x16 = WelsMdP8x16(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x16 < iBestCost {
                iBestCost = iCostP8x16;
                pCurMb.uiMbType = MB_TYPE_8x16;
            }
        }
        6 | 9 => {
            let iCostP8x8 = crate::encoder::svc_mode_decision::WelsMdP8x8(
        pEncCtx,
        pEncCtx.func_list(),
                &*pCurDqLayer,
                pWelsMd,
                pSlice,
            );
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                pCurMb.uiMbType = MB_TYPE_8x8;
                pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];
            }
        }
        _ => {
            let iCostP8x8 = crate::encoder::svc_mode_decision::WelsMdP8x8(
        pEncCtx,
        pEncCtx.func_list(),
                &*pCurDqLayer,
                pWelsMd,
                pSlice,
            );
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                pCurMb.uiMbType = MB_TYPE_8x8;
                pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];

                let iCostP16x8 = WelsMdP16x8(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
                if iCostP16x8 <= iBestCost {
                    iBestCost = iCostP16x8;
                    pCurMb.uiMbType = MB_TYPE_16x8;
                }

                let iCostP8x16 = WelsMdP8x16(pEncCtx, pEncCtx.func_list(), &*pCurDqLayer, pWelsMd, pSlice);
                if iCostP8x16 <= iBestCost {
                    iBestCost = iCostP8x16;
                    pCurMb.uiMbType = MB_TYPE_8x16;
                }
            }
        }
    }
    pWelsMd.iCostLuma = iBestCost;
}

/// `svc_base_layer_md.cpp:1423`. Motion-compensates the P_SKIP predictor and decides
/// whether the macroblock can be coded as P_SKIP.
pub fn WelsMdPSkipEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let pCurLayer = current_layer_expect(pEncCtx);
    let pFunc = pEncCtx.func_list();

    let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
    let mut n: i32;

    let encView = crate::encoder::svc_encode_slice::layer_enc_view_expect(&*pCurLayer);
    let mut pEncMb = pMbCache.SPicData.mb_cursor_ro(encView, 0);
    let kpEncBlockOffset = pEncCtx
        .pStrideTab
        .as_ref()
        .and_then(|tab| tab.EncBlockOffsets(pEncCtx.uiDependencyId as usize))
        .expect("AllocStrideTables builds the block-offset table for every layer");

    let iSadCostLuma: i32;
    let mut iSadCostChroma: i32;
    let iSadCostMb: i32;

    PredSkipMv(&pMbCache.sMvComponents, &mut sMvp);

    // Special case, need to clip the vector //
    let sQpelMvp = SMVUnitXY {
        iMvX: (sMvp.iMvX >> 2) as i16,
        iMvY: (sMvp.iMvY >> 2) as i16,
    };
    n = ((pCurMb.iMbX as i32) << 4) + sQpelMvp.iMvX as i32;
    if n < -29 {
        return false;
    } else if n > (((pCurLayer.iMbWidth as i32) << 4) + 12) {
        return false;
    }

    n = ((pCurMb.iMbY as i32) << 4) + sQpelMvp.iMvY as i32;
    if n < -29 {
        return false;
    } else if n > (((pCurLayer.iMbHeight as i32) << 4) + 12) {
        return false;
    }

    let kiMbXLuma = (pCurMb.iMbX as isize) << 4;
    let kiMbYLuma = (pCurMb.iMbY as isize) << 4;
    let kiMbXChroma = (pCurMb.iMbX as isize) << 3;
    let kiMbYChroma = (pCurMb.iMbY as isize) << 3;

    //luma
    {
        let pRefPicture = layer_ref_view_expect(pEncCtx, &*pCurLayer);
        let cRefLuma = pRefPicture.plane(0).cursor(
            kiMbXLuma + sQpelMvp.iMvX as isize,
            kiMbYLuma + sQpelMvp.iMvY as isize,
        );
        let mut cDstLuma = PlaneCursorMut::new(&mut pMbCache.sSkipMb[..256], 0, 16);
        mc_luma(&cRefLuma, &mut cDstLuma, sMvp.iMvX, sMvp.iMvY, 16, 16);
    }
    iSadCostLuma = {
        let pEncPicture = layer_enc_view_expect(&*pCurLayer);
        let cEncLuma = pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        let cSkipLuma = RecCursor::over_owned(&mut pMbCache.sSkipMb[..256], 0, 16);
        sample_sad::<16, 16, _>(&cEncLuma, &cSkipLuma)
    };

    // `iStrideUV` was `(mvY >> 1) * strideUV + (mvX >> 1)` off the chroma macroblock
    // origin; in samples that is `(mvX >> 1, mvY >> 1)` from the same origin, and
    // `sQpelMvp` is already `sMvp >> 2`, so both are `sMvp >> 3`.
    {
        let pRefPicture = layer_ref_view_expect(pEncCtx, &*pCurLayer);
        let cRefCb = pRefPicture.plane(1).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCb = PlaneCursorMut::new(&mut pMbCache.sSkipMb[256..320], 0, 8);
        mc_chroma(&cRefCb, &mut cDstCb, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cb
    }
    iSadCostChroma = {
        let pEncPicture = layer_enc_view_expect(&*pCurLayer);
        let cEncCb = pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
        let cSkipCb = RecCursor::over_owned(&mut pMbCache.sSkipMb[256..320], 0, 8);
        sample_sad::<8, 8, _>(&cEncCb, &cSkipCb)
    };

    {
        let pRefPicture = layer_ref_view_expect(pEncCtx, &*pCurLayer);
        let cRefCr = pRefPicture.plane(2).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCr = PlaneCursorMut::new(&mut pMbCache.sSkipMb[320..384], 0, 8);
        mc_chroma(&cRefCr, &mut cDstCr, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cr
    }
    iSadCostChroma += {
        let pEncPicture = layer_enc_view_expect(&*pCurLayer);
        let cEncCr = pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
        let cSkipCr = RecCursor::over_owned(&mut pMbCache.sSkipMb[320..384], 0, 8);
        sample_sad::<8, 8, _>(&cEncCr, &cSkipCr)
    };

    iSadCostMb = iSadCostLuma + iSadCostChroma;

    if iSadCostMb == 0
        || iSadCostMb < pWelsMd.iSadPredSkip
        || (layer_ref_pic(pEncCtx, &*pCurLayer).map_or(false, |p| p.iPictureType == EWelsSliceType::P_SLICE as i32)
            && pMbCache.uiRefMbType == MB_TYPE_SKIP
            && iSadCostMb < (&layer_ref_pic_expect(pEncCtx, &*pCurLayer).pMbSkipSad)[pCurMb.iMbXY as usize])
    {
        //update motion info to current MB
        AcceptPskip(pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
        return true;
    }

    let pDstLuma = RecCursor::over_owned(&mut pMbCache.sSkipMb, 0, 16);
    WelsDctMb(
        &mut pMbCache.sCoeffLevel,
        &pEncMb,
        &pDstLuma,
        pEncCtx.func_list().pfDctFourT4,
    );

    if WelsTryPYskip(pEncCtx, pCurMb, pMbCache) {
        pEncMb = pMbCache.SPicData.mb_cursor_ro(encView, 1);

        let pDstCb = RecCursor::over_owned(&mut pMbCache.sSkipMb, 256, 8);
        (pFunc.pfDctFourT4)(
            &mut pMbCache.sCoeffLevel[256..],
            &pEncMb.advance(kpEncBlockOffset[16] as isize, 0),
            &pDstCb,
        );
        if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 1) {
            pEncMb = pMbCache.SPicData.mb_cursor_ro(encView, 2);

            let pDstCr = RecCursor::over_owned(&mut pMbCache.sSkipMb, 320, 8);
            (pFunc.pfDctFourT4)(
                &mut pMbCache.sCoeffLevel[320..],
                &pEncMb.advance(kpEncBlockOffset[20] as isize, 0),
                &pDstCr,
            );
            if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 2) {
                //update motion info to current MB
                AcceptPskip(pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
                return true;
            }
        }
    }
    false
}

/// The block `WelsMdPSkipEnc` runs verbatim at both of its `return true` sites
/// (`svc_base_layer_md.cpp:1489` and `:1521`).
#[inline]
fn AcceptPskip(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &SMbCache,
    sMvp: &SMVUnitXY,
    iSadCostLuma: i32,
    iSadCostMb: i32,
) {
    let pCurLayer = current_layer_expect(pEncCtx);
    let pFunc = pEncCtx.func_list();

    // ST32 (pCurMb->pRefIndex, 0)
    pCurMb.iRefIndex = [0; crate::encoder::md::MB_BLOCK8x8_NUM];
    (pFunc.pfUpdateMbMv)(&mut pCurMb.sMv, *sMvp);

    if pWelsMd.bMdUsingSad {
        pCurMb.iSadCost = iSadCostLuma;
        pWelsMd.iCostLuma = pCurMb.iSadCost;
    } else {
        let pEncPicture = layer_enc_view_expect(&*pCurLayer);
        let cEncLuma = pEncPicture
            .plane(0)
            .cursor((pCurMb.iMbX as isize) << 4, (pCurMb.iMbY as isize) << 4);
        let cSkipLuma = PlaneCursor::new(&pMbCache.sSkipMb[..256], 0, 16);
        pWelsMd.iCostLuma = satd_16x16(&cEncLuma, &cSkipLuma);
    }

    pWelsMd.iCostSkipMb = iSadCostMb;

    pCurMb.sP16x16Mv = *sMvp;
    layer_rec_view_expect(&*pCurLayer)
        .mv_list()
        .set(pCurMb.iMbXY as usize, *sMvp);
}

/// `svc_base_layer_md.cpp:1573`. Quarter-pel refinement of whichever partitioning the
/// integer search chose, plus the chroma motion compensation for each partition.
pub fn WelsMdInterMbRefinement(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let pFunc = pEncCtx.func_list();
    let mut iBestSadCost = 0i32;
    let mut iBestSatdCost = 0i32;
    let mut sMeRefine = SMeRefinePointer::default();

    let kiMbXChroma = (pCurMb.iMbX as isize) << 3;
    let kiMbYChroma = (pCurMb.iMbY as isize) << 3;

    // Byte offsets of the three prediction regions inside `sMemPredMb`.
    let kiOffLuma = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
    let kiOffCb = mem_pred_chroma_off(pMbCache.uiMemPredLumaHalf);
    let kiOffCr = kiOffCb + 64;

    /// One chroma motion compensation, per partition. `$plane` is 1 (Cb) or 2 (Cr);
    /// `($dx, $dy)` is the partition's own chroma offset **plus** the motion vector's
    /// integer chroma part, in samples from the macroblock's chroma origin; `$off` is
    /// the destination's byte offset inside `sMemPredMb`, whose prediction rows are
    /// 8 samples apart.
    ///
    /// The destination slice is exactly the block's span, `($h - 1) * 8 + $w`.
    macro_rules! mc_chroma_at {
        ($plane:expr, $off:expr, $dx:expr, $dy:expr, $mv:expr, $w:expr, $h:expr) => {{
            let pRefPicture =
                layer_ref_view_expect(pEncCtx, &*pCurDqLayer);
            let cRef = pRefPicture
                .plane($plane)
                .cursor(kiMbXChroma + ($dx) as isize, kiMbYChroma + ($dy) as isize);
            let mut cDst = PlaneCursorMut::new(
                &mut pMbCache.sMemPredMb[($off)..][..(($h) - 1) * 8 + ($w)],
                0,
                8,
            );
            mc_chroma(&cRef, &mut cDst, ($mv).iMvX, ($mv).iMvY, $w, $h);
        }};
    }

    match pCurMb.uiMbType {
        MB_TYPE_16x16 => {
            //luma
            InitMeRefinePointer(&mut sMeRefine, 0);
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_16x16(a, b));
            MeRefineFracPixel(
                pEncCtx,
                kiOffLuma,
                &mut pWelsMd.sMe.sMe16x16,
                &mut sMeRefine,
                pMbCache,
                16,
                16,
            );
            UpdateP16x16MotionInfo(
                &mut pMbCache.sMvComponents,
                pCurMb,
                pWelsMd.uiRef as i8,
                &mut pWelsMd.sMe.sMe16x16.sMv,
            );

            pMbCache.sMbMvp[0] = pWelsMd.sMe.sMe16x16.sMvp;
            //save the best cost of final mode
            iBestSadCost = pWelsMd.sMe.sMe16x16.uiSadCost as i32;
            iBestSatdCost = pWelsMd.sMe.sMe16x16.uiSatdCost as i32;

            //chroma
            let sMv = pWelsMd.sMe.sMe16x16.sMv;
            let dx = sMv.iMvX as i32 >> 3;
            let dy = sMv.iMvY as i32 >> 3;
            mc_chroma_at!(1, kiOffCb, dx, dy, sMv, 8, 8); //Cb
            mc_chroma_at!(2, kiOffCr, dx, dy, sMv, 8, 8); //Cr

            let pEncPicture =
                layer_enc_view_expect(&*pCurDqLayer);
            let cEncLuma = pEncPicture.plane(0).cursor(kiMbXChroma << 1, kiMbYChroma << 1);
            let cEncCb = pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
            let cEncCr = pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
            pWelsMd.iCostSkipMb = sample_sad::<16, 16, _>(
                &cEncLuma,
                &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiOffLuma..][..256], 0, 16),
            );
            pWelsMd.iCostSkipMb += sample_sad::<8, 8, _>(
                &cEncCb,
                &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiOffCb..][..64], 0, 8),
            );
            pWelsMd.iCostSkipMb += sample_sad::<8, 8, _>(
                &cEncCr,
                &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiOffCr..][..64], 0, 8),
            );
        }

        MB_TYPE_16x8 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_16x8(a, b));
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 3;
                InitMeRefinePointer(&mut sMeRefine, iPixStride);
                iPixStride += ME_REFINE_BUF_STRIDE_BLK8;
                PredInter16x8Mv(
                    &pMbCache.sMvComponents,
                    iIdx,
                    pWelsMd.uiRef as i8,
                    &mut pWelsMd.sMe.sMe16x8[i].sMvp,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    kiOffLuma + g_kuiSmb4AddrIn256[iIdx as usize] as usize,
                    &mut pWelsMd.sMe.sMe16x8[i],
                    &mut sMeRefine,
                pMbCache,
                    16,
                    8,
                );
                UpdateP16x8MotionInfo(
                    &mut pMbCache.sMvComponents,
                    pCurMb,
                    iIdx,
                    pWelsMd.uiRef as i8,
                    &mut pWelsMd.sMe.sMe16x8[i].sMv,
                );
                pMbCache.sMbMvp[i] = pWelsMd.sMe.sMe16x8[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += pWelsMd.sMe.sMe16x8[i].uiSadCost as i32;
                iBestSatdCost += pWelsMd.sMe.sMe16x8[i].uiSatdCost as i32;

                //chroma
                // `iRefBlk4Stride` was `(i << 2) * strideUV` — a pure row offset, so
                // the partition sits `4 * i` rows down and in column 0; the
                // destination's `i << 5` is `4 * i` rows at stride 8, the same place.
                let iBlk4Y = (i as i32) << 2;
                let sMv = pWelsMd.sMe.sMe16x8[i].sMv;
                let dx = sMv.iMvX as i32 >> 3;
                let dy = iBlk4Y + (sMv.iMvY as i32 >> 3);
                let iDstOff = (i as usize) << 5; // 4 rows x 8
                mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 8, 4); //Cb
                mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 8, 4); //Cr
            }
        }

        MB_TYPE_8x16 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_8x16(a, b));
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 2;
                InitMeRefinePointer(&mut sMeRefine, iPixStride);
                iPixStride += ME_REFINE_BUF_WIDTH_BLK8;
                PredInter8x16Mv(
                    &pMbCache.sMvComponents,
                    iIdx,
                    pWelsMd.uiRef as i8,
                    &mut pWelsMd.sMe.sMe8x16[i].sMvp,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    kiOffLuma + g_kuiSmb4AddrIn256[iIdx as usize] as usize,
                    &mut pWelsMd.sMe.sMe8x16[i],
                    &mut sMeRefine,
                pMbCache,
                    8,
                    16,
                );
                update_P8x16_motion_info(
                    &mut pMbCache.sMvComponents,
                    pCurMb,
                    iIdx,
                    pWelsMd.uiRef as i8,
                    &mut pWelsMd.sMe.sMe8x16[i].sMv,
                );
                pMbCache.sMbMvp[i] = pWelsMd.sMe.sMe8x16[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += pWelsMd.sMe.sMe8x16[i].uiSadCost as i32;
                iBestSatdCost += pWelsMd.sMe.sMe8x16[i].uiSatdCost as i32;

                //chroma
                // `iRefBlk4Stride` was `iIdx` (= 4 * i) added to a byte pointer with no
                // stride factor — a pure *column* offset — and the destination used the
                // same number, which at stride 8 is also column `4 * i` of row 0.
                let iBlk4X = iIdx; // 4 * i
                let sMv = pWelsMd.sMe.sMe8x16[i].sMv;
                let dx = iBlk4X + (sMv.iMvX as i32 >> 3);
                let dy = sMv.iMvY as i32 >> 3;
                let iDstOff = iBlk4X as usize;
                mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 4, 8); //Cb
                mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 4, 8); //Cr
            }
        }

        MB_TYPE_8x8 => {
            pMbCache.sMvComponents.iRefIndexCache[9] = REF_NOT_AVAIL;
            pMbCache.sMvComponents.iRefIndexCache[21] = REF_NOT_AVAIL;
            for i in 0..4usize {
                let iBlk8Idx = (i as i32) << 2; //0, 4, 8, 12

                pCurMb.iRefIndex[i] = pWelsMd.uiRef as i8;
                match pCurMb.uiSubMbType[i] {
                    SUB_MB_TYPE_8x8 => {
                        sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_8x8(a, b));
                        //luma
                        InitMeRefinePointer(&mut sMeRefine, g_kiPixStrideIdx8x8[i]);
                        PredMv(
                            &pMbCache.sMvComponents,
                            iBlk8Idx as i8,
                            2,
                            pWelsMd.uiRef as i32,
                            &mut pWelsMd.sMe.sMe8x8[i].sMvp,
                        );
                        MeRefineFracPixel(
                            pEncCtx,
                            kiOffLuma + g_kuiSmb4AddrIn256[iBlk8Idx as usize] as usize,
                            &mut pWelsMd.sMe.sMe8x8[i],
                            &mut sMeRefine,
                pMbCache,
                            8,
                            8,
                        );
                        UpdateP8x8MotionInfo(
                            &mut pMbCache.sMvComponents,
                            pCurMb,
                            iBlk8Idx,
                            pWelsMd.uiRef as i8,
                            &mut pWelsMd.sMe.sMe8x8[i].sMv,
                        );
                        pMbCache.sMbMvp[g_kuiMbCountScan4Idx[iBlk8Idx as usize] as usize] =
                            pWelsMd.sMe.sMe8x8[i].sMvp;
                        iBestSadCost += pWelsMd.sMe.sMe8x8[i].uiSadCost as i32;
                        iBestSatdCost += pWelsMd.sMe.sMe8x8[i].uiSatdCost as i32;

                        //chroma
                        let sMv = pWelsMd.sMe.sMe8x8[i].sMv;
                        let iBlk4X = ((i as i32) & 1) << 2;
                        let iBlk4Y = ((i as i32) >> 1) << 2;
                        let dx = iBlk4X + (sMv.iMvX as i32 >> 3);
                        let dy = iBlk4Y + (sMv.iMvY as i32 >> 3);
                        // `iDstBlk4Stride` was `(iBlk4Y << 3) + iBlk4X`, which is the
                        // coordinate `(iBlk4X, iBlk4Y)` at stride 8.
                        let iDstOff = ((iBlk4Y << 3) + iBlk4X) as usize;
                        mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 4, 4); //Cb
                        mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 4, 4); //Cr
                    }
                    // In the port, every writer of `uiSubMbType` sets
                    // `SUB_MB_TYPE_8x8`; in upstream the only writers are inside
                    // `WelsMdInterFinePartitionVaaOnScreen`'s
                    // `#if 0 //Disable for sub8x8 modes for now`
                    // (`svc_mode_decision.cpp:634-661`).
                    _ => unreachable!(
                        "sub-8x8 partition {:#x} — the sub-8x8 search is #if 0 upstream \
                         and unwritten here (D-dead-2/F122)",
                        pCurMb.uiSubMbType[i]
                    ),
                }
            }
        }
        _ => {}
    }
    pCurMb.iSadCost = iBestSadCost;
    if pWelsMd.bMdUsingSad {
        pWelsMd.iCostLuma = iBestSadCost;
    } else {
        pWelsMd.iCostLuma = iBestSatdCost;
    }
}

/// `svc_base_layer_md.cpp:1829`. Costs I16x16 against the current inter cost and, if
/// intra wins, runs the whole intra encode for this macroblock.
pub fn WelsMdFirstIntraMode(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let pFunc = pEncCtx.func_list();

    let iCostI16x16 = crate::encoder::svc_mode_decision::WelsMdI16x16(
        &*pFunc,
        current_layer_ref(pEncCtx),
        pMbCache,
        pWelsMd.iLambda,
    );

    //compare cost_p16x16 with cost_i16x16
    if iCostI16x16 < pWelsMd.iCostLuma {
        pCurMb.uiMbType = MB_TYPE_INTRA16x16;
        pWelsMd.iCostLuma = iCostI16x16;

        pFunc.pfIntraFineMd.expect("pfIntraFineMd unset")(pEncCtx, pWelsMd, pCurMb, pMbCache);

        //add pEnc&rec to MD--2010.3.15
        if IS_INTRA16x16(pCurMb.uiMbType) {
            pCurMb.uiCbp = 0;
            crate::encoder::svc_encode_mb::WelsEncRecI16x16Y(pEncCtx, pCurMb, pMbCache);
        }

        //chroma
        pWelsMd.iCostChroma =
            WelsMdIntraChroma(&*pFunc, current_layer_expect(pEncCtx), pMbCache, pWelsMd.iLambda);
        crate::encoder::svc_encode_slice::WelsIMbChromaEncode(pEncCtx, pCurMb, pMbCache); //add pEnc&rec to MD--2010.3.15
        pCurMb.uiChromPredMode = pMbCache.uiChmaI8x8Mode as u32;
        pCurMb.iSadCost = 0;
        return true; //intra_mb_type is best
    }

    false
}

/// `svc_base_layer_md.cpp:1858`. The P-slice per-macroblock entry point; C++ assigns
/// it to `pfInterMd` in `WelsCodePSlice` (`svc_encode_slice.cpp:736`).
pub fn WelsMdInterMb<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let kuiNeighborAvail = mbs.cur().uiNeighborAvail as u32;
    let bMbLeftAvailPskip = if (kuiNeighborAvail & LEFT_MB_POS) != 0 {
        IS_SKIP(mbs.left().uiMbType)
    } else {
        false
    };
    let bMbTopAvailPskip = if (kuiNeighborAvail & TOP_MB_POS) != 0 {
        IS_SKIP(mbs.top().uiMbType)
    } else {
        false
    };
    let bMbTopLeftAvailPskip = if (kuiNeighborAvail & TOPLEFT_MB_POS) != 0 {
        IS_SKIP(mbs.top_left().uiMbType)
    } else {
        false
    };
    let bMbTopRightAvailPskip = if (kuiNeighborAvail & TOPRIGHT_MB_POS) != 0 {
        IS_SKIP(mbs.top_right().uiMbType)
    } else {
        false
    };
    let bTrySkip =
        bMbLeftAvailPskip || bMbTopAvailPskip || bMbTopLeftAvailPskip || bMbTopRightAvailPskip;
    let mut bKeepSkip = bMbLeftAvailPskip && bMbTopAvailPskip && bMbTopRightAvailPskip;
    let bSkip;

    //try BGD skip
    if (pEncCtx.func_list().pfInterMdBackgroundDecision)(
        pEncCtx,
        pWelsMd,
        pSlice,
        mbs.cur_mut(),
        &mut bKeepSkip,
    ) {
        return;
    }

    //try static or scrolled Pskip
    if (pEncCtx.func_list().pfSCDPSkipDecision)(pEncCtx, pWelsMd, pSlice, mbs.cur_mut())
    {
        return;
    }

    //step 1: try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bTrySkip);

    if bSkip {
        if bKeepSkip {
            WelsMdInterDecidedPskip(pEncCtx, pSlice, mbs.cur_mut());
            return;
        }
    } else {
        let pMbCache = &mut pSlice.sMbCacheInfo;
        PredictSad(
            &pMbCache.sMvComponents.iRefIndexCache,
            &pMbCache.iSadCost,
            0,
            &mut pWelsMd.iSadPredMb,
        );

        //step 2: P_16x16
        pWelsMd.iCostLuma = crate::encoder::svc_mode_decision::WelsMdP16x16(
            pEncCtx,
            pEncCtx.func_list(),
            &*pCurDqLayer,
            pWelsMd,
            pSlice,
            mbs,
        );
        mbs.cur_mut().uiMbType = MB_TYPE_16x16;
    }

    WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bSkip);
}

/// `svc_base_layer_md.cpp:1937`. Re-classifies a zero-CBP 16x16 as P_SKIP when its MV
/// equals the skip predictor.
pub fn WelsMdInterDoubleCheckPskip(pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    if MB_TYPE_16x16 == pCurMb.uiMbType && 0 == pCurMb.uiCbp {
        if 0 == pCurMb.iRefIndex[0] {
            let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };

            PredSkipMv(&pMbCache.sMvComponents, &mut sMvp);
            if LD32_MV_PUB(&sMvp) == LD32_MV_PUB(&pCurMb.sMv[0]) {
                pCurMb.uiMbType = MB_TYPE_SKIP;
            }
        }
        pMbCache.bCollocatedPredFlag = LD32_MV_PUB(&pCurMb.sMv[0]) == 0;
    }
}

/// `LD32` on a motion vector — the 32-bit word an `SMVUnitXY` occupies.
#[inline]
fn LD32_MV_PUB(pMv: &SMVUnitXY) -> u32 {
    let x = pMv.iMvX.to_ne_bytes();
    let y = pMv.iMvY.to_ne_bytes();
    u32::from_ne_bytes([x[0], x[1], y[0], y[1]])
}

/// `svc_base_layer_md.cpp:1964`. Transforms, quantises and reconstructs the chosen
/// inter macroblock, then copies the prediction into the CS planes.
pub fn WelsMdInterEncode(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pFunc = pEncCtx.func_list();
    let pCurDqLayer = current_layer_expect(pEncCtx);

    //add pEnc&rec to MD--2010.3.15
    let kiCsStrideY = pCurDqLayer.iCsStride[0];
    let kiCsStrideUV = pCurDqLayer.iCsStride[1];

    //add pEnc&rec to MD--2010.3.15
    pCurMb.uiCbp = 0;
    crate::encoder::svc_mode_decision::WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    crate::encoder::svc_encode_slice::WelsPMbChromaEncode(pEncCtx, pSlice, pCurMb);

    let view = layer_rec_view_expect(&*pCurDqLayer);
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let (lx, ly) = pMbCache.SPicData.luma_origin();
    let (cx, cy) = pMbCache.SPicData.chroma_origin();
    let kiLumaOff = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
    let kiChromaOff = mem_pred_chroma_off(pMbCache.uiMemPredLumaHalf);
    let src = &pMbCache.sMemPredMb;
    copy_block_to_view::<16>(&src[kiLumaOff..kiLumaOff + 256], 16, &view.plane(0).cursor(lx, ly), 16);
    copy_block_to_view::<8>(&src[kiChromaOff..kiChromaOff + 64], 8, &view.plane(1).cursor(cx, cy), 8);
    copy_block_to_view::<8>(
        &src[kiChromaOff + 64..kiChromaOff + 128],
        8,
        &view.plane(2).cursor(cx, cy),
        8,
    );
}

/// `svc_base_layer_md.cpp:1987`. Records the skip SAD and the coded macroblock type
/// for the next frame's predictors.
///
/// Both arrays must have room for `pCurMb->iMbXY`.
pub fn WelsMdInterSaveSadAndRefMbType(
    pRecView: &crate::encoder::rec_view::RecPicView,
    pCurMb: &SMB,
    pMd: &SWelsMD<'_>,
) {
    let kmtCurMbtype = pCurMb.uiMbType;
    let kiMbXY = pCurMb.iMbXY as usize;

    //sad
    pRecView.mb_skip_sad().set(
        kiMbXY,
        if kmtCurMbtype == MB_TYPE_SKIP { pMd.iCostSkipMb } else { 0 },
    );
    //uiMbType
    pRecView.ref_mb_type().set(kiMbXY, kmtCurMbtype);
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

        modes[idx - 8] = 5;
        modes[idx - 1] = 3;
        assert_eq!(PredIntra4x4Mode(&modes, idx as i32), 3);

        modes[idx - 8] = 1;
        assert_eq!(PredIntra4x4Mode(&modes, idx as i32), 1);

        modes[idx - 1] = -1;
        assert_eq!(PredIntra4x4Mode(&modes, idx as i32), 2);

        modes[idx - 1] = 4;
        modes[idx - 8] = -1;
        assert_eq!(PredIntra4x4Mode(&modes, idx as i32), 2);
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
