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

// deny(unsafe_code) lands with Phase 7; this file is the thread machinery.
// The exemption itself is the `#[allow(unsafe_code)]` on this file's `pub mod`
// line in `encoder/mod.rs`, where the module-level deny reaches from.

use std::ffi::c_void;

use crate::encoder::nal_encap::{
    WelsEncodeNal, WelsLoadNalForSlice, WelsUnloadNalForSlice, WelsWriteSVCPrefixNal, SWelsNalRaw,
};
use crate::common::wels_common_defs::{EWelsNalRefIdc, EWelsNalUnitType};
use crate::encoder::encoder_context::ctx_func_list;
use crate::encoder::svc_encode_slice::{
    InitOneSliceInThread, SetSliceBoundaryInfo, WelsCodeOneSlice,
};
use crate::encoder::vlc_encoder::BsWriter;
use crate::encoder::wels_encoder_ext::WelsTime;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;
use crate::encoder::svc_encode_slice::thread_bs_buffer;
use crate::encoder::svc_encode_slice::{current_layer, set_current_layer, LayerIdx};
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
// **T6.H4, and the one MT edit this session makes**: the frame bitstream is owned
// by the context now, so the two `pFrameBs` spellings in this file become the two
// accessors. Field spellings only — no body in this file is touched, and the
// thread machinery is Phase 7's.
use crate::encoder::encoder_context::{
    ctx_dq_layer, ctx_frame_bs, ctx_frame_bs_at, ctx_param, ctx_rc, ctx_rc_at,
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
/// `iDataLengthOfData` must be 1, 2 or 4. (`*mut u16` since Phase 6 session B: the C++
/// takes `void*`, and all four callers pass a `uint16_t` macroblock map with
/// `iDataLengthOfData == 2`. The other two widths are the C++ macro's, kept.)
/// `WelsSetMemMultiplebytes_c` over the macroblock map, as a `fill` on a subslice —
/// **T6.D7**.
///
/// The raw call it replaces is `WelsSetMemMultiplebytes_c(map + first, value, count,
/// 2)`, which reaches `WelsSetMemUint16_c`'s `for i in 0..count as usize`: a negative
/// `count` wraps to ~2^64 there and a `first + count` past the end writes off the
/// allocation. Neither is reachable — 341/341 has held over both — so the guard and
/// the clamp below change nothing that is defined today, and make the two
/// preconditions the raw spelling relied on explicit instead of latent.
#[inline]
pub fn fill_mb_map(map: &mut [u16], kiFirstMb: i32, kiCount: i32, uiValue: u16) {
    if kiFirstMb < 0 || kiCount <= 0 {
        return;
    }
    let a = kiFirstMb as usize;
    let b = a.saturating_add(kiCount as usize).min(map.len());
    if a < b {
        map[a..b].fill(uiValue);
    }
}

pub unsafe fn WelsSetMemMultiplebytes_c(
    pDst: *mut u16,
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
/// The C++ `SSliceThreading` carries, besides these fields, four per-thread
/// `WELS_EVENT` arrays (`pSliceCodedEvent`, `pReadySliceCodingEvent`,
/// `pUpdateMbListEvent`, `pFinUpdateMbListEvent`), `pSliceCodedMasterEvent`,
/// `mutexEvent`, the `eventNamespace` that names them, and `pThreadHandles`.
/// **Every one of them is dead on both sides** — the C++ opens and closes the
/// events and never signals or waits on one, never locks `mutexEvent`, and never
/// spawns into `pThreadHandles`; the port never reproduced any of it. They are
/// vestiges of the pre-thread-pool design. Deleted at T7.A2 with the site-by-site
/// grep recorded in the session-A log entry; the join is `WelsTaskBarrier`.
pub struct SSliceThreading {
    pub pThreadPEncCtx: *mut SSliceThreadPrivateData,
    pub mutexSliceNumUpdate: *mut c_void,
    pub pThreadBsBuffer: [*mut u8; MAX_THREADS_NUM],
    /// Length, in bytes, of every `pThreadBsBuffer` entry — the `iCountBsLen`
    /// `RequestMtResource` was called with. Kept because `ReleaseMtResource` needs
    /// the allocation's `Layout` back to free it, and the C++ did not: its
    /// allocator carries its own size table (`pMa->WelsFree`). Without this the
    /// buffers could only be nulled, which is what the raw translation did — see
    /// T7.A3.
    pub uiThreadBsBufferLen: usize,
    /// How many of the `MAX_THREADS_NUM` slots actually have a buffer behind them:
    /// `min(iMultipleThreadIdc, MAX_THREADS_NUM)`. **F67's bound, made readable.**
    /// `QueryEmptyThread` scanned all `MAX_THREADS_NUM` slots and it was the pool's
    /// concurrency cap that kept its answer in range; with the pool gone the cap has
    /// to be stated, and this is where the fork reads it (`ForkWidth`).
    pub uiThreadBsBufferNum: usize,
    pub bThreadBsBufferUsage: [bool; MAX_THREADS_NUM],
    pub mutexThreadBsBufferUsage: *mut c_void,
    pub mutexEvent: *mut c_void,
    pub mutexThreadSlcBuffReallocate: *mut c_void,
}

impl Default for SSliceThreading {
    fn default() -> Self {
        Self {
            pThreadPEncCtx: std::ptr::null_mut(),
            mutexSliceNumUpdate: std::ptr::null_mut(),
            pThreadBsBuffer: [std::ptr::null_mut(); MAX_THREADS_NUM],
            uiThreadBsBufferLen: 0,
            uiThreadBsBufferNum: 0,
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

// Not `repr(C)` and not `Copy` since **T6.D7**: `pOverallMbMap` is a `Vec<u16>`,
// which has no C shape and owns its storage. Nothing in the crate copied this struct
// by value — the compiler's answer, not an argument.
#[derive(Debug)]
pub struct SSliceCtx {
    pub uiSliceMode: SliceMode,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    pub iSliceNumInFrame: i32,
    pub iMbNumInFrame: i32,
    /// One slice index per macroblock, in raster order — **owned since T6.D7**
    /// (plan §4's "maps -> `Vec<u16>`"). Allocated by `InitSlicePEncCtx`, released by
    /// the layer's `Drop` where `UninitSlicePEncCtx` used to free it.
    pub pOverallMbMap: Vec<u16>,
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
            pOverallMbMap: Vec::new(),
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
    let first: &[i32] = &(*pCurDq).pFirstMbIdxOfSlice;
    let count: &[i32] = &(*pCurDq).pCountMbNumInSlice;
    let mut iIdx = first[kiSliceIdc as usize];
    let kiEndMbInSlice = iIdx + count[kiSliceIdc as usize] - 1;

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
    let mut iSumAv = 0i32;
    let kiSliceCount = pSliceCtx.iSliceNumInFrame;
    let mut iSliceIdx = 0i32;
    let mut iAvI = [0i32; MAX_SLICES_NUM];

    if kiSliceCount > MAX_SLICES_NUM as i32 {
        return;
    }
    WelsEmms();

    while iSliceIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
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
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
        if !pSlice.is_null() {
            (*pSlice).iSliceComplexRatio =
                WelsDivRound(INT_MULTIPLY * iAvI[iSliceIdx as usize], iSumAv);
        }
    }
}

/// Statistical decision engine that evaluates whether the timing variance across
/// slices exceeds the core-dependent threshold to justify dynamic slicing.
pub unsafe fn NeedDynamicAdjust(pCurDq: *mut SDqLayer, iSliceNum: i32) -> i32 {
    if pCurDq.is_null() || iSliceNum <= 0 {
        return 0;
    }

    let mut uiTotalConsume: u32 = 0;
    let mut iSliceIdx: i32 = 0;
    let mut iNeedAdj: i32 = 0;

    WelsEmms();

    while iSliceIdx < iSliceNum {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
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
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
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
    let kiCountSliceNum = pSliceCtx.iSliceNumInFrame;
    let kiCountNumMb = pSliceCtx.iMbNumInFrame;
    let mut iMinimalMbNum = pSliceCtx.iMbWidth as i32;
    let mut iMaximalMbNum;
    let mut iMbNumLeft = kiCountNumMb;
    let mut iRunLen = [0i32; MAX_THREADS_NUM];
    let mut iSliceIdx: i32;

    let pSvcParam = ctx_param(pCtx);
    if pSvcParam.is_null() {
        return;
    }

    let rc_mode = (*pSvcParam).iRCMode;
    let mut iNumMbInEachGom = 0i32;
    if rc_mode != RCMode::RC_OFF_MODE {
        if ctx_rc(pCtx).is_null() {
            return;
        }
        let pWelsSvcRc = ctx_rc_at(pCtx, iCurDid as usize);
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
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDqLayer, iSliceIdx);
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
        let first: &[i32] = &(*pCurDq).pFirstMbIdxOfSlice;
        if *pRunLength.add(iSliceIdx as usize) != first[iSliceIdx as usize] {
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
        {
            let first: &mut Vec<i32> = &mut (*pCurDq).pFirstMbIdxOfSlice;
            first[iSliceIdx as usize] = iFirstMbIdx;
            let count: &mut Vec<i32> = &mut (*pCurDq).pCountMbNumInSlice;
            count[iSliceIdx as usize] = kiSliceRun;
        }

        {
            let map: &mut Vec<u16> = &mut pSliceCtx.pOverallMbMap;
            fill_mb_map(map, iFirstMbIdx, kiSliceRun, iSliceIdx as u16);
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
        iIdx += 1;
    }

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

    (*pSmt).uiThreadBsBufferLen = iCountBsLen as usize;
    (*pSmt).uiThreadBsBufferNum = iThreadBufferNum;
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
        let pSvcParam = ctx_param(pCtx);
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

    // The C++ frees here (`pMa->WelsFree (pSmt->pThreadBsBuffer[i], ...)`,
    // slice_multi_threading.cpp:426). The raw translation kept the null-out and
    // dropped the free, so every encoder instance with iMultipleThreadIdc > 1 leaked
    // `iCountBsLen` bytes per worker for the process's life — T7.A3. The length is
    // `uiThreadBsBufferLen` because a Rust `dealloc` needs the `Layout` back and the
    // C++ allocator's size table has no counterpart here.
    let buf_len = (*pSmt).uiThreadBsBufferLen;
    for i in 0..MAX_THREADS_NUM {
        if !(*pSmt).pThreadBsBuffer[i].is_null() {
            if buf_len > 0 {
                let buf_layout = std::alloc::Layout::array::<u8>(buf_len).unwrap();
                std::alloc::dealloc((*pSmt).pThreadBsBuffer[i], buf_layout);
            }
            (*pSmt).pThreadBsBuffer[i] = std::ptr::null_mut();
        }
        (*pSmt).bThreadBsBufferUsage[i] = false;
    }
    (*pSmt).uiThreadBsBufferLen = 0;
    (*pSmt).uiThreadBsBufferNum = 0;

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
    if pCtx.is_null() || pLbi.is_null() || current_layer(pCtx).is_null() {
        return 0;
    }

    let pCurDq = current_layer(pCtx);
    let mut iLayerSize = 0i32;
    let mut iNalIdxBase = 0i32;
    (*pLbi).iNalCount = 0;

    let mut iSliceIdx = 0i32;
    while iSliceIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(pCurDq, iSliceIdx);
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

                if !ctx_frame_bs(pCtx).is_null() && !pSliceBs.pBs.is_null() {
                    std::ptr::copy(
                        pSliceBs.pBs,
                        ctx_frame_bs_at(pCtx, (*pCtx).iPosBsBuffer),
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
///
/// Takes the slice rather than its `sSliceBs` (the C++ took `SWelsSliceBs*`):
/// the NAL list is offsets into the thread buffer the slice was claimed into, and
/// that buffer is `pThreadBsBuffer[pSlice->uiBufferIdx]` — the field that used to
/// cache it is gone (Phase 6 session B).
pub unsafe fn WriteSliceBs(
    pCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    _iSliceIdx: i32,
    iSliceSize: &mut i32,
) -> i32 {
    if pCtx.is_null() || pSlice.is_null() || current_layer(pCtx).is_null() {
        return 0;
    }
    let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);

    let kiNalCnt = (*pSliceBs).iNalIndex;
    let mut iNalIdx = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;
    let iTotalLeftLength = ((*pSliceBs).uiBsSize - (*pSliceBs).uiBsPos) as i32;
    let pNalHdrExt = std::ptr::addr_of!((*current_layer(pCtx)).sLayerInfo.sNalHeaderExt);
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
            &*thread_bs_buffer(pCtx, pSlice),
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
    if pCtx.is_null() || ctx_dq_layer(pCtx, 0).is_null() {
        return 0;
    }

    let pCurDq = ctx_dq_layer(pCtx, 0);
    if pCurDq.is_null() {
        return 0;
    }

    // T6.G2's one edit in an MT file: the field is a position now, and layer 0 is
    // the position this function has always meant (`ppDqLayerList[0]`, two lines
    // up). Body otherwise untouched — Phase 7 owns everything else here.
    set_current_layer(pCtx, Some(LayerIdx(0)));

    let iNeedAdj = NeedDynamicAdjust(pCurDq, (*pCurDq).sSliceEncCtx.iSliceNumInFrame);

    if iNeedAdj != 0 {
        DynamicAdjustSlicing(pCtx, pCurDq, 0);
    }

    iNeedAdj
}

/// Evaluates load balance and dynamically adjusts slicing for spatial enhancement layers.
pub unsafe fn AdjustEnhanceLayer(pCtx: *mut sWelsEncCtx, iCurDid: i32) -> i32 {
    if pCtx.is_null() || ctx_dq_layer(pCtx, 0).is_null() || current_layer(pCtx).is_null() {
        return 0;
    }

    let pSvcParam = ctx_param(pCtx);
    if pSvcParam.is_null() {
        return 0;
    }

    let kbModelingFromSpatial = (*current_layer(pCtx)).pRefLayer.is_some()
        && iCurDid > 0
        && (iCurDid as usize - 1 < MAX_SPATIAL_LAYER_NUM)
        && ((*pSvcParam).sSpatialLayers[iCurDid as usize - 1].sSliceArgument.uiSliceMode
            == SliceMode::SM_FIXEDSLCNUM_SLICE)
        && ((*pSvcParam).iMultipleThreadIdc as u32
            >= (*pSvcParam).sSpatialLayers[iCurDid as usize - 1].sSliceArgument.uiSliceNum);

    let iNeedAdj: i32;
    if kbModelingFromSpatial {
        let pBaseLayer = ctx_dq_layer(pCtx, iCurDid as usize - 1);
        if pBaseLayer.is_null() {
            return 0;
        }
        iNeedAdj = NeedDynamicAdjust(
            pBaseLayer,
            (*current_layer(pCtx)).sSliceEncCtx.iSliceNumInFrame,
        );
        if iNeedAdj != 0 {
            DynamicAdjustSlicing(pCtx, current_layer(pCtx), iCurDid);
        }
    } else {
        let pCurLayer = ctx_dq_layer(pCtx, iCurDid as usize);
        if pCurLayer.is_null() {
            return 0;
        }
        iNeedAdj = NeedDynamicAdjust(
            pCurLayer,
            (*current_layer(pCtx)).sSliceEncCtx.iSliceNumInFrame,
        );
        if iNeedAdj != 0 {
            DynamicAdjustSlicing(pCtx, current_layer(pCtx), iCurDid);
        }
    }

    iNeedAdj
}

// `SetOneSliceBsBufferUnderMultithread(pCtx, kiThreadIdx, pSlice)` was here. It
// re-stamped `sSliceBs.pBsBuffer = pThreadBsBuffer[kiThreadIdx]` and zeroed
// `uiBsPos`; the task called it with the same `kiThreadIdx` it had just passed to
// `InitOneSliceInThread`, which stores that index in `uiBufferIdx` and zeroes
// `uiBsPos` itself. With the cached pointer gone it did nothing the previous call
// had not — deleted with its call (S18), Phase 6 session B.

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

    /// A layer with one bank of `n` slices and `ppSliceInLayer` naming them in
    /// order — the shape `InitSliceInLayer` builds, in the two lines a test needs.
    /// The bank is returned so it outlives the layer that points into it.
    fn layer_with_bank(n: usize) -> SDqLayer {
        let mut dq_layer = SDqLayer::default();
        dq_layer.sSliceBufferInfo[0].pSliceBuffer = (0..n).map(|_| SSlice::new()).collect();
        dq_layer.sSliceBufferInfo[0].iMaxSliceNum = n as i32;
        dq_layer.ppSliceInLayer = (0..n)
            .map(|i| crate::encoder::svc_encode_slice::SliceIdx { bank: 0, offset: i as i32 })
            .collect();
        dq_layer
    }

    #[test]
    fn test_need_dynamic_adjust_zero_consume() {
        let mut dq_layer = layer_with_bank(2);
        let ret = unsafe { NeedDynamicAdjust(&mut dq_layer, 2) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_calc_slice_complex_ratio() {
        let mut dq_layer = layer_with_bank(2);
        for slice in dq_layer.sSliceBufferInfo[0].pSliceBuffer.iter_mut() {
            slice.iCountMbNumInSlice = 100;
            slice.uiSliceConsumeTime = 1000;
        }
        dq_layer.sSliceEncCtx.iSliceNumInFrame = 2;

        unsafe {
            CalcSliceComplexRatio(&mut dq_layer);
        }

        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[0].iSliceComplexRatio, 50);
        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[1].iSliceComplexRatio, 50);
    }
}

// ============================================================================
// The spawn seam — D-mt-1 (plan §7.4), and the fork/join it carries
// ============================================================================
//
// This section replaces the pool dispatch for the fixed slice modes. What it does
// NOT change is the access pattern: the workers call the same slice-encode tree
// with the same raw context pointer the pool's tasks handed it, in the same order,
// with the same per-slice state. Only the machinery around the call shrinks — the
// task hierarchy, the C++-list ports, the `static mut` singleton, the claiming
// mutex and the condvar dance all become `std::thread::scope`.

/// One worker's share of a frame's slices, and the one value in this crate that
/// crosses a spawn.
///
/// The worker owns bs scratch slot `iBsSlot` for the whole scope and encodes the
/// slices `iFirstSlice`, `iFirstSlice + iSliceStep`, ... below `iSliceCount` — a
/// static partition where the pool did a dynamic claim, which is strictly more
/// deterministic and is why the claiming mutex can go.
pub struct SliceJobHandle {
    /// The encoder context, held raw exactly as `CWelsBaseTask::m_pCtx` held it.
    pCtx: *mut sWelsEncCtx,
    /// This worker's bs scratch slot: `pSliceThreading->pThreadBsBuffer[iBsSlot]`,
    /// reached from the slice through `SSlice::uiBufferIdx`.
    iBsSlot: i32,
    iFirstSlice: i32,
    iSliceStep: i32,
    iSliceCount: i32,
    /// `CWelsLoadBalancingSlicingEncodingTask`'s two extra stamps — the start time
    /// in `InitTask`, the elapsed time in `FinishTask`. False is
    /// `CWelsSliceEncodingTask`, which is what `bUseLoadBalancing = false` builds.
    bRecordsTime: bool,
}

// unsafe-cat: send-seam(Phase 9)
//
// **The one hand-written `Send` this phase permits** (decision D-mt-1, plan §7.4)
// — and the ratchet's `unsafe_impl` metric is an occurrence count, so this comment
// deliberately does not spell the two words it would otherwise double. It retires
// when Phase 9's context split makes this handle naturally `Send`:
// `sWelsEncCtx` is `!Sync` for twelve distinct reasons (F67), five of them inside
// types Phases 8 and 10 own, so the split is a precondition of the fork/join and
// not a consequence of it. Until then the soundness argument is three parts, each
// verified rather than asserted:
//
// **1 — disjointness is index-based, and the index is now static** (session A's
// premise-1 proof, T7.A1). The only per-thread mutable state the encode reaches
// through the context is the bs scratch buffer, named by `iBsSlot` and reached by
// exactly one route: `iBsSlot` -> `InitOneSliceInThread(kiSlcBuffIdx)` ->
// `SSlice::uiBufferIdx` -> `thread_bs_buffer()` = `pThreadBsBuffer[uiBufferIdx]`.
// One handle per slot is constructed, the slots are `0..worker_count`, and the
// handle is moved into its spawn — so no two live workers can name the same slot.
// The pool proved the same property with a test-and-set under
// `mutexThreadBsBufferUsage`; a partition proves it by construction.
// Everything else the slices touch is per-slice: for a fixed mode
// `bThreadSlcBufferFlag` is false, so slice `i` is `slice_in_bank(layer, 0, i)` —
// a pure function of the slice index, never of the schedule — and each slice
// writes only its own `sSliceBs` (`pBs`, `iNalLen[]`, `uiBsPos`).
//
// **2 — assembly is order-based** (premise 2, same proof). `AppendSliceToFrameBs`
// runs after the join, on the calling thread, and walks slice index 0..N stitching
// each slice's bytes in that order. Completion order is not observable, which is
// why MT output is byte-deterministic today and why the slot a slice borrows is
// byte-neutral. Today's slot assignment is already a race the output does not see;
// this makes it a partition the output still does not see.
//
// **3 — concurrency is capped at the allocated buffer count, not the slot count**
// (F67's independent consequence, the bound nobody had written down).
// `RequestMtResource` allocates `uiThreadBsBufferNum` buffers, which is
// `min(iMultipleThreadIdc, MAX_THREADS_NUM)` — fewer than `MAX_THREADS_NUM` slots
// whenever the encoder was asked for fewer threads. It was the *pool's* concurrency
// cap that kept `QueryEmptyThread`'s answer below that; a spawn-per-slice would
// hand out a slot with a null buffer behind it the first time a fixed mode asked
// for more slices than threads, which the sweep does routinely (`t=2`, `sm=1 n=4`).
// The worker count here is `min(slices, uiThreadBsBufferNum)` and
// `SliceJobHandle::new` `debug_assert!`s the bound at construction.
unsafe impl Send for SliceJobHandle {}

impl SliceJobHandle {
    /// # Safety
    /// `pCtx` must be a live context whose `pSliceThreading` has been built by
    /// `RequestMtResource`, and `iBsSlot` must be a slot that call allocated.
    unsafe fn new(
        pCtx: *mut sWelsEncCtx,
        iBsSlot: i32,
        iFirstSlice: i32,
        iSliceStep: i32,
        iSliceCount: i32,
        bRecordsTime: bool,
    ) -> Self {
        // Part 3 of the safety argument, checked where the handle is made rather
        // than trusted where it is used. Both entry points refuse a null
        // `pSliceThreading` before they get here, so the deref is the same one the
        // workers will do.
        debug_assert!(
            iBsSlot >= 0 && (iBsSlot as usize) < (*(*pCtx).pSliceThreading).uiThreadBsBufferNum,
            "job slot {} is outside the {} allocated bs buffers — F67's bound",
            iBsSlot,
            (*(*pCtx).pSliceThreading).uiThreadBsBufferNum
        );
        debug_assert!(
            !(*(*pCtx).pSliceThreading).pThreadBsBuffer[iBsSlot as usize].is_null(),
            "job slot {iBsSlot} has a null buffer behind it"
        );
        Self { pCtx, iBsSlot, iFirstSlice, iSliceStep, iSliceCount, bRecordsTime }
    }
}

/// The prefix-NAL pair both encode bodies open with
/// (`CWelsBaseTask::WritePrefixNal`).
unsafe fn WritePrefixNalForSlice(
    pCtx: *mut sWelsEncCtx,
    pSlice: *mut SSlice,
    pSliceBs: *mut SWelsSliceBs,
    eNalRefIdc: EWelsNalRefIdc,
    eNalType: EWelsNalUnitType,
) {
    if eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        WelsLoadNalForSlice(pSliceBs, EWelsNalUnitType::NAL_UNIT_PREFIX as i32, eNalRefIdc as i32);
        WelsWriteSVCPrefixNal(
            thread_bs_buffer(pCtx, pSlice),
            std::ptr::addr_of_mut!((*pSliceBs).sBsWrite),
            eNalRefIdc as i32,
            EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR == eNalType,
        );
        WelsUnloadNalForSlice(pSliceBs);
    } else {
        // No Prefix NAL Unit RBSP syntax here, but need add NAL Unit Header extension
        WelsLoadNalForSlice(pSliceBs, EWelsNalUnitType::NAL_UNIT_PREFIX as i32, eNalRefIdc as i32);
        WelsUnloadNalForSlice(pSliceBs);
    }
}

/// What one slice's encode reports back through the join.
///
/// `bInitFailed` is not decoration. `CWelsSliceEncodingTask::Execute` returns
/// early when `InitTask` fails — **before** `FinishTask`, which is the only place
/// the task's result is ORed into `pCtx->iEncoderError`. So a failed init is
/// swallowed on both sides today, and reproducing that is hard rule 6: the frame
/// comes back short rather than as an error, and changing which one the caller
/// sees is a behaviour change. Carried here so the calling thread can OR exactly
/// what `FinishTask` would have.
struct SliceJobResult {
    iResult: i32,
    bInitFailed: bool,
}

/// One slice, start to finish, on a worker thread: the `InitTask` / `ExecuteTask`
/// / `FinishTask` triple of `CWelsSliceEncodingTask`, minus the two mutexes.
///
/// The slot claim is gone because the slot is the worker's for the whole scope
/// (part 1 of the seam's argument); the error mutex is gone because the result
/// travels back through the join instead of being ORed from the worker.
unsafe fn EncodeOneSliceInJob(
    pCtx: *mut sWelsEncCtx,
    iSliceIdx: i32,
    iBsSlot: i32,
    bRecordsTime: bool,
) -> SliceJobResult {
    // ---- CWelsSliceEncodingTask::InitTask
    let eNalType = (*pCtx).eNalType;
    let eNalRefIdc = (*pCtx).eNalPriority;
    let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

    let mut pSlice: *mut SSlice = std::ptr::null_mut();
    let mut iReturn = InitOneSliceInThread(
        pCtx,
        &mut pSlice,
        iBsSlot,
        (*pCtx).uiDependencyId as i32,
        iSliceIdx,
    );
    if iReturn != ENC_RETURN_SUCCESS {
        return SliceJobResult { iResult: iReturn, bInitFailed: true };
    }
    let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);

    iReturn = SetSliceBoundaryInfo(current_layer(pCtx), pSlice, iSliceIdx);
    if iReturn != ENC_RETURN_SUCCESS {
        return SliceJobResult { iResult: iReturn, bInitFailed: true };
    }
    (*pSliceBs).sBsWrite = BsWriter::new();
    let iSliceStart = if bRecordsTime { WelsTime() } else { 0 };

    // ---- CWelsSliceEncodingTask::ExecuteTask
    let iResult = (|| {
        if bNeedPrefix {
            WritePrefixNalForSlice(pCtx, pSlice, pSliceBs, eNalRefIdc, eNalType);
        }
        WelsLoadNalForSlice(pSliceBs, eNalType as i32, eNalRefIdc as i32);
        debug_assert_eq!(iSliceIdx, (*pSlice).iSliceIdx);
        let mut iReturn = WelsCodeOneSlice(pCtx, pSlice, eNalType as i32);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }
        WelsUnloadNalForSlice(pSliceBs);

        let mut iSliceSize = 0i32;
        iReturn = WriteSliceBs(pCtx, pSlice, iSliceIdx, &mut iSliceSize);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }

        let pfDeblockingFilterSlice =
            (*ctx_func_list(pCtx)).pfDeblocking.pfDeblockingFilterSlice.unwrap();
        pfDeblockingFilterSlice(current_layer(pCtx), ctx_func_list(pCtx), pSlice);
        ENC_RETURN_SUCCESS
    })();

    // ---- CWelsSliceEncodingTask::FinishTask (the load-balancing override's half;
    //      the base half was the slot release and the error OR, both now gone)
    if bRecordsTime && !pSlice.is_null() {
        (*pSlice).uiSliceConsumeTime = (WelsTime() - iSliceStart) as u32;
    }

    SliceJobResult { iResult, bInitFailed: false }
}

/// How many workers a fork gets: never more than there are slices to encode, and
/// never more than there are bs scratch buffers behind the slots (F67's bound).
unsafe fn ForkWidth(pCtx: *mut sWelsEncCtx, iItemCount: i32) -> i32 {
    let pSmt = (*pCtx).pSliceThreading;
    let iBuffers = if pSmt.is_null() { 1 } else { (*pSmt).uiThreadBsBufferNum as i32 };
    iItemCount.min(iBuffers.max(1)).max(1)
}

/// **The fork/join for every fixed slice mode** — what
/// `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)` did for
/// `uiSliceMode != SM_SIZELIMITED_SLICE`.
///
/// Returns the value `FinishTask` would have ORed into `pCtx->iEncoderError`; the
/// caller ORs it, exactly where it read the field before.
///
/// # Safety
/// `pCtx` must be a live context with `pSliceThreading` built and the layer's
/// slice bank sized for `kiSliceCount` slices.
pub unsafe fn EncodeFixedSlicesForked(pCtx: *mut sWelsEncCtx, kiSliceCount: i32) -> i32 {
    if pCtx.is_null() || kiSliceCount <= 0 || (*pCtx).pSliceThreading.is_null() {
        return ENC_RETURN_SUCCESS;
    }
    let bRecordsTime = !ctx_param(pCtx).is_null() && (*ctx_param(pCtx)).bUseLoadBalancing;
    let iWidth = ForkWidth(pCtx, kiSliceCount);

    // One handle per worker, each carrying its own slot — constructed here, on the
    // calling thread, so the slot bound is checked before anything spawns.
    let mut jobs: Vec<SliceJobHandle> = Vec::with_capacity(iWidth as usize);
    for k in 0..iWidth {
        jobs.push(SliceJobHandle::new(pCtx, k, k, iWidth, kiSliceCount, bRecordsTime));
    }

    let mut iErr = ENC_RETURN_SUCCESS;
    std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(jobs.len());
        for job in jobs {
            handles.push(s.spawn(move || {
                // `job` is the whole capture: the seam is this one move, and
                // nothing else crosses.
                let job = job;
                let mut iWorkerErr = ENC_RETURN_SUCCESS;
                let mut iSliceIdx = job.iFirstSlice;
                while iSliceIdx < job.iSliceCount {
                    let r = EncodeOneSliceInJob(
                        job.pCtx,
                        iSliceIdx,
                        job.iBsSlot,
                        job.bRecordsTime,
                    );
                    if !r.bInitFailed && r.iResult != ENC_RETURN_SUCCESS {
                        iWorkerErr |= r.iResult;
                    }
                    if r.bInitFailed {
                        // `Execute`'s early return: this task is over, and it
                        // reports nothing. The remaining slices of this worker
                        // were separate tasks and still run, as they would have.
                    }
                    iSliceIdx += job.iSliceStep;
                }
                iWorkerErr
            }));
        }
        // The join IS the barrier `WelsTaskBarrier` was.
        for h in handles {
            iErr |= h.join().unwrap_or(ENC_RETURN_UNEXPECTED);
        }
    });

    iErr
}

/// **The fork/join for the macroblock-map update** — what
/// `pTaskManage->InitFrame` dispatched as `WELS_ENC_TASK_UPDATEMBMAP`.
///
/// Ordering, and it is the C++'s: this is a *separate* fork/join that fully joins
/// before the encoding one starts. `InitFrame` runs at the top of
/// `WelsInitCurrentLayer`, hundreds of lines above the encode dispatch, and its
/// task list is drained by the same barrier before it returns. The two are not
/// fused here because fusing them would let a slice encode against a neighbour map
/// another worker had not finished writing.
///
/// It fires only when `bNeedAdjustingSlicing` is set, which only
/// `DynamicAdjustSlicing` does — so this path is reachable only with
/// `bUseLoadBalancing` on. See the step-5 ruling in the log.
///
/// # Safety
/// As [`EncodeFixedSlicesForked`].
pub unsafe fn UpdateMbMapForked(pCtx: *mut sWelsEncCtx, kiTaskCount: i32) {
    if pCtx.is_null() || kiTaskCount <= 0 || current_layer(pCtx).is_null()
        || (*pCtx).pSliceThreading.is_null()
    {
        return;
    }
    let iWidth = ForkWidth(pCtx, kiTaskCount);
    let mut jobs: Vec<SliceJobHandle> = Vec::with_capacity(iWidth as usize);
    for k in 0..iWidth {
        jobs.push(SliceJobHandle::new(pCtx, k, k, iWidth, kiTaskCount, false));
    }

    std::thread::scope(|s| {
        for job in jobs {
            s.spawn(move || {
                let job = job;
                let pCurDq = current_layer(job.pCtx);
                let pMbList = crate::encoder::svc_encode_slice::mb_list_root(pCurDq);
                let mut iSliceIdc = job.iFirstSlice;
                while iSliceIdc < job.iSliceCount {
                    UpdateMbListNeighborParallel(pCurDq, pMbList, iSliceIdc);
                    iSliceIdc += job.iSliceStep;
                }
            });
        }
    });
}
