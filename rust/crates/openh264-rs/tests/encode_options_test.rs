//! Integration test for dynamic encoder options and runtime reconfiguration.
//! Ported from `test/api/encode_options_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_encoder_set_and_get_options() {
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

        let init_ret = (*p_encoder).Initialize(&param as *const SEncParamBase);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        // 1. Test frame rate option modification
        let mut f_fps: f32 = 15.0;
        let opt_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_FRAME_RATE,
            &mut f_fps as *mut f32 as *mut std::ffi::c_void,
        );
        assert_eq!(opt_ret, CM_RESULT_SUCCESS);

        // 2. Test bitrate option modification
        let mut bitrate_info = SBitrateInfo::default();
        bitrate_info.iBitrate = 300000;
        let opt_br_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_BITRATE,
            &mut bitrate_info as *mut SBitrateInfo as *mut std::ffi::c_void,
        );
        assert_eq!(opt_br_ret, CM_RESULT_SUCCESS);

        // 3. Test IDR interval option modification
        let mut idr_interval: i32 = 60;
        let opt_idr_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_IDR_INTERVAL,
            &mut idr_interval as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(opt_idr_ret, CM_RESULT_SUCCESS);

        // 4. Test SEncParamBase query and set
        let mut base_param = SEncParamBase::default();
        let get_base_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE,
            &mut base_param as *mut SEncParamBase as *mut std::ffi::c_void,
        );
        assert_eq!(get_base_ret, CM_RESULT_SUCCESS);

        // 5. Test SEncParamExt query and set
        let mut ext_param = SEncParamExt::default();
        let get_ext_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
            &mut ext_param as *mut SEncParamExt as *mut std::ffi::c_void,
        );
        assert_eq!(get_ext_ret, CM_RESULT_SUCCESS);

        // 6. Test statistics option query
        let mut stats = SEncoderStatistics::default();
        let get_stats_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_GET_STATISTICS,
            &mut stats as *mut SEncoderStatistics as *mut std::ffi::c_void,
        );
        assert_eq!(get_stats_ret, CM_RESULT_SUCCESS);

        let uninit_ret = (*p_encoder).Uninitialize();
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}
