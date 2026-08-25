# Phase 9 — Session F: the dispatch survivors — de-virtualize, retire the triple, close sad_common

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F149** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` first.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session F is

Phase 6 session I found five dispatch-slot typedefs that name `*mut
SWelsFuncPtrList` **in their own signatures** — the table handed back into its own
callees — so their functions could never take a reference to the table. Phase 4a
retired this shape once before (`pfMdCost` became a value select); F does it for the
survivors, and takes the prizes that have been queuing behind them:

1. **The motion-estimation threading** (B3 measured it, E2 positioned it): the search
   typedefs stop carrying the table, the search functions take the two pictures, and
   `SWelsME` loses its three raw cursor fields — which retires **the transitional raw
   cost triple** (T9.B25's `pfSampleSad/Satd/4SadRaw` + `md_cost_raw`/`me_cost_raw`),
   deletes the 14 raw SAD shims, and takes **`common/sad_common.rs` to
   `#![deny(unsafe_code)]`** — the file that has been waiting since B2.
2. **Deblocking's two tables** (`PDeblockingBSCalc`/`PDeblockingFilterSlice`, and the
   `DeblockingFunc` struct) de-virtualize; `DeblockingFilterFrameAvcbase` — which
   waits *only* on its `*mut SWelsFuncPtrList` parameter — converts.
3. **F139's write-only slots delete** (S18, the read grep quoted per slot): the three
   `pfIDct*` slots and the eight write-only deblocking slots are installed,
   asserted-installed, and never called. (`pGomCost` is *not* yours — F133's deletion
   was never ruled; it stays atomic.)
4. **The survivors' sweep**: `: *mut SWelsFuncPtrList` appears **24** times today
   (`grep -rn ': \*mut SWelsFuncPtrList' src/encoder | grep -v ':\s*//' | wc -l`);
   what should remain at your close is only the lawful remainder (the C-ABI init
   path's `&mut` installs), and the `cursor` tags on dispatch accessors retire with
   their reason.

**Not this session**: the ctx family (G–H; S63's fork-split of it is *their* brief's
first number); the 35 `*mut SMB` neighbour-walkers and the harvest (E3); F117 (X1);
anything `SCREEN_CONTENT(dormant)` beyond mechanical signature updates its installs
need to keep compiling; the preprocess family.

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `gates.sh commit` (~2.5 min) before each;
  `gates.sh family` (583 rows/profile) after risky ones — a de-virtualization that
  changes *which* function runs is risky by definition; F118's constant-after-init
  argument (one writer, unconditional `_c` installs) is what makes it byte-identical,
  and you state it in the first such commit.
- **No Miri between commits — once at the close**: `MIRI_SCOPE=encoder bash
  rust/tools/gates.sh session` (~8:40 — its sharded lane includes the small-drive
  fork probes, which cover your in-fork ME changes). The **full-drive fork pair is
  not owed this session** — E2 paid it on this tree's shape and the phase exit
  repeats it; if your close's small-drive probes fail, bisect with Miri then, newest
  first. S61: quote the lane wall time against E2's 553 s.
- **No benches, no perf** (D-gate-1). **Ratchet only down**; new raw tagged same-day
  (D-exit-1); rebaselines carry their reason.
- **S20 closures**; **S54** — a parameter (or a whole table argument) is deleted only
  after *every* caller is read; **F101** — enumerate typedefs and signatures with a
  multiline-aware scan, never a one-line grep (E2 paid for this twice).
- **No tree edits while a gate runs; one battery at a time** (§7.5).
- **Stay in lane**; blockers become findings.

## The facts, measured at this brief's commit

### The five self-referential typedefs

| typedef | at | self-ref today |
|---|---|---|
| `PDeblockingBSCalc` | `deblocking.rs:269` | `*mut SWelsFuncPtrList` first param |
| `PDeblockingFilterSlice` | `deblocking.rs:279` | same (its `pSlice` went `&mut` in E2) |
| `PMotionSearchFunc` | `svc_motion_estimate.rs:322` | same |
| `PSearchMethodFunc` | `svc_motion_estimate.rs:329` | same |
| `PLineFullSearchFunc` | `svc_motion_estimate.rs:354` | same |

Why they exist: each callee reaches back into the table for *another* slot — the
search functions for the SAD/SATD cost slots, the deblocking walkers for the edge
filters. **De-virtualization = give the callee what it actually reaches, and the
table parameter dies**: after B2–B4 the cost slots are safe (`SSampleDealingFunc`'s
safe arrays + `md_cost`/`me_cost`), so a search function can take
`&SSampleDealingFunc` (or the two `Option<fn>` values it uses) instead of the table;
the deblocking walkers take the safe filter fns. Phase 4a's `pfMdCost` is the
precedent; read its commit before designing.

### The motion-estimation threading (the session's core)

- `SWelsME` still carries `pEncMb`/`pRefMb`/`pColoRefMb` (`svc_motion_estimate.rs:164+`).
  **B3 verified the coordinate identity** — `pRefMb == colo + mv` at its assignment
  sites, a probe reading **3271/438/5924 agreements, zero disagreements** — and E2's
  slice flip means the search functions' `pSlice` is already `&mut SSlice`.
  Re-verify the identity with the same probe shape (it is two sessions old), then:
  the struct keeps `iCurMeBlockPixX/Y` + the MVs, the three raw fields go, and the
  search/refine functions take `src: &PaddedPlane, refp: &PaddedPlane` (resolved by
  the MD callers via `layer_enc_pic`/`layer_ref_pic`, per call, never stored — S37).
  Under MT these are shared reads of pool pictures — the pattern every session since
  B2 uses, and the close's fork probes referee it (S63: nothing `&mut`, nothing new
  shared).
- **The dispatch reads**: `pfMotionSearch[0]` is a *fixed* index at the live sites
  (`svc_base_layer_md.rs:1079`, `:1138`, `svc_mode_decision.rs:1316`); the one
  runtime-indexed read (`svc_mode_decision.rs:1388`,
  `[iBlock8x8StaticIdc[i]]`) is inside the screen-content static path — verify its
  dormancy (the F125 method: quote the installing conjunction) before treating it as
  mechanical. The installs (`encoder_ext.rs:2446`, a loop over screen-content
  variants) update signatures with the typedef; the dormant variants change
  mechanically and stay tagged.
- **What retires when the threading lands**: the 7 ME-blocked census sites
  (`WelsDiamondSearch` 5, `WelsMotionEstimateInitialPoint` 2 — the plane census's
  last lit `safe-now` rows), then the transitional triple (`md.rs:762+`), the
  `md_cost_raw`/`me_cost_raw` accessors (`md.rs:837`/`:848`, **16** uses outside
  `md.rs`), the 14 `WelsSampleSad*_c` shims, and `sad_common.rs`'s allow-free
  `#![deny(unsafe_code)]`. `sample.rs`'s remaining raw items go with them if their
  last callers are these — grep, don't assume.

### Deblocking's tables and the frame filter

`tagDeblockingFunc`/`DeblockingFunc` (`deblocking.rs:290`/`:303`) — F139 measured 8
of its 10 slots write-only; the two read slots and the one aggregate read
(`deblocking.rs:1227`, `&(*pFunc).pfDeblocking as *const DeblockingFunc`) are the
live surface. Delete the write-only slots with their installs (read grep quoted);
re-type or de-virtualize the two live ones; `DeblockingFilterFrameAvcbase` then
takes what it reaches and its `*mut SWelsFuncPtrList` goes. `TagDeblockingFilter`
(`:198`) is the per-walk parameter struct, not a dispatch table — leave it to E3's
grid work if it holds `*mut SMB`.

## Steps

0. **F139's deletions** (one commit): per slot, the read grep in the commit message;
   slots, installs, install-asserts, and orphaned typedefs go. Byte-neutral by
   construction; `gates.sh commit` proves it.
1. **Re-verify the ME coordinate identity** (probe, reverted) and land the
   threading: typedefs lose the table + gain the planes, `SWelsME` loses the three
   fields, the 7 sites convert (planted fault once — S55/S59, covered rows counted).
   This is the biggest closure; stage it root-down if it needs more than one commit,
   each stage green.
2. **Retire the triple**: raw slots, raw accessors, 14 shims, `sample.rs` remnants;
   **`sad_common.rs` denies with zero allows**. Regenerate the plane census — its
   `safe-now` column should read only the preprocess row and Phase 10's; say so.
3. **Deblocking's tables + `DeblockingFilterFrameAvcbase`**; the frame filter's
   conversion is ST-only (it runs behind `!bDeblockingParallelFlag` — F108), so S63
   does not block it; verify that claim against the callers before relying on it.
4. **The sweep**: `: *mut SWelsFuncPtrList` from 24 down to the lawful remainder,
   each survivor named in the report with its reason; the dispatch `cursor` tags
   retired with their accessors.
5. **Close**: the session gate once; S61's numbers; both censuses regenerated;
   findings from **F149**; the log; the charter row; tags and ratchet re-measured.

**Drop order if short**: 4, then 3 — never 0–2 split apart once step 1 has begun
(the triple half-retired is a worse state than untouched; if step 1 cannot finish,
stop before it with F139's deletions landed and say so).

## What to report back

Plain prose: commits with ratchet deltas; the identity re-verification's numbers;
each de-virtualized typedef with what its callees take now; the read greps for every
deleted slot; `sad_common.rs`'s deny commit and the plane census's final columns;
the close's wall time and S61 ratio; every place this brief was wrong, quoting the
sentence; and what E3, X1–X2 and G–H inherit — including whether any
`*mut SWelsFuncPtrList` survivor is ctx-coupled and therefore theirs, not yours.
