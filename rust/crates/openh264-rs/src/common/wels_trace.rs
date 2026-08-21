#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

//! The codec trace, shared by both codecs.
//!
//! `codec/common/inc/utils.h` declares `SLogContext` and `WelsLog`;
//! `codec/common/inc/welsCodecTrace.h` declares the `welsCodecTrace` object that
//! the two boundary classes own; `codec/api/wels/codec_api.h` declares
//! `WelsTraceCallback`, the function the *caller* installs. Both codecs include
//! all three in C++, so this port has one copy of each here rather than one per
//! module — which retires the `SLogContext x2` and `WelsTraceCallback x2` entries
//! the duplicate census carried.
//!
//! # What this replaced, and what it decided
//!
//! **T8.B6.** Until this module existed, `WelsLog` was a stub in *two* places
//! (`encoder/wels_encoder_ext.rs` and `decoder/decoder_core.rs`), each of them
//! `let _ = (pLogCtx, iLevel, msg);`. Nothing in the crate read `m_fpTrace` or
//! `SLogContext::pfLog`, so a caller who installed a trace callback through
//! `ENCODER_OPTION_TRACE_CALLBACK` or `DECODER_OPTION_TRACE_CALLBACK` — a
//! documented option on a documented interface — was handed silence.
//!
//! **The one structural departure from the reference, and why.** In C++
//! `SLogContext::pfLog` is `StaticCodecTrace` and `pLogCtx` is *the
//! `welsCodecTrace` object itself*: the sink is a trampoline that finds the user's
//! callback by following a back-pointer, which is what lets a `SetOption` after
//! `Initialize` reach the copy of `SLogContext` that lives inside the codec
//! context. That back-pointer cannot be written here. The trace object is a member
//! of the boundary object, every entry point reaches the boundary object through
//! `&mut`, and a `&mut` retag of the owner invalidates any pointer previously
//! derived from the member — the F38 class exactly, and this instance would be
//! taken on the *logging* path, where nothing would ever observe it going wrong.
//!
//! So `SLogContext` carries what the sink needs instead of a route to it: the
//! user's callback, the user's context, the instance address for the message tag,
//! and the level to filter at. The indirection the back-pointer bought — a later
//! `SetOption` reaching the context's copy — is bought instead by *re-stamping*
//! that copy when the option is set, which is one line at each of the six option
//! arms and is checked by the covering tests.
//!
//! **The default sink is `None`, and the reference's is `welsStderrTrace`.** That
//! is a stated divergence rather than an oversight: upstream's default writes every
//! warning and error to the process's stderr (`WELS_LOG_DEFAULT` is
//! `WELS_LOG_WARNING`), this port has never done it, and turning it on is a
//! library-behaviour decision with a measurable cost on this project's own
//! instruments — the malformed corpus alone would emit a trace line per damaged
//! access unit across 2707 rows. What T8.B6 delivers is the *installed callback*
//! path, which is the one `codec_api.h` documents and a consumer can observe. A
//! consumer who wants the stderr behaviour installs a callback that writes to it.

use std::ffi::{CString, c_char, c_void};

pub use crate::api::codec_api::WelsTraceCallback;

/// `codec_app_def.h:323` — the trace levels, and `WELS_LOG_DEFAULT`.
pub const WELS_LOG_QUIET: i32 = 0;
pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;
pub const WELS_LOG_DEBUG: i32 = 4;
pub const WELS_LOG_DETAIL: i32 = 5;
pub const WELS_LOG_LEVEL_COUNT: i32 = 6;
pub const WELS_LOG_DEFAULT: i32 = WELS_LOG_WARNING;

/// `utils.h:45`. The reference truncates both the tag and the formatted message at
/// this width; so does [`WelsLog`].
pub const MAX_LOG_SIZE: usize = 1024;

/// `TagLogContext` — `utils.h:53`.
///
/// The copy that travels: `WelsInitEncoderExt` stores one in `sWelsEncCtx::sLogCtx`
/// and the decoder's `WelsDecoderDefaults` stores one in
/// `SWelsDecoderContext::sLogCtx`, so that code far below the boundary can log
/// without reaching back up to it. See the module comment for why this one carries
/// the callback rather than a route to the object that holds it.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    /// The callback the caller installed, or `None` — the reference's
    /// `pfLog`/`m_fpTrace` pair collapsed into the half that is observable.
    pub pfLog: WelsTraceCallback,
    /// **C-ABI**: the caller's opaque context, handed back to `pfLog` untouched.
    /// Never dereferenced by this crate.
    pub pLogCtx: *mut c_void,
    /// The boundary object's address, for the message tag's `this = 0x…` only.
    /// An address and not a pointer: `utils.cpp:51` formats it with `%p` and does
    /// nothing else with it, so a value that cannot be dereferenced is the honest
    /// type and the F38 question does not arise.
    pub pCodecInstance: usize,
    /// The level to filter at. `welsCodecTrace::CodecTrace` holds this in the
    /// trace object and reaches it through the back-pointer; here it travels with
    /// the rest.
    pub iTraceLevel: i32,
}

impl Default for SLogContext {
    /// **All zeros, deliberately.** In C++ this struct is a member of a context the
    /// codec `memset`s, and the level it filters at lives in the `welsCodecTrace`
    /// the back-pointer reaches — so a zeroed `SLogContext` is the reference's own
    /// initial state, and `sWelsEncCtx`'s zeroed-shell equivalence test (Phase 5b)
    /// is what holds this to it. `WELS_LOG_DEFAULT` is set where the reference sets
    /// it, in [`welsCodecTrace`]'s constructor, and travels from there by the
    /// stamp.
    fn default() -> Self {
        Self {
            pfLog: None,
            pLogCtx: std::ptr::null_mut(),
            pCodecInstance: 0,
            iTraceLevel: WELS_LOG_QUIET,
        }
    }
}

/// `void WelsLog (SLogContext*, int32_t iLevel, const char* kpFmt, ...)` —
/// `utils.cpp:51`, with `welsCodecTrace::CodecTrace`'s level filter folded in.
///
/// The reference splits the work: `WelsLog` builds the `[OpenH264] this = …, Error:`
/// tag and hands the *format string* plus a `va_list` to the sink, and the sink
/// filters on level and formats. Rust has no portable `va_list`, so every call site
/// in this port formats first and passes a `&str`; the filter and the tag are both
/// here, and the observable — one call to the caller's callback per delivered
/// message, with the level and the tagged text — is the same.
pub fn WelsLog(pLogCtx: *mut SLogContext, iLevel: i32, msg: &str) {
    if pLogCtx.is_null() {
        return;
    }
    // `SLogContext` is `Copy` and the caller owns the storage; taking a value here
    // keeps the callback's own re-entry (a callback that calls back into the codec)
    // from aliasing a borrow of it.
    let ctx = unsafe { *pLogCtx };
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
    // The reference's tag is `"[OpenH264] this = 0x%p, Error:"`, and `%p` prints its
    // own `0x` on every platform this port targets — so upstream's line really does
    // read `this = 0x0x16b5d7000`. The doubled prefix is not reproduced.
    let mut line = format!("[OpenH264] this = 0x{:x}, {tag}{msg}", ctx.pCodecInstance);
    // `WelsSnprintf`/`WelsStrcat` bound the reference's tag and message at
    // `MAX_LOG_SIZE` each; one bound over the whole line is the same guarantee for
    // a caller whose buffer is `MAX_LOG_SIZE`.
    if line.len() >= MAX_LOG_SIZE {
        let mut end = MAX_LOG_SIZE - 1;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    // A message with an interior NUL cannot be a C string; the reference cannot
    // produce one (its inputs are `printf` formats) and neither can this port's call
    // sites, so this is a guard and not a policy.
    let Ok(cline) = CString::new(line) else {
        return;
    };
    unsafe { pfLog(ctx.pLogCtx, iLevel, cline.as_ptr() as *const c_char) };
}

/// `welsCodecTrace` — `welsCodecTrace.h:41`.
///
/// The reference's four members are `m_iTraceLevel`, `m_fpTrace`, `m_pTraceCtx` and
/// `m_sLogCtx`, the first three of which the sink reaches through `m_sLogCtx`'s
/// back-pointer. With the back-pointer gone (module comment) they *are*
/// `m_sLogCtx`, and this object is the one place the caller's settings live before
/// they are stamped into a codec context.
///
/// `m_pCodecInstance` stood here as a fifth member and had **no counterpart in the
/// reference at all** — `welsCodecTrace.h` has no such field, and `SetCodecInstance`
/// writes `m_sLogCtx.pCodecInstance`. It had no reader either. Deleted at T8.B6.
#[derive(Debug)]
pub struct welsCodecTrace {
    pub m_sLogCtx: SLogContext,
}

impl Default for welsCodecTrace {
    /// `welsCodecTrace::welsCodecTrace()` — `welsCodecTrace.cpp:53`. The level is
    /// the constructor's business in both trees; the reference also installs
    /// `welsStderrTrace` here, and this port does not (module comment).
    fn default() -> Self {
        Self {
            m_sLogCtx: SLogContext {
                iTraceLevel: WELS_LOG_DEFAULT,
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

    pub fn SetTraceCallback(&mut self, func: WelsTraceCallback) {
        self.m_sLogCtx.pfLog = func;
    }

    pub fn SetTraceCallbackContext(&mut self, pCtx: *mut c_void) {
        self.m_sLogCtx.pLogCtx = pCtx;
    }

    /// The value to stamp into a codec context's `sLogCtx`, and to re-stamp
    /// whenever one of the setters above runs on a live codec.
    pub fn log_context(&self) -> SLogContext {
        self.m_sLogCtx
    }
}
