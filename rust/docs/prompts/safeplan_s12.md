# Safe-conversion plan — Session S12: the last code changes

*Everything you need is in this file. Re-run every count before quoting it — trust
the tree over this document. Before acting on any claim, read the lines it
describes; a claim of absence gets its grep; every count excludes comments or says
it doesn't.*

## Where the project stands

`rust/crates/openh264-rs/` is a line-by-line Rust port of Cisco's OpenH264,
shipping as a drop-in `libopenh264` replacement that must stay **byte-identical**
to the C++ on every harness stream. The conversion campaign is **done**: the
Phase 9 queue emptied at S11.52. The tree today:

* `#[allow(unsafe_code)]` outside `src/api/`: **32** — a *list*, not a count:
  21 `instrument(test)` probes plus 11 named permanent exceptions, pinned by
  equality in `rust/tools/unsafe_census.sh` (a new allow fails CI naming
  file:line, even if plausibly tagged).
* One `unsafe impl` in the whole tree (`Sync for SharedCells`, rec_view.rs).
* 65 of 86 files sealed `#![forbid(unsafe_code)]`; ratchet pinned at
  425 raw-ptr / 59 unsafe-fn / 136 unsafe-block.

**This session finishes every remaining code change of the refactoring — four
small, measured items.** The heavy verification (full Miri, the fork pair, the
benches — 81 checkpoints of bench debt, 53 of Miri-lane debt) is deliberately
**not** this session's: it runs as S13, the exit battery, immediately after.
Do not start any part of it here.

## Verification regime

Per checkpoint: `bash rust/tools/gates.sh commit` (unit tests both profiles +
ratchet), the census (`bash rust/tools/unsafe_census.sh`), the span scan
(`python3 rust/tools/f239_span_scan.py`), and the differential sweep for
anything on a live path (`family` level — byte referee, both profiles). Tools
run from the crate root; results quoted with denominators; one gate at a time;
non-reproducing failures re-run five times; tags come off only with their
unsafe.

## The steps

### Step 1 — promote `unsafe_op_in_unsafe_fn` (measured, mechanical)

S11 measured this exactly: deleting the lint from the crate-wide allow costs
zero errors, but the lint is warn-by-default so deletion alone gates nothing.
Promote it to `#![deny(unsafe_op_in_unsafe_fn)]` at the crate root: **137
explicit `unsafe {}` blocks across 5 files** — 121 in `api/codec_api.rs`'s
C-ABI thunks, 9 in `wels_preprocess.rs`, 4 in `encoder_context.rs`, 1 each in
`decoder/error_concealment.rs` and `api/version.rs`. One checkpoint; the
blocks are honesty markers, not new unsafe — the ratchet's `unsafe_block`
count rises by design; say so in the commit and regenerate after a
field-by-field diff, the way S11.41 did.

### Step 2 — the last C-ABI pair moves home

`SetOption`/`GetOption`'s two `c_void` options are the last non-`src/api/`
C-ABI surface. Move them into the api island exactly as the version exports
moved. Referees: the exported symbol set must not change —
`tools/abi_sizes.sh` identical, the export-list check green, `family` sweep
byte-identical.

### Step 3 — the last screen-content cast

One `SCREEN_CONTENT(dormant)` cast remains, in `svc_mode_decision.rs`. Its 22
siblings **dissolved** at S11.3 — the storage type was made unable to hold an
`SVAAFrameInfoExt`, so there was no cast left to make. Prefer the same here;
convert to a safe shape only if dissolution genuinely doesn't reach it.
Dark-code discipline: no sweep row executes this arm; the compiler and your
review are the referees. This is ruling D-scope-6's last item — there is no
"Phase 10 lane" to defer it to.

### Step 4 — tighten the pin

Re-run the census after steps 1–3, shrink the enumerated list to what remains,
seal any file whose last allow fell (leaf files only), regenerate the ratchet
downward and **pin** it. Print the final list in full — every remaining allow
with its tag and one-line justification. This is the floor the repository
keeps, and after this step **no code change of the refactoring remains**.

## The close

Both tables in `rust/docs/safe_conversion_execution_plan.md` (session map and
dated log); findings from **F298** in `rust/docs/phase9_findings.md`; the
final census list quoted in the session row. The roll-forward line is one
item and must say exactly this: **E3 — the exit battery, entire — runs as
S13** (full Miri including the two full-encode fork probes, both benches
against the 81-checkpoint debt, dlopen/gtest/ABI/sweeps).

Report in plain prose: each checkpoint with its gate verdict; the final
census list; the tracking number's movement; every place this brief was
wrong, quoting the sentence; and the one-line roll to S13.
