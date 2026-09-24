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

//! Safe Rust H.264 / SVC Encoder core (`Encoder`).
//!
//! Wraps [`crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder`] without C ABI vtables
//! or raw pointers on the main encoding path. Used by both the `cxx` C++ API (`cxx_api`)
//! and the raw C ABI (`c_api`).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use std::ffi::c_void;

use super::types::{
    ENCODER_OPTION, SEncParamBase, SEncParamExt, SFrameBSInfo, SSourcePicture, TraceUserCtx,
    WelsTraceCallback,
};

/// The H.264 encoder, as a Rust type.
///
/// Wraps `CWelsH264SVCEncoder`. Every method is safe except the two option calls,
/// which say so.
pub struct Encoder(pub(crate) crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder);

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    pub fn new() -> Self {
        Self(crate::encoder::wels_encoder_ext::CWelsH264SVCEncoder::new())
    }

    /// Returns `cmResultSuccess` or one of `codec_def.h`'s `CM_*` codes.
    pub fn initialize(&mut self, param: &SEncParamBase) -> i32 {
        self.0.Initialize(Some(param))
    }

    pub fn initialize_ext(&mut self, param: &SEncParamExt) -> i32 {
        self.0.InitializeExt(Some(param))
    }

    /// Fills `param` with the encoder's defaults. Every field is written.
    pub fn default_params(&mut self, param: &mut SEncParamExt) -> i32 {
        self.0.GetDefaultParams(param)
    }

    pub fn uninitialize(&mut self) -> i32 {
        self.0.Uninitialize()
    }

    /// Encodes one frame.
    ///
    /// `src`'s `pData` planes are the caller's and are read during this call only.
    /// On success `bs`'s layer buffers name memory owned by this encoder, valid
    /// until the next call on it — which is why this returns pointers, not slices.
    pub fn encode_frame(&mut self, src: &SSourcePicture, bs: &mut SFrameBSInfo) -> i32 {
        self.0.EncodeFrame(src, bs)
    }

    /// Emits SPS/PPS into `bs`, with the same output window as [`Self::encode_frame`].
    pub fn encode_parameter_sets(&mut self, bs: &mut SFrameBSInfo) -> i32 {
        self.0.EncodeParameterSets(bs)
    }

    pub fn force_intra_frame(&mut self, idr: bool, layer_id: i32) -> i32 {
        self.0.ForceIntraFrame(idr, layer_id)
    }

    /// The trace destination, as three typed setters rather than an option blob.
    pub fn set_trace_level(&mut self, level: u32) {
        self.0.m_pWelsTrace.SetTraceLevel(level);
        self.0.sync_log_ctx();
    }

    /// # Safety
    ///
    /// `callback` is entered on every delivered message until it is replaced or
    /// this encoder is dropped, with the context installed beside it by
    /// [`Self::set_trace_callback_context`]; it must be sound to enter for that
    /// whole window. Neither half can carry a lifetime across the C ABI.
    ///
    /// ```compile_fail,E0133
    /// # unsafe extern "C" fn sink(_: *mut std::ffi::c_void, _: i32, _: *const std::ffi::c_char) {}
    /// let mut e = openh264_rs::api::codec_api::Encoder::new();
    /// e.set_trace_callback(Some(sink));
    /// ```
    #[allow(unsafe_code)]
    pub unsafe fn set_trace_callback(&mut self, callback: WelsTraceCallback) {
        self.0.m_pWelsTrace.SetTraceCallback(callback);
        self.0.sync_log_ctx();
    }

    /// # Safety
    ///
    /// `ctx` is handed back to the trace callback on every message until it is
    /// replaced or this encoder is dropped, so it must stay valid for that long.
    /// It is the caller's, and this crate never dereferences it.
    #[allow(unsafe_code)]
    pub unsafe fn set_trace_callback_context(&mut self, ctx: *mut c_void) {
        self.0
            .m_pWelsTrace
            .SetTraceCallbackContext(TraceUserCtx::from_abi(ctx));
        self.0.sync_log_ctx();
    }

    /// `ENCODER_OPTION_*`, the type-erased pair.
    ///
    /// # Safety
    ///
    /// `option` must point at a readable, aligned object of the type `id` names,
    /// for the duration of the call — see [`crate::api::c_api::encoder_set_opt_c`]'s contract.
    #[allow(unsafe_code)]
    pub unsafe fn set_option_raw(&mut self, id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        unsafe { self.0.SetOption(id, option) }
    }

    /// # Safety
    ///
    /// As [`Self::set_option_raw`], with `option` written.
    #[allow(unsafe_code)]
    pub unsafe fn get_option_raw(&mut self, id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        unsafe { self.0.GetOption(id, option) }
    }
}
