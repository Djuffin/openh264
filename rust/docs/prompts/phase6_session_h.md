# Phase 6, session H — the context's members own their memory

## What this session does

The encoder context (`sWelsEncCtx`, `encoder_context.rs:469`) has a constructor
since session G, but its members are still manual allocations: `RequestMemorySvc`
(`encoder_ext.rs:979`) takes ~13 blocks from the custom allocator (`CMemoryAlign`,
reached through `ctx.pMemAlign`) and `WelsUninitEncoderExt` (`encoder_ext.rs`,
≈:1750) frees them one `WelsFree` at a time.

This session replaces each allocation with an owned container (`Vec`/`Box`) built
where the malloc was, and each free-cascade entry with the container's own `Drop`
— so that at the close, **the only allocator calls left in
`src/encoder src/processing` are the multi-threading ones** (Phase 7's), and the
free cascade is a fraction of its size.

Four results define done:

1. `RequestMemorySvc` allocates nothing except the enumerated Phase-7 set.
2. The free cascade contains only that set, and every converted member has a
   **named live drop path** (see "the leak check" below).
3. `mem::zeroed` in `src/encoder src/processing` reads only the enumerated
   survivors (session I's dispatch tables, plus instruments and POD test
   fixtures).
4. The Miri step runs **encoder-scoped** (the new knob), and the gates pass.

**Execution order is the section order. If the session runs short, drop whole
steps from the end** (step 3, then step 2) and say so in the log. Steps 0–1 are
the core and do not drop.

## Ground rules (self-contained)

- **Gates**: `bash rust/tools/gates.sh commit` in every commit, `gates.sh family`
  at each step's close, one `gates.sh exit` at the session close. The
  differential sweeps read **369/369 per profile** (presets `st mt def sl ltr`).
- **Miri runs once, at the session close, inside the exit battery — never
  between steps — and from this session on it is encoder-scoped** (decision
  D-gate-2): the three decoder probes and the decoder-module tests do not run in
  Phase 6 sessions. Step 0 builds the knob. The full unscoped step (349 tests,
  ≈1411 s) runs once more in this phase, at session I's close.
- **Perf**: exactly one stream measurement — 7 pairs against the HEAD this brief
  is committed at, plus a fresh 7-pair null band, via `rust/tools/perfpair.py`.
  If the span breaches its null: **attribute commit-by-commit before acting** —
  session F's planned revert was aimed at the wrong step and attribution found
  both the real contributor and a better remedy. The foreseeable risk here is
  cursor re-derivation cost on `pFrameBs` and the MVD table; the proven
  mitigation is deriving once per frame/slice where the C++ already does.
- **Every count and line number below is an anchor, not a surface** — re-grep
  before acting on any of them (they were read 2026-08-20, at session G's
  close).
- **Layout changes re-pin their asserts in the same commit**: the context's
  `assert_size!` and all 15 `assert_ctx_offset!` pins live in
  `src/encoder/abi_guard.rs`; measure the new numbers (both profiles where they
  split) and pin them. Deleting an assertion is a defect.
- **Constructor equality is proved by value, never by byte** (F64, session G):
  an `Option` field's `None` writes only its tag — payload bytes stay undefined
  whether or not the discriminant borrowed a niche — and `repr(C)` padding is
  undefined the same way. The working instrument is two-tier: byte-compare the
  fields whose bytes are fully defined, value-compare the rest, report the
  padding count. **The template is in the tree**:
  `ctx_new_reproduces_the_zeroed_shell` (`encoder_context.rs:1377`).
- **Root accessors must be retag-stable** (F63, session F): an accessor spelled
  `self.buf.as_mut_slice().as_mut_ptr()` takes `&mut` over the whole allocation
  and **pops the pointer the previous call handed out** — and consumers here ask
  repeatedly. The correct spelling reads the address without forming a
  reference; **copy `PaddedPlane::root_ptr` (`safe/plane.rs`, fixed at session
  F's close) verbatim** — its property is that repeated calls are sibling
  derivations, neither invalidating the other. Every new accessor gets the
  covering Miri test that property implies: **derive twice, then use the first
  cursor**.
- **Zeros are ruled, not defaulted**: where the C++ `WelsMallocz`'d (zeroed) a
  block, the owned container is built zero-filled and that is faithful; where it
  used plain `WelsMalloc` (uninitialized — `pFrameBs` is one), the owned `Vec`
  is still zero-filled, which is a **deviation to record in the commit**: sound
  because every read of that buffer sits behind a write cursor, and the safe
  container has no uninitialized alternative.
- **Sweep flake protocol**: if exactly one sweep configuration fails with the
  known signature — `mt` preset, `sm=3`, `t` of 2 or 4, wrong output length,
  either profile — re-run that exact configuration 5×. Byte-identical every
  time → record it as an F3 measurement in `rust/docs/phase0_findings.md` and
  continue. Anything else is yours: stop and fix. (If a retry loop must run:
  a tight loop under load is not a quiet run, and zsh does not word-split
  `set -- $cfg`.)
- The C++ tree is the behavioral reference; byte parity against it is the gate;
  its allocation *pattern* is not a constraint.

## The map (verified 2026-08-20)

**Two functions are the whole work list.**

- `RequestMemorySvc` (`encoder_ext.rs:979`) — read it top to bottom; every
  `WelsMalloc`/`WelsMallocz` in it is a row in the table below.
- `WelsUninitEncoderExt` (`encoder_ext.rs`, the function containing the
  `WelsFree` run at ≈:1766–:1830 and beyond — **read it to its end**) — the
  free cascade. Two entries in it are **already converted and are the recipe to
  copy**: `pVpp` (`drop(Box::from_raw(…))`, session F) and `pOut` (one `drop`
  replaced four `WelsFree` calls — "the three buffers are `Vec`s that free
  themselves, and the struct came from `Box::into_raw`"). A third entry was
  *deleted* rather than converted when its allocation stopped existing — that
  is what most entries here will do.

**The leak check (run per member, cite in the commit).** An owner that exists in
source but on no live path is a leak wearing a destructor — this exact codebase
leaked ~1.2 KB per teardown that way (the paraset strategy, found at T4b.2a).
For every member converted: name the function whose execution actually frees it
today, and the drop path that replaces it. For the context the live path is the
api teardown: `WelsUninitEncoderExt` ← the encoder handle's destructor in
`wels_encoder_ext.rs`. Follow it once, cite it once, and every member after
rides the same citation.

**The member table** (allocation order; anchors from G's close):

| member | alloc | free entry | becomes | notes |
|---|---|---|---|---|
| `pStrideTab` | `:269` (struct) + `:322` (one inner block, tag `"pBase"`) | `:1766–:1771` (frees inner first) | `Box<SStrideTables>` **with the inner block owned too** | the struct stores per-layer cursor tables (`pMbIndexX`/`pMbIndexY`, read at `:579–:580`) that all point into the one `"pBase"` block — an arena in miniature: keep one owned `Vec`, make the per-layer fields offsets or root-derived, never per-field `Vec`s that would change what a cursor may reach |
| `pSpsArray` / `pSubsetArray` / `pPPSArray` | `:844` / `:853` / `:867` | `:1801`–`:1809` | `Vec<SWelsSPS>` / `Vec<SSubsetSps>` / `Vec<SWelsPPS>` | session G's `SpsId`/`PpsId` accessors resolve through these — retarget them to the `Vec` roots with the retag-stable spelling |
| `pDqIdcMap` | `:883` | `:1775` | `Vec<SDqIdc>` | plain POD rows |
| `pFrameBs` | `:1107` (**`WelsMalloc` — uninitialized**) | `:1791` | `Vec<u8>` + a retag-stable root accessor | **an arena of bytes**: NAL writers hold cursors into it across calls (consumers: encoder_ext 22, svc_encode_slice 6, wels_encoder_ext 4, encoder_context 3, **slice_multi_threading 4 — an MT file; its edits are the one-line field-spelling kind, recorded, bodies untouched**). Enumerate every cursor derivation *before* converting; the conversion must add zero `&mut` on that path. The zero-fill deviation is recorded (ground rules) |
| `pDynamicBsBuffer[…]` | `:1121` | `:1796` | **stays — Phase 7** | thread bitstream buffers |
| `pLtr` | `:1158` | `SLTRState` entry | `Vec<SLTRState>` | one per dependency layer |
| `pWelsSvcRc` | `:1177` | its entry calls `WelsRcFreeMemory(pCtx)` **first** | `Vec<SWelsSvcRc>` **taking its inner allocations with it** | the per-layer `pTemporalOverRc` blocks hang off the array — the free order in the cascade documents the ownership; the inner blocks become owned fields of `SWelsSvcRc` in the same commit, and `WelsRcFreeMemory` dies. `rc.rs`'s own header attributes its 16+3 H-tagged lines (T6.G4) — they convert here |
| `ppRefPicListExt` | `:1208` | its entry | `Vec<Box<SRefList>>` | the `SRefList`s are Box-built since session F; the list becomes the owner |
| `ppDqLayerList` | `:1216` | its entry (the `SMB`-list comment marks the spot) | `Vec<Box<SDqLayer>>` | the layers are Box-built since session D; `current_layer` (session G's accessor) retargets to the `Vec` root, retag-stable |
| `pMvdCostTable` | `:1256` | `:1815±` | `Vec<u16>` + the `(row root, bias)` accessor | this is plan item **P11** finally landing. The two record fields that cache row roots (`SWelsMD.pMvdCost`, `SWelsME.pMvdCost`, set at `svc_encode_slice.rs:1418` and copied by `InitMe`) **stay `*mut u16`** — they now derive from the owned table's root accessor; enumerate them for session I's inventory |
| `pFuncList` | `:1458`, `:1642` (two sites) | its entry | **stays — session I** | the dispatch tables convert with the deny sweep |
| `pVaa` | (preprocess init) | its entry | `Box<SVAAFrameInfo>` | the pointee owns its arrays since session F; `Create`/`Destroy` fold into `new`/`Drop` |
| `pSvcParam` | **not in `RequestMemorySvc` — find the allocation first** | its entry | **the read decides**: if allocated on the context's behalf → `Box<SWelsSvcCodingParam>`; if the api object owns the block and the context aliases it → **leave raw, enumerate for Phase 8** | this is the decoder's F41 shape (the context's param pointer aliasing the api object's live block); do not guess — read, and write the verdict in the log either way |

**The remaining `mem::zeroed` sites** (20 in `src/encoder` + `src/processing`,
by file: svc_encode_slice 5, ref_list_mgr_svc 3, get_intra_predictor 3,
wels_func_ptr_def 2, encoder_context 2, sample 1, rc 1, paraset_strategy 1,
nal_encap 1, decode_mb_aux 1). The rule for step 2: **a type gets a field-wise
constructor only if it holds — or gains in this session — an owned or `Option`
field**; a zeroed all-POD `Default` is sound and *stays*, with a one-line
comment saying why. Known members of each group: `SSliceHeader` and
`SSliceHeaderExt` (svc_encode_slice) go with this session's work; the
`wels_func_ptr_def` pair is session I's; `ref_strategy_zero_is_the_default_arm`
is an instrument and stays; the `get_intra_predictor`/`sample`/`decode_mb_aux`
sites are POD test fixtures.

## Step 0 — the Miri scope knob

**Result**: `gates.sh` can run its Miri `--lib` step encoder-only; this session
and session I use that mode for every Miri run except I's final full battery.

**Facts**: the step's skip list is `MIRI_SKIPS=(--skip wels_thread_pool)` at
`gates.sh:345`, applied at `:366`. Decoder tests live under the `decoder::`
module path; the three decoder probes are `--lib` tests — get their exact names
from `cargo test --lib -- --list` (cheap; do **not** list under Miri).

**Do**: add an env knob (e.g. `MIRI_SCOPE=encoder`) that extends the skip list
with `--skip decoder::` plus the decoder probes' names if any sit outside that
path. Document it in the script header.

**Check**: run the scoped step once; read the test list out of its output — the
four encoder probes must appear **by name**, no `decoder::` test may. Record the
scoped count and wall time beside the unscoped 349 / 1411 s in the log.

## Step 1 — the members own, in allocation order

**Result**: every table row above lands; the free cascade shrinks to the Phase-7
set; the context's constructor builds what `RequestMemorySvc` used to malloc.

**Do, per member, as one commit-group**:
1. Constructor side: the owned container built (zero-filled) where the malloc
   was; the null-check-and-fail branch dies with it.
2. Free side: the cascade entry deleted (the container drops itself); the
   member's inner allocations — `pStrideTab`'s `"pBase"`, `pWelsSvcRc`'s
   `pTemporalOverRc` — converted in the same group, never left dangling behind
   an owned parent.
3. The leak check citation (see the map).
4. Consumers re-anchored: id/accessor resolutions retarget to the `Vec`/`Box`
   roots with the retag-stable spelling; cursor-handing accessors get the
   derive-twice-use-first Miri test.
5. Sizes and pins re-measured, same commit.

Order: exactly the table's (it is `RequestMemorySvc`'s own order), with the two
audited members — `pFrameBs` (cursor enumeration first) and `pSvcParam` (the
ownership read first) — allowed to move later in the sequence if their audits
run long.

**Check per member**: `gates.sh commit`. **Per the step**: `family`; both sweeps
369/369; `grep -rn 'WelsMalloc\|WelsMallocz' src/encoder/encoder_ext.rs` reads
only `pFuncList` (I's), `DynamicSliceBs` and the MT resources (Phase 7's).

## Step 2 — the zeroed `Default`s that now lie

**Result**: every type that holds or gained an owned/`Option` field has a
field-wise constructor with the two-tier equality proof (template:
`ctx_new_reproduces_the_zeroed_shell`, `encoder_context.rs:1377`); every zeroed
`Default` that stays carries its one-line soundness comment.

**Check**: `grep -rn 'mem::zeroed' src/encoder src/processing` reads only the
enumerated survivors, each attributed (I's two, the instrument, the POD
fixtures).

## Step 3 — the allocator census

**Result**: a table in the log attributing every remaining
`WelsMalloc`/`WelsMallocz`/`WelsFree` hit in `src/encoder src/processing` to
Phase 7 (or deleting it). `pMemAlign` survives serving exactly that set;
`common/memory_align.rs`'s death is Phase 7's to sign, but the list of what
still calls it is this session's deliverable.

## Step 4 — close

1. The span (7 pairs + fresh null; attribution before action on any breach).
2. `gates.sh exit` with the scoped Miri step — all four encoder probes green by
   name; both sweeps 369/369 (or the F3 protocol's adjudication on the record).
3. Ratchet re-baselined. Log entry: the member table with before/after states,
   the leak-check citation, the `pSvcParam` verdict, the cursor enumeration for
   `pFrameBs`, the zeroed survivors, the allocator census, sizes/pins, the
   scoped-Miri numbers, F3 measurements if any.
4. Update `rust/docs/safety_refactor_plan.md` §0 (H spent, I next) and
   `rust/docs/prompts/phase6.md` §6's H row; `rust/docs/perf_baseline.md`.

## Stays raw / untouched — the boundary list

| what | why | whose |
|---|---|---|
| `&mut sWelsEncCtx` anywhere; converting context *parameters* | the aliasing inventory over the whole context runs first, next session | I |
| `pFuncList`, `SWelsFuncPtrList`, its zeroed sites, the dispatch tables | with the deny sweep | I |
| `pSliceThreading`, `pTaskManage`, `mutexEncoderError`, `pDynamicBsBuffer`, `RequestMtResource`'s allocations, `sSliceBs.pBs`, MT files (beyond recorded one-line field spellings) | thread machinery | Phase 7 |
| `wels_encoder_ext.rs` internals; `pSvcParam` if the ownership read says api | ABI boundary | Phase 8 |
| the two `pMvdCost` record fields; E/F/G's named survivors (`*mut SMB` 48, `*mut SMbCache` 72, per-MB plane cursors) | measured settlements | I's inventory / Phase 9 |
| everything tagged `SCREEN_CONTENT(dormant: Phase 10)` | live upstream, fenced | Phase 10 |

## Done-test

- `grep -rn 'WelsMalloc\|WelsMallocz\|WelsFree' --include='*.rs' src/encoder
  src/processing` — every hit is `pFuncList`'s (I), or attributed Phase 7 in the
  census.
- The free cascade contains only the Phase-7 set; every converted member's
  commit carries its leak-check citation.
- `grep -rn 'mem::zeroed' --include='*.rs' src/encoder src/processing` — only
  the enumerated survivors.
- Every new root accessor has the derive-twice-use-first Miri test.
- The Miri scope knob exists, documented, with scoped count and wall time
  recorded; the scoped `exit` reads PASS (or failures adjudicated on the
  record); the span is inside its null band or attributed commit-by-commit.
