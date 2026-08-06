# Memory-Safety Refactoring Plan for the Rust H.264 Decoder

Status: **design proposal — no code changes yet**
Scope: `rust/crates/openh264-rs`, primarily `src/decoder/` and `src/common/`.
The encoder (`src/encoder/`) shares every pattern described here and can follow the
same playbook as a separate effort once the decoder approach is proven.

---

## 1. Goals and non-goals

### Goals

1. Eliminate raw pointers and `unsafe` from the decoder internals, module by module,
   while preserving **bit-exact output** (verified by the existing SHA-1 conformance
   suite after every step).
2. Concentrate all remaining `unsafe` into a single, auditable place: the C-ABI shim.
3. End state: `#![forbid(unsafe_code)]` on `src/decoder/` and `src/common/`,
   idiomatic ownership everywhere else.

### Non-goals

- **The external FFI C interface stays unchanged.** `src/api/codec_api.rs`
  (the `ISVCDecoder`/`ISVCEncoder` vtables, `#[repr(C)]` parameter/output structs
  such as `SBufferInfo`, `SDecodingParam`, `SParserBsInfo`) remains a C-shaped,
  `unsafe` boundary layer. It shrinks to a thin adapter but keeps its ABI.
- No behavioral changes, no new features, no algorithmic rewrites. The C++ in
  `codec/` remains the reference for any ambiguity.
- Performance parity is a constraint but micro-optimization is not a goal;
  `benches/c_vs_rust_bench.rs` gates regressions.

---

## 2. Current state (census, 2026-08)

The port is a deliberate line-by-line transliteration of the C++: Hungarian
naming, `#[repr(C)]` structs, raw pointers, C vtables, and a hand-rolled
aligned allocator. That was the right call for achieving bit-exactness; it is
also why nearly every function is `unsafe`.

| Metric | Value |
|---|---|
| Decoder + common LOC | ~37,000 |
| `pub unsafe fn` / `pub unsafe extern "C" fn` (decoder+common+api) | 562 |
| `unsafe { }` blocks in `src/` | ~570 |
| Raw-pointer struct fields in decoder+common | ~111 |
| Files with zero `unsafe` | 2 of 31 (`vlc_tables.rs`, near-zero: `slice.rs`, `parameter_sets.rs`) |

Heaviest files: `decoder_core.rs` (95 unsafe fns), `decode_slice.rs` (47),
`get_intra_predictor.rs` (42), `parse_mb_syn_cabac.rs` (41),
`parse_mb_syn_cavlc.rs` (35), `deblocking.rs` (31), `mc.rs` (29).

Two facts make this refactoring much more tractable than it first appears:

- **Threading is already stubbed out.** `OpenDecoderThreads` is a no-op; the
  decoder is single-threaded. No `Send`/`Sync`/atomics puzzle — plain
  single-owner borrows suffice.
- **All dispatch tables point to the `_c` scalar kernels.** There is no SIMD
  assembly behind the function pointers, so the tables can become direct calls
  or safe `fn` pointers without losing anything that exists today.

---

## 3. Taxonomy of unsafe patterns

Every `unsafe` in the decoder falls into one of eight patterns. Each has a
standard safe replacement; the phases in §5 apply them in dependency order.

### P1. Pixel kernels: pointer-into-the-middle-of-a-plane

`get_intra_predictor.rs`, `mc.rs`, `deblocking_common.rs`, `decode_mb_aux.rs`
(IDCT), `sad_common.rs`, expand-picture. Signature style:

```rust
pub unsafe extern "C" fn WelsI4x4LumaPredV_c(pPred: *mut u8, kiStride: i32) {
    let v = (pPred.offset(-kiStride as isize) as *const u32).read_unaligned();
    ...
}
```

Two things make `&mut [u8]` non-trivial here:

- **Negative indexing**: intra prediction reads neighbor pixels *above and to
  the left* of the pointed-to position (`pPred[-stride - 1]`), i.e. the pointer
  is deliberately into the interior of the plane.
- **Type-punned accesses**: `u32`/`u64` unaligned loads/stores over `u8` data
  as a scalar-SIMD idiom.

**Safe replacement — `PlaneView` / `PlaneViewMut`:** a fat cursor holding the
whole plane slice plus an origin offset, with *signed relative* indexing that
is bounds-checked against the real allocation (which always includes the
`PADDING_LENGTH` border the C++ relies on):

```rust
pub struct PlaneViewMut<'a> {
    data: &'a mut [u8],   // entire padded plane (or a conservatively clamped window)
    origin: usize,        // index of the (0,0) pixel of this block
    stride: usize,
}
impl PlaneViewMut<'_> {
    #[inline] fn px(&self, dx: isize, dy: isize) -> u8 { ... }        // checked
    #[inline] fn set_px(&mut self, dx: isize, dy: isize, v: u8) { ... }
    #[inline] fn row(&mut self, dy: isize, len: usize) -> &mut [u8] { ... }
    #[inline] fn fill_row4(&mut self, dy: isize, v: u32) { ... }      // replaces u32 stores
}
```

The `u32` puns become `u32::from_le_bytes`/`to_le_bytes` on 4-byte subslices or
explicit per-byte writes — the compiler vectorizes the fixed-size loop bodies
well, and the benchmark suite verifies it. Kernels become safe
`fn(&mut PlaneViewMut, ...)`.

### P2. Bitstream cursors

`SBitStringAux` carries `pStartBuf`/`pEndBuf`/`pCurBuf` raw pointers;
`SDataBuffer` carries `pHead`/`pEnd`/`pStartPos`/`pCurPos`. Used by
`bit_stream.rs`, `dec_golomb.rs`, `cabac_decoder.rs`, both parse modules, and
NAL demuxing in `nalu.rs`/`decoder_core.rs`.

**Safe replacement:** index-based readers over owned/borrowed slices:

```rust
pub struct BitReader<'a> { buf: &'a [u8], pos: usize, cur_bits: u32, left_bits: i32, index: usize }
pub struct DataBuffer { buf: Vec<u8>, start: usize, cur: usize }   // sRawData/sSavedData
```

The C++ overread-guard semantics (`InitReadBits` end-offset checks,
`ERR_INFO_READ_OVERFLOW`) map directly to checked slice accesses. This is the
lowest-risk conversion in the codebase because the state machine (32-bit
accumulator, `iLeftBits`) is pure arithmetic; only the byte-fetch changes.

### P3. Per-macroblock layer maps

`SDqLayer` holds ~25 raw pointers to per-MB arrays (`pMbType: *mut u32`,
`pMv: [*mut [[i16; 2]; 16]; 2]`, `pNzc: *mut [i8; 24]`,
`pScaledTCoeff: *mut [i16; 384]`, …), each allocated with `CMemoryAlign` and
indexed by `iMbXyIndex`. `SMbCache` mirrors them.

**Safe replacement:** owned vectors, one struct:

```rust
pub struct MbMaps {
    mb_type: Vec<u32>,
    mv: [Vec<[[i16; 2]; 16]>; 2],
    nzc: Vec<[i8; 24]>,
    scaled_tcoeff: Vec<[i16; 384]>,
    ...  // one Vec per current raw pointer, all of length mb_width * mb_height
}
```

Indexing by `mb_xy: usize` replaces pointer arithmetic one-for-one. This kills
the single largest population of raw-pointer fields and needs no ownership
cleverness — the maps are plainly owned by the layer.

### P4. The picture pool, DPB, and reference lists (the hard part)

The aliasing graph today:

- `SPicBuff` owns pictures via `ppPic: *mut *mut SPicture`.
- `SRefPic` holds *aliasing* pointers to the same pictures in up to six lists
  (`pRefList`/`pShortRefList`/`pLongRefList` × LIST_0/LIST_1), plus
  `pCtx->pDec`, `pCtx->pTempDec`, `pDqLayer->pRef`, `pDqLayer->pDec`,
  `pCtx->pPreviousDecodedPictureInDpb`, the reorder buffer in
  `CWelsDecoderImpl`, and `iRefCount`/`pSetUnRef` manual refcounting.
- Motion compensation reads one or two reference pictures while writing the
  current picture; error concealment copies between the last-good picture and
  the current one.

Raw pointers here cannot be replaced by `&`/`&mut` — the lists are exactly the
shared-mutable-graph shape Rust borrows reject. **Safe replacement: an
index-based pool.**

```rust
pub struct PicId(u32);                       // index into the pool; Option<PicId> replaces nullable ptrs

pub struct PicturePool { pics: Vec<Picture> }  // owns all DPB pictures & their planes (Vec<u8>)

impl PicturePool {
    fn get(&self, id: PicId) -> &Picture;
    fn get_mut(&mut self, id: PicId) -> &mut Picture;
    /// Split-borrow for MC / concealment: current picture mutably, refs shared.
    fn cur_and_refs(&mut self, cur: PicId, refs: &[PicId]) -> (&mut Picture, Vec<&Picture>);
}

pub struct RefPic {
    ref_list:       [[Option<PicId>; MAX_DPB_COUNT]; 2],
    short_ref_list: [[Option<PicId>; MAX_DPB_COUNT]; 2],
    long_ref_list:  [[Option<PicId>; MAX_DPB_COUNT]; 2],
    ...
}
```

`cur_and_refs` is implemented once with a small amount of interior
`split_at_mut`-style logic (or `Cell`-free raw indexing hidden behind a safe,
assert-checked API); the H.264 invariant that a picture never references
itself makes the disjointness assert always true — and if a corrupted stream
ever violated it, we get a clean panic instead of UB, which is precisely the
upgrade we want. `iRefCount`/`pSetUnRef` callbacks become plain methods on the
pool (`unref_pic(id)`), since threading is out of the picture.

### P5. The context god-object and self-referential wiring

`SWelsDecoderContext` has ~100 fields including raw pointers back into
`CWelsDecoderImpl` (`pParam`, `pLastDecPicInfo`, `pDecoderStatistics`,
`pPictInfoList`, `pStreamSeqNum`, `pVlcTable`, `pMemAlign`) wired at init —
a self-referential structure crossing the API boundary.

**Safe replacement:** invert the wiring — move ownership of those members *into*
the context (the C++ only keeps them outside to survive context recreation in
the threaded path, which the Rust port doesn't have; `InitDecoderCtx`
recreation semantics can be preserved by an explicit
`fn reinit(&mut self, keep: ReorderState)` that carries the survivors over).
The decoder entry points become methods:

```rust
impl WelsDecoderContext {
    pub fn decode_frame(&mut self, src: &[u8], dst_info: &mut SBufferInfo) -> DecodingState;
}
```

`CWelsDecoderImpl` then holds `ctx: Option<Box<WelsDecoderContext>>` and
nothing else, and the vtable shims are 3-line wrappers.

### P6. The custom allocator

`CMemoryAlign` / `WelsMalloc` / `WelsFree` implement aligned malloc with a
hidden header via FFI `malloc`. Every allocation it serves is replaced in P1–P5
by `Vec`/`Box`. When the last caller disappears, the module is deleted.
If future SIMD needs alignment, use an aligned wrapper
(`#[repr(align(64))] struct CacheLine([u8; 64])` + `Vec<CacheLine>`) — not a
custom allocator. The memory-usage accounting (`m_nMemoryUsageInBytes`) can be
kept as a plain counter if we care to preserve the statistics API.

### P7. Function-pointer dispatch tables

`pGetI4x4LumaPredFunc: [PGetIntraPredFunc; 14]`, IDCT, MC, deblocking, copy,
expand tables — all `Option<unsafe extern "C" fn(*mut u8, i32)>`, all
initialized to the sole `_c` implementations.

**Safe replacement (two steps):**
1. Change the type to safe `fn(&mut PlaneViewMut, ...)` pointers as kernels are
   converted in P1 — tables stay tables, calls lose `unsafe`.
2. Optionally collapse to `match` on the mode index (the C++ only has tables
   for runtime CPU dispatch we don't do). Keep the table shape if SIMD is a
   realistic future; a `trait Kernels` impl per ISA is the idiomatic endpoint.

### P8. Misc hygiene

- `unsafe { std::mem::zeroed() }` in `Default` impls (e.g. `SDqLayer`) →
  derive/implement `Default` field-wise; pointer fields disappear anyway.
- `*const c_char` tags and log strings → `&'static str`.
- `#![allow(unused_unsafe, dead_code, ...)]` headers → removed per module as it
  converts; add `#![deny(unsafe_op_in_unsafe_fn)]` crate-wide immediately (it
  is already the pattern in most files).
- `Copy + Clone` on huge structs (`SPicture`, `SDqLayer`) — remove `Copy` once
  they own `Vec`s; accidental copies of 4-KB structs also disappear.

---

## 4. Target architecture

```
codec_api.rs (unsafe, unchanged ABI)          ← ONLY remaining unsafe
    │  thin adapter: raw C structs ⇄ safe types
    ▼
WelsDecoderContext (safe, owns everything)
    ├── params, stats, reorder state           (owned values, was ptr-wired)
    ├── DataBuffer (raw NAL bytes)             (Vec<u8> + indices, was SDataBuffer)
    ├── AccessUnit / Vec<NalUnit>              (owned, was WelsMallocz'd ptr lists)
    ├── SpsPpsCtx: [Sps; 32], [Pps; 256]       (owned arrays; PicId-free, index by id)
    ├── PicturePool { Vec<Picture> }           (owns planes as Vec<u8>)
    │       ▲ PicId indices only
    ├── RefPic { [[Option<PicId>; ..]; 2] }    (was aliasing *mut Picture lists)
    ├── DqLayer { MbMaps { Vec<..> per map } } (was 25 raw ptrs via CMemoryAlign)
    ├── BitReader<'a> over DataBuffer          (was SBitStringAux)
    └── kernels: safe fn tables / match        (was unsafe extern "C" tables)
```

Rules of the end state:

1. **Ownership is a tree** rooted at the context; every cross-reference that
   isn't tree-shaped is a `PicId`/index, never a pointer or long-lived borrow.
2. **Borrows are function-scoped**: a decode step borrows what it needs
   (`&mut pool`, `&maps`, a `BitReader`) for the duration of a call, mirroring
   the C++ call graph so the diff against reference stays reviewable.
3. **Panics replace UB** for impossible states (corrupted-stream index out of
   bounds), while recoverable stream errors keep the existing `i32` error-code
   returns (converting to `Result` is a later, mechanical change and
   deliberately *not* bundled into this refactoring).

---

## 5. Phased migration plan

Ordering principle: **leaves first, ownership last**. Each phase converts a
layer whose callees are already safe, keeps the module boundaries identical to
the C++ (so the reference-diff workflow keeps working), and lands only when the
full conformance suite hashes match. Phases are independent merge units;
within a phase, each module conversion is a separate commit.

### Phase 1 — Safe pixel kernels (`PlaneView`)
Modules: `get_intra_predictor.rs`, `common/mc.rs`, `common/deblocking_common.rs`,
`decoder/deblocking.rs`, `decode_mb_aux.rs`, `common/intra_pred_common.rs`,
`common/sad_common.rs`, expand-picture in `decoder_core.rs`.

- Introduce `PlaneView`/`PlaneViewMut` in `common/`.
- Convert kernels to safe fns; change dispatch-table types (P7 step 1).
- Call sites still hold raw pointers: a one-line adapter
  (`PlaneViewMut::from_raw(ptr, stride, bounds)` — `unsafe`, lives at the call
  site) bridges until Phase 4 removes it. The unsafety doesn't vanish yet;
  it moves out of ~200 kernel bodies into ~30 constructor calls.
- **Perf checkpoint**: run `c_vs_rust_bench`; these are the hot loops. Accept
  ≤5% regression per kernel class, else adjust (`row()`-slice writes instead of
  per-pixel `set_px`, `from_le_bytes` word ops).

### Phase 2 — Safe bitstream and entropy decoding
Modules: `bit_stream.rs`, `dec_golomb.rs`, `cabac_decoder.rs`,
`parse_mb_syn_cavlc.rs`, `parse_mb_syn_cabac.rs`, `vlc_tables.rs` glue.

- `BitReader<'a>` replaces `SBitStringAux`; `DataBuffer` replaces `SDataBuffer`
  (`sRawData`, `sSavedData`, the SPS/PPS BS snapshot buffers).
- Golomb/CAVLC/CABAC readers become methods; overflow checks become the natural
  slice-bounds checks with the same error codes.
- `nalu.rs` EBSP→RBSP unescape rewritten over slices.

### Phase 3 — Owned macroblock maps
Modules: `decoder_core.rs` (alloc/free paths), `decode_slice.rs`, `mv_pred.rs`,
`fmo.rs`, users of `SDqLayer`/`SMbCache`.

- `MbMaps` struct (P3) replaces the 25 pointer fields; `SMbCache` layer
  indirection (`LAYER_NUM_EXCHANGEABLE == 1`) collapses.
- `fmo.rs` `pMbAllocMap` → `Vec<u8>`.
- Neighbor access (`iMbXyIndex ± 1/width`) becomes checked indexing with the
  existing availability flags guarding edges, exactly as in C++.

### Phase 4 — Picture pool, DPB, reference lists
Modules: `picture.rs`, `pic_queue.rs`, `manage_dec_ref.rs`,
`error_concealment.rs`, DPB parts of `decoder_core.rs` & `decoder_context.rs`.

- `PicturePool` + `PicId` (P4); planes become `Vec<u8>` inside `Picture`;
  `pData`/`pBuffer` pointer pairs become `origin` offsets (feeding straight
  into Phase 1's `PlaneView` and deleting the `from_raw` adapters).
- `SRefPic`, reorder buffer (`sPictInfoList`), `pLastDecPicInfo` all switch to
  `Option<PicId>`.
- Manual refcount/`pSetUnRef` → pool methods.
- This phase is the largest and riskiest; it should be split into
  (a) planes-as-Vec inside `SPicture` with pointers derived on demand,
  (b) lists-as-indices, (c) delete `AllocPicture`/`FreePicture`/`CMemoryAlign`
  usage in the pool.

### Phase 5 — Context ownership and API slimming
Modules: `decoder_context.rs`, `decoder_core.rs`, `api/codec_api.rs` internals.

- Invert the `CWelsDecoderImpl` ↔ context wiring (P5); context becomes a normal
  owned struct with methods; delete `Box::into_raw`/`from_raw` juggling.
- SPS/PPS: `pSps: *mut SSps`/`pPps: *mut SPps` (and the copies inside
  `SLayerInfo`) become ids into the owned parameter-set arrays.
- Delete `CMemoryAlign` (P6), `WelsMalloc*`, the FFI `malloc`/`free` externs.
- Flip `#![forbid(unsafe_code)]` on `src/decoder/` and `src/common/`;
  `codec_api.rs` keeps its `unsafe` vtable shims (raw-pointer validation,
  struct translation) and nothing else.

### Phase 6 — Encoder (separate effort, same playbook)
The encoder's ~1,100 unsafe sites map to the same taxonomy (P1 kernels are in
`encode_mb_aux`/`sad_common`, P2 is the CAVLC/CABAC *writer*, P3 is
`SMbCache`/strip maps, P4 is `ref_list_mgr_svc`, P5 is `sWelsEncCtx`, plus a
real P-threading dimension in `slice_multi_threading.rs` that the decoder
doesn't have — that one needs scoped threads or rayon over disjoint slice
partitions). Reuses `PlaneView`, `BitReader`'s writer twin, and the pool
pattern from Phases 1–5.

---

## 6. Verification strategy

Every phase, every module commit:

1. **Conformance hashes**: `decoder_conformance_test.rs` + `e2e_conformance_test.rs`
   must produce identical SHA-1s. **Also compare decoded-frame counts**, not
   just hashes — a decode error silently drops frames from the hash stream and
   can mask a regression as a "different but stable" hash.
2. **Differential ground truth**: for any divergence, diff against C++ `h264dec`
   YUV output (not JVT/ffmpeg golds on B-slice streams, where upstream C++ is
   not bit-exact), replicating the exact API call flow (`DecodeFrame2` vs
   `DecodeFrameNoDelay` matters for ordering).
3. **Sanitizer/miri sweep** while unsafe still exists: run the conformance
   subset under `cargo miri` (unit-level) and ASan (integration) to catch
   latent UB that the refactoring perturbs — transliterated C often "works"
   only by allocator accident, and reshuffling allocations exposes it.
4. **Fuzzing**: a `cargo-fuzz` target feeding the annex-B entry point pays off
   double here — before the refactor it finds the UB we're eliminating; after,
   it can only find panics. Add it in Phase 2 when the bitstream reader
   becomes safe.
5. **Benchmarks**: `c_vs_rust_bench` after Phases 1, 3, 4 (the phases touching
   hot paths). Budget: no more than ~5% end-to-end decode regression at any
   phase boundary; investigate with `--emit asm` / `perf` before accepting.
6. **Unsafe census in CI**: a script counting `unsafe` occurrences per module,
   asserted non-increasing, so progress ratchets.

---

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Bounds checks slow hot pixel loops | Fixed-size row slices + `from_le_bytes` word ops let LLVM elide most checks; benchmark per phase; `get_unchecked` inside `PlaneView` (still one audited module) only as a proven last resort |
| `cur_and_refs` disjointness violated by corrupt stream (self-reference) | Checked at the pool API → panic, which is a safety *improvement* over today's aliasing UB; fuzz to confirm no reachable panic on the corpus |
| Error concealment copies from "last good picture" that may be the current one | Audit `error_concealment.rs` flows in Phase 4; where C++ truly self-copies, use in-place `copy_within` on the single `&mut Picture` |
| Phase 4 ripples across many modules at once | Sub-phase it (planes → lists → allocator); keep `#[repr(C)]`-shaped accessors temporarily so untouched modules compile |
| Bit-exactness drift hides for several commits | Hash + frame-count gate on every commit, not every phase |
| Losing the C++-diffability that the port workflow relies on | Keep module/function names and control flow 1:1 through Phase 4; rename to idiomatic Rust only after `forbid(unsafe_code)` lands (Phase 5+), if at all |
| Encoder/decoder share `common/` — decoder phases must not break encoder | `common/` kernels get dual signatures during transition (safe fn + thin `unsafe extern "C"` wrapper for encoder callers) until Phase 6 |

---

## 8. Sizing (rough, sequential)

| Phase | Scope | Estimate |
|---|---|---|
| 1 | ~10 kernel modules, `PlaneView` | 1.5–2 weeks |
| 2 | bitstream + entropy | 1.5–2 weeks |
| 3 | MB maps | 1 week |
| 4 | pool/DPB/ref lists | 2–3 weeks (riskiest) |
| 5 | context + API slimming | 1 week |
| 6 | encoder | ~4–5 weeks (has real threading) |

Decoder total: **~7–9 weeks** of focused work, mergeable in ~30 independent,
individually verified commits.
