# Safe-conversion plan — Session S5: finish stage C, then the writers and the slice core

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim about the code, read the
lines it describes; a claim that something is absent gets its own grep before you
build on it.*

## The project, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 video
codec (the C++ reference sits at the repo root under `codec/`). It ships as a
drop-in `libopenh264` replacement, so it must stay **byte-identical** to the C++ on
every stream the test harness runs — bit-exactness is stop-the-line: if a
differential sweep reports one divergent SHA after your change, the change is wrong,
no matter how much safer it looks. All undefined behavior has already been
eliminated from the port; what remains is *conversion* — raw pointers and `unsafe`
functions that are sound but unnecessary, replaced stage by stage with safe Rust.
The end state is `#![forbid(unsafe_code)]` in every file outside the C-ABI layer
(`src/api/`), plus exactly two audited `unsafe impl` lines that make the
multi-threaded seam possible (`Sync` for the reconstruction view in `rec_view.rs`,
`Send` for the slice job handle in `slice_multi_threading.rs`).

Progress is one number: `#[allow(unsafe_code)]` attributes outside `src/api/`.
`bash rust/tools/safeplan_tracking.sh` prints it — 519 at the time of writing.

## The one architectural fact to hold at all times

The encoder has two worlds:

* **Single-threaded code** — init, teardown, the frame loop between frames — holds
  the context as `&mut sWelsEncCtx` and calls `&mut self` methods freely.
* **The fork** — per-slice worker threads spawned inside a frame — holds the context
  as `*mut sWelsEncCtx`. N workers can never each hold `&mut` into one allocation,
  and under Miri's aliasing model **merely creating a `&mut` to shared data inside
  the fork is a data race**, even if nothing is written through it. Fork-reachable
  code therefore takes shared `&` views (which coexist freely across threads) or
  keeps the raw pointer; the fields workers genuinely write are atomics, `Cell`s,
  or provably disjoint per-worker ranges.

The practical consequence for this session: when you narrow a raw parameter on a
path worker threads can reach, **`&` is almost always the answer**, even where
`&mut` would compile. To learn whether a function is fork-reachable, run
`python3 rust/tools/phase9_forksplit.py --list` from the crate root — it classifies
every body that holds a raw context pointer.

## Verification, sized to fit

Hour-scale runs are struck by the user's direction: the two full-encode threaded
Miri tests (`fork_join_encodes_*`, ~59 minutes as a pair) do **not** run in this
session. Small Miri runs of unit tests are the ceiling. The regime:

* **Per checkpoint** (one checkpoint = one gated commit):
  `bash rust/tools/gates.sh commit` — unit tests in both profiles plus the
  unsafe-counter ratchet (~15–19 min). For changes on the live encode path use
  `bash rust/tools/gates.sh family` instead — it adds the differential sweep, every
  harness config in both profiles byte-compared against the C++.
* **A byte gate cannot see an aliasing bug that changes no byte.** Any checkpoint
  that converts a raw pointer into a reference or slice must also run the Miri
  lane: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (~20 min, capped).
  This has already caught a real defect the sweep certified as byte-identical.
* **For changes to data the worker threads share**, write a **small targeted Miri
  test**: build the data structure by hand *without an encoder*, spawn two threads
  doing exactly what the workers do, and let Miri judge. A finished example to copy
  is `update_mb_map_forked_workers_share_the_layer_without_racing` in
  `svc_encode_slice.rs` — ~1 second under Miri, where the equivalent full-encode
  test costs hours. Write one such probe per seam you move, named under `encoder::`
  so the Miri lane picks it up automatically from then on.
* **Run all tools from the crate root** (`rust/crates/openh264-rs`). They glob
  `src/**/*.rs`; from the repo root they match nothing and print a false zero.
* One gate at a time; no edits while a gate runs. A test failure that does not
  reproduce is re-run five times before any conclusion — never shrugged off, never
  bisected on a single flake.
* Every remaining `unsafe` in the tree carries a `// unsafe-cat: …` tag. A tag
  comes off only together with the unsafe it annotates; new untagged unsafe fails
  the census gate.

## What S5 does

**C4b, C5, C6 close stage C (pixel-land parameters); then stage D (bit writers and
the slice core); then stage E (residue and the lint flip).** Every item is a
campaign; none is a tail to rush at the end of another checkpoint. Land each as its
own gated commit; if the session must stop, stop at a commit boundary and name
everything not done.

### C4b — the MVD-cost family (start here; it has the perf question)

The motion-vector-difference cost table is the innermost loop of the encoder — 80
mentions across seven files, 25 call sites. `pMvdCost` is a **three-level biased
cursor**, and the levels compose:

```
(*pMd).pMvdCost   = pMvdCostTable.add(luma_qp * stride)         // QP row base
pFeatureSearchIn.pMvdCostX = sMe.pMvdCost.offset(-iCurPixXQpel - sMvp.iMvX)
pMvdCost          = pMvdTable.offset(iMinMv * 4 - sMvp.iMvY)    // then .add(4) per step
```

`COST_MVD` indexes the result with a **signed** motion-vector difference, so every
level is a pointer deliberately parked in the *middle* of its table — a plain slice
cannot represent it without carrying the bias separately.

The obstacle: `SWelsME` is `repr(C)` + `Copy` and passed by value, so it cannot
hold a slice without a lifetime that cascades into `SWelsMD` and the context. The
measured fact that decides the alternative: **none of the motion-search functions
take the context** — `WelsMotionEstimateSearchStatic`,
`WelsMotionEstimateInitialPoint`, `WelsDiamondSearch`, `CheckDirectionalMv` all
take `pMeFuncs`, `sdf`, `pMe`, `pSlice` and the two pixel planes. So the index
representation (a `usize` base into the context's table, bias carried beside it)
means threading `&[u16]` through the whole motion-search tree. Price both designs
before choosing, and say in the report which you took and why.

**Bench before and after, inside the session** — this is the hottest loop in the
encoder. Absolute numbers in `rust/docs/perf_baseline.md` are not comparable across
machines; measure the same command on the same machine on either side of your own
change, and take **two** after-runs — a first run can show a phantom swing (a +3.0%
reading on 1080p that became −1.0% on the second run).

### C5 — screen-content feature arenas

`SScreenBlockFeatureStorage` (in `encoder/picture.rs`) holds `*mut *mut u16`
feature tables; convert them to `Vec` + index tables, along with their mirrors in
the motion-estimate code. **This is dark code**: the screen-content path installs
only under `bScreenContent && bEnableSceneChangeDetect && iComplexityMode < HIGH`,
and no harness driver ever sets `bScreenContent` — instrumentation confirmed the
judging arm executes in zero sweep rows. The byte sweep therefore proves nothing
about a semantic change here: convert **containers only**, keep the logic
line-for-line, and lean on the compiler. Review the diff as the only referee it
will get.

### C6 — preprocess views

`wels_preprocess.rs` measures **198 raw dereferences** with `unsafe` stripped, so
it is not a cheap sweep. Its centre is `SPixMap::pPixel: [*mut u8; 3]` — 30 use
sites, 24 in `wels_preprocess.rs` itself and the rest in `processing/` (`vaacalc`,
`background_detection`, `adaptive_quantization`), where it is the one
`from_raw_parts` left in each. Convert `SPixMap` to hold safe plane views and
those files follow.

### Stage D — the writers and the slice core

* **D1 — the bit writers.** `svc_set_mb_syn_cavlc.rs`, `set_mb_syn_cabac.rs`,
  `vlc_encoder.rs`, `nal_encap.rs`, `encode_mb_aux.rs`: move them onto the safe
  bit-writer that already exists in `safe::bits`.
* **D2/D3 — `svc_encode_slice.rs`**, the biggest file (~81 unsafe fns / ~117 raw
  sites), in two halves: slice construction, teardown and per-slice aliases first;
  the encode loop and dynamic slicing second, plus `svc_enc_slice_segment.rs`. The
  DQ-layer parameter conversion belongs to this territory — **read its section
  below before starting**.
* **D4b — the residue.** `slice_multi_threading.rs` residue (~25 sites) plus
  `encoder/deblocking.rs`, `mc.rs`, `copy_mb.rs`. (The decoder half of this
  checkpoint is already done and its file sealed.)

### Stage E — the close

* **E1** — decoder residue: re-measure `decoder/picture.rs` (largely retired
  already) and the stray raws in `decoder_core`, `decoder_context`, `pic_queue`;
  confine the trace logger's raw context to `src/api/`.
* **E2** — the lint flip: `#![forbid(unsafe_code)]` on every non-api file. 41
  files are sealed already; the remainder unlocks exactly as C and D land
  (`svc_motion_estimate.rs` alone still carries 32 allows — re-measure). Then
  delete the `lib.rs` blanket allows and pin the ratchet at the floor. Note:
  `#![forbid]` in a `mod.rs` seals the whole subtree — seal leaf files only.
* **E3** — the exit battery: `bash rust/tools/gates.sh exit` — ABI export list,
  dlopen harness, upstream gtest, full Miri including the differential tests
  **and the two full-encode fork probes deferred from the sessions**, benches,
  both-profile sweeps.

## Aliasing rules that will bite you (each learned the hard way in this tree)

* **Two raw cursors must come off ONE derivation.** If a converted function takes
  `&mut [u16]` and still needs raw cursors inside, call `as_mut_ptr()` **once**
  and derive every cursor from that one pointer. A second `as_mut_ptr()` call is a
  fresh whole-slice retag that invalidates the first cursor. The byte gate passed
  exactly this defect — it changes no byte — and only the Miri lane caught it.
  That is why reference conversions gate at `session`.
* **In the fork, creating a `&mut` is a write** (see the architectural fact
  above). Three recent conversions were refused on this ground and went
  `*mut` → `&` instead. When you narrow a parameter on the shared VAA block or
  the DQ layer, shared is almost always the answer.
* **Ask the compiler, not a regex.** "Which functions are unsafe only because
  their callees are?" — answer it by stripping `unsafe` on a scratch copy and
  reading the errors. A regex over bodies counts `(*pMbCache)` — a dereference of
  a *reference* — and lies. The compiler-driven fixpoint (strip, read the errors,
  revert what fails, iterate) recently retired 87 signatures in one pass; rerun it
  after your conversions land — the de-unsafe cascade is where most of the
  tracking number's movement comes from.
* **The prohibition checker counts comments.**
  `python3 rust/tools/safeplan_prohibitions.py` verifies no fork-reachable body
  calls a `&mut self` context accessor — but it flags an accessor's *name in a
  comment* as a violation. A red result on a comment line is noise, not a defect.
* **The load-balancing fork has no automated coverage.** `UpdateMbMapForked` runs
  only when `bUseLoadBalancing` is on, which every harness driver pins off. If you
  touch anything under it, its referee is the ~1 s Miri probe named in the
  verification section.

## The DQ-layer parameters (inside D2/D3) — read before converting

All 40 function bodies taking `*mut SDqLayer` have been tabulated. **Three write
through the layer; 37 only read.** The three writers are lawful as they stand: one
was a genuine data race (a whole-struct `&mut` taken concurrently by every worker
for one scalar read) and is already fixed; the other two (`ReallocateSliceList`,
`ReallocateSliceInThread`) write each worker's **own** slice-buffer bank — disjoint
per-worker ranges.

**But the 37 read-only bodies still cannot take `&SDqLayer`.** A `&SDqLayer` is a
shared borrow of the *whole* struct, and in size-limited slice mode
(`SM_SIZELIMITED_SLICE`) worker threads concurrently write bytes that live *inline
in the layer*: the slice-buffer bank slots and the per-partition counter arrays
(`NumSliceCodedOfPartition`, `LastCodedMbIdxOfPartition` — `[i32; MAX_THREADS_NUM]`
inline, incremented from inside the encode). A whole-struct shared borrow racing
those writes is undefined behavior under Miri's model, even though the C never
notices.

**So the flip needs the storage moved first** — the banks to their own allocation
outside the struct, the counters to atomics — and only then can the 37 take
`&SDqLayer`. Treat this as a multi-threading-seam checkpoint: the class of change
most likely to produce a defect no byte gate can see, and exactly where the
targeted-probe rule applies — one hand-built two-thread Miri probe per field
family you move, before you rely on it.

## Findings and the report

The project keeps a findings ledger, `rust/docs/phase9_findings.md` — an appended,
numbered (`F…`) record of everything surprising: defects found, claims in a brief
that turned out wrong, measurements that contradict a document. Yours start at
**F228**. When a blocker needs the user's ruling, write the finding and stop that
checkpoint rather than guessing. At the close, add the session's row to the log
table in `rust/docs/safe_conversion_execution_plan.md`.

Report back in plain prose: per-checkpoint commits with gate verdicts; C4b's bench
numbers before and after; the targeted probes you added, each named with its
runtime; the tracking number's movement; every place this brief was wrong, quoting
the sentence; and a roll-forward line naming **everything** owed — checkpoints,
benches, findings alike. A previous hand-off silently dropped a whole stage from
that line and it cost real work; name what you are handing on.
