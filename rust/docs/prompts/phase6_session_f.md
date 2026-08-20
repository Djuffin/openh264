# Phase 6, session F — the pictures: two pools, two id types, and the planes

## What this session does, and why

The encoder has exactly **three owners of pictures**, confirmed at their allocation
and free sites: the **reconstruction pool** (`SRefList.pRef[..]`, one `SRefList` per
dependency layer), the **spatial source pool**
(`CWelsPreProcess.m_pSpatialPic[did][..]`), and **one scaled input picture**
(`Scaled_Picture.pScaledInputPicture`). Every other `*mut SPicture` in the tree — 14
fields across the context, the ref lists, the layer, and the preprocess — is an
alias into one of the three.

This session:

1. makes `SPicture` **constructor-built** and gives it ownership of its four per-MB
   side arrays (and deletes `SMbCache.pEncSad`, the one cursor into them),
2. executes the **id flip**: both pools become `Pool<Box<SPicture>>`, every alias
   becomes `Option<SrcPicId>` / `Option<RecPicId>` — the fourteen-row table below is
   the work list, ~184 sites over nine files, **one pass, whole or not at all**,
3. gives the picture **owned planes** with root accessors for the hot cursors,
4. converts the **per-frame** plane flows — VAA, preprocess, reference-padding —
   to slices and out-views,
5. closes with the one span and the exit battery.

Two prior results shape everything here. **A picture is an arena** (S37): consumers
hold raw cursors into its planes across calls (`pEncMb`, `pRefMb`, the layer's
`pCsData`/`pEncData`), so no conversion below hands out `&mut SPicture` while
cursors live — aliases become *ids*, hot cursors stay raw and root-derived, and the
pool is asked through id-based accessors. And **per-candidate kernels are out of
scope** (session E measured the slices-and-offsets lead failing at 1.30–5.68x):
faces 3–4 convert only **per-frame** operations, where view construction amortizes
over a whole plane; `mc.rs`, `sad_common.rs`, `intra_pred_common.rs`,
`deblocking_common.rs` callers stay raw for Phase 9's kernel-signature work.

**Execution order is the section order. If the session runs short, drop whole steps
from the end** (4, then 3, then 2) and say so in the log. Steps 0–1 are the core
and do not drop; step 1 does not land partially.

## Ground rules

- **Gates**: `bash rust/tools/gates.sh commit` in every commit, `gates.sh family` at
  each step's close, one `gates.sh exit` at the session close. Sweeps read
  **353/353** per profile since session E (`st mt def sl`).
- **Miri runs once, at the session close, inside the exit battery — not between
  steps.** The four encoder probes cost ≈460 s per run; the whole `--lib` step read
  ≈1430 s at session D's close.
- **Perf**: one measurement — 7 pairs against the HEAD this brief is committed at,
  plus a fresh null band (`rust/tools/perfpair.py`). Step 2 (planes) is the
  hot-path risk and is **separable by construction**: if the closing span breaches
  its null band, revert step 2 alone, re-measure, and record both readings.
- **Re-grep every count and line number below before acting on it** — they were
  read at `ffa78a87` (the settlement) and re-checked 2026-08-19; sessions since
  have not touched these files, but verify:
  `grep -rn '\*mut SPicture\b' --include='*.rs' src/encoder src/common`.
- **Every struct whose layout changes re-pins its `assert_size!` in the same
  commit** (`src/encoder/abi_guard.rs`). Deleting an assertion is a defect;
  measuring and re-pinning is the job. `SPicture` is pinned at **136** and the pin
  becomes the port's own number the moment step 0 lands.
- **Zeros are ruled, not defaulted**: every alias that becomes `Option<Id>` gets
  `None` exactly where the C++ writes null, checked at each site (the F56
  discipline — `Option` has no niche here to inherit a zero image from).
- **Sweep flake protocol**: a single failing configuration with the known signature
  (`mt` preset, `sm=3`, `t` ∈ {2,4}, wrong output length, either profile) → re-run
  that exact configuration 5×; byte-identical every time → record as an F3
  measurement in `rust/docs/phase0_findings.md` and continue. Anything else is
  yours — stop and fix.
- The C++ is the behavioral reference; byte parity is the gate; its pointer
  spellings are not a constraint.

## The map (verified 2026-08-19)

**The three owners:**

| owner | storage | alloc / free |
|---|---|---|
| reconstruction pool | `SRefList.pRef: [*mut SPicture; 1 + MAX_REF_PIC_COUNT]` (`encoder_context.rs:335`) | `encoder_ext.rs:752` / `:1684`; one `SRefList` per dependency layer behind `sWelsEncCtx.ppRefPicListExt: *mut *mut SRefList` (`encoder_context.rs:468`) |
| spatial source pool | `CWelsPreProcess.m_pSpatialPic: [[*mut SPicture; MAX_REF_PIC_COUNT + 1]; MAX_DEPENDENCY_LAYER]` (`wels_preprocess.rs:950`) | `wels_preprocess.rs:1068` / `:1098` |
| scaled input | `Scaled_Picture.pScaledInputPicture` (`wels_preprocess.rs:194`) | `:890` / `FreeScaledPic` |

**Host readiness** (S21 — a `Pool` field is UB in a zeroed shell, so the host must
be constructor-built first):

- `CWelsPreProcess` is **already Box-built** — `CreatePreProcess`
  (`wels_preprocess.rs:977`) does `Box::new` + `into_raw`, `Destroy` does
  `from_raw`, `Default` is field-wise because `m_vp` is already a `Box`. It can own
  the spatial pool today.
- `SRefList` (`encoder_context.rs:331`) is a POD of pointer arrays with a `Default`
  — it needs session D's recipe first: **`Box`-built with a real constructor**, in
  the same commit that gives it the `Pool` (it is reached only through
  `ppRefPicListExt`, a raw pointer field in the zeroed context — T3.6's precedent,
  same as `SDqLayer`).
- `Scaled_Picture` is a by-value field of the Box-built `CWelsPreProcess`; its slot
  becomes `Option<Box<SPicture>>` (drop its `Copy` when it owns).

**`SPicture` itself** (`picture.rs`, `repr(C)` `Copy` `Clone`, pinned 136):
planes `pBuffer: *mut u8` + `pData: [*mut u8; 3]` + `iLineSize: [i32; 3]`; four
per-MB side arrays `uiRefMbType: *mut u32`, `pRefMbQp: *mut u8`,
`pMbSkipSad: *mut i32`, `sMvList: *mut SMVUnitXY`; the dormant
`pScreenBlockFeatureStorage` (tagged, Phase 10 — **do not touch**); `AllocPicture`
at `wels_preprocess.rs:733` is the single constructor site.

**The vocabulary already exists**: `safe/pool.rs` has `Pool<T>` with `get`/
`get_mut`/`pair_mut`/`mut_and_rest`/`replace` and a debug-generation `Id`;
`safe/plane.rs` has `PaddedPlane` (`new`, `from_parts`, `as_slice`/`as_mut_slice`,
`row`/`row_mut`, `at`/`set`) — the decoder's picture conversion shipped on both.

## Step 0 — the picture owns its side arrays, and `pEncSad` dies

**Goal**: `SPicture` is constructor-built; `uiRefMbType`/`pRefMbQp`/`pMbSkipSad`/
`sMvList` are owned `Vec`s; `SMbCache.pEncSad` is deleted and derived.

**Facts**:
- `AllocPicture` (`wels_preprocess.rs:733`) allocates the picture, its buffer, and
  the side arrays; it is the only constructor. It becomes `SPicture::new(...) ->
  Box<SPicture>` (planes stay raw in this step; step 2 takes them).
- `SMbCache.pEncSad` (`md.rs:475`) is a **neighbour cursor** into the recon
  picture's `pMbSkipSad` at `kiMbXY`: the four reads are `offset(-1)`,
  `offset(-iMbWidth)`, `offset(-iMbWidth - 1)`, `offset(-iMbWidth + 1)`
  (`md.rs:936/:968/:995/:1015`), each already behind its availability guard —
  13 occurrences total, enumerate them all.
- `sMvList`'s consumers are the two `SMVUnitXY` survivor sites session E left
  (picture.rs field + one wels_preprocess use).

**Do**:
1. `SPicture::new` builds the picture whole (F56: each field's zero written out —
   the `Default` already spells them); side arrays become `Vec<u32>`, `Vec<u8>`,
   `Vec<i32>`, `Vec<SMVUnitXY>` sized exactly as `AllocPicture` sizes them today.
   Drop `Copy`/`Clone` (the compiler names any by-value copy; expect none — the
   pools swap pointers, never values). Re-pin the size.
2. Delete `pEncSad`; each of the 13 sites derives from the layer's recon picture —
   the *array root* plus `iMbXY` and the neighbour offset (S28; the guards stay
   exactly where they are). Re-pin `SMbCache` (5600 → smaller by one pointer).
3. Keep every alias-holder raw in this step — the id flip is step 1's.

**Check**: `grep -rn 'pEncSad' src/` reads 0 code hits;
`grep -rn '\*mut SMVUnitXY' src/encoder src/processing` reads 0; sizes re-pinned;
`gates.sh family`.

## Step 1 — the id flip (whole or not at all)

**Goal**: both pools are `Pool<Box<SPicture>>`; every alias row below is an id;
no field anywhere stores `*mut SPicture`.

**The work list** — session B's settlement table, measured at ~184 consumer sites
over nine files (`encoder_ext`, `wels_preprocess`, `ref_list_mgr_svc`,
`encoder_context`, `svc_encode_slice`, `wels_encoder_ext`, `svc_base_layer_md`,
`svc_mode_decision`, `picture`):

| field | owner | becomes |
|---|---|---|
| `sWelsEncCtx.pEncPic` (`encoder_context.rs:462`) | spatial | `Option<SrcPicId>` |
| `sWelsEncCtx.pDecPic` (`:463`) | recon | `Option<RecPicId>` |
| `sWelsEncCtx.pRefPic` (`:464`) | recon | `Option<RecPicId>` |
| `sWelsEncCtx.pRefList0[16]` (`:468` area) | recon | `[Option<RecPicId>; 16]` |
| `SRefList.pShortRefList[]`, `pLongRefList[]` (`:332`, `:333`) | recon | `[Option<RecPicId>; N]` |
| `SRefList.pNextBuffer` (`:334`) | recon | `Option<RecPicId>` |
| `SRefList.pRef[]` (`:335`) | **is** the pool | `Pool<Box<SPicture>>` |
| `SSpatialPicIndex.pSrc` (`:398`) | spatial | `Option<SrcPicId>` |
| `SDqLayer.pRefPic`, `pDecPic` (`svc_encode_slice.rs`) | recon | `Option<RecPicId>` |
| `SDqLayer.pRefOri[]` | spatial | `[Option<SrcPicId>; N]` |
| `CWelsPreProcess.m_pSpatialPic[][]` (`wels_preprocess.rs:950`) | **is** the pool | `Pool<Box<SPicture>>` + id array per layer |
| `CWelsPreProcess.m_pLastSpatialPicture[][2]` (`:947`) | spatial | `[[Option<SrcPicId>; 2]; N]` |
| `SRefInfoParam.pRefPicture` (`:234`) | spatial | `Option<SrcPicId>` |
| `Scaled_Picture.pScaledInputPicture` (`:194`) | its own slot | `Option<Box<SPicture>>` |

**Settlements already made — do not re-derive**:
- **Two id types, not one**: `SrcPicId` and `RecPicId`, newtypes over `pool::Id`.
  `pEncPic` (spatial) and `pDecPic`/`pRefPic` (recon) meet in one
  `WelsEncoderEncodeExt` iteration (`encoder_ext.rs:3270`, `:3343`) and in
  `UpdateOriginalPicInfo` (`ref_list_mgr_svc.rs:1169`); one type would let either
  be passed where the other belongs.
- **S34 is measured and clean**: nothing permutes a pool's storage.
  `WelsExchangeSpatialPictures` (`wels_preprocess.rs:1787`, five call sites) swaps
  *slot pointers* — it becomes an id swap; the ref-list shifts are explicit index
  loops (`ref_list_mgr_svc.rs:273`, `:296`) and stay loops over id arrays.
- **F42's arm needs nothing**: no pointer-identity comparison between pictures
  exists in `src/encoder`; `pDecPic` (`pNextBuffer`, chosen in
  `ref_list_mgr_svc.rs:628–643` as a slot in *neither* ref list) and `pRefPic` are
  distinct slots by construction — no `classify`, no `same_picture`.
- **S37 shapes the API**: readers resolve ids through the pool per use
  (`pool.get(id)` / `get_mut`); MC/VAA-style read-one-write-another sites use
  `pair_mut` or `mut_and_rest`; **hot plane cursors are derived raw from the
  resolved picture and stay raw** — no `&mut SPicture` is held across a call that
  resolves another id.
- `SRefList` Box-built with a constructor **in the same commit** its `Pool` lands
  (S21); `FreeDqLayer`-style member walks die with the malloc/free pairs.

**Also in this step**: before it lands, check the harness can reach the long-term
reference paths (`pLongRefList` shifts) — grep the diffharness drivers for an LTR
knob. If no gate configuration exercises LTR, add one the way session E added
`sl` (the F60 lesson: a path the sweep cannot reach is a path the flip can break
silently), and record the new totals.

**Check**: `grep -rn '\*mut SPicture' --include='*.rs' src/encoder src/common`
reads **0 code sites** outside `picture.rs`'s own accessors;
`gates.sh family`; the byte gates unmoved at the (possibly grown) totals.

## Step 2 — the planes (separable; the hot-path risk lives here)

**Goal**: `pBuffer`/`pData`/`iLineSize` become owned planes; every hot cursor
derives from a root accessor.

**Facts**: the decoder's `SPicture` shipped exactly this (`PaddedPlane` + `Vec`s)
inside Phase 5's budget, with a `data_ptr` root accessor and the S28-mandated Miri
test. The encoder's consumers of the plane roots: the layer's
`pEncData`/`pDecPic`-derived `pCsData` cursors, `InitMe`'s `pEnc`/`pRef`,
`ExpandReferencingPicture`, VAA's `pCurY`/`pRefY`, and the MC/intra kernels.

**Do**: three `PaddedPlane`s (or one struct holding them) replace
`pBuffer`/`pData`/`iLineSize`; a root accessor per plane hands the per-MB world
its raw cursors (derived from the owned buffer — S28, with the Miri test the rule
mandates, walking a plane's full reach from the middle); `iLineSize` reads become
`stride()`. Per-MB kernel signatures **do not change** (Phase 9's). The layer's
`pCsData`/`pEncData` stay raw `[*mut u8; 3]` cursors, re-derived at the same
places they are today.

**Check**: `grep -n 'pBuffer' src/encoder` reads 0 code hits; size re-pinned;
`gates.sh family`; **this step is one revertable commit series** — if the closing
span breaches, it reverts alone.

## Step 3 — VAA and the preprocess flows (per-frame, so views amortize)

**Goal**: `SVAAFrameInfo` is Box-built and owns its per-frame result arrays; the
calc kernels take slices; `processing/` holds no raw pointer it does not need.

**Facts**: `sWelsEncCtx.pVaa: *mut SVAAFrameInfo` (`encoder_context.rs:487`) is a
raw pointer field in the zeroed context — T3.6's precedent applies, so the struct
behind it can own (same argument as `SDqLayer`, `SRefList`).
`SVAACalcResult` (`wels_preprocess.rs:321`) holds `pCurY`/`pRefY` (plane roots —
step 2's accessors feed them) and **six out-pointers** (`pSad8x8`, `pSsd16x16`,
`pSum16x16`, `pSumOfSquare16x16`, `pSumOfDiff8x8`, `pMad8x8`) into VAA-owned
per-frame arrays — the P9 shape: the arrays become owned `Vec`s in
`SVAAFrameInfo`, and the calc functions take `&[u8]` planes and `&mut [..]`
out-slices built at the call boundary. `vaacalc.rs` carries 57 raw-pointer
occurrences, the rest of `processing/` a handful.

**Do**: Box-build `SVAAFrameInfo` with a constructor; arrays → `Vec`s;
`SVAACalcResult` → borrowed views or (root, len) pairs per P9; convert
`vaacalc.rs`'s kernels to slice signatures (per-frame loops — E5's failure mode
does not apply); sweep the rest of `processing/` take-what-you-reach
(`complexity_analysis.rs`'s screen half is dormant-tagged — leave it).
`SVAAFrameInfoExt_t` (`svc_mode_decision.rs:205`): read its reachability before
touching — if it is screen-content-only, tag it dormant instead (D-scr-1).

**Check**: `processing/` raw count at or near 0 with survivors named;
`gates.sh family`.

## Step 4 — the per-frame common caller

**Goal**: `ExpandReferencingPicture` (`common/expand_pic.rs`, called per frame
from `ref_list_mgr_svc.rs:673`) takes the owned planes; whatever shim dies with
that, dies.

**Explicitly not this step**: `mc.rs` (72), `sad_common.rs` (36),
`intra_pred_common.rs`, `deblocking_common.rs` (34) — their callers are per-MB
and per-candidate; session E measured that conversion failing (1.30–5.68x); they
are Phase 9's kernel-signature work. `memory_align.rs` is the allocator (6.6).
`wels_thread_pool.rs` is Phase 7's.

**Check**: expand family converted or its blocker named; `gates.sh family`.

## Step 5 — close

1. The span: 7 pairs against the session-start HEAD, fresh null band; tripwire
   +25% median (cumulative encoder deficit stands at ≈+10…12%). If breached and
   step 2 landed, revert step 2, re-measure, record both.
2. `bash rust/tools/gates.sh exit` — the one Miri `--lib` run, all four encoder
   probes green by name in the output; both sweeps at current totals in both
   profiles.
3. Regenerate the ratchet baseline; log entry with: `*mut SPicture` 59 → 0 (or
   survivors named), the id types and both pools landed, sizes re-pinned, LTR
   coverage disposition, `processing/` residue, the span (and step 2's separate
   reading if it was reverted), F3 measurements if any.
4. Update the session F rows in `rust/docs/safety_refactor_plan.md` §0 and
   `rust/docs/prompts/phase6.md` §6, and `rust/docs/perf_baseline.md`.

## Stays raw this session — the boundary list

| what | why | whose |
|---|---|---|
| `pEncCtx`/`pCtx` params, every `(*pCtx).…` reach, `pCurDqLayer`, `pMvdCost` + table | the context flips last | G |
| `ppRefPicListExt` and `pVpp`/`pVaa` as *context fields* | the fields stay raw pointers to the now-owning `Box`es; only the pointees own | G |
| per-MB/per-candidate kernel signatures (`mc`, SAD/SATD, intra, deblock kernels) and `SWelsME.pEncMb/pRefMb/pColoRefMb` | E5's measured verdict; cursors derive from step 2's roots | Phase 9 |
| the `SCREEN_CONTENT(dormant)` family, incl. `SPicture.pScreenBlockFeatureStorage` | fenced, live upstream | Phase 10 (D-scr-1) |
| `sSliceBs.pBs`, MT files | thread machinery | Phase 7 |
| `wels_encoder_ext.rs` internals | ABI boundary | Phase 8 |
| E's named survivors (`*mut SMB` 48, `*mut SMbCache` 72) | arena + neighbour rules | Phase 7/9/G as recorded |

## Done-test

- `grep -rn '\*mut SPicture\b' --include='*.rs' src/encoder src/common` reads 0
  code sites outside `picture.rs`'s root accessors (or every survivor is named
  with its blocker).
- `pEncSad`, `pBuffer`, `*mut SMVUnitXY`: 0 code hits each.
- Both pools are `Pool<Box<SPicture>>` behind constructor-built hosts; two id
  types exist and neither converts to the other.
- LTR reachability checked, and closed or added the `sl` way.
- Sizes re-pinned; span inside its null band (step 2 separately dispositioned if
  not); `gates.sh exit` reads PASS.
