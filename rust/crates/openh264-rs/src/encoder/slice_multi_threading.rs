#![deny(unsafe_code)]
#![forbid(unsafe_code)]
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
    unused_variables
)]

use std::sync::atomic::{AtomicI32, AtomicU16, Ordering};

use crate::encoder::nal_encap::{
    WelsEncodeNal, WelsLoadNalForSlice, WelsUnloadNalForSlice, WelsWriteSVCPrefixNal, SWelsNalRaw,
};
use crate::common::wels_common_defs::{EWelsNalRefIdc, EWelsNalUnitType};
use crate::encoder::svc_encode_slice::{
    current_layer_mut, current_layer_ref, InitOneSliceInThread, ReallocateSliceInThread,
    SetSliceBoundaryInfo,
    WelsCodeOneSlice,
};
use crate::encoder::vlc_encoder::BsWriter;
use crate::encoder::wels_encoder_ext::WelsTime;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;
use crate::encoder::svc_encode_slice::{set_current_layer, LayerIdx};
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
use crate::encoder::encoder_context::{
    
};
use crate::encoder::svc_encode_slice::current_layer_expect;

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

/// `WelsSetMemMultiplebytes_c` over the macroblock map, as a `fill` on a subslice
/// — `codec/common/inc/macros.h:312`.
#[inline]
pub fn fill_mb_map(map: &[AtomicU16], kiFirstMb: i32, kiCount: i32, uiValue: u16) {
    if kiFirstMb < 0 || kiCount <= 0 {
        return;
    }
    let a = kiFirstMb as usize;
    let b = a.saturating_add(kiCount as usize).min(map.len());
    if a < b {
        // `&[AtomicU16]` rather than `&mut [u16]`: `AddSliceBoundary` calls this
        // from inside the fork, where the other partitions are reading the map.
        for c in &map[a..b] {
            c.store(uiValue, Ordering::Relaxed);
        }
    }
}


// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug)]
pub struct SSliceThreading {
    /// The `iSliceNumInFrame` lock. `DynSlcJudgeSliceBoundaryStepBack` holds it
    /// across `AddSliceBoundary` and the increment, which is what the C++ does
    /// (`svc_encode_slice.cpp:1776-1791`).
    pub mutexSliceNumUpdate: std::sync::Mutex<()>,
    /// One bs scratch buffer per worker slot. The fork entries `mem::take` each
    /// worker's `Vec` out of this array before the spawn and restore it after the
    /// join, so no worker derives from the shared context at all.
    pub pThreadBsBuffer: [Vec<u8>; MAX_THREADS_NUM],
    /// How many of the `MAX_THREADS_NUM` slots actually have a buffer behind them:
    /// `min(iMultipleThreadIdc, MAX_THREADS_NUM)`. This is where the fork reads it
    /// (`ForkWidth`).
    pub uiThreadBsBufferNum: usize,
}

impl Default for SSliceThreading {
    fn default() -> Self {
        Self {
            mutexSliceNumUpdate: std::sync::Mutex::new(()),
            pThreadBsBuffer: std::array::from_fn(|_| Vec::new()),
            uiThreadBsBufferNum: 0,
        }
    }
}
pub type TagSliceThreading = SSliceThreading;

pub type TagWelsSliceBs = SWelsSliceBs;


pub type TagSlice = SSlice;

// The element type is `AtomicU16`: `AddSliceBoundary` rewrites this map from
// inside the fork under `SM_SIZELIMITED_SLICE` while the other partitions'
// workers read it — one entry per macroblock, partitions disjoint, nothing
// synchronising the storage itself. `Relaxed` throughout: the disjointness is the
// argument, and the scope join is the publication edge.
#[derive(Debug)]
pub struct SSliceCtx {
    pub uiSliceMode: SliceMode,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    /// `DynSlcJudgeSliceBoundaryStepBack` increments this from inside the fork
    /// under `mutexSliceNumUpdate`, but every *reader* takes no lock:
    /// `WelsGetNextMbOfSlice` reborrows the whole `SSliceCtx` per macroblock on
    /// each worker. The mutex is *not* redundant: it brackets `AddSliceBoundary`'s
    /// map rewrite *with* the increment, which no single atomic can do.
    pub iSliceNumInFrame: AtomicI32,
    pub iMbNumInFrame: i32,
    /// One slice index per macroblock, in raster order. Allocated by
    /// `InitSlicePEncCtx`, released by the layer's `Drop` where `UninitSlicePEncCtx`
    /// used to free it.
    pub pOverallMbMap: Vec<AtomicU16>,
    pub uiSliceSizeConstraint: u32,
    pub iMaxSliceNumConstraint: i32,
}

impl Default for SSliceCtx {
    fn default() -> Self {
        Self {
            uiSliceMode: SliceMode::SM_SINGLE_SLICE,
            iMbWidth: 0,
            iMbHeight: 0,
            iSliceNumInFrame: AtomicI32::new(0),
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
// A `std::sync::Mutex` cannot be locked and unlocked through two separate calls
// the way pthreads can — the guard owns the lock — so the lock/unlock pair is
// expressed as one scoped call. Every C++ lock/unlock pair in the encoder
// brackets a single straight-line region, so the critical sections are
// identical; only the spelling differs.

/// Runs `f` holding `pMutex`, i.e. a `WelsMutexLock`/`WelsMutexUnlock` pair.
///
/// A null handle runs `f` unlocked; that mirrors the C++ behaviour on an
/// uninitialised mutex closely enough for the single-threaded paths, which
/// never contend.
pub fn with_wels_mutex<R>(pMutex: Option<&std::sync::Mutex<()>>, f: impl FnOnce() -> R) -> R {
    let Some(m) = pMutex else {
        return f();
    };
    // A worker that panicked mid-slice leaves the mutex poisoned; the encoder
    // has no recovery path for that, so take the guard either way.
    let _guard = m.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

// ============================================================================
// Core Multithreading Functions
// ============================================================================

#[inline]

/// Updates macroblock spatial neighbor availability bitmasks for all macroblocks
/// belonging to a specific slice partition in parallel. The window is the
/// caller's — carved out of the grid before the fork by [`UpdateMbMapForked`].
pub fn UpdateMbListNeighborParallel(
    mbs: &mut crate::safe::mb_grid::MbWindow<'_, SMB>,
    pSliceCtx: &crate::encoder::svc_encode_slice::SSliceCtx,
    kiMbWidth: i32,
    kiSliceIdc: i32,
    kiFirst: i32,
    kiCount: i32,
) {
    let kiEndMbInSlice = kiFirst + kiCount - 1;
    let mut iIdx = kiFirst;
    while iIdx <= kiEndMbInSlice {
        crate::encoder::svc_encode_slice::UpdateMbNeighbor(
            Some(pSliceCtx),
            mbs.at_mut(iIdx as usize),
            kiMbWidth,
            kiSliceIdc as u16,
        );
        iIdx += 1;
    }
}

/// Calculates the normalized computational complexity ratio (`iSliceComplexRatio`)
/// for each slice in a spatial layer based on measured CPU consumption time.
///
/// The producer half of the load-balancing loop. Called from
/// `WelsEncoderEncodeExt`, at the end of the per-layer body, under the C++'s own
/// four-term guard — the site is `encoder_ext.cpp:4064-4073`.
pub fn CalcSliceComplexRatio(pCurDq: &mut SDqLayer) {
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let mut iSumAv = 0i32;
    let kiSliceCount = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let mut iSliceIdx = 0i32;
    let mut iAvI = [0i32; MAX_SLICES_NUM];

    if kiSliceCount > MAX_SLICES_NUM as i32 {
        return;
    }
    while iSliceIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx);
        if let Some(pSlice) = pSlice {
            let consume_time = (*pSlice).uiSliceConsumeTime as i32;
            let mb_num = (*pSlice).iCountMbNumInSlice;
            iAvI[iSliceIdx as usize] = WelsDivRound(INT_MULTIPLY * mb_num, consume_time);
            iSumAv += iAvI[iSliceIdx as usize];
        }
        iSliceIdx += 1;
    }

    while iSliceIdx > 0 {
        iSliceIdx -= 1;
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx);
        if let Some(pSlice) = pSlice {
            (*pSlice).iSliceComplexRatio =
                WelsDivRound(INT_MULTIPLY * iAvI[iSliceIdx as usize], iSumAv);
        }
    }
}

/// Statistical decision engine that evaluates whether the timing variance across
/// slices exceeds the core-dependent threshold to justify dynamic slicing.
pub fn NeedDynamicAdjust(pCurDq: &mut SDqLayer, iSliceNum: i32) -> i32 {
    if iSliceNum <= 0 {
        return 0;
    }

    let mut uiTotalConsume: u32 = 0;
    let mut iSliceIdx: i32 = 0;
    let mut iNeedAdj: i32 = 0;

    while iSliceIdx < iSliceNum {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx)
        else {
            return 0;
        };
        uiTotalConsume += pSlice.uiSliceConsumeTime;
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
        // Absence was never a handled state, and the loop above has already
        // walked the same range.
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx)
            .expect("the layer's slice bank maps this slice index");
        let fRatio = pSlice.uiSliceConsumeTime as f32 / (uiTotalConsume as f32);
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
pub fn DynamicAdjustSlicing(
    pSvcParam: &crate::encoder::param_svc::SWelsSvcCodingParam,
    kpRc: &[crate::encoder::rc::SWelsSvcRc],
    pCurDqLayer: &mut SDqLayer,
    iCurDid: i32,
) {
    let pSliceCtx = &mut (*pCurDqLayer).sSliceEncCtx;
    let kiCountSliceNum = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let kiCountNumMb = pSliceCtx.iMbNumInFrame;
    let mut iMinimalMbNum = pSliceCtx.iMbWidth as i32;
    let mut iMaximalMbNum;
    let mut iMbNumLeft = kiCountNumMb;
    let mut iRunLen = [0i32; MAX_THREADS_NUM];
    let mut iSliceIdx: i32;

    let rc_mode = pSvcParam.iRCMode;
    let mut iNumMbInEachGom = 0i32;
    if rc_mode != RCMode::RC_OFF_MODE {
        if kpRc.is_empty() {
            return;
        }
        iNumMbInEachGom = kpRc[iCurDid as usize].iNumberMbGom;

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
    iSliceIdx = 0;
    while iSliceIdx + 1 < kiCountSliceNum {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDqLayer, iSliceIdx) else {
            return;
        };
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

    let ret = DynamicAdjustSlicePEncCtxAll(pCurDqLayer, &iRunLen);
    (*pCurDqLayer).bNeedAdjustingSlicing = ret == 0;
}

/// Applies newly calculated macroblock run-lengths to slice context structures.
pub fn DynamicAdjustSlicePEncCtxAll(pCurDq: &mut SDqLayer, pRunLength: &[i32]) -> i32 {
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let iCountNumMbInFrame = pSliceCtx.iMbNumInFrame;
    let iCountSliceNumInFrame = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let mut iSameRunLenFlag = 1i32;
    let mut iFirstMbIdx = 0i32;
    let mut iSliceIdx = 0i32;

    while iSliceIdx < iCountSliceNumInFrame {
        let first: &[i32] = &(*pCurDq).pFirstMbIdxOfSlice;
        if pRunLength[iSliceIdx as usize] != first[iSliceIdx as usize] {
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
        let kiSliceRun = pRunLength[iSliceIdx as usize];
        {
            let first: &mut Vec<i32> = &mut (*pCurDq).pFirstMbIdxOfSlice;
            first[iSliceIdx as usize] = iFirstMbIdx;
            let count: &mut Vec<i32> = &mut (*pCurDq).pCountMbNumInSlice;
            count[iSliceIdx as usize] = kiSliceRun;
        }

        {
            let map: &[AtomicU16] = &pSliceCtx.pOverallMbMap;
            fill_mb_map(map, iFirstMbIdx, kiSliceRun, iSliceIdx as u16);
        }

        iFirstMbIdx += kiSliceRun;
        iSliceIdx += 1;
    }

    0
}

/// Allocates and initializes multithreading synchronization resources and thread-local bitstream buffers.
pub fn RequestMtResource(
    ctx: &mut sWelsEncCtx,
    iCountBsLen: i32,
    _iMaxSliceBufferSize: i32,
    bDynamicSlice: bool,
) -> i32 {
    if iCountBsLen <= 0 {
        return 1;
    }

    let iThreadNum = ctx.param().iMultipleThreadIdc as i32;

    if iThreadNum <= 0 {
        return 1;
    }

    let mut pSmt = Box::new(SSliceThreading::default());

    // The fork is as wide as the buffers, so the buffers are counted from
    // `iMultipleThreadIdc`, already clipped to `[1, MAX_THREADS_NUM]` by
    // `ParamValidationExt`.
    let iThreadBufferNum = (iThreadNum as usize).min(MAX_THREADS_NUM);
    let _ = bDynamicSlice;

    pSmt.uiThreadBsBufferNum = iThreadBufferNum;
    for i in 0..iThreadBufferNum {
        pSmt.pThreadBsBuffer[i] = vec![0u8; iCountBsLen as usize];
    }
    ctx.pSliceThreading = Some(pSmt);

    0
}

/// Tears down and frees all multithreading objects and bitstream buffers.
pub fn ReleaseMtResource(ctx: &mut sWelsEncCtx) {
    // The take is the whole teardown: the box drops at the end of this function,
    // its `Vec`s and the mutex with it.
    let Some(pSmt) = ctx.pSliceThreading.take() else {
        return;
    };

    drop(pSmt);
}

/// Aggregates individual thread-local slice bitstream buffers into the contiguous frame bitstream buffer.
pub fn AppendSliceToFrameBs(
    pCtx: &mut sWelsEncCtx,
    pLbi: &mut SLayerBSInfo,
    kiSliceCount: i32,
) -> i32 {
    if current_layer_ref(pCtx).is_none() {
        return 0;
    }

    let sWelsEncCtx {
        ppDqLayerList, iCurDqLayer, pFrameBs, iPosBsBuffer, iFrameBsSize, iEncoderError, pOut, ..
    } = &mut *pCtx;
    // The NAL lengths this walk distributes are entries of `pOut.sNalLen`, and
    // the C-ABI pointer on the record is the reslice of it the application reads.
    let crate::encoder::nal_encap::SWelsEncoderOutput { sNalLen, iNalLenBase, .. } =
        &mut **pOut.as_mut().expect("pOut lives");
    let Some(pCurDq) = iCurDqLayer
        .and_then(|idx| ppDqLayerList.get_mut(idx.get()))
        .and_then(|l| l.as_deref_mut())
    else {
        return 0;
    };
    let mut iLayerSize = 0i32;
    let mut iNalIdxBase = 0i32;
    (*pLbi).iNalCount = 0;

    let mut iSliceIdx = 0i32;
    while iSliceIdx < kiSliceCount {
        let Some(pSlice) = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx) else {
            iSliceIdx += 1;
            continue;
        };
        {
            let pSliceBs = &mut pSlice.sSliceBs;
            if pSliceBs.uiBsPos > 0 {
                let iCountNal = pSliceBs.iNalIndex;

                if (*iPosBsBuffer as u64) + (pSliceBs.uiBsPos as u64)
                    > (*iFrameBsSize as u64)
                {
                    *iEncoderError |= ENC_RETURN_MEMALLOCERR;
                    return 0;
                }

                if !pFrameBs.is_empty() {
                    if let Some(src) = pSliceBs.pBs.as_ref() {
                        let kiPos = *iPosBsBuffer as usize;
                        let kiLen = pSliceBs.uiBsPos as usize;
                        pFrameBs[kiPos..kiPos + kiLen]
                            .copy_from_slice(&src[..kiLen]);
                    }
                }

                *iPosBsBuffer += pSliceBs.uiBsPos as i32;
                iLayerSize += pSliceBs.uiBsPos as i32;

                // `pNalLengthInByte` is the reslice of `pOut.sNalLen` at this
                // layer's base, so the slot is that base plus the running NAL
                // index.
                let mut iNalIdx = 0i32;
                while iNalIdx < iCountNal {
                    let kiSlot = *iNalLenBase + (iNalIdxBase + iNalIdx).max(0) as usize;
                    if kiSlot < sNalLen.len() {
                        sNalLen[kiSlot]
                            .store(pSliceBs.iNalLen[iNalIdx as usize], std::sync::atomic::Ordering::Relaxed);
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
/// that buffer is `pThreadBsBuffer[pSlice->uiBufferIdx]`.
pub fn WriteSliceBs(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    _iSliceIdx: i32,
    iSliceSize: &mut i32,
    pSliceBsBuf: &[u8],
) -> i32 {
    if current_layer_ref(pCtx).is_none() {
        return 0;
    }
    let pSliceBs = &mut pSlice.sSliceBs;

    let kiNalCnt = pSliceBs.iNalIndex;
    let mut iNalIdx = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;
    let iTotalLeftLength = (pSliceBs.uiBsSize - pSliceBs.uiBsPos) as i32;
    let kpNalHdrExt = &current_layer_expect(pCtx).sLayerInfo.sNalHeaderExt;
    // The write cursor is the slice's own buffer, absent when the slice shares
    // the frame's; `WelsEncodeNal` takes the `INVALIDINPUT` arm for `None`
    // exactly as the C++ did for null.
    let bHasOwnBuffer = pSliceBs.pBs.is_some();
    let mut iDstPos = 0usize;

    if kiNalCnt > 2 {
        return 0;
    }

    *iSliceSize = 0;
    while iNalIdx < kiNalCnt {
        let mut iNalSize = 0i32;
        // The slice's own buffer is re-sliced from the running offset each
        // iteration; `iTotalLeftLength - *iSliceSize` is the same bound said
        // as a number, and the two agree by construction because `uiBsSize` is
        // that buffer's length.
        let kNalEntry = pSliceBs.sNalList[iNalIdx as usize];
        let kiLeft = (iTotalLeftLength - *iSliceSize).max(0) as usize;
        let pDstTail = if bHasOwnBuffer {
            let buf = pSliceBs.pBs.as_mut().expect("checked above");
            let kiEnd = (iDstPos + kiLeft).min(buf.len());
            Some(&mut buf[iDstPos.min(kiEnd)..kiEnd])
        } else {
            None
        };
        iReturn = WelsEncodeNal(
            &kNalEntry,
            pSliceBsBuf,
            Some(kpNalHdrExt),
            pDstTail,
            &mut iNalSize,
        );

        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }

        pSliceBs.iNalLen[iNalIdx as usize] = iNalSize;
        *iSliceSize += iNalSize;
        if bHasOwnBuffer {
            iDstPos += iNalSize as usize;
        }
        iNalIdx += 1;
    }
    pSliceBs.uiBsPos = *iSliceSize as u32;

    iReturn
}

/// Queries the operating system for the number of active logical CPU processing cores.
pub fn DynamicDetectCpuCores() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(1)
}

/// Evaluates load balance and dynamically adjusts slicing for the base spatial dependency layer.
pub fn AdjustBaseLayer(pCtx: &mut sWelsEncCtx) -> i32 {
    if crate::encoder::encoder_context::dq_layer_mut(pCtx, 0).is_none() {
        return 0;
    }

    set_current_layer(pCtx, Some(LayerIdx(0)));

    let pCurDq = crate::encoder::encoder_context::dq_layer_mut(pCtx, 0).expect("checked above");
    let kiSliceNumInFrame = pCurDq.sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let iNeedAdj = NeedDynamicAdjust(pCurDq, kiSliceNumInFrame);

    if iNeedAdj != 0 {
        let sWelsEncCtx { pSvcParam, pWelsSvcRc, ppDqLayerList, .. } = &mut *pCtx;
        let Some(pSvcParam) = pSvcParam.as_deref() else {
            return iNeedAdj;
        };
        let Some(pCurDq) = ppDqLayerList.get_mut(0).and_then(|l| l.as_deref_mut()) else {
            return iNeedAdj;
        };
        DynamicAdjustSlicing(pSvcParam, pWelsSvcRc, pCurDq, 0);
    }

    iNeedAdj
}

/// Evaluates load balance and dynamically adjusts slicing for spatial enhancement layers.
pub fn AdjustEnhanceLayer(pCtx: &mut sWelsEncCtx, iCurDid: i32) -> i32 {
    if crate::encoder::encoder_context::dq_layer_ref(pCtx, 0).is_none() || current_layer_ref(pCtx).is_none() {
        return 0;
    }

    if pCtx.param_opt().is_none() {
        return 0;
    }
    let kPrevSliceArg = if iCurDid > 0 && (iCurDid as usize - 1) < MAX_SPATIAL_LAYER_NUM {
        let a = &pCtx.param().sSpatialLayers[iCurDid as usize - 1].sSliceArgument;
        Some((a.uiSliceMode, a.uiSliceNum))
    } else {
        None
    };
    let kiMultipleThreadIdc = pCtx.param().iMultipleThreadIdc;

    let kbModelingFromSpatial = current_layer_expect(pCtx).pRefLayer.is_some()
        && match kPrevSliceArg {
            Some((uiSliceMode, uiSliceNum)) => {
                uiSliceMode == SliceMode::SM_FIXEDSLCNUM_SLICE
                    && kiMultipleThreadIdc as u32 >= uiSliceNum
            }
            None => false,
        };

    let iNeedAdj: i32;
    if kbModelingFromSpatial {
        // The two names can be the same layer (base == current when `iCurDid`
        // is the base), so the load is hoisted above the exclusive borrow.
        let kiSliceNumInFrame =
            current_layer_expect(pCtx).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
        let Some(pBaseLayer) = crate::encoder::encoder_context::dq_layer_mut(pCtx, iCurDid as usize - 1) else {
            return 0;
        };
        iNeedAdj = NeedDynamicAdjust(pBaseLayer, kiSliceNumInFrame);
        if iNeedAdj != 0 {
            let sWelsEncCtx { pSvcParam, pWelsSvcRc, ppDqLayerList, iCurDqLayer, .. } = &mut *pCtx;
            let Some(pSvcParam) = pSvcParam.as_deref() else {
                return iNeedAdj;
            };
            let Some(pCurLayer) = iCurDqLayer
                .and_then(|idx| ppDqLayerList.get_mut(idx.get()))
                .and_then(|l| l.as_deref_mut())
            else {
                return iNeedAdj;
            };
            DynamicAdjustSlicing(pSvcParam, pWelsSvcRc, pCurLayer, iCurDid);
        }
    } else {
        let kiSliceNumInFrame =
            current_layer_expect(pCtx).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
        let pCurLayer = crate::encoder::encoder_context::dq_layer_mut(pCtx, iCurDid as usize)
            .expect("the dependency layer is built");
        iNeedAdj = NeedDynamicAdjust(pCurLayer, kiSliceNumInFrame);
        if iNeedAdj != 0 {
            let sWelsEncCtx { pSvcParam, pWelsSvcRc, ppDqLayerList, iCurDqLayer, .. } = &mut *pCtx;
            let Some(pSvcParam) = pSvcParam.as_deref() else {
                return iNeedAdj;
            };
            let Some(pCurLayer) = iCurDqLayer
                .and_then(|idx| ppDqLayerList.get_mut(idx.get()))
                .and_then(|l| l.as_deref_mut())
            else {
                return iNeedAdj;
            };
            DynamicAdjustSlicing(pSvcParam, pWelsSvcRc, pCurLayer, iCurDid);
        }
    }

    iNeedAdj
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

    /// Two workers write distinct patterns into their own taken slots while
    /// both hold a shared borrow of the pool's owner, and the patterns must
    /// land back in the owner's own slots.
    #[test]
    fn bs_pool_partition_carves_disjoint_slots_and_restores_them() {
        const WORKERS: usize = 2;
        const LEN: usize = 64;

        let mut pSmt = SSliceThreading::default();
        for k in 0..WORKERS {
            pSmt.pThreadBsBuffer[k] = vec![0u8; LEN];
        }
        pSmt.uiThreadBsBufferNum = WORKERS;

        // The take — the fork entries' partition.
        let mut vTakenBsBufs: Vec<Vec<u8>> = (0..WORKERS)
            .map(|k| std::mem::take(&mut pSmt.pThreadBsBuffer[k]))
            .collect();

        {
            let pSmtShared: &SSliceThreading = &pSmt;
            std::thread::scope(|s| {
                for (k, buf) in vTakenBsBufs.iter_mut().enumerate() {
                    s.spawn(move || {
                        // Production's shape: a shared borrow of the owner held
                        // beside this worker's `&mut` slot, both live across
                        // the writes.
                        assert_eq!(pSmtShared.uiThreadBsBufferNum, WORKERS);
                        for (i, b) in buf.iter_mut().enumerate() {
                            *b = (k as u8) ^ (i as u8);
                        }
                        assert_eq!(pSmtShared.uiThreadBsBufferNum, WORKERS);
                    });
                }
            });
        }

        // The restore — buffer `k` to slot `k`.
        for (k, buf) in vTakenBsBufs.into_iter().enumerate() {
            pSmt.pThreadBsBuffer[k] = buf;
        }

        for k in 0..WORKERS {
            for i in 0..LEN {
                assert_eq!(
                    pSmt.pThreadBsBuffer[k][i],
                    (k as u8) ^ (i as u8),
                    "slot {k} byte {i} did not come back from its own worker"
                );
            }
        }
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
        let ret = NeedDynamicAdjust(&mut dq_layer, 2);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_calc_slice_complex_ratio() {
        let mut dq_layer = layer_with_bank(2);
        for slice in dq_layer.sSliceBufferInfo[0].pSliceBuffer.iter_mut() {
            slice.iCountMbNumInSlice = 100;
            slice.uiSliceConsumeTime = 1000;
        }
        dq_layer.sSliceEncCtx.iSliceNumInFrame.store(2, Ordering::Relaxed);

        CalcSliceComplexRatio(&mut dq_layer);

        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[0].iSliceComplexRatio, 50);
        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[1].iSliceComplexRatio, 50);
    }

    /// Runs the whole encoder with `bUseLoadBalancing` on, four threads and four
    /// slices — the exact four-term guard `WelsEncoderEncodeExt` tests before it
    /// calls the producer — for enough frames that frame N+1's boundaries are
    /// computed from frame N's measured times.
    ///
    /// It asserts structure and never bytes, and it cannot do otherwise: the
    /// boundaries this path produces are a function of wall-clock encode times, so
    /// two runs of the **C++** disagree with each other; there is no reference to
    /// compare against.
    ///
    /// **256x192 is forced, not chosen.** `MIN_NUM_MB_PER_SLICE` is 48, and
    /// `SliceArgumentValidationFixedSliceMode` silently rewrites a request it cannot
    /// honour down to a mode that needs no threads — so four slices need at least
    /// 4 x 48 = 192 macroblocks, and a 16x12 grid is exactly 192. The
    /// `vcl_nals == 4` assertion is what would catch the rewrite: on a smaller
    /// picture every other assertion here passes while the encoder runs
    /// single-slice, single-threaded, and the load-balancing path stays dark.
    ///
    /// Ignored under Miri: 192 macroblocks x 4 frames x 4 threads is roughly eight
    /// times the work of the fork/join probe in `svc_encode_slice.rs`, which is
    /// itself the most expensive test in the battery.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_balancing_completes_frames_with_sane_slice_counts() {
        use crate::api::codec_api::abi_test_driver::{EncoderProbeOptions, drive_encoder_over};

        let (frames, dims) = drive_encoder_over(
            256,
            192,
            4,
            EncoderProbeOptions {
                slice_mode: crate::api::codec_api::SliceModeEnum::SM_FIXEDSLCNUM_SLICE,
                slice_num: 4,
                threads: 4,
                load_balancing: true,
                ..EncoderProbeOptions::default()
            },
        );

        assert_eq!(dims, (256, 192), "the encoder must be configured for a 16x12 grid");
        assert_eq!(frames.len(), 4, "the encode loop did not run to the end");
        assert!(
            frames.iter().all(|f| f.bytes > 0),
            "a frame produced no NAL bytes, which is what a lost slice looks like from \
             here: {:?}",
            frames.iter().map(|f| (f.kind, f.bytes)).collect::<Vec<_>>()
        );
        // The assertion that keeps the test on the path it names. `DynamicAdjustSlicing`
        // redistributes macroblocks *across* the slices; it never changes how many there
        // are. Four every frame, or either the request was rewritten or a rebalance lost
        // one.
        assert!(
            frames.iter().all(|f| f.vcl_nals == 4),
            "a frame did not carry four VCL NALs, so the slice count moved under the \
             rebalance or the mode was rewritten: {:?}",
            frames.iter().map(|f| (f.kind, f.vcl_nals)).collect::<Vec<_>>()
        );
        assert_eq!(
            frames[0].kind,
            crate::api::codec_api::EVideoFrameType::videoFrameTypeIDR,
            "the sequence must open on an IDR"
        );
        assert!(
            frames[1..]
                .iter()
                .all(|f| f.kind == crate::api::codec_api::EVideoFrameType::videoFrameTypeP),
            "frames 1..4 must be inter-coded, or the rebalance never sees a second \
             frame's times: {:?}",
            frames.iter().map(|f| f.kind).collect::<Vec<_>>()
        );
    }
}

/// One worker's share of a frame's slices, and the one value in this crate that
/// crosses a spawn.
///
/// The worker owns bs scratch slot `iBsSlot` for the whole scope and encodes the
/// slices `iFirstSlice`, `iFirstSlice + iSliceStep`, ... below `iSliceCount`.
pub struct SliceJobHandle<'a> {
    /// The encoder context.
    pCtx: &'a sWelsEncCtx,
    /// This worker's bs scratch bytes — `pThreadBsBuffer[iBsSlot]`'s buffer,
    /// taken out of the context before the fork and handed here. Restored to its
    /// slot after the join.
    pBsBuf: &'a mut [u8],
    /// This worker's slices. Worker `k` codes slices `k, k+step, k+2*step, …`, so
    /// the bank is distributed by that rule on the calling thread while the
    /// layer is `&mut`, and each worker holds `&mut SSlice`s **no sibling can
    /// name**.
    pSlices: Vec<&'a mut SSlice>,
    /// This worker's macroblock windows. One per slice this worker codes (fixed
    /// modes, paired 1:1 with `pSlices`), or the single partition run
    /// (size-limited). Each window is a disjoint `&mut [SMB]` peeled off the
    /// taken grid, and the coding chain and the deblocking walker both write
    /// through it.
    pMbs: Vec<crate::safe::mb_grid::MbWindow<'a, SMB>>,
    /// This worker's CABAC restore scratch: `pDynamicBsBuffer[k]`, taken from the
    /// context before the fork like everything else here; `None` for the fixed
    /// modes, which never stash-restore across a boundary.
    pDynBsBuf: Option<&'a mut [u8]>,
    /// This worker's slice bank, owned for the frame. Taken from the layer before
    /// the spawn; the size-limited job grows it and resolves every slice by
    /// index. `None` for the fixed modes, whose slices arrive already carved
    /// (`pSlices`).
    pBank: Option<&'a mut crate::encoder::svc_encode_slice::SSliceBufferInfo>,
    /// This worker's bs scratch slot index — still carried because the slice's
    /// `uiBufferIdx` must agree with the buffer above (asserted in the job).
    iBsSlot: i32,
    iFirstSlice: i32,
    iSliceStep: i32,
    iSliceCount: i32,
    /// `CWelsLoadBalancingSlicingEncodingTask`'s two extra stamps — the start time
    /// in `InitTask`, the elapsed time in `FinishTask`. False is
    /// `CWelsSliceEncodingTask`, which is what `bUseLoadBalancing = false` builds.
    bRecordsTime: bool,
}

impl<'a> SliceJobHandle<'a> {
    /// `pCtx`'s `pSliceThreading` must have been built by `RequestMtResource`,
    /// `iBsSlot` must be a slot that call allocated, and `pBsBuf` must be that
    /// slot's taken buffer (the fork entries' partition is the only maker).
    fn new(
        pCtx: &'a sWelsEncCtx,
        pBsBuf: &'a mut [u8],
        pSlices: Vec<&'a mut SSlice>,
        pMbs: Vec<crate::safe::mb_grid::MbWindow<'a, SMB>>,
        pDynBsBuf: Option<&'a mut [u8]>,
        pBank: Option<&'a mut crate::encoder::svc_encode_slice::SSliceBufferInfo>,
        iBsSlot: i32,
        iFirstSlice: i32,
        iSliceStep: i32,
        iSliceCount: i32,
        bRecordsTime: bool,
    ) -> Self {
        // Both entry points refuse a null `pSliceThreading` before they get
        // here; what is checked here is the slot bound.
        let pSmt = pCtx
            .pSliceThreading
            .as_deref()
            .expect("both fork entry points refuse a null pSliceThreading");
        debug_assert!(
            iBsSlot >= 0 && (iBsSlot as usize) < pSmt.uiThreadBsBufferNum,
            "job slot {} is outside the {} allocated bs buffers — F67's bound",
            iBsSlot,
            pSmt.uiThreadBsBufferNum
        );
        Self { pCtx, pBsBuf, pSlices, pMbs, pDynBsBuf, pBank, iBsSlot, iFirstSlice, iSliceStep, iSliceCount, bRecordsTime }
    }
}

/// The prefix-NAL pair both encode bodies open with
/// (`CWelsBaseTask::WritePrefixNal`).
fn WritePrefixNalForSlice(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    eNalRefIdc: EWelsNalRefIdc,
    eNalType: EWelsNalUnitType,
    pSliceBsBuf: &mut [u8],
) {
    if eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        WelsLoadNalForSlice(
            &mut pSlice.sSliceBs,
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            eNalRefIdc as i32,
        );
        let buf = pSliceBsBuf;
        WelsWriteSVCPrefixNal(
            buf,
            &mut pSlice.sSliceBs.sBsWrite,
            eNalRefIdc as i32,
            EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR == eNalType,
        );
        WelsUnloadNalForSlice(&mut pSlice.sSliceBs);
    } else {
        // No Prefix NAL Unit RBSP syntax here, but need add NAL Unit Header extension
        WelsLoadNalForSlice(
            &mut pSlice.sSliceBs,
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            eNalRefIdc as i32,
        );
        WelsUnloadNalForSlice(&mut pSlice.sSliceBs);
    }
}

/// What one slice's encode reports back through the join.
///
/// `bInitFailed` is not decoration. `CWelsSliceEncodingTask::Execute` returns
/// early when `InitTask` fails — **before** `FinishTask`, which is the only place
/// the task's result is ORed into `pCtx->iEncoderError`. So a failed init is
/// swallowed on both sides today: the frame comes back short rather than as an
/// error, and changing which one the caller sees is a behaviour change. Carried
/// here so the calling thread can OR exactly what `FinishTask` would have.
struct SliceJobResult {
    iResult: i32,
    bInitFailed: bool,
}

/// One slice, start to finish, on a worker thread: the `InitTask` / `ExecuteTask`
/// / `FinishTask` triple of `CWelsSliceEncodingTask`, minus the two mutexes.
fn EncodeOneSliceInJob(
    pCtx: &sWelsEncCtx,
    iSliceIdx: i32,
    iBsSlot: i32,
    bRecordsTime: bool,
    pSlotBuf: &mut [u8],
    pSlices: &mut [&mut SSlice],
    // This worker's macroblock windows, one per slice it codes, indexed exactly
    // as `pSlices` is.
    pMbs: &mut [crate::safe::mb_grid::MbWindow<'_, SMB>],
    iFirstSlice: i32,
    iSliceStep: i32,
) -> SliceJobResult {
    // ---- CWelsSliceEncodingTask::InitTask
    let eNalType = (*pCtx).eNalType;
    let eNalRefIdc = (*pCtx).eNalPriority;
    let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

    // The slice comes from this worker's own list: `iSliceIdx` advances by
    // `iSliceStep` from `iFirstSlice`, so this worker's `n`th slice is at
    // position `(iSliceIdx - iFirstSlice) / iSliceStep` in its list.
    let kiLocal = ((iSliceIdx - iFirstSlice) / iSliceStep) as usize;
    let Some(pSlice) = pSlices.get_mut(kiLocal) else {
        return SliceJobResult { iResult: ENC_RETURN_UNEXPECTED, bInitFailed: true };
    };
    InitOneSliceInThread(pCtx, pSlice, iBsSlot, iSliceIdx);
    let iReturn = SetSliceBoundaryInfo(current_layer_ref(pCtx), pSlice, iSliceIdx);
    if iReturn != ENC_RETURN_SUCCESS {
        return SliceJobResult { iResult: iReturn, bInitFailed: true };
    }
    pSlice.sSliceBs.sBsWrite = BsWriter::new();
    let iSliceStart = if bRecordsTime { WelsTime() } else { 0 };

    // The slice's bitstream buffer is the job's partition slot, subsliced to the
    // claimed size. The subslice length is the claimed size every deep call used
    // to pass to `thread_bs_buffer`; `uiSize` cannot exceed the slot (both are
    // `iCountBsLen`, single writer).
    // The `pOut` writer option is `None` on this side: every slice of a forked
    // layer has its own writer.
    debug_assert_eq!(pSlice.uiBufferIdx as i32, iBsSlot, "the slice's claimed slot is this job's");
    let kuiSize = pSlice.sSliceBs.uiSize;
    let pSliceBsBuf = &mut pSlotBuf[..kuiSize as usize];
    let mut pCtxOutBs: Option<&mut BsWriter> = None;

    // ---- CWelsSliceEncodingTask::ExecuteTask
    let iResult = (|| {
        if bNeedPrefix {
            WritePrefixNalForSlice(pCtx, pSlice, eNalRefIdc, eNalType, &mut *pSliceBsBuf);
        }
        WelsLoadNalForSlice(&mut pSlice.sSliceBs, eNalType as i32, eNalRefIdc as i32);
        debug_assert_eq!(iSliceIdx, pSlice.iSliceIdx);
        let pMbRun = &mut pMbs[kiLocal];
        // `None` restore scratch: the fixed loops never use it. `None`
        // next-slice too — the fixed modes never hit the dynamic boundary, so
        // `AddSliceBoundary` never fires here.
        let mut iReturn = WelsCodeOneSlice(pCtx, pSlice, eNalType as i32, &mut *pSliceBsBuf, &mut pCtxOutBs, pMbRun, None, None);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }
        WelsUnloadNalForSlice(&mut pSlice.sSliceBs);

        let mut iSliceSize = 0i32;
        iReturn = WriteSliceBs(pCtx, pSlice, iSliceIdx, &mut iSliceSize, &*pSliceBsBuf);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }

        let pfDeblockingFilterSlice =
            (*pCtx).func_list().pfDeblocking.pfDeblockingFilterSlice.unwrap();
        {
            // The walker's window is the worker's own carved run, the same one
            // the coding chain just wrote through. `uiFilterIdc == 1` (MT
            // validation rewrites idc 0 → 2, `encoder_ext.rs:1506`) confines
            // walk and neighbour reads to it.
            let pCurDq = current_layer_expect(pCtx);
            if let Some(view) = crate::encoder::svc_encode_slice::layer_rec_view(pCurDq) {
                pfDeblockingFilterSlice(
                    view,
                    &pCurDq.sSliceEncCtx,
                    &pCurDq.iCsStride,
                    pSlice,
                    &mut pMbs[kiLocal],
                );
            }
        }
        ENC_RETURN_SUCCESS
    })();

    // ---- CWelsSliceEncodingTask::FinishTask (the load-balancing override's half)
    if bRecordsTime {
        pSlice.uiSliceConsumeTime = (WelsTime() - iSliceStart) as u32;
    }

    SliceJobResult { iResult, bInitFailed: false }
}

/// How many workers a fork gets: never more than there are slices to encode, and
/// never more than there are bs scratch buffers behind the slots.
fn ForkWidth(pCtx: &mut sWelsEncCtx, iItemCount: i32) -> i32 {
    let iBuffers = pCtx
        .pSliceThreading
        .as_deref()
        .map_or(1, |pSmt| pSmt.uiThreadBsBufferNum as i32);
    iItemCount.min(iBuffers.max(1)).max(1)
}

/// **The fork/join for every fixed slice mode** — what
/// `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)` did for
/// `uiSliceMode != SM_SIZELIMITED_SLICE`.
///
/// Returns the value `FinishTask` would have ORed into `pCtx->iEncoderError`; the
/// caller ORs it, exactly where it read the field before.
///
/// # Panics
/// Panics if the layer's `pFirstMbIdxOfSlice` / `pCountMbNumInSlice` are not sized
/// for `kiSliceCount` slices, or if the macroblock ranges they hold are not
/// disjoint and in raster order.
pub fn EncodeFixedSlicesForked(pCtx: &mut sWelsEncCtx, kiSliceCount: i32) -> i32 {
    if kiSliceCount <= 0 || pCtx.pSliceThreading.is_none() {
        return ENC_RETURN_SUCCESS;
    }
    let bRecordsTime = pCtx.param_opt().is_some() && pCtx.param().bUseLoadBalancing;
    let iWidth = ForkWidth(pCtx, kiSliceCount);

    // Hoisted out of the fork: `WelsCodeOneSlice` wrote
    // `sLayerInfo.sNalHeaderExt.bIdrFlag` once per slice per worker; the write is the
    // same constant on every worker and no worker reads the field before its own
    // write, so running it once here, on the calling thread, before anything spawns,
    // is byte-for-byte what the race produced. See `StampLayerIdrFlagForSliceType`.
    crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

    // The pre-fork partition of the bitstream pool: the worker buffers leave the
    // context while it is still `&mut` — the borrow that cannot coexist with the
    // fork — so each worker's `&mut [u8]` borrows this local, never the shared
    // context. Taken, not copied: the pool is `[Vec<u8>; MAX_THREADS_NUM]`, and
    // moving a `Vec` moves no bytes. Restored below, after the join, behind the
    // same `&mut`.
    let mut vTakenBsBufs: Vec<Vec<u8>> = {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        (0..iWidth as usize)
            .map(|k| {
                let buf = std::mem::take(&mut pSmt.pThreadBsBuffer[k]);
                debug_assert!(!buf.is_empty(), "job slot {k} has no buffer behind it");
                buf
            })
            .collect()
    };

    // The slice bank is carved the same way. In every fixed slice mode all
    // workers resolve bank 0, so the bank leaves the layer here, while it is
    // still `&mut`, and its slices are distributed by the rule the fork already
    // uses: worker `k` codes slices `k, k+iWidth, k+2*iWidth, …`, which is
    // `i % iWidth == k`. Every slice reaches exactly one worker because
    // `iter_mut().enumerate()` yields each element once.
    let mut vTakenBank: Vec<SSlice> = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        std::mem::take(&mut pCurDq.sSliceBufferInfo[0].pSliceBuffer)
    };

    // The macroblock grid is carved beside the bank — a `split_at_mut` chain over
    // `[pFirstMbIdxOfSlice[i] .. +pCountMbNumInSlice[i])`, the ranges the slice map
    // itself was built from, and final before the fork (`SetSliceBoundaryInfo`,
    // inside the worker, only *reads* them). Every slice index in
    // `0..kiSliceCount` gets a window at its own position — no filtering and no
    // reordering, because the job pairs windows with slices by `kiLocal`
    // arithmetic.
    let (vSliceRanges, kiGridWidth) = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect(pCtx);
        let r: Vec<(i32, i32)> = (0..kiSliceCount as usize)
            .map(|i| (pCurDq.pFirstMbIdxOfSlice[i], pCurDq.pCountMbNumInSlice[i]))
            .collect();
        (r, pCurDq.sMbDataP.dims().mb_width())
    };
    let mut sTakenMbData = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        std::mem::replace(&mut pCurDq.sMbDataP, crate::safe::mb_grid::MbArray::empty())
    };

    let mut iErr = ENC_RETURN_SUCCESS;
    {
        // One handle per worker, each carrying its own slot, that slot's taken
        // buffer, and its own slices — constructed here, on the calling thread,
        // so the slot bound is checked before anything spawns.
        let pCtx: &sWelsEncCtx = pCtx;
        let mut vPerWorker: Vec<Vec<&mut SSlice>> =
            (0..iWidth as usize).map(|_| Vec::new()).collect();
        for (i, slc) in vTakenBank.iter_mut().enumerate() {
            if (i as i32) < kiSliceCount {
                vPerWorker[i % iWidth as usize].push(slc);
            }
        }
        // The peel: slice `i`'s records leave the taken grid as one chunk, in
        // index order — the ranges are raster-consecutive in every fixed mode,
        // and the assert names the coordinates if they ever overlap.
        let mut vMbPerWorker: Vec<Vec<crate::safe::mb_grid::MbWindow<'_, SMB>>> =
            (0..iWidth as usize).map(|_| Vec::new()).collect();
        {
            let mut rest: &mut [SMB] = sTakenMbData.as_mut_slice();
            let mut cursor = 0i32;
            for (i, &(first, count)) in vSliceRanges.iter().enumerate() {
                assert!(
                    first >= cursor && count > 0,
                    "slice {i} claims mbs [{first}..{}) against a carve cursor at {cursor} — \
                     the macroblock partition is not disjoint raster order",
                    first as i64 + count as i64,
                );
                let (_gap, tail) = rest.split_at_mut((first - cursor) as usize);
                let (chunk, tail) = tail.split_at_mut(count as usize);
                vMbPerWorker[i % iWidth as usize].push(crate::safe::mb_grid::MbWindow::new(
                    chunk,
                    first as usize,
                    kiGridWidth,
                    first as usize,
                ));
                rest = tail;
                cursor = first + count;
            }
        }

        let mut jobs: Vec<SliceJobHandle<'_>> = Vec::with_capacity(iWidth as usize);
        for (((k, buf), slices), mbs) in
            vTakenBsBufs.iter_mut().enumerate().zip(vPerWorker).zip(vMbPerWorker)
        {
            let k = k as i32;
            jobs.push(SliceJobHandle::new(pCtx, buf.as_mut_slice(), slices, mbs, None, None, k, k, iWidth, kiSliceCount, bRecordsTime));
        }

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(jobs.len());
            for job in jobs {
                handles.push(s.spawn(move || {
                    // `job` is the whole capture: nothing else crosses.
                    let mut job = job;
                    let mut iWorkerErr = ENC_RETURN_SUCCESS;
                    let mut iSliceIdx = job.iFirstSlice;
                    while iSliceIdx < job.iSliceCount {
                        let r = EncodeOneSliceInJob(
                            &*job.pCtx,
                            iSliceIdx,
                            job.iBsSlot,
                            job.bRecordsTime,
                            &mut *job.pBsBuf,
                            &mut job.pSlices,
                            &mut job.pMbs,
                            job.iFirstSlice,
                            job.iSliceStep,
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
    }

    // The buffers go back to their slots, contents carried — a worker's bytes
    // live in the `Vec` it wrote, and post-join assembly (`WriteSliceIToFrameBs`
    // and friends) reads them from the context exactly as before.
    {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        for (k, buf) in vTakenBsBufs.into_iter().enumerate() {
            pSmt.pThreadBsBuffer[k] = buf;
        }
    }
    {
        // The bank goes back with them, and so does the grid — a pointer move
        // each way, contents carried.
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        pCurDq.sSliceBufferInfo[0].pSliceBuffer = vTakenBank;
        pCurDq.sMbDataP = sTakenMbData;
    }

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
/// `bUseLoadBalancing` on.
///
/// # Panics
/// Panics if the slices' macroblock ranges are not disjoint, as in
/// [`EncodeFixedSlicesForked`]. Short per-slice tables are tolerated here.
pub fn UpdateMbMapForked(pCtx: &mut sWelsEncCtx, kiTaskCount: i32) {
    if kiTaskCount <= 0 || current_layer_ref(pCtx).is_none()
        || pCtx.pSliceThreading.is_none()
    {
        return;
    }
    let iWidth = ForkWidth(pCtx, kiTaskCount);
    let Some(pCurDq) = current_layer_mut(pCtx) else {
        return;
    };

    // The pre-fork partition: each slice's records are the contiguous run
    // `[pFirstMbIdxOfSlice[i] .. + pCountMbNumInSlice[i])`, and the runs are
    // disjoint, so a chain of `split_at_mut` carves them. The grid is carved
    // here, on the calling thread, while the layer is `&mut` — the borrow that
    // cannot coexist with the fork — and each worker is handed its own chunks.
    // Nothing crosses the spawn but `&mut [SMB]` and a shared `&SSliceCtx`.
    let SDqLayer { sMbDataP, sSliceEncCtx, pFirstMbIdxOfSlice, pCountMbNumInSlice, .. } =
        pCurDq;
    let kiMbWidth = sSliceEncCtx.iMbWidth as i32;
    let kiGridWidth = sMbDataP.dims().mb_width();

    // The ranges, in macroblock order: an empty slice contributes no chunk.
    let mut ranges: Vec<(i32, i32, i32)> = (0..kiTaskCount)
        .filter_map(|idc| {
            let first = *pFirstMbIdxOfSlice.get(idc as usize)?;
            let count = *pCountMbNumInSlice.get(idc as usize)?;
            (count > 0 && first >= 0).then_some((first, count, idc))
        })
        .collect();
    ranges.sort_unstable();

    // One `&mut [SMB]` per slice, peeled off the front in order. If the ranges
    // ever overlapped, this panics naming the coordinates instead of handing two
    // workers the same record.
    let mut rest: &mut [SMB] = sMbDataP.as_mut_slice();
    let mut cursor = 0i32;
    let mut per_worker: Vec<Vec<(i32, i32, i32, &mut [SMB])>> =
        (0..iWidth).map(|_| Vec::new()).collect();
    for (first, count, idc) in ranges {
        assert!(
            first >= cursor,
            "slice {idc} starts at mb {first}, inside the previous slice's records \
             (which end at {cursor}) — the macroblock partition is not disjoint"
        );
        let (_gap, tail) = rest.split_at_mut((first - cursor) as usize);
        let (chunk, tail) = tail.split_at_mut(count as usize);
        per_worker[(idc % iWidth) as usize].push((idc, first, count, chunk));
        rest = tail;
        cursor = first + count;
    }

    let pSliceCtx: &crate::encoder::svc_encode_slice::SSliceCtx = sSliceEncCtx;
    std::thread::scope(|s| {
        for group in per_worker {
            s.spawn(move || {
                for (idc, first, count, chunk) in group {
                    let mut mbs = crate::safe::mb_grid::MbWindow::new(
                        chunk,
                        first as usize,
                        kiGridWidth,
                        first as usize,
                    );
                    UpdateMbListNeighborParallel(
                        &mut mbs, pSliceCtx, kiMbWidth, idc, first, count,
                    );
                }
            });
        }
    });
}

/// One partition's slices under `SM_SIZELIMITED_SLICE` —
/// `CWelsConstrainedSizeSlicingEncodingTask`'s `InitTask` / `ExecuteTask` /
/// `FinishTask`, with the two mutexes the fork/join makes unnecessary removed and
/// nothing else moved.
///
/// **The order argument.** The claiming here was never a queue:
///
/// * there are exactly `iActiveThreadsNum` tasks, and task `t` works partition
///   `t % iActiveThreadsNum` — which is `t`. Partition is a **static** function of
///   the task index;
/// * the slice indices a partition produces are `t`, `t + N`, `t + 2N`, ... with
///   `N = iActiveThreadsNum` — a **static** arithmetic progression, stamped into
///   `SSlice::iSliceIdx`;
/// * `ReOrderSliceInLayer` (after the join, on the calling thread) recovers the
///   layer position from that stamp alone — `iSliceIdx % N` gives the partition and
///   `iSliceIdx / N` the position within it — never from which bank held the slice.
///
/// So the *only* schedule-dependent quantity today is which bank/bs-slot a partition
/// borrows, and `ReOrderSliceInLayer` is blind to it. Worker `p` owns partition `p`,
/// bank `p` and bs slot `p`, which also makes `NumSliceCodedOfPartition[p]` and
/// `LastCodedMbIdxOfPartition[p]` — written from inside the encode — disjoint by
/// construction.
fn EncodeOnePartitionSizeLimited(
    pCtx: &sWelsEncCtx,
    iPartitionIdx: i32,
    iBsSlot: i32,
    pSlotBuf: &mut [u8],
    // This partition's records, carved before the fork — every slice this worker
    // discovers lies inside the run. A partition whose span is zero codes
    // nothing and carries NO window: `MbWindow`'s invariant is that a window
    // always has a current macroblock, so the empty partition cannot be given
    // one. Every read below is past the `iDiffMbIdx == 0` guard, where the slice
    // is known to hold exactly one.
    pMbs: &mut [crate::safe::mb_grid::MbWindow<'_, SMB>],
    // This partition's CABAC restore scratch, taken from the context beside the
    // bitstream slot — reborrowed per coded slice below.
    mut pRestoreBuf: Option<&mut [u8]>,
    // This worker's slice bank, owned: the size-limited mode grows its bank
    // in-fork. The bank leaves the layer before the spawn (`std::mem::take`,
    // like the grid and the scratch), the growth is an ordinary `Vec` resize on
    // an exclusive borrow, and every resolve is an index. Restored after the
    // join.
    pBank: &mut crate::encoder::svc_encode_slice::SSliceBufferInfo,
) -> SliceJobResult {
    // ---- InitTask (base), minus the slot claim
    let eNalType = (*pCtx).eNalType;
    let eNalRefIdc = (*pCtx).eNalPriority;
    let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

    // The resolve is an index into the owned bank; the borrow is taken *after*
    // each growth, per iteration.
    {
        let kiCur = pBank.iCodedSliceNum as usize;
        let Some(pSlice) = pBank.pSliceBuffer.get_mut(kiCur) else {
            return SliceJobResult { iResult: ENC_RETURN_UNEXPECTED, bInitFailed: true };
        };
        crate::encoder::svc_encode_slice::InitOneSliceInThread(pCtx, pSlice, iBsSlot, iPartitionIdx);
        let iReturn = SetSliceBoundaryInfo(current_layer_ref(pCtx), pSlice, iPartitionIdx);
        if iReturn != ENC_RETURN_SUCCESS {
            return SliceJobResult { iResult: iReturn, bInitFailed: true };
        }
        pSlice.sSliceBs.sBsWrite = BsWriter::new();
    }
    // The last coded slice is a remembered *index*, stamped after the loop.
    let mut kiLastCodedSlot: Option<usize> = None;
    // `CWelsConstrainedSizeSlicingEncodingTask` derives from the load-balancing task,
    // not from `CWelsSliceEncodingTask`, so it stamps the slice time *unconditionally*
    // — `bUseLoadBalancing` does not gate this one (`wels_task_encoder.h:110`).
    let iSliceStart = WelsTime();

    // ---- ExecuteTaskConstrainedSize
    let iResult = (|| {
        let pCurDq = current_layer_expect(pCtx);
        let kiSliceIdxStep = (*pCtx).iActiveThreadsNum as i32;
        let kiPartitionId = iPartitionIdx % kiSliceIdxStep;
        let kiFirstMbInPartition = pCurDq.FirstMbIdxOfPartition[kiPartitionId as usize];
        let kiEndMbIdxInPartition = pCurDq.EndMbIdxOfPartition[kiPartitionId as usize];
        let kiCodedSliceNumByThread = pBank.iCodedSliceNum as usize;
        pBank.pSliceBuffer[kiCodedSliceNumByThread]
            .sSliceHeaderExt
            .sSliceHeader
            .iFirstMbInSlice = kiFirstMbInPartition;
        kiLastCodedSlot = Some(kiCodedSliceNumByThread);

        let iDiffMbIdx = kiEndMbIdxInPartition - kiFirstMbInPartition;
        if 0 == iDiffMbIdx {
            pBank.pSliceBuffer[kiCodedSliceNumByThread].iSliceIdx = -1;
            return ENC_RETURN_SUCCESS;
        }

        let mut iAnyMbLeftInPartition = iDiffMbIdx + 1;
        let mut iLocalSliceIdx = iPartitionIdx;
        while iAnyMbLeftInPartition > 0 {
            let bNeedReallocate = pBank.iCodedSliceNum >= pBank.iMaxSliceNum - 1;
            if bNeedReallocate {
                let iRet = crate::encoder::svc_encode_slice::ReallocateSliceInThread(
                    pCtx,
                    (*pCtx).uiDependencyId as i32,
                    pBank,
                );
                if ENC_RETURN_SUCCESS != iRet {
                    return iRet;
                }
            }

            // The resolve is a split of the owned bank — the current slot
            // exclusively, the boundary's forward slot beside it, from one
            // borrow. The borrow is taken *after* any growth above;
            // `tail.first_mut()` is `None` at the slot past the bank's end.
            let kiCurSlot = pBank.iCodedSliceNum as usize;
            if kiCurSlot >= pBank.pSliceBuffer.len() {
                return ENC_RETURN_UNEXPECTED;
            }
            let (kpHead, kpTail) = pBank.pSliceBuffer.split_at_mut(kiCurSlot + 1);
            let pSlice = &mut kpHead[kiCurSlot];
            let pNextSlice = kpTail.first_mut();
            crate::encoder::svc_encode_slice::InitOneSliceInThread(pCtx, pSlice, iBsSlot, iLocalSliceIdx);
            kiLastCodedSlot = Some(kiCurSlot);
            pSlice.sSliceBs.sBsWrite = BsWriter::new();

            // The partition slot, subsliced per coded slice — re-taken each
            // iteration because `InitOneSliceInThread` re-resolves the slice
            // (and with it the claimed size) after every reallocation. See
            // `EncodeOneSliceInJob` for the slot/size invariants.
            debug_assert_eq!(pSlice.uiBufferIdx as i32, iBsSlot, "the slice's claimed slot is this job's");
            let kuiSize = pSlice.sSliceBs.uiSize;
            let pSliceBsBuf = &mut pSlotBuf[..kuiSize as usize];
            let mut pCtxOutBs: Option<&mut BsWriter> = None;

            if bNeedPrefix {
                WritePrefixNalForSlice(pCtx, pSlice, eNalRefIdc, eNalType, &mut *pSliceBsBuf);
            }
            WelsLoadNalForSlice(&mut pSlice.sSliceBs, eNalType as i32, eNalRefIdc as i32);

            debug_assert_eq!(iLocalSliceIdx, pSlice.iSliceIdx);
            // The forward slot is the split's other half; `iCodedSliceNum + 1`
            // is the split point by construction.
            let mut iRet = WelsCodeOneSlice(pCtx, pSlice, eNalType as i32, &mut *pSliceBsBuf, &mut pCtxOutBs, &mut pMbs[0], pRestoreBuf.as_deref_mut(), pNextSlice);
            if ENC_RETURN_SUCCESS != iRet {
                return iRet;
            }
            WelsUnloadNalForSlice(&mut pSlice.sSliceBs);

            let mut iSliceSize = 0i32;
            iRet = WriteSliceBs(pCtx, pSlice, iLocalSliceIdx, &mut iSliceSize, &*pSliceBsBuf);
            if ENC_RETURN_SUCCESS != iRet {
                return iRet;
            }
            let pfDeblockingFilterSlice =
                (*pCtx).func_list().pfDeblocking.pfDeblockingFilterSlice.unwrap();
            // The walker reuses the partition run the coding chain just wrote
            // through, carved before the fork. `uiFilterIdc == 1` keeps the walk
            // and the neighbour reads inside the slice, hence inside the run.
            if let Some(view) = crate::encoder::svc_encode_slice::layer_rec_view(pCurDq) {
                pfDeblockingFilterSlice(
                    view,
                    &pCurDq.sSliceEncCtx,
                    &pCurDq.iCsStride,
                    pSlice,
                    &mut pMbs[0],
                );
            }

            iAnyMbLeftInPartition = kiEndMbIdxInPartition
                - pCurDq.LastCodedMbIdxOfPartition[kiPartitionId as usize].load(Ordering::Relaxed);
            iLocalSliceIdx += kiSliceIdxStep;
            pBank.iCodedSliceNum += 1;
        }
        ENC_RETURN_SUCCESS
    })();

    // ---- FinishTask
    if let Some(kiSlot) = kiLastCodedSlot {
        pBank.pSliceBuffer[kiSlot].uiSliceConsumeTime = (WelsTime() - iSliceStart) as u32;
    }

    SliceJobResult { iResult, bInitFailed: false }
}

/// **The fork/join for `SM_SIZELIMITED_SLICE`** — what
/// `pTaskManage->ExecuteTasks(WELS_ENC_TASK_ENCODING)` did on the dynamic path.
///
/// One worker per picture partition, which is what the task count already was
/// (`kiTaskCount = iActiveThreadsNum`, `wels_task_management.rs` `CreateTasks`).
/// Returns the value `FinishTask` would have ORed into `pCtx->iEncoderError`.
///
/// # Panics
/// Panics if the partitions' macroblock ranges are not disjoint and in raster
/// order, or if `iActiveThreadsNum` exceeds the per-thread slice banks
/// `InitSliceThreadInfo` sized.
pub fn EncodeSizeLimitedSlicesForked(pCtx: &mut sWelsEncCtx, kiPartitionCnt: i32) -> i32 {
    if kiPartitionCnt <= 0 || pCtx.pSliceThreading.is_none() {
        return ENC_RETURN_SUCCESS;
    }
    // Every partition is its own worker: the partition count is bounded by
    // `iMultipleThreadIdc` (`PicPartitionNumDecision`), which is also the bs-buffer
    // count, so the bound is met with equality rather than by clamping. The
    // `min` is kept as the enforcement, not as an expectation.
    let iWidth = ForkWidth(pCtx, kiPartitionCnt);
    debug_assert_eq!(
        iWidth, kiPartitionCnt,
        "a size-limited partition would go unencoded: {kiPartitionCnt} partitions, {iWidth} buffers"
    );

    // Hoisted out of the fork — see `EncodeFixedSlicesForked`.
    crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

    // The pre-fork partition of the bitstream pool: the worker buffers leave the
    // context while it is still `&mut` — the borrow that cannot coexist with the
    // fork — so each worker's `&mut [u8]` borrows this local, never the shared
    // context. Taken, not copied: the pool is `[Vec<u8>; MAX_THREADS_NUM]`, and
    // moving a `Vec` moves no bytes. Restored below, after the join, behind the
    // same `&mut`.
    let mut vTakenBsBufs: Vec<Vec<u8>> = {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        (0..iWidth as usize)
            .map(|k| {
                let buf = std::mem::take(&mut pSmt.pThreadBsBuffer[k]);
                debug_assert!(!buf.is_empty(), "job slot {k} has no buffer behind it");
                buf
            })
            .collect()
    };

    // The grid is carved by *partition*. Slice extents are discovered while
    // coding under `SM_SIZELIMITED_SLICE`, so per-slice runs are not knowable
    // here — but worker `p` owns partition `p` for the whole frame (the
    // static-partition argument above), and
    // `FirstMbIdxOfPartition`/`EndMbIdxOfPartition` are stamped before the
    // fork. One contiguous run each; every slice a worker discovers lies
    // inside its run, which is what lets the coding chain, the boundary
    // walker and the deblocking all write through the same window.
    let (vPartRanges, kiGridWidth) = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect(pCtx);
        let r: Vec<(i32, i32)> = (0..iWidth as usize)
            .map(|p| {
                let first = pCurDq.FirstMbIdxOfPartition[p];
                let end = pCurDq.EndMbIdxOfPartition[p];
                // A partition whose span is zero codes NOTHING, and the count must
                // say so. `EncodeOnePartitionSizeLimited` above is the authority —
                // `iDiffMbIdx == 0` stamps `iSliceIdx = -1` and returns before it
                // reads the window — and this is the same rule at the carve.
                // `WelsInitCurrentQBLayerMltslc` clamps `iPartitionNum` to 1 whenever
                // `kiMbNumInFrame / iPartitionNum` is 0 or 1, then zeroes every
                // remaining slot, while the fork still runs `iActiveThreadsNum`
                // workers: 32x16 with 3 threads is two macroblocks and three workers.
                let span = end - first;
                (first, if span == 0 { 0 } else { span + 1 })
            })
            .collect();
        (r, pCurDq.sMbDataP.dims().mb_width())
    };
    let mut sTakenMbData = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        std::mem::replace(&mut pCurDq.sMbDataP, crate::safe::mb_grid::MbArray::empty())
    };

    // The CABAC restore scratch joins the pre-fork takes: one buffer per
    // partition (`pDynamicBsBuffer[k]`), a `Vec` move each way, restored after
    // the join.
    let mut vTakenDynBufs: Vec<Vec<u8>> = (0..iWidth as usize)
        .map(|k| std::mem::take(&mut pCtx.pDynamicBsBuffer[k]))
        .collect();

    // The slice banks join the takes: worker `k` owns bank `k` for the frame,
    // growth is an owned `Vec` resize, and they are restored after the join with
    // grown size and coded slices carried.
    let mut vTakenBanks: Vec<crate::encoder::svc_encode_slice::SSliceBufferInfo> = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        (0..iWidth as usize)
            .map(|k| std::mem::take(&mut pCurDq.sSliceBufferInfo[k]))
            .collect()
    };

    let mut iErr = ENC_RETURN_SUCCESS;
    {
        let pCtx: &sWelsEncCtx = pCtx;
        let mut vMbPerWorker: Vec<Vec<crate::safe::mb_grid::MbWindow<'_, SMB>>> =
            (0..iWidth as usize).map(|_| Vec::new()).collect();
        {
            let mut rest: &mut [SMB] = sTakenMbData.as_mut_slice();
            let mut cursor = 0i32;
            for (p, &(first, count)) in vPartRanges.iter().enumerate() {
                if count == 0 {
                    // No window: `MbWindow` requires a current macroblock and
                    // this partition has none. The worker's own `iDiffMbIdx == 0`
                    // guard returns before it would index one.
                    continue;
                }
                assert!(
                    first >= cursor && count > 0,
                    "partition {p} claims mbs [{first}..{}) against a carve cursor at {cursor} — \
                     the partition map is not disjoint raster order",
                    first as i64 + count as i64,
                );
                let (_gap, tail) = rest.split_at_mut((first - cursor) as usize);
                let (chunk, tail) = tail.split_at_mut(count as usize);
                vMbPerWorker[p].push(crate::safe::mb_grid::MbWindow::new(
                    chunk,
                    first as usize,
                    kiGridWidth,
                    first as usize,
                ));
                rest = tail;
                cursor = first + count;
            }
        }
        let mut jobs: Vec<SliceJobHandle<'_>> = Vec::with_capacity(iWidth as usize);
        for ((((k, buf), mbs), dynbuf), bank) in vTakenBsBufs
            .iter_mut()
            .enumerate()
            .zip(vMbPerWorker)
            .zip(vTakenDynBufs.iter_mut())
            .zip(vTakenBanks.iter_mut())
        {
            let k = k as i32;
            // The size-limited fork carries no carved *slices* — the whole bank
            // instead, owned, because it grows in-fork — plus the partition's one
            // macroblock window and the restore scratch, `None` where the buffer
            // was never allocated.
            let dynbuf = if dynbuf.is_empty() { None } else { Some(dynbuf.as_mut_slice()) };
            jobs.push(SliceJobHandle::new(pCtx, buf.as_mut_slice(), Vec::new(), mbs, dynbuf, Some(bank), k, k, iWidth, kiPartitionCnt, true));
        }

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(jobs.len());
            for job in jobs {
                handles.push(s.spawn(move || {
                    let mut job = job;
                    let r = EncodeOnePartitionSizeLimited(
                        &*job.pCtx,
                        job.iFirstSlice,
                        job.iBsSlot,
                        &mut *job.pBsBuf,
                        &mut job.pMbs[..],
                        job.pDynBsBuf.take(),
                        job.pBank.take().expect("the size-limited job carries its bank"),
                    );
                    if !r.bInitFailed && r.iResult != ENC_RETURN_SUCCESS {
                        r.iResult
                    } else {
                        ENC_RETURN_SUCCESS
                    }
                }));
            }
            for h in handles {
                iErr |= h.join().unwrap_or(ENC_RETURN_UNEXPECTED);
            }
        });
    }

    // The buffers go back to their slots, contents carried — a worker's bytes
    // live in the `Vec` it wrote, and post-join assembly (`WriteSliceIToFrameBs`
    // and friends) reads them from the context exactly as before.
    {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        for (k, buf) in vTakenBsBufs.into_iter().enumerate() {
            pSmt.pThreadBsBuffer[k] = buf;
        }
    }
    {
        // The grid goes back with them, and so does the scratch.
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        pCurDq.sMbDataP = sTakenMbData;
    }
    for (k, buf) in vTakenDynBufs.into_iter().enumerate() {
        pCtx.pDynamicBsBuffer[k] = buf;
    }
    {
        // The banks go back — grown size and coded slices carried, which is what
        // `ReOrderSliceInLayer` and the NAL assembly read after this.
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_expect_mut(pCtx);
        for (k, bank) in vTakenBanks.into_iter().enumerate() {
            pCurDq.sSliceBufferInfo[k] = bank;
        }
    }

    iErr
}
