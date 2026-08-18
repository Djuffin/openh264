# Phase 6 — the encoder structural rewrite, and what Phase 5 hands it

Written at Phase 5's exit (2026-08-17, session AC). Rules: plan §7.6; per-session
scope is the S20 closure; **S24 at every face open** — every size below was taken
at `HEAD` on the day this was written and none of them is a settlement.

Plan §4's step list (**6.1**–**6.6**) is unchanged and is not restated here. What
this file carries is the **playbook Phase 5 paid for** and the **inheritance**,
because the playbook is what turned the decoder from "16 modules, 439 `unsafe fn`,
977 raw-pointer occurrences" into a tree where the lint is on every file.

## 0. What Phase 5 delivered, as the starting position

| | at Phase 5's open | at its exit | **after Phase 5b** |
|---|---|---|---|
| decoder `raw_ptr` | 1283 | **236** (173 code + 63 prose) | **113** (47 code + 66 prose) |
| decoder `unsafe fn` | ~1400 | **42** | **0** |
| decoder `unsafe {` blocks | — | 160 | **6** |
| decoder `#[allow(unsafe_code)]` items | — | 167 | **3** |
| decoder modules with `#![deny(unsafe_code)]` | **0** | **22 of 22** | 22 of 22 |
| corpus (output / codes) | 641 agree / 541 differ on truncation rows | **2707 / 0** | unmoved |

**Phase 5b (2026-08-17/18, sessions A–D, closed at `00a702ed`) closed all five
enumerated families and then F56**, and `src/decoder/` now carries **three**
`#[allow(unsafe_code)]` items and no `unsafe fn` at all. **The 47 code occurrences
that remain are enumerated by category and they are Phase 8's, not this phase's**
(T5b.9's done-test): **16** api-owned context fields and their accessors, **6** the
log callback's C shape, **10** values crossing the C ABI or the pool's own machinery
(`ppDst`, `PPicture`/`PPicBuff`, `slot_ptr_mut`, `data_ptr`), **15** unit-test
fixtures feeding the first group. The other **66** are prose — comment text
recording what a pointer used to be, which is S16's floor and cannot fall.

Four of Phase 5b's results are this phase's to reuse:

1. **F42's method, for 6.1/6.2.** The encoder's picture pool meets the same shape in
   `MarkPicAsRef`: **a safe container can hand back the identity of the slot it lends
   out for free**, and a caller that already holds the borrow supplies the rest. The
   address was what the raw pointer was protecting, not the aliasing.
2. **S34/F55, for every index this phase introduces.** An alias converted to an index
   is faithful only while the container never reorders — grep the container for
   `swap`, `rotate`, `retain`, `remove`, `sort`, `drain` before converting. It cost
   session A a whole face and eleven conformance assets.
3. **The zeroed shells' recipe, and its measured negative.** `SSps::default()`/
   `SPps::default()` are *not* all-zero, so "replace the shell with `Default`" is a
   wrong recipe; what works is a `memset_zero` beside the `Default`, each field's zero
   *meaning* written out, and a temporary byte-comparison test against the shell it
   replaces to prove the equivalence rather than argue it. `size_of` first: the
   decoder context measured 573,576 bytes at T5b.5 against a doc that said "several
   MiB" (572,784 after T5b.9 deleted a dead pointer field from `SSps`).
4. **F56 is closed, and it is this class's trap.** `Option<T>` over a type with a
   `bool` or small-enum niche is **valid at all-zero and reads as `Some(default)`**,
   where a raw pointer's zero was `None`. The encoder's structs will meet it the
   moment their aliases become ids. What Phase 5b learned closing it is the part to
   carry: **no gate this project owns can see the difference** — the corpus and the
   conformance set were byte-identical either way, because on every stream both
   values take the same arm — so the instrument is the **byte comparison against the
   zero image, with every differing offset attributed by `offset_of!`**, and the
   decision is a ruling rather than a default (the ladder's rung 3). T5b.8 ran both:
   573,574 of 573,576 bytes identical, the two exceptions named, corpus 2690/17 +
   2707/0 and conformance 60/60 unmoved.

The encoder is where the decoder was. Measured at Phase 5's exit:

| directory | files | `raw_ptr` | `unsafe fn` | deny-clean |
|---|---|---|---|---|
| `src/encoder/` | 32 | **2534** | **671** | 0 |
| `src/common/` | 11 | **237** | **72** | 0 |
| `src/api/` | 3 | **240** | **48** | 0 (exempt until Phase 8) |

*(Re-measured at Phase 5b's close. `common/` lost `DeblockingInit` and `InitMcFunc`,
which took `*mut T` and a null test and now take `&mut T`; `api/` gained the three
aliasing probes' vtable driver, which belongs there — see the log entry for T5b.6.
S24 still applies: re-grep at every face open.)*

Largest first: `wels_preprocess` 251/53, `svc_encode_slice` 251/64,
`svc_mode_decision` 245/61, `encoder_ext` 217/40, `svc_base_layer_md` 177/27,
`svc_motion_estimate` 165/39, `wels_encoder_ext` 139/27, `rc` 102/60,
`deblocking` 99/27, `md` 87/17.

**`src/safe/` is the asset**: `plane.rs` (`PaddedPlane`, `PlaneCursor`,
`PlaneCursorMut`), `mb_grid.rs` (`MbArray`, `MbDims`), `pool.rs` (`Pool`, `Id`),
`bits.rs`, `prng.rs`, `err.rs`. Every one of them was written in Phase 2 for the
decoder and every one is what the decoder's conversions landed *onto*. The encoder
needs no new vocabulary types before it starts; it needs its callers to take them.

## 1. The playbook, in the order it is used

**The ordering rule, and it is predictive.** *A raw alias into a container blocks
that container's ownership; aliases become ids/indices before their container
owns.* Session P used it forwards three times and needed no probe to make any of
the three calls. **Ask the rule before scheduling a conversion, not after Miri
does.** For 6.1 this says the reference lists become `PicId` before
`wels_preprocess`'s pool can own (6.2), which is the order plan §4 already has —
confirm it, do not re-derive it.

**Cache, not carrier.** The single highest-yield question to ask of any raw field:
*is this a copy of something the holder can already reach?* In Phase 5 the answer
was yes for the layer's `pDec` (T5.P′1 — one stamp site, deleted rather than
converted), `pCurDqLayer` (T5.R1 — 85 sites), `DqLayerState::pPred`, the context's
`iDecBlockOffsetArray`, and the layer's four slice-header aliases (T5.AC2 — the
layer had already copied the whole header one statement earlier). **A cache dies;
a carrier converts. Deciding which costs one grep and saves a face.**

**Take what you reach.** A callee stops taking a raw pointer by taking the thing it
actually touches — three context fields, one `bool`, one 24-byte record. Session W
converted five families this way *before* the view struct existed, and that is why
the view struct was last rather than first.

**The bracket maneuver — five applications, all the same shape.** One function
borrows a container once at a scope's top and hands back the disjoint pieces, so
the compiler owns the disjointness instead of the author: the pool at Q
(`cur_and_rest`), the slice at Y (`slice_split`), the layer at AA1, error
concealment's frame copy at AB3 (`pic_and_refs_mut` + `PicRefs::classify`), and the
EC prefetch at AC5. **The tell that you need it**: two derivations from one object
with a *guard between them* — under owned storage the second invalidates the first
before the guard can run. When you see `same_picture(a, b)` or an address
comparison, the answer is usually a `classify`-shaped enum, not a comparison.

**The unit is the pointer family, not the module** (session W's finding, and it
held for every session after). The layer, `SWelsNeighAvail`, the reference set, the
MV pair, the intra-pred array, the nzc record, `PPicture`, the api-owned aliases,
the coefficient cursors — each converted across every module at once, and the same
four aliasing shapes recurred: a reading the first time, a recognition afterwards.
**Schedule families; report modules.**

**The vestigial-keyword sweep, run whole** (session Z's rule, confirmed at AB5
after a one-module run contradicted a previous session's): strip `unsafe` from
*every* `fn` in the tree at once and let the compiler keep the ones that need it.
AB5 stripped 176 and 125 came back — a per-module estimate had said none would go.
**And session Z's clause is the one that survives: a signature that names a raw
pointer keeps the keyword; a body that merely contains an unsafe operation gets a
block.** T5.AC11 got this backwards on its first pass and the ratchet caught it —
`unsafe_fn` rose by 18 where it should have fallen. Re-run under the rule it fell
by 47.

**Strip-and-build is the enumerator.** Put `#![deny(unsafe_code)]` on a module,
strip the keyword, and the compiler prints the exact list of what is left, grouped
by item. It is faster than any grep and it cannot miss. Every module Phase 5 closed
was closed this way.

**Settlements in writing, before the first edit.** W2b, W3 and W6 were each settled
by reading the tree and writing the settlement down, and each executed in one pass
afterwards. Two of the three settlements were also *wrong in a measurable clause*,
which is the other half of the lesson: **S24 at the face, aimed at our own
documents too.** W6's settled census was wrong the day it was written; session Z's
brief and `phase5.md` both said `slice_split` was in the tree when it was in a
reverted branch.

**The decision ladder** (session AC's, and it is why that session closed):
1. **Settle by reading** — read the code, write the settlement, proceed. This
   closed every design question of Phase 5's second half.
2. **Lint-scope questions** (where a keyword or an allow lives) default to the
   **enumerated-survivor shape** with a Phase pointer. Always legal, never a
   behaviour change.
3. **Behaviour questions** (output, codes, API-visible anything) **never default**:
   park exactly that item behind an item-level allow with a one-paragraph record,
   and continue with everything else.

**S31**: context is a working buffer, not the session's memory. State lives on
disk; compact and continue; a per-face breadcrumb in the log so a stop at any point
costs nothing to make honest.

**S32/S33**: probe count is the Miri budget knob and probe *coverage* is the value
— each probe pays a multi-MiB `Initialize` under the interpreter, flat against
decoded work, so shortening a stream saves nothing. And a recorded outcome is a
measurement, never a prediction: a gate result, a golden value or a
red-under-revert claim is written only after the command ran.

## 2. The referee suite — D-par-1's standing inheritance

Phase 5 spent three sessions (S, T, U) discovering that **the port's own previous
output is not a reference**: F43, F44, F45 and F46 each lived a full life behind a
green gate. The instruments that closed that hole are standing and Phase 6 uses
them from its first commit:

* **`tools/ecref`** — the C++ decoder's answer, replicating the harness exactly,
  with `--stdin` so every corpus row has a referee (389 had none before session U).
* **`tools/diffharness`** — `sweep.sh st|mt|qp|def`, both profiles, byte-exact
  against the C++ **encoder**. This is Phase 6's primary gate and it is already the
  right instrument: 341 configurations, and the encoder is what it drives.
* **`tools/find_shadowing_stubs.py`** — F43's class made mechanical (*a stub
  shadowing a real implementation is invisible to every name-matching tool*). It
  reads 19 candidates today and **F52's six encoder-side shadowing-stub candidates
  are Phase 6's to adjudicate** (§3).
* **`tools/find_unwritten_fields.py`** — F45's class (a field read but never
  written).
* **`tools/find_elem_byte_confusion.py`** — F40's class, 0 suspects over 81 files,
  proved against the pre-fix tree.
* **`tools/census.sh`** + `census_allowlist.txt` — 59 allowlisted duplicates, of
  which **the encoder owns most of the (b) and 6.3 lines**; the allowlist names the
  step for each.
* **Miri**, `gates.sh full`/`exit` — and the encoder's probe coverage is *thinner
  than the decoder's was*. The `--lib` run skips `wels_thread_pool` (F12) and
  `encoder_ext` (F13); both skips are a work queue and **F13's is 6.6's**.

**Gate rhythm**: `gates.sh commit` per commit, `family` per seam (both sweeps),
`full`/`exit` at the close. Do not edit the working tree while a battery runs.

## 3. The inheritance, item by item

**F52 — six encoder-side shadowing-stub candidates.** `find_shadowing_stubs.py`'s
remaining set after Phase 5 resolved the decoder's. Each is a name defined in two
modules where a caller's own module hosts one locally; the decoder's instance
(`CheckAccessUnitBoundaryExt`) returned `true` unconditionally and shadowed an
80-line implementation with no caller at all. **Adjudicate all six before
converting anything they touch** — a conversion that lands on the shadow rather
than the body is invisible.

**F12 / P10 — the encoder's pool and thread machinery.** Phase 5's non-goal
throughout, and **Phase 7's**, not 6's. 6.4's slice structures border it; the
boundary is that Phase 6 converts *slice state*, Phase 7 rewrites *claiming*.

**F3 — the encoder MT race.** Sixty-four measurements, twenty alternations,
thirty-seven acquittals. Signature: `mt`, `sm=3`, `t∈{2,4}`, output of **any**
wrong length, either profile, either clip; rate ≈1/480–1/800 under a battery,
≈1/307 sustained. **It closes in Phase 7 one way or the other** (plan §4), and
Phase 6 only follows S14: step 0's hash shortcut first, then re-runs, then the
alternation. Two clauses this phase should carry:

* *"Decoder-only" is not the trigger; "test-only" is* — the driver links the whole
  lib. **But it is profile-dependent** (measurement 64): in release, `rust_enc`'s
  unreachable decoder code is eliminated, so a decoder-side change can hash
  identically where the debug build does not. **Hash the profile the hit occurred
  in.** For Phase 6 this cuts the other way and hard: every encoder change moves
  the binary, so the shortcut will essentially never apply.
* Append every measurement to F3 **at adjudication time, not at session close**.

**F13 — `InitDqLayers` holds a `&mut` into `sSpatialLayers` across an aliasing
use.** The Miri skip on `encoder_ext` names it; 6.6 owns it, and deleting the skip
is part of fixing it (S15 — no skip may be added without a finding).

**The parked SIMD-adjacent families** — `common/sad_common.rs` (14) and
`encoder/sample.rs`'s SATD (7), twice parked on dated verdicts (1.41x–4.94x against
a ≤1.05x bar). Plan §4 puts the **third attempt in 6.3**, because caller conversion
is what changes their calling convention. SATD still owes a measurement of its own
before any verdict.

**`SWelsSliceBs.pBsBuffer` and `SWelsNalRaw.pRawData`** — Phase 3's two deliberate
exclusions, inherited by 6.4. The port's last two `SHIM(phase3)` markers guard the
first and retire when thread-buffer ownership is redesigned.

**The three copy/expand kernels now in `common/`** — `copy_mb.rs`,
`expand_pic.rs`, `mc.rs`. The decoder reaches them safely
(`SPicture::expand_as_reference`, `copy_16x16`/`copy_8x8` on plane cursors); **the
encoder's two `ExpandReferencingPicture` sites and its seven `WelsCopy*_c` shims
still take the raw entry points**, and converting them is 6.2/6.3's. The safe
kernels are already there — this is caller work, not kernel work.

**`PCopyFunc` x2** — `encode_mb_aux.rs:200` and `md.rs:228`, one typedef declared
twice. F22's class, allowlisted, Phase 6's. The decoder's third copy is deleted
(T5.AC10).

## 4. Non-goals

No decoder work — Phase 5 is closed and its exceptions are enumerated with Phase
pointers (`phase5.md` §"Phase exit conditions"). No `api/` rewire (Phase 8's,
including F23, F41, the api-owned context aliases and the `api/` instrument
inventory). No threading rewrite (Phase 7's). No `get_unchecked` (S8). No window
hoisting (S8 #4). No golden movement — output equivalence per commit against the
C++ is the correctness definition (D-fid-1), and the encoder's is the diffharness.

**Perf**: D-perf-6 stands. Phase 5 exits with cumulative CB ≈ **+23.7…+24.3%**
against a ≈+23% stop-line — breached by ≈0.7…1.3 points, with D-perf-4's +25%
*median* tripwire unbreached — and recovery deferred to the **Phase 9** perf pass.
Phase 6 measures its own spans per D-gate-1 and S1/S2/S2b (interleaved pairs, a
null per session at the verdict's own pair count, a second day for any reading a
decision rests on) and **does not** open recovery work. 6.3 is the hot path; watch
the bench budget per seam, because that is where the decoder's cost came from.
