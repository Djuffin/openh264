# Safe-conversion plan — Session S4: the writers and the slice core

*Self-contained: everything you need is explained here in plain words; numbers in
parentheses (F…, S…, D…) point at entries in the project docs. Re-run every count
before quoting it — trust the tree over this document, and before acting on any
claim anywhere, re-read the code it describes (rule S68: a claim of absence gets
its grep, a cited line gets read). Findings are numbered `F…` in
`rust/docs/phase9_findings.md`; yours start at **F225** (the count prints 124
today — check it). The operative plan is
`rust/docs/safe_conversion_execution_plan.md` — read it first, its §10 amendments
included; this session is its **stage D**, and stage B closed cleanly ahead of it.*

## Where S3 left the tree

**Stage B is complete.** Three checkpoints, each its own gated commit, in
**reverse plan order** — B3, then B2, then B1 — for a reason you should read
before you plan anything (F221, and "the ordering question" below).

| checkpoint | what it did |
|---|---|
| **B3** `0dc2e7bb` | `wels_encoder_ext.rs`'s raw context root deleted. All five `Self::ctx_ptr` callers resolve the context off `m_pEncContext` as a reference; `ctx_ptr` itself is gone |
| **B2** `8c3de9fb` | `encoder_ext.rs`'s five `*mut *mut sWelsEncCtx` parameters → `&mut sWelsEncCtx`; `WelsInitEncoderExt`/`WelsUninitEncoderExt` build and tear down through the `Box`, `into_raw`/`from_raw` bracket deleted. `RequestMtResource`/`ReleaseMtResource` came with them (B1's row in the plan, forced here by the signature change) |
| **B1** `980bfb16` | `pOut`/`pVpp`/`pSliceThreading` → `Option<Box<T>>`, `mutexSliceNumUpdate` → an owned `Mutex<()>`. `WelsMutexInit`/`WelsMutexDestroy` deleted, `with_wels_mutex` now a safe fn |

### Numbers at S3's close (re-measure; do not quote these)

```
ratchet      raw_ptr 1088   unsafe_fn 580   unsafe_block 265   unsafe_impl 2
tracking     #[allow(unsafe_code)] outside src/api/ : 614
             (`rust/tools/safeplan_tracking.sh [ref]`) — it went UP, 612 → 614, and
             F224 explains why that is the honest figure for a checkpoint that
             strictly reduced raw exposure. Read it before quoting the number.
prohibition1 106 *mut-ctx bodies vs 15 writers, 0 violations
             (the writer list is written out in F222 — S2's "15" had no list attached)
prohibition2 explicit 10  (decoder 6, api 1, encoder 3), auto-ref **0** (was 29)
f208 scan    **0 candidates**  (was 3)
join         10 hazards, **0 LIVE** (was 2 — both were `pOldParam` in `WelsEncoderParamAdjust`)
forksplit    106 bodies carry *mut sWelsEncCtx, 101 in-fork + 5 ST
abi          `abi_sizes.sh` byte-identical: 51 types, 140 field offsets
```

Both prohibition-2 counts and the F208 scanner are now at or near zero **on the
encoder side**; what is left there is two test asserts that deliberately probe
`ctx_sps`'s aliasing property and one `ctx_ltr_at` call in `wels_preprocess.rs`.
The decoder's six are E1's.

### The five named raw survivors, and why each exists

Stage A left three. B1 added three and B3 retired one caller of another; the
family is now the place the whole design's aliasing contract is written down.
**They are not debt.** Each returns a value read out of a `Box`'s slot (F71), so
it carries the pointee allocation's own provenance and cannot be popped by any
retag of the context.

| survivor | why | shape |
|---|---|---|
| `ctx_param_raw` (A7) | F215: 26 per-layer cursors held across a call that reaches the parameters again | raw |
| `ctx_ref_list_raw` (A3) | F211: the value is *stored* in `SDqLayer::pRefList` and read by the fork for a frame | raw |
| `ctx_func_list_raw` (A6) | the two bodies that *write* the table hold the context as a raw. **B3 removed one** — `SetOption`'s RC_MODE arm now calls `func_list_mut()`; `ParasetStrategy` is the other | raw |
| `ctx_out_raw`, `ctx_slice_threading_raw` (B1) | the fork's route to `pOut`/`pSliceThreading` now that both are owned | raw |
| `ctx_src_pool_raw` (B1) | F211's category again: the answer is stored in `SDqLayer::pSrcPool` and read by the fork for a frame | raw |
| `ctx_vpp_ref` / `ctx_vpp_raw` (B1) | **the reader/writer split, and read F223 before you touch it** | `&` in-fork / `&mut` ST |

`phase9_forksplit.py --list` classifies them correctly on its own now:
`ctx_out_raw`, `ctx_vpp_ref` and `ctx_slice_threading_raw` land in-fork;
`ctx_vpp_raw` and `ctx_src_pool_raw` land ST-flippable. If a change of yours moves
one across that line, that is the signal to stop and re-read.

## The one thing to read before you write any code: F223

B1 converted a single field (`pVpp`) from a raw to an owned `Box` and **produced
two aliasing defects in a row**, each invisible to the gate that had just passed:

1. An `Option::take` dance passed `gates.sh family` — **583/583 byte-identical in
   both profiles**, 561/554 tests — and the **Miri encode shards** refused it:
   `SDqLayer::pSrcPool` is a raw into a *field of the vpp allocation*, so the
   `Box`'s per-call `Unique` and the pool's shared reads pop each other. That is
   F215's rule one allocation further in.
2. The fix (a slot read returning `&mut`) passed the encode shards **2/2** and
   both sweeps, and was committed. The **§4.7 MT probes** then refused it as a
   *data race*: two workers each retagging `&mut` in `WelsSliceHeaderExtInit`.
   A `&mut` retag is a write to the race model, so N workers taking one is S63
   with no read of the object required. Both bodies wanted a `&self` read.

**The ladder, and it is stage D's whole risk profile**: byte sweeps cannot see a
retag that changes no byte; **single-threaded Miri cannot see a race between
workers**. Defect 2 is the first recorded instance of that second gap in this
project, and stage D is the only stage where nearly every checkpoint is exposed to
it.

**So: gate at `session` per checkpoint, and run the two MT probes with every
landing that touches `svc_encode_slice.rs`, `slice_multi_threading.rs` or
`rec_view.rs` — which in stage D is all of them.**

### Your first duty, before any conversion — and it is inherited, not new

**S3 closes with the B1 fix unverified by the probes.** The data race in F223 was
*found* by a probe run; the fix that answers it never got a completed one. Five
attempts at S3's close produced no verdict (two killed by a 600 s foreground cap
mid-compile, one by an over-broad `pkill`, two stopped by direction). So run this
against the tree as you find it, **before your first edit**, exactly as S3's own
brief asked of S2's tree and for the same reason — a failure then belongs to B1,
not to you:

    # one compile, then two concurrent shards — gates.sh's own Miri-lane pattern
    MIRIFLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation" \
      cargo +nightly miri test --lib -- fork_join_encodes_a_multi_slice_frame &
    MIRIFLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation" \
      cargo +nightly miri test --lib -- fork_join_encodes_a_frame_whose_slice_boundary &
    wait

**Three mechanics S3 paid to learn:**

* **Background it.** A foreground run hits the harness's 600 s cap and is killed
  mid-compile — that cost ~25 min of pure compile, twice.
* **Compile once, then shard.** The Miri compile is **~12 min per invocation**
  (`[profile.dev] opt-level = 3` forces a full rebuild). Concurrent shards after
  a warm `target/miri` skip it entirely and the wall time becomes the slowest
  single probe rather than the sum. This is what `gates.sh`'s Miri lane already
  does — one compile-only pass, then four shards; do not invent a different
  scheme. What does **not** work is launching duplicate full invocations: they
  contend on the cargo target lock and on 8 cores, and S3 had three crawling at
  once before noticing.
* **Never `pkill -f` on a pattern that matches your own survivor.** S3 killed the
  one healthy run that way.

Measured wall times, for planning: Miri **compile ~12 min**; the `encode_loop_runs`
shards **~350 s** of execution (347.6 / 352.1 / 350.9 over three runs), so ~18 min
end to end cold. The `fork_join_encodes` probes have **no completed measurement** —
S2 never got one either. You will produce the first.

`gates.sh session` does **not** run the probes (its lane filters
`--skip 'fork_join_encodes'`), so they are always a separate command.

## What S4 does

**D1–D4, the plan as written, and then E1–E3 if the session holds.** Stage D is
the biggest single block in the plan and `svc_encode_slice.rs` is its largest file
(81 fn / 117 raw at the plan's census; re-measure).

| # | scope | notes |
|---|---|---|
| **D1** | Bit writers onto `safe::bits`: `svc_set_mb_syn_cavlc/cabac`, `set_mb_syn_cabac`, `vlc_encoder`, `nal_encap`, `encode_mb_aux`/`decode_mb_aux` | **§10.4: run the bench before and after, inside the session** — writer-loop regressions localize badly after the fact. `slice_bs_buffer`/`slice_writer` are the seam these reach the output through, and F217's arm is now **measured** main-thread-only (below) |
| **D2** | `svc_encode_slice.rs` part 1: per-slice aliases → handles (§3c-5), slice construction/teardown. **This is where the DQ-layer family finally converts** | **Price it off F221's 38, not F218's 11** |
| **D3** | `svc_encode_slice.rs` part 2: the encode loop, dynamic slicing, `svc_enc_slice_segment.rs` | MT probes gate every landing |
| **D4** | `slice_multi_threading.rs` residue + `common/deblocking_common.rs` raw-twin retirement + `encoder/deblocking.rs`, `mc.rs`, `copy_mb.rs` | |

**Perf duties (plan §6/§10.4)**: D1 runs the bench inside the session; **stage D's
close checks the 3% budget** against `perf_baseline.md`. Shape fixes only, never
`unsafe` back, never by trading away `overflow-checks`/`debug-assertions`.

### The ordering question, settled with a measurement S2's brief got wrong

S2 recommended S3 open with the layer (D2) rather than stage B, on F218's figure
of "11 in-fork `*mut SDqLayer` parameters, and the other thirty-one convert under
ordinary single-threaded rules". **F221 measures that with the tool's own mode:**

    $ python3 rust/tools/phase9_forksplit.py --type SDqLayer --list | tail -3
    **total**                              38             2
    40 bodies carry a `*mut SDqLayer` parameter.

**38 in-fork, 2 ST-flippable.** F218's 11 is the count of bodies taking *both* a
`*mut sWelsEncCtx` and a `*mut SDqLayer` (reproducible as 12); 26 of the 38 take
no context pointer at all. There is no "thirty-one that convert under ordinary
rules" — there are two. S3 therefore did stage B first, and within it put the two
files with **zero** fork-reachable bodies ahead of the one MT-seam checkpoint.
That decision was vindicated narrowly: B3 and B2 landed with no aliasing defect,
and both of the session's defects landed in B1.

**What this means for you.** D2 is 38 fork-reachable signatures at once, and it is
the one checkpoint in the plan that cannot be de-risked by ordering — the layer is
where the fork *writes* (F210), so the diagonal F216's protocol step 1 warns about
is real here, unlike every stage-A accessor. Classify before converting: for each
of the 38, establish whether the worker writes only its own slice's rows
(per-slice-disjoint → D1's `SharedCells` precedent applies field by field) or
whether it stays raw-tagged until D3. **Do not convert first and classify after.**

### The cascade, and what it is actually worth

F216 records stage A's cascade sitting in escrow behind the DQ-layer family, and
S3 did not collect it — the tracking number moved 612 → 613. The family is
measured fresh at S3's close (`grep -rn '\b<name>(' src`):

| route | sites | | route | sites |
|---|---:|---|---|---:|
| `current_layer` | 142 | | `layer_ref_pic` | 31 |
| `layer_rec_view` | 25 | | `layer_pps` | 23 |
| `layer_enc_pic` | 22 | | `ctx_dq_layer` | 15 |
| `set_current_layer` | 5 | | `layer_ref_feature_storage` | 5 |
| `layer_sps` / `layer_subset_sps` | 5 / 2 | | `ctx_ref_pic` / `ctx_pic_ref` | 4 / 2 |

**Note the spelling.** These are `name(` counts. F216 and plan §5 quote the *bare*
name (`current_layer` 158, `ctx_dq_layer` 22, `layer_pps` 30, `layer_ref_pic` 45)
— both are right under their own grep and the two are used interchangeably across
three documents. Say which you mean.

Two facts that size D2 smaller than the 38 suggests, both verified by reading the
code at S3's close:

1. **The storage is already owned.** `sWelsEncCtx::ppDqLayerList` is a
   `Vec<Option<Box<SDqLayer>>>` (`encoder_context.rs:1650` at S3's close — read the
   field, do not trust the line number), and
   `current_layer` resolves a *position* (`iCurDqLayer: Option<LayerIdx>`, the
   identity-moved design) rather than holding an alias. So D2 is an
   accessor-and-parameter job, not a storage migration.
2. **`ctx_dq_layer` already reads the slot as a value** and says so in its own
   comment: "The value read carries the heap block's own provenance — nothing here
   retags the layer, which is what lets two workers resolve it at once." The
   layer is a **separate heap allocation** from the context. That is the property
   B1's family depends on too, and it is why a reader for the layer is reachable
   at all.

## The protocol, compressed — and the two rules under it

* **F208** — a `&self` accessor is a `SharedReadOnly` retag over the **whole**
  context. It may not be called while a `&mut`-shaped derivation into that same
  context is live. Borrowck referees it where the context is a reference; where it
  is a raw, **only Miri does**. (B3 converted `UpdateStatistics`, F208's own body,
  so borrowck referees it now and the scanner's candidate list is empty.)
* **F215** — a `&mut self` accessor is a **fresh whole-struct `Unique` per call**.
  Two raw cursors may only coexist if they come off the *same* call. **F223 adds
  the third rule: in-fork, a `&mut` retag is a write.** N workers taking one is
  S63 whether or not anything is read through it.

1. **Classify before converting** (protocol step 1, and D2 is where it earns its
   keep): tabulate every body by the form of its context/layer parameter and
   whether it *writes* through the answer. Stage A's accessors came back with no
   diagonal; **the layer will not.**
2. **Convert naively, then let the compiler enumerate** (F196). B2 opened at 4
   errors and every one was a real conflict the raw root had hidden.
3. **Resolve in §4.6's order.** Reorder first — read `Copy` scalars out above the
   writer's `&mut`; that was the remedy at most of S3's sites and it is
   behaviour-preserving by construction. Then a combined accessor for two
   *different* fields genuinely wanted at once. Copy-out/write-back last.
4. **Narrow the callee before you convert its argument** (F192/S54).
5. **Gate at `session`, not `family`, and run the MT probes** — see F223.
6. **Then**: `f208_reader_retag_scan.py` (add new accessors to its `READERS`
   list), `safeplan_prohibitions.py <writer>…` (the fifteen are listed in F222),
   `safeplan_prohibition2.py`, `unsafe_ratchet.sh generate`, commit.

**Instrument caveats, both found in S3:** `phase9_ctx_join.py` over-reports on
converted files (text scan, cannot see two arms of one `if`); read its LIVE rows.
`safeplan_prohibitions.py` **does not skip comment lines** and exits non-zero on a
match, so a writer's name in prose inside an in-fork body is a red gate on a
comment — F222, and the fix is one line copied from prohibition 2.

## Ground rules, unchanged

- **Bit-exactness is stop-the-line.** A diffharness SHA divergence is a bug in
  your change. A test failure that does not reproduce follows the F3 adjudication
  protocol in `phase0_findings.md` (5/5 re-run; a second hit escalates to
  head-vs-control alternation; never shrug, never bisect a phantom).
- No edits while a gate runs; one gate at a time; blockers become findings.
- A tag comes off only with the `unsafe` it annotates — never early, never stale.
  (B3 corrected two comments that claimed "Miri is the only referee" after the
  conversion made borrowck the referee; stale prose is a defect like any other.)
- **Every count you quote carries the command that produced it.** S3 found the
  fourth instance of this rule being broken in four sessions (F221), in the very
  finding that diagnosed the third.
- **The sweep corpus has holes.** `sweep.sh`'s `SLICES` is
  `("1 2" "1 4" "2 3" "3 1500" "3 600")` — slicemode 0 under threads had never
  been swept until S3 added it by hand for F217's probe. If your change's
  behaviour depends on a configuration, check the corpus actually contains it
  before treating 583/583 as coverage.

## What to report back

Plain prose: per-checkpoint commits with gate verdicts; the classification table
for the 38 in-fork layer bodies (per-slice-disjoint / stays raw / ST) before any
conversion; D1's bench numbers before and after and stage D's 3% check; the join
and forksplit headlines before and after; both prohibitions plus the F208 scan at
the close; **the MT probes' verdict at every landing that touches the seam, not
just at the close**; the tracking number's movement with F224 read first; every
place this brief was wrong, quoting the sentence; and the hand-off to S5 if stage
E does not fit.
