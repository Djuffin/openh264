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
//!   is what `Error_I_P` referees — see [`ASSETS`].

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

#[path = "common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// The assets, chosen for what they make the parse-only path do rather than for
/// coverage of the decoder: CAVLC with four IDRs (so the SPS/PPS prepend runs four
/// times), CABAC with B-frames, all-IPCM, an error stream, a tiny grid, **two slices
/// per picture** (`fmo_2groups_64x64`, the only `res` asset whose access units carry
/// more than one VCL NAL), a stream carrying both an SPS and a subset SPS — and
/// `Error_I_P`, the damaged stream below.
///
/// **`Error_I_P` — F93, closed by F199 (Phase 9 session J).** A *damaged* stream,
/// and parse-only decodes it with error concealment **disabled** — a combination
/// nothing else referees: the malformed corpus runs every one of its 2707 rows with
/// `ERROR_CON_SLICE_COPY` (`malformed_stream_parity.rs:490`) and the conformance
/// assets are undamaged. It carries **three different SPSs** (ids 0/1/2 —
/// 352x288, 640x480, 352x288), so every access-unit boundary here leans on
/// `pActiveLayerSps`. Four of its seventeen rows diverged until session J: a
/// dropped access unit (EC disabled, refs lost) left `iTotalNumMbRec` nonzero, the
/// port was missing the fresh-picture zeroing at `decoder_core.cpp:2568`, and
/// `ResetActiveSPSForEachLayer` — gated on `iTotalNumMbRec == 0` in both trees —
/// never fired again, splitting the one recoverable IDR access unit in two. One
/// statement restored all four rows and the emitted frame's SHA-1.
///
/// One column that is *expected* to differ and is therefore pinned by the port's
/// own golden, not the reference's: `in=`, the input timestamp. Upstream's
/// `DecodeParser` overwrites the caller's `uiInBsTimeStamp` on its way out
/// (**F90**); the port does not, so the reference reports 0 where the port reports
/// what it was handed. That is a divergence in the port's favour and is recorded as
/// one. (The checked-in golden's `in=` column carries the caller's values, which
/// both implementations now produce on the rows that emit nothing; the emitting row
/// pins `in=0` — F90's overwrite — which the port reproduces at the copy-out.)
const ASSETS: &[&str] = &[
    "BA_MW_D",
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9",
    "QCIF_2P_I_allIPCM",
    "grid_48x32",
    "fmo_2groups_64x64",
    "sps_subsetsps_bothVUI",
    "Error_I_P",
];

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
            if emitted > 0 { all.digest() } else { "-".to_string() }
        ));

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
        out
    }
}

#[test]
fn decode_parser_matches_the_reference_on_every_asset() {
    // F93's `DIVERGING` list is retired: its one asset moved into `ASSETS` when
    // session J closed the finding (F199), so the guard that kept the row from
    // being quietly lost is now the golden itself.
    assert!(ASSETS.contains(&"Error_I_P"), "F93's asset left the referee; see F199");
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
