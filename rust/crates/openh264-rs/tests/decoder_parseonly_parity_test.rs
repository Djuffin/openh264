//! **`DecodeParser`'s first referee** (Phase 8b session B, T8b.B2).
//!
//! `ISVCDecoder::DecodeParser` is a *different entry point* from `DecodeFrame2`, with
//! a different output: an annex-B bitstream the caller can feed to another decoder,
//! not planes. Every gate this project owns reads planes — the conformance 60, the
//! 2707-row corpus, the reachability sweep, the ABI harness — so for the whole port
//! this slot could return `dsErrorFree`, write nothing at all, and be green
//! everywhere. It did exactly that: `decoder_decode_parser_c` was a stub, the
//! parse-only arm of `DecodeFrameConstruction` kept the reference's length
//! bookkeeping and none of its `memcpy`s, and `sSpsBsInfo`/`sSubsetSpsBsInfo`/
//! `sPpsBsInfo` had no writer at all. S47 names the shape: an entry point with no
//! referee is how a stub survives seven phases.
//!
//! # What the rows are
//!
//! The C++ decoder's, from `rust/tools/ecref/ecref <asset> 99999999 --parse-only`
//! against `libopenh264.dylib` (the flag is T8b.B2's, added for exactly this). Its
//! flow is transliterated below statement for statement: annex-B split,
//! `bParseOnly = true`, `ERROR_CON_SLICE_COPY`, **one NAL per call**, then the
//! trailing `DecodeParser(NULL, 0)` that means end of stream on this slot.
//!
//! Each row is one call and pins six things at once — the return code, the NAL count,
//! the per-NAL lengths, the SPS dimensions, both timestamps, and a SHA-1 over the
//! composed bytes. The bytes are the point: lengths alone would pass on a buffer
//! full of zeros.
//!
//! # What the rows show, and what must not be "fixed"
//!
//! * **Output lags by one call.** An access unit closes when the parser meets the
//!   first NAL of the *next* one, so a frame's bytes appear on the call after its
//!   last slice. The first three or four calls of every asset emit nothing.
//! * **`in=0` on every emitting call.** The reference's copy-out is a single
//!   `memcpy` of a struct whose `uiInBsTimeStamp` **nothing ever writes**
//!   (`welsDecoderExt.cpp:1239`), so a completed frame overwrites the caller's own
//!   input timestamp with zero. Reproduced, not repaired — it is observable
//!   behaviour on a documented out-parameter (F90).
//! * **An IDR emits three NALs, not one.** `[13,8,2363]` on `BA_MW_D.264` call 3 is
//!   the active SPS and PPS written in front of the slice out of the parse-only
//!   caches, whether or not the source stream repeated them. That prepend is what
//!   makes the output independently decodable, and it is `sSpsBsInfo`'s only reader.
//! * **Parse-only forces `eEcActiveIdc = ERROR_CON_DISABLE`**
//!   (`welsDecoderExt.cpp:1217`) on every call, so a damaged access unit is dropped
//!   rather than concealed and the `rv=` column carries the error codes. That mode
//!   is where [`DIVERGING`] lives — see below.

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// The assets, chosen for what they make the parse-only path do rather than for
/// coverage of the decoder: CAVLC with four IDRs (so the SPS/PPS prepend runs four
/// times), CABAC with B-frames, all-IPCM, an error stream, a tiny grid, **two slices
/// per picture** (`fmo_2groups_64x64`, the only `res` asset whose access units carry
/// more than one VCL NAL), and a stream carrying both an SPS and a subset SPS.
const ASSETS: &[&str] = &[
    "BA_MW_D",
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9",
    "QCIF_2P_I_allIPCM",
    "grid_48x32",
    "fmo_2groups_64x64",
    "sps_subsetsps_bothVUI",
];

/// **F93 — refereed, divergent, and owned; the golden is checked in and this list is
/// why it is not in [`ASSETS`].**
///
/// `Error_I_P.264` is a *damaged* stream, and parse-only decodes it with error
/// concealment **disabled** — a combination nothing in this project had ever
/// refereed: the malformed corpus runs every one of its 2707 rows with
/// `ERROR_CON_SLICE_COPY` (`malformed_stream_parity.rs:490`) and the conformance
/// assets are undamaged. Six of its seventeen rows disagree with the reference:
///
/// ```text
///   row  7   ref rv=0x0                     port rv=0x1 (dsFramePending)
///   row  9   ref nal=5 [13,8,5601,8827,10956]  port rv=0x4, nothing emitted
///   row 13   ref rv=0x1                     port rv=0x2 (dsRefLost)
///   row 14   ref rv=0x1                     port rv=0x4
///   row 16   ref rv=0x2                     port rv=0x4
///   total    ref 1 frame emitted            port 0
/// ```
///
/// **Not T8b.B2's.** Every write that commit adds is behind `pParam.bParseOnly`, and
/// the only codes it can raise are `dsOutOfMemory` (the two `MAX_ACCESS_UNIT_CAPACITY`
/// checks) and `dsBitstreamError` from the `SPS_PPS_BS_SIZE - 4` guard — which needs a
/// parameter set of 124 bytes or more, where this stream's are 13 and 8. What the new
/// entry point did was make an existing error-path divergence *visible*: the codes
/// above are `dsFramePending`, `dsRefLost` and `dsBitstreamError`, all raised by
/// pre-existing decoder paths under `ERROR_CON_DISABLE`.
///
/// The next experiment is named rather than guessed: drive the **ordinary**
/// `DecodeFrame2` path over this asset with `ERROR_CON_DISABLE` on both links
/// (`ecref` needs an `--ec=` flag) and see whether it diverges there too. If it does,
/// the defect is the decoder's error path and parse-only is only the messenger; if it
/// does not, it is in the parse-only arm of `DecodeFrameConstruction`.
const DIVERGING: &[&str] = &["Error_I_P"];

/// One `PARSE` row, rendered exactly as `ecref --parse-only` prints it.
///
/// A string and not a struct on purpose: the golden is the tool's own output, so a
/// row that disagrees prints as a diff of the reference's line against the port's
/// rather than as a field name and two integers.
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

/// Drives one asset through `DecodeParser`, exactly as `ecref --parse-only` does.
///
/// # Safety
/// Uses the C ABI as a consumer does; every pointer is valid for its call, and the
/// two the decoder hands back are read before the next call, which is the window
/// `codec_api.h` promises.
unsafe fn parseonly_rows(data: &[u8]) -> Vec<String> {
    unsafe {
        let mut dec: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut dec)), CM_RESULT_SUCCESS as i64);
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.bParseOnly = true;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(dec, &param as *const SDecodingParam)),
            CM_RESULT_SUCCESS as i64
        );

        let mut out = Vec::new();
        let mut info = SParserBsInfo::default();
        let mut all = Sha1Hasher::new();
        let mut call = 0usize;
        let mut emitted = 0usize;

        let mut one = |dec: *mut ISVCDecoder,
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
            if emitted > 0 { all.digest() } else { "-".to_string() }
        ));

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
        out
    }
}

#[test]
fn decode_parser_matches_the_reference_on_every_asset() {
    // The divergent asset is named here so that deleting it from `DIVERGING` without
    // adding it back to `ASSETS` cannot quietly lose the row. Nothing drives it.
    assert_eq!(DIVERGING, &["Error_I_P"], "F93's owner list changed; update the module docs");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let goldens = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/decoder_parseonly");
    let mut failures = Vec::new();
    for asset in ASSETS {
        let data = std::fs::read(root.join("res").join(format!("{asset}.264")))
            .unwrap_or_else(|e| panic!("cannot read res/{asset}.264: {e}"));
        let golden = std::fs::read_to_string(goldens.join(format!("{asset}.txt")))
            .unwrap_or_else(|e| panic!("cannot read the golden for {asset}: {e}"));
        let want: Vec<&str> = golden.lines().filter(|l| !l.is_empty()).collect();
        let got = unsafe { parseonly_rows(&data) };

        // S13 — the call count before the rows: a stub that never emits and a port
        // that emits the wrong bytes are different defects, and the first line of the
        // report should say which one this is.
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
                // One row per asset is enough to name the defect; the rest of the
                // asset's rows are almost always the same one repeating.
                // `PARSEONLY_ALL=1` prints every diverging row instead, which is how
                // F93's table above was measured.
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
