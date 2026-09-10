//! Core execution engine that drives C++ and Rust encoders through the identical API sequence in-memory.

#![allow(unused, unsafe_op_in_unsafe_fn, non_snake_case)]

use super::config::{BaseInitMode, DiffConfig};
use super::cpp_engine::CppLibrary;
use super::diagnostics::format_divergence_report;
use super::inputs::YuvClip;
use super::rust_engine::RustEngine;
use openh264_rs::api::codec_api::*;
use openh264_rs::encoder::wels_encoder_ext::{SLTRMarkingFeedback, SLTRRecoverRequest};
use openh264_rs::split_annexb_units;

/// Executes an encoding session in-memory on the provided `pEnc` instance.
///
/// Returns the generated Annex B H.264 bitstream.
pub unsafe fn encode_in_memory(
    pEnc: *mut ISVCEncoder,
    config: &DiffConfig,
    clip: &YuvClip,
) -> Vec<u8> {
    assert!(!pEnc.is_null());

    let mut sParam = SEncParamExt::default();
    ISVCEncoder::GetDefaultParams(pEnc, &mut sParam);

    if config.base_init == BaseInitMode::DefaultFlow {
        sParam.iPicWidth = config.width;
        sParam.iPicHeight = config.height;
        sParam.fMaxFrameRate = 30.0;
        sParam.iTargetBitrate = 2_000_000;
        sParam.iSpatialLayerNum = 1;
        sParam.iMultipleThreadIdc = config.threads;
        sParam.sSpatialLayers[0].iVideoWidth = config.width;
        sParam.sSpatialLayers[0].iVideoHeight = config.height;
        sParam.sSpatialLayers[0].fFrameRate = 30.0;
        sParam.sSpatialLayers[0].iSpatialBitrate = 2_000_000;
    } else {
        sParam.iUsageType = config.usage;
        sParam.iPicWidth = config.width;
        sParam.iPicHeight = config.height;
        sParam.iTargetBitrate = 500_000;
        sParam.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        sParam.iRCMode = config.rc_mode.unwrap_or(RC_MODES::RC_OFF_MODE);
        sParam.fMaxFrameRate = 30.0;
        sParam.iTemporalLayerNum = 1;
        sParam.iSpatialLayerNum = 1;
        sParam.iComplexityMode = config.complexity;
        sParam.uiIntraPeriod = config.gop as u32;
        sParam.iNumRefFrame = AUTO_REF_PIC_COUNT;
        sParam.eSpsPpsIdStrategy = config.ps_strategy;
        sParam.bPrefixNalAddingCtrl = false;
        sParam.bEnableSSEI = false;
        sParam.bSimulcastAVC = false;
        sParam.iPaddingFlag = 0;
        sParam.iEntropyCodingModeFlag = if config.cabac { 1 } else { 0 };
        sParam.bEnableFrameSkip = false;
        sParam.iMaxQp = 51;
        sParam.iMinQp = 0;
        sParam.uiMaxNalSize = 0;

        if let Some(ltr) = &config.ltr {
            sParam.bEnableLongTermReference = ltr.num_ref > 0;
            sParam.iLTRRefNum = ltr.num_ref;
            sParam.iLtrMarkPeriod = ltr.mark_period;
        } else {
            sParam.bEnableLongTermReference = false;
            sParam.iLTRRefNum = 0;
            sParam.iLtrMarkPeriod = 30;
        }

        sParam.iMultipleThreadIdc = config.threads;
        sParam.bUseLoadBalancing = false;
        sParam.iLoopFilterDisableIdc = 0;
        sParam.iLoopFilterAlphaC0Offset = 0;
        sParam.iLoopFilterBetaOffset = 0;
        sParam.bEnableDenoise = config.spatial_layers.as_ref().is_some_and(|sl| sl.denoise);
        sParam.bEnableBackgroundDetection = config.background_detection;
        sParam.bEnableAdaptiveQuant = false;
        sParam.bEnableFrameCroppingFlag = true;
        sParam.bEnableSceneChangeDetect = false;
        sParam.bIsLosslessLink = config.lossless;
        sParam.bFixRCOverShoot = false;
        sParam.iIdrBitrateRatio = 400;
        sParam.bPsnrY = false;
        sParam.bPsnrU = false;
        sParam.bPsnrV = false;

        sParam.sSpatialLayers[0].uiProfileIdc = if config.cabac {
            EProfileIdc::PRO_HIGH
        } else {
            EProfileIdc::PRO_BASELINE
        };
        sParam.sSpatialLayers[0].uiLevelIdc = ELevelIdc::LEVEL_UNKNOWN;
        sParam.sSpatialLayers[0].iVideoWidth = config.width;
        sParam.sSpatialLayers[0].iVideoHeight = config.height;
        sParam.sSpatialLayers[0].fFrameRate = 30.0;
        sParam.sSpatialLayers[0].iSpatialBitrate = 500_000;
        sParam.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
        sParam.sSpatialLayers[0].iDLayerQp = config.qp;

        match config.slice.mode {
            SliceModeEnum::SM_FIXEDSLCNUM_SLICE => {
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_FIXEDSLCNUM_SLICE;
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum = config.slice.arg;
            }
            SliceModeEnum::SM_RASTER_SLICE => {
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_RASTER_SLICE;
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum = config.slice.arg;
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceMbNum[0] = config.slice.arg;
            }
            SliceModeEnum::SM_SIZELIMITED_SLICE => {
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_SIZELIMITED_SLICE;
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceSizeConstraint = config.slice.arg;
            }
            _ => {
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_SINGLE_SLICE;
                sParam.sSpatialLayers[0].sSliceArgument.uiSliceNum = 1;
            }
        }

        if let Some(sl) = &config.spatial_layers {
            if sl.num_layers > 1 {
                let template = sParam.sSpatialLayers[0];
                sParam.iSpatialLayerNum = sl.num_layers;
                for i in 0..sl.num_layers as usize {
                    sParam.sSpatialLayers[i] = template;
                    sParam.sSpatialLayers[i].iVideoWidth =
                        config.width >> (sl.num_layers - 1 - i as i32);
                    sParam.sSpatialLayers[i].iVideoHeight =
                        config.height >> (sl.num_layers - 1 - i as i32);
                    sParam.sSpatialLayers[i].fFrameRate = 30.0;
                    sParam.sSpatialLayers[i].iSpatialBitrate = sParam.iTargetBitrate;
                    sParam.sSpatialLayers[i].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
                }
                sParam.iTargetBitrate *= sl.num_layers;
            }
        }
    }

    if config.base_init == BaseInitMode::Base {
        let mut b = SEncParamBase::default();
        b.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        b.fMaxFrameRate = 30.0;
        b.iPicWidth = config.width;
        b.iPicHeight = config.height;
        b.iTargetBitrate = 5_000_000;
        let ret = ISVCEncoder::Initialize(pEnc, &b);
        assert_eq!(ret, 0, "Initialize failed with code {}", ret);
    } else {
        let ret = ISVCEncoder::InitializeExt(pEnc, &sParam);
        assert_eq!(ret, 0, "InitializeExt failed with code {}", ret);
    }

    let mut bitstream = Vec::new();
    let luma_size = (config.width * config.height) as usize;
    let frames_to_encode = config.frames.min(clip.frames_yuv.len());
    let mut idr_seen: u32 = 0;

    for f in 0..frames_to_encode {
        let mut frame_buf = clip.frames_yuv[f].clone();
        let mut sPic = SSourcePicture::default();
        sPic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
        sPic.iPicWidth = config.width;
        sPic.iPicHeight = config.height;
        sPic.iStride[0] = config.width;
        sPic.iStride[1] = config.width >> 1;
        sPic.iStride[2] = config.width >> 1;
        sPic.pData[0] = frame_buf.as_mut_ptr();
        sPic.pData[1] = frame_buf.as_mut_ptr().add(luma_size);
        sPic.pData[2] = frame_buf.as_mut_ptr().add(luma_size + (luma_size >> 2));
        sPic.uiTimeStamp = (f as f64 * (1000.0 / 30.0)) as i64;

        let mut sInfo = SFrameBSInfo::default();
        let ret = ISVCEncoder::EncodeFrame(pEnc, &sPic, &mut sInfo);
        assert_eq!(ret, 0, "EncodeFrame failed at frame {}: code {}", f, ret);

        if config.set_opt_ext_frame == Some(f + 1) {
            let opt_ret = ISVCEncoder::SetOption(
                pEnc,
                ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
                std::ptr::from_mut(&mut sParam).cast::<std::ffi::c_void>(),
            );
            assert_eq!(opt_ret, 0, "SetOption failed at frame {}: code {}", f, opt_ret);
        }

        if sInfo.eFrameType != EVideoFrameType::videoFrameTypeSkip {
            for l in 0..sInfo.iLayerNum as usize {
                let lay = &sInfo.sLayerInfo[l];
                if lay.pNalLengthInByte.is_null() || lay.pBsBuf.is_null() {
                    continue;
                }
                let mut layer_len = 0usize;
                for n in 0..lay.iNalCount as usize {
                    layer_len += *lay.pNalLengthInByte.add(n) as usize;
                }
                if layer_len > 0 {
                    let nal_bytes = std::slice::from_raw_parts(lay.pBsBuf, layer_len);
                    bitstream.extend_from_slice(nal_bytes);
                }
            }
        }

        if sInfo.eFrameType == EVideoFrameType::videoFrameTypeIDR {
            idr_seen += 1;
        }

        if let Some(ltr) = &config.ltr {
            if (ltr.feedback_mask & 1) != 0 && f >= 2 {
                let mut fb = SLTRMarkingFeedback {
                    uiFeedbackType: 4, // LTR_MARKING_SUCCESS
                    uiIDRPicId: idr_seen,
                    iLTRFrameNum: f as i32 - 1,
                    iLayerId: 0,
                };
                ISVCEncoder::SetOption(
                    pEnc,
                    EncoderOption::ENCODER_LTR_MARKING_FEEDBACK,
                    &mut fb as *mut _ as *mut std::ffi::c_void,
                );
            }
            if (ltr.feedback_mask & 2) != 0 && f > 0 && (f % 8) == 5 {
                let mut rq = SLTRRecoverRequest {
                    uiFeedbackType: 1, // LTR_RECOVERY_REQUEST
                    uiIDRPicId: idr_seen,
                    iLastCorrectFrameNum: f as i32 - 2,
                    iCurrentFrameNum: f as i32,
                    iLayerId: 0,
                };
                ISVCEncoder::SetOption(
                    pEnc,
                    EncoderOption::ENCODER_LTR_RECOVERY_REQUEST,
                    &mut rq as *mut _ as *mut std::ffi::c_void,
                );
            }
        }
    }

    ISVCEncoder::Uninitialize(pEnc);
    bitstream
}

/// Verifies that the encoded H.264 bitstream can be decoded without error.
pub fn verify_decodable(bitstream: &[u8]) {
    if bitstream.is_empty() {
        return;
    }
    unsafe {
        let mut p_dec: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_dec);
        assert_eq!(ret, 0, "WelsCreateDecoder failed");
        assert!(!p_dec.is_null());

        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        let init_ret = ISVCDecoder::Initialize(p_dec, &param);
        assert_eq!(init_ret, 0, "ISVCDecoder::Initialize failed");

        let mut dst = [std::ptr::null_mut::<u8>(); 3];
        let mut buf_info = SBufferInfo::default();

        for unit in split_annexb_units(bitstream) {
            let dec_ret = ISVCDecoder::DecodeFrame2(
                p_dec,
                unit.as_ptr(),
                unit.len() as i32,
                dst.as_mut_ptr(),
                &mut buf_info,
            );
            assert!(
                (dec_ret.0 & DECODING_STATE::dsBitstreamError.0) == 0,
                "DecodeFrame2 returned bitstream error: {:?}",
                dec_ret
            );

            // Null DecodeFrame2 to construct access unit
            let _ = ISVCDecoder::DecodeFrame2(
                p_dec,
                std::ptr::null(),
                0,
                dst.as_mut_ptr(),
                &mut buf_info,
            );
        }

        ISVCDecoder::Uninitialize(p_dec);
        WelsDestroyDecoder(p_dec);
    }
}

/// Runs a differential comparison between C++ OpenH264 and Rust OpenH264 for a single configuration.
///
/// Panics with a rich divergence report if the two bitstreams do not match byte-for-byte.
pub fn run_diff_config(config: &DiffConfig, clip: &YuvClip) {
    let cpp_lib = CppLibrary::get();
    let cpp_enc = cpp_lib.create_encoder();
    let cpp_stream = unsafe { encode_in_memory(cpp_enc, config, clip) };
    cpp_lib.destroy_encoder(cpp_enc);

    let rust_enc = RustEngine::create_encoder();
    let rust_stream = unsafe { encode_in_memory(rust_enc, config, clip) };
    RustEngine::destroy_encoder(rust_enc);

    if rust_stream != cpp_stream {
        panic!(
            "{}",
            format_divergence_report(&config.label, &rust_stream, &cpp_stream)
        );
    }

    verify_decodable(&rust_stream);
}
