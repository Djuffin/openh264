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
pub const LAYER_NUM_EXCHANGEABLE: usize = 1;
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
pub const dsOutOfMemory: i32 = 0x4000;

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

pub use crate::decoder::error_concealment::{ERROR_CON_IDC, ERROR_CON_IDC::*};


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

pub use crate::decoder::slice::EWelsSliceType;
pub use crate::decoder::slice::EWelsSliceType::*;



pub use crate::decoder::nalu::EWelsNalUnitType;
pub use crate::decoder::nalu::EWelsNalUnitType::*;


// Data Structures Matching C/C++ Layout

pub use crate::decoder::decoder_context::SPosOffset;


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

pub use crate::decoder::decoder_context::SParserBsInfo;


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
    pub uiAspectRatioIdc: u8,
    pub uiSarWidth: u32,
    pub uiSarHeight: u32,
    pub bOverscanInfoPresentFlag: bool,
    pub bOverscanAppropriateFlag: bool,
    pub bVideoSignalTypePresentFlag: bool,
    pub uiVideoFormat: u8,
    pub bVideoFullRangeFlag: bool,
    pub bColourDescriptionPresentFlag: bool,
    pub bColourDescripPresentFlag: bool,
    pub uiColourPrimaries: u8,
    pub uiTransferCharacteristics: u8,
    pub uiMatrixCoefficients: u8,
    pub uiMatrixCoeffs: u8,
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

pub use crate::decoder::parameter_sets::SLevelLimits;


pub use crate::decoder::parameter_sets::{SSps, SPps, SSubsetSps, SSpsSvcExt};
pub use crate::decoder::decoder_context::{SWelsDecoderSpsPpsCTX as SSpsPpsCtx};


pub use crate::decoder::slice::{SPredWeightTable, SPredList};



pub use crate::decoder::slice::{SRefPicListReorderSyn, SRefPicMarking, SReorderingSyntax, SRefBasePicMarking};


pub use crate::decoder::bit_stream::SBitStringAux;
pub use crate::decoder::decoder_context::{SNalUnitHeader, SNalUnitHeaderExt};
pub use crate::decoder::slice::{SSliceHeader, SSliceHeaderExt, SSlice, PSlice};



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



pub use crate::decoder::nalu::{SAccessUnit, PAccessUnit};


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
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
    pub pBitStringAux: *mut SBitStringAux,
    pub pFmo: *mut crate::decoder::fmo::TagFmo,
    pub pMbType: *mut u32,
    pub pSliceIdc: *mut i32,
    pub pMv: [*mut i16; LIST_A],
    pub pMvd: [*mut [[i16; 2]; 16]; LIST_A],
    pub pRefIndex: [*mut i8; LIST_A],
    pub pDirect: *mut i8,
    pub pNoSubMbPartSizeLessThan8x8Flag: *mut bool,
    pub pTransformSize8x8Flag: *mut bool,
    pub pLumaQp: *mut i8,
    pub pChromaQp: *mut [i8; 2],
    pub pCbp: *mut i8,
    pub pCbfDc: *mut u16,
    pub pNzc: *mut [i8; 24],
    pub pNzcRs: *mut [i8; 24],
    pub pResidualPredFlag: *mut i8,
    pub pInterPredictionDoneFlag: *mut i8,
    pub pMbCorrectlyDecodedFlag: *mut bool,
    pub pMbRefConcealedFlag: *mut bool,
    pub pScaledTCoeff: *mut [i16; 384],
    pub pIntraPredMode: *mut i8,
    pub pIntra4x4FinalMode: *mut i8,
    pub pIntraNxNAvailFlag: *mut u8,
    pub pChromaPredMode: *mut i8,
    pub pSubMbType: *mut [u32; 4],
    pub iLumaStride: i32,
    pub iChromaStride: i32,
    pub pPred: [*mut u8; 3],
    pub iMbX: i32,
    pub iMbY: i32,
    pub iMbXyIndex: i32,
    pub iMbWidth: i32,
    pub iMbHeight: i32,

    /* Common syntax elements across all slices of a DQLayer */
    pub iSliceIdcBackup: i32,
    pub uiSpsId: u32,
    pub uiPpsId: u32,
    pub uiDisableInterLayerDeblockingFilterIdc: u32,
    pub iInterLayerSliceAlphaC0Offset: i32,
    pub iInterLayerSliceBetaOffset: i32,
    pub iSliceGroupChangeCycle: i32,

    pub pRefPicListReordering: *mut SRefPicListReorderSyn,
    pub pPredWeightTable: *mut SPredWeightTable,
    pub pRefPicMarking: *mut SRefPicMarking,
    pub pRefPicBaseMarking: *mut SRefBasePicMarking,

    pub pRef: *mut Picture,
    pub pDec: *mut Picture,

    pub iColocMv: [[[i16; 2]; 16]; 2],
    pub iColocRefIndex: [[i8; 16]; 2],
    pub iColocIntra: [i8; 16],

    pub bUseWeightPredictionFlag: bool,
    pub bUseWeightedBiPredIdc: bool,
    pub bStoreRefBasePicFlag: bool,
    pub bTCoeffLevelPredFlag: bool,
    pub bConstrainedIntraResamplingFlag: bool,
    pub uiRefLayerDqId: u8,
    pub uiRefLayerChromaPhaseXPlus1Flag: u8,
    pub uiRefLayerChromaPhaseYPlus1: u8,
    pub uiLayerDqId: u8,
    pub bUseRefBasePicFlag: bool,
}

impl Default for SDqLayer {
    fn default() -> Self {
        let mut layer: Self = unsafe { std::mem::zeroed() };
        layer.uiRefLayerDqId = 255;
        layer.uiRefLayerChromaPhaseYPlus1 = 1;
        layer
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

pub use crate::decoder::decoder_context::{SRefPic, PRefPic};

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

pub use crate::api::codec_api::SBufferInfo;

pub use crate::decoder::decoder_context::SDecoderStatistics;


pub use crate::decoder::decoder_context::{SDecodingParam, SLogContext};


pub use crate::decoder::decoder_context::SWelsCabacDecEngine;


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
    pub pfExpandLumaPicture: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
    pub pfExpandChromaPicture: [Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>; 2],
}

impl Default for SExpandPicFunc {
    fn default() -> Self {
        Self {
            pfExpandLumaPicture: None,
            pfExpandChromaPicture: [None, None],
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

pub use crate::decoder::decoder_context::{SWelsDecoderContext, PWelsDecoderContext};

pub use crate::decoder::nalu::{SNalUnit, PNalUnit};

pub type PDqLayer = *mut SDqLayer;

pub use crate::decoder::decoder_context::{Picture, SPicture, PPicture, SPicBuff};

pub use crate::decoder::parameter_sets::{PSps, PPps, PSubsetSps};

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
    crate::decoder::dec_golomb::BsGetBits(pBs, n as i32, pOut)
}

#[inline]
pub unsafe fn BsGetOneBit(pBs: *mut SBitStringAux, pOut: *mut u32) -> i32 {
    crate::decoder::dec_golomb::BsGetBits(pBs, 1, pOut)
}

#[inline]
pub unsafe fn BsGetUe(pBs: *mut SBitStringAux, pOut: *mut u32) -> i32 {
    crate::decoder::dec_golomb::BsGetUe(pBs, pOut) as i32
}

#[inline]
pub unsafe fn BsGetSe(pBs: *mut SBitStringAux, pOut: *mut i32) -> i32 {
    crate::decoder::dec_golomb::BsGetSe(pBs, pOut)
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

unsafe fn ResetDecStatNums(pDecStat: *mut SDecoderStatistics) {
    if pDecStat.is_null() {
        return;
    }
    let width = (*pDecStat).uiWidth;
    let height = (*pDecStat).uiHeight;
    let avg_luma_qp = (*pDecStat).iAvgLumaQp;
    let profile = (*pDecStat).uiProfile;
    let level = (*pDecStat).uiLevel;
    *pDecStat = SDecoderStatistics::default();
    (*pDecStat).uiWidth = width;
    (*pDecStat).uiHeight = height;
    (*pDecStat).iAvgLumaQp = avg_luma_qp;
    (*pDecStat).uiProfile = profile;
    (*pDecStat).uiLevel = level;
}

unsafe fn UpdateDecStatFreezingInfo(idr_flag: bool, pDecStat: *mut SDecoderStatistics) {
    if pDecStat.is_null() {
        return;
    }
    if idr_flag {
        (*pDecStat).uiFreezingIDRNum += 1;
    } else {
        (*pDecStat).uiFreezingNonIDRNum += 1;
    }
}

#[inline]
pub unsafe fn UpdateDecStatNoFreezingInfo(pCtx: PWelsDecoderContext) {
    if pCtx.is_null()
        || (*pCtx).pCurDqLayer.is_null()
        || (*pCtx).pDec.is_null()
        || (*pCtx).pDecoderStatistics.is_null()
    {
        return;
    }
    let pCurDq = (*pCtx).pCurDqLayer;
    let pPic = (*pCtx).pDec;
    let pDecStat = (*pCtx).pDecoderStatistics;

    if (*pDecStat).iAvgLumaQp == -1 {
        (*pDecStat).iAvgLumaQp = 0;
    }

    let mut iTotalQp = 0i64;
    let kiMbNum = ((*pCurDq).iMbWidth * (*pCurDq).iMbHeight) as usize;
    if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc == ERROR_CON_DISABLE {
        for iMb in 0..kiMbNum {
            iTotalQp += *(*pCurDq).pLumaQp.add(iMb) as i64;
        }
        if kiMbNum > 0 {
            iTotalQp /= kiMbNum as i64;
        }
    } else {
        let mut iCorrectMbNum = 0i64;
        for iMb in 0..kiMbNum {
            let correct = if !(*pCurDq).pMbCorrectlyDecodedFlag.is_null()
                && *(*pCurDq).pMbCorrectlyDecodedFlag.add(iMb)
            {
                1i64
            } else {
                0i64
            };
            iCorrectMbNum += correct;
            iTotalQp += (*(*pCurDq).pLumaQp.add(iMb) as i64) * correct;
        }
        if iCorrectMbNum == 0 {
            iTotalQp = (*pDecStat).iAvgLumaQp as i64;
        } else {
            iTotalQp /= iCorrectMbNum;
        }
    }

    if (*pDecStat).uiDecodedFrameCount == u32::MAX {
        ResetDecStatNums(pDecStat);
        (*pDecStat).iAvgLumaQp = iTotalQp as i32;
    } else {
        let count = (*pDecStat).uiDecodedFrameCount as i64;
        (*pDecStat).iAvgLumaQp =
            (((*pDecStat).iAvgLumaQp as i64 * count + iTotalQp) / (count + 1)) as i32;
    }

    if (*pCurDq).sLayerInfo.sNalHeaderExt.bIdrFlag {
        if (*pPic).bIsComplete {
            (*pDecStat).uiIDRCorrectNum += 1;
        } else if !(*pCtx).pParam.is_null() && (*(*pCtx).pParam).eEcActiveIdc != ERROR_CON_DISABLE {
            (*pDecStat).uiEcIDRNum += 1;
        }
    }
}

#[inline]
pub unsafe fn UpdateDecStat(pCtx: PWelsDecoderContext, bOutput: bool) {
    if pCtx.is_null() {
        return;
    }
    if (*pCtx).bFreezeOutput {
        if !(*pCtx).pCurDqLayer.is_null() {
            UpdateDecStatFreezingInfo(
                (*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt.bIdrFlag,
                (*pCtx).pDecoderStatistics,
            );
        }
    } else if bOutput {
        UpdateDecStatNoFreezingInfo(pCtx);
    }
}

#[inline]
pub unsafe fn WelsTargetSliceConstruction(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::decode_slice::WelsTargetSliceConstruction(pCtx)
}

#[inline]
pub unsafe fn WelsDecodeSlice(pCtx: PWelsDecoderContext, bFreshSlice: bool, pCurNal: PNalUnit) -> i32 {
    crate::decoder::decode_slice::WelsDecodeSlice(pCtx, bFreshSlice, pCurNal)
}

#[inline]
pub unsafe fn WelsDecodeAndConstructSlice(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::decode_slice::WelsDecodeAndConstructSlice(pCtx)
}

#[inline]
pub unsafe fn WelsInitRefList(pCtx: PWelsDecoderContext, iPoc: i32) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitRefList(pCtx, iPoc)
}

#[inline]
pub unsafe fn WelsInitBSliceRefList(pCtx: PWelsDecoderContext, iPoc: i32) -> i32 {
    crate::decoder::manage_dec_ref::WelsInitBSliceRefList(pCtx, iPoc)
}

#[inline]
pub unsafe fn WelsReorderRefList(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::manage_dec_ref::WelsReorderRefList(pCtx)
}

#[inline]
pub unsafe fn WelsReorderRefList2(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::manage_dec_ref::WelsReorderRefList2(pCtx)
}

#[inline]
pub unsafe fn WelsMarkAsRef(pCtx: PWelsDecoderContext) -> i32 {
    crate::decoder::manage_dec_ref::WelsMarkAsRef(pCtx, std::ptr::null_mut())
}

#[inline]
pub unsafe fn ExpandReferencingPicture(
    pData: [*mut u8; 4],
    iWidth: i32,
    iHeight: i32,
    iStride: [i32; 4],
    pfExpandLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>,
    pfExpandChroma: [Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>; 2],
) {
    crate::decoder::error_concealment::ExpandReferencingPicture(
        pData,
        iWidth,
        iHeight,
        iStride,
        std::mem::transmute(pfExpandLuma),
        std::mem::transmute(pfExpandChroma),
    );
}

#[inline]
pub unsafe fn GetI4LumaIChromaAddrTable(pBlockOffset: *mut i32, iStrideY: i32, iStrideUV: i32) {
    crate::decoder::decode_mb_aux::GetI4LumaIChromaAddrTable(pBlockOffset, iStrideY, iStrideUV);
}

#[inline]
pub unsafe fn ComputeColocatedTemporalScaling(pCtx: PWelsDecoderContext) {
    let _ = crate::decoder::decode_slice::ComputeColocatedTemporalScaling(pCtx);
}

pub unsafe fn SyncPictureResolutionExt(pCtx: PWelsDecoderContext, iWidth: u32, iHeight: u32) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let iPicWidth = (iWidth << 4) as i32;
    let iPicHeight = (iHeight << 4) as i32;
    let iPicBufSize = 16;

    if (*pCtx).pPicBuff.is_null() {
        let iErr = crate::decoder::pic_queue::CreatePicBuff(pCtx, &mut (*pCtx).pPicBuff, iPicBufSize, iPicWidth, iPicHeight);
        if iErr != 0 {
            return iErr;
        }
    }
    let iErr = InitialDqLayersContext(pCtx, iPicWidth, iPicHeight);
    if iErr != ERR_NONE {
        return iErr;
    }
    ERR_NONE
}

#[inline]
pub unsafe fn WelsResetRefPic(pCtx: PWelsDecoderContext) {
    crate::decoder::manage_dec_ref::WelsResetRefPic(pCtx)
}

pub use crate::decoder::pic_queue::{PrefetchPic, PrefetchLastPicForThread};

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
    (*pAu).pNalUnitsList = WelsMalloczHelper(pMa, std::mem::size_of::<PNalUnit>() * MAX_NAL_UNIT_NUM_IN_AU) as *mut PNalUnit;
    if (*pAu).pNalUnitsList.is_null() {
        WelsFreeHelper(pMa, pAu as *mut u8, std::mem::size_of::<SAccessUnit>());
        return ERR_INFO_OUT_OF_MEMORY;
    }
    for i in 0..MAX_NAL_UNIT_NUM_IN_AU {
        let pNal = WelsMalloczHelper(pMa, std::mem::size_of::<SNalUnit>()) as PNalUnit;
        if pNal.is_null() {
            return ERR_INFO_OUT_OF_MEMORY;
        }
        *(*pAu).pNalUnitsList.add(i) = pNal;
    }
    (*pAu).uiCountUnitsNum = MAX_NAL_UNIT_NUM_IN_AU as u32;
    (*pAu).uiAvailUnitsNum = 0;
    *ppNalList = pAu;
    ERR_NONE
}

#[inline]
pub unsafe fn MemFreeNalList(ppNalList: *mut PAccessUnit, pMa: *mut CMemoryAlign) {
    if ppNalList.is_null() || (*ppNalList).is_null() {
        return;
    }
    let pAu = *ppNalList;
    if !(*pAu).pNalUnitsList.is_null() {
        for i in 0..MAX_NAL_UNIT_NUM_IN_AU {
            let pNal = *(*pAu).pNalUnitsList.add(i);
            if !pNal.is_null() {
                WelsFreeHelper(pMa, pNal as *mut u8, std::mem::size_of::<SNalUnit>());
            }
        }
        WelsFreeHelper(pMa, (*pAu).pNalUnitsList as *mut u8, std::mem::size_of::<PNalUnit>() * MAX_NAL_UNIT_NUM_IN_AU);
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

    if (*pCtx).bNewSeqBegin {
        let pSps = (*pCurDq).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
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
                if !(*(*pCurAu).pNalUnitsList.add(iIdx as usize)).is_null() {
                    (*pParser).uiOutBsTimeStamp = (*(*(*pCurAu).pNalUnitsList.add(iIdx as usize))).uiTimeStamp;
                }
                if !(*pCtx).pSps.is_null() {
                    let pSps = (*pCtx).pSps as *mut SSps;
                    (*pParser).iSpsWidthInPixel = ((*pSps).iMbWidth as i32) * 16
                        - (((*pSps).sFrameCrop.iLeftOffset
                            + (*pSps).sFrameCrop.iRightOffset)
                            << 1);
                    (*pParser).iSpsHeightInPixel = ((*pSps).iMbHeight as i32) * 16
                        - (((*pSps).sFrameCrop.iTopOffset
                            + (*pSps).sFrameCrop.iBottomOffset)
                            << 1);
                }

                while iIdx <= iEndIdx {
                    let pCurNal = *(*pCurAu).pNalUnitsList.add(iIdx as usize);
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

    let pSps = (*pSh).pSps as *mut SSps;

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
    let pPps = (*pSliceHeader).pPps as *mut SPps;
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
                pRefPicListReordering.sReorderingSyn[iList][iIdx].uiReorderingOfPicNumsIdc = kuiIdc as _;

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
                    if uiCode >= (1u32 << (*(pSps as *mut SSps)).uiLog2MaxFrameNum) {
                        return GENERATE_ERROR_NO(ERR_LEVEL_SLICE_HEADER, ERR_INFO_INVALID_REF_REORDERING);
                    }
                    pRefPicListReordering.sReorderingSyn[iList][iIdx].uiAbsDiffPicNumMinus1 = uiCode;
                } else if kuiIdc == 2 {
                    if BsGetUe(pBs, &mut uiCode) != ERR_NONE {
                        return ERR_INFO_INVALID_ACCESS;
                    }
                    pRefPicListReordering.sReorderingSyn[iList][iIdx].uiLongTermPicNum = uiCode as u16;

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
    if (*pNalExt).bNoInterLayerPredFlag || (*pNalExt).uiQualityId > 0 {

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
        for i in 0..=(*(*pCtx).pAccessUnitList).uiAvailUnitsNum as usize {
            let pNal = *(*(*pCtx).pAccessUnitList).pNalUnitsList.add(i);
            if i < MAX_NAL_UNIT_NUM_IN_AU && !pNal.is_null() {
                let pSliceBitsRead = &mut (*pNal).sNalData.sVclNal.sSliceBitsRead;

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

pub unsafe fn WelsInitDecoderFuncs(pCtx: PWelsDecoderContext) {
    if pCtx.is_null() {
        return;
    }
    let cpu_flag = (*pCtx).uiCpuFlag;

    // 1. Deblocking Filter
    crate::common::deblocking_common::DeblockingInit(&mut (*pCtx).sDeblockingFunc as *mut _ as *mut _, cpu_flag as i32);

    // 2. Motion Compensation
    crate::common::mc::InitMcFunc(&mut (*pCtx).sMcFunc as *mut _ as *mut _, cpu_flag);

    // 3. IDCT Inverse Transform
    (*pCtx).pIdctResAddPredFunc = Some(crate::decoder::decode_mb_aux::IdctResAddPred_c);
    (*pCtx).pIdctResAddPredFunc8x8 = Some(crate::decoder::decode_mb_aux::IdctResAddPred8x8_c);
    (*pCtx).pIdctFourResAddPredFunc = Some(crate::decoder::decode_mb_aux::IdctFourResAddPred_c);

    // 4. Intra Prediction
    (*pCtx).pGetI4x4LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDL_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDR_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVR_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredHD_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVL_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredHU_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDcNA_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredDDLTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI4x4LumaPredVLTop_c),
    ];

    (*pCtx).pGetI16x16LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredPlane_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI16x16LumaPredDcNA_c),
    ];

    (*pCtx).pGetIChromaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredPlane_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsIChromaPredDcNA_c),
    ];

    (*pCtx).pGetI8x8LumaPredFunc = [
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredV_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredH_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDc_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDL_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDR_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVR_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredHD_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVL_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredHU_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcLeft_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDcNA_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredDDLTop_c),
        Some(crate::decoder::get_intra_predictor::WelsI8x8LumaPredVLTop_c),
    ];
}

pub unsafe fn WelsInitStaticMemory(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    WelsInitDecoderFuncs(pCtx);
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
    pHeaderExt.bNoInterLayerPredFlag = (uiCurByte >> 7) != 0;
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
    let pSps = pSps as *mut SSps;
    let pPps = pPps as *mut SPps;
    (*pDecoderStatistics).iCurrentActiveSpsId = (*pSps).iSpsId;
    (*pDecoderStatistics).iCurrentActivePpsId = (*pPps).iPpsId;
    (*pDecoderStatistics).uiProfile = (*pSps).uiProfileIdc as u32;
    (*pDecoderStatistics).uiLevel = (*pSps).uiLevelIdc as u32;
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
    let kpCurNal = *(*pCurAu).pNalUnitsList.add(((*pCurAu).uiAvailUnitsNum - 1) as usize);
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
    (*pSliceHead).pPps = pPps as *mut SPps as *mut c_void;
    (*pSliceHead).pSps = pSps as *mut SSps as *mut c_void;


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
        (*pSliceHead).uiIdrPicId = uiCode as u16;
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
    pNalHdrExtD.bNoInterLayerPredFlag = pNalHdrExtS.bNoInterLayerPredFlag;
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
    if iIdx < MAX_NAL_UNIT_NUM_IN_AU && !(*(*pCurAu).pNalUnitsList.add(iIdx)).is_null() {
        (*pCtx).uiTargetDqId = (*(*(*pCurAu).pNalUnitsList.add(iIdx))).sNalHeaderExt.uiLayerDqId;
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
    (*pCtx).sMb.iMbWidth = ((kiMaxWidth + 15) >> 4) as u32;
    (*pCtx).sMb.iMbHeight = ((kiMaxHeight + 15) >> 4) as u32;

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

        (*pCtx).sMb.pMbType[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u32>()) as *mut _;
        (*pCtx).sMb.pMv[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut _;
        (*pCtx).sMb.pMv[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut _;
        (*pCtx).sMb.pRefIndex[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pRefIndex[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pDirect[i] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pLumaQp[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pChromaQp[i] = WelsMalloczHelper(pMa, numMb * 2 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pMvd[i][LIST_0] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut _;
        (*pCtx).sMb.pMvd[i][LIST_1] = WelsMalloczHelper(pMa, numMb * 16 * 2 * std::mem::size_of::<i16>()) as *mut _;
        (*pCtx).sMb.pCbfDc[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u16>()) as *mut _;
        (*pCtx).sMb.pNzc[i] = WelsMalloczHelper(pMa, numMb * 24 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pNzcRs[i] = WelsMalloczHelper(pMa, numMb * 24 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pScaledTCoeff[i] = WelsMalloczHelper(pMa, numMb * MB_COEFF_LIST_SIZE * std::mem::size_of::<i16>()) as *mut _;
        (*pCtx).sMb.pIntraPredMode[i] = WelsMalloczHelper(pMa, numMb * 8 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pIntra4x4FinalMode[i] = WelsMalloczHelper(pMa, numMb * 16 * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pIntraNxNAvailFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<u8>()) as *mut _;
        (*pCtx).sMb.pChromaPredMode[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pCbp[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pSubMbType[i] = WelsMalloczHelper(pMa, numMb * MB_PARTITION_SIZE * std::mem::size_of::<u32>()) as *mut _;
        (*pCtx).sMb.pSliceIdc[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i32>()) as *mut _;
        (*pCtx).sMb.pResidualPredFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pInterPredictionDoneFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<i8>()) as *mut _;
        (*pCtx).sMb.pMbCorrectlyDecodedFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<bool>()) as *mut _;
        (*pCtx).sMb.pMbRefConcealedFlag[i] = WelsMalloczHelper(pMa, numMb * std::mem::size_of::<bool>()) as *mut _;
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
            let t = *(*pCurAu).pNalUnitsList.add(kuiActualNum as usize + iIdx);
            *(*pCurAu).pNalUnitsList.add(kuiActualNum as usize + iIdx) = *(*pCurAu).pNalUnitsList.add(iIdx);
            *(*pCurAu).pNalUnitsList.add(iIdx) = t;
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
        let t = *(*pAu).pNalUnitsList.add(uiSucAuIdx as usize);
        *(*pAu).pNalUnitsList.add(uiSucAuIdx as usize) = *(*pAu).pNalUnitsList.add(uiCurAuIdx as usize);
        *(*pAu).pNalUnitsList.add(uiCurAuIdx as usize) = t;
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
    let mut uiLastNuDependencyId = (*(*(*pCurAu).pNalUnitsList.add(iStartIdx as usize))).sNalHeaderExt.uiDependencyId;
    let mut uiLastNuLayerDqId = (*(*(*pCurAu).pNalUnitsList.add(iStartIdx as usize))).sNalHeaderExt.uiLayerDqId;
    let mut iCurNalUnitIdx = iStartIdx + 1;

    while iCurNalUnitIdx <= iEndIdx {
        let pNal = *(*pCurAu).pNalUnitsList.add(iCurNalUnitIdx as usize);
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
    (*pCtx).uiTargetDqId = (*(*(*pCurAu).pNalUnitsList.add(iCurNalUnitIdx as usize))).sNalHeaderExt.uiLayerDqId;
}

pub unsafe fn RefineIdxNoInterLayerPred(pCurAu: PAccessUnit, pIdxNoInterLayerPred: *mut i32) {
    if pCurAu.is_null() || pIdxNoInterLayerPred.is_null() {
        return;
    }
    let idx = *pIdxNoInterLayerPred as usize;
    let pNal = *(*pCurAu).pNalUnitsList.add(idx);
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
        let pCurNal = *(*pCurAu).pNalUnitsList.add(iCurIdx as usize);
        if !pCurNal.is_null() && (*pCurNal).sNalHeaderExt.bNoInterLayerPredFlag {
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
    let iCurAuPoc = (*(*(*pCurAu).pNalUnitsList.add(pIdxNoInterLayerPred as usize)))
        .sNalData
        .sVclNal
        .sSliceHeaderExt
        .sSliceHeader
        .iPicOrderCntLsb;

    for i in (pIdxNoInterLayerPred + 1)..iEndIdx {
        let iTmpPoc = (*(*(*pCurAu).pNalUnitsList.add(i as usize)))
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

    if !(*pCurAu).bCompletedAuFlag {
        return false;
    }

    if (*pCtx).bNewSeqBegin {
        (*pCurAu).uiStartPos = 0;
        let mut iIdxNoInterLayerPred = kiEndPos;
        while iIdxNoInterLayerPred >= 0 {
            if (*(*(*pCurAu).pNalUnitsList.add(iIdxNoInterLayerPred as usize))).sNalHeaderExt.bNoInterLayerPredFlag {
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
        (*pCtx).iCurSeqIntervalTargetDependId = (*(*(*pCurAu).pNalUnitsList.add(endIdx))).sNalHeaderExt.uiDependencyId as i32;
        (*pCtx).iCurSeqIntervalMaxPicWidth = (*(*(*pCurAu).pNalUnitsList.add(endIdx)))
            .sNalData
            .sVclNal
            .sSliceHeaderExt
            .sSliceHeader
            .iMbWidth
            << 4;
        (*pCtx).iCurSeqIntervalMaxPicHeight = (*(*(*pCurAu).pNalUnitsList.add(endIdx)))
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
    let uiDId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiDependencyId;
    let uiQId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiQualityId;
    let uiTId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiTemporalId;

    (*pCtx).bOnlyOneLayerInCurAuFlag = true;
    if iEndIdx == iCurIdx {
        return;
    }
    iCurIdx += 1;
    while iCurIdx <= iEndIdx {
        let uiCurDId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiDependencyId;
        let uiCurQId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiQualityId;
        let uiCurTId = (*(*(*pCurAu).pNalUnitsList.add(iCurIdx))).sNalHeaderExt.uiTemporalId;
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
    if endIdx < MAX_NAL_UNIT_NUM_IN_AU && !(*(*pCurAu).pNalUnitsList.add(endIdx)).is_null() {
        let pCurNal = *(*pCurAu).pNalUnitsList.add(endIdx);
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
        if i < MAX_NAL_UNIT_NUM_IN_AU && !(*(*pCurAu).pNalUnitsList.add(i)).is_null() {
            let pNal = *(*pCurAu).pNalUnitsList.add(i);
            let uiDid = (*pNal).sNalHeaderExt.uiDependencyId as usize;
            if uiDid < MAX_LAYER_NUM {
                pTmpLayerSps[uiDid] = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
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
    if startPos < MAX_NAL_UNIT_NUM_IN_AU && !(*(*pCurAu).pNalUnitsList.add(startPos)).is_null() {
        let pNal = *(*pCurAu).pNalUnitsList.add(startPos);
        (*pCtx).pSps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pSps as *mut SSps;
        (*pCtx).pPps = (*pNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader.pPps as *mut SPps;
    }
    iErr
}

pub unsafe fn AllocPicBuffOnNewSeqBegin(pCtx: PWelsDecoderContext) -> i32 {
    if pCtx.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    let pSps = if !(*pCtx).pSps.is_null() {
        (*pCtx).pSps
    } else {
        let mut found_sps: *mut SSps = std::ptr::null_mut();
        for sps in (*pCtx).sSpsPpsCtx.sSpsBuffer.iter_mut() {
            if sps.uiTotalMbCount > 0 {
                found_sps = sps as *mut SSps;
                break;
            }
        }
        found_sps
    };

    if pSps.is_null() {
        return ERR_INFO_INVALID_PTR;
    }
    (*pCtx).pSps = pSps;

    if GetThreadCount(pCtx) <= 1 {
        WelsResetRefPic(pCtx);
    }
    let iErr = SyncPictureResolutionExt(pCtx, (*pSps).iMbWidth as u32, (*pSps).iMbHeight as u32);
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
    if GetThreadCount(pCtx) <= 1 {
        let iErr = InitConstructAccessUnit(pCtx, pDstInfo);
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

    let iErr = DecodeCurrentAccessUnit(pCtx, ppDst, pDstInfo);
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
        (*pDqLayer).uiPpsId = (*(*pLayerInfo).pPps).iPpsId as u32;
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
            let pPps = (*pSh).pPps as *mut SPps;
            (*pDqLayer).bUseWeightPredictionFlag = (*pPps).bWeightedPredFlag;
            (*pDqLayer).bUseWeightedBiPredIdc = (*pPps).uiWeightedBipredIdc != 0;
            if (*pPps).bWeightedPredFlag || (*pPps).uiWeightedBipredIdc != 0 {
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
    let mut iRet = if (*pCtx).eSliceType == B_SLICE {
        let ret = WelsInitBSliceRefList(pCtx, iPoc);
        CreateImplicitWeightTable(pCtx);
        ret
    } else {
        WelsInitRefList(pCtx, iPoc)
    };
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
        (*pCurDq).pMv[LIST_0] = (*pCtx).sMb.pMv[0][LIST_0] as *mut _;
        (*pCurDq).pMv[LIST_1] = (*pCtx).sMb.pMv[0][LIST_1] as *mut _;
        (*pCurDq).pRefIndex[LIST_0] = (*pCtx).sMb.pRefIndex[0][LIST_0] as *mut _;
        (*pCurDq).pRefIndex[LIST_1] = (*pCtx).sMb.pRefIndex[0][LIST_1] as *mut _;
        (*pCurDq).pDirect = (*pCtx).sMb.pDirect[0] as *mut _;
        (*pCurDq).pNoSubMbPartSizeLessThan8x8Flag = (*pCtx).sMb.pNoSubMbPartSizeLessThan8x8Flag[0];
        (*pCurDq).pTransformSize8x8Flag = (*pCtx).sMb.pTransformSize8x8Flag[0];
        (*pCurDq).pLumaQp = (*pCtx).sMb.pLumaQp[0] as *mut _;
        (*pCurDq).pChromaQp = (*pCtx).sMb.pChromaQp[0] as *mut _;
        (*pCurDq).pMvd[LIST_0] = (*pCtx).sMb.pMvd[0][LIST_0] as *mut _;
        (*pCurDq).pMvd[LIST_1] = (*pCtx).sMb.pMvd[0][LIST_1] as *mut _;
        (*pCurDq).pCbfDc = (*pCtx).sMb.pCbfDc[0] as *mut _;
        (*pCurDq).pNzc = (*pCtx).sMb.pNzc[0] as *mut _;
        (*pCurDq).pNzcRs = (*pCtx).sMb.pNzcRs[0] as *mut _;
        (*pCurDq).pScaledTCoeff = (*pCtx).sMb.pScaledTCoeff[0] as *mut _;
        (*pCurDq).pIntraPredMode = (*pCtx).sMb.pIntraPredMode[0] as *mut _;
        (*pCurDq).pIntra4x4FinalMode = (*pCtx).sMb.pIntra4x4FinalMode[0] as *mut _;
        (*pCurDq).pIntraNxNAvailFlag = (*pCtx).sMb.pIntraNxNAvailFlag[0] as *mut _;
        (*pCurDq).pChromaPredMode = (*pCtx).sMb.pChromaPredMode[0] as *mut _;
        (*pCurDq).pCbp = (*pCtx).sMb.pCbp[0] as *mut _;
        (*pCurDq).pSubMbType = (*pCtx).sMb.pSubMbType[0] as *mut _;
        (*pCurDq).pInterPredictionDoneFlag = (*pCtx).sMb.pInterPredictionDoneFlag[0] as *mut _;
        (*pCurDq).pResidualPredFlag = (*pCtx).sMb.pResidualPredFlag[0] as *mut _;
        (*pCurDq).pMbCorrectlyDecodedFlag = (*pCtx).sMb.pMbCorrectlyDecodedFlag[0] as *mut _;
        (*pCurDq).pMbRefConcealedFlag = (*pCtx).sMb.pMbRefConcealedFlag[0] as *mut _;
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
    let mut iRet;
    let mut bAllRefComplete = true;

    let kuiTargetLayerDqId = GetTargetDqId((*pCtx).uiTargetDqId, (*pCtx).pParam);
    let kuiDependencyIdMax = (kuiTargetLayerDqId & 0x7F) >> 4;
    let mut iLastIdD: i16 = -1;
    let mut iLastIdQ: i16 = -1;
    (*pCtx).uiNalRefIdc = 0;
    let mut bFreshSliceAvailable;

    if (*pCtx).bInitialDqLayersMem || (*pCtx).pCurDqLayer.is_null() {
        (*pCtx).pCurDqLayer = (*pCtx).pDqLayersList[0];
    }
    InitCurDqLayerData(pCtx, (*pCtx).pCurDqLayer);

    let mut pNalCur = *(*pCurAu).pNalUnitsList.add(iIdx as usize);
    (*pCtx).pNalCur = pNalCur;

    while iIdx <= iEndIdx {
        let dq_cur = (*pCtx).pCurDqLayer;
        let mut pLayerInfo = SLayerInfo::default();
        let isNewFrame = (*pCtx).pDec.is_null();

        if (*pCtx).pDec.is_null() {
            (*pCtx).pDec = PrefetchPic((*pCtx).pPicBuff) as PPicture;
            if (*pCtx).pDec.is_null() {
                (*pCtx).iErrorCode |= dsOutOfMemory;
                return ERR_INFO_REF_COUNT_OVERFLOW;
            }
            (*(*pCtx).pDec).bNewSeqBegin = (*pCtx).bNewSeqBegin;
        }

        if !pNalCur.is_null() {
            (*(*pCtx).pDec).uiTimeStamp = (*pNalCur).uiTimeStamp;
        }
        (*(*pCtx).pDec).uiDecodingTimeStamp = (*pCtx).uiDecodingTimeStamp as u32;


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
            if pNalCur.is_null() || dq_cur.is_null() {
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
            pLayerInfo.sSliceInLayer.eSliceType = (*pSh).eSliceType as u8;

            pLayerInfo.sSliceInLayer.iLastMbQp = (*pSh).iSliceQp;
            (*dq_cur).pBitStringAux = &mut (*pNalCur).sNalData.sVclNal.sSliceBitsRead;

            (*pCtx).uiNalRefIdc = (*pNalCur).sNalHeaderExt.sNalUnitHeader.uiNalRefIdc;
            let iPpsId = (*pSh).iPpsId;
            pLayerInfo.pPps = (*pSh).pPps as *mut SPps;
            pLayerInfo.pSps = (*pSh).pSps as *mut SSps;
            pLayerInfo.pSubsetSps = (*pShExt).pSubsetSps as *mut SSubsetSps;


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
                pNalCur = *(*pCurAu).pNalUnitsList.add(iIdx as usize);
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

        if pNalCur.is_null() || dq_cur.is_null() {
            break;
        }

        if !(*pCtx).pDec.is_null() {
            (*(*pCtx).pDec).bIsComplete = bAllRefComplete;
            if !(*(*pCtx).pDec).bIsComplete {
                (*pCtx).iErrorCode |= dsDataErrorConcealed;
            }
        }

        if !dq_cur.is_null() && (*dq_cur).uiLayerDqId == kuiTargetLayerDqId {
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
        let pCurNal = *(*pAu).pNalUnitsList.add((*pAu).uiEndPos as usize);
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
            FmoNextMb((*pCtx).pFmo as *mut SFmo, iRealMbIdx)
        } else {
            (*pCurDqLayer).sLayerInfo.sSliceInLayer.sSliceHeaderExt.sSliceHeader.iFirstMbInSlice + iMbIdx + 1
        };
        if iRealMbIdx == -1 {
            return false;
        }
    }
    bAllRefComplete
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_dec_stat_null() {
        unsafe {
            UpdateDecStatNoFreezingInfo(std::ptr::null_mut());
            UpdateDecStat(std::ptr::null_mut(), true);
        }
    }

    #[test]
    fn test_update_dec_stat_freezing() {
        unsafe {
            let mut stat = SDecoderStatistics::default();
            UpdateDecStatFreezingInfo(true, &mut stat);
            assert_eq!(stat.uiFreezingIDRNum, 1);
            assert_eq!(stat.uiFreezingNonIDRNum, 0);
            UpdateDecStatFreezingInfo(false, &mut stat);
            assert_eq!(stat.uiFreezingNonIDRNum, 1);
        }
    }

    #[test]
    fn test_reset_dec_stat_nums() {
        unsafe {
            let mut stat = SDecoderStatistics::default();
            stat.uiWidth = 1920;
            stat.uiHeight = 1080;
            stat.iAvgLumaQp = 26;
            stat.uiProfile = 66;
            stat.uiLevel = 31;
            stat.uiDecodedFrameCount = 100;
            stat.uiIDRCorrectNum = 5;
            ResetDecStatNums(&mut stat);
            assert_eq!(stat.uiWidth, 1920);
            assert_eq!(stat.uiHeight, 1080);
            assert_eq!(stat.iAvgLumaQp, 26);
            assert_eq!(stat.uiProfile, 66);
            assert_eq!(stat.uiLevel, 31);
            assert_eq!(stat.uiDecodedFrameCount, 0);
            assert_eq!(stat.uiIDRCorrectNum, 0);
        }
    }

    #[test]
    fn test_inline_delegation_stubs_null() {
        unsafe {
            assert_eq!(WelsTargetSliceConstruction(std::ptr::null_mut()), ERR_NONE);
            assert_eq!(
                WelsDecodeSlice(std::ptr::null_mut(), true, std::ptr::null_mut()),
                ERR_NONE
            );
            assert_eq!(WelsDecodeAndConstructSlice(std::ptr::null_mut()), ERR_NONE);
            assert_ne!(WelsInitRefList(std::ptr::null_mut(), 0), ERR_NONE);
            assert_ne!(WelsInitBSliceRefList(std::ptr::null_mut(), 0), ERR_NONE);
            assert_ne!(WelsReorderRefList(std::ptr::null_mut()), ERR_NONE);
            assert_ne!(WelsReorderRefList2(std::ptr::null_mut()), ERR_NONE);
            assert_ne!(WelsMarkAsRef(std::ptr::null_mut()), ERR_NONE);
            WelsResetRefPic(std::ptr::null_mut());
        }
    }
}
