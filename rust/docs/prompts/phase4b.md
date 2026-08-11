# Phase 4b, session C — §3's tail, then the phase exit

> **This is the rewritten phase brief** (2026-08-11, after session B closed §2 and
> started §3). The session-B-era text is in git history (`87a89d31` vintage) and the
> session-A-era text before that (`9f88d768`). Seam numbering: **T4b.1, T4b.1b, T4b.2a,
> T4b.2b and T4b.3a are done** (`08b7c29d`, `3e583b9a`, `d6c78c1b`, `be67a754`,
> `33b1f0f3`). This session is **T4b.3 items 3–5** and the **phase exit**.

**Governing:** [`safety_refactor_plan.md`](../safety_refactor_plan.md) — §0 for where
the port stands, **§7.6 for S1–S24** (S20/S21 govern every struct edit; **S24 is
session B's and it is aimed squarely at this brief**), §7.4 for D-perf-4. Read the
**session-B log entry** in full before converting anything — its §1 (two wrong
premises), §2 (F19) and §4 (the aliasing hazard) are this brief's method, demonstrated.
This file scopes the session and supersedes on disagreement; fix disagreements in place.

**S24 applies to every number below.** Session B's brief carried two scouted counts and
both were wrong, each read out of a comment. The counts here were measured on
2026-08-11 at `33b1f0f3` — **re-grep them anyway, in the session that acts on them.**

---

## 0. Start here

Tree at session start: clean, or carrying a docs-only tail. Commit any tail first,
house style.

Then the control battery (`bash rust/tools/gates.sh full` **from the repo root** —
the paths inside are relative and it fails loudly from elsewhere; `OVERALL:` is the
verdict) and recount — **435 debug / 429 release / 20 ignored**, Miri **295** at
session B's close. The recount rule has now paid fourteen times.

**F3, per S23b as session B sharpened it.** Signature unchanged (`mt`, `sm=3`,
`t∈{2,4}`, wrong-length output of any form). What session B added is a **zero**: eight
341-configuration sweeps across four batteries, no hit, ~2728 configurations against a
measured ~1-in-800 rate. That is not evidence the race is gone — it is evidence about
*load*:

* A session-start hit is a **tendency**, not a rule. Two sessions running have opened
  clean. Expect nothing; apply S14 to what arrives.
* Single hit → re-run that configuration. Two → escalate.
* **Escalation alternates whole `mt` presets run back to back**, both binaries built
  once and swapped inside one loop, nothing else on the machine. Session A got 9 hits
  that way and 0/80 from isolated configurations; session B got 0 from eight sweeps
  that were *interleaved with builds and benches*. **Both the unit and the density
  matter.** A sweep per side inside a `gates.sh` run is not an alternation.
* Calibration: **~1 in 800 configurations under sustained load.** In twelve sweeps per
  side expect a handful on both sides; the question is whether HEAD is *worse*.

---

## 1. T4b.3 items 3–5 — the tail of 4a's leftovers

Session B took items 1 and 2, which turned out to be **one seam** (the intra-pred
constraint family), and left these. Descending value, filler last, and **one dispatch
family per commit** so a byte-exactness failure names its cause.

### 1a. The expand fn-pointer re-wraps — and the crate's last two `transmute` calls

**Do this one first.** `decoder_core.rs:893-894` passes `pfExpandLuma` and
`pfExpandChroma` through `std::mem::transmute` into a function expecting different
fn-pointer types. These are **the only two `transmute` calls left in the crate**
(`transmute` reads 5 in the ratchet; the other three are prose — two pre-existing in
`encoder_context.rs`, one in `IntraPredConstraint`'s doc).

Session B's finding tells you what to expect: a `transmute` around a function pointer
is **not a technique, it is the symptom of a slot type that does not match its
contents.** The fix is to correct the type, not to wrap the cast more carefully. Read
both sides before choosing: the `SExpandPicFunc` members, and the signatures of what is
actually stored.

**If these two go, the crate's real `transmute` count is zero** and the remaining
ratchet reading is entirely prose. Say so explicitly in the commit and in §3's
bookkeeping — a metric that reads 3 with zero calls behind it needs its floor
documented, or the next phase will chase it.

### 1b. `sBlockFunc` — the deblocking block-dispatch pair

`SWelsDecoderContext::sBlockFunc` (`decoder_context.rs`, near the other kernel
sub-structs). Scout it against S24 before deciding shape: **count the members, count
the implementors, and read the installer.** If it is set as a unit from one condition
it is T4b.1's enum case; if the members are chosen independently it is not, and it may
be CPU dispatch and therefore §1c's kind.

### 1c. Filler, only at a seam boundary with time left

The encoder's ~55 CPU-dispatch members that select between identical `_c` functions,
including `WelsInitSampleSadFunc` (deleting it unblocks nothing but cleans the
F13-adjacent struct). Low value, low risk; **do not let it displace 1a or 1b, and do
not start it if the exit is not already safe.**

### Face gates (each seam separately)

Full battery; sweeps 341/341 both profiles are the referee; **decoder goldens are the
referee for anything in `decoder_*`** — 1a and 1b both are; one interleaved pair both
benches per seam at **3 pairs, not 1** (S2b; session B ran 3 from the start on all
three seams and it cost minutes); Miri; ratchet regenerated per S16 with the deltas
named.

---

## 2. The phase exit — never compressed

If §1 runs long it stops at a seam boundary and **the exit gets session D**. Do not
compress it; that instruction has now survived two rewrites of this brief.

1. **Straggler sweep (S18)**: every vtable-era name (`pVtbl`, `…Vtbl`, `Create…Strategy`
   / `Destroy…Strategy` as factories-of-vtables), every `Option<fn>` slot in the
   converted families, every `pf*` member this phase touched — converted, deleted-dead,
   or listed with its owner phase. **The two `SHIM(phase3)` markers are Phase 6's and
   should still be exactly 2.** Note that sessions B's conversions leave *doc-comment*
   mentions of deleted vtable types on purpose (they are the C++ mapping, P14); the
   sweep must distinguish a name in prose from a definition.
2. **Full perf protocol**: 3-pair interleaved medians both benches, whole phase — entry
   is **`6e15c907`**. **S2b is in force**: a median outside the null band gets more
   pairs before it gets a mechanism; two pair-counts disagreeing in sign means the
   effect is below the floor. Ledger per D-perf-4; cumulative should still read
   ≈ +8.9% encoder, ≈ +17.8/+10.1/+9.6% decode. **Expect flat** — all five seams so far
   measured flat, for the reason 4a established (runtime-selected arms recover nothing),
   and the phase's return is in the ratchet instead.
3. **Bookkeeping**: §0 refreshed (Phase 4b complete; next = Phase 5 per D-seq-1);
   Progress appendix checkboxes with hashes; findings reconciled (F3's running totals;
   F19/F20 are closed and stay closed; anything new per S12). **Quote the phase's real
   ledger, which is not the size assert**: `SWelsFuncPtrList` 1272 → 1184 and then
   still 1184 through three more seams, against `raw_ptr` 5001 → 4834, `unsafe_fn`
   1286 → 1259, `transmute` 23 → 5. Say in §0 why the assert stopped moving.
4. **S19 — write `prompts/phase5.md`**, the hand-off for the plan's first pivot. Its
   staged contents, all measured facts on record: the 5.1–5.6 order from plan §5 with
   the exit-gate annotations added at Phase 3's exit (T3.0's goldens gate the phase;
   T7 stays deferred); **S20 computed first for `SDqLayer`** (plan 5.2 note) and the
   `MaybeUninit` shell fact for `decoder_core.rs` (plan 5.5 note — the decoder context
   embeds its buffers by value, `decoder_context.rs` has the existing shell);
   `SDqLayer::pBitStringAux` (`*mut BsReader`) retires at 5.2 and `cabac_decoder.rs`'s
   `SHIM(phase5)` accessor with it; the decoder's `SHIM(phase2)` kernel adapters retire
   as 5.2–5.4 convert their callers; the downgraded decode ledger rows are the phase's
   perf debt to collect (BaseMC dimensions become static); whatever `transmute` count
   survives §1a and **how much of it is prose**. Two things session B earned that
   Phase 5 needs by name: **S24** (re-grep every shape-deciding count) and **the
   aliasing rule** — converting a raw pointer to a borrow does not introduce an
   aliasing question, it surfaces one that was already there, and Phase 5 is made of
   those conversions. Estimated 9–12 sessions — say per-session scope is the S20
   closure, not the file. Stamp this brief superseded-historical.

---

## 3. Non-goals

No Phase 5 or 6 pulls: `SDqLayer`, `SMbCache`, the picture pool, `SSlice` layout, the
thread buffer pool, `SWelsSliceBs` — shells and comments instead. No re-opening the
parked families (second dated verdict; 6.3's third attempt is at caller conversion).
**No "fixing" the RC lag** — `eInstalledMode`'s divergence from `iRCMode` is upstream
behaviour preserved on purpose (S23/S6); a test pinning the lag is welcome if cheap,
otherwise it is Phase 6.5's, with `rc.rs`. No fixing F8/F9/F11-class arithmetic (S6).
No `get_unchecked`, ever (S8). No golden movement. No pool/threading edits (F12/P10).
And the exit is not compressible: if the clock says choose, §1 finishes at a seam
boundary, the exit becomes session D, and the hand-off says so.
