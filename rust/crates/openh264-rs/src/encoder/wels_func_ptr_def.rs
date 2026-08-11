#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code
)]

//! Encoder function-pointer table.
//!
//! Translated from `codec/encoder/core/inc/wels_func_ptr_def.h`. `SWelsFuncPtrList`
//! previously existed as ten partial copies, the largest of which had 13 of its 70
//! members; it is 1280 bytes and every entry is dispatched through at encode time, so
//! a missing member silently shifts every later one.

use std::ffi::c_void;

use crate::common::expand_pic::SExpandPicFunc;
use crate::common::mc::SMcFunc;
use crate::encoder::deblocking::{DeblockingFunc, PSetNoneZeroCountZeroFunc};
use crate::encoder::encoder_context::{
    sWelsEncCtx, BLOCK_STATIC_IDC_ALL, BLOCK_SIZE_ALL, C_PRED_A, I16_PRED_DC_A, I4_PRED_A,
};
use crate::encoder::encode_mb_aux::{
    PCalculateSingleCtrFunc, PCopyFunc, PDctFunc, PGetNoneZeroCountFunc, PQuantizationDcFunc,
    PQuantizationFunc, PQuantizationHadamardFunc, PQuantizationMaxFunc, PQuantizationSkipFunc,
    PScanFunc, PTransformHadamard4x4Func,
};
use crate::encoder::md::{
    PFillInterNeighborCacheFunc, PGetMbSignFromInterVaaFunc, PGetVarianceFromIntraVaaFunc,
    PUpdateMbMvFunc, SSampleDealingFunc, SWelsMD, SMB,
};
use crate::encoder::md::SMbCache;
use crate::encoder::rc::{PGetBsPositionFunc, SWelsRcFunc};
use crate::encoder::svc_encode_mb::{PDeQuantizationFunc, PIDctFunc, PSetMemoryZero};
use crate::encoder::svc_encode_slice::{SDqLayer, SDynamicSlicingStack, SSlice};
use crate::encoder::svc_motion_estimate::{
    PCalculateBlockFeatureOfFrame, PCalculateSatdFunc, PCalculateSingleBlockFeature,
    PCheckDirectionalMv, PFillQpelLocationByFeatureValueFunc, PInitializeHashforFeatureFunc,
    PLineFullSearchFunc, PMotionSearchFunc, PSampleSadHor8Func, PSearchMethodFunc,
    PUpdateFMESwitch,
};
use crate::encoder::wels_preprocess::SVAAFrameInfo;

// ============================================================================
// Function pointer typedefs that had no Rust counterpart
// ============================================================================

/// `wels_func_ptr_def.h:178`. Note this is **not** the decoder's `PGetIntraPredFunc`,
/// which takes two arguments; the encoder's takes a separate reference pointer.
pub type PGetIntraPredFunc =
    unsafe extern "C" fn(pPrediction: *mut u8, pRef: *mut u8, kiStride: i32);

/// `wels_func_ptr_def.h:106`
pub type PIntraFineMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> i32;

/// `wels_func_ptr_def.h:107`
pub type PInterFineMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
    bestCost: i32,
);

/// `wels_func_ptr_def.h:108`
pub type PInterMdFirstIntraModeFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool;

/// `wels_func_ptr_def.h:111`
pub type PAccumulateSadFunc = unsafe extern "C" fn(
    pSumDiff: *mut u32,
    pGomForegroundBlockNum: *mut i32,
    iSad8x8: *mut i32,
    pVaaBgMbFlag: *mut i8,
);

/// `wels_func_ptr_def.h:116`
pub type PInterMdBackgroundDecisionFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
    pKeepPskip: *mut bool,
) -> bool;

/// `wels_func_ptr_def.h:118`
pub type PMdBackgroundInfoUpdateFunc = unsafe extern "C" fn(
    pCurLayer: *mut SDqLayer,
    pCurMb: *mut SMB,
    bFlag: bool,
    kiRefPictureType: i32,
);

/// `wels_func_ptr_def.h:121`
pub type PInterMdScrollingPSkipDecisionFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
) -> bool;

/// `wels_func_ptr_def.h:123`
pub type PSetScrollingMv =
    unsafe extern "C" fn(pVaa: *mut SVAAFrameInfo, pMd: *mut SWelsMD);

/// `wels_func_ptr_def.h:125`
pub type PInterMdFunc = unsafe extern "C" fn(
    pEncCtx: *mut sWelsEncCtx,
    pWelsMd: *mut SWelsMD,
    slice: *mut SSlice,
    pCurMb: *mut SMB,
    pMbCache: *mut SMbCache,
);

/// `wels_func_ptr_def.h:64`
pub type PDeQuantizationHadamardFunc = unsafe extern "C" fn(pRes: *mut i16, kuiMF: u16);

/// `wels_func_ptr_def.h:190`
pub type PCavlcParamCalFunc = unsafe extern "C" fn(
    pCoff: *mut i16,
    pRun: *mut u8,
    pLevel: *mut i16,
    pTotalCoeffs: *mut i32,
    iEndIdx: i32,
) -> i32;

/// `wels_func_ptr_def.h:192`
pub type PWelsSpatialWriteMbSyn =
    unsafe extern "C" fn(pCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: *mut SMB) -> i32;

/// `wels_func_ptr_def.h:193`
///
/// The `buf` parameter is T3.5's, and it is the one slot in this family that
/// could not avoid gaining one. The CAVLC pair genuinely needs no buffer — a
/// detached cursor is `Copy`, so its snapshot is a value (T3.4) — but the CABAC
/// pair must copy the emitted bytes out and back, because `PropagateCarry`
/// rewrites bytes behind the cursor and restoring the cursor alone would leave
/// the output wrong. Neither variant can reach the buffer from `pDss`/`pSlice`
/// alone, so it is passed. The CAVLC variants ignore it.
///
/// Phase 4b, which owns this signature, is folding both slots into the
/// `EntropyCoder` dispatch enum; this parameter goes away there for CAVLC and
/// stays for CABAC.
pub type PStashMBStatus = unsafe fn(
    buf: &mut [u8],
    pDss: *mut SDynamicSlicingStack,
    pSlice: *mut SSlice,
    iMbSkipRun: i32,
);

/// `wels_func_ptr_def.h:194`. See [`PStashMBStatus`] for why `buf` is here.
pub type PStashPopMBStatus =
    unsafe fn(buf: &mut [u8], pDss: *mut SDynamicSlicingStack, pSlice: *mut SSlice) -> i32;

// ============================================================================
// SWelsFuncPtrList
// ============================================================================

/// `TagWelsFuncPointerList` — `codec/encoder/core/inc/wels_func_ptr_def.h:198`.
/// **1280 bytes**, 70 members, in C++ declaration order.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SWelsFuncPtrList {
    pub sExpandPicFunc: SExpandPicFunc,
    pub pfFillInterNeighborCache: Option<PFillInterNeighborCacheFunc>,

    pub pfGetVarianceFromIntraVaa: Option<PGetVarianceFromIntraVaaFunc>,
    pub pfGetMbSignFromInterVaa: Option<PGetMbSignFromInterVaaFunc>,
    pub pfUpdateMbMv: Option<PUpdateMbMvFunc>,
    pub pfFirstIntraMode: Option<PInterMdFirstIntraModeFunc>,
    pub pfIntraFineMd: Option<PIntraFineMdFunc>,
    pub pfInterFineMd: Option<PInterFineMdFunc>,
    pub pfInterMd: Option<PInterMdFunc>,

    pub pfInterMdBackgroundDecision: Option<PInterMdBackgroundDecisionFunc>,
    pub pfMdBackgroundInfoUpdate: Option<PMdBackgroundInfoUpdateFunc>,

    pub pfSCDPSkipDecision: Option<PInterMdScrollingPSkipDecisionFunc>,
    pub pfSetScrollingMv: Option<PSetScrollingMv>,

    pub sMcFuncs: SMcFunc,
    pub sSampleDealingFuncs: SSampleDealingFunc,
    pub pfGetLumaI16x16Pred: [Option<PGetIntraPredFunc>; I16_PRED_DC_A],
    pub pfGetLumaI4x4Pred: [Option<PGetIntraPredFunc>; I4_PRED_A],
    pub pfGetChromaPred: [Option<PGetIntraPredFunc>; C_PRED_A],

    /// 1: for 16x16 square; 0: for 8x8 square
    pub pfSampleSadHor8: [Option<PSampleSadHor8Func>; 2],
    pub pfMotionSearch: [Option<PMotionSearchFunc>; BLOCK_STATIC_IDC_ALL],
    pub pfSearchMethod: [Option<PSearchMethodFunc>; BLOCK_SIZE_ALL],
    pub pfCalculateSatd: Option<PCalculateSatdFunc>,
    pub pfCheckDirectionalMv: Option<PCheckDirectionalMv>,

    pub pfInitializeHashforFeature: Option<PInitializeHashforFeatureFunc>,
    pub pfFillQpelLocationByFeatureValue: Option<PFillQpelLocationByFeatureValueFunc>,
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateBlockFeatureOfFrame: [Option<PCalculateBlockFeatureOfFrame>; 2],
    /// 0 - for 8x8, 1 for 16x16
    pub pfCalculateSingleBlockFeature: [Option<PCalculateSingleBlockFeature>; 2],
    pub pfVerticalFullSearch: Option<PLineFullSearchFunc>,
    pub pfHorizontalFullSearch: Option<PLineFullSearchFunc>,
    pub pfUpdateFMESwitch: Option<PUpdateFMESwitch>,

    pub pfCopy16x16Aligned: Option<PCopyFunc>,
    pub pfCopy16x16NotAligned: Option<PCopyFunc>,
    pub pfCopy8x8Aligned: Option<PCopyFunc>,
    pub pfCopy16x8NotAligned: Option<PCopyFunc>,
    pub pfCopy8x16Aligned: Option<PCopyFunc>,
    pub pfCopy4x4: Option<PCopyFunc>,
    pub pfCopy8x4: Option<PCopyFunc>,
    pub pfCopy4x8: Option<PCopyFunc>,

    pub pfDctT4: Option<PDctFunc>,
    pub pfDctFourT4: Option<PDctFunc>,

    pub pfCalculateSingleCtr4x4: Option<PCalculateSingleCtrFunc>,
    /// DC/AC
    pub pfScan4x4: Option<PScanFunc>,
    pub pfScan4x4Ac: Option<PScanFunc>,

    pub pfQuantization4x4: Option<PQuantizationFunc>,
    pub pfQuantizationFour4x4: Option<PQuantizationFunc>,
    pub pfQuantizationDc4x4: Option<PQuantizationDcFunc>,
    pub pfQuantizationFour4x4Max: Option<PQuantizationMaxFunc>,
    pub pfQuantizationHadamard2x2: Option<PQuantizationHadamardFunc>,
    pub pfQuantizationHadamard2x2Skip: Option<PQuantizationSkipFunc>,

    pub pfTransformHadamard4x4Dc: Option<PTransformHadamard4x4Func>,

    pub pfGetNoneZeroCount: Option<PGetNoneZeroCountFunc>,

    pub pfDequantization4x4: Option<PDeQuantizationFunc>,
    pub pfDequantizationFour4x4: Option<PDeQuantizationFunc>,
    pub pfDequantizationIHadamard4x4: Option<PDeQuantizationHadamardFunc>,
    pub pfIDctFourT4: Option<PIDctFunc>,
    pub pfIDctT4: Option<PIDctFunc>,
    pub pfIDctI16x16Dc: Option<PIDctFunc>,

    /* For Deblocking */
    pub pfDeblocking: DeblockingFunc,
    pub pfSetNZCZero: Option<PSetNoneZeroCountZeroFunc>,

    pub pfRc: SWelsRcFunc,
    pub pfAccumulateSadForRc: Option<PAccumulateSadFunc>,

    /// for size is times to 8
    pub pfSetMemZeroSize8: Option<PSetMemoryZero>,
    /// for size is times of 64, and address is align to 16
    pub pfSetMemZeroSize64Aligned16: Option<PSetMemoryZero>,
    /// for size is times of 64, alignment unknown
    pub pfSetMemZeroSize64: Option<PSetMemoryZero>,

    pub pfCavlcParamCal: Option<PCavlcParamCalFunc>,
    pub pfWelsSpatialWriteMbSyn: Option<PWelsSpatialWriteMbSyn>,
    pub pfGetBsPosition: PGetBsPositionFunc,
    pub pfStashMBStatus: Option<PStashMBStatus>,
    pub pfStashPopMBStatus: Option<PStashPopMBStatus>,

    /// `IWelsParametersetStrategy*` — a thin pointer to the C-style vtable object in
    /// `paraset_strategy.rs`, matching the 8-byte member C++ declares here.
    pub pParametersetStrategy: *mut crate::encoder::paraset_strategy::IWelsParametersetStrategy,
}

pub type TagWelsFuncPointerList = SWelsFuncPtrList;

impl Default for SWelsFuncPtrList {
    fn default() -> Self {
        // All members are function pointers, small POD sub-structs of function
        // pointers, or a raw pointer; the C++ encoder zeroes this table before
        // InitFunctionPointers fills it in.
        unsafe { std::mem::zeroed() }
    }
}
