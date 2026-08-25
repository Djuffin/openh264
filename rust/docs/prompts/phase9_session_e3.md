# Phase 9 — Session E3: the macroblock grid, the harvest, and G's ground-laying

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F153** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` (52 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session E3 is

Cut large under D-scope-1, with strict drop-from-the-end. Four masses, in order:

1. **Step-0 hygiene**: one S18 deletion from folded session I (F84's — *corrected
   below; the finding is stale in two ways*) plus `WelsGetFirstMbOfSlice`.
2. **The core: the last 34 `*mut SMB` parameters** — the neighbour-walkers — get the
   decoder's grid-shaped answer, and `encoder/deblocking.rs`'s grid-blocked allows
   retire with them.
3. **The harvest**: `SDqLayer`'s two raw picture views (`sRefPicView`/`sDecPicView`)
   re-route onto per-call cursors, and the `cursor`-tagged accessors the flips of
   E2–F orphaned go, with their tags.
4. **G's advance work, droppable whole**: the detector learns *reads* (F148's
   ruling), and the S63 fork-split census of the ctx family is taken. Measurement
   and tooling only — **zero ctx conversions in E3**.

**Not this session**: any `*mut sWelsEncCtx` conversion (G's — you will read raw ctx
constantly and convert none of it, including the ctx parameter of typedefs you
retype for their SMB half); `pGomCost` (F133 is unruled); the `other` family (X's);
`SCREEN_CONTENT(dormant)` beyond mechanical signature updates; the preprocess family.

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `bash rust/tools/gates.sh commit` (~2.5 min)
  before each; `gates.sh family` (583 rows/profile) after risky ones — every grid
  commit is risky. A moved byte is a defect; bisect, don't explain.
- **No Miri between commits — once, at the close**: `MIRI_SCOPE=encoder bash
  rust/tools/gates.sh session` (~8:40; its sharded lane's small-drive fork probes
  referee your in-fork reads). The **full-drive fork pair is not owed** — E2 paid
  it and the phase exit repeats it. S61: quote the lane wall beside F's **528 s**
  and the ratio.
- **S24's clause binds this brief and your report**: a workload count is a grep —
  the command sits beside every number here; re-run it, and quote commands beside
  your own numbers.
- **S54 (+ its whole-tree clause)**: read every caller before deleting anything —
  tests, benches, and other families included. **F101**: enumerate signatures and
  typedefs with a multiline-aware scan, never a one-line grep.
- **S58 (+ F152's clause)**: if your grid respells how SMB access is written, teach
  `phase9_census.py`/`phase9_plane_callers.py` the new spelling **in the same
  commit**; a row that leaves a census by relocation is named in the log.
- **No benches, no perf** (D-gate-1). **Ratchet only down**; new raw tagged same-day
  (D-exit-1); a rebaseline carries its reason.
- **No tree edits while a gate runs; one battery at a time** (§7.5).
- **Stay in lane**; blockers become findings.

## The facts, measured at this brief's commit

### Step 0 — the folded-I deletions, with F84 corrected

The charter row (and D-scope-1) inherited F84 as "two dead threaded-decoder
functions + 18 pad kernels". The tree says otherwise, twice:

- `WelsDeblockingFilterMB` (`src/decoder/deblocking.rs:2295`) has **zero mentions
  beyond its definition** (`grep -rn 'WelsDeblockingFilterMB' src tests benches |
  grep -v 'fn WelsDeblockingFilterMB'` → the count you quote) — a clean S18.
- `WelsDecodeAndConstructSlice` is **not deletable**: T7.C7 kept it and fenced it —
  `decoder_core.rs:4690–4696` calls it under `iThreadCount > 1` behind the
  documented `DECODER_MT(incomplete: F36)` fence ("`GetThreadCount` returns 0"),
  through the wrapper at `decoder_core.rs:824`. Deleting it would reverse a recorded
  decision; leave it, and file the F84 correction as a finding.
- The 18 `MBPad*_c`/`PadMB*_c` kernels **were never ported** (`grep -rn 'MBPad\|PadMB'
  src` → 0; the C++ calls them at `decode_slice.cpp:1731` inside the untranslated
  arm). Nothing to delete.

`WelsGetFirstMbOfSlice` (`svc_enc_slice_segment.rs:673`) has zero callers — its only
two mentions are doc comments (`grep -rn 'WelsGetFirstMbOfSlice' src | grep -v 'fn
WelsGetFirstMbOfSlice'`); delete per S18 and fix the module doc that lists it.

### The grid (the session's core)

**34** `*mut SMB` parameters (`grep -rn ': \*mut SMB\b' src/encoder | grep -v
':\s*//' | wc -l`), by file: `svc_set_mb_syn_cabac.rs` 11, `deblocking.rs` 6,
`md.rs` 4, `svc_mode_decision.rs` 3, `svc_base_layer_md.rs` 3,
`wels_func_ptr_def.rs` 2, `encoder_ext.rs` 2, `svc_set_mb_syn_cavlc.rs` 1,
`svc_encode_slice.rs` 1, `slice_multi_threading.rs` 1.

- **The mint**: `mb_at(pCurLayer, kiMbXY)` over `mb_list_root`
  (`svc_encode_slice.rs:650`/`:660`, both `cursor`-tagged — they retire with this
  work). Walkers read `pCurMb.offset(-1)` / `.offset(-iMbStride)`: they need the
  *array's* provenance plus coordinates, which is why they never converted to
  `&mut SMB`.
- **The model** is the decoder's `safe/mb_grid.rs` (geometry only,
  `#![forbid(unsafe_code)]`, `#[track_caller]` asserts — the F77 instrument note).
  Design the encoder's shape **against the callers**, not by copying: what they
  share is "grid root + my index + a neighbour question".
- **The MT rule** (E2's edge 5, still exact): under the fork a worker's grid use is
  **its own slice's records** — round 5 removed the only cross-slice record reads;
  cross-slice questions go to the atomic `pOverallMbMap`
  (`deblocking.rs:1301`, T9.E4). If a grid shape needs anything else, it is the
  wrong shape. The layer stays raw in-fork (S63): mint the grid *from* the raw
  layer per call; no new `&`/`&mut SDqLayer` inside the fork.
- **Two cross-family carriers**: `PInterMdFunc` (`wels_func_ptr_def.rs:135`-ish) and
  `EntropyCoder::WelsSpatialWriteMbSyn` (`:233`-ish) carry `pCurMb: *mut SMB`
  *beside* `pEncCtx: *mut sWelsEncCtx`. Retype the SMB half with its family (S52);
  **the ctx parameter stays raw** — G's. Enumerate both with a multiline scan
  (F101) before touching either.
- `encoder/deblocking.rs` holds **11** allows (`grep -c 'allow(unsafe_code)'`);
  the site annotations are partial (three say "E3's grid", two say S63-layer).
  Re-derive the split yourself; the S63-layer and any ctx-blocked entries are G's —
  report the remainder by reason.

### The harvest

- **`sRefPicView` / `sDecPicView`** are twin fields on `SDqLayer`
  (`svc_encode_slice.rs:1131`), type `SRefPicView` (`picture.rs:444`): a raw
  per-frame view — `PicPlanes { pData: [*mut u8; 3], iLineSize, dims }` +
  `iPictureType` + a `SCREEN_CONTENT(dormant)` feature-storage pointer — stamped
  once per frame by `WelsInitCurrentLayer` from `SPicture::view()`. Mentions:
  `sRefPicView` **23** (`svc_base_layer_md.rs` 12, `svc_mode_decision.rs` 7,
  `encoder_ext.rs` 2, `svc_encode_slice.rs` 2), `sDecPicView` **4** (`grep -rn
  'sRefPicView' src | wc -l`, same for the twin). The readers are the classic plane
  idiom (`svc_base_layer_md.rs:1001–1008`: read strides, offset roots) — re-route
  them **per call** through `layer_enc_pic`/`layer_ref_pic`-style cursors (S37,
  never stored; F's ME threading is the fresh precedent, and these run in-fork as
  shared reads of pre-fork-stamped pool pictures, which the close's probes
  referee). If both fields then die, say so; if the dormant Phase-10 pointer or
  `iPictureType` needs a home, name it and tag it — don't smuggle the struct back.
- **The tag sweep**: 42 `cursor` tags crate-wide (`grep -rn 'unsafe-cat: cursor'
  src | wc -l`), by file: `svc_encode_slice.rs` 14, `encoder_ext.rs` 3,
  `picture.rs` 1, `paraset_strategy.rs` 1 — and `encoder_context.rs` 23, which are
  **G's, untouched**. Retire each accessor whose last raw caller your conversions
  remove, tag with it; conversions and reclassifications never summed (F128).

### G's advance work (droppable whole, in this order)

1. **`q1c.py` learns reads** — the shape E2's close proved the detector cannot see
   (F148.3: shape D models *mints*; a foreign READ under a fresh protector pops a
   protected Unique). Read F148's three gaps before designing. Calibration (S55,
   both directions): at `020dfb0a^` the new shape must report
   `WelsUpdateSliceHeaderSyntax`'s `iMaxSliceNum` read **line-for-line**; at
   `0bfc7687^` shapes A–D must still report F114a's four bodies unchanged.
2. **The S63 fork-split census of the ctx family**: **287** parameter mentions
   today (`grep -rn ': \*mut sWelsEncCtx' src/encoder | grep -v ':\s*//' | wc -l`).
   Classify the parameter *bodies* by fork-reachability from
   `slice_multi_threading.rs`'s worker entries — F146's method, E2's layer split is
   the worked example. Output: the two-column work list (ST-flippable / in-fork),
   committed as a doc section or script output G's brief can quote. If scripted,
   S58 applies in full (loud exit, calibration). **Zero conversions.**

## Steps

0. **The deletions** (one commit): `WelsDeblockingFilterMB` + `WelsGetFirstMbOfSlice`,
   read greps quoted at the sites; the F84 correction filed as a finding.
1. **The grid**: design against the callers, then convert file-by-file — each commit
   green, `family` after each risky one; the two cross-family carriers retype their
   SMB half only. Planted fault once (S55/S59): skew one neighbour lookup by one MB
   and quote the honest failed-row count (deblocking-sensitive rows should light
   up; a swallowed fault means the row referees nothing — count what actually
   fails).
2. **The harvest**: the two views re-routed per call and (if possible) deleted, with
   their own planted fault (+1 a re-routed ref cursor, quote the count); the tag
   sweep; `WelsGetFirstMbOfSlice` is already gone from step 0.
3. **Detector reads** (tools-only commit): shape E + both calibrations, verdicts
   stated expected-vs-actual (S60).
4. **The ctx fork-split census** (tools/doc commit): the two-column list, its
   method, its numbers.
5. **Close**: the session gate once (S61 vs 528 s); both censuses regenerated (S58's
   respelling duty — the grid *will* respell SMB access); findings from **F153**;
   the log; the charter row; tags and ratchet re-measured.

**Drop order if short**: 4, then 3, then step 2's tag sweep — never split the grid
mid-file; a completed file is a valid stopping point (name the frontier, S60/F143).
Steps 0 and the grid's first file are never dropped.

## What to report back

Plain prose: commits with ratchet deltas; the grid's shape described against three
representative callers (one cabac walker, one deblocking BS walk, one MD site) and
why the fork can't misuse it; the planted faults' honest counts; `deblocking.rs`'s
allow split by reason with the G remainder; the harvest's outcome (fields dead or
what survived and why); the calibration verdicts line-for-line; the census's
two-column totals; the close's wall time and S61 ratio; every place this brief was
wrong, quoting the sentence; and what G and X inherit — G's list is the one to write
most carefully, since after you the encoder spine is ctx alone.
