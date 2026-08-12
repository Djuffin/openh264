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

---

## F21 — One C++ function translated three times, and two copies drifted: sub-16 chroma was expanded three different ways

**Status: FIXED 2026-08-11 (Phase 4b session C, T4b.3b), by unification; PINNED
2026-08-11 (Phase 5 session B, T5.B1).** Found while computing T4b.3b's S20 closure —
the closure asked "who else implements this?" and the answer was three files. The pin
is `test_asset_narrow_16x16_idr_lost` and §The pin below says why it is one row and not
three.

### The divergence

The C++ has exactly one `ExpandReferencingPicture` (`common/src/expand_pic.cpp:388`),
called from five sites — two in the encoder (`ref_list_mgr_svc.cpp:375`, `:779`) and
three in the decoder (`decoder_core.cpp:2854`, `error_concealment.cpp:446`,
`manage_dec_ref.cpp:199`). The port had translated it **three times**, once per
consumer module, plus a fourth forwarding wrapper in `decoder_core.rs` that existed
only to `transmute` between two identical function-pointer types.

The three bodies agreed on the luma plane and on chroma when `kiWidthUV >= 16`. They
disagreed below that — the `else` branch of the C++'s width test, which fires for a
frame narrower than **32 pixels**:

| implementation | chroma when `kiWidthUV < 16` |
|---|---|
| C++ `expand_pic.cpp:388` | `ExpandPictureChroma_c` on Cb and Cr |
| `encoder/ref_list_mgr_svc.rs:199` | same — the faithful copy |
| `decoder/error_concealment.rs:838` | `pExpChrom[0]`, which on this port *is* `ExpandPictureChroma_c` — right answer, wrong reason |
| `decoder/manage_dec_ref.rs:176` | **nothing.** The `if kiWidthUV >= 16` had no `else` at all |

So on the `manage_dec_ref` path — `WelsInitRefList`'s error-concealment prefetch — a
reference picture 16 or 24 pixels wide kept an unexpanded chroma border, and every
motion vector pointing outside the frame read whatever `AllocPicture` had left there.

`error_concealment.rs`'s copy is the more interesting one: it is correct **only
because both chroma slots of `SExpandPicFunc` held the same function** in this port. In
a build with the SIMD variants the C++ installs (`_sse2`, `_neon`, `_mmi`), its
`pExpChrom[0]` is `ExpandPictureChromaUnalign`, and the sub-16 case would call an
alignment-specialised kernel where the C++ calls the scalar one. It was one table entry
away from being a second real divergence.

### Reachability, and why no gate could have caught it

`iWidthInPixel` is `iMbWidth << 4`, so the smallest legal frame is 16 pixels wide and
the divergent range — widths 16 and 24 — is representable in a conformant stream. The
project's corpus does not contain one: the JVT conformance assets are 176x144 and
larger, the diffharness inputs are 152x100 and up, and the malformed-stream corpus is
derived from the conformance streams, so it inherits their SPS dimensions. Both
`decoder_conformance_test` and `malformed_stream_parity` are therefore silent on this
by construction, not by luck.

**A test pinning it would need a new asset** — an encode at 16x16 or 24x16 — which is
golden movement, so it is not this phase's. Recorded for whoever adds a narrow-frame
stream; the arithmetic is now identical to the C++'s in the single copy, so such a test
would be pinning correct behaviour rather than repairing it.

### The pin (Phase 5 session B, T5.B1)

Golden movement authorized 2026-08-11. Three additive rows, C++ decoder goldens as
everywhere in that file, regenerable by `rust/tools/make_narrow_assets.py`:

| row | frame | what it covers |
|---|---|---|
| `test_asset_narrow_16x16` | 16x16 | the minimum legal width; `iWidthUV` 8 |
| `test_asset_narrow_24x18` | 24x18, coded 32x32 + cropped | `iWidthUV` exactly 16 — the branch's other side |
| `test_asset_narrow_16x16_idr_lost` | 16x16, concealed | **the F21 pin** |

**Only the third row covers this finding, and that is the fact worth carrying
forward.** Coverage was proven the way F17's lesson requires — reverting `d1c1a7d4` in
a scratch worktree, resolving the two conflicts to *keep* the later deletions so that
only the expand copies came back — and under the revert the third row goes red while
the first two stay green. They stay green because the pre-fix `decoder_core` path
forwarded to `error_concealment`'s copy, which had the sub-16 arm; **only
`manage_dec_ref`'s copy lacked it, and its only call site is `WelsInitRefList`'s
concealment prefetch.** A narrow-frame stream that decodes cleanly cannot reach it. The
first two rows are worth having — nothing else in the corpus decodes a frame narrower
than 176px, and 5.1 rewrites the plane geometry those two rows measure — but a session
that added them alone would have believed it had pinned F21 and pinned nothing.

Two properties the concealing stream needed, each found by a row that passed when it
should not have:

* **A recycled picture.** The prefetch memsets the picture it takes to 128 over the
  active area only (`iLinesize[1] * iHeightInPixel / 2` bytes from `pData[1]`, which
  covers the picture rows and their left/right padding but not the top and bottom
  border). `AllocPicture` also fills the whole buffer with 128, so a stream that
  conceals from a *fresh* pool reads 128 in the border whether it was expanded or not,
  and is silent on the defect. The asset therefore decodes a full 24-frame sequence
  first, then loses an IDR — the prefetched slot still holds that sequence's samples
  where the memset does not reach.
* **Motion.** The 16x16 source is a window *panned* across the clip, so the MVs are
  non-zero; on a one-macroblock frame any non-zero vertical MV reads outside the plane,
  which is the only way the border reaches the output at all.

The lost IDR itself is a second sequence whose SPS differs from the first (CAVLC
`profile_idc` 66 then CABAC `profile_idc` 100 — the decoder's `memcmp` against the
stored SPS is what makes it a new sequence, which clears the reference lists) with its
IDR NAL removed, so the first slice arriving with empty lists is a P slice.

### Why the fix is unification and not three patches

`common/expand_pic.rs::ExpandReferencingPicture` is now the only copy, with the C++'s
body, and the three duplicates are deleted. Patching `manage_dec_ref`'s missing `else`
would have restored byte-equality and left the shape that produced the bug: three
functions with one name, each free to drift again, and each reachable only through a
table that made them look like different implementations of the same interface. This is
the F13/F20 pattern once more — **a duplicate family is a divergence that has not
happened yet** — and S21's inventory rule is what turns it up.

### The reusable form

S21 says a duplicate-family finding must inventory every function per copy. T4b.3b
adds the corollary that the inventory has to be **behavioural, not structural**: all
three copies had the same name, the same six parameters and the same C++ citation in
their doc comments, and a signature-level diff would have called them identical. The
divergence was one missing `else` in one of three bodies, and reading the three bodies
side by side is the only thing that finds it.
