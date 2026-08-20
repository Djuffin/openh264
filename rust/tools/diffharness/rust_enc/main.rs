//! Rust-side differential driver — mirrors `cxx_enc.cpp` statement for statement.
//! usage: rust_enc <src.yuv> <w> <h> <frames> <qp> <cabac 0|1> <gop> <out.264>
#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use openh264_rs::encoder::wels_encoder_ext::{SLTRMarkingFeedback, SLTRRecoverRequest};
use std::fs::File;
use std::io::{Read, Write};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 9 {
        eprintln!("usage: rust_enc <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [rcmode] [baseinit 0|1|2] [slicemode] [slicenum] [threads] [complexity] [ltr] [ltrperiod] [ltrfb]");
        std::process::exit(1);
    }
    let src = &a[1];
    let w: i32 = a[2].parse().unwrap();
    let h: i32 = a[3].parse().unwrap();
    let frames: i32 = a[4].parse().unwrap();
    let qp: i32 = a[5].parse().unwrap();
    let cabac: i32 = a[6].parse().unwrap();
    let gop: i32 = a[7].parse().unwrap();
    let out = &a[8];
    // Optional 9th argument: iRCMode. Defaults to RC_OFF_MODE, the gate configuration.
    let rcmode: i32 = if a.len() > 9 { a[9].parse().unwrap() } else { RC_MODES::RC_OFF_MODE as i32 };
    // Optional 10th argument: 1 selects Initialize(SEncParamBase), the path
    // upstream's BaseEncoderTest::InitWithParam takes and the one the SHA-1 parity
    // test exercises. It leaves FillDefault's values in place, so scene-change
    // detection, background detection, adaptive quantisation and frame skip are all
    // ON. 0 (default) is InitializeExt with the gate configuration.
    // 2 selects the GetDefaultParams + InitializeExt path: FillDefault's values with
    // only width/height/framerate/bitrate/threads set on top — the ordinary API flow,
    // the one c_vs_rust_bench drives. qp/cabac/gop/rcmode/slice args are ignored.
    let baseinit: i32 = if a.len() > 10 { a[10].parse().unwrap() } else { 0 };
    // Optional 11th/12th: uiSliceMode and uiSliceNum. See cxx_enc.cpp.
    //   0 = SM_SINGLE_SLICE, 1 = SM_FIXEDSLCNUM_SLICE, 2 = SM_RASTER_SLICE,
    //   3 = SM_SIZELIMITED_SLICE (uiSliceNum is then the size constraint in bytes).
    let slicemode: i32 = if a.len() > 11 { a[11].parse().unwrap() } else { 0 };
    let slicenum: i32 = if a.len() > 12 { a[12].parse().unwrap() } else { 1 };
    // Optional 13th: iMultipleThreadIdc. 1 (default) is single-threaded.
    let threads: i32 = if a.len() > 13 { a[13].parse().unwrap() } else { 1 };
    // Optional 14th: iComplexityMode. 0 LOW (default, and what every sweep preset
    // runs), 1 MEDIUM, 2 HIGH. See cxx_enc.cpp.
    let complexity: i32 = if a.len() > 14 { a[14].parse().unwrap() } else { 0 };
    // Optional 15th: iLTRRefNum. 0 (default) leaves long-term reference OFF, which is
    // what every preset before `ltr` ran; N > 0 turns bEnableLongTermReference on and
    // asks for N long-term slots. See cxx_enc.cpp.
    let ltr: i32 = if a.len() > 15 { a[15].parse().unwrap() } else { 0 };
    // Optional 16th: iLtrMarkPeriod. 30 is FillDefault's.
    let ltrperiod: i32 = if a.len() > 16 { a[16].parse().unwrap() } else { 30 };
    // Optional 17th: LTR feedback bitmask. 1 = marking feedback, 2 = recovery
    // request. See cxx_enc.cpp — the schedule is fixed and identical on both sides.
    let ltrfb: i32 = if a.len() > 17 { a[17].parse().unwrap() } else { 0 };

    unsafe {
        let mut pEnc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut pEnc), 0, "WelsCreateSVCEncoder");
        assert!(!pEnc.is_null());

        let mut p = SEncParamExt::default();
        (*pEnc).GetDefaultParams(&mut p);

        if baseinit == 2 {
            // ---- defaults mode: exactly what c_vs_rust_bench's fill_params sets ----
            p.iPicWidth = w;
            p.iPicHeight = h;
            p.fMaxFrameRate = 30.0;
            p.iTargetBitrate = 2_000_000;
            p.iSpatialLayerNum = 1;
            p.iMultipleThreadIdc = threads as u16;
            p.sSpatialLayers[0].iVideoWidth = w;
            p.sSpatialLayers[0].iVideoHeight = h;
            p.sSpatialLayers[0].fFrameRate = 30.0;
            p.sSpatialLayers[0].iSpatialBitrate = 2_000_000;
        } else {
        p.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        p.iPicWidth = w;
        p.iPicHeight = h;
        p.iTargetBitrate = 500000;
        p.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        p.iRCMode = std::mem::transmute::<i32, RC_MODES>(rcmode);
        p.fMaxFrameRate = 30.0;
        p.iTemporalLayerNum = 1;
        p.iSpatialLayerNum = 1;
        p.iComplexityMode = match complexity {
            1 => ECOMPLEXITY_MODE::MEDIUM_COMPLEXITY,
            2 => ECOMPLEXITY_MODE::HIGH_COMPLEXITY,
            _ => ECOMPLEXITY_MODE::LOW_COMPLEXITY,
        };
        p.uiIntraPeriod = gop as u32;
        p.iNumRefFrame = AUTO_REF_PIC_COUNT;
        p.eSpsPpsIdStrategy = EParameterSetStrategy::CONSTANT_ID;
        p.bPrefixNalAddingCtrl = false;
        p.bEnableSSEI = false;
        p.bSimulcastAVC = false;
        p.iPaddingFlag = 0;
        p.iEntropyCodingModeFlag = cabac;
        p.bEnableFrameSkip = false;
        p.iMaxQp = 51;
        p.iMinQp = 0;
        p.uiMaxNalSize = 0;
        p.bEnableLongTermReference = ltr > 0;
        p.iLTRRefNum = ltr;
        p.iLtrMarkPeriod = ltrperiod as u32;
        p.iMultipleThreadIdc = threads as u16;
        p.bUseLoadBalancing = false;
        p.iLoopFilterDisableIdc = 0;
        p.iLoopFilterAlphaC0Offset = 0;
        p.iLoopFilterBetaOffset = 0;
        p.bEnableDenoise = false;
        p.bEnableBackgroundDetection = false;
        p.bEnableAdaptiveQuant = false;
        p.bEnableFrameCroppingFlag = true;
        p.bEnableSceneChangeDetect = false;
        p.bIsLosslessLink = false;
        p.bFixRCOverShoot = false;
        p.iIdrBitrateRatio = 400;
        p.bPsnrY = false;
        p.bPsnrU = false;
        p.bPsnrV = false;

        // See cxx_enc.cpp: a baseline layer forces CAVLC, so the profile has to
        // follow the cabac flag or `cabac 1` never reaches the CABAC writers.
        p.sSpatialLayers[0].uiProfileIdc = if cabac != 0 {
            EProfileIdc::PRO_HIGH
        } else {
            EProfileIdc::PRO_BASELINE
        };
        p.sSpatialLayers[0].uiLevelIdc = ELevelIdc::LEVEL_UNKNOWN;
        p.sSpatialLayers[0].iVideoWidth = w;
        p.sSpatialLayers[0].iVideoHeight = h;
        p.sSpatialLayers[0].fFrameRate = 30.0;
        p.sSpatialLayers[0].iSpatialBitrate = 500000;
        p.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
        p.sSpatialLayers[0].iDLayerQp = qp;
        match slicemode {
            1 => {
                p.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_FIXEDSLCNUM_SLICE;
                p.sSpatialLayers[0].sSliceArgument.uiSliceNum = slicenum as u32;
            }
            2 => {
                p.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_RASTER_SLICE;
                p.sSpatialLayers[0].sSliceArgument.uiSliceNum = slicenum as u32;
                p.sSpatialLayers[0].sSliceArgument.uiSliceMbNum[0] = slicenum as u32;
            }
            3 => {
                p.sSpatialLayers[0].sSliceArgument.uiSliceMode =
                    SliceModeEnum::SM_SIZELIMITED_SLICE;
                p.sSpatialLayers[0].sSliceArgument.uiSliceSizeConstraint = slicenum as u32;
            }
            _ => {
                p.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_SINGLE_SLICE;
                p.sSpatialLayers[0].sSliceArgument.uiSliceNum = 1;
            }
        }
        }

        if baseinit == 1 {
            let mut b = SEncParamBase::default();
            b.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
            b.fMaxFrameRate = 30.0;
            b.iPicWidth = w;
            b.iPicHeight = h;
            b.iTargetBitrate = 5000000;
            let ret = (*pEnc).Initialize(&b);
            assert_eq!(ret, 0, "Initialize returned {}", ret);
        } else {
            let ret = (*pEnc).InitializeExt(&p);
            assert_eq!(ret, 0, "InitializeExt returned {}", ret);
        }

        let mut fsrc = File::open(src).expect("open src");
        let mut fout = File::create(out).expect("create out");

        let luma = (w * h) as usize;
        let size = luma * 3 / 2;
        let mut buf = vec![0u8; size];

        let mut coded = 0;

        // See cxx_enc.cpp: the feedback must quote the encoder's own `uiIdrPicId`.

        let mut idr_seen: u32 = 0;
        for f in 0..frames {
            if fsrc.read_exact(&mut buf).is_err() {
                break;
            }
            let mut pic = SSourcePicture::default();
            pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            pic.iPicWidth = w;
            pic.iPicHeight = h;
            pic.iStride[0] = w;
            pic.iStride[1] = w >> 1;
            pic.iStride[2] = w >> 1;
            pic.pData[0] = buf.as_mut_ptr();
            pic.pData[1] = buf.as_mut_ptr().add(luma);
            pic.pData[2] = buf.as_mut_ptr().add(luma + (luma >> 2));
            pic.uiTimeStamp = (f as f64 * (1000.0 / 30.0)) as i64;

            let mut info = SFrameBSInfo::default();
            let ret = (*pEnc).EncodeFrame(&pic, &mut info);
            if ret != 0 {
                eprintln!("EncodeFrame failed at {}: {}", f, ret);
                break;
            }
            if info.eFrameType == EVideoFrameType::videoFrameTypeSkip {
                continue;
            }
            for l in 0..info.iLayerNum as usize {
                let lay = &info.sLayerInfo[l];
                let mut len = 0usize;
                if lay.pNalLengthInByte.is_null() {
                    continue;
                }
                for n in 0..lay.iNalCount as usize {
                    len += *lay.pNalLengthInByte.add(n) as usize;
                }
                if len > 0 && !lay.pBsBuf.is_null() {
                    fout.write_all(std::slice::from_raw_parts(lay.pBsBuf, len)).unwrap();
                }
            }
            eprintln!(
                "frame {}: type={:?} layers={} bytes={}",
                f, info.eFrameType, info.iLayerNum, info.iFrameSizeInBytes
            );
            coded += 1;
            if info.eFrameType == EVideoFrameType::videoFrameTypeIDR {
                idr_seen += 1;
            }

            if (ltrfb & 1) != 0 && f >= 2 {
                let mut fb = SLTRMarkingFeedback {
                    uiFeedbackType: 4, // LTR_MARKING_SUCCESS
                    uiIDRPicId: idr_seen,
                    iLTRFrameNum: f - 1,
                    iLayerId: 0,
                };
                (*pEnc).SetOption(
                    EncoderOption::ENCODER_LTR_MARKING_FEEDBACK,
                    &mut fb as *mut _ as *mut std::ffi::c_void,
                );
            }
            if (ltrfb & 2) != 0 && f > 0 && (f % 8) == 5 {
                let mut rq = SLTRRecoverRequest {
                    uiFeedbackType: 1, // LTR_RECOVERY_REQUEST
                    uiIDRPicId: idr_seen,
                    iLastCorrectFrameNum: f - 2,
                    iCurrentFrameNum: f,
                    iLayerId: 0,
                };
                (*pEnc).SetOption(
                    EncoderOption::ENCODER_LTR_RECOVERY_REQUEST,
                    &mut rq as *mut _ as *mut std::ffi::c_void,
                );
            }
        }
        eprintln!("coded {} frames", coded);
        (*pEnc).Uninitialize();
        WelsDestroySVCEncoder(pEnc);
    }
}
