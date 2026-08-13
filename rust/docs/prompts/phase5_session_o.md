# Phase 5, session O — T5.O: 5.5's first half (`decoder_core.rs`), after a perf debt

Governing: [`phase5.md`](phase5.md) §0/§2/§5/§7 verbatim; plan §7.4 (D-perf-4; the
cumulative position **moved at session N** and is **provisional again** until §1 lands)
and §7.6 — S13/S14/S16/S18/S20/S21/S24/S25/S27/S28/S29 as always; the session-N log entry.
This file scopes the session and supersedes on disagreement; fix disagreements in place.
Counts measured at `d0b7f399`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (session N's log entry, `perf_baseline.md`'s N section,
   F3 measurement 37, F37, phase5.md's §0/§1/§3/§4/§7 marks, plan §0's rows, this brief).
2. Open per **S27**: session N ended **accepted** — its final battery was
   **OVERALL: FAIL(1)** and the one failing step was the release sweep's F3 hit,
   adjudicated under S14 and acquitted in the log entry as measurement 38, which is
   exactly the predicate S27 admits (session D's FAIL(1)-all-F3 is the precedent).
   Everything else was green: Miri **330 / 0**, ratchet clean, census 59, both benches
   bit-identical, debug sweep 341/341.
   Tail is docs-only — cheap subset if `rust/tools/` and the toolchain are unchanged.
   Last recorded: **470 / 464 / 20**, Miri **330**, census **59**, duplicate-body budget
   **195**, goldens **57**, `raw_ptr` **4436**, `unsafe_fn` **1247**. Recount.

## 1. Face 1 — the perf debt, and it comes before any conversion

Session N is the first Phase 5 session that cost something: **+1.24% CB** at 7 pairs
against a null band of −0.14%…+0.34%, every decode row over the ceiling. Cumulative CB
≈ **+21.6…+21.9%**, headroom ≈**1.1…1.4 points** for 5.5 and 5.6. That number decides what
this session and the next may spend, so it is settled first.

Three readings, one sitting, binaries already stashed in `.perfpair/` — `n_base`
(`16a6130c`), `n_mid` (`9da4bede`), `n_head` (`d0b7f399`):

1. **The day-two confirmation** (S2b): `n_base` vs `n_head`, 7 pairs, with a fresh 7-pair
   null. This is the reading the headroom rests on.
2. **Split Face 3.** Build at `459013d8` (T5.N3, the plane mirror's death) and measure
   `n_mid → that` and `that → n_head`. The bisect already puts all of session N's cost in
   Face 3 (+1.17% CB / +2.25% Main / +2.06% High) and none in Face 1 (−0.72% CB, CABAC
   rows inside the band); this says which of T5.N3 and T5.N4 owns it.
3. **The niche, if step 2 says T5.N4.** `safe/pool.rs`'s `Id` is `{ index: u32 }` in
   release, so `Option<Id>` is eight bytes with a separate discriminant and `==` compares
   two fields. `index: NonZeroU32` holding `slot + 1` makes it one word and one compare;
   the change is in `pool.rs` and nowhere else, and §7.4's fast-by-construction clause
   covers it. **The supporting observation is the B-slice asymmetry** — Main and High cost
   roughly double CB, and T5.N4's extra work is B-slice work while T5.N3's is not.

**The hypothesis is unverified** (`perf_baseline.md` §Session N). S1 binds: disassemble
before believing it. **D-perf-4 allows one look and session N spent its on the bisect** —
if the niche does not recover it, ledger the span and go to §2. No second attempt.

## 2. Face 2 — 5.5, first half

`decoder_core.rs`: allocation → constructors, the paramset store (P4), `Drop` teardown.
Context decomposition (§2.2.6) is the second half unless the closure says otherwise —
**compute the S20 closure first and write it down**; the phase's largest remaining step is
expected to want two sessions and the split is this session's to choose.

- **S21 with force**: the decoder context embeds its buffers **by value**, so every owned
  field lands inside `mem::zeroed` reach. The `MaybeUninit` shell + `new_boxed()` is at
  `decoder_context.rs:769`; extend it per owned field, and replace it with a real
  constructor at the end of the *step*, not of this session.
- **F19's check per allocation**: *which line frees this?*
- `SWelsDecoderContext` has no `assert_size!` and no offset pins.
- **F31's redundant memset is 5.5's** and is small; take it if it is on the way.
- **F37 is 5.5's if it is anyone's**: `DestroyPicBuff` omits the C++'s opening
  `ResetReorderingPictureBuffers`, and the port's only call to it is at decoder creation,
  so an uninit/re-init cycle leaves `sPictInfoList` naming slots of a freed pool. The
  reset function is this file's.

## 3. The `pDec` question — decide it here, do not drift into it

5.1 and 5.4 are closed, but **the pool's slots are still `PPicture` and 5.3's colocated
face is blocked**, both for one reason: `pCtx->pDec` is a raw alias into a pool slot, so
`Pool::mut_and_rest` cannot be used without installing F24/F25/F28's class deliberately
(session N §5; `PicPool`'s type carries the reason).

Sizes, measured at `d0b7f399`, re-grep them anyway: **236 `.pDec` sites**, all in
`src/decoder/` — `decode_slice.rs` **77**, `decoder_core.rs` **55**,
`parse_mb_syn_cavlc.rs` 30, `deblocking.rs` 31, `parse_mb_syn_cabac.rs` 16,
`mv_pred.rs` 13, `error_concealment.rs` 12, `manage_dec_ref.rs` 2 — plus **94** decoder
`pRefList` and **209** `pRefPic` sites.

This session touches the second-largest share. **Decide, and write the decision down**:
either 5.5 takes `decoder_core.rs`'s 55 as part of its closure and 5.6 takes the rest, or
the conversion is scheduled as its own step after 5.6. Do not half-convert — session M's
disposition for the `*mut u8` caches is the precedent.

## 4. Gates

Full battery per face or tight cluster (batch before the long Miri step — ~15 minutes,
both probes); goldens frozen at **57**; sweeps 341/341 both profiles; F3 per S14 — the
signature is now **`mt` + `sm=3` + `t∈{2,4}` + any wrong length**, in either profile on
either clip, and nothing else has survived thirty-eight measurements. **Two harness traps
cost session N an isolation run each and both are in F3's measurement 37/38 notes**: pass
`compare.sh` an **absolute** yuv path (it `cd`s to the repo root) with `RUST_ENC_PROFILE`
set to the hit's profile, and take the frame count from the `out/*_loopN.yuv` that exists
(160x96 loops to 20, 320x192 to 18) — two encoders failing *identically* is a statement
about the harness, never about the trees. Ratchet per S16 with
per-file deltas, **and read them, not the total — session N's own comments inflated
`raw_ptr` by seven**; census green at **59** allowlisted with a duplicate-body budget of
**195**. **Do not edit the working tree while a battery is running.**

## 5. Close

Log entry, `perf_baseline.md` row, phase5.md §0/§5/§7 marks and plan §0's rows, hand-off.
If §1 recovers Face 3's cost, say so in the ledger and restore the headroom figure; if it
does not, the ledger entry names the family, the deficit and the phase that deletes it.

## 6. Non-goals

No window hoisting (S8). No `get_unchecked` (S8). **No F36 fix** — decoder threading's.
**No 5.3b** (279 `LD*`/`ST*` sites in `mv_pred.rs`, 21 in `parse_mb_syn_cavlc.rs`,
`SetRectBlock`/`CopyRectBlock4Cols` onto the grid) and **no colocated face** — both sit
behind §3's decision. **No `*mut u8` non-zero-count cache work** — 167 uses, 96 in
`decode_slice.rs`, 5.6's by P1. No 5.6 pulls. No golden movement. No pool/threading
(F12/P10). No re-litigating the 5.2 flip's +20.4…+20.7%; **session N's +1.24% is the only
perf number open, and §1 closes it.**
