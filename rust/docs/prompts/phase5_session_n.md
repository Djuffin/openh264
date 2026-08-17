> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session N — T5.N: the `PicId` cluster

Governing: [`phase5.md`](phase5.md) §0/§1/§2/§4 verbatim; plan §7.4 (D-perf-4; the
cumulative position is **settled**, not provisional — two days, ≈2.3 points of headroom,
§1 below) and §7.6 — S13/S14/S16/S20/S21/S24/S25/S27/S28/S29 as always; the session-M log
entry. This file scopes the session and supersedes on disagreement; fix disagreements in
place. Counts measured at `16a6130c`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (session M's log entry, `perf_baseline.md`'s M section,
   F3 measurement 36, F22's closure, phase5.md's §0/§2/§3/§7/§8 marks, plan §0's rows,
   this brief).
2. Open per **S27**: session M ended accepted (**OVERALL: PASS** on the final battery;
   the one mid-session FAIL pair was F3 measurement 36, acquitted under S14 step 1, and a
   ratchet increase that was the regeneration T5.M3 owed — both discharged in the log
   entry). Tail is docs-only — cheap subset if `rust/tools/` and the toolchain are
   unchanged. Last recorded: **468 / 462 / 20**, Miri **328** (~908s), census **59**,
   goldens **57**, `raw_ptr` **4460**, `unsafe_fn` **1241**. Recount.
3. **Run the S2 null at 7 pairs** before the first perf verdict.

## 1. The perf position — read once, do not re-derive

**Cumulative CB ≈ +20.4…+20.7%** against the ≈**+23%** stop-line: **≈2.3 points of
headroom** for all of 5.3–5.6. The whole 5.2 flip was measured directly as one span and
**confirmed on a second day** (+2.93% → +2.57%, null band 0.17 points wide), which is what
S2b's day-two clause exists to produce. Session M's own four faces read **−0.77% CB**,
below the null floor on every row.

Two rules follow and both are binding:

- **Measure the session as one span**, 7 pairs, with a null at 7 pairs — never a per-face
  half (S2b; session L's proof that per-unit readings at the resolution limit *under-report
  systematically*).
- **Do not re-derive the cumulative figure from per-family numbers.** It is measured.

## 2. Face 1 — 5.1's second half: `PicPool`, the recycling predicate, `PicId`

The last open piece of 5.1, deferred since 2026-08-11 and now the thing three other faces
want. `safe/pool.rs` already has the handle type and its `pair_mut`.

- **Read the five P3 identity tests first** — `deblocking.rs` ×3, `error_concealment.rs`
  ×2, all same-POC. They exist to gate exactly this.
- `pic_queue.rs`'s recycling predicate becomes a method on the pool.
- `PicId` for the three identity sites (`manage_dec_ref.rs`, `error_concealment.rs`).
- **F19's check per allocation**: *which line frees this?* `AllocPicture`/`FreePicture` are
  T5.C3's heap constructor/dropper; the pool must not add a second owner.
- S25 with force: `WelsInitRefList`'s re-entrant pair is the enumerated hazard (session C).

## 3. Face 2 — 5.3's colocated reads on `cur_and_ref`

Fenced out of session M **at the site**, because colocated access borrows *two pictures at
once* and needs Face 1's split-borrow API. `mv_pred.rs`'s `GetColocatedMb` is the consumer.

Doing this unblocks 5.3b's `SetRectBlock` work, whose call sites are mostly inside
`GetColocatedMb` — which is why session M did not start it.

## 4. Face 3 — 5.4's deblocking driver

`decoder/deblocking.rs`: `SDeblockingFilter` holds `PicId`s + per-MB plane cursors;
identity compare per P3, and the three deblocking identity tests gate it.

**`pCsData` is the last plane-pointer mirror in the decoder** and dies here — the same
class as `pBitStringAux` (T5.M3) and for the same reason: a pointer cached beside its
owner. T5.M3's method transfers directly, and so does its lesson — **check that the route
you replace the mirror with is as fresh as the mirror was.** `pCtx.pNalCur` was not, and
nothing had noticed for five phases.

## 5. Gates

Full battery per face or tight cluster (batch before the long Miri step — ~15 minutes,
both probes); goldens frozen at **57**; sweeps 341/341 both profiles; F3 per S14 — the
susceptible configuration is now `320x192 t=4 sm=3 n=600` in **either** profile with
**either** entropy coder (measurement 36 took `cabac` and `rc` out of the signature);
ratchet per S16 with per-file deltas; census green at **59** allowlisted and a
duplicate-body budget of **195**. **Do not edit the working tree while a battery is
running.**

## 6. Close

Log entry, `perf_baseline.md` row, phase5.md §1/§3/§4 marks and §0's rows, hand-off. If all
three faces land, **5.1 and 5.4 are closed and 5.3 is done but for 5.3b**, and the next
brief is 5.5 (`decoder_core.rs`: allocation → constructors, the paramset store, context
decomposition, `Drop` teardown) — the phase's largest remaining step, expected to want two
sessions.

## 7. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). **No F36 fix** — decoder threading's,
and it is a partial *function*; session M added the stale `pCtx.pNalCur` write to its
inventory. **No 5.3b** (the 279 `LD*`/`ST*` punning sites in `mv_pred.rs`, 21 in
`parse_mb_syn_cavlc.rs`, and `SetRectBlock`/`CopyRectBlock4Cols` onto the grid) — it is a
face of its own and it wants Face 2 done first. **No `*mut u8` non-zero-count cache work**
— 167 uses, 96 in `decode_slice.rs`, so 5.6's by P1. No 5.5 pulls. No golden movement. No
pool/threading (F12/P10). No re-litigating the cumulative measurement.
