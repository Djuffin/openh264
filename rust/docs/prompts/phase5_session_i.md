# Phase 5, session I — T5.I: the window-accessor retrofit (D-perf-5)

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.4 — **D-perf-5 is this
session's charter** — and §7.6 (S1/S2/S2b bind every measurement; S8/S9 are the
idiom; S24; S28 only if a raw bridge's derivation changes); the session-H log entry
§6 (the cost) and its family table. This file scopes the session and supersedes on
disagreement; fix disagreements in place. Counts measured at `75188044`; re-grep
before acting (S24).

## 0. Start

1. Commit the inherited doc tail (D-perf-5 in §7.4, §0's two rows, this brief).
2. Open per **S27**: session H ended accepted; tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: Miri **324**
   (~524s), census **60**, goldens **56**, `raw_ptr` **4570**; test counts grew in
   session H — recount.
3. **Run the S2 null immediately** — this session's verdicts are perf verdicts and
   the floor is the instrument. The verdict standard is **7 interleaved pairs**
   (S2b: session H's signal firmed from 3 → 7; 3-pair readings are progress checks
   only).

## 1. The idiom

Primary shape: **re-type each family's element so the per-MB record is the
element** — `MbArray<[T; K]>`, layout-identical to the flat allocation (the
transcription stays a transcription of the allocation block). `grid.pMv[mb]` is
then `&mut [i16; K]`: one bounds check per MB per array, and within-record
indexing is const-bounded, check-free. Hoist the window borrow to the MB loop
head, where the C++ hoists its pointer.

- Where a family's per-MB width is **not** constant (verify per family — S24), the
  fallback is an exact-span window accessor (S9's shape): still one check per MB.
- `safe/mb_grid.rs` stays `#![forbid(unsafe_code)]` — the idiom is fully safe
  (indexing plus const-size borrows). No `get_unchecked` (S8). No new raw
  derivations; S28 applies only if an existing raw bridge accessor changes how it
  derives.

## 2. The work

Retrofit **all eleven flipped families** (T5.H4–T5.H14's list; re-grep it).
Mechanical per family: element re-type, call-site window hoisting, one commit per
family or per tight cluster. Byte-exactness per commit — goldens and sweeps must
not move. **No new family flips** — that is session J's, gated on this session's
measurement.

## 3. The measurement — the session's actual deliverable

1. Whole-retrofit span at **7 interleaved pairs**, both benches, vs `75188044`.
2. Verdict lines to produce: decode median recovery; the CB row against the spent
   **+2.05%**; per-family diagnostics only if the whole-span number surprises
   (D-perf-4: the microbench is diagnostic, not mandatory).
3. **If recovery ≥ half the spent cost**: the idiom is proven — the hand-off
   carries session J's flip plan with the headroom arithmetic redone.
4. **If recovery < half**: one hour of profile + disassembly on the hottest
   accessor (D-perf-4's look; S1 — disassemble before theorising), then stop: the
   numbers and the look go to Eugene with the session-J question. **Do not
   proceed to hot-family flips on an unproven idiom.**

## 4. Gates

Full battery per commit-cluster (batch small commits before the ~524s Miri step);
goldens frozen at 56; sweeps 341/341 both profiles; F3 per S14 — append at
adjudication, reconcile at close; ratchet per S16, per-file deltas (expect ~flat —
this is accessor reshaping, not deletion); census green (re-key renamed fields in
the same commit).

## 5. Close

Log entry: per-family disposition, the 7-pair tables, the verdict against
D-perf-5's bar, and — if the idiom proves out — an S8 catalog addendum naming the
per-MB window shape as a positive result with its numbers. Update phase5.md §2
and §0's Next row. Hand-off: session J = the hot eleven with the proven idiom, or
the escalation to Eugene with data.

## 6. Non-goals

**No new family flips.** No 5.3+ pulls. No `PicPool`/identity (deferred). No
golden movement. No pool/threading (F12/P10). No `get_unchecked` (S8). And the
tripwire-vs-S20 question stays deferred per D-perf-5 — do not re-litigate it in
the log; it returns to Eugene only if the measurement says it still binds.
