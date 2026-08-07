//! Rust-side differential driver — mirrors `cxx_enc.cpp` statement for statement.
//! usage: rust_enc <src.yuv> <w> <h> <frames> <qp> <cabac 0|1> <gop> <out.264>
#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use std::fs::File;
use std::io::{Read, Write};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 9 {
        eprintln!("usage: rust_enc <src.yuv> <w> <h> <frames> <qp> <cabac> <gop> <out.264> [rcmode] [baseinit] [slicemode] [slicenum]");
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
    let baseinit: i32 = if a.len() > 10 { a[10].parse().unwrap() } else { 0 };
    // Optional 11th/12th: uiSliceMode and uiSliceNum. See cxx_enc.cpp.
    //   0 = SM_SINGLE_SLICE, 1 = SM_FIXEDSLCNUM_SLICE, 2 = SM_RASTER_SLICE,
    //   3 = SM_SIZELIMITED_SLICE (uiSliceNum is then the size constraint in bytes).
    let slicemode: i32 = if a.len() > 11 { a[11].parse().unwrap() } else { 0 };
    let slicenum: i32 = if a.len() > 12 { a[12].parse().unwrap() } else { 1 };

    unsafe {
        let mut pEnc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut pEnc), 0, "WelsCreateSVCEncoder");
        assert!(!pEnc.is_null());

        let mut p = SEncParamExt::default();
        (*pEnc).GetDefaultParams(&mut p);

        p.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        p.iPicWidth = w;
        p.iPicHeight = h;
        p.iTargetBitrate = 500000;
        p.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        p.iRCMode = std::mem::transmute::<i32, RC_MODES>(rcmode);
        p.fMaxFrameRate = 30.0;
        p.iTemporalLayerNum = 1;
        p.iSpatialLayerNum = 1;
        p.iComplexityMode = ECOMPLEXITY_MODE::LOW_COMPLEXITY;
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
        p.bEnableLongTermReference = false;
        p.iLTRRefNum = 0;
        p.iLtrMarkPeriod = 30;
        p.iMultipleThreadIdc = 1;
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

        if baseinit != 0 {
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
        }
        eprintln!("coded {} frames", coded);
        (*pEnc).Uninitialize();
        WelsDestroySVCEncoder(pEnc);
    }
}
