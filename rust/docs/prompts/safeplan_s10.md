# Safe-conversion plan — Session S10: the source-plane referee, the fork seam, and the rest of the middle

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim about the code, read the
lines it describes; a claim that something is absent gets its own grep. Every count
you produce excludes comments or says that it doesn't — naive greps here count the
tree's own documentation. And before honoring any recorded deferral or refusal,
re-verify its **premise** against today's battery: a deferral's premise expires,
not just its conclusion, and one stale premise recently traveled through three
documents before being caught.*

## The project, in one paragraph

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 video
codec (the C++ reference sits at the repo root under `codec/`). It ships as a
drop-in `libopenh264` replacement, so it must stay **byte-identical** to the C++
on every stream the test harness runs — bit-exactness is stop-the-line. All
undefined behavior is gone; what remains is *conversion* — raw pointers and
`unsafe` functions that are sound but unnecessary, replaced with safe Rust. The
end state is `#![forbid(unsafe_code)]` in every file outside the C-ABI layer
(`src/api/`), plus exactly two audited `unsafe impl` lines for the multi-threaded
seam (`Sync` for the reconstruction view, `Send` for the slice job handle).

Progress is one number: `#[allow(unsafe_code)]` outside `src/api/` —
`bash rust/tools/safeplan_tracking.sh` prints it (and names its basis; it
excludes comment mentions). **371** at the time of writing. 47 files are sealed;
`common/`, `safe/`, `processing/`'s neighbors and the decoder are essentially
done — the remainder is the encoder's mode-decision/motion land and the slice
core.

## The architecture, current (read this — it holds three hard-won facts)

* **The context and the DQ layer already pass as references** everywhere, the
  fork included (a handful of audited raw survivors: five `ctx_*_raw` slot
  readers, the job handle's `*const` field, nine layer-writer bodies).
* **The source picture is written inside the fork.** Background detection
  (`bEnableBackgroundDetection`, **true by default**) copies luma into the source
  picture from worker threads. Therefore a read view over a source plane must
  ride the **shared seam** (`SharedPlane`, built last session on the
  reconstruction view's model) — never a bare `&[u8]` over the plane, which is a
  whole-allocation shared claim racing those writes. This exact mistake was
  committed and corrected within an hour last session; the sweeps passed it
  because a race need not move a byte, and the Miri lane passed it because Miri
  runs `--lib` tests, never the sweeps.
* **A field-precise `&mut`/`addr_of_mut!` derivation held across a later
  whole-struct reborrow gets popped** — single-threaded, sweep-invisible. Fix by
  deriving at the use. `rust/tools/f239_span_scan.py` detects the three-step
  shape; run it after every conversion batch.
* Never form a `&mut` to anything context-, layer-, or source-picture-reachable
  inside the fork — creating one is a race in Miri's model even unwritten.

## Verification, sized to fit

Hour-scale runs are struck by the user's direction: the two full-encode threaded
Miri probes (~59 min as a pair) wait for the exit battery. The regime:

* Per checkpoint: `bash rust/tools/gates.sh commit` (~15–19 min), or `family`
  for anything on the live encode path (adds the differential sweep, both
  profiles, byte-compared).
* Any checkpoint converting a raw pointer into a reference or slice also runs
  the Miri lane: `MIRI_SCOPE=encoder bash rust/tools/gates.sh session` (~20 min).
  Four such defects have now been caught that the sweeps certified.
* Changes to worker-shared data get a **targeted two-thread Miri probe**,
  hand-built without an encoder, its *control* (a deliberately broken variant)
  seen red at an iteration count where it actually fails (8 rounds was blind,
  200 was a referee). Four examples in `svc_encode_slice.rs` and
  `svc_motion_estimate.rs`.
* After every conversion batch: `python3 rust/tools/deunsafe_cascade.py` (the
  converging de-unsafe pass, now a committed tool), then
  `python3 rust/tools/f239_span_scan.py`, then seal any file whose last allow
  fell (leaf files only — `forbid` in a `mod.rs` seals the subtree).
* Run tools from the crate root (`rust/crates/openh264-rs`); quote every checker
  result with its denominator. After feeding any baseline or config file,
  verify the reader **consumed** it — the Miri baseline was appended to for two
  sessions while the reader took newest-first, and twelve gates compared
  against a stale number. An instrument's output is not evidence its input was
  read.
* One gate at a time; no edits while a gate runs; non-reproducing failures
  re-run five times. Tags (`// unsafe-cat:`) come off only with their unsafe.
* At the close: advance `rust/tools/miri_wall_baseline.txt` by **prepending**
  the new data line (newest-first), and regenerate the ratchet baseline
  downward (`bash rust/tools/unsafe_ratchet.sh generate`).

## The steps

Ordered; one gated commit each; drop-from-the-end — if you must stop, stop at a
commit boundary and name everything not done. Counts measured at 371; re-measure.

### Step 1 — the source-plane race referee (instrument; the session's keystone)

Nothing in the battery can currently fail on a source-plane aliasing mistake:
the `bg` sweep is a byte referee and a race need not move a byte; the Miri lane
never runs the sweeps' configuration. Build the probe finding **F254** specifies:
two threads over one hand-built source plane — one writing as
`VaaBackgroundMbDataUpdate` does (the in-fork luma copy), one reading as the
mode-decision path does — on the model of the three probes in
`svc_encode_slice.rs`. Control seen red at a calibrated iteration count. Name it
under `encoder::` so the Miri lane runs it from then on. This unblocks step 2
and referees step 3's most dangerous edge.

### Step 2 — the 22 slice-cursor sites migrate to the shared seam

The remaining bare-`&[u8]` plane views over the fork-written source picture:
`svc_mode_decision.rs` **10**, `svc_base_layer_md.rs` **11**, `md.rs` **1** (the
enumeration is F254's; re-measure). Each moves onto `SharedPlane` views, the
shape S9.0b established. Byte-precise, per-worker-disjoint semantics preserved.
Gate: `session` + span scan. With the step-1 probe in the lane, a regression
here is caught at session cadence forever.

### Step 3 — the fork seam for slices, then step 7's remainder (~90 allows)

The largest single block left, and **design work — do it fresh, not last** (the
previous session stopped exactly here rather than half-build a seam tired, and
that was right). The facts, from F256:

* `slice_bank_root` hands out a raw pointer **by design**: every worker resolves
  bank 0, and a `&mut Vec<SSlice>` would be a retag the sibling workers' claims
  forbid.
* The `SSlice` fields workers write therefore need the **shared-cell treatment**
  on `RecPicView`'s model (the pattern decision D-mt-3 blessed): a view type
  over the bank whose worker-written fields are `Cell`/atomic behind the audited
  seam, handed per-worker, with the write set enumerated by the compiler — not
  by grep.
* **Partial conversion inside a body retires nothing** — an allow leaves only
  when the body's *last* raw operation does. Sequence to finish bodies, not to
  touch many. A lever built early last session "paid nothing" for exactly this
  reason until its file's other raw operations went.

Method: enumerate the worker-written `SSlice` field set with the compiler;
build the seam view; move the write sites; **one targeted probe per field
family, control red**; then convert the ~90 remaining bodies in
`svc_encode_slice.rs` (67 allows at last count) and `slice_multi_threading.rs`
(23), running the cascade after each batch. Gate: `session` + span scan every
commit; this is the change class that produced the plan's worst defects.

### Step 4 — the mode-decision and motion-estimate land (S8's steps 4a/4b)

* **4a — parameter roots**: `md.rs` and `svc_motion_estimate.rs` plane-root
  parameters onto slices/cursors (post-step-2, their source-plane reads ride
  the seam). Whole-allocation slices, never row-bounded; two cursors off one
  derivation.
* **4b — the body campaign in the MD pair**: `svc_mode_decision.rs` (~33) and
  `svc_base_layer_md.rs` (~18) — run the cascade first; convert by hand only
  what it reports blocked on its own body.

Gate: `session` + span scan.

### Step 5 — the body campaign in the big single-threaded files (S8's 6a/6b)

* **5a — camera path**: `ref_list_mgr_svc.rs` (~27) and `rc.rs` (~22); `family`
  gate minimum; the five-times rule for any flake.
* **5b — frame loop**: `encoder_ext.rs` (~32), `wels_encoder_ext.rs` (~24),
  `encoder_context.rs` (~24 — includes the two flippable slot readers,
  `ctx_src_pool_raw` and `ctx_vpp_raw`; the three in-fork ones stay).

### Step 6 — preprocess and processing (S8's step 8)

`wels_preprocess.rs` (~17, including the move-memory pair: price it — source
can be the caller's C-ABI `SSourcePicture`, source and destination may be the
same picture; an audited allow at the ABI edge is acceptable if the safe form
costs more) and the four `processing/` files (~12). Gate: `family`.

### Step 7 — E2, the final flip (only when steps 1–6 are done)

Delete `src/lib.rs`'s crate-wide `allow(unused_unsafe, unsafe_op_in_unsafe_fn,
…)`; reduce the census allowlist to the api island plus the two `unsafe impl`
lines; regenerate and **pin** the ratchet at the floor; prove the seal enforces
by injecting a violation and watching it rejected. Gate: `session` +
`cargo check --all-targets`.

### Step 8 — E3, the exit battery

`bash rust/tools/gates.sh exit`: ABI export list, dlopen harness, upstream
gtest ratchet, full Miri including the differential tests **and the two
full-encode fork probes** (`rust/tools/fork_join_probe.sh`, ~59 min pair,
tripwire vs its baseline), both-profile sweeps, and **the benches — twenty-six
checkpoints of debt, the largest single risk in the plan**. Same machine both
sides, two after-runs; a real regression bisects over the per-checkpoint
commits. Budget real time for whatever the battery surfaces.

## Working rules (each earned here)

* **A deferral's premise expires.** Before honoring any recorded "cannot be
  tested / has no referee / must stay raw", re-verify the premise against
  today's battery — presets and probes have been built since many were written.
* **Some "conversions" are deletions.** Three of the last session's items were
  dead code the tree had already recorded as dead — check callers before
  designing a conversion.
* **An allow retires with its body's last raw operation** — plan work to finish
  bodies.
* **Measured vs inferred**: label every tabulation row; promote inferred to
  measured before executing on it.
* **Ask the compiler, not a regex** — writer sets, cascade, call sites.
* **Read the comments at the site** — they carry rulings, not just history.

## Findings and the report

Findings: `rust/docs/phase9_findings.md`, appended, numbered; yours start at
**F258**. A blocker needing the user's ruling becomes a finding and stops that
checkpoint. At the close: both tables in
`rust/docs/safe_conversion_execution_plan.md` (session map *and* dated log),
the Miri baseline **prepended**, the ratchet regenerated downward.

Report in plain prose: per-checkpoint commits with gate verdicts; every probe
with its control seen red; the span scan and cascade results per batch; the
tracking number's movement on the corrected basis; every place this brief was
wrong, quoting the sentence; and a roll-forward line naming everything owed —
checkpoints, instruments, benches, findings alike.
