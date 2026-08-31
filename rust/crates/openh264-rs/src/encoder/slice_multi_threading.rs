#![deny(unsafe_code)]
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

// **The deny landed — T7.C8, and this file was the last one in `src/encoder` without
// it.** The module-level exemption on this file's `pub mod` line in `encoder/mod.rs`
// is retired; the deny is the inner attribute at the top of this file, and every
// unsafe item below carries its own allow with a category. 16 are `MT` — the fork
// bodies, the fork/join entry points, the resource pair, the one surviving mutex's
// helpers and the order-based assembly, none of which exists at
// `iMultipleThreadIdc == 1`, which is the rule the re-tag used. 9 are
// `port-raw(Phase 9)`: the three `macros.h` memory helpers and the six
// load-balancing functions, all of which run on the calling thread. The one
// hand-written `Send` this file carried — `send-seam(Phase 9)`, D-mt-1's seam —
// was deleted at S10.13 when `sWelsEncCtx` became `Sync`; its obituary and the
// three-part argument that justified it stay with `SliceJobHandle` below.


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
    ctx_dq_layer,
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

// **S9.3: the three `WelsSetMem*_c` kernels are deleted — they were dead.**
//
// `WelsSetMemMultiplebytes_c` had no caller anywhere in `src/` or `tests/`, only a
// stale import in `svc_enc_slice_segment.rs`, and it was in turn the *only* caller
// of `WelsSetMemUint16_c` and `WelsSetMemUint32_c`. The closure is all three —
// S18's rule that a dead-code deletion is a closure question, not a name question.
// The port's own fills go through `Vec`/slice writes and need no memset primitive.



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
pub fn fill_mb_map(map: &[AtomicU16], kiFirstMb: i32, kiCount: i32, uiValue: u16) {
    if kiFirstMb < 0 || kiCount <= 0 {
        return;
    }
    let a = kiFirstMb as usize;
    let b = a.saturating_add(kiCount as usize).min(map.len());
    if a < b {
        // `&[AtomicU16]` rather than `&mut [u16]` since T9.C2: `AddSliceBoundary`
        // calls this from inside the fork, and a `&mut` over the run would be a
        // `Unique` retag over storage the other partitions are reading.
        for c in &map[a..b] {
            c.store(uiValue, Ordering::Relaxed);
        }
    }
}


// ============================================================================
// Data Structures
// ============================================================================

// `SSliceThreadPrivateData` (`TagSliceThreadPrivateData`) stood here: a context
// pointer, a frame-bitstream pointer and two indices, one entry per thread. **Zero
// readers in the crate** — allocated by `RequestMtResource`, re-stamped by
// `WelsEncoderEncodeExt` before every dynamic dispatch, read by nothing. Deleted at
// T7.B4 with the same grep discipline the eight dead event fields got at T7.A1.

// Not `repr(C)` and not `Copy` since **T7.C5**: `pThreadBsBuffer` is an array of
// `Vec`s, which has no C shape and owns its storage. Nothing copied this struct by
// value — the compiler's answer, not an argument.
#[derive(Debug)]
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
    /// The `iSliceNumInFrame` lock — **F69**. `DynSlcJudgeSliceBoundaryStepBack`
    /// holds it across `AddSliceBoundary` and the increment, which is what the C++
    /// does (`svc_encode_slice.cpp:1776-1791`) and what the raw translation dropped.
    /// It was on T7.B4's delete list as dead; reading the reference is what saved it,
    /// and restoring the lock is what closed F3 (T7.B3).
    /// **T8.B10 — the handle names its type.** It was `*mut c_void`: the port
    /// erases nothing the C++ does not (`mt_defs.h` has `WELS_MUTEX`, a
    /// `pthread_mutex_t`), but the value on the other end of this pointer is a
    /// `std::sync::Mutex<()>` and nothing outside this crate ever sees it, so the
    /// erasure bought nothing and cost the reader a hop. The plumbing is unchanged
    /// — same allocation, same lock, same critical section.
    /// **S3.B1 — owned.** `WelsMutexInit`/`WelsMutexDestroy` and the
    /// `Box::into_raw` dance are deleted with the indirection: the mutex is born
    /// with the struct and dies with it. The fork locks it through a shared
    /// reference resolved off [`ctx_slice_threading_raw`]'s answer.
    pub mutexSliceNumUpdate: std::sync::Mutex<()>,
    /// One bs scratch buffer per worker slot, **owned since T7.C5**. It was
    /// `[*mut u8; MAX_THREADS_NUM]` over `alloc_zeroed` blocks, and the leak T7.A3
    /// fixed was in the walk that freed them; the array drops with the struct now, so
    /// there is no walk and nothing to leak.
    ///
    /// An **array of `Vec`s, not a `Vec<Vec<u8>>`**, and that is an aliasing choice
    /// rather than a stylistic one: `addr_of!((*pSmt).pThreadBsBuffer[i])` names one
    /// worker's element directly, where indexing an outer `Vec` would have to reborrow
    /// the outer container that *every* worker shares — which is F71's class exactly.
    /// `thread_bs_buffer` derived its slice that way until **S11.1b**, which made
    /// the question moot: the fork entries `mem::take` each worker's `Vec` out of
    /// this array before the spawn and restore it after the join, so no worker
    /// derives from the shared context at all — the pre-fork partition.
    pub pThreadBsBuffer: [Vec<u8>; MAX_THREADS_NUM],
    /// How many of the `MAX_THREADS_NUM` slots actually have a buffer behind them:
    /// `min(iMultipleThreadIdc, MAX_THREADS_NUM)`. **F67's bound, made readable.**
    /// `QueryEmptyThread` scanned all `MAX_THREADS_NUM` slots and it was the pool's
    /// concurrency cap that kept its answer in range; with the pool gone the cap has
    /// to be stated, and this is where the fork reads it (`ForkWidth`).
    ///
    /// `uiThreadBsBufferLen` stood beside it and is gone: it existed only because a
    /// Rust `dealloc` needs the `Layout` back, and a `Vec` already knows its length —
    /// the T3.3 standard, that extents are `buf.len()` and not fields.
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

// Not `repr(C)` and not `Copy` since **T6.D7**: `pOverallMbMap` owns its storage and
// has no C shape. Nothing in the crate copied this struct by value — the compiler's
// answer, not an argument.
//
// **T9.C2, F132 round 6**: the element type is `AtomicU16`. `AddSliceBoundary`
// rewrites this map from inside the fork under `SM_SIZELIMITED_SLICE` while the
// other partitions' workers read it, so the plain `u16` was a data race on the
// same shape `pGomCost` was (T9.C5; that field is itself gone — D-dead-3, it had
// no reader in either tree) — one entry per macroblock, partitions
// disjoint, nothing synchronising the storage itself. `Relaxed` throughout: the
// disjointness is the argument, and the scope join is the publication edge.
#[derive(Debug)]
pub struct SSliceCtx {
    pub uiSliceMode: SliceMode,
    pub iMbWidth: i16,
    pub iMbHeight: i16,
    /// **T9.C2, F136 — atomic, and the mutex above still stands.**
    /// `DynSlcJudgeSliceBoundaryStepBack` increments this from inside the fork
    /// under `mutexSliceNumUpdate` (F69), but every *reader* takes no lock:
    /// `WelsGetNextMbOfSlice` reborrows the whole `SSliceCtx` per macroblock on
    /// each worker, and a shared retag over the struct covers this field. A lock
    /// only one side takes is not synchronisation — the same shape F133 found on
    /// `pGomCost` (deleted whole since, D-dead-3: nothing read it, so it was never
    /// state; *this* field is read on every macroblock, which is the difference).
    /// The mutex is *not* redundant: it brackets `AddSliceBoundary`'s
    /// map rewrite *with* the increment, which no single atomic can do.
    pub iSliceNumInFrame: AtomicI32,
    pub iMbNumInFrame: i32,
    /// One slice index per macroblock, in raster order — **owned since T6.D7**
    /// (plan §4's "maps -> `Vec<u16>`"), **atomic since T9.C2**. Allocated by
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
// `SSliceThreading` stores its mutexes as opaque handles, matching the C++
// `WELS_MUTEX` fields. A `std::sync::Mutex` cannot be locked and unlocked
// through two separate calls the way pthreads can — the guard owns the lock —
// so the lock/unlock pair is expressed as one scoped call. Every C++
// lock/unlock pair in the encoder brackets a single straight-line region, so
// the critical sections are identical; only the spelling differs.

/// Allocates a mutex and returns its opaque handle (`WelsMutexInit`).
// (S3.B1: `WelsMutexInit` and `WelsMutexDestroy` stood here. The mutex is an
// owned field of `SSliceThreading` now — born in `Default`, dead with the box —
// so both helpers and their `Box::into_raw` dance are deleted rather than
// converted, which is what plan §5's B1 row promised.)

/// The slice-threading block **as a raw pointer, read out of the `Box`'s slot** —
/// F71's spelling, the fourth member of the named-raw family (`ctx_param_raw`,
/// `ctx_ref_list_raw`, `ctx_func_list_raw`).
///
/// **Why the fork cannot take a reference instead.** A worker resolving the block
/// through `(*pCtx).pSliceThreading.as_deref()` forms a shared borrow *of the
/// context's field*; N workers doing so concurrently is lawful, but a cursor into
/// this worker's bs slot must then be derived under that borrow and dies at any
/// sibling retag of the context. Read as a *value*, the answer carries the
/// heap block's own provenance (`Option<Box<T>>` is one pointer wide, `None` is
/// null), so per-slot cursors survive everything that happens to the context —
/// which is exactly the property the raw field gave the workers before S3.B1.
///
/// Null exactly where the field is `None`: a single-threaded encoder.
///
/// # Safety
/// `pCtx` must point to a live encoder context.
#[inline]
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn ctx_slice_threading_raw(pCtx: *const sWelsEncCtx) -> *mut SSliceThreading {
    std::ptr::read(std::ptr::addr_of!((*pCtx).pSliceThreading) as *const *mut SSliceThreading)
}

/// Runs `f` holding `pMutex`, i.e. a `WelsMutexLock`/`WelsMutexUnlock` pair.
///
/// A null handle runs `f` unlocked; that mirrors the C++ behaviour on an
/// uninitialised mutex closely enough for the single-threaded paths, which
/// never contend.
pub fn with_wels_mutex<R>(pMutex: Option<&std::sync::Mutex<()>>, f: impl FnOnce() -> R) -> R {
    // S3.B1: a safe fn — `None` is the C++'s `iMultipleThreadIdc <= 1` path, which
    // never contends and runs unlocked, exactly as the null pointer did.
    let Some(m) = pMutex else {
        return f();
    };
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
/// **S10.4: the partition is handed in, not resolved.** The window is the
/// caller's — carved out of the grid before the fork by
/// [`UpdateMbMapForked`] — so this body has no layer, no `mb_window` mint and no
/// `unsafe`.
///
/// F226's note, which stood here, described the defect the old shape had: a
/// `&mut (*pCurDq).sSliceEncCtx` reborrow taken for one scalar read, by N workers
/// on the *same* layer at once, which is a write-write race in the data-race
/// model with nothing written. It survives as history in
/// `update_mb_map_forked_workers_share_the_layer_without_racing`; the shape it
/// warned about is now inexpressible, because no worker here can name the layer.
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
/// **The producer half of the load-balancing loop — F72, completed at T7.C1**
/// (decision D-mt-2, plan §7.4). It is called from `WelsEncoderEncodeExt`, at the end
/// of the per-layer body, under the C++'s own four-term guard — the site is
/// `encoder_ext.cpp:4064-4073` and the port's call sits at the same place in the same
/// loop.
///
/// Until then the port had the *consumer* half of the loop and not the *producer*
/// half: nothing on a live path wrote `iSliceComplexRatio`, so `DynamicAdjustSlicing`
/// computed `WelsDivRound(kiCountNumMb * 0, 100) = 0` for every slice, clamped each to
/// `iMinimalMbNum` and handed the remainder to the last one. The balance was
/// **degenerate rather than absent** — nothing crashed and nothing warned, which is
/// why it took reading the C++ to find, and why a default-on feature must not be left
/// in that state.
///
/// **This path is expected-divergent and can never be byte-gated**, which is why its
/// coverage is structural rather than differential: the boundaries it produces for
/// frame N+1 are a function of frame N's measured per-slice *times*, so two runs of
/// the **C++** differ from each other. Both diffharness drivers pin
/// `bUseLoadBalancing = false`, as does the encode probe; the structural probe is
/// `load_balancing_completes_frames_with_sane_slice_counts`. It is the project's
/// second expected-divergent class after `CABA2_SVA_B` — see plan §1.5 and F72.
pub fn CalcSliceComplexRatio(pCurDq: &mut SDqLayer) {
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let mut iSumAv = 0i32;
    let kiSliceCount = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let mut iSliceIdx = 0i32;
    let mut iAvI = [0i32; MAX_SLICES_NUM];

    if kiSliceCount > MAX_SLICES_NUM as i32 {
        return;
    }
    WelsEmms();

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

    WelsEmms();

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
        // The raw form dereferenced unconditionally (T9.H): absence was never a
        // handled state, and the loop above has already walked the same range.
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer_mut(pCurDq, iSliceIdx)
            .expect("the layer's slice bank holds iSliceNum slices");
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DynamicAdjustSlicing(
    // **S11.12: the context parameter is narrowed to what this body reads.**
    // It was `&mut sWelsEncCtx` beside a `&mut SDqLayer` *that lives inside
    // that context* — the shape F71's slot read existed to keep apart, and the
    // reason `current_layer`'s raw survived at this call site. The body reads
    // exactly two things through the context (the coding parameters and the
    // rate controller) and writes only the layer, so it takes those two by
    // shared reference: they are disjoint fields from `ppDqLayerList`, the
    // caller can destructure, and no raw is needed at either end.
    pSvcParam: &crate::encoder::param_svc::SWelsSvcCodingParam,
    kpRc: &[crate::encoder::rc::SWelsSvcRc],
    pCurDqLayer: &mut SDqLayer,
    iCurDid: i32,
) {
    // T9.H: `if pCtx.is_null() { ... }` stood here. A `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one, so the guard is not
    // merely dead — it is inexpressible. Nothing replaces it.

    let pSliceCtx = &mut (*pCurDqLayer).sSliceEncCtx;
    let kiCountSliceNum = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let kiCountNumMb = pSliceCtx.iMbNumInFrame;
    let mut iMinimalMbNum = pSliceCtx.iMbWidth as i32;
    let mut iMaximalMbNum;
    let mut iMbNumLeft = kiCountNumMb;
    let mut iRunLen = [0i32; MAX_THREADS_NUM];
    let mut iSliceIdx: i32;

    // A7: the null test went with the raw; S11.12: the caller resolves the
    // parameters, so the `param_opt` question is answered before the call.
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
    WelsEmms();

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

    let ret = DynamicAdjustSlicePEncCtxAll(pCurDqLayer, iRunLen.as_mut_ptr());
    (*pCurDqLayer).bNeedAdjustingSlicing = ret == 0;
}

/// Applies newly calculated macroblock run-lengths to slice context structures.
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn DynamicAdjustSlicePEncCtxAll(pCurDq: &mut SDqLayer, pRunLength: *mut i32) -> i32 {
    if pRunLength.is_null() {
        return 1;
    }
    let pSliceCtx = &mut (*pCurDq).sSliceEncCtx;
    let iCountNumMbInFrame = pSliceCtx.iMbNumInFrame;
    let iCountSliceNumInFrame = pSliceCtx.iSliceNumInFrame.load(Ordering::Relaxed);
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
    // A7: the `pCodingParam` argument is gone — see `InitFunctionPointers`. The
    // caller held it as a `&mut` across this call, which Miri refused.
    // **S3.B2.** The two null tests are gone with the `*mut *mut` parameter: a
    // `&mut sWelsEncCtx` has no null state, and neither function ever reassigned
    // `*ctx`, which is what the double pointer was there to allow. The local raw
    // went with them — deriving one here and then reading `param()` through the
    // reference would pop it, which is F208's shape exactly.
    if iCountBsLen <= 0 {
        return 1;
    }

    let iThreadNum = ctx.param().iMultipleThreadIdc as i32;

    if iThreadNum <= 0 {
        return 1;
    }

    // **T7.C5: `Box`, not `alloc_zeroed`.** The struct owns `Vec`s now, and a `Vec`
    // field inside a zeroed block is UB at its first drop — S21, the same reason
    // every other member of this port's context is constructed rather than
    // zero-filled. `Default` writes what the zeroing stood for.
    let mut pSmt = Box::new(SSliceThreading::default());

    // **T7.B4.** An `SSliceThreadPrivateData` array was allocated and filled here, and
    // `WelsEncoderEncodeExt` re-stamped two of its fields before every dynamic-mode
    // dispatch. **It had zero readers** — grepped across the crate, the same way the
    // eight event fields were at T7.A1. It is the pre-thread-pool design's per-thread
    // context, carried through the raw translation and never wired to anything. Gone
    // with the struct.

    // **F69's mutex.** Not dead after all: it brackets `AddSliceBoundary` and
    // `++iSliceNumInFrame` in `DynSlcJudgeSliceBoundaryStepBack`, exactly as
    // `svc_encode_slice.cpp:1776-1791` does. It was on this step's delete list until
    // the C++ was read; see T7.B3.
    // (S3.B1: the `WelsMutexInit` call stood here — the mutex is a field of the
    // `Default` the box was just built from, so there is nothing left to install
    // and no failure path to report.)

    // **T7.B4.** `CreateTaskManage` stood here, and the buffer count was
    // `min(pTaskManage->GetThreadPoolThreadNum(), MAX_THREADS_NUM)` — the *pool*
    // answering how wide the concurrency would be. There is no pool; the fork is as
    // wide as the buffers, so the buffers are counted from the one number that ever
    // determined either (`iMultipleThreadIdc`, already clipped to
    // `[1, MAX_THREADS_NUM]` by `ParamValidationExt`). This is F67's bound with the
    // indirection removed rather than reproduced.
    let iThreadBufferNum = (iThreadNum as usize).min(MAX_THREADS_NUM);
    let _ = bDynamicSlice;

    pSmt.uiThreadBsBufferNum = iThreadBufferNum;
    for i in 0..iThreadBufferNum {
        pSmt.pThreadBsBuffer[i] = vec![0u8; iCountBsLen as usize];
    }
    ctx.pSliceThreading = Some(pSmt);

    // `mutexThreadBsBufferUsage` stood here, guarding `QueryEmptyThread`'s test-and-set
    // over `bThreadBsBufferUsage`. Both are gone: the slot is the worker's by
    // partition, so there is nothing to claim. `mutexEncoderError` stood here too,
    // guarding `FinishTask`'s OR into `iEncoderError`; the results come back through
    // the join now and the calling thread ORs them.
    //
    // **And `mutexThreadSlcBuffReallocate` stood here until T7.C5**, which is the third
    // of the four the C++ holds. Its own note said it "retires with `pMemAlign`, not
    // with the pool", and that is what happened: it serialised
    // `ReallocateSliceInThread`, whose bank has been the worker's own since T7.B2 and
    // whose one remaining shared object was the `CMemoryAlign` that allocated each new
    // slice's `sSliceBs.pBs`. T7.C4 made that buffer the slice's own, so the whole call
    // is worker-local writes over shared *reads* — see the census row and the deletion
    // note at its call site. **One mutex is left in this crate: F69's.**

    0
}

/// Tears down and frees all multithreading objects and bitstream buffers.
pub fn ReleaseMtResource(ctx: &mut sWelsEncCtx) {
    // **S3.B1** — the take is the whole teardown: the box drops at the end of
    // this function, its `Vec`s and the mutex with it.
    let Some(pSmt) = ctx.pSliceThreading.take() else {
        return;
    };

    // The C++ frees the thread buffers here (`pMa->WelsFree (pSmt->pThreadBsBuffer[i],
    // ...)`, slice_multi_threading.cpp:426). The raw translation kept the null-out and
    // dropped the free, so every encoder instance with iMultipleThreadIdc > 1 leaked
    // `iCountBsLen` bytes per worker for the process's life — T7.A3, which restored the
    // walk. **T7.C5 deletes the walk instead**: the buffers are the struct's own, so the
    // `Box` drop below releases every one of them and the class of defect T7.A3 fixed
    // cannot be written again here.

    // `WELS_DELETE_OP (pTaskManage)` stood here — dropping the manager released this
    // encoder's reference to the process-wide pool, and the last reference out
    // stopped and joined its worker threads. A scope's threads are joined by the
    // scope, so there is no pool lifetime left to manage.

    drop(pSmt);
}

/// Aggregates individual thread-local slice bitstream buffers into the contiguous frame bitstream buffer.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn AppendSliceToFrameBs(
    pCtx: &mut sWelsEncCtx,
    pLbi: &mut SLayerBSInfo,
    kiSliceCount: i32,
) -> i32 {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    // S11.20: the null disjunct retires with the raw (T9.H).
    if current_layer_ref(pCtx).is_none() {
        return 0;
    }

    let pCurDq = current_layer(pCtx);
    let mut iLayerSize = 0i32;
    let mut iNalIdxBase = 0i32;
    (*pLbi).iNalCount = 0;

    let mut iSliceIdx = 0i32;
    while iSliceIdx < kiSliceCount {
        let pSlice = crate::encoder::svc_encode_slice::slice_in_layer(Some(&*pCurDq), iSliceIdx);
        if !pSlice.is_null() {
            let pSliceBs = &mut (*pSlice).sSliceBs;
            if pSliceBs.uiBsPos > 0 {
                let iCountNal = pSliceBs.iNalIndex;

                if (pCtx.iPosBsBuffer as u64) + (pSliceBs.uiBsPos as u64)
                    > (pCtx.iFrameBsSize as u64)
                {
                    pCtx.iEncoderError |= ENC_RETURN_MEMALLOCERR;
                    return 0;
                }

                // T7.C4: the slice owns its bitstream, so the source is the `Vec`'s
                // root rather than a `CMemoryAlign` block. Same bytes, same length,
                // same destination.
                if !pCtx.frame_bs().is_null() {
                    if let Some(src) = pSliceBs.pBs.as_ref() {
                        std::ptr::copy(
                            src.as_ptr(),
                            pCtx.frame_bs_cur(),
                            pSliceBs.uiBsPos as usize,
                        );
                    }
                }

                pCtx.iPosBsBuffer += pSliceBs.uiBsPos as i32;
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn WriteSliceBs(
    pCtx: &sWelsEncCtx,
    pSlice: &mut SSlice,
    _iSliceIdx: i32,
    iSliceSize: &mut i32,
    pSliceBsBuf: &[u8],
) -> i32 {
    // **S7.A5**: the context arm retires with the parameter; the layer arm is live.
    if current_layer_ref(pCtx).is_none() {
        return 0;
    }
    let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);

    let kiNalCnt = (*pSliceBs).iNalIndex;
    let mut iNalIdx = 0i32;
    let mut iReturn = ENC_RETURN_SUCCESS;
    let iTotalLeftLength = ((*pSliceBs).uiBsSize - (*pSliceBs).uiBsPos) as i32;
    let pNalHdrExt = std::ptr::addr_of!(current_layer_ref(pCtx).expect("layer bound").sLayerInfo.sNalHeaderExt);
    // T7.C4: the write cursor is the slice's own buffer, absent when the slice
    // shares the frame's — which is what the raw `pBs` was, and `WelsEncodeNal`
    // takes the `INVALIDINPUT` arm for `None` exactly as the C++ did for null.
    // **S11.17**: an offset into that buffer rather than a pointer into it, so
    // the advance below is arithmetic the compiler checks against the Vec's own
    // length instead of against the caller's `iTotalLeftLength` promise.
    let bHasOwnBuffer = (*pSliceBs).pBs.is_some();
    let mut iDstPos = 0usize;

    if kiNalCnt > 2 {
        return 0;
    }

    *iSliceSize = 0;
    while iNalIdx < kiNalCnt {
        let mut iNalSize = 0i32;
        // The slice's NAL list is offsets into the thread buffer its writer is
        // positioned in; that buffer is named here, beside the entry.
        // The slice's own buffer is re-sliced from the running offset each
        // iteration; `iTotalLeftLength - *iSliceSize` was the same bound said
        // as a number, and the two agree by construction because `uiBsSize` is
        // that buffer's length.
        let kNalEntry = (*pSliceBs).sNalList[iNalIdx as usize];
        let kiLeft = (iTotalLeftLength - *iSliceSize).max(0) as usize;
        let pDstTail = if bHasOwnBuffer {
            let buf = (*pSliceBs).pBs.as_mut().expect("checked above");
            let kiEnd = (iDstPos + kiLeft).min(buf.len());
            Some(&mut buf[iDstPos.min(kiEnd)..kiEnd])
        } else {
            None
        };
        iReturn = WelsEncodeNal(
            &kNalEntry,
            pSliceBsBuf,
            Some(&*pNalHdrExt),
            pDstTail,
            &mut iNalSize,
        );

        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }

        (*pSliceBs).iNalLen[iNalIdx as usize] = iNalSize;
        *iSliceSize += iNalSize;
        if bHasOwnBuffer {
            iDstPos += iNalSize as usize;
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AdjustBaseLayer(pCtx: &mut sWelsEncCtx) -> i32 {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if ctx_dq_layer(pCtx, 0).is_null() {
        return 0;
    }

    let pCurDq = ctx_dq_layer(pCtx, 0);

    // T6.G2's one edit in an MT file: the field is a position now, and layer 0 is
    // the position this function has always meant (`ppDqLayerList[0]`, two lines
    // up). Body otherwise untouched — Phase 7 owns everything else here.
    set_current_layer(pCtx, Some(LayerIdx(0)));

    // T9.E2h, F66's shape B with an accessor-minted root the detector cannot
    // see: the count is read before the call whose first argument retags.
    let kiSliceNumInFrame = (*pCurDq).sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
    let iNeedAdj = NeedDynamicAdjust(&mut *pCurDq, kiSliceNumInFrame);

    if iNeedAdj != 0 {
        // S11.12: the two owners come out of one `&mut` by destructuring —
        // `pSvcParam`/`pWelsSvcRc` against `ppDqLayerList`, disjoint fields —
        // so the layer is a checked reference rather than a raw resolution.
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
// unsafe-cat: port-raw(Phase 9)
#[allow(unsafe_code)]
pub unsafe fn AdjustEnhanceLayer(pCtx: &mut sWelsEncCtx, iCurDid: i32) -> i32 {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if ctx_dq_layer(pCtx, 0).is_null() || current_layer_ref(pCtx).is_none() {
        return 0;
    }

    // A7: the null test went with the raw — see `param_opt`.
    if pCtx.param_opt().is_none() {
        return 0;
    }
    // A7, §4.6 reorder: the layer's slice mode and the thread count are scalars, so
    // the parameter borrow does not have to span `current_layer`'s claim.
    let kPrevSliceArg = if iCurDid > 0 && (iCurDid as usize - 1) < MAX_SPATIAL_LAYER_NUM {
        let a = &pCtx.param().sSpatialLayers[iCurDid as usize - 1].sSliceArgument;
        Some((a.uiSliceMode, a.uiSliceNum))
    } else {
        None
    };
    let kiMultipleThreadIdc = pCtx.param().iMultipleThreadIdc;

    let kbModelingFromSpatial = current_layer_ref(pCtx).expect("layer bound").pRefLayer.is_some()
        && match kPrevSliceArg {
            Some((uiSliceMode, uiSliceNum)) => {
                uiSliceMode == SliceMode::SM_FIXEDSLCNUM_SLICE
                    && kiMultipleThreadIdc as u32 >= uiSliceNum
            }
            None => false,
        };

    let iNeedAdj: i32;
    if kbModelingFromSpatial {
        let pBaseLayer = ctx_dq_layer(pCtx, iCurDid as usize - 1);
        if pBaseLayer.is_null() {
            return 0;
        }
        // T9.E2h, shape B again — and here the two arguments can NAME THE SAME
        // LAYER (base == current when iCurDid is the base), so the load is
        // hoisted above the retag.
        let kiSliceNumInFrame =
            current_layer_ref(pCtx).expect("layer bound").sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
        iNeedAdj = NeedDynamicAdjust(&mut *pBaseLayer, kiSliceNumInFrame);
        if iNeedAdj != 0 {
            // T9.G6: hoisted (shape B). **S11.11: this one keeps the raw.**
            // `DynamicAdjustSlicing` takes `&mut sWelsEncCtx` *and* `&mut
            // SDqLayer` where the layer lives inside that context, and F71's
            // slot read is what keeps the two provenances apart — a
            // `current_layer_mut` here would be a second `&mut` on the context
            // and the borrow checker refuses it, correctly. Restructuring that
            // parameter list is the real fix and is its own checkpoint.
            // S11.12: destructured, as in `AdjustBaseLayer`.
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
        let pCurLayer = ctx_dq_layer(pCtx, iCurDid as usize);
        let kiSliceNumInFrame =
            current_layer_ref(pCtx).expect("layer bound").sSliceEncCtx.iSliceNumInFrame.load(Ordering::Relaxed);
        iNeedAdj = NeedDynamicAdjust(&mut *pCurLayer, kiSliceNumInFrame);
        if iNeedAdj != 0 {
            // T9.G6: hoisted (shape B). **S11.11: this one keeps the raw.**
            // `DynamicAdjustSlicing` takes `&mut sWelsEncCtx` *and* `&mut
            // SDqLayer` where the layer lives inside that context, and F71's
            // slot read is what keeps the two provenances apart — a
            // `current_layer_mut` here would be a second `&mut` on the context
            // and the borrow checker refuses it, correctly. Restructuring that
            // parameter list is the real fix and is its own checkpoint.
            // S11.12: destructured, as in `AdjustBaseLayer`.
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

    /// **S11.1b's referee — the bitstream-pool partition's take/split/restore.**
    ///
    /// The per-worker disjointness itself is a compile fact now (each worker's
    /// `&mut [u8]` borrows a distinct `Vec` held by the fork entry's local), so
    /// what remains falsifiable is the machinery around it: that slot `k`'s
    /// bytes are the ones worker `k` wrote, through the take, the spawn, the
    /// join, and the restore. Two workers write distinct patterns into their
    /// own taken slots while both hold a shared borrow of the pool's owner —
    /// the aliasing shape production has after S11.1b (`&sWelsEncCtx` beside
    /// `&mut` partition slots) — and the patterns must land back in the
    /// owner's own slots. The Miri lane runs this and certifies the aliasing
    /// half; natively it certifies the arithmetic.
    ///
    /// **Teeth**: swapping the restore's indices (buffer `k` into slot
    /// `WORKERS-1-k`) fails the pattern assertion on every run — the failure
    /// is deterministic, so the calibration is a control run, not an iteration
    /// count (control seen red before this landed; see the checkpoint's log).
    #[test]
    fn bs_pool_partition_carves_disjoint_slots_and_restores_them() {
        const WORKERS: usize = 2;
        const LEN: usize = 64;

        let mut pSmt = SSliceThreading::default();
        for k in 0..WORKERS {
            pSmt.pThreadBsBuffer[k] = vec![0u8; LEN];
        }
        pSmt.uiThreadBsBufferNum = WORKERS;

        // The take — the fork entries' partition, same spelling.
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
    // unsafe-cat: instrument(test) — S11.5: a test, not a fork-reachable body.
    #[allow(unsafe_code)]
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
        dq_layer.sSliceEncCtx.iSliceNumInFrame.store(2, Ordering::Relaxed);

        CalcSliceComplexRatio(&mut dq_layer);

        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[0].iSliceComplexRatio, 50);
        assert_eq!(dq_layer.sSliceBufferInfo[0].pSliceBuffer[1].iSliceComplexRatio, 50);
    }

    /// **The load-balancing path's only coverage — F72, T7.C1** (decision D-mt-2).
    ///
    /// The two tests above drive `CalcSliceComplexRatio` and `NeedDynamicAdjust` on
    /// hand-built layers, which is why F72 could sit in the tree with both of them
    /// green: they prove the *functions* compute, not that anything **calls** them.
    /// This one runs the whole encoder with `bUseLoadBalancing` on, four threads and
    /// four slices — the exact four-term guard `WelsEncoderEncodeExt` tests before it
    /// calls the producer — for enough frames that frame N+1's boundaries are
    /// computed from frame N's measured times. Before T7.C1 the ratios were
    /// permanently zero and this test would have passed anyway, which is the point:
    /// it is the *shape* of the path that is now covered, and the ratios' correctness
    /// belongs to `test_calc_slice_complex_ratio` above.
    ///
    /// **It asserts structure and never bytes, and it cannot do otherwise.** The
    /// boundaries this path produces are a function of wall-clock encode times, so
    /// two runs of the **C++** disagree with each other; there is no reference to
    /// compare against. That makes the path the project's **second
    /// expected-divergent class** after `CABA2_SVA_B` (plan §1.5) — both diffharness
    /// drivers pin the flag off, so no sweep row reaches it, and this is the only
    /// thing that does.
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
    /// itself the most expensive test in the battery. The aliasing question this
    /// path raises is the fork/join's, and that probe answers it.
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

// ============================================================================
// The spawn seam — D-mt-1 (plan §7.4), and the fork/join it carries
// ============================================================================
//
// This section replaces the pool dispatch for the fixed slice modes. What it does
// NOT change is the access pattern: the workers call the same slice-encode tree
// with the same raw context pointer the pool's tasks handed it, in the same order,
// with the same per-slice state. Only the machinery around the call shrinks — the
// task hierarchy, the C++-list ports, the mutable process-wide singleton, the claiming
// mutex and the condvar dance all become `std::thread::scope`.

/// One worker's share of a frame's slices, and the one value in this crate that
/// crosses a spawn.
///
/// The worker owns bs scratch slot `iBsSlot` for the whole scope and encodes the
/// slices `iFirstSlice`, `iFirstSlice + iSliceStep`, ... below `iSliceCount` — a
/// static partition where the pool did a dynamic claim, which is strictly more
/// deterministic and is why the claiming mutex can go.
pub struct SliceJobHandle<'a> {
    /// The encoder context, held raw exactly as `CWelsBaseTask::m_pCtx` held it.
    /// The encoder context. **S7.A5**: `*const`, not `*mut` — the three sites that
    /// read it form `&*job.pCtx` and nothing writes through it (S7.A1 measured the
    /// context's write set at one body, single-threaded, and not this one). Still raw,
    /// and still behind the hand-written `Send` below: `sWelsEncCtx` cannot be `Sync`
    /// while `sLogCtx` holds the caller's `void*` (F245), so the pointer crosses the
    /// spawn and each worker derives its own shared reference after arrival.
    // **S10.13: a reference, not a raw.** It was `*const sWelsEncCtx` for exactly
    // one reason — `sWelsEncCtx` was not `Sync`, so `&sWelsEncCtx` was not `Send`
    // and could not cross a spawn. Twelve field conversions (S10.5-S10.12) cleared
    // the four raw families that made it so, and the borrow now crosses on its own
    // merits under `thread::scope`.
    pCtx: &'a sWelsEncCtx,
    /// This worker's bs scratch bytes — `pThreadBsBuffer[iBsSlot]`'s buffer,
    /// **taken out of the context before the fork and handed here** (S11.1b: the
    /// pre-fork partition, `UpdateMbMapForked`'s pattern on the bitstream pool).
    /// It borrows the fork entry's local, not the shared context, so the
    /// compiler proves the per-worker disjointness the per-use re-derivation
    /// could only assert. Restored to its slot after the join.
    pBsBuf: &'a mut [u8],
    /// **This worker's slices — S11.21, the last structural seam.** The slice
    /// bank was the one storage a fork body still reached through a raw root
    /// (`slice_in_bank`, F71's shape): in the fixed modes *every* worker
    /// resolves bank 0, so two of them retagging the `Vec` at once is a race
    /// even though neither writes the `Vec` itself. The fix is the one S10.4
    /// used for the macroblock grid and S11.1b for the bitstream pool — carve
    /// before the spawn. Worker `k` codes slices `k, k+step, k+2*step, …`, so
    /// the bank is distributed by that rule on the calling thread while the
    /// layer is `&mut`, and each worker holds `&mut SSlice`s **no sibling can
    /// name**. Disjointness is the compiler's, not a comment's.
    pSlices: Vec<&'a mut SSlice>,
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

// **`send-seam(Phase 9)` — retired at S10.13; no tag, because the tag's unsafe is
// gone.** What follows is the seam's record: the hand-written `Send` decision
// D-mt-1 (plan §7.4) permitted stood here, and the ratchet's `unsafe_impl` metric
// is an occurrence count, so this comment deliberately does not spell the two
// words it would otherwise double.
//
// **Its retirement condition, corrected by measurement — F164, Phase 9 session G.**
// This comment used to say it retires "when Phase 9's context split makes this
// handle naturally `Send`", on F67's inventory of twelve `!Sync` reasons with
// "five of them inside types Phases 8 and 10 own". Re-derived at HEAD with F67's
// own probe, it is still twelve and it is a **different** twelve — `CMemoryAlign`
// retired with the allocator, `SrcPicPool` arrived inside `SDqLayer`, and the three
// scalar-pointer reasons F67 attributed to the context directly all reach through
// `SVAAFrameInfo` now. A stable total is not a stable list. **Only four of the
// twelve are the context split's**: `SSliceThreading`, `SWelsEncoderOutput`, and
// `SRefList`/`SrcPicPool` through `SDqLayer`. The other eight are `SVAAFrameInfo`'s
// four (the VAA family), `SWelsSvcCodingParam`'s signed-char pointer (Phase 8's),
// `SScreenBlockFeatureStorage` (Phase 10's), `SLogContext`'s opaque handle, and
// `CWelsPreProcess`. (Type names only above: the ratchet's `raw_ptr` metric is an
// occurrence count too, and F164's table in the findings is where the spellings
// belong.)
//
// **Settled at the phase exit — F205, session J, and the answer is structural.**
// F195 showed why every count above is unreadable: that probe emits one error per
// distinct *type*, so it cannot see a field retire whose type survives elsewhere.
// Re-derived **by field** (one `Sync` question per member of the context), the
// twelve types are **seven fields**, and their owners are five:
//
//   pSliceThreading   the field itself                 the ctx split's
//   pOut              the field itself                 the ctx split's
//   pVpp              the field itself                 the preprocessor's
//   pVaa              SVAAFrameInfo's six plane cursors    the VAA family's
//   ppDqLayerList     SDqLayer::pRefList                   the layer family's
//   ppRefPicListExt   SPicture::pScreenBlockFeatureStorage  **Phase 10's**
//   sLogCtx           pLogCtx: *mut c_void                 **the C ABI's**
//
// The last line is the one that decides this comment's future: `SLogContext` holds
// the *application's* opaque handle, set through `SetOption(TRACE_CALLBACK)`, and
// no amount of internal conversion can make a caller-owned `void*` `Sync`. So the
// seam does not retire when the port finishes converting itself — it retires only
// if that field stops being a member of the context, which is an API question and
// not a safety one. It stays, per D-exit-2, and the soundness argument below is
// what carries it. Three parts, each verified rather than asserted:
//
// **Re-derived at S7 (F245), and F205's seven fields are three.** The same
// question, asked the same way — one `Sync` bound on the context, read the
// compiler's chains — now answers with six root types under three fields:
//
//   ppDqLayerList   `SDqLayer`'s two pool pointers        the layer family's
//   pVaa            `SVAAFrameInfo`'s three plane roots   the VAA family's
//   sLogCtx         `pLogCtx: *mut c_void`                **the C ABI's**
//
// Four of F205's seven have retired: `pSliceThreading`, `pOut` and `pVpp` to
// earlier sessions' conversions, and `ppRefPicListExt` to S6.B1, which made
// `SPicture::pScreenBlockFeatureStorage` own its five buffers. The conclusion is
// unchanged and is now the *only* thing left blocking `Sync`: `sLogCtx`.
//
// **And S7 settled what this seam costs the context flip: nothing.** The flip does
// not send a `&sWelsEncCtx` anywhere. It sends this handle — a raw pointer, which
// is what the impl below covers — and each worker forms `&*job.pCtx` after arrival,
// on its own thread. The compiler demands no `Sync` bound for that, verified by
// probe. What the shared reference *does* need is precisely the three-part argument
// below, which the `Send` already carries; the flip widens who relies on it, not
// what it claims. The handle therefore keeps its raw field and this impl, which is
// the plan's stated end state (§7.4's two audited lines) rather than a debt.
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
// **S10.13: `unsafe impl Send for SliceJobHandle` stood here, and it is gone.**
//
// It was one of the two hand-written impls decision D1 pins the tree at, and the
// three numbered arguments above are its justification — disjointness by static
// index, order-based assembly, and the buffer-count bound. Every one of them is
// still true and still worth reading; none of them is *load-bearing* any more.
//
// The impl existed because the handle carried `*const sWelsEncCtx`, and it carried
// a raw because `sWelsEncCtx` was `!Sync` — so `&sWelsEncCtx` was not `Send` and
// could not cross `thread::scope`'s spawn. S10.5-S10.12 removed the four raw
// families behind that (`SDqLayer`'s four fields, `SVAAFrameInfo`'s four,
// `SLogContext`'s handle, `CBackgroundDetection`'s three), and the auto impl now
// applies: a shared reference to a `Sync` type is `Send`, checked by the compiler
// rather than asserted here.
//
// **D1's pin drops from two to one.** What remains is `SharedCells`'s `Sync`, the
// reconstruction seam's, which is a genuinely unprovable claim about byte
// disjointness and is refereed by the probes in `svc_encode_slice.rs`.

impl<'a> SliceJobHandle<'a> {
    /// `pCtx`'s `pSliceThreading` must have been built by `RequestMtResource`,
    /// `iBsSlot` must be a slot that call allocated, and `pBsBuf` must be that
    /// slot's taken buffer (the fork entries' partition is the only maker).
    /// Liveness is the references' since S10.13; nothing here is `unsafe`.
    fn new(
        pCtx: &'a sWelsEncCtx,
        pBsBuf: &'a mut [u8],
        pSlices: Vec<&'a mut SSlice>,
        iBsSlot: i32,
        iFirstSlice: i32,
        iSliceStep: i32,
        iSliceCount: i32,
        bRecordsTime: bool,
    ) -> Self {
        // Part 3 of the safety argument, checked where the handle is made rather
        // than trusted where it is used. Both entry points refuse a null
        // `pSliceThreading` before they get here.
        //
        // **S11.1b: the buffer-allocated check moved to the take** (the buffer
        // the handle sees IS the slot's, by the partition's construction), so
        // what remains here is F67's slot bound. The shared borrow is the whole
        // of what this needs (S10.13).
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
        Self { pCtx, pBsBuf, pSlices, iBsSlot, iFirstSlice, iSliceStep, iSliceCount, bRecordsTime }
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
    // **S10.3a: the `addr_of_mut!` cursor is gone.** T9.E2b's note explained why it
    // was minted here rather than threaded: both callers used to pass
    // `addr_of_mut!((*pSlice).sSliceBs)` *beside* the slice, and the `&mut`
    // argument reborrow pops a sibling cursor into it. There is no sibling any
    // more — the bs half is reached through this body's own `&mut SSlice`, one
    // field projection at a time — so the reason for the raw went with the shape
    // that needed it.
    //
    if eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
        WelsLoadNalForSlice(
            &mut pSlice.sSliceBs,
            EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
            eNalRefIdc as i32,
        );
        // S11.1a: the buffer arrives threaded from the job's one derivation
        // instead of being re-derived from the shared context here.
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
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
unsafe fn EncodeOneSliceInJob(
    pCtx: &sWelsEncCtx,
    iSliceIdx: i32,
    iBsSlot: i32,
    bRecordsTime: bool,
    pSlotBuf: &mut [u8],
    pSlices: &mut [&mut SSlice],
    iFirstSlice: i32,
    iSliceStep: i32,
) -> SliceJobResult {
    // ---- CWelsSliceEncodingTask::InitTask
    let eNalType = (*pCtx).eNalType;
    let eNalRefIdc = (*pCtx).eNalPriority;
    let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

    // **S11.21: the slice comes from this worker's own list.** It used to be
    // resolved out of the shared bank by index (`slice_in_bank`'s raw root);
    // the fork entry carves that bank before spawning, so the worker already
    // holds `&mut SSlice`s no sibling can name. `iSliceIdx` advances by
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

    // **S11.1b: the slice's bitstream buffer is the job's partition slot**,
    // subsliced to the claimed size — no derivation from the shared context at
    // all. The slot buffer left the context before the fork (the pre-fork
    // partition), so this `&mut` is the compiler's, not an assertion; the
    // assert bridges the old resolution path (by the slice's recorded
    // `uiBufferIdx`) to the partition's (by the job's slot) — equal by
    // construction, since `InitOneSliceInThread` stamps the claim with this
    // job's slot. The subslice length is the claimed size every deep call used
    // to pass to `thread_bs_buffer`; `uiSize` cannot exceed the slot (both are
    // `iCountBsLen`, single writer), and a safe slice would panic where the
    // raw form would have quietly overrun.
    // The `pOut` writer option is `None` on this side: F217 measured that arm
    // main-thread-only, and every slice of a forked layer has its own writer.
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
        let mut iReturn = WelsCodeOneSlice(pCtx, pSlice, eNalType as i32, &mut *pSliceBsBuf, &mut pCtxOutBs);
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
        pfDeblockingFilterSlice(
            current_layer_ref(pCtx).expect("the frame's current layer is stamped"),
            pSlice,
        );
        ENC_RETURN_SUCCESS
    })();

    // ---- CWelsSliceEncodingTask::FinishTask (the load-balancing override's half;
    //      the base half was the slot release and the error OR, both now gone)
    // The `!pSlice.is_null()` disjunct is gone with the raw: a `&mut SSlice`
    // cannot be null, and the `else` above already took the absent case.
    if bRecordsTime {
        pSlice.uiSliceConsumeTime = (WelsTime() - iSliceStart) as u32;
    }

    SliceJobResult { iResult, bInitFailed: false }
}

/// How many workers a fork gets: never more than there are slices to encode, and
/// never more than there are bs scratch buffers behind the slots (F67's bound).
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
/// # Safety
/// `pCtx` must be a live context with `pSliceThreading` built and the layer's
/// slice bank sized for `kiSliceCount` slices.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn EncodeFixedSlicesForked(pCtx: &mut sWelsEncCtx, kiSliceCount: i32) -> i32 {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if kiSliceCount <= 0 || pCtx.pSliceThreading.is_none() {
        return ENC_RETURN_SUCCESS;
    }
    let bRecordsTime = pCtx.param_opt().is_some() && pCtx.param().bUseLoadBalancing;
    let iWidth = ForkWidth(pCtx, kiSliceCount);

    // **T7.C3 — F71's residue, hoisted out of the fork.** `WelsCodeOneSlice` wrote
    // `sLayerInfo.sNalHeaderExt.bIdrFlag` once per slice per worker; the write is the
    // same constant on every worker and no worker reads the field before its own
    // write, so running it once here, on the calling thread, before anything spawns,
    // is byte-for-byte what the race produced. See `StampLayerIdrFlagForSliceType`.
    //
    // **S10.13 moved it above the handles**, and the compiler is why: the handles
    // hold `&sWelsEncCtx` now, and this is the body's last `&mut` use. "Before
    // anything spawns" is what T7.C3 required and is still what happens — the
    // ordering is now enforced rather than described.
    crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

    // **S11.1b: the pre-fork partition of the bitstream pool** — S10.4's pattern
    // (`UpdateMbMapForked`'s grid carve) applied to the pair's buffer half. The
    // worker buffers leave the context while it is still `&mut` — the borrow
    // that cannot coexist with the fork — so each worker's `&mut [u8]` borrows
    // this local, never the shared context, and the compiler proves the
    // disjointness the per-use re-derivation could only assert. Taken, not
    // copied: the pool is `[Vec<u8>; MAX_THREADS_NUM]`, and moving a `Vec`
    // moves no bytes. Restored below, after the join, behind the same `&mut`.
    let mut vTakenBsBufs: Vec<Vec<u8>> = {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        (0..iWidth as usize)
            .map(|k| {
                let buf = std::mem::take(&mut pSmt.pThreadBsBuffer[k]);
                // Part 3's allocation check, moved here from the handle: the
                // buffer the handle will see is this one.
                debug_assert!(!buf.is_empty(), "job slot {k} has no buffer behind it");
                buf
            })
            .collect()
    };

    // **S11.21: the slice bank is carved the same way — the last structural
    // seam.** In every fixed slice mode all workers resolve bank 0 (F71), so
    // the bank leaves the layer here, while it is still `&mut`, and its slices
    // are distributed by the rule the fork already uses: worker `k` codes
    // slices `k, k+iWidth, k+2*iWidth, …`, which is `i % iWidth == k`. Every
    // slice reaches exactly one worker because `iter_mut().enumerate()` yields
    // each element once — the compiler carries that fact, where
    // `slice_in_bank`'s raw could only assert it.
    let mut vTakenBank: Vec<SSlice> = {
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_mut(pCtx)
            .expect("the frame's current layer is stamped");
        std::mem::take(&mut pCurDq.sSliceBufferInfo[0].pSliceBuffer)
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
        let mut jobs: Vec<SliceJobHandle<'_>> = Vec::with_capacity(iWidth as usize);
        for ((k, buf), slices) in vTakenBsBufs.iter_mut().enumerate().zip(vPerWorker) {
            let k = k as i32;
            jobs.push(SliceJobHandle::new(pCtx, buf.as_mut_slice(), slices, k, k, iWidth, kiSliceCount, bRecordsTime));
        }

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(jobs.len());
            for job in jobs {
                handles.push(s.spawn(move || {
                    // `job` is the whole capture: the seam is this one move, and
                    // nothing else crosses.
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
        // S11.21: the bank goes back with them.
        let pCurDq = crate::encoder::svc_encode_slice::current_layer_mut(pCtx)
            .expect("the frame's current layer is stamped");
        pCurDq.sSliceBufferInfo[0].pSliceBuffer = vTakenBank;
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
/// `bUseLoadBalancing` on. See the step-5 ruling in the log.
///
/// # Safety
/// As [`EncodeFixedSlicesForked`].
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub fn UpdateMbMapForked(pCtx: &mut sWelsEncCtx, kiTaskCount: i32) {
    // T9.H: the `pCtx.is_null()` disjunct is gone — a `&mut sWelsEncCtx`
    // cannot be null and every caller now holds one. The rest is unchanged.
    if kiTaskCount <= 0 || current_layer_ref(pCtx).is_none()
        || pCtx.pSliceThreading.is_none()
    {
        return;
    }
    let iWidth = ForkWidth(pCtx, kiTaskCount);
    let Some(pCurDq) = current_layer_mut(pCtx) else {
        return;
    };

    // **S10.4: the pre-fork partition, and it needs no `unsafe` at all.**
    //
    // Every worker used to resolve the layer itself — `current_layer(&*job.pCtx)`
    // through `SliceJobHandle`'s raw — and then mint a `&mut [SMB]` window out of
    // a *shared* layer with `mb_window`, whose safety contract is the unverifiable
    // "the caller owns these records exclusively". That contract is true here, and
    // it is true for a reason the compiler can be shown instead of told: each
    // slice's records are the contiguous run
    // `[pFirstMbIdxOfSlice[i] .. + pCountMbNumInSlice[i])`, and the runs are
    // disjoint. A chain of `split_at_mut` **is** that fact.
    //
    // So the grid is carved here, on the calling thread, while the layer is `&mut`
    // — the borrow that cannot coexist with the fork — and each worker is handed
    // its own chunks. Nothing crosses the spawn but `&mut [SMB]` and a shared
    // `&SSliceCtx`, both `Send` on their own merits, so this fork no longer needs
    // `SliceJobHandle` or its hand-written `Send`.
    let SDqLayer { sMbDataP, sSliceEncCtx, pFirstMbIdxOfSlice, pCountMbNumInSlice, .. } =
        pCurDq;
    let kiMbWidth = sSliceEncCtx.iMbWidth as i32;
    let kiGridWidth = sMbDataP.dims().mb_width();

    // The ranges, in macroblock order. `kiCount <= 0` was the old body's early
    // return and stays one: an empty slice contributes no chunk.
    let mut ranges: Vec<(i32, i32, i32)> = (0..kiTaskCount)
        .filter_map(|idc| {
            let first = *pFirstMbIdxOfSlice.get(idc as usize)?;
            let count = *pCountMbNumInSlice.get(idc as usize)?;
            (count > 0 && first >= 0).then_some((first, count, idc))
        })
        .collect();
    ranges.sort_unstable();

    // One `&mut [SMB]` per slice, peeled off the front in order. The assert is the
    // disjointness claim `mb_window`'s `# Safety` used to make in prose: if the
    // ranges ever overlapped, this panics naming the coordinates instead of
    // handing two workers the same record.
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
/// **The order argument** (step 2's prerequisite, written down before the shape was
/// chosen). The claiming here was never a queue:
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
/// borrows, and `ReOrderSliceInLayer` is blind to it. Neither an `AtomicUsize`
/// counter nor an `mpsc` of indices is needed to reproduce the order: **a static
/// partition reproduces it exactly and removes the last nondeterminism**, where a
/// queue would reintroduce one. Worker `p` therefore owns partition `p`, bank `p`
/// and bs slot `p`, which also makes `NumSliceCodedOfPartition[p]` and
/// `LastCodedMbIdxOfPartition[p]` — written from inside the encode — disjoint by
/// construction rather than by the claim.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
unsafe fn EncodeOnePartitionSizeLimited(
    pCtx: &sWelsEncCtx,
    iPartitionIdx: i32,
    iBsSlot: i32,
    pSlotBuf: &mut [u8],
) -> SliceJobResult {
    // ---- InitTask (base), minus the slot claim
    let eNalType = (*pCtx).eNalType;
    let eNalRefIdc = (*pCtx).eNalPriority;
    let bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

    // **S10.3a: the init half takes the reference; the loop below cannot.** This
    // body *re-resolves* its slice after every `ReallocateSliceInThread` — the
    // dynamic mode grows its bank in-fork, so a `&mut` taken before the growth
    // would name freed storage. The reference's scope therefore ends here, and the
    // encode loop keeps `slice_in_bank`'s raw. That split is the honest shape of
    // step 3: the fixed modes partition, the size-limited mode reallocates.
    {
        let Some(pSlice) = crate::encoder::svc_encode_slice::ResolveSliceInOwnBank(
            pCtx,
            iBsSlot,
            iPartitionIdx,
        ) else {
            return SliceJobResult { iResult: ENC_RETURN_UNEXPECTED, bInitFailed: true };
        };
        let iReturn = SetSliceBoundaryInfo(current_layer_ref(pCtx), pSlice, iPartitionIdx);
        if iReturn != ENC_RETURN_SUCCESS {
            return SliceJobResult { iResult: iReturn, bInitFailed: true };
        }
        pSlice.sSliceBs.sBsWrite = BsWriter::new();
    }
    let mut pSlice: *mut SSlice = std::ptr::null_mut();
    // `CWelsConstrainedSizeSlicingEncodingTask` derives from the load-balancing task,
    // not from `CWelsSliceEncodingTask`, so it stamps the slice time *unconditionally*
    // — `bUseLoadBalancing` does not gate this one (`wels_task_encoder.h:110`).
    let iSliceStart = WelsTime();

    // ---- ExecuteTaskConstrainedSize
    let iResult = (|| {
        let pCurDq = current_layer(pCtx);
        let kiSliceIdxStep = (*pCtx).iActiveThreadsNum as i32;
        let kiPartitionId = iPartitionIdx % kiSliceIdxStep;
        let kiFirstMbInPartition = (*pCurDq).FirstMbIdxOfPartition[kiPartitionId as usize];
        let kiEndMbIdxInPartition = (*pCurDq).EndMbIdxOfPartition[kiPartitionId as usize];
        let kiCodedSliceNumByThread =
            (*pCurDq).sSliceBufferInfo[iBsSlot as usize].iCodedSliceNum;
        pSlice = crate::encoder::svc_encode_slice::slice_in_bank(
            &*pCurDq,
            iBsSlot as usize,
            kiCodedSliceNumByThread,
        );
        (*pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = kiFirstMbInPartition;

        let iDiffMbIdx = kiEndMbIdxInPartition - kiFirstMbInPartition;
        if 0 == iDiffMbIdx {
            (*pSlice).iSliceIdx = -1;
            return ENC_RETURN_SUCCESS;
        }

        let mut iAnyMbLeftInPartition = iDiffMbIdx + 1;
        let mut iLocalSliceIdx = iPartitionIdx;
        while iAnyMbLeftInPartition > 0 {
            let bNeedReallocate = (*pCurDq).sSliceBufferInfo[iBsSlot as usize].iCodedSliceNum
                >= (*pCurDq).sSliceBufferInfo[iBsSlot as usize].iMaxSliceNum - 1;
            if bNeedReallocate {
                // **`mutexThreadSlcBuffReallocate` is gone — T7.C5**, and the C++ site
                // it came from is `wels_task_encoder.cpp:258-261`. T7.B2 made the bank
                // being grown this worker's alone; the one shared object left in the
                // call was the `CMemoryAlign` behind `InitSliceBsBuffer`, and **T7.C4
                // made that buffer the slice's own**. What remains is worker-local
                // writes over shared *reads* (`iSliceBufferSize`, `iNumRef0`,
                // `iGlobalQp`, the partition bounds — all frame-level and fixed before
                // the fork), so there is nothing left to serialise. The lock's own note
                // said it "retires with `pMemAlign`, not with the pool", and this is
                // that.
                let iRet = ReallocateSliceInThread(
                    pCtx,
                    pCurDq,
                    (*pCtx).uiDependencyId as i32,
                    iBsSlot,
                );
                if ENC_RETURN_SUCCESS != iRet {
                    return iRet;
                }
            }

            // **S10.3a: the size-limited loop keeps its raw, and the reason is the
            // tail.** `FinishTask` below stamps `uiSliceConsumeTime` on **the last
            // slice this partition coded**, which is what `pSlice` holds after the
            // loop — so the pointer has to outlive each iteration. A per-iteration
            // `&mut SSlice` cannot carry that, and stamping inside the loop instead
            // would stamp *every* slice where the C++ stamps one. Converting this
            // body needs the tail redesigned with its own referee, and it retires
            // no allow on its own (the reallocation and the bank re-resolve stay),
            // so it is left for step 3's dynamic half.
            let mut pNext: *mut SSlice = std::ptr::null_mut();
            let Some(pNextRef) = crate::encoder::svc_encode_slice::ResolveSliceInOwnBank(
                pCtx,
                iBsSlot,
                iLocalSliceIdx,
            ) else {
                return ENC_RETURN_UNEXPECTED;
            };
            pNext = pNextRef;
            pSlice = pNext;
            let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);
            (*pSliceBs).sBsWrite = BsWriter::new();

            // S11.1b: the partition slot, subsliced per coded slice — re-taken
            // each iteration because `InitOneSliceInThread` re-resolves the
            // slice (and with it the claimed size) after every reallocation.
            // Same slot, same size the deep calls used to read for themselves;
            // see `EncodeOneSliceInJob` for the slot/size invariants.
            debug_assert_eq!((*pSlice).uiBufferIdx as i32, iBsSlot, "the slice's claimed slot is this job's");
            let kuiSize = (*pSlice).sSliceBs.uiSize;
            let pSliceBsBuf = &mut pSlotBuf[..kuiSize as usize];
            let mut pCtxOutBs: Option<&mut BsWriter> = None;

            if bNeedPrefix {
                WritePrefixNalForSlice(pCtx, &mut *pSlice, eNalRefIdc, eNalType, &mut *pSliceBsBuf);
            }
            let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);
            WelsLoadNalForSlice(&mut *pSliceBs, eNalType as i32, eNalRefIdc as i32);

            debug_assert_eq!(iLocalSliceIdx, (*pSlice).iSliceIdx);
            let mut iRet = WelsCodeOneSlice(pCtx, &mut *pSlice, eNalType as i32, &mut *pSliceBsBuf, &mut pCtxOutBs);
            if ENC_RETURN_SUCCESS != iRet {
                return iRet;
            }
            let pSliceBs = std::ptr::addr_of_mut!((*pSlice).sSliceBs);
            WelsUnloadNalForSlice(&mut *pSliceBs);

            let mut iSliceSize = 0i32;
            iRet = WriteSliceBs(pCtx, &mut *pSlice, iLocalSliceIdx, &mut iSliceSize, &*pSliceBsBuf);
            if ENC_RETURN_SUCCESS != iRet {
                return iRet;
            }
            let pfDeblockingFilterSlice =
                (*pCtx).func_list().pfDeblocking.pfDeblockingFilterSlice.unwrap();
            pfDeblockingFilterSlice(&*pCurDq, &mut *pSlice);

            iAnyMbLeftInPartition = kiEndMbIdxInPartition
                - (*pCurDq).LastCodedMbIdxOfPartition[kiPartitionId as usize].load(Ordering::Relaxed);
            iLocalSliceIdx += kiSliceIdxStep;
            // S11.11: the raw stays — this is a *fork* body holding
            // `&sWelsEncCtx`, so no `&mut` to the context exists to resolve the
            // layer through. The counter it bumps is the worker's own bank slot.
            (*current_layer(pCtx)).sSliceBufferInfo[iBsSlot as usize].iCodedSliceNum += 1;
        }
        ENC_RETURN_SUCCESS
    })();

    // ---- FinishTask
    if !pSlice.is_null() {
        (*pSlice).uiSliceConsumeTime = (WelsTime() - iSliceStart) as u32;
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
/// # Safety
/// As [`EncodeFixedSlicesForked`], and `iActiveThreadsNum` must be within the
/// per-thread slice banks `InitSliceThreadInfo` sized.
// unsafe-cat: fork-shared(S63)
#[allow(unsafe_code)]
pub unsafe fn EncodeSizeLimitedSlicesForked(pCtx: &mut sWelsEncCtx, kiPartitionCnt: i32) -> i32 {
    // T9.H4: the `is_null()` disjunct that opened this guard is gone — a
    // `&mut sWelsEncCtx` cannot be null, and every caller now holds one. The
    // remaining conditions are unchanged.
    if kiPartitionCnt <= 0 || pCtx.pSliceThreading.is_none() {
        return ENC_RETURN_SUCCESS;
    }
    // Every partition is its own worker: the partition count is bounded by
    // `iMultipleThreadIdc` (`PicPartitionNumDecision`), which is also the bs-buffer
    // count, so the F67 bound is met with equality rather than by clamping. The
    // `min` is kept as the enforcement, not as an expectation.
    let iWidth = ForkWidth(pCtx, kiPartitionCnt);
    debug_assert_eq!(
        iWidth, kiPartitionCnt,
        "a size-limited partition would go unencoded: {kiPartitionCnt} partitions, {iWidth} buffers"
    );

    // **T7.C3 — F71's residue, hoisted out of the fork**, and moved above the
    // handles at S10.13 for the reason `EncodeFixedSlicesForked` gives: this is the
    // body's last `&mut` use and the handles hold a shared borrow now.
    crate::encoder::svc_encode_slice::StampLayerIdrFlagForSliceType(pCtx);

    // **S11.1b: the pre-fork partition of the bitstream pool** — S10.4's pattern
    // (`UpdateMbMapForked`'s grid carve) applied to the pair's buffer half. The
    // worker buffers leave the context while it is still `&mut` — the borrow
    // that cannot coexist with the fork — so each worker's `&mut [u8]` borrows
    // this local, never the shared context, and the compiler proves the
    // disjointness the per-use re-derivation could only assert. Taken, not
    // copied: the pool is `[Vec<u8>; MAX_THREADS_NUM]`, and moving a `Vec`
    // moves no bytes. Restored below, after the join, behind the same `&mut`.
    let mut vTakenBsBufs: Vec<Vec<u8>> = {
        let pSmt = pCtx.pSliceThreading.as_deref_mut().expect("guarded above");
        (0..iWidth as usize)
            .map(|k| {
                let buf = std::mem::take(&mut pSmt.pThreadBsBuffer[k]);
                // Part 3's allocation check, moved here from the handle: the
                // buffer the handle will see is this one.
                debug_assert!(!buf.is_empty(), "job slot {k} has no buffer behind it");
                buf
            })
            .collect()
    };

    let mut iErr = ENC_RETURN_SUCCESS;
    {
        let pCtx: &sWelsEncCtx = pCtx;
        let mut jobs: Vec<SliceJobHandle<'_>> = Vec::with_capacity(iWidth as usize);
        for (k, buf) in vTakenBsBufs.iter_mut().enumerate() {
            let k = k as i32;
            // S11.21: the size-limited fork carries no carved slices — it
            // resolves from its own bank inside the fork, because that bank
            // grows there (`ResolveSliceInOwnBank`).
            jobs.push(SliceJobHandle::new(pCtx, buf.as_mut_slice(), Vec::new(), k, k, iWidth, kiPartitionCnt, true));
        }

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(jobs.len());
            for job in jobs {
                handles.push(s.spawn(move || {
                    let mut job = job;
                    let r = EncodeOnePartitionSizeLimited(&*job.pCtx, job.iFirstSlice, job.iBsSlot, &mut *job.pBsBuf);
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

    iErr
}
