//! Integration test for simple encode and decode pipeline flow.
//! Ported from `test/api/simple_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_simple_encode_init_and_encode_param_sets() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let mut param = SEncParamBase::default();
        param.iPicWidth = 160;
        param.iPicHeight = 120;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 250000;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;

        let init_ret = (*p_encoder).Initialize(&param as *const SEncParamBase);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        let mut bs_info = SFrameBSInfo::default();
        let enc_param_ret = (*p_encoder).EncodeParameterSets(&mut bs_info as *mut SFrameBSInfo);
        assert_eq!(enc_param_ret, CM_RESULT_SUCCESS);

        let uninit_ret = (*p_encoder).Uninitialize();
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}
