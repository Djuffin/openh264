#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables
)]

#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! H.264 / AVC and SVC Slice Header and Control Architecture.
//!
//! Translated from `codec/decoder/core/inc/slice.h`.
//! Corresponds to ITU-T H.264 Section 7.3.3 and Annex G (SVC) Section G.7.3.3.4.

use std::ffi::c_void;
use crate::decoder::decoder_context::SpsRef;

// Constants matching `wels_common_defs.h` and `wels_const.h`
pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = MAX_REF_PIC_COUNT + 1; // 17
pub const MAX_MMCO_COUNT: usize = 66;

// Memory Management Control Operations (MMCO) opcodes
pub const MMCO_END: u32 = 0;
pub const MMCO_SHORT2UNUSED: u32 = 1;
pub const MMCO_LONG2UNUSED: u32 = 2;
pub const MMCO_SHORT2LONG: u32 = 3;
pub const MMCO_SET_MAX_LONG: u32 = 4;
pub const MMCO_RESET: u32 = 5;
pub const MMCO_LONG: u32 = 6;

// Reference picture list reordering command opcodes
pub const REORDER_SHORT_SUB: u16 = 0;
pub const REORDER_SHORT_ADD: u16 = 1;
pub const REORDER_LONG: u16 = 2;
pub const REORDER_END: u16 = 3;

/// H.264 slice coding types.
///
/// Matches `enum EWelsSliceType` in `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
    UNKNOWN_SLICE = 5,
}

/// Scalable extension slice types.
///
/// Matches `enum ESliceTypeExt` in `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ESliceTypeExt {
    #[default]
    EP_SLICE = 0,
    EB_SLICE = 1,
    EI_SLICE = 2,
}

/// Reference picture list indices.
///
/// Matches `enum EListIndex` in `codec/common/inc/wels_common_defs.h`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EListIndex {
    #[default]
    LIST_0 = 0,
    LIST_1 = 1,
    LIST_A = 2,
}

/// Single reference picture list reordering command syntax element.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SReorderingSyntax {
    pub uiAbsDiffPicNumMinus1: u32,
    pub uiLongTermPicNum: u16,
    pub uiReorderingOfPicNumsIdc: u16,
}

/// Reference picture list reordering syntax.
///
/// Refer to ITU-T H.264 Section 7.3.3.1 and JVT-X201wcm Page 64.
/// Matches `TagRefPicListReorderSyntax` / `SRefPicListReorderSyn`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagRefPicListReorderSyntax {
    pub sReorderingSyn: [[SReorderingSyntax; MAX_REF_PIC_COUNT + 1]; LIST_A],
    pub bRefPicListReorderingFlag: [bool; LIST_A],
}

pub type SRefPicListReorderSyn = TagRefPicListReorderSyntax;

impl Default for TagRefPicListReorderSyntax {
    fn default() -> Self {
        Self {
            sReorderingSyn: [[SReorderingSyntax::default(); MAX_REF_PIC_COUNT + 1]; LIST_A],
            bRefPicListReorderingFlag: [false; LIST_A],
        }
    }
}

/// Explicit prediction weights and offsets for a single reference picture list.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPredWeightList {
    pub iLumaWeight: [i32; MAX_REF_PIC_COUNT],
    pub iLumaOffset: [i32; MAX_REF_PIC_COUNT],
    pub iChromaWeight: [[i32; 2]; MAX_REF_PIC_COUNT],
    pub iChromaOffset: [[i32; 2]; MAX_REF_PIC_COUNT],
    pub bLumaWeightFlag: bool,
    pub bChromaWeightFlag: bool,
}

impl Default for SPredWeightList {
    fn default() -> Self {
        Self {
            iLumaWeight: [0; MAX_REF_PIC_COUNT],
            iLumaOffset: [0; MAX_REF_PIC_COUNT],
            iChromaWeight: [[0; 2]; MAX_REF_PIC_COUNT],
            iChromaOffset: [[0; 2]; MAX_REF_PIC_COUNT],
            bLumaWeightFlag: false,
            bChromaWeightFlag: false,
        }
    }
}

/// Prediction weight table syntax.
///
/// Refer to ITU-T H.264 Section 7.3.3.2 and JVT-X201wcm Page 65.
/// Matches `TagPredWeightTabSyntax` / `SPredWeightTabSyn`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagPredWeightTabSyntax {
    pub uiLumaLog2WeightDenom: u32,
    pub uiChromaLog2WeightDenom: u32,
    pub sPredList: [SPredWeightList; LIST_A],
    pub iImplicitWeight: [[i32; MAX_REF_PIC_COUNT]; MAX_REF_PIC_COUNT],
}

pub type SPredWeightTabSyn = TagPredWeightTabSyntax;
pub type SPredWeightTable = SPredWeightTabSyn;
pub type SPredList = SPredWeightList;

impl Default for TagPredWeightTabSyntax {
    fn default() -> Self {
        Self {
            uiLumaLog2WeightDenom: 0,
            uiChromaLog2WeightDenom: 0,
            sPredList: [SPredWeightList::default(); LIST_A],
            iImplicitWeight: [[0; MAX_REF_PIC_COUNT]; MAX_REF_PIC_COUNT],
        }
    }
}

/// Single Decoded Reference Picture Marking (MMCO) command entry.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SMmcoRef {
    pub uiMmcoType: u32,
    pub iShortFrameNum: i32,
    pub iDiffOfPicNum: i32,
    pub uiLongTermPicNum: u32,
    pub iLongTermFrameIdx: i32,
    pub iMaxLongTermFrameIdx: i32,
}

/// Decoded reference picture marking syntax.
///
/// Refer to ITU-T H.264 Section 7.3.3.3 and JVT-X201wcm Page 66.
/// Matches `TagRefPicMarking` / `SRefPicMarking`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagRefPicMarking {
    pub sMmcoRef: [SMmcoRef; MAX_MMCO_COUNT],
    pub bNoOutputOfPriorPicsFlag: bool,
    pub bLongTermRefFlag: bool,
    pub bAdaptiveRefPicMarkingModeFlag: bool,
}

pub type SRefPicMarking = TagRefPicMarking;

impl Default for TagRefPicMarking {
    fn default() -> Self {
        Self {
            sMmcoRef: [SMmcoRef::default(); MAX_MMCO_COUNT],
            bNoOutputOfPriorPicsFlag: false,
            bLongTermRefFlag: false,
            bAdaptiveRefPicMarkingModeFlag: false,
        }
    }
}

/// Single Decoded Reference Base Picture Marking command entry for SVC.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SMmcoBase {
    pub uiMmcoType: u32,
    pub iShortFrameNum: i32,
    pub uiDiffOfPicNums: u32,
    pub uiLongTermPicNum: u32,
}

/// Decoded reference base picture marking syntax.
///
/// Refer to ITU-T H.264 Annex G Section G.7.3.3.4 and JVT-X201wcm Page 396.
/// Matches `TagRefBasePicMarkingSyn` / `SRefBasePicMarking`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagRefBasePicMarkingSyn {
    pub mmco_base: [SMmcoBase; MAX_MMCO_COUNT],
    pub bAdaptiveRefBasePicMarkingModeFlag: bool,
}

pub type SRefBasePicMarking = TagRefBasePicMarkingSyn;

impl Default for TagRefBasePicMarkingSyn {
    fn default() -> Self {
        Self {
            mmco_base: [SMmcoBase::default(); MAX_MMCO_COUNT],
            bAdaptiveRefBasePicMarkingModeFlag: false,
        }
    }
}

/// Header of slice syntax elements.
///
/// Refer to ITU-T H.264 Section 7.3.3 and JVT-X201wcm Page 63.
/// Matches `TagSliceHeaders` / `SSliceHeader`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagSliceHeaders {
    // slice header syntax and generated
    pub iFirstMbInSlice: i32,
    pub iFrameNum: i32,
    pub iPicOrderCntLsb: i32,
    pub iDeltaPicOrderCntBottom: i32,
    pub iDeltaPicOrderCnt: [i32; 2],
    pub iRedundantPicCnt: i32,
    pub iDirectSpatialMvPredFlag: i32,
    pub uiRefCount: [i32; LIST_A],
    pub iSliceQpDelta: i32,
    pub iSliceQp: i32,
    pub iSliceQsDelta: i32,
    pub uiDisableDeblockingFilterIdc: u32,
    pub iSliceAlphaC0Offset: i32,
    pub iSliceBetaOffset: i32,
    pub iSliceGroupChangeCycle: i32,

    /// The active parameter sets as ids, not aliases; `sps_of`/`pps_of` rebuild the
    /// address at each use. `None` is the null they hold before
    /// `ParseSliceHeaderSyntaxs` fills them.
    pub sps_ref: Option<SpsRef>,
    pub pps_id: Option<i32>,
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub bIdrFlag: bool,

    // got from other layer for efficiency if possible
    pub pRefPicListReordering: SRefPicListReorderSyn,
    pub sPredWeightTable: SPredWeightTabSyn,
    pub iCabacInitIdc: i32,
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub sRefMarking: SRefPicMarking,

    pub uiIdrPicId: u16,
    pub eSliceType: EWelsSliceType,
    pub bNumRefIdxActiveOverrideFlag: bool,
    pub bFieldPicFlag: bool,
    pub bBottomFiledFlag: bool,
    pub uiPadding1Byte: u8,
    pub bSpForSwitchFlag: bool,
    pub iPadding2Bytes: i16,
}

pub type SSliceHeader = TagSliceHeaders;

impl Default for TagSliceHeaders {
    fn default() -> Self {
        Self {
            iFirstMbInSlice: 0,
            iFrameNum: 0,
            iPicOrderCntLsb: 0,
            iDeltaPicOrderCntBottom: 0,
            iDeltaPicOrderCnt: [0; 2],
            iRedundantPicCnt: 0,
            iDirectSpatialMvPredFlag: 0,
            uiRefCount: [0; LIST_A],
            iSliceQpDelta: 0,
            iSliceQp: 0,
            iSliceQsDelta: 0,
            uiDisableDeblockingFilterIdc: 0,
            iSliceAlphaC0Offset: 0,
            iSliceBetaOffset: 0,
            iSliceGroupChangeCycle: 0,
            sps_ref: None,
            pps_id: None,
            iSpsId: 0,
            iPpsId: 0,
            bIdrFlag: false,
            pRefPicListReordering: SRefPicListReorderSyn::default(),
            sPredWeightTable: SPredWeightTabSyn::default(),
            iCabacInitIdc: 0,
            iMbWidth: 0,
            iMbHeight: 0,
            sRefMarking: SRefPicMarking::default(),
            uiIdrPicId: 0,
            eSliceType: EWelsSliceType::P_SLICE,
            bNumRefIdxActiveOverrideFlag: false,
            bFieldPicFlag: false,
            bBottomFiledFlag: false,
            uiPadding1Byte: 0,
            bSpForSwitchFlag: false,
            iPadding2Bytes: 0,
        }
    }
}

/// Slice header in scalable extension syntax.
///
/// Refer to ITU-T H.264 Annex G Section G.7.3.3.4 and JVT-X201wcm Page 394.
/// Matches `TagSliceHeaderExt` / `SSliceHeaderExt`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    /// The subset SPS id, not an alias; `subset_sps_of` resolves it.
    pub subset_sps_id: Option<i32>,

    pub uiDisableInterLayerDeblockingFilterIdc: u32,
    pub iInterLayerSliceAlphaC0Offset: i32,
    pub iInterLayerSliceBetaOffset: i32,

    pub iScaledRefLayerPicWidthInSampleLuma: i32,
    pub iScaledRefLayerPicHeightInSampleLuma: i32,

    pub sRefBasePicMarking: SRefBasePicMarking,
    pub bBasePredWeightTableFlag: bool,
    pub bStoreRefBasePicFlag: bool,
    pub bConstrainedIntraResamplingFlag: bool,
    pub bSliceSkipFlag: bool,

    pub bAdaptiveBaseModeFlag: bool,
    pub bDefaultBaseModeFlag: bool,
    pub bAdaptiveMotionPredFlag: bool,
    pub bDefaultMotionPredFlag: bool,
    pub bAdaptiveResidualPredFlag: bool,
    pub bDefaultResidualPredFlag: bool,
    pub bTCoeffLevelPredFlag: bool,
    pub uiRefLayerChromaPhaseXPlus1Flag: u8,

    pub uiRefLayerChromaPhaseYPlus1: u8,
    pub uiRefLayerDqId: u8,
    pub uiScanIdxStart: u8,
    pub uiScanIdxEnd: u8,
}

pub type SSliceHeaderExt = TagSliceHeaderExt;

impl Default for TagSliceHeaderExt {
    fn default() -> Self {
        Self {
            sSliceHeader: SSliceHeader::default(),
            subset_sps_id: None,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            iScaledRefLayerPicWidthInSampleLuma: 0,
            iScaledRefLayerPicHeightInSampleLuma: 0,
            sRefBasePicMarking: SRefBasePicMarking::default(),
            bBasePredWeightTableFlag: false,
            bStoreRefBasePicFlag: false,
            bConstrainedIntraResamplingFlag: false,
            bSliceSkipFlag: false,
            bAdaptiveBaseModeFlag: false,
            bDefaultBaseModeFlag: false,
            bAdaptiveMotionPredFlag: false,
            bDefaultMotionPredFlag: false,
            bAdaptiveResidualPredFlag: false,
            bDefaultResidualPredFlag: false,
            bTCoeffLevelPredFlag: false,
            uiRefLayerChromaPhaseXPlus1Flag: 0,
            uiRefLayerChromaPhaseYPlus1: 0,
            uiRefLayerDqId: 0,
            uiScanIdxStart: 0,
            uiScanIdxEnd: 0,
        }
    }
}

/// Active slice context and state tracking.
///
/// Matches `TagSlice` / `SSlice`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagSlice {
    // slice_header
    pub sSliceHeaderExt: SSliceHeaderExt,

    // for Macroblock coding within slice
    pub iLastMbQp: i32,

    // slice_data
    pub iMbSkipRun: i32,
    pub iTotalMbInCurSlice: i32,

    // misc use
    pub bSliceHeaderExtFlag: bool,

    // from lower layer: slice header
    pub eSliceType: u8,
    pub uiPadding: [u8; 2],
    pub iLastDeltaQp: i32,
    pub iMvScale: [[i16; MAX_DPB_COUNT]; LIST_A],
}

pub type SSlice = TagSlice;

impl Default for TagSlice {
    fn default() -> Self {
        Self {
            sSliceHeaderExt: SSliceHeaderExt::default(),
            iLastMbQp: 0,
            iMbSkipRun: 0,
            iTotalMbInCurSlice: 0,
            bSliceHeaderExtFlag: false,
            eSliceType: 0,
            uiPadding: [0; 2],
            iLastDeltaQp: 0,
            iMvScale: [[0; MAX_DPB_COUNT]; LIST_A],
        }
    }
}

// ---------------------------------------------------------------------------
// Algorithmic Math & Syntax Helper Functions
// ---------------------------------------------------------------------------

/// Derives the baseline slice quantization parameter and clamps it to [0, 51].
#[inline(always)]
pub fn calc_slice_qp(pic_init_qp: i32, slice_qp_delta: i32) -> i32 {
    let qp = pic_init_qp + slice_qp_delta;
    qp.clamp(0, 51)
}

/// Updates the running macroblock quantization parameter using `mb_qp_delta`.
#[inline(always)]
pub fn update_mb_qp(prev_qp: i32, mb_qp_delta: i32) -> i32 {
    (prev_qp + mb_qp_delta + 52) % 52
}

/// Computes the Picture Order Count MSB for POC Type 0.
#[inline]
pub fn calc_poc_msb(
    pic_order_cnt_lsb: i32,
    prev_poc_msb: i32,
    prev_poc_lsb: i32,
    max_poc_lsb: i32,
) -> i32 {
    let half_max = max_poc_lsb / 2;
    if (pic_order_cnt_lsb < prev_poc_lsb) && (prev_poc_lsb - pic_order_cnt_lsb >= half_max) {
        prev_poc_msb + max_poc_lsb
    } else if (pic_order_cnt_lsb > prev_poc_lsb) && (pic_order_cnt_lsb - prev_poc_lsb > half_max) {
        prev_poc_msb - max_poc_lsb
    } else {
        prev_poc_msb
    }
}

/// Computes the implicit bi-prediction scaling factor and weight matrix entry.
///
/// Matches `CreateImplicitWeightTable` logic in `decoder_core.cpp`.
#[inline]
pub fn calc_implicit_weight(poc_curr: i32, poc_ref0: i32, poc_ref1: i32) -> i32 {
    let tb = (poc_curr - poc_ref0).clamp(-128, 127);
    let td = (poc_ref1 - poc_ref0).clamp(-128, 127);
    if td != 0 {
        let tx = (16384 + (td.abs() / 2)) / td;
        let dist_scale_factor = (tb * tx + 32) >> 8;
        if (-64..=128).contains(&dist_scale_factor) {
            return 64 - dist_scale_factor;
        }
    }
    32
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_slice_header_default() {
        let sh = SSliceHeader::default();
        assert_eq!(sh.iSliceQp, 0);
        assert_eq!(sh.eSliceType, EWelsSliceType::P_SLICE);
        assert!(!sh.bIdrFlag);
    }

    #[test]
    fn test_slice_ext_default() {
        let ext = SSliceHeaderExt::default();
        assert_eq!(ext.uiDisableInterLayerDeblockingFilterIdc, 0);
        assert!(!ext.bSliceSkipFlag);
    }

    #[test]
    fn test_slice_context_default() {
        let s = SSlice::default();
        assert_eq!(s.iLastMbQp, 0);
        assert_eq!(s.iTotalMbInCurSlice, 0);
        assert_eq!(s.iMvScale[0][0], 0);
    }

    #[test]
    fn test_calc_slice_qp() {
        assert_eq!(calc_slice_qp(26, -5), 21);
        assert_eq!(calc_slice_qp(26, 30), 51);
        assert_eq!(calc_slice_qp(10, -20), 0);
    }

    #[test]
    fn test_update_mb_qp() {
        assert_eq!(update_mb_qp(26, 2), 28);
        assert_eq!(update_mb_qp(0, -1), 51);
        assert_eq!(update_mb_qp(50, 4), 2);
    }

    #[test]
    fn test_calc_poc_msb() {
        let max_lsb = 256;
        // Normal progression
        assert_eq!(calc_poc_msb(10, 0, 8, max_lsb), 0);
        // Wrap-around forward
        assert_eq!(calc_poc_msb(2, 0, 250, max_lsb), 256);
        // Wrap-around backward
        assert_eq!(calc_poc_msb(250, 256, 2, max_lsb), 0);
    }

    #[test]
    fn test_calc_implicit_weight() {
        let w = calc_implicit_weight(2, 0, 4);
        assert_eq!(w, 32);
    }
}
