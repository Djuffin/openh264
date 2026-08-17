> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session V — the closing session

**Eugene, 2026-08-15: "make the next session finish the work of this phase."**
This session closes Phase 5: the owed day two, F50, W6 whole per the
decomposition (phase5.md §"W6 decomposed"), W7's remainder, and the close.
**D-par-1**, **D-fid-1**, **S31**–**S33**, forcing rules v2 in force. Counts at
`ba4da8d8`; re-grep at each face's open (S24).

**U's rule holds: no code lands before face 0 closes** — a code commit moves the
window's endpoint and invalidates the day-two reading it exists to confirm.

## 0. Start

1. Commit the inherited doc tail (the decomposition, the mapping, this brief).
2. Open per **S27**: U closed `OVERALL: PASS` at exit → cheap subset if
   tools/toolchain unchanged. Last recorded: corpus output 2690/17, codes
   2683/24, conformance 60/60, decoder `raw_ptr` 980, `SHIM(` 52, deny-clean
   modules 3/22. Recount.
3. S32 on probes; S33 on every number; breadcrumb per face.

## 1. Face 0 — the day two, then the stop-line disposition

Exactly what `perf_baseline.md` §"Day two — owed, and named" specifies — the
binaries are stashed, **no build needed**:

1. **The window at 7 pairs** (`n_head` → `u_head`) with a **7-pair null taken
   the same day**. The stop-line verdict rests on this and nothing else.
2. **The niche at 3 pairs** (`n_head` → `o_niche`), closed as resolved or
   inside the floor.
3. **Not the bisect halves** (session K's law — a second unresolvable answer).

Then the disposition, written per S33 from what day two says:

- **Breach stands** → record the table's **option 6** — recovery deferred to
  Phase 9's perf pass, D-perf-4's own disposition, Phase 2's precedent — as
  **D-perf-6** in plan §7.4 (Eugene's default under his closing order; his veto
  window was the steward's report). Cumulative and the new line noted in
  `perf_baseline.md` §Phase 5 exit.
- **Breach dissolves** (7-pair window median puts cumulative ≤ the line) →
  record that, and the stop-line stands unbreached.

Face 0 closes when the verdict and disposition are written. Code may land from
here on.

## 2. Face 1 — F50

24 rows, `ParseSps` rejecting what the C++ accepts; the reproduction is in the
record. Fix, re-referee the 24 through `compare_all.sh`, log the delta.
Done-test: corpus codes read 2707/0 minus only documented-deliberate rows.

## 3. Face 2 — W6 whole, per the decomposition

The four steps of phase5.md §"W6 decomposed", in order; each is a proven recipe:

1. **The view struct** (§S-settlements): one `unsafe` constructor per bracket
   top, in `decoder_context.rs`; S23-check per copied scalar; `pParam`'s
   scalars copied inside. Kills the 44 `*mut SWelsDecoderContext`.
2. **The reference-flip**: bracket tops hand out `&mut`/`&`; the 87
   struct-pointer params (`DqLayerState` 51, `SWelsNeighAvail` 13, `SSlice`
   10, `SNalUnit` 8, `BsCursor` 5) become references — re-spelling,
   byte-identical, probe per seam.
3. **The plane/block-slice conversion**, one family at a time (~38 scalar
   types), retiring the **42 `SHIM(phase2)` adapters** that exist to bridge
   these exact callers — same commits. S6 widths; F35's alignment rules.
4. **`cabac_rbsp_window`** rides step 2: one more `&[u8]` from the slice
   bracket top (18 sites, 72 occurrences).

EC MC paths per P1 inside step 3's families. Functions may merge (D-fid-1).
Done-test: `decode_slice.rs` compiles under `#![deny(unsafe_code)]`, the
constructor enumerated outside the file. If the face splits despite S31, the
remainder is a **named list of families** in the hand-off — never "the rest".

## 4. Face 3 — W7's remainder

5.2's straggler sweep; `SHIM(` **→ 1 named survivor** (`data_ptr`'s
output-contract consumer, Phase 8's — the 42 retired in face 2, the residue
enumerated); `#![deny(unsafe_code)]` on the remaining 19 decoder modules,
exceptions enumerated with Phase 8 pointers. Done-test: SHIM greps match the
survivor list; every decoder module deny-clean or on the exception list.

## 5. Face 4 — the close

1. **V's own window extension, measured**: `u_head` → V's last code commit, 3
   pairs + null (S2b). Its **day two is the one sanctioned hand-off** out of
   this session — named exactly if owed.
2. Full battery at `exit` level; F3 per S14.
3. **`prompts/phase6.md` per S19** — the encoder brief inherits the playbook:
   the ordering rule, cache-not-carrier, settlements-in-writing, the bracket
   maneuver (three applications now), the probe budget and S32, S31, S33,
   D-par-1's standing referee (`ecref`, `compare_all.sh`, the corpus
   discipline).
4. Briefs stamped historical; this checklist **closed**; §0 refreshed; open
   findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the `api/`
   inventory→Phase 8, F36→threading-or-deletion, the `CABA2_SVA_B` annotation
   standing).

## 6. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam (S32 beside any probe change). Full battery once at close. **Do
not edit the working tree while the battery runs.**

## 7. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work
(Phase 8's). No F36 work. No `get_unchecked` (S8). No golden movement except
F50's re-refereed rows (logged). No perf work beyond faces 0 and 4's
measurements — recovery, if owed, is Phase 9's by D-perf-6. No re-opening
settled designs.
