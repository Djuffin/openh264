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

**Second instance, 2026-08-10 (session F, T7's SATD) — whole rows are not enough
for composites.** The SATD differential applied this rule as `h * stride` and Miri
flagged it anyway: a *composite* kernel's sub-blocks bump from their own anchors,
so `WelsSampleSatd8x4_c`'s right 4x4 computes `anchor + 4 + 4*stride` — up to
`(w-4) + h*stride` past the block anchor, beyond a whole-row buffer at any anchor
past column 0. The rule as now applied: raw-side buffers span
**`(h + 1) * stride`**, which covers every sub-block's trailing bump at every
legal anchor. The safe side keeps exact spans; the instrument that caught this was
the same phase-exit Miri run that found the original, run mid-session on purpose.

---

**Third instance, 2026-08-10 (session G, T9's Miri-gate widening) — it was in the
module's own unit tests all along.** Widening `gates.sh`'s Miri step from
`--lib safe::` to the whole library (the session-B item, executed at this phase
boundary) failed immediately on `common/sad_common.rs`'s
`test_sample_sad_16x16_diff`, which hands `[0u8; 256]` at stride 16 to a parked
raw composite: the bottom-right 8x8 starts at `8*stride + 8` and bumps eight
times, reaching `264` in a 256-byte allocation. `test_sample_sad_partitions` had
the same defect at stride 32. Both are now `(h + 1) * stride`, the rule as
corrected by the second instance, with the derivation in the test.

This is the finding's own lesson turned back on itself: F10 was recorded twice in
the *differential* file and both times the fix was applied only there, because
that was the only file the Miri gate ran. The kernels' own unit tests — written
long before, in the module — had been exercising the same UB continuously and
invisibly. **An accommodation is only as wide as the instrument that found it**,
and this one had been two files too narrow for three sessions.


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

## F12 — Every worker thread takes `&mut` to the one shared `CWelsThreadPool`, and Miri calls it a data race

**Status: open, pre-existing, live on every multi-threaded encode — a *soundness*
defect rather than an arithmetic one, and the first of its class this refactor has
recorded.** Found by widening `gates.sh`'s Miri step from `--lib safe::` to the
port's own unit tests, at Phase 2's exit (the session-B item).

### What it is

`IWelsTaskThreadSink`'s methods take `&mut self`:

```rust
pub trait IWelsTaskThreadSink: Send + Sync {
    fn OnTaskStart(&mut self, pThread: *mut CWelsTaskThread, pTask: Option<TaskPtr>) -> …;
    fn OnTaskStop (&mut self, pThread: *mut CWelsTaskThread, pTask: Option<TaskPtr>) -> …;
}
```

and `CWelsThreadPool` implements it. Each `CWelsTaskThread` holds
`m_pSink: *mut dyn IWelsTaskThreadSink` pointing at the **one** pool, and calls
`OnTaskStart`/`OnTaskStop` from its own thread as tasks are dispatched. Every such
call materialises a `&mut CWelsThreadPool`, so two worker threads plus the pool's
own thread routinely hold `&mut` to the same object at the same time. Miri's
report is the retag itself, not any field access:

```
Data race detected between (1) retag read on thread `CWelsThreadPool`
and (2) retag write of type `CWelsThreadPool` on thread `CWelsTaskThread`
   --> src/common/wels_thread_pool.rs:435:9   ( `&mut self,` )
```

### Why the mutexes do not save it

Every field of `CWelsThreadPool` is a `Mutex` (`m_cWaitedTasks`, `m_cIdleThreads`,
`m_cBusyThreads`, `m_cLockPool`, `m_running`, `m_end_flag`, `m_handle`), so the
*data* is race-free and the code works. What is unsound is the reference: `&mut T`
carries a uniqueness claim to the compiler regardless of what `T` contains, and two
live `&mut` to one allocation is Undefined Behaviour whether or not either is
dereferenced. LLVM is entitled to optimise on the `noalias` that claim implies.
This is the same category as F1 — a size/aliasing fact the type system was told
wrongly — not the F8/F9/F11 arithmetic-parity category.

The `unsafe impl Send for CWelsThreadPool` / `unsafe impl Sync` pair is the place
the claim was made; it makes sharing legal but says nothing about `&mut`.

### Reachability

Every encode with `iMultipleThreadIdc > 1`, on every frame. It has never produced
an observed failure — which is exactly the F1 warning restated: 341/341 byte-identical
sweeps ran on top of a stack overflow for the port's whole life.

### What Phase 2 did about it

Nothing to the code, per the phase's parity rule. `gates.sh`'s widened Miri step
**skips the `wels_thread_pool` tests by name**, with this finding cited at the
skip, so the rest of the library's unit tests are covered from now on. Removing
that skip is part of the fix, not a separate task.

### Who fixes it

**Phase 7** (the threading rework), which owns this file, or Phase 6.4 if the
`Vec<SliceState>` work reaches it first. The likely shape is small — every field is
already a `Mutex`, so the sink methods can take `&self` and the trait object can be
`*const dyn` — but it is a threading-layer interface change and it wants to land
with the thread-pool rework rather than in front of it. Phase 7's exit gate should
run the Miri step with the skip removed.

### The F3 connection (hypothesis, recorded 2026-08-10)

F3 — the ~1-per-several-hundred wrong-length MT `sm=3` bitstream — now has **two**
known-UB candidate mechanisms sitting under it: the `ReallocateSliceList` pointer
invalidation F3's own write-up names (plan §P10), and this finding, a `&mut` retag
race on the pool that every dispatched task executes. A retag race is exactly the
class that miscompiles *rarely* and moves with codegen — which is F3's observed
shape (rate varies with machine load and profile, crash-free, output wrong-length
in either direction). No causal claim is made; the practical consequences are two:
**nobody should burn a session chasing F3 while two known-UB mechanisms sit under
it** (fix the UB first, then see what remains), and **Phase 7's exit — which
deletes both mechanisms and removes this skip — doubles as the F3 experiment**: if
the repeated-determinism gate runs clean after the rework, F12/P10 were
load-bearing; if F3 persists, it is a third mechanism and gets hunted on sound
code with Miri's full coverage.

---

## F13 — The widened Miri gate's remaining queue: four aliasing defects it cannot get past

**Status: open, pre-existing, live. Two are in production code and one is an API
signature that lies about mutability.** Found in the same pass as F12, by widening
`gates.sh`'s Miri step from `--lib safe::` to the whole library at Phase 2's exit.
Recorded together because they are one class with one cause, and because they are
the reason the widened gate carries a skip list.

### The class

A raw pointer is derived from an owner, a **second** pointer is then derived from
that same owner, and the **first** is used afterwards. Under Stacked Borrows the
second derivation pops the first pointer's tag, so the later use is Undefined
Behaviour — even though the addresses are identical and every build produces the
code the author expected. Nothing in a normal test run can see it; Miri sees it
immediately.

### The four sites

| site | shape | owner phase |
|---|---|---|
| `decoder/manage_dec_ref.rs:476` `AddLongTermToList` | `ptr::copy(list.as_ptr().add(i), list.as_mut_ptr().add(i+1), n)` — the `as_mut_ptr()` argument is evaluated after the `as_ptr()` one and invalidates it, so the copy reads through a dead tag. Fix is `copy_within`. | **Phase 5** (decoder structural rewrite owns the ref lists) |
| `encoder/encoder_ext.rs:820` `InitDqLayers` | takes `&mut (*(*pCtx).pSvcParam).sSpatialLayers[i].sSliceArgument` while another live reference into the same parameter struct is in scope | **Phase 6** (encoder context restructuring) |
| `encoder/vlc_encoder.rs:353` `InitBits` | declares `kpBuf: *const u8`, stores it as `pStartBuf: *mut u8`, and the writer writes through it. A caller that honestly passes `as_ptr()` produces a pointer with no write provenance, and the first `BsFlush` is UB. | **Phase 3.2** (the encoder write side; F2's family) |
| `encoder/encoder_ext.rs:2418`, `:2427` `SetFastCodingFunc` / `SetNormalCodingFunc` | **`SWelsFuncPtrList` is self-referential.** `sdf.pfMdCost = sdf.pfSampleSad.as_mut_ptr()` stores a pointer into the struct's own `pfSampleSad` array; `SetNormalCodingFunc` does the same with `pfSampleSatd`. Every later `&mut SWelsFuncPtrList` — and the encoder takes one constantly — reborrows the whole struct and pops that interior pointer's tag, so the next `(*pFuncList).sSampleDealingFuncs.pfMdCost.add(BLOCK_16x16)` read is UB. | **Phase 4a** (it owns `SWelsFuncPtrList` and the dispatch tables) |

The third is the interesting one: it is not a caller mistake, it is a signature
that documents the opposite of what the function does. Every honest caller is
wrong. `au_set.rs`'s two tests carried the accommodation
(`as_mut_ptr() as *const u8`, with the reason written next to it) rather than the
signature being changed, because changing it is Phase 3's job and it wants to
happen with `BsWriter`.

### What Phase 2 did about it

Recorded them, and **widened the gate around them**: `gates.sh`'s Miri step now
runs the whole library with `--skip wels_thread_pool --skip manage_dec_ref
--skip encoder_ext --skip svc_mode_decision`, each skip commented with the finding
that owns it. The skip
list is a work queue — deleting a line from it is part of fixing the thing it
names, and no skip may be added without a finding.

Four *test-side* instances of the same class were fixed outright in the same pass,
since a test that manufactures the aliasing is the test's bug and not a behaviour
change to the port: `common/sad_common.rs` (one `as_mut_ptr()` used for both
operands), `decoder/error_concealment.rs` (array written through its binding while
a derived pointer was still live), `decoder/parse_mb_syn_cavlc.rs` (three
derivations for one `SBitStringAux`), and F10's third instance in `sad_common`'s
buffer sizes.

### Why this matters more than the count suggests

Seven real defects in the first afternoon of a gate that had been running on 63 tests
and now runs on ~270. None of them had ever produced an observable failure, which
is F1's lesson restated for the third time in this refactor: **byte-exactness does
not imply soundness**, and the instrument that can tell the difference had been
pointed at a small corner of the codebase.

---

## F14 — The 16x16 SAD walks one row past `pMemPredMb`, in production

**Status: FIXED 2026-08-10 (Phase 4a), by accommodation rather than by repair.**
Found by the Miri `--lib` gate the moment F13's fourth site stopped blocking it —
which is the finding's real subject and the reason it is written up rather than
just patched.

### What it is

`SMbCache::pMemPredMb` is `WelsMallocz(2 * 256)` (`svc_encode_slice.rs`), two
256-byte halves that are the I16x16 prediction ping-pong: `pMemPredLuma` at +0,
`pMemPredChroma` at +256 (`svc_base_layer_md.rs:371-373`). `WelsMdI16x16` writes
a candidate prediction into one half and scores it with `WelsSampleSad16x16_c`
at stride 16.

That kernel decomposes into four `WelsSampleSad8x8_c` calls; the last starts at
`base + (16 << 3) + 8` = +136 and reads eight rows, through byte `256 + 136 +
7*16 + 7` = **byte 511 of a 512-byte allocation — exactly in bounds, with
nothing to spare.** But `WelsSampleSad8x8_c` is a faithful C transliteration and
bumps its row pointer at the *end* of every iteration including the last
(`sad_common.rs:158`), so it computes `base + 520` and then returns without
dereferencing it.

Forming that pointer is UB — in Rust (`ptr::offset` requires the result to stay
in bounds) and in C alike. Nothing observable ever came of it: the value is
computed into a register and discarded. That is exactly why it survived the
port, the conformance suite, 341/341 sweeps, and every profile.

### Why it is F10's class and S12's rule, met in production

S12 was written after F10 was found three times in *tests*: "a raw kernel's
pointer footprint is bigger than its read footprint. Any test handing an
exactly-sized buffer to a raw kernel must size it `(h + 1) * stride`." Every
prior instance was a test manufacturing an exact buffer. This one is the encoder
itself, and the C++ upstream has the identical latent defect — the port did not
introduce it, it transliterated it.

The distinction matters for how it gets fixed. A test-side instance is the
test's bug and gets corrected outright (F13's four). A production instance is
behaviour, and §7.6 S6 says parity, not repair.

### The fix, and why this one is an accommodation and not a repair

`2 * 256` became `2 * 256 + 16` — one luma row at the ping-pong's stride, the
smallest allocation that makes the arithmetic legal. It **cannot change an
encoded byte**: the extra 16 bytes are never read, never written, and never
addressed except by the one-past bump that this exists to keep in bounds.
Byte-exactness gates confirm it (341/341 both profiles, loopback and option
sweeps unchanged).

The alternative — stop the kernel bumping after its last row — is a change to a
**parked** family's raw body, and S12 is explicit that exact spans are for the
safe side and get restored on the raw side only at re-landing. Sizing the buffer
is the move that does not touch the kernel.

**It is temporary by construction.** If `common/sad_common.rs` re-lands from its
park, the shim hands the safe kernel an exact span and the safe kernel stops at
its last row, so the `+ 16` becomes dead and goes with it. Deleting the `+ 16`
today restores the UB, and Miri's `--lib` gate now catches it in
`svc_mode_decision::tests::test_wels_md_i16x16_cost` — the accommodation is
guarded by the instrument that found it.

### What this says about the gate

**Removing one Miri skip immediately exposed a second, unrelated defect
underneath it** — F13's `pfMdCost` was standing in front of F14, and no
instrument could see past it. That is the third time in this refactor that
widening an instrument paid out on the first run (F1 behind the release
segfault, seven defects behind the `--lib safe::` narrowing, and now this), and
it is the concrete argument for treating the skip list as a work queue rather
than a settled state: **each skip hides an unknown number of further defects,
not just the one it names.**

It is also the brief's prediction about parked code coming true on schedule.
`common/sad_common.rs` has now yielded latent UB **four** separate times — F10
three times, F14 once — and it is the family that has spent the longest
uninstalled with raw bodies live. Re-landing it is a safety goal, not only a
perf one.

---
