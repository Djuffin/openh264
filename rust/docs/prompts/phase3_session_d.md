# Session prompt — Phase 3, session D: T3.3, the ownership seam (`RawDataBuffer`, `nalu.rs` ranges, `ExpandBsBuffer` dies, F15 closes)

> **SUPERSEDED — HISTORICAL.** Phase 3 completed 2026-08-11 (sessions A–F). This
> brief is kept as the record of what was asked, not as instructions. The phase's
> outcome is in [`../safety_refactor_log.md`](../safety_refactor_log.md) (entries
> for sessions A–F) and plan §0; the next phase's brief is
> [`phase4b.md`](phase4b.md).

**Governing:** [`phase3.md`](phase3.md) §T3.3 + §2 (shells, fallback ladder), plan
§7.6, the [P3] notes in plan §2.2.2, and — read in full —
[`phase3_findings.md`](../phase3_findings.md) **F15 and F16** plus the session B and
C log entries. This file scopes the session and supersedes on disagreement; fix
disagreements in place.

**Session goal:** seam T3.3 whole. It is one seam with four faces — the owned
buffer, the deletion of `ExpandBsBuffer` (and with it F16's stale-`avail`
instance), the NAL payload ranges, and the F15 fix with its golden un-withholding —
and they land in that order because each face's proof depends on the previous one.
T3.4 (the encoder write side) is next session's; its dedupe-first discipline
deserves a fresh start, and this seam ends with the phase's decoder read side
**fully converted** — worth an unhurried close.

Tree at session start: clean at `d737a450`. The battery is trustworthy now
(`OVERALL:` line, exit-code-matching, proven red-capable at `eae61b94`) — open
with a control run and the ratchet `check`; recount tests (~433/427-ish/20 — the
engine tests moved the totals; the recount rule has paid nine times).

---

## 1. The design north star: derive, don't store

F16 existed because an extent was *stored* and went stale; session C's audit
closed T3.2 cleanly because the engine *derives no extent at all* — it takes an
exact `rbsp_window()` slice and owns nothing. T3.3 is where the last stored
extents die. The rule for everything this session builds:

> **Every readable extent is derived from the owning buffer at window-creation
> time. Nothing stores a length that the buffer can outgrow.**

If a design sketch this session has a field caching a size, redesign until it
doesn't. The one stored thing is the owned `Vec` itself — everything else is a
computation on it. This is what makes the F16 class *unrepresentable*, which is
the standard the seam is judged by.

## 2. Face 1 — `SDataBuffer` → `RawDataBuffer`

`SWelsDecoderContext` carries two `SDataBuffer`s (`sRawData`, `sSavedData` — the
saved one services the new-sequence stash; both convert, same type):

```rust
/// Owns the accumulated bitstream. All positions are offsets; all windows are
/// derived at call time (§1). Replaces SDataBuffer {pHead,pEnd,pStartPos,pCurPos}.
pub struct RawDataBuffer {
    buf: Vec<u8>,        // the allocation; capacity policy preserved from C (2 MiB init)
    cur: usize,          // was pCurPos − pHead
}
```

*(Fixed in place at session D per this file's header: the sketch originally
stored `start` and `end` too. `end` was `pEnd − pHead` — a stored copy of the
allocation size, i.e. exactly the class §1 forbids; it is `buf.len()`. `start`
was `pStartPos − pHead`, and the survey found `pStartPos` write-only in this
port — the upstream parse-only rewind that reads it was never carried — so
there is nothing to store. And the C init size is 2 MiB
(`MIN_ACCESS_UNIT_CAPACITY * MAX_BUFFERED_NUM` = 256 KiB × 8), not 4 MiB.)*

Design points, each with its referee:

- **The slop semantics are load-bearing and subtle — preserve them, don't
  redesign them.** T3.1a/F16 established: the reader's slack bytes are *real
  neighboring stream bytes* (they feed decoded values on malformed input;
  zeroing was rejected on evidence). In the owned buffer that property holds by
  construction for every NAL except the last — the bytes past one NAL's RBSP are
  the next NAL's. For the **last** NAL, the raw code read into the 4 MiB
  allocation's tail (in-bounds garbage); the converted reader already handles
  short slack through `avail` accounting and checked reads to the error path
  (the F16 fix). So: `readable` for a window = `buf.len() − window_start`
  (initialized bytes only — **never** capacity), and the 2316 goldens are the
  referee that the accounting is exact. If any non-F15 row moves, the accounting
  is wrong; stop and compare against the raw reader per session B's
  disambiguation test.
- **`rbsp_window()` becomes a method on `RawDataBuffer`** (or its thin
  equivalent), and T3.1's boundary bridge — the `SHIM(phase3)`
  `BsReader`-from-raw-parts helper in `bit_stream.rs` (~`:136-145`) and
  `readable_from` — **is deleted in this seam**. That deletion is the
  headline ratchet event: grep for every caller first, bucket them, and make
  the bridge's removal its own commit so the diff reads as "the last raw
  reconstruction dies".
- **Offset arithmetic in comparison form.** Session C's rule, now general: a
  `usize` subtraction in bounds math wraps into a *new* OOB — write predicates
  as comparisons (`cur < end`, `end - cur` only under a proven `cur <= end`
  guard or via `checked_sub` routed to the error path). T3.3 is wall-to-wall
  distance math (`pEnd − pCurPos` everywhere); every conversion site chooses
  comparison form deliberately.
- Consumers to bucket in the inventory (S18's corollary): `decoder_core.rs`'s
  fill/init path (`InitBsBuffer`, `CheckBsBuffer`-shaped code, the
  `WelsDecodeBs` loop ~`:3620-4200`), the EPB/copy path that appends into the
  buffer, `sSavedData` swap sites, and every `pHead/pEnd/pStartPos/pCurPos`
  read anywhere (`grep -rn "sRawData\|sSavedData"` and bucket every hit into:
  converts now / shells for Phase 5 / dies with `ExpandBsBuffer`).

## 3. Face 2 — `ExpandBsBuffer` is deleted, and staleness dies with it

- The pointer-rebasing block (`decoder_core.rs` ~`:1758-1842`) is **deleted, not
  converted** — growth is `Vec` growth and offsets survive by definition. The
  function's remaining useful content (the growth-trigger policy: when and by
  how much) moves into `RawDataBuffer`'s grow method with the same thresholds.
- **F16's second instance closes here**: the stale-`avail` hazard existed
  because a stored extent survived a growth. Under §1's rule there is nothing
  left to go stale — say exactly that in the commit message, and close the
  instance in the findings file.
- **Write the P5 test the plan has asked for since rev 1**: grow the buffer
  mid-AU and assert parse continuity — feed a stream in fragments sized to force
  growth between NALs of one access unit, assert identical output to the
  unfragmented feed. This was the untestable case under pointers (the latent-bug
  class offsets fix silently); it becomes a permanent unit test now, and it is
  the proof face 2 is done.

## 4. Face 3 — `nalu.rs` payload ranges

Scope precisely (the seam converts *payload identity*, not NAL bookkeeping
structure):

- `SVclNal`'s payload pointer (~`:265`) and every NAL-payload
  pointer-into-`sRawData` → `Range<usize>` (or `start+len` pair — pick one form
  and use it everywhere). The NAL *list* structures (`SAccessUnit`,
  `pNalUnitsList`, `ExpandNalUnitList`'s growable array) are **Phase 5's** —
  they keep their shapes; only the payload fields inside them retype.
- `DetectStartCodePrefix` and `DecodeNalHeaderExt` take slices —
  `DecodeNalHeaderExt` today has **no length parameter at all** (survey fact;
  re-verify); it gains one by construction, and its callers pass the range's
  window.
- `struct_bytes_eq` (the padding-sensitive SPS/PPS memcmp) is **untouched** —
  Phase 5's P12.

## 5. Face 4 — F15 closes, and the goldens finally move (exactly the `WITHHELD` rows)

*(Fixed in place at session D per this file's header: this heading said
"exactly 13 rows" — that number is **F16's** (13 corpus entries hit the CAVLC
prime narrowing), conflated here. F15's own text says ~11 per stream, and the
golden tables actually hold **105** `WITHHELD` rows across 12 files. The
referee is unchanged: the regeneration diff must show exactly the rows that
today say `WITHHELD` gaining outcomes, and zero other movement.)*

- **Re-grep for the expression shape before fixing** — the finding names
  `nalu.rs:762`, the survey knew three `BsGetTrailingBits(pNal.add(iNalSize−1))`
  shaped sites (~`:675`, `:762`, `:1000`), and upstream C++ has it twice; fix
  **every** site the grep finds, not the count anyone remembers.
- The fix defines behavior where the profiles currently disagree (debug aborts,
  release does OOB arithmetic that happens to land on the header byte):
  `iNalSize == 0` takes the graceful error path. **Both profiles must now
  agree** — that agreement is what turns the 13 `WITHHELD` rows into recorded
  outcomes.
- **The un-withholding protocol, exactly as specified at T3.3's spec in
  `phase3.md`**: fix first, then `UPDATE_MALFORMED_GOLDEN=1` regeneration, and
  the diff must show **precisely the F15-named rows gaining outcomes and zero
  other movement**. That diff goes in the commit message as the fix's proof.
  Session C's near-miss preserved this diff's cleanliness deliberately (the
  reverted-unlanded corpus extension) — honor it: **no other golden change may
  ride along**. If this session discovers corpus gaps, extend *after* the F15
  commit, separately, additions-only, with its own additive-proof diff.

## 6. Rules in force, sharpened by session C

- **Comparison-form predicates** (§2 above) — the seam-wide arithmetic rule.
- **Capture-immune verification for coverage claims** (S17's third instance,
  now as method): any "does the corpus reach X" or "is this path exercised"
  measurement taken under libtest must use a channel test capture cannot
  swallow (the session C tracing approach) — a zero from a captured channel is
  a statement about the instrument until proven otherwise.
- **S1 proportionally**: this seam's hot-ish path is the fill/EPB copy, not a
  per-bin loop — one disassembly look at the converted copy path (memcpy stays
  memcpy, no per-byte checks) is enough; T3.2's full protocol is not required.
  Perf: one interleaved pair both benches at seam close; expect neutral (CB is
  the sensitive row; the parse loop's bit work didn't change). Fresh null if
  any verdict needs the floor.
- **F3/S14**: battery-stopped hits are now normal (tenth measurement precedent);
  signature per R-g as twice-broadened; alternation in a worktree if elevated.
- Commit discipline: four faces ≈ four-to-six commits (bridge deletion and F15
  regeneration deserve their own), each gated, house style.

## 7. Gates for the seam

Control battery at open; full battery per face where a face is behavior-affecting
(faces 1–2: goldens **unmoved**, 53 conformance hashes, frame counts; face 4:
goldens moved by exactly the F15 diff); the P5 growth test in-tree and green;
Miri — the bridge deletion removes the last `from_raw_parts` in the read path,
so the differential files' raw side shrinks: retire what dies into properties
(phase brief §2's retirement rule) and expect the Miri count to move, recorded;
ratchet — `raw_ptr` takes its biggest Phase 3 drop (two `SDataBuffer`s = 8
pointer fields, the NAL payload pointers, the bridge, `ExpandBsBuffer`'s
arithmetic), `SHIM(phase3)` count *falls* for the first time (the bridge was
one); log entry with the bucketed inventory, the two findings-file closures
(F15 resolved, F16 instance 2 closed), Progress checkbox, and the hand-off.

## 8. Non-goals

No encoder-side work (T3.4's dedupe-first discipline starts fresh next session).
No NAL-list/`SAccessUnit` restructuring, no `struct_bytes_eq` change, no
slice-header or `SDqLayer` work (Phase 5). No corpus changes outside §5's
protocol. No capacity-based `readable` (initialized bytes only). No stored
extents (§1 — the standard, not a preference). No `get_unchecked` (S8). No
re-opening T3.2's engine or T3.1's cursor — if a face reveals a defect in them,
that's a finding with the session B disambiguation test applied, not an
in-place rework. And the close: when the seam gates green, the decoder's entire
read side — cursor, engine, buffer, NAL identity — is safe-owned; write the log
entry like the milestone it is, and hand off T3.4 with F2's dedupe map and F5
re-read as its first action.
