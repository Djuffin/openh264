//! Integration tests for OpenH264 API lifecycle and memory management.
//! Ported from `test/api/c_interface_test.c` and `test/api/cpp_interface_test.cpp`.

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
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64);
        assert!(!p_decoder.is_null());

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;

        // 1. Initialize
        let init_ret = (*p_decoder).Initialize(&param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        // 2. DecodeFrame
        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
        let mut stride: [i32; 2] = [0; 2];
        let mut width: i32 = 0;
        let mut height: i32 = 0;
        let dec_state = (*p_decoder).DecodeFrame(
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            stride.as_mut_ptr(),
            &mut width,
            &mut height,
        );
        assert_eq!(dec_state, DECODING_STATE::dsErrorFree);

        // 3. DecodeFrameNoDelay
        let mut buf_info = SBufferInfo::default();
        let dec_nodelay_state = (*p_decoder).DecodeFrameNoDelay(
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        assert_eq!(dec_nodelay_state, DECODING_STATE::dsErrorFree);

        // 4. DecodeFrame2
        let dec2_state = (*p_decoder).DecodeFrame2(
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        assert_eq!(dec2_state, DECODING_STATE::dsErrorFree);

        // 5. FlushFrame
        let flush_state = (*p_decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
        assert_eq!(flush_state, DECODING_STATE::dsErrorFree);

        // 6. DecodeParser
        let mut parser_info = SParserBsInfo::default();
        let parse_state = (*p_decoder).DecodeParser(std::ptr::null(), 0, &mut parser_info);
        assert_eq!(parse_state, DECODING_STATE::dsErrorFree);

        // 7. DecodeFrameEx
        let mut dst_len: i32 = 0;
        let mut color_fmt: i32 = 0;
        let dec_ex_state = (*p_decoder).DecodeFrameEx(
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

        // 8. SetOption & GetOption
        let mut trace_level = 0i32;
        let set_opt_ret = (*p_decoder).SetOption(
            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(set_opt_ret), CM_RESULT_SUCCESS as i64);

        let get_opt_ret = (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(get_opt_ret), CM_RESULT_SUCCESS as i64);

        // 9. Uninitialize
        let uninit_ret = (*p_decoder).Uninitialize();
        assert_eq!(i64::from(uninit_ret), CM_RESULT_SUCCESS as i64);

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

        // 1. Initialize
        let mut param = SEncParamBase::default();
        param.iPicWidth = 320;
        param.iPicHeight = 240;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 500000;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;

        let init_ret = (*p_encoder).Initialize(&param as *const SEncParamBase);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        // 2. InitializeExt
        let mut param_ext = SEncParamExt::default();
        param_ext.iPicWidth = 320;
        param_ext.iPicHeight = 240;
        param_ext.fMaxFrameRate = 30.0;
        param_ext.iTargetBitrate = 500000;
        param_ext.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        let init_ext_ret = (*p_encoder).InitializeExt(&param_ext as *const SEncParamExt);
        assert_eq!(init_ext_ret, CM_RESULT_SUCCESS);

        // 3. GetDefaultParams
        let mut default_param = SEncParamExt::default();
        let get_def_ret = (*p_encoder).GetDefaultParams(&mut default_param as *mut SEncParamExt);
        assert_eq!(get_def_ret, CM_RESULT_SUCCESS);

        // 4. EncodeFrame
        let mut src_pic = SSourcePicture::default();
        src_pic.iPicWidth = 160;
        src_pic.iPicHeight = 120;
        src_pic.iColorFormat = 23;
        let mut bs_info = SFrameBSInfo::default();
        let enc_frame_ret = (*p_encoder).EncodeFrame(&src_pic, &mut bs_info);
        assert_eq!(enc_frame_ret, CM_RESULT_SUCCESS);

        // 5. EncodeParameterSets
        let enc_ps_ret = (*p_encoder).EncodeParameterSets(&mut bs_info);
        assert_eq!(enc_ps_ret, CM_RESULT_SUCCESS);

        // 6. ForceIntraFrame
        let force_idr_ret = (*p_encoder).ForceIntraFrame(true);
        assert_eq!(force_idr_ret, CM_RESULT_SUCCESS);

        // 7. SetOption & GetOption
        let mut trace_level = 0i32;
        let set_opt_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(set_opt_ret, CM_RESULT_SUCCESS);

        let get_opt_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
            &mut trace_level as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(get_opt_ret, CM_RESULT_SUCCESS);

        // 8. Uninitialize
        let uninit_ret = (*p_encoder).Uninitialize();
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}
