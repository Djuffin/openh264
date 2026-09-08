#![allow(unsafe_code)]
// Copyright (c) 2013, Cisco Systems
// All rights reserved.
//
// The C++ counterpart is `codec/common/inc/WelsThreadPool.h` /
// `codec/common/src/WelsThreadPool.cpp`, as `wels_task_management.cpp` uses it:
// a set of threads created once, fed tasks from a queue, signalling an event when
// a frame's task count reaches zero. Nothing here is a line-by-line port of that;
// the shape that is kept is the one the encoder relies on — persistent threads,
// one wake-up and one completion signal per frame.

//! # The persistent worker pool
//!
//! The encoder's three slice forks (`EncodeFixedSlicesForked`,
//! `EncodeSizeLimitedSlicesForked`, `UpdateMbMapForked` in
//! `slice_multi_threading.rs`) used to open a `std::thread::scope` per frame,
//! which creates and destroys one OS thread per worker per frame: a 2 MiB stack
//! mapped and unmapped, a guard page protected, a thread created and joined,
//! roughly 30 µs of an 80 µs QVGA frame. [`WorkerPool`] keeps the threads alive
//! for the encoder's lifetime and gives the forks the same API — `pool.scope(|s|
//! { s.spawn(..) .. h.join() })` — so their borrow structure, which is the port's
//! whole multi-threading argument, is unchanged.
//!
//! # Shape
//!
//! * `N` worker threads, created by [`WorkerPool::new`], parked when idle.
//! * One shared FIFO of type-erased jobs behind a `Mutex`, with an atomic
//!   `pending` count beside it so an idle worker can poll for work without the
//!   lock.
//! * Per scope: an outstanding-job counter and the owning thread's handle, in an
//!   `Arc` so a worker can finish its bookkeeping after the scope has returned.
//! * Per job: a packet holding the result (`Ok(T)` or the panic payload) and a
//!   `done` flag.
//!
//! The calling thread **helps**: while it waits in [`JobHandle::join`] or at the
//! end of [`WorkerPool::scope`] it runs any job still in the queue itself, so a
//! frame never waits on a worker's wake-up latency for a job nobody has started,
//! and a pool of zero workers runs everything inline. A job is still a whole
//! worker's share of a frame — the pool decides which thread runs a job, never
//! how the slices are grouped into jobs.
//!
//! Idle threads spin for a bounded time before parking (`WORKER_SPIN`,
//! `CALLER_SPIN`), and only when the pool leaves a core free for the caller:
//! see [`spin_budget`]. Parking and waking are `std::thread::park`/`unpark`;
//! every other piece of synchronisation is `std::sync`.
//!
//! # The one `unsafe`, and why it is sound
//!
//! A job borrows the frame's stack (`'scope`), and a persistent thread is
//! `'static`, so the closure has to cross that boundary: it is boxed as
//! `Box<dyn FnOnce() + Send + 'scope>` and its lifetime is erased to `'static`
//! at the one `unsafe` site in this module (`Scope::spawn`). The argument is
//! `std::thread::scope`'s (`library/std/src/thread/scoped.rs`, whose
//! `Scope::spawn` goes through `Builder::spawn_unchecked` for the same reason):
//!
//! 1. **The scope does not return, and does not unwind past its own frame, until
//!    every job it spawned has completed.** `ScopeData::running` is incremented
//!    at spawn and decremented by the thread that ran the job; `scope` waits for
//!    it to reach zero before returning `f`'s result, and the wait is
//!    unconditional — `f` runs under `catch_unwind`, so a panic in the scope body
//!    also waits, then resumes unwinding. The wait's `Acquire` load against the
//!    worker's `Release` decrement is the happens-before edge that publishes
//!    everything the job wrote to whatever reads it after the scope; the
//!    reconstruction seam (`rec_view.rs`) relies on exactly that edge.
//! 2. **The decrement is the last thing a job does with anything borrowed.** The
//!    user's closure is consumed by its call, so its captures are dropped when it
//!    returns; the result `T` (which may itself borrow `'scope`) is stored in the
//!    packet and the running thread's reference to the packet is dropped before
//!    the decrement, so if the handle was already gone the `T` is dropped then
//!    too. After the decrement the thread touches only the `Arc<ScopeData>` it
//!    owns, which is heap, and its own queue.
//! 3. **`'scope` cannot shrink.** `Scope` is invariant in `'scope` (the
//!    `PhantomData<&'scope mut &'scope ()>` marker, as in `std`), and `spawn`
//!    demands `F: 'scope`, so a job cannot capture a local that dies before `f`
//!    returns — that is a compile error, the same one `std` gives.
//!
//! So the erased `'static` is never relied on: the closure is called, and its
//! captures dropped, strictly before the borrows it holds can end. Panics inside
//! a job are caught (`catch_unwind`), stored, and returned from `join` as `Err`,
//! which the forks map to `ENC_RETURN_UNEXPECTED`; a job that panicked and was
//! never joined makes `scope` panic at its end, as `std` does.
//!
//! # What is not claimed
//!
//! The pool's threads are owned by [`WorkerPool`] and joined in its `Drop`.
//! Nothing here claims `Sync` by hand: `WorkerPool` is `Sync` by its fields
//! (atomics, a mutex, thread handles), which is what lets `&sWelsEncCtx` — which
//! holds the pool through `pSliceThreading` — cross a spawn. `Scope` and
//! `JobHandle` are deliberately `!Sync`/`!Send`: a handle is joined on the thread
//! that owns the scope, because that is the thread a completing job unparks.
//!
//! # Invariants the encoder relies on
//!
//! * The pool is built by `RequestMtResource`, with `uiThreadBsBufferNum`
//!   workers, only when `iMultipleThreadIdc > 1`; single-threaded encoding never
//!   constructs one.
//! * It lives in `SSliceThreading`, which the context drops at
//!   `WelsUninitEncoderExt`, so the threads end with the encoder and none outlives
//!   it.
//! * A worker panic is reported through `join` and leaves the pool usable.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

/// A job as the queue holds it: type-erased, lifetime-erased (see the module
/// header for why the `'static` is honest).
type Job = ScopedJob<'static>;
type ScopedJob<'a> = Box<dyn FnOnce() + Send + 'a>;
/// A panic payload, as `std::thread::Result` carries it.
pub type Payload = Box<dyn Any + Send + 'static>;

/// How long an idle worker spins for the next job before parking.
///
/// Short on purpose, and measured rather than reasoned: on the QVGA bars row at
/// four threads (an M1, four performance and four efficiency cores, 8 s runs,
/// three per point) the bound has a sharp optimum — 0 µs 14.3k fps, 15 µs 13k,
/// **20 µs 19.2k**, 25 µs 17.3k, 30 µs 14.2k, 100 µs 13.7k, 200 µs 11.3k — and
/// the same at two threads above 20 µs. Four workers spinning through the
/// caller's serial work between forks are a fifth busy thread on four fast
/// cores, and the longer they spin the likelier the scheduler moves one of
/// the frame's threads onto a slow core, which the frame then waits for. At
/// larger pictures the workers park once per frame either way and the wake is
/// microseconds against milliseconds of work.
const WORKER_SPIN: Duration = Duration::from_micros(20);
/// How long the calling thread spins for the last job of a scope before
/// parking. The jobs of a frame are of similar size, so the last one lands
/// within microseconds of the caller's own; without this spin the caller parks
/// every frame and the QVGA row falls back to the per-frame-spawn rate (9.7k
/// fps). Measured as a plateau: 40 to 100 µs all within noise of 19k fps.
const CALLER_SPIN: Duration = Duration::from_micros(50);
/// Spin iterations between clock reads.
const SPIN_BATCH: u32 = 32;

/// The spin bounds a pool of `workers` threads may use on this machine: `None`
/// (park at once) unless the pool leaves at least one core for the calling
/// thread. A spinning thread is a busy one, and on a machine with no spare core
/// it would only delay the thread it is waiting for.
fn spin_budget(workers: usize) -> Option<(Duration, Duration)> {
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    (workers > 0 && workers < cores).then_some((WORKER_SPIN, CALLER_SPIN))
}

/// A parked worker's mailbox: its thread handle, registered by the worker itself
/// before it first parks, and the flag a producer claims to wake it.
struct WorkerSlot {
    thread: OnceLock<Thread>,
    /// `true` while the worker is parked or about to park. A producer that wants
    /// this worker swaps it to `false` and unparks; the worker leaving the park
    /// loop on its own clears it too.
    parked: AtomicBool,
}

/// What the pool's threads and its owner share.
struct Shared {
    queue: Mutex<VecDeque<Job>>,
    /// Jobs pushed and not yet popped. Maintained under `queue`'s lock; read
    /// without it by idle threads polling for work.
    pending: AtomicUsize,
    stop: AtomicBool,
    workers: Vec<WorkerSlot>,
    /// `(worker, caller)` spin bounds, or `None` to park immediately.
    spin: Option<(Duration, Duration)>,
    /// Worker threads that have not yet exited their loop. The `Drop` test reads
    /// it after the join.
    live: AtomicUsize,
}

impl Shared {
    fn push(&self, job: Job) {
        {
            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.push_back(job);
            self.pending.fetch_add(1, Ordering::SeqCst);
        }
        self.wake_one();
    }

    fn try_pop(&self) -> Option<Job> {
        if self.pending.load(Ordering::Acquire) == 0 {
            return None;
        }
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let job = q.pop_front();
        if job.is_some() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
        job
    }

    /// Wakes one parked worker, if there is one. A spinning worker needs no
    /// wake: it reads `pending`.
    ///
    /// The lost-wake-up argument: the producer increments `pending` and then
    /// reads `parked`; the worker sets `parked` and then reads `pending`; all
    /// four are `SeqCst`, so at least one side sees the other's write — either
    /// the worker skips the park or the producer claims and unparks it. An
    /// `unpark` before the `park` is not lost: the token is kept.
    fn wake_one(&self) {
        for slot in &self.workers {
            if slot.parked.load(Ordering::SeqCst)
                && slot
                    .parked
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                if let Some(t) = slot.thread.get() {
                    t.unpark();
                }
                return;
            }
        }
    }

    /// A worker's idle wait: spin within the budget, then park until a producer
    /// claims this slot, `stop` is raised, or work shows up.
    fn wait_for_work(&self, slot: &WorkerSlot) {
        let has_work = || self.pending.load(Ordering::SeqCst) > 0 || self.stop.load(Ordering::SeqCst);
        if let Some((bound, _)) = self.spin
            && spin_until(bound, has_work)
        {
            return;
        }
        slot.parked.store(true, Ordering::SeqCst);
        loop {
            if has_work() {
                slot.parked.store(false, Ordering::SeqCst);
                return;
            }
            thread::park();
            if !slot.parked.load(Ordering::SeqCst) {
                // Claimed by a producer (or by `Drop`); the wake carries no job
                // by itself, the loop above pops one.
                return;
            }
            // A spurious wake-up: still parked, check again.
        }
    }

    /// The calling thread's wait for `done`: spin within the budget, then park.
    /// Every completing job unparks the scope's owner, which is this thread.
    fn wait_caller(&self, done: impl Fn() -> bool) {
        if let Some((_, bound)) = self.spin
            && spin_until(bound, &done)
        {
            return;
        }
        while !done() {
            thread::park();
        }
    }

    fn worker_main(&self, slot: &WorkerSlot) {
        // Registration before the first park. Its readers (`wake_one`, `Drop`)
        // reach it only through an RMW on `parked` that read this thread's own
        // later write of it, which is the edge that makes the handle visible.
        let _ = slot.thread.set(thread::current());
        loop {
            if let Some(job) = self.try_pop() {
                // The job is a `Scope::spawn` wrapper: it catches its own panics
                // and does all the scope bookkeeping, so nothing here can unwind.
                job();
                continue;
            }
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            self.wait_for_work(slot);
        }
        self.live.fetch_sub(1, Ordering::Release);
    }
}

/// Spins until `cond` holds or `bound` elapses; `true` if it was the condition.
fn spin_until(bound: Duration, cond: impl Fn() -> bool) -> bool {
    let t0 = Instant::now();
    loop {
        for _ in 0..SPIN_BATCH {
            if cond() {
                return true;
            }
            std::hint::spin_loop();
        }
        if t0.elapsed() >= bound {
            return cond();
        }
    }
}

/// `N` persistent worker threads and the queue that feeds them. See the module
/// header.
pub struct WorkerPool {
    shared: Arc<Shared>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl WorkerPool {
    /// A pool of `workers` threads. `0` is valid: every job then runs on the
    /// calling thread inside `scope`.
    ///
    /// # Panics
    /// If the OS refuses to create a thread; [`WorkerPool::try_new`] reports that
    /// instead.
    pub fn new(workers: usize) -> Self {
        Self::try_new(workers).expect("failed to spawn a worker thread")
    }

    /// [`WorkerPool::new`], returning the OS error of a failed thread creation.
    /// The threads already created are joined before the error is returned.
    pub fn try_new(workers: usize) -> std::io::Result<Self> {
        Self::try_new_with_spin(workers, spin_budget(workers))
    }

    /// [`WorkerPool::new`] with an explicit `(worker, caller)` spin budget in
    /// place of [`spin_budget`]'s — the latency micro-test's instrument.
    #[cfg(test)]
    fn with_spin(workers: usize, spin: Option<(Duration, Duration)>) -> Self {
        Self::try_new_with_spin(workers, spin).expect("failed to spawn a worker thread")
    }

    fn try_new_with_spin(workers: usize, spin: Option<(Duration, Duration)>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::with_capacity(workers.max(1) * 2)),
            pending: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            workers: (0..workers)
                .map(|_| WorkerSlot { thread: OnceLock::new(), parked: AtomicBool::new(false) })
                .collect(),
            spin,
            live: AtomicUsize::new(workers),
        });
        let mut pool = Self { shared, threads: Vec::with_capacity(workers) };
        for k in 0..workers {
            let shared = Arc::clone(&pool.shared);
            let spawned = thread::Builder::new()
                .name(format!("openh264-worker-{k}"))
                .spawn(move || shared.worker_main(&shared.workers[k]));
            match spawned {
                Ok(h) => pool.threads.push(h),
                Err(e) => {
                    // The slots past `k` never got a thread; `live` counts only
                    // the ones that did, so the drop's join is exact.
                    pool.shared.live.fetch_sub(workers - k, Ordering::Release);
                    drop(pool);
                    return Err(e);
                }
            }
        }
        Ok(pool)
    }

    /// How many worker threads the pool owns.
    pub fn workers(&self) -> usize {
        self.threads.len()
    }

    /// Runs `f` with a scope; every job spawned on it has finished when this
    /// returns, whether `f` returned or panicked.
    ///
    /// The signature is `std::thread::scope`'s: jobs may borrow anything that
    /// outlives `'env`, and `'scope` is invariant so it cannot be shrunk to
    /// outlive a local of `f`. Scopes may nest — `f` may call `scope` again on
    /// the same pool, and so may a job — because the calling thread helps with
    /// queued jobs while it waits, so an inner scope never waits on a worker that
    /// is itself waiting.
    ///
    /// # Panics
    /// Resumes a panic of `f` after waiting for the jobs; panics with
    /// "a job spawned on the worker pool panicked" if a job panicked and its
    /// handle was never joined, as `std::thread::scope` does.
    #[track_caller]
    pub fn scope<'env, F, R>(&'env self, f: F) -> R
    where
        F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> R,
    {
        let scope = Scope {
            shared: &self.shared,
            data: Arc::new(ScopeData {
                running: AtomicUsize::new(0),
                unhandled_panics: AtomicUsize::new(0),
                owner: thread::current(),
            }),
            scope: PhantomData,
            env: PhantomData,
            not_sync: PhantomData,
        };

        // `f` under `catch_unwind` so that the wait below runs on the panic path
        // too: the frame's locals die when this function unwinds, and a job may
        // still be borrowing them.
        let result = catch_unwind(AssertUnwindSafe(|| f(&scope)));

        // The wait, on both paths. This is what makes the erased `'static` in
        // `spawn` honest (module header, point 1).
        scope.wait_all();

        match result {
            Err(e) => resume_unwind(e),
            Ok(_) if scope.data.unhandled_panics.load(Ordering::Relaxed) != 0 => {
                panic!("a job spawned on the worker pool panicked")
            }
            Ok(r) => r,
        }
    }

    /// The shared block, for the tests that watch the threads exit.
    #[cfg(test)]
    fn shared_for_tests(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }
}

impl Drop for WorkerPool {
    /// Raises `stop`, wakes every worker and joins them. No scope can be live
    /// here — `scope` borrows the pool for its whole duration — so the queue is
    /// empty and each worker exits at the top of its loop.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for slot in &self.shared.workers {
            // A `swap`, not a `store`: the read half of the RMW is what
            // synchronises with the worker's own write of `parked`, and through
            // it with the registration of `thread` before that write. A plain
            // store has no read, so the handle could legitimately read as unset
            // here and the worker would stay parked — Miri's weak-memory
            // emulation showed exactly that. A worker that has not parked yet
            // reads `stop` (`SeqCst`, stored above) before it would.
            let was_parked = slot.parked.swap(false, Ordering::SeqCst);
            if let Some(t) = slot.thread.get() {
                t.unpark();
            } else {
                debug_assert!(!was_parked, "a parked worker has registered its thread");
            }
        }
        for h in self.threads.drain(..) {
            // A worker cannot panic out of its loop (jobs catch their own), so
            // `Err` here would be a bug in this module, not something to act on
            // during a drop.
            let _ = h.join();
        }
    }
}

impl fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerPool")
            .field("workers", &self.threads.len())
            .field("pending", &self.shared.pending.load(Ordering::Relaxed))
            .field("spin", &self.shared.spin)
            .finish()
    }
}

/// One scope's bookkeeping, `Arc`-owned so a job can finish its decrement after
/// `scope` has returned.
struct ScopeData {
    /// Jobs spawned and not yet completed.
    running: AtomicUsize,
    /// Jobs that panicked and whose `Err` no `join` has taken yet.
    unhandled_panics: AtomicUsize,
    /// The thread that called `scope`, which every completion unparks.
    owner: Thread,
}

/// A scope to spawn jobs in. See [`WorkerPool::scope`].
pub struct Scope<'scope, 'env: 'scope> {
    shared: &'env Arc<Shared>,
    data: Arc<ScopeData>,
    /// Invariance over `'scope` — the `std` marker, for the `std` reason: without
    /// it a job could spawn a job borrowing one of its own locals.
    scope: PhantomData<&'scope mut &'scope ()>,
    env: PhantomData<&'env mut &'env ()>,
    /// `!Sync` (and `!Send`): jobs are spawned and joined on the owning thread,
    /// the one a completing job unparks.
    not_sync: PhantomData<*mut ()>,
}

impl<'scope, 'env> Scope<'scope, 'env> {
    /// Hands the job to the pool and returns a handle for its result. The job
    /// runs on the first idle worker, or on the calling thread while it waits
    /// in [`JobHandle::join`] or at the end of the scope, whichever comes first.
    /// The job may borrow anything `'env` outlives.
    pub fn spawn<F, T>(&'scope self, f: F) -> JobHandle<'scope, T>
    where
        F: FnOnce() -> T + Send + 'scope,
        T: Send + 'scope,
    {
        let packet = Arc::new(Packet { result: Mutex::new(None), done: AtomicBool::new(false) });
        let their_packet = Arc::clone(&packet);
        let data = Arc::clone(&self.data);
        self.data.running.fetch_add(1, Ordering::SeqCst);

        let wrapper = move || {
            // `f` is consumed by the call: its captures are dropped by the time
            // this returns, panic or not.
            let result = catch_unwind(AssertUnwindSafe(f));
            if result.is_err() {
                // Before `done`, so a `join` that observes the `Err` always finds
                // this count already raised.
                data.unhandled_panics.fetch_add(1, Ordering::Relaxed);
            }
            *their_packet.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            their_packet.done.store(true, Ordering::Release);
            // The result may borrow `'scope`; if the handle is already gone this
            // drops it, and it happens before the decrement (module header,
            // point 2).
            drop(their_packet);
            data.running.fetch_sub(1, Ordering::Release);
            data.owner.unpark();
            // `data` is an `Arc` — heap, not the frame — and is dropped here.
        };

        let boxed: ScopedJob<'scope> = Box::new(wrapper);
        // SAFETY: lifetime erasure only — the same trait object, `'scope` read
        // as `'static`; the fat pointer's layout is identical. It is sound
        // because the closure is called, and every capture and result dropped,
        // before `'scope` can end: `scope` waits for `ScopeData::running` to
        // reach zero on both its return and its unwind path, and the wrapper
        // above decrements only after `f` has been consumed and its result
        // handed over. See the module header.
        let job: Job = unsafe { std::mem::transmute::<ScopedJob<'scope>, Job>(boxed) };
        self.shared.push(job);

        JobHandle { packet, shared: self.shared, data: &self.data, not_send: PhantomData }
    }

    /// Waits until every job of this scope has completed, running queued ones on
    /// this thread meanwhile.
    fn wait_all(&self) {
        let done = || self.data.running.load(Ordering::Acquire) == 0;
        loop {
            if done() {
                return;
            }
            if let Some(job) = self.shared.try_pop() {
                job();
                continue;
            }
            self.shared.wait_caller(done);
        }
    }
}

impl fmt::Debug for Scope<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scope")
            .field("running", &self.data.running.load(Ordering::Relaxed))
            .field("unhandled_panics", &self.data.unhandled_panics.load(Ordering::Relaxed))
            .finish()
    }
}

/// A job's result slot.
struct Packet<T> {
    result: Mutex<Option<Result<T, Payload>>>,
    /// Set, with `Release`, after the result is stored.
    done: AtomicBool,
}

/// An owned permission to wait for one job; see [`Scope::spawn`].
pub struct JobHandle<'scope, T> {
    packet: Arc<Packet<T>>,
    shared: &'scope Arc<Shared>,
    data: &'scope ScopeData,
    /// `!Send`: joined on the scope's owning thread.
    not_send: PhantomData<*mut ()>,
}

impl<T> JobHandle<'_, T> {
    /// Waits for this job; `Err` carries the panic payload if it panicked, like
    /// `ScopedJoinHandle::join`. While the job is still queued, or another job
    /// is, this thread runs them itself rather than waiting.
    pub fn join(self) -> Result<T, Payload> {
        let done = || self.packet.done.load(Ordering::Acquire);
        loop {
            if done() {
                break;
            }
            if let Some(job) = self.shared.try_pop() {
                job();
                continue;
            }
            self.shared.wait_caller(done);
        }
        let result = self
            .packet
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("a finished job has a result, and a handle joins once");
        if result.is_err() {
            self.data.unhandled_panics.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }

    /// Whether the job has completed (not blocking).
    pub fn is_finished(&self) -> bool {
        self.packet.done.load(Ordering::Acquire)
    }
}

impl<T> fmt::Debug for JobHandle<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobHandle").field("done", &self.is_finished()).finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Barrier;

    /// Jobs borrow the frame's locals — a `Vec` and a counter — and their writes
    /// are visible after the scope. Repeated so that under Miri the drop of the
    /// locals after each scope is checked against a job that might still hold
    /// them.
    #[test]
    fn jobs_borrow_locals_and_publish_their_writes() {
        let pool = WorkerPool::new(3);
        for round in 0..if cfg!(miri) { 4 } else { 200 } {
            let input: Vec<u32> = (0..16).map(|i| i + round).collect();
            let mut output = vec![0u32; 16];
            let hits = AtomicU32::new(0);
            pool.scope(|s| {
                let mut handles = Vec::new();
                for (k, out) in output.chunks_mut(4).enumerate() {
                    let input = &input;
                    let hits = &hits;
                    handles.push(s.spawn(move || {
                        for (j, o) in out.iter_mut().enumerate() {
                            *o = input[k * 4 + j] * 2;
                        }
                        hits.fetch_add(1, Ordering::Relaxed);
                        k
                    }));
                }
                for (k, h) in handles.into_iter().enumerate() {
                    assert_eq!(h.join().unwrap(), k);
                }
            });
            assert_eq!(hits.load(Ordering::Relaxed), 4);
            assert!(output.iter().zip(&input).all(|(o, i)| *o == i * 2));
        }
    }

    /// A job's result may itself borrow `'scope`.
    #[test]
    fn results_come_back_through_join_and_may_borrow_the_scope() {
        let pool = WorkerPool::new(2);
        let text = String::from("hello, worker");
        let (a, b) = pool.scope(|s| {
            let ha = s.spawn(|| &text[..5]);
            let hb = s.spawn(|| text.len());
            (ha.join().unwrap(), hb.join().unwrap())
        });
        assert_eq!(a, "hello");
        assert_eq!(b, 13);
    }

    /// A panicking job is an `Err` from `join` carrying the payload, and the pool
    /// runs the next scope as if nothing happened.
    #[test]
    fn a_panicking_job_is_an_err_and_does_not_poison_the_pool() {
        let pool = WorkerPool::new(2);
        let r = pool.scope(|s| {
            let bad = s.spawn(|| -> u32 { panic!("slice {} failed", 7) });
            let good = s.spawn(|| 42u32);
            (bad.join(), good.join())
        });
        let payload = r.0.expect_err("the panic must come back as Err");
        // A formatted panic's payload is a `String`, unless the compiler folded
        // the literal arguments and it is a `&str`; either way the text is there.
        let text = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        assert_eq!(text, Some("slice 7 failed"));
        assert_eq!(r.1.unwrap(), 42);
        // The pool is intact: same workers, another scope, all jobs run.
        let n = AtomicU32::new(0);
        pool.scope(|s| {
            for _ in 0..6 {
                s.spawn(|| n.fetch_add(1, Ordering::Relaxed));
            }
        });
        assert_eq!(n.load(Ordering::Relaxed), 6);
    }

    /// A job that panicked and was never joined makes the scope panic at its end
    /// — `std::thread::scope`'s rule — and the pool is still usable after.
    #[test]
    fn an_unjoined_panic_surfaces_at_the_end_of_the_scope() {
        let pool = WorkerPool::new(1);
        let r = catch_unwind(AssertUnwindSafe(|| {
            pool.scope(|s| {
                s.spawn(|| panic!("dropped on the floor"));
            })
        }));
        let payload = r.expect_err("the scope must panic");
        assert_eq!(
            payload.downcast_ref::<&str>().copied(),
            Some("a job spawned on the worker pool panicked")
        );
        assert_eq!(pool.scope(|s| s.spawn(|| 5).join().unwrap()), 5);
    }

    /// A panic in the scope body with a job in flight: the scope must not unwind
    /// until that job has finished, because the job borrows the body's frame.
    #[test]
    fn a_panic_in_the_scope_body_still_waits_for_the_jobs() {
        let pool = WorkerPool::new(2);
        let finished = AtomicBool::new(false);
        let gate = Barrier::new(2);
        let r = catch_unwind(AssertUnwindSafe(|| {
            pool.scope(|s| {
                s.spawn(|| {
                    gate.wait();
                    // Long enough that an unwinding scope which did not wait would
                    // have returned by now.
                    thread::sleep(Duration::from_millis(if cfg!(miri) { 5 } else { 50 }));
                    finished.store(true, Ordering::Release);
                });
                gate.wait();
                panic!("body failed with a job running");
            });
        }));
        assert!(r.is_err());
        assert!(finished.load(Ordering::Acquire), "scope unwound before its job completed");
    }

    /// A job's result with a destructor that touches the frame: when the handle
    /// is dropped without a join, the worker drops the result before its
    /// decrement, so by the end of the scope the borrow is gone. Under Miri the
    /// assertion is the retag of `dropped` inside `Drop`.
    #[test]
    fn an_unjoined_result_is_dropped_before_the_scope_ends() {
        struct Tally<'a>(&'a AtomicU32);
        impl Drop for Tally<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }
        let pool = WorkerPool::new(2);
        let dropped = AtomicU32::new(0);
        pool.scope(|s| {
            for _ in 0..3 {
                let h = s.spawn(|| Tally(&dropped));
                drop(h);
            }
        });
        assert_eq!(dropped.load(Ordering::Acquire), 3);
    }

    /// Scopes nest on the same thread: the inner scope's jobs run while the
    /// outer scope's are outstanding, and both complete.
    #[test]
    fn scopes_nest_from_the_same_thread() {
        let pool = WorkerPool::new(2);
        let outer_hits = AtomicU32::new(0);
        let inner_sum = pool.scope(|s| {
            for _ in 0..3 {
                s.spawn(|| outer_hits.fetch_add(1, Ordering::Relaxed));
            }
            pool.scope(|inner| {
                let hs: Vec<_> = (0..4u32).map(|k| inner.spawn(move || k * k)).collect();
                hs.into_iter().map(|h| h.join().unwrap()).sum::<u32>()
            })
        });
        assert_eq!(inner_sum, 0 + 1 + 4 + 9);
        assert_eq!(outer_hits.load(Ordering::Relaxed), 3);
    }

    /// A job may itself open a scope on the pool (the pool is `Sync`): with one
    /// worker, that worker owns the inner scope and helps with its jobs, so this
    /// cannot deadlock.
    #[test]
    fn a_job_may_open_a_nested_scope_on_the_pool() {
        let pool = WorkerPool::new(1);
        let total = pool.scope(|s| {
            let pool = &pool;
            let h = s.spawn(move || {
                pool.scope(|inner| {
                    let a = inner.spawn(|| 20);
                    let b = inner.spawn(|| 22);
                    a.join().unwrap() + b.join().unwrap()
                })
            });
            h.join().unwrap()
        });
        assert_eq!(total, 42);
    }

    /// The calling thread helps: with one worker blocked in job A until job B has
    /// run, `join(B)` runs B on the caller. Without helping this test hangs.
    #[test]
    fn the_caller_runs_queued_jobs_while_it_waits() {
        let pool = WorkerPool::new(1);
        let b_done = AtomicBool::new(false);
        let a_started = Barrier::new(2);
        pool.scope(|s| {
            let a = s.spawn(|| {
                a_started.wait();
                while !b_done.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                "a"
            });
            a_started.wait();
            let b = s.spawn(|| {
                b_done.store(true, Ordering::Release);
                thread::current().name().map(str::to_owned)
            });
            let ran_on = b.join().unwrap();
            assert_eq!(a.join().unwrap(), "a");
            assert_ne!(
                ran_on.as_deref(),
                Some("openh264-worker-0"),
                "the only worker was busy in A, so B must have run on the caller"
            );
        });
    }

    /// More jobs than workers, and their results in spawn order.
    #[test]
    fn more_jobs_than_workers() {
        let pool = WorkerPool::new(2);
        let n = if cfg!(miri) { 12 } else { 200 };
        let results: Vec<usize> = pool.scope(|s| {
            let hs: Vec<_> = (0..n).map(|k| s.spawn(move || k * 3)).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(results, (0..n).map(|k| k * 3).collect::<Vec<_>>());
    }

    /// A scope with no jobs returns at once with `f`'s value.
    #[test]
    fn zero_jobs() {
        let pool = WorkerPool::new(2);
        assert_eq!(pool.scope(|_| 17), 17);
    }

    /// A pool with no workers runs everything on the calling thread.
    #[test]
    fn zero_workers_run_inline() {
        let pool = WorkerPool::new(0);
        assert_eq!(pool.workers(), 0);
        let me = thread::current().id();
        let ids = pool.scope(|s| {
            let hs: Vec<_> = (0..3).map(|_| s.spawn(|| thread::current().id())).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
        });
        assert!(ids.iter().all(|id| *id == me));
    }

    /// Jobs must not be re-run or lost across many small scopes — the encoder's
    /// shape, one scope per frame.
    #[test]
    fn many_frames_of_four_jobs() {
        let pool = WorkerPool::new(4);
        let frames = if cfg!(miri) { 6 } else { 2000 };
        let mut acc = 0u64;
        for frame in 0..frames as u64 {
            let bufs: Vec<Vec<u64>> = (0..4).map(|k| vec![frame + k; 8]).collect();
            let sum: u64 = pool.scope(|s| {
                let hs: Vec<_> = bufs.iter().map(|b| s.spawn(move || b.iter().sum::<u64>())).collect();
                hs.into_iter().map(|h| h.join().unwrap()).sum()
            });
            assert_eq!(sum, (0..4).map(|k| (frame + k) * 8).sum::<u64>());
            acc += sum;
        }
        assert!(acc > 0);
    }

    /// `Drop` joins the threads: the counter each worker decrements on exit reads
    /// zero afterwards, and it is read through the shared block that outlives the
    /// pool.
    #[test]
    fn dropping_the_pool_ends_its_threads() {
        let pool = WorkerPool::new(3);
        let shared = pool.shared_for_tests();
        assert_eq!(shared.live.load(Ordering::Acquire), 3);
        // Park the workers first, so the drop has to wake them.
        pool.scope(|s| {
            s.spawn(|| 1).join().unwrap();
        });
        thread::sleep(Duration::from_millis(if cfg!(miri) { 1 } else { 5 }));
        drop(pool);
        assert_eq!(shared.live.load(Ordering::Acquire), 0);
        assert!(shared.stop.load(Ordering::Acquire));
        assert_eq!(shared.pending.load(Ordering::Acquire), 0);
    }

    /// The pool is `Sync` (the context that holds it crosses the spawn as
    /// `&sWelsEncCtx`), and `Send`.
    #[test]
    fn the_pool_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkerPool>();
    }

    /// The scope micro-timing: `scope` with N trivial jobs, against
    /// `std::thread::scope` with N trivial spawns in the same process. A
    /// measurement, not a check, so it runs only when asked — on an idle machine:
    /// `POOL_LATENCY=1 cargo test --release --lib worker_pool::tests::latency -- --nocapture`.
    /// (Not `#[ignore]`: the gate battery pins the crate's ignored-test count.)
    #[test]
    fn latency_micro_timing() {
        if std::env::var_os("POOL_LATENCY").is_none() {
            return;
        }
        fn stats(mut v: Vec<u64>) -> String {
            v.sort_unstable();
            let n = v.len();
            let mean = v.iter().sum::<u64>() as f64 / n as f64;
            format!(
                "min {:.1} us, median {:.1} us, p90 {:.1} us, mean {:.1} us",
                v[0] as f64 / 1e3,
                v[n / 2] as f64 / 1e3,
                v[n * 9 / 10] as f64 / 1e3,
                mean / 1e3
            )
        }
        let iters = 3000;
        let data = vec![1u32; 64];
        // One frame's worth of trivial jobs on `pool`, timed; `gap` is caller work
        // between two frames, where a worker's wake-up (or spin bound) shows.
        let frame = |pool: &WorkerPool, workers: usize, gap: Duration| -> u64 {
            let t0 = Instant::now();
            while t0.elapsed() < gap {
                std::hint::spin_loop();
            }
            let t = Instant::now();
            pool.scope(|s| {
                let hs: Vec<_> = (0..workers)
                    .map(|k| {
                        let d = &data;
                        s.spawn(move || std::hint::black_box(d[k]) + k as u32)
                    })
                    .collect();
                for h in hs {
                    std::hint::black_box(h.join().unwrap());
                }
            });
            t.elapsed().as_nanos() as u64
        };
        let variants: [(&str, Option<(Duration, Duration)>); 3] = [
            ("park only", None),
            ("default spin", spin_budget(4)),
            ("spin 100/50 us", Some((Duration::from_micros(100), Duration::from_micros(50)))),
        ];
        for &workers in &[2usize, 4, 8] {
            for (name, spin) in &variants {
                let pool = WorkerPool::with_spin(workers, *spin);
                for _ in 0..200 {
                    frame(&pool, workers, Duration::ZERO);
                }
                for gap_us in [0u64, 60, 300] {
                    let gap = Duration::from_micros(gap_us);
                    let samples: Vec<u64> = (0..iters).map(|_| frame(&pool, workers, gap)).collect();
                    println!(
                        "WorkerPool({workers}, {name}) x{workers} trivial jobs, gap {gap_us:>3} us, {iters} iters: {}",
                        stats(samples)
                    );
                }
            }
            let mut samples = Vec::with_capacity(iters);
            for _ in 0..iters {
                let t = Instant::now();
                thread::scope(|s| {
                    let hs: Vec<_> = (0..workers)
                        .map(|k| {
                            let d = &data;
                            s.spawn(move || std::hint::black_box(d[k]) + k as u32)
                        })
                        .collect();
                    for h in hs {
                        std::hint::black_box(h.join().unwrap());
                    }
                });
                samples.push(t.elapsed().as_nanos() as u64);
            }
            println!("std::thread::scope x{workers} trivial spawns, {iters} iters: {}", stats(samples));
        }
    }
}
