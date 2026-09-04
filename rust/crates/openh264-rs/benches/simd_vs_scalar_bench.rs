//! Rust encoder throughput with the SIMD kernels on versus off.
//!
//! `OPENH264_NO_SIMD=1` is latched once per process (`simd::latch_cpu_features`),
//! and it gates both halves of the dispatch: `WelsCPUFeatureDetect` feeds
//! `uiCpuFlag`, which is what fills the `pfXxx` tables, and `has_sse2` is what the
//! directly-dispatched kernels ask. So the switch is all-or-nothing, and it cannot
//! be flipped inside one run — each side gets its own child process.
//!
//! **Why both sides are children.** The parent generates the source clips, which
//! warms the page cache and the CPU; whichever side ran in-process afterwards
//! would inherit that. Two children, launched the same way, do not. Each side is
//! then run twice in alternating order and the **best** frame rate of the two is
//! reported, which is the usual way to keep a scheduler hiccup or a thermal step
//! from being read as a result.
//!
//! Bit-exactness is checked, not assumed: every SIMD kernel in this port is a
//! parity port of its scalar, so the two bitstreams must hash the same. A speedup
//! over work that is not the same work is not a speedup, and the table says which
//! it got.
//!
//! Environment knobs:
//!
//! | variable | effect |
//! |---|---|
//! | `FFMPEG` | path to the ffmpeg binary (default: `ffmpeg` on `PATH`) |
//! | `BENCH_FRAMES=<n>` | cap every configuration's frame count at `n` |
//! | `BENCH_THREADS=<n>` | `iMultipleThreadIdc` (default `1`, so the kernels are what varies) |
//! | `BENCH_REPEATS=<n>` | runs per side (default `2`); the best frame rate wins |

#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::time::Instant;

#[path = "../tests/common/mod.rs"]
mod common;
use common::Sha1Hasher;

/// One clip: an ffmpeg lavfi source, a label, and a geometry.
const CONFIGS: [(&str, &str, i32, i32, usize); 6] = [
    ("testsrc2", "320x240 QVGA high-contrast", 320, 240, 200),
    ("mandelbrot", "320x240 QVGA mandelbrot", 320, 240, 200),
    ("smptebars", "640x480 VGA SMPTE bars", 640, 480, 100),
    ("mandelbrot", "640x480 VGA mandelbrot", 640, 480, 100),
    ("smptehdbars", "1280x720 720p SMPTE bars", 1280, 720, 50),
    ("mandelbrot", "1280x720 720p mandelbrot", 1280, 720, 50),
];

// ============================================================================
// Source clips
// ============================================================================

fn clip_path(idx: usize, w: i32, h: i32) -> PathBuf {
    std::env::temp_dir().join(format!("openh264_simd_bench_{idx}_{w}x{h}.yuv"))
}

/// Renders one clip to a file both children read, so the two sides are compared
/// over byte-identical input rather than over two runs of a generator.
fn render_clip(pattern: &str, path: &Path, w: i32, h: i32, frames: usize) {
    let frame_size = (w * h * 3 / 2) as usize;
    if fs::metadata(path).is_ok_and(|m| m.len() as usize == frame_size * frames) {
        return;
    }
    let ffmpeg = std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string());
    let out = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("{pattern}=size={w}x{h}:rate=30"),
            "-frames:v",
            &frames.to_string(),
            "-pix_fmt",
            "yuv420p",
            path.to_str().unwrap(),
        ])
        .output();
    let ok = matches!(&out, Ok(o) if o.status.success())
        && fs::metadata(path).is_ok_and(|m| m.len() as usize == frame_size * frames);
    assert!(
        ok,
        "could not render `{pattern}` {w}x{h} with `{ffmpeg}`; set FFMPEG to a working binary.\n\
         This bench needs real pixels: the synthetic noise fallback the C-vs-Rust bench \
         carries would put the encoder on a mode-decision path no clip takes, and the \
         kernel mix is the whole measurement here."
    );
}

// ============================================================================
// One encode
// ============================================================================

struct RunResult {
    fps: f64,
    bytes: usize,
    sha1: String,
}

unsafe fn encode_clip(path: &Path, w: i32, h: i32, frames: usize, threads: u16) -> RunResult {
    let frame_size = (w * h * 3 / 2) as usize;
    let mut raw = fs::read(path).expect("clip");
    let (y_len, uv_len) = ((w * h) as usize, (w * h / 4) as usize);

    let mut pics = Vec::with_capacity(frames);
    for f in 0..frames {
        let off = f * frame_size;
        let mut pic = SSourcePicture::default();
        pic.iColorFormat = EVideoFormatType::videoFormatI420 as i32;
        pic.iPicWidth = w;
        pic.iPicHeight = h;
        pic.iStride = [w, w / 2, w / 2, 0];
        pic.pData[0] = raw[off..off + y_len].as_mut_ptr();
        pic.pData[1] = raw[off + y_len..off + y_len + uv_len].as_mut_ptr();
        pic.pData[2] = raw[off + y_len + uv_len..off + frame_size].as_mut_ptr();
        pics.push(pic);
    }

    let mut enc: *mut ISVCEncoder = ptr::null_mut();
    assert_eq!(unsafe { WelsCreateSVCEncoder(&mut enc) }, CM_RESULT_SUCCESS);
    let vtbl = unsafe { &*(*enc).lpVtbl };

    let mut param: SEncParamExt = unsafe { std::mem::zeroed() };
    unsafe { (vtbl.GetDefaultParams)(enc, &mut param) };
    param.iPicWidth = w;
    param.iPicHeight = h;
    param.fMaxFrameRate = 30.0;
    param.iTargetBitrate = 2_000_000;
    param.iSpatialLayerNum = 1;
    param.iMultipleThreadIdc = threads;
    // Load balancing makes the slice split a function of measured encode *times*
    // (`codec_app_def.h:579`), which would make the two sides' bitstreams differ
    // for reasons that have nothing to do with the kernels.
    param.bUseLoadBalancing = false;
    param.sSpatialLayers[0].iVideoWidth = w;
    param.sSpatialLayers[0].iVideoHeight = h;
    param.sSpatialLayers[0].fFrameRate = 30.0;
    param.sSpatialLayers[0].iSpatialBitrate = 2_000_000;
    let arg = &mut param.sSpatialLayers[0].sSliceArgument;
    arg.uiSliceMode = SliceModeEnum::SM_SINGLE_SLICE;
    arg.uiSliceNum = 1;
    assert_eq!(unsafe { (vtbl.InitializeExt)(enc, &param) }, 0, "InitializeExt {w}x{h}");

    let mut bs = SFrameBSInfo::default();
    for pic in pics.iter().take(3) {
        let _ = unsafe { (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs)) };
    }

    let mut bitstream = Vec::new();
    let start = Instant::now();
    for pic in pics.iter() {
        black_box(unsafe { (vtbl.EncodeFrame)(enc, black_box(pic), black_box(&mut bs)) });
        let len = bs.iFrameSizeInBytes as usize;
        let buf = bs.sLayerInfo[0].pBsBuf;
        if !buf.is_null() && len > 0 {
            bitstream.extend_from_slice(unsafe { std::slice::from_raw_parts(buf, len) });
        }
    }
    let secs = start.elapsed().as_secs_f64();
    unsafe { (vtbl.Uninitialize)(enc) };
    unsafe { WelsDestroySVCEncoder(enc) };

    let mut hasher = Sha1Hasher::new();
    hasher.update(&bitstream);
    RunResult { fps: frames as f64 / secs, bytes: bitstream.len(), sha1: hasher.digest() }
}

// ============================================================================
// Child mode
// ============================================================================

/// Prints one `RESULT` line per configuration for the parent to read back.
fn run_as_child(frame_cap: Option<usize>, threads: u16) {
    println!("CPUFLAGS {:#010x}", openh264_rs::simd::detect_cpu_features());
    for (i, (_, _, w, h, nominal)) in CONFIGS.iter().enumerate() {
        let frames = frame_cap.map_or(*nominal, |c| c.min(*nominal));
        let path = clip_path(i, *w, *h);
        let r = unsafe { encode_clip(&path, *w, *h, frames, threads) };
        println!("RESULT {i} {:.4} {} {}", r.fps, r.bytes, r.sha1);
    }
}

// ============================================================================
// Parent
// ============================================================================

fn spawn_side(no_simd: bool, frame_cap: Option<usize>, threads: u16) -> (u32, Vec<RunResult>) {
    let exe = std::env::current_exe().expect("current_exe");
    let mut cmd = Command::new(exe);
    cmd.env("BENCH_SIMD_CHILD", "1").env("BENCH_THREADS", threads.to_string());
    if let Some(c) = frame_cap {
        cmd.env("BENCH_FRAMES", c.to_string());
    }
    if no_simd {
        cmd.env("OPENH264_NO_SIMD", "1");
    } else {
        cmd.env_remove("OPENH264_NO_SIMD");
    }
    let out = cmd.output().expect("spawn child");
    assert!(
        out.status.success(),
        "child ({}) failed: {}\n{}",
        if no_simd { "scalar" } else { "SIMD" },
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);

    let mut flags = 0u32;
    let mut results = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["CPUFLAGS", v] => flags = u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(0),
            ["RESULT", _, fps, bytes, sha1] => results.push(RunResult {
                fps: fps.parse().unwrap(),
                bytes: bytes.parse().unwrap(),
                sha1: sha1.to_string(),
            }),
            _ => {}
        }
    }
    assert_eq!(results.len(), CONFIGS.len(), "child reported {} of {} rows", results.len(), CONFIGS.len());
    (flags, results)
}

fn feature_names(flags: u32) -> String {
    use openh264_rs::common::cpu_core::*;
    let mut v = Vec::new();
    for (bit, name) in [
        (WELS_CPU_SSE2, "SSE2"),
        (WELS_CPU_SSE3, "SSE3"),
        (WELS_CPU_SSSE3, "SSSE3"),
        (WELS_CPU_SSE41, "SSE4.1"),
        (WELS_CPU_SSE42, "SSE4.2"),
        (WELS_CPU_AVX, "AVX"),
        (WELS_CPU_AVX2, "AVX2"),
        (WELS_CPU_FMA, "FMA"),
    ] {
        if flags & bit != 0 {
            v.push(name);
        }
    }
    if v.is_empty() { "none".to_string() } else { v.join(" ") }
}

fn main() {
    let frame_cap = std::env::var("BENCH_FRAMES").ok().and_then(|v| v.parse::<usize>().ok());
    let threads: u16 = std::env::var("BENCH_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);

    if std::env::var("BENCH_SIMD_CHILD").is_ok() {
        run_as_child(frame_cap, threads);
        return;
    }

    let repeats: usize = std::env::var("BENCH_REPEATS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);

    println!("========================================================================================");
    println!(" Rust encoder throughput: SIMD kernels on vs. OPENH264_NO_SIMD=1");
    println!("========================================================================================");

    for (i, (pattern, label, w, h, nominal)) in CONFIGS.iter().enumerate() {
        let frames = frame_cap.map_or(*nominal, |c| c.min(*nominal));
        println!(" rendering {label}, {frames} frames");
        render_clip(pattern, &clip_path(i, *w, *h), *w, *h, frames);
    }
    println!();

    // Alternate the order across repeats so neither side always runs first, and
    // keep the best of each — a slow run is noise, a fast one is not.
    let (mut simd_best, mut scalar_best): (Vec<RunResult>, Vec<RunResult>) = (Vec::new(), Vec::new());
    let (mut simd_flags, mut scalar_flags) = (0u32, 0u32);
    for pass in 0..repeats.max(1) {
        let scalar_first = pass % 2 == 1;
        let order = if scalar_first { [true, false] } else { [false, true] };
        for no_simd in order {
            println!(
                " pass {}/{}: {} side ...",
                pass + 1,
                repeats.max(1),
                if no_simd { "scalar" } else { "SIMD  " }
            );
            let (flags, rows) = spawn_side(no_simd, frame_cap, threads);
            let (best, best_flags) = if no_simd {
                (&mut scalar_best, &mut scalar_flags)
            } else {
                (&mut simd_best, &mut simd_flags)
            };
            *best_flags = flags;
            if best.is_empty() {
                *best = rows;
            } else {
                for (b, r) in best.iter_mut().zip(rows) {
                    if r.fps > b.fps {
                        *b = r;
                    }
                }
            }
        }
    }

    println!();
    println!(" threads: {threads}   passes: {}   best-of reported", repeats.max(1));
    println!(" SIMD child   CPU flags {simd_flags:#010x}  ({})", feature_names(simd_flags));
    println!(" scalar child CPU flags {scalar_flags:#010x}  ({})", feature_names(scalar_flags));
    assert_eq!(scalar_flags, 0, "OPENH264_NO_SIMD=1 did not clear the feature word");
    println!();
    println!(
        " {:<30} {:>6}  {:>11}  {:>11}  {:>8}  {:>10}",
        "configuration", "frames", "scalar fps", "SIMD fps", "speedup", "bitstream"
    );
    println!("----------------------------------------------------------------------------------------");

    let mut mismatched = 0usize;
    let (mut sum_scalar, mut sum_simd) = (0.0f64, 0.0f64);
    for (i, (_, label, _, _, nominal)) in CONFIGS.iter().enumerate() {
        let frames = frame_cap.map_or(*nominal, |c| c.min(*nominal));
        let (s, v) = (&scalar_best[i], &simd_best[i]);
        let same = s.sha1 == v.sha1 && s.bytes == v.bytes;
        if !same {
            mismatched += 1;
        }
        sum_scalar += s.fps;
        sum_simd += v.fps;
        println!(
            " {:<30} {:>6}  {:>11.1}  {:>11.1}  {:>7.2}x  {:>10}",
            label,
            frames,
            s.fps,
            v.fps,
            v.fps / s.fps,
            if same { "identical" } else { "DIFFERS" }
        );
    }
    println!("----------------------------------------------------------------------------------------");
    let n = CONFIGS.len() as f64;
    let geomean = ((0..CONFIGS.len())
        .map(|i| (simd_best[i].fps / scalar_best[i].fps).ln())
        .sum::<f64>()
        / n)
        .exp();
    println!(
        " {:<30} {:>6}  {:>11.1}  {:>11.1}  {:>7.2}x",
        "mean (speedup is geometric)", "", sum_scalar / n, sum_simd / n, geomean
    );

    if mismatched > 0 {
        eprintln!();
        eprintln!(
            " {mismatched} configuration(s) produced different bitstreams with and without SIMD."
        );
        eprintln!(" Every kernel here is a parity port, so this is a bug, not a tuning result.");
        std::process::exit(1);
    }
}
