//! A mid-stream resolution change must reallocate the picture pool.
//!
//! `res/Error_I_P.264` is the only stream in `res/` that changes resolution while
//! decoding: 352x288 → 640x480 → 352x288.
//!
//! The numbers below are the C++ decoder's, taken from
//! `rust/tools/ecref/ecref res/Error_I_P.264 61251 --frames` against
//! `libopenh264.dylib` — not from the port.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// The reference's answer, per emitted frame: `(width, height, sha1-of-planes)`.
///
/// Five frames, and the two 640x480 ones are what a decoder that cannot change
/// resolution never reaches.
const CPP_FRAMES: &[(i32, i32, &str)] = &[
    (352, 288, "0f786183de107a429a903cdd838d08268ea34bbe"),
    (352, 288, "477e333c677615c2e3e369793358f327ca4fac61"),
    (640, 480, "5c1f742798b2c1061cb83ab2f2cddd7929b2fb4e"),
    (640, 480, "60270f979f8e2ccd662c05f58d0775271f90a4cd"),
    (352, 288, "b1e052919cf52ecff84fa100eeddaf8c762957c4"),
];

/// The reference's `DecodeFrame2` return code per call, in call order (the last is
/// the end-of-stream drain call). `0x20` is `dsDataErrorConcealed`.
const CPP_CODES: &[i32] = &[
    0x0, 0x0, 0x0, 0x20, 0x0, 0x0, 0x20, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x20, 0x20, 0x0, 0x20,
];

/// …and its `iBufferStatus` per call.
const CPP_BUFS: &[i32] = &[0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0];

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
fn resolution_change_stream_matches_the_reference_and_does_not_abort() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("res")
        .join("Error_I_P.264");
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut codes = Vec::new();
    let mut bufs = Vec::new();
    let mut frames: Vec<(i32, i32, String)> = Vec::new();

    // The same flow `malformed_stream_parity.rs` and `ecref` use: annex-B split,
    // `ERROR_CON_SLICE_COPY`, one NAL per call, then the end-of-stream drain.
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

    assert_eq!(codes, CPP_CODES, "DecodeFrame2 return codes must match the C++ decoder's");
    assert_eq!(bufs, CPP_BUFS, "iBufferStatus per call must match the C++ decoder's");
    assert_eq!(frames.len(), CPP_FRAMES.len(), "emitted frame count");
    for (i, (got, want)) in frames.iter().zip(CPP_FRAMES).enumerate() {
        assert_eq!(
            (got.0, got.1, got.2.as_str()),
            (want.0, want.1, want.2),
            "frame {i}: dimensions and plane hash must match the C++ decoder's"
        );
    }
}
