> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session Y — the context, and the phase close

Exit conditions 1–3 are unmet (decoder `raw_ptr` 456; 11 of 22 modules deny-clean;
`SHIM(` 7 against a survivor list of 1). Condition 5 is met and closed (D-perf-6).
**D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**, forcing rules v2 in force.
Counts at `ec672022`; re-grep at each face's open (S24).

**Read first**: `phase5.md` §"The context, measured at the face (session X)". The
flip was attempted, measured and reverted; the blocker is one shape and the design
that clears it is the settlement's, unchanged.

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27**: X closed `OVERALL: PASS` at `exit`. Last recorded: decoder
   `raw_ptr` **456**, deny-clean **11/22**, `SHIM(` 7, corpus 2690/17 + 2707/0,
   conformance 60/60, Miri 338/0 + 20/7/3. Recount.
3. Probe per seam (S32 beside any probe change); S33 on every number; breadcrumb
   per face.

## 1. Face 0 — the slice bracket splits the context

The measured blocker: `WelsDecodeSlice` does `let (pDec, pRefs) = cur_and_refs(pCtx)`
and `PicRefs<'a>` borrows `pPicBuff` for the slice, while the per-macroblock dispatch
below takes `pCtx` **whole** (`pDecMbFunc(pCtx, dq, pDec, pRefs, pNalCur, uiEosFlag)`).
As raw pointers those coexisted; as borrows they cannot.

1. Build `SliceCtx<'a>` in `decoder_context.rs` at the three bracket tops
   (`DecodeCurrentAccessUnit`'s loop, `CheckAndFinishLastPic`'s concealment block,
   `InitialDqLayersContext`) — field-precise borrows (S29) packaged as `&mut` for
   the three state machines (the raw-data reader, the CABAC engine, the flag/counter
   set), `&` for tables and config, **copied scalars** where S23 clears them
   (verify per field), `pParam`'s scalars copied *inside* the constructor so F41's
   raw field never escapes. **`pPicBuff` is not in it** — `PicRefs`/`pDec` already
   travel as their own parameters and that is what makes the split work.
2. The dispatch type and everything below it take `SliceCtx`, never `pCtx`.
3. Order: `parse_mb_syn_cabac` (24 of 34 take the context, and its three operands
   are already disjoint field paths — see §2), then `parse_mb_syn_cavlc`,
   `mv_pred`, `deblocking`, `error_concealment`, `manage_dec_ref`, `nalu`,
   `decode_slice`, `decoder_core`, `decoder_context` last.

Done when `*mut SWelsDecoderContext` and `PWelsDecoderContext` read **0** outside
the constructor, and the constructor is the enumerated exception.

**F53's rule applies to every step of this face** (`phase5_findings.md`): converting a
parameter from `*mut T` to `&mut T` invalidates every raw alias derived from it that
outlives a reborrow, so each function's work list is its own `addr_of_mut!((*p).…)`
sites as well as the parameter's uses. X met it twice — once as a Miri failure, once
as a grep sweep — and the decoder holds none *of the layer's*; the context's are the
ones this face creates.

## 2. Two facts session X measured and did not land

- The CABAC call's three operands are three **disjoint field paths** — `sRawData`,
  `sCabacDecEngine`, `pCabacCtx` — and compile as such once the window borrows the
  field: `cabac_window_of(&pCtx.sRawData, slice_bit_reader(pCtx))` in place of
  `cabac_rbsp_window(pCtx)`. `cabac_ctx_base`'s callers index a base pointer;
  `&mut pCtx.pCabacCtx[i]` is the same access with the bound checked.
- `slice_bit_reader` needs only `&SWelsDecoderContext`: the reader lives in the NAL,
  reached through `pNalCur`'s value, so its `*mut` carries the NAL's provenance.

Both were exercised in X's reverted attempt and both compiled.

## 3. Face 1 — the remaining families

`manage_dec_ref.rs`'s 11 non-context pointers; `pic_queue.rs`'s 30 (its two named
exceptions stay); `decoder_context.rs`'s 59; `decoder_core.rs`'s 68; the `SSlice`
and `BsCursor` locals in `decode_slice.rs`; `parse_mb_syn_cavlc`'s `SVlcTable`
sub-tables. Sizes re-derived at each family's open.

## 4. Face 2 — the `common/` boundary, **decided** (steward, at `3e2f43e6`)

The either/or ("convert the kernels or except the modules") missed the strangler's
own third form: **safe slice-taking entry points added beside the raw ones, in
`common/`**. The kernel body is shared; the safe form takes what the decoder now
has (slices, cursors); its internal `unsafe` block lives in `common/`, which exit
condition 2 does not scope (it reads "every **decoder** module"). So:

1. Decoder callers move to the safe forms; `deblocking.rs`, `decode_mb_aux.rs`
   and `error_concealment.rs` go deny-clean **now**, with no encoder edit
   (F12/P10 holds — the raw forms stay for the encoder's callers).
2. The raw forms carry a one-line pointer: **deleted in Phase 6** when the
   encoder's callers convert onto the safe forms.
3. Per-kernel fallback, used only where a wrapper cannot express the contract:
   the enumerated exception with its Phase pointer (`expand_pic`'s two named
   survivors are already this).

The surface is small — re-derive at the face (S24), but the raw-signature
kernels the decoder still reaches number single digits. Write each wrapper's
width/alignment contract from the kernel's own (S6, F35); no behavior change,
byte-identical per commit.

## 5. Face 3 — W7's closure, then the phase close

1. 5.2's straggler sweep; `SHIM(` → **the survivor list exactly** (1 named:
   `data_ptr`'s output-contract consumer, Phase 8's); census green.
2. Full battery at `exit` level; F3 per S14.
3. Y's own span per S2b — **no other perf work** (D-perf-6).
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
8's). No F36 work. No `get_unchecked` (S8). No golden movement. No perf work beyond
Y's own span (D-perf-6). No re-opening settled designs — and **no half-landed
context flip**: it compiles whole or it is reverted, which is what session X did
with the measurement it produced.
