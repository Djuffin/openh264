//! Integration tests for OpenH264 API lifecycle and memory management.
//! C++: `test/api/c_interface_test.c`, `test/api/cpp_interface_test.cpp`.

use openh264_rs::api::codec_api::*;

#[repr(C)]
struct BoolTestStruct {
    c: std::ffi::c_char,
    b: bool,
}

#[test]
fn test_c_abi_bool_and_struct_alignment() {
    assert_eq!(std::mem::size_of::<bool>(), 1);
    assert_eq!(std::mem::offset_of!(BoolTestStruct, b), 1);
    assert_eq!(std::mem::size_of::<BoolTestStruct>(), 2);
}

#[test]
fn test_decoder_create_and_destroy_lifecycle() {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(ret, CM_RESULT_SUCCESS as i64);
        assert!(!p_decoder.is_null());

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;

        let init_ret = ISVCDecoder::Initialize(p_decoder, &param as *const SDecodingParam);
        assert_eq!(init_ret, CM_RESULT_SUCCESS as i64);

        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
        let mut stride: [i32; 2] = [0; 2];
        let mut width: i32 = 0;
        let mut height: i32 = 0;
        let dec_state = ISVCDecoder::DecodeFrame(
            p_decoder,
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            stride.as_mut_ptr(),
            &mut width,
            &mut height,
        );
        assert_eq!(dec_state, DECODING_STATE::dsErrorFree);

        let mut buf_info = SBufferInfo::default();
        let dec_nodelay_state = ISVCDecoder::DecodeFrameNoDelay(
            p_decoder,
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        assert_eq!(dec_nodelay_state, DECODING_STATE::dsErrorFree);

        let dec2_state = ISVCDecoder::DecodeFrame2(
            p_decoder,
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        assert_eq!(dec2_state, DECODING_STATE::dsErrorFree);

        let flush_state = ISVCDecoder::FlushFrame(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
        assert_eq!(flush_state, DECODING_STATE::dsErrorFree);

        // This decoder was initialised without `bParseOnly`, so `DecodeParser` ors
        // `dsInvalidArgument` into the context's error code and returns it
        // (`welsDecoderExt.cpp:1189-1193`).
        let mut parser_info = SParserBsInfo::default();
        let parse_state =
            ISVCDecoder::DecodeParser(p_decoder, std::ptr::null(), 0, &mut parser_info);
        assert_eq!(parse_state, DECODING_STATE::dsInvalidArgument);

        let mut dst_len: i32 = 0;
        let mut color_fmt: i32 = 0;
        let dec_ex_state = ISVCDecoder::DecodeFrameEx(
            p_decoder,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut dst_len,
            &mut width,
            &mut height,
            &mut color_fmt,
        );
        assert_eq!(dec_ex_state, DECODING_STATE::dsErrorFree);

        let mut trace_level = 0i32;
        let set_opt_ret = ISVCDecoder::SetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(set_opt_ret, CM_RESULT_SUCCESS as i64);

        // `DECODER_OPTION_TRACE_LEVEL` is settable but not gettable: `GetOption` has no
        // `TRACE_LEVEL` arm and returns `cmInitParaError`.
        let get_opt_ret = ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(get_opt_ret, CM_INIT_PARA_ERROR as i64);

        let uninit_ret = ISVCDecoder::Uninitialize(p_decoder);
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS as i64);

        WelsDestroyDecoder(p_decoder);
    }
}

#[test]
fn test_encoder_create_and_destroy_lifecycle() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let mut param = SEncParamBase::default();
        param.iPicWidth = 320;
        param.iPicHeight = 240;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 500000;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;

        let init_ret = ISVCEncoder::Initialize(p_encoder, &param as *const SEncParamBase);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        let mut param_ext = SEncParamExt::default();
        param_ext.iPicWidth = 320;
        param_ext.iPicHeight = 240;
        param_ext.fMaxFrameRate = 30.0;
        param_ext.iTargetBitrate = 500000;
        param_ext.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        let init_ext_ret = ISVCEncoder::InitializeExt(p_encoder, &param_ext as *const SEncParamExt);
        assert_eq!(init_ext_ret, CM_RESULT_SUCCESS);

        let mut default_param = SEncParamExt::default();
        let get_def_ret =
            ISVCEncoder::GetDefaultParams(p_encoder, &mut default_param as *mut SEncParamExt);
        assert_eq!(get_def_ret, CM_RESULT_SUCCESS);

        let mut src_pic = SSourcePicture::default();
        src_pic.iPicWidth = 160;
        src_pic.iPicHeight = 120;
        src_pic.iColorFormat = 23;
        let mut bs_info = SFrameBSInfo::default();
        // A 160x120 source picture against a 320x240 encoder, with `pData` null, is
        // rejected.
        let enc_frame_ret = ISVCEncoder::EncodeFrame(p_encoder, &src_pic, &mut bs_info);
        assert_eq!(enc_frame_ret, CM_UNSUPPORTED_DATA);

        let enc_ps_ret = ISVCEncoder::EncodeParameterSets(p_encoder, &mut bs_info);
        assert_eq!(enc_ps_ret, CM_RESULT_SUCCESS);

        let force_idr_ret = ISVCEncoder::ForceIntraFrame(p_encoder, true);
        assert_eq!(force_idr_ret, CM_RESULT_SUCCESS);

        let mut trace_level = 0i32;
        let set_opt_ret = ISVCEncoder::SetOption(
            p_encoder,
            ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(set_opt_ret, CM_RESULT_SUCCESS);

        // ENCODER_OPTION_TRACE_LEVEL is set-only: `CWelsH264SVCEncoder::GetOption`
        // has no case for it and falls to `default: return cmInitParaError`.
        let get_opt_ret = ISVCEncoder::GetOption(
            p_encoder,
            ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(get_opt_ret, CM_INIT_PARA_ERROR);

        // A gettable option, for contrast.
        let mut idr_interval = 0i32;
        let get_opt_ret = ISVCEncoder::GetOption(
            p_encoder,
            ENCODER_OPTION::ENCODER_OPTION_IDR_INTERVAL,
            &mut idr_interval as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(get_opt_ret, CM_RESULT_SUCCESS);

        let uninit_ret = ISVCEncoder::Uninitialize(p_encoder);
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}

// ============================================================================
// Re-init probe
// ============================================================================

/// One decode pass over `units`, stopping after `limit` access units.
///
/// Returns `(frames emitted, frames the decoder says are still buffered)`. The
/// second number is `sReoderingStatus.iNumOfPicts`, read back through
/// `DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER`.
///
/// `drain` runs the end-of-stream sequence — signal `DECODER_OPTION_END_OF_STREAM`,
/// one null `DecodeFrame2`, then `FlushFrame` per buffered picture — so the frame
/// count covers the whole stream: a stream that reorders holds up to its DPB size of
/// pictures back (Annex C.4.5.3).
unsafe fn decode_pass(
    p_decoder: *mut ISVCDecoder,
    units: &[&[u8]],
    limit: usize,
    drain: bool,
) -> (usize, i32) {
    unsafe {
        let mut frames = 0usize;
        for unit in units.iter().take(limit) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            ISVCDecoder::DecodeFrame2(
                p_decoder,
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            if buf_info.iBufferStatus == 1 {
                frames += 1;
            }
        }
        if drain {
            let mut eos = 1i32;
            ISVCDecoder::SetOption(
                p_decoder,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                &mut eos as *mut i32 as *mut std::ffi::c_void,
            );
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            ISVCDecoder::DecodeFrame2(
                p_decoder,
                std::ptr::null(),
                0,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            if buf_info.iBufferStatus == 1 {
                frames += 1;
            }
            loop {
                let mut left = 0i32;
                ISVCDecoder::GetOption(
                    p_decoder,
                    DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                    &mut left as *mut i32 as *mut std::ffi::c_void,
                );
                if left <= 0 {
                    break;
                }
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                ISVCDecoder::FlushFrame(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
                if buf_info.iBufferStatus == 1 {
                    frames += 1;
                } else {
                    break;
                }
            }
        }
        let mut remaining = 0i32;
        ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        (frames, remaining)
    }
}

/// A re-initialised decoder must not inherit the previous session's reordering
/// slots.
///
/// The reordering pair (`sPictInfoList`, `sReoderingStatus`) is api-owned state that
/// outlives the decoder context: `Uninitialize` frees the context and its picture
/// pool, and a second `Initialize` builds new ones, so the slots are cleared at the
/// head of `DestroyPicBuff` (`decoder.cpp:260`). A surviving non-sentinel `iPOC` makes
/// a stale slot look occupied, and its `iPicBuffIdx` then indexes the new pool with
/// the old pool's index.
///
/// The stream is interrupted while pictures are still buffered, then re-initialised
/// and decoded whole: `remaining` is 0 after the second `Initialize`, and the second
/// pass emits what a decoder that never saw the first emits.
#[test]
fn test_decoder_reinit_does_not_inherit_reordering_slots() {
    let mut repo_root = std::path::PathBuf::from("../../../");
    if !repo_root.join("res").exists() {
        repo_root = std::path::PathBuf::from("../../");
    }
    let path = repo_root.join("res/CABA2_SVA_B.264");
    assert!(path.exists(), "asset missing: {:?}", path);
    let data = std::fs::read(&path).expect("read asset");
    let units = openh264_rs::split_annexb_units(&data);
    assert!(units.len() > 12, "asset too short to interrupt mid-stream");

    unsafe {
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

        // --- the reference: a decoder that has only ever seen one session ------
        let mut p_fresh: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(WelsCreateDecoder(&mut p_fresh), CM_RESULT_SUCCESS as i64);
        assert_eq!(
            ISVCDecoder::Initialize(p_fresh, &param),
            CM_RESULT_SUCCESS as i64
        );
        let (fresh_frames, _) = decode_pass(p_fresh, &units, units.len(), true);
        ISVCDecoder::Uninitialize(p_fresh);
        WelsDestroyDecoder(p_fresh);
        assert!(fresh_frames > 0, "the asset decoded nothing at all");

        // --- the subject: interrupted mid-stream, then re-initialised ---------
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS as i64);
        assert_eq!(
            ISVCDecoder::Initialize(p_decoder, &param),
            CM_RESULT_SUCCESS as i64
        );
        // Stop while the reordering buffer still holds pictures; a B-slice stream
        // buffers by construction.
        let (_, buffered) = decode_pass(p_decoder, &units, 12, false);
        assert!(
            buffered > 0,
            "nothing was buffered at the interruption — this probe no longer reaches the state it checks"
        );

        assert_eq!(
            ISVCDecoder::Uninitialize(p_decoder),
            CM_RESULT_SUCCESS as i64
        );
        assert_eq!(
            ISVCDecoder::Initialize(p_decoder, &param),
            CM_RESULT_SUCCESS as i64
        );

        // The new session starts empty.
        let mut remaining = 0i32;
        ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(
            remaining, 0,
            "the re-initialised decoder inherited {remaining} buffered picture(s) from the \
             previous session — DestroyPicBuff did not reset the reordering buffers"
        );

        // …and decodes the stream as a decoder that never saw the first pass.
        let (reinit_frames, _) = decode_pass(p_decoder, &units, units.len(), true);
        assert_eq!(
            reinit_frames, fresh_frames,
            "a re-initialised decoder emitted {reinit_frames} frames where a fresh one emits \
             {fresh_frames}"
        );

        ISVCDecoder::Uninitialize(p_decoder);
        WelsDestroyDecoder(p_decoder);
    }
}

/// The safe cores are part of the crate's public surface, not an internal detail
/// of the shells.
#[test]
fn test_safe_core_types_are_exported() {
    let _d: openh264_rs::api::Decoder = openh264_rs::api::Decoder::new();
    let _e: openh264_rs::api::Encoder = openh264_rs::api::Encoder::new();
    // And through the flat re-export the rest of this file uses.
    let _d2: Decoder = Decoder::default();
    let _e2: Encoder = Encoder::default();
}
