# Phase 6, session B — the encode probe goes live, and the clearing

Four faces, in order: **F52 adjudicated** → **the encode-path probe runs green
under Miri** (the three blocker settlements executed, then the walk) → **the
`c_void` clearing** → **the `SPicture` settlement, and its head if the boundary
allows**. Governing: [`phase6.md`](phase6.md) §1–§3 and §6; **S15**, **S18**,
**S24**, **S29**, **S31**, **S32**, **S33**, **S34**; **D-fid-1**, **D-par-1**,
**D-perf-6**. Counts below were taken at `a3a717ff`; re-grep at each face's open.

## The finish rule

**This session has no hand-off.** It ends when §6's done-test reads met, or at a
blocker only the steward can clear, named. A size is never a stop (S31: state
lives on disk — compact, re-read this file, continue). A clean face boundary is a
checkpoint. Questions take the decision ladder: settle by reading and write the
settlement; lint-scope questions default to the enumerated exception with a
pointer; behaviour questions never default — park exactly that item with a
one-paragraph record and continue.

**The probe finding UB is the probe working** (session A: ten defects on the init
path alone). Every red on the encode path is a finding: fix it if it is Phase 6's
(provenance, ordering, spelling, an F14-class accommodation, an S18 deletion),
park it with its owner if it is Phase 7's (MT) or Phase 8's (`api/`), continue.
Byte-exactness is the correctness definition: `gates.sh family` (both sweeps,
341/341) at every face that touched production code.

## 0. Start

1. Commit the inherited doc tail (this brief, `phase6.md` §6).
2. Open per **S27** (session A closed `exit` `OVERALL: PASS` 13/0/1): build both
   profiles + `--all-targets`, tests (484 / 478 / 20), ratchet, census (58).
3. **First commit, two small items:**
   - `svc_encode_slice.rs:2990` carries a leftover token: `PLACEHOLDER_REVERT`.
     Replace it with the measured sentence from T6.A1's commit message (F57's
     `+ kuiMvdCostTableOvershoot` deleted → the live probe red at `md.rs:1544`,
     "attempting to offset pointer by 1042 bytes"; restored → green).
   - `MIRI_SKIPS=(--skip wels_thread_pool --skip encoder_ext)` (`gates.sh:339`):
     the `encoder_ext` line names F13, whose production site session A fixed. It
     now skips exactly two tests, `request_memory_svc_builds_the_parameter_sets`
     and `request_memory_svc_builds_the_dq_layers` (`encoder_ext.rs:1787`, `:1820`).
     Run them: `cargo +nightly miri test --lib -- request_memory_svc` (with the
     `--lib` step's flags). Green → delete the `--skip encoder_ext` token (S15:
     deleting the line is part of fixing what it names). Red → keep it, record
     what it hits and who owns it, and note that in phase6_findings.md.
4. S33 on every number written; a per-face breadcrumb in the log.

## 1. Face 0 — F52's six, adjudicated by reading

`python3 tools/find_shadowing_stubs.py` lists them. Pre-read by the steward;
**confirm each by opening both lines** before recording:

| name | "trivial" definition | "substantial" | reading |
|---|---|---|---|
| `Uninit` | `wels_task_management.rs:662` | `:748`, `wels_thread_pool.rs:688` | trait **declaration** in `IWelsTaskManage` (`unsafe fn Uninit(&mut self);`) — no body to shadow with |
| `InitFrame` | `wels_task_management.rs:663` | `:926` | same trait, declaration |
| `ExecuteTasks` | `wels_task_management.rs:664` | `:941`, `:1010` | same trait, declaration; the two impls are the C++ base and `CWelsTaskManageOne` overrides |
| `OnTaskStop` | `wels_thread_pool.rs:97` | `:442` | trait declaration in `IWelsTaskThreadSink` |
| `WelsRcPostFrameSkipping` | `rc.rs:1860` (`false`) | `rc.rs:651` | the free fn is **faithful**: `ratectl.cpp:1015` is `return false;` with a TODO; `:651` is T4b's `RCMode` dispatcher that calls it |
| `push_back` | `wels_thread_pool.rs:131` | `wels_task_management.rs:578` | two methods on two types (`CWelsList<T>`, `CWelsTaskList`); resolution is by receiver type, no shadowing possible |

Then:

1. Record the adjudication in `phase6_findings.md` as **F52's Phase 6 close** —
   six lines, one per name, with the reason. `phase5.md`'s open-findings list and
   plan §0's open-findings row: F52 closed.
2. **Amend the sweep**: a `fn` line ending in `;` is a declaration, not a body —
   skip it. Re-run; state the count before/after (21 → expected 17) and confirm
   the six are gone. Guard the amendment the way F52 guarded its own fix: the
   original F52 stub shape (`fn X(...) -> bool { true }` beside a real `X`) must
   still print — show it on a scratch file or an inline self-test.
3. Nothing in the encoder converts before this face is recorded.

## 2. Face 1 — the encode probe goes live

The probe is in the tree: `encode_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker`
(`svc_encode_slice.rs:3035`), `#[cfg_attr(miri, ignore)]`, blocked by
`SSlice.pSliceBsa`. Deleting the attribute is this face's done-test.

### 2.1 S29 spelling sweep, one commit, before anything else

The encoder's stored escaping borrows (`= &mut (*p).f` assigned into a struct
field, S29's worst class), all of them:

- `au_set.rs:816`, `paraset_strategy.rs:419`, `encoder_ext.rs:2255` — `pSpsP = &mut (*pSubsetSps).pSps`
- `wels_preprocess.rs:1673`, `:1729`, `:1754`, `:2552` — `pCalcResult = &mut (*pVaaInfo).sVaaCalcInfo` (`:1754` stores it *inside* `pVaaInfo` itself)
- `wels_task_management.rs:255`, `:414` — `m_pSliceBs = &mut (*self.m_pSlice).sSliceBs`
- `svc_encode_slice.rs:2306`, `:2613` — `pSliceBsa` (dies in 2.2; do not respell)

`addr_of_mut!` at each. No behaviour, no layout; `gates.sh commit`.

### 2.2 The three settlements — write them into the log, then execute

**(a) `SSlice.pSliceBsa` is a cache of a one-bit choice — it dies.**
`InitSliceBsBuffer` (`svc_encode_slice.rs:2294`) sets it to the slice's own
`sSliceBs.sBsWrite` when it also allocates `sSliceBs.pBs`, and to
`pOut->sBsWrite` when it leaves `pBs` null. So the choice is already recorded in
`pBs`'s nullness. Execute:

- `fn slice_writer(pEncCtx, pSlice) -> *mut BsWriter` beside `slice_bs_buffer`
  (`:1899`): `!(*pSlice).sSliceBs.pBs.is_null()` → `addr_of_mut!((*pSlice).sSliceBs.sBsWrite)`,
  else `addr_of_mut!((*(*pEncCtx).pOut).sBsWrite)`. Derived fresh at every use, so
  `InitBitStream`'s per-frame write through `pOut` (`encoder_context.rs:840`) has
  nothing to invalidate. `slice_bs_buffer` takes the same discriminator; its
  pointer comparison at `:1900` goes.
- Every `let pBs = (*pSlice).pSliceBsa;` (12: `svc_set_mb_syn_cavlc.rs` ×5,
  `svc_encode_slice.rs` ×5, `svc_set_mb_syn_cabac.rs:1080`, plus `GetBsPosCavlc`)
  becomes `slice_writer(...)`. Four have no ctx in scope — take what you reach:
  `StashMBStatusCavlc`/`StashPopMBStatusCavlc` (`svc_set_mb_syn_cavlc.rs:990`,
  `:1013`) and `WelsWriteSliceEndSyn` (`svc_encode_slice.rs:1990`) take the
  writer from their callers, which have the ctx (through
  `EntropyCoder::StashMBStatus`/`StashPopMBStatus`, `wels_func_ptr_def.rs:234`,
  `:259` — the callers already pass `slice_bs_buffer(pEncCtx, pSlice)` beside
  it); `GetBsPosCavlc` (`svc_set_mb_syn_cavlc.rs:1124`) gets it through
  `EntropyCoder::GetBsPosition` (`wels_func_ptr_def.rs:282`) — three callers
  (`rc.rs:2168`, `:2216`, `svc_encode_slice.rs:1332`).
- The field, `InitSliceBsBuffer`'s and `InitSliceList`'s `pBsWrite` parameter,
  and `ReallocateSliceList`'s re-stamp loop (`:2610–2615`) are deleted.
- **Do not add a `bool`** — a second copy of the choice is the defect in a new
  spelling. If Phase 7 makes `pBs` an `Option<Vec<u8>>`, the discriminator becomes
  `is_some()` and nothing else moves.

**(b) `SWelsSliceBs.pBsBuffer` is a cache of `pThreadBsBuffer[uiBufferIdx]` — it
dies.** Both stamp sites (`svc_encode_slice.rs:2414`, `slice_multi_threading.rs:942`)
write `pSliceThreading->pThreadBsBuffer[idx]` with the same `idx` that
`InitOneSliceInThread` stores in `uiBufferIdx` (`:2409`). Execute:

- The two reads (`slice_bs_buffer`'s own arm, `wels_task_management.rs:313`)
  resolve `(*(*pCtx).pSliceThreading).pThreadBsBuffer[(*pSlice).uiBufferIdx as usize]`
  through `bs_buffer` — the SHIM(phase3) helper stays with that one job, and its
  doc (`nal_encap.rs:98`) and `slice_bs_buffer`'s (`:1877`) say so: what is left is
  the thread pool's own buffers, **Phase 7's**.
- `SetOneSliceBsBufferUnderMultithread` (`slice_multi_threading.rs:933`) is left
  with `uiBsPos = 0`, which `InitOneSliceInThread` already did one call earlier —
  delete it and its call (S18).
- The `pThreadBsBuffer` array itself is **not** this session's (F12/P10, Phase 7).

**(c) `SWelsNalRaw.pRawData` is redundant with `iStartPos` — it dies; the caller
names the buffer.** Session A re-derived it at unload from the offset the record
already carries; the objection ("one type cannot hold offsets into two owners")
dissolves once the type holds only the offset. Execute:

- `WelsEncodeNal(raw: &SWelsNalRaw, src: &[u8], ext: Option<&SNalUnitHeaderExt>, dst: *mut u8, dst_len: i32, out_len: &mut i32) -> i32`
  reading `src[iStartPos .. iStartPos + iPayloadSize]`. Nine production callers:
  eight pass `&(*pOut).sBsBuffer[..]` (`encoder_ext.rs` ×5, `wels_encoder_ext.rs`
  ×3), `WriteSliceBs` (`slice_multi_threading.rs:827`) passes the thread buffer
  via `bs_buffer`; three tests in `nal_encap.rs` pass their local payload with
  `iStartPos = 0`. `ext` was `&mut … as *mut _ as *mut c_void` at every site — it
  becomes a shared borrow.
- `WelsLoadNal`/`WelsUnloadNal`/`WelsLoadNalForSlice` lose the stamp lines;
  `assert_size!(SWelsNalRaw, 40)` re-pinned to the measured size in the same
  commit, reason at the line.
- `dst` stays a typed raw pointer for now — `pFrameBs` is `*mut u8` and is handed
  to the API as `pBsBuf`; its ownership is a later family, not this one.

One commit per settlement; `gates.sh commit` each; `family` after (c).

### 2.3 The attribute comes off, and the walk

1. Delete `#[cfg_attr(miri, ignore)]` at `svc_encode_slice.rs:3036`. Add
   `-Zmiri-disable-isolation` to the `--lib` step in `gates.sh` with the reason at
   the line: `WelsTime()` is `SystemTime::now()` (`wels_encoder_ext.rs:227`),
   called by `EncodeFrameInternal` around every frame; it does not reach the
   bitstream. That flag disables host isolation, not aliasing or validity
   checking; the forbidden list from session A stands (`-Zmiri-disable-stacked-borrows`,
   `-Zmiri-disable-validation`, anything that weakens the checker).
2. `cargo +nightly miri test --lib -- encode_loop_runs_over_a_macroblock_grid`.
   Each red: read the report, classify (provenance / ordering / spelling /
   accommodation / deletion / Phase 7 / Phase 8), fix or park, re-run. Record
   every one as session A did — file:line, the shape, red-before and green-after
   both observed. New classes get F-numbers in `phase6_findings.md`; recurrences
   of S29/F13/F14 shapes go in the log's list.
3. **A Phase 7 blocker** (anything that needs `iMultipleThreadIdc > 1` or the
   task/thread machinery to fix): the probe gets the attribute back **naming that
   blocker and Phase 7** (S15), the finding is recorded with a reproduction, and
   the session continues with the rest of this face's done-test unmet and stated.
4. Green: `gates.sh family` — both sweeps 341/341, no encoded byte moved. Then the
   probe's name in the `--lib` Miri step's output (F17/F18: neither `--skip` matches
   it — verify by reading the output, not the filter).

### 2.4 The numbers this face owes

- **S32's frame-count clause** (owed by session A): time the encode probe under
  Miri at 2, 3 and 6 frames (temporarily edit the driver call; restore), same
  geometry. Flat → say so; scaling → write the per-frame cost into plan §7.6's S32
  amendment. Miri's `finished in` is its virtual clock (S32's amendment) — quote
  it for comparisons, convert by session A's ratio for the budget.
- **The budget statement**: `--lib` Miri before/after, in both units. The
  initialisation probe (`:3006`) is a strict subset of the encode probe by
  construction (same driver, `frames = 0`); keep both unless the encode probe is
  expensive enough that 31s matters, and say which and why. If the encode probe is
  expensive, session A's levers in session A's order — the floor is F34 and the
  test's own non-vacuity assertions (≥ 2 frames, second inter-coded, moving
  source, 3×2 macroblocks). **No decoder probe is retired** — session A measured
  that each reaches code no other reaches.

## 3. Face 2 — the `c_void` clearing

`grep -c c_void` over `src/encoder src/processing src/common` at the face's open,
by file (S24, code/prose split). Three categories; the recipe differs per category
and the done-test is that **every remaining occurrence is in the third**.

### 3.1 The `IWelsVP` shape — dissolved (S18)

`wels_preprocess.rs:583–618` (the struct and its three methods),
`processing/mod.rs` (`WelsVpInit/Uninit/Flush/Set/Get/Process/SpecialFeature`,
`WelsCreateVpInterface`, `WelsDestroyVpInterface`), 44 `m_pInterfaceVp` uses.

- `CWelsPreProcess.m_pInterfaceVp: *mut IWelsVP` → `m_vp: Box<SWelsVpContext>`
  (`processing::SWelsVpContext` already is the concrete object). Constructed where
  `WelsCreateVpInterface` was called (`:1033–1039`), dropped where destroyed
  (`:1045–1047`); the 13 `m_pInterfaceVp.is_null()` guards die.
- Each plugin's `Set(_iType, pParam: *mut c_void)` / `Get(_iType, pParam)` becomes
  a typed method — the casts inside them name the type: `SVAACalcParam`,
  `SComplexityAnalysisParam`, `SAdaptiveQuantizationParam`, `SBGDInterface`,
  `SSceneChangeResult` (`vaacalc.rs:633`, `complexity_analysis.rs:83/95`,
  `adaptive_quantization.rs:179/191`, `background_detection.rs:113/125`,
  `scene_change_detection.rs:41/53`). `Process(iType, *mut SPixMap, *mut SPixMap)`
  takes `&SPixMap`. The `iType` dispatch dies with the vtable — the caller names
  the plugin.
- `pCalcResult: *mut SVAACalcResult` inside the param structs (2.1's `:1673`,
  `:1729`, `:1754`, `:2552`) is the same take-what-you-reach shape: pass
  `&mut (*pVaaInfo).sVaaCalcInfo` at the `process` call instead of storing it, if
  the plugin only reads it during `process` — check each; `:1754` stores it in
  `pVaaInfo` itself and must not survive as a stored pointer.
- **The five unsupported methods keep their behaviour exactly.** `METHOD_DENOISE`
  (`:1548`), `METHOD_DOWNSAMPLE` (`:1592`), `SCENE_CHANGE_DETECTION_SCREEN`,
  `COMPLEXITY_ANALYSIS_SCREEN`, `SCROLL_DETECTION` return `RET_NOTSUPPORTED` today
  and each caller skips the follow-up on non-success. Keep the return value and
  the skip at each site with a comment naming the untranslated method; do not
  invent a stub plugin (S18) and do not "fix" the path (a behaviour question — the
  sweeps decide, and 341/341 holds today with all five unsupported).
- `SPixMap.pPixel: [*mut c_void; 3]` (`:263`) → `[*mut u8; 3]`. The cursor
  conversion is session F's; this only removes the erasure.

### 3.2 Typed at both ends — the cast is dead shape (S18)

Delete the erasure; the callee already casts back to one type:

- `WelsMdInterMbLoop` / `WelsMdInterMbLoopOverDynamicSlice` `pWelsMd: *mut c_void`
  (`svc_encode_slice.rs:1419`, `:1576`, callers `:1776`, `:1791`),
  `DynSlcJudgeSliceBoundaryStepBack(pCtx: *mut c_void, pSlice: *mut c_void, …)`
  (`:2064`, callers `:1336–1337`, `:1715–1716`).
- `WelsCabacInit(pCtx: *mut c_void)` (`set_mb_syn_cabac.rs:821`, caller
  `encoder_ext.rs:1585`), `set_mb_syn_cabac.rs:870`, `:1137`, `:1143`,
  `svc_set_mb_syn_cabac.rs:1087`.
- `SetBlockStaticIdcToMd(pVaa: *mut c_void)` (`svc_mode_decision.rs:2071`, `:2104`).
- `AssignMbMap*`'s `pMbMap: *mut c_void` (`svc_enc_slice_segment.rs:377`, `:419`,
  `:516`, `:545`, `:563`, `:606`) — memset targets; `_pPpsArg: *mut c_void`
  (`:635`) and its argument `pPps as *mut c_void` (`encoder_ext.rs:967`) — unused,
  delete.
- `PSetMemoryZero` / `WelsSetMemZero_c` / `_extern` (`svc_encode_mb.rs:301`,
  `encoder_context.rs:1006/1012`) and the three `pfSetMemZeroSize*` slots
  (`encoder_context.rs:662–664`), all installed with the one `_c` body and no other:
  S18 — delete the slots and the fn type, call a typed zeroing helper at the seven
  sites (`svc_encode_mb.rs:714/743/768/832/859/939`, `svc_mode_decision.rs:464`).
  `WelsSetMemMultiplebytes_c(pDst: *mut c_void, …)` (`slice_multi_threading.rs:110`,
  callers `encoder_ext.rs:2874`, `svc_encode_slice.rs:2053`, both `u16` maps) →
  typed `*mut u16`, or a `slice::fill` where the caller already has the extent.
  `assert_size!(SWelsFuncPtrList, 1160)` re-pinned in the same commit (−24
  expected: three 8-byte slots).
- `md.rs:473–477`, `:491–493` — eight `pfIntra*Combined3*: *mut c_void` fields,
  never assigned, guarded by `assert_no_combined3` (`svc_base_layer_md.rs:277`):
  S18 — delete the fields, the guard, its call sites, and the branches they
  guarded (the scalar branch is the only live one and becomes unconditional).
  `assert_size!(SSampleDealingFunc, 240)` re-pinned in the same commit (−64
  expected: eight 8-byte fields).
- `InitPic(kpSrc: *const c_void, …)` (`encoder_context.rs:538`, test `:1036`) →
  `*const SSourcePicture`; the PSNR pair `kpTarPic/kpRefPic: *const c_void`
  (`wels_common_defs.rs:267/269`, callers `encoder_ext.rs:3720–3742`,
  `wels_common_defs.rs:312/314/346`) → `*const u8`.
- `WelsEncodeNal`'s two — done at 2.2(c).

`gates.sh commit` per commit; `family` at the face's close.

### 3.3 The residue — enumerated with its owner, not converted

- **The allocator and the free cascade** — `memory_align.rs` (16) and every
  `WelsFree(x as *mut c_void, tag)` (~110 across `encoder_ext.rs`,
  `wels_preprocess.rs`, `svc_encode_slice.rs`, …). Each cast dies with its
  allocation's ownership conversion — MB arrays (C), slices/layers (D), scratch (E),
  pictures/planes (F), the context (G). **Do not genericize `WelsFree<T>`** — the
  casts are the free-cascade inventory (F19's check at 6.6) and a generic would
  hide it while converting nothing.
- **The C ABI's `void*`** — the log/trace callback (`encoder_context.rs:89–91`,
  `wels_encoder_ext.rs:235–282`, `:1572`), `SetOption/GetOption(pOption)`
  (`wels_encoder_ext.rs:1942`, `:2300`, `:2315`, `:2480`, `:2489`) — **Phase 8**.
- **MT plumbing** — `pTaskManage`, `mutexEncoderError` (`encoder_context.rs:450`,
  `:516`), `slice_multi_threading.rs` (handles, events, mutexes, `pWelsPEncCtx`),
  `wels_task_management.rs` — **Phase 7**.

Done-test for the face: the count per file with the unit, and every remaining site
in one of the three lines above; `IWelsVP` = 0 in code; `Combined3` = 0.

## 4. Face 3 — `SPicture`: the settlement, then the head

Three owners allocate pictures: the reconstruction pool (`encoder_ext.rs:752`,
`pRefList->pRef[i]`, freed `:1685`), the spatial source pool
(`wels_preprocess.rs:1100`, `m_pSpatialPic[did][i]`, freed `:1134`), and the one
scaled input picture (`:921`/`:961`). Everything else that stores `*mut SPicture`
is an alias into one of them.

### 4.1 The settlement — written into the log before the first edit

1. **The alias table**: every field or array holding `*mut SPicture`, with
   file:line, which owner it points into, and what it becomes. Start from:
   `sWelsEncCtx.{pEncPic, pDecPic, pRefPic, pRefList0[16]}`
   (`encoder_context.rs:462–468`), `SDqLayer.{pRefPic, pDecPic, pRefOri[]}`
   (`svc_encode_slice.rs:414–416`), `SRefList.{pShortRefList, pLongRefList, pRef,
   pNextBuffer}` (`encoder_context.rs:331`), `SSpatialPicIndex.pSrc` (`:397`),
   `CWelsPreProcess.{m_pSpatialPic, m_pLastSpatialPicture}`, `SScaledPicture`.
2. **S34, measured**: `WelsExchangeSpatialPictures` (`wels_preprocess.rs:1837`)
   swaps *pointers* in `m_pSpatialPic` and `m_pLastSpatialPicture` — the arrays
   permute, the pictures do not move. So a *position* in those arrays is not an
   identity; the picture's pool slot is. Grep both pools and the ref lists for
   `swap`/`rotate`/`retain`/`remove`/`sort`/`drain` and every `Exchange`; list the
   hits.
3. **The shape**: `safe/pool.rs`'s `Pool<Box<SPicture>>` per owner (stable slots
   are the identity), the arrays hold ids and permute freely, aliases become
   `Option<PicId>` (`None` where the C++ has null — F56's rule: rule the zero, do
   not default it). Two pools → two id types or one id with the pool named at
   the resolver — choose by reading how `pEncPic` (source pool) and
   `pDecPic`/`pRefPic` (recon pool) meet in `WelsEncoderEncodeExt`; write the
   choice down. **The ordering rule**: aliases become ids before either container
   owns — plan §4's 6.1-before-6.2, confirmed, not re-derived.
4. **F42's arm**: where MC/VAA read one picture and write another, `pDecPic`
   and `pRefPic` are distinct pool slots by construction — say so per site, or
   name the site that needs `classify`.

### 4.2 The head, if the boundary allows

6.1's first move: the reconstruction pool's aliases become ids — `SRefList`'s
three alias arrays, `pRefList0[]`, `pRefPic`, `pDecPic`, the layer's three —
with `ref_list_mgr_svc.rs`'s list operations as the consumers (plan §4: "zero
identity comparisons, pure index conversion"; measure that claim first with a
grep for `==` between picture pointers). Then `SRefList.pRef` becomes the pool.
Family gate at the seam. Whatever of 6.1 does not land is **named for session F**
in the close, with the table's rows marked done/not.

## 5. Gates

`gates.sh commit` per commit (build both + `--all-targets`, tests, ratchet,
census). `family` at each face's close that changed production code (2, 3, 4).
`exit` at the close. Ratchet increases are recorded, not hidden (T5b.6's rule).
Do not edit the working tree while a battery runs. **No perf work** (D-perf-6);
a session span only if a hot-path line moved (S2b) — face 3.2's memset helper
and face 4's head are the candidates; say whether either did.

## 6. Done-test, and the close

1. F52's six recorded as adjudicated in `phase6_findings.md`; the sweep skips
   declarations; the re-run count stated.
2. `pSliceBsa`, `pBsBuffer`, `pRawData` are gone; `WelsEncodeNal` is typed;
   `SetOneSliceBsBufferUnderMultithread` is deleted; the SHIM(phase3) markers name
   the thread buffers and Phase 7 and nothing else.
3. The encode probe runs **green in the `--lib` Miri step**, its name in the
   output — or the attribute is back naming a Phase 7 blocker with its
   reproduction recorded, and this line says so.
4. Every finding on the walk is recorded, red-before/green-after observed; new
   classes have F-numbers.
5. Both sweeps 341/341 in both profiles after face 1, after face 2, and at exit.
6. S32's frame-count clause is measured and written into plan §7.6; the `--lib`
   Miri budget before/after in both units; the two-probe decision stated.
7. `--skip encoder_ext`: deleted, or kept with the finding named.
8. `c_void`: `IWelsVP` and the `Combined3` fields gone; the residue enumerated
   by owner (allocator / C ABI / MT), per file with the unit.
9. The `SPicture` settlement is in the log (alias table, S34 hits, id shape,
   order); the head landed or handed to F by name.
10. `exit` battery; F3 per S14 (the hash shortcut will not apply — every face
    moves the encoder binary); log entry ≤ 40 lines from the breadcrumbs;
    `phase6.md` §6 shows B spent and C next with anything handed forward named;
    plan §0's rows and open-findings entry updated (F52).

## 7. Non-goals

No `SMB`/`MbArray` (C). No slice/layer brackets beyond 2.2's settlements (D). No
`SMbCache`, ME/MD/RC/CABAC records (E). No plane cursors (F). No context flip (G).
No `pThreadBsBuffer` or task/thread ownership (Phase 7). No `WelsFree<T>`. No
`api/` change beyond the test driver (Phase 8; F23 routed around, not fixed). No
decoder work. No perf work. No golden movement — the sweeps are the definition.
