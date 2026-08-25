# Phase 9 — Session H: the pair first, then the flip the accessors decide

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F165** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` (64 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session H is

G's chartered second half. G paid the ground: live hazards 131 → 7 (all seven
analysed false positives by allocation, F163), the family at **266 bodies / 111
in-fork / 155 ST-flippable**, the join shipped as a tool. What G did *not* do: run
the full-drive fork pair to completion (stopped at 56 min, healthy, **recorded
incomplete — not green**), or start the flip. H does both, in that order, plus the
in-fork read surface and the harvest.

The flip's crux is the one Phase 6's session J actually died on, resurfacing as
compile errors instead of UB: **after the flip, the accessors cannot coexist.**
`ctx_ltr_at` and `ctx_sps` are themselves ST bodies; two live projections out of
one `&mut sWelsEncCtx` are a borrow error regardless of hazard counts. That end
state — not call depth alone — decides how the 155 stage.

**Not this session**: the `other` family's raw even in your files (X's — F158's
list); the in-fork half taking `&mut` (S63, permanent); the send-seam's retirement
(**D-exit-2**: it stays — 8 of its 12 `!Sync` reasons are outside your lane; the
comment already says so; do not touch it except to keep it true); the 2
`recon-seam` items (D-mt-3); F162's site (**D-fid-2**: the one-past-the-end read
stays as upstream has it — it is commented; leave it); `SCREEN_CONTENT(dormant)`;
no perf (D-gate-1).

## Rules that never bend — gating per the user's standing directive

- **Step 0a runs before any tree edit** (see below): the serial fork pair on G's
  exact close tree. No edits while it runs (§7.5) — plan reading work alongside.
- **Byte-identical every commit**: `gates.sh commit` before each; `family`
  (583/583 both profiles) after every flip stage and every seam accessor.
  `sweep.sh` refuses stale drivers (exit 2) — `diffharness/build.sh` after edits.
- **No Miri between commits — the close runs the session gate once** (S61: quote
  the lane wall beside G's **521 s**). H's close does **not** repeat the full
  pair — step 0a pays G's debt, the session gate's small-drive fork probes cover
  H's own in-fork changes, and the phase exit (J) repeats the full pair.
- **The per-stage gate is the compiler + the byte gates + the join read per
  site** — *not* q1c-to-zero (F163: its shape-B remedy produces shape A on
  pointer hoists, and it models the retag by type while the hazard is by
  allocation; "0" is not writable as an exit criterion). If a stage's `--kind
  ref` report is noisy, either read it against the join or build the
  per-allocation classifier F163 sketched (`DERIV_HINT` is most of the way) —
  do not chase zero.
- **S65/S66** are this session's own inheritance: filter every hazard report by
  fork column and allocation before treating it as work; calibrate anything you
  build on a known negative as well as a known positive.
- **S64/S24/S54/S20/S58/S62** as always. Ratchet only down, live at both ends
  (§7.1); tags same-day; no edits while a gate runs; one battery at a time.
- **Stay in lane**; blockers become findings.

## The facts, measured at this brief's commit

### Step 0a — the serial fork pair (G's debt, paid before anything moves)

`MIRI_FULL=1`, per-probe invocations (D-gate-7), **one probe at a time** —
E8's reference numbers (3356 s / 3449 s) were *serial* numbers; run them the
same way and expect ~115 min total. The un-refereed residue is narrow and named:
**T9.G7's four raw `pLtr` bindings and T9.G6's three pointer hoists in
`slice_multi_threading.rs`**. If a probe reports, bisect newest-first through
G's six step-2 commits; if both are green, G's close is retroactively whole —
say so in the log, and update the Miri baseline note (it currently reads
`fork-pair-NOT-run`).

### Step 0b — D-dead-4, with its scope corrected by the steward

The user ruled "delete both" on F160; verification found **the `pfIDct*` half
was a stale gravestone** — session F's step 0 already deleted the trio (the
installer's note at `decode_mb_aux.rs:482`-region carries the read greps), and
the comment twenty lines above it still claims the slots exist; F160's item 2
inherited that comment's date. What executes: delete `SharedMbArray::capture`
(`rec_view.rs:348`, zero callers —
`grep -rn '::capture(' src | grep -v SharedCells` before and after), and
**correct the stale gravestone** so the file stops contradicting itself.
Byte-neutral.

### Step 0c — the negative calibrations F159 left owed

`q1c.py` and `phase9_forksplit.py` both ship positive calibrations and no
negative one. Record one each: q1c aimed at a fully-flipped family must report
nothing (`--type SSlice --kind raw` exits 2 "nothing to find" — that is the
negative; quote it), and the forksplit walker on a seed-free scope must report
zero in-fork (devise the cheapest honest form; `--no-slots` is a *positive*
calibration, not this). One run each, recorded in the tools' docstrings.

### The flip (steps 1–2): 155 ST bodies, staged by borrow graph

- **The inventory** (`python3 rust/tools/phase9_forksplit.py`; the join:
  `python3 rust/tools/phase9_ctx_join.py --live`): 155 ST bodies; live
  blockers **7**, all analysed false positives — 4 × `pParamD` into the
  coding-param `Box` (a different allocation; the retag cannot reach it), 3 ×
  T9.G6's layer hoists (the correct post-flip form already). Neither blocks a
  flip; both must stay *understood* — re-read the join at every stage.
- **The accessors decide the stages.** 21 `ctx_*` accessors
  (`grep -c 'pub unsafe fn ctx_' src/encoder/encoder_context.rs`): **14 are
  in-fork** (the forksplit doc lists them — `ctx_param`, `ctx_func_list`,
  `ctx_dq_layer`, `ctx_ref_list`, `ctx_rc`, `ctx_rc_at`, the strides, the
  parameter-set arrays…) and stay raw with their callers; **7 are ST bodies**
  (`ctx_mb_index_x/y`, `ctx_ltr`, `ctx_ltr_at`, `ctx_frame_bs`,
  `ctx_frame_bs_cur`, `ctx_dq_idc_map`) and cannot survive the flip as `&mut`
  projections — two live at once is a borrow error. The design direction: **the
  ST accessors dissolve into direct field paths** — disjoint field borrows
  coexist, and the accessors existed to launder raw derivations, which `&mut`
  makes unnecessary. Where a caller genuinely needs two structured views at
  once, a split-borrow helper on the ctx (one `&mut self` method returning
  disjoint `&mut` fields) is the shape; design it against the callers that
  actually collide, not speculatively.
- **The staging is J's model with a borrow-graph eye**: root-down depth levels,
  one stage per commit, each green; boundaries to not-yet-flipped callees
  reborrow; **ST→in-fork boundaries pass raw permanently**. The 13 typedef
  mentions flip with the stage that reaches them (S52, F101 — multiline scan
  only). The two duplicate names q1c warns about (`WelsRcPostFrameSkipping`,
  `WelsSpatialWriteMbSyn`) get their columns confirmed by hand before their
  stages.
- A stage that will not compile because of accessor coexistence is *the crux
  arriving on schedule*: fix it by dissolving the accessor or splitting the
  borrow, never by minting a raw pointer to dodge the checker (that is J's
  ghost) — and never `&mut` anything in-fork (S63).

### The in-fork read surface (step 3)

Unchanged from G's brief, now with G's evidence behind it: F132's rounds made
every measured in-fork ctx *write* atomic or per-slice; what remains is reads
of pre-fork-stamped state through the 14 in-fork accessors. Build the
**smallest** safe shared-view surface on D-mt-3's template (`rec_view.rs`'s
shape: one `UnsafeCell`-crossing accessor + one `Sync` impl, stamp-side
race-free asserts, outcome-equality where a value is substituted — S62).
**Every new seam item is counted and named in your report for the user's
confirmation** — D-mt-3 admitted exactly 2 and its veto is open. A read of
fork-constant state may also become a plain projection at the accessor —
fewer items beats more views. Planted fault once per new conversion shape
(S55/S59/S64): perturb a field that varies per site; honest failed-row counts.

### The harvest (step 4)

The 7 ST accessors retire with the flip; the 14 in-fork ones stay tagged
(theirs is the seam's or the exit's story). `ctx_func_list`'s 106 sites
resolve (ST: field access; in-fork: per-call shared reads of the
pre-fork-write-only table). `encoder_context.rs`'s 23 `cursor` tags fall with
their accessors; `deblocking.rs`'s 4 non-test allows (2 S63 raw-layer drivers,
1 null slot fn, `PerformDeblockingFilter`) convert where the flip unblocks
them; the layer's **42** raw parameter mentions resolve alongside
(`grep -rn ': \*mut SDqLayer' src/encoder | grep -v ':\s*//' | wc -l`). The
**21 `MT` tags** (slice_mt 18, nal_encap 3): retire what the flip and seam
make safe; report the rest with owners. The send-seam stays (D-exit-2).

## Steps

0. **(a)** the serial pair, before any edit; **(b)** D-dead-4 as corrected;
   **(c)** the two negative calibrations.
1. **The stage plan**: the borrow-graph reading of the 155 — which accessors
   collide where, which stages dissolve them — committed as a short doc note
   before the first flip commit (S60: the plan is falsifiable before it runs).
2. **The flip**, staged, each stage green + the join re-read.
3. **The in-fork read surface**, smallest form, items counted.
4. **The harvest**.
5. **Close**: the session gate once (S61 vs **521 s**); both censuses (S58's
   respelling duty); findings from **F165**; the log; the charter row; metrics
   live at both ends.

**Drop order if short**: 4, then 3 — those become X-adjacent or exit items with
the frontier named; step 2 stops only at a stage boundary; steps 0–1 are never
dropped. If the flip itself cannot finish, stop at a boundary, name the
frontier and the remaining stage list — the steward re-plans with the user
(there is no pre-authorized H2).

## What to report back

Plain prose: step 0a's two wall times and verdicts (and the baseline note
updated); the stage plan as executed vs as written; every accessor's fate
(dissolved / split-borrow / stayed raw in-fork); **every new seam item, counted
and named**; the planted faults' honest counts; the close's S61 numbers; the
join's final live list; every place this brief was wrong, quoting the
sentence; and what X and J inherit — J's exit-item list (the send-seam's
contingencies, the full pair, F163's classifier if unbuilt) written most
carefully.
