# Phase 9 — Session H3: the LTR accessor returns the reference — 75 sites, one shape, F3's neighbourhood

*Self-contained. Read top to bottom once; then work the steps in order. Every count
below was measured at the commit this brief landed in, with the command beside it —
re-run before quoting, trust the tree over this document (S68: this brief's
predecessor was refuted by one grep). Your findings start at **F197** — verify with
`grep -c '^## F' rust/docs/phase9_findings.md` (96 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the C++
is at the repo root, `codec/`). It ships as a drop-in `libopenh264` replacement and
must stay **byte-identical** to the C++ on every stream the gates run. Phase 9 is the
encoder's safety endgame: every file carries `#![deny(unsafe_code)]`, each raw site
is tagged, and the phase retires them family by family. The plan is
`rust/docs/safety_refactor_plan.md` (rules §7.6, S-numbers); the charter is
`rust/docs/prompts/phase9.md`; findings are `rust/docs/phase9_findings.md`.

## What session H3 is

One conversion, undiluted (D-scope-4): **`ctx_ltr_at` returns `&mut SLTRState`**,
and `ctx_ltr` goes with it if the shape carries. H2 attempted this, measured it,
and reverted under its own pre-set rule — and the measurement is your work list
(F196): converted, the compiler reports **84 errors across 75 distinct sites — 49
E0499, 28 E0503, 5 E0502, 1 E0506: 83 borrow conflicts, zero mechanical type
errors**. The raw return is load-bearing at 75 sites, not the 4 an old comment
documented, and every one of those sites is an unaudited coexistence of the LTR
state with another use of the context. The remedy family is already proven in this
tree: **split-borrows** (H2's `LoadPreviousStructure` precedent — disjoint field
borrows out of one `&mut` context in one function; S63's clarifying clause), plus
hoists, reorders, and S54 parameter-narrowing. Never a re-minted raw.

**The care**: two of the four central bodies — `WelsUpdateRefList`
(`ref_list_mgr_svc.rs:743`) and `WelsMarkPic` (`:1016`) — are live camera-path
reference-list management in **F3's neighbourhood** (the project's historic
byte-alternation flake). The other two — `WelsUpdateRefListScreen` (`:1440`),
`WelsMarkPicScreen` (`:1647`) — are **dark** (F192 measured it: SCREEN_CONTENT is
rejected at `RequestMemorySvc`), so their only referee is the compiler; their
redesign stays minimal and Phase 10 revalidates when the path lives.

**Not this session**: `ctx_frame_bs`/`ctx_frame_bs_cur` (F193: **permanent raw
returns by the ABI** — the cursor is stored into `SLayerBSInfo::pBsBuf`,
`codec_app_def.h:640`, a public C-ABI field; the notes stand); `ctx_dq_idc_map`
(J's inventory); the send-seam table (J re-derives it by field, F195);
F187's refusals; anything in-fork (this family has none — below); no perf.

## Rules that never bend — gating per the user's standing directive

- **Byte-identical every commit**: `gates.sh commit` before each; **`family`
  (583/583 both profiles) after every body** — two of the four are live camera
  path. `sweep.sh` refuses stale drivers; `diffharness/build.sh` after edits.
- **F3's alternation discipline stands armed**: any sweep anomaly in or near
  these bodies — a FAIL that does not reproduce, a short or zero-length output —
  is adjudicated by the F3 protocol (S14; `phase0_findings.md`): 5/5 re-run
  first; a second hit escalates to head-vs-control alternation under load;
  verdicts go in the acquittal ledger. Never explain a flake away, never bisect
  a phantom.
- **No Miri between commits — the session gate once at the close** (S61: lane
  wall beside H2's **551 s**, battery vs the 1200-s cap). **The fork pair is
  not owed**: `ref_list_mgr_svc.rs` has zero in-fork bodies (the forksplit's
  column) and `ctx_ltr`/`ctx_ltr_at` are ST accessors (F165) — this session
  never touches what the fork reads. Say so in the close.
- **The revert rule is inherited and binding** (H2's own): if a body's fix
  turns from relocating bindings into rewriting logic, stop at the body
  boundary, revert the incomplete body, and report the frontier — half-landed
  is worse than reverted, and any leftover needs the user's sign-off.
- **S68** on every count here; **S64** on any family you enumerate; **metrics
  live at both ends** (§7.1: today `raw_ptr` **1345**, `unsafe_fn` **595**);
  F178's prose caveat; no edits while a gate runs; one battery at a time.

## The facts, measured at this brief's commit

- **The pair**: `ctx_ltr` (`encoder_context.rs:880`, the root —
  `addr_of!(pCtx.pLtr)`) and `ctx_ltr_at` (`:910`, `pLtr[kiDid]`), both
  `&mut sWelsEncCtx`-taking since H, both raw-returning under `cursor` tags.
  Callers: `ctx_ltr_at` **28** mentions (`grep -rn 'ctx_ltr_at(' src | grep -v
  'fn ctx_ltr_at'` — F196 counted 26 call sites, 22 of which immediately deref;
  re-derive the split), `ctx_ltr` **5** (3 are sibling-derivation tests, some
  possibly retired with F192's probes — re-check).
- **The census** (F196, reproduced at H2's HEAD): 84 errors / 75 sites / 83
  borrow conflicts / **zero mechanical**. The error kinds map to remedies:
  E0503 (28) is usually a **hoist** — a context field read after the borrow
  moves before it; E0499 (49) is a **split-borrow or narrowing** — the LTR
  state and another context piece used in one breath become one helper's
  disjoint returns, or the callee stops taking the whole context (S54);
  E0502/E0506 (6) are read-side and assignment variants of the same. Classify
  before editing.
- **The four bodies** own the census's mass; the remaining sites are the
  mechanical tail — 22 spellings of `&mut *ctx_ltr_at(..)` /
  `&*ctx_ltr_at(..)` / `(*ctx_ltr_at(..)).field` that collapse to the direct
  call once the bodies compile.
- **The referees for the live pair**: the `ltr` preset (16 configs — LTR
  feedback bitmask × intra period) and `mt`; a planted fault (S55/S59/S64)
  must perturb a field that varies — the marked frame's identity or the
  recovery-request check — and its honest failed-row count is quoted; a 0-row
  reading escalates per F175 before it concludes anything.

## Steps

0. **Reproduce the census** (S60 — run the instrument first, land nothing):
   re-apply the flip (`-> &mut SLTRState`, body `&mut pCtx.pLtr[kiDid]`),
   capture `cargo check`'s full error set, diff against F196's 84/75/83/0 —
   expected-vs-actual stated; drift means the tree moved and the classification
   below starts from *your* census, not F196's. Revert the probe.
1. **Classify the 75 by remedy** — hoist / reorder / split-borrow helper /
   S54-narrowing / genuinely-interleaved (the last is the revert-rule's
   tripwire) — and commit the table as a doc note before the first edit
   (S60: the plan is falsifiable before it runs).
2. **The dark pair first** (`WelsUpdateRefListScreen`, `WelsMarkPicScreen`):
   minimal reorders and splits, compiler-refereed, logic untouched — the
   learning run for the shape. One commit each, gates green (they prove the
   rest of the tree; the dark bodies themselves are compiler-only, said
   plainly in the commit).
3. **The live pair** (`WelsUpdateRefList`, `WelsMarkPic`): one body per
   commit, `family` after each, the classification followed, the revert rule
   armed. Then the planted fault on the landed form (quote `ltr`/`mt` counts),
   reverted.
4. **The flip lands**: `ctx_ltr_at` returns `&mut SLTRState` (a safe fn if the
   body allows), `ctx_ltr` follows or its remaining callers are named; the
   22-spelling tail collapses; the `cursor` tags retire; any surviving raw
   spelling in the family is named with its reason.
5. **Close**: the session gate once (S61 vs 551; battery vs 1200); both
   censuses; findings from **F197**; metrics live at both ends; the log; the
   charter row; **J's inheritance**: ideally one line — "nothing new; the
   exit's ledger is unchanged" — and if it is more than that, each item named
   with its reason.

**Drop order**: there is none — this is one item. If it cannot finish, the
revert rule produces the honest state: completed bodies stay, the incomplete
one reverts, the frontier and the remaining census go to the report, and the
user rules on the leftover.

## What to report back

Plain prose: step 0's expected-vs-actual; the classification table as executed
vs as written; each body's commit with its gate verdict; the planted fault's
honest counts; any F3-protocol adjudication in full; the accessor pair's final
form and every surviving raw spelling with its reason; the close's numbers;
every place this brief was wrong, quoting the sentence; and J's inheritance in
one line if you earned it.
