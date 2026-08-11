# Phase 4b, session B — the two strategy objects, 4a's leftovers, and the phase exit

> **This is the rewritten phase brief** (2026-08-11, after session A closed §1).
> Session A's record is the log's Phase 4b session A entry; the session-A-era text of
> this file is in git history (`9f88d768` vintage). Seam numbering: **T4b.1/T4b.1b are
> done** (`08b7c29d`, `3e583b9a`); this session is **T4b.2** (both strategy objects),
> **T4b.3** (4a's leftovers), and the **phase exit — never compressed**: if T4b.3 runs
> long, the exit moves whole to a session C.

**Governing:** [`safety_refactor_plan.md`](../safety_refactor_plan.md) — §0 for where
the port stands, **§7.6 for S1–S23b** (S20/S21 govern every struct edit here; **S23 and
S23b are session A's and both bind today**), §7.4 for D-perf-4. Read the **session-A log
entry** in full before converting anything — its §1 (the S23 story) and §2 (the three
call sites that did not substitute) are this brief's method, demonstrated. This file
scopes the session and supersedes on disagreement; fix disagreements in place.

**The running tally:** `assert_size!(SWelsFuncPtrList)` is at **1184** (from 1272 at
4b's entry; 1280 in C++ — 4a took 8 first). Every remaining de-virtualization moves it
or the `sWelsEncCtx` assert, and the comment above each assert records why, each time.

---

## 0. Start here

Tree at session start: clean **except a four-hunk inherited doc tail** (steward
reconciliation of session A's bookkeeping: plan §0's ratchet row rewritten, the
1272-vs-1280 tally, a gates-row stamp, and two `T4b.2`→`T4b.1b` battery labels in
`phase0_findings.md`). Commit it first, house style.

Then the control battery (`bash rust/tools/gates.sh full`, `OVERALL:` is the verdict)
and recount — **431 debug / 425 release / 20 ignored**, Miri **291** at session A's
close. The recount rule has paid thirteen times.

**F3, per S23b — the protocol changed at session A and this is the first brief written
under it.** The signature is unchanged (`mt`, `sm=3`, `t∈{2,4}`, wrong-length output of
any form — zero, short, and long are one signature). What changed:

* A session-start hit is a *tendency*, not a rule (session A's opening battery was
  clean). Expect nothing; apply S14 to what arrives.
* Single hit → re-run that configuration. **Escalation is an alternation at the sweep
  level**: whole `mt` presets, 341 configurations back to back, both binaries built
  once and swapped inside one loop. **Do not alternate isolated configurations** — the
  race needs a loaded machine, session A measured 0/80 in isolation against 9 hits at
  sweep level on the same day, and a 0/0 alternation has not run yet (S23b).
* Calibration, now measured rather than inferred: **~1 in 800 configurations under
  load**. In twelve sweeps per side expect a handful of hits *on both sides*; the
  question an alternation answers is whether HEAD is *worse*, not whether hits exist.

---

## 1. T4b.2a — the parameter-set strategy is not dispatch at all (seam)

**The scouted fact that rewrites §2's plan:** `IWelsParametersetStrategy` has **one
ported implementor** — `CWelsParametersetIdConstant` (`paraset_strategy.rs:156`);
`CreateParametersetStrategy` returns an explicit error for the other four C++
strategies (module doc, `paraset_strategy.rs:7-8`). A vtable with one implementor is
a concrete type wearing a costume. So this face is **deletion, not design**:

* **Delete `IWelsParametersetStrategyVtbl`** — all **20** entries (measured; the
  taxonomy's 16 was a miscount, corrected at Phase 3's exit) — the static vtable
  instance, and the per-entry `unsafe extern "C"` thunks. The associated-fn wrappers
  (`IWelsParametersetStrategy::Method(pThis, …)` shapes) become **inherent methods on
  `CWelsParametersetIdConstant`**. Keep the Wels name (P14).
* **The field**: `SWelsFuncPtrList.pParametersetStrategy: *mut IWelsParametersetStrategy`
  (`wels_func_ptr_def.rs:401`) becomes **`Option<Box<CWelsParametersetIdConstant>>`**.
  This is the S21-sound shape: the null niche makes all-zero a valid `None`, so the
  `WelsMallocz`/`mem::zeroed` construction paths stay sound — **discharge the audit by
  stating exactly that in the commit message**, and verify `Box`'s ownership matches
  today's: the object is already `Box::into_raw`'d at creation
  (`encoder_context.rs:725`) and `Box::from_raw`'d in its `Destroy`, so `Drop` is the
  honest owner and **`Destroy` call sites become scope ends / `take()`** — R4 in
  miniature, third instance. The `WelsUninitEncoderExt` entry for it dies in the same
  commit. The null check at `encoder_context.rs:731` becomes the `Option` it always was.
* **S20 first**: the field stays 8 bytes (`Option<Box<_>>` is pointer-sized), so
  `assert_size!(SWelsFuncPtrList, 1184)` (`abi_guard.rs:168`) should not move — **state
  that in the closure computation rather than discovering it**; if the compiler
  disagrees, the closure was bigger than computed and the commit re-sizes per S20.
* **S23, mandatorily**: the strategy is selected/created at init from param state.
  Before treating anything about it as derivable, **read every update path**
  (`WelsEncoderParamAdjust` both arms included) and answer per field whether the
  installed object can lag the live parameters. Session A got opposite answers for two
  fields of one struct; do not assume this struct's answer.

## 2. T4b.2b — the reference strategy becomes an enum over `pCtx` (seam)

**Three implementors, and the port has already done half the work**: the C++
subclasses `CWelsReference_TemporalLayer/_Screen/_LosslessWithLtr` all carry exactly
one data member, `m_pEncoderCtx`, so the port merged them into **one object struct**
(`CWelsReferenceStrategyObj`, `ref_list_mgr_svc.rs:~1594`) with **three static
vtables** (`TEMPORAL_LAYER_VTBL` :1651, `SCREEN_VTBL` :1669, `LOSSLESS_WITH_LTR_VTBL`
:1697). Only the vtables differ. Therefore:

* **`enum RefStrategyKind { TemporalLayer, Screen, LosslessWithLtr }`** with the seven
  methods (`Destroy`, `BuildRefList`, `MarkPic`, `UpdateRefList`, `EndofUpdateRefList`,
  `AfterBuildRefList`, `Init`) as `#[inline]` `match`es — the T4b.1 pattern verbatim.
  Check every call-site shape against the session-A log §2 before substituting: any
  site that is not a plain `if let Some → call` gets its equivalence argument written
  down.
* **The back-pointer dies.** Every call site already holds `pCtx` and reaches the
  strategy *through* it (`(*pCtx).pReferenceStrategy as *mut IWelsReferenceStrategy` —
  e.g. `ref_list_mgr_svc.rs:808`, `:1325`), so the methods take `pCtx` as a parameter
  and `m_pEncoderCtx` — a stored context back-pointer, T8's shape — is deleted rather
  than converted. The object struct, the three static vtables, the ~21 thunks, and the
  `Box::into_raw`/`from_raw` pair all go with it; nothing is left to free, so the
  free-cascade entry dies too.
* **The field**: `sWelsEncCtx.pReferenceStrategy: *mut c_void` (`encoder_context.rs:452`)
  → `eRefStrategy: RefStrategyKind` (name it for what it is per S23's naming rule —
  and if the update-path read says the installed kind can lag the live parameters,
  the name is `eInstalledRefStrategy` and the lag gets a doc comment, the
  `eInstalledMode` precedent). **S20**: this changes `sWelsEncCtx`'s size — the assert
  at `abi_guard.rs:179` updates *in the same commit* with its comment extended
  (session A's precedent: update, don't delete, while the struct is still C-shaped),
  and the closure enumeration names anything embedding `sWelsEncCtx` by value.
* **S23 for the selector**: read where the kind is chosen and every path that could
  change the choosing parameters without re-running the chooser. Same drill as 2a.
* **MT note**: strategies are per-encoder, called from the main encode path, not
  thread-crossing — verify with a grep for uses inside the task/pool files and state
  the result; touch nothing in the pool (F12/P10, Phases 6/7).

### Face gates (each of 2a, 2b separately)

Full battery; sweeps 341/341 both profiles are the referee (reference-list and
paramset-id behaviour is stream-visible — LTR configurations in the sweep matrix are
the sensitive rows); decoder goldens must not move; one interleaved pair both benches
per seam (S2b in force: **medians, not rows** — a screaming row gets more pairs before
it gets a mechanism); Miri; ratchet (expect `unsafe_fn` and `raw_ptr` to *fall* this
time — the thunks and both raw fields go; say the numbers).

---

## 3. T4b.3 — 4a's leftovers, and the standing order: **start at `transmute`**

**`transmute` is 23 and has not moved since Phase 0** — 19 in `decode_slice.rs`, 2 in
`decoder_core.rs`, 2 in `encoder_context.rs` (counted 2026-08-11; recount). Two 4b
seams have now passed it by, which is exactly what this brief's earlier version warned
would happen. So the order inside this seam is fixed: **transmutes first, while the
session is fresh; the rest in descending value; the filler last.**

1. **Inventory all 23 first**, classify each: *data pun* (cache-fill reinterpretation —
   the `decode_slice.rs` bulk; replacement is `from_ne_bytes`/byte ops per P7/T7, same
   codegen) vs *fn-pointer type erasure* (replacement is a direct call or a typed slot —
   the dispatch work of this phase makes several of these simply deletable). Convert
   what is mechanical; anything that is not, list with its owner phase and the reason.
   The count at session end goes in the log — **this is the one ratchet metric whose
   movement is the session's headline**.
2. **The decoder intra-pred *mode* tables** — 4a converted the kernels, not the mode
   selection that indexes them.
3. **`sBlockFunc`** — the deblocking block-dispatch pair.
4. **The expand fn-pointer re-wraps** — `pfExpandLumaPicture`-class slots 4a rewrapped
   rather than removed.
5. **Filler, only at a seam boundary with time left**: the encoder's ~55 CPU-dispatch
   members that select between identical `_c` functions, including
   `WelsInitSampleSadFunc` (deleting it unblocks nothing but cleans the F13-adjacent
   struct). Low value, low risk; do not let it displace 1–4.

Gates per landing as in §2. S23 applies to any of these that caches a selector.

---

## 4. The phase exit (or session C whole; never compressed)

1. **Straggler sweep (S18)**: every vtable-era name (`pVtbl`, `…Vtbl`, `Create…Strategy`
   as a factory-of-vtables), every `Option<fn>` slot in the converted families, every
   `pf*` member this phase touched — converted, deleted-dead, or listed with its owner
   phase. The two `SHIM(phase3)` markers are Phase 6's and should still be exactly 2.
2. **Full perf protocol**: 3-pair interleaved medians both benches, whole phase —
   entry is **`6e15c907`** (session A's log fixed this; the brief-vintage number was
   wrong once already). **S2b is in force**: a median outside the null band gets more
   pairs before it gets a mechanism; two pair-counts disagreeing in sign means the
   effect is below the floor, and the disposition is diagnostic-only. Ledger per
   D-perf-4; cumulative should still read ≈ +8.9% encoder, ≈ +17.8/+10.1/+9.6% decode.
3. **Bookkeeping**: §0 refreshed (Phase 4b complete; next = Phase 5 per D-seq-1);
   Progress appendix checkboxes with hashes; findings reconciled (F3's running totals;
   anything new per S12); the `SWelsFuncPtrList` and `sWelsEncCtx` assert-comments
   read as the phase's ledger — quote the final numbers in the log.
4. **S19 — write `prompts/phase5.md`**, the hand-off for the plan's first pivot. Its
   staged contents, all measured facts on record: the 5.1–5.6 order from plan §5 with
   the exit-gate annotations added at Phase 3's exit (T3.0's goldens gate the phase;
   T7 stays deferred); **S20 computed first for `SDqLayer`** (plan 5.2 note) and the
   `MaybeUninit` shell fact for `decoder_core.rs` (plan 5.5 note — the decoder context
   embeds its buffers by value, `decoder_context.rs:769` is the existing shell);
   `SDqLayer::pBitStringAux` (`*mut BsReader`) retires at 5.2 and `cabac_decoder.rs`'s
   `SHIM(phase5)` accessor with it; the decoder's `SHIM(phase2)` kernel adapters retire
   as 5.2–5.4 convert their callers; the downgraded decode ledger rows are the phase's
   perf debt to collect (BaseMC dimensions become static); whatever `transmute` count
   survives T4b.3 and where it lives. Estimated 9–12 sessions — say per-session scope
   is the S20 closure, not the file. Stamp this brief superseded-historical.

## 5. Non-goals

No Phase 5 or 6 pulls: `SDqLayer`, `SMbCache`, the picture pool, `SSlice` layout, the
thread buffer pool, `SWelsSliceBs` — shells and comments instead. No re-opening the
parked families (second dated verdict; 6.3's third attempt is at caller conversion).
**No "fixing" the RC lag** — `eInstalledMode`'s divergence from `iRCMode` is upstream
behaviour preserved on purpose (S23/S6); a test pinning the lag is welcome if it is
cheap, otherwise it is Phase 6.5's, with `rc.rs`. No fixing F8/F9/F11-class arithmetic
(S6). No `get_unchecked`, ever (S8). No golden movement. No pool/threading edits. And
the exit is not compressible: if the clock says choose between finishing T4b.3 and
doing the exit properly, T4b.3 finishes at a seam boundary, the exit becomes session C,
and the hand-off says so.
