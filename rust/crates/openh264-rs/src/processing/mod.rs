#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! Port of `codec/processing/` — the video-processing (VP) plugins the encoder's
//! pre-processor drives.
//!
//! Implemented: `METHOD_VAA_STATISTICS` (all five SAD kernels),
//! `METHOD_COMPLEXITY_ANALYSIS`, `METHOD_ADAPTIVE_QUANT`,
//! `METHOD_BACKGROUND_DETECTION`, `METHOD_SCENE_CHANGE_DETECTION_VIDEO`,
//! `METHOD_DENOISE` and `METHOD_DOWNSAMPLE`, and the three screen-content
//! methods `METHOD_SCROLL_DETECTION`,
//! `METHOD_SCENE_CHANGE_DETECTION_SCREEN` and `METHOD_COMPLEXITY_ANALYSIS_SCREEN`.
//!
//! The reference's processing library ships two further methods that the encoder
//! never requests, and neither is ported: `METHOD_IMAGE_ROTATE`
//! (`WelsFrameWork.cpp:292` constructs it and nothing invokes it) and
//! `METHOD_COLORSPACE_CONVERT` (which upstream does not implement either).

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
    /// The three screen-content plugins. `CWelsPreProcessScreen` drives all
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
