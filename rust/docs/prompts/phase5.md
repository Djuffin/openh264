# Phase 5 — decoder structural rewrite: the remainder, as a closed checklist

Re-planned 2026-08-13 under **D-fid-1** (structural fidelity to the C++ retired;
output equivalence — goldens, sweeps, conformance — remains the correctness
definition) and **D-gate-1** (sprint gating). Rules: plan §7.6; per-session scope
is the S20 closure; S24 at every face open. History: sessions A–O's record is the
log and git — this file carries only what remains.

**The anti-circles contract.** This checklist is **closed**: work enters it only
via an F-finding or Eugene. Every session's commits map to W-items in its log
entry. The progress metric is monotone and grep-able — **decoder `raw_ptr`
occurrences 1229** (1283 at the re-plan, 1267 at session P′'s open), → ~0 at exit
(excluding the named
survivor and prose); a session that closes no W-item and moves no metric is a stall
and says so — **and a W-item can close while the metric stands still**, which is
what W2a did (85 sites, `raw_ptr` +0): two pointer fields became handles and four
accessor signatures were born, and the instrument counts pointer *types written*
(S16). Sizes below were re-measured at `eef8a90b` on 2026-08-13; re-grep at
the face. **And a size carries the directory it was taken in** (session P §0): the
re-plan's `pRefPic` and `pRefList` figures were taken at *different* scopes and said
so nowhere — together they put **228 sites of `src/encoder/`** inside W2, which
F12/P10 puts outside this phase entirely.

## The blocker class, named once

*A raw alias into a container blocks that container's ownership.* Everything
below W1 was sequenced by this one fact (T5.N1, T5.O7). The ordering rule:
**aliases become ids/indices before their container owns.** W2 is the key; it
unblocks W3–W5 and half of W6.

**Session P used the rule forwards, and it is predictive.** W1 was available
because T5.O4 had already given the NAL nodes their own allocations; W2a was
available because `PicPool` never owned the pictures its slots address; W2b's layer
copy is not available because its readers cannot reach the container at all. Three
calls made from the rule, no probe needed to make any of them, and all three probe
runs green on the first attempt. **Ask the rule before scheduling a conversion, not
after Miri does.**

## The checklist

| # | item | size (measured) | done when | unblocks |
|---|---|---|---|---|
| ~~W1~~ | ~~`pAccessUnitList` → `Option<Box<SAccessUnit>>`~~ — **DONE, T5.P1** (`a3b68334`) | 54 sites | — | W3's first cascade entry, taken |
| ~~W2a~~ | ~~the context's two pool aliases: `pDec`/`pECRefPic` → `PicId`~~ — **DONE, T5.P2** (`eef8a90b`) | 85 sites | — | `dec_pic`/`ec_ref_pic` are now the **only two sites** W3 has to convert |
| ~~W2b~~ | ~~the layer's `pDec` + the reference lists~~ — **DONE, T5.P′1 + T5.P′2** (`59dbbb0b`, `48355825`). The layer's `pDec` was a **cache** and died; `pRef` was dead. The lists became `Option<PicId>`. **302 real sites, not 438**: `pRefPic`'s 113 were one identifier naming two types (S24's new clause) | 160 layer `.pDec` + 142 list field accesses | — | **every raw alias into `PicPool` is gone** — W3 is unblocked |
| W3 | ownership cascade — **the flip is DONE; the tail is not**. ~~seam 1, T5.P′3~~ (`6758d885`): `SPicture` finishes owning itself, so `AllocPicture`'s `Box` is a *complete* owner. ~~seam 2, T5.P″1~~ (`acf5bfd1`): `pTempDec` and `pPicBuff` are owned fields, `alloc_picture` is the constructor, `CreatePicBuff`/`DestroyPicBuff` shrink, `PrefetchPic` deleted. ~~seam 3, T5.P″2 + T5.P″3~~ (`d7a1d130`, `b07e407c`): **the hoist** — 79 per-use resolutions become two derivations at three slice bracket tops, threaded through 30 functions and the per-macroblock dispatch type. ~~seam 4, T5.Q1 + T5.Q2~~ (`539212bd`, `a8eaa3e5`): **the flip** — `Pool<Option<Box<SPicture>>>`, the resolver family split shared/`_mut` so the compiler enumerates the writers, five brackets where a span really crossed, **F42** found and fixed with a red-under-revert test, **F19 closed decoder-side**, **R4 by construction**, F37's reset kept beside a `drop`. **Remaining**: `pDqLayersList` owned + `pCurDqLayer`'s deletion (**81 mid-tree ctx-field reads**, re-grepped at `a8eaa3e5`); `fmo.rs`'s map (25 sites, one file); the parser buffers (3 `WelsMalloczHelper` calls, all under `bParseOnly`, and `SParserBsInfo` is a public API type) | `pDqLayersList` + `fmo.rs`'s map + the parser buffers | zero `WelsMallocz`/`WelsFree` in `src/decoder/` (**8 call sites left, none a picture's**); cascade functions deleted; probe green per container | 5.5 closes |
| W4 | colocated + 5.3b: `GetColocatedMb` on `cur_and_ref`; `SetRectBlock`/`CopyRectBlock4Cols` on the grid; punning → byte ops | 325 `LD*/ST*` tokens remaining | decoder `LD32\|ST32\|LD16\|ST16\|LD64\|ST64` grep reads 0 | `mv_pred.rs` deny-ready |
| W5 | P4: `pSps`/`pPps` → active-paramset ids + lookup | 205 field occurrences (131 + 74), 4 carriers | `.pSps\|.pPps` greps read 0; no lookup borrow outlives its expression (F41's mistake, not repeated) | context sheds 2 raw fields |
| W6 | 5.6: `decode_slice.rs` per P1 — EC MC paths, the NZC `*mut u8` cache family (~167 uses, re-grep), F31's memset, the signature leg (**D-fid-1: functions may merge — the 148-function count is an upper bound, not a target**), `cabac_rbsp_window` retirement | the phase's largest file | `decode_slice.rs` compiles under `#![deny(unsafe_code)]` | W7 |
| W7 | closure of the instruments: 5.2's straggler sweep; **F40-class sweep crate-wide** (element-vs-byte in copies); decoder `SHIM(phase2)` 48 + `SHIM(phase5)` 3 → **1 named survivor** (`data_ptr`'s output-contract consumer, Phase 8's); `deny(unsafe_code)` on every decoder module, exceptions enumerated | sweeps + deletions | SHIM greps match the survivor list exactly; every decoder module deny-clean or on the exception list with its Phase 8 pointer | the exit |
| W8 | **the exit — never compressed**: all D-gate-1-deferred measurement (session N's stashed binaries, the niche verdict, the ≈+23% stop-line, the recovery/ledger adjudication vs ≈+21.6–21.9% cumulative CB), 3-pair+day-two protocol, §0 refresh, `prompts/phase6.md` (S19), briefs stamped historical | one session | `OVERALL: PASS` at `exit` level; ledger reconciled or escalated with the table; phase6.md exists | Phase 6 |

## Session mapping

**P** = W1 + W2a — **spent** (`a3b68334`, `eef8a90b`). **P′** = W2b + W3's first seam —
**spent** (`59dbbb0b`, `48355825`, `6758d885`;
[`phase5_session_p2.md`](phase5_session_p2.md)). **P″** = W3's seams 2 and 3 — **spent**
(`acf5bfd1`, `d7a1d130`, `b07e407c`; [`phase5_session_p3.md`](phase5_session_p3.md)).
**Q** = W3's flip + W4 + W5 + W6 + W7 — everything but the exit
([`phase5_session_q.md`](phase5_session_q.md)), under the **forcing rules** written
at its head (Eugene, 2026-08-14: sessions that underdeliver get forcing prompts —
a clean seam is a checkpoint, not an exit; a reverted attempt with settled blockers
is open work; close-out ≤ 30 log lines; drops require a written reason). **S** =
W8. *(Naming: primes retired at Eugene's direction — the letter sequence resumes
after P″. Older forward references to "Q" meant the exit; the exit is now **S**.)*
A probe run per container/file
converted, and **budget it to fire**: session P ran three green, session P′ ran three and
the second convicted `AddShortTermToList` mid-face, session P″ ran three green over two
motion faces and a one-caller lifecycle swap. No perf measurement before W8 (D-gate-1).

**Session P″ costs one more session than it was scoped for**, and the reason is a size:
the hoist is 148 call sites, not 115 (§W3, corrected below), and the flip behind it is 83
consumer sites in the *write* paths — DPB, concealment, the access-unit loop — where the
settlement's own worked example (the decode path) does not apply. The flip is atomic: the
slot type change forces every consumer in one commit, so there is no half of it to land.

The re-plan's "two sessions if P reaches W5" did not hold, and neither did the
correction that replaced it. W2b was **not** two carriers — the layer's `pDec` was a
cache with one stamp site, and it deleted rather than converting — so W2b cost one
session's first two faces rather than a session. What cost the rest of session P′ was
W3's first seam (204 sites) and the discovery in §W3 below. Count is **three work
sessions plus the exit** from here — **superseded 2026-08-14**: Q absorbs what was
R (Eugene's forcing direction), so the count is **Q (the flip through W7) + S (W8)**,
two from here, with Q's remainder rolling forward only under its forcing rules.

**W2b's design question — settled by reading the tree (steward, at `c8ebc20f`), and
executed as settled (T5.P′1).** The settlement was right on every clause; the S23 check
it gated is recorded at the deleted field and in session P′'s log §1. Kept here because
the *method* is what W3 needs next: a design settled in writing before the first edit
executed in one pass, where W2b's earlier framing would have opened a 148-function leg.
The choice was false: the layer's `pDec` is a **cache of `dec_pic(pCtx)`** — one
stamp site in the whole decoder (`decoder_core.rs:3704` → `InitDqLayerInfo`
`:3448`), and its null arm is **parse-only mode**, not threading (`GetThreadCount`
≡ 0, `decoder_core.rs:705`). So the field dies rather than converts: readers that
hold `pCtx` derive (the two plane consumers, `decode_slice.rs:2063`/`:2115`,
already take it as their first parameter); the few layer-only leaves
(`PredPSkipMvFromNeighbor` `mv_pred.rs:439`'s shape) take `PPicture` from their
ctx-holding callers or merge into them (D-fid-1); identity compares become `PicId`
equality. **It is not the 148-function leg arriving from the other side — it is a
handful of leaves.** `DqLayerState::pRef` is **dead** — zero readers, zero
writers — and deletes first. One S23 check gates the mechanical pass (the
`pDec = None` mid-AU resets); the P′ brief carries it with sites. The non-null
arm's real target — `SPicture.pMv`/`pRefIndex`, still raw allocations — is W3's,
named in its row.

## W3's design question — settled by reading the tree, executed and corrected (T5.P″2/3)

The surface is **148 call sites, not 115 and not hundreds** — the settled figure was out
by a third and its smallest per-file number was four times the truth, because three of
`deblocking.rs`'s four were **prose** (S16's floor, S24's unit clause; re-derived at the
face and corrected in place per the brief). The tree at `f5a3eac8`: decoder_core 38,
decode_slice 35, manage_dec_ref 24, cabac 17, ctx bodies+helpers 10, cavlc 8, mv_pred 8,
EC 7, deblocking 1. **The design did not move** — the conversion has the same shape at 148
as at 115, which is why the disagreement was logged rather than re-opened.

With owned slots a result stops being a copy (T5.N1's invariant) and becomes
a derivation through the Box's tag — any two live results conflict. So per-use
resolution dies and **accesses move inside brackets**: a scope that borrows the pool
once at its top and threads the resolution down, touching the pool nowhere else
inside. The decode bracket is **the slice** — `dec_pic` and both ref lists are
constant across one slice's MB loop; EC, DPB and output ops bracket per operation.

Execution is the ids-before-ownership maneuver one level up — **derivations move to
bracket tops before brackets become borrows** — in two steps:

1. ~~**The hoist, under `PPicture` slots.**~~ **DONE — T5.P″2 (`d7a1d130`) + T5.P″3
   (`b07e407c`).** Per-use accessor calls became parameters threaded from bracket tops;
   pure motion, byte-identical, three green probes. `PicRefs` is the view the settlement
   asked for, `ref_id` is `ref_pic` split at the line the flip moves, and the decode path
   now reaches the pool at **three** bracket tops and nowhere below them. `MapColToList0`
   is why the view is a resolver rather than a resolved array: it is the one site whose
   handle comes out of another *picture*.
2. ~~**The flip, brackets only.**~~ **DONE — T5.Q1 (`539212bd`) + T5.Q2 (`a8eaa3e5`).**
   All six settled facts held; the type change itself cost **six** production edits, and
   what cost the face was the part no compiler could ask for — *which resolutions may
   coexist*. The answer that scaled: split the resolver family into a shared default and
   `_mut` forms and let the compiler enumerate the writers (ten sites), rather than read
   88 by eye. **F42** is the new finding — a reference list can name the picture being
   decoded, and `PoolRest::get` panics on exactly that slot. The six facts, as executed:
   - `PicRefs::get` returns **`*const SPicture`** — below a bracket top the decode path
     only *reads* a reference, verified by grep over every ref-bound local: zero writes.
   - **The current slot resolves through the mutable picture's own pointer**, never
     through the rest: `PoolRest::get(cur)` *panics*, and a malformed stream can legally
     put the picture being decoded in a reference list, which the C aliases and reads on.
     Same tag, so one borrow, and S6's never-widen default is kept on an ungated path no
     gate here can see.
   - `PoolRest` needs a **hand-written `Copy`** (a derive adds `T: Copy`; `T` is
     `Option<Box<SPicture>>`), or every macroblock-tree signature grows a lifetime.
   - `prefetch_free`/`next_for_thread` return **`Option<PicId>`**, not a pointer.
   - `SPicture::data_ptr` takes `&mut self`; `GetRefPic`'s three calls are the only
     reference-side uses and need a `&self` form.
   - `CreatePicBuff`'s partial-failure arm becomes the `Vec` going out of scope;
     `DestroyPicBuff` becomes F37's reset plus a `drop`, and R4 is discharged by
     construction.
   **83 consumer sites remain** (decoder_core 38, manage_dec_ref 32, error_concealment 10,
   decode_slice 3) and they are the *write* paths, where per-use resolution survives
   wherever the result does not outlive its expression — read each for **span**, not for
   count. F19 closes decoder-side here; F37 is adjudicated here, per its record. The four
   accessors are **deleted** — done-test: their greps read 0 and direct `.pPicBuff` reads
   enumerate to bracket sites only.

W4's colocated falls out of the bracket: inside the slice bracket cur and ref are
already resolved, so `GetColocatedMb` takes both as parameters and T5.N5's
`debug_assert!` becomes `mut_and_rest`'s type-level fact.

**And `pDqLayersList` is W2b again, not W1** (the row undersold it): the list is one
`Box::into_raw` (`decoder_core.rs:2780`) but **`pCurDqLayer` is its cache** — one
production stamp (`:3578`), ~580 uses, of which **81 read the ctx field mid-tree**
(decode_slice 25, decoder_core 15, cabac 12, manage_dec_ref 11, EC 11, rest 7)
while the call tree already threads it as a parameter. The field deletes the way
`pDec`'s did: derive once at the loop top from the owned Box, thread the existing
parameter, the 81 field-reads become param sites. ~~`pTempDec` alone is truly
W1-shaped, and `pPicBuff` → owned is safe **before** the flip~~ — **both DONE, T5.P″1
(`acf5bfd1`)**, and the ordering claim held: under `PPicture` slots a `pool_pic` result's
provenance is `AllocPicture`'s, not the pool `Box`'s.

## Phase exit conditions (the definition of done)

1. Decoder `raw_ptr` ≈ 0: every occurrence in `src/decoder/` is on the survivor
   list (the output-contract consumer) or is prose.
2. Every decoder module `#![deny(unsafe_code)]`, exceptions enumerated with
   Phase 8 pointers.
3. `SHIM(` decoder share = the survivor list exactly; census green; F40-class
   sweep run with its results recorded.
4. Gates: full battery `OVERALL: PASS` at `exit` level — goldens 57 (none ever
   moved), sweeps 341/341 both profiles, benches bit-identical, Miri both probes,
   the widened `exit`-level Miri targets (S22: the backlog check).
5. The perf adjudication done and written: stop-line verdict, niche verdict,
   ledger reconciled toward the recovery expectation — or the escalation table in
   front of Eugene.
6. `prompts/phase6.md` written; §0 refreshed; open findings each carry an owner
   (expected open set: F3→Phase 7, F23/F38-class/F41 + the `api/` inventory →
   Phase 8, F36→decoder-threading-or-deletion, F4/F6–F14 survivors per file).

## Standing constraints (unchanged by D-fid-1)

Output equivalence per commit (goldens/sweeps/benches — the C++-as-reference).
No golden movement. No `get_unchecked` (S8). No window hoisting (S8 #4). No
pool/threading edits (F12/P10 — encoder side). F3 per S14. The shell is the
constructor (session O's correction).
