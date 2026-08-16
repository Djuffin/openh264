# Phase 5, session X — the endgame

This session takes exit conditions 1–3 to met and **closes Phase 5**. The order
is V's forced order (callees first, view struct last) with W's working unit (the
pointer family, converted everywhere at once). **D-par-1**, **D-fid-1**,
**D-perf-6**, **S31**–**S33**, forcing rules v2 in force. Counts at `361592a7`;
re-grep at each face's open (S24). The one design item W left is **settled** —
phase5.md §"The residual-chain blocker, settled by reading" — execute it, verify
its reach-claims by grep, fix disagreements in place and log them.

## 0. Start

1. Commit the inherited doc tail (the settlement, the mapping, this brief).
2. Open per **S27**: W closed `OVERALL: PASS` 13/0/1 → cheap subset if
   tools/toolchain unchanged. Last recorded: decoder `raw_ptr` **765**,
   deny-clean **9/22**, `SHIM(` 52, corpus 2690/17 + 2707/0, conformance 60/60,
   Miri 338/0. Recount.
3. Probe per seam (S32 beside any probe change); S33 on every number;
   breadcrumb per face. **A clean boundary is a checkpoint, not an exit.**

## 1. Face 0 — the residual chain, per the settlement

1. `ParseCbfInfoCabac` takes what it reaches: `iMbXyIndex`/`iMbWidth` as
   values, `&mut` the `cbf_dc` family, the mb_type view read-only, and the
   `pDec` dual-path operand as it verifies at the face (S24).
2. `ParseResidualBlockCabac`/`8x8` drop the layer parameter and forward the
   narrow set.
3. The callers' layer pointer becomes disjoint grid-field borrows —
   `scaled_tcoeff` block, `cbf_dc`, the qp families are all distinct fields;
   plain split borrows, no new API. **Fallback only if** a residual-path callee
   is found reaching `scaled_tcoeff` itself: `tcoeff_and_rest`, PoolRest's
   shape on the grid — log the discovery first.
4. Done when `decode_slice.rs`'s 51 layer pointers fall (the CAVLC twin
   included) and the layer family reads **0 in the file**. Probe run.

## 2. Face 1 — the family queue to zero

W's table continues, family-at-a-time, everywhere-at-once: the layer's
remaining **54** (deblocking, manage_dec_ref, decoder_core), then the modules
still carrying raw families per phase5.md's table — `mv_pred.rs`,
`deblocking.rs`, `manage_dec_ref.rs`, `nalu.rs`, `decoder_core.rs` — sizes
re-derived at each family's open. The four recurring aliasing shapes W named
are expected; recognition, not reading. Probe per seam.

## 3. Face 2 — planes + dispatch

`get_intra_predictor.rs` (44/44) with **the 42 `SHIM(phase2)` retirements** —
the callers' plane pointers convert with it; `decode_mb_aux.rs`'s 4 table shims
(a dispatch change, done as one); the EC MC paths per P1. S6 widths; F35's
alignment rules.

## 4. Face 3 — the view struct, last

S29's objection is dead decoder-wide once faces 0–2 land. The §S-settlements
design, built at the three bracket tops in `decoder_context.rs`; S23-check per
copied scalar; `pParam`'s scalars copied inside (F41 never escapes).
`decoder_context.rs` converts; `decode_slice.rs` compiles under
`#![deny(unsafe_code)]`; **every decoder module deny-clean or on the exception
list with its Phase 8 pointer.**

## 5. Face 4 — W7's closure, then the phase close

1. 5.2's straggler sweep; `SHIM(` → **the survivor list exactly** (1 named:
   `data_ptr`'s output-contract consumer, Phase 8's); census green.
2. Full battery at `exit` level; F3 per S14.
3. X's own span per S2b — **no other perf work** (D-perf-6).
4. **`prompts/phase6.md` per S19**: the encoder brief inherits the playbook —
   the ordering rule, cache-not-carrier, **take-what-you-reach**, the bracket
   maneuver (three applications), **the pointer family as the working unit**,
   settlements-in-writing, the probe budget and S32, S31, S33, and D-par-1's
   standing referee suite (`ecref`, `compare_all.sh`, the corpus discipline).
5. Briefs stamped historical; phase5.md's checklist **closed**; §0 refreshed;
   open findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the
   `api/` inventory→Phase 8, F36→threading-or-deletion, the `CABA2_SVA_B`
   annotation standing).

## 6. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree
while the battery runs.**

## 7. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work
(Phase 8's). No F36 work. No `get_unchecked` (S8). No golden movement. No perf
work beyond X's own span (D-perf-6). No re-opening settled designs — the
residual-chain settlement's reach-claims are verified by grep, and a
disagreement is fixed in place and logged, not re-litigated.
