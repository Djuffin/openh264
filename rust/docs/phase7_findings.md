# Phase 7 findings — the threading rework

Numbering continues the project-wide sequence; F66 is the last of Phase 6.

---

## F67 — the fork/join design has a third premise, it is unstated, and it does not hold: `sWelsEncCtx` is `!Sync` for twelve distinct reasons, so no slice-encode closure can cross a `thread::scope` spawn

**Status: OPEN, and it re-scopes the phase.** Found by session A at step 2, before
any conversion was attempted, by compiling the question rather than arguing it.

### What the charter assumed

Phase 7's design (`prompts/phase7.md` §1, from plan §2.2.7) rests on two premises,
both of which session A verified and **both of which hold** (proofs in the session-A
log entry, T7.A1):

1. disjointness is already index-based — no two live tasks share a bs-buffer slot;
2. assembly is already order-based — `AppendSliceToFrameBs` stitches by slice index
   after the join, so completion order is not observable.

From those it concludes the target shape is reachable:

```rust
std::thread::scope(|s| {
    for job in jobs { s.spawn(|| encode_slice(job, shared)); }
});
```

**The two premises are about scheduling. The target shape also needs a third thing,
which nobody wrote down: `shared` must be able to cross the spawn.** In Rust that is
not a property of the schedule, it is a property of the type — `Scope::spawn` requires
the closure to be `Send`, and a captured `&T` requires `T: Sync`.

### The measurement

Session A asked the compiler directly. A temporary probe added to
`encoder/svc_encode_slice.rs`, built, read, and reverted in the same step (tree clean
at `11f8695f`):

```rust
fn _probe_assert_sync<T: Sync>() {}
fn _probe_ctx_sync() { _probe_assert_sync::<sWelsEncCtx>(); }
```

**12 `E0277`s, 12 distinct blocking types**, and the nesting is the interesting part:

| blocking type | reached through |
|---|---|
| `*mut SSliceThreading` | `sWelsEncCtx` directly |
| `*mut CWelsPreProcess` | `sWelsEncCtx` directly |
| `*mut SWelsEncoderOutput` | `sWelsEncCtx` directly |
| `*mut CMemoryAlign` | `sWelsEncCtx` directly |
| `*mut c_void` | `sWelsEncCtx` directly (the mutex handles, `pTaskManage`) |
| `*mut u8` | `[*mut u8; 4]` in `sWelsEncCtx` |
| `*mut i8` | `SWelsSvcCodingParam` → `Box` → `Option<Box<…>>` → `sWelsEncCtx` |
| `*mut SMotionTextureUnit` | `SAdaptiveQuantizationParam` → `SVAAFrameInfo` → `Box` → `sWelsEncCtx` |
| `*mut SRefList` | `sWelsEncCtx` |
| `*mut SScreenBlockFeatureStorage` | `sWelsEncCtx` (Phase 10's, dormant) |
| `*mut i32`, `*mut u32` | `sWelsEncCtx` |

Five of the twelve sit inside types **other phases own**: `SWelsSvcCodingParam` is
Phase 8's (`wels_encoder_ext.rs`), `SScreenBlockFeatureStorage` is Phase 10's
`SCREEN_CONTENT(dormant)`, `CMemoryAlign` retires with the last two allocator sites in
session B, and `SVAAFrameInfo`/`CWelsPreProcess` belong to the preprocessing side.

### Why it is not a corner that can be worked around

Every route the charter or a reader might reach for is either forbidden by the
phase's own rules or is the thing the phase exists to delete:

- `unsafe impl Sync for sWelsEncCtx` — the standing rule forbids it, and the phase's
  headline deliverable is deleting the **5 existing `unsafe impl` pairs**. Adding a
  sixth to delete five is not progress.
- a `Send` newtype around `*mut sWelsEncCtx` — that is `TaskPtr`
  (`common/wels_thread_pool.rs:79`), by another name.
- laundering the pointer through a `usize` — explicitly on the charter's delete list.
- the charter's own fallback, a persistent pool over
  `mpsc::Sender<Box<dyn FnOnce + Send>>` — a closure capturing the context is not
  `Send` either, so the fallback has the identical hole.
- narrowing `shared` to only what the encode needs — this is the real fix, and its
  size is measured below.

### The size of the real fix, and it is already tagged

`WelsCodeOneSlice` (`svc_encode_slice.rs:2781`) is not a leaf. It reaches
`WelsSliceHeaderExtInit(pEncCtx, pCurLayer, pCurSlice)`, `ctx_rc_at` and
`GomRCInitForOneSlice`, `g_pWelsWriteSliceHeader[..](pEncCtx, pBs, pCurLayer, …)`,
`layer_pps(pEncCtx, …)`, `slice_bs_buffer(pEncCtx, …)`, and then
`g_pWelsSliceCoding[idr][dyn](pEncCtx, pCurSlice)` — the whole macroblock loop, the
mode decision, and the rate controller, all of which take the context raw and mutate
through it.

**That call tree is exactly this phase's tagged inventory.** The 105 items
(**89** `port-raw(Phase 7)` + **16** `MT`) are distributed:

```
40  encoder/svc_encode_slice.rs        8  encoder/svc_set_mb_syn_cavlc.rs
15  encoder/svc_mode_decision.rs       5  encoder/svc_base_layer_md.rs
11  encoder/svc_enc_slice_segment.rs   4  encoder/wels_func_ptr_def.rs
11  encoder/rc.rs                      3  encoder/svc_set_mb_syn_cabac.rs
                                       3  encoder/nal_encap.rs
                                       2  encoder/mod.rs, 2 encoder_ext.rs, 1 ref_list_mgr_svc.rs
```

**Zero of the 105 are in `slice_multi_threading.rs`, `wels_task_management.rs` or
`wels_thread_pool.rs`** — those three carry module-level allows instead. So the tag
count the phase was given to retire was never the pool's; it is, item for item, the
signature surface that would have to become `&`/`&mut` splits for `encode_slice(job,
shared)` to compile. The inventory and the blocker are the same object seen twice.

### Disposition — what this changes

**It does not change the design; it changes the order.** Fork/join over owned splits
is still right, and premises 1 and 2 still say the schedule is sound. What is now
measured is that **the context split is a precondition of the fork/join, not a
consequence of it**, and the phase's session plan had them the other way round.

Three orderings are available; the phase owner picks:

1. **Split first, fork later** (the design's own logic). Convert the 105 tagged
   items to `&`/`&mut`, make `sWelsEncCtx` structurally `Sync` — which needs Phase 8's
   `SWelsSvcCodingParam` and the preprocessor's `SVAAFrameInfo` first — then the
   fork/join is a small commit at the end. **Honest cost: several sessions, and it
   takes work from Phases 8 and 10 forward into 7.**
2. **One named `unsafe impl` at one seam.** Delete the pool, the `static mut`
   singleton, the two C++-list ports, `CWelsTaskThread`, the claiming mutex and the
   condvar dance; keep a single, minimal, documented `Send` job type at the spawn.
   Net: **5 `unsafe impl` pairs → 1**, several hundred lines of C++-shaped
   concurrency gone, and the F3 surface shrinks to the thing the ablation wants to
   measure. **This is not what the standing rule permits**, which is why session A
   did not do it — it needs an explicit decision, not an inference.
3. **Defer the fork/join to Phase 9** and let Phase 7 do what it *can* do safely:
   the dead machinery (done, T7.A2), the leak (done, T7.A3), the `SM_SIZELIMITED`
   work queue, and the F3 after-arm — all of which are reachable without crossing
   the spawn seam, because they run on the calling thread.

Session A took none of them on its own authority and stopped at a green boundary,
per the phase's own drop rule. **The decision is the phase owner's and it is the
first thing session B needs.**

### One consequence that is independent of the choice

Premise 1's proof carries a bound that was never written down and that **any**
replacement must honour: `QueryEmptyThread` scans all `MAX_THREADS_NUM` slots, but
only `min(GetThreadPoolThreadNum(), MAX_THREADS_NUM)` buffers are allocated. It is
the *pool's* concurrency cap that keeps the claim in range. A fork/join that spawns
one thread per slice — the charter's literal shape — hands out a slot index with a
null buffer behind it as soon as a fixed mode asks for more slices than threads, and
the sweep asks for that routinely (`t=2` with `sm=1 n=4`). **Cap the concurrency at
the buffer count, or allocate one buffer per slice.**

---

## F68 — the encoder bench's thread axis does not exercise threading: every `[4t]` row runs the single-threaded path

**Status: OPEN, one line to fix, and it blocks a decision the charter delegates to
the span.** Found by session A while reading the T7.A span rather than by the span
failing.

`benches/c_vs_rust_bench.rs` sweeps `iMultipleThreadIdc` over `BENCH_THREADS`
(default `1,4`) and reports a row per stream per thread count. It builds its
parameters with `GetDefaultParams` and then sets width, height, frame rate,
bitrate, `iSpatialLayerNum` and `iMultipleThreadIdc` — **and nothing else**
(`:179`). In particular it never sets
`sSpatialLayers[0].sSliceArgument.uiSliceMode`, which `GetDefaultParams` leaves at
**`SM_SINGLE_SLICE`** (`param_svc.rs:385` and `:473`, `encoder_ext.rs:1613`).

Parameter validation never raises the slice mode to match a thread count; it only
ever forces it *down* to `SM_SINGLE_SLICE` (`encoder_ext.cpp:543`, `:601`, `:609`,
and the Rust mirror). And `SM_SINGLE_SLICE` is the **first** arm of the slice-mode
chain in `WelsEncoderEncodeExt` (`encoder_ext.rs:3290`), tested before any
`iMultipleThreadIdc > 1` condition in the chain: it calls `WelsCodeOneSlice` on the
calling thread and returns.

So a `[4t]` row creates a thread pool, takes a reference to it, allocates
`iCountBsLen` bytes per worker — and hands it no task. The measured speedup is
**0.0% to 1.4% at every resolution from QVGA to 1080p** (numbers in
`perf_baseline.md`, Phase 7 session A). That is the signature of a path that never
runs, and it has been read as noise in every prior span.

**What it blocks.** `prompts/phase7.md` §1 makes the persistent pool conditional:
"rebuilt safely **only if** the span shows spawn cost". With this bench the span
cannot show spawn cost in either direction, so the condition can be neither met nor
refuted — and a pool decision taken from these numbers would be taken from nothing.

**The fix**, and it is byte-neutral to the codec because it only changes what the
bench asks for:

```rust
param.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_FIXEDSLCNUM_SLICE;
param.sSpatialLayers[0].sSliceArgument.uiSliceNum  = threads.max(1);
```

behind a knob beside the existing `BENCH_THREADS` (say `BENCH_SLICE_MODE`), so the
`SM_SINGLE_SLICE` rows stay comparable with every span already in
`perf_baseline.md`. Adding rows rather than changing them keeps the ledger's history
readable, which is why a knob and not an edit.

**The C++ reference shows the same flat line, which is the confirmation.** The
`exit` battery's own bench rows, C++ and Rust side by side at 1 and 4 threads:

```
1080p    [1 thread] C++   336.10 fps (2.975 ms) | Rust   222.01 fps (4.504 ms)
1080p    [4 thread] C++   338.83 fps (2.951 ms) | Rust   221.85 fps (4.507 ms)
QVGA     [1 thread] C++  7556.76 fps (0.132 ms) | Rust  4679.71 fps (0.214 ms)
QVGA     [4 thread] C++  7557.98 fps (0.132 ms) | Rust  4766.73 fps (0.210 ms)
```

The **reference encoder** does not scale with thread count on this bench either —
0.8% and 0.02%. If this were a port defect the two columns would disagree; they
agree exactly. It is the parameters, on both sides, and that is what makes the fix
a bench change rather than an investigation.

**One thing this does *not* mean.** The MT path is not untested — the diffharness
`mt` preset drives all four slice modes at `t ∈ {2,4}` and 120 configurations a
sweep, and that is where F3 lives. It is untested *for speed*. The byte instrument
and the perf instrument disagree about which paths they cover, and only the byte one
was ever checked.

---

## F69 — F3's cause: the raw translation dropped `mutexSliceNumUpdate`, and the variable it guards is the one whose corruption empties the frame

**Status: FIXED at T7.B3, closed by a three-arm ablation.** Found by reading, at
step 3, while answering a bookkeeping question — "is `mutexSliceNumUpdate` dead?"
The answer is that it is dead **in the port** and live **in the C++**.

### The divergence

`codec/encoder/core/src/svc_encode_slice.cpp:1776-1791`, inside
`DynSlcJudgeSliceBoundaryStepBack`:

```cpp
if (pEncCtx->pSvcParam->iMultipleThreadIdc > 1) {
  WelsMutexLock (&pEncCtx->pSliceThreading->mutexSliceNumUpdate);
  //lock the acessing to this variable: pSliceCtx->iSliceNumInFrame
}
//tmp choice to avoid complex memory operation, 100520, to be modify
AddSliceBoundary (pEncCtx, pCurSlice, pSliceCtx, pCurMb, iCurMbIdx, kiEndMbIdxOfPartition);
++ pSliceCtx->iSliceNumInFrame;
if (pEncCtx->pSvcParam->iMultipleThreadIdc > 1) {
  WelsMutexUnlock (&pEncCtx->pSliceThreading->mutexSliceNumUpdate);
}
```

The port had the two statements and neither lock. `git log -S mutexSliceNumUpdate`
puts the omission in **`68c4f6a5 "Raw translation"`** — the same commit that
dropped `pThreadBsBuffer`'s free (T7.A3). The field survived the translation into
`SSliceThreading`, is initialised in `RequestMtResource` and destroyed in
`ReleaseMtResource`, and **has never been locked anywhere in this crate**. It was
on session B's step-3 delete list as "dead", which is how it was found.

### Why it produces exactly F3

`pSliceCtx` is `addr_of_mut!((*pCurLayer).sSliceEncCtx)` — the **layer's** slice
context, one per layer, shared by every worker on the dynamic path. So:

* `++iSliceNumInFrame` is an unsynchronised read-modify-write across threads. A
  lost increment leaves `iEncodeSliceNum != pCurLayer->sSliceEncCtx.iSliceNumInFrame`
  at `ReOrderSliceInLayer` (`svc_encode_slice.rs:3659`), which answers
  `ENC_RETURN_UNEXPECTED`; `SliceLayerInfoUpdate` propagates it and
  `WelsEncoderEncodeExt` returns it, so **the frame is emitted empty**. That is
  F3's shape in **18 of the 25** hits of its before-arm.
* `AddSliceBoundary` writes `pSliceCtx->pOverallMbMap` over the next slice's
  macroblock range and copies the slice header into the next slice — the C++'s own
  comment one line above the lock calls it "complex memory operation". Interleaved,
  it produces a boundary that is wrong rather than absent: **short** (6 of 25) or
  **longer** (1 of 25) output.

The fingerprint follows from the code: `iMultipleThreadIdc > 1` and the dynamic
path only, which is `mt` + `sm=3` + `t in {2,4}` — F3's signature, unchanged since
its first measurement.

### The ablation — three arms, one variable each

| arm | tree | runs | hits | rate |
|---|---|---|---|---|
| before (measurement 88) | `b08d6c47`, untouched | 3000 | 25 | 1/120 |
| **A** (measurement 91) | `46a66023` — pool dispatch deleted from both paths, claiming mutex gone, `mutexSliceNumUpdate` still missing | 2000 | 20 | 1/100 |
| **B** (measurement 92) | `T7.B3` — the lock restored | 2000 | **0** | — |

**Arm A settles what the charter could not assume.** The phase's ablation was
designed to test "the deleted claiming/pool machinery" as F3's cause. Against the
before-arm, arm A is 20/2000 vs 25/3000 — z = 0.61, p ≈ 0.54, **not a
distinguishable rate.** The machinery was not the cause; deleting it neither fixed
F3 nor moved it. Without arm A, a clean arm B would have been evidence for two
changes at once and for neither in particular.


**Arm B closes it.** 0 hits in 2000 runs at the same density and the same load.
Against arm A's measured 1/100 that is P = e^-20 = 2e-9; against the before-arm's
1/120, e^-16.7 = 6e-8. **F3 is closed as F69**, and the two facts that make the
verdict attributable rather than merely favourable are that arm A held the lock
constant while removing the machinery (no change) and arm B held the machinery
constant while restoring the lock (total).

### What this says about the phase's design

The charter fixed the ablation in advance as a test of "the deleted claiming/pool
machinery" (`prompts/phase7.md` §2). That hypothesis is now measured and **false**.
The machinery was worth deleting for every other reason the phase gives — five
`unsafe impl` pairs, a `static mut`, two C++-list ports, a `usize` laundering — but
it was not the defect, and a single after-arm run on a tree that had both changes
would have credited it anyway. **One variable per arm is the whole reason the
answer is usable.**

### The general lesson, and it is the third of its kind in this phase

F69 is the third defect this project has found by asking a *bookkeeping* question
rather than a behavioural one — T7.A3's leak (found while proving premise 1),
`SSliceThreadPrivateData`'s zero readers (found while listing what to delete), and
now this. All three were invisible to every gate the project owns: byte parity
cannot see a lock that is missing when the race does not fire, Miri never ran the
MT path (F12's skip), and the unit tests never take `iMultipleThreadIdc > 1`.
**"Which of these fields is dead?" is a question that finds live bugs**, because a
field that looks dead in the port and is live in the reference is a dropped
statement by definition.

---

## F70 — `InitSliceSettings` reads a slice-argument through a tag its own callee popped, on the multi-slice arm

**Status: FIXED at T7.B4.** Found by the fork/join Miri probe on its **first run**,
and not because the probe threads: because it is the first test in this crate ever
to ask for `SM_FIXEDSLCNUM_SLICE`.

`encoder_ext.rs:1309` bound `let pSliceArgument = &mut (*pDlp).sSliceArgument;` — a
`Unique` retag over the whole slice-argument struct — and held it across the
`SM_FIXEDSLCNUM_SLICE` arm, which passes **a second `&mut` to the same field**
(`:1320`) to `SliceArgumentValidationFixedSliceMode`. The second retag pops the
first, and `:1329` then reads `pSliceArgument.uiSliceNum` through the dead tag.
Miri's report names all three lines.

This is **F13's family and S29's clause**, in the one place the project had never
looked: parameter validation. The fix is the same one that family always takes —
`addr_of_mut!`, which creates no reference, so there is no tag to pop.

**What it cost to not know.** The arm is reached by every multi-slice
configuration: 369 diffharness rows a sweep include `sm=1` and `sm=2`, and every
one of them ran this UB. No byte gate can see it — the read returns the right value
on every compiler this project has used — and Miri had never executed the arm
because all four encoder probes were `SM_SINGLE_SLICE`. **Two instruments, and the
configuration axis that would have crossed them was missing from both.**

---

## F71 — the root-accessor idiom is sound for one thread and unsound for two

**Status: CLOSED at T7.C3.** The accessor half was fixed at T7.B4; the residue this
finding handed to Phase 9 turned out not to need it, and the section "Where it stops"
below is superseded by "How it actually closed" at the end. Found by the MT Miri
probe, immediately after F70, and it is the class F12's Miri skip had been hiding for
five phases.

### The mechanism

S28 established the project's root-accessor idiom: a raw cursor must carry the
*whole* allocation's provenance, so accessors derive from the root —

```rust
pub unsafe fn slice_bank_root(pCurLayer: *mut SDqLayer, kiBank: usize) -> *mut SSlice {
    let bank: &mut Vec<SSlice> = &mut (*pCurLayer).sSliceBufferInfo[kiBank].pSliceBuffer;
    bank.as_mut_ptr()
}
```

That is correct about provenance and correct for one thread. Under two it is a
**data race**: `&mut` is a `Unique` retag over the `Vec`'s own three words, and
Miri counts a retag as an access ("retags permit optimizations that insert
speculative reads or writes"). Every encoder worker resolves the *same* layer, the
*same* parameter block, the *same* function table — for every fixed slice mode all
workers resolve **bank 0** — so two workers calling the same accessor at the same
instant race on the accessor's own borrow, while neither writes anything.

**Nothing in the crate had ever run two threads under Miri**, so the whole family
was unaudited: `--skip wels_thread_pool` (F12) named the pool, and no test drove
`iMultipleThreadIdc > 1`.

### The fix, and it is mechanical

Narrow the access to the *container* from exclusive to shared, and take the buffer
pointer as a value rather than through a reborrow:

```rust
let bank = std::ptr::addr_of!((*pCurLayer).sSliceBufferInfo[kiBank].pSliceBuffer);
(*bank).as_ptr() as *mut SSlice
```

The returned pointer is bit-identical and carries the buffer's own provenance, so
what is behind it stays writable; only the access to the three-word header narrows.
For `Option<Box<T>>` and `Box<T>` slots the same idea is spelled as a pointer-sized
`ptr::read` — the layout is guaranteed, `None` is null, and reading a pointer value
retags nothing.

**Fixed here:** `ctx_dq_layer`, `ctx_ref_list`, `ctx_param`, `ctx_func_list`,
`ctx_vaa`, `ctx_mvd_cost_table`, `ctx_rc`, `ctx_ltr`, `ctx_frame_bs`,
`ctx_dq_idc_map`, `ctx_sps_array`, `ctx_subset_array`, `ctx_pps_array`;
`slice_bank_root`, `mb_list_root`, and `MbArray::root_ptr` (new, beside the
`as_mut_ptr` it does not replace — single-threaded callers keep the old one).

### Where it stops, and why the boundary is principled

The next report after those fixes is not a retag conflict but a **real shared
write**: `WelsCodeOneSlice` stamps `(*pCurLayer).sLayerInfo.sNalHeaderExt` per
slice (`svc_encode_slice.rs:2812`), from every worker. **The C++ does the same
thing** — `svc_encode_slice.cpp:1649-1655` — so it is a shared latent race in the
reference design, not a port divergence, and it is benign in practice only because
every worker writes the same value.

That cannot be spelled away. Making the layer header per-slice is a change to what
a slice *owns*, which is the context split **F67** already establishes as Phase 9's
precondition. So F71 is the third sighting of the same object: F67 found it as a
`!Sync` count, T7.B2 found it as an ordering argument, and Miri finds it here as a
race. The accessor half is fixed; the ownership half travels with F67.

**The probe is `#[cfg_attr(miri, ignore)]` with this finding cited** — gates.sh's
own rule that no skip may exist without a finding, applied to a test rather than a
module. It runs in both profiles normally.

### How it actually closed — T7.C3, and the section above was wrong about why

"That cannot be spelled away" was the wrong conclusion, and the reason it was wrong is
worth more than the fix. The residue was read as an **ownership** question — *which
object should hold the layer header* — and framed that way it really does need F67's
context split. Read as a **scheduling** question it is trivial, and the port already
had every fact needed to see that:

* the write's condition, `eSliceType == I_SLICE`, is a **frame**-level value fixed
  before the fork, so every worker takes the same arm;
* the value written is the constant `true`;
* **no worker reads the field before its own write.** Ahead of `WelsCodeOneSlice` a
  worker runs `InitOneSliceInThread`, `SetSliceBoundaryInfo` and
  `WritePrefixNalForSlice`, and none of the three touches the layer header — the
  prefix NAL's own idr argument comes from `eNalType`, not from here.

A write that is the same constant on every worker, that no worker observes before
making it, is **loop-invariant across the fork**. Hoisting it to the calling thread
immediately before the spawn is byte-for-byte what the race produced, and the race is
*gone* rather than serialised. `StampLayerIdrFlagForSliceType` is that hoist; every
single-threaded caller keeps the statement exactly where it stood, one line above its
own `WelsCodeOneSlice`, and a `debug_assert!` where the write used to be is the
contract.

Placed at the fork, deliberately, and **not** merged into `WelsInitCurrentLayer`'s
frame-level `bIdrFlag` stamp: the two disagree whenever `eSliceType == I_SLICE` with
`iFrameNum != 0`, so moving the write across the hundreds of lines between them would
be a behaviour change rather than a hoist.

Three `&mut (*pCurLayer).sLayerInfo.sNalHeaderExt` derivations went with it —
`WelsSliceHeaderScalExtInit` and both `g_pWelsWriteSliceHeader` bodies, all three
read-only — because Miri counts a `&mut` retag as a *write* for data-race purposes, so
a read-only borrow of shared layer state taken by every worker races with anything.
Same `addr_of!` idiom as the sixteen accessors, no signature moved.

**The lesson, stated because the first read cost a session's worth of deferral:** a
shared write is not automatically an ownership problem. Ask first whether it is
invariant across the fork; if it is, it belongs on the calling thread and the whole
question dissolves. What survived the hoist is a *different* class — see **F73**.

---

## F72 — the load-balancing path is half-translated: the port has the consumer of the feedback loop and not the producer

**Status: CLOSED at T7.C1** by decision **D-mt-2** (plan §7.4). Session B ruled it
fenced-and-open; the phase plan overruled that for one reason — *a default-on feature
must not ship degenerate* — and session C completed it. The ruling section below is
kept as written; "The ruling, revised" at the end is what was done.

### What the read found

The load-balancing path is a **feedback loop across frames**: frame N's per-slice
encode *times* become frame N+1's slice boundaries.

| stage | C++ | port |
|---|---|---|
| stamp `uiSliceConsumeTime` per slice | `wels_task_encoder.h` FinishTask | present (T7.B1/B2 carry it) |
| **`CalcSliceComplexRatio`** — times -> `iSliceComplexRatio` | called at `encoder_ext.cpp:4069` | **function present, never called** |
| `AdjustBaseLayer`/`AdjustEnhanceLayer` -> `NeedDynamicAdjust` | `encoder_ext.cpp:3573` | present (`encoder_ext.rs:3196`) |
| `DynamicAdjustSlicing` — ratios -> run lengths | present | present |
| `UpdateMbListNeighborParallel` re-slice | present | present (T7.B1's fork) |

**Four of the five stages are there.** The missing one is the producer of the only
input the fourth consumes. `iSliceComplexRatio` is initialised to 0 and no live path
writes it — the sole caller of `CalcSliceComplexRatio` in the crate is a unit test.

So `DynamicAdjustSlicing` computes `WelsDivRound(kiCountNumMb * 0, INT_MULTIPLY) = 0`
for every slice, clamps each to `iMinimalMbNum`, and gives the remainder to the last.
**The balance is degenerate, not absent** — nothing crashes, nothing warns, and the
encoder happily produces a bitstream with a worse slice distribution than the
reference's.

### Why nothing caught it

`bUseLoadBalancing` is `false` in both diffharness drivers (`cxx_enc.cpp:119`) and
so has **no byte coverage in any gate** — session A's brief already recorded that.
What it did not record is that the flag is **`true` by default through the public
API**: `GetDefaultParams` sets it on both sides (`param_svc.h:146`,
`param_svc.rs:294`). Any ordinary caller with `uiSliceMode = 1` and
`iMultipleThreadIdc >= uiSliceNum` takes this path.

### The byte evidence, which is what makes the ruling

T7.B0's bench knob reached the path for the first time. Two consecutive runs of the
**C++ reference alone**, same input, same parameters:

```
run 1   22834   144438   233477   5624    122715
run 2   22660   144340   233251   11357   120687
```

**The reference is not deterministic on this path**, which is what
`codec_app_def.h:579` says in words: "will change slicing of a picture during the
run-time of multi-thread encoding, so the result of each run may be different."
With `BENCH_LOAD_BALANCING=0` every row is bit-identical, `sm=1 n=4` at four threads
included.

### The ruling

**Both halves of the brief's dichotomy apply, and that is the finding.**

* The path is **incomplete in the port**, so the brief's fence applies: tagged
  `LOAD_BALANCING(incomplete: F72)` at the missing call
  (`slice_multi_threading.rs`, `CalcSliceComplexRatio`) and at the consumer
  (`encoder_ext.rs`, the `SM_FIXEDSLCNUM_SLICE` arm), with the guard cited and this
  finding named at both.
* And it **cannot be byte-gated even once completed**, because the boundaries are a
  function of measured time. So when someone does complete it, it joins the
  `CABA2_SVA_B` precedent as the project's **second expected-divergent class** — a
  recorded position, not a defect — and gets structural coverage only.

**Not completed here, deliberately.** The brief's instruction for an incomplete path
is to fence it and say which. Completing it is one call at the C++'s own site under
the guard the port already reproduces, and it is a behaviour change on a path with
no coverage; that is a decision for whoever owns the path, made with this finding in
front of them.

### One thing this does *not* mean

The rest of the multi-threaded encoder is unaffected. `bUseLoadBalancing` gates only
which task class `CreateTasks` built (now: whether `EncodeFixedSlicesForked` stamps
`uiSliceConsumeTime`) and whether the adjusters run. Every gated configuration in
this project runs with it off, and 369/369 in both profiles says so.

### The ruling, revised — D-mt-2, executed at T7.C1

The phase plan's answer to "fence or complete" is **complete**, and the argument is
one sentence: *a feature that is on by default must not be left degenerate.* Fencing
is right for a path that is absent; this one is present, reachable through the public
API with no flag to set, and quietly producing a worse slice distribution than the
reference — the failure mode a fence does nothing about.

**What landed.** One call, at the C++'s own site: `encoder_ext.cpp:4064-4073`, the end
of the per-layer body in `WelsEncoderEncodeExt`, after the padding block and
immediately above the `eLastNalPriority` stamp, under the C++'s own four-term guard —
which is the same guard the consumer arm a thousand lines up already reproduced. Both
`LOAD_BALANCING(incomplete: F72)` fences retire, and their comments now say what the
path *is* rather than what it is missing.

**The expected-divergent class is recorded, and it is the project's second.** The
evidence above (two C++ runs, five differing byte counts) is not going to change:
frame N+1's boundaries are a function of frame N's measured times, so there is no
reference to compare against and there never will be. The class joins `CABA2_SVA_B`
in plan §1.5 so a regenerated annotation set cannot lose it.

**Coverage is structural, because differential is impossible.**
`load_balancing_completes_frames_with_sane_slice_counts` drives the real encoder with
the flag **on**, four threads, four slices, four frames — the guard's own
configuration — and asserts shape and never bytes: four frames out, none empty, an IDR
then three inter frames, and `vcl_nals == 4` every frame, since `DynamicAdjustSlicing`
moves macroblocks *between* slices and never changes how many there are. 256x192 is
forced rather than chosen: `MIN_NUM_MB_PER_SLICE` is 48, four slices need 192
macroblocks, and below that `SliceArgumentValidationFixedSliceMode` silently rewrites
the request while every other assertion still passes — which is exactly how a probe
ends up green on the path it was written to leave.

**Hard rule 6 holds.** Behaviour changes only where `bUseLoadBalancing` is on, and no
gated configuration runs it: both diffharness drivers pin it off, every byte-asserting
probe pins it off, and the sweeps stayed 369/369 in both profiles across the commit.

---

## F73 — the workers reach the reconstruction picture through `&mut`, and that is a family, not a site

*Phase 7 session C, 2026-08-21, found by the MT Miri probe on the run immediately
after **F71** closed — the fourth defect that probe has produced by being run.*

**Status: OPEN, owned by Phase 9 (F67's context split).** The MT probe's
`#[cfg_attr(miri, ignore)]` cites this finding now; it retires with it.

### What it is

With F71's shared write hoisted out of the fork (T7.C3), Miri's next answer on the
same probe is a different class:

```
Data race detected between (1) retag write on thread `unnamed-2`
                       and (2) non-atomic write on thread `unnamed-3`
  encoder::encoder_context::SRefList::pic_mut       encoder_context.rs
  encoder::svc_encode_slice::layer_dec_pic_mut      svc_encode_slice.rs
  encoder::svc_base_layer_md::WelsMdIntraInit       svc_base_layer_md.rs
```

Every worker resolves the layer's **reconstruction picture** to stamp its macroblock
cursors, and resolves it the only way the port has: `layer_dec_pic_mut` ->
`SRefList::pic_mut` -> `&mut SPicture`, then `SPicture::planes(&mut self)` for the
plane roots. Two workers doing that at the same instant race on the *accessor's own
borrow* — the `Unique` retag over the picture's header — while what they go on to
write, their own disjoint macroblock rows, does not overlap at all.

**The aliasing is in how the port reaches the picture, not in what it writes.** That
is F71's mechanism exactly, one level down, and it is why F71's fix does not reach it:
F71's sixteen conversions were accessors that hand back a *pointer*, and this family
hands back a `&mut SPicture` whose `planes()` needs `&mut self` because `data_ptr`
must produce `*mut u8` out of an owned plane.

### Why it is a family and not a site

Measured, not estimated: **32 `planes()` call sites** and **68 `_mut` picture-accessor
calls** across `wels_preprocess.rs`, `ref_list_mgr_svc.rs`, `svc_mode_decision.rs`,
`svc_base_layer_md.rs`, `deblocking.rs`, `encoder_ext.rs` and `picture.rs`. Six of
those sites are inside the fork; the rest are not, and the shared ones cannot be
converted without deciding what a `&self` `planes()` hands back — which is a change to
what a picture *owns* and how a caller names a writable plane.

A raw twin used only by the six worker-tree sites would move the report one call
deeper on the next Miri run, not close it. That is the shape of every accessor family
this project has converted, and the ones that went well went **root-down and whole**.

### Why Phase 9

`SPicture`'s ownership is exactly what F67 names as the context split's content, and
F66 (Phase 6 session J) already established that the conversion has to be run as a
*precondition* with its detector rather than as a post-mortem. This is the same object
a fourth time: F67 saw it as a `!Sync` count, T7.B2 as an ordering argument, F71 as a
retag on the layer, and Miri sees it here as a retag on the picture.

### What it does not mean

No byte moves and no gate is affected. The workers' writes are disjoint by
construction — this is the seam's part 1, and 369/369 in both profiles across every
commit of this phase says the output does not see it. What is unaudited is the
*aliasing model*, which is what the MT probe exists to check and why the skip has a
finding attached rather than a shrug.

---

## F74 — perfpair's encode parser has been blind to every multi-slice row since the axis existed

*Phase 7 session C, 2026-08-21, found by running step 5's span: the null run printed
an empty encode table and exited 0.*

**Status: FIXED at T7.C9.**

`_ENCODE_MS` was `\[(\d+) thread\]` — a literal `]` straight after the count.
`BENCH_SLICE_MODE` (T7.B0, F68's knob) makes the bench print `[1 thread sm=1 n=4]`, so
from the moment the slice-mode axis existed the parser matched **nothing** on any
multi-slice run, and `report()` printed a header with no rows under it.

**It is F68's own shape, one instrument further out.** F68 was a bench axis that
silently measured the wrong path; this is a parser that silently measures no path. Both
are invisible unless someone reads a table for the rows it does *not* have, and this
one cost nothing only because session B ran no span and this is the first span since
the axis landed.

Fixed twice over, because the parse bug is the smaller half:

* the axis label is captured into the row key, so `sm=1 n=4` and `sm=3` are separate
  rows rather than colliding on one;
* **a parser that matches nothing is now a refusal.** S17 already requires a missing
  ffmpeg to be loud rather than a quiet skip, and gates.sh's Miri step already refuses
  a run of zero tests. An empty encode table is indistinguishable from a green one in
  the report, so it gets the same rule.

The general form is the one this project keeps re-learning and should stop paying for:
**an instrument that can produce an empty result must refuse, not report.** Three
sightings now — gates.sh's Miri step taking `tail`'s exit status (Phase 3), F68's
thread axis, and this.

---

## F75 — the unscoped Miri gate became unrunnable at the exact moment the project earned zero skips

*Phase 7 session C, 2026-08-21, found by running the phase's own `exit` battery — it
died on the Miri step after 25 minutes of green gates.*

**Status: FIXED at T7.C10.**

```
=== miri (--lib (whole library, no skips))
rust/tools/gates.sh: line 402: MIRI_SKIPS[@]: unbound variable
```

macOS ships **bash 3.2**, where `"${arr[@]}"` on an *empty* array is an
unbound-variable error under `set -u` — which `gates.sh` sets, deliberately and with a
comment saying why. `MIRI_SKIPS` is the list of `--skip` arguments the Miri `--lib`
step passes, and it is empty exactly when the project has **no Miri skips left**.

**Every phase exit before this one ran with at least one entry in it**, so the
expansion was never empty and the bug never fired. T7.B4 deleted the last skip — F12's
`--skip wels_thread_pool`, with the module it named — and the very next unscoped run,
this phase's exit, died instead of running Miri.

So the gate's own success condition disabled the gate. Had this landed without the
`exit` battery being run, the phase would have closed on a Miri step that never
executed, and the next session to run it would have inherited an unbounded backlog —
which is F18's lesson ("the backlog behind a skip is not confined to the code the skip
was written for") arriving through a door nobody was watching.

**Fixed at the expansion**, with the bash 3.2 idiom
`${MIRI_SKIPS[@]+"${MIRI_SKIPS[@]}"}`, and **not** by keeping a dummy entry in the
array: an empty skip list is the correct state and must stay expressible.

**This is the fourth sighting of one shape**, and the pattern is now explicit enough to
state as a rule: *an instrument's empty case is a case, and it is the one nobody
tests.* F17 — `gates.sh` taking `tail`'s exit status, so every `PASS miri` for two
phases was unconditional. F68 — a bench thread axis that silently drove the
single-threaded path. F74 — a parser that matched no encode row and printed an empty
table. F75 — an argument list that was correct only while non-empty. Three of the four
were in the *measuring* apparatus rather than the code, and all four were invisible
until someone looked at a result for the part that was missing.
