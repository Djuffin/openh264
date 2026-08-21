//! Integration test for decoder error concealment modes.
//! Ported from `test/api/decoder_ec_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_decoder_error_concealment_modes() {
    let ec_modes = [
        ERROR_CON_IDC::ERROR_CON_DISABLE,
        ERROR_CON_IDC::ERROR_CON_FRAME_COPY,
        ERROR_CON_IDC::ERROR_CON_SLICE_COPY,
        ERROR_CON_IDC::ERROR_CON_FRAME_COPY_CROSS_IDR,
        ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR,
        ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
        ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR,
        ERROR_CON_IDC::ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE,
    ];

    for &ec_mode in &ec_modes {
        unsafe {
            let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
            let ret = WelsCreateDecoder(&mut p_decoder);
            assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut param = SDecodingParam::default();
            param.uiTargetDqLayer = u8::MAX;
            param.eEcActiveIdc = ec_mode;

            let init_ret = ISVCDecoder::Initialize(p_decoder, &param as *const SDecodingParam);
            assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

            let mut current_ec: i32 = 0;
            let get_opt_ret = ISVCDecoder::GetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC,
                &mut current_ec as *mut i32 as *mut std::ffi::c_void,
            );
            assert_eq!(i64::from(get_opt_ret), CM_RESULT_SUCCESS as i64);

            let uninit_ret = ISVCDecoder::Uninitialize(p_decoder);
            assert_eq!(i64::from(uninit_ret), CM_RESULT_SUCCESS as i64);

            WelsDestroyDecoder(p_decoder);
        }
    }
}

// ============================================================================
// F41's covering test
// ============================================================================

/// Decodes `data` and returns the OR of every `DecodeFrame2` state, optionally
/// switching the concealment mode through `SetOption` after `Initialize`.
///
/// The switch is the whole point: it is the one write the public API makes into
/// the parameter block *after* the decoder is configured.
unsafe fn decode_states(data: &[u8], init_ec: ERROR_CON_IDC, switch_to: Option<ERROR_CON_IDC>) -> i32 {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut p_decoder)), CM_RESULT_SUCCESS as i64);

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = init_ec;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(p_decoder, &param)),
            CM_RESULT_SUCCESS as i64
        );

        if let Some(mode) = switch_to {
            let mut val = mode;
            ISVCDecoder::SetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC,
                &mut val as *mut ERROR_CON_IDC as *mut std::ffi::c_void,
            );
        }

        let mut states = 0i32;
        for unit in openh264_rs::split_annexb_units(data) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ISVCDecoder::DecodeFrame2(
                p_decoder,
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            states |= ret.0;
        }
        ISVCDecoder::Uninitialize(p_decoder);
        WelsDestroyDecoder(p_decoder);
        states
    }
}

/// **F41 — the block the api writes and the block the decoder reads are one
/// block, and it is the context's.**
///
/// The C++ context owns its `SDecodingParam`: `InitDecoderCtx` allocates it
/// (`welsDecoderExt.cpp:426`), `DecoderConfigParam` copies the caller's values in,
/// and `SetOption(DECODER_OPTION_ERROR_CON_IDC)` writes
/// `pDecContext->pParam->eEcActiveIdc` (`:535`). The port had invented a
/// `CWelsDecoderImpl::param` with no counterpart in the reference and pointed
/// `pCtx->pParam` at it — an alias into an object with its own lifetime, rewritten
/// on every `Initialize` before the existing-context test, and read by the
/// teardown's `bParseOnly` arm. T8.A5 gives the block to the context.
///
/// **What this test pins is the property the move could break**: after the move
/// there are two candidate blocks a careless `SetOption` could write — the api's
/// (now deleted) and the context's — and only one of them is read by the decoder.
/// So it *switches concealment off after `Initialize`* on a stream that conceals,
/// and asserts the decoder noticed:
///
/// * initialised with `ERROR_CON_SLICE_COPY`, `BA_MW_D_IDR_LOST.264` comes back
///   with `dsDataErrorConcealed` set — concealment ran;
/// * initialised the same way and then switched to `ERROR_CON_DISABLE`, it does
///   not, and reports `dsBitstreamError` instead.
///
/// A `SetOption` writing anything but the block the decoder reads leaves the first
/// state on the second run, and the assertion fires — measured at T8.A5 by making
/// the option write a scratch copy of the context's block:
///
/// ```text
/// assertion `left == right` failed: concealment still ran after
/// SetOption(ERROR_CON_IDC = DISABLE): the api wrote one parameter block and the
/// decoder read another — F41
/// ```
#[test]
fn test_error_con_idc_set_after_initialize_reaches_the_decoder() {
    let mut repo_root = std::path::PathBuf::from("../../../");
    if !repo_root.join("res").exists() {
        repo_root = std::path::PathBuf::from("../../");
    }
    let path = repo_root.join("res/BA_MW_D_IDR_LOST.264");
    assert!(path.exists(), "asset missing: {:?}", path);
    let data = std::fs::read(&path).expect("read asset");

    const CONCEALED: i32 = 0x20; // dsDataErrorConcealed

    unsafe {
        let concealing = decode_states(&data, ERROR_CON_IDC::ERROR_CON_SLICE_COPY, None);
        assert_ne!(
            concealing & CONCEALED,
            0,
            "the asset did not conceal at all — this test no longer covers F41"
        );

        let switched = decode_states(
            &data,
            ERROR_CON_IDC::ERROR_CON_SLICE_COPY,
            Some(ERROR_CON_IDC::ERROR_CON_DISABLE),
        );
        assert_eq!(
            switched & CONCEALED,
            0,
            "concealment still ran after SetOption(ERROR_CON_IDC = DISABLE): the api wrote one \
             parameter block and the decoder read another — F41"
        );
    }
}
