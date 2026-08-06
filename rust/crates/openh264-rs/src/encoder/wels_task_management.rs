//! Encoder Task Management Subsystem.
//!
//! Translated from `codec/encoder/core/inc/wels_task_management.h` and
//! `codec/encoder/core/src/wels_task_management.cpp`.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Arc, Condvar, Mutex};
pub use crate::encoder::encoder_context::SLogContext;
pub use crate::encoder::param_svc::SWelsSvcCodingParam;
pub use crate::encoder::svc_encode_slice::SDqLayer;
pub use crate::encoder::encoder_context::sWelsEncCtx;

pub const MAX_DEPENDENCY_LAYER: usize = 4;

// Return & Error Codes
pub const ENC_RETURN_SUCCESS: i32 = 0;
pub const ENC_RETURN_CORRECTING: i32 = 1;
pub const ENC_RETURN_MEMALLOCERR: i32 = 2;
pub const ENC_RETURN_UNEXPECTED: i32 = -1;

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
pub trait IWelsTaskSink {
    unsafe fn OnTaskExecuted(&mut self) -> i32;
    unsafe fn OnTaskCancelled(&mut self) -> i32;
}

/// Abstract task interface (`IWelsTask`).
pub trait IWelsTask {
    unsafe fn Execute(&mut self) -> i32;
    fn GetTaskType(&self) -> u32;
}

/// Base task representation (`CWelsBaseTask`).
#[repr(C)]
pub struct CWelsBaseTask {
    pub m_pSink: *mut CWelsTaskManageBase,
    pub m_pCtx: *mut sWelsEncCtx,
    pub m_iSliceIdx: i32,
    pub m_uiTaskType: u32,
}

impl CWelsBaseTask {
    pub fn new(
        pSink: *mut CWelsTaskManageBase,
        pCtx: *mut sWelsEncCtx,
        iSliceIdx: i32,
        uiTaskType: u32,
    ) -> Self {
        Self {
            m_pSink: pSink,
            m_pCtx: pCtx,
            m_iSliceIdx: iSliceIdx,
            m_uiTaskType: uiTaskType,
        }
    }

    pub unsafe fn Execute(&mut self) -> i32 {
        if !self.m_pSink.is_null() {
            unsafe {
                (*self.m_pSink).OnTaskExecuted();
            }
        }
        ENC_RETURN_SUCCESS
    }

    pub fn GetTaskType(&self) -> u32 {
        self.m_uiTaskType
    }
}

/// Macroblock map update task (`CWelsUpdateMbMapTask`).
#[repr(C)]
pub struct CWelsUpdateMbMapTask {
    pub base: CWelsBaseTask,
}

impl CWelsUpdateMbMapTask {
    pub fn new(pSink: *mut CWelsTaskManageBase, pCtx: *mut sWelsEncCtx, iSliceIdx: i32) -> Self {
        Self {
            base: CWelsBaseTask::new(
                pSink,
                pCtx,
                iSliceIdx,
                WELS_ENC_TASK_UPDATEMBMAP as u32,
            ),
        }
    }

    pub unsafe fn Execute(&mut self) -> i32 {
        self.base.Execute()
    }

    pub fn GetTaskType(&self) -> u32 {
        WELS_ENC_TASK_UPDATEMBMAP as u32
    }
}

/// Standard slice encoding task (`CWelsSliceEncodingTask`).
#[repr(C)]
pub struct CWelsSliceEncodingTask {
    pub base: CWelsBaseTask,
    pub m_iStartMbIdx: i32,
    pub m_iEndMbIdx: i32,
    pub m_iSliceSize: i32,
}

impl CWelsSliceEncodingTask {
    pub fn new(pSink: *mut CWelsTaskManageBase, pCtx: *mut sWelsEncCtx, iSliceIdx: i32) -> Self {
        Self {
            base: CWelsBaseTask::new(
                pSink,
                pCtx,
                iSliceIdx,
                WELS_ENC_TASK_ENCODE_FIXED_SLICE as u32,
            ),
            m_iStartMbIdx: 0,
            m_iEndMbIdx: 0,
            m_iSliceSize: 0,
        }
    }

    pub unsafe fn Execute(&mut self) -> i32 {
        self.base.Execute()
    }

    pub fn GetTaskType(&self) -> u32 {
        WELS_ENC_TASK_ENCODE_FIXED_SLICE as u32
    }
}

/// Load-balanced slice encoding task (`CWelsLoadBalancingSlicingEncodingTask`).
#[repr(C)]
pub struct CWelsLoadBalancingSlicingEncodingTask {
    pub slice_task: CWelsSliceEncodingTask,
    pub m_iSliceStart: i64,
}

impl CWelsLoadBalancingSlicingEncodingTask {
    pub fn new(pSink: *mut CWelsTaskManageBase, pCtx: *mut sWelsEncCtx, iSliceIdx: i32) -> Self {
        let mut slice_task = CWelsSliceEncodingTask::new(pSink, pCtx, iSliceIdx);
        slice_task.base.m_uiTaskType = WELS_ENC_TASK_ENCODE_SLICE_LOADBALANCING as u32;
        Self {
            slice_task,
            m_iSliceStart: 0,
        }
    }

    pub unsafe fn Execute(&mut self) -> i32 {
        self.slice_task.Execute()
    }

    pub fn GetTaskType(&self) -> u32 {
        WELS_ENC_TASK_ENCODE_SLICE_LOADBALANCING as u32
    }
}

/// Size-constrained slice encoding task (`CWelsConstrainedSizeSlicingEncodingTask`).
#[repr(C)]
pub struct CWelsConstrainedSizeSlicingEncodingTask {
    pub lb_task: CWelsLoadBalancingSlicingEncodingTask,
}

impl CWelsConstrainedSizeSlicingEncodingTask {
    pub fn new(pSink: *mut CWelsTaskManageBase, pCtx: *mut sWelsEncCtx, iSliceIdx: i32) -> Self {
        let mut lb_task = CWelsLoadBalancingSlicingEncodingTask::new(pSink, pCtx, iSliceIdx);
        lb_task.slice_task.base.m_uiTaskType =
            WELS_ENC_TASK_ENCODE_SLICE_SIZECONSTRAINED as u32;
        Self { lb_task }
    }

    pub unsafe fn Execute(&mut self) -> i32 {
        self.lb_task.Execute()
    }

    pub fn GetTaskType(&self) -> u32 {
        WELS_ENC_TASK_ENCODE_SLICE_SIZECONSTRAINED as u32
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

/// Shared thread pool representation (`CWelsThreadPool`).
pub struct CWelsThreadPool {
    pub m_iThreadNum: i32,
    pub m_iRefCount: i32,
}

static mut G_THREAD_POOL: *mut CWelsThreadPool = null_mut();

impl CWelsThreadPool {
    pub fn new() -> Self {
        Self {
            m_iThreadNum: 4,
            m_iRefCount: 0,
        }
    }

    pub fn SetThreadNum(iMaxThreadNum: i32) -> i32 {
        unsafe {
            if G_THREAD_POOL.is_null() {
                let pool = Box::into_raw(Box::new(CWelsThreadPool::new()));
                G_THREAD_POOL = pool;
            }
            let pool = &mut *G_THREAD_POOL;
            pool.m_iThreadNum = if iMaxThreadNum > 0 {
                iMaxThreadNum
            } else {
                1
            };
            ENC_RETURN_SUCCESS
        }
    }

    pub fn AddReference() -> *mut CWelsThreadPool {
        unsafe {
            if G_THREAD_POOL.is_null() {
                let pool = Box::into_raw(Box::new(CWelsThreadPool::new()));
                G_THREAD_POOL = pool;
            }
            let pool = &mut *G_THREAD_POOL;
            pool.m_iRefCount += 1;
            G_THREAD_POOL
        }
    }

    pub fn RemoveInstance(&mut self) {
        self.m_iRefCount -= 1;
    }

    pub fn GetThreadNum(&self) -> i32 {
        self.m_iThreadNum
    }

    pub unsafe fn QueueTask(&mut self, pTask: *mut CWelsBaseTask) -> i32 {
        if !pTask.is_null() {
            unsafe {
                (*pTask).Execute();
            }
        }
        ENC_RETURN_SUCCESS
    }
}

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
            if !(*pEncCtx).pSvcParam.is_null() {
                self.m_iThreadNum = (*(*pEncCtx).pSvcParam).iMultipleThreadIdc as i32;
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
            unsafe {
                (*self.m_pThreadPool).RemoveInstance();
            }
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
            if !(*pEncCtx).pSvcParam.is_null() {
                let pParam = &*(*pEncCtx).pSvcParam;
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
            if !(*pEncCtx).pSvcParam.is_null() {
                (*(*pEncCtx).pSvcParam).bUseLoadBalancing
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
        self.m_iWaitTaskNum -= 1;
        self.barrier.decrement_and_signal();
    }

    pub unsafe fn OnTaskExecuted(&mut self) -> i32 {
        self.OnTaskMinusOne();
        ENC_RETURN_SUCCESS
    }

    pub unsafe fn OnTaskCancelled(&mut self) -> i32 {
        self.OnTaskMinusOne();
        ENC_RETURN_SUCCESS
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

        let iCurrentTaskCount = self.m_iWaitTaskNum;
        self.barrier.set_count(iCurrentTaskCount);
        for iIdx in 0..iCurrentTaskCount {
            let task_node = unsafe { (*pTargetTaskList).getNode(iIdx) };
            if !self.m_pThreadPool.is_null() {
                unsafe {
                    (*self.m_pThreadPool).QueueTask(task_node);
                }
            } else if !task_node.is_null() {
                unsafe {
                    (*task_node).Execute();
                }
            }
        }

        self.barrier.wait_for_completion();
        ENC_RETURN_SUCCESS
    }

    pub unsafe fn InitFrame(&mut self, kiCurDid: i32) {
        self.m_iCurDid = kiCurDid;
        let mut bNeedAdjustingSlicing = false;
        unsafe {
            if !self.m_pEncCtx.is_null() && !(*self.m_pEncCtx).pCurDqLayer.is_null() {
                bNeedAdjustingSlicing = (*(*self.m_pEncCtx).pCurDqLayer).bNeedAdjustingSlicing;
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
    fn test_task_list_operations() {
        let mut list = CWelsTaskList::new();
        assert_eq!(list.size(), 0);
        assert!(list.begin().is_null());

        let mut base_task = CWelsBaseTask::new(null_mut(), null_mut(), 0, 0);
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

        let mut dq_layer = SDqLayer::default();
        let mut enc_ctx = sWelsEncCtx::default();
        enc_ctx.pSvcParam = &mut coding_param;
        enc_ctx.pCurDqLayer = &mut dq_layer;

        unsafe {
            let pMgr = CreateTaskManage(&mut enc_ctx, 1, false);
            assert!(!pMgr.is_null());

            let mgr = &mut *pMgr;
            assert_eq!(mgr.m_iTaskNum[0], 2);

            mgr.InitFrame(0);
            let ret = mgr.ExecuteTasks(WELS_ENC_TASK_ENCODING);
            assert_eq!(ret, ENC_RETURN_SUCCESS);

            drop(Box::from_raw(pMgr));
        }
    }

    #[test]
    fn test_task_manage_one_sync() {
        let mut coding_param = SWelsSvcCodingParam::default();
        coding_param.sSpatialLayers[0].sSliceArgument.uiSliceNum = 2;

        let mut enc_ctx = sWelsEncCtx::default();
        enc_ctx.pSvcParam = &mut coding_param;

        unsafe {
            let mut one = CWelsTaskManageOne::new();
            assert_eq!(one.GetThreadPoolThreadNum(), 1);
            let init_res = one.Init(&mut enc_ctx);
            assert_eq!(init_res, ENC_RETURN_SUCCESS);

            let exec_res = one.ExecuteTasks(WELS_ENC_TASK_ENCODING);
            assert_eq!(exec_res, ENC_RETURN_SUCCESS);
        }
    }
}
