#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/` — the video-processing (VP) plugins the encoder's
//! pre-processor drives.
//!
//! **The `IWelsVP` vtable is dissolved (Phase 6 session B).** The C++ reaches
//! these plugins through `IWelsVP` — a `void* pCtx` plus `Init`/`Uninit`/`Flush`/
//! `Process`/`Get`/`Set`/`SpecialFeature` function pointers, each dispatching on
//! an `EMethods` id and casting a `void*` parameter back to the one struct that
//! method takes. Phase 4b dissolved the port's other vtables; this one was the
//! last, and it was carrying `*mut c_void` at both ends of every call. What is
//! left is the concrete object, [`SWelsVpContext`] — one field per implemented
//! plugin — owned by `CWelsPreProcess` as a `Box`, and each plugin's `Set`/`Get`/
//! `Process` typed to the parameter struct the cast used to name. The caller
//! names the plugin; there is no method id to dispatch on.
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
//! `METHOD_BACKGROUND_DETECTION`, `METHOD_SCENE_CHANGE_DETECTION_VIDEO` and —
//! Phase 8b session C — `METHOD_DENOISE`.
//!
//! **Not translated**, and each caller in `wels_preprocess.rs` says so at the
//! site and skips the follow-up exactly as it did when the dispatch returned
//! `RET_NOTSUPPORTED` (S18: no stub plugin is invented for them):
//!
//! | method | gated by |
//! |---|---|
//! | `METHOD_SCENE_CHANGE_DETECTION_SCREEN` | `SCREEN_CONTENT_REAL_TIME` |
//! | `METHOD_DOWNSAMPLE` | more than one spatial layer, or a resized layer |
//! | `METHOD_COMPLEXITY_ANALYSIS_SCREEN` | `SCREEN_CONTENT_REAL_TIME` |
//! | `METHOD_SCROLL_DETECTION` | `SCREEN_CONTENT_REAL_TIME` |
//!
//! Every one is off in the gate configuration, and 341/341 holds with all five
//! unsupported.

#![deny(unsafe_code)]

pub mod adaptive_quantization;
pub mod denoise;
pub mod background_detection;
pub mod complexity_analysis;
pub mod scene_change_detection;
pub mod vaacalc;

use adaptive_quantization::CAdaptiveQuantization;
use background_detection::CBackgroundDetection;
use complexity_analysis::CComplexityAnalysis;
use denoise::CDenoiser;
use scene_change_detection::CSceneChangeDetection;

use vaacalc::CVAACalculation;

/// The concrete video-processing object, the Rust counterpart of
/// `CWelsPreProcessPlus`'s plugin table: one field per implemented method.
/// `CWelsPreProcess::m_vp` owns one.
pub struct SWelsVpContext {
    pub sVaaCalc: CVAACalculation,
    pub sComplexityAnalysis: CComplexityAnalysis,
    pub sAdaptiveQuant: CAdaptiveQuantization,
    pub sBackgroundDetection: CBackgroundDetection,
    pub sSceneChangeDetection: CSceneChangeDetection,
    pub sDenoise: CDenoiser,
}

impl Default for SWelsVpContext {
    fn default() -> Self {
        Self {
            sVaaCalc: CVAACalculation::default(),
            sComplexityAnalysis: CComplexityAnalysis::default(),
            sAdaptiveQuant: CAdaptiveQuantization::default(),
            sBackgroundDetection: CBackgroundDetection::default(),
            sSceneChangeDetection: CSceneChangeDetection::default(),
            sDenoise: CDenoiser::default(),
        }
    }
}
