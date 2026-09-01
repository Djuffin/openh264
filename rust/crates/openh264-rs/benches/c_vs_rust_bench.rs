//! In-Process Side-by-Side Performance & Bitstream SHA-1 Hash Comparison:
//! Native C++ OpenH264 Library (libopenh264.so) vs. Rust OpenH264 Encoder (openh264-rs).
//!
//! Both encoders are driven through the same call sequence — `GetDefaultParams`,
//! `InitializeExt`, then `EncodeFrame` per frame — over the same `SSourcePicture`
//! array, so the only variable is the implementation. Every row reports the SHA-1
//! of the whole bitstream alongside the timing: a speedup over work that is not
//! byte-for-byte the same work is not a speedup, and the row says so.
//!
//! Environment knobs:
//!
//! | variable | effect |
//! |---|---|
//! | `FFMPEG` | path to the ffmpeg binary (default: `ffmpeg` on `PATH`) |
//! | `BENCH_REQUIRE_FFMPEG=1` | abort rather than fall back to the synthetic pattern |
//! | `BENCH_FRAMES=<n>` | cap every configuration's frame count at `n` |
//! | `BENCH_THREADS=<a,b>` | `iMultipleThreadIdc` values to sweep (default `1,4`) |
//! | `BENCH_SLICE_MODE=<m[:n],..>` | slice-mode axis (default `0`, i.e. `SM_SINGLE_SLICE`) |
//! | `BENCH_LOAD_BALANCING=0\|1` | override `bUseLoadBalancing` (default: leave `GetDefaultParams`' value) |
//!
//! **F68** (`phase7_findings.md`): before the slice-mode knob existed this bench
//! set `iMultipleThreadIdc` and nothing else, so `GetDefaultParams`' `SM_SINGLE_SLICE`
//! survived into every row — and `SM_SINGLE_SLICE` is the first arm of the slice-mode
//! chain in `WelsEncoderEncodeExt`, tested before any thread-count condition. Every
//! `[4 thread]` row this bench has ever printed, on **both** sides, encoded on the
//! calling thread. The flat 0.0-1.4% "speedup" at every resolution was the signature
//! of a path that never ran. `BENCH_SLICE_MODE` is a knob rather than an edit so the
//! `sm=0` rows stay comparable with every span already in `perf_baseline.md`.
//!
//! **`BENCH_LOAD_BALANCING`, and why it exists.** `GetDefaultParams` sets
//! `bUseLoadBalancing = true` on both sides. With `uiSliceMode = 1` and
//! `iMultipleThreadIdc >= uiSliceNum` that reaches `AdjustBaseLayer` →
//! `DynamicAdjustSlicing`, whose slice boundaries for frame N+1 are computed from
//! frame N's measured per-slice encode *times* — so the bitstream is a function of
//! the schedule. The C++ header says so itself (`codec_app_def.h:579`: the result of
//! each run may be different), and two consecutive runs of this bench confirm it —
//! the **C++** side alone returns a different byte count every time. A row on that
//! path can never be bit-identical, so `BENCH_LOAD_BALANCING=0` is what a
//! byte-checked multi-slice span wants; it is also the configuration the diffharness
//! gates (`cxx_enc.cpp:119` sets it false). Leaving the knob unset keeps every
//! historical row exactly as it was.
//!
//! Exits non-zero if any configuration's bitstreams disagree, after running and
//! reporting all of them — one mismatch should not cost you the other 29 rows.

#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use std::fs;
use std::hint::black_box;
use std::os::raw::c_void;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;
use std::time::Instant;

type CppWelsCreateSVCEncoderFn = unsafe extern "C" fn(ppEncoder: *mut *mut ISVCEncoder) -> i32;
type CppWelsDestroySVCEncoderFn = unsafe extern "C" fn(pEncoder: *mut ISVCEncoder);

fn workspace_root() -> PathBuf {
    let mut root = PathBuf::from("../../../");
    if !root.join("res").exists() {
        root = PathBuf::from("../../");
    }
    root
}

#[path = "../tests/common/mod.rs"]
mod common;
use common::Sha1Hasher;

fn compute_sha1(data: &[u8]) -> String {
    let mut hasher = Sha1Hasher::new();
    hasher.update(data);
    hasher.digest()
}

/// Where a configuration's pixels came from. The synthetic fallback is high-entropy
/// noise — roughly worst-case for an encoder and not representative of video — so a
/// run that silently used it would report throughput for a workload nobody has.
#[derive(Clone, Copy, PartialEq)]
enum InputSource {
    Ffmpeg,
    Synthetic,
}

fn generate_ffmpeg_pattern(
    pattern_name: &str,
    width: i32,
    height: i32,
    num_frames: usize,
) -> (Vec<u8>, InputSource) {
    let temp_file = std::env::temp_dir().join(format!("bench_lib_{}_{}x{}.yuv", pattern_name, width, height));
    let frame_size = (width * height * 3 / 2) as usize;

    let ffmpeg = std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string());
    let filter_expr = format!("{}=size={}x{}:rate=30", pattern_name, width, height);
    let status = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &filter_expr,
            "-frames:v",
            &num_frames.to_string(),
            "-pix_fmt",
            "yuv420p",
            temp_file.to_str().unwrap(),
        ])
        .output();

    let reason = match &status {
        Err(e) => format!("could not run `{ffmpeg}`: {e}"),
        Ok(output) if !output.status.success() => {
            format!("`{ffmpeg}` exited {}", output.status)
        }
        Ok(_) => match fs::read(&temp_file) {
            Err(e) => format!("could not read {}: {e}", temp_file.display()),
            Ok(bytes) => {
                let _ = fs::remove_file(&temp_file);
                if bytes.len() == frame_size * num_frames {
                    return (bytes, InputSource::Ffmpeg);
                }
                format!(
                    "{} produced {} bytes, expected {}",
                    temp_file.display(),
                    bytes.len(),
                    frame_size * num_frames
                )
            }
        },
    };

    if std::env::var("BENCH_REQUIRE_FFMPEG").is_ok_and(|v| v != "0") {
        panic!("BENCH_REQUIRE_FFMPEG is set and lavfi source `{pattern_name}` is unavailable: {reason}");
    }
    eprintln!("  !! lavfi `{pattern_name}` unavailable ({reason}); using the synthetic pattern");

    let mut buffer = vec![128u8; frame_size * num_frames];
    for f in 0..num_frames {
        let frame_offset = f * frame_size;
        for y in 0..height as usize {
            for x in 0..width as usize {
                buffer[frame_offset + y * width as usize + x] = ((x.wrapping_mul(x) ^ y.wrapping_mul(y) ^ (f * 17)) & 0xFF) as u8;
            }
        }
    }
    (buffer, InputSource::Synthetic)
}

struct CppLibrary {
    _handle: *mut c_void,
    create_fn: CppWelsCreateSVCEncoderFn,
    destroy_fn: CppWelsDestroySVCEncoderFn,
}

impl CppLibrary {
    pub fn load() -> Option<Self> {
        let root = workspace_root();
        let lib_paths = [
            root.join("libopenh264.so"),
            root.join("libopenh264.dylib"),
            PathBuf::from("/usr/local/lib/libopenh264.so"),
            PathBuf::from("/usr/lib/libopenh264.so"),
        ];

        for path in &lib_paths {
            if !path.exists() {
                continue;
            }
            let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
            unsafe {
                let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW);
                if !handle.is_null() {
                    let create_sym = libc::dlsym(handle, b"WelsCreateSVCEncoder\0".as_ptr() as *const _);
                    let destroy_sym = libc::dlsym(handle, b"WelsDestroySVCEncoder\0".as_ptr() as *const _);
                    if !create_sym.is_null() && !destroy_sym.is_null() {
                        return Some(Self {
                            _handle: handle,
                            create_fn: std::mem::transmute(create_sym),
                            destroy_fn: std::mem::transmute(destroy_sym),
                        });
                    }
                }
            }
        }
        None
    }
}

/// One entry of the slice-mode axis (F68).
///
/// `SM_SINGLE_SLICE` is the default so an unset `BENCH_SLICE_MODE` reproduces every
/// row this bench printed before the knob existed, byte for byte and number for
/// number. The multi-slice entries are what actually reach the threaded paths:
/// `SM_FIXEDSLCNUM_SLICE` takes the fork/join dispatch, `SM_SIZELIMITED_SLICE` the
/// dynamic one.
#[derive(Clone, Copy, PartialEq)]
struct SliceSpec {
    mode: SliceModeEnum,
    /// `uiSliceNum` for modes 1 and 2 (**0 means "one slice per thread"**, resolved
    /// per row); `uiSliceSizeConstraint` in bytes for mode 3.
    arg: u32,
}

impl SliceSpec {
    const DEFAULT: SliceSpec = SliceSpec { mode: SliceModeEnum::SM_FIXEDSLCNUM_SLICE, arg: 0 };

    /// `m` or `m:n` — `1:4` is four fixed slices, `1` is one per thread, `3` is
    /// size-limited at the 1500-byte default, `3:600` at 600.
    fn parse(spec: &str) -> Option<SliceSpec> {
        let (m, n) = match spec.trim().split_once(':') {
            Some((m, n)) => (m.trim(), n.trim().parse::<u32>().ok()?),
            None => (spec.trim(), 0),
        };
        let mode = match m {
            "0" => SliceModeEnum::SM_SINGLE_SLICE,
            "1" => SliceModeEnum::SM_FIXEDSLCNUM_SLICE,
            "2" => SliceModeEnum::SM_RASTER_SLICE,
            "3" => SliceModeEnum::SM_SIZELIMITED_SLICE,
            _ => return None,
        };
        let arg = match (mode, n) {
            (SliceModeEnum::SM_SINGLE_SLICE, _) => 1,
            (SliceModeEnum::SM_SIZELIMITED_SLICE, 0) => 1500,
            (_, n) => n,
        };
        Some(SliceSpec { mode, arg })
    }

    fn label(&self, threads: u16) -> String {
        match self.mode {
            SliceModeEnum::SM_SINGLE_SLICE => "sm=0".to_string(),
            SliceModeEnum::SM_SIZELIMITED_SLICE => format!("sm=3 c={}", self.arg),
            _ => format!(
                "sm={} n={}",
                self.mode as i32,
                if self.arg == 0 { threads.max(1) as u32 } else { self.arg }
            ),
        }
    }
}

/// One measured configuration's result.
struct RunResult {
    fps: f64,
    latency_ms: f64,
    bytes: usize,
    sha1: String,
}

/// The parameter set both encoders are initialized with. Kept in one place so the
/// two sides cannot drift: a benchmark whose halves configure differently is
/// measuring two different questions.
unsafe fn fill_params(
    enc: *mut ISVCEncoder,
    width: i32,
    height: i32,
    threads: u16,
    slice: SliceSpec,
    load_balancing: Option<bool>,
) -> SEncParamExt {
    let mut param: SEncParamExt = unsafe { std::mem::zeroed() };
    let vtbl = unsafe { &*(*enc).lpVtbl };
    unsafe { (vtbl.GetDefaultParams)(enc, &mut param) };
    param.iPicWidth = width;
    param.iPicHeight = height;
    param.fMaxFrameRate = 30.0;
    param.iTargetBitrate = 2_000_000;
    param.iSpatialLayerNum = 1;
    param.iMultipleThreadIdc = threads;
    param.sSpatialLayers[0].iVideoWidth = width;
    param.sSpatialLayers[0].iVideoHeight = height;
    param.sSpatialLayers[0].fFrameRate = 30.0;
    param.sSpatialLayers[0].iSpatialBitrate = 2_000_000;
    // F68. `GetDefaultParams` leaves `uiSliceMode` at `SM_SINGLE_SLICE`, and until
    // this block existed nothing here raised it — so the thread axis measured the
    // single-threaded path at every count. The arms mirror `cxx_enc.cpp:144`, which
    // is what the byte harness drives.
    if let Some(lb) = load_balancing {
        param.bUseLoadBalancing = lb;
    }
    let arg = &mut param.sSpatialLayers[0].sSliceArgument;
    arg.uiSliceMode = slice.mode;
    match slice.mode {
        SliceModeEnum::SM_SIZELIMITED_SLICE => {
            arg.uiSliceSizeConstraint = slice.arg;
        }
        SliceModeEnum::SM_SINGLE_SLICE => {
            arg.uiSliceNum = 1;
        }
        _ => {
            // 0 = "one slice per thread", which is the shape that makes the thread
            // axis mean something; anything else is taken literally.
            arg.uiSliceNum = if slice.arg == 0 { threads.max(1) as u32 } else { slice.arg };
            if slice.mode == SliceModeEnum::SM_RASTER_SLICE {
                arg.uiSliceMbNum[0] = arg.uiSliceNum;
            }
        }
    }
    param
}

/// Initialize `enc`, encode `pics`, and return timing plus the bitstream hash.
///
/// Both implementations run through this one function, reached through the same
/// `ISVCEncoderVtbl`. For the dlopen'd C++ library that vtable is the real Itanium-ABI
/// vtable of `ISVCEncoder`: its nine pure-virtual methods occupy slots 0..8 in
/// declaration order, and the virtual destructor is declared last so its slots land
/// after them. (`ForceIntraFrame` is the one member whose Rust signature is not
/// call-compatible with C++ — the C++ declaration has a defaulted second parameter.
/// Nothing here calls it.)
unsafe fn run_encoder(
    enc: *mut ISVCEncoder,
    width: i32,
    height: i32,
    threads: u16,
    slice: SliceSpec,
    load_balancing: Option<bool>,
    pics: &[SSourcePicture],
) -> RunResult {
    let vtbl = unsafe { &*(*enc).lpVtbl };
    let param = unsafe { fill_params(enc, width, height, threads, slice, load_balancing) };
    let init_ret = unsafe { (vtbl.InitializeExt)(enc, &param) };
    assert_eq!(
        init_ret,
        0,
        "InitializeExt failed for {width}x{height} threads={threads} {}",
        slice.label(threads)
    );

    let mut bs_info = SFrameBSInfo::default();

    // Warmup. Deliberately outside the timed loop, and deliberately on the same
    // encoder instance: it primes caches and lets rate control settle, which is what
    // the steady-state numbers below are meant to describe.
    for pic in pics.iter().take(3) {
        let _ = unsafe { (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs_info)) };
    }

    let mut full_bitstream = Vec::new();
    let start = Instant::now();
    for pic in pics.iter() {
        let enc_ret = unsafe { (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs_info)) };
        black_box(enc_ret);

        let out_len = bs_info.iFrameSizeInBytes as usize;
        let p_buf = bs_info.sLayerInfo[0].pBsBuf;
        if !p_buf.is_null() && out_len > 0 {
            full_bitstream.extend_from_slice(unsafe { std::slice::from_raw_parts(p_buf, out_len) });
        }
    }
    let elapsed = start.elapsed();

    unsafe { (vtbl.Uninitialize)(enc) };

    let total_secs = elapsed.as_secs_f64();
    RunResult {
        fps: (pics.len() as f64) / total_secs,
        latency_ms: (total_secs * 1_000.0) / (pics.len() as f64),
        bytes: full_bitstream.len(),
        sha1: compute_sha1(&full_bitstream),
    }
}

fn run_c_library_encoder(
    cpp_lib: &CppLibrary,
    width: i32,
    height: i32,
    threads: u16,
    slice: SliceSpec,
    load_balancing: Option<bool>,
    pics: &[SSourcePicture],
) -> RunResult {
    unsafe {
        let mut enc: *mut ISVCEncoder = ptr::null_mut();
        assert_eq!((cpp_lib.create_fn)(&mut enc), 0, "C++ WelsCreateSVCEncoder failed");
        assert!(!enc.is_null());
        let result = run_encoder(enc, width, height, threads, slice, load_balancing, pics);
        (cpp_lib.destroy_fn)(enc);
        result
    }
}

fn run_rust_library_encoder(
    width: i32,
    height: i32,
    threads: u16,
    slice: SliceSpec,
    load_balancing: Option<bool>,
    pics: &[SSourcePicture],
) -> RunResult {
    unsafe {
        let mut enc: *mut ISVCEncoder = ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        assert!(!enc.is_null());
        let result = run_encoder(enc, width, height, threads, slice, load_balancing, pics);
        WelsDestroySVCEncoder(enc);
        result
    }
}

fn main() {
    let cpp_lib = CppLibrary::load();

    println!("========================================================================================================================");
    println!(" Side-by-Side In-Memory Benchmark: Native C++ OpenH264 vs. Translated openh264-rs");
    if cpp_lib.is_some() {
        println!(" C++ OpenH264 Dynamic Library (libopenh264.so): LOADED");
    } else {
        println!(" C++ OpenH264 Dynamic Library (libopenh264.so): NOT FOUND (Benchmarking Rust Encoder Performance)");
    }
    println!("========================================================================================================================");

    let test_inputs = [
        ("testsrc", "320x240 (QVGA Moving Box)", 320, 240, 200),
        ("testsrc2", "320x240 (QVGA High-Contrast)", 320, 240, 200),
        ("smptebars", "320x240 (QVGA SMPTE Bars)", 320, 240, 200),
        ("pal75bars", "320x240 (QVGA PAL 75%)", 320, 240, 200),
        ("rgbtestsrc", "320x240 (QVGA RGB Test)", 320, 240, 200),
        ("yuvtestsrc", "320x240 (QVGA YUV Space)", 320, 240, 200),
        ("gradients", "320x240 (QVGA Spatial Ramps)", 320, 240, 200),
        ("mandelbrot", "320x240 (QVGA Mandelbrot)", 320, 240, 200),
        ("mandelbrot", "640x480 (VGA Mandelbrot)", 640, 480, 100),
        ("smptebars", "640x480 (VGA SMPTE Bars)", 640, 480, 100),
        ("mandelbrot", "1280x720 (720p HD Mandelbrot)", 1280, 720, 50),
        ("smptehdbars", "1280x720 (720p HD SMPTE Bars)", 1280, 720, 50),
        ("mandelbrot", "1920x1080 (1080p Full HD Mandelbrot)", 1920, 1080, 30),
        ("smptehdbars", "1920x1080 (1080p Full HD SMPTE Bars)", 1920, 1080, 30),
        ("testsrc", "1920x1080 (1080p Full HD Testsrc)", 1920, 1080, 30),
    ];

    // `BENCH_FRAMES` caps every configuration. Useful while a known bitstream
    // divergence sits partway into a sequence: capping below it gives comparable
    // work on both sides and therefore meaningful timings, at the cost of coverage.
    let frame_cap: Option<usize> = std::env::var("BENCH_FRAMES").ok().and_then(|v| v.parse().ok());
    let thread_counts: Vec<u16> = std::env::var("BENCH_THREADS")
        .ok()
        .map(|v| v.split(',').filter_map(|t| t.trim().parse().ok()).collect())
        .filter(|v: &Vec<u16>| !v.is_empty())
        .unwrap_or_else(|| vec![1, 4]);
    // F68's knob. Unset = the one default entry, which is what every span in
    // `perf_baseline.md` before 2026-08-20 measured.
    let slice_specs: Vec<SliceSpec> = std::env::var("BENCH_SLICE_MODE")
        .ok()
        .map(|v| v.split(',').filter_map(SliceSpec::parse).collect())
        .filter(|v: &Vec<SliceSpec>| !v.is_empty())
        .unwrap_or_else(|| vec![SliceSpec::DEFAULT]);
    // Only tag the rows when the axis actually has something on it, so an unset
    // `BENCH_SLICE_MODE` prints the exact row text the ledger's history is written in.
    let tag_rows = slice_specs.len() > 1 || slice_specs[0] != SliceSpec::DEFAULT;
    // Default to false for deterministic multi-slice bitstream comparison, overrideable by BENCH_LOAD_BALANCING.
    let load_balancing: Option<bool> = Some(
        std::env::var("BENCH_LOAD_BALANCING")
            .ok()
            .map_or(false, |v| v.trim() != "0")
    );

    if let Some(cap) = frame_cap {
        println!(" BENCH_FRAMES={cap}: every configuration capped to {cap} frames.");
    }
    println!(" Threads swept: {thread_counts:?}");
    if tag_rows {
        let labels: Vec<String> = slice_specs.iter().map(|s| s.label(0)).collect();
        println!(" Slice modes swept: {}", labels.join(", "));
    }
    if let Some(lb) = load_balancing {
        println!(" bUseLoadBalancing forced to {lb}");
    }

    let mut mismatches: Vec<String> = Vec::new();
    let mut synthetic_used = false;

    for (pattern, label, w, h, nominal_frames) in test_inputs {
        let frames = frame_cap.map_or(nominal_frames, |c| c.min(nominal_frames));
        let frame_size = (w * h * 3 / 2) as usize;
        let (mut raw_yuv, source) = generate_ffmpeg_pattern(pattern, w, h, frames);
        synthetic_used |= source == InputSource::Synthetic;

        let y_len = (w * h) as usize;
        let uv_len = (w * h / 4) as usize;

        let mut src_pics = Vec::with_capacity(frames);

        for frame_idx in 0..frames {
            let offset = frame_idx * frame_size;
            let mut pic = SSourcePicture::default();
            pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
            pic.iPicWidth = w;
            pic.iPicHeight = h;
            pic.iStride[0] = w;
            pic.iStride[1] = w / 2;
            pic.iStride[2] = w / 2;
            pic.pData[0] = raw_yuv[offset..offset + y_len].as_mut_ptr();
            pic.pData[1] = raw_yuv[offset + y_len..offset + y_len + uv_len].as_mut_ptr();
            pic.pData[2] = raw_yuv[offset + y_len + uv_len..offset + frame_size].as_mut_ptr();
            src_pics.push(pic);
        }

        let source_tag = match source {
            InputSource::Ffmpeg => "lavfi",
            InputSource::Synthetic => "SYNTHETIC",
        };
        println!("------------------------------------------------------------------------------------------------------------------------");
        println!(
            " {:<38} {:>4} frames, {:.2} MB/frame, source: {}",
            label,
            frames,
            (frame_size as f64) / (1024.0 * 1024.0),
            source_tag
        );
        println!("------------------------------------------------------------------------------------------------------------------------");

        for spec in &slice_specs {
            let spec = *spec;
            for threads in &thread_counts {
                let threads = *threads;
                let row = if tag_rows {
                    format!("{:1} thread {}", threads, spec.label(threads))
                } else {
                    format!("{threads:1} thread")
                };
                let rust = run_rust_library_encoder(w, h, threads, spec, load_balancing, &src_pics);
                let Some(ref cpp) = cpp_lib else {
                    println!(
                        "  [{}] Rust: {:8.2} fps ({:6.3} ms) | {} bytes | SHA-1 {}",
                        row, rust.fps, rust.latency_ms, rust.bytes, &rust.sha1[..16]
                    );
                    continue;
                };
                let c = run_c_library_encoder(cpp, w, h, threads, spec, load_balancing, &src_pics);

                // A speedup over work that is not the same work is not a speedup. Report
                // it either way, but never label a mismatched row with one.
                let identical = c.bytes > 0 && rust.bytes > 0 && c.sha1 == rust.sha1;
                let verdict = if identical {
                    format!("{:5.2}x [bit-identical]", rust.fps / c.fps)
                } else {
                    mismatches.push(format!(
                        "{label} threads={threads} {}: C++ {} bytes / {}, Rust {} bytes / {}",
                        spec.label(threads),
                        c.bytes,
                        &c.sha1[..16],
                        rust.bytes,
                        &rust.sha1[..16]
                    ));
                    format!("MISMATCH ({} vs {} bytes)", c.bytes, rust.bytes)
                };
                println!(
                    "  [{}] C++: {:8.2} fps ({:6.3} ms) | Rust: {:8.2} fps ({:6.3} ms) | {}",
                    row, c.fps, c.latency_ms, rust.fps, rust.latency_ms, verdict
                );
            }
        }
    }
    println!("========================================================================================================================");

    if synthetic_used {
        println!(" NOTE: at least one configuration fell back to the synthetic pattern — high-entropy");
        println!("       noise, roughly worst case for an encoder. Install ffmpeg, or set FFMPEG, for");
        println!("       representative throughput. BENCH_REQUIRE_FFMPEG=1 turns the fallback into an error.");
    }
    if !mismatches.is_empty() {
        println!();
        println!(" {} configuration(s) produced different bitstreams:", mismatches.len());
        for m in &mismatches {
            println!("   - {m}");
        }
        std::process::exit(1);
    }
}
