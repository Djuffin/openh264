//! **`GetOption`'s referee**.
//!
//! # The goldens are the C++ decoder's
//!
//! `tests/data/decoder_options/<asset>.txt` is the literal stdout of
//!
//! ```text
//! DYLD_LIBRARY_PATH=$PWD rust/tools/ecref/ecref res/<asset>.264 99999999 --options
//! ```
//!
//! against `libopenh264.dylib` — one line per decode call, every get-able scalar
//! option with **both** its return code and its value, because half the reference's
//! arms answer `cmInitExpected` rather than writing. The sentinel `1592614637`
//! (`0x5EED5EED`) is what `ecref` puts in the caller's `int` before each call, so a
//! "did not write" is visible and reproducible instead of being stack noise.
//!
//! The flow below is `ecref`'s statement for statement: annex-B split,
//! `ERROR_CON_SLICE_COPY`, one NAL per `DecodeFrame2`, the `END_OF_STREAM` option,
//! a final `DecodeFrame2(NULL, 0)`, then `FlushFrame` for what `GetOption` reports
//! remaining, capped at 24.
//!
//! # What the six assets are for
//!
//! | asset | what it exercises |
//! |---|---|
//! | `BA_MW_D` | 100 frames of CAVLC baseline — `FRAME_NUM` counting and `VCL_NAL`'s alternation over a long run |
//! | `Error_I_P` | the damaged stream: **two resolution changes**, so `PROFILE`/`LEVEL` move mid-stream and the `cmInitExpected` arm is exercised before the first SPS activates |
//! | `MR2_TANDBERG_E` | the only kind of asset in `res/` that marks a long-term reference — `LTR_MARKING_FLAG` and `LTR_MARKED_FRAME_NUM` are non-trivial here and nowhere else |
//! | `Cisco_Men_whisper_640x320_CABAC_Bframe_9` | CABAC with B-frames: `IS_REF_PIC` is 0 on the non-reference pictures |
//! | `QCIF_2P_I_allIPCM` | all-IPCM, a different reconstruction path |
//! | `narrow_16x16` | one macroblock, three IDR periods — `IDR_PIC_ID` increments |

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

/// The twelve ids `ecref --options` prints, in its order. `GET_STATISTICS` and
/// `GET_SAR_INFO` are struct-valued and are refereed by their own assertions below;
/// `NUM_OF_THREADS` is the object's field and does not change per call.
const OPTS: &[(&str, DECODER_OPTION)] = &[
    ("EOS", DECODER_OPTION::DECODER_OPTION_END_OF_STREAM),
    ("VCL", DECODER_OPTION::DECODER_OPTION_VCL_NAL),
    ("TID", DECODER_OPTION::DECODER_OPTION_TEMPORAL_ID),
    ("FN", DECODER_OPTION::DECODER_OPTION_FRAME_NUM),
    ("IDR", DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID),
    ("LTRF", DECODER_OPTION::DECODER_OPTION_LTR_MARKING_FLAG),
    ("LTRN", DECODER_OPTION::DECODER_OPTION_LTR_MARKED_FRAME_NUM),
    ("EC", DECODER_OPTION::DECODER_OPTION_ERROR_CON_IDC),
    ("PROF", DECODER_OPTION::DECODER_OPTION_PROFILE),
    ("LEVEL", DECODER_OPTION::DECODER_OPTION_LEVEL),
    ("REF", DECODER_OPTION::DECODER_OPTION_IS_REF_PIC),
    ("REM", DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER),
];

/// `ecref`'s `int v = 0x5EED5EED;` — see the module docs.
const SENTINEL: i32 = 0x5EED_5EED;

/// One `OPT <idx> <what> …` line, byte-for-byte as `ecref` prints it.
///
/// # Safety
/// `dec` is a live decoder; every pointer handed to `GetOption` is a local `i32`.
unsafe fn options_line(dec: *mut ISVCDecoder, idx: usize, what: &str) -> String {
    let mut out = format!("OPT {idx} {what}");
    for (name, id) in OPTS {
        let mut v: i32 = SENTINEL;
        let rc = unsafe { ISVCDecoder::GetOption(dec, *id, std::ptr::addr_of_mut!(v).cast()) };
        out.push_str(&format!(" {name}={}/{v}", rc as i64));
    }
    out
}

/// Drives one asset exactly as `ecref --options` does, returning its transcript.
///
/// # Safety
/// Uses the C ABI as a consumer does; every pointer is valid for its call.
unsafe fn options_transcript(data: &[u8]) -> Vec<String> {
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

        let mut lines = Vec::new();
        let mut idx = 0usize;
        let feed = |dec: *mut ISVCDecoder, src: *const u8, len: i32, lines: &mut Vec<String>, idx: &mut usize| {
            let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            ISVCDecoder::DecodeFrame2(dec, src, len, dst.as_mut_ptr(), &mut info);
            lines.push(options_line(dec, *idx, "decode"));
            *idx += 1;
        };

        for unit in split_annexb_units(data) {
            feed(dec, unit.as_ptr(), unit.len() as i32, &mut lines, &mut idx);
        }

        let mut eos = 1i32;
        ISVCDecoder::SetOption(
            dec,
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            std::ptr::addr_of_mut!(eos).cast(),
        );
        feed(dec, std::ptr::null(), 0, &mut lines, &mut idx);

        let mut remaining = 0i32;
        ISVCDecoder::GetOption(
            dec,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            std::ptr::addr_of_mut!(remaining).cast(),
        );
        for _ in 0..remaining.clamp(0, 24) {
            let mut dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            ISVCDecoder::FlushFrame(dec, dst.as_mut_ptr(), &mut info);
            lines.push(options_line(dec, idx, "flush"));
            idx += 1;
        }

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
        lines
    }
}

const ASSETS: &[&str] = &[
    "BA_MW_D",
    "Error_I_P",
    "MR2_TANDBERG_E",
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9",
    "QCIF_2P_I_allIPCM",
    "narrow_16x16",
];

#[test]
fn get_option_matches_the_cxx_per_call() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for asset in ASSETS {
        let data = std::fs::read(root.join("res").join(format!("{asset}.264")))
            .unwrap_or_else(|e| panic!("{asset}.264: {e}"));
        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/decoder_options")
            .join(format!("{asset}.txt"));
        let golden = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("{}: {e}", golden_path.display()));
        let expected: Vec<&str> = golden.lines().filter(|l| !l.is_empty()).collect();
        let got = unsafe { options_transcript(&data) };

        // The first differing line, not a wall of them: a wrong arm usually goes
        // wrong on every call, and the *first* one is the one that says why.
        for (i, want) in expected.iter().enumerate() {
            let have = got.get(i).map(String::as_str).unwrap_or("<no such call>");
            assert_eq!(
                have, *want,
                "\n{asset}: option transcript diverges at call {i}\n  C++  {want}\n  Rust {have}\n\
                 (goldens: {}; regenerate with `ecref res/{asset}.264 99999999 --options`)",
                golden_path.display()
            );
        }
        assert_eq!(
            got.len(),
            expected.len(),
            "{asset}: the port made {} decode calls where the C++ made {}",
            got.len(),
            expected.len()
        );
    }
}

/// The two struct-valued get arms and the error codes around them —
/// `welsDecoderExt.cpp:639-651` and `:664-672`, plus the head clauses at `:586-592`.
///
/// These are not in the per-call transcript because `ecref` prints scalars; they are
/// pinned here against the reference's *codes*, which is what they are about.
#[test]
fn option_error_codes_match_the_reference() {
    unsafe {
        let mut dec: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut dec)), CM_RESULT_SUCCESS as i64);

        // ---- before Initialize -------------------------------------------
        // `:586-589`: `NUM_OF_THREADS` is answered from the object and succeeds;
        // everything else is `cmInitExpected` — *including* when `pOption` is null,
        // because the reference tests the context first.
        let mut v = 0i32;
        let p: *mut std::ffi::c_void = std::ptr::addr_of_mut!(v).cast();
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(dec, DECODER_OPTION::DECODER_OPTION_NUM_OF_THREADS, p)),
            CM_RESULT_SUCCESS as i64,
            "NUM_OF_THREADS is the object's field and works before Initialize"
        );
        assert_eq!(v, 0, "this port is single-threaded (D3)");
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(dec, DECODER_OPTION::DECODER_OPTION_VCL_NAL, p)),
            CM_INIT_EXPECTED as i64,
            "no context yet"
        );
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_VCL_NAL,
                std::ptr::null_mut()
            )),
            CM_INIT_EXPECTED as i64,
            "the context is tested before pOption — welsDecoderExt.cpp:589 then :592"
        );
        // `:512`: with no context, a set of anything but the three trace ids is
        // `dsInitialOptExpected`.
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(dec, DECODER_OPTION::DECODER_OPTION_END_OF_STREAM, p)),
            i64::from(DECODING_STATE::dsInitialOptExpected.0),
        );

        // ---- after Initialize --------------------------------------------
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(dec, &param as *const SDecodingParam)),
            CM_RESULT_SUCCESS as i64
        );

        // `:592`: now a null `pOption` is `cmInitParaError`.
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_VCL_NAL,
                std::ptr::null_mut()
            )),
            CM_INIT_PARA_ERROR as i64,
        );

        // `:562` and `:578` — the two get-only ids refuse a set.
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(dec, DECODER_OPTION::DECODER_OPTION_GET_STATISTICS, p)),
            CM_INIT_PARA_ERROR as i64,
        );
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(dec, DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO, p)),
            CM_INIT_PARA_ERROR as i64,
        );

        // `:653-659` / `:571-577` — `STATISTICS_LOG_INTERVAL` is an `unsigned int`
        // both ways, and the default the reference installs is 1000
        // (`WelsDecoderDefaults`).
        let mut interval = 0u32;
        let ip: *mut std::ffi::c_void = std::ptr::addr_of_mut!(interval).cast();
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL,
                ip
            )),
            CM_RESULT_SUCCESS as i64
        );
        assert_eq!(interval, 1000);
        let mut set_to = 77u32;
        assert_eq!(
            i64::from(ISVCDecoder::SetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL,
                std::ptr::addr_of_mut!(set_to).cast()
            )),
            CM_RESULT_SUCCESS as i64
        );
        interval = 0;
        ISVCDecoder::GetOption(dec, DECODER_OPTION::DECODER_OPTION_STATISTICS_LOG_INTERVAL, ip);
        assert_eq!(interval, 77, "the set arm writes what the get arm reads");

        // `:664-672` — `GET_SAR_INFO` before any SPS: the struct is zeroed *and*
        // `cmInitExpected` is returned. A caller that ignores the code still gets
        // zeros rather than its own stack.
        let mut sar = SVuiSarInfo {
            uiSarWidth: 4242,
            uiSarHeight: 4242,
            bOverscanAppropriateFlag: true,
        };
        assert_eq!(
            i64::from(ISVCDecoder::GetOption(
                dec,
                DECODER_OPTION::DECODER_OPTION_GET_SAR_INFO,
                std::ptr::addr_of_mut!(sar).cast()
            )),
            CM_INIT_EXPECTED as i64
        );
        assert_eq!((sar.uiSarWidth, sar.uiSarHeight), (0, 0));

        // `:696` and `:583` — **an id with no arm is an error, not a silent
        // success.** It is reachable with real ids rather than an out-of-range
        // discriminant: the two switches are not the same set. The three trace ids
        // are settable and not gettable; the ten feedback ids are gettable and not
        // settable.
        for id in [
            DECODER_OPTION::DECODER_OPTION_TRACE_LEVEL,
            DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK,
            DECODER_OPTION::DECODER_OPTION_TRACE_CALLBACK_CONTEXT,
        ] {
            assert_eq!(
                i64::from(ISVCDecoder::GetOption(dec, id, p)),
                CM_INIT_PARA_ERROR as i64,
                "{id:?} has no GetOption arm in welsDecoderExt.cpp:584-695"
            );
        }
        for id in [
            DECODER_OPTION::DECODER_OPTION_VCL_NAL,
            DECODER_OPTION::DECODER_OPTION_TEMPORAL_ID,
            DECODER_OPTION::DECODER_OPTION_FRAME_NUM,
            DECODER_OPTION::DECODER_OPTION_IDR_PIC_ID,
            DECODER_OPTION::DECODER_OPTION_LTR_MARKING_FLAG,
            DECODER_OPTION::DECODER_OPTION_LTR_MARKED_FRAME_NUM,
            DECODER_OPTION::DECODER_OPTION_PROFILE,
            DECODER_OPTION::DECODER_OPTION_LEVEL,
            DECODER_OPTION::DECODER_OPTION_IS_REF_PIC,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
        ] {
            assert_eq!(
                i64::from(ISVCDecoder::SetOption(dec, id, p)),
                CM_INIT_PARA_ERROR as i64,
                "{id:?} has no SetOption arm in welsDecoderExt.cpp:479-584"
            );
        }

        ISVCDecoder::Uninitialize(dec);
        WelsDestroyDecoder(dec);
    }
}
