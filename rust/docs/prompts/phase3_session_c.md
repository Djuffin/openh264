# Session prompt — Phase 3, session C: fix the gate that cannot fail (F17), re-baseline, then T3.2 from a standing start

> **SUPERSEDED — HISTORICAL.** Phase 3 completed 2026-08-11 (sessions A–F). This
> brief is kept as the record of what was asked, not as instructions. The phase's
> outcome is in [`../safety_refactor_log.md`](../safety_refactor_log.md) (entries
> for sessions A–F) and plan §0; the next phase's brief is
> [`phase4b.md`](phase4b.md).

**Governing:** [`phase3.md`](phase3.md) (phase brief), plan §7.6 (S-rules), plan
§2.2.2 [P3] notes, and — read them **in full before anything else** —
[`phase3_findings.md`](../phase3_findings.md) **F16 and F17** plus the session B log
entry. This file scopes the session and supersedes on disagreement; fix
disagreements in place.

**Session shape, in order and not negotiable:** (1) F17 — make `gates.sh` able to
fail, prove it can, re-baseline; (2) T3.2 — the CABAC engine, the phase's
perf-critical seam, from a standing start with a trustworthy instrument. T3.3 is
**read-only prep at most**. If F17 + re-baseline somehow consume half the session,
T3.2 moves to session D whole — a gate you can trust is worth more than a seam
started tired, and the "small thing ran long, start the big thing anyway" pattern
is the logged trap of three phases.

Tree at session start: clean at `331668a7`. **The battery is not trustworthy until
step 1 is done** — that inverts the usual opening: no control run first. Run
everything **foreground and sequentially** this session; session B's corrections
record what background batteries plus foreground `cargo` did to the target lock,
and that a wrapper's exit code was read where `gates.sh`'s own belonged.

---

## 1. F17 — the gate integrity fix

### 1.1 What is actually broken (verified in the current script, wider than F17's two lines)

The script already contains the **sound** pattern — `run_cargo_test`
(`gates.sh:53-71`) captures `${PIPESTATUS[0]}` *and* corroborates by parsing
`test result:` totals from the log, failing on `rc != 0` **or** `failed != 0` or
the ignored-set moving. The broken steps never got that treatment:

| lines | step | status the `if` actually reads | consequence |
|---|---|---|---|
| `237-239`, `255-256` | both Miri steps | `tail -5` — always 0 | **cannot fail, ever** (F17 proper; true since Phase 2's exit) |
| `157` | decode bench | `grep -E 'Rust :\|…\|MISMATCH\|frames'` | a bench that **crashes mid-run** after printing clean rows passes the `if`; the MISMATCH check reads the log, but a non-zero exit with matching output is invisible |
| `186` | encoder bench | same shape | same hole |
| `117-122` | sweeps | ~~pipe status ignored; verdict from parsing `PASS=/FAIL=` tally~~ **— wrong, corrected in place per the header.** `sweep_gate` captures `${PIPESTATUS[0]}` at `:118` and the verdict at `:124` reads *that*; the tally is display text only | so the verdict was sound. The real hole is the one the audit column already named: early death → no tally → `tail -3` fallback text that reads like a result. Fixed as an explicit `fail` |

### 1.2 The fix

- **Both Miri steps** get the `run_cargo_test` treatment: capture
  `${PIPESTATUS[0]}`; parse the libtest `test result:` lines from the log for
  passed/failed; `fail` on `rc != 0` **or** `failed != 0` **or** `passed == 0` —
  the zero-test clause matters, the script's own header comment names the trap ("a
  test that stops being compiled in looks exactly like a test that passes"). Keep
  the `tail -5` display; it was never the problem.
- **Both bench `if`s**: keep the display grep, but take the verdict from
  `${PIPESTATUS[0]}` *and* the existing MISMATCH/DIFFER log check — both must
  hold to pass.
- **The sweep step**: keep the tally-parse verdict, but make the no-tally path an
  explicit `fail` (not a text fallback), and record `sweep.sh`'s own pipe status
  in the log line for diagnosis.
- **Audit the rest of the script** for the shape `if (cmd …) | filter; then` and
  for any bare pipeline whose status matters — the four rows above are what I
  found; the audit is yours to finish and to state complete in the commit message
  (S18's corollary in miniature: "fixed" is a per-step claim).
- **Do not** reach for global `set -o pipefail` as the fix — several greps in the
  script legitimately return 1 on no-match as part of display filtering;
  per-step `PIPESTATUS` capture is the pattern the script already proves out.
- Add an unmissable final line — `OVERALL: PASS` / `OVERALL: FAIL (N steps
  failed)` — printed after the summary, matching the script's exit code, so the
  session-B misread (wrapper exit vs script exit) has nothing left to misread.

### 1.3 Prove the gate can fail — the known-red self-test

A fixed gate is *plausible*; this makes it *proven*, and it costs minutes:

1. Run the `--lib` Miri step once with the **F12 skip removed** from
   `MIRI_SKIPS` (wels_thread_pool) — Miri must fail on F12's retag race, and
   `gates.sh` must print `FAIL miri --lib` and `OVERALL: FAIL` with a non-zero
   exit. That is a *real* known-red input, not an injected fake.
2. Restore the skip list. Run the full battery, foreground, nothing else on the
   machine. It must come back green — and this run is the **new baseline**: the
   first machine-judged full battery since Phase 2's exit. Record both runs in
   the log entry: the red proof (with the FAIL lines quoted) and the green
   baseline (with totals: expect 430/424/20, goldens 2316 unmoved, sweeps
   341/341 both profiles modulo S14, Miri 285/0 — session B's hand-verified
   numbers, now machine-confirmed).
3. One line in the log for the record: every `PASS miri` printed by `gates.sh`
   between Phase 2's exit and this commit was unconditional; the underlying runs
   that mattered (F13's skip deletion, F14, session B's 285/0) were all verified
   by humans reading logs, so no finding is invalidated — but the *gate* starts
   existing today.

Commit: `tools: gates.sh can fail again — PIPESTATUS verdicts, a zero-test trap,
and a proven-red self-test (F17)`.

## 2. T3.2 — the CABAC engine

**Read first:** `cabac_decoder.rs` in full (the engine struct, init at ~`:676-697`,
the end ladder at ~`:732-784`, the handoff at ~`:712-716` — line numbers are
survey-vintage, re-verify), F16's closing section (its warning names this seam),
and `bit_stream.rs`'s corrected extent derivation (`BsReader::avail`,
`readable_from` — the pattern your audit below extends).

### 2.0 Step 0: the read-extent audit — F16's lesson made procedure

F16 happened because a slack constant was derived from *one half* of the reader
and asserted for all of it. **Before converting anything**, enumerate every load
the engine can issue and bound each one on adversarial input, on paper, in the
module docs — the way `READER_SLOP`'s derivation now reads post-F16:

- the init prime (5 bytes at the rewound position `cavlc_pos − iRemainingBytes`);
- the per-renorm byte loads as `uiOffset` refills;
- the 4/3/2/1-byte **end ladder** — F16's named suspect: its selector is a
  distance computation, and the loads it licenses must be checked against
  `BsReader::avail`'s guarantee, not against `len`, and not against an assumption
  the CAVLC side already disproved once;
- the handoff write-back (position only, no load — say so explicitly).

For each: the maximal index reachable as a function of engine state on truncated
input, and whether `avail` covers it. **The claim "avail covers every engine
read" must name each read site individually** — a quantifier over an unenumerated
set is exactly what F16 was. If any load can exceed `avail` (the analog of
`iIndex`'s unboundedness): reproduce the raw behavior through checked reads
routed to the error path, and check whether T3.0's truncated-CABAC rows actually
*reach* that ladder state — if not, **extend the corpus first**: additions only,
`UPDATE_MALFORMED_GOLDEN=1`, and the regeneration diff must show new rows and
zero moved rows (that diff goes in the commit message; F15's rows stay
WITHHELD).

### 2.1 The conversion

- Engine state → `{ pos: usize, range: u64, offset: u64, bits_left: i32 }` over
  the same `BsReader` window T3.1b established. **One extent authority**: the
  engine derives nothing itself — it uses the existing helper; a second
  `readable_from`-style computation anywhere is a defect (T3.3 wants exactly one
  thing to delete, and F16's second instance — `ExpandBsBuffer` leaving `avail`
  stale, still live, still unexercised by the battery — dies when T3.3 deletes
  that function; this seam must not add sites that consume `avail` between a
  growth and a refresh, and the engine docs must name the staleness hazard and
  its owner).
- Init: the rewind `pos = cavlc_pos − remaining` gets a `debug_assert!` that it
  cannot underflow, with the comment saying *why* it cannot today (primed cursor
  ⇒ position ≥ 4 bytes); the end-guard (`pEndBuf − 1`) semantics preserved as
  arithmetic on `len`.
- The ladder: byte-for-byte the same selector and loads, expressed as
  `len`/`avail`/`pos` arithmetic per the audit.
- **The handoff becomes one `usize` assignment** — and gains the pinning test:
  a CAVLC→CABAC→CAVLC round-trip at a known bit offset, asserting full cursor
  state, both profiles.
- Consumers: `ctx.pCabacDecEngine: *mut SWelsCabacDecEngine` follows the T3.1b
  precedent — **retype the pointer** to the converted engine type; the
  `parse_mb_syn_cabac.rs` call sites get call-expression edits only
  (`(reader_window, &mut engine)`-shaped), no struct or logic reshaping (Phase
  5's). Deviations from this default shape: allowed with the reason written in
  the log, [P3]-style.

### 2.2 The perf protocol — this seam is why the session starts it fresh

`DecodeBinCabac` was 4a's largest single decode consumer (544 self-samples). S1,
in this order, no shortcuts:

1. **Disassemble the raw hot path first** (from the release bench binary) and
   write down its shape: which state lives in registers across the renorm loop,
   how many loads per bin, where the branches are. That is the "as good as"
   reference — you cannot recognize a regression in the converted code without
   it.
2. Convert.
3. Disassemble again and compare against the reference. **Any bounds check
   inside the per-bin path is a restructure-now defect** — the known-good move
   is the exact-span window trick (trim the slice once per refill boundary so
   the per-bin indexing folds; T8 → G-1 precedent), not `get_unchecked`, which
   stays banned (S8) no matter how hot the loop reads.
4. `perfpair.py`: one interleaved pair, both benches. **The CABAC rows (Main,
   High) are the signal this time** — they sat inside the floor at T3.1b
   because their bit work is *this* seam's. CB is the cross-check (should be
   flat; its reads are CAVLC's). Fresh null first (the floor is per-session,
   S2). Tripwire arithmetic from decode ≈ +16.9 / +10.1 / +9.6 (post-T3.1b);
   D-perf-4 as always — and if the fold works the way §4 of the phase brief
   predicts (literal bit-counts staying literal), flat-to-win is the expected
   band, so treat a loss as a defect to understand via the disassembly diff
   before ledgering it.

### 2.3 Seam close

Full battery (the *fixed* battery) + goldens unmoved-or-additively-extended per
2.0 + the handoff round-trip test + Miri including the new engine tests + ratchet
shape (`raw_ptr` falls with the engine's pointer triple; `SHIM(phase3)` only if a
shell was genuinely needed) + log entry with the read-extent audit reproduced and
the disassembly verdict + Progress checkbox + hand-off naming T3.3 (the ownership
seam: `RawDataBuffer`, `nalu.rs` ranges, `ExpandBsBuffer` deleted **and F16's
stale-`avail` instance with it**, F15 fixed and its WITHHELD rows un-withheld via
the regeneration-diff proof).

## 3. Meta-rules this session inherits from session B (new or sharpened)

- **Regression-vs-pre-existing, the corrected principle** (session B caught the
  brief giving wrong guidance here): when *your* conversion narrows a window the
  raw code had, a T3.0 abort is **your regression until proven otherwise** — the
  disambiguation test is "does the raw reader, on the same input, read those
  bytes successfully?" If yes: regression, fix the window. S12's
  quarantine-as-pre-existing path is for divergence the *raw* code already
  exhibits (F15's shape), and taking it wrongly hides a regression behind
  withheld rows. When in doubt, run the input against the pre-seam commit.
- **Battery hygiene:** foreground, sequential, nothing contending on the target
  dir; the verdict is `gates.sh`'s `OVERALL` line and exit code, nothing else.
- **Alternation runs use a separate worktree** for the control side (session B's
  S14 execution is the worked example — 6 rounds × 120 configs per side, one
  loop), and F3-signature hits in **debug** are normal since the driver went to
  `opt-level = 3`; the ninth measurement's control-heavy split (control 2, HEAD
  0) is the expected shape of innocence.

## 4. Non-goals

No T3.3 beyond read-only inventory (the extent helper dies there, F15 is fixed
there, the stale-`avail` dies there — none of it early). No `ExpandBsBuffer`
patching. No writer-side work (T3.4+). No residual-path or `parse_mb_syn_cabac`
restructuring (Phase 5). No golden regeneration except the additive corpus
extension licensed by 2.0, proven additive by its diff. No new Miri skips — if
the engine conversion trips Miri on *raw* neighbor code, that's a finding (S12)
with the F14 precedent, not a skip. No `get_unchecked`. And no starting T3.3
because T3.2 finished early — spend the surplus on the read-extent audit's prose
and the disassembly comparison, which are exactly the artifacts session D and
Phase 5 will lean on.
