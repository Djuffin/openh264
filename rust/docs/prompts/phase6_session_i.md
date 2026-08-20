# Phase 6, session I — the deny sweep, and the phase closes

## What this session does

Everything the encoder context reaches is owned or an id; what remains is
spelling and proof. This session: deletes one dead field, gives the dispatch
tables their owner, converts the context *parameters* to references where the
aliasing structure permits, puts `#![deny(unsafe_code)]` on every encoder and
processing module with each surviving unsafe item enumerated in a lawful
category — and closes Phase 6 against the written exit conditions
(`phase6.md` §7, restated below).

**The phase does not close around leftovers.** If the parameter conversion
(step 2) overflows the session, stop at a family boundary, report, and a
session J finishes — §7 is not bent to fit a calendar.

Five results define done (they are §7's five conditions):

1. `#![deny(unsafe_code)]` on every module of `src/encoder` + `src/processing`
   except an enumerated MT set, each exception a *file* with Phase 7 as owner.
2. Every surviving `#[allow(unsafe_code)]` item enumerated in one of four
   categories: **C-ABI** (Phase 8), **owned-storage cursor machinery carrying
   its mandated Miri tests** (the derive-twice tests exist for every accessor
   since T6.H14), **MT seam** (Phase 7), **`SCREEN_CONTENT(dormant)`**
   (Phase 10).
3. Encoder-side `raw_ptr` residue enumerated by category, code split from prose.
4. Full exit battery PASS — **including the one full unscoped Miri run this
   phase still owes** (D-gate-2: every other run stays `MIRI_SCOPE=encoder`) —
   and the cumulative perf position restated.
5. The handoffs written into the plan: Phase 7, 8, 9, 10 (the lists below).

## Ground rules (self-contained)

- **Gates**: `gates.sh commit` per commit, `family` per step, the full `exit`
  at the close. Sweeps read **369/369 per profile** (`st mt def sl ltr`).
- **Miri**: `MIRI_SCOPE=encoder` (252 tests, ≈600 s) for every run **except the
  final phase-close battery**, which runs the full unscoped step once (349
  tests, ≈1411 s at G's close) — the handoff gate.
- **Perf**: one span (7 pairs + fresh null, `rust/tools/perfpair.py`); on a
  breach, **attribute commit-by-commit before acting** — two sessions running,
  the first hypothesis was wrong and attribution found the real contributor.
  Then the close restates the cumulative position: it stands at ≈ **+15…+17%**
  encode against D-perf-4's +25% median tripwire; D-perf-6's recovery is
  Phase 9's and this session only *reports* the number.
- **Anchors, not surfaces**: every count and line number below was read
  2026-08-20 at H's close — re-grep before acting.
- **Pins**: layout changes re-pin `assert_size!`/`assert_ctx_offset!` in
  `src/encoder/abi_guard.rs` in the same commit, both profiles where split.
- **The `&mut`-from-raw trap governs step 2**: if a caller holds `*mut
  sWelsEncCtx` and creates `&mut` from it for a callee, the caller's raw
  pointer is popped the moment the `&mut` exists — using it after the call is
  UB. So conversion proceeds **from the call-tree root down** (the api entry
  holds the `Box`; each `&mut` derives fresh from the owner), never
  bottom-up, and never `&mut *pCtx` at a call site whose caller keeps using
  `pCtx` (the silent-reborrow trap: it compiles, no lint sees it, only Miri
  does).
- **Sweep flake protocol**: the known signature (`mt`, `sm=3`, `t` ∈ {2,4},
  **wrong output length — since measurement 85 that includes short streams,
  not only empty ones**) → re-run that exact configuration 5×; byte-identical
  every time → record as an F3 measurement in `rust/docs/phase0_findings.md`,
  continue. Anything else is yours.
- The C++ is the behavioral reference; byte parity is the gate.

## The map (verified 2026-08-20, H's close)

**`pPSOVector` is dead**: 4 occurrences tree-wide — the field declaration, its
`new()` initializer, and the equality instrument's field list. Never written,
never read. H measured this and left it only because deleting a context field
re-pins the offset asserts and its exit battery was already running.

**The dispatch tables** (`SWelsFuncPtrList`):
- The context's last unowned member: `pFuncList` allocated at
  `encoder_ext.rs:1448` and `:1620` (two sites), freed at `:1817` — the three
  "session I" allocator hits. One `mem::zeroed` `Default` in
  `wels_func_ptr_def.rs`; the `init_fills_*` tests
  (`init_fills_sad_and_satd_and_clears_combined3`,
  `init_fills_every_slot_the_md_layer_indexes`,
  `init_fills_every_reconstruction_slot`) are the existing instruments for a
  field-wise constructor.
- **The tables are re-written mid-stream**: `SetFastCodingFunc` /
  `SetNormalCodingFunc` (`encoder_ext.rs:2233`/`:2243`, called per frame at
  `:2298`/`:2300`) and `WelsInitSampleSadFunc`
  (`encoder_context.rs:1457`, `svc_mode_decision.rs:2444`) swap slot values on
  complexity switches. So: the three writer functions take `&mut
  SWelsFuncPtrList`; every reader takes `&SWelsFuncPtrList` **derived per call**
  — readers copy `Option<fn>` values out and hold no cursor into the table
  across calls (verify with the type-driven checker session E built; if it
  finds a holder, that one site stays raw and is enumerated).
- 57 `*mut SWelsFuncPtrList` parameter sites re-spell with the above.

**The context parameters**: 297 `*mut sWelsEncCtx` lines, by file — rc 55,
svc_encode_slice 50, ref_list_mgr_svc 32, encoder_context 31, encoder_ext 28,
svc_mode_decision 21, svc_base_layer_md 15, wels_preprocess 13,
**wels_task_management 12 (Phase 7 — do not touch)**, **wels_encoder_ext 8
(Phase 8 — do not touch)**, tail in smaller files. The conversion surface is
therefore ≈270 lines. Known cross-call cursor holders that stay raw-parameter
and become allow items in category 2: the NAL-writer chain over the frame
bitstream (H enumerated its **19** cursor derivations), and any function the
step-2 audit finds holding a context-derived cursor across a call that takes
the context again.

**The 9 remaining `mem::zeroed` sites, attributed**: wels_func_ptr_def 1 (this
session, with the constructor), svc_encode_slice 2 + ref_list_mgr_svc 1 +
decode_mb_aux 1 + get_intra_predictor 3 + sample 1 — POD `Default`s and test
fixtures: each keeps its one-line soundness comment or converts if step 2 gives
its type an owned/`Option` field. Zero must remain unattributed.

**Modules**: 32 files in `src/encoder`, plus `src/processing`. Expected deny
exceptions (files, not blankets): `slice_multi_threading.rs`,
`wels_task_management.rs` — Phase 7's, enumerated in the close log.

**The handoff lists to write** (step 4; collect, verify each against the tree,
and put them in the plan's §4 phase rows):
- **Phase 7**: F61 (MT bank growth never re-stamps the slice list — the C++
  shares the defect); F3's close-by-ablation with the measurement-85 shape
  note; F12 and the thread pool; `sSliceBs.pBs` (alloc
  `svc_encode_slice.rs:2982`, free `:3011`) and the thread-buffer ownership;
  `DynamicSliceBs` (`encoder_ext.rs:1122`/`:1767`); the context's MT members
  (`pSliceThreading`, `pTaskManage`, `mutexEncoderError`, `pDynamicBsBuffer`);
  the MT files; `pMemAlign` + `common/memory_align.rs`'s death once the MT
  allocations go; E's one MT `*mut SMB` site.
- **Phase 8**: `wels_encoder_ext.rs` internals (its 8 ctx lines included); the
  `c_void` C-ABI line; the decoder-side items already listed there (F23, F37,
  F41, the api inventory). Note in the row: **the encoder's `pSvcParam` is
  context-owned (H's verdict) — Phase 8 inherits nothing there.**
- **Phase 9**: the SAD/SATD third park (1.30–5.68x / 1.36–4.05x, kernel
  signature needed); `SMbCache`'s 72 kernels-take-slices sites; F5's ~10
  side-array resolutions; **H's measured lead — `Vec`-backed accessors cost
  where `Option<Box>` ones are free** (the mechanism named:
  `WelsRcMbInitGom` per macroblock opens with `ctx_rc_at`; cost localised to
  {LTR, rate control, ref lists}); D-perf-6's recovery with cumulative
  restated; the two `pMvdCost` record fields.
- **Phase 10**: the `SCREEN_CONTENT(dormant)` tag census (16 items + the 8
  allocator hits H's census added).

## Step 0 — `pPSOVector` is deleted

**Result**: the field, its initializer, and its instrument row are gone (dead
code is deleted, not converted); the context's size and all offset pins
re-measured once, before anything else moves them.

**Check**: `grep -rn 'pPSOVector' --include='*.rs' src/` reads 0;
`gates.sh commit`.

## Step 1 — the dispatch tables own and re-spell

**Result**: `pFuncList: Box<SWelsFuncPtrList>` built by a field-wise
constructor (the `init_fills_*` instruments prove it; the zeroed `Default`
dies); the free entry at `:1817` is gone; the three writers take `&mut`, the
readers take per-call `&`; the 57 parameter sites re-spelled; allocator hits in
`src/encoder src/processing` read **Phase 7's four only**.

**Check**: `grep -rn 'WelsMalloc\|WelsMallocz\|WelsFree' --include='*.rs'
src/encoder src/processing` → 4 hits, all Phase 7's;
`grep -rn '\*mut SWelsFuncPtrList' …` → 0 outside enumerated survivors;
`gates.sh family`.

## Step 2 — the context parameters, root-down

**Result**: every function on the conversion surface takes `&mut sWelsEncCtx`
(or `&` where it only reads), **or** is enumerated as a survivor with its
blocker named — cross-call cursor holder (category 2), MT (Phase 7), boundary
(Phase 8). No caller anywhere keeps a raw context pointer live across a callee
that takes a reference.

**Do**: start at the api entries that hold the owner
(`WelsInitEncoderExt` / `WelsEncoderEncodeExt` / the `wels_encoder_ext.rs`
boundary keeps its raw handle — the reference is *born* at that boundary,
derived from the `Box`, once per call). Convert closure-by-closure downward
(the reachability closure is the commit unit). Per function, the audit is two
questions: does it hold a context-derived cursor across a call that reaches
the context again, and does it re-reach the same object through two
parameters? Two nos → reference; any yes → survivor, enumerated.

**Check**: `grep -rn '\*mut sWelsEncCtx' --include='*.rs' src/encoder
src/processing` reads only the enumerated survivor list + the two untouched
files; `gates.sh family`; sweeps 369/369.

## Step 3 — the deny sweep

**Result**: `#![deny(unsafe_code)]` at the top of every `src/encoder` and
`src/processing` module except the enumerated MT files; every surviving
`unsafe` item carries `#[allow(unsafe_code)]` with a one-line category tag
(the four categories above); the counts recorded — items per category per
file.

**Do**: module-by-module, smallest first; strip-and-build names what the
compiler still needs. An `unsafe fn` whose signature names a raw pointer stays
`unsafe fn` — that is its correct spelling, allowed and categorised, not
converted by force.

**Check**: `grep -rn 'deny(unsafe_code)' src/encoder src/processing | wc -l`
= module count minus the MT exceptions; `grep -c 'allow(unsafe_code)'` per
file matches the log's enumeration; ratchet re-baselined with the shape
explained.

## Step 4 — §7's checklist, the residue, the handoffs

**Result**: each of §7's five conditions checked line by line in the log with
its evidence; the `raw_ptr` residue enumerated by category with the code/prose
split; the four handoff lists (the map above, verified) written into the
plan's Phase 7/8/9/10 rows; the cumulative perf position restated against
D-perf-4 and D-perf-6.

## Step 5 — the phase closes

1. The span, then the **full unscoped exit battery** — Miri `--lib` complete
   (all four encoder probes and all three decoder probes green by name), both
   sweeps both profiles, both benches, ratchet, census.
2. The phase-close log entry: Phase 6's whole arc in numbers — encoder-side
   `raw_ptr` from 2668 at the phase's open to the close's reading, `unsafe_fn`
   likewise, the allow-item enumeration, the finding ledger (F57–F64), the
   sweep growth 341 → 369, the perf arc with every session's span.
3. Plan §0: the **Phase 6 COMPLETE** row (model: Phase 5b's), session I marked
   spent, **Phase 7 named next**; phase6.md §6's I row; `perf_baseline.md`.

## Stays untouched — the boundary list

| what | why | whose |
|---|---|---|
| `slice_multi_threading.rs`, `wels_task_management.rs`, the MT context members, `sSliceBs.pBs`, `DynamicSliceBs`, `pMemAlign` + `memory_align.rs` | thread machinery | Phase 7 |
| `wels_encoder_ext.rs` internals (8 ctx lines included) | ABI boundary | Phase 8 |
| measured parks and settlements: SAD/SATD, `SMbCache` 72, `*mut SMB` 48's named groups, per-MB plane cursors, the two `pMvdCost` fields, {LTR, rc, ref-list} accessor cost | Phase 9 owns each with its number | Phase 9 |
| everything `SCREEN_CONTENT(dormant)` | live upstream, fenced | Phase 10 |
| behavior: no byte may move; no parity deviation beyond the recorded ones | the gate | — |

## Done-test

§7's five conditions, each with evidence in the log — the deny grep, the
category enumeration, the residue split, the full battery PASS with the
unscoped Miri run, the handoffs present in the plan's §4 rows — and the
phase-close row in plan §0. If any condition cannot be met this session: the
phase stays open, the shortfall is enumerated, and session J's scope is the
report's last section.
