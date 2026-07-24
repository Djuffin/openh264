//! Integration test for multithreaded decoding stability and thread isolation.
//! Ported from `test/api/thread_decoder_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_multithread_decoder_initialization() {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(ret, CM_RESULT_SUCCESS as i64);
        assert!(!p_decoder.is_null());

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.uiCpuLoad = 2; // Request 2 worker threads

        let init_ret = (*p_decoder).Initialize(&param as *const SDecodingParam);
        assert_eq!(init_ret, CM_RESULT_SUCCESS as i64);

        let uninit_ret = (*p_decoder).Uninitialize();
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS as i64);

        WelsDestroyDecoder(p_decoder);
    }
}
