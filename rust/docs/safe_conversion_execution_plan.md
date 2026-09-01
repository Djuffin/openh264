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
   - `src/encoder/rec_view.rs`, which carries `#![deny(unsafe_code)]` plus **exactly
     one** product `#[allow(unsafe_code)]` — the `Sync for SharedCells` impl (see D1;
     S10.13 retired the second line, `Send for SliceJobHandle`, by making it
     compiler-derived, so the original two-file exception is now one),
   - files whose test modules carry Miri/provenance instruments (~17 today, tag
     `instrument(test)`), which take `#![deny(unsafe_code)]` with every test allow
     enumerated in the pinned census (**D-exit-4**, 2026-08-31).
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
   reduces to the api files, `rec_view.rs`'s one product line, and the enumerated
   `instrument(test)` set (D-exit-4), so any new `unsafe` anywhere else fails CI.
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
- **D-exit-4 / D-scope-6 / D-gate-8 (user, 2026-08-31, at the remaining-work scoping)**:
  test-instrument allows retire by deny-plus-enumeration in place; the 23 dormant
  screen-content casts convert to safe form (dark-code discipline); benches remain
  E3-only, reaffirmed with S10's 7% catch known. Full entries in
  `safety_refactor_plan.md`'s decisions ledger.
- **D1 (user, 2026-08-27): keep the two MT `unsafe impl` lines.** *(S10.13 amendment:
  the `Send for SliceJobHandle` line is retired — the handle now carries
  `&'a sWelsEncCtx` and `Send` is compiler-derived from `sWelsEncCtx: Sync` — so the
  decision's intent, an audited MT seam, is preserved with one fewer hand assertion;
  one line remains.)*
  `unsafe impl<T: Copy> Sync for SharedCells<T>` (`rec_view.rs:152`) stays as the audited
  exception; the `Send for SliceJobHandle` line is the one S10.13 retired. The Sync impl
  is the keystone of the disjoint-write MT scheme (shared `Cell` view over the
  reconstruction picture, fork via `std::thread::scope`). Its proof obligation
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
| **S6** ✅ **CLOSED 2026-08-29** — brief [`prompts/safeplan_s6.md`](prompts/safeplan_s6.md) | **D2c, the cascade, C5, D1's `nal_encap`, D1's `SSliceArgument` landed** — five gated checkpoints; roll to S7: the **context flip** (133 allows, and F239 says what it will cost), the `SLogContext` family (F242 has its shape), C6's move-memory pair, the rest of D1, D3, D4, E1, E2, E3 | **The layer flip landed, and Miri caught what the sweep could not.** **D2c**: 37 bodies take `*mut SDqLayer` — not 41, because F236 counted four fn-pointer typedefs and a test local as bodies (**F237**) — and 28 convert, 11 to `Option<&SDqLayer>` and 17 to `&SDqLayer`. The writer list was three short and the *compiler* found all three, including `DeblockingFilterSliceAvcbase`, whose `&mut` into `sMbDataP` is lawful only because F108 verified it single-threaded. **F238**: the unguarded group's rule ("they already dereference unconditionally") is false for a body that dereferences *nothing* — `UpdateFMESwitchNull` is empty and unconditionally installed, so `&*(current_layer(..))` at its call site would have been newly undefined; it takes `Option<&SDqLayer>`. Then **F239, the session's real finding**: `&SDqLayer` is a retag over the *whole* 760-byte layer and pops any field-precise derivation live across it — **on one thread**. Miri named it exactly (a `&mut` at `[0x208..0x220]` invalidated by a shared retag at `[0x0..0x2f8]`, then written through); **both sweeps had already passed it, twice, in both profiles**. A scan for the three-step span found **five, in four functions**; Miri reports one, because it aborts at the first. The cascade after the flip kept **7**, not the wave the brief predicted, and **F240** measures why: `ref_list_mgr_svc.rs` is blocked 52-to-17 by bare raw dereferences and the *context*, not by `current_layer`. **C5** (user-directed): the screen-content storage owns its five buffers with the two pointer tables as index tables, and the family gets **its first test ever** — no sweep row reaches a line of it — with three controls each seen red (**F241**). Then D1's ready-made `nal_encap` pair (nine callers, none changed) and `SSliceArgument`'s nine parameters, split by the compiler five writers / four readers | the DQ layer as a shared borrow, the screen-content arena, the NAL load/unload pair, the slice-argument family; allows 460 → **444** |
| **S7** ✅ **CLOSED 2026-08-29** — brief: continued from S6 in the same session | **The context flip, complete** — A1 (tabulation), A2 (the one write), A3 (`ParasetStrategy`), A4 (the seam, struck), A5 (the root-down flip); roll to S8: C6's move-memory pair, the rest of D1, D3, D4, E1, E2, E3, and the bench debt | **The item the plan budgeted a session for, and the campaign it budgeted was already done.** **A1** is the tabulation, and its instrument is the compiler: all 106 raw-context parameters and 9 typedefs flipped to `&sWelsEncCtx`, call sites rewritten off rustc's spans, residual type errors cleared to *zero* — the condition for MIR borrowck to run over every body. What survives is the complete blocker set, and it is **five errors**: one `E0596` (`sWelsCabacContexts`, one caller, not fork-reachable) and four `E0502`, all `ParasetStrategy`, all single-threaded. **Zero writes from inside the fork** — not because workers write nothing, but because every field they write already reaches them through interior mutability or separate storage (**F243**). So the storage campaign the plan specifies was complete before this session opened; what remained was structural. **A2** took the single write (`WelsCabacInit` → `&mut`, its read sibling → `&`, and a `copy_nonoverlapping` between two same-typed arrays → `copy_from_slice`). **A3** dissolved `ParasetStrategy`: the strategy object lives *inside* the context and its methods took the context again, but every one of them reached it through one or two accessor calls, so each narrowed to its fields and each call site splits them off one `&mut` context — five helpers in `ctx_paraset_arrays`' established shape, and **three comments that had been arguing borrowck's case in prose became type-level** (**F244**). **A4** struck itself: `sWelsEncCtx: Sync` is unreachable because `sLogCtx` holds the caller's `void*` — which the `unsafe impl`'s own audit had recorded and **D-exit-2** had ruled — *and* it was never needed, because the flip sends a raw pointer and each worker forms `&*job.pCtx` after arrival (**F245**, verified by probe). **A5** landed the flip: **106 raw-context bodies to zero**, the prohibition checker's scan set 106 → 5, and the cascade's candidate pool 129 → 209 with nine allows retiring behind it (**F246**) | the encoder context as a shared borrow, the parameter-set strategy, the CABAC context pair; allows 444 → **431**, raw-context sites 115 → **6** |
| **S8** ✅ **CLOSED 2026-08-30** — brief: S8's brief (the pixel family, stage D's remainder, the log context) | **Step 0 (the F239 span scanner), step 1 (the `SLogContext` family), step 2 (`BsWriter`'s parameters), step 9's trace half — four gated landings plus two instruments built and one fixed**; **steps 3, 4a and 5 stopped for the user's ruling (F249 — since **AMENDED**: ownership was never the question, the storage is owned and the view pattern is in production; the plan was missing a checkpoint)**; **RULED 2026-08-30: S9.0 — `EncPicView` + a VAA view in ONE checkpoint on `RecPicView`'s template (~99 sites), then steps 3/4a/4b/5 behind it**; roll to S9: S9.0, then steps 3/4a/4b/5, step 6 (the body-deref campaign), step 7 (the slice core), step 8 (preprocess + processing), E2, E3, and the bench debt — now **twenty** checkpoints deep | **The brief's own working rules caught the brief four times, and the instruments were the session's real output.** **Step 0** rebuilt the F239 scanner (`rust/tools/f239_span_scan.py`); its acceptance said the clean tree reports zero and it reported **one** — `WelsEncoderEncodeExt`'s three steps in line order, with the reborrow's branch ending `return`, so the use is unreachable. Line order is not execution order; fixed in the instrument, not waived in a table (**F247**), and calibrated against a *reconstruction of F239's real defect* rather than only the recorded synthetic control. **Step 1** took `SLogContext` by value: 19 raw parameters to zero, and because a write through a by-value `Copy` parameter compiles and goes nowhere, the writer set was established by reading every site rather than by rustc. 22 `addr_of_mut!` call sites became plain reads — an aliasing dividend nobody asked for. **Step 2** flipped six `BsWriter` parameters; `slice_writer`'s raw *return* stays by ruling (two unrelated parents, no single lifetime). **Both steps' predicted cascades were wrong** (**F248**): the blocking edge on all four named files is the *coding-parameter* family, which the plan carries as no step at all. **Steps 3, 4a and 5 cannot start** (**F249**): they convert pixel *parameters*, but every caller derives them from raw *storage* — `SDqLayer::pCsData`/`pEncData`, `SVAAFrameInfo`'s six planes (in **step 8's** file), `SPicture::pData` — so a kernel taking `&mut [u8]` whose caller builds it with `from_raw_parts` moves the `unsafe` rather than retiring it. **The progress metric counted its own documentation** (**F250**): seven comment mentions, six of them comments *warning* that this count is wrong — 431 was **424**. And the decoder residue was already complete (**F251**): two allows, both Miri instruments the tree mandates, not eight convertible ones | the log-context family, the entropy coder's writer parameters, the trace callback's `void*`; **`common/wels_trace.rs` sealed** (`#![forbid(unsafe_code)]`, 0/0/0 on the ratchet); allows 424 → **416** on the corrected basis |
| **S9** ✅ **CLOSED 2026-08-30** — continued from S8 in the same session, on the user's direction | **S9.0a/b/c (the plane views), S9.1 (steps 3+5), S9.2 (step 7a), S9.3 (step 7b) — six gated checkpoints**; roll to S10: **the slice-bank seam** (F256 — step 7's remaining 90, a D-mt-3 design problem), step 4a/4b, step 6a/6b, step 8, E2, E3, the 22-site source-plane migration (F254), and the bench debt | **The session's headline is three design errors found and corrected, two of them mine.** F249 called the pixel family an ownership problem and needed the user's ruling; **amended** — the storage was already owned (`PaddedPlane`, T6.F2), identity already existed (`SrcPicId`), and the view pattern was already in production (`RecPicView`). The plan was missing a *checkpoint*, not a ruling. **S9.0a then shipped the wrong view**: a read-only `RoPicView` forming a whole-plane `&[u8]` over a picture the fork *writes* (F117, and `bEnableBackgroundDetection` defaults **true**) — the project's own rule about shared claims, violated by the type whose doc quoted it. Miri passed it 296/0 because Miri never runs the sweeps. **S9.0b** backs the view onto `SharedPlane` and the added `unsafe` retires with the design that needed it. **S9.0c** then converts F117's three copy sites, which T9.B20 had deferred — **because that deferral's premise expired in session B4** (F126's `bg` preset plants a fault in this very luma copy and fails all three clips), and it took a user's objection rather than an instrument to notice (**F253**). **S9.1–S9.3**: steps 3 and 5 finish, **`common/` seals**, step 7a takes `svc_enc_slice_segment.rs` 20 → 0, and F240's `current_layer_ref` gets built and **retires nothing** — an allow leaves a body only when its *last* raw op does (**F255**, **F256**) | the source planes as a shared view, the DCT and block-copy slots, F117's background copies, the slice-argument validators; **`common/` sealed entirely**, plus `svc_enc_slice_segment.rs`; allows 416 → **371** |
| **S10** ✅ **CLOSED 2026-08-31** — brief: S10's brief (the source-plane referee, the fork seam, the rest of the middle); **first close 2026-08-30 at 347, then continued in the same session through S10.15** | **Twenty-two gated checkpoints in two halves.** First half: **S10.1 (step 1 — the source-plane referee), S10.5a + S10.5b (step 5's camera path and parameter family), S10.6a (the harvest)**; first close with step 2 and the slice-header writers built, measured and **reverted** (F259, F263). Second half: **S10.2** (step 2 lands as F259 priced it), **S10.3a–e + S10.4** (the slice-bank seam dissolves — the resolver boundary, safe twins, width narrowing, the pre-fork partition), **S10.5–S10.12** (the `Sync` ladder), **S10.13** (`SliceJobHandle` carries `&sWelsEncCtx`; the second audited `unsafe impl` deleted), **S10.14–S10.15** (the slice core's cheap half); roll to S11: **the close-out debts** (the unsettled Miri-lane ratio, findings F264+, both tables — paid at S11's open), **the bitstream seam** (F272 — the last structural item), the conversion mass, the screen-content casts (D-scope-6), the version exports to `src/api/`, the enumerated floor (D-exit-4), E2, E3, and the bench debt | **The first half landed the keystone; the second half spent it — the fork seam fell to a measurement.** Step 1's referee, F258's calibration lesson, F259's re-pricing of step 2, F260's comment-rule discovery and F262's empty harvest are the first close's record. Then **S10.2** landed step 2 exactly as F259 priced it, and the one bench run by exception caught a **−7% decoder regression the byte sweep certified as green** (F264 — `row_into`'s copy; fixed with an associated-type row borrow; D-gate-8 then reaffirmed by the user with the catch known). **S10.3c–e measured both seams as ~4:1 single-threaded** — callers paying the fork's price with no fork near them (F266: borrow *width*, not aliasing; safe twins whose `&mut` receiver is the type-enforced proof of no sibling). **S10.4** made the macroblock-grid partition a `split_at_mut` chain the compiler checks, turning F226's race shape inexpressible (F267). **S10.5 asked the compiler what blocks `sWelsEncCtx: Sync`** and got three roots and six pointer types (F269); eight checkpoints walked the ladder — three write-only fields deleted (F268: `unused_variables` allowed crate-wide is why 15 purposeless pointers survived as "work"), one live field re-resolved through the context, three identity/token pointers to `usize` with strict-provenance round trips, one stored-working-state family to parameters — with one absence claim corrected by a wider grep (F270). **S10.13 deleted `unsafe impl Send for SliceJobHandle`** — the auto impl applies, compiler-checked, and T7.C3's ordering comment upgraded into a borrow error (F271). S10.14–15 took the freed slice core's cheap half and scoped what remains: **one bitstream seam, 25 entropy-coder call sites** (F272) | the source-plane race referee and the coding-parameter family (first close); `layer_enc_pic` deleted; F254's whole-allocation defect class structurally closed; `SDqLayer`, `SVAAFrameInfo`, `SLogContext`, `CBackgroundDetection` and **`sWelsEncCtx` all `Sync`**; the tree's `unsafe impl` count at **one** (`rec_view.rs`); `UpdateMbMapForked` and its walker fully safe; allows 371 → 347 → **327** |
| **S11** ✅ **CLOSED 2026-08-31** — brief: [`prompts/safeplan_s11.md`](prompts/safeplan_s11.md) | **S11.0 through S11.52 — fifty-three gated checkpoints**; steps 0–5 closed, **E2 closed**; remaining at close: E3 + the bench debt | **Every step but E3 is done, and the conversion mass is exhausted at the enumerated floor.** The session's first half is recorded in the row this replaces (steps 0–5, allows 327 → 187). The second half converted the remainder by family and then closed E2. **The macroblock grid** (S11.26–S11.27): deblocking's access splits — `&mut SMB` for the record it writes, `&[SMB]` for the neighbours it reads, from one `split_at_mut` at the cursor (**F286**), which is the third answer to "which borrow rule is this raw dodging": *partition*. Two defects surfaced and are recorded: a filtered list paired with an unfiltered index (**F287**) and `iCountMbNumInSlice` used as a run descriptor when it is a running count (**F288**). **The slice banks** (S11.32–S11.36) went to the workers pre-fork in four phases, and produced the session's sharpest rule: a capability threaded into a *repeated judgement* must be reborrowed, never `.take()`n (**F289**) — the compiler types both spellings and only the second is correct. **`encoder_ext.rs`** (S11.37–S11.41) fell in five checkpoints: nineteen bodies in one strip, the stride arena's fill through two bounded accessors, then the three heavies — `WelsInitCurrentLayer`, `InitDqLayers` and `WelsEncoderEncodeExt` — each of which deleted resolvers on the way out. **The preprocess and VAA families** (S11.42–S11.43) took their planes as borrows on the family's own `ScdPlanes` precedent, leaving `processing/` at zero. **Then four structural results.** The doc comments were the queue: three resolvers whose own exit clauses had fired, deleted together (**F290**), and four more with *zero callers and only their imports left* (**F291**) — plus nine census tags that had outlived their `unsafe` and were inflating the categories the plan prioritises by (**F292**). The stride arena's "a bound the C++ never had" was **the element type all along** — two typed stores, offsets become indices, sharing preserved, 19 blocks gone (**F293**). And `SLayerBSInfo::pNalLengthInByte`, 25 blocks across four files, was a *view* of the encoder's own `Vec` — the position became the state and the ABI pointer a safe reslice (**F294**). **S11.49** sealed ten more files with `forbid` (65 of 86), and **S11.50 closed E2**: the exceptions are pinned as an enumerated list, not a count, and the pin was seen red four ways — including a *tagged* new allow, which a count-only check passes (**F297**). | **the tree's last structural raws are gone**: `ctx_ref_list_raw`, `ctx_src_pool_raw`, `ctx_vpp_raw`, `ctx_param_raw`, `ctx_out_raw`, `ctx_func_list_raw`, `WelsSetMemZero_c`, `Destroy`, `SampleVariance16x16_c`, the seven stride-arena cursors and the four `ctx_*` wrappers above them, all deleted. `MbSplit`, `VaaCalcPlanes`, `BgdPlanes`, `plane_tail`, `nal_len_ptr`/`nal_len_at`/`advance_nal_len_base`, `i16_region_mut`/`i32_block24_mut` built. **allows 327 → 32** (21 instrument + 11 named), `unsafe_fn` 59, `unsafe_block` 136, **65 files sealed**. **S11.51–S11.52** close the row: the enumerated floor regenerated 26 → 21 → 22, and `DownsamplePadding`'s pool-to-pool copy converted to `SPicture::copy_planes_from` behind a *differential* referee — the sweep is structurally blind to that arm (an `eprintln` fired zero times across all 76 `dl` configurations), so the byte gate could only prove nothing else broke |
| **S12** ✅ **CLOSED 2026-08-31** — brief: S12's brief (the last code changes; the exit battery deliberately split out) | **S12.1–S12.11 — eleven gated checkpoints**. S12.1–S12.4 are the brief's four items; **S12.5–S12.11 were added at the user's direction** after a question the brief never asks — *which of the remaining unsafe and raw pointers are not about the external C API?* Roll-forward, one item: **E3 — the exit battery, entire — runs as S13**. (S12 also produced a scoping report the plan did not ask for: [`api_island_scope.md`](api_island_scope.md), measuring the C-ABI island and finding **193 of its 218 raw dereferences are not the C ABI** — decoder logic exempt from E2's census by file path. Not scheduled; the recommendation is after S13.) | **Four small items, and three of them found the brief wrong about its own numbers.** **S12.1** promoted `unsafe_op_in_unsafe_fn` from the crate-wide allow to `#![deny]`. The reason it had been allowed since session J — "an inner `unsafe {}` per deref would add thousands of blocks" — was an estimate that had never been measured; measured, the whole crate holds **135 unguarded operations in 13 `unsafe fn` across 4 files** (108 of them raw derefs on the decoder's five reordering thunks). Thirteen `cargo fix`-shaped body wraps, chosen over 135 per-operation wraps because most sites are *place* expressions that cannot take a block without restructuring the statement — body wrapping moves no interior line, which is the diffability the whole allow list protects (**F298**). **S12.2** moved `SetOption`/`GetOption` — 605 lines, diffed byte-identical — into `api/encoder_options.rs`, and `wels_encoder_ext.rs` sealed behind them. A cross-module move is a **re-resolution**, and that is what needed refereeing: the first attempt bound `RC_OFF_MODE` to `wels_preprocess`'s `i32` sentinel where the original resolved `RC_MODES::RC_OFF_MODE`, a named import shadowing a glob (**F299**). The class was then audited, not trusted, and the ratchet's per-file counts conserved exactly (`raw_ptr` 39 → 1 / 0 → 38). Also deleted: two `clippy::not_unsafe_ptr_arg_deref` allows measured to suppress **zero** sites (**F300**). **S12.3** took the last `SCREEN_CONTENT(dormant)` allow. Dissolution — S11.3's move — does not reach it, because the site is a deref of a field *of* an extension already held, not a cast *to* one. It converted instead, on the C++'s own annotations (`//real memory` / `//pointer`) and its allocator: the sixteen `pVaaBlockStaticIdc` slots are equal-stride rows of ONE block, so every pointer in that family was carrying a **row number** (**F301**). S10.11's `expose_provenance` round trip dissolved with it. At the user's direction no screen-content code was discarded — every removed line is a field declaration or its retyped replacement. Its referee is a unit test, because no sweep row can reach the body; the test caught a real defect in the new accessor on its first run (**F302**). **S12.4** pinned the floor and corrected two tags that lied: `port-raw(F162)` → `lawful-single(F162)` (the `port-raw` tag *means* the queue, and the queue is empty), and the crate root's category list, which still advertised two categories with no members. **Then the classification question, and it found three things four gated checkpoints had not.** Split by purpose, only ~10 of the 36 production raw-pointer tokens outside `src/api/` reach a type the application sees; the rest mirror the C++'s *internal* headers. **S12.5**: `WelsEmms`'s `asm!("emms")` had carried **no `#[allow(unsafe_code)]`** since it was written, hidden from `#![deny]` and from every instrument by `#[cfg(target_arch = "x86_64")]` on an aarch64 host — **an x86_64 build did not compile** (**F303**). Deleted rather than tagged: `cpu.h` declares `WelsEmms` only inside `#if defined(X86_ASM)`, beside five siblings *none of which this port translated*, and a crate with no assembly kernels executes no MMX for `emms` to clear. A `cfg-region` tripwire, armed at **zero**, replaces it. **S12.6** deleted three dead raw-pointer residues — `mb_cursor` (superseded at S9.0; its last two mentions were comments saying so), `stride_idx` (its only caller) and `WelsOpenDecoder`'s ignored `_pLogCtx` — and recorded why eleven sessions of tooling missed them: the cascade works on convertible *bodies*, so it cannot see a converted function nobody calls (**F304**). **Then the island itself, on the user's direction to do it before S13 rather than after** ([`api_island_scope.md`](api_island_scope.md) scoped it; the report is marked done with estimate against actual). **S12.7** took the reordering family's out-parameters off their round trip — `decode` held `&mut [*mut u8; 3]` / `&mut SBufferInfo` and converted them *back* to raw at three places so the helpers could deref them again. **S12.8** gave the five helpers and `pool_for` the context by reference; all six stop being `unsafe fn`, `PicPool::slot_at_mut` returns `Option<&mut SPicture>` and `slot_ptr_mut` retires, and **four null tests deleted — two of them self-contradictory**, their `None`/`else` arms dereferencing the pointer they tested. **S12.9** converted `decode` (46 dereferences) and found the scoping's one named blocker was not one: NLL ends a borrow per path, so `self.initialize(..)` compiled unchanged (**F305**). **S12.10** took `decode_parser` and `flush`, and `ctx_ptr` — S42's decoder-side root — died with its last caller. **S12.11** is the one that outlasts the rest: the census now exempts the **ABI surface** (`extern "C"` items) rather than the **directory**, pinning 27 `island-nonextern(...)` rows that no instrument could see, because an exemption written by path accumulates whatever is put inside it (**F306**) | `unsafe_op_in_unsafe_fn` no longer allowed anywhere; **`SCREEN_CONTENT(dormant)` and `port-raw` are categories with no members**; zero C-ABI *option* surface outside `src/api/`; zero `expose_provenance` in the crate. **allows 32 → 29** (22 instrument + 7 named), **`unsafe_fn` 59 → 53**, `unsafe_block` 147 → **138**, `raw_ptr` 418 → **399**, **66 of 86 files sealed**; the crate holds **no `asm!`, no `std::arch` and no `#[cfg(target_*)]` at all**, and `codec_api.rs` carries only ABI-shaped unsafe — the `extern "C"` thunks, the vtable dispatches and the deliberate C-ABI test drivers (56 → 50 `unsafe fn`, 44 → 39 allows). The census pins three quantities apart now: the tracking number **29**, island non-extern allows **27**, cfg-regions **0**. **The final census — the floor the repository keeps, all 29 rows outside `src/api/`:** `decoder/picture.rs instrument(test) x2`; `encoder/encoder_context.rs fork-shared(S63) x1` (`ctx_dq_layer` — one context, N workers); `encoder/encoder_context.rs instrument(test) x4`; `encoder/encoder_ext.rs instrument(test) x2`; `encoder/encoder_ext.rs lawful-single(F162) x1` (upstream's read one past `sSpatialIndexMap`, reproduced because safe indexing would panic where the C++ reads and a panic is not byte-identical); `encoder/nal_encap.rs instrument(test) x3`; `encoder/picture.rs instrument(test) x3`; `encoder/rec_view.rs recon-seam x2` (D-mt-3's single seam; holds the crate's only `unsafe impl`, `Sync for SharedCells`); `encoder/ref_list_mgr_svc.rs instrument(test) x1`; `encoder/slice_multi_threading.rs instrument(test) x1`; `encoder/svc_encode_slice.rs fork-shared(S63) x1` (`current_layer`; argument at `phase9_disposition.md` §2, refereed by the fork/join Miri probes); `encoder/svc_encode_slice.rs instrument(test) x4`; `encoder/svc_mode_decision.rs instrument(test) x1`; `encoder/svc_set_mb_syn_cabac.rs C-ABI(test) x1`; `encoder/wels_preprocess.rs C-ABI x2` (`WelsMoveMemory_c` and its one ingest caller — the application's own plane buffers, contract on the header). The 22 `instrument`/`C-ABI(test)` rows are itemized per test in `tools/unsafe_instrument_floor.txt`; inside the island, 48 allows in `src/api/` watched by `abi_exports.sh` and `abi_sizes.sh` |

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
| 2026-08-29 | **S6** | D2c (the `&SDqLayer` flip), the de-unsafe cascade, C5 (screen-content), D1's `nal_encap` pair, D1's `SSliceArgument` — **five gated checkpoints**; roll to S7: the context flip, the `SLogContext` family, C6's move-memory pair, the rest of D1, D3, D4, E1, E2, E3 | 460 → **444** | `session` PASS at all five — sweeps 583/583 ×2 throughout, Miri 4 shards 295 → **296** (the lane gained C5's referee), gtest 4/4. **One checkpoint failed its first Miri run and that is the session's headline**: D2c's flip introduced a whole-struct retag defect (F239) that both differential sweeps certified byte-identical in both profiles; fixed by narrowing four derivations' live ranges, and a scan found four more spans of the same shape than Miri could report. C5 added `feature_storage_arena_invariants_hold_over_a_synthetic_frame` (<0.01 s, **three controls each seen red**) — the screen-content family's first test, and its only referee, since no gate reaches it. **No benches** — E3 owns them, and the debt is now eleven checkpoints deep (S5's six plus these five). |
| 2026-08-29 | **S7** | A1 (context-flip tabulation), A2 (the one context write), A3 (`ParasetStrategy`), A4 (the `SliceJobHandle` seam — **struck, not landed**), A5 (the root-down flip) — **five gated checkpoints**, continued from S6 in the same session; roll to S8: **the pixel family** (53 plane-root allows + 10 coefficient-block, of 87 other-raw — the report's own census, omitted from this line as first written), C6's move-memory pair, the rest of D1 (`BsWriter`'s 6), D3, D4, E1 (`SLogContext` **by value** per F242), E2, E3 — **plus two instrument debts: the F239 span scanner exists only in prose (commit it as a tool, controls red) and `miri_wall_baseline.txt` still reads S5's close** | 444 → **431** | `session` PASS at all five — sweeps 583/583 ×2 throughout, Miri 4 shards 296/296, gtest 4/4. **The headline is a measurement, not a conversion**: the plan's field-migration campaign for this flip was already complete, and the compiler said so across all 106 bodies at once (F243). **Two of the tabulation's own blockers were wrong in opposite directions** — §4(c) called `ParasetStrategy` mechanical when it was design work (F244), §4(b) called the seam a blocker when it is not one (F245) — and both were the *inferred* items, where §2's *measured* write set held exactly. F245 records that asymmetry as a rule. **No benches** — E3 owns them, and the debt is now sixteen checkpoints deep. |
| 2026-08-30 | **S8** | step 0 (span scanner), step 1 (`SLogContext` by value), step 2 (`BsWriter` parameters), step 9's trace half — **four gated landings**; two instruments built (`f239_span_scan.py`, `deunsafe_cascade.py`) and one fixed (`safeplan_tracking.sh`); **steps 3, 4a, 5 stopped for the user's ruling (F249)**; step 9's decoder half needed no work (F251); roll to S9: **S9.0 (the plane views — ruled 2026-08-30: `EncPicView` + VAA view in one checkpoint, ~99 sites, gating 132 allows / 32% of the remainder)**, then steps 3/4a/4b/5, 6, 7, 8, E2, E3, and the bench debt | **424 → 416** (the basis changed: F250 — the metric counted seven comment mentions, so the brief's 431 was 424 and every S8 figure is the attribute count) | `family` PASS for step 1 (sweeps 583/583 ×2, 561 debug / 554 release); `session` PASS with `MIRI_SCOPE=encoder` for steps 2 and 9 (**Miri 4 shards 296/296** both times, sweeps 583/583 ×2, gtest 4/4). **Step 9's gate FAILED on its first run** — the unsafe ratchet, `api/codec_api.rs` +6 raw_ptr / +2 blocks / +1 fn — because the checkpoint *moves* two C-ABI items into the island and the ratchet is a per-file monotonic guard that reads a relocation as a regression. Baseline regenerated after a field-by-field diff: **3 increases, all three the moved items; 36 decreases**, led by `common/wels_trace.rs` to zero on all three metrics. That checkpoint reduces no total unsafe (two allows out of `common/`, two into `src/api/`); the gain is the seal. **No benches** — E3 owns them, and the debt is now twenty checkpoints deep. Miri baseline advanced, and **F-note: it had stood at S5's close** — S6 and S7 both failed to advance it, so S8's 1.06 ratio spans three sessions |
| 2026-08-30 | **S9** | S9.0a/b/c (plane views + DCT + F117's copies), S9.1 (steps 3+5), S9.2 (step 7a), S9.3 (step 7b) — **six gated checkpoints**; steps 3 and 5 done to their limits, step 7 at **20 of 110**; roll to S10: the slice-bank seam, steps 4a/4b/6a/6b/8, E2, E3, F254's 22-site migration, and the bench debt | **416 → 371** | `session` with MIRI_SCOPE=encoder at every checkpoint — **PASS at all six**; sweeps 583/583 ×2 throughout (including the `bg` preset's 48 rows, which are the byte referee for the F117 conversion), Miri 4 shards **296 → 297** (the S9.0b twin test joins the lane), gtest 4/4. **Two checkpoints failed their first run on the unsafe ratchet**, both because a checkpoint *moved* `unsafe` rather than adding it — S8.9 into the api island, S9.0a into the recon seam; each baseline regenerated only after a field-by-field diff proved the increases were exactly the moved items. **One soundness defect was found in this session's own committed code** (S9.0a, corrected in S9.0b) and one standing deferral was overturned (T9.B20, via F253) — neither by an instrument. **No benches** — E3 owns them, and the debt is now **twenty-six** checkpoints deep. |
| 2026-08-30 | **S10** | S10.1 (step 1, the source-plane referee), S10.5a (`ref_list_mgr_svc.rs`), S10.5b (the coding-parameter family), S10.6a (the harvest) — **four gated checkpoints**; **step 2 built to the `mc.rs` boundary and reverted (F259); the slice-header writers built and reverted (F263)**; roll to S11: step 2 as a kernel-family migration, step 3 as **two** seams, step 4a/4b, the rest of 5b, step 6, E2, E3, the bench debt | **371 → 347** | `session` with `MIRI_SCOPE=encoder` at every checkpoint — **PASS at all four** (9 passed / 0 failed / 2 skipped each); sweeps 583/583 ×2 throughout, gtest 4/4, Miri 4 shards **297 → 298** (step 1's probe joins the lane). Miri cpu **1458 s** against S9's 1419 s, **ratio 1.03**; F257's prepend fix verified by reading the baseline back the way `gates.sh` reads it. **Two checkpoints failed their first gate run**, neither on code: S10.5a on `ignored set is 21` — a ```ignore doc fence on a *public* accessor is an ignored doctest (**F261**) — and S10.5b on the unsafe ratchet, `encoder_context.rs unsafe_block 31 → 32`, the relocation case S8.9 and S9.0a hit, regenerated after a field-by-field diff proving **1 increase, exactly the new accessor; 5 decreases**. Ratchet totals now 643/323/199/2. Instruments per batch: `f239_span_scan.py` **0 spans** throughout, `deunsafe_cascade.py` re-run after every batch (it found the eighth body of S10.5a). **No benches** — E3 owns them, and the debt is now **thirty** checkpoints deep. |
| 2026-08-31 | **S10** (continuation — same session, second close) | S10.2 (step 2 lands), S10.3a–e (the slice-bank seam's resolver, twins and width work), S10.4 (the pre-fork grid partition), S10.5–S10.12 (the `Sync` ladder), S10.13 (the `unsafe impl Send` deleted), S10.14–S10.15 (the slice core's cheap half) — **eighteen gated checkpoints**, ratchet regenerated downward at the end (**607/290/198/1**; field-by-field diff: 0 increases, 20 decreases across 12 files); the close-out (findings F264–F272, these tables, the ratio settle) **paid at S11's open, not at the session's own end** | **347 → 327** | Per-checkpoint referees rather than the full session battery: `cargo test --lib` **405/0** and differential sweep **583/583 ×2 byte-identical at every checkpoint**; targeted Miri probes at the worker-shared checkpoints (S10.4: both affected probes; S10.13: the four MT probes + the source-plane probe); `f239_span_scan.py` **0 spans** at every checkpoint that ran it; ratchet **no increases** at any checkpoint. **One bench ran, by exception** (S10.2): `decode_1080p_bench` caught `row_into`'s **−7%** decoder regression that the byte sweep certified green — fixed by an associated-type row borrow, 354.8 fps recovered (F264); **D-gate-8 then reaffirmed by the user with the catch known** — the bench debt still clears whole at E3. **The session-level Miri lane was left unsettled**: its last in-session reading was **2.16** against the 1.3 tripwire (the reading was never committed — S11's brief is its only record; no committed change matches the "fix" it mentions). **Settled at S11's open (F273): the reading reproduces** — cpu 3041 s vs 1458, **ratio 2.09 [CPU, F170]** (wall 1166 vs 540 = 2.16, the brief's number); 73% of the lane's cpu is the two full-encode fork probes (1148 s + 1064 s, one test each, drives unchanged), so the continuation made the encode loop itself ~4× costlier under Miri — baseline prepended at the settled level, and **E3's full-drive probe pair must be re-budgeted before it runs**. S10.4's coverage caveat stands in F267: both diffharness drivers pin `bUseLoadBalancing` off, so the load-balancing fork's only end-to-end cover is its unit test. |
| 2026-08-31 | **S11** | S11.0–S11.50 — **fifty-one gated checkpoints**; steps 0–5 and **E2** closed; remaining: E3 + the bench debt | **327 → 32** | **Two cadences, by the user's direction**, and the second half is the one this row adds. Per checkpoint: the differential sweep as byte referee — **847/847 byte-identical at every one of the twenty-seven checkpoints from S11.24 on**, both profiles — plus `cargo test --lib` (405 → 409), `f239_span_scan.py` (**0 spans throughout**), `deunsafe_cascade.py` to convergence, and the ratchet. The cascade's convertible queue reached **"kept 0 of 0"** at S11.45 and has stayed there: at function granularity the mass is exhausted. **Ratchet regenerated seven more times**, each after a field-by-field diff: S11.39–S11.43 (monotone decreases), S11.41 (**+10 statement-scoped blocks against −3 fn-wide claims** — stated plainly as a +7 on the tracking number, the wrong axis to read that checkpoint on), S11.46 (−19 blocks), S11.47 (**−25 blocks against +1 raw pointer**, the single ABI derivation site). **The full `gates.sh commit` battery ran green at S11.50**: 567 tests debug / 560 release, 0 failed, 21 ignored both profiles; ratchet flat; both censuses green — and the gate caught its own pin drift on the way (the `#[ignore]` count, moved *with its reason*, **F297**). **Findings F286–F297** written this session close the docs debt. **No benches** — E3 owns them, and the debt is now **eighty-one** checkpoints deep. |
| 2026-08-31 | **S12** | S12.1 (the lint promoted), S12.2 (`SetOption`/`GetOption` to `src/api/`; `wels_encoder_ext.rs` sealed), S12.3 (the last screen-content `unsafe`), S12.4 (the pin tightened), **S12.5** (`WelsEmms` deleted; the `cfg-region` tripwire), **S12.6** (three dead raw-pointer residues), **S12.7–S12.11** (the C-ABI island's Group A: out-parameters, the reordering family, `decode`, `decode_parser`/`flush`, and the census's exemption rule) — **eleven gated checkpoints**; **the refactoring's last code change**; roll-forward, one item: **E3 — the exit battery, entire — runs as S13** | **32 → 29** | `gates.sh family` **PASS at every checkpoint** — tests 568/561 → **569/562** (S12.3's referee), sweep **583/583 byte-identical in both profiles** four times over, `f239_span_scan.py` **0 spans** throughout (2671 → 2677 bodies, 85 → 86 files), census green at 17 → 16 → 15 rows. **The ABI referees are the ones that matter for S12.2** and both ran on it: `abi_sizes.sh --check` current (C and C++ agreeing on 170 lines) and `abi_exports.sh release` **7/7 — exactly upstream's seven names, unchanged**. Ratchet regenerated five times, each after a field-by-field diff: S12.1 **+13 `unsafe_block` and nothing else**, all four files accounted for (the promotion's honesty markers, an increase by design); S12.2 **conserving exactly** — `unsafe_block` 2 → 0 / 0 → 2 and `raw_ptr` 39 → 1 / 0 → 38 across the two files, totals flat, which is what proves a move is a move; S12.4 **downward and pinned** at `raw_ptr` 421 / `unsafe_block` 148; S12.5 `unsafe_block` 148 → **147** (the deleted `emms` block, and nothing else); S12.6 `raw_ptr` 421 → **418** (two for `mb_cursor`'s signature, one for the dropped parameter). `assert_size!(SVAAFrameInfoExt, 1520 → 1696)` re-pinned with its arithmetic at the site. **Findings F298–F306** written this session. **S12.7–S12.11 ran `family` plus both ABI referees at every checkpoint** — `abi_sizes.sh --check` current at 170 lines and `abi_exports.sh` 7/7 throughout, which is the whole constraint on work inside `src/api/`; the byte sweep stayed 583/583 in both profiles and is the statement that the *encoder* is untouched, since it is encoder-only and enters none of this code. The referee that does is the integration suite, measured by probe: `decode` and `ReorderPicturesInDisplay` **24,800** entries per run, the buffering family 2,807 / 2,803 / 2,017 / 831, against byte-exact JVT conformance comparisons. One F170 phantom logged at S12.11 (release sweep 3366 s wall against 35 s cpu on a loaded machine; cpu matched the prior run's 36 s). **S12.5's referee was not a gate**: the host cannot build x86_64 (no std for the target), so the defect and its fix were both proved by flipping the `cfg` predicate to `aarch64` and reading `cargo check` — `error: usage of an unsafe block` before, 0 errors after. The new `cfg-region` check was seen red in both directions on F297's rule. **S12.6** additionally pinned `cargo doc`'s **75 unresolved intra-doc links before and after**, so the deleted helpers left none behind. **No benches, and no Miri** — S13 owns the whole exit battery by the user's direction; the bench debt stands at **eighty-five** checkpoints and the Miri-lane debt at **fifty-seven**. |

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
