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

#![deny(unsafe_code)]
use crate::encoder::rec_view::RecCursor;
use crate::encoder::rec_view::copy_block_to_view;
use crate::encoder::svc_encode_slice::{
    layer_enc_view, layer_rec_view, layer_ref_pic, layer_ref_view,
    current_layer_ref,
};
use crate::encoder::svc_encode_slice::current_layer;
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
pub fn PredIntra4x4Mode(pIntraPredMode: &[i8; 48], iIdx4: i32) -> i32 {
    // S4.C3: was `*const i8` reached at `iIdx4 - 8` and `iIdx4 - 1`. The extent is
    // `SMbCache::iIntraPredMode`, an `[i8; 48]`, which is what both call sites and
    // the test already hand it. Indexing bounds-checks the two neighbour reads that
    // the raw form took on trust — `iIdx4` comes from `g_kuiCache48CountScan4Idx`,
    // whose smallest entry is 9, so `iIdx4 - 8` is in range for every real caller
    // and a future one that is not now panics instead of reading behind the array.
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
///
pub fn WelsMdIntraInit(
    pEncCtx: &sWelsEncCtx,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pMbCache: &mut SMbCache,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");

    let kiMbX = mbs.cur().iMbX as i32;
    let kiMbY = mbs.cur().iMbY as i32;
    let kiMbXY = mbs.cur().iMbXY;

    // step 3. locating current pEnc and pDec
    //
    // **S4.C2: the six cursors are gone and the branch with them.** This was an
    // `if 0 == kiMbX || iSliceFirstMbXY == kiMbXY` that computed
    // `pEncMb`/`pCsMb` absolutely, against an `else` that walked the previous
    // macroblock's by one macroblock width. The two arms were the same address:
    // the walk was taken exactly when the previous macroblock was this one's left
    // neighbour, so `previous + 16` equalled
    // `root + ((iMbX + iMbY * stride) << 4)`. Both arms also stamped the
    // coordinate pair — T9.B30 put it here for exactly this — so with the cursors
    // resolved at use (`enc_mb`/`cs_mb`) the whole construct collapses to the
    // stamp both arms shared.
    (*pMbCache).SPicData.iMbX = kiMbX;
    (*pMbCache).SPicData.iMbY = kiMbY;

    //step 2. initial pWelsMd
    mbs.cur_mut().uiCbp = 0;

    //step 4: locating scaled_tcoeff

    //step 1. load neighbor cache
    FillNeighborCacheIntra(pMbCache, mbs);
    // in WelsMdI16x16() will be changed, so re-init here!
    // Init with default, maybe change in WelsMdI16x16 and svc_md_i16x16_sad:
    // luma is the first 256-byte half of `sMemPredMb` and chroma the second.
    (*pMbCache).uiMemPredLumaHalf = 0;
}

/// `svc_base_layer_md.cpp:418`. The full 16-mode-per-block I4x4 search, used on the
/// non-`LOW_COMPLEXITY` path via [`WelsMdIntraFinePartition`].
///
/// # Safety
/// See [`WelsMdIntraInit`]. The I4x4 scratch (`sMemPredBlk4`) and both intra-mode
/// flag arrays are inline in `SMbCache` since T6.C3, so there is nothing left for a
/// caller to have allocated.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdI4x4(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pFunc = (*pEncCtx).func_list();
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let iLambda = (*pWelsMd).iLambda;
    let iBestCostLuma = (*pWelsMd).iCostLuma;
    // `pEncMb` and `kiLineSizeEnc` stood here — the source block as an address and
    // its stride. **T9.B30** reads the source through the layer at the carrier's
    // coordinates instead; `pDecMb` is the reconstruction plane and stays raw with
    // the intra predictors that read it (session C).
    // **T9.C2**: `pDecMb` — the reconstruction luma plane's raw root for this
    // macroblock — and `kiLineSizeDec`, the stride the predictors were handed, are
    // both the seam's plane view now. The view carries the stride, and the
    // per-block cursors below come from the macroblock origin by coordinate.
    let view = layer_rec_view(&*pCurDqLayer)
        .expect("the layer's reconstruction view is built for this frame");

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[(*pMbCache).uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    // `pfSampleSatd[BLOCK_4x4]`, a constant index — called direct (F118).
    let pEncPicture = layer_enc_view(&*pCurDqLayer).expect("the layer's source view is built for this frame");
    let (kiMbOrgX, kiMbOrgY) = (*pMbCache).SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        // **T9.C2**: `pDecMb.offset(iCoordinateY * kiLineSizeDec + iCoordinateX)` was
        // the reconstruction plane's raw cursor at this 4x4 block; the seam reaches
        // the same sample by coordinate, from the macroblock origin the carrier
        // already holds.
        let pCurDec = view
            .plane(0)
            .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            &(*pMbCache).iIntraPredMode,
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

            // **T9.C2 — the intra-pred read path.** The predictor's *destination*
            // was always the arena (`sMemPredBlk4`, packed 4x4 at stride 4) and its
            // *reference* was always the reconstruction plane, read and never
            // written. The slot is now safe over both: a `&mut [u8; 16]` into the
            // arena half, and the seam's read cursor at this 4x4 block's origin.
            //
            // Unlike the idct and copy families the slot is **flipped, not
            // bypassed** — F118's exemption is for fixed-size sites, and this one
            // indexes the table by a runtime mode, so the operand and the slot had
            // to move together.
            let kiDstOff = ((1 - iBestPredBufferNum) << 4) as usize;
            let pDst: &mut [u8; 16] = (&mut (*pMbCache).sMemPredBlk4[kiDstOff..kiDstOff + 16])
                .try_into()
                .expect("a packed 4x4 prediction block is 16 bytes");
            (*pFunc).pfGetLumaI4x4Pred[iCurMode as usize].unwrap()(pDst, &pCurDec);
            let iCurCost = {
                let cPred = RecCursor::over_owned(
                    &mut (*pMbCache).sMemPredBlk4[((1 - iBestPredBufferNum) << 4) as usize..][..16],
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

        (*pMbCache).uiBestPredI4x4Blk4Half = iBestPredBufferNum as u8;
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize] as i32;
        if iPredMode == iFinalMode {
            (*pMbCache).bPrevIntra4x4PredModeFlag[i] = true;
        } else {
            (*pMbCache).bPrevIntra4x4PredModeFlag[i] = false;
            (*pMbCache).iRemIntra4x4PredModeFlag[i] =
                (if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 }) as i8;
        }
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
fn StoreIntra4x4PredModeToMb(pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdI4x4Fast(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pFunc = (*pEncCtx).func_list();
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let iLambda = (*pWelsMd).iLambda;
    let iBestCostLuma = (*pWelsMd).iCostLuma;
    // `pEncMb` and `kiLineSizeEnc` stood here — the source block as an address and
    // its stride. **T9.B30** reads the source through the layer at the carrier's
    // coordinates instead; `pDecMb` is the reconstruction plane and stays raw with
    // the intra predictors that read it (session C).
    // **T9.C2**: `pDecMb` — the reconstruction luma plane's raw root for this
    // macroblock — and `kiLineSizeDec`, the stride the predictors were handed, are
    // both the seam's plane view now. The view carries the stride, and the
    // per-block cursors below come from the macroblock origin by coordinate.
    let view = layer_rec_view(&*pCurDqLayer)
        .expect("the layer's reconstruction view is built for this frame");

    let lambda: [i32; 2] = [iLambda << 2, iLambda];
    let kpNeighborIntraToI4x4 = &g_kiNeighborIntraToI4x4[(*pMbCache).uiNeighborIntra as usize];
    let mut iBestPredBufferNum: i32 = 0;
    let mut iCosti4x4: i32 = 0;

    let pfMdCost4x4 = (*pFunc).sSampleDealingFuncs.md_cost(BLOCK_4x4).unwrap();
    let pEncPicture = layer_enc_view(&*pCurDqLayer).expect("the layer's source view is built for this frame");
    let (kiMbOrgX, kiMbOrgY) = (*pMbCache).SPicData.luma_origin();

    for i in 0..16usize {
        let kiOffset = kpNeighborIntraToI4x4[i] as usize;

        //step 1: locating current 4x4 block position in pEnc and pDecMb
        let iCoordinateX = g_kiCoordinateIdx4x4X[i] as i32;
        let iCoordinateY = g_kiCoordinateIdx4x4Y[i] as i32;

        // **T9.C2**, as `WelsMdI4x4` above: the block's reconstruction cursor by
        // coordinate rather than by raw offset.
        let pCurDec = view
            .plane(0)
            .cursor(kiMbOrgX + iCoordinateX as isize, kiMbOrgY + iCoordinateY as isize);

        //step 2: get predicted mode from neighbor
        let iPredMode = PredIntra4x4Mode(
            &(*pMbCache).iIntraPredMode,
            g_kuiCache48CountScan4Idx[i] as i32,
        ) as i8;
        //step 3: collect candidates of iPredMode
        let iAvailCount = g_kiIntra4AvailCount[kiOffset] as i32;
        let kpAvailMode = &g_kiIntra4AvailMode[kiOffset];

        let mut iBestMode: i8;
        let mut iBestCost: i32;

        // `lambda[iPredMode == g_kiMapModeI4x4[iCurMode]]`, hoisted so the mode-scoring
        // below reads like the C++ one-liner it is translating.
        // **T9.B30**: the macro takes the prediction buffer's *offset* rather than a
        // pointer. `pfGetLumaI4x4Pred` is still a raw slot (intra prediction reads the
        // reconstruction plane — session C's), so the raw is derived inside, at that
        // call and nowhere else; the cost then takes a **shared** borrow of the same
        // field, which does not pop it the way a `&mut` would (F114a's boundary).
        macro_rules! score {
            ($mode:expr, $dst_off:expr) => {{
                let m: i8 = $mode;
                let off: usize = $dst_off;
                (*pFunc).pfGetLumaI4x4Pred[m as usize].unwrap()(
                    (&mut (*pMbCache).sMemPredBlk4[off..off + 16])
                        .try_into()
                        .expect("a packed 4x4 prediction block is 16 bytes"),
                    &pCurDec,
                );
                pfMdCost4x4(
                    &RecCursor::over_owned(&mut (*pMbCache).sMemPredBlk4[off..][..16], 0, 4),
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

        (*pMbCache).uiBestPredI4x4Blk4Half = iBestPredBufferNum as u8;
        iCosti4x4 += iBestCost;
        if iCosti4x4 >= iBestCostLuma {
            break;
        }

        //step 5: update pred mode and sample avail cache
        let iFinalMode = g_kiMapModeI4x4[iBestMode as usize];
        if iPredMode == iFinalMode {
            (*pMbCache).bPrevIntra4x4PredModeFlag[i] = true;
        } else {
            (*pMbCache).bPrevIntra4x4PredModeFlag[i] = false;
            (*pMbCache).iRemIntra4x4PredModeFlag[i] =
                if iFinalMode < iPredMode { iFinalMode } else { iFinalMode - 1 };
        }
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
pub extern "C" fn WelsMdIntraChroma(
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: &SDqLayer,
    pMbCache: &mut SMbCache,
    iLambda: i32,
) -> i32 {
    let mut iChmaIdx: usize = 0;
    // **T9.C2**: `pPredIntraChma` / `pDstChma` were a two-pointer ping-pong over the
    // chroma half's two 128-byte sides, carrying exactly the bit `iChmaIdx` already
    // holds — the same shape `WelsMdI16x16`'s `pPredI16x16` had. With the
    // destination an offset (`kiDstOff` below), the pointers carry nothing.
    // `pEncCb`/`pEncCr` stood here; T9.B30's cost sites read the source picture.
    let view = layer_rec_view(pCurDqLayer)
        .expect("the layer's reconstruction view is built for this frame");

    let mut iBestCost = i32::MAX;

    let iOffset = ((*pMbCache).uiNeighborIntra & 0x07) as usize;
    let iAvailCount = g_kiIntraChromaAvailMode[iOffset][4] as i32;
    let kpAvailMode = &g_kiIntraChromaAvailMode[iOffset];

    let pfMdCost8x8 = pFunc.sSampleDealingFuncs.md_cost(BLOCK_8x8).unwrap();
    // **T9.B30**: the two source blocks by coordinate. This function has neither an
    // `SMB` nor a slice in scope — it is one of the three readers the carrier's
    // `iMbX`/`iMbY` exist for.
    let pEncPicture = layer_enc_view(pCurDqLayer).expect("the layer's source view is built for this frame");
    let (kiChrOrgX, kiChrOrgY) = (*pMbCache).SPicData.chroma_origin();
    let kiPredOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);

    let mut iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount as usize {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        let pfChromaPred = pFunc.pfGetChromaPred[iCurMode as usize].unwrap();
        // `pDstChma` is `sMemPredMb` at the chroma half's `iChmaIdx` 128-byte side;
        // as an offset it is that side's start, and the Cr block sits 64 beyond it.
        let kiDstOff = kiPredOff + 128 * iChmaIdx;
        // **T9.C2**: `pDecCb`/`pDecCr` were raw roots into the reconstruction chroma
        // planes; they are the seam's two plane views at this macroblock's chroma
        // origin, which the carrier already holds for the cost sites below. The
        // destination stays the arena half it always was. Slot flipped rather than
        // bypassed — the mode index is a runtime value (F118's exemption is for
        // fixed-size sites only).
        pfChromaPred(
            (&mut (*pMbCache).sMemPredMb[kiDstOff..kiDstOff + 64])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cb
        let mut iCurCost = pfMdCost8x8(
            &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb[kiDstOff..][..64], 0, 8),
            &pEncPicture.plane(1).cursor(kiChrOrgX, kiChrOrgY),
        );

        pfChromaPred(
            (&mut (*pMbCache).sMemPredMb[kiDstOff + 64..kiDstOff + 128])
                .try_into()
                .expect("a packed 8x8 chroma prediction block is 64 bytes"),
            &view.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ); //Cr
        iCurCost += pfMdCost8x8(
            &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb[kiDstOff + 64..][..64], 0, 8),
            &pEncPicture.plane(2).cursor(kiChrOrgX, kiChrOrgY),
        ) + iLambda * BsSizeUE(crate::encoder::md::g_kiMapModeIntraChroma[iCurMode as usize] as u32) as i32;
        if iCurCost < iBestCost {
            iBestMode = iCurMode;
            iBestCost = iCurCost;
            iChmaIdx ^= 0x01;
        }
    }

    (*pMbCache).uiBestPredIntraChromaHalf = (iChmaIdx ^ 0x01) as u8;
    (*pMbCache).uiChmaI8x8Mode = iBestMode as u8;
    iBestCost
}

/// `svc_base_layer_md.cpp:932`. The non-`LOW_COMPLEXITY` `pfIntraFineMd`.
///
/// # Safety
/// Same as [`WelsMdI4x4`].
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdIntraFinePartition(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdIntraFinePartitionVaa(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> i32 {
    let pCurLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    // S10.5: the source macroblock through the seam, not `pEncData`'s raw root.
    let encView = crate::encoder::svc_encode_slice::layer_enc_view(&*pCurLayer)
        .expect("the frame's source view is stamped with pEncData");
    let cEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 0);
    if MdIntraAnalysisVaaInfo(pEncCtx, &cEncMb) {
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdIntraMb(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    //initial prediction memory for I_16x16
    (*pWelsMd).iCostLuma = crate::encoder::svc_mode_decision::WelsMdI16x16(
        (*pEncCtx).func_list(),
        (current_layer(pEncCtx)).as_ref(),
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

// D-dead-2 / F122: `g_kiPixStrideIdx4x4` (the 4x4 refinement's pixel-stride table)
// deleted with its only reader, `WelsMdInterMbRefinement`'s `SUB_MB_TYPE_4x4` arm.
// `g_kiPixStrideIdx8x8` above stays — the 8x8 arm is the one partition this encoder
// actually produces.

/// `svc_base_layer_md.cpp:321`. Per-macroblock inter setup: neighbour cache, the
/// reference-plane pointers, and the integer MV clamp for this macroblock position.
///
pub fn WelsMdInterInit(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    // Stays raw, and the reason is one call below: `pfFillInterNeighborCache` is a
    // neighbour-walker (`pCurMb.offset(-1)` in `FillNeighborCacheInter*`), so its
    // slot type keeps `*mut SMB` and this parameter has to carry the array's
    // provenance to it. Session E's classifier propagated neighbour-boundness
    // through *named* callees and could not see this one, because the call goes
    // through a function-pointer slot — Miri's encode probe found it.
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    iSliceFirstMbXY: i32,
) {
    let pCurLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let kiMbX = mbs.cur().iMbX as i32;
    let kiMbY = mbs.cur().iMbY as i32;
    let kiMbXY = mbs.cur().iMbXY;
    let kiMbWidth = (*pCurLayer).iMbWidth as i32;
    let kiMbHeight = (*pCurLayer).iMbHeight as i32;

    //step 1. load neighbor cache
    //
    // **T6.F0**: the skip-SAD array goes to the fill function whole. The C++ stashes
    // `pDecPic->pMbSkipSad + kiMbXY` in `pMbCache->pEncSad` first and the callee walks
    // backwards off it; the array is the picture's own now, so the callee indexes
    // `iMbXY + <neighbour offset>` against its root under the same guards.
    (*pEncCtx).func_list()
        .pfFillInterNeighborCache
        .expect("pfFillInterNeighborCache unset")(
        &mut *pMbCache,
        &*mbs,
        // S4.C4: the whole array, shared. T9.E7 (F132 round 8) had to reach the
        // root through `addr_of!` because `Vec::as_mut_ptr` autorefs `&mut` on the
        // SHARED VAA struct's vector — a retag-write every worker made per
        // macroblock, and the race the fixed-slice probe stopped on. A shared slice
        // is that reader path spelled directly, and the `.add(kiMbXY)` pre-offset
        // goes with it: the callee indexes `iMbXY + <neighbour offset>` off the root
        // now, which is what T6.F0 did to `pMbSkipSad` beside it.
        &(*pEncCtx)
            .vaa()
            .expect("the frame's video-analysis block")
            .pVaaBackgroundMbFlag[..],
        layer_rec_view(&*pCurLayer)
            .expect("the layer's reconstruction picture is bound")
            .mb_skip_sad(),
    ); //BGD spatial pFunc

    //step 4. locating current p_ref
    //
    // **S4.C2**, as `WelsMdIntraInit`'s: the three reference cursors resolve at use
    // through [`ref_mb`] now, so the absolute-vs-walk branch collapses to the
    // coordinate stamp both arms shared. The resolver keeps the stamp's derivation
    // exactly — a shared resolution of the pre-fork-stamped pool picture (E3's
    // harvest; the route `sMvList`/`uiRefMbType` have taken since T9.C3), plane
    // origins minted through the shared root so two workers are siblings (F71, S28),
    // and chroma plane 2 addressed with **stride index 1**, which is what the
    // single `kiCurStrideUV` applied to both chroma planes here.
    (*pMbCache).SPicData.iMbX = kiMbX;
    (*pMbCache).SPicData.iMbY = kiMbY;

    (*pMbCache).uiRefMbType = (&layer_ref_pic(pEncCtx, &*pCurLayer).expect("bound").uiRefMbType)[kiMbXY as usize];
    (*pMbCache).bCollocatedPredFlag = false;

    //comment: sometimes, mode decision process may skip the md_p16x16 and md_pskip function,
    mbs.cur_mut().sP16x16Mv = SMVUnitXY { iMvX: 0, iMvY: 0 };
    layer_rec_view(&*pCurLayer)
        .expect("bound")
        .mv_list()
        .set(kiMbXY as usize, SMVUnitXY { iMvX: 0, iMvY: 0 });

    SetMvWithinIntegerMvRange(
        kiMbWidth,
        kiMbHeight,
        kiMbX,
        kiMbY,
        (*pEncCtx).iMvRange,
        &mut (*pSlice).sMvStartMin,
        &mut (*pSlice).sMvStartMax,
    );
}

/// `svc_base_layer_md.cpp:1023`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
// unsafe-cat: fork-shared(S63) — the layer/SMB cursors (E3's grid); the
// dispatch cursor this tag used to name is a shared reference since T9.F4
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdP16x8<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: &'a SDqLayer,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
) -> i32 {
    let mut iCostP16x8 = 0i32;
    for i in 0..2i32 {
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let sMe16x8 = &mut (*pWelsMd).sMe.sMe16x8[i as usize];
        let iPixelY = i << 3;
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_16x8 as i32,
            crate::encoder::svc_encode_slice::layer_ref_feature_storage(pEncCtx, &*pCurDqLayer),
            sMe16x8,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        (*sMe16x8).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe16x8).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 1) as u32;

        (*pSlice).sMvc[0] = (*sMe16x8).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredInter16x8Mv(&(*pMbCache).sMvComponents, i << 3, 0, &mut (*sMe16x8).sMvp);
        {
            let pEncPicture = layer_enc_view(pCurDqLayer).expect("the layer's source view is built for this frame");
            let pRefPicture = layer_ref_view(pEncCtx, &*pCurDqLayer).expect("the layer's reference view is built for this frame");
            pFunc.pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
                &pFunc.sMeFuncs,
                &pFunc.sSampleDealingFuncs,
                sMe16x8,
                &mut *pSlice,
                pEncPicture.plane(0),
                pRefPicture.plane(0),
            );
        }
        // T9.E2b: the slot call above passes `&mut *pSlice` (the typedef flipped
        // with its family, S52), and that whole-slice reborrow pops every cursor
        // derived from the slice before it — q1c is blind here in both kinds
        // (dispatch slot, F111/F144.3; raw-param root). Fresh window per use
        // cluster, F144.2's spelling; the loop head re-derives for the next
        // iteration's own reads.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        UpdateP16x8Motion2Cache(
            &mut (*pMbCache).sMvComponents,
            i << 3,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe16x8).sMv,
        );
        iCostP16x8 += (*sMe16x8).uiSatdCost as i32;
    }
    iCostP16x8
}

/// `svc_base_layer_md.cpp:1053`.
///
/// # Safety
/// All pointers must be valid and `pfMotionSearch[0]` assigned.
// unsafe-cat: fork-shared(S63) — the layer/SMB cursors (E3's grid); the
// dispatch cursor this tag used to name is a shared reference since T9.F4
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdP8x16<'a>(
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
        let sMe8x16 = &mut (*pWelsMd).sMe.sMe8x16[i as usize];
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_8x16 as i32,
            crate::encoder::svc_encode_slice::layer_ref_feature_storage(pEncCtx, &*pCurLayer),
            sMe8x16,
        );
        //not putting the lines below into InitMe to avoid judging mode in InitMe
        (*sMe8x16).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe8x16).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 1) as u32;

        (*pSlice).sMvc[0] = (*sMe8x16).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredInter8x16Mv(&(*pMbCache).sMvComponents, i << 2, 0, &mut (*sMe8x16).sMvp);
        {
            let pEncPicture = layer_enc_view(pCurLayer).expect("the layer's source view is built for this frame");
            let pRefPicture = layer_ref_view(pEncCtx, &*pCurLayer).expect("the layer's reference view is built for this frame");
            pFunc.pfMotionSearch[0].expect("pfMotionSearch[0] unset")(
                &pFunc.sMeFuncs,
                &pFunc.sSampleDealingFuncs,
                sMe8x16,
                &mut *pSlice,
                pEncPicture.plane(0),
                pRefPicture.plane(0),
            );
        }
        // T9.E2b: the slot call above passes `&mut *pSlice` (the typedef flipped
        // with its family, S52), and that whole-slice reborrow pops every cursor
        // derived from the slice before it — q1c is blind here in both kinds
        // (dispatch slot, F111/F144.3; raw-param root). Fresh window per use
        // cluster, F144.2's spelling; the loop head re-derives for the next
        // iteration's own reads.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        UpdateP8x16Motion2Cache(
            &mut (*pMbCache).sMvComponents,
            i << 2,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe8x16).sMv,
        );
        iCostP8x16 += (*sMe8x16).uiSatdCost as i32;
    }
    iCostP8x16
}

// `WelsMdP4x4`, `WelsMdP8x4` and `WelsMdP4x8` stood here (`svc_base_layer_md.cpp:1120`,
// `:1159`, `:1198`). **Deleted under D-dead-1 (T9.B23, S18)**: no caller anywhere in
// the port, `tests/` included, and upstream reaches them only from inside
// `svc_mode_decision.cpp:635`'s `#if 0 //Disable for sub8x8 modes for now` — F115.
// Sub-8x8 partitions are a feature the codec does not run; a session that wants them
// back ports the three from the reference, where they are ~40 lines each.

/// `svc_base_layer_md.cpp:1238`. The non-VAA (`!LOW_COMPLEXITY`) fine partition search.
///
/// # Safety
/// All pointers must be valid.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterFinePartition<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    iBestCost: i32,
) {
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let mut iCost = crate::encoder::svc_mode_decision::WelsMdP8x8(
        pEncCtx,
        (*pEncCtx).func_list(),
        &*pCurDqLayer,
        pWelsMd,
        pSlice,
    );

    if iCost < iBestCost {
        (*pCurMb).uiMbType = MB_TYPE_8x8;
        (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];

        let mut iCostPart = WelsMdP16x8(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
        if iCostPart <= iCost {
            iCost = iCostPart;
            (*pCurMb).uiMbType = MB_TYPE_16x8;
        }

        iCostPart = WelsMdP8x16(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterFinePartitionVaa<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    iBestCostIn: i32,
) {
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let mut iBestCost = iBestCostIn;
    let uiMbSign = (*pEncCtx).func_list()
        .pfGetMbSignFromInterVaa
        .expect("pfGetMbSignFromInterVaa unset")(
        {
            // T9.E7, as the background-flag mint above (F132 round 8's class).
            let v = std::ptr::addr_of!((*pEncCtx).vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSad8x8);
            (*v).as_ptr().add((*pCurMb).iMbXY as usize) as *mut i32
        },
    );

    if crate::encoder::dump_enabled(&FP_DUMP, "OH264_FPDUMP") {
        let sad = (&(*pEncCtx).vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSad8x8)[(*pCurMb).iMbXY as usize];
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
            let iCostP16x8 = WelsMdP16x8(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
            if iCostP16x8 < iBestCost {
                iBestCost = iCostP16x8;
                (*pCurMb).uiMbType = MB_TYPE_16x8;
            }
        }
        5 | 10 => {
            let iCostP8x16 = WelsMdP8x16(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
            if iCostP8x16 < iBestCost {
                iBestCost = iCostP8x16;
                (*pCurMb).uiMbType = MB_TYPE_8x16;
            }
        }
        6 | 9 => {
            let iCostP8x8 = crate::encoder::svc_mode_decision::WelsMdP8x8(
        pEncCtx,
        (*pEncCtx).func_list(),
                &*pCurDqLayer,
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
        pEncCtx,
        (*pEncCtx).func_list(),
                &*pCurDqLayer,
                pWelsMd,
                pSlice,
            );
            if iCostP8x8 < iBestCost {
                iBestCost = iCostP8x8;
                (*pCurMb).uiMbType = MB_TYPE_8x8;
                (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];

                let iCostP16x8 = WelsMdP16x8(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
                if iCostP16x8 <= iBestCost {
                    iBestCost = iCostP16x8;
                    (*pCurMb).uiMbType = MB_TYPE_16x8;
                }

                let iCostP8x16 = WelsMdP8x16(pEncCtx, (*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdPSkipEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let pCurLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let pFunc = (*pEncCtx).func_list();

    // **T9.B22 — the three reference cursors are gone.** `SPicData.pRefMb[i]` was
    // read here and offset by the clipped motion vector at each of the three motion
    // compensations below; each is now a `PlaneCursor` anchored straight at the
    // sample the C++ arithmetic lands on. The reference picture resolves through
    // `layer_ref_pic`, which names the same picture the `pRefMb` stamp reads —
    // `WelsInitCurrentLayer` stamps that view as `(*pRefList).pic_mut(pRefPic)
    // .view()` (`encoder_ext.rs:1952`), so the strides this drops were the same
    // numbers `plane(i).stride()` returns.
    //
    // `pDstLuma`/`pDstCb`/`pDstCr` are **not** bound here any more, and that is the
    // point rather than tidying: each motion compensation now takes `&mut` to its
    // own slice of `sSkipMb`, and a raw into that field held across such a borrow is
    // F114a's shape C exactly. They are re-derived below, after the last of those
    // borrows has ended — T9.D11's rule, "derive at the call, never hold across".
    // (T9.D3's note about one derivation rather than three applied to a raw that was
    // live across the whole body; there is no such raw here now.)
    let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
    let mut n: i32;

    // S9.0: `iEncStride` retires with the raw operands — it existed only to hand the
    // DCT a stride the cursor now carries. The plane-2 call below relied on it still
    // holding `iEncStride[1]`, which is the same rule `stride_idx` states and which
    // the view reproduces by construction (both chroma planes share one stride).
    let encView = crate::encoder::svc_encode_slice::layer_enc_view(&*pCurLayer)
        .expect("the frame's source view is stamped with pEncData");
    let mut pEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 0);
    // T9.H2: `&sWelsEncCtx`. The layer id is read through the same raw beside it —
    // both are shared reads, so the argument and the borrow coexist by construction
    // rather than by the hoist T9.G6 needed when the callee took a `&mut`.
    let pStrideEncBlockOffset = crate::encoder::encoder_context::ctx_stride_enc_block_offset(
        &*pEncCtx,
        (*pEncCtx).uiDependencyId as usize,
    );
    let mut pEncBlockOffset: *const i32;

    let iSadCostLuma: i32;
    let mut iSadCostChroma: i32;
    let iSadCostMb: i32;

    PredSkipMv(&(*pMbCache).sMvComponents, &mut sMvp);

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

    // The macroblock's own position, which is what `SPicData.pRefMb` encoded as an
    // address: it is stamped as `data_ptr_shared(i) + ((mbX + mbY * stride) << 4)`
    // for luma and `<< 3` for chroma (`WelsMdInterInit`, `:945`). A cursor says the
    // same thing in samples, and the plane's padding is what makes the clipped
    // vector's excursion outside the visible picture addressable rather than merely
    // survivable.
    let kiMbXLuma = ((*pCurMb).iMbX as isize) << 4;
    let kiMbYLuma = ((*pCurMb).iMbY as isize) << 4;
    let kiMbXChroma = ((*pCurMb).iMbX as isize) << 3;
    let kiMbYChroma = ((*pCurMb).iMbY as isize) << 3;

    //luma
    {
        let pRefPicture = layer_ref_view(pEncCtx, &*pCurLayer).expect("the layer's reference view is built for this frame");
        let cRefLuma = pRefPicture.plane(0).cursor(
            kiMbXLuma + sQpelMvp.iMvX as isize,
            kiMbYLuma + sQpelMvp.iMvY as isize,
        );
        let mut cDstLuma = PlaneCursorMut::new(&mut (*pMbCache).sSkipMb[..256], 0, 16);
        mc_luma(&cRefLuma, &mut cDstLuma, sMvp.iMvX, sMvp.iMvY, 16, 16);
    }
    // **T9.B26 — the three SAD sites call the kernel directly.** `pfSampleSad
    // [BLOCK_16x16]` is a compile-time index into a table with one writer and no CPU
    // flag (`WelsInitSampleSadFunc`), so the slot is a constant from the first frame
    // on and `sample_sad::<16, 16, _>` *is* what it held — byte-identical by the F118
    // argument, with no table on the path. The source operand is the same sample
    // `SPicData.pEncMb[0]` named: the source picture through the layer's handle
    // (`layer_enc_view`, S10.2), anchored at the macroblock's own origin; the
    // prediction is the `sSkipMb` region the motion compensation above just wrote,
    // borrowed **shared** — which does not pop the raw `pDstLuma` below the way a
    // `&mut` borrow would (F114a), and the raw is re-derived after it anyway.
    iSadCostLuma = {
        let pEncPicture = layer_enc_view(&*pCurLayer).expect("the layer's source view is built for this frame");
        let cEncLuma = pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        let cSkipLuma = RecCursor::over_owned(&mut (*pMbCache).sSkipMb[..256], 0, 16);
        sample_sad::<16, 16, _>(&cEncLuma, &cSkipLuma)
    };

    // `iStrideUV` was `(mvY >> 1) * strideUV + (mvX >> 1)` off the chroma macroblock
    // origin; in samples that is `(mvX >> 1, mvY >> 1)` from the same origin, and
    // `sQpelMvp` is already `sMvp >> 2`, so both are `sMvp >> 3`.
    {
        let pRefPicture = layer_ref_view(pEncCtx, &*pCurLayer).expect("the layer's reference view is built for this frame");
        let cRefCb = pRefPicture.plane(1).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCb = PlaneCursorMut::new(&mut (*pMbCache).sSkipMb[256..320], 0, 8);
        mc_chroma(&cRefCb, &mut cDstCb, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cb
    }
    iSadCostChroma = {
        let pEncPicture = layer_enc_view(&*pCurLayer).expect("the layer's source view is built for this frame");
        let cEncCb = pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
        let cSkipCb = RecCursor::over_owned(&mut (*pMbCache).sSkipMb[256..320], 0, 8);
        sample_sad::<8, 8, _>(&cEncCb, &cSkipCb)
    };

    {
        let pRefPicture = layer_ref_view(pEncCtx, &*pCurLayer).expect("the layer's reference view is built for this frame");
        let cRefCr = pRefPicture.plane(2).cursor(
            kiMbXChroma + (sQpelMvp.iMvX as isize >> 1),
            kiMbYChroma + (sQpelMvp.iMvY as isize >> 1),
        );
        let mut cDstCr = PlaneCursorMut::new(&mut (*pMbCache).sSkipMb[320..384], 0, 8);
        mc_chroma(&cRefCr, &mut cDstCr, sMvp.iMvX, sMvp.iMvY, 8, 8); //Cr
    }
    iSadCostChroma += {
        let pEncPicture = layer_enc_view(&*pCurLayer).expect("the layer's source view is built for this frame");
        let cEncCr = pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
        let cSkipCr = RecCursor::over_owned(&mut (*pMbCache).sSkipMb[320..384], 0, 8);
        sample_sad::<8, 8, _>(&cEncCr, &cSkipCr)
    };

    iSadCostMb = iSadCostLuma + iSadCostChroma;

    if iSadCostMb == 0
        || iSadCostMb < (*pWelsMd).iSadPredSkip
        || (layer_ref_pic(pEncCtx, &*pCurLayer).map_or(false, |p| p.iPictureType == EWelsSliceType::P_SLICE as i32)
            && (*pMbCache).uiRefMbType == MB_TYPE_SKIP
            && iSadCostMb < (&layer_ref_pic(pEncCtx, &*pCurLayer).expect("bound").pMbSkipSad)[(*pCurMb).iMbXY as usize])
    {
        //update motion info to current MB
        AcceptPskip(pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
        return true;
    }

    // The residual path below is the forward DCT's, which still takes raw operands
    // (step 4 of session B3 flips it); `pDstLuma` is derived here, at the call, after
    // every borrow above has ended (F114a).
    let pDstLuma = RecCursor::over_owned(&mut (*pMbCache).sSkipMb, 0, 16);
    WelsDctMb(
        &mut (*pMbCache).sCoeffLevel,
        &pEncMb,
        &pDstLuma,
        (*pEncCtx).func_list().pfDctFourT4,
    );

    if WelsTryPYskip(pEncCtx, pCurMb, pMbCache) {
        pEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 1);
        pEncBlockOffset = pStrideEncBlockOffset.add(16);
        let pDstCb = RecCursor::over_owned(&mut (*pMbCache).sSkipMb, 256, 8);
        (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset")(
            &mut (*pMbCache).sCoeffLevel[256..],
            &pEncMb.advance(*pEncBlockOffset as isize, 0),
            &pDstCb,
        );
        if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 1) {
            pEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 2);
            pEncBlockOffset = pStrideEncBlockOffset.add(20);
            let pDstCr = RecCursor::over_owned(&mut (*pMbCache).sSkipMb, 320, 8);
            (*pFunc).pfDctFourT4.expect("pfDctFourT4 unset")(
                &mut (*pMbCache).sCoeffLevel[320..],
                &pEncMb.advance(*pEncBlockOffset as isize, 0),
                &pDstCr,
            );
            if WelsTryPUVskip(pEncCtx, pCurMb, pMbCache, 2) {
                //update motion info to current MB
                // (T9.D6 re-derived a raw `pDstLuma` here because three arena-taking
                // calls sit between the top of the function and this point; since
                // T9.B26 `AcceptPskip` borrows the prediction from the arena itself,
                // so there is no raw to re-derive.)
                AcceptPskip(pEncCtx, pWelsMd, pCurMb, pMbCache, &sMvp, iSadCostLuma, iSadCostMb);
                return true;
            }
        }
    }
    false
}

/// The block `WelsMdPSkipEnc` runs verbatim at both of its `return true` sites
/// (`svc_base_layer_md.cpp:1489` and `:1521`).
///
/// **T9.B26**: the `kpPicData: &SPicData` / `pDstLuma: *mut u8` pair is gone. The
/// one thing it read from the carrier was `pEncMb[0]`, the source macroblock, which
/// the layer's handle names (`layer_enc_view` + the macroblock's origin); the
/// prediction is the arena's own `sSkipMb`, borrowed here for the SATD and for
/// nothing else. `pMbCache` comes in as `&SMbCache` rather than `&mut` because this
/// body only reads it — and it is a protector for the call (F114b, S56): nothing
/// below touches the arena through another path (`pfUpdateMbMv` writes the
/// macroblock's own MV row, `layer_dec_pic_mut` the reconstruction pool), which is
/// the walk that licenses the shared reference.
///
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
    let pCurLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let pFunc = (*pEncCtx).func_list();

    // ST32 (pCurMb->pRefIndex, 0)
    (*pCurMb).iRefIndex = [0; crate::encoder::md::MB_BLOCK8x8_NUM];
    (*pFunc).pfUpdateMbMv.expect("pfUpdateMbMv unset")(&mut (*pCurMb).sMv, *sMvp);

    if (*pWelsMd).bMdUsingSad {
        (*pCurMb).iSadCost = iSadCostLuma;
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        // `pfSampleSatd[BLOCK_16x16]`, constant after init (F118) — called direct.
        let pEncPicture = layer_enc_view(&*pCurLayer).expect("the layer's source view is built for this frame");
        let cEncLuma = pEncPicture
            .plane(0)
            .cursor(((*pCurMb).iMbX as isize) << 4, ((*pCurMb).iMbY as isize) << 4);
        // S10.2: `pMbCache` is shared here, so this stays a `PlaneCursor` — a
        // read-only view of an owned pane. `satd_16x16` is generic over its two
        // operands independently, so the source plane's `RecCursor` and this
        // cursor meet without either being converted.
        let cSkipLuma = PlaneCursor::new(&pMbCache.sSkipMb[..256], 0, 16);
        (*pWelsMd).iCostLuma = satd_16x16(&cEncLuma, &cSkipLuma);
    }

    (*pWelsMd).iCostSkipMb = iSadCostMb;

    (*pCurMb).sP16x16Mv = *sMvp;
    layer_rec_view(&*pCurLayer)
        .expect("bound")
        .mv_list()
        .set((*pCurMb).iMbXY as usize, *sMvp);
}

/// `svc_base_layer_md.cpp:1573`. Quarter-pel refinement of whichever partitioning the
/// integer search chose, plus the chroma motion compensation for each partition.
///
/// # Safety
/// All four pointers must be valid; the `pfCopy*` slots and `sMcFuncs.pMcChromaFunc`
/// must be assigned.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbRefinement(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
    let pFunc = (*pEncCtx).func_list();
    let mut iBestSadCost = 0i32;
    let mut iBestSatdCost = 0i32;
    let mut sMeRefine = SMeRefinePointer::default();

    // **T9.B28 — the chroma reference cursors are gone.** `SPicData.pRefMb[1|2]` was
    // read here and offset per partition and per motion vector at each of the twelve
    // `McChroma_c` calls below; each is now a `PlaneCursor` on the reference picture,
    // anchored by coordinate. The macroblock's chroma origin is what `pRefMb[i]`
    // encoded as an address (`WelsMdInterInit`: `data_ptr_shared(i) +
    // ((mbX + mbY * stride) << 3)`), the per-partition `iRefBlk4Stride` is a byte
    // offset `blk4Y * stride + blk4X` — i.e. the coordinate `(blk4X, blk4Y)` — and
    // `iMvStride` is `(mvY >> 3) * stride + (mvX >> 3)`, i.e. `(mvX >> 3, mvY >> 3)`.
    // The strides this drops (the reference picture's `stride(1)`) are the numbers
    // `plane(1).stride()` returns; the view is stamped from the same picture
    // (`encoder_ext.rs:1952`).
    let kiMbXChroma = ((*pCurMb).iMbX as isize) << 3;
    let kiMbYChroma = ((*pCurMb).iMbY as isize) << 3;

    // `pBufMe` stood here — one raw derivation of `sBufferInterPredMe` handed to every
    // `MeRefineFracPixel` call. **T9.B29** deleted it: the refinement borrows the
    // field itself through the `&mut SMbCache` it now takes.

    // Byte offsets of the three prediction regions inside `sMemPredMb`, not pointers:
    // an index cannot be invalidated by a retag, and the twelve chroma motion
    // compensations below take `&mut` to their own slice of that field while the luma
    // raw (`MeRefineFracPixel`'s destination) has to survive the whole body. T9.D3's
    // note about deriving all four cursors adjacently applied to raws held across the
    // body; the rule that replaces it is T9.D11's — derive at the call.
    let kiOffLuma = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
    let kiOffCb = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);
    let kiOffCr = kiOffCb + 64;

    /// One chroma motion compensation, per partition. `$plane` is 1 (Cb) or 2 (Cr);
    /// `($dx, $dy)` is the partition's own chroma offset **plus** the motion vector's
    /// integer chroma part, in samples from the macroblock's chroma origin; `$off` is
    /// the destination's byte offset inside `sMemPredMb`, whose prediction rows are
    /// 8 samples apart.
    ///
    /// The destination slice is exactly the block's span, `($h - 1) * 8 + $w` — the
    /// same discipline `common/mc.rs`'s shims apply to a raw pointer, stated here in
    /// the type instead. Each borrow ends with the statement (F114a).
    macro_rules! mc_chroma_at {
        ($plane:expr, $off:expr, $dx:expr, $dy:expr, $mv:expr, $w:expr, $h:expr) => {{
            let pRefPicture =
                layer_ref_view(pEncCtx, &*pCurDqLayer).expect("the layer's reference view is built for this frame");
            let cRef = pRefPicture
                .plane($plane)
                .cursor(kiMbXChroma + ($dx) as isize, kiMbYChroma + ($dy) as isize);
            let mut cDst = PlaneCursorMut::new(
                &mut (*pMbCache).sMemPredMb[($off)..][..(($h) - 1) * 8 + ($w)],
                0,
                8,
            );
            mc_chroma(&cRef, &mut cDst, ($mv).iMvX, ($mv).iMvY, $w, $h);
        }};
    }

    match (*pCurMb).uiMbType {
        MB_TYPE_16x16 => {
            //luma
            InitMeRefinePointer(&mut sMeRefine, 0);
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_16x16(a, b)); // was `(*pFunc).pfCopy16x16NotAligned` (T9.B29)
            MeRefineFracPixel(
                pEncCtx,
                kiOffLuma,
                &mut (*pWelsMd).sMe.sMe16x16,
                &mut sMeRefine,
                pMbCache,
                16,
                16,
            );
            UpdateP16x16MotionInfo(
                &mut (*pMbCache).sMvComponents,
                pCurMb,
                (*pWelsMd).uiRef as i8,
                &mut (*pWelsMd).sMe.sMe16x16.sMv,
            );

            (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;
            //save the best cost of final mode
            iBestSadCost = (*pWelsMd).sMe.sMe16x16.uiSadCost as i32;
            iBestSatdCost = (*pWelsMd).sMe.sMe16x16.uiSatdCost as i32;

            //chroma
            let sMv = (*pWelsMd).sMe.sMe16x16.sMv;
            let dx = sMv.iMvX as i32 >> 3;
            let dy = sMv.iMvY as i32 >> 3;
            mc_chroma_at!(1, kiOffCb, dx, dy, sMv, 8, 8); //Cb
            mc_chroma_at!(2, kiOffCr, dx, dy, sMv, 8, 8); //Cr

            // The three cost sites: constant block indices, so the kernels are called
            // direct (F118, and `WelsInitSampleSadFunc`'s doc carries the proof). The
            // source operand is the sample `SPicData.pEncMb[i]` named — the source
            // picture through the layer's handle at the macroblock's own origin.
            let pEncPicture =
                layer_enc_view(&*pCurDqLayer).expect("the layer's source view is built for this frame");
            let cEncLuma = pEncPicture.plane(0).cursor(kiMbXChroma << 1, kiMbYChroma << 1);
            let cEncCb = pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
            let cEncCr = pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
            (*pWelsMd).iCostSkipMb = sample_sad::<16, 16, _>(
                &cEncLuma,
                &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb[kiOffLuma..][..256], 0, 16),
            );
            (*pWelsMd).iCostSkipMb += sample_sad::<8, 8, _>(
                &cEncCb,
                &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb[kiOffCb..][..64], 0, 8),
            );
            (*pWelsMd).iCostSkipMb += sample_sad::<8, 8, _>(
                &cEncCr,
                &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb[kiOffCr..][..64], 0, 8),
            );
        }

        MB_TYPE_16x8 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_16x8(a, b)); // was `(*pFunc).pfCopy16x8NotAligned` (T9.B29)
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 3;
                InitMeRefinePointer(&mut sMeRefine, iPixStride);
                iPixStride += ME_REFINE_BUF_STRIDE_BLK8;
                PredInter16x8Mv(
                    &(*pMbCache).sMvComponents,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe16x8[i].sMvp,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    kiOffLuma + g_kuiSmb4AddrIn256[iIdx as usize] as usize,
                    &mut (*pWelsMd).sMe.sMe16x8[i],
                    &mut sMeRefine,
                pMbCache,
                    16,
                    8,
                );
                UpdateP16x8MotionInfo(
                    &mut (*pMbCache).sMvComponents,
                    pCurMb,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe16x8[i].sMv,
                );
                (*pMbCache).sMbMvp[i] = (*pWelsMd).sMe.sMe16x8[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += (*pWelsMd).sMe.sMe16x8[i].uiSadCost as i32;
                iBestSatdCost += (*pWelsMd).sMe.sMe16x8[i].uiSatdCost as i32;

                //chroma
                // `iRefBlk4Stride` was `(i << 2) * strideUV` — a pure row offset, so
                // the partition sits `4 * i` rows down and in column 0; the
                // destination's `i << 5` is `4 * i` rows at stride 8, the same place.
                let iBlk4Y = (i as i32) << 2;
                let sMv = (*pWelsMd).sMe.sMe16x8[i].sMv;
                let dx = sMv.iMvX as i32 >> 3;
                let dy = iBlk4Y + (sMv.iMvY as i32 >> 3);
                let iDstOff = (i as usize) << 5; // 4 rows x 8
                mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 8, 4); //Cb
                mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 8, 4); //Cr
            }
        }

        MB_TYPE_8x16 => {
            let mut iPixStride = 0i32;
            sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_8x16(a, b)); // was `(*pFunc).pfCopy8x16Aligned` (T9.B29)
            for i in 0..2usize {
                //luma
                let iIdx = (i as i32) << 2;
                InitMeRefinePointer(&mut sMeRefine, iPixStride);
                iPixStride += ME_REFINE_BUF_WIDTH_BLK8;
                PredInter8x16Mv(
                    &(*pMbCache).sMvComponents,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe8x16[i].sMvp,
                );
                MeRefineFracPixel(
                    pEncCtx,
                    kiOffLuma + g_kuiSmb4AddrIn256[iIdx as usize] as usize,
                    &mut (*pWelsMd).sMe.sMe8x16[i],
                    &mut sMeRefine,
                pMbCache,
                    8,
                    16,
                );
                update_P8x16_motion_info(
                    &mut (*pMbCache).sMvComponents,
                    pCurMb,
                    iIdx,
                    (*pWelsMd).uiRef as i8,
                    &mut (*pWelsMd).sMe.sMe8x16[i].sMv,
                );
                (*pMbCache).sMbMvp[i] = (*pWelsMd).sMe.sMe8x16[i].sMvp;
                //save the best cost of final mode
                iBestSadCost += (*pWelsMd).sMe.sMe8x16[i].uiSadCost as i32;
                iBestSatdCost += (*pWelsMd).sMe.sMe8x16[i].uiSatdCost as i32;

                //chroma
                // `iRefBlk4Stride` was `iIdx` (= 4 * i) added to a byte pointer with no
                // stride factor — a pure *column* offset — and the destination used the
                // same number, which at stride 8 is also column `4 * i` of row 0.
                let iBlk4X = iIdx; // 4 * i
                let sMv = (*pWelsMd).sMe.sMe8x16[i].sMv;
                let dx = iBlk4X + (sMv.iMvX as i32 >> 3);
                let dy = sMv.iMvY as i32 >> 3;
                let iDstOff = iBlk4X as usize;
                mc_chroma_at!(1, kiOffCb + iDstOff, dx, dy, sMv, 4, 8); //Cb
                mc_chroma_at!(2, kiOffCr + iDstOff, dx, dy, sMv, 4, 8); //Cr
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
                        sMeRefine.pfCopyBlockByMode = Some(|a, b| copy_8x8(a, b)); // was `(*pFunc).pfCopy8x8Aligned` (T9.B29)
                        //luma
                        InitMeRefinePointer(&mut sMeRefine, g_kiPixStrideIdx8x8[i]);
                        PredMv(
                            &(*pMbCache).sMvComponents,
                            iBlk8Idx as i8,
                            2,
                            (*pWelsMd).uiRef as i32,
                            &mut (*pWelsMd).sMe.sMe8x8[i].sMvp,
                        );
                        MeRefineFracPixel(
                            pEncCtx,
                            kiOffLuma + g_kuiSmb4AddrIn256[iBlk8Idx as usize] as usize,
                            &mut (*pWelsMd).sMe.sMe8x8[i],
                            &mut sMeRefine,
                pMbCache,
                            8,
                            8,
                        );
                        UpdateP8x8MotionInfo(
                            &mut (*pMbCache).sMvComponents,
                            pCurMb,
                            iBlk8Idx,
                            (*pWelsMd).uiRef as i8,
                            &mut (*pWelsMd).sMe.sMe8x8[i].sMv,
                        );
                        (*pMbCache).sMbMvp[g_kuiMbCountScan4Idx[iBlk8Idx as usize] as usize] =
                            (*pWelsMd).sMe.sMe8x8[i].sMvp;
                        iBestSadCost += (*pWelsMd).sMe.sMe8x8[i].uiSadCost as i32;
                        iBestSatdCost += (*pWelsMd).sMe.sMe8x8[i].uiSatdCost as i32;

                        //chroma
                        let sMv = (*pWelsMd).sMe.sMe8x8[i].sMv;
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
                    // **D-dead-2 / F122 — `SUB_MB_TYPE_4x4`, `_8x4` and `_4x8` deleted here.**
                    // Nothing in either encoder produces those values. In the port,
                    // every writer of `uiSubMbType` sets `SUB_MB_TYPE_8x8`
                    // (`:1164`, `:1249`, `:1262`, `svc_mode_decision.rs:2495`); in
                    // upstream the only writers are inside
                    // `WelsMdInterFinePartitionVaaOnScreen`'s
                    // `#if 0 //Disable for sub8x8 modes for now`
                    // (`svc_mode_decision.cpp:634-661`), the same block D-dead-1
                    // deleted `WelsMdP4x4`/`WelsMdP8x4`/`WelsMdP4x8` for. F122's probe
                    // read 0 entries in all three arms across three configurations
                    // while `SUB_MB_TYPE_8x8` read 264/68/540.
                    //
                    // The arm is `unreachable!` rather than a silent `{}` on purpose:
                    // if Phase 10 or a later re-port revives the sub-8x8 search, this
                    // is the refinement it must bring back with it, and a fall-through
                    // would emit a macroblock refined at the wrong partition size
                    // instead of saying so.
                    _ => unreachable!(
                        "sub-8x8 partition {:#x} — the sub-8x8 search is #if 0 upstream \
                         and unwritten here (D-dead-2/F122)",
                        (*pCurMb).uiSubMbType[i]
                    ),
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdFirstIntraMode(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) -> bool {
    let pFunc = (*pEncCtx).func_list();

    let iCostI16x16 = crate::encoder::svc_mode_decision::WelsMdI16x16(
        &*pFunc,
        (current_layer(pEncCtx)).as_ref(),
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
            WelsMdIntraChroma(&*pFunc, &*(current_layer(pEncCtx)), pMbCache, (*pWelsMd).iLambda);
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMb<'a>(
    pEncCtx: &'a sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
) {
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");
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
    if (*pEncCtx).func_list()
        .pfInterMdBackgroundDecision
        .expect("pfInterMdBackgroundDecision unset")(
        pEncCtx,
        pWelsMd,
        pSlice,
        mbs.cur_mut(),
        &mut bKeepSkip,
    ) {
        return;
    }

    //try static or scrolled Pskip
    if (*pEncCtx).func_list()
        .pfSCDPSkipDecision
        .expect("pfSCDPSkipDecision unset")(pEncCtx, pWelsMd, pSlice, mbs.cur_mut())
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
        // T9.E2b: a field borrow, not a raw — the parent is `&mut` now (F112's
        // one step); disjoint-field borrows coexist and NLL ends this one at its
        // last use below, before the next whole-slice reborrow.
        let pMbCache = &mut pSlice.sMbCacheInfo;
        PredictSad(
            &pMbCache.sMvComponents.iRefIndexCache,
            &pMbCache.iSadCost,
            0,
            &mut (*pWelsMd).iSadPredMb,
        );

        //step 2: P_16x16
        (*pWelsMd).iCostLuma = crate::encoder::svc_mode_decision::WelsMdP16x16(
            pEncCtx,
            (*pEncCtx).func_list(),
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
///
/// # Safety
/// `pCurMb` and `pMbCache` must be valid.
pub fn WelsMdInterDoubleCheckPskip(pCurMb: &mut SMB, pMbCache: &mut SMbCache) {
    if MB_TYPE_16x16 == (*pCurMb).uiMbType && 0 == (*pCurMb).uiCbp {
        if 0 == (*pCurMb).iRefIndex[0] {
            let mut sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };

            PredSkipMv(&(*pMbCache).sMvComponents, &mut sMvp);
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
pub fn WelsMdInterEncode(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pFunc = (*pEncCtx).func_list();
    let pCurDqLayer = current_layer_ref(pEncCtx)
        .expect("the frame's current layer is stamped");

    //add pEnc&rec to MD--2010.3.15
    let kiCsStrideY = (*pCurDqLayer).iCsStride[0];
    let kiCsStrideUV = (*pCurDqLayer).iCsStride[1];

    //add pEnc&rec to MD--2010.3.15
    (*pCurMb).uiCbp = 0;
    crate::encoder::svc_mode_decision::WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    crate::encoder::svc_encode_slice::WelsPMbChromaEncode(pEncCtx, pSlice, pCurMb);

    // **T9.C2 — the same triple T9.C7 converted in `WelsRecPskip`**, with
    // `sMemPredMb`'s two halves as the arena instead of `sSkipMb`: luma 16x16 at
    // stride 16 from the luma half, then the two chroma 8x8 at stride 8 from the
    // chroma half and 64 beyond it. Destination is the seam's cursor at this
    // macroblock's own origin; the three destination strides leave the call
    // because the view carries them (`iCsStride[i]` and the plane stride are
    // stamped from one `SPicture::stride(i)`).
    //
    // Slots bypassed, not flipped (F118) — the eight `pfCopy*` entries are
    // installed unconditionally by `WelsInitEncodingFuncs` and constant after
    // init, so a fixed-size site may call the kernel directly, byte-identically.
    let view = layer_rec_view(&*pCurDqLayer)
        .expect("the layer's reconstruction view is built for this frame");
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let (lx, ly) = (*pMbCache).SPicData.luma_origin();
    let (cx, cy) = (*pMbCache).SPicData.chroma_origin();
    let kiLumaOff = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
    let kiChromaOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);
    let src = &(*pMbCache).sMemPredMb;
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
/// **T6.F0**: both arrays arrive whole. The C++ writes the SAD through
/// `pMbCache->pEncSad`, the cursor `WelsMdInterInit` parked at `kiMbXY`; the cursor is
/// gone and this indexes `iMbXY` in the reconstruction picture's own array, exactly as
/// it already did for `pRefMbtypeList`. `pMbCache` was the only reason this function
/// took the arena and it no longer needs it.
///
/// # Safety
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
        if kmtCurMbtype == MB_TYPE_SKIP { (*pMd).iCostSkipMb } else { 0 },
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
