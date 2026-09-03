//! **`ForceIntraFrame` with LTR on** — `ltr_test.cpp:14-42`'s loop.
//!
//! The reference's assertion is blunt: with long-term reference enabled
//! (`iLTRRefNum = 1`, marking period 2) and `ForceIntraFrame(true)` called after every
//! frame, **every frame the encoder produces must report `videoFrameTypeIDR`**. The
//! first one is IDR because it opens the stream; each one after it is IDR because the
//! caller asked for it.
//!
//! ```c
//! while (iIdx <= p.numframes) {
//!   EncodeOneFrame (0);
//!   ASSERT_TRUE (info.eFrameType == videoFrameTypeIDR);
//!   encoder_->ForceIntraFrame (true);
//!   iIdx++;
//! }
//! ```
//!
//! The three gtest instances of this are `rand()`-seeded (`simple_test.cpp:20-24`
//! seeds from `time(NULL)`, and `iIDRPeriod` is `2^(layers-1) * (rand()%5 + 1)`), so
//! *which* instance fails moves between runs and the gtest binary alone is a poor
//! regression net. This file pins the configuration and the frame types.
//!
//! The configuration below is `prepareParamDefault` + `prepareParam`
//! (`encode_decode_api_test.cpp`) with the fixture's first parameter row
//! (`decoder_ec_test.cpp:138`: 300 frames, 160x96, 6 fps, 2 slices), and the same
//! five `SetOption` calls the test makes.

use openh264_rs::api::codec_api::*;
use openh264_rs::encoder::wels_encoder_ext::SLTRConfig;

/// `VALID_SIZE` (`encode_decode_api_test.h`) — the fixture rounds odd dimensions up.
fn valid_size(x: i32) -> i32 {
    if x % 2 == 1 { x + 1 } else { x }
}

/// One run of the reference's loop. Returns the frame type of each encoded frame.
///
/// # Safety
/// Uses the C ABI as a consumer does; every pointer is valid for its call.
unsafe fn force_idr_frame_types(width: i32, height: i32, slices: i32, frames: usize, idr_interval: i32) -> Vec<i32> {
    unsafe {
        let mut enc: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);

        let mut param = SEncParamExt::default();
        assert_eq!(
            ISVCEncoder::GetDefaultParams(enc, &mut param as *mut SEncParamExt),
            CM_RESULT_SUCCESS
        );
        // `EncodeDecodeTestBase::prepareParam`.
        let (w, h) = (valid_size(width), valid_size(height));
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.iPicWidth = w;
        param.iPicHeight = h;
        param.fMaxFrameRate = 6.0;
        param.iRCMode = RC_MODES::RC_OFF_MODE;
        param.iMultipleThreadIdc = 1;
        param.iSpatialLayerNum = 1;
        param.iNumRefFrame = AUTO_REF_PIC_COUNT;
        param.sSpatialLayers[0].iVideoWidth = w;
        param.sSpatialLayers[0].iVideoHeight = h;
        param.sSpatialLayers[0].fFrameRate = 6.0;
        param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SliceModeEnum::SM_FIXEDSLCNUM_SLICE;
        param.sSpatialLayers[0].sSliceArgument.uiSliceNum = slices as u32;

        assert_eq!(
            ISVCEncoder::InitializeExt(enc, &param as *const SEncParamExt),
            CM_RESULT_SUCCESS
        );

        let set = |id: ENCODER_OPTION, p: *mut std::ffi::c_void| {
            assert_eq!(ISVCEncoder::SetOption(enc, id, p), CM_RESULT_SUCCESS, "SetOption {id:?}");
        };
        let mut trace = 0i32; // WELS_LOG_QUIET
        set(
            ENCODER_OPTION::ENCODER_OPTION_TRACE_LEVEL,
            std::ptr::addr_of_mut!(trace).cast(),
        );
        let mut sps_pps_strategy = 1i32; // INCREASING_ID
        set(
            ENCODER_OPTION::ENCODER_OPTION_SPS_PPS_ID_STRATEGY,
            std::ptr::addr_of_mut!(sps_pps_strategy).cast(),
        );
        let mut idr = idr_interval;
        set(
            ENCODER_OPTION::ENCODER_OPTION_IDR_INTERVAL,
            std::ptr::addr_of_mut!(idr).cast(),
        );
        let mut ltr = SLTRConfig {
            bEnableLongTermReference: true,
            iLTRRefNum: 1,
        };
        set(
            ENCODER_OPTION::ENCODER_OPTION_LTR,
            std::ptr::addr_of_mut!(ltr).cast(),
        );
        let mut ltr_period = 2i32;
        set(
            ENCODER_OPTION::ENCODER_LTR_MARKING_PERIOD,
            std::ptr::addr_of_mut!(ltr_period).cast(),
        );

        // `EncodeOneFrame`: a flat luma plane and a flat chroma plane, so the content
        // never decides the frame type — only the encoder's own state does.
        let luma = (w * h) as usize;
        let frame = luma * 3 / 2;
        let mut buf = vec![0u8; frame];
        buf[..luma].fill(0x2a);
        buf[luma..].fill(0x80);

        let mut pic = SSourcePicture::default();
        pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
        pic.iPicWidth = w;
        pic.iPicHeight = h;
        pic.iStride[0] = w;
        pic.iStride[1] = w / 2;
        pic.iStride[2] = w / 2;
        pic.pData[0] = buf.as_mut_ptr();
        pic.pData[1] = buf.as_mut_ptr().add(luma);
        pic.pData[2] = buf.as_mut_ptr().add(luma + luma / 4);

        let mut types = Vec::with_capacity(frames);
        for _ in 0..frames {
            let mut info = SFrameBSInfo::default();
            let rv = ISVCEncoder::EncodeFrame(enc, &pic as *const SSourcePicture, &mut info);
            assert_eq!(rv, CM_RESULT_SUCCESS, "EncodeFrame");
            types.push(info.eFrameType as i32);
            assert_eq!(
                ISVCEncoder::ForceIntraFrame(enc, true),
                CM_RESULT_SUCCESS,
                "ForceIntraFrame"
            );
        }

        ISVCEncoder::Uninitialize(enc);
        WelsDestroySVCEncoder(enc);
        types
    }
}

/// The reference's own expectation, on the fixture's three parameter rows.
///
/// 32 frames rather than the fixture's 300: the assertion fails on the *second*
/// frame when it fails at all, and 300 flat frames at three sizes is 40 s of gate
/// time for no extra evidence.
#[test]
fn force_intra_frame_with_ltr_gives_an_idr_every_time() {
    // `decoder_ec_test.cpp:138-140`, the three instantiations of
    // `EncodeDecodeTestAPIBase/EncodeDecodeTestAPI`.
    for (w, h, slices) in [(160, 96, 2), (140, 96, 4), (140, 96, 4)] {
        // `iIDRPeriod = 2^(iTemporalLayerNum - 1) * (rand() % 5 + 1)`. The default
        // temporal-layer count is 1, so the power is 1 and the range is 1..=5; all
        // five are run, because `ForceIntraFrame` is supposed to make the interval
        // irrelevant and that claim is the point of the test.
        for idr_interval in 1..=5 {
            let types = unsafe { force_idr_frame_types(w, h, slices, 32, idr_interval) };
            for (i, t) in types.iter().enumerate() {
                assert_eq!(
                    *t,
                    EVideoFrameType::videoFrameTypeIDR as i32,
                    "{w}x{h}, {slices} slices, IDR interval {idr_interval}: frame {i} is \
                     type {t}, not IDR ({}), after ForceIntraFrame(true) — ltr_test.cpp:39",
                    EVideoFrameType::videoFrameTypeIDR as i32
                );
            }
        }
    }
}
