// Copyright (c) 2009-2015, Cisco Systems
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

//! # Thread Pool Engine (`WelsThreadPool`)
//!
//! Translated from `codec/common/inc/WelsThreadPool.h` and `codec/common/src/WelsThreadPool.cpp`.
//!
//! Provides OpenH264's centralized multi-threading task scheduler, persistent worker
//! thread management, dual fast-path and queued work dispatching, and reference-counted
//! singleton lifecycle management.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_unsafe
)]

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

// ============================================================================
// Constants and Error Codes
// ============================================================================

pub const DEFAULT_THREAD_NUM: i32 = 4;
pub const WELS_THREAD_ERROR_OK: i32 = 0;
pub const WELS_THREAD_ERROR_GENERAL: i32 = 1;
pub type WELS_THREAD_ERROR_CODE = i32;

// ============================================================================
// Task & Sink Interfaces / Traits
// ============================================================================

/// Task sink interface receiving completion and cancellation callbacks.
pub trait IWelsTaskSink: Send + Sync {
    fn OnTaskExecuted(&mut self) -> i32;
    fn OnTaskCancelled(&mut self) -> i32;
}

/// Task interface to be executed by worker threads in the thread pool.
pub trait IWelsTask: Send + Sync {
    fn Execute(&mut self) -> i32;
    fn GetSink(&mut self) -> Option<&mut (dyn IWelsTaskSink + 'static)> {
        None
    }
}

/// Wrapper for fat dynamic trait pointers to support `Copy`, `Clone`, and `PartialEq`.
#[derive(Copy, Clone)]
pub struct TaskPtr(pub *mut (dyn IWelsTask + 'static));

unsafe impl Send for TaskPtr {}
unsafe impl Sync for TaskPtr {}

impl PartialEq for TaskPtr {
    fn eq(&self, other: &Self) -> bool {
        (self.0 as *const ()) == (other.0 as *const ())
    }
}

/// Sink callback interface invoked by worker threads on task state transitions.
pub trait IWelsTaskThreadSink: Send + Sync {
    fn OnTaskStart(
        &mut self,
        pThread: *mut CWelsTaskThread,
        pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE;
    fn OnTaskStop(
        &mut self,
        pThread: *mut CWelsTaskThread,
        pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE;
}

// ============================================================================
// List Containers: CWelsList & CWelsNonDuplicatedList
// ============================================================================

/// Doubly linked/contiguous list container matching `CWelsList<T>`.
#[derive(Debug, Clone)]
pub struct CWelsList<T: PartialEq + Copy> {
    items: Vec<T>,
}

impl<T: PartialEq + Copy> Default for CWelsList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PartialEq + Copy> CWelsList<T> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    #[inline]
    pub fn size(&self) -> i32 {
        self.items.len() as i32
    }

    #[inline]
    pub fn push_back(&mut self, item: T) -> bool {
        self.items.push(item);
        true
    }

    #[inline]
    pub fn begin(&self) -> Option<T> {
        self.items.first().copied()
    }

    #[inline]
    pub fn pop_front(&mut self) {
        if !self.items.is_empty() {
            self.items.remove(0);
        }
    }

    #[inline]
    pub fn erase(&mut self, item: T) -> bool {
        if let Some(pos) = self.items.iter().position(|x| *x == item) {
            self.items.remove(pos);
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn findNode(&self, item: T) -> bool {
        self.items.iter().any(|x| *x == item)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// Non-duplicated list container matching `CWelsNonDuplicatedList<T>`.
#[derive(Debug, Clone)]
pub struct CWelsNonDuplicatedList<T: PartialEq + Copy> {
    pub inner: CWelsList<T>,
}

impl<T: PartialEq + Copy> Default for CWelsNonDuplicatedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PartialEq + Copy> CWelsNonDuplicatedList<T> {
    pub fn new() -> Self {
        Self {
            inner: CWelsList::new(),
        }
    }

    #[inline]
    pub fn size(&self) -> i32 {
        self.inner.size()
    }

    #[inline]
    pub fn push_back(&mut self, item: T) -> bool {
        if self.inner.size() != 0 && self.inner.findNode(item) {
            return false;
        }
        self.inner.push_back(item)
    }

    #[inline]
    pub fn begin(&self) -> Option<T> {
        self.inner.begin()
    }

    #[inline]
    pub fn pop_front(&mut self) {
        self.inner.pop_front();
    }

    #[inline]
    pub fn erase(&mut self, item: T) -> bool {
        self.inner.erase(item)
    }

    #[inline]
    pub fn findNode(&self, item: T) -> bool {
        self.inner.findNode(item)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

// ============================================================================
// Worker Thread: CWelsTaskThread
// ============================================================================

/// Worker thread implementation executing tasks assigned by `CWelsThreadPool`.
pub struct CWelsTaskThread {
    pub m_uiID: usize,
    pub m_pSink: *mut dyn IWelsTaskThreadSink,
    pub m_pTask: Mutex<Option<TaskPtr>>,
    pub m_cLockTask: Mutex<()>,
    pub m_running: Arc<Mutex<bool>>,
    pub m_end_flag: Arc<Mutex<bool>>,
    pub m_cond: Arc<Condvar>,
    pub m_mutex: Arc<Mutex<()>>,
    pub m_handle: Mutex<Option<JoinHandle<()>>>,
}

unsafe impl Send for CWelsTaskThread {}
unsafe impl Sync for CWelsTaskThread {}

impl CWelsTaskThread {
    pub fn new(pSink: *mut dyn IWelsTaskThreadSink) -> *mut Self {
        let boxed = Box::new(Self {
            m_uiID: 0,
            m_pSink: pSink,
            m_pTask: Mutex::new(None),
            m_cLockTask: Mutex::new(()),
            m_running: Arc::new(Mutex::new(false)),
            m_end_flag: Arc::new(Mutex::new(false)),
            m_cond: Arc::new(Condvar::new()),
            m_mutex: Arc::new(Mutex::new(())),
            m_handle: Mutex::new(None),
        });
        let raw = Box::into_raw(boxed);
        unsafe {
            (*raw).m_uiID = raw as usize;
        }
        raw
    }

    pub fn Start(ptr: *mut Self) -> WELS_THREAD_ERROR_CODE {
        unsafe {
            if ptr.is_null() {
                return WELS_THREAD_ERROR_GENERAL;
            }
            let this = &*ptr;
            if let Ok(mut r) = this.m_running.lock() {
                *r = true;
            }
            if let Ok(mut e) = this.m_end_flag.lock() {
                *e = false;
            }

            let raw_addr = ptr as usize;
            let running_clone = Arc::clone(&this.m_running);
            let end_flag_clone = Arc::clone(&this.m_end_flag);
            let cond_clone = Arc::clone(&this.m_cond);
            let mutex_clone = Arc::clone(&this.m_mutex);

            let handle = thread::Builder::new()
                .name("CWelsTaskThread".to_string())
                .spawn(move || {
                    let pThread = raw_addr as *mut CWelsTaskThread;
                    loop {
                        let mut guard = mutex_clone.lock().unwrap();
                        while !*end_flag_clone.lock().unwrap()
                            && unsafe { (*pThread).m_pTask.lock().unwrap().is_none() }
                        {
                            guard = cond_clone.wait(guard).unwrap();
                        }
                        if *end_flag_clone.lock().unwrap() {
                            break;
                        }
                        drop(guard);

                        unsafe {
                            (*pThread).ExecuteTask();
                        }
                    }
                    if let Ok(mut r) = running_clone.lock() {
                        *r = false;
                    }
                });

            match handle {
                Ok(h) => {
                    *this.m_handle.lock().unwrap() = Some(h);
                    WELS_THREAD_ERROR_OK
                }
                Err(_) => {
                    if let Ok(mut r) = this.m_running.lock() {
                        *r = false;
                    }
                    WELS_THREAD_ERROR_GENERAL
                }
            }
        }
    }

    pub unsafe fn ExecuteTask(&self) {
        let _guard = self.m_cLockTask.lock().unwrap();
        let pTask = *self.m_pTask.lock().unwrap();
        let raw_self = self as *const Self as *mut Self;

        if !self.m_pSink.is_null() {
            (*self.m_pSink).OnTaskStart(raw_self, pTask);
        }

        if let Some(task) = pTask {
            if !task.0.is_null() {
                (*task.0).Execute();
            }
        }

        if !self.m_pSink.is_null() {
            (*self.m_pSink).OnTaskStop(raw_self, pTask);
        }

        *self.m_pTask.lock().unwrap() = None;
    }

    pub unsafe fn SetTask(&self, pTask: *mut (dyn IWelsTask + 'static)) -> WELS_THREAD_ERROR_CODE {
        let _guard_task = self.m_cLockTask.lock().unwrap();
        if !*self.m_running.lock().unwrap() {
            return WELS_THREAD_ERROR_GENERAL;
        }

        {
            let mut task_guard = self.m_pTask.lock().unwrap();
            *task_guard = if pTask.is_null() {
                None
            } else {
                Some(TaskPtr(pTask))
            };
        }

        self.m_cond.notify_all();
        WELS_THREAD_ERROR_OK
    }

    pub fn Kill(ptr: *mut Self) {
        unsafe {
            if ptr.is_null() {
                return;
            }
            let this = &*ptr;
            if let Ok(mut e) = this.m_end_flag.lock() {
                *e = true;
            }
            this.m_cond.notify_all();
            let handle = this.m_handle.lock().unwrap().take();
            if let Some(h) = handle {
                let _ = h.join();
            }
            if let Ok(mut r) = this.m_running.lock() {
                *r = false;
            }
        }
    }

    pub fn GetID(&self) -> usize {
        self.m_uiID
    }
}

// ============================================================================
// Thread Pool Core Engine: CWelsThreadPool
// ============================================================================

static GLOBAL_REF_COUNT: AtomicI32 = AtomicI32::new(0);
static GLOBAL_MAX_THREAD_NUM: AtomicI32 = AtomicI32::new(DEFAULT_THREAD_NUM);
static GLOBAL_INIT_MUTEX: Mutex<()> = Mutex::new(());
static mut GLOBAL_THREAD_POOL: *mut CWelsThreadPool = std::ptr::null_mut();

/// Centralized worker thread pool implementation matching `CWelsThreadPool`.
pub struct CWelsThreadPool {
    pub m_cWaitedTasks: Mutex<CWelsNonDuplicatedList<TaskPtr>>,
    pub m_cIdleThreads: Mutex<CWelsNonDuplicatedList<usize>>,
    pub m_cBusyThreads: Mutex<CWelsList<usize>>,

    pub m_cLockPool: Mutex<()>,

    pub m_running: Arc<Mutex<bool>>,
    pub m_end_flag: Arc<Mutex<bool>>,
    pub m_cond: Arc<Condvar>,
    pub m_mutex: Arc<Mutex<()>>,
    pub m_handle: Mutex<Option<JoinHandle<()>>>,
}

unsafe impl Send for CWelsThreadPool {}
unsafe impl Sync for CWelsThreadPool {}

impl IWelsTaskThreadSink for CWelsThreadPool {
    fn OnTaskStart(
        &mut self,
        pThread: *mut CWelsTaskThread,
        _pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE {
        self.AddThreadToBusyList(pThread)
    }

    fn OnTaskStop(
        &mut self,
        pThread: *mut CWelsTaskThread,
        pTask: Option<TaskPtr>,
    ) -> WELS_THREAD_ERROR_CODE {
        self.RemoveThreadFromBusyList(pThread);
        self.AddThreadToIdleQueue(pThread);

        if let Some(task) = pTask {
            unsafe {
                if !task.0.is_null() {
                    if let Some(sink) = (*task.0).GetSink() {
                        sink.OnTaskExecuted();
                    }
                }
            }
        }

        self.SignalThread();
        WELS_THREAD_ERROR_OK
    }
}

impl CWelsThreadPool {
    pub fn new() -> *mut Self {
        let boxed = Box::new(Self {
            m_cWaitedTasks: Mutex::new(CWelsNonDuplicatedList::new()),
            m_cIdleThreads: Mutex::new(CWelsNonDuplicatedList::new()),
            m_cBusyThreads: Mutex::new(CWelsList::new()),
            m_cLockPool: Mutex::new(()),
            m_running: Arc::new(Mutex::new(false)),
            m_end_flag: Arc::new(Mutex::new(false)),
            m_cond: Arc::new(Condvar::new()),
            m_mutex: Arc::new(Mutex::new(())),
            m_handle: Mutex::new(None),
        });
        Box::into_raw(boxed)
    }

    // ------------------------------------------------------------------------
    // Static Singleton Management Interface
    // ------------------------------------------------------------------------

    pub fn SetThreadNum(mut iMaxThreadNum: i32) -> WELS_THREAD_ERROR_CODE {
        let _guard = GLOBAL_INIT_MUTEX.lock().unwrap();

        if GLOBAL_REF_COUNT.load(Ordering::SeqCst) != 0 {
            return WELS_THREAD_ERROR_GENERAL;
        }

        if iMaxThreadNum <= 0 {
            iMaxThreadNum = 1;
        }
        GLOBAL_MAX_THREAD_NUM.store(iMaxThreadNum, Ordering::SeqCst);
        WELS_THREAD_ERROR_OK
    }

    pub fn AddReference() -> *mut CWelsThreadPool {
        let _guard = GLOBAL_INIT_MUTEX.lock().unwrap();

        unsafe {
            if GLOBAL_THREAD_POOL.is_null() {
                GLOBAL_THREAD_POOL = CWelsThreadPool::new();
                if GLOBAL_THREAD_POOL.is_null() {
                    return std::ptr::null_mut();
                }
            }

            if GLOBAL_REF_COUNT.load(Ordering::SeqCst) == 0 {
                if (*GLOBAL_THREAD_POOL).Init() != WELS_THREAD_ERROR_OK {
                    (*GLOBAL_THREAD_POOL).Uninit();
                    let _ = Box::from_raw(GLOBAL_THREAD_POOL);
                    GLOBAL_THREAD_POOL = std::ptr::null_mut();
                    return std::ptr::null_mut();
                }
            }

            GLOBAL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
            GLOBAL_THREAD_POOL
        }
    }

    pub fn RemoveInstance() {
        let _guard = GLOBAL_INIT_MUTEX.lock().unwrap();

        unsafe {
            let count = GLOBAL_REF_COUNT.fetch_sub(1, Ordering::SeqCst) - 1;
            if count == 0 && !GLOBAL_THREAD_POOL.is_null() {
                (*GLOBAL_THREAD_POOL).StopAllRunning();
                (*GLOBAL_THREAD_POOL).Uninit();
                let _ = Box::from_raw(GLOBAL_THREAD_POOL);
                GLOBAL_THREAD_POOL = std::ptr::null_mut();
            }
        }
    }

    pub fn IsReferenced() -> bool {
        let _guard = GLOBAL_INIT_MUTEX.lock().unwrap();
        GLOBAL_REF_COUNT.load(Ordering::SeqCst) > 0
    }

    // ------------------------------------------------------------------------
    // Instance Task Scheduling & Dispatch Pipeline
    // ------------------------------------------------------------------------

    pub fn QueueTask(&self, pTask: *mut (dyn IWelsTask + 'static)) -> WELS_THREAD_ERROR_CODE {
        let _guard = self.m_cLockPool.lock().unwrap();

        if self.GetWaitedTaskNum() == 0 {
            let pThread = self.GetIdleThread();
            if !pThread.is_null() {
                unsafe {
                    (*pThread).SetTask(pTask);
                }
                return WELS_THREAD_ERROR_OK;
            }
        }

        if !self.AddTaskToWaitedList(pTask) {
            return WELS_THREAD_ERROR_GENERAL;
        }

        self.SignalThread();
        WELS_THREAD_ERROR_OK
    }

    pub fn ExecuteTask(&self) {
        while self.GetWaitedTaskNum() > 0 {
            let pThread = self.GetIdleThread();
            if pThread.is_null() {
                break;
            }

            let pTask = self.GetWaitedTask();
            if let Some(task) = pTask {
                unsafe {
                    (*pThread).SetTask(task.0);
                }
            } else {
                self.AddThreadToIdleQueue(pThread);
            }
        }
    }

    pub fn SignalThread(&self) {
        self.m_cond.notify_all();
    }

    // ------------------------------------------------------------------------
    // Internal Lifecycle & Thread Management
    // ------------------------------------------------------------------------

    pub fn Init(&self) -> WELS_THREAD_ERROR_CODE {
        let _guard = self.m_cLockPool.lock().unwrap();

        let max_threads = GLOBAL_MAX_THREAD_NUM.load(Ordering::SeqCst);
        for _ in 0..max_threads {
            if self.CreateIdleThread() != WELS_THREAD_ERROR_OK {
                return WELS_THREAD_ERROR_GENERAL;
            }
        }

        self.Start()
    }

    pub fn Start(&self) -> WELS_THREAD_ERROR_CODE {
        if let Ok(mut r) = self.m_running.lock() {
            *r = true;
        }
        if let Ok(mut e) = self.m_end_flag.lock() {
            *e = false;
        }

        let raw_addr = self as *const Self as usize;
        let running_clone = Arc::clone(&self.m_running);
        let end_flag_clone = Arc::clone(&self.m_end_flag);
        let cond_clone = Arc::clone(&self.m_cond);
        let mutex_clone = Arc::clone(&self.m_mutex);

        let handle = thread::Builder::new()
            .name("CWelsThreadPool".to_string())
            .spawn(move || {
                let pool = raw_addr as *mut CWelsThreadPool;
                loop {
                    let mut guard = mutex_clone.lock().unwrap();
                    while !*end_flag_clone.lock().unwrap() && unsafe { (*pool).GetWaitedTaskNum() == 0 } {
                        guard = cond_clone.wait(guard).unwrap();
                    }
                    if *end_flag_clone.lock().unwrap() {
                        break;
                    }
                    drop(guard);

                    unsafe {
                        (*pool).ExecuteTask();
                    }
                }
                if let Ok(mut r) = running_clone.lock() {
                    *r = false;
                }
            });

        match handle {
            Ok(h) => {
                *self.m_handle.lock().unwrap() = Some(h);
                WELS_THREAD_ERROR_OK
            }
            Err(_) => {
                if let Ok(mut r) = self.m_running.lock() {
                    *r = false;
                }
                WELS_THREAD_ERROR_GENERAL
            }
        }
    }

    pub fn StopAllRunning(&self) -> WELS_THREAD_ERROR_CODE {
        self.ClearWaitedTasks();

        while self.GetBusyThreadNum() > 0 {
            thread::sleep(Duration::from_millis(10));
        }

        let max_threads = GLOBAL_MAX_THREAD_NUM.load(Ordering::SeqCst);
        if self.GetIdleThreadNum() != max_threads {
            return WELS_THREAD_ERROR_GENERAL;
        }

        WELS_THREAD_ERROR_OK
    }

    pub fn Uninit(&self) -> WELS_THREAD_ERROR_CODE {
        let _guard = self.m_cLockPool.lock().unwrap();
        let iReturn = self.StopAllRunning();

        {
            let mut idle = self.m_cIdleThreads.lock().unwrap();
            while idle.size() > 0 {
                if let Some(thread_addr) = idle.begin() {
                    self.DestroyThread(thread_addr as *mut CWelsTaskThread);
                    idle.pop_front();
                }
            }
        }

        self.Kill();

        self.m_cWaitedTasks.lock().unwrap().clear();
        self.m_cIdleThreads.lock().unwrap().clear();
        self.m_cBusyThreads.lock().unwrap().clear();

        iReturn
    }

    pub fn Kill(&self) {
        if let Ok(mut e) = self.m_end_flag.lock() {
            *e = true;
        }
        self.SignalThread();
        let handle = self.m_handle.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
        if let Ok(mut r) = self.m_running.lock() {
            *r = false;
        }
    }

    pub fn CreateIdleThread(&self) -> WELS_THREAD_ERROR_CODE {
        let pThread = CWelsTaskThread::new(self as *const Self as *mut Self as *mut dyn IWelsTaskThreadSink);
        if pThread.is_null() {
            return WELS_THREAD_ERROR_GENERAL;
        }

        if CWelsTaskThread::Start(pThread) != WELS_THREAD_ERROR_OK {
            CWelsTaskThread::Kill(pThread);
            let _ = unsafe { Box::from_raw(pThread) };
            return WELS_THREAD_ERROR_GENERAL;
        }

        self.AddThreadToIdleQueue(pThread);
        WELS_THREAD_ERROR_OK
    }

    pub fn DestroyThread(&self, pThread: *mut CWelsTaskThread) {
        if !pThread.is_null() {
            CWelsTaskThread::Kill(pThread);
            let _ = unsafe { Box::from_raw(pThread) };
        }
    }

    // ------------------------------------------------------------------------
    // Thread & Task List Management Helpers
    // ------------------------------------------------------------------------

    pub fn AddThreadToIdleQueue(&self, pThread: *mut CWelsTaskThread) -> WELS_THREAD_ERROR_CODE {
        let mut idle = self.m_cIdleThreads.lock().unwrap();
        idle.push_back(pThread as usize);
        WELS_THREAD_ERROR_OK
    }

    pub fn AddThreadToBusyList(&self, pThread: *mut CWelsTaskThread) -> WELS_THREAD_ERROR_CODE {
        let mut busy = self.m_cBusyThreads.lock().unwrap();
        busy.push_back(pThread as usize);
        WELS_THREAD_ERROR_OK
    }

    pub fn RemoveThreadFromBusyList(&self, pThread: *mut CWelsTaskThread) -> WELS_THREAD_ERROR_CODE {
        let mut busy = self.m_cBusyThreads.lock().unwrap();
        if busy.erase(pThread as usize) {
            WELS_THREAD_ERROR_OK
        } else {
            WELS_THREAD_ERROR_GENERAL
        }
    }

    pub fn AddTaskToWaitedList(&self, pTask: *mut (dyn IWelsTask + 'static)) -> bool {
        if pTask.is_null() {
            return false;
        }
        let mut waited = self.m_cWaitedTasks.lock().unwrap();
        waited.push_back(TaskPtr(pTask))
    }

    pub fn GetIdleThread(&self) -> *mut CWelsTaskThread {
        let mut idle = self.m_cIdleThreads.lock().unwrap();
        if idle.size() == 0 {
            return std::ptr::null_mut();
        }
        if let Some(addr) = idle.begin() {
            idle.pop_front();
            addr as *mut CWelsTaskThread
        } else {
            std::ptr::null_mut()
        }
    }

    pub fn GetWaitedTask(&self) -> Option<TaskPtr> {
        let mut waited = self.m_cWaitedTasks.lock().unwrap();
        if waited.size() == 0 {
            return None;
        }
        if let Some(task) = waited.begin() {
            waited.pop_front();
            Some(task)
        } else {
            None
        }
    }

    pub fn GetBusyThreadNum(&self) -> i32 {
        let busy = self.m_cBusyThreads.lock().unwrap();
        busy.size()
    }

    pub fn GetIdleThreadNum(&self) -> i32 {
        let idle = self.m_cIdleThreads.lock().unwrap();
        idle.size()
    }

    pub fn GetWaitedTaskNum(&self) -> i32 {
        let waited = self.m_cWaitedTasks.lock().unwrap();
        waited.size()
    }

    pub fn ClearWaitedTasks(&self) {
        let mut waited = self.m_cWaitedTasks.lock().unwrap();
        while waited.size() != 0 {
            if let Some(task) = waited.begin() {
                unsafe {
                    if !task.0.is_null() {
                        if let Some(sink) = (*task.0).GetSink() {
                            sink.OnTaskCancelled();
                        }
                    }
                }
                waited.pop_front();
            }
        }
    }

    pub fn GetThreadNum(&self) -> i32 {
        GLOBAL_MAX_THREAD_NUM.load(Ordering::SeqCst)
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    struct DummySink {
        executed: bool,
        cancelled: bool,
    }

    impl IWelsTaskSink for DummySink {
        fn OnTaskExecuted(&mut self) -> i32 {
            self.executed = true;
            0
        }
        fn OnTaskCancelled(&mut self) -> i32 {
            self.cancelled = true;
            0
        }
    }

    struct DummyTask {
        executed_count: i32,
        sink: Option<DummySink>,
    }

    impl IWelsTask for DummyTask {
        fn Execute(&mut self) -> i32 {
            self.executed_count += 1;
            0
        }

        fn GetSink(&mut self) -> Option<&mut (dyn IWelsTaskSink + 'static)> {
            self.sink.as_mut().map(|s| s as &mut dyn IWelsTaskSink)
        }
    }

    #[test]
    fn test_thread_pool_singleton_lifecycle() {
        let res = CWelsThreadPool::SetThreadNum(2);
        assert_eq!(res, WELS_THREAD_ERROR_OK);

        let pPool = CWelsThreadPool::AddReference();
        assert!(!pPool.is_null());
        assert!(CWelsThreadPool::IsReferenced());

        // Cannot change thread count when referenced
        assert_eq!(CWelsThreadPool::SetThreadNum(4), WELS_THREAD_ERROR_GENERAL);

        let mut task = DummyTask {
            executed_count: 0,
            sink: Some(DummySink {
                executed: false,
                cancelled: false,
            }),
        };

        unsafe {
            let pTask = &mut task as *mut dyn IWelsTask;
            let q_res = (*pPool).QueueTask(pTask);
            assert_eq!(q_res, WELS_THREAD_ERROR_OK);
        }

        // Wait a short moment for async task execution
        thread::sleep(Duration::from_millis(50));
        assert_eq!(task.executed_count, 1);
        assert!(task.sink.as_ref().unwrap().executed);

        CWelsThreadPool::RemoveInstance();
        assert!(!CWelsThreadPool::IsReferenced());
    }

    #[test]
    fn test_non_duplicated_list() {
        let mut list = CWelsNonDuplicatedList::<usize>::new();
        assert_eq!(list.size(), 0);
        assert!(list.push_back(100));
        assert_eq!(list.size(), 1);
        assert!(!list.push_back(100)); // duplicate rejected
        assert_eq!(list.size(), 1);
        assert!(list.push_back(200));
        assert_eq!(list.size(), 2);
        assert_eq!(list.begin(), Some(100));
        list.pop_front();
        assert_eq!(list.size(), 1);
        assert_eq!(list.begin(), Some(200));
    }
}
