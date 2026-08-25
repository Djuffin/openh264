# Phase 9 — Session E2: the flips — slice, layer, and the macroblock grid

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F145** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` first.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++ is
at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and must
stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site is
tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session E2 is — and why the flip is safe now when it wasn't before

Phase 6's session J once made exactly this kind of conversion — 109 `*mut ctx`
signatures, root-down in five depth levels, four commits, every one green — **and
reverted it**, because 64 of the 109 had a caller holding a context-derived cursor
across the call, and a `&mut` entry retag kills every one (F66). That measurement
founded this phase's order: hazards to zero first, then the flip.

Session E did the first half for the slice family: `q1c.py --type SSlice` reads **0
hazardous sites across all four shapes**, with the pointers still raw, and both
fork/join probes run green under Miri with their ignores deleted. E2 is the second
half — the flips E priced and unblocked:

1. **`*mut SSlice` → `&mut SSlice`**: 82 bodies, 107 parameter mentions, the 41 arena
   root spellings, and the fn-pointer typedefs that carry a slice — one campaign,
   staged root-down on session J's model, with the revert-reason gone.
2. **`*mut SDqLayer` → `&mut SDqLayer`**: 73 parameter mentions, after the 3 real
   pre-flip fixes (F144: eleven of the detector's fourteen layer "hazards" are a held
   *bool* misread as a held cursor — your step 0 narrows the detector first).
3. **The macroblock grid**: the 35 remaining `*mut SMB` parameters — the
   neighbour-walkers — get the decoder's `MbGrid`-shaped answer, and
   `encoder/deblocking.rs`'s last **13** allows retire with them (except any whose
   raw parameter is the *ctx*, which are G–H's — report the split).
4. **The harvest**: `sRefPicView` (21 reads) retires onto `layer_ref_pic` cursors;
   the `cursor` tags (57, of which ~23 are `ctx_*` accessors that stay for G–H)
   fall with the accessors the flips orphan.

**Not this session**: the `*mut sWelsEncCtx` family (G–H — you will read through raw
ctx constantly and convert none of it); the transitional cost tables and the 7
ME-blocked sites (F); F117 (X1's); anything `SCREEN_CONTENT(dormant)`; the preprocess
family.

## Rules that never bend — including this session's gating directive

- **Byte-identical every commit**: `bash rust/tools/gates.sh commit` (~2.5 min)
  before each; `gates.sh family` (583 rows/profile) after risky ones — and every
  flip stage is risky. A moved byte is a defect; bisect, don't explain.
- **No Miri between commits — Miri once, at the close** (the user's directive for
  this session, 2026-08-24). Between commits your aliasing check is *static*:
  `q1c.py --type <T> --kind ref` after every stage, all four shapes, expected 0.
  The close runs `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (the sharded
  lane, ~8:40 — D-gate-6) **once**, plus **one** full-drive fork-probe pair
  (`MIRI_FULL=1`, per-probe invocations per D-gate-7, ~57 min in parallel — the
  standing cost recorded in the D-gate-6 block of `gates.sh`). If the close fails,
  *that* is when Miri bisects — re-run the failing probe at intermediate commits,
  newest first. Do not run probes mid-session on a hunch.
- **S61's duty at the close**: state the Miri wall time beside E's 517 s and the
  ratio.
- **No benches, no perf** (D-gate-1). **Ratchet only down**; new raw tagged same-day
  (D-exit-1); a rebaseline carries its reason in the commit.
- **S20 closures, staged the J way**: a flip stage is a root-down depth level;
  boundaries to not-yet-flipped callees pass `&mut *pSlice` (a reborrow, dying at the
  call) — the same spelling the fork entry keeps permanently. Every stage compiles,
  gates green, and ends with the detector at 0.
- **Stay in lane**; blockers become findings.

## The sharp edges, in order of how much they cost if missed

1. **The flip changes what every derivation beneath the parameter means (F112 ×
   F114a, S29's clause).** The **41** `addr_of_mut!((*pSlice).sMbCacheInfo)`
   spellings (`grep -rn 'addr_of_mut!((\*pSlice).sMbCacheInfo)' src/encoder | wc -l`
   — E's hazard work *added* derivation windows, so this is 41 now, not the 26 E's
   brief carried) are sound today *because* `pSlice` is raw. In each stage that flips
   a body, its roots convert **in the same commit** to plain
   `&mut pSlice.sMbCacheInfo` field borrows — disjoint-field borrows coexist, and the
   ~40 `&mut *pMbCache` call-site reborrows need no change. Then the stage's shape-C
   scan. Skipping this re-audit is how session D shipped F114.
2. **The fork boundary keeps its raw mint.** Workers reach their slice as
   `*mut SSlice` through the job; that stays. One `&mut *pSlice` reborrow at each
   worker entry, everything beneath is `&mut SSlice`. Multiple workers borrowing
   through the owning `Vec` would be F73 across threads. The close's fork-probe pair
   is the referee; the *static* rule is: no `&mut` to the slice **array** (or the
   layer that owns it) inside the fork, ever.
3. **The fn-pointer typedefs flip with their family (S52).** The worker's count is
   **six** typedefs carrying `*mut SSlice`; a one-line grep finds only two
   (`PDeblockingFilterSlice`, `deblocking.rs:279`; `PWelsCodingSliceFunc`,
   `svc_encode_slice.rs:1308`) because the others wrap past the window — **F101's
   lesson: enumerate with a multiline-aware scan** (the ME slot types
   `PMotionSearchFunc`/`PSearchMethodFunc` are among them,
   `svc_motion_estimate.rs:307+`). Each typedef re-types atomically with its
   installers and callers in the stage that reaches it; the self-referential
   `*mut SWelsFuncPtrList` first parameters stay (session F's).
4. **The layer flip meets the seam.** `SDqLayer` carries `pRecView` — the
   reconstruction seam. A `&mut SDqLayer` entry retag retags the view *struct* (its
   captured pointers are values; the pixel buffers are separate allocations and
   unaffected), but any `RecCursor` or view borrow **held across** a call taking
   `&mut SDqLayer` dies (S37). C's discipline already builds view access per call
   and drops it; keep it that way, and let the close's probes confirm.
5. **The grid, not more cursors.** The 35 `*mut SMB` parameters (by file:
   `svc_set_mb_syn_cabac.rs` 11, `deblocking.rs` 7, `md.rs` 4, `svc_base_layer_md.rs`
   3, `svc_mode_decision.rs` 3, others 7) are neighbour-walkers — they read
   `pCurMb.offset(-1)` / `.offset(-iMbStride)` and need the array's provenance. The
   mint is `mb_at(pCurLayer, kiMbXY)` (`svc_encode_slice.rs:660`); the model is the
   decoder's `MbGrid` (grid + coordinates, `#[track_caller]` asserts — the F77
   instrument note). Design against the callers. Round 5 already removed the fork's
   only cross-slice *record* reads (deblocking's guards ask the atomic map, T9.E4),
   so under MT a worker's grid use is its own slice's records — if a shape needs
   anything else, it is the wrong shape.

## Steps

0. **Narrow the detector (F144), recalibrate, re-measure** (one commit, tools only).
   A held `bool` (or any non-pointer local) is not a held cursor. Narrow shape A's
   binding filter, re-fire against the calibration tree (`0bfc7687^` — F123's
   precedent: the narrowed scanner must still report F114a's four bodies
   line-for-line), then re-run `--type SDqLayer`: expected **3** real sites (all
   shape B), and whatever it actually says is your step-2 work list. State the
   expected-vs-actual verdict (S60).
1. **The slice flip**, root-down, one depth level per commit (J's model, four-to-six
   commits). Each stage: signatures → roots (edge 1) → typedefs it reaches (edge 3)
   → callers reborrow at un-flipped boundaries → `gates.sh commit` →
   `q1c --type SSlice --kind ref` = 0. Final stage ends with zero `*mut SSlice`
   parameters (`grep -rn ': \*mut SSlice' src/encoder | grep -v ':\s*//' | wc -l`
   → 0), the worker-entry reborrows in place, and one `family` run.
2. **The layer pre-fix and flip**: the (expected) 3 shape-B sites fixed raw-first,
   then `*mut SDqLayer` → `&mut SDqLayer` the same staged way — 73 mentions, edge 4
   in force. `mb_at` and the remaining layer accessors either take `&SDqLayer`/
   `&mut SDqLayer` or retire.
3. **The grid**: the 35 `*mut SMB` parameters against the callers, decoder-model;
   `deblocking.rs`'s allows retire as its parameters go safe — report the
   ctx-blocked remainder for G–H.
4. **The harvest**: `sRefPicView`'s 21 reads onto `layer_ref_pic` cursors and the
   field deleted; every `cursor`-tagged accessor whose last raw caller is gone goes
   with its tag; regenerate both censuses (`phase9_census.py --write`,
   `phase9_plane_callers.py`).
5. **Close**: the session gate **once** (the only Miri of the session) + **one**
   `MIRI_FULL=1` fork-probe pair; S61's numbers quoted; findings from **F145**; the
   log; the charter row; tags and ratchet re-measured, conversions and
   reclassifications never summed (F128).

**Drop order if short**: 4, then 3, then 2 — a completed flip stage is a valid
stopping point (each is green and detector-clean), so if step 1 itself must stop,
stop *at a stage boundary*, name the frontier (S60's F143 clause: it is a frontier,
not an inventory), and leave the next stage's work list. **Never drop 0**, and never
leave a stage half-flipped.

## What to report back

Plain prose: commits with ratchet deltas; step 0's expected-vs-actual; the flip's
stage list with each stage's gate + detector verdicts; the close's two Miri runs with
wall times and the S61 ratio against 517 s; the counts re-measured (`*mut
SSlice`/`SDqLayer`/`SMB` at 0/0/what-remains, arena root spellings, `cursor` tags,
`deblocking.rs` allows and the ctx-blocked split); every place this brief was wrong,
quoting the sentence; and what F, G–H, and X1 inherit — G–H's inheritance is the one
to write most carefully, since the ctx family is all that remains on the spine after
you.
