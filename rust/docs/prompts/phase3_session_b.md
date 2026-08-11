# Session prompt — Phase 3, session B: T3.1b (the CAVLC mode + the ownership move), then T3.2 if it truly fits

**Governing documents:** [`phase3.md`](phase3.md) (the phase brief — §2's shell
policy and fallback ladder, §4's constant-dimension rule, §5's gates), plan **§7.6**
(standing rules, cited by S-tag), plan **§2.2.2** including the two **[P3]** notes
(the CAVLC-mode decision and P6's real resolution — both made 2026-08-10 and both
binding on this session). This file scopes the session; where it disagrees with
those, they win, and part of your job is to fix the disagreement in place.

**Session goal:** finish seam T3.1 — T3.1a landed in session A (`96fb04a4`); T3.1b
is the other half. **T3.2 (the CABAC engine) starts only if T3.1b is fully gated
with at least a third of the session left** — it is the phase's perf-critical seam
(`DecodeBinCabac`) and deserves an unhurried start. Beginning it late to "make
progress" is the order-deviation trap three phases have now logged; don't.

---

## 0. Open the session

1. `git status` — you likely inherit uncommitted doc edits (the [P3] decisions in
   the plan + the updated `phase3.md`). Commit them as inherited, house style
   (`docs: T3.1b unblocked — BsCursor gains a CAVLC mode ([P3]); P6's real
   resolution recorded; F15 closure specified`), before anything else.
2. Control battery: `gates.sh` green (T3.0's parity test is part of it now —
   **2316 rows, byte-identical both profiles**, and its `WITHHELD` rows naming F15
   stay withheld until T3.3; if any *other* row moves at any point this session,
   stop and revert, that's the gate doing its job), ratchet `check`, and note the
   session-A numbers you measure against: tests 412/410/20-ish (recount — session A
   added the parity test and mode tests will add more), decode pair baseline
   carrying T3.1a's **+0.45% marshalling, which this session should delete** —
   measure it gone and say so in the log, or say why not.

## 1. T3.1b step 0 — the CAVLC mode in `src/safe/bits.rs`, proven before any consumer moves

The [P3] decision, restated as implementation spec. `iIndex` is an absolute bit
position — a second representation of the cursor that is authoritative while the
accumulator is deliberately stale between `BsStartCavlc` and `BsEndCavlc`
(`parse_mb_syn_cavlc.rs:2229-2249`; read them first, they are 20 lines).

**Fields and methods:**

- `cavlc_bit_pos: isize` — new field on `BsCursor`. `isize`, not `usize`: parity
  with the port's `iIndex: isize`, and the arithmetic below is signed by design.
  Default/init value `0`.
- `start_cavlc(&mut self)` — exactly
  `self.cavlc_bit_pos = ((self.pos as isize) << 3) - (16 - self.left_bits as isize);`
  This is the 16-bit half-window convention: **the `16` is not a typo for `32`** —
  the CAVLC residual machinery works a 16-bit window and the C++ constant is 16.
  Copy the expression, don't rederive it.
- `end_cavlc(&mut self, buf: &[u8])` — exactly the raw `BsEndCavlc`:
  `pos = (idx >> 3) as usize`; prime 4 bytes big-endian from `buf[pos..pos+4]`
  (in-bounds by the `READER_SLOP = 3` regime T3.1a established — the same slack
  that makes every other prime legal; if indexing ever fails here it is a real
  pre-existing overrun surfacing, not your bug); `cur_bits = cache << (idx & 7)`;
  `pos += 4`; `left_bits = -16 + (idx & 7) as i32` — **negative on purpose**.
  `left_bits` stays `i32`; do not "fix" the sign.
- **The debug coherence guard:** `#[cfg(debug_assertions)] in_cavlc: bool`.
  `start_cavlc` sets it, `end_cavlc` clears it; every accumulator op
  (`get_bits`/`get_ue`/`get_se`/`peek_bits`/…) `debug_assert!(!self.in_cavlc)`;
  the `cavlc_bit_pos` accessor pair `debug_assert!(self.in_cavlc)`. Release builds:
  zero cost, zero behavior. This is the desync class the C++ could never detect,
  made loud.
- **`PartialEq`/`Copy` care:** `BsCursor` derives `Copy + PartialEq` and the parity
  tests compare cursor states. Write `PartialEq` **manually** over the six
  C-mirrored fields (`pos, cur_bits, left_bits, len, bits, cavlc_bit_pos`) and
  exclude the `cfg`-gated flag — otherwise equality differs between profiles,
  which is exactly the kind of skew S16's dual-profile discipline exists to
  prevent. Note the manual impl's reason in a comment.

**Proof, in the same commit, before any consumer converts:**

- In-module unit tests (safe-only, `forbid`-compatible): round-trips at PRNG bit
  positions (`start → end` restores a state that continues reading identically to
  a cursor that never entered the mode — drive both forward and compare outputs);
  the −16 window arithmetic at byte-aligned and each of the 7 unaligned phases;
  mode-guard panics under `#[cfg(debug_assertions)]` `#[should_panic]` tests.
- Differential tests (in the existing `tests/safe_bits_differential.rs`, which may
  use unsafe): PRNG cursor states pushed through raw `BsStartCavlc`/`BsEndCavlc`
  vs the safe pair, **full state compared** (all six fields), including states
  near the buffer end where the 4-byte prime leans on the slop — sized per the
  F10 rule (`(h+1)*stride` was the kernel form; here it means raw-side buffers
  carry the real `READER_SLOP` slack, exact-span only on the safe side).
- Miri on both (S16's protocol; the differential file runs the raw code under
  Miri — if Miri objects to the *raw* pair on some state, that is a finding per
  S12, accommodated in the test and recorded, not fixed).

## 2. T3.1b steps 1–3 — the ownership move

**Inventory before editing** (S18's corollary applies in miniature: "converted"
is a per-consumer claim): `grep -n "sBs\b" decoder/`, `grep -rn "sSliceBitsRead"`,
`grep -rn "pBitStringAux"` — bucket every hit into the three access paths below,
and put the bucketed list in the log entry. Expect ~20 consumer *functions*;
session A counted them, recount anyway (the recount rule has paid out five times).

1. **`SWelsDecoderContext::sBs: SBitStringAux` → `BsCursor`.** The buffer side:
   until T3.3 replaces `SDataBuffer` with an owned `Vec`, the bytes still live
   behind `sRawData`'s pointers — so this seam reads through **one** boundary
   helper (`fn raw_data_slice(ctx: &…) -> &[u8]`, or the minimal equivalent)
   that reconstructs the slice `pHead..pEnd + READER_SLOP` exactly once, is
   marked `SHIM(phase3)`, documents why the slop bytes are legal
   (`decoder_core.rs:3637`'s allocation guarantee — quote it), and is **deleted
   by T3.3**. No other site does that pointer arithmetic; if a second site needs
   it, it calls the helper.
2. **`SVclNal::sSliceBitsRead: SBitStringAux` → `BsCursor`.** Same story; the NAL's
   payload identity stays pointer-based until T3.3 turns it into ranges — the
   helper covers it.
3. **The consumers** move from `(pBs: *mut SBitStringAux)` to
   `(buf: &[u8], cursor: &mut BsCursor)`:
   - *Direct users* (NAL/SPS/PPS/slice-header parsing through `ctx.sBs` or the
     NAL's reader): convert outright.
   - *The CAVLC residual path* (`BsStartCavlc`/`BsEndCavlc` and the readers
     between them): convert **mechanically onto the mode** — call sites swap the
     raw pair for `cursor.start_cavlc()` / `cursor.end_cavlc(buf)` and the
     `iIndex` reads for the accessor. Their deeper restructure is Phase 5's; do
     not reshape the residual loop.
   - *Storage in Phase-5 structs*: `SDqLayer::pBitStringAux: *mut SBitStringAux`
     is the shell point. **Default shape (deviate only with a written reason):
     retype the field to `*mut BsCursor`** — the pointer stays (its removal is
     Phase 5's, ratchet-neutral now), the pointee is the converted cursor, and
     the deep files (`decode_slice.rs`, `parse_mb_syn_*.rs`) keep their access
     *pattern* while their call expressions pass `(buf, &mut *cursor_ptr)`
     — call-expression edits in Phase-5 files are fine (Phase 2 did exactly
     this); struct/logic reshaping is not. If this shape turns out worse than a
     mirror-and-sync shell during implementation, take the shell, mark the sync
     boundary `SHIM(phase3)`, and record the deviation in the log and the plan.
4. **Delete what dies**: with `sBs` and `sSliceBitsRead` converted, the raw reader
   entry points from T3.1a's unchanged-signature layer lose their reason to be
   `unsafe fn(pBs)` — collapse the T3.1a marshalling layer (that's where the
   +0.45% goes), retire the corresponding differential entries into the
   round-trip/property tests per §2 of the phase brief, and let the compiler
   enumerate stragglers. `SBitStringAux` itself stays defined (the encoder side
   and the shells still use it until T3.4–T3.6/Phase 5) — note in its doc comment
   which phase deletes it.

**Behavior invariants for the whole step:** zero golden movement (T3.0's table is
regenerated **only** at T3.3 for F15 — this seam must not touch it), all 53
conformance hashes, frame counts, the 20 ignored, sweeps 341/341 both profiles.
This is a pure ownership/plumbing change; any output difference is a bug.

## 3. Gates for the seam (T3.1 closes here)

Full battery + T3.0 green-and-unmoved + one interleaved pair both benches
(`perfpair.py`; decode is the live one — **expect ≈ −0.45% recovering T3.1a's
marshalling**, and if instead it *costs*, apply §4 of the phase brief: check that
literal bit-counts stayed literal through the new call shape before anything
else) + Miri (`--lib safe::` now includes the mode tests; the differential file
run) + ratchet: `SHIM(phase3)` counts the boundary helper and any shell sync;
`raw_ptr` should fall (dead `SBitStringAux` plumbing) — R-f shape judgment,
prose-counting wrinkles included. Log entry per the standing exit protocol with
the consumer bucket list, then check T3.1 off in the plan's Progress appendix.

## 4. T3.2 — only with a third of the session left, and from a standing start

The phase brief's T3.2 section is the spec (engine fields → offsets; init's
rewind with a can't-underflow debug_assert; the 4/3/2/1 end ladder as `len - pos`;
the handoff at `cabac_decoder.rs:712-716` becoming one `usize` assignment plus a
pinning round-trip test). Two session-B-specific notes:

- **S1 before conversion, not after:** `DecodeBinCabac` was 4a's largest single
  decode consumer (544 self-samples). Disassemble the *current* raw hot path
  first so you know what "as good" looks like, then keep the converted engine's
  state in registers across the renorm loop. Any bounds check inside the per-bin
  path is a restructure-now defect (S8 idioms; `get_unchecked` stays banned).
- The engine reads the same buffer through the same T3.1b boundary helper —
  do not introduce a second slice-reconstruction site; T3.3 wants exactly one
  thing to delete.

If T3.2 doesn't fit: bookkeeping, hand-off naming it session C's first action,
clean stop at the seam. That is a full, successful session.

## 5. Non-goals

Everything in `phase3.md` §6, plus session-scoped: no T3.3 pulls (the helper
dies *there*, not early; F15 stays WITHHELD until then); no encoder-side work
(T3.4+ — the writer dedupe is its own seam with its own commit discipline); no
residual-path restructuring (Phase 5's); no golden regeneration; no `SBitStringAux`
deletion; no new instruments. The CAVLC mode is built in `src/safe/bits.rs` under
its `forbid(unsafe_code)` — if you find yourself wanting unsafe there, the design
is wrong, stop and re-read the [P3] note.
