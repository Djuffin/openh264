# Phase 7, session B — fork/join through one named seam, and the F3 verdict

You are executing Phase 7's main session under a made decision. Work the steps
in order, commit per unit, run the gates as stated, report in the format at the
end.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the behavioral reference; byte parity is gate-enforced. Charter:
  `rust/docs/prompts/phase7.md`.
- Session A verified the two scheduling premises in writing (slot disjointness
  under `mutexThreadBsBufferUsage`; assembly order a pure function of slice
  index), found the third premise broken (**F67**: `sWelsEncCtx` is `!Sync`
  for 12 reasons; the 105 tagged items are the slice-encode tree's signatures,
  so the context split is Phase 9's precondition, not this phase's step),
  measured F3's true rate (**25/3000 ≈ 1/120** under proper load), deleted the
  dead event machinery, and fixed the translation-era `pThreadBsBuffer` leak.
- **The ordering decision is made — D-mt-1 (plan §7.4): F67's option 2.** One
  named `unsafe impl Send` on **one** job type at the spawn seam. Everything
  else the charter marked for deletion dies. The impl carries a three-part
  safety comment — premise 1 (slot disjointness), premise 2 (order-based
  assembly), and session A's bound (**concurrency capped at the allocated
  buffer count, not the slot count**) — and is tagged
  `// unsafe-cat: send-seam(Phase 9)`: it retires when Phase 9's context split
  makes the job naturally `Send`. The workers keep calling the existing
  slice-encode tree with a raw context pointer exactly as the pool's tasks do
  today — **the access pattern does not change; only the machinery around it
  shrinks.**
- **Amended hard rule for this phase**: no `unsafe impl` anywhere EXCEPT that
  single D-mt-1 type; no `static mut`; no `usize` laundering; no raw pointer
  crosses a spawn outside the one seam type.
- Docs to update at close: `safety_refactor_log.md`, plan §0 row, phase7.md
  §3, `perf_baseline.md`, `phase0_findings.md` (the after-arm).
- Commits: `refactor(T7.B<n>): …` / `gate(T7.B<n>): …` / `docs(T7.B-close)`.

## Hard rules

1. **Gates**: `gates.sh commit` per commit; `family` per step (sweeps
   **369/369 both profiles** — determinism per `(config, t)` IS the gate);
   the close per step 7.
2. **Miri**: `MIRI_SCOPE=encoder`, once per step end at most. The
   `--skip wels_thread_pool` (F12) is deleted in step 3 **in the same commit
   that deletes the pool** — after that, the new threading code runs under
   Miri like everything else.
3. **Perf**: one span at the close (`perfpair.py`, 7 pairs + null), and it is
   finally meaningful for threads because step 0 fixes F68 first. On a median
   breach: bisect before acting. Spawn cost has a pre-authorized remedy (the
   safe persistent pool: workers + `mpsc::Sender<Box<dyn FnOnce + Send>>`) —
   build it only if the fixed bench shows the cost.
4. **Counts are anchors** (2026-08-20, session A's close). Re-grep first.
5. **F3 fingerprint**: `mt`, `sm=3`, `t∈{2,4}`, any wrong output length. One
   sweep hit → re-run that config 5×; all byte-identical → record the
   measurement, continue. The measured rate is ≈1/120 **under load** and
   density-sensitive (measurement 89: 1/120 → 1/63) — sweep-time hits stay
   rare; do not panic on one, do not ignore two.
6. **No behavior change**: no byte moves in any mode at any thread count.
7. **Overflow**: stop at a green whole-mode boundary; session C closes the
   phase if needed. The determinism gate does not bend.

## Current state — verified facts

- **The claiming today** (A's premise-1 proof, in the log): a task claims bs
  slot `k` via `QueryEmptyThread(bThreadBsBufferUsage)` test-and-set under
  `mutexThreadBsBufferUsage` in `InitTask`; `FinishTask` clears it. Slot
  `k`'s buffer is `pThreadBsBuffer[k]` (now freed correctly, T7.A3).
- **The assembly** (premise 2): after the barrier, the calling thread walks
  slice index 0..N through `AppendSliceToFrameBs`/`WriteSliceBs`. **Do not
  restructure these two functions.**
- **The pool**: `common/wels_thread_pool.rs` — `TaskPtr` + its 2 `unsafe
  impl`s, `CWelsTaskThread`, `CWelsList`/`CWelsNonDuplicatedList`, one
  `static mut` singleton. `wels_task_management.rs` — `CWelsBaseTask` with a
  discriminant, four wrapper types, `ExecuteTask` /
  `ExecuteTaskConstrainedSize` bodies. The remaining 3 `unsafe impl` pairs
  are in the encoder MT files.
- **`SM_SIZELIMITED`** is the `ConstrainedSizeSlicing` path
  (`ExecuteTaskConstrainedSize`); F3 lives on it (`sm=3`).
- **The bench hole (F68)**: `benches/c_vs_rust_bench` never sets
  `uiSliceMode` → `SM_SINGLE_SLICE` → every `[4t]` row is single-threaded on
  both sides.
- **`bUseLoadBalancing` is `false`** in both diffharness drivers and in the
  default params — `CWelsLoadBalancingSlicingEncodingTask`,
  `AdjustBaseLayer`/`AdjustEnhanceLayer`, `DynamicAdjustSlicing`,
  `UpdateMbListNeighborParallel` have **no byte coverage in any gate**, and
  the path's output is timing-dependent by design (frame N+1's boundaries
  read frame N's slice times).
- The 105 tagged items (`port-raw(Phase 7)` 89 + `MT` 16) are the tree's
  signatures — **they do not retire this phase** (F67); they re-tag to
  `port-raw(Phase 9)` where the deny sweep of the seam files needs them
  named.
- The before-arm: **25/3000**, four `rustc` streams at load-average 12–15,
  449 s, shapes 18 empty / 6 short / 1 long. The arm script is recorded with
  measurement 88 in `phase0_findings.md`.

## Step 0 — two instruments before any conversion

1. **F68**: give `benches/c_vs_rust_bench` a slice-mode knob beside
   `BENCH_THREADS` (default unchanged: `SM_SINGLE_SLICE`, so every existing
   number stays comparable). Add multi-slice rows (e.g. `sm=1 n=4` and
   `sm=3`) to the bench matrix used for this phase's spans, both sides.
   Verify the `[4t]` rows now scale on the C++ side; record the first honest
   thread-scaling numbers.
2. **Commit the arm script**: reconstruct `f3_arm.sh` verbatim from
   measurement 88's record into `rust/tools/f3_arm.sh`, and verify a 200-run
   smoke gives a rate consistent with 1/120 under the same load discipline.

Accept: bench knob works both sides; `f3_arm.sh` committed and smoke-run;
no production code touched.

## Step 1 — the seam type, and fork/join on the fixed modes

1. Define the one job type (e.g. `SliceJobHandle`) holding: the slice/bank
   position, the bs slot index, and the raw context pointer — with the
   **single** `unsafe impl Send` and the three-part safety comment + the
   `send-seam(Phase 9)` tag. `debug_assert!` the buffer-count bound at
   construction (jobs ≤ allocated buffers).
2. Replace the pool dispatch for every fixed mode with
   `std::thread::scope` + one spawn per job; the slot claim becomes the
   job's index (the mutex dies); the join is the barrier; assembly unchanged.
3. Fold `UpdateMbMap` the way the C++ orders it (read first; note the
   decision). `LoadBalancing` handling: see step 5 — do not silently delete.
4. The single-threaded path is untouched.

Work mode-by-mode; `gates.sh commit` each; the `mt` rows are the per-commit
determinism proof.

Accept: all fixed-mode `mt` rows byte-identical, 369/369 both profiles;
exactly **one** `unsafe impl` in the diff; `family` + scoped Miri green.

## Step 2 — `SM_SIZELIMITED` on the queue

Replace `ConstrainedSizeSlicing`'s claiming with a work distribution that
preserves today's claiming order exactly (it is index-driven — keep it
index-driven: an `AtomicUsize` next-slice counter or an `mpsc` of indices,
whichever reproduces the current order; read `ExecuteTaskConstrainedSize`
first and write the order argument down before choosing). Same seam type,
same assembly.

Accept: every `sm=3` row byte-identical in both profiles (`sl` preset rows
included); `family` green.

## Step 3 — the machinery dies

Delete: `common/wels_thread_pool.rs` (file), `TaskPtr` + both impls, the two
C++-list ports, `CWelsTaskThread`, the `static mut`, the claiming mutex and
`bThreadBsBufferUsage` if the split made them dead, the task wrapper types
and `CWelsBaseTask` if nothing reaches them, `pTaskManage` and
`mutexEncoderError` context members if now dead (read first). **Same commit**:
delete `--skip wels_thread_pool` from `gates.sh` (F12 closes) and run the
scoped Miri step over the new threading.

Accept: `grep -rn 'TaskPtr\|static mut\|unsafe impl' src/common src/encoder`
→ only the one D-mt-1 impl; F12's skip gone; Miri green; ratchet
re-baselined with the shape explained.

## Step 4 — the F3 after-arm

`bash rust/tools/f3_arm.sh` — **2000 runs**, same four-stream load
discipline, same clip/config. Verdicts:
- **0/2000** → ablated (P ≈ e⁻¹⁶·⁷ at 1/120): F3 closes as "the deleted
  claiming/pool machinery," with the before/after arms as the evidence.
- **hits at ≈1/120** → the defect is upstream of the deleted machinery and
  now lives in a surface a fraction of the size: enumerate that surface (the
  seam type, the scope spawn, the counter/queue, the assembly walk) and open
  the cornering as a named finding with the arms attached.
Either way: record the arm and the verdict in `phase0_findings.md`.

## Step 5 — the load-balancing ruling

Read the `bUseLoadBalancing` path end to end. Then one of:
- **Complete in the port** → it cannot be byte-gated by construction
  (timing-dependent boundaries): record it as the project's second
  expected-divergent class (the `CABA2_SVA_B` precedent — a recorded
  position, not a defect), add structural coverage only (one probe run:
  no crash, slice counts sane, Miri clean), leave the default off.
- **Incomplete/unreachable** → fence it exactly as screen content was
  (tag, guard cited, owner recorded), and say which.
Write the ruling in the log either way; one hour cap on the read.

## Step 6 — the stragglers

1. `sSliceBs.pBs` → owned per-slice (`Vec<u8>` in `SSliceBs`), the last two
   allocator sites in `svc_encode_slice.rs` die.
2. `DynamicSliceBs` buffers → owned by the job/worker, the last two in
   `encoder_ext.rs` die. `pMemAlign` census → 0 live; `pMemAlign` field
   deleted; `common/memory_align.rs` retired (the dormant screen-content 8
   move to a comment or die with the field — read which).
3. The MT context members (`pSliceThreading` innards, `pDynamicBsBuffer`)
   typed or deleted as the redesign leaves them.
4. **F61**: verify by construction that the class (bank growth without
   re-stamp) cannot recur in the new shape; record closed-by-redesign, or
   what remains.
5. F36 (decoder threading stubs): settle by reading, one hour cap — delete
   or leave with one line saying why.

Accept: `grep -rn 'WelsMalloc\|WelsFree' --include='*.rs' src/encoder
src/processing src/common` → 0 live (dormant-tagged only);
`memory_align.rs` gone or its retirement blocked-and-named.

## Step 7 — deny, and the phase close (or the handoff to C)

1. `#![deny(unsafe_code)]` on `slice_multi_threading.rs`,
   `wels_task_management.rs` (their `pub mod` allows retire), and whatever
   remains of the pool's home; every surviving item allowed + tagged
   (`port-raw(Phase 9)` for the tree signatures, `send-seam(Phase 9)` for
   the impl, `cursor` where tests exist). Re-tag the 89 `port-raw(Phase 7)`
   items to their real owner.
2. The span (step 0's bench making the thread rows honest), cumulative
   restated (≈+15…17%, tripwire +25%).
3. The **full unscoped** `gates.sh exit` — the phase's handoff gate.
4. Phase-close log entry (the arc: 3 files 3134 lines → final, 5 impl pairs
   → 1, F3's verdict, F12/F61/F36 dispositions, the leak, the two arms);
   plan §0 **Phase 7 COMPLETE** row; phase7.md §3; Phase 8 named next.
If steps 2–6 overflow instead: stop green, report, C closes.

## Do not touch

| what | why |
|---|---|
| `AppendSliceToFrameBs` / `WriteSliceBs` structure | the determinism carrier |
| the 105 tree signatures beyond re-tagging | Phase 9's split (F67/S42) |
| `wels_encoder_ext.rs`; parked perf families; `SCREEN_CONTENT(dormant)` | Phases 8 / 9 / 10 |

## Report back, in this order

1. One line: steps landed; phase closed or handed to C; exit verdict; HEAD.
2. F3: after-arm hits/2000 and the verdict sentence.
3. The seam: the one impl's location, its three-part comment, jobs-≤-buffers
   enforced where.
4. What died (files/types/lines) and what the ratchet says.
5. Determinism: sweep totals both profiles; any F3-protocol events.
6. F68's honest thread-scaling numbers; the pool decision.
7. The load-balancing ruling; F61/F36/F12 dispositions.
8. Anything found and not fixed, with owner.
