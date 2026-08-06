//! 1080p decode throughput: native C++ OpenH264 (`libopenh264.dylib`) vs. the
//! Rust port, both fed the byte-identical Annex-B streams that ffmpeg produced.
//!
//! Each stream is decoded twice per implementation: a verification pass that
//! SHA-1s every output plane and counts frames, then a timed pass that runs the
//! decode calls on their own. Timings are only reported as a comparison once
//! both sides agree on the frame count and the hash -- a decode error drops
//! frames from the output silently, and fewer frames decoded reads as a
//! speedup if nobody is checking.
//!
//! Both sides are driven through the same loop over the same NAL units, one
//! unit per `DecodeFrame2` call, matching how `decoder_conformance_test.rs`
//! feeds the decoder.
//!
//! Environment knobs: `FFMPEG` (path to the binary), `BENCH_FRAMES` (default
//! 60), `BENCH_ITERS` (timed passes per stream, default 3), `BENCH_PATTERN`
//! (lavfi source, default `testsrc2`).

#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;
use std::ffi::{CString, c_long, c_void};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::time::{Duration, Instant};

#[path = "../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;
use common::Sha1Hasher;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

// ---------------------------------------------------------------------------
// Test streams
// ---------------------------------------------------------------------------

/// An ffmpeg recipe for one 1080p stream. The profile is what moves decode
/// cost around: CAVLC vs CABAC, B-frames, and the 8x8 transform.
struct StreamSpec {
    slug: &'static str,
    label: &'static str,
    profile: &'static str,
}

const STREAMS: &[StreamSpec] = &[
    StreamSpec {
        slug: "baseline",
        label: "Constrained Baseline (CAVLC, no B-frames)",
        profile: "baseline",
    },
    StreamSpec {
        slug: "main",
        label: "Main (CABAC, B-frames)",
        profile: "main",
    },
    StreamSpec {
        slug: "high",
        label: "High (CABAC, B-frames, 8x8 transform)",
        profile: "high",
    },
];

fn resolve_ffmpeg() -> Option<PathBuf> {
    if let Ok(from_env) = std::env::var("FFMPEG") {
        let path = PathBuf::from(from_env);
        if path.exists() {
            return Some(path);
        }
    }
    if Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|out| out.status.success())
    {
        return Some(PathBuf::from("ffmpeg"));
    }
    // Cargo does not necessarily inherit a login shell's PATH.
    for candidate in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        if Path::new(candidate).exists() {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

fn stream_cache_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-streams");
    std::fs::create_dir_all(&dir).expect("cannot create bench stream cache dir");
    dir
}

/// Encodes the stream with ffmpeg unless a matching file is already cached.
/// Everything that changes the bitstream is in the filename, so bumping
/// `BENCH_FRAMES` or `BENCH_PATTERN` regenerates rather than reusing a stale
/// file.
fn ensure_stream(ffmpeg: Option<&PathBuf>, spec: &StreamSpec, pattern: &str, frames: usize) -> Option<PathBuf> {
    let path = stream_cache_dir().join(format!(
        "{}_{}_{}x{}_{}f.264",
        pattern, spec.slug, WIDTH, HEIGHT, frames
    ));
    if path.exists() && std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Some(path);
    }

    let ffmpeg = ffmpeg?;
    let source = format!("{}=size={}x{}:rate=30", pattern, WIDTH, HEIGHT);
    println!("  generating {} ...", path.file_name().unwrap().to_string_lossy());
    let out = Command::new(ffmpeg)
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &source,
            "-frames:v",
            &frames.to_string(),
            "-c:v",
            "libx264",
            "-profile:v",
            spec.profile,
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "5M",
            "-f",
            "h264",
            path.to_str().unwrap(),
        ])
        .output()
        .ok()?;

    if !out.status.success() {
        eprintln!(
            "  ffmpeg failed for {}: {}",
            spec.slug,
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    Some(path)
}

// ---------------------------------------------------------------------------
// The decoder under test
// ---------------------------------------------------------------------------

/// The slice of `ISVCDecoder` the benchmark drives. Both implementations go
/// through this so the decode loop itself is shared code, not two copies that
/// could drift apart.
trait Decoder {
    unsafe fn decode(&mut self, src: *const u8, len: i32, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32;
    unsafe fn flush(&mut self, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32;
    unsafe fn signal_end_of_stream(&mut self);
    unsafe fn frames_remaining(&mut self) -> i32;
}

fn decoding_param() -> SDecodingParam {
    let mut param = SDecodingParam::default();
    param.uiTargetDqLayer = u8::MAX;
    param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
    param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
    param
}

struct RustDecoder {
    dec: *mut ISVCDecoder,
}

impl RustDecoder {
    fn new() -> Self {
        unsafe {
            let mut dec: *mut ISVCDecoder = ptr::null_mut();
            let ret = WelsCreateDecoder(&mut dec);
            assert_eq!(ret as i64, CM_RESULT_SUCCESS as i64, "WelsCreateDecoder failed");
            assert!(!dec.is_null());
            let param = decoding_param();
            let init = (*dec).Initialize(&param);
            assert_eq!(init as i64, CM_RESULT_SUCCESS as i64, "Rust decoder Initialize failed");
            Self { dec }
        }
    }
}

impl Decoder for RustDecoder {
    unsafe fn decode(&mut self, src: *const u8, len: i32, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32 {
        unsafe { (*self.dec).DecodeFrame2(src, len, dst, info) as i32 }
    }
    unsafe fn flush(&mut self, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32 {
        unsafe { (*self.dec).FlushFrame(dst, info) as i32 }
    }
    unsafe fn signal_end_of_stream(&mut self) {
        unsafe {
            let mut eos = 1i32;
            (*self.dec).SetOption(
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
                &mut eos as *mut i32 as *mut c_void,
            );
        }
    }
    unsafe fn frames_remaining(&mut self) -> i32 {
        unsafe {
            let mut remaining = 0i32;
            (*self.dec).GetOption(
                DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
                &mut remaining as *mut i32 as *mut c_void,
            );
            remaining
        }
    }
}

impl Drop for RustDecoder {
    fn drop(&mut self) {
        unsafe {
            (*self.dec).Uninitialize();
            WelsDestroyDecoder(self.dec);
        }
    }
}

type CppCreateDecoderFn = unsafe extern "C" fn(ppDecoder: *mut *mut c_void) -> c_long;
type CppDestroyDecoderFn = unsafe extern "C" fn(pDecoder: *mut c_void);
type CppCpuFeatureDetectFn = unsafe extern "C" fn(pNumberOfLogicProcessors: *mut i32) -> u32;

/// `WELS_CPU_NEON` from `codec/common/inc/cpu_core.h`.
const WELS_CPU_NEON: u32 = 0x000004;
/// `WELS_CPU_SSE2` from the same header, for the x86 side of the story.
const WELS_CPU_SSE2: u32 = 0x000080;
type CppInitializeFn = unsafe extern "C" fn(*mut c_void, *const SDecodingParam) -> c_long;
type CppUninitializeFn = unsafe extern "C" fn(*mut c_void) -> c_long;
type CppDecodeFrame2Fn = unsafe extern "C" fn(*mut c_void, *const u8, i32, *mut *mut u8, *mut SBufferInfo) -> i32;
type CppFlushFrameFn = unsafe extern "C" fn(*mut c_void, *mut *mut u8, *mut SBufferInfo) -> i32;
type CppOptionFn = unsafe extern "C" fn(*mut c_void, i32, *mut c_void) -> c_long;

/// `ISVCDecoder` vtable slots, in declaration order from
/// `codec/api/wels/codec_api.h`. The virtual destructor is declared last, so
/// its two Itanium-ABI slots land after `GetOption` and do not shift these.
const VT_INITIALIZE: usize = 0;
const VT_UNINITIALIZE: usize = 1;
const VT_DECODE_FRAME2: usize = 4;
const VT_FLUSH_FRAME: usize = 5;
const VT_SET_OPTION: usize = 8;
const VT_GET_OPTION: usize = 9;

struct CppLibrary {
    _handle: *mut c_void,
    path: PathBuf,
    create_fn: CppCreateDecoderFn,
    destroy_fn: CppDestroyDecoderFn,
    /// What `WelsCPUFeatureDetect` reports for this build. A library whose
    /// hand-written SIMD kernels are linked in but never dispatched decodes
    /// entirely in scalar C, which is a very different thing to benchmark
    /// against -- so the run prints this rather than leaving it to be assumed.
    cpu_flags: Option<u32>,
}

impl CppLibrary {
    fn load() -> Option<Self> {
        let mut root = PathBuf::from("../../../");
        if !root.join("res").exists() {
            root = PathBuf::from("../../");
        }
        let candidates = [
            root.join("libopenh264.dylib"),
            root.join("libopenh264.so"),
            PathBuf::from("/usr/local/lib/libopenh264.dylib"),
            PathBuf::from("/usr/local/lib/libopenh264.so"),
            PathBuf::from("/usr/lib/libopenh264.so"),
        ];

        for path in &candidates {
            if !path.exists() {
                continue;
            }
            let c_path = CString::new(path.to_str().unwrap()).unwrap();
            unsafe {
                let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW);
                if handle.is_null() {
                    continue;
                }
                let create = libc::dlsym(handle, c"WelsCreateDecoder".as_ptr());
                let destroy = libc::dlsym(handle, c"WelsDestroyDecoder".as_ptr());
                if create.is_null() || destroy.is_null() {
                    continue;
                }
                let detect = libc::dlsym(handle, c"WelsCPUFeatureDetect".as_ptr());
                let cpu_flags = (!detect.is_null()).then(|| {
                    let detect =
                        std::mem::transmute::<*mut c_void, CppCpuFeatureDetectFn>(detect);
                    let mut cores = 0i32;
                    detect(&mut cores)
                });
                return Some(Self {
                    _handle: handle,
                    path: path.canonicalize().unwrap_or_else(|_| path.clone()),
                    create_fn: std::mem::transmute::<*mut c_void, CppCreateDecoderFn>(create),
                    destroy_fn: std::mem::transmute::<*mut c_void, CppDestroyDecoderFn>(destroy),
                    cpu_flags,
                });
            }
        }
        None
    }
}

struct CppDecoder<'a> {
    lib: &'a CppLibrary,
    dec: *mut c_void,
    uninitialize: CppUninitializeFn,
    decode_frame2: CppDecodeFrame2Fn,
    flush_frame: CppFlushFrameFn,
    set_option: CppOptionFn,
    get_option: CppOptionFn,
}

impl<'a> CppDecoder<'a> {
    fn new(lib: &'a CppLibrary) -> Self {
        unsafe {
            let mut dec: *mut c_void = ptr::null_mut();
            let ret = (lib.create_fn)(&mut dec);
            assert_eq!(ret, 0, "C++ WelsCreateDecoder failed");
            assert!(!dec.is_null());

            let vtable = *(dec as *mut *mut *const ());
            let initialize: CppInitializeFn = std::mem::transmute(*vtable.add(VT_INITIALIZE));
            let this = Self {
                lib,
                dec,
                uninitialize: std::mem::transmute(*vtable.add(VT_UNINITIALIZE)),
                decode_frame2: std::mem::transmute(*vtable.add(VT_DECODE_FRAME2)),
                flush_frame: std::mem::transmute(*vtable.add(VT_FLUSH_FRAME)),
                set_option: std::mem::transmute(*vtable.add(VT_SET_OPTION)),
                get_option: std::mem::transmute(*vtable.add(VT_GET_OPTION)),
            };

            let param = decoding_param();
            let init = initialize(dec, &param);
            assert_eq!(init, 0, "C++ decoder Initialize failed");
            this
        }
    }
}

impl Decoder for CppDecoder<'_> {
    unsafe fn decode(&mut self, src: *const u8, len: i32, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32 {
        unsafe { (self.decode_frame2)(self.dec, src, len, dst, info) }
    }
    unsafe fn flush(&mut self, dst: *mut *mut u8, info: *mut SBufferInfo) -> i32 {
        unsafe { (self.flush_frame)(self.dec, dst, info) }
    }
    unsafe fn signal_end_of_stream(&mut self) {
        unsafe {
            let mut eos = 1i32;
            (self.set_option)(
                self.dec,
                DECODER_OPTION::DECODER_OPTION_END_OF_STREAM as i32,
                &mut eos as *mut i32 as *mut c_void,
            );
        }
    }
    unsafe fn frames_remaining(&mut self) -> i32 {
        unsafe {
            let mut remaining = 0i32;
            (self.get_option)(
                self.dec,
                DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER as i32,
                &mut remaining as *mut i32 as *mut c_void,
            );
            remaining
        }
    }
}

impl Drop for CppDecoder<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.uninitialize)(self.dec);
            (self.lib.destroy_fn)(self.dec);
        }
    }
}

// ---------------------------------------------------------------------------
// The decode loop, shared by both implementations
// ---------------------------------------------------------------------------

const DS_ERROR_FREE: i32 = DECODING_STATE::dsErrorFree as i32;

fn hash_plane(hasher: &mut Sha1Hasher, plane: *const u8, width: usize, height: usize, stride: usize) {
    if plane.is_null() || width == 0 || height == 0 || stride == 0 {
        return;
    }
    unsafe {
        for y in 0..height {
            hasher.update(std::slice::from_raw_parts(plane.add(y * stride), width));
        }
    }
}

fn hash_frame(hasher: &mut Sha1Hasher, dst: [*mut u8; 3], info: &SBufferInfo) {
    unsafe {
        let buf = info.UsrData.sSystemBuffer;
        let (w, h) = (buf.iWidth as usize, buf.iHeight as usize);
        hash_plane(hasher, dst[0], w, h, buf.iStride[0] as usize);
        hash_plane(hasher, dst[1], w / 2, h / 2, buf.iStride[1] as usize);
        hash_plane(hasher, dst[2], w / 2, h / 2, buf.iStride[1] as usize);
    }
}

struct PassResult {
    frames: usize,
    hash: Option<String>,
    elapsed: Duration,
}

/// Decodes every NAL unit and then drains the decoder, the way the conformance
/// harness does. Hashing is off during timed passes: walking 3 MB of planes per
/// frame costs more than decoding some of them, and would bury the difference
/// the benchmark exists to measure.
fn run_pass<D: Decoder>(dec: &mut D, units: &[&[u8]], verify: bool) -> PassResult {
    let mut hasher = Sha1Hasher::new();
    let mut frames = 0usize;

    let start = Instant::now();
    unsafe {
        let mut consume = |dst: [*mut u8; 3], info: &SBufferInfo, state: i32| {
            if state == DS_ERROR_FREE && info.iBufferStatus == 1 {
                frames += 1;
                if verify {
                    hash_frame(&mut hasher, dst, info);
                }
            }
        };

        for unit in units {
            let mut dst: [*mut u8; 3] = [ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let state = dec.decode(
                black_box(unit.as_ptr()),
                black_box(unit.len() as i32),
                dst.as_mut_ptr(),
                &mut info,
            );
            consume(black_box(dst), black_box(&info), black_box(state));
        }

        // Drain: flag EOS, pull the frame that flushes out, then flush the rest.
        dec.signal_end_of_stream();
        let mut dst: [*mut u8; 3] = [ptr::null_mut(); 3];
        let mut info = SBufferInfo::default();
        let state = dec.decode(ptr::null(), 0, dst.as_mut_ptr(), &mut info);
        consume(dst, &info, state);

        for _ in 0..dec.frames_remaining() {
            let mut dst: [*mut u8; 3] = [ptr::null_mut(); 3];
            let mut info = SBufferInfo::default();
            let state = dec.flush(dst.as_mut_ptr(), &mut info);
            consume(dst, &info, state);
        }
    }
    let elapsed = start.elapsed();

    PassResult {
        frames,
        hash: verify.then(|| hasher.digest()),
        elapsed,
    }
}

struct Measurement {
    frames: usize,
    hash: String,
    best: Duration,
}

impl Measurement {
    fn fps(&self) -> f64 {
        if self.best.is_zero() {
            return 0.0;
        }
        self.frames as f64 / self.best.as_secs_f64()
    }
    fn ms_per_frame(&self) -> f64 {
        if self.frames == 0 {
            return 0.0;
        }
        self.best.as_secs_f64() * 1000.0 / self.frames as f64
    }
}

/// Verifies each implementation once (which also warms the caches), then runs
/// the timed passes *interleaved*, on a fresh decoder every time.
///
/// Interleaving is the point: running all of one implementation's passes before
/// the other lets CPU frequency drift over the run show up as a difference
/// between the two, and on a laptop that drift is the same order as the effect
/// being measured. Each pass reports its best time, since the slow passes are
/// the ones that picked up scheduler noise.
fn measure_pair(
    units: &[&[u8]],
    iters: usize,
    cpp_lib: Option<&CppLibrary>,
) -> (Measurement, Option<Measurement>) {
    let rust_verified = {
        let mut dec = RustDecoder::new();
        run_pass(&mut dec, units, true)
    };
    let cpp_verified = cpp_lib.map(|lib| {
        let mut dec = CppDecoder::new(lib);
        run_pass(&mut dec, units, true)
    });

    let mut rust_best = Duration::MAX;
    let mut cpp_best = Duration::MAX;
    for _ in 0..iters {
        {
            let mut dec = RustDecoder::new();
            let timed = run_pass(&mut dec, units, false);
            assert_eq!(
                timed.frames, rust_verified.frames,
                "Rust decoder returned a different frame count across passes"
            );
            rust_best = rust_best.min(timed.elapsed);
        }
        if let (Some(lib), Some(verified)) = (cpp_lib, cpp_verified.as_ref()) {
            let mut dec = CppDecoder::new(lib);
            let timed = run_pass(&mut dec, units, false);
            assert_eq!(
                timed.frames, verified.frames,
                "C++ decoder returned a different frame count across passes"
            );
            cpp_best = cpp_best.min(timed.elapsed);
        }
    }

    let rust = Measurement {
        frames: rust_verified.frames,
        hash: rust_verified.hash.unwrap_or_default(),
        best: rust_best,
    };
    let cpp = cpp_verified.map(|v| Measurement {
        frames: v.frames,
        hash: v.hash.unwrap_or_default(),
        best: cpp_best,
    });
    (rust, cpp)
}

// ---------------------------------------------------------------------------

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn main() {
    let frames = env_usize("BENCH_FRAMES", 60);
    let iters = env_usize("BENCH_ITERS", 3);
    let pattern = std::env::var("BENCH_PATTERN").unwrap_or_else(|_| "testsrc2".to_string());

    let ffmpeg = resolve_ffmpeg();
    let cpp_lib = CppLibrary::load();

    let rule = "=".repeat(88);
    println!("{rule}");
    println!(" 1080p decode throughput: native C++ OpenH264 vs. Rust port");
    println!("{rule}");
    println!(
        " source     : ffmpeg lavfi {pattern}, {WIDTH}x{HEIGHT}, {frames} frames, x264 @ 5 Mbit/s"
    );
    println!(
        " ffmpeg     : {}",
        ffmpeg
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "NOT FOUND (falling back to cached streams)".into())
    );
    match &cpp_lib {
        Some(lib) => {
            println!(" C++ library: {}", lib.path.display());
            match lib.cpu_flags {
                Some(flags) if flags & (WELS_CPU_NEON | WELS_CPU_SSE2) != 0 => println!(
                    " C++ SIMD   : ACTIVE (WelsCPUFeatureDetect = 0x{flags:06x})"
                ),
                Some(flags) => {
                    println!(" C++ SIMD   : INACTIVE (WelsCPUFeatureDetect = 0x{flags:06x})");
                    println!(
                        "              This build dispatches only scalar C, so the numbers below"
                    );
                    println!(
                        "              compare scalar against scalar -- not against tuned SIMD."
                    );
                }
                None => println!(" C++ SIMD   : UNKNOWN (WelsCPUFeatureDetect not exported)"),
            }
        }
        None => println!(" C++ library: NOT FOUND -- build it with `make` in the repo root (Rust-only run)"),
    }
    println!(" timing     : best of {iters} interleaved passes, decode calls only (no hashing)");

    let mut any_stream = false;
    let mut mismatches = 0;

    for spec in STREAMS {
        println!("{}", "-".repeat(88));
        let Some(path) = ensure_stream(ffmpeg.as_ref(), spec, &pattern, frames) else {
            println!(" {} -- SKIPPED (no stream available)", spec.label);
            continue;
        };
        let data = std::fs::read(&path).expect("cannot read generated stream");
        let units = split_annexb_units(&data);
        any_stream = true;

        println!(
            " {}\n   {:.2} MB bitstream, {} NAL units",
            spec.label,
            data.len() as f64 / (1024.0 * 1024.0),
            units.len()
        );

        let (rust, cpp) = measure_pair(&units, iters, cpp_lib.as_ref());
        println!(
            "   Rust : {:8.2} fps   {:8.3} ms/frame   {} frames",
            rust.fps(),
            rust.ms_per_frame(),
            rust.frames
        );

        let Some(cpp) = cpp else { continue };
        println!(
            "   C++  : {:8.2} fps   {:8.3} ms/frame   {} frames",
            cpp.fps(),
            cpp.ms_per_frame(),
            cpp.frames
        );

        if cpp.frames != rust.frames || cpp.hash != rust.hash {
            mismatches += 1;
            println!("   OUTPUT MISMATCH -- the timings below are not comparable");
            if cpp.frames != rust.frames {
                println!("     frame count: C++ {} vs Rust {}", cpp.frames, rust.frames);
            }
            if cpp.hash != rust.hash {
                println!("     SHA-1 C++  : {}", cpp.hash);
                println!("     SHA-1 Rust : {}", rust.hash);
            }
        } else if cpp.best <= rust.best {
            println!(
                "   ratio: Rust is {:.2}x slower than C++   (output bit-identical, SHA-1 {})",
                rust.best.as_secs_f64() / cpp.best.as_secs_f64().max(f64::MIN_POSITIVE),
                &cpp.hash[..16]
            );
        } else {
            println!(
                "   ratio: Rust is {:.2}x faster than C++   (output bit-identical, SHA-1 {})",
                cpp.best.as_secs_f64() / rust.best.as_secs_f64().max(f64::MIN_POSITIVE),
                &cpp.hash[..16]
            );
        }
    }

    println!("{rule}");
    if !any_stream {
        eprintln!(
            "No streams to decode. Install ffmpeg, or point FFMPEG at it, then re-run."
        );
        std::process::exit(1);
    }
    if mismatches > 0 {
        eprintln!("{mismatches} stream(s) decoded differently by the two implementations.");
        std::process::exit(1);
    }
}
