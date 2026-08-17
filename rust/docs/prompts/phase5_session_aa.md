# Phase 5, session AA — the deny-clean sweep, the `common/` boundary, and the close

Exit conditions 1–3 unmet. Condition 5 met and closed (D-perf-6).
**D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**, forcing rules v2 and
**the finish rule** (`phase5_session_z.md` §"The finish rule") all in force.
Counts at session Z's close; re-grep at each face's open (S24).

**Read first**: `phase5.md` §"The context, closed (session Z)". The context is a
`&mut` everywhere — the crate-wide grep for a raw context parameter reads **0** —
and every `decoder_context` accessor returns a borrow and takes the *field* it
reaches. That is the ground the remaining work stands on, and none of it is
blocked any more.

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27** (Z's `exit` battery result is in its log entry §"Gates";
   `rust/tools/` and the toolchain are unchanged): build both profiles, tests,
   ratchet, census. Recount decoder `raw_ptr`, `unsafe fn`, deny-clean, `SHIM(`.
3. Probe per seam (S32 beside any probe change); S33 on every number; breadcrumb
   per face.

## 1. Face 0 — the raw families that are left, by module

Decoder `unsafe fn` **230** and `raw_ptr` **310** at Z's close, and the two are
now roughly proportional again: what is left is genuinely raw, not vestigial.
Per module (`unsafe fn` / raw-pointer occurrences), re-grep at the face:
`decoder_core` 59/60, `decode_slice` 47/32, `deblocking` 21/20,
`manage_dec_ref` 18/15, `mv_pred` 18/4, `parse_mb_syn_cabac` 16/10,
`parse_mb_syn_cavlc` 14/18, `nalu` 13/29, `error_concealment` 11/23,
`decoder_context` 5/43, `pic_queue` 1/29.

The families, in the order the phase's own rule (*aliases become ids before
their container owns*) puts them:

1. **`PDqLayer` — the layer bracket.** The largest single family left and the one
   the context flip named at its own seam: `DecodeCurrentAccessUnit` and
   `CheckAndFinishLastPic` still round-trip `cur_dq_layer`'s borrow back to a
   pointer because the loop below passes `dq_cur` *beside* `pCtx` to a dozen
   functions. That is `slice_split`'s maneuver one container over — one function
   that hands back the layer and the rest of the context from one borrow.
2. **`PNalUnit`/`PSliceHeader`** — the parse tree's two remaining pointers. The
   NAL nodes are their own allocations (T5.O4), so this is a spelling change plus
   the access-unit walk; the slice header is a field of the NAL.
3. **`decoder_context` 5/43 and `pic_queue` 1/29** are mostly *type aliases and
   prose* (S16's floor) — count them before scheduling anything.

`mv_pred` (18/4) and `parse_mb_syn_cabac` (16/10) are the cheapest deny-clean
candidates; each needs its callees safe first, which is what makes the order
bottom-up (§"The order the decomposition needs" in `phase5.md`).

## 2. Face 1 — the `common/` boundary, **decided** (steward, at `3e2f43e6`)

Still not started, and now nothing else blocks it: safe slice-taking entry points
beside the raw ones, in `common/`; decoder callers move to the safe forms;
`deblocking.rs`, `decode_mb_aux.rs` and `error_concealment.rs` go deny-clean with
no encoder edit (F12/P10 holds); the raw forms carry a one-line pointer and are
deleted in Phase 6; per-kernel enumerated exception where a wrapper cannot
express the contract. Re-derive the surface at the face (S24).

## 3. Face 2 — W7's closure, then the phase close

1. 5.2's straggler sweep; `SHIM(` → **the survivor list exactly** (1 named:
   `data_ptr`'s output-contract consumer, Phase 8's — of the 6 today, 2 are prose
   and 2 are `decoder_core`'s `expand_picture` bridges); census green.
2. Full battery at `exit` level; F3 per S14.
3. AA's own span per S2b — **no other perf work** (D-perf-6).
4. **`prompts/phase6.md` per S19** — only if the phase actually exits.
5. Briefs stamped historical; phase5.md's checklist closed; §0 refreshed; open
   findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the `api/`
   inventory→Phase 8, F36→threading-or-deletion, **F52's six encoder-side
   shadowing-stub candidates→Phase 6**, **F54's rule folded into S21**, the
   `CABA2_SVA_B` annotation standing).

## 4. Two rules session Z added, and they are cheap to lose

- **A definition that names a raw pointer in its signature keeps `unsafe`,
  whatever the body does.** Face 2's strip-and-build finds the bodies that need
  the keyword; it cannot see the nine functions whose *signature* carries the
  contract, six of which already wrap their whole body in an explicit `unsafe`
  block. Re-run the sweep the same way — over every module at once, because the
  answer is order-dependent — and then read the signatures.
- **When a field's type changes, ask what its all-zero bit pattern now means**
  (F54), not whether it owns anything. `Option<T>` over a type with a `bool` or a
  small enum is *valid* at zero and reads back as `Some(default)`; a raw
  pointer's zero was `None`. The two look identical in a diff, and the zeroed
  shell is the whole decoder context.

## 5. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree while
the battery runs.**

## 6. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work (Phase
8's). No F36 work — `sTmpRefPic`'s arm stays. No `get_unchecked` (S8). No golden
movement. No perf work beyond AA's own span (D-perf-6). No re-opening settled
designs.
