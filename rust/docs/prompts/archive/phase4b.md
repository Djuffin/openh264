# Phase 4b, session C — §3's tail, then the phase exit

> **SUPERSEDED-HISTORICAL (2026-08-11).** Phase 4b is complete. §1a landed as
> **T4b.3b** (`d1c1a7d4`) and §1b as **T4b.3c** (`f2e3c5af`); §1c (the CPU-dispatch
> filler) was deliberately not started, exactly as this brief instructed when the exit
> was the alternative. The exit ran whole. The successor is
> [`phase5.md`](../phase5.md); the session record is the 2026-08-11 session C entry in
> [`safety_refactor_log.md`](../../safety_refactor_log.md).
>
> **Two of this brief's own scouted premises were wrong, and it warned about both in
> advance.** §1a predicted that some erased fn-pointer types would lack `extern "C"`
> (they are all the same type — the transmutes reinterpreted a type into itself), and
> §1b's own correction of its predecessor still under-described `sBlockFunc`: three
> members, yes, but *declared twice*, bridged by a double cast that the `transmute`
> metric cannot see. **S24 is now three-for-three at catching this brief's family of
> mistakes**, which is the argument for keeping it.
>
> *(Original header follows.)*
>
> **This is the rewritten phase brief** (2026-08-11, after session B closed §2 and
> started §3; expanded by the steward the same day, every count re-verified at
> `50212e09`). The session-B-era text is in git history (`87a89d31` vintage) and the
> session-A-era text before that (`9f88d768`). Seam numbering: **T4b.1, T4b.1b, T4b.2a,
> T4b.2b and T4b.3a are done** (`08b7c29d`, `3e583b9a`, `d6c78c1b`, `be67a754`,
> `33b1f0f3`). This session is **T4b.3 items 3–5** and the **phase exit**.

**Governing:** [`safety_refactor_plan.md`](../../safety_refactor_plan.md) — §0 for where
the port stands, **§7.6 for S1–S25** (S20/S21 govern every struct edit; **S24 and
S25 are session B's, and S24 is aimed squarely at this brief**), §7.4 for D-perf-4.
Read the **session-B log entry** in full before converting anything — its §1 (two wrong
premises), §2 (F19), §3 (what the S20 closure caught that field-types would not) and
§4 (the aliasing hazard, now **S25**) are this brief's method, demonstrated. This file
scopes the session and supersedes on disagreement; fix disagreements in place.

**S24 applies to every number below, including the ones this revision just measured.**
Session B's brief carried two scouted counts and both were wrong, each read out of a
comment. The counts here were re-greped on 2026-08-11 at `50212e09` — **re-grep them
anyway, in the session that acts on them.** One of them (§1b's) *corrects the previous
revision of this very brief*, which called a three-member struct "the deblocking
block-dispatch pair" — both words wrong, inherited from Phase 4a's hand-off prose.

---

## 0. Start here

Tree at session start: clean **except a seven-edit steward doc tail across three
files** — the session-B log entry's T4b.3a hash corrected (`489b95d5` was the
pre-amend orphan; `33b1f0f3` is on the branch), plan §0's F3 count brought to nineteen
with the revised session-start advice, **S25 hoisted into §7.6** (the aliasing rule,
from the session-B log §4) with the standing-rules row updated, an F19-class
annotation on plan §Phase 6.6, and this brief's two S25 references. **Commit the tail
first, house style.**

Then the control battery — `bash rust/tools/gates.sh full` **from the repo root** (the
paths inside are relative and it fails loudly from elsewhere); `OVERALL:` is the
verdict — and recount: **435 debug / 429 release / 20 ignored**, Miri **295**, at
session B's close. The recount rule has now paid fourteen times.

**F3, per S23b as session B sharpened it.** Signature unchanged (`mt`, `sm=3`,
`t∈{2,4}`, wrong-length output of any form — zero, short, and long are one signature).
The current calibration and protocol:

* **Rate: ~1 in 800 configurations under *sustained* load** (session A, measured by
  alternating whole sweeps). Session B then ran eight 341-configuration sweeps across
  four batteries and drew **zero** — ~2728 configurations, ~3.4 expected, P(0) ≈ 3%.
  Not a contradiction; a sharpening: session A's hits came from sweeps run
  *back to back* in a loop, and a `gates.sh` battery interleaves its sweeps with
  builds and benches. **The load is part of the signature — both the unit and the
  density matter.**
* A session-start hit is a **tendency, not a rule** — the last two sessions opened
  clean. Expect nothing; apply S14 to what arrives.
* Single hit → re-run that configuration (5×). A *different* wrong length from the
  same binary and configuration is race evidence, not divergence — a deterministic
  port bug repeats its bytes. Two hits → escalate.
* **Escalation alternates whole `mt` presets run back to back**, both binaries built
  once and swapped inside one loop, nothing else on the machine. Twelve sweeps per
  side is the session-A precedent; expect a handful of hits **on both sides** at the
  measured rate — the question an alternation answers is whether HEAD is *worse*, not
  whether hits exist. Isolated-configuration alternation is a non-result (session A:
  0/80 in isolation on a day the sweeps produced 9), and **a 0/0 alternation has not
  run yet** (S23b). A sweep per side inside a `gates.sh` run is not an alternation.

---

## 1. T4b.3 items 3–5 — the tail of 4a's leftovers

Session B took items 1 and 2, which turned out to be **one seam** (the 19
`decode_slice.rs` transmutes were all one dispatch family — `IntraPredConstraint`
now), and left these three. Descending value, filler last, and **one dispatch family
per commit** so a byte-exactness failure names its cause.

### 1a. The expand fn-pointer re-wraps — and the crate's last two `transmute` calls

**Do this one first.** The full mechanics, verified at `50212e09`:

* `decoder_core.rs:880-896` is a **forwarding wrapper** named `ExpandReferencingPicture`
  that takes `pfExpandLuma: Option<unsafe extern "C" fn(*mut u8, i32, i32, i32)>` and
  `pfExpandChroma: [Option<…>; 2]` (same inline type), then calls
  **`error_concealment::ExpandReferencingPicture`** — a *different function with the
  same name* — passing both through `std::mem::transmute` (`:893`, `:894`). Note the
  second call transmutes an entire `[Option<fn>; 2]` **array**. These are **the only
  two real `transmute` calls left in the crate** (the ratchet reads 5; the other three
  are prose — `encoder_context.rs:709`, `:1116`, and `decode_slice.rs:833`, the
  deleted family's tombstone comment).
* The slots' *declared* home is `SExpandPicFunc` (`common/expand_pic.rs:94`):
  `pfExpandLumaPicture: Option<PExpandPictureFunc>`,
  `pfExpandChromaPicture: [Option<PExpandPictureFunc>; 2]`, installed by
  `InitExpandPictureFunc` (ports `expand_pic.cpp:351`). So at least **three
  fn-pointer types are in play for one family**: `PExpandPictureFunc`, the wrapper's
  inline `extern "C"` type, and whatever `error_concealment`'s callee declares —
  session B's finding says two of the erased ones have **no `extern "C"` at all**.

Session B's result tells you what this is: **a `transmute` around a function pointer
is not a technique, it is the symptom of a slot type that does not match its
contents.** The wrapper exists *because* the types disagree — fix the disagreement
and the wrapper loses its reason to exist. Read all three sides before choosing, then
take the strongest conversion the facts license, in this order:

1. **If `InitExpandPictureFunc` installs identical `_c` functions regardless of CPU
   flag** (the 4a shape — re-grep, do not assume), the slots become **direct calls**:
   `SExpandPicFunc` dies, the wrapper dies, both transmutes die, and the callers name
   `ExpandPictureLuma_c`/`ExpandPictureChromaUnalign_c`-class functions directly.
   **Caution, S18:** `expand_pic.rs` is `common/` — serving both codecs — so
   "converted" is a per-consumer claim; sweep the *encoder's* uses of the struct too
   before deleting it, and if one codec's callers cannot convert this session, shim
   per R7 rather than leaving two half-states.
2. **If real dispatch survives**, unify on one typedef (`PExpandPictureFunc`) end to
   end so the wrapper forwards without transmuting, and say in the commit why full
   de-virtualization was not licensed.

**Either way the crate's real `transmute` count becomes zero, and that needs its
floor documented**: the ratchet will still read 3 on prose alone. Update §0's ratchet
row and the metric's mention in the exit bookkeeping to say "3, all prose, zero
calls" — a metric that reads 3 with nothing behind it will otherwise be chased by
Phase 5. (Precedent: session B already had to discover that 2 of the "23" could not
move; do not make the next reader re-derive that.)

### 1b. `sBlockFunc` — three members, not a pair, and not deblocking

**The previous revision of this brief called this "the deblocking block-dispatch
pair" — both wrong**, inherited from 4a's hand-off. Measured at `50212e09`:
`SBlockFunc` (`decoder_context.rs:299`) has **three** members —

| member | type |
|---|---|
| `pWelsSetNonZeroCountFunc` | `PWelsNonZeroCountFunc` |
| `pWelsBlockZero16x16Func` | `PWelsBlockZeroFunc` |
| `pWelsBlockZero8x8Func` | `PWelsBlockZeroFunc` |

— reconstruction helpers (NZC bookkeeping and block zeroing), embedded by value as
`SWelsDecoderContext.sBlockFunc` (`decoder_context.rs:714`), one confirmed reader at
`decode_slice.rs:2111` (`if let Some(nzc_func) = …`). Scout per S24 before choosing
shape:

* **Find the installer** (grep for `sBlockFunc` *assignments*; expect a
  `decoder_core.rs` init function). If the three slots are set as a unit from the CPU
  flag with identical `_c` variants, this is the 4a shape — **direct calls, struct
  deleted**. If any slot is config-selected, that slot is T4b.1's enum shape instead.
  Count the variants per slot; do not assume the three answer alike (S23's lesson:
  two fields of one struct answered opposite ways).
* The struct derives **`Copy, Clone`** — session B's §3 found the same derives on
  `SWelsFuncPtrList` had been licensing a silent second owner. Whatever shape wins,
  check who copies `sBlockFunc` by value before deleting the derives with the struct.
* S25 note: these convert to direct calls or an enum, not to borrows, so no
  re-entrancy audit is expected here — but if any conversion turns a `*mut` argument
  into `&mut` along the way, the audit is part of that commit.

### 1c. Filler, only at a seam boundary with time left

`SWelsFuncPtrList` still has **62 `pub pf` members** (re-grep; 66 at 4b's entry, −4 at
T4b.1). Roughly **55 are CPU-dispatch members that select between identical `_c`
functions** — the survey's estimate, never re-measured, S24 applies. Named target if
time allows: `WelsInitSampleSadFunc` (its deletion unblocks nothing but cleans the
F13-adjacent struct). Each deletion moves `assert_size!(SWelsFuncPtrList)` down from
**1184** and its comment records why — that comment is the phase's dispatch ledger.
**Low value, low risk; do not let it displace 1a or 1b, and do not start it if the
exit is not already safe.**

### Face gates (each seam separately)

Full battery; sweeps 341/341 both profiles are the referee; **decoder goldens are the
referee for anything in `decoder_*`** — 1a and 1b both are (1a's expand path runs
under error concealment, which the T3.0 malformed-stream table exercises directly);
one interleaved pair both benches per seam at **3 pairs, not 1** (S2b; session B ran
3 from the start on all three seams and it cost minutes); Miri; ratchet regenerated
per S16 with the deltas named per commit.

---

## 2. The phase exit — never compressed

If §1 runs long it stops at a seam boundary and **the exit gets session D**. Do not
compress it; that instruction has now survived two rewrites of this brief.

1. **Straggler sweep (S18)**: every vtable-era name (`pVtbl`, `…Vtbl`,
   `Create…Strategy` / `Destroy…Strategy` as factories-of-vtables), every
   `Option<fn>` slot in the converted families, every `pf*` member this phase
   touched — converted, deleted-dead, or listed with its owner phase. **The two
   `SHIM(phase3)` markers are Phase 6's and should still be exactly 2.** Session B's
   conversions deliberately leave *doc-comment* mentions of deleted vtable types
   (the C++ mapping, P14) — the sweep must distinguish a name in prose from a
   definition, the same distinction the transmute floor needs.
2. **Full perf protocol**: 3-pair interleaved medians both benches, whole phase —
   entry is **`6e15c907`**. **S2b is in force**: a median outside the null band gets
   more pairs before it gets a mechanism; two pair-counts disagreeing in sign means
   the effect is below the floor, and the disposition is diagnostic-only. Ledger per
   D-perf-4; cumulative should still read ≈ +8.9% encoder, ≈ +17.8/+10.1/+9.6%
   decode. **Expect flat** — all five seams so far measured flat, for the reason 4a
   established (runtime-selected arms recover nothing); this phase's return is in the
   ratchet, not the benches.
3. **Bookkeeping**: §0 refreshed (Phase 4b complete; next = Phase 5 per D-seq-1);
   Progress appendix checkboxes with hashes; findings reconciled (F3's running
   totals; **F19/F20 are closed and stay closed**; anything new per S12). **Quote the
   phase's real ledger, which is not the size assert**: `SWelsFuncPtrList` 1272 →
   1184 and then unmoved through three more seams, against `raw_ptr` 5001 → 4834 →
   (final), `unsafe_fn` 1286 → 1259 → (final), `transmute` 23 → 5 → (final, with the
   prose floor stated). Say in §0 why the assert stopped moving (`Option<Box<_>>` is
   pointer-sized) so the next phase reads the right instrument.
4. **S19 — write `prompts/phase5.md`**, the hand-off for the plan's first pivot. Its
   staged contents, all measured facts on record: the 5.1–5.6 order from plan §5 with
   the exit-gate annotations added at Phase 3's exit (T3.0's 2316-row golden table
   gates the phase; T7 stays deferred, the golden corpus is the standing
   approximation); **S20 computed first for `SDqLayer`** (plan 5.2 note) and the
   `MaybeUninit` shell fact for `decoder_core.rs` (plan 5.5 note — the decoder
   context embeds its buffers **by value**, `decoder_context.rs:769` is the existing
   shell, unlike the encoder's pointer-reached `pOut`); `SDqLayer::pBitStringAux`
   (`*mut BsReader`) retires at 5.2 and `cabac_decoder.rs`'s `SHIM(phase5)` accessor
   with it; the decoder's `SHIM(phase2)` kernel adapters retire as 5.2–5.4 convert
   their callers; the downgraded decode ledger rows are the phase's perf debt to
   collect (BaseMC dimensions become static; ≈ 7 points of CB headroom under the
   tripwire is Phase 5's to spend); whatever `transmute` reading survives §1a and
   **how much of it is prose**. Three things this phase earned that Phase 5 needs by
   name: **S24** (re-grep every shape-deciding count), **S25** (a raw-to-borrow
   conversion surfaces pre-existing aliasing — the re-entrancy audit is enumerated
   with the S20 closure, not discovered at compile time; Phase 5 is *made of* those
   conversions), and **F19's class** (every `Box::into_raw` whose `from_raw` lives on
   no live path is a leak wearing a destructor — Phase 6.6 runs the check
   encoder-side; Phase 5.5's constructor work runs it decoder-side). Estimated 9–12
   sessions — say per-session scope is the S20 closure, not the file. Stamp this
   brief superseded-historical.

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
