//! TEMPORARY (session T, F46) — the port's side of one truncation, printed.
//! Mirrors `malformed_stream_parity.rs`'s `decode_case` and `ecref`'s main.
//! Deleted before the face commits.

mod common;

use common::Sha1Hasher;
use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

fn run(name: &str, want: usize) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("res")
        .join(name);
    let mut data = std::fs::read(&path).unwrap();
    if want < data.len() {
        data.truncate(want);
    }
    unsafe {
        let mut decoder: *mut ISVCDecoder = std::ptr::null_mut();
        WelsCreateDecoder(&mut decoder);
        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        (*decoder).Initialize(&dec_param as *const SDecodingParam);

        let mut codes = Vec::new();
        let mut bufs = Vec::new();
        let mut frame_hashes: Vec<String> = Vec::new();
        let mut feed = |unit: &[u8]| {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let src = if unit.is_empty() {
                std::ptr::null()
            } else {
                unit.as_ptr()
            };
            let ret = (*decoder).DecodeFrame2(src, unit.len() as i32, p_dst.as_mut_ptr(), &mut buf_info);
            eprintln!("--- call len={} -> 0x{:x} bufstatus={}", unit.len(), ret.0, buf_info.iBufferStatus);
            codes.push(ret.0);
            bufs.push(buf_info.iBufferStatus);
            if buf_info.iBufferStatus == 1 { frame_hashes.push(hash_frame(&buf_info, p_dst)); }
        };
        for unit in split_annexb_units(&data) {
            feed(unit);
        }
        let mut eos_flag = 1i32;
        (*decoder).SetOption(
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
        );
        feed(&[]);

        let mut remaining = 0i32;
        (*decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        for _ in 0..remaining.clamp(0, 24) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = (*decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
            eprintln!("--- flush -> 0x{:x} bufstatus={}", ret.0, buf_info.iBufferStatus);
            codes.push(ret.0);
            bufs.push(buf_info.iBufferStatus);
            if buf_info.iBufferStatus == 1 { frame_hashes.push(format!("{} (flush)", hash_frame(&buf_info, p_dst))); }
        }
        (*decoder).Uninitialize();
        WelsDestroyDecoder(decoder);
        for (i, h) in frame_hashes.iter().enumerate() {
            eprintln!("PORTFRAME {i} {h}");
        }
        eprintln!(
            "PORT {name} @{want}: codes {} bufs {:?}",
            codes.iter().map(|c| format!("0x{c:x}")).collect::<Vec<_>>().join(","),
            bufs
        );
    }
}

#[test]
fn f46_probe() {
    let spec = std::env::var("F46_CASE").unwrap_or_else(|_| "narrow_16x16.264:41".into());
    let (name, want) = spec.split_once(':').unwrap();
    run(name, want.parse().unwrap());
}

unsafe fn hash_frame(info: &SBufferInfo, dst: [*mut u8; 3]) -> String {
    let sys = info.UsrData.sSystemBuffer;
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
    hasher.digest()
}
