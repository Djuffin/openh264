# Phase 9 — Session E: the gate repaired, the fork's last race, and the slice/layer families

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F141** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` before believing it. This session is allowed to be two
(E/E2); the drop order says what carries.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site is
tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session E is, in one sentence each

1. **The gate gets repaired first** — D-gate-5 (shrink the two full-encode Miri probes
   under `cfg!(miri)`) plus S61's wall-time tripwire, then **one clean session Miri
   run**, which is the precondition for everything: the encoder's aliasing evidence is
   unverified since `bdf425fd` (F140).
2. **The fork's last shared-state family closes** — round 5 of F132, deblocking's
   cross-slice reads of neighbour `SMB` records — and then **both encoder fork/join
   probes lose their `#[cfg_attr(miri, ignore)]` and run green: that is this session's
   acceptance**, taken *early* so every later commit is refereed by them.
3. **The slice family converts** — 68 measured hazards fixed raw-first, then every
   `*mut SSlice` parameter becomes `&mut SSlice` in one closure that *includes* the 26
   arena roots (F112's one-step) and the S29 re-audit of every derivation under the
   flipped parameter.
4. **The layer family converts** — 14 hazards, then `*mut SDqLayer` → `&mut SDqLayer`;
   the neighbour-bound `*mut SMB` parameters get their grid answer; the last 13 allows
   in `encoder/deblocking.rs` fall out; `sRefPicView` retires.

**Not this session**: the `*mut sWelsEncCtx` family (G–H; you will *read* through raw
ctx constantly and convert none of it); the transitional cost tables and the 7
ME-blocked sites (F); F117's three source copies (X1's, paired with `copy_mb.rs`'s
deny); everything `SCREEN_CONTENT(dormant)`; the preprocess family.

## Rules that never bend

- **Byte-identical every commit**: `gates.sh commit` (~2.5 min) before each;
  `gates.sh family` (583 rows/profile) after risky ones. Round 5's fix included — it
  replaces a value with a provably-equal value, so bytes must not move.
- **Session close**: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` — back to
  ~20 min *after your own step 0*. **S61's duty**: state the Miri wall-time beside the
  previous close's and the ratio; past ~1.3× is a finding, not a shrug.
- **No benches, no perf, no unscoped Miri** (D-gate-1/2). **Ratchet only down**; new
  raw tagged same-day (D-exit-1).
- **S20 closures**; **the D-session playbook for parameter flips** (proven twice):
  fix the detector's hazards *first, with the pointers still raw* — one derivation per
  use, callees narrowed to the field they touch, caller state kept as `usize` offsets
  (S54, F110) — drive `q1c.py` to 0, and only then flip the signatures in one commit.
- **Planted-fault calibration per new shape** (S55/S59), coverage counted in covered
  rows; **expected-next-verdict stated for every instrument re-run** (S60's F136
  clause — it is what caught round 6 being two fields).
- **Stay in lane**; blockers become findings.

## Stacked-Borrows facts, with this session's two sharp edges

The standing five: a `&mut T` parameter retags all of `T` (F66); a shared `&T`
argument is a protector (F114b/S56); `addr_of_mut!` under a `&mut` parent dies to a
sibling safe borrow (F114a/S29); repeated `&mut` of one container is a stack, not
siblings (S40/F73); byte gates cannot see any of this. Plus:

- **The flip changes what the arena roots mean (F112 × F114a).** Today the 26
  `addr_of_mut!((*pSlice).sMbCacheInfo)` roots are sound *because* `pSlice` is raw.
  The moment the parameter is `&mut SSlice`, every one of those derivations gets its
  own item above the parent's `Unique` and any sibling safe borrow pops it. So the
  flip's closure converts the roots **in the same commit** — they become plain
  `&mut pSlice.sMbCacheInfo` field borrows (disjoint-field borrows coexist; the ~40
  `&mut *pMbCache` call-site reborrows need no change) — and the commit ends with the
  shape-C scan (`q1c.py --type SSlice --kind ref`) at zero. F112 priced this as "one
  step instead of thirty-two"; this is that step, and skipping the re-audit is how
  session D shipped F114.
- **The fork boundary keeps its raw mint.** Workers reach their slice through the job
  as `*mut SSlice` today; that stays. Multiple workers holding `&mut` into one
  `Vec<SSlice>` through the *vector* would be F73's whole-buffer retag across threads
  — the sound shape is: the raw element pointer crosses the fork (siblings, disjoint
  elements), each worker reborrows `&mut *pSlice` **once at its entry**, and
  everything beneath is `&mut SSlice`. The two fork probes — live from step 2 onward —
  are the referee for exactly this.

## The map, measured at this brief's commit

### Step 0 — the gate (D-gate-5 + S61), then the clean run

- The two probes to shrink: `encode_loop_runs_over_a_macroblock_grid_…`
  (`svc_encode_slice.rs:4220`, 254 s under Miri at C2's close) and
  `encode_loop_runs_over_size_limited_dynamic_slices_…` (`:4661`, >19 min, killed).
  The model is the house `scale()` helper (`tests/kernels_differential_phase2.rs:38`):
  smaller resolution/frame count under `cfg!(miri)` **only**, full size on every plain
  `cargo test`. What must survive shrinking: each probe's own assertions, and enough
  frames/macroblocks that the paths they exist for still run — state the shrunken
  geometry and *why* it still covers, in the commit.
- **S61's tripwire in `gates.sh`**: the Miri step already logs its wall time; store
  the last close's seconds in a small committed file, print `this/prev` at the end of
  the step, and WARN loudly past 1.3× (warn, not fail — machine variance is real; the
  duty is that the close *report* quotes both numbers).
- Then: **one clean `MIRI_SCOPE=encoder gates.sh session`** at the un-shrunk tree +
  your shrink commit. Expected verdict (S60): everything passes; the two fork probes
  are still ignored at this point. This run is C/C2's missing aliasing evidence —
  say so in the report with the wall time.

### Steps 1–2 — round 5, and the acceptance taken early

Deblocking decides "filter this edge?" by comparing the current macroblock's slice
membership with its neighbour's — by reading **the neighbour's `SMB` record**:

```rust
iMbX > 0 && ((*pCurMb).uiSliceIdc == (*pCurMb.offset(-1)).uiSliceIdc)
```

(`deblocking.rs:841`, `:845`, `:1005`, `:1009`, `:1078`, … — grep `uiSliceIdc`).
Under MT that neighbour can be another worker's *current* macroblock — a record the
worker holds `&mut` over — and that is F132's round 5, the verdict both fork probes
stop at today.

**The lead C2 left you**: `SSliceCtx::pOverallMbMap` is already `Vec<AtomicU16>`
(T9.C2b) and holds each macroblock's slice index. If `SMB.uiSliceIdc ==
pOverallMbMap[iMbXY]` **at deblocking time**, the neighbour read becomes a relaxed
atomic load of the map — same value, same bytes, no `SMB` record touched. Prove the
equality F134's way: a `debug_assert_eq!` at every deblocking read, carried through
the full `family` battery *plus* an `mt` and `bg` sweep, then calibrated by planting
an inequality and watching it abort. If the proof fails (dynamic slicing re-stamps one
and not the other, most likely under `SM_SIZELIMITED_SLICE`), write down which site
disagrees and fall back to the seam pattern: the field deblocking needs moves into a
shared view beside `RecPicView`'s side arrays.

**Iterate the probe, don't assume the read-set** (S60): `uiSliceIdc` is what Miri
named, but deblocking's boundary-strength calculation also reads neighbour `uiMbType`
/ QP / MVs. Fix what the probe names, re-run, state the expected next verdict each
time — either it advances to another field (a finding and the next fix) or it passes.

**Step 2, the moment both probes pass**: delete their `#[cfg_attr(miri, ignore)]`
(`svc_encode_slice.rs:4314` and `:4416`) in their own commit, quote both green runs
and their wall times, and note the Miri-step delta (S61). From here every later
commit in this session is refereed by the fork probes at each Miri run — that is why
the acceptance comes third, not last.

### Steps 3–4 — the slice family

`q1c.py --type SSlice`: **97 bodies (96 names), 68 hazardous sites in 22 callers —
66 shape A, 2 shape B, 0 C/D**. Parameters: **122** `*mut SSlice` mentions
(`grep -rn ': \*mut SSlice' src/encoder | grep -v ':\s*//' | wc -l`), 0 `&mut`.
The arena roots: **26** `addr_of_mut!((*pSlice).sMbCacheInfo)`.

- **Step 3 — hazards to zero, pointers still raw** (several commits, D's playbook).
  Expect the bulk in the entropy writers: `WelsSpatialWriteMbSyn`
  (`svc_set_mb_syn_cavlc.rs:662`) and `WelsSpatialWriteMbSynCabac`
  (`svc_set_mb_syn_cabac.rs:1118`) are the four-deep nesting F112 named — T6.C2's
  comment in the first one records the original Miri verdict; read it before touching.
- **Step 4 — the flip, one closure**: every `*mut SSlice` parameter → `&mut SSlice`;
  the 26 roots → field borrows in the same commit; the fork boundary keeps its raw
  mint with one `&mut *pSlice` reborrow at each worker entry (the sharp edges above);
  end with `q1c.py --type SSlice --kind ref` = 0 in all four shapes and a green
  Miri run of the (now un-ignored) fork probes.

### Steps 5–6 — the layer family, the SMB grid, deblocking's last allows

`q1c.py --type SDqLayer`: **72 bodies (71 names), 14 hazardous sites in 5 callers —
11 A, 3 B**. Parameters: **73** `*mut SDqLayer`, 0 `&mut`. Same playbook: hazards
first, then the flip.

With the layer flipped:

- **The neighbour-bound `SMB` parameters get their answer.** **35** `*mut SMB`
  parameters remain (by file: `svc_set_mb_syn_cabac.rs` 11, `deblocking.rs` 7,
  `md.rs` 4, `svc_base_layer_md.rs` 3, `svc_mode_decision.rs` 3, others 7); the
  neighbour-walkers read `pCurMb.offset(-1)` / `.offset(-iMbStride)` and need the
  array's provenance. The mint today is `mb_at(pCurLayer, kiMbXY) -> *mut SMB`
  (`svc_encode_slice.rs:660`); the model is the decoder's `MbGrid` (grid + coordinate
  accessors, `#[track_caller]` asserts per the F77 instrument note). Design the
  encoder's equivalent against the *callers*: a walker takes the grid (or a row) plus
  the coordinate, not a cursor — and under MT a worker's grid access to *neighbour*
  records is read-only and cross-slice only where round 5 already made it lawful.
  Whatever shape you choose, the fork probes are its referee; if a shape needs a
  `&mut` a sibling worker can invalidate, it is the wrong shape (F73).
- **`encoder/deblocking.rs`'s 13 remaining allows retire** — every one sits on a
  function whose parameters are `*mut SMB` / `*mut SDqLayer` / `*mut sWelsEncCtx`
  (C2 measured). The ctx-taking ones may have to stay for G–H; report the split.
- **`sRefPicView` retires**: **21** reads remain; the picture it copies is
  `layer_ref_pic`'s, and B3's conversions established the cursor route. Delete the
  by-value view when its last reader goes; its screen-content field moves wherever
  its dormant reader needs it.
- **`cursor` tags fall**: 57 today, most on layer/slice accessors (`mb_at`,
  `slice_writer`, `layer_*`) whose reason to exist is the raw parameter you are
  removing. Retire each with its last raw caller; do not leave an accessor whose only
  caller is safe code that could take a reference.

### Step 7 — close

The `session` gate (now with the fork probes in it — state the Miri wall time and the
S61 ratio); regenerate both censuses; findings from **F141**; the log; the charter's
E row; tags and ratchet re-measured, conversions and reclassifications never summed.

### Drop order if E becomes E/E2

Drop 6, then 5, then 4+3 held together (a flip without its hazard work is F114 by
construction). **Never drop 0, 1, or 2** — the repaired gate and the green fork
probes are worth more than any conversion count, and E2 inherits a tree whose every
commit they referee.

## What to report back

Plain prose: commits with ratchet deltas; step 0's clean-run verdict and wall time
(C/C2's evidence debt paid); round 5's mechanism (the equality proof's verdict — or
the seam fallback and why); both fork probes' first green runs; the four family
counts re-measured (`*mut SSlice`/`SDqLayer`/`SMB` parameters, arena roots, cursor
tags, `deblocking.rs` allows); every place this brief was wrong, quoting the
sentence; and what F, G–H, X1 and E2-if-split inherit.
