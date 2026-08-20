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

## 5. The family census — the checklist, measured at `5f2bd711`

**Schedule families; report modules** (§1). Phase 5 spent sessions R–V learning
that the module is the wrong unit; Phase 6 starts from the unit that worked.
`src/encoder/` holds **2212 raw-pointer occurrences in code** (the totals in §0
include prose) and **671 `unsafe fn` definitions**. By pointee, with its Phase 5
analogue and the recipe that closed it:

| family | code sites | analogue | recipe |
|---|---|---|---|
| `*mut c_void` | 222 | — | **deletion, not conversion** (S18): `wels_preprocess`'s `IWelsVP` shape (`pCtx: *mut c_void` + `Init`/`Uninit` fn-pointer pairs) is Phase 4b's dissolved-vtable residue; the rest is MT plumbing (Phase 7's) and log/task fields |
| `*mut u8` | 277 | the decoder's plane/scratch pointers | `PlaneCursor`/`PlaneCursorMut` (`safe/plane.rs`) — **eight encoder files already consume them** |
| `*mut sWelsEncCtx` | 258 | `*mut SWelsDecoderContext` | the flip — **accessors first, view struct last** (X→Z's forced order: `deny` fires on the *call*, so callees convert first) |
| `*mut SMB` + `SMB`'s five raw arrays (`sMv`, `pRefIndex`, `pSadCost`, `pIntra4x4PredMode`, `pNonZeroCount`, `md.rs:294–306`) | 131 | 5.2's `MbGrid` in spirit — **ruled inline at session C's brief** | the five arrays become inline POD arrays in `SMB` (S21: the `SMB` block is `WelsMallocz`'d, so an inline array is valid at zero and an `MbArray` field is not until 6.6); the `*mut SMB` list and parameter family go with the layer bracket (D) |
| `*mut SSlice` 134 + `*mut SDqLayer` 72 | 206 | the layer/slice brackets | **session D**: the layer goes `Box`-built (T3.6's `pOut` precedent — S21 read the other way) and then owns; its stored aliases become ids first (the ordering rule); the slice banks follow as `Vec<SSlice>`; the `*mut` parameter families are E's |
| `*mut SMbCache` (12 raw fields, `md.rs:354–375`) | 102 | `DqLayerState`'s scratch caches (session M's 45 params) | the eight malloc'd buffers go inline and the four ping-pong aliases become half-selectors (session C, same recipe as `SMB`); the `*mut SMbCache` parameters take-what-you-reach (E) |
| `*mut SPicture` | 64 | `PPicture`/F42 | `Pool<Box<SPicture>>` + `classify` — `MarkPicAsRef` has the same shape (§0.1) |
| `*mut SWelsFuncPtrList` | 57 | the dispatch tables | the table is already `Option<fn>` fields; the **pointer to it** is the residue |
| ME/MD/RC/CABAC records (`SWelsMD` 55, `SWelsME` 41, `SMVUnitXY` 78, `SMeRefinePointer` 17, `SCabacCtx` 28) | ~220 | the parse scratch | take-what-you-reach |
| scalars (`i32` 109, `i16` 107, `u16` 52, `i8` 35, `u32` 24) | ~330 | the coefficient cursors | slices/`&mut [T; N]`, per family |

Two assets the decoder did not have at its open: the encoder **already consumes
the safe vocabulary** in eight files (Phase 2's kernels landed on both sides), and
`abi_guard.rs`'s **53 assertions** mean every struct that de-`repr(C)`s has a
proof to delete in the same commit — the check is free and it is mechanical.

## 6. Session plan — revised at Phase 6's open (steward, 2026-08-18)

The plan's §4 step list (6.1–6.6) is the *scope*; this is the **order**, and it
differs in three places, each earned by Phase 5:

1. **The probe comes before the first conversion.** The encoder has **zero
   encode-path Miri probes** — the only encoder-adjacent Miri test is a
   table-install check that is `#[cfg_attr(miri, ignore)]`. F47 is the precedent:
   real UB on the *ordinary* CAVLC path sat undetected for five phases because no
   probe covered it, and the probe that finally ran found it on its first
   execution. **Build the probe first and budget it to fire.**
2. **`c_void` and F52 are cleared before anything lands on them.** A conversion
   that lands on a shadow rather than a body is invisible (F43/F52), and a
   conversion of a dissolved vtable's residue is work spent on dead shape (S18).
3. **The context flip is last, not first** — V's measured order, and it cost
   three sessions to learn on the decoder: `deny(unsafe_code)` fires on *calling*
   an `unsafe fn`, so the callees must be safe before the caller can be.

**Sessions** (each large, per S31 and the terminal rule; drop-from-the-end only
at a family boundary, and only with a written reason):

* **A** — **SPENT** (2026-08-18). **the encoder Miri probe, and the Miri budget.**
  Delivered: `drive_encoder_over` beside `drive_decoder_over`, raw-pointer-based
  (F23's encoder twin is structural and was confirmed by construction); the probe
  **found real UB on its first execution and nine more times after that**, and all
  ten are closed — **F13's last production site** and its 20-derivation family,
  **F57** (`MvdCostInit` off the end of the MVD table) and **F58** (VAA reading a
  never-written picture's visible luma) both new in
  [`phase6_findings.md`](../phase6_findings.md), two escaping borrows and four
  ordering defects. Miri's cost is measured per step (`--lib` 835s, of which the
  three decoder probes are 665s), **S32 transfers to the encoder** on picture size
  (flat to 1.3% across 64x the area) and the plan's §7.6 carries the amendment, and
  **no decoder probe could be retired** — measured reach says each reaches code no
  other reaches. The live probe costs **+1.95%** of the Miri battery in Miri's own
  deterministic clock (1624.96 -> 1656.72s), about **+20s** of wall clock.
  [`phase6_session_a.md`](phase6_session_a.md)
* **B — SPENT** (2026-08-18). **the encode probe goes live, and the clearing.** Brief:
  [`phase6_session_b.md`](phase6_session_b.md). Delivered, all four faces:
  **F52 closed** — all six adjudicated by opening both lines (four trait-method
  *declarations*, a faithful `return false` beside its dispatcher, two methods on two
  types); `find_shadowing_stubs.py` skips declarations now (22 → 18 names) and carries
  a `--self-test` that keeps F52's own stub shape printing, red under three
  deliberately broken copies. **The encode probe is live and green in the `--lib` Miri
  step** — the three settlements executed first (`SSlice.pSliceBsa`,
  `SWelsSliceBs.pBsBuffer`, `SWelsNalRaw.pRawData`, all three caches of something the
  holder already reaches; `WelsEncodeNal` typed; `SetOneSliceBsBufferUnderMultithread`
  deleted; both `SHIM(phase3)` markers now name `pThreadBsBuffer` and **Phase 7** and
  nothing else), then the walk: **eight reds**, each read, classified, fixed and
  observed gone — S29 nested and its boundary clause, **S28's provenance class** (nine
  walking cursors re-derived from the whole array), **F14's class** (twelve neighbour
  pointers before their guard), F13's family in the `as_mut_ptr()` spelling, a
  protected shared borrow beside a write into the same struct, and **F59** (new): the
  IDCT-reconstruction shims built `&mut` and `&` over one span because the inter
  reconstruction is *in place*. **No Phase 7 blocker was reached.** **`c_void` 268 →
  131** (122 code, 9 prose): `IWelsVP` dissolved with its seven thunks and its
  create/destroy pair (`m_vp: Box<SWelsVpContext>`, typed plugin `Set`/`Get`/`Process`,
  the five untranslated methods keeping their exact behaviour), the typed-at-both-ends
  casts deleted, and four dead dispatch families with them — the three
  `pfSetMemZeroSize*` slots, `PSetMemoryZero`, `PWriteBlockResidualCabac`, the eight
  `Combined3` fields with `assert_no_combined3`. Every remaining code occurrence is in
  one of three enumerated lines (allocator / C ABI → Phase 8 / MT → Phase 7). **The
  `SPicture` settlement is written** — three owners, a fourteen-row alias table, S34
  measured (`WelsExchangeSpatialPictures` swaps *slots*; no pool storage ever
  permutes), two id types with the reading that decides it, and F42's arm answered (no
  identity comparison exists; `pDecPic` and `pRefPic` are distinct slots by
  construction). **6.1's head did not land** and is named for session F with the table
  as its work list: measured at ~184 consumer sites over nine files, converting whole
  or not at all. S32's frame-count clause is measured (2 / 3 / 6 frames = 71.29 /
  80.53 / 102.28 s, ≈7.7 s a frame — scaling, unlike picture size) and §7.6 carries it
  with the unit change `-Zmiri-disable-isolation` brings. `exit` **PASS 13/0/1** (Miri `--lib`
  **341/0**, 1034.61 s; both sweeps 341/341, both benches identical) — and its first run **failed**,
  on a defect this session's own typing introduced (`InitPic` given `*const` where the C++ casts the
  const away and writes): fixed at T6.B9, re-run green, recorded rather than hidden.
* **C — SPENT** (2026-08-19). **The scratch goes inline** — brief:
  [`phase6_session_c.md`](phase6_session_c.md). Delivered, all three faces. **`SMB`
  holds no raw pointer**: its five outward pointers are inline arrays (`sMv`,
  `iRefIndex`, `iSadCost`, `iIntra4x4PredMode`, `iNonZeroCount`) and the five context
  arrays, their two parity banks, allocations, frees and `InitMbInfo`'s wiring half are
  gone, along with `deblocking.rs`'s dead `TagMB` — a field-for-field second copy of
  `SMB` a name-matching census could not see (F43's class). **`SMbCache` holds no raw
  pointer but `pEncSad`**: eight buffers inline, the four ping-pong aliases three `u8`
  selectors, `AllocMbCacheAligned`/`FreeMbCache`/`AllocateSliceMBBuffer` deleted, and
  `ReallocateSliceList`'s error-path aliasing over copied scratch pointers closed with
  them. Every surviving raw pointer into the inline arrays is derived from the array
  root (`addr_of_mut!`, S28/S29), and rustc's deny-by-default
  `dangerous_implicit_autorefs` turned out to enforce that spelling for free — fifteen
  sites. Sizes measured and re-pinned: `SMB` **152 → 208**, `SMbCache` **576 → 5600**,
  `SSlice` **1520 → 6544**, `sWelsEncCtx` **97952 → 97912** with all fifteen
  `assert_ctx_offset!` pins, which sat behind the five deleted pointers and are the
  port's own layout from here. **The second encode probe is live** (CAVLC + the fine
  mode-decision family in one encode) and **found two reds on its first execution** —
  the CAVLC writer holding a frame buffer and `sMbCacheInfo` across the callee that
  re-derives them, and the ninth cursor of session B's S28 family, on a path no CABAC
  probe reaches. Solo cost **24.96 s** against the first probe's **79.02 s**. It also
  exposed a hole in the *byte* gate — all 341 sweep configurations are `LOW_COMPLEXITY`
  — so both diffharness drivers gained an optional complexity argument (default `LOW`;
  the 341 count is unchanged) and ten configurations at `MEDIUM`/`HIGH` read
  byte-identical. **The span is measured** (7 pairs against `2666a83c`): encode median
  **−0.97%**, decode **+0.25%**, both inside the null band, tripwire unbreached by ≈24.5
  points, cumulative ≈ **+10…+12%** — the AoS cost face 1 predicted is below the
  instrument's resolution and **the SoA fallback stays named, not opened**. Encoder-side
  `raw_ptr` **2446 → 2389**, `unsafe_fn` **686 → 684**. `exit` **PASS 13/0/1** with Miri
  `--lib` **342/0**; its first two runs failed — once on this session's own covering
  test (T6.C4) and once on a prose `*mut ` in that fix's comment plus **F3 measurement
  67**, whose alternation (12 `mt` presets a side, 1 hit each) was run and acquitted.
* **D — SPENT** (2026-08-19). **The layer owns, and the slice banks with it** — brief:
  [`phase6_session_d.md`](phase6_session_d.md). Delivered, all five faces. **The enabler is
  one argument used twice**: `SDqLayer` is reached only through a pointer in the zeroed
  context (T3.6's `pOut` precedent), so it became **`Box`-built with a real constructor** and
  could then own what a `WelsMallocz`'d struct may not — and **the order was forced rather
  than chosen**, because a `Vec` field in a zeroed block is UB at its first drop (S21) and
  `Option<LayerIdx>` has **no niche to borrow**, so its `None` cannot be inherited from a zero
  image (F56 from the other side). The layer owns **`MbArray<SMB>`** (with `ppMbListD`
  deleted — `InitMbListD` cut *one* flat block across the layers and stored the cuts twice,
  so neither field was a carrier), **`Vec<SliceIdx>`** for its slice list, two **`Vec<i32>`**,
  **`Vec<u16>`** for the macroblock map and **`Vec<SSlice>`** per bank; four malloc/copy/free
  triples and eleven allocations went with them, and **`ReallocateSliceList` is a
  `resize_with` that closes session C's handed-over `sSliceBs.pBs` double free** — a resize
  *moves*, so each `pBs` is held once at every point. **The dynamic-slice probe found four
  reds and the largest is byte-visible**: **F60**, `FrameBsRealloc` moving `pOut->sNalLen` and
  never re-aiming the layers at it — **three SIGBUS crashes and four byte divergences against
  the C++ before, all twelve configurations BYTE-IDENTICAL after** — on a path **no sweep
  configuration reaches**, because `iMaxSliceNum` opens at `MAX_SLICES_NUM` = 35 and the
  sweep's size-limited rows code at most nine slices. Non-vacuity and the realloc are
  measured: **37 / 9 / 3** slices against **1 / 1 / 1** at the sweep's constraint, and 112x96
  is the smallest geometry on the grid that crosses 35. Its solo cost is **≈ 357 s** in-step
  (the `--lib` step reads 1432.01 s at 344 tests against session C's 1022.46 s at 342) — the
  expensive probe, and 42 macroblocks against 6 is why. **F61 is recorded, not fixed**: the MT
  bank growth never re-stamps the slice list and **the C++ has the same shape**, so it is
  Phase 7's — and the position spelling removes the class's memory-safety half while leaving
  the synchronisation half. `pFeatureSearchPreparation` and five functions behind it are
  deleted (S18, enumerated by strip-and-build). Every raw pointer still handed out is
  **root-derived** (S28) through five accessors, with the Miri test S28 mandates walking the
  macroblock array's full reach in both directions. Encoder-side `raw_ptr` **2389 → 2331**,
  `unsafe_block` **288 → 283**, **`unsafe_fn` 684 unmoved** — six new root accessors whose
  signatures name raw pointers, against six deleted dead functions; `SDqLayer` **512 → 640**
  across six measured steps, `SSliceCtx` **32 → 48**, `SSliceBufferInfo` **16 → 32**,
  `sWelsEncCtx` **97912 → 97904**. **The span is flat**: 7 pairs, decode **−0.08%** and encode
  **+0.00%**, both inside their own 7-pair null bands with the encode median *identical* to
  the null's; worst row **+1.01%** against the +25% tripwire, cumulative ≈ **+10…+12%**
  unmoved, and the brief's two named candidates are below the instrument's resolution so
  neither is claimed. **F3 measurements 68–70**, all acquitted, with the alternation S14 owed:
  base `085e2e41` **5 hits** against head **1** over 2880 configurations — HEAD is not worse.
  `exit` **PASS 13/0/1 on its first run**, Miri `--lib` **344/0** with all four encoder probe
  names in the output. **Miri ran once, at the close, on the steward's in-session direction
  (S30)**; face 0's reds predate it. **Drop-from-the-end was taken at the authorised boundary
  and nowhere else**: the `*mut SMB` (128 sites) and `*mut SMbCache` parameter families go to E.
* **E — SPENT** (2026-08-19). **The records become references, and the neighbour
  rule gets written down** — brief:
  [`phase6_session_e.md`](phase6_session_e.md). **All six steps landed, in order.**
  Five of the seven families read **zero**: `SCabacCtx` 25 → 0 crate-wide,
  `SWelsMD` 51 → 0 (and `Copy`/`Clone` dropped off the 4000-byte record — nothing
  copied it by value), `SWelsME` 39 → 0, `SMeRefinePointer` 17 → 0,
  `SQuarRefineParams` 1 → 0; `SMVUnitXY` 65 → **2**, both the F-owned
  `SPicture.sMvList` sites on the boundary list. **`SMbCache` is 96 → 72, and the
  retreat is the session's most useful finding.** Fourteen parameters were deleted
  outright as caches of `pSlice` (rule (c) — `WelsWriteMbResidualCabac` took
  `pSlice`, `sMbCacheInfo` *and* `pCabacCtx` while its body wrote
  `(*pSlice).uiLastMbQp` between uses of them). The other 62 were retyped to
  `&mut SMbCache` and **the `exit` battery's Miri step rejected that, correctly**:
  `SMbCache` is an *arena*, its consumers hold several raw cursors into different
  parts of it across calls, and a `&mut` covering all 5600 bytes invalidates every
  one of them — including the caller's. `WelsEncRecUV(pFunc, pCurMb, pMbCache,
  pRes, iUV)` takes the arena *and* a cursor into it in one call, so no argument
  ordering works. Reverted; the deletions stand, because **deleting the second path
  is what an arena wants and typing it is not**. Two further findings from the same
  battery: the neighbour classifier propagated through *named* callees and missed a
  forward into the `pfFillInterNeighborCache` **slot** (replaced by a type-driven
  checker), and `&mut *pCurMb` passed where `*mut SMB` is expected **silently
  re-narrows** — so every reborrow spelling was stripped and re-derived from the
  compiler's own type errors, closing the class rather than the instances. **`SMeRefinePointer` holds no pointer**:
  its five `*mut u8` were four fixed offsets into one `SMbCache` buffer plus an
  alias slot, so it is `iStride` + `iHalfPixHV` + a `bQuarPixSwapped` selector that
  replaced the `pQuarPixBest`/`pQuarPixTmp` `mem::swap` — **48 → 32 bytes, newly
  pinned**.
  **`*mut SMB` is 125 → 48, and the shortfall is a finding rather than a debt.** A
  macroblock parameter converts only if the function (1) does no cursor arithmetic
  on it and (2) does not re-reach the array through another parameter — both clauses
  are provenance, and the second is F13's shape. Classified mechanically over every
  `src/encoder/*.rs`, then applied: 68 converted (svc_mode_decision 31,
  svc_base_layer_md 15, rc.rs 9, svc_encode_slice 5, deblocking 2, plus 7 slot
  types), 48 named as survivors in four groups — neighbour-walkers (the CABAC
  writers' 11, deblocking's 9 with 18 arithmetic sites, md's `FillNeighborCache*`),
  array-owners (svc_encode_slice 8, encoder_ext 2), slot entry points
  (svc_set_mb_syn_cavlc 3), and Phase 7's one. **The brief's step-4 note about
  `deblocking.rs` turned out to be the rule for the whole family, not one file's
  exception.**
  **Step 0a closed F60's coverage hole for good**: preset `sl`, 12 rows, all twelve
  measured entering `FrameBsRealloc`, and a **red-proof** — with session D's re-aim
  loop deleted the preset reads 0/12 (eight divergences, two hard crashes), restored
  12/12. Sweep totals **341 → 353** in both profiles for every gate from here.
  **Step 0b fenced the screen-content family** (D-scr-1): 16 items tagged
  `SCREEN_CONTENT(dormant: Phase 10)`, the init guard verified standing, nothing
  converted.
  **Step 5 discharged the SATD debt and parked both families a third time**: SATD's
  first-ever measurement is **1.36x–4.05x** (flatter than SAD's 1.3–5.7x — a bigger
  body absorbs a fixed per-call cost better), SAD re-measured at 1.30–5.68x, and the
  ledger's untested slices-and-offsets lead is now **tested and failing** —
  `PlaneCursor::advance` re-runs the constructor's asserts, so building cursors once
  per search moves the per-candidate cost rather than removing it. Re-attempt named:
  **Phase 9**, and the thing to build there is a kernel signature, not another
  call-site rearrangement.
  **The span is flat**: 7 pairs, encode median **+0.00%** (max +1.52%) against a
  +0.16%/+1.52% null, decode **−0.24%**; cumulative ≈ **+10…+12%** unmoved and the
  +25% tripwire untouched. Encoder-side `raw_ptr` **2331 → 2015 (−316)**,
  `unsafe_fn` 684 → 679. **F3 measurements 71-74**, with the alternation the
  second owed: **head 3 / control 1 over 800 encodes under load, and 0/0 over 120 on
  an idle machine** — the sharpest demonstration on record that load is the variable,
  not the tree.
* **F** — `wels_preprocess` + the plane families (6.2), the `common/` kernel
  callers (§3), **and 6.1's recon-pool alias family**, handed over by session B with
  its settlement written and its size measured (~184 sites, nine files).
* **G** — the context flip and the deny sweep (6.6), then the phase close. **Opens
  with 6.5's fold** (steward, 2026-08-19): `au_set.rs` + `paraset_strategy.rs` R1
  remnants and `rc.rs`'s 56 `*mut sWelsEncCtx` land here with the flip;
  `wels_encoder_ext.rs` internals are Phase 8's boundary and are **not** this
  session's. Close against §7's exit conditions, not against "the lint is on".

**Seven sessions is the estimate, not the contract**; Phase 5 ran fourteen against
a plan of nine to twelve, and the difference was discovery, which this phase has
already done. A session that closes no family and moves no metric is a stall and
says so — with session A the one exception, because its deliverable is coverage
and a cost model, not a converted family.

## 7. Exit conditions — written before G, not at the close (steward, 2026-08-19)

Phase 5 ended with the lint on and **167 `#[allow(unsafe_code)]` items still
standing**, and Phase 5b existed because "done" had been defined as the former.
Phase 6's close is defined now, and it is the decoder's close, transposed:

1. **`#![deny(unsafe_code)]` on every module of `src/encoder` + `src/processing`
   except an enumerated MT set** — expected `slice_multi_threading.rs`,
   `wels_task_management.rs`, and any file whose remaining unsafe is solely the
   thread machinery — each exception named in the close log with **Phase 7** as its
   owner. An exception is a file, never a blanket.
2. **Every surviving `#[allow(unsafe_code)]` item under the denied modules is
   enumerated by category with an owner**, and only four categories are lawful:
   values crossing the C ABI (Phase 8), pool/allocator machinery carrying S28's
   mandated Miri test, an MT seam (Phase 7), and the `SCREEN_CONTENT(dormant)` tag
   (Phase 10, D-scr-1). "The lint is on" is not the test;
   the enumeration is.
3. **Encoder-side `raw_ptr` residue is enumerated by category**, code split from
   prose (S16), the way 5b handed the decoder's 47+66 to Phase 8.
4. **`exit` battery PASS**, and the cumulative perf position restated against
   D-perf-4's tripwire and D-perf-6's parked recovery.
5. **The handoffs are written**: Phase 7's (F61, F3's ablation close, F12,
   `sSliceBs.pBs` and the thread-buffer ownership, the MT files), Phase 8's
   (`wels_encoder_ext.rs` internals, the `c_void` C-ABI line, the encoder's
   api-owned fields beside the decoder's twelve).

Standing unless overruled; G's brief cites this section as its done-test.
