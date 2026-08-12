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
