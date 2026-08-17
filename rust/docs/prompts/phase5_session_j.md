> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session J — T5.J: the multi-MB probe stream, then the hot eleven

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.4 — **D-perf-5's
"Direction on the return" paragraph is this session's charter** — and §7.6
(S2b now carries the day-two clause; S8's fourth negative result stands: **no
window hoisting**; S14/S16/S20/S21/S24/S28/S29 as always); the session-I log
entry. This file scopes the session and supersedes on disagreement; fix
disagreements in place. Counts measured at `e82a6cd0`; re-grep before acting
(S24).

## 0. Start

1. Commit the inherited doc tail (D-perf-5's direction paragraph, §0's rows,
   S2b's extension, this brief).
2. Open per **S27**: session I ended accepted; tail is docs-only — cheap subset
   if `rust/tools/` and the toolchain are unchanged. Last recorded: Miri **324**,
   census **60**, goldens **56**; test counts and `raw_ptr` moved in session I —
   recount.
3. **Run the S2 null at open** — this session issues perf verdicts. Verdict
   standard: **7 interleaved pairs plus a day-two confirmation on any reading a
   decision rests on** (S2b as extended). If the session cannot span two days,
   the confirmation is the *first* item of the next session and the hand-off
   says so.

## 1. Face 1 — the second probe stream (before any family moves)

F34's lesson: the probe's 711-byte stream is one macroblock per frame — no
neighbour path has ever run under the aliasing checker, and the hot families
(`pNzc`, `pMv`, `pMvd`, `pRefIndex`, `pScaledTCoeff`) are exactly the
neighbour-reading ones.

Requirements for the new stream:

- **A real macroblock grid** — multiple MBs per row *and* multiple rows, so
  left/top/top-left neighbour reads execute.
- **Inter prediction with non-zero motion** (a static scene encodes to zero MVs —
  T5.B1's lesson; give it motion), so the MV-prediction neighbour paths run.
- **Both entropy modes** if the probe harness parameterizes; CABAC at minimum
  (F34 was CABAC).
- **Small enough to budget Miri**: the current probe is ~524s on 711 bytes;
  state the new stream's Miri time in the log and keep the pair under ~25 min
  total, or gate the big stream at `full`/`exit` level with the small one at
  commit level — say which, at the site.
- Generate with the C++ encoder (`make_narrow_assets.py` precedent; extend it —
  regenerable, checked in with its command line).
- **Prove coverage the F21 way**: revert F34's fix in a scratch worktree — the
  new stream's probe run must go **red**. A probe stream that cannot re-find the
  known miss is not covering the paths it was built for. Record the red run's
  error site in the log.

## 2. Face 2 — the hot eleven, hottest first

One family per commit, under D-perf-4's normal swap-and-ledger. Order by
measured heat (session H/I's `sample` profiles; re-derive, S24), **hottest
first** so the wire question resolves earliest.

Per family commit:

- Flip as `MbArray<[T; K]>` from the start (the T5.H2 transcription shape);
  in-record indexing is const-bounded — **no window hoisting** (S8 negative
  result #4); hoist nothing that isn't already the C++'s shape.
- S28 full-reach Miri tests for any raw bridge; S29 spellings; S21 if any
  construction path changes; census re-keys in the same commit.
- Both probe streams green (the new one covers what this family touches).
- **Measurement: 7 pairs, both benches.** Ledger the row per D-perf-4. A family
  projecting large on one stream still lands if cumulative holds (D-perf-4's
  letter); log the flag.
- **Stop-line: cumulative CB ≈ +23%** (day-two-confirmed, not single-reading) —
  stop at the family boundary, write the hand-off, escalate to Eugene with the
  table. Parking is family-granular and S20-coherent; its cost is phase-exit
  debt.

## 3. Face 3 — behind the flips, as families land

The remaining `parse_mb_syn_*` cache-fill re-points for flipped families
(mechanical, only for families already flipped). `pBitStringAux` and
`cabac_decoder.rs:855`'s `SHIM(phase5)` accessor die in the family that carries
them, if they haven't already — re-grep (S24).

## 4. Gates

Full battery per family or tight cluster (batch before the long Miri step);
goldens frozen at 56 — the new probe stream's decode goldens, if any are added,
are additive and named in their own commit; sweeps 341/341 both profiles; F3 per
S14 (append at adjudication, reconcile at close); ratchet per S16, per-file
deltas; census green.

## 5. Close

Log entry: the new stream's coverage proof (the red run), per-family ledger rows
with 7-pair tables, cumulative CB tracking after each family, S25/S28/S21 notes
per commit. Update phase5.md §2 marks and §0's Next row. Hand-off: remaining
families (if the stop-line fired: the escalation table), else the rest of 5.2's
re-points, then 5.1's second half or 5.3.

## 6. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). No 5.3 pulls (F22's map still
waits). No `PicPool`/identity (deferred). No golden movement beyond the new
stream's additive rows. No pool/threading (F12/P10). No re-litigating D-perf-5 —
its record is closed; this session executes its direction.
