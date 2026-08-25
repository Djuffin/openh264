# Phase 9 — Session G: the context family — hazards to zero, the ST flip, the fork's read surface, and the seam

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. Briefs in this phase are
reliably wrong about something structural; find this one's defect and say so plainly.
Your findings start at **F159** — verify with `grep -c '^## F'
rust/docs/phase9_findings.md` (58 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session G is

The spine's last family. After E3 the encoder's per-macroblock world is safe; what
remains raw on the spine is the one object every worker shares — `sWelsEncCtx` —
plus the layer's in-fork half, which moves with it (F158: "both are the object every
worker shares"). Your step 0 arrived pre-paid from E3: the fork-split census exists
as a tool and a document, and the detector knows the read class that bit E2's close.

Phase 6's session J is the ghost at this table: it once flipped 109 ctx signatures
root-down, every commit green, **and reverted the whole campaign** because 64 of the
109 had a caller holding a context-derived cursor across the call (F66). This
session does what J could not, in the order E→E2 proved on the slice family:
hazards to **zero with the pointers still raw**, then the flip — and only the half
of the family that fork-reachability permits (S63).

**H exists only as the spillover name.** A stage boundary with the frontier named
(S60/F143) is the only legal stopping point.

**Not this session**: the `other` family's raw even where it shares your files —
`rc.rs`'s remaining raw beyond ctx and `pGomCost`, the VAA flag walks, residual
cursors, MVD cost tables (all X's, F158); F86/F100/F117 (X's); the au_parser
cluster (J's); the 2 `recon-seam` items (D-mt-3 stands, open to the user's veto);
`SCREEN_CONTENT(dormant)` beyond mechanical updates; no perf work (D-gate-1).

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `bash rust/tools/gates.sh commit` (~2.5 min)
  before each; `gates.sh family` (583 rows/profile) after risky ones — every flip
  stage and every seam accessor is risky. A moved byte is a defect; bisect, don't
  explain. `sweep.sh` now **refuses stale drivers** (exit 2) — run
  `diffharness/build.sh` after edits before any hand probe; that guard exists
  because E3 lost two probes to it.
- **No Miri between commits — at the close only**: `MIRI_SCOPE=encoder bash
  rust/tools/gates.sh session` once, **plus one `MIRI_FULL=1` fork-probe pair**
  (per-probe invocations, D-gate-7; ~3300–3450 s each, ~57 min as a parallel
  pair). The pair **is owed this session** — you are changing what the fork
  reads and how it crosses; E2's precedent. If the close fails, Miri bisects
  then, newest first. S61: quote the lane wall beside E3's **536 s**.
- **Static aliasing check between stages**: `python3 rust/tools/q1c.py --type
  sWelsEncCtx --kind ref` after every flip stage, **all five shapes**, expected 0.
- **S64**: a grep bounds a spelling, not a relationship — the family is 285
  parameter mentions = 268 bodies + 13 typedef mentions + 4 second-parameter /
  `*mut *mut` spellings (the forksplit doc's reconciliation; the brief's own
  one-line typedef grep returned 0 — F101's multiline lesson, again). Enumerate
  the type, and aim any planted fault at a field that varies per site.
- **S24/S54/S20/S58/S62** as always: counts are greps with commands; read every
  caller (whole tree) before deleting; compiling middles; instruments stay loud;
  a cross-thread assert must itself be race-free — prove at the stamp, compare
  outcomes.
- **No tree edits while a gate runs; one battery at a time** (§7.5). Ratchet only
  down; new raw tagged same-day; rebaselines carry reasons.
- **Stay in lane**; blockers become findings.

## The facts, measured at this brief's commit

### The two pre-paid instruments (step 1 re-runs both — S60)

- **The fork split** (`python3 rust/tools/phase9_forksplit.py`;
  `rust/docs/phase9_ctx_forksplit.md` is the snapshot, the tool is the authority):
  **111 in-fork / 157 ST-flippable over 268 bodies**. Seeds are the three
  `std::thread::scope` blocks (`slice_multi_threading.rs:1483/:1551/:1768`); the
  decisive arm is the static fn-item array `g_pWelsSliceCoding` (F157;
  `--no-slots` is the shipped calibration, 27 vs 111). Per file: `rc.rs` 10/45,
  `svc_encode_slice.rs` 37/12, `ref_list_mgr_svc.rs` 0/32, `encoder_context.rs`
  14/13, `svc_mode_decision.rs` 19/1, `encoder_ext.rs` 0/20,
  `svc_base_layer_md.rs` 15/0, `wels_preprocess.rs` 0/12, the rest small.
- **The detector** (`python3 rust/tools/q1c.py --type sWelsEncCtx --kind raw`):
  **266 hazardous sites in 69 callers across 82 ctx-taking callees** — shape A
  205 sites over **44 distinct held cursors**, shape B 61, C/D/E 0. Concentration
  is the plan: `encoder_ext.rs` 122 + `ref_list_mgr_svc.rs` 93 = **81%** of all
  sites. The most-implicated callees are the accessors themselves: `ctx_param`
  55, `ctx_frame_bs_at` 24, `ctx_ref_list` 19, `current_layer` 19, `ctx_rc_at`
  10, `ctx_sps` 9, `ctx_vaa` 9, `DeleteLTRFromLongList` 8. Caveat the tool
  prints: two duplicate names (`WelsRcPostFrameSkipping`,
  `WelsSpatialWriteMbSyn`) — a text scan, not a resolver.
- **The join is your work list**: hazards ∩ ST column = the pre-flip fixes;
  hazards ∩ in-fork = raw-soundness fixes done raw-first (their bodies never
  flip). Both instruments emit lists; join them and commit the joined list with
  step 1 (expected-vs-actual stated, S60).

### Step 0 — D-dead-3, ruled by the user 2026-08-25

**`pGomCost` is deleted whole**: field (`rc.rs:376`), default (`:457`),
allocation (`:792`), per-frame zeroing (`:1565`), per-MB accumulate (`:2413`) —
the write-only GOM cost accumulator, F133's upstream data race, never read in
either tree (upstream's five sites: `rc.h:191`, `ratectl.cpp:79/90/669/1273`; its
three sibling GOM arrays are the live mechanism). Quote the read grep for both
trees in the commit. The **seven comment cross-references** update
(`encoder_ext.rs:1214`, `slice_multi_threading.rs:285/:299`, `rec_view.rs:340`,
`rc.rs:320`, `abi_guard.rs:274`, `decode_mb_aux.rs:463` — re-grep, don't trust
this list), and `assert_size!(SWelsSvcRc, 440)` (`abi_guard.rs:277`) re-pins with
the reason. Byte-neutral; `gates.sh commit` proves it.

### The hazard campaign (step 2) and the ST flip (step 3)

- Shape A's remedy is **retiring the cursor's family**: the wide accessors narrow
  to the fields callers touch (S54, E's toolkit — narrowings, hoists,
  per-use-cluster derivation windows). Narrowing `ctx_param` alone implicates 55
  sites. Shape B's remedy is hoisting the argument. All raw-first — the detector
  at 0 **before** any signature changes.
- The flip is J's model as E2 ran it: root-down depth levels, one stage per
  commit, boundaries to not-yet-flipped callees reborrow — and the **ST→in-fork
  boundary passes raw permanently** (a flipped ST caller hands `pCtx` back as
  `*mut` to an in-fork callee; that spelling is the end state, not a debt).
- The **13 typedef mentions** flip with the stage that reaches them (S52), found
  by multiline scan only (F101): `PInterMdFunc` (its SMB half went safe in E3;
  the ctx half is yours) and `PWelsCodingSliceFunc`/the `g_pWelsSliceCoding`
  rows are among them — but the in-fork typedefs' ctx parameter **stays raw**
  with their bodies (S63).

### The in-fork read surface (step 4) — the session's design work

S63's two end states for the 111 were "interior mutability (the seam's shape) or
lawful raw" — and the phase's exit condition retires **every** `port-raw` and
`cursor` allow, so lawful raw is a *waypoint*, not where the phase ends. What
makes this tractable now: **F132's nine rounds already made every measured
in-fork ctx WRITE atomic or per-slice** (`pOverallMbMap`, `iSliceNumInFrame`,
the stride tables const-after-init; `pGomCost` dies in your step 0). What
remains is the **read surface**: the 14 in-fork `ctx_*` accessors
(`encoder_context.rs:632`–`:1016` — `ctx_param`, `ctx_func_list`, `ctx_dq_layer`,
`ctx_ref_list`, `ctx_rc`…, the forksplit doc lists them) and the in-fork bodies
that read pre-fork-stamped state through them.

Build the smallest safe shared-view surface that covers the *measured* reads, on
**D-mt-3's template** (`rec_view.rs`: one `UnsafeCell`-crossing accessor + one
`unsafe impl Sync`, stamp-side race-free asserts, outcome-equality where a value
is substituted — S62). **Each new seam item is counted and named in your report
for the user's confirmation** — D-mt-3 admitted exactly 2 items and its veto is
open; the exit condition's category list amends only by that ruling. Do not
overbuild: a read of pre-fork-stamped, fork-constant state may also become a
plain `&` projection through a raw deref *at the accessor*, which is fewer seam
items than views everywhere.

### The seam itself (step 5)

`slice_multi_threading.rs:1242` carries the phase's **one hand-written `Send`**
(`send-seam(Phase 9)`, decision D-mt-1). Its own comment names the retirement
condition: it goes "when Phase 9's context split makes this handle naturally
`Send`" — and records that **`sWelsEncCtx` is `!Sync` for twelve distinct
reasons (F67), five of them inside types Phases 8 and 10 own**. That inventory
predates Phase 8's completion. **Re-derive F67's twelve at HEAD** before
designing: retire the seam if the survivors fall to your conversions and the
atomics; if Phase-10-owned reasons survive, narrow the justification, name the
residue as the exit condition's amendment, and file it — do not force it. The
**21 `MT` tags** (`slice_multi_threading.rs` 18, `nal_encap.rs` 3) retire with
the machinery that goes safe; report any that turn out to belong to X with the
reason.

### The harvest (step 6)

The **21 `ctx_*` accessors** and `encoder_context.rs`'s **23 `cursor` tags**
retire as their callers flip or their seam replacements land; `ctx_func_list`'s
**106 call sites** resolve (ST: plain field access; in-fork: per-call shared
reads of the pre-fork-write-only table — F's argument). `deblocking.rs`'s **4**
non-test allows are yours (2 S63 raw-layer drivers, 1 null slot fn,
`PerformDeblockingFilter` converts with its family). The layer's in-fork remainder — **42** raw parameter mentions today (`grep -rn ': \*mut
SDqLayer' src/encoder | grep -v ':\s*//' | wc -l`; E2 measured 43, E3's retired mints
took one) — resolves alongside the ctx work (same treatment, same probes). Planted fault once
per new conversion shape (S55/S59/S64): perturb a field that varies per site —
a seam-accessor misindex, honest failed-row counts quoted.

## Steps

0. **D-dead-3** (one commit, byte-neutral, both read greps quoted).
1. **Instruments re-run** (S60): forksplit expected 111/157/268, q1c expected
   266/69/82 — state expected-vs-actual; commit the joined work list.
2. **Hazards to zero, raw-first**: accessor narrowings + hoists; `q1c --kind raw`
   → 0 across all shapes with signatures unchanged.
3. **The ST flip**: 157 bodies in root-down stages, each stage green + `--kind
   ref` 0; typedefs with their stages; ST→in-fork boundaries pass raw.
4. **The in-fork read surface**: the smallest seam that covers the measured
   reads; every new item named; S62 discipline at every substitution.
5. **The seam**: F67 re-derived; `Send` retired or narrowed with the residue
   named; MT tags settled.
6. **The harvest**: accessors, tags, `ctx_func_list`, deblocking's 4, the
   layer's 42.
7. **Close**: the session gate once + **one** `MIRI_FULL=1` fork pair; S61's
   numbers; both censuses (S58's respelling duty); findings from **F159**; the
   log; the charter row; tags and ratchet re-measured **live at both ends**
   (§7.1's clause — the baseline JSON is the gate's memory, not the
   measurement).

**Drop order if short**: 6, then 5 — those become H's list with the frontier
named. Step 4 stops only at a boundary where both fork probes are green and
every landed seam item is complete with its asserts. Step 3 stops only at a
stage boundary. Steps 0–2 are never dropped, and no stage is ever left
half-flipped.

## What to report back

Plain prose: commits with ratchet deltas (live at both ends); step 1's
expected-vs-actual; the hazard campaign's shape (which narrowings cleared which
blocks); the flip's stage list with gate + detector verdicts; **every new seam
item, counted and named, for the user's confirmation**; F67's re-derived list
and the send-seam's fate; the close's two Miri runs with wall times and the S61
ratio against 536 s; the planted faults' honest counts; every place this brief
was wrong, quoting the sentence; and what H (if it fires), X, and J inherit —
X's inheritance in your shared files (`rc.rs`, `ref_list_mgr_svc.rs`,
`wels_preprocess.rs`) is the list to write most carefully.
