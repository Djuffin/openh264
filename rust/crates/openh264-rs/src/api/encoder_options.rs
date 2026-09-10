//! `CWelsH264SVCEncoder::SetOption` / `GetOption` — the encoder's untyped
//! `void* pOption` boundary. `pOption`'s real type is named by `eOptionId`.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use std::ffi::c_void;

use crate::api::codec_api::EParameterSetStrategy::*;
use crate::api::codec_api::LAYER_NUM::*;
use crate::api::codec_api::RC_MODES::*;
use crate::api::codec_api::TraceUserCtx;
use crate::api::codec_api::{
    EComplexityMode, EncoderOption, SBitrateInfo, SEncParamBase, SEncParamExt,
};
use crate::common::wels_trace::{WELS_LOG_INFO, WelsLog, WelsTraceCallback};
use crate::encoder::param_svc::{MAX_SPATIAL_LAYER_NUM, SWelsSvcCodingParam, WELS_CLIP3};
use crate::encoder::rc::WelsRcInitFuncPointers;
use crate::encoder::ref_list_mgr_svc::{
    FilterLTRMarkingFeedback, FilterLTRRecoveryRequest, SLTRMarkingFeedback, SLTRRecoverRequest,
};
use crate::encoder::wels_encoder_ext::{
    CWelsH264SVCEncoder, CheckLevelSetting, CheckProfileSetting, CheckReferenceNumSetting,
    MAX_BIT_RATE, MAX_DEPENDENCY_LAYER, MAX_FRAME_RATE, MIN_BIT_RATE, MIN_FRAME_RATE,
    SDeliveryStatus, SLTRConfig, SLevelInfo, SProfileInfo, WelsEncoderApplyBitRate,
    WelsEncoderApplyBitVaryRang, WelsEncoderApplyFrameRate, WelsEncoderApplyLTR,
    WelsEncoderParamAdjust, cmInitExpected, cmInitParaError, cmResultSuccess, rc_mode_from_raw,
};

impl CWelsH264SVCEncoder {
    #[allow(unsafe_code)]
    /// # Safety
    ///
    /// `pOption` must point at a readable, aligned object of the type `eOptionId`
    /// names, live for the call. The three trace ids retain what they read: the
    /// callback and context are used by every later message, under
    /// [`crate::api::codec_api::Encoder::set_trace_callback`]'s contract.
    ///
    /// ```compile_fail,E0133
    /// # unsafe extern "C" fn sink(_: *mut std::ffi::c_void, _: i32, _: *const std::ffi::c_char) {}
    /// use openh264_rs::api::codec_api::{EncoderOption, WelsTraceCallback};
    /// use openh264_rs::encoder::wels_encoder_ext::CWelsH264SVCEncoder;
    /// let mut e = CWelsH264SVCEncoder::new();
    /// let mut cb: WelsTraceCallback = Some(sink);
    /// e.SetOption(EncoderOption::ENCODER_OPTION_TRACE_CALLBACK,
    ///             &mut cb as *mut _ as *mut std::ffi::c_void);
    /// ```
    pub unsafe fn SetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
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
                    // Unsupported feature: accepted, no effect.
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
                    if self.m_iMaxPicWidth != iTargetWidth || self.m_iMaxPicHeight != iTargetHeight
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
                    // `WelsEncoderParamAdjust` may replace the context, so the
                    // timestamp is copied out before the `&mut self` logging calls.
                    let ts = match self.m_pEncContext.as_deref() {
                        Some(ctx) => ctx.iLastStatisticsLogTs,
                        None => return cmInitExpected,
                    };
                    self.LogStatistics(ts, 0);
                }
                EncoderOption::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT => {
                    let sEncodingParam = *(pOption as *const SEncParamExt);
                    // Traced before the spatial-layer check, so rejected parameters
                    // are still echoed.
                    self.TraceParamInfo(&sEncodingParam);
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
                    if self.m_iMaxPicWidth != iTargetWidth || self.m_iMaxPicHeight != iTargetHeight
                    {
                        self.m_iMaxPicWidth = iTargetWidth;
                        self.m_iMaxPicHeight = iTargetHeight;
                    }
                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                    // `WelsEncoderParamAdjust` may replace the context, so the
                    // timestamp is copied out before the `&mut self` logging calls.
                    let ts = match self.m_pEncContext.as_deref() {
                        Some(ctx) => ctx.iLastStatisticsLogTs,
                        None => return cmInitExpected,
                    };
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
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2 | SPATIAL_LAYER_3 => {
                            ctx.param_mut().sSpatialLayers[pInfo.iLayer as usize].iSpatialBitrate =
                                iBitrate;
                        }
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyBitRate(log_ctx, ctx.param_mut(), pInfo.iLayer as i32) != 0 {
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
                        SPATIAL_LAYER_0 | SPATIAL_LAYER_1 | SPATIAL_LAYER_2 | SPATIAL_LAYER_3 => {
                            ctx.param_mut().sSpatialLayers[pInfo.iLayer as usize]
                                .iMaxSpatialBitrate = iBitrate;
                        }
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyBitRate(log_ctx, ctx.param_mut(), pInfo.iLayer as i32) != 0 {
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
                    // Re-point the dispatch table; the field alone leaves the
                    // encoder running the previous mode's callbacks.
                    let iRCMode = ctx.param().iRCMode;
                    WelsRcInitFuncPointers(&mut ctx.func_list_mut().pfRc, iRCMode);
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
                    // rc off: the setting is accepted and ignored.
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
                    // The second argument points into the caller's memory, not the context.
                    FilterLTRRecoveryRequest(ctx, pLTR_Recover_Request);
                }
                EncoderOption::ENCODER_LTR_MARKING_FEEDBACK => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let fb = &mut *(pOption as *mut SLTRMarkingFeedback);
                    // As the recovery-request arm above — `pOption` is the caller's.
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
                    let pLTRValue = &mut *(pOption as *mut SLTRConfig);
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    if WelsEncoderApplyLTR(log_ctx, &mut self.m_pEncContext, pLTRValue) != 0 {
                        return cmInitParaError;
                    }
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
                        // Out of range: eNewStrategy stays CONSTANT_ID, not an error.
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
                    let mut sConfig: SWelsSvcCodingParam = *ctx.param();
                    sConfig.eSpsPpsIdStrategy = eNewStrategy;

                    if WelsEncoderParamAdjust(&mut self.m_pEncContext, &mut sConfig) != 0 {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_CURRENT_PATH => {
                    // Accepted and ignored: the path is never read.
                }
                EncoderOption::ENCODER_OPTION_DUMP_FILE => {
                    // Frame dumping is not compiled in: accepted, no effect.
                }
                EncoderOption::ENCODER_OPTION_PROFILE => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pProfileInfo = &*(pOption as *const SProfileInfo);
                    if pProfileInfo.iLayer < SPATIAL_LAYER_0 as i32
                        || pProfileInfo.iLayer > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    CheckProfileSetting(
                        log_ctx,
                        ctx.param_mut(),
                        pProfileInfo.iLayer,
                        pProfileInfo.uiProfileIdc,
                    );
                }
                EncoderOption::ENCODER_OPTION_LEVEL => {
                    let Some(ctx) = self.m_pEncContext.as_deref_mut() else {
                        return cmInitExpected;
                    };
                    let pLevelInfo = &*(pOption as *const SLevelInfo);
                    if pLevelInfo.iLayer < SPATIAL_LAYER_0 as i32
                        || pLevelInfo.iLayer > SPATIAL_LAYER_3 as i32
                    {
                        return cmInitParaError;
                    }
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    CheckLevelSetting(
                        log_ctx,
                        ctx.param_mut(),
                        pLevelInfo.iLayer,
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
                    // Get-only: accepted, no effect.
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
                    ctx.param_mut().iBitsVaryPercentage = WELS_CLIP3(iValue, 0, 100);
                    let log_ctx = self.m_pWelsTrace.m_sLogCtx;
                    let iRang = ctx.param().iBitsVaryPercentage;
                    WelsEncoderApplyBitVaryRang(log_ctx, ctx.param_mut(), iRang);
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
                    // The caller's opaque trace context: kept until replaced, handed
                    // back to the callback untouched, never dereferenced here.
                    let ctx = pOption.cast::<*mut c_void>().read();
                    self.m_pWelsTrace
                        .SetTraceCallbackContext(TraceUserCtx::from_abi(ctx));
                    self.sync_log_ctx();
                } // Exhaustive: a new option id must be handled here.
            }
        }
        0
    }

    #[allow(unsafe_code)]
    /// # Safety
    ///
    /// As [`Self::SetOption`], with `pOption` written rather than read.
    pub unsafe fn GetOption(&mut self, eOptionId: EncoderOption, pOption: *mut c_void) -> i32 {
        if pOption.is_null() {
            return cmInitParaError;
        }
        let Some(pCtx) = self.m_pEncContext.as_deref() else {
            return cmInitExpected;
        };
        if !self.m_bInitialFlag {
            return cmInitExpected;
        }

        unsafe {
            match eOptionId {
                EncoderOption::ENCODER_OPTION_INTER_SPATIAL_PRED => {
                    // Unsupported feature: accepted, no effect.
                }
                EncoderOption::ENCODER_OPTION_DATAFORMAT => {
                    *(pOption as *mut i32) = self.m_iCspInternal;
                }
                EncoderOption::ENCODER_OPTION_IDR_INTERVAL => {
                    *(pOption as *mut i32) = pCtx.param().uiIntraPeriod as i32;
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
                    } else if (pInfo.iLayer as i32) >= 0
                        && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER
                    {
                        pInfo.iBitrate =
                            pCtx.param().sSpatialLayers[pInfo.iLayer as usize].iSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_MAX_BITRATE => {
                    let pInfo = &mut *(pOption as *mut SBitrateInfo);
                    if pInfo.iLayer == SPATIAL_LAYER_ALL {
                        pInfo.iBitrate = pCtx.param().iMaxBitrate;
                    } else if (pInfo.iLayer as i32) >= 0
                        && (pInfo.iLayer as i32) < MAX_DEPENDENCY_LAYER
                    {
                        pInfo.iBitrate =
                            pCtx.param().sSpatialLayers[pInfo.iLayer as usize].iMaxSpatialBitrate;
                    } else {
                        return cmInitParaError;
                    }
                }
                EncoderOption::ENCODER_OPTION_GET_STATISTICS => {
                    let pStatistics = &mut *(pOption as *mut crate::SEncoderStatistics);
                    let iLayerIdx = (pCtx.param().iSpatialLayerNum - 1) as usize;
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
                    *(pOption as *mut i32) = pCtx.param().iComplexityMode as i32;
                }
                // Trace level is set-only; a get falls to the error arm below.
                _ => return cmInitParaError,
            }
        }
        0
    }
}
