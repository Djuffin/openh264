//! OpenH264-compatible C++ API exposed via `cxx` in the top-level namespace (`::`).
//!
//! Uses OpenH264's canonical C/C++ parameter, picture, buffer, and bitstream structures
//! (`SEncParamBase`, `SEncParamExt`, `SSourcePicture`, `SFrameBSInfo`, `SLayerBSInfo`,
//! `SDecodingParam`, `SBufferInfo`, `SParserBsInfo`, `SDecoderCapability`, `OpenH264Version`,
//! `SSpatialLayerConfig`, `SSliceArgument`) directly from `codec_app_def.h` and `codec_def.h`.

#![deny(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_qualifications
)]

use crate::api::c_api::{self, abi_guard};
use crate::api::encoder::Encoder;
use crate::api::types::{
    CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS, CM_UNKNOWN_REASON, DECODER_OPTION,
    DECODING_STATE, ECOMPLEXITY_MODE, ELevelIdc, ENCODER_OPTION, EParameterSetStrategy,
    EProfileIdc, ERROR_CON_IDC, ESampleAspectRatio, EUsageType, EVideoFrameType, OpenH264Version,
    RC_MODES, SBufferInfo, SDecoderCapability, SDecodingParam, SEncParamBase, SEncParamExt,
    SFrameBSInfo, SLayerBSInfo, SParserBsInfo, SSliceArgument, SSourcePicture,
    SSpatialLayerConfig, SSysMEMBuffer, SVideoProperty, SliceModeEnum, VIDEO_BITSTREAM_TYPE,
};
use std::ptr;

macro_rules! impl_extern_type {
    ($($ty:ident => $cxx_id:literal),* $(,)?) => {
        $(
            #[allow(unsafe_code)]
            unsafe impl cxx::ExternType for $ty {
                type Id = cxx::type_id!($cxx_id);
                type Kind = cxx::kind::Trivial;
            }
        )*
    };
}

impl_extern_type! {
    EUsageType => "EUsageType",
    RC_MODES => "RC_MODES",
    ECOMPLEXITY_MODE => "ECOMPLEXITY_MODE",
    EParameterSetStrategy => "EParameterSetStrategy",
    EProfileIdc => "EProfileIdc",
    ELevelIdc => "ELevelIdc",
    SliceModeEnum => "SliceModeEnum",
    ESampleAspectRatio => "ESampleAspectRatio",
    EVideoFrameType => "EVideoFrameType",
    ENCODER_OPTION => "ENCODER_OPTION",
    DECODER_OPTION => "DECODER_OPTION",
    DECODING_STATE => "DECODING_STATE",
    ERROR_CON_IDC => "ERROR_CON_IDC",
    VIDEO_BITSTREAM_TYPE => "VIDEO_BITSTREAM_TYPE",
    SSliceArgument => "SSliceArgument",
    SSpatialLayerConfig => "SSpatialLayerConfig",
    SEncParamBase => "SEncParamBase",
    SEncParamExt => "SEncParamExt",
    SSourcePicture => "SSourcePicture",
    SLayerBSInfo => "SLayerBSInfo",
    SFrameBSInfo => "SFrameBSInfo",
    SVideoProperty => "SVideoProperty",
    SDecodingParam => "SDecodingParam",
    SSysMEMBuffer => "SSysMEMBuffer",
    SBufferInfo => "SBufferInfo",
    SParserBsInfo => "SParserBsInfo",
    SDecoderCapability => "SDecoderCapability",
    OpenH264Version => "OpenH264Version",
}

/// Opaque type representing `void` pointer payloads across the `cxx` FFI boundary.
#[repr(C)]
pub struct c_void {
    _priv: [u8; 0],
}

/// H.264 / SVC Encoder instance exposed to C++ via `cxx` (`::ISVCEncoder`).
pub struct ISVCEncoder {
    pub(crate) inner: Encoder,
}

impl ISVCEncoder {
    #[inline]
    fn log_ctx(&self) -> Option<crate::common::wels_trace::SLogContext> {
        Some(self.inner.0.m_pWelsTrace.log_context())
    }

    /// Initializes the encoder with basic parameters (`SEncParamBase`).
    ///
    /// # Safety
    ///
    /// `param` must be either null or a valid, aligned pointer to `SEncParamBase`
    /// readable for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn initialize(&mut self, param: *const ffi::SEncParamBase) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::Initialize", log, CM_INIT_PARA_ERROR, {
            let param_ref = unsafe { param.as_ref() };
            self.inner.0.Initialize(param_ref)
        })
    }

    /// Initializes the encoder with extended SVC parameters (`SEncParamExt`).
    ///
    /// # Safety
    ///
    /// `param` must be either null or a valid, aligned pointer to `SEncParamExt`
    /// readable for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn initialize_ext(&mut self, param: *const ffi::SEncParamExt) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::InitializeExt", log, CM_INIT_PARA_ERROR, {
            let param_ref = unsafe { param.as_ref() };
            self.inner.0.InitializeExt(param_ref)
        })
    }

    /// Populates `param` with default extended encoding parameters.
    ///
    /// # Safety
    ///
    /// `param` must be either null or a valid, aligned, writable pointer to `SEncParamExt`.
    #[allow(unsafe_code)]
    pub unsafe fn get_default_params(&mut self, param: *mut ffi::SEncParamExt) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::GetDefaultParams", log, CM_UNKNOWN_REASON, {
            let Some(param_mut) = (unsafe { param.as_mut() }) else {
                return CM_INIT_PARA_ERROR;
            };
            self.inner.default_params(param_mut)
        })
    }

    /// Uninitializes the encoder and releases session resources.
    pub fn uninitialize(&mut self) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::Uninitialize", log, CM_UNKNOWN_REASON, {
            self.inner.uninitialize()
        })
    }

    /// Encodes a single uncompressed source picture (`SSourcePicture`) into `bs_info`.
    ///
    /// # Safety
    ///
    /// - `src_pic` must be non-null and point to a valid `SSourcePicture` whose
    ///   `pData` planes are valid for the configured picture dimensions and strides.
    /// - `bs_info` must be non-null and point to a writable `SFrameBSInfo`.
    #[allow(unsafe_code)]
    pub unsafe fn encode_frame(
        &mut self,
        src_pic: *const ffi::SSourcePicture,
        bs_info: *mut ffi::SFrameBSInfo,
    ) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::EncodeFrame", log, CM_UNKNOWN_REASON, {
            let (Some(src_ref), Some(bs_mut)) = (unsafe { src_pic.as_ref() }, unsafe {
                bs_info.as_mut()
            }) else {
                return CM_INIT_PARA_ERROR;
            };
            self.inner.0.EncodeFrame(src_ref, bs_mut)
        })
    }

    /// Encodes out-of-band parameter sets (SPS/PPS) into `bs_info`.
    ///
    /// # Safety
    ///
    /// `bs_info` must be non-null and point to a writable `SFrameBSInfo`.
    #[allow(unsafe_code)]
    pub unsafe fn encode_parameter_sets(&mut self, bs_info: *mut ffi::SFrameBSInfo) -> i32 {
        let log = self.log_ctx();
        abi_guard!(
            "ISVCEncoder::EncodeParameterSets",
            log,
            CM_UNKNOWN_REASON,
            {
                let Some(bs_mut) = (unsafe { bs_info.as_mut() }) else {
                    return CM_INIT_PARA_ERROR;
                };
                self.inner.encode_parameter_sets(bs_mut)
            }
        )
    }

    /// Forces the next frame to be encoded as an IDR or Intra frame.
    pub fn force_intra_frame(&mut self, idr: bool, layer_id: i32) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::ForceIntraFrame", log, CM_UNKNOWN_REASON, {
            self.inner.force_intra_frame(idr, layer_id)
        })
    }

    /// Sets a runtime encoder option.
    ///
    /// # Safety
    ///
    /// `option` must point to a valid payload matching the expected type of `option_id`.
    #[allow(unsafe_code)]
    pub unsafe fn set_option(&mut self, option_id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::SetOption", log, CM_UNKNOWN_REASON, {
            unsafe {
                self.inner
                    .set_option_raw(option_id, option as *mut std::ffi::c_void)
            }
        })
    }

    /// Queries a runtime encoder option.
    ///
    /// # Safety
    ///
    /// `option` must point to a writable payload matching the expected type of `option_id`.
    #[allow(unsafe_code)]
    pub unsafe fn get_option(&mut self, option_id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::GetOption", log, CM_UNKNOWN_REASON, {
            unsafe {
                self.inner
                    .get_option_raw(option_id, option as *mut std::ffi::c_void)
            }
        })
    }
}

/// Allocates and initializes a new `ISVCEncoder` instance.
///
/// # Safety
///
/// `pp_encoder` must be a non-null, valid pointer to a `*mut ISVCEncoder` out-parameter.
#[allow(unsafe_code)]
pub unsafe fn wels_create_svc_encoder(pp_encoder: *mut *mut ISVCEncoder) -> i32 {
    abi_guard!("WelsCreateSVCEncoder", None, CM_MALLOC_MEM_ERROR, {
        if pp_encoder.is_null() {
            return CM_INIT_PARA_ERROR;
        }
        let enc = Box::new(ISVCEncoder {
            inner: Encoder::new(),
        });
        unsafe {
            *pp_encoder = Box::into_raw(enc);
        }
        CM_RESULT_SUCCESS
    })
}

/// Destroys an `ISVCEncoder` instance previously created by `WelsCreateSVCEncoder`.
///
/// # Safety
///
/// `p_encoder` must be either null or a pointer returned by `WelsCreateSVCEncoder`
/// that has not yet been destroyed.
#[allow(unsafe_code)]
pub unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder) {
    let Some(enc_ref) = (unsafe { p_encoder.as_ref() }) else {
        return;
    };
    let log = enc_ref.log_ctx();
    abi_guard!("WelsDestroySVCEncoder", log, (), {
        unsafe {
            drop(Box::from_raw(p_encoder));
        }
    })
}

/// H.264 / SVC Decoder instance exposed to C++ via `cxx` (`::ISVCDecoder`).
pub struct ISVCDecoder {
    pub(crate) ptr: *mut c_api::ISVCDecoder,
}

#[allow(unsafe_code)]
impl ISVCDecoder {
    pub unsafe fn initialize(&mut self, param: *const ffi::SDecodingParam) -> isize {
        unsafe { c_api::ISVCDecoder::Initialize(self.ptr, param) as isize }
    }

    pub unsafe fn uninitialize(&mut self) -> isize {
        unsafe { c_api::ISVCDecoder::Uninitialize(self.ptr) as isize }
    }

    pub unsafe fn decode_frame(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        stride: *mut i32,
        width: *mut i32,
        height: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            c_api::ISVCDecoder::DecodeFrame(self.ptr, src, src_len, dst, stride, width, height)
        }
    }

    pub unsafe fn decode_frame_no_delay(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { c_api::ISVCDecoder::DecodeFrameNoDelay(self.ptr, src, src_len, dst, dst_info) }
    }

    pub unsafe fn decode_frame2(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { c_api::ISVCDecoder::DecodeFrame2(self.ptr, src, src_len, dst, dst_info) }
    }

    pub unsafe fn flush_frame(
        &mut self,
        dst: *mut *mut u8,
        dst_info: *mut ffi::SBufferInfo,
    ) -> DECODING_STATE {
        unsafe { c_api::ISVCDecoder::FlushFrame(self.ptr, dst, dst_info) }
    }

    pub unsafe fn decode_parser(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst_info: *mut ffi::SParserBsInfo,
    ) -> DECODING_STATE {
        unsafe { c_api::ISVCDecoder::DecodeParser(self.ptr, src, src_len, dst_info) }
    }

    pub unsafe fn decode_frame_ex(
        &mut self,
        src: *const u8,
        src_len: i32,
        dst: *mut u8,
        dst_stride: i32,
        dst_len: *mut i32,
        width: *mut i32,
        height: *mut i32,
        color_format: *mut i32,
    ) -> DECODING_STATE {
        unsafe {
            c_api::ISVCDecoder::DecodeFrameEx(
                self.ptr,
                src,
                src_len,
                dst,
                dst_stride,
                dst_len,
                width,
                height,
                color_format,
            )
        }
    }

    pub unsafe fn set_option(&mut self, option_id: DECODER_OPTION, option: *mut c_void) -> isize {
        unsafe {
            c_api::ISVCDecoder::SetOption(self.ptr, option_id, option as *mut std::ffi::c_void)
                as isize
        }
    }

    pub unsafe fn get_option(&mut self, option_id: DECODER_OPTION, option: *mut c_void) -> isize {
        unsafe {
            c_api::ISVCDecoder::GetOption(self.ptr, option_id, option as *mut std::ffi::c_void)
                as isize
        }
    }
}

/// Allocates and initializes a new `ISVCDecoder` instance.
///
/// # Safety
///
/// `pp_decoder` must be a non-null, valid pointer to a `*mut ISVCDecoder` out-parameter.
#[allow(unsafe_code)]
pub unsafe fn wels_create_decoder(pp_decoder: *mut *mut ISVCDecoder) -> isize {
    abi_guard!("WelsCreateDecoder", None, CM_MALLOC_MEM_ERROR as isize, {
        if pp_decoder.is_null() {
            return CM_INIT_PARA_ERROR as isize;
        }
        let mut raw_dec: *mut c_api::ISVCDecoder = ptr::null_mut();
        let rc = unsafe { c_api::WelsCreateDecoder(&mut raw_dec) };
        if rc != 0 || raw_dec.is_null() {
            return rc as isize;
        }
        let dec = Box::new(ISVCDecoder { ptr: raw_dec });
        unsafe {
            *pp_decoder = Box::into_raw(dec);
        }
        CM_RESULT_SUCCESS as isize
    })
}

/// Destroys an `ISVCDecoder` instance previously created by `WelsCreateDecoder`.
///
/// # Safety
///
/// `p_decoder` must be either null or a pointer returned by `WelsCreateDecoder`
/// that has not yet been destroyed.
#[allow(unsafe_code)]
pub unsafe fn wels_destroy_decoder(p_decoder: *mut ISVCDecoder) {
    if p_decoder.is_null() {
        return;
    }
    abi_guard!("WelsDestroyDecoder", None, (), {
        let dec = unsafe { Box::from_raw(p_decoder) };
        unsafe {
            c_api::WelsDestroyDecoder(dec.ptr);
        }
    })
}

#[allow(unsafe_code)]
pub unsafe fn wels_get_decoder_capability(p_dec_capability: *mut ffi::SDecoderCapability) -> i32 {
    unsafe { c_api::WelsGetDecoderCapability(p_dec_capability) }
}

pub fn wels_get_codec_version() -> ffi::OpenH264Version {
    crate::api::version::WelsGetCodecVersion()
}

#[allow(unsafe_code)]
pub unsafe fn wels_get_codec_version_ex(p_version: *mut ffi::OpenH264Version) {
    unsafe { crate::api::version::WelsGetCodecVersionEx(p_version) }
}

#[allow(unsafe_code)]
#[cxx::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("codec_app_def.h");
        include!("codec_def.h");

        type EUsageType = crate::api::types::EUsageType;
        type RC_MODES = crate::api::types::RC_MODES;
        type ECOMPLEXITY_MODE = crate::api::types::ECOMPLEXITY_MODE;
        type EParameterSetStrategy = crate::api::types::EParameterSetStrategy;
        type EProfileIdc = crate::api::types::EProfileIdc;
        type ELevelIdc = crate::api::types::ELevelIdc;
        type SliceModeEnum = crate::api::types::SliceModeEnum;
        type ESampleAspectRatio = crate::api::types::ESampleAspectRatio;
        type EVideoFrameType = crate::api::types::EVideoFrameType;
        type ENCODER_OPTION = crate::api::types::ENCODER_OPTION;
        type DECODER_OPTION = crate::api::types::DECODER_OPTION;
        type DECODING_STATE = crate::api::types::DECODING_STATE;
        type ERROR_CON_IDC = crate::api::types::ERROR_CON_IDC;
        type VIDEO_BITSTREAM_TYPE = crate::api::types::VIDEO_BITSTREAM_TYPE;

        type SSliceArgument = crate::api::types::SSliceArgument;
        type SSpatialLayerConfig = crate::api::types::SSpatialLayerConfig;
        type SEncParamBase = crate::api::types::SEncParamBase;
        type SEncParamExt = crate::api::types::SEncParamExt;
        type SSourcePicture = crate::api::types::SSourcePicture;
        type SLayerBSInfo = crate::api::types::SLayerBSInfo;
        type SFrameBSInfo = crate::api::types::SFrameBSInfo;
        type SVideoProperty = crate::api::types::SVideoProperty;
        type SDecodingParam = crate::api::types::SDecodingParam;
        type SSysMEMBuffer = crate::api::types::SSysMEMBuffer;
        type SBufferInfo = crate::api::types::SBufferInfo;
        type SParserBsInfo = crate::api::types::SParserBsInfo;
        type SDecoderCapability = crate::api::types::SDecoderCapability;
        type OpenH264Version = crate::api::types::OpenH264Version;
    }

    extern "Rust" {
        type c_void;
        type ISVCEncoder;
        type ISVCDecoder;

        // ISVCEncoder methods
        #[cxx_name = "Initialize"]
        unsafe fn initialize(self: &mut ISVCEncoder, param: *const SEncParamBase) -> i32;

        #[cxx_name = "InitializeExt"]
        unsafe fn initialize_ext(self: &mut ISVCEncoder, param: *const SEncParamExt) -> i32;

        #[cxx_name = "GetDefaultParams"]
        unsafe fn get_default_params(self: &mut ISVCEncoder, param: *mut SEncParamExt) -> i32;

        #[cxx_name = "Uninitialize"]
        fn uninitialize(self: &mut ISVCEncoder) -> i32;

        #[cxx_name = "EncodeFrame"]
        unsafe fn encode_frame(
            self: &mut ISVCEncoder,
            src_pic: *const SSourcePicture,
            bs_info: *mut SFrameBSInfo,
        ) -> i32;

        #[cxx_name = "EncodeParameterSets"]
        unsafe fn encode_parameter_sets(self: &mut ISVCEncoder, bs_info: *mut SFrameBSInfo) -> i32;

        #[cxx_name = "ForceIntraFrame"]
        fn force_intra_frame(self: &mut ISVCEncoder, idr: bool, layer_id: i32) -> i32;

        #[cxx_name = "SetOption"]
        unsafe fn set_option(
            self: &mut ISVCEncoder,
            option_id: ENCODER_OPTION,
            option: *mut c_void,
        ) -> i32;

        #[cxx_name = "GetOption"]
        unsafe fn get_option(
            self: &mut ISVCEncoder,
            option_id: ENCODER_OPTION,
            option: *mut c_void,
        ) -> i32;

        // ISVCDecoder methods
        #[cxx_name = "Initialize"]
        unsafe fn initialize(self: &mut ISVCDecoder, param: *const SDecodingParam) -> isize;

        #[cxx_name = "Uninitialize"]
        unsafe fn uninitialize(self: &mut ISVCDecoder) -> isize;

        #[cxx_name = "DecodeFrame"]
        unsafe fn decode_frame(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            stride: *mut i32,
            width: *mut i32,
            height: *mut i32,
        ) -> DECODING_STATE;

        #[cxx_name = "DecodeFrameNoDelay"]
        unsafe fn decode_frame_no_delay(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> DECODING_STATE;

        #[cxx_name = "DecodeFrame2"]
        unsafe fn decode_frame2(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> DECODING_STATE;

        #[cxx_name = "FlushFrame"]
        unsafe fn flush_frame(
            self: &mut ISVCDecoder,
            dst: *mut *mut u8,
            dst_info: *mut SBufferInfo,
        ) -> DECODING_STATE;

        #[cxx_name = "DecodeParser"]
        unsafe fn decode_parser(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst_info: *mut SParserBsInfo,
        ) -> DECODING_STATE;

        #[cxx_name = "DecodeFrameEx"]
        unsafe fn decode_frame_ex(
            self: &mut ISVCDecoder,
            src: *const u8,
            src_len: i32,
            dst: *mut u8,
            dst_stride: i32,
            dst_len: *mut i32,
            width: *mut i32,
            height: *mut i32,
            color_format: *mut i32,
        ) -> DECODING_STATE;

        #[cxx_name = "SetOption"]
        unsafe fn set_option(
            self: &mut ISVCDecoder,
            option_id: DECODER_OPTION,
            option: *mut c_void,
        ) -> isize;

        #[cxx_name = "GetOption"]
        unsafe fn get_option(
            self: &mut ISVCDecoder,
            option_id: DECODER_OPTION,
            option: *mut c_void,
        ) -> isize;

        // Global lifecycle & query functions
        #[cxx_name = "WelsCreateSVCEncoder"]
        unsafe fn wels_create_svc_encoder(pp_encoder: *mut *mut ISVCEncoder) -> i32;

        #[cxx_name = "WelsDestroySVCEncoder"]
        unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder);

        #[cxx_name = "WelsCreateDecoder"]
        unsafe fn wels_create_decoder(pp_decoder: *mut *mut ISVCDecoder) -> isize;

        #[cxx_name = "WelsDestroyDecoder"]
        unsafe fn wels_destroy_decoder(p_decoder: *mut ISVCDecoder);

        #[cxx_name = "WelsGetDecoderCapability"]
        unsafe fn wels_get_decoder_capability(p_dec_capability: *mut SDecoderCapability) -> i32;

        #[cxx_name = "WelsGetCodecVersion"]
        fn wels_get_codec_version() -> OpenH264Version;

        #[cxx_name = "WelsGetCodecVersionEx"]
        unsafe fn wels_get_codec_version_ex(p_version: *mut OpenH264Version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unsafe_code)]
    fn cxx_encoder_and_decoder_lifecycle() {
        unsafe {
            let mut encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(wels_create_svc_encoder(&mut encoder), CM_RESULT_SUCCESS);
            assert!(!encoder.is_null());

            let enc = &mut *encoder;
            let mut param = SEncParamExt::default();
            assert_eq!(enc.get_default_params(&mut param), CM_RESULT_SUCCESS);

            param.iPicWidth = 320;
            param.iPicHeight = 192;
            param.iTargetBitrate = 500_000;
            param.fMaxFrameRate = 30.0;
            param.sSpatialLayers[0].iVideoWidth = 320;
            param.sSpatialLayers[0].iVideoHeight = 192;
            param.sSpatialLayers[0].fFrameRate = 30.0;
            param.sSpatialLayers[0].iSpatialBitrate = 500_000;

            assert_eq!(enc.initialize_ext(&param), CM_RESULT_SUCCESS);
            assert_eq!(enc.force_intra_frame(true, -1), CM_RESULT_SUCCESS);

            let width = 320usize;
            let height = 192usize;
            let mut y_plane = vec![128u8; width * height];
            let mut u_plane = vec![128u8; (width / 2) * (height / 2)];
            let mut v_plane = vec![128u8; (width / 2) * (height / 2)];

            let mut src_pic = SSourcePicture::default();
            src_pic.iPicWidth = width as i32;
            src_pic.iPicHeight = height as i32;
            src_pic.iStride = [width as i32, (width / 2) as i32, (width / 2) as i32, 0];
            src_pic.pData = [
                y_plane.as_mut_ptr(),
                u_plane.as_mut_ptr(),
                v_plane.as_mut_ptr(),
                ptr::null_mut(),
            ];

            let mut bs_info = SFrameBSInfo::default();
            assert_eq!(enc.encode_frame(&src_pic, &mut bs_info), CM_RESULT_SUCCESS);
            assert!(bs_info.iFrameSizeInBytes > 0);
            assert!(bs_info.iLayerNum > 0);

            assert_eq!(enc.uninitialize(), CM_RESULT_SUCCESS);
            wels_destroy_svc_encoder(encoder);
        }
    }
}
