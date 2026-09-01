# Safe-conversion plan — Session S12: the exit

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim, read the lines it
describes; a claim of absence gets its grep; every count excludes comments or says
it doesn't; before honoring a recorded deferral, re-verify its premise.*

## Where the project stands

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264,
shipping as a drop-in `libopenh264` replacement that must stay **byte-identical**
to the C++ on every harness stream. The conversion campaign is **done**: the
Phase 9 queue (`port-raw(Phase 9)`) emptied at S11.52. The tree today:

* `#[allow(unsafe_code)]` outside `src/api/`: **32** — and it is a *list*, not a
  count: 21 `instrument(test)` probes plus 11 named permanent exceptions, pinned
  by equality in `rust/tools/unsafe_census.sh` (a new allow fails CI naming
  file:line, even if plausibly tagged).
* One `unsafe impl` in the whole tree (`Sync for SharedCells`, rec_view.rs).
* 65 of 86 files sealed `#![forbid(unsafe_code)]`; ratchet pinned at
  425 raw-ptr / 59 unsafe-fn / 136 unsafe-block.
* Differential sweeps 847/847 byte-identical, both profiles, at every gated
  checkpoint on record.

**This session runs what was deferred.** Two standing rulings deferred the heavy
verification to the end: D-gate-9 (no full Miri during conversion — small probes
only) and D-gate-8 as amended (all benchmarks postponed "till we're done with
the refactoring"). The refactoring is done, so both rulings' own conditions are
met — **the user's launch of this brief lifts them**. What that deferral
accumulated, stated plainly so it is priced: **81 checkpoints have never been
benched**, and **53 checkpoints of reference conversions ran without the Miri
lane** — the aliasing class only Miri sees was watched by the span scanner and
targeted probes alone. E3 pays both debts.

## Verification regime for this session

Per closing checkpoint (steps 1–4): `bash rust/tools/gates.sh commit`, plus the
census (`bash rust/tools/unsafe_census.sh`), the span scan
(`python3 rust/tools/f239_span_scan.py`), and the ratchet. Tools run from the
crate root; results quoted with denominators; one gate at a time;
non-reproducing failures re-run five times; tags come off only with their
unsafe. The Miri baseline file is newest-first — **prepend**.

## The steps

### Step 1 — promote `unsafe_op_in_unsafe_fn` (measured, mechanical)

S11 measured this exactly: deleting the lint from the crate-wide allow costs
zero errors, but the lint is warn-by-default so deletion alone gates nothing.
Promote it to `#![deny(unsafe_op_in_unsafe_fn)]` at the crate root: **137
explicit `unsafe {}` blocks across 5 files** — 121 in `api/codec_api.rs`'s
C-ABI thunks, 9 in `wels_preprocess.rs`, 4 in `encoder_context.rs`, 1 each in
`decoder/error_concealment.rs` and `api/version.rs`. One checkpoint; the blocks
are honesty markers, not new unsafe (the ratchet's `unsafe_block` count rises
by design — say so in the commit, the way S11.41 did).

### Step 2 — the last C-ABI pair moves home

`SetOption`/`GetOption`'s two `c_void` options are the last non-`src/api/`
C-ABI surface. Move them into the api island exactly as the version exports
moved (same referees: the exported symbol set must not change —
`tools/abi_sizes.sh` identical, export-list check green).

### Step 3 — the last screen-content cast

One `SCREEN_CONTENT(dormant)` cast remains, in `svc_mode_decision.rs`. Its 22
siblings **dissolved** at S11.3 — the storage type was made unable to hold an
`SVAAFrameInfoExt`, so there was no cast left to make. Prefer the same for
this one; convert to a safe shape only if dissolution genuinely doesn't reach
it. Dark-code discipline: no sweep row executes this arm; the compiler and
review are the referees. (A note from the ledger: this is D-scope-6's item —
there is no "Phase 10 lane" to defer it to; that vocabulary is retired.)

### Step 4 — tighten the pin

Re-run the census after steps 1–3, shrink the enumerated list to what remains,
seal any file whose last allow fell, regenerate the ratchet downward and
**pin** it. Print the final list in full — every remaining allow with its tag
and one-line justification. This is the floor the repository keeps.

### Step 5 — E3: the exit battery, entire

`FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh exit`, plus the
pieces it doesn't fold in, each with its verdict quoted:

* **Full test suite** both profiles; **both-profile sweeps** (all 847 configs);
  **ABI export list**; **dlopen harness**; **upstream gtest** against
  `tools/abi_harness/gtest_known_failures.txt` (ratcheted — no new failures).
* **Full Miri**: the `--lib` lane (re-budgeted — the fork pair is 73% of the
  lane's cost, F273), Miri over the differential integration tests, and **the
  two full-encode fork probes** — `rust/tools/fork_join_probe.sh`, ~59 minutes
  as a parallel pair, tripwire against its baseline. This run carries the
  53-checkpoint no-lane debt: if it reports anything, remember Miri stops at
  the **first** error — fix, then run the span scanner for siblings before
  trusting a green re-run, and localize with the per-checkpoint commits.
* **Both benches against the perf budget** (plan §6) — the 81-checkpoint debt.
  Protocol: same machine both sides, **two** after-runs (phantom ±3% first-run
  swings are recorded fact), decoder and encoder both. Precedent says expect a
  find: the only bench taken since S5 caught a real 7% regression on its first
  run. A real regression bisects over the per-checkpoint commits — never
  guessed at; a fix that changes output bytes is wrong by definition; a fix
  that changes speed re-benches.
* **Budget real time for what the battery surfaces.** This is the plan's last
  and largest risk, deferred here by explicit ruling, twice reaffirmed.

### Step 6 — the close

Tick the plan's §1 exit conditions **item by item, 1 through 7, each with the
command that proves it**. Update both tables in
`rust/docs/safe_conversion_execution_plan.md`; prepend the Miri baseline;
write the final scorecard (project start → end: raw_ptr 5390 → the pinned
number, unsafe_fn 1372 → the pinned number, the one impl, the enumerated
floor); findings from **F298**; and a closing line that either declares the
plan **COMPLETE** or names exactly what stands between the tree and complete —
nothing vaguer than a list.

## Rules that bind (the compressed set)

* Bit-exactness is stop-the-line; the F3 flake protocol for anything that
  doesn't reproduce (five re-runs, then head-vs-control alternation).
* A probe's control must be seen red at a calibrated round count; a green
  mutation run is only evidence if the rebuild actually happened (an mtime
  moved backwards silently reruns the stale binary — S11 caught this).
* Miri reports one error and aborts — a green re-run after one fix proves that
  one fix; scan for siblings.
* In any tabulation, label rows measured or inferred; promote before executing.
* Read the justifications, not just the code — a resolver's written exit
  clause is a work queue, and three accessors died in S11 because someone
  finally read theirs.
* Backticks inside a double-quoted `git commit -m` are command-substituted
  away; quote single or use a file.

## The report

Plain prose: each closing checkpoint with its gate verdict; the full E3 battery
verdict by piece — Miri lane, fork pair with ratio, differential Miri, benches
with both runs' numbers, gtest, dlopen, ABI, sweeps; anything found, with its
fix and re-verdict; the final census list; the §1 checklist with commands;
every place this brief was wrong, quoting the sentence; and the closing
declaration. If any piece of E3 does not run, that is the report's first
sentence, not its last.
