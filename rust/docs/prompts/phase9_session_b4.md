# Phase 9 — Session B4: the referee, the lit campaign, and the inventory correction

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at commit `cee37f9c` with the command shown beside it — re-run the
command before you quote a number, and trust the tree over this document when they
disagree. Most briefs in this phase have been wrong about something structural; this
one corrects its own predecessor in its second section, and you should assume it has a
matching defect of its own — find it and say so plainly. Your findings start at
**F125**.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file already carries `#![deny(unsafe_code)]`, each raw
site has a tag (`// unsafe-cat: port-raw(Phase 9)`, `cursor`, or a boundary category),
and the phase retires them family by family, byte-identical at every commit. The plan
is `rust/docs/safety_refactor_plan.md` (rules §7.6, cited as S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## The correction this brief starts from

Session B3 closed saying its unfinished referee "unblocks 17 sites". **It unblocks 8.**
The 17 dark sites split by *which flag* gates them, and the flags differ:

- **Background detection (BGD)** — installed by `WelsInitBGDFunc(&mut *fl,
  (*pParam).bEnableBackgroundDetection)` (`encoder_context.rs:1606`). The `bg` preset
  the user authorized (D-ref-1) lights exactly these: `WelsMdBackgroundMbEnc`'s **5**
  census sites and `VaaBackgroundMbDataUpdate`'s **3** copy sites (F117's).
- **Scene-change/static-scroll skip (SCD)** — installed by `WelsInitSCDPskipFunc(...,
  bScreenContent && (*pParam).bEnableSceneChangeDetect && iComplexityMode <
  HIGH_COMPLEXITY)` (`encoder_context.rs:1607–1612`). **`bScreenContent` is in the
  conjunction**, so no camera-usage preset can ever light `SvcMdSCDMbEnc` (5 sites),
  `CalUVSadCost` (1), or their `JudgeStaticSkip`/`JudgeScrollSkip` callers. Together
  with the three screen-content motion-search sites B3 already identified
  (`WelsMotionEstimateSearchStatic`, `..Scrolled`, `LineFullSearch_c`), **9 of the 17
  are Phase 10's family mis-filed in Phase 9's plate** — their fix here is a *retag*,
  not a conversion.

The per-function split of all 25 remaining `safe-now` census sites
(`python3 rust/tools/phase9_plane_callers.py --sites`, owner column):

| owner | sites | gate | B4's action |
|---|---:|---|---|
| `WelsMdBackgroundMbEnc` | 5 | BGD → **lit by the preset** | convert |
| `VaaBackgroundMbDataUpdate` | 3 | BGD → lit | **leave raw** — F117, session C's |
| `SvcMdSCDMbEnc` | 5 | screen content | retag `SCREEN_CONTENT(dormant)` |
| `CalUVSadCost` | 1 | screen content | retag |
| `WelsMotionEstimateSearchStatic`/`Scrolled`, `LineFullSearch_c` | 3 | screen content | already/retag |
| `WelsDiamondSearch` + `WelsMotionEstimateInitialPoint` | 7 | lit, ME-blocked | session F's |
| `Process` (`scene_change_detection.rs`) | 1 | preprocess | not this session |

File the split as **F125** with your own re-derivation — quote the
`WelsInitSCDPskipFunc` conjunction, and re-probe one SCD body under the finished `bg`
preset to prove it stays at zero.

## What session B4 does

1. **D-ref-1**: build the `bg` sweep preset and calibrate it — the first byte referee
   the background-detection paths have ever had.
2. Fix whatever parity work the new rows find (they run code no gate has ever run).
3. Convert `WelsMdBackgroundMbEnc`'s 5 sites on the proven route.
4. The inventory correction: retag the 9 screen-content sites' bodies to
   `SCREEN_CONTENT(dormant)`.
5. **D-dead-2**: delete F122's closure (the orphaned sub-8x8 consumers).
6. **D-cov-1** (as corrected): the dead `mc.rs` shims and their two tests go;
   `common/mc.rs` reaches `#![deny(unsafe_code)]` with two dormant-tagged survivors.

**Not this session**: the three `VaaBackgroundMbDataUpdate` copies (F117 — the write
into the picture `pEncMb` reads; session C's, under D-mt-3 — the preset *lights* them
for C, you do not convert them); the 7 ME-blocked sites and the transitional raw
cost tables (session F's — `sad_common.rs`'s deny waits there); everything
reconstruction; `*mut` layer/slice/ctx parameters; the preprocess site.

## Rules that never bend

- **Every commit is byte-identical**: `bash rust/tools/gates.sh commit` (~2.5 min)
  before each; `gates.sh family` after risky ones. A moved byte is a defect — except in
  step 0/1, where a **failing `bg` row is the referee working**: the fix goes on the
  port side until the row passes, C++ is the reference (D-fid-1).
- **Session close**: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (D-gate-4,
  ≈18 min; Miri is 894 s). The verdict names the commit it ran at — re-run if anything
  lands after (B3's rule).
- **No benches, no perf, no unscoped Miri** (D-gate-1/2). **The ratchet only goes
  down**; new raw is tagged same-day (D-exit-1). **S20 closures** per commit, and for
  plane work the closure includes the slot an operand feeds (F118's clause).
- **A dark site converts never, not carefully (S57)** — and after this session the
  reverse duty applies: a *newly lit* site's first conversion carries a planted-fault
  calibration (S55's clause — plant a one-sample error, watch a `bg` row fail, revert,
  note the char in the commit).
- **Stay in lane**; blockers become findings.

## Stacked-Borrows facts (compressed; B3's brief §"Six facts" has the long forms)

(1) `&mut T` parameter retags all of `T` — no caller-held raw survives the call (F66).
(2) A shared `&T` *argument* is a protector for the whole call (F114b/S56).
(3) `addr_of_mut!` under a now-`&mut` parent dies to a sibling safe borrow — derive at
the call, never hold across (F114a/S29). (4) `planes()` is `&mut self` — in-loop
pictures resolve only through the shared routes (`layer_enc_pic`/`layer_ref_pic`)
(S40/F73). (5) Byte gates cannot see aliasing; the close's Miri can, once. (6) `q1c.py
--type <T> --kind ref` before and after each family; all four shapes are calibrated.

## The mechanics you need

### How the harness takes a new axis (the denoise axis is the model — T8b.C1/C2)

`compare.sh` (`rust/tools/diffharness/`) passes one positional argument list to
**both** drivers:

- `compare.sh:4` — the usage line ends `[dlayers] [denoise]`; your axis appends after
  it, with a default that keeps every existing row byte-identical (off).
- `compare.sh:63` — the `TAG` string gains a `${BG:+_bg$BG}` component so row artifacts
  don't collide.
- `cxx_enc.cpp` — `:76` documents the optional-arg convention; `:141` is
  `sParam.bEnableBackgroundDetection = false;` and `:182` shows how denoise reads its
  arg. Mirror that pair.
- `rust_enc/main.rs` — `:57–:62` parse the optional args; `:126` is
  `p.bEnableBackgroundDetection = false;`. Mirror the same pair.
- `sweep.sh` — add `sweep_bg()` shaped like `sweep_ltr()` (`:251`): a small matrix, two
  clips × a handful of configs. BGD needs a background to model, so use the longer
  loopfile (ltr uses 72 frames) and include at least one multi-threaded row —
  `WelsMdBackgroundMbEnc` runs in-fork.
- `gates.sh:215` and `:227` — the header text and the preset list
  (`st mt def sl ltr ps dl`) are **hardcoded**; add `bg` to both. Nothing pins the row
  count (the gate corroborates `PASS=n FAIL=n` with the exit code, `:231–:237`), but
  prose quotes "535" in several docs — update the number where you touch it, with the
  reason (D-ref-1).

### Calibrating the preset (S55/S47 — do this before trusting anything)

1. **Prove entry**: a temporary probe (one print, reverted — F121's method) in
   `WelsMdBackgroundMbEnc` and in `BackgroundDetection` (the analyzer), counted per
   `bg` row. B3's calibration probes read 300–2136 entries on lit paths and 0 on dark
   ones; expect that shape. If a row shows 0, the config doesn't drive BGD — fix the
   row (more frames, static content), not the code.
2. **Prove teeth**: plant a one-sample fault inside `WelsMdBackgroundMbEnc`'s path,
   watch a `bg` row fail, revert.
3. Only then compare for real. **If rows fail un-planted, that is found parity work**:
   the port's BGD path has never been compared. Bisect port-vs-C++ along the path
   (`BackgroundDetection` in `wels_preprocess.rs` / `processing/`, then the MD side)
   and fix to the C++'s bytes. Budget for this being anywhere from zero to the
   session's main event; if it threatens the session, land the preset + findings and
   drop the later steps (the drop order below).

### The conversion (step 3's recipe)

`WelsMdBackgroundMbEnc` (`svc_mode_decision.rs:~557`): 3 MC calls (`McLuma_c` ×1,
`McChroma_c` ×2, zero-MV from the reference picture into `sMemPredMb`/chroma halves)
plus its SAD sites. T9.B22's shape, exactly: reference resolves per call via
`layer_ref_pic(pCurLayer)`, cursor anchored at the C++ arithmetic's sample,
`PlaneCursorMut::new(&mut cache-field[..], 0, stride)` destinations, surviving raws
re-derived after borrows end. Constant-index SAD reads call `sample_sad::<W,H>`
directly (F118's order — the tables are constant after init). `q1c.py --kind ref`
before and after.

**Ordering fact for the same body**: `WelsMdBackgroundMbEnc`'s caller path also runs
`VaaBackgroundMbDataUpdate`, which stays raw (F117) and **writes the current source
picture through raw roots**. That is sound beside your conversions only while no
cursor into the current source picture is held across that call — build cursors per
kernel call and drop them, which the T9.B22 shape does anyway. Say so in the commit.

### D-dead-2's targets (step 5)

Re-derive the closure yourself (S18's F122 clause: the straggler sweep runs *with* the
deletion), starting from: `WelsMdInterMbRefinement`'s three sub-8x8 arms;
`UpdateP4x4MotionInfo`/`UpdateP8x4MotionInfo`/`UpdateP4x8MotionInfo` (6 call sites,
all inside those arms); `SWelsMD`'s `sMe4x4`/`sMe8x4`/`sMe4x8` (`md.rs:191–193` —
**32 `SWelsME` = 3072 bytes of a 4000-byte struct**, so `abi_guard.rs:221`'s
`assert_size!(SWelsMD, 4000)` must be re-pinned with the reason in the same commit);
the `SUB_MB_TYPE_{8x4,4x8,4x4}` arms of the CAVLC/CABAC writers and `svc_encode_mb.rs`
(17 mentions across the three files — some are tables/consts shared with the 8x8 arm;
delete arms, not shared tables, and say which). Every deletion quotes upstream's
`#if 0` chain as D-dead-1 did.

### D-cov-1's execution (step 6)

Delete the **26** shims with no live caller (the 22 F119 named, plus
`McHorVer02_c`/`McHorVer20_c`/`McHorVer22_c`/`PixelAvg_c`, whose last callers left in
T9.B29) **and both surviving tests together** — `mc_shims_stay_inside_the_spans_they_
declare` (`tests/kernels_differential_phase2.rs:251`) and the Phase 4a dispatch
assert-map inside `mc.rs`'s test module (F124's count) — quoting the harness doctrine
and `46053993`. `McLuma_c`/`McChroma_c` **survive**: after step 3 their only callers
are the retagged-dormant SCD bodies, so the two shims are retagged
`SCREEN_CONTENT(dormant)` and retire in Phase 10. Re-type `SMcFunc`'s six slots to the
safe kernels' signatures and install them in `InitMcFunc` (both codecs share the
struct; `abi_guard.rs:184`'s `assert_size!(SMcFunc, 48)` must still hold — fn pointers
keep the size). Then `#![deny(unsafe_code)]` on `common/mc.rs`, with exactly the two
dormant-tagged allows.

## Steps, in order

0. **The preset** (`compare.sh` + both drivers + `sweep.sh` + `gates.sh` lists +
   calibration probes 1–2). Accept: `bg` rows run in both profiles, entry counts
   recorded, the planted fault fails a row and is reverted; every pre-existing row
   still passes.
1. **Parity work**, if any (see above). Accept: `bg` rows PASS in both profiles.
2. *(merged into 0–1's commits where natural)* Docs: the row count re-quoted where
   touched.
3. **Convert `WelsMdBackgroundMbEnc`** (5 sites). Accept: byte-identical including the
   new `bg` rows; `q1c` clean; ratchet down.
4. **Retag the SCD family** (`SvcMdSCDMbEnc`, `CalUVSadCost`, `JudgeStaticSkip`,
   `JudgeScrollSkip`, `MdInterSCDPskipProcess`, `WelsMdInterJudgeSCDPskip`,
   `SetBlockStaticIdcToMd`, and the SC search trio if any tag is still `port-raw`) to
   `SCREEN_CONTENT(dormant: Phase 10)`. Regenerate both censuses. **Report this as
   reclassification, not progress** — the tag count drops without a conversion, and
   the report must say so in those words.
5. **D-dead-2** (the closure, probe-backed, abi pin re-pinned).
6. **D-cov-1** (26 shims + 2 tests go; `mc.rs` denies with 2 dormant allows).
7. **Drop order if short**: 6, then 5, then 4. Never drop 0–3 — the referee and its
   first lit conversions are the session.
8. **Close**: the `session` gate (its sweep now includes `bg` — the close is the first
   full-battery proof of the preset); regenerate censuses; findings from **F125**; the
   log; the charter's B4 row; ratchet + tags re-measured (the census tool's line is
   authoritative: **638 + 58** at your start).

## What to report back

Plain prose: commits with ratchet deltas; every gate verdict (each `bg` calibration
number, the planted-fault row, the final tallies — the sweep total is no longer 535,
state the new number in both profiles); the parity work found, if any, as its own
section however small; the counts re-measured (census columns, tags **split into
converted vs reclassified**, `SPicData` reads remaining); where this brief was wrong,
quoting the sentence; and what C, E and F inherit (C: F117 now lit + whatever pool
aliasing you learned; F: the 7 ME sites + the transitional triple + `sad_common.rs`'s
deny; E: unchanged at `q1c --type SDqLayer` = 14/5).
