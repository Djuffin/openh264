# Session prompt — Phase 3, session F: T3.5 (reduced), T3.6, and the phase exit

> **SUPERSEDED — HISTORICAL.** Phase 3 completed 2026-08-11 (sessions A–F). This
> brief is kept as the record of what was asked, not as instructions. The phase's
> outcome is in [`../safety_refactor_log.md`](../../safety_refactor_log.md) (entries
> for sessions A–F) and plan §0; the next phase's brief is
> [`phase4b.md`](phase4b.md).

**Governing:** [`phase3.md`](phase3.md) §T3.5/T3.6 + §2, plan **§7.6 including the
new S20 (signature-reachability closure) and S21 (construction audit)** — both were
hoisted from this phase's own sessions and both are load-bearing today — and the
**session E log entry** (its §1 closure generalisation and its hand-off). This file
scopes the session and supersedes on disagreement; fix disagreements in place.

**Where the phase stands:** decoder read side fully safe-owned (T3.0–T3.3);
encoder CAVLC write side safe-owned and `SBitStringAux` deleted (T3.4).
**Remaining: T3.5, now smaller than the phase brief assumes** — its CAVLC half
landed with T3.4's closure, leaving the encoder CABAC triple, the two
`SHIM(phase3)` boundaries (`svc_encode_slice.rs:1865`, `nal_encap.rs:98` — located,
not survey-vintage), and `BsWriter::set_pos` (`safe/bits.rs:238`), which the
hand-off says disappear together — **then T3.6** (the owned output buffers, where
S21 meets real `Vec` fields), **then the phase exit, never compressed** (the
standing rule): if T3.6 runs long, the exit moves whole to a short session G.

Tree at session start: clean except inherited doc edits (the §7.6 S20/S21
additions) — commit them first, house style. Then the control battery
(`OVERALL:` is the verdict), ratchet `check`, recounts (431/425/20 at session E's
close; the recount rule has paid eleven times).

---

## 1. T3.5 — the encoder CABAC triple, the stash as a `Copy`, and the death of `set_pos`

### 1.0 Step 0: the write-extent audit — F16's procedure, applied to a *writer* with a backward walk

The CABAC encoder is the phase's last cursor triple
(`set_mb_syn_cabac.rs` — `m_pBufStart/m_pBufCur/m_pBufEnd`, walks at
~`:825`, `:839-860`, `:1009-1028`; survey-vintage, re-verify). Before converting,
enumerate **every write it can issue**, on paper, in the module docs — and this
engine has the one access shape nothing else in the phase had: **carry
propagation walks backward** from the cursor (`m_pBufCur[-1]`-shaped adjustments
into already-written bytes) until it finds a byte that isn't `0xFF`. The audit
must bound that walk explicitly (it terminates at the first non-`0xFF` or at the
slice buffer's start — say which guard the conversion relies on, in comparison
form; a `pos - 1` underflow is session C's wrap class from the write side).
Also enumerate: the forward byte emits, the terminate/flush sequence, and the
stash/pop pair's state capture. The claim "every write is bounded" names each
site individually — the F16 standard, unchanged.

### 1.1 The conversion

- The triple → `pos: usize` over the slice's output buffer (still the raw
  allocation this session — buffers are T3.6's; the writer reaches it the same
  way T3.4's CAVLC writer does). Same arithmetic, byte for byte; backward carry
  in comparison form per 1.0.
- **The stash becomes a `Copy`.** `StashMBStatusCabac`/`StashPopMBStatusCabac`
  (`svc_set_mb_syn_cavlc.rs:1018`, `:1042`) snapshot into
  `SDynamicSlicingStack` (`svc_encode_slice.rs:315`): the pointer-shaped
  snapshot fields become **by-value copies of the writer state** — restore is
  `*w = saved;`, which is the whole point of detached cursors, and it is why
  `BsWriter::set_pos` can then be deleted: its doc comment already argues it
  exists only as the transitional restore bridge. Verify the CAVLC stash pair
  (already converted in T3.4's closure — `:977`, `:997`) reads the same way
  afterwards; the two families should end symmetric.
- **Delete `set_pos` and re-run the mode-guard tests** — if anything still
  needs it after the stash conversion, that caller is a missed snapshot site,
  not a reason to keep the method.
- The two `SHIM(phase3)` boundary markers: `nal_encap.rs:98`'s comment says
  `BsWriter` is a position and the raw `pBsBuffer` allocations are the other
  side — that boundary *narrows* here (the CABAC writer joins the CAVLC one on
  the same convention) and *dies* in T3.6; `svc_encode_slice.rs:1865`'s is
  T3.6's outright. Do not delete either this face; re-word them if the CABAC
  landing changes what they guard.
- **MT discipline unchanged:** writer state is thread-private by the
  task-claiming invariant; `sm=3` sweeps exercise exactly this path, so expect
  F3-signature hits (thirteenth-plus measurements; the
  two-batteries-disagreed-on-one-tree fact is on record — a single battery's mt
  verdict is a sample) and apply S14 without flinching.

### 1.2 Face gate

Full battery; sweeps 341/341 both profiles are the referee (**the `cabac=1`
configurations are the ones this face can break**); decoder goldens must not
move at all; one interleaved pair both benches (encoder CABAC-config rows are
the signal; S1-proportional disassembly look at the put-bits + carry path —
the refill-inline lesson transfers); Miri; ratchet (the triple's pointers die).

## 2. T3.6 — the owned output buffers: S21's real test, with one deliberate exclusion

### 2.0 The scoping decision, made here so the session doesn't discover it mid-seam

The phase brief says "owned buffers". The inventory will show **not all of them
are ownable this phase**: `SWelsSliceBs.pBsBuffer` is an **alias in MT mode** —
`SetOneSliceBsBufferUnderMultithread` (`slice_multi_threading.rs:~933-944`,
survey-vintage, re-verify) binds it to `pThreadBsBuffer[kiThreadIdx]`, the
thread-claimed buffer pool that is P10/F12/F3 territory and fenced until Phases
6/7. **Scope T3.6 to the unambiguously singly-owned buffers** and leave the
MT-aliased slice buffer raw with its shell comment re-pointed at Phase 6:

- **Convert:** `SWelsEncoderOutput.{pBsBuffer, sNalList, pNalLen}` (the AU-level
  output — owned by `pOut`, allocated in `RequestMemorySvc`, freed in the
  cascade) → `Vec<u8>` / `Vec<SWelsNalRaw>` / `Vec<i32>`; `SWelsNalRaw.pRawData`
  → a range/slice into the output buffer **if** the inventory shows it is always
  a view into `pBsBuffer` (survey says NAL raws point into the output buffer —
  verify; if any site owns separately, convert that one to owned and say so).
  The corresponding `WelsUninitEncoderExt` free-cascade entries **die in the
  same commit** (R4 in miniature — the first encoder cascade entries to fall).
- **Exclude, with the reason written at the site:** `SWelsSliceBs.pBsBuffer` and
  the per-thread buffer machinery. `svc_encode_slice.rs:1865`'s marker retires
  only for the converted allocations; what guards the exclusion gets a fresh
  comment naming Phase 6.

### 2.1 S20 + S21, applied for real

- **Compute the closure first** (S20): flipping `SWelsEncoderOutput`'s fields
  changes its layout → every struct embedding it by value and every
  `encoder/abi_guard.rs` assert pinning any member of the closure trips
  together. Enumerate before sizing commits; delete the asserts in the same
  commit; the closure list goes in the commit message.
- **The construction audit** (S21) is now the real thing: these are `Vec`s
  going into structs reached through `WelsMallocz`'d allocations and the
  zeroed context. Every construction path of every closure member gets audited
  in the same commit — expect `RequestMemorySvc`'s `pOut` allocation to become
  a real constructor (heap-constructing, the decoder's `new_boxed` precedent),
  and expect the audit to be the majority of the face's diff. A zeroed `Vec`
  is UB *at a distance* — the incident that proved it was invisible to every
  test; treat any "it seems to work" as unmeasured, not as safe.
- Perf: buffer allocation moved from `CMemoryAlign` to `Vec` — once per
  encoder init, not hot; one pair both benches anyway (fresh null if a verdict
  needs the floor).

## 3. The phase exit (T3.6's second half — or session G whole; never compressed)

1. **Straggler sweep** (S18 + corollary): every `SBitStringAux`-era name, every
   `SHIM(phase3)` marker, every writer/reader entry point — converted,
   deleted-dead, or listed with its owner phase. `SHIM(phase3)` should end
   **zero or enumerated**: the surviving shells (`SDqLayer`'s retyped pointer,
   the MT slice buffer exclusion) are Phase 5/6 debt with comments naming them,
   not markers.
2. **Full perf protocol** (the one place it still runs): 3-pair interleaved
   medians both benches vs the phase-entry baseline (`b308f7d5`'s numbers are
   in the log); Phase 3 column appended to `perf_baseline.md`; ledger
   reconciled — decode cumulative should read ≈ T3.1b's −1% better than phase
   entry, encoder ≈ +8.9% unchanged; any surprise gets the one-hour look, then
   ledgered per D-perf-4.
3. **Miri, the widened gate, and the differential retirements complete** — the
   Phase 1 differential files' raw sides are gone; what remains of them is
   properties and golden tables; say the final Miri count and what it covers.
4. **T3.0's standing**: 2316+ rows, unmoved this session, dual-profile — restate
   it in the log as the phase's permanent instrument (it goes on gating Phases
   5 and 8).
5. **Bookkeeping**: §0 refreshed (Phase 3 complete; next = Phase 4b per
   D-seq-1's order); Progress appendix checkboxes with hashes; the phase's
   findings file reconciled (F15–F17 closed states, the F2 annotation);
   auto-memory if the store exists.
6. **S19 — write `prompts/phase4b.md`**, the hand-off artifact. Its contents,
   staged from what this phase and 4a left: the config-dispatch enums now
   unblocked by the writer being one (`pfWelsSpatialWriteMbSyn` CAVLC/CABAC →
   `EntropyCoder` enum; the RC-mode table), the two strategy vtables
   (`ParamsetStrategy` 16 entries, `RefStrategy` 7 → traits), `IWelsVP`'s
   inlining, and **4a's named unfinished business** (the decoder intra-pred
   *mode* tables, `sBlockFunc`, the expand fn-pointer re-wraps, the
   `decode_slice.rs` cache-fill transmutes — `transmute` still 23 — and the
   encoder's ~55 CPU-dispatch members including `WelsInitSampleSadFunc`, whose
   deletion unblocks nothing but cleans the F13-adjacent struct). Cite
   S20/S21 by tag; carry the parked families' status (re-attempt at caller
   conversion, 6.3); estimated 1–2 sessions. Stamp this brief and the phase's
   session briefs superseded-historical.

## 4. Non-goals

No thread-buffer or pool work (the MT alias exclusion is the fence, F12/P10
are Phase 6/7's). No `pfWelsSpatialWriteMbSyn`/enum work — it is 4b's *next
session*, and starting it early tangles the exit. No Phase 5 pulls
(`SDqLayer`, NAL lists, `struct_bytes_eq`). No fixing F8/F9/F11 (S6). No
golden movement of any kind — the phase's last golden change was F15's, and it
stays that way. No `get_unchecked` (S8). No new stored extents (the T3.3
standard covers the new `Vec` owners: lengths are `buf.len()`, not fields).
And the exit is not compressible: if the clock says choose between finishing
T3.6 and doing the exit properly, T3.6 finishes, the exit becomes session G,
and the hand-off says so — a phase that ends with "nothing explained away"
twice in a row is worth one more session boundary.
