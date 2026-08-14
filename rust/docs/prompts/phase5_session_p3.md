# Phase 5, session P″ — W3's remainder, then W4 and W5: the pool owns

This session executes **W3's remainder + W4 + W5** of [`phase5.md`](phase5.md)'s
closed checklist — its done-tests are the face gates. **D-fid-1** (C++ = reference,
not template) and **D-gate-1** (no perf measurement; cheap gates per commit; one
full battery at close) in force. Faces drop from the end at seam boundaries.
Counts measured at `f5a3eac8`; **re-grep at each face's open** (S24 — a count
carries its directory *and its unit*).

**W3's design is settled** (phase5.md, "settled by reading the tree") — do not
re-open it; verify its claims by grep as you go and fix disagreements in place,
logged. The two-step maneuver is the session's spine: **hoist under `PPicture`
slots first (pure motion), flip to owned slots second (brackets only).** Do not
interleave the steps in one commit.

## 0. Start

1. Commit the inherited doc tail (phase5.md's W3 settlement, this brief).
2. Open per **S27**: session P′ ended accepted → cheap subset if `rust/tools/`
   and the toolchain are unchanged. Last recorded: 474/468/20, Miri 334, census
   59, goldens 57, decoder `raw_ptr` **1245**. Recount.
3. Probe run per file/container converted, **budgeted to fire** (P′'s second run
   convicted `AddShortTermToList` mid-face; the close battery would have found it
   three commits later).

## 1. Face 0 — the two easy owners

- `pTempDec` → owned `Box<SPicture>` field (truly W1-shaped; not a pool slot,
  `decoder_context.rs:678`; its `AllocPicture` call at `decode_slice.rs:2044`
  becomes the constructor). F19 per allocation.
- `pPicBuff: *mut SPicBuff` → owned (`Option<Box<PicPool>>`). Safe **before** the
  flip: under `PPicture` slots a `pool_pic` result's provenance is
  `AllocPicture`'s, not the pool Box's (T5.N1's invariant). Accessors deref
  through the owned field; `CreatePicBuff`/`DestroyPicBuff` shrink accordingly.
- Probe run; commit.

## 2. Face 1 — the hoist, under `PPicture` slots

Per-use accessor calls (`dec_pic`/`ec_ref_pic`/`ref_pic`/`pool_pic` — **115 call
sites**: decoder_core 30, decode_slice 27, cabac 15, manage_dec_ref 13, ctx
bodies 9, EC 7, cavlc 6, mv_pred 4, deblocking 4) become parameters threaded from
bracket tops — W2b(i)-(b)'s proven recipe. Brackets: **the slice** for the decode
path (`dec_pic` and both ref lists are constant across one slice's MB loop); per
operation for EC, DPB (`manage_dec_ref`) and output. Pure motion, semantically a
no-op (`pool_pic` copies), **byte-identical per commit**; incremental per file;
probe per file. If a chain wants more than ~2 hops of new parameter, merge the
callee into its caller instead (D-fid-1) or log the site and move on.

## 3. Face 2 — the flip, brackets only

- Slots: `Pool<PPicture>` → `Pool<Option<Box<SPicture>>>` — null slots are real
  (`CreatePicBuff`'s partial-failure arms). `slot()`/`slot_at()` copy-outs die.
- Bracket tops become the borrows: `mut_and_rest(dec_id)` → (`&mut SPicture`,
  `PoolRest`), refs via `rest.get(id)`; read-only brackets take `&` gets. The
  hoisted raw params now derive from these borrows (S28, pointers-from-borrows;
  S29 field-precise at the ctx field) and **nothing below a bracket top touches
  the pool** — that is the invariant every probe run checks.
- `AllocPicture`/`FreePicture`: `into_raw`/`from_raw` die into `Pool::replace` +
  `Drop` teardown (R4). F19 closes decoder-side. The recycling predicate is
  unchanged. **F37 is adjudicated at this seam** (the missing
  `sPictInfoList` reset — implement per its record, test per S6's rescope).
- Delete the four accessors. Done-test: `pool_pic\(|dec_pic\(|ec_ref_pic\(|ref_pic\(`
  greps read **0**; direct `.pPicBuff`/pool-field reads enumerate to bracket
  sites only (list them in the log).
- Probe run per container; commit per seam (slots, brackets, teardown may split).

## 4. Face 3 — `pDqLayersList` owns, because `pCurDqLayer` was a cache

W2b's recipe, third use: the list is one `Box::into_raw`
(`decoder_core.rs:2780`); `pCurDqLayer` is its cache — one production stamp
(`:3578`; the two `error_concealment.rs` writes are test fixtures), and the call
tree already threads it as a parameter. Delete the field: derive once at the loop
top from the owned Box (S29 spelling), thread the existing parameter; the **81
mid-tree ctx-field reads** (decode_slice 25, decoder_core 15, cabac 12,
manage_dec_ref 11, EC 11, rest 7) become param sites. Done when
`WelsMallocz|WelsFree` calls in `src/decoder/` read **0** (W3's row closes) and
the cascade functions are deleted. Probe run.

## 5. Face 4 — W4: colocated + 5.3b

`GetColocatedMb` takes cur and ref as parameters — both are already resolved at
the slice bracket, so T5.N5's `debug_assert!` becomes `mut_and_rest`'s type-level
fact. `SetRectBlock`/`CopyRectBlock4Cols` onto the grid; remaining punning → byte
ops (**325 `LD*/ST*` tokens** at `f5a3eac8` — re-grep). S6 widths preserved; no
window hoisting (S8 #4). Done when the decoder `LD32|ST32|LD16|ST16|LD64|ST64`
grep reads 0.

## 6. Face 5 — W5: P4

`pSps`/`pPps` → active-paramset ids + lookup at use — **205 occurrences
(131 + 74), 4 carriers**. The S23 read is done (session O hand-off §3): the
buffer is written on the activation path only, so no lookup borrow outlives its
expression — F41 is this question answered wrongly for `pParam`; do not repeat
it. Done when `.pSps|.pPps` greps read 0.

## 7. Gates — D-gate-1

Per commit (~3 min): build both profiles + tests + ratchet + census. Face 1's
commits additionally **byte-identical** (the hoist is motion, not change). Probe
runs per file/container as above. Once at close: full battery — goldens 57
frozen, sweeps 341/341 both profiles, benches bit-identical, Miri both probes; F3
per S14 (hash shortcut first; measurements 40–42 are on the books). **Do not edit
the working tree while the battery runs.**

## 8. Close

Log entry (per-face inventories, bracket-site list, F19/F37 answers, reconciled
counts), phase5.md strike-throughs and §0's rows, hand-off: whatever faces
remain, else **P‴ = W6 + W7**, then **Q = W8 — the exit, never compressed**
(session N's stashed binaries, the niche verdict, the stop-line, the
recovery/ledger adjudication, `prompts/phase6.md` per S19).

## 9. Non-goals

No perf measurement (D-gate-1). No encoder sites (F12/P10). No
F23/F38-class/F41/`api/` work (Phase 8's). No F36 work (decoder threading's). No
golden movement. No `get_unchecked` (S8). No shell deletion (it is the
constructor). No re-opening settled designs (W2b's, W3's — disagreements between
a settlement and the tree are fixed in place and logged).
