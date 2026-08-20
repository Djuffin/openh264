//! Encoder Task Management Subsystem.
//!
//! Translated from `codec/encoder/core/inc/wels_task_management.h` and
//! `codec/encoder/core/src/wels_task_management.cpp`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    unused_variables,
    unused_unsafe
)]

use std::ptr::null_mut;
use std::sync::{Arc, Condvar, Mutex};
pub use crate::encoder::encoder_context::SLogContext;
use crate::encoder::encoder_context::ctx_param;
pub use crate::encoder::param_svc::SWelsSvcCodingParam;
pub use crate::encoder::svc_encode_slice::SDqLayer;
use crate::encoder::svc_encode_slice::{current_layer, LayerIdx};
pub use crate::encoder::encoder_context::sWelsEncCtx;

use crate::common::wels_common_defs::{EWelsNalRefIdc, EWelsNalUnitType};
// The shared thread pool. `wels_task_management.cpp` uses the one and only
// `CWelsThreadPool` from `common/`; so does this module. An earlier port
// declared a second, inline-executing `CWelsThreadPool` here that shadowed it.
use crate::common::wels_thread_pool::{CWelsThreadPool, IWelsTask, IWelsTaskSink, TaskPtr};
use crate::encoder::nal_encap::{
    SWelsSliceBs, WelsLoadNalForSlice, WelsUnloadNalForSlice, WelsWriteSVCPrefixNal,
};
use crate::encoder::slice_multi_threading::{
    with_wels_mutex, UpdateMbListNeighborParallel, WriteSliceBs, MAX_THREADS_NUM,
};
use crate::encoder::svc_encode_slice::{
    thread_bs_buffer, InitOneSliceInThread, ReallocateSliceInThread, SSlice, SetSliceBoundaryInfo,
    WelsCodeOneSlice,
};
use crate::encoder::vlc_encoder::BsWriter;
use crate::encoder::wels_encoder_ext::WelsTime;

pub const MAX_DEPENDENCY_LAYER: usize = 4;

// Return & Error Codes
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_MEMALLOCERR: i32 = 0x01;
pub const ENC_RETURN_UNEXPECTED: i32 = 0x04;

// ETaskType constants (matching `CWelsBaseTask::ETaskType`)
pub const WELS_ENC_TASK_ENCODING: usize = 0;
pub const WELS_ENC_TASK_ENCODE_FIXED_SLICE: usize = WELS_ENC_TASK_ENCODING;
pub const WELS_ENC_TASK_ENCODE_SLICE_LOADBALANCING: usize = WELS_ENC_TASK_ENCODING;
pub const WELS_ENC_TASK_ENCODE_SLICE_SIZECONSTRAINED: usize = WELS_ENC_TASK_ENCODING;
pub const WELS_ENC_TASK_UPDATEMBMAP: usize = 1;
pub const WELS_ENC_TASK_PREPROCESS: usize = 2;
pub const WELS_ENC_TASK_ALL: usize = 3;

// Slicing Modes (matching `SliceModeEnum`)
pub const SM_SINGLE_SLICE: u32 = 0;
pub const SM_FIXEDSLCNUM_SLICE: u32 = 1;
pub const SM_RASTER_SLICE: u32 = 2;
pub const SM_SIZELIMITED_SLICE: u32 = 3;
pub const SM_RESERVED: u32 = 4;

/// Slice argument configuration (`SSliceArgument`).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SSliceArgument {
    pub uiSliceMode: u32,
    pub uiSliceNum: u32,
    pub uiSliceSizeConstraint: u32,
    pub uiSliceMbNum: [u32; 35],
    pub bSliceNumBoxCount: bool,
}

impl Default for SSliceArgument {
    fn default() -> Self {
        Self {
            uiSliceMode: SM_SINGLE_SLICE,
            uiSliceNum: 0,
            uiSliceSizeConstraint: 0,
            uiSliceMbNum: [0; 35],
            bSliceNumBoxCount: false,
        }
    }
}

/// Spatial layer configuration (`SSpatialLayerConfig`).
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct SSpatialLayerConfig {
    pub iVideoWidth: i32,
    pub iVideoHeight: i32,
    pub fFrameRate: f32,
    pub iSpatialBitrate: i32,
    pub iMaxSpatialBitrate: i32,
    pub uiProfileIdc: i32,
    pub uiLevelIdc: i32,
    pub iDLayerQp: i32,
    pub sSliceArgument: SSliceArgument,
}

/// SVC coding parameters (`SWelsSvcCodingParam`).


/// Spatial DQ layer context (`SDqLayer`).

/// Logger context (`SLogContext`).


/// Top-level encoder context state (`sWelsEncCtx`).


/// Task sink callback interface (`IWelsTaskSink`).
/// Which `CWelsBaseTask` subclass a task instance stands for.
///
/// The C++ hierarchy is
/// `CWelsBaseTask` <- `CWelsSliceEncodingTask` <- {`CWelsLoadBalancingSlicingEncodingTask`,
/// `CWelsConstrainedSizeSlicingEncodingTask`}, plus `CWelsUpdateMbMapTask`, and it
/// dispatches `InitTask`/`ExecuteTask`/`FinishTask` virtually from a single
/// non-virtual `Execute()`. One struct carrying a discriminant reproduces that
/// vtable exactly; the previous port cast `*mut Derived` to `*mut CWelsBaseTask`,
/// which silently resolved every call to the base and encoded nothing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ETaskKind {
    /// `CWelsUpdateMbMapTask`
    UpdateMbMap,
    /// `CWelsSliceEncodingTask`
    SliceEncoding,
    /// `CWelsLoadBalancingSlicingEncodingTask`
    LoadBalancingSlicing,
    /// `CWelsConstrainedSizeSlicingEncodingTask`
    ConstrainedSizeSlicing,
}

/// Base task representation (`CWelsBaseTask` and its encoding subclasses).
#[repr(C)]
pub struct CWelsBaseTask {
    pub m_pSink: *mut CWelsTaskManageBase,
    pub m_pCtx: *mut sWelsEncCtx,
    pub m_iSliceIdx: i32,
    pub m_uiTaskType: u32,
    pub m_eKind: ETaskKind,

    // CWelsSliceEncodingTask members
    pub m_eTaskResult: i32,
    pub m_iThreadIdx: i32,
    pub m_pSlice: *mut SSlice,
    pub m_pSliceBs: *mut SWelsSliceBs,
    pub m_eNalType: EWelsNalUnitType,
    pub m_eNalRefIdc: EWelsNalRefIdc,
    pub m_bNeedPrefix: bool,
    pub m_iSliceSize: i32,
    pub m_iStartMbIdx: i32,
    pub m_iEndMbIdx: i32,

    // CWelsLoadBalancingSlicingEncodingTask member
    pub m_iSliceStart: i64,
}

// The task is handed to a worker thread as a raw pointer, exactly as in C++.
// Every field it reaches is either thread-private (m_pSlice, the bs buffer
// selected by QueryEmptyThread) or guarded by one of the SSliceThreading mutexes.
unsafe impl Send for CWelsBaseTask {}
unsafe impl Sync for CWelsBaseTask {}

impl CWelsBaseTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
        uiTaskType: u32,
        eKind: ETaskKind,
    ) -> Self {
        Self {
            m_pSink: pSink,
            m_pCtx: pCtx,
            m_iSliceIdx: iSliceIdx,
            m_uiTaskType: uiTaskType,
            m_eKind: eKind,
            m_eTaskResult: ENC_RETURN_SUCCESS,
            m_iThreadIdx: 0,
            m_pSlice: null_mut(),
            m_pSliceBs: null_mut(),
            m_eNalType: EWelsNalUnitType::NAL_UNIT_UNSPEC_0,
            m_eNalRefIdc: EWelsNalRefIdc::NRI_PRI_LOWEST,
            m_bNeedPrefix: false,
            m_iSliceSize: 0,
            m_iStartMbIdx: 0,
            m_iEndMbIdx: 0,
            m_iSliceStart: 0,
        }
    }

    pub fn GetTaskType(&self) -> u32 {
        self.m_uiTaskType
    }

    /// True for the two kinds that resolve `InitTask`/`FinishTask` to
    /// `CWelsLoadBalancingSlicingEncodingTask`'s overrides.
    /// `CWelsConstrainedSizeSlicingEncodingTask` derives from the load-balancing
    /// task, not from `CWelsSliceEncodingTask` directly, so it inherits the
    /// slice timing as well (`wels_task_encoder.h:110`).
    fn RecordsSliceTime(&self) -> bool {
        matches!(
            self.m_eKind,
            ETaskKind::LoadBalancingSlicing | ETaskKind::ConstrainedSizeSlicing
        )
    }

    /// `CWelsSliceEncodingTask::SetBoundary`
    pub fn SetBoundary(&mut self, iStartIdx: i32, iEndIdx: i32) -> i32 {
        self.m_iStartMbIdx = iStartIdx;
        self.m_iEndMbIdx = iEndIdx;
        ENC_RETURN_SUCCESS
    }

    /// `CWelsSliceEncodingTask::QueryEmptyThread`
    pub fn QueryEmptyThread(pThreadBsBufferUsage: &mut [bool; MAX_THREADS_NUM]) -> i32 {
        for k in 0..MAX_THREADS_NUM {
            if !pThreadBsBufferUsage[k] {
                pThreadBsBufferUsage[k] = true;
                return k as i32;
            }
        }
        -1
    }

    /// `CWelsSliceEncodingTask::InitTask`
    pub unsafe fn InitTask(&mut self) -> i32 {
        let pCtx = self.m_pCtx;
        self.m_eNalType = (*pCtx).eNalType;
        self.m_eNalRefIdc = (*pCtx).eNalPriority;
        self.m_bNeedPrefix = (*pCtx).bNeedPrefixNalFlag;

        let pSmt = (*pCtx).pSliceThreading;
        self.m_iThreadIdx = with_wels_mutex((*pSmt).mutexThreadBsBufferUsage, || {
            CWelsBaseTask::QueryEmptyThread(&mut (*pSmt).bThreadBsBufferUsage)
        });

        if self.m_iThreadIdx < 0 {
            return ENC_RETURN_UNEXPECTED;
        }

        let mut pSlice: *mut SSlice = null_mut();
        let mut iReturn = InitOneSliceInThread(
            pCtx,
            &mut pSlice,
            self.m_iThreadIdx,
            (*pCtx).uiDependencyId as i32,
            self.m_iSliceIdx,
        );
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }
        self.m_pSlice = pSlice;
        self.m_pSliceBs = std::ptr::addr_of_mut!((*self.m_pSlice).sSliceBs);

        iReturn = SetSliceBoundaryInfo(current_layer(pCtx), self.m_pSlice, self.m_iSliceIdx);
        if iReturn != ENC_RETURN_SUCCESS {
            return iReturn;
        }

        // `SetOneSliceBsBufferUnderMultithread(pCtx, m_iThreadIdx, m_pSlice)` was
        // called here; `InitOneSliceInThread` above already stored the same index
        // in `uiBufferIdx` and zeroed `uiBsPos` (Phase 6 session B).

        // Was `InitBits(&…sBsWrite, …pBsBuffer, …uiSize)`. The buffer and its length stay
        // where they were; the writer is a position, and resetting it is all `InitBits`
        // did that still means anything. Its `kpBuf: *const u8` parameter — stored as
        // `pStartBuf: *mut u8` and written through — is deleted rather than amended
        // (`phase2_findings.md` F13, third site).
        (*self.m_pSliceBs).sBsWrite = BsWriter::new();

        // CWelsLoadBalancingSlicingEncodingTask::InitTask runs the base first, then
        // stamps the start time.
        if self.RecordsSliceTime() {
            self.m_iSliceStart = WelsTime();
        }

        ENC_RETURN_SUCCESS
    }

    /// `CWelsSliceEncodingTask::FinishTask`
    pub unsafe fn FinishTask(&mut self) {
        let pCtx = self.m_pCtx;
        let pSmt = (*pCtx).pSliceThreading;

        with_wels_mutex((*pSmt).mutexThreadBsBufferUsage, || {
            (*pSmt).bThreadBsBufferUsage[self.m_iThreadIdx as usize] = false;
        });

        // sync multi-threading error
        with_wels_mutex((*pCtx).mutexEncoderError, || {
            if ENC_RETURN_SUCCESS != self.m_eTaskResult {
                (*pCtx).iEncoderError |= self.m_eTaskResult;
            }
        });

        // CWelsLoadBalancingSlicingEncodingTask::FinishTask runs the base first,
        // then records the elapsed time the load balancer reads next frame.
        if self.RecordsSliceTime() && !self.m_pSlice.is_null() {
            (*self.m_pSlice).uiSliceConsumeTime = (WelsTime() - self.m_iSliceStart) as u32;
        }
    }

    /// Emits the prefix NAL pair shared by both `ExecuteTask` bodies.
    unsafe fn WritePrefixNal(&mut self) {
        if self.m_bNeedPrefix {
            if self.m_eNalRefIdc != EWelsNalRefIdc::NRI_PRI_LOWEST {
                WelsLoadNalForSlice(
                    self.m_pSliceBs,
                    EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
                    self.m_eNalRefIdc as i32,
                );
                WelsWriteSVCPrefixNal(
                    thread_bs_buffer(self.m_pCtx, self.m_pSlice),
                    &mut (*self.m_pSliceBs).sBsWrite,
                    self.m_eNalRefIdc as i32,
                    EWelsNalUnitType::NAL_UNIT_CODED_SLICE_IDR == self.m_eNalType,
                );
                WelsUnloadNalForSlice(self.m_pSliceBs);
            } else {
                // No Prefix NAL Unit RBSP syntax here, but need add NAL Unit Header extension
                WelsLoadNalForSlice(
                    self.m_pSliceBs,
                    EWelsNalUnitType::NAL_UNIT_PREFIX as i32,
                    self.m_eNalRefIdc as i32,
                );
                WelsUnloadNalForSlice(self.m_pSliceBs);
            }
        }
    }

    /// `CWelsSliceEncodingTask::ExecuteTask`
    pub unsafe fn ExecuteTask(&mut self) -> i32 {
        let pCtx = self.m_pCtx;

        self.WritePrefixNal();

        WelsLoadNalForSlice(self.m_pSliceBs, self.m_eNalType as i32, self.m_eNalRefIdc as i32);
        debug_assert_eq!(self.m_iSliceIdx, (*self.m_pSlice).iSliceIdx);
        let mut iReturn = WelsCodeOneSlice(pCtx, self.m_pSlice, self.m_eNalType as i32);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }
        WelsUnloadNalForSlice(self.m_pSliceBs);

        self.m_iSliceSize = 0;
        iReturn = WriteSliceBs(pCtx, self.m_pSlice, self.m_iSliceIdx, &mut self.m_iSliceSize);
        if ENC_RETURN_SUCCESS != iReturn {
            return iReturn;
        }

        let pfDeblockingFilterSlice =
            (*(*pCtx).pFuncList).pfDeblocking.pfDeblockingFilterSlice.unwrap();
        pfDeblockingFilterSlice(current_layer(pCtx), (*pCtx).pFuncList, self.m_pSlice);

        ENC_RETURN_SUCCESS
    }

    /// `CWelsConstrainedSizeSlicingEncodingTask::ExecuteTask`
    pub unsafe fn ExecuteTaskConstrainedSize(&mut self) -> i32 {
        let pCtx = self.m_pCtx;
        let pCurDq = current_layer(pCtx);
        let kiSliceIdxStep = (*pCtx).iActiveThreadsNum as i32;
        let kiPartitionId = self.m_iSliceIdx % kiSliceIdxStep;
        let kiFirstMbInPartition = (*pCurDq).FirstMbIdxOfPartition[kiPartitionId as usize];
        let kiEndMbIdxInPartition = (*pCurDq).EndMbIdxOfPartition[kiPartitionId as usize];
        let kiCodedSliceNumByThread =
            (*pCurDq).sSliceBufferInfo[self.m_iThreadIdx as usize].iCodedSliceNum;
        // Phase 7's boundary holds: the task still takes `*mut SSlice` for the slice
        // it claims — derived at the claim, from the bank's root (T6.D8, S28).
        self.m_pSlice = crate::encoder::svc_encode_slice::slice_in_bank(
            pCurDq,
            self.m_iThreadIdx as usize,
            kiCodedSliceNumByThread,
        );
        (*self.m_pSlice).sSliceHeaderExt.sSliceHeader.iFirstMbInSlice = kiFirstMbInPartition;
        let mut iReturn;
        let mut bNeedReallocate;

        let iDiffMbIdx = kiEndMbIdxInPartition - kiFirstMbInPartition;
        if 0 == iDiffMbIdx {
            (*self.m_pSlice).iSliceIdx = -1;
            return ENC_RETURN_SUCCESS;
        }

        let mut iAnyMbLeftInPartition = iDiffMbIdx + 1;
        let mut iLocalSliceIdx = self.m_iSliceIdx;
        while iAnyMbLeftInPartition > 0 {
            bNeedReallocate = (*pCurDq).sSliceBufferInfo[self.m_iThreadIdx as usize].iCodedSliceNum
                >= (*pCurDq).sSliceBufferInfo[self.m_iThreadIdx as usize].iMaxSliceNum - 1;
            if bNeedReallocate {
                let pSmt = (*pCtx).pSliceThreading;
                iReturn = with_wels_mutex((*pSmt).mutexThreadSlcBuffReallocate, || {
                    // for memory statistic variable
                    ReallocateSliceInThread(
                        pCtx,
                        pCurDq,
                        (*pCtx).uiDependencyId as i32,
                        self.m_iThreadIdx,
                    )
                });
                if ENC_RETURN_SUCCESS != iReturn {
                    return iReturn;
                }
            }

            let mut pSlice: *mut SSlice = null_mut();
            iReturn = InitOneSliceInThread(
                pCtx,
                &mut pSlice,
                self.m_iThreadIdx,
                (*pCtx).uiDependencyId as i32,
                iLocalSliceIdx,
            );
            if iReturn != ENC_RETURN_SUCCESS {
                return iReturn;
            }
            self.m_pSlice = pSlice;
            self.m_pSliceBs = std::ptr::addr_of_mut!((*self.m_pSlice).sSliceBs);
            (*self.m_pSliceBs).sBsWrite = BsWriter::new();

            self.WritePrefixNal();

            WelsLoadNalForSlice(self.m_pSliceBs, self.m_eNalType as i32, self.m_eNalRefIdc as i32);

            debug_assert_eq!(iLocalSliceIdx, (*self.m_pSlice).iSliceIdx);
            iReturn = WelsCodeOneSlice(pCtx, self.m_pSlice, self.m_eNalType as i32);
            if ENC_RETURN_SUCCESS != iReturn {
                return iReturn;
            }
            WelsUnloadNalForSlice(self.m_pSliceBs);

            iReturn = WriteSliceBs(pCtx, self.m_pSlice, iLocalSliceIdx, &mut self.m_iSliceSize);
            if ENC_RETURN_SUCCESS != iReturn {
                return iReturn;
            }
            let pfDeblockingFilterSlice =
                (*(*pCtx).pFuncList).pfDeblocking.pfDeblockingFilterSlice.unwrap();
            pfDeblockingFilterSlice(pCurDq, (*pCtx).pFuncList, self.m_pSlice);

            iAnyMbLeftInPartition =
                kiEndMbIdxInPartition - (*pCurDq).LastCodedMbIdxOfPartition[kiPartitionId as usize];
            iLocalSliceIdx += kiSliceIdxStep;
            (*current_layer(pCtx)).sSliceBufferInfo[self.m_iThreadIdx as usize].iCodedSliceNum += 1;
        }

        ENC_RETURN_SUCCESS
    }
}

impl IWelsTask for CWelsBaseTask {
    fn Execute(&mut self) -> i32 {
        unsafe {
            match self.m_eKind {
                // CWelsUpdateMbMapTask::Execute
                ETaskKind::UpdateMbMap => {
                    UpdateMbListNeighborParallel(
                        current_layer(self.m_pCtx),
                        crate::encoder::svc_encode_slice::mb_list_root(current_layer(self.m_pCtx)),
                        self.m_iSliceIdx,
                    );
                    ENC_RETURN_SUCCESS
                }
                // CWelsSliceEncodingTask::Execute, shared by all three encoding
                // subclasses. Note the early return: a failed InitTask skips
                // both ExecuteTask and FinishTask.
                _ => {
                    self.m_eTaskResult = self.InitTask();
                    if self.m_eTaskResult != ENC_RETURN_SUCCESS {
                        return self.m_eTaskResult;
                    }

                    self.m_eTaskResult = if self.m_eKind == ETaskKind::ConstrainedSizeSlicing {
                        self.ExecuteTaskConstrainedSize()
                    } else {
                        self.ExecuteTask()
                    };

                    self.FinishTask();

                    self.m_eTaskResult
                }
            }
        }
    }

    fn GetSink(&mut self) -> Option<&mut (dyn IWelsTaskSink + 'static)> {
        if self.m_pSink.is_null() {
            None
        } else {
            unsafe { Some(&mut *self.m_pSink) }
        }
    }
}

/// Macroblock map update task (`CWelsUpdateMbMapTask`).
pub struct CWelsUpdateMbMapTask;

impl CWelsUpdateMbMapTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
    ) -> CWelsBaseTask {
        CWelsBaseTask::new(
            pSink,
            pCtx,
            iSliceIdx,
            WELS_ENC_TASK_UPDATEMBMAP as u32,
            ETaskKind::UpdateMbMap,
        )
    }
}

/// Standard slice encoding task (`CWelsSliceEncodingTask`).
pub struct CWelsSliceEncodingTask;

impl CWelsSliceEncodingTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
    ) -> CWelsBaseTask {
        CWelsBaseTask::new(
            pSink,
            pCtx,
            iSliceIdx,
            WELS_ENC_TASK_ENCODE_FIXED_SLICE as u32,
            ETaskKind::SliceEncoding,
        )
    }
}

/// Load-balanced slice encoding task (`CWelsLoadBalancingSlicingEncodingTask`).
pub struct CWelsLoadBalancingSlicingEncodingTask;

impl CWelsLoadBalancingSlicingEncodingTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
    ) -> CWelsBaseTask {
        CWelsBaseTask::new(
            pSink,
            pCtx,
            iSliceIdx,
            WELS_ENC_TASK_ENCODE_SLICE_LOADBALANCING as u32,
            ETaskKind::LoadBalancingSlicing,
        )
    }
}

/// Size-constrained slice encoding task (`CWelsConstrainedSizeSlicingEncodingTask`).
pub struct CWelsConstrainedSizeSlicingEncodingTask;

impl CWelsConstrainedSizeSlicingEncodingTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
    ) -> CWelsBaseTask {
        CWelsBaseTask::new(
            pSink,
            pCtx,
            iSliceIdx,
            WELS_ENC_TASK_ENCODE_SLICE_SIZECONSTRAINED as u32,
            ETaskKind::ConstrainedSizeSlicing,
        )
    }
}

/// Non-duplicated task list container (`TASKLIST_TYPE`).
#[repr(C)]
pub struct CWelsTaskList {
    pub tasks: Vec<*mut CWelsBaseTask>,
}

impl CWelsTaskList {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn push_back(&mut self, pTask: *mut CWelsBaseTask) -> bool {
        if pTask.is_null() {
            return false;
        }
        if !self.tasks.contains(&pTask) {
            self.tasks.push(pTask);
        }
        true
    }

    pub fn begin(&self) -> *mut CWelsBaseTask {
        if self.tasks.is_empty() {
            null_mut()
        } else {
            self.tasks[0]
        }
    }

    pub fn pop_front(&mut self) -> *mut CWelsBaseTask {
        if self.tasks.is_empty() {
            null_mut()
        } else {
            self.tasks.remove(0)
        }
    }

    pub fn getNode(&self, iIdx: i32) -> *mut CWelsBaseTask {
        let idx = iIdx as usize;
        if idx < self.tasks.len() {
            self.tasks[idx]
        } else {
            null_mut()
        }
    }

    pub fn size(&self) -> usize {
        self.tasks.len()
    }

    pub fn clear(&mut self) {
        self.tasks.clear();
    }
}

pub type TASKLIST_TYPE = CWelsTaskList;

/// Internal thread barrier synchronization primitive.
pub struct WelsTaskBarrier {
    pub lock: Mutex<i32>,
    pub cvar: Condvar,
}

impl WelsTaskBarrier {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    pub fn set_count(&self, wait_count: i32) {
        let mut count = self.lock.lock().unwrap();
        *count = wait_count;
    }

    pub fn wait_for_completion(&self) {
        let mut count = self.lock.lock().unwrap();
        while *count > 0 {
            count = self.cvar.wait(count).unwrap();
        }
    }

    pub fn decrement_and_signal(&self) {
        let mut count = self.lock.lock().unwrap();
        *count -= 1;
        if *count <= 0 {
            self.cvar.notify_all();
        }
    }
}

/// Abstract task management interface (`IWelsTaskManage`).
pub trait IWelsTaskManage {
    unsafe fn Init(&mut self, pEncCtx: *mut sWelsEncCtx) -> i32;
    unsafe fn Uninit(&mut self);
    unsafe fn InitFrame(&mut self, kiCurDid: i32);
    unsafe fn ExecuteTasks(&mut self, iTaskType: usize) -> i32;
    /// Three definitions of this name are correct: this trait declaration plus
    /// one impl each on CWelsTaskManageBase and CWelsTaskManageOne, matching
    /// the C++ virtual and its two overrides. --dups flags the group.
    fn GetThreadPoolThreadNum(&self) -> i32;
}

/// Standard multi-threaded task management implementation (`CWelsTaskManageBase`).
#[repr(C)]
pub struct CWelsTaskManageBase {
    pub m_pEncCtx: *mut sWelsEncCtx,
    pub m_pThreadPool: *mut CWelsThreadPool,
    pub m_pcAllTaskList: [[*mut CWelsTaskList; MAX_DEPENDENCY_LAYER]; WELS_ENC_TASK_ALL],
    pub m_cEncodingTaskList: [*mut CWelsTaskList; MAX_DEPENDENCY_LAYER],
    pub m_cPreEncodingTaskList: [*mut CWelsTaskList; MAX_DEPENDENCY_LAYER],
    pub m_iTaskNum: [i32; MAX_DEPENDENCY_LAYER],
    pub m_iThreadNum: i32,
    pub m_iWaitTaskNum: i32,
    pub m_iCurDid: i32,
    pub barrier: Arc<WelsTaskBarrier>,
}

impl CWelsTaskManageBase {
    pub fn new() -> Self {
        let mut encoding_list: [*mut CWelsTaskList; MAX_DEPENDENCY_LAYER] =
            [null_mut(); MAX_DEPENDENCY_LAYER];
        let mut pre_encoding_list: [*mut CWelsTaskList; MAX_DEPENDENCY_LAYER] =
            [null_mut(); MAX_DEPENDENCY_LAYER];
        let all_task_list: [[*mut CWelsTaskList; MAX_DEPENDENCY_LAYER]; WELS_ENC_TASK_ALL] =
            [[null_mut(); MAX_DEPENDENCY_LAYER]; WELS_ENC_TASK_ALL];

        for iDid in 0..MAX_DEPENDENCY_LAYER {
            let enc_list = Box::into_raw(Box::new(CWelsTaskList::new()));
            let pre_list = Box::into_raw(Box::new(CWelsTaskList::new()));
            encoding_list[iDid] = enc_list;
            pre_encoding_list[iDid] = pre_list;
        }

        Self {
            m_pEncCtx: null_mut(),
            m_pThreadPool: null_mut(),
            m_pcAllTaskList: all_task_list,
            m_cEncodingTaskList: encoding_list,
            m_cPreEncodingTaskList: pre_encoding_list,
            m_iTaskNum: [0; MAX_DEPENDENCY_LAYER],
            m_iThreadNum: 0,
            m_iWaitTaskNum: 0,
            m_iCurDid: 0,
            barrier: Arc::new(WelsTaskBarrier::new()),
        }
    }

    pub unsafe fn Init(&mut self, pEncCtx: *mut sWelsEncCtx) -> i32 {
        if pEncCtx.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }
        self.m_pEncCtx = pEncCtx;
        unsafe {
            if !ctx_param(pEncCtx).is_null() {
                self.m_iThreadNum = (*ctx_param(pEncCtx)).iMultipleThreadIdc as i32;
            } else {
                self.m_iThreadNum = 1;
            }
        }

        let _ = CWelsThreadPool::SetThreadNum(self.m_iThreadNum);
        self.m_pThreadPool = CWelsThreadPool::AddReference();
        if self.m_pThreadPool.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }

        let mut iReturn = ENC_RETURN_SUCCESS;
        for iDid in 0..MAX_DEPENDENCY_LAYER {
            self.m_pcAllTaskList[WELS_ENC_TASK_ENCODING][iDid] = self.m_cEncodingTaskList[iDid];
            self.m_pcAllTaskList[WELS_ENC_TASK_UPDATEMBMAP][iDid] =
                self.m_cPreEncodingTaskList[iDid];
            unsafe {
                iReturn |= self.CreateTasks(pEncCtx, iDid as i32);
            }
        }

        iReturn
    }

    pub unsafe fn Uninit(&mut self) {
        unsafe {
            self.DestroyTasks();
        }
        if !self.m_pThreadPool.is_null() {
            CWelsThreadPool::RemoveInstance();
            self.m_pThreadPool = null_mut();
        }

        for iDid in 0..MAX_DEPENDENCY_LAYER {
            if !self.m_cEncodingTaskList[iDid].is_null() {
                unsafe {
                    drop(Box::from_raw(self.m_cEncodingTaskList[iDid]));
                }
                self.m_cEncodingTaskList[iDid] = null_mut();
            }
            if !self.m_cPreEncodingTaskList[iDid].is_null() {
                unsafe {
                    drop(Box::from_raw(self.m_cPreEncodingTaskList[iDid]));
                }
                self.m_cPreEncodingTaskList[iDid] = null_mut();
            }
        }
    }

    pub unsafe fn CreateTasks(&mut self, pEncCtx: *mut sWelsEncCtx, kiCurDid: i32) -> i32 {
        if pEncCtx.is_null() {
            return ENC_RETURN_MEMALLOCERR;
        }

        let did = kiCurDid as usize;
        let mut uiSliceMode = crate::SliceMode::SM_SINGLE_SLICE;
        let mut kiTaskCount: i32 = 1;

        unsafe {
            if !ctx_param(pEncCtx).is_null() {
                let pParam = &*ctx_param(pEncCtx);
                uiSliceMode = pParam.sSpatialLayers[did].sSliceArgument.uiSliceMode;
                if uiSliceMode != crate::SliceMode::SM_SIZELIMITED_SLICE {
                    kiTaskCount =
                        pParam.sSpatialLayers[did].sSliceArgument.uiSliceNum as i32;
                    self.m_iTaskNum[did] = kiTaskCount;
                } else {
                    kiTaskCount = (*pEncCtx).iActiveThreadsNum as i32;
                    self.m_iTaskNum[did] = kiTaskCount;
                }
            }
        }

        let this_ptr = self as *mut CWelsTaskManageBase;

        // Pre-encoding tasks (CWelsUpdateMbMapTask)
        for idx in 0..kiTaskCount {
            let task = Box::into_raw(Box::new(CWelsUpdateMbMapTask::new(
                this_ptr, pEncCtx, idx,
            )));
            let base_task = task as *mut CWelsBaseTask;
            if !self.m_cPreEncodingTaskList[did].is_null() {
                unsafe {
                    (*self.m_cPreEncodingTaskList[did]).push_back(base_task);
                }
            }
        }

        // Encoding tasks
        let bUseLoadBalancing = unsafe {
            if !ctx_param(pEncCtx).is_null() {
                (*ctx_param(pEncCtx)).bUseLoadBalancing
            } else {
                false
            }
        };

        for idx in 0..kiTaskCount {
            let base_task: *mut CWelsBaseTask = if uiSliceMode == crate::SliceMode::SM_SIZELIMITED_SLICE {
                let task = Box::into_raw(Box::new(
                    CWelsConstrainedSizeSlicingEncodingTask::new(this_ptr, pEncCtx, idx),
                ));
                task as *mut CWelsBaseTask
            } else if bUseLoadBalancing {
                let task = Box::into_raw(Box::new(
                    CWelsLoadBalancingSlicingEncodingTask::new(this_ptr, pEncCtx, idx),
                ));
                task as *mut CWelsBaseTask
            } else {
                let task = Box::into_raw(Box::new(CWelsSliceEncodingTask::new(
                    this_ptr, pEncCtx, idx,
                )));
                task as *mut CWelsBaseTask
            };

            if !self.m_cEncodingTaskList[did].is_null() {
                unsafe {
                    (*self.m_cEncodingTaskList[did]).push_back(base_task);
                }
            }
        }

        ENC_RETURN_SUCCESS
    }

    pub unsafe fn DestroyTaskList(&mut self, pTargetTaskList: *mut CWelsTaskList) {
        if pTargetTaskList.is_null() {
            return;
        }
        unsafe {
            while !(*pTargetTaskList).begin().is_null() {
                let pTask = (*pTargetTaskList).begin();
                if !pTask.is_null() {
                    drop(Box::from_raw(pTask));
                }
                (*pTargetTaskList).pop_front();
            }
        }
    }

    pub unsafe fn DestroyTasks(&mut self) {
        for iDid in 0..MAX_DEPENDENCY_LAYER {
            if self.m_iTaskNum[iDid] > 0 {
                unsafe {
                    self.DestroyTaskList(self.m_cEncodingTaskList[iDid]);
                    self.DestroyTaskList(self.m_cPreEncodingTaskList[iDid]);
                }
                self.m_iTaskNum[iDid] = 0;
                self.m_pcAllTaskList[WELS_ENC_TASK_ENCODING][iDid] = null_mut();
            }
        }
    }

    pub fn OnTaskMinusOne(&mut self) {
        // WelsEventSignal (&m_hTaskEvent, &m_hEventMutex, &m_iWaitTaskNum) under
        // m_cWaitTaskNumLock: decrement the outstanding count and wake the
        // waiter once it reaches zero.
        self.barrier.decrement_and_signal();
    }

    pub unsafe fn ExecuteTaskList(&mut self, pTaskList: *const *mut CWelsTaskList) -> i32 {
        if pTaskList.is_null() {
            return ENC_RETURN_SUCCESS;
        }
        let did = self.m_iCurDid as usize;
        self.m_iWaitTaskNum = self.m_iTaskNum[did];
        if self.m_iWaitTaskNum == 0 {
            return ENC_RETURN_SUCCESS;
        }

        let pTargetTaskList = unsafe { *pTaskList.add(did) };
        if pTargetTaskList.is_null() {
            return ENC_RETURN_SUCCESS;
        }

        // if directly use m_iWaitTaskNum in the loop make cause sync problem
        let iCurrentTaskCount = self.m_iWaitTaskNum;
        self.barrier.set_count(iCurrentTaskCount);
        for iIdx in 0..iCurrentTaskCount {
            let task_node = unsafe { (*pTargetTaskList).getNode(iIdx) };
            if task_node.is_null() {
                self.barrier.decrement_and_signal();
                continue;
            }
            if !self.m_pThreadPool.is_null() {
                unsafe {
                    (*self.m_pThreadPool).QueueTask(task_node as *mut (dyn IWelsTask + 'static));
                }
            } else {
                // No pool: run inline and settle the barrier ourselves, since
                // nothing will deliver the OnTaskExecuted callback.
                unsafe {
                    (*task_node).Execute();
                }
                self.barrier.decrement_and_signal();
            }
        }

        self.barrier.wait_for_completion();
        ENC_RETURN_SUCCESS
    }

    pub unsafe fn InitFrame(&mut self, kiCurDid: i32) {
        self.m_iCurDid = kiCurDid;
        let mut bNeedAdjustingSlicing = false;
        unsafe {
            if !self.m_pEncCtx.is_null() && !current_layer(self.m_pEncCtx).is_null() {
                bNeedAdjustingSlicing = (*current_layer(self.m_pEncCtx)).bNeedAdjustingSlicing;
            }
        }
        if bNeedAdjustingSlicing {
            unsafe {
                self.ExecuteTaskList(self.m_pcAllTaskList[WELS_ENC_TASK_UPDATEMBMAP].as_ptr());
            }
        }
    }

    pub unsafe fn ExecuteTasks(&mut self, iTaskType: usize) -> i32 {
        let task_type = if iTaskType < WELS_ENC_TASK_ALL {
            iTaskType
        } else {
            WELS_ENC_TASK_ENCODING
        };
        unsafe { self.ExecuteTaskList(self.m_pcAllTaskList[task_type].as_ptr()) }
    }

    pub fn GetThreadPoolThreadNum(&self) -> i32 {
        if !self.m_pThreadPool.is_null() {
            unsafe { (*self.m_pThreadPool).GetThreadNum() }
        } else {
            1
        }
    }
}

/// `CWelsTaskManageBase` is the `IWelsTaskSink` every task reports to
/// (`OnTaskExecuted`/`OnTaskCancelled` both funnel into `OnTaskMinusOne`).
impl IWelsTaskSink for CWelsTaskManageBase {
    fn OnTaskExecuted(&mut self) -> i32 {
        self.OnTaskMinusOne();
        ENC_RETURN_SUCCESS
    }

    fn OnTaskCancelled(&mut self) -> i32 {
        self.OnTaskMinusOne();
        ENC_RETURN_SUCCESS
    }
}

// The manager pointer is shared with worker threads only as a sink; the sole
// mutable state it touches from them is `barrier`, which is itself synchronised.
unsafe impl Send for CWelsTaskManageBase {}
unsafe impl Sync for CWelsTaskManageBase {}

impl Default for CWelsTaskManageBase {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CWelsTaskManageBase {
    fn drop(&mut self) {
        unsafe {
            self.Uninit();
        }
    }
}

/// Single-threaded synchronous fallback task manager (`CWelsTaskManageOne`).
#[repr(C)]
pub struct CWelsTaskManageOne {
    pub base: CWelsTaskManageBase,
}

impl CWelsTaskManageOne {
    pub fn new() -> Self {
        Self {
            base: CWelsTaskManageBase::new(),
        }
    }

    pub unsafe fn Init(&mut self, pEncCtx: *mut sWelsEncCtx) -> i32 {
        self.base.m_pEncCtx = pEncCtx;
        unsafe { self.base.CreateTasks(pEncCtx, 0) }
    }

    pub unsafe fn ExecuteTasks(&mut self, _iTaskType: usize) -> i32 {
        let target_list = self.base.m_cEncodingTaskList[0];
        if !target_list.is_null() {
            unsafe {
                while !(*target_list).begin().is_null() {
                    let task = (*target_list).begin();
                    if !task.is_null() {
                        (*task).Execute();
                    }
                    (*target_list).pop_front();
                }
            }
        }
        ENC_RETURN_SUCCESS
    }

    pub fn GetThreadPoolThreadNum(&self) -> i32 {
        1
    }
}

impl Default for CWelsTaskManageOne {
    fn default() -> Self {
        Self::new()
    }
}

/// Static factory function (`IWelsTaskManage::CreateTaskManage`).
pub unsafe fn CreateTaskManage(
    pCtx: *mut sWelsEncCtx,
    _iSpatialLayer: i32,
    _bNeedLock: bool,
) -> *mut CWelsTaskManageBase {
    if pCtx.is_null() {
        return null_mut();
    }

    let pTaskManage = Box::into_raw(Box::new(CWelsTaskManageBase::new()));
    if pTaskManage.is_null() {
        return null_mut();
    }

    let err = unsafe { (*pTaskManage).Init(pCtx) };
    if err != ENC_RETURN_SUCCESS {
        unsafe {
            (*pTaskManage).Uninit();
            drop(Box::from_raw(pTaskManage));
        }
        return null_mut();
    }

    pTaskManage
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;
    #[test]
    fn test_task_list_operations() {
        let mut list = CWelsTaskList::new();
        assert_eq!(list.size(), 0);
        assert!(list.begin().is_null());

        let mut base_task = CWelsBaseTask::new(
            null_mut(),
            null_mut(),
            0,
            0,
            ETaskKind::SliceEncoding,
        );
        let ptr = &mut base_task as *mut CWelsBaseTask;

        assert!(list.push_back(ptr));
        assert_eq!(list.size(), 1);
        assert_eq!(list.begin(), ptr);
        assert_eq!(list.getNode(0), ptr);

        // Non-duplicated push
        assert!(list.push_back(ptr));
        assert_eq!(list.size(), 1);

        let popped = list.pop_front();
        assert_eq!(popped, ptr);
        assert_eq!(list.size(), 0);
    }

    #[test]
    fn test_task_manage_base_lifecycle() {
        let mut coding_param = SWelsSvcCodingParam::default();
        coding_param.iMultipleThreadIdc = 2;
        coding_param.sSpatialLayers[0].sSliceArgument.uiSliceMode =
            crate::SliceMode::SM_FIXEDSLCNUM_SLICE;
        coding_param.sSpatialLayers[0].sSliceArgument.uiSliceNum = 2;

        // T6.G2: the context names the current layer by *position*, so the test has
        // to stand up the one-entry list the position indexes into — which is what
        // `RequestMemorySvc` builds on the live path. **T6.H8**: the list owns the
        // layer now, so the fixture hands it a `Box` instead of borrowing a local.
        let mut enc_ctx = sWelsEncCtx::default();
        enc_ctx.pSvcParam = Some(Box::new(coding_param.clone()));
        enc_ctx.ppDqLayerList = vec![Some(Box::new(SDqLayer::default()))];
        enc_ctx.iCurDqLayer = Some(LayerIdx(0));

        unsafe {
            let pMgr = CreateTaskManage(&mut enc_ctx, 1, false);
            assert!(!pMgr.is_null());

            let mgr = &mut *pMgr;
            assert_eq!(mgr.m_iTaskNum[0], 2);

            // Safe on a Default context: bNeedAdjustingSlicing is false, so no
            // task list is dispatched.
            mgr.InitFrame(0);
            assert_eq!(mgr.m_iCurDid, 0);

            // As in test_task_manage_one_sync, ExecuteTasks is not called: the
            // task bodies now encode real slices and need a live encoder
            // context. The differential harness covers execution.
            drop(Box::from_raw(pMgr));
        }
    }

    #[test]
    fn test_task_manage_one_sync() {
        let mut coding_param = SWelsSvcCodingParam::default();
        coding_param.sSpatialLayers[0].sSliceArgument.uiSliceNum = 2;

        let mut enc_ctx = sWelsEncCtx::default();
        enc_ctx.pSvcParam = Some(Box::new(coding_param.clone()));

        unsafe {
            let mut one = CWelsTaskManageOne::new();
            assert_eq!(one.GetThreadPoolThreadNum(), 1);
            let init_res = one.Init(&mut enc_ctx);
            assert_eq!(init_res, ENC_RETURN_SUCCESS);

            // Init's job is to build the task lists; check that it did.
            let enc_list = one.base.m_cEncodingTaskList[0];
            assert!(!enc_list.is_null());
            assert_eq!((*enc_list).size(), 2);
        }

        // Deliberately not calling ExecuteTasks here. The tasks now carry the
        // real CWelsSliceEncodingTask body, which encodes a slice and needs a
        // fully initialised sWelsEncCtx (pCurDqLayer, pSliceThreading, the
        // function-pointer list); a Default context null-derefs. Executing
        // tasks is covered end to end by the differential harness instead --
        // see rust/docs/encoder_port_status.md, the iMultipleThreadIdc sweep.
        // Before the task bodies were filled in, this call "passed" only
        // because Execute() did nothing but signal its sink.
    }
}
