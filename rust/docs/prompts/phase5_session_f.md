> **HISTORICAL — Phase 5 closed at session AC (2026-08-17, `5ebaf904`).**
> This brief is the record of what one session was asked to do. It is not an
> instruction to anyone now: read [`phase5.md`](phase5.md) for the phase's
> close and [`phase6.md`](phase6.md) for what follows.

# Phase 5, session F — T5.F: F26's experiment, the allocator fix, the backlog it releases, then the grid

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.6 (**S22 is this
session's spine; S29 is new** — with S20/S21/S24/S25/S28);
[`phase5_findings.md`](../phase5_findings.md) **F26 in full** (its experiment and
fix sketch are the contract); the session-E log entry. This file scopes the session
and supersedes on disagreement; fix disagreements in place. Counts measured at
`2a366af4`; re-grep before acting (S24).

## 0. Start

1. Commit the inherited doc tail (S27's clarified predicate, S29, the S20 clause,
   this brief).
2. Open per **S27 as amended**: session E ended accepted (its one FAIL was
   F3-adjudicated, measurement 29); the tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: **449 / 443 /
   20**, Miri **309**, census **60**, goldens **56**, `raw_ptr` 4548.
3. Recount every number you rely on.

## 1. Face 1 — the one-run experiment (before touching the allocator)

Per F26's entry, verbatim: restore the single `&mut` spelling at the
`pBitStringAux` store (`decoder_core.rs`, the `DecodeCurrentAccessUnit` store),
keep everything else, run the probe once, record which error Miri reports, revert.
This settles whether F26 was reachable before T5.E1 or exposed by it — **do not
settle it by reasoning**; the answer goes in F26's entry either way and decides
nothing about face 2 (the wildcard heap blinds Miri regardless).

## 2. Face 2 — the allocator keeps provenance

`memory_align.rs:49-50`: the integer→pointer round trip becomes pointer-land
arithmetic (`align_offset`, or `add`/`byte_sub` per F26's sketch). Constraints:

- **Identical addresses, byte for byte** — only provenance changes; zero output
  movement, full battery agrees or the commit reverts.
- `memory_align.rs` is `common/` — the fix un-blinds **both** codecs' heaps
  (S18: say so in the commit; the encoder's Miri skips (F12) still stand and are
  not this session's).
- The alignment arithmetic's edge cases (already-aligned input, the header trick)
  get a unit test pinning address equality against the old formula across
  alignments.

## 3. Face 3 — the S22 backlog run

Tracked provenance crate-wide means Miri can now see borrow structure it has been
silently permitting. In the same session as the repair (S22):

1. Full Miri `--lib`, the differential set, and the probe with its
   `#[cfg_attr(miri, ignore)]` **removed** (`decode_slice.rs`, labelled against
   F26 — this face is that label's due date).
2. Expect new failures. Per defect: S12-record with owner; in 5.2's path → fix
   in-session per S29 (`addr_of_mut!`; escaping borrows first); out of path →
   route with an F23-style label and continue.
3. **Bound: three defects beyond the first new one** — then stop at a green seam
   and hand off the inventory. The queue list is this face's deliverable even if
   fixes don't all land.

## 4. Face 4 — the grid conversion, only from a standing start

Per the closure **as corrected by T5.E2** (the session-E log section is
authoritative): `sMb` has **three** live consumers, and the free path's `numMb`
reads the **allocation's** dimensions where the layer carries the current
slice's — `DqLayerState`/`MbGrid` must keep allocation-sized dimensions reachable
for teardown or the free is wrong on any stream below the negotiated maximum.
Grid goes **in the layer** (50 of 66 signatures). Per commit: S28 accessors with
full-reach Miri tests; S21 for `DqLayerState` construction (`WelsMallocz`-reached);
S29 spelling throughout; `pBitStringAux` and `cabac_decoder.rs`'s `SHIM(phase5)`
accessor die with the fields that hold them.

## 5. Gates

Per phase5.md §7, per face: full battery; goldens frozen at 56; sweeps 341/341
both profiles; F3 per **S14 including step 0** (docs/tests-only faces acquit by
binary hash); 3-pair medians per seam (expect flat); Miri — from face 3 on, the
probe runs un-ignored and is part of the gate; ratchet per S16 (face 2 may move
nothing; faces 3–4 delete); census green.

## 6. Close

Log entry: the experiment's answer (into F26's entry too), the backlog inventory
with owners, faces landed. Update phase5.md §2 marks and its F26 note. Hand-off
names the next unit (rest of 5.2: the ~40 `parse_mb_syn_*` re-points behind the
grid flip, then 5.3 with F22's per-function map).

## 7. Non-goals

No F23 work (Phase 8's; route, don't fix). No mass `&mut *pCtx` sweep (S29: per
file touched, with its conversion). No 5.3 unification. No `PicPool`/identity
(deferred per phase5.md §1). No golden movement. No pool/threading edits
(F12/P10). No `get_unchecked` (S8).
