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

**Revised at session A's close, 2026-08-20, by F67** (`phase7_findings.md`). The
plan below the rule is what was written; what follows here is what the tree
supports.

### The decision session B opens with

**F67: the design has a third premise and it does not hold.** Premises 1 and 2
(§1) are about *scheduling* and both were verified. The target shape also needs
`shared` to cross the spawn, which is a property of the type, not the schedule:
`Scope::spawn` wants `Send`, a captured `&T` wants `T: Sync`, and `sWelsEncCtx`
is `!Sync` for **12 distinct reasons** (compiler output in the finding), five of
them nested inside types Phases 8 and 10 own. And the **105 tagged items this
phase was given to retire are, item for item, the slice-encode call tree's raw
signatures** — `svc_encode_slice` 40, `svc_mode_decision` 15,
`svc_enc_slice_segment` 11, `rc` 11, and **zero in the three seam files**. The
inventory and the blocker are the same object seen twice.

So the context split is a **precondition** of the fork/join, not a consequence of
it, and the two sessions were planned the other way round. Three orderings; **RESOLVED 2026-08-20 — D-mt-1 (plan §7.4) picks ordering 2**, and
[`phase7_session_b.md`](phase7_session_b.md) is its execution brief:

1. **Split first, fork later.** The design's own logic, honestly costed: several
   sessions, and it pulls work from Phases 8 and 10 forward into 7.
2. **One named `unsafe impl` at one seam.** Delete the pool, the `static mut`
   singleton, both C++-list ports, `CWelsTaskThread`, the claiming mutex and the
   condvar dance; keep a single documented `Send` job type at the spawn. Net
   **5 `unsafe impl` pairs → 1**, and F3's surface shrinks to what the ablation
   wants to measure. **The standing rule forbids this**, so it needs a ruling,
   not an inference.
3. **Defer the fork/join to Phase 9.** Phase 7 then does what is reachable
   without crossing the spawn seam: the dead machinery (done, T7.A2), the leak
   (done, T7.A3), the `SM_SIZELIMITED` work queue, and the F3 after-arm.

### Sessions as run

- **A** — done, 2026-08-20, `b08d6c47..` . The F3 before-arm at **3000 runs,
  25 hits, ≈1/120** — *not* the ~1/11 §2 carries forward, which was a 9-in-96
  sample; §2's "P(0/500) ≈ e⁻⁴⁵" arithmetic goes with it, and **B's after-arm
  should be 2000 runs, not 500** (e⁻¹⁶·⁷). Premises 1 and 2 verified in writing,
  with a bound on premise 1 nobody had written down: concurrency must be capped
  at the *allocated buffer* count, not at the slot count, or a fixed mode with
  more slices than threads hands out a slot with a null buffer behind it. The
  eight dead event fields deleted; a leak fixed that has been in the tree since
  the raw translation. **F67 opened, and it re-scopes the phase.** No fork/join:
  session A stopped at a green, whole-mode boundary rather than write the
  `unsafe impl` the rules forbid.
- **B** — done, 2026-08-20, `9a392cc9..7e038545`, six commits. The fork/join
  through the one D-mt-1 seam on every mode; **F3 CLOSED by F69** (a lock the
  raw translation dropped; three arms: 25/3000 → 20/2000 pool-deleted →
  0/2000 lock-restored — the pool acquitted, S45); 2,115 lines of machinery
  deleted, F12 closed with an MT probe; F68 fixed (`BENCH_SLICE_MODE`) and the
  first honest thread numbers read: the port scales to two threads and stops,
  on the *old* pool; F70 fixed; F71 sixteen accessors converted, residue named;
  F72 fenced pending ruling (**now D-mt-2: complete it**). Steps 6–7 handed
  to C at a green boundary.
- **C** — [`phase7_session_c.md`](phase7_session_c.md): the dropped-sync
  census (S44 at phase scale, 16 C++ lock sites classified), F72 completed
  (D-mt-2), the stragglers through `memory_align.rs`, F71's residue read
  before assigned, the last deny and the 89 re-tags, the span that answers
  the thread ceiling, the unscoped close.
- *(original B bullet, superseded:)* the ordering decision above, first. Then, whichever way it goes: the
  F3 after-arm (2000 runs, `f3_arm.sh` verbatim — density is not comparable
  otherwise, measurement 89); `sSliceBs.pBs`/`DynamicSliceBs`/thread-bs
  ownership; `pMemAlign` + `memory_align.rs`; the F12 Miri skip when the pool
  goes; F61 by construction or by ablation.

- **B** — done, 2026-08-20, `9a392cc9..`. Built D-mt-1's seam and the fork/join on
  **every** slice mode, deleted the machinery (2,115 lines; `unsafe_impl` 12 → 2),
  and **closed F3** — by a mutex the raw translation dropped, not by any of that.
  Steps 0-5 landed; **steps 6 and 7 hand to C** at a green boundary (hard rule 7).
  Findings: **F69** (F3's cause, fixed), **F68** (fixed), **F70** (fixed), **F71**
  (partly fixed, residue → Phase 9), **F72** (open, fenced). See §3.1.

**Two sessions was the estimate and it was wrong** — the same way Phase 6 session
I was wrong: a phase's own precondition gets measured only when a session tries
to build against it. **Three was also wrong, for a better reason**: session B spent
its second half on four defects the conversion *exposed*, none of which were on any
plan.

### 3.1 What session B found, and what session C inherits

**The ablation's verdict is the headline, and it is not the one the charter
expected.** §2 fixes the experiment in advance as a test of "the deleted
claiming/pool machinery". Session B ran **three arms with one variable each**
because a candidate cause appeared mid-phase:

| arm | tree | runs | hits | rate |
|---|---|---|---|---|
| before (m. 88) | untouched | 3000 | 25 | 1/120 |
| **A** (m. 91) | pool machinery deleted, lock still missing | 2000 | 20 | 1/100 |
| **B** (m. 92) | the lock restored | 2000 | **0** | — |

Arm A against the before-arm is **z = 0.61, p ≈ 0.54** — deleting the pool did not
move F3. Arm B is **P ≈ 2e-9**. **F3's cause is F69**, a `WelsMutexLock` the raw
translation dropped from around `++iSliceNumInFrame`. The machinery was worth
deleting for five other reasons and **was not the defect**; a single after-arm on a
tree carrying both changes would have credited it anyway. *One variable per arm is
why the answer is usable.*

**The method that found it, three times over.** F69, T7.A3's leak, and
`SSliceThreadPrivateData`'s zero readers all came from the same question, asked
while listing what to delete: **"which of these fields is dead?"** A field that
looks dead in the port and is live in the reference is a dropped statement by
definition — and no gate this project owns can see one.

**Steps 6 and 7 are C's**, unchanged as written, plus:

1. **F71's residue** — the accessor family is fixed; the shared *writes* from
   workers (`sLayerInfo.sNalHeaderExt`) need F67's context split and go to Phase 9.
   The MT Miri probe is `#[cfg_attr(miri, ignore)]` with F71 cited; **that attribute
   is step 7's business** — it retires when F71 does, and it is the only skip in the
   battery (F12's is gone with the module it named).
2. **F72** — the load-balancing path is fenced, not completed. Completing it is one
   call at `encoder_ext.cpp:4069`'s site; it can never be byte-gated, so it joins
   `CABA2_SVA_B` as the second expected-divergent class when someone does.
3. **The span is now meaningful** (F68 fixed) and says something C should read
   before touching the pool decision: **the port scales 1→2 threads and then stops**
   (1080p `sm=1 n=4`: C++ 1.65x/2.56x, Rust 1.65x/1.65x). That was the *old* pool's
   ceiling; whether `thread::scope` still has it is the first thing step 7's span
   should answer, and it is the input the charter's conditional pool rebuild wanted.

---

*Original plan, superseded above, kept as written:*

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
