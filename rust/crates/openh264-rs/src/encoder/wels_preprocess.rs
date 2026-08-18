pub const MAX_DEPENDENCY_LAYER: usize = 4;
/// `EComplexityAnalysisMode` — `codec/processing/interface/IWelsVP.h:215`.
/// The two GOM modes are **negative**; the port had them as `1`/`2` until Phase 5.1,
/// which sent `CComplexityAnalysis::Process` down its `default:` arm.
pub use crate::processing::complexity_analysis::{FRAME_SAD, GOM_SAD, GOM_VAR};
// Copyright (c) 2011-2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

/// # Video Pre-Processing & Video Analysis/Assessment (VAA) Subsystem
///
/// Translated from `codec/encoder/core/inc/wels_preprocess.h` and `codec/encoder/core/src/wels_preprocess.cpp`.
///
/// Handles raw YUV 4:2:0 ingestion, cropping, border padding, bilateral denoising,
/// spatial downsampling pyramids, video analytics assessment (8x8 SAD, 16x16 variance/SSD,
/// background macroblock detection, adaptive quantization delta-QP estimation),
/// scene change detection, scroll motion vector detection, and multi-reference picture ranking.

#[allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};
use std::mem::size_of;
use crate::{
    EUsageType, SEncParamExt, SSourcePicture, SSpatialLayerConfig, VideoFormat,
};
use crate::common::memory_align::CMemoryAlign;
use crate::encoder::encoder_ext::{PADDING_LENGTH, WELS_ALIGN};
use crate::encoder::param_svc::{MB_HEIGHT_LUMA, MB_WIDTH_LUMA};
use crate::encoder::encoder_context::SMVUnitXY;

/// Allocation tag for `CMemoryAlign`; the C++ tags are diagnostic strings only.
macro_rules! tag {
    ($s:literal) => {
        concat!($s, "\0").as_ptr() as *const c_char
    };
}

// ============================================================================
// Constants
// ============================================================================

pub const MAX_REF_PIC_COUNT: usize = 16;
// Single definition in `encoder_context.rs` from `wels_const.h`; this module's copy of
// MAX_SHORT_REF_COUNT was 16 where C++ derives 4, which over-sized `SRefList` here and
// let the `WelsPreprocess` unref loop read one past `pShortRefList`.
pub use crate::encoder::encoder_context::{MAX_GOP_SIZE, MAX_SHORT_REF_COUNT, MAX_TEMPORAL_LEVEL};
pub use crate::encoder::encoder_context::SRefList;
pub use crate::encoder::picture::SPicture;
pub use crate::encoder::encoder_context::SLTRState;
pub use crate::encoder::encoder_context::SLogContext;
pub use crate::encoder::param_svc::SSpatialLayerInternal;
pub use crate::encoder::param_svc::SWelsSvcCodingParam;
pub use crate::encoder::rc::SWelsSvcRc;
pub const INVALID_TEMPORAL_ID: u8 = 0xff;
pub const STATIC_SCENE_MOTION_RATIO: f32 = 0.01;
pub const g_kiPixMapSizeInBits: i32 = (std::mem::size_of::<u8>() * 8) as i32;

/// `rc.h:57` says **8**, not 2. Only `SComplexityAnalysisScreenParam` uses it and
/// `METHOD_COMPLEXITY_ANALYSIS_SCREEN` is still unported, so the wrong value is
/// dead today -- but it is the same shape as GOM_SAD/GOM_VAR in Phase 5.1.
pub use crate::encoder::rc::GOM_H_SCC;
pub const MAX_MBS_PER_FRAME: i32 = 36864;

pub const I_SLICE: i32 = 2;
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;

pub const RC_QUALITY_MODE: i32 = 0;
pub const RC_BITRATE_MODE: i32 = 1;
pub const RC_BUFFERBASED_MODE: i32 = 2;
pub const RC_TIMESTAMP_MODE: i32 = 3;
pub const RC_OFF_MODE: i32 = -1;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_INVALIDINPUT: i32 = 0x10;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;

pub const WELSVP_MAJOR_VERSION: i32 = 1;
pub const WELSVP_MINOR_VERSION: i32 = 1;
pub const WELSVP_VERSION: i32 = (WELSVP_MAJOR_VERSION << 8) + WELSVP_MINOR_VERSION;
pub const WELSVP_INTERFACE_VERSION: i32 = 0x8000 + (WELSVP_VERSION & 0x7fff);

/// `wels_const.h:152-153` — RECIEVE_SUCCESS = 1, RECIEVE_FAILED = **2**. This
/// module's copy of RECIEVE_FAILED said 0; nothing here reads it (only
/// RECIEVE_SUCCESS is tested), so the wrong value was dead. One definition now.
pub use crate::encoder::picture::{RECIEVE_FAILED, RECIEVE_SUCCESS};

/// Look-up table mapping the frame coding index within a GOP to its temporal reference index.
pub const g_kuiRefTemporalIdx: [[u8; MAX_GOP_SIZE]; MAX_TEMPORAL_LEVEL] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 0, 0, 0, 0],
    [0, 0, 0, 2, 0, 1, 1, 2],
];

// ============================================================================
// Enums
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ESceneChangeIdc {
    #[default]
    SIMILAR_SCENE = 0,
    MEDIUM_CHANGED_SCENE = 1,
    LARGE_CHANGED_SCENE = 2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EStaticBlockIdc {
    #[default]
    NO_STATIC = 0,
    COLLOCATED_STATIC = 1,
    SCROLLED_STATIC = 2,
    BLOCK_STATIC_IDC_ALL = 3,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EMethods {
    #[default]
    METHOD_NULL = 0,
    METHOD_COLORSPACE_CONVERT = 1,
    METHOD_DENOISE = 2,
    METHOD_SCENE_CHANGE_DETECTION_VIDEO = 3,
    METHOD_SCENE_CHANGE_DETECTION_SCREEN = 4,
    METHOD_DOWNSAMPLE = 5,
    METHOD_VAA_STATISTICS = 6,
    METHOD_BACKGROUND_DETECTION = 7,
    METHOD_ADAPTIVE_QUANT = 8,
    METHOD_COMPLEXITY_ANALYSIS = 9,
    METHOD_COMPLEXITY_ANALYSIS_SCREEN = 10,
    METHOD_IMAGE_ROTATE = 11,
    METHOD_SCROLL_DETECTION = 12,
    METHOD_MASK = 13,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EComplexityAnalysisMode {
    #[default]
    FRAME_SAD = 0,
    GOM_SAD = -1,
    GOM_VAR = -2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EPixMapBufferProperty {
    #[default]
    BUFFER_HOSTMEM = 0,
    BUFFER_SURFACE = 1,
}

// ============================================================================
// Preprocessing & VAA Data Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Scaled_Picture {
    pub pScaledInputPicture: *mut SPicture,
    pub iScaledWidth: [i32; MAX_DEPENDENCY_LAYER],
    pub iScaledHeight: [i32; MAX_DEPENDENCY_LAYER],
}

impl Default for Scaled_Picture {
    fn default() -> Self {
        Self {
            pScaledInputPicture: std::ptr::null_mut(),
            iScaledWidth: [0; MAX_DEPENDENCY_LAYER],
            iScaledHeight: [0; MAX_DEPENDENCY_LAYER],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefJudgement {
    pub iMinFrameComplexity: i64,
    pub iMinFrameComplexity08: i64,
    pub iMinFrameComplexity11: i64,
    pub iMinFrameNumGap: i32,
    pub iMinFrameQp: i32,
}

impl Default for SRefJudgement {
    fn default() -> Self {
        Self {
            iMinFrameComplexity: i32::MAX as i64,
            iMinFrameComplexity08: i32::MAX as i64,
            iMinFrameComplexity11: i32::MAX as i64,
            iMinFrameNumGap: i32::MAX,
            iMinFrameQp: i32::MAX,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefInfoParam {
    pub pRefPicture: *mut SPicture,
    pub iSrcListIdx: i32,
    pub bSceneLtrFlag: bool,
    pub pBestBlockStaticIdc: *mut u8,
}

impl Default for SRefInfoParam {
    fn default() -> Self {
        Self {
            pRefPicture: std::ptr::null_mut(),
            iSrcListIdx: 0,
            bSceneLtrFlag: false,
            pBestBlockStaticIdc: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRect {
    pub iRectTop: i32,
    pub iRectLeft: i32,
    pub iRectWidth: i32,
    pub iRectHeight: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPixMap {
    pub pPixel: [*mut c_void; 3],
    pub iSizeInBits: i32,
    pub iStride: [i32; 3],
    pub sRect: SRect,
    pub eFormat: VideoFormat,
    pub eProperty: EPixMapBufferProperty,
}

impl Default for SPixMap {
    fn default() -> Self {
        Self {
            pPixel: [std::ptr::null_mut(); 3],
            iSizeInBits: g_kiPixMapSizeInBits,
            iStride: [0; 3],
            sRect: SRect::default(),
            eFormat: VideoFormat::videoFormatI420,
            eProperty: EPixMapBufferProperty::BUFFER_HOSTMEM,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SScrollDetectionParam {
    pub sMaskRect: SRect,
    pub bMaskInfoAvailable: bool,
    pub iScrollMvX: i32,
    pub iScrollMvY: i32,
    pub bScrollDetectFlag: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSceneChangeResult {
    pub eSceneChangeIdc: ESceneChangeIdc,
    pub iMotionBlockNum: i32,
    pub iFrameComplexity: i64,
    pub pStaticBlockIdc: *mut u8,
    pub sScrollResult: SScrollDetectionParam,
}

impl Default for SSceneChangeResult {
    fn default() -> Self {
        Self {
            eSceneChangeIdc: ESceneChangeIdc::SIMILAR_SCENE,
            iMotionBlockNum: 0,
            iFrameComplexity: 0,
            pStaticBlockIdc: std::ptr::null_mut(),
            sScrollResult: SScrollDetectionParam::default(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SVAACalcResult {
    pub pCurY: *mut u8,
    pub pRefY: *mut u8,
    pub pSad8x8: *mut [i32; 4],
    pub pSsd16x16: *mut i32,
    pub pSum16x16: *mut i32,
    pub pSumOfSquare16x16: *mut i32,
    pub pSumOfDiff8x8: *mut [i32; 4],
    pub pMad8x8: *mut [u8; 4],
    pub iFrameSad: i32,
}

impl Default for SVAACalcResult {
    fn default() -> Self {
        Self {
            pCurY: std::ptr::null_mut(),
            pRefY: std::ptr::null_mut(),
            pSad8x8: std::ptr::null_mut(),
            pSsd16x16: std::ptr::null_mut(),
            pSum16x16: std::ptr::null_mut(),
            pSumOfSquare16x16: std::ptr::null_mut(),
            pSumOfDiff8x8: std::ptr::null_mut(),
            pMad8x8: std::ptr::null_mut(),
            iFrameSad: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SVAACalcParam {
    pub iCalcVar: bool,
    pub iCalcBgd: bool,
    pub iCalcSsd: bool,
    pub iReserved: i32,
    pub pCalcResult: *mut SVAACalcResult,
}

impl Default for SVAACalcParam {
    fn default() -> Self {
        Self {
            iCalcVar: false,
            iCalcBgd: false,
            iCalcSsd: false,
            iReserved: 0,
            pCalcResult: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBGDInterface {
    pub pBackgroundMbFlag: *mut i8,
    pub pCalcRes: *mut SVAACalcResult,
}

impl Default for SBGDInterface {
    fn default() -> Self {
        Self {
            pBackgroundMbFlag: std::ptr::null_mut(),
            pCalcRes: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMotionTextureUnit {
    pub uiMotionIndex: u16,
    pub uiTextureIndex: u16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SAdaptiveQuantizationParam {
    pub iAdaptiveQuantMode: i32,
    pub pCalcResult: *mut SVAACalcResult,
    pub pMotionTextureUnit: *mut SMotionTextureUnit,
    pub pMotionTextureIndexToDeltaQp: *mut i8,
    pub iAverMotionTextureIndexToDeltaQp: i32,
}

impl Default for SAdaptiveQuantizationParam {
    fn default() -> Self {
        Self {
            iAdaptiveQuantMode: 0,
            pCalcResult: std::ptr::null_mut(),
            pMotionTextureUnit: std::ptr::null_mut(),
            pMotionTextureIndexToDeltaQp: std::ptr::null_mut(),
            iAverMotionTextureIndexToDeltaQp: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SComplexityAnalysisParam {
    pub iComplexityAnalysisMode: i32,
    pub iCalcBgd: bool,
    pub iMbNumInGom: i32,
    pub iFrameComplexity: i64,
    pub pGomComplexity: *mut i32,
    pub pGomForegroundBlockNum: *mut i32,
    pub pBackgroundMbFlag: *mut i8,
    pub uiRefMbType: *mut u32,
    pub pCalcResult: *mut SVAACalcResult,
}

impl Default for SComplexityAnalysisParam {
    fn default() -> Self {
        Self {
            iComplexityAnalysisMode: 0,
            iCalcBgd: false,
            iMbNumInGom: 0,
            iFrameComplexity: 0,
            pGomComplexity: std::ptr::null_mut(),
            pGomForegroundBlockNum: std::ptr::null_mut(),
            pBackgroundMbFlag: std::ptr::null_mut(),
            uiRefMbType: std::ptr::null_mut(),
            pCalcResult: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SComplexityAnalysisScreenParam {
    pub iMbRowInGom: i32,
    pub pGomComplexity: *mut i32,
    pub iGomNumInFrame: i32,
    pub iFrameComplexity: i64,
    pub iIdrFlag: i32,
    pub sScrollResult: SScrollDetectionParam,
}

impl Default for SComplexityAnalysisScreenParam {
    fn default() -> Self {
        Self {
            iMbRowInGom: GOM_H_SCC,
            pGomComplexity: std::ptr::null_mut(),
            iGomNumInFrame: 0,
            iFrameComplexity: 0,
            iIdrFlag: 0,
            sScrollResult: SScrollDetectionParam::default(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SVAAFrameInfo {
    pub sVaaCalcInfo: SVAACalcResult,
    pub sAdaptiveQuantParam: SAdaptiveQuantizationParam,
    pub sComplexityAnalysisParam: SComplexityAnalysisParam,

    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iPicStride: i32,
    pub iPicStrideUV: i32,

    pub pRefY: *mut u8,
    pub pCurY: *mut u8,
    pub pRefU: *mut u8,
    pub pCurU: *mut u8,
    pub pRefV: *mut u8,
    pub pCurV: *mut u8,

    pub pVaaBackgroundMbFlag: *mut i8,
    pub uiValidLongTermPicIdx: u8,
    pub uiMarkLongTermPicIdx: u8,

    pub eSceneChangeIdc: ESceneChangeIdc,
    pub bSceneChangeFlag: bool,
    pub bIdrPeriodFlag: bool,
}

impl Default for SVAAFrameInfo {
    fn default() -> Self {
        Self {
            sVaaCalcInfo: SVAACalcResult::default(),
            sAdaptiveQuantParam: SAdaptiveQuantizationParam::default(),
            sComplexityAnalysisParam: SComplexityAnalysisParam::default(),
            iPicWidth: 0,
            iPicHeight: 0,
            iPicStride: 0,
            iPicStrideUV: 0,
            pRefY: std::ptr::null_mut(),
            pCurY: std::ptr::null_mut(),
            pRefU: std::ptr::null_mut(),
            pCurU: std::ptr::null_mut(),
            pRefV: std::ptr::null_mut(),
            pCurV: std::ptr::null_mut(),
            pVaaBackgroundMbFlag: std::ptr::null_mut(),
            uiValidLongTermPicIdx: 0,
            uiMarkLongTermPicIdx: 0,
            eSceneChangeIdc: ESceneChangeIdc::SIMILAR_SCENE,
            bSceneChangeFlag: false,
            bIdrPeriodFlag: false,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SVAAFrameInfoExt {
    pub sVaaFrameInfo: SVAAFrameInfo,
    pub sComplexityScreenParam: SComplexityAnalysisScreenParam,
    pub sScrollDetectInfo: SScrollDetectionParam,
    pub sVaaStrBestRefCandidate: [SRefInfoParam; MAX_REF_PIC_COUNT],
    pub sVaaLtrBestRefCandidate: [SRefInfoParam; MAX_REF_PIC_COUNT],
    pub iNumOfAvailableRef: i32,

    pub iVaaBestRefFrameNum: i32,
    pub pVaaBestBlockStaticIdc: *mut u8,
    pub pVaaBlockStaticIdc: [*mut u8; 16],
}

impl Default for SVAAFrameInfoExt {
    fn default() -> Self {
        Self {
            sVaaFrameInfo: SVAAFrameInfo::default(),
            sComplexityScreenParam: SComplexityAnalysisScreenParam::default(),
            sScrollDetectInfo: SScrollDetectionParam::default(),
            sVaaStrBestRefCandidate: [SRefInfoParam::default(); MAX_REF_PIC_COUNT],
            sVaaLtrBestRefCandidate: [SRefInfoParam::default(); MAX_REF_PIC_COUNT],
            iNumOfAvailableRef: 0,
            iVaaBestRefFrameNum: 0,
            pVaaBestBlockStaticIdc: std::ptr::null_mut(),
            pVaaBlockStaticIdc: [std::ptr::null_mut(); 16],
        }
    }
}

// ============================================================================
// Core Structures: SPicture, Parameters, Context, and Plugins
// ============================================================================



#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPosOffset {
    pub iLeft: i32,
    pub iTop: i32,
    pub iWidth: i32,
    pub iHeight: i32,
}






// The canonical encoder context. This module previously declared its own 15-field
// `SWelsEncCtx` plus a `pub type sWelsEncCtx = SWelsEncCtx;` alias, so the lowercase
// name resolved to the fake struct *inside this module only* and every field access
// read the wrong offsets when handed a real context — which is exactly what
// `WelsEncoderEncodeExt` passes to `BuildSpatialPicList` / `AnalyzeSpatialPic` /
// `UpdateSpatialPictures`. `SSpatialIndexMap` was likewise a byte-identical rename of
// `encoder_context::SSpatialPicIndex`, the name C++ uses (`encoder_context.h:198`).
pub use crate::encoder::encoder_context::{sWelsEncCtx, SSpatialPicIndex};
pub use crate::common::wels_common_defs::EWelsSliceType;

#[repr(C)]
pub struct IWelsVP {
    pub pCtx: *mut c_void,
    pub Init: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32, pCfg: *mut c_void) -> i32>,
    pub Uninit: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32) -> i32>,
    pub Flush: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32) -> i32>,
    pub Process: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32, pSrc: *mut SPixMap, pDst: *mut SPixMap) -> i32>,
    pub Get: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32, pParam: *mut c_void) -> i32>,
    pub Set: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32, pParam: *mut c_void) -> i32>,
    pub SpecialFeature: Option<unsafe extern "C" fn(pCtx: *mut c_void, iType: i32, pIn: *mut c_void, pOut: *mut c_void) -> i32>,
}

impl IWelsVP {
    pub unsafe fn Process(&self, iType: i32, pSrc: *mut SPixMap, pDst: *mut SPixMap) -> i32 {
        if let Some(f) = self.Process {
            f(self.pCtx, iType, pSrc, pDst)
        } else {
            0
        }
    }

    pub unsafe fn Get(&self, iType: i32, pParam: *mut c_void) -> i32 {
        if let Some(f) = self.Get {
            f(self.pCtx, iType, pParam)
        } else {
            0
        }
    }

    pub unsafe fn Set(&self, iType: i32, pParam: *mut c_void) -> i32 {
        if let Some(f) = self.Set {
            f(self.pCtx, iType, pParam)
        } else {
            0
        }
    }
}

// ============================================================================
// Helper Memory & Padding Functions
// ============================================================================

/// Zeroes out the line stride padding area `[iWidth .. iStride)` for all lines.
#[inline]
pub unsafe fn ClearEndOfLinePadding(pData: *mut u8, iStride: i32, iWidth: i32, iHeight: i32) {
    if !pData.is_null() && iWidth < iStride {
        let diff = (iStride - iWidth) as usize;
        for i in 0..iHeight {
            let p = pData.offset((i * iStride + iWidth) as isize);
            std::ptr::write_bytes(p, 0, diff);
        }
    }
}

/// Row-by-row planar memory copy for I420 YUV buffers.
#[inline]
pub unsafe fn WelsMoveMemory_c(
    mut pDstY: *mut u8,
    mut pDstU: *mut u8,
    mut pDstV: *mut u8,
    iDstStrideY: i32,
    iDstStrideU: i32,
    iDstStrideV: i32,
    mut pSrcY: *mut u8,
    mut pSrcU: *mut u8,
    mut pSrcV: *mut u8,
    iSrcStrideY: i32,
    iSrcStrideU: i32,
    iSrcStrideV: i32,
    iWidth: i32,
    iHeight: i32,
) {
    let iWidth2 = (iWidth >> 1) as usize;
    let iHeight2 = iHeight >> 1;
    let iWidthY = iWidth as usize;

    for _ in 0..iHeight {
        std::ptr::copy_nonoverlapping(pSrcY, pDstY, iWidthY);
        pDstY = pDstY.offset(iDstStrideY as isize);
        pSrcY = pSrcY.offset(iSrcStrideY as isize);
    }

    for _ in 0..iHeight2 {
        std::ptr::copy_nonoverlapping(pSrcU, pDstU, iWidth2);
        std::ptr::copy_nonoverlapping(pSrcV, pDstV, iWidth2);
        pDstU = pDstU.offset(iDstStrideU as isize);
        pDstV = pDstV.offset(iDstStrideV as isize);
        pSrcU = pSrcU.offset(iSrcStrideU as isize);
        pSrcV = pSrcV.offset(iSrcStrideV as isize);
    }
}

/// Updates the spatial index map pointer for a dependency layer.
#[inline]
pub unsafe fn WelsUpdateSpatialIdxMap(
    pEncCtx: *mut sWelsEncCtx,
    iPos: i32,
    pPic: *mut SPicture,
    iDidx: i32,
) {
    if !pEncCtx.is_null() && iPos >= 0 && (iPos as usize) < MAX_DEPENDENCY_LAYER {
        let idx = iPos as usize;
        (*pEncCtx).sSpatialIndexMap[idx].pSrc = pPic;
        (*pEncCtx).sSpatialIndexMap[idx].iDid = iDidx;
    }
}

/// Evaluates whether the input picture requires aspect-ratio preserving scaling.
pub unsafe fn JudgeNeedOfScaling(
    pParam: *mut SWelsSvcCodingParam,
    pScaledPicture: *mut Scaled_Picture,
) -> bool {
    if pParam.is_null() || pScaledPicture.is_null() {
        return false;
    }

    let kiInputPicWidth = (*pParam).SUsedPicRect.iWidth;
    let kiInputPicHeight = (*pParam).SUsedPicRect.iHeight;
    let layerCount = (*pParam).iSpatialLayerNum;
    if layerCount <= 0 {
        return false;
    }

    let lastLayerIdx = (layerCount - 1) as usize;
    let kiDstPicWidth = (*pParam).sDependencyLayers[lastLayerIdx].iActualWidth;
    let kiDstPicHeight = (*pParam).sDependencyLayers[lastLayerIdx].iActualHeight;
    let mut bNeedDownsampling = true;

    if kiDstPicWidth >= kiInputPicWidth && kiDstPicHeight >= kiInputPicHeight {
        bNeedDownsampling = false;
    }

    let mut iSpatialIdx = layerCount - 1;
    while iSpatialIdx >= 0 {
        let idx = iSpatialIdx as usize;
        let pCurLayer = &(*pParam).sDependencyLayers[idx];
        let iCurDstWidth = pCurLayer.iActualWidth;
        let iCurDstHeight = pCurLayer.iActualHeight;
        let iInputWidthXDstHeight = kiInputPicWidth * iCurDstHeight;
        let iInputHeightXDstWidth = kiInputPicHeight * iCurDstWidth;

        if iInputWidthXDstHeight > iInputHeightXDstWidth {
            (*pScaledPicture).iScaledWidth[idx] = iCurDstWidth.max(4);
            let h = if kiInputPicWidth != 0 {
                iInputHeightXDstWidth / kiInputPicWidth
            } else {
                0
            };
            (*pScaledPicture).iScaledHeight[idx] = h.max(4);
        } else {
            let w = if kiInputPicHeight != 0 {
                iInputWidthXDstHeight / kiInputPicHeight
            } else {
                0
            };
            (*pScaledPicture).iScaledWidth[idx] = w.max(4);
            (*pScaledPicture).iScaledHeight[idx] = iCurDstHeight.max(4);
        }

        iSpatialIdx -= 1;
    }

    bNeedDownsampling
}

/// `picture_handle.cpp:51`. Allocates an `SPicture` with the padded, aligned plane
/// layout the rest of the encoder assumes.
///
/// The previous body here was hand-rolled and wrong in three ways that all mattered:
/// it dropped the `PADDING_LENGTH` border entirely (so `pData[0] == pBuffer` and every
/// intra predictor reading `pRef[-iLineSize]` on the top macroblock row ran off the
/// front of the allocation), it computed `iLineSize[0]` as `WELS_ALIGN(width, 32)`
/// instead of `WELS_ALIGN(WELS_ALIGN(width, 16) + 2 * PADDING_LENGTH, 32)` (160 rather
/// than 224 at the harness resolution, which disagreed with the stride the
/// `pStrideDecBlockOffset` table was built from, so 4x4 reconstructions landed in the
/// wrong rows), and it never allocated `uiRefMbType` / `pRefMbQp` / `sMvList` /
/// `pMbSkipSad`. It also used Rust's global allocator where the caller frees through
/// `CMemoryAlign`.
///
/// # Safety
/// `pMa` must be a valid `CMemoryAlign`. The returned picture must be released with
/// [`FreePicture`] against the same allocator.
pub unsafe fn AllocPicture(
    pMa: *mut CMemoryAlign,
    kiWidth: i32,
    kiHeight: i32,
    bNeedMbInfo: bool,
    iNeedFeatureStorage: i32,
) -> *mut SPicture {
    let pPic = (*pMa).WelsMallocz(size_of::<SPicture>() as u32, tag!("pPic")) as *mut SPicture;
    if pPic.is_null() {
        return std::ptr::null_mut();
    }

    // with width of horizon / height of vertical
    let mut iPicWidth = WELS_ALIGN(kiWidth, MB_WIDTH_LUMA) + (PADDING_LENGTH << 1);
    let iPicHeight = WELS_ALIGN(kiHeight, MB_HEIGHT_LUMA) + (PADDING_LENGTH << 1);
    let mut iPicChromaWidth = iPicWidth >> 1;
    let iPicChromaHeight = iPicHeight >> 1;
    // 32 (or 16 for chroma below) to match original imp. here instead of cache_line_size
    iPicWidth = WELS_ALIGN(iPicWidth, 32);
    iPicChromaWidth = WELS_ALIGN(iPicChromaWidth, 16);
    let iLumaSize = iPicWidth * iPicHeight;
    let iChromaSize = iPicChromaWidth * iPicChromaHeight;

    // **F58, and it is F14/F57's accommodation a third time.** `WelsMallocz`, where
    // the C++ (`picture_handle.cpp:76`) and this port both had `WelsMalloc`: on the
    // **first** frame `AnalyzeSpatialPic` hands `VaaCalculation` a reference picture
    // that nothing has written yet — `wels_preprocess.cpp:289` does the same — and
    // `VAACalcSad` reads its *visible* luma. Reading uninitialised bytes is
    // indeterminate-but-tolerated in C and **Undefined Behaviour in Rust**, so the
    // port cannot transliterate it. Zeroing is the smallest thing that makes the
    // read defined, and it is what both implementations observe in practice: a
    // fresh 18 KB `malloc` is served from zero pages, which is why 341/341 has
    // always agreed. Found by the encoder aliasing probe, Phase 6 session A, at
    // `processing/vaacalc.rs:307`.
    (*pPic).pBuffer =
        (*pMa).WelsMallocz((iLumaSize + (iChromaSize << 1)) as u32, tag!("pPic->pBuffer")) as *mut u8;
    if (*pPic).pBuffer.is_null() {
        let mut p = pPic;
        FreePicture(pMa, &mut p);
        return std::ptr::null_mut();
    }
    (*pPic).iLineSize[0] = iPicWidth;
    (*pPic).iLineSize[1] = iPicChromaWidth;
    (*pPic).iLineSize[2] = iPicChromaWidth;
    (*pPic).pData[0] = (*pPic)
        .pBuffer
        .add(((1 + (*pPic).iLineSize[0]) * PADDING_LENGTH) as usize);
    (*pPic).pData[1] = (*pPic)
        .pBuffer
        .add(iLumaSize as usize + ((((1 + (*pPic).iLineSize[1]) * PADDING_LENGTH) >> 1) as usize));
    (*pPic).pData[2] = (*pPic).pBuffer.add(
        (iLumaSize + iChromaSize) as usize
            + ((((1 + (*pPic).iLineSize[2]) * PADDING_LENGTH) >> 1) as usize),
    );

    (*pPic).iWidthInPixel = kiWidth;
    (*pPic).iHeightInPixel = kiHeight;
    (*pPic).iFrameNum = -1;

    (*pPic).bIsLongRef = false;
    (*pPic).iLongTermPicNum = -1;
    (*pPic).uiRecieveConfirmed = 0;
    (*pPic).iMarkFrameNum = -1;

    if bNeedMbInfo {
        let kuiCountMbNum = (((15 + kiWidth) >> 4) * ((15 + kiHeight) >> 4)) as u32;

        (*pPic).uiRefMbType =
            (*pMa).WelsMallocz(kuiCountMbNum * 4, tag!("pPic->uiRefMbType")) as *mut u32;
        (*pPic).pRefMbQp = (*pMa).WelsMallocz(kuiCountMbNum, tag!("pPic->pRefMbQp")) as *mut u8;
        (*pPic).sMvList = (*pMa).WelsMallocz(
            kuiCountMbNum * size_of::<SMVUnitXY>() as u32,
            tag!("pPic->sMvList"),
        ) as *mut SMVUnitXY;
        (*pPic).pMbSkipSad =
            (*pMa).WelsMallocz(kuiCountMbNum * 4, tag!("pPic->pMbSkipSad")) as *mut i32;

        if (*pPic).uiRefMbType.is_null()
            || (*pPic).pRefMbQp.is_null()
            || (*pPic).sMvList.is_null()
            || (*pPic).pMbSkipSad.is_null()
        {
            let mut p = pPic;
            FreePicture(pMa, &mut p);
            return std::ptr::null_mut();
        }
    }

    // `RequestScreenBlockFeatureStorage` is part of the screen-content path, which is
    // outside the gate configuration and unported; refuse rather than hand back a
    // picture whose storage the caller believes exists.
    if iNeedFeatureStorage != 0 {
        let mut p = pPic;
        FreePicture(pMa, &mut p);
        return std::ptr::null_mut();
    }
    (*pPic).pScreenBlockFeatureStorage = std::ptr::null_mut();

    pPic
}

/// `picture_handle.cpp:129`. Releases a picture and every block `AllocPicture` took
/// from `pMa`.
///
/// # Safety
/// `pMa` must be the allocator the picture was built with.
pub unsafe fn FreePicture(pMa: *mut CMemoryAlign, ppPic: *mut *mut SPicture) {
    if ppPic.is_null() || (*ppPic).is_null() {
        return;
    }
    let pPic = *ppPic;

    if !(*pPic).pBuffer.is_null() {
        (*pMa).WelsFree((*pPic).pBuffer as *mut c_void, tag!("pPic->pBuffer"));
        (*pPic).pBuffer = std::ptr::null_mut();
    }
    (*pPic).pData = [std::ptr::null_mut(); 3];
    (*pPic).iLineSize = [0; 3];

    (*pPic).iWidthInPixel = 0;
    (*pPic).iHeightInPixel = 0;
    (*pPic).iFrameNum = -1;

    (*pPic).bIsLongRef = false;
    (*pPic).uiRecieveConfirmed = 0;
    (*pPic).iLongTermPicNum = -1;
    (*pPic).iMarkFrameNum = -1;

    if !(*pPic).uiRefMbType.is_null() {
        (*pMa).WelsFree((*pPic).uiRefMbType as *mut c_void, tag!("pPic->uiRefMbType"));
        (*pPic).uiRefMbType = std::ptr::null_mut();
    }
    if !(*pPic).pRefMbQp.is_null() {
        (*pMa).WelsFree((*pPic).pRefMbQp as *mut c_void, tag!("pPic->pRefMbQp"));
        (*pPic).pRefMbQp = std::ptr::null_mut();
    }
    if !(*pPic).sMvList.is_null() {
        (*pMa).WelsFree((*pPic).sMvList as *mut c_void, tag!("pPic->sMvList"));
        (*pPic).sMvList = std::ptr::null_mut();
    }
    if !(*pPic).pMbSkipSad.is_null() {
        (*pMa).WelsFree((*pPic).pMbSkipSad as *mut c_void, tag!("pPic->pMbSkipSad"));
        (*pPic).pMbSkipSad = std::ptr::null_mut();
    }

    (*pMa).WelsFree(pPic as *mut c_void, tag!("pPic"));
    *ppPic = std::ptr::null_mut();
}

/// Initializes scaled intermediate picture buffers if aspect-ratio scaling is required.
pub unsafe fn WelsInitScaledPic(
    pParam: *mut SWelsSvcCodingParam,
    pScaledPicture: *mut Scaled_Picture,
    pMemoryAlign: *mut CMemoryAlign,
) -> i32 {
    let bInputPicNeedScaling = JudgeNeedOfScaling(pParam, pScaledPicture);
    if bInputPicNeedScaling {
        (*pScaledPicture).pScaledInputPicture = AllocPicture(
            pMemoryAlign,
            (*pParam).SUsedPicRect.iWidth,
            (*pParam).SUsedPicRect.iHeight,
            false,
            0,
        );
        if (*pScaledPicture).pScaledInputPicture.is_null() {
            return -1;
        }

        let pPic = (*pScaledPicture).pScaledInputPicture;
        ClearEndOfLinePadding(
            (*pPic).pData[0],
            (*pPic).iLineSize[0],
            (*pPic).iWidthInPixel,
            (*pPic).iHeightInPixel,
        );
        ClearEndOfLinePadding(
            (*pPic).pData[1],
            (*pPic).iLineSize[1],
            (*pPic).iWidthInPixel >> 1,
            (*pPic).iHeightInPixel >> 1,
        );
        ClearEndOfLinePadding(
            (*pPic).pData[2],
            (*pPic).iLineSize[2],
            (*pPic).iWidthInPixel >> 1,
            (*pPic).iHeightInPixel >> 1,
        );
    }
    0
}

/// Releases the scaled picture memory.
pub unsafe fn FreeScaledPic(
    pScaledPicture: *mut Scaled_Picture,
    pMemoryAlign: *mut CMemoryAlign,
) {
    if !pScaledPicture.is_null() && !(*pScaledPicture).pScaledInputPicture.is_null() {
        FreePicture(pMemoryAlign, &mut (*pScaledPicture).pScaledInputPicture);
        (*pScaledPicture).pScaledInputPicture = std::ptr::null_mut();
    }
}

// ============================================================================
// Core Preprocessing Engine: CWelsPreProcess
// ============================================================================

#[repr(C)]
pub struct CWelsPreProcess {
    pub m_pInterfaceVp: *mut IWelsVP,
    pub m_pEncCtx: *mut sWelsEncCtx,
    pub m_uiSpatialLayersInTemporal: [u8; MAX_DEPENDENCY_LAYER],
    pub m_sScaledPicture: Scaled_Picture,
    pub m_pLastSpatialPicture: [[*mut SPicture; 2]; MAX_DEPENDENCY_LAYER],
    pub m_bInitDone: bool,
    pub m_uiSpatialPicNum: [u8; MAX_DEPENDENCY_LAYER],
    pub m_pSpatialPic: [[*mut SPicture; MAX_REF_PIC_COUNT + 1]; MAX_DEPENDENCY_LAYER],
    pub m_iAvaliableRefInSpatialPicList: i32,
    pub m_eUsageType: EUsageType,
}

impl Default for CWelsPreProcess {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl CWelsPreProcess {
    /// Factory constructor instantiating the preprocessing subsystem.
    pub unsafe fn CreatePreProcess(pEncCtx: *mut sWelsEncCtx) -> *mut CWelsPreProcess {
        if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() {
            return std::ptr::null_mut();
        }

        let layout = std::alloc::Layout::new::<CWelsPreProcess>();
        let p = std::alloc::alloc_zeroed(layout) as *mut CWelsPreProcess;
        if p.is_null() {
            return std::ptr::null_mut();
        }

        (*p).m_pInterfaceVp = std::ptr::null_mut();
        (*p).m_bInitDone = false;
        (*p).m_pEncCtx = pEncCtx;
        (*p).m_eUsageType = (*(*pEncCtx).pSvcParam).iUsageType;

        p
    }

    /// Destructor releasing allocated picture buffers and plugin interfaces.
    pub unsafe fn Destroy(pPreProcess: *mut CWelsPreProcess) {
        if !pPreProcess.is_null() {
            let pMa = if !(*pPreProcess).m_pEncCtx.is_null() {
                (*(*pPreProcess).m_pEncCtx).pMemAlign
            } else {
                std::ptr::null_mut()
            };
            FreeScaledPic(&mut (*pPreProcess).m_sScaledPicture, pMa);
            (*pPreProcess).WelsPreprocessDestroy();
            let layout = std::alloc::Layout::new::<CWelsPreProcess>();
            std::alloc::dealloc(pPreProcess as *mut u8, layout);
        }
    }

    /// `CWelsPreProcess::WelsPreprocessCreate` — `wels_preprocess.cpp:198`.
    ///
    /// This used to `alloc_zeroed` the `IWelsVP` and stop there, leaving every
    /// method `None`. `IWelsVP::Process`/`Set`/`Get` then returned 0 (success)
    /// without writing anything, so the whole video-analysis stage silently
    /// produced zeros — see `crate::processing`.
    pub unsafe fn WelsPreprocessCreate(&mut self) -> i32 {
        if self.m_pInterfaceVp.is_null() {
            let pVp = crate::processing::WelsCreateVpInterface();
            if pVp.is_null() {
                self.WelsPreprocessDestroy();
                return 1;
            }
            self.m_pInterfaceVp = pVp;
        }
        0
    }

    pub unsafe fn WelsPreprocessDestroy(&mut self) -> i32 {
        if !self.m_pInterfaceVp.is_null() {
            crate::processing::WelsDestroyVpInterface(self.m_pInterfaceVp);
            self.m_pInterfaceVp = std::ptr::null_mut();
        }
        0
    }

    pub unsafe fn WelsPreprocessReset(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        iWidth: i32,
        iHeight: i32,
    ) -> i32 {
        if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
            return -1;
        }

        let pSvcParam = (*pCtx).pSvcParam;
        (*pSvcParam).SUsedPicRect.iLeft = 0;
        (*pSvcParam).SUsedPicRect.iTop = 0;
        (*pSvcParam).SUsedPicRect.iWidth = iWidth;
        (*pSvcParam).SUsedPicRect.iHeight = iHeight;

        if iWidth < 16 || iHeight < 16 {
            return -1;
        }

        FreeScaledPic(&mut self.m_sScaledPicture, (*pCtx).pMemAlign);
        self.InitLastSpatialPictures(pCtx);
        WelsInitScaledPic((*pCtx).pSvcParam, &mut self.m_sScaledPicture, (*pCtx).pMemAlign)
    }

    pub unsafe fn AllocSpatialPictures(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        pParam: *mut SWelsSvcCodingParam,
    ) -> i32 {
        let pMa = (*pCtx).pMemAlign;
        let kiDlayerCount = (*pParam).iSpatialLayerNum;
        let mut iDlayerIndex = 0;

        while iDlayerIndex < kiDlayerCount {
            let idx = iDlayerIndex as usize;
            let kiPicWidth = (*pParam).sSpatialLayers[idx].iVideoWidth;
            let kiPicHeight = (*pParam).sSpatialLayers[idx].iVideoHeight;
            let highestTid = (*pParam).sDependencyLayers[idx].iHighestTemporalId as i32;
            let kuiLayerInTemporal = (2 + highestTid.max(1)) as u8;
            // wels_preprocess.cpp:180 — the sum is computed in int and narrowed to
            // uint8_t, so kuiRefNumInTemporal really is a uint8_t.
            let kuiRefNumInTemporal: u8 =
                (kuiLayerInTemporal as i32 + (*pParam).iLTRRefNum) as u8;

            self.m_uiSpatialPicNum[idx] = kuiRefNumInTemporal;
            let mut i: u8 = 0;
            while i < kuiRefNumInTemporal {
                let pPic = AllocPicture(pMa, kiPicWidth, kiPicHeight, false, 0);
                if pPic.is_null() {
                    return 1;
                }
                self.m_pSpatialPic[idx][i as usize] = pPic;
                i += 1;
            }

            if (*pParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                self.m_uiSpatialLayersInTemporal[idx] = 1;
            } else {
                self.m_uiSpatialLayersInTemporal[idx] = kuiLayerInTemporal;
            }

            iDlayerIndex += 1;
        }

        0
    }

    pub unsafe fn FreeSpatialPictures(&mut self, pCtx: *mut sWelsEncCtx) {
        if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
            return;
        }
        let pMa = (*pCtx).pMemAlign;
        let mut j = 0;
        while j < (*(*pCtx).pSvcParam).iSpatialLayerNum {
            let jIdx = j as usize;
            let mut i: u8 = 0;
            let uiRefNumInTemporal = self.m_uiSpatialPicNum[jIdx];

            while i < uiRefNumInTemporal {
                let iIdx = i as usize;
                if !self.m_pSpatialPic[jIdx][iIdx].is_null() {
                    FreePicture(pMa, &mut self.m_pSpatialPic[jIdx][iIdx]);
                }
                i += 1;
            }
            self.m_uiSpatialLayersInTemporal[jIdx] = 0;
            j += 1;
        }
    }

    pub unsafe fn BuildSpatialPicList(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        kpSrcPic: *const SSourcePicture,
        pSpatialNum: *mut i32,
    ) -> i32 {
        let pSvcParam = (*pCtx).pSvcParam;
        let iWidth = ((*kpSrcPic).iPicWidth >> 1) << 1;
        let iHeight = ((*kpSrcPic).iPicHeight >> 1) << 1;
        *pSpatialNum = 0;

        if !self.m_bInitDone {
            if self.WelsPreprocessCreate() != 0 {
                return ENC_RETURN_MEMALLOCERR;
            }
            if self.WelsPreprocessReset(pCtx, iWidth, iHeight) != 0 {
                return ENC_RETURN_MEMALLOCERR;
            }
            self.m_iAvaliableRefInSpatialPicList = (*pSvcParam).iNumRefFrame;
            self.m_bInitDone = true;
        } else if iWidth != (*pSvcParam).SUsedPicRect.iWidth || iHeight != (*pSvcParam).SUsedPicRect.iHeight {
            if self.WelsPreprocessReset(pCtx, iWidth, iHeight) != 0 {
                return ENC_RETURN_MEMALLOCERR;
            }
        }

        if self.m_pInterfaceVp.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }

        if !(*pCtx).pVaa.is_null() {
            (*(*pCtx).pVaa).bSceneChangeFlag = false;
            (*(*pCtx).pVaa).bIdrPeriodFlag = false;
        }

        // The `pScaledPic` argument stood here — `addr_of_mut!(self.m_sScaledPicture)`
        // handed to a method that takes `&mut self`. Miri rejects the callee's first
        // read through it: a `&mut` argument is strongly protected for the call, so
        // reaching the same object through a sibling raw pointer would remove a
        // protected `Unique`. It is `phase6.md` §1's "cache, not carrier" — the
        // parameter was a copy of something the holder already reaches — so it dies
        // rather than converts. Deriving one pointer at the callee's top would not
        // do either: the `self.` calls between the uses reborrow `self` and pop it.
        // Found by the encoder aliasing probe, Phase 6 session A.
        let iRet = self.SingleLayerPreprocess(pCtx, kpSrcPic, pSpatialNum);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        ENC_RETURN_SUCCESS
    }

    pub unsafe fn SingleLayerPreprocess(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        kpSrc: *const SSourcePicture,
        pSpatialNum: *mut i32,
    ) -> i32 {
        let pSvcParam = (*pCtx).pSvcParam;
        let mut iDependencyId = (*pSvcParam).iSpatialLayerNum - 1;

        let depIdx = iDependencyId as usize;
        // S29: this binding is read again at the bottom of the function, and the
        // shared reborrows of the same array in between (the `iClosestDid` scan)
        // pop a `Unique` where they would leave a `SharedReadWrite` alone.
        let pDlayerParamInternal = std::ptr::addr_of_mut!((*pSvcParam).sDependencyLayers[depIdx]);
        let pDlayerParam = &(*pSvcParam).sSpatialLayers[depIdx];
        let iTargetWidth = pDlayerParam.iVideoWidth;
        let iTargetHeight = pDlayerParam.iVideoHeight;
        let iSrcWidth = (*pSvcParam).SUsedPicRect.iWidth;
        let iSrcHeight = (*pSvcParam).SUsedPicRect.iHeight;

        if (*pSvcParam).uiIntraPeriod != 0 && !(*pCtx).pVaa.is_null() {
            (*(*pCtx).pVaa).bIdrPeriodFlag = (1 + (*pDlayerParamInternal).iFrameIndex) >= (*pSvcParam).uiIntraPeriod as i32;
        }

        *pSpatialNum = 0;
        let pSrcPic = if !self.m_sScaledPicture.pScaledInputPicture.is_null() {
            self.m_sScaledPicture.pScaledInputPicture
        } else {
            self.GetCurrentOrigFrame(iDependencyId)
        };

        let iRet = self.WelsMoveMemoryWrapper(pSvcParam, pSrcPic, kpSrc, iSrcWidth, iSrcHeight);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        if (*pSvcParam).bEnableDenoise {
            self.BilateralDenoising(pSrcPic, iSrcWidth, iSrcHeight);
        }

        let mut iShrinkWidth = iSrcWidth;
        let mut iShrinkHeight = iSrcHeight;
        let mut pDstPic = pSrcPic;
        if !self.m_sScaledPicture.pScaledInputPicture.is_null() {
            pDstPic = self.GetCurrentOrigFrame(iDependencyId);
            iShrinkWidth = self.m_sScaledPicture.iScaledWidth[depIdx];
            iShrinkHeight = self.m_sScaledPicture.iScaledHeight[depIdx];
        }

        self.DownsamplePadding(
            pSrcPic,
            pDstPic,
            iSrcWidth,
            iSrcHeight,
            iShrinkWidth,
            iShrinkHeight,
            iTargetWidth,
            iTargetHeight,
            false,
        );

        if (*pSvcParam).bEnableSceneChangeDetect && !(*pCtx).pVaa.is_null() && !(*(*pCtx).pVaa).bIdrPeriodFlag {
            if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                let idc = if (*pDlayerParamInternal).bEncCurFrmAsIdrFlag {
                    ESceneChangeIdc::LARGE_CHANGED_SCENE
                } else {
                    self.DetectSceneChange(pDstPic, std::ptr::null_mut())
                };
                (*(*pCtx).pVaa).eSceneChangeIdc = idc;
                (*(*pCtx).pVaa).bSceneChangeFlag = idc == ESceneChangeIdc::LARGE_CHANGED_SCENE;
            } else if !(*pDlayerParamInternal).bEncCurFrmAsIdrFlag
                && ((*pDlayerParamInternal).iCodingIndex & ((*pSvcParam).uiGopSize as i32 - 1)) == 0
            {
                let pRefPic = if (*(*pCtx).pLtr.add(depIdx)).bReceivedT0LostFlag {
                    let pos = self.m_uiSpatialLayersInTemporal[depIdx] as usize
                        + (*(*pCtx).pVaa).uiValidLongTermPicIdx as usize;
                    self.m_pSpatialPic[depIdx][pos]
                } else {
                    self.m_pLastSpatialPicture[depIdx][0]
                };
                let idc = self.DetectSceneChange(pDstPic, pRefPic);
                (*(*pCtx).pVaa).bSceneChangeFlag = self.GetSceneChangeFlag(idc);
            }
        }

        let mut iSpatialNum = 0;
        for i in 0..(*pSvcParam).iSpatialLayerNum {
            let pInternal = &(*pSvcParam).sDependencyLayers[i as usize];
            let gopMask = (*pSvcParam).uiGopSize as i32 - 1;
            let tid = pInternal.uiCodingIdx2TemporalId[(pInternal.iCodingIndex & gopMask) as usize];
            if tid != INVALID_TEMPORAL_ID {
                iSpatialNum += 1;
            }
        }

        let gopMask = (*pSvcParam).uiGopSize as i32 - 1;
        let tid = (*pDlayerParamInternal).uiCodingIdx2TemporalId[((*pDlayerParamInternal).iCodingIndex & gopMask) as usize];
        let mut iActualSpatialNum = iSpatialNum - 1;
        if tid != INVALID_TEMPORAL_ID {
            WelsUpdateSpatialIdxMap(pCtx, iActualSpatialNum, pDstPic, iDependencyId);
            iActualSpatialNum -= 1;
        }

        self.m_pLastSpatialPicture[depIdx][1] = self.GetCurrentOrigFrame(iDependencyId);
        let mut iClosestDid = iDependencyId;
        iDependencyId -= 1;

        if (*pSvcParam).iSpatialLayerNum > 1 {
            while iDependencyId >= 0 {
                let curDepIdx = iDependencyId as usize;
                let pInt = &(*pSvcParam).sDependencyLayers[curDepIdx];
                let pLay = &(*pSvcParam).sSpatialLayers[curDepIdx];
                let pSrcPic = self.m_pLastSpatialPicture[iClosestDid as usize][1];
                let iTargetW = pLay.iVideoWidth;
                let iTargetH = pLay.iVideoHeight;
                let tId = pInt.uiCodingIdx2TemporalId[(pInt.iCodingIndex & gopMask) as usize];

                let iSrcW = self.m_sScaledPicture.iScaledWidth[iClosestDid as usize];
                let iSrcH = self.m_sScaledPicture.iScaledHeight[iClosestDid as usize];
                let pDst = self.GetCurrentOrigFrame(iDependencyId);
                let iShrinkW = self.m_sScaledPicture.iScaledWidth[curDepIdx];
                let iShrinkH = self.m_sScaledPicture.iScaledHeight[curDepIdx];

                self.DownsamplePadding(
                    pSrcPic,
                    pDst,
                    iSrcW,
                    iSrcH,
                    iShrinkW,
                    iShrinkH,
                    iTargetW,
                    iTargetH,
                    true,
                );

                if tId != INVALID_TEMPORAL_ID {
                    WelsUpdateSpatialIdxMap(pCtx, iActualSpatialNum, pDst, iDependencyId);
                    iActualSpatialNum -= 1;
                }

                self.m_pLastSpatialPicture[curDepIdx][1] = pDst;
                iClosestDid = iDependencyId;
                iDependencyId -= 1;
            }
        }

        *pSpatialNum = iSpatialNum;
        ENC_RETURN_SUCCESS
    }

    pub unsafe fn AnalyzeSpatialPic(&mut self, pCtx: *mut sWelsEncCtx, kiDidx: i32) -> i32 {
        let pSvcParam = (*pCtx).pSvcParam;
        let bNeededMbAq = (*pSvcParam).bEnableAdaptiveQuant && ((*pCtx).eSliceType == EWelsSliceType::P_SLICE);
        let bCalculateBGD = ((*pCtx).eSliceType == EWelsSliceType::P_SLICE) && (*pSvcParam).bEnableBackgroundDetection;
        let dIdx = kiDidx as usize;
        let pParamInternal = &(*pSvcParam).sDependencyLayers[dIdx];
        let iCurTemporalIdx = self.m_uiSpatialLayersInTemporal[dIdx] as i32 - 1;

        let gopMask = (*pSvcParam).uiGopSize as i32 - 1;
        let stageIdx = (*pParamInternal).iDecompositionStages.max(0).min(MAX_TEMPORAL_LEVEL as i32 - 1) as usize;
        let gopIdx = (pParamInternal.iCodingIndex & gopMask) as usize;
        let mut iRefTemporalIdx = g_kuiRefTemporalIdx[stageIdx][gopIdx] as i32;

        if (*pCtx).uiTemporalId == 0
            && (*(*pCtx).pLtr.add((*pCtx).uiDependencyId as usize)).bReceivedT0LostFlag
        {
            iRefTemporalIdx = self.m_uiSpatialLayersInTemporal[dIdx] as i32
                + (*(*pCtx).pVaa).uiValidLongTermPicIdx as i32;
        }

        let pCurPic = self.m_pSpatialPic[dIdx][iCurTemporalIdx as usize];
        let bCalculateVar = ((*pSvcParam).iRCMode as i32 >= RC_BITRATE_MODE) && ((*pCtx).eSliceType == EWelsSliceType::I_SLICE);

        if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            let pRefPic = self.GetBestRefPicScreen(
                (*pSvcParam).iUsageType,
                (*pCtx).bCurFrameMarkedAsSceneLtr,
                (*pCtx).eSliceType,
                kiDidx,
                iRefTemporalIdx,
            );

            self.VaaCalculation((*pCtx).pVaa, pCurPic, pRefPic, false, bCalculateVar, bCalculateBGD);

            if (*pSvcParam).bEnableBackgroundDetection {
                let bFlag = bCalculateBGD && !pRefPic.is_null() && ((*pRefPic).iPictureType != I_SLICE);
                self.BackgroundDetection((*pCtx).pVaa, pCurPic, pRefPic, bFlag);
            }
            if bNeededMbAq {
                self.AdaptiveQuantCalculation((*pCtx).pVaa, pCurPic, pRefPic);
            }
        } else {
            let pRefPic = self.GetBestRefPic(kiDidx, iRefTemporalIdx);
            let pLastPic = self.m_pLastSpatialPicture[dIdx][0];
            let bCalculateSQDiff = !pLastPic.is_null()
                && !pRefPic.is_null()
                && ((*pLastPic).pData[0] == (*pRefPic).pData[0])
                && bNeededMbAq;

            self.VaaCalculation((*pCtx).pVaa, pCurPic, pRefPic, bCalculateSQDiff, bCalculateVar, bCalculateBGD);

            if (*pSvcParam).bEnableBackgroundDetection {
                let bFlag = bCalculateBGD && !pRefPic.is_null() && ((*pRefPic).iPictureType != I_SLICE);
                self.BackgroundDetection((*pCtx).pVaa, pCurPic, pRefPic, bFlag);
            }

            if bNeededMbAq {
                self.AdaptiveQuantCalculation(
                    (*pCtx).pVaa,
                    self.m_pLastSpatialPicture[dIdx][1],
                    self.m_pLastSpatialPicture[dIdx][0],
                );
            }
            VP_DUMP_SQD.store(bCalculateSQDiff, std::sync::atomic::Ordering::Relaxed);
        }

        if crate::encoder::dump_enabled(&VP_DUMP, "OH264_VPDUMP")
            && (*pCtx).eSliceType == EWelsSliceType::P_SLICE
            && !(*(*pCtx).pVaa).pVaaBackgroundMbFlag.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSad8x8.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSumOfDiff8x8.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pMad8x8.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSsd16x16.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSum16x16.is_null()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSumOfSquare16x16.is_null()
        {
            let v = &*(*pCtx).pVaa;
            let iMbNum = (((*pCurPic).iWidthInPixel + 15) >> 4)
                * (((*pCurPic).iHeightInPixel + 15) >> 4);
            let (mut aq, mut bg, mut sad, mut ssd, mut sd, mut mad, mut sum, mut sqsum) =
                (0i64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
            for i in 0..iMbNum as isize {
                let w = i as i64 + 1;
                bg += w * *v.pVaaBackgroundMbFlag.offset(i) as i64;
                let s8 = &*v.sVaaCalcInfo.pSad8x8.offset(i);
                let d8 = &*v.sVaaCalcInfo.pSumOfDiff8x8.offset(i);
                let m8 = &*v.sVaaCalcInfo.pMad8x8.offset(i);
                for k in 0..4 {
                    sad += w * s8[k] as i64;
                    sd += w * d8[k] as i64;
                    mad += w * m8[k] as i64;
                }
                ssd += w * *v.sVaaCalcInfo.pSsd16x16.offset(i) as i64;
                sum += w * *v.sVaaCalcInfo.pSum16x16.offset(i) as i64;
                sqsum += w * *v.sVaaCalcInfo.pSumOfSquare16x16.offset(i) as i64;
            }
            eprintln!(
                "VP st={} sqd={} var={} bgd={} frmsad={} aqavg={} aq={} bg={} sad={} sd={} mad={} ssd={} sum={} sqsum={} scd={}",
                (*pCtx).eSliceType as i32,
                VP_DUMP_SQD.load(std::sync::atomic::Ordering::Relaxed) as i32,
                bCalculateVar as i32,
                bCalculateBGD as i32,
                v.sVaaCalcInfo.iFrameSad,
                v.sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp,
                aq, bg, sad, sd, mad, ssd, sum, sqsum,
                v.bSceneChangeFlag as i32
            );
        }

        0
    }

    pub unsafe fn GetCurPicPosition(&self, kiDidx: i32) -> i32 {
        self.m_uiSpatialLayersInTemporal[kiDidx as usize] as i32 - 1
    }

    pub unsafe fn GetCurrentOrigFrame(&mut self, iDIdx: i32) -> *mut SPicture {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.m_pSpatialPic[iDIdx as usize][0]
        } else {
            let pos = self.GetCurPicPosition(iDIdx) as usize;
            self.m_pSpatialPic[iDIdx as usize][pos]
        }
    }

    pub unsafe fn GetBestRefPic(&self, kiDidx: i32, iRefTemporalIdx: i32) -> *mut SPicture {
        self.m_pSpatialPic[kiDidx as usize][iRefTemporalIdx as usize]
    }

    pub unsafe fn GetBestRefPicScreen(
        &self,
        _iUsageType: EUsageType,
        bSceneLtr: bool,
        _eSliceType: EWelsSliceType,
        _kiDidx: i32,
        _iRefTemporalIdx: i32,
    ) -> *mut SPicture {
        let pVaaExt = (*self.m_pEncCtx).pVaa as *mut SVAAFrameInfoExt;
        let pBest = if bSceneLtr {
            &(*pVaaExt).sVaaLtrBestRefCandidate[0]
        } else {
            &(*pVaaExt).sVaaStrBestRefCandidate[0]
        };
        self.m_pSpatialPic[0][pBest.iSrcListIdx as usize]
    }

    pub unsafe fn UpdateSpatialPictures(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        pParam: *mut SWelsSvcCodingParam,
        iCurTid: i8,
        kiDidx: i32,
    ) -> i32 {
        if (*pCtx).pSvcParam.is_null() || (*(*pCtx).pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            return 0;
        }

        let dIdx = kiDidx as usize;
        Self::WelsExchangeSpatialPictures(
            &mut self.m_pLastSpatialPicture[dIdx][1],
            &mut self.m_pLastSpatialPicture[dIdx][0],
        );

        let kiCurPos = self.GetCurPicPosition(kiDidx);
        if (iCurTid as i32) < kiCurPos || (*pParam).iDecompStages == 0 {
            if (iCurTid as usize) >= MAX_TEMPORAL_LEVEL || (kiCurPos as usize) > MAX_TEMPORAL_LEVEL {
                self.InitLastSpatialPictures(pCtx);
                return 1;
            }
            if (*pCtx).bRefOfCurTidIsLtr[dIdx][iCurTid as usize] {
                let kiAvailableLtrPos = self.m_uiSpatialLayersInTemporal[dIdx] as usize
                    + (*(*pCtx).pVaa).uiMarkLongTermPicIdx as usize;
                Self::WelsExchangeSpatialPictures(
                    &mut self.m_pSpatialPic[dIdx][kiAvailableLtrPos],
                    &mut self.m_pSpatialPic[dIdx][iCurTid as usize],
                );
                (*pCtx).bRefOfCurTidIsLtr[dIdx][iCurTid as usize] = false;
            }
            Self::WelsExchangeSpatialPictures(
                &mut self.m_pSpatialPic[dIdx][kiCurPos as usize],
                &mut self.m_pSpatialPic[dIdx][iCurTid as usize],
            );
        }

        0
    }

    pub unsafe fn BilateralDenoising(&self, pSrc: *mut SPicture, kiWidth: i32, kiHeight: i32) {
        if self.m_pInterfaceVp.is_null() || pSrc.is_null() {
            return;
        }
        let mut sSrcPixMap = SPixMap::default();
        sSrcPixMap.pPixel[0] = (*pSrc).pData[0] as *mut c_void;
        sSrcPixMap.pPixel[1] = (*pSrc).pData[1] as *mut c_void;
        sSrcPixMap.pPixel[2] = (*pSrc).pData[2] as *mut c_void;
        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.sRect.iRectWidth = kiWidth;
        sSrcPixMap.sRect.iRectHeight = kiHeight;
        sSrcPixMap.iStride[0] = (*pSrc).iLineSize[0];
        sSrcPixMap.iStride[1] = (*pSrc).iLineSize[1];
        sSrcPixMap.iStride[2] = (*pSrc).iLineSize[2];
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        (*self.m_pInterfaceVp).Process(EMethods::METHOD_DENOISE as i32, &mut sSrcPixMap, std::ptr::null_mut());
    }

    pub unsafe fn DownsamplePadding(
        &self,
        pSrc: *mut SPicture,
        pDstPic: *mut SPicture,
        iSrcWidth: i32,
        iSrcHeight: i32,
        mut iShrinkWidth: i32,
        mut iShrinkHeight: i32,
        iTargetWidth: i32,
        iTargetHeight: i32,
        bForceCopy: bool,
    ) -> i32 {
        let mut iRet = 0;
        let mut sSrcPixMap = SPixMap::default();
        let mut sDstPicMap = SPixMap::default();

        sSrcPixMap.pPixel[0] = (*pSrc).pData[0] as *mut c_void;
        sSrcPixMap.pPixel[1] = (*pSrc).pData[1] as *mut c_void;
        sSrcPixMap.pPixel[2] = (*pSrc).pData[2] as *mut c_void;
        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.sRect.iRectWidth = iSrcWidth;
        sSrcPixMap.sRect.iRectHeight = iSrcHeight;
        sSrcPixMap.iStride[0] = (*pSrc).iLineSize[0];
        sSrcPixMap.iStride[1] = (*pSrc).iLineSize[1];
        sSrcPixMap.iStride[2] = (*pSrc).iLineSize[2];
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight || bForceCopy {
            sDstPicMap.pPixel[0] = (*pDstPic).pData[0] as *mut c_void;
            sDstPicMap.pPixel[1] = (*pDstPic).pData[1] as *mut c_void;
            sDstPicMap.pPixel[2] = (*pDstPic).pData[2] as *mut c_void;
            sDstPicMap.iSizeInBits = g_kiPixMapSizeInBits;
            sDstPicMap.sRect.iRectWidth = iShrinkWidth;
            sDstPicMap.sRect.iRectHeight = iShrinkHeight;
            sDstPicMap.iStride[0] = (*pDstPic).iLineSize[0];
            sDstPicMap.iStride[1] = (*pDstPic).iLineSize[1];
            sDstPicMap.iStride[2] = (*pDstPic).iLineSize[2];
            sDstPicMap.eFormat = VideoFormat::videoFormatI420;

            if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight {
                if !self.m_pInterfaceVp.is_null() {
                    iRet = (*self.m_pInterfaceVp).Process(
                        EMethods::METHOD_DOWNSAMPLE as i32,
                        &mut sSrcPixMap,
                        &mut sDstPicMap,
                    );
                }
            } else {
                WelsMoveMemory_c(
                    (*pDstPic).pData[0],
                    (*pDstPic).pData[1],
                    (*pDstPic).pData[2],
                    (*pDstPic).iLineSize[0],
                    (*pDstPic).iLineSize[1],
                    (*pDstPic).iLineSize[2],
                    (*pSrc).pData[0],
                    (*pSrc).pData[1],
                    (*pSrc).pData[2],
                    (*pSrc).iLineSize[0],
                    (*pSrc).iLineSize[1],
                    (*pSrc).iLineSize[2],
                    iSrcWidth,
                    iSrcHeight,
                );
            }
        } else {
            sDstPicMap = sSrcPixMap;
        }

        iShrinkWidth -= iShrinkWidth & 1;
        iShrinkHeight -= iShrinkHeight & 1;
        self.Padding(
            sDstPicMap.pPixel[0] as *mut u8,
            sDstPicMap.pPixel[1] as *mut u8,
            sDstPicMap.pPixel[2] as *mut u8,
            sDstPicMap.iStride[0],
            sDstPicMap.iStride[1],
            iShrinkWidth,
            iTargetWidth,
            iShrinkHeight,
            iTargetHeight,
        );

        iRet
    }

    pub unsafe fn VaaCalculation(
        &self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
        bCalculateSQDiff: bool,
        bCalculateVar: bool,
        bCalculateBGD: bool,
    ) {
        if pVaaInfo.is_null() || pCurPicture.is_null() || pRefPicture.is_null() || self.m_pInterfaceVp.is_null() {
            return;
        }
        (*pVaaInfo).sVaaCalcInfo.pCurY = (*pCurPicture).pData[0];
        (*pVaaInfo).sVaaCalcInfo.pRefY = (*pRefPicture).pData[0];

        let mut sCurPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();
        let mut calc_param = SVAACalcParam::default();

        sCurPixMap.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
        sCurPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sCurPixMap.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
        sCurPixMap.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
        sCurPixMap.iStride[0] = (*pCurPicture).iLineSize[0];
        sCurPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
        sRefPixMap.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
        sRefPixMap.iStride[0] = (*pRefPicture).iLineSize[0];
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        calc_param.iCalcVar = bCalculateVar;
        calc_param.iCalcBgd = bCalculateBGD;
        calc_param.iCalcSsd = bCalculateSQDiff;
        calc_param.pCalcResult = &mut (*pVaaInfo).sVaaCalcInfo;

        let method = EMethods::METHOD_VAA_STATISTICS as i32;
        (*self.m_pInterfaceVp).Set(method, &mut calc_param as *mut _ as *mut c_void);
        (*self.m_pInterfaceVp).Process(method, &mut sCurPixMap, &mut sRefPixMap);
    }

    pub unsafe fn BackgroundDetection(
        &self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
        bDetectFlag: bool,
    ) {
        if pVaaInfo.is_null() || pCurPicture.is_null() {
            return;
        }
        if bDetectFlag && !pRefPicture.is_null() && !self.m_pInterfaceVp.is_null() {
            (*pVaaInfo).iPicWidth = (*pCurPicture).iWidthInPixel;
            (*pVaaInfo).iPicHeight = (*pCurPicture).iHeightInPixel;
            (*pVaaInfo).iPicStride = (*pCurPicture).iLineSize[0];
            (*pVaaInfo).iPicStrideUV = (*pCurPicture).iLineSize[1];
            (*pVaaInfo).pCurY = (*pCurPicture).pData[0];
            (*pVaaInfo).pRefY = (*pRefPicture).pData[0];
            (*pVaaInfo).pCurU = (*pCurPicture).pData[1];
            (*pVaaInfo).pRefU = (*pRefPicture).pData[1];
            (*pVaaInfo).pCurV = (*pCurPicture).pData[2];
            (*pVaaInfo).pRefV = (*pRefPicture).pData[2];

            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();
            let mut BGDParam = SBGDInterface::default();

            sSrcPixMap.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
            sSrcPixMap.pPixel[1] = (*pCurPicture).pData[1] as *mut c_void;
            sSrcPixMap.pPixel[2] = (*pCurPicture).pData[2] as *mut c_void;
            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = (*pCurPicture).iLineSize[0];
            sSrcPixMap.iStride[1] = (*pCurPicture).iLineSize[1];
            sSrcPixMap.iStride[2] = (*pCurPicture).iLineSize[2];
            sSrcPixMap.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            sRefPixMap.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
            sRefPixMap.pPixel[1] = (*pRefPicture).pData[1] as *mut c_void;
            sRefPixMap.pPixel[2] = (*pRefPicture).pData[2] as *mut c_void;
            sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sRefPixMap.iStride[0] = (*pRefPicture).iLineSize[0];
            sRefPixMap.iStride[1] = (*pRefPicture).iLineSize[1];
            sRefPixMap.iStride[2] = (*pRefPicture).iLineSize[2];
            sRefPixMap.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
            sRefPixMap.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
            sRefPixMap.eFormat = VideoFormat::videoFormatI420;

            BGDParam.pBackgroundMbFlag = (*pVaaInfo).pVaaBackgroundMbFlag;
            BGDParam.pCalcRes = &mut (*pVaaInfo).sVaaCalcInfo;

            let method = EMethods::METHOD_BACKGROUND_DETECTION as i32;
            (*self.m_pInterfaceVp).Set(method, &mut BGDParam as *mut _ as *mut c_void);
            (*self.m_pInterfaceVp).Process(method, &mut sSrcPixMap, &mut sRefPixMap);
        } else if !(*pVaaInfo).pVaaBackgroundMbFlag.is_null() {
            let iPicWidthInMb = ((*pCurPicture).iWidthInPixel + 15) >> 4;
            let iPicHeightInMb = ((*pCurPicture).iHeightInPixel + 15) >> 4;
            std::ptr::write_bytes(
                (*pVaaInfo).pVaaBackgroundMbFlag as *mut u8,
                0,
                (iPicWidthInMb * iPicHeightInMb) as usize,
            );
        }
    }

    pub unsafe fn AdaptiveQuantCalculation(
        &self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
    ) {
        if pVaaInfo.is_null() || pCurPicture.is_null() || pRefPicture.is_null() || self.m_pInterfaceVp.is_null() {
            return;
        }
        (*pVaaInfo).sAdaptiveQuantParam.pCalcResult = &mut (*pVaaInfo).sVaaCalcInfo;
        (*pVaaInfo).sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp = 0;

        let method = EMethods::METHOD_ADAPTIVE_QUANT as i32;
        let mut pSrc = SPixMap::default();
        let mut pRef = SPixMap::default();

        pSrc.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
        pSrc.iSizeInBits = g_kiPixMapSizeInBits;
        pSrc.iStride[0] = (*pCurPicture).iLineSize[0];
        pSrc.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
        pSrc.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
        pSrc.eFormat = VideoFormat::videoFormatI420;

        pRef.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
        pRef.iSizeInBits = g_kiPixMapSizeInBits;
        pRef.iStride[0] = (*pRefPicture).iLineSize[0];
        pRef.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
        pRef.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
        pRef.eFormat = VideoFormat::videoFormatI420;

        (*self.m_pInterfaceVp).Set(method, &mut (*pVaaInfo).sAdaptiveQuantParam as *mut _ as *mut c_void);
        let iRet = (*self.m_pInterfaceVp).Process(method, &mut pSrc, &mut pRef);
        if iRet == 0 {
            (*self.m_pInterfaceVp).Get(method, &mut (*pVaaInfo).sAdaptiveQuantParam as *mut _ as *mut c_void);
        }
    }

    pub unsafe fn Padding(
        &self,
        pSrcY: *mut u8,
        pSrcU: *mut u8,
        pSrcV: *mut u8,
        iStrideY: i32,
        iStrideUV: i32,
        iActualWidth: i32,
        iPaddingWidth: i32,
        iActualHeight: i32,
        iPaddingHeight: i32,
    ) {
        if pSrcY.is_null() || pSrcU.is_null() || pSrcV.is_null() {
            return;
        }

        if iPaddingHeight > iActualHeight {
            for i in iActualHeight..iPaddingHeight {
                std::ptr::write_bytes(pSrcY.offset((i * iStrideY) as isize), 0, iActualWidth as usize);
                if (i & 1) == 0 {
                    std::ptr::write_bytes(
                        pSrcU.offset(((i / 2) * iStrideUV) as isize),
                        0x80,
                        (iActualWidth / 2) as usize,
                    );
                    std::ptr::write_bytes(
                        pSrcV.offset(((i / 2) * iStrideUV) as isize),
                        0x80,
                        (iActualWidth / 2) as usize,
                    );
                }
            }
        }

        if iPaddingWidth > iActualWidth {
            let diff = (iPaddingWidth - iActualWidth) as usize;
            let diffUV = diff / 2;
            for i in 0..iPaddingHeight {
                std::ptr::write_bytes(pSrcY.offset((i * iStrideY + iActualWidth) as isize), 0, diff);
                if (i & 1) == 0 {
                    std::ptr::write_bytes(
                        pSrcU.offset(((i / 2) * iStrideUV + iActualWidth / 2) as isize),
                        0x80,
                        diffUV,
                    );
                    std::ptr::write_bytes(
                        pSrcV.offset(((i / 2) * iStrideUV + iActualWidth / 2) as isize),
                        0x80,
                        diffUV,
                    );
                }
            }
        }
    }

    pub unsafe fn WelsExchangeSpatialPictures(
        ppPic1: *mut *mut SPicture,
        ppPic2: *mut *mut SPicture,
    ) {
        if !ppPic1.is_null() && !ppPic2.is_null() {
            let tmp = *ppPic1;
            *ppPic1 = *ppPic2;
            *ppPic2 = tmp;
        }
    }

    pub unsafe fn InitLastSpatialPictures(&mut self, pCtx: *mut sWelsEncCtx) -> i32 {
        let pParam = (*pCtx).pSvcParam;
        let kiDlayerCount = (*pParam).iSpatialLayerNum;
        let mut iDlayerIndex = 0;

        if (*pParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            while iDlayerIndex < MAX_DEPENDENCY_LAYER {
                self.m_pLastSpatialPicture[iDlayerIndex][0] = std::ptr::null_mut();
                self.m_pLastSpatialPicture[iDlayerIndex][1] = std::ptr::null_mut();
                iDlayerIndex += 1;
            }
        } else {
            while iDlayerIndex < kiDlayerCount as usize {
                let kiLayerInTemporal = self.m_uiSpatialLayersInTemporal[iDlayerIndex as usize] as usize;
                self.m_pLastSpatialPicture[iDlayerIndex as usize][0] =
                    self.m_pSpatialPic[iDlayerIndex as usize][kiLayerInTemporal.saturating_sub(2)];
                self.m_pLastSpatialPicture[iDlayerIndex as usize][1] = std::ptr::null_mut();
                iDlayerIndex += 1;
            }
            while (iDlayerIndex as usize) < MAX_DEPENDENCY_LAYER {
                self.m_pLastSpatialPicture[iDlayerIndex as usize][0] = std::ptr::null_mut();
                self.m_pLastSpatialPicture[iDlayerIndex as usize][1] = std::ptr::null_mut();
                iDlayerIndex += 1;
            }
        }

        0
    }

    pub unsafe fn WelsMoveMemoryWrapper(
        &self,
        pSvcParam: *mut SWelsSvcCodingParam,
        pDstPic: *mut SPicture,
        kpSrc: *const SSourcePicture,
        kiTargetWidth: i32,
        kiTargetHeight: i32,
    ) -> i32 {
        if (VideoFormat::videoFormatI420 as i32) != ((*kpSrc).iColorFormat & !(-0x80000000i32)) {
            return ENC_RETURN_INVALIDINPUT;
        }

        let mut iSrcWidth = (*kpSrc).iPicWidth;
        let mut iSrcHeight = (*kpSrc).iPicHeight;

        if iSrcHeight > kiTargetHeight {
            iSrcHeight = kiTargetHeight;
        }
        if iSrcWidth > kiTargetWidth {
            iSrcWidth = kiTargetWidth;
        }

        if (iSrcWidth & 1) != 0 {
            iSrcWidth -= 1;
        }
        if (iSrcHeight & 1) != 0 {
            iSrcHeight -= 1;
        }

        let kiSrcTopOffsetY = (*pSvcParam).SUsedPicRect.iTop;
        let kiSrcTopOffsetUV = kiSrcTopOffsetY >> 1;
        let kiSrcLeftOffsetY = (*pSvcParam).SUsedPicRect.iLeft;
        let kiSrcLeftOffsetUV = kiSrcLeftOffsetY >> 1;

        let iSrcOffset0 = (*kpSrc).iStride[0] * kiSrcTopOffsetY + kiSrcLeftOffsetY;
        let iSrcOffset1 = (*kpSrc).iStride[1] * kiSrcTopOffsetUV + kiSrcLeftOffsetUV;
        let iSrcOffset2 = (*kpSrc).iStride[2] * kiSrcTopOffsetUV + kiSrcLeftOffsetUV;

        let pSrcY = if !(*kpSrc).pData[0].is_null() {
            (*kpSrc).pData[0].offset(iSrcOffset0 as isize)
        } else {
            std::ptr::null_mut()
        };
        let pSrcU = if !(*kpSrc).pData[1].is_null() {
            (*kpSrc).pData[1].offset(iSrcOffset1 as isize)
        } else {
            std::ptr::null_mut()
        };
        let pSrcV = if !(*kpSrc).pData[2].is_null() {
            (*kpSrc).pData[2].offset(iSrcOffset2 as isize)
        } else {
            std::ptr::null_mut()
        };

        let kiSrcStrideY = (*kpSrc).iStride[0];
        let kiSrcStrideU = (*kpSrc).iStride[1];
        let kiSrcStrideV = (*kpSrc).iStride[2];

        let pDstY = (*pDstPic).pData[0];
        let pDstU = (*pDstPic).pData[1];
        let pDstV = (*pDstPic).pData[2];
        let kiDstStrideY = (*pDstPic).iLineSize[0];
        let kiDstStrideU = (*pDstPic).iLineSize[1];
        let kiDstStrideV = (*pDstPic).iLineSize[2];

        if !pSrcY.is_null() {
            if iSrcWidth <= 0 || iSrcHeight <= 0 || (iSrcWidth * iSrcHeight > (MAX_MBS_PER_FRAME << 8)) {
                return ENC_RETURN_INVALIDINPUT;
            }
            if kiSrcTopOffsetY >= iSrcHeight
                || kiSrcLeftOffsetY >= iSrcWidth
                || iSrcWidth > kiSrcStrideY
                || (iSrcWidth >> 1) > kiSrcStrideU
                || (iSrcWidth >> 1) > kiSrcStrideV
            {
                return ENC_RETURN_INVALIDINPUT;
            }
        }
        if !pDstY.is_null() {
            if kiTargetWidth <= 0
                || kiTargetHeight <= 0
                || (kiTargetWidth * kiTargetHeight > (MAX_MBS_PER_FRAME << 8))
            {
                return ENC_RETURN_INVALIDINPUT;
            }
            if kiTargetWidth > kiDstStrideY
                || (kiTargetWidth >> 1) > kiDstStrideU
                || (kiTargetWidth >> 1) > kiDstStrideV
            {
                return ENC_RETURN_INVALIDINPUT;
            }
        }

        if pSrcY.is_null()
            || pSrcU.is_null()
            || pSrcV.is_null()
            || pDstY.is_null()
            || pDstU.is_null()
            || pDstV.is_null()
            || (iSrcWidth & 1) != 0
            || (iSrcHeight & 1) != 0
        {
            return ENC_RETURN_INVALIDINPUT;
        }

        WelsMoveMemory_c(
            pDstY,
            pDstU,
            pDstV,
            kiDstStrideY,
            kiDstStrideU,
            kiDstStrideV,
            pSrcY,
            pSrcU,
            pSrcV,
            kiSrcStrideY,
            kiSrcStrideU,
            kiSrcStrideV,
            iSrcWidth,
            iSrcHeight,
        );

        if kiTargetWidth > iSrcWidth || kiTargetHeight > iSrcHeight {
            self.Padding(
                pDstY,
                pDstU,
                pDstV,
                kiDstStrideY,
                kiDstStrideU,
                iSrcWidth,
                kiTargetWidth,
                iSrcHeight,
                kiTargetHeight,
            );
        }

        ENC_RETURN_SUCCESS
    }

    pub unsafe fn GetSceneChangeFlag(&self, eSceneChangeIdc: ESceneChangeIdc) -> bool {
        eSceneChangeIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE
    }

    pub unsafe fn DetectSceneChange(
        &mut self,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
    ) -> ESceneChangeIdc {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.DetectSceneChangeScreen(pCurPicture, pRefPicture)
        } else {
            self.DetectSceneChangeVideo(pCurPicture, pRefPicture)
        }
    }

    unsafe fn DetectSceneChangeVideo(
        &mut self,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
    ) -> ESceneChangeIdc {
        if self.m_pInterfaceVp.is_null() || pCurPicture.is_null() || pRefPicture.is_null() {
            return ESceneChangeIdc::SIMILAR_SCENE;
        }

        let iMethodIdx = EMethods::METHOD_SCENE_CHANGE_DETECTION_VIDEO as i32;
        let mut sSceneChangeDetectResult = SSceneChangeResult::default();
        let mut sSrcPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();

        sSrcPixMap.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.iStride[0] = (*pCurPicture).iLineSize[0];
        sSrcPixMap.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
        sSrcPixMap.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.iStride[0] = (*pRefPicture).iLineSize[0];
        sRefPixMap.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
        sRefPixMap.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        let iRet = (*self.m_pInterfaceVp).Process(iMethodIdx, &mut sSrcPixMap, &mut sRefPixMap);
        if iRet == 0 {
            (*self.m_pInterfaceVp).Get(iMethodIdx, &mut sSceneChangeDetectResult as *mut _ as *mut c_void);
        }
        sSceneChangeDetectResult.eSceneChangeIdc
    }

    unsafe fn DetectSceneChangeScreen(
        &mut self,
        pCurPicture: *mut SPicture,
        _pRef: *mut SPicture,
    ) -> ESceneChangeIdc {
        let pCtx = self.m_pEncCtx;
        if pCtx.is_null() || (*pCtx).pVaa.is_null() || pCurPicture.is_null() {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        let pSvcParam = (*pCtx).pSvcParam;
        let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
        let iTargetDid = (*pSvcParam).iSpatialLayerNum - 1;
        if iTargetDid != 0 {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        let pRefPicList = self.m_pSpatialPic[iTargetDid as usize].as_ptr().cast_mut().add(1);
        let mut sAvailableRefParam = [SRefInfoParam::default(); MAX_REF_PIC_COUNT];
        let mut iAvailableRefNum = 0;
        let mut iAvailableSceneRefNum = 0;

        let pParamInternal = &(*pSvcParam).sDependencyLayers[0];
        let gopMask = (*pSvcParam).uiGopSize as i32 - 1;
        let iCurTid = pParamInternal.uiCodingIdx2TemporalId[(pParamInternal.iCodingIndex & gopMask) as usize];
        if iCurTid == INVALID_TEMPORAL_ID {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        let iClosestLtrFrameNum =
            (*(*pCtx).pLtr.add(iTargetDid as usize)).iLastLtrIdx[iCurTid as usize];
        if (*pSvcParam).bEnableLongTermReference {
            self.GetAvailableRefListLosslessScreenRefSelection(
                pRefPicList,
                iCurTid,
                iClosestLtrFrameNum,
                sAvailableRefParam.as_mut_ptr(),
                &mut iAvailableRefNum,
                &mut iAvailableSceneRefNum,
            );
        } else {
            self.GetAvailableRefList(
                pRefPicList,
                iCurTid,
                iClosestLtrFrameNum,
                sAvailableRefParam.as_mut_ptr(),
                &mut iAvailableRefNum,
                &mut iAvailableSceneRefNum,
            );
        }

        if iAvailableRefNum == 0 {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        let mut sSrcMap = SPixMap::default();
        let mut sRefMap = SPixMap::default();
        let mut sLtrJudgement = SRefJudgement::default();
        let mut sSceneLtrJudgement = SRefJudgement::default();
        let mut sLtrSaved = SRefInfoParam::default();
        let mut sSceneLtrSaved = SRefInfoParam::default();

        let mut iNumOfLargeChange = 0;
        let mut iNumOfMediumChangeToLtr = 0;

        self.InitPixMap(pCurPicture, &mut sSrcMap);
        self.InitRefJudgement(&mut sLtrJudgement);
        self.InitRefJudgement(&mut sSceneLtrJudgement);

        let iNegligibleMotionBlocks = (((*pCurPicture).iWidthInPixel >> 3)
            * ((*pCurPicture).iHeightInPixel >> 3)) as f32
            * STATIC_SCENE_MOTION_RATIO;
        let iNegligibleBlocks = iNegligibleMotionBlocks as i32;

        let iSceneChangeMethodIdx = EMethods::METHOD_SCENE_CHANGE_DETECTION_SCREEN as i32;

        for iScdIdx in 0..iAvailableRefNum {
            let pCurBlockStaticPointer = (*pVaaExt).pVaaBlockStaticIdc[iScdIdx as usize];
            let mut sSceneChangeResult = SSceneChangeResult::default();
            sSceneChangeResult.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;
            sSceneChangeResult.pStaticBlockIdc = pCurBlockStaticPointer;

            let pRefPicInfo = &mut sAvailableRefParam[iScdIdx as usize];
            let pRefPic = pRefPicInfo.pRefPicture;
            if pRefPic.is_null() {
                continue;
            }
            self.InitPixMap(pRefPic, &mut sRefMap);

            let bIsClosestLtrFrame = (*pRefPic).iLongTermPicNum == iClosestLtrFrameNum;
            if iScdIdx == 0 {
                let pScrollDetectInfo = &mut (*pVaaExt).sScrollDetectInfo;
                *pScrollDetectInfo = SScrollDetectionParam::default();

                let iMethodIdx = EMethods::METHOD_SCROLL_DETECTION as i32;
                if !self.m_pInterfaceVp.is_null() {
                    (*self.m_pInterfaceVp).Set(iMethodIdx, pScrollDetectInfo as *mut _ as *mut c_void);
                    let ret = (*self.m_pInterfaceVp).Process(iMethodIdx, &mut sSrcMap, &mut sRefMap);
                    if ret == 0 {
                        (*self.m_pInterfaceVp).Get(iMethodIdx, pScrollDetectInfo as *mut _ as *mut c_void);
                        if pScrollDetectInfo.bScrollDetectFlag {
                            pScrollDetectInfo.iScrollMvX = pScrollDetectInfo
                                .iScrollMvX
                                .clamp(-(*pCtx).iMvRange, (*pCtx).iMvRange);
                            pScrollDetectInfo.iScrollMvY = pScrollDetectInfo
                                .iScrollMvY
                                .clamp(-(*pCtx).iMvRange, (*pCtx).iMvRange);
                        }
                    }
                }
                sSceneChangeResult.sScrollResult = (*pVaaExt).sScrollDetectInfo;
            }

            if !self.m_pInterfaceVp.is_null() {
                (*self.m_pInterfaceVp).Set(iSceneChangeMethodIdx, &mut sSceneChangeResult as *mut _ as *mut c_void);
                let ret = (*self.m_pInterfaceVp).Process(iSceneChangeMethodIdx, &mut sSrcMap, &mut sRefMap);
                if ret == 0 {
                    (*self.m_pInterfaceVp).Get(iSceneChangeMethodIdx, &mut sSceneChangeResult as *mut _ as *mut c_void);

                    let iFrameComplexity = sSceneChangeResult.iFrameComplexity;
                    let iSceneDetectIdc = sSceneChangeResult.eSceneChangeIdc;
                    let iMotionBlockNum = sSceneChangeResult.iMotionBlockNum;

                    let bCurRefIsSceneLtr = (*pRefPic).bIsSceneLTR;
                    let iRefPicAvQP = (*pRefPic).iFrameAverageQp;

                    if iSceneDetectIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE {
                        iNumOfLargeChange += 1;
                    }
                    if bCurRefIsSceneLtr && iSceneDetectIdc != ESceneChangeIdc::SIMILAR_SCENE {
                        iNumOfMediumChangeToLtr += 1;
                    }

                    if self.JudgeBestRef(pRefPic, &sLtrJudgement, iFrameComplexity, bIsClosestLtrFrame) {
                        self.SaveBestRefToJudgement(iRefPicAvQP, iFrameComplexity, &mut sLtrJudgement);
                        self.SaveBestRefToLocal(pRefPicInfo, &sSceneChangeResult, &mut sLtrSaved);
                    }
                    if bCurRefIsSceneLtr
                        && self.JudgeBestRef(pRefPic, &sSceneLtrJudgement, iFrameComplexity, bIsClosestLtrFrame)
                    {
                        self.SaveBestRefToJudgement(iRefPicAvQP, iFrameComplexity, &mut sSceneLtrJudgement);
                        self.SaveBestRefToLocal(pRefPicInfo, &sSceneChangeResult, &mut sSceneLtrSaved);
                    }

                    if iMotionBlockNum <= iNegligibleBlocks {
                        break;
                    }
                }
            }
        }

        let iVaaFrameSceneChangeIdc = if iNumOfLargeChange == iAvailableRefNum {
            ESceneChangeIdc::LARGE_CHANGED_SCENE
        } else if iNumOfMediumChangeToLtr == iAvailableSceneRefNum && iAvailableSceneRefNum != 0 {
            ESceneChangeIdc::MEDIUM_CHANGED_SCENE
        } else {
            ESceneChangeIdc::SIMILAR_SCENE
        };

        self.SaveBestRefToVaa(&sLtrSaved, &mut (*pVaaExt).sVaaStrBestRefCandidate[0]);
        if !sLtrSaved.pRefPicture.is_null() {
            (*pVaaExt).iVaaBestRefFrameNum = (*sLtrSaved.pRefPicture).iFrameNum;
        }
        (*pVaaExt).pVaaBestBlockStaticIdc = sLtrSaved.pBestBlockStaticIdc;

        if iAvailableSceneRefNum > 0 {
            self.SaveBestRefToVaa(&sSceneLtrSaved, &mut (*pVaaExt).sVaaLtrBestRefCandidate[0]);
        }

        (*pVaaExt).iNumOfAvailableRef = 1;
        iVaaFrameSceneChangeIdc
    }

    unsafe fn InitPixMap(&self, pPicture: *const SPicture, pPixMap: *mut SPixMap) {
        if !pPicture.is_null() && !pPixMap.is_null() {
            (*pPixMap).pPixel[0] = (*pPicture).pData[0] as *mut c_void;
            (*pPixMap).pPixel[1] = (*pPicture).pData[1] as *mut c_void;
            (*pPixMap).pPixel[2] = (*pPicture).pData[2] as *mut c_void;
            (*pPixMap).iSizeInBits = std::mem::size_of::<u8>() as i32;
            (*pPixMap).iStride[0] = (*pPicture).iLineSize[0];
            (*pPixMap).iStride[1] = (*pPicture).iLineSize[1];
            (*pPixMap).sRect.iRectWidth = (*pPicture).iWidthInPixel;
            (*pPixMap).sRect.iRectHeight = (*pPicture).iHeightInPixel;
            (*pPixMap).eFormat = VideoFormat::videoFormatI420;
        }
    }

    unsafe fn InitRefJudgement(&self, pRefJudgement: *mut SRefJudgement) {
        if !pRefJudgement.is_null() {
            (*pRefJudgement).iMinFrameComplexity = i32::MAX as i64;
            (*pRefJudgement).iMinFrameComplexity08 = i32::MAX as i64;
            (*pRefJudgement).iMinFrameComplexity11 = i32::MAX as i64;
            (*pRefJudgement).iMinFrameNumGap = i32::MAX;
            (*pRefJudgement).iMinFrameQp = i32::MAX;
        }
    }

    unsafe fn JudgeBestRef(
        &self,
        pRefPic: *mut SPicture,
        sRefJudgement: &SRefJudgement,
        iFrameComplexity: i64,
        bIsClosestLtrFrame: bool,
    ) -> bool {
        if bIsClosestLtrFrame {
            iFrameComplexity < sRefJudgement.iMinFrameComplexity11
        } else {
            (iFrameComplexity < sRefJudgement.iMinFrameComplexity08)
                || ((iFrameComplexity <= sRefJudgement.iMinFrameComplexity11)
                    && (!pRefPic.is_null() && ((*pRefPic).iFrameAverageQp < sRefJudgement.iMinFrameQp)))
        }
    }

    unsafe fn SaveBestRefToJudgement(
        &self,
        iRefPictureAvQP: i32,
        iComplexity: i64,
        pRefJudgement: *mut SRefJudgement,
    ) {
        if !pRefJudgement.is_null() {
            (*pRefJudgement).iMinFrameQp = iRefPictureAvQP;
            (*pRefJudgement).iMinFrameComplexity = iComplexity;
            (*pRefJudgement).iMinFrameComplexity08 = (iComplexity as f64 * 0.8) as i64;
            (*pRefJudgement).iMinFrameComplexity11 = (iComplexity as f64 * 1.1) as i64;
        }
    }

    unsafe fn SaveBestRefToLocal(
        &self,
        pRefPicInfo: *mut SRefInfoParam,
        sSceneChangeResult: &SSceneChangeResult,
        pRefSaved: *mut SRefInfoParam,
    ) {
        if !pRefSaved.is_null() && !pRefPicInfo.is_null() {
            *pRefSaved = *pRefPicInfo;
            (*pRefSaved).pBestBlockStaticIdc = sSceneChangeResult.pStaticBlockIdc;
        }
    }

    unsafe fn SaveBestRefToVaa(&self, sRefSaved: &SRefInfoParam, pVaaBestRef: *mut SRefInfoParam) {
        if !pVaaBestRef.is_null() {
            *pVaaBestRef = *sRefSaved;
        }
    }

    unsafe fn GetAvailableRefListLosslessScreenRefSelection(
        &self,
        pRefPicList: *mut *mut SPicture,
        iCurTid: u8,
        iClosestLtrFrameNum: i32,
        pAvailableRefParam: *mut SRefInfoParam,
        pAvailableRefNum: *mut i32,
        pAvailableSceneRefNum: *mut i32,
    ) {
        let iSourcePicNum = self.m_iAvaliableRefInSpatialPicList;
        if iSourcePicNum <= 0 {
            *pAvailableRefNum = 0;
            *pAvailableSceneRefNum = 0;
            return;
        }

        let bCurFrameMarkedAsSceneLtr = (*self.m_pEncCtx).bCurFrameMarkedAsSceneLtr;
        *pAvailableRefNum = 1;
        *pAvailableSceneRefNum = 0;

        let mut i = iSourcePicNum - 1;
        while i >= 0 {
            let pRefPic = *pRefPicList.offset(i as isize);
            if pRefPic.is_null()
                || !(*pRefPic).bUsedAsRef
                || !(*pRefPic).bIsLongRef
                || (bCurFrameMarkedAsSceneLtr && !(*pRefPic).bIsSceneLTR)
            {
                i -= 1;
                continue;
            }

            let uiRefTid = (*pRefPic).uiTemporalId;
            let bRefRealLtr = (*pRefPic).bIsSceneLTR;

            if bRefRealLtr || (iCurTid == 0 && uiRefTid == 0) || (uiRefTid < iCurTid) {
                let idx = if (*pRefPic).iLongTermPicNum == iClosestLtrFrameNum {
                    0
                } else {
                    let old = *pAvailableRefNum;
                    *pAvailableRefNum += 1;
                    old
                };
                let param = &mut *pAvailableRefParam.offset(idx as isize);
                param.pRefPicture = pRefPic;
                param.iSrcListIdx = i + 1;
                if bRefRealLtr {
                    *pAvailableSceneRefNum += 1;
                }
            }

            i -= 1;
        }

        if (*pAvailableRefParam.offset(0)).pRefPicture.is_null() {
            let mut j = 1;
            while j < *pAvailableRefNum {
                let pPrev = &mut *pAvailableRefParam.offset((j - 1) as isize);
                let pCur = &*pAvailableRefParam.offset(j as isize);
                pPrev.pRefPicture = pCur.pRefPicture;
                pPrev.iSrcListIdx = pCur.iSrcListIdx;
                j += 1;
            }
            let last = &mut *pAvailableRefParam.offset((*pAvailableRefNum - 1) as isize);
            last.pRefPicture = std::ptr::null_mut();
            last.iSrcListIdx = 0;
            *pAvailableRefNum -= 1;
        }
    }

    unsafe fn GetAvailableRefList(
        &self,
        pSrcPicList: *mut *mut SPicture,
        iCurTid: u8,
        _iClosestLtrFrameNum: i32,
        pAvailableRefList: *mut SRefInfoParam,
        pAvailableRefNum: *mut i32,
        pAvailableSceneRefNum: *mut i32,
    ) {
        let iSourcePicNum = self.m_iAvaliableRefInSpatialPicList;
        if iSourcePicNum <= 0 {
            *pAvailableRefNum = 0;
            *pAvailableSceneRefNum = 0;
            return;
        }

        *pAvailableRefNum = 0;
        *pAvailableSceneRefNum = 0;

        let mut i = iSourcePicNum - 1;
        while i >= 0 {
            let pRefPic = *pSrcPicList.offset(i as isize);
            if pRefPic.is_null() || !(*pRefPic).bUsedAsRef {
                i -= 1;
                continue;
            }

            let uiRefTid = (*pRefPic).uiTemporalId;
            if uiRefTid <= iCurTid {
                let param = &mut *pAvailableRefList.offset(*pAvailableRefNum as isize);
                param.pRefPicture = pRefPic;
                param.iSrcListIdx = i + 1;
                *pAvailableRefNum += 1;
            }

            i -= 1;
        }
    }

    /// `wels_preprocess.cpp:811`. Picks the reference picture whose macroblock-type
    /// array feeds the complexity analyser: the first confirmed long-term reference
    /// when LTR is on and a T0 frame was lost, otherwise the first usable short-term
    /// reference at or below the current temporal id.
    pub unsafe fn SetRefMbType(&self, pCtx: *mut sWelsEncCtx, pRefMbTypeArray: *mut *mut u32, _iRefPicType: i32) {
        let uiTid = (*pCtx).uiTemporalId;
        let uiDid = (*pCtx).uiDependencyId;
        let pRefPicLlist = *(*pCtx).ppRefPicListExt.add(uiDid as usize);
        let pLtr = (*pCtx).pLtr.add(uiDid as usize);
        if pRefPicLlist.is_null() {
            return;
        }

        if (*(*pCtx).pSvcParam).bEnableLongTermReference && (*pLtr).bReceivedT0LostFlag && uiTid == 0 {
            for i in 0..(*pRefPicLlist).uiLongRefCount as usize {
                let pRef = (*pRefPicLlist).pLongRefList[i];
                if !pRef.is_null() && (*pRef).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    *pRefMbTypeArray = (*pRef).uiRefMbType;
                    break;
                }
            }
        } else {
            for i in 0..(*pRefPicLlist).uiShortRefCount as usize {
                let pRef = (*pRefPicLlist).pShortRefList[i];
                if !pRef.is_null()
                    && (*pRef).bUsedAsRef
                    && (*pRef).iFramePoc >= 0
                    && (*pRef).uiTemporalId <= uiTid
                {
                    *pRefMbTypeArray = (*pRef).uiRefMbType;
                    break;
                }
            }
        }
    }

    pub unsafe fn AnalyzePictureComplexity(
        &self,
        pCtx: *mut sWelsEncCtx,
        pCurPicture: *mut SPicture,
        pRefPicture: *mut SPicture,
        kiDependencyId: i32,
        bCalculateBGD: bool,
    ) {
        if pCtx.is_null() || (*pCtx).pSvcParam.is_null() || pCurPicture.is_null() || self.m_pInterfaceVp.is_null() {
            return;
        }

        let pSvcParam = (*pCtx).pSvcParam;
        if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
            let sComplexityAnalysisParam = &mut (*pVaaExt).sComplexityScreenParam;
            let pWelsSvcRc = &mut *(*pCtx).pWelsSvcRc.offset(kiDependencyId as isize);

            let _iComplexityAnalysisMode = if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
                GOM_SAD
            } else if (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
                GOM_VAR
            } else {
                return;
            };

            if !pWelsSvcRc.pGomForegroundBlockNum.is_null() {
                std::ptr::write_bytes(
                    pWelsSvcRc.pGomForegroundBlockNum as *mut u8,
                    0,
                    pWelsSvcRc.iGomSize as usize * std::mem::size_of::<i32>(),
                );
            }
            if !pWelsSvcRc.pCurrentFrameGomSad.is_null() {
                std::ptr::write_bytes(
                    pWelsSvcRc.pCurrentFrameGomSad as *mut u8,
                    0,
                    pWelsSvcRc.iGomSize as usize * std::mem::size_of::<i32>(),
                );
            }

            sComplexityAnalysisParam.iFrameComplexity = 0;
            sComplexityAnalysisParam.pGomComplexity = pWelsSvcRc.pCurrentFrameGomSad;
            sComplexityAnalysisParam.iGomNumInFrame = pWelsSvcRc.iGomSize;
            sComplexityAnalysisParam.iIdrFlag = if (*pCtx).eSliceType == EWelsSliceType::I_SLICE { 1 } else { 0 };
            sComplexityAnalysisParam.iMbRowInGom = GOM_H_SCC;
            sComplexityAnalysisParam.sScrollResult.bScrollDetectFlag = false;
            sComplexityAnalysisParam.sScrollResult.iScrollMvX = 0;
            sComplexityAnalysisParam.sScrollResult.iScrollMvY = 0;

            let iMethodIdx = EMethods::METHOD_COMPLEXITY_ANALYSIS_SCREEN as i32;
            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();

            sSrcPixMap.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = (*pCurPicture).iLineSize[0];
            sSrcPixMap.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            if !pRefPicture.is_null() {
                sRefPixMap.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
                sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
                sRefPixMap.iStride[0] = (*pRefPicture).iLineSize[0];
                sRefPixMap.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
                sRefPixMap.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
                sRefPixMap.eFormat = VideoFormat::videoFormatI420;
            }

            (*self.m_pInterfaceVp).Set(iMethodIdx, sComplexityAnalysisParam as *mut _ as *mut c_void);
            let iRet = (*self.m_pInterfaceVp).Process(iMethodIdx, &mut sSrcPixMap, &mut sRefPixMap);
            if iRet == 0 {
                (*self.m_pInterfaceVp).Get(iMethodIdx, sComplexityAnalysisParam as *mut _ as *mut c_void);
            }
        } else {
            let pVaaInfo = (*pCtx).pVaa;
            let sComplexityAnalysisParam = &mut (*pVaaInfo).sComplexityAnalysisParam;
            let pWelsSvcRc = &mut *(*pCtx).pWelsSvcRc.offset(kiDependencyId as isize);

            let iComplexityAnalysisMode = if (*pSvcParam).iRCMode as i32 == RC_QUALITY_MODE && (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
                FRAME_SAD
            } else if (((*pSvcParam).iRCMode as i32 == RC_BITRATE_MODE) || ((*pSvcParam).iRCMode as i32 == RC_TIMESTAMP_MODE))
                && (*pCtx).eSliceType == EWelsSliceType::P_SLICE
            {
                GOM_SAD
            } else if (((*pSvcParam).iRCMode as i32 == RC_BITRATE_MODE) || ((*pSvcParam).iRCMode as i32 == RC_TIMESTAMP_MODE))
                && (*pCtx).eSliceType == EWelsSliceType::I_SLICE
            {
                GOM_VAR
            } else {
                return;
            };

            sComplexityAnalysisParam.iComplexityAnalysisMode = iComplexityAnalysisMode;
            sComplexityAnalysisParam.pCalcResult = &mut (*pVaaInfo).sVaaCalcInfo;
            sComplexityAnalysisParam.pBackgroundMbFlag = (*pVaaInfo).pVaaBackgroundMbFlag;
            if !pRefPicture.is_null() {
                self.SetRefMbType(
                    pCtx,
                    &mut sComplexityAnalysisParam.uiRefMbType,
                    (*pRefPicture).iPictureType,
                );
            }
            sComplexityAnalysisParam.iCalcBgd = bCalculateBGD;
            sComplexityAnalysisParam.iFrameComplexity = 0;

            if !pWelsSvcRc.pGomForegroundBlockNum.is_null() {
                std::ptr::write_bytes(
                    pWelsSvcRc.pGomForegroundBlockNum as *mut u8,
                    0,
                    pWelsSvcRc.iGomSize as usize * std::mem::size_of::<i32>(),
                );
            }
            if iComplexityAnalysisMode != FRAME_SAD && !pWelsSvcRc.pCurrentFrameGomSad.is_null() {
                std::ptr::write_bytes(
                    pWelsSvcRc.pCurrentFrameGomSad as *mut u8,
                    0,
                    pWelsSvcRc.iGomSize as usize * std::mem::size_of::<i32>(),
                );
            }

            sComplexityAnalysisParam.pGomComplexity = pWelsSvcRc.pCurrentFrameGomSad;
            sComplexityAnalysisParam.pGomForegroundBlockNum = pWelsSvcRc.pGomForegroundBlockNum;
            sComplexityAnalysisParam.iMbNumInGom = pWelsSvcRc.iNumberMbGom;

            let iMethodIdx = EMethods::METHOD_COMPLEXITY_ANALYSIS as i32;
            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();

            sSrcPixMap.pPixel[0] = (*pCurPicture).pData[0] as *mut c_void;
            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = (*pCurPicture).iLineSize[0];
            sSrcPixMap.sRect.iRectWidth = (*pCurPicture).iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = (*pCurPicture).iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            if !pRefPicture.is_null() {
                sRefPixMap.pPixel[0] = (*pRefPicture).pData[0] as *mut c_void;
                sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
                sRefPixMap.iStride[0] = (*pRefPicture).iLineSize[0];
                sRefPixMap.sRect.iRectWidth = (*pRefPicture).iWidthInPixel;
                sRefPixMap.sRect.iRectHeight = (*pRefPicture).iHeightInPixel;
                sRefPixMap.eFormat = VideoFormat::videoFormatI420;
            }

            (*self.m_pInterfaceVp).Set(iMethodIdx, sComplexityAnalysisParam as *mut _ as *mut c_void);
            let iRet = (*self.m_pInterfaceVp).Process(iMethodIdx, &mut sSrcPixMap, &mut sRefPixMap);
            if iRet == 0 {
                (*self.m_pInterfaceVp).Get(iMethodIdx, sComplexityAnalysisParam as *mut _ as *mut c_void);
            }
        }
    }

    /// Look up the source picture and long-term index for a best-reference candidate.
    ///
    /// Matches `CWelsPreProcess::GetRefFrameInfo` (`wels_preprocess.cpp:1262`). The
    /// port previously declared this only as a vtable entry in `ref_list_mgr_svc.rs`
    /// with no body behind it.
    ///
    /// # Safety
    /// `m_pEncCtx`, its `pSvcParam`/`pVaa`, and the selected `m_pSpatialPic` entry
    /// must be valid, as in C++ where all three are dereferenced unconditionally.
    pub unsafe fn GetRefFrameInfo(
        &mut self,
        iRefIdx: i32,
        bCurrentFrameIsSceneLtr: bool,
        pRefOri: *mut *mut SPicture,
    ) -> i32 {
        let iTargetDid = (*(*self.m_pEncCtx).pSvcParam).iSpatialLayerNum - 1;
        let pVaaExt = (*self.m_pEncCtx).pVaa as *mut SVAAFrameInfoExt;
        let pBestRefCandidateParam = if bCurrentFrameIsSceneLtr {
            &(*pVaaExt).sVaaLtrBestRefCandidate[iRefIdx as usize]
        } else {
            &(*pVaaExt).sVaaStrBestRefCandidate[iRefIdx as usize]
        };
        let pPic =
            self.m_pSpatialPic[iTargetDid as usize][pBestRefCandidateParam.iSrcListIdx as usize];
        *pRefOri = pPic;
        (*pPic).iLongTermPicNum
    }

    pub unsafe fn UpdateBlockIdcForScreen(
        &self,
        pCurBlockStaticPointer: *mut u8,
        kpRefPic: *const SPicture,
        kpSrcPic: *const SPicture,
    ) -> i32 {
        if self.m_pInterfaceVp.is_null() || kpRefPic.is_null() || kpSrcPic.is_null() {
            return 1;
        }

        let iSceneChangeMethodIdx = EMethods::METHOD_SCENE_CHANGE_DETECTION_SCREEN as i32;
        let mut sSceneChangeResult = SSceneChangeResult::default();
        sSceneChangeResult.pStaticBlockIdc = pCurBlockStaticPointer;

        let mut sSrcMap = SPixMap::default();
        let mut sRefMap = SPixMap::default();
        self.InitPixMap(kpSrcPic, &mut sSrcMap);
        self.InitPixMap(kpRefPic, &mut sRefMap);

        (*self.m_pInterfaceVp).Set(iSceneChangeMethodIdx, &mut sSceneChangeResult as *mut _ as *mut c_void);
        let iRet = (*self.m_pInterfaceVp).Process(iSceneChangeMethodIdx, &mut sSrcMap, &mut sRefMap);
        if iRet == 0 {
            (*self.m_pInterfaceVp).Get(iSceneChangeMethodIdx, &mut sSceneChangeResult as *mut _ as *mut c_void);
        }
        iRet
    }

    pub unsafe fn UpdateSrcList(
        &mut self,
        pCurPicture: *mut SPicture,
        kiCurDid: i32,
        pShortRefList: *mut *mut SPicture,
        kuiShortRefCount: u32,
    ) {
        let pRefSrcList = &mut self.m_pSpatialPic[kiCurDid as usize][0] as *mut *mut SPicture;

        if !pCurPicture.is_null() && ((*pCurPicture).bUsedAsRef || (*pCurPicture).bIsLongRef) {
            if (*pCurPicture).iPictureType == P_SLICE && (*pCurPicture).uiTemporalId != 0 {
                let mut iRefIdx = kuiShortRefCount as i32 - 1;
                while iRefIdx >= 0 {
                    Self::WelsExchangeSpatialPictures(
                        pRefSrcList.offset((iRefIdx + 1) as isize),
                        pRefSrcList.offset(iRefIdx as isize),
                    );
                    iRefIdx -= 1;
                }
                self.m_iAvaliableRefInSpatialPicList = kuiShortRefCount as i32;
            } else {
                Self::WelsExchangeSpatialPictures(pRefSrcList, pRefSrcList.offset(1));
                let mut i = MAX_SHORT_REF_COUNT as i32 - 1;
                while i > 0 {
                    let pRef = *pRefSrcList.offset((i + 1) as isize);
                    if !pRef.is_null() {
                        (*pRef).SetUnref();
                    }
                    i -= 1;
                }
                self.m_iAvaliableRefInSpatialPicList = 1;
            }
        }
        let pOrig = self.GetCurrentOrigFrame(kiCurDid);
        if !pOrig.is_null() {
            (*pOrig).SetUnref();
        }
    }

    pub unsafe fn UpdateSrcListLosslessScreenRefSelectionWithLtr(
        &mut self,
        _pCurPicture: *mut SPicture,
        kiCurDid: i32,
        kuiMarkLongTermPicIdx: i32,
        pLongRefList: *mut *mut SPicture,
    ) {
        let pLongRefSrcList = &mut self.m_pSpatialPic[kiCurDid as usize][0] as *mut *mut SPicture;
        for i in 0..MAX_REF_PIC_COUNT {
            let pRef = *pLongRefSrcList.offset((i + 1) as isize);
            let pLong = *pLongRefList.offset(i as isize);
            if pRef.is_null() || (!pLong.is_null() && (*pLong).bUsedAsRef && (*pLong).bIsLongRef) {
                continue;
            } else {
                (*pRef).SetUnref();
            }
        }
        Self::WelsExchangeSpatialPictures(
            pLongRefSrcList,
            pLongRefSrcList.offset((1 + kuiMarkLongTermPicIdx) as isize),
        );
        self.m_iAvaliableRefInSpatialPicList = MAX_REF_PIC_COUNT as i32;
        let pOrig = self.GetCurrentOrigFrame(kiCurDid);
        if !pOrig.is_null() {
            (*pOrig).SetUnref();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vaa_enum_defaults() {
        assert_eq!(ESceneChangeIdc::default(), ESceneChangeIdc::SIMILAR_SCENE);
        assert_eq!(EStaticBlockIdc::default(), EStaticBlockIdc::NO_STATIC);
        assert_eq!(EMethods::default(), EMethods::METHOD_NULL);
    }

    #[test]
    fn test_wels_preprocess_init_and_uninit() {
        let _align = CMemoryAlign::new(16);
        let mut preprocess = CWelsPreProcess::default();
        unsafe {
            let mut param = SEncParamExt::default();
            param.iPicWidth = 128;
            param.iPicHeight = 128;
            param.fMaxFrameRate = 30.0;
            param.iSpatialLayerNum = 1;
            param.sSpatialLayers[0].iVideoWidth = 128;
            param.sSpatialLayers[0].iVideoHeight = 128;
            param.sSpatialLayers[0].fFrameRate = 30.0;
            assert_eq!(param.iPicWidth, 128);

            let ret = preprocess.WelsPreprocessCreate();
            assert_eq!(ret, ENC_RETURN_SUCCESS);

            preprocess.WelsPreprocessDestroy();
        }
    }

    #[test]
    fn test_downsample_buffer_geometry() {
        let scaled_pic = Scaled_Picture::default();
        assert_eq!(scaled_pic.iScaledWidth[0], 0);
        assert_eq!(scaled_pic.iScaledHeight[0], 0);
        assert!(scaled_pic.pScaledInputPicture.is_null());
    }

    #[test]
    fn test_ref_judgement_defaults() {
        let judgement = SRefJudgement::default();
        assert_eq!(judgement.iMinFrameComplexity, i32::MAX as i64);
        assert_eq!(judgement.iMinFrameNumGap, i32::MAX);
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static VP_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static VP_DUMP_SQD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
