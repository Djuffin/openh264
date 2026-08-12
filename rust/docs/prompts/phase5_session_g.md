# Phase 5, session G — T5.G: F25's inventory, the probe un-ignored, then the grid

Governing: [`phase5.md`](phase5.md) §0/§2 verbatim; plan §7.6 (S29 is the spelling
for every site this session touches; S24 now carries F29's multiline clause;
S20/S21/S25/S28 as always); [`phase5_findings.md`](../phase5_findings.md) F25 and
F28 (the whole-context/whole-reach retag law); the session-F log entry. This file
scopes the session and supersedes on disagreement; fix disagreements in place.
Counts measured at `9a5ce054`; re-grep before acting (S24 — including its new
clause: the sites below are expression-shaped, so grep multiline).

## 0. Start

1. Commit the inherited doc tail (the S24 clause, this brief).
2. Open per **S27**: session F ended accepted (its FAILs were the S16-regenerated
   ratchet and F3-adjudicated sweep rows); tail is docs-only — cheap subset if
   `rust/tools/` and the toolchain are unchanged. Last recorded: **451 / 445 /
   20**, Miri **311**, census **60**, goldens **56**, `raw_ptr` 4589.
3. Recount every number you rely on.

## 1. Face 1 — the 12 `&mut *pCtx` bindings

F25's named inventory: **7 in `decode_slice.rs`, 5 in `manage_dec_ref.rs`**.
Spelling per **S29** — `addr_of_mut!`-derived pointers or `(*pCtx).field` per use;
no binding that outlives one expression; escaping bindings (stored anywhere)
first. F29's helper precedent applies where several sites derive from one field:
one `addr_of_mut!` helper beats N inline derivations. Zero bytes of output move.

## 2. Face 2 — the probe comes off ignore, and the queue behind it

The same commit that clears the last binding removes the probe's
`#[cfg_attr(miri, ignore)]` (`decode_slice.rs` — the label now names exactly this
inventory). Then run it. **Assume the queue is non-empty** — every round trip
since T5.D2 has found something. Per defect: S12-record with owner; in 5.2's path
→ fix per S29 in-session; out of path → route with an F23-style label. **Bound:
three defects beyond the first — then stop at a green seam and hand off the
inventory.** The probe passing un-ignored, end to end, is the standing start
face 3 has been waiting for since session E.

## 3. Face 3 — the grid conversion, only from a standing start

Unchanged from session F's face 4, carried forward:

Per the closure **as corrected by T5.E2**: `sMb` has three live consumers, and the
free path's `numMb` reads the **allocation's** dimensions where the layer carries
the current slice's — `DqLayerState`/`MbGrid` must keep allocation-sized
dimensions reachable for teardown or the free is wrong on any stream below the
negotiated maximum. Grid goes **in the layer** (50 of 66 signatures). Per commit:
S28 accessors with full-reach Miri tests; S21 for `DqLayerState` construction
(`WelsMallocz`-reached — though the allocator now keeps provenance, zeroed reach
is still zeroed reach); S29 spelling throughout; `pBitStringAux` and
`cabac_decoder.rs`'s `SHIM(phase5)` accessor die with the fields that hold them.
The ~40 `parse_mb_syn_*` signature re-points ride behind the flip, mechanical.

## 4. Gates

Per phase5.md §7, per face: full battery; goldens frozen at 56; sweeps 341/341
both profiles; F3 per **S14** (step 0 applies to face 1 only if the binary is
genuinely unchanged — these are decoder-side edits, so expect to run the full
protocol; measurements 33+ append); 3-pair medians per seam (expect flat); Miri —
from face 2 on, the probe is part of the gate, un-ignored; ratchet per S16
(S29 spellings may move `raw_ptr` *up* again — say so, the way session F did);
census green.

## 5. Close

Log entry: the 12 bindings' disposition, the queue as found (each defect: owner,
fixed or routed), grid faces landed with their closures. Update phase5.md §2
marks and its blocker note (which should finally say **none**). Hand-off names the
next unit — rest of 5.2 if the grid is partial, else 5.1's second half
(`PicPool`/identity) or 5.3 (F22's per-function map is ready).

## 6. Non-goals

No F23 work (Phase 8's; route, don't fix). No 5.3 unification. No
`PicPool`/identity (still deferred per phase5.md §1). No golden movement. No
pool/threading edits (F12/P10). No `get_unchecked` (S8). No allocator follow-ups —
T5.F2 is complete and pinned; anything new it exposes is a finding, not a tweak.
