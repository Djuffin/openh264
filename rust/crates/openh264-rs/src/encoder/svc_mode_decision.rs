#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

//! SVC Spatial Enhancement Layer Mode Decision & Screen Content Coding Engine.
//!
//! Translated from `codec/encoder/core/inc/svc_mode_decision.h` and
//! `codec/encoder/core/src/svc_mode_decision.cpp`.

#![deny(unsafe_code)]


use crate::encoder::rec_view::{copy_block_to_view, RecCursor};
use crate::encoder::svc_encode_slice::{layer_enc_pic, layer_rec_view, layer_ref_pic};
use crate::encoder::svc_encode_slice::layer_pps;
use crate::encoder::svc_encode_slice::current_layer;
use crate::encoder::picture::{RecPicId, SrcPicId};
use crate::encoder::md::{PredictSad, PredictSadSkip, WelsMedian};
use crate::encoder::md::{mem_pred_chroma_off, mem_pred_luma_off};
use crate::encoder::svc_encode_mb::WelsEncInterY;
use crate::encoder::svc_encode_slice::WelsPMbChromaEncode;
use crate::encoder::svc_set_mb_syn_cavlc::IS_INTRA16x16;
use crate::encoder::vlc_encoder::BsSizeUE;
pub use crate::encoder::encoder_context::SMVUnitXY;
use crate::encoder::encoder_context::ctx_dq_layer;
pub use crate::encoder::encoder_context::SMVComponentUnit;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::picture::SScreenBlockFeatureStorage;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::wels_preprocess::EStaticBlockIdc;
pub use crate::encoder::md::SMcFunc;
// Phase 4a: MC is called directly, not via `sMcFuncs`.
use crate::common::mc::{mc_chroma, mc_luma, McChroma_c, McLuma_c};
use crate::common::sad_common::sample_sad;
use crate::encoder::sample::satd_16x16;
use crate::safe::plane::{PlaneCursor, PlaneCursorMut};
pub use crate::encoder::wels_preprocess::SVAACalcResult;
pub use crate::encoder::wels_preprocess::SScrollDetectionParam;
pub use crate::encoder::svc_motion_estimate::SWelsME;
use crate::safe::mvd_cost::MvdCostCursor;
pub use crate::encoder::md::SWelsMD;
pub use crate::encoder::wels_preprocess::SVAAFrameInfo;
pub use crate::encoder::wels_preprocess::SVAAFrameInfoExt;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::md::SMbCache;
pub use crate::encoder::encoder_context::SPicData;
pub use crate::encoder::md::SMB;
pub use crate::encoder::md::{MB_BLOCK4x4_NUM, MB_BLOCK8x8_NUM, MB_LUMA_CHROMA_BLOCK4x4_NUM};
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;

// ============================================================================
// Constants and Thresholds
// ============================================================================

pub const DELTA_QP_SCD_THD: i32 = 5;
pub const DELTA_QP_BGD_THD: i32 = 3;
pub const KNOWN_CHROMA_TOO_LARGE: i32 = 640;
pub const SMALLEST_INVISIBLE: i32 = 128; // 2 * 64
pub const MBVAASIGN_FLAT: u8 = 15;

pub const MB_LEFT_BIT: u32 = 0;
pub const MB_TOP_BIT: u32 = 1;
pub const MB_TOPRIGHT_BIT: u32 = 2;
pub const REF_NOT_AVAIL: i8 = -2;

pub const g_kuiCache30ScanIdx: [u8; 16] = [
    7, 8, 13, 14, 9, 10, 15, 16, 19, 20, 25, 26, 21, 22, 27, 28,
];

pub const I16_PRED_V: i8 = 0;
pub const I16_PRED_H: i8 = 1;
pub const I16_PRED_DC: i8 = 2;
pub const I16_PRED_P: i8 = 3;
pub const I16_PRED_DC_L: i8 = 4;
pub const I16_PRED_DC_T: i8 = 5;
pub const I16_PRED_DC_128: i8 = 6;
pub const I16_PRED_INVALID: i8 = -1;

pub const g_kiIntra16AvaliMode: [[i8; 5]; 8] = [
    [I16_PRED_DC_128, I16_PRED_INVALID, I16_PRED_INVALID, I16_PRED_INVALID, 1],
    [I16_PRED_DC_L, I16_PRED_H, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_DC_T, I16_PRED_V, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_INVALID, 3],
    [I16_PRED_DC_128, I16_PRED_INVALID, I16_PRED_INVALID, I16_PRED_INVALID, 1],
    [I16_PRED_DC_L, I16_PRED_H, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_DC_T, I16_PRED_V, I16_PRED_INVALID, I16_PRED_INVALID, 2],
    [I16_PRED_V, I16_PRED_H, I16_PRED_DC, I16_PRED_P, 4],
];

pub const g_kiMapModeI16x16: [i8; 7] = [0, 1, 2, 3, 2, 2, 2];

// Neighbor Availability Bitmasks
pub const LEFT_MB_POS: u8 = 0x01;
pub const TOP_MB_POS: u8 = 0x02;
pub const TOPRIGHT_MB_POS: u8 = 0x04;
pub const TOPLEFT_MB_POS: u8 = 0x08;

// Macroblock Types
pub type Mb_Type = u32;
pub const MB_TYPE_INTRA4x4: Mb_Type = 0x00000001;
pub const MB_TYPE_INTRA16x16: Mb_Type = 0x00000002;
pub const MB_TYPE_INTRA8x8: Mb_Type = 0x00000004;
pub const MB_TYPE_16x16: Mb_Type = 0x00000008;
pub const MB_TYPE_16x8: Mb_Type = 0x00000010;
pub const MB_TYPE_8x16: Mb_Type = 0x00000020;
pub const MB_TYPE_8x8: Mb_Type = 0x00000040;
pub const MB_TYPE_8x8_REF0: Mb_Type = 0x00000080;
pub const MB_TYPE_SKIP: Mb_Type = 0x00000100;
pub const MB_TYPE_INTRA_PCM: Mb_Type = 0x00000200;
pub const MB_TYPE_INTRA_BL: Mb_Type = 0x00000400;
pub const MB_TYPE_DIRECT: Mb_Type = 0x00000800;
pub const MB_TYPE_BACKGROUND: Mb_Type = 0x00010000;

pub const MB_TYPE_INTRA: Mb_Type =
    MB_TYPE_INTRA4x4 | MB_TYPE_INTRA16x16 | MB_TYPE_INTRA8x8 | MB_TYPE_INTRA_PCM;

// Sub-MB Types
pub const SUB_MB_TYPE_8x8: u8 = 0x01;
// D-dead-2 / F122: `SUB_MB_TYPE_8x4` (0x02), `_4x8` (0x04) and `_4x4` (0x08) are
// gone from the *encoder*. No encoder path ever assigned them. The decoder keeps its
// own copies — it must parse any conforming stream, whatever partitions the stream's
// encoder chose, and 50 references there say so.

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;

// Block Sizes for Cost Functions
pub const BLOCK_16x16: usize = 0;
pub const BLOCK_16x8: usize = 1;
pub const BLOCK_8x16: usize = 2;
pub const BLOCK_8x8: usize = 3;
pub const BLOCK_4x4: usize = 4;
pub const BLOCK_8x4: usize = 5;
pub const BLOCK_4x8: usize = 6;

// Reference Block 4x4 Scan Order Table
pub const g_kuiMbCountScan4Idx: [u8; 24] = [
    0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15, 16, 17, 20, 21, 18, 19, 22, 23,
];

// ============================================================================
// Enumerations & Callback Typedefs
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ESkipModes {
    STATIC = 0,
    SCROLLED = 1,
}


pub type pJudgeSkipFun = unsafe extern "C" fn(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    pWelsMd: &mut SWelsMD<'_>,
) -> bool;

// ============================================================================
// Core Structures Matching C/C++ Layout
// ============================================================================



// **D-dead-2's straggler — `SWelsMeContainers` deleted.** A second, field-identical
// declaration of `md.rs`'s `SWelsMD_sMe` with **zero references anywhere in the
// crate**: not constructed, not named in a signature, not re-exported. It survived
// because every encoder module carries `#![allow(dead_code)]` and every item is
// `pub`, so neither rustc's dead-code pass nor a warning could see it — which is the
// same blindfold that let F122's closure sit unnoticed for two sessions.




#[repr(C)]
#[derive(Copy, Clone)]
pub struct SSampleDealingPicData {
    pub pEncMb: [*mut u8; 3],
    pub pRefMb: [*mut u8; 3],
    pub pCsMb: [*mut u8; 3],
}

impl Default for SSampleDealingPicData {
    fn default() -> Self {
        Self {
            pEncMb: [std::ptr::null_mut(); 3],
            pRefMb: [std::ptr::null_mut(); 3],
            pCsMb: [std::ptr::null_mut(); 3],
        }
    }
}









// SCREEN_CONTENT(dormant: Phase 10) — `pVaa` is only ever an `SVAAFrameInfoExt`
// under screen content, which `RequestMemorySvc` refuses. **F213**: this module
// used to declare its own three-field `SVAAFrameInfoExt_t` for the same C type,
// and the two disagreed on layout — the canonical struct has
// `sComplexityScreenParam` between the base and `sScrollDetectInfo`, so every
// field the local twin named was read at the wrong offset. Both were dead, so
// nothing saw it. The twin is deleted; the canonical type is the one the
// context's `vaa_ext` accessor hands out.

// wels_func_ptr_def.h:127 takes uint8_t*, not const uint8_t*; this module's own
// alias had it const, which made it a distinct function type from the one the
// SSampleDealingFunc tables actually hold.
pub use crate::encoder::md::PSampleSadSatdCostFunc;

// `SSampleDealingFuncs` (trailing `s`) used to be declared here: a dead, truncated
// rename of the canonical `md::SSampleDealingFunc`. Removed — the canonical type is
// the one `SWelsFuncPtrList` embeds.


// ============================================================================
// Macro / Inline Condition Helpers
// ============================================================================

#[inline(always)]
pub fn IS_SKIP(uiMbType: Mb_Type) -> bool {
    (uiMbType & MB_TYPE_SKIP) != 0
}

#[inline(always)]
pub fn IS_INTRA(uiMbType: Mb_Type) -> bool {
    (uiMbType & MB_TYPE_INTRA) != 0
}

#[inline(always)]
pub fn IS_I_BL(uiMbType: Mb_Type) -> bool {
    uiMbType == MB_TYPE_INTRA_BL
}

#[inline(always)]
pub fn IS_SVC_INTRA(uiMbType: Mb_Type) -> bool {
    IS_I_BL(uiMbType) || IS_INTRA(uiMbType)
}

#[inline(always)]
pub fn WELS_CLIP3(iX: i32, iMin: i32, iMax: i32) -> i32 {
    if iX < iMin {
        iMin
    } else if iX > iMax {
        iMax
    } else {
        iX
    }
}

// ============================================================================
// External C Routine Declarations
// ============================================================================

/// `svc_base_layer_md.cpp:1924`.
///
/// Previously a stub that set `uiMbType = MB_TYPE_SKIP` (which the C++ does *not* do
/// here — its caller already has) and skipped the QP carry-over and the collocated
/// flag entirely, so every P_SKIP macroblock coded with a stale luma/chroma QP.
///
/// **Takes the context since T6.G3**, which the C++ signature does not
/// (`svc_base_layer_md.h:86`). The layer names its PPS by *position* now, and the
/// arrays it indexes live on the context, so resolving one needs both — this is the
/// only consumer of the family that did not already hold a context. It is a plain
/// function with two direct callers, not a dispatch-table slot, so widening it is not
/// 4b's fence: both callers pass the `pEncCtx` they already have, and each was
/// deriving `pCurDqLayer` from it one line earlier.
///
/// # Safety
/// All pointers must be valid and the layer's PPS position assigned.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdInterUpdatePskip(
    pEncCtx: &sWelsEncCtx,
    pCurDqLayer: &SDqLayer,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    //add pEnc&rec to MD--2010.3.15
    (*pCurMb).uiCbp = 0;
    (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
    let kiChromaQpIndexOffset = (*layer_pps(pEncCtx, pCurDqLayer)).uiChromaQpIndexOffset as i32;
    (*pCurMb).uiChromaQp = crate::encoder::svc_encode_slice::g_kuiChromaQpTable
        [WELS_CLIP3((*pCurMb).uiLumaQp as i32 + kiChromaQpIndexOffset, 0, 51) as usize];
    (*pMbCache).bCollocatedPredFlag = LD32_MV(&(*pCurMb).sMv[0]) == 0;
}

/// `LD32 (&pCurMb->sMv[0])` — one motion vector read as a 32-bit word. T6.C1 spells
/// the pun as the two halves it is, rather than reading a `u32` through the pair.
#[inline]
fn LD32_MV(pMv: &SMVUnitXY) -> u32 {
    let x = pMv.iMvX.to_ne_bytes();
    let y = pMv.iMvY.to_ne_bytes();
    u32::from_ne_bytes([x[0], x[1], y[0], y[1]])
}

/// `svc_base_layer_md.cpp:1906`. Tries the ordinary P_SKIP.
///
/// Previously a stub: it ran `PredictSadSkip` unconditionally and always returned
/// `false`, so no macroblock could ever be coded as P_SKIP.
///
/// # Safety
/// All pointers must be valid; `pEncCtx->pRefPic` must be assigned.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdInterJudgePskip(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    bTrySkip: bool,
) -> bool {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let bRet;
    if ((crate::encoder::svc_encode_slice::ctx_ref_pic(pEncCtx)
        .map_or(0, |p| p.iPictureType)
        == EWelsSliceType::P_SLICE as i32)
        && (pMbCache.uiRefMbType == MB_TYPE_SKIP || pMbCache.uiRefMbType == MB_TYPE_BACKGROUND))
        || bTrySkip
    {
        PredictSadSkip(
            &(*pMbCache).sMvComponents.iRefIndexCache,
            &(*pMbCache).bMbTypeSkip,
            &(*pMbCache).iSadCostSkip,
            0,
            &mut (*pWelsMd).iSadPredSkip,
        );
        bRet = crate::encoder::svc_base_layer_md::WelsMdPSkipEnc(pEncCtx, pWelsMd, pCurMb, &mut *pMbCache);
        return bRet;
    }

    false
}

/// `svc_base_layer_md.cpp:1954`. P_SKIP macroblock encode.
///
/// Previously omitted `WelsRecPskip`, so a skipped macroblock's motion-compensated
/// samples were never copied into the reconstruction.
///
/// # Safety
/// All four pointers must be valid.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdInterDecidedPskip(
    pEncCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
) {
    let pCurDqLayer = current_layer(pEncCtx);
    (*pCurMb).uiMbType = MB_TYPE_SKIP;
    WelsRecPskip(&*pCurDqLayer, (*pEncCtx).func_list(), pCurMb, &mut pSlice.sMbCacheInfo);
    WelsMdInterUpdatePskip(pEncCtx, &*pCurDqLayer, &mut *pSlice, pCurMb);
}

/// `svc_base_layer_md.cpp:1997`.
///
/// Previously a stub: it omitted `pfFirstIntraMode`, `pfSetScrollingMv`,
/// `pfInterFineMd`, `WelsMdInterMbRefinement` and `WelsMdInterDoubleCheckPskip`, and
/// inlined a partial `WelsMdInterEncode` that skipped `uiCbp = 0` and the three
/// `pfCopy*` writes back into the CS plane.
///
/// # Safety
/// All pointers must be valid and `pfFirstIntraMode`, `pfSetScrollingMv` and
/// `pfInterFineMd` assigned — `PreprocessSliceCoding` does this for a P slice.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdInterSecondaryModesEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    bSkip: bool,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pFuncList = (*pEncCtx).func_list();
    //step 2: Intra
    let kbTrySkip = (*pFuncList).pfFirstIntraMode.expect(
        "pfFirstIntraMode is unset; PreprocessSliceCoding must assign WelsMdFirstIntraMode \
         before any P macroblock is coded",
    )(pEncCtx, pWelsMd, pCurMb, &mut *pMbCache);
    if kbTrySkip {
        return;
    }

    if bSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb);
    } else {
        //Step 3: SubP16 MD
        // S4.C1: `vaa_ptr()` was the raw form of this reach; the slot takes the
        // shared reference now, which is the same `&self` retag `func_list()` above
        // already makes and which two workers may hold at once.
        (*pFuncList).pfSetScrollingMv.expect("pfSetScrollingMv is unset")(
            (*pEncCtx).vaa().expect("the frame's video-analysis block"),
            pWelsMd,
        ); //SCC
        (*pFuncList).pfInterFineMd.expect(
            "pfInterFineMd is unset; PreprocessSliceCoding must assign \
             WelsMdInterFinePartition[Vaa] before any P macroblock is coded",
        )(pEncCtx, pWelsMd, &mut *pSlice, pCurMb, (*pWelsMd).iCostLuma);

        //refinement for inter type
        crate::encoder::svc_base_layer_md::WelsMdInterMbRefinement(pEncCtx, pWelsMd, pCurMb, &mut pSlice.sMbCacheInfo);

        //step 7: invoke encoding
        crate::encoder::svc_base_layer_md::WelsMdInterEncode(pEncCtx, pSlice, pCurMb);

        //step 8: double check Pskip
        crate::encoder::svc_base_layer_md::WelsMdInterDoubleCheckPskip(pCurMb, &mut pSlice.sMbCacheInfo);
    }
}

/// `svc_base_layer_md.cpp:2023`. Runs the fine intra partition search through
/// `pfIntraFineMd`, reconstructs the luma if I16x16 survived, then decides and
/// reconstructs chroma.
///
/// Previously this was a stub that only zeroed `uiCbp` and `pSadCost[0]`: it never
/// called `pfIntraFineMd`, `WelsEncRecI16x16Y`, `WelsMdIntraChroma` or
/// `WelsIMbChromaEncode`, so no residual was ever produced for an intra macroblock.
///
/// # Safety
/// All four pointers must be valid, `pEncCtx->pFuncList->pfIntraFineMd` must be
/// assigned (`PreprocessSliceCoding` does this), and `WelsMdIntraInit` must have run
/// for this macroblock.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdIntraSecondaryModesEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    let pFunc = (*pEncCtx).func_list();
    //initial prediction memory for I_4x4
    (*pFunc).pfIntraFineMd.expect(
        "pfIntraFineMd is unset; PreprocessSliceCoding must assign \
         WelsMdIntraFinePartition[Vaa] before any macroblock is coded",
    )(pEncCtx, pWelsMd, pCurMb, pMbCache);

    //add pEnc&rec to MD--2010.3.15
    if IS_INTRA16x16((*pCurMb).uiMbType) {
        (*pCurMb).uiCbp = 0;
        crate::encoder::svc_encode_mb::WelsEncRecI16x16Y(pEncCtx, pCurMb, pMbCache);
    }

    //chroma
    (*pWelsMd).iCostChroma = crate::encoder::svc_base_layer_md::WelsMdIntraChroma(
        &*pFunc,
        &*(current_layer(pEncCtx)),
        pMbCache,
        (*pWelsMd).iLambda,
    );
    //add pEnc&rec to MD--2010.3.15
    crate::encoder::svc_encode_slice::WelsIMbChromaEncode(pEncCtx, pCurMb, pMbCache);
    (*pCurMb).uiChromPredMode = (*pMbCache).uiChmaI8x8Mode as u32;
    (*pCurMb).iSadCost = 0;
}

/// Reconstructs a **P_SKIP** macroblock by copying motion-compensated samples directly
/// to the reconstructed frame buffer and clearing non-zero coefficient counts.
///
/// Translated from `WelsRecPskip` in `codec/encoder/core/src/svc_encode_mb.cpp:315`.
///
/// # Safety
/// All pointers in `pCurLayer`, `pFuncList`, `pCurMb`, and `pMbCache` must be valid.
pub extern "C" fn WelsRecPskip(
    pCurLayer: &SDqLayer,
    _pFuncList: &SWelsFuncPtrList,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
) {
    // **T9.C7 — the seam's first consumer, and the biggest single one.**
    //
    // Was three `pfCopy*Aligned` slot calls onto `SPicData.pCsMb[i]`, a raw
    // cursor into the reconstruction plane. The destination is now the seam's
    // cursor at this macroblock's own origin, and the operand is the arena slice
    // it always was — `sSkipMb`'s luma 16x16 at stride 16 and its two chroma 8x8
    // at stride 8, exactly the `(pSrc, 16)` / `(pSrc + 256, 8)` / `(pSrc + 320, 8)`
    // triple the slots were handed.
    //
    // **The slots are bypassed rather than flipped** (F118's rule): the eight
    // `pfCopy*` entries are installed unconditionally by `WelsInitEncodingFuncs`
    // and constant after init, so a fixed-size site may call the kernel directly,
    // byte-identically. The table itself flips when its last raw reader goes.
    //
    // The three destination strides are gone from the call because the view
    // carries them: `plane(i).stride()` is `iCsStride[i]` — `WelsInitCurrentLayer`
    // stamps both from the same `SPicture::stride(i)`.
    let view = crate::encoder::svc_encode_slice::layer_rec_view(pCurLayer)
        .expect("the layer's reconstruction view is built for this frame");
    let (lx, ly) = (*pMbCache).SPicData.luma_origin();
    let (cx, cy) = (*pMbCache).SPicData.chroma_origin();
    let src = &(*pMbCache).sSkipMb;

    copy_block_to_view::<16>(&src[..256], 16, &view.plane(0).cursor(lx, ly), 16);
    copy_block_to_view::<8>(&src[256..320], 8, &view.plane(1).cursor(cx, cy), 8);
    copy_block_to_view::<8>(&src[320..384], 8, &view.plane(2).cursor(cx, cy), 8);
    // `WelsSetMemZero (pCurMb->pNonZeroCount, 24)` — the row is inline now.
    (*pCurMb).iNonZeroCount = [0; MB_LUMA_CHROMA_BLOCK4x4_NUM];
}

/// Copies the current/reference luma & chroma blocks for a background MB into the VAA
/// info so future-frame background comparisons stay in sync.
///
/// Translated from `VaaBackgroundMbDataUpdate` in
/// `codec/encoder/core/src/svc_base_layer_md.cpp:1341`.
#[inline(always)]
fn VaaBackgroundMbDataUpdate(
    pFunc: &SWelsFuncPtrList,
    // S4.C3: `*mut` -> `&`. Every read below is a field read or a raw plane cursor
    // taken *out of* a field; nothing writes the block. Shared is also the only
    // correct shape here — this body is fork-reachable, and F223's third rule makes
    // an exclusive reborrow of the one shared video-analysis block a write to the
    // race model whether or not anything is written through it.
    pVaaInfo: &crate::encoder::wels_preprocess::SVAAFrameInfo,
    pCurMb: &mut SMB,
) {
    // **S9.0c — F117's three sites, and they are raw no longer.**
    //
    // T9.B20 left these raw because "no gate exercises them"; session B4's `bg`
    // preset made that false (F126 plants a one-sample fault in this very luma copy
    // and fails all three clips), so the deferral's premise had expired.
    //
    // The byte offsets become sample coordinates, and they name the same addresses:
    // `kiOffsetY = ((iMbY * stride + iMbX) << 4)` expands to
    // `(iMbY << 4) * stride + (iMbX << 4)`, which is a cursor at `(iMbX << 4,
    // iMbY << 4)` anchored on the plane's padded origin — the very address
    // `pCurY`/`pRefY` held. Chroma is the same with `<< 3`.
    //
    // `pCur*` is the **destination**: the copy runs previous-source -> current-source
    // (F117), in-fork, into the picture the encoder is reading. That is exactly why
    // both views are `SharedPlane`-backed — writing through a cell is lawful where a
    // `&mut [u8]` into the plane would not be, and a `&[u8]` over it would race.
    let (Some(curView), Some(refView)) = (&(*pVaaInfo).pCurView, &(*pVaaInfo).pRefView) else {
        return;
    };
    let (lx, ly) = (((*pCurMb).iMbX as isize) << 4, ((*pCurMb).iMbY as isize) << 4);
    let (cx, cy) = (((*pCurMb).iMbX as isize) << 3, ((*pCurMb).iMbY as isize) << 3);

    if let Some(copy16) = pFunc.pfCopy16x16Aligned {
        copy16(&curView.plane(0).cursor(lx, ly), &refView.plane(0).cursor(lx, ly));
    }
    if let Some(copy8) = pFunc.pfCopy8x8Aligned {
        copy8(&curView.plane(1).cursor(cx, cy), &refView.plane(1).cursor(cx, cy));
        copy8(&curView.plane(2).cursor(cx, cy), &refView.plane(2).cursor(cx, cy));
    }
}

/// Encodes a background macroblock: motion-compensates it from the reference frame at
/// zero MV, then either reconstructs it as `P_SKIP` (`bSkipMbFlag`) or falls through to a
/// regular 16x16 inter encode.
///
/// Translated from `WelsMdBackgroundMbEnc` in
/// `codec/encoder/core/src/svc_base_layer_md.cpp:1352`.
///
/// # Safety
/// All pointers must be valid and non-null.
/// **Lit as of T9.B4 — the `bg` preset is this body's referee (D-ref-1, F126).** It
/// is reached only through `pfInterMdBackgroundDecision` = `WelsMdInterJudgeBGDPskip`,
/// which `WelsInitBGDFunc` installs only behind `bEnableBackgroundDetection`, and both
/// diffharness drivers pinned that `false` until B4 — which is what F117/T9.B27
/// measured as **0** entries across five sweep configurations. The new axis turns it
/// on: 1159-5771 entries per `bg` row, 0 in the same row with the flag off. A planted
/// one-sample fault after the luma motion compensation below fails **32 of the 48**
/// rows, so the conversions here are refereed rather than merely re-read.
///
/// The 16 rows it does *not* fail are `Static_152_100`'s, and they stay inert at
/// `+128` too: on that clip every background macroblock is `MB_TYPE_BACKGROUND`
/// P_SKIP with no residual, and `WelsMdInterJudgeBGDPskip`'s decision inputs come
/// from the analyzer's source-domain VAA planes, so the prediction never reaches the
/// bitstream *or* a decision. Quote 32, not 48, when this body's coverage is cited.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdBackgroundMbEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pSlice: &mut SSlice,
    bSkipMbFlag: bool,
) {
    // T9.E2c: a field borrow under the `&mut` parent (F112's one step); its
    // last use precedes the whole-slice passes to WelsInterMbEncode and
    // WelsPMbChromaEncode below, so NLL ends it in time.
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pCurDqLayer = current_layer(pEncCtx);
    let pFunc = (*pEncCtx).func_list();
    let sMvp = SMVUnitXY::default();

    // T9.B22's shape, at zero motion. The C++ addressed the reference through
    // `SPicData.pRefMb[i]`, which `WelsMdInterInit` (`:945`) stamps as
    // `data_ptr_shared(i) + ((mbX + mbY * stride) << 4)` for luma and `<< 3` for
    // chroma — the macroblock's own origin. The motion vector here is
    // `SMVUnitXY::default()`, so there is no excursion to add: the cursor is the same
    // sample the pointer named, said in samples instead of bytes, and
    // the reference picture's `stride(..)` is the stride the plane already carries.
    let kiMbXLuma = ((*pCurMb).iMbX as isize) << 4;
    let kiMbYLuma = ((*pCurMb).iMbY as isize) << 4;
    let kiMbXChroma = ((*pCurMb).iMbX as isize) << 3;
    let kiMbYChroma = ((*pCurMb).iMbY as isize) << 3;

    // The destination is one of two disjoint cache regions, chosen by the same flag
    // the C++ chose it by: `sSkipMb`'s three panes when the macroblock will be coded
    // as a background skip, `sMemPredMb`'s luma/chroma halves when it falls through to
    // the 16x16 inter encode. Both are plain arrays on `SMbCache`, so each is a slice,
    // and the halves' offsets are `md.rs`'s own `mem_pred_*_off` — unchanged
    // arithmetic, now bounds-checked.
    //
    // **Each cursor is built at its call and dropped at the end of that call's block**
    // (S29/F114a). That is not tidiness: the skip arm below runs
    // `VaaBackgroundMbDataUpdate`, which stays raw (F117, session C's) and writes the
    // *current source picture* through raw roots. A cursor into that picture held
    // across the call would be a live tag when a raw write lands under it; built and
    // dropped per kernel call, there is none. The reference-picture cursors here are
    // a different plane again, and read-only.

    // MC
    {
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        let cRefLuma = pRefPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        let mut cDstLuma = if bSkipMbFlag {
            let pSkipMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sSkipMb);
            PlaneCursorMut::new(&mut pSkipMb[..256], 0, 16)
        } else {
            let kiOff = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
            let pMemPredMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sMemPredMb);
            PlaneCursorMut::new(&mut pMemPredMb[kiOff..kiOff + 256], 0, 16)
        };
        mc_luma(&cRefLuma, &mut cDstLuma, 0, 0, 16, 16);
    }
    {
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        let cRefCb = pRefPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma);
        let mut cDstCb = if bSkipMbFlag {
            let pSkipMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sSkipMb);
            PlaneCursorMut::new(&mut pSkipMb[256..320], 0, 8)
        } else {
            let kiOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);
            let pMemPredMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sMemPredMb);
            PlaneCursorMut::new(&mut pMemPredMb[kiOff..kiOff + 64], 0, 8)
        };
        mc_chroma(&cRefCb, &mut cDstCb, sMvp.iMvX, sMvp.iMvY, 8, 8); // Cb
    }
    {
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        let cRefCr = pRefPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma);
        let mut cDstCr = if bSkipMbFlag {
            let pSkipMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sSkipMb);
            PlaneCursorMut::new(&mut pSkipMb[320..384], 0, 8)
        } else {
            let kiOff = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf) + 64;
            let pMemPredMb = &mut *std::ptr::addr_of_mut!((*pMbCache).sMemPredMb);
            PlaneCursorMut::new(&mut pMemPredMb[kiOff..kiOff + 64], 0, 8)
        };
        mc_chroma(&cRefCr, &mut cDstCr, sMvp.iMvX, sMvp.iMvY, 8, 8); // Cr
    }

    (*pCurMb).uiCbp = 0;
    (*pMbCache).bCollocatedPredFlag = true;
    (*pWelsMd).iCostLuma = 0; // BGD&RC integration
    // `pfSampleSadRaw[BLOCK_16x16]` is a compile-time index into a table with one
    // writer and no CPU flag (`WelsInitSampleSadFunc`), so the slot is constant from
    // the first frame on and `sample_sad::<16, 16>` *is* what it held — F118's order,
    // with no table on the path. Both operands are pictures the layer already holds:
    // the source through `layer_enc_pic` (the sample `SPicData.pEncMb[0]` named) and
    // the reference through `layer_ref_pic`, both at the macroblock's own origin.
    (*pCurMb).iSadCost = {
        let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        let cEncLuma = pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        let cRefLuma = pRefPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        sample_sad::<16, 16>(&cEncLuma, &cRefLuma)
    };
    (*pCurMb).sP16x16Mv = SMVUnitXY::default();
    layer_rec_view(&*pCurDqLayer)
        .expect("bound")
        .mv_list()
        .set((*pCurMb).iMbXY as usize, SMVUnitXY::default());

    if bSkipMbFlag {
        (*pCurMb).uiMbType = MB_TYPE_BACKGROUND;

        // update motion info to current MB
        (*pCurMb).iRefIndex = [0; MB_BLOCK8x8_NUM];
        if let Some(pfUpdateMbMv) = (*pFunc).pfUpdateMbMv {
            pfUpdateMbMv(&mut (*pCurMb).sMv, sMvp);
        }

        (*pCurMb).uiLumaQp = (*pSlice).uiLastMbQp;
        (*pCurMb).uiChromaQp = crate::encoder::svc_encode_slice::g_kuiChromaQpTable
            [crate::encoder::svc_encode_slice::CLIP3_QP_0_51(
                (*pCurMb).uiLumaQp as i32 + (*layer_pps(pEncCtx, pCurDqLayer)).uiChromaQpIndexOffset as i32,
            )];

        WelsRecPskip(&*pCurDqLayer, &*pFunc, pCurMb, &mut *pMbCache);
        VaaBackgroundMbDataUpdate(
            &*pFunc,
            (*pEncCtx).vaa().expect("the frame's video-analysis block"),
            pCurMb,
        );
        return;
    }

    (*pCurMb).uiMbType = MB_TYPE_16x16;

    (*pWelsMd).sMe.sMe16x16.sMv = SMVUnitXY::default();
    PredMv(
        &(*pMbCache).sMvComponents,
        0,
        4,
        (*pWelsMd).uiRef as i32,
        &mut (*pWelsMd).sMe.sMe16x16.sMvp,
    );
    (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;

    UpdateP16x16MotionInfo(
        &mut (*pMbCache).sMvComponents,
        pCurMb,
        (*pWelsMd).uiRef as i8,
        &mut (*pWelsMd).sMe.sMe16x16.sMv,
    );

    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        // `pfSampleSatd[BLOCK_16x16]`, constant after init (F118) — called direct, on
        // the same two picture cursors the SAD above used.
        let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        let cEncLuma = pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        let cRefLuma = pRefPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma);
        (*pWelsMd).iCostLuma = satd_16x16(&cEncLuma, &cRefLuma);
    }

    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(
        pEncCtx,
        &mut *pSlice,
        pCurMb,
    );

    // **T9.C2**, the `WelsRecPskip` triple again — see `WelsMdInterEncode` for the
    // shape and F118 for why the slots are bypassed rather than flipped. This
    // owner's referee is the narrow one: `WelsMdBackgroundMbEnc` is lit only by the
    // `bg` preset, and F126 measured its teeth at **32 of 48** `bg` rows, not 48 —
    // the other 16 light the family without refereeing it.
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

// ============================================================================
// Native Mode Decision & Motion Prediction Implementations
// ============================================================================

pub extern "C" fn PredMv(
    kpMvComp: &SMVComponentUnit,
    iPartIdx: i8,
    iPartW: i8,
    iRef: i32,
    sMvp: &mut SMVUnitXY,
) {
    let kuiLeftIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 1;
    let kuiTopIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 6;

    let iLeftRef = kpMvComp.iRefIndexCache[kuiLeftIdx] as i32;
    let iTopRef = kpMvComp.iRefIndexCache[kuiTopIdx] as i32;
    let iRightTopRef = kpMvComp.iRefIndexCache[kuiTopIdx + iPartW as usize] as i32;
    let iDiagonalRef: i32;

    let sMvA = kpMvComp.sMotionVectorCache[kuiLeftIdx];
    let sMvB = kpMvComp.sMotionVectorCache[kuiTopIdx];
    let sMvC: SMVUnitXY;

    if REF_NOT_AVAIL as i32 == iRightTopRef {
        iDiagonalRef = kpMvComp.iRefIndexCache[kuiTopIdx - 1] as i32;
        sMvC = kpMvComp.sMotionVectorCache[kuiTopIdx - 1];
    } else {
        iDiagonalRef = iRightTopRef;
        sMvC = kpMvComp.sMotionVectorCache[kuiTopIdx + iPartW as usize];
    }

    if (REF_NOT_AVAIL as i32 == iTopRef) && (REF_NOT_AVAIL as i32 == iDiagonalRef) && iLeftRef != REF_NOT_AVAIL as i32 {
        *sMvp = sMvA;
        return;
    }

    let mut iMatchRef = (if iRef == iLeftRef { 1 } else { 0 }) << MB_LEFT_BIT;
    iMatchRef |= (if iRef == iTopRef { 1 } else { 0 }) << MB_TOP_BIT;
    iMatchRef |= (if iRef == iDiagonalRef { 1 } else { 0 }) << MB_TOPRIGHT_BIT;

    match iMatchRef {
        1 => *sMvp = sMvA, // LEFT_MB_POS
        2 => *sMvp = sMvB, // TOP_MB_POS
        4 => *sMvp = sMvC, // TOPRIGHT_MB_POS
        _ => {
            (*sMvp).iMvX = WelsMedian(sMvA.iMvX as i32, sMvB.iMvX as i32, sMvC.iMvX as i32) as i16;
            (*sMvp).iMvY = WelsMedian(sMvA.iMvY as i32, sMvB.iMvY as i32, sMvC.iMvY as i32) as i16;
        }
    }
}

pub extern "C" fn PredSkipMv(kpMvComp: &SMVComponentUnit, sMvp: &mut SMVUnitXY) {
    let kiLeftRef = kpMvComp.iRefIndexCache[6] as i32;
    let kiTopRef = kpMvComp.iRefIndexCache[1] as i32;

    if REF_NOT_AVAIL as i32 == kiLeftRef
        || REF_NOT_AVAIL as i32 == kiTopRef
        || (0 == kiLeftRef && kpMvComp.sMotionVectorCache[6].iMvX == 0 && kpMvComp.sMotionVectorCache[6].iMvY == 0)
        || (0 == kiTopRef && kpMvComp.sMotionVectorCache[1].iMvX == 0 && kpMvComp.sMotionVectorCache[1].iMvY == 0)
    {
        *sMvp = SMVUnitXY { iMvX: 0, iMvY: 0 };
        return;
    }

    PredMv(kpMvComp, 0, 4, 0, sMvp);
}

pub extern "C" fn PredInter16x8Mv(kpMvComp: &SMVComponentUnit, iPartIdx: i32, iRef: i8, sMvp: &mut SMVUnitXY) {
    if 0 == iPartIdx {
        let kiTopRef = kpMvComp.iRefIndexCache[1];
        if iRef == kiTopRef {
            *sMvp = kpMvComp.sMotionVectorCache[1];
            return;
        }
    } else {
        let kiLeftRef = kpMvComp.iRefIndexCache[18];
        if iRef == kiLeftRef {
            *sMvp = kpMvComp.sMotionVectorCache[18];
            return;
        }
    }
    PredMv(kpMvComp, iPartIdx as i8, 4, iRef as i32, sMvp);
}

pub extern "C" fn PredInter8x16Mv(kpMvComp: &SMVComponentUnit, iPartIdx: i32, iRef: i8, sMvp: &mut SMVUnitXY) {
    if 0 == iPartIdx {
        let kiLeftRef = kpMvComp.iRefIndexCache[6];
        if iRef == kiLeftRef {
            *sMvp = kpMvComp.sMotionVectorCache[6];
            return;
        }
    } else {
        let mut iDiagonalRef = kpMvComp.iRefIndexCache[5];
        let mut iIndex = 5usize;
        if REF_NOT_AVAIL == iDiagonalRef {
            iDiagonalRef = kpMvComp.iRefIndexCache[2];
            iIndex = 2;
        }
        if iRef == iDiagonalRef {
            *sMvp = kpMvComp.sMotionVectorCache[iIndex];
            return;
        }
    }
    PredMv(kpMvComp, iPartIdx as i8, 2, iRef as i32, sMvp);
}

pub extern "C" fn UpdateP16x16MotionInfo(
    pMvComp: &mut SMVComponentUnit,
    pCurMb: &mut SMB,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    // The entry guard that stood here was the port's own — `mv_pred.cpp:148` opens
    // straight at `SMVComponentUnit* pMvComp = &pMbCache->sMvComponents;`. Two of its
    // three clauses were about parameters that are references now and cannot be null;
    // the third named `pMv`, which every caller spells `&mut …sMe.sMe16x16.sMv`.
    for i in 0..16 {
        pMvComp.iRefIndexCache[g_kuiCache30ScanIdx[i] as usize] = kiRef;
        pMvComp.sMotionVectorCache[g_kuiCache30ScanIdx[i] as usize] = *pMv;
    }
    // The two null guards that stood here were the port's own: the C++ writes both
    // rows unconditionally and `InitMbInfo` never left either pointer null. An inline
    // array cannot be absent.
    for i in 0..MB_BLOCK4x4_NUM {
        (*pCurMb).sMv[i] = *pMv;
    }
    (*pCurMb).iRefIndex = [kiRef; MB_BLOCK8x8_NUM];
}

// ============================================================================
// `codec/encoder/core/src/mv_pred.cpp:195-436` — motion info / cache updates
//
// The C++ writes these through `ST16`/`ST32`/`ST64` on `BUTTERFLY*`-replicated
// words. `BUTTERFLY1x2(b)` is `((b)<<8)|(b)` on an `int8_t` promoted to `int`, so
// for a negative reference index the two bytes are *not* equal — the high byte
// picks up the sign extension. `ST16`/`ST64` are therefore reproduced as raw
// unaligned stores of the same word rather than as element-wise assignment, so the
// transcription holds for any `kiRef`, not only the non-negative ones the encoder
// happens to pass.
// ============================================================================

/// `BUTTERFLY1x2` (`macros.h:275`) applied to a reference index, as C++ evaluates it:
/// `int8_t` -> `int` -> `|<<8` -> truncated to `uint16_t`.
#[inline]
fn butterfly1x2_ref(kiRef: i8) -> u16 {
    (((kiRef as i32) << 8) | (kiRef as i32)) as u16
}

/// `ST16 (&pMvComp->iRefIndexCache[k], kuiRef16)` — the same two bytes, written as
/// two bytes. **T9.D2**: the unaligned `u16` store was the transliteration, and the
/// sign extension `BUTTERFLY1x2` puts in the high byte is what it exists to carry;
/// `to_ne_bytes` carries it identically, and the neighbouring `pCurMb->pRefIndex`
/// store in `UpdateP16x8MotionInfo` was already spelled this way.
#[inline]
fn st16_ref_cache(pCache: &mut [i8; 30], k: usize, kuiRef16: u16) {
    let kaRef16 = kuiRef16.to_ne_bytes();
    pCache[k] = kaRef16[0] as i8;
    pCache[k + 1] = kaRef16[1] as i8;
}

/// `ST64 (&pMvComp->sMotionVectorCache[k], kuiMv64)`. `BUTTERFLY4x8` zero-extends the
/// 32-bit MV word, so the 64-bit store is exactly two copies of `*pMv`.
#[inline]
fn st64_mv(pCache: &mut [SMVUnitXY; 29], k: usize, mv: SMVUnitXY) {
    pCache[k] = mv;
    pCache[k + 1] = mv;
}

/// The same `ST64`, into the macroblock's own MV row — an inline array since T6.C1.
#[inline]
fn st64_mv_mb(sMv: &mut [SMVUnitXY; MB_BLOCK4x4_NUM], k: usize, mv: SMVUnitXY) {
    sMv[k] = mv;
    sMv[k + 1] = mv;
}

/// `mv_pred.cpp:195`. Updates ref index and MV in both `SMB` and the MB cache, P16x8.
///
/// `kiPartIdx` must be 0 or 8.
pub extern "C" fn UpdateP16x8MotionInfo(
    pMvComp: &mut SMVComponentUnit,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;
    let kuiRef16 = butterfly1x2_ref(kiRef);

    // ST16 (&pCurMb->pRefIndex[kiPartIdx >> 2], kuiRef16) — two bytes of one value.
    let kiBlk = (kiPartIdx >> 2) as usize;
    let kaRef16 = kuiRef16.to_ne_bytes();
    (*pCurMb).iRefIndex[kiBlk] = kaRef16[0] as i8;
    (*pCurMb).iRefIndex[kiBlk + 1] = kaRef16[1] as i8;
    // memcpy (&pCurMb->sMv[kiScan4Idx], uiMvBuf, sizeof (uint64_t[4])) — 8 MVs
    for i in 0..8 {
        (*pCurMb).sMv[kiScan4Idx + i] = *pMv;
    }

    let pRefCache = &mut pMvComp.iRefIndexCache;
    pRefCache[kiCacheIdx] = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 1, kuiRef16);
    pRefCache[kiCacheIdx + 3] = kiRef;
    pRefCache[kiCacheIdx + 6] = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 7, kuiRef16);
    pRefCache[kiCacheIdx + 9] = kiRef;

    let pMvCache = &mut pMvComp.sMotionVectorCache;
    pMvCache[kiCacheIdx] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 1, *pMv);
    pMvCache[kiCacheIdx + 3] = *pMv;
    pMvCache[kiCacheIdx + 6] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 7, *pMv);
    pMvCache[kiCacheIdx + 9] = *pMv;
}

/// `mv_pred.cpp:235`. The C++ really does spell this one in snake case; the name is
/// kept verbatim.
/// `kiPartIdx` must be 0 or 4.
pub extern "C" fn update_P8x16_motion_info(
    pMvComp: &mut SMVComponentUnit,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;
    let kiBlkIdx = (kiPartIdx >> 2) as usize;
    let kuiRef16 = butterfly1x2_ref(kiRef);

    (*pCurMb).iRefIndex[kiBlkIdx] = kiRef;
    (*pCurMb).iRefIndex[2 + kiBlkIdx] = kiRef;
    let pMbMv = &mut (*pCurMb).sMv;
    st64_mv_mb(pMbMv, kiScan4Idx, *pMv);
    st64_mv_mb(pMbMv, 4 + kiScan4Idx, *pMv);
    st64_mv_mb(pMbMv, 8 + kiScan4Idx, *pMv);
    st64_mv_mb(pMbMv, 12 + kiScan4Idx, *pMv);

    let pRefCache = &mut pMvComp.iRefIndexCache;
    pRefCache[kiCacheIdx] = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 1, kuiRef16);
    pRefCache[kiCacheIdx + 3] = kiRef;
    pRefCache[kiCacheIdx + 12] = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 13, kuiRef16);
    pRefCache[kiCacheIdx + 15] = kiRef;

    let pMvCache = &mut pMvComp.sMotionVectorCache;
    pMvCache[kiCacheIdx] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 1, *pMv);
    pMvCache[kiCacheIdx + 3] = *pMv;
    pMvCache[kiCacheIdx + 12] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 13, *pMv);
    pMvCache[kiCacheIdx + 15] = *pMv;
}

/// `mv_pred.cpp:279`. P8x8.
pub extern "C" fn UpdateP8x8MotionInfo(
    pMvComp: &mut SMVComponentUnit,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;

    let pMbMv = &mut (*pCurMb).sMv;
    st64_mv_mb(pMbMv, kiScan4Idx, *pMv);
    st64_mv_mb(pMbMv, 4 + kiScan4Idx, *pMv);

    let pRefCache = &mut pMvComp.iRefIndexCache;
    pRefCache[kiCacheIdx] = kiRef;
    pRefCache[kiCacheIdx + 1] = kiRef;
    pRefCache[kiCacheIdx + 6] = kiRef;
    pRefCache[kiCacheIdx + 7] = kiRef;

    let pMvCache = &mut pMvComp.sMotionVectorCache;
    pMvCache[kiCacheIdx] = *pMv;
    pMvCache[kiCacheIdx + 1] = *pMv;
    pMvCache[kiCacheIdx + 6] = *pMv;
    pMvCache[kiCacheIdx + 7] = *pMv;
}

// **D-dead-2 / F122 — the sub-8x8 motion-info updaters are gone.**
// `UpdateP4x4MotionInfo` / `UpdateP8x4MotionInfo` / `UpdateP4x8MotionInfo`
// (`mv_pred.cpp:305`/`:318`/`:334`) and their cache-only siblings
// `UpdateP4x4Motion2Cache` / `UpdateP8x4Motion2Cache` / `UpdateP4x8Motion2Cache`
// (`:407`/`:416`/`:427`) had, between them, three call sites in the port — all three
// inside `WelsMdInterMbRefinement`'s `SUB_MB_TYPE_4x4`/`_8x4`/`_4x8` arms, which this
// same commit deletes. The `Motion2Cache` trio had **none at all**: it survived on an
// unused `use` line. Upstream reaches the whole family only through
// `WelsMdInterFinePartitionVaaOnScreen`'s `#if 0 //Disable for sub8x8 modes for now`
// (`svc_mode_decision.cpp:634-661`) — the same block D-dead-1 deleted
// `WelsMdP4x4`/`WelsMdP8x4`/`WelsMdP4x8` for. Their 16x8/8x16/8x8 siblings above and
// below stay: those have live callers.
/// `mv_pred.cpp:353`. Cache-only update for P16x8.
pub extern "C" fn UpdateP16x8Motion2Cache(
    pMvComp: &mut SMVComponentUnit,
    mut iPartIdx: i32,
    iRef: i8,
    pMv: &mut SMVUnitXY,
) {
    for _ in 0..2 {
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        for k in [0usize, 1, 6, 7] {
            pMvComp.iRefIndexCache[kuiCacheIdx + k] = iRef;
            pMvComp.sMotionVectorCache[kuiCacheIdx + k] = *pMv;
        }
        iPartIdx += 4;
    }
}

/// `mv_pred.cpp:372`. Cache-only update for P8x16.
pub extern "C" fn UpdateP8x16Motion2Cache(
    pMvComp: &mut SMVComponentUnit,
    mut iPartIdx: i32,
    iRef: i8,
    pMv: &mut SMVUnitXY,
) {
    for _ in 0..2 {
        let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
        for k in [0usize, 1, 6, 7] {
            pMvComp.iRefIndexCache[kuiCacheIdx + k] = iRef;
            pMvComp.sMotionVectorCache[kuiCacheIdx + k] = *pMv;
        }
        iPartIdx += 8;
    }
}

/// `mv_pred.cpp:392`. Cache-only update for P8x8.
pub extern "C" fn UpdateP8x8Motion2Cache(
    pMvComp: &mut SMVComponentUnit,
    iPartIdx: i32,
    pRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    for k in [0usize, 1, 6, 7] {
        pMvComp.iRefIndexCache[kuiCacheIdx + k] = pRef;
        pMvComp.sMotionVectorCache[kuiCacheIdx + k] = *pMv;
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdI16x16(
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: Option<&SDqLayer>,
    pMbCache: &mut SMbCache,
    iLambda: i32,
) -> i32 {
    // T6.I2: `pFunc.is_null()` was the first arm; the table is a `&` now. **T9.D7**
    // dropped `pMbCache.is_null()` the same way — the arena is one owned field of the
    // slice, every caller reaches it as `&mut (*pSlice).sMbCacheInfo`, and a reference
    // cannot be absent. **S6.A1**: `pCurDqLayer` is the layer family's `Option` form —
    // the guard below is the same question the null check asked, kept in the callee.
    let Some(pCurDqLayer) = pCurDqLayer else {
        return i32::MAX;
    };
    // `svc_base_layer_md.cpp:369` reads pMemPredMb, not pMemPredLuma. The two are
    // equal on entry only because WelsMdIntraInit re-points pMemPredLuma at
    // pMemPredMb; this function then *moves* pMemPredLuma to the losing ping-pong
    // half before returning, so reading pMemPredLuma here would follow the previous
    // macroblock's pointer whenever WelsMdIntraInit had not just run.
    // **T9.C2**: the two-pointer ping-pong `pPredI16x16` / `pDst` carried exactly
    // one bit — which 256-byte half of `sMemPredMb` the search last wrote — and
    // `iIdx` already *is* that bit, as the tail of this function has always said.
    // With the destination an offset, the pointers have nothing left to carry.
    let pEnc = (*pMbCache).SPicData.mb_cursor(&(*pCurDqLayer).pEncData, &(*pCurDqLayer).iEncStride, 0);
    let view = layer_rec_view(pCurDqLayer)
        .expect("the layer's reconstruction view is built for this frame");
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let mut iBestMode;
    let mut iBestCost = i32::MAX;
    let mut iIdx = 0usize;

    let iOffset = ((*pMbCache).uiNeighborIntra & 0x07) as usize;
    let iAvailCount = g_kiIntra16AvaliMode[iOffset][4] as usize;
    let kpAvailMode = &g_kiIntra16AvaliMode[iOffset];

    // The `pfIntra16x16Combined3` fast path is not translated (see the module docs
    // on `svc_base_layer_md.rs`): NULL in the C++ on every target this port builds
    // for, never assigned here, and the slot itself is deleted (S18). The scalar
    // cost below is the only branch.
    // `svc_base_layer_md.cpp:402` costs with pfMdCost, which SetFastCodingFunc points
    // at pfSampleSad and SetNormalCodingFunc at pfSampleSatd. Hardcoding pfSampleSad
    // here silently forced the fast-mode choice in normal mode.
    let pfMdCost16x16 = pFunc.sSampleDealingFuncs.md_cost(BLOCK_16x16).unwrap();
    // **T9.B30**: the source macroblock by coordinate — this function has neither an
    // `SMB` nor a slice in scope, which is what the carrier's `iMbX`/`iMbY` are for.
    let pEncPicture = crate::encoder::svc_encode_slice::layer_enc_pic(pCurDqLayer)
        .expect("the layer's source picture is bound");
    let (kiMbOrgX, kiMbOrgY) = (*pMbCache).SPicData.luma_origin();

    iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        // **T9.C2 — the last of the intra-pred read sites.** `pDst` was one of the
        // two 256-byte halves of `sMemPredMb` and `pDec` the reconstruction luma
        // plane's raw root; the half is `iIdx * 256` as an offset, and the plane is
        // the seam's view at this macroblock's origin. Both operands are safe now,
        // so the F114a dance — raw at the call, shared borrow only afterwards — has
        // nothing left to arbitrate. Slot flipped rather than bypassed: the mode
        // index is a runtime value.
        let kiDstOff = iIdx * 256;
        pFunc.pfGetLumaI16x16Pred[iCurMode as usize].unwrap()(
            (&mut (*pMbCache).sMemPredMb[kiDstOff..kiDstOff + 256])
                .try_into()
                .expect("a packed 16x16 prediction block is 256 bytes"),
            &view.plane(0).cursor(kiMbOrgX, kiMbOrgY),
        );
        let mut iCurCost = pfMdCost16x16(
            &PlaneCursor::new(&(*pMbCache).sMemPredMb[kiDstOff..][..256], 0, 16),
            &pEncPicture.plane(0).cursor(kiMbOrgX, kiMbOrgY),
        );
        let mode_val = g_kiMapModeI16x16[iCurMode as usize] as u32;
        iCurCost += iLambda * (BsSizeUE(mode_val) as i32);
        if iCurCost < iBestCost {
            iBestMode = iCurMode;
            iBestCost = iCurCost;
            iIdx ^= 0x01;
        }
    }
    // The two pointers carried one bit between them and the selector *is* that bit:
    // chroma keeps the half the search last wrote (`iIdx`), luma takes the other.
    (*pMbCache).uiMemPredLumaHalf = (iIdx ^ 0x01) as u8;
    (*pMbCache).uiLumaI16x16Mode = iBestMode as u8;
    iBestCost
}

/// `svc_base_layer_md.cpp:964`, `static inline` in C++ so it is inlined here as a
/// private helper rather than exported.
///
/// Takes the three `SWelsMD` fields it reads rather than `&SWelsMD` (the C++ takes
/// `const SWelsMD&`): every `sWelsMe` a caller passes lives *inside* that same
/// `SWelsMD` (`sMe.sMe16x16`, `sMe.sMe16x8[i]`, ...), and a shared reference to the
/// whole struct is a promise the callee breaks the moment it writes the search
/// block — Miri's protector on the argument says so (the encode probe's seventh
/// red, Phase 6 session B). Take what you reach.
///
/// # Safety
/// `sWelsMe` must be valid; `pEnc`/`pRef` must point into the encode and reference
/// planes for this partition.
/// Session F: the `pEnc`/`pRef` cursor arguments and the three `SWelsME`
/// cursor stores are gone — the coordinates this function already stamps are
/// the same information (the verified identity), and the search family
/// receives the planes as parameters.
#[inline]
pub(crate) fn InitMe<'a>(
    iMbPixX: i32,
    iMbPixY: i32,
    pMvdCost: MvdCostCursor<'a>,
    iBlockSize: i32,
    // SCREEN_CONTENT(dormant: Phase 10)
    pRefFeatureStorage: Option<&'a SScreenBlockFeatureStorage>,
    sWelsMe: &mut SWelsME<'a>,
) {
    sWelsMe.iCurMeBlockPixX = iMbPixX;
    sWelsMe.iCurMeBlockPixY = iMbPixY;
    sWelsMe.uiBlockSize = iBlockSize as u8;
    sWelsMe.pMvdCost = pMvdCost;

    sWelsMe.pRefFeatureStorage = pRefFeatureStorage;
}

// unsafe-cat: fork-shared(S63) — the layer/SMB cursors (E3's grid); the
// dispatch cursor this tag used to name is a shared reference since T9.F4
#[allow(unsafe_code)]
pub unsafe fn WelsMdP16x16<'a>(
    pFunc: &SWelsFuncPtrList,
    pCurLayer: &'a SDqLayer,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
) -> i32 {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pMe16x16 = &mut (*pWelsMd).sMe.sMe16x16;
    let uiNeighborAvail = mbs.cur().uiNeighborAvail as u32;
    let kiMbWidth: i32 = (*pCurLayer).iMbWidth as i32;
    let kiMbHeight: i32 = (*pCurLayer).iMbHeight as i32;
    // `svc_base_layer_md.cpp:983`. This call was missing: without it the search block
    // kept the previous macroblock's coordinates/uiBlockSize/pMvdCost.
    InitMe(
        (*pWelsMd).iMbPixX,
        (*pWelsMd).iMbPixY,
        (*pWelsMd).pMvdCost,
        BLOCK_16x16 as i32,
        crate::encoder::svc_encode_slice::layer_ref_feature_storage(pCurLayer),
        pMe16x16,
    );
    //not putting the line below into InitMe to avoid judging mode in InitMe
    (*pMe16x16).uSadPredISatd.uiSadPred = (*pWelsMd).iSadPredMb as u32;

    (*pSlice).uiMvcNum = 0;
    (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*pMe16x16).sMvBase;
    (*pSlice).uiMvcNum += 1;

    if (uiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = mbs.left().sP16x16Mv;
        (*pSlice).uiMvcNum += 1;
    }
    if (uiNeighborAvail & TOP_MB_POS as u32) != 0 {
        (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = mbs.top().sP16x16Mv;
        (*pSlice).uiMvcNum += 1;
    }

    if layer_ref_pic(pCurLayer).map_or(false, |p| p.iPictureType == P_SLICE) {
        if (mbs.cur().iMbX as i32) < kiMbWidth - 1 {
            let sTempMv =
                layer_ref_pic(pCurLayer).expect("bound").sMvList[(mbs.cur().iMbXY + 1) as usize];
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
        if (mbs.cur().iMbY as i32) < kiMbHeight - 1 {
            let sTempMv = layer_ref_pic(pCurLayer).expect("bound").sMvList
                [(mbs.cur().iMbXY + kiMbWidth) as usize];
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
    }

    PredMv(
        &(*pMbCache).sMvComponents,
        0,
        4,
        0,
        &mut (*pMe16x16).sMvp,
    );

    if let Some(search_fn) = pFunc.pfMotionSearch[0] {
        // The de-virtualized slot takes what it reaches (the ME group + the
        // cost tables — both shared reads of the pre-fork table) and the two
        // planes, resolved per call through the layer's frame-stable handles
        // (S37's value half; the pattern MeRefineFracPixel proved).
        let pEncPicture = layer_enc_pic(pCurLayer).expect("the layer's source picture is bound");
        let pRefPicture = layer_ref_pic(pCurLayer).expect("the layer's reference picture is bound");
        search_fn(
            &pFunc.sMeFuncs,
            &pFunc.sSampleDealingFuncs,
            pMe16x16,
            &mut *pSlice,
            pEncPicture.plane(0),
            pRefPicture.plane(0),
        );
    }

    mbs.cur_mut().sP16x16Mv = (*pMe16x16).sMv;
    // `is_empty()` is the port's spelling of the C++'s null test: a picture built
    // without `bNeedMbInfo` carries no MV list at all (T6.F0). One view where
    // there were two picture borrows: the test and the write now read the same
    // capture rather than resolving the pool twice.
    let sMvList = layer_rec_view(pCurLayer).expect("bound").mv_list();
    if !sMvList.is_empty() {
        sMvList.set(mbs.cur().iMbXY as usize, (*pMe16x16).sMv);
    }

    (*pMe16x16).uiSatdCost as i32
}

// unsafe-cat: fork-shared(S63) — the layer/SMB cursors (E3's grid); the
// dispatch cursor this tag used to name is a shared reference since T9.F4
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdP8x8<'a>(
    pFunc: &SWelsFuncPtrList,
    pCurDqLayer: &'a SDqLayer,
    pWelsMd: &mut SWelsMD<'a>,
    pSlice: &mut SSlice,
) -> i32 {
    let mut iCostP8x8 = 0i32;
    for i in 0..4 {
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let iIdxX = i & 1;
        let iIdxY = i >> 1;
        let iPixelX = iIdxX << 3;
        let iPixelY = iIdxY << 3;

        let sMe8x8 = &mut (*pWelsMd).sMe.sMe8x8[i as usize];
        // `svc_base_layer_md.cpp:1096`. The InitMe call, the two block-pixel offsets,
        // the SAD predictor, the sMvc seed, the static-idc-selected search function
        // and the cache update were all missing.
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_8x8 as i32,
            crate::encoder::svc_encode_slice::layer_ref_feature_storage(pCurDqLayer),
            sMe8x8,
        );
        //not putting these three lines below into InitMe to avoid judging mode in InitMe
        (*sMe8x8).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe8x8).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe8x8).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 2) as u32;

        (*pSlice).sMvc[0] = (*sMe8x8).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredMv(
            &(*pMbCache).sMvComponents,
            (i << 2) as i8,
            2,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe8x8).sMvp,
        );

        {
            // The runtime index selects among four identical entries today:
            // the only writer of `pfMotionSearch` is `PreprocessSliceCoding`'s
            // loop, which installs `WelsMotionEstimateSearch` in every slot
            // (the C++'s SCREEN_CONTENT block that would install the
            // static/scrolled variants is untranslated), and the only nonzero
            // writer of `iBlock8x8StaticIdc` is `SetBlockStaticIdcToMd`,
            // SCREEN_CONTENT(dormant). Both locks are stated so the dispatch
            // stays honest when Phase 10 lights them.
            let pEncPicture = layer_enc_pic(pCurDqLayer).expect("the layer's source picture is bound");
            let pRefPicture = layer_ref_pic(pCurDqLayer).expect("the layer's reference picture is bound");
            pFunc.pfMotionSearch[(*pWelsMd).iBlock8x8StaticIdc[i as usize] as usize]
                .expect("pfMotionSearch unset")(
                &pFunc.sMeFuncs,
                &pFunc.sSampleDealingFuncs,
                sMe8x8,
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
        UpdateP8x8Motion2Cache(
            &mut (*pMbCache).sMvComponents,
            i << 2,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe8x8).sMv,
        );
        iCostP8x8 += (*sMe8x8).uiSatdCost as i32;
    }
    iCostP8x8
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsInterMbEncode(pEncCtx: &sWelsEncCtx, pSlice: &mut SSlice, pCurMb: &mut SMB) {
    // Port-added guard deleted with the retyping: `svc_encode_slice.cpp:458` opens at
    // `SMbCache* pMbCache = &pSlice->sMbCacheInfo;` and checks nothing.
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pCurDqLayer = current_layer(pEncCtx);
    let pFuncList = (*pEncCtx).func_list();
    // T6.I1: the `|| pFuncList.is_null()` arm went with the raw table.
    if pCurDqLayer.is_null() {
        return;
    }

    // S9.0: this is `WelsDctMb`'s body inlined, and it converts the same way — the
    // four quadrants named in samples rather than in bytes-times-stride. The
    // prediction scratch is stride 16, so its old `+8 / +128 / +136` are
    // `(8,0) / (0,8) / (8,8)`.
    //
    // The three `is_null()` guards went with the raws. All three pointers came from
    // `addr_of_mut!` on owned fields or from a plane root that had already been
    // null-checked through `pCurDqLayer` above, so none of them could ever be null.
    let encView = crate::encoder::svc_encode_slice::layer_enc_view(&*pCurDqLayer)
        .expect("the frame's source view is stamped with pEncData");
    let pEncMb = (*pMbCache).SPicData.mb_cursor_ro(encView, 0);
    let pMemPredLuma = PlaneCursor::new(
        &(*pMbCache).sMemPredMb,
        mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf),
        16,
    );

    if let Some(dct_fn) = (*pFuncList).pfDctFourT4 {
        for (k, (dx, dy)) in [(0isize, 0isize), (8, 0), (0, 8), (8, 8)].into_iter().enumerate() {
            dct_fn(
                &mut (*pMbCache).sCoeffLevel[k << 6..],
                &pEncMb.advance(dx, dy),
                &pMemPredLuma.advance(dx, dy),
            );
        }
    }

    WelsEncInterY(&*pFuncList, pCurMb, &mut *pMbCache);
}

// ============================================================================
// 1. Spatial Enhancement Layer Mode Decision (ILFMD / NoILP)
// ============================================================================

/// Retrieves the collocated base-layer reference macroblock in dyadic SVC downsampling.
///
/// **Takes the context rather than the layer since T6.D3**: `pRefLayer` is a
/// position in `ppDqLayerList` now, so resolving it is one lookup in the list and
/// the list is reachable only through the context. The base layer is a *different*
/// `SDqLayer` than `pCurDqLayer`, which is why this reads through the list rather
/// than through the current layer.
#[inline(always)]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn GetRefMb(pEncCtx: &sWelsEncCtx, pCurMb: &SMB) -> SMB {
    let kRefIdx = (*current_layer(pEncCtx))
        .pRefLayer
        .expect("GetRefMb on a layer with no base layer: bBaseLayerAvailableFlag gates every caller");
    let kpRefLayer = ctx_dq_layer(pEncCtx, kRefIdx.get());
    let kiRefMbIdx =
        ((pCurMb.iMbY as i32 >> 1) * (*kpRefLayer).iMbWidth as i32) + (pCurMb.iMbX as i32 >> 1);
    // The base layer is quiescent by the time its enhancement layer encodes
    // (the fork joins per layer), so a shared read of its record — through the
    // MbArray's own checked indexing — is the whole access, copied out (SMB is
    // Copy). The raw hand-out this replaced carried the array's provenance for
    // a walk nobody performed.
    //
    // **D-fid-3 (the user, 2026-08-26): the index is CLAMPED to the base layer's
    // last record, and that is a deliberate divergence from upstream.** The
    // `>> 1` pair above is only an address when the base layer really is half
    // size on both axes, which is what upstream's own comment at
    // `svc_mode_decision.cpp:125` asserts and never checks:
    //
    // ```cpp
    // const int32_t kiRefMbIdx = (pCurMb->iMbY >> 1) * kpRefLayer->iMbWidth + (pCurMb->iMbX >> 1);
    //   //because current lower layer is half size on both vertical and horizontal
    // return (&kpRefLayer->sMbDataP[kiRefMbIdx]);
    // ```
    //
    // Simulcast can break the invariant — `EncodeDecodeTestAPI.SimulcastAVC_SPS_PPS_LISTING`
    // halves layer 0's dimensions alone, leaving a pair that is not 2:1 — and
    // then upstream indexes past `sMbDataP` and returns whatever follows the
    // allocation, while the port's checked read aborted the process
    // (panic-in-nounwind through the C ABI). F173: the gtest suite has not
    // tallied since session E3 because of it.
    //
    // Clamping is byte-identical wherever the invariant holds, because there
    // the index is already in bounds and `min` is the identity; where it does
    // not hold, upstream reads out of bounds and this reads a real record.
    // Neither is *correct* — the mode decision below is seeded from a base-layer
    // macroblock that does not collocate with this one either way — but one is
    // defined and the other is not, and only the defined one lets the suite run.
    let ref_mbs = (*std::ptr::addr_of!((*kpRefLayer).sMbDataP)).dims().count();
    // A base layer with no macroblocks at all cannot be a reference layer (it
    // would have no reconstruction to predict from), so this leaves the checked
    // read to fail loudly rather than inventing a record for it.
    let kiClampedIdx = (kiRefMbIdx as usize).min(ref_mbs.saturating_sub(1));
    *(*std::ptr::addr_of!((*kpRefLayer).sMbDataP)).get(kiClampedIdx)
}

/// Scales base-layer motion vectors by 2x to initialize enhancement-layer candidates.
pub fn SetMvBaseEnhancelayer(
    pMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    kpRefMb: &SMB,
) {
    let kuiRefMbType = kpRefMb.uiMbType;

    if !IS_SVC_INTRA(kuiRefMbType) {
        let iRefMbPartIdx =
            (((pCurMb.iMbY as i32 & 0x01) << 1) + (pCurMb.iMbX as i32 & 0x01)) as usize;
        let iScan4RefPartIdx = g_kuiMbCountScan4Idx[iRefMbPartIdx << 2] as isize;

        let ref_mv = kpRefMb.sMv[(iScan4RefPartIdx) as usize];
        let sMv = SMVUnitXY {
            iMvX: ref_mv.iMvX * 2,
            iMvY: ref_mv.iMvY * 2,
        };

        (*pMd).sMe.sMe16x16.sMvBase = sMv;
        (*pMd).sMe.sMe8x8[0].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[1].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[2].sMvBase = sMv;
        (*pMd).sMe.sMe8x8[3].sMvBase = sMv;

        (*pMd).sMe.sMe16x8[0].sMvBase = sMv;
        (*pMd).sMe.sMe16x8[1].sMvBase = sMv;
        (*pMd).sMe.sMe8x16[0].sMvBase = sMv;
        (*pMd).sMe.sMe8x16[1].sMvBase = sMv;
    }
}

/// Core spatial enhancement layer mode decision without Inter-Layer Prediction.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdSpatialelInterMbIlfmdNoilp(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    kuiRefMbType: Mb_Type,
) {
    let pCurDqLayer = current_layer(pEncCtx);
    let kuiNeighborAvail = mbs.cur().uiNeighborAvail as u32;

    let kbMbLeftAvailPskip = if (kuiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        IS_SKIP(mbs.left().uiMbType)
    } else {
        false
    };
    let kbMbTopAvailPskip = if (kuiNeighborAvail & TOP_MB_POS as u32) != 0 {
        IS_SKIP(mbs.top().uiMbType)
    } else {
        false
    };
    let kbMbTopLeftAvailPskip = if (kuiNeighborAvail & TOPLEFT_MB_POS as u32) != 0 {
        IS_SKIP(mbs.top_left().uiMbType)
    } else {
        false
    };
    let kbMbTopRightAvailPskip = if (kuiNeighborAvail & TOPRIGHT_MB_POS as u32) != 0 {
        IS_SKIP(mbs.top_right().uiMbType)
    } else {
        false
    };

    let bTrySkip =
        kbMbLeftAvailPskip | kbMbTopAvailPskip | kbMbTopLeftAvailPskip | kbMbTopRightAvailPskip;
    let mut bKeepSkip = kbMbLeftAvailPskip & kbMbTopAvailPskip & kbMbTopRightAvailPskip;
    let bSkip: bool;

    if let Some(pfBgd) = (*pEncCtx).func_list().pfInterMdBackgroundDecision {
        if pfBgd(pEncCtx, pWelsMd, &mut *pSlice, mbs.cur_mut(), &mut bKeepSkip) {
            return;
        }
    }

    // Step 1: Try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bTrySkip);

    if bSkip && bKeepSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, mbs.cur_mut());
        return;
    }

    if !IS_SVC_INTRA(kuiRefMbType) {
        if !bSkip {
            let pMbCache = &mut pSlice.sMbCacheInfo;
            PredictSad(
                &pMbCache.sMvComponents.iRefIndexCache,
                &pMbCache.iSadCost,
                0,
                &mut (*pWelsMd).iSadPredMb,
            );

            // Step 2: P_16x16
            (*pWelsMd).iCostLuma =
                WelsMdP16x16((*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice, mbs);
            mbs.cur_mut().uiMbType = MB_TYPE_16x16;
        }

        WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, mbs.cur_mut(), bSkip);
    } else {
        // Base layer is Intra (BLMODE == SVC_INTRA)
        let pMbCache = &mut pSlice.sMbCacheInfo;
        let kiCostI16x16 = WelsMdI16x16(
            (*pEncCtx).func_list(),
            (current_layer(pEncCtx)).as_ref(),
            &mut *pMbCache,
            (*pWelsMd).iLambda,
        );
        if bSkip && ((*pWelsMd).iCostLuma <= kiCostI16x16) {
            WelsMdInterDecidedPskip(pEncCtx, pSlice, mbs.cur_mut());
        } else {
            (*pWelsMd).iCostLuma = kiCostI16x16;
            mbs.cur_mut().uiMbType = MB_TYPE_INTRA16x16;

            WelsMdIntraSecondaryModesEnc(pEncCtx, pWelsMd, mbs.cur_mut(), &mut pSlice.sMbCacheInfo);
        }
    }
}

/// Top-level MD entry point for spatial enhancement layer inter MBs.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterMbEnhancelayer(
    pEncCtx: &sWelsEncCtx,
    pMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
) {
    let kInterLayerRefMb = GetRefMb(pEncCtx, mbs.cur());
    let kuiInterLayerRefMbType = kInterLayerRefMb.uiMbType;

    SetMvBaseEnhancelayer(pMd, mbs.cur_mut(), &kInterLayerRefMb);
    WelsMdSpatialelInterMbIlfmdNoilp(pEncCtx, pMd, pSlice, mbs, kuiInterLayerRefMbType);
}

// ============================================================================
// 2. Background Detection (BGD) P-Skip Mode Decision & Chroma Verification
// ============================================================================

#[inline(always)]
/// `svc_mode_decision.cpp:161`. Every pointer is non-const in C++; the port
/// passes the selected safe cost slot and the two chroma cursors (session F —
/// the last interior pointer into the cost tables went with the raw triple).
pub fn GetChromaCost(
    pSad: Option<crate::encoder::md::PSampleSadSatdCostFunc>,
    cSrcChroma: &crate::safe::plane::PlaneCursor<'_>,
    cRefChroma: &crate::safe::plane::PlaneCursor<'_>,
) -> i32 {
    if let Some(f) = pSad {
        f(cSrcChroma, cRefChroma)
    } else {
        0
    }
}

#[inline(always)]
pub fn IsCostLessEqualSkipCost(
    iCurCost: i32,
    iPredPskipSad: i32,
    iRefMbType: Mb_Type,
    pRef: Option<&SPicture>,
    iMbXy: i32,
    iSmallestInvisibleTh: i32,
) -> bool {
    (iPredPskipSad > iSmallestInvisibleTh && iCurCost >= iPredPskipSad)
        || pRef.map_or(false, |pRef| {
            pRef.iPictureType == P_SLICE
                && iRefMbType == MB_TYPE_SKIP
                && pRef.pMbSkipSad[iMbXy as usize] > iSmallestInvisibleTh
                && iCurCost >= pRef.pMbSkipSad[iMbXy as usize]
        })
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn CheckChromaCost(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pMbCache: &mut SMbCache,
    iCurMbXy: i32,
) -> bool {
    // T9.E7's `addr_of!` interior pointer retired with the raw table (session
    // F): a plain place-projection copy of the safe slot reads without any
    // autoref, and the chroma positions come from the carrier coordinates —
    // the stamped `SPicData.pEncMb[1]`/`pRefMb[1]` were exactly
    // plane-root + ((iMbX + iMbY*stride) << 3), T9.B30's identity.
    let pSad = (*pEncCtx).func_list().sSampleDealingFuncs.pfSampleSad[BLOCK_8x8];
    let pCurDqLayer = current_layer(pEncCtx);

    let kiMbXChroma = ((*pMbCache).SPicData.iMbX as isize) << 3;
    let kiMbYChroma = ((*pMbCache).SPicData.iMbY as isize) << 3;
    let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
    let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");

    let iCbSad = GetChromaCost(
        pSad,
        &pEncPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma),
        &pRefPicture.plane(1).cursor(kiMbXChroma, kiMbYChroma),
    );
    let iCrSad = GetChromaCost(
        pSad,
        &pEncPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma),
        &pRefPicture.plane(2).cursor(kiMbXChroma, kiMbYChroma),
    );

    let bChromaTooLarge = iCbSad > KNOWN_CHROMA_TOO_LARGE || iCrSad > KNOWN_CHROMA_TOO_LARGE;
    let iChromaSad = iCbSad + iCrSad;

    PredictSadSkip(
        &(*pMbCache).sMvComponents.iRefIndexCache,
        &(*pMbCache).bMbTypeSkip,
        &(*pMbCache).iSadCostSkip,
        0,
        &mut (*pWelsMd).iSadPredSkip,
    );

    let bChromaCostCannotSkip = IsCostLessEqualSkipCost(
        iChromaSad,
        (*pWelsMd).iSadPredSkip,
        (*pMbCache).uiRefMbType,
        layer_ref_pic(&*pCurDqLayer),
        iCurMbXy,
        SMALLEST_INVISIBLE,
    );

    !bChromaCostCannotSkip && !bChromaTooLarge
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterJudgeBGDPskip(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    bKeepSkip: &mut bool,
) -> bool {
    // T9.E2b: a field borrow under the `&mut` parent (F112's one step); its last
    // use is above the whole-slice pass to WelsMdBackgroundMbEnc, so NLL ends it
    // in time.
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pCurDqLayer = current_layer(pEncCtx);

    let kiRefMbQp = (&layer_ref_pic(&*pCurDqLayer).expect("bound").pRefMbQp)[(*pCurMb).iMbXY as usize] as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;
    // T9.E7, as svc_base_layer_md's mint (F132 round 8's class).
    let pVaaBgMbFlag = {
        let v = std::ptr::addr_of!((*pEncCtx).vaa().expect("the frame's video-analysis block").pVaaBackgroundMbFlag);
        (*v).as_ptr().add((*pCurMb).iMbXY as usize) as *mut i8
    };

    let kiMbWidth: isize = (*pCurDqLayer).iMbWidth as isize;

    *bKeepSkip = *bKeepSkip
        && (*pVaaBgMbFlag.offset(-1) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth + 1) == 0);

    if *pVaaBgMbFlag != 0
        && !IS_INTRA(pMbCache.uiRefMbType)
        && ((kiRefMbQp - kiCurMbQp <= DELTA_QP_BGD_THD) || (kiRefMbQp <= 26))
    {
        if CheckChromaCost(pEncCtx, pWelsMd, &mut *pMbCache, (*pCurMb).iMbXY) {
            let mut sVaaPredSkipMv = SMVUnitXY::default();
            PredSkipMv(&pMbCache.sMvComponents, &mut sVaaPredSkipMv);
            let bZeroMv = sVaaPredSkipMv.iMvX == 0 && sVaaPredSkipMv.iMvY == 0;
            WelsMdBackgroundMbEnc(pEncCtx, pWelsMd, pCurMb, pSlice, bZeroMv);
            return true;
        }
    }

    false
}

pub fn WelsMdInterJudgeBGDPskipFalse(
    _pCtx: &sWelsEncCtx,
    _pMd: &mut SWelsMD<'_>,
    _pSlice: &mut SSlice,
    _pCurMb: &mut SMB,
    _bKeepSkip: &mut bool,
) -> bool {
    false
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdUpdateBGDInfo(
    pCurLayer: &SDqLayer,
    pCurMb: &mut SMB,
    bCollocatedPredFlag: bool,
    iRefPictureType: i32,
) {
    let kiMbXY = (*pCurMb).iMbXY as usize;

    // Two *different* pictures, and the read is sequenced before the write so neither
    // borrow of a `pRefMbQp` outlives the other — `pDecPic` and `pRefPic` are distinct
    // slots by construction (session B's F42 note), but the spelling does not rely on it.
    let uiQp = if (*pCurMb).uiCbp != 0 || iRefPictureType == I_SLICE || !bCollocatedPredFlag {
        (*pCurMb).uiLumaQp
    } else {
        (&layer_ref_pic(pCurLayer).expect("bound").pRefMbQp)[kiMbXY]
    };
    layer_rec_view(pCurLayer).expect("bound").ref_mb_qp().set(kiMbXY, uiQp);

    if (*pCurMb).uiMbType == MB_TYPE_BACKGROUND {
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
    }
}

// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe extern "C" fn WelsMdUpdateBGDInfoNULL(
    pCurLayer: &SDqLayer,
    pCurMb: &mut SMB,
    bCollocatedPredFlag: bool,
    iRefPictureType: i32,
) {
    WelsMdUpdateBGDInfo(pCurLayer, pCurMb, bCollocatedPredFlag, iRefPictureType);
}

// ============================================================================
// 3. Screen Content Coding (SCC) & Scene Change Detection (SCD) P-Skip
// ============================================================================

#[inline(always)]
pub fn IsMbStatic(pBlockType: &[i32; 4], eType: EStaticBlockIdc) -> bool {
    // S4.C3: was `*const i32` walked with `.add(1..3)`, and the extent it walked is
    // the array both call sites hand it — `SWelsMD::iBlock8x8StaticIdc`, an
    // `[i32; 4]`, passed as `.as_ptr()`. The null guard goes with the raw: a
    // reference cannot be absent, and it answered `false` for a case no caller had.
    let target = eType as i32;
    pBlockType.iter().all(|&b| b == target)
}

#[inline(always)]
pub fn IsMbCollocatedStatic(pBlockType: &[i32; 4]) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::COLLOCATED_STATIC)
}

#[inline(always)]
pub fn IsMbScrolledStatic(pBlockType: &[i32; 4]) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::SCROLLED_STATIC)
}

/// **Dark — S57, measured (F121, T9.B27).** This function's only two callers are
/// [`JudgeStaticSkip`] and [`JudgeScrollSkip`], and both are reached only through
/// `pfSCDPSkipDecision`, which `WelsInitSCDPskipFunc` sets to the *judging* arm only
/// when `bScreenContent && bEnableSceneChangeDetect && complexity < HIGH`
/// (`encoder_context.rs:1574`). The diffharness driver encodes as
/// `CAMERA_VIDEO_REAL_TIME`, so `bScreenContent` is false in every sweep preset in
/// both profiles and the slot is `WelsMdInterJudgeSCDPskipFalse`. A probe printing
/// once per entry read **0** across five configurations (CAVLC/CABAC, complexity
/// LOW/HIGH, two streams, `sm=1 t=4` multi-threaded) against a calibration probe in
/// `WelsMdI16x16` that read 2008/1882/300/377/2136 in the same runs.
///
/// So this stays raw and tagged, with its two callers: session B3's brief listed it
/// as step 1 item 3 and the reachability answer says otherwise. It converts behind a
/// referee — the screen-content preset Phase 10 owns, or step 6's background preset
/// extended to `SCREEN_CONTENT_REAL_TIME`.
#[inline(always)]
// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
pub fn CalUVSadCost(
    sdf: &crate::encoder::md::SSampleDealingFunc,
    cEncOri: &crate::safe::plane::PlaneCursor<'_>,
    cRefOri: &crate::safe::plane::PlaneCursor<'_>,
) -> i32 {
    if let Some(sad_func) = sdf.pfSampleSad[BLOCK_8x8] {
        sad_func(cEncOri, cRefOri)
    } else {
        0
    }
}

#[inline(always)]
pub fn CheckBorder(
    iMbX: i32,
    iMbY: i32,
    iScrollMvX: i32,
    iScrollMvY: i32,
    iMbWidth: i32,
    iMbHeight: i32,
) -> bool {
    (iMbX << 4) + iScrollMvX < 0
        || (iMbX << 4) + iScrollMvX > ((iMbWidth - 1) << 4)
        || (iMbY << 4) + iScrollMvY < 0
        || (iMbY << 4) + iScrollMvY > ((iMbHeight - 1) << 4)
}

/// **Dark — S57**: see [`CalUVSadCost`] for the measurement. The
/// `ctx_pic_ref_mut(..).planes()` below is a whole-picture `&mut` retag taken inside
/// the macroblock loop (F73), which session B2's brief flagged as a live hazard in
/// the tree — it is live as *code* and unreachable as *behaviour*, on every path any
/// gate runs. It converts to the shared route with the rest of this function, behind
/// a referee.
// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn JudgeStaticSkip(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    pWelsMd: &mut SWelsMD<'_>,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;

    let mut bTryStaticSkip = IsMbCollocatedStatic(&(*pWelsMd).iBlock8x8StaticIdc);
    if bTryStaticSkip {
        // Session F: the shared picture route (`ctx_pic_ref` + plane cursors)
        // replaces the `ctx_pic_ref_mut(..).planes()` whole-picture retag F121
        // named live-as-code — and the raw-table read goes with the triple.
        let sdf = &(*pEncCtx).func_list().sSampleDealingFuncs;
        let pRefOriPic = (*pCurDqLayer).pRefOri[0]
            .and_then(|r| crate::encoder::svc_encode_slice::ctx_pic_ref(pEncCtx, r));
        if let Some(pRefOriPic) = pRefOriPic {
            let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
            let kiCx = (kiMbX as isize) << 3;
            let kiCy = (kiMbY as isize) << 3;

            let iSadCostCb = CalUVSadCost(
                sdf,
                &pEncPicture.plane(1).cursor(kiCx, kiCy),
                &pRefOriPic.plane(1).cursor(kiCx, kiCy),
            );
            if iSadCostCb == 0 {
                let iSadCostCr = CalUVSadCost(
                    sdf,
                    &pEncPicture.plane(2).cursor(kiCx, kiCy),
                    &pRefOriPic.plane(2).cursor(kiCx, kiCy),
                );
                bTryStaticSkip = iSadCostCr == 0;
            } else {
                bTryStaticSkip = false;
            }
        } else {
            bTryStaticSkip = false;
        }
    }
    bTryStaticSkip
}

/// **Dark — S57**: as [`JudgeStaticSkip`], and doubly so — it returns early unless
/// `sScrollDetectInfo.bScrollDetectFlag`, which only the screen-content preprocessor
/// sets.
// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn JudgeScrollSkip(
    pEncCtx: &sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: &mut SMbCache,
    pWelsMd: &mut SWelsMD<'_>,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbWidth: i32 = (*pCurDqLayer).iMbWidth as i32;
    let kiMbHeight: i32 = (*pCurDqLayer).iMbHeight as i32;
    let pVaaExt = (*pEncCtx).vaa_ext();

    let mut bTryScrollSkip;
    if (*pVaaExt).sScrollDetectInfo.bScrollDetectFlag {
        bTryScrollSkip = IsMbScrolledStatic(&(*pWelsMd).iBlock8x8StaticIdc);
    } else {
        return false;
    }

    if bTryScrollSkip {
        // Session F: as JudgeStaticSkip — shared picture route, safe cost slot.
        let sdf = &(*pEncCtx).func_list().sSampleDealingFuncs;
        let pRefOriPic = (*pCurDqLayer).pRefOri[0]
            .and_then(|r| crate::encoder::svc_encode_slice::ctx_pic_ref(pEncCtx, r));
        if let Some(pRefOriPic) = pRefOriPic {
            let iScrollMvX = (*pVaaExt).sScrollDetectInfo.iScrollMvX;
            let iScrollMvY = (*pVaaExt).sScrollDetectInfo.iScrollMvY;
            if CheckBorder(kiMbX, kiMbY, iScrollMvX, iScrollMvY, kiMbWidth, kiMbHeight) {
                bTryScrollSkip = false;
            } else {
                let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
                let kiCx = (kiMbX as isize) << 3;
                let kiCy = (kiMbY as isize) << 3;
                let kiRx = kiCx + (iScrollMvX >> 1) as isize;
                let kiRy = kiCy + (iScrollMvY >> 1) as isize;

                let iSadCostCb = CalUVSadCost(
                    sdf,
                    &pEncPicture.plane(1).cursor(kiCx, kiCy),
                    &pRefOriPic.plane(1).cursor(kiRx, kiRy),
                );
                if iSadCostCb == 0 {
                    let iSadCostCr = CalUVSadCost(
                        sdf,
                        &pEncPicture.plane(2).cursor(kiCx, kiCy),
                        &pRefOriPic.plane(2).cursor(kiRx, kiRy),
                    );
                    bTryScrollSkip = iSadCostCr == 0;
                } else {
                    bTryScrollSkip = false;
                }
            }
        }
    }
    bTryScrollSkip
}

/// **Dark — S57**: as [`CalUVSadCost`] (the same `pfSCDPSkipDecision` gate), probe
/// **0** across five configurations. Its three motion compensations and two SAD
/// calls stay raw.
// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn SvcMdSCDMbEnc(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pSlice: &mut SSlice,
    bQpSimilarFlag: bool,
    bMbSkipFlag: bool,
    sCurMbMv: &[SMVUnitXY; 2],
    eSkipMode: ESkipModes,
) {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pCurDqLayer = current_layer(pEncCtx);
    let pFunc = (*pEncCtx).func_list();
    let skip_idx = eSkipMode as usize;
    let sCandidateMv = sCurMbMv[skip_idx];

    let sMvp = SMVUnitXY {
        iMvX: sCandidateMv.iMvX,
        iMvY: sCandidateMv.iMvY,
    };

    // S4.C2: `SPicData.pRefMb[i]`, resolved at use. The roots and strides here are
    // the reference *picture*'s rather than the layer's arrays, so this does not go
    // through `mb_cursor` — but it is the same expression, and note the third line:
    // **plane 2 takes stride index 1**, which is what `WelsMdInterInit`'s single
    // `kiCurStrideUV` applied to both chroma planes. `data_ptr_shared` keeps the
    // root a shared derivation, so two workers resolving it are siblings (F71).
    let pRefPic = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
    let pd = &(*pMbCache).SPicData;
    let pRefLuma = pRefPic.data_ptr_shared(0).wrapping_offset(pd.mb_offset(pRefPic.stride(0), 0));
    let pRefCb = pRefPic.data_ptr_shared(1).wrapping_offset(pd.mb_offset(pRefPic.stride(1), 1));
    let pRefCr = pRefPic.data_ptr_shared(2).wrapping_offset(pd.mb_offset(pRefPic.stride(1), 2));
    let iLineSizeY = layer_ref_pic(&*pCurDqLayer).map_or(0, |p| p.stride(0));
    let iLineSizeUV = layer_ref_pic(&*pCurDqLayer).map_or(0, |p| p.stride(1));

    let mut pDstLuma = std::ptr::addr_of_mut!((*pMbCache).sSkipMb).cast::<u8>();
    let mut pDstCb = std::ptr::addr_of_mut!((*pMbCache).sSkipMb).cast::<u8>().add(256);
    let mut pDstCr = std::ptr::addr_of_mut!((*pMbCache).sSkipMb).cast::<u8>().add(256 + 64);

    let iOffsetY = (sCandidateMv.iMvX as i32 >> 2) + (sCandidateMv.iMvY as i32 >> 2) * iLineSizeY;
    let iOffsetUV = (sCandidateMv.iMvX as i32 >> 3) + (sCandidateMv.iMvY as i32 >> 3) * iLineSizeUV;

    if !bQpSimilarFlag || !bMbSkipFlag {
        pDstLuma = std::ptr::addr_of_mut!((*pMbCache).sMemPredMb)
            .cast::<u8>()
            .add(mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf));
        pDstCb = std::ptr::addr_of_mut!((*pMbCache).sMemPredMb)
            .cast::<u8>()
            .add(mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf));
        pDstCr = std::ptr::addr_of_mut!((*pMbCache).sMemPredMb)
            .cast::<u8>()
            .add(mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf))
            .add(64);
    }

    // Motion Compensation
    McLuma_c(
        pRefLuma.offset(iOffsetY as isize),
        iLineSizeY,
        pDstLuma,
        16,
        0,
        0,
        16,
        16,
    );
    McChroma_c(
        pRefCb.offset(iOffsetUV as isize),
        iLineSizeUV,
        pDstCb,
        8,
        sMvp.iMvX,
        sMvp.iMvY,
        8,
        8,
    );
    McChroma_c(
        pRefCr.offset(iOffsetUV as isize),
        iLineSizeUV,
        pDstCr,
        8,
        sMvp.iMvX,
        sMvp.iMvY,
        8,
        8,
    );

    (*pCurMb).uiCbp = 0;
    (*pWelsMd).iCostLuma = 0;

    // Session F: the safe cost slot and the carrier coordinates replace the
    // raw table and the stamped cursors (`pRefMb[0] + iOffsetY` was the ref
    // plane at MB origin + integer candidate MV — the verified identity).
    let sad_16x16 = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].unwrap();
    let kiMbXLuma = ((*pMbCache).SPicData.iMbX as isize) << 4;
    let kiMbYLuma = ((*pMbCache).SPicData.iMbY as isize) << 4;
    let sad_cost = {
        let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        sad_16x16(
            &pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma),
            &pRefPicture.plane(0).cursor(
                kiMbXLuma + ((sCandidateMv.iMvX as isize) >> 2),
                kiMbYLuma + ((sCandidateMv.iMvY as isize) >> 2),
            ),
        )
    };
    (*pCurMb).iSadCost = sad_cost;
    (*pWelsMd).iCostSkipMb = sad_cost;

    (*pCurMb).sP16x16Mv = sCandidateMv;
    layer_rec_view(&*pCurDqLayer)
        .expect("bound")
        .mv_list()
        .set((*pCurMb).iMbXY as usize, sCandidateMv);

    if bQpSimilarFlag && bMbSkipFlag {
        (*pCurMb).iRefIndex = [0; MB_BLOCK8x8_NUM];
        if let Some(pfUpdateMbMv) = (*pFunc).pfUpdateMbMv {
            pfUpdateMbMv(&mut (*pCurMb).sMv, sMvp);
        }
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
        WelsRecPskip(&*pCurDqLayer, &*pFunc, pCurMb, &mut *pMbCache);
        WelsMdInterUpdatePskip(pEncCtx, &*pCurDqLayer, &mut *pSlice, pCurMb);
        return;
    }

    (*pCurMb).uiMbType = MB_TYPE_16x16;

    (*pWelsMd).sMe.sMe16x16.sMv = sCandidateMv;
    let pMbCache = &mut pSlice.sMbCacheInfo;
    PredMv(
        &(*pMbCache).sMvComponents,
        0,
        4,
        0,
        &mut (*pWelsMd).sMe.sMe16x16.sMvp,
    );
    (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;

    UpdateP16x16MotionInfo(&mut (*pMbCache).sMvComponents, pCurMb, 0, &mut (*pWelsMd).sMe.sMe16x16.sMv);

    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        let pEncPicture = layer_enc_pic(&*pCurDqLayer).expect("the layer's source picture is bound");
        let pRefPicture = layer_ref_pic(&*pCurDqLayer).expect("the layer's reference picture is bound");
        (*pWelsMd).iCostLuma = sad_16x16(
            &pEncPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma),
            &pRefPicture.plane(0).cursor(kiMbXLuma, kiMbYLuma),
        );
    }

    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(
        pEncCtx,
        &mut *pSlice,
        pCurMb,
    );

    let pMbCache = &mut pSlice.sMbCacheInfo;
    // S9.0c: reconstruction plane through the frame's shared view, prediction scratch
    // through `RecCursor::over_owned` — the same operand type for two different
    // storages, which is what lets the dispatch slot stop being a raw pointer pair.
    // The chroma cursors both resolve at stride index 1, which is `stride_idx`'s rule
    // and what the raw form passed by hand.
    let recView = layer_rec_view(&*pCurDqLayer).expect("the frame's reconstruction view");
    let luma_off = mem_pred_luma_off((*pMbCache).uiMemPredLumaHalf);
    let chroma_off = mem_pred_chroma_off((*pMbCache).uiMemPredLumaHalf);
    if let Some(copy16) = (*pFunc).pfCopy16x16Aligned {
        copy16(
            &(*pMbCache).SPicData.mb_cursor_rec(recView, 0),
            &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, luma_off, 16),
        );
    }
    if let Some(copy8) = (*pFunc).pfCopy8x8Aligned {
        copy8(
            &(*pMbCache).SPicData.mb_cursor_rec(recView, 1),
            &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, chroma_off, 8),
        );
        copy8(
            &(*pMbCache).SPicData.mb_cursor_rec(recView, 2),
            &RecCursor::over_owned(&mut (*pMbCache).sMemPredMb, chroma_off + 64, 8),
        );
    }
}

// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe extern "C" fn MdInterSCDPskipProcess(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    eSkipMode: ESkipModes,
) -> bool {
    let pMbCache = &mut pSlice.sMbCacheInfo;
    let pVaaExt = (*pEncCtx).vaa_ext();
    let pCurDqLayer = current_layer(pEncCtx);

    let kiRefMbQp = (&layer_ref_pic(&*pCurDqLayer).expect("bound").pRefMbQp)[(*pCurMb).iMbXY as usize] as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;

    let pJudgeSkip: [pJudgeSkipFun; 2] = [JudgeStaticSkip, JudgeScrollSkip];
    let bSkipFlag = pJudgeSkip[eSkipMode as usize](pEncCtx, pCurMb, &mut *pMbCache, pWelsMd);

    if bSkipFlag {
        let bQpSimilarFlag = (kiRefMbQp - kiCurMbQp <= DELTA_QP_SCD_THD) || (kiRefMbQp <= 26);
        let mut sVaaPredSkipMv = SMVUnitXY::default();
        let mut sCurMbMv: [SMVUnitXY; 2] = [SMVUnitXY::default(), SMVUnitXY::default()];
        PredSkipMv(&pMbCache.sMvComponents, &mut sVaaPredSkipMv);

        if eSkipMode == ESkipModes::SCROLLED {
            sCurMbMv[1].iMvX = (WELS_CLIP3(
                (*pVaaExt).sScrollDetectInfo.iScrollMvX,
                -(*pEncCtx).iMvRange,
                (*pEncCtx).iMvRange,
            ) << 2) as i16;
            sCurMbMv[1].iMvY = (WELS_CLIP3(
                (*pVaaExt).sScrollDetectInfo.iScrollMvY,
                -(*pEncCtx).iMvRange,
                (*pEncCtx).iMvRange,
            ) << 2) as i16;
        }

        let bMbSkipFlag = sVaaPredSkipMv == sCurMbMv[eSkipMode as usize];
        SvcMdSCDMbEnc(
            pEncCtx,
            pWelsMd,
            pCurMb,
            pSlice,
            bQpSimilarFlag,
            bMbSkipFlag,
            &sCurMbMv,
            eSkipMode,
        );
        return true;
    }

    false
}

// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn SetBlockStaticIdcToMd(
    // S4.C3: `*mut` -> `&`, as `VaaBackgroundMbDataUpdate` above and for the same
    // reason — read-only, and fork-reachable through `pfSCDPSkipDecision`.
    // `extern "C"` came off with it: nothing in this tree crosses the C ABI (T4b.1).
    pVaaExt: &SVAAFrameInfoExt,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
    pDqLayer: &SDqLayer,
) {

    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbWidth: i32 = (*pDqLayer).iMbWidth as i32;
    let kiWidth: i32 = kiMbWidth << 1;

    let kiBlockIndexUp = (kiMbY << 1) * kiWidth + (kiMbX << 1);
    let kiBlockIndexLow = ((kiMbY << 1) + 1) * kiWidth + (kiMbX << 1);

    (*pWelsMd).iBlock8x8StaticIdc[0] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset(kiBlockIndexUp as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[1] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset((kiBlockIndexUp + 1) as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[2] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset(kiBlockIndexLow as isize) as i32;
    (*pWelsMd).iBlock8x8StaticIdc[3] =
        *(*pVaaExt).pVaaBestBlockStaticIdc.offset((kiBlockIndexLow + 1) as isize) as i32;
}

// SCREEN_CONTENT(dormant: Phase 10) — F125. `WelsInitSCDPskipFunc`
// (`encoder_context.rs:1607-1612`) installs `pfSCDPSkipDecision`'s judging arm only
// when `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
// `bScreenContent` is `iUsageType == SCREEN_CONTENT_REAL_TIME` — an axis neither
// diffharness driver expresses. So no camera-usage preset can reach this body, and
// B4's `bg` preset does not either: a probe in `SvcMdSCDMbEnc` read **0** in every
// one of the 48 `bg` rows, including the row where `WelsMdBackgroundMbEnc` entered
// 5771 times. This is Phase 10's family, and the retag says so rather than leaving it
// filed under Phase 9's port-raw backlog where it reads as pending work.
// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterJudgeSCDPskip(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    slice: &mut SSlice,
    pCurMb: &mut SMB,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    SetBlockStaticIdcToMd(&*(*pEncCtx).vaa_ext(), pWelsMd, pCurMb, &*pCurDqLayer);

    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, ESkipModes::STATIC) {
        return true;
    }
    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, ESkipModes::SCROLLED) {
        return true;
    }

    false
}

pub fn WelsMdInterJudgeSCDPskipFalse(
    _pEncCtx: &sWelsEncCtx,
    _pWelsMd: &mut SWelsMD<'_>,
    _slice: &mut SSlice,
    _pCurMb: &mut SMB,
) -> bool {
    false
}

pub extern "C" fn WelsInitSCDPskipFunc(
    pFuncList: &mut SWelsFuncPtrList,
    bScrollingDetection: bool,
) {
    if bScrollingDetection {
        pFuncList.pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskip);
    } else {
        pFuncList.pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskipFalse);
    }
}

// ============================================================================
// 4. Sub-Macroblock Fine Partitioning & Mode Merging
// ============================================================================

#[inline(always)]
pub fn MergeSub16Me<'a>(sSrcMe0: &SWelsME<'a>, sSrcMe1: &SWelsME<'_>, pTarMe: &mut SWelsME<'a>) {
    // Was `copy_nonoverlapping(sSrcMe0, pTarMe, 1)`; `SWelsME` is `Copy`, so the
    // whole-record copy is an assignment and the `unsafe` goes with the pointers.
    *pTarMe = *sSrcMe0;
    pTarMe.uiSadCost = sSrcMe0.uiSadCost + sSrcMe1.uiSadCost;
    pTarMe.uiSatdCost = sSrcMe0.uiSatdCost + sSrcMe1.uiSatdCost;
}

#[inline(always)]
pub fn IsSameMv(sMv0: &SMVUnitXY, sMv1: &SMVUnitXY) -> bool {
    sMv0.iMvX == sMv1.iMvX && sMv0.iMvY == sMv1.iMvY
}

// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn TryModeMerge(
    pMbCache: &mut SMbCache,
    pWelsMd: &mut SWelsMD<'_>,
    pCurMb: &mut SMB,
) -> bool {
    let pMe8x8 = (*pWelsMd).sMe.sMe8x8.as_ptr();

    let bSameMv16x8_0 = IsSameMv(&(*pMe8x8.add(0)).sMv, &(*pMe8x8.add(1)).sMv);
    let bSameMv16x8_1 = IsSameMv(&(*pMe8x8.add(2)).sMv, &(*pMe8x8.add(3)).sMv);

    let bSameMv8x16_0 = IsSameMv(&(*pMe8x8.add(0)).sMv, &(*pMe8x8.add(2)).sMv);
    let bSameMv8x16_1 = IsSameMv(&(*pMe8x8.add(1)).sMv, &(*pMe8x8.add(3)).sMv);

    let bSameRefIdx16x8_0 = true;
    let bSameRefIdx16x8_1 = true;
    let bSameRefIdx8x16_0 = true;
    let bSameRefIdx8x16_1 = true;

    let iSameMv = (((bSameMv16x8_0 && bSameRefIdx16x8_0 && bSameMv16x8_1 && bSameRefIdx16x8_1) as i32)
        << 1)
        | ((bSameMv8x16_0 && bSameRefIdx8x16_0 && bSameMv8x16_1 && bSameRefIdx8x16_1) as i32);

    match iSameMv {
        2 => {
            (*pCurMb).uiMbType = MB_TYPE_16x8;
            MergeSub16Me(&*pMe8x8.add(0), &*pMe8x8.add(1), &mut (*pWelsMd).sMe.sMe16x8[0]);
            MergeSub16Me(&*pMe8x8.add(2), &*pMe8x8.add(3), &mut (*pWelsMd).sMe.sMe16x8[1]);
            PredInter16x8Mv(&(*pMbCache).sMvComponents, 0, 0, &mut (*pWelsMd).sMe.sMe16x8[0].sMvp);
            PredInter16x8Mv(&(*pMbCache).sMvComponents, 8, 0, &mut (*pWelsMd).sMe.sMe16x8[1].sMvp);
        }
        1 => {
            (*pCurMb).uiMbType = MB_TYPE_8x16;
            MergeSub16Me(&*pMe8x8.add(0), &*pMe8x8.add(2), &mut (*pWelsMd).sMe.sMe8x16[0]);
            MergeSub16Me(&*pMe8x8.add(1), &*pMe8x8.add(3), &mut (*pWelsMd).sMe.sMe8x16[1]);
            PredInter8x16Mv(&(*pMbCache).sMvComponents, 0, 0, &mut (*pWelsMd).sMe.sMe8x16[0].sMvp);
            PredInter8x16Mv(&(*pMbCache).sMvComponents, 4, 0, &mut (*pWelsMd).sMe.sMe8x16[1].sMvp);
        }
        _ => {}
    }

    (*pCurMb).uiMbType != MB_TYPE_8x8
}

// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn WelsMdInterFinePartitionVaaOnScreen(
    pEncCtx: &sWelsEncCtx,
    pWelsMd: &mut SWelsMD<'_>,
    pSlice: &mut SSlice,
    pCurMb: &mut SMB,
    mut iBestCost: i32,
) {
    let pCurDqLayer = current_layer(pEncCtx);

    // T9.E7's spelling, which this site was missing: `as_mut_ptr` autorefs `&mut`
    // on the *shared* VAA struct's vector — a retag every worker makes per
    // macroblock — where `as_ptr` autorefs `&`. Same address, and the two sibling
    // mints in `svc_base_layer_md.rs` already read it this way.
    let pSad8x8_ptr = (*pEncCtx)
        .vaa()
        .expect("the frame's video-analysis block")
        .sVaaCalcInfo
        .pSad8x8
        .as_ptr()
        .add((*pCurMb).iMbXY as usize) as *mut i32;
    let get_sign = (*pEncCtx).func_list().pfGetMbSignFromInterVaa.unwrap();
    let uiMbSign = get_sign(pSad8x8_ptr);

    if uiMbSign == MBVAASIGN_FLAT {
        return;
    }

    let iCostP8x8 = WelsMdP8x8((*pEncCtx).func_list(), &*pCurDqLayer, pWelsMd, pSlice);
    if iCostP8x8 < iBestCost {
        iBestCost = iCostP8x8;
        (*pCurMb).uiMbType = MB_TYPE_8x8;
        (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];
        TryModeMerge(&mut pSlice.sMbCacheInfo, pWelsMd, pCurMb);
    }
    (*pWelsMd).iCostLuma = iBestCost;
}

// ============================================================================
// 5. Global Scrolling Motion Vector Dispatch
// ============================================================================

// unsafe-cat: SCREEN_CONTENT(dormant)
#[allow(unsafe_code)]
pub unsafe fn SetScrollingMvToMd(pVaa: &SVAAFrameInfo, pWelsMd: &mut SWelsMD<'_>) {
    // The screen-content downcast — the C++'s `static_cast<SVAAFrameInfoExt*>`.
    // It stays inside an `unsafe fn` rather than becoming an explicit block in a
    // safe one: A5 centralised this cast in `sWelsEncCtx::vaa_ext` so it would not
    // be claimed in fifteen separate places, and re-spelling it here as a safe-fn
    // block would make it a second claim *and* move an `unsafe_block` onto this
    // file's ratchet row for no aliasing gain. What C1 changes is the parameter,
    // not the cast: `*mut SVAAFrameInfo` -> `&SVAAFrameInfo`, so the slot can
    // never hand a worker an exclusive reference to the one shared block.
    let pVaaExt = pVaa as *const SVAAFrameInfo as *const SVAAFrameInfoExt;
    let sTempMv = SMVUnitXY {
        iMvX: (*pVaaExt).sScrollDetectInfo.iScrollMvX as i16,
        iMvY: (*pVaaExt).sScrollDetectInfo.iScrollMvY as i16,
    };

    pWelsMd.sMe.sMe16x16.sDirectionalMv = sTempMv;
    pWelsMd.sMe.sMe8x8[0].sDirectionalMv = sTempMv;
    pWelsMd.sMe.sMe8x8[1].sDirectionalMv = sTempMv;
    pWelsMd.sMe.sMe8x8[2].sDirectionalMv = sTempMv;
    pWelsMd.sMe.sMe8x8[3].sDirectionalMv = sTempMv;
}

/// Intentional no-op mode decision scrolling MV callback.
/// Matches `void SetScrollingMvToMdNull (SVAAFrameInfo* pVaa, SWelsMD* pWelsMd)` in `svc_mode_decision.cpp:689`.
pub fn SetScrollingMvToMdNull(_pVaa: &SVAAFrameInfo, _pWelsMd: &mut SWelsMD<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_pred_mv_basic_median() {
        unsafe {
            let mut mv_comp = SMVComponentUnit::default();
            // Cache index 6 is Left (kuiLeftIdx), 1 is Top (kuiTopIdx), 5 is RightTop (kuiTopIdx + 4)
            mv_comp.iRefIndexCache[6] = 0;
            mv_comp.iRefIndexCache[1] = 0;
            mv_comp.iRefIndexCache[5] = 0;

            mv_comp.sMotionVectorCache[6] = SMVUnitXY { iMvX: 10, iMvY: 20 };
            mv_comp.sMotionVectorCache[1] = SMVUnitXY { iMvX: 30, iMvY: 40 };
            mv_comp.sMotionVectorCache[5] = SMVUnitXY { iMvX: 20, iMvY: 30 };

            let mut sMvp = SMVUnitXY::default();
            PredMv(&mv_comp, 0, 4, 0, &mut sMvp);

            // Median of (10, 30, 20) is 20; Median of (20, 40, 30) is 30
            assert_eq!(sMvp.iMvX, 20);
            assert_eq!(sMvp.iMvY, 30);
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_pred_skip_mv_zero_ref() {
        unsafe {
            // iSadCost/iSadCostSkip/bMbTypeSkip are fixed arrays in C++
            // (mb_cache.h:81, :110, :111), not pointers; these tests only need the
            // MV cache, so the rest comes from Default.
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                sMvComponents: SMVComponentUnit::default(),
                sMbMvp: [SMVUnitXY::default(); 16],
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
                bCollocatedPredFlag: false,
                ..Default::default()
            };

            // When left & top ref MVs are (0,0) and ref=0, PredSkipMv returns (0,0)
            mb_cache.sMvComponents.iRefIndexCache[6] = 0;
            mb_cache.sMvComponents.iRefIndexCache[1] = 0;
            mb_cache.sMvComponents.sMotionVectorCache[6] = SMVUnitXY { iMvX: 0, iMvY: 0 };
            mb_cache.sMvComponents.sMotionVectorCache[1] = SMVUnitXY { iMvX: 0, iMvY: 0 };

            let mut sMvp = SMVUnitXY { iMvX: 99, iMvY: 99 };
            PredSkipMv(&mb_cache.sMvComponents, &mut sMvp);

            assert_eq!(sMvp.iMvX, 0);
            assert_eq!(sMvp.iMvY, 0);
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_pred_inter_16x8_8x16_mv() {
        unsafe {
            // iSadCost/iSadCostSkip/bMbTypeSkip are fixed arrays in C++
            // (mb_cache.h:81, :110, :111), not pointers; these tests only need the
            // MV cache, so the rest comes from Default.
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                sMvComponents: SMVComponentUnit::default(),
                sMbMvp: [SMVUnitXY::default(); 16],
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
                bCollocatedPredFlag: false,
                ..Default::default()
            };

            mb_cache.sMvComponents.iRefIndexCache[1] = 0; // Top ref for 16x8 part 0
            mb_cache.sMvComponents.sMotionVectorCache[1] = SMVUnitXY { iMvX: 12, iMvY: 34 };

            let mut sMvp = SMVUnitXY::default();
            PredInter16x8Mv(&mb_cache.sMvComponents, 0, 0, &mut sMvp);
            assert_eq!(sMvp.iMvX, 12);
            assert_eq!(sMvp.iMvY, 34);

            mb_cache.sMvComponents.iRefIndexCache[6] = 0; // Left ref for 8x16 part 0
            mb_cache.sMvComponents.sMotionVectorCache[6] = SMVUnitXY { iMvX: 56, iMvY: 78 };

            let mut sMvp8x16 = SMVUnitXY::default();
            PredInter8x16Mv(&mb_cache.sMvComponents, 0, 0, &mut sMvp8x16);
            assert_eq!(sMvp8x16.iMvX, 56);
            assert_eq!(sMvp8x16.iMvY, 78);
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_update_p16x16_motion_info() {
        unsafe {
            // iSadCost/iSadCostSkip/bMbTypeSkip are fixed arrays in C++
            // (mb_cache.h:81, :110, :111), not pointers; these tests only need the
            // MV cache, so the rest comes from Default.
            let mut mb_cache = SMbCache {
                uiRefMbType: 0,
                sMvComponents: SMVComponentUnit::default(),
                sMbMvp: [SMVUnitXY::default(); 16],
                uiNeighborIntra: 0,
                uiLumaI16x16Mode: 0,
                bCollocatedPredFlag: false,
                ..Default::default()
            };

            let mut cur_mb = SMB {
                uiMbType: MB_TYPE_16x16,
                uiSubMbType: [0; 4],
                iMbXY: 0,
                iMbX: 0,
                iMbY: 0,
                uiNeighborAvail: 0,
                uiCbp: 0,
                sMv: [SMVUnitXY::default(); MB_BLOCK4x4_NUM],
                iRefIndex: [0; MB_BLOCK8x8_NUM],
                iSadCost: 0,
                iIntra4x4PredMode: [0; crate::encoder::md::INTRA_4x4_MODE_NUM],
                iNonZeroCount: [0; MB_LUMA_CHROMA_BLOCK4x4_NUM],
                sP16x16Mv: SMVUnitXY::default(),
                uiLumaQp: 26,
                uiChromaQp: 26,
                uiSliceIdc: 0,
                uiChromPredMode: 0,
                iLumaDQp: 0,
                sMvd: [SMVUnitXY::default(); 16],
                iCbpDc: 0,
            };

            let mut target_mv = SMVUnitXY { iMvX: 42, iMvY: -15 };
            UpdateP16x16MotionInfo(
                &mut mb_cache.sMvComponents,
                &mut cur_mb,
                0,
                &mut target_mv,
            );

            assert_eq!(cur_mb.sMv[0], target_mv);
            assert_eq!(cur_mb.iRefIndex[0], 0);
            assert_eq!(
                mb_cache.sMvComponents.sMotionVectorCache[g_kuiCache30ScanIdx[0] as usize],
                target_mv
            );
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_wels_md_i16x16_cost() {
        unsafe {
            // The function-pointer tables must be populated the way the real caller
            // does it: WelsInitIntraPredFuncs installs pfGetLumaI16x16Pred and
            // WelsInitSampleSadFunc installs pfSampleSad, which SetFastCodingFunc then
            // selects via pfMdCost. This test previously left every entry unset, so it
            // asserted only that a function which silently did nothing returned
            // something below i32::MAX.
            let mut func_list = SWelsFuncPtrList::default();
            crate::encoder::get_intra_predictor::WelsInitIntraPredFuncs(&mut func_list, 0);
            crate::encoder::sample::WelsInitSampleSadFunc(&mut func_list, 0);
            func_list.sSampleDealingFuncs.pfMdCost = crate::encoder::md::CostFamily::Sad;

            // **T9.C2**: the reconstruction is reached through the layer's seam view
            // now, so the fixture builds a real picture for it rather than a bare
            // `Vec` and a hand-offset pointer — the same change T9.B30 made to the
            // source side above, and for the same reason. It still needs a real
            // border, because the V/H/DC predictors read `(x, -1)` and `(-1, y)`.
            const STRIDE: usize = 48;
            let mut rec_pic = crate::encoder::picture::SPicture::new(160, 160, false);
            {
                let plane = rec_pic.plane_mut(0);
                let (w, h) = (plane.width() as isize, plane.height() as isize);
                for y in -1..h {
                    plane.row_mut(y, -1, (w + 2) as usize).fill(128);
                }
            }

            // **The source comes through the layer since T9.B30**, so the fixture
            // builds a real picture and a pool rather than a bare `Vec` and a pointer.
            // The macroblock under test is (1, 1) and its 16x16 luma block is set 10
            // above the neighbours, which is what makes the SAD a known number.
            const MB_X: i32 = 1;
            const MB_Y: i32 = 1;
            let mut src_pic = crate::encoder::picture::SPicture::new(160, 160, false);
            {
                let plane = src_pic.plane_mut(0);
                let (w, h) = (plane.width() as isize, plane.height() as isize);
                for y in -1..h {
                    plane.row_mut(y, -1, (w + 2) as usize).fill(128);
                }
                for y in 0..16 {
                    plane
                        .row_mut((MB_Y as isize) * 16 + y, (MB_X as isize) * 16, 16)
                        .fill(138);
                }
            }
            let mut src_pool = crate::encoder::picture::SrcPicPool::new(vec![src_pic]);
            let src_id = src_pool.at(0);
            // The prediction ping-pong is `SMbCache::sMemPredMb` since T6.C3 —
            // `[u8; 2 * 256 + 16]`, and the `+ 16` is F14's accommodation, documented
            // on the field. **This test is the instrument that keeps it**: delete the
            // `+ 16` and the raw 16x16 SAD's one-past-the-row pointer takes this test
            // red under Miri.
            let mut mb_cache = SMbCache {
                // The three cursor triples were stamped null here on purpose — the
                // assertion that this function reaches the source and the
                // reconstruction through the layer plus these coordinates, and never
                // falls back to a stored pointer. S4.C2 made that structural: the
                // fields are gone, so the coordinates are all there is to give.
                SPicData: SPicData {
                    iMbX: MB_X,
                    iMbY: MB_Y,
                },
                uiNeighborIntra: 0x07, // left + top + top-left available
                ..Default::default()
            };

            let mut dq_layer = SDqLayer {
                iMbWidth: 10,
                iMbHeight: 10,
                iEncStride: [STRIDE as i32; 3],
                iCsStride: [STRIDE as i32; 3],
                sLayerInfo: SLayerInfo::default(),
                pEncPic: Some(src_id),
                pSrcPool: &mut src_pool,
                pRecView: Some(crate::encoder::rec_view::RecPicView::build(&mut rec_pic)),
                ..Default::default()
            };

            let iLambda = 10;
            let cost = WelsMdI16x16(
                &func_list,
                (&mut dq_layer as *mut SDqLayer).as_ref(),
                &mut mb_cache,
                iLambda,
            );

            // Every neighbour sample is 128 and every source sample is 138, so V, H and
            // DC all predict 128 and all score SAD = 256 * 10 = 2560. The tie is broken
            // by the first candidate, g_kiIntra16AvaliMode[7][0] = I16_PRED_V, whose
            // mode-signalling cost is iLambda * BsSizeUE(g_kiMapModeI16x16[V]=0) = 10.
            assert_eq!(mb_cache.uiLumaI16x16Mode, I16_PRED_V as u8);
            assert_eq!(cost, 2560 + iLambda);

            // The winning prediction lands in the luma half and the scratch half is
            // handed to the chroma search — one selector bit, two halves of one array.
            //
            // **Order matters and the `exit` battery is what said so.** Each accessor
            // call retags the whole `SMbCache` (it takes a raw pointer, and passing
            // `&mut mb_cache` is a `Unique` retag over all 5600 bytes), so a pointer
            // derived from `sMemPredMb` *before* the calls is popped by them and reading
            // through it afterwards is UB — the same class this session converted, in
            // this test's own assertions. The accessor answers are taken first and the
            // expectation is derived last, so the tag that reads the buffer is on top.
            assert_eq!(mb_cache.uiMemPredLumaHalf, 0);
            let pLuma = std::ptr::addr_of_mut!(mb_cache.sMemPredMb).cast::<u8>().add(mem_pred_luma_off(mb_cache.uiMemPredLumaHalf));
            let pChroma = std::ptr::addr_of_mut!(mb_cache.sMemPredMb).cast::<u8>().add(mem_pred_chroma_off(mb_cache.uiMemPredLumaHalf));
            let pPredBuf = std::ptr::addr_of_mut!(mb_cache.sMemPredMb).cast::<u8>();
            assert_eq!(pLuma, pPredBuf);
            assert_eq!(pChroma, pPredBuf.add(256));
            assert!(std::slice::from_raw_parts(pPredBuf, 256).iter().all(|&b| b == 128));
        }
    }

    #[test]
    // unsafe-cat: instrument(test)
    #[allow(unsafe_code)]
    fn test_svc_mode_decision_noop_callback() {
        // The MD argument used to be a null raw MD pointer; it is a `&mut` now, so
        // the null goes and a real record takes its place. **S4.C1 does the same to
        // `pVaa`**: the slot's parameter is `&SVAAFrameInfo` rather than a raw, so
        // there is no null to pass and a real block takes its place here too. This
        // callback is the no-op arm of `PSetScrollingMv` and reads neither.
        let sVaa = SVAAFrameInfo::default();
        let mut sMd = SWelsMD::default();
        unsafe {
            SetScrollingMvToMdNull(&sVaa, &mut sMd);
        }
    }
}
