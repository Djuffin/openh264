//! Base-layer mode decision — `codec/encoder/core/src/svc_base_layer_md.cpp`.
//!
//! The intra (I-slice) half lives here: the tables, the neighbour-mode predictor, the
//! `WelsMdIntraInit` -> `WelsMdIntraMb` chain driven by `WelsISliceMdEnc`, plus
//! `WelsMdInterInit`, `WelsMdInterMbRefinement` and `WelsMdFirstIntraMode`. The inter
//! half is split across `svc_mode_decision.rs` (`WelsMdP16x16`,
//! `WelsMdBackgroundMbEnc`, `WelsMdInterSecondaryModesEnc`) and `svc_encode_slice.rs`
//! (`WelsMdInterMbLoop`).
//!
//! The `sSampleDealingFuncs.pfIntra*Combined3` slots are never populated, so the
//! scalar branches below are the ones that decide output bytes.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
#![forbid(unsafe_code)]
use crate::common::copy_mb::{copy_8x8, copy_8x16, copy_16x8, copy_16x16};
use crate::common::mc::{mc_chroma, mc_luma};
use crate::encoder::encoder_context::{SMVUnitXY, sWelsEncCtx};
use crate::encoder::md::{
    BsSizeUE, FillNeighborCacheIntra, InitMeRefinePointer, MB_TYPE_8x8, MB_TYPE_8x16, MB_TYPE_16x8,
    MB_TYPE_16x16, MB_TYPE_INTRA4x4, MB_TYPE_INTRA16x16, MB_TYPE_SKIP, ME_REFINE_BUF_STRIDE_BLK8,
    ME_REFINE_BUF_WIDTH_BLK8, MdIntraAnalysisVaaInfo, MeRefineFracPixel, PredictSad, SMB, SMbCache,
    SMeRefinePointer, SWelsMD,
};
use crate::encoder::md::{LEFT_MB_POS, TOP_MB_POS, TOPLEFT_MB_POS, TOPRIGHT_MB_POS};
use crate::encoder::md::{MB_BLOCK8x8_NUM, MbSideInfo, MdSliceCtx, g_kiMapModeIntraChroma};
use crate::encoder::md::{mem_pred_chroma_off, mem_pred_luma_off};
use crate::encoder::rec_view::{RecCursor, RecPicView, copy_block_to_view};
use crate::encoder::svc_encode_mb::WelsEncRecI16x16Y;
use crate::encoder::svc_encode_mb::{WelsDctMb, WelsEncRecI4x4Y, WelsTryPUVskip, WelsTryPYskip};
use crate::encoder::svc_encode_slice::{SDqLayer, SSlice};
use crate::encoder::svc_encode_slice::{
    WelsIMbChromaEncode, WelsPMbChromaEncode, current_layer_ref, layer_enc_view_expect,
    layer_ref_feature_storage,
};
use crate::encoder::svc_encode_slice::{
    current_layer_expect, layer_rec_view_expect, layer_ref_view_expect,
};
use crate::encoder::svc_mode_decision::{
    BLOCK_4x4, BLOCK_8x8, BLOCK_8x16, BLOCK_16x8, BLOCK_16x16, IS_SKIP, InitMe, PredInter8x16Mv,
    PredInter16x8Mv, PredMv, PredSkipMv, REF_NOT_AVAIL, SUB_MB_TYPE_8x8, UpdateP8x8MotionInfo,
    UpdateP8x16Motion2Cache, UpdateP16x8Motion2Cache, UpdateP16x8MotionInfo,
    UpdateP16x16MotionInfo, WelsMdInterDecidedPskip, WelsMdInterJudgePskip,
    WelsMdInterSecondaryModesEnc, WelsMdIntraSecondaryModesEnc, g_kuiMbCountScan4Idx,
    update_P8x16_motion_info,
};
use crate::encoder::svc_mode_decision::{WelsInterMbEncode, WelsMdI16x16FromLayer, WelsMdP8x8};
use crate::encoder::svc_motion_estimate::SetMvWithinIntegerMvRange;
use crate::encoder::svc_set_mb_syn_cavlc::{IS_INTRA16x16, g_kuiCache48CountScan4Idx};
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
use crate::safe::mb_grid::MbSplit;
use crate::safe::plane::PlaneCursorMut;
use crate::simd::kernels;

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
// Tables
// ============================================================================

/// Number of I4x4 modes available per availability code.
pub const g_kiIntra4AvailCount: [u8; 16] = [1, 3, 2, 4, 1, 3, 2, 7, 1, 3, 4, 6, 1, 3, 4, 9];

/// The I4x4 modes available per availability code, indexed by
/// `left_avail | (top_avail<<1) | (left_top_avail<<2) | (right_top_avail<<3)`.
pub const g_kiIntra4AvailMode: [[i8; 16]; 16] = [
    // 0000
    [
        I4_PRED_DC_128,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0001
    [
        I4_PRED_DC_L,
        I4_PRED_H,
        I4_PRED_HU,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0010
    [
        I4_PRED_DC_T,
        I4_PRED_V,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0011
    [
        I4_PRED_DC,
        I4_PRED_H,
        I4_PRED_V,
        I4_PRED_HU,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0100
    [
        I4_PRED_DC_128,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0101
    [
        I4_PRED_DC_L,
        I4_PRED_H,
        I4_PRED_HU,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0110
    [
        I4_PRED_DC_T,
        I4_PRED_V,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 0111
    [
        I4_PRED_DC,
        I4_PRED_H,
        I4_PRED_V,
        I4_PRED_HU,
        I4_PRED_DDR,
        I4_PRED_VR,
        I4_PRED_HD,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1000
    [
        I4_PRED_DC_128,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1001
    [
        I4_PRED_DC_L,
        I4_PRED_H,
        I4_PRED_HU,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1010
    [
        I4_PRED_DC_T,
        I4_PRED_V,
        I4_PRED_DDL,
        I4_PRED_VL,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1011
    [
        I4_PRED_DC,
        I4_PRED_H,
        I4_PRED_V,
        I4_PRED_HU,
        I4_PRED_DDL,
        I4_PRED_VL,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1100
    [
        I4_PRED_DC_128,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1101
    [
        I4_PRED_DC_L,
        I4_PRED_H,
        I4_PRED_HU,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1110
    [
        I4_PRED_DC_T,
        I4_PRED_V,
        I4_PRED_DDL,
        I4_PRED_VL,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
    // 1111
    [
        I4_PRED_DC,
        I4_PRED_H,
        I4_PRED_V,
        I4_PRED_HU,
        I4_PRED_DDL,
        I4_PRED_VL,
        I4_PRED_DDR,
        I4_PRED_VR,
        I4_PRED_HD,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
        I4_PRED_INVALID,
    ],
];

/// The chroma prediction modes available per availability code, with the count last.
pub const g_kiIntraChromaAvailMode: [[i8; 5]; 8] = [
    [
        C_PRED_DC_128,
        C_PRED_INVALID,
        C_PRED_INVALID,
        C_PRED_INVALID,
        1,
    ],
    [C_PRED_DC_L, C_PRED_H, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_DC_T, C_PRED_V, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_V, C_PRED_H, C_PRED_DC, C_PRED_INVALID, 3],
    [
        C_PRED_DC_128,
        C_PRED_INVALID,
        C_PRED_INVALID,
        C_PRED_INVALID,
        1,
    ],
    [C_PRED_DC_L, C_PRED_H, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_DC_T, C_PRED_V, C_PRED_INVALID, C_PRED_INVALID, 2],
    [C_PRED_V, C_PRED_H, C_PRED_DC, C_PRED_P, 4],
];

/// X offset in the macroblock of each 4x4 block, in raster-of-8x8 scan order.
pub const g_kiCoordinateIdx4x4X: [i8; 16] = [0, 4, 0, 4, 8, 12, 8, 12, 0, 4, 0, 4, 8, 12, 8, 12];

/// Y offset in the macroblock of each 4x4 block, in raster-of-8x8 scan order.
pub const g_kiCoordinateIdx4x4Y: [i8; 16] = [0, 0, 4, 4, 0, 0, 4, 4, 8, 8, 12, 12, 8, 8, 12, 12];

/// Maps `uiNeighborIntra` and the 4x4 block index to the availability code that indexes
/// `g_kiIntra4AvailCount` / `g_kiIntra4AvailMode`.
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

/// Folds the restricted I4x4 mode ids (`DC_L`, `DC_T`, `DC_128`, `DDL_TOP`, `VL_TOP`)
/// back onto the nine coded ones.
pub const g_kiMapModeI4x4: [i8; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 2, 2, 2, 3, 7];

// ============================================================================
// Helpers
// ============================================================================

/// Predicts an I4x4 mode from the left and top neighbours' modes.
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

/// Re-points the cached per-macroblock plane pointers and reloads the intra neighbour
/// cache. Called once per macroblock by `WelsISliceMdEnc` before the re-encoding loop,
/// so it must not depend on the QP.
pub fn WelsMdIntraInit(mbs: &mut MbSplit<'_, SMB>, pMbCache: &mut SMbCache) {
    let cur = mbs.cur();
    let kiMbX = cur.iMbX as i32;
    let kiMbY = cur.iMbY as i32;

    pMbCache.SPicData.iMbX = kiMbX;
    pMbCache.SPicData.iMbY = kiMbY;

    mbs.cur_mut().uiCbp = 0;

    FillNeighborCacheIntra(pMbCache, mbs);
    // Re-init: `WelsMdI16x16` and `svc_md_i16x16_sad` may change this. Luma is the
    // first 256-byte half of `sMemPredMb` and chroma the second.
    pMbCache.uiMemPredLumaHalf = 0;
}

/// The full 16-mode-per-block I4x4 search, used on the non-`LOW_COMPLEXITY` path via
/// [`WelsMdIntraFinePartition`].
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
    let view = layer_rec_view_expect(pCurDqLayer);

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[pMbCache.uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pEncPicture = layer_enc_view_expect(pCurDqLayer);
    let (kiMbOrgX, kiMbOrgY) = pMbCache.SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let pCurDec = view.plane(0).cursor(
            kiMbOrgX + iCoordinateX as isize,
            kiMbOrgY + iCoordinateY as isize,
        );

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
                let cEnc = pEncPicture.plane(0).cursor(
                    kiMbOrgX + iCoordinateX as isize,
                    kiMbOrgY + iCoordinateY as isize,
                );
                (pFunc.sSampleDealingFuncs.pfSampleSatd[BLOCK_4x4].unwrap())(&cPred, &cEnc)
            } + lambda
                [(iPredMode == g_kiMapModeI4x4[iCurMode as usize] as i32) as usize];

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
            pMbCache.iRemIntra4x4PredModeFlag[i] = (if iFinalMode < iPredMode {
                iFinalMode
            } else {
                iFinalMode - 1
            }) as i8;
        }
        pMbCache.iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode as i8;

        //step 6: encoding I_4x4
        WelsEncRecI4x4Y(pEncCtx, pCurMb, pMbCache, i as u8);
    }

    StoreIntra4x4PredModeToMb(pCurMb, pMbCache);
    iCosti4x4 += (iLambda << 4) + (iLambda << 3); //4*6*lambda from JVT SATD0
    iCosti4x4
}

/// Publishes the four right-column and three bottom-row I4x4 prediction modes into the
/// macroblock so the next macroblock's `FillNeighborCacheIntra` can read them. Shared by
/// [`WelsMdI4x4`] and [`WelsMdI4x4Fast`].
#[inline]
fn StoreIntra4x4PredModeToMb(pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    let pMbMode = &mut pCurMb.iIntra4x4PredMode;
    let pCacheMode = &pMbCache.iIntraPredMode;
    pMbMode[0..4].copy_from_slice(&pCacheMode[33..37]);
    pCurMb.iIntra4x4PredMode[4] = pMbCache.iIntraPredMode[12];
    pCurMb.iIntra4x4PredMode[5] = pMbCache.iIntraPredMode[20];
    pCurMb.iIntra4x4PredMode[6] = pMbCache.iIntraPredMode[28];
}

/// The `LOW_COMPLEXITY` I4x4 search: instead of scoring every available mode it scores
/// DC/H/V, then follows whichever of the vertical or horizontal families won into at
/// most four more modes.
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
    let view = layer_rec_view_expect(pCurDqLayer);

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[pMbCache.uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pfMdCost4x4 = pFunc.sSampleDealingFuncs.md_cost(BLOCK_4x4).unwrap();
    let pEncPicture = layer_enc_view_expect(pCurDqLayer);
    let (kiMbOrgX, kiMbOrgY) = pMbCache.SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        let pCurDec = view.plane(0).cursor(
            kiMbOrgX + iCoordinateX as isize,
            kiMbOrgY + iCoordinateY as isize,
        );

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

        // Predicts `mode` into `dst_off` of `sMemPredBlk4` and returns its SATD plus
        // `lambda[iPredMode == g_kiMapModeI4x4[mode]]`.
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
                    &pEncPicture.plane(0).cursor(
                        kiMbOrgX + iCoordinateX as isize,
                        kiMbOrgY + iCoordinateY as isize,
                    ),
                ) + lambda[(iPredMode == g_kiMapModeI4x4[m as usize]) as usize]
            }};
        }
        macro_rules! alt_buf {
            () => {
                ((1 - iBestPredBufferNum) << 4) as usize
            };
        }
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
            pMbCache.iRemIntra4x4PredModeFlag[i] = if iFinalMode < iPredMode {
                iFinalMode
            } else {
                iFinalMode - 1
            };
        }
        pMbCache.iIntraPredMode[g_kuiCache48CountScan4Idx[i] as usize] = iFinalMode;
        //step 6: encoding I_4x4
        WelsEncRecI4x4Y(pEncCtx, pCurMb, pMbCache, i as u8);
    }

    StoreIntra4x4PredModeToMb(pCurMb, pMbCache);
    iCosti4x4 += (iLambda << 4) + (iLambda << 3); //4*6*lambda from JVT SATD0
    iCosti4x4
}

/// The 8x8 chroma predictor for `mode`, dispatched per mode rather than through
/// `pfGetChromaPred`: `DC`, `H`, `V` and `P` have SIMD kernels, the two one-sided DC
/// variants and the 128 fallback do not.
#[inline(always)]
fn ChromaPred(iMode: i32, pPred: &mut [u8; 64], cRec: &RecCursor<'_>) {
    use crate::encoder::get_intra_predictor as gip;
    match iMode as i8 {
        C_PRED_DC => kernels::intra_pred::enc_chroma_pred_dc(pPred, cRec),
        C_PRED_H => kernels::intra_pred::enc_chroma_pred_h(pPred, cRec),
        C_PRED_V => kernels::intra_pred::enc_chroma_pred_v(pPred, cRec),
        C_PRED_P => kernels::intra_pred::enc_chroma_pred_plane(pPred, cRec),
        C_PRED_DC_L => gip::WelsIChromaPredDcLeft_c(pPred, cRec),
        C_PRED_DC_T => gip::WelsIChromaPredDcTop_c(pPred, cRec),
        C_PRED_DC_128 => gip::WelsIChromaPredDcNA_c(pPred, cRec),
        _ => panic!("chroma prediction mode {iMode} is not one of the seven"),
    }
}

/// Picks the 8x8 chroma prediction mode over Cb and Cr jointly and leaves the winning
/// prediction in `pBestPredIntraChroma`.
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

    let pfMdCost8x8 = pFunc
        .sSampleDealingFuncs
        .md_cost(BLOCK_8x8)
        .expect("pfMdCost selects an installed 8x8 slot");
    let pEncPicture = layer_enc_view_expect(pCurDqLayer);
    let (kiChrOrgX, kiChrOrgY) = pMbCache.SPicData.chroma_origin();
    let kiPredOff = mem_pred_chroma_off(pMbCache.uiMemPredLumaHalf);

    let mut iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount as usize {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        // The chroma half's `iChmaIdx` 128-byte side of `sMemPredMb`; the Cr block
        // sits 64 bytes beyond the Cb one.
        let kiDstOff = kiPredOff + 128 * iChmaIdx;
        ChromaPred(
            iCurMode,
            (&mut pMbCache.sMemPredMb[kiDstOff..kiDstOff + 64])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cb
        let mut iCurCost = pfMdCost8x8(
            &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiDstOff..][..64], 0, 8),
            &pEncPicture.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        );

        ChromaPred(
            iCurMode,
            (&mut pMbCache.sMemPredMb[kiDstOff + 64..kiDstOff + 128])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cr
        iCurCost += pfMdCost8x8(
            &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiDstOff + 64..][..64], 0, 8),
            &pEncPicture.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ) + iLambda * BsSizeUE(g_kiMapModeIntraChroma[iCurMode as usize] as u32) as i32;
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

/// The non-`LOW_COMPLEXITY` `pfIntraFineMd`.
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

/// The `LOW_COMPLEXITY` `pfIntraFineMd`. Skips the I4x4 search entirely for macroblocks
/// whose intra variance is below `INTRA_VARIANCE_SAD_THRESHOLD`.
pub fn WelsMdIntraFinePartitionVaa(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pCurLayer = current_layer_expect(pEncCtx);
    let encView = layer_enc_view_expect(pCurLayer);
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

/// The whole intra mode decision for one macroblock: score I16x16, then let
/// `WelsMdIntraSecondaryModesEnc` try I4x4 and chroma and reconstruct whichever won.
///
/// [`WelsMdIntraInit`] must have run for this macroblock.
pub fn WelsMdIntraMb(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    //initial prediction memory for I_16x16
    pWelsMd.iCostLuma = WelsMdI16x16FromLayer(
        pEncCtx.func_list(),
        current_layer_ref(pEncCtx),
        pMbCache,
        pWelsMd.iLambda,
    );
    pCurMb.uiMbType = MB_TYPE_INTRA16x16;

    WelsMdIntraSecondaryModesEnc(pEncCtx, pWelsMd, pCurMb, pMbCache);
}

// ============================================================================
// Inter (P-slice) mode decision
// ============================================================================

/// Byte offset of each 4x4 block inside a 16x16 prediction buffer, in
/// raster-scan-of-4x4 order.
pub const g_kuiSmb4AddrIn256: [u8; 16] = [
    0,
    4,
    16 * 4,
    16 * 4 + 4,
    8,
    12,
    16 * 4 + 8,
    16 * 4 + 12,
    16 * 8,
    16 * 8 + 4,
    16 * 12,
    16 * 12 + 4,
    16 * 8 + 8,
    16 * 8 + 12,
    16 * 12 + 8,
    16 * 12 + 12,
];

/// Byte offset of each 8x8 block inside a motion-refinement buffer.
pub const g_kiPixStrideIdx8x8: [i32; 4] = [
    0,
    ME_REFINE_BUF_WIDTH_BLK8,
    ME_REFINE_BUF_STRIDE_BLK8,
    ME_REFINE_BUF_STRIDE_BLK8 + ME_REFINE_BUF_WIDTH_BLK8,
];

/// Per-macroblock inter setup: neighbour cache, the reference-plane pointers, and the
/// integer MV clamp for this macroblock position.
pub fn WelsMdInterInit(
    sc: &MdSliceCtx<'_>,
    mbi: &MbSideInfo,
    iMvRange: i32,
    pSlice: &mut SSlice,
    mbs: &mut MbSplit<'_, SMB>,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let cur = mbs.cur();
    let kiMbX = cur.iMbX as i32;
    let kiMbY = cur.iMbY as i32;
    let kiMbXY = cur.iMbXY;

    (sc.func.pfFillInterNeighborCache)(
        &mut *pMbCache,
        &*mbs,
        &sc.vaa.pVaaBackgroundMbFlag[..],
        sc.rec.mb_skip_sad(),
    );

    pMbCache.SPicData.iMbX = kiMbX;
    pMbCache.SPicData.iMbY = kiMbY;

    // `uiRefMbType[iMbXY]` of the layer's reference picture.
    pMbCache.uiRefMbType = mbi.ref_mb_type;
    pMbCache.bCollocatedPredFlag = false;

    // Mode decision may skip both `WelsMdP16x16` and `WelsMdPSkip`, so the MV is zeroed
    // here rather than left over from the previous macroblock.
    mbs.cur_mut().sP16x16Mv = SMVUnitXY { iMvX: 0, iMvY: 0 };
    sc.rec
        .mv_list()
        .set(kiMbXY as usize, SMVUnitXY { iMvX: 0, iMvY: 0 });

    SetMvWithinIntegerMvRange(
        sc.mb_width,
        sc.mb_height,
        kiMbX,
        kiMbY,
        iMvRange,
        &mut pSlice.sMvStartMin,
        &mut pSlice.sMvStartMax,
    );
}

/// Scores the two 16x8 partitions.
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
            layer_ref_feature_storage(pEncCtx, pCurDqLayer),
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
            let pRefPicture = layer_ref_view_expect(pEncCtx, pCurDqLayer);
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

/// Scores the two 8x16 partitions.
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
            layer_ref_feature_storage(pEncCtx, pCurLayer),
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
            let pRefPicture = layer_ref_view_expect(pEncCtx, pCurLayer);
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

/// The non-VAA (`!LOW_COMPLEXITY`) fine partition search.
pub fn WelsMdInterFinePartition<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    iBestCost: i32,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let mut iCost = WelsMdP8x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);

    if iCost < iBestCost {
        pCurMb.uiMbType = MB_TYPE_8x8;
        pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];

        let mut iCostPart = WelsMdP16x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            iCost = iCostPart;
            pCurMb.uiMbType = MB_TYPE_16x8;
        }

        iCostPart = WelsMdP8x16(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            pCurMb.uiMbType = MB_TYPE_8x16;
        }
    }
}

/// The VAA-guided fine partition search, the `LOW_COMPLEXITY` path.
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
        &pEncCtx.vaa_expect().sVaaCalcInfo.pSad8x8[pCurMb.iMbXY as usize],
    );

    if crate::encoder::dump_enabled(&FP_DUMP, "OH264_FPDUMP") {
        let sad = (&pEncCtx.vaa_expect().sVaaCalcInfo.pSad8x8)[pCurMb.iMbXY as usize];
        eprintln!(
            "FP mb={:3} sign={:2} best={:7} sad8x8={},{},{},{}",
            pCurMb.iMbXY, uiMbSign, iBestCost, sad[0], sad[1], sad[2], sad[3]
        );
    }

    if uiMbSign == 15 {
        return;
    }

    match uiMbSign {
        3 | 12 => {
            let iCostP16x8 =
                WelsMdP16x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
            if iCostP16x8 < iBestCost {
                iBestCost = iCostP16x8;
                pCurMb.uiMbType = MB_TYPE_16x8;
            }
        }
        5 | 10 => {
            let iCostP8x16 =
                WelsMdP8x16(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x16 < iBestCost {
                iBestCost = iCostP8x16;
                pCurMb.uiMbType = MB_TYPE_8x16;
            }
        }
        6 | 9 => {
            let iCostP8x8 = WelsMdP8x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                pCurMb.uiMbType = MB_TYPE_8x8;
                pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];
            }
        }
        _ => {
            let iCostP8x8 = WelsMdP8x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                pCurMb.uiMbType = MB_TYPE_8x8;
                pCurMb.uiSubMbType = [SUB_MB_TYPE_8x8; 4];

                let iCostP16x8 =
                    WelsMdP16x8(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
                if iCostP16x8 <= iBestCost {
                    iBestCost = iCostP16x8;
                    pCurMb.uiMbType = MB_TYPE_16x8;
                }

                let iCostP8x16 =
                    WelsMdP8x16(pEncCtx, pEncCtx.func_list(), pCurDqLayer, pWelsMd, pSlice);
                if iCostP8x16 <= iBestCost {
                    iBestCost = iCostP8x16;
                    pCurMb.uiMbType = MB_TYPE_8x16;
                }
            }
        }
    }
    pWelsMd.iCostLuma = iBestCost;
}

/// Motion-compensates the P_SKIP predictor and decides whether the macroblock can be
/// coded as P_SKIP.
pub fn WelsMdPSkipEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let sc = *pWelsMd.sc();
    let mbi = pWelsMd.mbi;
    let (cEncLuma, cEncCb, cEncCr) = {
        let mbc = pWelsMd.mbc();
        (mbc.enc_y, mbc.enc_cb, mbc.enc_cr)
    };
    let pFunc = sc.func;

    let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
    let mut n: i32;

    let mut pEncMb = cEncLuma;
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
        iMvX: (sMvp.iMvX >> 2),
        iMvY: (sMvp.iMvY >> 2),
    };
    n = ((pCurMb.iMbX as i32) << 4) + sQpelMvp.iMvX as i32;
    if n < -29 {
        return false;
    } else if n > ((sc.mb_width << 4) + 12) {
        return false;
    }

    n = ((pCurMb.iMbY as i32) << 4) + sQpelMvp.iMvY as i32;
    if n < -29 {
        return false;
    } else if n > ((sc.mb_height << 4) + 12) {
        return false;
    }

    let kiMbXLuma = (pCurMb.iMbX as isize) << 4;
    let kiMbYLuma = (pCurMb.iMbY as isize) << 4;
    let kiMbXChroma = (pCurMb.iMbX as isize) << 3;
    let kiMbYChroma = (pCurMb.iMbY as isize) << 3;

    // The reference cursors below are at the motion-compensated position, not the
    // macroblock's own.
    let pRefPicture = sc
        .refv
        .expect("the layer's reference view is built for this frame");

    //luma
    {
        let cRefLuma = pRefPicture.plane(0).cursor(
            kiMbXLuma + sQpelMvp.iMvX as isize,
            kiMbYLuma + sQpelMvp.iMvY as isize,
        );
        let mut cDstLuma = PlaneCursorMut::new(&mut pMbCache.sSkipMb[..256], 0, 16);
        mc_luma(&cRefLuma, &mut cDstLuma, sMvp.iMvX, sMvp.iMvY, 16, 16);
    }
    iSadCostLuma = {
        let cSkipLuma = RecCursor::over_owned(&mut pMbCache.sSkipMb[..256], 0, 16);
        (sc.sad16)(&cEncLuma, &cSkipLuma)
    };

    // Chroma offsets are `(mvX >> 1, mvY >> 1)` in samples from the chroma macroblock
    // origin; `sQpelMvp` is already `sMvp >> 2`, so both are `sMvp >> 3`.
    {
        let cRefCb = pRefPicture.plane(1).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCb = PlaneCursorMut::new(&mut pMbCache.sSkipMb[256..320], 0, 8);
        mc_chroma(&cRefCb, &mut cDstCb, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cb
    }
    iSadCostChroma = {
        let cSkipCb = RecCursor::over_owned(&mut pMbCache.sSkipMb[256..320], 0, 8);
        kernels::sad::sample_sad_8x8(&cEncCb, &cSkipCb)
    };

    {
        let cRefCr = pRefPicture.plane(2).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCr = PlaneCursorMut::new(&mut pMbCache.sSkipMb[320..384], 0, 8);
        mc_chroma(&cRefCr, &mut cDstCr, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cr
    }
    iSadCostChroma += {
        let cSkipCr = RecCursor::over_owned(&mut pMbCache.sSkipMb[320..384], 0, 8);
        kernels::sad::sample_sad_8x8(&cEncCr, &cSkipCr)
    };

    iSadCostMb = iSadCostLuma + iSadCostChroma;

    if iSadCostMb == 0
        || iSadCostMb < pWelsMd.iSadPredSkip
        || (mbi.ref_is_p && pMbCache.uiRefMbType == MB_TYPE_SKIP && iSadCostMb < mbi.ref_skip_sad)
    {
        //update motion info to current MB
        AcceptPskip(pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
        return true;
    }

    let pDstLuma = RecCursor::over_owned(&mut pMbCache.sSkipMb, 0, 16);
    WelsDctMb(
        &mut pMbCache.sCoeffLevel,
        &pEncMb,
        &pDstLuma,
        pFunc.pfDctFourT4,
    );

    if WelsTryPYskip(pEncCtx, pCurMb, pMbCache) {
        pEncMb = cEncCb;

        let pDstCb = RecCursor::over_owned(&mut pMbCache.sSkipMb, 256, 8);
        (pFunc.pfDctFourT4)(
            &mut pMbCache.sCoeffLevel[256..],
            &pEncMb.advance(kpEncBlockOffset[16] as isize, 0),
            &pDstCb,
        );
        if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 1) {
            pEncMb = cEncCr;

            let pDstCr = RecCursor::over_owned(&mut pMbCache.sSkipMb, 320, 8);
            (pFunc.pfDctFourT4)(
                &mut pMbCache.sCoeffLevel[320..],
                &pEncMb.advance(kpEncBlockOffset[20] as isize, 0),
                &pDstCr,
            );
            if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 2) {
                //update motion info to current MB
                AcceptPskip(pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
                return true;
            }
        }
    }
    false
}

/// Commits the P_SKIP decision: zero reference indices, the skip MV, and the luma cost.
#[inline]
fn AcceptPskip(
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    sMvp: &SMVUnitXY,
    iSadCostLuma: i32,
    iSadCostMb: i32,
) {
    let sc = *pWelsMd.sc();
    let cEncLuma = pWelsMd.mbc().enc_y;

    pCurMb.iRefIndex = [0; MB_BLOCK8x8_NUM];
    (sc.func.pfUpdateMbMv)(&mut pCurMb.sMv, *sMvp);

    if pWelsMd.bMdUsingSad {
        pCurMb.iSadCost = iSadCostLuma;
        pWelsMd.iCostLuma = pCurMb.iSadCost;
    } else {
        let cSkipLuma = RecCursor::over_owned(&mut pMbCache.sSkipMb[..256], 0, 16);
        pWelsMd.iCostLuma = (sc.satd16)(&cEncLuma, &cSkipLuma);
    }

    pWelsMd.iCostSkipMb = iSadCostMb;

    pCurMb.sP16x16Mv = *sMvp;
    sc.rec.mv_list().set(pCurMb.iMbXY as usize, *sMvp);
}

/// Quarter-pel refinement of whichever partitioning the integer search chose, plus the
/// chroma motion compensation for each partition.
pub fn WelsMdInterMbRefinement(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let pCurDqLayer = current_layer_expect(pEncCtx);
    let pFunc = pEncCtx.func_list();
    let pRefPicture = layer_ref_view_expect(pEncCtx, pCurDqLayer);
    let pEncPicture = layer_enc_view_expect(pCurDqLayer);
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
    /// `($dx, $dy)` is the partition's own chroma offset plus the motion vector's
    /// integer chroma part, in samples from the macroblock's chroma origin; `$off` is
    /// the destination's byte offset inside `sMemPredMb`, whose prediction rows are 8
    /// samples apart. The destination slice spans exactly `($h - 1) * 8 + $w` bytes.
    macro_rules! mc_chroma_at {
        ($plane:expr, $off:expr, $dx:expr, $dy:expr, $mv:expr, $w:expr, $h:expr) => {{
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

            let cEncLuma = pEncPicture
                .plane(0)
                .cursor(kiMbXChroma << 1, kiMbYChroma << 1);
            let cEncCb = pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
            let cEncCr = pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
            pWelsMd.iCostSkipMb = (pFunc.sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].unwrap())(
                &cEncLuma,
                &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiOffLuma..][..256], 0, 16),
            );
            pWelsMd.iCostSkipMb += (pFunc.sSampleDealingFuncs.pfSampleSad[BLOCK_8x8].unwrap())(
                &cEncCb,
                &RecCursor::over_owned(&mut pMbCache.sMemPredMb[kiOffCb..][..64], 0, 8),
            );
            pWelsMd.iCostSkipMb += (pFunc.sSampleDealingFuncs.pfSampleSad[BLOCK_8x8].unwrap())(
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
                // The partition sits `4 * i` chroma rows down, in column 0.
                let iBlk4Y = (i as i32) << 2;
                let sMv = pWelsMd.sMe.sMe16x8[i].sMv;
                let dx = sMv.iMvX as i32 >> 3;
                let dy = iBlk4Y + (sMv.iMvY as i32 >> 3);
                let iDstOff = i << 5; // 4 rows x 8
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
                // The partition sits in chroma column `4 * i`, in row 0.
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
                        // Coordinate `(iBlk4X, iBlk4Y)` at stride 8.
                        let iDstOff = ((iBlk4Y << 3) + iBlk4X) as usize;
                        mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 4, 4); //Cb
                        mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 4, 4); //Cr
                    }
                    // Every writer of `uiSubMbType` sets `SUB_MB_TYPE_8x8`; no
                    // sub-8x8 partitioning is searched.
                    _ => unreachable!(
                        "sub-8x8 partition {:#x} — the sub-8x8 search is #if 0 upstream \
                         and unwritten here",
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

/// Costs I16x16 against the current inter cost and, if intra wins, runs the whole intra
/// encode for this macroblock.
pub fn WelsMdFirstIntraMode(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let sc = *pWelsMd.sc();
    let pFunc = sc.func;

    let (cRecLuma, cEncLuma) = {
        let mbc = pWelsMd.mbc();
        (mbc.rec_y, mbc.enc_y)
    };
    let iCostI16x16 = crate::encoder::svc_mode_decision::WelsMdI16x16(
        sc.md_cost16,
        &cRecLuma,
        &cEncLuma,
        pMbCache,
        pWelsMd.iLambda,
    );

    //compare cost_p16x16 with cost_i16x16
    if iCostI16x16 < pWelsMd.iCostLuma {
        pCurMb.uiMbType = MB_TYPE_INTRA16x16;
        pWelsMd.iCostLuma = iCostI16x16;

        pFunc.pfIntraFineMd.expect("pfIntraFineMd unset")(pEncCtx, pWelsMd, pCurMb, pMbCache);

        if IS_INTRA16x16(pCurMb.uiMbType) {
            pCurMb.uiCbp = 0;
            WelsEncRecI16x16Y(pEncCtx, pCurMb, pMbCache);
        }

        //chroma
        pWelsMd.iCostChroma = WelsMdIntraChroma(pFunc, sc.layer, pMbCache, pWelsMd.iLambda);
        WelsIMbChromaEncode(pEncCtx, pCurMb, pMbCache);
        pCurMb.uiChromPredMode = pMbCache.uiChmaI8x8Mode as u32;
        pCurMb.iSadCost = 0;
        return true; //intra_mb_type is best
    }

    false
}

/// The P-slice per-macroblock entry point, installed as `pfInterMd` by
/// `WelsCodePSlice`.
pub fn WelsMdInterMb<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    mbs: &mut MbSplit<'_, SMB>,
) {
    let sc = *pWelsMd.sc();
    let pCurDqLayer = sc.layer;
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
    if (sc.func.pfInterMdBackgroundDecision)(
        pEncCtx,
        pWelsMd,
        pSlice,
        mbs.cur_mut(),
        &mut bKeepSkip,
    ) {
        return;
    }

    //try static or scrolled Pskip
    if (sc.func.pfSCDPSkipDecision)(pEncCtx, pWelsMd, pSlice, mbs.cur_mut()) {
        return;
    }

    //step 1: try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bTrySkip);

    if bSkip {
        if bKeepSkip {
            WelsMdInterDecidedPskip(pWelsMd, pSlice, mbs.cur_mut());
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
            sc.func,
            pCurDqLayer,
            pWelsMd,
            pSlice,
            mbs,
        );
        mbs.cur_mut().uiMbType = MB_TYPE_16x16;
    }

    WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bSkip);
}

/// Re-classifies a zero-CBP 16x16 as P_SKIP when its MV equals the skip predictor.
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

/// The 32-bit word an `SMVUnitXY` occupies, for comparing two MVs in one operation.
#[inline]
fn LD32_MV_PUB(pMv: &SMVUnitXY) -> u32 {
    let x = pMv.iMvX.to_ne_bytes();
    let y = pMv.iMvY.to_ne_bytes();
    u32::from_ne_bytes([x[0], x[1], y[0], y[1]])
}

/// Transforms, quantises and reconstructs the chosen inter macroblock, then copies the
/// prediction into the CS planes.
pub fn WelsMdInterEncode(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, pCurMb: &mut SMB) {
    let pCurDqLayer = current_layer_expect(pEncCtx);

    pCurMb.uiCbp = 0;
    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(pEncCtx, pSlice, pCurMb);

    let view = layer_rec_view_expect(pCurDqLayer);
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let (lx, ly) = pMbCache.SPicData.luma_origin();
    let (cx, cy) = pMbCache.SPicData.chroma_origin();
    let kiLumaOff = mem_pred_luma_off(pMbCache.uiMemPredLumaHalf);
    let kiChromaOff = mem_pred_chroma_off(pMbCache.uiMemPredLumaHalf);
    let src = &pMbCache.sMemPredMb;
    copy_block_to_view::<16, 16>(
        &src[kiLumaOff..kiLumaOff + 256],
        &view.plane(0).cursor(lx, ly),
    );
    copy_block_to_view::<8, 8>(
        &src[kiChromaOff..kiChromaOff + 64],
        &view.plane(1).cursor(cx, cy),
    );
    copy_block_to_view::<8, 8>(
        &src[kiChromaOff + 64..kiChromaOff + 128],
        &view.plane(2).cursor(cx, cy),
    );
}

/// Records the skip SAD and the coded macroblock type for the next frame's predictors.
///
/// Both arrays must have room for `pCurMb->iMbXY`.
pub fn WelsMdInterSaveSadAndRefMbType(pRecView: &RecPicView, pCurMb: &SMB, pMd: &SWelsMD<'_>) {
    let kmtCurMbtype = pCurMb.uiMbType;
    let kiMbXY = pCurMb.iMbXY as usize;

    //sad
    pRecView.mb_skip_sad().set(
        kiMbXY,
        if kmtCurMbtype == MB_TYPE_SKIP {
            pMd.iCostSkipMb
        } else {
            0
        },
    );
    //uiMbType
    pRecView.ref_mb_type().set(kiMbXY, kmtCurMbtype);
}

/// Cached gate for the debug dump; see `encoder::dump_enabled`.
static FP_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `g_kiIntra4AvailMode` row lists exactly `g_kiIntra4AvailCount` modes before
    /// its `I4_PRED_INVALID` padding; `WelsMdI4x4` walks the row by the count.
    #[test]
    fn intra4_avail_count_matches_mode_table() {
        // Rows whose count is 1 are the DC_128-only rows.
        for (idx, &count) in g_kiIntra4AvailCount.iter().enumerate() {
            assert!(count as usize <= 16, "row {idx} count out of range");
            if count == 1 {
                assert_eq!(g_kiIntra4AvailMode[idx][0], I4_PRED_DC_128, "row {idx}");
            }
        }
        // Rows 0000 (nothing available) and 1111 (interior) in full.
        assert_eq!(g_kiIntra4AvailCount[0], 1);
        assert_eq!(g_kiIntra4AvailCount[15], 9);
        assert_eq!(
            &g_kiIntra4AvailMode[15][..9],
            &[
                I4_PRED_DC,
                I4_PRED_H,
                I4_PRED_V,
                I4_PRED_HU,
                I4_PRED_DDL,
                I4_PRED_VL,
                I4_PRED_DDR,
                I4_PRED_VR,
                I4_PRED_HD
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
    /// smaller of the two mode ids otherwise.
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
