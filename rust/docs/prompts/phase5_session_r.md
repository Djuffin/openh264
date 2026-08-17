> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session R — W3's tail through the instruments: W4–W7 included

This session executes **everything left of Phase 5 except the exit**: W3's tail
(one face), then W4, W5, W6, W7 of [`phase5.md`](phase5.md)'s closed checklist.
**D-fid-1**, **D-gate-1**, and **S31** in force. Counts at `e9d22130`; re-grep at
each face's open (S24 — directory *and unit*).

## The forcing rules, v2 (Eugene, 2026-08-14; S31 is the standing form)

Session Q delivered one face of five and closed "on context" at ~300K of a **1M
window**. That stop was an early exit; the claim was uncheckable. The rules now
check:

1. **The session ends when the checklist ends**, or at a blocker only Eugene or
   the steward can clear, named in the hand-off. **Context is not a stop reason
   before the second compaction** (S31). State lives on disk — this brief, the
   settlements in phase5.md, the log draft. After each compaction: re-read this
   brief and the open face's settlement, re-grep the counts, continue. Plan for
   compactions; do not wrap up to avoid one.
2. **Breadcrumb per face**: as each face closes, append one line to the log
   entry draft (face, commits, counts, probe result). This makes a stop at any
   point cheap and honest — and removes "saving context for the close-out" as a
   reason to stop early.
3. **A clean seam is a checkpoint, not an exit.** After every commit, the next
   face opens immediately.
4. **Close-out ≤ 30 log lines** + marks + hand-off. The commits are the record.
5. **Dropping a face requires rule 1's blocker or S31's exhaustion, written
   down.** Drop from the end only.

Unchanged and absolute: probe per seam, byte-exactness per commit,
revert-at-the-seam. Q's flip proves the regime: settled facts executed in one
pass, one green probe, `OVERALL: PASS` at full.

## 0. Start

1. Commit the inherited doc tail (S31 in plan §7.6, phase5.md's mapping, this
   brief).
2. Open per **S27**: Q closed `OVERALL: PASS` at full → cheap subset if
   tools/toolchain unchanged. Last recorded: 476/470/20, Miri 336, census 59,
   goldens 57, decoder `raw_ptr` **1237**. Recount. (The 1229 → 1237 move is
   S16's alias-vs-spelled law — resolvers spell `*const`/`*mut` where the
   `PPicture` alias was invisible. Read probes for safety; this number falls
   when W6/W7 delete spelled types.)
3. **Probe runs budgeted to fire**; a conviction is fixed by ordering (S29).

## 1. Face 0 — `pDqLayersList` and `pCurDqLayer` are one face (Q's discovery)

Owning the list makes the cache a stored derivation through the Box, and
`decoder_core.rs:3644` re-derives from the list **inside the AU loop**, which
would invalidate it — so the cache dies first, then the list owns. Two seams:

1. **The cache → a parameter, under the still-raw list** (pure motion,
   byte-identical): delete the `pCurDqLayer` field; the AU loop derives a local
   from the raw list at each iteration top — the `:3644` mid-loop re-derive
   becomes that iteration's own derivation; the **81 mid-tree ctx-field reads**
   (re-grep; Q may have shifted them) become param sites on the already-threaded
   parameter. Probe run.
2. **The list → `Option<Box<DqLayerState>>`**: the loop-top derivation becomes
   `&mut` through the Box per iteration (S29 field-precise spelling). Done when
   `WelsMallocz|WelsFree` in `src/decoder/` reads **0** and the cascade
   functions are deleted — **W3 closes**. Probe run.

## 2. Face 1 — W4: colocated + 5.3b

`GetColocatedMb` takes cur and ref (both resolved at the slice bracket); T5.N5's
`debug_assert!` becomes the type-level fact. `SetRectBlock`/`CopyRectBlock4Cols`
onto the grid; punning → byte ops (**325 `LD*/ST*` tokens** — re-grep). S6
widths; no window hoisting (S8 #4). Done when the decoder LD/ST grep reads 0.

## 3. Face 2 — W5: P4

`pSps`/`pPps` → active-paramset ids + lookup at use (**205 occurrences, 4
carriers** — re-grep). The S23 read is done (session O §3): no lookup borrow
outlives its expression — F41 was this answered wrongly; do not repeat it. Done
when `.pSps|.pPps` greps read 0.

## 4. Face 3 — W6: `decode_slice.rs` goes clean

Per P1: the EC MC paths; the NZC `*mut u8` cache family (~167 uses, re-grep);
F31's redundant memset; `cabac_rbsp_window`'s retirement; the signature leg —
**D-fid-1: functions may merge; 148 is an upper bound, not a target**. Done when
`decode_slice.rs` compiles under `#![deny(unsafe_code)]`.

## 5. Face 4 — W7: closure of the instruments

5.2's straggler sweep; **F40-class sweep crate-wide** (every
`copy_nonoverlapping`/`copy` whose count multiplies by an element size — one
grep, one read per hit); decoder `SHIM(phase2)` + `SHIM(phase5)` → **1 named
survivor** (`data_ptr`'s output-contract consumer, Phase 8's);
`#![deny(unsafe_code)]` on every decoder module, exceptions enumerated with
Phase 8 pointers. Done when SHIM greps match the survivor list exactly.

## 6. Gates — D-gate-1

Per commit (~3 min): build both profiles + tests + ratchet + census. Probe per
seam. Once at close: full battery — goldens 57 frozen, sweeps 341/341 both
profiles, benches bit-identical, Miri both probes; F3 per S14 (running total per
the log). **Do not edit the working tree while the battery runs.**

## 7. Close

Log entry (the accumulated breadcrumbs + close figures, ≤ 30 lines), phase5.md
strike-throughs and §0's rows, hand-off: whatever remains per the rules, else
**S = W8 — the exit, never compressed** (session N's stashed binaries, the niche
verdict, the stop-line, the recovery/ledger adjudication, `prompts/phase6.md`
per S19).

## 8. Non-goals

No perf measurement (D-gate-1). No encoder sites (F12/P10). No
F23/F38-class/F41/`api/` work (Phase 8's). No F36 work. No golden movement. No
`get_unchecked` (S8). No shell deletion. No re-opening settled designs — a tree
disagreement is fixed in place and logged, not re-litigated.
