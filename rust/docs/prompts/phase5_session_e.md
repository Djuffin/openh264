> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session E — T5.E: F24, the probe queue, then 5.2's first faces

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.6 (S14 now carries the
hash shortcut; S20/S21/S24/S25/S28); [`phase5_findings.md`](../phase5_findings.md)
F23/F24; the session-D log entry (the 5.2 closure and the probe's story). This file
scopes the session and supersedes on disagreement; fix disagreements in place.
Counts measured at `b3e94151`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (S14 step 0, the Phase 8 F23 annotation, this
   brief).
2. Open per **S27**: the tail is docs-only on a PASS exit — cheap subset (build
   both profiles, tests, ratchet, census) if `rust/tools/` and the toolchain are
   unchanged. Last recorded: **449 / 443 / 20**, Miri **309**, census **61**,
   goldens **56**, ratchet `raw_ptr` 4600.
3. Recount every number you rely on.

## 1. Face 1 — F24 (the 5.2 blocker)

`ParseSliceHeaderSyntaxs`, `decoder_core.rs:2000-2576`: `pSliceHead` borrows a
field **inside** what `pSliceHeadExt` borrows (`:2018-2021` — creating the second
invalidates the first; `pSliceHead` is then used 74 times, `pSliceHeadExt` 50),
plus `:2023` writes through the raw `kpCurNal` while both are live. `pNalHeaderExt`
(17 uses) is a non-overlapping sibling — not part of the defect.

Fix shape per session B's precedent: derive per use from `kpCurNal` — no long-lived
overlapping `&mut`; the F24 entry's Miri output gives the byte-exact offsets to
check against. Zero bytes of output move. **The same commit removes the probe's
`#[cfg_attr(miri, ignore)]`** (`decode_slice.rs:5409`) — the attr is labelled debt
against exactly this fix.

## 2. Face 2 — the probe queue (depth unknown; bounded)

The probe reports one defect per run. After F24: run it under Miri; then per
outcome —

- **Passes end to end** → the S25 audit gate 5.2 needed exists; record the pass in
  the log and proceed to face 3.
- **New defect** → S12-record with owner (F-number if it clears the bar). If it
  sits in the parse/decode path 5.2 converts, fix it in-session by the same
  per-use pattern and loop. If it is out of 5.2's path, route around it the way
  F23 was, note the routing at the probe, and continue.
- **Bound**: if three defects queue beyond F24, stop at a green seam and hand the
  list off — session E must not become an unbounded hunt. The probe staying
  partially routed is acceptable **only** with each routing labelled like F23's.

## 3. Face 3 — 5.2's subtraction commit, then the grid as licensed

From the recorded closure (session-D log):

1. **Pure subtraction first**: `SMbCache` dies whole (129 of 130 accesses are
   lifecycle; the closure names the one reader and its replacement),
   `LAYER_NUM_EXCHANGEABLE = 1` flattens, `pMotionPredFlag` is dead. Zero output
   movement, no layout asserts in the way, zero census entries.
2. **The grid conversion only from a standing start**: the closure's decisive
   number — 50 of 66 DqLayer-pointer signatures take only that pointer — puts the
   `MbGrid` **in the layer**, not the context. S28 for every accessor (+ Miri
   full-reach test each); S21 for `DqLayerState`'s construction paths
   (`WelsMallocz`-reached); `pBitStringAux` and the `SHIM(phase5)` rbsp accessor
   die with the fields that hold them.

## 4. Gates

Per phase5.md §7, per face: full battery; goldens frozen at 56; sweeps 341/341 both
profiles; **F3 per S14 including step 0** — a docs/tests-only face builds a
byte-identical `rust_enc`, and hash equality is the whole acquittal; 3-pair medians
per seam (expect flat); Miri — after face 1 the probe runs un-ignored and is part
of the gate; ratchet per S16; census green.

## 5. Close

Log entry: F24's fix with the Miri before/after, the probe queue as found (each
defect: owner, routed or fixed), 5.2 faces landed. Update phase5.md §2 marks and
the F24 block note. Hand-off names the next unit (rest of 5.2: the ~40
`parse_mb_syn_*` signature re-points ride behind the grid flip).

## 6. Non-goals

No F23 work (Phase 8's — the annotation is in the plan; route, don't fix). No 5.3
unification (F22's per-function map is written; it waits for the grid). No
`PicPool`/identity (deferred per phase5.md §1). No golden movement. No
pool/threading (F12/P10). No `get_unchecked` (S8).
