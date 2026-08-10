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

## F9 — `iFrameSad` is an `int32_t` accumulated over a whole picture, and overflows at the maximum frame size

**Status: open, pre-existing, unreachable below ~8.4 megapixels — a second instance of
F8's defect class, in the encoder's analysis pass rather than the decoder's transform.**
Found while converting T8 (`processing/vaacalc.rs`).

### What it is

All five `VAACalc*` kernels accumulate the picture's total SAD into one `int32_t`,
faithfully to `codec/processing/src/vaacalc/vaacalcfuncs.cpp`:

```rust
*pFrameSad += l_sad;        // int32_t += int32_t, once per 8x8 quadrant
```

`l_sad` is bounded (64 samples x 255 = 16 320 per quadrant) and the per-macroblock
accumulators are reset each macroblock, so `iFrameSad` is the only quantity in the
family that accumulates across the whole picture. Its ceiling is
`width * height * 255`.

| | C++ | this port |
|---|---|---|
| behaviour | signed-overflow **UB**; wraps on every target this builds for | **panics**, `attempt to add with overflow`, in a debug build |
| release | wraps | wraps (overflow checks off) |

### Reachability

`i32::MAX / 255 = 8 421 504` samples, i.e. **32 896 macroblocks**. `MAX_MBS_PER_FRAME`
is **36 864** (`encoder/wels_preprocess.rs:93`, level 5.2's `uiMaxFS`), which is
4096x2304 and 9 437 184 samples — comfortably past the threshold. So the overflow is
reachable at the top two frame sizes the encoder accepts, and only there, and only with
a frame pair whose mean absolute difference exceeds ~227 of 255 across the entire
picture. A cut from near-black to near-white at 4K does it; ordinary content nowhere
near does.

This is why the gate battery has never seen it: every bench and sweep configuration
tops out at 1080p, where the ceiling is 528 768 000 — a quarter of the way to overflow.

### Why it is F8's class and not a new one

Same shape, same verdict: a C++ signed-overflow UB that the transliteration turns into
a debug-build panic, unreachable on realistic input, reachable in principle, and
**not** Phase 2's to repair. Widening `iFrameSad` to `int64_t` would change the release
path's output on the wrapping case and break byte-exactness against the C++, which is
the one thing this refactor may never do.

Unlike F8 this one is encoder-side, so the input that reaches it is the *application's*
raw frames rather than a hostile bitstream — there is no attacker-controlled path here,
only a correctness ceiling. That makes it lower severity than F8, and it is recorded
for completeness rather than as a decoder-panic candidate.

### What Phase 2 did about it

Nothing to the kernels — the safe walker accumulates `frame_sad` as an `i32` with the
same `+=` in the same order (quadrant within macroblock within row), so it panics and
wraps exactly where the old code does. The differential test drives pictures up to
64x48, far below the threshold, so it compares two kernels rather than two panics.

### Who fixes it

Whoever owns the encoder's analysis arithmetic — not P13, which is decoder panic
policy. The fix is a type change at the `SVAACalcResult::iFrameSad` field and every
consumer of it, which is Phase 4's plumbing or later, and it has to be taken together
with a decision about whether matching the C++'s wrap still matters at that point.

---

## F10 — The raw SAD kernels' trailing row-pointer bump is out-of-allocation pointer arithmetic on an exactly-sized buffer

**Status: open, pre-existing, latent on every real call — the UB is in pointer
*arithmetic*, not in any access, and real picture allocations always extend past the
block.** Found by Miri during T6's session, in the **parked** T5-sad family.

### What it is

Every single-block raw SAD kernel walks its rows as the C++ does:

```rust
for _ in 0..4 {
    iSadSum += ...;                       // reads row
    pSrc1 = pSrc1.offset(iStride1 as isize);   // bumps AFTER the last row too
}
```

After the final row the bump computes `pSample + h*stride` (`+ offset` for the
composite kernels' sub-blocks, `+ (h+2)*stride` through a four-point down arm). On a
buffer that ends at the block's last row — `(h-1)*stride + w` bytes, exactly the span
the T5 shim contracts declare — that pointer is past one-past-the-end, which is UB in
both Rust and C (`sad_common.cpp` has the identical `pSrc += iStride`). Nothing ever
dereferences it.

### How it surfaced, and why only now

`sad_shims_stay_inside_the_spans_they_declare` probes the Wels names with exact-span
buffers. When session C wrote it the names were shims (safe kernels compute nothing
past their span); the same session's unswap `11f82d41` made the names raw again, and
the next Miri run of the differential file — T6's session — flagged the bump.
`gates.sh` runs Miri on `--lib safe::` per commit and on the differential files only
at phase exits, which is the gap it slipped through.

### What was done about it

The probe's buffers are sized to the raw kernels' pointer-arithmetic footprint while
the family is parked (`h*stride + w`, and `(h+2)*stride + w` on the four-point
reference side), with a comment in the test naming this finding and instructing the
re-landing commit to restore the exact spans. T4 set the precedent: the test is the
thing handing the kernel an allocation no real caller hands it, so the test carries
the accommodation, not the kernel — the raw bodies are already written safe-side
(`common/sad_common.rs` kernels are proven and parked) and die at re-landing or
caller conversion.

### Who fixes it

Nobody fixes the raw bodies — they are scheduled to be *replaced* (Phase 4
direct-dispatch checkpoint or Phase 6.3 caller conversion; `perf_baseline.md`
§Parked). If the family re-lands, the safe kernels take over and the bump ceases to
exist. Worth knowing for T7: the encoder's `sample.rs` SAD/SATD raw kernels have the
same loop shape, so their prove-and-park differential tests should size raw-side
buffers to whole rows from the start.

---

## F11 — `WelsIHadamard4x4Dc`'s plain `i16` additions overflow above ±2047, and a debug build panics where the C++ wraps

**Status: open, pre-existing, unreachable on realistic content — a third instance of
F8's defect class, in the encoder's qp < 12 luma-DC reconstruction path.** Found
while converting T7's `encoder/decode_mb_aux.rs` family (the raw body lives in
`svc_encode_mb.rs`).

### What it is

The inverse 4x4 Hadamard for the I16x16 luma DC block does both passes in plain
`i16`, faithfully to `codec/encoder/core/src/decode_mb_aux.cpp`:

```rust
iTemp[0] = *pRes.add(kiIdx) + *pRes.add(kiIdx2);   // i16 + i16, no wrapping_*
```

Worst-case gain across the two passes is **16x** (each pass sums four inputs), so
any input above `32767 / 16 ≈ 2047` can overflow an intermediate.

| | C++ | this port |
|---|---|---|
| behaviour | `int` arithmetic, implicit narrowing to `int16_t` per store — wraps | **panics**, `attempt to add with overflow`, in a debug build |
| release | wraps | wraps (overflow checks off) |

Unlike its qp >= 12 sibling `WelsDequantIHadamard4x4_c` — whose port already uses
`wrapping_add`/`wrapping_mul` throughout and is total — this kernel kept the plain
operators, so the two siblings disagree about the same arithmetic hazard.

### Reachability

The one caller is gated on `uiQp < 12` (`svc_encode_mb.rs:586`) and hands DC levels
that just came out of `pfQuantizationDc4x4`. At qp 0 with a saturated Hadamard DC
input of 32767 the quantized level reaches `((0 + 32767) * 13107) >> 16 = 6553` —
past the 2047 threshold, so the overflow is reachable in principle through
defined-value paths. Reaching it takes a hand-crafted frame whose 16 4x4 DC sums all
saturate at qp < 12; ordinary content is orders of magnitude below. Encoder-side, so
F9's severity class: the input is the application's raw frames, not a hostile
bitstream — a correctness ceiling, not a panic candidate for P13.

### What Phase 2 did about it

Nothing to the kernel — `ihadamard_4x4_dc` reproduces the plain `i16` additions
exactly, panic for panic (R-e). The differential test bounds its inputs to ±2047
with the 16x-gain derivation written next to the bound, because above it the test
would compare two panics. Recorded here as fuzz-absence data: an encoder-input
fuzzer would plausibly reach this in its first corpus.

### Who fixes it

Whoever unifies the encoder's DC arithmetic policy — the natural fix is the same
`wrapping_*` treatment its qp >= 12 sibling already has, but that is a behaviour
choice (match-the-C++-wrap) that belongs with F9's owner, Phase 4's plumbing or
later, not with a parity conversion.

---
