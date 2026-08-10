# Safety refactor, Phase 3 (the bitstream layer)

You are starting **Phase 3** of [`safety_refactor_plan.md`](../safety_refactor_plan.md).
Phase 4a closed on 2026-08-10; **D-seq-1** is discharged and Phase 3 is now next.

**Read the plan's [§0 status preamble](../safety_refactor_plan.md) first** — it is one
screen and it is maintained. Then **§7.6 Standing working rules (S1–S19)**, which is
where the durable rules live. This brief cites them by tag and does not repeat them;
where this brief and §7.6 disagree, §7.6 is the rule and this brief is wrong.

Expected span: **4–5 sessions**. Unlike Phase 2 this phase has no natural per-family
rhythm — it has two halves (decoder read, encoder write) that are each one connected
rewrite, so the interruptible unit is coarser. Plan session boundaries at the seams
listed in §3, not wherever the clock runs out.

---

## 1. State you inherit

| | |
|---|---|
| Tests | 410 debug / 408 release / 20 ignored |
| Sweeps | 341/341 both profiles (F3 retries per **S14**; the day-rate was ≈1/540 on 4a's day, back in the historical band) |
| Ratchet | `unsafe_fn` 1346, `raw_ptr` 5171, `SHIM(` 154, `no_mangle` 24, `transmute` 23 — run `check`, never trust these numbers |
| Miri | whole library minus **three** skips, 274 tests, ~82 s; plus the differential files at phase exits |
| Ledger | encoder ≈ **+8.9%** cumulative, decode ≈ **+17.8 / +10.1 / +9.6%**. No row `pending` |
| Parked | `common/sad_common.rs` (14), `encoder/sample.rs` SATD (7) — re-parked with a second dated verdict |

**Read first, in this order** — this list is staged and each entry is load-bearing:

- **[`phase1_findings.md`](../phase1_findings.md) F4, F5, F7** — the decoder read side.
  F4 is the slop (the reader legitimately reads three bytes past the RBSP and primes
  four from the start); F5 is the canonical writer's debug-only panic on a 32-bit
  write into an empty accumulator; F7 is what Miri found in the raw reader.
- **[`phase2_findings.md`](../phase2_findings.md) F2** — the encoder write side, and
  the reason it is not one file: **the writer exists four times**. `vlc_encoder.rs`
  is canonical, `svc_encode_slice.rs:498-585` is a verbatim copy, and two more
  disagree about guard/masking/wrapping semantics. **Dedupe before converting**
  (§2.2.2), and per **S18**'s corollary, "converted" is a per-consumer claim.
- **[`phase2_findings.md`](../phase2_findings.md) F13's `InitBits` site** —
  `vlc_encoder.rs:353` declares `kpBuf: *const u8`, stores it as `pStartBuf: *mut u8`,
  and the writer writes through it. **Every honest caller is wrong**: a caller that
  passes `as_ptr()` produces a pointer with no write provenance and the first
  `BsFlush` is UB. `au_set.rs`'s two tests carry an explicit accommodation
  (`as_mut_ptr() as *const u8`) rather than the signature being fixed, because fixing
  it is *this phase's* job. **`BsWriter` is where that signature dies.**
- **§2.2.2** of the plan — `BsCursor`/`BsWriter` are already built, unit-tested and
  Miri-clean from Phase 1, with three **[P1]** deviations from the original sketch
  already flagged and explained. You are adopting an existing API, not designing one.

## 2. Why this phase is different from Phase 2, and what that changes

Phase 2 converted ~190 leaf kernels: thousands of call sites, trivial contracts, and
a strangler shim per family so every step was independently gateable. **Phase 3 has
the opposite shape** — few call sites, deep contracts, and state that is threaded
through the whole parse/write path. Three consequences:

1. **The shim boundary is a struct, not a function.** `SBitStringAux` stays as a
   compat shell wherever unconverted structs still embed it (`SDqLayer::pBitStringAux`
   waits for Phase 5); conversion means changing who *owns* the position, not
   wrapping a call. Budget for shells, and delete them in Phase 5/6 as §5.3 says.
2. **Error-code parity is the gate, not just byte-exactness.** The decoder's malformed
   -stream behaviour is expressed through exactly *where* the cursor is allowed to sit
   past the end (F4, and `dec_golomb.rs:128-150`'s deliberate 1-byte-past read). A
   conversion that is byte-exact on well-formed streams and shifts an error code on a
   truncated one has failed. The phase's exit gate names a dedicated malformed-stream
   parity test for this reason — **write it early, not at the exit**.
3. **`get_unchecked` is still banned and the fast-idiom catalog still applies**
   (**S8**), but the hot loop here is `DecodeBinCabac`, which the Phase 4a profile put
   at **544 self-samples — the largest single decode consumer after the benchmark's
   own hashing**. It is worth a disassembly look *before* you convert it rather than
   after (**S1**).

## 3. The work, and the session seams

Plan §5 Phase 3 has the full list. The order below is the one whose seams leave the
tree green:

1. **`bit_stream.rs` + `dec_golomb.rs`** — `BsCursor` adoption, and the F4 guard-byte
   decision (P6) made explicitly and written down. Seam.
2. **`cabac_decoder.rs`** — the offset engine, and the CAVLC↔CABAC handoff at
   `cabac_decoder.rs:712-716`, which becomes an assignment of one `usize` to another.
   Seam.
3. **`SDataBuffer` → `RawDataBuffer` + `nalu.rs` payload ranges** — this is where
   `ExpandBsBuffer`'s pointer-rebasing block (`decoder_core.rs:1816-1842`) is
   **deleted, not converted**; offsets survive realloc by definition. Seam, and the
   most satisfying one.
4. **Encoder write side**: dedupe F2's four writers *first*, then `vlc_encoder.rs` →
   `BsWriter` (killing F13's `InitBits` signature), then `set_mb_syn_cabac.rs`'s
   cursor triple, then `svc_set_mb_syn_cavlc.rs`'s rollback snapshots — which become
   a `Copy` of a `BsWriter` — and its `pEndBuf - pCurBuf` space checks, which become
   `len - pos`. Seam after the dedupe, seam after `vlc_encoder.rs`.
5. **`nal_encap.rs`** owned buffers.

## 4. What Phase 4a learned that you should apply

**The one finding that transfers.** Direct dispatch recovers per-call scaffolding
*only where the caller supplies constant dimensions* — the encoder's MC sites
recovered ~5%, the decoder's recovered nothing because the sizes arrive as parameters.
The bitstream layer is the same shape: a `BsCursor` method whose caller knows the bit
count statically will fold; one that takes a runtime `n` will not. **`get_bits(n)`
with a literal `n` is the common case in this codebase** — check that it stays literal
through your abstraction, and if a wrapper turns a literal into a runtime argument,
that wrapper is the regression.

**Two instrument notes**, both paid for in 4a:

- `rust/tools/perfpair.py` now implements S1/S2/S17. `build <label>`, `run <A> <B>`,
  `null <label>`. Use it rather than rebuilding the protocol by hand. Its own bug is
  worth knowing: the first draft parsed bench output by *field offset* and silently
  dropped every row over 10 ms — six encoder rows — while printing a plausible table.
  It now parses by regex and reports missing rows explicitly.
- **Address comparison is not a sound assert-map technique.** `#[inline(always)]`
  functions are instantiated per codegen unit that takes their address, and Miri mints
  a fresh synthetic address per cast. If you need to prove a table holds a given
  function, split it: flag-invariance by comparing two tables from the *same*
  installer, and identity by comparing *behaviour*. See `common::mc::tests` and
  `mc_table_slots_match_the_direct_calls`.

## 5. Gates

Per **S14**, **S15**, **S16**, **S17** — unchanged. Two additions that are yours:

- **The malformed-stream error-code parity test is a deliverable, not a gate you
  inherit.** Write it in session 1 against the *unconverted* reader so it pins current
  behaviour, then keep it green.
- **This phase should delete a Miri skip too.** `--skip encoder_ext` is F13's
  `InitDqLayers` site and is Phase 6's, but F13's **`InitBits`** site is yours, and it
  is the one the skip list does not currently cover because the tests accommodate it.
  When `BsWriter` lands, delete the accommodation in `au_set.rs`'s two tests and let
  Miri see the real thing. **Removing an accommodation is a gate strengthening and
  needs no ceremony beyond the run being green** — and 4a is the precedent for what
  it buys: deleting one skip immediately exposed F14, production UB that had been
  sitting behind it.

The perf gate is D-perf-4's: one interleaved pair per bench per step, full 3-pair
medians at the phase exit. Note the asymmetry — this phase touches the **decode**
side hardest, and decode is the bench that did *not* recover in 4a and is carrying
≈ +17.8% on Constrained Baseline. There is less headroom here than the encoder's
+8.9% suggests, and the tripwire (+25% median cumulative) is closer than it looks on
the CAVLC stream.

## 6. Non-goals

No Phase 4b work — the config-dispatch enums (`pfWelsSpatialWriteMbSyn`, the RC-mode
table) and the strategy vtables come *after* this phase, by the fence 4a held. No
structural rewrites: `SDqLayer`, `SMbCache` and the picture pool are Phases 5 and 6.
No fixing F8/F9/F11-class arithmetic (**S6** is parity, not repair). No re-opening the
parked families — they have a second dated verdict and their next re-attempt point is
caller conversion in Phase 6.3.

**One explicitly deferred item you may be tempted by:** 4a cut the decoder intra-pred
arrays, `sBlockFunc`, expand, and the `decode_slice.rs` cache-fill transmutes, so the
`transmute` count is still 23. That is Phase 4b's or a later 4a-remainder pass — it is
*not* Phase 3's, and pulling it forward will tangle the bitstream conversion with
dispatch churn in the same files.
