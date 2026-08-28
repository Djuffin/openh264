# Safe-conversion plan — Session S5: finish stage C, then the writers and the slice core

*Self-contained. Numbers in parentheses (F…, S…, D…) point at project docs. **Re-run
every count before quoting it** — trust the tree over this document, and before acting
on any claim here, re-read the code it describes (rule S68: a claim of absence gets its
grep, a cited line gets read). Findings are in `rust/docs/phase9_findings.md`; yours
start at **F228** (the count prints 127 today — check it). The operative plan is
`rust/docs/safe_conversion_execution_plan.md`, §10 amendments included.*

## Where S4 left the tree

Eight gated commits, `410b9c13..19ff3142`. **The plan's progress number moved
614 → 532**, the largest movement since it was defined, and most of it came from one
compiler-driven pass rather than from hand conversion.

| commit | what |
|---|---|
| `e19ea6fd` | **step 0** — the inherited MT-probe duty, discharged |
| `c938b875` | **C1** — the fn-pointer table's ten unsafe aliases |
| `63303338` | **C2** — `SPicData`'s nine roving pixel cursors |
| `de1267b0` | **F226** — a live data race, fixed, with a referee |
| `0f384e4b` | **C3** — every non-ctx raw param in the mode-decision pair |
| `a0796cf9` | **C4a** — the MD helpers; MVD-cost family scoped out |
| `aefac7dd` | **C-cascade** — 87 signatures + 72 allows, one fixpoint |
| `19ff3142` | **C-params** — four more params, one accessor |

### Numbers at S4's close (re-measure; do not quote these)

```
ratchet      raw_ptr 1043   unsafe_fn 483   unsafe_block 264   unsafe_impl 2
tracking     #[allow(unsafe_code)] outside src/api/ : 532   (was 614 at S4's open)
prohibition1 106 *mut-ctx bodies vs the 15 writers (listed in F222), 0 violations
prohibition2 explicit 10 (decoder 6, api 1, encoder 3), auto-ref 0
f208 scan    0 candidates
join         10 hazards, 0 LIVE
forksplit    SDqLayer 38 in-fork / 2 ST   (unchanged — D2's population)
```

**Run the instruments from the crate root** (`rust/crates/openh264-rs`). They glob
`src/**/*.rs`; from the repo root they match nothing and print a **false zero**. S4
lost a measurement to this before noticing.

## Your first duty: the MT probes stage C owes

S4 landed six conversion checkpoints touching fork-reachable code and, by the user's
direction, **did not re-run the fork pair after step 0**. It is owed before stage C
closes. One command now, where S3 and S4 both had to reconstruct a two-line
incantation:

```bash
rust/tools/fork_join_probe.sh
```

Background it — a foreground run dies at the harness's 600 s cap mid-compile, which
cost S3 ~25 min twice. **~59 minutes** as a parallel pair; the compile is ~5 s on a
warm `target/miri` and minutes from cold. It compares against
`rust/tools/fork_join_baseline.txt` and warns past 1.3x. Do **not** `pkill -f` on a
pattern matching your own runner — S3 killed its one healthy run that way.

What S4's own probes measured, for your ratio: **3463.45 s / 3530.38 s, pair 3535 s**.

## What S5 does

**C4b, C5, C6 to close stage C — then D1–D4, then E1–E3.** Every remaining item is a
campaign; none is a tail to rush at the end of another checkpoint.

### C4b — the MVD-cost family (start here; it is the one with a perf question)

`pMvdCost` is a **three-level biased cursor**, and the levels compose:

```
(*pMd).pMvdCost   = pMvdCostTable.add(luma_qp * stride)         // QP row base
pFeatureSearchIn.pMvdCostX = sMe.pMvdCost.offset(-iCurPixXQpel - sMvp.iMvX)
pMvdCost          = pMvdTable.offset(iMinMv * 4 - sMvp.iMvY)    // then .add(4) per step
```

`COST_MVD` indexes the result with a **signed** MVD, so every level is a pointer
deliberately parked in the middle of its table. **80 mentions across seven files, 25
call sites, and it is the innermost loop in the encoder.**

`mvd_cost_origin`'s own doc names both the destination ("the field is stage C's to
convert") and the obstacle: `SWelsME` is `repr(C)` + `Copy` and passed by value, so it
cannot hold a slice without a lifetime that cascades into `SWelsMD` and the context.
**S4 measured the other route and it is the deciding fact: none of the ME search
functions take the context** — `WelsMotionEstimateSearchStatic`,
`WelsMotionEstimateInitialPoint`, `WelsDiamondSearch`, `CheckDirectionalMv` all take
`pMeFuncs`, `sdf`, `pMe`, `pSlice` and the two planes. So an index representation
(`usize` base into the context's table) means threading `&[u16]` through the whole
motion-search tree. Price both before choosing.

**§10.4 binds C4b**: bench before and after, inside the session. Use S4's method — the
absolute numbers in `perf_baseline.md` are **not** comparable on an arbitrary machine
(S4's reads every row at ~0.5x C++ where the baseline has Rust faster, at every commit
alike). Measure the same command on the same machine either side of your own change,
and take two after-runs: S4 saw a first-run +3.0% on 1080p SMPTE that did not
reproduce (-1.0% on the second).

### C5 — screen-content feature arenas

`SScreenBlockFeatureStorage`'s `*mut *mut u16` tables and the ME mirrors. **This is
dark code**: `WelsInitSCDPskipFunc` installs the judging arm only when
`bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`, and
`bScreenContent` is an axis **neither diffharness driver expresses** — a probe in
`SvcMdSCDMbEnc` read 0 in all 48 `bg` rows. So the sweep cannot adjudicate a semantic
change here. Convert **containers only**, keep semantics byte-for-byte, and lean on the
compiler; the plan asks for the screen-content differential before/after.

### C6 — preprocess views

`wels_preprocess.rs` measures **198 raw derefs** with `unsafe` stripped (S4 ran it),
so it is not a cheap sweep. Its centre is `SPixMap::pPixel: [*mut u8; 3]` — 30 use
sites, 24 of them in `wels_preprocess.rs` itself and the rest in `processing/`
(`vaacalc`, `background_detection`, `adaptive_quantization`), where it is the one
`from_raw_parts` left in each. Convert `SPixMap` and those files follow.

### Then D1–D4, then E1–E3

Unchanged from S4's brief, with one correction below. **E2's lint flip cannot happen
until stage C and D are done** — `svc_motion_estimate.rs` alone still carries 32
allows.

## The rules that earned their keep in S4, and one correction to the brief

* **F215/F227 — two raw cursors must come off ONE derivation.** C4a converted a
  parameter to `&mut [u16]` and took `as_mut_ptr()` **twice**; the second call is a
  fresh `Unique` retag that pops the first cursor's tag. `family` passed it whole
  (583/583 byte-identical, both profiles) because the defect changes no byte. **The
  correction to S4's brief's gate split**: it said stage C runs `family` per checkpoint
  and reserves `session` for the seam files. That is wrong in one specific way — **a
  checkpoint that converts a raw parameter into a slice or reference is a Miri question
  wherever it lives.** The seam is what makes the *MT probes* necessary; a byte gate
  cannot see a retag at all. Gate those at `session`.
* **F223 rule three — in-fork, a `&mut` retag is a write.** S4 refused it three times:
  `PSetScrollingMv` (C1), and both VAA parameters (C3) went `*mut` → **`&`**, shared,
  because their bodies only read and every one is fork-reachable. When you narrow a
  parameter on the shared VAA block or the layer, shared is almost always the answer.
* **F226 — the load-balancing fork has no gate.** `UpdateMbMapForked` is reachable only
  with `bUseLoadBalancing`, which both diffharness drivers and both MT probes pin off,
  and its only covering test is `#[cfg_attr(miri, ignore)]`. If you touch anything
  under it, the referee is
  `update_mb_map_forked_workers_share_the_layer_without_racing` (in
  `svc_encode_slice.rs`, 1.24 s under Miri) — and note the pattern it uses: **drop the
  encoder**, build the layer by hand, spawn the same shape. That bought eight hours of
  coverage for a second.
* **Ask the compiler, not a regex.** "Which fns are only unsafe because their callees
  are?" is answered by stripping `unsafe` on a scratch copy and reading the errors.
  A regex over bodies counts `(*pMbCache)` — a deref of a *reference* — and lies. S4
  made that mistake once in C3 and then used the compiler for the cascade, which is
  where 87 of its 87 signature conversions came from.
* **`safeplan_prohibitions.py` still does not skip comment lines** (F222, left as-is by
  S3 and by S4). A writer's name in prose inside an in-fork body is a red gate on a
  comment. The fix is one line copied from `safeplan_prohibition2.py`.

## D2, priced properly — the classification S4 did before converting

S4 tabulated all 40 `*mut SDqLayer` bodies before touching any (the protocol's step 1).
**Three write through the layer**, not 38:

| body | write | disposition |
|---|---|---|
| `UpdateMbListNeighborParallel` | `&mut (*pCurDq).sSliceEncCtx` for one scalar **read** | **F226 — fixed in S4** |
| `ReallocateSliceList` | `&mut (*pDqLayer).sSliceBufferInfo[kiBank].pSliceBuffer` | lawful: per-worker bank (T7.B2), sibling ranges |
| `ReallocateSliceInThread` | `(*pDqLayer).sSliceBufferInfo[slot].iMaxSliceNum = ..` | lawful per-worker disjoint raw write |

The other 37 only read the layer struct. **But they still cannot take `&SDqLayer`**,
and this is the correction that matters most for D2: a `&SDqLayer` is a
`SharedReadOnly` retag over the **whole** struct, and under `SM_SIZELIMITED_SLICE` the
fork's workers write layer-*inline* bytes concurrently — `sSliceBufferInfo[slot]`'s
scalars and that bank's `Vec` header, plus the per-partition counter arrays
(`NumSliceCodedOfPartition`, `LastCodedMbIdxOfPartition`, `[i32; MAX_THREADS_NUM]`
inline, incremented from inside the encode). A whole-layer shared retag racing those
writes is UB.

**So S4's brief was wrong that "D2 is an accessor-and-parameter job, not a storage
migration."** The layer's own storage is owned, which is what that claim rested on —
but the *fields inside it* are worker-written, and the flip needs them moved out (the
banks to their own allocation, the counters to atomics) before any of the 37 can take
a reference. Price D2 as an MT-seam checkpoint of B1's class — the class that produced
F223's two defects — and gate it accordingly.

The good news the same measurement gives you: the mid-row boundary probe
(`fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row`) runs
`SM_SIZELIMITED_SLICE` **at two threads**, so the realloc concurrency this turns on is
actually covered. Unlike F226's fork, this one has a referee.

## What to report back

Plain prose: per-checkpoint commits with gate verdicts; C4b's bench numbers before and
after, and stage C's 3% check; the MT probes' verdict at every landing that touches the
seam, with the ratio against `fork_join_baseline.txt`; both prohibitions plus the F208
scan at the close; the tracking number's movement; **every place this brief was wrong,
quoting the sentence**; and the roll-forward line naming everything owed — not just
checkpoints, but probes, benches and findings. S4's brief had stage C silently dropped
from that line at the S2→S3 hand-off and it cost a whole stage; name what you are
handing on.
