# Phase 5b, session B — the divergence, alone

One face (Eugene, 2026-08-17: face 0 in its own session): diagnose the B-slice
divergence in session A's preserved parse-tree conversion to a **measured**
cause, correct it, and **land the conversion at 60/60**. Nothing else.
**D-par-1**, **D-fid-1**, **S31**–**S33**, the terminal rule and the decision
ladder in force. Counts at `e39cfaaf`; re-grep at open (S24).

**The parity guard is absolute**: corpus 2690/17 + 2707/0, conformance 60/60,
benches bit-identical. The conversion lands only at 60/60 — that is the
done-test, not a preference.

## 0. Start

1. Commit the inherited doc tail (this brief, session C's, the patch at
   `rust/docs/patches/phase5b_face1_parse_tree.patch` — copied from `/tmp` by
   the steward; `/tmp` survival is no longer load-bearing).
2. Open per **S27** (A closed `OVERALL: PASS` 13/0/1 at `7a4ad7b5`): build both
   profiles + `--all-targets`, tests, ratchet, census. Recount: allow **147
   (143+4)**, blocks **148**, `unsafe fn` **30**, raw_ptr **219 (156+63)**.
3. Probes per seam; S33 on every number; breadcrumbs.

## 1. The face — diagnose, then land

The signature (log, session A): **11 of 60 assets, every one B-slice**
(`bframe_9` ×2, `temporal_direct` ×6, `grid_48x32`, `test_scalinglist_jm`),
frame counts correct — a pixel divergence. Eliminated by measurement: byte-wise
`bytes_equal`/`bytes_copy` (same 11), slice header parsed in place (same 11).

1. Apply the patch to a scratch branch. **Test the leading hypothesis first,
   because it predicts exactly this failure set**: `InitDqLayerInfo` reads
   `sPredWeightTable`, `sRefPicListReordering`, `sRefMarking` out of the node —
   the B-slice fields the sub-parsers write. A lost write-back produces these
   11 and no others. Measure it: old and new builds on `grid_48x32` (smallest
   failing asset, in-repo), `portref` per-frame hashes to the first divergent
   frame, then the three fields compared at `InitDqLayerInfo` entry on both
   sides. One eprintln per side, two runs.
2. If it holds: fix the write-back, 60/60, land. If not: the three untested,
   in order — the `SNalData` union→struct change; `nal_cur`/`slice_hdr_nal`
   index semantics across access units; `slice_split`'s reader-by-value. Each
   is one bounded experiment; S33 keeps hypotheses and findings in separate
   columns.
3. **The fallback that guarantees termination**: bisect the patch itself.
   Split it into its independent sub-conversions (owned slots / stored aliases
   as indices / union→struct), apply cumulatively, run the 11 at each step —
   the diverging hunk identifies itself. Mechanical, bounded, no judgement
   required.
4. If the cause is a **pre-existing order-dependence the old code masked**,
   that is a finding (next F-number, with the reproduction) and the fix still
   preserves C++ output — parity is the definition, not the old tree.
5. Land whole: patch + fix, probe run, gates per commit.

## 2. Gates and close

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Probe per seam. Full battery once at close; F3 per S14. **No perf span** — the
5b window is measured once, at session C's close (D-gate-1's shape). Close-out
≤ 30 lines: the measured cause, the fix, the landing counts; hand-off →
**session C** ([`phase5b_session_c.md`](phase5b_session_c.md)).

## 3. Non-goals

No shells, no sweep, no allow-list work — session C's. No encoder sites. No
`api/` internals. No golden movement. No landing below 60/60 — the diagnosis
has a terminating fallback, so there is no version of this session that ships
a divergence, and none that cannot finish.
