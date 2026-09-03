//! **The reset-arm reachability sweep.**
//!
//! It is the widest decode this project runs: every `res/*.264` under every base
//! concealment mode under both bitstream declarations, in one pass. The conformance
//! 60 pin *output* on a curated set; the malformed corpus pins *codes* on damaged
//! prefixes of eleven streams plus `Error_I_P`; this pins **which error classes the
//! whole asset tree can produce at all**.
//!
//! **Release only** (`cfg(not(debug_assertions))`), deliberately: it is one pass over
//! ~250 whole-stream decodes and the debug suite already carries the conformance 60.
#![cfg(not(debug_assertions))]

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;

/// The four base concealment modes — the `CROSS_IDR` and `MV_COPY` variants are
/// compositions of these and the sweep is about reaching *decoder* arms, not about
/// enumerating the option space (`error_concealment_api_test.rs` does that).
const EC_MODES: [ERROR_CON_IDC; 4] = [
    ERROR_CON_IDC::ERROR_CON_DISABLE,
    ERROR_CON_IDC::ERROR_CON_FRAME_COPY,
    ERROR_CON_IDC::ERROR_CON_SLICE_COPY,
    ERROR_CON_IDC::ERROR_CON_SLICE_COPY_CROSS_IDR,
];

/// Both declarations a caller can make about the bitstream.
const BS_TYPES: [VIDEO_BITSTREAM_TYPE; 2] = [
    VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_AVC,
    VIDEO_BITSTREAM_TYPE::VIDEO_BITSTREAM_SVC,
];

/// The union of every `DECODING_STATE` bit the sweep produces.
///
/// `0x02 dsRefLost | 0x04 dsBitstreamError | 0x10 dsNoParamSets | 0x20
/// dsDataErrorConcealed`.
///
/// **This is pinned in both directions on purpose.** A bit appearing means a decode
/// path became reachable that was not; a bit disappearing means one stopped being
/// reachable.
const EXPECTED_UNION: i32 = 0x36;

/// The two arms this sweep exists to keep honest: reachable from **no** stream in
/// `res/`.
const UNREACHED: [(i32, &str); 2] = [
    (0x4000, "dsOutOfMemory"),
    (0x0040, "dsRefListNullPtrs"),
];

fn assets() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").join("res");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "264"))
        .collect();
    v.sort();
    v
}

/// One whole-stream decode; returns the OR of every state the decoder reported.
///
/// # Safety
/// Drives the C ABI exactly as `malformed_stream_parity.rs` does.
unsafe fn sweep_one(data: &[u8], ec: ERROR_CON_IDC, bs: VIDEO_BITSTREAM_TYPE) -> i32 {
    unsafe {
        let mut decoder: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(i64::from(WelsCreateDecoder(&mut decoder)), CM_RESULT_SUCCESS as i64);
        let mut param = SDecodingParam::default();
        param.uiTargetDqLayer = u8::MAX;
        param.eEcActiveIdc = ec;
        param.sVideoProperty.eVideoBsType = bs;
        assert_eq!(
            i64::from(ISVCDecoder::Initialize(decoder, &param as *const SDecodingParam)),
            CM_RESULT_SUCCESS as i64
        );

        let mut union_bits = 0i32;
        let mut feed = |unit: &[u8]| {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let src = if unit.is_empty() { std::ptr::null() } else { unit.as_ptr() };
            let st = ISVCDecoder::DecodeFrame2(decoder, src, unit.len() as i32, p_dst.as_mut_ptr(), &mut info);
            union_bits |= st.0;
        };
        for unit in split_annexb_units(data) {
            feed(unit);
        }
        let mut eos = 1i32;
        ISVCDecoder::SetOption(
            decoder,
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos as *mut i32 as *mut std::ffi::c_void,
        );
        feed(&[]);
        ISVCDecoder::Uninitialize(decoder);
        WelsDestroyDecoder(decoder);
        union_bits
    }
}

#[test]
fn every_res_stream_under_every_concealment_mode_reaches_a_known_set_of_states() {
    let files = assets();
    assert!(files.len() >= 60, "res/ should hold the whole asset tree, found {}", files.len());

    // **Forked across the asset list.** Serially this is ~90s of whole-stream
    // decodes, which is too much to add to a per-commit gate; the decodes are
    // independent — one decoder object per (stream, mode, declaration), created and
    // destroyed inside the thread that uses it, and no `*mut ISVCDecoder` ever crosses
    // a thread — so the work parallelises exactly. Nothing here relies on `Decoder`
    // being `Send`, which it is not.
    let nthreads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(files.len().max(1));
    let (union_bits, offenders, decodes) = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for t in 0..nthreads {
            let slice: Vec<&std::path::PathBuf> = files.iter().skip(t).step_by(nthreads).collect();
            handles.push(scope.spawn(move || {
                let mut union_bits = 0i32;
                let mut offenders: Vec<String> = Vec::new();
                let mut decodes = 0usize;
                for path in slice {
                    let data = std::fs::read(path).unwrap();
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    for &ec in &EC_MODES {
                        for &bs in &BS_TYPES {
                            // The decode itself is the abort test: an `extern "C"`
                            // thunk that panics kills this process and no assertion
                            // below ever runs.
                            let bits = unsafe { sweep_one(&data, ec, bs) };
                            decodes += 1;
                            union_bits |= bits;
                            for (bit, label) in UNREACHED {
                                if bits & bit != 0 {
                                    offenders.push(format!("{name} ec={ec:?} bs={bs:?} -> {label}"));
                                }
                            }
                        }
                    }
                }
                (union_bits, offenders, decodes)
            }));
        }
        handles.into_iter().map(|h| h.join().expect("sweep worker")).fold(
            (0i32, Vec::new(), 0usize),
            |(u, mut o, d), (u2, o2, d2)| {
                o.extend(o2);
                (u | u2, o, d + d2)
            },
        )
    });

    eprintln!(
        "reachability sweep: {} streams x {} modes x {} declarations = {decodes} decodes, state union 0x{union_bits:x}",
        files.len(),
        EC_MODES.len(),
        BS_TYPES.len()
    );

    assert!(
        offenders.is_empty(),
        "an arm documented as unreachable was reached — that is a fact worth a finding, \
         not a number to change:\n  {}",
        offenders.join("\n  ")
    );
    assert_eq!(
        union_bits, EXPECTED_UNION,
        "the reachable state set moved (0x{union_bits:x} vs 0x{EXPECTED_UNION:x}); \
         a decode path became reachable or stopped being reachable"
    );
}
