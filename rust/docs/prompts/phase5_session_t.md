# Phase 5, session T — the parity closure

**D-par-1 governs** (plan §7.4: test parity first, safety refactoring later) —
this session finishes the parity work session S opened. **W8's perf adjudication
is postponed out of this session** (Eugene, 2026-08-14) and rides with session
U's exit, where D-gate-1 always pointed. **D-gate-1**, **S31**, forcing rules v2, and **S33** (outcomes
are measurements, never predictions) in force. Counts at `bfe3c80f`; re-grep at
each face's open (S24).

**No W6/W7 work** — deferred under D-par-1 until session U. A safety conversion
discovered necessary for a parity fix is logged and taken to the steward, not
slipped in.

## 0. Start

1. Commit the inherited doc tail (D-par-1's §7.4 record, S32/S33 in §7.6, the
   mapping, this brief).
2. Open per **S27**: S closed pass-modulo-F3 → cheap subset if tools/toolchain
   unchanged. Last recorded: 482/476/20, Miri **338/0** (4 probes), census 59,
   sweeps 341/341 release, decoder `raw_ptr` 980. Recount.
3. S32 applies to any probe you add: probe count is the budget knob. S33 applies
   to every number you write.

## 1. Face 0 — F46: the return codes

Per F46's record (its §Scoped table is the map — three `(port, cpp)` pairs, not
"45% of rows"):

1. **Falsify or confirm the written hypothesis first, on the 41-byte
   reproduction**: instrument `iTotalNumMbRec` on both sides
   (`tools/ecref` runs the C++; a temporary eprintln on the port). The
   hypothesis: the C++ decodes ≥1 macroblock from the truncated slice, enters
   the `iTotalNumMbRec != 0`-gated concealment bracket, and sets
   `dsDataErrorConcealed`; the port decodes none and skips it. The fix is
   *which path runs*, not a missing `|=` — the three `dsDataErrorConcealed`
   sites already read equivalently.
2. Fix the 71-row pair first (93%), re-run `narrow_16x16`'s codes; then the
   `0x0→0x4` (3) and `0x0→0x10` (2) pairs — each is its own cause per the
   record's scoping.
3. **Corpus-wide re-run**: `ecref` codes vs port on all 11 streams. The
   malformed table's `ret_rle` column regenerates from the C++'s answer where
   the fix lands (F43's precedent, logged per row-file); **the output columns
   must not move** — that is the guard that the fix touched codes only.
4. Done-test: every truncation row of the corpus agrees with the C++ on codes,
   or the residue is enumerated with causes in the log.

## 2. Face 1 — `CABA2_SVA_B`'s 12 rows: confirm or refute

The last output divergence. Same frame counts, differing hashes — the signature
of the POC tie-break this suite already documents (log, session S §divergence).
**Confirm it**: trace POC values on the 12 rows on both sides; if the divergence
is exactly the documented tie-break, record the rows expected-divergent with the
documentation pointer and the traced values. **If it is anything else**: new
finding, scoped per F46's example (reduce to distinct causes with a minimal
reproduction), not fixed this face unless the cause is one line.

## 3. Face 2 — the corpus statement

Re-run the full malformed corpus + conformance set after faces 0–1; write the
one-paragraph statement into the log and §0: which streams are exact on every
row (target: 11/11 on output; codes per face 0's done-test), and the exact
residue if not. This is the parity milestone D-par-1 asked for — state it
plainly, per S33 (measured, not hoped).

## 4. Gates

Per commit (~3 min): build both profiles + tests + ratchet + census. Face 0
runs its affected tables at the face. Full battery once at close; F3 per S14
(running total per the log). **Do not edit the working tree while the battery
runs.**

## 5. Close

Log entry from breadcrumbs (≤ 30 lines), §0's rows, phase5.md marks. Hand-off →
**U = W6 + W7 + W8 whole** — the postponed adjudication (session N's stashed
binaries, the niche verdict, the D-gate-1 window as spans, the stop-line
verdict, the ledger) and the phase close (phase6.md per S19, briefs historical,
the checklist closed).

## 6. Non-goals

No W6/W7 conversion work (D-par-1 — deferred to U). No encoder sites (F12/P10).
No F23/F38-class/F41/`api/` work (Phase 8's). No F36 work. No `get_unchecked`
(S8). No golden movement except face 0's `ret_rle` regenerations and face 1's
expected-divergent annotations, each logged. No new probes without S32's
arithmetic written next to the addition.
