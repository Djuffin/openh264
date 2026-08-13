# Phase 5, session K — T5.K: the day-two confirmation, then the remaining ten

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.4 — D-perf-5's "Direction on
the return" is still the charter, and session J executed its first half — and §7.6 (S2b
with its day-two clause; S8's fourth negative result stands: **no window hoisting**;
S13/S14/S16/S20/S21/S24/S25/S28/S29 as always); the session-J log entry. This file scopes
the session and supersedes on disagreement; fix disagreements in place. Counts measured at
`<J-exit>`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (session J's log entry, §0's rows, `perf_baseline.md`'s
   J rows, this brief).
2. Open per **S27**: session J ended accepted; tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: **466 / 460 / 20**,
   Miri **326** (~780s — two probes now), census **60**, goldens **57**, `raw_ptr`
   **4550**. Recount.
3. **Run the S2 null at open** — this session issues perf verdicts.

## 1. Face 1 — the day-two confirmation, before any new code

**This is the first item and it is not optional.** T5.J3 measured `pRefIndex` at **+0.03%
decode median, 7 pairs**, and the decision that rests on it is whether the remaining ten
flip on D-perf-4's ordinary terms. S2b as extended exists because two seven-pair readings
of one span disagreed by a factor of two overnight.

```bash
FFMPEG=/opt/homebrew/bin/ffmpeg python3 rust/tools/perfpair.py run j_face1 j_j3 --pairs 7
```

Both binaries are already stashed (`.perfpair/j_face1` = `d8becdf0`, `.perfpair/j_j3` =
`e207fdd9`). Record both readings side by side in the log, as session I did for the flip's
span, whichever way it comes out.

- **Confirms (median inside the null band):** the mechanism in `perf_baseline.md`'s J
  section holds — one bounds check per macroblock *record*, in-record indices const-bounded
  — and §2 proceeds at ordinary speed.
- **Does not confirm:** stop at that finding, put the two readings in front of Eugene with
  the null band beside them, and do not flip further on an unconfirmed number.

## 2. Face 2 — the remaining ten, hottest first

Order and per-family facts are in [`phase5.md`](phase5.md) §2's table: `pMv` (3.1%, 18
sites), `pMbType` (2.0%, 8), `pNzc` (1.7%, 30), `pSliceIdc` (1.6%, 19), `pChromaQp`
(1.3%, 29), `pMvd` (1.2%, 36), `pDirect` (0.7%, 12), `pMbCorrectlyDecodedFlag` (0.5%, 18),
`pScaledTCoeff` (0.4%, 11), `pIntraPredMode` (0.0%, 12). Re-derive before acting (S24) —
the heat moves as families flip.

Per family commit, unchanged from session J's shape:

- Flip as `MbArray<[T; K]>`; in-record indexing is const-bounded — **no window hoisting**
  (S8 #4). Hoist nothing that is not already the C++'s shape.
- **Grep the family's consumers for a wider-than-element access before flipping** (F35).
  `SetRectBlock` and `CopyRectBlock4Cols` are already unaligned-spelled; that is a fact
  about `mv_pred.rs`, not about the codebase. The alignment each family lands on is in
  §2's table.
- S28 full-reach Miri test for any raw bridge; S29 spellings; S21 if any construction path
  changes; census re-keys in the same commit.
- **Both probes green.** Budget a Miri **round trip** per family, not a verdict: the grid
  probe opened B-direct, neighbour MV prediction and the 8x8 transform, none of which had
  ever been under the checker, and it found F35 on run one.
- **Measurement: 7 pairs, both benches**, ledger row per D-perf-4. A family projecting
  large on one stream still lands if cumulative holds; log the flag.
- **Stop-line: cumulative CB ≈ +23%** (day-two-confirmed, not single-reading) — stop at the
  family boundary, write the hand-off, escalate to Eugene with the table. Currently
  ≈ +19.2…+20.1%.

`pMv` and `pRefIndex` share their hot functions but never a signature, so `pMv` is an
ordinary single-family commit; T5.J3's diff is the template.

## 3. Face 3 — behind the flips, as families land

The remaining `parse_mb_syn_*` cache-fill re-points for flipped families (mechanical, only
for families already flipped) — **not started, ~28 signatures over three files per §2**.
`pBitStringAux` and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor die in the family that
carries them, if they haven't already — re-grep (S24).

## 4. Gates

Full battery per family or tight cluster (batch before the long Miri step — it is ~13
minutes now, both probes included); goldens frozen at **57**; sweeps 341/341 both profiles;
F3 per S14 (append at adjudication, reconcile at close — the running total is
thirty-three/eleven/twelve); ratchet per S16, per-file deltas; census green. **Do not edit
the working tree while a battery is running.**

## 5. Close

Log entry: the day-two confirmation with both readings, per-family ledger rows with 7-pair
tables, cumulative CB after each family, S25/S28/S21 notes per commit. Update phase5.md §2
marks and §0's Next row. Hand-off: remaining families (if the stop-line fired: the
escalation table), else the rest of 5.2's re-points, then 5.1's second half or 5.3.

## 6. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). No 5.3 pulls (F22's map still waits —
`mv_pred.rs`'s punning is *not* re-opened beyond what a flip's alignment forces). No
`PicPool`/identity (deferred). No golden movement. No pool/threading (F12/P10). No
re-litigating D-perf-5 or the probe's construction — both records are closed.
