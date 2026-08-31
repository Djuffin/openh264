#![deny(unsafe_code)]
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
use std::ffi::c_char;
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
use crate::encoder::encoder_context::{ctx_ltr_at, ctx_param_raw};
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
    /// The block-static array this scene-change result was computed against, as
    /// an **address**.
    ///
    /// **S10.11: `*mut u8` -> `usize`, and it is the last thing keeping
    /// `sWelsEncCtx` off `Sync` on this path** — `CSceneChangeDetection` owns one
    /// of these, `CWelsPreProcess` owns the detector, and the context owns the
    /// preprocess.
    ///
    /// It is a **ferry**: filled from `SVAAFrameInfoExt::pVaaBlockStaticIdc[i]` and
    /// copied straight out again into `SRefInfoParam::pBestBlockStaticIdc`. Nothing
    /// on this hop reads through it. The one site that does dereference the family
    /// — `SetBlockStaticIdcToMd`, four reads off
    /// `pVaaBestBlockStaticIdc` — is downstream of the copy-out and keeps its
    /// pointer type, so the exposure has to survive the round trip: in through
    /// `expose_provenance`, out through `with_exposed_provenance_mut`, as
    /// `TraceUserCtx` does.
    ///
    /// (The whole family is `SCREEN_CONTENT(dormant)`: F177 records that the port
    /// never allocates an `SVAAFrameInfoExt`, so every one of these is null and the
    /// dereference is unreachable. The round trip is written to be sound if it ever
    /// is reached, not because it is reached today.)
    pub pStaticBlockIdc: usize,
    pub sScrollResult: SScrollDetectionParam,
}

impl Default for SSceneChangeResult {
    fn default() -> Self {
        Self {
            eSceneChangeIdc: ESceneChangeIdc::SIMILAR_SCENE,
            iMotionBlockNum: 0,
            iFrameComplexity: 0,
            pStaticBlockIdc: 0,
            sScrollResult: SScrollDetectionParam::default(),
        }
    }
}

#[derive(Debug)]
pub struct SVAACalcResult {
    /// The two source plane **addresses**, as integers — the identity of the pair
    /// this result was computed over, and nothing else.
    ///
    /// **S10.9: `*mut u8` -> `usize`, because that is what they are.** They were
    /// raw plane roots, and the whole-tree read is one comparison in
    /// `adaptive_quantization.rs`: "reuse the VAA statistics when they were
    /// computed over exactly this pair of pictures". Nothing dereferences them —
    /// the walk that *reads* pixels takes its own cursors. Storing an identity as
    /// a pointer bought nothing and cost `SVAAFrameInfo` its `*mut u8` `!Sync`
    /// reason, which is one of the three that keep `sWelsEncCtx` off the fork
    /// seam.
    ///
    /// An address is plain `Copy` data — the tree's own phrase, from the two-thread
    /// probes that carry layer addresses across a spawn the same way.
    pub pCurY: usize,
    pub pRefY: usize,
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
            pCurY: 0,
            pRefY: 0,
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
    /// **T9.X — `pMotionTextureUnit` and `pMotionTextureIndexToDeltaQp` are gone from
    /// here.** They were `*mut`-SMotionTextureUnit and `*mut`-i8, and they were two of
    /// `SVAAFrameInfo`'s four `!Sync` reasons (F67/F164). They are owned `Vec`s on
    /// `SVAAFrameInfo` now and reach `CAdaptiveQuantization::Process` as slices, which
    /// is the shape `SVAACalcResult`'s six arrays already had — "handed over at the
    /// call rather than storing a pointer to it in the parameter block".
    ///
    /// This struct stays `Copy`, which is why the buffers could not simply become
    /// `Vec` fields *here*: `Set` copies the whole block once per frame.
    pub iAverMotionTextureIndexToDeltaQp: i32,
}

impl Default for SAdaptiveQuantizationParam {
    fn default() -> Self {
        Self {
            iAdaptiveQuantMode: 0,
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
    // **T9.X — `pGomComplexity` and `pGomForegroundBlockNum` are gone from here.**
    // They were two `*mut`-i32 and they were never this block's memory: the caller
    // aimed them at the rate controller's `pCurrentFrameGomSad` and
    // `pGomForegroundBlockNum` `Vec`s one line before every `Process` call
    // (`wels_preprocess.cpp:859/:924` does the same, misnomer and all —
    // `pGomComplexity` is pointed at the *SAD* array). They reach the plugin as
    // slices now, which retires `SVAAFrameInfo`'s `*mut`-i32 `!Sync` reason.
    //
    // **S10.9: `pBackgroundMbFlag` and `uiRefMbType` followed them out.** The note
    // that stood here said they stay — the first "a self-pointer into
    // `SVAAFrameInfo::pVaaBackgroundMbFlag`", the second with "no writer in this
    // port at all". The second half was stale: `SetRefMbType` *is* ported and does
    // aim the pointer at a reference picture's array. And "a self-pointer into a
    // field of the enclosing struct" is a reason to remove it, not to keep it —
    // the plugin can be handed the slice.
    //
    // Both reach `Process` as slices now, which retires `SVAAFrameInfo`'s last two
    // `!Sync` reasons (`*mut i8`, `*mut u32`) alongside the `*mut i32` the earlier
    // pair took.
}

impl Default for SComplexityAnalysisParam {
    fn default() -> Self {
        Self {
            iComplexityAnalysisMode: 0,
            iCalcBgd: false,
            iMbNumInGom: 0,
            iFrameComplexity: 0,
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

    /// The reference and current **source** pictures, as shared three-plane views —
    /// S9.0c, replacing six `*mut u8` plane roots.
    ///
    /// These are the operands of `VaaBackgroundMbDataUpdate`'s three copies, and the
    /// copy runs *previous source -> current source* (F117: `PCopyFunc` is
    /// `(pDst, .., pSrc, ..)`, so `pCur*` is the **destination**). It happens
    /// in-fork, per macroblock, into the picture the encoder is simultaneously
    /// reading — which is why these are `RoPicView`s over `SharedPlane` rather than
    /// slice cursors: the cells make the concurrent write lawful by construction,
    /// where a `&[u8]` over the plane would claim every byte and race it.
    pub pRefView: Option<crate::encoder::rec_view::RoPicView>,
    pub pCurView: Option<crate::encoder::rec_view::RoPicView>,

    /// One byte per macroblock, **owned since T6.F3** — `RequestMemorySvc`'s
    /// seventh and last `WelsMallocz` for the VAA block.
    pub pVaaBackgroundMbFlag: Vec<i8>,

    /// `encoder_ext.cpp:1721` — `iCountMaxMbNum * sizeof(SMotionTextureUnit)`.
    /// **T9.X: this allocation did not exist in the port at all**, and the field it
    /// fills was a permanently-null `*mut` on `sAdaptiveQuantParam` with two
    /// unguarded dereferences (`adaptive_quantization.rs`). See F177.
    pub pMotionTextureUnit: Vec<SMotionTextureUnit>,
    /// `encoder_ext.cpp:1724` — `iCountMaxMbNum * sizeof(int8_t)`. Same story.
    pub pMotionTextureIndexToDeltaQp: Vec<i8>,
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
        // `encoder_ext.cpp:1721-1726`, the two blocks this constructor never cut.
        p.pMotionTextureUnit = vec![SMotionTextureUnit::default(); n];
        p.pMotionTextureIndexToDeltaQp = vec![0i8; n];
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
            pRefView: None,
            pCurView: None,
            pVaaBackgroundMbFlag: Vec::new(),
            pMotionTextureUnit: Vec::new(),
            pMotionTextureIndexToDeltaQp: Vec::new(),
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
///
/// **S5.C6a**: `pData: *mut u8` became the plane it always pointed into. The caller
/// hands `&mut plane.as_mut_slice()[plane.origin()..]`, which is byte-for-byte the
/// `data_ptr(i)` this used to take — `data_ptr` *is* `root_ptr() + origin()` — so the
/// addresses written are unchanged and the extent is now carried rather than trusted.
/// The `is_null()` guard becomes `is_empty()`: a picture with no plane answered null
/// there and answers an empty slice here, which is the same question.
#[inline]
pub fn ClearEndOfLinePadding(pData: &mut [u8], iStride: i32, iWidth: i32, iHeight: i32) {
    if !pData.is_empty() && iWidth < iStride {
        let diff = (iStride - iWidth) as usize;
        for i in 0..iHeight {
            let at = (i * iStride + iWidth) as usize;
            pData[at..at + diff].fill(0);
        }
    }
}

/// Row-by-row planar memory copy for I420 YUV buffers — **the ingest
/// primitive**: the source pointers are the application's plane buffers, raw
/// C-ABI data with no owner on this side of the boundary, so the claim lives
/// on this signature and is asserted once, by the wrapper's guards.
///
/// # Safety
/// Every pointer must address a live plane of at least `iWidth x iHeight`
/// (halved for chroma) bytes at its stride, and the source and destination
/// planes must not overlap.
#[inline]
// unsafe-cat: C-ABI — the application's plane buffers (ingest).
#[allow(unsafe_code)]
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
pub fn WelsUpdateSpatialIdxMap(
    pEncCtx: &mut sWelsEncCtx,
    iPos: i32,
    pPic: Option<SrcPicId>,
    iDidx: i32,
) {
    // T9.H: the `!pEncCtx.is_null()` conjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null, so it was always true. The rest is unchanged.
    if iPos >= 0 && (iPos as usize) < MAX_DEPENDENCY_LAYER {
        let idx = iPos as usize;
        pEncCtx.sSpatialIndexMap[idx].pSrc = pPic;
        pEncCtx.sSpatialIndexMap[idx].iDid = iDidx;
    }
}

/// Evaluates whether the input picture requires aspect-ratio preserving scaling.
/// **S5.C6a**: `*mut Scaled_Picture` became the `&mut` its one caller already held.
/// `WelsInitScaledPic` is passed `&mut self.m_sScaledPicture` and hands it straight
/// through, so the pointer was a reference that had been through a cast and back, and
/// the `is_null()` guard could not fire.
pub fn JudgeNeedOfScaling(
    pParam: &SWelsSvcCodingParam,
    pScaledPicture: &mut Scaled_Picture,
) -> bool {
    let kiInputPicWidth = pParam.SUsedPicRect.iWidth;
    let kiInputPicHeight = pParam.SUsedPicRect.iHeight;
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
            pScaledPicture.iScaledWidth[idx] = iCurDstWidth.max(4);
            let h = if kiInputPicWidth != 0 {
                iInputHeightXDstWidth / kiInputPicWidth
            } else {
                0
            };
            pScaledPicture.iScaledHeight[idx] = h.max(4);
        } else {
            let w = if kiInputPicHeight != 0 {
                iInputWidthXDstHeight / kiInputPicHeight
            } else {
                0
            };
            pScaledPicture.iScaledWidth[idx] = w.max(4);
            pScaledPicture.iScaledHeight[idx] = iCurDstHeight.max(4);
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
///
/// **S5.C6a**: as [`JudgeNeedOfScaling`], which it hands its own parameter to — the
/// single caller passes `&mut self.m_sScaledPicture`.
pub fn WelsInitScaledPic(
    pParam: &SWelsSvcCodingParam,
    pScaledPicture: &mut Scaled_Picture,
) -> i32 {
    let bInputPicNeedScaling = JudgeNeedOfScaling(pParam, pScaledPicture);
    if bInputPicNeedScaling {
        pScaledPicture.pScaledInputPicture = AllocPicture(
            pParam.SUsedPicRect.iWidth,
            pParam.SUsedPicRect.iHeight,
            false,
            0,
        );
        if pScaledPicture.pScaledInputPicture.is_none() {
            return -1;
        }

        // S5.C6a: the plane triple, safely. `planes_mut3` is the same three planes
        // `planes()` handed back as `data_ptr`s, and the `[origin..]` re-slice is what
        // that pointer arithmetic spelled.
        let pPic = (*pScaledPicture)
            .pScaledInputPicture
            .as_deref_mut()
            .expect("just allocated");
        let (kiW, kiH) = (pPic.iWidthInPixel, pPic.iHeightInPixel);
        let [py, pu, pv] = pPic.planes_mut3();
        for (plane, kiPlaneW, kiPlaneH) in
            [(py, kiW, kiH), (pu, kiW >> 1, kiH >> 1), (pv, kiW >> 1, kiH >> 1)]
        {
            let (o, kiStride) = (plane.origin(), plane.stride() as i32);
            ClearEndOfLinePadding(&mut plane.as_mut_slice()[o..], kiStride, kiPlaneW, kiPlaneH);
        }
    }
    0
}

/// Releases the scaled picture. **Since T6.F2 that is a drop** — the picture owns
/// every byte it has, so `CMemoryAlign` is not involved and neither is a free walk.
/// **S5.C6a**: as [`WelsInitScaledPic`] — both call sites pass
/// `&mut ..m_sScaledPicture`, so the null test could not fire.
pub fn FreeScaledPic(pScaledPicture: &mut Scaled_Picture) {
    pScaledPicture.pScaledInputPicture = None;
}

// ============================================================================
// Core Preprocessing Engine: CWelsPreProcess
// ============================================================================

pub struct CWelsPreProcess {
    /// The video-processing plugins, owned. Was `m_pInterfaceVp: *mut IWelsVP`, a
    /// pointer to the dissolved vtable whose `pCtx` was this object behind a `void*`.
    pub m_vp: Box<crate::processing::SWelsVpContext>,
    // `m_pEncCtx` stood here — **deleted at T9.H2, and the deletion is the fix**
    // (F192). It was a raw copy of the encoder context, stashed by `CreatePreProcess`
    // and read back by four screen-content methods. Miri calls those reads Undefined
    // Behavior whenever a caller holds `&mut sWelsEncCtx`, because a reference
    // function argument is strongly protected for the duration of the call and the
    // whole context is inside it. All four take the context as a parameter now, as
    // ten of their sibling methods already did, which left this field write-only —
    // and a write-only raw copy of the context is not merely dead, it is the
    // hazard's *storage*. Deleting it means no later reader can reintroduce the
    // shape by accident. It has no C-ABI surface: `CWelsPreProcess` is this port's
    // own object and no size pin names it.
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
    pub fn CreatePreProcess(pEncCtx: &mut sWelsEncCtx) -> Option<Box<CWelsPreProcess>> {
        // T9.H: the `pEncCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
        // cannot be null and every caller now holds one. S3.B1: the `Box` is
        // returned as itself — `None` is the null the raw return carried.
        pEncCtx.param_opt()?;

        // Built whole and boxed (S21: the object owns a `Box` now, so a zeroed
        // shell is not a valid intermediate). This used to `alloc_zeroed` and set
        // three fields; `Default` is those zeros written out.
        Some(Box::new(CWelsPreProcess {
            m_eUsageType: pEncCtx.param().iUsageType,
            ..Default::default()
        }))
    }

    // `Destroy(pPreProcess: *mut CWelsPreProcess)` stood here — the C++
    // destructor's shape, `Box::from_raw` on a raw slot. **S11.42, deleted: no
    // callers.** The object lives in `sWelsEncCtx::pVpp` as `Option<Box<..>>`
    // and drops by ownership; `FreeScaledPic` runs from the owner's teardown.

    // `WelsPreprocessCreate` / `WelsPreprocessDestroy` (`wels_preprocess.cpp:198`)
    // were here: they allocated and freed the `IWelsVP` vtable and its `void*`
    // context. The plugins are owned by `m_vp` from construction, so there is
    // nothing left for either to do — deleted with their calls (S18, Phase 6
    // session B). Their history: the create used to `alloc_zeroed` the vtable and
    // stop, leaving every method `None` and the whole video-analysis stage
    // silently producing zeros — see `crate::processing`.

    pub fn WelsPreprocessReset(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        iWidth: i32,
        iHeight: i32,
    ) -> i32 {
        // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
        // cannot be null and every caller now holds one. The rest is unchanged.
        if pCtx.param_opt().is_none() {
            return -1;
        }

        let pSvcParam = pCtx.param_mut();
        (*pSvcParam).SUsedPicRect.iLeft = 0;
        (*pSvcParam).SUsedPicRect.iTop = 0;
        (*pSvcParam).SUsedPicRect.iWidth = iWidth;
        (*pSvcParam).SUsedPicRect.iHeight = iHeight;

        if iWidth < 16 || iHeight < 16 {
            return -1;
        }

        FreeScaledPic(&mut self.m_sScaledPicture);
        self.InitLastSpatialPictures(pCtx);
        WelsInitScaledPic(pCtx.param(), &mut self.m_sScaledPicture)
    }

    pub fn AllocSpatialPictures(&mut self, pCtx: &mut sWelsEncCtx) -> i32 {
        // A7: the `pParam` argument is gone — see `InitFunctionPointers`.
        let pParam = pCtx.param();
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

    pub fn FreeSpatialPictures(&mut self, pCtx: &mut sWelsEncCtx) {
        // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
        // cannot be null and every caller now holds one. The rest is unchanged.
        if pCtx.param_opt().is_none() {
            return;
        }
        let mut j = 0;
        while j < pCtx.param().iSpatialLayerNum {
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

    pub fn BuildSpatialPicList(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        // S11.41: the application's picture struct arrives as a reference — the
        // API layer null-checks the C pointer once; only the plane roots inside
        // it stay raw (they are the application's buffers).
        kpSrcPic: &SSourcePicture,
        pSpatialNum: &mut i32,
    ) -> i32 {
        // A7, §4.6 reorder: three scalars, read where they are used — this body
        // hands the context to `WelsPreprocessReset` between them.
        let iWidth = (kpSrcPic.iPicWidth >> 1) << 1;
        let iHeight = (kpSrcPic.iPicHeight >> 1) << 1;
        *pSpatialNum = 0;

        if !self.m_bInitDone {
            if self.WelsPreprocessReset(pCtx, iWidth, iHeight) != 0 {
                return ENC_RETURN_MEMALLOCERR;
            }
            self.m_iAvaliableRefInSpatialPicList = pCtx.param().iNumRefFrame;
            self.m_bInitDone = true;
        } else if iWidth != pCtx.param().SUsedPicRect.iWidth
            || iHeight != pCtx.param().SUsedPicRect.iHeight
        {
            if self.WelsPreprocessReset(pCtx, iWidth, iHeight) != 0 {
                return ENC_RETURN_MEMALLOCERR;
            }
        }

        if pCtx.vaa().is_some() {
            pCtx.vaa_mut().expect("the frame's video-analysis block").bSceneChangeFlag = false;
            pCtx.vaa_mut().expect("the frame's video-analysis block").bIdrPeriodFlag = false;
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

    // unsafe-cat: port-raw(Phase 9)
    pub fn SingleLayerPreprocess(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        kpSrc: &SSourcePicture,
        pSpatialNum: &mut i32,
    ) -> i32 {
        // A7, §4.6 reorder: the layer's geometry is four scalars, read here rather
        // than through a borrow that would have to span every context call below.
        let mut iDependencyId = pCtx.param().iSpatialLayerNum - 1;

        let depIdx = iDependencyId as usize;
        // S11.42: S29's cursor is gone — its five uses were reads, and each
        // re-derives through `param()` at its use (the loop below already spelled
        // the same idiom), so nothing survives anything.
        let iTargetWidth = pCtx.param().sSpatialLayers[depIdx].iVideoWidth;
        let iTargetHeight = pCtx.param().sSpatialLayers[depIdx].iVideoHeight;
        let iSrcWidth = pCtx.param().SUsedPicRect.iWidth;
        let iSrcHeight = pCtx.param().SUsedPicRect.iHeight;
        let uiIntraPeriod = pCtx.param().uiIntraPeriod;

        if uiIntraPeriod != 0 && pCtx.vaa().is_some() {
            let iFrameIndex = pCtx.param().sDependencyLayers[depIdx].iFrameIndex;
            pCtx.vaa_mut()
                .expect("the frame's video-analysis block")
                .bIdrPeriodFlag = (1 + iFrameIndex) >= uiIntraPeriod as i32;
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

        let iRet = self.WelsMoveMemoryWrapper(pCtx.param(), pSrcPic, kpSrc, iSrcWidth, iSrcHeight);
        if iRet != ENC_RETURN_SUCCESS {
            return iRet;
        }

        if pCtx.param().bEnableDenoise {
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

        if pCtx.param().bEnableSceneChangeDetect && pCtx.vaa().is_some() && !pCtx.vaa().expect("the frame's video-analysis block").bIdrPeriodFlag {
            if pCtx.param().iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
                let idc = if pCtx.param().sDependencyLayers[depIdx].bEncCurFrmAsIdrFlag {
                    ESceneChangeIdc::LARGE_CHANGED_SCENE
                } else {
                    self.DetectSceneChange(pCtx, pDstPic, None)
                };
                pCtx.vaa_mut().expect("the frame's video-analysis block").eSceneChangeIdc = idc;
                pCtx.vaa_mut().expect("the frame's video-analysis block").bSceneChangeFlag = idc == ESceneChangeIdc::LARGE_CHANGED_SCENE;
            } else if !pCtx.param().sDependencyLayers[depIdx].bEncCurFrmAsIdrFlag
                && (pCtx.param().sDependencyLayers[depIdx].iCodingIndex
                    & (pCtx.param().uiGopSize as i32 - 1))
                    == 0
            {
                let pRefPic = if ctx_ltr_at(pCtx, depIdx).bReceivedT0LostFlag {
                    let pos = self.m_uiSpatialLayersInTemporal[depIdx] as usize
                        + pCtx.vaa().expect("the frame's video-analysis block").uiValidLongTermPicIdx as usize;
                    self.m_pSpatialPic[depIdx][pos]
                } else {
                    self.m_pLastSpatialPicture[depIdx][0]
                };
                let pRefPic = pRefPic.map(SrcPicRef::Pooled);
                let idc = self.DetectSceneChange(pCtx, pDstPic, pRefPic);
                pCtx.vaa_mut().expect("the frame's video-analysis block").bSceneChangeFlag = self.GetSceneChangeFlag(idc);
            }
        }

        let mut iSpatialNum = 0;
        for i in 0..pCtx.param().iSpatialLayerNum {
            let pInternal = &pCtx.param().sDependencyLayers[i as usize];
            let gopMask = pCtx.param().uiGopSize as i32 - 1;
            let tid = pInternal.uiCodingIdx2TemporalId[(pInternal.iCodingIndex & gopMask) as usize];
            if tid != INVALID_TEMPORAL_ID {
                iSpatialNum += 1;
            }
        }

        let gopMask = pCtx.param().uiGopSize as i32 - 1;
        let tid = {
            let pInternal = &pCtx.param().sDependencyLayers[depIdx];
            pInternal.uiCodingIdx2TemporalId[(pInternal.iCodingIndex & gopMask) as usize]
        };
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

        if pCtx.param().iSpatialLayerNum > 1 {
            while iDependencyId >= 0 {
                let curDepIdx = iDependencyId as usize;
                let pInt = &pCtx.param().sDependencyLayers[curDepIdx];
                let pLay = &pCtx.param().sSpatialLayers[curDepIdx];
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

    pub fn AnalyzeSpatialPic(&mut self, pCtx: &mut sWelsEncCtx, kiDidx: i32) -> i32 {
        // A7, §4.6 reorder: every parameter field this body reads is a scalar, and
        // this body hands the context to eight different callees, so nothing is held
        // as a borrow across them.
        let dIdx = kiDidx as usize;
        let bNeededMbAq =
            pCtx.param().bEnableAdaptiveQuant && (pCtx.eSliceType == EWelsSliceType::P_SLICE);
        let bCalculateBGD = (pCtx.eSliceType == EWelsSliceType::P_SLICE)
            && pCtx.param().bEnableBackgroundDetection;
        let kbEnableBackgroundDetection = pCtx.param().bEnableBackgroundDetection;
        let kiUsageType = pCtx.param().iUsageType;
        let iCurTemporalIdx = self.m_uiSpatialLayersInTemporal[dIdx] as i32 - 1;

        let gopMask = pCtx.param().uiGopSize as i32 - 1;
        let (kiDecompositionStages, kiCodingIndex) = {
            let p = &pCtx.param().sDependencyLayers[dIdx];
            (p.iDecompositionStages, p.iCodingIndex)
        };
        let stageIdx =
            kiDecompositionStages.max(0).min(MAX_TEMPORAL_LEVEL as i32 - 1) as usize;
        let gopIdx = (kiCodingIndex & gopMask) as usize;
        let mut iRefTemporalIdx = g_kuiRefTemporalIdx[stageIdx][gopIdx] as i32;

        // T9.G6: hoisted — `ctx_ltr_at` takes the context retag and its own second
        // argument reads through the same context (shape B).
        let uiDidForLtr = pCtx.uiDependencyId as usize;
        if pCtx.uiTemporalId == 0
            && ctx_ltr_at(pCtx, uiDidForLtr).bReceivedT0LostFlag
        {
            iRefTemporalIdx = self.m_uiSpatialLayersInTemporal[dIdx] as i32
                + pCtx.vaa().expect("the frame's video-analysis block").uiValidLongTermPicIdx as i32;
        }

        let pCurPic = self.m_pSpatialPic[dIdx][iCurTemporalIdx as usize];
        let bCalculateVar = (pCtx.param().iRCMode as i32 >= RC_BITRATE_MODE)
            && (pCtx.eSliceType == EWelsSliceType::I_SLICE);

        if kiUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            // T9.G6's hoist, for T9.H2's reason: the callee takes the context now,
            // so its two other arguments may not read through the same context in the
            // same argument list.
            let bSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
            let eSliceType = pCtx.eSliceType;
            let iUsageType = kiUsageType;
            let pRefPic = self.GetBestRefPicScreen(
                pCtx,
                iUsageType,
                bSceneLtr,
                eSliceType,
                kiDidx,
                iRefTemporalIdx,
            );

            // A5: the three passes take `&mut SVAAFrameInfo` now, so their old
            // `if pVaaInfo.is_null() { return; }` prologues are these `if let`s —
            // the same no-op on an unbuilt block, asked one level out.
            if let Some(pVaa) = pCtx.vaa_mut() {
                self.VaaCalculation(pVaa, pCurPic, pRefPic, false, bCalculateVar, bCalculateBGD);
            }

            if kbEnableBackgroundDetection {
                let bFlag = bCalculateBGD && self.ref_is_inter(pRefPic);
                if let Some(pVaa) = pCtx.vaa_mut() {
                    self.BackgroundDetection(pVaa, pCurPic, pRefPic, bFlag);
                }
            }
            if bNeededMbAq {
                if let Some(pVaa) = pCtx.vaa_mut() {
                    self.AdaptiveQuantCalculation(pVaa, pCurPic, pRefPic);
                }
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

            if let Some(pVaa) = pCtx.vaa_mut() {
                self.VaaCalculation(pVaa, pCurPic, pRefPic, bCalculateSQDiff, bCalculateVar, bCalculateBGD);
            }

            if kbEnableBackgroundDetection {
                let bFlag = bCalculateBGD && self.ref_is_inter(pRefPic);
                if let Some(pVaa) = pCtx.vaa_mut() {
                    self.BackgroundDetection(pVaa, pCurPic, pRefPic, bFlag);
                }
            }

            if bNeededMbAq {
                let pLast1 = self.m_pLastSpatialPicture[dIdx][1];
                let pLast0 = self.m_pLastSpatialPicture[dIdx][0];
                if let Some(pVaa) = pCtx.vaa_mut() {
                    self.AdaptiveQuantCalculation(pVaa, pLast1, pLast0);
                }
            }
            VP_DUMP_SQD.store(bCalculateSQDiff, std::sync::atomic::Ordering::Relaxed);
        }

        if crate::encoder::dump_enabled(&VP_DUMP, "OH264_VPDUMP")
            && pCtx.eSliceType == EWelsSliceType::P_SLICE
            && !pCtx.vaa().expect("the frame's video-analysis block").pVaaBackgroundMbFlag.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSad8x8.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSumOfDiff8x8.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pMad8x8.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSsd16x16.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSum16x16.is_empty()
            && !pCtx.vaa().expect("the frame's video-analysis block").sVaaCalcInfo.pSumOfSquare16x16.is_empty()
        {
            let v = pCtx.vaa().expect("the frame's video-analysis block");
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
                pCtx.eSliceType as i32,
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

    pub fn GetCurPicPosition(&self, kiDidx: i32) -> i32 {
        self.m_uiSpatialLayersInTemporal[kiDidx as usize] as i32 - 1
    }

    pub fn GetCurrentOrigFrame(&mut self, iDIdx: i32) -> Option<SrcPicId> {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.m_pSpatialPic[iDIdx as usize][0]
        } else {
            let pos = self.GetCurPicPosition(iDIdx) as usize;
            self.m_pSpatialPic[iDIdx as usize][pos]
        }
    }

    pub fn GetBestRefPic(&self, kiDidx: i32, iRefTemporalIdx: i32) -> Option<SrcPicId> {
        self.m_pSpatialPic[kiDidx as usize][iRefTemporalIdx as usize]
    }

    pub fn GetBestRefPicScreen(
        &self,
        // **T9.H2, F192.** The context is a parameter now, where this used to reach
        // it through `self.m_pEncCtx`. Miri calls that read Undefined Behavior: a
        // reference function argument is *strongly protected* for the duration of
        // the call, and the caller above holds `&mut sWelsEncCtx` over the whole
        // context — so a read through a stored raw of the same allocation may not
        // remove it, whatever the byte ranges are. Ten sibling methods on this type
        // already took the context this way; these four were the exceptions.
        pCtx: &mut sWelsEncCtx,
        _iUsageType: EUsageType,
        bSceneLtr: bool,
        _eSliceType: EWelsSliceType,
        _kiDidx: i32,
        _iRefTemporalIdx: i32,
    ) -> Option<SrcPicId> {
        // S11.3: `None` in this port (F177) — there are no screen best-reference
        // candidates, so there is no candidate picture to name.
        let pVaaExt = pCtx.vaa_ext_ref()?;
        let pBest = if bSceneLtr {
            &pVaaExt.sVaaLtrBestRefCandidate[0]
        } else {
            &pVaaExt.sVaaStrBestRefCandidate[0]
        };
        self.m_pSpatialPic[0][pBest.iSrcListIdx as usize]
    }

    pub fn UpdateSpatialPictures(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        iCurTid: i8,
        kiDidx: i32,
    ) -> i32 {
        // A7: the `pParam` argument is gone — see `InitFunctionPointers`. This body
        // already derived the parameters from the context two lines down.
        if pCtx.param_opt().is_none() || pCtx.param().iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            return 0;
        }

        let dIdx = kiDidx as usize;
        Self::WelsExchangeSpatialPictures(&mut self.m_pLastSpatialPicture[dIdx], 1, 0);

        let kiCurPos = self.GetCurPicPosition(kiDidx);
        if (iCurTid as i32) < kiCurPos || pCtx.param().iDecompStages == 0 {
            if (iCurTid as usize) >= MAX_TEMPORAL_LEVEL || (kiCurPos as usize) > MAX_TEMPORAL_LEVEL {
                self.InitLastSpatialPictures(pCtx);
                return 1;
            }
            if pCtx.bRefOfCurTidIsLtr[dIdx][iCurTid as usize] {
                let kiAvailableLtrPos = self.m_uiSpatialLayersInTemporal[dIdx] as usize
                    + pCtx.vaa().expect("the frame's video-analysis block").uiMarkLongTermPicIdx as usize;
                Self::WelsExchangeSpatialPictures(
                    &mut self.m_pSpatialPic[dIdx],
                    kiAvailableLtrPos,
                    iCurTid as usize,
                );
                pCtx.bRefOfCurTidIsLtr[dIdx][iCurTid as usize] = false;
            }
            Self::WelsExchangeSpatialPictures(
                &mut self.m_pSpatialPic[dIdx],
                kiCurPos as usize,
                iCurTid as usize,
            );
        }

        0
    }

    /// `CWelsPreProcess::BilateralDenoising` — `wels_preprocess.cpp:620`.
    ///
    /// **Ported in Phase 8b session C (T8b.C1).** This was an empty body: the C++
    /// built the source pixel map and ran the denoise plugin in place, the port's
    /// dispatch returned `RET_NOTSUPPORTED`, and *this caller never read it*, so
    /// asking for denoise silently produced un-denoised output. That is the exact
    /// class of lie S48 refuses, and it is why `ParamValidationExt` rejected
    /// `bEnableDenoise` until this session.
    ///
    /// The C++ hands `CDenoiser::Process` an `SPixMap` of three raw plane pointers
    /// and a `NULL` destination (denoising is in place). The kernels here take
    /// slices, so the `SPixMap` survives only as the geometry carrier that
    /// `Process` reads `sRect` from, and the planes are resolved through
    /// `planes_mut3()` — safe, no pointer crosses the boundary.
    pub fn BilateralDenoising(&mut self, pSrc: SrcPicRef, kiWidth: i32, kiHeight: i32) {
        let mut sSrcPixMap = SPixMap {
            sRect: SRect {
                iRectWidth: kiWidth,
                iRectHeight: kiHeight,
                ..SRect::default()
            },
            ..SPixMap::default()
        };
        // `m_uiType` copied out first: the picture and the plugin are both behind
        // this `&mut self`, and only the method call makes those borrows look like
        // they overlap. See `denoise::Denoise`.
        let uiType = self.m_vp.sDenoise.m_uiType;
        let pic = self.src_mut(pSrc);
        sSrcPixMap.iStride = [pic.stride(0), pic.stride(1), pic.stride(2)];

        let [py, pu, pv] = pic.planes_mut3();
        let stride = [py.stride(), pu.stride(), pv.stride()];
        let (oy, ou, ov) = (py.origin(), pu.origin(), pv.origin());
        let mut planes = crate::processing::denoise::DenoisePlanes {
            y: &mut py.as_mut_slice()[oy..],
            u: &mut pu.as_mut_slice()[ou..],
            v: &mut pv.as_mut_slice()[ov..],
            stride,
        };
        // The C++ drops this return too (`m_pInterfaceVp->Process(...)` as a
        // statement); the only failure it can report is a null/empty plane, which
        // for an owned picture is the parse-only shape and never reaches here.
        let _ = crate::processing::denoise::Denoise(uiType, &sSrcPixMap, &mut planes);
    }

    pub fn DownsamplePadding(
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

        // **S5.C6a — the two `SPixMap` locals are gone, and the compiler is what
        // found them.** `sSrcPixMap` and `sDstPicMap` were built field by field on
        // every call — twenty assignments — and once `Padding` stopped taking plane
        // roots, `rustc` reported every one of them as "value assigned is never
        // read". They were **write-only**: `sSrcPixMap`'s single reader was
        // `sDstPicMap = sSrcPixMap`, and `sDstPicMap`'s single reader was the
        // `Padding` call. Deleting a write-only local cannot move a byte.
        //
        // What they were carrying is `Padding`'s question — *which picture* — and
        // that is now one `SrcPicRef` (`padRef` below) instead of two descriptors.
        //
        // S37's note stood here: "resolve both pictures to their plane roots up front
        // and work through raw cursors from here — `srcRef` and `dstRef` are
        // frequently the same picture, which no pair of references could express."
        // That is still true of `pSrc`/`pDstPic` below, which the copy arm hands to
        // `WelsMoveMemory_c` as six raw roots, and it is why that arm stays raw.
        let pSrc = self.src_mut(srcRef).planes();
        let pDstPic = self.src_mut(dstRef).planes();

        // **S5.C6a**: the branch's condition, named. It is what the deleted
        // `sDstPicMap` encoded — the destination when this arm writes into it, the
        // source when it does not — and so it is what decides which picture `Padding`
        // must borrow at the end.
        let bDstIsWritten = iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight || bForceCopy;

        if bDstIsWritten {
            if iSrcWidth != iShrinkWidth || iSrcHeight != iShrinkHeight {
                // **`METHOD_DOWNSAMPLE`, ported in Phase 8b session C (T8b.C2).**
                // This was `iRet = RET_NOTSUPPORTED` — and *both* callers dropped
                // `iRet`, so a lower spatial layer was encoded from whatever the
                // picture pool last held. Every multi-layer row in the gtest
                // allowlist was this one line.
                //
                // The scratch is moved out of the plugin first: the two pictures
                // and `m_vp` are all behind this `&mut self`, and the borrows are
                // disjoint in fact but not in what a method call can express (the
                // same shape `BilateralDenoising` has). `src_pair_mut` is safe to
                // use here because this arm runs only when the two differ in size,
                // so they cannot be the same picture.
                let mut scratch =
                    std::mem::take(&mut self.m_vp.sDownsample.m_pSampleBuffer);
                let (srcPic, dstPic) = self.src_pair_mut(srcRef, dstRef);
                {
                    let [sy, su, sv] = srcPic.planes_mut3();
                    let srcStride = [sy.stride(), su.stride(), sv.stride()];
                    let (soy, sou, sov) = (sy.origin(), su.origin(), sv.origin());
                    let src = crate::processing::downsample::DownsampleSrc {
                        planes: [
                            &sy.as_slice()[soy..],
                            &su.as_slice()[sou..],
                            &sv.as_slice()[sov..],
                        ],
                        stride: srcStride,
                        width: iSrcWidth,
                        height: iSrcHeight,
                    };
                    let [dy, du, dv] = dstPic.planes_mut3();
                    let dstStride = [dy.stride(), du.stride(), dv.stride()];
                    let (doy, dou, dov) = (dy.origin(), du.origin(), dv.origin());
                    let mut dst = crate::processing::downsample::DownsampleDst {
                        planes: [
                            &mut dy.as_mut_slice()[doy..],
                            &mut du.as_mut_slice()[dou..],
                            &mut dv.as_mut_slice()[dov..],
                        ],
                        stride: dstStride,
                        width: iShrinkWidth,
                        height: iShrinkHeight,
                    };
                    iRet = crate::processing::downsample::Downsample(
                        &mut scratch,
                        &src,
                        &mut dst,
                    );
                }
                self.m_vp.sDownsample.m_pSampleBuffer = scratch;
            } else {
                // S11.42: the copy primitive's claim, at a pool-to-pool site —
                // both `PicPlanes` were resolved from live pool pictures above
                // (S37's shape), two distinct slots, geometry the pool's own.
                // unsafe-cat: port-raw(Phase 9)
                #[allow(unsafe_code)]
                unsafe {
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
            }
        }

        iShrinkWidth -= iShrinkWidth & 1;
        iShrinkHeight -= iShrinkHeight & 1;
        // **S5.C6a.** `sDstPicMap`'s three plane roots and its two strides were only
        // ever a way to carry *one* picture's identity to this call, and which picture
        // is what `bDstIsWritten` decides. Borrowing it here rather than describing it
        // above is what makes a safe `Padding` possible: only one picture is live at
        // this point, so S37's "no pair of references could express" — which is about
        // holding source and destination at once — does not apply.
        //
        // `plane.stride()` is `SPicture::stride(i)`, which is what `iLineSize[i]` and
        // so `sDstPicMap.iStride[i]` held; `[origin..]` is what `data_ptr(i)` pointed
        // at. Same picture, same strides, same first byte.
        let padRef = if bDstIsWritten { dstRef } else { srcRef };
        let [py, pu, pv] = self.src_mut(padRef).planes_mut3();
        let (oy, ou, ov) = (py.origin(), pu.origin(), pv.origin());
        let (kiStrideY, kiStrideUV) = (py.stride() as i32, pu.stride() as i32);
        Self::Padding(
            &mut py.as_mut_slice()[oy..],
            &mut pu.as_mut_slice()[ou..],
            &mut pv.as_mut_slice()[ov..],
            kiStrideY,
            kiStrideUV,
            iShrinkWidth,
            iTargetWidth,
            iShrinkHeight,
            iTargetHeight,
        );

        iRet
    }

    pub fn VaaCalculation(
        &mut self,
        pVaaInfo: &mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
        bCalculateSQDiff: bool,
        bCalculateVar: bool,
        bCalculateBGD: bool,
    ) {
        let (Some(idCur), Some(idRef)) = (pCurPicture, pRefPicture) else {
            return;
        };
        // §4.6 (S11.43): the plugin and the pool are sibling fields, split by
        // destructure so the pass can run while both pictures stay borrowed —
        // the plane slices replace the raw roots the pixel maps used to carry
        // (S37's prologue resolved raw cursors here for the same reach).
        let CWelsPreProcess { m_vp, m_pSpatialPicPool, .. } = &mut *self;
        let (kpCur, kpRef) = (m_pSpatialPicPool.get(idCur), m_pSpatialPicPool.get(idRef));
        let (kpCurY, kpRefY) = (kpCur.plane_tail(0), kpRef.plane_tail(0));
        pVaaInfo.sVaaCalcInfo.pCurY = kpCurY.as_ptr() as usize;
        pVaaInfo.sVaaCalcInfo.pRefY = kpRefY.as_ptr() as usize;

        let mut sCurPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();
        let mut calc_param = SVAACalcParam::default();

        sCurPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sCurPixMap.sRect.iRectWidth = kpCur.iWidthInPixel;
        sCurPixMap.sRect.iRectHeight = kpCur.iHeightInPixel;
        sCurPixMap.iStride[0] = kpCur.stride(0);
        sCurPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.sRect.iRectWidth = kpRef.iWidthInPixel;
        sRefPixMap.sRect.iRectHeight = kpRef.iHeightInPixel;
        sRefPixMap.iStride[0] = kpRef.stride(0);
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        calc_param.iCalcVar = bCalculateVar;
        calc_param.iCalcBgd = bCalculateBGD;
        calc_param.iCalcSsd = bCalculateSQDiff;

        // METHOD_VAA_STATISTICS. The result is handed over at the call; the C++
        // stored `&pVaaInfo->sVaaCalcInfo` in the parameter block.
        m_vp.sVaaCalc.Set(&calc_param);
        m_vp.sVaaCalc.Process(
            &sCurPixMap,
            &sRefPixMap,
            crate::processing::vaacalc::VaaCalcPlanes { cur: kpCurY, refp: kpRefY },
            &mut pVaaInfo.sVaaCalcInfo,
        );
    }

    pub fn BackgroundDetection(
        &mut self,
        pVaaInfo: &mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
        bDetectFlag: bool,
    ) {
        let Some(idCur) = pCurPicture else {
            return;
        };
        // §4.6 (S11.43): as at `VaaCalculation` — the plugin and the pool split
        // by destructure, the six plane roots now six borrows (S37's raw-cursor
        // prologue retired with them).
        let CWelsPreProcess { m_vp, m_pSpatialPicPool, .. } = &mut *self;
        let kpCur = m_pSpatialPicPool.get(idCur);
        let kpRef = pRefPicture.map(|id| m_pSpatialPicPool.get(id));
        if let (true, Some(kpRef)) = (bDetectFlag, kpRef) {
            pVaaInfo.iPicWidth = kpCur.iWidthInPixel;
            pVaaInfo.iPicHeight = kpCur.iHeightInPixel;
            pVaaInfo.iPicStride = kpCur.stride(0);
            pVaaInfo.iPicStrideUV = kpCur.stride(1);
            // S9.0c: the six plane roots become two views, built where the
            // pictures are reachable. Rebuilt every frame, as the layer's views are:
            // the pool may hand the next frame a different slot.
            pVaaInfo.pCurView =
                Some(crate::encoder::rec_view::RoPicView::build(kpCur));
            pVaaInfo.pRefView =
                Some(crate::encoder::rec_view::RoPicView::build(kpRef));

            let mut sSrcPixMap = SPixMap::default();
            let mut sRefPixMap = SPixMap::default();
            let BGDParam = SBGDInterface::default();

            sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sSrcPixMap.iStride[0] = kpCur.stride(0);
            sSrcPixMap.iStride[1] = kpCur.stride(1);
            sSrcPixMap.iStride[2] = kpCur.stride(2);
            sSrcPixMap.sRect.iRectWidth = kpCur.iWidthInPixel;
            sSrcPixMap.sRect.iRectHeight = kpCur.iHeightInPixel;
            sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

            sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
            sRefPixMap.iStride[0] = kpRef.stride(0);
            sRefPixMap.iStride[1] = kpRef.stride(1);
            sRefPixMap.iStride[2] = kpRef.stride(2);
            sRefPixMap.sRect.iRectWidth = kpRef.iWidthInPixel;
            sRefPixMap.sRect.iRectHeight = kpRef.iHeightInPixel;
            sRefPixMap.eFormat = VideoFormat::videoFormatI420;

            // S10.12: the flag array reaches `Process` as a slice. `Set` no longer
            // stashes it — see `CBackgroundDetection::Set` — but the call stays,
            // because the C++ makes it and the port mirrors the sequence.
            m_vp.sBackgroundDetection.Set(&BGDParam);
            let SVAAFrameInfo { sVaaCalcInfo, pVaaBackgroundMbFlag, .. } = pVaaInfo;
            m_vp.sBackgroundDetection.Process(
                &sSrcPixMap,
                &sRefPixMap,
                &crate::processing::background_detection::BgdPlanes {
                    cur: [kpCur.plane_tail(0), kpCur.plane_tail(1), kpCur.plane_tail(2)],
                    refp: [kpRef.plane_tail(0), kpRef.plane_tail(1), kpRef.plane_tail(2)],
                },
                sVaaCalcInfo,
                pVaaBackgroundMbFlag,
            );
        } else {
            let iPicWidthInMb = (kpCur.iWidthInPixel + 15) >> 4;
            let iPicHeightInMb = (kpCur.iHeightInPixel + 15) >> 4;
            let n = ((iPicWidthInMb * iPicHeightInMb).max(0) as usize)
                .min(pVaaInfo.pVaaBackgroundMbFlag.len());
            (&mut pVaaInfo.pVaaBackgroundMbFlag)[..n].fill(0);
        }
    }

    pub fn AdaptiveQuantCalculation(
        &mut self,
        pVaaInfo: &mut SVAAFrameInfo,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<SrcPicId>,
    ) {
        let (Some(idCur), Some(idRef)) = (pCurPicture, pRefPicture) else {
            return;
        };
        // §4.6 (S11.43): as at `VaaCalculation` — the plugin and the pool split
        // by destructure, the two luma planes as borrows.
        let CWelsPreProcess { m_vp, m_pSpatialPicPool, .. } = &mut *self;
        let (kpCur, kpRef) = (m_pSpatialPicPool.get(idCur), m_pSpatialPicPool.get(idRef));
        // The C++ stored `&pVaaInfo->sVaaCalcInfo` *inside* `pVaaInfo` here
        // (`sAdaptiveQuantParam.pCalcResult`) — a self-pointer; the result is
        // handed over at the `Process` call instead.
        pVaaInfo.sAdaptiveQuantParam.iAverMotionTextureIndexToDeltaQp = 0;

        let mut pSrc = SPixMap::default();
        let mut pRef = SPixMap::default();

        pSrc.iSizeInBits = g_kiPixMapSizeInBits;
        pSrc.iStride[0] = kpCur.stride(0);
        pSrc.sRect.iRectWidth = kpCur.iWidthInPixel;
        pSrc.sRect.iRectHeight = kpCur.iHeightInPixel;
        pSrc.eFormat = VideoFormat::videoFormatI420;

        pRef.iSizeInBits = g_kiPixMapSizeInBits;
        pRef.iStride[0] = kpRef.stride(0);
        pRef.sRect.iRectWidth = kpRef.iWidthInPixel;
        pRef.sRect.iRectHeight = kpRef.iHeightInPixel;
        pRef.eFormat = VideoFormat::videoFormatI420;

        // METHOD_ADAPTIVE_QUANT.
        m_vp.sAdaptiveQuant.Set(&pVaaInfo.sAdaptiveQuantParam);
        let iRet = m_vp.sAdaptiveQuant.Process(
            &pSrc,
            &pRef,
            crate::processing::vaacalc::VaaCalcPlanes {
                cur: kpCur.plane_tail(0),
                refp: kpRef.plane_tail(0),
            },
            &pVaaInfo.sVaaCalcInfo,
            &mut pVaaInfo.pMotionTextureUnit,
            &mut pVaaInfo.pMotionTextureIndexToDeltaQp,
        );
        if iRet == 0 {
            m_vp.sAdaptiveQuant.Get(&mut pVaaInfo.sAdaptiveQuantParam);
        }
    }

    /// **S5.C6a**: the three `*mut u8` plane roots became the planes themselves, and
    /// `&self` went with them — the body never touched it. `Padding` writes into
    /// *one* picture, which is what makes the conversion possible where the rest of
    /// `DownsamplePadding` resisted it: S37's note that source and destination "are
    /// frequently the same picture, which no pair of references could express" is
    /// about holding both maps at once, and this call holds neither. Its one caller
    /// resolves which picture to pad *after* the copy or downsample is done, so a
    /// single `&mut` is all that is ever live here.
    ///
    /// The `is_null()` triple becomes `is_empty()`, the same question a plane answers
    /// (see `ClearEndOfLinePadding`), and every write is a `fill` over a slice range
    /// rather than a `write_bytes` at an offset — the same bytes, with the extent
    /// checked instead of assumed.
    pub fn Padding(
        pSrcY: &mut [u8],
        pSrcU: &mut [u8],
        pSrcV: &mut [u8],
        iStrideY: i32,
        iStrideUV: i32,
        iActualWidth: i32,
        iPaddingWidth: i32,
        iActualHeight: i32,
        iPaddingHeight: i32,
    ) {
        if pSrcY.is_empty() || pSrcU.is_empty() || pSrcV.is_empty() {
            return;
        }

        if iPaddingHeight > iActualHeight {
            for i in iActualHeight..iPaddingHeight {
                let at = (i * iStrideY) as usize;
                pSrcY[at..at + iActualWidth as usize].fill(0);
                if (i & 1) == 0 {
                    let atc = ((i / 2) * iStrideUV) as usize;
                    let kiW2 = (iActualWidth / 2) as usize;
                    pSrcU[atc..atc + kiW2].fill(0x80);
                    pSrcV[atc..atc + kiW2].fill(0x80);
                }
            }
        }

        if iPaddingWidth > iActualWidth {
            let diff = (iPaddingWidth - iActualWidth) as usize;
            let diffUV = diff / 2;
            for i in 0..iPaddingHeight {
                let at = (i * iStrideY + iActualWidth) as usize;
                pSrcY[at..at + diff].fill(0);
                if (i & 1) == 0 {
                    let atc = ((i / 2) * iStrideUV + iActualWidth / 2) as usize;
                    pSrcU[atc..atc + diffUV].fill(0x80);
                    pSrcV[atc..atc + diffUV].fill(0x80);
                }
            }
        }
    }

    /// **S5.C6a**: the list and two indices, instead of two `*mut` into it.
    ///
    /// Every one of the six call sites was already naming positions in *one* array —
    /// three of them as `&mut arr[i], &mut arr[j]` (which the raw parameters then
    /// erased), and three as `list.offset(i)` off an `as_mut_ptr()` of the same array.
    /// Two `&mut` into one array is the thing references cannot express and the reason
    /// this was raw; a slice plus indices expresses it exactly, and `[T]::swap` is the
    /// body. The null tests go with the pointers — an array element has no null.
    ///
    /// The three `.offset()` sites gain a bounds check they did not have. The three
    /// `&mut arr[i]` sites had one already, so nothing there changes.
    pub fn WelsExchangeSpatialPictures(
        pPicList: &mut [Option<SrcPicId>],
        iPos1: usize,
        iPos2: usize,
    ) {
        pPicList.swap(iPos1, iPos2);
    }

    pub fn InitLastSpatialPictures(&mut self, pCtx: &mut sWelsEncCtx) -> i32 {
        let pParam = pCtx.param();
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

    pub fn WelsMoveMemoryWrapper(
        &mut self,
        pSvcParam: &SWelsSvcCodingParam,
        pDstRef: SrcPicRef,
        kpSrc: &SSourcePicture,
        kiTargetWidth: i32,
        kiTargetHeight: i32,
    ) -> i32 {
        if (VideoFormat::videoFormatI420 as i32) != (kpSrc.iColorFormat & !(-0x80000000i32)) {
            return ENC_RETURN_INVALIDINPUT;
        }

        // S37: the destination resolved once to its plane roots.
        let pDstPic = self.src_mut(pDstRef).planes();

        let mut iSrcWidth = kpSrc.iPicWidth;
        let mut iSrcHeight = kpSrc.iPicHeight;

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

        let iSrcOffset0 = kpSrc.iStride[0] * kiSrcTopOffsetY + kiSrcLeftOffsetY;
        let iSrcOffset1 = kpSrc.iStride[1] * kiSrcTopOffsetUV + kiSrcLeftOffsetUV;
        let iSrcOffset2 = kpSrc.iStride[2] * kiSrcTopOffsetUV + kiSrcLeftOffsetUV;

        // S11.42: `wrapping_offset` — the arithmetic is safe Rust; whether the
        // resulting pointers are valid is the copy's claim, asserted once at the
        // `WelsMoveMemory_c` call below after the guards have run.
        let pSrcY = if !kpSrc.pData[0].is_null() {
            kpSrc.pData[0].wrapping_offset(iSrcOffset0 as isize)
        } else {
            std::ptr::null_mut()
        };
        let pSrcU = if !kpSrc.pData[1].is_null() {
            kpSrc.pData[1].wrapping_offset(iSrcOffset1 as isize)
        } else {
            std::ptr::null_mut()
        };
        let pSrcV = if !kpSrc.pData[2].is_null() {
            kpSrc.pData[2].wrapping_offset(iSrcOffset2 as isize)
        } else {
            std::ptr::null_mut()
        };

        let kiSrcStrideY = kpSrc.iStride[0];
        let kiSrcStrideU = kpSrc.iStride[1];
        let kiSrcStrideV = kpSrc.iStride[2];

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

        // unsafe-cat: C-ABI — the ingest copy from the application's buffers.
        // The guards above have checked the null/size/stride contract the C++
        // checks; what remains — that the application's pointers address what
        // its strides promise — is the API's contract, named on the callee.
        #[allow(unsafe_code)]
        unsafe {
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
        }

        if kiTargetWidth > iSrcWidth || kiTargetHeight > iSrcHeight {
            // S5.C6a: the destination re-derived as planes, after the copy above has
            // finished with its raw cursors. One picture, one borrow — see `Padding`.
            let [py, pu, pv] = self.src_mut(pDstRef).planes_mut3();
            let (oy, ou, ov) = (py.origin(), pu.origin(), pv.origin());
            Self::Padding(
                &mut py.as_mut_slice()[oy..],
                &mut pu.as_mut_slice()[ou..],
                &mut pv.as_mut_slice()[ov..],
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

    pub fn GetSceneChangeFlag(&self, eSceneChangeIdc: ESceneChangeIdc) -> bool {
        eSceneChangeIdc == ESceneChangeIdc::LARGE_CHANGED_SCENE
    }

    pub fn DetectSceneChange(
        &mut self,
        // T9.H2, F192: threaded from `SingleLayerPreprocess`, which already holds it,
        // so the screen arm below can stop reading `self.m_pEncCtx`. The video arm
        // does not need it and does not take it.
        pCtx: &mut sWelsEncCtx,
        pCurPicture: SrcPicRef,
        pRefPicture: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        if self.m_eUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            self.DetectSceneChangeScreen(pCtx, pCurPicture, pRefPicture)
        } else {
            self.DetectSceneChangeVideo(pCurPicture, pRefPicture)
        }
    }

    fn DetectSceneChangeVideo(
        &mut self,
        pCurPicture: SrcPicRef,
        pRefPicture: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        let Some(pRefPicture) = pRefPicture else {
            return ESceneChangeIdc::SIMILAR_SCENE;
        };
        // **T9.X — the views come off the pictures, not off a raw stamp.** Both
        // pictures are read here and only read, so two *shared* borrows of the pool
        // coexist where the old `src_mut(..).planes()` pair had to launder itself
        // through raw to end the borrow. `src()` takes `&self` and so would borrow
        // the plugin along with the pool; destructuring names the two fields
        // separately, which is what lets the `&mut` on `m_vp` sit beside them.
        let Self {
            m_pSpatialPicPool,
            m_sScaledPicture,
            m_vp,
            ..
        } = self;
        let pick = |which: SrcPicRef| -> &SPicture {
            match which {
                SrcPicRef::Pooled(id) => m_pSpatialPicPool.get(id),
                SrcPicRef::Scaled => m_sScaledPicture
                    .pScaledInputPicture
                    .as_deref()
                    .expect("the scaled input picture is allocated"),
            }
        };
        let cur_pic = pick(pCurPicture);
        let cur_y = cur_pic.plane(0);
        let (cur_w, cur_h) = (cur_pic.iWidthInPixel, cur_pic.iHeightInPixel);
        let ref_pic = pick(pRefPicture);
        let ref_y = ref_pic.plane(0);
        let (ref_w, ref_h) = (ref_pic.iWidthInPixel, ref_pic.iHeightInPixel);

        // METHOD_SCENE_CHANGE_DETECTION_VIDEO: no `Set` in the C++ either.
        let mut sSceneChangeDetectResult = SSceneChangeResult::default();
        let mut sSrcPixMap = SPixMap::default();
        let mut sRefPixMap = SPixMap::default();

        sSrcPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sSrcPixMap.iStride[0] = cur_y.stride() as i32;
        sSrcPixMap.sRect.iRectWidth = cur_w;
        sSrcPixMap.sRect.iRectHeight = cur_h;
        sSrcPixMap.eFormat = VideoFormat::videoFormatI420;

        sRefPixMap.iSizeInBits = g_kiPixMapSizeInBits;
        sRefPixMap.iStride[0] = ref_y.stride() as i32;
        sRefPixMap.sRect.iRectWidth = ref_w;
        sRefPixMap.sRect.iRectHeight = ref_h;
        sRefPixMap.eFormat = VideoFormat::videoFormatI420;

        let planes = crate::processing::scene_change_detection::ScdPlanes {
            cur: &cur_y.as_slice()[cur_y.origin()..],
            cur_stride: cur_y.stride(),
            refp: &ref_y.as_slice()[ref_y.origin()..],
            ref_stride: ref_y.stride(),
        };

        let iRet = m_vp.sSceneChangeDetection.Process(&sSrcPixMap, &planes);
        if iRet == 0 {
            m_vp.sSceneChangeDetection.Get(&mut sSceneChangeDetectResult);
        }
        sSceneChangeDetectResult.eSceneChangeIdc
    }

    fn DetectSceneChangeScreen(
        &mut self,
        // T9.H2, F192 — see `GetBestRefPicScreen`.
        pCtx: &mut sWelsEncCtx,
        pCurPicture: SrcPicRef,
        _pRef: Option<SrcPicRef>,
    ) -> ESceneChangeIdc {
        // The `pCtx.is_null()` disjunct went with the stored raw: a `&mut
        // sWelsEncCtx` cannot be null. The rest of the guard is unchanged.
        if pCtx.vaa().is_none() {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        // A7, §4.6 reorder: four scalars out of the parameter block, none of them
        // held across the context calls below.
        // S11.3: `None` in this port (F177). This body both reads the screen
        // candidates and writes them back, so it takes the mutable accessor;
        // with no extension it takes the same exit `iTargetDid != 0` does two
        // lines down, which is this screen arm's established "no usable scene
        // analysis" answer.
        if pCtx.vaa_ext_ref().is_none() {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }
        // §4.6, reorder: the scalar comes out before the extension's `&mut`.
        let kiMvRange = pCtx.iMvRange;
        let iTargetDid = pCtx.param().iSpatialLayerNum - 1;
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

        let gopMask = pCtx.param().uiGopSize as i32 - 1;
        let iCurTid = {
            let p = &pCtx.param().sDependencyLayers[0];
            p.uiCodingIdx2TemporalId[(p.iCodingIndex & gopMask) as usize]
        };
        if iCurTid == INVALID_TEMPORAL_ID {
            return ESceneChangeIdc::LARGE_CHANGED_SCENE;
        }

        // A7: nothing of the parameter block is live here now — the two reads above
        // were consumed into `i32`s — so the LTR state's `&mut` stands alone.
        let iClosestLtrFrameNum =
            ctx_ltr_at(&mut *pCtx, iTargetDid as usize).iLastLtrIdx[iCurTid as usize];
        if pCtx.param().bEnableLongTermReference {
            self.GetAvailableRefListLosslessScreenRefSelection(
                pCtx,
                &pRefPicList,
                iCurTid,
                iClosestLtrFrameNum,
                &mut sAvailableRefParam,
                &mut iAvailableRefNum,
                &mut iAvailableSceneRefNum,
            );
        } else {
            self.GetAvailableRefList(
                &pRefPicList,
                iCurTid,
                iClosestLtrFrameNum,
                &mut sAvailableRefParam,
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
            let pCurBlockStaticPointer = pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").pVaaBlockStaticIdc[iScdIdx as usize];
            let mut sSceneChangeResult = SSceneChangeResult::default();
            sSceneChangeResult.eSceneChangeIdc = ESceneChangeIdc::SIMILAR_SCENE;
            sSceneChangeResult.pStaticBlockIdc = pCurBlockStaticPointer.expose_provenance();

            let pRefPicInfo = &mut sAvailableRefParam[iScdIdx as usize];
            let Some(idRefPic) = pRefPicInfo.pRefPicture else {
                continue;
            };
            let sRefGeom = self.m_pSpatialPicPool.get_mut(idRefPic).planes();
            Self::InitPixMap(&sRefGeom, &mut sRefMap);

            let bIsClosestLtrFrame =
                self.src_id(idRefPic).iLongTermPicNum == iClosestLtrFrameNum;
            if iScdIdx == 0 {
                let pScrollDetectInfo = &mut pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").sScrollDetectInfo;
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
                                .clamp(-kiMvRange, kiMvRange);
                            pScrollDetectInfo.iScrollMvY = pScrollDetectInfo
                                .iScrollMvY
                                .clamp(-kiMvRange, kiMvRange);
                        }
                    }
                }
                sSceneChangeResult.sScrollResult = pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").sScrollDetectInfo;
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

        self.SaveBestRefToVaa(&sLtrSaved, &mut pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").sVaaStrBestRefCandidate[0]);
        if let Some(id) = sLtrSaved.pRefPicture {
            pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").iVaaBestRefFrameNum = self.src_id(id).iFrameNum;
        }
        pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").pVaaBestBlockStaticIdc = sLtrSaved.pBestBlockStaticIdc;

        if iAvailableSceneRefNum > 0 {
            self.SaveBestRefToVaa(&sSceneLtrSaved, &mut pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").sVaaLtrBestRefCandidate[0]);
        }

        pCtx.vaa_ext_ref_mut().expect("guarded at this body's head").iNumOfAvailableRef = 1;
        iVaaFrameSceneChangeIdc
    }

    /// **S5.C6b**: `*mut SPixMap` became the `&mut` both call sites already held
    /// (`&mut sSrcMap`, `&mut sRefMap` — locals of `DetectSceneChangeScreen`), so the
    /// null test could not fire.
    fn InitPixMap(pPicture: &PicPlanes, pPixMap: &mut SPixMap) {
        {
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

    /// **S5.C6b**: as [`InitPixMap`](Self::InitPixMap) — both call sites pass `&mut`
    /// to a caller local.
    fn InitRefJudgement(&self, pRefJudgement: &mut SRefJudgement) {
        {
            (*pRefJudgement).iMinFrameComplexity = i32::MAX as i64;
            (*pRefJudgement).iMinFrameComplexity08 = i32::MAX as i64;
            (*pRefJudgement).iMinFrameComplexity11 = i32::MAX as i64;
            (*pRefJudgement).iMinFrameNumGap = i32::MAX;
            (*pRefJudgement).iMinFrameQp = i32::MAX;
        }
    }

    fn JudgeBestRef(
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

    /// **S5.C6b**: as [`InitRefJudgement`](Self::InitRefJudgement).
    fn SaveBestRefToJudgement(
        &self,
        iRefPictureAvQP: i32,
        iComplexity: i64,
        pRefJudgement: &mut SRefJudgement,
    ) {
        {
            (*pRefJudgement).iMinFrameQp = iRefPictureAvQP;
            (*pRefJudgement).iMinFrameComplexity = iComplexity;
            (*pRefJudgement).iMinFrameComplexity08 = (iComplexity as f64 * 0.8) as i64;
            (*pRefJudgement).iMinFrameComplexity11 = (iComplexity as f64 * 1.1) as i64;
        }
    }

    /// **S5.C6b**: two raw out-parameters into one read and one write. The `&`/`&mut`
    /// split is what the body already did — `pRefPicInfo` is only read from.
    fn SaveBestRefToLocal(
        &self,
        pRefPicInfo: &SRefInfoParam,
        sSceneChangeResult: &SSceneChangeResult,
        pRefSaved: &mut SRefInfoParam,
    ) {
        {
            *pRefSaved = *pRefPicInfo;
            (*pRefSaved).pBestBlockStaticIdc =
                std::ptr::with_exposed_provenance_mut(sSceneChangeResult.pStaticBlockIdc);
        }
    }

    /// **S5.C6b**: as its siblings — both call sites pass `&mut (*pVaaExt).sVaa*[0]`.
    fn SaveBestRefToVaa(&self, sRefSaved: &SRefInfoParam, pVaaBestRef: &mut SRefInfoParam) {
        {
            *pVaaBestRef = *sRefSaved;
        }
    }

    fn GetAvailableRefListLosslessScreenRefSelection(
        &self,
        // T9.H2, F192 — see `GetBestRefPicScreen`.
        pCtx: &mut sWelsEncCtx,
        pRefPicList: &[Option<SrcPicId>],
        iCurTid: u8,
        iClosestLtrFrameNum: i32,
        pAvailableRefParam: &mut [SRefInfoParam],
        pAvailableRefNum: &mut i32,
        pAvailableSceneRefNum: &mut i32,
    ) {
        let iSourcePicNum = self.m_iAvaliableRefInSpatialPicList;
        if iSourcePicNum <= 0 {
            *pAvailableRefNum = 0;
            *pAvailableSceneRefNum = 0;
            return;
        }

        let bCurFrameMarkedAsSceneLtr = pCtx.bCurFrameMarkedAsSceneLtr;
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
                let param = &mut pAvailableRefParam[idx as usize];
                param.pRefPicture = Some(idRefPic);
                param.iSrcListIdx = i + 1;
                if bRefRealLtr {
                    *pAvailableSceneRefNum += 1;
                }
            }

            i -= 1;
        }

        if pAvailableRefParam[0].pRefPicture.is_none() {
            let mut j = 1;
            while j < *pAvailableRefNum {
                // S5.C6b: one `swap`-free shuffle down the slice — `pCur` is a copy
                // rather than a second borrow, which is what the two raw cursors were.
                let pCur = pAvailableRefParam[j as usize];
                let pPrev = &mut pAvailableRefParam[(j - 1) as usize];
                pPrev.pRefPicture = pCur.pRefPicture;
                pPrev.iSrcListIdx = pCur.iSrcListIdx;
                j += 1;
            }
            let last = &mut pAvailableRefParam[(*pAvailableRefNum - 1) as usize];
            last.pRefPicture = None;
            last.iSrcListIdx = 0;
            *pAvailableRefNum -= 1;
        }
    }

    fn GetAvailableRefList(
        &self,
        pSrcPicList: &[Option<SrcPicId>],
        iCurTid: u8,
        _iClosestLtrFrameNum: i32,
        pAvailableRefList: &mut [SRefInfoParam],
        pAvailableRefNum: &mut i32,
        pAvailableSceneRefNum: &mut i32,
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
                let param = &mut pAvailableRefList[*pAvailableRefNum as usize];
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
    /// **S10.9: returns *which* picture, not a pointer into it.** The out-parameter
    /// was `*mut *mut u32` and the body stored
    /// `pic_mut(id).ref_mb_type_root()` — a raw into the reference picture's
    /// `uiRefMbType` `Vec`, which then lived on `SComplexityAnalysisParam` and made
    /// `SVAAFrameInfo` `!Sync`. An identity carries the same decision and lets the
    /// caller take the slice under a borrow the compiler can see.
    pub fn SetRefMbType(
        &self,
        pCtx: &mut sWelsEncCtx,
        _iRefPicType: i32,
    ) -> Option<crate::encoder::picture::RecPicId> {
        let uiTid = pCtx.uiTemporalId;
        let uiDid = pCtx.uiDependencyId;
        // §4.6, reorder: the branch condition is decided before the list borrow —
        // it reads the parameters and the LTR state, two other fields of the same
        // context. T9.H3's inline-borrow shape, one statement earlier.
        let bLtrRecovery = pCtx.param().bEnableLongTermReference
            && ctx_ltr_at(pCtx, uiDid as usize).bReceivedT0LostFlag
            && uiTid == 0;
        let pRefPicLlist = pCtx.ref_list(uiDid as usize)?;

        if bLtrRecovery {
            for i in 0..pRefPicLlist.uiLongRefCount as usize {
                let Some(id) = pRefPicLlist.pLongRefList[i] else {
                    continue;
                };
                if pRefPicLlist.pic(id).uiRecieveConfirmed == RECIEVE_SUCCESS {
                    return Some(id);
                }
            }
        } else {
            for i in 0..pRefPicLlist.uiShortRefCount as usize {
                let Some(id) = pRefPicLlist.pShortRefList[i] else {
                    continue;
                };
                let pRef = pRefPicLlist.pic(id);
                if pRef.bUsedAsRef && pRef.iFramePoc >= 0 && pRef.uiTemporalId <= uiTid {
                    return Some(id);
                }
            }
        }
        None
    }

    pub fn AnalyzePictureComplexity(
        &mut self,
        pCtx: &mut sWelsEncCtx,
        pCurPicture: Option<SrcPicId>,
        pRefPicture: Option<RecPicId>,
        kiDependencyId: i32,
        bCalculateBGD: bool,
    ) {
        // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
        // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
        // remaining conditions are unchanged.
        if pCtx.param_opt().is_none() {
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
        let uiDidCur = pCtx.uiDependencyId as usize;
        let sCur = self.m_pSpatialPicPool.get_mut(idCur).planes();
        let sRefPic = pRefPicture.filter(|_| pCtx.ref_list(uiDidCur).is_some());
        let sRef = sRefPic
            .map(|id| {
                pCtx.ref_list_mut(uiDidCur)
                    .expect("checked just above")
                    .pic_mut(id)
                    .planes()
            })
            .unwrap_or(sCur);

        let pSvcParam = pCtx.param_mut();
        if (*pSvcParam).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
            // S11.3: `None` in this port (F177) — the extension's complexity
            // block does not exist, so the screen arm's analysis has nothing to
            // write into. This is the extension's **one writer**, and it is the
            // site a future screen-content effort makes live first.
            // §4.6, reorder: the slice type is read before the writer's `&mut`.
            let eSliceType = pCtx.eSliceType;

            let _iComplexityAnalysisMode = if eSliceType == EWelsSliceType::P_SLICE {
                GOM_SAD
            } else if eSliceType == EWelsSliceType::I_SLICE {
                GOM_VAR
            } else {
                return;
            };

            // S11.3, §4.6: the rate controller's work and the values the
            // extension needs from it come first, and its borrow ends before
            // the extension's begins — they are two `&mut` of one context, the
            // split S11.2c's `rc_and_current_layer_mut` makes for the layer.
            // Ordering suffices here because nothing reads the two at once.
            let (kpGomComplexity, kiGomNumInFrame) = {
                let pWelsSvcRc = pCtx.rc_at_mut(kiDependencyId as usize);
                pWelsSvcRc.pGomForegroundBlockNum.fill(0);
                pWelsSvcRc.pCurrentFrameGomSad.fill(0);
                (pWelsSvcRc.gom_sad_ptr(), pWelsSvcRc.iGomSize)
            };
            let kiIdrFlag = if eSliceType == EWelsSliceType::I_SLICE { 1 } else { 0 };

            let Some(pVaaExt) = pCtx.vaa_ext_ref_mut() else {
                return;
            };
            let sComplexityAnalysisParam = &mut pVaaExt.sComplexityScreenParam;

            sComplexityAnalysisParam.iFrameComplexity = 0;
            sComplexityAnalysisParam.pGomComplexity = kpGomComplexity;
            sComplexityAnalysisParam.iGomNumInFrame = kiGomNumInFrame;
            sComplexityAnalysisParam.iIdrFlag = kiIdrFlag;
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
            // §4.6, reorder: the writer's `&mut` sinks past everything that still
            // needs the context — the slice-type reads and `SetRefMbType`'s own
            // `&mut` — down to the first use of the rate controller's arrays.
            let eSliceType = pCtx.eSliceType;

            let kiRCMode = pCtx.param().iRCMode as i32;
            let iComplexityAnalysisMode = if kiRCMode == RC_QUALITY_MODE && eSliceType == EWelsSliceType::P_SLICE {
                FRAME_SAD
            } else if ((kiRCMode == RC_BITRATE_MODE) || (kiRCMode == RC_TIMESTAMP_MODE))
                && eSliceType == EWelsSliceType::P_SLICE
            {
                GOM_SAD
            } else if ((kiRCMode == RC_BITRATE_MODE) || (kiRCMode == RC_TIMESTAMP_MODE))
                && eSliceType == EWelsSliceType::I_SLICE
            {
                GOM_VAR
            } else {
                return;
            };

            // **§4.6, reorder, and A5's one real knot.** `SetRefMbType` takes the
            // whole context `&mut`, and its out-parameter is a single `*mut u32`
            // *inside* the block below — so the block's own `&mut` may not span
            // the call. The out-parameter is staged in a local instead, seeded
            // with the field's current value so that a `SetRefMbType` which
            // matches no reference leaves exactly what it left before (it writes
            // only on a match). None of the block's other writes are read by
            // `SetRefMbType`, so they move below it unchanged.
            // **S10.9: an identity, not a pointer.** This used to read the raw
            // `uiRefMbType` back off the analysis block, hand `SetRefMbType` a
            // `*mut *mut u32` to overwrite, and store the result — a borrow of the
            // reference picture's array living in a `Copy` struct. The identity is
            // resolved to a slice below, under a borrow the compiler can see.
            let mut idRefMbType: Option<crate::encoder::picture::RecPicId> = None;
            if let Some(idRef) = sRefPic {
                // §4.6, reorder: the picture type is read out before
                // `SetRefMbType` claims the context mutably.
                let iPictureType = pCtx
                    .ref_list(uiDidCur)
                    .expect("checked when `sRefPic` was filtered")
                    .pic(idRef)
                    .iPictureType;
                idRefMbType = self.SetRefMbType(pCtx, iPictureType);
            }

            // §4.6, combined accessor: `Process` below takes the analysis block's
            // `sVaaCalcInfo` shared and the rate controller's two GOM arrays
            // mutably, in one call.
            let (pVaaInfo, pWelsSvcRc, pRefListShared) =
                pCtx.vaa_rc_and_ref_list_mut(kiDependencyId as usize);
            let pVaaInfo = pVaaInfo.expect("the frame's video-analysis block");
            // S10.9: the reference picture's per-macroblock type array, resolved
            // from `SetRefMbType`'s identity. Empty where the raw was null — which
            // is the state `ref_mb_type_root` answered null for, a picture built
            // without `bNeedMbInfo`.
            let uiRefMbType: &[u32] = match (idRefMbType, pRefListShared) {
                (Some(id), Some(list)) => &list.pic(id).uiRefMbType,
                _ => &[],
            };
            let sComplexityAnalysisParam = &mut pVaaInfo.sComplexityAnalysisParam;

            sComplexityAnalysisParam.iComplexityAnalysisMode = iComplexityAnalysisMode;
            sComplexityAnalysisParam.iCalcBgd = bCalculateBGD;
            sComplexityAnalysisParam.iFrameComplexity = 0;

            (*pWelsSvcRc).pGomForegroundBlockNum.fill(0);
            if iComplexityAnalysisMode != FRAME_SAD {
                (*pWelsSvcRc).pCurrentFrameGomSad.fill(0);
            }

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
            // **T9.X**: the two GOM arrays are the rate controller's own `Vec`s and
            // reach the plugin as slices — `sComplexityAnalysisParam.pGomComplexity`
            // and `.pGomForegroundBlockNum` were `rc_gom_sad`/`rc_gom_fg_blocks`
            // stamped one line above this. `pGomComplexity` really is aimed at
            // `pCurrentFrameGomSad`: the VP's field name is a misnomer and
            // `wels_preprocess.cpp:859/:924` does exactly the same.
            let iRet = self.m_vp.sComplexityAnalysis.Process(
                &sSrcPixMap,
                &sRefPixMap,
                &pVaaInfo.sVaaCalcInfo,
                &mut (*pWelsSvcRc).pCurrentFrameGomSad,
                &mut (*pWelsSvcRc).pGomForegroundBlockNum,
                &pVaaInfo.pVaaBackgroundMbFlag,
                uiRefMbType,
            );
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
    pub fn GetRefFrameInfo(
        &mut self,
        // T9.H2, F192 — see `GetBestRefPicScreen`. This is the one of the four whose
        // caller is reachable-shaped: `ref_list_mgr_svc.rs`'s `WelsBuildRefListScreen`
        // takes `&mut sWelsEncCtx` and reached this through `pCtx.pVpp`, which is the
        // exact shape the probe reproduces.
        pCtx: &mut sWelsEncCtx,
        iRefIdx: i32,
        bCurrentFrameIsSceneLtr: bool,
        pRefOri: &mut Option<SrcPicId>,
    ) -> i32 {
        let iTargetDid = pCtx.param().iSpatialLayerNum - 1;
        // S11.3: `None` in this port (F177) — no screen candidates, so this
        // reports the "no reference chosen" result its callers already handle.
        let Some(pVaaExt) = pCtx.vaa_ext_ref() else {
            return 0;
        };
        let pBestRefCandidateParam = if bCurrentFrameIsSceneLtr {
            &pVaaExt.sVaaLtrBestRefCandidate[iRefIdx as usize]
        } else {
            &pVaaExt.sVaaStrBestRefCandidate[iRefIdx as usize]
        };
        let pPic =
            self.m_pSpatialPic[iTargetDid as usize][pBestRefCandidateParam.iSrcListIdx as usize];
        *pRefOri = pPic;
        self.src_id(pPic.expect("the best-reference candidate names a live slot"))
            .iLongTermPicNum
    }

    pub fn UpdateBlockIdcForScreen(
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
    pub fn UpdateSrcList(
        &mut self,
        pCurPicture: Option<SrcPicId>,
        kiCurDid: i32,
        kuiShortRefCount: u32,
    ) {

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
                        &mut self.m_pSpatialPic[kiCurDid as usize],
                        (iRefIdx + 1) as usize,
                        iRefIdx as usize,
                    );
                    iRefIdx -= 1;
                }
                self.m_iAvaliableRefInSpatialPicList = kuiShortRefCount as i32;
            } else {
                Self::WelsExchangeSpatialPictures(&mut self.m_pSpatialPic[kiCurDid as usize], 0, 1);
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

    pub fn UpdateSrcListLosslessScreenRefSelectionWithLtr(
        &mut self,
        _pCurPicture: Option<SrcPicId>,
        kiCurDid: i32,
        kuiMarkLongTermPicIdx: i32,
        pLongRefList: &crate::encoder::encoder_context::SRefList,
    ) {
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
            &mut self.m_pSpatialPic[kiCurDid as usize],
            0,
            (1 + kuiMarkLongTermPicIdx) as usize,
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
        // `m_pEncCtx` is gone (F192) — there is no stored context to assert about,
        // which is the point. What a fresh preprocessor still owes is its plugins at
        // their defaults, asserted below.
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
