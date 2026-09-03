//! `ENCODER_OPTION_TRACE_CALLBACK` / `_CONTEXT` and `DECODER_OPTION_TRACE_CALLBACK`
//! / `_CONTEXT` are documented options on a documented interface: a caller installs
//! a function and every message the codec logs is handed to it.
//!
//! These tests drive the option pair from outside and count what arrives.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;
use std::ffi::{CStr, c_char, c_void};

/// What a callback run collects. The address of one of these is what the caller
/// installs as the trace *context*, which is the pointer the callback is handed
/// back.
#[derive(Default)]
struct Collected {
    lines: Vec<(i32, String)>,
}

unsafe extern "C" fn collect(ctx: *mut c_void, level: i32, string: *const c_char) {
    unsafe {
        assert!(!ctx.is_null(), "the trace context did not come back");
        assert!(!string.is_null(), "the trace message was null");
        let text = CStr::from_ptr(string).to_string_lossy().into_owned();
        (*ctx.cast::<Collected>()).lines.push((level, text));
    }
}

/// `WELS_LOG_INFO` / `WELS_LOG_ERROR` — `codec_app_def.h:323-331`, where the
/// levels are a **bit mask**: `WELS_LOG_ERROR = 1 << 0`, `WELS_LOG_INFO = 1 << 2`.
///
/// **Spelled out rather than imported, on purpose.** These are the values a C
/// caller gets from the real header, and this test's job is to assert the port
/// delivers *those* — importing `common::wels_trace`'s constants would make the
/// assertion agree with the port by construction and check nothing.
const WELS_LOG_INFO: i32 = 4;
const WELS_LOG_ERROR: i32 = 1;

// ---------------------------------------------------------------------------
// The encoder
// ---------------------------------------------------------------------------

#[test]
fn test_encoder_trace_callback_receives_the_init_line() {
    unsafe {
        let mut sink = Collected::default();
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);

        let mut cb: WelsTraceCallback = Some(collect);
        assert_eq!(
            ISVCEncoder::SetOption(
                enc,
                ENCODER_OPTION::ENCODER_OPTION_TRACE_CALLBACK,
                std::ptr::from_mut(&mut cb).cast::<c_void>(),
            ),
            CM_RESULT_SUCCESS
        );
        let mut ctx = std::ptr::from_mut(&mut sink).cast::<c_void>();
        assert_eq!(
            ISVCEncoder::SetOption(
                enc,
                ENCODER_OPTION::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT,
                std::ptr::from_mut(&mut ctx).cast::<c_void>(),
            ),
            CM_RESULT_SUCCESS
        );
        // The default level is `WELS_LOG_DEFAULT` = `WELS_LOG_WARNING`, so the init
        // line — `WELS_LOG_INFO` in the reference — needs the level raised. Setting
        // it is half the covering: a filter that never passes and a filter that
        // never blocks look the same from outside.
        let mut level = WELS_LOG_INFO as u32;
        assert_eq!(
            ISVCEncoder::SetOption(
                enc,
                ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
                std::ptr::from_mut(&mut level).cast::<c_void>(),
            ),
            CM_RESULT_SUCCESS
        );

        let mut param = SEncParamBase::default();
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.iPicWidth = 64;
        param.iPicHeight = 64;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 64000;
        assert_eq!(ISVCEncoder::Initialize(enc, &param), CM_RESULT_SUCCESS);

        assert!(
            !sink.lines.is_empty(),
            "the encoder's trace callback never fired: {:?}",
            sink.lines
        );
        // `welsEncoderExt.cpp:188` — the line, its level, and the tag `WelsLog`
        // builds around it (`utils.cpp:51`).
        let init = sink
            .lines
            .iter()
            .find(|(_, t)| t.contains("openh264 codec version"))
            .unwrap_or_else(|| panic!("no init line among {:?}", sink.lines));
        assert_eq!(init.0, WELS_LOG_INFO, "the init line's level is not INFO");
        assert!(
            init.1.starts_with("[OpenH264] this = 0x"),
            "the message carries no OpenH264 tag: {:?}",
            init.1
        );
        assert!(
            init.1.contains("Info:"),
            "the tag does not name the level: {:?}",
            init.1
        );

        assert_eq!(ISVCEncoder::Uninitialize(enc), CM_RESULT_SUCCESS);
        WelsDestroySVCEncoder(enc);
    }
}

/// The filter, from the other side: at the default level an `Info` line is dropped
/// and an `Error` line is not. `Initialize(NULL)` is the reference's
/// `welsEncoderExt.cpp:192` error arm.
#[test]
fn test_encoder_trace_level_filters() {
    unsafe {
        let mut sink = Collected::default();
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);

        let mut cb: WelsTraceCallback = Some(collect);
        ISVCEncoder::SetOption(
            enc,
            ENCODER_OPTION::ENCODER_OPTION_TRACE_CALLBACK,
            std::ptr::from_mut(&mut cb).cast::<c_void>(),
        );
        let mut ctx = std::ptr::from_mut(&mut sink).cast::<c_void>();
        ISVCEncoder::SetOption(
            enc,
            ENCODER_OPTION::ENCODER_OPTION_TRACE_CALLBACK_CONTEXT,
            std::ptr::from_mut(&mut ctx).cast::<c_void>(),
        );

        // Default level: WELS_LOG_WARNING.
        assert_eq!(
            ISVCEncoder::Initialize(enc, std::ptr::null()),
            CM_INIT_PARA_ERROR
        );
        assert!(
            sink.lines.iter().all(|(l, _)| *l <= 2),
            "an Info line passed the default WARNING filter: {:?}",
            sink.lines
        );
        assert!(
            sink.lines
                .iter()
                .any(|(l, t)| *l == WELS_LOG_ERROR && t.contains("invalid argv")),
            "the invalid-argv error never arrived: {:?}",
            sink.lines
        );

        WelsDestroySVCEncoder(enc);
    }
}

// ---------------------------------------------------------------------------
// The decoder
// ---------------------------------------------------------------------------

/// `bPrintFrameErrorTraceFlag` lets one `decode failed, failure type:` line
/// through per error burst and counts the rest, so a stream that fails on many
/// access units must produce strictly fewer lines than it produces errors.
#[test]
fn test_decoder_trace_callback_and_the_error_throttle() {
    let mut repo_root = std::path::PathBuf::from("../../../");
    if !repo_root.join("res").exists() {
        repo_root = std::path::PathBuf::from("../../");
    }
    let data = std::fs::read(repo_root.join("res/BA_MW_D_IDR_LOST.264")).expect("asset");

    unsafe {
        let mut sink = Collected::default();
        let mut dec: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(
            i64::from(WelsCreateDecoder(&mut dec)),
            CM_RESULT_SUCCESS as i64
        );

        let mut cb: WelsTraceCallback = Some(collect);
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK,
                std::ptr::from_mut(&mut cb).cast::<c_void>(),
            )),
            CM_RESULT_SUCCESS as i64
        );
        let mut ctx = std::ptr::from_mut(&mut sink).cast::<c_void>();
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT,
                std::ptr::from_mut(&mut ctx).cast::<c_void>(),
            )),
            CM_RESULT_SUCCESS as i64
        );
        let mut level = WELS_LOG_INFO as u32;
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
                std::ptr::from_mut(&mut level).cast::<c_void>(),
            )),
            CM_RESULT_SUCCESS as i64
        );

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(dec, &param)),
            CM_RESULT_SUCCESS as i64
        );

        let mut erroring = 0usize;
        for unit in split_annexb_units(&data) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ISVCDecoder::DecodeFrame2(
                dec,
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            if ret.0 != 0 {
                erroring += 1;
            }
        }
        assert!(erroring > 4, "the asset barely errored: {erroring} calls");

        assert!(
            !sink.lines.is_empty(),
            "the decoder's trace callback never fired over {erroring} erroring calls"
        );
        let failures = sink
            .lines
            .iter()
            .filter(|(_, t)| t.contains("decode failed, failure type:"))
            .count();
        assert!(
            failures >= 1,
            "the DecodeFrame2 error line never arrived: {:?}",
            sink.lines
        );
        // The throttle: `bPrintFrameErrorTraceFlag` is cleared on the first line of a
        // burst and re-armed only by a complete frame, so the lines must be strictly
        // fewer than the erroring calls.
        assert!(
            failures < erroring,
            "the error line was not throttled: {failures} lines over {erroring} \
             erroring calls"
        );

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
    }
}
