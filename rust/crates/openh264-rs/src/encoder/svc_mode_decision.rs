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


use crate::encoder::svc_encode_slice::{layer_dec_pic, layer_dec_pic_mut, layer_ref_pic, layer_ref_pic_mut};
use crate::encoder::svc_encode_slice::layer_pps;
use crate::encoder::svc_encode_slice::current_layer;
use crate::encoder::picture::{RecPicId, SrcPicId};
use crate::encoder::md::{PredictSad, PredictSadSkip, WelsMedian};
use crate::encoder::svc_encode_mb::WelsEncInterY;
use crate::encoder::svc_encode_slice::WelsPMbChromaEncode;
use crate::encoder::svc_set_mb_syn_cavlc::IS_INTRA16x16;
use crate::encoder::vlc_encoder::BsSizeUE;
pub use crate::encoder::encoder_context::SMVUnitXY;
use crate::encoder::encoder_context::{ctx_dq_layer, ctx_vaa, ctx_func_list};
pub use crate::encoder::encoder_context::SMVComponentUnit;
pub use crate::encoder::encoder_context::EWelsSliceType;
pub use crate::encoder::picture::SScreenBlockFeatureStorage;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::param_svc::SWelsPPS;
pub use crate::encoder::wels_preprocess::EStaticBlockIdc;
pub use crate::encoder::md::SMcFunc;
// Phase 4a: MC is called directly, not via `sMcFuncs`.
use crate::common::mc::{McChroma_c, McLuma_c};
pub use crate::encoder::wels_preprocess::SVAACalcResult;
pub use crate::encoder::wels_preprocess::SScrollDetectionParam;
pub use crate::encoder::svc_motion_estimate::SWelsME;
pub use crate::encoder::md::SWelsMD;
pub use crate::encoder::wels_preprocess::SVAAFrameInfo;
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
pub const SUB_MB_TYPE_8x4: u8 = 0x02;
pub const SUB_MB_TYPE_4x8: u8 = 0x04;
pub const SUB_MB_TYPE_4x4: u8 = 0x08;

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
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: &mut SWelsMD,
) -> bool;

// ============================================================================
// Core Structures Matching C/C++ Layout
// ============================================================================



#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SWelsMeContainers {
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
// under screen content, which `RequestMemorySvc` refuses.
#[repr(C)]
pub struct SVAAFrameInfoExt_t {
    pub sVaaBase: SVAAFrameInfo,
    pub sScrollDetectInfo: SScrollDetectionParam,
    pub pVaaBestBlockStaticIdc: *mut u8,
}

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
pub unsafe extern "C" fn WelsMdInterUpdatePskip(
    pEncCtx: *mut sWelsEncCtx,
    pCurDqLayer: *mut SDqLayer,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
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
pub unsafe extern "C" fn WelsMdInterJudgePskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
    bTrySkip: bool,
) -> bool {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let bRet;
    if ((crate::encoder::svc_encode_slice::ctx_ref_pic(pEncCtx)
        .map_or(0, |p| p.iPictureType)
        == EWelsSliceType::P_SLICE as i32)
        && ((*pMbCache).uiRefMbType == MB_TYPE_SKIP || (*pMbCache).uiRefMbType == MB_TYPE_BACKGROUND))
        || bTrySkip
    {
        PredictSadSkip(
            (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
            (*pMbCache).bMbTypeSkip.as_mut_ptr(),
            (*pMbCache).iSadCostSkip.as_mut_ptr(),
            0,
            &mut (*pWelsMd).iSadPredSkip,
        );
        bRet = crate::encoder::svc_base_layer_md::WelsMdPSkipEnc(pEncCtx, pWelsMd, pCurMb, pMbCache);
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
pub unsafe extern "C" fn WelsMdInterDecidedPskip(
    pEncCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);
    (*pCurMb).uiMbType = MB_TYPE_SKIP;
    WelsRecPskip(pCurDqLayer, ctx_func_list(pEncCtx), pCurMb, pMbCache);
    WelsMdInterUpdatePskip(pEncCtx, pCurDqLayer, pSlice, pCurMb);
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
pub unsafe extern "C" fn WelsMdInterSecondaryModesEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
    bSkip: bool,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pFuncList = ctx_func_list(pEncCtx);
    //step 2: Intra
    let kbTrySkip = (*pFuncList).pfFirstIntraMode.expect(
        "pfFirstIntraMode is unset; PreprocessSliceCoding must assign WelsMdFirstIntraMode \
         before any P macroblock is coded",
    )(pEncCtx, pWelsMd, pCurMb, pMbCache);
    if kbTrySkip {
        return;
    }

    if bSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, pCurMb);
    } else {
        //Step 3: SubP16 MD
        (*pFuncList).pfSetScrollingMv.expect("pfSetScrollingMv is unset")(
            ctx_vaa(pEncCtx),
            pWelsMd,
        ); //SCC
        (*pFuncList).pfInterFineMd.expect(
            "pfInterFineMd is unset; PreprocessSliceCoding must assign \
             WelsMdInterFinePartition[Vaa] before any P macroblock is coded",
        )(pEncCtx, pWelsMd, pSlice, pCurMb, (*pWelsMd).iCostLuma);

        //refinement for inter type
        crate::encoder::svc_base_layer_md::WelsMdInterMbRefinement(pEncCtx, pWelsMd, pCurMb, pMbCache);

        //step 7: invoke encoding
        crate::encoder::svc_base_layer_md::WelsMdInterEncode(pEncCtx, pSlice, pCurMb);

        //step 8: double check Pskip
        crate::encoder::svc_base_layer_md::WelsMdInterDoubleCheckPskip(pCurMb, pMbCache);
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
pub unsafe extern "C" fn WelsMdIntraSecondaryModesEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
) {
    let pFunc = ctx_func_list(pEncCtx);
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
        pFunc,
        current_layer(pEncCtx),
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
pub unsafe extern "C" fn WelsRecPskip(
    pCurLayer: *mut SDqLayer,
    pFuncList: *mut SWelsFuncPtrList,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
) {
    let iRecStride = (*pCurLayer).iCsStride;
    let pCsMb = (*pMbCache).SPicData.pCsMb;

    (*pFuncList).pfCopy16x16Aligned.expect("pfCopy16x16Aligned unset")(
        pCsMb[0],
        iRecStride[0],
        crate::encoder::md::skip_mb(pMbCache),
        16,
    );
    let copy8 = (*pFuncList).pfCopy8x8Aligned.expect("pfCopy8x8Aligned unset");
    copy8(pCsMb[1], iRecStride[1], crate::encoder::md::skip_mb(pMbCache).add(256), 8);
    copy8(pCsMb[2], iRecStride[2], crate::encoder::md::skip_mb(pMbCache).add(320), 8);
    // `WelsSetMemZero (pCurMb->pNonZeroCount, 24)` — the row is inline now.
    (*pCurMb).iNonZeroCount = [0; MB_LUMA_CHROMA_BLOCK4x4_NUM];
}

/// Copies the current/reference luma & chroma blocks for a background MB into the VAA
/// info so future-frame background comparisons stay in sync.
///
/// Translated from `VaaBackgroundMbDataUpdate` in
/// `codec/encoder/core/src/svc_base_layer_md.cpp:1341`.
#[inline(always)]
unsafe fn VaaBackgroundMbDataUpdate(
    pFunc: *mut SWelsFuncPtrList,
    pVaaInfo: *mut crate::encoder::wels_preprocess::SVAAFrameInfo,
    pCurMb: &mut SMB,
) {
    let kiPicStride = (*pVaaInfo).iPicStride;
    let kiPicStrideUV = (*pVaaInfo).iPicStrideUV;
    let kiOffsetY = (((*pCurMb).iMbY as i32) * kiPicStride + (*pCurMb).iMbX as i32) << 4;
    let kiOffsetUV = (((*pCurMb).iMbY as i32) * kiPicStrideUV + (*pCurMb).iMbX as i32) << 3;

    if let Some(copy16) = (*pFunc).pfCopy16x16Aligned {
        copy16(
            (*pVaaInfo).pCurY.offset(kiOffsetY as isize),
            kiPicStride,
            (*pVaaInfo).pRefY.offset(kiOffsetY as isize),
            kiPicStride,
        );
    }
    if let Some(copy8) = (*pFunc).pfCopy8x8Aligned {
        copy8(
            (*pVaaInfo).pCurU.offset(kiOffsetUV as isize),
            kiPicStrideUV,
            (*pVaaInfo).pRefU.offset(kiOffsetUV as isize),
            kiPicStrideUV,
        );
        copy8(
            (*pVaaInfo).pCurV.offset(kiOffsetUV as isize),
            kiPicStrideUV,
            (*pVaaInfo).pRefV.offset(kiOffsetUV as isize),
            kiPicStrideUV,
        );
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
pub unsafe extern "C" fn WelsMdBackgroundMbEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pSlice: *mut SSlice,
    bSkipMbFlag: bool,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);
    let pFunc = ctx_func_list(pEncCtx);
    let sMvp = SMVUnitXY::default();

    let pRefLuma = (*pMbCache).SPicData.pRefMb[0];
    let pRefCb = (*pMbCache).SPicData.pRefMb[1];
    let pRefCr = (*pMbCache).SPicData.pRefMb[2];
    let iLineSizeY = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[0];
    let iLineSizeUV = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[1];

    let mut pDstLuma = crate::encoder::md::skip_mb(pMbCache);
    let mut pDstCb = crate::encoder::md::skip_mb(pMbCache).add(256);
    let mut pDstCr = crate::encoder::md::skip_mb(pMbCache).add(256 + 64);

    if !bSkipMbFlag {
        pDstLuma = crate::encoder::md::mem_pred_luma(pMbCache);
        pDstCb = crate::encoder::md::mem_pred_chroma(pMbCache);
        pDstCr = crate::encoder::md::mem_pred_chroma(pMbCache).add(64);
    }

    // MC
    McLuma_c(pRefLuma, iLineSizeY, pDstLuma, 16, 0, 0, 16, 16);
    McChroma_c(pRefCb, iLineSizeUV, pDstCb, 8, sMvp.iMvX, sMvp.iMvY, 8, 8); // Cb
    McChroma_c(pRefCr, iLineSizeUV, pDstCr, 8, sMvp.iMvX, sMvp.iMvY, 8, 8); // Cr

    (*pCurMb).uiCbp = 0;
    (*pMbCache).bCollocatedPredFlag = true;
    (*pWelsMd).iCostLuma = 0; // BGD&RC integration
    (*pCurMb).iSadCost = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].unwrap()(
        (*pMbCache).SPicData.pEncMb[0],
        (*pCurDqLayer).iEncStride[0],
        pRefLuma,
        iLineSizeY,
    );
    (*pCurMb).sP16x16Mv = SMVUnitXY::default();
    (&mut layer_dec_pic_mut(pCurDqLayer).expect("bound").sMvList)[(*pCurMb).iMbXY as usize] = SMVUnitXY::default();

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

        WelsRecPskip(pCurDqLayer, pFunc, pCurMb, pMbCache);
        VaaBackgroundMbDataUpdate(
            pFunc,
            ctx_vaa(pEncCtx) as *mut crate::encoder::wels_preprocess::SVAAFrameInfo,
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
        pMbCache,
        pCurMb,
        (*pWelsMd).uiRef as i8,
        &mut (*pWelsMd).sMe.sMe16x16.sMv,
    );

    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        (*pWelsMd).iCostLuma = (*pFunc).sSampleDealingFuncs.pfSampleSatd[BLOCK_16x16].unwrap()(
            (*pMbCache).SPicData.pEncMb[0],
            (*pCurDqLayer).iEncStride[0],
            pRefLuma,
            iLineSizeY,
        );
    }

    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(
        pEncCtx as *mut crate::encoder::svc_encode_slice::sWelsEncCtx,
        pSlice as *mut crate::encoder::svc_encode_slice::SSlice,
        pCurMb,
    );

    if let Some(copy16) = (*pFunc).pfCopy16x16Aligned {
        copy16(
            (*pMbCache).SPicData.pCsMb[0],
            (*pCurDqLayer).iCsStride[0],
            crate::encoder::md::mem_pred_luma(pMbCache),
            16,
        );
    }
    if let Some(copy8) = (*pFunc).pfCopy8x8Aligned {
        copy8(
            (*pMbCache).SPicData.pCsMb[1],
            (*pCurDqLayer).iCsStride[1],
            crate::encoder::md::mem_pred_chroma(pMbCache),
            8,
        );
        copy8(
            (*pMbCache).SPicData.pCsMb[2],
            (*pCurDqLayer).iCsStride[1],
            crate::encoder::md::mem_pred_chroma(pMbCache).add(64),
            8,
        );
    }
}

// ============================================================================
// Native Mode Decision & Motion Prediction Implementations
// ============================================================================

pub unsafe extern "C" fn PredMv(
    kpMvComp: *const SMVComponentUnit,
    iPartIdx: i8,
    iPartW: i8,
    iRef: i32,
    sMvp: &mut SMVUnitXY,
) {
    let kuiLeftIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 1;
    let kuiTopIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize - 6;

    let iLeftRef = (*kpMvComp).iRefIndexCache[kuiLeftIdx] as i32;
    let iTopRef = (*kpMvComp).iRefIndexCache[kuiTopIdx] as i32;
    let iRightTopRef = (*kpMvComp).iRefIndexCache[kuiTopIdx + iPartW as usize] as i32;
    let iDiagonalRef: i32;

    let sMvA = (*kpMvComp).sMotionVectorCache[kuiLeftIdx];
    let sMvB = (*kpMvComp).sMotionVectorCache[kuiTopIdx];
    let sMvC: SMVUnitXY;

    if REF_NOT_AVAIL as i32 == iRightTopRef {
        iDiagonalRef = (*kpMvComp).iRefIndexCache[kuiTopIdx - 1] as i32;
        sMvC = (*kpMvComp).sMotionVectorCache[kuiTopIdx - 1];
    } else {
        iDiagonalRef = iRightTopRef;
        sMvC = (*kpMvComp).sMotionVectorCache[kuiTopIdx + iPartW as usize];
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

pub unsafe extern "C" fn PredSkipMv(pMbCache: *mut SMbCache, sMvp: &mut SMVUnitXY) {
    let kpMvComp = &(*pMbCache).sMvComponents;
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

pub unsafe extern "C" fn PredInter16x8Mv(pMbCache: *mut SMbCache, iPartIdx: i32, iRef: i8, sMvp: &mut SMVUnitXY) {
    let kpMvComp = &(*pMbCache).sMvComponents;
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

pub unsafe extern "C" fn PredInter8x16Mv(pMbCache: *mut SMbCache, iPartIdx: i32, iRef: i8, sMvp: &mut SMVUnitXY) {
    let kpMvComp = &(*pMbCache).sMvComponents;
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

pub unsafe extern "C" fn UpdateP16x16MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    // The entry guard that stood here was the port's own — `mv_pred.cpp:148` opens
    // straight at `SMVComponentUnit* pMvComp = &pMbCache->sMvComponents;`. Two of its
    // three clauses were about parameters that are references now and cannot be null;
    // the third named `pMv`, which every caller spells `&mut …sMe.sMe16x16.sMv`.
    let pMvComp = &mut (*pMbCache).sMvComponents;
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

/// `ST16 (&pMvComp->iRefIndexCache[k], kuiRef16)`.
#[inline]
unsafe fn st16_ref_cache(pCache: *mut i8, k: usize, kuiRef16: u16) {
    (pCache.add(k) as *mut u16).write_unaligned(kuiRef16);
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
/// # Safety
/// `pMbCache`, `pCurMb` (with `sMv`/`pRefIndex` allocated) and `pMv` must be valid;
/// `kiPartIdx` must be 0 or 8.
pub unsafe extern "C" fn UpdateP16x8MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
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

    let pRefCache = pMvComp.iRefIndexCache.as_mut_ptr();
    *pRefCache.add(kiCacheIdx) = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 1, kuiRef16);
    *pRefCache.add(kiCacheIdx + 3) = kiRef;
    *pRefCache.add(kiCacheIdx + 6) = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 7, kuiRef16);
    *pRefCache.add(kiCacheIdx + 9) = kiRef;

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
///
/// # Safety
/// As [`UpdateP16x8MotionInfo`]; `kiPartIdx` must be 0 or 4.
pub unsafe extern "C" fn update_P8x16_motion_info(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
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

    let pRefCache = pMvComp.iRefIndexCache.as_mut_ptr();
    *pRefCache.add(kiCacheIdx) = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 1, kuiRef16);
    *pRefCache.add(kiCacheIdx + 3) = kiRef;
    *pRefCache.add(kiCacheIdx + 12) = kiRef;
    st16_ref_cache(pRefCache, kiCacheIdx + 13, kuiRef16);
    *pRefCache.add(kiCacheIdx + 15) = kiRef;

    let pMvCache = &mut pMvComp.sMotionVectorCache;
    pMvCache[kiCacheIdx] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 1, *pMv);
    pMvCache[kiCacheIdx + 3] = *pMv;
    pMvCache[kiCacheIdx + 12] = *pMv;
    st64_mv(pMvCache, kiCacheIdx + 13, *pMv);
    pMvCache[kiCacheIdx + 15] = *pMv;
}

/// `mv_pred.cpp:279`. P8x8.
///
/// # Safety
/// As [`UpdateP16x8MotionInfo`].
pub unsafe extern "C" fn UpdateP8x8MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;

    let pMbMv = &mut (*pCurMb).sMv;
    st64_mv_mb(pMbMv, kiScan4Idx, *pMv);
    st64_mv_mb(pMbMv, 4 + kiScan4Idx, *pMv);

    let pRefCache = pMvComp.iRefIndexCache.as_mut_ptr();
    *pRefCache.add(kiCacheIdx) = kiRef;
    *pRefCache.add(kiCacheIdx + 1) = kiRef;
    *pRefCache.add(kiCacheIdx + 6) = kiRef;
    *pRefCache.add(kiCacheIdx + 7) = kiRef;

    let pMvCache = &mut pMvComp.sMotionVectorCache;
    pMvCache[kiCacheIdx] = *pMv;
    pMvCache[kiCacheIdx + 1] = *pMv;
    pMvCache[kiCacheIdx + 6] = *pMv;
    pMvCache[kiCacheIdx + 7] = *pMv;
}

/// `mv_pred.cpp:305`. P4x4.
///
/// # Safety
/// As [`UpdateP16x8MotionInfo`].
pub unsafe extern "C" fn UpdateP4x4MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;

    (*pCurMb).sMv[kiScan4Idx] = *pMv;
    pMvComp.iRefIndexCache[kiCacheIdx] = kiRef;
    pMvComp.sMotionVectorCache[kiCacheIdx] = *pMv;
}

/// `mv_pred.cpp:318`. P8x4.
///
/// # Safety
/// As [`UpdateP16x8MotionInfo`].
pub unsafe extern "C" fn UpdateP8x4MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;

    (*pCurMb).sMv[kiScan4Idx] = *pMv;
    (*pCurMb).sMv[1 + kiScan4Idx] = *pMv;
    pMvComp.iRefIndexCache[kiCacheIdx] = kiRef;
    pMvComp.iRefIndexCache[1 + kiCacheIdx] = kiRef;
    pMvComp.sMotionVectorCache[kiCacheIdx] = *pMv;
    pMvComp.sMotionVectorCache[1 + kiCacheIdx] = *pMv;
}

/// `mv_pred.cpp:334`. P4x8.
///
/// # Safety
/// As [`UpdateP16x8MotionInfo`].
pub unsafe extern "C" fn UpdateP4x8MotionInfo(
    pMbCache: *mut SMbCache,
    pCurMb: &mut SMB,
    kiPartIdx: i32,
    kiRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kiScan4Idx = g_kuiMbCountScan4Idx[kiPartIdx as usize] as usize;
    let kiCacheIdx = g_kuiCache30ScanIdx[kiPartIdx as usize] as usize;

    (*pCurMb).sMv[kiScan4Idx] = *pMv;
    (*pCurMb).sMv[4 + kiScan4Idx] = *pMv;
    pMvComp.iRefIndexCache[kiCacheIdx] = kiRef;
    pMvComp.iRefIndexCache[6 + kiCacheIdx] = kiRef;
    pMvComp.sMotionVectorCache[kiCacheIdx] = *pMv;
    pMvComp.sMotionVectorCache[6 + kiCacheIdx] = *pMv;
}

/// `mv_pred.cpp:353`. Cache-only update for P16x8.
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP16x8Motion2Cache(
    pMbCache: *mut SMbCache,
    mut iPartIdx: i32,
    iRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
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
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP8x16Motion2Cache(
    pMbCache: *mut SMbCache,
    mut iPartIdx: i32,
    iRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
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
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP8x8Motion2Cache(
    pMbCache: *mut SMbCache,
    iPartIdx: i32,
    pRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    for k in [0usize, 1, 6, 7] {
        pMvComp.iRefIndexCache[kuiCacheIdx + k] = pRef;
        pMvComp.sMotionVectorCache[kuiCacheIdx + k] = *pMv;
    }
}

/// `mv_pred.cpp:407`. Cache-only update for P4x4.
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP4x4Motion2Cache(
    pMbCache: *mut SMbCache,
    iPartIdx: i32,
    pRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    pMvComp.iRefIndexCache[kuiCacheIdx] = pRef;
    pMvComp.sMotionVectorCache[kuiCacheIdx] = *pMv;
}

/// `mv_pred.cpp:416`. Cache-only update for P8x4.
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP8x4Motion2Cache(
    pMbCache: *mut SMbCache,
    iPartIdx: i32,
    pRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    pMvComp.iRefIndexCache[kuiCacheIdx] = pRef;
    pMvComp.iRefIndexCache[1 + kuiCacheIdx] = pRef;
    pMvComp.sMotionVectorCache[kuiCacheIdx] = *pMv;
    pMvComp.sMotionVectorCache[1 + kuiCacheIdx] = *pMv;
}

/// `mv_pred.cpp:427`. Cache-only update for P4x8.
///
/// # Safety
/// `pMbCache` and `pMv` must be valid.
pub unsafe extern "C" fn UpdateP4x8Motion2Cache(
    pMbCache: *mut SMbCache,
    iPartIdx: i32,
    pRef: i8,
    pMv: &mut SMVUnitXY,
) {
    let pMvComp = &mut (*pMbCache).sMvComponents;
    let kuiCacheIdx = g_kuiCache30ScanIdx[iPartIdx as usize] as usize;
    pMvComp.iRefIndexCache[kuiCacheIdx] = pRef;
    pMvComp.iRefIndexCache[6 + kuiCacheIdx] = pRef;
    pMvComp.sMotionVectorCache[kuiCacheIdx] = *pMv;
    pMvComp.sMotionVectorCache[6 + kuiCacheIdx] = *pMv;
}

pub unsafe extern "C" fn WelsMdI16x16(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pMbCache: *mut SMbCache,
    iLambda: i32,
) -> i32 {
    if pFunc.is_null() || pCurDqLayer.is_null() || pMbCache.is_null() {
        return i32::MAX;
    }
    // `svc_base_layer_md.cpp:369` reads pMemPredMb, not pMemPredLuma. The two are
    // equal on entry only because WelsMdIntraInit re-points pMemPredLuma at
    // pMemPredMb; this function then *moves* pMemPredLuma to the losing ping-pong
    // half before returning, so reading pMemPredLuma here would follow the previous
    // macroblock's pointer whenever WelsMdIntraInit had not just run.
    let pPredI16x16: [*mut u8; 2] = [crate::encoder::md::mem_pred_mb(pMbCache), crate::encoder::md::mem_pred_mb(pMbCache).add(256)];
    let mut pDst = pPredI16x16[0];
    let pDec = (*pMbCache).SPicData.pCsMb[0];
    let pEnc = (*pMbCache).SPicData.pEncMb[0];
    let iLineSizeDec = (*pCurDqLayer).iCsStride[0];
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
    let pfMdCost16x16 = (*pFunc).sSampleDealingFuncs.md_cost(BLOCK_16x16).unwrap();

    iBestMode = kpAvailMode[0] as i32;
    for i in 0..iAvailCount {
        let iCurMode = kpAvailMode[i] as i32;
        debug_assert!((0..7).contains(&iCurMode));

        (*pFunc).pfGetLumaI16x16Pred[iCurMode as usize].unwrap()(pDst, pDec, iLineSizeDec);
        let mut iCurCost = pfMdCost16x16(pDst, 16, pEnc, iLineSizeEnc);
        let mode_val = g_kiMapModeI16x16[iCurMode as usize] as u32;
        iCurCost += iLambda * (BsSizeUE(mode_val) as i32);
        if iCurCost < iBestCost {
            iBestMode = iCurMode;
            iBestCost = iCurCost;
            iIdx ^= 0x01;
            pDst = pPredI16x16[iIdx];
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
#[inline]
pub(crate) unsafe fn InitMe(
    iMbPixX: i32,
    iMbPixY: i32,
    pMvdCost: *mut u16,
    iBlockSize: i32,
    pEnc: *mut u8,
    pRef: *mut u8,
    // SCREEN_CONTENT(dormant: Phase 10)
    pRefFeatureStorage: *mut SScreenBlockFeatureStorage,
    sWelsMe: &mut SWelsME,
) {
    (*sWelsMe).iCurMeBlockPixX = iMbPixX;
    (*sWelsMe).iCurMeBlockPixY = iMbPixY;
    (*sWelsMe).uiBlockSize = iBlockSize as u8;
    (*sWelsMe).pMvdCost = pMvdCost;

    (*sWelsMe).pEncMb = pEnc;
    (*sWelsMe).pRefMb = pRef;
    (*sWelsMe).pColoRefMb = pRef;

    (*sWelsMe).pRefFeatureStorage = pRefFeatureStorage;
}

pub unsafe extern "C" fn WelsMdP16x16(
    pFunc: *mut SWelsFuncPtrList,
    pCurLayer: *mut SDqLayer,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pMe16x16 = &mut (*pWelsMd).sMe.sMe16x16;
    let uiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let kiMbWidth: i32 = (*pCurLayer).iMbWidth as i32;
    let kiMbHeight: i32 = (*pCurLayer).iMbHeight as i32;
    // `svc_base_layer_md.cpp:983`. This call was missing: without it the search block
    // kept the previous macroblock's pEncMb/pRefMb/uiBlockSize/pMvdCost.
    InitMe(
        (*pWelsMd).iMbPixX,
        (*pWelsMd).iMbPixY,
        (*pWelsMd).pMvdCost,
        BLOCK_16x16 as i32,
        (*pMbCache).SPicData.pEncMb[0],
        (*pMbCache).SPicData.pRefMb[0],
        (*pCurLayer).sRefPicView.pScreenBlockFeatureStorage,
        pMe16x16,
    );
    //not putting the line below into InitMe to avoid judging mode in InitMe
    (*pMe16x16).uSadPredISatd.uiSadPred = (*pWelsMd).iSadPredMb as u32;

    (*pSlice).uiMvcNum = 0;
    (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*pMe16x16).sMvBase;
    (*pSlice).uiMvcNum += 1;

    if (uiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        let left_mb = pCurMb.offset(-1);
        if !left_mb.is_null() {
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*left_mb).sP16x16Mv;
            (*pSlice).uiMvcNum += 1;
        }
    }
    if (uiNeighborAvail & TOP_MB_POS as u32) != 0 {
        let top_mb = pCurMb.offset(-(kiMbWidth as isize));
        if !top_mb.is_null() {
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize] = (*top_mb).sP16x16Mv;
            (*pSlice).uiMvcNum += 1;
        }
    }

    if ((*pCurLayer).pRefPic.is_some() && (*pCurLayer).sRefPicView.iPictureType == P_SLICE) {
        if ((*pCurMb).iMbX as i32) < kiMbWidth - 1 {
            let sTempMv =
                layer_ref_pic(pCurLayer).expect("bound").sMvList[((*pCurMb).iMbXY + 1) as usize];
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
        if ((*pCurMb).iMbY as i32) < kiMbHeight - 1 {
            let sTempMv = layer_ref_pic(pCurLayer).expect("bound").sMvList
                [((*pCurMb).iMbXY + kiMbWidth) as usize];
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvX = sTempMv.iMvX >> (*pSlice).sScaleShift;
            (*pSlice).sMvc[(*pSlice).uiMvcNum as usize].iMvY = sTempMv.iMvY >> (*pSlice).sScaleShift;
            (*pSlice).uiMvcNum += 1;
        }
    }

    PredMv(
        &(*pMbCache).sMvComponents as *const SMVComponentUnit,
        0,
        4,
        0,
        &mut (*pMe16x16).sMvp,
    );

    if let Some(search_fn) = (*pFunc).pfMotionSearch[0] {
        search_fn(pFunc, pCurLayer, pMe16x16, pSlice);
    }

    (*pCurMb).sP16x16Mv = (*pMe16x16).sMv;
    // `is_empty()` is the port's spelling of the C++'s null test: a picture built
    // without `bNeedMbInfo` carries no MV list at all (T6.F0).
    if layer_dec_pic(pCurLayer).map_or(false, |p| !p.sMvList.is_empty()) {
        (&mut layer_dec_pic_mut(pCurLayer).expect("bound").sMvList)[(*pCurMb).iMbXY as usize] = (*pMe16x16).sMv;
    }

    (*pMe16x16).uiSatdCost as i32
}

pub unsafe extern "C" fn WelsMdP8x8(
    pFunc: *mut SWelsFuncPtrList,
    pCurDqLayer: *mut SDqLayer,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
) -> i32 {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let iLineSizeEnc = (*pCurDqLayer).iEncStride[0];
    let iLineSizeRef = if let Some(p) = layer_ref_pic(pCurDqLayer) {
        p.stride(0)
    } else {
        iLineSizeEnc
    };

    let mut iCostP8x8 = 0i32;
    for i in 0..4 {
        let iIdxX = i & 1;
        let iIdxY = i >> 1;
        let iPixelX = iIdxX << 3;
        let iPixelY = iIdxY << 3;
        let iStrideEnc = iPixelX + (iPixelY * iLineSizeEnc);
        let iStrideRef = iPixelX + (iPixelY * iLineSizeRef);

        let sMe8x8 = &mut (*pWelsMd).sMe.sMe8x8[i as usize];
        // `svc_base_layer_md.cpp:1096`. The InitMe call, the two block-pixel offsets,
        // the SAD predictor, the sMvc seed, the static-idc-selected search function
        // and the cache update were all missing.
        InitMe(
            (*pWelsMd).iMbPixX,
            (*pWelsMd).iMbPixY,
            (*pWelsMd).pMvdCost,
            BLOCK_8x8 as i32,
            (*pMbCache).SPicData.pEncMb[0].offset(iStrideEnc as isize),
            (*pMbCache).SPicData.pRefMb[0].offset(iStrideRef as isize),
            (*pCurDqLayer).sRefPicView.pScreenBlockFeatureStorage,
            sMe8x8,
        );
        //not putting these three lines below into InitMe to avoid judging mode in InitMe
        (*sMe8x8).iCurMeBlockPixX = (*pWelsMd).iMbPixX + iPixelX;
        (*sMe8x8).iCurMeBlockPixY = (*pWelsMd).iMbPixY + iPixelY;
        (*sMe8x8).uSadPredISatd.uiSadPred = ((*pWelsMd).iSadPredMb >> 2) as u32;

        (*pSlice).sMvc[0] = (*sMe8x8).sMvBase;
        (*pSlice).uiMvcNum = 1;

        PredMv(
            &(*pMbCache).sMvComponents as *const SMVComponentUnit,
            (i << 2) as i8,
            2,
            (*pWelsMd).uiRef as i32,
            &mut (*sMe8x8).sMvp,
        );

        (*pFunc).pfMotionSearch[(*pWelsMd).iBlock8x8StaticIdc[i as usize] as usize]
            .expect("pfMotionSearch unset")(pFunc, pCurDqLayer, sMe8x8, pSlice);
        UpdateP8x8Motion2Cache(
            pMbCache,
            i << 2,
            (*pWelsMd).uiRef as i8,
            &mut (*sMe8x8).sMv,
        );
        iCostP8x8 += (*sMe8x8).uiSatdCost as i32;
    }
    iCostP8x8
}

pub unsafe extern "C" fn WelsInterMbEncode(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: &mut SMB) {
    // Port-added guard deleted with the retyping: `svc_encode_slice.cpp:458` opens at
    // `SMbCache* pMbCache = &pSlice->sMbCacheInfo;` and checks nothing.
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);
    let pFuncList = ctx_func_list(pEncCtx);
    if pCurDqLayer.is_null() || pFuncList.is_null() {
        return;
    }

    let pCoeffLevel = crate::encoder::md::coeff_level(pMbCache);
    let pEncMb = (*pMbCache).SPicData.pEncMb[0];
    let iEncStride = (*pCurDqLayer).iEncStride[0];
    let pMemPredLuma = crate::encoder::md::mem_pred_luma(pMbCache);

    if !pCoeffLevel.is_null() && !pEncMb.is_null() && !pMemPredLuma.is_null() {
        if let Some(dct_fn) = (*pFuncList).pfDctFourT4 {
            dct_fn(pCoeffLevel, pEncMb, iEncStride, pMemPredLuma, 16);
            dct_fn(pCoeffLevel.add(64), pEncMb.add(8), iEncStride, pMemPredLuma.add(8), 16);
            dct_fn(pCoeffLevel.add(128), pEncMb.add((8 * iEncStride) as usize), iEncStride, pMemPredLuma.add(128), 16);
            dct_fn(pCoeffLevel.add(192), pEncMb.add((8 * iEncStride + 8) as usize), iEncStride, pMemPredLuma.add(136), 16);
        }
    }

    WelsEncInterY(
        pFuncList as *mut crate::encoder::svc_encode_mb::SWelsFuncPtrList,
        pCurMb,
        pMbCache,
    );
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
pub unsafe extern "C" fn GetRefMb(pEncCtx: *mut sWelsEncCtx, pCurMb: &mut SMB) -> *mut SMB {
    let kRefIdx = (*current_layer(pEncCtx))
        .pRefLayer
        .expect("GetRefMb on a layer with no base layer: bBaseLayerAvailableFlag gates every caller");
    let kpRefLayer = ctx_dq_layer(pEncCtx, kRefIdx.get());
    let kiRefMbIdx =
        (((*pCurMb).iMbY as i32 >> 1) * (*kpRefLayer).iMbWidth as i32) + ((*pCurMb).iMbX as i32 >> 1);
    crate::encoder::svc_encode_slice::mb_at(kpRefLayer, kiRefMbIdx)
}

/// Scales base-layer motion vectors by 2x to initialize enhancement-layer candidates.
pub unsafe extern "C" fn SetMvBaseEnhancelayer(
    pMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    kpRefMb: *const SMB,
) {
    let kuiRefMbType = (*kpRefMb).uiMbType;

    if !IS_SVC_INTRA(kuiRefMbType) {
        let iRefMbPartIdx =
            ((((*pCurMb).iMbY as i32 & 0x01) << 1) + ((*pCurMb).iMbX as i32 & 0x01)) as usize;
        let iScan4RefPartIdx = g_kuiMbCountScan4Idx[iRefMbPartIdx << 2] as isize;

        let ref_mv = (*kpRefMb).sMv[(iScan4RefPartIdx) as usize];
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
pub unsafe extern "C" fn WelsMdSpatialelInterMbIlfmdNoilp(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
    kuiRefMbType: Mb_Type,
) {
    let pCurDqLayer = current_layer(pEncCtx);
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);

    let kuiNeighborAvail = (*pCurMb).uiNeighborAvail as u32;
    let kiMbWidth: i32 = (*pCurDqLayer).iMbWidth as i32;
    // F14's class: formed before the availability guards below, read only under them.
    let kpTopMb = pCurMb.wrapping_offset(-(kiMbWidth as isize));

    let kbMbLeftAvailPskip = if (kuiNeighborAvail & LEFT_MB_POS as u32) != 0 {
        IS_SKIP((*pCurMb.offset(-1)).uiMbType)
    } else {
        false
    };
    let kbMbTopAvailPskip = if (kuiNeighborAvail & TOP_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb).uiMbType)
    } else {
        false
    };
    let kbMbTopLeftAvailPskip = if (kuiNeighborAvail & TOPLEFT_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb.offset(-1)).uiMbType)
    } else {
        false
    };
    let kbMbTopRightAvailPskip = if (kuiNeighborAvail & TOPRIGHT_MB_POS as u32) != 0 {
        IS_SKIP((*kpTopMb.offset(1)).uiMbType)
    } else {
        false
    };

    let bTrySkip =
        kbMbLeftAvailPskip | kbMbTopAvailPskip | kbMbTopLeftAvailPskip | kbMbTopRightAvailPskip;
    let mut bKeepSkip = kbMbLeftAvailPskip & kbMbTopAvailPskip & kbMbTopRightAvailPskip;
    let bSkip: bool;

    if let Some(pfBgd) = (*ctx_func_list(pEncCtx)).pfInterMdBackgroundDecision {
        if pfBgd(pEncCtx, pWelsMd, pSlice, &mut *pCurMb, &mut bKeepSkip) {
            return;
        }
    }

    // Step 1: Try SKIP
    bSkip = WelsMdInterJudgePskip(pEncCtx, pWelsMd, pSlice, &mut *pCurMb, bTrySkip);

    if bSkip && bKeepSkip {
        WelsMdInterDecidedPskip(pEncCtx, pSlice, &mut *pCurMb);
        return;
    }

    if !IS_SVC_INTRA(kuiRefMbType) {
        if !bSkip {
            PredictSad(
                (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
                (*pMbCache).iSadCost.as_mut_ptr(),
                0,
                &mut (*pWelsMd).iSadPredMb,
            );

            // Step 2: P_16x16
            (*pWelsMd).iCostLuma =
                WelsMdP16x16(ctx_func_list(pEncCtx), pCurDqLayer, pWelsMd, pSlice, pCurMb);
            (*pCurMb).uiMbType = MB_TYPE_16x16;
        }

        WelsMdInterSecondaryModesEnc(pEncCtx, pWelsMd, pSlice, &mut *pCurMb, bSkip);
    } else {
        // Base layer is Intra (BLMODE == SVC_INTRA)
        let kiCostI16x16 = WelsMdI16x16(
            ctx_func_list(pEncCtx),
            current_layer(pEncCtx),
            pMbCache,
            (*pWelsMd).iLambda,
        );
        if bSkip && ((*pWelsMd).iCostLuma <= kiCostI16x16) {
            WelsMdInterDecidedPskip(pEncCtx, pSlice, &mut *pCurMb);
        } else {
            (*pWelsMd).iCostLuma = kiCostI16x16;
            (*pCurMb).uiMbType = MB_TYPE_INTRA16x16;

            WelsMdIntraSecondaryModesEnc(pEncCtx, pWelsMd, &mut *pCurMb, pMbCache);
        }
    }
}

/// Top-level MD entry point for spatial enhancement layer inter MBs.
pub unsafe extern "C" fn WelsMdInterMbEnhancelayer(
    pEncCtx: *mut sWelsEncCtx,
    pMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: *mut SMB,
) {
    let kpInterLayerRefMb = GetRefMb(pEncCtx, &mut *pCurMb);
    let kuiInterLayerRefMbType = (*kpInterLayerRefMb).uiMbType;

    SetMvBaseEnhancelayer(pMd, &mut *pCurMb, kpInterLayerRefMb);
    WelsMdSpatialelInterMbIlfmdNoilp(pEncCtx, pMd, pSlice, pCurMb, kuiInterLayerRefMbType);
}

// ============================================================================
// 2. Background Detection (BGD) P-Skip Mode Decision & Chroma Verification
// ============================================================================

#[inline(always)]
/// `svc_mode_decision.cpp:161`. Every pointer is non-const in C++.
pub unsafe fn GetChromaCost(
    pCalculateFunc: *mut Option<PSampleSadSatdCostFunc>,
    pSrcChroma: *mut u8,
    iSrcStride: i32,
    pRefChroma: *mut u8,
    iRefStride: i32,
) -> i32 {
    let func = *pCalculateFunc.add(BLOCK_8x8);
    if let Some(f) = func {
        f(pSrcChroma, iSrcStride, pRefChroma, iRefStride)
    } else {
        0
    }
}

#[inline(always)]
pub unsafe fn IsCostLessEqualSkipCost(
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

pub unsafe fn CheckChromaCost(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pMbCache: *mut SMbCache,
    iCurMbXy: i32,
) -> bool {
    let pSad = (*ctx_func_list(pEncCtx)).sSampleDealingFuncs.pfSampleSad.as_mut_ptr();
    let pCurDqLayer = current_layer(pEncCtx);

    let pCbEnc = (*pMbCache).SPicData.pEncMb[1];
    let pCrEnc = (*pMbCache).SPicData.pEncMb[2];
    let pCbRef = (*pMbCache).SPicData.pRefMb[1];
    let pCrRef = (*pMbCache).SPicData.pRefMb[2];

    let iCbEncStride = (*pCurDqLayer).iEncStride[1];
    let iCrEncStride = (*pCurDqLayer).iEncStride[2];
    let iChromaRefStride = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[1];

    let iCbSad = GetChromaCost(pSad, pCbEnc, iCbEncStride, pCbRef, iChromaRefStride);
    let iCrSad = GetChromaCost(pSad, pCrEnc, iCrEncStride, pCrRef, iChromaRefStride);

    let bChromaTooLarge = iCbSad > KNOWN_CHROMA_TOO_LARGE || iCrSad > KNOWN_CHROMA_TOO_LARGE;
    let iChromaSad = iCbSad + iCrSad;

    PredictSadSkip(
        (*pMbCache).sMvComponents.iRefIndexCache.as_mut_ptr(),
        (*pMbCache).bMbTypeSkip.as_mut_ptr(),
        (*pMbCache).iSadCostSkip.as_mut_ptr(),
        0,
        &mut (*pWelsMd).iSadPredSkip,
    );

    let bChromaCostCannotSkip = IsCostLessEqualSkipCost(
        iChromaSad,
        (*pWelsMd).iSadPredSkip,
        (*pMbCache).uiRefMbType,
        layer_ref_pic(pCurDqLayer),
        iCurMbXy,
        SMALLEST_INVISIBLE,
    );

    !bChromaCostCannotSkip && !bChromaTooLarge
}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
    bKeepSkip: *mut bool,
) -> bool {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);

    let kiRefMbQp = (&layer_ref_pic(pCurDqLayer).expect("bound").pRefMbQp)[(*pCurMb).iMbXY as usize] as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;
    let pVaaBgMbFlag = (*ctx_vaa(pEncCtx)).pVaaBackgroundMbFlag.as_mut_ptr().add((*pCurMb).iMbXY as usize);

    let kiMbWidth: isize = (*pCurDqLayer).iMbWidth as isize;

    *bKeepSkip = *bKeepSkip
        && (*pVaaBgMbFlag.offset(-1) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth) == 0)
        && (*pVaaBgMbFlag.offset(-kiMbWidth + 1) == 0);

    if *pVaaBgMbFlag != 0
        && !IS_INTRA((*pMbCache).uiRefMbType)
        && ((kiRefMbQp - kiCurMbQp <= DELTA_QP_BGD_THD) || (kiRefMbQp <= 26))
    {
        if CheckChromaCost(pEncCtx, pWelsMd, pMbCache, (*pCurMb).iMbXY) {
            let mut sVaaPredSkipMv = SMVUnitXY::default();
            PredSkipMv(pMbCache, &mut sVaaPredSkipMv);
            let bZeroMv = sVaaPredSkipMv.iMvX == 0 && sVaaPredSkipMv.iMvY == 0;
            WelsMdBackgroundMbEnc(pEncCtx, pWelsMd, pCurMb, pSlice, bZeroMv);
            return true;
        }
    }

    false
}

pub unsafe extern "C" fn WelsMdInterJudgeBGDPskipFalse(
    _pCtx: *mut sWelsEncCtx,
    _pMd: &mut SWelsMD,
    _pSlice: *mut SSlice,
    _pCurMb: &mut SMB,
    _bKeepSkip: *mut bool,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfo(
    pCurLayer: *mut SDqLayer,
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
    (&mut layer_dec_pic_mut(pCurLayer).expect("bound").pRefMbQp)[kiMbXY] = uiQp;

    if (*pCurMb).uiMbType == MB_TYPE_BACKGROUND {
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
    }
}

pub unsafe extern "C" fn WelsMdUpdateBGDInfoNULL(
    pCurLayer: *mut SDqLayer,
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
pub unsafe fn IsMbStatic(pBlockType: *const i32, eType: EStaticBlockIdc) -> bool {
    if pBlockType.is_null() {
        return false;
    }
    let target = eType as i32;
    *pBlockType == target
        && *pBlockType.add(1) == target
        && *pBlockType.add(2) == target
        && *pBlockType.add(3) == target
}

#[inline(always)]
pub unsafe fn IsMbCollocatedStatic(pBlockType: *const i32) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::COLLOCATED_STATIC)
}

#[inline(always)]
pub unsafe fn IsMbScrolledStatic(pBlockType: *const i32) -> bool {
    IsMbStatic(pBlockType, EStaticBlockIdc::SCROLLED_STATIC)
}

#[inline(always)]
pub unsafe fn CalUVSadCost(
    pFunc: *mut SWelsFuncPtrList,
    pEncOri: *mut u8,
    iStrideUV: i32,
    pRefOri: *mut u8,
    iRefLineSize: i32,
) -> i32 {
    let f = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_8x8];
    if let Some(sad_func) = f {
        sad_func(pEncOri, iStrideUV, pRefOri, iRefLineSize)
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

pub unsafe extern "C" fn JudgeStaticSkip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: &mut SWelsMD,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;

    let mut bTryStaticSkip = IsMbCollocatedStatic((*pWelsMd).iBlock8x8StaticIdc.as_ptr());
    if bTryStaticSkip {
        let pFunc = ctx_func_list(pEncCtx);
        let pRefOri = (*pCurDqLayer).pRefOri[0]
            .and_then(|r| crate::encoder::svc_encode_slice::ctx_pic_ref_mut(pEncCtx, r))
            .map(|p| p.planes());
        if let Some(pRefOri) = pRefOri {
            let iStrideUV = (*pCurDqLayer).iEncStride[1];
            let iOffsetUV = (kiMbX + kiMbY * iStrideUV) << 3;

            let iSadCostCb = CalUVSadCost(
                pFunc,
                (*pMbCache).SPicData.pEncMb[1],
                iStrideUV,
                pRefOri.pData[1].offset(iOffsetUV as isize),
                pRefOri.iLineSize[1],
            );
            if iSadCostCb == 0 {
                let iSadCostCr = CalUVSadCost(
                    pFunc,
                    (*pMbCache).SPicData.pEncMb[2],
                    iStrideUV,
                    pRefOri.pData[2].offset(iOffsetUV as isize),
                    pRefOri.iLineSize[1],
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

pub unsafe extern "C" fn JudgeScrollSkip(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    pMbCache: *mut SMbCache,
    pWelsMd: &mut SWelsMD,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    let kiMbX = (*pCurMb).iMbX as i32;
    let kiMbY = (*pCurMb).iMbY as i32;
    let kiMbWidth: i32 = (*pCurDqLayer).iMbWidth as i32;
    let kiMbHeight: i32 = (*pCurDqLayer).iMbHeight as i32;
    let pVaaExt = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt_t;

    let mut bTryScrollSkip;
    if (*pVaaExt).sScrollDetectInfo.bScrollDetectFlag {
        bTryScrollSkip = IsMbScrolledStatic((*pWelsMd).iBlock8x8StaticIdc.as_ptr());
    } else {
        return false;
    }

    if bTryScrollSkip {
        let pFunc = ctx_func_list(pEncCtx);
        let pRefOri = (*pCurDqLayer).pRefOri[0]
            .and_then(|r| crate::encoder::svc_encode_slice::ctx_pic_ref_mut(pEncCtx, r))
            .map(|p| p.planes());
        if let Some(pRefOri) = pRefOri {
            let iScrollMvX = (*pVaaExt).sScrollDetectInfo.iScrollMvX;
            let iScrollMvY = (*pVaaExt).sScrollDetectInfo.iScrollMvY;
            if CheckBorder(kiMbX, kiMbY, iScrollMvX, iScrollMvY, kiMbWidth, kiMbHeight) {
                bTryScrollSkip = false;
            } else {
                let iStrideUV = (*pCurDqLayer).iEncStride[1];
                let iOffsetUV = (kiMbX << 3)
                    + (iScrollMvX >> 1)
                    + (((kiMbY << 3) + (iScrollMvY >> 1)) * iStrideUV);

                let iSadCostCb = CalUVSadCost(
                    pFunc,
                    (*pMbCache).SPicData.pEncMb[1],
                    iStrideUV,
                    pRefOri.pData[1].offset(iOffsetUV as isize),
                    pRefOri.iLineSize[1],
                );
                if iSadCostCb == 0 {
                    let iSadCostCr = CalUVSadCost(
                        pFunc,
                        (*pMbCache).SPicData.pEncMb[2],
                        iStrideUV,
                        pRefOri.pData[2].offset(iOffsetUV as isize),
                        pRefOri.iLineSize[1],
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

pub unsafe extern "C" fn SvcMdSCDMbEnc(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pSlice: *mut SSlice,
    bQpSimilarFlag: bool,
    bMbSkipFlag: bool,
    sCurMbMv: &[SMVUnitXY; 2],
    eSkipMode: ESkipModes,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);
    let pFunc = ctx_func_list(pEncCtx);
    let skip_idx = eSkipMode as usize;
    let sCandidateMv = sCurMbMv[skip_idx];

    let sMvp = SMVUnitXY {
        iMvX: sCandidateMv.iMvX,
        iMvY: sCandidateMv.iMvY,
    };

    let pRefLuma = (*pMbCache).SPicData.pRefMb[0];
    let pRefCb = (*pMbCache).SPicData.pRefMb[1];
    let pRefCr = (*pMbCache).SPicData.pRefMb[2];
    let iLineSizeY = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[0];
    let iLineSizeUV = (*pCurDqLayer).sRefPicView.sPlanes.iLineSize[1];

    let mut pDstLuma = crate::encoder::md::skip_mb(pMbCache);
    let mut pDstCb = crate::encoder::md::skip_mb(pMbCache).add(256);
    let mut pDstCr = crate::encoder::md::skip_mb(pMbCache).add(256 + 64);

    let iOffsetY = (sCandidateMv.iMvX as i32 >> 2) + (sCandidateMv.iMvY as i32 >> 2) * iLineSizeY;
    let iOffsetUV = (sCandidateMv.iMvX as i32 >> 3) + (sCandidateMv.iMvY as i32 >> 3) * iLineSizeUV;

    if !bQpSimilarFlag || !bMbSkipFlag {
        pDstLuma = crate::encoder::md::mem_pred_luma(pMbCache);
        pDstCb = crate::encoder::md::mem_pred_chroma(pMbCache);
        pDstCr = crate::encoder::md::mem_pred_chroma(pMbCache).add(64);
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

    let sad_16x16 = (*pFunc).sSampleDealingFuncs.pfSampleSad[BLOCK_16x16].unwrap();
    let sad_cost = sad_16x16(
        (*pMbCache).SPicData.pEncMb[0],
        (*pCurDqLayer).iEncStride[0],
        pRefLuma.offset(iOffsetY as isize),
        iLineSizeY,
    );
    (*pCurMb).iSadCost = sad_cost;
    (*pWelsMd).iCostSkipMb = sad_cost;

    (*pCurMb).sP16x16Mv = sCandidateMv;
    (&mut layer_dec_pic_mut(pCurDqLayer).expect("bound").sMvList)[(*pCurMb).iMbXY as usize] = sCandidateMv;

    if bQpSimilarFlag && bMbSkipFlag {
        (*pCurMb).iRefIndex = [0; MB_BLOCK8x8_NUM];
        if let Some(pfUpdateMbMv) = (*pFunc).pfUpdateMbMv {
            pfUpdateMbMv(&mut (*pCurMb).sMv, sMvp);
        }
        (*pCurMb).uiMbType = MB_TYPE_SKIP;
        WelsRecPskip(pCurDqLayer, pFunc, pCurMb, pMbCache);
        WelsMdInterUpdatePskip(pEncCtx, pCurDqLayer, pSlice, pCurMb);
        return;
    }

    (*pCurMb).uiMbType = MB_TYPE_16x16;

    (*pWelsMd).sMe.sMe16x16.sMv = sCandidateMv;
    PredMv(
        &(*pMbCache).sMvComponents,
        0,
        4,
        0,
        &mut (*pWelsMd).sMe.sMe16x16.sMvp,
    );
    (*pMbCache).sMbMvp[0] = (*pWelsMd).sMe.sMe16x16.sMvp;

    UpdateP16x16MotionInfo(pMbCache, pCurMb, 0, &mut (*pWelsMd).sMe.sMe16x16.sMv);

    if (*pWelsMd).bMdUsingSad {
        (*pWelsMd).iCostLuma = (*pCurMb).iSadCost;
    } else {
        (*pWelsMd).iCostLuma = sad_16x16(
            (*pMbCache).SPicData.pEncMb[0],
            (*pCurDqLayer).iEncStride[0],
            pRefLuma,
            iLineSizeY,
        );
    }

    WelsInterMbEncode(pEncCtx, pSlice, pCurMb);
    WelsPMbChromaEncode(
        pEncCtx as *mut crate::encoder::svc_encode_slice::sWelsEncCtx,
        pSlice as *mut crate::encoder::svc_encode_slice::SSlice,
        pCurMb,
    );

    if let Some(copy16) = (*pFunc).pfCopy16x16Aligned {
        copy16(
            (*pMbCache).SPicData.pCsMb[0],
            (*pCurDqLayer).iCsStride[0],
            crate::encoder::md::mem_pred_luma(pMbCache),
            16,
        );
    }
    if let Some(copy8) = (*pFunc).pfCopy8x8Aligned {
        copy8(
            (*pMbCache).SPicData.pCsMb[1],
            (*pCurDqLayer).iCsStride[1],
            crate::encoder::md::mem_pred_chroma(pMbCache),
            8,
        );
        copy8(
            (*pMbCache).SPicData.pCsMb[2],
            (*pCurDqLayer).iCsStride[1],
            crate::encoder::md::mem_pred_chroma(pMbCache).add(64),
            8,
        );
    }
}

pub unsafe extern "C" fn MdInterSCDPskipProcess(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
    eSkipMode: ESkipModes,
) -> bool {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pVaaExt = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt_t;
    let pCurDqLayer = current_layer(pEncCtx);

    let kiRefMbQp = (&layer_ref_pic(pCurDqLayer).expect("bound").pRefMbQp)[(*pCurMb).iMbXY as usize] as i32;
    let kiCurMbQp = (*pCurMb).uiLumaQp as i32;

    let pJudgeSkip: [pJudgeSkipFun; 2] = [JudgeStaticSkip, JudgeScrollSkip];
    let bSkipFlag = pJudgeSkip[eSkipMode as usize](pEncCtx, pCurMb, pMbCache, pWelsMd);

    if bSkipFlag {
        let bQpSimilarFlag = (kiRefMbQp - kiCurMbQp <= DELTA_QP_SCD_THD) || (kiRefMbQp <= 26);
        let mut sVaaPredSkipMv = SMVUnitXY::default();
        let mut sCurMbMv: [SMVUnitXY; 2] = [SMVUnitXY::default(), SMVUnitXY::default()];
        PredSkipMv(pMbCache, &mut sVaaPredSkipMv);

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

pub unsafe extern "C" fn SetBlockStaticIdcToMd(
    pVaaExt: *mut SVAAFrameInfoExt_t,
    pWelsMd: &mut SWelsMD,
    pCurMb: &mut SMB,
    pDqLayer: *mut SDqLayer,
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

pub unsafe extern "C" fn WelsMdInterJudgeSCDPskip(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: &mut SMB,
) -> bool {
    let pCurDqLayer = current_layer(pEncCtx);
    SetBlockStaticIdcToMd(ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt_t, pWelsMd, pCurMb, pCurDqLayer);

    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, ESkipModes::STATIC) {
        return true;
    }
    if MdInterSCDPskipProcess(pEncCtx, pWelsMd, slice, pCurMb, ESkipModes::SCROLLED) {
        return true;
    }

    false
}

pub unsafe extern "C" fn WelsMdInterJudgeSCDPskipFalse(
    _pEncCtx: *mut sWelsEncCtx,
    _pWelsMd: &mut SWelsMD,
    _slice: *mut SSlice,
    _pCurMb: &mut SMB,
) -> bool {
    false
}

pub unsafe extern "C" fn WelsInitSCDPskipFunc(
    pFuncList: *mut SWelsFuncPtrList,
    bScrollingDetection: bool,
) {
    if bScrollingDetection {
        (*pFuncList).pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskip);
    } else {
        (*pFuncList).pfSCDPSkipDecision = Some(WelsMdInterJudgeSCDPskipFalse);
    }
}

// ============================================================================
// 4. Sub-Macroblock Fine Partitioning & Mode Merging
// ============================================================================

#[inline(always)]
pub fn MergeSub16Me(sSrcMe0: &SWelsME, sSrcMe1: &SWelsME, pTarMe: &mut SWelsME) {
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

pub unsafe fn TryModeMerge(
    pMbCache: *mut SMbCache,
    pWelsMd: &mut SWelsMD,
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
            PredInter16x8Mv(pMbCache, 0, 0, &mut (*pWelsMd).sMe.sMe16x8[0].sMvp);
            PredInter16x8Mv(pMbCache, 8, 0, &mut (*pWelsMd).sMe.sMe16x8[1].sMvp);
        }
        1 => {
            (*pCurMb).uiMbType = MB_TYPE_8x16;
            MergeSub16Me(&*pMe8x8.add(0), &*pMe8x8.add(2), &mut (*pWelsMd).sMe.sMe8x16[0]);
            MergeSub16Me(&*pMe8x8.add(1), &*pMe8x8.add(3), &mut (*pWelsMd).sMe.sMe8x16[1]);
            PredInter8x16Mv(pMbCache, 0, 0, &mut (*pWelsMd).sMe.sMe8x16[0].sMvp);
            PredInter8x16Mv(pMbCache, 4, 0, &mut (*pWelsMd).sMe.sMe8x16[1].sMvp);
        }
        _ => {}
    }

    (*pCurMb).uiMbType != MB_TYPE_8x8
}

pub unsafe extern "C" fn WelsMdInterFinePartitionVaaOnScreen(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: &mut SWelsMD,
    pSlice: *mut SSlice,
    pCurMb: &mut SMB,
    mut iBestCost: i32,
) {
    let pMbCache = std::ptr::addr_of_mut!((*pSlice).sMbCacheInfo);
    let pCurDqLayer = current_layer(pEncCtx);

    let pSad8x8_ptr = (*ctx_vaa(pEncCtx))
        .sVaaCalcInfo
        .pSad8x8
        .as_mut_ptr()
        .add((*pCurMb).iMbXY as usize) as *mut i32;
    let get_sign = (*ctx_func_list(pEncCtx)).pfGetMbSignFromInterVaa.unwrap();
    let uiMbSign = get_sign(pSad8x8_ptr);

    if uiMbSign == MBVAASIGN_FLAT {
        return;
    }

    let iCostP8x8 = WelsMdP8x8(ctx_func_list(pEncCtx), pCurDqLayer, pWelsMd, pSlice);
    if iCostP8x8 < iBestCost {
        iBestCost = iCostP8x8;
        (*pCurMb).uiMbType = MB_TYPE_8x8;
        (*pCurMb).uiSubMbType = [SUB_MB_TYPE_8x8; 4];
        TryModeMerge(pMbCache, pWelsMd, pCurMb);
    }
    (*pWelsMd).iCostLuma = iBestCost;
}

// ============================================================================
// 5. Global Scrolling Motion Vector Dispatch
// ============================================================================

pub unsafe extern "C" fn SetScrollingMvToMd(pVaa: *mut SVAAFrameInfo, pWelsMd: &mut SWelsMD) {
    let pVaaExt = pVaa as *mut SVAAFrameInfoExt_t;
    let sTempMv = SMVUnitXY {
        iMvX: (*pVaaExt).sScrollDetectInfo.iScrollMvX as i16,
        iMvY: (*pVaaExt).sScrollDetectInfo.iScrollMvY as i16,
    };

    (*pWelsMd).sMe.sMe16x16.sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[0].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[1].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[2].sDirectionalMv = sTempMv;
    (*pWelsMd).sMe.sMe8x8[3].sDirectionalMv = sTempMv;
}

/// Intentional no-op mode decision scrolling MV callback.
/// Matches `void SetScrollingMvToMdNull (SVAAFrameInfo* pVaa, SWelsMD* pWelsMd)` in `svc_mode_decision.cpp:689`.
pub unsafe extern "C" fn SetScrollingMvToMdNull(_pVaa: *mut SVAAFrameInfo, _pWelsMd: &mut SWelsMD) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
            PredMv(&mv_comp as *const SMVComponentUnit, 0, 4, 0, &mut sMvp);

            // Median of (10, 30, 20) is 20; Median of (20, 40, 30) is 30
            assert_eq!(sMvp.iMvX, 20);
            assert_eq!(sMvp.iMvY, 30);
        }
    }

    #[test]
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
            PredSkipMv(&mut mb_cache, &mut sMvp);

            assert_eq!(sMvp.iMvX, 0);
            assert_eq!(sMvp.iMvY, 0);
        }
    }

    #[test]
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
            PredInter16x8Mv(&mut mb_cache, 0, 0, &mut sMvp);
            assert_eq!(sMvp.iMvX, 12);
            assert_eq!(sMvp.iMvY, 34);

            mb_cache.sMvComponents.iRefIndexCache[6] = 0; // Left ref for 8x16 part 0
            mb_cache.sMvComponents.sMotionVectorCache[6] = SMVUnitXY { iMvX: 56, iMvY: 78 };

            let mut sMvp8x16 = SMVUnitXY::default();
            PredInter8x16Mv(&mut mb_cache, 0, 0, &mut sMvp8x16);
            assert_eq!(sMvp8x16.iMvX, 56);
            assert_eq!(sMvp8x16.iMvY, 78);
        }
    }

    #[test]
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
                &mut mb_cache,
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

            // Reconstruction and source planes need a real border: the V/H/DC
            // predictors read pRef[-stride] and pRef[-1].
            const STRIDE: usize = 48;
            let mut cs_plane = vec![128u8; STRIDE * 40];
            let mut enc_plane = vec![128u8; STRIDE * 40];
            // Give the source a constant offset from the reconstruction so the SAD is
            // a known non-zero number: 16*16 pixels differing by 10.
            for y in 0..16 {
                for x in 0..16 {
                    enc_plane[(y + 16) * STRIDE + (x + 16)] = 138;
                }
            }
            // The prediction ping-pong is `SMbCache::sMemPredMb` since T6.C3 —
            // `[u8; 2 * 256 + 16]`, and the `+ 16` is F14's accommodation, documented
            // on the field. **This test is the instrument that keeps it**: delete the
            // `+ 16` and the raw 16x16 SAD's one-past-the-row pointer takes this test
            // red under Miri.
            let mut mb_cache = SMbCache {
                SPicData: SPicData {
                    pEncMb: [
                        enc_plane.as_mut_ptr().add(16 * STRIDE + 16),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ],
                    pDecMb: [std::ptr::null_mut(); 3],
                    pRefMb: [std::ptr::null_mut(); 3],
                    pCsMb: [
                        cs_plane.as_mut_ptr().add(16 * STRIDE + 16),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ],
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
                ..Default::default()
            };

            let iLambda = 10;
            let cost = WelsMdI16x16(
                &mut func_list as *mut SWelsFuncPtrList,
                &mut dq_layer as *mut SDqLayer,
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
            let pLuma = crate::encoder::md::mem_pred_luma(&mut mb_cache);
            let pChroma = crate::encoder::md::mem_pred_chroma(&mut mb_cache);
            let pPredBuf = std::ptr::addr_of_mut!(mb_cache.sMemPredMb).cast::<u8>();
            assert_eq!(pLuma, pPredBuf);
            assert_eq!(pChroma, pPredBuf.add(256));
            assert!(std::slice::from_raw_parts(pPredBuf, 256).iter().all(|&b| b == 128));
        }
    }

    #[test]
    fn test_svc_mode_decision_noop_callback() {
        // The MD argument used to be a null raw MD pointer; it is a `&mut` now, so
        // the null goes and a real record takes its place. The `pVaa` argument is
        // still raw (the preprocess family is session F's) and stays null: this
        // callback is the no-op arm of `PSetScrollingMv` and reads neither.
        let mut sMd = SWelsMD::default();
        unsafe {
            SetScrollingMvToMdNull(std::ptr::null_mut(), &mut sMd);
        }
    }
}
