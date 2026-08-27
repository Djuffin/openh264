# Safe-conversion plan — Session S3: owned fields and the MT lifecycle, then the writers and the slice core

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs. Re-run every count
before quoting it — trust the tree over this document, and before acting on any
claim anywhere, re-read the code it describes (rule S68: a claim of absence gets
its grep, a cited line gets read). Findings are numbered `F…` in
`rust/docs/phase9_findings.md`; yours start at **F221** (the count prints 120 today
— the F218 this brief's first draft named already exists; it is cited below). The operative plan is
`rust/docs/safe_conversion_execution_plan.md` — read it first, its §10 amendments
included; this session is its S3, and it opens with S2's B-tail.*

## Where S2 left the tree

**Stage A is complete.** S2 landed three checkpoints, each its own gated commit,
and stopped at the stage boundary per drop-from-the-end. It converted the
video-analysis block (A5), the kernel dispatch table (A6, F212's flip) and the
coding parameters (A7, 258 sites). The close gate — `gates.sh session` — is
green: 583/583 sweeps in both profiles, 561 debug / 554 release tests, Miri
291/291 across four shards (**cpu 1105 s against S1's 1091 s, ratio 1.01**),
gtest smoke 4/4.

**Your first job, before any conversion: run the two MT probes.** §4.7 requires
`fork_join_encodes_a_multi_slice_frame_under_the_aliasing_checker` and
`fork_join_encodes_a_frame_whose_slice_boundary_is_mid_row` for anything touching
`svc_encode_slice.rs` / `slice_multi_threading.rs`; A7 touched both, and
**neither probe ran at S2's close** — see F220 for exactly what that leaves
unverified. `gates.sh session` does not run them (its lane filters
`--skip 'fork_join_encodes'`), so it is one command, and it wants ~25 minutes
because `[profile.dev] opt-level = 3` makes every Miri invocation recompile:

    MIRIFLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation" \
      cargo +nightly miri test --lib -- fork_join_encodes

Run it against S2's tree, before your first edit, so a failure is attributed
where it belongs.

**Every remaining `unsafe fn` accessor on the god-struct is the DQ-layer
family**, and that is the single most important fact in this brief. Read F216
before you plan anything.

### The design, unchanged, and the two rules that now sit under it

Readers take `&self` and are what everyone calls, the fork included; an in-fork
site spells it `(*pCtx).name()`. Writers take `&mut self` and are
single-threaded only. Where a return stays raw it is because the far end cannot
carry a lifetime, or because of one of the two rules below; those accessors are
still *safe fns*.

* **F208** — a `&self` accessor is a `SharedReadOnly` retag over the **whole**
  context. It may not be called while a `&mut`-shaped derivation into that same
  context is live. Borrowck referees it where the context is a reference; where
  it is a raw, **only Miri does**.
* **F215, new in S2 and the one that cost real time** — a `&mut self` accessor
  is a **fresh whole-struct `Unique` retag per call**. Two raw cursors into that
  struct may only coexist if they come off the *same* call, and a cursor that
  must outlive a later call keeps the slot-read root (F71).

Three named raw survivors follow from those two rules, and each has a written
reason and a counted call set — they are not debt:

| survivor | why | callers |
|---|---|---:|
| `ctx_param_raw` (A7) | F215: 26 per-layer cursors held across a call that reaches the parameters again | 30 |
| `ctx_ref_list_raw` (A3) | F211: the value is *stored* in `SDqLayer::pRefList` and read by the fork for a frame, so it must carry the list's own provenance | 4 |
| `ctx_func_list_raw` (A6) | the two bodies that *write* the table hold the context as a **raw**, and `func_list_mut` would be a whole-context `&mut` retag through a raw root | 8 |

**Run Miri inside every checkpoint, not at the close.** S2's A7 hit two UB sites
twenty minutes apart; both profiles' sweeps were 583/583 and all 561 tests passed
through both. The instruments that exist —
`rust/tools/f208_reader_retag_scan.py` (add your new accessors to its `READERS`
list; it has a `CLEARED` table for hand-audited false positives),
`rust/tools/safeplan_prohibitions.py <writer>…`,
`rust/tools/safeplan_prohibition2.py` — are all necessary and none is sufficient.
The join analyser (`rust/tools/phase9_ctx_join.py`) now over-reports on the
converted layer: it is a text scan and cannot see that two sites sit in the two
arms of one `if`. Read its LIVE rows, do not count them.

## What S3 does

**B1–B3 first, then stage D. But read the ordering question before you start.**

| # | scope | notes |
|---|---|---|
| **B1** | `pOut`/`pVpp`/`pSliceThreading` → `Option<Box<T>>`; `mutexSliceNumUpdate` → owned `Mutex<()>`; rewire `RequestMtResource`/`ReleaseMtResource` | **This is an MT-seam checkpoint, not bookkeeping — F217.** All four fields are reached from in-fork bodies, and `slice_bs_buffer` hands out `&mut (&mut *(*pEncCtx).pOut).sBsBuffer[..]` **in the fork**. Under the design an in-fork body may take only the `&self` reader, so the bitstream buffer needs either D1's `SharedCells` treatment or a demonstration that the arm is unreachable under MT. The fence is measured in F217: the arm is taken when `iMultipleThreadIdc <= 1` **or** the layer is `SM_SINGLE_SLICE`, and whether the second case still spawns a worker that reaches `slice_bs_buffer` is a probe B1 runs *first*. Sites: `pOut` 125, `pVpp` 46, `pSliceThreading` 28, the mutex 8 |
| **B2** | `encoder_ext.rs` sweep (38 unsafe fn) | mostly cascade after B1 — but see F216: the DQ-layer family gates most of it too |
| **B3** | `wels_encoder_ext.rs` (16 fn / 73 raw): frame-loop orchestration; the 2 version exports stay | this file is where prohibition 2's auto-ref count sits (24 of 29): its `pCtx` is `Self::ctx_ptr`'s raw, so every parameter write A7 converted is a `&mut` through a raw root. Converting the file's root retires all 24 at once |
| **D1–D4** | stage D as the plan has it | writers onto `safe::bits`; `svc_encode_slice.rs` in two checkpoints; MT residue; `deblocking_common` + `mc`/`copy_mb`. **Perf duties (plan §10.4/§6): D1 runs the bench before and after inside the session, and stage D's close checks the 3% budget** — shape fixes only, never `unsafe` back |

### The ordering question, and S2's recommendation

The plan runs B before D. **F216 argues D2's layer work should come first**, and
S3 should decide this deliberately rather than by default:

* Stage A retired **every accessor in the plan's §3(a) inventory except three**
  — `ctx_dq_layer`, `ctx_ref_pic`, `ctx_pic_ref`, all three the layer/picture
  family — and moved the tracking number **627 → 612**. Every body it touched is
  still `unsafe` because it also calls `current_layer` (158 sites) or one of the
  ten relatives beside it (`layer_sps`, `layer_subset_sps`, `layer_pps`,
  `layer_ref_pic`, `layer_enc_pic`, `layer_rec_view`,
  `layer_ref_feature_storage`, and the three above). §3(a) does not list any of
  the `layer_*` seven, which is part of why the stage looked smaller than it is.
  The cascade the plan bills to stage A is real and is sitting in escrow behind
  that one family.
* F210 is why it was deferred: the fork **writes** the DQ layer, so `dq_layer`
  can return neither `&mut` (S63) nor `&`. The ordering constraint is
  **layer-before-accessor** — the layer's *storage* has to move first, which is
  D2–D3's own work. **F218 corrects the size**: the "42 in-fork `*mut SDqLayer`
  parameters" F191 and F210 both quote is 42 *tree-wide* and **11** in-fork. The
  eleven are what a handle redesign must satisfy; the other thirty-one convert
  under the ordinary single-threaded rules stage A has been using.
* So the choice is: do B1–B3 first as written, and collect nothing visible for a
  third session running; or open with the layer, take the cascade, and let B1's
  singletons land afterwards against a tree where most of their callers have
  already shed `unsafe`.

S2's read is the second, and B1 is the reason: B1's hardest problem
(`slice_bs_buffer`'s in-fork `&mut` into `pOut`) is the *same* problem as the
layer's, one field over. Solving the layer first gives B1 a pattern to copy
rather than a design to invent. But this is S3's call, and either way it is a
checkpoint-boundary decision, made before any conversion starts, and recorded.

### If you take layer-first: the family, measured (steward addition at review)

The routes into the DQ layer and its pictures, fresh at this commit
(`grep -rn '\b<name>(' src`, mentions incl. defs/docs):

| route | sites | | route | sites |
|---|---:|---|---|---:|
| `current_layer` | 142 | | `layer_ref_pic` | 31 |
| `layer_rec_view` | 25 | | `layer_pps` | 23 |
| `layer_enc_pic` | 22 | | `ctx_dq_layer` | 15 |
| `layer_sps` / `layer_subset_sps` | 5 / 2 | | `layer_ref_feature_storage` | 5 |
| `set_current_layer` | 5 | | `ctx_ref_pic` / `ctx_pic_ref` | 4 / 2 |

plus **42** `: *mut SDqLayer` parameters (`grep -rn ': \*mut SDqLayer' src/encoder
| grep -v ':\s*//' | wc -l`), of which **11 are in-fork** (F218) — the eleven are
the redesign's real clients; the other thirty-one convert under stage A's ordinary
single-threaded rules.

Three facts that size the work smaller than it looks:

1. **The storage is already owned.** `sWelsEncCtx::ppDqLayerList` is a `Vec` of
   owned layers (`encoder_context.rs:1538` — read the field), and `current_layer`
   already resolves a *position* (`iCurDqLayer`, the identity-moved design at
   `:1517`) rather than holding an alias. D2's "aliases → handles" is therefore an
   accessor-and-parameter job, not a storage migration: readers become `&self`
   resolution (position → `&SDqLayer`), single-threaded writers `&mut self`, and
   the position itself is the handle (S37: resolved per call, never stored).
2. **The diagonal exists here — classify first, and expect writers in-fork**
   (F210, unlike every stage-A accessor). The eleven in-fork parameters are where
   the fork *writes* the layer; each is either per-slice-disjoint (the worker
   writes only its own slice's rows — provable, and then the D1 `SharedCells`
   precedent applies field by field) or stays raw-tagged until D3 handles it.
   F208 and F215 bind any new layer accessor identically.
3. **F216's escrow pays out here**: the stage-A bodies still `unsafe` are so
   only for these calls, so the cascade the tracking number has been waiting for
   lands with this family — measure it per checkpoint, don't promise it.

## The protocol that worked, compressed

1. **Classify before converting.** For each accessor or field, tabulate every
   body that reaches it by (a) the form of its context parameter and (b) whether
   it *writes* through the answer. S2's three accessors all came back with no
   diagonal — every writer took `&mut sWelsEncCtx` or sat on the C-API/init path,
   every in-fork body only read. Do not assume it holds for the layer; F210 says
   it does not.
2. **Convert naively, then let the compiler enumerate.** F196's method. A5 opened
   at 12 errors, A6 at 14, A7 at 70; each resolved to ~20 bodies of work.
3. **Resolve in §4.6's order.** *Reorder* first — in practice, read the `Copy`
   scalars out above the writer's `&mut`; that was the remedy at roughly forty of
   S2's sites and it is behaviour-preserving by construction. Then a **combined
   accessor** where two *different fields* are genuinely wanted at once: S2 minted
   four (`vaa_ref_list_and_ltr_mut`, `vaa_and_rc_at_mut`, `param_and_rc_at_mut`,
   `param_and_paraset_arrays_mut`), each on `ctx_paraset_arrays`'s precedent.
4. **Narrow the callee before you convert its argument.** Five of A7's callees
   lost their `pParam` parameter entirely because they already held the context
   (F192's shape), and `ReallocateSliceList` lost a `*mut SSliceArgument` in
   favour of the one `Copy` field it read (S54). That is usually cheaper than
   making the argument a reference, and it removes a route rather than retyping it.
5. **Gate at `family`, not `commit`.** Every S2 checkpoint ran family (~3 min) and
   the Miri encode shards mid-checkpoint (~5 min). The commit level does not
   sweep, and it does not run Miri at all.
6. **Then**: `f208_reader_retag_scan.py`, `safeplan_prohibitions.py`,
   `safeplan_prohibition2.py`, `unsafe_ratchet.sh generate`, commit.

## Numbers at S2's close (re-measure; do not quote these)

```
ratchet      raw_ptr 1095   unsafe_fn 579   unsafe_block 265   unsafe_impl 2
tracking     #[allow(unsafe_code)] outside src/api/ : 612
             (`rust/tools/safeplan_tracking.sh [ref]` — it has a command now, F219)
prohibition1 101 *mut-ctx bodies vs 15 writers, 0 violations
prohibition2 explicit 27   auto-ref 29  (24 of them in wels_encoder_ext.rs — B3's)
f208 scan    3 candidates, 0 needing a hand read (1 CLEARED, hand-audited)
join         12 hazards, 2 LIVE, 10 moot   (both LIVE are text-scan false
             positives in `WelsEncoderParamAdjust` — the derivation and the call
             sit in the two arms of one `if bNeedReset`; see A7's commit)
forksplit    101 bodies carry *mut sWelsEncCtx, 98 in-fork + 3 ST
miri lane    677 s wall / 1105 s cpu — S61 compares CPU against CPU (F170)
```

The tracking number is the plan's single progress figure and it moved **613 →
612** across S2, and **627 → 612** across the whole of stage A. (S1's close logged
614; the tree says 613 — F219, and the reason the figure now has a tool.) F214 and F216
explain why that is structural work rather than a stalled session, and both
should be read before the figure is quoted at anyone.

## Ground rules, unchanged

- **Bit-exactness is stop-the-line.** A diffharness SHA divergence is a bug in
  your change. A test failure that does not reproduce follows the F3
  adjudication protocol in `phase0_findings.md` (5/5 re-run; a second hit
  escalates to head-vs-control alternation; never shrug, never bisect a phantom).
- No edits while a gate runs; one gate at a time; blockers become findings.
- A tag comes off only with the `unsafe` it annotates — never early, never stale.
- Every count you quote carries the command that produced it. S1 quoted a
  prohibition-2 figure of 22 without recording its grep and no spelling
  reproduces it; S2 pinned the check to `rust/tools/safeplan_prohibition2.py`
  instead of guessing.

## What to report back

Plain prose: per-checkpoint commits with gate verdicts; the ordering decision
above and what it was based on; the per-accessor / per-field outcome table
(reader / writer / in-fork spelling / combined accessor if any); B1's measurement
of whether `slice_bs_buffer`'s `pOut` arm is reachable under MT; the join and
forksplit headlines before and after; both prohibitions plus the F208 scan at the
close; the close gate's Miri **CPU** number against S2's 1105 s **and the two MT probes
run again at your own close** (§4.7 — this session touches all three seam files;
the opening run proves S2's tree, the closing run proves yours); the tracking
number's movement; every place this brief was wrong, quoting the sentence; and
the hand-off to S4.
