> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session H — T5.H: the grid flip

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.6 (S20/S21/S28/S29;
**S2c and the S14/S16 amendments are new this tail**); the session-G log entry's
hand-off and §3 sizing; the session-D closure **as corrected by T5.E2**. This file
scopes the session and supersedes on disagreement; fix disagreements in place.
Counts measured at `52a84c42`; re-grep before acting (S24 — multiline where the
shape is an expression).

## 0. Start

1. Commit the inherited doc tail (S2c, the S14/S16 amendments, this brief).
2. Open per **S27**: session G ended accepted; tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: **451+ / 445+ /
   20** (session G added tests — recount), Miri **312** (the probe is in the
   `--lib` gate now, ~7 min), census **60**, goldens **56**, `raw_ptr` 4604.
3. Recount every number you rely on. The sizing that governs this session:
   **75 signature positions, 24 fields, ~697 accesses** — bigger than one session,
   and half a flip is the state S20 forbids. The decomposition below is the
   S20-legal split from the hand-off.

## 1. Face 1 — pure addition: `MbGrid` exists and is proven, nothing flips

- `MbGrid` = a struct of the Phase-1 `MbArray<T>`s; its **field union is read off
  `InitialDqLayersContext`'s allocation block** — mechanical since T5.G2 (all 24
  element types agree with their allocations; F32 fixed the two that lied).
- The safe grid goes in `safe/mb_grid.rs` (**`#![forbid(unsafe_code)]` — S28's raw
  accessors do not go there**; `SPicture::data_ptr`'s home is the precedent: raw
  bridges live with the consumer side, marked `SHIM(phase5)`).
- **Teardown dimensions** (T5.E2's correction): the grid carries the
  **allocation's** dimensions, not the current slice's — sized once at
  `InitialDqLayersContext`, and whatever frees it reads those.
- S28 full-reach Miri tests per accessor, **before anything flips onto the grid**.
- S21: `DqLayerState`/the grid's construction — the layer is `WelsMallocz`-reached;
  zeroed reach is still zeroed reach even with the allocator fixed. Owned `Vec`s
  inside it need the constructor-or-shell decision made in the same commit.

## 2. Face 2 — the flip, per array-family

`SDqLayer`'s 24 per-MB array fields flip onto the grid, **one family per commit**
(the plan's own 5.2 shape), strangler shims where callers lag (R7, marked
`SHIM(phase5)`). Per commit: S29 spelling at every touched site; census keys
re-keyed in the rename's own commit; the probe is a gate — if it goes red, that is
the flip's own defect and it is attributable, which is what sessions E–G bought.
`pBitStringAux` and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor die in the
family that carries them.

## 3. Face 3 — behind the flip, as families land

The ~28 cache-fill signature re-points (`parse_mb_syn_*`'s 30-entry scratch caches
become `&mut` locals passed down) — mechanical, only for families already flipped.

## 4. Gates

Per phase5.md §7, per face: full battery; goldens frozen at 56; sweeps 341/341
both profiles; F3 per **S14** (append at adjudication; reconcile the running
total at close — the ledger was just brought current at 32/11/11); perf per
**S2b/S2c** — the flip's commits touch allocation and layout, so they do **not**
qualify for S2c's waiver: 3-pair medians per family commit; Miri with the probe
(~7 min per round trip — batch faces before running when commits are small);
ratchet per S16, **per-file deltas, never totals**; census green with re-keyed
entries.

## 5. Close

Log entry: which families flipped (and which remain, by name), the S21 decisions,
S28 test inventory, probe verdicts. Update phase5.md §2 marks. Hand-off names the
remaining families or — if the flip completes — the next unit (the ~40 remaining
re-points, then 5.1's second half or 5.3, whichever the tree argues).

## 6. Non-goals

No 5.3 pulls (F22's map waits for the grid). No `PicPool`/identity (deferred). No
F23 work (Phase 8's). No golden movement. No pool/threading (F12/P10). No
`get_unchecked` (S8). Stop at a family boundary rather than land half a family —
S20 applies inside the decomposition exactly as it did to the whole.
