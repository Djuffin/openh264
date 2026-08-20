# Phase 6, session E — the records, the parameter families, and the third SAD/SATD attempt

## What this session does, and why

After session D, every big encoder structure on the macroblock path **owns its
storage**: the layer (`SDqLayer`) is `Box`-built and holds the macroblock records
(`MbArray<SMB>`) and the slice banks (`Vec<SSlice>`); the slice holds its scratch
(`SMbCache`, `SCabacCtx`) inline. What is still raw on that path is almost entirely
**spelling**: functions take `*mut` to things that now have single owners, and a
handful of small scratch records still carry raw pointer fields that are derivable
from things already in scope.

This session:

1. closes a **byte-coverage hole** in the differential sweep (the slice-realloc path
   is exercised by no standing configuration),
2. fences the **screen-content residue** — dormant feature scope kept for Phase 10
   (decision D-scr-1), tagged rather than deleted,
3. converts the **record and parameter families** to references —
   `SWelsMD`, `SWelsME`, `SMVUnitXY`, `SMeRefinePointer`, `SCabacCtx`, and the
   `*mut SMB` / `*mut SMbCache` parameter spellings — as three signature closures,
4. makes the **third, boxed attempt** at the parked SAD/SATD kernels, whose blocking
   mechanism (per-call setup cost) is exactly what step 3's conversions remove.

Everything reached through the encoder context stays raw (that flip is session G).
Everything reached through pictures/planes stays raw (session F). The boundary list
is at the end; when in doubt, check it before converting.

**Execution order is the section order. If the session runs short, drop whole steps
from the end** (step 5 first, then step 4) and say so in the log — never drop parts
of a step.

## Ground rules

- **Gates**: `bash rust/tools/gates.sh commit` in every commit, `gates.sh family`
  at each step's close (builds + tests + ratchet + census + both diffharness sweeps
  in both profiles), one `gates.sh exit` at the session close.
- **Miri runs once, at the session close, as the `exit` battery's `--lib` step —
  not between steps.** Cost model: the four encoder probes are ≈460 s per run
  (the dynamic-slice probe alone ≈357 s); the whole `--lib` step read 1432 s at
  session D's close. Budget the close accordingly.
- **Perf**: one measurement for the whole session — 7 pairs against the HEAD this
  brief is committed at (`rust/tools/perfpair.py`), read against a fresh null band.
  No stream measurements between steps. Step 5 uses its own microbench only.
- **Re-grep every count in this brief before acting on it.** Counts below were
  measured 2026-08-19; the grep is
  `grep -rn '\*mut <T>\b' --include='*.rs' src/encoder src/processing`.
- **Every struct whose layout changes re-pins its `assert_size!` in
  `src/encoder/abi_guard.rs` in the same commit** — measure the new size, pin it.
  Never delete an assertion; a diff that deletes one is a defect.
- **Deletions are compiler-enumerated**: delete the root (a field, a function), let
  the build name everything that dies with it, and list that in the commit message.
- **Sweep flake protocol**: if exactly one sweep configuration fails with this
  signature — `mt` preset, `sm=3`, `t` of 2 or 4, wrong output length, either
  profile — re-run that exact configuration 5×. Byte-identical every time → a known
  intermittent (F3): record it as a measurement in `rust/docs/phase0_findings.md`
  and continue. Any other failure is yours: stop and fix.
- The C++ tree is the behavioral reference. Byte parity against it is the gate;
  its *pointer spellings* are not a constraint.

## The map — where everything lives (verified 2026-08-19)

| struct | defined | size (pinned in `abi_guard.rs`) | owner / instances |
|---|---|---|---|
| `SWelsMD` | `md.rs:194` | 4000 | **stack**: `let mut sMd = SWelsMD::default()` at `svc_encode_slice.rs:1450`, `:1566`, `:2076`, `:2090` — nowhere else |
| `SWelsMD_sMe` | `md.rs:182` | (inside the 4000) | 41 inline `SWelsME`: `sMe16x16` + `sMe8x8[4]` + `sMe16x8[2]` + `sMe8x16[2]` + `sMe4x4[4][4]` + `sMe8x4[2][4]` + `sMe4x8[2][4]` |
| `SWelsME` | `svc_motion_estimate.rs:151` | 96 | inside `SWelsMD.sMe` only |
| `SMeRefinePointer` | `md.rs:260` | (pin if changed) | **stack**: one builder, `svc_base_layer_md.rs:1576` |
| `SCabacCtx` | `set_mb_syn_cabac.rs:188` | 504 | `SSlice.sCabacCtx` (`svc_encode_slice.rs:299`) — **already pointer-free inside**: its buffer positions are `usize` offsets (`m_iBufStart`/`m_iBufEnd`/`m_iBufCur`) |
| `SMVUnitXY` | `encoder_context.rs:107` | 4 (two `i16`) | value type; lives in `SMB.sMv: [SMVUnitXY; 16]`, in `SMbCache.sMvComponents: SMVComponentUnit` (`md.rs:371`, the `sMotionVectorCache` array), in `SWelsME.sMvp/sMvBase/sDirectionalMv/sMv`, in `SSlice.sMvStartMin/Max` |
| `SMB` | `md.rs:298` | 208, `repr(C)` `Copy` | rows of the layer's `MbArray<SMB>` (`SDqLayer.sMbDataP`); raw access only via the root accessors `mb_list_root`/`mb_at` (`svc_encode_slice.rs`, top) |
| `SMbCache` | `md.rs` (~354) | 5600, `repr(C, align(16))`, no `Copy` | `SSlice.sMbCacheInfo` (`svc_encode_slice.rs:277`) |

Raw pointer fields **inside** the records, and their disposition:

| field | what it points at | this session |
|---|---|---|
| `SWelsMD.pMvdCost`, `SWelsME.pMvdCost` (`*mut u16`) | a **row root** of the context's MVD cost table — set at `svc_encode_slice.rs:1418` as `pMvdCostTable.add(luma_qp * stride)`, copied into each `SWelsME` by `InitMe`; the table is one context-owned `WelsMallocz` block (`encoder_ext.rs:1293`, freed `:1857`); readers: `md.rs:706` (`MvdCost` accessor), `svc_motion_estimate.rs:550/:674/:835/:979–:1005` | **stays raw** — it aliases context-owned memory; converts at G when the table becomes owned |
| `SWelsME.pEncMb/pRefMb/pColoRefMb` (`*mut u8`) | per-MB cursors into the encode/reference picture planes | **stay raw** — the picture family is session F's |
| `SWelsME.pRefFeatureStorage` | the screen-content feature search — dormant in the port, **live in the C++** | **stays raw, tagged in step 0b** (Phase 10) |
| `SMeRefinePointer`'s five `*mut u8` | fixed offsets into one `SMbCache` buffer (details in step 3) | **converted in step 3** |
| `SMeRefinePointer.pfCopyBlockByMode` | fn pointer (params are planes) | stays a fn-pointer field |

Parameter sites by family and receiver file (the work list; re-grep first):

| family | total | where |
|---|---|---|
| `*mut SWelsMD` | 53 | svc_mode_decision 24, svc_base_layer_md 18, wels_func_ptr_def 7, svc_encode_slice 4 |
| `*mut SWelsME` | 39 | svc_motion_estimate 21, svc_base_layer_md 12, svc_mode_decision 4, md 2 |
| `*mut SMVUnitXY` | 65 | svc_base_layer_md 34, svc_mode_decision 27, svc_motion_estimate 2; **picture.rs 1 + wels_preprocess.rs 1 — session F's, leave** |
| `*mut SMeRefinePointer` | 17 | svc_base_layer_md 14, md 3 |
| `*mut SCabacCtx` | 25 | svc_set_mb_syn_cabac 13, set_mb_syn_cabac 12 |
| `*mut SMB` | 128 | svc_mode_decision 37, svc_base_layer_md 18, svc_encode_slice 14, svc_set_mb_syn_cabac 13, deblocking 11, rc 9, wels_func_ptr_def 8, svc_encode_mb 7, svc_set_mb_syn_cavlc 4, md 4, encoder_ext 2, slice_multi_threading 1 (**MT file, Phase 7 — leave**) |
| `*mut SMbCache` | 96 | svc_mode_decision 41, svc_base_layer_md 16, md 16, svc_set_mb_syn_cabac 8, svc_encode_mb 7, wels_func_ptr_def 5, svc_encode_slice 2, svc_set_mb_syn_cavlc 1 |

Also in the same signatures, **not** targets this session but relevant to (g) below:
`*mut SSlice` ≈103 encoder-side (svc_encode_slice 41, svc_mode_decision 17, rc 11, …)
and `*mut SDqLayer` ≈78 (svc_encode_slice 30, svc_mode_decision 9, …).

## How to convert (applies to steps 1–4)

**(a) `*mut Record` parameter → `&mut Record`.** `&mut T` coerces to `*mut T` at a
call site, so a converted caller feeds a not-yet-converted callee without churn —
convert in whatever order the closure makes natural.

**(b) Caller-side casts just drop.** Most `SMVUnitXY` "pointers" are literally
`&mut (*sMe16x8).sMvp as *mut SMVUnitXY` at the call site (34 of the 65 are in
svc_base_layer_md in exactly this shape) — the fix is deleting the cast.

**(c) A parameter that names what another parameter already reaches is a cache —
delete it, derive in the callee.** The recurring pair is `pSlice` alongside
`pMbCache`/`sMbCacheInfo` (which is `&mut (*pSlice).sMbCacheInfo`). Enumerate these
pairs per step before editing. The aliasing rule, concretely:

```rust
// BAD — two live paths to one slice; the callee touches pSlice while the &mut lives:
f(pSlice, &mut (*pSlice).sMbCacheInfo);
// GOOD — one path in; the callee derives what it needs:
f(pSlice);                         // inside f: let cache = &mut (*pSlice).sMbCacheInfo;
// FINE — disjoint field borrows at one call site:
g(&mut (*pSlice).sMvStartMin, &mut (*pSlice).sMvStartMax);
```

**(d) A pointer into the middle of an array derives from the array root**:
`addr_of_mut!((*m).arr).cast::<T>().add(k)`, never `(*m).arr[k..].as_mut_ptr()`
(which narrows provenance to the slice and is UB at the first out-of-slice access).
The deny-by-default `dangerous_implicit_autorefs` lint catches the worst spelling.

**(e) Dispatch-table slot types retype in place.** The fn-pointer type aliases in
`wels_func_ptr_def.rs` (slots at `:54–:121`, `:210`) name `*mut SWelsMD`/`*mut SMB`/
`*mut SMbCache`; give them the converted reference types. They are internal dispatch
— **verify no installed fn is `#[no_mangle]`** (grep before the commit) — and a slot
whose params become references also **drops `extern "C"`** (a reference to a
non-`repr(C)`-safe type behind `extern "C"` trips `improper_ctypes_definitions`).
The table mechanism itself stays; G dissolves it.

**(f) Port-added null guards delete with the params.** Entry checks like
`WelsMdP16x16`'s five-way `if pFunc.is_null() || … { return i32::MAX; }`
(svc_mode_decision, right after `InitMe`) have **no C++ counterpart** — check each
against the C++ function, then delete with the retyping.

**(g) `pSlice`/`pCurLayer` params convert opportunistically only.** Convert one only
when the callee does not also re-reach the same object through `pCtx` while the
borrow lives (that re-reach is the classic aliasing bug here). Default: leave raw
and keep a survivor list for the close log. `pEncCtx`/`pFunc` params always stay raw.

**(h) Derives are the compiler's call.** Drop `Copy`/`Clone` on `SWelsMD` (a 4000-byte
memcpy) if nothing copies it by value; the build is the authority. Keep `repr(C)`
unless a change forces otherwise.

**(i) Close each step with the grep**: the step's families read zero in the step's
files, or the survivors are named in the commit message.

## Step 0a — give the slice-realloc path standing byte coverage

**Goal**: the standing sweep gains a preset that drives `FrameBsRealloc`, so the
path stays byte-checked forever, not just in session D's one-off runs.

**Facts**: `FrameBsRealloc` (svc_encode_slice.rs:3173) runs only when the coded
slice count crosses `iMaxSliceNum`, which opens at 35. The existing size-limited
sweep rows code at most 9 slices, so no standing configuration reaches it. Session D
validated its fix out-of-band: `tools/diffharness/compare.sh`, `st` single-threaded,
320x192, 12 configurations — 3 SIGBUS + 4 byte divergences before the fix, 12/12
byte-identical after. At the probe's constraint (401 bytes), 112x96 codes 37 slices;
48x32 codes 3, 96x64 codes 21, 96x96 codes 31.

**Do**:
1. Add preset `sl` to `rust/tools/diffharness/sweep.sh`: ~12 rows, `st`
   single-threaded, 320x192, `SM_SIZELIMITED_SLICE`, constraint values small enough
   that the coded slice count **crosses 35** (measure it — count output NALs or
   instrument once), both entropy coders, at least two rc modes. The existing st
   slice-row `check` invocation shows the argument shape:
   `check "<label>" $yuv $W $H $frames 26 $cabac -1 0 0 $SM $SN 1`.
2. Wire `sl` into the `all` case, and into `gates.sh`'s `sweep_gate` line (≈:199):
   `sweep.sh st mt def` → `st mt def sl`. The pass/fail tally is parsed from the
   `PASS=n FAIL=n` line — nothing hardcodes the configuration count.
3. **Prove the preset sees the path**: temporarily revert the re-aim loop at the
   bottom of `FrameBsRealloc` (the `for iBack in (0..=kiLayersBefore).rev()` loop) —
   at least one `sl` row must fail; restore, re-run green. Record both runs.

**Check**: both profiles green at the new totals (341 + N); the totals and the
red-proof go in the commit message, and every later gate quote uses the new totals.

## Step 0b — fence the screen-content residue (keep it; do not convert it)

**Goal**: the screen-content machinery is enumerated and tagged so steps 2–3 flow
around it. It is dormant feature scope — kept for Phase 10 — not dead code.

**Facts** (decision D-scr-1, 2026-08-19): `SCREEN_CONTENT_REAL_TIME` is live,
supported code in the C++ — validation accepts it with constraints
(encoder_ext.cpp:274: one spatial layer, AQ off, background detection off,
scene-change detect on; the port translates that block faithfully at
wels_encoder_ext.rs:1085), the FME storage is allocated for it
(encoder_ext.cpp:1032, :1129), and `PerformFMEPreprocess` runs per frame (:2747).
What blocks the mode in the port is one **port-added init-time guard**:
encoder_ext.rs:817 returns `ENC_RETURN_UNSUPPORTED_PARA` where the C++ allocates
the FME preparation, because that machinery was never ported (the preparation half
was deleted at T6.D2 and returns as a re-port from the C++). Re-enabling the mode
is **Phase 10**; until then nothing reaches the in-tree search half, so converting
it would be unverifiable by every gate this project owns — it stays exactly as it
is.

**Do**:
1. Verify the guard stands (encoder_ext.rs:817) — it is what makes leaving the
   family unconverted sound. If it does not, stop and report.
2. Tag each item of the dormant family with a one-line comment
   `// SCREEN_CONTENT(dormant: Phase 10)` — no other edit, no behavior change:
   - `svc_motion_estimate.rs`: `WelsMotionEstimateSearchStatic` (:586),
     `WelsMotionEstimateSearchScrolled` (:619), `WelsDiamondCrossFeatureSearch`
     (:1076), `SetFeatureSearchIn` (:1321), `SaveFeatureSearchOut` (:1370),
     `FeatureSearchOne` (:1383), `PerformFMEPreprocess` (:1296),
     `SFeatureSearchIn` (:201), `SFeatureSearchOut` (:252), `CalcFMESwitchFlag`
     (:441), `SWelsME.pRefFeatureStorage` (:170)
   - `svc_mode_decision.rs`: `InitMe`'s `pRefFeatureStorage` parameter (≈:1204)
   - `ref_list_mgr_svc.rs:1142`: the allocation branch
   - `picture.rs`: `SScreenBlockFeatureStorage` (:30),
     `SPicture.pScreenBlockFeatureStorage` (:101) and its `Reset` touch (:120)
3. Convert none of it. Steps 2–3 convert the signatures *around* it; a converted
   function that passes the storage through (`InitMe`) keeps that one parameter as
   `*mut SScreenBlockFeatureStorage`.

**Check**: `grep -rn 'SCREEN_CONTENT(dormant' src/` enumerates exactly the list
above; sizes unmoved (`SWelsME` 96, `SWelsMD` 4000); build clean.

## Step 1 — the syntax-writer closure

**Goal**: zero `*mut SCabacCtx` anywhere; zero `*mut SMB`/`*mut SMbCache` in the
four writer files.

**Files and counts**: `set_mb_syn_cabac.rs` (12 SCabacCtx),
`svc_set_mb_syn_cabac.rs` (13 SCabacCtx + 13 SMB + 8 SMbCache),
`svc_set_mb_syn_cavlc.rs` (4 SMB + 1 SMbCache — the `sMbCacheInfo: *mut SMbCache`
param at :773), `svc_encode_mb.rs` (7 SMB + 7 SMbCache).

**Facts**: `SCabacCtx` needs no internal work — its buffer positions are already
offsets. The flush site already spells the target:
`WelsCabacEncodeFlush(buf, &mut (*pSlice).sCabacCtx)` (svc_encode_slice.rs:2370).

**Do**: rules (a), (c), (f). Enumerate the `pSlice`-alongside-cache pairs in these
files before editing; expect several — the writers get both today.

**Check**: rule (i); `gates.sh family`.

## Step 2 — the mode-decision closure

**Goal**: `&mut SWelsMD` flows from the four stack roots down; `*mut SWelsMD`,
`*mut SMB`, `*mut SMbCache` read zero in the MD files and the dispatch slots.

**Files**: `svc_mode_decision.rs` (24 MD + 37 SMB + 41 SMbCache + 4 ME + 27
SMVUnitXY), `svc_base_layer_md.rs` (18 MD + 18 SMB + 16 SMbCache), `md.rs` (16
SMbCache + 4 SMB), `svc_encode_slice.rs` (4 MD + 14 SMB + 2 SMbCache — the four MB
loops), `wels_func_ptr_def.rs` (7 MD + 8 SMB + 5 SMbCache slot types).

**Do**: start at the four roots and walk down (rule (a) makes partial states
compile). Sub-records are field borrows: `&mut sMd.sMe.sMe8x8[i]`. Retype the slots
(rule (e)), delete the guards (rule (f)) — `WelsMdP16x16`'s shape recurs across the
slot-installed functions. Apply (c) wherever a callee gets both a slice and its
cache. `pSlice`/`pCurLayer`/`pEncCtx` params: rule (g).

**Check**: rule (i); `gates.sh family`.

## Step 3 — the motion-estimation closure

**Goal**: `*mut SWelsME`, `*mut SMVUnitXY`, `*mut SMeRefinePointer` read zero
(minus the two F-owned SMVUnitXY sites and the `SCREEN_CONTENT`-tagged family); `SMeRefinePointer` holds no raw pointer.

**Files**: `svc_motion_estimate.rs` (21 ME + 2 SMVUnitXY), `svc_base_layer_md.rs`
(12 ME + 34 SMVUnitXY + 14 MeRefine), `md.rs` (2 ME + 3 MeRefine),
`svc_mode_decision.rs` (`InitMe` and its callers).

**Facts, `SMeRefinePointer`**: `InitMeRefinePointer` (md.rs:1237) fills the fields
as fixed offsets into **one** `SMbCache` buffer —
`buffer_inter_pred_me(pMbCache).add(X + iStride)` with X = 0 (`pHalfPixH`), 640
(`pHalfPixV`), 1280 (`pQuarPixBest`), 1920 (`pQuarPixTmp`). Two leads discovered
reading it: **`pHalfPixHV` is not written there** — find its writer or it is dead
(delete it, re-pin); and `pQuarPixBest`/`pQuarPixTmp` **ping-pong** during
refinement — find the swap sites in `MeRefineQuarPixel`/its callers first, then
convert the pair to one selector bit over {1280, 1920} (the same recipe that turned
`SMbCache`'s prediction-buffer aliases into half-selectors in session C).

**Do**:
1. `SMeRefinePointer` → offsets/selector + `iStride`; readers derive from the
   `&mut SMbCache` already in scope at each use (rule (d) for any raw tail handed
   to a kernel). The struct is stack-local with one builder
   (svc_base_layer_md.rs:1576).
2. `InitMe` (svc_mode_decision.rs:1204) — `sWelsMe` becomes `&mut SWelsME`;
   `pMvdCost`/`pEnc`/`pRef`/`pRefFeatureStorage` stay raw (boundary list).
3. `SMVUnitXY`: drop the caller casts (rule (b)); single-element params →
   `&mut SMVUnitXY`; array params → `&mut SMVComponentUnit` or
   `&mut [SMVUnitXY; N]` from the host in scope.
4. `SQuarRefineParams` (md.rs, below SMeRefinePointer): the `*mut SQuarRefineParams`
   param → `&mut`; its `pRef`/`pSrcB[4]` plane fields **stay raw** (F). Keep
   `MeRefineQuarPixel`'s `#[inline(always)]`.

**Check**: rule (i); `gates.sh family`.

## Step 4 — the residue

**Goal**: the remaining `*mut SMB` sites outside the closures convert or are named.

**Sites**: `deblocking.rs` 11, `rc.rs` 9, `encoder_ext.rs` 2.

**Facts**: `deblocking.rs` reads **neighbours** — 18 pointer-arithmetic sites of the
`pCurMb.offset(-1)` / `.offset(-iMbStride)` shape. A `&mut SMB` for the current MB
cannot coexist with derivations of its neighbours from the same array, so its
neighbour-walking core may keep **root-derived raw cursors** (via
`mb_list_root`/`mb_at`) — convert the single-object signatures, and name the
neighbour-walkers as survivors. `rc.rs`'s 9 are single-object (`pCurMb` beside a
raw `pEncCtx` that stays); convert the SMB param only.

**Check**: rule (i) with the survivor list; `gates.sh family`.

## Step 5 — the third SAD/SATD attempt (boxed)

**Goal**: one bounded attempt at un-parking the SAD/SATD kernel families, now that
their callers' calling convention has changed; and the SATD solo measurement that
is owed regardless.

**Facts** (from the parked-families ledger, `rust/docs/perf_baseline.md` §Parked):
- `common/sad_common.rs`: 14 safe kernels, proven byte-correct, **unswapped**.
  First park 2026-08-09: ~7.0 ns/call L1-resident regardless of block shape;
  swapped stream cost +16.8% median / +78% worst. Second park 2026-08-10 on a
  rebuilt harness: 1.41x–4.94x across seven shapes against a ≤1.05x bar.
- `encoder/sample.rs`: 7 safe SATD kernels, **never installed**, parked by
  projection (SATD = SAD + a Hadamard butterfly, strictly more work) — **it has no
  measurement of its own; this session owes one no matter what.**
- The ledger's untested lead: "a real slices-and-offsets kernel" — per-call cursor
  construction was the measured cost, and callers building slices **once per
  partition search** is exactly what steps 2–3 make possible. Most call sites are
  fixed-size (`pfSampleSatd[BLOCK_16x16]` at svc_mode_decision.rs:610 and the like),
  so monomorphic direct calls can skip the `Option<fn>` table entirely — the same
  direct-dispatch move that recovered `mc.rs`'s deficit in Phase 2.
- Harness: `benches/sad_bodies_bench.rs` (`cargo bench --bench sad_bodies_bench`).

**Do, in order**:
1. Measure the 7 SATD kernels solo on the harness. Record the numbers.
2. One attempt: safe-signature kernels (slices + offsets), slices built once per
   partition search at the converted call sites, direct/monomorphic dispatch where
   the block size is fixed. SAD and SATD judged separately — if one clears the bar
   and the other does not, land the one.
3. Verdict: bodies ≤1.05x on the L1-resident microbench → swap lands and the ledger
   row closes. Otherwise → third park, dated verdict in the ledger, re-attempt
   point named (Phase 9). **Hard stop at this step's boundary — one attempt, one
   verdict.** The stream instrument stays the session span; no mid-step stream
   reads.

**If this step is dropped for budget**: the SATD solo measurement is still owed —
record it as owed in the ledger row.

## Step 6 — close

1. The span: 7 pairs against the session-start HEAD, plus a fresh null band; the
   tripwire is +25% on the median (cumulative encoder deficit stands at ≈+10…12%).
2. `bash rust/tools/gates.sh exit` — includes the one Miri `--lib` run (all four
   encoder probes must be green in its output) and both sweeps at the step-0a
   totals in both profiles.
3. Regenerate the ratchet baseline (`rust/tools/unsafe_ratchet.sh`) at the final
   tree.
4. Log entry (`rust/docs/safety_refactor_log.md`) with: family counts before/after;
   the survivor list (rule (g) leftovers, deblocking's neighbour-walkers, the two
   F-owned SMVUnitXY sites, the `SCREEN_CONTENT`-tagged family, the MT file's
   one SMB site); sizes re-pinned; the
   sweep's new totals with the red-proof; the SATD numbers and the step-5 verdict;
   the span. Update the session E rows in `rust/docs/safety_refactor_plan.md` §0
   and `rust/docs/prompts/phase6.md` §6, and `rust/docs/perf_baseline.md`.

## Stays raw this session — the boundary list

| what | why | whose |
|---|---|---|
| `pEncCtx`/`pCtx`/`pFunc` params, everything `(*pCtx).…`-reached, `pCurDqLayer` (271 sites) | the context is `mem::zeroed`-built until its flip | G |
| `pMvdCost` fields + `pMvdCostTable` | context-owned allocation (see the map) | G |
| `SWelsME.pEncMb/pRefMb/pColoRefMb`, `pEncSad`, every `*mut u8` plane+stride param, `SQuarRefineParams.pRef/pSrcB` | picture/plane family | F |
| `SPicture`, SVAA, `picture.rs` + `wels_preprocess.rs` SMVUnitXY sites, ref-strategy table | preprocess/pool session | F |
| `wels_encoder_ext.rs` | ABI boundary | Phase 8 |
| `slice_multi_threading.rs`, `wels_task_management.rs`, `wels_thread_pool`, `sSliceBs.pBs` | thread machinery | Phase 7 |
| the screen-content family — the tagged feature-search items, `SScreenBlockFeatureStorage`, `SPicture.pScreenBlockFeatureStorage`, the ref-list allocation branch | dormant feature scope behind the port's init guard (`encoder_ext.rs:817`); unverifiable until ported | Phase 10 (D-scr-1) |

No new `Vec`/`Box` fields anywhere this session; no allocation changes; sweep
changes are additive only.

## Done-test

- `grep -rn '\*mut SWelsMD\b\|\*mut SWelsME\b\|\*mut SMVUnitXY\b\|\*mut SMeRefinePointer\b\|\*mut SCabacCtx\b\|\*mut SMB\b\|\*mut SMbCache\b' --include='*.rs' src/encoder src/processing`
  reads **zero code sites** except the enumerated survivors.
- The screen-content family is tagged `SCREEN_CONTENT(dormant: Phase 10)` and
  otherwise byte-for-byte unchanged; `SWelsME` still 96, `SWelsMD` still 4000.
- The `sl` preset runs in `gates.sh` in both profiles, with the red-proof recorded.
- The SATD solo measurement exists; step 5 has a dated verdict (swap or third park).
- The span is inside its null band; `gates.sh exit` reads PASS.
