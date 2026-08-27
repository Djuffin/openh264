# Phase 9 — Session H3: convert the long-term-reference accessor to return a real reference

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs for deeper detail,
but you should not need them to execute. Re-run every count before quoting it —
trust the tree over this document. Findings are numbered `F…` in
`rust/docs/phase9_findings.md`; yours start at **F197** (verify:
`grep -c '^## F' rust/docs/phase9_findings.md` prints 96 today).*

## What this project is, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 (the
C++ reference is at the repo root, `codec/`). It ships as a drop-in
`libopenh264` replacement and must stay **byte-identical** to the C++ on every
stream the test harness runs. Phase 9 is the encoder's safety endgame: every
file carries `#![deny(unsafe_code)]`, each remaining raw-pointer site is tagged
with a comment, and the phase retires them family by family. The master plan is
`rust/docs/safety_refactor_plan.md` (its §7.6 holds the working rules, cited as
S-numbers); the phase charter is `rust/docs/prompts/phase9.md`.

## What this session does

One job. The encoder context struct `sWelsEncCtx` holds per-layer long-term-
reference (LTR) state in a field `pLtr`. Two accessor functions hand that state
out as **raw pointers**:

- `ctx_ltr_at(pCtx: &mut sWelsEncCtx, kiDid) -> *mut SLTRState`
  (`src/encoder/encoder_context.rs:910`) — one layer's LTR state;
- `ctx_ltr(pCtx: &mut sWelsEncCtx) -> *mut SLTRState` (`:880`) — the array root.

This session makes `ctx_ltr_at` return **`&mut SLTRState`** (and converts
`ctx_ltr` too if practical), then fixes everything that breaks.

The previous session already attempted this, captured the compiler's verdict,
and deliberately reverted, leaving you the measurement (recorded as finding
F196): with the accessor returning a reference, `cargo check` reports **84
errors at 75 distinct call sites**, and the breakdown is what matters —

```
49  E0499  cannot borrow `*pCtx` as mutable more than once
28  E0503  cannot use `pCtx.…` because it was mutably borrowed
 5  E0502  mutable borrow conflicts with immutable borrow
 1  E0506  assignment to borrowed value
---
83 of 84 are borrow conflicts. ZERO are simple type errors.
```

Zero simple type errors means none of this is find-and-replace work. At 75
places the code uses the LTR state **and** some other part of the context in
the same expression or across the same call. A raw pointer lets that coexist
silently; a real reference makes the borrow checker adjudicate every case.
Making those 75 coexistences explicit and lawful is the whole session.

## Where the work concentrates, and why care is required

Four functions in `src/encoder/ref_list_mgr_svc.rs` own most of the conflicts:

| function | line | runs when |
|---|---|---|
| `WelsUpdateRefList` | 743 | **every frame, live camera path** — reference-list bookkeeping after each encode |
| `WelsMarkPic` | 1016 | **live camera path** — marks a frame as a long-term reference |
| `WelsUpdateRefListScreen` | 1440 | never — dead code today (see below) |
| `WelsMarkPicScreen` | 1647 | never — dead code today |

**The live two sit in the neighbourhood of this project's oldest flake.** Under
machine load, an encode here has very rarely produced a byte-different or
truncated output (recorded as finding F3, with a full adjudication protocol in
`rust/docs/phase0_findings.md`). The protocol, which binds you: a test failure
that does not reproduce is re-run five times; a second hit escalates to
alternating runs of your commit against a control commit under load; verdicts
are recorded, and you never shrug off a flake or bisect a failure you cannot
reproduce twice.

**The two Screen variants are dead code today, measured not assumed**: screen-
content mode is rejected during encoder initialization (`RequestMemorySvc`
returns "unsupported parameter" — finding F192 verified this), so those two
functions never execute. Consequence: the byte-identity harness proves nothing
about them, the **compiler is their only referee**, and your changes there stay
minimal — reorder and split borrows, never touch the logic. A later phase
revalidates them when screen content is re-enabled.

## The gates, in one paragraph each

- **`bash rust/tools/gates.sh commit`** (~2.5 min) — a quick byte-identity
  check plus the "unsafe ratchet" (counts of unsafe constructs may only fall;
  a rise fails the gate). Run before every commit.
- **`bash rust/tools/gates.sh family`** — the full differential sweep: 583
  encoder configurations, both build profiles, every output byte compared
  against the C++ encoder. **Run after every commit that touches the live
  pair.** A single differing byte is a defect: bisect it, don't explain it.
- **`MIRI_SCOPE=encoder bash rust/tools/gates.sh session`** — run **once, at
  the session's close, and never between commits** (the user's standing
  directive). It adds the Miri (undefined-behavior interpreter) battery.
  Compare the reported Miri-lane wall time to the previous close's **551 s**;
  more than 1.3× slower means stop and investigate the gate itself before
  believing the tree. Total battery budget: 20 minutes.
- The **multi-threaded fork Miri probes are NOT needed this session**: the file
  you are editing has no fork-reachable functions and both accessors are only
  called on the single-threaded path (the fork-reachability tool
  `rust/tools/phase9_forksplit.py` confirms; re-run it). Say so in your close
  rather than silently skipping.
- **`rust/tools/diffharness/sweep.sh` refuses stale binaries** (exit 2): run
  `rust/tools/diffharness/build.sh` after any source edit before hand-running
  a sweep.
- Report unsafe-count changes from **live runs at start and close**
  (`bash rust/tools/unsafe_ratchet.sh report` — today: `raw_ptr` 1345,
  `unsafe_fn` 595), never by differencing the baseline file — it lags.
- Do not edit the tree while a gate is running, and run one gate at a time.

## The method

**Step 0 — reproduce the measurement; land nothing.** Re-apply the flip
(`ctx_ltr_at` returns `&mut SLTRState`, body `&mut pCtx.pLtr[kiDid]`), capture
the full `cargo check` error list, and compare it against the 84/75/83/0 above.
State expected-vs-actual: if the numbers drifted, the tree moved and your
classification starts from *your* census, not the recorded one. Revert the
probe.

**Step 1 — classify every failing site by remedy, and commit the table as a
doc note before editing anything.** The remedies, from cheapest to deepest:

- **Hoist**: a context field read *after* the LTR borrow moves *before* it
  (most E0503s).
- **Reorder**: swap two statements so the borrows no longer overlap.
- **Split borrow**: where the LTR state and another context field are used in
  one breath, one helper method on the context returns both as **disjoint
  field borrows** — Rust accepts `(&mut s.a, &mut s.b)` from a single function
  where two separate accessor calls conflict. There is a worked example in
  this tree: `LoadPreviousStructure` (`src/encoder/wels_encoder_ext.rs:912`)
  returns three parameter-set arrays as one split borrow; copy its shape.
- **Narrow the callee**: a function that takes the whole context but only
  needs the LTR state plus one other field takes those two instead (read every
  caller first — rule S54).
- **Genuinely interleaved**: the LTR state and the context alternate in a way
  none of the above untangles. This is the stop signal — see the revert rule.

**Step 2 — convert the two dead Screen functions first.** They are the
learning run for the shape: minimal reorders and splits, compiler-refereed,
logic untouched, one commit each, and the commit message says plainly that the
byte gates prove nothing about dead code.

**Step 3 — the two live functions, one commit each, `family` after each.**
Follow the classification; the revert rule is armed (below). Then plant one
deliberate fault in the landed form — for example, an off-by-one in which
frame gets marked as long-term — and run the harness's long-term-reference
preset (`sweep.sh ltr`, 16 configurations) plus `mt`: record how many
configurations fail, so you know the tests actually referee this code. A
zero-failure fault is not proof of coverage — escalate the fault until it
moves the output, and report both numbers (rule S59/F175). Revert the fault.

**Step 4 — land the flip.** `ctx_ltr_at` returns `&mut SLTRState` (as a safe
`fn` if the body allows); `ctx_ltr` follows, or its remaining callers are
listed with reasons. The ~22 call sites that today spell `&mut *ctx_ltr_at(…)`
or `(*ctx_ltr_at(…)).field` collapse to direct calls. The accessors'
raw-pointer tag comments come off. Any raw spelling that survives in this
family is named in your report with its reason.

**Step 5 — close.** The session gate once (numbers as above); regenerate the
two census documents their tools maintain (`phase9_census.py`,
`phase9_plane_callers.py`); findings written from F197; unsafe counts from
live runs at both ends; the session log entry; the charter row. The next and
final session is the phase exit — ideally your hand-off to it is one line:
"nothing new; the exit's ledger is unchanged." If it is more than that, name
each item and why.

## The revert rule (inherited from the previous session, binding)

If fixing a function stops being *relocating bindings* and becomes *rewriting
what the function does*, stop at that function's boundary, revert the
incomplete function, and report the frontier. Completed functions stay. A
half-converted reference-list manager is worse than a reverted one, and any
leftover work needs the user's sign-off — it does not silently become debt.

## What to report back

Plain prose: step 0's expected-vs-actual; the classification table as written
and as executed; each function's commit with its gate verdict; the planted
fault's honest failure counts; any flake adjudicated under the F3 protocol,
in full; the accessors' final signatures and every surviving raw spelling with
its reason; the close's numbers; every place this brief was wrong, quoting the
sentence; and the hand-off line to the exit session.
