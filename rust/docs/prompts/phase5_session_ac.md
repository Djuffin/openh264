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

## 0. Start

1. Commit the inherited doc tail.
2. Open per **S27** (AB's `exit` battery was `OVERALL: PASS` 13/0/1 at `11002e4c`;
   `rust/tools/` and the toolchain are unchanged): build both profiles, tests,
   ratchet, census. Recount decoder `raw_ptr` **split code/prose**, `unsafe fn`,
   deny-clean, `SHIM(`, and `PPicture` by its own grep.
3. Probe per seam (S32 beside any probe change); S33 on every number; breadcrumb
   per face.

## 1. The nine modules, measured at AB's close

Decoder `raw_ptr` **276 = 224 code + 52 prose**. The prose share is a fifth of the
metric now, so **report the split or the number means nothing** (S16). Per module,
**code occurrences / `unsafe fn`**, re-grep at the face:

| module | code | `unsafe fn` | what it is |
|---|---|---|---|
| `decoder_core` | 43 | 59 | the largest, and the one nothing else waits on |
| `decoder_context` | 36 | 6 | the accessors' home; count the type aliases first |
| `pic_queue` | 28 | 1 | **already deny-clean**; the 28 are the survivor's producers and `PicRefs`/`RefSlot`'s own machinery |
| `decode_slice` | 23 | 47 | 16 of the `unsafe fn` are `PPicture` survivors — they keep the keyword |
| `nalu` | 23 | 13 | `PNalUnit`/`PSliceHeader`, the parse tree's two |
| `error_concealment` | 21 | 11 | §2 — one structural item |
| `parse_mb_syn_cavlc` | 16 | 18 | `SVlcTable`'s varying-length raw sub-tables (family 8's old blocker) |
| `slice` | 10 | 0 | **deny-clean already**; aliases and API-boundary types — count before scheduling |
| `parse_mb_syn_cabac` | 8 | 11 | 2 survivors |
| `parameter_sets` | 6 | 0 | **deny-clean already** |
| `manage_dec_ref` | 4 | 15 | signatures that name a raw pointer keep the keyword (session Z's rule) |
| `picture` | 4 | 0 | **deny-clean already**; `data_ptr`/`data_ptr_ref` are the survivor |
| `fmo` | 1 | 0 | **deny-clean already** |
| `mv_pred` | 0 | 3 | **deny-clean**; the 3 are survivors |

**Five modules carrying `raw_ptr` in code are already deny-clean** (`pic_queue`,
`slice`, `parameter_sets`, `picture`, `fmo`) — their occurrences are the survivor,
its producers, or type aliases inside allowed items. So exit condition 1's remaining
surface is **the nine non-deny modules**, and condition 2's is the same nine. Ask
which of the 224 are the survivor's before scheduling anything (S24's unit clause).

Order by what each takes off the deny-clean list, cheapest first: `manage_dec_ref`
(4/15), `parse_mb_syn_cabac` (8/11), `error_concealment` (21/11 — §2 first),
`parse_mb_syn_cavlc` (16/18), `nalu` (23/13), `decode_slice` (23/47),
`decoder_context` (36/6), `decoder_core` (43/59).

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

1. Full battery at `exit` level; F3 per S14 — **and check step 0 first**: session AB
   found the hash shortcut inapplicable to a pure code move, which is worth one
   build to establish rather than assuming either way.
2. AC's span per S2b — base **AB's close (`11002e4c`)**, stashed as `ab_head`.
   **No other perf work** (D-perf-6). Do not inherit a base from a previous brief
   without checking it was not already measured; AB's brief carried a stale one.
3. **`prompts/phase6.md` per S19** — only if the phase actually exits.
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
