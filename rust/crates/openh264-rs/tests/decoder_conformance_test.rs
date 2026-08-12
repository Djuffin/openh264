mod common;
use common::Sha1Hasher;
use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

fn update_hash_from_plane(
    hasher: &mut Sha1Hasher,
    plane: *const u8,
    width: usize,
    height: usize,
    stride: usize,
) {
    if plane.is_null() || width == 0 || height == 0 || stride == 0 {
        return;
    }
    unsafe {
        for y in 0..height {
            let row = std::slice::from_raw_parts(plane.add(y * stride), width);
            hasher.update(row);
        }
    }
}

fn update_hash_from_frame(hasher: &mut Sha1Hasher, data: [*mut u8; 3], buf_info: &SBufferInfo) {
    if buf_info.iBufferStatus == 1 {
        unsafe {
            let width = buf_info.UsrData.sSystemBuffer.iWidth as usize;
            let height = buf_info.UsrData.sSystemBuffer.iHeight as usize;
            let stride_y = buf_info.UsrData.sSystemBuffer.iStride[0] as usize;
            let stride_uv = buf_info.UsrData.sSystemBuffer.iStride[1] as usize;

            update_hash_from_plane(hasher, data[0], width, height, stride_y);
            update_hash_from_plane(hasher, data[1], width / 2, height / 2, stride_uv);
            update_hash_from_plane(hasher, data[2], width / 2, height / 2, stride_uv);
        }
    }
}

#[test]
fn test_sha1_hasher_sanity() {
    let mut hasher = Sha1Hasher::new();
    hasher.update(b"abc");
    assert_eq!(hasher.digest(), "a9993e364706816aba3e25717850c26c9cd0d89d");
}

#[test]
fn test_decoder_capability_query() {
    unsafe {
        let mut dec_cap = SDecoderCapability::default();
        let ret = WelsGetDecoderCapability(&mut dec_cap);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert_eq!(dec_cap.iProfileIdc, 66);
        assert_eq!(dec_cap.iProfileIop, 0xE0);
        assert_eq!(dec_cap.iLevelIdc, 32);
        assert_eq!(dec_cap.iMaxMbps, 216000);
        assert_eq!(dec_cap.iMaxFs, 5120);
        assert_eq!(dec_cap.iMaxCpb, 20000);
        assert_eq!(dec_cap.iMaxDpb, 20480);
        assert_eq!(dec_cap.iMaxBr, 20000);
        assert_eq!(dec_cap.bRedPicCap, false);
    }
}

/// Decodes one asset and compares the SHA-1 of its output planes against the
/// C++ decoder's.
///
/// `hash_concealed` selects which frames count. The default (`false`) is this
/// file's long-standing rule — only frames returned `dsErrorFree` — and it
/// agrees with `h264dec` on every stream that decodes without concealment,
/// which is all of them above. `true` is `h264dec`'s own rule, every frame it
/// writes out (`iBufferStatus == 1`, whatever the decoding state), and it is
/// what an asset that *deliberately* conceals has to be judged by: under the
/// default rule the concealed frames would drop out of the hash silently, and a
/// stream whose whole purpose is a concealment path would compare only its
/// clean prefix.
fn test_single_bitstream_asset_ex(file_name: &str, expected_hash: &str, hash_concealed: bool) {
    let mut repo_root = std::path::PathBuf::from("../../../");
    if !repo_root.join("res").exists() {
        repo_root = std::path::PathBuf::from("../../");
    }
    let file_path = repo_root.join(file_name);
    assert!(
        file_path.exists(),
        "Bitstream asset file missing: {:?}",
        file_path
    );

    let data = std::fs::read(&file_path).expect("Failed to read bitstream asset");
    assert!(!data.is_empty(), "Bitstream asset {} is empty", file_name);

    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64, "Failed to create decoder for {}", file_name);
        assert!(!p_decoder.is_null());

        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

        let init_ret = (*p_decoder).Initialize(&dec_param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64, "Failed to initialize decoder for {}", file_name);

        let mut hasher = Sha1Hasher::new();
        let units = split_annexb_units(&data);
        let mut decoded_frames = 0;

        for (_idx, unit) in units.iter().enumerate() {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let dec_ret = (*p_decoder).DecodeFrame2(
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            if (hash_concealed || dec_ret == DECODING_STATE::dsErrorFree) && buf_info.iBufferStatus == 1 {
                update_hash_from_frame(&mut hasher, p_dst, &buf_info);
                decoded_frames += 1;
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
        if (hash_concealed || dec_ret == DECODING_STATE::dsErrorFree) && buf_info.iBufferStatus == 1 {
            update_hash_from_frame(&mut hasher, p_dst, &buf_info);
            decoded_frames += 1;
        }

        let mut remaining_frames = 0i32;
        (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining_frames as *mut i32 as *mut std::ffi::c_void,
        );

        for _f in 0..remaining_frames {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let flush_ret = (*p_decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
            if (hash_concealed || flush_ret == DECODING_STATE::dsErrorFree) && buf_info.iBufferStatus == 1 {
                update_hash_from_frame(&mut hasher, p_dst, &buf_info);
                decoded_frames += 1;
            }
        }

        assert!(decoded_frames > 0, "No frames decoded for {}", file_name);
        let calculated_hash = hasher.digest();
        assert_eq!(
            calculated_hash, expected_hash,
            "SHA-1 hash mismatch for bitstream asset {}",
            file_name
        );

        (*p_decoder).Uninitialize();
        WelsDestroyDecoder(p_decoder);
    }
}

macro_rules! asset_test {
    ($test_name:ident, $filename:expr, $hash:expr) => {
        #[test]
        fn $test_name() {
            test_single_bitstream_asset_ex(concat!("res/", $filename), $hash, false);
        }
    };
}

/// As [`asset_test!`], for a stream that conceals: every frame the decoder
/// outputs counts, which is what the C++ golden contains.
macro_rules! asset_test_concealed {
    ($test_name:ident, $filename:expr, $hash:expr) => {
        #[test]
        fn $test_name() {
            test_single_bitstream_asset_ex(concat!("res/", $filename), $hash, true);
        }
    };
}

asset_test!(test_asset_ba_mw_d, "BA_MW_D.264", "afd7a9765961ca241bb4bdf344b31397bec7465a");
asset_test!(test_asset_adobe_pdf_sample_a_1024x768_50frms, "Adobe_PDF_sample_a_1024x768_50Frms.264", "9aa9a4d9598eb3e1093311826844f37c43e4c521");
asset_test!(test_asset_ba1_ft_c, "BA1_FT_C.264", "418d152fb85709b6f172799dcb239038df437cfa");
asset_test!(test_asset_ba1_sony_d, "BA1_Sony_D.jsv", "d94b5ceed5686a03ea682b53d415dee999d27eb6");
asset_test!(test_asset_bamq1_jvc_c, "BAMQ1_JVC_C.264", "613cf662c23e5d9e1d7da7fe880a3c427411d171");
asset_test!(test_asset_bamq2_jvc_c, "BAMQ2_JVC_C.264", "11bcf3713f520e606a8326d37e00e5fd6c9fd4a0");
asset_test!(test_asset_banm_mw_d, "BANM_MW_D.264", "92d924a857a1a7d7d9b224eaa3887830f15dee7f");
asset_test!(test_asset_basqp1_sony_c, "BASQP1_Sony_C.jsv", "3986c8c9d2876d2f0748b925101b152c6ec8b811");
asset_test!(test_asset_ci1_ft_b, "CI1_FT_B.264", "cbfec15e17a504678b19a1191992131c92a1ac26");
asset_test!(test_asset_ci_mw_d, "CI_MW_D.264", "289f29a103c8d95adf2909c646466904be8b06d7");
asset_test!(test_asset_cvfc1_sony_c, "CVFC1_Sony_C.jsv", "4641abd7419a5580b97f16e83fd1d566339229d0");
asset_test!(test_asset_cvpcmnl1_sva_c, "CVPCMNL1_SVA_C.264", "c2b0d964de727c64b9fccb58f63b567c82bda95a");
asset_test!(test_asset_ls_sva_d, "LS_SVA_D.264", "72118f4d1674cf14e58bed7e67cb3aeed3df62b9");
asset_test!(test_asset_midr_mw_d, "MIDR_MW_D.264", "9467030f4786f75644bf06a7fc809c36d1959827");
asset_test!(test_asset_mps_mw_a, "MPS_MW_A.264", "67f1cfbef0e8025ed60dedccf8d9558d0636be5f");
asset_test!(test_asset_mr1_bt_a, "MR1_BT_A.h264", "6e585f8359667a16b03e5f49a06f5ceae8d991e0");
asset_test!(test_asset_mr1_mw_a, "MR1_MW_A.264", "d9e2bf34e9314dcc171ddaea2c5015d0421479f2");
asset_test!(test_asset_mr2_mw_a, "MR2_MW_A.264", "628b1d4eff04c2d277f7144e23484957dad63cbe");
asset_test!(test_asset_mr2_tandberg_e, "MR2_TANDBERG_E.264", "74d618bc7d9d41998edf4c85d51aa06111db6609");
asset_test!(test_asset_nl1_sony_d, "NL1_Sony_D.jsv", "e401e30669938443c2f02522fd4d5aa1382931a0");
asset_test!(test_asset_nlmq1_jvc_c, "NLMQ1_JVC_C.264", "f3265c6ddf8db1b2bf604d8a2954f75532e28cda");
asset_test!(test_asset_nlmq2_jvc_c, "NLMQ2_JVC_C.264", "350ae86ef9ba09390d63a09b7f9ff54184109ca8");
asset_test!(test_asset_nrf_mw_e, "NRF_MW_E.264", "20732198c04cd2591350a361e4510892f6eed3f0");
asset_test!(test_asset_qcif_2p_i_allipcm, "QCIF_2P_I_allIPCM.264", "8724c0866ebdba7ebb7209a0c0c3ae3ae38a0240");
asset_test!(test_asset_sva_ba1_b, "SVA_BA1_B.264", "c4543b24823b16c424c673616c36c7f537089b2d");
asset_test!(test_asset_sva_ba2_d, "SVA_BA2_D.264", "98ff2d67860462d8d8bcc9352097c06cc401d97e");
asset_test!(test_asset_sva_base_b, "SVA_Base_B.264", "91f514d81cd33de9f6fbf5dbefdb189cc2e7ecf4");
asset_test!(test_asset_sva_cl1_e, "SVA_CL1_E.264", "4fe09ab6cdc965ea10a20f1d6dd38aca954412bb");
asset_test!(test_asset_sva_fm1_e, "SVA_FM1_E.264", "fad08c4ff7cf2307b6579853d0f4652fc26645d3");
asset_test!(test_asset_sva_nl1_b, "SVA_NL1_B.264", "6d63f72a0c0d833b1db0ba438afff3b4180fb3e6");
asset_test!(test_asset_sva_nl2_e, "SVA_NL2_E.264", "70453ef8097c94dd190d6d2d1d5cb83c67e66238");
asset_test!(test_asset_sarvui, "SarVui.264", "98ff2d67860462d8d8bcc9352097c06cc401d97e");
asset_test!(test_asset_static, "Static.264", "91dd4a7a796805b2cd015cae8fd630d96c663f42");
asset_test!(test_asset_zhling_1280x720, "Zhling_1280x720.264", "ad99f5eaa2d73ae3840e7da67313de8cfc866ce6");
asset_test!(test_asset_sps_subsetsps_bothvui, "sps_subsetsps_bothVUI.264", "d3a47032eb5dcc1963343a68e9bea12435bf1e4c");
asset_test!(test_asset_test_cif_i_cabac_pcm, "test_cif_I_CABAC_PCM.264", "95fdf21470d3bbcf95505abb2164042063a79d98");
asset_test!(test_asset_test_cif_i_cabac_slice, "test_cif_I_CABAC_slice.264", "19121bc67f2b13fb8f030504fc0827e1ac6d0fdb");
asset_test!(test_asset_test_cif_p_cabac_slice, "test_cif_P_CABAC_slice.264", "521bbd0ba2422369b724c7054545cf107a56f959");
asset_test!(test_asset_test_qcif_cabac, "test_qcif_cabac.264", "587d1d05943f3cd416bf69469975fdee05361e69");
// Hash intentionally differs from the C++ decoder's output (992a25b4...). This stream
// keeps two pictures buffered at end of stream, and upstream's DecodeFrame2 flush path
// gives them the same uiDecodingTimeStamp, so ReleaseBufferedReadyPictureNoReorder falls
// back to slot order and emits POC 8 before POC 6. Both are iSeqNum 1, so POC order is
// display order and 6 must come first; the port breaks the tie by POC instead (see
// ReleaseBufferedReadyPictureNoReorder in src/api/codec_api.rs). The direction of that
// tiebreak is confirmed independently by the JVT gold for CABA2_SVA_B, which upstream
// also fails on the same tie.
asset_test!(test_asset_test_scalinglist_jm, "test_scalinglist_jm.264", "f690a3af2896a53360215fb5d35016bfd41499b3");
asset_test!(test_asset_test_vd_1d, "test_vd_1d.264", "5827d2338b79ff82cd091c707823e466197281d3");
asset_test!(test_asset_test_vd_rc, "test_vd_rc.264", "eea02e97bfec89d0418593a8abaaf55d02eaa1ca");
asset_test!(test_asset_cisco_men_whisper_640x320_cabac_bframe_9, "Cisco_Men_whisper_640x320_CABAC_Bframe_9.264", "931ba1caf075e7b47445c1f4410ade77a46048f6");
asset_test!(test_asset_cisco_men_whisper_640x320_cavlc_bframe_9, "Cisco_Men_whisper_640x320_CAVLC_Bframe_9.264", "9819c0345abdd4faedbaf8f8c4dadb7749515e4d");
asset_test!(test_asset_cisco_adobe_pdf_sample_a_1024x768_cavlc_bframe_9, "Cisco_Adobe_PDF_sample_a_1024x768_CAVLC_Bframe_9.264", "9d758d9e6f4dead0d7b361f3ddf2ee009d0ea190");
asset_test!(test_asset_vid_1280x544_cabac_temporal_direct, "VID_1280x544_cabac_temporal_direct.264", "b7f04399f38a90c866f0b518d1dd93c823d5d91f");
asset_test!(test_asset_vid_1280x720_cabac_temporal_direct, "VID_1280x720_cabac_temporal_direct.264", "dabc1d0d44921a5c72ed2d4fde1d602465249c97");
asset_test!(test_asset_vid_1920x1080_cabac_temporal_direct, "VID_1920x1080_cabac_temporal_direct.264", "6e719adb650cee4ca99a45242685d261257c04cc");
asset_test!(test_asset_vid_1280x544_cavlc_temporal_direct, "VID_1280x544_cavlc_temporal_direct.264", "33bfa44b4a3c87fe28354cace1d4b99a03d2967d");
asset_test!(test_asset_vid_1280x720_cavlc_temporal_direct, "VID_1280x720_cavlc_temporal_direct.264", "4face6b5d73a378b6e564a831b49311c230158e4");
asset_test!(test_asset_vid_1920x1080_cavlc_temporal_direct, "VID_1920x1080_cavlc_temporal_direct.264", "b35dc99604ea2a1fda5b84d1b9098cb7565dec8f");

// ---------------------------------------------------------------------------
// Narrow frames — the F21 trigger class (`phase4b_findings.md`).
//
// `ExpandReferencingPicture` takes a different arm for `iWidth >> 1 < 16`, i.e.
// a frame narrower than 32 luma pixels, and one of the port's three copies of
// that function had no such arm at all. Nothing above pinned it: every prior
// asset here is 176x144 or wider, the diffharness inputs start at 152x100, and
// the malformed corpus inherits the conformance streams' SPS dimensions, so the
// narrow arm was unreachable by construction rather than by luck.
//
// The three streams below are encoded by the C++ encoder from a window panned
// across `CiscoVT2people_320x192_12fps.yuv` — panned so the MVs are non-zero and
// point outside a 16-pixel frame, which is the only way an expanded border
// reaches the output. Goldens are the C++ decoder's, as everywhere in this file.
//
//  * 16x16 — the minimum legal frame width; `iWidthUV` 8, the divergent arm.
//  * 24x18 — coded 32x32 and cropped, so `iWidthUV` is exactly 16: the other
//    side of the same branch, one step away, plus frame cropping.
//  * 16x16 with a lost IDR — reaches `WelsInitRefList`'s error-concealment
//    prefetch, which is the call site the divergent copy served. See the
//    header comment on `test_asset_narrow_16x16_idr_lost` for its construction.
asset_test!(test_asset_narrow_16x16, "narrow_16x16.264", "6299ce8a7dc8a86d367dca65ca123eb499fc5ca8");
asset_test!(test_asset_narrow_24x18, "narrow_24x18.264", "f6197477215d8847b570982d3c2747da2911f047");
// Two 16x16 encodes concatenated, the second one's IDR NAL removed: a CAVLC
// (profile_idc 66) sequence of 24 frames, then a CABAC (profile_idc 100) SPS,
// which differs from the stored one and so begins a new sequence, and then that
// sequence's P slices with no IDR to open them. The new sequence clears the
// reference lists, the first P slice therefore finds them empty, and
// `WelsCheckAndRecoverForFutureDecoding` prefetches a *recycled* picture — one
// still holding the first sequence's samples outside the area it memsets to 128
// — and expands its border. That recycled content is what makes the missing
// chroma arm observable; a stream that concealed from a fresh pool would find
// 128 on both sides of the fix and prove nothing.
asset_test_concealed!(test_asset_narrow_16x16_idr_lost, "narrow_16x16_idr_lost.264", "754db24b395cc7aff338e036a416a9b5bb409c81");
