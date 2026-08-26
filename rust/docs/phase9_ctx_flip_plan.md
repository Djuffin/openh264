# Phase 9 — the `sWelsEncCtx` flip: stage plan (session H, step 1)

*Written before the first flip commit, so it can be falsified by one (S60).
**Status at session H's stop: phase A is 111 of 154 bodies done across five stages
(A0-A4), every stage green, the remaining 44 blocked behind the init path. Phase B
has not started; the ten accessors are all still raw. See the postscript.**
 Every
number was re-derived at H's step-0 commit with the command beside it; the tools
are the authority, this is the snapshot (S24).*

## The three lists, and why they are three and not two

| | bodies | end state |
|---|---:|---|
| in-fork | **111** | permanently raw, or interior mutability (S63) |
| ST, flips | **154** | `&mut sWelsEncCtx` |
| ST, stays raw for borrow reasons | **1** (`ParasetStrategy`) | permanently raw — F166 |

`python3 rust/tools/phase9_forksplit.py` reads 266 / 111 / 155. The third row is
the correction this plan adds: **the ST column is a candidate list, not the flip
list.** Fork-reachability is necessary for a body to stay raw; it is not
sufficient to make the rest flippable. `ParasetStrategy` is ST, and flipping it
turns ten call sites that are sound today into borrow errors with no local fix
(F166).

## The top boundary is available, and taking it costs one thing

The context is owned as `Option<Box<sWelsEncCtx>>` (`wels_encoder_ext.rs:694`),
not as a leaked raw pointer, so the flip's root stage needs **no raw laundering
at the top**: a root taking `&mut sWelsEncCtx` is called with `ppCtx.as_mut()`'s
`&mut Box<sWelsEncCtx>` reborrowed, and the owning `Box` is the only root
protector. There are **0** bodies taking `&mut sWelsEncCtx` today — the flip is
greenfield and its first commit creates the first one.

**But minting that first `&mut` is not free, and the cost is not in the flip's
own derivations.** The API layer reaches the context today with
`std::ptr::addr_of_mut!(**pEncContext)` (`wels_encoder_ext.rs:711`) — a raw place
with no intermediate reference, chosen deliberately (T8.B5/S42). Every raw
derivation of the context is therefore a *sibling* under the `Box`'s tag, and
siblings coexist. Replacing that with a `&mut` puts a `Unique` at the top of the
allocation's stack and pops every raw sibling stored earlier. There is exactly
one such stored sibling — `CWelsPreProcess::m_pEncCtx` — and **stage A0 must
carry its remedy in the same commit that mints the first `&mut`** (F167). This
is the one place where the campaign creates a hazard rather than revealing one,
and it is why A0 is not the trivial stage its body sizes suggest.

## How big the flip actually is

Measured by brace-matched body extents over the tool's ST column:

| | |
|---|---:|
| ST bodies to flip | **154** |
| lines of body | **7 143** |
| `(*pCtx)` derefs to rewrite | **517** |
| ST-accessor call sites to re-spell (phase B) | **90** (68 in ST bodies) |
| stored raw copies of the context to remedy | **1** (`m_pEncCtx`, F167) |

The distribution is lopsided and one body dominates: **`WelsEncoderEncodeExt`
is 884 lines and 114 derefs — 22% of the campaign's derefs in a single depth-0
root.** The next heaviest is 24. Stage A0 therefore is not "twelve small roots";
it is one large root and eleven small ones, and it is the right place to split a
stage if any stage needs splitting.

By file: `encoder_ext.rs` 191 derefs, `rc.rs` 109, `ref_list_mgr_svc.rs` 109,
then a long tail (`wels_preprocess.rs` 22, `svc_encode_slice.rs` 18,
`encoder_context.rs` 27, `wels_encoder_ext.rs` 27, `slice_multi_threading.rs` 8,
`paraset_strategy.rs` 6, `svc_mode_decision.rs` 0, `deblocking.rs` 0).

**This is not a one-session step**, and the brief's step 2 ("the flip, staged,
each stage green") is written as though it were. Recorded here so the frontier
this session stops at is read against a measured total rather than an assumed
one.

## The borrow graph

Depth = longest path from a root through the ST-only call graph
(`stgraph.py`, this session's scratch tool; roots are ST bodies with no ST
caller). 12 roots, max depth 12.

```
depth  0: 12   CreatePreProcess, FilterLTRMarkingFeedback, FilterLTRRecoveryRequest,
               InitFunctionPointers, InitMbInfo, InitSliceInLayer, OutputCurrentStructure,
               WelsEncoderEncodeExt, WelsEncoderEncodeParameterSetsRust,
               WelsMdInterFinePartitionVaaOnScreen, WelsRcDropFrameUpdate, WelsRcInitModule
depth  1: 32   … AppendSliceToFrameBs, WelsInitCurrentLayer, PreprocessSliceCoding, ctx_mb_index_x/y
depth  2: 20   … DecideFrameType, UpdateFrameNum, set_current_layer, ctx_dq_idc_map
depth  3: 10   … WelsMarkPicScreen, ReallocSliceBuffer, ctx_pps
depth  4: 11   … WelsUpdateRefList, WelsMarkMMCORefInfoScreen, ExtendLayerBuffer
depth  5: 13   … LTRMarkProcess, DeleteInvalidLTR, GenerateNewSps, ctx_frame_bs_cur
depth  6:  7   … AfterBuildRefList, SpsReset, ctx_frame_bs
depth  7:  7   … InitPps, WelsGenerateNewSps, RcCalculateCascadingQp
depth  8: 10   … WelsBuildRefList, WelsMarkPic, UpdateSrcPicList
depth  9: 22   … WelsMarkMMCORefInfo, CheckCurMarkFrameNumUsed, the Rc* calculators
depth 10:  7   … SetRefMbType, RcInitVGop, ctx_sps
depth 11:  2   WelRcPictureInitScc, ctx_ltr_at
depth 12:  1   ctx_ltr
```

**The scheduling fact that shapes the plan: the ten ST accessors sit at the
bottom of this graph, not the top.** `ctx_ltr` is the single deepest body in the
family. A literal root-down campaign therefore reaches the accessor-coexistence
crux only in its last three commits — and reaches all of it at once, in bodies
that were flipped ten stages earlier and are no longer in hand.

## So the flip runs in two phases, not one

### Phase A — flip the 154, leaving every accessor raw

Stages A0…A12, one per depth level, one commit each, `gates.sh family` green
before the next.

**Why the order is required and not merely tidy.** In phase A a boundary is
spellable in both directions, so a naive reading says any subset may flip in any
order. It may not. A *not-yet-flipped* caller reaching a *flipped* callee has to
mint `&mut *pCtx` from its own raw parameter — a fresh Unique retag on the
context's own allocation, and any other ctx derivation that caller is holding is
popped by it. That is F66's hazard verbatim, and it is the one thing this
campaign must not manufacture. Root-down never mints: the roots take their `&mut`
from the owning `Box`, and every flipped body hands its own `&mut` downward.
Depth-as-longest-path-from-a-root is exactly the property that makes this hold —
if a body is at depth *k*, every one of its ST callers is at depth < *k*, so by
the time its stage runs they are all flipped.

Within phase A **nothing holds a checker-visible borrow of the context**: every
derivation still goes through an accessor that takes `*mut sWelsEncCtx`, so each
call site becomes `ctx_ltr_at(ctx, did)` with `ctx` coerced —
`ctx as *mut sWelsEncCtx` — and the resulting pointer carries no lifetime tied to
`ctx`. Both boundary kinds are the same spelling:

* **ST → in-fork** (`ctx_param`, `ctx_func_list`, `ctx_dq_layer`, `current_layer`,
  …): raw **permanently**, S63.
* **ST → not-yet-flipped ST** (including all ten accessors): raw **temporarily**,
  until that callee's own stage.

Consequence: **phase A cannot produce a borrow error**, and a borrow error inside
phase A is evidence the plan is wrong about a body, not a stage to fight through.
That is this plan's falsifiable claim (S60), and it was run before the plan was
written rather than asserted — a standalone `rustc` model of the three
boundary kinds in one flipped body (reproduced in the appendix below) (a raw accessor derivation held live across a
permanently-raw callee call, an already-flipped callee call, and a write through
the held pointer): **compiles clean**.

The per-stage gate is the compiler + `gates.sh family` (583/583 both profiles) +
the join re-read per site — **not** q1c-to-zero (F163).

### Phase B — dissolve the ten accessors, one commit each, cheapest first

This is where the crux lives, and phase A's ordering is what lets it arrive in
ten controlled doses instead of one. Each stage dissolves one accessor into a
field path and repairs the collisions that surfaces in that accessor's caller
set — and **only** that set.

| stage | accessor | sites | dissolves to |
|---|---|---:|---|
| B1 | `ctx_mb_index_x` | 2 | `ctx.pStrideTab` → `tab.MbIndexX(did)` (boxed table, other allocation) |
| B2 | `ctx_mb_index_y` | 2 | as B1 |
| B3 | `ctx_pps` | 2 | `ctx.pPPSArray[ctx.iPps]` — **chains through in-fork `ctx_pps_array`** |
| B4 | `ctx_dq_idc_map` | 4 | `&mut ctx.pDqIdcMap[..]` |
| B5 | `ctx_ltr` | 4 | `&mut ctx.pLtr[..]` |
| B6 | `set_current_layer` | 4 | `ctx.iCurDqLayer = …` — **writer half of a split pair**, F165 |
| B7 | `ctx_frame_bs` | 8 | `&mut ctx.pFrameBs[..]` |
| B8 | `ctx_sps` | 18 | `ctx.pSpsArray[ctx.iSps]` — **chains through in-fork `ctx_sps_array`** |
| B9 | `ctx_frame_bs_cur` | 21 | `&mut ctx.pFrameBs[ctx.iPosBsBuffer as usize ..]` — **two disjoint fields** |
| B10 | `ctx_ltr_at` | 25 | `&mut ctx.pLtr[did]` |

90 call sites in all; **68 of them are inside ST bodies** and are the ones that
can collide. The other 22 are in bodies outside the ctx-parameter family
(`WelsSetMemZero_c`'s region, `FreeDqLayer`, `InitDqLayers`, `RequestMemorySvc`,
`FreeScaledPic`, `WelsEncoderApplyBitVaryRang`) and keep a raw context in hand;
they take the raw spelling unchanged.

**B9 is the shape to design against, and it is a better shape than expected.**
`ctx_frame_bs_cur` reads `pFrameBs` *and* `iPosBsBuffer` — two disjoint fields of
one struct, which is precisely the case the accessor existed to launder. Both
spellings compile (appendix): the scalar hoisted to a local
(`let pos = ctx.iPosBsBuffer as usize; &mut ctx.pFrameBs[pos..]`) **and** the
inline one (`&mut ctx.pFrameBs[ctx.iPosBsBuffer as usize ..]`), the latter
because two-phase borrows evaluate the index before activating the mutable
borrow. **No split-borrow helper is needed anywhere in phase B on current
evidence** — the brief's "split-borrow helper on the ctx" shape is available if a
site needs it, and no site found so far does.

**B3 and B8 are the shape that needs a ruling.** `ctx_sps` resolves `iSps`
against `ctx_sps_array`, which is **in-fork** and permanently raw. An ST caller
cannot reach the SPS "through the field path" without either (a) duplicating the
array projection as a direct field borrow — legal, since the ST caller holds
`&mut ctx` and `pSpsArray` is a plain `Vec` field, leaving the in-fork accessor
untouched for in-fork callers; or (b) keeping `ctx_sps` raw. **(a) is the
choice**: the two callers never run concurrently — ST callers run before and
after the fork, in-fork callers during — and a `Vec` field borrow and a raw
projection of the same field are the port's existing idiom either side of a fork
boundary. Recorded here so the commit that does it can be checked against a
stated intent rather than a discovered one.

## What is deliberately not in this plan

* The 111 in-fork bodies (S63) and the 17+ in-fork accessors — theirs is step 3's
  or the exit's story. Note the in-fork accessor count is a **lower bound**: the
  `ctx_` spelling finds 17, and `current_layer`, `layer_sps`, `layer_pps`,
  `slice_writer`, `dynamic_bs_buffer` are the same class under other names
  (F165).
* The send-seam (D-exit-2), F162's site (D-fid-2), the two recon-seam items
  (D-mt-3), `SCREEN_CONTENT(dormant)`, perf (D-gate-1).
* The fn-pointer typedefs. **Both deferred checks were run here rather than left
  to their stages, and both came back clean:**
  * **No typedef flips.** Nine `pub type` fn-pointer typedefs carry
    `*mut sWelsEncCtx` (`pJudgeSkipFun`, `PIntraFineMdFunc`, `PInterFineMdFunc`,
    `PInterMdFirstIntraModeFunc`, `PInterMdBackgroundDecisionFunc`,
    `PInterMdScrollingPSkipDecisionFunc`, `PInterMdFunc`,
    `PWelsCodingSliceFunc`, `PWelsSliceHeaderWriteFunc`; a tenth mention is the
    plain alias `SWelsEncCtx = sWelsEncCtx`). Every one is a mode-decision or
    slice-coding dispatch type, and every dispatch path is in-fork — so all nine
    stay raw and the brief's "the 13 typedef mentions flip with the stage that
    reaches them" is **vacuous**: no stage reaches one.
  * **No ST body is signature-pinned.** 16 ST bodies are declared
    `extern "C"` (14 in `rc.rs`, plus `PerformDeblockingFilter` and
    `WelsMdInterFinePartitionVaaOnScreen`), which would pin a signature if any
    were stored in a slot. **None is** — every one is reached by direct name,
    the RC family from `match self.eInstalledMode` arms rather than a table, so
    the `extern "C"` is vestigial. All 16 flip normally; `&mut T` stays
    FFI-safe (a non-null pointer), so no `improper_ctypes_definitions` arises.
  * The two duplicate names resolve uniformly and need no per-stage hand check:
    `WelsSpatialWriteMbSyn` is **in-fork at both definitions**
    (`svc_set_mb_syn_cavlc.rs:662`, `wels_func_ptr_def.rs:233`) so neither
    flips; `WelsRcPostFrameSkipping` is **ST at both** (`rc.rs:731` the
    dispatcher method, `rc.rs:2020` the free fn) so both flip in their stages.
    q1c's warning is about its own text scan; `forksplit` lists the two
    definitions separately with distinct line numbers and is not confused.

## Drop order

Phase A stops only at a stage boundary. If phase B cannot start, its ten stages
are the named frontier and the accessors stay raw — a tree in that state is
green and self-consistent, because phase A never depended on them dissolving.

## Appendix — the borrow model, run before the plan was written (S60)

Three standalone `rustc` files, ~30 lines each, modelling the shapes this plan
turns on. They are reproduced here rather than kept as scratch because a claim
about what will and will not compile is the plan's whole falsifiable content.

**(a) F166's collision — the shape that must NOT be flipped.** `strategy_raw`
models `ParasetStrategy`: raw in, unbound lifetime out.

```rust
pub fn strategy_ref(ctx: &mut Ctx) -> &mut Strategy { /* the flipped form */ }
pub unsafe fn strategy_raw<'a>(ctx: *mut Ctx) -> &'a mut Strategy { /* today */ }

unsafe { strategy_raw(ctx).update(ctx) }            // compiles — today's site
strategy_ref(ctx).update_ref(ctx)                   // error[E0499]
strategy_ref(ctx).update(ctx as *mut Ctx)           // error[E0499]  <-- the one to note
```

The third line is why F166 is a permanent ruling and not a repair: **passing the
argument raw does not rescue the site.** The coercion is itself a use of `ctx`,
and two-phase borrows do not reach it because the receiver is a function's return
value rather than an autoref. "Pass it raw at the boundary" is the remedy every
other collision in this campaign takes, and it is exactly the remedy that fails
here.

**(b) Phase A's falsifiable claim — all three boundary kinds in one flipped
body.** Compiles clean:

```rust
pub fn phase_a_body(ctx: &mut Ctx) {
    unsafe {
        let p = ctx_ltr_at(ctx as *mut Ctx, 0);  // -> not-yet-flipped ST accessor
        callee_raw(ctx as *mut Ctx);             // -> in-fork, permanently raw
        callee_ref(ctx);                         // -> already-flipped ST
        *p += 1;                                 // held live across all three
    }
}
```

The raw derivation carries no lifetime tied to the borrow, which is precisely why
phase A cannot produce a borrow error — and why a borrow error inside phase A
falsifies this plan rather than describing a stage to fight through.

**(c) F166's payoff — a flipped method reached through the raw accessor.**
Compiles clean:

```rust
pub fn site(ctx: &mut Ctx) {
    unsafe { strategy_raw(ctx as *mut Ctx).output_current_structure(ctx); }
}
```

One body staying raw is what lets the ten sites — and `OutputCurrentStructure`,
a depth-0 root reached through one of them — flip at all.

**(d) B9's two disjoint fields.** Both spellings compile, including the inline
one, because two-phase borrows evaluate the index before activating the mutable
borrow:

```rust
let pos = ctx.i_pos as usize; &mut ctx.p_frame_bs[pos..]   // hoisted
&mut ctx.p_frame_bs[ctx.i_pos as usize ..]                 // inline — also fine
```

So **no split-borrow helper is needed anywhere in phase B on current evidence.**
The brief's "split-borrow helper on the ctx (one `&mut self` method returning
disjoint `&mut` fields)" remains available if a site needs it; no site found so
far does.

---

# Postscript — what the plan got right and wrong, written at session H's stop

Five stages ran (A0–A4), each green on `gates.sh family` at 583/583 in both
profiles. **111 of 154 bodies flipped; 44 remain.** Census `266/111/155` →
`155/111/44`; `raw_ptr` 1587 → 1474.

## Right

**Phase A's falsifiable claim held for all five stages.** Every compiler error
across the whole campaign was a *contract* question, never a borrow error: 10 +
17 + 1 + 0 `is_null()` guards on a reference, and three type mismatches at
boundaries where a non-family caller had to start borrowing the owning `Box`.
The prediction was that phase A cannot produce a borrow error because nothing
holds a checker-visible borrow of the context, and it did not.

**Deferring the accessors to phase B was right for a reason the plan did not
give.** The plan argued it spreads the crux into ten doses. The stronger reason
appeared in practice: with the accessors raw, **a stage touches only the bodies
it flips** — signature and derefs, nothing else — which is what made 111 bodies
in five stages possible at all.

## Wrong

**"Root-down by depth level" is the wrong unit.** Depth is a property of the
static call graph; the flip's actual precondition is *every caller already
flipped*, which is a property of the current tree and changes after every stage.
The stages ran off a computed frontier instead, and their sizes — 1, 36, 32, 34,
9 — look nothing like the depth levels the plan tabulated (12, 32, 20, 10, 11).
**Replace "depth levels" with "recompute the frontier after each stage."**

**The boundary spelling was over-thought.** The plan and F169 both prescribed an
explicit coercion; Rust coerces `&mut T` → `*mut T` implicitly in argument
position and none was ever needed (F169's amendment).

**The plan said nothing about the top boundary bodies, and they are the gate.**
`WelsEncoderEncodeExt`'s caller could hand it a `&mut` for free. The init path
cannot: `WelsInitEncoderExt` owns the context as `Box::into_raw` and has no live
`Box` to borrow. **All 44 remaining bodies are behind that one edit**, and the
plan's tables — which counted only bodies *inside* the family — could not show it.
The next session's first move is that body, not a depth level.

## The remaining work, named

| | |
|---|---:|
| ST bodies still raw | **44** |
| — blocked behind the init path (`WelsInitEncoderExt` + `CWelsH264SVCEncoder` methods) | 42 |
| — dead, awaiting a ruling (`WelsRcDropFrameUpdate`, `WelsMdInterFinePartitionVaaOnScreen`) | 2 |
| permanently raw by ruling (`ParasetStrategy`, F166) | 1 |
| phase B accessors, none started | **10** |
| in-fork, permanently raw (S63) | **111** |

Phase B's table above is unchanged and still costed; nothing in phase A
invalidated it, because phase A deliberately never touched an accessor.
