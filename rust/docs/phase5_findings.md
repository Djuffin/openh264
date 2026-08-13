# Phase 5 findings

Things found while executing Phase 5 of [`safety_refactor_plan.md`](safety_refactor_plan.md)
— the decoder structural rewrite — that are *not* Phase 5's job to fix at the moment
they were found, or that Phase 5 did fix and wants on the record. Numbering continues
from [`phase4b_findings.md`](phase4b_findings.md) (F19–F21).

---

## F22 — `parse_mb_syn_cabac.rs` re-translated six `mv_pred` functions, and three of them lost a `pDec` null guard

**Status: CLOSED at Phase 5 session M (T5.M4, 2026-08-13) — the copies are one.** Eight
names had two definitions each; every duplicate is deleted, each function now lives where
the **C++** declares it, and the resolution was decided **per function** rather than per
module. Zero bytes moved: goldens, 341/341 sweeps in both profiles, and both benches
bit-identical, which was the commit's own condition. See "The unification, as it landed"
at the end of this entry.

*(History below is as it stood before the close; reachability was ANSWERED at T5.D1 —
the guard is dead code in both trees, so this was a divergence and not a latent crash.)*
Found at Phase 5
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

**And the instrument under-reported this finding by five eighths**, which is worth more
than the finding: unifying all eight copies took the count **198 → 195**. It could only
ever see the three whose bodies had stayed *identical*; the five that had **diverged** —
the ones that mattered, the ones this entry is about — were invisible to it by
construction. A duplicate-body count is a floor on the duplication, never a measure of
it, and the divergent copies are always the ones it cannot see.

### The unification, as it landed (T5.M4)

**Home = the C++'s home**, which is the whole answer to "which copy is the reference":
`mv_pred.cpp` declares seven of the eight and `parse_mb_syn_cabac.cpp` merely *calls*
them; `parse_mb_syn_cabac.cpp:141` declares the eighth and `mv_pred.cpp` is *its* caller.
So `mv_pred.rs` keeps seven, `parse_mb_syn_cabac.rs` keeps `UpdateP8x8RefIdxCabac`, each
module imports the other's, and a local item no longer shadows an import.

| function | copies | resolution | why |
|---|---|---|---|
| `PredMv` | both | `mv_pred.rs`'s | bodies agreed; never touches `pDec` |
| `PredInter16x8Mv` | both | `mv_pred.rs`'s | ditto |
| `PredInter8x16Mv` | both | `mv_pred.rs`'s | ditto |
| `UpdateP16x16MotionInfo` | both | `mv_pred.rs`'s, **guard kept** | `mv_pred.cpp:807` has the `pDec` branch |
| `UpdateP16x8MotionInfo` | both | `mv_pred.rs`'s, **guard kept** | `mv_pred.cpp:885` ditto |
| `UpdateP8x16MotionInfo` | both | `mv_pred.rs`'s, **guard kept** | `mv_pred.cpp:925` ditto |
| `Update8x8RefIdx` | both | **the CABAC copy's shape** | `mv_pred.cpp:1175` is unguarded; `mv_pred.rs` had *added* the guard |
| `UpdateP8x8RefIdxCabac` | both | **`parse_mb_syn_cabac.rs`'s**, two added guards deleted | its C++ home is the CABAC file, and that copy is the faithful one |

**Three port-added guards died with the duplicates**, all of the class session L killed
seventeen of: `Update8x8RefIdx`'s `pDec` test, and `UpdateP8x8RefIdxCabac`'s `pDec` *and*
ref-index-list tests. Two more died in `UpdateP16x8MotionInfo`/`UpdateP8x16MotionInfo`,
where the port had added `is_null` tests on the **scratch caches** that
`mv_pred.cpp:871`/`:910` do not have — the parameters are declared
`int16_t iMotionVector[LIST_A][30][MV_A]` there and indexed unconditionally. (T5.M2 had
faithfully preserved those as `Option<&mut …>` hours earlier, which is the correct order:
convert faithfully, *then* unify, so the divergence is visible in one diff instead of
being quietly dropped in another.)

**What the C++ genuinely does guard, and the port goes on guarding**:
`FillSpatialDirect8x8Mv`/`FillTemporalDirect8x8Mv` test `pMotionVector != NULL` and
`pMvdCache != NULL` at every use, because the CAVLC path has no mvd cache to pass. Those
stayed `Option<&mut …>`.

**One dead parameter deleted.** `UpdateP8x8RefIdxCabac` takes `int8_t pRefIndex[LIST_A][30]`
in the C++ **and never reads it**; both port copies marked it `_`. Keeping it meant the two
Rust callers had to invent a value they did not have — `mv_pred.cpp` passes a real cache,
`mv_pred.rs` passed a null — so the dead parameter *was* the divergence. It is gone from
the unified definition. Nothing observable changes, in either tree, by construction.

**Not in this finding's scope, and still standing**: the port's added `pDec` tests in
single-copy functions — `parse_mb_syn_cavlc.rs`'s four, eleven of `decode_slice.rs`'s
thirteen, and `UpdateP16x16RefIdx` (`mv_pred.rs`), whose added `if !pDec.is_null()` has no
`else` and so silently skips a write where the C++ would fault. Those are not duplicates;
they are single copies that diverge from their references, which is a different job.
`decode_slice.rs`'s eleven are 5.6's by P1.

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

Every live `&mut *pCtx` site in `src/decoder/` is fine alone and UB the
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

### The inventory this left behind — CLOSED at T5.G1, and its count was wrong (2026-08-12)

This entry left the pattern's *other* sites for the files that own them, and named the
remainder as **12 bindings decoder-side, 7 in `decode_slice.rs` and 5 in
`manage_dec_ref.rs`**. That number was carried verbatim into the probe's `#[cfg_attr]`
label, `phase5.md` §2, S29 and session F's hand-off.

**The real count is 11: 6 and 5.** `grep 'let ctx = &mut \*pCtx;' decode_slice.rs`
returns seven lines and the seventh is `:5405` — a `///` line inside the probe's own doc
comment, illustrating the defect. Prose inflating a count of code is exactly S16's
standing warning about `raw_ptr`, arriving in a place S24 was already watching, and the
instrument that separates them is one comment-stripping pass before the count.

Nothing about the work changed — the shape was "convert every `&mut *pCtx` binding", not
"convert twelve things" — which is the reason this cost nothing and the F29 miss cost a
round trip. **A wrong count is harmless exactly when it is not load-bearing**, and there
is no way to know which kind you have without checking.

The prior claim here that "**~30** live `&mut *pCtx` sites remain" was never measured
either: at T5.D2, before the fix above, the decoder held **14** occurrences across two
files, 12 of them bindings. The number that turned out to matter was F28's, and it was
larger and about a different object.

**Disposition (T5.G1):** all 11 converted, with the 13 nested borrows that hung off them
(2 in `decode_slice.rs`, 11 in `manage_dec_ref.rs`, `WelsMarkAsRef`'s
`&mut (*pCtx).sTmpRefPic as *mut SRefPic` among them — S29's forbidden derivation with
the cast already in place). `src/decoder/` now contains **zero** `&mut *pCtx`, and no
function in the port takes `&mut SWelsDecoderContext`.

---

## F26 — `CMemoryAlign::WelsMalloc` launders every allocation's provenance through `usize`, so the whole decoder heap is wildcard-tagged

**Status: FIXED 2026-08-12 (Phase 5 session F, T5.F2).** Owner was 5.2 for the accessor
that surfaced it; the allocator itself was cross-cutting and got its own face. Found at
Phase 5 session E (T5.E1) by
the probe, on the third round trip, once F24's family and F25 stopped failing first.
**This is the finding that stopped session E's queue** — the brief's bound is three
defects beyond F24, and this is the third.

> **Correction, session F (2026-08-12), from the experiment §"The one open question"
> below asked for.** The laundering described here is real and the fix stands. But the
> *evidence* it was found on is not evidence of it: the refusal at `cabac_decoder.rs:855`
> reproduces with a fully tracked tag, so that site is a genuine aliasing conflict and
> not a consequence of wildcard provenance. It is written up separately as **F27**. Read
> this entry as "the allocator blinds Miri crate-wide" — which is true, and which no
> single site demonstrates — and F27 as "what the probe was actually stopping on".

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

### The one open question — ANSWERED, T5.F1 (2026-08-12), and the answer was a fourth option

The accessor is reached through `pBitStringAux`, which T5.E1 changed from a `&mut`
coercion to `addr_of_mut!`. A `&mut` *retags* — it would mint a tracked tag from the
wildcard parent — where `addr_of_mut!` inherits the wildcard. So it was not proven
whether F26 was reachable before T5.E1 or whether that one line exposed it. The
experiment: restore `(*dq_cur).pBitStringAux = &mut …` (`decoder_core.rs`, the
`DecodeCurrentAccessUnit` store) alone, keep everything else, one probe run, revert.

**It was run. The `&mut` spelling fails at the same site, in the same words, with a
tracked tag:**

```text
error: Undefined Behavior: not granting access to tag <35268364> because that would
       remove [Unique for <36617218>] which is strongly protected
    --> src/decoder/cabac_decoder.rs:855:25
help: <35268364> was created by a SharedReadWrite retag at offsets [0x1350..0x1380]
    --> src/decoder/decoder_core.rs:3694:39
3694 |  (*dq_cur).pBitStringAux = &mut (*pNalCur).sNalData.sVclNal.sSliceBitsRead;
help: <36617218> is this argument
    --> src/decoder/decode_slice.rs:3785:5
3785 |     pBsAux: &mut BsCursor,
```

The finding anticipated two outcomes — passes (F26 latent) or fails identically (F26
live). It did neither, and the difference is the whole result. **The tag is not
`<wildcard>` any more and Miri refuses the access anyway.** A wildcard refusal is
conservative: *this access might disable a protected tag, so it is not granted.* A
refusal against a concrete tag over `[0x1350..0x1380]` is a finding: *this access does
overlap a live, strongly protected `&mut`.* The experiment therefore converted "Miri
cannot clear this path" into "Miri has convicted this path", and the conviction has
nothing to do with the allocator.

**Both halves of the answer:**

1. **F26 was live before T5.E1.** That one line exposed nothing; the allocator has
   laundered every allocation's provenance for the port's whole life. `addr_of_mut!`
   stays.
2. **F26 is not what the probe was stopping on.** The site is F27's, and fixing the
   allocator will not make `cabac_decoder.rs:855` pass — it will change the diagnostic
   from a wildcard refusal to exactly the tracked-tag error above. Anyone who expected
   the allocator commit to turn the probe green should read F27 first.

The finding declined to settle this by reasoning, and it was right to: the reasoning on
offer covered two outcomes and the instrument returned a third.

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

**And the C++ does not do this.** `memory_align.cpp:92` is
`pAlignedBuffer -= ((uintptr_t) pAlignedBuffer & kiAlignedBytes);` — pointer arithmetic
with the integer used only as the mask. The round trip is the port's, introduced in
translation. So the repair is S6 arithmetic parity, not a repair of the algorithm.

---

## F27 — the CABAC path reaches the NAL's whole `BsReader` while holding a `&mut` to the `BsCursor` inside it

**Status: FIXED 2026-08-12 (Phase 5 session F, T5.F3).** Owner was 5.2 — `pBitStringAux`
retires there and `cabac_decoder.rs:855`'s `SHIM(phase5)` accessor dies with it; the fix
here removes the *conflict* without waiting for the field. Split out of F26 at
Phase 5 session F (T5.F1) by the experiment F26's entry specified: it is the defect that
site was actually reporting, and it is provenance-independent.

### The defect

`decode_slice.rs:4111`, `WelsDecodeMbCabacIntraModeHelper`:

```rust
let (buf, pBsAux) = (*(*dq).pBitStringAux).split(&(*pCtx).sRawData);
```

`BsReader::split` (`bit_stream.rs:280`) returns `(&'a [u8], &'a mut BsCursor)`, and the
cursor half is `&mut self.cursor` — a borrow of a **field inside** the NAL unit's
`BsReader`. It is then passed down as an argument (`ParseIntra4x4Mode`'s `pBsAux`,
`decode_slice.rs:3785`), which makes it *strongly protected* for that call's whole
duration.

Inside that call, the CABAC arm reaches the same object by the other route:

```rust
// cabac_decoder.rs:855, cabac_rbsp_window
raw.rbsp_window(&*(*(*pCtx).pCurDqLayer).pBitStringAux)
```

`&*pBitStringAux` is a shared reference to the **whole** `BsReader` — `start` *and*
`cursor`, `[0x1350..0x1380]`, 0x30 bytes — so it covers the bytes the protected `&mut
BsCursor` owns. Two live paths to one object, one of them a protected `&mut`: S25's
shape, and the reason it is invisible without Miri is that neither path writes through
the other.

The call chain, from the probe's backtrace:

```text
WelsDecodeMbCabacIntraModeHelper:4111   let (buf, pBsAux) = (*(*dq).pBitStringAux).split(…)
  ParseIntra4x4Mode:3785                pBsAux: &mut BsCursor        ← strongly protected
    ParseIntraPredModeLumaCabac:1362
      cabac_rbsp_window → cabac_decoder.rs:855   &*(…).pBitStringAux  ← refused
```

### Inventory, greped rather than reasoned (S24)

Ten `.split(&…)` sites reach a `BsReader` in `src/decoder/`. Nine are in
`decode_slice.rs`; the tenth (`parse_mb_syn_cabac.rs:3240`, `ParseIPCMInfoCabac`) ends
its cursor borrow before the next use and is not this shape. Of the nine, **seven are
`WelsDecodeMbCavlcResidual`/`DecodeMbCavlcPcm`** — CAVLC reads through the `(buf,
cursor)` pair itself and never re-enters through `pCtx->pCurDqLayer->pBitStringAux`, so
they are clean. **Two are live F27 sites, both `decode_slice.rs`, both CABAC:**

| site | function |
|---|---|
| `:4111` | `WelsDecodeMbCabacIntraModeHelper` — the one Miri reported |
| `:4163` | `WelsDecodeMbCabacResidualHelper` — same shape, same file |

The other end is `cabac_rbsp_window`, **19 use sites** across `parse_mb_syn_cabac.rs`
and `cabac_decoder.rs`, all reading through the one accessor.

### Why it matters, and why the fix is 5.2's rather than a patch

The conflict exists because one object has two owners: the NAL unit holds the
`BsReader`, and the layer holds a pointer to it that outlives every borrow anyone takes.
That is exactly what `pBitStringAux` is — session D's closure calls it "not an owned
field but a borrow of another object outliving its source" — and 5.2 deletes it. Once
the CABAC engine takes its window from the same `(buf, cursor)` pair its caller already
split, rather than re-deriving it through the layer, there is one path and no conflict.

A local patch is possible (hand `cabac_rbsp_window` the `start` it needs instead of a
`&BsReader`, so the shared reference never covers the cursor) and it is a smaller change
than 5.2. It is not obviously right: it would leave the second path in place and make
the accessor's contract subtler at exactly the moment 5.2 is due to delete it.

### Why no other gate can see it

Same class as F24/F25: no byte moves. The battery agreed with it through the port's
whole life — 449/443 tests, 56 golden rows, 2316 malformed rows, 341/341 sweeps in both
profiles — and it took a Miri run through the public API and the slice loop, which is
the instrument T5.D2 built and F26's experiment pointed at this site.

### The fix (T5.F3)

Three parts, because the conflict has two ends and one of the ends turned out to be
decoration:

* **Two of the cursor parameters were already dead.** `ParseResidualBlockCabac` and
  `ParseResidualBlockCabac8x8` take `_pBsAux: &mut BsCursor` — underscore-prefixed,
  never read, because the CABAC engine reads through `pCabacDecEngine` and
  `cabac_rbsp_window`. They were minting a strongly protected borrow over the very
  object the engine reads, for nothing. **Deleted**, with their 6 arguments.
* **The three live ones are `*mut BsCursor` now** (`ParseIntra4x4Mode`,
  `ParseIntra8x8Mode`, `ParseIntra16x16Mode`), with `&mut *pBsAux` re-derived at each of
  the 7 `BsGet*` uses — S25's "no borrow outlives one expression", S29's spelling. The
  CAVLC callers are unchanged: `&mut T` coerces to `*mut T` at the call.
* **The two CABAC helpers stop calling `split()`.** `WelsDecodeMbCabacIntraModeHelper`
  and `WelsDecodeMbCabacResidualHelper` derive `addr_of_mut!((*pBsRd).cursor)` and take
  the byte window straight from `sRawData` — no reference to the reader, so nothing for
  `cabac_rbsp_window`'s `&BsReader` to conflict with.

---

## F28 — the layer is borrowed across calls that reach it again through the context, in 20 functions of `decode_slice.rs`

**Status: FIXED 2026-08-12 (Phase 5 session F, T5.F3).** Owner was 5.6 by file and
5.2 by subject; fixed here because the probe stopped on it. **This is the first item of
F26's S22 backlog** — the defect existed before T5.F2 and Miri could not see it, because
the layer came from `WelsMalloc` and a wildcard read does not pop a `Unique`.

### The defect

```text
error: Undefined Behavior: attempting a write access using <tag>, but that tag does not
       exist in the borrow stack for this location
  --> src/decoder/decode_slice.rs:3888   *(*dq).pChromaPredMode.add(iMbXy) = iCode as i8;
help: <tag> was created by a Unique retag        (ParseIntra4x4Mode's `let dq = &mut *pCurDqLayer`)
help: … invalidated by a read through the parent (parse_mb_syn_cabac.rs:1410,
                                                  ParseIntraPredModeChromaCabac)
```

`ParseIntraPredModeChromaCabac` re-reaches the layer the ordinary way — `let pCurDqLayer
= (*pCtx).pCurDqLayer;` then reads through it. That read is *through the parent tag*,
and a read pops `Unique` items above it, so the caller's `&mut` dies and the caller's
next write through it is UB. **F25's law, one level down**: it is not only the god
context: any object reachable from `pCtx` behaves this way, and the layer is reachable
from `pCtx` by construction.

### Scale, greped once Miri named the shape (S24, and the technique from session E)

**20 functions in `decode_slice.rs`** bind the layer as `&mut` — 8 as
`&mut *pCurDqLayer`, 12 as `&mut *(*pCtx).pCurDqLayer` — and **10 of them derive a
further borrow that outlives an expression** (`&mut (*dq).sLayerInfo.sSliceInLayer`,
which several store into a `*mut SSlice` local and read for the rest of the function),
with 6 more nested one level below that (`&mut (*pSlice).sSliceHeaderExt.sSliceHeader`).
Those are S29's escaping class, the half that "hurts everything downstream".

### The fix

`dq` becomes `*mut SDqLayer` in all 20; 207 uses re-spelled `(*dq).`; the 16 nested
borrows re-derived with `addr_of_mut!`; their uses re-spelled the same way. Zero bytes
moved. Three of the 20 already bound `dq` raw and only their null checks needed
restoring — worth saying because the mechanical pass rewrote `dq.is_null()` into
`(*dq).is_null()` and only the compiler caught it.

---

## F29 — 30 CABAC context pointers are derived through `&mut` of the array, and four of them are live in pairs

**Status: FIXED 2026-08-12 (Phase 5 session F, T5.F3).** Owner 5.5 by rights
(`pCabacCtx` is a context field), fixed here because the spelling is a standing rule
(S29) and the probe cannot proceed past it. **Second item of F26's backlog.**

### The defect

`(*pCtx).pCabacCtx.as_mut_ptr()` takes `&mut` of the array first, so it retags `Unique`
over the whole of it (`[0x7e479..0x7e811]`) and invalidates every pointer previously
derived from it. `ParseSignificantMapCabac` and `ParseSignificantCoeffCabac` each keep
**two** live at once:

```rust
let pMapCtx  = (*pCtx).pCabacCtx.as_mut_ptr().add(map_base  + …);
let pLastCtx = (*pCtx).pCabacCtx.as_mut_ptr().add(last_base + …);   // kills pMapCtx
…
DecodeBinCabac(…, pMapCtx.offset(iCtx), …)                          // UB
```

This is **F13's `as_mut_ptr()` shape**, the one T5.B2 found six times in
`manage_dec_ref.rs`, in a file nobody had looked at for it.

### The S24 instance worth keeping

The first fix pass re-pointed **25 sites and missed the 4 that mattered**, because those
four are formatted across three lines each — `(*pCtx)` / `.pCabacCtx` / `.as_mut_ptr()`
— and a line-anchored `grep 'pCabacCtx.*as_mut_ptr'` cannot see them. Miri had *named*
two of them in its diagnostic and the next round trip reported the identical error at
the identical line. **The count came from a grep of the code and was still a summary of
the fact**: the grep's unit was the line, and the code's unit was the expression. Cost:
one 8-minute round trip.

### The fix

One helper, `cabac_decoder::cabac_ctx_base`, returning
`addr_of_mut!((*pCtx).pCabacCtx).cast::<SWelsCabacCtx>()` — no reference anywhere in the
derivation, so every pointer carries the context allocation's own provenance and none
can invalidate another. All 30 sites route through it (29 in `parse_mb_syn_cabac.rs`,
1 in `cabac_decoder.rs`).

---

## F30 — `ParseSignificantCoeffCabac` walks its coefficient pointer one element before the array

**Status: FIXED 2026-08-12 (Phase 5 session F, T5.F3). Owner 5.6.** Third item of F26's
backlog, and the first one that is not an aliasing defect at all.

### The defect

```text
error: Undefined Behavior: in-bounds pointer arithmetic failed: attempting to offset
       pointer by -4 bytes, but got alloc… which is at the beginning of the allocation
  --> src/decoder/parse_mb_syn_cabac.rs:3016   pCoff = pCoff.offset(-1);
```

The C++ is `pCoff--` (`parse_mb_syn_cabac.cpp:1394`) at the bottom of a `while (i >= 0)`
loop, so the last iteration lands one element *before* `pSignificant` and the loop then
exits on `i`. The value is never dereferenced. In C that is a benign idiom; in Rust
`offset` past the start of an allocation is UB **by the arithmetic alone** — which is
[`phase1_findings.md`](phase1_findings.md) **F7's class**, the one T3.3 deleted from
`InitReadBits` rather than preserved.

### The fix

`wrapping_offset(-1)`: same address, no UB, no behaviour change — S6 parity rather than
repair. Greped for siblings: no other negative `offset` remains in
`parse_mb_syn_cabac.rs`, `decode_slice.rs` or `cabac_decoder.rs`.

---

## F31 — the SPS/PPS `memcpy` was translated as a typed copy, so the `memcmp` beside it reads uninitialized padding

**Status: FIXED 2026-08-12 (Phase 5 session G, T5.G1). Owner 5.5** (`nalu.rs`'s paramset
store is P4/§2.2.6), fixed here because it is what the probe stopped on and the fix is
three lines. **Not an aliasing defect** — the first thing this probe has found that
isn't, and the first defect it found after F25's inventory closed.

### The defect

```text
error: Undefined Behavior: reading memory at alloc253221[0x1fe0..0x2378], but memory is
       uninitialized at [0x231d..0x2320], and this operation requires initialized memory
  --> core/src/slice/cmp.rs:157   compare_bytes(lhs as _, rhs as _, size) == 0
     3: decoder::nalu::bytes_equal::<decoder::parameter_sets::TagSps>  (nalu.rs:436)
     4: decoder::nalu::ParseSps                                        (nalu.rs:1593)
```

`alloc253221` is the decoder context (568,592 bytes = `0x8ad10`), so the uninitialized
operand is the **stored** SPS, not the freshly parsed one. The three bytes are
`sVui + 1`: `TagVui` opens with `bAspectRatioInfoPresentFlag: bool` and the next field is
a 4-aligned `u32`, so `[0x33d..0x340]` inside `SSps` is interior padding.

### Why it was uninitialized, which is the whole finding

`au_parser.cpp` runs a three-legged idiom and it only works whole:

```c
memset (pSubsetSps, 0, sizeof (SSubsetSps));                       // 1: zero, padding included
memcpy (&pCtx->sSpsPpsCtx.sSpsBuffer[iSpsId], pSps, sizeof (SSps)); // 2: byte copy, padding carried
memcmp (&pCtx->sSpsPpsCtx.sSpsBuffer[iSpsId], pSps, sizeof (SSps)); // 3: byte compare
```

The port translated leg 2 as `copy_nonoverlapping::<SSps>(src, dst, 1)` — a **typed**
copy. A typed copy does not carry padding: Rust leaves the destination's padding
uninitialized no matter how initialized the source was. Leg 3 then reads exactly those
bytes. Leg 1 had the same hole: `let mut s: SSubsetSps = std::mem::zeroed()` produces a
zeroed *value* and then **moves** it into the binding, and a move is a typed copy too, so
the binding's padding is uninitialized before the parse even starts.

`ParseSps`'s own comment has explained since the function was written that the zeroing
exists so the byte comparison is meaningful, and that leftover padding "would read as a
changed SPS and force a spurious new sequence, resetting the DPB mid-stream". The
comment was right about the stakes and the code discarded the zeroing one line later.
**The prose was not wrong, and it was not evidence** — S24's failure mode with the
summary and the fact written by the same hand.

### Why no other gate could see it

Byte-identical output proves nothing here: on this compiler the padding happens to be
carried by the memcpy LLVM emits for a typed copy, so the comparison reads the same
zeros it would have read anyway. 341/341 sweeps, 56 golden rows and the 1080p bench all
pass on top of it, and would keep passing until an optimizer decided to skip the padding
— at which point the symptom is a DPB reset on a stream that never changed its SPS.
This is F30's class (an idiom benign in C, UB in Rust by the operation alone) reached
from the opposite direction: F30's arithmetic was visibly odd, this looks like ordinary
translation.

### The fix

One helper, `nalu::bytes_copy<T>` — `copy_nonoverlapping` at `*mut u8` over
`size_of::<T>()`, which *is* `memcpy` — and all **10** paramset stores route through it
(6 SPS/subset-SPS, 2 more subset, 2 PPS). Both `mem::zeroed()` temps get a
`write_bytes(.., 0, size_of::<T>())` over their own storage, which is the `memset`. The
`bytes_equal` doc comment now states the contract it depends on, and names `bytes_copy`
as the half that keeps it true.

S6 parity, not repair, in the strong sense: the fix makes the Rust do what the C++ does,
and the defect was introduced in translation.

### Sibling check (S13 — run the instrument everywhere it could apply)

`bytes_equal` has exactly three call sites, all in `nalu.rs`, all covered. The two
remaining typed `copy_nonoverlapping` of whole structs in the file — `ExpandAuList`'s
`SNalUnit` copy and the scaling-list array copies — feed nothing that is compared
byte-wise, so they stay as they are (S6: parity, not a sweep). **The general rule this
leaves for 5.5 and Phase 6**: a struct that is ever compared or hashed as bytes must
only ever be *stored* as bytes, and the two halves are usually written in different
functions by different people.

---

## F32 — two of the grid's 24 arrays declare a scalar pointee and allocate a per-MB array

**Status: FIXED 2026-08-12 (Phase 5 session G, T5.G2). Owner 5.2**, and it is a
precondition of 5.2's own conversion rather than a defect in the running code — nothing
misbehaves today. Found by cross-checking every grid field's declared pointee against
its allocation size before starting the flip, which took one script and no round trips.

### The mismatch

```text
field                 declared      allocated per mb            element really is
pIntraPredMode        *mut i8       numMb * 8  * sizeof(i8)     [i8; 8]
pIntra4x4FinalMode    *mut i8       numMb * 16 * sizeof(i8)     [i8; 16]
```

The other 22 agree with their allocations exactly. Both of these are indexed as arrays
at every use — `pIntraPredMode.add(iMbXy * 8 + 7)`, `pIntra4x4FinalMode.add(iMbXy * 16 +
g_kuiScan4[i])` — so the code has always known the stride; only the type did not.

**And the C++ types are right, so this is a translation loss rather than a design
choice** (`codec/decoder/core/inc/dec_frame.h:85-86`):

```c
  int8_t (*pIntraPredMode)[8];   //0~3 top4x4 ; 4~6 left 4x4; 7 intra16x16
  int8_t (*pIntra4x4FinalMode)[MB_BLOCK4x4_NUM];
```

Pointer-to-array-of-8 became pointer-to-`i8` on the way across, and the comment naming
the slot layout came across as nothing at all. The fix restores both.

### Why it matters, given nothing is broken

5.2 turns these 24 fields into owned `MbArray<T>`s, and `T` comes from the declared
pointee. Read mechanically — which is how a 700-site conversion has to be read — these
two become `MbArray<i8>` and allocate **8× and 16× too little**, with the overrun landing
in whatever the allocator hands out next. The natural gates would not catch it quickly:
the writes are to `[7]` and to scan-order slots, so a small stream can touch only the
first element of each and pass.

This is the closure's own arithmetic disagreeing with itself. `phase5.md` §2 records
"F19's check discharges clean at **field** level — 24 arrays against 24 frees"; that is
true and it is a check of *pairing*, not of *size*. **Pairing an allocation with its free
does not check that either one agrees with the type** — the two frees here are as wrong
as the two allocations, in the same direction, which is exactly why they pair.

### The fix

Declarations corrected to `*mut [i8; 8]` and `*mut [i8; 16]`, and the 24 sites re-spelled
to match: allocation and free by `size_of::<[i8; N]>()`, indexed uses as
`(*p.add(iMbXy))[k]`, and the flat-pointer consumers (`LD32`/`ST32` over four modes, the
4x4/8x8 predictor walks) taking `.add(iMbXy).cast::<i8>()` — a pointer cast, no reference,
same address, provenance still the whole allocation. One S29 escaping borrow went with
it: `let pMode = &mut *(*dq).pIntraPredMode.add(iMbXy * 8 + 7)` is now `addr_of_mut!`.

Zero bytes moved. Done *before* the flip rather than inside it, so that 5.2 reads the
element types off honest declarations.

### The instrument, which is the reusable part

Cross-check declared pointee size against allocation expression, per field, mechanically.
It is a dozen lines of script over the struct definition and the allocation block, it
runs in a second, and it is worth doing on **any** struct about to become owned — the
encoder's `SDqLayer`/`SMbCache` in 6.3 have the same shape and have never been checked
this way. S24's law, aimed at types instead of counts.

## F33 — two of the grid's arrays have no reader in either tree, and one of them has no writer either

**Status: FIXED 2026-08-12 (Phase 5 session H, T5.H1). Owner 5.2**, and like F32 it is a
precondition of the flip rather than a defect in running code: nothing misbehaves, and
nothing ever could, because nothing reads what they hold. Found the same way F32 was —
by inventorying the 24 arrays before converting them, not by a gate.

### The inventory

```text
field                       writes   reads   allocation
pNzcRs                        0        0     numMb * 24 * sizeof(i8)
pInterPredictionDoneFlag     14        0     numMb * sizeof(i8)
```

`pNzcRs` is allocated (`decoder_core.cpp:1552`), aliased onto the layer (`:2471`) and
freed (`:1711`), and no other line in `codec/` mentions it. It has been an allocation
with neither a reader nor a writer for the whole life of both trees.

`pInterPredictionDoneFlag` is written `= 0` at 14 sites in `decode_slice.cpp` — once per
macroblock on every parse path, in both entropy coders — and read at none. Write-only,
also in both trees. The port is faithful to the C++ at all 14.

### Why it matters, given nothing is broken

Same reason as F32, and it is the other half of the same job. `MbGrid`'s field union is
transcribed off the allocation block, so an array that survives into the union survives
into a **safe** API, where it becomes a thing future readers reasonably assume something
reads. Deleting them before the transcription costs 2 of 24 arrays and 2 of 27
allocations, and removes 14 stores per macroblock from the parse path.

The two are not the same kind of dead, and the difference is worth keeping. `pNzcRs`
would be found by any "is this ever mentioned" sweep. `pInterPredictionDoneFlag` would
not: it has 14 live writes, so every reference-counting instrument reports it as used,
and only a read/write **split** shows it. An unread write is invisible to exactly the
tools that find unused fields.

### The instrument

Count reads and writes separately, per field, over both trees. Comment-strip first —
`pInterPredictionDoneFlag`'s naive count is 19, of which 5 are lifecycle and several
more sit in prose (S16's floor, and it lands on inventories as readily as on metrics).
The encoder's `SDqLayer`/`SMbCache` in 6.3 have never been read this way either.

---

## F34 — `ParseTransformSize8x8FlagCabac` reads its own array while holding the `&mut` its caller handed it

**Status: OPEN as of discovery, FIXED in the same session (Phase 5 session I, T5.I2).
Owner 5.2.** Pre-existing at `75188044`: T5.H8 flipped `pTransformSize8x8Flag` onto the
grid, which turned the parameter from `*mut bool` into `&mut bool`, and a `&mut`
argument is *strongly protected* for the duration of the call. Found by the window
analysis T5.I1 needed — not by a gate — and then **proved rather than asserted**, on a
standalone twenty-line reproduction under Miri.

### The shape

`decode_slice.rs:4198` and `:4312` hand the callee a window into the current
macroblock's entry:

```rust
ParseTransformSize8x8FlagCabac(pCtx, pNeighAvail, (*dq).grid.transform_size8x8_flag.get_mut(iMbXy))
```

and the callee (`parse_mb_syn_cabac.rs:1228`) reads the **same array** at the left and
top addresses before writing through that argument:

```rust
let iIdxA = if (*pNeighAvail).iLeftAvail != 0 {
    *(*pCurDqLayer).grid.transform_size8x8_flag.get(iMbXy - 1) as i32
} else { 0 };
...
*bTransformSize8x8Flag = uiCode != 0;
```

The two addresses are different, which is why this looks fine and is not. `MbArray::get`
is `&self.data[i]`, `data` is a `Vec`, and `Vec`'s `Index` goes through `Deref` —
`slice::from_raw_parts(self.as_ptr(), self.len)` — which builds a shared reference over
the **whole buffer**. That retag removes the protected `Unique` above it, and the write
two lines later goes through a dead tag.

### The proof

Twenty lines, no crate involved, `cargo +nightly miri run`:

```text
error: Undefined Behavior: not granting access to tag <630> because that would remove
       [Unique for <720>] which is strongly protected
    --> alloc/src/vec/mod.rs:1865:13
     |
1865 |   &*core::intrinsics::aggregate_raw_ptr::<*const [T], _, _>(self.as_ptr(), self.len)
help: <720> is this argument
    --> src/main.rs:12:18  |  unsafe fn callee(flag: &mut bool) {
```

### Why every gate was green on it

Two conditions have to hold together, and the probe's stream meets neither. The parse
needs `bTransform8x8ModeFlag` — a High-profile feature — and the neighbour reads are
gated on `iLeftAvail`/`iTopAvail`. `decode_slice_loop_runs_under_the_aliasing_checker`
decodes `narrow_16x16.264`, which is **one macroblock per frame** (the test's own
comment says so), so both availability flags are 0 and neither read executes. Miri ran
this function and returned green because the two lines that make it UB were never
reached. The byte gates cannot see it at all: the C++ does the identical thing and the
output is identical.

**S22's law, aimed at a stream instead of at a scope list.** The aliasing probe's
coverage is its *asset*, not its target list — and a 711-byte one-macroblock stream
cannot exercise a neighbour-dependent path. Every conclusion of the form "the probe is
green, so the flip is sound" is bounded by what that stream decodes.

### It is not a port divergence

`parse_mb_syn_cabac.cpp:391` takes `bool& bTransformSize8x8Flag` and reads
`pTransformSize8x8Flag[iMbXyIndex - 1]` in the same body. In C++ that is well-defined:
references carry no exclusivity and the two elements are distinct objects. The port is
**faithful**; what changed is that `&mut` means something stronger than `&`. This is the
class to expect wherever the flip turns a `T*` out-parameter into `&mut T` on an array
the callee also reads — and the grep for it is narrow: exactly two call sites pass a
grid window as an argument, both to this function. `CheckIntraChromaPredMode` is the
other `&mut` into a flipped family and is clean by signature — it takes a `u8` and the
borrow, and never reaches the layer.

### The fix

The caller keeps the value in a local and stores it through the window afterwards, so
no borrow into the array is live across the call:

```rust
let mut bTransformSize8x8Flag = false;
let ret = ParseTransformSize8x8FlagCabac(pCtx, pNeighAvail, &mut bTransformSize8x8Flag);
if ret != ERR_NONE { return ret; }
*(*dq).grid.transform_size8x8_flag.get_mut(iMbXy) = bTransformSize8x8Flag;
```

Byte-exact by construction, including on the error path: the callee returns before its
write, so the grid entry keeps its prior value either way, and both call sites propagate
the error immediately.

---

## F35 — the B-direct MV paths read a stack `[i16; 2]` as an `i32`, and the block helpers only work because `WelsMallocz` over-aligns

**Status: OPEN as of discovery, FIXED in the same commit (Phase 5 session J, T5.J1).
Owner 5.2.** Pre-existing for the port's whole life. **Found by the second probe
stream on its first run** — the run that exists because F34 showed every Miri verdict
this phase issued was bounded by a one-macroblock stream.

```text
error: Undefined Behavior: accessing memory based on pointer with alignment 2,
       but alignment 4 is required
    --> src/decoder/mv_pred.rs:1005:13
     |
1005 | if (*(iMvp[LIST_0].as_ptr() as *const i32) | *(iMvp[LIST_1].as_ptr() as *const i32)) != 0 {
     |
             0: decoder::mv_pred::PredMvBDirectSpatial
             1: decoder::decode_slice::WelsDecodeMbCabacBSlice
```

### Two halves, and only one of them was UB yet

**Half one — 13 sites that were already UB.** `PredMvBDirectSpatial`,
`FillSpatialDirect8x8Mv`, `FillTemporalDirect8x8Mv` and their 8x8 variants pun
*stack locals* — `iMvp: &mut [[i16; 2]; 2]`, `pMV: [i16; 4]`, `pMvDirect: [[i16; 2]; 2]`
— through `*const i32` / `*mut u32`. An `i16` array is 2-aligned and nothing rounds a
stack slot up to 4, so these were unconditionally UB on every B slice that reached
them. The same file has had `LD32`/`ST32` — `read_unaligned`/`write_unaligned` wrappers
— since the port was written; these sites simply never used them.

**Half two — `SetRectBlock` and `CopyRectBlock4Cols`, which are UB *the moment 5.2
flips their arrays*.** Both take `*mut u8`/`*const u8` and store 2 and 4 bytes at a
time into `pRefIndex` (`[i8; 16]`, align 1) and the MV arrays (`[[i16; 2]; 16]`,
align 2). Today the addresses happen to be 16-byte aligned because every one of those
arrays comes from `WelsMallocz`. That is a property of the allocator, not of the data:
`MbArray<[i8; 16]>` is a `Vec`, its allocation is align 1, and the first family commit
that moves `pRefIndex`, `pMv` or `pMvd` onto the grid makes all 78 accesses UB at once.

### Why no gate saw it

The same reason F34 was invisible, one step further out. `PredMvBDirectSpatial` needs a
**B slice** *and* a neighbouring macroblock; `narrow_16x16.264` has neither — the C++
encoder that builds it emits no B slices at all, and its frames are one macroblock. So
the direct-mode family had never executed under Miri in any form. The byte gates cannot
see it either: on aarch64 and x86-64 an unaligned 4-byte load is the same instruction as
an aligned one, so the output is byte-identical and always has been. This is a
soundness defect with no observable behaviour — the class the whole Miri gate exists
for.

### The fix

Every wide access in the family goes through the unaligned spelling: the 13 direct-mode
sites through the file's own `LD32`/`ST32`, and the two block helpers through
`read_unaligned`/`write_unaligned` directly. Byte-exact by construction — same bytes,
same order, same width; what is dropped is the alignment precondition, which was the
only thing the aligned spelling ever bought. Half two is a fix ahead of its defect, and
it is in this commit rather than in the family commit that would trip it because that
family commit would otherwise carry two unrelated things, and because S13 says to run a
rule everywhere it can apply in the session that first applies it.

### What it says about the remaining flip

**A family's alignment is part of its conversion.** `WelsMallocz` returns 16-byte
alignment to every raw array in the layer; `MbArray<T>` returns `align_of::<T>()`.
For the eleven families still to flip that is a drop from 16 to 1 (`pRefIndex`,
`pDirect`, `pNzc`, `pIntraPredMode`), to 2 (`pMv`, `pMvd`, `pScaledTCoeff`,
`pChromaQp`) and to 4 (`pMbType`, `pSliceIdc`). Every family commit greps its arrays'
consumers for a wider-than-element access before it flips them, and this finding is why.


## F36 — the port's multi-threaded slice loop is a partial translation; `pSliceIdc` was the first symptom

**Status: OPEN, dormant.** Owner: whoever ports decoder threading (the T5c
"decoder threading scaffolding" entry; F12's neighbourhood). It must be fixed
**before** `GetThreadCount` returns anything greater than 1, not after.
**Widened at session L** — see the last section: two more families' writer greps
(T5.L1's `pNzc`, T5.L6's `pMbCorrectlyDecodedFlag`) landed in the same function, and
it is missing five statements, not one.

Noticed at T5.K3 while re-greping `pSliceIdc`'s writers to flip the family (S24) —
the C++ has two and the port has one.

### The divergence

`decode_slice.cpp` writes the slice id in **both** of its macroblock loops, in
identical position — first statement inside the loop, immediately before
`pCtx->bMbRefConcealed = false`:

| | computes `iSliceIdc` | writes `pSliceIdc[iNextMbXyIndex]` |
|---|---|---|
| C++ `WelsDecodeSlice` | `decode_slice.cpp:1582` | `:1593` |
| C++ `WelsDecodeAndConstructSlice` | `decode_slice.cpp:1689` | `:1708` |
| port `WelsDecodeSlice` | `decode_slice.rs:5242` | `:5257` |
| port `WelsDecodeAndConstructSlice` | **absent** | **absent** |

`grep -c SliceIdc` over the port's `WelsDecodeAndConstructSlice` body returns
**0** — it does not compute the value, let alone store it. The two C++ loops are
otherwise line-for-line the same through that region, which is what makes this a
dropped line rather than a design difference.

### What it would do if the path ran

`pSliceIdc` is reset to `0xff` bytes (i.e. `-1`) once per picture
(`decoder_core.cpp:1623`, port `decoder_core.rs:3641`) and is then written per
macroblock as each one is decoded. Every neighbour-availability predicate in the
decoder is a comparison of two entries of this array — `mv_pred.rs` ×10,
`parse_mb_syn_cavlc.rs` ×5, and `deblocking.rs`'s `iFilterIdc == 2` arm. With no
per-macroblock write, every entry stays `-1`, every comparison reads equal, and
**prediction and deblocking would cross slice boundaries as if the picture were a
single slice**. On a single-slice stream it is invisible; on a multi-slice one it
is wrong output, not a crash.

### Why nothing has caught it, and why nothing would

1. **The path is dead today.** `decoder_core.rs:3747` calls
   `WelsDecodeAndConstructSlice` only when `iThreadCount > 1`, and
   `GetThreadCount` (`decoder_core.rs:693`) returns **0** unconditionally — the
   decoder half of T9 was never ported. F22's §5 already establishes this and uses
   it to prove `WelsDecodeSlice` is the only parse entry.
2. **Even if it were live, no gate in this battery reaches it.** The decode
   goldens and `decode_1080p_bench` drive the single-threaded API path; the
   diffharness sweeps multi-thread the **encoder**. The decoder's MT path has no
   differential coverage at all. This is the general point worth carrying: a
   stubbed-out subsystem's translation is unaudited *and* uncovered, and the two
   conditions hide each other.

### What T5.K3 did about it

**Nothing, deliberately.** The flip preserves the divergence exactly: the write
that exists still happens, the write that never existed still does not, and the
family's bytes are unchanged. Fixing it here would be a behaviour change on a path
this session cannot measure, inside a commit whose contract is that it moves no
bytes (S6). The finding is recorded in the commit that noticed it, which is the
rule F35 established.
### Widened at session L: it is not one dropped line, it is a partial function (T5.L1, T5.L6)

Two more families' writer greps landed in the same place, so the finding is
restated: **the port's `WelsDecodeAndConstructSlice` is a partial translation of
the C++'s, and `pSliceIdc` was the first symptom rather than the whole of it.**

| C++ `WelsDecodeAndConstructSlice`, per macroblock | port |
|---|---|
| `pCurDqLayer->pSliceIdc[iNextMbXyIndex] = iSliceIdc` (`decode_slice.cpp:1708`) | absent (T5.K3) |
| `memcpy (pCtx->pDec->pNzc[iMbXy], pCurDqLayer->pNzc[iMbXy], 24)` (`:1722`) | absent (T5.L1) |
| `pWelsSetNonZeroCountFunc (pCtx->pDec->pNzc[iMbXy])` for non-I slices (`:1724`) | absent (T5.L1) |
| `WelsDeblockingFilterMB (pCurDqLayer, pFilter, iFilterIdc, pDeblockMb)` (`:1727`) | absent |
| the `uiNalRefIdc > 0` border-padding block (`:1729`) | absent |

The C++ function is 162 lines (`decode_slice.cpp:1620-1782`); the port's is 101
(`decode_slice.rs:5286-5387`) and its per-macroblock loop stops after
`pDecMbFunc`, the `pMbCorrectlyDecodedFlag` update and the EC bookkeeping.

A third site belongs to the same class and is not in that function:
`decoder_core.cpp:2595` re-points the layer's flag array at the picture's
(`pCurDqLayer->pMbCorrectlyDecodedFlag = pCtx->pDec->pMbCorrectlyDecodedFlag`)
**when `pThreadCtx != NULL`**, which is how the C++'s MT path gives each thread
its own per-picture copy. The port has no live `pThreadCtx` at all
(`decoder_core.rs:687` records the field as never ported), so the layer's array is
always the layer's own — which is *why* T5.L6 could make it an owned `MbArray`
without changing anything, and equally why whoever ports threading cannot simply
turn the arm back on.

**What this does not change:** the owner, the dormancy, and the deadline. The arm
runs only when `GetThreadCount` exceeds 1 and it returns 0 unconditionally. What
changes is the size of the job: the fix is not "add the missing `pSliceIdc`
write", it is "translate the rest of the function, and decide where the
per-picture arrays live when more than one thread has one". Both flips left the
divergence exactly as they found it (S6).

---

## F37 — `DestroyPicBuff` does not reset the reordering picture buffers, so a re-init leaves `sPictInfoList` naming slots of a freed pool

*Found Phase 5 session N (2026-08-13), reading `CreatePicBuff`/`DestroyPicBuff`
against the C++ for T5.N1's rewrite. **Not fixed here.** Owner: 5.5, or Phase 8 if
5.5 does not reach the lifecycle.*

C++ `decoder.cpp:260` opens `DestroyPicBuff` with

```c
ResetReorderingPictureBuffers (pCtx->pPictReoderingStatus, pCtx->pPictInfoList, false);
```

before it frees a single picture. The port's `DestroyPicBuff`
(`pic_queue.rs`) does not, and takes its `pCtx` parameter as `_pCtx`.

The port calls `ResetReorderingPictureBuffers` in exactly **one** place, and it is
decoder *creation*: `codec_api.rs:2023`, inside `WelsCreateDecoder`. So the reset
never runs on teardown.

**What that costs.** `CWelsDecoderImpl::sPictInfoList` holds, per buffered
picture, an `iPOC` and an `iPicBuffIdx` — an index into `pPicBuff->ppPic`.
`EmitBufferedPicture` (`codec_api.rs`) reads that index back and decrements the
named picture's `iRefCount`, calling its `pSetUnRef`. Across an uninit/re-init
cycle — `WelsFreeDynamicMemory` runs `DestroyPicBuff`, and a later
`InitialDqLayersContext` builds a new pool — the list keeps its old POCs and old
indices, and the emit path indexes the **new** pool with the **old** pool's index.
The entries are not stale pointers (the port stores an index, and T5.N1's
`slot_at` bounds-checks it), so this is a wrong-picture bug rather than a
use-after-free: a POC of `IMinInt32` is what marks a slot empty, and a surviving
non-sentinel POC makes a stale slot look occupied.

**Why no gate sees it.** Every decode gate in the battery creates a decoder,
decodes, and destroys it. Nothing re-initialises a live decoder, which is the
only way to reach the state — the same shape as F31 and F36, defects behind a
transition no asset exercises.

**FIXED at Phase 5 session O (T5.O1).** The reset is restored to the head of
`DestroyPicBuff`, before the early returns, exactly where the C++ has it; the `pCtx`
null-guard is the port's own. A test walks the cycle the public API exposes — dirty the
two buffers as a decode leaves them, destroy the pool, assert the slots no longer name
it — and pins the `fullReset = false` extent (`iLargestBufferedPicIndex + 1`, not 16).
**The fix immediately convicted F38**, which had been sitting under it: the first read
the port ever made through `pCtx->pPictReoderingStatus` found a pointer whose tag had
been popped.

**Why it is not fixed at session N.** The reset function is `decoder_core.rs`'s
and the list it resets is `codec_api.rs`'s; the session's brief fences both, and
the fix is one line in a file whose whole lifecycle 5.5 rewrites. Adding the call
without the surrounding conversion would put a `pCtx`-reaching call inside a
function that currently ignores its `pCtx` — S20's "atomically, per closure"
argument running the wrong way. Listed with its owner per S18.


---

## F38 — the eight decoder-object back-pointers are derived through `&mut`, and every write through the object itself pops them

*Found Phase 5 session O (2026-08-13), by the small aliasing probe, in the first
minute after F37's fix made the port read through one of them. **Fixed in the same
session** (S29's spelling, three files). Sibling of F24/F25/F28/F29 and the first
instance of the class **outside** `src/decoder/`.*

`decoder_init_c` (`api/codec_api.rs`) mirrors C++ `CWelsDecoder::InitDecoderCtx` by
wiring eight of `CWelsDecoderImpl`'s own members into the decoder context:

```rust
ctx_box.pMemAlign            = &mut (*dec_impl).align;
ctx_box.pParam               = &mut (*dec_impl).param;
ctx_box.pLastDecPicInfo      = &mut (*dec_impl).sLastDecPicInfo;
ctx_box.pDecoderStatistics   = &mut (*dec_impl).sDecoderStatistics;
ctx_box.pVlcTable            = &mut (*dec_impl).sVlcTable as *mut _ as *mut c_void;
ctx_box.pPictInfoList        = (*dec_impl).sPictInfoList.as_mut_ptr();
ctx_box.pPictReoderingStatus = &mut (*dec_impl).sReoderingStatus;
ctx_box.pStreamSeqNum        = &mut (*dec_impl).iStreamSeqNum;
```

Every one is S29's **worst class**: a reference to a field of a raw-reached struct,
immediately coerced to a raw pointer and **stored into another struct** that outlives
the expression by the decoder's whole life. The coercion retags that field's range as
SharedReadWrite on top of `dec_impl`'s own tag — and the next write through `dec_impl`
itself pops everything above it, including the stored pointer. `codec_api.rs` makes
such writes constantly; the two the probe reached are

```rust
(*dec_impl).sReoderingStatus.iLastWrittenSeqNum = (*dec_impl).sPictInfoList[idx].iSeqNum;   // :1672
```

and its reorder twin. After either, `pCtx->pPictReoderingStatus` is a dead tag.

**Miri's verdict, verbatim**, at the first read the port ever made through it:

```
error: Undefined Behavior: attempting a read access using <1029891> at alloc267873[0x1558],
       but that tag does not exist in the borrow stack for this location
  --> src/decoder/decoder_core.rs:1931   (ResetReorderingPictureBuffers)
help: <1029891> was created by a SharedReadWrite retag ... at codec_api.rs:1440
help: <1029891> was later invalidated ... by a write access at codec_api.rs:1672
```

**Why it never fired before, and this is the interesting part.** The context's copy of
`pPictReoderingStatus` had **one** reader in the whole port and it was dead code; every
live use went through `(*dec_impl).sReoderingStatus` directly. So the port stored eight
pointers it had invalidated and dereferenced none of them on the probe's path. F37's
one-line fix added the first such dereference, and the defect convicted on the next
Miri run. *A stored pointer that is never read is not a fixed defect; it is an
unexercised one* — which is S22's law about instruments, aimed at data instead.

**The fix** is S29's, unchanged since F24: `ptr::addr_of_mut!` at all eight sites. It
creates no reference, so the derived pointer carries `dec_impl`'s provenance and "whose
retag is on top" stops being a question. Both probes green afterwards.

**Two more sites of the same shape, found by grepping for it rather than by paying for
another probe round trip** (the rate-beating move F27–F30 established), both fixed
here:

* `deblocking.rs:2178` and `:2235` — `pFilter.pLoopf = &mut (*pCtx).sDeblockingFunc`,
  a borrow of a context field stored into `SDeblockingFilter` and held for the whole
  macroblock loop. It does not fire today only because nothing writes
  `sDeblockingFunc` after `WelsInitDecoderFuncs`.
* `decoder_core.rs:3628` — `(*pCtx).iDecBlockOffsetArray.as_mut_ptr()`, which takes a
  `&mut [i32; 24]` of a context field first. Transient rather than stored, so it is the
  mild form.

**What it says about the phase's inventory.** T5.G1 closed the `&mut *pCtx` inventory
for `src/decoder/` and F28 generalised it to "anything reachable from `pCtx`, held
across a call". Both were scoped to the decoder's own modules. `src/api/` is the module
the plan exempts from `deny(unsafe_code)` forever (§2.2.8), and exempting it from the
*lint* quietly exempted it from the *sweep*: eight instances of the phase's signature
defect sat in the file that is supposed to be "a few hundred lines of pure
translation". Phase 8 inherits the module; it does not inherit an audit.
