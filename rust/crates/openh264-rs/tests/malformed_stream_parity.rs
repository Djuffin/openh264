//! Malformed-stream error-code parity — Phase 3's **T3.0** gate.
//!
//! Written against the **unconverted** bitstream layer (raw `SBitStringAux` reader,
//! raw `SDataBuffer`, `nalu.rs` payload pointers) so that it pins today's behaviour
//! on the input class the conformance gates never reach: truncated NALs, emulation-
//! prevention edges, degenerate NALs and corrupt NAL headers. Every seam of Phase 3
//! rewrites code this test drives, and the golden tables below are the referee for
//!
//! * the **P6 guard-byte decision** (`phase1_findings.md` §F4 — the refill predicate
//!   lets the cursor sit one byte past the RBSP end and then loads two bytes, so
//!   off-by-one truncations at a refill boundary are exactly what must not shift),
//! * the **CABAC end-of-slice byte ladder** (`cabac_decoder.rs:732-784`, seam T3.2),
//! * and the **`nalu.rs` range conversion** (seam T3.3), whose
//!   `BsGetTrailingBits(pNal + len - 1)` sites underflowed on a zero-length payload
//!   — **F15**, which this test found on its first run and T3.3 fixed; the rows it
//!   withheld until then now carry real outcomes.
//!
//! Fuzzing was removed from Phase 3 by direction (2026-08-10) and the plan's exit
//! gate edited to match, so this file is the phase's **only** malformed-input
//! instrument. It is therefore not to be weakened. The golden tables are regenerated
//! only by a deliberate act:
//!
//! ```text
//! UPDATE_MALFORMED_GOLDEN=1 cargo test --test malformed_stream_parity
//! ```
//!
//! and every regenerated line is a behaviour change that has to be justified in the
//! commit that regenerates it.
//!
//! # What is recorded per corpus entry
//!
//! The exact `DecodeFrame2` return (`DECODING_STATE`, not "nonzero"), the
//! `iBufferStatus` outcome of every call, the `NUM_OF_FRAMES_REMAINING_IN_BUFFER`
//! answer at end of stream, the **decoded-frame count** (S13: frame counts before
//! hashes), the dimensions of the first emitted frame, and one SHA-1 over every
//! emitted plane in emission order.
//!
//! # Why the corpus runs in a child process
//!
//! A panic inside the decoder cannot be caught: the entry points are the `extern "C"`
//! vtable thunks, so an unwind out of one is a `panic in a function that cannot
//! unwind` — the process **aborts**. Each stream's test therefore re-executes this
//! binary as a worker that appends one row per corpus entry to a file as it goes; if
//! the worker dies, the parent knows exactly which entry killed it, records `ABORT`
//! with the panic site, and resumes at the next one. The happy path costs one process
//! spawn per stream.
//!
//! # Cost
//!
//! One `#[test]` per base stream, so the harness runs them in parallel and the
//! corpus knobs below are the per-stream budget. The knobs bound the corpus
//! deliberately; what they leave out is stated in each golden table's header rather
//! than left to be inferred.

mod common;

use common::Sha1Hasher;
use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Corpus knobs — the per-stream budget
// ---------------------------------------------------------------------------

/// Boundaries at the head of the stream that get the full ±[`FINE_DELTA`] sweep:
/// parameter sets and the first slices, which is where header parsing lives.
const FINE_HEAD: usize = 6;

/// Further boundaries, evenly spread over the rest of the stream and always
/// including the **last** one, that get the same sweep. Spreading rather than
/// sweeping every boundary is the deliberate cost bound: a truncation at boundary
/// *k* costs *k* NAL decodes, so an all-boundaries sweep is quadratic in the NAL
/// count.
const FINE_SPREAD: usize = 6;

/// The sweep itself — every truncation length within ±8 bytes of the boundary.
const FINE_DELTA: i64 = 8;

/// Truncations evenly spaced across the whole stream body, on top of the boundary
/// sweeps: cheap coverage of mid-NAL truncation at arbitrary bit positions.
const COARSE_POINTS: usize = 16;

/// Emulation-prevention (`00 00 03`) sites probed, evenly spread through the stream.
const EPB_SITES: usize = 4;

/// Header-corruption and synthetic-tail variants run against a prefix this many
/// boundaries long, so their cost stays proportional to the head of the stream
/// rather than to all of it.
const PREFIX_BOUNDARIES: usize = 8;

/// Replacement bytes for the NAL header byte (`forbidden_zero_bit`, `nal_ref_idc`,
/// `nal_unit_type`) — chosen to hit the early-exit paths: reserved type 0, an
/// unreferenced IDR, SPS/PPS types on a slice, the unspecified type 31, a set
/// forbidden bit, and a referenced-IDR header on a non-IDR NAL.
const HEADER_BYTES: &[u8] = &[0x00, 0x05, 0x07, 0x08, 0x1F, 0x65, 0x80];


/// The base streams. Diversity, not breadth: CAVLC and CABAC, PCM, B-frames, VUI
/// and subset-SPS parsing, an already-damaged pair, and one stream with a NAL count
/// two orders of magnitude above the others.
const BASE_STREAMS: &[&str] = &[
    "SarVui.264",
    "Static.264",
    "sps_subsetsps_bothVUI.264",
    "CABA2_SVA_B.264",
    "CABA3_SVA_B.264",
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9.264",
    "SVA_NL1_B.264",
    "QCIF_2P_I_allIPCM.264",
    "BA_MW_D.264",
    "BA_MW_D_IDR_LOST.264",
    "BA_MW_D_P_LOST.264",
];

// ---------------------------------------------------------------------------
// Corpus construction
// ---------------------------------------------------------------------------

/// How a corpus entry reaches the decoder.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Feed {
    /// Split into Annex-B units and fed one NAL per `DecodeFrame2` call — the flow
    /// `decoder_conformance_test.rs` uses, and therefore the flow whose error codes
    /// the gate set already constrains.
    AnnexB,
    /// One `DecodeFrame2` call with the exact bytes, start-code detection included.
    Raw,
}

struct Case {
    name: String,
    feed: Feed,
    data: Vec<u8>,
}

/// Byte offsets of the Annex-B start codes, by the same rules
/// [`split_annexb_units`] uses (3- and 4-byte prefixes, `i + 2 < len` scan bound).
/// `start_code_scan_agrees_with_split_annexb_units` pins the agreement.
fn scan_start_codes(data: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let len = data.len();
    let mut i = 0;
    while i + 2 < len {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                offsets.push(i);
                i += 3;
                continue;
            } else if i + 3 < len && data[i + 2] == 0 && data[i + 3] == 1 {
                offsets.push(i);
                i += 4;
                continue;
            }
        }
        match data[i + 1..].iter().position(|&b| b == 0) {
            Some(pos) => i += 1 + pos,
            None => break,
        }
    }
    offsets
}

/// The boundary indices that get the fine sweep: the first [`FINE_HEAD`], plus
/// [`FINE_SPREAD`] evenly spread over the remainder with the last always included.
fn fine_boundary_indices(count: usize) -> Vec<usize> {
    let mut picked: Vec<usize> = (0..count.min(FINE_HEAD)).collect();
    if count > FINE_HEAD && FINE_SPREAD > 0 {
        let span = count - 1 - FINE_HEAD; // last index reachable by the spread
        let steps = FINE_SPREAD.max(2) - 1; // k = steps lands on the last boundary
        for k in 0..=steps.min(FINE_SPREAD - 1) {
            picked.push(FINE_HEAD + span * k / steps);
        }
    }
    picked.sort_unstable();
    picked.dedup();
    picked
}

/// Offsets of `00 00 03` sequences, evenly thinned to at most [`EPB_SITES`].
fn epb_sites(data: &[u8]) -> Vec<usize> {
    let all: Vec<usize> = data
        .windows(3)
        .enumerate()
        .filter(|(_, w)| w == &[0u8, 0, 3])
        .map(|(i, _)| i)
        .collect();
    if all.len() <= EPB_SITES {
        return all;
    }
    (0..EPB_SITES)
        .map(|k| all[(all.len() - 1) * k / (EPB_SITES - 1).max(1)])
        .collect()
}

fn build_corpus(data: &[u8], offsets: &[usize]) -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    let mut seen_len: Vec<usize> = Vec::new();
    let total = data.len() as i64;

    let push_trunc = |cases: &mut Vec<Case>, seen: &mut Vec<usize>, name: String, len: i64| {
        if len < 0 || len > total {
            return;
        }
        let len = len as usize;
        if seen.contains(&len) {
            return;
        }
        seen.push(len);
        cases.push(Case {
            name,
            feed: Feed::AnnexB,
            data: data[..len].to_vec(),
        });
    };

    // (1) The fine sweep around selected NAL boundaries.
    for &b in &fine_boundary_indices(offsets.len()) {
        for d in -FINE_DELTA..=FINE_DELTA {
            push_trunc(
                &mut cases,
                &mut seen_len,
                format!("trunc.b{b:03}{d:+03}"),
                offsets[b] as i64 + d,
            );
        }
    }

    // (2) The coarse sweep across the body.
    for k in 1..=COARSE_POINTS {
        push_trunc(
            &mut cases,
            &mut seen_len,
            format!("trunc.coarse{k:02}"),
            total * k as i64 / (COARSE_POINTS as i64 + 1),
        );
    }

    // (3) Emulation-prevention edges: cut between `00 00` and the `03`, right after
    //     the `03`, and one byte past it.
    for (n, &s) in epb_sites(data).iter().enumerate() {
        push_trunc(&mut cases, &mut seen_len, format!("epb{n}.zz"), s as i64 + 2);
        push_trunc(&mut cases, &mut seen_len, format!("epb{n}.at03"), s as i64 + 3);
        push_trunc(&mut cases, &mut seen_len, format!("epb{n}.after"), s as i64 + 4);
    }

    // (4) Synthetic tails on a bounded prefix: a stream ending in `00 00`, in
    //     `00 00 03`, in a bare start code, and in a start code plus one byte.
    let prefix_end = offsets
        .get(PREFIX_BOUNDARIES.min(offsets.len().saturating_sub(1)))
        .copied()
        .unwrap_or(data.len());
    let prefix = &data[..prefix_end];
    for (suffix_name, suffix) in [
        ("zz", &[0u8, 0][..]),
        ("zzz", &[0, 0, 0][..]),
        ("epb", &[0, 0, 3][..]),
        ("sc3", &[0, 0, 1][..]),
        ("sc4", &[0, 0, 0, 1][..]),
        ("sc3_byte", &[0, 0, 1, 0x65][..]),
    ] {
        let mut buf = prefix.to_vec();
        buf.extend_from_slice(suffix);
        cases.push(Case {
            name: format!("tail.{suffix_name}"),
            feed: Feed::AnnexB,
            data: buf,
        });
    }

    // (5) Header corruption on the same bounded prefix: the byte after the start
    //     code carries forbidden_zero_bit / nal_ref_idc / nal_unit_type, so these
    //     variants drive the early-exit paths.
    let sites: Vec<usize> = {
        let mut v: Vec<usize> = offsets.iter().take(2).copied().collect();
        if let Some(&vcl) = offsets.iter().find(|&&o| {
            let hdr = header_byte(data, o);
            matches!(hdr.map(|h| h & 0x1F), Some(1) | Some(5))
        }) {
            v.push(vcl);
        }
        v.retain(|&o| o < prefix_end);
        v.sort_unstable();
        v.dedup();
        v
    };
    for (n, &site) in sites.iter().enumerate() {
        let hdr_at = site + start_code_len(data, site);
        if hdr_at >= prefix.len() {
            continue;
        }
        for &byte in HEADER_BYTES {
            if prefix[hdr_at] == byte {
                continue;
            }
            let mut buf = prefix.to_vec();
            buf[hdr_at] = byte;
            cases.push(Case {
                name: format!("hdr{n}.{byte:02x}"),
                feed: Feed::AnnexB,
                data: buf,
            });
        }
    }

    cases
}

fn start_code_len(data: &[u8], offset: usize) -> usize {
    if data.get(offset + 2) == Some(&1) { 3 } else { 4 }
}

fn header_byte(data: &[u8], offset: usize) -> Option<u8> {
    data.get(offset + start_code_len(data, offset)).copied()
}

/// Degenerate inputs that no truncation of a real stream produces: empty input,
/// bare start codes, zero-length payloads, a lone SPS, and an SPS/PPS pair with the
/// slice cut to a handful of bytes. Both feed modes, because `Raw` exercises the
/// start-code scan (`split_annexb_units`) on bytes the whole-stream feed would
/// have dropped.
fn degenerate_corpus() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut push = |name: &str, bytes: Vec<u8>| {
        cases.push(Case {
            name: format!("raw.{name}"),
            feed: Feed::Raw,
            data: bytes.clone(),
        });
        cases.push(Case {
            name: format!("annexb.{name}"),
            feed: Feed::AnnexB,
            data: bytes,
        });
    };

    push("empty", vec![]);
    push("z1", vec![0]);
    push("z2", vec![0, 0]);
    push("z3", vec![0, 0, 0]);
    push("epb_only", vec![0, 0, 3]);
    push("sc3_only", vec![0, 0, 1]);
    push("sc4_only", vec![0, 0, 0, 1]);
    push("sc3_zero_payload", vec![0, 0, 1, 0, 0, 1]);
    push("sc3_sps_byte", vec![0, 0, 1, 0x67]);
    push("sc3_slice_byte", vec![0, 0, 1, 0x65]);
    push("sc3_one_zero", vec![0, 0, 1, 0x00]);
    push("sc3_type31", vec![0, 0, 1, 0x1F]);

    // Parameter sets lifted out of a real stream, so the payloads are valid and only
    // the *sequence* is degenerate.
    let data = read_stream("SarVui.264");
    let units = split_annexb_units(&data);
    let find = |ty: u8| -> Option<Vec<u8>> {
        units
            .iter()
            .find(|u| header_byte(u, 0).map(|h| h & 0x1F) == Some(ty))
            .map(|u| u.to_vec())
    };
    let sps = find(7).expect("SarVui.264 has an SPS");
    let pps = find(8).expect("SarVui.264 has a PPS");
    let slice = find(5).or_else(|| find(1)).expect("SarVui.264 has a slice");

    push("sps_only", sps.clone());
    push("pps_only", pps.clone());
    push("pps_then_sps", [pps.clone(), sps.clone()].concat());
    for n in [1usize, 2, 3, 4, 5, 8] {
        let mut buf = [sps.clone(), pps.clone()].concat();
        buf.extend_from_slice(&slice[..(slice.len()).min(start_code_len(&slice, 0) + n)]);
        push(&format!("sps_pps_slice{n}"), buf);
    }
    // An SPS whose own payload is cut short, one byte at a time.
    for n in [1usize, 2, 3, 4] {
        push(
            &format!("sps_cut{n}"),
            sps[..(start_code_len(&sps, 0) + n).min(sps.len())].to_vec(),
        );
    }

    cases
}

// ---------------------------------------------------------------------------
// Running one corpus entry
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Run {
    /// `(DECODING_STATE as i32, iBufferStatus)` per `DecodeFrame2`/`FlushFrame` call.
    calls: Vec<(i32, i32)>,
    /// `DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER` after the EOS call.
    drain: i32,
    frames: usize,
    dims: Option<(i32, i32)>,
}

/// Hard cap on the flush loop: a `remaining` answer above this is a defect, and the
/// raw value is recorded in the table either way rather than silently clamped.
const MAX_DRAIN: i32 = 24;

unsafe fn feed(
    decoder: *mut ISVCDecoder,
    unit: &[u8],
    run: &mut Run,
    hasher: &mut Sha1Hasher,
) {
    unsafe {
        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
        let mut buf_info = SBufferInfo::default();
        let src = if unit.is_empty() {
            std::ptr::null()
        } else {
            unit.as_ptr()
        };
        let ret = (*decoder).DecodeFrame2(
            src,
            unit.len() as i32,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        record(ret as i32, &buf_info, p_dst, run, hasher);
    }
}

unsafe fn record(
    ret: i32,
    buf_info: &SBufferInfo,
    dst: [*mut u8; 3],
    run: &mut Run,
    hasher: &mut Sha1Hasher,
) {
    run.calls.push((ret, buf_info.iBufferStatus));
    if buf_info.iBufferStatus != 1 {
        return;
    }
    unsafe {
        let width = buf_info.UsrData.sSystemBuffer.iWidth;
        let height = buf_info.UsrData.sSystemBuffer.iHeight;
        let stride_y = buf_info.UsrData.sSystemBuffer.iStride[0] as usize;
        let stride_uv = buf_info.UsrData.sSystemBuffer.iStride[1] as usize;
        run.frames += 1;
        if run.dims.is_none() {
            run.dims = Some((width, height));
        }
        let (w, h) = (width as usize, height as usize);
        hash_plane(hasher, dst[0], w, h, stride_y);
        hash_plane(hasher, dst[1], w / 2, h / 2, stride_uv);
        hash_plane(hasher, dst[2], w / 2, h / 2, stride_uv);
    }
}

unsafe fn hash_plane(
    hasher: &mut Sha1Hasher,
    plane: *const u8,
    width: usize,
    height: usize,
    stride: usize,
) {
    if plane.is_null() || width == 0 || height == 0 || stride == 0 {
        return;
    }
    unsafe {
        for y in 0..height {
            hasher.update(std::slice::from_raw_parts(plane.add(y * stride), width));
        }
    }
}

/// Drives one corpus entry through the same C-API flow as
/// `decoder_conformance_test.rs`: create → `Initialize` with `ERROR_CON_SLICE_COPY`
/// → per-NAL `DecodeFrame2` → EOS → drain → destroy.
fn decode_case(case: &Case) -> (Run, String) {
    unsafe {
        let mut decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut decoder);
        assert_eq!(i64::from(ret), CM_RESULT_SUCCESS as i64, "WelsCreateDecoder");
        assert!(!decoder.is_null());

        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        let init_ret = (*decoder).Initialize(&dec_param as *const SDecodingParam);
        assert_eq!(i64::from(init_ret), CM_RESULT_SUCCESS as i64, "Initialize");

        let mut run = Run::default();
        let mut hasher = Sha1Hasher::new();

        match case.feed {
            Feed::AnnexB => {
                for unit in split_annexb_units(&case.data) {
                    feed(decoder, unit, &mut run, &mut hasher);
                }
            }
            Feed::Raw => feed(decoder, &case.data, &mut run, &mut hasher),
        }

        // End of stream, then the null/zero-length call that flushes it.
        let mut eos_flag = 1i32;
        (*decoder).SetOption(
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
        );
        feed(decoder, &[], &mut run, &mut hasher);

        let mut remaining = 0i32;
        (*decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        run.drain = remaining;
        for _ in 0..remaining.clamp(0, MAX_DRAIN) {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let flush_ret = (*decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
            record(flush_ret as i32, &buf_info, p_dst, &mut run, &mut hasher);
        }

        (*decoder).Uninitialize();
        WelsDestroyDecoder(decoder);

        let digest = if run.frames == 0 {
            "-".to_string()
        } else {
            hasher.digest()
        };
        (run, digest)
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// `[0, 0, 2, 2, 2]` → `0x0*2,0x2*3`. Exact, and short for the common case where a
/// truncated stream decodes cleanly and then fails once.
fn rle<T: PartialEq + Copy, F: Fn(T) -> String>(values: &[T], show: F) -> String {
    if values.is_empty() {
        return "-".to_string();
    }
    let mut out = String::new();
    let mut i = 0;
    while i < values.len() {
        let mut n = 1;
        while i + n < values.len() && values[i + n] == values[i] {
            n += 1;
        }
        if !out.is_empty() {
            out.push(',');
        }
        let _ = write!(out, "{}", show(values[i]));
        if n > 1 {
            let _ = write!(out, "*{n}");
        }
        i += n;
    }
    out
}

/// What a corpus entry produced. `Aborted` is a run whose process died,
/// reconstructed by the parent from the worker's exit status.
///
/// A third variant, `Withheld`, existed from T3.0 until T3.3: entries that drove
/// **F15** were not run at all, because that input had no profile-independent
/// behaviour to record (debug aborted; release read out of bounds). T3.3 fixed the
/// sites, both profiles now agree, and the 105 `WITHHELD` rows filled in with real
/// outcomes — the seam's evidence, and the reason the variant is gone rather than
/// merely unused.
enum Outcome {
    Ran(Run, String),
    Aborted(String),
}

fn row(case: &Case, outcome: &Outcome) -> String {
    match outcome {
        Outcome::Aborted(message) => {
            format!("{:<24} {:>8}  ABORT     {}", case.name, case.data.len(), message)
        }
        Outcome::Ran(run, digest) => {
            let rets: Vec<i32> = run.calls.iter().map(|c| c.0).collect();
            let statuses: Vec<i32> = run.calls.iter().map(|c| c.1).collect();
            let dims = match run.dims {
                Some((w, h)) => format!("{w}x{h}"),
                None => "-".to_string(),
            };
            format!(
                "{:<24} {:>8} {:>5} {:>5} {:>4} {:<10} {:<40} {:<26} {}",
                case.name,
                case.data.len(),
                run.calls.len(),
                run.drain,
                run.frames,
                dims,
                digest,
                rle(&rets, |v| format!("{v:#x}")),
                rle(&statuses, |v| v.to_string()),
            )
        }
    }
}

const COLUMN_HEADER: &str =
    "# columns: variant | bytes | calls | drain | frames | dims | planes_sha1 | ret_rle | bufstatus_rle";

/// Runs one corpus entry and renders its row — the unit both the worker and a direct
/// (non-forking) run share.
fn run_case(case: &Case) -> String {
    let (run, digest) = decode_case(case);
    row(case, &Outcome::Ran(run, digest))
}

// ---------------------------------------------------------------------------
// Worker / parent split
// ---------------------------------------------------------------------------

/// Set by the parent to the corpus index the child should start from.
const ENV_START: &str = "MALFORMED_WORKER_START";
/// Set by the parent to the file the child appends `index<TAB>row` lines to.
const ENV_OUT: &str = "MALFORMED_WORKER_OUT";

/// The child: decode from `start` to the end of the corpus, appending each row to
/// `out` **unbuffered**, so the rows already written survive an abort.
fn run_worker(cases: &[Case], start: usize, out: &Path) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .expect("worker output file");
    for (i, case) in cases.iter().enumerate().skip(start) {
        let line = format!("{i}\t{}\n", run_case(case));
        file.write_all(line.as_bytes()).expect("worker row");
    }
}

/// The parent: run the corpus in child processes, resuming past any entry that kills
/// one. Returns one row per case, in corpus order.
fn collect_rows(cases: &[Case], test_name: &str) -> Vec<String> {
    let out = std::env::temp_dir().join(format!(
        "openh264-malformed-{}-{}.rows",
        test_name,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&out);
    let exe = std::env::current_exe().expect("test binary path");

    let mut rows: Vec<Option<String>> = vec![None; cases.len()];
    let mut next = 0usize;
    while next < cases.len() {
        let child = std::process::Command::new(&exe)
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env(ENV_START, next.to_string())
            .env(ENV_OUT, &out)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .expect("spawn corpus worker");

        for line in std::fs::read_to_string(&out).unwrap_or_default().lines() {
            if let Some((idx, row)) = line.split_once('\t') {
                if let Ok(i) = idx.parse::<usize>() {
                    if i < rows.len() {
                        rows[i] = Some(row.to_string());
                    }
                }
            }
        }

        match rows.iter().position(Option::is_none) {
            None => break,
            Some(dead) => {
                assert!(
                    !child.status.success(),
                    "corpus worker for {test_name} exited cleanly but produced no row for \
                     `{}` (index {dead}) — the worker protocol is broken, not the decoder",
                    cases[dead].name
                );
                rows[dead] = Some(row(
                    &cases[dead],
                    &Outcome::Aborted(panic_site(&String::from_utf8_lossy(&child.stderr))),
                ));
                next = dead + 1;
            }
        }
    }
    let _ = std::fs::remove_file(&out);
    rows.into_iter().map(|r| r.expect("every row filled")).collect()
}

/// `thread '…' panicked at src/x.rs:1:2:\nmessage` → `src/x.rs:1:2: message`.
fn panic_site(stderr: &str) -> String {
    let mut lines = stderr.lines();
    while let Some(line) = lines.next() {
        if let Some((_, site)) = line.split_once("panicked at ") {
            let message = lines.next().unwrap_or("").trim();
            return format!("{} {message}", site.trim());
        }
    }
    "died without a panic message (signal?)".to_string()
}

fn table(base: &str, data: &[u8], offsets: &[usize], cases: &[Case], test_name: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# malformed-stream error-code parity — generated by tests/malformed_stream_parity.rs"
    );
    let _ = writeln!(
        out,
        "# base: {base}  bytes={}  start_codes={}",
        data.len(),
        offsets.len()
    );
    let _ = writeln!(
        out,
        "# knobs: fine_head={FINE_HEAD} fine_spread={FINE_SPREAD} fine_delta=±{FINE_DELTA} \
         coarse={COARSE_POINTS} epb_sites={EPB_SITES} prefix_boundaries={PREFIX_BOUNDARIES}"
    );
    let _ = writeln!(
        out,
        "# not covered by design: boundaries outside the fine set (cost is quadratic in NAL count)"
    );
    let _ = writeln!(out, "{COLUMN_HEADER}");
    for row in collect_rows(cases, test_name) {
        let _ = writeln!(out, "{row}");
    }
    out
}

// ---------------------------------------------------------------------------
// Golden-table plumbing
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    let mut root = PathBuf::from("../../../");
    if !root.join("res").exists() {
        root = PathBuf::from("../../");
    }
    root
}

fn read_stream(name: &str) -> Vec<u8> {
    let path = repo_root().join("res").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn golden_path(stem: &str) -> PathBuf {
    Path::new("tests/data/malformed_parity").join(format!("{stem}.txt"))
}

fn check_table(stem: &str, actual: &str) {
    let aborts: Vec<&str> = actual
        .lines()
        .filter(|l| l.contains("  ABORT     "))
        .collect();
    assert!(
        aborts.is_empty(),
        "{} corpus entries killed the decoder process:\n{}\n\
         A panic inside the decoder aborts (it unwinds out of an `extern \"C\"` thunk), so this \
         is a pre-existing defect to record, not to repair (plan §7.6 S6/S12): write it up in \
         docs/phase3_findings.md. If the two build profiles disagree on it, it is UB evidence \
         (plan §7.2 gate 0) and the golden table cannot hold both — that is what F15 was, and \
         the answer there was to fix the defect, not to keep withholding the rows.",
        aborts.len(),
        aborts.join("\n")
    );

    let path = golden_path(stem);
    if std::env::var_os("UPDATE_MALFORMED_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden table");
        eprintln!("regenerated {}", path.display());
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "golden table {} is missing ({e}); regenerate deliberately with \
             UPDATE_MALFORMED_GOLDEN=1 and review the diff",
            path.display()
        )
    });
    if expected == actual {
        return;
    }

    let mut diff = String::new();
    let (want, got): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    let mut shown = 0;
    for i in 0..want.len().max(got.len()) {
        let (w, g) = (want.get(i).copied(), got.get(i).copied());
        if w == g {
            continue;
        }
        shown += 1;
        if shown > 12 {
            let _ = writeln!(diff, "  … and more");
            break;
        }
        let _ = writeln!(diff, "  line {}:\n    -{}\n    +{}", i + 1, w.unwrap_or("<missing>"), g.unwrap_or("<missing>"));
    }
    panic!(
        "malformed-stream parity changed for {stem} ({} expected lines, {} produced):\n{diff}\n\
         Every line here is an error code, a frame count or a plane hash on malformed input. \
         If the change is intended, regenerate with UPDATE_MALFORMED_GOLDEN=1 and justify each \
         line in the commit message.",
        want.len(),
        got.len()
    );
}

fn check_stream(name: &str, test_name: &str) {
    let data = read_stream(name);
    let offsets = scan_start_codes(&data);
    assert!(!offsets.is_empty(), "{name} has no start codes");
    let cases = build_corpus(&data, &offsets);
    if worker_mode(&cases) {
        return;
    }
    let actual = table(&format!("res/{name}"), &data, &offsets, &cases, test_name);
    check_table(stem_of(name), &actual);
}

/// True when this process is a worker spawned by [`collect_rows`] — in which case it
/// has just written its rows and must not compare or regenerate anything.
fn worker_mode(cases: &[Case]) -> bool {
    let (Some(start), Some(out)) = (std::env::var_os(ENV_START), std::env::var_os(ENV_OUT)) else {
        return false;
    };
    let start: usize = start.to_string_lossy().parse().expect("worker start index");
    run_worker(cases, start, Path::new(&out));
    true
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The corpus builder walks start codes itself so it can talk about *offsets*; this
/// pins that walk against the decoder-facing splitter it has to agree with.
#[test]
fn start_code_scan_agrees_with_split_annexb_units() {
    for &name in BASE_STREAMS {
        let data = read_stream(name);
        let offsets = scan_start_codes(&data);
        let units = split_annexb_units(&data);
        assert_eq!(offsets.len(), units.len(), "{name}: unit count");
        for (i, unit) in units.iter().enumerate() {
            let end = offsets.get(i + 1).copied().unwrap_or(data.len());
            assert_eq!(&data[offsets[i]..end], *unit, "{name}: unit {i}");
        }
    }
}

macro_rules! stream_case {
    ($test_name:ident, $file:expr) => {
        #[test]
        fn $test_name() {
            check_stream($file, stringify!($test_name));
        }
    };
}

stream_case!(malformed_sarvui, "SarVui.264");
stream_case!(malformed_static, "Static.264");
stream_case!(malformed_sps_subsetsps_bothvui, "sps_subsetsps_bothVUI.264");
stream_case!(malformed_caba2_sva_b, "CABA2_SVA_B.264");
stream_case!(malformed_caba3_sva_b, "CABA3_SVA_B.264");
stream_case!(
    malformed_cisco_men_whisper_cabac_bframe,
    "Cisco_Men_whisper_640x320_CABAC_Bframe_9.264"
);
stream_case!(malformed_sva_nl1_b, "SVA_NL1_B.264");
stream_case!(malformed_qcif_2p_i_allipcm, "QCIF_2P_I_allIPCM.264");
stream_case!(malformed_ba_mw_d, "BA_MW_D.264");
stream_case!(malformed_ba_mw_d_idr_lost, "BA_MW_D_IDR_LOST.264");
stream_case!(malformed_ba_mw_d_p_lost, "BA_MW_D_P_LOST.264");

#[test]
fn malformed_degenerate_nals() {
    let cases = degenerate_corpus();
    if worker_mode(&cases) {
        return;
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# malformed-stream error-code parity — generated by tests/malformed_stream_parity.rs"
    );
    let _ = writeln!(
        out,
        "# degenerate NALs: empty input, bare start codes, zero-length payloads, lone/cut \
         parameter sets"
    );
    let _ = writeln!(
        out,
        "# parameter sets are lifted from res/SarVui.264; `raw.` feeds one DecodeFrame2 call, \
         `annexb.` splits first"
    );
    let _ = writeln!(out, "{COLUMN_HEADER}");
    for row in collect_rows(&cases, "malformed_degenerate_nals") {
        let _ = writeln!(out, "{row}");
    }
    check_table("degenerate", &out);
}
