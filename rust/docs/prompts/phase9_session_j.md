# Phase 9 — Session J: the exit — prove the phase's claims, close its ledger, put the final decisions to the user

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs for deeper detail.
Re-run every count before quoting it — trust the tree over this document. Findings
are numbered `F…` in `rust/docs/phase9_findings.md`; yours start at **F199**
(verify: `grep -c '^## F'` prints 98 today). One instruction above all others,
learned expensively across nineteen sessions (S68): before acting on any claim in
this brief or any document, re-read the code it describes — a claim of absence
gets its grep, a cited line gets read, a quoted number gets re-measured.*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the
C++ reference is at the repo root, `codec/`). It ships as a drop-in
`libopenh264` replacement and must stay **byte-identical** to the C++ on every
stream the test harness runs. Phase 9 was the encoder's safety endgame — twenty
sessions retiring raw-pointer families until the fork ran under Miri, the
undefined-behavior interpreter, with real references at the root. You are the
exit: no new conversion campaigns; you prove what the phase claims, reconcile
its exit conditions with the tree's real end state, and put the decisions only
the user can make to the user, with final numbers.

## The session's shape

Three kinds of work, in this order: a small **code cluster** (steps 0–4, each
byte-gated), then the **proof battery** run once over the finished tree (steps
5–6), then the **decision memo and the phase's close** (steps 7–8). One
**mid-session stop**: step 1 ends by presenting a table to the user and waiting
for a ruling before you recategorize anything.

The gates, with honest costs measured on this machine: `bash
rust/tools/gates.sh commit` — byte-identity spot-check plus the unsafe ratchet
(counts may only fall) — **15–19 minutes per run** (the "~2.5 min" in older
docs is wrong; finding F197's session measured it). `gates.sh family` — the
full differential sweep, 583 encoder configurations, both build profiles, every
output byte compared to the C++. `gates.sh session` — adds the Miri battery;
compare the Miri-lane wall to the previous close's **506 s** (>1.3× = stop and
investigate the gate before the tree; the timing is wall-clock and step 2 fixes
that). `gates.sh exit` — the unscoped marathon, this session's step 5. No Miri
between commits (the user's standing directive); do not edit while a gate runs;
one gate at a time. Report unsafe counts from live runs (`bash
rust/tools/unsafe_ratchet.sh report`; today `raw_ptr` **1338**, `unsafe_fn`
**593**), never by differencing the baseline file.

## Step 0 — the parser fixes (the one real code cluster)

Three findings from Phase 8b were folded into this session (decision D-scope-1)
— defects in the decoder's *parse-only* mode, all in NAL/parameter-set
handling (`src/decoder/nalu.rs` — `ParseNalHeader` is at `:902` — and the
parameter-set code near it):

- **F91**: the slice-extension arm rewrites the NAL type byte **in the
  application's input buffer** before copying it out — the library mutates
  memory the caller owns.
- **F92**: the subset-SPS rewrite (re-encoding a subset SPS as a plain SPS so
  parse-only output stays AVC-decodable) has two unbounded writes and a length
  bug.
- **F93**: four parse-only output rows still diverge from the reference.

Read each finding in full first, then the code (S68 — these findings are
months old; F86's citation was wrong when a session finally executed on it).
Fix what can be fixed byte-identically on the decode outputs the harness
compares. Where a fix means *diverging from upstream behavior* — for example,
no longer scribbling on the caller's buffer when upstream does — *stop and
file for a ruling* with the evidence; the user has ruled on every deliberate
divergence this phase (the D-fid series), and this is that pattern.

## Step 1 — the disposition census, then STOP for the user's ruling

The charter's exit condition says every `port-raw` and `cursor` tag retires by
phase end. The tree holds **507 `port-raw(Phase 9)` + 33 `cursor`** tagged
items (the census doc's header; regenerate with `python3
rust/tools/phase9_census.py --write`). Most of that is not debt — it is a
theorem this phase proved (rule S63, finding F146): the encoder's fork hands
one shared context to N worker threads, N workers cannot each hold `&mut` to
one allocation, so the **111 fork-reachable function bodies keep `*mut
sWelsEncCtx` parameters permanently** (`python3
rust/tools/phase9_forksplit.py` confirms the count). Their reads are shared,
their writes are atomic (proven across nine contention rounds), and the
multi-threaded Miri fork probes referee the whole arrangement — green since
session E, re-proven at every close since.

The exit condition was written before that theorem existed. Your job: build
the **disposition table** — every one of the 540 tagged items classified as:

1. **fork-shared (S63)** — the in-fork context/layer families; propose this as
   a new lawful tag category, with the soundness argument written once at the
   category's definition;
2. **ABI-shaped** — values that cross the C boundary and can never carry
   lifetimes (e.g., the bitstream-buffer accessors whose cursor is stored into
   the public `SLayerBSInfo::pBsBuf` field — finding F193); recategorize as
   `C-ABI`;
3. **dormant** — screen-content code Phase 10 will revive (category exists);
4. **named lawful singles** — each with its reason comment: the
   parameter-set-strategy accessor (F166), the kept upstream out-of-bounds
   read at the encode error tail (ruling D-fid-2), the background-detection
   copies (ruling S57), the six documented refusals (F187);
5. **convertible remnant** — genuinely finishable items (expect few; convert
   them or list them with costs);
6. **dead** — zero callers in both trees (delete with quoted greps).

**Then stop and present the table.** The exit-condition amendment (it will be
recorded as D-exit-3) is the user's ruling, not yours. Execute the
recategorization only after it.

## Step 2 — the instrument fixes (both are gate changes; calibrate both ways)

- **F170**: the gates time sweeps and the Miri lane with wall clock, so a
  suspended or busy machine reads as a regression (one session lost time to a
  125× phantom). Fix: report CPU time (user+sys) beside wall in `run_sweep`
  and `s61_report` (`gates.sh:361/:379` and `:136/:168` regions), and point
  the 1.3× tripwire at CPU time. Record the regime change in
  `rust/tools/miri_wall_baseline.txt`'s comment block, the way earlier regime
  changes were.
- **F178**: the unsafe ratchet counts `*mut` and `no_mangle` **inside
  comments** — documentation currently fails gates (two sessions hit it). Fix
  `unsafe_ratchet.sh` to skip comment text; rebaseline with the reason in the
  commit. A known validation exists: one session measured exactly 4 prose
  counts in `raw_ptr` — the fix should move the total by exactly the prose
  count, and you should say what it moved.
- **Regenerate the baseline, and make close-time regeneration standing practice.** The
  baseline snapshot (`rust/tools/unsafe_baseline.json`) was last written six sessions
  ago; the live count has since fallen 1587 → 1338, so the per-file no-increase guard
  carries **249 `raw_ptr` of accumulated headroom** — a file could regress by its slack
  without failing `check`. After the F178 fix lands, run `unsafe_ratchet.sh generate`
  so the snapshot is fresh and prose-free, and record the practice in the plan's §7.1:
  **every session close regenerates the baseline downward** (downward regeneration is
  routine and needs no justification — only an *increase* is a "rebaseline" that
  carries a reason).
- Calibrate every instrument change in **both directions** (rule S66): a
  planted fault must still fail it; a known-clean case must pass. While
  there: the fork-reachability walker and the hazard detector never got
  recorded *negative* calibrations (F159's coda) — record one each, or write
  down why it is moot (the hazard detector was retired as a gate by F163).

## Step 3 — crate root, dependencies, lint hygiene

- `src/lib.rs` has **no crate-root `#![deny(unsafe_code)]`** — add it (the
  per-file denies exist; the root must too). The root's `#![allow(...)]`
  block keeps the naming allows (`non_snake_case` etc. — C++ diffability is a
  requirement) but **`unused_unsafe`, `dead_code`, `unused_variables` should
  narrow or go** — measure what each still suppresses before removing.
- **`libc = "0.2"` is still a dependency with exactly one use**
  (`grep -rn 'libc::' src` — one hit today). Replace it (`core::ffi` has the
  C types since Rust 1.64) and remove the dependency.
- A **clippy pass**: `cargo clippy --all-targets`. Fix the cheap and the
  real; anything suppressed gets an explicit `allow` with a one-line reason.
  No drive-by refactors — this is hygiene, not redesign.

## Step 4 — the log referee's gap list, settled

`rust/tools/diffharness/log_referee.sh` compares both encoders' trace output
on one fixed configuration. Today it captures **cxx 35 lines / rust 20** —
the port went from 1 matching message to 20 during this phase, and the
remaining gap is yours to settle: port the missing messages, or document each
as permanent with its reason (two are known permanent: the C++ prints memory-
arena totals from an allocator this port retired in Phases 3–6, and
synthesizing numbers would be inventing an observable). The "levels
delivered" line closes when the warning-level messages port. Acceptance: the
referee's gap list is empty-or-all-documented, and its exit code reflects it.

## Step 5 — the proof battery (after all code lands; budget half a day)

1. The **per-slot read grep** over every dispatch table: a slot installed and
   asserted but never called is a deletion candidate — session F deleted that
   class; re-run the greps to prove none regressed (exit condition 3).
2. **`gates.sh exit`** unscoped: full sweeps in both profiles, the
   **benches** (`benches/` — first run since the phase froze performance
   work) bit-identical, **whole-library Miri** (not encoder-scoped), the
   differential tests, and the **full gtest suite** — it must tally (a fix
   this phase revived it: 191/199 with an allowlist of exactly 8, including
   the one deliberate timestamp-tiebreak divergence), and the allowlist must
   still be exactly those 8.
3. The **full-drive Miri fork pair**, run in parallel (~58 min — both probes
   together; the numbers to beat are 1.008/1.001 ratios from the last run).
4. The **`!Sync` inventory re-derived by field, not type** (finding F195
   showed the quick probe counts distinct *types* and undercounts): enumerate
   every field of `sWelsEncCtx` that blocks `Sync`, with an owner for each —
   this is the final contingency table for the one hand-written `Send`
   (`slice_multi_threading.rs`, tag `send-seam`), which **stays** per ruling
   D-exit-2, its comment naming exactly what must fall before it retires.

## Step 6 — performance (the freeze lifts here)

The phase-long rule "no perf work" (D-gate-1) ends. Run the bench pair
against the recorded baseline (`rust/docs/perf_baseline.md`); restate the
position against the **+25% tripwire** (D-perf-4; the cumulative encoder
deficit was last measured ≈ +10–12%). Then decision D-perf-6 — a parked plan
to revisit performance-sensitive families now that dispatch is direct — is
**taken or explicitly re-deferred with the user**; do not silently drop it.

## Step 7 — the decision memo

Present to the user, each with the final numbers attached:

- **D-exit-3** — step 1's amendment (presented mid-session; recorded here).
- **D4 — workspace split**: keep the single crate, or split into a pure-safe
  core crate (`#![forbid(unsafe_code)]`) plus a C-ABI wrapper. Attach the
  real end-state numbers: files at `deny` with zero allows, the api
  boundary's size, the fork-shared category's size.
- **D5 — the great rename**: keep the Wels/Hungarian names (maximum C++
  diffability) vs idiomatic renaming with `/// C++:` cross-references.
- **D6 — error style** (i32 codes vs `Result`) if the user wants it decided
  now.
- **D-perf-6's disposition** (step 6), and any ruling step 0 filed.

## Step 8 — the phase's close

The plan's Phase 9 row goes to **DONE** with the final scorecard: unsafe
counts start-of-phase → end (the ratchet's live numbers), the tag table by
category after D-exit-3, the firsts worth recording (the first full
multi-threaded encodes under Miri in the project's history; the fork under a
`&mut` root; every live undefined-behavior defect this phase created caught
by its own gates). The charter's exit checklist ticked item by item, each
with its evidence. The ratchet's remaining job — watching `src/api/`'s size —
documented in the plan's §7.1. Findings from F199; both censuses; the log;
the session gate's numbers as every session has quoted them.

## What to report back

Plain prose: step 0's fixes and any rulings filed; the disposition table and
the D-exit-3 ruling as recorded; each instrument fix with its two-way
calibration; every battery verdict with its numbers (and any failure
adjudicated fully — the flake protocol in `phase0_findings.md` binds if a
sweep misbehaves); the perf memo; the decision memo; the final scorecard;
every place this brief was wrong, quoting the sentence; and what, if
anything, is handed to Phase 10 or to maintenance beyond what the charter
already names.
