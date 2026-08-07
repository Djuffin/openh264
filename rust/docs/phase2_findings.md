# Phase 2 findings

Things found while executing Phase 2 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— converting the leaf DSP kernels onto safe signatures — that are *not* Phase 2's job to
fix. Recorded here so no later session has to rediscover them. Phase 0's findings (F1 the
release segfault, F2 the four bitstream writers, F3 the MT nondeterminism) are in
[`phase0_findings.md`](phase0_findings.md), Phase 1's (F4–F7) in
[`phase1_findings.md`](phase1_findings.md); numbering continues from there.

---

## F8 — The 8x8 IDCT's `i16` intermediates overflow, and a debug build panics where the C++ wraps

**Status: open, pre-existing, unreachable on conformant streams — but reachable on
malformed ones, which makes it a decoder-panic candidate (plan P13).** Found while
writing the pilot family's differential test (T2).

### What it is

`IdctResAddPred8x8_c` (`decoder/decode_mb_aux.rs`) does both of its 1-D passes in `i16`,
faithfully to `codec/decoder/core/src/decode_mb_aux.cpp`:

```rust
let mut p = [0i16; 8];
let mut a = [0i16; 4];
…
a[0] = p[0] + p[4];                       // int16_t + int16_t, stored to int16_t
a[3] = p[3] + p[5] + p[1] + (p[1] >> 1);
b[1] = a[0] + (a[3] >> 2);
iTmp[…] = b[0] + b[7];
```

Worst-case gain through one pass is **7.875x** (`a` reaches 3.5x the input, `b[1]`
1.25x that, and `iTmp` sums a 3.5x and a 4.375x term), so ~62x over the two passes.
With `i16` coefficients that means any input above roughly `32767 / 62 ≈ 528`
overflows an intermediate.

| | C++ | this port |
|---|---|---|
| behaviour | signed-overflow **UB**; wraps on every target this builds for | **panics**, `attempt to add with overflow`, in a debug build |
| release | wraps | wraps (overflow checks off) |

The 4x4 kernel does **not** have this shape: it computes in `i32` and truncates with an
explicit `as i16`, so it is total over the whole `i16` input range. Only the 8x8 path is
affected. This is F5's defect class (a C++ UB that the port turns into a debug panic),
one transform over.

### Reachability

`pScaledTCoeff` is `int16_t`, so the coefficients handed to this kernel are bounded by
`±32767` — well past 528. For a *conformant* stream the spec's own bounds keep
dequantised 8x8 coefficients inside the transform's dynamic range, which is why 341/341
sweep configurations, the whole conformance suite and every bench row have never hit it.
A malformed or hostile stream is a different question, and nothing between the parser
and this kernel clamps.

**This is a place where the fuzzer would have been the right instrument.** Phase 0's T7
was deferred by direction, so there is no corpus net; this finding was reached by
hand-written property testing instead, and only because the differential test deliberately
generated full-range coefficients. Recorded here as data for reopening T7 (plan §7.3): a
`decode_annexb` target would plausibly find this within its first minutes, and it is
exactly the "no panics on arbitrary input" invariant P13 names.

### What Phase 2 did about it

Nothing to the kernel — the conversion is behaviour-preserving, and the safe kernel
reproduces the `i16` arithmetic exactly, panic for panic. What it *did* do is bound the
8x8 differential test's coefficients to ±512 with the derivation above written out at
`tests/kernels_differential_phase2.rs`'s `BOUND_8X8`, because above the bound the test
would be measuring the old side's panic rather than comparing two kernels. The 4x4 tests
run at the full `i16` range precisely because they can.

### Who fixes it

Whoever owns decoder panic policy — plan P13, in practice **Phase 5** (the decoder
structural rewrite) or a reopened T7. The fix is not to widen the intermediates, which
would change output on the wrapping path and break byte-exactness; it is to decide
whether malformed input may reach the kernel at all, and to clamp or reject at the
parse/dequant boundary if so. Do not "fix" it inside the transform.

---
