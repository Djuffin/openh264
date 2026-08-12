# Session prompt — Phase 3, session E: T3.4, the encoder write side part 1 (dedupe, then `BsWriter`, then — possibly — the death of `SBitStringAux` itself)

> **SUPERSEDED — HISTORICAL.** Phase 3 completed 2026-08-11 (sessions A–F). This
> brief is kept as the record of what was asked, not as instructions. The phase's
> outcome is in [`../safety_refactor_log.md`](../../safety_refactor_log.md) (entries
> for sessions A–F) and plan §0; the next phase's brief is
> [`phase4b.md`](phase4b.md).

**Governing:** [`phase3.md`](phase3.md) §T3.4 + §2, plan §7.6, and — read in full
before converting anything — [`phase2_findings.md`](../../phase2_findings.md) **F2 and
F5**, [`phase2_findings.md`](../../phase2_findings.md) **F13's `InitBits` site**, and
the **session D log entry** (its §2 zeroed-struct hazard and its hand-off section
are this brief's sources). This file scopes the session and supersedes on
disagreement; fix disagreements in place.

**Where the phase stands:** the decoder read side is fully safe-owned (cursor,
engine, buffer, NAL identity — T3.0–T3.3 complete, F15/F16/F17 closed). T3.4 opens
the encoder write side. Session shape: **face 1 = the F2 dedupe; face 2 =
`vlc_encoder.rs` → `BsWriter` with F13's signature dying; face 3 = the
`SWelsSliceBs` flip and, if the inventory says so, the deletion of `SBitStringAux`
itself.**

> **Correction, made after the fact (2026-08-11): faces 2 and 3 do not separate, and
> landed as one commit.** A writer function taking `(buf: &mut [u8], w: &mut BsWriter)`
> cannot be fed by a struct field holding an `SBitStringAux` — the field's type is
> *forced* by the signature change, not adjacent to it — and the `abi_guard` asserts
> are `const` assertions, so there is no intermediate green state. What *does*
> separate cleanly is the third face's other half: deleting `SBitStringAux` is a pure
> subtraction and is its own commit. See the log's session-E entry §1 for the
> generalisation, which Phases 5 and 6 need: **the decomposable unit in a struct-field
> conversion is the set of structs reachable from one signature, not one struct.** T3.5 (the encoder CABAC triple + rollback) starts only from a standing
start with a third of the session left — the standing rule, unchanged.

Tree at session start: clean at `b308f7d5`. Open with the control battery
(`OVERALL:` line is the verdict), ratchet `check`, and recounts.

---

## 1. Two rules in force before any edit

- **The construction-audit rule (session D §2, now binding on every encoder
  struct):** any struct gaining an owned field gets its construction paths audited
  *in the same commit* — `mem::zeroed` construction makes field-type changes UB at
  a distance, and the encoder is wholesale-zeroed: `sWelsEncCtx::default()` is
  `mem::zeroed` behind `Box::into_raw(Box::new(…))` (`encoder_ext.rs:~1532`), and
  the `nal_encap.rs` structs are reached through zeroed contexts and
  `WelsMallocz`'d slice arrays. The decoder's precedent (`MaybeUninit` shell +
  `new_boxed()`, comment naming its deleting phase) is the pattern when an audit
  fails. **The mitigating fact for this session:** `BsWriter` is
  `{pos: usize, cur_bits: u32, left_bits: i32}` — all-integers, **valid at
  all-zero** — so embedding it where `SBitStringAux` sat is construction-*safe*
  under zeroed allocation. Verify that claim against the real `BsWriter` (if
  Phase 1 gave it anything non-zero-default, the claim dies), state it in the
  commit message, and the audit is discharged. Owned `Vec`s are a different
  story and they are **T3.6's** — do not add any this session.
- **Comparison-form arithmetic, third instance queued by name:**
  `svc_set_mb_syn_cavlc.rs:752`'s `pEndBuf - pCurBuf - 1` space checks convert in
  comparison form (`pos < len` guards; `checked_sub` to the error path) — a
  `usize` subtraction wrapping is a *new* OOB, twice proven in sessions C and D.

## 2. Face 1 — the F2 dedupe, as its own zero-output commit

**Re-read F2 in full first.** The map, verbatim from the finding and the hand-off:

| copy | status | what dies with it |
|---|---|---|
| `vlc_encoder.rs:367` | **canonical** (matches C++ `bit_stream.h`) | — |
| `svc_set_mb_syn_cavlc.rs:157` | equivalent; hand-rolled 4-byte store | the hand-rolled store |
| `nal_encap.rs:169` | equivalent; explicit `iLen == 0` guard | the guard (canonical relies on `(1 << 0) − 1 == 0`) |
| `svc_encode_slice.rs:509` | **divergent**: null/`iLen <= 0` early-returns, pre-masks `kuiValue` to `iLen` bits, inverted branch sense, `wrapping_add` in `BsWriteUE` | all four divergences |

Route every caller onto the canonical family; delete the three copies; the commit
message records **which guard semantics die and why that is safe** (all four copies
agree on in-contract inputs — 341/341 across both profiles is the standing proof,
and remains the referee: this commit changes **zero bytes of output or it
reverts**). Two constraints: **do not fix F5** while in there (the canonical
writer's debug-only panic on a 32-bit write into an empty accumulator is S6 parity
— `BsWriter` already handles it differently and a test pins both); and per the
dead-duplicate lesson (session D §3), expect the dedupe to surface dead
transliterations around the copies — delete what provably dies, list what doesn't.

## 3. Face 2 — `vlc_encoder.rs` onto `BsWriter`, and F13's signature dies

- The writer functions move to `(buf: &mut [u8], w: &mut BsWriter)` shape;
  `InitBits`'s lying signature (`kpBuf: *const u8` stored as `*mut`, written
  through — **every honest caller is UB**) is deleted, not amended.
- **Delete `au_set.rs`'s two accommodations in the same commit** and let Miri see
  the honest thing. The precedent (4a's F14, session D's F15-adjacent finds) says
  deleting an accommodation exposes the next finding immediately — **expect one**,
  and treat it per S12 with session B's disambiguation test (raw-code divergence →
  finding; your-window-narrower → your regression).
- Semantics preserved exactly: the 32-bit big-endian accumulator flush
  (`WRITE_BE_32` + advance-4) against `buf[pos..pos+4]` — slice indexing now
  supplies the bounds the C never had, and a panic there is a pre-existing sizing
  bug surfacing (plan §2.2.2), not new behavior on any in-contract path;
  `BsGetBitsPos` → `bits_pos()`; the `:752` space checks per §1's comparison rule.
- **The literal-`n` rule (phase brief §4) applies to writes too:** `write_bits(n)`
  with a literal `n` is the common case in slice-header writing — check the hot
  call shape keeps the literal visible through the adoption, and give the
  `BsWriteBits` path one S1-proportional disassembly look (per-syntax-element hot;
  T3.2's full protocol not required, but the refill-inline lesson transfers).

## 4. Face 3 — the `SWelsSliceBs` flip, the ABI asserts, and possibly the headline

- `SWelsSliceBs::sBsWrite: SBitStringAux` → `BsWriter` (**correction: the field is
  `sBsWrite`, not `sBs`** — `nal_encap.rs:133`; `SWelsEncoderOutput` and `SWelsOut`
  carry a field of the same name and had to flip with it) (construction-safe per §1 —
  discharge the audit in the commit message), and `SSlice::pSliceBsa` retypes
  `*mut SBitStringAux` → `*mut BsWriter` (the T3.1b precedent: pointer stays,
  pointee converts, Phase 6 owns the pointer's removal). The slice's output buffer
  (`pBsBuffer`, raw allocation) stays exactly where it is — buffers are T3.6's.
- **`encoder/abi_guard.rs` will fight you, by design:** it pins
  `SBitStringAux` at 48 bytes (`:55`) and `SWelsSliceBs` at 176 (`:61`)
  — **correction: four asserts had to go, not two.** `SWelsEncoderOutput` (96, `:62`)
  embeds an `SBitStringAux` too, and `SSlice` (1584, `:146`) embeds `SWelsSliceBs`
  by value, so both shrank by the same 32 bytes. The
  plan's standing rule (§Phase 6.6, applied early here): **delete each assert in
  the same commit that de-C-ifies its struct** — they are encoder-internal locks
  ("nothing outside the crate depends on these layouts"), and a tripped assert is
  the signal the commit is doing what it says. Do not contort a layout to keep an
  assert green.
- **The possible headline — inventory it:** after the decoder's conversion
  (T3.1b retyped its last `SBitStringAux` pointer) and this face's flip, grep for
  remaining `SBitStringAux` users. The encoder CABAC writer (T3.5's
  `m_pBufStart/Cur/End` triple) carries its own fields, not the struct. **If the
  inventory comes back empty, delete `SBitStringAux` itself** — the
  pointer-triple cursor type that §1.2's taxonomy named as T3's emblem — together
  with its 48-byte assert, as its own commit with the inventory in the message.
  If users remain, list them with owners (likely the MT slice-buffer path —
  verify against `slice_multi_threading.rs`) and leave the type with a
  doc-comment naming its deleter.
- **MT discipline:** the writer state is thread-private by the task-claiming
  invariant (each task owns `m_iThreadIdx`'s buffers); this face changes *what*
  each thread's writer is, not *who* owns it. Touch nothing in the pool or task
  files (F12 territory, Phase 7's), and expect F3-signature sweep hits per S14 —
  the twelfth-plus measurements, with the docs-only-tree precedent on record for
  how innocent they can be.

## 5. Gates

Control at open; per-face: full battery, **decoder goldens must not move at all**
(no exceptions remain — F15's rows are recorded now), 53 conformance hashes,
sweeps 341/341 **both profiles** (the encoder's byte-exactness referee — every
face of this session is encoder-side, so the sweeps carry the proof the goldens
carried for the decoder); one interleaved pair both benches per face (encoder
rows are the signal; cumulative encoder ≈ +8.9%; fresh null for any floor
verdict); Miri with the accommodations gone; ratchet (writer pointer-triples die
— `raw_ptr` down; `SHIM(phase3)` for any shell sync; abi_guard deletions
enumerated). Log entry with the F2 semantics decision, the `SBitStringAux`
inventory verdict, and the hand-off.

## 6. Non-goals

No T3.5 except from a standing start (its rollback-snapshot conversion — the
`Copy`-of-`BsWriter` design — is specified in the phase brief and wants a fresh
session with the CABAC triple). No T3.6 (owned output buffers are where the
construction-audit rule meets `Vec` fields — next session, deliberately). No
`pfWelsSpatialWriteMbSyn`/entropy-enum work (Phase 4b's, *because* it touches
these files — the fence exists for this exact moment). No pool/threading edits.
No fixing F5 or F12. No golden regeneration of any kind. No `get_unchecked`
(S8). And if the `SBitStringAux` deletion tempts a "while I'm here" sweep of
`wels_common_defs.rs` — the type's neighbors have their own owners; take the
one deletion the inventory licenses and stop.
