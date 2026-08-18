#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

#![deny(unsafe_code)]
// Phase 5 W7. This module holds none of the three forms the lint denies — no
// `unsafe fn`, no `unsafe` block, no unsafe trait implementation — so the lint
// is a statement of fact rather than a goal, and it is here to keep it one: the
// ratchet counts tokens (S16) and cannot see one of those arriving in a file
// that had none.
//
// The wording above is deliberate and the ratchet is why: spelling the third
// form as its two-word token made this comment itself count as an occurrence,
// and the gate went red on three files whose code had not changed at all —
// S16's floor, met from the direction of a comment written to celebrate it.
//
// Raw pointer **types** in a signature do not trip this lint; dereferencing one
// does. That is why a module can carry `*mut` fields and still be deny-clean,
// and it is the distinction the phase's exit condition 2 is written against.

//! H.264 / AVC and SVC Sequence Parameter Set (SPS), Picture Parameter Set (PPS),
//! and Video Usability Information (VUI) data structures.
//!
//! Translated from `codec/decoder/core/inc/parameter_sets.h`.

pub const MAX_SLICEGROUP_IDS: usize = 8;
pub const MAX_SPS_COUNT: usize = 32;
pub const MAX_PPS_COUNT: usize = 256;
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const SPS_MAX_NUM_REF_FRAMES_MAX: usize = 16;
pub const MAX_MB_SIZE: u32 = 1024;

/// H.264 Profile IDC definitions.
pub type ProfileIdc = u8;
pub const PRO_BASELINE: ProfileIdc = 66;
pub const PRO_MAIN: ProfileIdc = 77;
pub const PRO_EXTENDED: ProfileIdc = 88;
pub const PRO_HIGH: ProfileIdc = 100;
pub const PRO_HIGH10: ProfileIdc = 110;
pub const PRO_HIGH422: ProfileIdc = 122;
pub const PRO_HIGH444: ProfileIdc = 244;
pub const PRO_CAVLC444: ProfileIdc = 44;
pub const PRO_SCALABLE_BASELINE: ProfileIdc = 83;
pub const PRO_SCALABLE_HIGH: ProfileIdc = 86;

/// Frame cropping and picture position offset structure.
///
/// Matches `TagPosOffset` / `SPosOffset` from `codec/decoder/core/inc/wels_common_basis.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagPosOffset {
    pub iLeftOffset: i32,
    pub iTopOffset: i32,
    pub iRightOffset: i32,
    pub iBottomOffset: i32,
}

pub type SPosOffset = TagPosOffset;

/// Level limits for H.264 compliance validation.
///
/// Matches `TagLevelLimits` / `SLevelLimits` from `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagLevelLimits {
    pub uiLevelIdc: u8,
    pub uiMaxMBPS: u32,
    pub uiMaxFS: u32,
    pub uiMaxDPBMbs: u32,
    pub uiMaxBR: u32,
    pub uiMaxCPB: u32,
    pub iMinVmv: i16,
    pub iMaxVmv: i16,
    pub uiMinCR: u16,
    pub iMaxMvsPer2Mb: i16,
}

pub type SLevelLimits = TagLevelLimits;

/// Sample Aspect Ratio (SAR) entry for VUI Table E-1.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagSar {
    pub uiWidth: u32,
    pub uiHeight: u32,
}

pub type sSar = TagSar;

/// Table E-1: Meaning of sample aspect ratio indicator (aspect_ratio_idc).
pub const g_ksVuiSampleAspectRatio: [sSar; 17] = [
    sSar { uiWidth: 0, uiHeight: 0 },
    sSar { uiWidth: 1, uiHeight: 1 },
    sSar { uiWidth: 12, uiHeight: 11 },
    sSar { uiWidth: 10, uiHeight: 11 },
    sSar { uiWidth: 16, uiHeight: 11 },
    sSar { uiWidth: 40, uiHeight: 33 },
    sSar { uiWidth: 24, uiHeight: 11 },
    sSar { uiWidth: 20, uiHeight: 11 },
    sSar { uiWidth: 32, uiHeight: 11 },
    sSar { uiWidth: 80, uiHeight: 33 },
    sSar { uiWidth: 18, uiHeight: 11 },
    sSar { uiWidth: 15, uiHeight: 11 },
    sSar { uiWidth: 64, uiHeight: 33 },
    sSar { uiWidth: 160, uiHeight: 99 },
    sSar { uiWidth: 4, uiHeight: 3 },
    sSar { uiWidth: 3, uiHeight: 2 },
    sSar { uiWidth: 2, uiHeight: 1 },
];

/// VUI syntax in Sequence Parameter Set, refer to Annex E.1 in ITU-T H.264 Rec.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TagVui {
    pub bAspectRatioInfoPresentFlag: bool,
    pub uiAspectRatioIdc: u32,
    pub uiSarWidth: u32,
    pub uiSarHeight: u32,
    pub bOverscanInfoPresentFlag: bool,
    pub bOverscanAppropriateFlag: bool,
    pub bVideoSignalTypePresentFlag: bool,
    pub uiVideoFormat: u8,
    pub bVideoFullRangeFlag: bool,
    pub bColourDescripPresentFlag: bool,
    pub uiColourPrimaries: u8,
    pub uiTransferCharacteristics: u8,
    pub uiMatrixCoeffs: u8,
    pub bChromaLocInfoPresentFlag: bool,
    pub uiChromaSampleLocTypeTopField: u32,
    pub uiChromaSampleLocTypeBottomField: u32,
    pub bTimingInfoPresentFlag: bool,
    pub uiNumUnitsInTick: u32,
    pub uiTimeScale: u32,
    pub bFixedFrameRateFlag: bool,
    pub bNalHrdParamPresentFlag: bool,
    pub bVclHrdParamPresentFlag: bool,
    pub bPicStructPresentFlag: bool,
    pub bBitstreamRestrictionFlag: bool,
    pub bMotionVectorsOverPicBoundariesFlag: bool,
    pub uiMaxBytesPerPicDenom: u32,
    pub uiMaxBitsPerMbDenom: u32,
    pub uiLog2MaxMvLengthHorizontal: u32,
    pub uiLog2MaxMvLengthVertical: u32,
    pub uiMaxNumReorderFrames: u32,
    pub uiMaxDecFrameBuffering: u32,
}

pub type SVui = TagVui;
pub type PVui = *mut SVui;

impl Default for TagVui {
    fn default() -> Self {
        Self {
            bAspectRatioInfoPresentFlag: false,
            uiAspectRatioIdc: 0,
            uiSarWidth: 0,
            uiSarHeight: 0,
            bOverscanInfoPresentFlag: false,
            bOverscanAppropriateFlag: false,
            bVideoSignalTypePresentFlag: false,
            uiVideoFormat: 0,
            bVideoFullRangeFlag: false,
            bColourDescripPresentFlag: false,
            uiColourPrimaries: 0,
            uiTransferCharacteristics: 0,
            uiMatrixCoeffs: 0,
            bChromaLocInfoPresentFlag: false,
            uiChromaSampleLocTypeTopField: 0,
            uiChromaSampleLocTypeBottomField: 0,
            bTimingInfoPresentFlag: false,
            uiNumUnitsInTick: 0,
            uiTimeScale: 0,
            bFixedFrameRateFlag: false,
            bNalHrdParamPresentFlag: false,
            bVclHrdParamPresentFlag: false,
            bPicStructPresentFlag: false,
            bBitstreamRestrictionFlag: false,
            bMotionVectorsOverPicBoundariesFlag: false,
            uiMaxBytesPerPicDenom: 0,
            uiMaxBitsPerMbDenom: 0,
            uiLog2MaxMvLengthHorizontal: 0,
            uiLog2MaxMvLengthVertical: 0,
            uiMaxNumReorderFrames: 0,
            uiMaxDecFrameBuffering: 0,
        }
    }
}

/// Sequence Parameter Set (SPS) structure, refer to Section 7.3.2.1.1 in ITU-T H.264 Rec.
#[repr(C)]
#[derive(PartialEq, Debug, Copy, Clone)]
pub struct TagSps {
    pub iSpsId: i32,
    pub iMbWidth: u32,
    pub iMbHeight: u32,
    pub uiTotalMbCount: u32,

    pub uiLog2MaxFrameNum: u32,
    pub uiPocType: u32,
    /* POC type 0 */
    pub iLog2MaxPocLsb: i32,
    /* POC type 1 */
    pub iOffsetForNonRefPic: i32,

    pub iOffsetForTopToBottomField: i32,
    pub iNumRefFramesInPocCycle: i32,
    pub iOffsetForRefFrame: [i8; 256],
    pub iNumRefFrames: i32,

    pub sFrameCrop: SPosOffset,

    pub uiProfileIdc: ProfileIdc,
    pub uiLevelIdc: u8,
    pub uiChromaFormatIdc: u8,
    pub uiChromaArrayType: u8,

    pub uiBitDepthLuma: u8,
    pub uiBitDepthChroma: u8,
    /* TO BE CONTINUE: POC type 1 */
    pub bDeltaPicOrderAlwaysZeroFlag: bool,
    pub bGapsInFrameNumValueAllowedFlag: bool,

    pub bFrameMbsOnlyFlag: bool,
    pub bMbaffFlag: bool,
    pub bDirect8x8InferenceFlag: bool,
    pub bFrameCroppingFlag: bool,

    pub bVuiParamPresentFlag: bool,
    pub bConstraintSet0Flag: bool,
    pub bConstraintSet1Flag: bool,
    pub bConstraintSet2Flag: bool,
    pub bConstraintSet3Flag: bool,
    pub bSeparateColorPlaneFlag: bool,
    pub bQpPrimeYZeroTransfBypassFlag: bool,
    pub bSeqScalingMatrixPresentFlag: bool,
    pub bSeqScalingListPresentFlag: [bool; 12],
    pub iScalingList4x4: [[u8; 16]; 6],
    pub iScalingList8x8: [[u8; 64]; 6],
    pub sVui: SVui,
    pub pSLevelLimits: *const SLevelLimits,
}

pub type SSps = TagSps;
pub type PSps = *mut SSps;

impl Default for TagSps {
    fn default() -> Self {
        Self {
            iSpsId: 0,
            iMbWidth: 0,
            iMbHeight: 0,
            uiTotalMbCount: 0,
            uiLog2MaxFrameNum: 0,
            uiPocType: 0,
            iLog2MaxPocLsb: 0,
            iOffsetForNonRefPic: 0,
            iOffsetForTopToBottomField: 0,
            iNumRefFramesInPocCycle: 0,
            iOffsetForRefFrame: [0; 256],
            iNumRefFrames: 0,
            sFrameCrop: SPosOffset::default(),
            uiProfileIdc: 0,
            uiLevelIdc: 0,
            uiChromaFormatIdc: 0,
            uiChromaArrayType: 0,
            uiBitDepthLuma: 8,
            uiBitDepthChroma: 8,
            bDeltaPicOrderAlwaysZeroFlag: false,
            bGapsInFrameNumValueAllowedFlag: false,
            bFrameMbsOnlyFlag: true,
            bMbaffFlag: false,
            bDirect8x8InferenceFlag: false,
            bFrameCroppingFlag: false,
            bVuiParamPresentFlag: false,
            bConstraintSet0Flag: false,
            bConstraintSet1Flag: false,
            bConstraintSet2Flag: false,
            bConstraintSet3Flag: false,
            bSeparateColorPlaneFlag: false,
            bQpPrimeYZeroTransfBypassFlag: false,
            bSeqScalingMatrixPresentFlag: false,
            bSeqScalingListPresentFlag: [false; 12],
            iScalingList4x4: [[0; 16]; 6],
            iScalingList8x8: [[0; 64]; 6],
            sVui: SVui::default(),
            pSLevelLimits: std::ptr::null(),
        }
    }
}

/// Sequence Parameter Set SVC Extension syntax, refer to Annex G / Section G.7.3.2.1.4.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TagSpsSvcExt {
    pub sSeqScaledRefLayer: SPosOffset,

    pub uiExtendedSpatialScalability: u8,
    pub uiChromaPhaseXPlus1Flag: u8,
    pub uiChromaPhaseYPlus1: u8,
    pub uiSeqRefLayerChromaPhaseXPlus1Flag: u8,
    pub uiSeqRefLayerChromaPhaseYPlus1: u8,
    pub bInterLayerDeblockingFilterCtrlPresentFlag: bool,
    pub bSeqTCoeffLevelPredFlag: bool,
    pub bAdaptiveTCoeffLevelPredFlag: bool,
    pub bSliceHeaderRestrictionFlag: bool,
}

pub type SSpsSvcExt = TagSpsSvcExt;
pub type PSpsSvcExt = *mut SSpsSvcExt;

/// Subset Sequence Parameter Set syntax (NAL Unit Type 15).
#[repr(C)]
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub struct TagSubsetSps {
    pub sSps: SSps,
    pub sSpsSvcExt: SSpsSvcExt,
    pub bSvcVuiParamPresentFlag: bool,
    pub bAdditionalExtension2Flag: bool,
    pub bAdditionalExtension2DataFlag: bool,
}

pub type SSubsetSps = TagSubsetSps;
pub type PSubsetSps = *mut SSubsetSps;

/// Picture Parameter Set (PPS) structure, refer to Section 7.3.2.2 in ITU-T H.264 Rec.
#[repr(C)]
#[derive(PartialEq, Debug, Copy, Clone)]
pub struct TagPps {
    pub iSpsId: i32,
    pub iPpsId: i32,

    pub uiNumSliceGroups: u32,
    pub uiSliceGroupMapType: u32,
    /* slice_group_map_type = 0 */
    pub uiRunLength: [u32; MAX_SLICEGROUP_IDS],
    /* slice_group_map_type = 2 */
    pub uiTopLeft: [u32; MAX_SLICEGROUP_IDS],
    pub uiBottomRight: [u32; MAX_SLICEGROUP_IDS],
    /* slice_group_map_type = 3, 4 or 5 */
    pub uiSliceGroupChangeRate: u32,
    /* slice_group_map_type = 6 */
    pub uiPicSizeInMapUnits: u32,
    pub uiSliceGroupId: [u32; MAX_SLICEGROUP_IDS],

    pub uiNumRefIdxL0Active: u32,
    pub uiNumRefIdxL1Active: u32,

    pub iPicInitQp: i32,
    pub iPicInitQs: i32,
    pub iChromaQpIndexOffset: [i32; 2], // [0] = cb, [1] = cr

    pub bEntropyCodingModeFlag: bool,
    pub bPicOrderPresentFlag: bool,
    /* slice_group_map_type = 3, 4 or 5 */
    pub bSliceGroupChangeDirectionFlag: bool,
    pub bDeblockingFilterControlPresentFlag: bool,

    pub bConstainedIntraPredFlag: bool,
    pub bRedundantPicCntPresentFlag: bool,
    pub bWeightedPredFlag: bool,
    pub uiWeightedBipredIdc: u8,

    pub bTransform8x8ModeFlag: bool,
    pub bPicScalingMatrixPresentFlag: bool,
    pub bPicScalingListPresentFlag: [bool; 12],
    pub iScalingList4x4: [[u8; 16]; 6],
    pub iScalingList8x8: [[u8; 64]; 6],

    pub iSecondChromaQPIndexOffset: i32,
}

pub type SPps = TagPps;
pub type PPps = *mut SPps;

impl Default for TagPps {
    fn default() -> Self {
        Self {
            iSpsId: 0,
            iPpsId: 0,
            uiNumSliceGroups: 1,
            uiSliceGroupMapType: 0,
            uiRunLength: [0; MAX_SLICEGROUP_IDS],
            uiTopLeft: [0; MAX_SLICEGROUP_IDS],
            uiBottomRight: [0; MAX_SLICEGROUP_IDS],
            uiSliceGroupChangeRate: 0,
            uiPicSizeInMapUnits: 0,
            uiSliceGroupId: [0; MAX_SLICEGROUP_IDS],
            uiNumRefIdxL0Active: 1,
            uiNumRefIdxL1Active: 1,
            iPicInitQp: 26,
            iPicInitQs: 26,
            iChromaQpIndexOffset: [0; 2],
            bEntropyCodingModeFlag: false,
            bPicOrderPresentFlag: false,
            bSliceGroupChangeDirectionFlag: false,
            bDeblockingFilterControlPresentFlag: false,
            bConstainedIntraPredFlag: false,
            bRedundantPicCntPresentFlag: false,
            bWeightedPredFlag: false,
            uiWeightedBipredIdc: 0,
            bTransform8x8ModeFlag: false,
            bPicScalingMatrixPresentFlag: false,
            bPicScalingListPresentFlag: [false; 12],
            iScalingList4x4: [[0; 16]; 6],
            iScalingList8x8: [[0; 64]; 6],
            iSecondChromaQPIndexOffset: 0,
        }
    }
}

impl TagSps {
    /// Computes the picture width in pixels from `iMbWidth`.
    #[inline(always)]
    pub fn frame_width_in_pixels(&self) -> u32 {
        self.iMbWidth * 16
    }

    /// Computes the picture height in pixels from `iMbHeight`.
    #[inline(always)]
    pub fn frame_height_in_pixels(&self) -> u32 {
        self.iMbHeight * 16
    }

    /// Returns the active cropped display rectangle `(left, top, right, bottom)` in pixels.
    pub fn get_crop_rectangle(&self) -> (u32, u32, u32, u32) {
        let width = self.frame_width_in_pixels();
        let height = self.frame_height_in_pixels();
        if !self.bFrameCroppingFlag {
            return (0, 0, width, height);
        }
        let left = (self.sFrameCrop.iLeftOffset * 2) as u32;
        let top = (self.sFrameCrop.iTopOffset * 2) as u32;
        let right = width.saturating_sub((self.sFrameCrop.iRightOffset * 2) as u32);
        let bottom = height.saturating_sub((self.sFrameCrop.iBottomOffset * 2) as u32);
        (left, top, right, bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vui_defaults() {
        let vui = SVui::default();
        assert!(!vui.bAspectRatioInfoPresentFlag);
        assert_eq!(vui.uiAspectRatioIdc, 0);
        assert_eq!(vui.uiSarWidth, 0);
        assert_eq!(vui.uiSarHeight, 0);
    }

    #[test]
    fn test_sps_defaults_and_geometry() {
        let mut sps = SSps::default();
        sps.iMbWidth = 20; // 320 px
        sps.iMbHeight = 15; // 240 px
        assert_eq!(sps.frame_width_in_pixels(), 320);
        assert_eq!(sps.frame_height_in_pixels(), 240);

        let (l, t, r, b) = sps.get_crop_rectangle();
        assert_eq!((l, t, r, b), (0, 0, 320, 240));

        sps.bFrameCroppingFlag = true;
        sps.sFrameCrop.iLeftOffset = 2; // 4 px
        sps.sFrameCrop.iTopOffset = 4; // 8 px
        sps.sFrameCrop.iRightOffset = 2; // 4 px
        sps.sFrameCrop.iBottomOffset = 4; // 8 px
        let (cl, ct, cr, cb) = sps.get_crop_rectangle();
        assert_eq!((cl, ct, cr, cb), (4, 8, 316, 232));
    }

    #[test]
    fn test_pps_defaults() {
        let pps = SPps::default();
        assert_eq!(pps.uiNumSliceGroups, 1);
        assert_eq!(pps.iPicInitQp, 26);
        assert_eq!(pps.iPicInitQs, 26);
        assert_eq!(pps.uiNumRefIdxL0Active, 1);
        assert_eq!(pps.uiNumRefIdxL1Active, 1);
    }

    #[test]
    fn test_sar_table() {
        assert_eq!(g_ksVuiSampleAspectRatio[1].uiWidth, 1);
        assert_eq!(g_ksVuiSampleAspectRatio[1].uiHeight, 1);
        assert_eq!(g_ksVuiSampleAspectRatio[2].uiWidth, 12);
        assert_eq!(g_ksVuiSampleAspectRatio[2].uiHeight, 11);
    }
}
