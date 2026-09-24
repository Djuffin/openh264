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

//! Raw C ABI Virtual Function Tables (`ISVCEncoderVtbl`, `ISVCDecoderVtbl`),
//! `extern "C"` vtable thunks, C-ABI shells (`CWelsH264SVCEncoderImpl`, `CWelsDecoderImpl`),
//! and `#[no_mangle]` dynamic library exports (`WelsCreateSVCEncoder`, `WelsCreateDecoder`, etc.).

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]

use std::ffi::{c_long, c_void};
use std::ptr;

use super::decoder::{Decoder, ec_idc_from_raw, video_bs_type_from_raw};
use super::encoder::Encoder;
use super::types::*;

// ============================================================================
// C/C++ Virtual Function Tables & Interface Definitions
// ============================================================================

/// C-compatible virtual function table for `ISVCEncoder`.
#[repr(C)]
pub struct ISVCEncoderVtbl {
    pub Initialize:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32,
    pub InitializeExt:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32,
    pub GetDefaultParams:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32,
    pub Uninitialize: unsafe extern "C" fn(pThis: *mut ISVCEncoder) -> i32,
    pub EncodeFrame: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32,
    pub EncodeParameterSets:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32,
    pub ForceIntraFrame:
        unsafe extern "C" fn(pThis: *mut ISVCEncoder, bIDR: bool, iLayerId: i32) -> i32,
    /// `pOption`'s type is a function of `eOptionId` (`codec_api.h:245`). See
    /// [`encoder_set_opt_c`]'s contract.
    pub SetOption: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32,
    pub GetOption: unsafe extern "C" fn(
        pThis: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32,
}

/// Opaque H.264 / SVC Encoder class instance representation (`ISVCEncoder`).
#[repr(C)]
pub struct ISVCEncoder {
    pub lpVtbl: *const ISVCEncoderVtbl,
}

#[allow(unsafe_code)]
// # Safety — the contract every function below shares
//
// `this` must be a pointer the matching factory returned (`WelsCreateSVCEncoder` /
// `WelsCreateDecoder`), not yet passed to its destroyer, derived from the whole
// implementation allocation, non-null, and unaliased for the call. Every pointer
// argument carries the C header's own contract (`codec_api.h`), unchanged.
impl ISVCEncoder {
    /// Initializes the encoder with basic parameters.
    #[inline]
    pub unsafe fn Initialize(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
        unsafe { ((*(*this).lpVtbl).Initialize)(this, pParam) }
    }

    /// Initializes the encoder with extended SVC parameters.
    #[inline]
    pub unsafe fn InitializeExt(this: *mut ISVCEncoder, pParam: *const SEncParamExt) -> i32 {
        unsafe { ((*(*this).lpVtbl).InitializeExt)(this, pParam) }
    }

    /// Retrieves default extension encoding parameters.
    #[inline]
    pub unsafe fn GetDefaultParams(this: *mut ISVCEncoder, pParam: *mut SEncParamExt) -> i32 {
        unsafe { ((*(*this).lpVtbl).GetDefaultParams)(this, pParam) }
    }

    /// Uninitializes and frees encoder session resources.
    #[inline]
    pub unsafe fn Uninitialize(this: *mut ISVCEncoder) -> i32 {
        unsafe { ((*(*this).lpVtbl).Uninitialize)(this) }
    }

    /// Encodes a single uncompressed frame.
    #[inline]
    pub unsafe fn EncodeFrame(
        this: *mut ISVCEncoder,
        kpSrcPic: *const SSourcePicture,
        pBsInfo: *mut SFrameBSInfo,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).EncodeFrame)(this, kpSrcPic, pBsInfo) }
    }

    /// Serializes out-of-band parameter sets (SPS/PPS).
    #[inline]
    pub unsafe fn EncodeParameterSets(this: *mut ISVCEncoder, pBsInfo: *mut SFrameBSInfo) -> i32 {
        unsafe { ((*(*this).lpVtbl).EncodeParameterSets)(this, pBsInfo) }
    }

    /// Forces the next frame to be encoded as an IDR keyframe.
    #[inline]
    pub unsafe fn ForceIntraFrame(this: *mut ISVCEncoder, bIDR: bool) -> i32 {
        unsafe { ((*(*this).lpVtbl).ForceIntraFrame)(this, bIDR, -1) }
    }

    /// Sets runtime encoder option. `pOption` is raw — see the vtable slot.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime encoder option. `pOption` is raw.
    #[inline]
    pub unsafe fn GetOption(
        this: *mut ISVCEncoder,
        eOptionId: ENCODER_OPTION,
        pOption: *mut c_void,
    ) -> i32 {
        unsafe { ((*(*this).lpVtbl).GetOption)(this, eOptionId, pOption) }
    }
}

/// C-compatible virtual function table for `ISVCDecoder`.
#[repr(C)]
pub struct ISVCDecoderVtbl {
    pub Initialize:
        unsafe extern "C" fn(pThis: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long,
    pub Uninitialize: unsafe extern "C" fn(pThis: *mut ISVCDecoder) -> c_long,
    pub DecodeFrame: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pStride: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
    ) -> DECODING_STATE,
    pub DecodeFrameNoDelay: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub DecodeFrame2: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub FlushFrame: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE,
    pub DecodeParser: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDstInfo: *mut SParserBsInfo,
    ) -> DECODING_STATE,
    pub DecodeFrameEx: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDst: *mut u8,
        iDstStride: i32,
        iDstLen: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
        iColorFormat: *mut i32,
    ) -> DECODING_STATE,
    /// As `ISVCEncoderVtbl::SetOption` — `codec_api.h:518`'s slot.
    pub SetOption: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long,
    pub GetOption: unsafe extern "C" fn(
        pThis: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long,
}

/// Opaque H.264 / SVC Decoder class instance representation (`ISVCDecoder`).
#[repr(C)]
pub struct ISVCDecoder {
    pub lpVtbl: *const ISVCDecoderVtbl,
}

#[allow(unsafe_code)]
impl ISVCDecoder {
    // The `# Safety` contract: the note above `impl ISVCEncoder`.

    /// Initializes the decoder context.
    #[inline]
    pub unsafe fn Initialize(this: *mut ISVCDecoder, pParam: *const SDecodingParam) -> c_long {
        unsafe { ((*(*this).lpVtbl).Initialize)(this, pParam) }
    }

    /// Uninitializes the decoder context.
    #[inline]
    pub unsafe fn Uninitialize(this: *mut ISVCDecoder) -> c_long {
        unsafe { ((*(*this).lpVtbl).Uninitialize)(this) }
    }

    /// Decodes a single frame.
    #[inline]
    pub unsafe fn DecodeFrame(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pStride: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            ((*(*this).lpVtbl).DecodeFrame)(this, pSrc, iSrcLen, ppDst, pStride, iWidth, iHeight)
        }
    }

    /// Zero-latency frame decoding (recommended real-time decoder API).
    #[inline]
    pub unsafe fn DecodeFrameNoDelay(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeFrameNoDelay)(this, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Multi-slice frame assembly decoding entry point.
    #[inline]
    pub unsafe fn DecodeFrame2(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeFrame2)(this, pSrc, iSrcLen, ppDst, pDstInfo) }
    }

    /// Flushes remaining decoded reference frames from the DPB.
    #[inline]
    pub unsafe fn FlushFrame(
        this: *mut ISVCDecoder,
        ppDst: *mut *mut u8,
        pDstInfo: *mut SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).FlushFrame)(this, ppDst, pDstInfo) }
    }

    /// Parses input bitstream headers only without pixel reconstruction.
    #[inline]
    pub unsafe fn DecodeParser(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDstInfo: *mut SParserBsInfo,
    ) -> DECODING_STATE {
        unsafe { ((*(*this).lpVtbl).DecodeParser)(this, pSrc, iSrcLen, pDstInfo) }
    }

    /// Decodes to arbitrary destination format buffer.
    #[inline]
    pub unsafe fn DecodeFrameEx(
        this: *mut ISVCDecoder,
        pSrc: *const u8,
        iSrcLen: i32,
        pDst: *mut u8,
        iDstStride: i32,
        iDstLen: *mut i32,
        iWidth: *mut i32,
        iHeight: *mut i32,
        iColorFormat: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            ((*(*this).lpVtbl).DecodeFrameEx)(
                this,
                pSrc,
                iSrcLen,
                pDst,
                iDstStride,
                iDstLen,
                iWidth,
                iHeight,
                iColorFormat,
            )
        }
    }

    /// Sets runtime decoder option. `pOption` is raw — see the vtable slot.
    #[inline]
    pub unsafe fn SetOption(
        this: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long {
        unsafe { ((*(*this).lpVtbl).SetOption)(this, eOptionId, pOption) }
    }

    /// Queries runtime decoder option. `pOption` is raw.
    #[inline]
    pub unsafe fn GetOption(
        this: *mut ISVCDecoder,
        eOptionId: DECODER_OPTION,
        pOption: *mut c_void,
    ) -> c_long {
        unsafe { ((*(*this).lpVtbl).GetOption)(this, eOptionId, pOption) }
    }
}

// ===========================================================================
// C-ABI shells
// ===========================================================================

#[repr(C)]
pub struct CWelsH264SVCEncoderImpl {
    pub base: ISVCEncoder,
    pub pVtbl: Box<ISVCEncoderVtbl>,
    pub inner: Encoder,
}

#[repr(C)]
pub struct CWelsDecoderImpl {
    pub base: ISVCDecoder,
    pub pVtbl: Box<ISVCDecoderVtbl>,
    /// The safe core this C-ABI shell wraps.
    pub core: Decoder,
}

// ===========================================================================
// No panic crosses the ABI.
//
// A `panic!` inside an `extern "C" fn` is not an unwind a C caller can ignore: since
// Rust 1.81 the runtime aborts the process. Every entry point below therefore catches.
//
// The guard rests on `panic = "unwind"`, Cargo's default in every profile this crate
// builds under. A consumer who rebuilds it with `panic = "abort"` gets no window here.
//
// A caught panic is reported at `WELS_LOG_ERROR` so it is visible, and the code it
// maps to is the slot's own failure code so a caller's error handling works.
//
// `AssertUnwindSafe` is asserted, not proved. The impl object is `&mut`-reachable
// across the window, so a panic can leave a codec context half-updated. The claim is
// narrower than `UnwindSafe`'s: after a caught panic the object is memory-safe to drop
// and to call again, and the call that panicked reports failure. The codec's state is
// not claimed to be coherent — a consumer that gets one of these codes should destroy
// the object.
// ===========================================================================

#[allow(unsafe_code)]
/// The trace settings to report a caught panic through, read from the impl object
/// before the guarded body runs: afterwards the object is in whatever state the unwind
/// left it.
///
/// # Safety
///
/// `this` is either null or a pointer to a live `CWelsDecoderImpl` — the same
/// contract every decoder slot states for its own `this`.
unsafe fn decoder_log(this: *mut ISVCDecoder) -> Option<crate::common::wels_trace::SLogContext> {
    if this.is_null() {
        return None;
    }
    unsafe { Some((*(this as *mut CWelsDecoderImpl)).core.trace.log_context()) }
}

#[allow(unsafe_code)]
/// [`decoder_log`] for the encoder side.
///
/// # Safety
///
/// `this` is either null or a pointer to a live `CWelsH264SVCEncoderImpl`.
unsafe fn encoder_log(this: *mut ISVCEncoder) -> Option<crate::common::wels_trace::SLogContext> {
    if this.is_null() {
        return None;
    }
    unsafe {
        Some(
            (*(this as *mut CWelsH264SVCEncoderImpl))
                .inner
                .0
                .m_pWelsTrace
                .log_context(),
        )
    }
}

/// Reports a caught panic through the trace at `WELS_LOG_ERROR`.
///
/// The message is included when the payload is `&'static str` or `String`; any other
/// payload is named as such.
pub(crate) fn report_abi_panic(
    slot: &str,
    payload: Box<dyn std::any::Any + Send>,
    log: Option<crate::common::wels_trace::SLogContext>,
) {
    let what = payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>");
    // `WelsLog` does nothing when there is no sink, which is the whole of the `None`
    // case.
    crate::common::wels_trace::WelsLog(
        log.unwrap_or_default(),
        crate::common::wels_trace::WELS_LOG_ERROR,
        &format!(
            "{slot}: a panic was caught at the C-ABI boundary and reported as a failure code instead of aborting the process. Panic message: {what}"
        ),
    );
}

/// One `catch_unwind` window per `extern "C"` entry point.
///
/// `$slot` names the entry for the log, `$log` is its [`decoder_log`]/[`encoder_log`]
/// read, `$fail` is the code this slot returns when it cannot do its job, and `$body`
/// is the slot's own body.
///
/// Every `return` inside `$body` returns from the closure, so it is the value of the
/// whole expression.
macro_rules! abi_guard {
    ($slot:literal, $log:expr, $fail:expr, $body:block) => {{
        let __log = $log;
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(move || $body)) {
            Ok(v) => v,
            Err(payload) => {
                $crate::api::c_api::report_abi_panic($slot, payload, __log);
                $fail
            }
        }
    }};
}
pub(crate) use abi_guard;

#[cfg(test)]
thread_local! {
    /// The guard's covering-test hook, compiled only under `cfg(test)`.
    ///
    /// Thread-local rather than global: other unit tests drive `DecodeFrame2` and
    /// `EncodeFrame` in parallel in one process, and a global switch would fire in
    /// whichever test happened to be inside a thunk at the time.
    pub(crate) static PANIC_PROBE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `PANIC_PROBE` values. `0` is off.
#[cfg(test)]
pub(crate) const PROBE_DECODE_FRAME2: u32 = 1;
#[cfg(test)]
pub(crate) const PROBE_ENCODE_FRAME: u32 = 2;

/// Panics if this thread armed `PANIC_PROBE` for `$which`. Expands to nothing
/// outside `cfg(test)`.
macro_rules! panic_probe {
    ($which:expr) => {
        #[cfg(test)]
        if PANIC_PROBE.with(|p| p.get()) == $which {
            panic!("P13 covering test: a deliberate panic inside the guarded body");
        }
    };
}

// ===========================================================================
// The encoder's nine vtable slots.
//
// `codec_api.h` hands a C caller a vtable of nine `extern "C"` functions over an
// opaque `ISVCEncoder*`. Everything that arrives here is a raw pointer with a
// validity window the caller guarantees, and the window is not the same for every
// slot: `pParam` is read for the duration of one call, `pBsInfo` is written during
// one call and read by the caller until the next one, and `pOption`'s size is a
// function of the option id. Each slot below states its own.
//
// `this` is cast to the whole impl allocation and never borrowed as `ISVCEncoder`,
// which is one pointer wide.
// ===========================================================================

#[allow(unsafe_code)]
/// `ISVCEncoder::Initialize` — `codec_api.h:196`.
///
/// # Safety
///
/// * `this` is either null or a pointer to a live `CWelsH264SVCEncoderImpl`
///   produced by [`WelsCreateSVCEncoder`] and not yet destroyed. It is cast to the
///   whole impl allocation, never borrowed as the one-pointer-wide `ISVCEncoder`.
/// * `pParam` is either null or a readable, aligned `SEncParamBase` for the duration
///   of this call. Nothing in the encoder retains it: `Initialize` transcodes it into
///   its own `SWelsSvcCodingParam` before returning.
unsafe extern "C" fn encoder_init_c(this: *mut ISVCEncoder, pParam: *const SEncParamBase) -> i32 {
    abi_guard!(
        "ISVCEncoder::Initialize",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            // The impl reports a null `pParam`; only `this` has to be checked before
            // the cast.
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            let param = unsafe { pParam.as_ref() };
            enc.0.Initialize(param)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::InitializeExt` — `codec_api.h:203`.
///
/// # Safety
///
/// As [`encoder_init_c`], with `SEncParamExt` in place of `SEncParamBase`.
unsafe extern "C" fn encoder_init_ext_c(
    this: *mut ISVCEncoder,
    pParam: *const SEncParamExt,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::InitializeExt",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            let param = unsafe { pParam.as_ref() };
            enc.0.InitializeExt(param)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::GetDefaultParams` — `codec_api.h:210`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pParam` must be null or a writable, aligned `SEncParamExt` for the duration
///   of this call. It is an out parameter: every field is overwritten and none is
///   read first, so its prior contents may be anything, including uninitialised.
unsafe extern "C" fn encoder_get_default_c(
    this: *mut ISVCEncoder,
    pParam: *mut SEncParamExt,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::GetDefaultParams",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let Some(pParam) = (unsafe { pParam.as_mut() }) else {
                return CM_INIT_PARA_ERROR;
            };
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            enc.default_params(pParam)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::Uninitialize` — `codec_api.h:216`.
///
/// # Safety
///
/// `this` as in [`encoder_init_c`]. Nothing else crosses.
unsafe extern "C" fn encoder_uninit_c(this: *mut ISVCEncoder) -> i32 {
    abi_guard!(
        "ISVCEncoder::Uninitialize",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            enc.uninitialize()
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::EncodeFrame` — `codec_api.h:224`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `kpSrcPic` must be null or a readable, aligned `SSourcePicture` for the
///   duration of this call. Its `pData[0..3]` stay raw: they are the caller's plane
///   pointers, each readable for `iStride[i] * height` bytes in the caller's own
///   layout, for this call only. The encoder copies what it needs before returning
///   and retains none of them.
/// * `pBsInfo` must be null or a writable, aligned `SFrameBSInfo` for the duration
///   of this call. On success its `sLayerInfo[].pBsBuf` pointers name memory owned
///   by the encoder, valid until the next call on this encoder — which is why this
///   cannot be a `&mut [u8]`.
unsafe extern "C" fn encoder_encode_frame_c(
    this: *mut ISVCEncoder,
    kpSrcPic: *const SSourcePicture,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::EncodeFrame",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            panic_probe!(PROBE_ENCODE_FRAME);
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let (Some(kpSrcPic), Some(pBsInfo)) =
                (unsafe { kpSrcPic.as_ref() }, unsafe { pBsInfo.as_mut() })
            else {
                return CM_INIT_PARA_ERROR;
            };
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            enc.encode_frame(kpSrcPic, pBsInfo)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::EncodeParameterSets` — `codec_api.h:231`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pBsInfo` as in [`encoder_encode_frame_c`], including the output window: the
///   SPS/PPS bytes it names are the encoder's, valid until the next call.
unsafe extern "C" fn encoder_encode_param_c(
    this: *mut ISVCEncoder,
    pBsInfo: *mut SFrameBSInfo,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::EncodeParameterSets",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let Some(pBsInfo) = (unsafe { pBsInfo.as_mut() }) else {
                return CM_INIT_PARA_ERROR;
            };
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            enc.encode_parameter_sets(pBsInfo)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::ForceIntraFrame` — `codec_api.h:238`.
///
/// # Safety
///
/// `this` as in [`encoder_init_c`]. `bIDR` must hold 0 or 1, as the C++ ABI requires
/// of `bool`.
unsafe extern "C" fn encoder_force_intra_c(
    this: *mut ISVCEncoder,
    bIDR: bool,
    iLayerId: i32,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::ForceIntraFrame",
        unsafe { encoder_log(this) },
        CM_UNKNOWN_REASON,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            enc.force_intra_frame(bIDR, iLayerId)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::SetOption` — `codec_api.h:245`.
///
/// # Safety
///
/// * `this` as in [`encoder_init_c`].
/// * `pOption` stays raw, because its type is a function of `eOptionId` and of
///   nothing else: `ENCODER_OPTION_TRACE_LEVEL` reads a `u32` through it,
///   `ENCODER_OPTION_TRACE_CALLBACK` a `WelsTraceCallback`,
///   `ENCODER_OPTION_SVC_ENCODE_PARAM_EXT` an `SEncParamExt`, and so on for
///   thirty-two ids. The caller must point it at a readable, aligned object of the
///   type that id names, for the duration of this call.
pub(crate) unsafe extern "C" fn encoder_set_opt_c(
    this: *mut ISVCEncoder,
    eOptionId: ENCODER_OPTION,
    pOption: *mut c_void,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::SetOption",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            unsafe { enc.set_option_raw(eOptionId, pOption) }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCEncoder::GetOption` — `codec_api.h:252`.
///
/// # Safety
///
/// As [`encoder_set_opt_c`], with the blob written rather than read: the caller must
/// point `pOption` at a writable, aligned object of the type `eOptionId` names, for
/// the duration of this call.
unsafe extern "C" fn encoder_get_opt_c(
    this: *mut ISVCEncoder,
    eOptionId: ENCODER_OPTION,
    pOption: *mut c_void,
) -> i32 {
    abi_guard!(
        "ISVCEncoder::GetOption",
        unsafe { encoder_log(this) },
        CM_INIT_PARA_ERROR,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR;
            }
            let enc = unsafe { &mut (*(this as *mut CWelsH264SVCEncoderImpl)).inner };
            unsafe { enc.get_option_raw(eOptionId, pOption) }
        }
    )
}

// ===========================================================================
// The decoder's ten vtable slots. The window that matters most is `DecodeFrame2`'s
// output: `ppDst` comes back naming the decoder's planes, valid until the next call
// on this decoder.
// ===========================================================================

/// `ISVCDecoder::Initialize` — `codec_api.h:452`.
///
/// # Safety
///
/// * `this` is either null or a pointer to a live `CWelsDecoderImpl` from
///   [`WelsCreateDecoder`], cast to the whole impl allocation.
/// * `pParam` must be null or point to a readable, aligned `SDecodingParam`-sized
///   object for the duration of this call.
///
/// `pParam` stays a raw pointer: two of its fields are C `int`s whose wire domain is
/// wider than the Rust enums they are typed as, so `&*pParam` would be undefined for
/// exactly the inputs the clamp exists to handle. It is read as bytes and sanitised
/// field-wise before it becomes an `SDecodingParam`.
#[allow(unsafe_code)]
unsafe extern "C" fn decoder_init_c(
    this: *mut ISVCDecoder,
    pParam: *const SDecodingParam,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::Initialize",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };
            // The null is reported by the impl, which has the trace object, rather
            // than short-circuited here.
            if pParam.is_null() {
                return core.report_init_null_param();
            }
            // The caller's block, read as a C caller may have written it.
            //
            // `SDecodingParam`'s two enum-typed fields, `eEcActiveIdc` and
            // `sVideoProperty.eVideoBsType`, are plain `int`s on the C side and are
            // sanitised after the copy: the first is clamped into
            // `[ERROR_CON_DISABLE, …FREEZE_RES_CHANGE]` (`decoder.cpp:654`), the
            // second normalised to `VIDEO_BITSTREAM_DEFAULT` (`:667`). Each has a
            // closed set of variants in Rust, so `*pParam` would be undefined for
            // exactly the inputs the sanitising exists to handle.
            //
            // So the block is copied as bytes, those two fields are read and written
            // at their own offsets as the `i32`s they are on the wire, and only then
            // does it become an `SDecodingParam`. Every other field is a pointer, an
            // integer or a `bool`.
            let param = unsafe {
                let mut buf = std::mem::MaybeUninit::<SDecodingParam>::uninit();
                ptr::copy_nonoverlapping(
                    pParam.cast::<u8>(),
                    buf.as_mut_ptr().cast::<u8>(),
                    size_of::<SDecodingParam>(),
                );
                let ec = ptr::addr_of_mut!((*buf.as_mut_ptr()).eEcActiveIdc).cast::<i32>();
                ec.write(ec_idc_from_raw(ec.read()) as i32);
                let bs = ptr::addr_of_mut!((*buf.as_mut_ptr()).sVideoProperty.eVideoBsType)
                    .cast::<i32>();
                bs.write(video_bs_type_from_raw(bs.read()) as i32);
                buf.assume_init()
            };
            core.initialize(&param)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::Uninitialize` — `codec_api.h:458`.
///
/// # Safety
///
/// `this` as in [`decoder_init_c`]. Nothing else crosses. After this call the
/// planes any previous `DecodeFrame2` handed out are freed with the context and
/// must not be read.
unsafe extern "C" fn decoder_uninit_c(this: *mut ISVCDecoder) -> c_long {
    abi_guard!(
        "ISVCDecoder::Uninitialize",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };
            core.uninitialize()
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrame` — `codec_api.h:466`. Deprecated upstream.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pSrc` / `iSrcLen`: either `pSrc` is null or `iSrcLen <= 0` (the flush call),
///   or `pSrc` names `iSrcLen` readable bytes for the duration of this call. The
///   decoder copies what it keeps; nothing of the caller's buffer is retained.
/// * `ppDst` must name three writable plane-pointer slots. They come back pointing
///   into the decoder's own picture buffer, readable until the next call on this
///   decoder, which is why they cannot be a `&mut [u8]`.
/// * `pStride` must be null or name two writable `i32`s; `iWidth`, `iHeight` null or
///   one each. All three are out parameters and are written only when a frame was
///   emitted.
unsafe extern "C" fn decoder_decode_frame_c(
    this: *mut ISVCDecoder,
    pSrc: *const u8,
    iSrcLen: i32,
    ppDst: *mut *mut u8,
    pStride: *mut i32,
    iWidth: *mut i32,
    iHeight: *mut i32,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrame",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            let mut buf_info = SBufferInfo::default();
            let state =
                unsafe { decoder_decode_frame2_c(this, pSrc, iSrcLen, ppDst, &mut buf_info) };
            if buf_info.iBufferStatus != 1 {
                return state;
            }
            // Each of the three is optional and written only on the frame-emitted
            // path; `pStride` is two `i32`s.
            let sys = &buf_info.UsrData.sSystemBuffer;
            if let Some(pStride) = unsafe { pStride.cast::<[i32; 2]>().as_mut() } {
                pStride[0] = sys.iStride[0];
                pStride[1] = sys.iStride[1];
            }
            if let Some(iWidth) = unsafe { iWidth.as_mut() } {
                *iWidth = sys.iWidth;
            }
            if let Some(iHeight) = unsafe { iHeight.as_mut() } {
                *iHeight = sys.iHeight;
            }
            state
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrameNoDelay` — `codec_api.h:479`.
///
/// # Safety
///
/// As [`decoder_decode_frame2_c`], which this calls twice. Both calls write `ppDst`
/// and `pDstInfo`, and the second call's values are the ones the caller sees.
///
/// # What "no delay" is
///
/// The second call — `DecodeFrame2 (NULL, 0, …)`, `welsDecoderExt.cpp:720–725` —
/// forces reconstruction, so a caller gets the frame on the call that fed the access
/// unit rather than on the next one.
///
/// The out-parameters are deliberately not restored: if the first call emits a picture
/// and the second does not, the second call's `iBufferStatus = 0` overwrites it and
/// the frame is lost to that caller.
unsafe extern "C" fn decoder_decode_frame_nodelay_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrameNoDelay",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            // `iRet |=` on `DECODING_STATE`, which is a bitset of `ds*` flags — the two
            // calls' states are ORed, not replaced, so an error in either half survives.
            let first = unsafe { decoder_decode_frame2_c(this, kpSrc, kiSrcLen, ppDst, pDstInfo) };
            let second = unsafe { decoder_decode_frame2_c(this, ptr::null(), 0, ppDst, pDstInfo) };
            DECODING_STATE(first.0 | second.0)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrame2` — `codec_api.h:490`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `kpSrc` / `kiSrcLen` as in [`decoder_decode_frame_c`]: null-or-zero is the
///   end-of-stream flush, otherwise `kiSrcLen` readable bytes for this call.
/// * `ppDst` must name three writable plane-pointer slots, and `pDstInfo` a writable,
///   aligned `SBufferInfo`. Both are out parameters.
/// * When `pDstInfo->iBufferStatus == 1`, `ppDst[0..3]` and `pDstInfo->pDst[0..3]`
///   name planes inside the decoder's picture buffer with the strides
///   `pDstInfo->UsrData.sSystemBuffer.iStride` reports. They are valid until the next
///   call on this decoder — the next `DecodeFrame*`, `FlushFrame`, `Uninitialize` or
///   `WelsDestroyDecoder` — and are not the caller's to free, which is why this slot
///   hands back pointers instead of slices.
unsafe extern "C" fn decoder_decode_frame2_c(
    this: *mut ISVCDecoder,
    kpSrc: *const u8,
    kiSrcLen: i32,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrame2",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            panic_probe!(PROBE_DECODE_FRAME2);
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };
            // The caller's access unit, or `None` for the end-of-stream flush that
            // `(NULL, 0)` means on this slot.
            let src: Option<&[u8]> = if kpSrc.is_null() || kiSrcLen <= 0 {
                None
            } else {
                Some(unsafe { std::slice::from_raw_parts(kpSrc, kiSrcLen as usize) })
            };
            let Some(ppDst) = (unsafe { (ppDst as *mut [*mut u8; 3]).as_mut() }) else {
                return DECODING_STATE::dsInitialOptExpected;
            };
            let Some(pDstInfo) = (unsafe { pDstInfo.as_mut() }) else {
                return DECODING_STATE::dsInitialOptExpected;
            };
            core.decode(src, ppDst, pDstInfo)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeFrameEx` — `codec_api.h:503`.
///
/// # Safety
///
/// `this` as in [`decoder_init_c`]; every other argument is unread.
///
/// A stub: returns `dsErrorFree` without touching anything. The slot exists so the
/// vtable's shape and slot order match `codec_api.h`.
unsafe extern "C" fn decoder_decode_frame_ex_c(
    _this: *mut ISVCDecoder,
    _pSrc: *const u8,
    _iSrcLen: i32,
    _pDst: *mut u8,
    _iDstStride: i32,
    _iDstLen: *mut i32,
    _iWidth: *mut i32,
    _iHeight: *mut i32,
    _iColorFormat: *mut i32,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeFrameEx",
        unsafe { decoder_log(_this) },
        DECODING_STATE::dsBitstreamError,
        { DECODING_STATE::dsErrorFree }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::SetOption` — `codec_api.h:518`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pOption` stays raw, for the same reason as
///   [`encoder_set_opt_c`]: its type is a function of `eOptionId`. The caller must
///   point it at a readable, aligned object of the type that id names, for the
///   duration of this call — an `i32` for `END_OF_STREAM` and `ERROR_CON_IDC`, a
///   `u32` for `TRACE_LEVEL`, a `WelsTraceCallback` for `TRACE_CALLBACK`, a `void*`
///   for `TRACE_CALLBACK_CONTEXT`.
/// * The pointer installed by `DECODER_OPTION_TRACE_CALLBACK_CONTEXT` is kept and
///   handed back to the callback on every message until it is replaced or the decoder
///   is destroyed. It is the one value on this interface whose window outlives the
///   call, and it is the caller's to keep alive.
unsafe extern "C" fn decoder_set_opt_c(
    this: *mut ISVCDecoder,
    eOptionId: DECODER_OPTION,
    pOption: *mut c_void,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::SetOption",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            // The blob's type is the option id's: every arm reads it at that type and
            // hands the value to a safe method.
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };

            // `welsDecoderExt.cpp:479-584`. Nine arms, and two head clauses:
            //
            //   1. `NUM_OF_THREADS` first, and it succeeds whether or not the decoder
            //      has a context — it is the object's field;
            //   2. then, for every other id except the three trace ones, a missing
            //      context is `dsInitialOptExpected`.
            if eOptionId == DECODER_OPTION::DECODER_OPTION_NUM_OF_THREADS {
                // `:481-501`. Single-threaded here, so the count clamps to 0.
                // Success on any input, including a null.
                return CM_RESULT_SUCCESS as c_long;
            }
            let ctx_needed = !matches!(
                eOptionId,
                DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL
                    | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK
                    | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT
            );
            if ctx_needed && !core.has_ctx() {
                return DECODING_STATE::dsInitialOptExpected.0 as c_long;
            }

            match eOptionId {
                // `pOption` is tested per arm, and the arms disagree about what a
                // null means: `END_OF_STREAM` and `ERROR_CON_IDC` reject it, the
                // trace ones dereference it, `STATISTICS_LOG_INTERVAL` reaches the
                // trailing `cmInitParaError`.
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                    if pOption.is_null() {
                        return CM_INIT_PARA_ERROR as c_long;
                    }
                    core.set_end_of_stream(unsafe { pOption.cast::<i32>().read() } != 0);
                    CM_RESULT_SUCCESS as c_long
                }
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                    if pOption.is_null() {
                        return CM_INIT_PARA_ERROR as c_long;
                    }
                    // The blob is an `int`, run through
                    // `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, …FREEZE_RES_CHANGE)`
                    // before the store (`welsDecoderExt.cpp:528`). Reading it as
                    // `*const ERROR_CON_IDC` would be undefined for anything outside
                    // 0..=7, which is exactly the range the clamp enforces, so it
                    // crosses as an `i32` and becomes an `ERROR_CON_IDC` only once
                    // `Decoder::set_error_concealment` has clamped it — which is also
                    // where the parse-only refusal (`:531-536`) lives.
                    core.set_error_concealment(unsafe { pOption.cast::<i32>().read() })
                }
                // The three trace options — `welsDecoderExt.cpp:541-561`. These are
                // the three ids that work without a context.
                DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL
                | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK
                | DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT => {
                    if pOption.is_null() {
                        return CM_INIT_PARA_ERROR as c_long;
                    }
                    match eOptionId {
                        DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL => {
                            core.set_trace_level(unsafe { pOption.cast::<u32>().read() });
                        }
                        DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK => unsafe {
                            core.set_trace_callback(pOption.cast::<WelsTraceCallback>().read());
                        },
                        // The one value whose window outlives the call — see the
                        // contract.
                        _ => unsafe {
                            core.set_trace_callback_context(pOption.cast::<*mut c_void>().read());
                        },
                    }
                    CM_RESULT_SUCCESS as c_long
                }
                // `welsDecoderExt.cpp:562` and `:578` — get-only, and both say so.
                DECODER_OPTION::DECODER_OPTION_GET_STATISTICS
                | DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO => CM_INIT_PARA_ERROR as c_long,
                // `:571-577`. A null `pOption` is `cmInitParaError`.
                DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    if pOption.is_null() {
                        return CM_INIT_PARA_ERROR as c_long;
                    }
                    if core.set_statistics_log_interval(unsafe { pOption.cast::<u32>().read() }) {
                        CM_RESULT_SUCCESS as c_long
                    } else {
                        DECODING_STATE::dsInitialOptExpected.0 as c_long
                    }
                }
                // `:583` — an id with no arm is an error.
                _ => CM_INIT_PARA_ERROR as c_long,
            }
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::GetOption` — `codec_api.h:525`.
///
/// # Safety
///
/// As [`decoder_set_opt_c`], with the blob written: `DECODER_OPTION_GET_STATISTICS`
/// writes a whole `SDecoderStatistics` through it, so a caller who passes an `i32`
/// for that id overflows its own object. `pOption` must be a writable, aligned
/// object of the type `eOptionId` names, for the duration of this call.
unsafe extern "C" fn decoder_get_opt_c(
    this: *mut ISVCDecoder,
    eOptionId: DECODER_OPTION,
    pOption: *mut c_void,
) -> c_long {
    abi_guard!(
        "ISVCDecoder::GetOption",
        unsafe { decoder_log(this) },
        CM_INIT_PARA_ERROR as c_long,
        {
            if this.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }
            // Translate-out: each arm asks the core for a value and writes it at the type
            // the option id names.
            let core = unsafe { &(*(this as *mut CWelsDecoderImpl)).core };

            // `welsDecoderExt.cpp:584-695`. Three head clauses, in this order:
            //
            //   1. `NUM_OF_THREADS` is answered before the context is looked at —
            //      it is the object's field, not the context's, and it is the one id
            //      that works on an uninitialized decoder;
            //   2. then `pDecContext == NULL` -> `cmInitExpected`;
            //   3. then `pOption == NULL` -> `cmInitParaError`.
            //
            // So a null `pOption` on an uninitialized decoder reports
            // `cmInitExpected`, not `cmInitParaError`.
            if eOptionId == DECODER_OPTION::DECODER_OPTION_NUM_OF_THREADS {
                if pOption.is_null() {
                    return CM_INIT_PARA_ERROR as c_long;
                }
                // `m_iThreadCount`: single-threaded, and `SetOption`'s arm clamps
                // every request to it.
                unsafe { pOption.cast::<i32>().write(0) };
                return CM_RESULT_SUCCESS as c_long;
            }
            if !core.has_ctx() {
                return CM_INIT_EXPECTED as c_long;
            }
            if pOption.is_null() {
                return CM_INIT_PARA_ERROR as c_long;
            }

            // Past the head clauses every arm below has a context, so each accessor's
            // `Option` is `Some`.
            macro_rules! write_i32 {
                ($v:expr) => {{
                    let Some(v) = $v else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    unsafe { pOption.cast::<i32>().write(v) };
                    return CM_RESULT_SUCCESS as c_long;
                }};
            }

            match eOptionId {
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM => {
                    write_i32!(Some(i32::from(core.end_of_stream())))
                }
                // `:603-619`, the four `LONG_TERM_REF` arms.
                DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID => write_i32!(core.cur_idr_pic_id()),
                DECODER_OPTION::DECODER_OPTION_FRAME_NUM => write_i32!(core.frame_num()),
                DECODER_OPTION::DECODER_OPTION_LTR_MARKING_FLAG => {
                    write_i32!(core.ltr_marking_flag())
                }
                DECODER_OPTION::DECODER_OPTION_LTR_MARKED_FRAME_NUM => {
                    write_i32!(core.ltr_marked_frame_num())
                }
                DECODER_OPTION::DECODER_OPTION_VCL_NAL => write_i32!(core.feedback_vcl_nal()),
                DECODER_OPTION::DECODER_OPTION_TEMPORAL_ID => {
                    write_i32!(core.feedback_temporal_id())
                }
                DECODER_OPTION::DECODER_OPTION_IS_REF_PIC => {
                    write_i32!(core.feedback_is_ref_pic())
                }
                // `welsDecoderExt.cpp:634-637`. The mode the decoder actually runs
                // with is only visible through this option.
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC => {
                    write_i32!(core.error_concealment().map(|idc| idc as i32))
                }
                // `welsDecoderExt.cpp:639-651`.
                DECODER_OPTION::DECODER_OPTION_GET_STATISTICS => {
                    let Some(stats) = core.statistics() else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    unsafe { pOption.cast::<SDecoderStatistics>().write(stats) };
                    return CM_RESULT_SUCCESS as c_long;
                }
                // `:653-659`. An `unsigned int` on this id, in both directions.
                DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL => {
                    let Some(v) = core.statistics_log_interval() else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    unsafe { pOption.cast::<u32>().write(v) };
                    return CM_RESULT_SUCCESS as c_long;
                }
                // `:664-672`. The caller's struct is zeroed before the SPS check, so
                // a refusal still leaves zeros rather than the caller's stack.
                DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO => {
                    unsafe { pOption.cast::<SVuiSarInfo>().write(SVuiSarInfo::default()) };
                    let Some(sar) = core.sar_info() else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    let Some(sar) = sar else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    unsafe { pOption.cast::<SVuiSarInfo>().write(sar) };
                    return CM_RESULT_SUCCESS as c_long;
                }
                DECODER_OPTION::DECODER_OPTION_PROFILE => {
                    let Some(v) = core.active_sps_profile() else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    write_i32!(v)
                }
                DECODER_OPTION::DECODER_OPTION_LEVEL => {
                    let Some(v) = core.active_sps_level() else {
                        return CM_INIT_EXPECTED as c_long;
                    };
                    write_i32!(v)
                }
                DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER => {
                    // `:688-694`. The count is the context's own.
                    unsafe { pOption.cast::<i32>().write(core.frames_remaining()) };
                    return CM_RESULT_SUCCESS as c_long;
                }
                // `:696` — an id with no arm is an error, not a silent success.
                _ => return CM_INIT_PARA_ERROR as c_long,
            }
        }
    )
}

#[allow(unsafe_code)]
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `ppDst` and `pDstInfo` as in [`decoder_decode_frame2_c`], including the
///   output window: a picture released from the reordering buffer is the
///   decoder's, valid until the next call on this decoder.
unsafe extern "C" fn decoder_flush_frame_c(
    this: *mut ISVCDecoder,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::FlushFrame",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            // A caller that hands either out-parameter null gets the drain skipped
            // rather than a write through null.
            let (Some(ppDst), Some(pDstInfo)) =
                (unsafe { (ppDst as *mut [*mut u8; 3]).as_mut() }, unsafe {
                    pDstInfo.as_mut()
                })
            else {
                return DECODING_STATE::dsErrorFree;
            };
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };
            core.flush(ppDst, pDstInfo)
        }
    )
}

#[allow(unsafe_code)]
/// `ISVCDecoder::DecodeParser` — `codec_api.h:511`.
///
/// # Safety
///
/// * `this` as in [`decoder_init_c`].
/// * `pSrc` / `iSrcLen` as in [`decoder_decode_frame2_c`]: null-or-zero is the
///   end-of-stream flush.
/// * `pDstInfo` must name a writable, aligned `SParserBsInfo`. It is an in-out
///   parameter: `uiInBsTimeStamp` is read, everything else is written.
/// * When `iNalNum > 0`, `pNalLenInByte` and `pDstBuff` point into this decoder's
///   parse-only buffers and are valid until the next call on this decoder — the plane
///   contract [`decoder_decode_frame2_c`] states, for bytes instead of planes.
unsafe extern "C" fn decoder_decode_parser_c(
    this: *mut ISVCDecoder,
    pSrc: *const u8,
    iSrcLen: i32,
    pDstInfo: *mut SParserBsInfo,
) -> DECODING_STATE {
    abi_guard!(
        "ISVCDecoder::DecodeParser",
        unsafe { decoder_log(this) },
        DECODING_STATE::dsBitstreamError,
        {
            if this.is_null() {
                return DECODING_STATE::dsInitialOptExpected;
            }
            let core = unsafe { &mut (*(this as *mut CWelsDecoderImpl)).core };
            let src: Option<&[u8]> = if pSrc.is_null() || iSrcLen <= 0 {
                None
            } else {
                Some(unsafe { std::slice::from_raw_parts(pSrc, iSrcLen as usize) })
            };
            // A null `pDstInfo` is refused rather than written through.
            let Some(pDstInfo) = (unsafe { pDstInfo.as_mut() }) else {
                return DECODING_STATE::dsInitialOptExpected;
            };
            core.decode_parser(src, pDstInfo)
        }
    )
}

// ===========================================================================
// Dynamic Library Export Lifecycle Bindings
// ===========================================================================

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateSVCEncoder(ppEncoder: *mut *mut ISVCEncoder) -> i32 {
    abi_guard!("WelsCreateSVCEncoder", None, CM_MALLOC_MEM_ERROR, {
        if ppEncoder.is_null() {
            return CM_INIT_PARA_ERROR;
        }
        let vtbl = Box::new(ISVCEncoderVtbl {
            Initialize: encoder_init_c,
            InitializeExt: encoder_init_ext_c,
            GetDefaultParams: encoder_get_default_c,
            Uninitialize: encoder_uninit_c,
            EncodeFrame: encoder_encode_frame_c,
            EncodeParameterSets: encoder_encode_param_c,
            ForceIntraFrame: encoder_force_intra_c,
            SetOption: encoder_set_opt_c,
            GetOption: encoder_get_opt_c,
        });
        let mut enc = Box::new(CWelsH264SVCEncoderImpl {
            base: ISVCEncoder {
                lpVtbl: ptr::null(),
            },
            pVtbl: vtbl,
            inner: Encoder::new(),
        });
        enc.base.lpVtbl = &*enc.pVtbl as *const ISVCEncoderVtbl;
        unsafe {
            *ppEncoder = Box::into_raw(enc) as *mut ISVCEncoder;
        }
        CM_RESULT_SUCCESS
    })
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroySVCEncoder(pEncoder: *mut ISVCEncoder) {
    abi_guard!(
        "WelsDestroySVCEncoder",
        unsafe { encoder_log(pEncoder) },
        (),
        {
            if !pEncoder.is_null() {
                drop(unsafe { Box::from_raw(pEncoder as *mut CWelsH264SVCEncoderImpl) });
            }
        }
    )
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsCreateDecoder(ppDecoder: *mut *mut ISVCDecoder) -> c_long {
    abi_guard!("WelsCreateDecoder", None, CM_MALLOC_MEM_ERROR as c_long, {
        if ppDecoder.is_null() {
            return CM_INIT_PARA_ERROR as c_long;
        }
        let vtbl = Box::new(ISVCDecoderVtbl {
            Initialize: decoder_init_c,
            Uninitialize: decoder_uninit_c,
            DecodeFrame: decoder_decode_frame_c,
            DecodeFrameNoDelay: decoder_decode_frame_nodelay_c,
            DecodeFrame2: decoder_decode_frame2_c,
            FlushFrame: decoder_flush_frame_c,
            DecodeParser: decoder_decode_parser_c,
            DecodeFrameEx: decoder_decode_frame_ex_c,
            SetOption: decoder_set_opt_c,
            GetOption: decoder_get_opt_c,
        });
        let mut dec = Box::new(CWelsDecoderImpl {
            base: ISVCDecoder {
                lpVtbl: ptr::null(),
            },
            pVtbl: vtbl,
            core: Decoder::new(),
        });
        dec.base.lpVtbl = &*dec.pVtbl as *const ISVCDecoderVtbl;
        // Taken once the heap box has its stable address (`&*dec`). It is the `this = 0x…`
        // of every trace line and nothing else, which is why it travels as an address.
        let addr = (&*dec as *const CWelsDecoderImpl) as usize;
        dec.core.trace.SetCodecInstance(addr);
        // The trace object's constructor sets `WELS_LOG_WARNING`, the encoder's
        // default; the decoder's own default is `WELS_LOG_ERROR`.
        dec.core
            .trace
            .SetTraceLevel(crate::common::wels_trace::WELS_LOG_ERROR as u32);
        unsafe {
            *ppDecoder = Box::into_raw(dec) as *mut ISVCDecoder;
        }
        CM_RESULT_SUCCESS as c_long
    })
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsGetDecoderCapability(pDecCapability: *mut SDecoderCapability) -> i32 {
    abi_guard!("WelsGetDecoderCapability", None, CM_INIT_PARA_ERROR, {
        let Some(cap) = (unsafe { pDecCapability.as_mut() }) else {
            return CM_INIT_PARA_ERROR;
        };
        *cap = SDecoderCapability {
            iProfileIdc: 66,
            iProfileIop: 0xE0,
            iLevelIdc: 32,
            iMaxMbps: 216000,
            iMaxFs: 5120,
            iMaxCpb: 20000,
            iMaxDpb: 20480,
            iMaxBr: 20000,
            bRedPicCap: false,
        };
        CM_RESULT_SUCCESS
    })
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WelsDestroyDecoder(pDecoder: *mut ISVCDecoder) {
    abi_guard!(
        "WelsDestroyDecoder",
        unsafe { decoder_log(pDecoder) },
        (),
        {
            if !pDecoder.is_null() {
                let mut dec = unsafe { Box::from_raw(pDecoder as *mut CWelsDecoderImpl) };
                // Order matters: the dynamic memory goes before the context does.
                dec.core.uninitialize();
            }
        }
    )
}

// ===========================================================================
// C ABI Unit Tests & Test Driver
// ===========================================================================

#[cfg(test)]
pub(crate) mod abi_test_driver {
    use super::*;

    #[allow(unsafe_code)]
    /// Decodes `stream` through the C ABI and returns `(frames, last frame's
    /// dimensions, states)`, where `states` is the bitwise OR of every `DecodeFrame2`
    /// return.
    ///
    /// Calls the vtable thunks directly rather than the conveniences, which is what a
    /// C caller compiles to and exercises the slot table itself. `states` matters
    /// because a concealment path that does not run looks exactly like one that runs
    /// and changes nothing; `dsDataErrorConcealed` in the OR is the difference.
    pub(crate) fn drive_decoder_over(stream: &[u8]) -> (usize, Option<(i32, i32)>, i32) {
        // SAFETY: `WelsCreateDecoder` hands out `Box::into_raw(dec) as *mut
        // ISVCDecoder`, so the pointer carries provenance for the whole
        // implementation object and every call below is the sequence a C caller
        // makes.
        unsafe {
            {
                let mut p_decoder: *mut ISVCDecoder = ptr::null_mut();
                assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS as i64);
                assert!(!p_decoder.is_null());
                let vtbl = (*p_decoder).lpVtbl;

                let mut dec_param = SDecodingParam::default();
                dec_param.uiTargetDqLayer = u8::MAX;
                dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
                dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
                assert_eq!(
                    ((*vtbl).Initialize)(p_decoder, &dec_param as *const SDecodingParam),
                    CM_RESULT_SUCCESS as i64
                );

                let mut frames = 0;
                let mut dims = None;
                let mut states = 0i32;
                for unit in crate::split_annexb_units(stream) {
                    let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                    let mut buf_info = SBufferInfo::default();
                    let ret = ((*vtbl).DecodeFrame2)(
                        p_decoder,
                        unit.as_ptr(),
                        unit.len() as i32,
                        p_dst.as_mut_ptr(),
                        &mut buf_info,
                    );
                    states |= ret.0;
                    if buf_info.iBufferStatus == 1 {
                        frames += 1;
                        let sys = *buf_info.UsrData.sys();
                        dims = Some((sys.iWidth, sys.iHeight));
                    }
                }

                // End of stream, then the zero-length call that flushes it. Without it a
                // stream whose only frame arrives there looks like one that decodes
                // nothing.
                let mut eos_flag = 1i32;
                ((*vtbl).SetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                    &mut eos_flag as *mut i32 as *mut c_void,
                );
                let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let ret = ((*vtbl).DecodeFrame2)(
                    p_decoder,
                    ptr::null(),
                    0,
                    p_dst.as_mut_ptr(),
                    &mut buf_info,
                );
                states |= ret.0;
                if buf_info.iBufferStatus == 1 {
                    frames += 1;
                    let sys = *buf_info.UsrData.sys();
                    dims = Some((sys.iWidth, sys.iHeight));
                }

                // …and the drain the flush announces. Leaving it out costs a frame on
                // every stream whose last picture is still buffered at EOS.
                let mut remaining = 0i32;
                ((*vtbl).GetOption)(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                    &mut remaining as *mut i32 as *mut c_void,
                );
                for _ in 0..remaining.clamp(0, 24) {
                    let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
                    let mut buf_info = SBufferInfo::default();
                    let ret = ((*vtbl).FlushFrame)(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
                    states |= ret.0;
                    if buf_info.iBufferStatus == 1 {
                        frames += 1;
                        let sys = *buf_info.UsrData.sys();
                        dims = Some((sys.iWidth, sys.iHeight));
                    }
                }

                ((*vtbl).Uninitialize)(p_decoder);
                WelsDestroyDecoder(p_decoder);
                (frames, dims, states)
            }
        }
    }

    /// One encoded frame, as the probe sees it: what the encoder called the frame,
    /// how many bytes of NAL it produced, and how many NALs those bytes came in.
    ///
    /// A frame's slices are exactly the NALs of its `VIDEO_CODING_LAYER` layers
    /// (`uiLayerType`), so `vcl_nals` is the coded slice count — which a count over
    /// every layer would not be, since an IDR also carries its parameter sets in a
    /// `NON_VIDEO_CODING_LAYER`.
    pub(crate) struct EncodedFrame {
        pub(crate) kind: EVideoFrameType,
        pub(crate) bytes: usize,
        pub(crate) vcl_nals: usize,
        pub(crate) frame_size: i32,
        /// `first_mb_in_slice` of every VCL NAL of this frame, in emission order.
        ///
        /// Read straight off the slice header as `ue(v)`; the value cannot need
        /// two zero bytes of prefix at any picture size this crate's probes use,
        /// so emulation prevention never falls inside it.
        pub(crate) first_mbs: Vec<u32>,
    }

    /// Fills `buf` with frame `f` of a synthetic I420 sequence that moves.
    ///
    /// Two motions at different velocities. The whole picture translates by (2, 1)
    /// samples per frame, so every macroblock has a non-zero motion vector; a bright
    /// block crosses it at (3, 2), so the macroblocks it touches disagree with their
    /// neighbours and the predicted vector is wrong for them. One velocity everywhere
    /// would run the search and then code `mvd = 0` at every macroblock but the first.
    ///
    /// The texture is a xor/quotient pattern rather than a gradient because a
    /// translated gradient is a gradient plus a constant, so the search would answer
    /// (0, 0) with a DC residual and nothing about motion estimation is measured.
    fn moving_i420(width: i32, height: i32, f: usize, buf: &mut [u8]) {
        fn texture(u: i32, v: i32) -> u8 {
            let a = (u.wrapping_mul(3) ^ v.wrapping_mul(5)) as u32;
            let b = (u / 7)
                .wrapping_mul(37)
                .wrapping_add((v / 5).wrapping_mul(53)) as u32;
            (16 + ((a ^ b) & 0x7f)) as u8
        }
        let (w, h) = (width as usize, height as usize);
        let (dx, dy) = (2 * f as i32, f as i32);
        // The bright block's own track, wrapped so it stays inside the picture.
        let (bw, bh) = (20.min(w / 2) as i32, 12.min(h / 2) as i32);
        let bx = (3 * f as i32) % (width - bw).max(1);
        let by = (2 * f as i32) % (height - bh).max(1);

        for y in 0..h {
            for x in 0..w {
                let inside = (x as i32) >= bx
                    && (x as i32) < bx + bw
                    && (y as i32) >= by
                    && (y as i32) < by + bh;
                buf[y * w + x] = if inside {
                    235
                } else {
                    texture(x as i32 + dx, y as i32 + dy)
                };
            }
        }
        let luma = w * h;
        let (cw, ch) = (w / 2, h / 2);
        for y in 0..ch {
            for x in 0..cw {
                let t = texture(2 * x as i32 + dx, 2 * y as i32 + dy);
                buf[luma + y * cw + x] = 112u8.wrapping_add(t & 0x1f);
                buf[luma + cw * ch + y * cw + x] = 144u8.wrapping_sub(t & 0x1f);
            }
        }
    }

    /// The configuration knobs the encoder probes vary.
    ///
    /// Each of these selects a different body of code, not a different parameter
    /// value:
    ///
    /// * `cabac` picks the entropy writers — `svc_set_mb_syn_cabac.rs` or
    ///   `svc_set_mb_syn_cavlc.rs`.
    /// * `complexity` picks the mode-decision family: `LOW_COMPLEXITY` installs
    ///   `SetFastCodingFunc` (`bFastMode`) and anything else
    ///   runs the fine intra partition search (`WelsMdIntraFinePartition`,
    ///   `WelsMdI4x4`) and the `pMemPredBlk4` ping-pong.
    ///
    /// * `slice_mode`/`slice_constraint` pick the slicing machinery.
    ///   `SM_SIZELIMITED_SLICE` is the only encode path with a loop of
    ///   its own (`WelsMdInterMbLoopOverDynamicSlice`), the only caller of the
    ///   CAVLC/CABAC stash-and-rollback pair (`StashMBStatus`/`StashPopMBStatus`)
    ///   and of `pDynamicBsBuffer`, and the only reader of
    ///   `CalculateNewSliceNum` → `ReallocSliceBuffer` → `ExtendLayerBuffer` →
    ///   `ReOrderSliceInLayer`. `slice_constraint` is `uiSliceSizeConstraint` in
    ///   bytes and is ignored by every other mode; validation refuses anything
    ///   ≤ `MAX_MACROBLOCK_SIZE_IN_BYTE` (400), and a slice closes at
    ///   `constraint - AVER_MARGIN_BYTES` (100) bytes of payload.
    #[derive(Debug, Copy, Clone)]
    pub(crate) struct EncoderProbeOptions {
        pub cabac: bool,
        pub complexity: ECOMPLEXITY_MODE,
        pub slice_mode: SliceModeEnum,
        pub slice_constraint: u32,
        /// `iMultipleThreadIdc`. Above 1 the encode takes the fork/join path;
        /// `bUseLoadBalancing` is forced off below, so it stays byte-deterministic.
        pub threads: u16,
        /// `uiSliceNum` for `SM_FIXEDSLCNUM_SLICE`/`SM_RASTER_SLICE`.
        pub slice_num: u32,
        /// `bUseLoadBalancing`. Default `false`, and every byte-asserting probe must
        /// leave it there: with it on, and `iMultipleThreadIdc >= uiSliceNum`, frame
        /// N+1's slice boundaries are a function of frame N's measured encode times, so
        /// the bitstream stops being a function of the input. `GetDefaultParams` sets it
        /// on, which is why the field is forced here rather than inherited.
        pub load_balancing: bool,
    }

    impl Default for EncoderProbeOptions {
        fn default() -> Self {
            Self {
                cabac: true,
                complexity: ECOMPLEXITY_MODE::LOW_COMPLEXITY,
                slice_mode: SliceModeEnum::SM_SINGLE_SLICE,
                slice_constraint: 0,
                threads: 1,
                slice_num: 1,
                load_balancing: false,
            }
        }
    }

    /// `first_mb_in_slice` of one Annex-B VCL NAL — the leading `ue(v)` of the
    /// slice header, and nothing else of it.
    ///
    /// Returns `None` for a NAL with no start code or no payload byte, which is
    /// what a caller should see rather than a panic: the counts beside it are
    /// still meaningful.
    fn first_mb_in_slice(nal: &[u8]) -> Option<u32> {
        // Skip the start code (3 or 4 bytes) and the one-byte NAL header.
        let body = if nal.starts_with(&[0, 0, 0, 1]) {
            &nal[4..]
        } else if nal.starts_with(&[0, 0, 1]) {
            &nal[3..]
        } else {
            nal
        };
        // Refuse rather than guess on the SVC extension types. `eNalUnitType` 14
        // (`NAL_UNIT_PREFIX`) and 20 (`NAL_UNIT_CODED_SLICE_EXT`) carry a three-byte
        // `SNalUnitHeaderExt` between the NAL header and the slice header, so skipping
        // one byte would read the extension as a `ue(v)` and return a plausible wrong
        // number.
        let nal_type = body.first()? & 0x1F;
        if nal_type == 14 || nal_type == 20 {
            return None;
        }
        let rbsp = body.get(1..)?;
        // `ue(v)`: count leading zero bits, then read that many more.
        let bit =
            |i: usize| -> Option<u32> { Some(((*rbsp.get(i / 8)? >> (7 - (i % 8))) & 1) as u32) };
        let mut lead = 0usize;
        while bit(lead)? == 0 {
            lead += 1;
            // 32 leading zeros is not a slice header; refuse rather than loop.
            if lead > 31 {
                return None;
            }
        }
        let mut v: u32 = 1;
        for k in 1..=lead {
            v = (v << 1) | bit(lead + k)?;
        }
        Some(v - 1)
    }

    #[allow(unsafe_code)]
    /// Encodes `frames` frames of [`moving_i420`] at `width` x `height` through the
    /// C ABI, and returns what came out frame by frame together with the encoder's
    /// own report of the resolution it is configured for.
    ///
    /// Calls the vtable thunks directly rather than the conveniences, for the reason
    /// [`drive_decoder_over`] gives.
    ///
    /// Three settings are fixed for the determinism the probe's assertions rest on:
    /// scene-change detection off (a detected cut would make frame 1 an IDR and there
    /// would be no inter frame), frame skip off (a skipped frame emits no NAL), and
    /// `uiIntraPeriod = 0` (no periodic IDR). The profile is `PRO_HIGH`, because a
    /// baseline layer forces CAVLC and the probe has to be able to ask for either
    /// writer; entropy coding and complexity come from [`EncoderProbeOptions`] and
    /// default to CABAC over `LOW_COMPLEXITY`.
    pub(crate) fn drive_encoder_over(
        width: i32,
        height: i32,
        frames: usize,
        opts: EncoderProbeOptions,
    ) -> (Vec<EncodedFrame>, (i32, i32)) {
        assert!(
            width % 16 == 0 && height % 16 == 0 && width >= 16 && height >= 16,
            "the driver synthesises whole macroblocks: {width}x{height} is not one"
        );
        // SAFETY: `WelsCreateSVCEncoder` hands out `Box::into_raw(enc) as *mut
        // ISVCEncoder`, so the pointer carries provenance for the whole
        // implementation object, and every call below is the sequence a C caller
        // makes.
        unsafe {
            let mut p_encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut p_encoder), CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());
            let vtbl = (*p_encoder).lpVtbl;

            let mut param = SEncParamExt::default();
            assert_eq!(
                ((*vtbl).GetDefaultParams)(p_encoder, &mut param),
                CM_RESULT_SUCCESS
            );
            param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            param.iPicWidth = width;
            param.iPicHeight = height;
            param.iTargetBitrate = 500_000;
            param.iMaxBitrate = UNSPECIFIED_BIT_RATE;
            param.iRCMode = RC_MODES::RC_QUALITY_MODE;
            param.fMaxFrameRate = 30.0;
            param.iTemporalLayerNum = 1;
            param.iSpatialLayerNum = 1;
            param.iComplexityMode = opts.complexity;
            param.uiIntraPeriod = 0;
            param.iNumRefFrame = AUTO_REF_PIC_COUNT;
            param.eSpsPpsIdStrategy = EParameterSetStrategy::CONSTANT_ID;
            param.iEntropyCodingModeFlag = if opts.cabac { 1 } else { 0 };
            param.bEnableFrameSkip = false;
            param.iMaxQp = 51;
            param.iMinQp = 0;
            param.iMultipleThreadIdc = opts.threads;
            // Off unless a probe asks for it. With it on and
            // `iMultipleThreadIdc >= uiSliceNum` the encoder takes `AdjustBaseLayer`
            // -> `DynamicAdjustSlicing`, whose slice boundaries for frame N+1 come
            // from frame N's measured encode times — so the bitstream stops being a
            // function of the input and any byte assertion stops meaning anything.
            // `GetDefaultParams` turns it on, so this line is a force, not a default.
            param.bUseLoadBalancing = opts.load_balancing;
            param.bEnableDenoise = false;
            param.bEnableBackgroundDetection = false;
            param.bEnableAdaptiveQuant = false;
            param.bEnableSceneChangeDetect = false;
            param.bEnableLongTermReference = false;
            param.bEnableFrameCroppingFlag = true;
            param.iLoopFilterDisableIdc = 0;
            param.sSpatialLayers[0].uiProfileIdc = EProfileIdc::PRO_HIGH;
            param.sSpatialLayers[0].uiLevelIdc = ELevelIdc::LEVEL_UNKNOWN;
            param.sSpatialLayers[0].iVideoWidth = width;
            param.sSpatialLayers[0].iVideoHeight = height;
            param.sSpatialLayers[0].fFrameRate = 30.0;
            param.sSpatialLayers[0].iSpatialBitrate = 500_000;
            param.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
            param.sSpatialLayers[0].sSliceArgument.uiSliceMode = opts.slice_mode;
            param.sSpatialLayers[0].sSliceArgument.uiSliceNum = opts.slice_num;
            param.sSpatialLayers[0].sSliceArgument.uiSliceSizeConstraint = opts.slice_constraint;
            assert_eq!(
                ((*vtbl).InitializeExt)(p_encoder, &param as *const SEncParamExt),
                CM_RESULT_SUCCESS
            );

            // The encoder's own answer for the geometry it is configured for, rather
            // than the geometry we asked for: the grid assertion has to be the
            // encoder's report or it asserts the test's own argument.
            let mut effective = SEncParamExt::default();
            assert_eq!(
                ((*vtbl).GetOption)(
                    p_encoder,
                    ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
                    &mut effective as *mut SEncParamExt as *mut c_void,
                ),
                CM_RESULT_SUCCESS
            );

            let luma = (width * height) as usize;
            let mut buf = vec![0u8; luma * 3 / 2];
            let mut out = Vec::with_capacity(frames);
            for f in 0..frames {
                moving_i420(width, height, f, &mut buf);
                // One derivation for all three planes. Three `as_mut_ptr()` calls
                // would each retag and pop the previous one.
                let base = buf.as_mut_ptr();
                let mut pic = SSourcePicture::default();
                pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
                pic.iPicWidth = width;
                pic.iPicHeight = height;
                pic.iStride[0] = width;
                pic.iStride[1] = width / 2;
                pic.iStride[2] = width / 2;
                pic.pData[0] = base;
                pic.pData[1] = base.add(luma);
                pic.pData[2] = base.add(luma + luma / 4);
                pic.uiTimeStamp = (f as i64) * 1000 / 30;

                let mut info = SFrameBSInfo::default();
                assert_eq!(
                    ((*vtbl).EncodeFrame)(p_encoder, &pic as *const SSourcePicture, &mut info),
                    CM_RESULT_SUCCESS,
                    "EncodeFrame failed at frame {f}"
                );
                let mut bytes = 0usize;
                let mut vcl_nals = 0usize;
                let mut first_mbs: Vec<u32> = Vec::new();
                for l in 0..info.iLayerNum as usize {
                    let lay = &info.sLayerInfo[l];
                    if lay.pNalLengthInByte.is_null() {
                        continue;
                    }
                    let is_vcl = lay.uiLayerType == LAYER_TYPE::VIDEO_CODING_LAYER as u8;
                    if is_vcl {
                        vcl_nals += lay.iNalCount as usize;
                    }
                    let mut at = 0usize;
                    for n in 0..lay.iNalCount as usize {
                        let len = *lay.pNalLengthInByte.add(n) as usize;
                        bytes += len;
                        if is_vcl && !lay.pBsBuf.is_null() {
                            let nal = std::slice::from_raw_parts(lay.pBsBuf.add(at), len);
                            if let Some(v) = first_mb_in_slice(nal) {
                                first_mbs.push(v);
                            }
                        }
                        at += len;
                    }
                }
                out.push(EncodedFrame {
                    kind: info.eFrameType,
                    bytes,
                    vcl_nals,
                    frame_size: info.iFrameSizeInBytes,
                    first_mbs,
                });
            }

            ((*vtbl).Uninitialize)(p_encoder);
            WelsDestroySVCEncoder(p_encoder);
            (out, (effective.iPicWidth, effective.iPicHeight))
        }
    }
}

#[cfg(test)]
mod f23_boundary_provenance {
    use super::*;

    #[allow(unsafe_code)]
    /// The consumer conveniences on `ISVCDecoder`/`ISVCEncoder` take `this` as a raw
    /// pointer rather than `&mut self`: those two structs are one pointer wide, while
    /// the thunk behind every slot casts `this` to a pointer to `CWelsDecoderImpl` /
    /// `CWelsH264SVCEncoderImpl` and writes the implementation object past those eight
    /// bytes — `decoder_init_c` writes at offset `0x20`.
    ///
    /// Miri is the assertion. What this must keep doing is calling a convenience, on
    /// both codecs, so that a re-introduced `&mut self` receiver is caught by the
    /// checker.
    #[test]
    fn conveniences_call_through_the_whole_impl_allocation() {
        unsafe {
            // --- the decoder half -------------------------------------------
            let mut p_decoder = ptr::null_mut();
            assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;
            dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
            dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
            // The write that is out of bounds for an eight-byte borrow: this call
            // stores `*pParam` into `SWelsDecoderContext::pParam` via `core.initialize()`.
            assert_eq!(
                ISVCDecoder::Initialize(p_decoder, &dec_param),
                CM_RESULT_SUCCESS as i64
            );

            // `bEndOfStream` lives further out still, and this pair writes then reads
            // it — so the probe covers a round trip and not only the init.
            let mut eos = 1i32;
            ISVCDecoder::SetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                ptr::from_mut(&mut eos).cast(),
            );
            let mut eos_back = 0i32;
            ISVCDecoder::GetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                ptr::from_mut(&mut eos_back).cast(),
            );
            assert_eq!(eos_back, 1, "END_OF_STREAM did not round-trip");

            assert_eq!(
                ISVCDecoder::Uninitialize(p_decoder),
                CM_RESULT_SUCCESS as i64
            );
            WelsDestroyDecoder(p_decoder);

            // --- the encoder half -------------------------------------------
            let mut p_encoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut p_encoder), CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());

            let mut enc_param = SEncParamBase::default();
            enc_param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            enc_param.iPicWidth = 64;
            enc_param.iPicHeight = 64;
            enc_param.fMaxFrameRate = 30.0;
            enc_param.iTargetBitrate = 64000;
            assert_eq!(
                ISVCEncoder::Initialize(p_encoder, &enc_param),
                CM_RESULT_SUCCESS
            );
            assert_eq!(ISVCEncoder::Uninitialize(p_encoder), CM_RESULT_SUCCESS);
            WelsDestroySVCEncoder(p_encoder);
        }
    }
}

#[cfg(test)]
mod send_verdict {
    use super::*;

    /// `true` iff the named type is `Send`, decided at compile time and reported rather
    /// than enforced.
    ///
    /// A plain `fn assert_send<T: Send>() {}` states a verdict only when the verdict is
    /// yes; when it is no the file stops compiling. This is the
    /// inherent-method-beats-trait-method trick, and it has to be a macro rather than a
    /// generic function: inside `fn is_send<T>()` the method resolves at definition
    /// time, where `T` is not known to be `Send`, so the answer would be `false` for
    /// everything. Expanded at a concrete type it resolves at the use site.
    macro_rules! is_send {
        ($t:ty) => {{
            struct Probe<T>(std::marker::PhantomData<T>);
            // Each expansion uses exactly one of the two arms and leaves the other
            // dead, so both carry the allow: the trait arm is dead when `$t` is `Send`,
            // the inherent one when it is not.
            #[allow(dead_code)]
            trait NotSend {
                fn probe(&self) -> bool {
                    false
                }
            }
            impl<T> NotSend for Probe<T> {}
            impl<T: Send> Probe<T> {
                #[allow(dead_code)]
                fn probe(&self) -> bool {
                    true
                }
            }
            Probe::<$t>(std::marker::PhantomData).probe()
        }};
    }

    /// Neither core is `Send`, for the same reason: `SWelsDecoderContext` and
    /// `sWelsEncCtx` each carry `*mut`/`*const` members below the boundary, and a raw
    /// pointer is `!Send` by construction, so `Decoder` and `Encoder` are too.
    ///
    /// The encoder's own threading does not depend on this: it forks onto its worker
    /// pool through a `scope` shaped like `std::thread::scope`, and the workers take
    /// what they reach by static partition. `Send` on the whole encoder is the property
    /// a consumer would need to move a codec between threads.
    #[test]
    fn the_cores_are_not_send_yet_and_this_is_the_inventory() {
        assert!(
            !is_send!(Decoder),
            "Decoder became Send — rewrite the verdict in this test and in the \
             session log, with what changed"
        );
        assert!(
            !is_send!(Encoder),
            "Encoder became Send — rewrite the verdict in this test and in the \
             session log, with what changed"
        );
        // The probe itself has to be able to say yes, or the assertions above are
        // vacuous.
        assert!(is_send!(u32), "the Send probe reports false for u32");
        assert!(
            !is_send!(*mut u8),
            "the Send probe reports true for a raw pointer"
        );
    }
}

#[cfg(test)]
mod abi_panic_guard {
    use super::*;

    #[allow(unsafe_code)]
    /// A panic inside a decoder entry comes back as that entry's failure code, and
    /// the process is still here to assert it.
    ///
    /// Without `abi_guard!` this would be a non-unwinding panic and a SIGABRT that
    /// takes every other test in the binary with it.
    #[test]
    fn a_panic_inside_a_decoder_thunk_becomes_dsbitstreamerror() {
        unsafe {
            let mut decoder: *mut ISVCDecoder = ptr::null_mut();
            assert_eq!(WelsCreateDecoder(&mut decoder), CM_RESULT_SUCCESS as i64);
            let param = SDecodingParam {
                uiTargetDqLayer: u8::MAX,
                ..SDecodingParam::default()
            };
            assert_eq!(
                ISVCDecoder::Initialize(decoder, &param as *const SDecodingParam),
                CM_RESULT_SUCCESS as i64
            );

            let mut p_dst: [*mut u8; 3] = [ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let bytes = [0u8, 0, 0, 1, 0x67];

            PANIC_PROBE.with(|p| p.set(PROBE_DECODE_FRAME2));
            let state = ISVCDecoder::DecodeFrame2(
                decoder,
                bytes.as_ptr(),
                bytes.len() as i32,
                p_dst.as_mut_ptr(),
                &mut info,
            );
            PANIC_PROBE.with(|p| p.set(0));

            assert_eq!(
                state,
                DECODING_STATE::dsBitstreamError,
                "a caught panic must be reported as this slot's failure code"
            );

            // Alive, and the object is still usable enough to tear down — the narrow
            // half of the `AssertUnwindSafe` claim.
            ISVCDecoder::Uninitialize(decoder);
            WelsDestroyDecoder(decoder);
        }
    }

    #[allow(unsafe_code)]
    /// The encoder half: `cmUnknownReason`, which is what an encode entry has for
    /// "something went wrong and it was not your parameters".
    #[test]
    fn a_panic_inside_an_encoder_thunk_becomes_cm_unknown_reason() {
        unsafe {
            let mut encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(WelsCreateSVCEncoder(&mut encoder), CM_RESULT_SUCCESS);
            let mut base = SEncParamBase {
                iPicWidth: 176,
                iPicHeight: 144,
                iTargetBitrate: 128_000,
                fMaxFrameRate: 30.0,
                ..SEncParamBase::default()
            };
            assert_eq!(
                ISVCEncoder::Initialize(encoder, &mut base as *const SEncParamBase),
                CM_RESULT_SUCCESS
            );

            let mut plane = vec![0u8; 176 * 144 * 3 / 2];
            let mut pic = SSourcePicture::default();
            pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            pic.iPicWidth = 176;
            pic.iPicHeight = 144;
            pic.iStride = [176, 88, 88, 0];
            pic.pData = [
                plane.as_mut_ptr(),
                plane.as_mut_ptr().add(176 * 144),
                plane.as_mut_ptr().add(176 * 144 * 5 / 4),
                ptr::null_mut(),
            ];
            let mut bs = SFrameBSInfo::default();

            PANIC_PROBE.with(|p| p.set(PROBE_ENCODE_FRAME));
            let rc = ISVCEncoder::EncodeFrame(encoder, &pic as *const SSourcePicture, &mut bs);
            PANIC_PROBE.with(|p| p.set(0));

            assert_eq!(
                rc, CM_UNKNOWN_REASON,
                "a caught panic must be reported as this slot's failure code"
            );

            ISVCEncoder::Uninitialize(encoder);
            WelsDestroySVCEncoder(encoder);
        }
    }

    /// The probe is this thread's, so a thunk called from another thread runs the real
    /// body and the switch cannot fire in tests running in parallel with these two.
    #[test]
    fn the_probe_does_not_leak_to_other_threads() {
        PANIC_PROBE.with(|p| p.set(PROBE_DECODE_FRAME2));
        let seen = std::thread::spawn(|| PANIC_PROBE.with(|p| p.get()))
            .join()
            .unwrap();
        PANIC_PROBE.with(|p| p.set(0));
        assert_eq!(seen, 0);
    }
}
