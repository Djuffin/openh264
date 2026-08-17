> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session L — T5.L: the cumulative number, then the remaining seven

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.4 (D-perf-4; D-perf-5's
"probe first, then flip" is executing) and §7.6 — **S2b now carries session K's
pair-count clause and its "measure something bigger" conclusion; read it before running
anything**; S8's fourth negative result stands: **no window hoisting**;
S13/S14/S16/S20/S21/S24/S25/S28/S29 as always; the session-K log entry. This file scopes
the session and supersedes on disagreement; fix disagreements in place. Counts measured
at `135ebcb7`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (session K's log entry, `perf_baseline.md`'s K section,
   F3 measurement 34, F36, phase5.md's §0/§2/§8 marks, plan §0's rows and S2b/S14, this
   brief).
2. Open per **S27**: session K ended accepted; tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: **468 / 462 / 20**,
   Miri **328** (~910s), census **60**, goldens **57**, `raw_ptr` **4540**. Recount.
3. **Run the S2 null at open, at 7 pairs** — not 3. This session issues perf verdicts and
   S2b's new clause is that the null matches the verdict's pair count.

## 1. Face 1 — the cumulative span, which is the number the stop-line gates on

**First item.** Session K established that a per-family cost of ≈0.3% CB is below this
harness's resolution: three 7-pair readings of T5.J3's span disagree in sign, and the
session nulls disagree by as much. The plan's arithmetic — cumulative CB ≈ +19.2…+20.1%
against a ≈+23% stop-line — is a **sum of fifteen numbers each below the floor**, and
session I already found that sum high by a factor of two.

```bash
FFMPEG=/opt/homebrew/bin/ffmpeg python3 rust/tools/perfpair.py build l_head
FFMPEG=/opt/homebrew/bin/ffmpeg python3 rust/tools/perfpair.py run flip_head l_head --pairs 7
```

`flip_head` (`1438d762`) is session H's post-flip binary and is already stashed;
`seat_head` (`3c4c6f4e`) is the pre-flip one if the wider span is wanted. A ~20% effect
is far above the floor, so this reading means something the per-family ones cannot.

- **Record it as the cumulative position** in `perf_baseline.md`, replacing the summed
  estimate rather than sitting beside it, and say which span it measures.
- **If it lands near +23%**: stop at the family boundary, write the hand-off, escalate to
  Eugene with the table. If it lands materially below the summed estimate — which is what
  session I's factor-of-two suggests — say so plainly; the headroom question changes.

## 2. Face 2 — the remaining seven, hottest first

Order and per-family facts are in [`phase5.md`](phase5.md) §2's table: `pNzc` (1.9%, 32
sites), `pChromaQp` (1.9%, 29), `pMvd` (1.4%, 40), `pDirect` (0.8%, 16),
`pMbCorrectlyDecodedFlag` (0.6%, 18), `pScaledTCoeff` (0.3%, 11), `pIntraPredMode` (0.0%,
16). **Re-derive the heat first (S24): it moved between sessions J and K** — `pSliceIdc`
went 1.6% → 2.5% and overtook `pNzc`, because heat is attributed to the functions holding
a family's accesses and those functions do not change when a different family leaves.

Per family commit, unchanged from sessions J and K:

- Flip as `MbArray<[T; K]>`; in-record indexing is const-bounded — **no window hoisting**
  (S8 #4). Hoist nothing that is not already the C++'s shape.
- **Grep the family's consumers for a wider-than-element access before flipping** (F35).
  All three of session K's families came back clean; that is a fact about them.
- **Grep the family's writers against the C++'s, not just its readers.** F36 was found
  this way and nothing else would have found it: the C++ writes `pSliceIdc` in two
  macroblock loops and the port writes it in one.
- S28 full-reach Miri test for any raw bridge — and note that `pSliceIdc` needed none,
  because a base pointer whose only use is neighbour comparison becomes a shared borrow.
- S29 spellings; S21 if any construction path changes; census re-keys in the same commit.
- **Both probes green.** Budget a Miri **round trip** per family, not a verdict.
- **Measurement**: per D-perf-4, but read S2b first. After Face 1 the honest unit is the
  **cluster or the cumulative span**, not a ≈0.05% per-family half; session K measured
  T5.K2+K3 as one span for that reason and said so in the ledger.
- **Stop-line: cumulative CB ≈ +23%**, now measured directly rather than summed.

**`pMvd` carries a shape none of the flipped families had**: `mv_pred.rs` guards fifteen
of its accesses with `if !(*pCurDqLayer).pMvd[LIST_x].is_null()`. Check each against the
C++ before deleting it — `pSliceIdc`'s two turned out to be port-added, but that is a
fact about `pSliceIdc`.

## 3. Face 3 — behind the flips, as families land

The remaining `parse_mb_syn_*` cache-fill re-points for flipped families (mechanical,
only for families already flipped) — **not started, ~28 signatures over three files per
§2**. `pBitStringAux` and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor die in the
family that carries them, if they haven't already — re-grep (S24).

## 4. Gates

Full battery per family or tight cluster (batch before the long Miri step — it is ~15
minutes now, both probes included); goldens frozen at **57**; sweeps 341/341 both
profiles; F3 per S14 — **its signature was corrected at session K** (`n=600` is a rate
artifact, not a condition; the sustained-load rate is ≈1/307), and two hits in one
battery means step 2 alternates at **the profile the hits occurred in**; ratchet per S16,
per-file deltas; census green. **Do not edit the working tree while a battery is
running.** The F3 step-0 hash shortcut needs both binaries built in the **same
directory** — a `git worktree` control differs by path alone and proves nothing.

## 5. Close

Log entry: the cumulative reading against the summed estimate, per-family ledger rows or
the cluster's, S25/S28/S21 notes per commit. Update phase5.md §2 marks and §0's rows.
Hand-off: remaining families, then the rest of 5.2's re-points, then 5.1's second half or
5.3.

## 6. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). No 5.3 pulls (F22's map still waits).
**No F36 fix** — it is decoder-threading's and fixing it moves bytes on a path nothing
here measures. No `PicPool`/identity (deferred). No golden movement. No pool/threading
(F12/P10). No re-litigating D-perf-5 or the probe's construction — both records are
closed.
