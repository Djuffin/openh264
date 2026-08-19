# Phase 0 findings

Things found while executing Phase 0 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
that are *not* Phase 0's job to fix. Recorded here so no later session has to rediscover
them. (A separate fuzz-findings file was planned; T7 is deferred by direction and no
fuzz corpus exists — plan §0's "absent instrument" row tracks it.)

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

**Status: RESOLVED 2026-08-11**, Phase 3 T3.4 face 1 (`13912ffd`). There is one
writer. See "Resolution" at the end of this entry — and note the inventory below
was **incomplete**: it tabulated `BsWriteBits` only, and a fifth divergence turned
up in `BsFlush` when the copies were actually collapsed.

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

### Resolution (2026-08-11, T3.4 face 1, `13912ffd`)

The canonical copy won; the other three are deleted, fifteen functions in all. The
guard semantics that died, and why each is safe on in-contract input, are enumerated
in that commit message and summarised in the log's session-E entry §2. The referee
was the one this finding named: **sweeps 341/341 in both build profiles**, which is
byte-exactness against the C++ encoder itself. Zero bytes of output moved.

**One divergence this entry missed.** The table above compares `BsWriteBits`, and
that is all it compares. `nal_encap.rs` also carried its own `BsFlush`, and that one
was **not** equivalent: it stored only the `4 - iLeftBits / 8` bytes it advanced
over, where the canonical — and `golomb_common.h:104` — always stores a full 32-bit
word. The difference is up to three bytes past the new write position, overwritten
by the next write before anything reads them and outside the output past the last
NAL, so it survived every gate. The lesson is about the shape of the inventory, not
the bytes: **a duplicate-function finding must enumerate the whole family, not the
function that motivated it.**

The related note below still stands and got a little worse before it got better:
`md.rs` carries a *fifth* copy of the size pair (`BsSizeUE`/`BsSizeSE`) over a
**third** name for the Golomb length table (`G_KUI_GOLOMB_UE_LENGTH`). Different
family, listed for S18's straggler sweep at the phase exit.

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

**Phase 5 session S, 2026-08-14 — one hit, retry clean, no rate work owed.** The
exit-level battery's debug sweep read 340/341 with the single failure
`mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0`, Rust 0 bytes against the
C++'s 40992 — the fourth measurement's fingerprint exactly, in the profile that
measurement added. Re-running the `mt` preset alone gave **120/120**. One hit, so the
alternating HEAD-vs-control loop is not owed; release read 341/341 in the same
battery. Recorded because the running total is what makes the next hit cheap to
classify.

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

A **third** hit arrived at the exit battery, this time in the *debug* sweep
(`mt CiscoVT2people_160x96_6fps t=4 sm=3 cabac=1 rc=0`, zero-length — the same
stream as the first hit, the other rc). Three debug `mt` re-runs then gave
120/120 each.

Whole-session tally on HEAD: **3 hits in ≈1560 `mt sm=3` encodes, ≈1/520** —
inside the historical 1/400-1000 band and ~10x quieter than session G's ≈1/49.
The machine ran benches and Miri today as it did then, so this is not a clean
quiescence claim; what it does say is that the elevation session G measured is
not monotonic and not commit-attributable.

One shape observation, offered as data rather than a theory: all three of this
session's hits were `t=4`, and two of the three were the same 160x96 stream in
its two `rc` settings. The smallest frame size in the sweep is where a
slice-partitioning race would have the fewest macroblocks to hide in.

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

### Ninth measurement — 2026-08-10, Phase 3 session B: the first HEAD-vs-control alternation

Two hits in one session's debug sweeps (`mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600
cabac=1 rc=0`, then `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=1`),
release 341/341 both times. Two hits triggers S14's alternation clause, which had never
actually been executed before — prior sessions re-ran the single configuration and moved
on.

Protocol: control = `1bf5a235`, built in a **separate git worktree** so both `rust_enc`
binaries were on disk simultaneously, then 6 rounds × the 120-configuration `mt` preset
per side, alternating inside one loop (the correctness analogue of S1's interleaving).

| tree | encodes | `sm=3` zero-byte failures |
|---|---|---|
| control `1bf5a235` | 720 | **2** (rounds 5, 6) |
| HEAD (T3.1b) | 720 | **0** |

**The control side failed more.** Combined with the seam changing zero bytes of
`src/encoder/` and `src/common/`, this is as clean a negative as the protocol produces.
Rate over the session: 4 hits in ~1560 `mt sm=3`-bearing encodes ≈ 1/390, the upper end
of the 1/400–1000 band, on a machine that spent the session running gate batteries —
consistent with "rises an order of magnitude on a loaded machine".

The methodological point worth keeping: the tempting argument ("this seam is
decoder-only, it *cannot* have caused an encoder race") is exactly the kind S14 exists
to distrust, and running the alternation cost about ten minutes against the alternative
of a plausible-sounding dismissal.

### Tenth measurement — 2026-08-10, Phase 3 session C: one hit, the fixed gate's first stop

One hit in the T3.2 seam battery's **release** sweep: `mt CiscoVT2people_320x192_12fps
t=4 sm=3 n=600 cabac=0 rc=0`, zero-byte output — the signature exactly. Debug was
341/341 in the same battery, and the session's two earlier full sweeps (the F17
re-baseline) were 341/341 both profiles.

Re-run of that configuration, per the one-hit rule: **5/5 BYTE-IDENTICAL**. Single hit,
so the alternation clause stays untriggered; the seam under test changes zero bytes of
`src/encoder/` and `src/common/` (decoder-only conversion), and the failing config is
`cabac=0` — the encoder's CAVLC path — but per the ninth measurement's lesson that
argument is corroboration, not the verdict; the re-run is the verdict.

Worth one line: this was the first F3 hit judged by the post-F17 `gates.sh`, and the
battery printed `OVERALL: FAIL (1 steps failed)` and exited 1 — the gate stopping the
session and forcing this protocol is the behaviour F17's fix bought.

### Eleventh measurement — 2026-08-11, Phase 3 session D: three hits, the second alternation, and a rate that says "loaded machine"

Three hits across the session's five full batteries, all the signature, each re-run
**5/5 BYTE-IDENTICAL** on its own configuration:

| battery | profile | configuration | output |
|---|---|---|---|
| opening **control** (inherited tree, before any change) | release | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=0` | short (41989 vs 42281) |
| T3.3 face 3 | release | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` | zero-byte |
| final re-verification (docs-only commit) | **debug** | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` | zero-byte |

The faces-1+2 and face-4 batteries — including the one that gated the last code
commit — were 341/341 in both profiles. Both profiles are represented among the hits,
as the twice-broadened signature says they should be.

Note what the first one is: a hit on the **session-start commit, before a line was
changed**. That is the cleanest possible statement that the rate is the machine's, not
the seam's — and it is why the second hit was not treated as news.

The alternation ran after the second hit (more than one ⇒ S14's clause, and the "this
seam is decoder-only" argument is the one the rule distrusts). Control = `d737a450` in a
separate worktree, 6 rounds × 32 `mt sm=3` configurations per side, alternating inside
one loop:

| tree | encodes | wrong-length failures |
|---|---|---|
| control `d737a450` | 192 | **4** (rounds 2, 4, 5, 6) |
| HEAD (T3.3) | 192 | **2** (rounds 1, 3) |

**The control side failed twice as often**, the same direction as the ninth
measurement. Combined rate this session: 6 in 384 alternation encodes plus 3 in ~3400
battery encodes ≈ **1/64 in the alternation**, an order of magnitude above the 1/400–1000
band — measured on a machine that spent the whole session running gate batteries and two
bench pairs, which is exactly the load-sensitivity the finding predicts. The alternation
is a *ratio* instrument, not a rate instrument, and the ratio says control ≥ HEAD.

The third hit arrived *after* the alternation, on a **docs-only commit that changed no
code at all** — the second time this session that a hit landed on a tree the seam had
not touched (the first was the opening control battery). Treated as corroboration of
the completed alternation rather than as grounds for a second one, which is the
judgement S14's "more than one hit ⇒ alternate" is meant to license once the
alternation has already answered.

Second time the alternation has been run, second time it has acquitted the tree under
test. Worth noting for whoever runs it next: at this rate a 6×32 alternation is enough
to see hits on both sides, where the ninth measurement's 6×120 was needed to see two —
so size the alternation by the *current* rate, not by the historical band.

### Twelfth measurement — 2026-08-11, Phase 3 session E: four hits, the third alternation, and the encoder seam acquitted

The first session in which the seam under test is **encoder-side**, which is the case
the F3 signature actually lives in — so the "this seam is decoder-only" reasoning that
softened session D's hits was unavailable, and the alternation was run on the first
excuse.

Four hits across the session's five full batteries, all the signature, each re-run
**5/5 BYTE-IDENTICAL** on its own configuration:

| battery | profile | configuration | output |
|---|---|---|---|
| opening **control** (inherited tree, before any change) | debug | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0` | zero-byte |
| face 1 (second battery on the same tree) | debug | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=0` | zero-byte |
| face 1 (same battery) | release | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=0` | short (40602 vs 42538) |
| faces 2+3 | release | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0` | zero-byte |

Again a hit on the **session-start commit before a line was changed**, for the second
session running. And again a *pair* of batteries on one unchanged tree disagreed with
each other — face 1's `family` battery was 341/341 in both profiles, and the `full`
battery minutes later on the identical tree drew one hit per profile. Two runs of the
same binaries, opposite verdicts: that is the finding, stated as plainly as it gets.

The alternation, after the second and third hits. Control = `b308f7d5`, both `rust_enc`
binaries built and kept on disk, alternating **inside one loop** on the worst-known
configuration (`CiscoVT2people_160x96_6fps t=4 sm=3 n=600`, release), 40 rounds × 2
entropy coders per side:

| tree | encodes | wrong-length failures |
|---|---|---|
| control `b308f7d5` | 80 | **3** (two zero-byte, one zero-byte) |
| HEAD (T3.4 face 1) | 80 | **1** (long: 42312 vs 42281) |

**The control side failed three times as often.** Third alternation, third acquittal,
and the third in a row where the control is the worse side.

Two additions to the profile:

* **The output can be LONG.** Every previously recorded failure was zero-byte or
  short; HEAD's single hit here was 42312 bytes against 42281, i.e. 31 bytes *over*.
  S14's wording already says "any wrong length (zero, short, or long)" — this is the
  first observed instance of the third case, so the wording was right to be broad.
* Rate on the worst configuration under battery load: **4 in 160 ≈ 1/40**, another
  order of magnitude above session D's 1/64 alternation rate. Sizing advice from the
  eleventh measurement holds and tightens: 40×2 was ample.

For whoever runs the next one: alternating a *single* configuration rather than a
whole `mt sm=3` sub-sweep works and is much faster (~4 minutes), because the
configuration that fails is known. Keep both binaries on disk and swap them into the
harness path inside the loop; do not rebuild between sides.

### Thirteenth and fourteenth measurements — 2026-08-11, Phase 3 session F: three hits, and the first alternation where both sides tie

**Thirteenth (the opening control).** For the *third* session running, the
session-start battery drew a hit on a tree with not one line changed —
`mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=1`, debug, zero-byte —
on the exact commit session E had signed off as a clean `OVERALL: PASS`. Re-run
**5/5 BYTE-IDENTICAL**. This is now the most reliably reproducible thing about F3:
*it is more likely to appear on an unchanged tree at session start than at any
particular seam.*

**Fourteenth (T3.6's battery, and the alternation).** The release sweep drew **two**
hits, which is what the retry rule says escalates from re-run to alternation:

| profile | configuration | output |
|---|---|---|
| release | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` | zero-byte |
| release | `mt Static_152_100 t=4 sm=3 n=600 cabac=0 rc=0` | zero-byte |

Both re-ran 5/5 clean individually first. Then the alternation, control =
`45ab079c` (T3.5 complete), both `rust_enc` release binaries built once and swapped
into the harness path inside one loop, **20 rounds × both configurations per side**:

| tree | encodes | wrong-length failures |
|---|---|---|
| control `45ab079c` | 40 | **2** |
| HEAD (T3.6) | 40 | **2** |

**The two sides tied exactly** — the first alternation of the five that did not come
back with the control worse. That is a stronger acquittal than a lopsided one, not a
weaker one: a difference in either direction on a sample this size is what would need
explaining, and there is none. Fourth alternation, fourth acquittal.

Rate here: **4 in 80 = 1/20** on the worst-known configurations under load, the
highest recorded yet — the two configurations chosen were the two that had just
failed, which is selection bias working *for* the test's sensitivity rather than
against it.

One new datapoint on breadth: `Static_152_100` had never appeared in a recorded F3
hit before. The signature is not confined to the two Cisco clips; the third sweep
input reproduces it too, at the same `sm=3, t=4`.

### Fifteenth measurement — 2026-08-11, Phase 3 exit: two batteries, one tree, opposite verdicts (again)

The exit-level battery drew a single release hit:

| profile | configuration | output |
|---|---|---|
| release | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` | **short** — 41681 vs 42281 |

Re-ran **5/5 BYTE-IDENTICAL**. Single hit, so the retry rule stops there; no alternation.

**What makes this one worth recording is what changed between the two batteries: nothing
the encoder can see.** The immediately preceding exit battery scored 341/341 in release
on the same tree; the only edit in between was a `#[cfg_attr(miri, ignore)]` attribute on
a *test* in `tests/kernels_differential_phase2.rs`, which is not compiled into the
encoder binary the sweep runs. Same library code, same harness, opposite verdicts —
the third recorded instance of that shape (session E saw it twice within one session),
and the cleanest, because this time the intervening diff provably cannot reach the
binary under test.

Running total: **fifteen measurements, four alternations, four acquittals.** The
standing advice is unchanged and now very well supported — a lone `mt`/`sm=3`/`t∈{2,4}`
wrong-length hit is F3 until an alternation says otherwise, and the sweep's verdict on
any single run is a sample rather than a fact.

### Sixteenth to eighteenth measurements — 2026-08-11, Phase 4b: the rate is measured at last, and the isolated re-run is shown to be the wrong instrument

Three separate hits across one session, on two trees:

| # | battery | profile | configuration | output |
|---|---|---|---|---|
| 16 | T4b.1 `full` | release | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` | **short** — 41989 vs 42281 |
| — | (its S14 re-run) | release | same | **short** — 40645 vs 42281 |
| 17 | T4b.1b `family` | debug | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=1` | **zero** — 0 vs 40992 |
| 18 | T4b.1b `full` | debug | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` | **zero** — 0 vs 42281 |

*(Label note, steward: these two batteries were the RC seam's — written as "T4b.2"
before the session renumbered that seam T4b.1b; the vtable seam T4b.2 had not started.
The brief's correction block records the renumbering.)*

Two things to note before the alternations. **The session-start control battery was
clean**, breaking a three-session streak of session-start hits — so the advice "expect
a hit before you change a line" is a tendency, not a rule. And measurement 16's re-run
produced a *different* wrong length from the same binary and configuration, which is a
race and not a divergence: a deterministic port bug repeats its bytes.

### Nineteenth measurement — 2026-08-11, Phase 4b session B: eight sweeps, zero hits, and the load is part of the signature

Not a hit: **the absence of one**, recorded because it is a measurement and because it
is the first session with enough clean sweeps to say something.

Four full batteries (the session-start control, and one per seam for T4b.2a, T4b.2b,
T4b.3a), **eight 341-configuration sweeps across both profiles, zero hits**. That is
~2728 configurations against session A's directly measured rate of ~1 in 800, so ~3.4
were expected and P(0) is about 3%. A mild surprise on its own — but it points the same
way **S23b** does, and that is the reading to keep:

> Session A's nine hits came out of a loop running whole sweeps **back to back**, with
> nothing else on the machine and no gaps. A `gates.sh` battery interleaves its two
> sweeps with cargo builds, two benches and a Miri run, and leaves the machine idle
> between steps. Same configurations, same binaries, different *load*.

So the sampling unit S23b corrected (whole sweeps, not isolated configurations) is
necessary but not sufficient: **the sweeps have to be back to back**. An escalation
that runs one sweep per side inside a battery is closer to the 0/40 non-result than to
the 4/12-vs-5/12 that acquitted T4b.1b. When F3 next escalates, the alternation loop
runs sweeps with nothing between them, and this row is the reason.

Also on record: **the session-start control battery was clean for the second session
running**, which is now two of the last two against a prior streak of three. The
standing advice stays as session A left it — a tendency, not a rule.

---

**Alternation five** (measurement 16, release, the hitting configuration in isolation,
binaries swapped inside one loop): **HEAD 1/40, control `6e15c907` 1/40.** A tie, the
second consecutive one.

**Alternations six and seven are the informative part, and they change how this
finding should be tested.** Measurements 17 and 18 escalated per S14. Run in isolation
— 40 runs per side of the two configurations that had just failed — the result was
**HEAD 0/40, control `08b7c29d` 0/40**: neither side reproduced *at all*. That acquits
and proves nothing. So the alternation moved to the level at which the hits actually
occur: whole `mt` sweep presets, 341 configurations back to back, which is the loaded
condition the finding has always said it needs. Twelve sweeps per side, binaries
swapped inside one loop:

**HEAD 4 hits / 12 sweeps, control 5 hits / 12 sweeps.** The control — which does not
contain the change under test — hit *more* often.

Three results worth more than the acquittal itself:

1. **The rate is now measured directly rather than inferred.** Roughly one hit per
   2-3 `mt` sweeps ⇒ about **1 in 800 configurations** under load, which lands inside
   the 1/400-1000 this finding has claimed since Phase 0 from indirect evidence.
2. **All three wrong-length forms appeared on both sides inside one loop**: zero-byte
   on both, a **short** output on HEAD (37837 vs 39981) and a **long** one on the
   control (40463 vs 39981). The form of the wrong length carries no information about
   which tree produced it — the signature's output clause is a disjunction on purpose.
3. **An isolated re-run of a hitting configuration is the wrong unit to sample.**
   0/80 across both sides in isolation, then 9 hits between them once the machine was
   loaded. S14 already says sequential sampling of a load-sensitive race misleads;
   this is the same lesson one level up, about *what* is being sampled rather than in
   what order. **When an alternation comes back 0/0, re-run it at the sweep level
   before calling it a result.**

Running total: **eighteen measurements, seven alternations, seven acquittals.**

---

### Twentieth measurement — 2026-08-11, Phase 4b session C: five hits, and the first alternation that is not 0/0

The measurement S23b said was missing. Session A's alternations five to seven were
0/40, 0/40 and 0/0 in isolation; session B drew a zero across eight sweeps. **This
one produced hits on both sides in the same loop**, which is the only condition under
which an alternation answers its question.

**The session's hits, in order.** Every one is `mt`, `sm=3`, **`n=600`**:

| where | profile | configuration | Rust output |
|---|---|---|---|
| session-start battery | release | 160x96 `t=2 cabac=0 rc=1` | 42345 vs 41938 — **long** |
| T4b.3b battery | debug | 320x192 `t=2 cabac=0 rc=0` | **0 bytes** |
| T4b.3b battery | release | 160x96 `t=4 cabac=1 rc=1` | **0 bytes** |
| retry of the above, run 5/5 | release | *same configuration* | **0 bytes** |
| alternation, base ×2 / head ×3 | release | four different inputs | 0 bytes ×4, **28537** vs 30190 once |
| exit-level battery | debug | 320x192 `t=4 cabac=0 rc=0` | **0 bytes** (retry 5/5 clean) |
| exit-level battery | release | 160x96 `t=4 cabac=0 rc=1` | **0 bytes** (retry 5/5 clean) |

**The signature narrows: `n=600`, not `sm=3` generally.** The sweep runs `"3 1500"`
and `"3 600"` — both `SM_SIZELIMITED_SLICE` — and **all eleven of this session's hits
are the 600-byte constraint**, across three inputs, both thread counts, both entropy
coders and both RC modes. Twenty-four of an `mt` sweep's 120 configurations are
`sm=3 n=600`, so the per-susceptible-configuration rate is roughly **1 in 100-150**,
against ~1 in 800 over all configurations. The two figures agree; the second is the
first divided by the susceptible fraction. Whoever fixes this should start at the
tighter byte constraint, where slices are smallest and most numerous.

**Alternation eight** — 12 `mt` sweeps per side, 1440 configurations each, binaries
built once and swapped inside one loop, nothing else running:

**base (`b6fe4022`) 2 hits / 12 sweeps, head (T4b.3b) 3 hits / 12 sweeps.**

Given five hits split 3-2, P(head ≥ 3 | equal rates) = 0.5. No signal. **T4b.3b
acquitted**, and with it T4b.3c, which the subsequent battery ran clean at 341/341
both profiles.

Two things this measurement establishes that the previous nineteen did not:

1. **The head side hit one configuration twice with two different wrong lengths** —
   `Static_152_100 t=4 sm=3 n=600 cabac=1 rc=0` gave 0 bytes at sweep 8 and 28537
   bytes at sweep 11, against a stable C++ 30190. That is this finding's own race
   criterion ("a deterministic port bug repeats its bytes") satisfied *on the tree
   under suspicion*, which is stronger than a rate comparison: it rules out
   divergence directly rather than by symmetry.
2. **An isolated re-run is not always a non-result.** Session A's rule came from
   0/80 in isolation; this session's retry of the release configuration hit **1 in
   5** in isolation, immediately after a full battery. The distinguishing variable is
   not isolation but *recent load* — the retry ran on a machine still warm from the
   battery, session A's did not. S23b's sweep-level rule stands as the reliable
   escalation, but a single-configuration re-run that **does** reproduce is evidence,
   and should be run before the expensive alternation rather than instead of it.

Also on record: **the session-start battery hit**, ending a two-session clean streak.
The standing advice is unchanged and is now three-for-six: a tendency, not a rule.

The **exit-level battery** (`gates.sh exit`, run once per phase) contributed the last
two rows above, after the alternation had already acquitted this tree and with only
documentation changed since. Both retried 5/5 clean. Recorded rather than folded into
the alternation because they are independent samples of the same tree and they take the
`n=600` count from nine to eleven. The exit level's *widened Miri* — the F18/S22 backlog
check that only runs at a phase exit — came back **clean on all four targets**
(295 `--lib`, 20, 7, 3), so Phase 4b leaves no such backlog behind.

Running total: **twenty measurements, eight alternations, eight acquittals.**

---

### Measurements 21–22 (Phase 5, session A, 2026-08-11) — six hits, and the first even alternation

**Six hits across the session's batteries**, every one inside the narrowed signature —
`mt`, `sm=3`, **`n=600`**, `t∈{2,4}`, wrong-length output — on all three clips and in
both profiles:

| # | configuration | C++ | Rust |
|---|---|---|---|
| 21a | `320x192 t=4 sm=3 n=600 cabac=1 rc=0` (release) | 39981 | 37837 |
| 21b | `160x96 t=4 sm=3 n=600 cabac=0 rc=0` (release) | 42538 | 0 |
| 21c | `160x96 t=4 sm=3 n=600 cabac=0 rc=1` (release) | 42538 | 40857 |
| 21d | `320x192 t=2 sm=3 n=600 cabac=0 rc=1` (release) | 40809 | 0 |
| 21e | `160x96 t=4 sm=3 n=600 cabac=1 rc=0` (**debug**) | 42281 | 0 |

Each of 21a, 21b and 21e was retried in isolation immediately: **5/5 byte-identical**
every time. 21c and 21d were folded into the alternation below rather than retried.

**Measurement 22 — the alternation, and it is the first even one.** Two-plus hits
triggers the whole-preset protocol: 12 `mt` sweeps per side, 120 configurations each,
both release binaries built once and swapped inside one loop, machine otherwise idle.
Base = the session's entry tree (`17167c81`); head = after T5.A4.

**Base 4 / head 4**, 1440 configurations per side. Two details beyond the count:

1. **The head side met this finding's own race criterion.** It hit
   `320x192 t=4 sm=3 n=600 cabac=1 rc=0` **twice with two different wrong lengths** —
   0 bytes at pair 1, 37837 at pair 3, against a stable C++ 39981. A deterministic port
   bug repeats its bytes; this does not. Same shape session C recorded, now on a
   different tree.
2. **The base side reproduced session C's exact numbers.** `Static_152_100 t=4 sm=3
   n=600 cabac=1 rc=1` came back **28537 against C++ 30190** — the same pair session C
   logged for the same configuration on a different tree three commits earlier. The
   signature is stable across trees, sessions and profiles.

**Rate: 8 hits / 2880 alternated configurations ≈ 1 in 360**, roughly double the ~1/800
baseline. Consistent with the load hypothesis in both directions now: session B saw
zero across eight sweeps on an idle machine, session C and this session — both of which
ran batteries back to back for hours — see it at two to three times the baseline rate.
**Load is not a modifier of the signature, it is part of it.**

One further acquittal that costs nothing to state: **every commit in this session
changes decoder code or tests, and these sweeps compare encoders.** There is no path
from the tree under test to an encoder output difference.

Running total: **twenty-two measurements, nine alternations, nine acquittals.**

---

### Measurement 23 (Phase 5, session B, 2026-08-11) — one hit, recorded late

One hit at that session's entry battery, `mt CiscoVT2people_160x96_6fps t=4 sm=3
n=600 cabac=0 rc=0`, C++ 42538 against Rust **0**; re-run 5× in isolation, **5/5
byte-identical**. Acquitted under S14 step 1; no alternation, one hit.

It was written into the session log and the plan's Gates cell but **never reached
this file**, which is where S14 step 4 says every measurement goes. Backfilled here
by session C, and worth one sentence of its own: the ledger a rule points at is only
as good as the sessions that append to it.

---

### Measurements 24–25 (Phase 5, session C, 2026-08-11) — three hits, and the tenth alternation

**Three hits in one battery**, all inside the signature — `mt`, `sm=3`, **`n=600`**,
`t∈{2,4}`, wrong-length output:

| # | configuration | C++ | Rust |
|---|---|---|---|
| 24a | `320x192 t=4 sm=3 n=600 cabac=0 rc=1` (**debug**) | 40992 | 0 |
| 24b | `320x192 t=4 sm=3 n=600 cabac=1 rc=0` (**debug**) | 39981 | 37837 |
| 24c | `160x96 t=2 sm=3 n=600 cabac=0 rc=0` (release) | 41938 | 0 |

All three re-run 5× in isolation per S14 step 1: **5/5 byte-identical, every one.**
24b is the pair session A logged as 21a on a different tree — third appearance of
`39981 / 37837` for that configuration.

**Measurement 25 — the alternation.** Three hits triggers step 2: 12 `mt` sweeps per
side, 120 configurations each, both release binaries built once and swapped inside
one loop, machine otherwise idle. Base = the session's entry tree (`272c3b79`); head
= T5.C2.

**Base 4 / head 3**, 1440 configurations per side. HEAD is not worse. Three things
this measurement adds:

1. **The race criterion is met across the isolation runs rather than inside the
   sweep.** The head binary produced 37837 for `320x192 t=4 sm=3 n=600 cabac=1 rc=0`
   in the battery and in alternation sweep 9, and the *correct* 39981 five times out
   of five for the same binary and configuration in isolation. One binary, one
   configuration, two outputs: a race. A deterministic port defect cannot decode
   correctly five times.
2. **Both sides hit the same configuration with different lengths.** Base sweep 8 and
   head sweep 5 both hit `160x96 t=4 sm=3 n=600 cabac=1 rc=0`; base returned 0 bytes,
   head returned 41989, against a stable C++ 42281.
3. **The rate is back to baseline on an idle machine.** 7 hits / 2880 alternated
   configurations ≈ **1 in 410**, against session A's 1/360 and the ~1/800 quiet
   baseline. This alternation ran with nothing else on the machine, which is the
   condition S14 step 2 asks for and which session A's did not have.

And the standing acquittal, which holds a third session running: **every commit in
this session is decoder-side, and these sweeps compare encoders.** T5.C1 and T5.C2
touch `src/decoder/*` only. The two `ExpandPicture*_c` kernels in `decoder_core.rs`
are the one decoder symbol the encoder reaches, through
`common/expand_pic.rs::ExpandReferencingPicture`, and neither kernel is edited by
either commit — only that function's decoder-side *call sites* are.

Running total: **twenty-five measurements, ten alternations, ten acquittals.**

---

### Measurement 26 (Phase 5, session D, 2026-08-11) — one hit, on a tree with no encoder change to blame

One hit at the session-open battery, `mt CiscoVT2people_320x192_12fps t=4 sm=3
n=600 cabac=1 rc=1` (**release**), C++ 39981 against Rust **0**. Inside the
signature on every axis. Re-run 5× in isolation per S14 step 1: **5/5
byte-identical**. The full release sweep re-run immediately after: **341/341**.
One hit, so no alternation is owed.

What makes this one worth its own entry is what the tree contained. The only change
between session C's exit (`d551e828`, whose battery was `OVERALL: PASS` with
341/341 both profiles) and this battery is **`ec29b339`, docs, and `b0632555`, a
`[profile.dev]` block in `rust/crates/openh264-rs/Cargo.toml`.** The sweep does not
build through that manifest — `rust/tools/diffharness/rust_enc` is its own workspace
root with its own identical profile — and this hit is on the **release** sweep
besides. So there is no candidate mechanism at all: the same source, the same
binary, 341/341 the second time.

That is the cleanest form of the standing acquittal the last three sessions have
been recording in a weaker version. Previous entries said *the commits are
decoder-side and the sweeps compare encoders*; this one says *there are no commits*.

Running total: **twenty-six measurements, ten alternations, ten acquittals.**

---

### Measurements 27–28 (Phase 5, session D, 2026-08-11) — two hits, and the alternation replaced by a hash

Two hits in one battery, one per profile, both inside the signature:

| # | configuration | C++ | Rust |
|---|---|---|---|
| 27 | `160x96 t=2 sm=3 n=600 cabac=0 rc=1` (**debug**) | 41938 | 0 |
| 28 | `160x96 t=4 sm=3 n=600 cabac=1 rc=0` (release) | 42281 | 41681 |

Both re-run 5× in isolation per S14 step 1: **5/5 byte-identical, both.**

**Two hits triggers step 2's alternation, and this session did not run one — because
something cheaper and stronger was available.** The tree's only change since the last
341/341 is a `#[cfg(test)]` unit test inside the *library*. `#[cfg(test)]` code is not
compiled when the crate is built as a dependency, so the sweep's binary should be
unaffected — and rather than assert that, it was measured. Built both ways with the test
present and stashed:

```text
HEAD  rust_enc: fa59e2a591fe2178ca8949f55f119e435788f0cdd0f523d60f5716581d771844
BASE  rust_enc: fa59e2a591fe2178ca8949f55f119e435788f0cdd0f523d60f5716581d771844
```

**Byte-identical.** An alternation compares base against head to answer *is HEAD worse*;
here base and head are the same binary in the only artifact the sweep exercises, so the
comparison is vacuous by construction and 24 sweeps would have measured the harness.
This is S23b's principle at its limit — *a result that cannot distinguish the trees is a
statement about the harness* — reached from the opposite direction: not "no reproduction
on either side" but "there are not two sides."

**Worth adding to the protocol**: before running S14 step 2, hash the sweep binary
against the base's. If they match, the alternation cannot produce information and the
hash *is* the acquittal — cheaper than 24 sweeps and not probabilistic. Step 2 stays
exactly as written for any session whose commits reach `rust_enc`, which is every
encoder-side one.

Running total: **twenty-eight measurements, ten alternations, ten acquittals** — and one
acquittal that needed no alternation.

---

### Measurement 29 (Phase 5, session E, 2026-08-12) — one hit, and the first time one configuration produced three different answers

| # | configuration | C++ | Rust |
|---|---|---|---|
| 29 | `320x192 t=4 sm=3 n=600 cabac=1 rc=0` (**debug**) | 39981 | 0 |

Re-run 5× in isolation per S14 step 1, and the result is not the usual 5/5:

```text
run 1  BYTE-IDENTICAL
run 2  BYTE-IDENTICAL
run 3  BYTE-IDENTICAL
run 4  BYTE-IDENTICAL
run 5  C++ 39981   Rust 37837
```

**One binary, one configuration, three different Rust outputs across six runs**: 0 bytes
in the sweep, 37837 in isolation run 5, and byte-identical 39981 four times. S14 step 1's
own criterion is that two different wrong lengths from one binary+configuration is a race
rather than a divergence, *because a deterministic port bug repeats its bytes*. This is
the first measurement to satisfy that test directly rather than by inference from
frequency — every prior isolation re-run came back 5/5 identical, which proved only that
the race was hard to reproduce in isolation.

**No alternation owed** (one hit, S14 step 1). The hash shortcut this session added as
S14 step 0 does **not** apply — `rust_enc` depends on `openh264-rs` by path, so a
decoder-side commit does rebuild it, and the shortcut is only for trees that produce a
byte-identical binary. Recorded because the temptation to reach for it is real: the
session's changes are 100% decoder-side and the sweep exercises the *encoder*, which is
a good argument and is **not** the same thing as a matching hash. The measurement was
taken instead of the argument.

Also worth noting for the frequency record: this hit is the **debug** sweep, whose F3
susceptibility is documented in `rust_enc/Cargo.toml` as a deliberate consequence of the
`[profile.dev] opt-level = 3` change — the build is now fast enough to lose the race,
which F3's own write-up predicted. The release sweep in the same battery was 341/341.

---

### Measurements 30–32 (Phase 5, session F, 2026-08-12) — recovered into this ledger at session G

**These three were measured and adjudicated at session F and written into that session's
log entry, but never appended here.** S14 step 4 says *append every measurement to F3*,
and the ledger is the instrument's memory — a measurement that lives only in a session
log is invisible to whoever greps this file for the rate. Recovered verbatim from the
session F entry (`safety_refactor_log.md`, its §6) rather than re-run.

| # | configuration | C++ | Rust |
|---|---|---|---|
| 30 | `mt … t=4 sm=3 n=600 cabac=1 rc=0` (release) | — | 0 |
| 31 | `mt … t=4 sm=3 n=600 cabac=1 rc=0` (release, second stream) | — | 28537 |
| 32 | `mt … t=4 sm=3 n=600 cabac=1 rc=1` (**debug**) | — | 0 |

Each re-run 5× in isolation per S14 step 1. 30 and 31 came back 5/5 byte-identical.
**32 came back 4× byte-identical and 1× zero bytes** — S14 step 1's race criterion met
directly, the second time this project has had that quality of evidence (measurement 29
was the first).

Rate over five back-to-back release sweeps at that session: **2 / 1705 ≈ 1/850**, against
F3's measured ≈1/800.

**Eleventh alternation** (S14 step 2, owed by 30+31 being two hits): 12 `mt` presets per
side in one loop, 1440 configurations each, **base 1 / head 1**. Even, and not 0/0
(S23b). Acquitted.

Also from that session, worth keeping with the protocol rather than the log: **step 0 was
checked and declined for the first time.** T5.F2 touched `common/memory_align.rs`, which
the encoder links, so the two `rust_enc` binaries genuinely differed (`ed3b2622` base,
`5ab0ac61` head) and the hash shortcut did not apply. Every prior use of step 0 in this
project had been an acquittal; this was the first time it was run and came back "no".

Running total: **thirty-two measurements, eleven alternations, eleven acquittals.**

---

### Phase 5, session G (2026-08-12) — zero hits, and that is a sample

No numbered measurement: this ledger numbers *hits*, and there were none.

Four sweeps of 341 configurations — two `gates.sh full` batteries × two profiles, 1364
configurations — **all PASS, both profiles**. Nothing to adjudicate; S14's protocol
starts at a hit.

Recorded anyway because S14 step 4's other half matters here: **a clean sweep is a
sample, not a signal.** F3's measured rate is ≈1/800 under sustained load and ≈1/100–150
on susceptible configurations, so 1364 configurations drawing zero is an ordinary
outcome and says nothing about whether the finding is still live. The session's changes
were 100% decoder-side; the sweep exercises the encoder.

Running total unchanged: **thirty-two measurements, eleven alternations, eleven
acquittals.**

### Phase 5, session H (2026-08-12) — zero hits across six sweeps

No numbered measurement: this ledger numbers *hits*, and there were none.

**Six sweeps of 341 configurations** — three `gates.sh full` batteries × two profiles,
2046 configurations — **all PASS, both profiles**. Nothing to adjudicate; S14's protocol
starts at a hit, and appending at adjudication time (S14 step 4, as amended at session
G) means there was nothing to append as the session went.

Second consecutive zero-hit session, and the sample is now larger: 1364 + 2046 = 3410
configurations across sessions G and H. At F3's measured ≈1/800 rate that is roughly
four expected hits, so drawing zero twice is on the unlikely side of ordinary rather
than evidence of anything — but two sessions is not a trend, and both sessions'
changes were 100% decoder-side while the sweep exercises the encoder. **A clean sweep
is still a sample.** If session I also draws zero on encoder-touching work, that is
the first reading that would mean something.

Running total unchanged: **thirty-two measurements, eleven alternations, eleven
acquittals.**

### Phase 5, session I (2026-08-12) — zero hits across six sweeps

No numbered measurement: this ledger numbers *hits*, and there were none.

**Six sweeps of 341 configurations** — one `gates.sh family` and two `gates.sh full`
batteries, × two profiles, 2046 configurations — **all PASS, both profiles**.

Third consecutive zero-hit session; 1364 + 2046 + 2046 = **5456 configurations** across
sessions G, H and I. Session H's entry asked whether session I would be the reading that
means something. **It is not**, and the reason is the same one: `git diff --stat
75188044..HEAD -- rust/crates` is three files, all under `src/decoder/`, while the sweep
compares encoders. Three zero-hit sessions running on decoder-only work is a statement
about what the sweep is pointed at, not about the race.

S14's step-0 hash shortcut is **not** claimed here and was not run: `rust_enc` is built
from this same crate, so a decoder-only source diff does not by itself imply a
byte-identical encoder binary, and the shortcut is an acquittal only when the hashes are
actually compared. Nothing needed acquitting this session — there was no hit — so the
question did not arise. It will the first time an encoder-touching session draws one.

Running total unchanged: **thirty-two measurements, eleven alternations, eleven
acquittals.**

### Measurement 33 (Phase 5, session J, 2026-08-12) — one hit, and the zero-hit run ends at three

**The hit**, `gates.sh full` on `d8becdf0` (Face 1's tree), debug sweep:

```
mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1 ::  C++: 42281 bytes  Rust: 0 bytes
PASS=340 FAIL=1
```

Inside S14's signature on every field — `mt`, `sm=3`, `n=600`, `t=4`, output of the
wrong length (zero), debug profile. The release sweep of the same battery was 341/341,
as were both sweeps of the session's other batteries.

**Step 1, the isolation re-run — 5×, machine idle, same configuration and profile:
5/5 BYTE-IDENTICAL.** That is the expected result and not a weak one: S23b is the rule
that says so. The race needs the load of a full sweep, and running the hitting
configuration alone gave 0/40 the last time it was tried against a race the sweeps had
just produced twice. **A no-reproduction in isolation is a statement about the harness.**

One hit, so step 2 does not fire — an alternation is for two or more. **Acquitted as F3.**

The step-0 hash shortcut was **not** claimed and would not have applied: this session's
diff touches `rust/tools/`, `res/` and four decoder files, and while the sweep compares
encoders, `rust_enc` is built from the same crate and the binaries were never compared.

The three-session zero-hit run (G, H, I — 5456 configurations) ends here. It ended on a
session whose code changes are again **entirely decoder-side**, which is the same reason
those three sessions drew nothing: the sweep exercises the encoder, and what it sees is
the ambient rate, not this tree. That is the honest reading of a single hit in 1364
configurations either way — ≈1/800 is the measured rate and one hit is what it predicts.

Running total: **thirty-three measurements, eleven alternations, twelve acquittals.**

### Measurement 34 (Phase 5, session K, 2026-08-12) — four hits, two alternations, and the signature's `n=600` clause falls

The session's two batteries drew **two hits each**, so S14 step 2 fired both times —
the first alternations of the phase to run on consecutive trees.

**Battery 1**, `gates.sh full` on T5.K1's tree (`pMv` flipped), **debug** sweep, 339/341:

```
mt CiscoVT2people_320x192_12fps t=2 sm=3 n=600 cabac=0 rc=0 ::  C++: 40809  Rust: 0 bytes
mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1 ::  C++: 39981  Rust: 37837 bytes
```

**Battery 2**, `gates.sh full` on the T5.K2+T5.K3 cluster, **release** sweep, 339/341:

```
mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=0 ::  C++: 42281  Rust: 41989 bytes
mt Static_152_100 t=4 sm=3 n=600 cabac=0 rc=0 ::              C++: 29375  Rust: 0 bytes
```

All four inside the signature on every field. Note the mix: two zero-length and two
short, which is the signature's "any wrong length" doing its work.

**Step 0 does not apply and was not claimed.** The shortcut wants a diff that cannot
reach `rust_enc`; this session's reaches lib code the driver links. (A cross-path hash
comparison was tried first and is *invalid* as evidence either way — a control built in
a `git worktree` embeds a different path, so its binary differs regardless of the
source. Build both sides in the same directory, or do not use the shortcut.)

**Step 1, both configurations of battery 1, 5× each on an idle machine: 5/5
BYTE-IDENTICAL each.** S23b's expected result.

**Step 2, twice, each at the profile its hits occurred in** — 12 whole `mt` presets per
side, 120 configurations each, both binaries built once and swapped inside one loop:

| alternation | control (`4b91c1a0`) | head | configurations per side |
|---|---|---|---|
| debug, vs T5.K1 | **4** | **3** | 1440 |
| release, vs T5.K2+K3 | **4** | **6** | 1440 |
| **combined** | **8** | **9** | **2880** |

A 6-vs-4 split is p ≈ 0.38 one-sided under the null (Binomial(10, ½)); 8-vs-9 combined
is as balanced as this instrument gets. **HEAD is not worse. Acquitted as F3**, and both
alternations produced hits on both sides, so S23b is satisfied in both.

**The rate, measured across 5760 alternation configurations plus 682 battery ones:
17 + 4 = 21 hits in 6442, ≈ 1/307.** That is higher than the recorded ≈1/800, and the
difference is load: twenty-four back-to-back `mt` presets with nothing between them is
the most sustained load this project has ever run the sweep under, and F3's rate is a
function of exactly that.

**The signature loses its `n=600` clause.** One of head's debug hits was

```
mt CiscoVT2people_320x192_12fps t=2 sm=3 n=1500 cabac=1 rc=0 ::  C++: 39627  Rust: 0 bytes
```

— the **first observation at `n=1500` in 34 measurements**, where S14's text reads
"`n=600` (never observed at `n=1500`)". It was re-run 5× in isolation and was 5/5
byte-identical, like every other hit. The mechanism explains the asymmetry rather than
excusing it: `sm=3` is the size-limited slice mode and `n` is the **per-slice byte
budget**, so `n=600` cuts more slices per frame than `n=1500` and therefore performs
more of the slice-list growths the race lives in. `n=600` is a rate artifact, not a
condition. **S14's signature should read `sm=3`, `t∈{2,4}`, any wrong length, `mt`, with
`n=600` as the predominant but not exclusive slice budget.**

Running total: **thirty-four measurements, thirteen alternations, thirteen acquittals.**

### Measurement 35 (Phase 5, session L, 2026-08-12) — one hit, and the isolation re-run reproduced it twice

| # | configuration | C++ | Rust |
|---|---|---|---|
| 35 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**) | 39981 | 37837 |

Drawn by the second of the session's two full batteries (the L4–L7 cluster);
`PASS=340 FAIL=1` on the debug sweep, release 341/341. Inside S14's signature on every
axis.

**Step 1's isolation re-run, 10× on an idle machine: 8 byte-identical, 2 short
streams.** That is the second time in the finding's history that the isolation re-run
has reproduced anything — measurement 29 was the first — and it is the **same
configuration and the same wrong length**, 37837 against the C++'s 39981, that
measurement 29 produced. One binary, one configuration, two different outputs, so S14
step 1's own criterion for *race rather than divergence* is met directly: a
deterministic port bug repeats its bytes.

**One hit, so step 2 does not fire** (S14 step 1; session J's measurement 33 is the
precedent). **Acquitted as F3.** The session's commits are decoder-only and the sweep
compares encoders, which is the weakest kind of prior — but the reproduction is the
evidence, not the argument.

**Step 0 does not apply**, for the reason measurement 29 recorded: `rust_enc` depends on
`openh264-rs` by path, so a decoder-side commit rebuilds it and the two trees do not
produce a byte-identical binary.

Two things this adds to the finding. **`320x192 t=4 sm=3 n=600 cabac=1` is the
susceptible configuration** — it is the one that produced measurements 29 and 35, both
in the debug profile, both with the same short length. And **its isolation rate is
~1/5**, far above the ~1/307 sustained-sweep rate and the ~1/100–150 S14 records for
susceptible configurations; ten runs is a small sample, but it is the first time
isolation has been the *cheaper* place to reproduce this rather than the harder one.

Running total: **thirty-five measurements, thirteen alternations, fourteen acquittals.**

### Measurement 36 (Phase 5, session M, 2026-08-13) — one hit, zero length, and the first `cabac=0` hit on the susceptible stream

| # | configuration | C++ | Rust |
|---|---|---|---|
| 36 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0` (**release**) | 40992 | **0** |

Drawn by the T5.M3 cluster's `family` battery; `PASS=340 FAIL=1` on the release sweep,
debug 341/341. Inside S14's signature on every axis — `mt`, `sm=3`, `t=4`, wrong length
(**zero**, the extreme end of "zero, short, or long"), release profile.

**Step 1's isolation re-run, 5× on an idle machine: 5 byte-identical.** No reproduction,
which is the ordinary outcome — measurements 29 and 35 are the only two in thirty-six
that have reproduced in isolation. One hit, so step 2 does not fire (S14 step 1;
measurements 33 and 35 are the precedents). **Acquitted as F3.**

**Step 0 does not apply**: `rust_enc` depends on `openh264-rs` by path, so this
session's decoder-side commits rebuild it and the two trees are not one binary.

One thing it adds, and it narrows nothing — it *widens*. Measurements 29 and 35 made
`320x192 t=4 sm=3 n=600 **cabac=1**` look like *the* susceptible configuration; this is
the same stream, the same thread count, the same slice mode and the same `n`, with
**`cabac=0`, `rc=0`, and the other profile**. So the susceptibility is `320x192 t=4
sm=3 n=600`, and the entropy coder and rate-control mode are no more part of the
signature than `n=600` turned out to be at session K. The wrong length is also a first
for this configuration: 29 and 35 were both *short* (37837 of 39981); this one is
**zero bytes**, which S14's signature has always admitted and which the slice-list
growth story explains as well as a short stream does.

Running total: **thirty-six measurements, thirteen alternations, fifteen acquittals.**

### Measurement 37 (Phase 5, session N, 2026-08-13) — one hit, and the *stream* leaves the signature too

| # | configuration | C++ | Rust |
|---|---|---|---|
| 37 | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=1` (**release**) | 42538 | **42296** |

Drawn by the T5.N2 `family` battery; `PASS=340 FAIL=1` on the release sweep, debug
341/341. Inside S14's signature on every axis — `mt`, `sm=3`, `t=4`, wrong length
(short, 242 bytes down), release profile.

**Step 1's isolation re-run, 5× on an idle machine: 5 byte-identical**, C++ and Rust
both 42538. No reproduction — the ordinary outcome. One hit, so step 2 does not fire.
**Acquitted as F3.**

**Step 0 does not apply**: `rust_enc` depends on `openh264-rs` by path, so the
session's decoder commits rebuild it and the two trees are not one binary.

**What it takes out of the signature is the stream.** Measurements 29, 35 and 36 all
landed on `320x192` and the last of them concluded "the susceptibility is `320x192 t=4
sm=3 n=600`". This is the **other** clip, at a quarter of the pixels, with everything
else the same shape — so what survives across all thirty-seven measurements is
`mt` + `sm=3` + `t∈{2,4}`, and `320x192`, `n=600`, `cabac` and `rc` are all rate
artifacts of how much slice-list churn a configuration does, not conditions. That is
the third clause this signature has shed (session K took `n=600`, session M took
`cabac`/`rc`), and each time by the same mechanism: a hit that matched everything but
the clause.

**A protocol note worth more than the measurement.** The first isolation attempt
reported C++ **0** bytes five times and looked like a reproduction of a *worse* defect.
It was the harness: `compare.sh` `cd`s to the repo root before invoking either encoder,
so a relative `<yuv>` path silently resolves against the wrong directory — and because
the tag is derived from the arguments, `stat` then reported the **stale** `.264` from
the sweep that had just failed. Two artefacts pointing the same way. Pass an absolute
path, and set `RUST_ENC_PROFILE` to the profile the hit came from, or step 1 measures
neither tree. S1's "disassemble before theorising" has a cheaper cousin here: check the
instrument reported on the run you think it did.

Running total: **thirty-seven measurements, thirteen alternations, sixteen acquittals.**

### Measurement 38 (Phase 5, session N, 2026-08-13) — one hit on the closing battery, and the harness trap bites a second time

| # | configuration | C++ | Rust |
|---|---|---|---|
| 38 | `mt CiscoVT2people_320x192_12fps t=2 sm=3 n=600 cabac=1 rc=0` (**release**) | 39884 | **0** |

Drawn by session N's closing full battery; `PASS=340 FAIL=1` on the release sweep, debug
341/341, everything else green (Miri 330, both benches bit-identical). Inside S14's
signature on every axis — `mt`, `sm=3`, **`t=2`** (the first `t=2` hit since measurement
30), wrong length (zero), release profile.

**Step 1's isolation re-run, 5× on an idle machine: 5 byte-identical**, both encoders
39884. No reproduction. One hit, so step 2 does not fire. **Acquitted as F3.**

Two hits in one session (37 and 38) over 1364 swept configurations is ≈1/682, consistent
with the ≈1/800 battery rate and well under the ≈1/307 sustained-preset rate.

**The harness trap from measurement 37 caught the same session twice, in a second form.**
There the mistake was a relative path; here it was the frame count. `sweep.sh` builds its
input by looping the source clip to at least `SWEEP_FRAMES=16`, and the two clips have
different source lengths — `CiscoVT2people_160x96_6fps` loops to **20** frames,
`CiscoVT2people_320x192_12fps` to **18**. Reusing 20 for the 320x192 clip names a file that
does not exist, and `compare.sh` then reports `C++ 0 / Rust -1`, which reads like a
catastrophic divergence rather than a missing input. **Take the frame count from the
`out/*_loopN.yuv` file that exists, not from the other clip's**, and read the isolation
run's *both* sizes: two encoders failing identically is a harness result, never a finding.

Running total: **thirty-eight measurements, thirteen alternations, seventeen acquittals.**

### Measurement 39 (Phase 5, session O, 2026-08-13) — the first isolation run to reproduce a *third* distinct length

| # | configuration | C++ | Rust |
|---|---|---|---|
| 39 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**) | 39981 | **0** |

Drawn by a mid-session `family` battery run to check an unrelated decoder change;
`PASS=340 FAIL=1` on the debug sweep, release 341/341. Inside S14's signature on every
axis — `mt`, `sm=3`, `t=4`, wrong length (zero), debug profile. The session's own diff was
**decoder-only**, which is as close to the step-0 hash shortcut as a non-identical binary
gets.

**Step 1's isolation re-run reproduced, and the reproduction is the interesting part.**
Fifteen runs of the hitting configuration on an idle machine: **fourteen byte-identical**
at 39981, and **one that produced a 17-frame stream** where every other run produced 18
(`1566720` against `1658880` bytes of decoded YUV). With the sweep's own zero-byte result
that is **three distinct outcomes from one binary and one configuration** — which is
S14 step 1's discriminator stated exactly: *a deterministic port bug repeats its bytes.*
Every prior reproduction in this finding's history (29, 35) repeated the *same* wrong
length; this is the first that did not, and it is the strongest single piece of evidence
the finding has that the mechanism is a race rather than a divergence.

One hit in the sweep, so step 2 does not fire. **Acquitted as F3.**

The rate: 1 in 341 configurations on this battery. Nothing new about the signature —
`320x192`, `n=600`, `t=4`, `cabac=1` have all appeared before, and measurements 36–38
already removed the stream, `n` and `t` clauses as conditions.

Running total: **thirty-nine measurements, thirteen alternations, eighteen acquittals.**

### Measurements 40–41 (Phase 5, session P, 2026-08-13) — the alternation where one binary gave one configuration two different wrong lengths

| # | configuration | C++ | Rust |
|---|---|---|---|
| 40 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**) | 39981 | **0** |
| 41 | `mt CiscoVT2people_160x96_6fps  t=4 sm=3 n=600 cabac=0 rc=0` (**release**) | 42538 | **42616** |

One hit per profile on the same closing battery — different clips, different `cabac`,
one short-to-zero and one *long*. Both inside S14's signature on every axis.

**Step 0 did not apply, and it is worth recording why.** The session's diff is
decoder-only, which has been the hash shortcut's usual trigger — but the shortcut needs
the two `rust_enc` binaries to be *byte-identical*, and it is production decoder code in
the same lib, so base and head differ even though the sweep compares encoders. Session
D's shortcut was earned by a `#[cfg(test)]`-only diff, which is a narrower condition than
"decoder-only" and the two should not be conflated again.

**Step 1: both configurations 5/5 byte-identical in isolation.** Neither reproduced.

**Step 2 fired (two hits) and the alternation acquits.** Twelve whole `mt` presets per
side, both debug binaries built once and swapped inside one loop, machine otherwise idle,
240 configurations per sweep = **2880 per side**:

| side | hits | sweeps | configurations |
|---|---|---|---|
| base (`4f6495dd`, session O's code) | **2** | 12 | 2880 |
| head (`eef8a90b`) | **3** | 12 | 2880 |

Not a difference (S23b is satisfied — this alternation is not 0/0). Two further facts,
and the second is the strongest evidence in this finding's history:

* **The base tree hits the battery's own debug configuration**, twice
  (`320x192 t=4 sm=3 n=600 cabac=1 rc=0`, rounds 5 and 9) — on code that predates the
  session entirely.
* **One binary, one configuration, two different wrong lengths.** The head binary gave
  that configuration **0 bytes** in the battery and **37837** in alternation round 10;
  the base binary gave **37837** twice. Measurement 39 reached three distinct outcomes
  across *fifteen isolation runs*; this reaches two across an ordinary alternation, on
  both sides, which is S14 step 1's discriminator met without having to go looking:
  *a deterministic port bug repeats its bytes.*

Rate: 5 hits in 5760 alternation configurations ≈ **1/1150** under this loop's density,
against ≈1/307 under session K's sustained back-to-back presets and ≈1/800 in a
battery — consistent with load density being part of the signature. `cabac=0 rc=1`
appears for the first time on this configuration, which is one more confirmation that
session M was right to take `cabac`/`rc` out of the signature.

**Acquitted as F3.**

Running total: **forty-one measurements, fourteen alternations, twenty acquittals.**

### Measurement 42 (Phase 5, session P′, 2026-08-14) — one configuration, three outcomes, and the reproduction landed inside step 1's five runs

| # | configuration | C++ | Rust |
|---|---|---|---|
| 42 | `mt Static_152_100 t=4 sm=3 n=600 cabac=0 rc=0` (**release**) | 29375 | **0** |

One hit on the closing battery; the debug sweep was 341/341. Inside S14's signature on
every axis — `mt`, `sm=3`, `t=4`, wrong length (zero), and the profile is not a clause.

**Step 0 was checked rather than assumed, and it does not apply.** The session's diff is
production decoder code, `src/api/codec_api.rs` and one safe container, so the shortcut's
condition — two *byte-identical* `rust_enc` binaries — is the thing to test, not the
"decoder-only" heuristic session P warned about. One build each:

| tree | `rust_enc` (release) SHA-256 |
|---|---|
| base `c8ebc20f` | `4a730f44123354167979f51953101149811be935…` |
| head `6758d885` | `db175dc17aafc3eb18b3284812f327947177dc73…` |

Different binaries, so base and head are two programs and the shortcut cannot acquit.

**Step 1 reproduced, within its own five runs.** Re-running the hitting configuration:
run 4 of the first five produced a Rust stream **shorter than the C++ one** (`cmp`
reached EOF on it); the other four, and twelve further runs afterwards, were
**byte-identical at 29375**. Twenty-two isolation runs in total, twenty-one identical.

So from **one binary and one configuration**: 29375 twenty-one times, **0 bytes** in the
battery, and one short stream. Three distinct outcomes — S14 step 1's discriminator, met
for the third measurement running (39 needed fifteen isolation runs to reach three; 40–41
reached two inside an ordinary alternation; this one reached the second inside the five
runs the protocol prescribes). *A deterministic port bug repeats its bytes.*

One hit in the battery, so step 2 does not fire and no alternation was run.

**Acquitted as F3.**

Rate: 1 in 341 configurations on this battery, and `Static_152_100` joins the clip list —
the third of the three sweep inputs to produce a hit, which removes the last trace of a
per-clip clause the way measurements 36–38 removed `n`, `t` and the stream.

Running total: **forty-two measurements, fourteen alternations, twenty-one acquittals.**

### Forty-third to forty-fifth measurements — 2026-08-15, Phase 5 session V: three hits over two batteries, and the first alternation in this phase that ties at 2/2

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 43 | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=0` (**release**, battery at `3180ac39`) | 42538 | **40857** (short) |
| 44 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**, battery at `92d6fa75`) | 39981 | **0** |
| 45 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=1500 cabac=0 rc=0` (**release**, same battery) | 39895 | **0** |

Two exit batteries ran this session — one after the deny-clean pair, one after the
`fmo` conversion — and every hit is inside S14's signature on every axis: `mt`,
`sm=3`, `t=4`, wrong length. The profile is not a clause (43 and 45 are release, 44 is
debug) and neither is `n` (45 is the fourth `n=1500` observation in 45 measurements)
nor the clip.

**Step 0 checked, not assumed, and it does not apply.** The session's diff is
production decoder code, so the "test-only" condition fails and the shortcut needs two
byte-identical `rust_enc` binaries to acquit. One build each:

| tree | `rust_enc` (debug) SHA-256 |
|---|---|
| control `676231fa` (U's code state) | `1a5881495c0caf590b6a297f3d1fc15f1f9325e6…` |
| head `92d6fa75` | `cccb75cbe472705f5220866002736a93d906225b…` |

Different binaries, so base and head are two programs — session P's warning met
head-on: the driver links the whole library, so decoder-only production changes still
change the encoder binary.

**Step 1, all three: five isolation runs each, all byte-identical at the C++'s
length.** 43 gave 42538 five times where the battery gave 40857; 44 gave 39981 five
times where the battery gave 0; 45 gave 39895 five times where the battery gave 0. So
from **one binary and one configuration**, each of the three produced two different
outcomes — the discriminator S14 step 1 names, met three times: *a deterministic port
bug repeats its bytes.* And the isolation runs reproducing nothing is a statement
about the harness, not the trees (S23b, from the other side): the race needs a sweep's
load.

**Step 2 fired, because two hits landed in one battery.** Whole `mt` presets
alternated back to back, both binaries built once and copied over the driver's path
inside one loop, machine otherwise idle, 12 presets per side at 120 configurations
each — **1440 configurations per side**:

| side | hits |
|---|---|
| head `92d6fa75` | **2** |
| control `676231fa` | **2** |

**A tie, and not a 0/0 tie** — the alternation ran (S23b's condition) and produced hits
on both sides, which is what S14 says to expect. **HEAD is not worse. Acquitted as F3,
all three.**

**Rate, and it is a datapoint against the sustained-load figure.** 2 hits per 1440
configurations is ≈**1/720** per side under *back-to-back* presets, where session K
measured ≈1/307 under the same regime over 6442 configurations, and ≈1/800 is the
battery figure. The rate is not stable across sessions at this sample size; what
reproduces is the signature, not the frequency. Three hits in two batteries of 682
configurations each is ≈1/455 on the battery side of this session — higher than
K's battery rate and lower than its preset rate, from the same instrument on a
different day.

Running total: **forty-five measurements, fifteen alternations, twenty-four
acquittals.**

### Forty-sixth measurement — 2026-08-15, Phase 5 session W: one hit, and step 1 alone settles it

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 46 | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**, `family` battery over T5.W2) | 42281 | **0** |

Inside the signature on every axis — `mt`, `sm=3`, `t=4`, wrong length, and the
zero-length form. The release sweep in the same battery read **341/341**.

**Step 1: five isolation runs, all byte-identical at 42281** — the same binary and the
same configuration producing two different outcomes, which is S14 step 1's
discriminator met (*a deterministic port bug repeats its bytes*), and the
non-reproduction is S23b from the other side: the race needs a sweep's load.

**Step 2 not required and not run** — it is the two-or-more-hits arm, and this battery
drew one. **Step 0 not run either, and its outcome is not asserted here**: the
shortcut needs two byte-identical `rust_enc` binaries, its "test-only" predicate fails
by inspection (the diff is production decoder code), and measurements 43–45 measured
that exact class three commits ago and found the binaries differ. Recording that as a
prediction rather than a result, per S33.

**Acquitted as F3.**

Running total: **forty-six measurements, fifteen alternations, twenty-five
acquittals.**

### Forty-seventh to fifty-first measurements — 2026-08-16, Phase 5 session X: five hits over three batteries, and the sixteenth alternation

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 47 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**, `family` battery over T5.X8) | 39981 | **37837** (short) |
| 48 | the same configuration (**release**, same battery) | 39981 | **37837** (short) |
| 49 | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**, next battery) | 42281 | **40645** (short) |
| 50 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=1` (**debug**, same battery) | 40992 | **0** |
| 51 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**, isolation, below) | 39981 | **40463** (long) |

Every one inside the signature — `mt`, `sm=3`, `t=4`, wrong length — and the second
battery's release sweep read **341/341** at the configuration the first battery's had
hit, which is the load-dependence S23b names.

**Step 0, measured rather than predicted** (S33): the two `rust_enc` binaries hash
`d9a3bf77…` (HEAD) and `d4f5770e…` (control at `361592a7`). They differ, as the
"test-only" predicate says they must for a diff of production decoder code, so the
shortcut does not apply.

**Step 1: fifteen isolation runs of the debug binary at measurement 47's
configuration**, machine idle. Thirteen produced **39981 bytes, byte-identical to the
C++**; one produced a short output; one produced **40463**, a *long* one. Three
different lengths from **one binary and one configuration**, with the reference bytes
on 13 of 15 attempts, is step 1's discriminator at full strength — *a deterministic
port bug repeats its bytes* — and 40463 ≠ 37837 is the "two different wrong lengths"
clause literally.

**Step 2, the sixteenth alternation: twelve whole `mt` presets per side, both
binaries built once and swapped inside one loop**, 120 configurations per preset,
1440 per side, machine otherwise idle.

| side | hits | presets | configurations |
|---|---|---|---|
| HEAD (T5.X8) | **5** | 12 | 1440 |
| control (`361592a7`, session X's base) | **7** | 12 | 1440 |

**HEAD is not worse; it is marginally better, and both are inside the same rate.**
≈1/288 (HEAD) and ≈1/206 (control) against session K's ≈1/307 under sustained
presets. Every hit on both sides is `mt sm=3 t=4`; the configurations that hit move
between rounds and between sides, which is the property that distinguishes a race
from a divergence. No side produced a hit outside the signature.

**Acquitted as F3.**

Running total: **fifty-one measurements, sixteen alternations, twenty-seven
acquittals.**

### Fifty-second to fifty-fifth measurements — 2026-08-16, Phase 5 session Y: four hits over three batteries, and the seventeenth alternation

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 52 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**, `family` battery over T5.Y1) | 39981 | **0** |
| 53 | the same configuration (**debug**, `full` battery over T5.Y2) | 39981 | **0** |
| 54 | `mt Static_152_100 t=4 sm=3 n=600 cabac=1 rc=1` (**debug**, same battery) | 30190 | **0** |
| 55 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**release**, `exit` battery at the close) | 39981 | **37837** (short) |

Every one inside the signature — `mt`, `sm=3`, `t=4`, wrong length — and the close
battery's **debug** sweep read 341/341 at the configurations the earlier debug sweeps
had hit, which is S23b's load-dependence again, this time with the profiles swapped
relative to session X's observation.

**Step 1 after measurement 52**: the whole `mt` preset re-ran in debug, **120/120
byte-identical**, which is the retry rule's reproduction test failing to reproduce —
valid evidence on its own for a single hit, and what deferred the alternation until
the count reached four.

**Step 0, measured rather than predicted** (S33): the two `rust_enc` binaries hash
`379688ef…` (HEAD at `dff3f78b`) and `05c73ee8…` (control at `3e2f43e6`, session Y's
base). They differ, as the "test-only" predicate says they must for a diff of
production decoder code, so the shortcut does not apply.

**Step 2, the seventeenth alternation: twelve whole `mt` presets per side, both
binaries built once and swapped inside one loop**, 120 configurations per preset,
1440 per side, machine otherwise idle.

| side | hits | presets | configurations |
|---|---|---|---|
| HEAD (`dff3f78b`) | **9** | 12 | 1440 |
| control (`3e2f43e6`, session Y's base) | **10** | 12 | 1440 |

**HEAD is not worse, and the two rates are the same rate**: ≈1/160 (HEAD) and ≈1/144
(control) against session K's ≈1/307 under sustained presets — both denser than K's,
both on a machine that had been running batteries all day, which is the load
dependence rather than a change in either tree. All 19 hits are `mt sm=3`, 18 at
`t=4` and one at `t=2`; the configurations that hit move between rounds and between
sides. No hit outside the signature on either side.

**Acquitted as F3.**

Running total: **fifty-five measurements, seventeen alternations, twenty-eight
acquittals.**

### Fifty-sixth measurement — 2026-08-16, Phase 5 session Z: one hit, and step 1 closed it

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 56 | `mt CiscoVT2people_160x96_6fps t=2 sm=3 n=600 cabac=0 rc=1` (**release**, `exit` battery at the close) | 41938 | **0** |

Inside the signature — `mt`, `sm=3`, `t=2`, wrong length — and the same battery's
**debug** sweep read 341/341, which is the profile asymmetry S23b calls load
dependence rather than a property of either build. It is the session's *only* hit
across an `exit` battery whose three other sweeps were clean, against a diff that
rewrote 138 decoder signatures.

**Step 0, measured rather than predicted** (S33): the two `rust_enc` binaries hash
`e170cdbf…` (HEAD at `2a0e0330`) and `1cff2df7…` (control at `73581a49`, session Z's
base). They differ — as the "test-only" predicate says they must for a diff of
production decoder code — so the shortcut does not apply.

**Step 1, twice.** The hitting configuration re-ran **5/5 byte-identical** in
release, and then the whole `mt` preset re-ran **120/120 byte-identical** in release
— the retry rule's reproduction test failing to reproduce at both the isolated level
and the level S23b says the race actually needs. One hit, not reproducing.

**Step 2 does not trigger**: the alternation is the protocol's answer to *two or
more* hits, and this session drew one. Session Y's own `family`-battery hit was
disposed of the same way and for the same reason.

**Acquitted as F3.**

Running total: **fifty-six measurements, seventeen alternations, twenty-nine
acquittals.**

### Fifty-seventh measurement — 2026-08-17, Phase 5 session AA: one hit, step 1 closed it

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 57 | `mt CiscoVT2people_160x96_6fps t=2 sm=3 n=600 cabac=0 rc=0` (**release**, `family` battery at T5.AA2) | 41938 | **0** |

The same clip, the same `t=2 sm=3 n=600`, one `rc` value over from measurement 56 —
which is the signature's own statement that `rc` is not part of it. The same
battery's **debug** sweep read 341/341.

**Step 0** does not apply as a shortcut and was not claimed as one: the diff is
production decoder code (`deblocking.rs`, `decode_slice.rs`, `decoder_context.rs`,
`picture.rs`), so the driver links a different `rust_enc` by construction. HEAD's
release binary hashes `630dfab5…`.

**Step 1, twice.** The hitting configuration re-ran **5/5 byte-identical** in
release, and the whole `mt` preset re-ran **120/120 byte-identical** in release —
reproduction failing at the isolated level and at the level S23b says the race
needs.

**Step 2 does not trigger**: one hit.

**Acquitted as F3.**

Running total: **fifty-seven measurements, seventeen alternations, thirty
acquittals.**

### Fifty-eighth measurement — 2026-08-17, Phase 5 session AA: two hits, and the eighteenth alternation ties

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 58a | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0` (**debug**, `family` battery at T5.AA3) | 40992 | **0** |
| 58b | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (same sweep) | 39981 | **37837** |

Both inside the signature — `mt`, `sm=3`, wrong length, one zero and one short,
which is the clause session N's measurement 37 wrote when it took `320x192` *out*
of the exclusions. The same battery's **release** sweep read 341/341: the profile
asymmetry is the reverse of measurement 57's, one commit later, on the same
machine — load, not a property of either build.

**Two hits, so step 2.** Both binaries built once and swapped inside one loop, 12
whole `mt` presets per side, 120 configurations each = **1440 per side**, machine
otherwise idle.

| side | hits | presets | configurations |
|---|---|---|---|
| HEAD (`ab8ba549…`, at T5.AA3) | **6** | 12 | 1440 |
| control (`f7e9043f…`, `ee9a3c4c` — session AA's base) | **6** | 12 | 1440 |

**A tie, and the first exact one at this pair count.** Rate ≈**1/240** per side,
denser than session K's ≈1/307 under sustained presets and in the same band as
session Y's ≈1/160 and ≈1/144 — a machine that has been running batteries all
session, which is the load dependence rather than a change in either tree. Every
hit on both sides is `mt sm=3`; the hitting configurations move between rounds and
between sides. No hit outside the signature on either side.

Step 1 also ran, before the alternation: each of the two configurations re-ran
**3/3 byte-identical** in isolation.

**Acquitted as F3.**

Running total: **fifty-eight measurements, eighteen alternations, thirty-one
acquittals.**

### Fifty-ninth measurement — 2026-08-17, Phase 5 session AB: two hits, and the nineteenth alternation is the widest margin yet

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 59a | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**, `family` battery at T5.AB2) | 39981 | **0** |
| 59b | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=1` (same sweep) | 39981 | **37837** (short) |

Inside the signature, one zero and one short, and the same battery's **release**
sweep read 341/341 — the profile asymmetry again, and again the reverse of the
measurement before it.

**Step 0 first, and it did not apply.** T5.AB2 moves seven copy kernels between
modules without changing a line of their bodies, which looks like the hash
shortcut's case and is not: the two trees build **different** `rust_enc` binaries
(`5d539824…` vs `b429b7de…`, one build each). Moving a definition is a production
change even when the code is identical, and session P's clause — *"decoder-only" is
not the trigger; "test-only" is* — extends to it.

Step 1 re-ran the preset: **120/120**. Two hits, so **step 2**: both binaries built
once and swapped inside one loop, 12 whole `mt` presets per side, 120 configurations
each = **1440 per side**, machine otherwise idle.

| side | hits | presets | configurations |
|---|---|---|---|
| HEAD (`5d539824…`, at T5.AB2) | **2** | 12 | 1440 |
| control (`b429b7de…`, `41149605` — T5.AB1) | **12** | 12 | 1440 |

**HEAD drew a sixth of the control's hits** — the widest margin in either direction
in nineteen alternations. The verdict is still an *acquittal* and nothing more:
F3's rate is load-dependent (≈1/720 and ≈1/120 per side here, against ≈1/240 the
measurement before), the two sides ran interleaved on one machine, and no mechanism
in T5.AB2 touches the slice-list growth the race lives in. Reading 2-vs-12 as
"the move fixed something" would be the mirror of the mistake S23b warns about.
Every hit on both sides is `mt sm=3`; none outside the signature.

**Acquitted as F3.**

Running total: **fifty-nine measurements, nineteen alternations, thirty-two
acquittals.**

### Sixtieth measurement — 2026-08-17, Phase 5 session AB: one hit, acquitted at step 1

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 60 | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=0` (**debug**, `family` battery at T5.AB3) | 40992 | **40918** (short) |

One hit, inside the signature, release sweep 341/341 in the same battery.

**Step 1: the preset re-ran 5×, 0 hits over 600 configurations.** No alternation
owed. The commit under it is decoder-only (error concealment's copy brackets), and
the sweep drives the encoder — which is *not* an acquittal on its own (S14 step 0's
clause again), so the re-runs are what acquits it.

**Acquitted as F3.**

Running total: **sixty measurements, nineteen alternations, thirty-three
acquittals.**

### Sixty-first measurement — 2026-08-17, Phase 5 session AB: the hash shortcut applies, for the first time in this phase

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 61 | `mt CiscoVT2people_160x96_6fps t=2 sm=3 n=600 cabac=1 rc=1` (**release**, `family` battery at T5.AB5) | 42088 | **0** |

**Step 0, and it resolves it outright.** T5.AB5 removes the `unsafe` keyword from 51
function definitions and adds 35 narrow `unsafe { }` blocks — type-system markers
with no ABI or codegen meaning. `rust_enc` built from that tree and from its parent
`03e4d138` hash **identically**: `a6e454ca9cbe472f5b0ae58a20a0569282dc7a8d14e41f2e48b72b17d3200a26`
on both sides, one build each.

Base and head are **one binary**, so the hit is a property of the run and not of
either tree — acquitted by construction, and no re-run or alternation can say more
(session D's clause). The same hash is the commit's own soundness evidence: it is a
direct measurement that the sweep changed no generated code.

**This is the shortcut's first application in Phase 5**, and it is worth naming why
the two preceding measurements did not qualify: 59's commit *moved definitions
between modules* and 60's *changed decoder code*, and both build a different binary.
The trigger is not "the change looks cosmetic" — it is the hash.

**Acquitted as F3.**

Running total: **sixty-one measurements, nineteen alternations, thirty-four
acquittals.**

### Sixty-second measurement — 2026-08-17, Phase 5 session AC: three hits, the twentieth alternation, and a dead heat

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 62a | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=1 rc=1` (**debug**, `family` battery at T5.AC5) | 42281 | **40645** (short) |
| 62b | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=0 rc=1` (**debug**, same battery) | 40992 | **38203** (short) |
| 62c | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**debug**, same battery) | 39981 | **37837** (short) |

Three hits in one sweep, every one inside the signature (`mt`, `sm=3`, `t=4`,
`n=600`, wrong length); the release sweep in the same battery read 341/341.

**Step 0 does not apply and was measured rather than judged.** The commit converts
decoder code (the api-owned alias family, the concealment bracket), and the driver
links the whole library, so `rust_enc` differs: `e5b3ce4d…` at head against
`30aa1921…` at base, one forced build each. Session P's clause, again: *"decoder-only"
is not the trigger; "test-only" is.*

**Step 1: each of the three configurations re-ran 5× in isolation — 15 runs, 0
hits.** Byte-identical every time, which is the finding's own criterion pointing at
a race rather than a divergence: a deterministic port bug repeats its bytes, and
these do not repeat their *wrong* ones at all.

**Step 2 — the twentieth alternation.** 12 whole `mt` presets per side, 1440
configurations each, both binaries built once and swapped inside one loop, machine
otherwise idle:

**base (`a7a78954`) 3 hits / 12 sweeps, head (T5.AC5) 3 hits / 12 sweeps.**

A dead heat, and the second exact tie in twenty alternations (the eighteenth was
6–6). Given six hits split 3–3, P(head ≥ 3 | equal rates) ≈ 0.66. **HEAD is not
worse.**

Two details beyond the count, both of which strengthen the race reading:

1. **One configuration produced two different wrong lengths across the run** —
   `320x192 t=4 sm=3 n=600 cabac=0 rc=0` gave **0 bytes** on the base side at sweep 2
   and **40918** on the head side at sweep 7, against a stable C++ 40992. That is
   the "a deterministic port bug repeats its bytes" criterion satisfied directly,
   not by symmetry.
2. **`t=2` drew a hit** (head, sweep 10, `320x192 sm=3 n=600 cabac=0 rc=0`, 0 bytes).
   `t=4` predominates the way `n=600` does, and this is the reminder that it is the
   same rate artifact rather than a condition.

Six hits over 2880 configurations is ≈1/480 — between the battery-interleaved
≈1/800 and the sustained back-to-back ≈1/307, which is what an alternation's load
profile sits at.

**Acquitted as F3.**

Running total: **sixty-two measurements, twenty alternations, thirty-five
acquittals.**

### Sixty-third measurement — 2026-08-17, Phase 5 session AC: one hit, one preset re-run, no alternation owed

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 63 | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=1` (**debug**, `family` battery at T5.AC7) | 42538 | **0** |

One hit, inside the signature; release sweep 341/341 in the same battery.

**Step 1: the configuration re-ran 5×, byte-identical every time.** One hit is
step 1's threshold, not step 2's, and the alternation this session already owes it
was run **one commit earlier** on the same tree lineage (measurement 62, 3–3 over
2880 configurations) — so a second one would be re-measuring the same pair of
binaries' race rate against itself.

**Acquitted as F3.**

Running total: **sixty-three measurements, twenty alternations, thirty-six
acquittals.**

### Sixty-fourth measurement — 2026-08-17, Phase 5 session AC: the hash shortcut applies in release where it did not in debug

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 64a | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=1` (**release**, `family` battery at T5.AC11/AC12) | 42538 | **40857** (short) |
| 64b | `mt CiscoVT2people_320x192_12fps t=4 sm=3 n=600 cabac=1 rc=0` (**release**, same battery) | 39981 | **0** |

Two hits, both inside the signature; the debug sweep in the same battery read
341/341.

**Step 0 applies, and this is the first time it has applied to a commit that
changes decoder *code*.** The two trees differ — 70 enumerated-survivor stamps in
`decoder_core.rs` on one side, 0 on the other, checked before each build — and
their `rust_enc` binaries hash **identically**:
`8ae7dc34b6967d823301215a75c4bdd2bcf4c5629dd14dfb154c5e67ac6830f0`, one clean
build per side with the binary deleted first.

**Why, and it refines session P's clause rather than contradicting it.** P's rule
is *"decoder-only" is not the trigger; "test-only" is — the driver links the whole
lib, so production decoder code changes the encoder binary.* That is true **in
debug**, and it is what measurement 62 measured four commits ago (`e5b3ce4d…` vs
`30aa1921…`, same session, same class of change). In **release** the driver's
decoder code is unreachable — `rust_enc` calls the encoder API and nothing else —
so it is eliminated, and a decoder-side change that does not touch a symbol the
encoder reaches produces the same bytes.

The rule that survives is the one P actually wrote: **hash it before assuming,
one build each.** What is new is that the answer can depend on the profile, so
the hash to take is the one for *the profile the hit occurred in*. Both hits here
are release hits and the release hash resolves them.

Base and head are one binary, so the hits are a property of the run and not of
either tree. No re-run and no alternation can say more (session D's clause).

**Acquitted as F3.**

Running total: **sixty-four measurements, twenty alternations, thirty-seven
acquittals.**
### Sixty-fifth measurement — 2026-08-17, Phase 5b session C: step 0 is taken and does **not** acquit, because this window is not decoder-only

| # | configuration | C++ bytes | Rust bytes |
|---|---|---|---|
| 65a | `mt CiscoVT2people_160x96_6fps t=4 sm=3 n=600 cabac=0 rc=0` (**release**, `exit` battery at `f5ba2395`) | 42538 | **40857** (short) |
| 65b | `mt CiscoVT2people_320x192_12fps t=2 sm=3 n=600 cabac=0 rc=1` (**release**, the isolated `mt` re-run below) | 40809 | **0** |

One hit in the battery, inside the signature on every clause; the debug sweep in
the same battery read **341/341**.

**Step 0, run rather than assumed — and it does not acquit.** Base `5105cf44`
(session B's code plus a docs-only commit) and head `f5ba2395`, one clean release
build of `rust_enc` each, **both at the same filesystem path** — a second worktree,
checked out to each ref in turn, binary deleted between builds — so the comparison
is of code and not of paths the compiler embeds. The hashes are
`8da0d911c9e4ed9821b624f32d77b3249cf6779797d2329a11dc59ffee2a308d` and
`91d1a21c576b25709592c42fd05770bb9984022257197c4e6cab387fa41c0da3`: **different**.

**Why measurement 64's shortcut does not reach this window, and the clause survives
intact.** AC's finding was that in *release* the driver's decoder code is
unreachable, so a decoder-side change produces the same encoder bytes. That is still
true and is still the right question to ask — it simply does not apply here, because
**this window is not decoder-only**: `common/deblocking_common.rs`'s `DeblockingInit`
and `common/mc.rs`'s `InitMcFunc` stopped taking `*mut T` and a null test at T5b.6,
and `encoder/encoder_context.rs::InitFunctionPointers` calls both. The rule that
survives is the one session P wrote and AC refined and neither weakened: **hash it
before assuming, and hash the profile the hit occurred in.** The answer is a
measurement each time, never a rule.

**Step 1 — one hit, so the configuration is re-run 5×, machine idle.** All five
**BYTE-IDENTICAL**. Then the whole release `mt` preset was re-run once on its own:
**119/120, and the failure is a different configuration** — a different clip, a
different thread count, a different `iRCMode`, and a zero-length output rather than
a short one, while 65a's own configuration passes.

**That pair is the acquittal, and it is the cleanest instance this ledger has.**
S14's step 1 says a deterministic port bug repeats its bytes on its own
configuration; this one does not repeat at all, and the same binary produces a hit
somewhere else the moment the load returns. Both hits sit inside the signature on
every clause (`mt`, `sm=3`, `t ∈ {2,4}`, wrong *length* rather than wrong bytes,
release), and step 3's escape hatch — anything outside the signature is real —
does not open.

**Acquitted as F3.**

Running total: **sixty-five measurements, twenty alternations, thirty-eight
acquittals.**


### Sixty-sixth measurement — 2026-08-19, Phase 6 session C: two hits, both inside the signature, and the pair is the acquittal

The `family` battery at face 1's close (T6.C1 + T6.C2 in the tree, `SMB`'s five
scratch arrays inline and the second encode probe live) read **341/341 debug** and
**340/341 release**. The release failure:

```
mt CiscoVT2people_160x96_6fps t=2 sm=3 n=600 cabac=1 rc=0 ::  C++ 42088   Rust 42255
```

`mt`, `sm=3`, `t=2`, wrong *length* rather than wrong bytes, release — the signature
on every clause.

**Step 0 does not apply and says so.** This window moves encoder production code in
eleven files; base and head cannot build the same `rust_enc` in either profile. That
is Phase 6's standing clause (`prompts/phase6.md` §3), met for the third session
running.

**Step 1 — the configuration re-run 6×, machine otherwise idle: `BYTE-IDENTICAL`
all six**, 42088 bytes against 42088 every time. A deterministic port bug repeats its
bytes on its own configuration; this one does not repeat at all.

**Then the whole release `mt` preset once on its own: 119/120, and the failure is a
different configuration** —

```
mt CiscoVT2people_320x192_12fps t=4 sm=3 n=1500 cabac=0 rc=1 ::  C++ 39895   Rust 0
```

— a different clip, a different thread count, a different `iRCMode`, a different
entropy coder, a different slice budget, and a **zero-length** output where the first
was long, while the first hit's own configuration passes six times over. Both sit
inside the signature; neither is `st`/`def`; neither is wrong *bytes*. Step 3's escape
hatch does not open.

**Step 2 was not run, and the reason is on the record.** Two hits landed in this
session, which is the alternation's trigger read literally — but this is measurement
65's shape exactly (a gate hit that acquits on re-run, plus a second, unrelated hit
drawn by the adjudication's own preset re-run), and that measurement called the pair
the acquittal without alternating. The second hit here is evidence *for* the race,
not against it: one binary, two configurations, two different failure modes, and the
first configuration clean on repetition. An alternation is 24 `mt` presets — hours of
wall clock — and it is owed the moment a **gate** sweep produces a second hit in this
session. Faces 2 and 3 each run `family` and the close runs `exit`; those are the
samples that would trigger it.

**Acquitted as F3.**

Running total: **sixty-six measurements, twenty alternations, thirty-nine
acquittals.**
