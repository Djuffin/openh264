# Safe-conversion plan — Session S6: the layer flip, the no-seam middle, then toward the close

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
(`src/api/`), with three recorded exceptions: the two audited `unsafe impl` lines
that make the multi-threaded seam possible (`Sync` for the reconstruction view,
`Send` for the slice job handle), and the screen-content feature family, which the
user has **descoped** (decision D-scope-5: its storage is structurally unreachable —
the allocator was deliberately deleted — but its search path is public-API-reachable,
so it is neither converted nor deleted; it stays raw and tagged
`SCREEN_CONTENT(dormant)`; do not touch it).

Progress is one number: `#[allow(unsafe_code)]` attributes outside `src/api/`.
`bash rust/tools/safeplan_tracking.sh` prints it — 460 at the time of writing.

## The one architectural fact to hold at all times

The encoder has two worlds:

* **Single-threaded code** — init, teardown, the frame loop between frames — holds
  the context as `&mut sWelsEncCtx` and calls `&mut self` methods freely.
* **The fork** — per-slice worker threads spawned inside a frame — holds the context
  as `*mut sWelsEncCtx`. N workers can never each hold `&mut` into one allocation,
  and under Miri's aliasing model **merely creating a `&mut` to shared data inside
  the fork is a data race**, even if nothing is written through it. A shared `&`
  borrow of a struct is also a claim over the *whole* struct: it races any
  concurrent write to any byte inside that struct, so a struct can only be passed
  around as `&T` once every field that worker threads write has been moved out of
  it (into atomics, or into separately-allocated storage reached through it).

The practical consequences: when you narrow a raw parameter on a path worker
threads can reach, **`&` is almost always the answer**, even where `&mut` would
compile — and before giving any fork-shared struct a `&T` life, verify nothing
inside it is still written concurrently. To learn whether a function is
fork-reachable, run `python3 rust/tools/phase9_forksplit.py --list` from the crate
root — it classifies every body that holds a raw context pointer.

## Verification, sized to fit

Hour-scale runs are struck by the user's direction: the two full-encode threaded
Miri tests (`fork_join_encodes_*`, ~59 minutes as a pair) do **not** run in this
session — they wait for the exit battery. Small Miri runs of unit tests are the
ceiling. The regime:

* **Per checkpoint** (one checkpoint = one gated commit):
  `bash rust/tools/gates.sh commit` — unit tests in both profiles plus the
  unsafe-counter ratchet (~15–19 min). For changes on the live encode path use
  `bash rust/tools/gates.sh family` instead — it adds the differential sweep, every
  harness config in both profiles byte-compared against the C++.
* **A byte gate cannot see an aliasing bug that changes no byte.** Any checkpoint
  that converts a raw pointer into a reference or slice must also run the Miri
  lane: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (~20 min, capped).
  This has caught two real defects the sweep certified as byte-identical.
* **For changes to data the worker threads share**, write a **small targeted Miri
  test**: build the data structure by hand *without an encoder*, spawn two threads
  doing exactly what the workers do, and let Miri judge. Three finished examples
  to copy sit in `svc_encode_slice.rs`:
  `update_mb_map_forked_workers_share_the_layer_without_racing`,
  `partition_counters_take_a_shared_layer_borrow_across_the_forked_writes`,
  `slice_banks_take_a_shared_layer_borrow_across_the_forked_writes` — each ~1.3 s
  under Miri. **Two hard rules for any probe you add**: (1) run its *control* — a
  deliberately-broken variant — and see it fail before you trust a pass; (2) use
  enough iterations that the control actually fails: in this tree's own history a
  probe was silent at 8 rounds and solidly red at 200. A probe whose control was
  never seen red is a blind instrument that reads as a passing test.
* **Run all tools from the crate root** (`rust/crates/openh264-rs`). They glob
  `src/**/*.rs`; from the repo root they match nothing and print a false zero.
* One gate at a time; no edits while a gate runs. A test failure that does not
  reproduce is re-run five times before any conclusion — never shrugged off, never
  bisected on a single flake.
* Every remaining `unsafe` in the tree carries a `// unsafe-cat: …` tag. A tag
  comes off only together with the unsafe it annotates; new untagged unsafe fails
  the census gate.

## What S6 does, in order (drop-from-the-end: if you must stop, stop at a commit
boundary and name everything not done)

### First: the `&SDqLayer` flip

The DQ layer (`SDqLayer`) is the per-spatial-layer struct the fork's workers share.
Its two obstacles to being passed as `&SDqLayer` were removed last session, each
verified by a targeted probe: the per-partition counters
(`NumSliceCodedOfPartition`, `LastCodedMbIdxOfPartition`) are now atomics, and the
slice-buffer banks are boxed off the struct's own bytes. **Zero fields of
`SDqLayer` are written from inside the fork any more** — measured, not asserted.
The flip itself was then attempted, and deliberately reverted at the end of that
session because the last steps needed hand care, not because anything is unknown.
The recipe is fully worked out; apply it from this clean tree.

**The hazard that makes a naive flip wrong even though it compiles**:
`current_layer` returns `*mut SDqLayer` and answers **null** whenever the current
layer is unset (`iCurDqLayer` is `None`). Today fourteen callee bodies open with
`if pCurLayer.is_null() { return .. }`. Flip a parameter to `&SDqLayer` and that
obligation silently moves to the call sites, which become
`&*current_layer(pEncCtx)` — a null dereference the moment the layer is unset.
Every test passes; the defect waits on the unset-layer path.

**The split, measured over the 41 bodies taking `*mut SDqLayer`** (re-verify each
group membership by reading the body — a regex cannot do this classification:
a writes-detector built on patterns missed `..sSliceBufferInfo[i].iMaxSliceNum = ..`
because of the field path after the index):

1. **4 writers — stay raw**: `UpdateMbListNeighborParallel`,
   `WelsSliceHeaderWrite`, `WelsSliceHeaderExtWrite`, `ReallocateSliceList`.
2. **10 readers whose null guard is load-bearing — take `Option<&SDqLayer>`**,
   the guard rewritten as `let Some(x) = x else { <the guard's own return> };`:
   `slice_in_layer`, `WelsMbToSliceIdc`, `UpdateMbNeighbor`,
   `UpdateMbNeighbourInfoForNextSlice`, `WelsSliceHeaderScalExtInit`,
   `WelsSliceHeaderExtInit`, `OutputPMbWithoutConstructCsRsNoCopy`,
   `WelsGetNextMbOfSlice`, `SetSliceBoundaryInfo`, `WelsMdI16x16`. Four of these
   guard on a *compound* condition (`pCurLayer.is_null() || kiSliceIdx < 0`, and
   the like) — split those by hand, never by pattern.
3. **27 readers with no guard — take `&SDqLayer` directly.** They already
   dereference unconditionally, so forming `&*ptr` at their call sites assumes
   exactly what the existing dereference assumed, and no more.

**Method**: writers first, then the `Option` group, then the straight group — each
group applied deliberately, never a broad regex pass over a half-converted tree.
The ~125 call-site rewrites drive mechanically off rustc's error spans; the prior
attempt converged 296 → 39 errors in one pass before it was stopped at the ten
hand edits.

**Why this checkpoint pays beyond its own sites**: `current_layer` is the single
largest blocking edge in the whole remaining graph. `ref_list_mgr_svc.rs` alone
has 31 allows, *every one* on a signature with no raw pointer — blocked purely by
body calls into raw accessors, 17 of them `current_layer`. After the flip lands,
rerun the de-unsafe cascade (below) and expect it to travel far.

### Then: the no-seam middle — the volume is here

A census of every remaining allow, classified by what actually blocks it, says
only ~30% of what is left sits behind the two fork-shared pointers (the context
and, until you fix it, the layer). The rest is reachable **without touching the
fork seam**. The files with real convertible surface and zero context dependency,
in descending order (counts from the census — re-measure):

| file | allows | note |
|---|---:|---|
| `encoder_ext.rs` | 32 | 16 raw-signature, 16 body-blocked |
| `ref_list_mgr_svc.rs` | 31 | all body-blocked; mostly unlocked by the layer flip |
| `wels_preprocess.rs` | 26 | see the move-memory caveat below |
| `wels_encoder_ext.rs` | 24 | 17 raw-signature |
| `svc_motion_estimate.rs` | 22 | the screen-content sites among them are descoped — skip those |
| `svc_enc_slice_segment.rs` | 21 | |
| `encode_mb_aux.rs` | 12 | DCT kernels (pixel/coefficient pointers), **not** bit writing |
| `nal_encap.rs` | 11 | first bite ready-made, below |

A ready-made first conversion in `nal_encap.rs`: `WelsLoadNal` / `WelsUnloadNal`
→ `&mut SWelsEncoderOutput`. Already analysed: every call site already passes a
reference. It was skipped last session only to avoid one more gate run.

**The move-memory caveat** (`wels_preprocess.rs`): `WelsMoveMemory_c` /
`WelsMoveMemoryWrapper` are blocked — their source can be the caller's own C-ABI
`SSourcePicture` buffer (raw by ABI), and the pair must tolerate source and
destination in the same picture. Price that pair when you reach it; convert around
it if the price is high, and record what you found.

**The de-unsafe cascade, after each batch lands**: "which functions are unsafe
only because their callees were?" is answered by the compiler, never a regex —
strip `unsafe` on a scratch copy and read the errors. **The converging form
strips only `unsafe fn` declarations whose signature carries no raw pointer**;
stripping declarations and blocks together makes the error set oscillate and the
pass does not terminate. Run the converging form to a fixpoint, then revert
whatever failed. This pass has retired as many allows as all hand conversion
combined.

**A cheap sweep to fold in**: ~55 `# Safety` doc clauses describe pointer
preconditions their functions no longer have (the parameters are references now).
Sweep each file's stale clauses with the conversion that touches it.

### Then: the slice core's remainder and the residue files

* **`svc_encode_slice.rs`** — the biggest file. Its ~72 allows split ~38 blocked
  by the fork's context pointer and ~15 by the layer; the layer share unlocks
  with the flip above. Convert what unlocks; the context share stays (next item).
* **`slice_multi_threading.rs` residue** (~25 sites) plus `encoder/deblocking.rs`,
  `mc.rs`, `copy_mb.rs`.
* **Decoder residue**: `decoder/picture.rs` is largely retired — re-measure; the
  trace logger's raw context (`common/wels_trace.rs`) gets a newtype whose
  construction and invocation live only in `src/api/`, so `common/` can seal.

### The context question — classify, don't improvise

~114 remaining allows sit behind `*mut sWelsEncCtx` in fork-reachable bodies. The
end state needs them gone, and the method is the one the layer just proved twice:
**tabulate every fork-reachable body by what it writes through the context, move
each worker-written field to atomics / `Cell`s / separately-allocated storage
(each move with its own targeted two-thread probe, control seen red), and only
then flip bodies to `&sWelsEncCtx`.** Do not start this as a side effect of
another checkpoint. If you reach it, open it as its own checkpoint with the
tabulation as the first commit's content; it is likely a whole session by itself,
and stopping before it is an honest stop.

### The close, when everything above is done

* **Seal as you go**: `#![forbid(unsafe_code)]` on each file as its last allow
  falls (42 files are sealed today; seal **leaf files only** — a `forbid` in a
  `mod.rs` seals the whole subtree and will not compile until everything under it
  is done).
* **The final flip**: delete `src/lib.rs`'s crate-wide `allow(unused_unsafe,
  unsafe_op_in_unsafe_fn, …)`; reduce the census allowlist to the api island, the
  two `unsafe impl` lines, and the screen-content family's enumerated sites
  (the descope ruling); pin the ratchet at the floor.
* **The exit battery** — `bash rust/tools/gates.sh exit`: ABI export list, dlopen
  harness, upstream gtest, full Miri including the differential tests **and the
  two full-encode fork probes deferred all along**, benches against
  `rust/docs/perf_baseline.md` (six recent checkpoints were never benched — the
  exit bench is where that debt clears), both-profile sweeps.

## Aliasing rules that will bite you (each learned the hard way in this tree)

* **Two raw cursors must come off ONE derivation.** If a converted function takes
  `&mut [u16]` and still needs raw cursors inside, call `as_mut_ptr()` **once**
  and derive every cursor from that pointer. A second `as_mut_ptr()` call is a
  fresh whole-slice retag that invalidates the first cursor. The byte gate passed
  exactly this defect; only the Miri lane caught it.
* **In the fork, creating a `&mut` is a write** — and a `&self` accessor on the
  context is a shared claim over the *whole* context, which races any worker's
  concurrent write to any field inside it. Derive precisely what you need
  (field-precise accessors), and prefer `&` everywhere a fork-reachable body only
  reads.
* **Miri's wording names the fix.** In a race report, "non-atomic write" means
  the field wants atomics; "retag write" means the storage wants its own
  allocation, boxed off the shared struct. They are different fixes and the
  message tells you which one you owe.
* **Ask the compiler, not a regex** — for writer classification, for the
  de-unsafe cascade, for call-site enumeration. Every regex shortcut tried in
  this project has lied at least once (a writes-detector missed an assignment
  behind an index expression; a census classifier counted `&mut sWelsEncCtx` — a
  reference — as a raw pointer and overstated a category by 2×).
* **The prohibition checker counts comments.**
  `python3 rust/tools/safeplan_prohibitions.py` verifies no fork-reachable body
  calls a `&mut self` context accessor — but it flags an accessor's *name in a
  comment* as a violation. A red result on a comment line is noise, not a defect.

## Findings and the report

The project keeps a findings ledger, `rust/docs/phase9_findings.md` — an appended,
numbered (`F…`) record of everything surprising: defects found, claims in a brief
that turned out wrong, measurements that contradict a document. Yours start at
**F237**. When a blocker needs the user's ruling, write the finding and stop that
checkpoint rather than guessing. At the close, add the session's row to **both**
tables in `rust/docs/safe_conversion_execution_plan.md` — the session map *and*
the dated log table (the previous session updated one and missed the other).

Report back in plain prose: per-checkpoint commits with gate verdicts; the
targeted probes you added, each named with its runtime and its control seen red;
the tracking number's movement; every place this brief was wrong, quoting the
sentence; and a roll-forward line naming **everything** owed — checkpoints,
benches, findings alike. A previous hand-off silently dropped a whole stage from
that line and it cost real work; name what you are handing on.
