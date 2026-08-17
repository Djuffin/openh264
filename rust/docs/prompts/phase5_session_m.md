> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session M — T5.M: 5.2's last structural pieces, then 5.3's core

> **EXECUTED 2026-08-13. Superseded-historical.** Faces 1–5 landed (T5.M1–T5.M4); face 6
> dropped per this file's own rule. Three of its premises were wrong and are corrected in
> place below, marked **[S24]**. The record is the session-M log entry.

> Loaded per Eugene (2026-08-12): faces 5–6 add 5.3's grid-independent core. If wall
> time runs out, faces drop from the end at seam boundaries — 6 first, then 5.

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.4 (D-perf-4; **D-perf-5 is
answered and closed** — every hot family flipped and the cumulative span is measured
rather than summed) and §7.6 — S2b's pair-count clause is ratified and its "measure
something bigger" conclusion is now a session-L *result*; S8's fourth negative result
stands: **no window hoisting**; S13/S14/S16/S20/S21/S24/S25/S27/S28/S29 as always; the
session-L log entry. This file scopes the session and supersedes on disagreement; fix
disagreements in place. Counts measured at `f63e8ef6`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (session L's log entry, `perf_baseline.md`'s L section,
   F3 measurement 35, F36's widening, phase5.md's §0/§2/§8 marks, plan §0's rows and
   §7.4's SIMD correction, this brief).
2. Open per **S27**: session L ended accepted (**OVERALL: PASS** on the L1–L3 battery;
   the L4–L7 battery's two failures were the ratchet, regenerated in `f63e8ef6`, and one
   F3 hit adjudicated as measurement 35 — both discharged in the log entry, which is what
   S27's "accepted" means). Tail is docs-only — cheap subset if `rust/tools/` and the
   toolchain are unchanged. Last recorded: **468 / 462 / 20**, Miri **328** (~908s),
   census **60**, goldens **57**, `raw_ptr` **4507**. Recount.
3. **Run the S2 null at 7 pairs** before §1.

## 1. Face 1 — the day-two confirmation the cumulative reading owes

Session L measured the whole 5.2 flip as one span — **CB +2.93%, decode median +1.01%**,
against a null band 0.16 points wide — and put cumulative CB at **≈ +20.7%** against the
≈**+23%** stop-line: **≈2.3 points of headroom** for the rest of 5.2 and all of 5.3–5.6.
Both binaries are stashed; nothing needs building.

```bash
FFMPEG=/opt/homebrew/bin/ffmpeg python3 rust/tools/perfpair.py null l_c2 --pairs 7
FFMPEG=/opt/homebrew/bin/ffmpeg python3 rust/tools/perfpair.py run seat_head l_c2 --pairs 7
```

- **If it confirms** (CB above the null band, within ~1 point of +2.93%): the cumulative
  position is settled for the phase and the headroom figure is what the rest of 5.2 and
  5.3–5.6 spend against.
- **If it disagrees in sign or by more than a point**: that is a result about the
  *aggregate* instrument, as session K's was about the per-family one. Say so plainly and
  take it to Eugene with both tables before spending headroom.

**The headroom is thin and the per-family cost is no longer "nothing".** Session L's seven
families read +1.27% CB together where each had read ≈0 alone. Anything in this session
that touches the decode path gets measured as a **cluster or a whole-session span**, never
as a per-face half (S2b, and session L §2's correction).

## 2. Face 2 — `SDqLayer` becomes `DqLayerState`

Unblocked: all 22 array families are flipped and the struct holds no per-macroblock
pointer. The struct's own doc comment names this as the trigger.

- ~~The census key `type SDqLayer x2` goes to `x1`.~~ **[S24] Wrong: the line is
  *removed*, not re-keyed.** `find_dup_types.sh`'s `dup_report` prints only names with
  `n > 1`, so after the rename `SDqLayer` is never reported and an `x1` entry is text the
  gate never consults — the disposition `type SMbCache x2` got at T5.E2. **60 → 59.**
- `assert_size!(SDqLayer, 512)` in `encoder/abi_guard.rs` pins the **encoder's** namesake
  and does not move. The decoder's has no size assert and no offset pin (§2).
- **29 files name `SDqLayer` crate-wide** (re-grep: the encoder's `svc_encode_slice.rs`
  copy is in that count and is not this rename's). A rename is not a conversion — one
  mechanical commit, S20 satisfied by construction.
- Perf: S2c's waiver applies if the commit is a pure rename (byte-identical output, no
  kernel, no allocation path, no dispatch, no shim retirement) — state the conditions.

## 3. Face 3 — the scratch-cache re-points

~~**Re-greped at session L: 32 raw scratch-cache parameters** — `parse_mb_syn_cabac.rs`
19, `parse_mb_syn_cavlc.rs` 8, `mv_pred.rs` 5.~~ **[S24] It is 45** —
`parse_mb_syn_cabac.rs` **26**, `parse_mb_syn_cavlc.rs` **10**, `mv_pred.rs` **9**. The
brief's grep missed `FillSpatialDirect8x8Mv`/`FillTemporalDirect8x8Mv`'s pairs and both
`*const` spellings in `ParseMvdInfoCabac`. A fourth cache type it names, the `*mut u8`
non-zero-count family, is **167 uses with 96 in `decode_slice.rs`** and is 5.6's by P1. The 30-entry
caches (`*mut [[[i16; 2]; 30]; LIST_A]`, `*mut [[i8; 30]; LIST_A]`, `*mut [i8; 30]`,
`*mut u8`) become `&mut` locals passed down; their owners are two stack locals in
`decode_slice.rs` (§2 records `:4632` and `:4836` — the line numbers have moved, re-grep).

These are **stack** arrays, not the grid: the work is a signature change plus an S25
re-entrancy question per function, not a family flip. It is the last large mechanical
block in 5.2. Size it by the S20 closure and expect it to want more than one commit.

## 4. Face 4 — `pBitStringAux` and the decoder's last `SHIM(phase5)`

§2 records 24 sites, **one writer** (`decoder_core.rs`, pointing into the NAL unit) and 17
readers in `decode_slice.rs`; `cabac_decoder.rs`'s `cabac_rbsp_window` accessor dies with
it. Those counts are three sessions old — re-grep (S24). ~~`SHIM(` should fall from 159.~~
**[S24] It does not, and the accessor does not die**: 33 sites, not 24, and
`cabac_rbsp_window`'s 18 callers in `parse_mb_syn_cabac.rs` take `pCtx` and nothing else,
so it stays until **5.6** converts them. `SHIM(` **159**, unmoved.

## 5. Face 5 — 5.3a: F22's per-function unification

The map is written (F22's entry + session D's log): the divergence is **three**
functions — `UpdateP16x16MotionInfo`, `UpdateP16x8MotionInfo`, `UpdateP8x16MotionInfo`
unify onto `mv_pred.rs`'s guarded shape — and **`Update8x8RefIdx` runs the other way**
(C++ unguarded; `mv_pred.rs`'s added guard comes off). Per function, not per module
(S21: divergences enumerated per copy in the commit message). The guard is dead code in
both trees (T5.D1's reachability answer), so **zero bytes of output move, or the commit
reverts**. F22 closes here; update its entry and §0's open-findings row in the same
commit.

## 6. Face 6 — 5.3b: the punning and `SetRectBlock`

- **Punning → byte ops** (T7/P7): F35 already converted the 13 B-direct sites and the
  block helpers; re-grep `mv_pred.rs` and the parse files for what remains of the
  `LD32/ST32`-class accesses (the survey said 92 in `mv_pred.rs` alone — S24, that
  number predates five phases). `from_ne_bytes`/`to_ne_bytes`/`copy_from_slice` on
  slice windows; same codegen; S6 widths preserved.
- **`SetRectBlock` → a typed generic on the grid** (plan 5.3's item): the fill-rect
  helper takes the grid family's `MbArray<[T; K]>` and a const width instead of a raw
  base + byte stride. S28 if any raw bridge remains; expect none (the pSliceIdc
  precedent — read-only bases became shared borrows).
- **Excluded, with the fence at the site: the colocated reads / `cur_and_ref`.**
  Colocated access borrows *two pictures at once* and needs `PicPool`'s split-borrow
  API — that is 5.1's second half, and it rides with the `PicId` cluster (next
  session), not here.

Measurement for faces 5–6 per face 1's own rule: **cluster or whole-session span**,
7 pairs, never per-face halves.

## 7. Gates

Full battery per face or tight cluster (batch before the long Miri step — ~15 minutes,
both probes); goldens frozen at **57**; sweeps 341/341 both profiles; F3 per S14 —
`n=600` is **not** a condition, and `320x192 t=4 sm=3 n=600 cabac=1` in **debug** is the
susceptible configuration (measurements 29 and 35, both reproduced in isolation); ratchet
per S16 with per-file deltas; census green. **Do not edit the working tree while a
battery is running.**

## 8. Close

Log entry, `perf_baseline.md` row, phase5.md §2 marks and §0's rows, hand-off. If faces 2–4 land, 5.2 is done but for its straggler sweep; if 5–6 land too,
5.3 is done but for the colocated reads. **The next brief is the `PicId` cluster
either way**: 5.1's second half (`PicPool`, recycling predicate, identity — five P3
tests waiting), 5.3's colocated reads on `cur_and_ref`, and 5.4's deblocking driver
(`SDeblockingFilter` holds `PicId`s; `pCsData`, the last plane-pointer mirror, dies) —
one coherent closure over picture identity.

## 9. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). **No F36 fix** — decoder threading's,
and session L established it is a partial *function*, not a dropped line. No
`PicPool`/identity/colocated/`cur_and_ref` work — the `PicId` cluster is the next
session's whole job, and starting it from a tired tail violates the standing-start
rule. No golden movement. No
pool/threading (F12/P10). No re-litigating D-perf-5 or the probe's construction.
