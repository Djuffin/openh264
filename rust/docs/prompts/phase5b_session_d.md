# Phase 5b, session D — F56 fixed, and the decoder's dead pointers deleted

A short session between Phase 5b's close and Phase 6's open. Two faces: the
F56 fix (a **behavior change, ruled by the steward**, gated by the referee suite),
and the deletion of the pointer spellings that survive only as dead code. No
conversions; no encoder; no `api/`. **D-par-1**, **S31**–**S34** in force. Counts
at `f5ba2395`; re-grep at open (S24, code/prose split).

## 0. Start

1. Commit the inherited doc tail (this brief; the F56 ruling recorded in the
   finding).
2. Open per **S27** (5b-C closed `exit` 12/1/1, the failure F3 measurement 65,
   acquitted): build both profiles + `--all-targets`, tests, ratchet, census.
   Recount: decoder `raw_ptr` **131 (67 code + 64 prose)**, allow items **3**,
   `unsafe fn` **0**, blocks **6**.
3. S33 on every number; breadcrumb per face.

## 1. Face 0 — F56: `None` is the faithful value

**The ruling (steward, 2026-08-18): fix it.** The C memsets an `SSps*` to NULL;
the port's spelling of NULL is `None`; the `Some(SpsRef { id: 0 })` is a
niche-layout artifact (F54's class), not a transcription of anything. This is
the same argument T5.Z1 accepted for `pActiveLayerSps`, and it is ruled here —
not defaulted — because the referee suite exists precisely to make a behavior
question answerable: **the change lands only with the full corpus and
conformance run beside it, both unmoved.**

1. Read F56's record first (`phase5_findings.md`): the two reachable sites are
   `SWelsDecoderContext::active_sps` (30+ readers; the `else`-arm scan in
   `AllocPicBuffOnNewSeqBegin` cannot run today because the field is never
   `None`) and `SNalUnit`'s `sSliceHeader.sps_ref` per `MemGetNextNal` reset (one
   reader). The third site (`sPrefixNal`'s VCL arm) is written by nothing and
   read by nothing — fix it in the same commit for consistency; it costs nothing.
2. The fix is what the record says: `active_sps: None` in the context's
   constructor; drop the overwrite in `SNalUnit::memset_zero`; the prefix-NAL
   arm likewise. **Extend the byte-comparison test the shells earned** (T5b.5's
   method): after the fix the constructor and the old zeroed image differ at
   exactly the fields the finding names — assert the `offset_of!` list, so the
   change is measured rather than described.
3. **A covering test, red under revert** (F21's rule): a unit test that
   constructs the context and asserts `active_sps.is_none()`, plus one that
   drives `AllocPicBuffOnNewSeqBegin`'s `else` arm — the arm F56 says is
   unreachable today — and checks it selects the SPS the C++ would. If no
   existing stream reaches the arm, say so in the test's doc and keep the
   constructor assertion as the red-under-revert.
4. **The gate that decides**: full corpus (`compare_all.sh`, all 2707 rows) and
   conformance 60/60, **before commit**. Both unmoved → land as `T5b.8`, F56
   closed in the finding with the run's figures. Anything moves → do not land;
   the mover is evidence — record it in the finding and stop the face (this is
   the one place a stop is correct: a behavior change the referee disputes is
   the steward's to re-rule, not the session's to argue).

## 2. Face 1 — the dead pointer spellings

The 67 code occurrences are not unsafety (all six blocks are inside the three
FFI items); most are spellings with no consumer. Delete what is dead;
**leave** what Phase 8 owns. Read each before touching (S18: dead code is
deleted, not converted).

1. **`TagMCRefMember` / `sMCRefMember`** (`error_concealment.rs:181–210`): the
   MC descriptor whose consumers died at 5b-A. Definition, typedef, `Default`
   impl — zero uses outside the definition. Delete; the 12 `*mut u8` go with
   it.
2. **The three pointer typedefs**: `PPicture` (`picture.rs:322`, ~11 uses),
   `PPicBuff` (`pic_queue.rs:139`, ~3), `PWelsDecoderContext`
   (`decoder_context.rs:1965`, ~12). Re-grep each; where every use is a
   fixture cast or a doc mention, delete the typedef and respell the fixtures
   (below); where a use is live in `api/`, **leave it** with a Phase 8 pointer
   at the definition — the typedef *is* the boundary's spelling.
3. **The test fixtures** (16 `&mut x as *mut T` casts in `pic_queue.rs`'s tests
   and siblings): these build the raw context shape the api still hands the
   decoder. Where the field they feed is now a `&mut`/owned type, respell the
   fixture to match; where the field is one of the api-owned pointers
   (`pParam`, `pLastDecPicInfo`, `pMemAlign` behind `api_alias`), leave the
   cast — it is the boundary, and Phase 8 dissolves it. Do not "fix" a fixture
   into a wrong shape to lower a count (S16 — the number is not the goal).
4. **The 20-typedef sweep's stragglers**: any `pub type P… = *mut/*const` in
   `src/decoder/` with zero live uses after step 2 — delete.
5. Done-test, by grep, code/prose split: every remaining `*mut`/`*const`
   spelling in `src/decoder/` is one of — (a) an api-owned field or its
   accessor, (b) `pfLog`/`_pLogCtx`'s C-callback shape (Phase 8's trace
   plumbing), (c) a pointer *value* passed to `common/`'s kernels or the safe
   pool's generic machinery, (d) a fixture feeding (a). **List the residue in
   the log by category with counts** — that list is Phase 8's inheritance,
   stated exactly.

## 3. Gates and close

Per commit: build both profiles + `--all-targets` + tests + ratchet + census.
Face 0 additionally runs corpus + conformance **before** its commit. Full battery
at `exit` level at close; F3 per S14 (step 0's hash shortcut: this session's
face 1 is decoder-only and should hash-acquit; face 0 changes behavior only on
paths no gate reaches, but check with a build, never a judgement). No perf span
(no hot-path change; D-perf-6). Close-out ≤ 20 lines: F56's disposition with
the run's figures, the residue list, §0's row, `phase6.md`'s starting numbers
refreshed if the decoder row moved. Hand-off: **Phase 6 session A** — its brief
is the steward's next item.

## 4. Non-goals

No conversions — nothing here changes a signature's safety. No encoder sites.
No `api/` internals. No golden movement (face 0's gate is that nothing moves).
No perf work. No deleting a typedef or fixture Phase 8 still needs — the
residue is enumerated, not zeroed.
