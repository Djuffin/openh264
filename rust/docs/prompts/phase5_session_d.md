> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session D — T5.D: the 5.2 closure, F22's answer, and the double path

Governing: [`phase5.md`](phase5.md) §0 and §2 verbatim; plan §7.6
(S20/S21/S24/S25/S28); the session-C log entry's "what to carry" list. This file
scopes the session and supersedes on disagreement; fix disagreements in place.
Counts measured at `d551e828`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (the S27/S28 plan additions, phase5.md's updates,
   this brief).
2. Open per **S27** — first live use: the tail is docs-only and session C ended
   `OVERALL: PASS`, so if `rust/tools/` and the toolchain are unchanged, run the
   cheap subset only (build both profiles, tests, ratchet, census); the S2 null
   waits until a perf verdict needs it. Last recorded: **448 / 442 / 20**, Miri
   **309**, census **61**, goldens **56**.
3. Recount every number you rely on.

## 1. Face 1 — the 5.2 closure, computed and recorded before any edit

`SDqLayer` → `DqLayerState` with owned `MbGrid`; the `sMb`/`SDqLayer` double path
(P2) is the core. The wiring is `InitialDqLayersContext`
(`decoder_core.rs:2620`; per-MB array allocations at ~`:2650`; the plan's
`:3843-3869` cite is stale). Expect the phase's largest closure:

- Every struct reachable from `SDqLayer`'s changed fields through embeddings,
  signatures, and asserts (S20).
- The S25 re-entrancy enumeration alongside: who reaches `SDqLayer` / `sMb` while
  a borrow is held.
- S21 construction paths: `SDqLayer` and the `sMb` arrays are `WelsMallocz`-reached
  — the constructor-vs-shell decision lands with the first owned field.
- `census_allowlist.txt` entries owned by 5.2 unify inside this step's commits;
  remove their lines as they go.

**The written closure is a deliverable in its own right** — it goes in the log even
if no conversion lands this session.

## 2. Face 2 — F22's reachability answer (cheap; do it early)

Can `pDec` be null on the CABAC parse path? Trace from the CABAC entry points
(`parse_mb_syn_cabac.rs`'s callers) to where `pDec` is set. Record the answer with
evidence in the log — it decides 5.3's guard semantics (28 null guards in
`mv_pred.rs` vs 0 in the CABAC copies). No code change in this face.

## 3. Face 3 — first conversion commits, as the closure licenses

The double-path kill: `MbGrid` becomes the single owner of the per-MB arrays that
`sMb` and `SDqLayer` alias today. One family per commit if the closure decomposes;
**the closure decides the commit boundaries, not this brief.** Per commit:

- **S28 for every new accessor** — derive from the allocation root, never through a
  narrowing slice, and give each accessor a Miri test reading its full legal reach.
  `MbGrid`'s accessors are exactly the shape that earned the rule.
- `SDqLayer::pBitStringAux` retires and `cabac_decoder.rs`'s `SHIM(phase5)` rbsp
  accessor dies, in whichever commit flips those fields.
- The ~40 `parse_mb_syn_*` signatures re-point (30-entry scratch caches become
  `&mut` locals passed down) — mechanical, and only after the owner flip.

## 4. Tail (cheap, if passing)

Re-scope `SPicture::data_ptr`'s `SHIM(phase5)` marker (`picture.rs`) to name its
surviving consumer: the output-path fill at `decoder_core.rs:~1087` is the public
`(pointer, stride)` contract — Phase 8's, not 5.6's — so the phase-exit sweep
expects exactly that one.

## 5. Gates

Per phase5.md §7, per face: full battery; goldens frozen at 56; sweeps 341/341 both
profiles; S14 for any `mt` wrong-length hit; 3-pair medians per seam (expect flat —
the decode recovery arrives when kernel dimensions go static, not here); Miri (the
S28 tests run under it); ratchet per S16 with deltas named; census green with
allowlist lines removed as entries unify.

## 6. Close

Log entry: the closure as computed, F22's answer with evidence, the S25
enumerations, the S21 decisions. Update phase5.md §2's marks. Hand-off names the
next unit: the rest of 5.2, or 5.3 only if 5.2 landed whole (unlikely).

## 7. Non-goals

No 5.3 unification before F22's answer is recorded, and none this session unless
5.2 lands whole. No `PicPool`/identity work (deferred until after 5.2, phase5.md
§1). No 5.4+ pulls. No golden movement. No pool/threading edits (F12/P10). No F3
work beyond S14. No `get_unchecked` (S8).
