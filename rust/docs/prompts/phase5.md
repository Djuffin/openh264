# Phase 5 — decoder structural rewrite: the remainder, as a closed checklist

Re-planned 2026-08-13 under **D-fid-1** (structural fidelity to the C++ retired;
output equivalence — goldens, sweeps, conformance — remains the correctness
definition) and **D-gate-1** (sprint gating). Rules: plan §7.6; per-session scope
is the S20 closure; S24 at every face open. History: sessions A–O's record is the
log and git — this file carries only what remains.

**The anti-circles contract.** This checklist is **closed**: work enters it only
via an F-finding or Eugene. Every session's commits map to W-items in its log
entry. The progress metric is monotone and grep-able — **decoder `raw_ptr`
occurrences 1267** (1283 at the re-plan), → ~0 at exit (excluding the named
survivor and prose); a session that closes no W-item and moves no metric is a stall
and says so — **and a W-item can close while the metric stands still**, which is
what W2a did (85 sites, `raw_ptr` +0): two pointer fields became handles and four
accessor signatures were born, and the instrument counts pointer *types written*
(S16). Sizes below were re-measured at `eef8a90b` on 2026-08-13; re-grep at
the face. **And a size carries the directory it was taken in** (session P §0): the
re-plan's `pRefPic` and `pRefList` figures were taken at *different* scopes and said
so nowhere — together they put **228 sites of `src/encoder/`** inside W2, which
F12/P10 puts outside this phase entirely.

## The blocker class, named once

*A raw alias into a container blocks that container's ownership.* Everything
below W1 was sequenced by this one fact (T5.N1, T5.O7). The ordering rule:
**aliases become ids/indices before their container owns.** W2 is the key; it
unblocks W3–W5 and half of W6.

**Session P used the rule forwards, and it is predictive.** W1 was available
because T5.O4 had already given the NAL nodes their own allocations; W2a was
available because `PicPool` never owned the pictures its slots address; W2b's layer
copy is not available because its readers cannot reach the container at all. Three
calls made from the rule, no probe needed to make any of them, and all three probe
runs green on the first attempt. **Ask the rule before scheduling a conversion, not
after Miri does.**

## The checklist

| # | item | size (measured) | done when | unblocks |
|---|---|---|---|---|
| ~~W1~~ | ~~`pAccessUnitList` → `Option<Box<SAccessUnit>>`~~ — **DONE, T5.P1** (`a3b68334`) | 54 sites | — | W3's first cascade entry, taken |
| ~~W2a~~ | ~~the context's two pool aliases: `pDec`/`pECRefPic` → `PicId`~~ — **DONE, T5.P2** (`eef8a90b`) | 85 sites | — | `dec_pic`/`ec_ref_pic` are now the **only two sites** W3 has to convert |
| W2b | **the layer's `pDec` + the reference lists** → `PicId`/indices | 159 layer `.pDec` (`decode_slice` 72, `parse_mb_syn_cavlc` 30, `deblocking` 27, `parse_mb_syn_cabac` 13, `mv_pred` 13, rest 4); `pRefList` 82 + `pShortRefList` 25 + `pLongRefList` 24; `pRefPic` 113; `ppRefPic` 24; `pPreviousDecodedPictureInDpb` 11 — all decoder-only | greps read 0 outside `PicId` plumbing; probe green per file | W3, W4, W6's signature leg, 5.5's `Drop` |
| W3 | ownership cascade: `pDqLayersList`, `pPicBuff`, `pTempDec` owned; `Pool<Box<SPicture>>` + `mut_and_rest`; `Drop` teardown; shell extended per field (never deleted); **`SPicture` finishes owning itself** — `pMv`/`pRefIndex` (`picture.rs:261`, the grid's picture-side twins) are still raw where the planes have been owned since T5.C3 | 3 containers + cascade fns + 2 picture arrays | zero `WelsMallocz`/`WelsFree` in `src/decoder/`; cascade functions deleted; probe green per container | 5.5 closes |
| W4 | colocated + 5.3b: `GetColocatedMb` on `cur_and_ref`; `SetRectBlock`/`CopyRectBlock4Cols` on the grid; punning → byte ops | 325 `LD*/ST*` tokens remaining | decoder `LD32\|ST32\|LD16\|ST16\|LD64\|ST64` grep reads 0 | `mv_pred.rs` deny-ready |
| W5 | P4: `pSps`/`pPps` → active-paramset ids + lookup | 205 field occurrences (131 + 74), 4 carriers | `.pSps\|.pPps` greps read 0; no lookup borrow outlives its expression (F41's mistake, not repeated) | context sheds 2 raw fields |
| W6 | 5.6: `decode_slice.rs` per P1 — EC MC paths, the NZC `*mut u8` cache family (~167 uses, re-grep), F31's memset, the signature leg (**D-fid-1: functions may merge — the 148-function count is an upper bound, not a target**), `cabac_rbsp_window` retirement | the phase's largest file | `decode_slice.rs` compiles under `#![deny(unsafe_code)]` | W7 |
| W7 | closure of the instruments: 5.2's straggler sweep; **F40-class sweep crate-wide** (element-vs-byte in copies); decoder `SHIM(phase2)` 48 + `SHIM(phase5)` 3 → **1 named survivor** (`data_ptr`'s output-contract consumer, Phase 8's); `deny(unsafe_code)` on every decoder module, exceptions enumerated | sweeps + deletions | SHIM greps match the survivor list exactly; every decoder module deny-clean or on the exception list with its Phase 8 pointer | the exit |
| W8 | **the exit — never compressed**: all D-gate-1-deferred measurement (session N's stashed binaries, the niche verdict, the ≈+23% stop-line, the recovery/ledger adjudication vs ≈+21.6–21.9% cumulative CB), 3-pair+day-two protocol, §0 refresh, `prompts/phase6.md` (S19), briefs stamped historical | one session | `OVERALL: PASS` at `exit` level; ledger reconciled or escalated with the table; phase6.md exists | Phase 6 |

## Session mapping

**P** = W1 + W2a — **spent** (`a3b68334`, `eef8a90b`). **P′** = W2b → W5
([`phase5_session_p2.md`](phase5_session_p2.md)). **P″** = W6 + W7. **Q** = W8. A probe run per container/file converted (T5.O3's lesson, and
session P's three were green first time), no perf measurement before W8 (D-gate-1).

The re-plan's "two sessions if P reaches W5" **did not hold, and the reason is
W2b's**: `pDec` is two carriers, not one, and the second is session O's
148-function signature leg wearing a different hat. Count is **three work sessions
plus the exit**.

**W2b's design question — settled by reading the tree (steward, at `c8ebc20f`).**
The choice was false: the layer's `pDec` is a **cache of `dec_pic(pCtx)`** — one
stamp site in the whole decoder (`decoder_core.rs:3704` → `InitDqLayerInfo`
`:3448`), and its null arm is **parse-only mode**, not threading (`GetThreadCount`
≡ 0, `decoder_core.rs:705`). So the field dies rather than converts: readers that
hold `pCtx` derive (the two plane consumers, `decode_slice.rs:2063`/`:2115`,
already take it as their first parameter); the few layer-only leaves
(`PredPSkipMvFromNeighbor` `mv_pred.rs:439`'s shape) take `PPicture` from their
ctx-holding callers or merge into them (D-fid-1); identity compares become `PicId`
equality. **It is not the 148-function leg arriving from the other side — it is a
handful of leaves.** `DqLayerState::pRef` is **dead** — zero readers, zero
writers — and deletes first. One S23 check gates the mechanical pass (the
`pDec = None` mid-AU resets); the P′ brief carries it with sites. The non-null
arm's real target — `SPicture.pMv`/`pRefIndex`, still raw allocations — is W3's,
named in its row.

## Phase exit conditions (the definition of done)

1. Decoder `raw_ptr` ≈ 0: every occurrence in `src/decoder/` is on the survivor
   list (the output-contract consumer) or is prose.
2. Every decoder module `#![deny(unsafe_code)]`, exceptions enumerated with
   Phase 8 pointers.
3. `SHIM(` decoder share = the survivor list exactly; census green; F40-class
   sweep run with its results recorded.
4. Gates: full battery `OVERALL: PASS` at `exit` level — goldens 57 (none ever
   moved), sweeps 341/341 both profiles, benches bit-identical, Miri both probes,
   the widened `exit`-level Miri targets (S22: the backlog check).
5. The perf adjudication done and written: stop-line verdict, niche verdict,
   ledger reconciled toward the recovery expectation — or the escalation table in
   front of Eugene.
6. `prompts/phase6.md` written; §0 refreshed; open findings each carry an owner
   (expected open set: F3→Phase 7, F23/F38-class/F41 + the `api/` inventory →
   Phase 8, F36→decoder-threading-or-deletion, F4/F6–F14 survivors per file).

## Standing constraints (unchanged by D-fid-1)

Output equivalence per commit (goldens/sweeps/benches — the C++-as-reference).
No golden movement. No `get_unchecked` (S8). No window hoisting (S8 #4). No
pool/threading edits (F12/P10 — encoder side). F3 per S14. The shell is the
constructor (session O's correction).
