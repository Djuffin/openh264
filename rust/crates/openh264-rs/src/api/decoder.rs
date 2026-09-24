// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:
//
// * Redistributions of source code must retain the above copyright notice, this
//   list of conditions and the following disclaimer.
//
// * Redistributions in binary form must reproduce the above copyright notice, this
//   list of conditions and the following disclaimer in the documentation and/or
//   other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
// ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Safe Rust H.264 / SVC Decoder core (`Decoder`) and display reordering buffer.
//!
//! Ported from `welsDecoderExt.cpp`. Used by both safe Rust callers and the raw C ABI (`c_api`).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use std::ffi::{c_long, c_void};
use std::ptr;

use super::types::{
    CM_INIT_PARA_ERROR, CM_RESULT_SUCCESS, DECODING_STATE, ERROR_CON_IDC, SBufferInfo,
    SDecoderStatistics, SDecodingParam, SParserBsInfo, SVuiSarInfo, TraceUserCtx,
    VIDEO_BITSTREAM_DEFAULT, VIDEO_BITSTREAM_TYPE, WelsTraceCallback,
};
use crate::decoder::decoder_context::{
    LIST_0, MAX_DPB_COUNT, PICT_INFO_LIST_SIZE, parser_bs, pic_pool_ptr, prev_dpb_id,
    prev_dpb_pic_mut, slice_header_of,
};
use crate::decoder::decoder_core::{
    ERR_NONE, OutputStatisticsLog, ResetDecStatNums, WelsDecoderLastDecPicInfoDefaults,
    WelsDecoderSpsPpsDefaults, WelsInitStaticMemory,
};
use crate::decoder::nalu::{EWelsNalUnitType, IS_PARAM_SETS_NALS};
use crate::decoder::pic_queue::SPicBuff;

/// The H.264 decoder, as a Rust type.
///
/// Owns the decoder context and the trace object; every method below is safe and
/// takes references and slices.
///
/// The one thing that cannot be a Rust type is the output window: a decoded frame
/// is handed back as three plane pointers into this decoder's own picture buffer,
/// valid until the next call on it. That is `codec_api.h`'s contract and the
/// methods that return it say so.
pub struct Decoder {
    /// The allocation root for the decoder context; every teardown site is `take()`.
    /// One context per decoder, so the context owns the reordering buffer, the
    /// statistics block and the vlc table.
    pub(crate) ctx: Option<Box<crate::decoder::decoder_core::SWelsDecoderContext>>,
    /// The trace object the context's own `sLogCtx` names.
    pub(crate) trace: Box<crate::common::wels_trace::welsCodecTrace>,
    /// This object's own record of `DECODER_OPTION_END_OF_STREAM`, which `GetOption`
    /// reads back.
    pub(crate) end_of_stream: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE)`
/// — `decoder.cpp:654` and `welsDecoderExt.cpp:528`, one function.
///
/// The clamp runs on the wire integer, which is an `int` there and an eight-variant
/// enum here.
pub(crate) fn ec_idc_from_raw(raw: i32) -> ERROR_CON_IDC {
    match raw.clamp(
        ERROR_CON_IDC::ERROR_CON_DISABLE as i32,
        ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE as i32,
    ) {
        0 => ERROR_CON_IDC::ERROR_CON_DISABLE,
        1 => ERROR_CON_IDC::ERROR_CON_FRAME_COPY,
        2 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY,
        3 => ERROR_CON_IDC::ERROR_CON_FRAME_COPY_CROSS_IDR,
        4 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR,
        5 => ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
        6 => ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR,
        _ => ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
    }
}

/// `VIDEO_BITSTREAM_SVC`/`VIDEO_BITSTREAM_AVC` pass; anything else is
/// `VIDEO_BITSTREAM_DEFAULT` — `decoder.cpp:667–671`'s `else`.
pub(crate) fn video_bs_type_from_raw(raw: i32) -> VIDEO_BITSTREAM_TYPE {
    match raw {
        0 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
        1 => VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC,
        _ => VIDEO_BITSTREAM_DEFAULT,
    }
}

impl Decoder {
    /// `CWelsDecoder`'s constructor — `welsDecoderExt.cpp:155`.
    pub fn new() -> Self {
        Self {
            ctx: None,
            trace: Box::new(crate::common::wels_trace::welsCodecTrace::new()),
            end_of_stream: false,
        }
    }

    /// `CWelsDecoder::Initialize` — `welsDecoderExt.cpp:260`.
    ///
    /// Returns `cmResultSuccess` or a `CM_*` code. The caller's block has already
    /// been sanitised by the time it gets here: the range clamp and the bitstream
    /// type's normalisation run on the wire values, at the thunk.
    pub fn initialize(&mut self, pParam: &SDecodingParam) -> c_long {
        // A second `Initialize` on a live decoder rebuilds: the previous session's
        // reordering buffer, statistics, last decoded-picture record and decode
        // timestamps do not survive the teardown.
        if let Some(mut pCtx) = self.ctx.take() {
            crate::decoder::decoder_core::WelsEndDecoder(&mut pCtx);
        }
        {
            // In-place heap construction: the context is several MiB and owns `Vec`s,
            // so neither `Box::default()` (stack round-trip) nor
            // `new_zeroed().assume_init()` (invalid zeroed `Vec`) is usable.
            let mut ctx_box = crate::decoder::decoder_context::SWelsDecoderContext::new_boxed();
            // The caller's parameters go in before `WelsDecoderDefaults`, because
            // everything built below this line may read them.
            ctx_box.pParam = *pParam;
            // These defaults are not zeros: `iPrevFrameNum` starts at -1.
            WelsDecoderLastDecPicInfoDefaults(&mut ctx_box.pLastDecPicInfo);
            // A fresh reordering buffer. `IMinInt32` in every slot's `iPOC` is what
            // "empty" is; zeroes are a valid POC.
            let crate::decoder::decoder_core::SWelsDecoderContext {
                pPictReoderingStatus,
                pPictInfoList,
                ..
            } = &mut *ctx_box;
            crate::decoder::decoder_core::ResetReorderingPictureBuffers(
                pPictReoderingStatus,
                pPictInfoList,
                true,
            );
            let log_ctx = self.trace.log_context();
            crate::decoder::decoder_core::WelsDecoderDefaults(&mut ctx_box, Some(&log_ctx));
            WelsDecoderSpsPpsDefaults(&mut ctx_box.sSpsPpsCtx);
            if WelsInitStaticMemory(&mut ctx_box) != 0 {
                // The failure path is the `Box` going out of scope.
                return CM_INIT_PARA_ERROR as c_long;
            }
            self.ctx = Some(ctx_box);
        }

        // `DecoderConfigParam` runs on every `Initialize`, and its `InitErrorCon` does
        // two things nothing else does:
        //
        //  * clears `bFreezeOutput`, which `WelsDecoderDefaults` sets true. The only
        //    other site that clears it is the "complete non-ECed IDR" arm of
        //    `DecodeFrameConstruction`, so without it a stream whose first IDR is
        //    missing or damaged stays frozen and emits nothing until a clean IDR
        //    arrives.
        //  * installs `sCopyFunc`'s two kernels. `DoErrorConSliceCopy` and
        //    `DoErrorConSliceMVCopy` guard every copy with `if let Some(f)`, so with
        //    the table `None` the slice-copy concealment runs and copies nothing.
        if let Some(pCtx) = self.ctx.as_mut() {
            crate::decoder::decoder_core::DecoderConfigParam(pCtx, pParam);
        }

        CM_RESULT_SUCCESS as c_long
    }

    /// `CWelsDecoder::Uninitialize` — `welsDecoderExt.cpp:279`.
    pub fn uninitialize(&mut self) -> c_long {
        if let Some(mut pCtx) = self.ctx.take() {
            crate::decoder::decoder_core::WelsEndDecoder(&mut pCtx);
        }
        CM_RESULT_SUCCESS as c_long
    }

    /// The number of pictures the display-reordering buffer is holding —
    /// `DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER`.
    pub fn frames_remaining(&self) -> i32 {
        self.ctx
            .as_ref()
            .map_or(0, |pCtx| pCtx.pPictReoderingStatus.iNumOfPicts)
    }

    /// `DECODER_OPTION_END_OF_STREAM`, both ways.
    pub fn end_of_stream(&self) -> bool {
        self.end_of_stream
    }

    pub fn set_end_of_stream(&mut self, eos: bool) {
        self.end_of_stream = eos;
        if let Some(pCtx) = self.ctx.as_mut() {
            pCtx.bEndOfStreamFlag = eos;
        }
    }

    /// The concealment mode the decoder is actually running with — the context's
    /// own `pParam`, which is the block `SetOption` writes.
    pub fn error_concealment(&self) -> Option<ERROR_CON_IDC> {
        self.ctx.as_ref().map(|pCtx| pCtx.pParam.eEcActiveIdc)
    }

    /// `welsDecoderExt.cpp:521–539`: clamp the caller's `int`, refuse it outright
    /// when parse-only is on, then store it and re-run `InitErrorCon` — the mode
    /// selects which kernels `sCopyFunc` holds.
    pub fn set_error_concealment(&mut self, raw: i32) -> c_long {
        let val = ec_idc_from_raw(raw);
        let Some(pCtx) = self.ctx.as_mut() else {
            return DECODING_STATE::dsInitialOptExpected.0 as c_long;
        };
        if pCtx.pParam.bParseOnly && val != ERROR_CON_IDC::ERROR_CON_DISABLE {
            return CM_INIT_PARA_ERROR as c_long;
        }
        pCtx.pParam.eEcActiveIdc = val;
        crate::decoder::error_concealment::InitErrorCon(pCtx);
        CM_RESULT_SUCCESS as c_long
    }

    /// `DECODER_OPTION_GET_STATISTICS` — `welsDecoderExt.cpp:639`. The two speed
    /// fields are computed at read time, as there.
    pub fn statistics(&self) -> Option<SDecoderStatistics> {
        let pCtx = self.ctx.as_ref()?;
        let mut out = pCtx.pDecoderStatistics;
        if out.uiDecodedFrameCount != 0 {
            out.fAverageFrameSpeedInMs =
                (pCtx.dDecTime / f64::from(out.uiDecodedFrameCount)) as f32;
            out.fActualAverageFrameSpeedInMs = (pCtx.dDecTime
                / f64::from(
                    out.uiDecodedFrameCount
                        .wrapping_add(out.uiFreezingIDRNum)
                        .wrapping_add(out.uiFreezingNonIDRNum),
                )) as f32;
        }
        Some(out)
    }

    /// The trace destination, as typed setters rather than an option blob. Each
    /// pushes the settings into the context's copy of the log context.
    pub fn set_trace_level(&mut self, level: u32) {
        self.trace.SetTraceLevel(level);
        self.sync_log_ctx();
    }

    /// # Safety
    ///
    /// As [`crate::api::encoder::Encoder::set_trace_callback`], for this decoder's lifetime.
    #[allow(unsafe_code)]
    pub unsafe fn set_trace_callback(&mut self, callback: WelsTraceCallback) {
        self.trace.SetTraceCallback(callback);
        self.sync_log_ctx();
        crate::common::wels_trace::WelsLog(
            self.trace.m_sLogCtx,
            crate::common::wels_trace::WELS_LOG_INFO,
            "CWelsDecoder::SetOption():DECODER_OPTION_TRACE_CALLBACK callback set.",
        );
    }

    // -----------------------------------------------------------------------
    // `CWelsDecoder::GetOption`'s 16 arms — `welsDecoderExt.cpp:584-695`.
    //
    // The option-id dispatch, the pointer types and the error codes stay in the
    // thunk, which is where the C interface's rules live.
    // -----------------------------------------------------------------------

    /// Whether the context exists yet. `GetOption` answers `cmInitExpected` for
    /// every id but `NUM_OF_THREADS` when it does not (`welsDecoderExt.cpp:589`).
    pub fn has_ctx(&self) -> bool {
        self.ctx.is_some()
    }

    /// `DECODER_OPTION_VCL_NAL` — `:621-624`.
    pub fn feedback_vcl_nal(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFeedbackVclNalInAu)
    }

    /// `DECODER_OPTION_TEMPORAL_ID` — `:625-628`.
    pub fn feedback_temporal_id(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFeedbackTidInAu)
    }

    /// `DECODER_OPTION_IS_REF_PIC` — `:629-634`. The stored `nal_ref_idc` is clamped
    /// to 0/1 on the way out, but −1 is left alone.
    pub fn feedback_is_ref_pic(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| {
            let iVal = pCtx.iFeedbackNalRefIdc;
            if iVal > 0 { 1 } else { iVal }
        })
    }

    /// `DECODER_OPTION_FRAME_NUM` — `:608-611`, under `LONG_TERM_REF`.
    pub fn frame_num(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFrameNum)
    }

    /// `DECODER_OPTION_IDR_PIC_ID` — `:603-607`, under `LONG_TERM_REF`. The field is
    /// a `uint16_t` and crosses as the `int` the option id names.
    pub fn cur_idr_pic_id(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| i32::from(pCtx.uiCurIdrPicId))
    }

    /// `DECODER_OPTION_LTR_MARKING_FLAG` — `:612-615`.
    pub fn ltr_marking_flag(&self) -> Option<i32> {
        self.ctx
            .as_ref()
            .map(|pCtx| i32::from(pCtx.bCurAuContainLtrMarkSeFlag))
    }

    /// `DECODER_OPTION_LTR_MARKED_FRAME_NUM` — `:616-619`.
    pub fn ltr_marked_frame_num(&self) -> Option<i32> {
        self.ctx.as_ref().map(|pCtx| pCtx.iFrameNumOfAuMarkedLtr)
    }

    /// `DECODER_OPTION_PROFILE` / `DECODER_OPTION_LEVEL` — `:673-687`. Both return
    /// `cmInitExpected` when no SPS has been activated yet, which the outer `Option`
    /// carries; the inner one is the context.
    pub fn active_sps_profile(&self) -> Option<Option<i32>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
                .map(|sps| i32::from(sps.uiProfileIdc))
        })
    }

    pub fn active_sps_level(&self) -> Option<Option<i32>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps)
                .map(|sps| i32::from(sps.uiLevelIdc))
        })
    }

    /// `DECODER_OPTION_GET_SAR_INFO` — `:664-672`. The caller's struct is zeroed and
    /// then filled from the active SPS's VUI, so a stream whose VUI carries no aspect
    /// ratio reads back zeros rather than the previous stream's.
    pub fn sar_info(&self) -> Option<Option<SVuiSarInfo>> {
        self.ctx.as_ref().map(|pCtx| {
            crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).map(
                |sps| SVuiSarInfo {
                    uiSarWidth: sps.sVui.uiSarWidth,
                    uiSarHeight: sps.sVui.uiSarHeight,
                    bOverscanAppropriateFlag: sps.sVui.bOverscanAppropriateFlag,
                },
            )
        })
    }

    /// `DECODER_OPTION_STATISTICS_LOG_INTERVAL`, both ways — `:653-659` and
    /// `:571-577`.
    pub fn statistics_log_interval(&self) -> Option<u32> {
        self.ctx
            .as_ref()
            .map(|pCtx| pCtx.pDecoderStatistics.iStatisticsLogInterval)
    }

    pub fn set_statistics_log_interval(&mut self, interval: u32) -> bool {
        match self.ctx.as_mut() {
            Some(pCtx) => {
                pCtx.pDecoderStatistics.iStatisticsLogInterval = interval;
                true
            }
            None => false,
        }
    }

    /// `CWelsDecoder::Initialize`'s null-parameter arm — `welsDecoderExt.cpp:266-268`.
    ///
    /// On the impl because the message needs the trace object; the thunk only has to
    /// notice the null.
    pub(crate) fn report_init_null_param(&mut self) -> c_long {
        crate::common::wels_trace::WelsLog(
            self.trace.m_sLogCtx,
            crate::common::wels_trace::WELS_LOG_ERROR,
            "CWelsDecoder::Initialize(), invalid input argument.",
        );
        CM_INIT_PARA_ERROR as c_long
    }

    /// # Safety
    ///
    /// `ctx` is handed back to the trace callback on every message until it is
    /// replaced or this decoder is dropped, so it must stay valid for that long.
    /// It is the caller's, and this crate never dereferences it.
    #[allow(unsafe_code)]
    pub unsafe fn set_trace_callback_context(&mut self, ctx: *mut c_void) {
        self.trace
            .SetTraceCallbackContext(TraceUserCtx::from_abi(ctx));
        self.sync_log_ctx();
    }

    /// `CWelsDecoder::DecodeFrame2WithCtx` — `welsDecoderExt.cpp:735`.
    ///
    /// `src` is one access unit, or `None` for the end-of-stream flush that
    /// `(NULL, 0)` means on the C interface.
    ///
    /// The output window: when `pDstInfo.iBufferStatus == 1`, `ppDst[0..3]` and
    /// `pDstInfo.pDst[0..3]` name planes inside this decoder's picture buffer with
    /// the strides `pDstInfo.UsrData` reports. They stay valid until the next call on
    /// this decoder and are not the caller's to free, which is why they are pointers
    /// here and not slices.
    pub fn decode(
        &mut self,
        src: Option<&[u8]>,
        ppDst: &mut [*mut u8; 3],
        pDstInfo: &mut SBufferInfo,
    ) -> DECODING_STATE {
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            return DECODING_STATE::dsInitialOptExpected;
        };
        p_ctx.iErrorCode = DECODING_STATE::dsErrorFree.0;
        // `dDecTime` is a millisecond accumulator; its one reader is
        // `DECODER_OPTION_GET_STATISTICS`'s two speed fields.
        let dec_started = std::time::Instant::now();

        // ------------------------------------------------------------------
        // The per-call reset block — `welsDecoderExt.cpp:784-811`. Every field here is
        // read back through `GetOption`.
        ppDst[0] = ptr::null_mut();
        ppDst[1] = ptr::null_mut();
        ppDst[2] = ptr::null_mut();
        p_ctx.iFeedbackVclNalInAu = crate::decoder::decoder_core::FEEDBACK_UNKNOWN_NAL;
        // `:789-793`: the whole `SBufferInfo` is zeroed and only `uiInBsTimeStamp`
        // survives, being the caller's input on this slot.
        let uiInBsTimeStamp = pDstInfo.uiInBsTimeStamp;
        *pDstInfo = SBufferInfo::default();
        pDstInfo.uiInBsTimeStamp = uiInBsTimeStamp;
        // `:795-800`, under `LONG_TERM_REF`.
        p_ctx.bReferenceLostAtT0Flag = false;
        p_ctx.bCurAuContainLtrMarkSeFlag = false;
        p_ctx.iFrameNumOfAuMarkedLtr = 0;
        p_ctx.iFrameNum = -1;
        // `:804-805`.
        p_ctx.iFeedbackTidInAu = -1;
        p_ctx.iFeedbackNalRefIdc = -1;
        // `:807-811`. `pDstInfo` is a reference here: `decoder_decode_frame2_c` has
        // already returned `dsInitialOptExpected` for a null.
        pDstInfo.uiOutYuvTimeStamp = 0;
        p_ctx.uiTimeStamp = uiInBsTimeStamp;

        if let Some(src) = src {
            p_ctx.bEndOfStreamFlag = false;
            if crate::decoder::decoder_core::GetThreadCount(&*p_ctx) <= 0 {
                p_ctx.uiDecodeTimeStamp += 1;
                p_ctx.uiDecodingTimeStamp = p_ctx.uiDecodeTimeStamp;
            }
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                src,
                src.len() as i32,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        } else {
            // `WelsDecodeBs` runs on both arms, so `DecodeFrame2 (NULL, 0, …)` always
            // reconstructs and this arm is not gated on end of stream:
            // `DecodeFrameNoDelay`'s second call is a null call made before it.
            p_ctx.bEndOfStreamFlag = true;
            // `DecodeFrameConstruction` reads this flag: with it set, an incomplete
            // frame returns `ERR_INFO_MB_NUM_INADEQUATE` instead of falling through to
            // the output path and emitting a partial frame.
            p_ctx.bInstantDecFlag = true;
            crate::decoder::decoder_core::WelsDecodeBs(
                &mut *p_ctx,
                &[],
                0,
                ppDst,
                pDstInfo,
                ptr::null_mut(),
            );
        }
        p_ctx.bInstantDecFlag = false; // reset no-delay flag

        // ------------------------------------------------------------------
        // The error-reporting block — `welsDecoderExt.cpp:815–891`.
        // ------------------------------------------------------------------
        if p_ctx.iErrorCode != 0 {
            let eNalType = p_ctx.sCurNalHead.eNalUnitType;

            // `:820–831` — the two reset arms, which differ only in the code they
            // report. `ResetDecoder` (`:439`) saves the parameter block and rebuilds
            // the context over it; the rebuilt buffers are already at their
            // constructor defaults.
            let reset_code = if p_ctx.iErrorCode & crate::decoder::decoder_core::dsOutOfMemory != 0
            {
                Some(DECODING_STATE::dsOutOfMemory)
            } else if p_ctx.iErrorCode & crate::decoder::decoder_core::dsRefListNullPtrs != 0 {
                Some(DECODING_STATE::dsRefListNullPtrs)
            } else {
                None
            };
            if let Some(code) = reset_code {
                let sPrevParam = p_ctx.pParam;
                crate::decoder::decoder_core::WelsLog(
                    p_ctx.sLogCtx,
                    crate::decoder::decoder_core::WELS_LOG_INFO,
                    &format!("ResetDecoder(), context error code is {}", p_ctx.iErrorCode),
                );
                let _ = self.initialize(&sPrevParam);
                pDstInfo.iBufferStatus = 0;
                return code;
            }

            // `:833–842` — on an AVC bitstream, or on an error in a parameter set or
            // IDR NAL, the upper layer is notified of key frame loss. This is
            // `eVideoType`'s one reader, and `bParamSetsLostFlag` is what
            // `UpdateAccessUnit`'s mosaic-avoidance block reads.
            if IS_PARAM_SETS_NALS(eNalType)
                || eNalType == EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
                || p_ctx.eVideoType == VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC
            {
                if p_ctx.pParam.eEcActiveIdc == ERROR_CON_IDC::ERROR_CON_DISABLE {
                    p_ctx.bParamSetsLostFlag = true;
                }
            }

            // `:844–854` — the trace throttle. One line per error burst, then a
            // counter, so a stream that fails on every access unit does not fill
            // the caller's log. `bPrintFrameErrorTraceFlag` is re-armed by
            // `DecodeFrameConstruction` on a complete frame.
            if p_ctx.bPrintFrameErrorTraceFlag {
                crate::decoder::decoder_core::WelsLog(
                    p_ctx.sLogCtx,
                    crate::decoder::decoder_core::WELS_LOG_INFO,
                    &format!("decode failed, failure type:{} \n", p_ctx.iErrorCode),
                );
                p_ctx.bPrintFrameErrorTraceFlag = false;
            } else {
                p_ctx.iIgnoredErrorInfoPacketCount =
                    p_ctx.iIgnoredErrorInfoPacketCount.wrapping_add(1);
                if p_ctx.iIgnoredErrorInfoPacketCount == i32::MAX {
                    crate::decoder::decoder_core::WelsLog(
                        p_ctx.sLogCtx,
                        crate::decoder::decoder_core::WELS_LOG_WARNING,
                        "continuous error reached INT_MAX! Restart as 0.",
                    );
                    p_ctx.iIgnoredErrorInfoPacketCount = 0;
                }
            }

            // `:856–882` — concealment happened and the frame came out anyway. The
            // four counters behind it are what `DECODER_OPTION_GET_STATISTICS`
            // reports.
            if p_ctx.pParam.eEcActiveIdc != ERROR_CON_IDC::ERROR_CON_DISABLE
                && pDstInfo.iBufferStatus == 1
            {
                p_ctx.iErrorCode |= DECODING_STATE::dsDataErrorConcealed.0;

                let iMbConcealedNum = p_ctx.iMbEcedNum.wrapping_add(p_ctx.iMbEcedPropNum);
                let iMbNum = p_ctx.iMbNum;
                let iMbEcedPropNum = p_ctx.iMbEcedPropNum;
                let stat = &mut p_ctx.pDecoderStatistics;

                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                if stat.uiDecodedFrameCount == 0 {
                    // exceeded the max value of uint32_t
                    ResetDecStatNums(stat);
                    stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                }
                // The running average is de-normalised by the frame count, the new
                // frame's percentage added, and the whole re-normalised below.
                stat.uiAvgEcRatio = if iMbNum == 0 {
                    stat.uiAvgEcRatio.wrapping_mul(stat.uiEcFrameNum)
                } else {
                    stat.uiAvgEcRatio
                        .wrapping_mul(stat.uiEcFrameNum)
                        .wrapping_add((iMbConcealedNum.wrapping_mul(100) / iMbNum) as u32)
                };
                stat.uiAvgEcPropRatio = if iMbNum == 0 {
                    stat.uiAvgEcPropRatio.wrapping_mul(stat.uiEcFrameNum)
                } else {
                    stat.uiAvgEcPropRatio
                        .wrapping_mul(stat.uiEcFrameNum)
                        .wrapping_add((iMbEcedPropNum.wrapping_mul(100) / iMbNum) as u32)
                };
                stat.uiEcFrameNum = stat
                    .uiEcFrameNum
                    .wrapping_add(u32::from(iMbConcealedNum != 0));
                stat.uiAvgEcRatio = if stat.uiEcFrameNum == 0 {
                    0
                } else {
                    stat.uiAvgEcRatio / stat.uiEcFrameNum
                };
                stat.uiAvgEcPropRatio = if stat.uiEcFrameNum == 0 {
                    0
                } else {
                    stat.uiAvgEcPropRatio / stat.uiEcFrameNum
                };
            }
            p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
            OutputStatisticsLog(&mut *p_ctx);
            // `:885–890`.
            ReorderPicturesInDisplay(&mut *p_ctx, ppDst, pDstInfo);
            // The accumulated error code, whole.
            return DECODING_STATE(p_ctx.iErrorCode);
        }

        // `:894–905` — error free. This frame counter is the divisor
        // `DECODER_OPTION_GET_STATISTICS` reports its two speeds by.
        if pDstInfo.iBufferStatus == 1 {
            let stat = &mut p_ctx.pDecoderStatistics;
            stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
            if stat.uiDecodedFrameCount == 0 {
                ResetDecStatNums(stat);
                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
            }
            OutputStatisticsLog(&mut *p_ctx);
        }
        p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
        ReorderPicturesInDisplay(&mut *p_ctx, ppDst, pDstInfo);

        DECODING_STATE::dsErrorFree
    }

    /// `CWelsDecoder::DecodeParser` — `welsDecoderExt.cpp:1180-1262`.
    ///
    /// `src` is one access unit, or `None` for the `(NULL, 0)` end-of-stream call.
    ///
    /// The output window: on the call that completes a frame, `pDstInfo`'s
    /// `pNalLenInByte` and `pDstBuff` name this decoder's parse-only buffers —
    /// `iNalNum` lengths and their concatenated bytes — valid until the next call on
    /// this decoder and not the caller's to free, as with [`Self::decode`]'s planes.
    pub fn decode_parser(
        &mut self,
        src: Option<&[u8]>,
        pDstInfo: &mut SParserBsInfo,
    ) -> DECODING_STATE {
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_ERROR,
                "Call DecodeParser without Initialize.",
            );
            return DECODING_STATE::dsInitialOptExpected;
        };
        // `:1189-1193` — the mode check.
        if !p_ctx.pParam.bParseOnly {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_ERROR,
                "bParseOnly should be true for this API calling! \n",
            );
            p_ctx.iErrorCode |= DECODING_STATE::dsInvalidArgument.0;
            return DECODING_STATE::dsInvalidArgument;
        }
        let dec_started = std::time::Instant::now();

        if src.is_some() {
            p_ctx.bEndOfStreamFlag = false;
        } else {
            // "for CONSOLE MODE, when decoding LAST AU, kiSrcLen==0 && kpSrc==NULL"
            p_ctx.bEndOfStreamFlag = true;
            p_ctx.bInstantDecFlag = true;
        }

        p_ctx.iErrorCode = DECODING_STATE::dsErrorFree.0;
        // "add protection to disable EC here" (`:1216`).
        p_ctx.pParam.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_DISABLE;
        p_ctx.iFeedbackNalRefIdc = -1;
        if !p_ctx.bFramePending {
            // `:1219-1220`. Every slot is written by `pNalLenInByte[iNalNum++] = …`
            // before anything reads it, and the one reader sums `0..iNalNum`.
            if let Some(p) = parser_bs(&mut p_ctx.pParserBsInfo) {
                p.iNalNum = 0;
                p.pNalLenInByte.fill(0);
            }
        }
        pDstInfo.iNalNum = 0;
        pDstInfo.iSpsWidthInPixel = 0;
        pDstInfo.iSpsHeightInPixel = 0;
        p_ctx.uiTimeStamp = pDstInfo.uiInBsTimeStamp;
        pDstInfo.uiOutBsTimeStamp = 0;

        // `:1230`. The picture-output parameters are never reached in parse-only mode:
        // `DecodeFrameConstruction` returns out of its `bParseOnly` arm before the
        // reconstruction path touches either. Locals here; nothing reads them back.
        let mut ppDstUnused: [*mut u8; 3] = [ptr::null_mut(); 3];
        let mut sDstInfoUnused = SBufferInfo::default();
        let (bs, len): (&[u8], i32) = match src {
            Some(s) => (s, s.len() as i32),
            None => (&[], 0),
        };
        crate::decoder::decoder_core::WelsDecodeBs(
            &mut *p_ctx,
            bs,
            len,
            &mut ppDstUnused,
            &mut sDstInfoUnused,
            ptr::null_mut(),
        );

        // `:1231-1236` — out of memory rebuilds the decoder over the saved parameter
        // block, the rebuild being the recovery.
        if p_ctx.iErrorCode & crate::decoder::decoder_core::dsOutOfMemory != 0 {
            let sPrevParam = p_ctx.pParam;
            let _ = self.initialize(&sPrevParam);
            return DECODING_STATE::dsOutOfMemory;
        }

        // `:1238-1249` — the copy-out, field by field; the two raw pointers are minted
        // from the `Vec`s that own the bytes.
        let bFrameDone = !p_ctx.bFramePending;
        if bFrameDone {
            let filled = match parser_bs(&mut p_ctx.pParserBsInfo) {
                Some(p) if p.iNalNum != 0 => {
                    pDstInfo.iNalNum = p.iNalNum;
                    pDstInfo.pNalLenInByte = p.pNalLenInByte.as_mut_ptr();
                    pDstInfo.pDstBuff = p.pDstBuff.as_mut_ptr();
                    pDstInfo.iSpsWidthInPixel = p.iSpsWidthInPixel;
                    pDstInfo.iSpsHeightInPixel = p.iSpsHeightInPixel;
                    // Nothing writes the decoder-side `uiInBsTimeStamp`, so the
                    // caller's input timestamp is overwritten with zero on every
                    // completed frame.
                    pDstInfo.uiInBsTimeStamp = p.uiInBsTimeStamp;
                    pDstInfo.uiOutBsTimeStamp = p.uiOutBsTimeStamp;
                    true
                }
                _ => false,
            };
            if filled && p_ctx.iErrorCode == ERR_NONE {
                let stat = &mut p_ctx.pDecoderStatistics;
                stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                if stat.uiDecodedFrameCount == 0 {
                    ResetDecStatNums(stat);
                    stat.uiDecodedFrameCount = stat.uiDecodedFrameCount.wrapping_add(1);
                }
            }
        }

        p_ctx.bInstantDecFlag = false; // reset no-delay flag

        if p_ctx.iErrorCode != 0 && p_ctx.bPrintFrameErrorTraceFlag {
            crate::common::wels_trace::WelsLog(
                self.trace.m_sLogCtx,
                crate::common::wels_trace::WELS_LOG_INFO,
                &format!("decode failed, failure type:{} \n", p_ctx.iErrorCode),
            );
            p_ctx.bPrintFrameErrorTraceFlag = false;
        }
        p_ctx.dDecTime += dec_started.elapsed().as_secs_f64() * 1e3;
        DECODING_STATE(p_ctx.iErrorCode)
    }

    /// `CWelsDecoder::FlushFrame` — `welsDecoderExt.cpp:1094`: drains the display
    /// reordering buffer only. The decoder core itself is flushed by the caller
    /// through [`Self::decode`] with `None` after signalling end of stream.
    ///
    /// Same output window as [`Self::decode`].
    pub fn flush(
        &mut self,
        ppDst: &mut [*mut u8; 3],
        pDstInfo: &mut SBufferInfo,
    ) -> DECODING_STATE {
        // With no context there is no reordering state to drain: the buffers are the
        // context's own fields.
        let Some(p_ctx) = self.ctx.as_deref_mut() else {
            return DECODING_STATE::dsErrorFree;
        };
        if p_ctx.bEndOfStreamFlag && p_ctx.pPictReoderingStatus.iNumOfPicts > 0 {
            // `false`: drain the slot list without touching the live pool. See
            // `pool_for`.
            ReleaseBufferedReadyPictureReorder(&mut *p_ctx, false, ppDst, pDstInfo, true);
        }
        DECODING_STATE::dsErrorFree
    }

    fn sync_log_ctx(&mut self) {
        let log_ctx = self.trace.log_context();
        if let Some(pCtx) = self.ctx.as_mut() {
            pCtx.sLogCtx = log_ctx;
        }
    }
}

/// `pCtx ? pCtx->pPicBuff : m_pPicBuff` — the C++'s pool selection, as a flag.
///
/// The ternary carries exactly one bit: `FlushFrame` passes a null context to say "do
/// not touch the live pool" (`welsDecoderExt.cpp:1103`), and every other caller passes
/// the real one. That bit is `bUsePool`, and spelling it as a bool is what lets the
/// reordering state live in the context — a null context argument cannot carry the
/// state the callee reads out of it.
#[inline]
fn pool_for(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
) -> Option<&mut SPicBuff> {
    if !bUsePool {
        return None;
    }
    pic_pool_ptr(&mut pCtx.pPicBuff)
}

/// Matches `void CWelsDecoder::UpdateReorderingParameters (...)`.
///
/// Refreshes, from the active SPS, the three things the display layer needs to put
/// pictures into output order: whether this stream reorders at all, how many frames
/// its DPB holds, and what its VUI promises about reordering. `false` when no SPS is
/// active yet — the C's `if (pDecContext->pSps != NULL)` guard.
fn UpdateReorderingParameters(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
) -> bool {
    let Some(sps) =
        crate::decoder::decoder_context::active_sps(&pCtx.sSpsPpsCtx, pCtx.active_sps).copied()
    else {
        return false;
    };
    pCtx.bIsBaseline = sps.uiProfileIdc == 66 || sps.uiProfileIdc == 83;
    let st = &mut pCtx.pPictReoderingStatus;
    st.bReorderPictures = crate::decoder::decoder_core::NeedsPictureReordering(&sps);
    st.iDpbSize = crate::decoder::decoder_core::GetDpbSize(&sps);
    st.iMaxNumReorderFrames = if sps.sVui.bBitstreamRestrictionFlag {
        sps.sVui.uiMaxNumReorderFrames as i32
    } else {
        -1
    };
    true
}

/// Matches `int32_t CWelsDecoder::GetDpbFullness (...)`.
///
/// DPB fullness in frame buffers, C.4.1: `|R| + |W \ R|`, where `R` is the set of
/// pictures the core holds as reference right now and `W` the set waiting here for
/// output. A picture that is both is one frame buffer, not two; `iPicBuffIdx` names
/// the pool slot on both sides.
///
/// Marking for the current picture has already run when this is reached —
/// `DecodeCurrentAccessUnit` calls `DecodeFrameConstruction` and then `WelsMarkAsRef`
/// before returning to this layer — so `R` already holds the current picture if it
/// is a reference, and `W` holds it because it has just been buffered. The count is
/// therefore one more than the fullness C.4.5.3 tests before storing the current
/// picture, which is why the caller's test is `> iDpbSize` and not `>=`.
fn GetDpbFullness(pCtx: &crate::decoder::decoder_core::SWelsDecoderContext) -> i32 {
    // The distinct pool slots held as reference, by `iPicBuffIdx`. Short and long
    // lists can name the same picture, so they are unioned rather than added.
    let mut aiRefBuffIdx = [-1i32; 2 * MAX_DPB_COUNT];
    let mut iRefs = 0usize;
    let pPicBuff = pCtx.pPicBuff.as_deref();
    for iList in 0..2 {
        let (pList, kuiCount) = if iList == 0 {
            (
                &pCtx.sRefPic.pShortRefList[LIST_0],
                pCtx.sRefPic.uiShortRefCount[LIST_0],
            )
        } else {
            (
                &pCtx.sRefPic.pLongRefList[LIST_0],
                pCtx.sRefPic.uiLongRefCount[LIST_0],
            )
        };
        for slot in pList.iter().take((kuiCount as usize).min(MAX_DPB_COUNT)) {
            let Some(id) = *slot else { continue };
            let Some(iPicBuffIdx) = pPicBuff.and_then(|p| p.slot(id)).map(|pic| pic.iPicBuffIdx)
            else {
                continue;
            };
            if iPicBuffIdx < 0 || aiRefBuffIdx[..iRefs].contains(&iPicBuffIdx) {
                continue;
            }
            if iRefs < aiRefBuffIdx.len() {
                aiRefBuffIdx[iRefs] = iPicBuffIdx;
                iRefs += 1;
            }
        }
    }
    let mut iWaiting = 0i32;
    let largest = pCtx.pPictReoderingStatus.iLargestBufferedPicIndex;
    for i in 0..=largest {
        let info = pCtx.pPictInfoList[i as usize];
        if info.iPOC == crate::decoder::decoder_context::IMinInt32 {
            continue;
        }
        if !aiRefBuffIdx[..iRefs].contains(&info.iPicBuffIdx) {
            iWaiting += 1;
        }
    }
    iRefs as i32 + iWaiting
}

/// Matches `void CWelsDecoder::StoreReadyPicture (...)`.
///
/// Moves the current picture into the first free slot of the picture list and takes
/// the DPB reference that keeps its planes alive until it is emitted. Leaves
/// `iBufferStatus` alone — i.e. the picture where it is — if the list is full; see
/// [`EmitOnFullPictInfoList`].
fn StoreReadyPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    pDstInfo: &mut SBufferInfo,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    let Some(i) = (0..PICT_INFO_LIST_SIZE).find(|&i| pCtx.pPictInfoList[i].iPOC == IMinInt32)
    else {
        return;
    };
    pCtx.pPictInfoList[i].sBufferInfo = *pDstInfo;
    // The DPB's "previous picture" is a slot handle, so it is resolved here. The
    // thread count is read before the pool borrow opens, since `GetThreadCount`
    // takes the context.
    let bSingleThreaded = crate::decoder::decoder_core::GetThreadCount(&*pCtx) <= 1;
    let prev_id = prev_dpb_id(&pCtx.pLastDecPicInfo);
    let mut sPrev: Option<(i32, i32)> = None;
    if let Some(prev) = prev_dpb_pic_mut(&mut pCtx.pPicBuff, prev_id) {
        sPrev = Some((prev.iFramePoc, prev.iPicBuffIdx));
        if bSingleThreaded {
            prev.iRefCount += 1;
        }
    }
    // The decoded picture's own POC, not the slice header's: the two agree except
    // after an MMCO 5, where marking zeroes the picture's POC as 8.2.1 requires and
    // the slice header still carries what was coded.
    pCtx.pPictInfoList[i].iPOC = match sPrev {
        Some((iFramePoc, iPicBuffIdx)) => {
            pCtx.pPictInfoList[i].iPicBuffIdx = iPicBuffIdx;
            iFramePoc
        }
        None => slice_header_of(&*pCtx).map_or(0, |sh| sh.iPicOrderCntLsb),
    };
    pCtx.pPictInfoList[i].iSeqNum = pCtx.pPictReoderingStatus.iOutputSeqNum;
    pCtx.pPictInfoList[i].uiDecodingTimeStamp = pCtx.uiDecodingTimeStamp;
    pDstInfo.iBufferStatus = 0;
    pCtx.pPictReoderingStatus.iNumOfPicts += 1;
    if i as i32 > pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        pCtx.pPictReoderingStatus.iLargestBufferedPicIndex = i as i32;
    }
}

/// Matches `void CWelsDecoder::EmitOnFullPictInfoList (...)`.
///
/// The picture list is full: emit whichever of the current picture and the smallest
/// waiting picture comes first in output order, so that the output order still holds
/// and neither is dropped.
///
/// [`PICT_INFO_LIST_SIZE`] is twice the largest DPB Table A-1 allows, and the backlog
/// this layer can build is bounded below that, so this is a guarantee that a picture
/// is never dropped rather than a path a conforming stream reaches.
fn EmitOnFullPictInfoList(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    let kiCurPoc = match prev_dpb_id(&pCtx.pLastDecPicInfo)
        .and_then(|id| pCtx.pPicBuff.as_deref().and_then(|p| p.slot(id)))
    {
        Some(pic) => pic.iFramePoc,
        None => slice_header_of(&*pCtx).map_or(0, |sh| sh.iPicOrderCntLsb),
    };
    let mut iMinIdx: Option<usize> = None;
    for i in 0..=pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        let info = pCtx.pPictInfoList[i as usize];
        if info.iPOC == IMinInt32 {
            continue;
        }
        let bSmaller = match iMinIdx {
            None => true,
            Some(m) => {
                let cur = pCtx.pPictInfoList[m];
                if info.iSeqNum == cur.iSeqNum {
                    info.iPOC < cur.iPOC
                } else {
                    info.iSeqNum.wrapping_sub(cur.iSeqNum) < 0
                }
            }
        };
        if bSmaller {
            iMinIdx = Some(i as usize);
        }
    }
    let kbCurrentIsFirst = match iMinIdx {
        None => true,
        Some(m) => {
            let min = pCtx.pPictInfoList[m];
            let iOutputSeqNum = pCtx.pPictReoderingStatus.iOutputSeqNum;
            if min.iSeqNum == iOutputSeqNum {
                kiCurPoc <= min.iPOC
            } else {
                iOutputSeqNum.wrapping_sub(min.iSeqNum) < 0
            }
        }
    };
    if kbCurrentIsFirst {
        // Straight out, the way C.4.5.2 outputs a picture no waiting one precedes.
        ppDst[0] = pDstInfo.pDst[0];
        ppDst[1] = pDstInfo.pDst[1];
        ppDst[2] = pDstInfo.pDst[2];
        return;
    }
    // Force the smallest waiting picture out, which frees its slot, buffer the
    // current picture into it, and hand the freed picture to the caller.
    let sCurrent = *pDstInfo;
    ReleaseBufferedReadyPictureReorder(pCtx, true, ppDst, pDstInfo, true);
    let sEmitted = *pDstInfo;
    let pEmitted = *ppDst;
    *pDstInfo = sCurrent;
    StoreReadyPicture(pCtx, pDstInfo);
    *pDstInfo = sEmitted;
    *ppDst = pEmitted;
}

/// Matches `void CWelsDecoder::BufferingReadyPicture (...)` in `welsDecoderExt.cpp`.
///
/// Moves a just-decoded picture out of `pDstInfo` and into the reordering slot list,
/// clearing `iBufferStatus` so nothing is emitted until a release call picks the
/// picture back up in display order, and closes off the coded video sequence the
/// picture ends, if it ends one.
fn BufferingReadyPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    _ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    if pDstInfo.iBufferStatus == 0 {
        return;
    }
    UpdateReorderingParameters(pCtx);
    // A coded video sequence ends at an IDR or an SPS change — which is what the
    // core's own `iSeqNum` counts — and at a memory_management_control_operation
    // equal to 5, which 8.2.1 makes the current picture's POC zero and C.4.4 makes a
    // point past which nothing earlier may still be waiting. Marking has already run,
    // so `bLastHasMmco5` is this picture's flag.
    let kbNewSequence = pCtx.pPictReoderingStatus.iPrevCoreSeqNum != pCtx.iSeqNum
        || pCtx.pLastDecPicInfo.bLastHasMmco5;
    if kbNewSequence {
        pCtx.pPictReoderingStatus.iOutputSeqNum += 1;
        pCtx.pPictReoderingStatus.iPrevCoreSeqNum = pCtx.iSeqNum;
        // C.4.4: an IDR that asks for it discards the pictures of the sequence before
        // it instead of outputting them.
        let bNoOutputOfPriorPics = slice_header_of(&*pCtx)
            .is_some_and(|sh| sh.bIdrFlag && sh.sRefMarking.bNoOutputOfPriorPicsFlag);
        if bNoOutputOfPriorPics {
            DiscardBufferedPictures(pCtx);
        }
    }
    StoreReadyPicture(pCtx, pDstInfo);
}

/// The `no_output_of_prior_pics_flag` arm of C.4.4: every waiting picture is
/// released without being output.
fn DiscardBufferedPictures(pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    for i in 0..=pCtx.pPictReoderingStatus.iLargestBufferedPicIndex {
        let idx = i as usize;
        if pCtx.pPictInfoList[idx].iPOC == IMinInt32 {
            continue;
        }
        pCtx.pPictInfoList[idx].iPOC = IMinInt32;
        let iPicBuffIdx = pCtx.pPictInfoList[idx].iPicBuffIdx;
        if let Some(pPicBuff) = pic_pool_ptr(&mut pCtx.pPicBuff) {
            if let Some(pPic) = pPicBuff.slot_at_mut(iPicBuffIdx) {
                pPic.iRefCount -= 1;
                if pPic.iRefCount <= 0 {
                    if let Some(set_unref) = pPic.pSetUnRef {
                        set_unref(pPic);
                    }
                }
            }
        }
        pCtx.pPictReoderingStatus.iNumOfPicts -= 1;
    }
}

/// Releases the buffered picture whose slot is referenced by `iPictInfoIndex`,
/// dropping the DPB reference taken in [`StoreReadyPicture`]. Shared tail of
/// `ReleaseBufferedReadyPictureReorder` and `EmitOnFullPictInfoList` in
/// `welsDecoderExt.cpp`.
fn EmitBufferedPicture(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    let idx = pCtx.pPictReoderingStatus.iPictInfoIndex as usize;
    *pDstInfo = pCtx.pPictInfoList[idx].sBufferInfo;
    ppDst[0] = pDstInfo.pDst[0];
    ppDst[1] = pDstInfo.pDst[1];
    ppDst[2] = pDstInfo.pDst[2];
    pCtx.pPictInfoList[idx].iPOC = crate::decoder::decoder_context::IMinInt32;
    let iPicBuffIdx = pCtx.pPictInfoList[idx].iPicBuffIdx;
    // The pool is resolved here, not by the caller: the flag comes down instead and
    // the borrow is taken — and ended — inside this block.
    if let Some(pPicBuff) = pool_for(pCtx, bUsePool) {
        // `slot_at_mut` carries the `>= 0 && < iCapacity` range test, so a failed test
        // is its `None`.
        if let Some(pPic) = pPicBuff.slot_at_mut(iPicBuffIdx) {
            pPic.iRefCount -= 1;
            if pPic.iRefCount <= 0 {
                if let Some(set_unref) = pPic.pSetUnRef {
                    set_unref(pPic);
                }
            }
        }
    }
    pCtx.pPictReoderingStatus.iNumOfPicts -= 1;
}

/// Matches `void CWelsDecoder::ReleaseBufferedReadyPictureReorder (...)`.
///
/// The bumping process of C.4.5.3, one output per completed picture.
///
/// Picks the buffered picture with the smallest `(sequence, POC)` — the next one in
/// output order, since output order is POC order within a coded video sequence and
/// sequences follow one another in decoding order — and emits it when the DPB says
/// nothing still to come can precede it:
///
/// * `isFlush`: end of stream, everything left goes out in order;
/// * the picture is from an older sequence than the one being decoded. C.4.4 empties
///   the DPB across a sequence boundary, so nothing that follows can precede it;
/// * the DPB has no empty frame buffer (C.4.5.3). [`GetDpbFullness`] counts one more
///   than the fullness the specification tests, because the current picture is
///   already stored here and already marked there, hence `> iDpbSize`;
/// * the VUI carries `max_num_reorder_frames` and more than that many pictures are
///   waiting (E.2.1): the smallest of them cannot be preceded by a picture that has
///   not been decoded yet. This is the term that keeps the latency of a stream with
///   a VUI down to what its encoder promised.
fn ReleaseBufferedReadyPictureReorder(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    bUsePool: bool,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
    isFlush: bool,
) {
    let IMinInt32 = crate::decoder::decoder_context::IMinInt32;
    // The pool resolution happens down in [`EmitBufferedPicture`]: held here it would
    // be a borrow of `pCtx` spanning the whole body below, which reads `pCtx`
    // throughout. Nothing between this point and the call writes `pCtx.pPicBuff`.

    if pCtx.pPictReoderingStatus.iNumOfPicts > 0 {
        pCtx.pPictReoderingStatus.iMinPOC = IMinInt32;
        let mut firstValidIdx: i32 = -1;
        let largest = pCtx.pPictReoderingStatus.iLargestBufferedPicIndex;
        for i in 0..=largest {
            let info = pCtx.pPictInfoList[i as usize];
            if pCtx.pPictReoderingStatus.iMinPOC == IMinInt32 && info.iPOC > IMinInt32 {
                pCtx.pPictReoderingStatus.iMinPOC = info.iPOC;
                pCtx.pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                pCtx.pPictReoderingStatus.iPictInfoIndex = i;
                firstValidIdx = i;
                break;
            }
        }
        for i in 0..=largest {
            if i == firstValidIdx {
                continue;
            }
            let info = pCtx.pPictInfoList[i as usize];
            let min_seq = pCtx.pPictReoderingStatus.iMinSeqNum;
            let min_poc = pCtx.pPictReoderingStatus.iMinPOC;
            if info.iPOC > IMinInt32
                && (if info.iSeqNum == min_seq {
                    info.iPOC < min_poc
                } else {
                    info.iSeqNum.wrapping_sub(min_seq) < 0
                })
            {
                pCtx.pPictReoderingStatus.iMinPOC = info.iPOC;
                pCtx.pPictReoderingStatus.iMinSeqNum = info.iSeqNum;
                pCtx.pPictReoderingStatus.iPictInfoIndex = i;
            }
        }
    }

    if pCtx.pPictReoderingStatus.iMinPOC > IMinInt32 {
        let mut isReady = true;
        if !isFlush {
            let st = pCtx.pPictReoderingStatus;
            isReady = st.iMinSeqNum.wrapping_sub(st.iOutputSeqNum) < 0
                || GetDpbFullness(&*pCtx) > st.iDpbSize
                || (st.iMaxNumReorderFrames >= 0 && st.iNumOfPicts > st.iMaxNumReorderFrames);
        }
        if isReady {
            pCtx.pPictReoderingStatus.iLastWrittenPOC = pCtx.pPictReoderingStatus.iMinPOC;
            pCtx.pPictReoderingStatus.iLastWrittenSeqNum = pCtx.pPictReoderingStatus.iMinSeqNum;
            EmitBufferedPicture(pCtx, bUsePool, ppDst, pDstInfo);
            pCtx.pPictReoderingStatus.iMinPOC = IMinInt32;
        }
    }
}

/// Matches `DECODING_STATE CWelsDecoder::ReorderPicturesInDisplay (...)`.
///
/// A stream that cannot reorder — baseline, a POC type this decoder derives no POC
/// for, or a VUI that promises output order is decoding order — is handed its
/// picture the moment it is decoded. Everything else goes into the buffer and comes
/// out by the bumping process.
fn ReorderPicturesInDisplay(
    pCtx: &mut crate::decoder::decoder_core::SWelsDecoderContext,
    ppDst: &mut [*mut u8; 3],
    pDstInfo: &mut SBufferInfo,
) {
    if !UpdateReorderingParameters(pCtx) {
        return;
    }
    if !pCtx.pPictReoderingStatus.bReorderPictures || pDstInfo.iBufferStatus != 1 {
        return;
    }
    BufferingReadyPicture(pCtx, ppDst, pDstInfo);
    if pDstInfo.iBufferStatus == 1 {
        // The picture list was full, so the picture is still here; it is sized so that
        // this cannot happen.
        EmitOnFullPictInfoList(pCtx, ppDst, pDstInfo);
    } else {
        ReleaseBufferedReadyPictureReorder(pCtx, true, ppDst, pDstInfo, false);
    }
}
