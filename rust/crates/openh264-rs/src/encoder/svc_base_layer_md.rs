//! Port of `codec/encoder/core/src/svc_base_layer_md.cpp` — the base-layer
//! mode-decision layer.
//!
//! This module currently carries the **intra (I-slice) half**: the tables, the
//! neighbour-mode predictor, and the `WelsMdIntraInit` -> `WelsMdIntraMb` chain that
//! `WelsISliceMdEnc` (`svc_encode_slice.cpp:562`/`:566`) drives. The inter half of the
//! file is still unported; see `rust/docs/encoder_port_status.md`.
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

use crate::encoder::encoder_context::sWelsEncCtx;
use crate::encoder::md::{
    FillNeighborCacheIntra, MdIntraAnalysisVaaInfo, SMB, SMbCache, SWelsMD,
    BsSizeUE, MB_TYPE_INTRA16x16, MB_TYPE_INTRA4x4,
};
use crate::encoder::svc_encode_mb::WelsEncRecI4x4Y;
use crate::encoder::svc_encode_slice::SDqLayer;
use crate::encoder::svc_mode_decision::{
    g_kiIntra16AvaliMode, g_kiMapModeI16x16, WelsMdIntraSecondaryModesEnc, BLOCK_16x16, BLOCK_4x4,
    BLOCK_8x8,
};
use crate::encoder::svc_set_mb_syn_cavlc::g_kuiCache48CountScan4Idx;
use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;

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

/// Guards the three untranslated `Combined3` SIMD fast paths. See the module docs:
/// the reference leaves these slots NULL on every target this port builds for, and
/// this port never assigns them, so the scalar branch is always the live one. Panics
/// rather than silently taking the wrong branch if that ever stops being true.
#[inline]
pub unsafe fn assert_no_combined3(p: *mut core::ffi::c_void, which: &str) {
    assert!(
        p.is_null(),
        "sSampleDealingFuncs.{which} is non-null, but its Combined3 fast path in \
         svc_base_layer_md.cpp is not translated (see the module docs). Taking the \
         scalar branch here would silently diverge from the C++."
    );
}

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

    assert_no_combined3((*pFunc).sSampleDealingFuncs.pfIntra4x4Combined3, "pfIntra4x4Combined3");
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
    let pIntra4x4PredMode = (*pCurMb).pIntra4x4PredMode;
    // ST32 (pCurMb->pIntra4x4PredMode, LD32 (&pMbCache->iIntraPredMode[33]));
    core::ptr::copy_nonoverlapping(
        (*pMbCache).iIntraPredMode.as_ptr().add(33),
        pIntra4x4PredMode,
        4,
    );
    *pIntra4x4PredMode.add(4) = (*pMbCache).iIntraPredMode[12];
    *pIntra4x4PredMode.add(5) = (*pMbCache).iIntraPredMode[20];
    *pIntra4x4PredMode.add(6) = (*pMbCache).iIntraPredMode[28];
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

    let pfMdCost4x4 = (*(*pFunc).sSampleDealingFuncs.pfMdCost.add(BLOCK_4x4)).unwrap();

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

    assert_no_combined3((*pFunc).sSampleDealingFuncs.pfIntra8x8Combined3, "pfIntra8x8Combined3");
    let pfMdCost8x8 = (*(*pFunc).sSampleDealingFuncs.pfMdCost.add(BLOCK_8x8)).unwrap();

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
