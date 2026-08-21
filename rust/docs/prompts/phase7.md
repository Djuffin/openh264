# Phase 7 — the threading rework: fork/join over owned splits

The charter for the phase. Session briefs cite it; the standing rules stay in
`safety_refactor_plan.md` §7.6 and the phase's full inheritance list is plan §5
("Inherited from Phase 6", the Phase 7 block — every count re-verified against
the tree when written).

## 0. Starting position (Phase 6's close, `b08d6c47`)

- The encoder is structurally safe outside this phase's seam: 36 of 38 modules
  carry `#![deny(unsafe_code)]`; the two exceptions are this phase's files —
  `encoder/slice_multi_threading.rs` and `encoder/wels_task_management.rs`,
  exempted at their `pub mod` lines in `encoder/mod.rs`. **Both allows retire
  with this phase**, plus the tagged items it owns: **89 `port-raw(Phase 7)` +
  16 `MT`** (grep `unsafe-cat:` for the live list).
- Everything the threads touch is owned or an id: the slice banks are
  `Vec<SSlice>` per bank, the layer list is `Vec<Box<SDqLayer>>`, the frame
  bitstream is a `Vec<u8>` with retag-stable root accessors, slices are named
  by position (`SliceIdx`), pictures by handle.
- The last 4 live allocator call sites are this phase's:
  `DynamicSliceBs` (`encoder/encoder_ext.rs`, alloc/free) and `sSliceBs.pBs`
  (`encoder/svc_encode_slice.rs`, alloc/free). `pMemAlign` and
  `common/memory_align.rs` die when they do. (The other 8 hits are
  `SCREEN_CONTENT(dormant)`, Phase 10's.)
- `common/wels_thread_pool.rs` is still the only `--skip` in the Miri `--lib`
  gate (**F12**). This phase deletes the skip by deleting what it skips.
- **Anchors written before commit `e69a0984` are stale** — the deny sweep
  shifted every line number in both directories. Re-grep; never trust a
  `file.rs:NNN` from an older document.

## 1. The design (settled at the Phase 0 survey, §2.2.7 of the plan; re-verify
its two premises, then build)

**Premise 1 — disjointness is already index-based.** A task owns slot
`m_iThreadIdx` of parallel per-thread arrays, claimed by `QueryEmptyThread`
under a mutex (`wels_task_management.rs`). No two tasks share a slot.
**Premise 2 — assembly is already order-based.** Slices are stitched into the
frame bitstream in slice order (`AppendSliceToFrameBs` / `WriteSliceBs`),
which is why MT output is byte-deterministic today.

Target shape (from the plan, unchanged):

```rust
std::thread::scope(|s| {
    for job in jobs { s.spawn(|| encode_slice(job, shared)); }
}); // join == the barrier
```

- **Fixed slice modes**: partition the slice bank and the per-thread bs
  buffers with `split_at_mut`/`chunks_mut`, one job per slice — the current
  claiming logic minus the mutex.
- **`SM_SIZELIMITED` (dynamic)**: an `mpsc` work queue of slice indices,
  per-worker owned scratch, results stitched in slice order. **The claiming
  order must be preserved exactly** — it is index-driven today; keep it
  index-driven.
- The persistent pool is rebuilt safely **only if** the span shows spawn cost
  (workers + `mpsc::Sender<Box<dyn FnOnce + Send>>`); otherwise scoped
  threads suffice.
- Deleted outright: `TaskPtr` and its `Send/Sync` impls, the `usize`
  launderings, the `*mut c_void` mutexes (`WelsMutexInit` family), the
  `static mut` pool singleton (→ `OnceLock` if anything survives), all 5
  `unsafe impl` pairs, the dead C++ event-machinery fields in
  `SSliceThreading`, and the `CWelsList`/`CWelsNonDuplicatedList` C++-list
  ports if nothing safe needs them.
- Deblocking runs after the join, as today.

## 2. The referee

- **The exit gate is byte determinism**: every `(configuration, thread count)`
  the sweeps run must produce today's bytes. The instrument already exists —
  presets `st mt def sl ltr`, 369 rows per profile, `t ∈ {2,4}` in `mt`.
  Gate rhythm unchanged: `gates.sh commit` / `family` / one `exit` per
  session; Miri runs `MIRI_SCOPE=encoder` (D-gate-2 carried forward for
  encoder-side phases — steward; full unscoped battery at the phase exit).
- **F3 closes here by ablation, and the protocol is fixed in advance.**
  Current state: 87 measurements; signature `mt`, `sm=3`, `t ∈ {2,4}`, any
  wrong output length; fastest reproducer on record
  `mt CiscoVT2people_320x192_12fps sm=3 n=600 t=4 cabac=1` at **~1 in 11
  under load**. The ablation:
  1. **Before-arm first, on the untouched tree**: 500 runs of the fast
     configuration under the same load discipline session H used. Expected
     ~45 hits at the recorded rate. Record the count.
  2. The redesign lands (the flake lives on the `sm=3` path, so the verdict
     is only available after the `SM_SIZELIMITED` rework).
  3. **After-arm**: 500 runs, same discipline. **0 hits = ablated** (at the
     old rate, P(0/500) ≈ e^−45 — conclusive); **any hits = the defect is
     upstream of the deleted machinery** and now lives in a surface a
     fraction of the size — corner it there. Either exit closes F3.
- **F61** (bank growth never re-stamps the slice list — the C++ shares the
  shape): the redesign replaces the mechanism; verify by construction that
  the class cannot recur, and record F61 closed-by-ablation or fixed, with
  the C++ divergence note unchanged (byte-neutral both sides).

## 3. Session plan

- **A** — the before-arm F3 baseline; the fork/join skeleton on the fixed
  slice modes; the pool path deleted for those modes; first denies where
  files empty of unsafe. Drop-from-the-end boundary: the `SM_SIZELIMITED`
  rework.
- **B** — `SM_SIZELIMITED` on the work queue (claiming order preserved); the
  after-arm F3 verdict; `sSliceBs.pBs`/`DynamicSliceBs`/thread-bs ownership;
  the MT context members typed; `pMemAlign` + `memory_align.rs` retired; the
  F12 Miri skip deleted and the new threading run under Miri; both module
  denies land and the 105 tagged items retire; the phase close on the full
  unscoped battery.

Two sessions is the estimate. If A lands its drop boundary early it may take
B's head; if B overflows, C closes — the exit gate does not bend.

## 4. Non-goals

The decoder's threading stubs (F36: delete or leave — settle by reading in B,
one hour cap); any encoder behavior change; the parked perf families (Phase
9); `wels_encoder_ext.rs` (Phase 8); everything `SCREEN_CONTENT(dormant)`
(Phase 10).
