//! `CWelsH264SVCEncoder::SetOption` / `GetOption` — the encoder's untyped
//! `void* pOption` boundary.
//!
//! **S12.2 moved these here from `encoder/wels_encoder_ext.rs`** (plan step 4's
//! pattern, the same one that moved the version exports to [`crate::api::version`]).
//! They were the last production `unsafe` outside this island that is *about* the
//! C ABI rather than about the codec: `pOption` is `codec_api.h`'s untyped blob,
//! its real type is named by `eOptionId` and by nothing else the compiler can see,
//! and each arm's cast **is** that enum's contract with the application. S11.16
//! already retagged them `C-ABI` after they were wrongly queued as convertible port
//! work; this finishes the thought by putting them where the tag says they live.
//!
//! Nothing about the code changed in the move — the two methods are verbatim, and
//! they stay inherent methods of [`CWelsH264SVCEncoder`], which Rust permits from
//! any module of the defining crate. So `codec_api.rs`'s vtable thunks still call
//! `self.SetOption(..)`, **the exported symbol set is untouched**, and
//! `tools/abi_exports.sh` and `tools/abi_sizes.sh` are the referees that say so.
//!
//! What the move buys is one file: `encoder/wels_encoder_ext.rs` held these two
//! allows and nothing else, so it seals `#![forbid(unsafe_code)]` behind them.

// No `clippy::not_unsafe_ptr_arg_deref` allow travelled here with the two methods.
// One was written, then measured out: with it removed, `cargo clippy --lib`
// (clippy 0.1.97) does not report the lint at either `SetOption` or `GetOption`,
// nor anywhere else in the crate. An allow that suppresses nothing is silence
// without a subject, which is the thing session J's crate-root pass deleted four of.
// The C++ names come across verbatim, as everywhere in the port.
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    unused_variables
)]
// The island's regime (T8.C7): the module denies, and every surviving item carries
// its own tagged `#[allow(unsafe_code)]`.
#![deny(unsafe_code)]

use std::ffi::c_void;

use crate::api::codec_api::{
    EComplexityMode, EncoderOption, RCMode, SBitrateInfo, SEncParamBase, SEncParamExt,
    SSpatialLayerConfig, VideoFormat,
};
use crate::api::codec_api::LAYER_NUM::*;
use crate::api::codec_api::ECOMPLEXITY_MODE::*;
use crate::api::codec_api::EParameterSetStrategy::*;
use crate::common::wels_trace::{WelsLog, WelsTraceCallback, WELS_LOG_INFO};
use crate::encoder::param_svc::{
    SWelsSvcCodingParam, MAX_SPATIAL_LAYER_NUM, WELS_CLIP3,
};
use crate::encoder::rc::WelsRcInitFuncPointers;
use crate::encoder::ref_list_mgr_svc::{
    FilterLTRMarkingFeedback, FilterLTRRecoveryRequest, SLTRMarkingFeedback, SLTRRecoverRequest,
};
use crate::api::codec_api::RC_MODES::*;
use crate::encoder::wels_encoder_ext::{
    cmInitExpected, cmInitParaError, cmResultSuccess, rc_mode_from_raw, CWelsH264SVCEncoder,
    CheckLevelSetting, CheckProfileSetting, CheckReferenceNumSetting, SDeliveryStatus, SLTRConfig,
    SLevelInfo, SProfileInfo, TagVideoEncoderStatistics, WelsEncoderApplyBitRate,
    WelsEncoderApplyBitVaryRang, WelsEncoderApplyFrameRate, WelsEncoderApplyLTR,
    WelsEncoderParamAdjust, MAX_BIT_RATE, MAX_DEPENDENCY_LAYER, MAX_FRAME_RATE, MIN_BIT_RATE,
    MIN_FRAME_RATE,
};

impl CWelsH264SVCEncoder {
    // **S11.16: `C-ABI`, not `port-raw`.** This was tagged as convertible port
    // work, and it is not: the parameter is the C interface's untyped
    // `void* pOption`, whose real type is named by `eOptionId` and by nothing
    // else the compiler can see. Each arm's cast *is* that enum's contract with
    // the application, `codec_api.rs`'s thunks pass the caller's pointer
    // straight through, and there is no Rust shape that removes the cast
    // without changing the exported interface — which is frozen (plan §7.1).
    // Same allow, honest category; it stops sitting in the conversion queue.
    // unsafe-cat: C-ABI
    #[allow(unsafe_code)]
    /// `pOption` is **C-ABI** and stays a `c_void` (T8.B10): its type is a function
    /// of `eOptionId` and of nothing else, over thirty-two ids, and no Rust type
    /// states that. `Encoder::set_option_raw` is the safe surface's `unsafe`
    /// spelling of the same obligation.
    pub fn SetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        // **B3 — the function-level raw is gone.** Each arm resolves the context off
        // its own slot at the point it uses it, so the three arms that *replace* the
        // context (`WelsEncoderParamAdjust` twice, `ENCODER_OPTION_LTR`) no longer
        // need a re-derivation: there is no stale handle to refresh, and borrowck
        // rejects using one across the replacement rather than leaving it to Miri.
        // `is_none()` is `ctx_ptr(..).is_null()` — the accessor answered null exactly
        // when the slot was empty.
        if (self.m_pEncContext.is_none() || !self.m_bInitialFlag)
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_LEVEL
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_CALLBACK
            && eOptionId != EncoderOption::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT
        {
            return cmInitExpected;
        }

        unsafe {
            match eOptionId {
                EncoderOption::ENCODER_OPTION_INTER_SPATIAL_PRED => {
                    // "this feature not supported at present" — C++ logs and
                    // returns success without touching anything.
                }
                EncoderOption::ENCODER_OPTION_DATAFORMAT => {
                    let iValue = *(pOption as *const i32);
                    if iValue == 0 {
                        return cmInitParaError;
                    }
                    self.m_iCspInternal = iValue;
                }
                EncoderOption::ENCODER_OPTION_IDR_INTERVAL => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let mut iValue = *(pOption as *const i32);
                    if iValue <= -1 {
                        iValue = 0;
                    }
                    if iValue == ctx.param().uiIntraPeriod as i32 {
                        return cmResultSuccess;
                    }
                    ctx.param_mut().uiIntraPeriod = iValue as u32;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE => {
                    let sEncodingParam = *(pOption as *const SEncParamBase);
                    let mut sConfig = SWelsSvcCodingParam::default();
                    if sConfig.ParamBaseTranscode(&sEncodingParam) != 0 {
                        return cmInitParaError;
                    }
                    let iTargetWidth = sConfig.iPicWidth;
                    let iTargetHeight = sConfig.iPicHeight;
                    if self.m_iMaxPicWidth != iTargetWidth
                        || self.m_iMaxPicHeight != iTargetHeight
                    {
                        self.m_iMaxPicWidth = iTargetWidth;
                        self.m_iMaxPicHeight = iTargetHeight;
                    }
                    if sConfig.DetermineTemporalSettings() != 0 {
                        return cmInitParaError;
                    }
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: `WelsEncoderParamAdjust` may replace the context
                    // (`encoder_ext.cpp`'s uninit/init pair), so any earlier handle no
                    // longer names this encoder's context. **B3**: the re-derivation is
                    // gone with the function-level raw — each arm resolves the slot
                    // where it uses it, and the timestamp is copied out so the borrow
                    // ends before the `&mut self` logging calls below.
                    let ts = match self.m_pEncContext.as_deref() {
                        Some(ctx) => ctx.iLastStatisticsLogTs,
                        None => return cmInitExpected,
                    };
                    // LogStatistics
                    self.LogStatistics(ts, 0);
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let sEncodingParam = *(pOption as *const SEncParamExt);
                    // **T9.X2 — the port's fourth `TraceParamInfo` call site, missing
                    // until now.** `welsEncoderExt.cpp:796` logs the incoming block
                    // here, immediately after the copy and *before* the spatial-layer
                    // check below, so a caller whose parameters are about to be
                    // rejected still gets them echoed. The port had the other three
                    // (`:202`, `:229`, `:334`) and not this one; X2's brief lists only
                    // those three, which is where the omission survived. It made no
                    // observable difference while the body was empty. See F182.
                    self.TraceParamInfo(&sEncodingParam);
                    // verify number of spatial layer
                    if sEncodingParam.iSpatialLayerNum < 1
                        || sEncodingParam.iSpatialLayerNum > MAX_SPATIAL_LAYER_NUM as i32
                    {
                        return cmInitParaError;
                    }
                    let mut sConfig = SWelsSvcCodingParam::default();
                    if sConfig.ParamTranscode(&sEncodingParam) != 0 {
                        return cmInitParaError;
                    }
                    if sConfig.iSpatialLayerNum < 1 {
                        return cmInitParaError;
                    }
                    if sConfig.DetermineTemporalSettings() != 0 {
                        return cmInitParaError;
                    }
                    let iTargetWidth = sConfig.iPicWidth;
                    let iTargetHeight = sConfig.iPicHeight;
                    if self.m_iMaxPicWidth != iTargetWidth
                        || self.m_iMaxPicHeight != iTargetHeight
                    {
                        self.m_iMaxPicWidth = iTargetWidth;
                        self.m_iMaxPicHeight = iTargetHeight;
                    }
                    /* Check every field whether there is new request for memory block changed or else */
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: `WelsEncoderParamAdjust` may replace the context
                    // (`encoder_ext.cpp`'s uninit/init pair), so any earlier handle no
                    // longer names this encoder's context. **B3**: the re-derivation is
                    // gone with the function-level raw — each arm resolves the slot
                    // where it uses it, and the timestamp is copied out so the borrow
                    // ends before the `&mut self` logging calls below.
                    let ts = match self.m_pEncContext.as_deref() {
                        Some(ctx) => ctx.iLastStatisticsLogTs,
                        None => return cmInitExpected,
                    };
                    // LogStatistics
                    //
                    // **T9.X2 — the announcement line was missing too (F186).**
                    // `welsEncoderExt.cpp:845` logs this immediately before the
                    // statistics block, and the port had the call without it. It went
                    // unnoticed because until this session neither diffharness driver
                    // ever took this arm; the referee's 23rd argument is what made the
                    // arm reachable and the omission visible on the same run.
                    WelsLog(
                        self.log_ctx(),
                        WELS_LOG_INFO,
                        "CWelsH264SVCEncoder::SetOption():ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, LogStatisticsBeforeNewEncoding",
                    );
                    self.LogStatistics(ts, sEncodingParam.iSpatialLayerNum - 1);
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const f32);
                    if iValue <= 0.0 {
                        return cmInitParaError;
                    }
                    ctx.param_mut().fMaxFrameRate =
                        WELS_CLIP3(iValue, MIN_FRAME_RATE, MAX_FRAME_RATE);
                    WelsEncoderApplyFrameRate(ctx.param_mut());
                }
                EncoderOption::ENCODER_OPTION_BITRATE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pInfo = &*(pOption as *const SBitrateInfo);
                    let mut iBitrate = pInfo.iBitrate;
                    if iBitrate <= 0 {
                        return cmInitParaError;
                    }
                    iBitrate = WELS_CLIP3(iBitrate, MIN_BIT_RATE, MAX_BIT_RATE);
                    match pInfo.iLayer {
                        SPATIAL_LAYER_ALL => {
                            ctx.param_mut().iTargetBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
                            ctx.param_mut().sSpatialLayers[pInfo.iLayer as usize]
                                .iSpatialBitrate = iBitrate;
                        }
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyBitRate(log_ctx, ctx.param_mut(), pInfo.iLayer as i32)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pInfo = &*(pOption as *const SBitrateInfo);
                    let mut iBitrate = pInfo.iBitrate;
                    if iBitrate <= 0 {
                        return cmInitParaError;
                    }
                    iBitrate = WELS_CLIP3(iBitrate, MIN_BIT_RATE, MAX_BIT_RATE);
                    match pInfo.iLayer {
                        SPATIAL_LAYER_ALL => {
                            ctx.param_mut().iMaxBitrate = iBitrate;
                        }
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2
                        | SPATIAL_LAYER_3 => {
                            ctx.param_mut().sSpatialLayers[pInfo.iLayer as usize]
                                .iMaxSpatialBitrate = iBitrate;
                        }
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyBitRate(log_ctx, ctx.param_mut(), pInfo.iLayer as i32)
                        != 0
                    {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_RC_MODE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    // 0:quality mode;1:bit-rate mode;2:bitrate limited mode
                    let iValue = *(pOption as *const i32);
                    ctx.param_mut().iRCMode = rc_mode_from_raw(iValue);
                    // Re-point the dispatch table. Setting the field alone leaves
                    // the encoder running the previous mode's callbacks.
                    let iRCMode = ctx.param().iRCMode;
                    // **A6: the second of the two derivations the flip could
                    // not take** — see `ctx_func_list_raw`. `pCtx` here is
                    // `Self::ctx_ptr`'s raw, so `func_list_mut` would mean a
                    // whole-context `&mut` retag through a raw root.
                    WelsRcInitFuncPointers(
                        &mut ctx.func_list_mut().pfRc,
                        iRCMode,
                    );
                }
                EncoderOption::ENCODER_OPTION_RC_FRAME_SKIP => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    // 0:FRAME-SKIP disabled;1:FRAME-SKIP enabled
                    let bValue = *(pOption as *const bool);
                    if ctx.param().iRCMode != RC_OFF_MODE {
                        ctx.param_mut().bEnableFrameSkip = bValue;
                    }
                    // rc off: the setting is accepted and ignored, as in C++.
                }
                EncoderOption::ENCODER_PADDING_PADDING => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    // 0:disable padding;1:padding
                    let iValue = *(pOption as *const i32);
                    ctx.param_mut().iPaddingFlag = iValue;
                }
                EncoderOption::ENCODER_LTR_RECOVERY_REQUEST => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pLTR_Recover_Request = &mut *(pOption as *mut SLTRRecoverRequest);
                    // S67 blessed (H2): the second argument points into the **C caller's**
                    // memory, not the context.
                    FilterLTRRecoveryRequest(ctx, pLTR_Recover_Request);
                }
                EncoderOption::ENCODER_LTR_MARKING_FEEDBACK => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let fb = &mut *(pOption as *mut SLTRMarkingFeedback);
                    // S67 blessed (H2): as the recovery-request arm above — `pOption` is the
                    // caller's.
                    FilterLTRMarkingFeedback(ctx, fb);
                }
                EncoderOption::ENCODER_LTR_MARKING_PERIOD => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const u32);
                    ctx.param_mut().iLtrMarkPeriod = iValue;
                }
                EncoderOption::ENCODER_OPTION_LTR => {
                    // S10.5b: the deref stays *here*, at the C-ABI edge where the
                    // `void*` arrives, instead of travelling into
                    // `WelsEncoderApplyLTR` as a raw parameter. This arm is one of
                    // this dispatcher's many caller-supplied-pointer reads and its
                    // allow is not going anywhere; the callee's is.
                    let pLTRValue = &mut *(pOption as *mut SLTRConfig);
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyLTR(log_ctx, &mut self.m_pEncContext, pLTRValue) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: the context may be a different allocation from here
                    // on — `WelsEncoderApplyLTR` runs the uninit/init pair — and
                    // this arm reads nothing after it, so there is nothing to
                    // re-derive. The stale pointer is unreachable, not tolerated.
                }
                EncoderOption::ENCODER_OPTION_ENABLE_SSEI => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const bool);
                    ctx.param_mut().bEnableSSEI = iValue;
                }
                EncoderOption::ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const bool);
                    ctx.param_mut().bPrefixNalAddingCtrl = iValue;
                }
                EncoderOption::ENCODER_OPTION_SPS_PPS_ID_STRATEGY => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const i32);
                    let mut eNewStrategy = CONSTANT_ID;
                    match iValue {
                        0 => eNewStrategy = CONSTANT_ID,
                        0x01 => eNewStrategy = INCREASING_ID,
                        0x02 => eNewStrategy = SPS_LISTING,
                        0x03 => eNewStrategy = SPS_LISTING_AND_PPS_INCREASING,
                        0x06 => eNewStrategy = SPS_PPS_LISTING,
                        // out of range: unchanged, and *not* an error in C++ —
                        // eNewStrategy stays CONSTANT_ID and the code below runs.
                        _ => {}
                    }

                    let eOld = ctx.param().eSpsPpsIdStrategy;
                    if ((eNewStrategy as i32 & SPS_LISTING as i32) != 0
                        || (eOld as i32 & SPS_LISTING as i32) != 0)
                        && eOld != eNewStrategy
                    {
                        // changing in the middle of call is NOT allowed for
                        // eSpsPpsIdStrategy > INCREASING_ID
                        return cmInitParaError;
                    }
                    let mut sConfig: SWelsSvcCodingParam =
                        *ctx.param();
                    sConfig.eSpsPpsIdStrategy = eNewStrategy;

                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // T8.B5: as in `ENCODER_OPTION_LTR` — nothing below reads the
                    // context in this arm, so there is nothing to re-derive.
                }
                EncoderOption::ENCODER_OPTION_CURRENT_PATH => {
                    // **D-dead-7** (the user, 2026-08-26, from F183). This arm stored
                    // `pOption` into `pSvcParam->pCurPath`, exactly as
                    // `welsEncoderExt.cpp:1076` does — and nothing in either tree ever
                    // read the field. The field is gone; the option id keeps returning
                    // success and now does nothing, which is observably identical to
                    // storing into storage no one reads. Same shape as
                    // `ENCODER_OPTION_DUMP_FILE` below.
                }
                EncoderOption::ENCODER_OPTION_DUMP_FILE => {
                    // The whole body is `#ifdef ENABLE_FRAME_DUMP` in C++, and
                    // ENABLE_FRAME_DUMP is not defined in the build this port
                    // tracks, so the case compiles to an empty success.
                }
                EncoderOption::ENCODER_OPTION_PROFILE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pProfileInfo = &*(pOption as *const SProfileInfo);
                    if (pProfileInfo.iLayer as i32) < SPATIAL_LAYER_0 as i32
                        || (pProfileInfo.iLayer as i32) > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    CheckProfileSetting(
                        log_ctx,
                        ctx.param_mut(),
                        pProfileInfo.iLayer as i32,
                        pProfileInfo.uiProfileIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_LEVEL => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pLevelInfo = &*(pOption as *const SLevelInfo);
                    if (pLevelInfo.iLayer as i32) < SPATIAL_LAYER_0 as i32
                        || (pLevelInfo.iLayer as i32) > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    CheckLevelSetting(
                        log_ctx,
                        ctx.param_mut(),
                        pLevelInfo.iLayer as i32,
                        pLevelInfo.uiLevelIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_NUMBER_REF => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const i32);
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    CheckReferenceNumSetting(log_ctx, ctx.param_mut(), iValue);
                }
                EncoderOption::ENCODER_OPTION_DELIVERY_STATUS => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pValue = &*(pOption as *const SDeliveryStatus);
                    ctx.bDeliveryFlag = pValue.bDeliveryFlag;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const i32);
                    ctx.param_mut().iComplexityMode = match iValue {
                        0 => EComplexityMode::LOW_COMPLEXITY,
                        1 => EComplexityMode::MEDIUM_COMPLEXITY,
                        _ => EComplexityMode::HIGH_COMPLEXITY,
                    };
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    // "this option is get-only!" — C++ warns and returns success.
                }
                EncoderOption::ENCODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const i32);
                    ctx.iStatisticsLogInterval = iValue;
                }
                EncoderOption::ENCODER_OPTION_IS_LOSSLESS_LINK => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let bValue = *(pOption as *const bool);
                    ctx.param_mut().bIsLosslessLink = bValue;
                }
                EncoderOption::ENCODER_OPTION_BITS_VARY_PERCENTAGE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let iValue = *(pOption as *const i32);
                    ctx.param_mut().iBitsVaryPercentage =
                        WELS_CLIP3(iValue, 0, 100);
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    let iRang = ctx.param().iBitsVaryPercentage;
                    WelsEncoderApplyBitVaryRang(
                        log_ctx,
                        ctx.param_mut(),
                        iRang,
                    );
                }
                EncoderOption::ENCODER_OPTION_TRACE_LEVEL => {
                    let level = pOption.cast::<u32>().read();
                    self.m_pWelsTrace.SetTraceLevel(level);
                    self.sync_log_ctx();
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK => {
                    let callback = pOption.cast::<WelsTraceCallback>().read();
                    self.m_pWelsTrace.SetTraceCallback(callback);
                    self.sync_log_ctx();
                }
                EncoderOption::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT => {
                    // **C-ABI**: the caller's opaque trace context, kept until it
                    // is replaced and handed back to the callback untouched. Never
                    // dereferenced by this crate.
                    let ctx = pOption.cast::<*mut c_void>().read();
                    self.m_pWelsTrace.SetTraceCallbackContext(crate::api::codec_api::TraceUserCtx::from_abi(ctx));
                    self.sync_log_ctx();
                }
                // C++ ends with `default: return cmInitParaError`. There is no
                // wildcard arm here on purpose: `SetOption` takes a typed
                // `ENCODER_OPTION`, so every id the reference can be handed is
                // one of the 32 variants above, and leaving the match exhaustive
                // turns "a new option was added and not handled" into a compile
                // error instead of the silent success this replaced.
            }
        }
        0
    }

    // **S11.16: `C-ABI`, not `port-raw`.** This was tagged as convertible port
    // work, and it is not: the parameter is the C interface's untyped
    // `void* pOption`, whose real type is named by `eOptionId` and by nothing
    // else the compiler can see. Each arm's cast *is* that enum's contract with
    // the application, `codec_api.rs`'s thunks pass the caller's pointer
    // straight through, and there is no Rust shape that removes the cast
    // without changing the exported interface — which is frozen (plan §7.1).
    // Same allow, honest category; it stops sitting in the conversion queue.
    // unsafe-cat: C-ABI
    #[allow(unsafe_code)]
    /// `pOption` is **C-ABI**, as in [`Self::SetOption`], with the blob written.
    pub fn GetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        // **B3.** The context resolves as a shared reference off its own slot rather
        // than through `ctx_ptr`'s raw: every use below is a read, so `&sWelsEncCtx`
        // is the whole requirement, and the null guard becomes the `None` arm and
        // answers the same `cmInitExpected`. `m_bInitialFlag` is a sibling field, so
        // the borrow and the flag read coexist by construction.
        let Some(pCtx) = self.m_pEncContext.as_deref() else {
            return cmInitExpected;
        };
        if !self.m_bInitialFlag {
            return cmInitExpected;
        }

        unsafe {
            match eOptionId {
                EncoderOption::ENCODER_OPTION_INTER_SPATIAL_PRED => {
                    // "this feature not supported at present" — log-only in C++,
                    // and still a success return.
                }
                EncoderOption::ENCODER_OPTION_DATAFORMAT => {
                    *(pOption as *mut i32) = self.m_iCspInternal;
                }
                EncoderOption::ENCODER_OPTION_IDR_INTERVAL => {
                    *(pOption as *mut i32) =
                        pCtx.param().uiIntraPeriod as i32;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let param_ext = pCtx.param().to_param_ext();
                    *(pOption as *mut SEncParamExt) = param_ext;
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE => {
                    pCtx.param()
                        .GetBaseParams(&mut *(pOption as *mut SEncParamBase));
                }
                EncoderOption::ENCODER_OPTION_FRAME_RATE => {
                    *(pOption as *mut f32) = pCtx.param().fMaxFrameRate;
                }
                EncoderOption::ENCODER_OPTION_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = pCtx.param().iTargetBitrate;
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = pCtx.param().sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = pCtx.param().iMaxBitrate;
                    } else if (pInfo.iLayer as i32) >= 0 && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER {
                        pInfo.iBitrate = pCtx.param().sSpatialLayers
                            [pInfo.iLayer as usize]
                            .iMaxSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    let pStatistics = &mut *(pOption as *mut crate::SEncoderStatistics);
                    let iLayerIdx =
                        (pCtx.param().iSpatialLayerNum - 1) as usize;
                    let pEncStats = &pCtx.sEncoderStatistics[iLayerIdx];

                    pStatistics.uiWidth = pEncStats.uiWidth;
                    pStatistics.uiHeight = pEncStats.uiHeight;
                    pStatistics.fAverageFrameSpeedInMs = pEncStats.fAverageFrameSpeedInMs;

                    // rate control related
                    pStatistics.fAverageFrameRate = pEncStats.fAverageFrameRate;
                    pStatistics.fLatestFrameRate = pEncStats.fLatestFrameRate;
                    pStatistics.uiBitRate = pEncStats.uiBitRate;
                    pStatistics.uiAverageFrameQP = pEncStats.uiAverageFrameQP;

                    pStatistics.uiInputFrameCount = pEncStats.uiInputFrameCount;
                    pStatistics.uiSkippedFrameCount = pEncStats.uiSkippedFrameCount;

                    pStatistics.uiResolutionChangeTimes = pEncStats.uiResolutionChangeTimes;
                    pStatistics.uiIDRReqNum = pEncStats.uiIDRReqNum;
                    pStatistics.uiIDRSentNum = pEncStats.uiIDRSentNum;
                    pStatistics.uiLTRSentNum = pEncStats.uiLTRSentNum;
                }
                EncoderOption::ENCODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    *(pOption as *mut i32) = pCtx.iStatisticsLogInterval;
                }
                EncoderOption::ENCODER_OPTION_COMPLEXITY => {
                    *(pOption as *mut i32) =
                        pCtx.param().iComplexityMode as i32;
                }
                // NOTE: C++'s GetOption has **no** ENCODER_OPTION_TRACE_LEVEL case —
                // it is set-only, and a get falls to `default: return cmInitParaError`.
                // This port used to answer it, which accepted a call the reference
                // rejects.
                _ => return cmInitParaError,
            }
        }
        0
    }
}
