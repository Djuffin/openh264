> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session AC — the nine modules, and the phase close

Exit conditions 1–2 unmet; 3 met in substance with the list restated; 4 met;
5 met and closed (D-perf-6). **D-par-1**, **D-fid-1**, **D-perf-6**, **S31**–**S33**,
forcing rules v2 and **the finish rule** (`phase5_session_z.md` §"The finish rule")
all in force. Counts at session AB's close; re-grep at each face's open (S24).

**Read first**: `phase5.md` §"`PPicture` — the family the metric cannot see" and
§"Phase exit conditions". `PPicture` is **settled and executed** — 23 enumerated
survivors, each carrying `#[allow(unsafe_code)]` and the F42 argument at the item,
counted by its own signature grep and not by `raw_ptr`. **Nothing below is blocked
by a decision.** What is left is nine modules and a size each.

## The terminal rule (Eugene, 2026-08-17: "make sure it doesn't stop until
everything is done")

**This session has no hand-off.** Its §3 close is the only exit: the session ends
when exit conditions 1–3 read met **and** the close artifacts land — `phase6.md`,
briefs historical, the checklist shut, §0 refreshed, every open finding owned.
"Done" is the checklist's own definition, unchanged since it was written: every
remaining occurrence **converts, or is enumerated** (survivor / exception / prose)
with its argument at the item and its Phase pointer. A size is never a stop
(S31 — the list is finite and every row above is measured). A question is never a
stop either — it takes **the decision ladder**:

1. **Settle by reading** (the W2b method): read the code, write the settlement in
   the log, proceed. This has closed every design question of the phase's second
   half.
2. **Lint-scope questions** (where a keyword or an allow lives) default to **the
   enumerated-survivor shape** with a Phase 8 pointer — always a legal move,
   never a behavior change.
3. **Behavior questions** (output, codes, API-visible anything) **never
   default**: park exactly that item behind an item-level allow with a
   one-paragraph record and its pointer, and continue with everything else. A
   parked item joins the enumerated exceptions — which exit conditions 1–2 admit
   by construction — so it does not block the close.

The close's §0 statement **lists every item the ladder parked**; if the list is
more than a handful, the close says so plainly and the phase still exits, with
the list as Phase 8 inheritance. The only sentence that ends this session early
is a named Eugene/steward blocker that survives all three rungs — and given the
rungs, that set should be empty.

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27** (AB closed `exit` at `OVERALL: PASS` 13/0/1, twice — at
   `11002e4c` and again at `dc4d8177`;
   `rust/tools/` and the toolchain are unchanged): build both profiles, tests,
   ratchet, census. Recount decoder `raw_ptr` **split code/prose**, `unsafe fn`,
   deny-clean, `SHIM(`, and `PPicture` by its own grep.
3. Probe per seam (S32 beside any probe change); S33 on every number; breadcrumb
   per face.

## 1. The nine modules, measured at AB's close

Decoder `raw_ptr` **276 = 223 code + 53 prose**, decoder `unsafe fn` **125**. The
prose share is a fifth of the metric now, so **report the split or the number means
nothing** (S16). **Every remaining `unsafe fn` is load-bearing**: T5.AB5 stripped
all 176 at once and the compiler kept exactly these 125, so there is no cheap half
left — each one now costs a real conversion. Per module, **code occurrences /
`unsafe fn`**, re-grep at the face:

| module | code | `unsafe fn` | what it is |
|---|---|---|---|
| `decoder_core` | 43 | 36 | the largest, and the one nothing else waits on |
| `decoder_context` | 36 | 5 | the accessors' home; count the type aliases first |
| `pic_queue` | 28 | 0 | **already deny-clean**; the 28 are the survivor's producers and `PicRefs`/`RefSlot`'s own machinery |
| `decode_slice` | 23 | 34 | 16 of the `unsafe fn` are `PPicture` survivors — they keep the keyword |
| `nalu` | 23 | 13 | `PNalUnit`/`PSliceHeader`, the parse tree's two |
| `error_concealment` | 21 | 8 | §2 — one structural item |
| `parse_mb_syn_cavlc` | 16 | 8 | `SVlcTable`'s varying-length raw sub-tables (family 8's old blocker) |
| `slice` | 10 | 0 | **deny-clean already**; aliases and API-boundary types — count before scheduling |
| `parse_mb_syn_cabac` | 8 | 11 | 2 survivors |
| `parameter_sets` | 6 | 0 | **deny-clean already** |
| `manage_dec_ref` | 4 | 7 | signatures that name a raw pointer keep the keyword (session Z's rule) |
| `picture` | 4 | 0 | **deny-clean already**; `data_ptr`/`data_ptr_ref` are the survivor |
| `fmo` | 1 | 0 | **deny-clean already** |
| `mv_pred` | 0 | 3 | **deny-clean**; the 3 are survivors |

**Five modules carrying `raw_ptr` in code are already deny-clean** (`pic_queue`,
`slice`, `parameter_sets`, `picture`, `fmo`) — their occurrences are the survivor,
its producers, or type aliases inside allowed items. So exit condition 1's remaining
surface is **the nine non-deny modules**, and condition 2's is the same nine. Ask
which of the 223 are the survivor's before scheduling anything (S24's unit clause).

Order by what each takes off the deny-clean list, cheapest first: `manage_dec_ref`
(4/7), `parse_mb_syn_cavlc` (16/8), `error_concealment` (21/8 — §2 first),
`parse_mb_syn_cabac` (8/11), `nalu` (23/13), `decoder_context` (36/5),
`decode_slice` (23/34), `decoder_core` (43/36).

**`decoder_context` at 36/5 is the odd one and worth reading before scheduling**:
five `unsafe fn` against thirty-six raw-pointer occurrences says most of those are
type aliases and API-boundary spellings, not conversions.

## 2. `error_concealment.rs` — the last instance of a shape this phase has done three times

`DoErrorConSliceMVCopy` and `DoMbECMvCopy` are the module's two unconverted copy
paths, and **one thing blocks both**: `DoMbECMvCopy` takes
`&mut SWelsDecoderContext`, and its caller invokes it inside the concealment
bracket, so a picture borrow derived from `pCtx.pPicBuff` would travel beside a
borrow of the whole context. T5.Z4 already moved the EC reference's POC out of that
call for the same reason.

The answer is **`slice_split`'s maneuver aimed at the concealment bracket** — one
function that hands back the picture, the reference view and an EC view of the
context out of one borrow. The phase has executed this three times (the slice at Y,
the pool at Q, the layer at AA1) and the pieces exist: `pic_and_refs_mut` +
`PicRefs::classify` (T5.AB3) are the two halves that already work; what is missing
is the context half.

The other two copy paths converted at T5.AB3 and are the worked example. Note the
S25 table at the top of the module: it is current, and its last two rows say what
is left and why.

**`sMCRefMember`'s six `*mut u8` are a separate question and are not this face's**
unless the bracket lands first: it is the C++'s own MC descriptor (`#[repr(C)]`),
and converting it to plane cursors is a vocabulary change. Size it before deciding;
do not start it as a spelling pass.

## 3. The close

1. Full battery at `exit` level; F3 per S14 — **and check step 0 first, with a
   build rather than a judgement**. Session AB ran it three times: it did *not*
   apply to a definition moved between modules (measurement 59) or to a decoder
   change (60), and it **did** apply to the keyword sweep (61), where the two
   binaries hashed identically and the hit was acquitted by construction. The
   trigger is the hash, never "it looks cosmetic".
2. AC's span per S2b — base **`ab_head`**, which is stashed at **`11002e4c`**, two
   commits before AB's close. That is deliberate and safe rather than sloppy:
   T5.AB4 and T5.AB5 are keyword-and-block only, and T5.AB5's `rust_enc` hashes
   identically to its parent's — measured, not assumed — so the span already
   covers the whole session. Re-stash at `dc4d8177` if you would rather not rely
   on that. **No other perf work** (D-perf-6). Do not inherit a base from a
   previous brief without checking it was not already measured; AB's brief carried
   a stale one.
3. **`prompts/phase6.md` per S19 — unconditionally**: under the terminal rule the
   phase exits this session by construction (everything converts or is
   enumerated with its pointer), so the encoder brief is written, full stop. It
   inherits the playbook: the ordering rule, cache-not-carrier,
   take-what-you-reach, the bracket maneuver (four applications), the pointer
   family as the working unit, the vestigial sweep run whole (session Z's rule),
   settlements-in-writing, the decision ladder, the probe budget and S32, S31,
   S33, and D-par-1's standing referee suite.
4. Briefs stamped historical; phase5.md's checklist closed; §0 refreshed; open
   findings each with an owner (F3→Phase 7, F23/F38-class/F41 + the `api/`
   inventory→Phase 8, **`PPicture`'s option-1/2 revisit→Phase 8**, F36→threading-
   or-deletion, F52's six encoder-side shadowing-stub candidates→Phase 6, F54's
   rule folded into S21, the `CABA2_SVA_B` annotation standing).

## 4. Gates

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close. **Do not edit the working tree while
the battery runs.**

## 5. Non-goals

No encoder sites (F12/P10 — Phase 6's), and note the one rule AB earned: *an
encoder-file edit that moves a definition to its C++ home and changes only import
spellings is not encoder work.* No F23/F38-class/F41/`api/` work (Phase 8's). No
F36 work — `sTmpRefPic`'s arm stays. **No re-opening `PPicture`**: the 23 survivors
are ruled, and retiring the F42 arm is Eugene-level whenever proposed. No
`get_unchecked` (S8). No golden movement. No perf work beyond the span (D-perf-6).
