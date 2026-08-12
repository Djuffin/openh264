# Phase 5 findings

Things found while executing Phase 5 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— the decoder structural rewrite — that are *not* Phase 5's job to fix at the moment
they were found, or that Phase 5 did fix and wants on the record. Numbering continues
from [`phase4b_findings.md`](phase4b_findings.md) (F19–F21).

---

## F22 — `parse_mb_syn_cabac.rs` re-translated six `mv_pred` functions and dropped every `pDec` null guard

**Status: OPEN. Owner: 5.3 (Neighbor & MV), which converts `mv_pred.rs` and is where
the two copies become one.** Found at Phase 5 session A while widening
`find_stub_bodies.py --dups` to cover `src/decoder`, which it had never scanned.

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

### Reachability

Unknown, and that is the honest state. Whether `pCurDqLayer->pDec` can be null while a
CABAC P-macroblock is being parsed is a question about `InitDqLayers`/`DecodeSlice`
ordering that 5.2 answers as a side effect of building `DqLayerState`. Two readings, and
the finding is worth recording under either:

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
