# Phase 0 findings

Things found while executing Phase 0 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
that are *not* Phase 0's job to fix. Recorded here so no later session has to rediscover
them. Fuzzer crash artifacts live separately in [`fuzz_findings.md`](fuzz_findings.md).

---

## F1 — The `eb463dbd` release segfault: root-caused (fixed at HEAD by accident)

**Status: closed.** Reproduced, root-caused, and confirmed fixed in-tree. Task T3.

### What it actually was

`DeblockingMbAvcbase` (`encoder/deblocking.rs`) declared its boundary-strength scratch
buffer as

```rust
let mut uiBS: [[u8; 4]; 4] = [[0; 4]; 4];        // 16 bytes
```

where the C++ (`codec/encoder/core/src/deblocking.cpp:629`) declares

```c
uint8_t uiBS[2][4][4];                            /* 32 bytes */
```

— two 4×4 planes, one for vertical edges and one for horizontal. `DeblockingBSCalc_c`
writes the *second* plane through five separate

```rust
let slice = std::slice::from_raw_parts_mut(uiBS as *mut u8, 32);
```

sites, and `DeblockingInterMb` reads all 32 bytes back. So every inter macroblock wrote
16 bytes past the end of a 16-byte stack array: **a stack buffer overflow on the
mainline encode path**, executed thousands of times per frame.

### Why it looked like a null `pNonZeroCount`

It didn't corrupt anything *reachable* in most builds — the overflow landed in stack
padding or in slots the optimizer had already finished with. The observed crash site
(`WelsNonZeroCount_c` dereferencing a null `(*pCurMb).pNonZeroCount`, recorded in plan
§1.4) was a **symptom of the smashed frame, not the cause**. Instrumenting the real
crash showed `pNonZeroCount` was *never* null across 378 encoder configurations; the
faulting build died earlier, in `DeblockingFilterFrameAvcbase`, on
`EXC_BAD_ACCESS address=0x8` — a null-plus-8 load of a pointer pair that the overflow
had zeroed.

That is the signature of this bug class: **the crash site is wherever the optimizer
happened to park a live pointer next to the buffer**, so it moves with codegen and
tells you almost nothing about the cause.

### Why it "stopped reproducing"

It never stopped. It is layout-sensitive, and the pristine `eb463dbd` release build
lands the overflow somewhere harmless:

| build | result |
|---|---|
| `eb463dbd` release, pristine | clean, 378/378 configurations |
| `eb463dbd` release, `-C codegen-units=1`, `-C opt-level=1/2`, `-C lto=fat` | clean |
| `eb463dbd` debug | clean |
| `eb463dbd` release **+ an `eprintln!` added to `deblocking.rs`** | **SIGSEGV, 5/5 runs, deterministic** |
| HEAD release, same instrumentation | clean, 378/378 |
| HEAD release, same instrumentation, `uiBS` narrowed back to 16 bytes | **SIGSEGV, 5/5 runs** |

The last row is the proof: reintroducing the narrow array at HEAD reintroduces the
crash, and nothing else was changed. Optimiser flags alone were not enough to tip it —
only a source edit that shifted the stack frame was.

### Where the fix came from

`e6fce464` ("encoding benchmark") widened `uiBS` to `[[[u8; 4]; 4]; 2]` and switched the
two call sites to `as_mut_ptr()`/`as_ptr()`. The commit message does not mention it. So
the fix landed **incidentally, one commit after the crash was observed**, which is
exactly why "it no longer reproduces" read as a mystery.

Trigger configuration for the record: `SM_SIZELIMITED_SLICE` (slice mode 3, byte
constraints 1500 and 600), single-threaded, 320×240 smptebars, 200 frames, explicit gate
params — `rust_enc <yuv> 320 240 200 26 <cabac> -1 <out> 0 0 3 1500 1`.

### What to carry forward

1. **A debug/release disagreement is UB evidence, not flakiness.** Plan §7.2 gate 0
   already says this; this is the worked example. Both profiles are now gated
   (`RUST_ENC_PROFILE`, `da5c06ae`).
2. **"It stopped reproducing" is not a resolution.** Two commits of "it's fine now" sat
   on top of a live stack-smash.
3. **Grep the port for array-size disagreements with the C++.** This one survived
   because the *type* was plausible and only the raw-pointer reinterpretations knew the
   real size. Phase 2 (R2 kernel designature) and Phase 5/6 delete this class outright by
   giving these buffers real types — `[[[u8; 4]; 4]; 2]` cannot be written 32 bytes deep
   by accident once the writers take `&mut [[[u8; 4]; 4]; 2]` instead of `*mut u8`.
4. **The five `from_raw_parts_mut(uiBS as *mut u8, 32)` sites are still there** and still
   the only thing enforcing the size relationship. They are correct today. Phase 2 should
   remove them rather than re-verify them.

---

## F2 — The encoder bitstream writer exists four times, and the copies are not identical

**Status: open, do not dedupe yet.** Task T5d, per the phase brief's stop rule.

The Phase 0 brief expected `svc_encode_slice.rs:498-585` to be a verbatim duplicate of
`vlc_encoder.rs`, to be deduped by re-pointing callers. Neither half of that holds.

There are **four** definitions of the `Bs*` writer family, one per module, and each
module's callers bind to its own local copy:

| module | `BsWriteBits` | notes |
|---|---|---|
| `encoder/vlc_encoder.rs:367` | canonical | matches C++ `bit_stream.h`; used by `au_set.rs`, `encoder_ext.rs` |
| `encoder/svc_set_mb_syn_cavlc.rs:157` | equivalent | hand-written 4-byte store instead of `WRITE_BE_32`; `(1u32 << iLen).wrapping_sub(1)` |
| `encoder/nal_encap.rs:169` | equivalent | explicit `if iLen == 0 { 0 }` where the canonical relies on `(1 << 0) - 1 == 0` |
| `encoder/svc_encode_slice.rs:509` | **divergent guards** | see below |

The `svc_encode_slice.rs` copy differs behaviourally:

- it returns early on a null `pBs` or `iLen <= 0`, where the canonical would deref null
  or shift by a negative amount;
- it **pre-masks** `kuiValue` to `iLen` bits (`kuiValue & ((1u64 << iLen) - 1) as u32`),
  which the canonical does not — so a caller passing bits above `iLen` gets different
  output from the two copies;
- it branches on `iLeftBits >= iLen` and flushes when `iLeftBits` reaches 0, where the
  canonical branches on `iLen < iLeftBits` and flushes in the else arm. These converge to
  the same state on the `iLen == iLeftBits` boundary — checked by hand, both end with the
  word written, `uiCurBits = 0`, `iLeftBits = 32`;
- `BsWriteUE` uses `wrapping_add(1)` where the canonical uses `+ 1` (debug-build overflow
  panic vs wrap).

On in-contract inputs all four agree, which is why the encoder is byte-identical to the
C++ across 341 sweep configurations. The divergence is confined to guards, masking, and
overflow behaviour.

**Why not dedupe now:** Phase 0 changes no behaviour, and collapsing four
not-quite-identical functions onto one is a behaviour change on exactly the edge cases
nobody has tested. Phase 3 (§Phase 3.2) converts the write side to a safe `BsWriter`
anyway; that is the commit where the four copies become one, and it must decide
explicitly which guard semantics survive. Record the decision there.

Related: the recovered `rust/tools/find_dup_types.sh` reports 268 duplicated declarations
today, including constants that hold **two distinct values** under one name. It was
written after `SBitStringAux` was found duplicated four times with a wrong-constness field
and `g_kuiGolombUELength` three times with two wrong copies. Duplication of this kind is a
standing defect class in this port, not an incident.

---

## F3 — The multi-threaded dynamic-slicing encoder is nondeterministic in release builds

**Status: open, unfixed, and it makes one gate unreliable.** Found during T5b.

Roughly 1 in 400–1000 encodes of a `iMultipleThreadIdc=4` + `SM_SIZELIMITED_SLICE`
configuration produces the wrong bitstream. Two failure shapes were seen:

- a **zero-byte** output while `rust_enc` still exits 0, and
- a **short but non-empty** output — `Rust : 41946 bytes` against `C++ : 42538`,
  i.e. a genuinely different bitstream, not a truncated write.

Measured rates, 120-configuration `sweep.sh mt` runs, same machine, same tree:

| profile | tree | configurations | failures |
|---|---|---|---|
| release | before the T5b deletion (stashed) | 1200 | 1 |
| release | after the T5b deletion | 1200 | 3 |
| debug | after the T5b deletion | 1200 | 0 |

1 vs 3 out of 1200 is the same rate within Poisson noise, which is what
establishes that the T5b deletion did not cause it — the failure reproduces on
the committed tree with the change stashed. Every failure observed so far is on
`t=4 sm=3`, at both byte constraints (600 and 1500) and both entropy coders.
Isolated, the failing configuration ran 40/40 clean, so it needs the sweep's
back-to-back load to show up.

**Do not read "release fails, debug doesn't" as proof of optimiser-induced UB
here.** A debug build is several times slower, which widens every thread window;
a race can simply never lose in debug. What the shape *does* point at is a data
race in the one encoder path that combines threading with a data structure that
grows during encoding — the slice list. Plan §P10 already flags
`ReallocateSliceList` (`svc_encode_slice.rs:2520+`) as invalidating outstanding
`SSlice` pointers, and §2.2.7/T9 covers the threading. This is very likely the two
interacting.

**Gate consequence, act on this now:** a single `sweep.sh mt` release run is not a
reliable 341/341 signal. A failure confined to `t=4 sm=3` must be re-run before
being treated as a regression, and a *new* failure anywhere else should be treated
as real immediately. `gates.sh` should say this where it prints the sweep result
rather than leaving each session to rediscover it.

**Who fixes it:** Phase 6.4 (`Vec<SliceState>` + indices, P10) and Phase 7 (the
threading rework) between them delete the mechanism. Phase 7's exit gate already
demands MT determinism across thread counts; this finding says that gate must be
run *repeatedly*, not once, because the bug's natural frequency is well below one
sweep. Until then, do not add `t=4 sm=3` results to any pass/fail automation
without a retry.
