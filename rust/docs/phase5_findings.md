# Phase 5 findings

Things found while executing Phase 5 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— the decoder structural rewrite — that are *not* Phase 5's job to fix at the moment
they were found, or that Phase 5 did fix and wants on the record. Numbering continues
from [`phase4b_findings.md`](phase4b_findings.md) (F19–F21).

---

## F22 — `parse_mb_syn_cabac.rs` re-translated six `mv_pred` functions, and three of them lost a `pDec` null guard

**Status: OPEN, reachability ANSWERED at T5.D1 (session D) — the guard is dead code in
both trees, so this is a divergence and not a latent crash. Owner: 5.3 (Neighbor & MV),
which converts `mv_pred.rs` and is where the copies become one.** Found at Phase 5
session A while widening `find_stub_bodies.py --dups` to cover `src/decoder`, which it
had never scanned. **Two corrections landed at session D** — the reachability answer, and
an S24 re-grep of the C++ side that narrows the divergence from six functions to three
and reverses its direction for a fourth; both are below, and the title above is the
corrected claim.

### The divergence

C++ declares each of these **once**, in `mv_pred.cpp`, and `parse_mb_syn_cabac.cpp`
*calls* them (`:562`, `:592`, `:797`, `:841`):

| function | C++ | Rust copies |
|---|---|---|
| `UpdateP16x16MotionInfo` | `mv_pred.cpp:797` | `mv_pred.rs`, `parse_mb_syn_cabac.rs:1749` |
| `UpdateP16x8MotionInfo` | `mv_pred.cpp:871` | `mv_pred.rs`, `parse_mb_syn_cabac.rs:1775` |
| `UpdateP8x16MotionInfo` | `mv_pred.cpp` | `mv_pred.rs`, `parse_mb_syn_cabac.rs` |
| `Update8x8RefIdx` | `mv_pred.cpp` | `mv_pred.rs`, `parse_mb_syn_cabac.rs` |
| `PredMv` | `mv_pred.cpp` | `mv_pred.rs`, `parse_mb_syn_cabac.rs` |
| `PredInter16x8Mv` / `PredInter8x16Mv` | `mv_pred.cpp` | `mv_pred.rs`, `parse_mb_syn_cabac.rs` |

The C++ body of `UpdateP16x8MotionInfo` (`mv_pred.cpp:885`) branches on the current
layer's decoded-picture pointer:

```c
    //mb
    if (pCurDqLayer->pDec != NULL) {
      ST16 (&pCurDqLayer->pDec->pRefIndex[listIdx][iMbXy][kuiScan4Idx], kiRef2);
      ...
    } else {
      ST16 (&pCurDqLayer->pRefIndex[listIdx][iMbXy][kuiScan4Idx], kiRef2);
      ...
    }
```

`mv_pred.rs`'s copy has the branch — **28** `pDec.is_null()` guards across the module,
matching the C++ file. `parse_mb_syn_cabac.rs`'s copy has **zero**:

```rust
        let pDecRef = &mut *(*(*pCurDqLayer).pDec).pRefIndex[listIdx].add(iMbXy);
        let pDecMv  = &mut *(*(*pCurDqLayer).pDec).pMv[listIdx].add(iMbXy);
```

An unconditional dereference where the C++ tests for null, on the path CABAC macroblock
parsing actually takes (`parse_mb_syn_cabac.rs:1924`, `:1971`, `:2283`, `:2334` call the
**local** copies — a local item beats an import, and the module imports only
`SubMbType`, `FillSpatialDirect8x8Mv` and `FillTemporalDirect8x8Mv` from `mv_pred`).

Guard counts per decoder module, for scale: `mv_pred.rs` 28, `decode_slice.rs` 13,
`parse_mb_syn_cavlc.rs` 4, `parse_mb_syn_cabac.rs` **0**.

### S24 correction (Phase 5 session D) — the per-module counts are right, the story they tell is wrong

The four Rust numbers above re-grep exactly. **The C++ side of the comparison was never
greped**, and it does not say what the finding assumed. Measured with both C++ spellings
of the test — `pDec != NULL` *and* the ternary `pCurDqLayer->pDec ? A : B`, which is the
form `mv_pred.cpp` mostly uses and which a search for `NULL` misses:

| module | C++ null tests | Rust null tests |
|---|---|---|
| `mv_pred` | **20** (4 if-form + 16 ternary) | 28 |
| `parse_mb_syn_cabac` | **0** | 0 |
| `parse_mb_syn_cavlc` | **0** | 4 |
| `decode_slice` | **2** | 13 |

So `parse_mb_syn_cabac.rs`'s zero is a **faithful** translation: the C++ CABAC file
dereferences `pCurDqLayer->pDec` unguarded 28 times of its own (`:114`, `:115`, `:133`,
`:134`, `:145-147`, `:482`, `:540`, `:685-710`, `:742`, `:1044-1069`, `:1091`, `:1284`,
`:1508`, `:1509`, `:1523`). "The port dropped every guard in this file" is not what
happened.

**The real divergence is three functions, not six.** Of the seven the finding lists,
three touch `pDec` *and* are guarded in C++:

| function | C++ home | C++ guard | `mv_pred.rs` | `parse_mb_syn_cabac.rs` copy |
|---|---|---|---|---|
| `UpdateP16x16MotionInfo` | `mv_pred.cpp:797` | `:807` if-form | guarded, `:1209` | **unguarded**, `:1749` |
| `UpdateP16x8MotionInfo` | `mv_pred.cpp:871` | `:885` if-form | guarded, `:1300` | **unguarded**, `:1775` |
| `UpdateP8x16MotionInfo` | `mv_pred.cpp:910` | `:925` if-form | guarded, `:1361` | **unguarded**, `:1809` |
| `Update8x8RefIdx` | `mv_pred.cpp:1175` | **none** | guarded, `:1422` | unguarded, `:1843` — *faithful* |
| `PredMv`, `PredInter16x8Mv`, `PredInter8x16Mv` | `mv_pred.cpp` | n/a — never touch `pDec` | — | — |

`Update8x8RefIdx` inverts the finding's direction: the C++ dereferences unconditionally,
the CABAC copy matches it, and **`mv_pred.rs` is the copy that added a guard**. The port
added `pDec` tests in several more places the C++ has none — `parse_mb_syn_cavlc.rs`'s
four, eleven of `decode_slice.rs`'s thirteen, and `UpdateP16x16RefIdx`
(`mv_pred.rs:1249`), where the added `if !pDec.is_null()` has **no `else`**: where the
C++ would fault, the port silently skips the write and decodes on with a stale cache.
Dead either way, given the reachability answer above, but it is the shape that F21 was.

**Why this matters for 5.3 rather than being pedantry**: unification's question is *which
copy is the reference*, and the answer per function is not the same. The three guarded
ones unify onto `mv_pred.rs`'s shape. `Update8x8RefIdx` unifies onto the CABAC shape and
`mv_pred.rs`'s extra guard comes off. Deciding it per-module from the 28-vs-0 headline
would have added a guard to `Update8x8RefIdx` that the reference does not have — S21's
"inventory every function of the family per copy", earned again, and the third time a
count in a Phase 5 document has been a summary of a fact rather than the fact.

### Reachability — ANSWERED at Phase 5 session D (T5.D1): it cannot be null there

**`pCurDqLayer->pDec` cannot be null on the CABAC parse path, and the guard is dead
code in the C++ too.** The first of the two readings below is the correct one. Proved
by domination, not by testing — no gate can distinguish the readings, as the original
text says, so the evidence is a five-link chain over the writers:

1. **`SDqLayer::pDec` has exactly one writer in the whole crate**:
   `decoder_core.rs:3444`, inside `InitDqLayerInfo`, writing its fourth argument.
   (`grep -E '\.pDec[[:space:]]*=[^=]'` over `src/decoder/*.rs` returns ten hits;
   nine are `(*pCtx).pDec` or a test's `ctx.pDec`, one is this.)
2. **`InitDqLayerInfo` has exactly one call site**: `decoder_core.rs:3682`, passing
   `(*pCtx).pDec`.
3. **That call site is dominated by the prefetch-or-return**, in the same iteration of
   the same loop, at `decoder_core.rs:3589-3597`:
   `if (*pCtx).pDec.is_null() { (*pCtx).pDec = PrefetchPic(...); if (*pCtx).pDec.is_null() { return ERR_INFO_REF_COUNT_OVERFLOW; } }`.
   So `(*pCtx).pDec` is non-null at `:3682` or the function has already returned.
4. **Nothing between `:3682` and the parse can undo it.** `(*pCtx).pCurDqLayer` has
   one production writer, `decoder_core.rs:3578`, which runs *before* the loop (the
   only other two are `error_concealment.rs:970` / `:1044`, both inside `mod tests`,
   pointing at a stack `SDqLayer`). The six `(*pCtx).pDec = null` sites are all after
   the parse or immediately followed by `return`; and in any case they write the
   *context's* field, while the CABAC code dereferences the *layer's* copy.
5. **The parse is single-threaded and there is only one of it.**
   `GetThreadCount` (`decoder_core.rs:615`) returns `0` unconditionally — the decoder
   half of T9 was never ported — so the `iThreadCount > 1` arm at `:3704` is dead and
   `WelsDecodeSlice` is the only parse entry. Its MB loop (`decode_slice.rs:5143`)
   calls `pDecMbFunc` with no further `pDec` write.

**The same proof holds for upstream C++**, which is the part that decides 5.3's
semantics: `pDqLayer->pDec` has one writer there too (`decoder_core.cpp:2386`), one
call site (`:2674`), and the same prefetch-or-return above it (`:2542-2575`) — which
is **not** conditional on `bParseOnly`, so even the parse-only path has a picture
struct attached. The 20 null tests in `mv_pred.cpp` are defensive branches that no
stream reaches.

Consequences, all of which 5.2/5.3 act on:

* **The port's unguarded CABAC copies are not a latent crash.** The second reading is
  refuted. What remains is the first: a divergence that is a trap for the next editor.
* **The `else` arms of `mv_pred.rs`'s guards are dead code** — and they are the only
  writers of `SDqLayer`'s own `pMv` / `pRefIndex` per-MB arrays through this path,
  which is what makes 5.2's double-path kill subtraction rather than unification for
  those two fields (see the session-D closure).
* **Unification keeps the guard**, because the C++ has it and byte-exactness is
  measured against the C++, not against reachability. A guard over a branch that never
  runs costs nothing and is what the reference does.

### The original two readings, kept for the record

* **It cannot be null there** — then the C++ guard is defensive and the port's copy is
  merely divergent, and the divergence is a trap for whoever next edits either copy.
* **It can be null there** — then this is a null dereference on the mainline CABAC path,
  reachable from a stream that reaches macroblock parsing before a decoded picture is
  attached, which is error-concealment territory.

No gate here can distinguish them: every corpus stream decodes with `pDec` attached, so
both readings produce identical bytes on everything the battery runs. That is F21's
situation again — silent by construction, not by luck.

### Why it is 5.3's and not this session's

The fix is unification, not adding a branch to the second copy (F21's lesson: patching
the divergent copy leaves the family). Unifying means `parse_mb_syn_cabac` calling
`mv_pred`'s definitions, and those take `*mut SDqLayer` with the two modules'
cache-array shapes differing (`*mut [[i16; 2]; 30]` vs `*mut [[[i16; 2]; 30]; LIST_A]`)
— which is exactly the signature the 5.2/5.3 `MbGrid` conversion changes. Doing it now
would be a conversion decided before its S20 closure is computed.

### The instrument

`find_stub_bodies.py`'s `RUST_DIRS` listed `encoder`, `processing` and `common`, and its
`CPP_DIRS` listed the matching three; **neither included the decoder**. Phase 5 is the
decoder phase and its brief names this instrument as one of three to run. Widened at
session A: duplicate-body groups **51 → 198**, of which **156** touch `src/decoder`.
S22's shape exactly — the backlog surfaces the moment the instrument first covers what
it claims to.
