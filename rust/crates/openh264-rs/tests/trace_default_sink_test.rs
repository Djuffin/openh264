//! The default trace sink is upstream's, and a consumer who installs nothing still
//! hears the library speak.
//!
//! `welsCodecTrace::welsCodecTrace()` installs `welsStderrTrace` at
//! `WELS_LOG_DEFAULT`, and so does this port.
//!
//! **The two constructors do not agree.**
//! `welsCodecTrace()` sets `WELS_LOG_WARNING`; `CWelsDecoder::CWelsDecoder()` then
//! calls `SetTraceLevel (WELS_LOG_ERROR)` (`welsDecoderExt.cpp:164`) and
//! `CWelsH264SVCEncoder` does not (`welsEncoderExt.cpp:166`). So the decoder's default
//! is ERROR and the encoder's is WARNING.
//!
//! # Capturing stderr
//!
//! `welsStderrTrace` writes to fd 2, exactly as `fprintf (stderr, ...)` does, so
//! libtest's output capture does not see it and neither would an in-process reader
//! without `dup2` surgery — which would race every other test in the binary. Instead
//! each case **re-executes this test binary** as a child with an environment variable
//! selecting the case, and reads the child's stderr. One process per case, no fd
//! games, no ordering assumptions.

use openh264_rs::api::codec_api::*;
use std::process::Command;

const CHILD_ENV: &str = "OPENH264_TRACE_SINK_CASE";

/// Runs `case` in a child copy of this binary and returns everything it wrote to
/// stderr.
fn stderr_of(case: &str) -> String {
    let exe = std::env::current_exe().expect("current_exe");
    let out = Command::new(exe)
        .args(["the_child_case", "--exact", "--nocapture"])
        .env(CHILD_ENV, case)
        .output()
        .expect("re-exec the test binary");
    assert!(
        out.status.success(),
        "child case {case} exited {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The child's body. A no-op in the parent, where `CHILD_ENV` is unset.
#[test]
fn the_child_case() {
    let Ok(case) = std::env::var(CHILD_ENV) else { return };
    unsafe {
        match case.as_str() {
            // `welsDecoderExt.cpp:266-268` — the null-parameter arm logs at
            // `WELS_LOG_ERROR` and returns `cmInitParaError`.
            "decoder_default" => {
                let mut d: *mut ISVCDecoder = std::ptr::null_mut();
                assert_eq!(i64::from(WelsCreateDecoder(&mut d)), CM_RESULT_SUCCESS as i64);
                let rc = ISVCDecoder::Initialize(d, std::ptr::null());
                assert_eq!(i64::from(rc), CM_INIT_PARA_ERROR as i64);
                WelsDestroyDecoder(d);
            }
            // `welsEncoderExt.cpp:192` — the same shape on the encoder.
            "encoder_default" => {
                let mut e: *mut ISVCEncoder = std::ptr::null_mut();
                assert_eq!(WelsCreateSVCEncoder(&mut e), CM_RESULT_SUCCESS);
                let rc = ISVCEncoder::Initialize(e, std::ptr::null());
                assert_eq!(rc, CM_INIT_PARA_ERROR);
                WelsDestroySVCEncoder(e);
            }
            // What a consumer that wants silence does, and what this tree's
            // high-volume harnesses do: install a callback that drops the line.
            "decoder_quiet" => {
                let mut d: *mut ISVCDecoder = std::ptr::null_mut();
                WelsCreateDecoder(&mut d);
                let mut cb: WelsTraceCallback = Some(quiet_sink);
                ISVCDecoder::SetOption(
                    d,
                    DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK,
                    &mut cb as *mut WelsTraceCallback as *mut std::ffi::c_void,
                );
                ISVCDecoder::Initialize(d, std::ptr::null());
                WelsDestroyDecoder(d);
            }
            "encoder_quiet" => {
                let mut e: *mut ISVCEncoder = std::ptr::null_mut();
                WelsCreateSVCEncoder(&mut e);
                let mut cb: WelsTraceCallback = Some(quiet_sink);
                ISVCEncoder::SetOption(
                    e,
                    ENCODER_OPTION::ENCODER_OPTION_TRACE_CALLBACK,
                    &mut cb as *mut WelsTraceCallback as *mut std::ffi::c_void,
                );
                ISVCEncoder::Initialize(e, std::ptr::null());
                WelsDestroySVCEncoder(e);
            }
            // The decoder's default level is `WELS_LOG_ERROR`, so a WARNING-level
            // message must not appear. `BA_MW_D_IDR_LOST.264` drives
            // `UpdateAccessUnit()`'s "Key frame lost" warning and
            // `WelsInitRefList`'s "referencing pictures lost due frame gaps exist";
            // the reference prints **nothing** for this stream at its default
            // (measured: `rust/tools/ecref/ecref res/BA_MW_D_IDR_LOST.264 999999`
            // writes zero bytes to stderr). Then a null `Initialize`, so the same
            // run proves the sink is live rather than merely silent.
            "decoder_level" => {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../..").join("res").join("BA_MW_D_IDR_LOST.264");
                let data = std::fs::read(path).expect("asset");
                let mut d: *mut ISVCDecoder = std::ptr::null_mut();
                WelsCreateDecoder(&mut d);
                let mut param = SDecodingParam::default();
                param.uiTargetDqLayer = u8::MAX;
                param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
                param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
                ISVCDecoder::Initialize(d, &param as *const SDecodingParam);
                for unit in openh264_rs::split_annexb_units(&data) {
                    let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                    let mut info = SBufferInfo::default();
                    ISVCDecoder::DecodeFrame2(d, unit.as_ptr(), unit.len() as i32, dst.as_mut_ptr(), &mut info);
                }
                ISVCDecoder::Uninitialize(d);
                WelsDestroyDecoder(d);

                let mut d2: *mut ISVCDecoder = std::ptr::null_mut();
                WelsCreateDecoder(&mut d2);
                ISVCDecoder::Initialize(d2, std::ptr::null());
                WelsDestroyDecoder(d2);
            }
            other => panic!("unknown case {other}"),
        }
    }
}

/// A sink that writes nowhere — a C consumer's "be quiet, please".
///
/// # Safety
/// Matches `WelsTraceCallback`; reads nothing.
unsafe extern "C" fn quiet_sink(_ctx: *mut std::ffi::c_void, _level: i32, _s: *const std::ffi::c_char) {}

#[test]
fn a_fresh_decoder_writes_its_error_to_stderr_with_no_callback_installed() {
    let err = stderr_of("decoder_default");
    assert!(
        err.contains("[OpenH264]") && err.contains("Error:") && err.contains("invalid input argument"),
        "the default sink must be upstream's stderr writer; got:\n{err}"
    );
}

#[test]
fn a_fresh_encoder_writes_its_error_to_stderr_with_no_callback_installed() {
    let err = stderr_of("encoder_default");
    assert!(
        err.contains("[OpenH264]") && err.contains("Error:") && err.contains("invalid argv"),
        "the default sink must be upstream's stderr writer; got:\n{err}"
    );
}

#[test]
fn an_installed_callback_replaces_the_default_sink_on_both_codecs() {
    for case in ["decoder_quiet", "encoder_quiet"] {
        let err = stderr_of(case);
        assert!(
            !err.contains("[OpenH264]"),
            "{case}: a caller who installs a sink must not also get the default one; got:\n{err}"
        );
    }
}

/// The decoder's default level is `WELS_LOG_ERROR`, not the trace object's
/// `WELS_LOG_WARNING` — `welsDecoderExt.cpp:164`.
///
/// It cannot be read back: neither codec's `GetOption` handles `*_TRACE_LEVEL`
/// upstream (only `SetOption` does, `welsDecoderExt.cpp:541` /
/// `welsEncoderExt.cpp:1090`), so the level is asserted where it is observable — in
/// what does and does not reach stderr.
#[test]
fn the_decoder_defaults_to_error_level_so_its_warnings_are_silent() {
    let err = stderr_of("decoder_level");
    assert!(
        !err.contains("Warning:"),
        "the decoder's default is WELS_LOG_ERROR and the reference prints nothing for \
         this stream; got:\n{err}"
    );
    assert!(
        err.contains("Error:") && err.contains("invalid input argument"),
        "...and the sink is live, not merely silent; got:\n{err}"
    );
}
