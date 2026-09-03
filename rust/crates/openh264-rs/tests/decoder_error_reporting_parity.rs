//! `CWelsDecoder::DecodeFrame2`'s error-reporting block, the three
//! `DecoderConfigParam` statements it depends on, and the live re-initialisation
//! rebuild, as covering tests.
//!
//! Every arm below is a *status code, a recovery action, or a statistic*.
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
// The three `DecoderConfigParam` statements
// ---------------------------------------------------------------------------

/// **`decoder.cpp:654–661`, the range clamp.**
///
/// A C caller's `eEcActiveIdc` is an `int`. The reference clamps it into
/// `[ERROR_CON_DISABLE, ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE]`,
/// warns, and uses the top value.
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

// ---------------------------------------------------------------------------
// A second `Initialize` on a live decoder rebuilds the context
// ---------------------------------------------------------------------------

/// **`welsDecoderExt.cpp:407–409`.**
///
/// `CWelsDecoder::InitDecoder` calls `InitDecoderCtx` for every context, and
/// `InitDecoderCtx` opens with `UninitDecoderCtx (pCtx)` and then allocates a fresh
/// one. So in the reference a second `Initialize` on a live decoder is a rebuild.
///
/// The observable is the reordering buffer: a B-frame stream stopped mid-GOP leaves
/// pictures buffered, `DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER` counts
/// them, and after a rebuild there is nothing to count.
#[test]
fn test_second_initialize_rebuilds_the_context() {
    let data = asset("Cisco_Men_whisper_640x320_CABAC_Bframe_9.264");
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

    unsafe {
        let dec = Dec::new(&param, None);

        // Feed units until the reordering buffer is holding something.
        let mut buffered = 0i32;
        for unit in split_annexb_units(&data) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            ISVCDecoder::DecodeFrame2(
                dec.0,
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            buffered = remaining_in_buffer(&dec);
            if buffered > 0 {
                break;
            }
        }
        assert!(
            buffered > 0,
            "the asset never buffered a picture — this test no longer covers the rebuild"
        );

        // The transition the reference rebuilds through.
        let mut buf = std::mem::MaybeUninit::<SDecodingParam>::uninit();
        std::ptr::copy_nonoverlapping(
            std::ptr::from_ref(&param).cast::<u8>(),
            buf.as_mut_ptr().cast::<u8>(),
            std::mem::size_of::<SDecodingParam>(),
        );
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(dec.0, buf.as_ptr())),
            CM_RESULT_SUCCESS as i64
        );

        assert_eq!(
            remaining_in_buffer(&dec),
            0,
            "a second Initialize kept the previous session's reordering buffer"
        );
    }
}

unsafe fn remaining_in_buffer(dec: &Dec) -> i32 {
    unsafe {
        let mut v = 0i32;
        ISVCDecoder::GetOption(
            dec.0,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            std::ptr::from_mut(&mut v).cast::<c_void>(),
        );
        v
    }
}
/// **`decoder.cpp:667–671`, `eVideoType`, and `welsDecoderExt.cpp:833–842`, its
/// one reader.**
///
/// The field's reader is the key-frame-loss notification inside the `DecodeFrame2`
/// error block — *"for AVC bitstream, as long as error occur, SHOULD notify upper
/// layer key frame loss"* — which raises `bParamSetsLostFlag` when concealment is
/// off. `UpdateAccessUnit`'s mosaic-avoidance block then counts one `uiIDRLostNum`
/// for the next access unit that arrives without an IDR.
///
/// **The observable is that counter and not the frame count.**
/// `bParamSetsLostFlag` is already true whenever a frame failed to construct, so the
/// arm only adds something when an error arrives on a call that *did* construct a
/// frame — one truncated slice in an otherwise clean stream. Whole streams never
/// produce that coincidence; one truncated slice in `BA_MW_D.264` produces it on
/// every unit from the fourth on, and it moves `uiIDRLostNum` by exactly one while
/// the emitted frame count does not move at all. That is the notification the
/// reference documents: an accounting event for the upper layer, not a change of
/// output.
#[test]
fn test_avc_bitstream_type_notifies_key_frame_loss_when_ec_is_off() {
    let data = asset("BA_MW_D.264");
    // One slice NAL cut in half, deep enough into the stream that the frames around
    // it construct cleanly — which is what clears `bParamSetsLostFlag` and leaves
    // the arm something to do.
    let mut units: Vec<Vec<u8>> = split_annexb_units(&data)
        .iter()
        .map(|u| u.to_vec())
        .collect();
    assert!(units.len() > 8, "asset is too short to corrupt a settled slice");
    let half = units[3].len() / 2;
    assert!(half > 6, "unit 3 is too short to corrupt");
    units[3].truncate(half);

    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_DISABLE;

    unsafe {
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC;
        let avc = Dec::new(&param, None);
        let (avc_states, avc_frames) = decode_units(&avc, &units);
        let avc_stats = statistics(&avc);

        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC;
        let svc = Dec::new(&param, None);
        let (svc_states, svc_frames) = decode_units(&svc, &units);
        let svc_stats = statistics(&svc);

        assert_ne!(
            avc_states & DECODING_STATE::dsBitstreamError.0,
            0,
            "the corrupted unit did not produce an error — this test covers nothing"
        );
        assert_eq!(
            avc_stats.uiIDRLostNum,
            svc_stats.uiIDRLostNum + 1,
            "declaring the stream AVC changed nothing (avc states {avc_states:#x}, \
             svc states {svc_states:#x})"
        );
        assert_eq!(
            avc_frames, svc_frames,
            "the notification changed the emitted frame count, which it must not"
        );
    }
}

/// The same feed as [`decode_all`], over units the caller has already cut up.
unsafe fn decode_units(dec: &Dec, units: &[Vec<u8>]) -> (i32, u32) {
    unsafe {
        let mut states = 0i32;
        let mut frames = 0u32;
        for unit in units {
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

/// **`welsDecoderExt.cpp:856–882`, the four concealment statistics.**
///
/// `uiDecodedFrameCount`, `uiAvgEcRatio`, `uiAvgEcPropRatio` and `uiEcFrameNum`,
/// read back through the public `DECODER_OPTION_GET_STATISTICS` option.
#[test]
fn test_concealment_statistics_reach_get_statistics() {
    let data = asset("BA_MW_D_IDR_LOST.264");
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

    unsafe {
        let dec = Dec::new(&param, None);
        let before = statistics(&dec);
        assert_eq!(before.uiDecodedFrameCount, 0, "a fresh decoder had a frame count");
        assert_eq!(before.uiEcFrameNum, 0, "a fresh decoder had concealed frames");

        let (states, frames) = decode_all(&dec, &data);
        assert!(frames > 0, "the asset decoded nothing");
        assert_ne!(
            states & DECODING_STATE::dsDataErrorConcealed.0,
            0,
            "the asset did not conceal — this test no longer covers the statistics"
        );

        let after = statistics(&dec);
        assert!(
            after.uiDecodedFrameCount > 0,
            "uiDecodedFrameCount never moved: {after:?}"
        );
        assert_eq!(
            after.uiDecodedFrameCount, frames,
            "uiDecodedFrameCount disagrees with the frames the caller saw"
        );
        assert!(
            after.uiEcFrameNum > 0,
            "uiEcFrameNum never moved on a stream that concealed: {after:?}"
        );
        // `:645–649` — both speeds are derived from `dDecTime` at read time, so a
        // decoder that has decoded anything reports a positive one.
        assert!(
            after.fAverageFrameSpeedInMs > 0.0,
            "fAverageFrameSpeedInMs is not positive: {after:?}"
        );
    }
}

unsafe fn statistics(dec: &Dec) -> SDecoderStatistics {
    unsafe {
        let mut s = SDecoderStatistics::default();
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(
                dec.0,
                DECODER_OPTION::DECODER_OPTION_GET_STATISTICS,
                std::ptr::from_mut(&mut s).cast::<c_void>(),
            )),
            CM_RESULT_SUCCESS as i64
        );
        s
    }
}
