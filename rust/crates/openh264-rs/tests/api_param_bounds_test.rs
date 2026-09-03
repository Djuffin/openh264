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

        let init_ret = ISVCEncoder::Initialize(p_encoder, std::ptr::null());
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

        let init_ret = ISVCEncoder::Initialize(p_encoder, &param as *const SEncParamBase);
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

        let init_ret = ISVCDecoder::Initialize(p_decoder, std::ptr::null());
        assert_ne!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        WelsDestroyDecoder(p_decoder);
    }
}

// Verified against the C++ reference encoder (libopenh264.a, same parameters):
// this configuration is REJECTED, returning cmInitParaError, because
// iTargetBitrate is left at 0 while iRCMode defaults to RC_QUALITY_MODE, and
// ParamValidation() rejects `iTargetBitrate <= 0` for any RC mode but RC_OFF.
#[test]
fn test_encoder_very_large_slices() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);

        let mut param = SEncParamExt::default();
        let get_ret = ISVCEncoder::GetDefaultParams(p_encoder, &mut param as *mut SEncParamExt);
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

        let init_ret = ISVCEncoder::InitializeExt(p_encoder, &param as *const SEncParamExt);
        assert_eq!(init_ret, CM_INIT_PARA_ERROR);

        ISVCEncoder::Uninitialize(p_encoder);
        WelsDestroySVCEncoder(p_encoder);
    }
}

/// Upstream's `EncoderInitTest.ScreenContentScrollMotionVectorBounds`
/// (`test/api/encoder_test.cpp:303-360`), mirrored exactly: a 640x1800 screen
/// encode *initializes* and encodes two frames, the second scrolled down by 512
/// rows with one macroblock corrupted, so that scroll detection predicts a large
/// vertical displacement and the MVD-table indexing has to stay in bounds.
///
/// `ParamTranscode` aligns the layer size to 16 and crops (`param_svc.h:486-489`),
/// so `ParamValidationExt`'s `& 0x0F` check (`encoder_ext.cpp:521`) sees 1808 and
/// the 1800-row height is accepted.
#[test]
fn test_encoder_screen_content_scroll_motion_vector_bounds() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);

        let mut param = SEncParamExt::default();
        let get_ret = ISVCEncoder::GetDefaultParams(p_encoder, &mut param as *mut SEncParamExt);
        assert_eq!(get_ret, CM_RESULT_SUCCESS);

        param.iUsageType = EUsageType::SCREEN_CONTENT_REAL_TIME;
        param.iPicWidth = 640;
        param.iPicHeight = 1800;
        param.fMaxFrameRate = 30.0;
        param.iSpatialLayerNum = 1;
        param.iRCMode = RC_MODES::RC_OFF_MODE;

        param.sSpatialLayers[0].iVideoWidth = param.iPicWidth;
        param.sSpatialLayers[0].iVideoHeight = param.iPicHeight;
        param.sSpatialLayers[0].fFrameRate = param.fMaxFrameRate;
        param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_SINGLE_SLICE;
        param.sSpatialLayers[0].iDLayerQp = 51;
        param.iMinQp = 51;
        param.iMaxQp = 51;

        let rv = ISVCEncoder::InitializeExt(p_encoder, &param as *const SEncParamExt);
        assert_eq!(rv, 0);

        let width = param.iPicWidth as usize;
        let height = param.iPicHeight as usize;
        let frame_size = width * height * 3 / 2;
        let mut frame0 = vec![128u8; frame_size];
        let mut frame1 = vec![128u8; frame_size];

        // Fill frame0 luma with pseudo-random patterns to ensure CheckLine qualifies
        for y in 0..height {
            for x in 0..width {
                frame0[y * width + x] = ((x * 29 + y * 43 + 17) % 251) as u8;
            }
        }

        // Frame 1: shift vertically by -512 pixels (content moving downward by 512)
        let scroll_mv: isize = -512;
        for y in 0..height {
            let src = y as isize + scroll_mv;
            if src >= 0 && src < height as isize {
                let src = src as usize;
                frame1[y * width..(y + 1) * width]
                    .copy_from_slice(&frame0[src * width..(src + 1) * width]);
            } else {
                for x in 0..width {
                    frame1[y * width + x] = ((x * 53 + y * 71 + 101) & 0xFF) as u8;
                }
            }
        }

        // Modify macroblock (1, 32) at pixel rows 512..527 and cols 16..31 in frame1.
        // MB(0, 32) will be skipped by scroll detection with MV -2048.
        // MB(1, 32) cannot be skipped due to this modification, forcing it into
        // motion estimation where it uses MB(0, 32)'s scrolled MV as a predictor
        // during vertical full search.
        for y in 512..528 {
            for x in 16..32 {
                frame1[y * width + x] ^= 0xFF;
            }
        }

        let luma = width * height;
        let mut pic = SSourcePicture::default();
        pic.iPicWidth = width as i32;
        pic.iPicHeight = height as i32;
        pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
        pic.iStride[0] = width as i32;
        pic.iStride[1] = (width >> 1) as i32;
        pic.iStride[2] = (width >> 1) as i32;
        pic.pData[0] = frame0.as_mut_ptr();
        pic.pData[1] = frame0.as_mut_ptr().add(luma);
        pic.pData[2] = frame0.as_mut_ptr().add(luma + (luma >> 2));

        let mut info = SFrameBSInfo::default();

        // Encode Frame 0 (Base pattern)
        let rv = ISVCEncoder::EncodeFrame(p_encoder, &pic as *const SSourcePicture, &mut info);
        assert_eq!(rv, 0);
        pic.uiTimeStamp += 33;

        // Encode Frame 1 (Scrolled pattern with modified MB)
        pic.pData[0] = frame1.as_mut_ptr();
        pic.pData[1] = frame1.as_mut_ptr().add(luma);
        pic.pData[2] = frame1.as_mut_ptr().add(luma + (luma >> 2));
        let rv = ISVCEncoder::EncodeFrame(p_encoder, &pic as *const SSourcePicture, &mut info);
        assert_eq!(rv, 0);

        ISVCEncoder::Uninitialize(p_encoder);
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
        let init_ret = ISVCDecoder::Initialize(p_decoder, &dec_param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64);

        let mut vcl_nal = 0i32;
        let opt_vcl = ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_VCL_NAL,
            &mut vcl_nal as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_vcl), CM_RESULT_SUCCESS as i64);

        let mut frame_num = 0i32;
        let opt_fn = ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_FRAME_NUM,
            &mut frame_num as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_fn), CM_RESULT_SUCCESS as i64);

        let mut idr_id = 0i32;
        let opt_idr = ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID,
            &mut idr_id as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_idr), CM_RESULT_SUCCESS as i64);

        let mut is_ref = 0i32;
        let opt_ref = ISVCDecoder::GetOption(
            p_decoder,
            DECODER_OPTION::DECODER_OPTION_IS_REF_PIC,
            &mut is_ref as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(i64::from(opt_ref), CM_RESULT_SUCCESS as i64);

        ISVCDecoder::Uninitialize(p_decoder);
        WelsDestroyDecoder(p_decoder);
    }
}
