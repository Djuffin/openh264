# Phase 9 — Session H2: the fork workers' context reads get their lawful end state — the last core-chain session before the exit

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document. The last three briefs were
wrong four, four, and two times, mostly by quoting documents instead of re-reading
code (S68) — find this one's defect and say so plainly. Your findings start at
**F188** — verify with `grep -c '^## F' rust/docs/phase9_findings.md` (87 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session H2 is

The last session on the encoder's core object chain (the charter's "cursor spine":
`sWelsEncCtx` → `SDqLayer` → `SSlice` → the macroblock records and pictures, each
family's raw cursors derived from the one above — converted in that order all
phase) — after it, only J, the exit, remains. H flipped the ST
half of the context family; what remains is **the in-fork half's read surface**:
the fork's workers read pre-fork-stamped context state through raw accessors, and
this session gives those reads their lawful end state. Around that core: a
three-ruling step 0, S67's audit, F167's owed Miri, the five slice-returning
accessor APIs, and the ~36 sites X2 measured as blocked on exactly this surface.

The founding fact (G/H's work): **F132's nine rounds already made every measured
in-fork context WRITE atomic or per-slice** — `pOverallMbMap`, `iSliceNumInFrame`,
the stride tables const-after-init; the dead accumulators are deleted (D-dead-3/6).
What remains in-fork is *reads*, and the phase's exit condition retires every
`port-raw` and `cursor` allow — so "lawful raw" is a waypoint and this session
decides what each read becomes.

**Not this session**: the send-seam's retirement (D-exit-2 — it falls at the exit
when its contingencies do; your job is to *report* F67's count and owners for J);
the 2 `recon-seam` items (D-mt-3); `ParasetStrategy`'s rawness (F166 — though its
*call sites* are yours via the surface); F187's six documented refusals — re-read
each note at the site (S68) and touch one only if your surface dissolves its
stated reason; `SCREEN_CONTENT(dormant)` semantics; the au_parser cluster,
F170/F178's instrument fixes, and D4/D5 (all J's); no perf (D-gate-1).

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `gates.sh commit` before each; `family`
  (583/583 both profiles) after every seam item and every conversion cluster.
  `sweep.sh` refuses stale drivers — `diffharness/build.sh` after edits.
- **No Miri between commits — the close runs the session gate once** (S61: lane
  wall beside X2's **480 s**; the battery against **D-gate-6's amended 1200-s
  cap**), **plus one parallel `MIRI_FULL=1` fork pair** (~58 min — F168's
  settled numbers; per-probe invocations, D-gate-7). The pair **is owed**: this
  session changes what the fork *reads*, which is exactly what the probes
  referee (E2's and H's precedent). If the close fails, Miri bisects then,
  newest first.
- **S62 at every substitution**: a seam accessor that replaces a raw read gets
  stamp-side race-free asserts and outcome-equality where a value is
  substituted; a cross-thread assert must itself be race-free.
- **S68 everywhere**: this brief's citations are one session old; the ~36, the
  refusals, and every count below get re-read at the site before you act. A
  claim of absence gets its grep.
- **S63/S65/S67** are the session's own subject matter: nothing in-fork takes
  `&mut`; a hazard report is filtered by whether its conversion can happen; a
  detector's domain is reachability, not parameters — and you must not add a
  new out-of-family retag while auditing the existing ones.
- **S69** if you touch the referee; **F178's caveat** (the ratchet counts
  prose — respell and say so; the fix is J's); **metrics live at both ends**
  (§7.1: today `raw_ptr` **1356**, `unsafe_fn` **598**, `unsafe_block` **267**,
  `shim` **32**); no edits while a gate runs; one battery at a time; blockers
  become findings.

## Step 0 — three rulings, byte-gated

- **D-fid-4 — the log levels align with upstream's bit mask.** The port's
  definitions: `wels_trace.rs:59–60` (`WELS_LOG_INFO: i32 = 3`, `DEBUG = 4`);
  upstream `codec_app_def.h:323` has INFO = 4 in a bit mask. F184 counted
  **five** duplication sites including `tests/trace_callback_test.rs:41` —
  which hardcodes 3 while its own doc comment cites the header line that says
  4 — **enumerate all five yourself** (`grep -rn 'WELS_LOG_' src tests`, both
  codecs; S64). Remap, correct the test, and the acceptance is pre-built:
  `diffharness/log_referee.sh`'s level check goes green — run it and quote the
  gap list before and after. Byte-neutral on streams (levels reach callbacks,
  not bitstreams); the referee is the instrument that sees it.
- **D-dead-7 — `pCurPath` deleted.** Port sites today: `param_svc.rs:269`
  (field), `:327` (default), `:507` (the store's helper), and
  `wels_encoder_ext.rs:2655` (the `SetOption(ENCODER_OPTION_CURRENT_PATH)`
  store) — re-grep, lines drift. Upstream's three: `param_svc.h:118`,
  `:228`, `welsEncoderExt.cpp:1076`; no reader in either tree — quote both
  greps. The option keeps returning success and now does nothing, observably
  identical (the ruling's text).
- **The F67 probe after both** (the scratch `fn _s<T: Sync>() {}
  _s::<sWelsEncCtx>();`, read the E0277 chain, revert): **ten → nine
  expected** — state expected-vs-actual (S60), and keep the member list; you
  will re-run this at the close.

## Step 1 — S67's audit (before the seam; the audit informs its design)

**23** out-of-family `&mut *pCtx`-class retags today (`grep -rn '&mut
\*pCtx\|&mut \*\*ppCtx\|&mut \*pEncCtx' src/encoder | grep -v ':\s*//' | wc
-l` — H measured 24; one has since gone; re-run and list them). For each site,
F171's method: **what does the surrounding body hold across the retag?** The
class that bit H was a `&mut` projection held in a body that reaches the
context through a local raw — invisible to every parameter-scoped instrument.
Fix what holds something (re-derive through the parameter, or hoist), and
**bless the rest with a one-line comment naming what was checked** — the
audit's product is a per-site verdict table in the log, so J inherits a list,
not a promise. Fixes are byte-neutral; gates prove.

## Step 2 — F167's owed Miri

`CWelsPreProcess::m_pEncCtx` (10 mentions in `wels_preprocess.rs` today) was
argued sound on its dormant half and never run — and F171 is what that
distinction costs. Build the verification: a Miri-driven test that reaches the
dormant read's aliasing shape. Drive the real path if a config reaches it;
if not, a unit drive that **mints the same shapes** (the owner `Box`, the
stored field copy, the flipped `&mut` root, the read) is acceptable — **say
which you built and why it is the same shape**. Green retires the "argued";
a report is a finding with the trace.

## Step 3 — the seam (the session's core)

**The facts**: `phase9_forksplit.py` reads **113** bodies still carrying
`*mut sWelsEncCtx` (111 in-fork + F166 + re-derive the tail — run it). The
in-fork read routes are the **17 in-fork accessors** (F165's corrected
family: the 14 in `encoder_context.rs` + `ctx_ref_pic`/`ctx_pic_ref`/
`current_layer` in `svc_encode_slice.rs`); `ctx_func_list`'s **106** call
sites; the layer's **42** raw parameter mentions; and the **20** `MT` tags
(`slice_multi_threading.rs` 18, `nal_encap.rs:361/:393` — re-verify lines).

**The design duties**:

- **Smallest surface wins.** D-mt-3's template (`rec_view.rs`: one
  `UnsafeCell`-crossing accessor + one `Sync` impl, stamp-side race-free
  asserts, outcome-equality — S62) is the shape for state that is *genuinely
  shared-mutable adjacent*; but a read of **fork-constant** state (stamped
  pre-fork, never written in-fork — the majority, by F132's rounds) may
  instead become a plain shared projection at the accessor. Fewer seam items
  beats views everywhere. Design against the *readers*, per accessor family.
- **Every new seam item is counted and named in your report for the user's
  confirmation.** The count is **2 today** (D-mt-3's, never grown). D-mt-3's
  veto is open; the exit condition's category list amends only by the user's
  ruling on your named list.
- **Nothing in-fork takes `&mut`** (S63) and no new out-of-family retag
  (S67) — the surface is reads, shared, of stamped state.
- **Planted fault per new conversion shape** (S55/S59/S64): perturb a field
  that varies per site; where a verdict is thresholded, escalate to the
  verdict and report both numbers (F175).
- **With the surface landed, the blocked work re-opens**: X2's ~36 sites
  (the `LoadPrevious` shape — four simultaneous projections out of one
  context — becomes lawful when the parameter-set arrays are reached through
  the surface); `ctx_func_list`'s 106 (ST callers: field access; in-fork:
  through the surface or named lawful); the layer's 42 (same treatment); the
  two `nal_encap` seam tags and the 18 `slice_multi_threading` MT tags retire
  where the crossing they mark goes safe, and are named for J where it does
  not. **Re-attempt, don't assume** — and F187's refusal notes are read at
  the site before any of the six is touched.

## Step 4 — the five slice-returning APIs

`encoder_context.rs`: `ctx_ltr:868`, `ctx_ltr_at:885`, `ctx_frame_bs:914`,
`ctx_frame_bs_cur:947`, `ctx_dq_idc_map:968` — all take `&mut sWelsEncCtx`
since H and still return raw pointers under `cursor` tags. Give each a
typed/slice-returning API sized by its callers (S54: read them all first),
convert the callers, retire the tags. ST-side work; the borrow checker is the
gate (H's precedent: a coexistence error is the crux arriving, solved by
splitting borrows, never by minting a raw).

## Step 5 — close

- The session gate once (S61 vs **480 s**; battery vs **1200 s**), then **one
  parallel `MIRI_FULL=1` fork pair** (~58 min). Both green is the core chain's
  proof; a report bisects newest-first.
- **The F67 probe re-run**: the count and the member list **by owner** — this
  is J's send-seam contingency list and the report's most load-bearing table.
- Both censuses (S58); findings from **F188**; the log; the charter row;
  metrics live at both ends; tags re-measured (`cursor` 37 and `MT` 20 today
  crate-wide — say where each went or stays).
- **J's inheritance, written most carefully**: J is the exit — it needs the
  remaining-allows enumeration by lawful category, the instrument-fix list
  (F170, F178), the referee's gap-list ownership (ten messages, two permanent
  by design), the ruling backlog (D4/D5), and every "named lawful" site your
  session leaves, each with its reason on the line above it.

**Drop order if short**: 4, then step 3's re-attempt tail (whatever the
surface has not yet dissolved is named for J with its reason — a list, not a
debt). Steps 0–2, the seam's own items, and the fork pair are never dropped.

## What to report back

Plain prose: step 0's three verdicts (the referee's before/after gap list,
both trees' pCurPath greps, F67 expected-vs-actual); the audit's per-site
table; F167's verification design and its verdict; **every new seam item,
counted and named, with its asserts** — the user confirms the list; the
planted faults' honest counts; what the surface dissolved of the ~36 / 106 /
42 / 20 and what it left, each named with a reason; the five APIs' shapes;
the close's three wall times and ratios; every place this brief was wrong,
quoting the sentence; and J's inheritance as a checklist J can execute
without re-deriving anything.
