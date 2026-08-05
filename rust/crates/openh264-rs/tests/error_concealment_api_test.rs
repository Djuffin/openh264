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

            let init_ret = (*p_decoder).Initialize(&param as *const SDecodingParam);
            assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

            let mut current_ec: i32 = 0;
            let get_opt_ret = (*p_decoder).GetOption(
                DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC,
                &mut current_ec as *mut i32 as *mut std::ffi::c_void,
            );
            assert_eq!(i64::from(get_opt_ret), CM_RESULT_SUCCESS as i64);

            let uninit_ret = (*p_decoder).Uninitialize();
            assert_eq!(i64::from(uninit_ret), CM_RESULT_SUCCESS as i64);

            WelsDestroyDecoder(p_decoder);
        }
    }
}
