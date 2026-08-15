//! `portref` — the **port's** answer for one malformed-corpus entry.
//!
//! The exact counterpart of `rust/tools/ecref`, which prints the C++ decoder's
//! answer for the same bytes, and it exists for the same reason: when the two
//! disagree, "the port changed" is not evidence either way — the question is what
//! each decoder *does*.
//!
//! Session S built `ecref` and had to reach for the port's side twice through
//! temporary test files; session T needed it on every one of F46's five causes and
//! on face 1. So it is an instrument now rather than a scratch file, and it lives in
//! `examples/` on purpose: `cargo test` does not run examples, so this cannot become
//! a test that asserts nothing while occupying a slot in a ratcheted count.
//!
//! ```text
//! cargo run --example portref -- narrow_16x16.264 41
//! cargo run --example portref -- CABA2_SVA_B.264 2284
//! ```
//!
//! Prints, in `DecodeFrame2`/`FlushFrame` call order: the `DECODING_STATE` and
//! `iBufferStatus` of every call, one SHA-1 per emitted frame **individually**
//! (which is what `ecref`'s single whole-run digest cannot give you — face 1 is
//! settled by comparing the two per-frame lists as multisets), and the same
//! `frames / dims / codes / bufstatus` row shape the golden tables store.
//!
//! Same decode as `tests/malformed_stream_parity.rs`'s `decode_case`: annex-B split,
//! `ERROR_CON_SLICE_COPY`, per-NAL feed, EOS + drain, planes in emission order.

#[path = "../tests/common/mod.rs"]
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
    decode(&format!("{name} @{want}"), &data, false);
}

/// Bytes on stdin, the mirror of `ecref --stdin` (Phase 5 session U, T5.U2).
///
/// A prefix truncation is `(stream, length)`; the `hdr*.*`, `tail.*` and
/// degenerate corpus entries are built inside the harness and no such pair names
/// them. Both referees therefore read a blob, and the harness hands one over via
/// `MALFORMED_DUMP_DIR` — which is what makes a per-frame multiset comparison
/// possible on those rows rather than only on the truncations.
fn run_stdin(raw: bool) {
    use std::io::Read as _;
    let mut data = Vec::new();
    std::io::stdin().read_to_end(&mut data).expect("stdin");
    decode("<stdin>", &data, raw);
}

fn decode(label: &str, data: &[u8], raw: bool) {
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
        if raw {
            feed(data);
        } else {
            for unit in split_annexb_units(data) {
                feed(unit);
            }
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
            "PORT {label}: codes {} bufs {:?}",
            codes.iter().map(|c| format!("0x{c:x}")).collect::<Vec<_>>().join(","),
            bufs
        );
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("--stdin") {
        run_stdin(argv.iter().any(|a| a == "--raw"));
        return;
    }
    let mut args = argv.into_iter();
    let (Some(name), Some(bytes)) = (args.next(), args.next()) else {
        eprintln!("usage: cargo run --example portref -- <stream.264> <truncate-to-bytes>");
        eprintln!("       cargo run --example portref -- --stdin [--raw]");
        std::process::exit(2);
    };
    let want: usize = bytes.parse().unwrap_or_else(|_| {
        eprintln!("not a byte count: {bytes}");
        std::process::exit(2);
    });
    run(&name, want);
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
