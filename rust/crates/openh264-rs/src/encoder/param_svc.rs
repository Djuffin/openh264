//! Configurable parameters, temporal scalability mapping, and parameter set management in H.264/SVC Encoder.
//!
//! Translated from `codec/encoder/core/inc/param_svc.h`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

use crate::{
    EComplexityMode, EParameterSetStrategy, EUsageType, RCMode, SEncParamBase, SEncParamExt,
    SSliceArgument, SSpatialLayerConfig, SliceMode, VideoFormat,
};
// Profile/level/complexity/SPS-id enumerators live in api::codec_api (one definition
// per type); glob-import the variants so the C++ spellings stay bare, as in the C++.
use crate::api::codec_api::{EProfileIdc, ELevelIdc as _ELevelIdc};
use crate::api::codec_api::{
    EColorMatrix, EColorPrimaries, ESampleAspectRatio, ETransferCharacteristics, EVideoFormatSPS,
};
use crate::api::codec_api::ECOMPLEXITY_MODE::*;
use crate::api::codec_api::EProfileIdc::*;
use crate::api::codec_api::ELevelIdc::*;
use crate::api::codec_api::EParameterSetStrategy::*;
pub use crate::encoder::encoder_context::SCropOffset;

pub const INVALID_TEMPORAL_ID: u8 = 0xff;

pub const MAX_TEMPORAL_LEVEL: usize = 4;
pub const MAX_GOP_SIZE: usize = 1 << (MAX_TEMPORAL_LEVEL - 1); // 8
pub const MAX_DEPENDENCY_LAYER: usize = 4;
pub const MAX_SPATIAL_LAYER_NUM: usize = 4;
pub const MAX_FNAME_LEN: usize = 256;
pub const MAX_SPS_COUNT: usize = 32;
/// The **encoder's** PPS ceiling — `wels_const.h:51` sets `MAX_PPS_COUNT` to
/// `MAX_PPS_COUNT_LIMITED` (57), not the standard's 256, "because of known
/// limitation of receiver endpoints". This module declared its own 256 (the
/// decoder's value), which oversized `SExistingParasetList::sPps` by 199
/// entries. Re-exported from the one encoder-side definition instead.
pub use crate::encoder::encoder_context::MAX_PPS_COUNT;
pub const MAX_SLICEGROUP_IDS: usize = 8;
/// `svc_enc_slice_segment.h:62` — `(MAX_NAL_UNITS_IN_LAYER - SAVED_NALUNIT_NUM) / 3`
/// = (128 - 21) / 3 = 35. The literal here matches; re-exported anyway so there is
/// one definition.
pub use crate::encoder::wels_encoder_ext::MAX_SLICES_NUM;
/// `codec_app_def.h:56` — `(MAX_NAL_UNITS_IN_LAYER - SAVED_NALUNIT_NUM_TMP) / 3`
/// = (128 - 21) / 3 = **35**, not 32. Both `ParamTranscode` and `FillDefault`
/// compute `kiLesserSliceNum = min (MAX_SLICES_NUM, MAX_SLICES_NUM_TMP)`
/// (param_svc.h:203), so the port capped `uiSliceNum` at 32 where the reference
/// caps at 35.
pub use crate::api::codec_api::MAX_SLICES_NUM_TMP;

/// `wels_const.h:60` says **60**, not 30. This module's own copy said 30, and it
/// is the one `FillDefault` and the two transcoders use -- so `GetDefaultParams`
/// reported 30 fps where the reference reports 60, and `ParamTranscode`'s
/// `WELS_CLIP3 (fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE)` silently capped
/// any caller asking for more than 30. Measured against libopenh264.a: the
/// reference returns 60.0 from GetDefaultParams and keeps 50.0 through
/// InitializeExt. Re-exported from the one definition.
pub use crate::encoder::wels_encoder_ext::{MAX_FRAME_RATE, MIN_FRAME_RATE};

pub const UNSPECIFIED_BIT_RATE: i32 = 0;
pub const AUTO_REF_PIC_COUNT: i32 = -1;
pub const MIN_REF_PIC_COUNT: i32 = 1;
pub const MAX_REF_PIC_COUNT: i32 = 16;

pub const QP_MAX_VALUE: i32 = 51;
pub const QP_MIN_VALUE: i32 = 0;

pub const IDR_BITRATE_RATIO: i32 = 4;
pub const SVC_QUALITY_BASE_QP: i32 = 26;

pub const MB_WIDTH_LUMA: i32 = 16;
pub const MB_HEIGHT_LUMA: i32 = 16;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_INVALIDINPUT: i32 = 0x10;

pub const ASP_UNSPECIFIED: i32 = 0;
pub const VF_UNDEF: u8 = 5;
pub const CP_UNDEF: u8 = 2;
pub const TRC_UNDEF: u8 = 2;
pub const CM_UNDEF: u8 = 2;

pub const g_kuiTemporalIdListTable: [[u8; MAX_GOP_SIZE + 1]; MAX_TEMPORAL_LEVEL] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0], // uiGopSize = 1
    [0, 1, 0, 0, 0, 0, 0, 0, 0], // uiGopSize = 2
    [0, 2, 1, 2, 0, 0, 0, 0, 0], // uiGopSize = 4
    [0, 3, 2, 3, 1, 3, 2, 3, 0], // uiGopSize = 8
];

#[inline]
pub fn WELS_CLIP3<T: PartialOrd + Copy>(val: T, min_val: T, max_val: T) -> T {
    if val < min_val {
        min_val
    } else if val > max_val {
        max_val
    } else {
        val
    }
}

#[inline]
pub fn WELS_ALIGN(x: i32, n: i32) -> i32 {
    (x + (n - 1)) & !(n - 1)
}

#[inline]
pub fn WELS_LOG2(x: u32) -> i32 {
    if x == 0 {
        0
    } else {
        31 - x.leading_zeros() as i32
    }
}

/// Computes base-2 logarithm scaling factor of `(upper / base)`.
/// Returns `round(log2(upper / base))` if `(upper / base)` is a power of 2 within floating-point tolerance,
/// or `u32::MAX` otherwise.
#[inline]
pub fn GetLogFactor(base: f32, upper: f32) -> u32 {
    let dLog2factor = (1.0f64 * upper as f64 / base as f64).log10() / 2.0f64.log10();
    let dEpsilon = 0.0001f64;
    let dRound = (dLog2factor + 0.5f64).floor();

    if dLog2factor < dRound + dEpsilon && dRound < dLog2factor + dEpsilon {
        dRound as u32
    } else {
        u32::MAX
    }
}

/// Dependency Layer Internal Runtime Parameters
#[repr(C)]
#[derive(Debug, Copy, Clone)]
/// `TagDLayerParam` — `codec/encoder/core/inc/param_svc.h:82`. 68 bytes.
///
/// `sRecFileName` is **not** a member: `param_svc.h:98` guards it with
/// `#ifdef ENABLE_FRAME_DUMP`, and `as264_common.h:61-75` only defines that under
/// `WELS_TESTBED` or `__UNITTEST__`, neither of which the library build sets.
pub struct SSpatialLayerInternal {
    pub iActualWidth: i32,
    pub iActualHeight: i32,
    pub iTemporalResolution: i32,
    pub iDecompositionStages: i32,
    pub uiCodingIdx2TemporalId: [u8; (1 << MAX_TEMPORAL_LEVEL) + 1],
    pub iHighestTemporalId: i8,
    pub fInputFrameRate: f32,
    pub fOutputFrameRate: f32,
    pub uiIdrPicId: u16,
    pub iCodingIndex: i32,
    pub iFrameIndex: i32,
    pub bEncCurFrmAsIdrFlag: bool,
    pub iFrameNum: i32,
    pub iPOC: i32,
}

pub type TagDLayerParam = SSpatialLayerInternal;

impl Default for SSpatialLayerInternal {
    fn default() -> Self {
        Self {
            iActualWidth: 0,
            iActualHeight: 0,
            iTemporalResolution: 0,
            iDecompositionStages: 0,
            uiCodingIdx2TemporalId: [INVALID_TEMPORAL_ID; (1 << MAX_TEMPORAL_LEVEL) + 1],
            iHighestTemporalId: 0,
            fInputFrameRate: 0.0,
            fOutputFrameRate: 0.0,
            uiIdrPicId: 0,
            iCodingIndex: 0,
            iFrameIndex: 0,
            bEncCurFrmAsIdrFlag: false,
            iFrameNum: 0,
            iPOC: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SUsedPicRect {
    pub iLeft: i32,
    pub iTop: i32,
    pub iWidth: i32,
    pub iHeight: i32,
}

/// Cisco OpenH264 Encoder Parameter Configuration
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSvcCodingParam {
    // SEncParamExt base, in the exact order of api/codec_api.rs. C++ derives
    // (TagWelsSvcCodingParam: SEncParamExt, param_svc.h:106) so the base must be a
    // byte-identical 924-byte prefix; this list had bEnableFrameCroppingFlag and the
    // fields after it out of order, which changed the padding and the total size.
    pub iUsageType: EUsageType,
    pub iPicWidth: i32,
    pub iPicHeight: i32,
    pub iTargetBitrate: i32,
    pub iRCMode: RCMode,
    pub fMaxFrameRate: f32,
    pub iTemporalLayerNum: i32,
    pub iSpatialLayerNum: i32,
    pub sSpatialLayers: [SSpatialLayerConfig; MAX_SPATIAL_LAYER_NUM],
    pub iComplexityMode: EComplexityMode,
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

    // Internal SVC-specific members (param_svc.h:107 onward).
    pub sDependencyLayers: [SSpatialLayerInternal; MAX_DEPENDENCY_LAYER],
    pub uiGopSize: u32,
    pub SUsedPicRect: SUsedPicRect,
    // `param_svc.h:118`'s `pCurPath` stood here — **deleted, D-dead-7** (the user,
    // 2026-08-26, from F183). Three writes and no reader, in *both* trees: upstream
    // declares it (`param_svc.h:118`), nulls it (`:228`) and stores to it
    // (`welsEncoderExt.cpp:1076`), and never reads it anywhere in `codec/`; this
    // port had the same three and likewise never read it. D-dead-3's shape a fourth
    // time. `SetOption(ENCODER_OPTION_CURRENT_PATH)` keeps returning success and now
    // does nothing, which is observably what it already did.
    pub bDeblockingParallelFlag: bool,
    pub iBitsVaryPercentage: i32,
    pub iDecompStages: i8,
    pub iMaxNumRefFrame: i32,
}

pub type TagWelsSvcCodingParam = SWelsSvcCodingParam;

impl Default for SWelsSvcCodingParam {
    fn default() -> Self {
        let mut param = Self {
            iUsageType: EUsageType::CAMERA_VIDEO_REAL_TIME,
            iPicWidth: 0,
            iPicHeight: 0,
            iTargetBitrate: UNSPECIFIED_BIT_RATE,
            iRCMode: RCMode::RC_QUALITY_MODE,
            fMaxFrameRate: MAX_FRAME_RATE,
            iTemporalLayerNum: 1,
            iSpatialLayerNum: 1,
            sSpatialLayers: [SSpatialLayerConfig::default(); MAX_SPATIAL_LAYER_NUM],
            iComplexityMode: LOW_COMPLEXITY,
            uiIntraPeriod: 0,
            iNumRefFrame: AUTO_REF_PIC_COUNT,
            eSpsPpsIdStrategy: INCREASING_ID,
            bPrefixNalAddingCtrl: false,
            bEnableSSEI: false,
            bSimulcastAVC: false,
            iPaddingFlag: 0,
            iEntropyCodingModeFlag: 0,
            bEnableFrameCroppingFlag: true,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            bEnableDenoise: false,
            bEnableSceneChangeDetect: true,
            bEnableBackgroundDetection: true,
            bEnableAdaptiveQuant: true,
            bEnableFrameSkip: true,
            bEnableLongTermReference: false,
            iLtrMarkPeriod: 30,
            iMultipleThreadIdc: 1,
            bUseLoadBalancing: true,
            iMaxBitrate: UNSPECIFIED_BIT_RATE,
            iMinQp: QP_MIN_VALUE,
            iMaxQp: QP_MAX_VALUE,
            uiMaxNalSize: 0,
            bIsLosslessLink: false,
            iLTRRefNum: 0,
            bFixRCOverShoot: true,
            iIdrBitrateRatio: IDR_BITRATE_RATIO * 100,
            bPsnrY: false,
            bPsnrU: false,
            bPsnrV: false,

            sDependencyLayers: [SSpatialLayerInternal::default(); MAX_DEPENDENCY_LAYER],
            uiGopSize: 1,
            SUsedPicRect: SUsedPicRect::default(),
            bDeblockingParallelFlag: false,
            iBitsVaryPercentage: 10,
            iDecompStages: 0,
            iMaxNumRefFrame: AUTO_REF_PIC_COUNT,
        };
        param.FillDefault();
        param
    }
}

impl SWelsSvcCodingParam {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn FillDefaultExt(param: &mut SEncParamExt) {
        param.uiIntraPeriod = 0;
        param.iNumRefFrame = AUTO_REF_PIC_COUNT;
        param.iPicWidth = 0;
        param.iPicHeight = 0;
        param.fMaxFrameRate = MAX_FRAME_RATE;
        param.iComplexityMode = LOW_COMPLEXITY;
        param.iTargetBitrate = UNSPECIFIED_BIT_RATE;
        param.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        param.iMultipleThreadIdc = 1;
        param.bUseLoadBalancing = true;

        param.iLTRRefNum = 0;

        param.bEnableSSEI = false;
        param.bSimulcastAVC = false;
        param.bEnableFrameCroppingFlag = true;

        param.iLoopFilterDisableIdc = 0;
        param.iLoopFilterAlphaC0Offset = 0;
        param.iLoopFilterBetaOffset = 0;

        param.iRCMode = RCMode::RC_QUALITY_MODE;
        param.iPaddingFlag = 0;
        param.iEntropyCodingModeFlag = 0;
        param.bEnableDenoise = false;
        param.bEnableSceneChangeDetect = true;
        param.bEnableBackgroundDetection = true;
        param.bEnableAdaptiveQuant = true;
        param.bEnableFrameSkip = true;
        param.bEnableLongTermReference = false;
        param.iLtrMarkPeriod = 30;
        param.eSpsPpsIdStrategy = INCREASING_ID;
        param.bPrefixNalAddingCtrl = false;
        param.iSpatialLayerNum = 1;
        param.iTemporalLayerNum = 1;

        param.iMaxQp = QP_MAX_VALUE;
        param.iMinQp = QP_MIN_VALUE;
        param.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        param.uiMaxNalSize = 0;
        param.bIsLosslessLink = false;
        param.bFixRCOverShoot = true;
        param.iIdrBitrateRatio = IDR_BITRATE_RATIO * 100;
        param.bPsnrY = false;
        param.bPsnrU = false;
        param.bPsnrV = false;

        for iLayer in 0..MAX_SPATIAL_LAYER_NUM {
            let layer = &mut param.sSpatialLayers[iLayer];
            layer.uiProfileIdc = PRO_UNKNOWN;
            layer.uiLevelIdc = LEVEL_UNKNOWN;
            layer.iDLayerQp = SVC_QUALITY_BASE_QP;
            layer.fFrameRate = param.fMaxFrameRate;
            layer.iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
            layer.iSpatialBitrate = 0;
            layer.iVideoWidth = 0;
            layer.iVideoHeight = 0;

            layer.sSliceArgument.uiSliceMode = SliceMode::SM_SINGLE_SLICE;
            layer.sSliceArgument.uiSliceNum = 0;
            layer.sSliceArgument.uiSliceSizeConstraint = 1500;

            layer.bAspectRatioPresent = false;
            layer.eAspectRatio = ESampleAspectRatio::ASP_UNSPECIFIED;
            layer.sAspectRatioExtWidth = 0;
            layer.sAspectRatioExtHeight = 0;

            let kiLesserSliceNum = if MAX_SLICES_NUM < MAX_SLICES_NUM_TMP {
                MAX_SLICES_NUM
            } else {
                MAX_SLICES_NUM_TMP
            };
            for idx in 0..kiLesserSliceNum.min(layer.sSliceArgument.uiSliceMbNum.len()) {
                layer.sSliceArgument.uiSliceMbNum[idx] = 0;
            }

            // See codec_app_def.h: defaults write no colour information to the header.
            layer.bVideoSignalTypePresent = false;
            layer.uiVideoFormat = EVideoFormatSPS::VF_UNDEF as u8;
            layer.bFullRange = false;
            layer.bColorDescriptionPresent = false;
            layer.uiColorPrimaries = EColorPrimaries::CP_UNDEF as u8;
            layer.uiTransferCharacteristics = ETransferCharacteristics::TRC_UNDEF as u8;
            layer.uiColorMatrix = EColorMatrix::CM_UNDEF as u8;
        }
    }

    pub fn FillDefault(&mut self) {
        self.uiIntraPeriod = 0;
        self.iNumRefFrame = AUTO_REF_PIC_COUNT;
        self.iPicWidth = 0;
        self.iPicHeight = 0;
        self.fMaxFrameRate = MAX_FRAME_RATE;
        self.iComplexityMode = LOW_COMPLEXITY;
        self.iTargetBitrate = UNSPECIFIED_BIT_RATE;
        self.iMaxBitrate = UNSPECIFIED_BIT_RATE;
        self.iMultipleThreadIdc = 1;
        self.bUseLoadBalancing = true;

        self.iLTRRefNum = 0;
        self.iLtrMarkPeriod = 30;

        self.bEnableSSEI = false;
        self.bSimulcastAVC = false;
        self.bEnableFrameCroppingFlag = true;

        self.iLoopFilterDisableIdc = 0;
        self.iLoopFilterAlphaC0Offset = 0;
        self.iLoopFilterBetaOffset = 0;

        self.iRCMode = RCMode::RC_QUALITY_MODE;
        self.iPaddingFlag = 0;
        self.iEntropyCodingModeFlag = 0;
        self.bEnableDenoise = false;
        self.bEnableSceneChangeDetect = true;
        self.bEnableBackgroundDetection = true;
        self.bEnableAdaptiveQuant = true;
        self.bEnableFrameSkip = true;
        self.bEnableLongTermReference = false;
        self.eSpsPpsIdStrategy = INCREASING_ID;
        self.bPrefixNalAddingCtrl = false;
        self.iSpatialLayerNum = 1;
        self.iTemporalLayerNum = 1;

        self.iMaxQp = QP_MAX_VALUE;
        self.iMinQp = QP_MIN_VALUE;
        self.iUsageType = EUsageType::CAMERA_VIDEO_REAL_TIME;
        self.uiMaxNalSize = 0;
        self.bIsLosslessLink = false;
        self.bFixRCOverShoot = true;
        self.iIdrBitrateRatio = IDR_BITRATE_RATIO * 100;
        self.bPsnrY = false;
        self.bPsnrU = false;
        self.bPsnrV = false;

        for iLayer in 0..MAX_SPATIAL_LAYER_NUM {
            let layer = &mut self.sSpatialLayers[iLayer];
            layer.uiProfileIdc = PRO_UNKNOWN;
            layer.uiLevelIdc = LEVEL_UNKNOWN;
            layer.iDLayerQp = SVC_QUALITY_BASE_QP;
            layer.fFrameRate = self.fMaxFrameRate;
            layer.iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
            layer.iSpatialBitrate = 0;
            layer.iVideoWidth = 0;
            layer.iVideoHeight = 0;

            layer.sSliceArgument.uiSliceMode = SliceMode::SM_SINGLE_SLICE;
            layer.sSliceArgument.uiSliceNum = 0;
            layer.sSliceArgument.uiSliceSizeConstraint = 1500;

            let kiLesserSliceNum = if MAX_SLICES_NUM < MAX_SLICES_NUM_TMP {
                MAX_SLICES_NUM
            } else {
                MAX_SLICES_NUM_TMP
            };
            for idx in 0..kiLesserSliceNum.min(layer.sSliceArgument.uiSliceMbNum.len()) {
                layer.sSliceArgument.uiSliceMbNum[idx] = 0;
            }
        }

        self.uiGopSize = 1;
        self.iMaxNumRefFrame = AUTO_REF_PIC_COUNT;
        self.SUsedPicRect = SUsedPicRect::default();
        self.bDeblockingParallelFlag = false;
        self.iDecompStages = 0;
        self.iBitsVaryPercentage = 10;
    }

    pub fn ParamBaseTranscode(&mut self, pCodingParam: &SEncParamBase) -> i32 {
        self.fMaxFrameRate = WELS_CLIP3(pCodingParam.fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);
        self.iTargetBitrate = pCodingParam.iTargetBitrate;
        self.iUsageType = pCodingParam.iUsageType;
        self.iPicWidth = pCodingParam.iPicWidth;
        self.iPicHeight = pCodingParam.iPicHeight;

        self.SUsedPicRect.iLeft = 0;
        self.SUsedPicRect.iTop = 0;
        self.SUsedPicRect.iWidth = (self.iPicWidth >> 1) * (1 << 1);
        self.SUsedPicRect.iHeight = (self.iPicHeight >> 1) * (1 << 1);

        self.iRCMode = pCodingParam.iRCMode;

        let mut iIdxSpatial: i32 = 0;
        let mut uiProfileIdc: EProfileIdc = if self.iEntropyCodingModeFlag != 0 {
            PRO_MAIN
        } else {
            PRO_UNKNOWN
        };

        while iIdxSpatial < self.iSpatialLayerNum {
            let idx = iIdxSpatial as usize;
            // `sSpatialLayers->uiProfileIdc` in the C++ is `sSpatialLayers[0]`, on
            // every iteration -- not `[iIdxSpatial]`. Five fields here decay the
            // array to a pointer that way (uiProfileIdc, uiLevelIdc,
            // iSpatialBitrate, iMaxSpatialBitrate, iDLayerQp) and five index it
            // properly (fFrameRate, iVideoWidth/Height and the two internal ones).
            // Writing both is not "a superset": at more than one spatial layer the
            // reference leaves `[1..]`'s profile, level, bitrate and QP at whatever
            // FillDefault left, and `[0]`'s profile ends at PRO_SCALABLE_BASELINE
            // because the last iteration rewrites it. Faithful to param_svc.h:222.
            self.sSpatialLayers[0].uiProfileIdc = uiProfileIdc;
            self.sSpatialLayers[0].uiLevelIdc = LEVEL_UNKNOWN;

            self.sSpatialLayers[idx].fFrameRate =
                WELS_CLIP3(pCodingParam.fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);
            self.sDependencyLayers[idx].fInputFrameRate = WELS_CLIP3(
                self.sSpatialLayers[idx].fFrameRate,
                MIN_FRAME_RATE,
                MAX_FRAME_RATE,
            );
            self.sDependencyLayers[idx].fOutputFrameRate =
                self.sDependencyLayers[idx].fInputFrameRate;

            self.sSpatialLayers[idx].iVideoWidth = self.iPicWidth;
            self.sDependencyLayers[idx].iActualWidth = self.iPicWidth;
            self.sSpatialLayers[idx].iVideoHeight = self.iPicHeight;
            self.sDependencyLayers[idx].iActualHeight = self.iPicHeight;

            // `sSpatialLayers->iSpatialBitrate = sSpatialLayers[iIdxSpatial]
            // .iSpatialBitrate = ...` -- this one really does write both.
            self.sSpatialLayers[idx].iSpatialBitrate = pCodingParam.iTargetBitrate;
            self.sSpatialLayers[0].iSpatialBitrate = pCodingParam.iTargetBitrate;

            self.sSpatialLayers[0].iMaxSpatialBitrate = UNSPECIFIED_BIT_RATE;
            self.sSpatialLayers[0].iDLayerQp = SVC_QUALITY_BASE_QP;

            uiProfileIdc = if !self.bSimulcastAVC {
                PRO_SCALABLE_BASELINE
            } else {
                uiProfileIdc
            };
            iIdxSpatial += 1;
        }

        self.SetActualPicResolution();
        0
    }

    pub fn GetBaseParams(&self, pCodingParam: &mut SEncParamBase) {
        pCodingParam.iUsageType = self.iUsageType;
        pCodingParam.iPicWidth = self.iPicWidth;
        pCodingParam.iPicHeight = self.iPicHeight;
        pCodingParam.iTargetBitrate = self.iTargetBitrate;
        pCodingParam.iRCMode = self.iRCMode;
        pCodingParam.fMaxFrameRate = self.fMaxFrameRate;
    }

    pub fn ParamTranscode(&mut self, pCodingParam: &SEncParamExt) -> i32 {
        let fParamMaxFrameRate =
            WELS_CLIP3(pCodingParam.fMaxFrameRate, MIN_FRAME_RATE, MAX_FRAME_RATE);
        self.iUsageType = pCodingParam.iUsageType;
        self.iPicWidth = pCodingParam.iPicWidth;
        self.iPicHeight = pCodingParam.iPicHeight;
        self.fMaxFrameRate = fParamMaxFrameRate;
        self.iComplexityMode = pCodingParam.iComplexityMode;

        self.SUsedPicRect.iLeft = 0;
        self.SUsedPicRect.iTop = 0;
        self.SUsedPicRect.iWidth = (self.iPicWidth >> 1) << 1;
        self.SUsedPicRect.iHeight = (self.iPicHeight >> 1) << 1;

        self.iMultipleThreadIdc = pCodingParam.iMultipleThreadIdc;
        self.bUseLoadBalancing = pCodingParam.bUseLoadBalancing;

        self.iLoopFilterDisableIdc = pCodingParam.iLoopFilterDisableIdc;
        self.iLoopFilterAlphaC0Offset = pCodingParam.iLoopFilterAlphaC0Offset;
        self.iLoopFilterBetaOffset = pCodingParam.iLoopFilterBetaOffset;
        self.iEntropyCodingModeFlag = pCodingParam.iEntropyCodingModeFlag;
        self.bEnableFrameCroppingFlag = pCodingParam.bEnableFrameCroppingFlag;

        self.iRCMode = pCodingParam.iRCMode;
        self.bSimulcastAVC = pCodingParam.bSimulcastAVC;
        self.iPaddingFlag = pCodingParam.iPaddingFlag;

        self.iTargetBitrate = pCodingParam.iTargetBitrate;
        self.iMaxBitrate = pCodingParam.iMaxBitrate;
        if self.iMaxBitrate != UNSPECIFIED_BIT_RATE && self.iMaxBitrate < self.iTargetBitrate {
            self.iMaxBitrate = self.iTargetBitrate;
        }

        self.iMaxQp = pCodingParam.iMaxQp;
        self.iMinQp = pCodingParam.iMinQp;
        self.uiMaxNalSize = pCodingParam.uiMaxNalSize;
        self.bEnableDenoise = pCodingParam.bEnableDenoise;
        self.bEnableSceneChangeDetect = pCodingParam.bEnableSceneChangeDetect;
        self.bEnableBackgroundDetection = pCodingParam.bEnableBackgroundDetection;
        self.bEnableAdaptiveQuant = pCodingParam.bEnableAdaptiveQuant;
        self.bEnableFrameSkip = pCodingParam.bEnableFrameSkip;

        self.bEnableLongTermReference = pCodingParam.bEnableLongTermReference;
        self.iLtrMarkPeriod = pCodingParam.iLtrMarkPeriod;
        self.bIsLosslessLink = pCodingParam.bIsLosslessLink;
        // These five are *copied* here (`param_svc.h:349-353`); the constants above
        // belong to `FillDefault`. Hardcoding them made `bFixRCOverShoot` true for
        // every caller, which sends `RcInitVGop` down its carry-over arm.
        self.bFixRCOverShoot = pCodingParam.bFixRCOverShoot;
        self.iIdrBitrateRatio = pCodingParam.iIdrBitrateRatio;
        self.bPsnrY = pCodingParam.bPsnrY;
        self.bPsnrU = pCodingParam.bPsnrU;
        self.bPsnrV = pCodingParam.bPsnrV;

        if self.iUsageType == EUsageType::SCREEN_CONTENT_REAL_TIME
            && !self.bIsLosslessLink
            && self.bEnableLongTermReference
        {
            self.bEnableLongTermReference = false;
        }

        self.bEnableSSEI = pCodingParam.bEnableSSEI;
        self.bSimulcastAVC = pCodingParam.bSimulcastAVC;

        self.iSpatialLayerNum = WELS_CLIP3(
            pCodingParam.iSpatialLayerNum,
            1,
            MAX_DEPENDENCY_LAYER as i32,
        );
        self.iTemporalLayerNum = WELS_CLIP3(
            pCodingParam.iTemporalLayerNum,
            1,
            MAX_TEMPORAL_LEVEL as i32,
        );

        self.uiGopSize = 1 << (self.iTemporalLayerNum - 1);
        self.iDecompStages = (self.iTemporalLayerNum - 1) as i8;
        self.uiIntraPeriod = pCodingParam.uiIntraPeriod;
        if self.uiIntraPeriod == u32::MAX {
            self.uiIntraPeriod = 0;
        } else if (self.uiIntraPeriod & (self.uiGopSize - 1)) != 0 {
            self.uiIntraPeriod = ((self.uiIntraPeriod + self.uiGopSize - 1) / self.uiGopSize)
                * self.uiGopSize;
        }

        if (pCodingParam.iNumRefFrame != AUTO_REF_PIC_COUNT
            && !(pCodingParam.iNumRefFrame > MAX_REF_PIC_COUNT
                || pCodingParam.iNumRefFrame < MIN_REF_PIC_COUNT))
            || (self.iNumRefFrame != AUTO_REF_PIC_COUNT
                && pCodingParam.iNumRefFrame == AUTO_REF_PIC_COUNT)
        {
            self.iNumRefFrame = pCodingParam.iNumRefFrame;
        }
        if self.iNumRefFrame != AUTO_REF_PIC_COUNT && self.iNumRefFrame > self.iMaxNumRefFrame {
            self.iMaxNumRefFrame = self.iNumRefFrame;
        }

        // **F62** (`param_svc.h:384`): `iLTRRefNum = (bEnableLongTermReference ?
        // pCodingParam.iLTRRefNum : 0)`. This read `0 : 0` — the caller's value was
        // dropped on the floor — and nothing saw it, because no configuration the
        // differential harness could express ever set `bEnableLongTermReference` until
        // Phase 6 session F added the `ltr` preset. It is *masked* rather than
        // observable even so: `WelsCheckNumRefSetting` (`au_set.rs`) overwrites the
        // field with `LONG_TERM_REF_NUM` on every init path that reaches it. Fixed as
        // a transcription, not as a behaviour change — the `ltr` preset reads 16/16
        // byte-identical either way.
        self.iLTRRefNum = if pCodingParam.bEnableLongTermReference {
            pCodingParam.iLTRRefNum
        } else {
            0
        };
        self.iLtrMarkPeriod = pCodingParam.iLtrMarkPeriod;
        self.bPrefixNalAddingCtrl = pCodingParam.bPrefixNalAddingCtrl;

        if pCodingParam.eSpsPpsIdStrategy == CONSTANT_ID
            || pCodingParam.eSpsPpsIdStrategy == INCREASING_ID
            || pCodingParam.eSpsPpsIdStrategy == SPS_LISTING
            || pCodingParam.eSpsPpsIdStrategy == SPS_LISTING_AND_PPS_INCREASING
            || pCodingParam.eSpsPpsIdStrategy == SPS_PPS_LISTING
        {
            self.eSpsPpsIdStrategy = pCodingParam.eSpsPpsIdStrategy;
        }

        let mut uiProfileIdc: EProfileIdc = if self.iEntropyCodingModeFlag != 0 {
            PRO_HIGH
        } else {
            PRO_BASELINE
        };

        let mut iIdxSpatial: i32 = 0;
        while iIdxSpatial < self.iSpatialLayerNum {
            let idx = iIdxSpatial as usize;
            self.sSpatialLayers[idx].uiProfileIdc =
                if pCodingParam.sSpatialLayers[idx].uiProfileIdc == PRO_UNKNOWN {
                    uiProfileIdc
                } else {
                    pCodingParam.sSpatialLayers[idx].uiProfileIdc
                };
            self.sSpatialLayers[idx].uiLevelIdc = pCodingParam.sSpatialLayers[idx].uiLevelIdc;

            let fLayerFrameRate = WELS_CLIP3(
                pCodingParam.sSpatialLayers[idx].fFrameRate,
                MIN_FRAME_RATE,
                fParamMaxFrameRate,
            );
            self.sDependencyLayers[idx].fInputFrameRate = fParamMaxFrameRate;
            self.sSpatialLayers[idx].fFrameRate =
                WELS_CLIP3(fLayerFrameRate, MIN_FRAME_RATE, fParamMaxFrameRate);
            self.sDependencyLayers[idx].fOutputFrameRate = self.sSpatialLayers[idx].fFrameRate;

            self.sSpatialLayers[idx].iVideoWidth = WELS_CLIP3(
                pCodingParam.sSpatialLayers[idx].iVideoWidth,
                0,
                self.iPicWidth,
            );
            self.sSpatialLayers[idx].iVideoHeight = WELS_CLIP3(
                pCodingParam.sSpatialLayers[idx].iVideoHeight,
                0,
                self.iPicHeight,
            );

            self.sSpatialLayers[idx].iSpatialBitrate =
                pCodingParam.sSpatialLayers[idx].iSpatialBitrate;
            self.sSpatialLayers[idx].iMaxSpatialBitrate =
                pCodingParam.sSpatialLayers[idx].iMaxSpatialBitrate;

            if self.iSpatialLayerNum == 1 && iIdxSpatial == 0 {
                if self.sSpatialLayers[idx].iVideoWidth == 0 {
                    self.sSpatialLayers[idx].iVideoWidth = self.iPicWidth;
                }
                if self.sSpatialLayers[idx].iVideoHeight == 0 {
                    self.sSpatialLayers[idx].iVideoHeight = self.iPicHeight;
                }
                if self.sSpatialLayers[idx].iSpatialBitrate == 0 {
                    self.sSpatialLayers[idx].iSpatialBitrate = self.iTargetBitrate;
                }
                if self.sSpatialLayers[idx].iMaxSpatialBitrate == 0 {
                    self.sSpatialLayers[idx].iMaxSpatialBitrate = self.iMaxBitrate;
                }
            }

            self.sSpatialLayers[idx].sSliceArgument =
                pCodingParam.sSpatialLayers[idx].sSliceArgument;
            self.sSpatialLayers[idx].iDLayerQp = pCodingParam.sSpatialLayers[idx].iDLayerQp;

            uiProfileIdc = if !self.bSimulcastAVC {
                PRO_SCALABLE_BASELINE
            } else {
                uiProfileIdc
            };
            iIdxSpatial += 1;
        }

        self.SetActualPicResolution();
        0
    }

    pub fn SetActualPicResolution(&mut self) {
        let mut iSpatialIdx = self.iSpatialLayerNum - 1;
        while iSpatialIdx >= 0 {
            let idx = iSpatialIdx as usize;
            self.sDependencyLayers[idx].iActualWidth = self.sSpatialLayers[idx].iVideoWidth;
            self.sDependencyLayers[idx].iActualHeight = self.sSpatialLayers[idx].iVideoHeight;
            self.sSpatialLayers[idx].iVideoWidth =
                WELS_ALIGN(self.sDependencyLayers[idx].iActualWidth, MB_WIDTH_LUMA);
            self.sSpatialLayers[idx].iVideoHeight =
                WELS_ALIGN(self.sDependencyLayers[idx].iActualHeight, MB_HEIGHT_LUMA);
            iSpatialIdx -= 1;
        }
    }

    /// Base-class slice of the C++ `TagWelsSvcCodingParam : SEncParamExt`
    /// inheritance, which the flattened Rust struct has to spell out.
    pub fn to_param_ext(&self) -> SEncParamExt {
        SEncParamExt {
            iUsageType: self.iUsageType,
            iPicWidth: self.iPicWidth,
            iPicHeight: self.iPicHeight,
            iTargetBitrate: self.iTargetBitrate,
            iRCMode: self.iRCMode,
            fMaxFrameRate: self.fMaxFrameRate,
            iTemporalLayerNum: self.iTemporalLayerNum,
            iSpatialLayerNum: self.iSpatialLayerNum,
            sSpatialLayers: self.sSpatialLayers,
            iComplexityMode: self.iComplexityMode,
            uiIntraPeriod: self.uiIntraPeriod,
            iNumRefFrame: self.iNumRefFrame,
            eSpsPpsIdStrategy: self.eSpsPpsIdStrategy,
            bPrefixNalAddingCtrl: self.bPrefixNalAddingCtrl,
            bEnableSSEI: self.bEnableSSEI,
            bSimulcastAVC: self.bSimulcastAVC,
            iPaddingFlag: self.iPaddingFlag,
            iEntropyCodingModeFlag: self.iEntropyCodingModeFlag,
            bEnableFrameCroppingFlag: self.bEnableFrameCroppingFlag,
            iLoopFilterDisableIdc: self.iLoopFilterDisableIdc,
            iLoopFilterAlphaC0Offset: self.iLoopFilterAlphaC0Offset,
            iLoopFilterBetaOffset: self.iLoopFilterBetaOffset,
            bEnableDenoise: self.bEnableDenoise,
            bEnableSceneChangeDetect: self.bEnableSceneChangeDetect,
            bEnableBackgroundDetection: self.bEnableBackgroundDetection,
            bEnableAdaptiveQuant: self.bEnableAdaptiveQuant,
            bEnableFrameSkip: self.bEnableFrameSkip,
            bEnableLongTermReference: self.bEnableLongTermReference,
            iLtrMarkPeriod: self.iLtrMarkPeriod,
            iMultipleThreadIdc: self.iMultipleThreadIdc,
            bUseLoadBalancing: self.bUseLoadBalancing,
            iMaxBitrate: self.iMaxBitrate,
            iMinQp: self.iMinQp,
            iMaxQp: self.iMaxQp,
            uiMaxNalSize: self.uiMaxNalSize,
            bIsLosslessLink: self.bIsLosslessLink,
            iLTRRefNum: self.iLTRRefNum,
            bFixRCOverShoot: self.bFixRCOverShoot,
            iIdrBitrateRatio: self.iIdrBitrateRatio,
            bPsnrY: self.bPsnrY,
            bPsnrU: self.bPsnrU,
            bPsnrV: self.bPsnrV,
        }
    }

    pub fn DetermineTemporalSettings(&mut self) -> i32 {
        let iDecStages = WELS_LOG2(self.uiGopSize);
        let pTemporalIdList = &g_kuiTemporalIdListTable[iDecStages as usize];

        let mut i: i32 = 0;
        while i < self.iSpatialLayerNum {
            let idx = i as usize;
            let kuiLogFactorInOutRate = GetLogFactor(
                self.sDependencyLayers[idx].fOutputFrameRate,
                self.sDependencyLayers[idx].fInputFrameRate,
            );
            let kuiLogFactorMaxInRate = GetLogFactor(
                self.sDependencyLayers[idx].fInputFrameRate,
                self.fMaxFrameRate,
            );

            if u32::MAX == kuiLogFactorInOutRate || u32::MAX == kuiLogFactorMaxInRate {
                return ENC_RETURN_INVALIDINPUT;
            }

            self.sDependencyLayers[idx]
                .uiCodingIdx2TemporalId
                .fill(INVALID_TEMPORAL_ID);

            let iNotCodedMask = (1 << (kuiLogFactorInOutRate + kuiLogFactorMaxInRate)) - 1;
            let mut iMaxTemporalId: i8 = 0;

            for uiFrameIdx in 0..=self.uiGopSize {
                if 0 == (uiFrameIdx & (iNotCodedMask as u32)) {
                    let kiTemporalId = pTemporalIdList[uiFrameIdx as usize] as i8;
                    self.sDependencyLayers[idx].uiCodingIdx2TemporalId[uiFrameIdx as usize] =
                        kiTemporalId as u8;
                    if kiTemporalId > iMaxTemporalId {
                        iMaxTemporalId = kiTemporalId;
                    }
                }
            }

            self.sDependencyLayers[idx].iHighestTemporalId = iMaxTemporalId;
            self.sDependencyLayers[idx].iTemporalResolution =
                (kuiLogFactorMaxInRate + kuiLogFactorInOutRate) as i32;
            self.sDependencyLayers[idx].iDecompositionStages =
                iDecStages - (kuiLogFactorMaxInRate + kuiLogFactorInOutRate) as i32;

            if self.sDependencyLayers[idx].iDecompositionStages < 0 {
                return ENC_RETURN_INVALIDINPUT;
            }

            i += 1;
        }

        self.iDecompStages = iDecStages as i8;
        ENC_RETURN_SUCCESS
    }
}

/// A parameter set's **position in the encoder context's array of them** — Phase 6
/// session G.
///
/// The context used to carry `pSps`/`pPps`/`pSubsetSps` as pointers *into*
/// `pSpsArray`/`pPPSArray`/`pSubsetArray`, and the layer and slice headers carried
/// copies of the same addresses. That is cache-not-carrier with the id already
/// named: **`SDqIdc` stores exactly these two numbers as data** (`iPpsId: u16`,
/// `iSpsId: u8`) because `InitDqLayers` writes them there, and the C++ itself indexes
/// the arrays with them (`&pCtx->pPPSArray[iCurPpsId]`). These types are those
/// numbers, given names.
///
/// **Position, not the syntax element.** `SWelsSPS::uiSpsId` and `SWelsPPS::iPpsId`
/// are what goes on the wire, and the id strategy may add an offset to the latter
/// when it does (`GetPpsIdOffset`). They agree with the position today because
/// `WelsGenerateSps`/`WelsGeneratePps` stamp each set with its own index — but they
/// are different things, and only one of them can index an array.
///
/// The widths are `SDqIdc`'s, which are the C++'s: the arrays are bounded by
/// `MAX_SPS_COUNT` and `MAX_PPS_COUNT` (57), so both fit several times over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpsId(pub u8);

/// A PPS's position in `sWelsEncCtx::pPPSArray` — see [`SpsId`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PpsId(pub u16);

/// A subset SPS's position in `sWelsEncCtx::pSubsetArray` — see [`SpsId`].
///
/// **Its own type, because the id strategy has its own space for it**:
/// `PARA_SET_TYPE_AVCSPS`, `PARA_SET_TYPE_SUBSETSPS` and `PARA_SET_TYPE_PPS` are
/// three separate id counters in `paraset_strategy.rs`, and the context keeps three
/// separate arrays. `WelsInitCurrentLayer` indexes `pSpsArray` and `pSubsetArray`
/// with the same local (`iCurSpsId`) in its two arms, which is precisely the
/// confusion a shared type would let through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubsetSpsId(pub u8);

macro_rules! paraset_id {
    ($t:ident, $raw:ty) => {
        impl $t {
            #[inline(always)]
            pub fn get(self) -> usize {
                self.0 as usize
            }
        }
        impl From<$t> for $raw {
            #[inline(always)]
            fn from(v: $t) -> $raw {
                v.0
            }
        }
    };
}
paraset_id!(SpsId, u8);
paraset_id!(PpsId, u16);
paraset_id!(SubsetSpsId, u8);

/// Frame crop offset syntax element in SPS

/// Sequence Parameter Set (SPS) syntax structure
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSPS {
    pub uiSpsId: u32,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    pub uiLog2MaxFrameNum: u32,
    pub uiPocType: u32,
    pub iLog2MaxPocLsb: i32,
    pub sFrameCrop: SCropOffset,
    pub iNumRefFrames: i16,
    pub uiProfileIdc: u8,
    pub iLevelIdc: u8,
    pub bGapsInFrameNumValueAllowedFlag: bool,
    pub bFrameCroppingFlag: bool,
    pub bVuiParamPresentFlag: bool,
    pub bVideoSignalTypePresent: bool,
    pub uiVideoFormat: u8,
    pub bFullRange: bool,
    pub bColorDescriptionPresent: bool,
    pub uiColorPrimaries: u8,
    pub uiTransferCharacteristics: u8,
    pub uiColorMatrix: u8,
    pub bConstraintSet0Flag: bool,
    pub bConstraintSet1Flag: bool,
    pub bConstraintSet2Flag: bool,
    pub bConstraintSet3Flag: bool,
    pub bAspectRatioPresent: bool,
    pub eAspectRatio: i32,
    pub sAspectRatioExtWidth: u16,
    pub sAspectRatioExtHeight: u16,
}

/// **The C++'s `memset (pSps, 0, sizeof (SWelsSPS))`, spelled out** — T6.G3.
///
/// `WelsInitSps` and `WelsInitSubsetSps` begin with that memset, and this is what
/// they assign now that they take a `&mut` instead of a `*mut`. It is deliberately
/// **not** [`Default`](SWelsSPS::default), which seeds `uiProfileIdc = PRO_BASELINE`
/// and the VUI `*_UNDEF` values — the port has carried that warning as a comment at
/// the memset since Phase 3, and this is the same statement as a value. F56's rule:
/// a zero image is ruled, not defaulted, and the two are different here.
impl SWelsSPS {
    pub const ZERO: Self = Self {
        uiSpsId: 0,
        iMbWidth: 0,
        iMbHeight: 0,
        uiLog2MaxFrameNum: 0,
        uiPocType: 0,
        iLog2MaxPocLsb: 0,
        sFrameCrop: SCropOffset { iCropLeft: 0, iCropRight: 0, iCropTop: 0, iCropBottom: 0 },
        iNumRefFrames: 0,
        // 0 is not `PRO_BASELINE`; `WelsInitSps` sets it, and its subset-SPS caller
        // deliberately takes `uiProfileIdc` verbatim with no fallback for 0.
        uiProfileIdc: 0,
        iLevelIdc: 0,
        bGapsInFrameNumValueAllowedFlag: false,
        bFrameCroppingFlag: false,
        bVuiParamPresentFlag: false,
        bVideoSignalTypePresent: false,
        uiVideoFormat: 0,
        bFullRange: false,
        bColorDescriptionPresent: false,
        // 0 in each of these three is *not* the `*_UNDEF` that `Default` seeds.
        uiColorPrimaries: 0,
        uiTransferCharacteristics: 0,
        uiColorMatrix: 0,
        bConstraintSet0Flag: false,
        bConstraintSet1Flag: false,
        bConstraintSet2Flag: false,
        bConstraintSet3Flag: false,
        bAspectRatioPresent: false,
        eAspectRatio: 0,
        sAspectRatioExtWidth: 0,
        sAspectRatioExtHeight: 0,
    };
}

/// The zero image of the SVC extension block — see [`SWelsSPS::ZERO`].
impl SSpsSvcExt {
    pub const ZERO: Self = Self {
        iExtendedSpatialScalability: 0,
        bSeqTcoeffLevelPredFlag: false,
        bAdaptiveTcoeffLevelPredFlag: false,
        bSliceHeaderRestrictionFlag: false,
    };
}

/// The zero image of a PPS — `WelsMallocz`'s zeros for `pCtx->pPPSArray`, spelled
/// out. [`Default`](SWelsPPS::default) happens to agree field for field today; this
/// exists so that if it ever stops agreeing — as `SWelsSPS`'s already does — the
/// array does not silently change with it. See [`SWelsSPS::ZERO`].
impl SWelsPPS {
    pub const ZERO: Self = Self {
        iSpsId: 0,
        iPpsId: 0,
        iPicInitQp: 0,
        iPicInitQs: 0,
        uiChromaQpIndexOffset: 0,
        bEntropyCodingModeFlag: false,
        bDeblockingFilterControlPresentFlag: false,
    };
}

/// The zero image of a whole subset SPS — `WelsInitSubsetSps`'s
/// `memset (pSubsetSps, 0, sizeof (SSubsetSps))`. See [`SWelsSPS::ZERO`].
impl SSubsetSps {
    pub const ZERO: Self = Self {
        pSps: SWelsSPS::ZERO,
        sSpsSvcExt: SSpsSvcExt::ZERO,
    };
}

impl Default for SWelsSPS {
    fn default() -> Self {
        Self {
            uiSpsId: 0,
            iMbWidth: 0,
            iMbHeight: 0,
            uiLog2MaxFrameNum: 0,
            uiPocType: 0,
            iLog2MaxPocLsb: 0,
            sFrameCrop: SCropOffset::default(),
            iNumRefFrames: 0,
            uiProfileIdc: PRO_BASELINE as u8,
            iLevelIdc: 0,
            bGapsInFrameNumValueAllowedFlag: false,
            bFrameCroppingFlag: false,
            bVuiParamPresentFlag: false,
            bVideoSignalTypePresent: false,
            uiVideoFormat: VF_UNDEF,
            bFullRange: false,
            bColorDescriptionPresent: false,
            uiColorPrimaries: CP_UNDEF,
            uiTransferCharacteristics: TRC_UNDEF,
            uiColorMatrix: CM_UNDEF,
            bConstraintSet0Flag: false,
            bConstraintSet1Flag: false,
            bConstraintSet2Flag: false,
            bConstraintSet3Flag: false,
            bAspectRatioPresent: false,
            eAspectRatio: ASP_UNSPECIFIED,
            sAspectRatioExtWidth: 0,
            sAspectRatioExtHeight: 0,
        }
    }
}

/// SPS SVC extension syntax elements
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpsSvcExt {
    pub iExtendedSpatialScalability: u8,
    pub bSeqTcoeffLevelPredFlag: bool,
    pub bAdaptiveTcoeffLevelPredFlag: bool,
    pub bSliceHeaderRestrictionFlag: bool,
}

/// Subset Sequence Parameter Set
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSubsetSps {
    pub pSps: SWelsSPS,
    pub sSpsSvcExt: SSpsSvcExt,
}

/// Picture Parameter Set (PPS) syntax structure.
///
/// `TagWelsPPS` — `codec/encoder/core/inc/parameter_sets.h:136`. **16 bytes**, with
/// `iPicInitQp` at offset 8.
///
/// The nine FMO fields (`uiNumSliceGroups` … `uiSliceGroupId`) that this port used to
/// declare here sit inside `#if !defined(DISABLE_FMO_FEATURE)`, and
/// `codec/encoder/core/inc/as264_common.h:53` defines `DISABLE_FMO_FEATURE`
/// unconditionally — so they are **not** part of the struct the C++ encoder compiles.
/// Including them made this struct roughly nine times too large.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsPPS {
    pub iSpsId: u32,
    pub iPpsId: u32,
    pub iPicInitQp: i8,
    pub iPicInitQs: i8,
    pub uiChromaQpIndexOffset: u8,
    pub bEntropyCodingModeFlag: bool,
    pub bDeblockingFilterControlPresentFlag: bool,
}

impl Default for SWelsPPS {
    fn default() -> Self {
        Self {
            iSpsId: 0,
            iPpsId: 0,
            iPicInitQp: 0,
            iPicInitQs: 0,
            uiChromaQpIndexOffset: 0,
            bEntropyCodingModeFlag: false,
            bDeblockingFilterControlPresentFlag: false,
        }
    }
}

/// Cache list for SPS, Subset-SPS, and PPS parameter sets
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SExistingParasetList {
    pub sSps: [SWelsSPS; MAX_SPS_COUNT],
    pub sSubsetSps: [SSubsetSps; MAX_SPS_COUNT],
    pub sPps: [SWelsPPS; MAX_PPS_COUNT],
    pub uiInUseSpsNum: u32,
    pub uiInUseSubsetSpsNum: u32,
    pub uiInUsePpsNum: u32,
}

pub type TagExistingParasetList = SExistingParasetList;

impl Default for SExistingParasetList {
    fn default() -> Self {
        Self {
            sSps: [SWelsSPS::default(); MAX_SPS_COUNT],
            sSubsetSps: [SSubsetSps::default(); MAX_SPS_COUNT],
            sPps: [SWelsPPS::default(); MAX_PPS_COUNT],
            uiInUseSpsNum: 0,
            uiInUseSubsetSpsNum: 0,
            uiInUsePpsNum: 0,
        }
    }
}

/// Releases dynamic coding param buffer via CMemoryAlign
/// The encoder's own copy of the coding parameters — **T6.H11**, and what
/// `AllocCodingParam`/`FreeCodingParam` became.
///
/// The pair was `WelsMallocz` + `FillDefault` and a matching `WelsFree`. The context
/// owns the block (see the field's own note for the ownership read this session did),
/// so the free is its drop and the allocation is a `Box`.
///
/// **The starting image is `Default`'s, not a memset's, and that is a deviation
/// worth one line**: the C++ zeroes the block and then calls `FillDefault`, which
/// writes some of it. `SWelsSvcCodingParam` holds `repr(C)` enums whose zero is not
/// in every case a declared variant, so there is no safe zero image to reproduce.
/// It is unobservable: `WelsInitEncoderExt` assigns the caller's whole parameter
/// struct over this one on the very next line, and it is the only live caller.
pub fn NewCodingParam() -> Box<SWelsSvcCodingParam> {
    let mut p = Box::new(SWelsSvcCodingParam::default());
    p.FillDefault();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_log_factor() {
        assert_eq!(GetLogFactor(1.0, 1.0), 0);
        assert_eq!(GetLogFactor(1.0, 2.0), 1);
        assert_eq!(GetLogFactor(1.0, 4.0), 2);
        assert_eq!(GetLogFactor(1.0, 8.0), 3);
        assert_eq!(GetLogFactor(15.0, 30.0), 1);
        assert_eq!(GetLogFactor(3.0, 10.0), u32::MAX);
    }

    #[test]
    fn test_param_defaults() {
        let param = SWelsSvcCodingParam::new();
        assert_eq!(param.uiGopSize, 1);
        // MAX_FRAME_RATE is 60 (wels_const.h:60). Measured: the reference's
        // GetDefaultParams returns fMaxFrameRate = 60.0. This assertion used to
        // say 30.0, which is what this module's own wrong copy of the constant
        // produced.
        assert_eq!(param.fMaxFrameRate, 60.0);
        assert_eq!(param.iSpatialLayerNum, 1);
        assert_eq!(param.iTemporalLayerNum, 1);
        assert_eq!(param.iBitsVaryPercentage, 10);
    }

    #[test]
    fn test_temporal_settings() {
        let mut param = SWelsSvcCodingParam::new();
        param.iTemporalLayerNum = 3;
        param.uiGopSize = 4;
        param.sDependencyLayers[0].fInputFrameRate = 30.0;
        param.sDependencyLayers[0].fOutputFrameRate = 30.0;
        param.fMaxFrameRate = 30.0;
        let ret = param.DetermineTemporalSettings();
        assert_eq!(ret, ENC_RETURN_SUCCESS);
        assert_eq!(param.sDependencyLayers[0].iHighestTemporalId, 2);
    }

    /// T6.H11: the allocate/free pair is one constructor, so what is left to test is
    /// that `FillDefault` ran — which is what the old test actually checked, either
    /// side of a `WelsFree` that is now the `Box`'s own.
    #[test]
    fn new_coding_param_is_filled() {
        let p = NewCodingParam();
        assert_eq!(p.uiGopSize, 1);
        assert_eq!(p.iNumRefFrame, AUTO_REF_PIC_COUNT);
    }
}
