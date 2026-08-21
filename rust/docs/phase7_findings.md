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

**One thing this does *not* mean.** The MT path is not untested — the diffharness
`mt` preset drives all four slice modes at `t ∈ {2,4}` and 120 configurations a
sweep, and that is where F3 lives. It is untested *for speed*. The byte instrument
and the perf instrument disagree about which paths they cover, and only the byte one
was ever checked.
