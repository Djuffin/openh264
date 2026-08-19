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
    unused_variables,
    unused_unsafe
)]

use std::ffi::{c_char, c_void};

use crate::encoder::nal_encap::{bs_buffer, WelsEncodeNal, SWelsNalRaw};
use crate::{
    RCMode, SEncParamExt, SFrameBSInfo, SLayerBSInfo, SliceMode, MAX_SPATIAL_LAYER_NUM,
};
pub use crate::encoder::nal_encap::SWelsSliceBs;
pub use crate::encoder::rc::SWelsSvcRc;
pub use crate::encoder::svc_encode_slice::SLayerInfo;
pub use crate::encoder::md::SMB;
pub use crate::encoder::svc_encode_slice::SSlice;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::encoder_context::sWelsEncCtx;

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
/// One definition, in `wels_encoder_ext` — `svc_enc_slice_segment.h:62`.
pub use crate::encoder::wels_encoder_ext::MAX_SLICES_NUM;
pub const MAX_DEPENDENCY_LAYER: usize = 4;

pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;

/// `DEFAULT_MAXPACKETSIZE_CONSTRAINT` — `svc_enc_slice_segment.h:67`, in bytes.
pub const DEFAULT_MAXPACKETSIZE_CONSTRAINT: u32 = 1200;

/// `WelsSetMemUint16_c` — `codec/common/inc/macros.h:306`.
///
/// # Safety
/// `pDst` must point to at least `iSizeOfData` writable `u16`s.
pub unsafe fn WelsSetMemUint16_c(pDst: *mut u16, iValue: u16, iSizeOfData: i32) {
    for i in 0..iSizeOfData as usize {
        *pDst.add(i) = iValue;
    }
}

/// `WelsSetMemUint32_c` — `codec/common/inc/macros.h:300`.
///
/// # Safety
/// `pDst` must point to at least `iSizeOfData` writable `u32`s.
pub unsafe fn WelsSetMemUint32_c(pDst: *mut u32, iValue: u32, iSizeOfData: i32) {
    for i in 0..iSizeOfData as usize {
        *pDst.add(i) = iValue;
    }
}

/// `WelsSetMemMultiplebytes_c` — `codec/common/inc/macros.h:312`.
///
/// Note the asymmetry C++ has and this reproduces: the non-zero paths write
/// `iSizeOfData` *elements*, while the zero path memsets `iSizeOfData *
/// iDataLengthOfData` *bytes* — the same span, expressed differently.
///
/// # Safety
/// `pDst` must point to at least `iSizeOfData * iDataLengthOfData` writable bytes, and
/// `iDataLengthOfData` must be 1, 2 or 4.
pub unsafe fn WelsSetMemMultiplebytes_c(
    pDst: *mut c_void,
    iValue: u32,
    iSizeOfData: i32,
    iDataLengthOfData: i32,
) {
    debug_assert!(iDataLengthOfData == 4 || iDataLengthOfData == 2 || iDataLengthOfData == 1);

    if 0 != iValue {
        if 4 == iDataLengthOfData {
            WelsSetMemUint32_c(pDst as *mut u32, iValue, iSizeOfData);
        } else if 2 == iDataLengthOfData {
            WelsSetMemUint16_c(pDst as *mut u16, iValue as u16, iSizeOfData);
        } else {
            std::ptr::write_bytes(pDst as *mut u8, iValue as u8, iSizeOfData as usize);
        }
    } else {
        std::ptr::write_bytes(
            pDst as *mut u8,
            0,
            (iSizeOfData * iDataLengthOfData) as usize,
        );
    }
}

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

pub type TagWelsSliceBs = SWelsSliceBs;


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
            uiSliceMode: SliceMode::SM_SINGLE_SLICE,
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




pub type TagDqLayer = SDqLayer;



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
// ============================================================================
// Mutex helpers (`WelsMutexInit` / `WelsMutexLock` / `WelsMutexUnlock` /
// `WelsMutexDestroy` from `codec/common/inc/WelsThreadLib.h`)
// ============================================================================
//
// `SSliceThreading` stores its mutexes as opaque handles, matching the C++
// `WELS_MUTEX` fields. A `std::sync::Mutex` cannot be locked and unlocked
// through two separate calls the way pthreads can — the guard owns the lock —
// so the lock/unlock pair is expressed as one scoped call. Every C++
// lock/unlock pair in the encoder brackets a single straight-line region, so
// the critical sections are identical; only the spelling differs.

/// Allocates a mutex and returns its opaque handle (`WelsMutexInit`).
pub unsafe fn WelsMutexInit(pMutex: *mut *mut c_void) -> i32 {
    let m: Box<std::sync::Mutex<()>> = Box::new(std::sync::Mutex::new(()));
    *pMutex = Box::into_raw(m) as *mut c_void;
    0 // WELS_THREAD_ERROR_OK
}

/// Frees a mutex allocated by [`WelsMutexInit`] (`WelsMutexDestroy`).
pub unsafe fn WelsMutexDestroy(pMutex: *mut *mut c_void) {
    if !(*pMutex).is_null() {
        drop(Box::from_raw(*pMutex as *mut std::sync::Mutex<()>));
        *pMutex = std::ptr::null_mut();
    }
}

/// Runs `f` holding `pMutex`, i.e. a `WelsMutexLock`/`WelsMutexUnlock` pair.
///
/// A null handle runs `f` unlocked; that mirrors the C++ behaviour on an
/// uninitialised mutex closely enough for the single-threaded paths, which
/// never contend.
pub unsafe fn with_wels_mutex<R>(pMutex: *mut c_void, f: impl FnOnce() -> R) -> R {
    if pMutex.is_null() {
        return f();
    }
    let m = &*(pMutex as *const std::sync::Mutex<()>);
    // A worker that panicked mid-slice leaves the mutex poisoned; the encoder
    // has no recovery path for that, so take the guard either way.
    let _guard = m.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

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
        crate::encoder::svc_encode_slice::UpdateMbNeighbor(pCurDq, pMbList.add(iIdx as usize), kiMbWidth, kiSliceIdc as u16);
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
    let mut iSliceIdx: i32;

    let pSvcParam = (*pCtx).pSvcParam;
    if pSvcParam.is_null() {
        return;
    }

    let rc_mode = (*pSvcParam).iRCMode;
    let mut iNumMbInEachGom = 0i32;
    if rc_mode != RCMode::RC_OFF_MODE {
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

        if rc_mode != RCMode::RC_OFF_MODE {
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
    pCodingParam: *mut crate::encoder::param_svc::SWelsSvcCodingParam,
    iCountBsLen: i32,
    _iMaxSliceBufferSize: i32,
    bDynamicSlice: bool,
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
        // The C++ zeroes the handle here and never spawns a thread of its own;
        // all worker threads come from the shared CWelsThreadPool that
        // CreateTaskManage acquires below. pThreadHandles is only ever read
        // back by WelsUninitEncoderExt.
        (*pSmt).pThreadHandles[iIdx as usize] = std::ptr::null_mut();
        iIdx += 1;
    }

    // The four per-thread event sets (pUpdateMbListEvent, pFinUpdateMbListEvent,
    // pSliceCodedEvent, pReadySliceCodingEvent), pSliceCodedMasterEvent and
    // mutexEvent are opened and closed by the C++ but never signalled or waited
    // on anywhere in codec/ — they are vestiges of the pre-thread-pool design.
    // They are deliberately not reproduced.

    if WelsMutexInit(&mut (*pSmt).mutexSliceNumUpdate) != 0 {
        return 1;
    }

    (*pCtx).pTaskManage = crate::encoder::wels_task_management::CreateTaskManage(
        pCtx,
        (*pCodingParam).iSpatialLayerNum,
        bDynamicSlice,
    ) as *mut c_void;
    if (*pCtx).pTaskManage.is_null() {
        return 1;
    }

    let pTaskManage =
        (*pCtx).pTaskManage as *mut crate::encoder::wels_task_management::CWelsTaskManageBase;
    let iThreadBufferNum =
        ((*pTaskManage).GetThreadPoolThreadNum() as usize).min(MAX_THREADS_NUM);

    for i in 0..iThreadBufferNum {
        let buf_layout = std::alloc::Layout::array::<u8>(iCountBsLen as usize).unwrap();
        let buf = std::alloc::alloc_zeroed(buf_layout);
        if buf.is_null() {
            return 1;
        }
        (*pSmt).pThreadBsBuffer[i] = buf;
    }

    if WelsMutexInit(&mut (*pSmt).mutexThreadBsBufferUsage) != 0 {
        return 1;
    }
    if WelsMutexInit(&mut (*pSmt).mutexThreadSlcBuffReallocate) != 0 {
        return 1;
    }
    if WelsMutexInit(&mut (*pCtx).mutexEncoderError) != 0 {
        return 1;
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

    WelsMutexDestroy(&mut (*pSmt).mutexSliceNumUpdate);
    WelsMutexDestroy(&mut (*pSmt).mutexThreadBsBufferUsage);
    WelsMutexDestroy(&mut (*pSmt).mutexThreadSlcBuffReallocate);
    WelsMutexDestroy(&mut (*pCtx).mutexEncoderError);

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

    // WELS_DELETE_OP (pTaskManage). Dropping the manager runs Uninit(), which
    // releases this encoder's reference to the shared thread pool; the last
    // reference out stops and joins the worker threads.
    if !(*pCtx).pTaskManage.is_null() {
        drop(Box::from_raw(
            (*pCtx).pTaskManage as *mut crate::encoder::wels_task_management::CWelsTaskManageBase,
        ));
        (*pCtx).pTaskManage = std::ptr::null_mut();
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
    let pNalHdrExt = std::ptr::addr_of!((*(*pCtx).pCurDqLayer).sLayerInfo.sNalHeaderExt);
    let mut pDst = (*pSliceBs).pBs;

    if kiNalCnt > 2 {
        return 0;
    }

    *iSliceSize = 0;
    while iNalIdx < kiNalCnt {
        let mut iNalSize = 0i32;
        // The slice's NAL list is offsets into the thread buffer its writer is
        // positioned in; that buffer is named here, beside the entry.
        iReturn = WelsEncodeNal(
            &(*pSliceBs).sNalList[iNalIdx as usize],
            &*bs_buffer((*pSliceBs).pBsBuffer, (*pSliceBs).uiSize),
            Some(&*pNalHdrExt),
            pDst,
            iTotalLeftLength - *iSliceSize,
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
            == SliceMode::SM_FIXEDSLCNUM_SLICE)
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
