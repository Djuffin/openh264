# Phase 7, session C — the dropped-sync census, the stragglers, and the phase close

You are executing the last session of Phase 7. Work the steps in order, commit
per unit, run the gates as stated, report in the format at the end.

## Context

- Repo `/Users/eugene/projects/openh264`, branch `rust3`; crate
  `rust/crates/openh264-rs/`, paths relative to its `src/`. C++ at `codec/` is
  the behavioral reference. Charter: `rust/docs/prompts/phase7.md`.
- Start: commit `7e038545`, tree clean. Session B landed the fork/join through
  the one D-mt-1 seam (`SliceJobHandle`, `slice_multi_threading.rs:1108`, the
  only `unsafe impl` in `src/common` + `src/encoder`), `SM_SIZELIMITED` on the
  same seam, and deleted 2,115 lines of pool/task machinery (F12 closed with an
  MT probe added). **F3 is closed**: its cause is **F69** — the raw translation
  dropped the `WelsMutexLock(&mutexSliceNumUpdate)` that brackets
  `AddSliceBoundary` + `++iSliceNumInFrame` (`svc_encode_slice.cpp:1776–1791`);
  restoring it took the arm from 20/2000 to **0/2000**; deleting the pool had
  not moved the rate (arm A, p≈0.54). **F3's acquittal protocol is therefore
  retired: from this session on, any wrong-length sweep output is a defect to
  fix, not a measurement to record.**
- Still open from B: **F71** (the root-accessor idiom is a `Unique` retag over
  the container — sixteen accessors converted; the residue is a shared write,
  `sLayerInfo.sNalHeaderExt` per slice per worker, which the C++ makes too at
  `svc_encode_slice.cpp:1649–1655`; the MT Miri probe at
  `svc_encode_slice.rs:4057` is `#[cfg_attr(miri, ignore)]` citing it);
  **F72** (load balancing half-translated: `CalcSliceComplexRatio`, the
  producer of the only input `DynamicAdjustSlicing` consumes, is never called,
  and the feature is **on by default** via `GetDefaultParams`; fenced
  `LOAD_BALANCING(incomplete: F72)` at two sites); **F61** and **F36** not
  reached; steps 6–7 of B's brief not reached; **the thread-scaling ceiling**
  — B's honest numbers show the port scaling 1.65× to two threads and then flat
  (C++ 2.56× at four), measured on the *old* pool; whether `thread::scope`
  has the ceiling is this session's span.
- **Rulings you execute** (plan §7.4): **D-mt-1** (the one seam) stands.
  **D-mt-2**: F72 is *completed*, not left fenced — one call at the C++'s own
  site; a default-on feature must not ship degenerate; it can never be
  byte-gated (two C++ runs differ), so it becomes the project's second
  expected-divergent class with structural coverage only.
- Docs at close: `safety_refactor_log.md` (session + phase-close entries),
  plan §0 (**Phase 7 COMPLETE** row, Phase 8 next), `phase7.md` §3,
  `perf_baseline.md`, `phase0_findings.md`/`phase7_findings.md` as needed.
- Commits: `refactor(T7.C<n>)` / `fix(T7.C<n>)` / `docs(T7.C-close)`.

## Hard rules

1. **Gates**: `gates.sh commit` per commit; `family` per step (sweeps
   **369/369 both profiles**); step 6 runs the **full unscoped** `exit`.
2. **Miri**: `MIRI_SCOPE=encoder` at step ends; unscoped once, at the close.
3. **Perf**: one span at the close with `BENCH_SLICE_MODE` rows
   (`sm=1 n=4` and `sm=3`, both sides) — the thread-ceiling question is
   answered here; on a median breach, bisect first.
4. **Anchors, not surfaces** (2026-08-20, B's close): re-grep first.
5. **Wrong-length output is a defect.** No retry protocol, no acquittal.
   Stop and fix. (If a hit appears on a configuration F69's fix should cover,
   the lock site or its scope is wrong — read the C++ bracket again.)
6. **No behavior change** in any gated configuration. F72's completion changes
   behavior only where `bUseLoadBalancing` is on, which no gate runs; record
   that class explicitly (step 1).
7. **No new `unsafe impl`, `static mut`, or laundering.** The D-mt-1 seam is
   the one.
8. **Overflow**: stop green; a session D closes. The gate does not bend.

## Step 0 — the dropped-sync census (the method F69 taught, at phase scale)

A field that looks dead in the port and is live in the reference is a dropped
statement — no gate this project owns can see one. Census every
synchronization site in the C++ encoder's MT paths and classify it:

- C++ sites (grep `WelsMutexLock|WelsMutexUnlock|WelsEventSignal|WelsEventWait|WelsAtomic|InterlockedIncrement`
  over `codec/encoder/core/src/*.cpp`): **16** — `svc_encode_slice.cpp` 2 (the
  F69 pair, now restored), `wels_task_encoder.cpp` 8,
  `wels_task_management.cpp` 6. Port sites: `slice_multi_threading.rs` 3,
  `svc_encode_slice.rs` 1.
- Per C++ site, one of: **(a)** guards machinery the port deleted (the pool,
  task lists, events) — no counterpart needed, say which deletion; **(b)**
  guards shared encoder *state* read or written by more than one worker — the
  port must have a counterpart: name it, or it is a dropped statement and you
  fix it in this step with a covering MT probe configuration.
- Then the inverse: every shared write the workers make in the port (start
  from F71's `sNalHeaderExt` and the slice-context counters) — is it bracketed
  in the C++? If the C++ brackets it and the port does not, fix; if neither
  does, record it as a shared C++ race (F61's class) with its byte-neutrality
  argument.

Accept: a 16-row table in the log + the inverse list; every (b) row has a
named counterpart or a fix commit.

## Step 1 — F72 completed (D-mt-2)

1. Add the one producer call where the C++ makes it (`CalcSliceComplexRatio`
   at its site in `svc_encode_slice.cpp`'s frame path — read it, cite it).
2. Remove both `LOAD_BALANCING(incomplete: F72)` fences.
3. Structural coverage: one probe configuration with `bUseLoadBalancing` on,
   `t=4`, multi-slice, several frames — no crash, slice counts sane each frame,
   Miri-clean if the probe is cheap enough to run scoped (if not, say why).
4. Record the **expected-divergent class**: output is timing-dependent by
   design (frame N+1's boundaries read frame N's slice times), both diffharness
   drivers keep `bUseLoadBalancing = false`, and the class joins
   `CABA2_SVA_B` in the referee's annotations so regeneration cannot lose it.

Accept: fences gone; the probe green; the class recorded; 369/369 unchanged.

## Step 2 — the stragglers (B's step 6)

1. `sSliceBs.pBs` → owned per slice (`Vec<u8>` in `SSliceBs`); the two
   allocator sites in `svc_encode_slice.rs` die.
2. `DynamicSliceBs` buffers → owned by the job/worker; the two in
   `encoder_ext.rs` die.
3. `pMemAlign`: census → 0 live (the 8 dormant screen-content hits die with
   the field or move to a comment — read which); the field deleted;
   `common/memory_align.rs` retired.
4. MT context members (`pSliceThreading` innards, `pDynamicBsBuffer`) typed or
   deleted as the redesign leaves them; `SSliceThreading` re-pinned if it
   keeps a size assert.
5. **F61**: with banks owned and slices named by position since Phase 6, and
   the growth path now on the seam — verify by construction that growth
   without re-stamp cannot recur; record closed-by-redesign, or exactly what
   remains and why.
6. **F36** (decoder threading stubs): read, decide delete-or-leave, one line
   why, one hour cap.

Accept: `grep -rn 'WelsMalloc\|WelsFree' --include='*.rs' src/encoder
src/processing src/common` → 0 live; `memory_align.rs` gone or its blocker
named; F61/F36 each with a one-line disposition.

## Step 3 — F71's residue, read before assigned

The shared write is `sLayerInfo.sNalHeaderExt` per slice per worker. Read
whether each worker can write its own copy (a per-job field) that the assembly
walk stamps onto the layer **after the join** — byte-neutral, because assembly
is order-based (premise 2). If yes: do it, retire the probe's
`#[cfg_attr(miri, ignore)]`, and run the MT probe under scoped Miri. If no:
leave the probe ignored with F71 cited, and hand the residue to Phase 9 with
the sentence that says why. Two hours cap either way.

## Step 4 — deny, and the re-tags

1. `#![deny(unsafe_code)]` on `slice_multi_threading.rs` (its `pub mod` allow
   in `encoder/mod.rs` retires — `wels_task_management.rs` no longer exists;
   delete its line); every surviving item allowed + tagged.
2. Re-tag the **89 `port-raw(Phase 7)`** items to `port-raw(Phase 9)` (F67:
   they are the slice-encode tree's signatures, Phase 9's context split
   retires them); re-examine the **16 `MT`** items — those that died with the
   machinery are gone; survivors keep `MT` only if they are still a thread
   seam, else re-tag.
3. Ratchet re-baselined with the shape explained.

Accept: `grep -rln 'deny(unsafe_code)' src/encoder src/processing | wc -l` =
the full module count; `grep -rn 'port-raw(Phase 7)'` → 0;
`grep -c 'allow(unsafe_code)'` = tag count per file.

## Step 5 — the span, and the thread-ceiling answer

`perfpair.py` 7 pairs + null, with the `BENCH_SLICE_MODE` rows. Read the
`[2t]`/`[4t]` scaling on `sm=1 n=4` and `sm=3`: if `thread::scope` scales past
two threads, record the win; if the ceiling persists, name the mechanism
(profile one run) or hand Phase 9 a measured target — do not guess. Cumulative
encode restated (≈+15…+17% vs the +25% tripwire; D-perf-6 Phase 9's).

## Step 6 — the phase closes

1. `bash rust/tools/gates.sh exit` **unscoped** — all seven probes (4 encoder +
   3 decoder) plus the MT probe by name; sweeps 369/369 both profiles; both
   benches bit-identical.
2. Phase-close log entry — Phase 7's arc: 3 files / 3,134 lines at open →
   what remains; 5 `unsafe impl` pairs → 1 (the seam, Phase 9's); **F3
   closed by F69 with the three arms**; F12, F61, F36, F68, F70, F71, F72
   dispositions; the leak; the census table; the thread-ceiling answer.
3. Plan §0: **Phase 7 COMPLETE** row (model: Phase 6's), sessions A–C spent,
   **Phase 8 named next** with its inheritance block re-verified; `phase7.md`
   §3 rows.

## Do not touch

| what | why |
|---|---|
| `AppendSliceToFrameBs` / `WriteSliceBs` order structure | the determinism carrier |
| the tree's signatures beyond re-tagging; `SliceJobHandle`'s impl | Phase 9's split (F67/S42, D-mt-1) |
| `wels_encoder_ext.rs`; parked perf families; `SCREEN_CONTENT(dormant)` | Phases 8 / 9 / 10 |

## Report back, in this order

1. One line: phase closed or not; unscoped `exit` verdict; HEAD; tree state.
2. The census: rows per class, any dropped statement found and fixed.
3. F72: the call added, the probe result, the class recorded.
4. Stragglers: allocator hits → 0?, `memory_align.rs`, F61, F36, one line each.
5. F71: moved-to-join or handed to Phase 9, and the probe's Miri state.
6. Deny/tags: modules denied, re-tags done, ratchet shape.
7. The span and the thread-ceiling answer.
8. Anything found and not fixed, with owner.
