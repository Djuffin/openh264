//! Integration test for end-to-end loopback encode/decode pipeline.
//! Ported from `test/api/decode_encode_test.cpp` and `test/api/encode_decode_api_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_loopback_encode_and_decode_pipeline() {
    let test_resolutions = [(160, 120), (320, 240), (640, 360)];

    for (width, height) in test_resolutions {
        unsafe {
            // 1. Instantiate and initialize encoder
            let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
            let enc_create = WelsCreateSVCEncoder(&mut p_encoder);
            assert_eq!(enc_create, CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());

            let mut enc_param = SEncParamBase::default();
            enc_param.iPicWidth = width;
            enc_param.iPicHeight = height;
            enc_param.fMaxFrameRate = 30.0;
            enc_param.iTargetBitrate = 250000;
            enc_param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;

            let enc_init = (*p_encoder).Initialize(&enc_param as *const SEncParamBase);
            assert_eq!(enc_init, CM_RESULT_SUCCESS);

            // 2. Instantiate and initialize decoder
            let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
            let dec_create = WelsCreateDecoder(&mut p_decoder);
            assert_eq!(dec_create, CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;

            let dec_init = (*p_decoder).Initialize(&dec_param as *const SDecodingParam);
            assert_eq!(dec_init, CM_RESULT_SUCCESS as i64);

            // 3. Generate parameter sets and encode frame
            let mut bs_info = SFrameBSInfo::default();
            let ps_ret = (*p_encoder).EncodeParameterSets(&mut bs_info);
            assert_eq!(ps_ret, CM_RESULT_SUCCESS);

            let frame_size = (width * height * 3 / 2) as usize;
            let mut yuv_input = vec![128u8; frame_size];
            // Pattern generator
            for y in 0..height as usize {
                for x in 0..width as usize {
                    yuv_input[y * width as usize + x] = ((x * 4 + y * 4) % 256) as u8;
                }
            }

            let mut src_pic = SSourcePicture::default();
            src_pic.iPicWidth = width;
            src_pic.iPicHeight = height;
            src_pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            src_pic.iStride[0] = width;
            src_pic.iStride[1] = width / 2;
            src_pic.iStride[2] = width / 2;
            src_pic.pData[0] = yuv_input.as_mut_ptr();
            src_pic.pData[1] = yuv_input.as_mut_ptr().add((width * height) as usize);
            src_pic.pData[2] = yuv_input.as_mut_ptr().add((width * height * 5 / 4) as usize);

            let enc_frame_ret = (*p_encoder).EncodeFrame(&src_pic, &mut bs_info);
            assert_eq!(enc_frame_ret, CM_RESULT_SUCCESS);

            // 4. Decode encoded stream
            if bs_info.eFrameType != EVideoFrameType::videoFrameTypeInvalid
                && bs_info.eFrameType != EVideoFrameType::videoFrameTypeSkip
            {
                for i in 0..bs_info.iLayerNum as usize {
                    let layer = &bs_info.sLayerInfo[i];
                    let mut layer_len = 0;
                    if !layer.pNalLengthInByte.is_null() {
                        for n in 0..layer.iNalCount as usize {
                            layer_len += *layer.pNalLengthInByte.add(n);
                        }
                    }
                    if layer_len > 0 && !layer.pBsBuf.is_null() {
                        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                        let mut dst_info = SBufferInfo::default();
                        let dec_ret = (*p_decoder).DecodeFrame2(
                            layer.pBsBuf,
                            layer_len,
                            p_dst.as_mut_ptr(),
                            &mut dst_info,
                        );
                        assert_eq!(dec_ret, DECODING_STATE::dsErrorFree);
                    }
                }
            }

            // 5. Uninitialize and destroy safely
            assert_eq!((*p_encoder).Uninitialize(), CM_RESULT_SUCCESS);
            assert_eq!((*p_decoder).Uninitialize(), CM_RESULT_SUCCESS as i64);

            WelsDestroySVCEncoder(p_encoder);
            WelsDestroyDecoder(p_decoder);
        }
    }
}
