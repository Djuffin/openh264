# Phase 5, session S — the last session: F43, W6's tail, W7, W8

This session **ends Phase 5**: F43's fix, W6's remaining legs, W7, and W8 — in
that order, because F43 makes W6's EC paths production-live before they convert,
and W8 adjudicates everything. **D-fid-1**, **D-gate-1**, **S31** and the forcing
rules v2 (session R's brief §rules) in force. Counts at `a158183c`; re-grep at
each face's open (S24). Both of session R's stops are **settled in phase5.md
§"Session S's two settlements"** — execute them, verify their claims by grep,
fix disagreements in place and log them.

**W8 is never compressed.** It runs last but is not droppable: if the session
cannot reach it whole, hand it off intact — that is the one sanctioned hand-off.
Squeezing the exit to finish "on time" is the one way this session can fail.

## 0. Start

1. Commit the inherited doc tail (the two settlements, the mapping, this brief).
2. Open per **S27**: R closed `OVERALL: PASS` at full → cheap subset if
   tools/toolchain unchanged. Last recorded: 476/470/20, Miri 336, census 59,
   goldens 57, decoder `raw_ptr` **980**. Recount.
3. Probes budgeted to fire; convictions fixed by ordering (S29).
4. Breadcrumb per face (forcing rule 2); close-out ≤ 30 lines.

## 1. Face 0 — F43: the stubs die and both subsystems go live

Per the settlement (phase5.md §S-settlements; F43's record has the stub list):

1. Delete the five stubs (`NeedErrorCon`, `ImplementErrorCon`,
   `MarkECFrameAsRef`, `FmoParamUpdate`, `FmoNextMb`) and the second `SFmo`
   (`decoder_core.rs:569`); imports resolve to `error_concealment.rs`/`fmo.rs`.
2. `pFmo` gains an owner (W1's recipe) and `FmoParamUpdate` runs where the C++
   runs it (paramset activation).
3. **Run the affected tests immediately** and log every mover: the two
   concealed-macro goldens are expected to move (today both hash `754db24b…` —
   concealment never ran); regenerate **against the C++ dylib**, logging each
   before/after with the dylib agreement shown. Malformed parity rows are
   expected to hold (NAL-level damage); a mover is evidence — log it, don't
   suppress it.
4. **New coverage, red-under-revert** (F21's rule; revert = re-stub
   `NeedErrorCon`): one slice-level-damage asset reaching
   `DoErrorConSliceCopy`/`MVCopy`; one FMO stream (conformance clip if the suite
   has one, else constructed). Golden both from the C++ dylib.
5. Probe run (EC paths execute under Miri for the first time in production
   shape — treat as a new container).

## 2. Face 1 — W6's tail: `decode_slice.rs` goes clean

1. **The view struct** per the settlement: one `unsafe` constructor per bracket
   top (three), in `decoder_context.rs` — `&mut` for the raw-data reader, the
   CABAC engine, and the flag/counter set; `&` for tables/config; copied
   scalars where S23 clears them (**verify constancy per field, log the
   check**); `pParam`'s scalars copied inside the constructor (F41's field
   never escapes). The 44 functions take the view.
2. The EC MC paths (live since Face 0) convert per P1.
3. `cabac_rbsp_window`'s retirement.
4. Done-test: `decode_slice.rs` compiles under `#![deny(unsafe_code)]`, the
   constructor enumerated as the exception outside the file.

## 3. Face 2 — W7: closure of the instruments

5.2's straggler sweep. **F40-class sweep** crate-wide (count operand × element
size). **F43-class sweep**: every fn name defined in ≥2 modules where a caller's
own module hosts one locally — enumerate, check resolution. Decoder `SHIM(` →
**1 named survivor** (`data_ptr`'s output-contract consumer, Phase 8's).
`#![deny(unsafe_code)]` per decoder module, exceptions enumerated with Phase 8
pointers. Done-test: SHIM greps match the survivor list exactly; every module
deny-clean or on the exception list.

## 4. Face 3 — W8: the exit, whole

1. **Full battery at `exit` level** — goldens (57 + Face 0's additions), sweeps
   341/341 both profiles, benches bit-identical, all four Miri targets (S22's
   backlog check). F3 per S14.
2. **The deferred perf adjudication** (D-gate-1's bill, all of it): session N's
   stashed binaries (`.perfpair/n_base|n_mid|n_head`), the NonZeroU32 niche
   verdict, then the D-gate-1 window measured as spans per its records —
   3-pair + day-two protocol (S2b). Stop-line verdict vs the ≈+23% line from
   cumulative ≈+21.6–21.9% at N plus the window's spans. Ledger reconciled
   toward the recovery expectation, or the escalation table written for Eugene.
3. §0 refreshed (status rows, ratchet, gates, F3 total); open findings each
   carry an owner (F3→Phase 7, F23/F38-class/F41 + `api/` inventory→Phase 8,
   F36→threading-or-deletion, F43 closed here, survivors per file).
4. `prompts/phase6.md` written per S19 (the encoder brief inherits Phase 5's
   playbook: the ordering rule, cache-not-carrier, settlements-in-writing,
   probe budget, S31); briefs stamped historical; the phase5.md checklist
   marked closed.

## 5. Gates — D-gate-1 until W8, then the exit level

Per commit (~3 min): build both profiles + tests + ratchet + census. Probe per
seam. Face 0 additionally runs its affected tests at the face, not at close.
**Do not edit the working tree while the battery runs.**

## 6. Close

The exit **is** the close: log entry from the breadcrumbs (≤ 30 lines), the
adjudication tables, phase5.md closed, phase6.md in place, hand-off only if W8
was handed off whole — then it names exactly what remains and why.

## 7. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work
(Phase 8's). No F36 work (threading's; F43 is not F36 — EC/FMO are
single-threaded subsystems). No `get_unchecked` (S8). No shell deletion. No
golden movement **except Face 0's logged regenerations**. No re-opening settled
designs.
