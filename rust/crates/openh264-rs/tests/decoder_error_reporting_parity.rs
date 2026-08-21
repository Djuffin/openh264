//! **F76** — `CWelsDecoder::DecodeFrame2`'s error-reporting block, the three
//! `DecoderConfigParam` statements it depends on, and the live re-initialisation
//! rebuild, as covering tests.
//!
//! Every arm below is a *status code, a recovery action, or a statistic*. That is
//! precisely the class the project's byte referees cannot see — conformance and the
//! 2707-row malformed corpus are silent about all of it, which is why the whole
//! block survived the port unnoticed until `eVideoType`'s duplicate declaration sent
//! someone to read the field (`phase8_findings.md`, F76). So these tests are the
//! only instrument the arms have, and each one is measured **red** against the tree
//! that precedes its fix; the message a red run prints is quoted at the test.
//!
//! The reference is `codec/decoder/plus/src/welsDecoderExt.cpp:813–905` (the block)
//! and `codec/decoder/core/src/decoder.cpp:649–676` (`DecoderConfigParam`).

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;
use std::ffi::c_void;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn repo_root() -> std::path::PathBuf {
    let mut root = std::path::PathBuf::from("../../../");
    if !root.join("res").exists() {
        root = std::path::PathBuf::from("../../");
    }
    root
}

fn asset(name: &str) -> Vec<u8> {
    let path = repo_root().join("res").join(name);
    assert!(path.exists(), "asset missing: {path:?}");
    std::fs::read(&path).expect("read asset")
}

/// A decoder created and initialised with `param`, as a C caller does it.
///
/// The parameter block is handed over as a **raw pointer into a byte image**, never
/// as `&SDecodingParam`: several of these tests put values in `eEcActiveIdc` that
/// the Rust enum has no variant for — which is the entire point of the clamp at
/// `decoder.cpp:654` — and a reference to such a block is undefined before the
/// callee can do anything about it.
struct Dec(*mut ISVCDecoder);

impl Dec {
    unsafe fn new(param: &SDecodingParam, ec_override: Option<i32>) -> Self {
        unsafe {
            let mut p: *mut ISVCDecoder = std::ptr::null_mut();
            assert_eq!(
                i64::from(WelsCreateDecoder(&mut p)),
                CM_RESULT_SUCCESS as i64
            );
            assert!(!p.is_null());

            let mut buf = std::mem::MaybeUninit::<SDecodingParam>::uninit();
            std::ptr::copy_nonoverlapping(
                std::ptr::from_ref(param).cast::<u8>(),
                buf.as_mut_ptr().cast::<u8>(),
                std::mem::size_of::<SDecodingParam>(),
            );
            if let Some(raw) = ec_override {
                std::ptr::addr_of_mut!((*buf.as_mut_ptr()).eEcActiveIdc)
                    .cast::<i32>()
                    .write(raw);
            }
            assert_eq!(
                i64::from(ISVCDecoder::Initialize(p, buf.as_ptr())),
                CM_RESULT_SUCCESS as i64
            );
            Self(p)
        }
    }

    unsafe fn get_ec_idc(&self) -> i32 {
        unsafe {
            let mut v = -1i32;
            assert_eq!(
                i64::from(ISVCDecoder::GetOption(
                    self.0,
                    DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC,
                    std::ptr::from_mut(&mut v).cast::<c_void>(),
                )),
                CM_RESULT_SUCCESS as i64
            );
            v
        }
    }

    unsafe fn set_ec_idc(&self, v: i32) -> i64 {
        unsafe {
            let mut v = v;
            i64::from(ISVCDecoder::SetOption(
                self.0,
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC,
                std::ptr::from_mut(&mut v).cast::<c_void>(),
            ))
        }
    }
}

impl Drop for Dec {
    fn drop(&mut self) {
        unsafe {
            ISVCDecoder::Uninitialize(self.0);
            WelsDestroyDecoder(self.0);
        }
    }
}

/// Feeds every Annex-B unit of `data` through `DecodeFrame2`, returning the OR of
/// every state and the number of frames emitted.
unsafe fn decode_all(dec: &Dec, data: &[u8]) -> (i32, u32) {
    unsafe {
        let mut states = 0i32;
        let mut frames = 0u32;
        for unit in split_annexb_units(data) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ISVCDecoder::DecodeFrame2(
                dec.0,
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            states |= ret.0;
            if buf_info.iBufferStatus == 1 {
                frames += 1;
            }
        }
        (states, frames)
    }
}

// ---------------------------------------------------------------------------
// T8.B1 — the three `DecoderConfigParam` statements
// ---------------------------------------------------------------------------

/// **`decoder.cpp:654–661`, the range clamp.**
///
/// A C caller's `eEcActiveIdc` is an `int`. The reference clamps it into
/// `[ERROR_CON_DISABLE, ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE]`,
/// warns, and uses the top value; the port had neither the clamp nor a way to
/// survive the read — `ctx_box.pParam = *pParam` produced an `ERROR_CON_IDC` with
/// no such variant, which is undefined before any policy question arises.
///
/// Red before T8.B1 with `DECODER_OPTION_ERROR_CON_IDC` unwired at `GetOption`
/// (the arm fell through to the catch-all and left the caller's `-1` in place):
///
/// ```text
/// assertion `left == right` failed: eEcActiveIdc = 99 was not clamped to 7
///   left: -1
///  right: 7
/// ```
#[test]
fn test_error_con_idc_out_of_range_is_clamped_at_initialize() {
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
    unsafe {
        for (raw, want) in [(99i32, 7i32), (-3, 0), (7, 7), (0, 0), (2, 2)] {
            let dec = Dec::new(&param, Some(raw));
            assert_eq!(
                dec.get_ec_idc(),
                want,
                "eEcActiveIdc = {raw} was not clamped to {want}"
            );
        }
    }
}

/// **`welsDecoderExt.cpp:528`, the same clamp on the `SetOption` path.**
///
/// `WELS_CLIP3 (iVal, ERROR_CON_DISABLE, …FREEZE_RES_CHANGE)` before the store.
/// The port read the option blob as `*const ERROR_CON_IDC` — the same undefined
/// read as above, one level out — and stored whatever it found.
#[test]
fn test_error_con_idc_out_of_range_is_clamped_at_set_option() {
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
    unsafe {
        let dec = Dec::new(&param, None);
        assert_eq!(dec.get_ec_idc(), 2, "the initial mode did not survive");
        assert_eq!(dec.set_ec_idc(99), CM_RESULT_SUCCESS as i64);
        assert_eq!(dec.get_ec_idc(), 7, "SetOption(99) was not clamped to 7");
        assert_eq!(dec.set_ec_idc(-5), CM_RESULT_SUCCESS as i64);
        assert_eq!(dec.get_ec_idc(), 0, "SetOption(-5) was not clamped to 0");
    }
}

/// **`decoder.cpp:663–664`** — parse-only decoding disables concealment.
///
/// Inert on output today only because `DecodeParser` is a stub, and *not* inert as
/// a configuration: the mode the caller asked for stays in the context's parameter
/// block and selects `sCopyFunc`'s kernels.
///
/// Red before T8.B1:
///
/// ```text
/// assertion `left == right` failed: bParseOnly did not disable concealment
///   left: 2
///  right: 0
/// ```
#[test]
fn test_parse_only_disables_error_concealment() {
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
    param.bParseOnly = true;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
    unsafe {
        let dec = Dec::new(&param, None);
        assert_eq!(
            dec.get_ec_idc(),
            0,
            "bParseOnly did not disable concealment"
        );
        // `welsDecoderExt.cpp:529–533`: and it may not be switched back on.
        assert_eq!(dec.set_ec_idc(2), CM_INIT_PARA_ERROR as i64);
        assert_eq!(dec.get_ec_idc(), 0, "a rejected SetOption still stored");
    }
}
