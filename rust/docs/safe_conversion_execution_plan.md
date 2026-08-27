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
| **S1** ◀ next — brief [`prompts/safeplan_s1.md`](prompts/safeplan_s1.md) | A1–A7 | **The whole accessor layer** (~640 sites): small fry → `ctx_rc_at` → `ctx_ref_list`+`ctx_dq_layer` → paraset trio + `ctx_frame_bs_cur` → `ctx_vaa` → `ctx_func_list` → `ctx_param`, join analysis before each, full cascade | the accessor layer itself; `rc.rs`, `ref_list_mgr_svc.rs`, `au_set.rs`, `paraset_strategy.rs`, `encoder_ext.rs` bulk |
| **S2** | B1–B3 + C1–C6 | **Owned fields + the pixel land**: singletons → `Option<Box<T>>`, owned mutex, MT lifecycle, init/teardown; then the fn-pointer table → safe signatures, roving trios → offsets, MD/ME onto plane cursors, feature arenas → index tables, preprocess + processing | ctx raw singletons; `wels_func_ptr_def.rs`, `svc_encode_mb.rs`, `svc_mode_decision.rs`, `svc_base_layer_md.rs`, `svc_motion_estimate.rs`, `md.rs`, `wels_preprocess.rs`, `processing/` |
| **S3** | D1–D4 | **Writers + the slice core**: bit writers onto `safe::bits`; `svc_encode_slice.rs` in two checkpoints (aliases → handles, then the encode loop + dynamic slicing); MT residue; `deblocking_common` + `mc`/`copy_mb`. Miri fork/join + mid-row probes gate every slice-core landing | writer files, `svc_encode_slice.rs`, `slice_multi_threading.rs` (to its D1 line), `common/` |
| **S4** | E1–E3 | Decoder/common residue + trace newtype; **the lint flip** + census pin; **exit battery** + fallout | everything — §1 acceptance list |

Checkpoint detail follows, grouped by stage.

### Stage A — flip the context accessors (S1–S2)

The enabler: ~200 of the remaining unsafe fns are unsafe *only* because they call these.
Run `tools/phase9_ctx_join.py` (the join/fork-split analyzer) before each to map which
callers hold multiple accessors simultaneously.

| # | Scope | Sites | Cascade collapses |
|---|---|---|---|
| A1 | `ctx_mvd_cost_table/origin`, `ctx_rc`, `ctx_frame_bs`, `rc_gom_*` — the small fry, re-validating the method | ~32 | starts `rc.rs` |
| A2 | `ctx_rc_at` | 60 | finishes `rc.rs` (−55 unsafe fn) |
| A3 | `ctx_ref_list` + `ctx_dq_layer` (held jointly in the encode loop — join analysis first) | 51 | `ref_list_mgr_svc.rs` (−30) |
| A4 | `ctx_sps_array`/`ctx_subset_array`/`ctx_pps_array` + `ctx_frame_bs_cur` | 65 | `au_set.rs`, `paraset_strategy.rs` |
| A5 | `ctx_vaa` — includes resolving the `SVAAFrameInfoExt` downcast (6 callers cast the result; becomes an enum or accessor pair) | 79 | parts of `wels_preprocess.rs` |
| A6 | `ctx_func_list` — dispatch sites adopt the copy-the-fn-ptr-first pattern | 106 | dispatch-heavy files |
| A7 | `ctx_param` — the monster; by now a large fraction of its 246 sites sit in already-converted callers. Split into `&self` reader + `&mut` writer accessors | ≤246 | `encoder_ext.rs` bulk |

**Exit criteria**: zero `pub unsafe fn ctx_*`/`rc_*` accessors; `rc.rs`,
`ref_list_mgr_svc.rs`, `au_set.rs`, `paraset_strategy.rs` at or near zero allows;
family gate green.

### Stage B — owned fields and the MT lifecycle (S3)

| # | Scope |
|---|---|
| B1 | `pOut`/`pVpp`/`pSliceThreading` → `Option<Box<T>>`; `mutexSliceNumUpdate` → owned `Mutex<()>` (deleting `WelsMutexInit/Destroy` and the `Box::into_raw` dance); rewire `RequestMtResource`/`ReleaseMtResource` (`*mut *mut sWelsEncCtx` → `&mut`) and the alloc/free paths |
| B2 | `encoder_ext.rs` sweep (38 unsafe fn): init/teardown, param plumbing — mostly cascade after A7+B1 |
| B3 | `wels_encoder_ext.rs` (16 fn / 73 raw): frame-loop orchestration; the 2 version exports stay |

**Exit criteria**: no raw ctx-singleton fields; `slice_multi_threading.rs` down to its MT
residue; session gate + Miri probes green.

### Stage C — pixels, MD/ME, arenas, preprocess (S4–S5)

| # | Scope |
|---|---|
| C1 | The 10 unsafe fn-pointer aliases → safe signatures (types, every impl, every dispatch). Mechanical but wide; lands as one change so the table is never half-typed |
| C2 | Roving trios `pEncMb/pRefMb/pCsMb` → plane offsets; `svc_encode_mb.rs` (9 fn / 40 raw) |
| C3 | `svc_mode_decision.rs` + `svc_base_layer_md.rs` (57 fn) onto plane cursors |
| C4 | `svc_motion_estimate.rs` (28 fn / 52 raw) + `md.rs` (14) — SAD/search paths; watch the perf budget here |
| C5 | Screen-content feature arena → `Vec` + index tables (`picture.rs` storage structs + ME mirrors); differential test on the screen-content sweep before/after |
| C6 | `wels_preprocess.rs` remaining fields + fns (44) + `processing/` wrappers (14) |

**Exit criteria**: encoder pixel land raw-free; benches within budget; family gate green.

### Stage D — writers and the slice core (S6–S7)

| # | Scope |
|---|---|
| D1 | Bit writers onto `safe::bits`: `svc_set_mb_syn_cavlc/cabac`, `set_mb_syn_cabac`, `vlc_encoder`, `nal_encap`, `encode_mb_aux`/`decode_mb_aux` |
| D2 | `svc_encode_slice.rs` part 1 (81 fn / 117 raw, the biggest file): per-slice aliases → handles (§3c-5), slice construction/teardown |
| D3 | `svc_encode_slice.rs` part 2: the encode loop, dynamic slicing, `svc_enc_slice_segment.rs`; Miri fork/join + mid-row probes gate every landing |
| D4 | `slice_multi_threading.rs` residue (25) + `common/deblocking_common.rs` raw-wrapper retirement (safe kernels already exist; delete the raw twins) + `encoder/deblocking.rs`, `mc.rs`, `copy_mb.rs` |

**Exit criteria**: `encoder/` allows ≈ 0 outside the D1 impl file; session gate green.

### Stage E — freeze and exit (S8)

| # | Scope |
|---|---|
| E1 | Decoder residue (`decoder/picture.rs` 11 blocks; stray raws in `decoder_core`/`decoder_context`/`pic_queue`); `wels_trace` `pLogCtx` newtype confined to `src/api/`; opportunistic api-interior shrink (D2) |
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
**`#[allow(unsafe_code)]` sites outside `src/api/`: 611 → 2**.

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
3. **The layer's 42 in-fork `*mut SDqLayer` parameters** (F191's fourth row — absent
   from §3's inventories) are named to D2–D3's handle redesign.
4. **Perf is measured inside C4 and D1** (bench before/after within the session), not
   only at stage closes — ME/SAD regressions localize badly after the fact.
5. **Carryover disciplines**: a tag (`// unsafe-cat: …`) is removed only by the
   conversion that removes its unsafe — never stripped early, never left stale; untagged
   unsafe keeps failing the census gate throughout; `lawful-single` items are rulings
   revisited under this plan's stricter goal (the kept out-of-bounds read at the encode
   error tail, ruling D-fid-2, needs a fresh ruling when its file converts); the F3
   flake-adjudication protocol binds any sweep anomaly; session hand-off briefs follow
   the standing plain-register style.
