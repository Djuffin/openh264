# Phase 5, session C — T5.C: `SPicture`'s planes become owned

Governing: [`phase5.md`](phase5.md) — its §0 (battery, census gate, F3 protocol) and
§1 apply verbatim and are not repeated here; plan §7.6 (S20/S21/S24/S25). The closure
is in the Phase 5 session-A log entry §5, corrected by session B's recount. This file
scopes the session and supersedes on disagreement; fix disagreements in place.

## 0. Start

1. Commit the inherited doc tail (this brief).
2. Battery per phase5.md §0. Expected: **443 debug / 437 release / 20 ignored**,
   Miri **304**, sweeps 341/341 both profiles, goldens **56 rows** (three narrow
   assets included). Recount before relying on any number.

## 1. The conversion

`SPicture` (`decoder/picture.rs`): `pData: [*mut u8; 4]` + `iLinesize: [i32; 4]` →
three owned planes (`PaddedPlane`, `safe/plane.rs` — `new` / `from_parts`; `new`'s
doc already names `AllocPicture`'s 128 fill). Facts that bound the work:

- **Delete, don't convert**: the fourth plane slot (nothing in `src/decoder` reads
  index 3) and `iPlanes` (written-never-read in the port and the C++ decoder).
- **Holders keep their pointers.** Nothing embeds `SPicture` by value; the ten-file
  `*mut SPicture` holder table (session-A log §5) is out of scope — the pointee's
  fields convert, the pointers stay (T3.1b precedent). PicId/arena/identity work is
  a later closure, not this one.
- **Call-site bridge**: every `pData[i]`/`iLinesize[i]` read site moves to an
  accessor on `SPicture` that derives from the owned plane. Where a raw
  `*mut u8`/stride pair must survive for still-raw kernel callers, the accessor is
  the shim — mark it `SHIM(phase5)`, deleted as 5.2–5.6 convert callers. **Never a
  stored mirror field**: a cached `pData` beside the owned plane is the F16/T5
  class.
- **Allocation** (S21 + F19): owned fields cannot live in a `WelsMallocz`'d
  `SPicture`. `AllocPicture` → heap construction with a real constructor;
  `FreePicture` → the matching drop. Audit every construction path of `SPicture` in
  the same commit (eight allocations were F19-clean at session A — keep that true
  and say so). If any zeroed-reach path survives, the `MaybeUninit` shell +
  `new_boxed()` pattern (`decoder_context.rs:769`) is the fallback; prefer the real
  constructor.
- **Byte-exactness invariants**: the 128 fill over the full padded allocation;
  stride arithmetic identical to `AllocPicture`'s current math (the kernels' output
  depends on it — conformance hashes and the 56 golden rows are the referee); the
  EC prefetch memsets only the active area (T5.B1's asset pins exactly this — the
  `narrow_16x16_idr_lost` row must stay green).

## 2. S25, per file touched (phase5.md §1 step 4)

Each of `pic_queue.rs`, `deblocking.rs`, `error_concealment.rs` gets its S25
enumeration **with its conversion in this session, not before**: who else reaches
this `SPicture` (or the `SPicBuff.ppPic` array — `pCtx->pDec` points into it) while
a borrow is held? Write the enumeration into the commit that converts the file.
Session B's fix shape applies: name `(*p)` per use; no borrow outlives one
expression.

## 3. Commit shape

S20: compute the closure of the field change first — expected small (accessors
absorb the signature pressure), but if a signature forces a struct, the closure
sizes the commit, not this brief. One conversion face per commit; the
delete-dead-members face (`iPlanes`, slot 3) may land first as pure subtraction,
zero output movement.

## 4. Gates

Per phase5.md §7 per face: full battery; goldens frozen at 56, none moved; sweeps
341/341 both profiles; Miri (it watches `manage_dec_ref` now — the new borrows run
under it); 3-pair interleaved medians both benches per seam (allocation-path
change, expect flat; S2b if not); ratchet per S16 — `raw_ptr` should fall as
`pData` pointers die; census unchanged.

## 5. Close

Log entry with the closure as computed, the S25 enumerations, and F19's per-
allocation answer. Update phase5.md's §1 marks (step 3 done; step 4 per file).
Name the next closure in the hand-off: `PicPool`/recycling-predicate/identity (the
five P3 tests gate it) or 5.2's start, whichever the tree state argues for.

## 6. Non-goals

No PicId/arena/identity conversion (the P3 tests exist to gate it later). No
`SPicBuff.ppPic` ownership change beyond what heap construction forces. No 5.2+
pulls (`SDqLayer`, cache fills, `mv_pred`). No golden movement — the 56 rows are
frozen. No F3 work beyond phase5.md §0's protocol. No `get_unchecked` (S8). No
encoder-side edits.
