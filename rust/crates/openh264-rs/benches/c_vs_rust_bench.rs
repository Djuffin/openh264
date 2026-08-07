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
unsafe fn fill_params(enc: *mut ISVCEncoder, width: i32, height: i32, threads: u16) -> SEncParamExt {
    let mut param: SEncParamExt = std::mem::zeroed();
    let vtbl = &*(*enc).lpVtbl;
    (vtbl.GetDefaultParams)(enc, &mut param);
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
    pics: &[SSourcePicture],
) -> RunResult {
    let vtbl = &*(*enc).lpVtbl;
    let param = fill_params(enc, width, height, threads);
    let init_ret = (vtbl.InitializeExt)(enc, &param);
    assert_eq!(init_ret, 0, "InitializeExt failed for {width}x{height} threads={threads}");

    let mut bs_info = SFrameBSInfo::default();

    // Warmup. Deliberately outside the timed loop, and deliberately on the same
    // encoder instance: it primes caches and lets rate control settle, which is what
    // the steady-state numbers below are meant to describe.
    for pic in pics.iter().take(3) {
        let _ = (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs_info));
    }

    let mut full_bitstream = Vec::new();
    let start = Instant::now();
    for pic in pics.iter() {
        let enc_ret = (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs_info));
        black_box(enc_ret);

        let out_len = bs_info.iFrameSizeInBytes as usize;
        let p_buf = bs_info.sLayerInfo[0].pBsBuf;
        if !p_buf.is_null() && out_len > 0 {
            full_bitstream.extend_from_slice(std::slice::from_raw_parts(p_buf, out_len));
        }
    }
    let elapsed = start.elapsed();

    (vtbl.Uninitialize)(enc);

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
    pics: &[SSourcePicture],
) -> RunResult {
    unsafe {
        let mut enc: *mut ISVCEncoder = ptr::null_mut();
        assert_eq!((cpp_lib.create_fn)(&mut enc), 0, "C++ WelsCreateSVCEncoder failed");
        assert!(!enc.is_null());
        let result = run_encoder(enc, width, height, threads, pics);
        (cpp_lib.destroy_fn)(enc);
        result
    }
}

fn run_rust_library_encoder(
    width: i32,
    height: i32,
    threads: u16,
    pics: &[SSourcePicture],
) -> RunResult {
    unsafe {
        let mut enc: *mut ISVCEncoder = ptr::null_mut();
        assert_eq!(WelsCreateSVCEncoder(&mut enc), CM_RESULT_SUCCESS);
        assert!(!enc.is_null());
        let result = run_encoder(enc, width, height, threads, pics);
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

    if let Some(cap) = frame_cap {
        println!(" BENCH_FRAMES={cap}: every configuration capped to {cap} frames.");
    }
    println!(" Threads swept: {thread_counts:?}");

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

        for threads in &thread_counts {
            let threads = *threads;
            let rust = run_rust_library_encoder(w, h, threads, &src_pics);
            let Some(ref cpp) = cpp_lib else {
                println!(
                    "  [{:1} thread] Rust: {:8.2} fps ({:6.3} ms) | {} bytes | SHA-1 {}",
                    threads, rust.fps, rust.latency_ms, rust.bytes, &rust.sha1[..16]
                );
                continue;
            };
            let c = run_c_library_encoder(cpp, w, h, threads, &src_pics);

            // A speedup over work that is not the same work is not a speedup. Report
            // it either way, but never label a mismatched row with one.
            let identical = c.bytes > 0 && rust.bytes > 0 && c.sha1 == rust.sha1;
            let verdict = if identical {
                format!("{:5.2}x [bit-identical]", rust.fps / c.fps)
            } else {
                mismatches.push(format!(
                    "{label} threads={threads}: C++ {} bytes / {}, Rust {} bytes / {}",
                    c.bytes,
                    &c.sha1[..16],
                    rust.bytes,
                    &rust.sha1[..16]
                ));
                format!("MISMATCH ({} vs {} bytes)", c.bytes, rust.bytes)
            };
            println!(
                "  [{:1} thread] C++: {:8.2} fps ({:6.3} ms) | Rust: {:8.2} fps ({:6.3} ms) | {}",
                threads, c.fps, c.latency_ms, rust.fps, rust.latency_ms, verdict
            );
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
