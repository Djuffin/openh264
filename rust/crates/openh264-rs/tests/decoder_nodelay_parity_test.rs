//! **F82's covering test** (Phase 8 session C, T8.C8) — `DecodeFrameNoDelay` is a
//! *different entry point* from `DecodeFrame2`, and it is refereed as one.
//!
//! `welsDecoderExt.cpp:720–725`, the whole of the reference's single-threaded body:
//!
//! ```c
//! iRet  = DecodeFrame2 (kpSrc, kiSrcLen, ppDst, pDstInfo);
//! iRet |= DecodeFrame2 (NULL, 0, ppDst, pDstInfo);
//! ```
//!
//! The second call is what "no delay" *means*: it forces reconstruction so the caller
//! gets its frame on the call that fed the access unit. **The port forwarded once** —
//! T8.B7 named the divergence at the slot and deferred it, which was reasonable while
//! nothing measured what it cost. T8.C5b measured it: **21 of the 81 `test/api`
//! failures** were this one missing statement, every one of them
//! `ASSERT_EQ (dstBufInfo_.iBufferStatus, 1)` reading 0 on a frame the encoder had
//! just produced.
//!
//! # Why this file exists rather than an extra assertion somewhere
//!
//! Every gate this project owns drives `DecodeFrame2`: the conformance 58, the 2919
//! corpus rows, the 504-decode reachability sweep, and the ABI harness. The two tests
//! that do call `DecodeFrameNoDelay` cannot see its behaviour —
//! `api_lifecycle_test.rs` calls it with `(null, 0)`, and `loopback_sha1_test.rs:258`
//! follows every call with **its own explicit `DecodeFrame2(NULL, 0, …)`**, which is
//! by hand exactly the statement that was missing. An entry point with no referee is
//! how this one survived seven phases.
//!
//! The rows below are the **C++ decoder's**, from
//! `rust/tools/ecref/ecref <asset> 99999999 --nodelay` against `libopenh264.dylib`
//! (the `--nodelay` flag is T8.C8's, added for exactly this). The flow here is
//! `ecref`'s statement for statement: annex-B split, `ERROR_CON_SLICE_COPY`, one NAL
//! per call, EOS, a final `DecodeFrameNoDelay(NULL, 0)`, then `FlushFrame` for what
//! `GetOption` reports remaining, capped at 24.
//!
//! # What the rows show, and what must not be "fixed"
//!
//! `Error_I_P.264` emits **one** frame here against `DecodeFrame2`'s five, and
//! `QCIF_2P_I_allIPCM.264` two against a different call pattern. That is not a defect:
//! when the first call emits a picture and the second does not, the second call's
//! `iBufferStatus = 0` overwrites it and the frame is lost to that caller. The
//! reference has the restore written out and **commented out**
//! (`welsDecoderExt.cpp:726–732`), so it is upstream's considered behaviour. A port
//! that repaired it here would diverge from every consumer's expectations, and these
//! rows are what keeps anyone from trying.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// `(asset, frames, first-frame dims, sha1 over every emitted plane, codes, buffer statuses)`
/// — the C++ decoder's answer, via `ecref --nodelay`.
struct Row {
    asset: &'static str,
    frames: usize,
    dims: (i32, i32),
    sha1: &'static str,
    codes: &'static [i32],
    bufs: &'static [i32],
}

/// Diversity over the axes that change emission timing: CAVLC, CABAC with B-frames,
/// a one-macroblock frame, all-IPCM, and the resolution-change stream. Every number
/// is `ecref --nodelay`'s, not the port's.
const ROWS: &[Row] = &[
    Row {
        asset: "BA_MW_D.264",
        frames: 100,
        dims: (176, 144),
        sha1: "afd7a9765961ca241bb4bdf344b31397bec7465a",
        codes: &[0; 103],
        bufs: &[0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    },
    Row {
        asset: "Cisco_Men_whisper_640x320_CABAC_Bframe_9.264",
        frames: 9,
        dims: (640, 320),
        sha1: "931ba1caf075e7b47445c1f4410ade77a46048f6",
        codes: &[0; 13],
        bufs: &[0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1],
    },
    Row {
        asset: "narrow_16x16.264",
        frames: 24,
        dims: (16, 16),
        sha1: "6299ce8a7dc8a86d367dca65ca123eb499fc5ca8",
        codes: &[0; 32],
        bufs: &[0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1],
    },
    Row {
        asset: "QCIF_2P_I_allIPCM.264",
        frames: 2,
        dims: (176, 144),
        sha1: "8724c0866ebdba7ebb7209a0c0c3ae3ae38a0240",
        codes: &[0; 6],
        bufs: &[0, 0, 0, 1, 0, 1],
    },
    Row {
        asset: "Error_I_P.264",
        frames: 1,
        dims: (640, 480),
        sha1: "5c1f742798b2c1061cb83ab2f2cddd7929b2fb4e",
        codes: &[0x0, 0x0, 0x0, 0x20, 0x0, 0x0, 0x20, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x20, 0x20, 0x20, 0x0],
        bufs: &[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    },
];

/// Drives one asset through `DecodeFrameNoDelay`, exactly as `ecref --nodelay` does.
///
/// # Safety
/// Uses the C ABI as a consumer does; every pointer is valid for its call.
unsafe fn nodelay_row(data: &[u8]) -> (usize, (i32, i32), String, Vec<i32>, Vec<i32>) {
    unsafe {
        let mut dec: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut dec)), CM_RESULT_SUCCESS as i64);
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(dec, &param as *const SDecodingParam)),
            CM_RESULT_SUCCESS as i64
        );

        let mut hasher = Sha1Hasher::new();
        let mut frames = 0usize;
        let mut first = (0i32, 0i32);
        let mut codes = Vec::new();
        let mut bufs = Vec::new();

        let mut take = |info: &SBufferInfo, dst: [*mut u8; 3], hasher: &mut Sha1Hasher, frames: &mut usize, first: &mut (i32, i32)| {
            if info.iBufferStatus != 1 {
                return;
            }
            let sys = *info.UsrData.sys();
            let (w, h) = (sys.iWidth as usize, sys.iHeight as usize);
            let (sy, suv) = (sys.iStride[0] as usize, sys.iStride[1] as usize);
            let mut plane = |p: *mut u8, w: usize, h: usize, stride: usize| {
                if p.is_null() || w == 0 || h == 0 || stride == 0 {
                    return;
                }
                for row in 0..h {
                    hasher.update(std::slice::from_raw_parts(p.add(row * stride), w));
                }
            };
            plane(dst[0], w, h, sy);
            plane(dst[1], w / 2, h / 2, suv);
            plane(dst[2], w / 2, h / 2, suv);
            *frames += 1;
            if first.0 == 0 {
                *first = (sys.iWidth, sys.iHeight);
            }
        };

        for unit in split_annexb_units(data) {
            let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let st = ISVCDecoder::DecodeFrameNoDelay(dec, unit.as_ptr(), unit.len() as i32, dst.as_mut_ptr(), &mut info);
            codes.push(st.0);
            bufs.push(info.iBufferStatus);
            take(&info, dst, &mut hasher, &mut frames, &mut first);
        }

        let mut eos = 1i32;
        ISVCDecoder::SetOption(
            dec,
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos as *mut i32 as *mut std::ffi::c_void,
        );
        {
            let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let st = ISVCDecoder::DecodeFrameNoDelay(dec, std::ptr::null(), 0, dst.as_mut_ptr(), &mut info);
            codes.push(st.0);
            bufs.push(info.iBufferStatus);
            take(&info, dst, &mut hasher, &mut frames, &mut first);
        }

        let mut remaining = 0i32;
        ISVCDecoder::GetOption(
            dec,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        for _ in 0..remaining.clamp(0, 24) {
            let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let st = ISVCDecoder::FlushFrame(dec, dst.as_mut_ptr(), &mut info);
            codes.push(st.0);
            bufs.push(info.iBufferStatus);
            take(&info, dst, &mut hasher, &mut frames, &mut first);
        }

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
        (frames, first, hasher.digest(), codes, bufs)
    }
}

#[test]
fn decode_frame_no_delay_matches_the_reference_on_every_axis() {
    let res = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").join("res");
    for row in ROWS {
        let data = std::fs::read(res.join(row.asset))
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", row.asset));
        let (frames, dims, sha1, codes, bufs) = unsafe { nodelay_row(&data) };
        // S13 — counts before hashes: a hash mismatch whose frame count also moved is
        // a different defect from one whose count held.
        assert_eq!(frames, row.frames, "{}: emitted frame count", row.asset);
        assert_eq!(dims, row.dims, "{}: first emitted frame's dimensions", row.asset);
        assert_eq!(codes, row.codes, "{}: DecodeFrameNoDelay return codes, in call order", row.asset);
        assert_eq!(bufs, row.bufs, "{}: iBufferStatus, in call order", row.asset);
        assert_eq!(sha1, row.sha1, "{}: SHA-1 over every emitted plane", row.asset);
    }
}
