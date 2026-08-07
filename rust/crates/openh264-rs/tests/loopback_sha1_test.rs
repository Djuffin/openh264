mod common;
use common::Sha1Hasher;
use openh264_rs::api::codec_api::*;

fn update_hash_from_encoded_frame(hasher: &mut Sha1Hasher, bs_info: &SFrameBSInfo) {
    for i in 0..bs_info.iLayerNum as usize {
        let layer = &bs_info.sLayerInfo[i];
        let mut layer_size = 0usize;
        if !layer.pNalLengthInByte.is_null() {
            for j in 0..layer.iNalCount as usize {
                unsafe {
                    layer_size += *layer.pNalLengthInByte.add(j) as usize;
                }
            }
        }
        if layer_size > 0 && !layer.pBsBuf.is_null() {
            unsafe {
                let slice = std::slice::from_raw_parts(layer.pBsBuf, layer_size);
                hasher.update(slice);
            }
        }
    }
}

use openh264_rs::split_annexb_units;

#[test]
fn test_loopback_encode_and_decode_pipeline() {
    let test_resolutions = [(160, 120), (320, 240), (640, 360)];

    for (width, height) in test_resolutions {
        unsafe {
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

            let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
            let dec_create = WelsCreateDecoder(&mut p_decoder);
            assert_eq!(i64::from(dec_create), CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;

            let dec_init = (*p_decoder).Initialize(&dec_param as *const SDecodingParam);
            assert_eq!(i64::from(dec_init), CM_RESULT_SUCCESS as i64);

            let mut bs_info = SFrameBSInfo::default();
            let ps_ret = (*p_encoder).EncodeParameterSets(&mut bs_info);
            assert_eq!(ps_ret, CM_RESULT_SUCCESS);

            let frame_size = (width * height * 3 / 2) as usize;
            let mut yuv_input = vec![128u8; frame_size];
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
            assert_eq!(i64::from((*p_decoder).Uninitialize()), CM_RESULT_SUCCESS as i64);

            WelsDestroySVCEncoder(p_encoder);
            WelsDestroyDecoder(p_decoder);
        }
    }
}

struct DecodeEncodeFileParam {
    file_name: &'static str,
    hash_str: &'static str,
    width: i32,
    height: i32,
    frame_rate: f32,
}

static K_DECODE_ENCODE_FILE_ARRAY: &[DecodeEncodeFileParam] = &[
    DecodeEncodeFileParam {
        file_name: "res/test_vd_1d.264",
        hash_str: "34fc3aee85cc0b0223c2701d810a536fe3818a00",
        width: 320,
        height: 192,
        frame_rate: 12.0,
    },
    DecodeEncodeFileParam {
        file_name: "res/test_vd_rc.264",
        hash_str: "9f15b0677b5f7daa922079ec4fa49e3f457fc998",
        width: 320,
        height: 192,
        frame_rate: 12.0,
    },
];

fn workspace_root() -> std::path::PathBuf {
    let mut root = std::path::PathBuf::from("../../../");
    if !root.join("res").exists() {
        root = std::path::PathBuf::from("../../");
    }
    root
}

/// Upstream's `DecodeEncodeFile/DecodeEncodeTest.CompareOutput`
/// (`test/api/decode_encode_test.cpp`), which passes against `libopenh264.a`.
///
/// This calls `Initialize` with a bare `SEncParamBase`, exactly as upstream's
/// `BaseEncoderTest::InitWithParam` does for this configuration. That leaves
/// **every** `FillDefault` value in place, not just `iRCMode = RC_QUALITY_MODE`:
/// `bEnableSceneChangeDetect`, `bEnableBackgroundDetection`, `bEnableAdaptiveQuant`
/// and `bEnableFrameSkip` are all `true` as well.
///
/// It was `#[ignore]`d through Phase 5.0 because the QP-adapting rate-control modes
/// were not byte-exact. Phase 5.1 closed that and three more things this
/// configuration needs — `METHOD_COMPLEXITY_ANALYSIS`,
/// `METHOD_BACKGROUND_DETECTION` and the `WelsMdUpdateBGDInfo` that was shadowed
/// by an empty stub — and `compare.sh` now exits 0 for this exact
/// `Initialize(SEncParamBase)` path as well
/// (`compare.sh <yuv> <w> <h> <n> <qp> <cabac> <gop> <rcmode> 1`).
#[test]
fn test_decode_encode_full_cycle_sha1_parity() {
    let repo_root = workspace_root();
    for param in K_DECODE_ENCODE_FILE_ARRAY {
        let file_path = repo_root.join(param.file_name);
        assert!(file_path.exists(), "Asset file {} must exist", file_path.display());
        let data = std::fs::read(&file_path).expect("Failed to read bitstream asset");
        assert!(!data.is_empty(), "Asset file {} must not be empty", file_path.display());

        unsafe {
            // 1. Create decoder
            let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
            let dec_create = WelsCreateDecoder(&mut p_decoder);
            assert_eq!(i64::from(dec_create), CM_RESULT_SUCCESS as i64);
            assert!(!p_decoder.is_null());

            let mut dec_param = SDecodingParam::default();
            dec_param.uiTargetDqLayer = u8::MAX;
            dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
            dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

            let dec_init = (*p_decoder).Initialize(&dec_param as *const SDecodingParam);
            assert_eq!(i64::from(dec_init), CM_RESULT_SUCCESS as i64);

            // 2. Create encoder
            let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
            let enc_create = WelsCreateSVCEncoder(&mut p_encoder);
            assert_eq!(enc_create, CM_RESULT_SUCCESS);
            assert!(!p_encoder.is_null());

            // `hash_str` is upstream's expectation from
            // `test/api/decode_encode_test.cpp:133`, so the encoder has to be set up
            // exactly the way that test sets it up. `BaseEncoderTest::InitWithParam`
            // takes its `bBaseParamFlag` branch for this configuration — single
            // slice, one spatial layer, no denoise, no lossless link, no LTR, CAVLC —
            // and calls `Initialize` with a zeroed `SEncParamBase` carrying only
            // usage type, frame rate, width, height and `iTargetBitrate = 5000000`.
            //
            // This used to build an `SEncParamExt` by hand with a 500 kbit/s target
            // and call `InitializeExt`, which is a different rate-control
            // configuration entirely — the hash could not have matched whatever the
            // encoder did.
            let mut enc_param = SEncParamBase::default();
            enc_param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            enc_param.fMaxFrameRate = param.frame_rate;
            enc_param.iPicWidth = param.width;
            enc_param.iPicHeight = param.height;
            enc_param.iTargetBitrate = 5_000_000;

            let enc_init = (*p_encoder).Initialize(&enc_param as *const SEncParamBase);
            assert_eq!(enc_init, CM_RESULT_SUCCESS);

            let mut hasher = Sha1Hasher::new();
            let units = split_annexb_units(&data);

            let encode_picture_frame = |p_dst: [*mut u8; 3], buf_info: &SBufferInfo, hasher: &mut Sha1Hasher| {
                if buf_info.iBufferStatus == 1 {
                    let w = buf_info.UsrData.sSystemBuffer.iWidth;
                    let h = buf_info.UsrData.sSystemBuffer.iHeight;
                    let stride_y = buf_info.UsrData.sSystemBuffer.iStride[0];
                    let stride_uv = buf_info.UsrData.sSystemBuffer.iStride[1];

                    let mut src_pic = SSourcePicture::default();
                    src_pic.iPicWidth = w;
                    src_pic.iPicHeight = h;
                    src_pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
                    src_pic.iStride[0] = stride_y;
                    src_pic.iStride[1] = stride_uv;
                    src_pic.iStride[2] = stride_uv;
                    src_pic.pData[0] = p_dst[0];
                    src_pic.pData[1] = p_dst[1];
                    src_pic.pData[2] = p_dst[2];

                    let mut bs_info = SFrameBSInfo::default();
                    let enc_ret = (*p_encoder).EncodeFrame(&src_pic, &mut bs_info);
                    if enc_ret == CM_RESULT_SUCCESS {
                        update_hash_from_encoded_frame(hasher, &bs_info);
                    }
                }
            };

            let mut timestamp = 0u64;
            for unit in units {
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                timestamp += 1;
                buf_info.uiInBsTimeStamp = timestamp;
                let dec_ret = (*p_decoder).DecodeFrameNoDelay(
                    unit.as_ptr(),
                    unit.len() as i32,
                    p_dst.as_mut_ptr(),
                    &mut buf_info,
                );
                if dec_ret == DECODING_STATE::dsErrorFree {
                    encode_picture_frame(p_dst, &buf_info, &mut hasher);
                }

                let mut null_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut null_buf_info = SBufferInfo::default();
                null_buf_info.uiInBsTimeStamp = timestamp;
                let recon_ret = (*p_decoder).DecodeFrame2(
                    std::ptr::null(),
                    0,
                    null_dst.as_mut_ptr(),
                    &mut null_buf_info,
                );
                if recon_ret == DECODING_STATE::dsErrorFree {
                    encode_picture_frame(null_dst, &null_buf_info, &mut hasher);
                }
            }

            // Flush remaining frames in decoder buffer
            let mut eos_flag = 1i32;
            (*p_decoder).SetOption(
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
            );

            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let dec_ret = (*p_decoder).DecodeFrame2(
                std::ptr::null(),
                0,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            if dec_ret == DECODING_STATE::dsErrorFree {
                encode_picture_frame(p_dst, &buf_info, &mut hasher);
            }

            let mut remaining_frames = 0i32;
            (*p_decoder).GetOption(
                DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                &mut remaining_frames as *mut i32 as *mut std::ffi::c_void,
            );

            for _ in 0..remaining_frames {
                let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                let mut buf_info = SBufferInfo::default();
                let flush_ret = (*p_decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
                if flush_ret == DECODING_STATE::dsErrorFree {
                    encode_picture_frame(p_dst, &buf_info, &mut hasher);
                }
            }

            let calculated_hash = hasher.digest();
            assert_eq!(
                calculated_hash, param.hash_str,
                "SHA-1 hash mismatch in full decode-encode cycle for {}",
                param.file_name
            );

            (*p_encoder).Uninitialize();
            WelsDestroySVCEncoder(p_encoder);
            (*p_decoder).Uninitialize();
            WelsDestroyDecoder(p_decoder);
        }
    }
}
