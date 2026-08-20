// Copyright (c) 2009-2013, Cisco Systems
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

//! # Rate Control Engine (`rc.h` & `ratectl.cpp`)
//!
//! Translated from `codec/encoder/core/inc/rc.h` and `codec/encoder/core/src/ratectl.cpp`.
//!
//! This module implements OpenH264's multi-level hierarchical rate control subsystem,
//! governing bit allocation across Virtual GOPs, frame-level quantization parameter ($QP$) derivation,
//! Group of Macroblocks (GOM) adaptive quantization, Virtual Buffer Verifier (VBV) leaky-bucket
//! management, and dynamic frame skipping for temporal and spatial layers.
//!
//! # The raw-pointer census, attributed — T6.G4
//!
//! Phase 6 session G's step 4 was "sweep `rc.rs`'s remaining single-object
//! parameters". The sweep found **none left to take**, and that is a result rather
//! than an omission, so it is written down here where the next session will see it.
//! All **93** occurrences of `*mut`/`*const` in this file, by who owns them (counted
//! at session G's close; **T6.H6 spent the two rows marked below, and the count is 84**
//! — the nineteen the rc blocks cost, minus the five accessors' signatures and the
//! `CMemoryAlign` parameters that went with them):
//!
//! | count | what | whose |
//! |---|---|---|
//! | 61 | `pEncCtx`/`pCtx` parameters, raw `sWelsEncCtx` | **session I** — the context is the largest arena in the tree, and the S37 inventory decides `&mut` for all of it at once, not file by file |
//! | 16 | `SWelsSvcRc`'s own five member pointers and the reaches through them | **spent at T6.H6** — the five are owned containers, reached through `rc_temporal_over` and its four siblings |
//! |  7 | `pSlice` parameters, raw `SSlice` | **session I** — five sit behind `pfWelsRcMbInit`/`pfWelsRcMbInfoUpdate`, which is 4b's fence, and the two that do not are covered by the blocker below |
//! |  6 | `ctx_vaa(pEncCtx)` cast to a raw `SVAAFrameInfoExt` | **Phase 10** — the `SCREEN_CONTENT(dormant)` family, fenced |
//! |  3 | `RcInitLayerMemory`'s carve-up of one `CMemoryAlign` block | **spent at T6.H6** — the carve-up is gone; this file no longer names `CMemoryAlign` at all |
//!
//! **The one that looks convertible and is not.** `GomRCInitForOneSlice` takes a raw
//! `SSlice` and nothing else but an integer: a plain function with a single caller and
//! a single-object parameter — R1's shape exactly. Its caller is `WelsCodeOneSlice`
//! (`svc_encode_slice.rs`), which binds `pBs = slice_writer(pEncCtx, pCurSlice)`
//! **before** this call and uses `pBs` **after** it. A `&mut SSlice` here is a
//! `Unique` retag over the whole 6496-byte slice, so it would pop `pBs` — F13's
//! family and S25's rule, the same shape `svc_set_mb_syn_cavlc`'s header comment
//! records for its own writers. Converting it needs either the caller's binding
//! moved after the call (a behavioural change to check, not a spelling) or a
//! parameter narrowed to `&mut SRCSlicing` plus the two slice fields it reads,
//! which walks away from the C++ signature. Neither is a sweep; both are a decision
//! with an owner, and it is not this session's.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use crate::{RCMode, SSliceArgument, SSpatialLayerConfig, EUsageType};
pub use crate::encoder::svc_encode_slice::SSliceHeader;
use crate::encoder::svc_encode_slice::layer_pps;
use crate::encoder::svc_encode_slice::ctx_pps;
use crate::encoder::svc_encode_slice::current_layer;
pub use crate::encoder::svc_encode_slice::SSliceHeaderExt;
pub use crate::encoder::encoder_context::SSpatialPicIndex;
pub use crate::encoder::wels_preprocess::SAdaptiveQuantizationParam;
pub use crate::encoder::wels_preprocess::SComplexityAnalysisParam;
pub use crate::encoder::wels_preprocess::SComplexityAnalysisScreenParam;
pub use crate::encoder::wels_preprocess::SVAAFrameInfoExt;
pub use crate::encoder::param_svc::SSpatialLayerInternal;
pub use crate::encoder::wels_preprocess::SVAAFrameInfo;
pub use crate::encoder::param_svc::SWelsSvcCodingParam;
pub use crate::encoder::slice_multi_threading::SSliceCtx;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::md::SMB;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::wels_func_ptr_def::SWelsFuncPtrList;
pub use crate::encoder::encoder_context::sWelsEncCtx;
use crate::encoder::encoder_context::{ctx_param, ctx_rc, ctx_rc_at, ctx_vaa};

// ============================================================================
// Constants and Macros
// ============================================================================

pub const GOM_TRACE_FLAG: i32 = 0;
pub const GOM_H_SCC: i32 = 8;

// Bit consumption status
pub const BITS_NORMAL: i32 = 0;
pub const BITS_LIMITED: i32 = 1;
pub const BITS_EXCEEDED: i32 = 2;

// Virtual GOP and QP thresholds
pub const VGOP_SIZE: usize = 8;

pub const GOM_MIN_QP_MODE: i32 = 12;
pub const GOM_MAX_QP_MODE: i32 = 36;
pub const MAX_LOW_BR_QP: i32 = 42;
pub const MIN_IDR_QP: i32 = 26;
pub const MAX_IDR_QP: i32 = 32;
pub const MIN_SCREEN_QP: i32 = 26;
pub const MAX_SCREEN_QP: i32 = 35;
pub const DELTA_QP: i32 = 2;
pub const DELTA_QP_BGD_THD: i32 = 3;
pub const QP_MIN_VALUE: i32 = 0;
pub const QP_MAX_VALUE: i32 = 51;

// Frame skip base QP thresholds by resolution
pub const SKIP_QP_90P: i32 = 24;
pub const SKIP_QP_180P: i32 = 24;
pub const SKIP_QP_360P: i32 = 31;
pub const SKIP_QP_720P: i32 = 31;

pub const LAST_FRAME_QP_RANGE_UPPER_MODE0: i32 = 3;
pub const LAST_FRAME_QP_RANGE_LOWER_MODE0: i32 = 2;
pub const LAST_FRAME_QP_RANGE_UPPER_MODE1: i32 = 5;
pub const LAST_FRAME_QP_RANGE_LOWER_MODE1: i32 = 3;

pub const MB_WIDTH_THRESHOLD_90P: i32 = 15;
pub const MB_WIDTH_THRESHOLD_180P: i32 = 30;
pub const MB_WIDTH_THRESHOLD_360P: i32 = 60;

// Mode 0 parameters
pub const GOM_ROW_MODE0_90P: i32 = 2;
pub const GOM_ROW_MODE0_180P: i32 = 2;
pub const GOM_ROW_MODE0_360P: i32 = 4;
pub const GOM_ROW_MODE0_720P: i32 = 4;
pub const QP_RANGE_MODE0: i32 = 3;

// Mode 1 parameters
pub const GOM_ROW_MODE1_90P: i32 = 1;
pub const GOM_ROW_MODE1_180P: i32 = 1;
pub const GOM_ROW_MODE1_360P: i32 = 2;
pub const GOM_ROW_MODE1_720P: i32 = 2;
pub const QP_RANGE_UPPER_MODE1: i32 = 9;
pub const QP_RANGE_LOWER_MODE1: i32 = 4;
pub const QP_RANGE_INTRA_MODE1: i32 = 3;

// Bit allocation scaling
pub const MAX_BITS_VARY_PERCENTAGE: i32 = 100;
pub const MAX_BITS_VARY_PERCENTAGE_x3d2: i32 = 150;
pub const INT_MULTIPLY: i32 = 100;
pub const WEIGHT_MULTIPLY: i32 = 2000;
pub const REMAIN_BITS_TH: i32 = 1;
pub const VGOP_BITS_PERCENTAGE_DIFF: i32 = 5;
pub const IDR_BITRATE_RATIO: i32 = 4;
pub const FRAME_iTargetBits_VARY_RANGE: i32 = 50;

// R-Q Model
pub const LINEAR_MODEL_DECAY_FACTOR: i32 = 80;
pub const FRAME_CMPLX_RATIO_RANGE: i32 = 20;
pub const SMOOTH_FACTOR_MIN_VALUE: i32 = 2;

// Skip and padding
pub const TIME_CHECK_WINDOW: i32 = 5000; // ms
pub const SKIP_RATIO: i32 = 50;
pub const LAST_FRAME_PREDICT_WEIGHT: f64 = 0.5;
pub const PADDING_BUFFER_RATIO: i32 = 50;
pub const PADDING_THRESHOLD: i32 = 5;

pub const VIRTUAL_BUFFER_LOW_TH: i32 = 120;
pub const VIRTUAL_BUFFER_HIGH_TH: i32 = 180;

pub const UNSPECIFIED_BIT_RATE: i32 = 0;
pub const EPSN: f64 = 0.000001;

// Time Windows
pub const EVEN_TIME_WINDOW: usize = 0;
pub const ODD_TIME_WINDOW: usize = 1;
pub const TIME_WINDOW_TOTAL: usize = 2;

// Slice Types
pub const P_SLICE: i32 = 0;
pub const B_SLICE: i32 = 1;
pub const I_SLICE: i32 = 2;

// Scene change IDC
pub const NO_SCENE_CHANGE: i32 = 0;
pub const SIMILAR_SCENE: i32 = 0;
pub const MEDIUM_CHANGED_SCENE: i32 = 1;
pub const LARGE_CHANGED_SCENE: i32 = 2;

// Slice Modes
pub const SM_SINGLE_SLICE: i32 = 0;
pub const SM_FIXEDSLCNUM_SLICE: i32 = 1;
pub const SM_RASTER_SLICE: i32 = 2;
pub const SM_SIZELIMITED_SLICE: i32 = 3;

// ============================================================================
// Lookup Tables
// ============================================================================

/// Integer quantization step table for QP = 0..51, scaled by `INT_MULTIPLY` (100).
pub const g_kiQpToQstepTable: [i32; 52] = [
    63, 71, 79, 89, 100, 112, 126, 141, 159, 178,
    200, 224, 252, 283, 317, 356, 400, 449, 504, 566,
    635, 713, 800, 898, 1008, 1131, 1270, 1425, 1600, 1796,
    2016, 2263, 2540, 2851, 3200, 3592, 4032, 4525, 5080, 5702,
    6400, 7184, 8063, 9051, 10159, 11404, 12800, 14368, 16127, 18102,
    20319, 22807,
];

/// Chroma QP translation table for H.264 standard luma-to-chroma QP mapping.
pub const g_kuiChromaQpTable: [u8; 52] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37,
    37, 38, 38, 38, 39, 39, 39, 39,
];

// ============================================================================
// Math Helper Macros / Inline Functions
// ============================================================================

#[inline]
pub fn WELS_CLIP3<T: PartialOrd + Copy>(x: T, min_val: T, max_val: T) -> T {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline]
pub fn CLIP3_QP_0_51(x: i32) -> usize {
    WELS_CLIP3(x, 0, 51) as usize
}

#[inline]
pub fn WELS_ROUND(x: f64) -> i32 {
    (x + 0.5) as i32
}

#[inline]
pub fn WELS_ROUND64(x: f64) -> i64 {
    (x + 0.5) as i64
}

#[inline]
pub fn WELS_DIV_ROUND(x: i32, y: i32) -> i32 {
    if y == 0 {
        x / (y + 1)
    } else {
        ((y / 2) + x) / y
    }
}

#[inline]
pub fn WELS_DIV_ROUND64(x: i64, y: i64) -> i64 {
    if y == 0 {
        x / (y + 1)
    } else {
        ((y / 2) + x) / y
    }
}

#[inline]
pub fn WELS_MAX<T: PartialOrd + Copy>(x: T, y: T) -> T {
    if x > y {
        x
    } else {
        y
    }
}

// ============================================================================
// Data Structures
// ============================================================================

/// Temporal layer rate control state tracking (`TagRCTemporal`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRCTemporal {
    pub iMinBitsTl: i32,
    pub iMaxBitsTl: i32,
    pub iTlayerWeight: i32,
    pub iGopBitsDq: i32,
    pub iLinearCmplx: i64,
    pub iPFrameNum: i32,
    pub iFrameCmplxMean: i64,
    pub iMaxQp: i32,
    pub iMinQp: i32,
}

/// Spatial dependency layer rate control state machine (`TagWelsRc`).
///
/// **No longer `Copy`** — T6.H6. The five arrays below are owned, so a bit-copy of
/// this struct would be a second owner of five allocations. Nothing copied it: the
/// only by-value use in the tree was `Default`, and the three `let r = &*pWelsSvcRc`
/// bindings take a reference, not a copy.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SWelsSvcRc {
    pub iRcVaryPercentage: i32,
    pub iRcVaryRatio: i32,
    pub iInitialQp: i32,
    pub iBitRate: i64,
    pub iPreviousBitrate: i32,
    pub iPreviousGopSize: i32,
    pub fFrameRate: f64,
    pub iBitsPerFrame: i32,
    pub iMaxBitsPerFrame: i32,
    pub dPreviousFps: f64,

    pub iLastAllocatedBits: i32,
    pub iRemainingBits: i32,
    pub iBitsPerMb: i32,
    pub iTargetBits: i32,
    pub iCurrentBitsLevel: i32,

    pub iIdrNum: i32,
    pub iIntraComplexity: i64,
    pub iIntraMbCount: i32,
    pub iIntraComplxMean: i64,

    pub iTlOfFrames: [i8; VGOP_SIZE],
    pub iRemainingWeights: i32,
    pub iFrameDqBits: i32,

    pub bGomRC: bool,
    // **T6.H6 — the four GOM arrays and `pTemporalOverRc` below are owned.** They
    // were five raw pointers into one `CMemoryAlign` block `RcInitLayerMemory` cut
    // and `RcFreeLayerMemory` released; see [`rc_gom_complexity`] for what replaced
    // them and why they are five containers rather than one arena.
    pub pGomComplexity: Vec<f64>,
    pub pGomForegroundBlockNum: Vec<i32>,
    pub pCurrentFrameGomSad: Vec<i32>,
    pub pGomCost: Vec<i32>,

    pub bEnableGomQp: i32,
    pub iAverageFrameQp: i32,
    pub iMinFrameQp: i32,
    pub iMaxFrameQp: i32,
    pub iNumberMbFrame: i32,
    pub iNumberMbGom: i32,
    pub iGomSize: i32,

    pub iSkipFrameNum: i32,
    pub iFrameCodedInVGop: i32,
    pub iSkipFrameInVGop: i32,
    pub iGopNumberInVGop: i32,
    pub iGopIndexInVGop: i32,

    pub iSkipQpValue: i32,
    pub iQpRangeUpperInFrame: i32,
    pub iQpRangeLowerInFrame: i32,
    pub iMinQp: i32,
    pub iMaxQp: i32,
    pub iSkipBufferRatio: i32,

    pub iQStep: i32,
    pub iFrameDeltaQpUpper: i32,
    pub iFrameDeltaQpLower: i32,
    pub iLastCalculatedQScale: i32,

    pub iBufferSizeSkip: i32,
    pub iBufferFullnessSkip: i64,
    pub iBufferMaxBRFullness: [i64; TIME_WINDOW_TOTAL],
    pub iPredFrameBit: i32,
    pub bNeedShiftWindowCheck: [bool; TIME_WINDOW_TOTAL],
    pub iBufferSizePadding: i32,
    pub iBufferFullnessPadding: i32,
    pub iPaddingSize: i32,
    pub iPaddingBitrateStat: i32,
    pub bSkipFlag: bool,
    pub iContinualSkipFrames: i32,
    /// **T6.H6 — owned**; the head of the block the other four hung off. See
    /// [`rc_temporal_over`].
    pub pTemporalOverRc: Vec<SRCTemporal>,

    pub iAvgCost2Bits: i64,
    pub iCost2BitsIntra: i64,
    pub iBaseQp: i32,
    pub uiLastTimeStamp: i64,

    pub iActualBitRate: i32,
    pub fLatestFrameRate: f32,
}

impl Default for SWelsSvcRc {
    fn default() -> Self {
        Self {
            iRcVaryPercentage: 0,
            iRcVaryRatio: 0,
            iInitialQp: 0,
            iBitRate: 0,
            iPreviousBitrate: 0,
            iPreviousGopSize: 0,
            fFrameRate: 0.0,
            iBitsPerFrame: 0,
            iMaxBitsPerFrame: 0,
            dPreviousFps: 0.0,
            iLastAllocatedBits: 0,
            iRemainingBits: 0,
            iBitsPerMb: 0,
            iTargetBits: 0,
            iCurrentBitsLevel: BITS_NORMAL,
            iIdrNum: 0,
            iIntraComplexity: 0,
            iIntraMbCount: 0,
            iIntraComplxMean: 0,
            iTlOfFrames: [0; VGOP_SIZE],
            iRemainingWeights: 0,
            iFrameDqBits: 0,
            bGomRC: false,
            pGomComplexity: Vec::new(),
            pGomForegroundBlockNum: Vec::new(),
            pCurrentFrameGomSad: Vec::new(),
            pGomCost: Vec::new(),
            bEnableGomQp: 1,
            iAverageFrameQp: 0,
            iMinFrameQp: 0,
            iMaxFrameQp: 0,
            iNumberMbFrame: 0,
            iNumberMbGom: 0,
            iGomSize: 0,
            iSkipFrameNum: 0,
            iFrameCodedInVGop: 0,
            iSkipFrameInVGop: 0,
            iGopNumberInVGop: 0,
            iGopIndexInVGop: 0,
            iSkipQpValue: 0,
            iQpRangeUpperInFrame: 0,
            iQpRangeLowerInFrame: 0,
            iMinQp: 0,
            iMaxQp: 0,
            iSkipBufferRatio: SKIP_RATIO,
            iQStep: 0,
            iFrameDeltaQpUpper: 0,
            iFrameDeltaQpLower: 0,
            iLastCalculatedQScale: 0,
            iBufferSizeSkip: 0,
            iBufferFullnessSkip: 0,
            iBufferMaxBRFullness: [0; TIME_WINDOW_TOTAL],
            iPredFrameBit: 0,
            bNeedShiftWindowCheck: [false; TIME_WINDOW_TOTAL],
            iBufferSizePadding: 0,
            iBufferFullnessPadding: 0,
            iPaddingSize: 0,
            iPaddingBitrateStat: 0,
            bSkipFlag: false,
            iContinualSkipFrames: 0,
            pTemporalOverRc: Vec::new(),
            iAvgCost2Bits: 1,
            iCost2BitsIntra: 1,
            iBaseQp: 0,
            uiLastTimeStamp: 0,
            iActualBitRate: 0,
            fLatestFrameRate: 0.0,
        }
    }
}

// Slice-level RC statistics
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRCSlicing {
    pub iComplexityIndexSlice: i32,
    pub iCalculatedQpSlice: i32,
    pub iStartMbSlice: i32,
    pub iEndMbSlice: i32,
    pub iTotalQpSlice: i32,
    pub iTotalMbSlice: i32,
    pub iTargetBitsSlice: i32,
    pub iBsPosSlice: i32,
    pub iFrameBitsSlice: i32,
    pub iGomBitsSlice: i32,
    pub iGomTargetBits: i32,
}

// Slice header subset for rate control















// The nine `PWelsRC*Func` typedefs were here. T4b.1b folded every one of them into
// `SWelsRcFunc`'s single mode; `PGetBsPositionFunc` went at T4b.1, into
// `EntropyCoder`. Both tables were configuration, not dispatch.

/// `SWelsRcFunc` — `rc.h:132`. **Nine `Option<fn>` slots became one `RCMode`.**
///
/// `WelsRcInitFuncPointers` filled all nine from a single `match` on the mode, with
/// no arm assigning them independently, so the table's whole information content
/// was the mode it was built from. It stores that instead, and each former slot is
/// an `#[inline]` method whose `match` is the same `match` — one level later, where
/// the compiler can see the call.
///
/// **`eInstalledMode` is deliberately *not* `pSvcParam->iRCMode`, and the two can
/// legitimately differ.** `WelsEncoderParamAdjust`'s no-reset arm assigns
/// `pOldParam->iRCMode = pNewParam->iRCMode` and does **not** re-point the table —
/// upstream's own "Any else initialization/reset for rate control here?" sits a few
/// lines below it — so from that moment the encoder runs the *previous* mode's
/// callbacks until something re-inits. `SetOption(ENCODER_OPTION_RC_MODE)` is the
/// path that does re-point, and it is the only one. Reading the live `iRCMode` here
/// would silently *fix* that, which is a behaviour change on a live configuration
/// path (S6: parity, not repair). The lag is preserved by storing the installed
/// mode, and naming the field is what makes it visible rather than accidental.
///
/// Zero (`RC_QUALITY_MODE`, C++'s value 0) is a declared variant, so `mem::zeroed()`
/// construction of `SWelsFuncPtrList` stays sound (S21).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SWelsRcFunc {
    /// The mode the callbacks were **installed** for — see the type note.
    pub eInstalledMode: RCMode,
}

impl SWelsRcFunc {
    /// `pfWelsRcPictureInit`.
    ///
    /// # Safety
    /// As the callbacks: `pCtx` must be a live, initialized encoder context.
    #[inline]
    pub unsafe fn WelsRcPictureInit(self, pCtx: *mut sWelsEncCtx, uiTimeStamp: i64) {
        match self.eInstalledMode {
            RCMode::RC_OFF_MODE => WelsRcPictureInitDisable(pCtx, uiTimeStamp),
            RCMode::RC_BUFFERBASED_MODE => WelRcPictureInitBufferBasedQp(pCtx, uiTimeStamp),
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_TIMESTAMP_MODE
            | RCMode::RC_QUALITY_MODE => WelsRcPictureInitGom(pCtx, uiTimeStamp),
        }
    }

    /// `pfWelsRcPicDelayJudge`. Installed by `RC_TIMESTAMP_MODE` alone; every other
    /// mode left the slot `None`, and every call site guarded on that, so the other
    /// arm is empty.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsRcPicDelayJudge(self, pCtx: *mut sWelsEncCtx, uiTimeStamp: i64, iDidIdx: i32) {
        if self.eInstalledMode == RCMode::RC_TIMESTAMP_MODE {
            WelsRcFrameDelayJudgeTimeStamp(pCtx, uiTimeStamp, iDidIdx);
        }
    }

    /// `pfWelsRcPictureInfoUpdate`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsRcPictureInfoUpdate(self, pCtx: *mut sWelsEncCtx, iLayerSize: i32) {
        match self.eInstalledMode {
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE => {
                WelsRcPictureInfoUpdateDisable(pCtx, iLayerSize)
            }
            RCMode::RC_TIMESTAMP_MODE => WelsRcPictureInfoUpdateGomTimeStamp(pCtx, iLayerSize),
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_QUALITY_MODE => WelsRcPictureInfoUpdateGom(pCtx, iLayerSize),
        }
    }

    /// `pfWelsRcMbInit`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit); `pCurMb` and
    /// `pSlice` must be live.
    #[inline]
    pub unsafe fn WelsRcMbInit(self, pCtx: *mut sWelsEncCtx, pCurMb: &mut SMB, pSlice: *mut SSlice) {
        match self.eInstalledMode {
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE => {
                WelsRcMbInitDisable(pCtx, pCurMb, pSlice)
            }
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_TIMESTAMP_MODE
            | RCMode::RC_QUALITY_MODE => WelsRcMbInitGom(pCtx, pCurMb, pSlice),
        }
    }

    /// `pfWelsRcMbInfoUpdate`.
    ///
    /// # Safety
    /// As [`WelsRcMbInit`](SWelsRcFunc::WelsRcMbInit).
    #[inline]
    pub unsafe fn WelsRcMbInfoUpdate(
        self,
        pCtx: *mut sWelsEncCtx,
        pCurMb: &mut SMB,
        iCostLuma: i32,
        pSlice: *mut SSlice,
    ) {
        match self.eInstalledMode {
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE => {
                WelsRcMbInfoUpdateDisable(pCtx, pCurMb, iCostLuma, pSlice)
            }
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_TIMESTAMP_MODE
            | RCMode::RC_QUALITY_MODE => WelsRcMbInfoUpdateGom(pCtx, pCurMb, iCostLuma, pSlice),
        }
    }

    /// `pfWelsCheckSkipBasedMaxbr`. Absent for `RC_OFF`, `RC_BUFFERBASED` and
    /// `RC_TIMESTAMP`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsCheckSkipBasedMaxbr(
        self,
        pCtx: *mut sWelsEncCtx,
        uiTimeStamp: i64,
        iDidIdx: i32,
    ) {
        match self.eInstalledMode {
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_QUALITY_MODE => CheckFrameSkipBasedMaxbr(pCtx, uiTimeStamp, iDidIdx),
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE | RCMode::RC_TIMESTAMP_MODE => {}
        }
    }

    /// `pfWelsUpdateBufferWhenSkip`. Absent for `RC_OFF`, `RC_BUFFERBASED` and
    /// `RC_TIMESTAMP`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsUpdateBufferWhenSkip(self, pCtx: *mut sWelsEncCtx, iSpatialNum: i32) {
        match self.eInstalledMode {
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_QUALITY_MODE => UpdateBufferWhenFrameSkipped(pCtx, iSpatialNum),
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE | RCMode::RC_TIMESTAMP_MODE => {}
        }
    }

    /// `pfWelsUpdateMaxBrWindowStatus`. Absent for `RC_OFF`, `RC_BUFFERBASED` and
    /// `RC_TIMESTAMP`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsUpdateMaxBrWindowStatus(
        self,
        pCtx: *mut sWelsEncCtx,
        iSpatialNum: i32,
        uiTimeStamp: i64,
    ) {
        match self.eInstalledMode {
            RCMode::RC_BITRATE_MODE
            | RCMode::RC_BITRATE_MODE_POST_SKIP
            | RCMode::RC_QUALITY_MODE => {
                UpdateMaxBrCheckWindowStatus(pCtx, iSpatialNum, uiTimeStamp)
            }
            RCMode::RC_OFF_MODE | RCMode::RC_BUFFERBASED_MODE | RCMode::RC_TIMESTAMP_MODE => {}
        }
    }

    /// `pfWelsRcPostFrameSkipping` — the one slot with a return value, and the one
    /// whose absence a caller reads: `if let Some(f) = …` guarded a whole
    /// skip-and-return path. **`false` is what "the slot was `None`" meant**, so the
    /// empty arms return it. Installed by the two bitrate modes only —
    /// `RC_QUALITY_MODE` is the arm that sets the other three and leaves this one
    /// `None`.
    ///
    /// # Safety
    /// As [`WelsRcPictureInit`](SWelsRcFunc::WelsRcPictureInit).
    #[inline]
    pub unsafe fn WelsRcPostFrameSkipping(
        self,
        pCtx: *mut sWelsEncCtx,
        iDid: i32,
        uiTimeStamp: i64,
    ) -> bool {
        match self.eInstalledMode {
            RCMode::RC_BITRATE_MODE | RCMode::RC_BITRATE_MODE_POST_SKIP => {
                WelsRcPostFrameSkipping(pCtx, iDid, uiTimeStamp)
            }
            RCMode::RC_OFF_MODE
            | RCMode::RC_BUFFERBASED_MODE
            | RCMode::RC_TIMESTAMP_MODE
            | RCMode::RC_QUALITY_MODE => false,
        }
    }
}


/// Central encoder context required by rate control (`TagWelsEncCtx`).

// ============================================================================
// Core Rate Control Functions
// ============================================================================

/// Builds a spatial layer's rate-control arrays — **T6.H6.**
///
/// The C++ (and this port until now) took **one** `CMemoryAlign` block and cut five
/// regions out of it: `SRCTemporal[kiMaxTl]`, then three or four GOM-sized arrays.
/// Here it is five owned containers, and unlike `SStrideTables` — the session's other
/// arena — that is the right shape rather than a shortcut: **no two of these regions
/// are ever named by the same pointer, and nothing walks from one into the next.**
/// The single block bought the C++ one `malloc` per spatial layer at init; the
/// aliasing that made `SStrideTables` an arena is simply absent here.
///
/// `pMA` is gone with the block, and with it the `alloc_zeroed` fallback the port had
/// grown for the null-`pMA` test path — one divergence fewer between the two.
///
/// The C++ takes the block with `WelsMalloc` (uninitialized) and every consumer
/// either writes before reading or is guarded by `bGomRC`; the containers are
/// zero-filled, which the port's own fallback path already did.
pub unsafe fn RcInitLayerMemory(pWelsSvcRc: *mut SWelsSvcRc, kiMaxTl: i32) {
    if pWelsSvcRc.is_null() {
        return;
    }
    let kiGomSize = (*pWelsSvcRc).iGomSize.max(0) as usize;
    (*pWelsSvcRc).pTemporalOverRc = vec![SRCTemporal::default(); kiMaxTl.max(0) as usize];
    (*pWelsSvcRc).pGomComplexity = vec![0.0f64; kiGomSize];
    (*pWelsSvcRc).pGomForegroundBlockNum = vec![0i32; kiGomSize];
    (*pWelsSvcRc).pCurrentFrameGomSad = vec![0i32; kiGomSize];
    (*pWelsSvcRc).pGomCost = vec![0i32; kiGomSize];
}

/// The **root** of a layer's `pTemporalOverRc` — S40's spelling, as everywhere in
/// this family: `Vec::as_mut_ptr` reads the header, so repeated calls are siblings
/// and a caller may hold one cursor across another. Empty answers the null the raw
/// field held before `RcInitLayerMemory` ran, which every consumer's guard reads.
///
/// # Safety
/// `pRc` must point to a live `SWelsSvcRc`.
#[inline]
pub unsafe fn rc_temporal_over(pRc: *mut SWelsSvcRc) -> *mut SRCTemporal {
    let v: &mut Vec<SRCTemporal> = &mut (*pRc).pTemporalOverRc;
    if v.is_empty() {
        return std::ptr::null_mut();
    }
    v.as_mut_ptr()
}

/// The **root** of a layer's `pGomComplexity` — see [`rc_temporal_over`].
///
/// # Safety
/// As [`rc_temporal_over`].
#[inline]
pub unsafe fn rc_gom_complexity(pRc: *mut SWelsSvcRc) -> *mut f64 {
    let v: &mut Vec<f64> = &mut (*pRc).pGomComplexity;
    if v.is_empty() {
        return std::ptr::null_mut();
    }
    v.as_mut_ptr()
}

/// The **root** of a layer's `pGomForegroundBlockNum` — see [`rc_temporal_over`].
///
/// # Safety
/// As [`rc_temporal_over`].
#[inline]
pub unsafe fn rc_gom_fg_blocks(pRc: *mut SWelsSvcRc) -> *mut i32 {
    let v: &mut Vec<i32> = &mut (*pRc).pGomForegroundBlockNum;
    if v.is_empty() {
        return std::ptr::null_mut();
    }
    v.as_mut_ptr()
}

/// The **root** of a layer's `pCurrentFrameGomSad` — see [`rc_temporal_over`].
///
/// # Safety
/// As [`rc_temporal_over`].
#[inline]
pub unsafe fn rc_gom_sad(pRc: *mut SWelsSvcRc) -> *mut i32 {
    let v: &mut Vec<i32> = &mut (*pRc).pCurrentFrameGomSad;
    if v.is_empty() {
        return std::ptr::null_mut();
    }
    v.as_mut_ptr()
}

/// The **root** of a layer's `pGomCost` — see [`rc_temporal_over`].
///
/// # Safety
/// As [`rc_temporal_over`].
#[inline]
pub unsafe fn rc_gom_cost(pRc: *mut SWelsSvcRc) -> *mut i32 {
    let v: &mut Vec<i32> = &mut (*pRc).pGomCost;
    if v.is_empty() {
        return std::ptr::null_mut();
    }
    v.as_mut_ptr()
}

/// Converts a quantization parameter ($QP$) to its scaled quantization step size ($Q_{\text{step}}$).
#[inline]
pub fn RcConvertQp2QStep(iQP: i32) -> i32 {
    let qp = WELS_CLIP3(iQP, 0, 51);
    g_kiQpToQstepTable[qp as usize]
}

/// Inversely converts a quantization step size ($Q_{\text{step}}$) to integer $QP$.
#[inline]
pub fn RcConvertQStep2Qp(iQpStep: i32) -> i32 {
    if iQpStep <= g_kiQpToQstepTable[0] {
        return 0;
    }
    let val = 6.0 * (iQpStep as f64 / INT_MULTIPLY as f64).ln() / 2.0f64.ln() + 4.0;
    (val + 0.5) as i32
}

/// Initializes sequence-level rate control parameters for all spatial layers.
pub unsafe fn RcInitSequenceParameter(pEncCtx: *mut sWelsEncCtx) {
    let pSvcParam = ctx_param(pEncCtx);
    let spatial_layer_num = (*pSvcParam).iSpatialLayerNum;

    for j in 0..spatial_layer_num as usize {
        let pWelsSvcRc = ctx_rc_at(pEncCtx, j);
        let pDLayerParam = &(*pSvcParam).sSpatialLayers[j];
        let iMbWidth = pDLayerParam.iVideoWidth >> 4;
        (*pWelsSvcRc).iNumberMbFrame = iMbWidth * (pDLayerParam.iVideoHeight >> 4);

        (*pWelsSvcRc).iRcVaryPercentage = (*pSvcParam).iBitsVaryPercentage;
        (*pWelsSvcRc).iRcVaryRatio = (*pWelsSvcRc).iRcVaryPercentage;

        (*pWelsSvcRc).iBufferFullnessSkip = 0;
        (*pWelsSvcRc).uiLastTimeStamp = 0;
        (*pWelsSvcRc).iCost2BitsIntra = 1;
        (*pWelsSvcRc).iAvgCost2Bits = 1;
        (*pWelsSvcRc).iSkipBufferRatio = SKIP_RATIO;
        (*pWelsSvcRc).iContinualSkipFrames = 0;

        (*pWelsSvcRc).iQpRangeUpperInFrame = (QP_RANGE_UPPER_MODE1 * MAX_BITS_VARY_PERCENTAGE
            - ((QP_RANGE_UPPER_MODE1 - QP_RANGE_MODE0) * (*pWelsSvcRc).iRcVaryRatio))
            / MAX_BITS_VARY_PERCENTAGE;
        (*pWelsSvcRc).iQpRangeLowerInFrame = (QP_RANGE_LOWER_MODE1 * MAX_BITS_VARY_PERCENTAGE
            - ((QP_RANGE_LOWER_MODE1 - QP_RANGE_MODE0) * (*pWelsSvcRc).iRcVaryRatio))
            / MAX_BITS_VARY_PERCENTAGE;

        let mut iGomRowMode0: i32;
        let iGomRowMode1: i32;
        if iMbWidth <= MB_WIDTH_THRESHOLD_90P {
            (*pWelsSvcRc).iSkipQpValue = SKIP_QP_90P;
            iGomRowMode0 = GOM_ROW_MODE0_90P;
            iGomRowMode1 = GOM_ROW_MODE1_90P;
        } else if iMbWidth <= MB_WIDTH_THRESHOLD_180P {
            (*pWelsSvcRc).iSkipQpValue = SKIP_QP_180P;
            iGomRowMode0 = GOM_ROW_MODE0_180P;
            iGomRowMode1 = GOM_ROW_MODE1_180P;
        } else if iMbWidth <= MB_WIDTH_THRESHOLD_360P {
            (*pWelsSvcRc).iSkipQpValue = SKIP_QP_360P;
            iGomRowMode0 = GOM_ROW_MODE0_360P;
            iGomRowMode1 = GOM_ROW_MODE1_360P;
        } else {
            (*pWelsSvcRc).iSkipQpValue = SKIP_QP_720P;
            iGomRowMode0 = GOM_ROW_MODE0_720P;
            iGomRowMode1 = GOM_ROW_MODE1_720P;
        }

        iGomRowMode0 = iGomRowMode1
            + ((iGomRowMode0 - iGomRowMode1) * (*pWelsSvcRc).iRcVaryRatio
                / MAX_BITS_VARY_PERCENTAGE);

        (*pWelsSvcRc).iNumberMbGom = iMbWidth * iGomRowMode0;
        (*pWelsSvcRc).iMinQp = (*pSvcParam).iMinQp;
        (*pWelsSvcRc).iMaxQp = (*pSvcParam).iMaxQp;

        (*pWelsSvcRc).iFrameDeltaQpUpper = LAST_FRAME_QP_RANGE_UPPER_MODE1
            - ((LAST_FRAME_QP_RANGE_UPPER_MODE1 - LAST_FRAME_QP_RANGE_UPPER_MODE0)
                * (*pWelsSvcRc).iRcVaryRatio
                / MAX_BITS_VARY_PERCENTAGE);
        (*pWelsSvcRc).iFrameDeltaQpLower = LAST_FRAME_QP_RANGE_LOWER_MODE1
            - ((LAST_FRAME_QP_RANGE_LOWER_MODE1 - LAST_FRAME_QP_RANGE_LOWER_MODE0)
                * (*pWelsSvcRc).iRcVaryRatio
                / MAX_BITS_VARY_PERCENTAGE);

        (*pWelsSvcRc).iSkipFrameNum = 0;
        (*pWelsSvcRc).iGomSize = ((*pWelsSvcRc).iNumberMbFrame + (*pWelsSvcRc).iNumberMbGom - 1)
            / (*pWelsSvcRc).iNumberMbGom;
        (*pWelsSvcRc).bEnableGomQp = 1;

        RcInitLayerMemory(
            pWelsSvcRc,
            1 + (*pSvcParam).sDependencyLayers[j].iHighestTemporalId as i32,
        );

        let slice_mode = pDLayerParam.sSliceArgument.uiSliceMode as i32;
        let bMultiSliceMode =
            slice_mode == SM_RASTER_SLICE || slice_mode == SM_SIZELIMITED_SLICE;
        if bMultiSliceMode {
            (*pWelsSvcRc).iNumberMbGom = (*pWelsSvcRc).iNumberMbFrame;
        }
    }
}

/// Initializes temporal layer weighting matrices for Virtual GOP bit allocation.
pub unsafe fn RcInitTlWeight(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pTOverRc = rc_temporal_over(pWelsSvcRc);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sDependencyLayers[did];
    let kiDecompositionStages = pDLayerParam.iDecompositionStages as usize;
    let kiHighestTid = pDLayerParam.iHighestTemporalId;

    let iWeightArray: [[i32; 4]; 4] = [
        [2000, 0, 0, 0],
        [1200, 800, 0, 0],
        [800, 600, 300, 0],
        [500, 300, 250, 175],
    ];
    let kiGopSize = 1 << kiDecompositionStages;

    let mut n: i32 = 0;
    while n <= kiHighestTid as i32 {
        let t_rc = &mut *pTOverRc.add(n as usize);
        t_rc.iTlayerWeight = iWeightArray[kiDecompositionStages][n as usize];
        t_rc.iMinQp = (*pWelsSvcRc).iMinQp + (n << 1);
        t_rc.iMinQp = WELS_CLIP3(t_rc.iMinQp, 0, 51);
        t_rc.iMaxQp = (*pWelsSvcRc).iMaxQp + (n << 1);
        t_rc.iMaxQp = WELS_CLIP3(t_rc.iMaxQp, t_rc.iMinQp, 51);
        n += 1;
    }

    let mut n = 0;
    while n < VGOP_SIZE as i32 {
        (*pWelsSvcRc).iTlOfFrames[n as usize] = 0;
        for i in 1..=kiDecompositionStages as i32 {
            let step = kiGopSize >> (i - 1);
            let mut k = 1 << (kiDecompositionStages as i32 - i);
            while k < kiGopSize {
                (*pWelsSvcRc).iTlOfFrames[(k + n) as usize] = i as i8;
                k += step;
            }
        }
        n += kiGopSize;
    }
    (*pWelsSvcRc).iPreviousGopSize = kiGopSize;
    (*pWelsSvcRc).iGopNumberInVGop = VGOP_SIZE as i32 / kiGopSize;
}

/// Updates frame and temporal bit quotas whenever user bitrate or framerate changes.
pub unsafe fn RcUpdateBitrateFps(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pTOverRc = rc_temporal_over(pWelsSvcRc);

    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
    let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[did];
    let kiGopSize = 1 << pDLayerParamInternal.iDecompositionStages;
    let kiHighestTid = pDLayerParamInternal.iHighestTemporalId;

    let input_iBitsPerFrame = if pDLayerParamInternal.fOutputFrameRate > EPSN as f32 {
        WELS_ROUND(pDLayerParam.iSpatialBitrate as f64 / pDLayerParamInternal.fOutputFrameRate as f64)
    } else {
        0
    };
    let kiGopBits = input_iBitsPerFrame as i64 * kiGopSize as i64;

    (*pWelsSvcRc).iBitRate = pDLayerParam.iSpatialBitrate as i64;
    (*pWelsSvcRc).fFrameRate = pDLayerParamInternal.fOutputFrameRate as f64;

    let iTargetVaryRange = (MAX_BITS_VARY_PERCENTAGE - (*pWelsSvcRc).iRcVaryRatio) >> 1;
    let iMinBitsRatio = MAX_BITS_VARY_PERCENTAGE - iTargetVaryRange;
    let iMaxBitsRatio = MAX_BITS_VARY_PERCENTAGE_x3d2;

    for i in 0..=kiHighestTid {
        let t_rc = &mut *pTOverRc.add(i as usize);
        let kdConstraintBits = kiGopBits * t_rc.iTlayerWeight as i64;
        t_rc.iMinBitsTl = WELS_DIV_ROUND64(
            kdConstraintBits * iMinBitsRatio as i64,
            (MAX_BITS_VARY_PERCENTAGE * WEIGHT_MULTIPLY) as i64,
        ) as i32;
        t_rc.iMaxBitsTl = WELS_DIV_ROUND64(
            kdConstraintBits * iMaxBitsRatio as i64,
            (MAX_BITS_VARY_PERCENTAGE * WEIGHT_MULTIPLY) as i64,
        ) as i32;
    }

    (*pWelsSvcRc).iBufferSizeSkip = WELS_DIV_ROUND(
        ((*pWelsSvcRc).iBitRate as i32).wrapping_mul((*pWelsSvcRc).iSkipBufferRatio),
        INT_MULTIPLY,
    );
    (*pWelsSvcRc).iBufferSizePadding = WELS_DIV_ROUND(
        ((*pWelsSvcRc).iBitRate as i32).wrapping_mul(PADDING_BUFFER_RATIO),
        INT_MULTIPLY,
    );

    if (*pWelsSvcRc).iBitsPerFrame > REMAIN_BITS_TH {
        (*pWelsSvcRc).iRemainingBits = WELS_DIV_ROUND64(
            (*pWelsSvcRc).iRemainingBits as i64 * input_iBitsPerFrame as i64,
            (*pWelsSvcRc).iBitsPerFrame as i64,
        ) as i32;
    }
    (*pWelsSvcRc).iBitsPerFrame = input_iBitsPerFrame;
    (*pWelsSvcRc).iMaxBitsPerFrame = if pDLayerParamInternal.fOutputFrameRate > EPSN as f32 {
        WELS_ROUND(pDLayerParam.iMaxSpatialBitrate as f64 / pDLayerParamInternal.fOutputFrameRate as f64)
    } else {
        0
    };
}

/// Resets the bit budget accumulator at the start of a Virtual GOP.
pub unsafe fn RcInitVGop(pEncCtx: *mut sWelsEncCtx) {
    let kiDid = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, kiDid);
    let pTOverRc = rc_temporal_over(pWelsSvcRc);
    let kiHighestTid = (*ctx_param(pEncCtx)).sDependencyLayers[kiDid].iHighestTemporalId;
    let fix_rc_overshoot = (*ctx_param(pEncCtx)).bFixRCOverShoot;

    if fix_rc_overshoot {
        let iLeftInVGop = (*pWelsSvcRc).iGopNumberInVGop - (*pWelsSvcRc).iGopIndexInVGop;
        if (*pWelsSvcRc).iGopNumberInVGop != 0 {
            (*pWelsSvcRc).iRemainingBits -=
                iLeftInVGop * ((*pWelsSvcRc).iLastAllocatedBits / (*pWelsSvcRc).iGopNumberInVGop);
        }
    }

    if fix_rc_overshoot && (*pWelsSvcRc).iRemainingBits < 0 {
        (*pWelsSvcRc).iRemainingBits += VGOP_SIZE as i32 * (*pWelsSvcRc).iBitsPerFrame;
    } else {
        (*pWelsSvcRc).iRemainingBits = VGOP_SIZE as i32 * (*pWelsSvcRc).iBitsPerFrame;
    }

    if fix_rc_overshoot {
        (*pWelsSvcRc).iLastAllocatedBits = (*pWelsSvcRc).iRemainingBits;
    }
    (*pWelsSvcRc).iRemainingWeights = (*pWelsSvcRc).iGopNumberInVGop * WEIGHT_MULTIPLY;
    (*pWelsSvcRc).iFrameCodedInVGop = 0;
    (*pWelsSvcRc).iGopIndexInVGop = 0;

    for i in 0..=kiHighestTid {
        (*pTOverRc.add(i as usize)).iGopBitsDq = 0;
    }
    (*pWelsSvcRc).iSkipFrameInVGop = 0;
}

/// Full reset of the rate control state machine upon encoder initialization or IDR insertion.
pub unsafe fn RcInitRefreshParameter(pEncCtx: *mut sWelsEncCtx) {
    let kiDid = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, kiDid);
    let pTOverRc = rc_temporal_over(pWelsSvcRc);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[kiDid];
    let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[kiDid];
    let kiHighestTid = pDLayerParamInternal.iHighestTemporalId;
    let fix_rc_overshoot = (*ctx_param(pEncCtx)).bFixRCOverShoot;

    (*pWelsSvcRc).iIntraComplexity = 0;
    (*pWelsSvcRc).iIntraMbCount = 0;
    (*pWelsSvcRc).iIntraComplxMean = 0;

    for i in 0..=kiHighestTid {
        let t_rc = &mut *pTOverRc.add(i as usize);
        t_rc.iPFrameNum = 0;
        t_rc.iLinearCmplx = 0;
        t_rc.iFrameCmplxMean = 0;
    }

    (*pWelsSvcRc).iBufferFullnessSkip = 0;
    (*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] = 0;
    (*pWelsSvcRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] = 0;
    (*pWelsSvcRc).iPredFrameBit = 0;
    (*pWelsSvcRc).iBufferFullnessPadding = 0;

    (*pWelsSvcRc).iGopIndexInVGop = 0;
    if fix_rc_overshoot {
        (*pWelsSvcRc).iLastAllocatedBits = 0;
    }
    (*pWelsSvcRc).iRemainingBits = 0;
    (*pWelsSvcRc).iBitsPerFrame = 0;

    (*pWelsSvcRc).iPreviousBitrate = pDLayerParam.iSpatialBitrate;
    (*pWelsSvcRc).dPreviousFps = pDLayerParamInternal.fOutputFrameRate as f64;

    // T6.H6: `write_bytes` through the raw cursor became a slice fill — the array is
    // owned, so its length is the bound rather than `iGomSize` restated.
    (*pWelsSvcRc).pCurrentFrameGomSad.fill(0);

    RcInitTlWeight(pEncCtx);
    RcUpdateBitrateFps(pEncCtx);
    RcInitVGop(pEncCtx);
}

/// Checks whether user bitrate or framerate settings have changed at runtime.
pub unsafe fn RcJudgeBitrateFpsUpdate(pEncCtx: *mut sWelsEncCtx) -> bool {
    let iCurDid = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, iCurDid);
    let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[iCurDid];
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[iCurDid];

    let diff = (*pWelsSvcRc).dPreviousFps - pDLayerParamInternal.fOutputFrameRate as f64;
    if (*pWelsSvcRc).iPreviousBitrate != pDLayerParam.iSpatialBitrate || diff > EPSN || diff < -EPSN {
        (*pWelsSvcRc).iPreviousBitrate = pDLayerParam.iSpatialBitrate;
        (*pWelsSvcRc).dPreviousFps = pDLayerParamInternal.fOutputFrameRate as f64;
        true
    } else {
        false
    }
}

/// Updates base temporal layer boundaries (`uiTemporalId == 0`).
pub unsafe fn RcUpdateTemporalZero(pEncCtx: *mut sWelsEncCtx) {
    let kiDid = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, kiDid);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sDependencyLayers[kiDid];
    let kiGopSize = 1 << pDLayerParam.iDecompositionStages;

    if (*pWelsSvcRc).iPreviousGopSize != kiGopSize {
        RcInitTlWeight(pEncCtx);
        RcInitVGop(pEncCtx);
    } else if (*pWelsSvcRc).iGopIndexInVGop == (*pWelsSvcRc).iGopNumberInVGop
        || (*pEncCtx).eSliceType as i32 == I_SLICE
    {
        RcInitVGop(pEncCtx);
    }
    (*pWelsSvcRc).iGopIndexInVGop += 1;
}

/// Calculates the quantization parameter for IDR keyframes.
pub unsafe fn RcCalculateIdrQp(pEncCtx: *mut sWelsEncCtx) {
    let dBpp: f64;
    let dBppArray: [[f64; 4]; 4] = [
        [0.25, 0.5, 0.75, 1.0],
        [0.1, 0.2, 0.3, 0.4],
        [0.03, 0.05, 0.09, 0.13],
        [0.01, 0.03, 0.06, 0.1],
    ];
    let dInitialQPArray: [[i32; 5]; 4] = [
        [34, 28, 26, 24, 22],
        [36, 30, 28, 26, 24],
        [36, 32, 30, 28, 26],
        [36, 34, 32, 30, 28],
    ];
    let iQpRangeArray: [[i32; 2]; 5] = [[40, 28], [37, 25], [36, 24], [35, 23], [34, 22]];

    let mut iFrameComplexity = (*ctx_vaa(pEncCtx)).sComplexityAnalysisParam.iFrameComplexity;
    let fix_rc_overshoot = (*ctx_param(pEncCtx)).bFixRCOverShoot;
    if (*ctx_param(pEncCtx)).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
        iFrameComplexity = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    }

    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
    let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[did];

    if pDLayerParamInternal.fOutputFrameRate > EPSN as f32
        && pDLayerParam.iVideoWidth != 0
        && pDLayerParam.iVideoHeight != 0
    {
        dBpp = pDLayerParam.iSpatialBitrate as f64
            / (pDLayerParamInternal.fOutputFrameRate as f64
                * pDLayerParam.iVideoWidth as f64
                * pDLayerParam.iVideoHeight as f64);
    } else {
        dBpp = 0.1;
    }

    let area = pDLayerParam.iVideoWidth * pDLayerParam.iVideoHeight;
    let iBppIndex = if area <= 28800 {
        0
    } else if area <= 115200 {
        1
    } else if area <= 460800 {
        2
    } else {
        3
    };

    let start_i = if fix_rc_overshoot { 0 } else { 1 };
    let mut i = start_i;
    while i < 4 {
        if dBpp <= dBppArray[iBppIndex][i] {
            break;
        }
        i += 1;
    }

    let mut iMaxQp = iQpRangeArray[i][0];
    let mut iMinQp = iQpRangeArray[i][1];
    iMinQp = WELS_CLIP3(iMinQp, (*pWelsSvcRc).iMinQp, (*pWelsSvcRc).iMaxQp);
    iMaxQp = WELS_CLIP3(iMaxQp, (*pWelsSvcRc).iMinQp, (*pWelsSvcRc).iMaxQp);

    if (*pWelsSvcRc).iIdrNum == 0 {
        (*pWelsSvcRc).iInitialQp = dInitialQPArray[iBppIndex][i];
    } else {
        if (*pWelsSvcRc).iNumberMbFrame != (*pWelsSvcRc).iIntraMbCount
            && (*pWelsSvcRc).iIntraMbCount != 0
        {
            (*pWelsSvcRc).iIntraComplexity = (*pWelsSvcRc).iIntraComplexity
                * (*pWelsSvcRc).iNumberMbFrame as i64
                / (*pWelsSvcRc).iIntraMbCount as i64;
        }

        let mut iCmplxRatio = WELS_DIV_ROUND64(
            iFrameComplexity * INT_MULTIPLY as i64,
            (*pWelsSvcRc).iIntraComplxMean,
        );
        iCmplxRatio = WELS_CLIP3(
            iCmplxRatio,
            (INT_MULTIPLY - FRAME_CMPLX_RATIO_RANGE) as i64,
            (INT_MULTIPLY + FRAME_CMPLX_RATIO_RANGE) as i64,
        );

        let denom = (*pWelsSvcRc).iTargetBits as i64 * INT_MULTIPLY as i64;
        (*pWelsSvcRc).iQStep = WELS_DIV_ROUND64((*pWelsSvcRc).iIntraComplexity * iCmplxRatio, denom) as i32;
        (*pWelsSvcRc).iInitialQp = RcConvertQStep2Qp((*pWelsSvcRc).iQStep);
    }

    (*pWelsSvcRc).iInitialQp = WELS_CLIP3((*pWelsSvcRc).iInitialQp, iMinQp, iMaxQp);
    (*pEncCtx).iGlobalQp = (*pWelsSvcRc).iInitialQp;
    (*pWelsSvcRc).iQStep = RcConvertQp2QStep((*pEncCtx).iGlobalQp);
    (*pWelsSvcRc).iLastCalculatedQScale = (*pEncCtx).iGlobalQp;
    (*pWelsSvcRc).iMinFrameQp =
        WELS_CLIP3((*pEncCtx).iGlobalQp - DELTA_QP_BGD_THD, iMinQp, iMaxQp);
    (*pWelsSvcRc).iMaxFrameQp =
        WELS_CLIP3((*pEncCtx).iGlobalQp + DELTA_QP_BGD_THD, iMinQp, iMaxQp);
}

/// Calculates the base quantization parameter for Inter P-frames.
pub unsafe fn RcCalculatePictureQp(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let iTl = (*pEncCtx).uiTemporalId as usize;
    let pTOverRc = rc_temporal_over(pWelsSvcRc).add(iTl);

    let mut iLumaQp: i32;
    let mut iDeltaQpTemporal: i32 = 0;
    let mut iFrameComplexity = (*ctx_vaa(pEncCtx)).sComplexityAnalysisParam.iFrameComplexity;
    if (*ctx_param(pEncCtx)).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
        iFrameComplexity = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    }

    if (*pTOverRc).iPFrameNum == 0 {
        iLumaQp = (*pWelsSvcRc).iInitialQp;
    } else if (*pWelsSvcRc).iCurrentBitsLevel == BITS_EXCEEDED {
        iLumaQp = (*pWelsSvcRc).iLastCalculatedQScale + DELTA_QP_BGD_THD;

        let mut iLastIdxCodecInVGop = (*pWelsSvcRc).iFrameCodedInVGop - 1;
        if iLastIdxCodecInVGop < 0 {
            iLastIdxCodecInVGop += VGOP_SIZE as i32;
        }
        let iTlLast = (*pWelsSvcRc).iTlOfFrames[iLastIdxCodecInVGop as usize] as i32;
        iDeltaQpTemporal = iTl as i32 - iTlLast;
        if iTlLast == 0 && iTl > 0 {
            iDeltaQpTemporal += 1;
        } else if iTl == 0 && iTlLast > 0 {
            iDeltaQpTemporal -= 1;
        }
    } else {
        let mut iCmplxRatio = WELS_DIV_ROUND64(
            iFrameComplexity * INT_MULTIPLY as i64,
            (*pTOverRc).iFrameCmplxMean,
        );
        iCmplxRatio = WELS_CLIP3(
            iCmplxRatio,
            (INT_MULTIPLY - FRAME_CMPLX_RATIO_RANGE) as i64,
            (INT_MULTIPLY + FRAME_CMPLX_RATIO_RANGE) as i64,
        );

        let denom = (*pWelsSvcRc).iTargetBits as i64 * INT_MULTIPLY as i64;
        (*pWelsSvcRc).iQStep = WELS_DIV_ROUND64((*pTOverRc).iLinearCmplx * iCmplxRatio, denom) as i32;
        iLumaQp = RcConvertQStep2Qp((*pWelsSvcRc).iQStep);

        let mut iLastIdxCodecInVGop = (*pWelsSvcRc).iFrameCodedInVGop - 1;
        if iLastIdxCodecInVGop < 0 {
            iLastIdxCodecInVGop += VGOP_SIZE as i32;
        }
        let iTlLast = (*pWelsSvcRc).iTlOfFrames[iLastIdxCodecInVGop as usize] as i32;
        iDeltaQpTemporal = iTl as i32 - iTlLast;
        if iTlLast == 0 && iTl > 0 {
            iDeltaQpTemporal += 1;
        } else if iTl == 0 && iTlLast > 0 {
            iDeltaQpTemporal -= 1;
        }
    }

    (*pWelsSvcRc).iMinFrameQp = WELS_CLIP3(
        (*pWelsSvcRc).iLastCalculatedQScale - (*pWelsSvcRc).iFrameDeltaQpLower + iDeltaQpTemporal,
        (*pTOverRc).iMinQp,
        (*pTOverRc).iMaxQp,
    );
    (*pWelsSvcRc).iMaxFrameQp = WELS_CLIP3(
        (*pWelsSvcRc).iLastCalculatedQScale + (*pWelsSvcRc).iFrameDeltaQpUpper + iDeltaQpTemporal,
        (*pTOverRc).iMinQp,
        (*pTOverRc).iMaxQp,
    );

    iLumaQp = WELS_CLIP3(iLumaQp, (*pWelsSvcRc).iMinFrameQp, (*pWelsSvcRc).iMaxFrameQp);

    if (*ctx_param(pEncCtx)).bEnableAdaptiveQuant {
        let delta_offset = (*ctx_vaa(pEncCtx))
            .sAdaptiveQuantParam
            .iAverMotionTextureIndexToDeltaQp;
        iLumaQp = WELS_DIV_ROUND(iLumaQp * INT_MULTIPLY - delta_offset, INT_MULTIPLY);
        iLumaQp = WELS_CLIP3(iLumaQp, (*pWelsSvcRc).iMinFrameQp, (*pWelsSvcRc).iMaxFrameQp);
    }

    (*pWelsSvcRc).iQStep = RcConvertQp2QStep(iLumaQp);
    (*pWelsSvcRc).iLastCalculatedQScale = iLumaQp;
    (*pEncCtx).iGlobalQp = iLumaQp;
}

/// Initializes slice-level GOM rate control parameters.
pub unsafe fn GomRCInitForOneSlice(pSlice: *mut SSlice, kiBitsPerMb: i32) {
    if pSlice.is_null() {
        return;
    }
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;
    pSOverRc.iStartMbSlice = (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    pSOverRc.iEndMbSlice = pSOverRc.iStartMbSlice + (*pSlice).iCountMbNumInSlice - 1;
    pSOverRc.iTargetBitsSlice = WELS_DIV_ROUND64(
        kiBitsPerMb as i64 * (*pSlice).iCountMbNumInSlice as i64,
        INT_MULTIPLY as i64,
    ) as i32;
}

/// Resets bit accumulators and macroblock counters across slices.
pub unsafe fn RcInitSliceInformation(pEncCtx: *mut sWelsEncCtx) {
    let pCurDq = current_layer(pEncCtx);
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let kiSliceNum = (*current_layer(pEncCtx)).iMaxSliceNum;

    (*pWelsSvcRc).iBitsPerMb = WELS_DIV_ROUND64(
        (*pWelsSvcRc).iTargetBits as i64 * INT_MULTIPLY as i64,
        (*pWelsSvcRc).iNumberMbFrame as i64,
    ) as i32;

    let rc_mode = (*ctx_param(pEncCtx)).iRCMode;
    (*pWelsSvcRc).bGomRC = !(rc_mode == RCMode::RC_OFF_MODE || rc_mode == RCMode::RC_BUFFERBASED_MODE);

    for i in 0..kiSliceNum as usize {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, i as i32);
        let pSOverRc = &mut (*pSlice).sSlicingOverRc;
        pSOverRc.iTotalQpSlice = 0;
        pSOverRc.iTotalMbSlice = 0;
        pSOverRc.iFrameBitsSlice = 0;
        pSOverRc.iGomBitsSlice = 0;
        pSOverRc.iStartMbSlice = 0;
        pSOverRc.iEndMbSlice = 0;
        pSOverRc.iTargetBitsSlice = 0;
    }
}

/// Allocates the target bit budget `iTargetBits` for the current frame.
pub unsafe fn RcDecideTargetBits(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let tid = (*pEncCtx).uiTemporalId as usize;
    let pTOverRc = rc_temporal_over(pWelsSvcRc).add(tid);

    (*pWelsSvcRc).iCurrentBitsLevel = BITS_NORMAL;
    let fix_rc_overshoot = (*ctx_param(pEncCtx)).bFixRCOverShoot;

    if (*pEncCtx).eSliceType as i32 == I_SLICE {
        if (*pWelsSvcRc).iIdrNum != 0 {
            (*pWelsSvcRc).iTargetBits = (*pWelsSvcRc).iBitsPerFrame
                * (*ctx_param(pEncCtx)).iIdrBitrateRatio
                / 100;
        } else {
            (*pWelsSvcRc).iTargetBits = (*pWelsSvcRc).iBitsPerFrame * IDR_BITRATE_RATIO;
        }
    } else {
        if (*pWelsSvcRc).iRemainingWeights > (*pTOverRc).iTlayerWeight
            || (fix_rc_overshoot && (*pWelsSvcRc).iRemainingWeights == (*pTOverRc).iTlayerWeight)
        {
            (*pWelsSvcRc).iTargetBits = WELS_DIV_ROUND64(
                (*pWelsSvcRc).iRemainingBits as i64 * (*pTOverRc).iTlayerWeight as i64,
                (*pWelsSvcRc).iRemainingWeights as i64,
            ) as i32;
        } else {
            (*pWelsSvcRc).iTargetBits = (*pWelsSvcRc).iRemainingBits;
        }

        if (*pWelsSvcRc).iTargetBits <= 0
            && (*ctx_param(pEncCtx)).iRCMode == RCMode::RC_BITRATE_MODE
            && !(*ctx_param(pEncCtx)).bEnableFrameSkip
        {
            (*pWelsSvcRc).iCurrentBitsLevel = BITS_EXCEEDED;
        }
        (*pWelsSvcRc).iTargetBits = WELS_CLIP3(
            (*pWelsSvcRc).iTargetBits,
            (*pTOverRc).iMinBitsTl,
            (*pTOverRc).iMaxBitsTl,
        );
    }
    (*pWelsSvcRc).iRemainingWeights -= (*pTOverRc).iTlayerWeight;
}

/// Target bit allocation routine used under `RC_TIMESTAMP_MODE`.
pub unsafe fn RcDecideTargetBitsTimestamp(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
    let iTl = (*pEncCtx).uiTemporalId as usize;
    let pTOverRc = rc_temporal_over(pWelsSvcRc).add(iTl);
    (*pWelsSvcRc).iCurrentBitsLevel = BITS_NORMAL;

    let iBufferTh = ((*pWelsSvcRc).iBufferSizeSkip as i64 - (*pWelsSvcRc).iBufferFullnessSkip) as i32;

    if (*pEncCtx).eSliceType as i32 == I_SLICE {
        if iBufferTh <= 0 {
            (*pWelsSvcRc).iCurrentBitsLevel = BITS_EXCEEDED;
            (*pWelsSvcRc).iTargetBits = (*pTOverRc).iMinBitsTl;
        } else {
            let iMaxTh = iBufferTh * 3 / 4;
            let iMinTh = if pDLayerParam.fFrameRate < 8.0 {
                (iBufferTh as f64 * 0.25) as i32
            } else {
                (iBufferTh as f64 * 2.0 / pDLayerParam.fFrameRate as f64) as i32
            };

            if pDLayerParam.fFrameRate < (IDR_BITRATE_RATIO + 1) as f32 {
                (*pWelsSvcRc).iTargetBits =
                    (pDLayerParam.iSpatialBitrate as f64 / pDLayerParam.fFrameRate as f64) as i32;
            } else {
                (*pWelsSvcRc).iTargetBits = ((pDLayerParam.iSpatialBitrate as f64
                    / pDLayerParam.fFrameRate as f64)
                    * IDR_BITRATE_RATIO as f64) as i32;
            }
            (*pWelsSvcRc).iTargetBits = WELS_CLIP3((*pWelsSvcRc).iTargetBits, iMinTh, iMaxTh);
        }
    } else {
        if iBufferTh <= 0 {
            (*pWelsSvcRc).iCurrentBitsLevel = BITS_EXCEEDED;
            (*pWelsSvcRc).iTargetBits = (*pTOverRc).iMinBitsTl;
        } else {
            let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[did];
            let kiGopSize = 1 << pDLayerParamInternal.iDecompositionStages;
            let iAverageFrameSize = if pDLayerParam.fFrameRate > 0.0 {
                (pDLayerParam.iSpatialBitrate as f64 / pDLayerParam.fFrameRate as f64) as i32
            } else {
                0
            };
            let kiGopBits = iAverageFrameSize * kiGopSize;
            (*pWelsSvcRc).iTargetBits = WELS_DIV_ROUND(
                (*pTOverRc).iTlayerWeight * kiGopBits,
                INT_MULTIPLY * 10 * 2,
            );

            let iMaxTh = iBufferTh / 2;
            let iMinTh = if pDLayerParam.fFrameRate < 8.0 {
                (iBufferTh as f64 * 0.25) as i32
            } else {
                (iBufferTh as f64 * 2.0 / pDLayerParam.fFrameRate as f64) as i32
            };
            (*pWelsSvcRc).iTargetBits = WELS_CLIP3((*pWelsSvcRc).iTargetBits, iMinTh, iMaxTh);
        }
    }
}

/// Clears GOM complexity and cost tracking arrays.
pub unsafe fn RcInitGomParameters(pEncCtx: *mut sWelsEncCtx) {
    let pCurDq = current_layer(pEncCtx);
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let kiSliceNum = (*current_layer(pEncCtx)).iMaxSliceNum;
    let kiGlobalQp = (*pEncCtx).iGlobalQp;

    (*pWelsSvcRc).iAverageFrameQp = 0;
    for i in 0..kiSliceNum as usize {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, i as i32);
        let pSOverRc = &mut (*pSlice).sSlicingOverRc;
        pSOverRc.iComplexityIndexSlice = 0;
        pSOverRc.iCalculatedQpSlice = kiGlobalQp;
    }

    (*pWelsSvcRc).pGomComplexity.fill(0.0);
    (*pWelsSvcRc).pGomCost.fill(0);
}

/// Assigns final macroblock luma and chroma QPs.
pub unsafe fn RcCalculateMbQp(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, pCurMb: &mut SMB) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;

    let mut iLumaQp = pSOverRc.iCalculatedQpSlice;
    let pCurLayer = current_layer(pEncCtx);
    let kuiChromaQpIndexOffset = (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset;

    if (*ctx_param(pEncCtx)).bEnableAdaptiveQuant {
        let pVaa = ctx_vaa(pEncCtx);
        let delta_qp_ptr = (*pVaa).sAdaptiveQuantParam.pMotionTextureIndexToDeltaQp;
        let mb_xy = (*pCurMb).iMbXY as usize;
        let delta = *delta_qp_ptr.add(mb_xy) as i32;
        iLumaQp = WELS_CLIP3(
            iLumaQp + delta,
            (*pWelsSvcRc).iMinFrameQp,
            (*pWelsSvcRc).iMaxFrameQp,
        );
    }

    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(iLumaQp + kuiChromaQpIndexOffset as i32)];
    (*pCurMb).uiLumaQp = iLumaQp as u8;
}

/// Evaluates if base layer GOM statistics can be reused for inter-layer prediction.
pub unsafe fn RcJudgeBaseUsability(pEncCtx: *mut sWelsEncCtx) -> *mut SWelsSvcRc {
    let did = (*pEncCtx).uiDependencyId as usize;
    if did == 0 {
        return std::ptr::null_mut();
    }
    let pDlpBaseInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[did - 1];
    if (*pEncCtx).uiTemporalId as i32 <= pDlpBaseInternal.iDecompositionStages {
        let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
        let pWelsSvcRc_Base = ctx_rc_at(pEncCtx, did - 1);
        let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
        let pDlpBase = &(*ctx_param(pEncCtx)).sSpatialLayers[did - 1];

        if (*pWelsSvcRc).iNumberMbGom != 0 && (*pWelsSvcRc_Base).iNumberMbGom != 0 {
            let ratio_cur = (pDLayerParam.iVideoWidth * pDLayerParam.iVideoHeight)
                / (*pWelsSvcRc).iNumberMbGom;
            let ratio_base = (pDlpBase.iVideoWidth * pDlpBase.iVideoHeight)
                / (*pWelsSvcRc_Base).iNumberMbGom;
            if ratio_cur == ratio_base {
                return pWelsSvcRc_Base;
            }
        }
    }
    std::ptr::null_mut()
}

/// Distributes slice bit budget to the upcoming GOM unit.
pub unsafe fn RcGomTargetBits(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;

    let kiComplexityIndex = pSOverRc.iComplexityIndexSlice;
    let iLastGomIndex = pSOverRc.iEndMbSlice / (*pWelsSvcRc).iNumberMbGom;
    let iLeftBits = pSOverRc.iTargetBitsSlice - pSOverRc.iFrameBitsSlice;

    if iLeftBits <= 0 {
        pSOverRc.iGomTargetBits = 0;
        return;
    } else if kiComplexityIndex >= iLastGomIndex {
        pSOverRc.iGomTargetBits = iLeftBits;
    } else {
        let base_ptr = RcJudgeBaseUsability(pEncCtx);
        let pWelsSvcRc_Base = if !base_ptr.is_null() {
            base_ptr
        } else {
            pWelsSvcRc
        };

        // `int32_t iSumSad` in C++, and it really does overflow: under `GOM_VAR`
        // each `pCurrentFrameGomSad[j]` is a whole GOM's luma variance, which at
        // 720p is order 1e9, so a sum over 20+ GOMs wraps. Keep the wrap — a
        // debug-build `+` traps here instead.
        let mut iSumSad: i32 = 0;
        for i in (kiComplexityIndex + 1)..=iLastGomIndex {
            iSumSad =
                iSumSad.wrapping_add(*rc_gom_sad(pWelsSvcRc_Base).add(i as usize));
        }

        let iAllocateBits = if iSumSad == 0 {
            WELS_DIV_ROUND(iLeftBits, iLastGomIndex - kiComplexityIndex)
        } else {
            let sad_val = *rc_gom_sad(pWelsSvcRc_Base)
                .add((kiComplexityIndex + 1) as usize);
            WELS_DIV_ROUND64(iLeftBits as i64 * sad_val as i64, iSumSad as i64) as i32
        };
        pSOverRc.iGomTargetBits = iAllocateBits;
    }
}

/// Dynamically adjusts slice QP at GOM boundaries.
pub unsafe fn RcCalculateGomQp(pEncCtx: *mut sWelsEncCtx, pSlice: *mut SSlice, _pCurMb: &mut SMB) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;

    let iLeftBits = (pSOverRc.iTargetBitsSlice - pSOverRc.iFrameBitsSlice) as i64;
    let iTargetLeftBits = iLeftBits + pSOverRc.iGomBitsSlice as i64 - pSOverRc.iGomTargetBits as i64;

    if iLeftBits <= 0 || iTargetLeftBits <= 0 {
        pSOverRc.iCalculatedQpSlice += 2;
    } else {
        let iBitsRatio = 10000 * iLeftBits / (iTargetLeftBits + 1);
        // The order of the last two arms is `ratectl.cpp:760-767` verbatim and is
        // load-bearing: `> 10600` is tested first, so the `-= 2` arm is unreachable
        // (every ratio above 11900 is also above 10600). Sorting the thresholds
        // "correctly" makes every ratio above 11900 drop the QP by 2 instead of 1.
        if iBitsRatio < 8409 {
            //2^(-1.5/6)*10000
            pSOverRc.iCalculatedQpSlice += 2;
        } else if iBitsRatio < 9439 {
            //2^(-0.5/6)*10000
            pSOverRc.iCalculatedQpSlice += 1;
        } else if iBitsRatio > 10600 {
            //2^(0.5/6)*10000
            pSOverRc.iCalculatedQpSlice -= 1;
        } else if iBitsRatio > 11900 {
            //2^(1.5/6)*10000
            pSOverRc.iCalculatedQpSlice -= 2;
        }
    }
    pSOverRc.iCalculatedQpSlice = WELS_CLIP3(
        pSOverRc.iCalculatedQpSlice,
        (*pWelsSvcRc).iMinFrameQp,
        (*pWelsSvcRc).iMaxFrameQp,
    );
    pSOverRc.iGomBitsSlice = 0;
}

/// Updates virtual buffer fullness after encoding a frame.
pub unsafe fn RcVBufferCalculationSkip(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pTOverRc = rc_temporal_over(pWelsSvcRc);
    let kiOutputBits = (*pWelsSvcRc).iBitsPerFrame;
    let kiOutputMaxBits = (*pWelsSvcRc).iMaxBitsPerFrame;

    (*pWelsSvcRc).iBufferFullnessSkip +=
        ((*pWelsSvcRc).iFrameDqBits - kiOutputBits) as i64;
    (*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] +=
        ((*pWelsSvcRc).iFrameDqBits - kiOutputMaxBits) as i64;
    (*pWelsSvcRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] +=
        ((*pWelsSvcRc).iFrameDqBits - kiOutputMaxBits) as i64;

    let mut iVGopBitsPred: i64 = 0;
    for i in ((*pWelsSvcRc).iFrameCodedInVGop + 1)..VGOP_SIZE as i32 {
        let tid = (*pWelsSvcRc).iTlOfFrames[i as usize] as usize;
        iVGopBitsPred += (*pTOverRc.add(tid)).iMinBitsTl as i64;
    }
    iVGopBitsPred -= (*pWelsSvcRc).iRemainingBits as i64;

    let denom = (*pWelsSvcRc).iBitsPerFrame as f64 * VGOP_SIZE as f64;
    let dIncPercent = if denom > 0.0 {
        iVGopBitsPred as f64 * 100.0 / denom - VGOP_BITS_PERCENTAGE_DIFF as f64
    } else {
        0.0
    };

    if ((*pWelsSvcRc).iBufferFullnessSkip > (*pWelsSvcRc).iBufferSizeSkip as i64
        && (*pWelsSvcRc).iAverageFrameQp > (*pWelsSvcRc).iSkipQpValue)
        || (dIncPercent > (*pWelsSvcRc).iRcVaryPercentage as f64)
    {
        (*pWelsSvcRc).bSkipFlag = true;
    }
}

/// Enforces maximum bitrate constraints over dual sliding time windows.
pub unsafe extern "C" fn CheckFrameSkipBasedMaxbr(
    pEncCtx: *mut sWelsEncCtx,
    _uiTimeStamp: i64,
    iDidIdx: i32,
) {
    let pWelsSvcRc = ctx_rc_at(pEncCtx, iDidIdx as usize);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[iDidIdx as usize];

    if !(*ctx_param(pEncCtx)).bEnableFrameSkip {
        return;
    }

    let iSentBits = (*pWelsSvcRc).iBitsPerFrame;
    let kiOutputMaxBits = (*pWelsSvcRc).iMaxBitsPerFrame;
    let kiMaxSpatialBitRate = pDLayerParam.iMaxSpatialBitrate as i64;

    let iPredSkipFramesTarBr =
        (WELS_DIV_ROUND64((*pWelsSvcRc).iBufferFullnessSkip, iSentBits as i64) as i32 + 1) >> 1;
    let iPredSkipFramesMaxBr = (WELS_MAX(
        WELS_DIV_ROUND64(
            (*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW],
            kiOutputMaxBits as i64,
        ) as i32,
        0,
    ) + 1)
        >> 1;

    let iAvailableBitsInTimeWindow = WELS_DIV_ROUND64(
        (TIME_CHECK_WINDOW - (*pEncCtx).iCheckWindowInterval) as i64 * kiMaxSpatialBitRate,
        1000,
    ) as i32;
    let iAvailableBitsInShiftTimeWindow = WELS_DIV_ROUND64(
        (TIME_CHECK_WINDOW - (*pEncCtx).iCheckWindowIntervalShift) as i64 * kiMaxSpatialBitRate,
        1000,
    ) as i32;

    let fix_rc_overshoot = (*ctx_param(pEncCtx)).bFixRCOverShoot;

    let bJudgeBufferFullSkip = ((*pWelsSvcRc).iContinualSkipFrames <= iPredSkipFramesTarBr)
        && ((*pWelsSvcRc).iBufferFullnessSkip > (*pWelsSvcRc).iBufferSizeSkip as i64);

    let bJudgeMaxBRbufferFullSkip = ((*pWelsSvcRc).iContinualSkipFrames <= iPredSkipFramesMaxBr)
        && ((*pEncCtx).iCheckWindowInterval > TIME_CHECK_WINDOW / 2)
        && ((*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW]
            + (*pWelsSvcRc).iPredFrameBit as i64
            - iAvailableBitsInTimeWindow as i64
            > 0);

    let mut bJudgeMaxBRbSkip = [false; TIME_WINDOW_TOTAL];
    bJudgeMaxBRbSkip[EVEN_TIME_WINDOW] = ((*pEncCtx).iCheckWindowInterval > TIME_CHECK_WINDOW / 2)
        && ((*pWelsSvcRc).bNeedShiftWindowCheck[EVEN_TIME_WINDOW])
        && ((*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW]
            + (*pWelsSvcRc).iPredFrameBit as i64
            - iAvailableBitsInTimeWindow as i64
            + kiOutputMaxBits as i64
            > 0);

    bJudgeMaxBRbSkip[ODD_TIME_WINDOW] =
        ((*pEncCtx).iCheckWindowIntervalShift > TIME_CHECK_WINDOW / 2)
            && ((*pWelsSvcRc).bNeedShiftWindowCheck[ODD_TIME_WINDOW])
            && ((*pWelsSvcRc).iBufferMaxBRFullness[ODD_TIME_WINDOW]
                + (*pWelsSvcRc).iPredFrameBit as i64
                - iAvailableBitsInShiftTimeWindow as i64
                + kiOutputMaxBits as i64
                > 0);

    (*pWelsSvcRc).bSkipFlag = false;
    if bJudgeBufferFullSkip
        || bJudgeMaxBRbufferFullSkip
        || bJudgeMaxBRbSkip[EVEN_TIME_WINDOW]
        || bJudgeMaxBRbSkip[ODD_TIME_WINDOW]
    {
        (*pWelsSvcRc).bSkipFlag = true;
        if !fix_rc_overshoot {
            (*pWelsSvcRc).iSkipFrameNum += 1;
            (*pWelsSvcRc).iSkipFrameInVGop += 1;
            (*pWelsSvcRc).iBufferFullnessSkip -= iSentBits as i64;
            (*pWelsSvcRc).iRemainingBits += iSentBits;
            (*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] -= kiOutputMaxBits as i64;
            (*pWelsSvcRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] -= kiOutputMaxBits as i64;
            (*pWelsSvcRc).iBufferFullnessSkip = WELS_MAX((*pWelsSvcRc).iBufferFullnessSkip, 0);
        }
    }
}

/// Evaluates frame skip status across active spatial layers.
pub unsafe fn WelsRcCheckFrameStatus(
    pEncCtx: *mut sWelsEncCtx,
    uiTimeStamp: i64,
    iSpatialNum: i32,
    iCurDid: i32,
) -> bool {
    let mut bSkipMustFlag = false;

    if (*ctx_param(pEncCtx)).bSimulcastAVC {
        let iDidIdx = iCurDid;
        (*(*pEncCtx).pFuncList)
            .pfRc
            .WelsRcPicDelayJudge(pEncCtx, uiTimeStamp, iDidIdx);
        if (*ctx_rc_at(pEncCtx, iDidIdx as usize)).bSkipFlag {
            bSkipMustFlag = true;
        }

        if !bSkipMustFlag
            && (*ctx_param(pEncCtx)).sSpatialLayers[iDidIdx as usize].iMaxSpatialBitrate
                != UNSPECIFIED_BIT_RATE
        {
            (*(*pEncCtx).pFuncList)
                .pfRc
                .WelsCheckSkipBasedMaxbr(pEncCtx, uiTimeStamp, iDidIdx);
            if (*ctx_rc_at(pEncCtx, iDidIdx as usize)).bSkipFlag {
                bSkipMustFlag = true;
            }
        }

        if bSkipMustFlag {
            let pRc = ctx_rc_at(pEncCtx, iDidIdx as usize);
            (*pRc).uiLastTimeStamp = uiTimeStamp;
            (*pRc).bSkipFlag = false;
            (*pRc).iContinualSkipFrames += 1;
            return true;
        }
    } else {
        for i in 0..iSpatialNum as usize {
            let iDidIdx = (*pEncCtx).sSpatialIndexMap[i].iDid;
            (*(*pEncCtx).pFuncList)
                .pfRc
                .WelsRcPicDelayJudge(pEncCtx, uiTimeStamp, iDidIdx);
            if (*ctx_rc_at(pEncCtx, iDidIdx as usize)).bSkipFlag {
                bSkipMustFlag = true;
            }

            if !bSkipMustFlag
                && (*ctx_param(pEncCtx)).sSpatialLayers[iDidIdx as usize].iMaxSpatialBitrate
                    != UNSPECIFIED_BIT_RATE
            {
                (*(*pEncCtx).pFuncList)
                    .pfRc
                    .WelsCheckSkipBasedMaxbr(pEncCtx, uiTimeStamp, iDidIdx);
                if (*ctx_rc_at(pEncCtx, iDidIdx as usize)).bSkipFlag {
                    bSkipMustFlag = true;
                }
            }
            if bSkipMustFlag {
                break;
            }
        }

        if bSkipMustFlag {
            for i in 0..iSpatialNum as usize {
                let iDidIdx = (*pEncCtx).sSpatialIndexMap[i].iDid;
                let pRc = ctx_rc_at(pEncCtx, iDidIdx as usize);
                (*pRc).uiLastTimeStamp = uiTimeStamp;
                (*pRc).bSkipFlag = false;
                (*pRc).iContinualSkipFrames += 1;
            }
            return true;
        }
    }
    false
}

/// Adjusts virtual buffer fullness and bit quotas when a frame is skipped.
pub unsafe extern "C" fn UpdateBufferWhenFrameSkipped(pEncCtx: *mut sWelsEncCtx, iCurDid: i32) {
    let pWelsSvcRc = ctx_rc_at(pEncCtx, iCurDid as usize);
    let kiOutputBits = (*pWelsSvcRc).iBitsPerFrame;
    let kiOutputMaxBits = (*pWelsSvcRc).iMaxBitsPerFrame;

    (*pWelsSvcRc).iBufferFullnessSkip -= kiOutputBits as i64;
    (*pWelsSvcRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] -= kiOutputMaxBits as i64;
    (*pWelsSvcRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] -= kiOutputMaxBits as i64;

    (*pWelsSvcRc).iBufferFullnessSkip = WELS_MAX((*pWelsSvcRc).iBufferFullnessSkip, 0);

    (*pWelsSvcRc).iRemainingBits += kiOutputBits;
    (*pWelsSvcRc).iSkipFrameNum += 1;
    (*pWelsSvcRc).iSkipFrameInVGop += 1;
    if crate::encoder::dump_enabled(&RC_DUMP, "OH264_RCDUMP") {
        let r = &*pWelsSvcRc;
        eprintln!(
            "RCS rem={} bfs={} skip={} csf={}",
            r.iRemainingBits, r.iBufferFullnessSkip, r.iSkipFrameNum, r.iContinualSkipFrames
        );
    }
}

/// Advances the 5000 ms sliding check window for maximum bitrate monitoring.
pub unsafe extern "C" fn UpdateMaxBrCheckWindowStatus(
    pEncCtx: *mut sWelsEncCtx,
    iSpatialNum: i32,
    uiTimeStamp: i64,
) {
    if (*pEncCtx).bCheckWindowStatusRefreshFlag {
        (*pEncCtx).iCheckWindowCurrentTs = uiTimeStamp;
    } else {
        (*pEncCtx).iCheckWindowCurrentTs = uiTimeStamp;
        (*pEncCtx).iCheckWindowStartTs = uiTimeStamp;
        (*pEncCtx).bCheckWindowStatusRefreshFlag = true;
        for i in 0..iSpatialNum as usize {
            let iCurDid = (*pEncCtx).sSpatialIndexMap[i].iDid as usize;
            let pRc = ctx_rc_at(pEncCtx, iCurDid);
            (*pRc).iBufferFullnessSkip = 0;
            (*pRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] = 0;
            (*pRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] = 0;
            (*pRc).bNeedShiftWindowCheck[ODD_TIME_WINDOW] = false;
            (*pRc).bNeedShiftWindowCheck[EVEN_TIME_WINDOW] = false;
        }
    }

    (*pEncCtx).iCheckWindowInterval =
        ((*pEncCtx).iCheckWindowCurrentTs - (*pEncCtx).iCheckWindowStartTs) as i32;

    if (*pEncCtx).iCheckWindowInterval >= (TIME_CHECK_WINDOW >> 1)
        && !(*pEncCtx).bCheckWindowShiftResetFlag
    {
        (*pEncCtx).bCheckWindowShiftResetFlag = true;
        for i in 0..iSpatialNum as usize {
            let iCurDid = (*pEncCtx).sSpatialIndexMap[i].iDid as usize;
            let pRc = ctx_rc_at(pEncCtx, iCurDid);
            if (*pRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] > 0
                && (*pRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] != (*pRc).iBufferMaxBRFullness[0]
            {
                (*pRc).bNeedShiftWindowCheck[EVEN_TIME_WINDOW] = true;
            } else {
                (*pRc).bNeedShiftWindowCheck[EVEN_TIME_WINDOW] = false;
            }
            (*pRc).iBufferMaxBRFullness[ODD_TIME_WINDOW] = 0;
        }
    }

    (*pEncCtx).iCheckWindowIntervalShift =
        if (*pEncCtx).iCheckWindowInterval >= (TIME_CHECK_WINDOW >> 1) {
            (*pEncCtx).iCheckWindowInterval - (TIME_CHECK_WINDOW >> 1)
        } else {
            (*pEncCtx).iCheckWindowInterval + (TIME_CHECK_WINDOW >> 1)
        };

    if (*pEncCtx).iCheckWindowInterval >= TIME_CHECK_WINDOW || (*pEncCtx).iCheckWindowInterval == 0
    {
        (*pEncCtx).iCheckWindowStartTs = (*pEncCtx).iCheckWindowCurrentTs;
        (*pEncCtx).iCheckWindowInterval = 0;
        (*pEncCtx).bCheckWindowShiftResetFlag = false;
        for i in 0..iSpatialNum as usize {
            let iCurDid = (*pEncCtx).sSpatialIndexMap[i].iDid as usize;
            let pRc = ctx_rc_at(pEncCtx, iCurDid);
            if (*pRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] > 0 {
                (*pRc).bNeedShiftWindowCheck[ODD_TIME_WINDOW] = true;
            } else {
                (*pRc).bNeedShiftWindowCheck[ODD_TIME_WINDOW] = false;
            }
            (*pRc).iBufferMaxBRFullness[EVEN_TIME_WINDOW] = 0;
        }
    }
}

/// Intentional no-op callback invoked after frame skipping.
/// Matches `WelsRcPostFrameSkipping` in `ratectl.cpp`.
pub unsafe extern "C" fn WelsRcPostFrameSkipping(
    _pCtx: *mut sWelsEncCtx,
    _iDid: i32,
    _uiTimeStamp: i64,
) -> bool {
    false
}

/// Intentional no-op callback invoked after frame skipped update.
/// Matches `WelsRcPostFrameSkippedUpdate` in `ratectl.cpp`.
pub unsafe fn WelsRcPostFrameSkippedUpdate(_pCtx: *mut sWelsEncCtx, _iDid: i32) {}

/// Evaluates virtual buffer underflow and calculates required padding bits.
pub unsafe fn RcVBufferCalculationPadding(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let kiOutputBits = (*pWelsSvcRc).iBitsPerFrame;
    let kiBufferThreshold = WELS_DIV_ROUND(
        PADDING_THRESHOLD * (-(*pWelsSvcRc).iBufferSizePadding),
        INT_MULTIPLY,
    );

    (*pWelsSvcRc).iBufferFullnessPadding += (*pWelsSvcRc).iFrameDqBits - kiOutputBits;

    if (*pWelsSvcRc).iBufferFullnessPadding < kiBufferThreshold {
        (*pWelsSvcRc).iPaddingSize = -(*pWelsSvcRc).iBufferFullnessPadding;
        (*pWelsSvcRc).iPaddingSize >>= 3;
        (*pWelsSvcRc).iBufferFullnessPadding = 0;
    } else {
        (*pWelsSvcRc).iPaddingSize = 0;
    }
}

/// Logs frame bit rate control telemetry.
pub unsafe fn RcTraceFrameBits(pEncCtx: *mut sWelsEncCtx, _uiTimeStamp: i64, _iFrameSize: i32) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    if (*pWelsSvcRc).iPredFrameBit != 0 {
        (*pWelsSvcRc).iPredFrameBit = (LAST_FRAME_PREDICT_WEIGHT
            * (*pWelsSvcRc).iFrameDqBits as f64
            + (1.0 - LAST_FRAME_PREDICT_WEIGHT) * (*pWelsSvcRc).iPredFrameBit as f64)
            as i32;
    } else {
        (*pWelsSvcRc).iPredFrameBit = (*pWelsSvcRc).iFrameDqBits;
    }
}

/// Computes average frame QP and updates temporal layer bit counters.
pub unsafe fn RcUpdatePictureQpBits(pEncCtx: *mut sWelsEncCtx, iCodedBits: i32) {
    let pCurDq = current_layer(pEncCtx);
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pCurSliceCtx = &(*current_layer(pEncCtx)).sSliceEncCtx;
    let mut iTotalQp = 0;
    let mut iTotalMb = 0;

    if (*pEncCtx).eSliceType as i32 == P_SLICE {
        for i in 0..pCurSliceCtx.iSliceNumInFrame as usize {
            let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, i as i32);
            let pSOverRc = &(*pSlice).sSlicingOverRc;
            iTotalQp += pSOverRc.iTotalQpSlice;
            iTotalMb += pSOverRc.iTotalMbSlice;
        }
        if iTotalMb > 0 {
            (*pWelsSvcRc).iAverageFrameQp = WELS_DIV_ROUND(
                INT_MULTIPLY * iTotalQp,
                iTotalMb * INT_MULTIPLY,
            );
        } else {
            (*pWelsSvcRc).iAverageFrameQp = (*pEncCtx).iGlobalQp;
        }
    } else {
        (*pWelsSvcRc).iAverageFrameQp = (*pEncCtx).iGlobalQp;
    }

    (*pWelsSvcRc).iFrameDqBits = iCodedBits;
    (*pWelsSvcRc).iLastCalculatedQScale = (*pWelsSvcRc).iAverageFrameQp;

    let tid = (*pEncCtx).uiTemporalId as usize;
    (*rc_temporal_over(pWelsSvcRc).add(tid)).iGopBitsDq += (*pWelsSvcRc).iFrameDqBits;
}

/// Updates the exponential moving average of Intra frame complexity.
pub unsafe fn RcUpdateIntraComplexity(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let iQStep = RcConvertQp2QStep((*pWelsSvcRc).iAverageFrameQp);
    let iIntraCmplx = iQStep as i64 * (*pWelsSvcRc).iFrameDqBits as i64;
    let mut iFrameComplexity = (*ctx_vaa(pEncCtx)).sComplexityAnalysisParam.iFrameComplexity;
    if (*ctx_param(pEncCtx)).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
        iFrameComplexity = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    }

    if (*pWelsSvcRc).iIdrNum == 0 {
        (*pWelsSvcRc).iIntraComplexity = iIntraCmplx;
        (*pWelsSvcRc).iIntraComplxMean = iFrameComplexity;
    } else {
        (*pWelsSvcRc).iIntraComplexity = WELS_DIV_ROUND64(
            LINEAR_MODEL_DECAY_FACTOR as i64 * (*pWelsSvcRc).iIntraComplexity
                + (INT_MULTIPLY - LINEAR_MODEL_DECAY_FACTOR) as i64 * iIntraCmplx,
            INT_MULTIPLY as i64,
        );
        (*pWelsSvcRc).iIntraComplxMean = WELS_DIV_ROUND64(
            LINEAR_MODEL_DECAY_FACTOR as i64 * (*pWelsSvcRc).iIntraComplxMean
                + (INT_MULTIPLY - LINEAR_MODEL_DECAY_FACTOR) as i64 * iFrameComplexity,
            INT_MULTIPLY as i64,
        );
    }

    (*pWelsSvcRc).iIntraMbCount = (*pWelsSvcRc).iNumberMbFrame;
    (*pWelsSvcRc).iIdrNum += 1;
    if (*pWelsSvcRc).iIdrNum > 255 {
        (*pWelsSvcRc).iIdrNum = 255;
    }
}

/// Updates the exponential moving average of Inter P-frame linear complexity.
pub unsafe fn RcUpdateFrameComplexity(pEncCtx: *mut sWelsEncCtx) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let kiTl = (*pEncCtx).uiTemporalId as usize;
    let pTOverRc = rc_temporal_over(pWelsSvcRc).add(kiTl);

    let mut iFrameComplexity = (*ctx_vaa(pEncCtx)).sComplexityAnalysisParam.iFrameComplexity;
    if (*ctx_param(pEncCtx)).iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME {
        let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
        iFrameComplexity = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    }

    let iQStep = RcConvertQp2QStep((*pWelsSvcRc).iAverageFrameQp);

    if (*pTOverRc).iPFrameNum == 0 {
        (*pTOverRc).iLinearCmplx = (*pWelsSvcRc).iFrameDqBits as i64 * iQStep as i64;
        (*pTOverRc).iFrameCmplxMean = iFrameComplexity;
    } else {
        (*pTOverRc).iLinearCmplx = WELS_DIV_ROUND64(
            LINEAR_MODEL_DECAY_FACTOR as i64 * (*pTOverRc).iLinearCmplx
                + (INT_MULTIPLY - LINEAR_MODEL_DECAY_FACTOR) as i64
                    * ((*pWelsSvcRc).iFrameDqBits as i64 * iQStep as i64),
            INT_MULTIPLY as i64,
        );
        (*pTOverRc).iFrameCmplxMean = WELS_DIV_ROUND64(
            LINEAR_MODEL_DECAY_FACTOR as i64 * (*pTOverRc).iFrameCmplxMean
                + (INT_MULTIPLY - LINEAR_MODEL_DECAY_FACTOR) as i64 * iFrameComplexity,
            INT_MULTIPLY as i64,
        );
    }

    (*pTOverRc).iPFrameNum += 1;
    if (*pTOverRc).iPFrameNum > 255 {
        (*pTOverRc).iPFrameNum = 255;
    }
}

/// Derives cascaded temporal layer QPs when rate control is disabled (`RC_OFF_MODE`).
pub unsafe fn RcCalculateCascadingQp(pEncCtx: *mut sWelsEncCtx, iQp: i32) -> i32 {
    let decomp = (*ctx_param(pEncCtx)).iDecompStages;
    if decomp != 0 {
        let tid = (*pEncCtx).uiTemporalId as i32;
        let mut iTemporalQp = if tid == 0 {
            iQp - 3 - (decomp as i32 - 1)
        } else {
            iQp - (decomp as i32 - tid)
        };
        iTemporalQp = WELS_CLIP3(iTemporalQp, 1, 51);
        iTemporalQp
    } else {
        iQp
    }
}

// ============================================================================
// Function Pointer Target Callbacks
// ============================================================================

pub unsafe extern "C" fn WelsRcPictureInitGom(pEncCtx: *mut sWelsEncCtx, uiTimeStamp: i64) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let kiSliceNum = (*current_layer(pEncCtx)).iMaxSliceNum;
    (*pWelsSvcRc).iContinualSkipFrames = 0;

    if (*pEncCtx).eSliceType as i32 == I_SLICE && (*pWelsSvcRc).iIdrNum == 0 {
        RcInitRefreshParameter(pEncCtx);
    }
    if RcJudgeBitrateFpsUpdate(pEncCtx) {
        RcUpdateBitrateFps(pEncCtx);
    }
    if (*pEncCtx).uiTemporalId == 0 {
        RcUpdateTemporalZero(pEncCtx);
    }
    if (*ctx_param(pEncCtx)).iRCMode == RCMode::RC_TIMESTAMP_MODE {
        RcDecideTargetBitsTimestamp(pEncCtx);
        (*pWelsSvcRc).uiLastTimeStamp = uiTimeStamp;
    } else {
        RcDecideTargetBits(pEncCtx);
    }

    if kiSliceNum > 1
        || ((*ctx_param(pEncCtx)).iRCMode == RCMode::RC_BITRATE_MODE
            && (*pEncCtx).eSliceType as i32 == I_SLICE)
    {
        (*pWelsSvcRc).bEnableGomQp = 0;
    } else {
        (*pWelsSvcRc).bEnableGomQp = 1;
    }

    if (*pEncCtx).eSliceType as i32 == I_SLICE {
        RcCalculateIdrQp(pEncCtx);
    } else {
        RcCalculatePictureQp(pEncCtx);
    }
    RcInitSliceInformation(pEncCtx);
    RcInitGomParameters(pEncCtx);
    if crate::encoder::dump_enabled(&RC_DUMP, "OH264_RCDUMP") {
        let r = &*pWelsSvcRc;
        eprintln!(
            "RCF st={} gqp={} tgt={} rem={} bpf={} maxbpf={} bpmb={} remw={} \
             idr={} gomqp={} minfq={} maxfq={} minq={} maxq={} nmbf={} nmbg={} gsz={} \
             gidx={} gnum={} fcv={} iq={} qs={} lcq={} icx={} icm={} imc={} skip={} bfs={} cbl={}",
            (*pEncCtx).eSliceType as i32,
            (*pEncCtx).iGlobalQp,
            r.iTargetBits,
            r.iRemainingBits,
            r.iBitsPerFrame,
            r.iMaxBitsPerFrame,
            r.iBitsPerMb,
            r.iRemainingWeights,
            r.iIdrNum,
            r.bEnableGomQp,
            r.iMinFrameQp,
            r.iMaxFrameQp,
            r.iMinQp,
            r.iMaxQp,
            r.iNumberMbFrame,
            r.iNumberMbGom,
            r.iGomSize,
            r.iGopIndexInVGop,
            r.iGopNumberInVGop,
            r.iFrameCodedInVGop,
            r.iInitialQp,
            r.iQStep,
            r.iLastCalculatedQScale,
            r.iIntraComplexity,
            r.iIntraComplxMean,
            r.iIntraMbCount,
            r.iSkipFrameNum,
            r.iBufferFullnessSkip,
            r.iCurrentBitsLevel
        );
    }
}

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static RC_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Gate for the differential-bisection dump; see `encoder::dump_enabled`.
static RC_MB_DUMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub unsafe extern "C" fn WelsRcPictureInfoUpdateGom(pEncCtx: *mut sWelsEncCtx, iLayerSize: i32) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let iCodedBits = iLayerSize << 3;

    RcUpdatePictureQpBits(pEncCtx, iCodedBits);

    if (*pEncCtx).eSliceType as i32 == P_SLICE {
        RcUpdateFrameComplexity(pEncCtx);
    } else {
        RcUpdateIntraComplexity(pEncCtx);
    }
    (*pWelsSvcRc).iRemainingBits -= (*pWelsSvcRc).iFrameDqBits;

    if (*ctx_param(pEncCtx)).bEnableFrameSkip {
        RcVBufferCalculationSkip(pEncCtx);
    }
    if (*ctx_param(pEncCtx)).iPaddingFlag != 0 {
        RcVBufferCalculationPadding(pEncCtx);
    }
    (*pWelsSvcRc).iFrameCodedInVGop += 1;
    if crate::encoder::dump_enabled(&RC_DUMP, "OH264_RCDUMP") {
        let r = &*pWelsSvcRc;
        eprintln!(
            "RCU dq={} rem={} bfs={} skip={} sivg={} fcv={} gidx={} sf={} lab={}",
            r.iFrameDqBits,
            r.iRemainingBits,
            r.iBufferFullnessSkip,
            r.iSkipFrameNum,
            r.iSkipFrameInVGop,
            r.iFrameCodedInVGop,
            r.iGopIndexInVGop,
            r.bSkipFlag as i32,
            r.iLastAllocatedBits
        );
    }
}

pub unsafe extern "C" fn WelsRcMbInitGom(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    pSlice: *mut SSlice,
) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;
    let pCurLayer = current_layer(pEncCtx);
    let kuiChromaQpIndexOffset = (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset;

    pSOverRc.iBsPosSlice = (*(*pEncCtx).pFuncList).eEntropyCoder.GetBsPosition(crate::encoder::svc_encode_slice::slice_writer(pEncCtx, pSlice), pSlice);

    if (*pWelsSvcRc).bEnableGomQp != 0 {
        if (*pWelsSvcRc).iNumberMbGom != 0
            && ((*pCurMb).iMbXY % (*pWelsSvcRc).iNumberMbGom == 0)
        {
            if (*pCurMb).iMbXY != pSOverRc.iStartMbSlice {
                pSOverRc.iComplexityIndexSlice += 1;
                RcCalculateGomQp(pEncCtx, pSlice, pCurMb);
            }
            RcGomTargetBits(pEncCtx, pSlice);
        }
        RcCalculateMbQp(pEncCtx, pSlice, pCurMb);
    } else {
        (*pCurMb).uiLumaQp = (*pEncCtx).iGlobalQp as u8;
        (*pCurMb).uiChromaQp = g_kuiChromaQpTable
            [CLIP3_QP_0_51((*pCurMb).uiLumaQp as i32 + kuiChromaQpIndexOffset as i32)];
    }
    if crate::encoder::dump_enabled(&RC_MB_DUMP, "OH264_RCMBDUMP") {
        eprintln!(
            "RCMB xy={} lq={} cq={} cqs={} cis={} gtb={} tbs={} fbs={} gbs={} bps={} minfq={} maxfq={}",
            (*pCurMb).iMbXY,
            (*pCurMb).uiLumaQp,
            (*pCurMb).uiChromaQp,
            pSOverRc.iCalculatedQpSlice,
            pSOverRc.iComplexityIndexSlice,
            pSOverRc.iGomTargetBits,
            pSOverRc.iTargetBitsSlice,
            pSOverRc.iFrameBitsSlice,
            pSOverRc.iGomBitsSlice,
            pSOverRc.iBsPosSlice,
            (*pWelsSvcRc).iMinFrameQp,
            (*pWelsSvcRc).iMaxFrameQp
        );
    }
}

pub unsafe extern "C" fn WelsRcMbInfoUpdateGom(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    iCostLuma: i32,
    pSlice: *mut SSlice,
) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pSOverRc = &mut (*pSlice).sSlicingOverRc;
    let kiComplexityIndex = pSOverRc.iComplexityIndexSlice as usize;

    let cur_bs = (*(*pEncCtx).pFuncList).eEntropyCoder.GetBsPosition(crate::encoder::svc_encode_slice::slice_writer(pEncCtx, pSlice), pSlice);
    let iCurMbBits = cur_bs - pSOverRc.iBsPosSlice;
    pSOverRc.iFrameBitsSlice += iCurMbBits;
    pSOverRc.iGomBitsSlice += iCurMbBits;

    if !rc_gom_cost(pWelsSvcRc).is_null() {
        *rc_gom_cost(pWelsSvcRc).add(kiComplexityIndex) += iCostLuma;
    }
    if iCurMbBits > 0 {
        pSOverRc.iTotalQpSlice += (*pCurMb).uiLumaQp as i32;
        pSOverRc.iTotalMbSlice += 1;
    }
}

pub unsafe extern "C" fn WelsRcPictureInitDisable(pEncCtx: *mut sWelsEncCtx, _uiTimeStamp: i64) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pDLayerParam = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
    let kiQp = pDLayerParam.iDLayerQp;

    (*pEncCtx).iGlobalQp = RcCalculateCascadingQp(pEncCtx, kiQp);

    if (*ctx_param(pEncCtx)).bEnableAdaptiveQuant && (*pEncCtx).eSliceType as i32 == P_SLICE {
        let delta_offset = (*ctx_vaa(pEncCtx))
            .sAdaptiveQuantParam
            .iAverMotionTextureIndexToDeltaQp;
        (*pEncCtx).iGlobalQp = WELS_CLIP3(
            ((*pEncCtx).iGlobalQp * INT_MULTIPLY - delta_offset) / INT_MULTIPLY,
            (*pWelsSvcRc).iMinQp,
            (*pWelsSvcRc).iMaxQp,
        );
    } else {
        (*pEncCtx).iGlobalQp = WELS_CLIP3((*pEncCtx).iGlobalQp, 0, 51);
    }

    (*pWelsSvcRc).iAverageFrameQp = (*pEncCtx).iGlobalQp;
}

/// Intentional no-op picture-level RC update callback when rate control is disabled.
/// Matches `WelsRcPictureInfoUpdateDisable` in `ratectl.cpp:1298`.
pub unsafe extern "C" fn WelsRcPictureInfoUpdateDisable(_pEncCtx: *mut sWelsEncCtx, _iLayerSize: i32) {}

pub unsafe extern "C" fn WelsRcMbInitDisable(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    _pSlice: *mut SSlice,
) {
    let mut iLumaQp = (*pEncCtx).iGlobalQp;
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pCurLayer = current_layer(pEncCtx);
    let kuiChromaQpIndexOffset = (*layer_pps(pEncCtx, pCurLayer)).uiChromaQpIndexOffset;

    if (*ctx_param(pEncCtx)).bEnableAdaptiveQuant && (*pEncCtx).eSliceType as i32 == P_SLICE {
        let pVaa = ctx_vaa(pEncCtx);
        let delta_qp_ptr = (*pVaa).sAdaptiveQuantParam.pMotionTextureIndexToDeltaQp;
        let mb_xy = (*pCurMb).iMbXY as usize;
        let delta = *delta_qp_ptr.add(mb_xy) as i32;
        iLumaQp = WELS_CLIP3(
            iLumaQp + delta,
            (*pWelsSvcRc).iMinQp,
            (*pWelsSvcRc).iMaxQp,
        );
    } else {
        iLumaQp = WELS_CLIP3(iLumaQp, 0, 51);
    }

    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51(iLumaQp + kuiChromaQpIndexOffset as i32)];
    (*pCurMb).uiLumaQp = iLumaQp as u8;
}

/// Intentional no-op macroblock-level RC update callback when rate control is disabled.
/// Matches `WelsRcMbInfoUpdateDisable` in `ratectl.cpp:1319`.
pub unsafe extern "C" fn WelsRcMbInfoUpdateDisable(
    _pEncCtx: *mut sWelsEncCtx,
    _pCurMb: &mut SMB,
    _iCostLuma: i32,
    _pSlice: *mut SSlice,
) {}

pub unsafe extern "C" fn WelRcPictureInitBufferBasedQp(
    pEncCtx: *mut sWelsEncCtx,
    _uiTimeStamp: i64,
) {
    let pVaa = ctx_vaa(pEncCtx);
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);

    let mut iMinQp = (*ctx_param(pEncCtx)).iMinQp;
    if (*pVaa).eSceneChangeIdc as i32 == LARGE_CHANGED_SCENE {
        iMinQp += 2;
    } else if (*pVaa).eSceneChangeIdc as i32 == MEDIUM_CHANGED_SCENE {
        iMinQp += 1;
    }

    if (*pEncCtx).bDeliveryFlag {
        (*pEncCtx).iGlobalQp -= 1;
    } else {
        (*pEncCtx).iGlobalQp += 2;
    }
    (*pEncCtx).iGlobalQp = WELS_CLIP3((*pEncCtx).iGlobalQp, iMinQp, (*pWelsSvcRc).iMaxQp);
    (*pWelsSvcRc).iAverageFrameQp = (*pEncCtx).iGlobalQp;
    (*pWelsSvcRc).iMaxFrameQp = (*pEncCtx).iGlobalQp;
    (*pWelsSvcRc).iMinFrameQp = (*pEncCtx).iGlobalQp;
}

pub unsafe extern "C" fn WelRcPictureInitScc(pEncCtx: *mut sWelsEncCtx, uiTimeStamp: i64) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
    let pDLayerConfig = &(*ctx_param(pEncCtx)).sSpatialLayers[did];
    let pDLayerParamInternal = &(*ctx_param(pEncCtx)).sDependencyLayers[did];

    let iFrameCplx = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    let iBitRate = pDLayerConfig.iSpatialBitrate;

    let mut iBaseQp = (*pWelsSvcRc).iBaseQp;
    (*pEncCtx).iGlobalQp = iBaseQp;

    if (*pEncCtx).eSliceType as i32 == I_SLICE {
        let mut iTargetBits = (iBitRate as i64 * 2) - (*pWelsSvcRc).iBufferFullnessSkip;
        iTargetBits = WELS_MAX(1, iTargetBits);
        let iQstep = WELS_DIV_ROUND64(iFrameCplx * (*pWelsSvcRc).iCost2BitsIntra, iTargetBits) as i32;
        let iQp = RcConvertQStep2Qp(iQstep);
        (*pEncCtx).iGlobalQp = WELS_CLIP3(iQp, (*pWelsSvcRc).iMinQp, (*pWelsSvcRc).iMaxQp);
    } else {
        let iTargetBits = if pDLayerParamInternal.fOutputFrameRate > 0.0 {
            WELS_ROUND(iBitRate as f64 / pDLayerParamInternal.fOutputFrameRate as f64) as i64
        } else {
            1
        };
        let iQstep = WELS_DIV_ROUND64(iFrameCplx * (*pWelsSvcRc).iAvgCost2Bits, iTargetBits) as i32;
        let iQp = RcConvertQStep2Qp(iQstep);
        let iDeltaQp = iQp - iBaseQp;

        if (*pWelsSvcRc).iBufferFullnessSkip > iBitRate as i64 {
            if iDeltaQp > 0 {
                iBaseQp += 1;
            }
        } else if (*pWelsSvcRc).iBufferFullnessSkip == 0 {
            if iDeltaQp < 0 {
                iBaseQp -= 1;
            }
        }

        if iDeltaQp >= 6 {
            iBaseQp += 3;
        } else if iDeltaQp <= -6 {
            iBaseQp -= 1;
        }
        iBaseQp = WELS_CLIP3(iBaseQp, (*pWelsSvcRc).iMinQp, (*pWelsSvcRc).iMaxQp);
        (*pEncCtx).iGlobalQp = iBaseQp;

        if iDeltaQp < -6 {
            (*pEncCtx).iGlobalQp = WELS_CLIP3(
                (*pWelsSvcRc).iBaseQp - 6,
                (*pWelsSvcRc).iMinQp,
                (*pWelsSvcRc).iMaxQp,
            );
        }

        if iDeltaQp > 5 {
            let scene_change = (*ctx_vaa(pEncCtx)).eSceneChangeIdc;
            if scene_change as i32 == LARGE_CHANGED_SCENE
                || (*pWelsSvcRc).iBufferFullnessSkip > 2 * iBitRate as i64
                || iDeltaQp > 10
            {
                (*pEncCtx).iGlobalQp = WELS_CLIP3(
                    (*pWelsSvcRc).iBaseQp + iDeltaQp,
                    (*pWelsSvcRc).iMinQp,
                    (*pWelsSvcRc).iMaxQp,
                );
            } else if scene_change as i32 == MEDIUM_CHANGED_SCENE
                || (*pWelsSvcRc).iBufferFullnessSkip > iBitRate as i64
            {
                (*pEncCtx).iGlobalQp = WELS_CLIP3(
                    (*pWelsSvcRc).iBaseQp + 5,
                    (*pWelsSvcRc).iMinQp,
                    (*pWelsSvcRc).iMaxQp,
                );
            }
        }
        (*pWelsSvcRc).iBaseQp = iBaseQp;
    }
    (*pWelsSvcRc).iAverageFrameQp = (*pEncCtx).iGlobalQp;
    (*pWelsSvcRc).uiLastTimeStamp = uiTimeStamp;
}

pub unsafe fn WelsRcDropFrameUpdate(pEncCtx: *mut sWelsEncCtx, iDropSize: u32) {
    let pWelsSvcRc = ctx_rc(pEncCtx);
    (*pWelsSvcRc).iBufferFullnessSkip -= iDropSize as i64;
    (*pWelsSvcRc).iBufferFullnessSkip = WELS_MAX(0, (*pWelsSvcRc).iBufferFullnessSkip);
}

pub unsafe extern "C" fn WelsRcPictureInfoUpdateScc(pEncCtx: *mut sWelsEncCtx, iNalSize: i32) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let iFrameBits = iNalSize << 3;
    (*pWelsSvcRc).iBufferFullnessSkip += iFrameBits as i64;

    let pVaa = ctx_vaa(pEncCtx) as *mut SVAAFrameInfoExt;
    let iQstep = RcConvertQp2QStep((*pEncCtx).iGlobalQp);
    let screen_cmplx = (*pVaa).sComplexityScreenParam.iFrameComplexity;
    let iCost2Bits = if screen_cmplx != 0 {
        WELS_DIV_ROUND64(iFrameBits as i64 * iQstep as i64, screen_cmplx)
    } else {
        0
    };

    if (*pEncCtx).eSliceType as i32 == P_SLICE {
        (*pWelsSvcRc).iAvgCost2Bits = WELS_DIV_ROUND64(
            95 * (*pWelsSvcRc).iAvgCost2Bits + 5 * iCost2Bits,
            INT_MULTIPLY as i64,
        );
    } else {
        (*pWelsSvcRc).iCost2BitsIntra = WELS_DIV_ROUND64(
            90 * (*pWelsSvcRc).iCost2BitsIntra + 10 * iCost2Bits,
            INT_MULTIPLY as i64,
        );
    }
}

pub unsafe extern "C" fn WelsRcMbInitScc(
    pEncCtx: *mut sWelsEncCtx,
    pCurMb: &mut SMB,
    _pSlice: *mut SSlice,
) {
    (*pCurMb).uiLumaQp = (*pEncCtx).iGlobalQp as u8;
    let offset = (*ctx_pps(pEncCtx)).uiChromaQpIndexOffset as i32;
    (*pCurMb).uiChromaQp = g_kuiChromaQpTable[CLIP3_QP_0_51((*pCurMb).uiLumaQp as i32 + offset)];
}

pub unsafe extern "C" fn WelsRcFrameDelayJudgeTimeStamp(
    pEncCtx: *mut sWelsEncCtx,
    uiTimeStamp: i64,
    iDidIdx: i32,
) {
    let pWelsSvcRc = ctx_rc_at(pEncCtx, iDidIdx as usize);
    let pDLayerConfig = &(*ctx_param(pEncCtx)).sSpatialLayers[iDidIdx as usize];

    let iBitRate = pDLayerConfig.iSpatialBitrate;
    let mut iEncTimeInv = if (*pWelsSvcRc).uiLastTimeStamp == 0 {
        0
    } else {
        (uiTimeStamp - (*pWelsSvcRc).uiLastTimeStamp) as i32
    };
    if iEncTimeInv < 0 || iEncTimeInv > 1000 {
        iEncTimeInv = if pDLayerConfig.fFrameRate > 0.0 {
            (1000.0 / pDLayerConfig.fFrameRate as f64) as i32
        } else {
            0
        };
        (*pWelsSvcRc).uiLastTimeStamp = uiTimeStamp - iEncTimeInv as i64;
    }
    let mut iSentBits = (iBitRate as f64 * iEncTimeInv as f64 * 1.0e-3 + 0.5) as i32;
    iSentBits = WELS_MAX(iSentBits, 0);

    (*pWelsSvcRc).iBufferSizeSkip = WELS_DIV_ROUND(
        pDLayerConfig.iSpatialBitrate * (*pWelsSvcRc).iSkipBufferRatio,
        INT_MULTIPLY,
    );
    (*pWelsSvcRc).iBufferSizePadding = WELS_DIV_ROUND(
        pDLayerConfig.iSpatialBitrate * PADDING_BUFFER_RATIO,
        INT_MULTIPLY,
    );

    (*pWelsSvcRc).iBufferFullnessSkip -= iSentBits as i64;
    (*pWelsSvcRc).iBufferFullnessSkip = WELS_MAX(
        (-1i64) * (pDLayerConfig.iSpatialBitrate as i64 / 4),
        (*pWelsSvcRc).iBufferFullnessSkip,
    );

    if (*ctx_param(pEncCtx)).bEnableFrameSkip {
        (*pWelsSvcRc).bSkipFlag = true;
        if (*pWelsSvcRc).iBufferFullnessSkip < (*pWelsSvcRc).iBufferSizeSkip as i64 {
            (*pWelsSvcRc).bSkipFlag = false;
        }
        if (*pWelsSvcRc).bSkipFlag {
            (*pWelsSvcRc).iSkipFrameNum += 1;
            (*pWelsSvcRc).uiLastTimeStamp = uiTimeStamp;
        }
    }
}

pub unsafe extern "C" fn WelsRcPictureInfoUpdateGomTimeStamp(
    pEncCtx: *mut sWelsEncCtx,
    iLayerSize: i32,
) {
    let did = (*pEncCtx).uiDependencyId as usize;
    let pWelsSvcRc = ctx_rc_at(pEncCtx, did);
    let iCodedBits = iLayerSize << 3;

    RcUpdatePictureQpBits(pEncCtx, iCodedBits);
    if (*pEncCtx).eSliceType as i32 == P_SLICE {
        RcUpdateFrameComplexity(pEncCtx);
    } else {
        RcUpdateIntraComplexity(pEncCtx);
    }

    (*pWelsSvcRc).iRemainingBits -= (*pWelsSvcRc).iFrameDqBits;
    (*pWelsSvcRc).iBufferFullnessSkip += (*pWelsSvcRc).iFrameDqBits as i64;

    if (*ctx_param(pEncCtx)).iPaddingFlag != 0 {
        RcVBufferCalculationPadding(pEncCtx);
    }
    (*pWelsSvcRc).iFrameCodedInVGop += 1;
}

/// Populates the rate control function dispatch table.
///
/// **T4b.1b**: the table is one field, so "populate" is one assignment. The
/// `match` that used to be here is now nine `match`es, one per former slot, each
/// at the point of use — see [`SWelsRcFunc`]. The C++ per-mode blocks are
/// transposed rather than deleted: read the methods down instead of across, and
/// `rc.cpp:WelsRcInitFuncPointers`'s five cases are still all there.
///
/// The signature is unchanged so its two callers — `InitFunctionPointers` and
/// `SetOption(ENCODER_OPTION_RC_MODE)` — keep their shape; those two are the
/// **only** places the installed mode may change, which is the property the type
/// note depends on.
pub unsafe fn WelsRcInitFuncPointers(pRcf: &mut SWelsRcFunc, iRcMode: RCMode) {
    pRcf.eInstalledMode = iRcMode;
}

/// Top-level initialization entry point called during encoder creation.
pub unsafe fn WelsRcInitModule(pEncCtx: *mut sWelsEncCtx, iRcMode: RCMode) {
    if !pEncCtx.is_null() && !(*pEncCtx).pFuncList.is_null() {
        WelsRcInitFuncPointers(&mut (*(*pEncCtx).pFuncList).pfRc, iRcMode);
    }
    RcInitSequenceParameter(pEncCtx);
}

// **T6.H6**: `WelsRcFreeMemory` and `RcFreeLayerMemory` stood here. They walked the
// spatial layers releasing each one's rate-control block, and `WelsUninitEncoderExt`
// had to call the pair *before* releasing `pWelsSvcRc` itself, because the blocks
// hung off the array being freed. The five containers are the layer's own, the layers
// are the context's `Vec`, and the whole cascade is one drop — so both functions are
// deleted rather than converted, and the ordering constraint they documented is gone
// with them.

/// Computes a monotonically increasing timestamp for rate control.
#[inline]
pub fn GetTimestampForRc(uiTimeStamp: i64, uiLastTimeStamp: i64, fFrameRate: f32) -> i64 {
    if (uiLastTimeStamp >= uiTimeStamp) || (uiTimeStamp == 0 && uiLastTimeStamp != -1) {
        if fFrameRate > 0.0 {
            uiLastTimeStamp + (1000.0 / fFrameRate as f64) as i64
        } else {
            uiLastTimeStamp
        }
    } else {
        uiTimeStamp
    }
}

#[inline]
pub fn WelsUpdateSkipFrameStatus() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rc_convert_qp_qstep() {
        assert_eq!(RcConvertQp2QStep(0), 63);
        assert_eq!(RcConvertQp2QStep(4), 100);
        assert_eq!(RcConvertQp2QStep(51), 22807);

        assert_eq!(RcConvertQStep2Qp(63), 0);
        assert_eq!(RcConvertQStep2Qp(100), 4);
    }

    #[test]
    fn test_get_timestamp_for_rc() {
        let ts = GetTimestampForRc(100, 100, 30.0);
        assert_eq!(ts, 133);
        let ts2 = GetTimestampForRc(200, 100, 30.0);
        assert_eq!(ts2, 200);
    }

    #[test]
    fn test_clip3_and_div_round() {
        assert_eq!(WELS_CLIP3(55, 0, 51), 51);
        assert_eq!(WELS_CLIP3(-5, 0, 51), 0);
        assert_eq!(WELS_CLIP3(25, 0, 51), 25);
        assert_eq!(WELS_DIV_ROUND(100, 10), 10);
    }

    #[test]
    fn test_rc_intentional_noop_callbacks() {
        unsafe {
            assert!(!WelsRcPostFrameSkipping(std::ptr::null_mut(), 0, 0));
            WelsRcPostFrameSkippedUpdate(std::ptr::null_mut(), 0);
            WelsRcPictureInfoUpdateDisable(std::ptr::null_mut(), 0);
            // The macroblock argument is a `&mut SMB` now, so the null goes and a
            // real record takes its place. The other three are still raw (context,
            // slice) and stay null: this is the no-op arm of `pfWelsRcMbInfoUpdate`
            // and reads none of them.
            let mut sMb = SMB::default();
            WelsRcMbInfoUpdateDisable(std::ptr::null_mut(), &mut sMb, 0, std::ptr::null_mut());
        }
    }
}

