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
//! `METHOD_BACKGROUND_DETECTION`, `METHOD_SCENE_CHANGE_DETECTION_VIDEO`,
//! Phase 8b session C's `METHOD_DENOISE` and `METHOD_DOWNSAMPLE`, and — P10.2 —
//! the three screen-content methods `METHOD_SCROLL_DETECTION`,
//! `METHOD_SCENE_CHANGE_DETECTION_SCREEN` and `METHOD_COMPLEXITY_ANALYSIS_SCREEN`.
//!
//! **Every method the encoder calls is translated.** A table of untranslated
//! methods stood here until P10.2 and is gone with the last row of it; no caller
//! in `wels_preprocess.rs` skips a plugin any more, and no `RET_NOTSUPPORTED`
//! stands on a live path (S18).
//!
//! The reference's processing library ships two further methods that the encoder
//! never requests, and neither is ported: `METHOD_IMAGE_ROTATE` (classified `dead`
//! by D-scc-12 — `WelsFrameWork.cpp:292` constructs it and nothing invokes it) and
//! `METHOD_COLORSPACE_CONVERT` (which upstream does not implement either).

// **S11.5 (step 5): NOT sealed, and the reason is `forbid`'s scope.** This
// file holds no `unsafe` itself, but `#![forbid]` in a module root applies to
// the module's whole subtree — every `mod` it declares — so sealing here would
// forbid `unsafe` across files that still carry audited allows. A module root
// seals when its subtree does, which is E2's business, not a per-file one.
#![deny(unsafe_code)]

pub mod adaptive_quantization;
pub mod denoise;
pub mod downsample;
pub mod background_detection;
pub mod complexity_analysis;
pub mod scene_change_detection;
pub mod scroll_detection;
pub mod vaacalc;

use adaptive_quantization::CAdaptiveQuantization;
use background_detection::CBackgroundDetection;
use complexity_analysis::CComplexityAnalysis;
use denoise::CDenoiser;
use downsample::CDownsampling;
use complexity_analysis::CComplexityAnalysisScreen;
use scene_change_detection::{CSceneChangeDetection, CSceneChangeDetectionScreen};
use scroll_detection::CScrollDetection;

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
    pub sDownsample: CDownsampling,
    /// The three screen-content plugins (P10.2). `CWelsPreProcessScreen` drives all
    /// three on every P frame; under camera usage they are constructed and never
    /// called, which is what the C++'s plugin table does too.
    pub sScrollDetection: CScrollDetection,
    pub sSceneChangeDetectionScreen: CSceneChangeDetectionScreen,
    pub sComplexityAnalysisScreen: CComplexityAnalysisScreen,
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
            sDownsample: CDownsampling::default(),
            sScrollDetection: CScrollDetection::default(),
            sSceneChangeDetectionScreen: CSceneChangeDetectionScreen::default(),
            sComplexityAnalysisScreen: CComplexityAnalysisScreen::default(),
        }
    }
}
