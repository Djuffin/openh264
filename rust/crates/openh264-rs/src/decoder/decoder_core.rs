/*!
 * \copy
 *     Copyright (c)  2013, Cisco Systems
 *     All rights reserved.
 *
 *     Redistribution and use in source and binary forms, with or without
 *     modification, are permitted provided that the following conditions
 *     are met:
 *
 *        * Redistributions of source code must retain the above copyright
 *          notice, this list of conditions and the following disclaimer.
 *
 *        * Redistributions in binary form must reproduce the above copyright
 *          notice, this list of conditions and the following disclaimer in
 *          the documentation and/or other materials provided with the
 *          distribution.
 *
 *     THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 *     "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 *     LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 *     FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 *     COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 *     INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 *     BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 *     LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 *     CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 *     LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 *     ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 *     POSSIBILITY OF SUCH DAMAGE.
 *
 *      decoder_core.rs: Wels decoder framework core implementation in Rust
 */

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe,
    unused_mut
)]

use std::ffi::{c_char, c_void};
use crate::common::memory_align::CMemoryAlign;

// Constants
pub const MIN_ACCESS_UNIT_CAPACITY: usize = 262144;
pub const MAX_ACCESS_UNIT_CAPACITY: usize = 4194304;
pub const MAX_BUFFERED_NUM: usize = 8;
pub const MAX_NAL_UNIT_NUM_IN_AU: usize = 1024;
pub const MAX_NAL_UNITS_IN_LAYER: usize = 128;
pub const MAX_MB_SIZE: i32 = 36864;
pub const LAYER_NUM_EXCHANGEABLE: usize = 4;
pub const MAX_REF_PIC_COUNT: usize = 16;
pub const MAX_DPB_COUNT: usize = 17;
pub const MB_BLOCK4x4_NUM: usize = 16;
pub const MB_COEFF_LIST_SIZE: usize = 384;
pub const MB_PARTITION_SIZE: usize = 4;
pub const MAX_MMCO_COUNT: usize = 66;
pub const MAX_PPS_COUNT: usize = 256;
pub const MAX_SPS_COUNT: usize = 32;
pub const MAX_LAYER_NUM: usize = 8;
pub const MAX_SLICEGROUP_IDS: usize = 8;
pub const BASE_QUALITY_ID: u8 = 0;
pub const MV_A: usize = 2;

pub const SLICE_HEADER_IDR_PIC_ID_MAX: u32 = 65535;
pub const SLICE_HEADER_REDUNDANT_PIC_CNT_MAX: u32 = 127;
pub const SLICE_HEADER_ALPHAC0_BETA_OFFSET_MIN: i32 = -12;
pub const SLICE_HEADER_ALPHAC0_BETA_OFFSET_MAX: i32 = 12;
pub const SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MIN: i32 = -12;
pub const SLICE_HEADER_INTER_LAYER_ALPHAC0_BETA_OFFSET_MAX: i32 = 12;
pub const MAX_NUM_REF_IDX_L0_ACTIVE_MINUS1: u32 = 15;
pub const MAX_NUM_REF_IDX_L1_ACTIVE_MINUS1: u32 = 15;
pub const SLICE_HEADER_CABAC_INIT_IDC_MAX: u32 = 2;

pub const LIST_0: usize = 0;
pub const LIST_1: usize = 1;
pub const LIST_A: usize = 2;

// Macroblock Types
pub const MB_TYPE_INTRA4x4: u32 = 0x00000001;
pub const MB_TYPE_16x16: u32 = 0x00000002;
pub const MB_TYPE_16x8: u32 = 0x00000004;
pub const MB_TYPE_8x16: u32 = 0x00000008;
pub const MB_TYPE_8x8: u32 = 0x00000010;
pub const MB_TYPE_8x8_REF0: u32 = 0x00000020;
pub const MB_TYPE_SKIP: u32 = 0x00000040;

// Error Codes
pub const ERR_NONE: i32 = 0;
pub const ERR_INFO_INVALID_PTR: i32 = 1;
pub const ERR_INFO_OUT_OF_MEMORY: i32 = 2;
pub const ERR_INFO_INVALID_ACCESS: i32 = 3;
pub const ERR_INFO_INVALID_PARAM: i32 = 4;
pub const ERR_INFO_MB_NUM_INADEQUATE: i32 = 5;
pub const ERR_INFO_PARSEONLY_PENDING: i32 = 6;
pub const ERR_INFO_PARSEONLY_ERROR: i32 = 7;
pub const ERR_INFO_REFERENCE_PIC_LOST: i32 = 8;
pub const ERR_INFO_DUPLICATE_FRAME_NUM: i32 = 9;
pub const ERR_LEVEL_SLICE_HEADER: i32 = 0x0001;
pub const ERR_INFO_INVALID_FIRST_MB_IN_SLICE: i32 = 10;
pub const ERR_INFO_INVALID_SLICE_TYPE: i32 = 11;
pub const ERR_INFO_PPS_ID_OVERFLOW: i32 = 12;
pub const ERR_INFO_INVALID_PPS_ID: i32 = 13;
pub const ERR_INFO_NO_PARAM_SETS: i32 = 14;
pub const ERR_INFO_INVALID_SPS_ID: i32 = 15;
pub const ERR_INFO_UNSUPPORTED_MBAFF: i32 = 16;
pub const ERR_INFO_INVALID_FRAME_NUM: i32 = 17;
pub const ERR_INFO_INVALID_IDR_PIC_ID: i32 = 18;
pub const ERR_INFO_INVALID_REDUNDANT_PIC_CNT: i32 = 19;
pub const ERR_INFO_INVALID_NUM_REF_IDX_L0_ACTIVE_MINUS1: i32 = 20;
pub const ERR_INFO_INVALID_NUM_REF_IDX_L1_ACTIVE_MINUS1: i32 = 21;
pub const ERR_INFO_REF_COUNT_OVERFLOW: i32 = 22;
pub const ERR_INFO_INVALID_REF_REORDERING: i32 = 23;
pub const ERR_INFO_INVALID_REF_MARKING: i32 = 24;
pub const ERR_INFO_INVALID_CABAC_INIT_IDC: i32 = 25;
pub const ERR_INFO_INVALID_QP: i32 = 26;
pub const ERR_INFO_UNSUPPORTED_SPSI: i32 = 27;
pub const ERR_INFO_INVALID_DBLOCKING_IDC: i32 = 28;
pub const ERR_INFO_INVALID_SLICE_ALPHA_C0_OFFSET_DIV2: i32 = 29;
pub const ERR_INFO_INVALID_SLICE_BETA_OFFSET_DIV2: i32 = 30;
pub const ERR_INFO_UNSUPPORTED_ILP: i32 = 31;
pub const ERR_INFO_UNSUPPORTED_MGS: i32 = 32;
pub const ERR_INFO_UNSUPPORTED_SLICESKIP: i32 = 33;
pub const ERR_INFO_FMO_INIT_FAIL: i32 = 34;
pub const ERR_INFO_INVALID_LUMA_LOG2_WEIGHT_DENOM: i32 = 35;
pub const ERR_INFO_INVALID_CHROMA_LOG2_WEIGHT_DENOM: i32 = 36;
pub const ERR_INFO_INVALID_LUMA_WEIGHT: i32 = 37;
pub const ERR_INFO_INVALID_LUMA_OFFSET: i32 = 38;
pub const ERR_INFO_INVALID_CHROMA_WEIGHT: i32 = 39;
pub const ERR_INFO_INVALID_CHROMA_OFFSET: i32 = 40;

// Bitmask Error Status Flags
pub const dsErrorFree: i32 = 0x00;
pub const dsFramePending: i32 = 0x01;
pub const dsRefLost: i32 = 0x02;
pub const dsBitstreamError: i32 = 0x04;
pub const dsDepLayerLost: i32 = 0x08;
pub const dsNoParamSets: i32 = 0x10;
pub const dsDataErrorConcealed: i32 = 0x20;
pub const dsRefListNullPtrs: i32 = 0x40;
pub const dsOutOfMemory: i32 = 0x80;

// MMCO Types
pub const MMCO_END: u32 = 0;
pub const MMCO_SHORT2UNUSED: u32 = 1;
pub const MMCO_LONG2UNUSED: u32 = 2;
pub const MMCO_SHORT2LONG: u32 = 3;
pub const MMCO_SET_MAX_LONG: u32 = 4;
pub const MMCO_RESET: u32 = 5;
pub const MMCO_LONG: u32 = 6;

// Overwrite Flags
pub const OVERWRITE_NONE: i32 = 0;
pub const OVERWRITE_PPS: i32 = 1;
pub const OVERWRITE_SPS: i32 = 2;
pub const OVERWRITE_SUBSETSPS: i32 = 4;

// Error Concealment IDCs
pub const ERROR_CON_DISABLE: i32 = 0;
pub const ERROR_CON_FRAME_COPY: i32 = 1;
pub const ERROR_CON_SLICE_COPY: i32 = 2;
pub const ERROR_CON_SLICE_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 3;
pub const ERROR_CON_SLICE_MV_COPY_CROSS_IDR_FREEZE_RES_CHANGE: i32 = 4;

// Logging Levels
pub const WELS_LOG_ERROR: i32 = 1;
pub const WELS_LOG_WARNING: i32 = 2;
pub const WELS_LOG_INFO: i32 = 3;
pub const WELS_LOG_DEBUG: i32 = 4;

pub const videoFormatI420: i32 = 23;

#[inline]
pub fn GENERATE_ERROR_NO(level: i32, info: i32) -> i32 {
    (level << 16) | info
}

#[inline]
pub fn WELS_MAX<T: PartialOrd>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

#[inline]
pub fn WELS_MIN<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

#[inline]
pub fn WELS_CLIP3(x: i32, min_val: i32, max_val: i32) -> i32 {
    if x < min_val {
        min_val
    } else if x > max_val {
        max_val
    } else {
        x
    }
}

#[inline]
pub fn WELS_ABS(x: i32) -> i32 {
    x.abs()
}

#[inline]
pub fn IS_VCL_NAL(eNalType: EWelsNalUnitType, _unused: i32) -> bool {
    matches!(
        eNalType,
        EWelsNalUnitType::NAL_UNIT_CODED_SLICE
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPA
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPB
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_DPC
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR
            | EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT
    )
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsSliceType {
    #[default]
    P_SLICE = 0,
    B_SLICE = 1,
    I_SLICE = 2,
    SP_SLICE = 3,
    SI_SLICE = 4,
}

pub const P_SLICE: EWelsSliceType = EWelsSliceType::P_SLICE;
pub const B_SLICE: EWelsSliceType = EWelsSliceType::B_SLICE;
pub const I_SLICE: EWelsSliceType = EWelsSliceType::I_SLICE;
pub const SP_SLICE: EWelsSliceType = EWelsSliceType::SP_SLICE;
pub const SI_SLICE: EWelsSliceType = EWelsSliceType::SI_SLICE;

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EWelsNalUnitType {
    #[default]
    NAL_UNIT_UNSPEC_0 = 0,
    NAL_UNIT_CODED_SLICE = 1,
    NAL_UNIT_CODED_SLICE_DPA = 2,
    NAL_UNIT_CODED_SLICE_DPB = 3,
    NAL_UNIT_CODED_SLICE_DPC = 4,
    NAL_UNIT_CODED_SLICE_IDR = 5,
    NAL_UNIT_SEI = 6,
    NAL_UNIT_SPS = 7,
    NAL_UNIT_PPS = 8,
    NAL_UNIT_AU_DELIMITER = 9,
    NAL_UNIT_END_OF_SEQ = 10,
    NAL_UNIT_END_OF_STR = 11,
    NAL_UNIT_FILER_DATA = 12,
    NAL_UNIT_SPS_EXT = 13,
    NAL_UNIT_PREFIX = 14,
    NAL_UNIT_SUBSET_SPS = 15,
    NAL_UNIT_RESV_16 = 16,
    NAL_UNIT_RESV_17 = 17,
    NAL_UNIT_RESV_18 = 18,
    NAL_UNIT_AUX_CODED_SLICE = 19,
    NAL_UNIT_CODED_SLICE_EXT = 20,
    NAL_UNIT_RESV_21 = 21,
    NAL_UNIT_RESV_22 = 22,
    NAL_UNIT_RESV_23 = 23,
    NAL_UNIT_UNSPEC_24 = 24,
}

pub const NAL_UNIT_CODED_SLICE: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE;
pub const NAL_UNIT_CODED_SLICE_IDR: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR;
pub const NAL_UNIT_SEI: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_SEI;
pub const NAL_UNIT_SPS: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_SPS;
pub const NAL_UNIT_PPS: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_PPS;
pub const NAL_UNIT_AU_DELIMITER: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_AU_DELIMITER;
pub const NAL_UNIT_PREFIX: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_PREFIX;
pub const NAL_UNIT_SUBSET_SPS: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_SUBSET_SPS;
pub const NAL_UNIT_CODED_SLICE_EXT: EWelsNalUnitType = EWelsNalUnitType::NAL_UNIT_CODED_SLICE_EXT;

// Data Structures Matching C/C++ Layout

#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SPosOffset {
    pub iLeftOffset: i32,
    pub iTopOffset: i32,
    pub iRightOffset: i32,
    pub iBottomOffset: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDataBuffer {
    pub pHead: *mut u8,
    pub pEnd: *mut u8,
    pub pStartPos: *mut u8,
    pub pCurPos: *mut u8,
}

impl Default for SDataBuffer {
    fn default() -> Self {
        Self {
            pHead: std::ptr::null_mut(),
            pEnd: std::ptr::null_mut(),
            pStartPos: std::ptr::null_mut(),
            pCurPos: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SParserBsInfo {
    pub pDstBuff: *mut u8,
    pub iNalNum: i32,
    pub pNalLenInByte: *mut i32,
    pub uiOutBsTimeStamp: u64,
    pub iSpsWidthInPixel: i32,
    pub iSpsHeightInPixel: i32,
}

impl Default for SParserBsInfo {
    fn default() -> Self {
        Self {
            pDstBuff: std::ptr::null_mut(),
            iNalNum: 0,
            pNalLenInByte: std::ptr::null_mut(),
            uiOutBsTimeStamp: 0,
            iSpsWidthInPixel: 0,
            iSpsHeightInPixel: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSpsBsInfo {
    pub pSpsBsBuf: [u8; 256],
    pub uiSpsBsLen: i32,
}

impl Default for SSpsBsInfo {
    fn default() -> Self {
        Self {
            pSpsBsBuf: [0; 256],
            uiSpsBsLen: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPpsBsInfo {
    pub pPpsBsBuf: [u8; 256],
    pub uiPpsBsLen: i32,
}

impl Default for SPpsBsInfo {
    fn default() -> Self {
        Self {
            pPpsBsBuf: [0; 256],
            uiPpsBsLen: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVui {
    pub bAspectRatioInfoPresentFlag: bool,
    pub uiSarWidth: u32,
    pub uiSarHeight: u32,
    pub bOverscanInfoPresentFlag: bool,
    pub bOverscanAppropriateFlag: bool,
    pub bVideoSignalTypePresentFlag: bool,
    pub uiVideoFormat: u8,
    pub bVideoFullRangeFlag: bool,
    pub bColourDescriptionPresentFlag: bool,
    pub uiColourPrimaries: u8,
    pub uiTransferCharacteristics: u8,
    pub uiMatrixCoefficients: u8,
    pub bChromaLocInfoPresentFlag: bool,
    pub uiChromaSampleLocTypeTopField: u8,
    pub uiChromaSampleLocTypeBottomField: u8,
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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLevelLimits {
    pub uiMaxMBPS: u32,
    pub uiMaxFS: u32,
    pub uiMaxDPBMbs: u32,
    pub uiMaxBR: u32,
    pub uiMaxCPB: u32,
    pub iMinVmv: i16,
    pub iMaxVmv: i16,
    pub uiMinCR: u32,
    pub uiMaxMvsPer2Mb: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSps {
    pub iSpsId: i32,
    pub iMbWidth: u32,
    pub iMbHeight: u32,
    pub uiTotalMbCount: u32,
    pub uiLog2MaxFrameNum: u32,
    pub uiPocType: u32,
    pub iLog2MaxPocLsb: i32,
    pub iOffsetForNonRefPic: i32,
    pub iOffsetForTopToBottomField: i32,
    pub iNumRefFramesInPocCycle: i32,
    pub iOffsetForRefFrame: [i8; 256],
    pub iNumRefFrames: i32,
    pub sFrameCrop: SPosOffset,
    pub uiProfileIdc: u32,
    pub uiLevelIdc: u8,
    pub uiChromaFormatIdc: u8,
    pub uiChromaArrayType: u8,
    pub uiBitDepthLuma: u8,
    pub uiBitDepthChroma: u8,
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

impl Default for SSps {
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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpsSvcExt {
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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSubsetSps {
    pub sSps: SSps,
    pub sSpsSvcExt: SSpsSvcExt,
    pub bSvcVuiParamPresentFlag: bool,
    pub bAdditionalExtension2Flag: bool,
    pub bAdditionalExtension2DataFlag: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPps {
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub uiNumSliceGroups: u32,
    pub uiSliceGroupMapType: u32,
    pub uiRunLength: [u32; MAX_SLICEGROUP_IDS],
    pub uiTopLeft: [u32; MAX_SLICEGROUP_IDS],
    pub uiBottomRight: [u32; MAX_SLICEGROUP_IDS],
    pub uiSliceGroupChangeRate: u32,
    pub uiPicSizeInMapUnits: u32,
    pub uiSliceGroupId: [u32; MAX_SLICEGROUP_IDS],
    pub uiNumRefIdxL0Active: u32,
    pub uiNumRefIdxL1Active: u32,
    pub iPicInitQp: i32,
    pub iPicInitQs: i32,
    pub iChromaQpIndexOffset: [i32; 2],
    pub bEntropyCodingModeFlag: bool,
    pub bPicOrderPresentFlag: bool,
    pub bSliceGroupChangeDirectionFlag: bool,
    pub bDeblockingFilterControlPresentFlag: bool,
    pub bConstainedIntraPredFlag: bool,
    pub bRedundantPicCntPresentFlag: bool,
    pub bWeightedPredFlag: bool,
    pub uiWeightedBipredIdc: u32,
}

impl Default for SPps {
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
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSpsPpsCtx {
    pub sSpsBuffer: [SSps; MAX_SPS_COUNT + 1],
    pub sSubsetSpsBuffer: [SSubsetSps; MAX_SPS_COUNT + 1],
    pub sPpsBuffer: [SPps; MAX_PPS_COUNT + 1],
    pub bSpsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bSubspsAvailFlags: [bool; MAX_SPS_COUNT],
    pub bPpsAvailFlags: [bool; MAX_PPS_COUNT],
    pub pActiveLayerSps: [*mut SSps; MAX_LAYER_NUM],
    pub iPPSLastInvalidId: i32,
    pub iPPSInvalidNum: i32,
    pub iSPSLastInvalidId: i32,
    pub iSPSInvalidNum: i32,
    pub iSubSPSLastInvalidId: i32,
    pub iSubSPSInvalidNum: i32,
    pub iOverwriteFlags: i32,
    pub bSpsExistAheadFlag: bool,
    pub bSubspsExistAheadFlag: bool,
    pub bPpsExistAheadFlag: bool,
    pub bAvcBasedFlag: bool,
    pub iSeqId: i32,
}

impl Default for SSpsPpsCtx {
    fn default() -> Self {
        Self {
            sSpsBuffer: [SSps::default(); MAX_SPS_COUNT + 1],
            sSubsetSpsBuffer: [SSubsetSps::default(); MAX_SPS_COUNT + 1],
            sPpsBuffer: [SPps::default(); MAX_PPS_COUNT + 1],
            bSpsAvailFlags: [false; MAX_SPS_COUNT],
            bSubspsAvailFlags: [false; MAX_SPS_COUNT],
            bPpsAvailFlags: [false; MAX_PPS_COUNT],
            pActiveLayerSps: [std::ptr::null_mut(); MAX_LAYER_NUM],
            iPPSLastInvalidId: -1,
            iPPSInvalidNum: 0,
            iSPSLastInvalidId: -1,
            iSPSInvalidNum: 0,
            iSubSPSLastInvalidId: -1,
            iSubSPSInvalidNum: 0,
            iOverwriteFlags: OVERWRITE_NONE,
            bSpsExistAheadFlag: false,
            bSubspsExistAheadFlag: false,
            bPpsExistAheadFlag: false,
            bAvcBasedFlag: false,
            iSeqId: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPredList {
    pub iLumaWeight: [i32; MAX_REF_PIC_COUNT],
    pub iLumaOffset: [i32; MAX_REF_PIC_COUNT],
    pub iChromaWeight: [[i32; 2]; MAX_REF_PIC_COUNT],
    pub iChromaOffset: [[i32; 2]; MAX_REF_PIC_COUNT],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPredWeightTable {
    pub uiLumaLog2WeightDenom: u32,
    pub uiChromaLog2WeightDenom: u32,
    pub sPredList: [SPredList; LIST_A],
    pub iImplicitWeight: [[i32; MAX_REF_PIC_COUNT]; MAX_REF_PIC_COUNT],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SReorderingSyn {
    pub uiReorderingOfPicNumsIdc: u32,
    pub uiAbsDiffPicNumMinus1: u32,
    pub uiLongTermPicNum: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SRefPicListReorderSyn {
    pub bRefPicListReorderingFlag: [bool; LIST_A],
    pub sReorderingSyn: [[SReorderingSyn; MAX_REF_PIC_COUNT]; LIST_A],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMmcoRef {
    pub uiMmcoType: u32,
    pub iDiffOfPicNum: i32,
    pub iShortFrameNum: i32,
    pub uiLongTermPicNum: u32,
    pub iLongTermFrameIdx: i32,
    pub iMaxLongTermFrameIdx: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefPicMarking {
    pub bNoOutputOfPriorPicsFlag: bool,
    pub bLongTermRefFlag: bool,
    pub bAdaptiveRefPicMarkingModeFlag: bool,
    pub sMmcoRef: [SMmcoRef; MAX_MMCO_COUNT],
}

impl Default for SRefPicMarking {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMmcoBase {
    pub uiMmcoType: u32,
    pub uiDiffOfPicNums: u32,
    pub iShortFrameNum: i32,
    pub uiLongTermPicNum: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefBasePicMarking {
    pub bAdaptiveRefBasePicMarkingModeFlag: bool,
    pub mmco_base: [SMmcoBase; MAX_MMCO_COUNT],
}

impl Default for SRefBasePicMarking {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeader {
    pub iFirstMbInSlice: i32,
    pub eSliceType: EWelsSliceType,
    pub iPpsId: i32,
    pub iSpsId: i32,
    pub pPps: *mut SPps,
    pub pSps: *mut SSps,
    pub bIdrFlag: bool,
    pub iFrameNum: i32,
    pub bFieldPicFlag: bool,
    pub bBottomFiledFlag: bool,
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub uiIdrPicId: u32,
    pub iPicOrderCntLsb: i32,
    pub iDeltaPicOrderCntBottom: i32,
    pub iDeltaPicOrderCnt: [i32; 2],
    pub iRedundantPicCnt: i32,
    pub iDirectSpatialMvPredFlag: u32,
    pub uiRefCount: [i32; LIST_A],
    pub bNumRefIdxActiveOverrideFlag: bool,
    pub pRefPicListReordering: SRefPicListReorderSyn,
    pub sPredWeightTable: SPredWeightTable,
    pub sRefMarking: SRefPicMarking,
    pub iCabacInitIdc: u32,
    pub iSliceQpDelta: i32,
    pub iSliceQp: i32,
    pub uiDisableDeblockingFilterIdc: u32,
    pub iSliceAlphaC0Offset: i32,
    pub iSliceBetaOffset: i32,
    pub iSliceGroupChangeCycle: i32,
}

impl Default for SSliceHeader {
    fn default() -> Self {
        Self {
            iFirstMbInSlice: 0,
            eSliceType: EWelsSliceType::P_SLICE,
            iPpsId: 0,
            iSpsId: 0,
            pPps: std::ptr::null_mut(),
            pSps: std::ptr::null_mut(),
            bIdrFlag: false,
            iFrameNum: 0,
            bFieldPicFlag: false,
            bBottomFiledFlag: false,
            iMbWidth: 0,
            iMbHeight: 0,
            uiIdrPicId: 0,
            iPicOrderCntLsb: 0,
            iDeltaPicOrderCntBottom: 0,
            iDeltaPicOrderCnt: [0; 2],
            iRedundantPicCnt: 0,
            iDirectSpatialMvPredFlag: 0,
            uiRefCount: [1, 1],
            bNumRefIdxActiveOverrideFlag: false,
            pRefPicListReordering: SRefPicListReorderSyn::default(),
            sPredWeightTable: SPredWeightTable::default(),
            sRefMarking: SRefPicMarking::default(),
            iCabacInitIdc: 0,
            iSliceQpDelta: 0,
            iSliceQp: 26,
            uiDisableDeblockingFilterIdc: 0,
            iSliceAlphaC0Offset: 0,
            iSliceBetaOffset: 0,
            iSliceGroupChangeCycle: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    pub pSubsetSps: *mut SSubsetSps,
    pub bBasePredWeightTableFlag: bool,
    pub uiRefLayerDqId: u8,
    pub uiDisableInterLayerDeblockingFilterIdc: u8,
    pub iInterLayerSliceAlphaC0Offset: i32,
    pub iInterLayerSliceBetaOffset: i32,
    pub bConstrainedIntraResamplingFlag: bool,
    pub uiRefLayerChromaPhaseXPlus1Flag: u8,
    pub uiRefLayerChromaPhaseYPlus1: u8,
    pub iScaledRefLayerPicWidthInSampleLuma: i32,
    pub iScaledRefLayerPicHeightInSampleLuma: i32,
    pub bSliceSkipFlag: bool,
    pub bAdaptiveBaseModeFlag: bool,
    pub bDefaultBaseModeFlag: bool,
    pub bAdaptiveMotionPredFlag: bool,
    pub bDefaultMotionPredFlag: bool,
    pub bAdaptiveResidualPredFlag: bool,
    pub bDefaultResidualPredFlag: bool,
    pub bTCoeffLevelPredFlag: bool,
    pub uiScanIdxStart: u32,
    pub uiScanIdxEnd: u32,
    pub bStoreRefBasePicFlag: bool,
    pub sRefBasePicMarking: SRefBasePicMarking,
}

impl Default for SSliceHeaderExt {
    fn default() -> Self {
        Self {
            sSliceHeader: SSliceHeader::default(),
            pSubsetSps: std::ptr::null_mut(),
            bBasePredWeightTableFlag: false,
            uiRefLayerDqId: 255,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            bConstrainedIntraResamplingFlag: false,
            uiRefLayerChromaPhaseXPlus1Flag: 0,
            uiRefLayerChromaPhaseYPlus1: 1,
            iScaledRefLayerPicWidthInSampleLuma: 0,
            iScaledRefLayerPicHeightInSampleLuma: 0,
            bSliceSkipFlag: false,
            bAdaptiveBaseModeFlag: false,
            bDefaultBaseModeFlag: false,
            bAdaptiveMotionPredFlag: false,
            bDefaultMotionPredFlag: false,
            bAdaptiveResidualPredFlag: false,
            bDefaultResidualPredFlag: false,
            bTCoeffLevelPredFlag: false,
            uiScanIdxStart: 0,
            uiScanIdxEnd: 15,
            bStoreRefBasePicFlag: false,
            sRefBasePicMarking: SRefBasePicMarking::default(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSlice {
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub bSliceHeaderExtFlag: bool,
    pub eSliceType: EWelsSliceType,
    pub iLastMbQp: i32,
    pub iTotalMbInCurSlice: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SNalUnitHeader {
    pub eNalUnitType: EWelsNalUnitType,
    pub uiNalRefIdc: u8,
    pub uiForbiddenZeroBit: u8,
}

impl Default for SNalUnitHeader {
    fn default() -> Self {
        Self {
            eNalUnitType: EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
            uiNalRefIdc: 0,
            uiForbiddenZeroBit: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SNalUnitHeaderExt {
    pub sNalUnitHeader: SNalUnitHeader,
    pub bIdrFlag: bool,
    pub uiPriorityId: u8,
    pub iNoInterLayerPredFlag: u8,
    pub uiDependencyId: u8,
    pub uiQualityId: u8,
    pub uiTemporalId: u8,
    pub bUseRefBasePicFlag: bool,
    pub bDiscardableFlag: bool,
    pub bOutputFlag: bool,
    pub uiReservedThree2Bits: u8,
    pub uiLayerDqId: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SBitStringAux {
    pub pStartBuf: *mut u8,
    pub pEndBuf: *mut u8,
    pub iBits: i32,
    pub pCurBuf: *mut u8,
    pub uiCurBits: u32,
}

impl Default for SBitStringAux {
    fn default() -> Self {
        Self {
            pStartBuf: std::ptr::null_mut(),
            pEndBuf: std::ptr::null_mut(),
            iBits: 0,
            pCurBuf: std::ptr::null_mut(),
            uiCurBits: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SVclNal {
    pub sSliceHeaderExt: SSliceHeaderExt,
    pub sSliceBitsRead: SBitStringAux,
    pub bSliceHeaderExtFlag: bool,
    pub iNalLength: i32,
    pub pNalPos: *mut u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SPrefixNalUnit {
    pub bStoreRefBasePicFlag: bool,
    pub sRefPicBaseMarking: SRefBasePicMarking,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union SNalData {
    pub sVclNal: SVclNal,
    pub sPrefixNal: SPrefixNalUnit,
}

impl Default for SNalData {
    fn default() -> Self {
        Self {
            sVclNal: SVclNal::default(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SNalUnit {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub sNalData: SNalData,
    pub uiTimeStamp: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SAccessUnit {
    pub pNalUnitsList: [*mut SNalUnit; MAX_NAL_UNIT_NUM_IN_AU],
    pub uiAvailUnitsNum: u32,
    pub uiActualUnitsNum: u32,
    pub uiStartPos: u32,
    pub uiEndPos: u32,
    pub bCompletedAuFlag: bool,
}

impl Default for SAccessUnit {
    fn default() -> Self {
        Self {
            pNalUnitsList: [std::ptr::null_mut(); MAX_NAL_UNIT_NUM_IN_AU],
            uiAvailUnitsNum: 0,
            uiActualUnitsNum: 0,
            uiStartPos: 0,
            uiEndPos: 0,
            bCompletedAuFlag: false,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLayerInfo {
    pub sNalHeaderExt: SNalUnitHeaderExt,
    pub sSliceInLayer: SSlice,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    pub pSubsetSps: *mut SSubsetSps,
}

impl Default for SLayerInfo {
    fn default() -> Self {
        Self {
            sNalHeaderExt: SNalUnitHeaderExt::default(),
            sSliceInLayer: SSlice::default(),
            pSps: std::ptr::null_mut(),
            pPps: std::ptr::null_mut(),
            pSubsetSps: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SPicture {
    pub pData: [*mut u8; 4],
    pub iLinesize: [i32; 4],
    pub iWidthInPixel: i32,
    pub iHeightInPixel: i32,
    pub iFrameNum: i32,
    pub iFramePoc: i32,
    pub uiTimeStamp: u64,
    pub uiDecodingTimeStamp: u64,
    pub bIsComplete: bool,
    pub bNewSeqBegin: bool,
    pub bUsedAsRef: bool,
    pub bIsLongRef: bool,
    pub bIdrFlag: bool,
    pub eSliceType: EWelsSliceType,
    pub iSpsId: i32,
    pub iPpsId: i32,
    pub iPicBuffIdx: i32,
    pub iRefCount: i32,
    pub iMbNum: i32,
    pub iMbEcedNum: i32,
    pub iMbEcedPropNum: i32,
    pub pRefPic: [[*mut SPicture; MAX_REF_PIC_COUNT]; LIST_A],
    pub pMbCorrectlyDecodedFlag: *mut bool,
    pub pMbRefConcealedFlag: *mut bool,
    pub pMbType: *mut u32,
    pub pRefIndex: [*mut i8; LIST_A],
    pub pReadyEvent: [u32; 128],
}

impl Default for SPicture {
    fn default() -> Self {
        Self {
            pData: [std::ptr::null_mut(); 4],
            iLinesize: [0; 4],
            iWidthInPixel: 0,
            iHeightInPixel: 0,
            iFrameNum: 0,
            iFramePoc: 0,
            uiTimeStamp: 0,
            uiDecodingTimeStamp: 0,
            bIsComplete: true,
            bNewSeqBegin: false,
            bUsedAsRef: false,
            bIsLongRef: false,
            bIdrFlag: false,
            eSliceType: EWelsSliceType::P_SLICE,
            iSpsId: 0,
            iPpsId: 0,
            iPicBuffIdx: 0,
            iRefCount: 0,
            iMbNum: 0,
            iMbEcedNum: 0,
            iMbEcedPropNum: 0,
            pRefPic: [[std::ptr::null_mut(); MAX_REF_PIC_COUNT]; LIST_A],
            pMbCorrectlyDecodedFlag: std::ptr::null_mut(),
            pMbRefConcealedFlag: std::ptr::null_mut(),
            pMbType: std::ptr::null_mut(),
            pRefIndex: [std::ptr::null_mut(); LIST_A],
            pReadyEvent: [0; 128],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
    pub pBitStringAux: *mut SBitStringAux,
    pub pDec: *mut SPicture,
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub iSliceIdcBackup: i32,
    pub uiPpsId: i32,
    pub uiDisableInterLayerDeblockingFilterIdc: u8,
    pub iInterLayerSliceAlphaC0Offset: i32,
    pub iInterLayerSliceBetaOffset: i32,
    pub iSliceGroupChangeCycle: i32,
    pub bStoreRefBasePicFlag: bool,
    pub bTCoeffLevelPredFlag: bool,
    pub bConstrainedIntraResamplingFlag: bool,
    pub uiRefLayerDqId: u8,
    pub uiRefLayerChromaPhaseXPlus1Flag: u8,
    pub uiRefLayerChromaPhaseYPlus1: u8,
    pub bUseWeightPredictionFlag: bool,
    pub bUseWeightedBiPredIdc: bool,
    pub pPredWeightTable: *mut SPredWeightTable,
    pub pRefPicListReordering: *mut SRefPicListReorderSyn,
    pub pRefPicMarking: *mut SRefPicMarking,
    pub pRefPicBaseMarking: *mut SRefBasePicMarking,
    pub uiLayerDqId: u8,
    pub bUseRefBasePicFlag: bool,
    pub pMbType: *mut u32,
    pub pSliceIdc: *mut i32,
    pub pMv: [*mut i16; LIST_A],
    pub pRefIndex: [*mut i8; LIST_A],
    pub pDirect: *mut i8,
    pub pNoSubMbPartSizeLessThan8x8Flag: *mut bool,
    pub pTransformSize8x8Flag: *mut bool,
    pub pLumaQp: *mut i8,
    pub pChromaQp: *mut i8,
    pub pMvd: [*mut i16; LIST_A],
    pub pCbfDc: *mut u16,
    pub pNzc: *mut i8,
    pub pNzcRs: *mut i8,
    pub pScaledTCoeff: *mut i16,
    pub pIntraPredMode: *mut i8,
    pub pIntra4x4FinalMode: *mut i8,
    pub pIntraNxNAvailFlag: *mut u8,
    pub pChromaPredMode: *mut i8,
    pub pCbp: *mut i8,
    pub pSubMbType: *mut u32,
    pub pInterPredictionDoneFlag: *mut i8,
    pub pResidualPredFlag: *mut i8,
    pub pMbCorrectlyDecodedFlag: *mut bool,
    pub pMbRefConcealedFlag: *mut bool,
}

impl Default for SDqLayer {
    fn default() -> Self {
        Self {
            sLayerInfo: SLayerInfo::default(),
            pBitStringAux: std::ptr::null_mut(),
            pDec: std::ptr::null_mut(),
            iMbWidth: 0,
            iMbHeight: 0,
            iSliceIdcBackup: 0,
            uiPpsId: 0,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            iSliceGroupChangeCycle: 0,
            bStoreRefBasePicFlag: false,
            bTCoeffLevelPredFlag: false,
            bConstrainedIntraResamplingFlag: false,
            uiRefLayerDqId: 255,
            uiRefLayerChromaPhaseXPlus1Flag: 0,
            uiRefLayerChromaPhaseYPlus1: 1,
            bUseWeightPredictionFlag: false,
            bUseWeightedBiPredIdc: false,
            pPredWeightTable: std::ptr::null_mut(),
            pRefPicListReordering: std::ptr::null_mut(),
            pRefPicMarking: std::ptr::null_mut(),
            pRefPicBaseMarking: std::ptr::null_mut(),
            uiLayerDqId: 0,
            bUseRefBasePicFlag: false,
            pMbType: std::ptr::null_mut(),
            pSliceIdc: std::ptr::null_mut(),
            pMv: [std::ptr::null_mut(); LIST_A],
            pRefIndex: [std::ptr::null_mut(); LIST_A],
            pDirect: std::ptr::null_mut(),
            pNoSubMbPartSizeLessThan8x8Flag: std::ptr::null_mut(),
            pTransformSize8x8Flag: std::ptr::null_mut(),
            pLumaQp: std::ptr::null_mut(),
            pChromaQp: std::ptr::null_mut(),
            pMvd: [std::ptr::null_mut(); LIST_A],
            pCbfDc: std::ptr::null_mut(),
            pNzc: std::ptr::null_mut(),
            pNzcRs: std::ptr::null_mut(),
            pScaledTCoeff: std::ptr::null_mut(),
            pIntraPredMode: std::ptr::null_mut(),
            pIntra4x4FinalMode: std::ptr::null_mut(),
            pIntraNxNAvailFlag: std::ptr::null_mut(),
            pChromaPredMode: std::ptr::null_mut(),
            pCbp: std::ptr::null_mut(),
            pSubMbType: std::ptr::null_mut(),
            pInterPredictionDoneFlag: std::ptr::null_mut(),
            pResidualPredFlag: std::ptr::null_mut(),
            pMbCorrectlyDecodedFlag: std::ptr::null_mut(),
            pMbRefConcealedFlag: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SMbCache {
    pub iMbWidth: i32,
    pub iMbHeight: i32,
    pub pMbType: [*mut u32; LAYER_NUM_EXCHANGEABLE],
    pub pSliceIdc: [*mut i32; LAYER_NUM_EXCHANGEABLE],
    pub pMv: [[*mut i16; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pRefIndex: [[*mut i8; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pDirect: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pNoSubMbPartSizeLessThan8x8Flag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pTransformSize8x8Flag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pLumaQp: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pChromaQp: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pMvd: [[*mut i16; LIST_A]; LAYER_NUM_EXCHANGEABLE],
    pub pCbfDc: [*mut u16; LAYER_NUM_EXCHANGEABLE],
    pub pNzc: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pNzcRs: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pScaledTCoeff: [*mut i16; LAYER_NUM_EXCHANGEABLE],
    pub pIntraPredMode: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pIntra4x4FinalMode: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pIntraNxNAvailFlag: [*mut u8; LAYER_NUM_EXCHANGEABLE],
    pub pChromaPredMode: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pCbp: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pSubMbType: [*mut u32; LAYER_NUM_EXCHANGEABLE],
    pub pInterPredictionDoneFlag: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pResidualPredFlag: [*mut i8; LAYER_NUM_EXCHANGEABLE],
    pub pMbCorrectlyDecodedFlag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
    pub pMbRefConcealedFlag: [*mut bool; LAYER_NUM_EXCHANGEABLE],
}

impl Default for SMbCache {
    fn default() -> Self {
        Self {
            iMbWidth: 0,
            iMbHeight: 0,
            pMbType: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pSliceIdc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMv: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pRefIndex: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pDirect: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNoSubMbPartSizeLessThan8x8Flag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pTransformSize8x8Flag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pLumaQp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pChromaQp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMvd: [[std::ptr::null_mut(); LIST_A]; LAYER_NUM_EXCHANGEABLE],
            pCbfDc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNzc: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pNzcRs: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pScaledTCoeff: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntraPredMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntra4x4FinalMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pIntraNxNAvailFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pChromaPredMode: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pCbp: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pSubMbType: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pInterPredictionDoneFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pResidualPredFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMbCorrectlyDecodedFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
            pMbRefConcealedFlag: [std::ptr::null_mut(); LAYER_NUM_EXCHANGEABLE],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SRefPic {
    pub pRefList: [[*mut SPicture; MAX_REF_PIC_COUNT]; LIST_A],
    pub uiRefCount: [u32; LIST_A],
}

impl Default for SRefPic {
    fn default() -> Self {
        Self {
            pRefList: [[std::ptr::null_mut(); MAX_REF_PIC_COUNT]; LIST_A],
            uiRefCount: [0; LIST_A],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSysMEMBuffer {
    pub iWidth: i32,
    pub iHeight: i32,
    pub iFormat: i32,
    pub iStride: [i32; 2],
}

impl Default for SSysMEMBuffer {
    fn default() -> Self {
        Self {
            iWidth: 0,
            iHeight: 0,
            iFormat: videoFormatI420,
            iStride: [0; 2],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union SUsrData {
    pub sSystemBuffer: SSysMEMBuffer,
}

impl Default for SUsrData {
    fn default() -> Self {
        Self {
            sSystemBuffer: SSysMEMBuffer::default(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SBufferInfo {
    pub iBufferStatus: i32,
    pub uiOutYuvTimeStamp: u64,
    pub UsrData: SUsrData,
    pub pDst: [*mut u8; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDecoderStatistics {
    pub uiWidth: u32,
    pub uiHeight: u32,
    pub fAverageFrameRate: f32,
    pub fLatestFrameRate: f32,
    pub uiBitRate: u32,
    pub uiAverageFrameQP: u32,
    pub uiInputFrameCount: u32,
    pub uiSkippedFrameCount: u32,
    pub uiResolutionChangeTimes: u32,
    pub uiIDRSentNum: u32,
    pub uiIDRLostNum: u32,
    pub uiFreezingIDRNum: u32,
    pub uiFreezingNonIDRNum: u32,
    pub iCurrentActiveSpsId: i32,
    pub iCurrentActivePpsId: i32,
    pub uiProfile: u32,
    pub uiLevel: u8,
    pub iPpsReportErrorNum: i32,
    pub iSpsReportErrorNum: i32,
    pub iSubSpsReportErrorNum: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SDecodingParam {
    pub pFileNameRestructed: *mut c_char,
    pub uiCpuLoad: u32,
    pub eEcActiveIdc: i32,
    pub bParseOnly: bool,
    pub sVideoProperty: [u8; 16],
    pub uiTargetDqLayer: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SLogContext {
    pub pOption: *mut c_void,
}

impl Default for SLogContext {
    fn default() -> Self {
        Self {
            pOption: std::ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsCabacDecEngine {
    pub uiRange: u32,
    pub uiOffset: u32,
    pub iBitsLeft: i32,
    pub pBuffCurr: *mut u8,
    pub pBuffEnd: *mut u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLastDecPicInfo {
    pub sLastNalHdrExt: SNalUnitHeaderExt,
    pub sLastSliceHeader: SSliceHeader,
    pub iPrevFrameNum: i32,
    pub iPrevPicOrderCntMsb: i32,
    pub iPrevPicOrderCntLsb: i32,
    pub bLastHasMmco5: bool,
    pub uiDecodingTimeStamp: u64,
    pub pPreviousDecodedPictureInDpb: *mut SPicture,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SFmo {
    pub pSliceGroupMap: *mut u8,
    pub iSliceGroupCount: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SExpandPicFunc {
    pub pfExpandLumaPicture: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32, i32, i32)>,
    pub pfExpandChromaPicture: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32, i32, i32)>,
}

impl Default for SExpandPicFunc {
    fn default() -> Self {
        Self {
            pfExpandLumaPicture: None,
            pfExpandChromaPicture: None,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SThreadInfo {
    pub uiThrNum: i32,
}

impl Default for SThreadInfo {
    fn default() -> Self {
        Self { uiThrNum: 0 }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsDecoderThreadCTX {
    pub pCtx: *mut SWelsDecoderContext,
    pub pDec: *mut SPicture,
    pub iPicBuffIdx: i32,
    pub sThreadInfo: SThreadInfo,
    pub sSliceDecodeStart: u32,
    pub sSliceDecodeFinish: u32,
    pub sImageReady: u32,
}

#[repr(C)]
pub struct SWelsDecoderContext {
    pub pMemAlign: *mut CMemoryAlign,
    pub sRawData: SDataBuffer,
    pub sSavedData: SDataBuffer,
    pub pParserBsInfo: *mut SParserBsInfo,
    pub pAccessUnitList: *mut SAccessUnit,
    pub pCurDqLayer: *mut SDqLayer,
    pub pDqLayersList: [*mut SDqLayer; LAYER_NUM_EXCHANGEABLE],
    pub sMb: SMbCache,
    pub pDec: *mut SPicture,
    pub pPicBuff: *mut c_void,
    pub sRefPic: SRefPic,
    pub sTmpRefPic: SRefPic,
    pub sSpsPpsCtx: SSpsPpsCtx,
    pub sFrameCrop: SPosOffset,
    pub pParam: *mut SDecodingParam,
    pub pDecoderStatistics: *mut SDecoderStatistics,
    pub sLogCtx: SLogContext,
    pub pCabacDecEngine: *mut SWelsCabacDecEngine,
    pub pSliceHeader: *mut SSliceHeader,
    pub pLastDecPicInfo: *mut SLastDecPicInfo,
    pub pSps: *mut SSps,
    pub pPps: *mut SPps,
    pub pNalCur: *mut SNalUnit,
    pub iErrorCode: i32,
    pub iTotalNumMbRec: i32,
    pub bNewSeqBegin: bool,
    pub bNextNewSeqBegin: bool,
    pub bInitialDqLayersMem: bool,
    pub uiTargetDqId: u8,
    pub bEndOfStreamFlag: bool,
    pub iMaxBsBufferSizeInByte: i32,
    pub iMaxNalNum: i32,
    pub iPicWidthReq: i32,
    pub iPicHeightReq: i32,
    pub bReferenceLostAtT0Flag: bool,
    pub bParamSetsLostFlag: bool,
    pub bPrintFrameErrorTraceFlag: bool,
    pub iIgnoredErrorInfoPacketCount: i32,
    pub bFrameFinish: bool,
    pub bFramePending: bool,
    pub bInstantDecFlag: bool,
    pub bFreezeOutput: bool,
    pub iMbEcedNum: i32,
    pub iMbNum: i32,
    pub iMbEcedPropNum: i32,
    pub iLastImgWidthInPixel: i32,
    pub iLastImgHeightInPixel: i32,
    pub eSliceType: EWelsSliceType,
    pub bUsedAsRef: bool,
    pub iFrameNum: i32,
    pub uiNalRefIdc: u8,
    pub uiDecodingTimeStamp: u64,
    pub iSeqNum: i32,
    pub pStreamSeqNum: *mut i32,
    pub bAuReadyFlag: bool,
    pub bOnlyOneLayerInCurAuFlag: bool,
    pub iCurSeqIntervalTargetDependId: u8,
    pub iCurSeqIntervalMaxPicWidth: i32,
    pub iCurSeqIntervalMaxPicHeight: i32,
    pub sSubsetSpsBsInfo: [SSpsBsInfo; MAX_SPS_COUNT],
    pub sSpsBsInfo: [SSpsBsInfo; MAX_SPS_COUNT],
    pub sPpsBsInfo: [SPpsBsInfo; MAX_PPS_COUNT],
    pub pFmo: *mut SFmo,
    pub sFmoList: [SFmo; MAX_PPS_COUNT],
    pub iActiveFmoNum: i32,
    pub sExpandPicFunc: SExpandPicFunc,
    pub iDecBlockOffsetArray: [i32; 24],
    pub bRPLRError: bool,
    pub pThreadCtx: *mut c_void,
    pub pLastThreadCtx: *mut c_void,
    pub lastReadyHeightOffset: [[i16; MAX_REF_PIC_COUNT]; LIST_A],
    pub sCurNalHead: SNalUnitHeader,
}

pub type PWelsDecoderContext = *mut SWelsDecoderContext;
pub type PNalUnit = *mut SNalUnit;
pub type PAccessUnit = *mut SAccessUnit;
pub type PDqLayer = *mut SDqLayer;
pub type PPicture = *mut SPicture;
pub type PSps = *mut SSps;
pub type PPps = *mut SPps;
pub type PSubsetSps = *mut SSubsetSps;
pub type PSliceHeader = *mut SSliceHeader;
pub type PSliceHeaderExt = *mut SSliceHeaderExt;
pub type PNalUnitHeaderExt = *mut SNalUnitHeaderExt;
pub type PBitStringAux = *mut SBitStringAux;
pub type PLayerInfo = *mut SLayerInfo;
pub type PRefPicListReorderSyn = *mut SRefPicListReorderSyn;
pub type PRefPicMarking = *mut SRefPicMarking;
pub type PRefBasePicMarking = *mut SRefBasePicMarking;
pub type PPredWeightTable = *mut SPredWeightTable;
pub type PPrefixNalUnit = *mut SPrefixNalUnit;

// Logging and Bitstream Reading Helpers

pub unsafe fn WelsLog(_pLogCtx: *mut SLogContext, _iLevel: i32, _fmt: &str) {}

#[inline]
pub unsafe fn BsGetBits(pBs: *mut SBitStringAux, n: u32, pOut: *mut u32) -> i32 {
    if pBs.is_null() || pOut.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let bs = &mut *pBs;
    if n == 0 {
        *pOut = 0;
        return ERR_NONE;
    }
    let mut val: u32 = 0;
    for _ in 0..n {
        if bs.iBits == 0 {
            if bs.pCurBuf >= bs.pEndBuf {
                return ERR_INFO_INVALID_ACCESS;
            }
            bs.uiCurBits = (*bs.pCurBuf) as u32;
            bs.pCurBuf = bs.pCurBuf.add(1);
            bs.iBits = 8;
        }
        bs.iBits -= 1;
        let bit = (bs.uiCurBits >> bs.iBits) & 1;
        val = (val << 1) | bit;
    }
    *pOut = val;
    ERR_NONE
}

#[inline]
pub unsafe fn BsGetOneBit(pBs: *mut SBitStringAux, pOut: *mut u32) -> i32 {
    BsGetBits(pBs, 1, pOut)
}

#[inline]
pub unsafe fn BsGetUe(pBs: *mut SBitStringAux, pOut: *mut u32) -> i32 {
    if pBs.is_null() || pOut.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut leading_zeros = 0;
    let mut bit = 0u32;
    loop {
        if BsGetOneBit(pBs, &mut bit) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if bit == 1 {
            break;
        }
        leading_zeros += 1;
        if leading_zeros > 31 {
            return ERR_INFO_INVALID_ACCESS;
        }
    }
    if leading_zeros == 0 {
        *pOut = 0;
        return ERR_NONE;
    }
    let mut suffix = 0u32;
    if BsGetBits(pBs, leading_zeros, &mut suffix) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    *pOut = (1u32 << leading_zeros) - 1 + suffix;
    ERR_NONE
}

#[inline]
pub unsafe fn BsGetSe(pBs: *mut SBitStringAux, pOut: *mut i32) -> i32 {
    let mut ue = 0u32;
    let ret = BsGetUe(pBs, &mut ue);
    if ret != ERR_NONE {
        return ret;
    }
    let sign = (ue & 1) == 0;
    let val = ((ue + 1) >> 1) as i32;
    *pOut = if sign { -val } else { val };
    ERR_NONE
}

// Memory Allocation Helper Wrappers

unsafe fn WelsMalloczHelper(pMa: *mut CMemoryAlign, size: usize) -> *mut u8 {
    if !pMa.is_null() {
        let tag = b"WelsMallocz\0".as_ptr() as *const c_char;
        (*pMa).WelsMallocz(size as u32, tag) as *mut u8
    } else {
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap_or(
            std::alloc::Layout::from_size_align(size, 1).unwrap()
        );
        std::alloc::alloc_zeroed(layout)
    }
}

unsafe fn WelsFreeHelper(pMa: *mut CMemoryAlign, ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }
    if !pMa.is_null() {
        let tag = b"WelsFree\0".as_ptr() as *const c_char;
        (*pMa).WelsFree(ptr as *mut c_void, tag);
    } else {
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap_or(
            std::alloc::Layout::from_size_align(size, 1).unwrap()
        );
        std::alloc::dealloc(ptr, layout);
    }
}

// External and Internal Helper Stubs

#[inline]
pub unsafe fn GetThreadCount(pCtx: PWelsDecoderContext) -> i32 {
    1
}

#[inline]
pub unsafe fn UpdateDecStatNoFreezingInfo(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn UpdateDecStat(pCtx: PWelsDecoderContext, bFlag: bool) {}

#[inline]
pub unsafe fn WelsTargetSliceConstruction(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsDecodeSlice(pCtx: PWelsDecoderContext, bFreshSlice: bool, pCurNal: PNalUnit) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsDecodeAndConstructSlice(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsInitRefList(pCtx: PWelsDecoderContext, iPoc: i32) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsInitBSliceRefList(pCtx: PWelsDecoderContext, iPoc: i32) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsReorderRefList(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsReorderRefList2(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsMarkAsRef(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn ExpandReferencingPicture(
    pData: [*mut u8; 4],
    iWidth: i32,
    iHeight: i32,
    iStride: [i32; 4],
    pfExpandLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32, i32, i32)>,
    pfExpandChroma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32, i32, i32)>,
) {}

#[inline]
pub unsafe fn GetI4LumaIChromaAddrTable(pBlockOffset: *mut i32, iStrideY: i32, iStrideUV: i32) {}

#[inline]
pub unsafe fn ComputeColocatedTemporalScaling(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn SyncPictureResolutionExt(pCtx: PWelsDecoderContext, iWidth: u32, iHeight: u32) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn WelsResetRefPic(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn PrefetchPic(pPicBuff: *mut c_void) -> PPicture {
    std::ptr::null_mut()
}

#[inline]
pub unsafe fn PrefetchLastPicForThread(pPicBuff: *mut c_void, idx: i32) -> PPicture {
    std::ptr::null_mut()
}

#[inline]
pub unsafe fn MemInitNalList(
    ppNalList: *mut PAccessUnit,
    uiCount: usize,
    pMa: *mut CMemoryAlign,
) -> i32 {
    if ppNalList.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pAu = WelsMalloczHelper(pMa, std::mem::size_of::<SAccessUnit>()) as PAccessUnit;
    if pAu.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    for i in 0..MAX_NAL_UNIT_NUM_IN_AU {
        let pNal = WelsMalloczHelper(pMa, std::mem::size_of::<SNalUnit>()) as PNalUnit;
        if pNal.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pAu).pNalUnitsList[i] = pNal;
    }
    *ppNalList = pAu;
    ERR_NONE
}

#[inline]
pub unsafe fn MemFreeNalList(ppNalList: *mut PAccessUnit, pMa: *mut CMemoryAlign) {
    if ppNalList.is_null() || (*ppNalList).is_null() {
        return;
    }
    let pAu = *ppNalList;
    for i in 0..MAX_NAL_UNIT_NUM_IN_AU {
        let pNal = (*pAu).pNalUnitsList[i];
        if !pNal.is_null() {
            WelsFreeHelper(pMa, pNal as *mut u8, std::mem::size_of::<SNalUnit>());
            (*pAu).pNalUnitsList[i] = std::ptr::null_mut();
        }
    }
    WelsFreeHelper(pMa, pAu as *mut u8, std::mem::size_of::<SAccessUnit>());
    *ppNalList = std::ptr::null_mut();
}

#[inline]
pub unsafe fn NeedErrorCon(pCtx: PWelsDecoderContext) -> bool {
    false
}

#[inline]
pub unsafe fn ImplementErrorCon(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn MarkECFrameAsRef(pCtx: PWelsDecoderContext) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn ResetActiveSPSForEachLayer(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn GetVclNalTemporalId(pCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn GetPrevFrameNum(pCtx: PWelsDecoderContext) -> i32 {
    0
}

#[inline]
pub unsafe fn CopySpsPps(pSrcCtx: PWelsDecoderContext, pDstCtx: PWelsDecoderContext) {}

#[inline]
pub unsafe fn FmoParamUpdate(
    pFmo: *mut SFmo,
    pSps: PSps,
    pPps: PPps,
    pActiveNum: *mut i32,
    pMa: *mut CMemoryAlign,
) -> i32 {
    ERR_NONE
}

#[inline]
pub unsafe fn FmoNextMb(pFmo: *mut SFmo, iMbIdx: i32) -> i32 {
    iMbIdx + 1
}

#[inline]
pub unsafe fn CheckAccessUnitBoundaryExt(
    pLastNalHdr: *mut SNalUnitHeaderExt,
    pCurNalHdr: *mut SNalUnitHeaderExt,
    pLastSh: *mut SSliceHeader,
    pCurSh: *mut SSliceHeader,
) -> bool {
    true
}

// Core Functions Implemented in `decoder_core.cpp`

pub unsafe fn DecodeFrameConstruction(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    if pCtx.is_null() || ppDst.is_null() || pDstInfo.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurDq = (*pCtx).pCurDqLayer;
    let pPic = (*pCtx).pDec;
    if pCurDq.is_null() || pPic.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let kiWidth = (*pCurDq).iMbWidth << 4;
    let kiHeight = (*pCurDq).iMbHeight << 4;
    let kiTotalNumMbInCurLayer = (*pCurDq).iMbWidth * (*pCurDq).iMbHeight;
    let mut bFrameCompleteFlag = true;

    if (*pPic).bNewSeqBegin {
        let pSps = (*pCurDq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pSps;
        if !pSps.is_null() {
            (*pCtx).sFrameCrop = (*pSps).sFrameCrop;
        }
        (*pCtx).bReferenceLostAtT0Flag = false;
        if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
            (*pCtx).bPrintFrameErrorTraceFlag = true;
            (*pCtx).iIgnoredErrorInfoPacketCount = 0;
        }
    }

    let kiActualWidth = kiWidth - ((*pCtx).sFrameCrop.iLeftOffset + (*pCtx).sFrameCrop.iRightOffset) * 2;
    let kiActualHeight = kiHeight - ((*pCtx).sFrameCrop.iTopOffset + (*pCtx).sFrameCrop.iBottomOffset) * 2;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        if !(*pCtx).pDecoderStatistics.is_null() {
            if (*(*pCtx).pDecoderStatistics).uiWidth != kiActualWidth as u32
                || (*(*pCtx).pDecoderStatistics).uiHeight != kiActualHeight as u32
            {
                (*(*pCtx).pDecoderStatistics).uiResolutionChangeTimes += 1;
                (*(*pCtx).pDecoderStatistics).uiWidth = kiActualWidth as u32;
                (*(*pCtx).pDecoderStatistics).uiHeight = kiActualHeight as u32;
            }
        }
        UpdateDecStatNoFreezingInfo(pCtx);
    }

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        let pCurAu = (*pCtx).pAccessUnitList;
        if (*pCtx).iErrorCode == dsErrorFree {
            let pParser = (*pCtx).pParserBsInfo;
            if !pParser.is_null() && !pCurAu.is_null() {
                let mut iTotalNalLen: i32 = 0;
                for i in 0..(*pParser).iNalNum {
                    if !(*pParser).pNalLenInByte.is_null() {
                        iTotalNalLen += *(*pParser).pNalLenInByte.add(i as usize);
                    }
                }
                let mut pDstBuf = (*pParser).pDstBuff.add(iTotalNalLen as usize);
                let mut iIdx = (*pCurAu).uiStartPos as i32;
                let iEndIdx = (*pCurAu).uiEndPos as i32;
                if !(*pCurAu).pNalUnitsList[iIdx as usize].is_null() {
                    (*pParser).uiOutBsTimeStamp = (*(*pCurAu).pNalUnitsList[iIdx as usize]).uiTimeStamp;
                }
                if !(*pCtx).pSps.is_null() {
                    (*pParser).iSpsWidthInPixel = ((*pCtx).pSps.as_ref().unwrap().iMbWidth as i32) * 16
                        - (((*pCtx).pSps.as_ref().unwrap().sFrameCrop.iLeftOffset
                            + (*pCtx).pSps.as_ref().unwrap().sFrameCrop.iRightOffset)
                            << 1);
                    (*pParser).iSpsHeightInPixel = ((*pCtx).pSps.as_ref().unwrap().iMbHeight as i32) * 16
                        - (((*pCtx).pSps.as_ref().unwrap().sFrameCrop.iTopOffset
                            + (*pCtx).pSps.as_ref().unwrap().sFrameCrop.iBottomOffset)
                            << 1);
                }

                while iIdx <= iEndIdx {
                    let pCurNal = (*pCurAu).pNalUnitsList[iIdx as usize];
                    if !pCurNal.is_null() {
                        let iNalLen = (*pCurNal).sNalData.sVclNal.iNalLength;
                        let pNalBs = (*pCurNal).sNalData.sVclNal.pNalPos;
                        if !(*pParser).pNalLenInByte.is_null() {
                            *(*pParser).pNalLenInByte.add((*pParser).iNalNum as usize) = iNalLen;
                            (*pParser).iNalNum += 1;
                        }
                        if !pNalBs.is_null() && !pDstBuf.is_null() && iNalLen > 0 {
                            std::ptr::copy_nonoverlapping(pNalBs, pDstBuf, iNalLen as usize);
                            pDstBuf = pDstBuf.add(iNalLen as usize);
                        }
                    }
                    iIdx += 1;
                }

                if (*pCtx).iTotalNumMbRec == kiTotalNumMbInCurLayer {
                    (*pCtx).iTotalNumMbRec = 0;
                    (*pCtx).bFramePending = false;
                    (*pCtx).bFrameFinish = true;
                } else if (*pCtx).iTotalNumMbRec != 0 {
                    (*pCtx).bFramePending = true;
                    (*(*pCtx).pDec).bIsComplete = false;
                    (*pCtx).bFrameFinish = false;
                    (*pCtx).iErrorCode |= dsFramePending;
                    return ERR_INFO_PARSEONLY_PENDING;
                }
            }
        } else {
            let pParser = (*pCtx).pParserBsInfo;
            if !pParser.is_null() {
                (*pParser).uiOutBsTimeStamp = 0;
                (*pParser).iNalNum = 0;
                (*pParser).iSpsWidthInPixel = 0;
                (*pParser).iSpsHeightInPixel = 0;
            }
            return ERR_INFO_PARSEONLY_ERROR;
        }
        return ERR_NONE;
    }

    if (*pCtx).iTotalNumMbRec != kiTotalNumMbInCurLayer {
        bFrameCompleteFlag = false;
        if (*pCtx).bInstantDecFlag {
            return ERR_INFO_MB_NUM_INADEQUATE;
        }
    } else if (*pCurDq).sLayerInfo.sNalHeaderExt.bIdrFlag && (*pCtx).iErrorCode == dsErrorFree {
        (*(*pCtx).pDec).bIsComplete = true;
        (*pCtx).bFreezeOutput = false;
    }

    (*pCtx).iTotalNumMbRec = 0;

    (*pDstInfo).uiOutYuvTimeStamp = (*pPic).uiTimeStamp;
    *ppDst.add(0) = (*pPic).pData[0];
    *ppDst.add(1) = (*pPic).pData[1];
    *ppDst.add(2) = (*pPic).pData[2];

    (*pDstInfo).UsrData.sSystemBuffer.iFormat = videoFormatI420;
    (*pDstInfo).UsrData.sSystemBuffer.iWidth = kiActualWidth;
    (*pDstInfo).UsrData.sSystemBuffer.iHeight = kiActualHeight;
    (*pDstInfo).UsrData.sSystemBuffer.iStride[0] = (*pPic).iLinesize[0];
    (*pDstInfo).UsrData.sSystemBuffer.iStride[1] = (*pPic).iLinesize[1];

    if !(*ppDst.add(0)).is_null() {
        *ppDst.add(0) = (*ppDst.add(0)).add(
            ((*pCtx).sFrameCrop.iTopOffset * 2 * (*pPic).iLinesize[0] + (*pCtx).sFrameCrop.iLeftOffset * 2) as usize
        );
    }
    if !(*ppDst.add(1)).is_null() {
        *ppDst.add(1) = (*ppDst.add(1)).add(
            ((*pCtx).sFrameCrop.iTopOffset * (*pPic).iLinesize[1] + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }
    if !(*ppDst.add(2)).is_null() {
        *ppDst.add(2) = (*ppDst.add(2)).add(
            ((*pCtx).sFrameCrop.iTopOffset * (*pPic).iLinesize[1] + (*pCtx).sFrameCrop.iLeftOffset) as usize
        );
    }

    for i in 0..3 {
        (*pDstInfo).pDst[i] = *ppDst.add(i);
    }
    (*pDstInfo).iBufferStatus = 1;

    (*pCtx).iLastImgWidthInPixel = (*pDstInfo).UsrData.sSystemBuffer.iWidth;
    (*pCtx).iLastImgHeightInPixel = (*pDstInfo).UsrData.sSystemBuffer.iHeight;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        (*pDstInfo).iBufferStatus = (bFrameCompleteFlag && (*pPic).bIsComplete) as i32;
    }

    if (*pDstInfo).iBufferStatus == 0 {
        if !bFrameCompleteFlag {
            (*pCtx).iErrorCode |= dsBitstreamError;
        }
        return ERR_INFO_MB_NUM_INADEQUATE;
    }

    if (*pCtx).bFreezeOutput {
        (*pDstInfo).iBufferStatus = 0;
    }

    (*pCtx).iMbEcedNum = (*pPic).iMbEcedNum;
    (*pCtx).iMbNum = (*pPic).iMbNum;
    (*pCtx).iMbEcedPropNum = (*pPic).iMbEcedPropNum;

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
        if (*pDstInfo).iBufferStatus != 0
            && !(*pCtx).pDecoderStatistics.is_null()
            && ((*(*pCtx).pDecoderStatistics).uiWidth != kiActualWidth as u32
                || (*(*pCtx).pDecoderStatistics).uiHeight != kiActualHeight as u32)
        {
            (*(*pCtx).pDecoderStatistics).uiResolutionChangeTimes += 1;
            (*(*pCtx).pDecoderStatistics).uiWidth = kiActualWidth as u32;
            (*(*pCtx).pDecoderStatistics).uiHeight = kiActualHeight as u32;
        }
        UpdateDecStat(pCtx, (*pDstInfo).iBufferStatus != 0);
    }

    ERR_NONE
}

#[inline]
pub fn CheckSliceNeedReconstruct(uiLayerDqId: u8, uiTargetDqId: u8) -> bool {
    uiLayerDqId == uiTargetDqId
}

#[inline]
pub unsafe fn GetTargetDqId(uiTargetDqId: u8, psParam: *mut SDecodingParam) -> u8 {
    let uiRequiredDqId = if !psParam.is_null() {
        (*psParam).uiTargetDqLayer
    } else {
        255
    };
    WELS_MIN(uiTargetDqId, uiRequiredDqId)
}

#[inline]
pub unsafe fn HandleReferenceLostL0(pCtx: PWelsDecoderContext, pCurNal: PNalUnit) {
    if !pCurNal.is_null() && (*pCurNal).sNalHeaderExt.uiTemporalId == 0 {
        (*pCtx).bReferenceLostAtT0Flag = true;
    }
    (*pCtx).iErrorCode |= dsBitstreamError;
}

#[inline]
pub unsafe fn HandleReferenceLost(pCtx: PWelsDecoderContext, pCurNal: PNalUnit) {
    if !pCurNal.is_null()
        && ((*pCurNal).sNalHeaderExt.uiTemporalId == 0 || (*pCurNal).sNalHeaderExt.uiTemporalId == 1)
    {
        (*pCtx).bReferenceLostAtT0Flag = true;
    }
    (*pCtx).iErrorCode |= dsRefLost;
}

#[inline]
pub unsafe fn WelsDecodeConstructSlice(pCtx: PWelsDecoderContext, pCurNal: PNalUnit) -> i32 {
    let iRet = WelsTargetSliceConstruction(pCtx);
    if iRet != ERR_NONE {
        HandleReferenceLostL0(pCtx, pCurNal);
    }
    iRet
}

pub unsafe fn ParsePredWeightedTable(pBs: PBitStringAux, pSh: PSliceHeader) -> i32 {
    if pBs.is_null() || pSh.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode > 7 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_LOG2_WEIGHT_DENOM);
    }
    (*pSh).sPredWeightTable.uiLumaLog2WeightDenom = uiCode;

    let pSps = (*pSh).pSps;
    if !pSps.is_null() && (*pSps).uiChromaArrayType != 0 {
        if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > 7 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_LOG2_WEIGHT_DENOM);
        }
        (*pSh).sPredWeightTable.uiChromaLog2WeightDenom = uiCode;
    }

    if ((*pSh).sPredWeightTable.uiLumaLog2WeightDenom | (*pSh).sPredWeightTable.uiChromaLog2WeightDenom) > 7 {
        return ERR_NONE;
    }

    let mut iList = 0;
    while iList < LIST_A {
        for i in 0..((*pSh).uiRefCount[iList] as usize) {
            if i >= MAX_REF_PIC_COUNT {
                break;
            }
            if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode != 0 {
                if BsGetSe(pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if iCode < -128 || iCode > 127 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_WEIGHT);
                }
                (*pSh).sPredWeightTable.sPredList[iList].iLumaWeight[i] = iCode;

                if BsGetSe(pBs, &mut iCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                if iCode < -128 || iCode > 127 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_LUMA_OFFSET);
                }
                (*pSh).sPredWeightTable.sPredList[iList].iLumaOffset[i] = iCode;
            } else {
                (*pSh).sPredWeightTable.sPredList[iList].iLumaWeight[i] =
                    1 << (*pSh).sPredWeightTable.uiLumaLog2WeightDenom;
                (*pSh).sPredWeightTable.sPredList[iList].iLumaOffset[i] = 0;
            }

            if !pSps.is_null() && (*pSps).uiChromaArrayType == 0 {
                continue;
            }

            if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
                return ERR_INFO_INVALID_ACCESS;
            }
            if uiCode != 0 {
                for j in 0..2 {
                    if BsGetSe(pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if iCode < -128 || iCode > 127 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_WEIGHT);
                    }
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaWeight[i][j] = iCode;

                    if BsGetSe(pBs, &mut iCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if iCode < -128 || iCode > 127 {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_CHROMA_OFFSET);
                    }
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = iCode;
                }
            } else {
                for j in 0..2 {
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaWeight[i][j] =
                        1 << (*pSh).sPredWeightTable.uiChromaLog2WeightDenom;
                    (*pSh).sPredWeightTable.sPredList[iList].iChromaOffset[i][j] = 0;
                }
            }
        }
        iList += 1;
        if (*pSh).eSliceType != B_SLICE {
            break;
        }
    }
    ERR_NONE
}

pub unsafe fn CreateImplicitWeightTable(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return;
    }
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pSliceHeader = &mut (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader;
    let pPps = (*pSliceHeader).pPps;
    if pPps.is_null() {
        return;
    }

    if (*pCurDqLayer).bUseWeightedBiPredIdc && (*pPps).uiWeightedBipredIdc == 2 {
        let iPoc = (*pSliceHeader).iPicOrderCntLsb;
        let ref0 = (*pCtx).sRefPic.pRefList[LIST_0][0];
        let ref1 = (*pCtx).sRefPic.pRefList[LIST_1][0];
        if !ref0.is_null() && !ref1.is_null() {
            if (*pSliceHeader).uiRefCount[0] == 1
                && (*pSliceHeader).uiRefCount[1] == 1
                && ((*ref0).iFramePoc as i64 + (*ref1).iFramePoc as i64 == 2 * (iPoc as i64))
            {
                (*pCurDqLayer).bUseWeightedBiPredIdc = false;
                return;
            }
        }

        if !(*pCurDqLayer).pPredWeightTable.is_null() {
            (*(*pCurDqLayer).pPredWeightTable).uiLumaLog2WeightDenom = 5;
            (*(*pCurDqLayer).pPredWeightTable).uiChromaLog2WeightDenom = 5;
            for iRef0 in 0..((*pSliceHeader).uiRefCount[0] as usize) {
                let pRef0 = (*pCtx).sRefPic.pRefList[LIST_0][iRef0];
                if !pRef0.is_null() {
                    let iPoc0 = (*pRef0).iFramePoc;
                    let bIsLongRef0 = (*pRef0).bIsLongRef;
                    for iRef1 in 0..((*pSliceHeader).uiRefCount[1] as usize) {
                        let pRef1 = (*pCtx).sRefPic.pRefList[LIST_1][iRef1];
                        if !pRef1.is_null() {
                            let iPoc1 = (*pRef1).iFramePoc;
                            let bIsLongRef1 = (*pRef1).bIsLongRef;
                            (*(*pCurDqLayer).pPredWeightTable).iImplicitWeight[iRef0][iRef1] = 32;
                            if !bIsLongRef0 && !bIsLongRef1 {
                                let iTd = WELS_CLIP3(iPoc1 - iPoc0, -128, 127);
                                if iTd != 0 {
                                    let iTb = WELS_CLIP3(iPoc - iPoc0, -128, 127);
                                    let iTx = (16384 + (WELS_ABS(iTd) >> 1)) / iTd;
                                    let iDistScaleFactor = (iTb * iTx + 32) >> 8;
                                    if iDistScaleFactor >= -64 && iDistScaleFactor <= 128 {
                                        (*(*pCurDqLayer).pPredWeightTable).iImplicitWeight[iRef0][iRef1] =
                                            64 - iDistScaleFactor;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub unsafe fn ParseRefPicListReordering(pBs: PBitStringAux, pSh: PSliceHeader) -> i32 {
    if pBs.is_null() || pSh.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let keSt = (*pSh).eSliceType;
    if keSt == I_SLICE || keSt == SI_SLICE {
        return ERR_NONE;
    }
    let pRefPicListReordering = &mut (*pSh).pRefPicListReordering;
    let pSps = (*pSh).pSps;
    if pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }

    let mut iList = 0;
    let mut uiCode: u32 = 0;
    loop {
        if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        pRefPicListReordering.bRefPicListReorderingFlag[iList] = uiCode != 0;

        if pRefPicListReordering.bRefPicListReorderingFlag[iList] {
            let mut iIdx = 0;
            loop {
                if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                let kuiIdc = uiCode;
                if (iIdx >= MAX_REF_PIC_COUNT && kuiIdc != 3) || kuiIdc > 3 {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                }
                pRefPicListReordering.sReorderingSyn[iList][iIdx].uiReorderingOfPicNumsIdc = kuiIdc;
                if kuiIdc == 3 {
                    break;
                }
                if iIdx >= (*pSh).uiRefCount[iList] as usize || iIdx >= MAX_REF_PIC_COUNT {
                    return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                }
                if kuiIdc == 0 || kuiIdc == 1 {
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    if uiCode >= (1u32 << (*pSps).uiLog2MaxFrameNum) {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                    }
                    pRefPicListReordering.sReorderingSyn[iList][iIdx].uiAbsDiffPicNumMinus1 = uiCode;
                } else if kuiIdc == 2 {
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    pRefPicListReordering.sReorderingSyn[iList][iIdx].uiLongTermPicNum = uiCode;
                }
                iIdx += 1;
            }
        }
        if keSt != B_SLICE {
            break;
        }
        iList += 1;
        if iList >= LIST_A {
            break;
        }
    }
    ERR_NONE
}

pub unsafe fn ParseDecRefPicMarking(
    pCtx: PWelsDecoderContext,
    pBs: PBitStringAux,
    pSh: PSliceHeader,
    pSps: PSps,
    kbIdrFlag: bool,
) -> i32 {
    if pBs.is_null() || pSh.is_null() || pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let kpRefMarking = &mut (*pSh).sRefMarking;
    let mut uiCode: u32 = 0;

    if kbIdrFlag {
        if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        kpRefMarking.bNoOutputOfPriorPicsFlag = uiCode != 0;
        if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        kpRefMarking.bLongTermRefFlag = uiCode != 0;
    } else {
        if BsGetOneBit(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        kpRefMarking.bAdaptiveRefPicMarkingModeFlag = uiCode != 0;
        if kpRefMarking.bAdaptiveRefPicMarkingModeFlag {
            let mut iIdx = 0;
            let mut bAllowMmco5 = true;
            let mut bMmco4Exist = false;
            let mut bMmco5Exist = false;
            let mut bMmco6Exist = false;

            while iIdx < MAX_MMCO_COUNT {
                if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                    return ERR_INFO_INVALID_ACCESS;
                }
                let kuiMmco = uiCode;
                kpRefMarking.sMmcoRef[iIdx].uiMmcoType = kuiMmco;
                if kuiMmco == MMCO_END {
                    break;
                }
                if kuiMmco == MMCO_SHORT2UNUSED || kuiMmco == MMCO_SHORT2LONG {
                    bAllowMmco5 = false;
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    kpRefMarking.sMmcoRef[iIdx].iDiffOfPicNum = 1 + (uiCode as i32);
                    kpRefMarking.sMmcoRef[iIdx].iShortFrameNum = ((*pSh).iFrameNum
                        - kpRefMarking.sMmcoRef[iIdx].iDiffOfPicNum)
                        & (((1 << (*pSps).uiLog2MaxFrameNum) - 1) as i32);
                } else if kuiMmco == MMCO_LONG2UNUSED {
                    bAllowMmco5 = false;
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    kpRefMarking.sMmcoRef[iIdx].uiLongTermPicNum = uiCode;
                }
                if kuiMmco == MMCO_SHORT2LONG || kuiMmco == MMCO_LONG {
                    if kuiMmco == MMCO_LONG {
                        if bMmco6Exist {
                            return -1;
                        }
                        bMmco6Exist = true;
                    }
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    kpRefMarking.sMmcoRef[iIdx].iLongTermFrameIdx = uiCode as i32;
                } else if kuiMmco == MMCO_SET_MAX_LONG {
                    if bMmco4Exist {
                        return -1;
                    }
                    bMmco4Exist = true;
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    let iMaxLongTermFrameIdx = -1 + (uiCode as i32);
                    if iMaxLongTermFrameIdx > (*pSps).iNumRefFrames {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_MARKING);
                    }
                    kpRefMarking.sMmcoRef[iIdx].iMaxLongTermFrameIdx = iMaxLongTermFrameIdx;
                } else if kuiMmco == MMCO_RESET {
                    if !bAllowMmco5 || bMmco5Exist {
                        return -1;
                    }
                    bMmco5Exist = true;
                    if !(*pCtx).pLastDecPicInfo.is_null() {
                        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb = 0;
                        (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb = 0;
                    }
                    (*pSh).iPicOrderCntLsb = 0;
                    if !(*pCtx).pSliceHeader.is_null() {
                        (*(*pCtx).pSliceHeader).iPicOrderCntLsb = 0;
                    }
                }
                iIdx += 1;
            }
        }
    }
    ERR_NONE
}

pub unsafe fn FillDefaultSliceHeaderExt(
    pShExt: PSliceHeaderExt,
    pNalExt: PNalUnitHeaderExt,
) -> bool {
    if pShExt.is_null() || pNalExt.is_null() {
        return false;
    }
    if (*pNalExt).iNoInterLayerPredFlag != 0 || (*pNalExt).uiQualityId > 0 {
        (*pShExt).bBasePredWeightTableFlag = false;
    } else {
        (*pShExt).bBasePredWeightTableFlag = true;
    }
    (*pShExt).uiRefLayerDqId = 255;
    (*pShExt).uiDisableInterLayerDeblockingFilterIdc = 0;
    (*pShExt).iInterLayerSliceAlphaC0Offset = 0;
    (*pShExt).iInterLayerSliceBetaOffset = 0;
    (*pShExt).bConstrainedIntraResamplingFlag = false;
    (*pShExt).uiRefLayerChromaPhaseXPlus1Flag = 0;
    (*pShExt).uiRefLayerChromaPhaseYPlus1 = 1;
    (*pShExt).iScaledRefLayerPicWidthInSampleLuma = (*pShExt).sSliceHeader.iMbWidth << 4;
    (*pShExt).iScaledRefLayerPicHeightInSampleLuma = (*pShExt).sSliceHeader.iMbHeight << 4;
    (*pShExt).bSliceSkipFlag = false;
    (*pShExt).bAdaptiveBaseModeFlag = false;
    (*pShExt).bDefaultBaseModeFlag = false;
    (*pShExt).bAdaptiveMotionPredFlag = false;
    (*pShExt).bDefaultMotionPredFlag = false;
    (*pShExt).bAdaptiveResidualPredFlag = false;
    (*pShExt).bDefaultResidualPredFlag = false;
    (*pShExt).bTCoeffLevelPredFlag = false;
    (*pShExt).uiScanIdxStart = 0;
    (*pShExt).uiScanIdxEnd = 15;
    true
}

pub unsafe fn InitBsBuffer(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pMa = (*pCtx).pMemAlign;
    (*pCtx).iMaxBsBufferSizeInByte = (MIN_ACCESS_UNIT_CAPACITY * MAX_BUFFERED_NUM) as i32;
    let head = WelsMalloczHelper(pMa, (*pCtx).iMaxBsBufferSizeInByte as usize);
    if head.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    (*pCtx).sRawData.pHead = head;
    (*pCtx).sRawData.pStartPos = head;
    (*pCtx).sRawData.pCurPos = head;
    (*pCtx).sRawData.pEnd = head.add((*pCtx).iMaxBsBufferSizeInByte as usize);

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        let pParser = WelsMalloczHelper(pMa, std::mem::size_of::<SParserBsInfo>()) as *mut SParserBsInfo;
        if pParser.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pCtx).pParserBsInfo = pParser;
        let dstBuff = WelsMalloczHelper(pMa, MAX_ACCESS_UNIT_CAPACITY);
        if dstBuff.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pParser).pDstBuff = dstBuff;

        let savedHead = WelsMalloczHelper(pMa, (*pCtx).iMaxBsBufferSizeInByte as usize);
        if savedHead.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pCtx).sSavedData.pHead = savedHead;
        (*pCtx).sSavedData.pStartPos = savedHead;
        (*pCtx).sSavedData.pCurPos = savedHead;
        (*pCtx).sSavedData.pEnd = savedHead.add((*pCtx).iMaxBsBufferSizeInByte as usize);

        (*pCtx).iMaxNalNum = (MAX_NAL_UNITS_IN_LAYER + 2) as i32;
        let nalLen = WelsMalloczHelper(pMa, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>()) as *mut i32;
        if nalLen.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pParser).pNalLenInByte = nalLen;
    }
    ERR_NONE
}

pub unsafe fn ExpandBsBuffer(pCtx: PWelsDecoderContext, kiSrcLen: i32) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let iExpandStepShift = 1;
    let iNewBuffLen = WELS_MAX(
        kiSrcLen * (MAX_BUFFERED_NUM as i32),
        (*pCtx).iMaxBsBufferSizeInByte << iExpandStepShift,
    );
    let pMa = (*pCtx).pMemAlign;
    let pNewBsBuff = WelsMalloczHelper(pMa, iNewBuffLen as usize);
    if pNewBsBuff.is_null() {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }

    if !(*pCtx).pAccessUnitList.is_null() {
        for i in 0..=(*(*pCtx).pAccessUnitList).uiActualUnitsNum as usize {
            if i < MAX_NAL_UNIT_NUM_IN_AU && !(*(*pCtx).pAccessUnitList).pNalUnitsList[i].is_null() {
                let pSliceBitsRead = &mut (*(*(*pCtx).pAccessUnitList).pNalUnitsList[i]).sNalData.sVclNal.sSliceBitsRead;
                if !pSliceBitsRead.pStartBuf.is_null() && !(*pCtx).sRawData.pHead.is_null() {
                    let offset = pSliceBitsRead.pStartBuf.offset_from((*pCtx).sRawData.pHead);
                    pSliceBitsRead.pStartBuf = pNewBsBuff.offset(offset);
                }
                if !pSliceBitsRead.pEndBuf.is_null() && !(*pCtx).sRawData.pHead.is_null() {
                    let offset = pSliceBitsRead.pEndBuf.offset_from((*pCtx).sRawData.pHead);
                    pSliceBitsRead.pEndBuf = pNewBsBuff.offset(offset);
                }
                if !pSliceBitsRead.pCurBuf.is_null() && !(*pCtx).sRawData.pHead.is_null() {
                    let offset = pSliceBitsRead.pCurBuf.offset_from((*pCtx).sRawData.pHead);
                    pSliceBitsRead.pCurBuf = pNewBsBuff.offset(offset);
                }
            }
        }
    }

    if !(*pCtx).sRawData.pHead.is_null() {
        std::ptr::copy_nonoverlapping(
            (*pCtx).sRawData.pHead,
            pNewBsBuff,
            (*pCtx).iMaxBsBufferSizeInByte as usize,
        );
        let startOff = (*pCtx).sRawData.pStartPos.offset_from((*pCtx).sRawData.pHead);
        let curOff = (*pCtx).sRawData.pCurPos.offset_from((*pCtx).sRawData.pHead);
        (*pCtx).sRawData.pStartPos = pNewBsBuff.offset(startOff);
        (*pCtx).sRawData.pCurPos = pNewBsBuff.offset(curOff);
        (*pCtx).sRawData.pEnd = pNewBsBuff.add(iNewBuffLen as usize);
        WelsFreeHelper(pMa, (*pCtx).sRawData.pHead, (*pCtx).iMaxBsBufferSizeInByte as usize);
        (*pCtx).sRawData.pHead = pNewBsBuff;
    }

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        let pNewSaved = WelsMalloczHelper(pMa, iNewBuffLen as usize);
        if pNewSaved.is_null() {
            (*pCtx).iErrorCode |= dsOutOfMemory;
            return ERR_INFO_OUT_OF_MEMORY;
        }
        if !(*pCtx).sSavedData.pHead.is_null() {
            std::ptr::copy_nonoverlapping(
                (*pCtx).sSavedData.pHead,
                pNewSaved,
                (*pCtx).iMaxBsBufferSizeInByte as usize,
            );
            let startOff = (*pCtx).sSavedData.pStartPos.offset_from((*pCtx).sSavedData.pHead);
            let curOff = (*pCtx).sSavedData.pCurPos.offset_from((*pCtx).sSavedData.pHead);
            (*pCtx).sSavedData.pStartPos = pNewSaved.offset(startOff);
            (*pCtx).sSavedData.pCurPos = pNewSaved.offset(curOff);
            (*pCtx).sSavedData.pEnd = pNewSaved.add(iNewBuffLen as usize);
            WelsFreeHelper(pMa, (*pCtx).sSavedData.pHead, (*pCtx).iMaxBsBufferSizeInByte as usize);
            (*pCtx).sSavedData.pHead = pNewSaved;
        }
    }

    (*pCtx).iMaxBsBufferSizeInByte = iNewBuffLen;
    ERR_NONE
}

pub unsafe fn ExpandBsLenBuffer(pCtx: PWelsDecoderContext, kiCurrLen: i32) -> i32 {
    let pParser = (*pCtx).pParserBsInfo;
    if pParser.is_null() || (*pParser).pNalLenInByte.is_null() {
        return ERR_INFO_INVALID_ACCESS;
    }
    if kiCurrLen >= MAX_MB_SIZE + 2 {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    let mut iNewLen = kiCurrLen << 1;
    iNewLen = WELS_MIN(iNewLen, MAX_MB_SIZE + 2);
    let pMa = (*pCtx).pMemAlign;
    let pNewLenBuffer = WelsMalloczHelper(pMa, (iNewLen as usize) * std::mem::size_of::<i32>()) as *mut i32;
    if pNewLenBuffer.is_null() {
        (*pCtx).iErrorCode |= dsOutOfMemory;
        return ERR_INFO_OUT_OF_MEMORY;
    }
    std::ptr::copy_nonoverlapping(
        (*pParser).pNalLenInByte,
        pNewLenBuffer,
        ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>(),
    );
    WelsFreeHelper(pMa, (*pParser).pNalLenInByte as *mut u8, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>());
    (*pParser).pNalLenInByte = pNewLenBuffer;
    (*pCtx).iMaxNalNum = iNewLen;
    ERR_NONE
}

pub unsafe fn CheckBsBuffer(pCtx: PWelsDecoderContext, kiSrcLen: i32) -> i32 {
    if kiSrcLen > MAX_ACCESS_UNIT_CAPACITY as i32 {
        (*pCtx).iErrorCode |= dsBitstreamError;
        return ERR_INFO_INVALID_ACCESS;
    } else if kiSrcLen > (*pCtx).iMaxBsBufferSizeInByte / (MAX_BUFFERED_NUM as i32) {
        let ret = ExpandBsBuffer(pCtx, kiSrcLen);
        if ret != ERR_NONE {
            return ERR_INFO_OUT_OF_MEMORY;
        }
    }
    ERR_NONE
}

pub unsafe fn WelsInitStaticMemory(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    if MemInitNalList(&mut (*pCtx).pAccessUnitList, MAX_NAL_UNIT_NUM_IN_AU, (*pCtx).pMemAlign) != 0 {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    if InitBsBuffer(pCtx) != 0 {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    (*pCtx).uiTargetDqId = 255;
    (*pCtx).bEndOfStreamFlag = false;
    ERR_NONE
}

pub unsafe fn WelsFreeStaticMemory(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pMa = (*pCtx).pMemAlign;
    MemFreeNalList(&mut (*pCtx).pAccessUnitList, pMa);

    if !(*pCtx).sRawData.pHead.is_null() {
        WelsFreeHelper(pMa, (*pCtx).sRawData.pHead, (*pCtx).iMaxBsBufferSizeInByte as usize);
    }
    (*pCtx).sRawData.pHead = std::ptr::null_mut();
    (*pCtx).sRawData.pEnd = std::ptr::null_mut();
    (*pCtx).sRawData.pStartPos = std::ptr::null_mut();
    (*pCtx).sRawData.pCurPos = std::ptr::null_mut();

    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
        if !(*pCtx).sSavedData.pHead.is_null() {
            WelsFreeHelper(pMa, (*pCtx).sSavedData.pHead, (*pCtx).iMaxBsBufferSizeInByte as usize);
        }
        (*pCtx).sSavedData.pHead = std::ptr::null_mut();
        (*pCtx).sSavedData.pEnd = std::ptr::null_mut();
        (*pCtx).sSavedData.pStartPos = std::ptr::null_mut();
        (*pCtx).sSavedData.pCurPos = std::ptr::null_mut();

        if !(*pCtx).pParserBsInfo.is_null() {
            let pParser = (*pCtx).pParserBsInfo;
            if !(*pParser).pNalLenInByte.is_null() {
                WelsFreeHelper(pMa, (*pParser).pNalLenInByte as *mut u8, ((*pCtx).iMaxNalNum as usize) * std::mem::size_of::<i32>());
                (*pParser).pNalLenInByte = std::ptr::null_mut();
                (*pCtx).iMaxNalNum = 0;
            }
            if !(*pParser).pDstBuff.is_null() {
                WelsFreeHelper(pMa, (*pParser).pDstBuff, MAX_ACCESS_UNIT_CAPACITY);
                (*pParser).pDstBuff = std::ptr::null_mut();
            }
            WelsFreeHelper(pMa, pParser as *mut u8, std::mem::size_of::<SParserBsInfo>());
            (*pCtx).pParserBsInfo = std::ptr::null_mut();
        }
    }
}

pub unsafe fn DecodeNalHeaderExt(pNal: PNalUnit, mut pSrc: *mut u8) {
    if pNal.is_null() || pSrc.is_null() {
        return;
    }
    let pHeaderExt = &mut (*pNal).sNalHeaderExt;
    let mut uiCurByte = *pSrc;
    pHeaderExt.bIdrFlag = (uiCurByte & 0x40) != 0;
    pHeaderExt.uiPriorityId = uiCurByte & 0x3F;

    pSrc = pSrc.add(1);
    uiCurByte = *pSrc;
    pHeaderExt.iNoInterLayerPredFlag = uiCurByte >> 7;
    pHeaderExt.uiDependencyId = (uiCurByte & 0x70) >> 4;
    pHeaderExt.uiQualityId = uiCurByte & 0x0F;

    pSrc = pSrc.add(1);
    uiCurByte = *pSrc;
    pHeaderExt.uiTemporalId = uiCurByte >> 5;
    pHeaderExt.bUseRefBasePicFlag = (uiCurByte & 0x10) != 0;
    pHeaderExt.bDiscardableFlag = (uiCurByte & 0x08) != 0;
    pHeaderExt.bOutputFlag = (uiCurByte & 0x04) != 0;
    pHeaderExt.uiReservedThree2Bits = uiCurByte & 0x03;
    pHeaderExt.uiLayerDqId = (pHeaderExt.uiDependencyId << 4) | pHeaderExt.uiQualityId;
}

pub unsafe fn UpdateDecoderStatisticsForActiveParaset(
    pDecoderStatistics: *mut SDecoderStatistics,
    pSps: PSps,
    pPps: PPps,
) {
    if pDecoderStatistics.is_null() || pSps.is_null() || pPps.is_null() {
        return;
    }
    (*pDecoderStatistics).iCurrentActiveSpsId = (*pSps).iSpsId;
    (*pDecoderStatistics).iCurrentActivePpsId = (*pPps).iPpsId;
    (*pDecoderStatistics).uiProfile = (*pSps).uiProfileIdc;
    (*pDecoderStatistics).uiLevel = (*pSps).uiLevelIdc;
}

pub unsafe fn ParseSliceHeaderSyntaxs(
    pCtx: PWelsDecoderContext,
    pBs: PBitStringAux,
    kbExtensionFlag: bool,
) -> i32 {
    if pCtx.is_null() || pBs.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    if (*pCurAu).uiAvailUnitsNum == 0 {
        return ERR_INFO_OUT_OF_MEMORY;
    }
    let kpCurNal = (*pCurAu).pNalUnitsList[((*pCurAu).uiAvailUnitsNum - 1) as usize];
    if kpCurNal.is_null() {
        return ERR_INFO_OUT_OF_MEMORY;
    }

    let pNalHeaderExt = &mut (*kpCurNal).sNalHeaderExt;
    let pSliceHead = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
    let eNalType = pNalHeaderExt.sNalUnitHeader.eNalUnitType;
    let pSliceHeadExt = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt;

    (*kpCurNal).sNalData.sVclNal.bSliceHeaderExtFlag = kbExtensionFlag;

    let mut uiCode: u32 = 0;
    let mut iCode: i32 = 0;

    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode > 36863 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FIRST_MB_IN_SLICE);
    }
    (*pSliceHead).iFirstMbInSlice = uiCode as i32;

    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    let mut uiSliceType = uiCode;
    if uiSliceType > 9 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
    }
    if uiSliceType > 4 {
        uiSliceType -= 5;
    }
    if eNalType == NAL_UNIT_CODED_SLICE_IDR && uiSliceType != 2 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
    }
    if kbExtensionFlag && uiSliceType > 2 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_SLICE_TYPE);
    }

    (*pSliceHead).eSliceType = match uiSliceType {
        0 => P_SLICE,
        1 => B_SLICE,
        2 => I_SLICE,
        3 => SP_SLICE,
        _ => SI_SLICE,
    };

    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    if uiCode >= MAX_PPS_COUNT as u32 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_PPS_ID_OVERFLOW);
    }
    let iPpsId = uiCode as i32;

    if !(*pCtx).sSpsPpsCtx.bPpsAvailFlags[iPpsId as usize] {
        if !(*pCtx).pDecoderStatistics.is_null() {
            (*(*pCtx).pDecoderStatistics).iPpsReportErrorNum += 1;
        }
        (*pCtx).iErrorCode |= dsNoParamSets;
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_PPS_ID);
    }

    let pPps = &mut (*pCtx).sSpsPpsCtx.sPpsBuffer[iPpsId as usize];
    let pSps = if kbExtensionFlag {
        let pSubsetSps = &mut (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[(*pPps).iSpsId as usize];
        &mut (*pSubsetSps).sSps
    } else {
        &mut (*pCtx).sSpsPpsCtx.sSpsBuffer[(*pPps).iSpsId as usize]
    };

    (*pSliceHead).iPpsId = iPpsId;
    (*pSliceHead).iSpsId = (*pPps).iSpsId;
    (*pSliceHead).pPps = pPps;
    (*pSliceHead).pSps = pSps;

    let bIdrFlag = (!kbExtensionFlag && eNalType == NAL_UNIT_CODED_SLICE_IDR)
        || (kbExtensionFlag && pNalHeaderExt.bIdrFlag);
    (*pSliceHead).bIdrFlag = bIdrFlag;

    if BsGetBits(pBs, (*pSps).uiLog2MaxFrameNum, &mut uiCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    (*pSliceHead).iFrameNum = uiCode as i32;
    (*pSliceHead).iMbWidth = (*pSps).iMbWidth as i32;
    (*pSliceHead).iMbHeight = (*pSps).iMbHeight as i32;

    if bIdrFlag {
        if (*pSliceHead).iFrameNum != 0 {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_FRAME_NUM);
        }
        if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        if uiCode > SLICE_HEADER_IDR_PIC_ID_MAX {
            return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_IDR_PIC_ID);
        }
        (*pSliceHead).uiIdrPicId = uiCode;
    }

    if (*pSps).uiPocType == 0 {
        if BsGetBits(pBs, (*pSps).iLog2MaxPocLsb as u32, &mut uiCode) != ERR_NONE {
            return ERR_INFO_INVALID_ACCESS;
        }
        let iMaxPocLsb = 1 << (*pSps).iLog2MaxPocLsb;
        let pocLsb = uiCode as i32;
        let prevLsb = if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb
        } else {
            0
        };
        let prevMsb = if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb
        } else {
            0
        };
        let pocMsb = if pocLsb < prevLsb && (prevLsb - pocLsb) >= (iMaxPocLsb / 2) {
            prevMsb + iMaxPocLsb
        } else if pocLsb > prevLsb && (pocLsb - prevLsb) > (iMaxPocLsb / 2) {
            prevMsb - iMaxPocLsb
        } else {
            prevMsb
        };
        (*pSliceHead).iPicOrderCntLsb = pocMsb + pocLsb;
        if !(*pCtx).pLastDecPicInfo.is_null() && pNalHeaderExt.sNalUnitHeader.uiNalRefIdc != 0 {
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntLsb = pocLsb;
            (*(*pCtx).pLastDecPicInfo).iPrevPicOrderCntMsb = pocMsb;
        }
    }

    if BsGetSe(pBs, &mut iCode) != ERR_NONE {
        return ERR_INFO_INVALID_ACCESS;
    }
    (*pSliceHead).iSliceQpDelta = iCode;
    (*pSliceHead).iSliceQp = (*pPps).iPicInitQp + (*pSliceHead).iSliceQpDelta;
    if (*pSliceHead).iSliceQp < 0 || (*pSliceHead).iSliceQp > 51 {
        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_QP);
    }

    if !kbExtensionFlag {
        FillDefaultSliceHeaderExt(pSliceHeadExt, pNalHeaderExt);
    }

    ERR_NONE
}

pub unsafe fn PrefetchNalHeaderExtSyntax(
    pCtx: PWelsDecoderContext,
    kpDst: PNalUnit,
    kpSrc: *const SNalUnit,
) -> bool {
    if kpDst.is_null() || kpSrc.is_null() {
        return false;
    }
    let pNalHdrExtD = &mut (*kpDst).sNalHeaderExt;
    let pNalHdrExtS = &(*kpSrc).sNalHeaderExt;
    let pShExtD = &mut (*kpDst).sNalData.sVclNal.sSliceHeaderExt;
    let pPrefixS = &(*kpSrc).sNalData.sPrefixNal;

    pNalHdrExtD.uiDependencyId = pNalHdrExtS.uiDependencyId;
    pNalHdrExtD.uiQualityId = pNalHdrExtS.uiQualityId;
    pNalHdrExtD.uiTemporalId = pNalHdrExtS.uiTemporalId;
    pNalHdrExtD.uiPriorityId = pNalHdrExtS.uiPriorityId;
    pNalHdrExtD.bIdrFlag = pNalHdrExtS.bIdrFlag;
    pNalHdrExtD.iNoInterLayerPredFlag = pNalHdrExtS.iNoInterLayerPredFlag;
    pNalHdrExtD.bDiscardableFlag = pNalHdrExtS.bDiscardableFlag;
    pNalHdrExtD.bOutputFlag = pNalHdrExtS.bOutputFlag;
    pNalHdrExtD.bUseRefBasePicFlag = pNalHdrExtS.bUseRefBasePicFlag;
    pNalHdrExtD.uiLayerDqId = pNalHdrExtS.uiLayerDqId;

    (*pShExtD).bStoreRefBasePicFlag = pPrefixS.bStoreRefBasePicFlag;
    (*pShExtD).sRefBasePicMarking = pPrefixS.sRefPicBaseMarking;
    true
}

pub unsafe fn UpdateAccessUnit(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let iIdx = (*pCurAu).uiEndPos as usize;
    if iIdx < MAX_NAL_UNIT_NUM_IN_AU && !(*pCurAu).pNalUnitsList[iIdx].is_null() {
        (*pCtx).uiTargetDqId = (*(*pCurAu).pNalUnitsList[iIdx]).sNalHeaderExt.uiLayerDqId;
    }
    (*pCurAu).uiActualUnitsNum = (*pCurAu).uiEndPos + 1;
    (*pCurAu).bCompletedAuFlag = true;
    ERR_NONE
}

pub unsafe fn InitialDqLayersContext(
    pCtx: PWelsDecoderContext,
    kiMaxWidth: i32,
    kiMaxHeight: i32,
) -> i32 {
    if pCtx.is_null() || kiMaxWidth <= 0 || kiMaxHeight <= 0 {
        return ERR_INFO_INVALID_PARAM;
    }
    (*pCtx).sMb.iMbWidth = (kiMaxWidth + 15) >> 4;
    (*pCtx).sMb.iMbHeight = (kiMaxHeight + 15) >> 4;

    if (*pCtx).bInitialDqLayersMem
        && kiMaxWidth <= (*pCtx).iPicWidthReq
        && kiMaxHeight <= (*pCtx).iPicHeightReq
    {
        return ERR_NONE;
    }

    UninitialDqLayersContext(pCtx);

    let pMa = (*pCtx).pMemAlign;
    let numMb = ((*pCtx).sMb.iMbWidth * (*pCtx).sMb.iMbHeight) as usize;

    for i in 0..LAYER_NUM_EXCHANGEABLE {
        let pDq = WelsMalloczHelper(pMa, std::mem::size_of::<SDqLayer>()) as PDqLayer;
        if pDq.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        (*pCtx).pDqLayersList[i] = pDq;

        (*pCtx).sMb.pMbType[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u32>()) as *mut u32;
        (*pCtx).sMb.pMv[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut i16;
        (*pCtx).sMb.pMv[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut i16;
        (*pCtx).sMb.pRefIndex[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pRefIndex[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pDirect[i] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pLumaQp[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pChromaQp[i] = WelsMalloczHelper(pMa, numMb * 2 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pMvd[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut i16;
        (*pCtx).sMb.pMvd[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut i16;
        (*pCtx).sMb.pCbfDc[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u16>()) as *mut u16;
        (*pCtx).sMb.pNzc[i] = WelsMalloczHelper(pMa, numMb * 24 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pNzcRs[i] = WelsMalloczHelper(pMa, numMb * 24 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pScaledTCoeff[i] = WelsMalloczHelper(pMa, numMb * MB_COEFF_LIST_SIZE * std::mem::size_of::<i16>()) as *mut i16;
        (*pCtx).sMb.pIntraPredMode[i] = WelsMalloczHelper(pMa, numMb * 8 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pIntra4x4FinalMode[i] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pIntraNxNAvailFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u8>()) as *mut u8;
        (*pCtx).sMb.pChromaPredMode[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pCbp[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pSubMbType[i] = WelsMalloczHelper(pMa, numMb * MB_PARTITION_SIZE * std::mem::size_of::<u32>()) as *mut u32;
        (*pCtx).sMb.pSliceIdc[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i32>()) as *mut i32;
        (*pCtx).sMb.pResidualPredFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pInterPredictionDoneFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut i8;
        (*pCtx).sMb.pMbCorrectlyDecodedFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<bool>()) as *mut bool;
        (*pCtx).sMb.pMbRefConcealedFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<bool>()) as *mut bool;
    }

    (*pCtx).bInitialDqLayersMem = true;
    (*pCtx).iPicWidthReq = kiMaxWidth;
    (*pCtx).iPicHeightReq = kiMaxHeight;
    ERR_NONE
}

pub unsafe fn UninitialDqLayersContext(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let pMa = (*pCtx).pMemAlign;
    let numMb = ((*pCtx).sMb.iMbWidth * (*pCtx).sMb.iMbHeight) as usize;

    for i in 0..LAYER_NUM_EXCHANGEABLE {
        let pDq = (*pCtx).pDqLayersList[i];
        if pDq.is_null() {
            continue;
        }
        if !(*pCtx).sMb.pMbType[i].is_null() {
            WelsFreeHelper(pMa, (*pCtx).sMb.pMbType[i] as *mut u8, numMb * std::mem::size_of::<u32>());
            (*pCtx).sMb.pMbType[i] = std::ptr::null_mut();
        }
        for list in 0..LIST_A {
            if !(*pCtx).sMb.pMv[i][list].is_null() {
                WelsFreeHelper(pMa, (*pCtx).sMb.pMv[i][list] as *mut u8, numMb * 16 * 2 * std::mem::size_of::<i16>());
                (*pCtx).sMb.pMv[i][list] = std::ptr::null_mut();
            }
            if !(*pCtx).sMb.pRefIndex[i][list].is_null() {
                WelsFreeHelper(pMa, (*pCtx).sMb.pRefIndex[i][list] as *mut u8, numMb * 16 * std::mem::size_of::<i8>());
                (*pCtx).sMb.pRefIndex[i][list] = std::ptr::null_mut();
            }
            if !(*pCtx).sMb.pMvd[i][list].is_null() {
                WelsFreeHelper(pMa, (*pCtx).sMb.pMvd[i][list] as *mut u8, numMb * 16 * 2 * std::mem::size_of::<i16>());
                (*pCtx).sMb.pMvd[i][list] = std::ptr::null_mut();
            }
        }
        WelsFreeHelper(pMa, pDq as *mut u8, std::mem::size_of::<SDqLayer>());
        (*pCtx).pDqLayersList[i] = std::ptr::null_mut();
    }
    (*pCtx).iPicWidthReq = 0;
    (*pCtx).iPicHeightReq = 0;
    (*pCtx).bInitialDqLayersMem = false;
}

pub unsafe fn ResetCurrentAccessUnit(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    (*pCurAu).uiStartPos = 0;
    (*pCurAu).uiEndPos = 0;
    (*pCurAu).bCompletedAuFlag = false;
    if (*pCurAu).uiActualUnitsNum > 0 {
        let kuiActualNum = (*pCurAu).uiActualUnitsNum;
        let kuiAvailNum = (*pCurAu).uiAvailUnitsNum;
        let kuiLeftNum = if kuiAvailNum > kuiActualNum { kuiAvailNum - kuiActualNum } else { 0 };
        for iIdx in 0..kuiLeftNum as usize {
            let t = (*pCurAu).pNalUnitsList[kuiActualNum as usize + iIdx];
            (*pCurAu).pNalUnitsList[kuiActualNum as usize + iIdx] = (*pCurAu).pNalUnitsList[iIdx];
            (*pCurAu).pNalUnitsList[iIdx] = t;
        }
        (*pCurAu).uiActualUnitsNum = kuiLeftNum;
        (*pCurAu).uiAvailUnitsNum = kuiLeftNum;
    }
}

pub unsafe fn ForceResetCurrentAccessUnit(pAu: PAccessUnit) {
    if pAu.is_null() {
        return;
    }
    let mut uiSucAuIdx = (*pAu).uiEndPos + 1;
    let mut uiCurAuIdx = 0;
    while uiSucAuIdx < (*pAu).uiAvailUnitsNum {
        let t = (*pAu).pNalUnitsList[uiSucAuIdx as usize];
        (*pAu).pNalUnitsList[uiSucAuIdx as usize] = (*pAu).pNalUnitsList[uiCurAuIdx as usize];
        (*pAu).pNalUnitsList[uiCurAuIdx as usize] = t;
        uiSucAuIdx += 1;
        uiCurAuIdx += 1;
    }
    if (*pAu).uiAvailUnitsNum > (*pAu).uiEndPos {
        (*pAu).uiAvailUnitsNum -= (*pAu).uiEndPos + 1;
    } else {
        (*pAu).uiAvailUnitsNum = 0;
    }
    (*pAu).uiActualUnitsNum = 0;
    (*pAu).uiStartPos = 0;
    (*pAu).uiEndPos = 0;
    (*pAu).bCompletedAuFlag = false;
}

pub unsafe fn ForceClearCurrentNal(pAu: PAccessUnit) {
    if !pAu.is_null() && (*pAu).uiAvailUnitsNum > 0 {
        (*pAu).uiAvailUnitsNum -= 1;
    }
}

pub unsafe fn ForceResetParaSetStatusAndAUList(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    (*pCtx).sSpsPpsCtx.bSpsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bSubspsExistAheadFlag = false;
    (*pCtx).sSpsPpsCtx.bPpsExistAheadFlag = false;

    if !(*pCtx).pAccessUnitList.is_null() {
        let pAu = (*pCtx).pAccessUnitList;
        (*pAu).uiAvailUnitsNum = 0;
        (*pAu).uiActualUnitsNum = 0;
        (*pAu).uiStartPos = 0;
        (*pAu).uiEndPos = 0;
        (*pAu).bCompletedAuFlag = false;
    }
}

pub unsafe fn CheckAvailNalUnitsListContinuity(
    pCtx: PWelsDecoderContext,
    iStartIdx: i32,
    iEndIdx: i32,
) {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let mut uiLastNuDependencyId = (*(*pCurAu).pNalUnitsList[iStartIdx as usize]).sNalHeaderExt.uiDependencyId;
    let mut uiLastNuLayerDqId = (*(*pCurAu).pNalUnitsList[iStartIdx as usize]).sNalHeaderExt.uiLayerDqId;
    let mut iCurNalUnitIdx = iStartIdx + 1;

    while iCurNalUnitIdx <= iEndIdx {
        let pNal = (*pCurAu).pNalUnitsList[iCurNalUnitIdx as usize];
        let uiCurNuDependencyId = (*pNal).sNalHeaderExt.uiDependencyId;
        let uiCurNuQualityId = (*pNal).sNalHeaderExt.uiQualityId;
        let uiCurNuLayerDqId = (*pNal).sNalHeaderExt.uiLayerDqId;
        let uiCurNuRefLayerDqId = (*pNal).sNalData.sVclNal.sSliceHeaderExt.uiRefLayerDqId;

        if uiCurNuDependencyId == uiLastNuDependencyId {
            uiLastNuLayerDqId = uiCurNuLayerDqId;
            iCurNalUnitIdx += 1;
        } else {
            if uiCurNuQualityId == 0 {
                uiLastNuDependencyId = uiCurNuDependencyId;
                if uiCurNuRefLayerDqId == uiLastNuLayerDqId {
                    uiLastNuLayerDqId = uiCurNuLayerDqId;
                    iCurNalUnitIdx += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    iCurNalUnitIdx -= 1;
    (*pCurAu).uiEndPos = iCurNalUnitIdx as u32;
    (*pCtx).uiTargetDqId = (*(*pCurAu).pNalUnitsList[iCurNalUnitIdx as usize]).sNalHeaderExt.uiLayerDqId;
}

pub unsafe fn RefineIdxNoInterLayerPred(pCurAu: PAccessUnit, pIdxNoInterLayerPred: *mut i32) {
    if pCurAu.is_null() || pIdxNoInterLayerPred.is_null() {
        return;
    }
    let idx = *pIdxNoInterLayerPred as usize;
    let pNal = (*pCurAu).pNalUnitsList[idx];
    if pNal.is_null() {
        return;
    }
    let iLastNalDependId = (*pNal).sNalHeaderExt.uiDependencyId;
    let iLastNalQualityId = (*pNal).sNalHeaderExt.uiQualityId;
    let uiLastNalTId = (*pNal).sNalHeaderExt.uiTemporalId;
    let iLastNalFrameNum = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
    let iLastNalPoc = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
    let iLastNalFirstMb = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

    let mut bMultiSliceFind = false;
    let mut iFinalIdxNoInterLayerPred = 0;
    let mut iCurIdx = (*pIdxNoInterLayerPred) - 1;

    while iCurIdx >= 0 {
        let pCurNal = (*pCurAu).pNalUnitsList[iCurIdx as usize];
        if !pCurNal.is_null() && (*pCurNal).sNalHeaderExt.iNoInterLayerPredFlag != 0 {
            let iCurNalDependId = (*pCurNal).sNalHeaderExt.uiDependencyId;
            let iCurNalQualityId = (*pCurNal).sNalHeaderExt.uiQualityId;
            let iCurNalTId = (*pCurNal).sNalHeaderExt.uiTemporalId;
            let iCurNalFrameNum = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFrameNum;
            let iCurNalPoc = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iPicOrderCntLsb;
            let iCurNalFirstMb = (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;

            if iCurNalDependId == iLastNalDependId
                && iCurNalQualityId == iLastNalQualityId
                && iCurNalTId == uiLastNalTId
                && iCurNalFrameNum == iLastNalFrameNum
                && iCurNalPoc == iLastNalPoc
                && iCurNalFirstMb != iLastNalFirstMb
            {
                bMultiSliceFind = true;
                iFinalIdxNoInterLayerPred = iCurIdx;
                iCurIdx -= 1;
                continue;
            } else {
                break;
            }
        }
        iCurIdx -= 1;
    }

    if bMultiSliceFind && *pIdxNoInterLayerPred != iFinalIdxNoInterLayerPred {
        *pIdxNoInterLayerPred = iFinalIdxNoInterLayerPred;
    }
}

pub unsafe fn CheckPocOfCurValidNalUnits(pCurAu: PAccessUnit, pIdxNoInterLayerPred: i32) -> bool {
    if pCurAu.is_null() {
        return false;
    }
    let iEndIdx = (*pCurAu).uiEndPos as i32;
    let iCurAuPoc = (*(*pCurAu).pNalUnitsList[pIdxNoInterLayerPred as usize])
        .sNalData
        .sVclNal
        .sSliceHeaderExt
        .sSliceHeader
        .iPicOrderCntLsb;

    for i in (pIdxNoInterLayerPred + 1)..iEndIdx {
        let iTmpPoc = (*(*pCurAu).pNalUnitsList[i as usize])
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .iPicOrderCntLsb;
        if iTmpPoc != iCurAuPoc {
            return false;
        }
    }
    true
}

pub unsafe fn CheckIntegrityNalUnitsList(pCtx: PWelsDecoderContext) -> bool {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return false;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let kiEndPos = (*pCurAu).uiEndPos as i32;
    let mut iIdxNoInterLayerPred: i32 = 0;

    if !(*pCurAu).bCompletedAuFlag {
        return false;
    }

    if (*pCtx).bNewSeqBegin {
        (*pCurAu).uiStartPos = 0;
        iIdxNoInterLayerPred = kiEndPos;
        while iIdxNoInterLayerPred >= 0 {
            if (*(*pCurAu).pNalUnitsList[iIdxNoInterLayerPred as usize]).sNalHeaderExt.iNoInterLayerPredFlag != 0 {
                break;
            }
            iIdxNoInterLayerPred -= 1;
        }
        if iIdxNoInterLayerPred < 0 {
            return false;
        }
        RefineIdxNoInterLayerPred(pCurAu, &mut iIdxNoInterLayerPred);
        (*pCurAu).uiStartPos = iIdxNoInterLayerPred as u32;
        CheckAvailNalUnitsListContinuity(pCtx, iIdxNoInterLayerPred, kiEndPos);
        if !CheckPocOfCurValidNalUnits(pCurAu, iIdxNoInterLayerPred) {
            return false;
        }
        let endIdx = (*pCurAu).uiEndPos as usize;
        (*pCtx).iCurSeqIntervalTargetDependId = (*(*pCurAu).pNalUnitsList[endIdx]).sNalHeaderExt.uiDependencyId;
        (*pCtx).iCurSeqIntervalMaxPicWidth = (*(*pCurAu).pNalUnitsList[endIdx])
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .iMbWidth
            << 4;
        (*pCtx).iCurSeqIntervalMaxPicHeight = (*(*pCurAu).pNalUnitsList[endIdx])
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .iMbHeight
            << 4;
    }
    true
}

pub unsafe fn CheckOnlyOneLayerInAu(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let iEndIdx = (*pCurAu).uiEndPos as usize;
    let mut iCurIdx = (*pCurAu).uiStartPos as usize;
    let uiDId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiDependencyId;
    let uiQId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiQualityId;
    let uiTId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiTemporalId;

    (*pCtx).bOnlyOneLayerInCurAuFlag = true;
    if iEndIdx == iCurIdx {
        return;
    }
    iCurIdx += 1;
    while iCurIdx <= iEndIdx {
        let uiCurDId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiDependencyId;
        let uiCurQId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiQualityId;
        let uiCurTId = (*(*pCurAu).pNalUnitsList[iCurIdx]).sNalHeaderExt.uiTemporalId;
        if uiDId != uiCurDId || uiQId != uiCurQId || uiTId != uiCurTId {
            (*pCtx).bOnlyOneLayerInCurAuFlag = false;
            return;
        }
        iCurIdx += 1;
    }
}

pub unsafe fn WelsDecodeAccessUnitStart(pCtx: PWelsDecoderContext) -> i32 {
    let iRet = UpdateAccessUnit(pCtx);
    if iRet != ERR_NONE {
        return iRet;
    }
    (*(*pCtx).pAccessUnitList).uiStartPos = 0;
    if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag && !CheckIntegrityNalUnitsList(pCtx) {
        (*pCtx).iErrorCode |= dsBitstreamError;
        return dsBitstreamError;
    }
    if !(*pCtx).sSpsPpsCtx.bAvcBasedFlag {
        CheckOnlyOneLayerInAu(pCtx);
    }
    ERR_NONE
}

pub unsafe fn WelsDecodeAccessUnitEnd(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let endIdx = (*pCurAu).uiEndPos as usize;
    if endIdx < MAX_NAL_UNIT_NUM_IN_AU && !(*pCurAu).pNalUnitsList[endIdx].is_null() {
        let pCurNal = (*pCurAu).pNalUnitsList[endIdx];
        if !(*pCtx).pLastDecPicInfo.is_null() {
            (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt = (*pCurNal).sNalHeaderExt;
            (*(*pCtx).pLastDecPicInfo).sLastSliceHeader =
                (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
        }
    }
    ResetCurrentAccessUnit(pCtx);
}

pub unsafe fn CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx: PWelsDecoderContext) -> bool {
    let mut bNewSeq = false;
    let pCurAu = (*pCtx).pAccessUnitList;
    let mut pTmpLayerSps: [*mut SSps; MAX_LAYER_NUM] = [std::ptr::null_mut(); MAX_LAYER_NUM];

    let start = (*pCurAu).uiStartPos as usize;
    let end = (*pCurAu).uiEndPos as usize;
    for i in start..=end {
        if i < MAX_NAL_UNIT_NUM_IN_AU && !(*pCurAu).pNalUnitsList[i].is_null() {
            let pNal = (*pCurAu).pNalUnitsList[i];
            let uiDid = (*pNal).sNalHeaderExt.uiDependencyId as usize;
            if uiDid < MAX_LAYER_NUM {
                pTmpLayerSps[uiDid] = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps;
            }
            if (*pNal).sNalHeaderExt.sNalUnitHeader.eNalUnitType == NAL_UNIT_CODED_SLICE_IDR
                || (*pNal).sNalHeaderExt.bIdrFlag
            {
                bNewSeq = true;
            }
        }
    }

    let mut iMaxActiveLayer = 0;
    let mut iMaxCurrentLayer = 0;
    for i in (0..MAX_LAYER_NUM).rev() {
        if !(*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_null() {
            iMaxActiveLayer = i;
            break;
        }
    }
    for i in (0..MAX_LAYER_NUM).rev() {
        if !pTmpLayerSps[i].is_null() {
            iMaxCurrentLayer = i;
            break;
        }
    }
    if iMaxCurrentLayer != iMaxActiveLayer
        || pTmpLayerSps[iMaxCurrentLayer] != (*pCtx).sSpsPpsCtx.pActiveLayerSps[iMaxActiveLayer]
    {
        bNewSeq = true;
    }
    if !bNewSeq {
        for i in 0..MAX_LAYER_NUM {
            if (*pCtx).sSpsPpsCtx.pActiveLayerSps[i].is_null() && !pTmpLayerSps[i].is_null() {
                (*pCtx).sSpsPpsCtx.pActiveLayerSps[i] = pTmpLayerSps[i];
            }
        }
    } else {
        (*pCtx).sSpsPpsCtx.pActiveLayerSps.copy_from_slice(&pTmpLayerSps);
    }
    bNewSeq
}

pub unsafe fn WriteBackActiveParameters(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_PPS) != 0 {
        let ppsId = (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT].iPpsId as usize;
        if ppsId < MAX_PPS_COUNT {
            (*pCtx).sSpsPpsCtx.sPpsBuffer[ppsId] = (*pCtx).sSpsPpsCtx.sPpsBuffer[MAX_PPS_COUNT];
        }
    }
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SPS) != 0 {
        let spsId = (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT].iSpsId as usize;
        if spsId < MAX_SPS_COUNT {
            (*pCtx).sSpsPpsCtx.sSpsBuffer[spsId] = (*pCtx).sSpsPpsCtx.sSpsBuffer[MAX_SPS_COUNT];
            (*pCtx).bNewSeqBegin = true;
        }
    }
    if ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SUBSETSPS) != 0 {
        let spsId = (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT].sSps.iSpsId as usize;
        if spsId < MAX_SPS_COUNT {
            (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[spsId] = (*pCtx).sSpsPpsCtx.sSubsetSpsBuffer[MAX_SPS_COUNT];
            (*pCtx).bNewSeqBegin = true;
        }
    }
    (*pCtx).sSpsPpsCtx.iOverwriteFlags = OVERWRITE_NONE;
}

pub unsafe fn DecodeFinishUpdate(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    (*pCtx).bNewSeqBegin = false;
    WriteBackActiveParameters(pCtx);
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || (*pCtx).bNextNewSeqBegin;
    (*pCtx).bNextNewSeqBegin = false;
    if (*pCtx).bNewSeqBegin {
        ResetActiveSPSForEachLayer(pCtx);
    }
}

pub unsafe fn WelsDecodeInitAccessUnitStart(
    pCtx: PWelsDecoderContext,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    (*pCtx).bAuReadyFlag = false;
    if !(*pCtx).pLastDecPicInfo.is_null() {
        (*(*pCtx).pLastDecPicInfo).bLastHasMmco5 = false;
    }
    let bTmpNewSeqBegin = CheckNewSeqBeginAndUpdateActiveLayerSps(pCtx);
    if bTmpNewSeqBegin {
        if !(*pCtx).pStreamSeqNum.is_null() {
            *(*pCtx).pStreamSeqNum += 1;
        } else {
            (*pCtx).iSeqNum += 1;
        }
    }
    (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || bTmpNewSeqBegin;
    if !(*pCtx).pStreamSeqNum.is_null() {
        (*pCtx).iSeqNum = *(*pCtx).pStreamSeqNum;
    }
    let iErr = WelsDecodeAccessUnitStart(pCtx);
    GetVclNalTemporalId(pCtx);

    if iErr != ERR_NONE {
        ForceResetCurrentAccessUnit((*pCtx).pAccessUnitList);
        if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly && !pDstInfo.is_null() {
            (*pDstInfo).iBufferStatus = 0;
        }
        (*pCtx).bNewSeqBegin = (*pCtx).bNewSeqBegin || (*pCtx).bNextNewSeqBegin;
        (*pCtx).bNextNewSeqBegin = false;
        if (*pCtx).bNewSeqBegin {
            ResetActiveSPSForEachLayer(pCtx);
        }
        return iErr;
    }

    let startPos = (*pCurAu).uiStartPos as usize;
    if startPos < MAX_NAL_UNIT_NUM_IN_AU && !(*pCurAu).pNalUnitsList[startPos].is_null() {
        let pNal = (*pCurAu).pNalUnitsList[startPos];
        (*pCtx).pSps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps;
        (*pCtx).pPps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pPps;
    }
    iErr
}

pub unsafe fn AllocPicBuffOnNewSeqBegin(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() || (*pCtx).pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    if GetThreadCount(pCtx) <= 1 {
        WelsResetRefPic(pCtx);
    }
    let iErr = SyncPictureResolutionExt(pCtx, (*(*pCtx).pSps).iMbWidth, (*(*pCtx).pSps).iMbHeight);
    iErr
}

pub unsafe fn InitConstructAccessUnit(
    pCtx: PWelsDecoderContext,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    let mut iErr = WelsDecodeInitAccessUnitStart(pCtx, pDstInfo);
    if iErr != ERR_NONE {
        return iErr;
    }
    if (*pCtx).bNewSeqBegin {
        iErr = AllocPicBuffOnNewSeqBegin(pCtx);
        if iErr != ERR_NONE {
            return iErr;
        }
    }
    iErr
}

pub unsafe fn ConstructAccessUnit(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    let mut iErr = ERR_NONE;
    if GetThreadCount(pCtx) <= 1 {
        iErr = InitConstructAccessUnit(pCtx, pDstInfo);
        if iErr != ERR_NONE {
            return iErr;
        }
    }
    if (*pCtx).pCabacDecEngine.is_null() {
        let pMa = (*pCtx).pMemAlign;
        (*pCtx).pCabacDecEngine = WelsMalloczHelper(pMa, std::mem::size_of::<SWelsCabacDecEngine>()) as *mut SWelsCabacDecEngine;
        if (*pCtx).pCabacDecEngine.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
    }

    iErr = DecodeCurrentAccessUnit(pCtx, ppDst, pDstInfo);
    WelsDecodeAccessUnitEnd(pCtx);
    iErr
}

pub unsafe fn InitDqLayerInfo(
    pDqLayer: PDqLayer,
    pLayerInfo: PLayerInfo,
    pNalUnit: PNalUnit,
    pPicDec: PPicture,
) {
    if pDqLayer.is_null() || pLayerInfo.is_null() || pNalUnit.is_null() {
        return;
    }
    let pNalHdrExt = &mut (*pNalUnit).sNalHeaderExt;
    let pShExt = &mut (*pNalUnit).sNalData.sVclNal.sSliceHeaderExt;
    let pSh = &mut (*pShExt).sSliceHeader;
    let kuiQualityId = pNalHdrExt.uiQualityId;

    (*pDqLayer).sLayerInfo = *pLayerInfo;
    (*pDqLayer).pDec = pPicDec;
    (*pDqLayer).iMbWidth = (*pSh).iMbWidth;
    (*pDqLayer).iMbHeight = (*pSh).iMbHeight;
    (*pDqLayer).iSliceIdcBackup = ((*pSh).iFirstMbInSlice << 7)
        | ((pNalHdrExt.uiDependencyId as i32) << 4)
        | (pNalHdrExt.uiQualityId as i32);

    if !(*pLayerInfo).pPps.is_null() {
        (*pDqLayer).uiPpsId = (*(*pLayerInfo).pPps).iPpsId;
    }
    (*pDqLayer).uiDisableInterLayerDeblockingFilterIdc = (*pShExt).uiDisableInterLayerDeblockingFilterIdc;
    (*pDqLayer).iInterLayerSliceAlphaC0Offset = (*pShExt).iInterLayerSliceAlphaC0Offset;
    (*pDqLayer).iInterLayerSliceBetaOffset = (*pShExt).iInterLayerSliceBetaOffset;
    (*pDqLayer).iSliceGroupChangeCycle = (*pSh).iSliceGroupChangeCycle;
    (*pDqLayer).bStoreRefBasePicFlag = (*pShExt).bStoreRefBasePicFlag;
    (*pDqLayer).bTCoeffLevelPredFlag = (*pShExt).bTCoeffLevelPredFlag;
    (*pDqLayer).bConstrainedIntraResamplingFlag = (*pShExt).bConstrainedIntraResamplingFlag;
    (*pDqLayer).uiRefLayerDqId = (*pShExt).uiRefLayerDqId;
    (*pDqLayer).uiRefLayerChromaPhaseXPlus1Flag = (*pShExt).uiRefLayerChromaPhaseXPlus1Flag;
    (*pDqLayer).uiRefLayerChromaPhaseYPlus1 = (*pShExt).uiRefLayerChromaPhaseYPlus1;
    (*pDqLayer).bUseWeightPredictionFlag = false;
    (*pDqLayer).bUseWeightedBiPredIdc = false;

    if kuiQualityId == BASE_QUALITY_ID {
        (*pDqLayer).pRefPicListReordering = &mut (*pSh).pRefPicListReordering;
        (*pDqLayer).pRefPicMarking = &mut (*pSh).sRefMarking;
        if !(*pSh).pPps.is_null() {
            (*pDqLayer).bUseWeightPredictionFlag = (*(*pSh).pPps).bWeightedPredFlag;
            (*pDqLayer).bUseWeightedBiPredIdc = (*(*pSh).pPps).uiWeightedBipredIdc != 0;
            if (*(*pSh).pPps).bWeightedPredFlag || (*(*pSh).pPps).uiWeightedBipredIdc != 0 {
                (*pDqLayer).pPredWeightTable = &mut (*pSh).sPredWeightTable;
            }
        }
        (*pDqLayer).pRefPicBaseMarking = &mut (*pShExt).sRefBasePicMarking;
    }
    (*pDqLayer).uiLayerDqId = pNalHdrExt.uiLayerDqId;
    (*pDqLayer).bUseRefBasePicFlag = pNalHdrExt.bUseRefBasePicFlag;
}

pub unsafe fn WelsDqLayerDecodeStart(
    pCtx: PWelsDecoderContext,
    pCurNal: PNalUnit,
    pSps: PSps,
    pPps: PPps,
) {
    if pCtx.is_null() || pCurNal.is_null() {
        return;
    }
    let pSh = &mut (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
    (*pCtx).eSliceType = (*pSh).eSliceType;
    (*pCtx).pSliceHeader = pSh;
    (*pCtx).bUsedAsRef = false;
    (*pCtx).iFrameNum = (*pSh).iFrameNum;
    UpdateDecoderStatisticsForActiveParaset((*pCtx).pDecoderStatistics, pSps, pPps);
}

pub unsafe fn InitRefPicList(pCtx: PWelsDecoderContext, _kuiNRi: u8, iPoc: i32) -> i32 {
    let mut iRet = ERR_NONE;
    if (*pCtx).eSliceType == B_SLICE {
        iRet = WelsInitBSliceRefList(pCtx, iPoc);
        CreateImplicitWeightTable(pCtx);
    } else {
        iRet = WelsInitRefList(pCtx, iPoc);
    }
    if (*pCtx).eSliceType != I_SLICE && (*pCtx).eSliceType != SI_SLICE {
        if !(*pCtx).pSps.is_null()
            && (*(*pCtx).pSps).uiProfileIdc != 66
            && !(*pCtx).pPps.is_null()
            && (*(*pCtx).pPps).bEntropyCodingModeFlag
        {
            iRet = WelsReorderRefList2(pCtx);
        } else {
            iRet = WelsReorderRefList(pCtx);
        }
    }
    iRet
}

pub unsafe fn InitCurDqLayerData(pCtx: PWelsDecoderContext, pCurDq: PDqLayer) {
    if !pCtx.is_null() && !pCurDq.is_null() {
        (*pCurDq).pMbType = (*pCtx).sMb.pMbType[0];
        (*pCurDq).pSliceIdc = (*pCtx).sMb.pSliceIdc[0];
        (*pCurDq).pMv[LIST_0] = (*pCtx).sMb.pMv[0][LIST_0];
        (*pCurDq).pMv[LIST_1] = (*pCtx).sMb.pMv[0][LIST_1];
        (*pCurDq).pRefIndex[LIST_0] = (*pCtx).sMb.pRefIndex[0][LIST_0];
        (*pCurDq).pRefIndex[LIST_1] = (*pCtx).sMb.pRefIndex[0][LIST_1];
        (*pCurDq).pDirect = (*pCtx).sMb.pDirect[0];
        (*pCurDq).pNoSubMbPartSizeLessThan8x8Flag = (*pCtx).sMb.pNoSubMbPartSizeLessThan8x8Flag[0];
        (*pCurDq).pTransformSize8x8Flag = (*pCtx).sMb.pTransformSize8x8Flag[0];
        (*pCurDq).pLumaQp = (*pCtx).sMb.pLumaQp[0];
        (*pCurDq).pChromaQp = (*pCtx).sMb.pChromaQp[0];
        (*pCurDq).pMvd[LIST_0] = (*pCtx).sMb.pMvd[0][LIST_0];
        (*pCurDq).pMvd[LIST_1] = (*pCtx).sMb.pMvd[0][LIST_1];
        (*pCurDq).pCbfDc = (*pCtx).sMb.pCbfDc[0];
        (*pCurDq).pNzc = (*pCtx).sMb.pNzc[0];
        (*pCurDq).pNzcRs = (*pCtx).sMb.pNzcRs[0];
        (*pCurDq).pScaledTCoeff = (*pCtx).sMb.pScaledTCoeff[0];
        (*pCurDq).pIntraPredMode = (*pCtx).sMb.pIntraPredMode[0];
        (*pCurDq).pIntra4x4FinalMode = (*pCtx).sMb.pIntra4x4FinalMode[0];
        (*pCurDq).pIntraNxNAvailFlag = (*pCtx).sMb.pIntraNxNAvailFlag[0];
        (*pCurDq).pChromaPredMode = (*pCtx).sMb.pChromaPredMode[0];
        (*pCurDq).pCbp = (*pCtx).sMb.pCbp[0];
        (*pCurDq).pSubMbType = (*pCtx).sMb.pSubMbType[0];
        (*pCurDq).pInterPredictionDoneFlag = (*pCtx).sMb.pInterPredictionDoneFlag[0];
        (*pCurDq).pResidualPredFlag = (*pCtx).sMb.pResidualPredFlag[0];
        (*pCurDq).pMbCorrectlyDecodedFlag = (*pCtx).sMb.pMbCorrectlyDecodedFlag[0];
        (*pCurDq).pMbRefConcealedFlag = (*pCtx).sMb.pMbRefConcealedFlag[0];
    }
}

pub unsafe fn DecodeCurrentAccessUnit(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> i32 {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pCurAu = (*pCtx).pAccessUnitList;
    let mut iIdx = (*pCurAu).uiStartPos as i32;
    let iEndIdx = (*pCurAu).uiEndPos as i32;
    let iThreadCount = GetThreadCount(pCtx);
    let mut iRet = ERR_NONE;
    let mut bAllRefComplete = true;

    let kuiTargetLayerDqId = GetTargetDqId((*pCtx).uiTargetDqId, (*pCtx).pParam);
    let kuiDependencyIdMax = (kuiTargetLayerDqId & 0x7F) >> 4;
    let mut iLastIdD: i16 = -1;
    let mut iLastIdQ: i16 = -1;
    (*pCtx).uiNalRefIdc = 0;
    let mut bFreshSliceAvailable = true;

    if (*pCtx).bInitialDqLayersMem || (*pCtx).pCurDqLayer.is_null() {
        (*pCtx).pCurDqLayer = (*pCtx).pDqLayersList[0];
    }
    InitCurDqLayerData(pCtx, (*pCtx).pCurDqLayer);

    let mut pNalCur = (*pCurAu).pNalUnitsList[iIdx as usize];
    (*pCtx).pNalCur = pNalCur;

    while iIdx <= iEndIdx {
        let dq_cur = (*pCtx).pCurDqLayer;
        let mut pLayerInfo = SLayerInfo::default();
        let isNewFrame = (*pCtx).pDec.is_null();

        if (*pCtx).pDec.is_null() {
            (*pCtx).pDec = PrefetchPic((*pCtx).pPicBuff);
            if (*pCtx).pDec.is_null() {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return ERR_INFO_REF_COUNT_OVERFLOW;
            }
            (*(*pCtx).pDec).bNewSeqBegin = (*pCtx).bNewSeqBegin;
        }

        if !pNalCur.is_null() {
            (*(*pCtx).pDec).uiTimeStamp = (*pNalCur).uiTimeStamp;
        }
        (*(*pCtx).pDec).uiDecodingTimeStamp = (*pCtx).uiDecodingTimeStamp;

        if (*pCtx).iTotalNumMbRec == 0 {
            if !(*pCtx).pSps.is_null() {
                (*(*pCtx).pDec).iMbNum = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;
            }
            (*(*pCtx).pDec).iMbEcedNum = 0;
            (*(*pCtx).pDec).iMbEcedPropNum = 0;
        }

        (*pCtx).bRPLRError = false;
        if !(*pCtx).pDec.is_null() {
            GetI4LumaIChromaAddrTable(
                (*pCtx).iDecBlockOffsetArray.as_mut_ptr(),
                (*(*pCtx).pDec).iLinesize[0],
                (*(*pCtx).pDec).iLinesize[1],
            );
        }

        if !pNalCur.is_null() && (*pNalCur).sNalHeaderExt.uiLayerDqId > kuiTargetLayerDqId {
            break;
        }

        while iIdx <= iEndIdx {
            if pNalCur.is_null() {
                break;
            }
            let iCurrIdQ = (*pNalCur).sNalHeaderExt.uiQualityId as i16;
            let iCurrIdD = (*pNalCur).sNalHeaderExt.uiDependencyId as i16;
            let pSh = &mut (*pNalCur).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
            let pShExt = &mut (*pNalCur).sNalData.sVclNal.sSliceHeaderExt;
            (*pCtx).bRPLRError = false;
            let bReconstructSlice = CheckSliceNeedReconstruct((*pNalCur).sNalHeaderExt.uiLayerDqId, kuiTargetLayerDqId);

            pLayerInfo.sNalHeaderExt = (*pNalCur).sNalHeaderExt;
            if !(*pCtx).pDec.is_null() {
                (*(*pCtx).pDec).iFrameNum = (*pSh).iFrameNum;
                (*(*pCtx).pDec).iFramePoc = (*pSh).iPicOrderCntLsb;
                (*(*pCtx).pDec).bIdrFlag = (*pNalCur).sNalHeaderExt.bIdrFlag;
                (*(*pCtx).pDec).eSliceType = (*pSh).eSliceType;
            }

            pLayerInfo.sSliceInLayer.sSliceHeaderExt = *pShExt;
            pLayerInfo.sSliceInLayer.bSliceHeaderExtFlag = (*pNalCur).sNalData.sVclNal.bSliceHeaderExtFlag;
            pLayerInfo.sSliceInLayer.eSliceType = (*pSh).eSliceType;
            pLayerInfo.sSliceInLayer.iLastMbQp = (*pSh).iSliceQp;
            (*dq_cur).pBitStringAux = &mut (*pNalCur).sNalData.sVclNal.sSliceBitsRead;

            (*pCtx).uiNalRefIdc = (*pNalCur).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc;
            let iPpsId = (*pSh).iPpsId;
            pLayerInfo.pPps = (*pSh).pPps;
            pLayerInfo.pSps = (*pSh).pSps;
            pLayerInfo.pSubsetSps = (*pShExt).pSubsetSps;

            bFreshSliceAvailable = iCurrIdD != iLastIdD || iCurrIdQ != iLastIdQ;
            WelsDqLayerDecodeStart(pCtx, pNalCur, pLayerInfo.pSps, pLayerInfo.pPps);

            if iLastIdD < 0 || iLastIdD == iCurrIdD {
                InitDqLayerInfo(dq_cur, &mut pLayerInfo, pNalCur, (*pCtx).pDec);

                if iCurrIdD == (kuiDependencyIdMax as i16) && iCurrIdQ == (BASE_QUALITY_ID as i16) && isNewFrame {
                    iRet = InitRefPicList(pCtx, (*pCtx).uiNalRefIdc, (*pSh).iPicOrderCntLsb);
                    if iRet != ERR_NONE {
                        (*pCtx).bRPLRError = true;
                        bAllRefComplete = false;
                        HandleReferenceLost(pCtx, pNalCur);
                        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                            if (*pCtx).iTotalNumMbRec == 0 {
                                (*pCtx).pDec = std::ptr::null_mut();
                            }
                            return iRet;
                        }
                    }
                }

                if (*pSh).eSliceType == B_SLICE && (*pSh).iDirectSpatialMvPredFlag == 0 {
                    ComputeColocatedTemporalScaling(pCtx);
                }

                if iThreadCount > 1 {
                    iRet = WelsDecodeAndConstructSlice(pCtx);
                } else {
                    iRet = WelsDecodeSlice(pCtx, bFreshSliceAvailable, pNalCur);
                }

                if iRet != ERR_NONE {
                    bAllRefComplete = false;
                    HandleReferenceLostL0(pCtx, pNalCur);
                    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                        if (*pCtx).iTotalNumMbRec == 0 {
                            (*pCtx).pDec = std::ptr::null_mut();
                        }
                        return iRet;
                    }
                }

                if iThreadCount <= 1 && bReconstructSlice {
                    iRet = WelsDecodeConstructSlice(pCtx, pNalCur);
                    if iRet != ERR_NONE {
                        if !(*pCtx).pDec.is_null() {
                            (*(*pCtx).pDec).bIsComplete = false;
                        }
                        return iRet;
                    }
                }

                if bAllRefComplete && (*pCtx).eSliceType != I_SLICE {
                    if iThreadCount <= 1 {
                        if (*pCtx).sRefPic.uiRefCount[LIST_0] > 0 {
                            bAllRefComplete = bAllRefComplete && CheckRefPicturesComplete(pCtx);
                        } else {
                            bAllRefComplete = false;
                        }
                    }
                }
            }

            iLastIdD = iCurrIdD;
            iLastIdQ = iCurrIdQ;

            iIdx += 1;
            if iIdx <= iEndIdx {
                pNalCur = (*pCurAu).pNalUnitsList[iIdx as usize];
            } else {
                pNalCur = std::ptr::null_mut();
            }

            if pNalCur.is_null()
                || iLastIdD != ((*pNalCur).sNalHeaderExt.uiDependencyId as i16)
                || iLastIdQ != ((*pNalCur).sNalHeaderExt.uiQualityId as i16)
            {
                break;
            }
        }

        if !(*pCtx).pDec.is_null() {
            (*(*pCtx).pDec).bIsComplete = bAllRefComplete;
            if !(*(*pCtx).pDec).bIsComplete {
                (*pCtx).iErrorCode |= dsDataErrorConcealed;
            }
        }

        if (*dq_cur).uiLayerDqId == kuiTargetLayerDqId {
            if !(*pCtx).bInstantDecFlag {
                if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly {
                    if NeedErrorCon(pCtx) && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
                        ImplementErrorCon(pCtx);
                        if !(*pCtx).pSps.is_null() {
                            (*pCtx).iTotalNumMbRec = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;
                            if !(*pCtx).pDec.is_null() {
                                (*(*pCtx).pDec).iSpsId = (*(*pCtx).pSps).iSpsId;
                            }
                        }
                        if !(*pCtx).pPps.is_null() && !(*pCtx).pDec.is_null() {
                            (*(*pCtx).pDec).iPpsId = (*(*pCtx).pPps).iPpsId;
                        }
                    }
                }
            }

            iRet = DecodeFrameConstruction(pCtx, ppDst, pDstInfo);
            if iRet != ERR_NONE {
                return iRet;
            }

            if !(*pCtx).pLastDecPicInfo.is_null() {
                (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb = (*pCtx).pDec;
            }
            (*pCtx).bUsedAsRef = (*pCtx).uiNalRefIdc > 0;
            if iThreadCount <= 1 {
                if (*pCtx).bUsedAsRef {
                    iRet = WelsMarkAsRef(pCtx);
                    if iRet != ERR_NONE {
                        if iRet == ERR_INFO_DUPLICATE_FRAME_NUM {
                            (*pCtx).iErrorCode |= dsBitstreamError;
                        }
                        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
                            (*pCtx).pDec = std::ptr::null_mut();
                            return iRet;
                        }
                    }
                    if !(*pCtx).pParam.is_null() && !(*(*pCtx).pParam).bParseOnly && !(*pCtx).pDec.is_null() {
                        ExpandReferencingPicture(
                            (*(*pCtx).pDec).pData,
                            (*(*pCtx).pDec).iWidthInPixel,
                            (*(*pCtx).pDec).iHeightInPixel,
                            (*(*pCtx).pDec).iLinesize,
                            (*pCtx).sExpandPicFunc.pfExpandLumaPicture,
                            (*pCtx).sExpandPicFunc.pfExpandChromaPicture,
                        );
                    }
                }
            }
            (*pCtx).pDec = std::ptr::null_mut();
        }
    }
    ERR_NONE
}

pub unsafe fn CheckAndFinishLastPic(
    pCtx: PWelsDecoderContext,
    ppDst: *mut *mut u8,
    pDstInfo: *mut SBufferInfo,
) -> bool {
    if pCtx.is_null() || (*pCtx).pAccessUnitList.is_null() {
        return false;
    }
    let pAu = (*pCtx).pAccessUnitList;
    let mut bAuBoundaryFlag = false;

    if IS_VCL_NAL((*pCtx).sCurNalHead.eNalUnitType, 1) {
        let pCurNal = (*pAu).pNalUnitsList[(*pAu).uiEndPos as usize];
        if !pCurNal.is_null() && !(*pCtx).pLastDecPicInfo.is_null() {
            bAuBoundaryFlag = (*pCtx).iTotalNumMbRec != 0
                && CheckAccessUnitBoundaryExt(
                    &mut (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt,
                    &mut (*pCurNal).sNalHeaderExt,
                    &mut (*(*pCtx).pLastDecPicInfo).sLastSliceHeader,
                    &mut (*pCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader,
                );
        }
    } else {
        if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_AU_DELIMITER
            || (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SEI
        {
            bAuBoundaryFlag = true;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SPS) != 0;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_SUBSET_SPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_SUBSETSPS) != 0;
        } else if (*pCtx).sCurNalHead.eNalUnitType == NAL_UNIT_PPS {
            bAuBoundaryFlag = ((*pCtx).sSpsPpsCtx.iOverwriteFlags & OVERWRITE_PPS) != 0;
        }
        if bAuBoundaryFlag && (*pAu).uiAvailUnitsNum != 0 {
            ConstructAccessUnit(pCtx, ppDst, pDstInfo);
        }
    }

    if bAuBoundaryFlag && (*pCtx).iTotalNumMbRec != 0 && NeedErrorCon(pCtx) {
        if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
            ImplementErrorCon(pCtx);
            if !(*pCtx).pSps.is_null() {
                (*pCtx).iTotalNumMbRec = ((*(*pCtx).pSps).iMbWidth * (*(*pCtx).pSps).iMbHeight) as i32;
                if !(*pCtx).pDec.is_null() {
                    (*(*pCtx).pDec).iSpsId = (*(*pCtx).pSps).iSpsId;
                }
            }
            if !(*pCtx).pPps.is_null() && !(*pCtx).pDec.is_null() {
                (*(*pCtx).pDec).iPpsId = (*(*pCtx).pPps).iPpsId;
            }
            DecodeFrameConstruction(pCtx, ppDst, pDstInfo);
            if !(*pCtx).pLastDecPicInfo.is_null() {
                (*(*pCtx).pLastDecPicInfo).pPreviousDecodedPictureInDpb = (*pCtx).pDec;
                if (*(*pCtx).pLastDecPicInfo).sLastNalHdrExt.sNalUnitHeader.uiNalRefIdc > 0 {
                    if MarkECFrameAsRef(pCtx) == ERR_INFO_INVALID_PTR {
                        (*pCtx).iErrorCode |= dsRefListNullPtrs;
                        return false;
                    }
                }
            }
        } else if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).bParseOnly {
            if !(*pCtx).pParserBsInfo.is_null() {
                (*(*pCtx).pParserBsInfo).iNalNum = 0;
            }
            (*pCtx).bFrameFinish = true;
        } else {
            if DecodeFrameConstruction(pCtx, ppDst, pDstInfo) != ERR_NONE {
                (*pCtx).pDec = std::ptr::null_mut();
                return false;
            }
        }
        (*pCtx).pDec = std::ptr::null_mut();
    }
    true
}

pub unsafe fn CheckRefPicturesComplete(pCtx: PWelsDecoderContext) -> bool {
    if pCtx.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return true;
    }
    let pCurDqLayer = (*pCtx).pCurDqLayer;
    let pDec = (*pCurDqLayer).pDec;
    if pDec.is_null() || (*pDec).pMbType.is_null() {
        return true;
    }
    let mut bAllRefComplete = true;
    let mut iRealMbIdx = (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice;
    let totalMb = (*pCurDqLayer).sLayerInfo.sSliceInLayer.iTotalMbInCurSlice;

    for iMbIdx in 0..totalMb {
        let mbType = *(*pDec).pMbType.add(iRealMbIdx as usize);
        match mbType {
            MB_TYPE_SKIP | MB_TYPE_16x16 => {
                let refIdx = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16) as usize;
                if refIdx < MAX_REF_PIC_COUNT {
                    let pRef = (*pCtx).sRefPic.pRefList[LIST_0][refIdx];
                    if !pRef.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef).bIsComplete;
                    }
                }
            }
            MB_TYPE_16x8 => {
                let refIdx0 = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16) as usize;
                let refIdx1 = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16 + 8) as usize;
                if refIdx0 < MAX_REF_PIC_COUNT {
                    let pRef0 = (*pCtx).sRefPic.pRefList[LIST_0][refIdx0];
                    if !pRef0.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef0).bIsComplete;
                    }
                }
                if refIdx1 < MAX_REF_PIC_COUNT {
                    let pRef1 = (*pCtx).sRefPic.pRefList[LIST_0][refIdx1];
                    if !pRef1.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef1).bIsComplete;
                    }
                }
            }
            MB_TYPE_8x16 => {
                let refIdx0 = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16) as usize;
                let refIdx1 = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16 + 2) as usize;
                if refIdx0 < MAX_REF_PIC_COUNT {
                    let pRef0 = (*pCtx).sRefPic.pRefList[LIST_0][refIdx0];
                    if !pRef0.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef0).bIsComplete;
                    }
                }
                if refIdx1 < MAX_REF_PIC_COUNT {
                    let pRef1 = (*pCtx).sRefPic.pRefList[LIST_0][refIdx1];
                    if !pRef1.is_null() {
                        bAllRefComplete = bAllRefComplete && (*pRef1).bIsComplete;
                    }
                }
            }
            MB_TYPE_8x8 | MB_TYPE_8x8_REF0 => {
                let indices = [0, 2, 8, 10];
                for &sub in &indices {
                    let refIdx = *(*pCurDqLayer).pRefIndex[0].add(iRealMbIdx as usize * 16 + sub) as usize;
                    if refIdx < MAX_REF_PIC_COUNT {
                        let pRef = (*pCtx).sRefPic.pRefList[LIST_0][refIdx];
                        if !pRef.is_null() {
                            bAllRefComplete = bAllRefComplete && (*pRef).bIsComplete;
                        }
                    }
                }
            }
            _ => {}
        }
        if !bAllRefComplete {
            break;
        }
        iRealMbIdx = if !(*pCtx).pPps.is_null() && (*(*pCtx).pPps).uiNumSliceGroups > 1 {
            FmoNextMb((*pCtx).pFmo, iRealMbIdx)
        } else {
            (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice + iMbIdx + 1
        };
        if iRealMbIdx == -1 {
            return false;
        }
    }
    bAllRefComplete
}
