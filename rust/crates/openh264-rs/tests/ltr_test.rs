//! Integration test for Long-Term Reference (LTR) marking and recovery.
//! Ported from `test/api/ltr_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_ltr_encoder_configuration() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let mut param = SEncParamExt::default();
        param.iPicWidth = 320;
        param.iPicHeight = 240;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 500000;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.bEnableLongTermReference = true;
        param.iLtrMarkPeriod = 30;

        let init_ret = ISVCEncoder::InitializeExt(p_encoder, &param as *const SEncParamExt);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        let uninit_ret = ISVCEncoder::Uninitialize(p_encoder);
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}
