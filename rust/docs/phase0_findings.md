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
| debug | after the T5b deletion | 1200 | 0 | *(at `opt-level = 0`; see the fourth measurement — the fast debug build does fail)*

1 vs 3 out of 1200 is the same rate within Poisson noise, which is what
establishes that the T5b deletion did not cause it — the failure reproduces on
the committed tree with the change stashed. Every failure observed so far is on
`t=4 sm=3`, at both byte constraints (600 and 1500) and both entropy coders.
Isolated, the failing configuration ran 40/40 clean, so it needs the sweep's
back-to-back load to show up.

**Second measurement, 2026-08-07 (Phase 1 exit gate).** Three release sweeps that
session each failed exactly one `t=4 sm=3` configuration, which is well above the
rate above and initially looked like a regression. It is not. Eight `sweep.sh mt`
runs at HEAD were compared against eight at the session's control commit
`53e211f7`, release, same machine:

| tree | configurations | failures |
|---|---|---|
| `53e211f7` (control) | 960 | 1 |
| HEAD (Phase 1 complete) | 960 | 2 |

Same signature, same clip, both zero-byte. 1 vs 2 in 960 is noise; the finding
reproduces on an untouched tree, and Phase 1 changed no encoder code at all
(its whole codec diff is one `pub mod` line). Two refinements to the profile
above: **every failure observed that day was at `n=600`, none at 1500**, and the
rate rises noticeably when the machine is busy — the elevated readings came from
sweeps run alongside `cargo test`/Miri, the quiescent 16-run comparison above sat
back at ~1.5 per 1000. Both are consistent with a race whose window widens under
load. Practical consequence for future sessions: **do not run the release `mt`
sweep concurrently with other builds** if you want a clean single-run signal, and
keep applying the retry rule regardless.

**Third measurement, 2026-08-08 (Phase 2 session C, T5).** The session-end battery
failed one `t=4 sm=3` configuration, the re-run failed two *different* ones, and the
rate looked alarming enough to run the full R-g comparison. Six `sweep.sh mt` runs at
HEAD (`11f82d41`) **alternating in one loop** with six at the session control
(`82014c9d`), so both sides saw identical machine load:

| tree | runs | configurations | failures |
|---|---|---|---|
| `82014c9d` (control) | 6 | 720 | **4** |
| HEAD (T5) | 6 | 720 | **1** |

Across the whole session, 14 HEAD runs gave 5 failures and 12 control runs gave 4 —
36% vs 33% of runs, indistinguishable. The control fails *more often* in the equal-count
comparison, which settles it. **Alternate the two trees inside one loop**; the earlier
"HEAD 4/8 vs control 0/6" reading came from measuring them at different times under
different load, which for a load-sensitive race is not a comparison at all.

Two refinements to the profile, both from the control side:

- **It is not confined to `t=4`.** The control produced
  `mt CiscoVT2people_160x96_6fps t=2 sm=3 n=600 cabac=0 rc=0` (41938 vs 42462 bytes).
  Every prior observation was `t=4`; `t=2 sm=3` is now in the signature, so the retry
  rule should read **any `sm=3` release `mt` configuration**, not just `t=4`.
- **The output is not always zero-length.** Roughly half of this session's failures
  were short or simply different rather than empty (42462 vs 41938, 37837 vs 39981,
  40857 vs 42538). A "zero bytes" test is too narrow to recognise the finding.

Both are consistent with a race in slice-list growth: a lost slice gives short output,
losing the first gives none.

**Fourth measurement, 2026-08-10 (the diffharness profile change) — F3 now fires in
DEBUG, and the signature grows a clause.** The debug driver was built at
`opt-level = 0` because its `Cargo.toml` declared no profile; giving it
`opt-level = 3` (checks still on) took the debug sweep from 141s to 24s and made it
**fail two `t=4 sm=3 n=600` configurations, both zero-byte, on the first run**. That
is F3's fingerprint in a build where the recorded rate was 0-in-1200.

**This is not a new defect; it is this finding's own prediction coming true.** The
paragraph immediately below has said since T5b that debug's immunity is an artefact
of its slowness, not evidence of optimiser-induced UB. Remove the slowness and the
immunity goes with it, which is about as direct a confirmation of the race hypothesis
as this project has produced without a debugger.

Rate, by R-g's alternating protocol (`sweep.sh mt`, three rounds each, the two debug
builds swapped in one loop so both see identical load):

| debug build | runs | configurations | failures |
|---|---|---|---|
| `opt-level = 3` (fast) | 3 | 360 | 0 |
| `opt-level = 0` (stock) | 3 | 360 | 0 |

So: 2 events in the first run and none in 480 further fast-debug configurations. Not
enough to state a rate, and the two events came directly after several optimising
builds — the least quiescent the machine had been, which the second measurement above
already identified as the condition that widens the window.

**Consequence for the retry rule, decided and applied:** the rule now reads **any
`sm=3` `mt` configuration in either profile**, not release only. The debug sweep loses
its status as the one gate with no exceptions; that was the price of the 5.9x, taken
knowingly. Full reasoning and the alternatives rejected: `rust/docs/diffharness_perf.md`.

**One upside worth naming.** F3 has only ever been reproducible in release builds,
which is the worst possible instrument for debugging a race — no assertions, poor
symbols. There is now a **debug** reproducer with `debug_assert!` live, full symbols,
and a 6x faster edit-run loop. Whoever takes plan §P10's `ReallocateSliceList`
hypothesis should start there.

**Do not read "release fails, debug doesn't" as proof of optimiser-induced UB
here.** A debug build is several times slower, which widens every thread window;
a race can simply never lose in debug. What the shape *does* point at is a data
race in the one encoder path that combines threading with a data structure that
grows during encoding — the slice list. Plan §P10 already flags
`ReallocateSliceList` (`svc_encode_slice.rs:2520+`) as invalidating outstanding
`SSlice` pointers, and §2.2.7/T9 covers the threading. This is very likely the two
interacting.

**Fifth measurement, 2026-08-10 (Phase 2 session E, T6).** The session-end battery's
release `mt` sweep failed exactly one configuration —
`Static_152_100 t=4 sm=3 n=600 cabac=0 rc=0`, Rust output **zero bytes** (C++
29375) — and the re-run of the full release `mt` preset passed **120/120**. One hit,
so the retry rule applied and the alternating-loop comparison did not. Signature
match is exact. Context consistent with every prior event: the battery ran after a
session of continuous optimising builds plus two concurrent background bench runs —
the least quiescent state this machine reaches. The session's three earlier full
sweep batteries (two mid-session, one dedicated to the F1 surgery) all passed
341/341 first try.

**Sixth measurement, 2026-08-10 (Phase 2 session F, T7 part 1) — the wrong
bitstream can be LONGER, not only short or zero.** The F-1 commit-B release sweep
failed one configuration — `CiscoVT2people_160x96 t=2 sm=3 n=600 cabac=0 rc=0`,
Rust **42462** bytes against C++ 41938 — which matches F3's configuration class
exactly but not its recorded output shape: every prior event was zero-byte or
short, this one is 1.2% *longer*. Because the shape was outside the signature, the
full attribution protocol ran instead of the one-hit retry: the exact configuration
re-run **12/12 clean on the swapped tree and 12/12 on the pre-swap tree,
alternating in one loop**; a single full release `mt` preset re-run cleared the
original config but produced one classic short-output hit at a *different*
config (`t=4 sm=3 n=600 rc=1`, Rust 40857 vs C++ 42538) — two hits in one session,
so the two-hit protocol ran: three full release `mt` presets per side, alternated
in one loop, read **pre-swap 0 failures / 360 configs, swapped 1 / 360** —
indistinguishable rates at F3's known frequency, on a commit that touches no MT
machinery and whose kernels are proven byte-identical differentially. Verdict: F3, with the **signature's output clause
widened — any wrong-length output at `mt` `sm=3`, `t` in {2,4}, either profile, is
the fingerprint**; a divergence at matching *length* has still never been seen.
Consistent with a slice-list race repacking slices rather than truncating them.

The same session's end battery added two more data points, both `t=4 sm=3 n=600`,
both cleared by a 120/120 preset re-run: a release hit that was again *longer*
(42616 vs 42538 — the widened shape's second sighting), and **the first debug-build
hit ever observed** (`320x192 cabac=1 rc=0`, Rust short at 37837 vs 39981) —
the manifestation the fourth measurement predicted when the debug driver went to
`opt-level = 3` but had not yet seen. Day's tally: 4 hits in ≈430 `mt sm=3`
encodes (≈1/110, against the historical 1/400-1000), on a machine that spent the
whole session under build + bench + Miri load — the known widening condition —
with the alternating-loop counts (pre-swap 0/360 vs swapped 1/360) showing the
rate is the day's, not the commit's.

**Seventh measurement, 2026-08-10 (Phase 2 session G, T7 part 2 + T9) — the
day-rate, measured twice, on a session whose control commit changed no code.**
This session ran the alternating-loop protocol **twice**, and the useful result is
the pair of counts rather than either one:

| loop | pre-change side | post-change side |
|---|---|---|
| G-1 (26 intra-predictor shims) | **3 failures / 360 configs** | **2 / 360** |
| G-2 (encoder deblocking dedup) | **1 / 360** | **2 / 360** |

Three release `mt` presets per side per loop, alternated inside one loop, binaries
kept on disk (the pre-change side built from a worktree at the commit-A hash).
Neither loop separates the sides, and in the first one the *raw* side was the
higher. Whole-day tally: **17 hits in ≈840 `mt sm=3` encodes, ≈1/49** — roughly
2.2x session F's ≈1/110 and 10-20x the historical 1/400-1000 — on a machine that
spent the session running builds, two bench pairs, four Miri passes and two
worktree compiles.

The attribution argument that makes this session's data worth more than its
predecessors': **two of the day's hits landed on the session's *control* commit,
which touches one markdown file and no code at all.** The elevation was therefore
present before the first kernel moved, which is what the alternating loops then
confirmed at both commits. Machine load is the widening condition; the commits are
not.

Every one of the seventeen matched the signature as widened by the sixth
measurement — `mt`, `sm=3`, `t` in {2, 4}, output of any wrong length, either
profile. The shapes seen today: zero-length (most), short, and long. No hit at any
other configuration class, in either profile, all day.

**Gate consequence, act on this now:** a single `sweep.sh mt` release run is not a
reliable 341/341 signal. A failure confined to `t=4 sm=3` must be re-run before
being treated as a regression, and a *new* failure anywhere else should be treated
as real immediately. `gates.sh` should say this where it prints the sweep result
rather than leaving each session to rediscover it.

**Eighth measurement, 2026-08-10 (Phase 4a session A, the F13/F14 commit) — the
first session where the post-change side is cleaner than the control, and the
day-rate falls back to its historical band.** The family gate's release sweep hit
once (`mt CiscoVT2people_160x96_6fps t=4 sm=3 cabac=1 rc=1`, 41989 vs 42281
bytes — short). Three re-runs of the `mt` preset gave 0, 1, 0, the single hit on
a *different* stream (`CiscoVT2people_320x192_12fps`, rc=0). Two hits in a
session is S14's escalation trigger, so the alternating loop ran:

| side | failures / configs |
|---|---|
| HEAD (mc de-virtualization + `CostFamily` + F14's `+ 16`) | **0 / 600** |
| control `2f65a765` (pre-4a) | **2 / 600** |

Five rounds, `mt` release preset both sides, alternated inside one loop, control
`rust_enc` built from a worktree at the pre-phase commit and the two binaries
swapped at `compare.sh`'s fixed path (md5-checked distinct). **The control side
hit and the changed side did not** — the fourth consecutive loop in which the
sides do not separate, and the first in which the direction favours the changed
tree.

Whole-session tally on HEAD: **2 hits in ≈1080 `mt sm=3` encodes, ≈1/540** —
back inside the historical 1/400-1000 band and ~11x quieter than session G's
≈1/49. The machine ran benches and Miri today as it did then, so this is not a
clean quiescence claim; what it does say is that the elevation session G measured
is not monotonic and not commit-attributable.

Both hits matched the signature exactly (`mt`, `sm=3`, `t=4`, wrong length,
release). No hit at any other configuration class, either profile, all session.

**Nothing in this session's changes could plausibly reach it**, which is worth
stating because the changes were large: `SMcFunc` de-virtualization, an enum
replacing two interior pointers, and a 16-byte allocation increase. None touches
threading, slice partitioning, or `ReallocateSliceList` — the two known-UB
mechanisms F12's note names as sitting under F3 are both untouched and both
still live.

**Who fixes it:** Phase 6.4 (`Vec<SliceState>` + indices, P10) and Phase 7 (the
threading rework) between them delete the mechanism. Phase 7's exit gate already
demands MT determinism across thread counts; this finding says that gate must be
run *repeatedly*, not once, because the bug's natural frequency is well below one
sweep. Until then, do not add `t=4 sm=3` results to any pass/fail automation
without a retry.
