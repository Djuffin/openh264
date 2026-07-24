//! In-Process Side-by-Side Performance & Bitstream SHA-1 Hash Comparison:
//! Native C++ OpenH264 Library (libopenh264.so) vs. Rust OpenH264 Encoder (openh264-rs).

#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use std::fs;
use std::hint::black_box;
use std::os::raw::c_void;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;
use std::time::Instant;

type CppWelsCreateSVCEncoderFn = unsafe extern "C" fn(ppEncoder: *mut *mut c_void) -> i32;
type CppWelsDestroySVCEncoderFn = unsafe extern "C" fn(pEncoder: *mut c_void);

type CppGetDefaultParamsFn = unsafe extern "C" fn(this: *mut c_void, param: *mut SEncParamExt) -> i32;
type CppInitializeExtFn = unsafe extern "C" fn(this: *mut c_void, param: *const SEncParamExt) -> i32;
type CppUninitializeFn = unsafe extern "C" fn(this: *mut c_void) -> i32;
type CppEncodeFrameFn = unsafe extern "C" fn(
    this: *mut c_void,
    kp_src_pic: *const SSourcePicture,
    p_bs_info: *mut SFrameBSInfo,
) -> i32;

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

fn generate_ffmpeg_pattern(pattern_name: &str, width: i32, height: i32, num_frames: usize) -> Vec<u8> {
    let temp_file = std::env::temp_dir().join(format!("bench_lib_{}_{}x{}.yuv", pattern_name, width, height));
    let frame_size = (width * height * 3 / 2) as usize;

    let filter_expr = format!("{}=size={}x{}:rate=30", pattern_name, width, height);
    let status = Command::new("ffmpeg")
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

    if let Ok(output) = status {
        if output.status.success() {
            if let Ok(bytes) = fs::read(&temp_file) {
                let _ = fs::remove_file(&temp_file);
                if bytes.len() == frame_size * num_frames {
                    return bytes;
                }
            }
        }
    }

    let mut buffer = vec![128u8; frame_size * num_frames];
    for f in 0..num_frames {
        let frame_offset = f * frame_size;
        for y in 0..height as usize {
            for x in 0..width as usize {
                buffer[frame_offset + y * width as usize + x] = ((x.wrapping_mul(x) ^ y.wrapping_mul(y) ^ (f * 17)) & 0xFF) as u8;
            }
        }
    }
    buffer
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

fn run_c_library_encoder(
    cpp_lib: &CppLibrary,
    width: i32,
    height: i32,
    num_frames: usize,
    threads: u16,
    c_pics: &[SSourcePicture],
) -> (f64, f64, usize, String) {
    unsafe {
        let mut p_encoder: *mut c_void = ptr::null_mut();
        let ret = (cpp_lib.create_fn)(&mut p_encoder);
        assert_eq!(ret, 0);

        let vtable_ptr = *(p_encoder as *mut *mut *const ());
        let get_default_params_fn: CppGetDefaultParamsFn = std::mem::transmute(*vtable_ptr.add(2));
        let initialize_ext_fn: CppInitializeExtFn = std::mem::transmute(*vtable_ptr.add(1));
        let uninitialize_fn: CppUninitializeFn = std::mem::transmute(*vtable_ptr.add(3));
        let encode_frame_fn: CppEncodeFrameFn = std::mem::transmute(*vtable_ptr.add(4));

        let mut param: SEncParamExt = std::mem::zeroed();
        let _ = get_default_params_fn(p_encoder, &mut param);
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

        let init_ret = initialize_ext_fn(p_encoder, &param);
        assert_eq!(init_ret, 0);

        let mut bs_info = SFrameBSInfo::default();

        // Warmup
        for pic in c_pics.iter().take(3) {
            let _ = encode_frame_fn(p_encoder, black_box(pic), black_box(&mut bs_info));
        }

        let mut full_bitstream = Vec::new();

        let start = Instant::now();
        for pic in c_pics.iter() {
            let enc_ret = encode_frame_fn(p_encoder, black_box(pic), black_box(&mut bs_info));
            black_box(enc_ret);

            let out_len = bs_info.iFrameSizeInBytes as usize;
            let p_buf = bs_info.sLayerInfo[0].pBsBuf;
            if !p_buf.is_null() && out_len > 0 {
                let slice = std::slice::from_raw_parts(p_buf, out_len);
                full_bitstream.extend_from_slice(slice);
            }
        }
        let elapsed = start.elapsed();

        let sha1_hash = compute_sha1(&full_bitstream);
        let total_bytes = full_bitstream.len();

        uninitialize_fn(p_encoder);
        (cpp_lib.destroy_fn)(p_encoder);

        let total_secs = elapsed.as_secs_f64();
        let fps = (num_frames as f64) / total_secs;
        let avg_latency_ms = (total_secs * 1_000.0) / (num_frames as f64);
        (fps, avg_latency_ms, total_bytes, sha1_hash)
    }
}

fn run_rust_library_encoder(
    width: i32,
    height: i32,
    num_frames: usize,
    threads: u16,
    src_pics: &[SSourcePicture],
) -> (f64, f64, usize, String) {
    unsafe {
        let mut p_encoder: *mut ISVCEncoder = ptr::null_mut();
        let ret = WelsCreateSVCEncoder(&mut p_encoder);
        assert_eq!(ret, CM_RESULT_SUCCESS);
        assert!(!p_encoder.is_null());

        let mut param: SEncParamExt = std::mem::zeroed();
        let _ = (*p_encoder).GetDefaultParams(&mut param);
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

        let init_ret = (*p_encoder).InitializeExt(&param);
        assert_eq!(init_ret, CM_RESULT_SUCCESS);

        let mut bs_info = SFrameBSInfo::default();

        // Warmup
        for pic in src_pics.iter().take(3) {
            let _ = (*p_encoder).EncodeFrame(black_box(pic), black_box(&mut bs_info));
        }

        let mut full_bitstream = Vec::new();

        let start = Instant::now();
        for pic in src_pics.iter() {
            let enc_ret = (*p_encoder).EncodeFrame(black_box(pic), black_box(&mut bs_info));
            black_box(enc_ret);

            let out_len = bs_info.iFrameSizeInBytes as usize;
            let p_buf = bs_info.sLayerInfo[0].pBsBuf;
            if !p_buf.is_null() && out_len > 0 {
                let slice = std::slice::from_raw_parts(p_buf, out_len);
                full_bitstream.extend_from_slice(slice);
            }
        }
        let elapsed = start.elapsed();

        let sha1_hash = compute_sha1(&full_bitstream);
        let total_bytes = full_bitstream.len();

        (*p_encoder).Uninitialize();
        WelsDestroySVCEncoder(p_encoder);

        let total_secs = elapsed.as_secs_f64();
        let fps = (num_frames as f64) / total_secs;
        let avg_latency_ms = (total_secs * 1_000.0) / (num_frames as f64);
        (fps, avg_latency_ms, total_bytes, sha1_hash)
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

    for (pattern, label, w, h, frames) in test_inputs {
        let frame_size = (w * h * 3 / 2) as usize;
        let mut raw_yuv = generate_ffmpeg_pattern(pattern, w, h, frames);

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

        println!("------------------------------------------------------------------------------------------------------------------------");
        println!(" Resolution: {:<38} (Frame Size: {:.2} MB)", label, (frame_size as f64) / (1024.0 * 1024.0));
        println!("------------------------------------------------------------------------------------------------------------------------");

        for threads in [1u16, 4u16] {
            let (rust_fps, rust_lat, _rust_bytes, rust_hash) = run_rust_library_encoder(w, h, frames, threads, &src_pics);
            if let Some(ref cpp) = cpp_lib {
                let (c_fps, c_lat, c_bytes, c_hash) = run_c_library_encoder(cpp, w, h, frames, threads, &src_pics);
                let speedup = if c_fps > 0.0 { rust_fps / c_fps } else { 1.0 };
                if c_bytes > 0 && _rust_bytes > 0 {
                    assert_eq!(
                        c_hash, rust_hash,
                        "FATAL: SHA-1 Bitstream mismatch on {} (threads: {})! C++={}, Rust={}",
                        label, threads, c_hash, rust_hash
                    );
                    println!(
                        "  [{:1} Thread(s)] C++: {:8.2} FPS ({:6.3} ms) | Safe Rust: {:8.2} FPS ({:6.3} ms) | Speedup: {:5.2}x [Bit-Identical]",
                        threads, c_fps, c_lat, rust_fps, rust_lat, speedup
                    );
                } else {
                    println!(
                        "  [{:1} Thread(s)] C++: {:8.2} FPS ({:6.3} ms, {} bytes) | Safe Rust: {:8.2} FPS ({:6.3} ms, {} bytes)",
                        threads, c_fps, c_lat, c_bytes, rust_fps, rust_lat, _rust_bytes
                    );
                }
            } else {
                println!(
                    "  [{:1} Thread(s)] Safe Rust: {:8.2} FPS ({:6.3} ms) | SHA-1: {}",
                    threads, rust_fps, rust_lat, rust_hash
                );
            }
        }
    }
    println!("========================================================================================================================");
}
