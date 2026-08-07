# Phase 1 findings

Things found while executing Phase 1 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— building the safe vocabulary types in `src/safe/` — that are *not* Phase 1's job to
fix. Recorded here so no later session has to rediscover them. Phase 0's findings (F1
the release segfault, F2 the four bitstream writers, F3 the MT nondeterminism) are in
[`phase0_findings.md`](phase0_findings.md); numbering continues from there.

---

## F4 — The reader's slop is a read *past the buffer*, not merely past the RBSP

**Status: open; it is Phase 3.1's decision to make, and the plan already assigns it
(§2.2.2 item 1, P6).** Found while building `BsCursor` (T4).

### What the C++ actually does

`dump_bits_aux` (`decoder/dec_golomb.rs:128-150`) refills like this:

```rust
let iAllowedBytes = pEndBuf - pStartBuf;      // the RBSP length
let iReadBytes    = pCurBuf - pStartBuf;
if iReadBytes > iAllowedBytes + 1 { return ERR_INFO_READ_OVERFLOW; }
let word = (*pCurBuf as u32) << 8 | *pCurBuf.add(1) as u32;
```

The guard permits `pCurBuf` to sit **one byte past** the end and then loads **two**
bytes there. So the largest index the refill can read is `len + 2`, i.e. **three bytes
past the logical end of the RBSP**. Separately, `InitReadBits`
(`decoder/bit_stream.rs:57`) primes the accumulator with `GetValue4Bytes`, four bytes
at the start position, which overruns any RBSP shorter than four bytes.

The plan (§2.2.2) describes this as "allows the read cursor to sit 1 byte past
`pEndBuf` and then loads 2 bytes". That is right about the cursor and understates the
consequence: the *reads* go three bytes past, not one, and one of them is on the
initial prime rather than the refill.

### Why it matters

The correctness of every such read is a property of the **allocation**, not of the
NAL: the port is sound today only because `pRbsp` points into `sRawData`, a
`MAX_ACCESS_UNIT_CAPACITY` (4 MiB) zeroed allocation, so the bytes past a NAL are
allocated and zero — unless an access unit happens to end flush against the end of
that buffer.

Nothing in the type system says so, which is exactly the F1 shape: a size relationship
that exists only in the raw-pointer view.

### What Phase 1 did about it

`BsCursor` reproduces the **predicate** exactly — the differential test walks every
declared RBSP length from 0 to 8 bytes against six byte patterns and asserts equal
values, equal error codes and equal cursor state at every step, so error-code parity
on truncated streams is preserved bit for bit. What it does not reproduce is the read
itself: those loads go through `slice::get`, so

* with **at least 3 bytes of slack** past the RBSP (4 covers the initial prime for
  very short NALs), `BsCursor` and the C++ are **identical for every input** — this is
  what the differential tests run against, and what they prove;
* with less, `BsCursor` returns `ERR_INFO_READ_OVERFLOW` where the C++ would have read
  whatever followed the allocation.

So the safe cursor is a drop-in replacement *given the guard bytes*, and strictly safer
without them.

### What Phase 3.1 must decide

Whether to append the guard bytes to the raw-data buffer (the plan's stated
resolution — deterministic, in-bounds, and it keeps the predicate honest) or to accept
the divergence and let short buffers error. **Either way the decision must be explicit
and tested**, because the difference is only observable on a malformed or exactly-sized
stream, which is precisely where the conformance gates are weakest.

---

## F5 — The canonical bitstream writer panics on a 32-bit write in debug builds

**Status: open, latent, unreachable today.** Found while building `BsWriter` (T5).

`BsWriteBits` (`encoder/vlc_encoder.rs:367`) computes, in its else branch:

```rust
(*pBs).uiCurBits = ((*pBs).uiCurBits << (*pBs).iLeftBits) | (kuiValue >> iLen);
```

When the accumulator is empty, `iLeftBits == 32`, and the branch is taken for any
`iLen >= 32`. `u32 << 32` is:

| | C++ | this port |
|---|---|---|
| behaviour | undefined | **panics** with `attempt to shift left with overflow` in a debug build |
| release | (whatever the target's shifter does — on x86-64 and AArch64, a no-op) | shift amount masked to 0, so a no-op |

The result is only *correct* because the shift can only be reached with an empty
accumulator, where shifting contributes nothing either way.

**Reachability today: none.** The `BsWriteBits` widths in `src/encoder/` are literals
1, 2, 3, 4, 8 and 16, plus computed Exp-Golomb lengths and table entries, none of which
reaches 32 for the values the encoder writes. This is why 341/341 sweep configurations
and the whole test suite have never hit it. It is recorded because it is one grep away
from becoming live: any future syntax element written as a full word — a 32-bit SEI
payload, a timestamp, a user-data field — turns a debug build into a crash.

`safe::bits::BsWriter` folds the shift away (`head = 0` when `left_bits == 32`, with a
`debug_assert!` pinning the empty-accumulator invariant that makes it valid) and so
produces the intended output in both profiles. The differential test
`writer_and_the_32_bit_word` asserts the old side panics in debug and matches in
release, so if the old code is ever fixed, that test says so.

**Who fixes it:** Phase 3.2, in the commit that collapses the four writer copies (F2)
and decides their guard semantics. The fix is the same one `BsWriter` already carries.

---

## F6 — The port defines `malloc`/`free` with signatures that do not match libc

**Status: open, cosmetic-looking, worth one grep in Phase 6.** Surfaced by Miri as a
`suspicious_runtime_symbol_definitions` warning while running the differential tests.

`common/memory_align.rs:17` declares

```rust
fn malloc(size: usize) -> *mut u8;
```

where the platform's is `unsafe extern "C" fn(usize) -> *mut c_void`. The return types
are ABI-compatible on every target this builds for, so the warning is not evidence of a
live bug, and Miri's own message is about the *definition* shape rather than a
miscompile. It is noted here only because `common/memory_align.rs` is deleted outright
in Phase 6 (plan §6 file map) — the right fix is deletion, not a signature patch, and
nobody should spend a session on it before then.

---

## F7 — The reader's boundary pointers go out of bounds — UB, not a wrong value — outside the codec's own invariants

**Status: open, unreachable through the codec's call paths, and it constrains Phase
3.1.** Found by running the differential tests under Miri (T8), which is the first time
any part of this port has been executed under Miri at all.

Two sites, both computing a boundary pointer without a range check:

| site | expression | UB when |
|---|---|---|
| `decoder/bit_stream.rs:98` (`DecInitBits`) | `pTmp.offset(kiSizeBuf)`, `kiSizeBuf = (kiSize + 7) >> 3` | `kiSize < -7`, so the shift goes negative and the end pointer lands *before* the allocation |
| `decoder/bit_stream.rs:66` (`InitReadBits`) | `pEndBuf.offset(-iEndOffset)` | `iEndOffset > pEndBuf - pStartBuf`, likewise before the allocation |

Both are UB by the *arithmetic alone* — no dereference needed
([reference: pointer offsets must stay in bounds][ptr-offset]) — so an optimiser is
entitled to assume they never happen, and Miri reports them as errors:

```text
error: Undefined Behavior: in-bounds pointer arithmetic failed: attempting to offset
pointer by -1 bytes, but got alloc192953 which is at the beginning of the allocation
  --> src/decoder/bit_stream.rs:98:23
```

**Reachability: none today, by two invariants that are nowhere written down.**
`DecInitBits`'s callers in `nalu.rs` guard on `iBitSize > 0`, and `iBitSize` is derived
as `(kiSrcLen << 3) - trailing_bits(..)` with `kiSrcLen >= 1`, so it cannot go negative.
`InitReadBits`'s only non-zero caller passes `1`
(`parse_mb_syn_cabac.rs:3312`), and a cursor that initialised successfully always has
`pEndBuf - pStartBuf >= 1`, because `DecInitBits` rejects a zero-length RBSP. Take
either invariant away — a caller that skips the `> 0` guard, a future `iEndOffset` of
2 — and this is UB on malformed input.

**What Phase 1 did about it.** `BsCursor` has no such hazard by construction: `init`
and `init_read_bits` do the same comparisons in `isize` arithmetic over offsets, so an
out-of-range parameter produces `ERR_INFO_INVALID_ACCESS` and nothing else. The
differential tests compare the two sides across the whole *in-contract* range and check
the safe side alone outside it, with the boundary called out at both sites. **No test
needed `#[cfg_attr(miri, ignore)]`** — which is a better outcome than the phase brief
anticipated, and worth keeping that way: an ignored test is UB coverage lost.

**Who fixes it:** Phase 3.1, which rewrites both functions onto `BsCursor` and thereby
deletes the arithmetic. The point of recording it is that Phase 3.1 must not "preserve"
the pointer form for fidelity — there is nothing to preserve, the offsets are the
faithful translation.

[ptr-offset]: https://doc.rust-lang.org/std/primitive.pointer.html#method.offset

---
