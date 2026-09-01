# Safe-conversion plan — Session S13: the exit battery

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. This session changes no code except to fix what the
battery itself surfaces.*

## What this session is

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264 that
must stay **byte-identical** to the C++ on every harness stream. Every code
change of the safe-conversion refactoring is complete (S12 ran to fifteen
checkpoints and closed; verify its close row and re-run the census before
starting — expect the enumerated floor of **21 allows in exactly three
categories** (17 `instrument(test)`, 2 `recon-seam`, 2 `C-ABI`), one
`unsafe impl`, 70 of 86 files sealed, the ratchet pinned at 393/52/122). What has **not** run is the verification that two standing rulings
deferred to the end:

* **D-gate-9** struck the per-checkpoint Miri lane mid-S11 — **53 checkpoints
  of reference conversions** ran on the byte sweep, the span scanner and
  targeted probes alone. The aliasing class only Miri sees has not been
  checked across them.
* **D-gate-8 (amended)** postponed all benchmarks "till we're done with the
  refactoring" — the debt is **81+ checkpoints**, and the only bench taken
  since S5 caught a real 7% regression on its first run.

**The launch of this session lifts both rulings** — their own conditions are
met: the refactoring is done. This session IS the deferred verification.
If any piece below does not run, that is the report's first sentence.

## The battery, piece by piece (each verdict quoted in the report)

Run `FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh exit`, plus the
pieces it doesn't fold in:

### 1 — the cheap sweep of everything

Full test suite, both profiles; both-profile differential sweeps (all 847
configs, byte-compared against the C++); ABI export list; dlopen harness;
upstream gtest against `tools/abi_harness/gtest_known_failures.txt` —
ratcheted, no new failures. **Plus a cross-target check this battery was
missing**: this host is aarch64, and an x86_64-only build failure (F303) was
caught in S12 only because someone happened to build there — the battery run
on one architecture is structurally blind to the class. Run
`cargo check --all-targets --target x86_64-apple-darwin` (and
`x86_64-unknown-linux-gnu` if the target is installed; `rustup target add`
either as needed) and quote both verdicts.

### 2 — full Miri

Three parts, in this order:

* **The `--lib` lane** (all shards — the per-checkpoint form D-gate-9 struck;
  note the fork pair is ~73% of the lane's budget, so run the lane *without*
  the pair first for a fast verdict on everything else).
* **The two full-encode fork probes** — `rust/tools/fork_join_probe.sh`,
  ~59 minutes as a parallel pair on a warm target, tripwire against its
  baseline file (newest-first; prepend your result).
* **Miri over the differential integration tests.**

This run carries the 53-checkpoint no-lane debt. If Miri reports anything:
it stops at the **first** error — fix, then run
`python3 rust/tools/f239_span_scan.py` for siblings before trusting a green
re-run, and localize with the per-checkpoint commits (every one is its own
gated commit precisely for this). Any fix that changes output bytes is wrong
by definition — the sweep re-adjudicates after every fix.

### 3 — both benches against the perf budget

The 81-checkpoint debt. Protocol, exactly: same machine both sides; **two**
after-runs (phantom ±3% first-run swings are recorded fact in this tree);
decoder **and** encoder. Compare against `rust/docs/perf_baseline.md`'s
method, not its absolute numbers — absolute rows are not comparable across
machines. A real regression **bisects over the per-checkpoint commits** —
never guessed at. A fix that changes speed re-benches with the same two-run
protocol; a fix that changes bytes is stop-the-line wrong.

### 4 — fix-and-rerun buffer

Budget real time for what the battery surfaces — this risk was deferred here
by explicit ruling, twice reaffirmed, and the one data point available (the
7% catch) says the benches find things. Every fix is its own gated commit
with the piece it answers re-run green.

## The close — the plan's exit, ticked

Tick the plan's §1 exit conditions **item by item, 1 through 7, each with the
command that proves it** (`rust/docs/safe_conversion_execution_plan.md`).
Then:

* Both tables updated; the Miri baseline **prepended** with this session's
  lane numbers; the ratchet regenerated and left **pinned**.
* The final scorecard, project start → end: raw_ptr 5390 → the pinned number,
  unsafe_fn 1372 → the pinned number, unsafe_impl 11 → 1, the enumerated
  floor printed in full.
* Findings from wherever the count stands (check `rust/docs/phase9_findings.md`).
* A closing line that either declares the plan **COMPLETE** or names exactly
  what stands between the tree and complete — a list, nothing vaguer.

## Rules that bind

* Bit-exactness is stop-the-line; non-reproducing failures re-run five times
  before any conclusion (a second hit escalates to head-vs-control
  alternation — never shrug, never bisect a phantom).
* A green mutation or Miri re-run is only evidence if the rebuild actually
  happened — a restored mtime silently reruns the stale binary.
* One gate at a time; no edits while a gate runs; every count carries its
  command; tools run from the crate root.

## The report

Plain prose, battery verdict by piece — lane, fork pair with its ratio,
differential Miri, benches with both runs' numbers for both codecs, gtest,
dlopen, ABI, sweeps; anything found, with its fix, its finding number, and
its re-verdict; the §1 checklist with commands; every place this brief was
wrong, quoting the sentence; and the closing declaration.
