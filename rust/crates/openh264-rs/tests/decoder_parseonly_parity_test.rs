//! `ISVCDecoder::DecodeParser` is a different entry point from `DecodeFrame2`, with
//! a different output: an annex-B bitstream the caller can feed to another decoder,
//! not planes.
//!
//! The flow driven here: annex-B split, `bParseOnly = true`,
//! `ERROR_CON_SLICE_COPY`, one NAL per call, then the trailing
//! `DecodeParser(NULL, 0)` that means end of stream on this slot. Each row is one
//! call and pins the return code, the NAL count, the per-NAL lengths, the SPS
//! dimensions, both timestamps, and a SHA-1 over the composed bytes.
//!
//! Behaviour the rows record:
//!
//! * Output lags by one call: an access unit closes when the parser meets the first
//!   NAL of the next one, so a frame's bytes appear on the call after its last
//!   slice, and the first three or four calls of every asset emit nothing.
//! * `in=0` on every emitting call: the copy-out is a single `memcpy` of a struct
//!   whose `uiInBsTimeStamp` nothing writes (`welsDecoderExt.cpp:1239`), so a
//!   completed frame overwrites the caller's input timestamp with zero.
//! * An IDR emits three NALs, not one: the active SPS and PPS are written in front
//!   of the slice out of the parse-only caches, whether or not the source stream
//!   repeated them. The prepend makes the output independently decodable, and is
//!   `sSpsBsInfo`'s only reader.
//! * Parse-only forces `eEcActiveIdc = ERROR_CON_DISABLE`
//!   (`welsDecoderExt.cpp:1217`) on every call, so a damaged access unit is dropped
//!   rather than concealed and the `rv=` column carries the error codes.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// The assets, chosen for what they make the parse-only path do: CAVLC with four
/// IDRs (so the SPS/PPS prepend runs four times), CABAC with B-frames, all-IPCM, a
/// tiny grid, two slices per picture (`fmo_2groups_64x64`, whose access units carry
/// more than one VCL NAL), a stream carrying both an SPS and a subset SPS, and
/// `Error_I_P` — a damaged stream decoded with error concealment disabled, carrying
/// three different SPSs (ids 0/1/2 — 352x288, 640x480, 352x288), so every
/// access-unit boundary leans on `pActiveLayerSps`.
const ASSETS: &[&str] = &[
    "BA_MW_D",
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9",
    "QCIF_2P_I_allIPCM",
    "grid_48x32",
    "fmo_2groups_64x64",
    "sps_subsetsps_bothVUI",
    "Error_I_P",
];

/// One `PARSE` row: the fields the golden pins, formatted so a mismatch prints as a
/// line diff.
fn row(call: usize, rv: i32, info: &SParserBsInfo, sha: &str) -> String {
    let mut lens = String::new();
    for i in 0..info.iNalNum {
        if i > 0 {
            lens.push(',');
        }
        // Safety: `iNalNum > 0` means the decoder filled the descriptor and
        // `pNalLenInByte` names `iNalNum` of its own `Vec`'s elements.
        let v = unsafe { *info.pNalLenInByte.add(i as usize) };
        lens.push_str(&v.to_string());
    }
    format!(
        "PARSE {} rv=0x{:x} nal={} lens=[{}] sps={}x{} in={} out={} sha1={}",
        call,
        rv,
        info.iNalNum,
        lens,
        info.iSpsWidthInPixel,
        info.iSpsHeightInPixel,
        info.uiInBsTimeStamp,
        info.uiOutBsTimeStamp,
        sha
    )
}

/// Drives one asset through `DecodeParser`, one NAL per call.
///
/// # Safety
/// Every pointer is valid for its call, and the two the decoder hands back are read
/// before the next call — the window `codec_api.h` promises.
unsafe fn parseonly_rows(data: &[u8]) -> Vec<String> {
    unsafe {
        let mut dec: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(WelsCreateDecoder(&mut dec), CM_RESULT_SUCCESS as i64);
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.bParseOnly = true;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            ISVCDecoder::Initialize(dec, &param as *const SDecodingParam),
            CM_RESULT_SUCCESS as i64
        );

        let mut out = Vec::new();
        let mut info = SParserBsInfo::default();
        let mut all = Sha1Hasher::new();
        let mut call = 0usize;
        let mut emitted = 0usize;

        let one = |dec: *mut ISVCDecoder,
                   buf: *const u8,
                   len: i32,
                   info: &mut SParserBsInfo,
                   out: &mut Vec<String>,
                   all: &mut Sha1Hasher,
                   call: &mut usize,
                   emitted: &mut usize| {
            info.uiInBsTimeStamp = *call as u64 + 1;
            let rv = ISVCDecoder::DecodeParser(dec, buf, len, info).0;
            let mut total = 0i64;
            for i in 0..info.iNalNum {
                total += i64::from(*info.pNalLenInByte.add(i as usize));
            }
            let sha = if info.iNalNum > 0 && !info.pDstBuff.is_null() {
                let bytes = std::slice::from_raw_parts(info.pDstBuff, total.max(0) as usize);
                let mut h = Sha1Hasher::new();
                h.update(bytes);
                all.update(bytes);
                *emitted += 1;
                h.digest()
            } else {
                "-".to_string()
            };
            out.push(row(*call, rv, info, &sha));
            *call += 1;
        };

        for unit in split_annexb_units(data) {
            one(
                dec,
                unit.as_ptr(),
                unit.len() as i32,
                &mut info,
                &mut out,
                &mut all,
                &mut call,
                &mut emitted,
            );
        }
        one(
            dec,
            std::ptr::null(),
            0,
            &mut info,
            &mut out,
            &mut all,
            &mut call,
            &mut emitted,
        );

        out.push(format!(
            "PARSEONLY {} {} {}",
            call,
            emitted,
            if emitted > 0 {
                all.digest()
            } else {
                "-".to_string()
            }
        ));

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
        out
    }
}

#[test]
fn decode_parser_matches_the_reference_on_every_asset() {
    assert!(
        ASSETS.contains(&"Error_I_P"),
        "Error_I_P must stay in the asset list; the comparison below depends on it"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let goldens =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/decoder_parseonly");
    let mut failures = Vec::new();
    for asset in ASSETS {
        let data = std::fs::read(root.join("res").join(format!("{asset}.264")))
            .unwrap_or_else(|e| panic!("cannot read res/{asset}.264: {e}"));
        let golden = std::fs::read_to_string(goldens.join(format!("{asset}.txt")))
            .unwrap_or_else(|e| panic!("cannot read the golden for {asset}: {e}"));
        let want: Vec<&str> = golden.lines().filter(|l| !l.is_empty()).collect();
        let got = unsafe { parseonly_rows(&data) };

        // Row count first: emitting nothing and emitting wrong bytes are
        // different defects.
        if got.len() != want.len() {
            failures.push(format!(
                "{asset}: {} rows, the reference has {}",
                got.len(),
                want.len()
            ));
            continue;
        }
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            if g != *w {
                failures.push(format!("{asset} row {i}:\n  ref:  {w}\n  port: {g}"));
                // One row per asset by default; `PARSEONLY_ALL=1` prints every
                // diverging row.
                if std::env::var("PARSEONLY_ALL").is_err() {
                    break;
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "DecodeParser diverges from the C++ reference:\n{}",
        failures.join("\n")
    );
}
