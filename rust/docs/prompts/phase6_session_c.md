# Phase 6, session C — the scratch goes inline: `SMB`'s five arrays, `SMbCache`'s twelve

Two families, one recipe: **fixed-size scratch that the C++ malloc'd separately and
pointed at becomes an inline array in the struct that always owned it.** `SMB`
points at five context-wide arrays; `SMbCache` points at eight per-slice buffers
and four aliases into them. Both structs live in `WelsMallocz`'d blocks, so an
inline POD array is valid at zero (S21) and a `Vec`/`MbArray` field would not be
until 6.6 makes the hosts constructible — that is why the recipe is inline and not
`safe/mb_grid.rs`, and the record says so. Governing: [`phase6.md`](phase6.md)
§1–§3, §6; **S21**, **S24**, **S28**, **S29**, **S31**, **S32**, **S33**;
**D-fid-1**, **D-par-1**, **D-perf-4**, **D-perf-6**. Counts at `2666a83c`; re-grep
at each face's open.

## The finish rule

**No hand-off.** The session ends when §6 reads met, or at a blocker only the
steward can clear, named. A size is never a stop (S31). Questions take the decision
ladder. **Both encoder probes are live** — run them at every face's close; a red is
a finding (fix or park with an owner, red-before/green-after observed). Byte-exactness
is the definition: `gates.sh family` at every face that touched production code.

## 0. Start

1. Commit the inherited doc tail. Open per **S27** (session B closed `exit`
   `OVERALL: PASS` 13/0/1): build both profiles + `--all-targets`, tests
   (484 / 478 / 20), ratchet, census (58).
2. Base for the span: **`2666a83c`** — build and stash its benches now
   (`.perfpair/`), before the tree moves.
3. S33 on every number; a per-face breadcrumb in the log.

## 1. Face 1 — `SMB` stops pointing outward

`SMB` (`md.rs:293`) holds `sMv: *mut SMVUnitXY`, `pRefIndex: *mut i8`,
`pSadCost: *mut i32`, `pIntra4x4PredMode: *mut i8`, `pNonZeroCount: *mut i8`, each
wired by `InitMbInfo` (`encoder_ext.rs:615–621`) into a context array
`RequestMemorySvc` allocates (`:1162–1201`, five `WelsMallocz`, freed `:1934–1958`):
`pMvUnitBlock4x4` and `pRefIndexBlock4x4` in **two banks** selected by the layer's
parity (`kiOffset = (kiDlayerId & 1) * kiMaxMbNum`, `:563`), the other three shared
across layers, all sized `iCountMaxMbNum`.

### 1.1 The settlement — in the log before the first edit

- **Inline, renamed** (the rename is the enumerator — the compiler prints every
  site): `sMv: [SMVUnitXY; MB_BLOCK4x4_NUM]`, `iRefIndex: [i8; MB_BLOCK8x8_NUM]`,
  `iSadCost: i32`, `iIntra4x4PredMode: [i8; INTRA_4x4_MODE_NUM]`,
  `iNonZeroCount: [i8; MB_LUMA_CHROMA_BLOCK4x4_NUM]`. `sMvd` (`:302`) is already
  inline in the same struct.
- **The zero state is unchanged**: the five arrays were `WelsMallocz`'d and the
  `SMB` block is `WelsMallocz`'d, so every macroblock still starts at MV (0,0),
  ref 0, nzc 0, mode 0, sad 0.
- **The two banks are a ruling, written**: banks exist so a layer and its
  reference layer never share slots; inline storage gives every macroblock its
  own, which is that guarantee and more. The one observable difference — a
  same-parity layer two levels away reading a slot before rewriting it — needs
  ≥ 3 spatial layers of equal size, a configuration the port cannot run
  (`METHOD_DOWNSAMPLE` is untranslated, `wels_preprocess.rs:1541`; the sweeps are
  single-layer, `diffharness/cxx_enc.cpp:81`). Ruled: inline. **If downsampling is
  ever translated, that phase re-opens this line with a multi-layer sweep
  configuration beside it.**
- **Two sites reach a neighbour by flat-array arithmetic** rather than through
  the neighbour's `SMB`: `md.rs:620` (`pNonZeroCount.offset(-24)`) and `:635`
  (`pIntra4x4PredMode.offset(-8)`) — the C++ does the same. They become reads of
  `(*pLeftMb).iNonZeroCount[..]` / `.iIntra4x4PredMode[..]`, value-identical by
  `InitMbInfo`'s wiring. Grep for any others: `\.(sMv|pRefIndex|pSadCost|pIntra4x4PredMode|pNonZeroCount)\.(offset|wrapping_offset|sub)\(`
  reads 8 today, six of them within-MB.
- **S24**: `.sMv` reads 107 across nine files but `SWelsME.sMv` is a scalar
  (`md.rs:1207`, `:1450` and their kin) — split by receiver type before counting.

### 1.2 Execute

1. Rename and retype the five fields; delete the five context fields
   (`encoder_context.rs:437–445`), their allocations, frees, `InitMbInfo`'s five
   wiring lines and the bank arithmetic; `assert_size!(SMB, 152)`
   (`abi_guard.rs:170`) re-pinned to the measured size in the same commit.
2. Every consumer site (~195 over `md.rs`, `svc_mode_decision.rs`,
   `svc_base_layer_md.rs`, `svc_motion_estimate.rs`, `svc_set_mb_syn_cabac.rs`,
   `svc_set_mb_syn_cavlc.rs`, `svc_encode_mb.rs`, `deblocking.rs`,
   `svc_encode_slice.rs`): `*x.add(i)` → `x[i]`; `copy_nonoverlapping(src.add(a), dst.add(b), n)`
   → slice copies; a kernel that takes `*mut i8` for the whole array
   (`WelsNonZeroCount_c`, `deblocking.rs:643`) takes `&mut [i8; 24]` or the raw
   pointer derived per the rule below.
3. **The one spelling rule** (S29, and session B's walk found the exact family):
   a raw pointer into an inline array of a struct reached through a raw pointer is
   `addr_of_mut!((*p).field).cast::<T>()` (or `as *mut T`) — **never**
   `(*p).field.as_mut_ptr()`, which auto-refs a `&mut` over the array and pops the
   parent's tag on those bytes. Prefer no raw pointer at all: index.
4. `gates.sh commit` per commit; both encoder probes at the face's close;
   `family` at the face's close.

### 1.3 The second encoder probe — one, with two knobs

`drive_encoder_over` (`api/codec_api.rs:2439`) is **CABAC** (`iEntropyCodingModeFlag = 1`,
`PRO_HIGH`) and **`LOW_COMPLEXITY`**. This face converts CAVLC's writers
(`svc_set_mb_syn_cavlc.rs`, 16 sites) and the fine I4x4 partition search that only
the non-LOW path runs (`svc_base_layer_md.rs:393–442`, `:513–563`, the `pMemPredBlk4`
ping-pong face 2 also touches) — **neither is under Miri today**, and F47 is what a
probe gap costs. Give the driver an options struct (entropy, complexity), keep the
existing probe as it is, and add **one** more test that flips **both** knobs — CAVLC
and a non-LOW complexity mode — 48×32, three frames, the same non-vacuity assertions.
Record its cost (expect the encode probe's ≈80 s order); its name read out of the
`--lib` output (F17/F18); placement `full`/`exit` beside the others. A third probe
needs a number behind it (S32). The size-limited dynamic-slice path
(`WelsMdInterMbLoopOverDynamicSlice`) stays unprobed and is **named for session D**,
which converts the slice structures.

## 2. Face 2 — `SMbCache` owns its scratch

`SMbCache` (`md.rs:354`) carries twelve raw pointers. `AllocMbCacheAligned`
(`svc_encode_slice.rs:2180`) mallocs **eight**: `pMemPredMb` (2·256 + 16 — F14's
accommodation, comment at the site), `pCoeffLevel` (`MB_COEFF_LIST_SIZE` i16),
`pSkipMb` (384), `pMemPredBlk4` (32), `pBufferInterPredMe` (4·640),
`pPrevIntra4x4PredModeFlag` (16 bool), `pRemIntra4x4PredModeFlag` (16 i8), `pDct`
(`SDCTCoeff`). **Four are aliases** into two of those — ping-pong halves:
`pMemPredLuma`/`pMemPredChroma` (the two 256-byte halves of `pMemPredMb`,
`svc_base_layer_md.rs:363–365`, swapped at `svc_mode_decision.rs:1169–1170`),
`pBestPredIntraChroma` (one of two 128-byte halves inside the chroma half,
`svc_base_layer_md.rs:703`, `:741`), `pBestPredI4x4Blk4` (one of two 16-byte halves of
`pMemPredBlk4`, `:441`, `:662`). **`pEncSad`** is an alias into a *picture*
(`pDecPic->pMbSkipSad + kiMbXY`, `:886`) — the `SPicture` family, session F's; it
stays a raw field, derived per macroblock as today.

### 2.1 The settlement — in the log before the first edit

- **Eight owners inline**: `sMemPredMb: [u8; 528]` (the `+16` stays, and its comment
  moves with it — deleting it is red under `svc_mode_decision::tests::test_wels_md_i16x16_cost`
  under Miri, measured), `sCoeffLevel: [i16; MB_COEFF_LIST_SIZE]`, `sSkipMb: [u8; 384]`,
  `sMemPredBlk4: [u8; 32]`, `sBufferInterPredMe: [u8; 2560]`,
  `bPrevIntra4x4PredModeFlag: [bool; 16]`, `iRemIntra4x4PredModeFlag: [i8; 16]`,
  `sDct: SDCTCoeff`.
- **Four aliases become three selectors** (cache, not carrier — each is one bit
  the ping-pong already tracks in a local): `uiMemPredLumaHalf: u8` (chroma is the
  other half), `uiBestPredIntraChromaHalf: u8`, `uiBestPredI4x4Blk4Half: u8`, with
  accessors returning `&[u8]`/`&mut [u8]` slices of the inline buffers. Where a
  prediction kernel still takes `*mut u8`, derive from the array root per 1.2's
  rule (`addr_of_mut!((*pMbCache).sMemPredMb).cast::<u8>().add(256 * half)`), never
  through a slice or an `as_mut_ptr()` (S28, S29).
- `AllocMbCacheAligned` and `FreeMbCache` are deleted with the pointers;
  `AllocateSliceMBBuffer` (`:2362`) becomes empty and goes (S18). `SMbCache` loses
  `Copy` unless something copies it (the compiler says which; a 5 KB by-value copy
  is a site to look at, not a derive to keep). `assert_size!(SMbCache, 576)`
  (`abi_guard.rs:169`) re-pinned; `SSlice` grows by the same amount — measure and
  record it, and note that the C++ allocated the same bytes per slice, malloc'd.
- **A latent defect closes with the pointers, record it**: `ReallocateSliceList`
  (`:2583`) `copy_nonoverlapping`s the old slices — pointers included — then on
  its error paths `FreeSliceBuffer`s the *new* list, whose copied scratch pointers
  are the old list's; inline scratch has no such aliasing.

### 2.2 Execute

Sites: `pCoeffLevel` 18, `pSkipMb` 22, `pMemPredMb` 8, `pMemPredLuma` 12,
`pMemPredChroma` 17, `pBestPredIntraChroma` 2, `pMemPredBlk4` 10, `pBestPredI4x4Blk4` 3,
`pBufferInterPredMe` 9, `pPrevIntra4x4PredModeFlag` 9, `pRemIntra4x4PredModeFlag` 9,
`pDct` 21 — over `svc_base_layer_md.rs`, `svc_mode_decision.rs`, `svc_encode_slice.rs`,
`svc_encode_mb.rs`, `svc_motion_estimate.rs`, `md.rs`, entropy writers. One commit per
owner or per ping-pong; `gates.sh commit` each; both probes and `family` at the
face's close.

## 3. Face 3 — the span

Both faces move the hot path (mode decision, motion estimation, entropy, deblocking
read these arrays per macroblock), so this session **owes a span** (S2b, D-gate-1):
`2666a83c` → the close, both benches, interleaved pairs through `perfpair.py`, the
null re-run at the verdict's own pair count, a second day for any reading a
decision rests on. Write the ledger row.

- The encoder's cumulative deficit is ≈ **+11…+13%** by the ledger; the tripwire is
  D-perf-4's **+25% median** on any bench stream. State the arithmetic before and
  after.
- A reading outside the null band names its mechanism as a **candidate, not a
  claim** (S33): for face 1, the neighbour reads now step through ~250-byte `SMB`
  structs where the flat arrays were 24/64 bytes apart (AoS locality); for face 2,
  inline scratch should be neutral or better (one fewer indirection per access).
- **The fallback is named, not executed**: if face 1's mechanism is confirmed as
  the cost, the SoA shape is available — the five arrays as `MbArray`s owned by a
  Box-built struct behind one raw context field (T3.6's `pOut` pattern, valid at
  zero), banks and all. It is a Phase 9 ledger item under D-perf-6 unless the
  tripwire arithmetic says otherwise; a face parks only if it *would cross +25%*.

## 4. Gates

`gates.sh commit` per commit; both encoder probes under Miri at each face's close
(`cargo +nightly miri test --lib -- encode_loop_runs_over_a_macroblock_grid` with
the step's flags — read the two names in the output); `family` at each face's
close; `exit` at the close. Ratchet increases recorded, not hidden. Do not edit
while a battery runs.

## 5. Non-goals

No `pCurMb: *mut SMB` parameter conversions and no ownership of the `SMB` list
(`sMbDataP`, `ppMbListD`) — the list goes with the layer bracket, session D. No
`pMbCache: *mut SMbCache` parameter conversions and no ME/MD/RC/CABAC records
(session E, which now starts there — `SMbCache`'s fields moved here because they
share this recipe). No SIMD third attempt (E). No pictures (F — `pEncSad` stays).
No context flip (G). No `MbArray` for these two structs (S21, above). No perf work
beyond the span (D-perf-6). No decoder work.

## 6. Done-test, and the close

1. `SMB` holds no raw pointer; the five context arrays, their allocations, frees
   and `InitMbInfo` wiring are gone; `SMB` re-pinned; the banks ruling is in the
   log; grep for the five old field names in `src/encoder` reads 0 in code.
2. `SMbCache` holds no raw pointer but `pEncSad`; the eight owners are inline, the
   three selectors replace the four aliases; `AllocMbCacheAligned`/`FreeMbCache`/
   `AllocateSliceMBBuffer` are gone; `SMbCache` and `SSlice` sizes measured and
   recorded; the `+16` and its Miri test survive.
3. The second encoder probe exists (CAVLC + non-LOW), green, its cost recorded, its
   name in the `--lib` output; the dynamic-slice path is named for D.
4. Both sweeps 341/341 in both profiles after face 1, after face 2, and at exit;
   both probes green at each face's close.
5. The span is measured per §3 and ledgered, with the tripwire arithmetic stated;
   any reading outside the band has its mechanism named as a candidate.
6. `exit` battery; F3 per S14; log entry ≤ 40 lines; `phase6.md` §6 shows C spent
   and D next with the dynamic-slice probe and the `SMB` list named for it; plan
   §0's row updated; the ratchet's encoder-side numbers before/after with the unit.
