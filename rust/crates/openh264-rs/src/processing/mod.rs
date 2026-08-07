#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/` — the video-processing (VP) plugin the encoder
//! reaches through the `IWelsVP` vtable.
//!
//! `CWelsPreProcess::WelsPreprocessCreate` used to allocate a **zeroed** `IWelsVP`,
//! so every `Init`/`Set`/`Process`/`Get` was `None` and returned 0 (success) without
//! writing anything. The visible consequence was that
//! `pVaa->sVaaCalcInfo.pSad8x8` stayed all-zero for every macroblock, which made
//! `MdInterAnalysisVaaInfo_c` report `uiMbSign == 15` (all four 8x8 blocks flat) for
//! every macroblock, which made `WelsMdInterFinePartitionVaa` return immediately —
//! so no sub-16x16 inter partition was ever evaluated.
//!
//! Implemented: `METHOD_VAA_STATISTICS` (all five SAD kernels),
//! `METHOD_COMPLEXITY_ANALYSIS`, `METHOD_ADAPTIVE_QUANT`,
//! `METHOD_BACKGROUND_DETECTION` and `METHOD_SCENE_CHANGE_DETECTION_VIDEO`.
//! Every other method returns
//! `RET_NOTSUPPORTED` instead of the previous silent success; see
//! [`WelsVpProcess`] for the list and what each one gates.

pub mod adaptive_quantization;
pub mod background_detection;
pub mod complexity_analysis;
pub mod scene_change_detection;
pub mod vaacalc;

use adaptive_quantization::CAdaptiveQuantization;
use background_detection::CBackgroundDetection;
use complexity_analysis::CComplexityAnalysis;
use scene_change_detection::CSceneChangeDetection;

use crate::encoder::wels_preprocess::{EMethods, IWelsVP, SPixMap};
use core::ffi::c_void;
use vaacalc::{CVAACalculation, RET_INVALIDPARAM, RET_NOTSUPPORTED, RET_SUCCESS};

/// The concrete object behind `IWelsVP::pCtx`, the Rust counterpart of
/// `CWelsPreProcessPlus`'s plugin table. One field per implemented method.
pub struct SWelsVpContext {
    pub sVaaCalc: CVAACalculation,
    pub sComplexityAnalysis: CComplexityAnalysis,
    pub sAdaptiveQuant: CAdaptiveQuantization,
    pub sBackgroundDetection: CBackgroundDetection,
    pub sSceneChangeDetection: CSceneChangeDetection,
}

impl Default for SWelsVpContext {
    fn default() -> Self {
        Self {
            sVaaCalc: CVAACalculation::default(),
            sComplexityAnalysis: CComplexityAnalysis::default(),
            sAdaptiveQuant: CAdaptiveQuantization::default(),
            sBackgroundDetection: CBackgroundDetection::default(),
            sSceneChangeDetection: CSceneChangeDetection::default(),
        }
    }
}

/// `IWelsVP::Init`. No plugin implemented here needs per-method initialisation.
///
/// # Safety
/// `pCtx` must be a pointer returned by [`WelsCreateVpInterface`].
pub unsafe extern "C" fn WelsVpInit(_pCtx: *mut c_void, _iType: i32, _pCfg: *mut c_void) -> i32 {
    RET_SUCCESS
}

/// `IWelsVP::Uninit`.
///
/// # Safety
/// As [`WelsVpInit`].
pub unsafe extern "C" fn WelsVpUninit(_pCtx: *mut c_void, _iType: i32) -> i32 {
    RET_SUCCESS
}

/// `IWelsVP::Flush`.
///
/// # Safety
/// As [`WelsVpInit`].
pub unsafe extern "C" fn WelsVpFlush(_pCtx: *mut c_void, _iType: i32) -> i32 {
    RET_SUCCESS
}

/// `IWelsVP::Set`.
///
/// # Safety
/// `pCtx` must be a pointer returned by [`WelsCreateVpInterface`]; `pParam` must
/// match the method's parameter struct.
pub unsafe extern "C" fn WelsVpSet(pCtx: *mut c_void, iType: i32, pParam: *mut c_void) -> i32 {
    if pCtx.is_null() {
        return RET_INVALIDPARAM;
    }
    let ctx = &mut *(pCtx as *mut SWelsVpContext);
    if iType == EMethods::METHOD_VAA_STATISTICS as i32 {
        return ctx.sVaaCalc.Set(iType, pParam);
    }
    if iType == EMethods::METHOD_COMPLEXITY_ANALYSIS as i32 {
        return ctx.sComplexityAnalysis.Set(iType, pParam);
    }
    if iType == EMethods::METHOD_ADAPTIVE_QUANT as i32 {
        return ctx.sAdaptiveQuant.Set(iType, pParam);
    }
    if iType == EMethods::METHOD_BACKGROUND_DETECTION as i32 {
        return ctx.sBackgroundDetection.Set(iType, pParam);
    }
    if iType == EMethods::METHOD_SCENE_CHANGE_DETECTION_VIDEO as i32 {
        return ctx.sSceneChangeDetection.Set(iType, pParam);
    }
    RET_NOTSUPPORTED
}

/// `IWelsVP::Get`.
///
/// # Safety
/// As [`WelsVpSet`].
pub unsafe extern "C" fn WelsVpGet(pCtx: *mut c_void, iType: i32, pParam: *mut c_void) -> i32 {
    if pCtx.is_null() {
        return RET_INVALIDPARAM;
    }
    let ctx = &mut *(pCtx as *mut SWelsVpContext);
    if iType == EMethods::METHOD_COMPLEXITY_ANALYSIS as i32 {
        return ctx.sComplexityAnalysis.Get(iType, pParam);
    }
    if iType == EMethods::METHOD_ADAPTIVE_QUANT as i32 {
        return ctx.sAdaptiveQuant.Get(iType, pParam);
    }
    if iType == EMethods::METHOD_BACKGROUND_DETECTION as i32 {
        return ctx.sBackgroundDetection.Get(iType, pParam);
    }
    if iType == EMethods::METHOD_SCENE_CHANGE_DETECTION_VIDEO as i32 {
        return ctx.sSceneChangeDetection.Get(iType, pParam);
    }
    // `METHOD_VAA_STATISTICS` results are written straight into the caller's
    // `SVAACalcResult` by Process — the C++ plugin has no Get for it either.
    RET_NOTSUPPORTED
}

/// `IWelsVP::Process`.
///
/// Implemented: `METHOD_VAA_STATISTICS`, `METHOD_COMPLEXITY_ANALYSIS`,
/// `METHOD_ADAPTIVE_QUANT`, `METHOD_BACKGROUND_DETECTION`,
/// `METHOD_SCENE_CHANGE_DETECTION_VIDEO`.
///
/// Returns `RET_NOTSUPPORTED` for the rest. Each is off in the gate configuration
/// and its caller in `wels_preprocess.rs` already skips the follow-up `Get` when
/// `Process` does not return `RET_SUCCESS`, so a non-success return leaves exactly
/// the state the previous no-op left — but loudly:
///
/// | method | gated by |
/// |---|---|
/// | `METHOD_DENOISE` | `bEnableDenoise` |
/// | `METHOD_SCENE_CHANGE_DETECTION_SCREEN` | `SCREEN_CONTENT_REAL_TIME` |
/// | `METHOD_DOWNSAMPLE` | more than one spatial layer, or a resized layer |
/// | `METHOD_COMPLEXITY_ANALYSIS_SCREEN` | `SCREEN_CONTENT_REAL_TIME` |
/// | `METHOD_SCROLL_DETECTION` | `SCREEN_CONTENT_REAL_TIME` |
///
/// # Safety
/// `pCtx` must be a pointer returned by [`WelsCreateVpInterface`]; `pSrc` and `pDst`
/// must describe readable planes.
pub unsafe extern "C" fn WelsVpProcess(
    pCtx: *mut c_void,
    iType: i32,
    pSrc: *mut SPixMap,
    pDst: *mut SPixMap,
) -> i32 {
    if pCtx.is_null() {
        return RET_INVALIDPARAM;
    }
    let ctx = &mut *(pCtx as *mut SWelsVpContext);
    if iType == EMethods::METHOD_VAA_STATISTICS as i32 {
        if pSrc.is_null() || pDst.is_null() {
            return RET_INVALIDPARAM;
        }
        // `CVAACalculation::Process` reads the current picture from pSrcPixMap and
        // the reference from pRefPixMap, which the caller passes as pDst.
        return ctx.sVaaCalc.Process(
            iType,
            (*pSrc).pPixel[0] as *mut u8,
            (*pDst).pPixel[0] as *mut u8,
            (*pSrc).sRect.iRectWidth,
            (*pSrc).sRect.iRectHeight,
            (*pSrc).iStride[0],
        );
    }
    if iType == EMethods::METHOD_COMPLEXITY_ANALYSIS as i32 {
        return ctx.sComplexityAnalysis.Process(iType, pSrc, pDst);
    }
    if iType == EMethods::METHOD_ADAPTIVE_QUANT as i32 {
        return ctx.sAdaptiveQuant.Process(iType, pSrc, pDst);
    }
    if iType == EMethods::METHOD_BACKGROUND_DETECTION as i32 {
        return ctx.sBackgroundDetection.Process(iType, pSrc, pDst);
    }
    if iType == EMethods::METHOD_SCENE_CHANGE_DETECTION_VIDEO as i32 {
        return ctx.sSceneChangeDetection.Process(iType, pSrc, pDst);
    }
    RET_NOTSUPPORTED
}

/// `IWelsVP::SpecialFeature`.
///
/// # Safety
/// As [`WelsVpSet`].
pub unsafe extern "C" fn WelsVpSpecialFeature(
    _pCtx: *mut c_void,
    _iType: i32,
    _pIn: *mut c_void,
    _pOut: *mut c_void,
) -> i32 {
    RET_NOTSUPPORTED
}

/// `WelsCreateVpInterface` — `codec/processing/src/common/WelsFrameWork.cpp`.
/// Allocates the plugin context and fills the vtable.
///
/// The returned `IWelsVP` and its `pCtx` are both Rust-heap allocations; free them
/// with [`WelsDestroyVpInterface`] and nothing else.
///
/// # Safety
/// The caller owns the returned pointer.
pub unsafe fn WelsCreateVpInterface() -> *mut IWelsVP {
    let layout = std::alloc::Layout::new::<IWelsVP>();
    let pVp = std::alloc::alloc_zeroed(layout) as *mut IWelsVP;
    if pVp.is_null() {
        return std::ptr::null_mut();
    }
    let pCtx = Box::into_raw(Box::new(SWelsVpContext::default()));
    (*pVp).pCtx = pCtx as *mut c_void;
    (*pVp).Init = Some(WelsVpInit);
    (*pVp).Uninit = Some(WelsVpUninit);
    (*pVp).Flush = Some(WelsVpFlush);
    (*pVp).Process = Some(WelsVpProcess);
    (*pVp).Get = Some(WelsVpGet);
    (*pVp).Set = Some(WelsVpSet);
    (*pVp).SpecialFeature = Some(WelsVpSpecialFeature);
    pVp
}

/// Frees what [`WelsCreateVpInterface`] allocated.
///
/// # Safety
/// `pVp` must have come from [`WelsCreateVpInterface`] and must not be used after.
pub unsafe fn WelsDestroyVpInterface(pVp: *mut IWelsVP) {
    if pVp.is_null() {
        return;
    }
    if !(*pVp).pCtx.is_null() {
        drop(Box::from_raw((*pVp).pCtx as *mut SWelsVpContext));
        (*pVp).pCtx = std::ptr::null_mut();
    }
    let layout = std::alloc::Layout::new::<IWelsVP>();
    std::alloc::dealloc(pVp as *mut u8, layout);
}
