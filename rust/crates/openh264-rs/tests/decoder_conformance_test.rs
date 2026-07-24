//! Integration tests for decoder capabilities and bitstream decoding conformance.
//! Ported from `test/api/decoder_test.cpp`.

use openh264_rs::api::codec_api::*;

#[derive(Default)]
struct Sha1Hasher {
    state: [u32; 5],
    buffer: Vec<u8>,
    count: u64,
}

impl Sha1Hasher {
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            buffer: Vec::new(),
            count: 0,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.count += data.len() as u64;
        self.buffer.extend_from_slice(data);
        while self.buffer.len() >= 64 {
            let block: [u8; 64] = self.buffer.drain(..64).collect::<Vec<_>>().try_into().unwrap();
            self.process_block(&block);
        }
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }

    pub fn digest(mut self) -> String {
        let bit_len = self.count * 8;
        self.buffer.push(0x80);
        while (self.buffer.len() % 64) != 56 {
            self.buffer.push(0x00);
        }
        self.buffer.extend_from_slice(&bit_len.to_be_bytes());
        while self.buffer.len() >= 64 {
            let block: [u8; 64] = self.buffer.drain(..64).collect::<Vec<_>>().try_into().unwrap();
            self.process_block(&block);
        }

        format!(
            "{:08x}{:08x}{:08x}{:08x}{:08x}",
            self.state[0], self.state[1], self.state[2], self.state[3], self.state[4]
        )
    }
}

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

fn split_annexb_units(bitstream: &[u8]) -> Vec<&[u8]> {
    let mut start_indices = Vec::new();
    let mut i = 0;
    while i + 3 < bitstream.len() {
        if bitstream[i] == 0 && bitstream[i + 1] == 0 && bitstream[i + 2] == 0 && bitstream[i + 3] == 1 {
            start_indices.push(i);
            i += 4;
        } else if bitstream[i] == 0 && bitstream[i + 1] == 0 && bitstream[i + 2] == 1 {
            start_indices.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }

    let mut units = Vec::new();
    for idx in 0..start_indices.len() {
        let start = start_indices[idx];
        let end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1]
        } else {
            bitstream.len()
        };
        units.push(&bitstream[start..end]);
    }
    units
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

struct FileParam {
    file_name: &'static str,
    hash_str: &'static str,
}

static K_FILE_PARAM_ARRAY: &[FileParam] = &[
    FileParam { file_name: "res/BA_MW_D.264", hash_str: "afd7a9765961ca241bb4bdf344b31397bec7465a" },
    FileParam { file_name: "res/Adobe_PDF_sample_a_1024x768_50Frms.264", hash_str: "9aa9a4d9598eb3e1093311826844f37c43e4c521" },
    FileParam { file_name: "res/BA1_FT_C.264", hash_str: "418d152fb85709b6f172799dcb239038df437cfa" },
    FileParam { file_name: "res/BA1_Sony_D.jsv", hash_str: "d94b5ceed5686a03ea682b53d415dee999d27eb6" },
    FileParam { file_name: "res/BAMQ1_JVC_C.264", hash_str: "613cf662c23e5d9e1d7da7fe880a3c427411d171" },
    FileParam { file_name: "res/BAMQ2_JVC_C.264", hash_str: "11bcf3713f520e606a8326d37e00e5fd6c9fd4a0" },
    FileParam { file_name: "res/BANM_MW_D.264", hash_str: "92d924a857a1a7d7d9b224eaa3887830f15dee7f" },
    FileParam { file_name: "res/BASQP1_Sony_C.jsv", hash_str: "3986c8c9d2876d2f0748b925101b152c6ec8b811" },
    FileParam { file_name: "res/CI1_FT_B.264", hash_str: "cbfec15e17a504678b19a1191992131c92a1ac26" },
    FileParam { file_name: "res/CI_MW_D.264", hash_str: "289f29a103c8d95adf2909c646466904be8b06d7" },
    FileParam { file_name: "res/CVFC1_Sony_C.jsv", hash_str: "4641abd7419a5580b97f16e83fd1d566339229d0" },
    FileParam { file_name: "res/CVPCMNL1_SVA_C.264", hash_str: "c2b0d964de727c64b9fccb58f63b567c82bda95a" },
    FileParam { file_name: "res/LS_SVA_D.264", hash_str: "72118f4d1674cf14e58bed7e67cb3aeed3df62b9" },
    FileParam { file_name: "res/MIDR_MW_D.264", hash_str: "9467030f4786f75644bf06a7fc809c36d1959827" },
    FileParam { file_name: "res/MPS_MW_A.264", hash_str: "67f1cfbef0e8025ed60dedccf8d9558d0636be5f" },
    FileParam { file_name: "res/MR1_BT_A.h264", hash_str: "6e585f8359667a16b03e5f49a06f5ceae8d991e0" },
    FileParam { file_name: "res/MR1_MW_A.264", hash_str: "d9e2bf34e9314dcc171ddaea2c5015d0421479f2" },
    FileParam { file_name: "res/MR2_MW_A.264", hash_str: "628b1d4eff04c2d277f7144e23484957dad63cbe" },
    FileParam { file_name: "res/MR2_TANDBERG_E.264", hash_str: "74d618bc7d9d41998edf4c85d51aa06111db6609" },
    FileParam { file_name: "res/NL1_Sony_D.jsv", hash_str: "e401e30669938443c2f02522fd4d5aa1382931a0" },
    FileParam { file_name: "res/NLMQ1_JVC_C.264", hash_str: "f3265c6ddf8db1b2bf604d8a2954f75532e28cda" },
    FileParam { file_name: "res/NLMQ2_JVC_C.264", hash_str: "350ae86ef9ba09390d63a09b7f9ff54184109ca8" },
    FileParam { file_name: "res/NRF_MW_E.264", hash_str: "20732198c04cd2591350a361e4510892f6eed3f0" },
    FileParam { file_name: "res/QCIF_2P_I_allIPCM.264", hash_str: "8724c0866ebdba7ebb7209a0c0c3ae3ae38a0240" },
    FileParam { file_name: "res/SVA_BA1_B.264", hash_str: "c4543b24823b16c424c673616c36c7f537089b2d" },
    FileParam { file_name: "res/SVA_BA2_D.264", hash_str: "98ff2d67860462d8d8bcc9352097c06cc401d97e" },
    FileParam { file_name: "res/SVA_Base_B.264", hash_str: "91f514d81cd33de9f6fbf5dbefdb189cc2e7ecf4" },
    FileParam { file_name: "res/SVA_CL1_E.264", hash_str: "4fe09ab6cdc965ea10a20f1d6dd38aca954412bb" },
    FileParam { file_name: "res/SVA_FM1_E.264", hash_str: "fad08c4ff7cf2307b6579853d0f4652fc26645d3" },
    FileParam { file_name: "res/SVA_NL1_B.264", hash_str: "6d63f72a0c0d833b1db0ba438afff3b4180fb3e6" },
    FileParam { file_name: "res/SVA_NL2_E.264", hash_str: "70453ef8097c94dd190d6d2d1d5cb83c67e66238" },
    FileParam { file_name: "res/SarVui.264", hash_str: "98ff2d67860462d8d8bcc9352097c06cc401d97e" },
    FileParam { file_name: "res/Static.264", hash_str: "91dd4a7a796805b2cd015cae8fd630d96c663f42" },
    FileParam { file_name: "res/Zhling_1280x720.264", hash_str: "ad99f5eaa2d73ae3840e7da67313de8cfc866ce6" },
    FileParam { file_name: "res/sps_subsetsps_bothVUI.264", hash_str: "d3a47032eb5dcc1963343a68e9bea12435bf1e4c" },
    FileParam { file_name: "res/test_cif_I_CABAC_PCM.264", hash_str: "95fdf21470d3bbcf95505abb2164042063a79d98" },
    FileParam { file_name: "res/test_cif_I_CABAC_slice.264", hash_str: "19121bc67f2b13fb8f030504fc0827e1ac6d0fdb" },
    FileParam { file_name: "res/test_cif_P_CABAC_slice.264", hash_str: "521bbd0ba2422369b724c7054545cf107a56f959" },
    FileParam { file_name: "res/test_qcif_cabac.264", hash_str: "587d1d05943f3cd416bf69469975fdee05361e69" },
    FileParam { file_name: "res/test_scalinglist_jm.264", hash_str: "992a25b4ec98db4a16d61c097e614eb16afe3478" },
    FileParam { file_name: "res/test_vd_1d.264", hash_str: "5827d2338b79ff82cd091c707823e466197281d3" },
    FileParam { file_name: "res/test_vd_rc.264", hash_str: "eea02e97bfec89d0418593a8abaaf55d02eaa1ca" },
    FileParam { file_name: "res/Cisco_Men_whisper_640x320_CABAC_Bframe_9.264", hash_str: "931ba1caf075e7b47445c1f4410ade77a46048f6" },
    FileParam { file_name: "res/Cisco_Men_whisper_640x320_CAVLC_Bframe_9.264", hash_str: "9819c0345abdd4faedbaf8f8c4dadb7749515e4d" },
    FileParam { file_name: "res/Cisco_Adobe_PDF_sample_a_1024x768_CAVLC_Bframe_9.264", hash_str: "9d758d9e6f4dead0d7b361f3ddf2ee009d0ea190" },
    FileParam { file_name: "res/VID_1280x544_cabac_temporal_direct.264", hash_str: "b7f04399f38a90c866f0b518d1dd93c823d5d91f" },
    FileParam { file_name: "res/VID_1280x720_cabac_temporal_direct.264", hash_str: "dabc1d0d44921a5c72ed2d4fde1d602465249c97" },
    FileParam { file_name: "res/VID_1920x1080_cabac_temporal_direct.264", hash_str: "6e719adb650cee4ca99a45242685d261257c04cc" },
    FileParam { file_name: "res/VID_1280x544_cavlc_temporal_direct.264", hash_str: "33bfa44b4a3c87fe28354cace1d4b99a03d2967d" },
    FileParam { file_name: "res/VID_1280x720_cavlc_temporal_direct.264", hash_str: "4face6b5d73a378b6e564a831b49311c230158e4" },
    FileParam { file_name: "res/VID_1920x1080_cavlc_temporal_direct.264", hash_str: "b35dc99604ea2a1fda5b84d1b9098cb7565dec8f" },
];

#[test]
fn test_decoder_conformance_bitstream_assets_hash_validation() {
    let repo_root = std::path::Path::new("../../");
    for param in K_FILE_PARAM_ARRAY {
        let file_path = repo_root.join(param.file_name);
        if file_path.exists() {
            let data = std::fs::read(&file_path).expect("Failed to read bitstream asset");
            assert!(!data.is_empty(), "Bitstream asset {} is empty", param.file_name);

            unsafe {
                let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
                let ret = WelsCreateDecoder(&mut p_decoder);
                if ret != CM_RESULT_SUCCESS as i64 || p_decoder.is_null() {
                    continue;
                }

                let mut dec_param = SDecodingParam::default();
                dec_param.uiTargetDqLayer = u8::MAX;
                dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
                dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

                if (*p_decoder).Initialize(&dec_param as *const SDecodingParam) != CM_RESULT_SUCCESS as i64 {
                    WelsDestroyDecoder(p_decoder);
                    continue;
                }

                let mut hasher = Sha1Hasher::new();
                let units = split_annexb_units(&data);

                for unit in units {
                    let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
                    let mut buf_info = SBufferInfo::default();
                    let dec_ret = (*p_decoder).DecodeFrame2(
                        unit.as_ptr(),
                        unit.len() as i32,
                        p_dst.as_mut_ptr(),
                        &mut buf_info,
                    );
                    if dec_ret == DECODING_STATE::dsErrorFree {
                        update_hash_from_frame(&mut hasher, p_dst, &buf_info);
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
                    update_hash_from_frame(&mut hasher, p_dst, &buf_info);
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
                        update_hash_from_frame(&mut hasher, p_dst, &buf_info);
                    }
                }

                let calculated_hash = hasher.digest();
                // Validate calculated SHA-1 against golden expected hash
                assert_eq!(
                    calculated_hash, param.hash_str,
                    "SHA-1 hash mismatch for bitstream asset {}",
                    param.file_name
                );

                (*p_decoder).Uninitialize();
                WelsDestroyDecoder(p_decoder);
            }
        }
    }
}
