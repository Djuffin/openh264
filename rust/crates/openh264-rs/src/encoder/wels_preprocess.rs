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

use crate::encoder::picture::{PicPlanes, RecPicId, RecPicPool, SrcPicId, SrcPicPool};
use std::ffi::{c_char, c_void};
use std::mem::size_of;
use crate::{
    EUsageType, SEncParamExt, SSourcePicture, SSpatialLayerConfig, VideoFormat,
};
use crate::encoder::encoder_ext::{PADDING_LENGTH, WELS_ALIGN};
use crate::encoder::param_svc::{MB_HEIGHT_LUMA, MB_WIDTH_LUMA};
use crate::encoder::encoder_context::SMVUnitXY;

// **The `tag!` macro stood here** and it has no use left: `CMemoryAlign` allocated
// six things for a picture and one for the scaled slot, and since T6.F2 it allocates
// none of them (T6.F3 took the VAA block the same way). The tags were diagnostic
// strings for an allocator this module no longer calls.

// ============================================================================
// Constants
// ============================================================================

pub const MAX_REF_PIC_COUNT: usize = 16;
// Single definition in `encoder_context.rs` from `wels_const.h`; this module's copy of
// MAX_SHORT_REF_COUNT was 16 where C++ derives 4, which over-sized `SRefList` here and
// let the `WelsPreprocess` unref loop read one past `pShortRefList`.
pub use crate::encoder::encoder_context::{MAX_GOP_SIZE, MAX_SHORT_REF_COUNT, MAX_TEMPORAL_LEVEL};
use crate::encoder::encoder_context::{ctx_ltr_at, ctx_rc_at};
use crate::encoder::rc::{rc_gom_fg_blocks, rc_gom_sad};
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

/// **Not `#[repr(C)]` and not `Copy` since T6.F1**: the scaled input picture is the
/// third and smallest of the encoder's three picture owners — one slot, owned in
/// place rather than pooled, because nothing else ever names it.
#[derive(Debug)]
pub struct Scaled_Picture {
    pub pScaledInputPicture: Option<Box<SPicture>>,
    pub iScaledWidth: [i32; MAX_DEPENDENCY_LAYER],
    pub iScaledHeight: [i32; MAX_DEPENDENCY_LAYER],
}

impl Default for Scaled_Picture {
    fn default() -> Self {
        Self {
            pScaledInputPicture: None,
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
    pub pRefPicture: Option<SrcPicId>,
    pub iSrcListIdx: i32,
    pub bSceneLtrFlag: bool,
    pub pBestBlockStaticIdc: *mut u8,
}

impl Default for SRefInfoParam {
    fn default() -> Self {
        Self {
            pRefPicture: None,
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
    /// `void*` in the C++ (`IWelsVP.h`); the three planes are bytes at every writer
    /// and reader, so the erasure is gone (Phase 6 session B). The cursor
    /// conversion is session F's.
    pub pPixel: [*mut u8; 3],
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

#[derive(Debug)]
pub struct SVAACalcResult {
    /// The two plane roots the walk reads. Still raw, and Phase 9's: they are
    /// `SPicture::data_ptr` cursors, handed over per frame by `VaaCalculation`.
    pub pCurY: *mut u8,
    pub pRefY: *mut u8,
    /// **The six per-frame result arrays, owned since T6.F3.** They were six
    /// `WelsMallocz` blocks `RequestMemorySvc` cut and `FreeMemorySvc` released one
    /// at a time, reachable only through this struct; each is one entry per
    /// macroblock, and the three that only the background-detection path fills are
    /// **empty** rather than null when it is off (`bEnableBackgroundDetection`).
    pub pSad8x8: Vec<[i32; 4]>,
    pub pSsd16x16: Vec<i32>,
    pub pSum16x16: Vec<i32>,
    pub pSumOfSquare16x16: Vec<i32>,
    pub pSumOfDiff8x8: Vec<[i32; 4]>,
    pub pMad8x8: Vec<[u8; 4]>,
    pub iFrameSad: i32,
}

impl Default for SVAACalcResult {
    fn default() -> Self {
        Self {
            pCurY: std::ptr::null_mut(),
            pRefY: std::ptr::null_mut(),
            pSad8x8: Vec::new(),
            pSsd16x16: Vec::new(),
            pSum16x16: Vec::new(),
            pSumOfSquare16x16: Vec::new(),
            pSumOfDiff8x8: Vec::new(),
            pMad8x8: Vec::new(),
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
    // `pCalcResult: *mut SVAACalcResult` was here (C++ `SVAACalcParam`): the caller
    // stored `&pVaaInfo->sVaaCalcInfo` in the block and the plugin read it during
    // `Process`. It is handed over at the `Process` call now — take what you reach
    // (Phase 6 session B). Same for the three sibling blocks below.
}

impl Default for SVAACalcParam {
    fn default() -> Self {
        Self {
            iCalcVar: false,
            iCalcBgd: false,
            iCalcSsd: false,
            iReserved: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBGDInterface {
    pub pBackgroundMbFlag: *mut i8,
}

impl Default for SBGDInterface {
    fn default() -> Self {
        Self {
            pBackgroundMbFlag: std::ptr::null_mut(),
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
    pub pMotionTextureUnit: *mut SMotionTextureUnit,
    pub pMotionTextureIndexToDeltaQp: *mut i8,
    pub iAverMotionTextureIndexToDeltaQp: i32,
}

impl Default for SAdaptiveQuantizationParam {
    fn default() -> Self {
        Self {
            iAdaptiveQuantMode: 0,
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

#[derive(Debug)]
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

    /// One byte per macroblock, **owned since T6.F3** — `RequestMemorySvc`'s
    /// seventh and last `WelsMallocz` for the VAA block.
    pub pVaaBackgroundMbFlag: Vec<i8>,
    pub uiValidLongTermPicIdx: u8,
    pub uiMarkLongTermPicIdx: u8,

    pub eSceneChangeIdc: ESceneChangeIdc,
    pub bSceneChangeFlag: bool,
    pub bIdrPeriodFlag: bool,
}

impl SVAAFrameInfo {
    /// `RequestMemorySvc`'s VAA block, `encoder_ext.cpp:1712-1760`, as a constructor.
    ///
    /// The C++ takes the struct from `WelsMallocz` and then cuts seven more blocks
    /// out of `CMemoryAlign` for its per-frame result arrays, each sized
    /// `iCountMaxMbNum`; here the struct is a `Box` and the seven are its own `Vec`s
    /// (S21 — a `Vec` field in a zeroed block is UB at its first drop, so the
    /// construction has to change before the ownership can).
    ///
    /// `bEnableBackgroundDetection` decides whether the last three exist at all, as
    /// it decides in the C++: `pSumOfDiff8x8` and `pMad8x8` are allocated only under
    /// it. **Empty is the port's spelling of that null.**
    ///
    /// **F56**: every other field's value is the zeroed block's, which is what
    /// `Default` already spells — and here `Default` *is* the zero image (no field of
    /// this struct is deliberately non-zero), so it is used rather than re-spelled.
    pub fn new(iCountMaxMbNum: i32, bEnableBackgroundDetection: bool) -> Box<SVAAFrameInfo> {
        let n = iCountMaxMbNum.max(0) as usize;
        let mut p = Box::new(SVAAFrameInfo::default());
        p.pVaaBackgroundMbFlag = vec![0i8; n];
        p.sVaaCalcInfo.pSad8x8 = vec![[0i32; 4]; n];
        p.sVaaCalcInfo.pSsd16x16 = vec![0i32; n];
        p.sVaaCalcInfo.pSum16x16 = vec![0i32; n];
        p.sVaaCalcInfo.pSumOfSquare16x16 = vec![0i32; n];
        if bEnableBackgroundDetection {
            p.sVaaCalcInfo.pSumOfDiff8x8 = vec![[0i32; 4]; n];
            p.sVaaCalcInfo.pMad8x8 = vec![[0u8; 4]; n];
        }
        p
    }
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
            pVaaBackgroundMbFlag: Vec::new(),
            uiValidLongTermPicIdx: 0,
            uiMarkLongTermPicIdx: 0,
            eSceneChangeIdc: ESceneChangeIdc::SIMILAR_SCENE,
            bSceneChangeFlag: false,
            bIdrPeriodFlag: false,
        }
    }
}

// SCREEN_CONTENT(dormant: Phase 10) — see `SVAAFrameInfoExt_t`.
#[repr(C)]
#[derive(Debug)]
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

// `IWelsVP` was here — the C++ video-processing vtable (`void* pCtx` plus seven
// `extern "C"` function pointers dispatching on an `EMethods` id, each casting a
// `void*` parameter back to the one struct its method takes). Phase 4b dissolved
// the port's other vtables; this was the last, and it carried `*mut c_void` at
// both ends of every call. `CWelsPreProcess::m_vp` owns the concrete
// `processing::SWelsVpContext` and each plugin's `Set`/`Get`/`Process` is typed
// (Phase 6 session B).

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
    pPic: Option<SrcPicId>,
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
/// **Safe since T6.F2**: the picture owns every byte it has, so there is no allocator
/// to be valid and no free contract to honour — dropping the `Box` releases it.
pub fn AllocPicture(
    kiWidth: i32,
    kiHeight: i32,
    bNeedMbInfo: bool,
    iNeedFeatureStorage: i32,
) -> Option<Box<SPicture>> {
    // `RequestScreenBlockFeatureStorage` is part of the screen-content path, which is
    // outside the gate configuration and unported; refuse rather than hand back a
    // picture whose storage the caller believes exists.
    if iNeedFeatureStorage != 0 {
        return None;
    }

    // **T6.F2**: what is left of `picture_handle.cpp:51`. The struct, its four
    // per-macroblock side arrays *and* its three planes are all `SPicture::new`'s
    // now — the geometry, the padding, the stride alignment and the zeroing moved
    // there with them, and this function is the refusal plus a constructor call.
    // `CMemoryAlign` has nothing left to allocate for a picture, so the parameter is
    // gone and `FreePicture` with it: a picture is released by dropping it.
    Some(SPicture::new(kiWidth, kiHeight, bNeedMbInfo))
}

/// Initializes scaled intermediate picture buffers if aspect-ratio scaling is required.
pub unsafe fn WelsInitScaledPic(
    pParam: *mut SWelsSvcCodingParam,
    pScaledPicture: *mut Scaled_Picture,
) -> i32 {
    let bInputPicNeedScaling = JudgeNeedOfScaling(pParam, pScaledPicture);
    if bInputPicNeedScaling {
        (*pScaledPicture).pScaledInputPicture = AllocPicture(
            (*pParam).SUsedPicRect.iWidth,
            (*pParam).SUsedPicRect.iHeight,
            false,
            0,
        );
        if (*pScaledPicture).pScaledInputPicture.is_none() {
            return -1;
        }

        let pPic = (*pScaledPicture)
            .pScaledInputPicture
            .as_deref_mut()
            .expect("just allocated")
            .planes();
        ClearEndOfLinePadding(
            pPic.pData[0],
            pPic.iLineSize[0],
            pPic.iWidthInPixel,
            pPic.iHeightInPixel,
        );
        ClearEndOfLinePadding(
            pPic.pData[1],
            pPic.iLineSize[1],
            pPic.iWidthInPixel >> 1,
            pPic.iHeightInPixel >> 1,
        );
        ClearEndOfLinePadding(
            pPic.pData[2],
            pPic.iLineSize[2],
            pPic.iWidthInPixel >> 1,
            pPic.iHeightInPixel >> 1,
        );
    }
    0
}

/// Releases the scaled picture. **Since T6.F2 that is a drop** — the picture owns
/// every byte it has, so `CMemoryAlign` is not involved and neither is a free walk.
pub unsafe fn FreeScaledPic(pScaledPicture: *mut Scaled_Picture) {
    if pScaledPicture.is_null() {
        return;
    }
    (*pScaledPicture).pScaledInputPicture = None;
}

// ============================================================================
// Core Preprocessing Engine: CWelsPreProcess
// ============================================================================

pub struct CWelsPreProcess {
    /// The video-processing plugins, owned. Was `m_pInterfaceVp: *mut IWelsVP`, a
    /// pointer to the dissolved vtable whose `pCtx` was this object behind a `void*`.
    pub m_vp: Box<crate::processing::SWelsVpContext>,
    pub m_pEncCtx: *mut sWelsEncCtx,
    pub m_uiSpatialLayersInTemporal: [u8; MAX_DEPENDENCY_LAYER],
    pub m_sScaledPicture: Scaled_Picture,
    pub m_pLastSpatialPicture: [[Option<SrcPicId>; 2]; MAX_DEPENDENCY_LAYER],
    pub m_bInitDone: bool,
    pub m_uiSpatialPicNum: [u8; MAX_DEPENDENCY_LAYER],
    /// **The spatial source pool** — every dependency layer's pictures in one owner,
    /// with [`m_pSpatialPic`](Self::m_pSpatialPic) the per-layer index into it. The
    /// C++ has one `SPicture*` array per layer and allocates into it directly; a
    /// handle has to name *one* pool, so the storage is flat and the shape
    /// `[did][i]` survives as the index.
    pub m_pSpatialPicPool: SrcPicPool,
    pub m_pSpatialPic: [[Option<SrcPicId>; MAX_REF_PIC_COUNT + 1]; MAX_DEPENDENCY_LAYER],
    pub m_iAvaliableRefInSpatialPicList: i32,
    pub m_eUsageType: EUsageType,
}

impl Default for CWelsPreProcess {
    /// Field-wise, because `m_vp` is a `Box` and an all-zero `Box` is not a value
    /// (S21). Every other field's zero is what the C++ constructor's zeroing meant:
    /// null pictures, no layers, not initialised.
    fn default() -> Self {
        Self {
            m_vp: Box::new(crate::processing::SWelsVpContext::default()),
            m_pEncCtx: std::ptr::null_mut(),
            m_uiSpatialLayersInTemporal: [0; MAX_DEPENDENCY_LAYER],
            m_sScaledPicture: Scaled_Picture::default(),
            m_pLastSpatialPicture: [[None; 2]; MAX_DEPENDENCY_LAYER],
            m_bInitDone: false,
            m_uiSpatialPicNum: [0; MAX_DEPENDENCY_LAYER],
            m_pSpatialPicPool: SrcPicPool::empty(),
            m_pSpatialPic: [[None; MAX_REF_PIC_COUNT + 1]; MAX_DEPENDENCY_LAYER],
            m_iAvaliableRefInSpatialPicList: 0,
            m_eUsageType: EUsageType::CAMERA_VIDEO_REAL_TIME,
        }
    }
}

/// Which **source-side** picture a preprocessing step reads or writes.
///
/// The spatial pool is one of the encoder's three picture owners; the scaled input is
/// another, a single slot with no pool because nothing else ever names it. Almost
/// every preprocessing step can be handed either — `SingleLayerPreprocess` moves the
/// caller's frame into whichever of the two is in play and downsamples out of it — so
/// the two are one parameter here rather than two overloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcPicRef {
    /// A slot of `CWelsPreProcess::m_pSpatialPicPool`.
    Pooled(SrcPicId),
    /// `Scaled_Picture::pScaledInputPicture`.
    Scaled,
}

impl CWelsPreProcess {
    /// The source-side picture `which` names.
    ///
    /// # Panics
    /// If `which` is [`SrcPicRef::Scaled`] and no scaled picture was allocated —
    /// which cannot happen: the only writer of `Scaled` is `SingleLayerPreprocess`,
    /// under the test that the slot is occupied.
    #[inline]
    pub fn src(&self, which: SrcPicRef) -> &SPicture {
        match which {
            SrcPicRef::Pooled(id) => self.m_pSpatialPicPool.get(id),
            SrcPicRef::Scaled => self
                .m_sScaledPicture
                .pScaledInputPicture
                .as_deref()
                .expect("the scaled input picture is allocated"),
        }
    }

    /// Mutable form of [`src`](Self::src).
    #[inline]
    pub fn src_mut(&mut self, which: SrcPicRef) -> &mut SPicture {
        match which {
            SrcPicRef::Pooled(id) => self.m_pSpatialPicPool.get_mut(id),
            SrcPicRef::Scaled => self
                .m_sScaledPicture
                .pScaledInputPicture
                .as_deref_mut()
                .expect("the scaled input picture is allocated"),
        }
    }

    /// Two *different* source-side pictures at once — the read-one-write-another
    /// shape every downsampling step has.
    ///
    /// # Panics
    /// If both name the same picture.
    pub fn src_pair_mut(
        &mut self,
        a: SrcPicRef,
        b: SrcPicRef,
    ) -> (&mut SPicture, &mut SPicture) {
        match (a, b) {
            (SrcPicRef::Pooled(x), SrcPicRef::Pooled(y)) => self.m_pSpatialPicPool.pair_mut(x, y),
            (SrcPicRef::Pooled(x), SrcPicRef::Scaled) => {
                let scaled = self
                    .m_sScaledPicture
                    .pScaledInputPicture
                    .as_deref_mut()
                    .expect("the scaled input picture is allocated");
                (self.m_pSpatialPicPool.get_mut(x), scaled)
            }
            (SrcPicRef::Scaled, SrcPicRef::Pooled(y)) => {
                let scaled = self
                    .m_sScaledPicture
                    .pScaledInputPicture
                    .as_deref_mut()
                    .expect("the scaled input picture is allocated");
                (scaled, self.m_pSpatialPicPool.get_mut(y))
            }
            (SrcPicRef::Scaled, SrcPicRef::Scaled) => {
                panic!("src_pair_mut on one picture (the scaled input)")
            }
        }
    }

    /// Factory constructor instantiating the preprocessing subsystem.
    pub unsafe fn CreatePreProcess(pEncCtx: *mut sWelsEncCtx) -> *mut CWelsPreProcess {
        if pEncCtx.is_null() || (*pEncCtx).pSvcParam.is_null() {
            return std::ptr::null_mut();
        }

        // Built whole and boxed (S21: the object owns a `Box` now, so a zeroed
        // shell is not a valid intermediate). This used to `alloc_zeroed` and set
        // three fields; `Default` is those zeros written out.
        let p = Box::new(CWelsPreProcess {
            m_pEncCtx: pEncCtx,
            m_eUsageType: (*(*pEncCtx).pSvcParam).iUsageType,
            ..Default::default()
        });
        Box::into_raw(p)
    }

    /// Destructor releasing allocated picture buffers and plugin interfaces.
    pub unsafe fn Destroy(pPreProcess: *mut CWelsPreProcess) {
        if !pPreProcess.is_null() {
            FreeScaledPic(&mut (*pPreProcess).m_sScaledPicture);
            // `WelsPreprocessDestroy` freed the vtable and its context here; the
            // plugins are `m_vp` and drop with the object.
            drop(Box::from_raw(pPreProcess));
        }
    }

    // `WelsPreprocessCreate` / `WelsPreprocessDestroy` (`wels_preprocess.cpp:198`)
    // were here: they allocated and freed the `IWelsVP` vtable and its `void*`
    // context. The plugins are owned by `m_vp` from construction, so there is
    // nothing left for either to do — deleted with their calls (S18, Phase 6
    // session B). Their history: the create used to `alloc_zeroed` the vtable and
    // stop, leaving every method `None` and the whole video-analysis stage
    // silently producing zeros — see `crate::processing`.

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

        FreeScaledPic(&mut self.m_sScaledPicture);
        self.InitLastSpatialPictures(pCtx);
        WelsInitScaledPic((*pCtx).pSvcParam, &mut self.m_sScaledPicture)
    }

    pub unsafe fn AllocSpatialPictures(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        pParam: *mut SWelsSvcCodingParam,
    ) -> i32 {
        let pMa = (*pCtx).pMemAlign;
        let kiDlayerCount = (*pParam).iSpatialLayerNum;
        let mut iDlayerIndex = 0;
        // The pool takes its slots in one piece and never grows, so the pictures are
        // collected first and the per-layer index is stamped from the finished pool.
        let mut pending: Vec<Box<SPicture>> = Vec::new();
        let mut slots = [[None::<usize>; MAX_REF_PIC_COUNT + 1]; MAX_DEPENDENCY_LAYER];

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
                let Some(pPic) = AllocPicture(kiPicWidth, kiPicHeight, false, 0) else {
                    return 1;
                };
                // The pool is flat across layers; `m_pSpatialPic[did][i]` keeps the
                // C++'s shape as the index into it.
                pending.push(pPic);
                slots[idx][i as usize] = Some(pending.len() - 1);
                i += 1;
            }

            if (*pParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                self.m_uiSpatialLayersInTemporal[idx] = 1;
            } else {
                self.m_uiSpatialLayersInTemporal[idx] = kuiLayerInTemporal;
            }

            iDlayerIndex += 1;
        }

        self.m_pSpatialPicPool = SrcPicPool::new(pending);
        for d in 0..MAX_DEPENDENCY_LAYER {
            for i in 0..=MAX_REF_PIC_COUNT {
                self.m_pSpatialPic[d][i] = slots[d][i].map(|k| self.m_pSpatialPicPool.at(k));
            }
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
                self.m_pSpatialPic[jIdx][i as usize] = None;
                i += 1;
            }
            self.m_uiSpatialLayersInTemporal[jIdx] = 0;
            j += 1;
        }
        self.m_pLastSpatialPicture = [[None; 2]; MAX_DEPENDENCY_LAYER];
        // T6.F2: the pool owns its pictures whole, so releasing them is dropping them.
        self.m_pSpatialPicPool = SrcPicPool::empty();
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
        let bScaling = self.m_sScaledPicture.pScaledInputPicture.is_some();
        let pSrcPic = if bScaling {
            SrcPicRef::Scaled
        } else {
            SrcPicRef::Pooled(
                self.GetCurrentOrigFrame(iDependencyId)
                    .expect("the spatial pool is allocated before any frame is preprocessed"),
            )
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
        if bScaling {
            pDstPic = SrcPicRef::Pooled(
                self.GetCurrentOrigFrame(iDependencyId)
                    .expect("the spatial pool is allocated before any frame is preprocessed"),
            );
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
                    self.DetectSceneChange(pDstPic, None)
                };
                (*(*pCtx).pVaa).eSceneChangeIdc = idc;
                (*(*pCtx).pVaa).bSceneChangeFlag = idc == ESceneChangeIdc::LARGE_CHANGED_SCENE;
            } else if !(*pDlayerParamInternal).bEncCurFrmAsIdrFlag
                && ((*pDlayerParamInternal).iCodingIndex & ((*pSvcParam).uiGopSize as i32 - 1)) == 0
            {
                let pRefPic = if (*ctx_ltr_at(pCtx, depIdx)).bReceivedT0LostFlag {
                    let pos = self.m_uiSpatialLayersInTemporal[depIdx] as usize
                        + (*(*pCtx).pVaa).uiValidLongTermPicIdx as usize;
                    self.m_pSpatialPic[depIdx][pos]
                } else {
                    self.m_pLastSpatialPicture[depIdx][0]
                };
                let pRefPic = pRefPic.map(SrcPicRef::Pooled);
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
            let idDst = match pDstPic {
                SrcPicRef::Pooled(id) => Some(id),
                // Unreachable in practice: `pDstPic` is only the scaled picture when
                // no scaling is configured, and then no scaled picture exists.
                SrcPicRef::Scaled => None,
            };
            WelsUpdateSpatialIdxMap(pCtx, iActualSpatialNum, idDst, iDependencyId);
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
                let pSrcPic = SrcPicRef::Pooled(
                    self.m_pLastSpatialPicture[iClosestDid as usize][1]
                        .expect("the closer layer was just written"),
                );
                let iTargetW = pLay.iVideoWidth;
                let iTargetH = pLay.iVideoHeight;
                let tId = pInt.uiCodingIdx2TemporalId[(pInt.iCodingIndex & gopMask) as usize];

                let iSrcW = self.m_sScaledPicture.iScaledWidth[iClosestDid as usize];
                let iSrcH = self.m_sScaledPicture.iScaledHeight[iClosestDid as usize];
                let pDstId = self
                    .GetCurrentOrigFrame(iDependencyId)
                    .expect("the spatial pool is allocated");
                let pDst = SrcPicRef::Pooled(pDstId);
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
                    WelsUpdateSpatialIdxMap(pCtx, iActualSpatialNum, Some(pDstId), iDependencyId);
                    iActualSpatialNum -= 1;
                }

                self.m_pLastSpatialPicture[curDepIdx][1] = Some(pDstId);
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
            && (*ctx_ltr_at(pCtx, (*pCtx).uiDependencyId as usize)).bReceivedT0LostFlag
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
                let bFlag = bCalculateBGD && self.ref_is_inter(pRefPic);
                self.BackgroundDetection((*pCtx).pVaa, pCurPic, pRefPic, bFlag);
            }
            if bNeededMbAq {
                self.AdaptiveQuantCalculation((*pCtx).pVaa, pCurPic, pRefPic);
            }
        } else {
            let pRefPic = self.GetBestRefPic(kiDidx, iRefTemporalIdx);
            let pLastPic = self.m_pLastSpatialPicture[dIdx][0];
            // **The one picture-identity test in `src/encoder`** (T6.F1). The C++ asks
            // it as `pLastPic->pData[0] == pRefPic->pData[0]` — two plane roots, which
            // is why session B's grep for `SPicture*` comparisons did not see it. Two
            // slots hold two distinct buffers, so equal roots is equal slots, and the
            // handle comparison is the same question asked directly. F42's arm, in the
            // encoder, is one line.
            let bCalculateSQDiff =
                pLastPic.is_some() && pLastPic == pRefPic && bNeededMbAq;

            self.VaaCalculation((*pCtx).pVaa, pCurPic, pRefPic, bCalculateSQDiff, bCalculateVar, bCalculateBGD);

            if (*pSvcParam).bEnableBackgroundDetection {
                let bFlag = bCalculateBGD && self.ref_is_inter(pRefPic);
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
            && !(*(*pCtx).pVaa).pVaaBackgroundMbFlag.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSad8x8.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSumOfDiff8x8.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pMad8x8.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSsd16x16.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSum16x16.is_empty()
            && !(*(*pCtx).pVaa).sVaaCalcInfo.pSumOfSquare16x16.is_empty()
        {
            let v = &*(*pCtx).pVaa;
            let sCurGeom = self
                .m_pSpatialPicPool
                .get_mut(pCurPic.expect("the spatial pool is allocated"))
                .planes();
            let iMbNum = ((sCurGeom.iWidthInPixel + 15) >> 4)
                * ((sCurGeom.iHeightInPixel + 15) >> 4);
            let (mut aq, mut bg, mut sad, mut ssd, mut sd, mut mad, mut sum, mut sqsum) =
                (0i64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
            for i in 0..iMbNum as isize {
                let w = i as i64 + 1;
                bg += w * v.pVaaBackgroundMbFlag[i as usize] as i64;
                let s8 = &v.sVaaCalcInfo.pSad8x8[i as usize];
                let d8 = &v.sVaaCalcInfo.pSumOfDiff8x8[i as usize];
                let m8 = &v.sVaaCalcInfo.pMad8x8[i as usize];
                for k in 0..4 {
                    sad += w * s8[k] as i64;
                    sd += w * d8[k] as i64;
                    mad += w * m8[k] as i64;
                }
                ssd += w * v.sVaaCalcInfo.pSsd16x16[i as usize] as i64;
                sum += w * v.sVaaCalcInfo.pSum16x16[i as usize] as i64;
                sqsum += w * v.sVaaCalcInfo.pSumOfSquare16x16[i as usize] as i64;
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

    /// The picture a spatial-pool handle names.
    ///
    /// # Panics
    /// If the pool has not been allocated. Every caller runs after
    /// `AllocSpatialPictures`.
    #[inline]
    pub fn src_id(&self, id: SrcPicId) -> &SPicture {
        self.m_pSpatialPicPool.get(id)
    }

    /// `pRef != NULL && pRef->iPictureType != I_SLICE` — the analysis stages' test
    /// for "there is a reference and it is not an intra frame", spelled once because
    /// three call sites ask it and each would otherwise borrow the pool inline while
    /// building an argument list for a `&mut self` method.
    #[inline]
    pub fn ref_is_inter(&self, id: Option<SrcPicId>) -> bool {
        match id {
            Some(id) => self.src_id(id).iPictureType != I_SLICE,
            None => false,
        }
    }

    pub unsafe fn GetCurPicPosition(&self, kiDidx: i32) -> i32 {
        self.m_uiSpatialLayersInTemporal[kiDidx as usize] as i32 - 1
    }

    pub unsafe fn GetCurrentOrigFrame(&mut self, iDIdx: i32) -> Option<SrcPicId> {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.m_pSpatialPic[iDIdx as usize][0]
        } else {
            let pos = self.GetCurPicPosition(iDIdx) as usize;
            self.m_pSpatialPic[iDIdx as usize][pos]
        }
    }

    pub unsafe fn GetBestRefPic(&self, kiDidx: i32, iRefTemporalIdx: i32) -> Option<SrcPicId> {
        self.m_pSpatialPic[kiDidx as usize][iRefTemporalIdx as usize]
    }

    pub unsafe fn GetBestRefPicScreen(
        &self,
        _iUsageType: EUsageType,
        bSceneLtr: bool,
        _eSliceType: EWelsSliceType,
        _kiDidx: i32,
        _iRefTemporalIdx: i32,
    ) -> Option<SrcPicId> {
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

    /// `CWelsPreProcess::BilateralDenoising`. **`METHOD_DENOISE` is not translated**
    /// (`crate::processing`): the C++ builds the source pixel map and runs the
    /// denoise plugin in place here; the port's dispatch returned
    /// `RET_NOTSUPPORTED`, which this caller never read, so nothing happened and
    /// nothing happens. Gated by `bEnableDenoise`, off in every gate configuration.
    pub unsafe fn BilateralDenoising(&mut self, _pSrc: SrcPicRef, _kiWidth: i32, _kiHeight: i32) {
        // METHOD_DENOISE: untranslated — no plugin runs (S18: no stub is invented).
    }

    pub unsafe fn DownsamplePadding(
        &mut self,
        srcRef: SrcPicRef,
        dstRef: SrcPicRef,
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

        // S37: resolve both pictures to their plane roots up front and work through
        // raw cursors from here — `srcRef` and `dstRef` are frequently *the same*
        // picture (no scaling configured), which no pair of references could express.
        let pSrc = self.src_mut(srcRef).planes();
        let pDstPic = self.src_mut(dstRef).planes();

        sSrcPixMap.pPixel[0] = pSrc.pData[0];
        sSrcPixMap.pPixel[1] = pSrc.pData[1];
        sSrcPixMap.pPixel[2] = pSrc.pData[2];
        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.sRect.iRectWidth = iSrcWidth;
        sSrcPixMap.sRect.iRectHeight = iSrcHeight;
        sSrcPixMap.iStride[0] = pSrc.iLineSize[0];
        sSrcPixMap.iStride[1] = pSrc.iLineSize[1];
        sSrcPixMap.iStride[2] = pSrc.iLineSize[2];
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight || bForceCopy {
            sDstPicMap.pPixel[0] = pDstPic.pData[0];
            sDstPicMap.pPixel[1] = pDstPic.pData[1];
            sDstPicMap.pPixel[2] = pDstPic.pData[2];
            sDstPicMap.iSizeInBits = g_kiPixMapSizeInBits;
            sDstPicMap.sRect.iRectWidth = iShrinkWidth;
            sDstPicMap.sRect.iRectHeight = iShrinkHeight;
            sDstPicMap.iStride[0] = pDstPic.iLineSize[0];
            sDstPicMap.iStride[1] = pDstPic.iLineSize[1];
            sDstPicMap.iStride[2] = pDstPic.iLineSize[2];
            sDstPicMap.eFormat = VideoFormat::videoFormatI420;

            if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight {
                // METHOD_DOWNSAMPLE: untranslated (`crate::processing`). The C++
                // runs the downsampler here; the port's dispatch returned
                // `RET_NOTSUPPORTED`, which is what `iRet` carries to the caller.
                // Reached only with more than one spatial layer or a resized
                // layer, off in every gate configuration (S18: no stub).
                iRet = crate::processing::vaacalc::RET_NOTSUPPORTED;
            } else {
                WelsMoveMemory_c(
                    pDstPic.pData[0],
                    pDstPic.pData[1],
                    pDstPic.pData[2],
                    pDstPic.iLineSize[0],
                    pDstPic.iLineSize[1],
                    pDstPic.iLineSize[2],
                    pSrc.pData[0],
                    pSrc.pData[1],
                    pSrc.pData[2],
                    pSrc.iLineSize[0],
                    pSrc.iLineSize[1],
                    pSrc.iLineSize[2],
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
            sDstPicMap.pPixel[0],
            sDstPicMap.pPixel[1],
            sDstPicMap.pPixel[2],
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
        &mut self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
        bCalculateSQDiff: bool,
        bCalculateVar: bool,
        bCalculateBGD: bool,
    ) {
        if pVaaInfo.is_null() {
            return;
        }
        // S37: both pictures resolved once to their plane roots; everything below
        // walks raw cursors, so no borrow of the pool outlives this prologue.
        let (Some(idCur), Some(idRef)) = (pCurPicture, pRefPicture) else {
            return;
        };
        let sCur = self.m_pSpatialPicPool.get_mut(idCur).planes();
        let sRef = self.m_pSpatialPicPool.get_mut(idRef).planes();
        (*pVaaInfo).sVaaCalcInfo.pCurY = sCur.pData[0];
        (*pVaaInfo).sVaaCalcInfo.pRefY = sRef.pData[0];

        let mut sCurPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();
        let mut calc_param = SVAACalcParam::default();

        sCurPixMap.pPixel[0] = sCur.pData[0];
        sCurPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sCurPixMap.sRect.iRectWidth = sCur.iWidthInPixel;
        sCurPixMap.sRect.iRectHeight = sCur.iHeightInPixel;
        sCurPixMap.iStride[0] = sCur.iLineSize[0];
        sCurPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.pPixel[0] = sRef.pData[0];
        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.sRect.iRectWidth = sRef.iWidthInPixel;
        sRefPixMap.sRect.iRectHeight = sRef.iHeightInPixel;
        sRefPixMap.iStride[0] = sRef.iLineSize[0];
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        calc_param.iCalcVar = bCalculateVar;
        calc_param.iCalcBgd = bCalculateBGD;
        calc_param.iCalcSsd = bCalculateSQDiff;

        // METHOD_VAA_STATISTICS. The result is handed over at the call; the C++
        // stored `&pVaaInfo->sVaaCalcInfo` in the parameter block.
        self.m_vp.sVaaCalc.Set(&calc_param);
        self.m_vp.sVaaCalc.Process(&sCurPixMap, &sRefPixMap, &mut (*pVaaInfo).sVaaCalcInfo);
    }

    pub unsafe fn BackgroundDetection(
        &mut self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
        bDetectFlag: bool,
    ) {
        if pVaaInfo.is_null() {
            return;
        }
        let Some(idCur) = pCurPicture else {
            return;
        };
        // S37 again — resolved once, then raw cursors.
        let sCur = self.m_pSpatialPicPool.get_mut(idCur).planes();
        let sRef = pRefPicture.map(|id| self.m_pSpatialPicPool.get_mut(id).planes());
        if let (true, Some(sRef)) = (bDetectFlag, sRef) {
            (*pVaaInfo).iPicWidth = sCur.iWidthInPixel;
            (*pVaaInfo).iPicHeight = sCur.iHeightInPixel;
            (*pVaaInfo).iPicStride = sCur.iLineSize[0];
            (*pVaaInfo).iPicStrideUV = sCur.iLineSize[1];
            (*pVaaInfo).pCurY = sCur.pData[0];
            (*pVaaInfo).pRefY = sRef.pData[0];
            (*pVaaInfo).pCurU = sCur.pData[1];
            (*pVaaInfo).pRefU = sRef.pData[1];
            (*pVaaInfo).pCurV = sCur.pData[2];
            (*pVaaInfo).pRefV = sRef.pData[2];

            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();
            let mut BGDParam = SBGDInterface::default();

            sSrcPixMap.pPixel[0] = sCur.pData[0];
            sSrcPixMap.pPixel[1] = sCur.pData[1];
            sSrcPixMap.pPixel[2] = sCur.pData[2];
            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = sCur.iLineSize[0];
            sSrcPixMap.iStride[1] = sCur.iLineSize[1];
            sSrcPixMap.iStride[2] = sCur.iLineSize[2];
            sSrcPixMap.sRect.iRectWidth = sCur.iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = sCur.iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            sRefPixMap.pPixel[0] = sRef.pData[0];
            sRefPixMap.pPixel[1] = sRef.pData[1];
            sRefPixMap.pPixel[2] = sRef.pData[2];
            sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sRefPixMap.iStride[0] = sRef.iLineSize[0];
            sRefPixMap.iStride[1] = sRef.iLineSize[1];
            sRefPixMap.iStride[2] = sRef.iLineSize[2];
            sRefPixMap.sRect.iRectWidth = sRef.iWidthInPixel;
            sRefPixMap.sRect.iRectHeight = sRef.iHeightInPixel;
            sRefPixMap.eFormat = VideoFormat::videoFormatI420;

            BGDParam.pBackgroundMbFlag = (*pVaaInfo).pVaaBackgroundMbFlag.as_mut_ptr();

            // METHOD_BACKGROUND_DETECTION; the VAA result handed over at the call.
            self.m_vp.sBackgroundDetection.Set(&BGDParam);
            self.m_vp.sBackgroundDetection.Process(&sSrcPixMap, &sRefPixMap, &(*pVaaInfo).sVaaCalcInfo);
        } else {
            let iPicWidthInMb = (sCur.iWidthInPixel + 15) >> 4;
            let iPicHeightInMb = (sCur.iHeightInPixel + 15) >> 4;
            let n = ((iPicWidthInMb * iPicHeightInMb).max(0) as usize)
                .min((*pVaaInfo).pVaaBackgroundMbFlag.len());
            (&mut (*pVaaInfo).pVaaBackgroundMbFlag)[..n].fill(0);
        }
    }

    pub unsafe fn AdaptiveQuantCalculation(
        &mut self,
        pVaaInfo: *mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
    ) {
        if pVaaInfo.is_null() {
            return;
        }
        // S37: both pictures resolved once to their plane roots; everything below
        // walks raw cursors, so no borrow of the pool outlives this prologue.
        let (Some(idCur), Some(idRef)) = (pCurPicture, pRefPicture) else {
            return;
        };
        let sCur = self.m_pSpatialPicPool.get_mut(idCur).planes();
        let sRef = self.m_pSpatialPicPool.get_mut(idRef).planes();
        // The C++ stored `&pVaaInfo->sVaaCalcInfo` *inside* `pVaaInfo` here
        // (`sAdaptiveQuantParam.pCalcResult`) — a self-pointer; the result is
        // handed over at the `Process` call instead.
        (*pVaaInfo).sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp = 0;

        let mut pSrc = SPixMap::default();
        let mut pRef = SPixMap::default();

        pSrc.pPixel[0] = sCur.pData[0];
        pSrc.iSizeInBits = g_kiPixMapSizeInBits;
        pSrc.iStride[0] = sCur.iLineSize[0];
        pSrc.sRect.iRectWidth = sCur.iWidthInPixel;
        pSrc.sRect.iRectHeight = sCur.iHeightInPixel;
        pSrc.eFormat = VideoFormat::videoFormatI420;

        pRef.pPixel[0] = sRef.pData[0];
        pRef.iSizeInBits = g_kiPixMapSizeInBits;
        pRef.iStride[0] = sRef.iLineSize[0];
        pRef.sRect.iRectWidth = sRef.iWidthInPixel;
        pRef.sRect.iRectHeight = sRef.iHeightInPixel;
        pRef.eFormat = VideoFormat::videoFormatI420;

        // METHOD_ADAPTIVE_QUANT.
        self.m_vp.sAdaptiveQuant.Set(&(*pVaaInfo).sAdaptiveQuantParam);
        let iRet = self.m_vp.sAdaptiveQuant.Process(&pSrc, &pRef, &(*pVaaInfo).sVaaCalcInfo);
        if iRet == 0 {
            self.m_vp.sAdaptiveQuant.Get(&mut (*pVaaInfo).sAdaptiveQuantParam);
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
        ppPic1: *mut Option<SrcPicId>,
        ppPic2: *mut Option<SrcPicId>,
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
                self.m_pLastSpatialPicture[iDlayerIndex][0] = None;
                self.m_pLastSpatialPicture[iDlayerIndex][1] = None;
                iDlayerIndex += 1;
            }
        } else {
            while iDlayerIndex < kiDlayerCount as usize {
                let kiLayerInTemporal = self.m_uiSpatialLayersInTemporal[iDlayerIndex as usize] as usize;
                self.m_pLastSpatialPicture[iDlayerIndex as usize][0] =
                    self.m_pSpatialPic[iDlayerIndex as usize][kiLayerInTemporal.saturating_sub(2)];
                self.m_pLastSpatialPicture[iDlayerIndex as usize][1] = None;
                iDlayerIndex += 1;
            }
            while (iDlayerIndex as usize) < MAX_DEPENDENCY_LAYER {
                self.m_pLastSpatialPicture[iDlayerIndex as usize][0] = None;
                self.m_pLastSpatialPicture[iDlayerIndex as usize][1] = None;
                iDlayerIndex += 1;
            }
        }

        0
    }

    pub unsafe fn WelsMoveMemoryWrapper(
        &mut self,
        pSvcParam: *mut SWelsSvcCodingParam,
        pDstRef: SrcPicRef,
        kpSrc: *const SSourcePicture,
        kiTargetWidth: i32,
        kiTargetHeight: i32,
    ) -> i32 {
        if (VideoFormat::videoFormatI420 as i32) != ((*kpSrc).iColorFormat & !(-0x80000000i32)) {
            return ENC_RETURN_INVALIDINPUT;
        }

        // S37: the destination resolved once to its plane roots.
        let pDstPic = self.src_mut(pDstRef).planes();

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

        let pDstY = pDstPic.pData[0];
        let pDstU = pDstPic.pData[1];
        let pDstV = pDstPic.pData[2];
        let kiDstStrideY = pDstPic.iLineSize[0];
        let kiDstStrideU = pDstPic.iLineSize[1];
        let kiDstStrideV = pDstPic.iLineSize[2];

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
        pCurPicture: SrcPicRef,
        pRefPicture: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.DetectSceneChangeScreen(pCurPicture, pRefPicture)
        } else {
            self.DetectSceneChangeVideo(pCurPicture, pRefPicture)
        }
    }

    unsafe fn DetectSceneChangeVideo(
        &mut self,
        pCurPicture: SrcPicRef,
        pRefPicture: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        let Some(pRefPicture) = pRefPicture else {
            return ESceneChangeIdc::SIMILAR_SCENE;
        };
        // S37: resolved once, raw cursors after.
        let sCur = self.src_mut(pCurPicture).planes();
        let sRef = self.src_mut(pRefPicture).planes();

        // METHOD_SCENE_CHANGE_DETECTION_VIDEO: no `Set` in the C++ either.
        let mut sSceneChangeDetectResult = SSceneChangeResult::default();
        let mut sSrcPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();

        sSrcPixMap.pPixel[0] = sCur.pData[0];
        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.iStride[0] = sCur.iLineSize[0];
        sSrcPixMap.sRect.iRectWidth = sCur.iWidthInPixel;
        sSrcPixMap.sRect.iRectHeight = sCur.iHeightInPixel;
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.pPixel[0] = sRef.pData[0];
        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.iStride[0] = sRef.iLineSize[0];
        sRefPixMap.sRect.iRectWidth = sRef.iWidthInPixel;
        sRefPixMap.sRect.iRectHeight = sRef.iHeightInPixel;
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        let iRet = self.m_vp.sSceneChangeDetection.Process(&sSrcPixMap, &sRefPixMap);
        if iRet == 0 {
            self.m_vp.sSceneChangeDetection.Get(&mut sSceneChangeDetectResult);
        }
        sSceneChangeDetectResult.eSceneChangeIdc
    }

    unsafe fn DetectSceneChangeScreen(
        &mut self,
        pCurPicture: SrcPicRef,
        _pRef: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        let pCtx = self.m_pEncCtx;
        if pCtx.is_null() || (*pCtx).pVaa.is_null() {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        let pSvcParam = (*pCtx).pSvcParam;
        let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
        let iTargetDid = (*pSvcParam).iSpatialLayerNum - 1;
        if iTargetDid != 0 {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        // The layer's spatial list from index 1 — the C++ passes `&m_pSpatialPic[d][1]`.
        let pRefPicList: [Option<SrcPicId>; MAX_REF_PIC_COUNT] = std::array::from_fn(|i| {
            self.m_pSpatialPic[iTargetDid as usize][i + 1]
        });
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
            (*ctx_ltr_at(pCtx, iTargetDid as usize)).iLastLtrIdx[iCurTid as usize];
        if (*pSvcParam).bEnableLongTermReference {
            self.GetAvailableRefListLosslessScreenRefSelection(
                &pRefPicList,
                iCurTid,
                iClosestLtrFrameNum,
                sAvailableRefParam.as_mut_ptr(),
                &mut iAvailableRefNum,
                &mut iAvailableSceneRefNum,
            );
        } else {
            self.GetAvailableRefList(
                &pRefPicList,
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

        let sCur = self.src_mut(pCurPicture).planes();
        Self::InitPixMap(&sCur, &mut sSrcMap);
        self.InitRefJudgement(&mut sLtrJudgement);
        self.InitRefJudgement(&mut sSceneLtrJudgement);

        let iNegligibleMotionBlocks = ((sCur.iWidthInPixel >> 3)
            * (sCur.iHeightInPixel >> 3)) as f32
            * STATIC_SCENE_MOTION_RATIO;
        let iNegligibleBlocks = iNegligibleMotionBlocks as i32;

        // `iSceneChangeMethodIdx = METHOD_SCENE_CHANGE_DETECTION_SCREEN` was here;
        // the method is untranslated and its sites below say so.

        for iScdIdx in 0..iAvailableRefNum {
            let pCurBlockStaticPointer = (*pVaaExt).pVaaBlockStaticIdc[iScdIdx as usize];
            let mut sSceneChangeResult = SSceneChangeResult::default();
            sSceneChangeResult.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;
            sSceneChangeResult.pStaticBlockIdc = pCurBlockStaticPointer;

            let pRefPicInfo = &mut sAvailableRefParam[iScdIdx as usize];
            let Some(idRefPic) = pRefPicInfo.pRefPicture else {
                continue;
            };
            let sRefGeom = self.m_pSpatialPicPool.get_mut(idRefPic).planes();
            Self::InitPixMap(&sRefGeom, &mut sRefMap);

            let bIsClosestLtrFrame =
                self.src_id(idRefPic).iLongTermPicNum == iClosestLtrFrameNum;
            if iScdIdx == 0 {
                let pScrollDetectInfo = &mut (*pVaaExt).sScrollDetectInfo;
                *pScrollDetectInfo = SScrollDetectionParam::default();

                // METHOD_SCROLL_DETECTION: untranslated (`crate::processing`). The
                // C++ runs the scroll detector here and clamps its vector; the port's
                // dispatch returned `RET_NOTSUPPORTED` and this block was skipped, so
                // `sScrollDetectInfo` stays at its default. Screen content only, off
                // in every gate configuration (S18: no stub).
                {
                    let ret = crate::processing::vaacalc::RET_NOTSUPPORTED;
                    if ret == 0 {
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

            // METHOD_SCENE_CHANGE_DETECTION_SCREEN: untranslated (`crate::processing`);
            // the port's dispatch returned `RET_NOTSUPPORTED` and the block below was
            // skipped. Screen content only, off in every gate configuration.
            {
                let ret = crate::processing::vaacalc::RET_NOTSUPPORTED;
                if ret == 0 {

                    let iFrameComplexity = sSceneChangeResult.iFrameComplexity;
                    let iSceneDetectIdc = sSceneChangeResult.eSceneChangeIdc;
                    let iMotionBlockNum = sSceneChangeResult.iMotionBlockNum;

                    let bCurRefIsSceneLtr = self.src_id(idRefPic).bIsSceneLTR;
                    let iRefPicAvQP = self.src_id(idRefPic).iFrameAverageQp;

                    if iSceneDetectIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE {
                        iNumOfLargeChange += 1;
                    }
                    if bCurRefIsSceneLtr && iSceneDetectIdc != ESceneChangeIdc::SIMILAR_SCENE {
                        iNumOfMediumChangeToLtr += 1;
                    }

                    if self.JudgeBestRef(idRefPic, &sLtrJudgement, iFrameComplexity, bIsClosestLtrFrame) {
                        self.SaveBestRefToJudgement(iRefPicAvQP, iFrameComplexity, &mut sLtrJudgement);
                        self.SaveBestRefToLocal(pRefPicInfo, &sSceneChangeResult, &mut sLtrSaved);
                    }
                    if bCurRefIsSceneLtr
                        && self.JudgeBestRef(idRefPic, &sSceneLtrJudgement, iFrameComplexity, bIsClosestLtrFrame)
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
        if let Some(id) = sLtrSaved.pRefPicture {
            (*pVaaExt).iVaaBestRefFrameNum = self.src_id(id).iFrameNum;
        }
        (*pVaaExt).pVaaBestBlockStaticIdc = sLtrSaved.pBestBlockStaticIdc;

        if iAvailableSceneRefNum > 0 {
            self.SaveBestRefToVaa(&sSceneLtrSaved, &mut (*pVaaExt).sVaaLtrBestRefCandidate[0]);
        }

        (*pVaaExt).iNumOfAvailableRef = 1;
        iVaaFrameSceneChangeIdc
    }

    unsafe fn InitPixMap(pPicture: &PicPlanes, pPixMap: *mut SPixMap) {
        if !pPixMap.is_null() {
            (*pPixMap).pPixel[0] = pPicture.pData[0];
            (*pPixMap).pPixel[1] = pPicture.pData[1];
            (*pPixMap).pPixel[2] = pPicture.pData[2];
            (*pPixMap).iSizeInBits = std::mem::size_of::<u8>() as i32;
            (*pPixMap).iStride[0] = pPicture.iLineSize[0];
            (*pPixMap).iStride[1] = pPicture.iLineSize[1];
            (*pPixMap).sRect.iRectWidth = pPicture.iWidthInPixel;
            (*pPixMap).sRect.iRectHeight = pPicture.iHeightInPixel;
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
        idRefPic: SrcPicId,
        sRefJudgement: &SRefJudgement,
        iFrameComplexity: i64,
        bIsClosestLtrFrame: bool,
    ) -> bool {
        if bIsClosestLtrFrame {
            iFrameComplexity < sRefJudgement.iMinFrameComplexity11
        } else {
            (iFrameComplexity < sRefJudgement.iMinFrameComplexity08)
                || ((iFrameComplexity <= sRefJudgement.iMinFrameComplexity11)
                    && (self.src_id(idRefPic).iFrameAverageQp < sRefJudgement.iMinFrameQp))
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
        pRefPicList: &[Option<SrcPicId>],
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
            let Some(idRefPic) = pRefPicList[i as usize] else {
                i -= 1;
                continue;
            };
            let pRefPic = self.src_id(idRefPic);
            if !pRefPic.bUsedAsRef
                || !pRefPic.bIsLongRef
                || (bCurFrameMarkedAsSceneLtr && !pRefPic.bIsSceneLTR)
            {
                i -= 1;
                continue;
            }

            let uiRefTid = pRefPic.uiTemporalId;
            let bRefRealLtr = pRefPic.bIsSceneLTR;

            if bRefRealLtr || (iCurTid == 0 && uiRefTid == 0) || (uiRefTid < iCurTid) {
                let idx = if pRefPic.iLongTermPicNum == iClosestLtrFrameNum {
                    0
                } else {
                    let old = *pAvailableRefNum;
                    *pAvailableRefNum += 1;
                    old
                };
                let param = &mut *pAvailableRefParam.offset(idx as isize);
                param.pRefPicture = Some(idRefPic);
                param.iSrcListIdx = i + 1;
                if bRefRealLtr {
                    *pAvailableSceneRefNum += 1;
                }
            }

            i -= 1;
        }

        if (*pAvailableRefParam.offset(0)).pRefPicture.is_none() {
            let mut j = 1;
            while j < *pAvailableRefNum {
                let pPrev = &mut *pAvailableRefParam.offset((j - 1) as isize);
                let pCur = &*pAvailableRefParam.offset(j as isize);
                pPrev.pRefPicture = pCur.pRefPicture;
                pPrev.iSrcListIdx = pCur.iSrcListIdx;
                j += 1;
            }
            let last = &mut *pAvailableRefParam.offset((*pAvailableRefNum - 1) as isize);
            last.pRefPicture = None;
            last.iSrcListIdx = 0;
            *pAvailableRefNum -= 1;
        }
    }

    unsafe fn GetAvailableRefList(
        &self,
        pSrcPicList: &[Option<SrcPicId>],
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
            let Some(idRefPic) = pSrcPicList[i as usize] else {
                i -= 1;
                continue;
            };
            if !self.src_id(idRefPic).bUsedAsRef {
                i -= 1;
                continue;
            }

            let uiRefTid = self.src_id(idRefPic).uiTemporalId;
            if uiRefTid <= iCurTid {
                let param = &mut *pAvailableRefList.offset(*pAvailableRefNum as isize);
                param.pRefPicture = Some(idRefPic);
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
        let pLtr = ctx_ltr_at(pCtx, uiDid as usize);
        if pRefPicLlist.is_null() {
            return;
        }

        if (*(*pCtx).pSvcParam).bEnableLongTermReference && (*pLtr).bReceivedT0LostFlag && uiTid == 0 {
            for i in 0..(*pRefPicLlist).uiLongRefCount as usize {
                let Some(id) = (*pRefPicLlist).pLongRefList[i] else {
                    continue;
                };
                if (*pRefPicLlist).pic(id).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    *pRefMbTypeArray = (*pRefPicLlist).pic_mut(id).ref_mb_type_root();
                    break;
                }
            }
        } else {
            for i in 0..(*pRefPicLlist).uiShortRefCount as usize {
                let Some(id) = (*pRefPicLlist).pShortRefList[i] else {
                    continue;
                };
                let pRef = (*pRefPicLlist).pic(id);
                if pRef.bUsedAsRef && pRef.iFramePoc >= 0 && pRef.uiTemporalId <= uiTid {
                    *pRefMbTypeArray = (*pRefPicLlist).pic_mut(id).ref_mb_type_root();
                    break;
                }
            }
        }
    }

    pub unsafe fn AnalyzePictureComplexity(
        &mut self,
        pCtx: *mut sWelsEncCtx,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<RecPicId>,
        kiDependencyId: i32,
        bCalculateBGD: bool,
    ) {
        if pCtx.is_null() || (*pCtx).pSvcParam.is_null() {
            return;
        }
        let Some(idCur) = pCurPicture else {
            return;
        };
        // **The flip caught a second two-pool crossing here.** The current picture is
        // a spatial *source* picture (`pCtx->pEncPic`) and the reference is a
        // *reconstruction* picture (`pCtx->pRefList0[0]`, `encoder_ext.cpp:2662`) —
        // both `SPicture*` in C++, so nothing there says the two arguments come from
        // different owners. S37: each resolved once in its own pool, to geometry.
        let pRefList = *(*pCtx).ppRefPicListExt.add((*pCtx).uiDependencyId as usize);
        let sCur = self.m_pSpatialPicPool.get_mut(idCur).planes();
        let sRefPic = pRefPicture.filter(|_| !pRefList.is_null());
        let sRef = sRefPic
            .map(|id| (*pRefList).pic_mut(id).planes())
            .unwrap_or(sCur);

        let pSvcParam = (*pCtx).pSvcParam;
        if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            let pVaaExt = (*pCtx).pVaa as *mut SVAAFrameInfoExt;
            let sComplexityAnalysisParam = &mut (*pVaaExt).sComplexityScreenParam;
            let pWelsSvcRc = ctx_rc_at(pCtx, kiDependencyId as usize);

            let _iComplexityAnalysisMode = if (*pCtx).eSliceType == EWelsSliceType::P_SLICE {
                GOM_SAD
            } else if (*pCtx).eSliceType == EWelsSliceType::I_SLICE {
                GOM_VAR
            } else {
                return;
            };

            (*pWelsSvcRc).pGomForegroundBlockNum.fill(0);
            (*pWelsSvcRc).pCurrentFrameGomSad.fill(0);

            sComplexityAnalysisParam.iFrameComplexity = 0;
            sComplexityAnalysisParam.pGomComplexity = rc_gom_sad(pWelsSvcRc);
            sComplexityAnalysisParam.iGomNumInFrame = (*pWelsSvcRc).iGomSize;
            sComplexityAnalysisParam.iIdrFlag = if (*pCtx).eSliceType == EWelsSliceType::I_SLICE { 1 } else { 0 };
            sComplexityAnalysisParam.iMbRowInGom = GOM_H_SCC;
            sComplexityAnalysisParam.sScrollResult.bScrollDetectFlag = false;
            sComplexityAnalysisParam.sScrollResult.iScrollMvX = 0;
            sComplexityAnalysisParam.sScrollResult.iScrollMvY = 0;

            // METHOD_COMPLEXITY_ANALYSIS_SCREEN: untranslated (`crate::processing`).
            // The C++ builds the two pixel maps, hands the block above to the screen
            // complexity plugin, runs it, and reads `iFrameComplexity` back on
            // success; the port's dispatch returned `RET_NOTSUPPORTED` and the
            // read-back was skipped, so the block keeps the values written into it
            // above. Screen content only, off in every gate configuration (S18: no
            // stub is invented, and the dead pixel maps are not built).
            let iRet = crate::processing::vaacalc::RET_NOTSUPPORTED;
            debug_assert_ne!(iRet, 0);
        } else {
            let pVaaInfo = (*pCtx).pVaa;
            let sComplexityAnalysisParam = &mut (*pVaaInfo).sComplexityAnalysisParam;
            let pWelsSvcRc = ctx_rc_at(pCtx, kiDependencyId as usize);

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
            sComplexityAnalysisParam.pBackgroundMbFlag = (*pVaaInfo).pVaaBackgroundMbFlag.as_mut_ptr();
            if sRefPic.is_some() {
                self.SetRefMbType(
                    pCtx,
                    &mut sComplexityAnalysisParam.uiRefMbType,
                    (*pRefList)
                        .pic(sRefPic.expect("checked just above"))
                        .iPictureType,
                );
            }
            sComplexityAnalysisParam.iCalcBgd = bCalculateBGD;
            sComplexityAnalysisParam.iFrameComplexity = 0;

            (*pWelsSvcRc).pGomForegroundBlockNum.fill(0);
            if iComplexityAnalysisMode != FRAME_SAD {
                (*pWelsSvcRc).pCurrentFrameGomSad.fill(0);
            }

            sComplexityAnalysisParam.pGomComplexity = rc_gom_sad(pWelsSvcRc);
            sComplexityAnalysisParam.pGomForegroundBlockNum = rc_gom_fg_blocks(pWelsSvcRc);
            sComplexityAnalysisParam.iMbNumInGom = (*pWelsSvcRc).iNumberMbGom;

            // METHOD_COMPLEXITY_ANALYSIS; the VAA result handed over at the call.
            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();

            sSrcPixMap.pPixel[0] = sCur.pData[0];
            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = sCur.iLineSize[0];
            sSrcPixMap.sRect.iRectWidth = sCur.iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = sCur.iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            if sRefPic.is_some() {
                sRefPixMap.pPixel[0] = sRef.pData[0];
                sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
                sRefPixMap.iStride[0] = sRef.iLineSize[0];
                sRefPixMap.sRect.iRectWidth = sRef.iWidthInPixel;
                sRefPixMap.sRect.iRectHeight = sRef.iHeightInPixel;
                sRefPixMap.eFormat = VideoFormat::videoFormatI420;
            }

            self.m_vp.sComplexityAnalysis.Set(sComplexityAnalysisParam);
            let iRet =
                self.m_vp.sComplexityAnalysis.Process(&sSrcPixMap, &sRefPixMap, &(*pVaaInfo).sVaaCalcInfo);
            if iRet == 0 {
                self.m_vp.sComplexityAnalysis.Get(sComplexityAnalysisParam);
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
        pRefOri: &mut Option<SrcPicId>,
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
        self.src_id(pPic.expect("the best-reference candidate names a live slot"))
            .iLongTermPicNum
    }

    pub unsafe fn UpdateBlockIdcForScreen(
        &self,
        pCurBlockStaticPointer: *mut u8,
        kpRefPic: Option<&PicPlanes>,
        kpSrcPic: Option<&PicPlanes>,
    ) -> i32 {
        if kpRefPic.is_none() || kpSrcPic.is_none() {
            return 1;
        }

        // METHOD_SCENE_CHANGE_DETECTION_SCREEN: untranslated (`crate::processing`).
        // The C++ hands `pCurBlockStaticPointer` to the screen scene-change plugin
        // through an `SSceneChangeResult`, runs it over the two pixel maps and reads
        // the result back on success; the port's dispatch returned `RET_NOTSUPPORTED`,
        // which is what the caller gets, and the read-back was skipped. Screen
        // content only, off in every gate configuration (S18: no stub is invented,
        // and the dead pixel maps are not built).
        let _ = pCurBlockStaticPointer;
        crate::processing::vaacalc::RET_NOTSUPPORTED
    }

    /// **`pShortRefList` stood in this signature and nothing read it** (S18): the
    /// C++ passes the reference list and uses only its *count*. Deleted with the
    /// flip rather than converted, because converting it would have meant handing
    /// the preprocessor a handle type it has no pool for.
    pub unsafe fn UpdateSrcList(
        &mut self,
        pCurPicture: Option<SrcPicId>,
        kiCurDid: i32,
        kuiShortRefCount: u32,
    ) {
        let pRefSrcList = self.m_pSpatialPic[kiCurDid as usize].as_mut_ptr();

        let bCur = match pCurPicture {
            Some(id) => {
                let p = self.src_id(id);
                (p.bUsedAsRef || p.bIsLongRef, p.iPictureType, p.uiTemporalId)
            }
            None => (false, 0, 0),
        };
        if bCur.0 {
            if bCur.1 == P_SLICE && bCur.2 != 0 {
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
                    if let Some(id) = self.m_pSpatialPic[kiCurDid as usize][(i + 1) as usize] {
                        self.m_pSpatialPicPool.get_mut(id).SetUnref();
                    }
                    i -= 1;
                }
                self.m_iAvaliableRefInSpatialPicList = 1;
            }
        }
        if let Some(id) = self.GetCurrentOrigFrame(kiCurDid) {
            self.m_pSpatialPicPool.get_mut(id).SetUnref();
        }
    }

    pub unsafe fn UpdateSrcListLosslessScreenRefSelectionWithLtr(
        &mut self,
        _pCurPicture: Option<SrcPicId>,
        kiCurDid: i32,
        kuiMarkLongTermPicIdx: i32,
        pLongRefList: &crate::encoder::encoder_context::SRefList,
    ) {
        let pLongRefSrcList = self.m_pSpatialPic[kiCurDid as usize].as_mut_ptr();
        for i in 0..MAX_REF_PIC_COUNT {
            // The *source* picture at `i + 1` and the *reconstruction* picture at `i`
            // — two pools, which is why the reference list arrives whole rather than
            // as a slice of handles this object could not resolve.
            let Some(idRef) = self.m_pSpatialPic[kiCurDid as usize][i + 1] else {
                continue;
            };
            let bLongLive = match pLongRefList.pLongRefList[i] {
                Some(idLong) => {
                    let p = pLongRefList.pic(idLong);
                    p.bUsedAsRef && p.bIsLongRef
                }
                None => false,
            };
            if bLongLive {
                continue;
            }
            self.m_pSpatialPicPool.get_mut(idRef).SetUnref();
        }
        Self::WelsExchangeSpatialPictures(
            pLongRefSrcList,
            pLongRefSrcList.offset((1 + kuiMarkLongTermPicIdx) as isize),
        );
        self.m_iAvaliableRefInSpatialPicList = MAX_REF_PIC_COUNT as i32;
        if let Some(id) = self.GetCurrentOrigFrame(kiCurDid) {
            self.m_pSpatialPicPool.get_mut(id).SetUnref();
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

    /// The pre-processor is constructed with its plugins in place — there is no
    /// create/destroy pair for a vtable any more (`IWelsVP` dissolved, Phase 6
    /// session B). This used to exercise `WelsPreprocessCreate`/`Destroy`.
    #[test]
    fn test_wels_preprocess_init_and_uninit() {
        let preprocess = CWelsPreProcess::default();
        assert!(!preprocess.m_bInitDone);
        assert!(preprocess.m_pEncCtx.is_null());
        // The plugins are owned and start at their defaults.
        assert!(!preprocess.m_vp.sVaaCalc.m_sCalcParam.iCalcBgd);
        drop(preprocess);
    }

    #[test]
    fn test_downsample_buffer_geometry() {
        let scaled_pic = Scaled_Picture::default();
        assert_eq!(scaled_pic.iScaledWidth[0], 0);
        assert_eq!(scaled_pic.iScaledHeight[0], 0);
        assert!(scaled_pic.pScaledInputPicture.is_none());
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
