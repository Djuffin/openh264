# Phase 3 findings

Things found while executing Phase 3 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— the bitstream layer — that are *not* Phase 3's job to fix at the moment they were
found. Recorded here so no later session has to rediscover them. Numbering continues
from [`phase2_findings.md`](phase2_findings.md) (F8–F14); Phase 0's F1–F3 and Phase 1's
F4–F7 are in their own files.

---

## F15 — A stream truncated one byte into a slice NAL takes the decoder out of bounds; the two build profiles disagree about what happens next

**Status: open, live on ordinary input, and it is T3.3's to fix.** Found by
`tests/malformed_stream_parity.rs` (T3.0) on its **first run**, before any conversion
— which is the argument for having built that test first.

### The two sites

`ParseNalHeader` converts a byte length into a bit length by looking at the last byte
of the payload:

| site | expression | reached by |
|---|---|---|
| `decoder/nalu.rs:762` | `BsGetTrailingBits(pNal.add(iNalSize as usize - 1))` | every VCL NAL (`NAL_UNIT_CODED_SLICE`, `..._IDR`, `..._EXT`) |
| `decoder/nalu.rs:675` | same expression | `NAL_UNIT_PREFIX` with `uiNalRefIdc != 0` |

A third instance, `nalu.rs:1000` (`ParseNonVclNal`), is **guarded** by
`if kiSrcLen <= 0 { return }` at `:995` and is not affected.

`iNalSize` reaches zero on input that is not exotic at all. `ParseNalHeader` strips
trailing zero bytes from the RBSP (`:536-545`) and then consumes the NAL header byte
(`:577-578`), so **any slice NAL whose payload is one non-zero byte followed by zero
or more zero bytes** arrives at the site with `iNalSize == 0`. Truncating a stream
exactly one byte past a slice's start code produces precisely that, and every base
stream in T3.0's corpus contains 10–11 such truncations.

### What each profile does

Measured with `MALFORMED_IGNORE_WITHHELD=1` (the diagnostic escape hatch in the parity
test), on `res/SarVui.264` truncated to 32 bytes:

| | behaviour |
|---|---|
| **debug** | `attempt to subtract with overflow` at `nalu.rs:762:88`. The panic unwinds out of the `extern "C"` vtable thunk, so it is a `panic in a function that cannot unwind` — the **process aborts**. Not a failed decode: a dead process. |
| **release** | `0usize - 1` wraps, so the argument is `pNal.add(usize::MAX)` — out-of-bounds pointer arithmetic, UB by the arithmetic alone even without the load ([F7]'s shape). The address it lands on is `pNal - 1`, the NAL header byte, which is inside the `sRawData` allocation; the header byte of a type-1/5 NAL is always odd, so `BsGetTrailingBits` returns 0, `iBitSize` is 0, `DecInitBits` fails its `pCurBuf >= pEndBuf` check, and the NAL is rejected with `dsBitstreamError`. |

So release "works", deterministically, by reading a byte it has no right to read and
getting the answer it would have wanted anyway. Per plan §7.2 gate 0 a debug/release
disagreement is UB evidence, never flakiness — and here both halves of the
disagreement are defects.

### Upstream has the same expression

`codec/decoder/core/src/au_parser.cpp:252` and `:396` are
`iBitSize = (iNalSize << 3) - BsGetTrailingBits (pNal + iNalSize - 1);`, i.e. the port
is a faithful transliteration of an upstream bug. In C++ `iNalSize` is `int32_t`, so
the expression decrements the pointer (UB by the standard, benign on every real
target); the port's `as usize` cast is what turns it into a checked-subtraction panic
in debug. **The C++ is not a specification to preserve here** — there is no correct
behaviour to be parity with, which is why the fix is a real fix rather than an S6
arithmetic-parity translation.

### What T3.0 did about it, and why it did not just record the outcome

The corpus **withholds** the triggering entries — see `withheld()` in
`tests/malformed_stream_parity.rs` — and writes a `WITHHELD` row naming this finding
in the golden table instead of a decode result. That is not squeamishness about the
abort (the test's worker/parent split survives an abort and names the entry that
caused it — that is how all 11 per-stream instances were enumerated in one run). It is
that a golden table has to hold in **both** profiles, and this input has no
profile-independent behaviour to record.

The rows are therefore visible, counted, and tied to this finding rather than silently
missing. `MALFORMED_IGNORE_WITHHELD=1` runs them anyway for diagnosis.

### Who fixes it

**T3.3**, the seam that converts `nalu.rs`'s payload pointers to `Range<usize>`: the
expression becomes an index into a slice, the zero-length case becomes explicit, and
`withheld()` is **deleted in that same commit** so the eleven-per-stream `WITHHELD`
rows fill in with real error codes. That golden diff is the seam's evidence, and it is
a required gate strengthening in the same sense as deleting `au_set.rs`'s F13
accommodation (T3.4).

Until then, the decoder aborts in a debug build on a truncated slice NAL. Nothing in
the gate battery except T3.0 says so.

[F7]: phase1_findings.md#f7--the-readers-boundary-pointers-go-out-of-bounds--ub-not-a-wrong-value--outside-the-codecs-own-invariants

---
