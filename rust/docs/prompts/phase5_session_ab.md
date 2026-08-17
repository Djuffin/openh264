> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session AB — `PPicture`, and the phase close

> **SPENT — historical (S19).** Executed 2026-08-17 in 3 commits,
> `41149605`…`11002e4c`. Faces 0 and 1 landed, face 2's W7 closure and phase-close
> items landed, and **the phase did not close** — exit conditions 1 and 2 are size,
> not blockers. The record is `safety_refactor_log.md` §"Phase 5, session AB"; the
> successor is [`phase5_session_ac.md`](phase5_session_ac.md). **Three of this
> brief's own facts were wrong and are corrected there**: the unblocked count is
> **39, not 41** (`manage_dec_ref`'s two are prose), the family total omits **7
> producer signatures** in `pic_queue`/`decoder_context`, and §3.3's perf base
> (`y_head`) is inherited from AA's brief where it was correct — AB's own span is
> measured against `aa_head`. §2's "the `common/` boundary's three named files,
> complete" also does not land: `error_concealment.rs` is blocked on the context,
> not on `common/`.

Exit conditions 1–3 unmet. Condition 5 met and closed (D-perf-6).
**D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**, forcing rules v2 and
**the finish rule** (`phase5_session_z.md` §"The finish rule") all in force.
Counts at session AA's close; re-grep at each face's open (S24).

**Read first**: `phase5.md` §"`PPicture` — the family the metric cannot see". Its
§0 below is the one thing in this brief a session cannot decide for itself.

## 0. The decision, and it is Eugene's or the steward's

`PPicture` (= `*mut SPicture`) is the largest family left. **78 signatures over 10
modules** at session AA's open; **64 at its close**, after `deblocking.rs`'s eleven
(T5.AA2) and `manage_dec_ref`'s three (T5.AA4) converted. Of the 64, **23 cannot
convert without a behaviour or design decision** and 41 can.

`PicPool::cur_and_rest` hands a bracket the current picture as `*mut SPicture` and
the rest as `PicRefs`. `PicRefs::get` answers the *current* slot from `cur_ptr`, a
pointer sharing the mutable half's tag, because a malformed stream can legally put
the picture being decoded into a reference list and the C++ resolves it and reads on
(**F42**; `PoolRest::get` panics on that slot). Sharing a tag is what makes the
aliasing sound. As a `&mut`, every function-entry retag on the picture pops
`cur_ptr` and the next read through the F42 arm is UB. `mv_pred`'s strip-and-build
prices it: **470 errors, every one a dereference of `pDec`**.

The three options:

1. **`PicRefs::get` returns `Option<&SPicture>`, F42's arm goes.** A reference list
   naming the current picture then resolves to nothing instead of to the picture —
   a behaviour change on malformed input, which S6's never-widen default forbids
   without a decision.
2. **Interior mutability on the picture's planes**, so a shared alias of the picture
   being written is legal. A design change to `SPicture`, and it reaches the
   encoder's picture type (F12/P10's boundary).
3. **`PPicture` becomes the phase's second enumerated survivor** with a Phase
   pointer; the 23 keep `#[allow(unsafe_code)]` at the item. Exit condition 2 admits
   this in shape; exit condition 1 would have to name the family the way it names
   `data_ptr`.

**Recommendation, if one is wanted**: 3 for this phase and 1 or 2 as a Phase 7/8
item, because 1 changes decoder behaviour on exactly the input class no gate here
referees and 2 is a vocabulary change with an encoder blast radius.

**RULED (steward, at `6b6dd9a3`): option 3, the recommendation ratified.**
Grounds: option 1 diverges on exactly the input class D-par-1 spent three
sessions bringing to a refereed **2707/0** — it is not a lint question, it is a
parity question, and it stays **Eugene-level whenever proposed**; option 2
changes the vocabulary of the hottest data in the decoder, reaches the encoder's
picture type (F12/P10), and buys an unknown perf cost against a stop-line only
just recovered to ≈+23.7…+24.3. Option 3 changes **no behaviour at all** — it is
what the survivor list exists for: a named raw alias whose soundness argument
(the shared tag, F42's contract) is written at the type. Execution: the 23 carry
`#[allow(unsafe_code)]` at the item with the F42 pointer; exit condition 1 names
the family beside `data_ptr` and counts it **by its own grep** (an alias
spelling is invisible to the `*mut` count — AA's lesson); the option-1/2 revisit
is **Phase 8's**, recorded in its inheritance list.

## 1. Face 0 — the 55 unblocked picture signatures

Not blocked by §0 and convertible today — **41**, re-grepped at AA's close:
`decode_slice` 18, `mv_pred` 12, `parse_mb_syn_cabac` 6, `parse_mb_syn_cavlc` 3,
`manage_dec_ref` 2. The blocked 23 are `decode_slice` 16, `mv_pred` 2,
`parse_mb_syn_cabac` 2, `parse_mb_syn_cavlc` 2, `error_concealment` 1.
`decoder_context::pic_split` is the bracket (T5.AA2) — `(Option<&mut SPicture>,
SliceCtx)`, sound wherever the scope resolves no reference pictures. **The test is
the bracket, not the function**: grep `PicRefs` in the enclosing bracket.

Order by what it takes off the deny-clean list: `mv_pred` (18 `unsafe fn` / 4
`raw_ptr`) first — with the two `PicRefs`-carrying functions left as named
exceptions, it is the first module to show §0's option 3 in practice.

## 2. Face 1 — `error_concealment.rs`'s MC paths, and a straggler under them

14 `*mut u8`, two pictures at once (`pDstPic` written, `pSrcPic` read), and
`same_picture` already guards the aliasing case §0 is about. Whether
`cur_and_rest` can express that guard is measurable without §0's answer, and if it
can, the module goes deny-clean with `decode_mb_aux.rs` (already there) and
`deblocking.rs` (T5.AA2) — the `common/` boundary's three named files, complete.

**Under it is an S18 straggler the F43 sweep surfaced at AA's close.**
`error_concealment.rs`'s `WelsCopy16x16_c`/`WelsCopy8x8_c` are **raw row loops**,
and the encoder's same-named pair (`encoder/encode_mb_aux.rs:968`/`:993`) are
Phase-2 shims over the safe `copy_8x8`/`copy_16x16`. Same C++ function, one copy
converted and one not — Phase 2's own finding, in the other codec. **The safe
kernels are stranded in `encoder/`**, so the decoder cannot reach them without the
dependency inversion `T5.AA3` just removed for `expand_pic`. The fix is to move
`copy_8x8`, `copy_16x16` and `copy_shim` to `common/` and have both codecs import
from there — which edits an encoder *file* while converting no encoder *site*.
**RULED (steward, at `6b6dd9a3`): admitted.** `common/` is the C++'s **own home**
for these kernels — `WelsCopy16x16_c` is defined in `codec/common/src/copy_mb.cpp`
— so the move restores F22's rule (*home = the C++'s home*) rather than bending
F12/P10: it converts **no encoder site**, changes only import spellings on the
encoder side, and is byte-identical per commit. The rule it earns, one line: an
encoder-file edit that moves a definition to its C++ home and changes only import
spellings is not encoder work.

The rest of the sweep is clean: `find_elem_byte_confusion.py` **0 suspects over 81
files**, `find_unwritten_fields.py` clean, and every other F43 candidate on the
decoder side is a trait-impl `default`, which the tool's own note excludes.

## 3. Face 2 — W7's closure, then the phase close

1. `SHIM(` reads **3** in `src/decoder/`: one prose tombstone and
   `SPicture::data_ptr` with its shared form `data_ptr_ref`. **Restate the survivor
   list as those two** — it names one today and the second is the same accessor's
   `&self` form, added because a reference resolves out of `PoolRest`.
2. Full battery at `exit` level; F3 per S14.
3. The span per S2b — base **Y's close (`dff3f78b`)**, which is stashed as
   `y_head`; sessions Z and AA are both inside it. **No other perf work** (D-perf-6).
4. **`prompts/phase6.md` per S19** — only if the phase actually exits.
5. Briefs stamped historical; phase5.md's checklist closed; §0 refreshed; open
   findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the `api/`
   inventory→Phase 8, F36→threading-or-deletion, F52's six encoder-side
   shadowing-stub candidates→Phase 6, F54's rule folded into S21, the `CABA2_SVA_B`
   annotation standing, **and `PPicture`'s disposition per §0**).

## 4. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree while
the battery runs.**

## 5. Non-goals

No encoder sites (F12/P10 — Phase 6's). No F23/F38-class/F41/`api/` work (Phase
8's). No F36 work — `sTmpRefPic`'s arm stays. No `get_unchecked` (S8). No golden
movement. No perf work beyond the span (D-perf-6). No re-opening settled designs.
