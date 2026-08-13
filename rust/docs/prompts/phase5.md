# Phase 5 — decoder structural rewrite

Scope: plan §5's steps 5.1–5.6, in order. **Sessions A (duplicate census, P3 identity
tests, 5.1 closure) and B (5.1's F21 pin and S25 audit) are done — work starts at
5.1's plane conversion, §1 step 3.** Rules: plan §7.6 — S20/S21/S24/S25
bind every struct edit here. Perf: §7.4 (D-perf-4, S2b). Before starting, read the
Phase 5 session-A log entry (the 5.1 closure is its §5) and
[`phase5_findings.md`](../phase5_findings.md) F22. This file supersedes on
disagreement; fix disagreements in place. Counts below measured at `f974e0e8`;
re-grep before acting on any of them (S24). **Estimated 2 sessions remain, plan for 2–3**
(re-planned 2026-08-13 at session O's close; **session O is spent** and the estimate
holds, but its content moved. **5.1 and 5.4 are done** (T5.N1–T5.N4); 5.3's colocated
face is blocked on `pDec` carrying a `PicId` — see §1 step 5 and §3. **O is spent**:
the 5.5 closure, the constructors that did not need it, F37/F38/F39/F40 fixed, F41
listed — see §5. **P** = the `decode_slice` cluster in dependency order, now carrying
5.5's tail at its head — `pAccessUnitList`'s `Drop` (the one cascade entry T5.O4
unblocked) → the `pDec` step (236 sites) → `cur_and_ref` + colocated + 5.3b → 5.6
whole, including the `*mut u8` cache family and `cabac_rbsp_window`'s retirement —
faces ordered, drop-from-the-end at seam boundaries; **Q** = the exit, never
compressed, carrying every measurement D-gate-1 deferred. P splitting once at a seam
is the likely third session. **The deep-Miri-queue risk was real and it was O's**: the
closing battery convicted twice, so budget a probe run per *container* conversion
rather than one at close.)

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
   verdict. Last recorded (session O exit): **474 debug / 468 release / 20 ignored**,
   Miri **334** (~942s), census **59**, decode goldens **57**, ratchet `raw_ptr`
   **4420**; sweeps 341/341 both profiles, and **either** profile can draw F3 (see §8 —
   session K drew two hits in each and alternated both; session L drew one in the debug
   sweep and reproduced it in isolation, measurement 35; session M drew one in the
   *release* sweep, measurement 36, which took `cabac` and `rc` out of the signature).
3. Recount every number you are about to rely on.

**Budget the Miri step at ~13 minutes, not ~1.** Since T5.G1 the aliasing probe runs
un-ignored inside `--lib`, and since T5.J2 there are **two** of them — the 1-macroblock
stream (250.7s) and the 3x2 grid (258.7s). That is the coverage the whole phase has been
buying, and F35 is what the second one bought on its first run; it is not a regression to
investigate.

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

## 1. Step 5.1 — Picture & DPB (**CLOSED, session N**: `PicPool` at T5.N1, `PicId` on the picture and P3's predicate at T5.N2. What was deferred from 2026-08-11 is done; what the deferral's own text assumed — "the callers that would hold a `PicId` convert in 5.2" — did **not** happen and is now sized, see step 5)

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
5. ~~`PicPool`, the recycling predicate, `PicId` for the identity sites.~~
   **Done, session N.** `SPicBuff` is a `Pool<PPicture>` plus a cursor (T5.N1); the
   recycling predicate is `PicPool::is_recyclable`; `SPicture` carries
   `slot: Option<PicId>`, stamped once at pool construction, and
   `picture.rs::same_picture` is P3's predicate over it (T5.N2). Six identity
   comparisons converted, and T5.N4 took the deblocking filter's whole reference
   list onto ids.
   **The pool's slots are `PPicture`, not `Box<SPicture>`, and that is the step's
   one open consequence.** Owning the pictures is what would make
   `Pool::mut_and_rest` prove the current-vs-reference split in safe code (plan
   §2.2.3's end state). It cannot be done while `pCtx->pDec` is a live raw pointer
   *into* a slot: a pool-issued `&mut` invalidates it under stacked borrows, which
   is F24/F25/F28's defect class installed deliberately. **So the split-borrow API
   waits on `pDec` becoming a `PicId`**, and that is sized at **236 `.pDec` sites,
   all in `src/decoder/`** (77 `decode_slice.rs`, 55 `decoder_core.rs`), plus
   94 `pRefList` and 209 `pRefPic` sites — a step of its own, owned by 5.5/5.6 by
   P1, not a face. The reason is written at `PicPool`'s type. See session N §5.
   **What this blocks: 5.3's colocated split borrow** (§3), and nothing else — the
   identity work it was also meant to unblock landed without it.

## 2. Step 5.2 — MbGrid (**IN PROGRESS. `MbGrid` exists, is proven and is owned by the
layer (T5.H2/T5.H3); **all 22 array families have flipped** (T5.H4–T5.H14, T5.J3,
T5.K1–T5.K3, T5.L1–T5.L7) and **`SDqLayer` holds no per-macroblock array pointer at all**,
and the first eleven carry per-MB window accessors (T5.I1). Blocker: none. Closure
computed and written, session D log §2 — do not recompute it. The subtraction has landed
(T5.E2). Unblocked as of T5.G1: F26 closed at T5.F2, F27–F30 at T5.F3, F25's inventory
and F31 at T5.G1. **Two probe streams are green un-ignored as of T5.J2.**
**5.2's structural work is DONE (session M).** The rename landed at **T5.M1**
(`SDqLayer` → `DqLayerState`; the census line was **removed**, not re-keyed to `x1` — the
instrument only reports names declared more than once, so 60 → **59** allowlisted); the
scratch-cache re-points at **T5.M2** (**45** parameters, not 32 — `parse_mb_syn_cabac.rs`
26, `parse_mb_syn_cavlc.rs` 10, `mv_pred.rs` 9); `pBitStringAux` at **T5.M3**.
**What 5.2 still owes: its straggler sweep, and one family** — the `*mut u8`
non-zero-count caches, **167 uses across 18 functions, 96 of them in `decode_slice.rs`**,
which makes it **5.6's** by P1 unless a later session decides otherwise. `SHIM(` did
**not** fall: `cabac_decoder.rs`'s `cabac_rbsp_window` did not die with `pBitStringAux`,
because its 18 callers take `pCtx` and nothing else; it retires when 5.6 converts them.)

**The perf question D-perf-5 asked is answered, and the answer is negative** (session I
log §2–§4; `perf_baseline.md` §Phase 5). The window retrofit does mechanically what it
claims — 76 bounds checks per macroblock on the CAVLC path become 38 — and recovers
**nothing**: decode median +0.14% at 7 pairs against a bar of −0.66%. Disassembly and
profile say why: those functions are 1–3% of decode time, so a 2% recovery was never
there. Two further facts:

* **The flip's cost re-measured at half.** The same two stashed binaries, same 7-pair
  protocol, a different day: +0.65% decode median / +1.18% CB, against session H's
  +1.32% / +2.05%. Real on every row in both readings; unstable in magnitude. D-perf-5's
  cumulative arithmetic (CB ≈ +19.9%) is high by roughly a factor of two.
* **That null result does not transfer to the hot families.** The eleven retrofitted are
  one scalar per macroblock. `pNzc` (`[i8; 24]`), `pMv`/`pMvd` (`[[i16; 2]; 16]`) and
  `pScaledTCoeff` (`[i16; 384]`) are indexed *inside* the record in inner loops, where the
  element re-type — already built at T5.H2 — makes the inner index const-bounded.

**No window hoisting** (S8's fourth negative result). Flip as `MbArray<[T; K]>` from the
start and hoist nothing that is not already the C++'s shape.

~~**The hot families are flipping and none of them costs anything the bench can see.**~~
**Corrected at session L, and this is the correction to read before quoting any
per-family number.** `pRefIndex` (T5.J3) read +0.03% decode median at 7 pairs, `pMv`
(T5.K1) **−0.13%**, `pMbType`+`pSliceIdc` +0.04% — each written up as free. Session L's
last **seven** families, measured as one span, read **+1.27% CB / +0.52% decode median**,
every row above the null band's ceiling. The per-family readings were not merely noisy,
they were **systematically under-reporting**: an effect at the resolution limit hides one
family at a time rather than averaging out. Encode is flat on every span, as a
decoder-only change should be.

**But the day-two confirmation session K ran is the result to carry, and it is about the
instrument rather than about `pRefIndex`.** Three 7-pair readings of T5.J3's one span, on
two days, read **+0.03%, +0.27% and −0.13%** — they disagree in sign. The null bands they
were judged against disagree at least as much: +0.13…+0.45% (session J, 3 pairs),
−0.16…−0.02% (session K, 3 pairs), −0.22…+0.25% (session K, **7 pairs**, matched to the
verdict). **Both the measurement and the yardstick move by more than the effect.** Three
consequences, all binding on what follows:

* **A verdict at N pairs must be judged against a null at N pairs.** Session K's 3-pair
  null was 0.14 points wide and entirely on one side of zero; its 7-pair null is 0.47
  points wide and straddles zero. Judged against the former, the confirmation read *fail*.
* **Per-family perf attribution is below this harness's resolution and no reachable pair
  count fixes it.** The number that decides the plan is ≈0.3% CB per family — 3 points of
  headroom over ten families — and that is the size of the noise.
* **So measure the aggregate.** The stop-line gates on *cumulative* CB, not on a sum of
  per-family numbers each below the floor. **Session L ran it** (`perf_baseline.md`
  §Session L): the whole 22-family flip, `seat_head` 3c4c6f4e → `l_c2` f63e8ef6, both
  stashed, **CB +2.93% at 7 pairs** against a null band 0.16 points wide — resolved
  without difficulty, exactly as predicted, and it is the number every later reading
  should be composed against rather than summed into.

**Cumulative CB is ≈ +20.7%** against the ≈+23% stop-line — Phase 4a's +17.8% plus the
directly measured whole-flip span — leaving **≈2.3 points of headroom** for 5.2's
remaining work and all of 5.3–5.6. The old summed figure (+19.2…+20.1%) was **not** high
by a factor of two, as session I's one re-read suggested; measured whole, the flip lands
slightly above it. The whole-flip reading owes a day-two confirmation (S2b) and it is
session M's first item.

**Where the flip stands: it is done (session L).** The arithmetic is **22 arrays, not
24** — F33 deleted `pNzcRs` and `pInterPredictionDoneFlag` at T5.H1, neither of which has
a reader in either tree. **All 22 are flipped**: `pIntraNxNAvailFlag`,
`pIntra4x4FinalMode`, `pResidualPredFlag`, `pChromaPredMode`, `pCbfDc`, `pLumaQp`,
`pNoSubMbPartSizeLessThan8x8Flag`, `pTransformSize8x8Flag`, `pCbp`,
`pMbRefConcealedFlag`, `pSubMbType` (session H), **`pRefIndex`** (T5.J3), **`pMv`**
(T5.K1), **`pMbType`** (T5.K2), **`pSliceIdc`** (T5.K3), **`pNzc`** (T5.L1),
**`pChromaQp`** (T5.L2), **`pMvd`** (T5.L3), **`pDirect`** (T5.L4), **`pScaledTCoeff`**
(T5.L5), **`pMbCorrectlyDecodedFlag`** (T5.L6), **`pIntraPredMode`** (T5.L7).

Session L's heat re-derivation, kept because the *method* is what the next family-shaped
job reuses (25s of `/usr/bin/sample` over the decode bench, 18098 self samples, the
bench's own SHA-1 excluded; a family's heat is the summed self time of the functions
holding its **layer-qualified** accesses, `SPicture`'s namesakes excluded):

| family | heat | layer sites | align after flip | bridge? |
|---|---|---|---|---|
| `pNzc` | 2.17% | 32 | 1 | no — shared borrow |
| `pChromaQp` | 1.88% | 29 | 1 | no |
| `pMvd` | 1.36% | 40 | 2 | no |
| `pDirect` | 0.76% | 16 | 1 | no |
| `pScaledTCoeff` | 0.45% | 11 | 2 | no |
| `pMbCorrectlyDecodedFlag` | 0.40% | **16, not 18** | 1 | no |
| `pIntraPredMode` | 0.10% | 16 | 1 | no |

**Two of session K's numbers were wrong and the S24 re-grep caught both**: the #5/#6
order (`pScaledTCoeff` is above `pMbCorrectlyDecodedFlag`, not below) and
`pMbCorrectlyDecodedFlag`'s site count, whose extra two were `SPicture`'s namesake in
`pic_queue.rs`. The counting rule that reproduces every family exactly is: **occurrences
of `.pXxx` whose receiver is not a picture**, comments stripped.

**Not one of the seven needed an S28 raw bridge**, which is the fact to carry into 6.3's
equivalent: a family needs `mb_grid_ptr` only when a consumer indexes the base at
*another macroblock's* address (`GetMbType`, `pCbfDc`), and none of these do — they index
inside their own record, and the two that hand out a base (`GetPNzc`,
`error_concealment.rs`'s flag loops) were read-only, where a shared borrow or per-use
indexing serves.

Each is one commit; the seat and the accessor are already in place, so a family commit is
the flip and nothing else. **All eleven of session H's families carry per-MB window
accessors as of T5.I1**, and F34 — the one defect the retrofit's analysis turned up — is
fixed at T5.I2.

**The alignment column is not decoration (F35).** `WelsMallocz` hands every raw array
16-byte alignment; `MbArray<T>` hands it `align_of::<T>()`. Consumers that store wider
than the element — `SetRectBlock`, `CopyRectBlock4Cols` — were legal only on the
allocator's accident. They were converted to the unaligned spelling at T5.J1, ahead of the
families that would have tripped them, but **every family commit greps its own consumers
for a wider-than-element access before flipping.**

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
  **"The probe" is now a pair, and the quiet was the stream's, not the code's (T5.J2).**
  `narrow_16x16.264` is one macroblock per frame, so `iLeftAvail`/`iTopAvail` are 0
  everywhere in it and **no neighbour-reading path had ever run under the checker** —
  which is what F34 cost, and F35 after it.
  `decode_slice_loop_runs_over_a_macroblock_grid_under_the_aliasing_checker` decodes
  `grid_48x32.264`: 3x2 macroblocks, CABAC, High with the 8x8 transform, I/P/B slices,
  panned so the MVs are non-zero. It found **F35 on its first run**. Coverage is proven
  the F21 way — revert T5.I2 in a scratch worktree and it goes red at
  `parse_mb_syn_cabac.rs:1242` while the small probe stays green. It costs **258.7s**
  against the small probe's 250.7s, so the pair is ~510s and `miri --lib` is ~13 min;
  both stay at `full` level. Keep the small one: it is the cheap verdict for everything
  that needs no neighbour.
  It is the one asset built by ffmpeg/libx264 rather than by the C++ encoder, because
  **OpenH264's encoder has no `transform_8x8_mode_flag` to write** and F34 sits behind
  it — `rust/tools/make_narrow_assets.py` carries the argument and the command line.

## 3. Step 5.3 — Neighbor & MV

`mv_pred.rs`: punning → byte ops; `SetRectBlock` → typed generic on the grid;
colocated reads via `cur_and_ref`.
- ~~**F22 unifies here**~~ **F22 is CLOSED (T5.M4, session M)**: eight duplicate
  function pairs became one each, home = the C++'s home, resolved per function — three
  kept `mv_pred.rs`'s `pDec` guard, `Update8x8RefIdx` lost the one the port had *added*,
  and `UpdateP8x8RefIdxCabac` moved to `parse_mb_syn_cabac.rs` (its C++ home) shedding two
  more added guards and one dead parameter. Zero bytes moved. It was unblocked by T5.M2,
  exactly as the finding predicted. **What 5.3 still owes**: 5.3b — **279** `LD*`/`ST*`
  punning sites in `mv_pred.rs` plus 21 in `parse_mb_syn_cavlc.rs`, and
  `SetRectBlock`/`CopyRectBlock4Cols` → typed generics on the grid, whose call sites are
  mostly inside `GetColocatedMb`, so the colocated reads want doing first. Historical
  detail below.
- **The colocated reads did not land at session N, and the reason is structural rather
  than a shortfall.** They were fenced out of session M "because they borrow two pictures
  at once and need `PicPool`'s split borrow", and `PicPool` landed — but it cannot supply
  that borrow while `pCtx->pDec` is a raw alias into a slot (§1 step 5). **So 5.3's
  colocated face is blocked on `pDec` carrying a `PicId`, which is 5.5/5.6-sized**, and
  5.3b sits behind it. What did land is the face's S25 enumeration as a runtime check
  (T5.N5): `GetColocatedMb`'s picture is never the picture being decoded, asserted rather
  than argued, green across 341/341 in both profiles. That assert is also the split
  borrow's precondition, so it is not throwaway.
- The reachability question is **answered** (T5.D1):
  `pCurDqLayer->pDec` cannot be null on the CABAC parse path — one writer, one call
  site, dominated by a prefetch-or-return, and the same in the C++. So the guard is
  dead code in both trees and this is a divergence, not a latent crash. Two things
  follow for the unification, both from session D's S24 re-grep of the C++ side:
  the divergence is **three** functions (`UpdateP16x16MotionInfo`,
  `UpdateP16x8MotionInfo`, `UpdateP8x16MotionInfo`), which unify onto `mv_pred.rs`'s
  guarded shape; and `Update8x8RefIdx` runs the other way — C++ has no guard, the
  CABAC copy is faithful, and `mv_pred.rs`'s added guard comes off. Per-function, not
  per-module (S21).

## 4. Step 5.4 — Deblocking driver (**CLOSED, session N**)

`SDeblockingFilter` holds `PicId`s (T5.N4: `ref_ids`, a snapshot of both reference
lists, replacing a raw pointer per list held into `pCtx->sRefPic` for the whole
macroblock loop — F28's shape). Identity compares per P3 in all six boundary-strength
helpers, and the `*mut c_void` erasure the 4x4 paths used is gone with it.

**No per-MB plane cursors were needed.** `pCsData` and `iCsStride` — the decoder's last
plane-pointer mirror — died at **T5.N3** with *nothing* replacing them: all three readers
take `pCurDqLayer`, which carries `pDec`, so each derives what it needs per use. T5.M3
needed one accessor because its readers had only `pCtx`; this needed none. The freshness
check T5.M3's lesson demands is a `debug_assert!` at filter init, green across both sweeps
and every golden.

The three deblocking identity tests gated it and now pin a narrower thing on purpose: the
POC half is **structural** (these helpers no longer receive a picture, so a POC-based
rewrite cannot compile), and what they assert is that the reference term is consulted and
the MV term does not mask it.

## 5. Step 5.5 — decoder_core.rs (**IN PROGRESS, session O.** The closure is computed
and does not need recomputing — session O log §1)

Allocation → constructors, paramset store (P4), context decomposition (§2.2.6),
`Drop` teardown, `Default` derives.

**The closure's verdict, so it is not re-derived: §2.2.6's decomposition is not 5.5's.**
S20's two propagating legs are *empty* here — no decoder-internal struct carries an
`assert_size!` or an offset pin (`api/abi_guard.rs` pins only public API types), and
the context is never embedded by value — so an owned field on the context drags nothing
with it and the constructors never waited on the decomposition. What binds is the
signature leg: **186 `pCtx` parameter positions over 11 files, 148 distinct functions
(59 leaves, 89 not), 1343 `(*pCtx).field` accesses over 13 files.** That is a bottom-up
sweep gated per function by S25, and it is sequenced **behind the `pDec` step**, because
`dec_state` and `dpb` cannot be split-borrowed while `pDec` aliases a pool slot. The
context's 25 raw-pointer fields are classified by owner in the log; **eight of them are
`CWelsDecoderImpl`'s members and are Phase 8's**, which is why the context cannot simply
become `Decoder`.

**Done at session O**: the CABAC engine stops being an allocation (T5.O3); the access
unit owns its NAL nodes and one allocator replaces two (T5.O4/T5.O7, F39); **F37**
fixed, **F38** fixed (eight `&mut`-derived back-pointers in `src/api/`), **F40** fixed,
**F41** listed for Phase 8.

**Left**: `pAccessUnitList` → `Option<Box<_>>` (53 sites — the one free-cascade entry
T5.O4 unblocked, because a node pointer now comes from the node's own allocation);
**P4**, sized at **208** `.pSps`/`.pPps` occurrences over nine files and four carriers
(context 134, `SSliceHeader` 48, `SLayerInfo` 14); and the rest of the cascade, which
waits on `pDec` with everything else.

- S21 with force: the decoder context embeds its buffers **by value**, so every
  owned field lands inside `mem::zeroed` reach. The `MaybeUninit` shell +
  `new_boxed()` is at `decoder_context.rs`; **extend it per owned field — and stop
  expecting to delete it.** It exists because the context is several MiB and a by-value
  constructor overflows a 2 MiB test-thread stack, not because of its owned fields
  (`make_zeroed_shell_valid` writes exactly two). `new_boxed` **is** the real
  constructor. Corrected at session O; the earlier text promised a deletion that
  nothing in 5.5 can perform.
- F19's check runs here, per allocation. Session O's inventory, with the line that
  frees each, is in its log §1 — reuse it rather than re-deriving it.
- **A container may not lend a borrow while raw aliases into it are live** (T5.N1,
  re-derived at T5.O7 by a probe). Keep the slots raw and put the ownership in `Drop`,
  or schedule the aliases' removal in the same closure. This is what blocks
  `pDqLayersList`, `pPicBuff` and `pTempDec` from becoming owned.
- `SWelsDecoderContext` has no `assert_size!` and no offset pins; don't look for
  an instrument that isn't there.

## 6. Step 5.6 — decode_slice.rs (last, per P1)

Including the EC MC paths. Delete the remaining decoder shims. Decoder modules get
`#![deny(unsafe_code)]` one by one. No `SBitStringAux` shell exists (deleted at
T3.4).

## 7. Gates and exit

**D-gate-1 (sprint gating, from session O until the phase exit — §7.4 is the
authority):**
- Per commit (~3 min): build both profiles + tests + ratchet + census.
- Once per session, at close: the full battery — decode goldens frozen at **57**
  (the three F21 rows and session J's `grid_48x32` included; a new probe stream's
  row is additive and named in its own commit, nothing else moves); sweeps 341/341
  both profiles; benches bit-identical; Miri both probes. FAILs adjudicated per
  S14/S16; a byte divergence bisects across the session's commits — keep them
  small (S20).
- **No mid-phase perf measurement.** Nulls, pairs, spans, and every S2/S2b verdict
  move to the phase exit, which runs the full protocol against the ≈+23% stop-line.
  (Sessions A–N predate this and ran per-face/per-session protocols; their numbers
  stand as data.)

**Do not edit the working tree while a battery is running** (session J's small
self-inflicted lesson): `gates.sh` builds from the working tree, not from the commit,
and an edit that lands mid-build silently moves what the Miri step is measuring.

Exit: frame-count parity and the `#[ignore]` set unchanged; T3.0's 2316-row golden
table green in both profiles (T7 stays deferred); decoder `src/` unsafe-free.
**One named shim survives the phase**: `SPicture::data_ptr`'s consumer at
`decoder_core.rs:~1087` fills the public output contract (`pointer, stride`) and is
not a kernel adapter — 5.6 cannot delete it; it retires with the API boundary work
(Phase 8). The straggler sweep expects exactly this one; its marker text names the
owner.
**every §7.4 ledger entry whose shims died in this phase must clear**. This phase
collects 4a's downgraded decode rows (≈ +17.8/+10.1/+9.6% cumulative at 4a's exit).
**Measured position: CB ≈ +21.6…+21.9%**, and it moved at session N. The 5.2 flip's
+20.4…+20.7% is settled (two days, +2.93% and +2.57%, null band 0.17 points wide) and is
not re-derived from per-family sums; **session N added +1.24% CB on top of it** — its
whole span at 7 pairs, every decode row above a null band of −0.14%…+0.34%. That leaves
**≈1.1…1.4 points under the ≈+23% stop-line** for 5.5 and 5.6, the phase's two largest
remaining steps. Session M's own four faces read **−0.77% CB**, below the null floor on
every row, so 5.2's tail and 5.3a cost nothing; **session N's are the first that did**.
**Session N's reading is D-gate-1-deferred to the exit** — the binaries are stashed as
`.perfpair/n_base`, `n_mid`, `n_head`. (The day-two confirmation was session O's brief
§0 item before D-gate-1 took every mid-phase measurement to the exit; session O ran
none, per its brief's own §0.3.) **Session O added no perf reading and three structural
changes the exit's ledger row measures through**: `Id`'s niche (T5.O0), the CABAC
engine as a field (T5.O3), and the access unit's owned node list (T5.O4/O7). Its bisect
puts the whole cost in Face 3 (the deblocking driver, +1.17% CB / +2.25% Main / +2.06%
High) and none in Face 1 (−0.72% CB, CABAC rows inside the band); the unverified mechanism
and the one-build experiment that settles it are in `perf_baseline.md` §Session N. The mechanism is constant dimensions reaching the
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
  T5.E1 made in the other direction) → **4604** (session G: +15, the same trade again —
  11 `&mut *pCtx` bindings and 13 nested borrows became raw derivations, and each one
  costs a pointer type annotation; T5.G2 was flat) → **4570** (session H: **−34, and the
  first decrease of the phase that comes from conversion rather than deletion** — 22
  field declarations, 22 allocations and 22 frees against two new S28 bridges, which cost
  +1 `raw_ptr` each), `unsafe_block` 613 → 618 → 616 →
  613 → 614 → 614 → 619 → **622**, `unsafe_fn` 1250 → 1249 → 1248 → 1247
  (`InitCurDqLayerData` deleted) → 1248 (`cabac_ctx_base`) → 1249 (`bytes_copy`) →
  **1248** (`CheckIntraChromaPredMode`, retired *by the flip* — its `*mut i8` became a
  `&mut i8` and its body stopped needing `unsafe`). **`mem_zeroed` 32 → 31** at T5.H3:
  the decoder's layer stopped being constructible by zeroing.
  **Session I is flat** — every metric and every per-file row unmoved, which is what
  accessor reshaping looks like: it converts safe indexing into safe borrows and deletes
  no pointer.
  **Session J: `raw_ptr` 4570 → 4550 (−20), `unsafe_block` 622 → 623 (+1).** Per file:
  `mv_pred.rs` −16 (T5.J1's 13 unaligned-pun rewrites — `*(x as *const i32)` names two
  pointer types, `LD32(x)` names none), `decoder_core.rs` −3 and `deblocking.rs` −1
  (T5.J3's field, two allocations, free block, and the `as *mut _` fallback). **The one
  increase is a test** — the S28 full-reach test the new bridge owes; no production
  `unsafe {}` was added. Both decreases are conversion rather than deletion.
  **Session K: `raw_ptr` 4550 → 4540 (−10), `unsafe_block` 623 → 625 (+2).** Per file,
  and this is the whole story: **`decoder_core.rs` −9, `deblocking.rs` −1, and every
  other touched file flat** — `mv_pred.rs`, `decode_slice.rs` and
  `parse_mb_syn_cavlc.rs` are unmoved across **32 converted sites** between them, because
  these conversions delete pointer *dereferences* and `raw_ptr` counts pointer *types
  written*. Both `unsafe_block` increases are the S28 full-reach tests T5.K1 and T5.K2
  owe; no production `unsafe {}` was added. T5.K3 is the first family of the flip that
  leaves **no raw derivation at all** and so owes no test.
  **Session L: `raw_ptr` 4540 → 4507 (−33), everything else flat.** Per file:
  `decoder_core.rs` −22 (seven field declarations, eight allocations, seven free blocks),
  `parse_mb_syn_cabac.rs` −5, `parse_mb_syn_cavlc.rs` −4, `decode_slice.rs` −2 — and
  `mv_pred.rs` **flat at 274 across 25 converted sites**, `deblocking.rs` flat,
  `error_concealment.rs` flat across four deleted base-pointer bindings, for the reason
  session K recorded. `unsafe_block` **625, unchanged**: no production `unsafe {}` for a
  third session, and for the first time no S28 test was owed either, because none of the
  seven families needed a raw bridge.
  **Session M: `raw_ptr` 4507 → 4460 (−47), `unsafe_fn` 1248 → **1241** (−8),
  everything else flat.** Per file: `parse_mb_syn_cabac.rs` −27, `parse_mb_syn_cavlc.rs`
  −10, `mv_pred.rs` −10, `decoder_core.rs` −2, `bit_stream.rs` **+2** — the one increase is
  T5.M3's `slice_bit_reader`, which replaces a deleted field, and T5.M1's rename is
  **invisible in the table, which is its evidence**: a rename writes no pointer type and
  deletes none. **`unsafe_fn`'s −8 is the shape S16 predicts and the first of the phase**:
  it stays ~flat while raw bodies are strangled and falls hard when they are *deleted*, and
  F22's eight duplicate definitions are the first such deletions here
  (`parse_mb_syn_cabac.rs` −7, `mv_pred.rs` −1). `unsafe_block` **625 for a fourth
  session**, no S28 test owed for a second.
  **Session O: `raw_ptr` 4436 → 4420 (−16), `unsafe_fn` 1247 → 1245 (−2),
  `unsafe_block` 622 → 627 (+5).** Per file: `decoder_core.rs` −10 (the deleted
  duplicate allocator pair and the CABAC engine's alloc/free), `nalu.rs` −5,
  `decoder_context.rs` −1 — and `parse_mb_syn_cabac.rs`, `decode_slice.rs`,
  `deblocking.rs` and `codec_api.rs` **flat across 55 converted sites between them**,
  session K's observation for a third session. `unsafe_fn`'s −2 is S16's shape: it
  falls when raw bodies are *deleted*, and F39's duplicate pair is the deletion. Of the
  five `unsafe_block` increases, **four are tests** and the fifth is `DestroyPicBuff`'s
  reset.
  **S16's prose floor has now been collected ten times, three of them inside session
  O** — a deleted field's own explanatory comment, a test's explanation of the metric,
  and the sentence explaining why not to write one. Session N's form (*a comment
  mourning a deleted pointer type reinstates it*) now has a corollary: so does a
  comment explaining the rule.
  **S16's prose floor has now been collected eight times** (session L's was a doc comment
  in `test_need_error_con` naming the pointer type its own flip had just deleted, which
  the per-file map caught and the total would have hidden; session H added three, two of
  them on `mem_zeroed` again and one putting `raw_ptr` at 1 in a `forbid(unsafe_code)`
  file); before session H it had been collected four times — `raw_ptr` and `SHIM(` at
  session C, `mem_zeroed` at T5.G1 (a doc comment naming the zeroing intrinsic, which
  would have corrupted S21's live construction-audit count) and `raw_ptr` again at
  T5.G2. Every one was reworded rather than baselined. Read per-file deltas, not
  totals: a one-line prose delta and a real conversion look identical in the total.
- Gates: **474 / 468 / 20**, Miri **334** at session O's exit (session N +2, session O
  +4: two AU-list tests, the F37 cycle test and the `Id` niche pin). Historic row below
  is session M's: **468 / 462 / 20**, Miri **328** — *unchanged across session M, which
  added no test because no conversion in it needed one* — census **59** (session M: the
  `type SDqLayer x2` line went away with the rename, and the duplicate-body budget fell
  **198 → 195** on F22's unification, its first decrease since the instrument was widened).
  Miri history (session G: +1, the aliasing probe now runs
  un-ignored — see §2; session H: +12, `MbGrid` and S28's reach tests; session I flat;
  session J: +2, the grid probe and `pRefIndex`'s reach test; session K: +2, `pMv`'s and
  `pMbType`'s reach tests), sweeps 341/341
  both profiles, decode goldens **57 rows** (session J: +1, `grid_48x32`, additive and
  named in its own commit). The debug sweep can reproduce F3 (`rust_enc`'s
  `[profile.dev] opt-level = 3` made it fast enough to lose the race); measurement 29
  is the cleanest evidence the finding has. Sessions G–I drew **zero** F3 hits across
  5456 configurations and session J drew **one**, acquitted at measurement 33 — a clean
  sweep is a sample, not a signal, and so is a dirty one (S14 step 4).
  **Session K drew four — two per battery, one battery per profile — and alternated
  twice** (measurement 34): control 8 vs head 9 over 2880 configurations per side, so
  HEAD is not worse. Two things it settled that outlive this session: **the rate under
  sustained back-to-back presets is ≈1/307, not ≈1/800** (load is part of the
  signature, and twenty-four presets with nothing between them is the most load the
  sweep has run under), and **`n=600` is a rate artifact rather than a condition** — the
  first `n=1500` hit in 34 measurements, explained by `sm=3`'s `n` being the per-slice
  byte budget, so a smaller `n` cuts more slices and performs more of the slice-list
  growths the race lives in.
  **Session L drew one** (debug, measurement 35) and its isolation re-run **reproduced it
  twice in ten** — the second reproduction in the finding's history and the same
  configuration and wrong length as measurement 29's, so `320x192 t=4 sm=3 n=600 cabac=1`
  in the debug profile is *the* susceptible configuration. One hit, so no alternation
  (S14 step 1); acquitted. Miri **328** unchanged across the session: seven families and
  not one new test, because not one needed a bridge.
- **Miri skips are 2, not 3**: `wels_thread_pool` (F12, Phase 7) and `encoder_ext`
  (F13, Phase 6). `manage_dec_ref` came off at T5.B2.
- Census gate state: inferred-target double casts 0, duplicate-body groups 198
  (ratcheted), **60** allowlisted entries (session E: the decoder's `SMbCache` was
  deleted, so its class-(a) line is gone — a deletion removes the entry where a
  rename would only re-key it). Remaining within-decoder entries are allowlisted
  with their owning steps.
- `SHIM(` **159**: `phase3` = 2 (Phase 6's), `phase5` = **3** — `cabac_decoder.rs`'s
  (**not** dead in 5.2, corrected at session M: `pBitStringAux` died at T5.M3 and
  `cabac_rbsp_window` outlived it, because its 18 callers in `parse_mb_syn_cabac.rs` take
  `pCtx` and nothing else; it retires when **5.6** converts them), `decoder_core.rs`'s
  `mb_grid_ptr` (T5.H2, S28's raw bridge; dies as the
  families that hand a pointer to a kernel convert) and `SPicture::data_ptr` (dies as
  5.2–5.6 convert the kernels,
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
