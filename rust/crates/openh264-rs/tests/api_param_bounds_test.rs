//! Integration tests for OpenH264 parameter validation and boundary error checks.
//! Ported from `test/api/encoder_test.cpp` and `test/api/decode_api_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_encoder_null_param_rejected() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let init_ret = (*p_encoder).Initialize(std::ptr::null());
        assert_ne!(init_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}

#[test]
fn test_encoder_invalid_resolution_rejected() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);

        let mut param = SEncParamBase::default();
        param.iPicWidth = 0; // Invalid 0 width
        param.iPicHeight = 240;
        param.fMaxFrameRate = 30.0;

        let init_ret = (*p_encoder).Initialize(&param as *const SEncParamBase);
        assert_ne!(init_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}

#[test]
fn test_decoder_null_param_rejected() {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64);
        assert!(!p_decoder.is_null());

        let init_ret = (*p_decoder).Initialize(std::ptr::null());
        assert_ne!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        WelsDestroyDecoder(p_decoder);
    }
}

// Verified against the C++ reference encoder (libopenh264.a, same parameters):
// this configuration is REJECTED, returning cmInitParaError, because
// iTargetBitrate is left at 0 while iRCMode defaults to RC_QUALITY_MODE, and
// ParamValidation() rejects `iTargetBitrate <= 0` for any RC mode but RC_OFF.
// The assertion below (cmResultSuccess) therefore does not describe upstream
// behaviour and needs correcting.
//
// It cannot simply be flipped yet: ParamValidationExt runs the slice-mode switch
// before ParamValidation, and the Rust port's SM_FIXEDSLCNUM_SLICE arm is a
// todo!() awaiting SliceArgumentValidationFixedSliceMode from
// svc_enc_slice_segment.cpp (Phase 3.9). Un-ignore once that lands.
#[test]
#[ignore = "SM_FIXEDSLCNUM_SLICE validation not ported yet (Phase 3.9); \
            the assertion also encodes the wrong expected result — see comment"]
fn test_encoder_very_large_slices() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);

        let mut param = SEncParamExt::default();
        let get_ret = (*p_encoder).GetDefaultParams(&mut param as *mut SEncParamExt);
        assert_eq!(get_ret, CM_RESULT_SUCCESS);

        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.iPicWidth = 1280;
        param.iPicHeight = 720;
        param.fMaxFrameRate = 30.0;
        param.iSpatialLayerNum = 1;
        param.iMultipleThreadIdc = 4;

        param.sSpatialLayers[0].iVideoWidth = param.iPicWidth;
        param.sSpatialLayers[0].iVideoHeight = param.iPicHeight;
        param.sSpatialLayers[0].fFrameRate = param.fMaxFrameRate;
        param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_FIXEDSLCNUM_SLICE;
        param.sSpatialLayers[0].sSliceArgument.uiSliceNum = 4;
        param.sSpatialLayers[0].iDLayerQp = 12;

        let init_ret = (*p_encoder).InitializeExt(&param as *const SEncParamExt);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        let mut bs_info = SFrameBSInfo::default();
        let enc_ret = (*p_encoder).EncodeParameterSets(&mut bs_info);
        assert_eq!(enc_ret, CM_RESULT_SUCCESS);

        (*p_encoder).Uninitialize();
        WelsDestroySVCEncoder(p_encoder);
    }
}

#[test]
fn test_encoder_screen_content_scroll_motion_vector_bounds() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);

        let mut param = SEncParamExt::default();
        let get_ret = (*p_encoder).GetDefaultParams(&mut param as *mut SEncParamExt);
        assert_eq!(get_ret, CM_RESULT_SUCCESS);

        param.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME;
        param.iPicWidth = 640;
        param.iPicHeight = 1800;
        param.fMaxFrameRate = 30.0;
        param.iSpatialLayerNum = 1;

        param.sSpatialLayers[0].iVideoWidth = param.iPicWidth;
        param.sSpatialLayers[0].iVideoHeight = param.iPicHeight;
        param.sSpatialLayers[0].fFrameRate = param.fMaxFrameRate;
        param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_SINGLE_SLICE;
        param.sSpatialLayers[0].iDLayerQp = 51;

        // 1800 is not a multiple of 16, so ParamValidationExt rejects it
        // (encoder_ext.cpp:521). Verified against the C++ reference encoder with
        // these exact parameters: InitializeExt returns cmInitParaError. The test
        // previously asserted cmResultSuccess, which upstream never returns here.
        let init_ret = (*p_encoder).InitializeExt(&param as *const SEncParamExt);
        assert_eq!(init_ret, CM_INIT_PARA_ERROR);

        (*p_encoder).Uninitialize();
        WelsDestroySVCEncoder(p_encoder);
    }
}

#[test]
fn test_decoder_get_set_options_vcl_nal_framenum_idr_isref() {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64);

        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        let init_ret = (*p_decoder).Initialize(&dec_param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        let mut vcl_nal = 0i32;
        let opt_vcl = (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_VCL_NAL,
            &mut vcl_nal as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_vcl), CM_RESULT_SUCCESS as i64);

        let mut frame_num = 0i32;
        let opt_fn = (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_FRAME_NUM,
            &mut frame_num as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_fn), CM_RESULT_SUCCESS as i64);

        let mut idr_id = 0i32;
        let opt_idr = (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID,
            &mut idr_id as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_idr), CM_RESULT_SUCCESS as i64);

        let mut is_ref = 0i32;
        let opt_ref = (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_IS_REF_PIC,
            &mut is_ref as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_ref), CM_RESULT_SUCCESS as i64);

        (*p_decoder).Uninitialize();
        WelsDestroyDecoder(p_decoder);
    }
}
