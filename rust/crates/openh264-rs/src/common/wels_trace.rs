#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![forbid(unsafe_code)]

//! The codec trace, shared by both codecs.
//!
//! `codec/common/inc/utils.h` declares `SLogContext` and `WelsLog`;
//! `codec/common/inc/welsCodecTrace.h` declares the `welsCodecTrace` object the two
//! boundary classes own; `codec/api/wels/codec_api.h` declares `WelsTraceCallback`,
//! the function the caller installs.
//!
//! `SLogContext` carries what the sink needs rather than a back-pointer to the trace
//! object: the trace object is a member of the boundary object, and a `&mut` retag of
//! the owner invalidates any pointer derived from a member. It holds the user's
//! callback, the user's context, the instance address for the message tag, and the
//! level to filter at. A later `SetOption` reaches a codec context's copy by
//! re-stamping that copy, one line at each of the six option arms.
//!
//! The default sink is `welsStderrTrace` at `WELS_LOG_WARNING`; a caller who wants
//! silence installs a quiet callback.

use std::ffi::CString;

pub use crate::api::codec_api::{TraceUserCtx, WelsTraceCallback};

/// `codec_app_def.h:323-331` — the trace levels, and `WELS_LOG_DEFAULT`.
///
/// A bit mask, not consecutive integers: `1 << 0 .. 1 << 5`. The level is the second
/// argument of the caller's trace callback and the value the trace-level option is
/// compared against (`m_iTraceLevel < iLevel`). The values are only compared and
/// matched, never arithmetic. `WELS_LOG_LEVEL_COUNT` is a count, not a mask member.
pub const WELS_LOG_QUIET: i32 = 0;
pub const WELS_LOG_ERROR: i32 = 1 << 0;
pub const WELS_LOG_WARNING: i32 = 1 << 1;
pub const WELS_LOG_INFO: i32 = 1 << 2;
pub const WELS_LOG_DEBUG: i32 = 1 << 3;
pub const WELS_LOG_DETAIL: i32 = 1 << 4;
pub const WELS_LOG_RESV: i32 = 1 << 5;
pub const WELS_LOG_LEVEL_COUNT: i32 = 6;
pub const WELS_LOG_DEFAULT: i32 = WELS_LOG_WARNING;

/// `utils.h:45`. Both the tag and the formatted message are truncated at this width.
pub const MAX_LOG_SIZE: usize = 1024;

/// `TagLogContext` — `utils.h:53`.
///
/// The copy that travels: both codec contexts store one in `sLogCtx`, so code far
/// below the boundary can log without reaching back up to it.
///
/// The two callback fields are crate-private, which is what keeps [`WelsLog`] safe:
///
/// ```compile_fail,E0451
/// # unsafe extern "C" fn sink(_: *mut std::ffi::c_void, _: i32, _: *const std::ffi::c_char) {}
/// use openh264_rs::common::wels_trace::SLogContext;
/// let _ = SLogContext { pfLog: Some(sink), ..SLogContext::default() };
/// ```
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    /// The callback the caller installed, or `None`.
    pub(crate) pfLog: WelsTraceCallback,
    /// The caller's opaque context, handed back to `pfLog` untouched. Never
    /// dereferenced by this crate.
    pub(crate) pLogCtx: TraceUserCtx,
    /// The boundary object's address, for the message tag's `this = 0x…` only. An
    /// address and not a pointer: it is formatted with `%p` and used for nothing else.
    pub pCodecInstance: usize,
    /// The level to filter at.
    pub iTraceLevel: i32,
    /// Explicit tail padding: `iTraceLevel` takes the size to 28 and alignment takes
    /// it to 32. Always zero.
    pub _reserved: u32,
}

impl Default for SLogContext {
    /// All zeros. `WELS_LOG_DEFAULT` is set in [`welsCodecTrace`]'s constructor and
    /// travels from there by the stamp.
    fn default() -> Self {
        Self {
            pfLog: None,
            pLogCtx: TraceUserCtx::default(),
            pCodecInstance: 0,
            iTraceLevel: WELS_LOG_QUIET,
            _reserved: 0,
        }
    }
}

/// `void WelsLog (SLogContext*, int32_t iLevel, const char* kpFmt, ...)` —
/// `utils.cpp:51`, with `welsCodecTrace::CodecTrace`'s level filter folded in.
///
/// Call sites format the message themselves and pass a `&str`; the level filter and
/// the `[OpenH264] this = …` tag are both applied here, and the caller's callback is
/// invoked once per delivered message.
pub fn WelsLog(ctx: SLogContext, iLevel: i32, msg: &str) {
    let Some(pfLog) = ctx.pfLog else {
        return;
    };
    // `welsCodecTrace::CodecTrace`, first statement.
    if ctx.iTraceLevel < iLevel {
        return;
    }
    let tag = match iLevel {
        WELS_LOG_ERROR => "Error:",
        WELS_LOG_WARNING => "Warning:",
        WELS_LOG_INFO => "Info:",
        WELS_LOG_DEBUG => "Debug:",
        _ => "Detail:",
    };
    let mut line = format!("[OpenH264] this = 0x{:x}, {tag}{msg}", ctx.pCodecInstance);
    // The whole line is bounded at `MAX_LOG_SIZE`, which is the same guarantee for a
    // caller whose buffer is that size.
    if line.len() >= MAX_LOG_SIZE {
        let mut end = MAX_LOG_SIZE - 1;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    // A message with an interior NUL cannot be a C string; no call site produces one.
    let Ok(cline) = CString::new(line) else {
        return;
    };
    ctx.pLogCtx.deliver(pfLog, iLevel, &cline);
}

/// `welsCodecTrace` — `welsCodecTrace.h:41`.
///
/// The one place the caller's trace settings live before they are stamped into a
/// codec context.
#[derive(Debug)]
pub struct welsCodecTrace {
    pub m_sLogCtx: SLogContext,
}

/// `welsStderrTrace` — `welsCodecTrace.cpp:49`, one `fprintf`.
///
/// The default sink, installed by the constructor below. An `extern "C" fn` because it
/// occupies the same slot a caller's own callback does: `SetTraceCallback` replaces it
/// and `GetOption(*_TRACE_CALLBACK)` hands its address back. Defined in the C-ABI
/// island — [`crate::api::codec_api::welsStderrTrace`] — and re-exported here.
pub use crate::api::codec_api::welsStderrTrace;

impl Default for welsCodecTrace {
    /// `welsCodecTrace::welsCodecTrace()` — `welsCodecTrace.cpp:53`, both statements:
    /// the level is `WELS_LOG_DEFAULT` and the sink is [`welsStderrTrace`].
    fn default() -> Self {
        Self {
            m_sLogCtx: SLogContext {
                iTraceLevel: WELS_LOG_DEFAULT,
                pfLog: Some(welsStderrTrace),
                ..SLogContext::default()
            },
        }
    }
}

impl welsCodecTrace {
    pub fn new() -> Self {
        Self::default()
    }

    /// `welsCodecTrace::SetCodecInstance` — `welsCodecTrace.cpp:87`, which writes
    /// `m_sLogCtx.pCodecInstance` and not a member of its own.
    pub fn SetCodecInstance(&mut self, instance: usize) {
        self.m_sLogCtx.pCodecInstance = instance;
    }

    /// `welsCodecTrace::SetTraceLevel` — negative levels are ignored, as there.
    pub fn SetTraceLevel(&mut self, kiLevel: u32) {
        let level = kiLevel as i32;
        if level >= 0 {
            self.m_sLogCtx.iTraceLevel = level;
        }
    }

    pub fn GetTraceLevel(&self) -> i32 {
        self.m_sLogCtx.iTraceLevel
    }

    pub(crate) fn SetTraceCallback(&mut self, func: WelsTraceCallback) {
        self.m_sLogCtx.pfLog = func;
    }

    /// Callers at the C-ABI boundary mint the token with
    /// [`TraceUserCtx::from_abi`].
    pub(crate) fn SetTraceCallbackContext(&mut self, pCtx: TraceUserCtx) {
        self.m_sLogCtx.pLogCtx = pCtx;
    }

    /// The value to stamp into a codec context's `sLogCtx`, and to re-stamp
    /// whenever one of the setters above runs on a live codec.
    pub fn log_context(&self) -> SLogContext {
        self.m_sLogCtx
    }
}
