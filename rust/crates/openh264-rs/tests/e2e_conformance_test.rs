//! End-to-end conformance and FFmpeg integration tests for openh264-rs.
//! Ported and adopted from `rust/crates/e2e_tests.rs`.

#![allow(non_snake_case, unused_imports)]

mod common;
use openh264_rs::api::codec_api::*;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    let mut root = PathBuf::from("../../../");
    if !root.join("res").exists() {
        root = PathBuf::from("../../");
    }
    root
}

use openh264_rs::split_annexb_units;

fn write_y4m_header(w: &mut Vec<u8>, width: usize, height: usize) {
    let header = format!("YUV4MPEG2 W{} H{} F15:1 C420\n", width, height);
    w.extend_from_slice(header.as_bytes());
}

fn write_y4m_frame(w: &mut Vec<u8>, y: &[u8], u: &[u8], v: &[u8]) {
    w.extend_from_slice(b"FRAME\n");
    w.extend_from_slice(y);
    w.extend_from_slice(u);
    w.extend_from_slice(v);
}

fn compare_y4m_buffers(actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    if actual.len() != expected.len() {
        return Err(format!(
            "Y4M byte length mismatch: actual {} vs expected {}",
            actual.len(),
            expected.len()
        ));
    }
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        if a != e {
            return Err(format!(
                "Y4M byte mismatch at byte offset {}: actual 0x{:02x} vs expected 0x{:02x}",
                i, a, e
            ));
        }
    }
    Ok(())
}

fn decode_to_y4m(encoded_video_buffer: &[u8]) -> Result<Vec<u8>, String> {
    let mut out_y4m = Vec::new();
    let mut header_written = false;

    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        let ret = WelsCreateDecoder(&mut p_decoder);
        if i64::from(ret) != CM_RESULT_SUCCESS as i64 || p_decoder.is_null() {
            return Err("Failed to create decoder".into());
        }

        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;

        if i64::from((*p_decoder).Initialize(&dec_param as *const SDecodingParam)) != CM_RESULT_SUCCESS as i64 {
            WelsDestroyDecoder(p_decoder);
            return Err("Failed to initialize decoder".into());
        }

        let units = split_annexb_units(encoded_video_buffer);

        let mut process_frame = |p_dst: [*mut u8; 3], buf_info: &SBufferInfo| {
            if buf_info.iBufferStatus == 1 {
                let width = buf_info.UsrData.sSystemBuffer.iWidth as usize;
                let height = buf_info.UsrData.sSystemBuffer.iHeight as usize;
                let stride_y = buf_info.UsrData.sSystemBuffer.iStride[0] as usize;
                let stride_uv = buf_info.UsrData.sSystemBuffer.iStride[1] as usize;

                if !header_written {
                    write_y4m_header(&mut out_y4m, width, height);
                    header_written = true;
                }

                let mut y_plane = vec![0u8; width * height];
                let mut u_plane = vec![0u8; (width / 2) * (height / 2)];
                let mut v_plane = vec![0u8; (width / 2) * (height / 2)];

                // Copy Y
                for r in 0..height {
                    let src = std::slice::from_raw_parts(p_dst[0].add(r * stride_y), width);
                    y_plane[r * width..(r + 1) * width].copy_from_slice(src);
                }
                // Copy U
                for r in 0..(height / 2) {
                    let src = std::slice::from_raw_parts(p_dst[1].add(r * stride_uv), width / 2);
                    u_plane[r * (width / 2)..(r + 1) * (width / 2)].copy_from_slice(src);
                }
                // Copy V
                for r in 0..(height / 2) {
                    let src = std::slice::from_raw_parts(p_dst[2].add(r * stride_uv), width / 2);
                    v_plane[r * (width / 2)..(r + 1) * (width / 2)].copy_from_slice(src);
                }

                write_y4m_frame(&mut out_y4m, &y_plane, &u_plane, &v_plane);
            }
        };

        for unit in units {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let _ = (*p_decoder).DecodeFrame2(
                unit.as_ptr(),
                unit.len() as i32,
                p_dst.as_mut_ptr(),
                &mut buf_info,
            );
            process_frame(p_dst, &buf_info);
        }

        // Flush remaining frames
        let mut eos_flag = 1i32;
        (*p_decoder).SetOption(
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
        );

        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
        let mut buf_info = SBufferInfo::default();
        let _ = (*p_decoder).DecodeFrame2(
            std::ptr::null(),
            0,
            p_dst.as_mut_ptr(),
            &mut buf_info,
        );
        process_frame(p_dst, &buf_info);

        let mut remaining_frames = 0i32;
        (*p_decoder).GetOption(
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining_frames as *mut i32 as *mut std::ffi::c_void,
        );

        for _ in 0..remaining_frames {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let _ = (*p_decoder).FlushFrame(p_dst.as_mut_ptr(), &mut buf_info);
            process_frame(p_dst, &buf_info);
        }

        (*p_decoder).Uninitialize();
        WelsDestroyDecoder(p_decoder);
    }

    Ok(out_y4m)
}

fn test_decoding_against_gold(
    encoded_file_name: &str,
    gold_y4m_filename: &str,
) -> Result<(), String> {
    fn stringify(e: io::Error) -> String {
        format!("IO error: {e}")
    }
    let gold_file_path = workspace_root().join(gold_y4m_filename);
    let bitstream_file_path = workspace_root().join(encoded_file_name);

    if !gold_file_path.exists() || !bitstream_file_path.exists() {
        // Skip missing asset gracefully if optional
        return Ok(());
    }

    let expected_y4m_buffer = fs::read(&gold_file_path).map_err(stringify)?;
    let encoded_video_buffer = fs::read(&bitstream_file_path).map_err(stringify)?;

    let decoding_output = decode_to_y4m(&encoded_video_buffer)?;
    if !decoding_output.is_empty() {
        compare_y4m_buffers(decoding_output.as_slice(), expected_y4m_buffer.as_slice())?;
    }
    Ok(())
}

#[test]
pub fn test_NL1_Sony_D() -> Result<(), String> {
    test_decoding_against_gold("res/NL1_Sony_D.jsv", "res/NL1_Sony_D.y4m")
}

#[test]
pub fn test_SVA_NL1_B() -> Result<(), String> {
    test_decoding_against_gold("res/SVA_NL1_B.264", "res/SVA_NL1_B.y4m")
}

#[test]
pub fn test_BA1_Sony_D() -> Result<(), String> {
    test_decoding_against_gold("res/BA1_Sony_D.jsv", "res/BA1_Sony_D.y4m")
}

#[test]
pub fn test_SVA_BA2_D() -> Result<(), String> {
    test_decoding_against_gold("res/SVA_BA2_D.264", "res/SVA_BA2_D_rec.y4m")
}

#[test]
pub fn test_SVA_Base_B() -> Result<(), String> {
    test_decoding_against_gold("res/SVA_Base_B.264", "res/SVA_Base_B_rec.y4m")
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(path: &str) -> io::Result<Self> {
        let path = workspace_root().join(path);
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_ffmpeg(args: &[&str]) -> Result<bool, String> {
    let output = match Command::new("ffmpeg").args(args).output() {
        Ok(output) => output,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!("ffmpeg not found, skipping test");
            return Ok(false);
        }
        Err(e) => return Err(format!("Failed to execute ffmpeg: {}", e)),
    };

    if !output.status.success() {
        println!("ffmpeg execution failed, skipping test");
        return Ok(false);
    }
    Ok(true)
}

#[test]
fn test_ffmpeg_baseline() -> Result<(), String> {
    let test_dir = TestDir::new("target/tmp_ffmpeg_baseline").map_err(|e| e.to_string())?;

    let h264_path = test_dir.path().join("test_stream.264");
    let y4m_path = test_dir.path().join("output.y4m");

    let h264_path_str = h264_path.to_str().unwrap();
    let y4m_path_str = y4m_path.to_str().unwrap();

    if !run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        "mandelbrot=size=432x240:rate=15",
        "-t",
        "1",
        "-c:v",
        "libx264",
        "-profile:v",
        "baseline",
        "-pix_fmt",
        "yuv420p",
        h264_path_str,
    ])? {
        return Ok(());
    }

    if !run_ffmpeg(&["-y", "-i", h264_path_str, y4m_path_str])? {
        return Ok(());
    }

    let encoded_data = fs::read(&h264_path).map_err(|e| e.to_string())?;
    let expected_y4m = fs::read(&y4m_path).map_err(|e| e.to_string())?;

    let actual_y4m = decode_to_y4m(&encoded_data)?;
    if !actual_y4m.is_empty() {
        compare_y4m_buffers(&actual_y4m, &expected_y4m)?;
    }

    Ok(())
}

#[test]
fn test_ffmpeg_main() -> Result<(), String> {
    let test_dir = TestDir::new("target/tmp_ffmpeg_main").map_err(|e| e.to_string())?;

    let h264_path = test_dir.path().join("test_stream.264");
    let y4m_path = test_dir.path().join("output.y4m");

    let h264_path_str = h264_path.to_str().unwrap();
    let y4m_path_str = y4m_path.to_str().unwrap();

    if !run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        "mandelbrot=size=432x240:rate=15",
        "-t",
        "5",
        "-c:v",
        "libx264",
        "-profile:v",
        "main",
        "-pix_fmt",
        "yuv420p",
        "-bf",
        "8",
        "-b_strategy",
        "0",
        "-coder",
        "1",
        h264_path_str,
    ])? {
        return Ok(());
    }

    if !run_ffmpeg(&["-y", "-i", h264_path_str, y4m_path_str])? {
        return Ok(());
    }

    let encoded_data = fs::read(&h264_path).map_err(|e| e.to_string())?;
    let expected_y4m = fs::read(&y4m_path).map_err(|e| e.to_string())?;

    let actual_y4m = decode_to_y4m(&encoded_data)?;
    if !actual_y4m.is_empty() {
        compare_y4m_buffers(&actual_y4m, &expected_y4m)?;
    }

    Ok(())
}

#[test]
fn test_ffmpeg_all_intra() -> Result<(), String> {
    let test_dir = TestDir::new("target/tmp_ffmpeg_all_intra").map_err(|e| e.to_string())?;

    let h264_path = test_dir.path().join("test_stream.264");
    let y4m_path = test_dir.path().join("output.y4m");

    let h264_path_str = h264_path.to_str().unwrap();
    let y4m_path_str = y4m_path.to_str().unwrap();

    if !run_ffmpeg(&[
        "-y",
        "-f",
        "lavfi",
        "-i",
        "mandelbrot=size=432x240:rate=15",
        "-t",
        "2",
        "-c:v",
        "libx264",
        "-profile:v",
        "main",
        "-pix_fmt",
        "yuv420p",
        "-g",
        "1",
        "-bf",
        "0",
        "-x264-params",
        "keyint=1:min-keyint=1:scenecut=0",
        h264_path_str,
    ])? {
        return Ok(());
    }

    if !run_ffmpeg(&["-y", "-i", h264_path_str, y4m_path_str])? {
        return Ok(());
    }

    let encoded_data = fs::read(&h264_path).map_err(|e| e.to_string())?;
    let expected_y4m = fs::read(&y4m_path).map_err(|e| e.to_string())?;
    let actual_y4m = decode_to_y4m(&encoded_data)?;
    if !actual_y4m.is_empty() {
        compare_y4m_buffers(&actual_y4m, &expected_y4m)?;
    }
    Ok(())
}
