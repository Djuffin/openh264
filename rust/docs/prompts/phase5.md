# Phase 5 — the decoder structural rewrite, session A

> **This is the phase brief**, written at Phase 4b's exit (2026-08-11) per S19. It is
> the first *pivot* phase: Phases 2–4 changed how code is reached, Phase 5 changes what
> owns memory. Every count below was greped at `f2e3c5af`. **S24 applies to all of
> them — re-grep in the session that acts on them.** Phase 4b earned that rule by
> having two scouted premises fail in one brief, and then a third fail in its successor.

**Governing:** [`safety_refactor_plan.md`](../safety_refactor_plan.md) — §0 for where
the port stands, **§5 for the 5.1–5.6 order**, §7.6 for S1–S25, §7.4 for the deficit
ledger this phase is expected to clear. Read Phase 4b's three log entries before
starting; the two that matter most are session B's (F19, and why an ownership audit
finds what no gate can) and session C's (F21, and why a duplicate family is a
divergence that has not happened yet). This file scopes the session and supersedes on
disagreement; fix disagreements in place.

---

## 0. Start here

Tree at session start: clean at Phase 4b's exit. Run the control battery —
`bash rust/tools/gates.sh full` **from the repo root** — and recount: **435 debug /
429 release / 20 ignored**, Miri **295**, sweeps 341/341 both profiles. The recount
rule has now paid fifteen times.

**F3, per S23b and session C's sharpening.** Signature: `mt`, `sm=3`, **`n=600`**,
`t∈{2,4}`, wrong-length output of any form (zero, short and long are one signature).

* **`n=600`, not `sm=3` generally** — session C's nine hits were all the tighter byte
  constraint, never `n=1500`. Per-susceptible-configuration rate ≈ **1 in 100–150**;
  ≈ 1 in 800 over all configurations. The two agree once divided by the susceptible
  fraction (24 of an `mt` sweep's 120).
* A session-start hit is a **tendency, not a rule** — three of the last six.
* Single hit → re-run that configuration 5×. **A reproduction in isolation is
  evidence**, not the non-result session A took it for; the variable is recent load,
  not isolation, and session C reproduced 1-in-5 straight after a battery. Run it
  before the expensive alternation, not instead of it.
* Two hits → escalate. **Escalation alternates whole `mt` presets back to back**,
  binaries built once and swapped inside one loop, nothing else on the machine. Twelve
  per side. Session C's alternation eight is the worked example and the first that was
  not 0/0: base 2/12, head 3/12, and the *head* side hit one configuration twice with
  two different wrong lengths — this finding's own race criterion, met on the tree
  under suspicion. **That is the strongest available acquittal**; prefer it to a rate
  comparison when you can get it.
* Twenty measurements, eight alternations, eight acquittals. Nobody has looked for the
  cause yet. It is a *multithreaded encoder* race; Phase 5 is the decoder, so this
  phase should expect to keep acquitting rather than fixing.

---

## 1. What this phase is, and the unit of work

**Per-session scope is the S20 closure, not the file.** This is the single most
important line in this brief. Phase 3's T3.4 proved that a signature change drags
every reachable struct into one commit; Phase 4b confirmed it twice more. Compute the
closure **first**, write it down, and let it size the commit — a step of plan §5 is
often two or three commits, and `5.2` is likely four.

Three rules Phase 4b earned that this phase is *made of*:

* **S24 — a count that decides a conversion's shape comes from a `grep` over the
  definitions, in the session that acts on it.** Not from a brief, a hand-off, a module
  doc, or a prior session's scouting. Phase 4b broke this three times and caught it
  three times; the third was this brief's own predecessor describing a three-member
  struct as a "pair".
* **S25 — converting a raw pointer to a borrow does not introduce an aliasing
  question, it surfaces one that was already there.** `WelsWriteParameterSets` held a
  `*mut` across calls that re-enter the same object through `pCtx->pFuncList`. As raw
  pointers, invisible; as `&mut`, UB that will not compile. **Phase 5 is nothing but
  raw-to-borrow conversions**, so the re-entrancy audit — *who else can reach this
  object while I hold it?* — is enumerated **with** the S20 closure, as planned work.
  Fix shape: re-acquire through one helper at each use, no borrow outliving one
  expression.
* **F19's class — every `Box::into_raw` whose `from_raw` lives on no live path is a
  leak wearing a destructor.** No gate in this project can see one: not byte-exactness,
  not the sweeps, not Miri `--lib`. 5.5's constructor work runs this check decoder-side
  (Phase 6.6 runs it encoder-side). The question is *"which line frees this?"*, asked
  per allocation, and it is only answerable once the field has a type that can hold an
  owner.

**Estimated 9–12 sessions.**

---

## 2. The order, with what is already known about each step

Plan §5's order stands. What follows is the per-step intelligence on record, so no
session re-derives it.

### 5.1 Picture & DPB
`Picture` re-struct (PaddedPlanes + `Vec`s), `PicPool`, `pic_queue.rs` recycling
predicate, `manage_dec_ref.rs` (near-mechanical per the survey), `error_concealment.rs`
identity sites. **P3 regression tests first.**

Note from 4b session C: `manage_dec_ref.rs` and `error_concealment.rs` each just lost
a duplicate `ExpandReferencingPicture`, and the decoder's plane arrays are `[_; 4]`
against the encoder's `[_; 3]` (`decoder/picture.rs:102-105`,
`encoder/picture.rs:70-71`). The fourth entry is never read by anything in the expand
family; check whether it is read by anything at all before `PaddedPlanes` fixes the
count at three.

### 5.2 MbGrid — the big one
Kill the `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with an owned
`MbGrid`; re-point `parse_mb_syn_*` cache fills (~40 mechanical signatures, their
30-entry scratch caches becoming `&mut` locals).

* **S20 computed first, and expect it to be large**: `SDqLayer` is embedded and
  asserted widely.
* `SDqLayer::pBitStringAux` (`*mut BsReader`) retires here, and `cabac_decoder.rs`'s
  **`SHIM(phase5)`** accessor — the crate's only one — dies with it.
* The decoder's **`SHIM(phase2)` kernel adapters** retire as 5.2–5.4 convert their
  callers. There are **154** `SHIM(phase2)` markers crate-wide; they are the bulk of
  the `SHIM(` metric and this phase is where the decoder's share goes.

### 5.3 Neighbor & MV machinery
`mv_pred.rs` (punning → byte ops, `SetRectBlock` → typed generic on the grid),
colocated reads via `cur_and_ref`.

### 5.4 Deblocking driver
`decoder/deblocking.rs`: `SDeblockingFilter` holds `PicId`s + plane cursors built
per-MB; identity compare per P3.

### 5.5 `decoder_core.rs`
Allocation → constructors (`AllocPicture` dies into `Picture::new`), paramset store
(P4), context decomposition per §2.2.6, `Drop` teardown, `Default` derives.

* **S21 applies with force.** Unlike the encoder's `pOut`, which is pointer-reached,
  the decoder context embeds its buffers **by value**, so every owned field lands
  inside `mem::zeroed` reach. The `MaybeUninit` shell + `new_boxed()` already exists at
  **`decoder_context.rs:769`** (T3.3); this step extends it per owned field and
  eventually replaces the shell with a real constructor.
* **This is where F19's check runs decoder-side.**
* Two members left this struct in Phase 4b (`sExpandPicFunc` at T4b.3b, `sBlockFunc` at
  T4b.3c) and **nothing moved**, because `SWelsDecoderContext` has no `assert_size!`
  and no offset pins. Do not go looking for an instrument that is not there.

### 5.6 `decode_slice.rs` last
Per P1, including the EC MC paths; delete the last decoder shims; decoder modules get
`#![deny(unsafe_code)]` one by one.

**There is no `SBitStringAux` shell to delete** — the type died outright at T3.4.

---

## 3. The exit gate, and the perf debt this phase is expected to collect

Battery, with **special attention to frame-count parity and the `#[ignore]` set**.
T3.0's **2316-row malformed-stream golden table over 12 files** gates this phase in
both profiles — T7 (fuzzing) stays deferred by direction, and the golden corpus is the
standing approximation. Decoder `src/` is unsafe-free at the end.

**Every §7.4 deficit-ledger entry whose shims died in this phase must clear.** And this
is the phase that collects Phase 4a's **downgraded decode rows**: direct dispatch
recovered nothing there because `BaseMC` passes runtime dimensions, and 5.x is what
makes those dimensions static. The debt is ≈ **+17.8 / +10.1 / +9.6% cumulative
decode**; the ≈ **7 points of CB headroom under the tripwire** is this phase's to spend.

**Do not expect the benches to reward the structural work step by step.** Phase 4b ran
five de-virtualization seams and measured flat on all five, for the reason 4a
established: a runtime-selected arm has no per-call scaffolding to recover. This
phase's wins are supposed to come from *constant dimensions reaching the kernels*, not
from removing indirection — so a flat reading mid-phase is expected, and the ledger
entries are the instrument that will move.

**S2b is in force throughout**: 3 interleaved pairs minimum, a median outside the null
band gets more pairs before it gets a mechanism, and two pair-counts disagreeing in
sign means the effect is below the floor.

---

## 4. The metric situation you are inheriting

Read these before quoting any number.

* **`transmute` is 4, all prose, zero calls.** The crate contains no `mem::transmute`
  call at all as of T4b.3b. Every ratchet match is a comment: two pre-existing in
  `encoder_context.rs`, T4b.3a's tombstone in `decode_slice.rs`, T4b.3b's note in
  `decoder_core.rs`. **Do not chase this metric.** It has a floor and the floor is
  prose.
* **The metric never covered the whole population.** T4b.3c found
  `&mut (*pCtx).sBlockFunc as *mut _ as *mut _` doing exactly what a `transmute` did —
  laundering one type into an identical one that a second declaration had made
  incompatible — and a double cast does not match the grep. If you want to find the
  rest of that family, grep for `as *mut _ as *mut` and for duplicate struct
  definitions (`rust/tools/find_dup_types.sh`), not for `transmute`.
* **`assert_size!(SWelsFuncPtrList)` is not the dispatch ledger it looks like.** It
  read 1184 across three seams that deleted two vtables, 25 thunks and 19 transmutes,
  because `Option<Box<_>>` is pointer-sized. It moved to **1160** only when T4b.3b
  deleted an embedded 24-byte struct. Size measures bytes of members; it cannot see a
  vtable leave. The **ratchet** is the instrument for that.
* Phase 4b's real ledger: `raw_ptr` **5001 → 4815**, `unsafe_fn` **1286 → 1250**,
  `transmute` **23 → 4 (0 calls)**, `SHIM(` 157 with `SHIM(phase3)` still exactly **2**.

---

## 5. Non-goals

No Phase 6 pulls: `SMbCache`, `SSlice` layout, the encoder's free cascade,
`wels_encoder_ext.rs` internals. No re-opening the parked SAD/SATD families (third
attempt is 6.3's, at caller conversion). No fixing F8/F9/F11-class arithmetic (S6). No
`get_unchecked`, ever (S8). No golden movement — including the narrow-frame asset F21
would need; that is a deliberate, separately-justified act. No pool/threading edits
(F12/P10), and **no attempt on F3**: it is an encoder race and this is the decoder.

One thing that *is* welcome if cheap: `pfSetNZCZero`
(`encoder/wels_func_ptr_def.rs:385`) is the last live slot of the NZC dispatch family
T4b.3c closed decoder-side — one slot, one unconditional constant, and deleting it
takes `SWelsFuncPtrList` to 1152 and removes the last reason
`encoder/deblocking.rs`'s duplicate `WelsNonZeroCount_c` exists. It is encoder-side, so
it is 6.5's by rights; it is listed here because it is six lines and its owner is
otherwise ten sessions away.
