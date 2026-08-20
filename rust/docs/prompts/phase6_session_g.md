# Phase 6, session G — the context flip, first third: the constructor and the aliases

## What this session does, and why

`sWelsEncCtx` is the last structure standing between the encoder and its deny
sweep: 71 fields, 26 of them raw pointers, built by `mem::zeroed` at three sites
and reached raw from every module. The flip is three sessions because the S21
chain forces the order:

- **G (this session)**: the context gets a **real constructor** (nothing may own
  until a zeroed shell stops being a valid context), and then its **aliases become
  ids** while every container stays raw — `pCurDqLayer` → a layer index,
  `pSps`/`pPps`/`pSubsetSps` and the layer/slice-header parameter-set pointers →
  the ids `SDqIdc` already stores. The 6.5 R1 fold (`au_set.rs`,
  `paraset_strategy.rs`, `rc.rs`'s context reaches) lands here because those files
  are exactly the alias family's writers.
- **H (next)**: the members own — `RequestMemorySvc` → constructors,
  `FreeMemorySvc` → `Drop` (F19's leak check per member), pointer fields → owned
  containers, `CMemoryAlign` dies.
- **I (last)**: parameters to references where the S37 inventory is clean, the
  deny sweep, §7's exit conditions, the handoffs, the phase close.

**Nothing in this session makes a member own, and nothing takes `&mut
sWelsEncCtx`** — the context is the largest arena in the codebase (S37), and the
whole point of this cut is that ids and a constructor are safe to land while
every consumer still holds raw cursors.

**Execution order is the section order. Drop whole steps from the end** (4, then
3) with the reason in the log; steps 0–2 are the core and do not drop.

## Ground rules

- **Gates**: `bash rust/tools/gates.sh commit` per commit, `family` per step, one
  `exit` at the close. Sweeps read **369/369** per profile since session F
  (`st mt def sl ltr`).
- **Miri once, at the close**, inside the exit battery (four encoder probes green
  by name; the `--lib` step costs ≈1400–1500 s).
- **Perf**: one span at the close, 7 pairs against the HEAD this brief is
  committed at, fresh null band. If it breaches: **attribute commit-by-commit
  before touching anything** (S39 — session F's contingency was aimed at the
  wrong step; attribution found the real contributor and a better remedy than the
  revert). The known risk here is step 2's resolution cost; its mitigation is
  F5's, already proven: resolve once per function/frame, exactly where consumers
  already bind `let pCurDqLayer = …` once.
- **Re-grep every count** (S24); numbers below were measured 2026-08-20 at
  session F's close.
- **Layout changes re-pin in the same commit** (S36): the 17 `assert_ctx_offset!`
  pins and the context's size assert move with every field-type change; measure,
  never delete. Where a pin splits by profile (F's precedent: `pool::Id` carries
  a generation only under `debug_assertions`), pin both numbers.
- **Zeros are ruled, not defaulted** (F56/S21): `Option<LayerIdx>` has no niche —
  all-zero is not `None` — which is exactly why the constructor lands first.
- **Sweep flake protocol** unchanged: the F3 signature (`mt`, `sm=3`, t∈{2,4},
  wrong length) → re-run that configuration 5×; byte-identical → record the
  measurement in `rust/docs/phase0_findings.md`, continue; anything else is
  yours. If a tight retry loop must run under load, remember measurements 77–78:
  a loop is not a quiet run, and `set -- $cfg` does not word-split in zsh.
- The C++ is the behavioral reference; byte parity is the gate.

## The map (verified 2026-08-20)

**The three `mem::zeroed` sites**: `encoder_context.rs:563` (the `Default` impl),
`:1107` and `:1127` (two test builders). All three become `sWelsEncCtx::new()`.

**The context's raw-pointer members, by disposition** (26 total):

| members | disposition |
|---|---|
| `pCurDqLayer` | **this session** — the alias, → `Option<LayerIdx>` |
| `pSps`, `pPps`, `pSubsetSps` | **this session** — aliases into their arrays, → ids |
| `pSvcParam`, `pMvdCostTable`, `pStrideTab`, `pFuncList`, `ppDqLayerList`, `ppRefPicListExt`, `pLtr`, `pWelsSvcRc`, `pVaa`, `pVpp`, `pSpsArray`, `pPPSArray`, `pSubsetArray`, `pOut`, `pFrameBs`, `pDqIdcMap`, `pPSOVector`, `pMemAlign` | **session H** — the containers; stay raw here |
| `pSliceThreading`, `pTaskManage`, `mutexEncoderError`, `pDynamicBsBuffer[..]` | **Phase 7** — thread machinery, untouched |

**The alias facts**:
- `pCurDqLayer`: **281 occurrences** across `src/encoder`. Four writers —
  `encoder_ext.rs:3035`/`:3081` (`*ppDqLayerList.add(iCurDid)` — the id is
  `iCurDid`, already in hand), `:1906` (a temporary during init), and
  `slice_multi_threading.rs:897` (**an MT file**: the field's type change forces
  a one-line edit there; make exactly that edit, body untouched, and record it —
  the D-scr-1 shared-slot clause's shape). Consumers overwhelmingly bind
  `let pCurDqLayer = (*pEncCtx).pCurDqLayer;` once per function and use the raw
  cursor after — the accessor preserves exactly that.
- `LayerIdx(pub u8)` exists (`svc_encode_slice.rs:456`), stamped into every layer
  as `iDqIdx` since session D.
- `pSps`/`pPps` are set as array heads (`encoder_ext.rs:1256–:1257`:
  `(**ppCtx).pSps = (**ppCtx).pSpsArray`) and the layer and slice headers carry
  their own copies (`sLayerInfo.pSpsP`/`pPpsP`; `sSliceHeader.pSps`/`pPps`,
  written at `encoder_ext.rs:2074–:2084`). **`SDqIdc` already stores the ids as
  data** (`encoder_context.rs:286`: `iPpsId: u16, iSpsId: u8`) — the pointer
  aliases duplicate information the C++ itself keeps as integers. That is
  cache-not-carrier with the id already named.
- The parameter-set writers are `au_set.rs` (37 raw occurrences — the
  `SWelsSPS`/`SWelsPPS` builders) and `paraset_strategy.rs` (41 — the
  id-strategy machinery, which traffics in ids natively); `rc.rs` (93) reaches
  the context for `pWelsSvcRc[did]` and friends.

## Step 0 — one inherited chore: the S40 test on the decoder's accessor

Session F's F63 established that a root accessor spelled through
`as_mut_slice().as_mut_ptr()` is a `Unique` retag that pops the previous call's
cursor — and that **the decoder's `SPicture::data_ptr` carries that spelling
today**, passing only because no current caller re-derives while a cursor lives.
Add the covering Miri test S40 mandates: call `data_ptr` **twice, then use the
first cursor**. If it fails, fix the spelling the way F did (read the address out
of the container header, no reference formed — `PaddedPlane::root_ptr` is the
model); if it passes, say why in the test's doc comment. Either way the decoder
stops inheriting the trap silently.

**Check**: the test runs un-ignored in the Miri `--lib` step; red-proof recorded
(swap in the narrowing spelling locally, watch it fail, restore).

## Step 1 — the constructor (the shells die)

**Goal**: `sWelsEncCtx::new()` exists; `mem::zeroed` reads 0 in
`src/encoder src/processing`; the zero image is a *documented decision* per field.

**The recipe is 5b's, verbatim**: field-wise construction with each field's zero
*meaning* written beside it; every embedded type whose `Default` is deliberately
not its zero gets the zero spelled explicitly; and a **temporary byte-comparison
test against the shell it replaces** — `size_of::<sWelsEncCtx>()` bytes compared
between `new()` and a zeroed image, every differing offset attributed by
`offset_of!` before it is accepted (expect **zero differences** in this step:
`new()` reproduces the shell exactly; the freedom it buys is spent in steps 2–3
and session H). Session F's `SPicture` lesson applies in reverse: the *encoder
context's* current semantics **is** the allocation zero — do not import any
"init" state into `new()` that the zeroed shell did not have.

Keep `repr(C)`, keep all 17 offset pins (they should not move in this step —
that is the byte test's point).

**Check**: `grep -rn 'mem::zeroed' src/encoder src/processing` reads 0; the byte
test passes at 0 attributed differences; `gates.sh family`.

## Step 2 — the current layer is an index

**Goal**: `pCurDqLayer: *mut SDqLayer` becomes `iCurDqLayer: Option<LayerIdx>`;
all 281 sites read through one resolution accessor; the raw-cursor idiom of every
consumer is preserved.

**Do**:
1. The field: `Option<LayerIdx>`, written `None` in `new()` (ruled, F56).
2. The accessor pair on the context (or free functions beside `mb_list_root`):
   `current_layer(pCtx) -> *mut SDqLayer` resolving through the still-raw
   `ppDqLayerList` **without forming a reference** (read the pointer out of the
   array — S40's spelling; the array is raw, so this is a plain load), plus the
   setter the four writers use. `debug_assert!` the index is in range.
3. The four writers: `encoder_ext.rs:3035`/`:3081` store `iCurDid` directly;
   `:1906` stores the temporary's `iDqIdx` (D stamped it for exactly this);
   `slice_multi_threading.rs:897` takes the one-line edit, recorded.
4. The 281 readers: `let pCurDqLayer = current_layer(pCtx);` — same binding, same
   raw cursor, same lifetime as today. Nothing downstream changes.

Re-pin the context's size and the offset pins that move (`Option<LayerIdx>` is 2
bytes against the pointer's 8; S36).

**Check**: `grep -rn 'pCurDqLayer' --include='*.rs' src/encoder` reads only the
accessor pair and prose; `gates.sh family`; both sweeps 369/369.

## Step 3 — the parameter-set aliases are ids (and the 6.5 writers fold in)

**Goal**: no context, layer, or slice-header field stores a `*mut SWelsSPS`,
`*mut SWelsPPS`, or `*mut SSubsetSps` alias; the ids are the ones `SDqIdc`
already stores; `au_set.rs` and `paraset_strategy.rs` hold no raw pointer their
signatures do not need.

**Do**:
1. Id types mirroring the data the C++ keeps: `SpsId(u8)`, `PpsId(u16)` (match
   `SDqIdc`'s widths; the subset array shares `SpsId`'s space — read
   `paraset_strategy.rs` for how the strategy distinguishes them and give the
   subset its own type if the strategy does).
2. `sWelsEncCtx.pSps`/`pPps`/`pSubsetSps` → `Option<SpsId>`/`Option<PpsId>`/…;
   `sLayerInfo.pSpsP`/`pPpsP` and `sSliceHeader.pSps`/`pPps` → the same ids.
   Resolution accessors derive raw from the still-raw arrays
   (`pSpsArray.add(id)`) — S40's spelling, references only where a callee already
   takes `&mut SWelsSPS`.
3. `au_set.rs`'s builders take `&mut SWelsSPS`/`&mut SWelsPPS` resolved at the
   call boundary (R1 — their eight `*mut SWelsSPS` params are single-object);
   `paraset_strategy.rs` converts take-what-you-reach (it already thinks in ids);
   `rc.rs`'s context reaches become accessor calls where an accessor exists and
   stay raw where the member is H's (do not chase `pWelsSvcRc`'s innards).
4. Sizes re-pinned as fields shrink.

**Check**: `grep -rn '\*mut SWelsSPS\b\|\*mut SWelsPPS\b\|\*mut SSubsetSps\b'
--include='*.rs' src/encoder` reads 0 stored-field sites (params on the writers
may remain where the resolution happens); `gates.sh family`.

## Step 4 — the rc sweep residue

**Goal**: `rc.rs`'s 93 raw occurrences read as: context-member reaches (H's),
nothing else. Convert its remaining single-object params (E converted the `SMB`
9; sweep what E's closures did not reach), delete any cache param of something
another param reaches (rule (c)), leave every `pWelsSvcRc`/member reach raw for H.

**Check**: rc.rs's raw count attributed line-by-line in the log: H's vs converted.

## Step 5 — close

1. The span (7 pairs, fresh null); on a breach, S39: attribute per commit first.
2. `gates.sh exit` — Miri once, four probes by name, sweeps 369/369 both
   profiles (or the adjudicated F3 protocol).
3. Ratchet re-baselined; log entry with: `mem::zeroed` 0, `pCurDqLayer` 281 → 2
   (the accessor pair), the sps/pps family counts, the MT one-liner recorded,
   sizes and pins re-measured, F3 measurements if any.
4. Update plan §0 (sessions A–G spent, H next) and phase6.md §6's G row;
   `perf_baseline.md`.

## Stays raw / untouched this session — the boundary list

| what | why | whose |
|---|---|---|
| every context member's *allocation* (`ppDqLayerList`'s block, the arrays, `pMvdCostTable`, `pFrameBs`, `pWelsSvcRc`, …) | containers own in H, after the constructor exists | H |
| `&mut sWelsEncCtx` anywhere | the context is the largest arena; the S37 inventory runs in I | I |
| `SWelsFuncPtrList` (57) and dispatch-table dissolution | I's, with the deny sweep | I |
| `pSliceThreading`, `pTaskManage`, `mutexEncoderError`, `pDynamicBsBuffer`, `sSliceBs.pBs`, MT files (beyond step 2's one-line field write) | thread machinery | Phase 7 |
| `wels_encoder_ext.rs` internals | ABI boundary | Phase 8 |
| E's and F's named survivors (`*mut SMB` 48, `*mut SMbCache` 72, per-MB plane cursors, F5's ~10 side-array resolutions) | measured verdicts | Phase 9 |
| the `SCREEN_CONTENT(dormant)` family | fenced | Phase 10 |

## Done-test

- `mem::zeroed` reads **0** in `src/encoder src/processing`; the byte-comparison
  test exists and attributed **0** differences when it landed.
- `pCurDqLayer` reads 281 → the accessor pair only; the four writers store ids;
  the MT file's edit is one line and recorded.
- No stored field anywhere names `*mut SWelsSPS`/`*mut SWelsPPS`/`*mut
  SSubsetSps`; `au_set.rs` + `paraset_strategy.rs` converted or their survivors
  named with blockers.
- The decoder's `data_ptr` carries S40's twice-derived test, red-proofed.
- Sizes and all 17 offset pins re-measured; span inside its null band or
  attributed per S39; `gates.sh exit` PASS or every failure adjudicated on the
  record.
