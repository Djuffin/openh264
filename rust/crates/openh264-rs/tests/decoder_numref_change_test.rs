//! A mid-stream change of `num_ref_frames` at an unchanged resolution must resize the
//! picture pool in place.
//!
//! `res/num_ref_change_320x192.264` is 24 frames at `iNumRefFrame = 1` followed by 24
//! at `iNumRefFrame = 4`, same 320x192 throughout, built with the reference encoder by
//! `rust/tools/make_numref_asset.cpp`. It is the only stream in `res/` that reaches
//! `WelsRequestMem`'s **third** arm (`decoder.cpp:493-509`): same picture size,
//! different picture-queue size. No other asset does — `ecref --sps` over all 63 shows
//! every multi-SPS stream repeating one `num_ref_frames`, and the only one that
//! changes anything (`Error_I_P.264`) changes the *resolution*, which is the second
//! arm and `decoder_resolution_change_test.rs`'s subject.
//!
//! This exercises `IncreasePicBuff`; the shrinking direction is `DecreasePicBuff`, and
//! no stream here exercises it — `safe/pool.rs`'s
//! `shrink_can_reorder_the_slots_it_keeps` and the generation rows beside it are what
//! stand behind that half.
//!
//! The numbers below are the C++ decoder's, from
//! `rust/tools/ecref/ecref res/num_ref_change_320x192.264 99999999 --frames` against
//! `libopenh264.dylib`.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

const CPP_W: i32 = 320;
const CPP_H: i32 = 192;

/// SHA-1 of each emitted frame's three planes, in emission order — 48 of them.
const CPP_FRAME_HASHES: &[&str] = &[
    "93e32076b63d9c2f4fe40b5004045a5d120533c6",
    "e761a335c239e83ff1ca7745ade7f8e9b93c05cb",
    "277f680ab1718ebde8317f449e70ab9b7b4689ea",
    "49412a4264e690a6e3855112478be0228ef12e2a",
    "db89fdf85a3f9403bfa0107d4b872bc6fc2e6123",
    "f105c33285bda17de3e337d0754c8272952fac2a",
    "d128fc8c80c873a100b02b5a60a19ff31d41a917",
    "6da5d18d91ec65d68709e0fe396fb70941ee9b6c",
    "be07274bd26e134223d30b0cbfef64f3b2d29b7d",
    "517ad7d960b3ac83b538cc9362643ed7f66fd396",
    "a3a6e0ad05a718fe13499a820f881c2266b2652f",
    "00b9c14b82bb2e2df9c53761bc55e7ac76d775ed",
    "96c3c31873f165d57c2e29fec92ec58d041ab9fe",
    "de2890174b90d0156461fe121b92c5d28ec925cd",
    "f26221d97802d5ba8744f515aa6c7e0b2d73f26b",
    "33134ec48acb101895e0c4c70b97c29ef8c19767",
    "98af22e7a8aaeb5209547ccd4ab0637724c19833",
    "6c2930a255d24879f98ea0e4826539eaab571e9a",
    "57f8befea43bcc8ebee1e05d66800249a54557c2",
    "7b1310264a6b825097cb8b5ca97806e826b92187",
    "2836b62c3b7b192b70f6138e4de53a28f77254b1",
    "38e5fabba814bdd7ba007eab95820c443b8cf9cc",
    "6c77128d42913a543d9f244cfb18c3d444947170",
    "38c4d7a22662f597dda46749e19a538e46d690f6",
    "00e9c9790ed58256fcd888233ba3918e0a3d1935",
    "132e117bb1d423ed7d7e7e72ae0c06abb67d2e40",
    "470ed3508d6caaddebb7c4d3180a2e47e2f7c7e2",
    "18a401253be4d8aa3abdf108fff2673dc28b890e",
    "d6ec0f9a9b35fdfc7c7172dfb86ea80ee5f59e6c",
    "969edae48b3349ca61fc365ff7451d02f6e86d6a",
    "f17a4a189d87389d3e76f3fa98ff140192231229",
    "d7d7e1e84ef9b5a3e9f2d9a898b68f717b1e372c",
    "897d6ff7568fac45e03377648cbba4daf8046f3b",
    "e180c328f561bf1857c7549e191b918064f6a678",
    "aa02fa503f1c1e4bcc18721f2af60523f69eb49c",
    "e4636d9ad298e526edddfab64ba0af936d3997fe",
    "ad301f6385e1882bc7db4358af691a4052e8344c",
    "96905bfd162bf04349e4d78418ace394c9d6d25e",
    "0647be93296d1ddbdbe924f0abe580140f86c6ac",
    "3211906e55e744a3b77f744b31c900b238be0f69",
    "1f0d9de64877ea6345eb4336dea4dc7d386a4d53",
    "58bbe0c6d52c740ed7590364fdc21b6002dfa4a3",
    "ed34af98bcc6cda85895d95f425eb9241244b64c",
    "5868fc9debf59d8d42961096dcc3d4d3dd95ea55",
    "afbe916f253cc7a2f3f2b7c420196efee9d7e1a1",
    "967f29d617ababdf9341134c6d5f3bcda0a8c1bf",
    "e1fe2f43a53e981f170c2f84d1308cd157ec84ea",
    "caf315ba45b78938956d81836b913972d2ac3d59",
];

/// `DecodeFrame2`'s return code per call (57 calls, the last the drain).
const CPP_CODES: &[i32] = &[
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
];

/// …and its `iBufferStatus` per call.
const CPP_BUFS: &[i32] = &[
    0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
];

/// SHA-1 over the three planes at the strides `UsrData` reports — the same digest
/// `ecref`, `portref` and the malformed corpus all compute.
///
/// # Safety
/// `dst` must be the plane pointers `DecodeFrame2` just wrote with
/// `iBufferStatus == 1`, valid for the dimensions and strides in `info`.
unsafe fn hash_frame(info: &SBufferInfo, dst: [*mut u8; 3]) -> (i32, i32, String) {
    unsafe {
        let sys = info.UsrData.sys();
        let (w, h) = (sys.iWidth as usize, sys.iHeight as usize);
        let (sy, suv) = (sys.iStride[0] as usize, sys.iStride[1] as usize);
        let mut hasher = Sha1Hasher::new();
        let mut plane = |p: *mut u8, w: usize, h: usize, stride: usize| {
            for row in 0..h {
                hasher.update(std::slice::from_raw_parts(p.add(row * stride), w));
            }
        };
        plane(dst[0], w, h, sy);
        plane(dst[1], w / 2, h / 2, suv);
        plane(dst[2], w / 2, h / 2, suv);
        (sys.iWidth, sys.iHeight, hasher.digest())
    }
}

#[test]
fn num_ref_frame_change_resizes_the_pool_and_matches_the_reference() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("res")
        .join("num_ref_change_320x192.264");
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut codes = Vec::new();
    let mut bufs = Vec::new();
    let mut frames: Vec<(i32, i32, String)> = Vec::new();

    // The same flow `decoder_resolution_change_test.rs` and `ecref` use: annex-B
    // split, `ERROR_CON_SLICE_COPY`, one NAL per call, then the end-of-stream drain.
    unsafe {
        let mut decoder: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut decoder)), CM_RESULT_SUCCESS as i64);
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(decoder, &param as *const SDecodingParam)),
            CM_RESULT_SUCCESS as i64
        );

        let mut feed = |unit: &[u8]| {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let src = if unit.is_empty() { std::ptr::null() } else { unit.as_ptr() };
            let ret =
                ISVCDecoder::DecodeFrame2(decoder, src, unit.len() as i32, p_dst.as_mut_ptr(), &mut buf_info);
            codes.push(ret.0);
            bufs.push(buf_info.iBufferStatus);
            if buf_info.iBufferStatus == 1 {
                frames.push(hash_frame(&buf_info, p_dst));
            }
        };
        for unit in split_annexb_units(&data) {
            feed(unit);
        }
        let mut eos_flag = 1i32;
        ISVCDecoder::SetOption(
            decoder,
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
        );
        feed(&[]);

        ISVCDecoder::Uninitialize(decoder);
        WelsDestroyDecoder(decoder);
    }

    // The frame count first: it is the assertion that fails loudest when the third
    // arm is missing, and reading it before the code sequence says *what* went wrong
    // rather than only where.
    assert_eq!(
        frames.len(),
        CPP_FRAME_HASHES.len(),
        "emitted frame count — the reference decodes every frame across the \
         num_ref_frames change"
    );
    assert_eq!(codes, CPP_CODES, "DecodeFrame2 return codes must match the C++ decoder's");
    assert_eq!(bufs, CPP_BUFS, "iBufferStatus per call must match the C++ decoder's");
    for (i, (got, want)) in frames.iter().zip(CPP_FRAME_HASHES).enumerate() {
        assert_eq!(
            (got.0, got.1, got.2.as_str()),
            (CPP_W, CPP_H, *want),
            "frame {i}: dimensions and plane hash must match the C++ decoder's"
        );
    }
}
