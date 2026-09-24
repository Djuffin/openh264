// Copyright 2026 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE_CHROMIUM file.

// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:
//
// * Redistributions of source code must retain the above copyright notice, this
//   list of conditions and the following disclaimer.
//
// * Redistributions in binary form must reproduce the above copyright notice, this
//   list of conditions and the following disclaimer in the documentation and/or
//   other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
// ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
#![deny(unsafe_code)]
#![forbid(unsafe_code)]

//! H.264/AVC and SVC slice header and control structures —
//! `codec/decoder/core/inc/slice.h`.
//!
//! ITU-T H.264 Section 7.3.3 and Annex G (SVC) Section G.7.3.3.4.

use crate::decoder::decoder_context::SpsRef;

/// H.264 slice coding types — `EWelsSliceType` in `codec/common/inc/wels_common_defs.h`.
///
/// The decoder shares the encoder's definition; this is a re-export rather than a
/// second copy of the same six variants.
pub use crate::common::wels_common_defs::EWelsSliceType;

// Constants matching `wels_common_defs.h` and `wels_const.h`
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

/// Single reference picture list reordering command syntax element.
#[repr(C)]
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct SReorderingSyntax {
    pub uiAbsDiffPicNumMinus1: u32,
    pub uiLongTermPicNum: u16,
    pub uiReorderingOfPicNumsIdc: u16,
}

/// Reference picture list reordering syntax — `SRefPicListReorderSyn`.
/// ITU-T H.264 Section 7.3.3.1.
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

/// Prediction weight table syntax — `SPredWeightTabSyn`.
/// ITU-T H.264 Section 7.3.3.2.
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

/// Decoded reference picture marking syntax — `SRefPicMarking`.
/// ITU-T H.264 Section 7.3.3.3.
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

/// Decoded reference base picture marking syntax — `SRefBasePicMarking`.
/// ITU-T H.264 Annex G Section G.7.3.3.4.
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

/// Header of slice syntax elements — `SSliceHeader`.
/// ITU-T H.264 Section 7.3.3.
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

    /// The active parameter sets as ids, resolved by `sps_of`/`pps_of` at each use.
    /// `None` until `ParseSliceHeaderSyntaxs` fills them.
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

/// Slice header in scalable extension syntax — `SSliceHeaderExt`.
/// ITU-T H.264 Annex G Section G.7.3.3.4.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TagSliceHeaderExt {
    pub sSliceHeader: SSliceHeader,
    /// The subset SPS id, resolved by `subset_sps_of`.
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

/// Active slice context and state tracking — `SSlice`.
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
// Note: the slice-QP, MB-QP, POC-MSB and implicit-weight derivations used to be
// mirrored here as standalone helpers. They had no callers outside this file's
// own unit tests, while the decode path computes them inline in `decoder_core`
// (`WelsDecodeSlice` for the QP derivations, `DecodePocFromSliceHeader` for the
// POC MSB, and `CreateImplicitWeightTable` for the implicit weights). Testing the
// copies proved nothing about the shipping code, so the copies are gone.
// ---------------------------------------------------------------------------

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
}
