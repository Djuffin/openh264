# Phase 5, session Z — the accessors, then the flip, then the close

Exit conditions 1–3 are unmet (decoder `raw_ptr` 383; 11 of 22 modules deny-clean;
`SHIM(` 6 against a survivor list of 1). Condition 5 is met and closed (D-perf-6).
**D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**, forcing rules v2 in force.
Counts at `dff3f78b`; re-grep at each face's open (S24).

**Read first**: `phase5.md` §"The context, measured twice (sessions X and Y)". The
slice view (`SliceCtx`, `slice_split`) is **landed and green**; the flip above it was
attempted, compiled whole, and reverted on a Miri verdict whose fix is face 0 below.

## The finish rule (Eugene, 2026-08-16: "finish the work without stopping in half
done state")

1. **This session ends with the phase closed**, or with a blocker only Eugene or
   the steward can clear, named in the hand-off. Nothing else ends it: not a
   tidy boundary (forcing rule 1), not context (S31 — compact, re-read,
   continue), and **not a revert**. A revert inside this session is a
   checkpoint: fix the blocker, re-attempt in-session. X and Y reverted and
   *handed off* because their blockers were unmeasured; Z's are measured and
   their fixes are faces 0–1 — "the flip reverted again" is no longer a valid
   end state.
2. **Face 1 does not open before face 0's done-test reads met.** The accessor
   class is the only named reason the flip has ever failed; running the flip
   early recreates session Y.
3. Half-landed is still forbidden the other way too: a face lands whole and
   gated, or is reverted **and then fixed** — never committed part-way.

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27**: Y closed `exit` at **12/1/1**, the one failure a single F3
   hit at the documented signature and acquitted by the alternation in its log entry
   — accepted, so the cheap subset applies if `rust/tools/` and the toolchain are
   unchanged. Last recorded: decoder
   `raw_ptr` **383**, deny-clean **11/22**, `SHIM(` 6, corpus 2690/17 + 2707/0,
   conformance 60/60, Miri 336/0 + 20/7/3. Recount.
3. Probe per seam (S32 beside any probe change); S33 on every number; breadcrumb
   per face.

## 1. Face 0 — the accessors return borrows

**The measured blocker.** `sps_of`, `active_sps`, `pps_of`, `active_pps`, `fmo_of`,
`active_fmo`, `subset_sps_of`, `dec_pic`, `pool_pic`, `pool_pic_mut`,
`prev_dpb_pic_mut`, `short_ref_pic*`, `long_ref_pic`, `ref_pic` and `pic_pool_ptr`
hand back raw pointers derived from the context. While the context is a pointer that
is sound; the moment it is a `&mut`, a function-entry retag on it pops the
derivation and the next read is UB — three instances in one probe run at Y, the last
of them (`FmoParamUpdate(fmo_of(pCtx, …), sps_of(pCtx, …), …)`) not fixable
site-locally at all.

**The conversion**: `Option<&SSps>` / `Option<&mut SPicture>` and their siblings, so
the rule stops being a Miri verdict on the paths a probe drives and becomes a
compile error at every site. `SliceCtx`'s methods are the shape already
(`decoder_context.rs`, `sps_of`/`pps_of`/`active_sps`/`active_pps`/`active_fmo`).

**The size, measured at Y's revert**: **60 bindings** of an accessor result into a
local — `manage_dec_ref` 35, `decoder_core` 15, `deblocking` 3, `decode_slice` 3,
`error_concealment` 2, `nalu` 2 — against ~180 call sites of the family. The
one-expression uses (`(*active_sps(pCtx)).uiChromaFormatIdc`) are the bulk and
convert mechanically; the bindings are where the reach question is real.

Done when no `decoder_context` accessor returns a raw pointer, and every consumer
either holds a borrow the compiler can see or copies the values it needs.

## 2. Face 1 — the flip, on top of it

`*mut SWelsDecoderContext` → `&mut` across the remaining 153 signatures. Y's attempt
is the recipe and it is **not** an estimate: 72 errors, five shapes, each with a fix
that compiled and passed 342 unit tests and 60 goldens.

1. **the null guards (28)** — unrepresentable; they move to the boundary that still
   holds a pointer. `api/codec_api.rs` guards all four of its dereferences already;
   `BufferingReadyPicture` and `ReorderPicturesInDisplay` need the accessors' old
   null arm kept **at the api site**.
2. **the bracket (2)** — `slice_split` (landed at Y) is the answer; the six brackets
   are `WelsDecodeSlice`, `WelsDecodeAndConstructSlice`, `WelsTargetSliceConstruction`,
   `decoder_core::ComputeColocatedTemporalScaling`, `CheckRefPicturesComplete` and
   `DoErrorConSliceMVCopy`.
3. **the access unit (8)** — `pCtx.access_unit.as_deref_mut()`, field-precise (S29 in
   safe code). Two AU *scans* collect their node pointers first; nodes are their own
   allocations (T5.O4).
4. **the reference set (13 functions)** — the selector travels instead of the borrow
   (`bTmpRefSet` + `ref_set(tmp)`). `sTmpRefPic`'s arm is kept: F36's, not this face's.
5. **the bitstream window (5 functions)** — the offset travels instead of the slice;
   the window is derived from `sRawData` per read (F16's rule). `ParseVui` and
   `DecodeSpsSvcExt` take a context they never use — delete the parameter (S18).

**Miri per seam, not only at the close** — the class Y found is invisible to the
compiler and to every byte gate.

## 3. Face 2 — `unsafe fn` that no longer needs to be

The slice view left ~150 `unsafe fn` whose bodies are already safe:
`parse_mb_syn_cabac` 34 against 10 raw-pointer occurrences, `mv_pred` 21 against 4,
`decode_slice` 54 against 32. Drop the keyword module by module, let the compiler
enumerate what actually needs it, and each module that reaches zero takes
`#![deny(unsafe_code)]` with it. Order by ratio: `mv_pred`, `parse_mb_syn_cabac`,
`parse_mb_syn_cavlc`, `manage_dec_ref`, `error_concealment`, `deblocking`,
`decode_slice`, `nalu`, `decoder_core`, `decoder_context`.

## 4. Face 3 — the `common/` boundary, **decided** (steward, at `3e2f43e6`)

Unchanged and not started: safe slice-taking entry points beside the raw ones, in
`common/`; decoder callers move to the safe forms; `deblocking.rs`,
`decode_mb_aux.rs` and `error_concealment.rs` go deny-clean with no encoder edit
(F12/P10 holds); the raw forms carry a one-line pointer and are deleted in Phase 6;
per-kernel enumerated exception where a wrapper cannot express the contract.
Re-derive the surface at the face (S24).

## 5. Face 4 — W7's closure, then the phase close

1. 5.2's straggler sweep; `SHIM(` → **the survivor list exactly** (1 named:
   `data_ptr`'s output-contract consumer, Phase 8's); census green.
2. Full battery at `exit` level; F3 per S14.
3. Z's own span per S2b — **no other perf work** (D-perf-6).
4. **`prompts/phase6.md` per S19** — only if the phase actually exits.
5. Briefs stamped historical; phase5.md's checklist closed; §0 refreshed; open
   findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the `api/`
   inventory→Phase 8, F36→threading-or-deletion, **F52's six encoder-side
   shadowing-stub candidates→Phase 6**, the `CABA2_SVA_B` annotation standing).

## 6. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census. Probe
per seam. Full battery once at close. **Do not edit the working tree while the
battery runs.**

## 7. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work (Phase
8's). No F36 work — `sTmpRefPic`'s arm stays. No `get_unchecked` (S8). No golden
movement. No perf work beyond Z's own span (D-perf-6). No re-opening settled designs
— and **no half-landed context flip**: face 1 lands whole on top of face 0, or it is
reverted, which is what sessions X and Y both did with the measurement they produced.
