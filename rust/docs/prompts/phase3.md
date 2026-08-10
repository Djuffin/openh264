# Safety refactor, Phase 3 (the bitstream layer)

You are starting **Phase 3** of [`safety_refactor_plan.md`](../safety_refactor_plan.md).
Phase 4a closed on 2026-08-10; **D-seq-1** is discharged and Phase 3 is next.

**Read the plan's [§0 status preamble](../safety_refactor_plan.md) first** — one
screen, maintained. Then **§7.6 Standing working rules (S1–S19)**: the durable rules
live there, this brief cites them by tag and does not repeat them, and where this
brief and §7.6 disagree, §7.6 wins. **Fuzzing is not part of this phase** — removed
by direction 2026-08-10, the plan's exit gate edited to match; the malformed-stream
parity test (§5, §T3.0 below) carries the malformed-input burden alone, and §0's
"absent instrument" row keeps the would-have-caught tally.

Expected span: **4–5 sessions**. Unlike Phase 2 this phase has no per-family rhythm —
it is two connected rewrites (decoder read side, encoder write side) plus a buffer-
ownership change, so the interruptible unit is the **seam**, and the seams are
numbered below (T3.0–T3.6). Plan session boundaries at seams, never mid-seam. A
reasonable split: session 1 = T3.0 + T3.1; session 2 = T3.2; session 3 = T3.3;
session 4 = T3.4 + T3.5; session 5 = T3.6 + exit. Merge only when a seam finishes
early and the next is small.

---

## 1. State you inherit

| | |
|---|---|
| Tests | 410 debug / 408 release / 20 ignored |
| Sweeps | 341/341 both profiles (F3 retries per **S14**; 4a's day-rate ≈1/540, historical band) |
| Ratchet | `unsafe_fn` 1346, `raw_ptr` 5171, `SHIM(` 154, `no_mangle` 24, `transmute` 23 — run `check`, never trust these numbers |
| Miri | whole library minus **three** skips, 274 tests, ~82 s; differential files at phase exits |
| Ledger | encoder ≈ **+8.9%** cumulative, decode ≈ **+17.8 / +10.1 / +9.6%**. No row `pending` |
| Parked | `common/sad_common.rs` (14), `encoder/sample.rs` SATD (7) — second dated verdict; re-attempt at Phase 6.3, **not here** |

**Read first, in this order** (each entry is load-bearing; line numbers are
2026-08-07-survey vintage — re-verify before relying on them):

1. **[`phase1_findings.md`](../phase1_findings.md) F4** — the reader's slop is a read
   *past the buffer*: the refill predicate allows the cursor 1 byte past the RBSP end
   and then loads bytes, and the initial prime reads 4 bytes from the start
   regardless of NAL length, so soundness today is a property of the 4 MiB `sRawData`
   allocation, not of the NAL. `BsCursor` reproduces the *predicate* exactly and is
   byte-identical given ≥3 bytes of slack. **T3.1 owns the guard-byte decision.**
2. **[`phase1_findings.md`](../phase1_findings.md) F5** — the canonical writer panics
   in debug on a 32-bit write into an empty accumulator (C++ UB, unreachable
   in-contract); `BsWriter` doesn't, and a Phase 1 test pins both. Don't "fix" the
   old one while deduping (S6).
3. **[`phase1_findings.md`](../phase1_findings.md) F7** — two out-of-bounds pointer
   computations in the raw reader, unreachable only via unwritten invariants. Your
   conversion deletes them; the parity test must cover the inputs that *approach*
   them.
4. **[`phase2_findings.md`](../phase2_findings.md) F2** — **the writer exists four
   times and the copies are not identical**: `vlc_encoder.rs:367` is canonical
   (matches C++ `bit_stream.h`); `svc_set_mb_syn_cavlc.rs:157` is equivalent with a
   hand-rolled 4-byte store; `nal_encap.rs:169` is equivalent with an explicit
   `iLen == 0` guard; `svc_encode_slice.rs:509` **diverges** — null/`iLen <= 0`
   early-returns, pre-masks `kuiValue` to `iLen` bits, inverts the branch sense, and
   uses `wrapping_add` in `BsWriteUE`. All four agree on in-contract inputs (that's
   why 341/341 holds). **T3.4 dedupes before converting**, and the dedupe commit
   documents which guard semantics die.
5. **[`phase2_findings.md`](../phase2_findings.md) F13, the `InitBits` site** —
   `vlc_encoder.rs:353` takes `kpBuf: *const u8`, stores it as `pStartBuf: *mut u8`,
   and the writer writes through it: a signature that lies about mutability, making
   **every honest caller UB**. `au_set.rs`'s two tests carry the accommodation
   (`as_mut_ptr() as *const u8`, commented). **`BsWriter` is where this signature
   dies (T3.4), and deleting the accommodation afterwards is a required gate
   strengthening** — 4a's precedent: the last skip deleted exposed F14 immediately.
6. **Plan §2.2.2** — `BsCursor`/`BsWriter` exist, unit-tested, Miri-clean, and
   differential-proven since Phase 1, with three **[P1]** deviations from the sketch
   already flagged in place. **You are adopting an API, not designing one.** If
   adoption reveals a real gap, extend it in `src/safe/bits.rs` under its
   `forbid(unsafe_code)` and its test discipline — don't fork a local variant.
7. **Plan §7.4 v3** (D-perf-4) and §Phase 4a's finding (repeated in §4 below), then
   this brief end to end.

## 2. What "conversion" means in this phase — ownership, not wrapping

Phase 2's unit was the leaf function: same signature outside, safe body inside, shim
at the boundary. **Phase 3's unit is a struct field.** `SBitStringAux` is a
pointer-triple cursor (`pStartBuf`/`pCurBuf`/`pEndBuf` + `uiCurBits`/`iLeftBits`)
embedded *by value* in things you don't own yet (`SWelsDecoderContext::sBs`, the
NAL-level readers, `SWelsSliceBs`, `SSlice` via `pSliceBsa`). Conversion means the
**position moves into a detached cursor** (`BsCursor`/`BsWriter`: offsets only, no
buffer reference — plan §2.1.3) **and the buffer stays with its owner**, passed as
`&[u8]` / `&mut [u8]` per call. Three consequences:

- **Compat shells.** Where a struct you are *not* converting this phase embeds or
  points at `SBitStringAux` (`SDqLayer::pBitStringAux` — Phase 5;
  `SSlice::pSliceBsa` / `SWelsSliceBs` internals — Phase 6), the struct keeps a
  shell: the `SBitStringAux` stays as a field, and a small, marked sync layer
  (`SHIM(phase3)`) translates cursor↔shell at the conversion boundary. Shells are
  debt with named owners; each one gets a one-line contract saying which phase
  deletes it. Budget real time for them — they are this phase's equivalent of
  Phase 2's shims, and the ratchet treats them the same way (R-f shape: `SHIM(`
  rises, `raw_ptr` falls where pointer cursors die).
- **No unswap lever.** A struct conversion has no commit-B-revert the way a kernel
  family did. The fallback ladder for a seam that lands badly is: one-hour look
  (S1: profile + disassembly, no boxes) → accept-and-ledger under the tripwire
  arithmetic → **revert the seam's commits** and re-design the seam at Phase 5's
  static-dimension vantage (4a's finding, §4). State which rung you took in the
  log. The tripwire (+25% median cumulative) is judged per D-perf-4 as always — and
  note decode CB is at **+17.8%**, so the whole phase's decode allowance is ~7
  points. §4 says why that should be enough; measure per seam, not per phase.
- **Differential tests retire into properties.** Phase 1's differential files prove
  `BsCursor`/`BsWriter` against the *raw* reader/writer. As each seam deletes the
  raw side, the corresponding differential entries convert to golden/property tests
  (Phase 2's commit-B pattern): the parity corpus (T3.0) plus the Phase 1 unit
  tests become the permanent spec. Do the retirement in the same commit that
  deletes the raw code, and say so in the message.

## 3. The work, seam by seam

### T3.0 — The malformed-stream error-code parity test (session 1, before any conversion)

**This is the phase's real gate, it is worthless written after the conversion it
judges, and — with fuzzing out — it is the only malformed-input instrument the
phase has.** Build it against the **unconverted** reader so it pins today's
behaviour, commit it green, then never let it move.

Shape:
- A dedicated integration test (`tests/malformed_stream_parity.rs` or similar)
  driving the decoder through the same C-API flow as `decoder_conformance_test.rs`
  (create → `Initialize` with `ERROR_CON_SLICE_COPY` → per-NAL `DecodeFrame2` → EOS
  drain → destroy).
- **Corpus, generated deterministically in the test** (no checked-in binary blobs
  beyond what `res/` already provides):
  - every conformance stream in the gate set, truncated at **systematic offsets**:
    every byte length within ±8 of each NAL boundary, and a coarse sweep (every
    N bytes) across the stream body — the F4 slop geometry means off-by-one
    truncations at the refill boundary are exactly the class that must not shift;
  - EPB (emulation-prevention) edges: truncation between `00 00 03` and its
    following byte; a stream ending in `00 00`; a `00 00 03` as the final bytes;
  - degenerate NALs: zero-length payload after the start code, start-code-only
    input, empty input, a lone SPS, an SPS/PPS pair with the slice truncated at
    byte 1;
  - header-corrupt variants: `nal_ref_idc`/`nal_unit_type` bytes flipped on an
    otherwise-valid stream (exercises the early-exit paths).
- **Assertions, per corpus entry**: the exact `DecodeFrame2` return
  (`DECODING_STATE` bits, not "nonzero"), the `dsOut` buffer-status/got-picture
  outcome, the **decoded-frame count** across the whole sequence (S13-class:
  frame counts before hashes), and per-plane hashes for whatever frames *are*
  produced. Record the expectations as a generated golden table (checked in,
  human-diffable) rather than hand-written constants — regenerating it must be a
  deliberate act with a diff to review.
- **No-panic is part of the assertion** — any panic today is a pre-existing P13
  item: record it as a finding (S12), `#[should_panic]`-quarantine that entry with
  the finding tag, and move on. Do not fix the old reader.

### T3.1 — `bit_stream.rs` + `dec_golomb.rs`: the CAVLC read side (seam)

Inventory first (`grep -n "pub.*fn" decoder/bit_stream.rs decoder/dec_golomb.rs`,
plus every `SBitStringAux` field access outside them). Known anchors:
`DecInitBits`/`InitReadBits` (two init variants — check whether their initial-fill
semantics differ before unifying), `GetValue4Bytes`, the refill `dump_bits_aux`
(`dec_golomb.rs:128-150` — the slop predicate `iAllowedBytes + 1` and the 2-byte
load), the ue/se/bit readers layered on it, and `rbsp_to_ebsp` (already safe).

The conversion:
- `SWelsDecoderContext::sBs: SBitStringAux` → `BsCursor` + the buffer reference
  expressed at call sites (`&ctx.raw_data[...]` — until T3.3 lands, the buffer is
  still `sRawData`'s pointers, so this seam reads through a **temporary slice
  reconstruction helper** at the boundary, marked `SHIM(phase3)` and deleted by
  T3.3; write it once, like T3/T4's span helpers, S-style: one place owns the
  arithmetic).
- **The F4 guard-byte decision, made explicitly here and written down** (options
  from plan P6): (a) append ≥3 zero guard bytes to the raw-data buffer at fill
  time, keeping the slop predicate verbatim — deterministic, in-bounds,
  byte-identical given the F4 analysis; or (b) express the overrun loads through
  `buf.get()` fallbacks. Phase 1's differential work says (a) matches the C++
  exactly and (b) is identical *from the caller's view* on every probed input; the
  parity corpus (T3.0) is the referee. Whichever you pick: record it as the P6
  resolution in the findings file and the plan, with the parity run as evidence.
- `pEndBuf`-arithmetic bounds checks (`iAllowedBytes` etc.) become `len`/`pos`
  arithmetic with the **same predicates** — resist the urge to "clean up" an
  off-by-one that is load-bearing (F4).
- Callers: `dec_golomb`'s readers are called from parameter-set parsing, slice
  headers, CAVLC residuals — the call sites keep their shapes; only the
  `(pBs)` argument becomes `(buf, &mut cursor)`. Where a caller is a struct you
  don't own (e.g. slice-header parsing writing into `SDqLayer`-adjacent state),
  shell per §2.

Gate the seam: full battery + T3.0 green + one interleaved pair both benches
(decode is the one that matters here) + ratchet shape.

### T3.2 — `cabac_decoder.rs`: the arithmetic-coding engine (seam)

`SWelsCabacDecEngine { pBuffStart, pBuffCurr, pBuffEnd, uiRange, uiOffset,
iBitsLeft }` → `{ pos: usize, range: u64, offset: u64, bits_left: i32 }` over the
same buffer the CAVLC cursor reads.

Specifics that must survive exactly:
- **Init** (`cabac_decoder.rs:676-697`): derives its start by *rewinding* the CAVLC
  cursor (`pCurBuf.offset(-(iRemainingBytes))`), sets an end guard at
  `pEndBuf - 1`, then primes 5 bytes. All three become offset arithmetic against
  the shared buffer — assert the rewind can't underflow (it can't today by
  construction; make the reason a debug_assert with a comment, not a silent
  subtraction).
- **The end ladder** (`:732-784`): `offset_from` distance selects a 4/3/2/1-byte
  final load — becomes `len - pos` and the same ladder, byte-for-byte. This is the
  CABAC end-of-slice byte counting the plan's risk note names; the truncated CABAC
  entries in T3.0's corpus are its test.
- **The CAVLC↔CABAC handoff** (`:712-716`): the engine rewinds by `iBitsLeft >> 3`
  and writes its position back into the CAVLC cursor — becomes one `usize`
  assignment. This is the moment both readers demonstrably share one position
  space; a unit test pinning a CAVLC→CABAC→CAVLC round-trip at a known bit offset
  is cheap and permanent.
- **Perf, before not after** (S1): `DecodeBinCabac` measured **544 self-samples in
  4a's profile — the largest single decode consumer after the bench's own
  hashing**. Disassemble the converted hot path before calling the seam done:
  the state loads/stores must stay in registers across the renorm loop, and any
  bounds check inside the per-bin path is a defect to restructure (S8 idioms;
  `get_unchecked` stays banned). Expect this seam to be where the phase's decode
  budget is won or lost; measure the pair immediately after it lands.

### T3.3 — `SDataBuffer` → owned buffer + `nalu.rs` ranges: the ownership seam

- `SWelsDecoderContext::sRawData/sSavedData: SDataBuffer {pHead,pEnd,pStartPos,
  pCurPos}` → an owned `RawDataBuffer { buf: Vec<u8>, start: usize, cur: usize }`
  (guard bytes per T3.1's decision live here, appended at fill).
- **`ExpandBsBuffer`'s pointer-rebasing block (`decoder_core.rs:1758-1842`) is
  deleted, not converted** — offsets survive realloc by definition. Add the unit
  test the plan (P5) asks for: grow the buffer mid-AU, assert parse continuity —
  the latent-bug class offsets fix silently should be pinned loudly.
- `nalu.rs`: `SVclNal.pNalPos: *mut u8` and every NAL payload pointer →
  `Range<usize>` into the raw buffer; `DetectStartCodePrefix`/`DecodeNalHeaderExt`
  take slices (`DecodeNalHeaderExt` today has **no length parameter at all** —
  it gains one by construction); the `BsGetTrailingBits(pNal.add(len - 1))`
  underflow-on-zero-length sites become checked indexing that the parity corpus's
  degenerate NALs exercise.
- This seam deletes T3.1's temporary slice-reconstruction shim and most of the
  decoder's remaining bitstream `raw_ptr` count. Expect the phase's biggest
  single ratchet drop here; say the numbers in the log.

### T3.4 — The encoder write side, part 1: dedupe, then `BsWriter` (seam after each)

**Dedupe first (F2), as its own commit.** Route `svc_encode_slice.rs`,
`nal_encap.rs`, and `svc_set_mb_syn_cavlc.rs`'s callers onto `vlc_encoder.rs`'s
canonical family; delete the three copies; the commit message records the
guard-semantics decision (canonical wins; the divergent copy's pre-masking and
early-returns die — they were unreachable in-contract, which 341/341 continues to
prove). Sweeps both profiles are the referee; this commit changes zero bytes of
output or it reverts.

**Then `vlc_encoder.rs` → `BsWriter`:**
- `InitBits`'s lying signature dies with the struct (F13 site): the writer's buffer
  is owned by its slice/NAL owner and passed `&mut [u8]`; `BsWriter` is the
  position. **Delete `au_set.rs`'s two accommodations in the same commit** and let
  Miri see the honest thing — that deletion is a named deliverable of this phase.
- `WRITE_BE_32` + `pCurBuf.add(4)` → the accumulator flush against `buf[pos..pos+4]`
  (canonical semantics; slice indexing supplies the bounds the C never had — a
  panic here is a pre-existing sizing bug surfacing, per plan §2.2.2).
- `BsGetBitsPos` → `bits_pos()`; the `pEndBuf - pCurBuf - 1` space checks
  (`svc_set_mb_syn_cavlc.rs:752`-class) → `len - pos - 1` with identical
  comparisons.
- Callers that hold `SBitStringAux` inside encoder structs (`SWelsSliceBs.sBs`,
  reached via `SSlice::pSliceBsa`) get shells per §2 — the slice-writing internals
  convert; the struct layout waits for Phase 6.

### T3.5 — The encoder write side, part 2: CABAC triple + rollback (seam)

- `set_mb_syn_cabac.rs`'s independent cursor triple (`m_pBufStart/m_pBufCur/
  m_pBufEnd`, `:139-143`, walks at `:825`, `:839-860`, `:1009-1028`) → `pos` over
  the slice buffer, same arithmetic.
- `svc_set_mb_syn_cavlc.rs`'s dynamic-slice rollback (`StashMBStatus`/
  `StashPopMBStatus`, `:1057-1076`): `pBsStackBufPtr`/`pRestoreBuffer` snapshots →
  **a `Copy` of the `BsWriter`** (and of the CABAC pos where stashed) held in
  `SDynamicSlicingStack` — the pointer fields become plain values, which is the
  whole point of detached cursors. The MT path exercises this under `sm=3`; expect
  F3's signature in the sweeps and apply S14 without flinching.

### T3.6 — `nal_encap.rs` + phase exit

- `SWelsNalRaw.pRawData`, `SWelsEncoderOutput.{pBsBuffer,sNalList,pNalLen}`,
  `SWelsSliceBs.{pBs,pBsBuffer}` → owned `Vec`s/ranges with their `CMemoryAlign`
  allocations retired (the free-cascade entries for them die too — R4 in
  miniature).
- **Exit**: full battery + 3-pair medians both benches + T3.0 green + the Miri run
  with the `au_set` accommodations gone + differential-retirement complete +
  ratchet (expect `raw_ptr` down by hundreds; `SHIM(phase3)` counted and each
  shell's owner named) + ledger/§0 updates + **S19**: write `prompts/phase4b.md`
  (config-dispatch enums `pfWelsSpatialWriteMbSyn`/RC table — *now* safe to touch
  since the writer is one; the strategy vtables; 4a's leftover de-virtualization
  scope) and stamp this brief historical.

## 4. What Phase 4a learned that you must apply

**Direct dispatch recovers per-call scaffolding only where the caller supplies
constant dimensions** — encoder MC folded because call sites name literal sizes;
`BaseMC`'s runtime parameters folded nothing. The bitstream layer is the same shape
in miniature: **`get_bits(n)` with a literal `n` is the overwhelmingly common call**
in this codebase, and it must *stay* literal through your abstraction — a wrapper
that launders a literal into a runtime argument is the regression, and `#[inline]`
on the cursor methods is what lets the fold happen. Check the hot paths' disassembly
(T3.2 especially) rather than assuming.

Instrument notes, paid for in 4a: `rust/tools/perfpair.py` implements S1/S2/S17
(`build <label>`, `run <A> <B>`, `null <label>`) — use it, don't rebuild the
protocol; and **address comparison is not a sound assert-map technique** (inlined
fns get per-CGU addresses and Miri mints synthetic ones) — prove table/dispatch
equivalences by behaviour, per `common::mc::tests`.

## 5. Gates

Per **S14–S17** unchanged, plus this phase's own: **T3.0 green from session 1
onward** (it is the malformed-input instrument — fuzzing removed by direction, exit
gate edited to match); **one interleaved pair per bench per seam** (decode headroom
is ~7 points on CB — watch it seam by seam, don't discover it at exit); **the
`au_set.rs` accommodation deletion** with a green honest Miri run (T3.4); full
3-pair medians at exit only. Ratchet per R-f's shape with `SHIM(phase3)` shells
counted and owned.

## 6. Non-goals

No fuzzing (removed by direction — do not "just quickly" stand it up; the decision
and its reversal path live in the plan's exit-gate note). No Phase 4b work — the
entropy-selection enum and RC table are 4b's *because* they touch these files;
finish the writer first, enum after, per the fence. No structural rewrites
(`SDqLayer`, `SMbCache`, picture pool, `SSlice` layout — Phases 5/6; shells instead).
No re-opening parked families (second dated verdict; Phase 6.3). No fixing
F8/F9/F11-class arithmetic (**S6**), no repairing the old reader's panics found by
T3.0 (record, quarantine, convert past them). No `get_unchecked`, ever (**S8**).
And the standing temptation warning, third phase running: the seams are ordered so
the tree stays green — do not reorder them because one looks quicker.
