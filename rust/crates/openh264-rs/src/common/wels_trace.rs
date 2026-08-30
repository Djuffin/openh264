#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]
// **S8.9: sealed.** `WelsLog` took the application's own `SLogContext*` and called
// the callback inside it, so this file carried both the deref and the C-ABI call.
// S8.1 retired the deref (the context travels by value) and S8.9 moved the call
// behind `TraceUserCtx::deliver` in `src/api/`, so nothing here is unsafe any more —
// and the `clippy::not_unsafe_ptr_arg_deref` allow that stood here went with the raw
// parameter that earned it.
#![forbid(unsafe_code)]

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
//! **The default sink is `welsStderrTrace` at `WELS_LOG_WARNING`, which is
//! upstream's** (decision **D-api-1**, 2026-08-21; T8.C6).
//!
//! T8.B6 left the default at `None` and recorded it as a stated divergence, on the
//! grounds that the malformed corpus alone would emit a line per damaged access unit.
//! That reasoning is about the *project's instruments*, not about the library, and
//! D-api-1 rules the other way: **a drop-in that is silent where the reference speaks
//! is a divergence**, and one a consumer cannot detect by reading the header. A
//! caller who wants silence installs a quiet callback — which is exactly what a C
//! consumer does, and what this tree's high-volume harnesses now do.

use std::ffi::{CString, c_char, c_void};

pub use crate::api::codec_api::{TraceUserCtx, WelsTraceCallback};

/// `codec_app_def.h:323-331` — the trace levels, and `WELS_LOG_DEFAULT`.
///
/// **These are a bit mask upstream, not consecutive integers** (decision
/// **D-fid-4**, 2026-08-26, from F184): `1 << 0 .. 1 << 5`. Until that ruling the
/// port declared them 0,1,2,3,4,5 here and in four other places, which made this
/// port's `DEBUG` bit-identical to the reference's `INFO` — ABI-visibly, because
/// the level is the second argument of the caller's own trace callback and the
/// value `SetOption(ENCODER_OPTION_TRACE_LEVEL, ..)` is compared against
/// (`m_iTraceLevel < iLevel`, `welsCodecTrace.cpp:76`). `ERROR` and `WARNING`
/// agreed by coincidence; everything above them did not.
///
/// The threshold keeps working because the values stay monotonic, and nothing in
/// either codec does arithmetic on them — they are compared and matched only.
/// `WELS_LOG_LEVEL_COUNT` is a *count*, not a mask member, and stays 6.
pub const WELS_LOG_QUIET: i32 = 0;
pub const WELS_LOG_ERROR: i32 = 1 << 0;
pub const WELS_LOG_WARNING: i32 = 1 << 1;
pub const WELS_LOG_INFO: i32 = 1 << 2;
pub const WELS_LOG_DEBUG: i32 = 1 << 3;
pub const WELS_LOG_DETAIL: i32 = 1 << 4;
pub const WELS_LOG_RESV: i32 = 1 << 5;
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
    ///
    /// **S8.9**: a [`TraceUserCtx`] rather than a bare `*mut c_void`. Same bytes
    /// (`repr(transparent)`), but constructing one and invoking through one both
    /// live in `src/api/`, so this field no longer carries an `unsafe` obligation
    /// into every module that holds an `SLogContext`.
    pub pLogCtx: TraceUserCtx,
    /// The boundary object's address, for the message tag's `this = 0x…` only.
    /// An address and not a pointer: `utils.cpp:51` formats it with `%p` and does
    /// nothing else with it, so a value that cannot be dereferenced is the honest
    /// type and the F38 question does not arise.
    pub pCodecInstance: usize,
    /// The level to filter at. `welsCodecTrace::CodecTrace` holds this in the
    /// trace object and reaches it through the back-pointer; here it travels with
    /// the rest.
    pub iTraceLevel: i32,
    /// **Explicit tail padding, and it has to be explicit.** This struct is a field
    /// of `sWelsEncCtx`, whose whole byte image is pinned against a `memset`-zero
    /// shell by `encoder_context.rs`'s equivalence test — a test that reads each
    /// field's extent byte for byte and whose safety argument is that *no field
    /// outside its by-value list has interior or trailing padding*. `iTraceLevel`
    /// took the size to 28 and the alignment took it back to 32, so four bytes of
    /// the context's image became uninitialised and Miri said so at the phase exit.
    /// Naming the four bytes restores the premise rather than weakening the test.
    /// Always zero.
    pub _reserved: u32,
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
/// The reference splits the work: `WelsLog` builds the `[OpenH264] this = …, Error:`
/// tag and hands the *format string* plus a `va_list` to the sink, and the sink
/// filters on level and formats. Rust has no portable `va_list`, so every call site
/// in this port formats first and passes a `&str`; the filter and the tag are both
/// here, and the observable — one call to the caller's callback per delivered
/// message, with the level and the tagged text — is the same.
// **S8.9: no `unsafe` left here at all.** The call to the application's sink is
// `TraceUserCtx::deliver`, in `src/api/` — the C-ABI obligation lives at the
// boundary that owns it, and this function is the plain formatter it always read
// as. The by-value parameter (S8.1) had already retired the deref and its null
// guard.
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
    ctx.pLogCtx.deliver(pfLog, iLevel, &cline);
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

/// `welsStderrTrace` — `welsCodecTrace.cpp:49`, which is one `fprintf`.
///
/// The default sink, installed by the constructor below. It is an `extern "C" fn`
/// because it occupies the same slot a caller's own callback does: `SetTraceCallback`
/// replaces it, and `GetOption(*_TRACE_CALLBACK)` hands its address back, so it has
/// to be the same type as anything a consumer could install.
///
/// The default trace sink now lives in the C-ABI island — see
/// [`crate::api::codec_api::welsStderrTrace`]. Re-exported here because this is the
/// module every caller reaches it through, and because `SLogContext`'s own default
/// names it.
pub use crate::api::codec_api::welsStderrTrace;

impl Default for welsCodecTrace {
    /// `welsCodecTrace::welsCodecTrace()` — `welsCodecTrace.cpp:53`, both statements:
    /// the level is `WELS_LOG_DEFAULT` and the sink is [`welsStderrTrace`] (D-api-1).
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

    pub fn SetTraceCallback(&mut self, func: WelsTraceCallback) {
        self.m_sLogCtx.pfLog = func;
    }

    /// **S8.9**: takes the token, not the pointer. Callers at the C-ABI boundary
    /// mint one with [`TraceUserCtx::from_abi`], which is where the rawness belongs.
    pub fn SetTraceCallbackContext(&mut self, pCtx: TraceUserCtx) {
        self.m_sLogCtx.pLogCtx = pCtx;
    }

    /// The value to stamp into a codec context's `sLogCtx`, and to re-stamp
    /// whenever one of the setters above runs on a live codec.
    pub fn log_context(&self) -> SLogContext {
        self.m_sLogCtx
    }
}
