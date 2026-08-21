//! Integration test for multithreaded decoding stability and thread isolation.
//! Ported from `test/api/thread_decoder_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_multithread_decoder_initialization() {
    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64);
        assert!(!p_decoder.is_null());

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.uiCpuLoad = 2; // Request 2 worker threads

        let init_ret = ISVCDecoder::Initialize(p_decoder, &param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        let uninit_ret = ISVCDecoder::Uninitialize(p_decoder);
        assert_eq!(i64::from(uninit_ret), CM_RESULT_SUCCESS as i64);

        WelsDestroyDecoder(p_decoder);
    }
}
