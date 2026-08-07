//! Integration test for dynamic encoder options and runtime reconfiguration.
//! Ported from `test/api/encode_options_test.cpp`.

use openh264_rs::api::codec_api::*;

#[test]
fn test_encoder_set_and_get_options() {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let mut param = SEncParamBase::default();
        param.iPicWidth = 320;
        param.iPicHeight = 240;
        param.fMaxFrameRate = 30.0;
        param.iTargetBitrate = 500000;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;

        let init_ret = (*p_encoder).Initialize(&param as *const SEncParamBase);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        // 1. Test frame rate option modification
        let mut f_fps: f32 = 15.0;
        let opt_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_FRAME_RATE,
            &mut f_fps as *mut f32 as *mut std::ffi::c_void,
        );
        assert_eq!(opt_ret, CM_RESULT_SUCCESS);

        // 2. Test bitrate option modification
        let mut bitrate_info = SBitrateInfo::default();
        bitrate_info.iBitrate = 300000;
        let opt_br_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_BITRATE,
            &mut bitrate_info as *mut SBitrateInfo as *mut std::ffi::c_void,
        );
        assert_eq!(opt_br_ret, CM_RESULT_SUCCESS);

        // 3. Test IDR interval option modification
        let mut idr_interval: i32 = 60;
        let opt_idr_ret = (*p_encoder).SetOption(
            ENCODER_OPTION::ENCODER_OPTION_IDR_INTERVAL,
            &mut idr_interval as *mut i32 as *mut std::ffi::c_void,
        );
        assert_eq!(opt_idr_ret, CM_RESULT_SUCCESS);

        // 4. Test SEncParamBase query and set
        let mut base_param = SEncParamBase::default();
        let get_base_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_BASE,
            &mut base_param as *mut SEncParamBase as *mut std::ffi::c_void,
        );
        assert_eq!(get_base_ret, CM_RESULT_SUCCESS);

        // 5. Test SEncParamExt query and set
        let mut ext_param = SEncParamExt::default();
        let get_ext_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
            &mut ext_param as *mut SEncParamExt as *mut std::ffi::c_void,
        );
        assert_eq!(get_ext_ret, CM_RESULT_SUCCESS);

        // 6. Test statistics option query
        let mut stats = SEncoderStatistics::default();
        let get_stats_ret = (*p_encoder).GetOption(
            ENCODER_OPTION::ENCODER_OPTION_GET_STATISTICS,
            &mut stats as *mut SEncoderStatistics as *mut std::ffi::c_void,
        );
        assert_eq!(get_stats_ret, CM_RESULT_SUCCESS);

        let uninit_ret = (*p_encoder).Uninitialize();
        assert_eq!(uninit_ret, CM_RESULT_SUCCESS);

        WelsDestroySVCEncoder(p_encoder);
    }
}

/// Every `ENCODER_OPTION_*` value `CWelsH264SVCEncoder` handles, driven through
/// `SetOption`/`GetOption` and compared against the C++ reference.
///
/// The expectations are **measured**, not derived: a probe linked against
/// `libopenh264.a` called the same sequence on the same 160x96 configuration and
/// printed each return code and the fields it wrote. See
/// `rust/docs/encoder_port_status.md`, Phase 5.2.
///
/// Before this test the port handled 12 of C++'s 32 `SetOption` cases and ended
/// its match with `_ => {}` followed by `return 0`, so the other 20 were accepted
/// and silently ignored.
#[test]
fn test_set_get_option_matches_cxx_for_every_option() {
    use openh264_rs::encoder::ref_list_mgr_svc::{SLTRMarkingFeedback, SLTRRecoverRequest};
    use openh264_rs::encoder::wels_encoder_ext::{
        SDeliveryStatus, SLTRConfig, SLevelInfo, SProfileInfo,
    };

    unsafe {
        let mut p_encoder: *mut ISVCEncoder = std::ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut p_encoder), CM_RESULT_SUCCESS);

        let mut param = SEncParamBase::default();
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.iPicWidth = 160;
        param.iPicHeight = 96;
        param.iTargetBitrate = 500000;
        param.iRCMode = RC_MODES::RC_QUALITY_MODE;
        param.fMaxFrameRate = 6.0;
        assert_eq!(
            (*p_encoder).Initialize(&param as *const SEncParamBase),
            CM_RESULT_SUCCESS
        );

        // ---- GetOption: which ids the reference answers at all ----------------
        // C++ `GetOption` has 11 cases and a `default: return cmInitParaError`.
        // Measured return code for every id 0..=31.
        const GET_EXPECTED: [i32; 32] = [
            0, 0, 0, 0, 0, 0, 0, 0, // DATAFORMAT..INTER_SPATIAL_PRED
            1, 1, 1, 1, 1, 1, 1, 1, // RC_MODE..LTR_RECOVERY_REQUEST
            1, 1, 1, 0, 1, 1, 1, 1, // LTR_MARKING_FEEDBACK..CURRENT_PATH
            1, 1, 1, 1, 0, 0, 1, 1, // DUMP_FILE..BITS_VARY_PERCENTAGE
        ];
        let mut buf = [0u8; 8192];
        for id in 0..32i32 {
            buf.fill(0);
            if id == ENCODER_OPTION::ENCODER_OPTION_BITRATE as i32
                || id == ENCODER_OPTION::ENCODER_OPTION_MAX_BITRATE as i32
            {
                // A layer id outside SPATIAL_LAYER_* is a legitimate error in C++.
                let bi = buf.as_mut_ptr() as *mut SBitrateInfo;
                (*bi).iLayer = LAYER_NUM::SPATIAL_LAYER_ALL;
            }
            let e: ENCODER_OPTION = std::mem::transmute(id);
            let got = (*p_encoder).GetOption(e, buf.as_mut_ptr() as *mut std::ffi::c_void);
            assert_eq!(
                got, GET_EXPECTED[id as usize],
                "GetOption(id={id}) returned {got}, C++ returns {}",
                GET_EXPECTED[id as usize]
            );
        }

        // ---- SetOption: the 20 options the port used to swallow ---------------
        let set = |e: ENCODER_OPTION, p: *mut std::ffi::c_void| (*p_encoder).SetOption(e, p);
        macro_rules! setopt {
            ($id:ident, $val:expr) => {{
                let mut v = $val;
                set(
                    ENCODER_OPTION::$id,
                    &mut v as *mut _ as *mut std::ffi::c_void,
                )
            }};
        }

        assert_eq!(setopt!(ENCODER_OPTION_INTER_SPATIAL_PRED, 1i32), 0);
        assert_eq!(setopt!(ENCODER_OPTION_RC_MODE, 1i32), 0);
        assert_eq!(setopt!(ENCODER_OPTION_RC_FRAME_SKIP, true), 0);
        assert_eq!(setopt!(ENCODER_PADDING_PADDING, 1i32), 0);
        assert_eq!(setopt!(ENCODER_LTR_MARKING_PERIOD, 30u32), 0);
        assert_eq!(
            setopt!(
                ENCODER_OPTION_LTR,
                SLTRConfig {
                    bEnableLongTermReference: true,
                    iLTRRefNum: 2,
                }
            ),
            0
        );
        assert_eq!(setopt!(ENCODER_OPTION_ENABLE_SSEI, true), 0);
        assert_eq!(setopt!(ENCODER_OPTION_ENABLE_PREFIX_NAL_ADDING, true), 0);
        assert_eq!(setopt!(ENCODER_OPTION_SPS_PPS_ID_STRATEGY, 1i32), 0);
        // Out of range: C++ logs, leaves eNewStrategy at CONSTANT_ID, and still
        // applies it. It is *not* an error.
        assert_eq!(setopt!(ENCODER_OPTION_SPS_PPS_ID_STRATEGY, 99i32), 0);
        assert_eq!(
            setopt!(
                ENCODER_OPTION_PROFILE,
                SProfileInfo {
                    iLayer: LAYER_NUM::SPATIAL_LAYER_0 as i32,
                    uiProfileIdc: EProfileIdc::PRO_HIGH,
                }
            ),
            0
        );
        // iLayer outside SPATIAL_LAYER_0..SPATIAL_LAYER_3 is rejected.
        assert_eq!(
            setopt!(
                ENCODER_OPTION_PROFILE,
                SProfileInfo {
                    iLayer: 7,
                    uiProfileIdc: EProfileIdc::PRO_HIGH,
                }
            ),
            CM_INIT_PARA_ERROR
        );
        assert_eq!(
            setopt!(
                ENCODER_OPTION_LEVEL,
                SLevelInfo {
                    iLayer: LAYER_NUM::SPATIAL_LAYER_0 as i32,
                    uiLevelIdc: ELevelIdc::LEVEL_3_0,
                }
            ),
            0
        );
        assert_eq!(setopt!(ENCODER_OPTION_NUMBER_REF, 3i32), 0);
        assert_eq!(
            setopt!(
                ENCODER_OPTION_DELIVERY_STATUS,
                SDeliveryStatus {
                    bDeliveryFlag: true,
                }
            ),
            0
        );
        assert_eq!(setopt!(ENCODER_OPTION_STATISTICS_LOG_INTERVAL, 500i32), 0);
        assert_eq!(setopt!(ENCODER_OPTION_IS_LOSSLESS_LINK, true), 0);
        assert_eq!(setopt!(ENCODER_OPTION_BITS_VARY_PERCENTAGE, 20i32), 0);
        assert_eq!(setopt!(ENCODER_OPTION_GET_STATISTICS, 0i32), 0);
        buf.fill(0);
        assert_eq!(
            set(
                ENCODER_OPTION::ENCODER_OPTION_DUMP_FILE,
                buf.as_mut_ptr() as *mut std::ffi::c_void
            ),
            0
        );
        let mut path = *b"/tmp\0";
        assert_eq!(
            set(
                ENCODER_OPTION::ENCODER_OPTION_CURRENT_PATH,
                path.as_mut_ptr() as *mut std::ffi::c_void
            ),
            0
        );
        // The two LTR feedback filters take a struct, not a scalar.
        let mut rec = SLTRRecoverRequest::default();
        assert_eq!(
            set(
                ENCODER_OPTION::ENCODER_LTR_RECOVERY_REQUEST,
                &mut rec as *mut _ as *mut std::ffi::c_void
            ),
            0
        );
        let mut fb = SLTRMarkingFeedback::default();
        assert_eq!(
            set(
                ENCODER_OPTION::ENCODER_LTR_MARKING_FEEDBACK,
                &mut fb as *mut _ as *mut std::ffi::c_void
            ),
            0
        );

        // C++'s `default: return cmInitParaError` has no testable counterpart:
        // `SetOption` takes a typed `ENCODER_OPTION`, so an out-of-range id is
        // not constructible. The port instead matches all 32 variants with no
        // wildcard arm, which makes "an option was added and not handled" a
        // compile error rather than a silent success.

        // ---- read back what those options wrote ------------------------------
        let mut ext = SEncParamExt::default();
        assert_eq!(
            (*p_encoder).GetOption(
                ENCODER_OPTION::ENCODER_OPTION_SVC_ENCODE_PARAM_EXT,
                &mut ext as *mut SEncParamExt as *mut std::ffi::c_void,
            ),
            CM_RESULT_SUCCESS
        );
        assert_eq!(ext.iRCMode, RC_MODES::RC_BITRATE_MODE, "iRCMode");
        assert!(ext.bEnableFrameSkip, "bEnableFrameSkip");
        assert_eq!(ext.iPaddingFlag, 1, "iPaddingFlag");
        assert_eq!(ext.iLtrMarkPeriod, 30, "iLtrMarkPeriod");
        assert!(ext.bEnableLongTermReference, "bEnableLongTermReference");
        assert_eq!(ext.iLTRRefNum, 2, "iLTRRefNum");
        assert!(ext.bEnableSSEI, "bEnableSSEI");
        assert!(ext.bPrefixNalAddingCtrl, "bPrefixNalAddingCtrl");
        assert_eq!(
            ext.eSpsPpsIdStrategy,
            EParameterSetStrategy::CONSTANT_ID,
            "eSpsPpsIdStrategy after the out-of-range set"
        );
        assert!(ext.bIsLosslessLink, "bIsLosslessLink");
        assert_eq!(ext.iNumRefFrame, 3, "iNumRefFrame");
        assert_eq!(
            ext.sSpatialLayers[0].uiProfileIdc,
            EProfileIdc::PRO_HIGH,
            "uiProfileIdc"
        );
        assert_eq!(
            ext.sSpatialLayers[0].uiLevelIdc,
            ELevelIdc::LEVEL_3_0,
            "uiLevelIdc"
        );

        let mut interval = -1i32;
        assert_eq!(
            (*p_encoder).GetOption(
                ENCODER_OPTION::ENCODER_OPTION_STATISTICS_LOG_INTERVAL,
                &mut interval as *mut i32 as *mut std::ffi::c_void,
            ),
            CM_RESULT_SUCCESS
        );
        assert_eq!(interval, 500, "iStatisticsLogInterval");

        assert_eq!((*p_encoder).Uninitialize(), CM_RESULT_SUCCESS);
        WelsDestroySVCEncoder(p_encoder);
    }
}
