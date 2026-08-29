# Execution plan — final push to safe Rust

*Drafted 2026-08-27 at `c3ff096e` from a fresh census of `rust/crates/openh264-rs/src` (code
and `rust/tools` instruments only, independent of the prior planning corpus). This is the
operative plan for eliminating every raw pointer and every `unsafe` fn/block outside the
C-ABI island, with two recorded exceptions.*

---

## 1. Goal and end state

Convert the remaining unsafe Rust in `src/` to safe Rust. "Done" means all of:

1. **`#![forbid(unsafe_code)]`** at the top of every file under `src/` **except**:
   - `src/api/codec_api.rs` + `src/api/abi_guard.rs` — the C-ABI island (see §2),
   - `src/encoder/rec_view.rs` and `src/encoder/slice_multi_threading.rs`, which carry
     `#![deny(unsafe_code)]` plus **exactly one** `#[allow(unsafe_code)]` each (see D1).
2. **Zero raw-pointer types** (`*mut`/`*const`) in fields, parameters, returns, or locals
   outside `src/api/`.
3. **The C-API is byte-for-byte preserved**: the exported symbol set (5 in
   `api/codec_api.rs` + 2 version exports in `encoder/wels_encoder_ext.rs`), every
   `repr(C)` struct layout (`tools/abi_sizes.sh` identical), and behavior under upstream's
   own `test/api` gtest suite (ratcheted against `tools/abi_harness/gtest_known_failures.txt`).
4. **Bit-exact output parity**: diffharness sweeps byte-identical in all modes, both
   profiles; decoder conformance SHAs unchanged.
5. **Gates green at exit level** (`rust/tools/gates.sh exit`): full test suite, Miri over
   `--lib` *and* the differential integration tests, both benches within the perf budget
   (§6), ABI export list + external dlopen harness + gtest.
6. The unsafe ratchet baseline reaches its floor and is **pinned**: the census allowlist
   reduces to the api files plus the two D1 lines, so any new `unsafe` anywhere else fails CI.
7. `src/lib.rs` drops its crate-wide `allow(unused_unsafe, unsafe_op_in_unsafe_fn, …)` —
   those lints become meaningful again once the island is all that's left.

**Scope note**: this covers `src/` (the library). `tests/`, `benches/`, and
`tools/diffharness` deliberately keep FFI unsafe — comparing against the C encoder through
the C ABI is their job.

### Recorded decisions

- **D-scope-5 (user, 2026-08-28): the screen-content family was descoped, then REVERSED
  the same day — it converts**, user-directed, in S6. The ownership shape is recorded with
  the reversal in `safety_refactor_plan.md`'s decisions ledger: full ownership (`Vec`
  arenas + index tables), `PerformFMEPreprocess` takes its buffer by value (zero call
  sites), and the checkpoint ships the family's first-ever referee — an invariant unit
  test on hand-built storage. Deletion stays rejected (the search path is
  public-API-reachable, F233).
- **D1 (user, 2026-08-27): keep the two MT `unsafe impl` lines.**
  `unsafe impl<T: Copy> Sync for SharedCells<T>` (`rec_view.rs:152`) and
  `unsafe impl Send for SliceJobHandle` (`slice_multi_threading.rs:1318`) stay as audited
  exceptions. They are the keystone of the disjoint-write MT scheme (shared `Cell` view
  over the reconstruction picture, fork via `std::thread::scope`). Their proof obligation
  is the pair of Miri data-race probes (fork/join and mid-row-boundary, in
  `svc_encode_slice.rs`), which stay in the suite permanently. The safe alternative
  (atomic storage) was considered and rejected: it buys nothing Miri doesn't already
  check, and costs a storage refactor plus a perf measurement.
- **D2: the api island is fixed, not shrinking-by-obligation.** Interior helpers in
  `api/codec_api.rs` (the `BufferingReadyPicture` family etc.) may be de-unsafed
  opportunistically in E1, but no milestone blocks on it. The island's budget is whatever
  the census records at E2 pin time.
- **D3 (user, 2026-08-27; amended at adoption the same day: bigger): sessions sized for a
  large-context agent, one per stage.** The work runs as **4 large sessions** (S1–S4, §5),
  each a long continuous agent run covering a full stage (S2 covers two adjacent ones).
  The original half-day milestones (A1…E3) survive as **in-session checkpoints**: each
  checkpoint still lands as its own gated commit, so bisection granularity, ratchet
  cadence, and SHA-divergence localization are unchanged — only the hand-off overhead
  between them is deleted. **Checkpoints within a session are ordered
  drop-from-the-end** (the Phase 9 discipline that made large sessions safe): if a
  session must stop, it stops at a checkpoint boundary and the tail becomes the next
  session's front — never a half-landed checkpoint. Hand-off briefs are written at
  session closes only.

---

## 2. What stays unsafe, exactly

The C-ABI boundary in `src/api/`:

- The 7 `#[unsafe(no_mangle)] pub unsafe extern "C"` exports: `WelsCreateSVCEncoder`,
  `WelsDestroySVCEncoder`, `WelsCreateDecoder`, `WelsDestroyDecoder`,
  `WelsGetDecoderCapability`, plus the two version exports in `wels_encoder_ext.rs`.
- The ISVCEncoder/ISVCDecoder vtable emulation and its raw `this` pointers.
- Raw-pointer members of the public `repr(C)` structs (`SFrameBSInfo`, `SSourcePicture`,
  `SBufferInfo`, …) — they are the ABI.
- The trace-callback plumbing: the user-supplied `*mut c_void` context is irreducibly
  raw. Today it lives in `common/wels_trace.rs` (`pLogCtx`); E1 wraps it in a newtype
  whose construction and invocation happen only in `src/api/`, so `common/` itself can
  flip to forbid.

Everything else converts.

---

## 3. The frontier, measured (2026-08-27)

Live counts at `c3ff096e`; the ratchet baseline (`tools/unsafe_baseline.json` @`2bb038a9`)
agrees within a session's drift.

| Region | unsafe fn | unsafe blocks | raw-ptr mentions | status |
|---|---|---|---|---|
| `api/` (stays) | ~55 | 75 | ~271 | the island |
| `encoder/` | ~415 | ~156 | ~880 | **the work** |
| `decoder/` | 0 | 11 | ~13 | residue only |
| `common/` | ~17 | ~24 | ~46 | `deblocking_common` + trace |
| `processing/` | 14 | 1 | ~12 | thin wrappers |
| `safe/` | 0 | 0 | 0 | done, forbid |

**656 `#[allow(unsafe_code)]` sites total; 45 in api; ~611 to eliminate.**

Three inventories drive the ordering:

**(a) 20 raw accessors** mediating the `sWelsEncCtx` god-struct (call-site counts include
defs/docs; measured by grep):

| accessor | sites | | accessor | sites |
|---|---|---|---|---|
| `ctx_param` | 246 | | `ctx_dq_layer` | 15 |
| `ctx_func_list` | 106 | | `ctx_sps_array` | 15 |
| `ctx_vaa` | 79 | | `ctx_pps_array` | 15 |
| `ctx_rc_at` | 60 | | `ctx_subset_array` | 13 |
| `ctx_ref_list` | 36 | | `ctx_frame_bs` | 9 |
| `ctx_frame_bs_cur` | 22 | | `ctx_rc` | 6 |
| `ctx_mvd_cost_table/origin` | 10 | | `rc_gom_fg_blocks/sad` | 7 |
| + 4 slice-local (`ctx_sps`, `ctx_pps`, `ctx_ref_pic`, `ctx_pic_ref` in `svc_encode_slice.rs`) | | | | |

≈640 sites. The flip method is proven: the `ctx_ltr` family (28 call sites; 83
compiler-adjudicated conflict sites) landed in one half-day block with zero split
borrows needed.

**(b) 10 unsafe fn-pointer type aliases** in `wels_func_ptr_def.rs` (of 14): the MD family
(`PIntraFineMdFunc`, `PInterFineMdFunc`, `PInterMdFirstIntraModeFunc`, `PAccumulateSadFunc`,
`PInterMdBackgroundDecisionFunc`, `PMdBackgroundInfoUpdateFunc`,
`PInterMdScrollingPSkipDecisionFunc`, `PSetScrollingMv`, `PInterMdFunc`) plus
`PCavlcParamCalFunc`. The conversion template exists — the intra-pred families already
take `&mut [u8; N]` + `&RecCursor`.

**(c) 51 raw-pointer struct fields**, in five families:
1. Roving per-MB pixel cursors: `pEncMb/pRefMb/pCsMb: [*mut u8; 3]` (`SMbCache` in
   `encoder_context.rs`, mirrored in `svc_mode_decision.rs`) → plane-id + byte-offset
   pairs resolved through `RecPicView`/plane cursors at use.
2. Ctx-owned singletons: `pSliceThreading`, `pVpp`, `pOut` → `Option<Box<T>>`;
   `mutexSliceNumUpdate: *mut Mutex<()>` → owned `Mutex<()>`.
3. Screen-content feature arenas: `SScreenBlockFeatureStorage`
   (`pFeatureOfBlockPointer`, `pLocationOfFeature: *mut *mut u16`,
   `pFeatureValuePointerList`, …) and its ME mirrors → owned `Vec`s + index tables
   (pointer-of-pointer becomes row-offset table).
4. Preprocess views: `pPixel[3]`, `pCurY/pRefY`, `pBackgroundMbFlag`, `uiRefMbType`,
   `pGomComplexity`, `pStaticBlockIdc`, … → pool handles / indices into ctx-owned storage.
5. Per-slice aliases: `pRestoreBuffer`, `pCsData/pEncData`, `pRefList`, `pSrcPool`
   (`svc_encode_slice.rs`) → handles/offsets resolved per call.

---

## 4. Ground rules (every session)

1. **The landing unit is the checkpoint, not the session** (D3): every checkpoint lands
   as its own commit with `gates.sh commit` green (~15–19 min); several checkpoints land
   per session. A session closes with `family`; stage closes run `session`-level;
   `exit` only at E3. Never batch two checkpoints into one landing — the per-checkpoint
   gate is what keeps a SHA divergence bisectable to one family of changes.
2. **Ratchet monotone**: counts in touched files strictly decrease; regenerate
   `unsafe_baseline.json` downward at every session close. Any increase anywhere is a
   rebaseline requiring a written reason.
3. **Bit-exactness is a stop-the-line invariant.** A diffharness SHA divergence is a bug
   in the session's change, full stop. No "close enough" for a codec.
4. **ABI frozen**: nothing in `src/api/` changes signature or layout; `repr(C)` types are
   untouchable. `abi_sizes.sh` at stage closes.
5. **Perf budget** (§6) checked at stage closes, not per session.
6. **Split-borrow protocol**: when a caller holds two accessor results live, prefer
   (in order) — reorder the uses; a combined accessor returning disjoint `&mut` fields
   from one borrow; copy-out/write-back only where the C semantics provably allow it.
   Fn-pointer dispatch conflicts dissolve by copying the (Copy) fn pointer to a local
   before building the argument borrows.
7. **Miri probes are load-bearing** for anything touching the MT seam: the fork/join and
   mid-row probes run in every `session`-level gate that touches `svc_encode_slice.rs`,
   `slice_multi_threading.rs`, or `rec_view.rs`.

---

## 5. Sessions and checkpoints

Sizing (D3, amended): **4 large sessions**, each a continuous agent run covering a full
stage and landing its checkpoints as separate gated commits, ordered drop-from-the-end.
Checkpoint estimates include the de-unsafe cascade (stripping `unsafe` from signatures
whose bodies became safe, and from their callers' call sites) — the cascade is each
checkpoint's second half, enforced by the ratchet.

### The session map

| Session | Checkpoints | Scope | What's at zero afterwards |
|---|---|---|---|
| **S1** ✅ **CLOSED 2026-08-27** — brief [`prompts/safeplan_s1.md`](prompts/safeplan_s1.md) | **A1–A4 landed**; A5–A7 roll to S2 | **The accessor layer's first half** (~230 sites): small fry → `ctx_rc_at` → `ctx_ref_list` → paraset trio + `ctx_frame_bs_cur` + `ctx_sps`/`ctx_pps`. `ctx_dq_layer` deferred to D2–D3 (F210). Four gated commits, `session` gate green at the close | `ctx_mvd_cost_*`, `ctx_rc`, `ctx_rc_at`, `ctx_frame_bs`, `ctx_frame_bs_cur`, `ctx_ref_list`, the paraset trio, `ctx_sps`, `ctx_pps`, `rc_gom_*`; allows 627 → 613 (F219) |
| **S2** ✅ **CLOSED 2026-08-27** — brief [`prompts/safeplan_s2.md`](prompts/safeplan_s2.md) | **A5–A7 landed**; B1–B3 roll to S3 | **The accessor layer's tail — stage A is complete**: `ctx_vaa` → `vaa`/`vaa_mut` (+ the screen-content downcast named once, F213), `ctx_func_list` → the F212 flip, `ctx_param` → `param`/`param_mut`/`param_opt` (258 sites). Three gated commits, `session` gate green at the close. B1 re-priced as an **MT-seam** checkpoint (F217) | every ctx accessor except the **DQ-layer family**, which F210 defers to D2–D3; allows 613 → 612 |
| **S3** ✅ **CLOSED 2026-08-27** — brief [`prompts/safeplan_s3.md`](prompts/safeplan_s3.md) | **B3, B2, B1 landed — stage B complete**; D1–D4 roll to S4 | **Owned fields and the MT lifecycle — stage B, in reverse order.** S2's brief recommended opening with D2's layer instead; S3 measured the basis for that recommendation and it did not hold (**F221**: 38 of 40 layer-parameter bodies are fork-reachable, not 11), so stage B went first and, within it, the two files with **zero** fork-reachable bodies (B3, B2) went ahead of the one MT-seam checkpoint (B1, F217). F217's open question is answered by measurement, not design: `slice_bs_buffer`'s `pOut` arm is **main-thread-only** (probe + 8 previously-unswept MT/single-slice configs + 895-case sweep), so neither `SharedCells` nor a fence was needed | ctx raw singletons (all four owned); `encoder_ext.rs` + `wels_encoder_ext.rs` raw roots; `WelsMutexInit`/`Destroy` deleted |
| **S4** ✅ **CLOSED 2026-08-27** — brief [`prompts/safeplan_s4.md`](prompts/safeplan_s4.md) | step 0 + **C1, C2, C3, C4a, C-cascade, C-params, D4a, E2a landed**, plus F226's fix; **roll to S5: C4b, C5, C6, D1–D3, D4b, E1, the rest of E2, E3** | **The probe debt paid and stage C opened.** The inherited MT-probe duty ran to a GREEN verdict (F225) — B1's fix is verified, F223's tail closed, and the pair now has a baseline file and a runner with a tripwire. Stage C's convertible surface: the fn-pointer table (C1), `SPicData`'s nine roving cursors (C2), every non-ctx raw parameter in the mode-decision pair (C3) and the MD helpers (C4a). Then **F216's escrow paid out** — a compiler-driven fixpoint retired 87 `unsafe fn` signatures and 72 allows in one pass. After the close-docs commit, two more landings: **D4a** (the decoder's deblocking shim table deleted, `deblocking_common.rs` sealed) and **E2a** (34 already-safe files sealed — 41 `forbid` files total; **no sweep verdict of its own**: attributes only, covered by S5's first sweep) | `ctx_mvd_cost`'s writer, the roving pixel trios, the fn-pointer table's ten aliases, the decoder's deblocking shims; allows 614 → **519** |
| **S5** ✅ **CLOSED 2026-08-28** — brief [`prompts/safeplan_s5.md`](prompts/safeplan_s5.md) | **C4b, D1, C6a, C6b, C6c, C6d, E2b, D2a, D2b landed** — nine gated checkpoints; **C5 stopped for a ruling (F229) — ruled next: D-scope-5, descoped, then reversed the same day: it converts in S6**; **D2c (the `&SDqLayer` flip) attempted and reverted — F236 has the split it needs**; C6's move-memory pair, D3–D4, E1, the rest of E2 and E3 roll to S6 | **Stage C's hard case, and the cascade the brief pointed at.** C4b took the MVD-cost family: a *biased* pointer — one parked mid-table because `COST_MVD` indexes with a signed MVD — became `MvdCostCursor` (the whole table plus the index the pointer held), and both `COST_MVD`s went safe. The lifetime design cost **four** compile errors tree-wide, not the cascade the brief predicted, because the search typedefs already took `&mut SWelsME` and elision makes them higher-ranked. **F228** redirected its shape mid-flight: a `&self` context accessor is a *whole-context* retag that races any worker's inline write, so the table is derived field-precisely and `mvd_cost_origin` is retired. D1 narrowed the entropy coder's stash/position family off `*mut BsWriter`/`*mut SDynamicSlicingStack`/`*mut SCabacCtx` (14 call sites enumerated before the null tests were deleted). C6a converted the preprocessor's padding family and the compiler then showed `DownsamplePadding` had been building **two write-only `SPixMap`s per call**. E2b ran the de-unsafe fixpoint tree-wide — **F230** records that the obvious form of it *diverges*, and that the form which converges strips only declarations whose signature carries no raw pointer. **F231**: 70 `# Safety` clauses document pointers their functions no longer take. Then the preprocessor's out-parameter family (C6b), and the move that paid best of all: **C6c** made `SPicture::pScreenBlockFeatureStorage` — a field *nothing in the tree ever assigns* — an owned `Option<Box<..>>`, which cost nothing (niche-optimised, `assert_size!` unchanged) and freed **ten** allows behind it, because it was the sole reason `SetUnref` was `unsafe` and `SetUnref` has 16 call sites; `ref_list_mgr_svc.rs` went 31 → 27 without a hand edit. **C6d** carried that through `SWelsME`, which now has **no raw field at all**, and turned up **F233**: a live null dereference in `SetFeatureSearchIn` on the `SCREEN_CONTENT_REAL_TIME` path. Then **the DQ-layer storage move**, which is D2/D3's stated prerequisite: **D2a** made the partition counters atomics, **D2b** boxed the slice-buffer banks off the layer's own bytes, and together they leave **zero** fields of `SDqLayer` written from inside the fork — measured, not asserted. Each has a two-thread Miri probe whose *control* was run and seen to fail (**F234**: at 8 rounds the control was silent and the probe was blind; 200 makes it a referee). **F235** records why the two halves needed different fixes and how Miri's wording says which. The flip itself (**D2c**) was attempted and reverted: `current_layer` returns null, so a straight substitution moves that obligation to 125 call sites and compiles — **F236** carries the three-way split it actually needs. Nine gated commits, `session` on every one, sweeps 583/583 in both profiles throughout | the MVD-cost family, the entropy stash family, the preprocessor's padding and out-parameter families, `SPicture`'s and `SWelsME`'s feature-storage pointers; allows 519 → **460**, `unsafe_fn` 470 → **415**, `raw_ptr` 1009 → **949**; `SDqLayer` carries no fork-written field |
| **S6** ◀ next | C5's ruling, **D2c (the flip — F236 is the recipe)**, C6's move-memory pair, D3–D4, E1–E3 | Screen-content arenas (blocked on F229's ruling), the preprocess ref-list and pix-map families, the slice core, the residue, the lint flip and the exit battery. **F232 is the map**, and C6c already collected part of what it predicted; `current_layer`, `ctx_param_raw`, `ctx_vpp_raw` and `slice_in_layer` are the edges still holding `ref_list_mgr_svc.rs`'s remaining 27 | everything — §1 acceptance list |

Checkpoint detail follows, grouped by stage.

### Stage A — flip the context accessors (S1–S2)

The enabler: ~200 of the remaining unsafe fns are unsafe *only* because they call these.
Run `tools/phase9_ctx_join.py` (the join/fork-split analyzer) before each to map which
callers hold multiple accessors simultaneously.

| # | Scope | Sites | Cascade collapses |
|---|---|---|---|
| A1 | `ctx_mvd_cost_table/origin`, `ctx_rc`, `ctx_frame_bs`, `rc_gom_*` — the small fry, re-validating the method | ~32 | starts `rc.rs` |
| A2 | `ctx_rc_at` | 60 | finishes `rc.rs` (−55 unsafe fn) |
| A3 | `ctx_ref_list` (**`ctx_dq_layer` split off — F210**: the fork *writes* the layer, so the accessor can return neither `&mut` nor `&`; it converts with the layer's storage in D2–D3, and the join tool reads 0 LIVE, so there is no joint-hold to dissolve) | 36 | `ref_list_mgr_svc.rs` — but see F209: the file's `unsafe fn` collapse waits on `ctx_param` |
| A4 | `ctx_sps_array`/`ctx_subset_array`/`ctx_pps_array` + `ctx_frame_bs_cur` | 65 | `au_set.rs`, `paraset_strategy.rs` |
| A5 ✅ | `ctx_vaa` → `vaa`/`vaa_mut`/`vaa_ptr`, plus `vaa_ext`/`vaa_ext_mut` for the **16** (not six) screen-content downcasts, whose two disagreeing declarations F213 records | 86 textual | `raw_ptr` −20 |
| A6 ✅ | `ctx_func_list` → `func_list`/`func_list_mut`, F212's flip taken. Moves nothing the ratchet counts and F214 says why; two derivations survive as `ctx_func_list_raw` | 121 textual | none — F214 |
| A7 ✅ | `ctx_param` → `param`/`param_mut`/`param_opt` + two combined accessors; 26 per-layer cursors keep the slot-read root as `ctx_param_raw` (**F215**: a `&mut self` accessor is a fresh whole-struct `Unique` *per call*) | 258 textual | **none — F216**: the straggler is `current_layer`, not `ctx_param` |

**Exit criteria**: zero `pub unsafe fn ctx_*`/`rc_*` accessors; `rc.rs`,
`ref_list_mgr_svc.rs`, `au_set.rs`, `paraset_strategy.rs` at or near zero allows;
family gate green.

**Met as amended, at S2's close (2026-08-27).** The first clause holds for the
eight accessors stage A owns, with **three named raw survivors** — `ctx_ref_list_raw`
(A3), `ctx_func_list_raw` (A6), `ctx_param_raw` (A7) — each with a written reason
and a counted call set; F211/F215 are the record. The **second clause does not
hold and could not**: those files' allow counts are unchanged, because every one
of their bodies also calls the **DQ-layer family** (`current_layer` 158 sites,
`ctx_dq_layer` 22, `layer_pps` 30, `layer_ref_pic` 45, and five more), which F210
defers to D2–D3. The criterion was written against F209's expectation that
`ctx_param` was the straggler; **F216 measures that it is not**, and stage A's
cascade is held in escrow until the layer's storage moves. The tracking number
moved 627 → 612 across the whole stage, which is the honest figure for it.

### Stage B — owned fields and the MT lifecycle (S3)

| # | Scope |
|---|---|
| B1 | `pOut`/`pVpp`/`pSliceThreading` → `Option<Box<T>>`; `mutexSliceNumUpdate` → owned `Mutex<()>` (deleting `WelsMutexInit/Destroy` and the `Box::into_raw` dance); rewire `RequestMtResource`/`ReleaseMtResource` (`*mut *mut sWelsEncCtx` → `&mut`) and the alloc/free paths |
| B2 | `encoder_ext.rs` sweep (38 unsafe fn): init/teardown, param plumbing — mostly cascade after A7+B1 |
| B3 | `wels_encoder_ext.rs` (16 fn / 73 raw): frame-loop orchestration; the 2 version exports stay |

**Exit criteria**: no raw ctx-singleton fields; `slice_multi_threading.rs` down to its MT
residue; session gate + Miri probes green.

**Met at S3's close (2026-08-27), with the Miri-probe clause NOT met.** The four singleton
fields are owned (`pOut`/`pVpp`/`pSliceThreading` as `Option<Box<T>>`,
`mutexSliceNumUpdate` as an owned `Mutex<()>`); `WelsMutexInit`/`WelsMutexDestroy`
and their `Box::into_raw` dance are deleted and `with_wels_mutex` is a safe fn.
Every checkpoint landed on `family` (sweeps 583/583 in both profiles) plus the Miri
encode shards run **inside** the checkpoint per F215. The unmet clause is "**Miri probes green**":
S3 gated at `family`, not `session`, per checkpoint, and the §4.7 MT probes were
deferred to the session close by the user's direction. **F223 records what that
cost and what it leaves open**: a probe run found a worker-vs-worker data race in
B1's second draft — a defect no byte gate and no single-threaded Miri shard can
see — and the fix that answers it never got a completed probe run of its own.
The fix is backed by static classification (`phase9_forksplit.py --why`, plus the
tool's own in-fork/ST split of the two accessors) and by `family` on the committed
tree, but **not by the probes**. That obligation transfers to S4 as its first
duty, the way F220 transferred it to S3. The residue B1 leaves for D4 is unchanged.

### Stage C — pixels, MD/ME, arenas, preprocess (S4–S5)

| # | Scope |
|---|---|
| C1 | The 10 unsafe fn-pointer aliases → safe signatures (types, every impl, every dispatch). Mechanical but wide; lands as one change so the table is never half-typed |
| C2 | Roving trios `pEncMb/pRefMb/pCsMb` → plane offsets; `svc_encode_mb.rs` (9 fn / 40 raw) |
| C3 | `svc_mode_decision.rs` + `svc_base_layer_md.rs` (57 fn) onto plane cursors |
| C4 | `svc_motion_estimate.rs` (28 fn / 52 raw) + `md.rs` (14) — SAD/search paths; watch the perf budget here |
| C5 | Screen-content feature arena → **owned** `Vec` arenas + index tables (`picture.rs` storage struct, the `SFeatureSearchIn` mirrors, the three dispatch typedefs and their `_c` bodies); ships the family's first-ever referee (invariant unit test on hand-built storage). *Briefly descoped and reinstated the same day — D-scope-5 and its reversal, 2026-08-28* |
| C6 | `wels_preprocess.rs` remaining fields + fns (44) + `processing/` wrappers (14) |

**Exit criteria**: encoder pixel land raw-free; benches within budget; family gate green.

### Stage D — writers and the slice core (S6–S7)

| # | Scope |
|---|---|
| D1 | Bit writers onto `safe::bits`: `svc_set_mb_syn_cavlc/cabac`, `set_mb_syn_cabac`, `vlc_encoder`, `nal_encap`, `encode_mb_aux`/`decode_mb_aux` |
| D2 | `svc_encode_slice.rs` part 1 (81 fn / 117 raw, the biggest file): per-slice aliases → handles (§3c-5), slice construction/teardown |
| D3 | `svc_encode_slice.rs` part 2: the encode loop, dynamic slicing, `svc_enc_slice_segment.rs`; targeted second-scale worker-race probes gate every landing (D-gate-7 — the full fork pair moved to E3) |
| D4 | `slice_multi_threading.rs` residue (25) + `common/deblocking_common.rs` raw-wrapper retirement (safe kernels already exist; delete the raw twins) + `encoder/deblocking.rs`, `mc.rs`, `copy_mb.rs` |

**Exit criteria**: `encoder/` allows ≈ 0 outside the D1 impl file; session gate green.

### Stage E — freeze and exit (S8)

| # | Scope |
|---|---|
| E1 | Decoder residue (`decoder/picture.rs` — largely retired already, re-measure; stray raws in `decoder_core`/`decoder_context`/`pic_queue`); `wels_trace` `pLogCtx` newtype confined to `src/api/`; opportunistic api-interior shrink (D2) |
| E2 | **The lint flip**: `#![forbid(unsafe_code)]` on every non-api file except the two D1 files (deny + exactly 1 allow each); delete `lib.rs` blanket allows; reduce `tools/census_allowlist.txt` to the island + 2 lines; pin the ratchet at the floor |
| E3 | **Exit battery**: `gates.sh exit` — ABI export list, dlopen harness, upstream gtest vs known-failures, full Miri incl. differential tests, both benches vs `perf_baseline.md`, both-profile sweeps. Plus buffer for whatever it surfaces |

**Exit criteria**: §1 acceptance list, item by item.

---

## 6. Perf budget

Bounds-checked indexing replaces pointer arithmetic in hot MB loops (ME/SAD, writers,
deblocking). Budget: **encoder wall-clock within 3% of the current `c_vs_rust_bench`
baseline** (recorded in `perf_baseline.md`); decode bench likewise. A stage close that
exceeds it stops and fixes shape first (chunked iteration, hoisted slices, kernels kept
autovectorizable) — never by trading away `overflow-checks`/`debug-assertions` in dev
(see the manifest's standing warning) and never by re-adding `unsafe`.

## 7. Risks

| Risk | Mitigation |
|---|---|
| Split-borrow knots in `ctx_param`'s 246 sites | A7 goes **last**; join analysis first; §4.6 protocol; combined accessors for the recurring pairs |
| Perf regression in ME/SAD and writer loops | §6 budget at stage closes; C4/D1 flagged as the watch points |
| MT soundness break while converting slice core | Miri probes are the gate (§4.7); `RecPicView` invariants never widened; D1 impls unchanged |
| Feature-arena aliasing subtleties (`*mut *mut u16` tables) | C5 converts representation and verifies on the screen-content differential before touching consumers |
| Long-tail parity breaks (EC modes, parse-only, odd assets) | full sweeps at family/session gates, not just commit; divergence = stop the line |
| Estimate drift on `svc_encode_slice.rs` | it is two checkpoints (D2, D3) with D4 buffered behind them; if it overflows, S3 splits at the D2/D3 boundary rather than rushing — the file's 84 allows are the single biggest tracked number |

## 8. Timeline

**4 sessions** planned (slack to **6**: S2 may split at the B/C boundary and S3 at the
D2/D3 boundary if `ctx_param`'s cascade, the pixel land, or the slice core overflows —
splits happen at checkpoint boundaries per D3's drop-from-the-end rule, never by
rushing). Sessions are strictly sequential — each stage's output is the next one's
precondition — so the calendar floor is the session count, not available parallelism.

| Cadence | Calendar |
|---|---|
| 1 large session/day | **~4–6 days** |

In agent-hours: ~75–105 of continuous work, of which ~8–10 h is gate wall-time (the
per-checkpoint commit gate is 15–19 min × ~23 checkpoints; the exit battery is a
half-day). The total work is unchanged from the original 8-session sizing — the 4-session
cut deletes a further four hand-off/re-orientation costs on top of the ~15 the large
sessions already deleted.

Basis: the repo's own measured burn-down (unsafe fns 890→432, raw-ptr mentions
5,212→1,338 over the 20 days to 2026-08-27) and the measured cost of the one completed
accessor flip (`ctx_ltr`: 28 call sites, 83 adjudicated conflicts, one half-day block). The remainder is the hard
tail, priced per cluster above rather than by extrapolating the mechanical-phase rate.

## 9. Tracking

Per checkpoint landing: regenerate the ratchet baseline downward. Per session close: log
one line — date, session id, checkpoints landed, allows remaining (total / encoder),
gate level run. The single progress number is
**`#[allow(unsafe_code)]` sites outside `src/api/`: 611 → 2**. The figure was
**627** when S1 opened, not 611 — F203's two found files plus the census fix had
drifted it upward since the plan was drafted. S1 closed it at **613** (its own log
says 614; F219 counts the tree). Stage A closed it at **612**.

The figure has a command of its own now — `rust/tools/safeplan_tracking.sh [ref]`
— so a session can check its opening number against the previous close instead of
trusting the hand-off brief.

**S3 closed it at 614 — upward.** F224 records why, and the reason is structural
rather than a regression: B1 converted four raw ctx *fields* to owned storage, and
a raw field carries no `#[allow]`, so the metric never counted the thing that was
removed. What it counts is the four named accessors that now write down the
aliasing contract those fields implied. This is F214's finding running the other
way, and it is the second stage in a row where the plan's single progress number
fails to track the work: read F216, F224 and this paragraph before quoting it.

**Session log**

| date | session | checkpoints landed | allows outside api | gate |
|---|---|---|---|---|
| 2026-08-27 | **S1** | A1, A2, A3, A4 (A5–A7 roll to S2) | 627 → **613** (logged 614; F219) | `session` PASS — sweeps 583/583 ×2, Miri 291/291, gtest 4/4 |
| 2026-08-27 | **S2** | A5, A6, A7 — **stage A complete** (B1–B3 roll to S3) | 613 → 612 | `session` PASS — sweeps 583/583 ×2, Miri 291/291 (**cpu 1105 s vs 1091 s, ratio 1.01**), gtest 4/4. **§4.7's two MT probes did not run** — F220 names what that leaves unverified; S3 runs them first |
| 2026-08-27 | **S3** | B3, B2, B1 — **stage B complete** (C1–C6 **and** D1–D4 roll to S4 — stage C was dropped from the roll chain at the S2→S3 hand-off and restored at the steward's review, on the user's catch, before S4 launched) | 612 → **614** (F224: the figure rises on a checkpoint that strictly reduces raw exposure) | `family` PASS per checkpoint — sweeps 583/583 ×2 each, 561 debug / 554 release; Miri encode shards inside every checkpoint (2/2, ~350 s each); ABI table byte-identical. **§4.7 MT probes: no verdict** — one run found F223's data race, no run completed on the fix (F223's tail; S4's first duty). **Checkpoint order reversed within stage B** on F221's correction |
| 2026-08-27 | **S4** | step 0 (probe duty), C1, C2, F226, C3, C4a, C-cascade, C-params, then post-close **D4a + E2a** — **stage C partially landed; roll to S5: C4b, C5, C6, D1–D3, D4b, E1, the rest of E2, E3 — plus two debts: the stage-C fork pair (six fork-touching landings probe-unverified; a relaunched run stopped ~35 min in, no verdict) and a sweep verdict for E2a** | 614 → **519** | `session` PASS at every gated landing — sweeps 583/583 ×2 each, 562 debug / 555 release, Miri 4 shards 292/292 (the lane gained F226's probe: 291 → 292), gtest 4/4. **The fork pair ran and is GREEN** (F225: 3463.45 s / 3530.38 s, pair 3535 s) — B1's fix is verified and F223's tail closes. Three findings: **F225** (probe verdict + the `fork_join_baseline.txt`/`fork_join_probe.sh` instrument), **F226** (a live data race in `UpdateMbListNeighborParallel` that no gate could reach, fixed, with a 1.24 s Miri referee), **F227** (a C4a defect `family` passed and `session` caught — F215's rule through `&mut [T]`; hoisted as rule **S70**). *(Row steward-amended: the close-docs commit predated the session's last two landings.)* |
| 2026-08-28 | **S5** | C4b, D1, C6a, E2b, C6b, C6c, C6d, D2a, D2b — **nine gated checkpoints**; **C5 stopped for the user's ruling** (F229: the arena is unreachable, not dark; F233: yet its search path is API-reachable — deletion recommended against — **ruled: D-scope-5, descoped; reversed the same day: converts in S6**); **D2c attempted and reverted** (F236 carries the three-way split the `&SDqLayer` flip needs); roll to S6: the D2c flip, C6's move-memory pair, D3, D4b, E1, the rest of E2, E3, F231's ~55 stale `# Safety` clauses | 519 → **460** | `session` PASS at all nine — sweeps 583/583 ×2 throughout; Miri lane **541 s wall / 1151 s cpu, ratio 1.00** (baseline advanced); three targeted worker-race probes added (1.30–1.37 s), each control run and seen to fail (F234 — 8 rounds blind, 200 a referee); C4b benched (+0.00% / +0.44% medians, twice, nothing over +5%); the six later checkpoints unbenched **by design** — E3 owns the full bench. *(Row added by the steward: the session's close updated the session map but not this table.)* |

## 10. Adoption amendments (ratified with the switch, 2026-08-27)

Bound at adoption; the drafted text above stands as drafted, these amend it. The prior
corpus (`safety_refactor_plan.md` §7.6 rules S1–S69, `phase9_findings.md` F1–F207)
**remains binding** — above all S68: before acting on any claim, re-read the code it
describes; a claim of absence gets its grep.

1. **F191 binds A6.** `ctx_func_list`'s table is rewritten at frame cadence
   (`SetFastCodingFunc`/`SetNormalCodingFunc`), so no borrow of it may be held across a
   frame boundary — the copy-the-fn-pointer-first pattern is the rule, per-dispatch and
   instantaneous. The deeper end state F191 names (the Phase 4b dispatch enums,
   finished) is priced as A6's alternative before the flip is chosen.
2. **The in-fork call-site rule is explicit.** N workers taking even transient
   `&mut *pCtx` is the S63 violation regardless of duration; every fork-reachable call
   site uses the `&self` reader path only, classified per checkpoint by
   `tools/phase9_forksplit.py`. A7's reader/writer split exists for exactly this.
3. **The layer's `*mut SDqLayer` parameters** (F191's fourth row — absent from
   §3's inventories) are named to D2–D3's handle redesign. F218 corrected F191's
   count to "42 tree-wide, 11 in-fork"; **F221 corrects F218**. Measured with the
   tool's own mode — `phase9_forksplit.py --type SDqLayer --list` — the split is
   **38 in-fork of 40 bodies, and 2 ST-flippable**. F218's 11 counts bodies taking
   *both* a `*mut sWelsEncCtx` and a `*mut SDqLayer` (reproducible as 12), which is
   a different population: 26 of the 38 take no context pointer at all. D2's
   redesign must satisfy **38** fork-reachable signatures, not eleven, and there is
   no "other thirty-one that convert under ordinary single-threaded rules" — there
   are two.
4. **Perf is measured inside C4 and D1** (bench before/after within the session), not
   only at stage closes — ME/SAD regressions localize badly after the fact.
5. **Carryover disciplines**: a tag (`// unsafe-cat: …`) is removed only by the
   conversion that removes its unsafe — never stripped early, never left stale; untagged
   unsafe keeps failing the census gate throughout; `lawful-single` items are rulings
   revisited under this plan's stricter goal (the kept out-of-bounds read at the encode
   error tail, ruling D-fid-2, needs a fresh ruling when its file converts); the F3
   flake-adjudication protocol binds any sweep anomaly; session hand-off briefs follow
   the standing plain-register style.
