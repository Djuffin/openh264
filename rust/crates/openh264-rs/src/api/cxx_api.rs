//! OpenH264-compatible C++ API exposed via `cxx` (`namespace = "openh264rs"`).
//!
//! Defines the canonical, self-contained parameter, picture, and bitstream structures
//! (`SEncParamBase`, `SEncParamExt`, `SSourcePicture`, `SFrameBSInfo`, `SLayerBSInfo`,
//! `SSpatialLayerConfig`, `SSliceArgument`) used by both the Rust core/C ABI and the
//! `cxx` C++ bridge without any duplicate definitions.

#![allow(unsafe_code)]
#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_qualifications
)]

use crate::api::c_api::abi_guard;
use crate::api::encoder::Encoder;
use crate::api::types::{
    CM_INIT_PARA_ERROR, CM_MALLOC_MEM_ERROR, CM_RESULT_SUCCESS, CM_UNKNOWN_REASON,
    ECOMPLEXITY_MODE, ELevelIdc, ENCODER_OPTION, EParameterSetStrategy, EProfileIdc,
    ESampleAspectRatio, EUsageType, EVideoFrameType, MAX_LAYER_NUM_OF_FRAME, MAX_SLICES_NUM_TMP,
    MAX_SPATIAL_LAYER_NUM, RC_MODES, SliceModeEnum, UNSPECIFIED_BIT_RATE,
};
use std::ptr;

macro_rules! impl_extern_enum {
    ($($ty:ident => $cxx_id:literal),* $(,)?) => {
        $(
            unsafe impl cxx::ExternType for $ty {
                type Id = cxx::type_id!($cxx_id);
                type Kind = cxx::kind::Trivial;
            }
        )*
    };
}

impl_extern_enum! {
    EUsageType => "openh264rs::EUsageType",
    RC_MODES => "openh264rs::RC_MODES",
    ECOMPLEXITY_MODE => "openh264rs::ECOMPLEXITY_MODE",
    EParameterSetStrategy => "openh264rs::EParameterSetStrategy",
    EProfileIdc => "openh264rs::EProfileIdc",
    ELevelIdc => "openh264rs::ELevelIdc",
    SliceModeEnum => "openh264rs::SliceModeEnum",
    ESampleAspectRatio => "openh264rs::ESampleAspectRatio",
    EVideoFrameType => "openh264rs::EVideoFrameType",
    ENCODER_OPTION => "openh264rs::ENCODER_OPTION",
}

/// Opaque type representing `void` pointer payloads across the `cxx` FFI boundary.
#[repr(C)]
pub struct c_void {
    _priv: [u8; 0],
}

/// H.264 / SVC Encoder instance exposed to C++ via `cxx` (`openh264rs::ISVCEncoder`).
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
    /// `param` must be either null or a valid, aligned pointer to `ffi::SEncParamBase`
    /// readable for the duration of the call.
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
    /// `param` must be either null or a valid, aligned pointer to `ffi::SEncParamExt`
    /// readable for the duration of the call.
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
    /// `param` must be either null or a valid, aligned, writable pointer to `ffi::SEncParamExt`.
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
    /// * `src_pic` must be either null or point to a valid `ffi::SSourcePicture` whose
    ///   `pData` planes are readable according to `iStride` and picture dimensions.
    /// * `bs_info` must be either null or point to a writable `ffi::SFrameBSInfo`.
    pub unsafe fn encode_frame(
        &mut self,
        src_pic: *const ffi::SSourcePicture,
        bs_info: *mut ffi::SFrameBSInfo,
    ) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::EncodeFrame", log, CM_UNKNOWN_REASON, {
            let (Some(src_ref), Some(bs_mut)) = (unsafe { (src_pic.as_ref(), bs_info.as_mut()) })
            else {
                return CM_INIT_PARA_ERROR;
            };
            self.inner.encode_frame(src_ref, bs_mut)
        })
    }

    /// Encodes out-of-band parameter sets (SPS/PPS) into `bs_info`.
    ///
    /// # Safety
    ///
    /// `bs_info` must be either null or point to a writable `ffi::SFrameBSInfo`.
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

    /// Forces the encoder to code the next frame as an IDR / intra frame.
    pub fn force_intra_frame(&mut self, idr: bool, layer_id: i32) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::ForceIntraFrame", log, CM_UNKNOWN_REASON, {
            self.inner.force_intra_frame(idr, layer_id)
        })
    }

    /// Sets a runtime encoder option (`ENCODER_OPTION`).
    ///
    /// # Safety
    ///
    /// `option` must point to a readable, aligned value matching the type expected by `option_id`.
    pub unsafe fn set_option(&mut self, option_id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::SetOption", log, CM_INIT_PARA_ERROR, {
            unsafe {
                self.inner
                    .set_option_raw(option_id, option as *mut std::ffi::c_void)
            }
        })
    }

    /// Gets a runtime encoder option (`ENCODER_OPTION`).
    ///
    /// # Safety
    ///
    /// `option` must point to a writable, aligned buffer matching the type expected by `option_id`.
    pub unsafe fn get_option(&mut self, option_id: ENCODER_OPTION, option: *mut c_void) -> i32 {
        let log = self.log_ctx();
        abi_guard!("ISVCEncoder::GetOption", log, CM_INIT_PARA_ERROR, {
            unsafe {
                self.inner
                    .get_option_raw(option_id, option as *mut std::ffi::c_void)
            }
        })
    }
}

/// Allocates and initializes a new `openh264rs::ISVCEncoder` instance.
///
/// # Safety
///
/// `pp_encoder` must be a non-null, valid pointer to a `*mut ISVCEncoder` out-parameter.
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

/// Destroys an `openh264rs::ISVCEncoder` instance previously created by `WelsCreateSVCEncoder`.
///
/// # Safety
///
/// `p_encoder` must be either null or a pointer returned by `WelsCreateSVCEncoder`
/// that has not yet been destroyed.
pub unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder) {
    let log = if p_encoder.is_null() {
        None
    } else {
        unsafe { (*p_encoder).log_ctx() }
    };
    abi_guard!("WelsDestroySVCEncoder", log, (), {
        if !p_encoder.is_null() {
            unsafe {
                drop(Box::from_raw(p_encoder));
            }
        }
    })
}

#[cxx::bridge(namespace = "openh264rs")]
pub mod ffi {
    unsafe extern "C++" {
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
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SSliceArgument {
        pub uiSliceMode: SliceModeEnum,
        pub uiSliceNum: u32,
        pub uiSliceMbNum: [u32; 35],
        pub uiSliceSizeConstraint: u32,
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SSpatialLayerConfig {
        pub iVideoWidth: i32,
        pub iVideoHeight: i32,
        pub fFrameRate: f32,
        pub iSpatialBitrate: i32,
        pub iMaxSpatialBitrate: i32,
        pub uiProfileIdc: EProfileIdc,
        pub uiLevelIdc: ELevelIdc,
        pub iDLayerQp: i32,

        pub sSliceArgument: SSliceArgument,

        pub bVideoSignalTypePresent: bool,
        pub uiVideoFormat: u8,
        pub bFullRange: bool,
        pub bColorDescriptionPresent: bool,
        pub uiColorPrimaries: u8,
        pub uiTransferCharacteristics: u8,
        pub uiColorMatrix: u8,

        pub bAspectRatioPresent: bool,
        pub eAspectRatio: ESampleAspectRatio,
        pub sAspectRatioExtWidth: u16,
        pub sAspectRatioExtHeight: u16,
    }

    #[derive(Debug, Copy, Clone, Default)]
    pub struct SEncParamBase {
        pub iUsageType: EUsageType,
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub iTargetBitrate: i32,
        pub iRCMode: RC_MODES,
        pub fMaxFrameRate: f32,
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SEncParamExt {
        pub iUsageType: EUsageType,
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub iTargetBitrate: i32,
        pub iRCMode: RC_MODES,
        pub fMaxFrameRate: f32,

        pub iTemporalLayerNum: i32,
        pub iSpatialLayerNum: i32,
        pub sSpatialLayers: [SSpatialLayerConfig; 4],

        pub iComplexityMode: ECOMPLEXITY_MODE,
        pub uiIntraPeriod: u32,
        pub iNumRefFrame: i32,
        pub eSpsPpsIdStrategy: EParameterSetStrategy,
        pub bPrefixNalAddingCtrl: bool,
        pub bEnableSSEI: bool,
        pub bSimulcastAVC: bool,
        pub iPaddingFlag: i32,
        pub iEntropyCodingModeFlag: i32,

        pub bEnableFrameSkip: bool,
        pub iMaxBitrate: i32,
        pub iMaxQp: i32,
        pub iMinQp: i32,
        pub uiMaxNalSize: u32,

        pub bEnableLongTermReference: bool,
        pub iLTRRefNum: i32,
        pub iLtrMarkPeriod: u32,

        pub iMultipleThreadIdc: u16,
        pub bUseLoadBalancing: bool,

        pub iLoopFilterDisableIdc: i32,
        pub iLoopFilterAlphaC0Offset: i32,
        pub iLoopFilterBetaOffset: i32,

        pub bEnableDenoise: bool,
        pub bEnableBackgroundDetection: bool,
        pub bEnableAdaptiveQuant: bool,
        pub bEnableFrameCroppingFlag: bool,
        pub bEnableSceneChangeDetect: bool,

        pub bIsLosslessLink: bool,
        pub bFixRCOverShoot: bool,
        pub iIdrBitrateRatio: i32,
        pub bPsnrY: bool,
        pub bPsnrU: bool,
        pub bPsnrV: bool,
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SSourcePicture {
        pub iColorFormat: i32,
        pub iStride: [i32; 4],
        pub pData: [*mut u8; 4],
        pub iPicWidth: i32,
        pub iPicHeight: i32,
        pub uiTimeStamp: i64,
        pub bPsnrY: bool,
        pub bPsnrU: bool,
        pub bPsnrV: bool,
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SLayerBSInfo {
        pub uiTemporalId: u8,
        pub uiSpatialId: u8,
        pub uiQualityId: u8,
        pub eFrameType: EVideoFrameType,
        pub uiLayerType: u8,
        pub iSubSeqId: i32,
        pub iNalCount: i32,
        pub pNalLengthInByte: *mut i32,
        pub pBsBuf: *mut u8,
        pub rPsnr: [f32; 3],
    }

    #[derive(Debug, Copy, Clone)]
    pub struct SFrameBSInfo {
        pub iLayerNum: i32,
        pub sLayerInfo: [SLayerBSInfo; 128],
        pub eFrameType: EVideoFrameType,
        pub iFrameSizeInBytes: i32,
        pub uiTimeStamp: i64,
    }

    extern "Rust" {
        type c_void;
        type ISVCEncoder;

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

        #[cxx_name = "WelsCreateSVCEncoder"]
        unsafe fn wels_create_svc_encoder(pp_encoder: *mut *mut ISVCEncoder) -> i32;

        #[cxx_name = "WelsDestroySVCEncoder"]
        unsafe fn wels_destroy_svc_encoder(p_encoder: *mut ISVCEncoder);
    }
}

impl Default for ffi::SSliceArgument {
    fn default() -> Self {
        Self {
            uiSliceMode: SliceModeEnum::SM_SINGLE_SLICE,
            uiSliceNum: 0,
            uiSliceMbNum: [0; MAX_SLICES_NUM_TMP],
            uiSliceSizeConstraint: 0,
        }
    }
}

impl Default for ffi::SSpatialLayerConfig {
    fn default() -> Self {
        Self {
            iVideoWidth: 0,
            iVideoHeight: 0,
            fFrameRate: 0.0,
            iSpatialBitrate: 0,
            iMaxSpatialBitrate: 0,
            uiProfileIdc: EProfileIdc::PRO_UNKNOWN,
            uiLevelIdc: ELevelIdc::LEVEL_UNKNOWN,
            iDLayerQp: 0,
            sSliceArgument: ffi::SSliceArgument::default(),
            bVideoSignalTypePresent: false,
            uiVideoFormat: 0,
            bFullRange: false,
            bColorDescriptionPresent: false,
            uiColorPrimaries: 0,
            uiTransferCharacteristics: 0,
            uiColorMatrix: 0,
            bAspectRatioPresent: false,
            eAspectRatio: ESampleAspectRatio::ASP_UNSPECIFIED,
            sAspectRatioExtWidth: 0,
            sAspectRatioExtHeight: 0,
        }
    }
}

impl Default for ffi::SEncParamExt {
    fn default() -> Self {
        Self {
            iUsageType: EUsageType::CAMERA_VIDEO_REAL_TIME,
            iPicWidth: 0,
            iPicHeight: 0,
            iTargetBitrate: 0,
            iRCMode: RC_MODES::RC_QUALITY_MODE,
            fMaxFrameRate: 0.0,
            iTemporalLayerNum: 1,
            iSpatialLayerNum: 1,
            sSpatialLayers: [ffi::SSpatialLayerConfig::default(); MAX_SPATIAL_LAYER_NUM],
            iComplexityMode: ECOMPLEXITY_MODE::LOW_COMPLEXITY,
            uiIntraPeriod: 0,
            iNumRefFrame: 1,
            eSpsPpsIdStrategy: EParameterSetStrategy::INCREASING_ID,
            bPrefixNalAddingCtrl: false,
            bEnableSSEI: false,
            bSimulcastAVC: false,
            iPaddingFlag: 0,
            iEntropyCodingModeFlag: 0,
            bEnableFrameSkip: false,
            iMaxBitrate: UNSPECIFIED_BIT_RATE,
            iMaxQp: 51,
            iMinQp: 0,
            uiMaxNalSize: 0,
            bEnableLongTermReference: false,
            iLTRRefNum: 0,
            iLtrMarkPeriod: 0,
            iMultipleThreadIdc: 1,
            bUseLoadBalancing: false,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            bEnableDenoise: false,
            bEnableBackgroundDetection: true,
            bEnableAdaptiveQuant: true,
            bEnableFrameCroppingFlag: true,
            bEnableSceneChangeDetect: true,
            bIsLosslessLink: false,
            bFixRCOverShoot: false,
            iIdrBitrateRatio: 0,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,
        }
    }
}

impl Default for ffi::SSourcePicture {
    fn default() -> Self {
        Self {
            iColorFormat: crate::api::types::EVideoFormatType::videoFormatI420 as i32,
            iStride: [0; 4],
            pData: [ptr::null_mut(); 4],
            iPicWidth: 0,
            iPicHeight: 0,
            uiTimeStamp: 0,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,
        }
    }
}

impl Default for ffi::SLayerBSInfo {
    fn default() -> Self {
        Self {
            uiTemporalId: 0,
            uiSpatialId: 0,
            uiQualityId: 0,
            eFrameType: EVideoFrameType::videoFrameTypeInvalid,
            uiLayerType: 0,
            iSubSeqId: 0,
            iNalCount: 0,
            pNalLengthInByte: ptr::null_mut(),
            pBsBuf: ptr::null_mut(),
            rPsnr: [0.0; 3],
        }
    }
}

impl Default for ffi::SFrameBSInfo {
    fn default() -> Self {
        Self {
            iLayerNum: 0,
            sLayerInfo: [ffi::SLayerBSInfo::default(); MAX_LAYER_NUM_OF_FRAME],
            eFrameType: EVideoFrameType::videoFrameTypeInvalid,
            iFrameSizeInBytes: 0,
            uiTimeStamp: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cxx_encoder_lifecycle_and_encode() {
        unsafe {
            let mut encoder: *mut ISVCEncoder = ptr::null_mut();
            assert_eq!(wels_create_svc_encoder(&mut encoder), CM_RESULT_SUCCESS);
            assert!(!encoder.is_null());

            let enc = &mut *encoder;
            let mut param = ffi::SEncParamExt::default();
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

            let mut src_pic = ffi::SSourcePicture::default();
            src_pic.iPicWidth = width as i32;
            src_pic.iPicHeight = height as i32;
            src_pic.iStride = [width as i32, (width / 2) as i32, (width / 2) as i32, 0];
            src_pic.pData = [
                y_plane.as_mut_ptr(),
                u_plane.as_mut_ptr(),
                v_plane.as_mut_ptr(),
                ptr::null_mut(),
            ];

            let mut bs_info = ffi::SFrameBSInfo::default();
            assert_eq!(enc.encode_frame(&src_pic, &mut bs_info), CM_RESULT_SUCCESS);
            assert!(bs_info.iFrameSizeInBytes > 0);
            assert!(bs_info.iLayerNum > 0);

            assert_eq!(enc.uninitialize(), CM_RESULT_SUCCESS);
            wels_destroy_svc_encoder(encoder);
        }
    }
}
