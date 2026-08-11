# Phase 4b findings

Things found while executing Phase 4b of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— dispatch de-virtualization — that are *not* Phase 4b's job to fix at the moment they
were found, or that Phase 4b did fix and wants on the record. Numbering continues from
[`phase3_findings.md`](phase3_findings.md) (F15–F18); Phase 0's F1–F3, Phase 1's F4–F7
and Phase 2's F8–F14 are in their own files.

---

## F19 — The parameter-set strategy object was leaked on every encoder teardown; C++ deletes it and the port did not

**Status: FIXED 2026-08-11 (Phase 4b session B, T4b.2a), structurally rather than by
adding the missing call.** Found while computing T4b.2a's ownership audit — the
question "who calls `Box::from_raw` on this?" had no answer on the production path.

### The divergence

`encoder_ext.cpp:1994-2000`:

```cpp
FreeCodingParam (&pCtx->pSvcParam, pMa);
if (NULL != pCtx->pFuncList) {
  if (NULL != pCtx->pFuncList->pParametersetStrategy) {
    WELS_DELETE_OP (pCtx->pFuncList->pParametersetStrategy);
  }

  pMa->WelsFree (pCtx->pFuncList, "SWelsFuncPtrList");
  pCtx->pFuncList = NULL;
}
```

The port's `WelsUninitEncoderExt` had the outer `if` and the `WelsFree`, and **not the
inner two lines**. So `InitFunctionPointers` did `Box::into_raw` on a
`CWelsParametersetIdConstant` (`encoder_context.rs:725`), the table that held the
pointer was `WelsFree`d out from under it, and the object was never reclaimed.

`DestroyParametersetStrategy` existed and was correct. Grep for its callers found
**four, all in tests** (`au_set.rs`'s PPS-syntax test and three in
`paraset_strategy.rs`'s own module) and **none on the encode path** — which is why no
gate ever noticed. A leak fails no byte-exactness test, no sweep, and no Miri `--lib`
run, because Miri's leak checking does not reach a `Box::into_raw` that is simply
forgotten by a raw-allocated owner.

### Size and reach

One `CWelsParametersetIdStrategyObj` is ~1200 bytes (`SParaSetOffset` alone is 1180),
leaked once per **encoder instance destroyed**, not per frame — so a long-running
single-encoder process leaks nothing further, and a process that creates and destroys
encoders leaks ~1.2 KB each time. `WelsEncoderParamAdjust`'s reset arm calls
`WelsUninitEncoderExt` then `WelsInitEncoderExt`, so **every parameter change that
forces a reset leaks one object too** — that is the path that makes this more than a
shutdown-time curiosity.

### Why the fix is not "add the missing call"

T4b.2a made the field
`Option<Box<CWelsParametersetIdStrategyObj>>`, so the object has a `Drop` and an owner
the type system can see. The teardown is now:

```rust
drop((*(*pCtx).pFuncList).pParametersetStrategy.take());
```

at exactly the point `encoder_ext.cpp:1995` deletes it. The `take()` is still explicit
— it has to be, because `SWelsFuncPtrList` is `WelsMallocz`'d and `WelsFree`'d, so the
struct's own drop glue never runs — but the shape of the bug changed: forgetting the
`take()` now leaks a `Box` that `cargo`'s own tooling and Miri can see, rather than a
raw pointer nobody owns.

**The reusable form, and it is R4's in miniature for the third time this phase:** a
`Box::into_raw` whose matching `from_raw` lives in a function no production path calls
is indistinguishable from a leak, and the only instrument that finds it is asking, per
allocation, *which line frees this?* The vtable made the question harder to ask,
because `Destroy` looked like an answer — it was a correct destructor, wired into a
static vtable, that nothing on the live path ever invoked.

### Who fixes it

Fixed at T4b.2a (this session). Recorded here rather than only in the commit because
the *class* is open: `sWelsEncCtx` has other `Box::into_raw` members, and the
free-cascade inventory Phase 6 owns should check each one for the same shape — an owner
that exists in source and not on any live path.

---

## F20 — Two ported paraset strategies, one stale module doc: three places said "only CONSTANT_ID"

**Status: FIXED 2026-08-11 (Phase 4b session B, T4b.2a).** Not a defect in the code —
a defect in what the code said about itself, which had propagated into a session brief
and nearly into a conversion.

`paraset_strategy.rs`'s module doc said only `CWelsParametersetIdConstant` was ported.
`encoder_context.rs:722`'s comment at the construction site said the same. The file
had, and has, **two** ported strategies: `CONSTANT_ID` *and* `INCREASING_ID`, the
latter with its own `ID_INCREASING_VTBL` and three overriding thunks — and
`INCREASING_ID` is `FillDefault`'s value (`param_svc.rs:274`, `codec_api.rs:661`), so
it is the strategy an unconfigured encoder actually runs. Only
`CreateParametersetStrategy`'s own doc comment was right.

The stale claim was load-bearing: Phase 4b session A's scouting read it, recorded "one
ported implementor" in the hand-off, and the session-B brief planned §1 as *deletion of
a vtable with a single implementor* — "a concrete type wearing a costume". Converting
on that plan would have deleted `ID_INCREASING_VTBL`'s three overrides and silently
encoded every default-configured stream with constant parameter-set ids. The sweeps
would have caught it (the id offsets are stream-visible), but the *design* would have
been wrong from the first line.

**The reusable form:** a module doc is not evidence. Session A's own rule for cached
selectors — read the update paths, do not read the summary — applies to prose about
the code exactly as it applies to a cached configuration value. The counts that decide
enum-vs-deletion get taken from `grep` over the vtable instances, every time.
