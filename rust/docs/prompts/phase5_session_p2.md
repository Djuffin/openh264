# Phase 5, session P′ — W2b → W5: the layer's aliases die, then the pool owns

This session executes **W2b → W5** of [`phase5.md`](phase5.md)'s closed checklist —
its done-tests are the face gates. **D-fid-1** (C++ = reference, not template;
output equivalence unchanged) and **D-gate-1** (no perf measurement; cheap gates
per commit; one full battery at close) in force. Faces drop from the end at seam
boundaries. Counts measured at `c8ebc20f`; **re-grep at each face's open** (S24 —
and a count carries the directory it was taken in).

## 0. Start

1. Commit the inherited doc tail (phase5.md's W2b settlement + W3 row, this brief).
2. Open per **S27**: session P ended accepted → cheap subset if `rust/tools/` and
   the toolchain are unchanged. Last recorded: **474 / 468 / 20**, ratchet **4404**
   (decoder **1267**), census 59, goldens 57. Recount.
3. Budget a probe run per container/file converted (T5.O3's lesson; session P's
   three were green first time).

## 1. Face 0 — W2b(i): `DqLayerState::pDec` and `pRef` die

The design is **settled** (phase5.md, "settled by reading the tree") — do not
re-open it; verify its claims by grep as you go (S24) and fix in place if the tree
disagrees.

1. **`pRef` first**: zero readers, zero writers (`rg '\.pRef\b' src/decoder/` = 0
   at `c8ebc20f`). Delete the field.
2. **S23 check before the mechanical pass** (a cache becomes a derived value only
   if the source cannot change behind it): `pDec`'s stamp is
   `InitDqLayerInfo(.., dec_pic(pCtx))` at `decoder_core.rs:3704`; production
   writes to `ctx.pDec` after it are the **`= None` resets**
   (`decoder_core.rs:3714, 3736, 3845, 3860, 3950, 3954` — re-grep; the
   `error_concealment.rs:1061` write is a test fixture). Read
   `DecodeCurrentAccessUnit`'s order: if every reset post-dates every layer-read
   in the AU, derivation is exact; if not, the divergence is load-bearing —
   record which reads see stale-non-null, and preserve that semantics explicitly
   (a local carrying the stamp, not a field). **Record the answer in the log.**
3. Then the field dies, by site class:
   - (a) function already takes `pCtx` — the plane consumers
     (`WelsMbInterConstruction` `decode_slice.rs:2063`, `WelsMbInterPrediction`
     `:2115`) and the deblocking/EC/parse paths that carry it → read
     `dec_pic(pCtx)`; the parse-only null arm is `dec_pic`'s null, **both arms
     stay live** (`bParseOnly` is API-reachable, D2).
   - (b) layer-only leaves (`PredPSkipMvFromNeighbor` `mv_pred.rs:439`'s shape;
     the 13 `mv_pred.rs` and some `parse_mb_syn_*` sites) → take `PPicture` from
     their ctx-holding caller, or merge into it (D-fid-1). A handful of leaves —
     if a chain turns out deeper than ~2 hops, stop and say so in the log rather
     than threading a parameter through half the file.
   - (c) identity compares → `PicId` equality, no deref.
4. The non-null arm touches the picture's per-MB arrays (`SPicture.pMv`/
   `pRefIndex`) — they stay behind `PPicture` this face; Face 2 owns them.
5. Sizing: layer-pointer-spelled reads grep **134** by
   `\(\*(pCurDqLayer|dq|pDqLayer)\)\.pDec`; phase5.md's W2b row totals 159 by a
   wider net. Re-grep, reconcile the two in the log, convert what the grep finds.
6. Probe run per file converted.

## 2. Face 1 — W2b(ii): the reference lists

`pRefList` 82 + `pShortRefList` 25 + `pLongRefList` 24 + `pRefPic` 113 +
`ppRefPic` 24 + `pPreviousDecodedPictureInDpb` 11 → `PicId`/indices — T5.N4's
recipe (the deblocking lists already did this). **All counts decoder-only**; the
228 `src/encoder/` sites are F12/P10's, out of scope. S25 per function; probe per
file.

## 3. Face 2 — W3: the pool owns

- `Pool<PPicture>` → `Pool<Box<SPicture>>`; **`dec_pic`/`ec_ref_pic` are the only
  two sites that become pool borrows** (T5.P2's design, `decoder_context.rs:584`).
- `mut_and_rest` — the split-borrow API owed since session N; T5.N5's colocated
  `debug_assert!` becomes the safe form.
- `pTempDec` → owned `Box<SPicture>` (not a pool slot, `decoder_context.rs:678`);
  `pDqLayersList`, `pPicBuff` owned; remaining free-cascade entries die into
  `Drop` (R4); F19 per allocation.
- **`SPicture` finishes owning itself**: `pMv`/`pRefIndex` (`picture.rs:261`) go
  the way the planes went at T5.C3 — same recipe as the grid families.
- Shell extended per owned field (S21); never deleted (it is the constructor).
- Done when: zero `WelsMallocz`/`WelsFree` in `src/decoder/`; cascade functions
  deleted; probe per container.

## 4. Face 3 — W4: colocated + 5.3b

`GetColocatedMb` onto `cur_and_ref`; `SetRectBlock`/`CopyRectBlock4Cols` onto the
grid; the remaining punning → byte ops (**325 `LD*/ST*` tokens** at `c8ebc20f` —
re-grep). S6 widths preserved; no window hoisting (S8 #4). Done when the decoder
`LD32|ST32|LD16|ST16|LD64|ST64` grep reads 0.

## 5. Face 4 — W5: P4

`pSps`/`pPps` → active-paramset ids + lookup at use — **205 occurrences (131 +
74), 4 carriers**. The S23 read is done (session O hand-off §3): the buffer is
written on the activation path, so no lookup borrow outlives its expression —
F41 is this question answered wrongly for `pParam`; do not repeat it. Done when
`.pSps|.pPps` greps read 0.

## 6. Stretch — W6 entry, only if faces 0–4 closed

Open `decode_slice.rs` per P1 with the NZC `*mut u8` cache family (~167 uses,
re-grep). Do not start what a seam boundary can't close.

## 7. Gates — D-gate-1

Per commit (~3 min): build both profiles + tests + ratchet + census. Probe runs
per container/file as above. Once at close: full battery — goldens 57 frozen,
sweeps 341/341 both profiles, benches bit-identical, Miri both probes; F3 per S14
(hash shortcut first; measurements 40–41 are on the books). **Do not edit the
working tree while the battery runs.**

## 8. Close

Log entry (per-face inventories, the S23 answer from Face 0, F19 answers,
reconciled counts), phase5.md strike-throughs and §0's rows, hand-off: whatever
faces remain, else **P″ = W6 + W7**, then **Q = W8 — the exit, never compressed**
(session N's stashed binaries, the niche verdict, the stop-line, the
recovery/ledger adjudication, `prompts/phase6.md` per S19).

## 9. Non-goals

No perf measurement (D-gate-1). No encoder `pRefPic`/`pRefList` sites (the 228 —
F12/P10). No F23/F38-class/F41/`api/` work (Phase 8's). No F36 work (decoder
threading's). No golden movement. No `get_unchecked` (S8). No shell deletion (it
is the constructor). No re-opening W2b's design (settled; disagreements between
the settlement and the tree are fixed in place and logged).
