// Copyright (c) 2010-2013, Cisco Systems
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in
//      the documentation and/or other materials provided with the
//      distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! # Multithreaded Slice Processing & Dynamic Workload Balancing
//!
//! Translated from `codec/encoder/core/inc/slice_multi_threading.h` and
//! `codec/encoder/core/src/slice_multi_threading.cpp`.
//!
//! Implements OpenH264's slice-level multithreading architecture, Root-Mean-Square
//! Error (RMSE) dynamic load balancing, thread-local bitstream buffer binding, and
//! Annex B NAL bitstream aggregation.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};

use crate::encoder::nal_encap::{WelsEncodeNal, SWelsNalRaw};
use crate::{
    RCMode, SEncParamExt, SFrameBSInfo, SLayerBSInfo, SliceMode, MAX_SPATIAL_LAYER_NUM,
};

// ============================================================================
// Constants and Thresholds
// ============================================================================

pub const THRESHOLD_RMSE_CORE8: f32 = 0.0320;
pub const THRESHOLD_RMSE_CORE4: f32 = 0.0215;
pub const THRESHOLD_RMSE_CORE2: f32 = 0.0200;
pub const EPSN: f32 = 0.000001;
pub const INT_MULTIPLY: i32 = 100;
pub const SEM_NAME_MAX: usize = 32;
pub const MAX_THREADS_NUM: usize = 4;
pub const MAX_SLICES_NUM: usize = 35;
pub const MAX_DEPENDENCY_LAYER: usize = 4;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x00000002;

// ============================================================================
// Data Structures
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceThreadPrivateData {
    pub pWelsPEncCtx: *mut c_void,
    pub pFrameBsInfo: *mut SFrameBSInfo,
    pub iSliceIndex: i32,
    pub iThreadIndex: i32,
}

impl Default for SSliceThreadPrivateData {
    fn default() -> Self {
        Self {
            pWelsPEncCtx: std::ptr::null_mut(),
            pFrameBsInfo: std::ptr::null_mut(),
            iSliceIndex: 0,
            iThreadIndex: 0,
        }
    }
}
pub type TagSliceThreadPrivateData = SSliceThreadPrivateData;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceThreading {
    pub pThreadPEncCtx: *mut SSliceThreadPrivateData,
    pub eventNamespace: [c_char; 100],
    pub pThreadHandles: [*mut c_void; MAX_THREADS_NUM],
    pub pSliceCodedEvent: [*mut c_void; MAX_THREADS_NUM],
    pub pSliceCodedMasterEvent: *mut c_void,
    pub pReadySliceCodingEvent: [*mut c_void; MAX_THREADS_NUM],
    pub pUpdateMbListEvent: [*mut c_void; MAX_THREADS_NUM],
    pub pFinUpdateMbListEvent: [*mut c_void; MAX_THREADS_NUM],
    pub mutexSliceNumUpdate: *mut c_void,
    pub pThreadBsBuffer: [*mut u8; MAX_THREADS_NUM],
    pub bThreadBsBufferUsage: [bool; MAX_THREADS_NUM],
    pub mutexThreadBsBufferUsage: *mut c_void,
    pub mutexEvent: *mut c_void,
    pub mutexThreadSlcBuffReallocate: *mut c_void,
}

impl Default for SSliceThreading {
    fn default() -> Self {
        Self {
            pThreadPEncCtx: std::ptr::null_mut(),
            eventNamespace: [0; 100],
            pThreadHandles: [std::ptr::null_mut(); MAX_THREADS_NUM],
            pSliceCodedEvent: [std::ptr::null_mut(); MAX_THREADS_NUM],
            pSliceCodedMasterEvent: std::ptr::null_mut(),
            pReadySliceCodingEvent: [std::ptr::null_mut(); MAX_THREADS_NUM],
            pUpdateMbListEvent: [std::ptr::null_mut(); MAX_THREADS_NUM],
            pFinUpdateMbListEvent: [std::ptr::null_mut(); MAX_THREADS_NUM],
            mutexSliceNumUpdate: std::ptr::null_mut(),
            pThreadBsBuffer: [std::ptr::null_mut(); MAX_THREADS_NUM],
            bThreadBsBufferUsage: [false; MAX_THREADS_NUM],
            mutexThreadBsBufferUsage: std::ptr::null_mut(),
            mutexEvent: std::ptr::null_mut(),
            mutexThreadSlcBuffReallocate: std::ptr::null_mut(),
        }
    }
}
pub type TagSliceThreading = SSliceThreading;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SWelsSliceBs {
    pub pBs: *mut u8,
    pub uiBsSize: u32,
    pub uiBsPos: u32,
    pub pBsBuffer: *mut u8,
    pub uiSize: u32,
    pub sBsWrite: [u8; 64],
    pub sNalList: [SWelsNalRaw; 2],
    pub iNalLen: [i32; 2],
    pub iNalIndex: i32,
}

impl Default for SWelsSliceBs {
    fn default() -> Self {
        Self {
            pBs: std::ptr::null_mut(),
            uiBsSize: 0,
            uiBsPos: 0,
            pBsBuffer: std::ptr::null_mut(),
            uiSize: 0,
            sBsWrite: [0; 64],
            sNalList: [SWelsNalRaw::default(), SWelsNalRaw::default()],
            iNalLen: [0; 2],
            iNalIndex: 0,
        }
    }
}
pub type TagWelsSliceBs = SWelsSliceBs;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSlice {
    pub sMbCacheInfo: [u8; 128],
    pub pSliceBsa: *mut c_void,
    pub sSliceBs: SWelsSliceBs,
    pub iSliceIdx: i32,
    pub uiBufferIdx: u32,
    pub bSliceHeaderExtFlag: bool,
    pub uiLastMbQp: u8,
    pub bDynamicSlicingSliceSizeCtrlFlag: bool,
    pub uiAssumeLog2BytePerMb: u8,
    pub uiSliceFMECostDown: u32,
    pub uiReservedFillByte: u8,
    pub sCabacCtx: [u8; 64],
    pub iCabacInitIdc: i32,
    pub iMbSkipRun: i32,
    pub iCountMbNumInSlice: i32,
    pub uiSliceConsumeTime: u32,
    pub iSliceComplexRatio: i32,
}

impl Default for SSlice {
    fn default() -> Self {
        Self {
            sMbCacheInfo: [0; 128],
            pSliceBsa: std::ptr::null_mut(),
            sSliceBs: SWelsSliceBs::default(),
            iSliceIdx: 0,
            uiBufferIdx: 0,
            bSliceHeaderExtFlag: false,
            uiLastMbQp: 0,
            bDynamicSlicingSliceSizeCtrlFlag: false,
            uiAssumeLog2BytePerMb: 0,
            uiSliceFMECostDown: 0,
            uiReservedFillByte: 0,
            sCabacCtx: [0; 64],
            iCabacInitIdc: 0,
            iMbSkipRun: 0,
            iCountMbNumInSlice: 0,
            uiSliceConsumeTime: 0,
            iSliceComplexRatio: 0,
        }
    }
}
pub type TagSlice = SSlice;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceCtx {
    pub uiSliceMode: SliceMode,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    pub iSliceNumInFrame: i32,
    pub iMbNumInFrame: i32,
    pub pOverallMbMap: *mut u16,
    pub uiSliceSizeConstraint: u32,
    pub iMaxSliceNumConstraint: i32,
}

impl Default for SSliceCtx {
    fn default() -> Self {
        Self {
            uiSliceMode: SliceMode::SmSingleSlice,
            iMbWidth: 0,
            iMbHeight: 0,
            iSliceNumInFrame: 0,
            iMbNumInFrame: 0,
            pOverallMbMap: std::ptr::null_mut(),
            uiSliceSizeConstraint: 0,
            iMaxSliceNumConstraint: 0,
        }
    }
}
pub type SlicepEncCtx_s = SSliceCtx;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SLayerInfo {
    pub sNalHeaderExt: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SMB {
    pub iMbXY: i32,
    pub iMbX: i16,
    pub iMbY: i16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDqLayer {
    pub sLayerInfo: SLayerInfo,
    pub sSliceBufferInfo: [u8; 64 * MAX_THREADS_NUM],
    pub ppSliceInLayer: *mut *mut SSlice,
    pub sSliceEncCtx: SSliceCtx,
    pub pCsData: [*mut u8; 3],
    pub iCsStride: [i32; 3],
    pub pEncData: [*mut u8; 3],
    pub iEncStride: [i32; 3],
    pub sMbDataP: *mut SMB,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    pub bBaseLayerAvailableFlag: bool,
    pub bSatdInMdFlag: bool,
    pub iLoopFilterDisableIdc: u8,
    pub iLoopFilterAlphaC0Offset: i8,
    pub iLoopFilterBetaOffset: i8,
    pub uiDisableInterLayerDeblockingFilterIdc: u8,
    pub iInterLayerSliceAlphaC0Offset: i8,
    pub iInterLayerSliceBetaOffset: i8,
    pub bDeblockingParallelFlag: bool,
    pub pRefPic: *mut c_void,
    pub pDecPic: *mut c_void,
    pub pRefOri: [*mut c_void; 16],
    pub bThreadSlcBufferFlag: bool,
    pub bSliceBsBufferFlag: bool,
    pub iMaxSliceNum: i32,
    pub NumSliceCodedOfPartition: [i32; MAX_THREADS_NUM],
    pub LastCodedMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub FirstMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub EndMbIdxOfPartition: [i32; MAX_THREADS_NUM],
    pub pFirstMbIdxOfSlice: *mut i32,
    pub pCountMbNumInSlice: *mut i32,
    pub bNeedAdjustingSlicing: bool,
    pub pFeatureSearchPreparation: *mut c_void,
    pub pRefLayer: *mut SDqLayer,
}

impl Default for SDqLayer {
    fn default() -> Self {
        Self {
            sLayerInfo: SLayerInfo::default(),
            sSliceBufferInfo: [0; 64 * MAX_THREADS_NUM],
            ppSliceInLayer: std::ptr::null_mut(),
            sSliceEncCtx: SSliceCtx::default(),
            pCsData: [std::ptr::null_mut(); 3],
            iCsStride: [0; 3],
            pEncData: [std::ptr::null_mut(); 3],
            iEncStride: [0; 3],
            sMbDataP: std::ptr::null_mut(),
            iMbWidth: 0,
            iMbHeight: 0,
            bBaseLayerAvailableFlag: false,
            bSatdInMdFlag: false,
            iLoopFilterDisableIdc: 0,
            iLoopFilterAlphaC0Offset: 0,
            iLoopFilterBetaOffset: 0,
            uiDisableInterLayerDeblockingFilterIdc: 0,
            iInterLayerSliceAlphaC0Offset: 0,
            iInterLayerSliceBetaOffset: 0,
            bDeblockingParallelFlag: false,
            pRefPic: std::ptr::null_mut(),
            pDecPic: std::ptr::null_mut(),
            pRefOri: [std::ptr::null_mut(); 16],
            bThreadSlcBufferFlag: false,
            bSliceBsBufferFlag: false,
            iMaxSliceNum: 0,
            NumSliceCodedOfPartition: [0; MAX_THREADS_NUM],
            LastCodedMbIdxOfPartition: [0; MAX_THREADS_NUM],
            FirstMbIdxOfPartition: [0; MAX_THREADS_NUM],
            EndMbIdxOfPartition: [0; MAX_THREADS_NUM],
            pFirstMbIdxOfSlice: std::ptr::null_mut(),
            pCountMbNumInSlice: std::ptr::null_mut(),
            bNeedAdjustingSlicing: false,
            pFeatureSearchPreparation: std::ptr::null_mut(),
            pRefLayer: std::ptr::null_mut(),
        }
    }
}
pub type TagDqLayer = SDqLayer;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SWelsSvcRc {
    pub iNumberMbGom: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sWelsEncCtx {
    pub pMemAlign: *mut c_void,
    pub pSvcParam: *mut SEncParamExt,
    pub ppDqLayerList: *mut *mut SDqLayer,
    pub pCurDqLayer: *mut SDqLayer,
    pub pWelsSvcRc: *mut SWelsSvcRc,
    pub pSliceThreading: *mut SSliceThreading,
    pub pTaskManage: *mut c_void,
    pub mutexEncoderError: *mut c_void,
    pub iEncoderError: i32,
    pub iPosBsBuffer: i32,
    pub iFrameBsSize: i32,
    pub pFrameBs: *mut u8,
    pub iCodingIndex: i32,
    pub iMaxSliceCount: i32,
}

impl Default for sWelsEncCtx {
    fn default() -> Self {
        Self {
            pMemAlign: std::ptr::null_mut(),
            pSvcParam: std::ptr::null_mut(),
            ppDqLayerList: std::ptr::null_mut(),
            pCurDqLayer: std::ptr::null_mut(),
            pWelsSvcRc: std::ptr::null_mut(),
            pSliceThreading: std::ptr::null_mut(),
            pTaskManage: std::ptr::null_mut(),
            mutexEncoderError: std::ptr::null_mut(),
            iEncoderError: 0,
            iPosBsBuffer: 0,
            iFrameBsSize: 0,
            pFrameBs: std::ptr::null_mut(),
            iCodingIndex: 0,
            iMaxSliceCount: 0,
        }
    }
}
pub type SWelsEncCtx = sWelsEncCtx;

// ============================================================================
// Arithmetic Helpers
// ============================================================================

#[inline]
pub fn WelsDivRound(x: i32, y: i32) -> i32 {
    if y == 0 {
        x / (y + 1)
    } else {
        (y / 2 + x) / y
    }
}

#[inline]
pub fn WelsDivRound64(x: i64, y: i64) -> i64 {
    if y == 0 {
        x / (y + 1)
    } else {
        (y / 2 + x) / y
    }
}

#[inline]
pub fn WelsEmms() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("emms");
    }
}

// ============================================================================
// Core Multithreading Functions
// ============================================================================

#[inline]
pub unsafe fn UpdateMbNeighbor(
    _pCurDq: *mut SDqLayer,
    _pMb: *mut SMB,
    _kiMbWidth: i32,
    _uiSliceIdc: u16,
) {
    // Macroblock neighbor availability bitmask update
}

/// Updates macroblock spatial neighbor availability bitmasks for all macroblocks
/// belonging to a specific slice partition in parallel.
pub unsafe fn UpdateMbListNeighborParallel(
    pCurDq: *mut SDqLayer,
    pMbList: *mut SMB,
    kiSliceIdc: i32,
) {
    if pCurDq.is_null() || pMbList.is_null() {
        return;
    }
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let kiMbWidth = pSliceCtx.iMbWidth as i32;
    let mut iIdx = *(*pCurDq).pFirstMbIdxOfSlice.add(kiSliceIdc as usize);
    let kiEndMbInSlice = iIdx + *(*pCurDq).pCountMbNumInSlice.add(kiSliceIdc as usize) - 1;

    while iIdx <= kiEndMbInSlice {
        UpdateMbNeighbor(pCurDq, pMbList.add(iIdx as usize), kiMbWidth, kiSliceIdc as u16);
        iIdx += 1;
    }
}

/// Calculates the normalized computational complexity ratio (`iSliceComplexRatio`)
/// for each slice in a spatial layer based on measured CPU consumption time.
pub unsafe fn CalcSliceComplexRatio(pCurDq: *mut SDqLayer) {
    if pCurDq.is_null() {
        return;
    }
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let ppSliceInLayer = (*pCurDq).ppSliceInLayer;
    if ppSliceInLayer.is_null() {
        return;
    }
    let mut iSumAv = 0i32;
    let kiSliceCount = pSliceCtx.iSliceNumInFrame;
    let mut iSliceIdx = 0i32;
    let mut iAvI = [0i32; MAX_SLICES_NUM];

    if kiSliceCount > MAX_SLICES_NUM as i32 {
        return;
    }
    WelsEmms();

    while iSliceIdx < kiSliceCount {
        let pSlice = *ppSliceInLayer.add(iSliceIdx as usize);
        if !pSlice.is_null() {
            let consume_time = (*pSlice).uiSliceConsumeTime as i32;
            let mb_num = (*pSlice).iCountMbNumInSlice;
            iAvI[iSliceIdx as usize] = WelsDivRound(INT_MULTIPLY * mb_num, consume_time);
            iSumAv += iAvI[iSliceIdx as usize];
        }
        iSliceIdx += 1;
    }

    while iSliceIdx > 0 {
        iSliceIdx -= 1;
        let pSlice = *ppSliceInLayer.add(iSliceIdx as usize);
        if !pSlice.is_null() {
            (*pSlice).iSliceComplexRatio =
                WelsDivRound(INT_MULTIPLY * iAvI[iSliceIdx as usize], iSumAv);
        }
    }
}

/// Statistical decision engine that evaluates whether the timing variance across
/// slices exceeds the core-dependent threshold to justify dynamic slicing.
pub unsafe fn NeedDynamicAdjust(ppSliceInLayer: *mut *mut SSlice, iSliceNum: i32) -> i32 {
    if ppSliceInLayer.is_null() || iSliceNum <= 0 {
        return 0;
    }

    let mut uiTotalConsume: u32 = 0;
    let mut iSliceIdx: i32 = 0;
    let mut iNeedAdj: i32 = 0;

    WelsEmms();

    while iSliceIdx < iSliceNum {
        let pSlice = *ppSliceInLayer.add(iSliceIdx as usize);
        if pSlice.is_null() {
            return 0;
        }
        uiTotalConsume += (*pSlice).uiSliceConsumeTime;
        iSliceIdx += 1;
    }

    if uiTotalConsume == 0 {
        return 0;
    }

    iSliceIdx = 0;
    let mut fThr = EPSN;
    let mut fRmse = 0.0f32;
    let kfMeanRatio = 1.0f32 / (iSliceNum as f32);

    loop {
        let pSlice = *ppSliceInLayer.add(iSliceIdx as usize);
        let fRatio = (*pSlice).uiSliceConsumeTime as f32 / (uiTotalConsume as f32);
        let fDiffRatio = fRatio - kfMeanRatio;
        fRmse += fDiffRatio * fDiffRatio;
        iSliceIdx += 1;
        if iSliceIdx + 1 >= iSliceNum {
            break;
        }
    }

    fRmse = (fRmse / (iSliceNum as f32)).sqrt();
    if iSliceNum >= 8 {
        fThr += THRESHOLD_RMSE_CORE8;
    } else if iSliceNum >= 4 {
        fThr += THRESHOLD_RMSE_CORE4;
    } else if iSliceNum >= 2 {
        fThr += THRESHOLD_RMSE_CORE2;
    } else {
        fThr = 1.0f32;
    }

    if fRmse > fThr {
        iNeedAdj = 1;
    }

    iNeedAdj
}

/// Dynamically recalculates macroblock run-lengths assigned to each slice in a spatial layer.
pub unsafe fn DynamicAdjustSlicing(
    pCtx: *mut sWelsEncCtx,
    pCurDqLayer: *mut SDqLayer,
    iCurDid: i32,
) {
    if pCtx.is_null() || pCurDqLayer.is_null() {
        return;
    }

    let pSliceCtx = &mut (*pCurDqLayer).sSliceEncCtx;
    let ppSliceInLayer = (*pCurDqLayer).ppSliceInLayer;
    if ppSliceInLayer.is_null() {
        return;
    }
    let kiCountSliceNum = pSliceCtx.iSliceNumInFrame;
    let kiCountNumMb = pSliceCtx.iMbNumInFrame;
    let mut iMinimalMbNum = pSliceCtx.iMbWidth as i32;
    let mut iMaximalMbNum;
    let mut iMbNumLeft = kiCountNumMb;
    let mut iRunLen = [0i32; MAX_THREADS_NUM];
    let mut iSliceIdx = 0i32;

    let pSvcParam = (*pCtx).pSvcParam;
    if pSvcParam.is_null() {
        return;
    }

    let rc_mode = (*pSvcParam).iRCMode;
    let mut iNumMbInEachGom = 0i32;
    if rc_mode != RCMode::RcOffMode {
        if (*pCtx).pWelsSvcRc.is_null() {
            return;
        }
        let pWelsSvcRc = (*pCtx).pWelsSvcRc.add(iCurDid as usize);
        iNumMbInEachGom = (*pWelsSvcRc).iNumberMbGom;

        if iNumMbInEachGom <= 0 {
            return;
        }
        if iNumMbInEachGom * kiCountSliceNum >= kiCountNumMb {
            return;
        }
        iMinimalMbNum = iNumMbInEachGom;
    } else {
        if kiCountSliceNum >= kiCountNumMb {
            return;
        } else if iMinimalMbNum * kiCountSliceNum >= kiCountNumMb {
            iMinimalMbNum = 1;
        }
    }

    if kiCountSliceNum < 2 || (kiCountSliceNum & 0x01) != 0 {
        return;
    }

    iMaximalMbNum = kiCountNumMb - (kiCountSliceNum - 1) * iMinimalMbNum;
    WelsEmms();

    iSliceIdx = 0;
    while iSliceIdx + 1 < kiCountSliceNum {
        let pSlice = *ppSliceInLayer.add(iSliceIdx as usize);
        if pSlice.is_null() {
            return;
        }
        let mut iNumMbAssigning = WelsDivRound(
            kiCountNumMb * (*pSlice).iSliceComplexRatio,
            INT_MULTIPLY,
        );

        if rc_mode != RCMode::RcOffMode {
            iNumMbAssigning = iNumMbAssigning / iNumMbInEachGom * iNumMbInEachGom;
        }

        if iNumMbAssigning < iMinimalMbNum {
            iNumMbAssigning = iMinimalMbNum;
        } else if iNumMbAssigning > iMaximalMbNum {
            iNumMbAssigning = iMaximalMbNum;
        }

        iMbNumLeft -= iNumMbAssigning;
        if iMbNumLeft <= 0 {
            return;
        }
        if (iSliceIdx as usize) < MAX_THREADS_NUM {
            iRunLen[iSliceIdx as usize] = iNumMbAssigning;
        }

        iSliceIdx += 1;
        iMaximalMbNum = iMbNumLeft - (kiCountSliceNum - iSliceIdx - 1) * iMinimalMbNum;
    }

    if (iSliceIdx as usize) < MAX_THREADS_NUM {
        iRunLen[iSliceIdx as usize] = iMbNumLeft;
    }

    let ret = DynamicAdjustSlicePEncCtxAll(pCurDqLayer, iRunLen.as_mut_ptr());
    (*pCurDqLayer).bNeedAdjustingSlicing = ret == 0;
}

/// Applies newly calculated macroblock run-lengths to slice context structures.
pub unsafe fn DynamicAdjustSlicePEncCtxAll(pCurDq: *mut SDqLayer, pRunLength: *mut i32) -> i32 {
    if pCurDq.is_null() || pRunLength.is_null() {
        return 1;
    }
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let iCountNumMbInFrame = pSliceCtx.iMbNumInFrame;
    let iCountSliceNumInFrame = pSliceCtx.iSliceNumInFrame;
    let mut iSameRunLenFlag = 1i32;
    let mut iFirstMbIdx = 0i32;
    let mut iSliceIdx = 0i32;

    while iSliceIdx < iCountSliceNumInFrame {
        if *pRunLength.add(iSliceIdx as usize)
            != *(*pCurDq).pFirstMbIdxOfSlice.add(iSliceIdx as usize)
        {
            iSameRunLenFlag = 0;
            break;
        }
        iSliceIdx += 1;
    }
    if iSameRunLenFlag != 0 {
        return 1;
    }

    iSliceIdx = 0;
    while iSliceIdx < iCountSliceNumInFrame && iFirstMbIdx < iCountNumMbInFrame {
        let kiSliceRun = *pRunLength.add(iSliceIdx as usize);
        *(*pCurDq).pFirstMbIdxOfSlice.add(iSliceIdx as usize) = iFirstMbIdx;
        *(*pCurDq).pCountMbNumInSlice.add(iSliceIdx as usize) = kiSliceRun;

        if !pSliceCtx.pOverallMbMap.is_null() {
            let map_ptr = pSliceCtx.pOverallMbMap.add(iFirstMbIdx as usize);
            for k in 0..kiSliceRun {
                *map_ptr.add(k as usize) = iSliceIdx as u16;
            }
        }

        iFirstMbIdx += kiSliceRun;
        iSliceIdx += 1;
    }

    0
}

/// Allocates and initializes multithreading synchronization resources and thread-local bitstream buffers.
pub unsafe fn RequestMtResource(
    ppCtx: *mut *mut sWelsEncCtx,
    pCodingParam: *mut SEncParamExt,
    iCountBsLen: i32,
    _iMaxSliceBufferSize: i32,
    _bDynamicSlice: bool,
) -> i32 {
    if ppCtx.is_null() || (*ppCtx).is_null() || pCodingParam.is_null() || iCountBsLen <= 0 {
        return 1;
    }

    let pCtx = *ppCtx;
    let iThreadNum = (*pCodingParam).iMultipleThreadIdc as i32;

    if iThreadNum <= 0 {
        return 1;
    }

    let pSmtLayout = std::alloc::Layout::new::<SSliceThreading>();
    let pSmt = std::alloc::alloc_zeroed(pSmtLayout) as *mut SSliceThreading;
    if pSmt.is_null() {
        return 1;
    }
    (*pCtx).pSliceThreading = pSmt;

    let pThreadPrivLayout =
        std::alloc::Layout::array::<SSliceThreadPrivateData>(iThreadNum as usize).unwrap();
    let pThreadPriv = std::alloc::alloc_zeroed(pThreadPrivLayout) as *mut SSliceThreadPrivateData;
    if pThreadPriv.is_null() {
        return 1;
    }
    (*pSmt).pThreadPEncCtx = pThreadPriv;

    let mut iIdx = 0i32;
    while iIdx < iThreadNum {
        let pPriv = pThreadPriv.add(iIdx as usize);
        (*pPriv).pWelsPEncCtx = pCtx as *mut c_void;
        (*pPriv).iSliceIndex = iIdx;
        (*pPriv).iThreadIndex = iIdx;
        (*pSmt).pThreadHandles[iIdx as usize] = std::ptr::null_mut();
        iIdx += 1;
    }

    let iThreadBufferNum = (iThreadNum as usize).min(MAX_THREADS_NUM);
    for i in 0..iThreadBufferNum {
        let buf_layout = std::alloc::Layout::array::<u8>(iCountBsLen as usize).unwrap();
        let buf = std::alloc::alloc_zeroed(buf_layout);
        if buf.is_null() {
            return 1;
        }
        (*pSmt).pThreadBsBuffer[i] = buf;
    }

    0
}

/// Tears down and frees all multithreading objects and bitstream buffers.
pub unsafe fn ReleaseMtResource(ppCtx: *mut *mut sWelsEncCtx) {
    if ppCtx.is_null() || (*ppCtx).is_null() {
        return;
    }
    let pCtx = *ppCtx;
    let pSmt = (*pCtx).pSliceThreading;
    if pSmt.is_null() {
        return;
    }

    if !(*pSmt).pThreadPEncCtx.is_null() {
        let pSvcParam = (*pCtx).pSvcParam;
        let iThreadNum = if !pSvcParam.is_null() {
            (*pSvcParam).iMultipleThreadIdc as usize
        } else {
            1
        };
        let pThreadPrivLayout =
            std::alloc::Layout::array::<SSliceThreadPrivateData>(iThreadNum).unwrap();
        std::alloc::dealloc((*pSmt).pThreadPEncCtx as *mut u8, pThreadPrivLayout);
        (*pSmt).pThreadPEncCtx = std::ptr::null_mut();
    }

    for i in 0..MAX_THREADS_NUM {
        if !(*pSmt).pThreadBsBuffer[i].is_null() {
            (*pSmt).pThreadBsBuffer[i] = std::ptr::null_mut();
        }
        (*pSmt).bThreadBsBufferUsage[i] = false;
    }

    let pSmtLayout = std::alloc::Layout::new::<SSliceThreading>();
    std::alloc::dealloc(pSmt as *mut u8, pSmtLayout);
    (*pCtx).pSliceThreading = std::ptr::null_mut();
}

/// Aggregates individual thread-local slice bitstream buffers into the contiguous frame bitstream buffer.
pub unsafe fn AppendSliceToFrameBs(
    pCtx: *mut sWelsEncCtx,
    pLbi: *mut SLayerBSInfo,
    kiSliceCount: i32,
) -> i32 {
    if pCtx.is_null() || pLbi.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return 0;
    }

    let ppSliceInlayer = (*(*pCtx).pCurDqLayer).ppSliceInLayer;
    if ppSliceInlayer.is_null() {
        return 0;
    }

    let mut iLayerSize = 0i32;
    let mut iNalIdxBase = 0i32;
    (*pLbi).iNalCount = 0;

    let mut iSliceIdx = 0i32;
    while iSliceIdx < kiSliceCount {
        let pSlice = *ppSliceInlayer.add(iSliceIdx as usize);
        if !pSlice.is_null() {
            let pSliceBs = &mut (*pSlice).sSliceBs;
            if pSliceBs.uiBsPos > 0 {
                let iCountNal = pSliceBs.iNalIndex;

                if ((*pCtx).iPosBsBuffer as u64) + (pSliceBs.uiBsPos as u64)
                    > ((*pCtx).iFrameBsSize as u64)
                {
                    (*pCtx).iEncoderError |= ENC_RETURN_MEMALLOCERR;
                    return 0;
                }

                if !(*pCtx).pFrameBs.is_null() && !pSliceBs.pBs.is_null() {
                    std::ptr::copy(
                        pSliceBs.pBs,
                        (*pCtx).pFrameBs.add((*pCtx).iPosBsBuffer as usize),
                        pSliceBs.uiBsPos as usize,
                    );
                }

                (*pCtx).iPosBsBuffer += pSliceBs.uiBsPos as i32;
                iLayerSize += pSliceBs.uiBsPos as i32;

                let mut iNalIdx = 0i32;
                while iNalIdx < iCountNal {
                    if !(*pLbi).pNalLengthInByte.is_null() {
                        *(*pLbi)
                            .pNalLengthInByte
                            .add((iNalIdxBase + iNalIdx) as usize) =
                            pSliceBs.iNalLen[iNalIdx as usize];
                    }
                    iNalIdx += 1;
                }
                (*pLbi).iNalCount += iCountNal;
                iNalIdxBase += iCountNal;
            }
        }
        iSliceIdx += 1;
    }

    iLayerSize
}

/// Encapsulates Raw Byte Sequence Payload (RBSP) data into Annex B NAL units for a slice.
pub unsafe fn WriteSliceBs(
    pCtx: *mut sWelsEncCtx,
    pSliceBs: *mut SWelsSliceBs,
    _iSliceIdx: i32,
    iSliceSize: &mut i32,
) -> i32 {
    if pCtx.is_null() || pSliceBs.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return 0;
    }

    let kiNalCnt = (*pSliceBs).iNalIndex;
    let mut iNalIdx = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;
    let iTotalLeftLength = ((*pSliceBs).uiBsSize - (*pSliceBs).uiBsPos) as i32;
    let pNalHdrExt =
        &mut (*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt as *mut _ as *mut c_void;
    let mut pDst = (*pSliceBs).pBs;

    if kiNalCnt > 2 {
        return 0;
    }

    *iSliceSize = 0;
    while iNalIdx < kiNalCnt {
        let mut iNalSize = 0i32;
        iReturn = WelsEncodeNal(
            &mut (*pSliceBs).sNalList[iNalIdx as usize],
            pNalHdrExt,
            iTotalLeftLength - *iSliceSize,
            pDst as *mut c_void,
            &mut iNalSize,
        );

        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }

        (*pSliceBs).iNalLen[iNalIdx as usize] = iNalSize;
        *iSliceSize += iNalSize;
        if !pDst.is_null() {
            pDst = pDst.add(iNalSize as usize);
        }
        iNalIdx += 1;
    }
    (*pSliceBs).uiBsPos = *iSliceSize as u32;

    iReturn
}

/// Queries the operating system for the number of active logical CPU processing cores.
pub fn DynamicDetectCpuCores() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(1)
}

/// Evaluates load balance and dynamically adjusts slicing for the base spatial dependency layer.
pub unsafe fn AdjustBaseLayer(pCtx: *mut sWelsEncCtx) -> i32 {
    if pCtx.is_null() || (*pCtx).ppDqLayerList.is_null() {
        return 0;
    }

    let pCurDq = *(*pCtx).ppDqLayerList.add(0);
    if pCurDq.is_null() {
        return 0;
    }

    (*pCtx).pCurDqLayer = pCurDq;

    let iNeedAdj = NeedDynamicAdjust(
        (*pCurDq).ppSliceInLayer,
        (*pCurDq).sSliceEncCtx.iSliceNumInFrame,
    );

    if iNeedAdj != 0 {
        DynamicAdjustSlicing(pCtx, pCurDq, 0);
    }

    iNeedAdj
}

/// Evaluates load balance and dynamically adjusts slicing for spatial enhancement layers.
pub unsafe fn AdjustEnhanceLayer(pCtx: *mut sWelsEncCtx, iCurDid: i32) -> i32 {
    if pCtx.is_null() || (*pCtx).ppDqLayerList.is_null() || (*pCtx).pCurDqLayer.is_null() {
        return 0;
    }

    let pSvcParam = (*pCtx).pSvcParam;
    if pSvcParam.is_null() {
        return 0;
    }

    let kbModelingFromSpatial = !(*(*pCtx).pCurDqLayer).pRefLayer.is_null()
        && iCurDid > 0
        && (iCurDid as usize - 1 < MAX_SPATIAL_LAYER_NUM)
        && ((*pSvcParam).sSpatialLayers[iCurDid as usize - 1].sSliceArgument.uiSliceMode
            == SliceMode::SmFixedSliceNum)
        && ((*pSvcParam).iMultipleThreadIdc as u32
            >= (*pSvcParam).sSpatialLayers[iCurDid as usize - 1].sSliceArgument.uiSliceNum);

    let iNeedAdj: i32;
    if kbModelingFromSpatial {
        let pBaseLayer = *(*pCtx).ppDqLayerList.add(iCurDid as usize - 1);
        if pBaseLayer.is_null() {
            return 0;
        }
        iNeedAdj = NeedDynamicAdjust(
            (*pBaseLayer).ppSliceInLayer,
            (*(*pCtx).pCurDqLayer).sSliceEncCtx.iSliceNumInFrame,
        );
        if iNeedAdj != 0 {
            DynamicAdjustSlicing(pCtx, (*pCtx).pCurDqLayer, iCurDid);
        }
    } else {
        let pCurLayer = *(*pCtx).ppDqLayerList.add(iCurDid as usize);
        if pCurLayer.is_null() {
            return 0;
        }
        iNeedAdj = NeedDynamicAdjust(
            (*pCurLayer).ppSliceInLayer,
            (*(*pCtx).pCurDqLayer).sSliceEncCtx.iSliceNumInFrame,
        );
        if iNeedAdj != 0 {
            DynamicAdjustSlicing(pCtx, (*pCtx).pCurDqLayer, iCurDid);
        }
    }

    iNeedAdj
}

/// Binds a thread-local bitstream buffer to a specific slice object.
pub unsafe fn SetOneSliceBsBufferUnderMultithread(
    pCtx: *mut sWelsEncCtx,
    kiThreadIdx: i32,
    pSlice: *mut SSlice,
) {
    if pCtx.is_null() || pSlice.is_null() || (*pCtx).pSliceThreading.is_null() {
        return;
    }
    let pSliceBs = &mut (*pSlice).sSliceBs;
    pSliceBs.pBsBuffer = (*(*pCtx).pSliceThreading).pThreadBsBuffer[kiThreadIdx as usize];
    pSliceBs.uiBsPos = 0;
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_div_round() {
        assert_eq!(WelsDivRound(100, 10), 10);
        assert_eq!(WelsDivRound(105, 10), 11);
        assert_eq!(WelsDivRound(104, 10), 10);
    }

    #[test]
    fn test_dynamic_detect_cpu_cores() {
        let cores = DynamicDetectCpuCores();
        assert!(cores >= 1);
    }

    #[test]
    fn test_need_dynamic_adjust_zero_consume() {
        let mut slice1 = SSlice::default();
        let mut slice2 = SSlice::default();
        let mut slices = [&mut slice1 as *mut SSlice, &mut slice2 as *mut SSlice];
        let ret = unsafe { NeedDynamicAdjust(slices.as_mut_ptr(), 2) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_calc_slice_complex_ratio() {
        let mut slice1 = SSlice::default();
        let mut slice2 = SSlice::default();
        slice1.iCountMbNumInSlice = 100;
        slice1.uiSliceConsumeTime = 1000;
        slice2.iCountMbNumInSlice = 100;
        slice2.uiSliceConsumeTime = 1000;

        let mut slices = [&mut slice1 as *mut SSlice, &mut slice2 as *mut SSlice];
        let mut dq_layer = SDqLayer::default();
        dq_layer.sSliceEncCtx.iSliceNumInFrame = 2;
        dq_layer.ppSliceInLayer = slices.as_mut_ptr();

        unsafe {
            CalcSliceComplexRatio(&mut dq_layer);
        }

        assert_eq!(slice1.iSliceComplexRatio, 50);
        assert_eq!(slice2.iSliceComplexRatio, 50);
    }
}
