# Phase 7, session A — the F3 baseline, and fork/join lands on the fixed slice modes

You are executing the first session of Phase 7 (charter:
`rust/docs/prompts/phase7.md`). Work through the steps in order. Commit per
unit of work. Run the gates exactly as stated. Report at the end in the format
given in the last section.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the behavioral reference; byte parity is gate-enforced.
- Start: commit `b08d6c47`, tree clean, Phase 6 closed (36/38 modules denied;
  the two exceptions are this phase's files). The threads' data is all owned:
  slice banks `Vec<SSlice>`, layers `Vec<Box<SDqLayer>>`, frame bitstream
  `Vec<u8>` behind retag-stable accessors, slices named by `SliceIdx`.
- This session: (1) the F3 before-arm baseline on the untouched tree, (2) the
  scoped fork/join skeleton replacing the task/pool dispatch on the **fixed**
  slice modes, (3) as much pool deletion as those modes strand. The
  `SM_SIZELIMITED` rework and the F3 verdict are session B's.
- **Anchors older than commit `e69a0984` are stale everywhere** — the deny
  sweep moved every line number. Re-grep anything you read in a doc.
- Docs to update at close: `rust/docs/safety_refactor_log.md` (session entry),
  `rust/docs/safety_refactor_plan.md` §0 (row), `rust/docs/prompts/phase7.md`
  §3 (session row), `rust/docs/perf_baseline.md` (span),
  `rust/docs/phase0_findings.md` (F3 entries — the baseline arm goes here).
- Commits: `refactor(T7.A<n>): …` / `gate(T7.A<n>): …` / `docs(T7.A-close): …`.

## Hard rules

1. **Gates.** `bash rust/tools/gates.sh commit` in every commit; `family` at
   each step's end (sweeps **369/369 both profiles** — the determinism gate IS
   the byte gate: every `(config, t)` must produce today's bytes);
   `MIRI_SCOPE=encoder gates.sh exit` at the close.
2. **Miri.** Scoped (`MIRI_SCOPE=encoder`, 252+ tests ≈600 s), once per step
   end at most, always at the close. Note: `--skip wels_thread_pool` is still
   in the gate (F12); it is deleted in session B when the pool is.
3. **Perf.** One span at the close: `perfpair.py build` at `b08d6c47` and
   HEAD, `run … --pairs 7`, `null … --pairs 7`. On a median breach: bisect
   commits before acting. **Spawn cost is the one expected mechanism** — if
   the span shows it, the charter pre-authorizes the safe persistent pool as
   the remedy (session B), not a revert. Cumulative encode ≈ +15…+17%,
   tripwire +25% median.
4. **Counts are anchors** (2026-08-20, `b08d6c47`). Re-grep before acting;
   trust the grep and say so.
5. **The F3 fingerprint**: `mt` preset, `sm=3`, `t=2` or `t=4`, **any wrong
   output length** (empty, short, or long). One sweep hit with the fingerprint
   → re-run that configuration 5×; all byte-identical → record the measurement
   in `phase0_findings.md`, continue. Anything else: stop, it is yours. A
   retry loop under load is not a quiet run; zsh does not word-split
   `set -- $cfg`.
6. **No behavior change.** No encoded byte moves, in any mode, at any thread
   count.
7. **Threading discipline for everything you write**: shared state crosses
   `thread::scope` as `&`/`&mut` splits or through channels — no raw pointer
   crosses a spawn, no `unsafe impl Send/Sync`, no `static mut`. If a design
   corner seems to need one, stop and re-read the charter's target shape; the
   survey says it does not.
8. **Overflow**: the drop boundary is the whole `SM_SIZELIMITED` path — do
   not start it unless it can finish. Stop at a green, whole-mode boundary.

## Current state — verified facts

- **The dispatch today**: `WelsEncoderEncodeExt` (encoder_ext.rs) decides
  MT per layer; `slice_multi_threading.rs` holds `RequestMtResource` /
  `ReleaseMtResource` (the MT allocations), `AppendSliceToFrameBs` /
  `WriteSliceBs` (order-based stitching — **the determinism carrier, do not
  restructure**), `DynamicAdjustSlicing` + `AdjustBaseLayer` /
  `AdjustEnhanceLayer` (load balancing for fixed modes), and
  `UpdateMbListNeighborParallel`.
- **The tasks**: `wels_task_management.rs` — one `CWelsBaseTask` with a
  discriminant wrapped by four types (`UpdateMbMap`, `SliceEncoding`,
  `LoadBalancing`, `ConstrainedSizeSlicing`); `InitTask` claims a bs-buffer
  slot via `QueryEmptyThread(pThreadBsBufferUsage)` under a mutex;
  `ExecuteTask` / `ExecuteTaskConstrainedSize` are the bodies; `FinishTask`
  releases. The `ConstrainedSize` variant is the `SM_SIZELIMITED` path —
  **session B's, untouched here**.
- **The pool**: `common/wels_thread_pool.rs` — `TaskPtr(*mut dyn IWelsTask)`
  with 2 of the 5 `unsafe impl` pairs, `CWelsTaskThread`, two C++-list ports
  (`CWelsList`, `CWelsNonDuplicatedList`), one `static mut` singleton.
- **`SSliceThreading`** (slice_multi_threading.rs): per-thread event arrays
  (`pSliceCodedEvent`, `pReadySliceCodingEvent`, …), `pThreadBsBuffer[..]` +
  `bThreadBsBufferUsage[..]` + two `*mut c_void` mutexes. The Phase-0 survey
  found the C++ event machinery dead in the port — **verify that first** (who
  signals each event array; if nothing does, they delete with the redesign).
- **The single-threaded inline fallback already exists** — the encoder runs
  every mode single-threaded when `iMultipleThreadIdc <= 1`; the sweeps' `st`
  preset exercises it. The fork/join replaces only the `> 1` dispatch.
- The tagged inventory this phase retires: 89 `port-raw(Phase 7)` + 16 `MT`
  items (`grep -rn 'unsafe-cat: port-raw(Phase 7)\|unsafe-cat: MT' …`).

## Step 0 — the F3 before-arm, on the untouched tree

Before any edit: 500 runs of
`mt CiscoVT2people_320x192_12fps sm=3 n=600 t=4 cabac=1` under load (session
H's discipline: a parallel compile or equivalent saturating the cores;
measurements 77–78 recorded that an idle loop reproduces nothing). Expected
~45 hits at the recorded ~1/11. Record: the count, the load method, the
wall time, each hit's length signature, as the ablation's before-arm in
`phase0_findings.md`.

Accept: the arm recorded; tree still clean (no code commits yet).

## Step 1 — verify the two premises, in writing

1. **Slot disjointness**: read `QueryEmptyThread` and every reader/writer of
   `bThreadBsBufferUsage` — confirm no two live tasks share a slot index, and
   write the two-line proof in the log.
2. **Order-based assembly**: read `AppendSliceToFrameBs`/`WriteSliceBs` —
   confirm output order depends only on slice index, never on completion
   order, and write it down.
3. **The dead events**: grep each event array's signal/wait sites; list which
   are dead in the port. Anything live gets a one-line role note — it must be
   reproduced or replaced, not dropped silently.

Accept: three written findings in the log; no code moved yet.

## Step 2 — fork/join on the fixed slice modes

Replace the task/pool dispatch for every mode **except** `SM_SIZELIMITED`:

1. Build the job split: per slice, `&mut` into the slice bank
   (`split_at_mut`/iterators over the `Vec<SSlice>`), the thread's bs chunk,
   and the shared read-only views — the charter's `SliceJob` shape. The
   claiming mutex disappears because the split IS the claim.
2. `std::thread::scope` spawn per job; join is the barrier; stitching
   unchanged (`AppendSliceToFrameBs` runs after join, in slice order, as
   today).
3. `UpdateMbMap` and `LoadBalancing` tasks fold into the same scope or run
   inline where the C++ ran them serially — read before deciding, note the
   decision.
4. The single-threaded path stays exactly as it is.
5. Delete what this strands in `wels_task_management.rs` (the three non-CS
   task types if nothing reaches them) and the pool entries they used.
   `ConstrainedSizeSlicing` and everything it needs **stays live** for B.

Work mode-by-mode, `gates.sh commit` each; the `mt` sweep rows are the
determinism proof per commit.

Accept: all fixed-mode `mt` rows byte-identical in both profiles (369/369);
`grep -rn 'TaskPtr' --include='*.rs' src/` shows only the
`ConstrainedSize`/pool remnant B owns; no new `unsafe impl`, no raw pointer
crosses a spawn (grep the new code); `family` green; scoped Miri green.

## Step 3 — first denies

Any of the three MT-seam files (or `common/wels_thread_pool.rs`) whose unsafe
count reaches zero gets its `#![deny(unsafe_code)]` (and its `pub mod` allow
removed) this session. Do not force it — B finishes the set.

## Step 4 — close

1. The span (hard rule 3) into `perf_baseline.md` — with the spawn-cost
   question answered: rows per thread-count compared, the persistent-pool
   decision for B stated from the numbers.
2. `MIRI_SCOPE=encoder bash rust/tools/gates.sh exit`; adjudicate any F3 hit
   per hard rule 5.
3. Log entry: the step-1 proofs, modes converted, deletions, the tag count
   retired so far (of 105), the before-arm record, the span, what B inherits
   exactly.
4. Plan §0 row (A spent, B next), phase7.md §3 row, `perf_baseline.md`.

## Do not touch

| what | why |
|---|---|
| `SM_SIZELIMITED` / `ConstrainedSizeSlicing` path, `DynamicSliceBs`, `sSliceBs.pBs` | session B's, with the F3 verdict |
| `AppendSliceToFrameBs` / `WriteSliceBs` ordering structure | the determinism carrier |
| the F12 Miri skip | deleted in B with the pool |
| decoder threading stubs (F36) | B settles by reading, one hour cap |
| `wels_encoder_ext.rs`, Phase 9's parked families, `SCREEN_CONTENT(dormant)` | later phases |

## Report back, in this order

1. One line: steps landed; `exit` verdict; HEAD; tree state.
2. The F3 before-arm: hits/500, load method, wall time.
3. The three step-1 proofs, one line each.
4. Modes converted; what was deleted; tags retired (n of 105); any new
   `unsafe` written (target: 0).
5. The span, the spawn-cost answer, and the persistent-pool decision for B.
6. Anything found and not fixed, with owner.
7. What session B inherits, exactly.
