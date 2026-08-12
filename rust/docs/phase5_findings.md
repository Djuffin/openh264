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

---

## F23 — every `&mut self` method on the public vtable structs is UB: the borrow is 8 bytes wide and the thunk writes past it

**Status: OPEN. Owner: Phase 8 (the API boundary work, plan §2.2.8/T10).** Found at
Phase 5 session D (T5.D2) by the first Miri-covered test that ever called the public
API, written to answer a *different* question (5.2's S25 re-entrancy audit).

### The defect

`ISVCDecoder` and `ISVCEncoder` are one-pointer structs — the C++ base class, vtable
pointer only:

```rust
#[repr(C)] pub struct ISVCDecoder { pub lpVtbl: *const ISVCDecoderVtbl }
```

Each is the **first field** of a much larger implementation object, which is what the
thunks actually operate on:

```rust
#[repr(C)] pub struct CWelsDecoderImpl { pub base: ISVCDecoder, pub pVtbl: …, pub pCtx: …, pub align: …, pub param: …, … }
```

The crate offers 19 convenience methods that forward to the vtable, and every one of
them takes `&mut self`:

```rust
pub unsafe fn Initialize(&mut self, pParam: *const SDecodingParam) -> c_long {
    unsafe { ((*self.lpVtbl).Initialize)(self, pParam) }     // self coerces to *mut ISVCDecoder
}
```

`&mut self` here is a borrow of **`ISVCDecoder`, eight bytes**. The pointer handed to
the thunk inherits exactly that provenance, and `decoder_init_c` then does the
base-to-derived cast every C++ port does and writes through it:

```rust
let dec_impl = this as *mut CWelsDecoderImpl;
(*dec_impl).param = *pParam;          // offset 0x20 — outside the borrow
```

Miri's verdict, on the very first run, before the decoder was initialized:

```text
error: Undefined Behavior: attempting a write access using <960729> at alloc251625[0x20],
       but that tag does not exist in the borrow stack for this location
  --> src/api/codec_api.rs:1425:9
    |
1425 |         (*dec_impl).param = *pParam;
    |         this error occurs as part of an access at alloc251625[0x20..0x40]
help: <960729> was created by a SharedReadWrite retag at offsets [0x0..0x8]
  --> src/api/codec_api.rs:1181:46
```

`[0x0..0x8]` against an access at `[0x20..0x40]` is the whole finding.

**It is not the cast that is wrong** — `WelsCreateDecoder` hands out
`Box::into_raw(dec) as *mut ISVCDecoder`, a pointer with provenance over the entire
implementation object, and casting *that* back up is sound. It is the `&mut self`
signature narrowing it on the way in. Proved by construction: the same call spelled
`((*vtbl).Initialize)(p_decoder, &param)` through the raw pointer runs clean, and
that is how `decode_slice_loop_runs_under_the_aliasing_checker` now spells it.

### Scope

**19 methods, both codecs** (`impl ISVCEncoder` at `codec_api.rs:1044`, 8 of them;
`impl ISVCDecoder` at `:1177`, 4 plus the frame-decode family). Both implementation
structs have the identical `base:` shape. Every one of the crate's **11 integration
test files** drives the codecs through this spelling, as would every Rust consumer.

### Why no gate has ever seen it

The same reason F18 sat red for four phases, and the same reason `data_ptr`'s
provenance defect survived nine gates at T5.C3: **the level that could see it had
never run there.** `gates.sh` runs Miri over `--lib` and, at the once-per-phase `exit`
level, over `tests/*differential*.rs` — which are the kernel, plane and bits
differentials, all of which call internal functions directly. The tests that use the
public API are the conformance and lifecycle suites, and **Miri has never been pointed
at them**. It is not a byte-visible defect either: the address is right, the write
lands where C++ lands it, and all 448/442 tests, 56 golden rows, 2316 malformed rows,
both benches and 341/341 sweeps agree with it.

S22's "an instrument's scope list is part of the instrument", third instance in this
project after `find_stub_bodies.py`'s directory list and `find_dup_types.sh`'s.

### The fix, and why it is not Phase 5's

One line per method: take `*mut Self` instead of `&mut self`, or re-derive with
`addr_of_mut!`. Cheap — and `api/codec_api.rs` is T10/§2.2.8, the drop-in ABI contract
that plan §1.2 says **stays** and is rewired at the API boundary work. Phase 5 converts
the decoder's internals; touching the public signatures mid-phase would change the
crate's Rust-facing API in a phase whose exit criterion is decoder `src/` unsafe-free.
Recorded, owner named, not fixed here — F19's precedent.

**What Phase 8 should not conclude** is that this is only a Rust-caller problem. C and
C++ callers reach the thunks directly and are unaffected; the defect is entirely in the
convenience layer. But the convenience layer is what every test in this repository
uses, which means the whole integration suite is running UB today and no gate says so.

### Owed work, cheaper than the fix

Point the Miri gate at one test that uses the public API, so the boundary stops being
unobserved. `decode_slice_loop_runs_under_the_aliasing_checker` (`decode_slice.rs`) is
now that test for the decode path, and it is in `--lib`, so it runs on every full
battery rather than once per phase — but it deliberately spells the calls the C way, so
it does **not** cover the `&mut self` layer. Covering that needs a test that is expected
to fail until F23 is fixed, which is Phase 8's to add with the fix.

---

## F24 — `ParseSliceHeaderSyntaxs` holds three overlapping `&mut` borrows of one NAL unit, and the outer one invalidates the inner

**Status: FIXED 2026-08-12 (Phase 5 session E, T5.E1), and the fix is proven by the
instrument that found it** — the probe now runs past `ParseSliceHeaderSyntaxs` under
Miri, which is the only evidence that could exist for it (see *Why it was invisible*).
Found at Phase 5 session D (T5.D2) by the same test that found F23, on the run after
F23 was routed around; owner was 5.5, taken early because it blocked 5.2.

**It was never one function.** Fixing the named site let Miri walk further and the same
shape appeared again immediately, so the finding's real scope is the *family* below —
five sites, one file, one rule. That is recorded here rather than as a new finding
because the diagnosis, the byte offsets and the fix are identical at each; the one the
probe reached under its own steam and could not have been predicted is **F25**.

### The defect

`decoder_core.rs:2018-2021`:

```rust
let pNalHeaderExt = &mut (*kpCurNal).sNalHeaderExt;
let pSliceHead    = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt.sSliceHeader;
let eNalType      = pNalHeaderExt.sNalUnitHeader.eNalUnitType;
let pSliceHeadExt = &mut (*kpCurNal).sNalData.sVclNal.sSliceHeaderExt;
```

`pSliceHeadExt` borrows the struct that **contains** what `pSliceHead` borrows, so
creating it invalidates `pSliceHead`. `pSliceHead` is then used 74 times, starting 13
lines later. Miri, with byte-exact offsets:

```text
error: Undefined Behavior: attempting a write access using <34592640> at alloc252357[0x28],
       but that tag does not exist in the borrow stack for this location
  --> src/decoder/decoder_core.rs:2034:5
    |
2034 |     (*pSliceHead).iFirstMbInSlice = uiCode as i32;
help: <34592640> was created by a Unique retag at offsets [0x28..0xf00]      (:2019)
help: <34592640> was later invalidated at offsets [0x28..0x1350]
      by a Unique retag                                                     (:2021)
```

`[0x28..0xf00]` nested inside `[0x28..0x1350]` is the finding. There is a third
invalidation in the same region: `:2023` writes
`(*kpCurNal).sNalData.sVclNal.bSliceHeaderExtFlag` through the raw pointer while both
borrows are live.

**Scale**: the function spans `decoder_core.rs:2000-2576` and uses `pSliceHead` 74
times, `pSliceHeadExt` 50, `pNalHeaderExt` 17. `pNalHeaderExt` is a *sibling* field
(`sNalHeaderExt` vs `sNalData`) and does not overlap the other two; the defect is the
`pSliceHead` / `pSliceHeadExt` pair plus the raw write.

### Why this is S25 and not new

Identical shape to T5.B2's nine functions in `manage_dec_ref.rs`, where `pRefPic` is a
*subfield* of the context that a second borrow also covered, and to T4b.2a's
`WelsWriteParameterSets`. The established fix is the same: **no borrow outlives one
expression** — name `(*kpCurNal).sNalData.sVclNal.sSliceHeaderExt…` per use, the shape
written at `SetUnRef`. 141 sites in one function, mechanical, and it wants its own
commit rather than a corner of a conversion.

### Why it was invisible

Same answer as F23 and the same answer as `data_ptr` at T5.C3: no Miri-covered test had
ever entered this code. It is not byte-visible — the addresses are all correct, the
writes land where C++ lands them, and the whole battery agrees. It took a 711-byte
stream decoded under an interpreter, and it is the *second* defect that stream found.

### What this means for 5.2, which is why session D stopped here

5.2's closure (session D's log §1) converts `SDqLayer`'s per-MB arrays to an owned
`MbGrid`, and S25 makes the re-entrancy audit part of that conversion. **The audit now
has a gate for the first time, and the gate says the decode path has pre-existing
aliasing UB in front of 5.2's own work.** Converting raw pointers to borrows on top of
that means every new `&mut` inherits an already-invalid stack, and a Miri failure after
the conversion would be unattributable — exactly the bisect problem T5.C2's
three-face split was designed to avoid.

So the order the closure licenses is **F24 first, then 5.2** — and F24 is 5.5's file but
5.2's blocker, which is a dependency the plan's step order does not currently express.
Session D's hand-off names it.

### Queue depth was unknown, and the answer was "not one" (T5.E1)

The probe fails at the *first* defect it reaches each run. F23 was first, F24 second,
and session D warned that nobody should assume F24 was the last. It was not. Fixing the
named site moved the probe forward by one function, where the identical shape was
waiting; greping that shape rather than paying another ~8-minute round trip per site
found the rest of the family in one pass.

**The family, all `decoder_core.rs`, all fixed at T5.E1:**

| site | shape | how found |
|---|---|---|
| `ParseSliceHeaderSyntaxs:2018` | `sSliceHeader` inside `sSliceHeaderExt`, 141 uses | Miri (T5.D2) |
| same fn, `:2091-2098` | `pSps` inside a re-borrowed `pSubsetSps` entry | reading — **no gate reaches it**, see below |
| `DecodeCurrentAccessUnit:3684` | the same nested pair, byte-identical diagnosis | Miri, run 1 after the fix |
| `InitDqLayerInfo:3471` | `pSh` nested in `pShExt`, plus four borrows that *escape* into the layer | grep of the shape |
| `WelsDqLayerDecodeStart:3526` | escapes into `(*pCtx).pSliceHeader` for the whole slice decode | grep of the shape |

The escaping ones are worth separating out: `InitDqLayerInfo` stores
`pRefPicListReordering`, `pRefPicMarking`, `pPredWeightTable` and `pRefPicBaseMarking`
into the layer, and `WelsDqLayerDecodeStart` stores `pSliceHeader` into the context.
As `&mut`-derived pointers those died at the *next* use of their parent and were read
for the rest of the decode. Same rule, larger blast radius.

**The paramset pair is the one no instrument can see.** `pSps` was `&mut (*pSubsetSps).sSps`
with three later `&mut` re-borrows of the same subset-SPS entry popping it. It is UB on
every `kbExtensionFlag` path — and `bExtensionFlag` is `eType == NAL_UNIT_CODED_SLICE_EXT`
(`nalu.rs:684`), so it needs an SVC stream. The corpus has none and the probe decodes
AVC. Found by reading while fixing the pair above; fixed in the same commit rather than
left for a test that does not exist. **S22's shape again**: an instrument's reach is part
of the instrument, and this sits outside it.

The fix everywhere is the same and it is not a `&mut` at all: `addr_of_mut!` derives from
the allocation root without creating a reference, so every pointer carries the parent's
provenance and none can invalidate another. That is T5.B2's rule (*no borrow outlives one
expression*) reached by removing the borrows rather than by shortening them.

---

## F25 — `WelsDecodeSlice` holds three overlapping `&mut`s across the re-entrant `pDecMbFunc`, and the callee's own `&mut *pCtx` kills them

**Status: FIXED 2026-08-12 (Phase 5 session E, T5.E1).** Owner was 5.2/5.6 jointly; taken
here because the probe reached it. **This is the defect
`decode_slice_loop_runs_under_the_aliasing_checker` was written for** — session D
enumerated it by reading (log §3) and explicitly labelled it *a prediction, not a
finding, until Miri gets there*. Miri got there on the second round trip of this session
and confirmed it byte-for-byte.

### The defect

`decode_slice.rs:5071-5078`, before the fix:

```rust
let ctx = &mut *pCtx;                                    // Unique over [0x0..0x8ae00]
let dq  = &mut *pCurDqLayer;
let pSlice = &mut dq.sLayerInfo.sSliceInLayer;           // reborrow of dq
let pSliceHeader = &mut pSlice.sSliceHeaderExt.sSliceHeader;
…
ctx.bMbRefConcealed = false;
let iRet = pDecMbFunc(pCtx, pNalCur, &mut uiEosFlag);    // re-enters through pCtx
```

Miri:

```text
error: Undefined Behavior: attempting a write access using <35266369> at alloc251880[0x7e92c],
       but that tag does not exist in the borrow stack for this location
  --> src/decoder/decode_slice.rs:5150:9   ctx.bMbRefConcealed = false;
help: <35266369> was created by a Unique retag at offsets [0x0..0x8ae00]   (:5071)
help: <35266369> was later invalidated at offsets [0x0..0x8ae00] by a Unique retag
  --> src/decoder/decode_slice.rs:698:15   let ctx = &mut *pCtx;
```

The invalidator is **inside the callee** — `pDecMbFunc` re-enters and takes its own
`&mut *pCtx` at `:698`. The loop then writes through the dead outer tag on the next
iteration. Whole-context retags (`[0x0..0x8ae00]`) mean *any* re-entrant callee that
borrows the context kills *every* outer borrow of it.

### Why it matters beyond this function

**~30 live `&mut *pCtx` sites remain in `src/decoder/`.** Each is fine alone and UB the
moment it is held across a call that re-enters through `pCtx` — which the decoder does
constantly, through `pDecMbFunc`, `pFuncList` and the deblocking callbacks. This is not
a list of independent defects; it is one systemic pattern, and 5.2–5.6 convert exactly
this code. The rule those conversions inherit: **the god-context is never held as a
borrow across a call.** `(*pCtx)` per use, or an `addr_of_mut!` pointer if a name is
wanted.

### The fix

`(*pCtx)` / `(*pCurDqLayer)` named per use — 43 sites — and the two nested pointers
derived with `addr_of_mut!` so they carry the layer's provenance and no retag exists to
invalidate. Zero bytes moved: 56 golden rows and 449 tests green either way.

---

## F26 — `CMemoryAlign::WelsMalloc` launders every allocation's provenance through `usize`, so the whole decoder heap is wildcard-tagged

**Status: OPEN. Owner: 5.2 for the accessor that surfaces it; the allocator itself is
cross-cutting and wants a decision before 5.3.** Found at Phase 5 session E (T5.E1) by
the probe, on the third round trip, once F24's family and F25 stopped failing first.
**This is the finding that stopped session E's queue** — the brief's bound is three
defects beyond F24, and this is the third.

### The defect

`memory_align.rs:49-50`:

```rust
let addr = pAlignedBuffer as usize;
pAlignedBuffer = (addr - (addr & (kiAlignedBytes as usize))) as *mut u8;
```

An integer→pointer round trip, so the returned pointer has **exposed** provenance. Every
`WelsMalloc`/`WelsMallocz` object in the port inherits it — the NAL unit list, the access
unit, `SDqLayer`, the per-MB arrays. Miri calls such pointers `<wildcard>` and will not
grant an access through one if doing so would disable a **strongly protected** tag (a
`&mut` live as a function argument):

```text
error: Undefined Behavior: not granting access to tag <wildcard> because that would
       remove [Unique for <36614846>] which is strongly protected
  --> src/decoder/cabac_decoder.rs:855:25
855 |     raw.rbsp_window(&*(*(*pCtx).pCurDqLayer).pBitStringAux)
```

reached from `ParseIntraPredModeLumaCabac`, whose own `iBinVal: &mut i32` is one such
protected argument.

### The one open question, and the experiment that settles it

The accessor is reached through `pBitStringAux`, which T5.E1 changed from a `&mut`
coercion to `addr_of_mut!`. A `&mut` *retags* — it would have minted a tracked tag from
the wildcard parent — where `addr_of_mut!` inherits the wildcard. So it is not yet proven
whether F26 was reachable before T5.E1 or whether that one line is what exposes it.
**The experiment is one probe run**: restore `(*dq_cur).pBitStringAux = &mut …`
(`decoder_core.rs`, the `DecodeCurrentAccessUnit` store) alone, keep everything else, and
see which error Miri reports. Either answer is useful — if the `&mut` spelling passes,
the allocator fix is optional for 5.2 and F26 is a latent finding; if it fails the same
way, F26 is live today and the escaping-borrow defect T5.E1 fixed was merely masking it.

Do not settle this by reasoning. Both spellings have a defensible story and the
instrument is eight minutes away.

### Why no other gate can see it

The addresses are all correct and the arithmetic is right — this is a *provenance*
property, invisible to every byte-level gate, and the whole battery agrees with it:
449/443 tests, 56 golden rows, 2316 malformed rows, both benches, 341/341 sweeps. Fourth
instance in this project of "the level that could see it had never run there" (F18,
`data_ptr` at T5.C3, F23/F24 at T5.D2).

### What the fix probably is

Compute the alignment without leaving pointer-land: `pBuf.add(offset)` then
`byte_sub(addr & mask)` — or `align_offset` — so provenance is never exposed. It is a few
lines in one function, and it is **not** a mechanical change: restoring tracked
provenance crate-wide will let Miri see borrow structure it has been silently permitting,
which is S22's backlog shape and should be expected to surface more. That is why it wants
its own session and a decision, not a corner of a conversion commit.
