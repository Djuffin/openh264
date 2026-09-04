//! Decode an Annex-B .264 file and dump raw YUV420 frames to a file.
//! Usage: dump_yuv <input.264> <output.yuv>
#![allow(non_snake_case)]

use openh264_rs::api::codec_api::*;
use openh264_rs::split_annexb_units;
use std::io::Write;

fn write_plane(out: &mut impl Write, plane: *const u8, width: usize, height: usize, stride: usize) {
    if plane.is_null() || width == 0 || height == 0 || stride == 0 {
        return;
    }
    unsafe {
        for y in 0..height {
            let row = std::slice::from_raw_parts(plane.add(y * stride), width);
            out.write_all(row).unwrap();
        }
    }
}

fn write_frame(out: &mut impl Write, data: [*mut u8; 3], buf_info: &SBufferInfo) -> bool {
    if buf_info.iBufferStatus != 1 {
        return false;
    }
    unsafe {
        let width = buf_info.UsrData.sSystemBuffer.iWidth as usize;
        let height = buf_info.UsrData.sSystemBuffer.iHeight as usize;
        let stride_y = buf_info.UsrData.sSystemBuffer.iStride[0] as usize;
        let stride_uv = buf_info.UsrData.sSystemBuffer.iStride[1] as usize;
        write_plane(out, data[0], width, height, stride_y);
        write_plane(out, data[1], width / 2, height / 2, stride_uv);
        write_plane(out, data[2], width / 2, height / 2, stride_uv);
    }
    true
}

fn main() {
    // Decoder uses large stack frames; run on a thread with a big stack.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let input = &args[1];
    let output = &args[2];
    let data = std::fs::read(input).expect("read input");
    let mut out = std::io::BufWriter::new(std::fs::File::create(output).expect("create output"));

    unsafe {
        let mut p_decoder: *mut ISVCDecoder = std::ptr::null_mut();
        assert_eq!(WelsCreateDecoder(&mut p_decoder), CM_RESULT_SUCCESS);

        let mut dec_param = SDecodingParam::default();
        dec_param.uiTargetDqLayer = u8::MAX;
        dec_param.eEcActiveIdc = ERROR_CON_IDC::ERROR_CON_SLICE_COPY;
        dec_param.sVideoProperty.eVideoBsType = VIDEO_BITSTREAM_DEFAULT;
        assert_eq!(ISVCDecoder::Initialize(p_decoder, &dec_param), CM_RESULT_SUCCESS);

        let units = split_annexb_units(&data);
        let mut n = 0;
        for unit in units.iter() {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ISVCDecoder::DecodeFrame2(p_decoder, unit.as_ptr(), unit.len() as i32, p_dst.as_mut_ptr(), &mut buf_info);
            if ret != DECODING_STATE::dsErrorFree {
                eprintln!("unit decode state: {:?}", ret);
            }
            if ret == DECODING_STATE::dsErrorFree && write_frame(&mut out, p_dst, &buf_info) {
                n += 1;
            }
        }

        let mut eos_flag = 1i32;
        ISVCDecoder::SetOption(p_decoder,
            DECODER_OPTION::DECODER_OPTION_END_OF_STREAM,
            &mut eos_flag as *mut i32 as *mut std::ffi::c_void,
        );
        let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
        let mut buf_info = SBufferInfo::default();
        let ret = ISVCDecoder::DecodeFrame2(p_decoder, std::ptr::null(), 0, p_dst.as_mut_ptr(), &mut buf_info);
        if ret == DECODING_STATE::dsErrorFree && write_frame(&mut out, p_dst, &buf_info) {
            n += 1;
        }

        let mut remaining = 0i32;
        ISVCDecoder::GetOption(p_decoder,
            DECODER_OPTION::DECODER_OPTION_NUM_OF_FRAMES_REMAINING_IN_BUFFER,
            &mut remaining as *mut i32 as *mut std::ffi::c_void,
        );
        for _ in 0..remaining {
            let mut p_dst: [*mut u8; 3] = [std::ptr::null_mut(); 3];
            let mut buf_info = SBufferInfo::default();
            let ret = ISVCDecoder::FlushFrame(p_decoder, p_dst.as_mut_ptr(), &mut buf_info);
            if ret == DECODING_STATE::dsErrorFree && write_frame(&mut out, p_dst, &buf_info) {
                n += 1;
            }
        }

        eprintln!("decoded {} frames", n);
        out.flush().unwrap();
        ISVCDecoder::Uninitialize(p_decoder);
        WelsDestroyDecoder(p_decoder);
    }
}
