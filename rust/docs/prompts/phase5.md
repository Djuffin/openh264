# Phase 5 — decoder structural rewrite

Scope: plan §5's steps 5.1–5.6, in order. **Sessions A (duplicate census, P3 identity
tests, 5.1 closure) and B (5.1's F21 pin and S25 audit) are done — work starts at
5.1's plane conversion, §1 step 3.** Rules: plan §7.6 — S20/S21/S24/S25
bind every struct edit here. Perf: §7.4 (D-perf-4, S2b). Before starting, read the
Phase 5 session-A log entry (the 5.1 closure is its §5) and
[`phase5_findings.md`](../phase5_findings.md) F22. This file supersedes on
disagreement; fix disagreements in place. Counts below measured at `f974e0e8`;
re-grep before acting on any of them (S24). Estimated 7–10 sessions remain.

Per-session scope is the **S20 closure, not the file** — compute it first, write it
down, size commits by it. Enumerate the S25 re-entrancy audit (who else reaches this
object while I hold a borrow?) together with the closure. In all constructor work,
run F19's check per allocation: *which line frees this?*

## 0. Session start

1. Commit any inherited doc tail first.
2. Session-open check, per **S27**: if the tail was `rust/docs/`-only, the previous
   session ended `OVERALL: PASS`, and `rust/tools/` and the toolchain are
   unchanged — build both profiles, tests, ratchet, census only, and run the S2
   null when the first perf verdict needs it. Otherwise (or if in doubt):
   `bash rust/tools/gates.sh full` **from the repo root**, `OVERALL:` is the
   verdict. Last recorded: **449 debug / 443 release / 20 ignored**, Miri **309**,
   census **60**, decode goldens **56**, ratchet `raw_ptr` **4548**; sweeps 341/341
   release, and the debug sweep now draws F3 (see §8).
3. Recount every number you are about to rely on.

The census gate runs at commit level (`rust/tools/census.sh` against
`rust/tools/census_allowlist.txt`): a new duplicate declaration or inferred-target
double cast fails the build. When a step unifies an allowlisted entry, remove its
line; new allowlist entries carry their class (a: cross-codec namesake — no renames,
P14; b: within-codec duplicate — unify, divergences per copy, S21; c: value-divergent
constant/table — preserve per-consumer behaviour, S6; d: legitimate same-name fn) and
owning step.

**F3** — known encoder MT race, not this phase's to fix. Apply **S14** — its §7.6
text is current (signature, measured rates, isolation re-runs, sweep-level
alternation). Anything outside S14's signature is real: stop, revert, investigate.

## 1. Step 5.1 — Picture & DPB (steps 1–3 done; step 4 done per file; **`PicPool` and identity remain — deferred until after 5.2**, accepted 2026-08-11: `pCtx->pDec` stays a pointer either way, 5.2 owns F22's reachability, and the callers that would hold a `PicId` convert in 5.2; the five P3 tests keep either order safe)

`SPicture`'s planes become owned (`PaddedPlanes` + `Vec`s); `PicPool`;
`pic_queue.rs` recycling predicate; `manage_dec_ref.rs`; `error_concealment.rs`
identity sites. Five P3 identity tests exist (`deblocking.rs` ×3,
`error_concealment.rs` ×2, all same-POC) — read them before touching either file.

Order inside the step:

1. ~~F21 pin first.~~ **Done, T5.B1 (`0fd0c9cc`).** Three assets, goldens 53 → 56,
   regenerable by `rust/tools/make_narrow_assets.py --check`. **Only
   `narrow_16x16_idr_lost` covers F21** — the two clean narrow rows are green under
   a revert of the fix, because the divergent copy's one call site is
   `WelsInitRefList`'s concealment prefetch, which no cleanly-decoding stream
   reaches. Coverage proven by that revert, not asserted.
2. ~~The S25 hazard.~~ **Done, T5.B2 (`ff12e966`) — and this brief was wrong about
   what it was.** `SPicture::unref` had one caller, its own unit test, and no C++
   counterpart: deleted, not restructured. The audit found **nine live functions**
   in `manage_dec_ref.rs` holding `&mut *pCtx` / `&mut *pRefPic` across re-entrant
   calls (`pRefPic` is `&mut ctx.sRefPic`, a *subfield* of the context, so the two
   overlap); all now name `(*pCtx)` / `(*pRefPic)` per use, with the rule written at
   `SetUnRef`. **F13's `manage_dec_ref` Miri skip is gone** — six
   `as_ptr()`/`as_mut_ptr()` list shifts behind it, and three test defects.
3. ~~The conversion.~~ **Done, session C**, in three faces: `79588684` (T5.C1,
   `iPlanes` and the fourth slot deleted), `999f300b` (T5.C2, every plane read onto
   `SPicture::linesize` / `SPicture::data_ptr`, the latter marked `SHIM(phase5)`),
   `7a28e620` (T5.C3, `planes: [PaddedPlane; 3]`, `AllocPicture` heap-constructing
   and `FreePicture` dropping). Goldens 56, none moved; perf flat at the allocation
   seam. **The one thing to carry: `data_ptr` must not narrow provenance.**
   `plane.as_mut_slice()[origin..].as_mut_ptr()` is safe code with the right
   address, passes all nine gates, and is UB at the first read into the padding —
   Miri caught it, nothing else could. It is `as_mut_ptr().wrapping_add(origin)`.
4. ~~Each file's S25 enumeration.~~ **Done, written into the commit that converted
   each**: `deblocking.rs` and `error_concealment.rs` at T5.C2, `pic_queue.rs` at
   T5.C3. The pool's answer is that nothing in it holds a borrow across anything;
   the re-entrant pair is `WelsInitRefList`'s and is enumerated at its own site.
5. **Still open in 5.1, and it is the next closure**: `PicPool`, the recycling
   predicate as a method on it, and `PicId` for the three identity sites. The five
   P3 tests exist to gate exactly this; `safe/pool.rs` already has the handle type
   and its `pair_mut`. Nothing in 5.1 blocks 5.2, so the order is a judgement call
   — see session C's hand-off.

## 2. Step 5.2 — MbGrid (**closure computed and written, session D log §2 — do not
recompute it. The subtraction has landed (T5.E2). The grid conversion is
`unblocked as of T5.G1`: F26 closed at T5.F2, F27–F30 at T5.F3, F25's inventory and
F31 at T5.G1, and the probe is green un-ignored. Blocker: none.**)

Kill the `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned
`MbGrid`; re-point the cache fills (**28** signatures over three files, not ~40 over
`parse_mb_syn_*`: `parse_mb_syn_cabac.rs` 15, `mv_pred.rs` 8, `parse_mb_syn_cavlc.rs`
5; the 30-entry scratch caches become `&mut` locals passed down, and their owners are
two stack locals at `decode_slice.rs:4632` and `:4836`).

~~**Blocked on F24.**~~ **F24 is fixed (T5.E1) and it was never one function** — the same
nested-borrow shape sat in four more places in `decoder_core.rs`, including four borrows
that escape into the layer and one into the context. **F25** (`WelsDecodeSlice`'s three
overlapping `&mut`s across the re-entrant `pDecMbFunc`) went from session D's prediction
to a Miri finding and is fixed too.

~~**The blocker is now F26**~~ **F26 is fixed (T5.F2)** — `memory_align.rs:49-50`'s
integer round trip is `addr()` + `byte_sub` now, which is what `memory_align.cpp:92`
always was, so **both codecs' heaps carry tracked provenance** (19 consumer files, 10 of
them the encoder's; S18). Two corrections to how it was written up here, both from
T5.F1's one-run experiment:

* **F26 was live before T5.E1** — the `addr_of_mut!` line exposed nothing.
* **F26 was never what the probe was stopping on.** `cabac_decoder.rs:855` fails with a
  *tracked* tag too, which is a conviction rather than a conservative refusal, and that
  defect is **F27** — a protected `&mut BsCursor`, split out of the NAL's `BsReader`,
  live while `cabac_rbsp_window` reaches the same reader whole.

S22's backlog then arrived as predicted: **F27, F28, F29 and F30 all closed at T5.F3**
over six probe round trips (the aliasing family in the CABAC path, the layer borrowed
across re-entrant calls in 20 functions, 30 `pCabacCtx.as_mut_ptr()` derivations, and one
`offset(-1)` before an array). F25's own inventory closed at **T5.G1** — **11 bindings,
not the 12 recorded** (the seventh in `decode_slice.rs` was the probe's own doc comment;
see F25's S24 note), plus the 13 nested borrows hanging off them.

**There is no blocker. `decode_slice_loop_runs_under_the_aliasing_checker` runs
un-ignored and green** (T5.G1), so the slice-decode loop is under the aliasing checker
end to end and every conversion from here gets a Miri verdict that means something. Two
things this changes for everyone downstream:

* **The Miri gate costs ~6 minutes more per run** and that is the price of the coverage;
  it is not a regression to investigate.
* **Adding `#[cfg_attr(miri, ignore)]` back needs a finding that owns it** (S15), and the
  label must name what it waits on. The queue behind the un-ignore was **one** defect,
  **F31**, and it was not an aliasing defect at all — the SPS/PPS `memcpy` translated as a
  typed copy, so the `memcmp` beside it read uninitialized padding. Owner 5.5, fixed in
  the same commit.

**The systemic rule stands and is now the wider one.** F25's retag is whole-context
(`[0x0..0x8ae00]`); `src/decoder/` now holds zero `&mut *pCtx` and no function in the
port takes `&mut SWelsDecoderContext`. What 5.2–5.6 still convert per file touched is
F28's generalization: **nothing reachable from `pCtx` is held as a borrow across a
call** — not the context, not the layer, not the picture.

The closure's conclusions, so they are not re-derived:
- **The grid goes inside the layer, not on the context.** Of 66 functions taking a
  DqLayer pointer, **50 take the layer only** and would each need a new parameter —
  atomically, per S20 — if the grid lived where `sMb` is. In the layer, those 50
  signatures do not change at all.
- ~~**`SMbCache` dies whole rather than converting.**~~ **Done, T5.E2** — the 27 arrays
  are allocated straight into the layer, `InitCurDqLayerData`'s 27-pointer re-alias is
  deleted, and the struct, its `Default` and the context field are gone.
  **S24 correction to the closure**: it named *one* live consumer (the `pSliceIdc`
  `0xff` memset). There were **three** — the memset and both `numMb` computations, in
  `InitialDqLayersContext` and `UninitialDqLayersContext`. The free path's is the one
  that matters: it needs the **allocation's** dimensions, and the layer's
  `iMbWidth`/`iMbHeight` are the *current slice's*, smaller on any stream decoding below
  the negotiated maximum. Acting on the closure verbatim would have freed with the wrong
  size. The context already keeps what is needed (`iPicWidthReq`/`iPicHeightReq`, set in
  the same function), so it stayed pure subtraction — but only because someone looked.
- ~~**Two pure subtractions…**~~ **Done, T5.E2.** `LAYER_NUM_EXCHANGEABLE` was **1** and
  is deleted, both definitions (it had two: `decoder_context.rs:72` and
  `decoder_core.rs:54`); `pDqLayersList` is a scalar and the three one-iteration loops
  are straight-line. `pMotionPredFlag` died with the struct. Census: the
  `type SMbCache x2` line is **removed**, not re-keyed — with the decoder's copy gone it
  is not a duplicate at all. 61 → **60** allowlisted.
- **No layout assert blocks this.** `assert_size!(SDqLayer, 512)` / `(SMbCache, 576)` in
  `encoder/abi_guard.rs` pin the **encoder's** namesakes. The decoder's two have no size
  assert and no offset pin.
- **S21 is where the work is**: `SDqLayer` is `WelsMallocz`-allocated at
  `decoder_core.rs:2650`, so an owned field wants T5.C3's heap constructor (one alloc
  site, one free site) or the `make_zeroed_shell_valid` shell. F19's check discharges
  clean at **field** level — 24 arrays against 24 frees; the raw call counts (28 vs 25+1)
  differ only because allocation unrolls the three `LIST_A` pairs and freeing loops.
- **The census needs editing in the rename's own commit**: zero allowlist entries name
  5.2, and `type SDqLayer x2` / `type SMbCache x2` are class (a). The rename takes both
  keys to `x1`, and the count is part of the key.
- `SDqLayer::pBitStringAux` (`*mut BsReader`) retires here — 24 sites, **one writer**
  (`decoder_core.rs:3669`, pointing *into* the NAL unit), 17 readers in
  `decode_slice.rs`; `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor dies with it.
- The decoder's `SHIM(phase2)` kernel adapters retire as 5.2–5.4 convert their
  callers (154 markers crate-wide; the decoder share goes here).
- ~~Answer F22's reachability question here.~~ **Answered, T5.D1**: it cannot be null —
  one writer, one call site, dominated by a prefetch-or-return, and the same in the C++.
  The guard is dead code in both trees. See §3 for what that does to the unification.
- **Every new accessor obeys S28** (derive from the allocation root, never through
  a narrowing slice; Miri test reading the full legal reach) — `MbGrid`'s
  accessors are exactly the shape that earned the rule. And nothing caches a
  pointer beside its owner: `SDeblockingFilter.pCsData` is the one live mirror
  left, 5.4's to delete.
- **The S25 enumeration has a gate, and as of T5.G1 the gate is green and part of the
  battery.** `decode_slice_loop_runs_under_the_aliasing_checker` (`decode_slice.rs`)
  decodes `narrow_16x16.264` under Miri. It found F23 and F24 at session D; at session E
  the `DecodeCurrentAccessUnit` pair, then **F25** — the loop it was written for, no
  longer a prediction — then **F26**; at session F, F27–F30 over six round trips; at
  session G, **F31** and then nothing. **Ten defects over eleven round trips**, and the
  last one was not an aliasing defect, which is the sign the aliasing seam is actually
  clean rather than merely quiet.
  One defect per run, ~6–8 minutes per run; the way to beat that rate is to grep the
  *shape* of each defect once Miri names it, which is how four of F27–F30's sites were
  found without paying for a round trip. **Comment-strip before you count** — twice now
  a count over this file has included its own doc comments (F29's four missed sites,
  F25's phantom seventh binding).

## 3. Step 5.3 — Neighbor & MV

`mv_pred.rs`: punning → byte ops; `SetRectBlock` → typed generic on the grid;
colocated reads via `cur_and_ref`.
- **F22 unifies here**, and its reachability question is **answered** (T5.D1):
  `pCurDqLayer->pDec` cannot be null on the CABAC parse path — one writer, one call
  site, dominated by a prefetch-or-return, and the same in the C++. So the guard is
  dead code in both trees and this is a divergence, not a latent crash. Two things
  follow for the unification, both from session D's S24 re-grep of the C++ side:
  the divergence is **three** functions (`UpdateP16x16MotionInfo`,
  `UpdateP16x8MotionInfo`, `UpdateP8x16MotionInfo`), which unify onto `mv_pred.rs`'s
  guarded shape; and `Update8x8RefIdx` runs the other way — C++ has no guard, the
  CABAC copy is faithful, and `mv_pred.rs`'s added guard comes off. Per-function, not
  per-module (S21).

## 4. Step 5.4 — Deblocking driver

`decoder/deblocking.rs`: `SDeblockingFilter` holds `PicId`s + per-MB plane
cursors; identity compare per P3 (the three deblocking identity tests gate this).

## 5. Step 5.5 — decoder_core.rs

Allocation → constructors, paramset store (P4), context decomposition (§2.2.6),
`Drop` teardown, `Default` derives.
- S21 with force: the decoder context embeds its buffers **by value**, so every
  owned field lands inside `mem::zeroed` reach. The `MaybeUninit` shell +
  `new_boxed()` exists at `decoder_context.rs:769`; extend it per owned field;
  replace it with a real constructor at the end of this step.
- F19's check runs here, per allocation.
- `SWelsDecoderContext` has no `assert_size!` and no offset pins; don't look for
  an instrument that isn't there.

## 6. Step 5.6 — decode_slice.rs (last, per P1)

Including the EC MC paths. Delete the remaining decoder shims. Decoder modules get
`#![deny(unsafe_code)]` one by one. No `SBitStringAux` shell exists (deleted at
T3.4).

## 7. Gates and exit

Per face: full battery; decoder goldens frozen (the F21 rows included, once
landed); sweeps 341/341 both profiles; 3-pair interleaved medians per seam (S2b: a
median outside the null band gets more pairs before it gets a mechanism); Miri;
ratchet regenerated per S16 with deltas named.

Exit: frame-count parity and the `#[ignore]` set unchanged; T3.0's 2316-row golden
table green in both profiles (T7 stays deferred); decoder `src/` unsafe-free.
**One named shim survives the phase**: `SPicture::data_ptr`'s consumer at
`decoder_core.rs:~1087` fills the public output contract (`pointer, stride`) and is
not a kernel adapter — 5.6 cannot delete it; it retires with the API boundary work
(Phase 8). The straggler sweep expects exactly this one; its marker text names the
owner.
**every §7.4 ledger entry whose shims died in this phase must clear**. This phase
collects 4a's downgraded decode rows (≈ +17.8/+10.1/+9.6% cumulative; ~7 points of
CB headroom under the tripwire). The mechanism is constant dimensions reaching the
kernels, so flat mid-phase bench readings are expected — the ledger is the
instrument that moves. S19 at exit: refresh §0, write `prompts/phase6.md`, stamp
this brief historical.

## 8. Metrics inherited

*(Re-greped at session C's exit. S24 still binds — recount before acting.)*

- `transmute` reads **4: all prose, zero calls**. Don't chase it.
- The **ratchet** is the instrument, not struct sizes. Phase 5 so far:
  `raw_ptr` 4815 → 4597 (session A, by deletion; session B flat) → 4595
  (session C) → 4600 (session D, +5 for the aliasing probe) → 4548 (session E:
  −4 as F24's fix collapsed the `as *mut T as *mut c_void` double casts, −48 as
  `SMbCache` and the one-element dimensions died) → **4589** (session F: +8 for the
  allocator's two new tests, +33 as F28's fix traded `&mut` bindings for `*mut`
  annotations — removing a retag costs a pointer type, which is the same trade
  T5.E1 made in the other direction), `unsafe_block` 613 → 618 → 616 →
  613 → 614 → 614 → **619**, `unsafe_fn` 1250 → 1249 → 1248 → 1247
  (`InitCurDqLayerData` deleted) → **1248** (`cabac_ctx_base`). S16's warning was collected twice at session C:
  prose inflates `raw_ptr` and `SHIM(`, and both were reworded rather than baselined.
- Gates: **451 / 445 / 20**, Miri **311** (session F: +2 both, the allocator's
  address-pin and full-reach tests), sweeps 341/341 both profiles, decode
  goldens **56 rows**. The debug sweep now reproduces F3 (`rust_enc`'s
  `[profile.dev] opt-level = 3` made it fast enough to lose the race); measurement 29
  is the cleanest evidence the finding has.
- **Miri skips are 2, not 3**: `wels_thread_pool` (F12, Phase 7) and `encoder_ext`
  (F13, Phase 6). `manage_dec_ref` came off at T5.B2.
- Census gate state: inferred-target double casts 0, duplicate-body groups 198
  (ratcheted), **60** allowlisted entries (session E: the decoder's `SMbCache` was
  deleted, so its class-(a) line is gone — a deletion removes the entry where a
  rename would only re-key it). Remaining within-decoder entries are allowlisted
  with their owning steps.
- `SHIM(` **158**: `phase3` = 2 (Phase 6's), `phase5` = **2** — `cabac_decoder.rs`'s
  (dies in 5.2) and `SPicture::data_ptr` (dies as 5.2–5.6 convert the kernels,
  *except* at `decoder_core.rs:1087`, where the public output contract hands the
  pointers to the API consumer and they outlive the call; that one outlives the
  phase). Rest `phase2`.

## 9. Non-goals

No Phase 6 pulls: encoder `SMbCache`, `SSlice` layout, the free cascade,
`wels_encoder_ext.rs` internals. No parked-family reopening (6.3's). No
F8/F9/F11-class fixes (S6). No `get_unchecked` (S8). No golden movement beyond the
authorized F21 rows. No pool/threading edits (F12/P10). No F3 work beyond §0's
protocol.

Cheap and welcome if passing: delete `pfSetNZCZero`
(`encoder/wels_func_ptr_def.rs:385`) — one slot, one unconditional constant;
takes `assert_size!(SWelsFuncPtrList)` to 1152 and removes the last reason
`encoder/deblocking.rs`'s duplicate `WelsNonZeroCount_c` exists. Encoder-side
(6.5's by rights), listed because it is ~6 lines.
