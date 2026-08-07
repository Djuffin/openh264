# Eliminating raw pointers and `unsafe` from the openh264 Rust port

**Status:** proposal, 2026-08-07 (rev 2: drop-in C ABI is now a requirement — see §2.2.8, D2 resolved). Written against branch `rust3` @ `eb463dbd`.
**Scope:** everything under `rust/crates/openh264-rs/` (src, tests, benches) plus `rust/tools/diffharness/`.
**Goal:** the crate builds as a **drop-in replacement for libopenh264** — same exported symbols, same vtable-based `ISVCEncoder`/`ISVCDecoder` object model, same `#[repr(C)]` structs — while *everything behind that boundary* is safe Rust: zero raw pointers, zero `unsafe`, zero `unsafe impl Send/Sync` anywhere except the single FFI boundary module (`src/api/`), which shrinks to a thin, auditable translation layer. Every byte-exactness gate that exists today keeps passing at every intermediate step.

---

## 1. Where we are

### 1.1 Inventory

The port is a deliberate, faithful C transliteration: `#[repr(C)]` structs, Hungarian names, pointer graphs identical to the C++, `Option<unsafe extern "C" fn>` dispatch tables, a re-implementation of C's aligned-malloc header trick on top of `extern "C" malloc/free`, and `Default` via `mem::zeroed()`. That was the right call for getting to byte-exactness; it is also why the crate is wall-to-wall unsafe.

| Metric (src/ only) | Count |
|---|---|
| Lines of Rust | 85,182 in 74 files |
| `unsafe fn` | ~922 (469 in decoder/ = 69% of all decoder fns) |
| `unsafe {}` blocks | ~612 |
| `*mut` / `*const` type tokens | ~5,000 |
| Raw derefs `(*p…` | ~11,000 (≈4,300 decoder, ≈6,900 encoder) |
| Pointer arithmetic (`.add/.offset/.sub/.offset_from`) | ~4,100 |
| `mem::transmute` | 23 (21 in `decode_slice.rs` — all fn-ptr type erasure) |
| `unsafe impl Send`/`Sync` | 10 (5 type pairs, all in encoder threading) |
| `mem::zeroed()` | 26 (mostly `Default` impls of pointer-laden structs) |
| `#[unsafe(no_mangle)]` exports | ~42 |

**The single most important config fact:** `src/lib.rs:10` sets `#![allow(unsafe_op_in_unsafe_fn)]`. Inside the 922 `unsafe fn` bodies, every deref and every `.add()` is unguarded and invisible to `unsafe {}` counting. The *real* unit of work is "unsafe fn bodies converted", not "unsafe blocks removed". Progress must be measured with a ratchet script (§7.1), not by grepping for `unsafe {`.

### 1.2 Why the unsafe exists — a taxonomy

Every unsafe use in the crate falls into one of ten categories. Each has a standard safe replacement; none requires novel research. This table is the spine of the whole plan.

| # | Pattern | Where it lives | Safe replacement |
|---|---|---|---|
| T1 | **Malloc-style allocation + manual free cascades.** `CMemoryAlign` header-trick allocator over `extern malloc/free`; 141 call sites encoder-side; 300-line `WelsUninitEncoderExt` free cascade; 10 allocations per decoder picture. Three coexisting allocator families (`CMemoryAlign`, `Box::into_raw`, `alloc_zeroed`). | `common/memory_align.rs`, `encoder/encoder_ext.rs:998-1863`, `decoder/pic_queue.rs:177-405`, `decoder/decoder_core.rs:2955-3101` | Owned `Vec<T>`/`Box<T>` fields + `Drop`. Alignment is only needed for future SIMD; `#[repr(align(16))]` wrappers where it matters. |
| T2 | **Pixel-plane cursors with stride math and negative offsets into padding.** `pData[i]` points into the *middle* of a padded allocation (32 px luma / 16 px chroma borders); MB cursors advance by `.add(16)`; MV clamp is the only bounds check; border expansion writes above row 0. | `decode_slice.rs:1944, 1072-1091`, `decoder_core.rs:637-651`, `svc_base_layer_md.rs:327-358`, all DSP kernels | A `PaddedPlane` owner + `(x, y)`-addressed views where logical coordinates may be negative but the *biased* slice index is always in range (§2.2.1). |
| T3 | **Detachable byte/bit cursors.** `SBitStringAux{pStartBuf,pCurBuf,pEndBuf}` (reader *and* writer), `SDataBuffer` (4 pointers), CABAC engines' second cursor triple, CAVLC↔CABAC cursor handoff, dynamic-slice rollback snapshots of `pCurBuf`, and `ExpandBsBuffer` manually rebasing every outstanding pointer after realloc (`decoder_core.rs:1758-1842`). | `common/wels_common_defs.rs:30-46`, `decoder/{bit_stream,dec_golomb,cabac_decoder,nalu}.rs`, `encoder/{vlc_encoder,set_mb_syn_cabac,svc_set_mb_syn_cavlc,nal_encap}.rs` | Offset-based cursors that don't carry a buffer reference (§2.2.2). Deletes the realloc-rebase code outright; rollback snapshot becomes a saved `usize`. |
| T4 | **Multi-alias object graphs.** One `SPicture` reachable through up to 9 decoder locations (DPB pool, ref lists, `pDec`, `pECRefPic`, per-picture `pRefPic` graph with cycles…) and 4 encoder locations. Three pointer-identity comparisons in the decoder carry real semantics; the encoder has none. | `decoder/{pic_queue,manage_dec_ref,picture}.rs`, `encoder/{ref_list_mgr_svc,encoder_context}.rs` | Picture arena + copyable `PicId` handles; identity comparison = handle equality (§2.2.3). |
| T5 | **Derived-pointer caches / interior self-reference.** Encoder `SMB` fields point into ctx-level flat arrays (5 pointers × ~8k MBs, wired once at `encoder_ext.rs:611-617`); decoder `SDqLayer` per-MB arrays alias `pCtx->sMb` — the *same allocation reachable through two paths* (`decoder_core.rs:3843-3869`); `pSps/pPps` point into arrays inside the same struct; `SMbCache` sub-allocates one aligned block; `pMvdCost` points into the middle of a table. | `encoder/encoder_ext.rs`, `decoder/decoder_core.rs`, `encoder/md.rs:352-384`, `svc_motion_estimate.rs:176-195` | Delete the cached pointer, keep the index: `mb_idx * stride` recomputed at use, `sps_id` instead of `pSps`, `(table, bias)` instead of mid-pointers. Struct-of-arrays `MbGrid` with one owner (§2.2.4). |
| T6 | **Function-pointer dispatch tables.** Decoder: ~15 table families + 21 transmutes for type erasure. Encoder: `SWelsFuncPtrList`, 70 members, ~180 call sites — but ~55 members only "select" between N identical scalar functions (all SIMD variants delegate to `_c`); ~15 are genuine algorithm dispatch. Plus three hand-rolled *internal* C++ vtables (`IWelsVP`, `IWelsParametersetStrategy` 16 entries, `IWelsReferenceStrategy` 7 entries). The public `ISVCEncoder/ISVCDecoder` vtable emulation is **not** in this category — it is the drop-in ABI contract and stays (T10/§2.2.8). | `encoder/wels_func_ptr_def.rs`, `decoder/decoder_core.rs:1913-1995`, `encoder/paraset_strategy.rs:50-115`, `ref_list_mgr_svc.rs:1560-1574`, `processing/mod.rs` | Direct calls for CPU dispatch (nothing dispatches to different code today); `enum` + `match` for config dispatch; `Box<dyn Trait>` for the two real strategy objects; safe `fn(...)` pointers are still allowed where a table is genuinely convenient (§2.2.5). |
| T7 | **Word-punning (`LD32/ST32` idiom).** 4 pixels as one unaligned `u32`; NZC cache fills via `ST32(p, LD32(q))`; `(pCache as *mut u16).write_unaligned` over `i8` arrays (provenance-violating today). ~140 punned accesses in decoder `get_intra_predictor.rs` alone, 92 in `mv_pred.rs`. | `decoder/get_intra_predictor.rs`, `mv_pred.rs:247-455`, `parse_mb_syn_cavlc.rs:694-748`, `svc_mode_decision.rs:817,846` | `u32::from_ne_bytes`/`to_ne_bytes` on slice windows, or `copy_from_slice` for the pure 4-byte moves. Same codegen, zero unsafe. |
| T8 | **`*mut ctx` threading of the god-structs.** `SWelsDecoderContext` (112 fields, 28 pointers), `sWelsEncCtx` (76 fields, 37 pointers), passed as `*mut` through every signature; `rc.rs` has 751 raw derefs that are *only* `(*pCtx).field` accesses. | everywhere | `&mut` receivers on decomposed subsystem structs; field-precise parameters where borrows must split (§2.2.6, R1). |
| T9 | **Thread-sharing via laundered pointers.** `TaskPtr(*mut dyn IWelsTask)`, ctx pointers cast to `usize` to cross `thread::spawn`, 5 `unsafe impl Send/Sync` pairs, `Mutex` boxed and erased to `*mut c_void` to match C field types. Disjointness is *already* index-based (`m_iThreadIdx` slot claiming); sync is *already* `std::sync`. Decoder has **no** threading (stubs only). | `common/wels_thread_pool.rs`, `encoder/wels_task_management.rs`, `slice_multi_threading.rs:162-308` | Scoped threads / a safe persistent pool with `Box<dyn FnOnce + Send>` jobs over channels; per-thread resources become owned disjoint chunks (§2.2.7). |
| T10 | **C-ABI ceremony.** Vtable structs + 24 `extern "C"` thunks + `Box::into_raw` factories; `#[repr(C)]` on ~everything; `abi_guard.rs` size asserts; `mem::zeroed` Defaults; `struct_bytes_eq` memcmp equality. | `api/codec_api.rs`, both `abi_guard.rs` | Split by role: the *public* surface (vtables, thunks, factories, POD structs, `api/abi_guard.rs`) is the drop-in ABI contract — it **stays**, rewired internally onto safe `Decoder`/`Encoder` cores, as the crate's one `unsafe` module (§2.2.8). The *internal* ceremony — `repr(C)` on codec-internal structs, `encoder/abi_guard.rs` asserts, `mem::zeroed` Defaults, memcmp equality — gets deleted as structs de-C-ify. |

### 1.3 What is already safe (build on it, don't reinvent)

- `decoder/vlc_tables.rs`, `slice.rs`, `parameter_sets.rs`, both `abi_guard.rs`, `encoder/param_svc.rs` — effectively value-semantics already.
- `manage_dec_ref.rs` opens nearly every fn with `let ctx = &mut *pCtx;` and then uses safe field syntax — 80% converted in spirit; it's the template for the T8 recipe.
- `sad_common.rs:415-441` has three unused slice-based SAD wrappers — the kernel-conversion proof of concept.
- `WelsTaskBarrier` (`wels_task_management.rs:627-659`) is a fully safe `Mutex<i32>`+`Condvar` — the model for the thread pool rewrite. `CWelsList<T>` is already a `Vec` wrapper.
- `tests/common/` (sha1, y4m) is 100% safe; `split_annexb_units` and `TC0_TBL_LOOKUP` are safe islands already used from unsafe code.
- `benches/decode_1080p_bench.rs:162` defines a `trait Decoder` abstracting the Rust and dlopen'd C++ decoders behind one interface — the natural seam for the future safe public API.

### 1.4 The safety nets (why this refactor is tractable at all)

1. **Byte-exactness gates already exist**: decoder conformance (SHA-1 per plane over ~20 JVT streams), e2e vs gold `.y4m` + ffmpeg cross-check, encode/decode loopback SHA-1, option-sweep parity vs C++, and the diffharness (`rust/tools/diffharness/`) comparing whole bitstreams against a C++ build, including a threads dimension and slice-mode sweeps.
2. **The external ABI is a commitment, not a constraint inherited by accident.** Today the crate is rlib-only, so nothing outside the repo can call it — every consumer is in-repo Rust, which means the refactor can rewire the *inside* of the API layer freely, with the tests as the witness. But the decided end state (D2, resolved 2026-08-07) is a **drop-in `libopenh264` replacement**: the vtable emulation, the 7 upstream exports, and the `#[repr(C)]` structs pinned by `api/abi_guard.rs` are permanent public surface. Upstream's own header design is what makes this sound: the C++ interfaces have **no virtual destructors** and pin `EXTAPI` (`__cdecl` on Windows), precisely so that the C struct-of-function-pointers view and the C++ Itanium/MSVC vtable view coincide — the port's existing `lpVtbl` emulation is already that shape. All 7 exports upstream ships already exist in the port (`WelsCreateSVCEncoder`, `WelsDestroySVCEncoder`, `WelsCreateDecoder`, `WelsDestroyDecoder`, `WelsGetDecoderCapability` in `codec_api.rs`; `WelsGetCodecVersion`, `WelsGetCodecVersionEx` in `wels_encoder_ext.rs`). Known fidelity gap to carry, not fix, in this plan: `DecodeParser`/`DecodeFrameEx` are real upstream vtable slots that the port stubs — the slots stay (layout!), the stub behavior is documented as a limitation.
3. **The C++ tree is the permanent reference**: every phase keeps function-level correspondence with `codec/` so divergences can still be triangulated with h264dec ground truth and the gtest repro flow.

Known verification caveats (from prior sessions, they still apply): a Rust decoder error silently drops frames from the conformance hash — **always compare frame counts, not just hashes**; JVT/ffmpeg golds can't judge B-slice streams where upstream C++ itself diverges (the `#[ignore]`s in e2e are permanent fixtures, their set must not change — measured 2026-08-07 as **20** tests, all in `tests/e2e_conformance_test.rs`, not the 13 earlier revisions of this plan claimed; `cargo test --test e2e_conformance_test -- --ignored --list` is the authority); all C++-vs-Rust perf numbers are scalar-vs-scalar because the C++ dylib never dispatches NEON.

Two sharper caveats about the *encoder's* evidence were measured 2026-08-07 at `eb463dbd`; both were resolved later the same day, with one loose end that Phase 0 owns:

1. **The `GetDefaultParams`-path divergence: found and fixed** (`fa67432f`). Rust `FillDefaultExt` (`encoder/param_svc.rs`) dropped two assignments the C++ `FillDefault(SEncParamExt&)` makes (`codec/encoder/core/inc/param_svc.h:181-182`): `bFixRCOverShoot = true` (first bites at the 10th coded frame, the first VGOP boundary) and `iIdrBitrateRatio = 400` (first bites at the second IDR). The defaults path is now verified byte-identical on four sequences up to 720p, and the diffharness gained a `def` sweep preset (`909c368b`, drivers take `baseinit=2`) that runs the real default config — frame skip, adaptive quant, scene change, background detection, overshoot fix all ON — long enough to cross a VGOP boundary and a second IDR. **`sweep.sh def` is part of the gate battery from here on**, alongside `st` and `mt`. Moral for every later phase: when the C++ serves one function to two structs, diff both Rust copies.
2. **The release-build segfault: root-caused and closed** (Phase 0 task T3, 2026-08-07 — full write-up in [`phase0_findings.md`](phase0_findings.md) §F1). It was a **stack buffer overflow on the mainline encode path**: `DeblockingMbAvcbase` declared `uiBS` as `[[u8; 4]; 4]` (16 bytes) where the C++ declares `uint8_t uiBS[2][4][4]` (32 bytes), and five `from_raw_parts_mut(uiBS as *mut u8, 32)` sites in `DeblockingBSCalc_c` wrote the full 32. The reported crash site — a null `(*pCurMb).pNonZeroCount` in `WelsNonZeroCount_c` — was a *symptom of the smashed frame*, not the cause; instrumentation showed `pNonZeroCount` was never null across 378 configurations, and the faulting build actually died in `DeblockingFilterFrameAvcbase` on a null-plus-8 load. It "stopped reproducing" because the overflow is layout-sensitive and lands harmlessly in most builds: pristine `eb463dbd` release is clean, but the same build with one added `eprintln!` in `deblocking.rs` segfaults 5/5 deterministically. `e6fce464` ("encoding benchmark") widened `uiBS` to `[[[u8; 4]; 4]; 2]` without mentioning it, which is the actual fix; narrowing it back at HEAD reintroduces the crash 5/5, which is the proof. **Morals, both load-bearing for later phases:** a debug/release disagreement is UB evidence and never flakiness (§7.2 gate 0 — both profiles are now gated via `RUST_ENC_PROFILE`), and "it no longer reproduces" is not a resolution. This bug class dies structurally in Phase 2, when kernels stop taking `*mut u8` and start taking real array types that cannot be written past.

Also: commit `eb463dbd` deleted the port's handoff/status docs and the diffharness from git. The diffharness is back in-tree (`909c368b`); the handoff/status docs are still only in git history — Phase 0 recovers them.

---

## 2. Target architecture

### 2.1 Principles

1. **Byte-exact at every merge.** No phase may change any decoded pixel, any encoded bitstream, any error code, or the conformance frame counts. Refactoring and behavior change never share a commit.
2. **Indices, not pointers; owners, not graphs.** Every "many pointers to one thing" becomes "one owner + copyable indices". Every interior pointer becomes a recomputed offset. This is not just a safety move — it's what deletes `ExpandBsBuffer` rebasing, the free cascades, and the `Default = zeroed` hacks.
3. **Detached cursors.** State structs store positions (`usize`), never references into buffers. Long-lived structs holding `&'a` borrows are forbidden in the design — that's how a C port stays refactorable and how self-reference dies.
4. **Convert leaves first, gods last.** DSP kernels and bitstream cursors have thousands of call sites but trivial contracts; the context structs have trivial call counts but decide the ownership model. Do the mechanical mass early, the pivot late, with strangler shims (R7) so every PR compiles and passes gates.
5. **Zero new runtime dependencies.** The crate currently depends on `libc` alone (and only from benches); the end state depends on nothing. `std` threads, `std::sync`, `std::alloc` suffice. Dev-dependencies (fuzzing) are fine.
6. **Keep the C++ diffability until the end.** Wels names, function granularity, and comment headers stay through the structural phases; renaming is a separate, optional, final phase (§10 D5).

### 2.2 Core vocabulary types

These live in a new `src/safe/` module (name bikesheddable), built and unit-tested in Phase 1, adopted incrementally. Sketches below are contracts, not final code.

**Built as of Phase 1** (2026-08-07): `src/safe/{plane,bits,pool,mb_grid,err}.rs`, every file `#![forbid(unsafe_code)]`, 63 in-module unit tests plus 18 differential tests against the implementations they replace (`tests/safe_plane_differential.rs`, `tests/safe_bits_differential.rs`), all Miri-clean. Where the implementation deviates from a sketch below, the sketch has been updated and the deviation flagged **[P1]** with its reason — the plan must never lag the code.

#### 2.2.1 `PaddedPlane` / plane views — replaces T2

The invariant the C code relies on: a plane allocation is `(pad + h + pad) × stride` bytes and `pData` points at `(0,0)` *inside* it, so `y ∈ [-pad, h+pad)` reads are in-allocation. Encode exactly that:

```rust
pub struct PaddedPlane {
    buf: Vec<u8>,          // owns >= (pad + h + pad) * stride
    stride: usize,
    origin: usize,         // byte offset of logical (0,0); origin = pad*stride + pad
    width: usize, height: usize, pad: usize,
}
impl PaddedPlane {
    pub fn new(width: usize, height: usize, pad: usize, stride: usize) -> Self;   // zeroed
    /// Validates the invariant; `pad` is recovered as `origin % stride`. Phase 2 shims feed this.
    pub fn from_parts(buf: Vec<u8>, stride: usize, origin: usize, width: usize, height: usize) -> Self;
    #[inline] pub fn at(&self, x: isize, y: isize) -> u8;
    #[inline] pub fn set(&mut self, x: isize, y: isize, v: u8);
    #[inline] pub fn row(&self, y: isize, x0: isize, len: usize) -> &[u8];
    #[inline] pub fn row_mut(&mut self, y: isize, x0: isize, len: usize) -> &mut [u8];
    /// Movable sub-views anchored at an MB origin — the safe version of the roving `pDstY`.
    pub fn cursor(&self, x: isize, y: isize) -> PlaneCursor<'_>;
    pub fn cursor_mut(&mut self, x: isize, y: isize) -> PlaneCursorMut<'_>;
    /// Escape hatch for `chunks_exact` row-walking and whole-plane fills.
    pub fn as_slice(&self) -> &[u8];  pub fn as_mut_slice(&mut self) -> &mut [u8];
    pub fn width/height/pad/stride/origin(&self) -> usize;
}
/// Read view; `Copy`, so rebasing is a value operation.
pub struct PlaneCursor<'a>    { buf: &'a [u8],     center: usize, stride: usize }
pub struct PlaneCursorMut<'a> { buf: &'a mut [u8], center: usize, stride: usize }
// both: new(buf, center, stride) [validated], at(dx,dy), row(dy,dx0,len),
//       advance(dx,dy) -> Self (by value), center(), stride()
// mut also: set(dx,dy,v), row_mut(dy,dx0,len), as_ref() -> PlaneCursor<'_>
// One private `fn idx(center, dx, dy, stride) -> usize` does all the biasing.
```

Key points:

- **Same-plane read-while-write is a non-problem** once access goes through one `&mut` view: intra prediction reading `(-1, dy)` and `(dx, -1)` while writing `(0..16, 0..16)`, and in-place deblocking across MB edges, are serial reads/writes through a single cursor — safe Rust is fine with that. The thing that is *actually* illegal today (two live `*mut` paths to one allocation, T5) dies with the aliasing, not with the arithmetic.
- Kernels take `PlaneCursorMut` (or `(buf: &mut [u8], center: usize, stride: usize)` triples for the hottest ones) instead of `(pPred: *mut u8, kiStride: i32)`.
- The MC clamp (`decode_slice.rs:1072-1091`) stays byte-for-byte: the clamp guarantees the biased index is in range; slice indexing then *proves* it, converting a silent-corruption class into a loud panic if the port ever miscomputes.
- Cross-plane pairs (MC: read ref picture, write current) are two views of two different `PaddedPlane`s — no conflict. Same-picture src/dst in error concealment goes through a `copy_within`-style method on one plane.
- Border expansion (`ExpandPictureCommon`) becomes methods on `PaddedPlane` — it is the one place that writes the padding, and owning the padding makes it safe by construction.

#### 2.2.2 Detached bit cursors — replaces T3

```rust
/// Reader state; NO buffer reference inside. The buffer is passed to each call.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct BsCursor { pos: usize, cur_bits: u32, left_bits: i32, len: usize, bits: i32 }
//                                                               ^^^^^^^^^^^^^^^^^^^^^ [P1]

impl BsCursor {                                   // mirrors, in order:
    pub fn init(buf: &[u8], size_bits: i32) -> Result<Self, ErrInfo>;          // DecInitBits
    pub fn init_read_bits(&mut self, buf: &[u8], end_offset: isize) -> …;      // InitReadBits
    pub fn get_bits(&mut self, buf: &[u8], n: i32) -> Result<u32, ErrInfo>;    // BsGetBits
    pub fn get_one_bit / get_ue / get_se / get_te0(…);                         // BsGet*
    pub fn peek_bits(&self, n: i32) -> u32;                                    // UBITS
    pub fn check_more_rbsp_data(&self) -> bool;                                // CheckMoreRBSPData
    pub fn pos/len/bits/cur_bits/left_bits(&self);                             // state, for parity tests
}
pub fn trailing_bits(byte: u8) -> i32;                                         // BsGetTrailingBits

/// Writer: owns position only; the output slice is a parameter, as for the reader.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BsWriter { pos: usize, cur_bits: u32, left_bits: i32 }
// new() [= InitBits], write_bits, write_one_bit, write_ue, write_se, flush,
// rbsp_trailing_bits, bits_pos [= BsGetBitsPos], pos, left_bits;
// free fns size_ue/size_se [= BsSizeUE/BsSizeSE, computed rather than table-driven].
```

- **[P1] `BsCursor` carries `len` and `bits`, not three fields.** `len` is `pEndBuf - pStartBuf`, the *logical* end of the RBSP, which is not the same as the length of the slice passed in — the allocation legitimately continues past it, and the slop below depends on the difference. Without it, error-code parity at the end of a NAL is not expressible. `bits` is `iBits`, needed only by `check_more_rbsp_data`. Both are state in the C++ struct too; nothing is stored that `SBitStringAux` did not already hold.
- **[P1] The slop reads three bytes past the RBSP, not one** (and the initial prime reads four bytes from the start regardless of NAL length). `BsCursor` reproduces the *predicate* exactly and expresses the loads through `get`, so it is identical to the C++ given ≥3 bytes of slack and strictly safer without. Measured and written up as [`phase1_findings.md`](phase1_findings.md) §F4; Phase 3.1 owns the guard-byte decision.
- **[P1] `BsWriter` implements only the canonical (`vlc_encoder.rs`) semantics.** No guard, no masking, no wrapping variant is smuggled in from the other three copies — Phase 3.2 must decide those explicitly (F2). Building it turned up [`phase1_findings.md`](phase1_findings.md) §F5: the canonical writer panics in debug builds on a 32-bit write into an empty accumulator; `BsWriter` does not, and a test pins both profiles.

- `SDataBuffer{pHead,pEnd,pStartPos,pCurPos}` → `Vec<u8>` + two `usize` offsets. `ExpandBsBuffer`'s pointer-rebasing block (`decoder_core.rs:1816-1842`) is deleted, not converted — offsets survive realloc by definition.
- NAL units store `Range<usize>` into the AU buffer instead of `pNalPos: *mut u8`.
- The CABAC decoder engine stores `{ pos: usize, range: u64, offset: u64, bits_left: i32 }`; the CAVLC↔CABAC handoff (`cabac_decoder.rs:712-716`) becomes an assignment of one `usize` to another.
- The encoder's dynamic-slice rollback (`StashMBStatus`/`StashPopMBStatus`, `svc_set_mb_syn_cavlc.rs:1057-1076`) snapshots a `BsWriter` by value — it's `Copy`.
- Two deliberate C quirks must be handled consciously, not accidentally:
  1. `dec_golomb.rs:128-150` allows the read cursor to sit 1 byte past `pEndBuf` and then loads 2 bytes. Reproduce by appending ≥4 zero guard bytes to the raw-data `Vec` (deterministic, in-bounds) and keeping the same `pos > len+1 → ERR_INFO_READ_OVERFLOW` predicate. Verify on the malformed-stream tests that error codes don't shift.
  2. `BsWriteBits` has **no** end-of-buffer check (`vlc_encoder.rs:367-382`) — sizing is the caller's contract. The safe writer keeps the same unchecked *semantics* w.r.t. output bytes but gets real bounds from slice indexing; buffer sizing logic is unchanged, so a panic here means a real pre-existing overflow bug.
- The writer exists **twice** (verbatim copy at `svc_encode_slice.rs:498-585`) — dedupe before converting.

#### 2.2.3 Picture arena + handles — replaces T4

```rust
/// Index into a pool; identity == handle equality. Generation is debug-only (D1) and
/// is NOT part of `PartialEq` — equality must mean the same thing in both profiles.
#[derive(Copy, Clone, Debug)]
pub struct Id { index: u32, #[cfg(debug_assertions)] generation: u32 }

/// Generic [P1]: the encoder needs the same shape (6.1/6.2), so `PicId` becomes a
/// newtype/alias over `Id` and `PicPool` over `Pool<Picture>` in Phase 5.1.
pub struct Pool<T> { slots: Vec<T>, #[cfg(debug_assertions)] generations: Vec<u32> }
impl<T> Pool<T> {
    pub fn new(slots: Vec<T>) -> Self;   pub fn len(&self) -> usize;
    pub fn id(&self, index: usize) -> Id;         // mints a handle, stamped
    pub fn ids(&self) -> impl Iterator<Item = Id>;
    pub fn iter(&self) -> impl Iterator<Item = (Id, &T)>;   // the `find_free` predicate rides here
    pub fn get(&self, id: Id) -> &T;    pub fn get_mut(&mut self, id: Id) -> &mut T;
    pub fn pair_mut(&mut self, a: Id, b: Id) -> (&mut T, &mut T);          // panics if a == b
    /// The critical split: one picture &mut while any others are read.
    pub fn mut_and_rest(&mut self, cur: Id) -> (&mut T, PoolRest<'_, T>);  // [P1]
    pub fn replace(&mut self, id: Id, value: T) -> T;   // recycling; bumps the generation
}
pub struct PoolRest<'a, T> { … }   // get(id) -> &T, panics on the mutably-held slot
```

- **[P1] `mut_and_rest(cur)` replaces `cur_and_ref`/`cur_and_refs(cur, refs)`.** The reference *list* carried no weight: its only job was rejecting `cur ∈ refs`, which `PoolRest::get` does anyway at the point of access, and taking it would force either an allocation or an arbitrary fixed capacity into a per-macroblock path. Splitting the slot span is allocation-free, subsumes `cur_and_ref`, and handles the B-slice case (one `&mut` + two `&`, P1) without a special method.
- **`find_free` is not a method**; it is `pool.iter().find(…)`, because the predicate (`bUsedAsRef`/`iRefCount`) belongs to `Picture`, which Phase 5.1 defines.

- All nine decoder alias sites (`pDec`, `pTempDec`, ref lists, `pECRefPic`, `pPreviousDecodedPictureInDpb`, `SPicture::pRefPic`, `SDeblockingFilter::pRefPics`, thread ctx, pool) become `Option<PicId>` / `[Option<PicId>; N]`. The per-picture `pRefPic` graph (cycles!) is just data as handles — no ownership cycles because handles don't own.
- The three decoder pointer-identity comparisons that carry semantics — `deblocking.rs:258` (boundary strength: "same reference picture?"), `manage_dec_ref.rs:739` (self-copy guard), `error_concealment.rs:599` — become `PicId == PicId`, which is *exactly* the same predicate. The encoder needs no care at all: the survey found zero pointer-identity comparisons; all matching is by `iFrameNum`/`iFramePoc` values.
- Recycling can hand a stale `PicId` the same slot a fresh picture occupies — identical to the C++ hazard, so behavior is preserved; a `debug_assertions`-only generation counter catches logic rot in tests without changing release semantics. **Implemented in Phase 1** (D1 resolved); `replace` bumps it, every accessor checks it, and `PartialEq` deliberately ignores it.
- `Picture` itself becomes: 3 × `PaddedPlane` + owned `Vec`s for the per-MB metadata (`pMbType`, `pMv`, `pRefIndex`, `pNzc`, `pMbCorrectlyDecodedFlag`) + value fields. Ten manual allocations and `FreePicture` collapse into struct construction and `Drop`.
- Encoder side identical shape; the pool ownership moves out of `CWelsPreProcess` (see §3 P10 for the cycle it currently forms).

#### 2.2.4 `MbGrid` — single owner for per-MB metadata — replaces T5

Decoder today: `pCtx->sMb.pXXX[0]` (25 arrays) and `pCurDqLayer->pXXX` are the same allocations reachable through two paths — instant UB under `&mut` rules and the #1 blocker for borrow-checking `decode_slice.rs`. Encoder today: each `SMB` caches 5 pointers into ctx flat arrays.

**Phase 1 built the addressing, not the fields** — `MbDims` (grid geometry: `mb_xy`, `xy_of`, `count`, and `left`/`top`/`top_left`/`top_right` mirroring the guards at `mv_pred.rs:485-510`, availability logic deliberately excluded) and `MbArray<T>` (one owned per-MB array over an `MbDims`). The field union below is Phase 5.2's to decide, since only it knows which of the `sMb`/`SDqLayer`/`SMB` fields survive; `MbGrid` is then a struct of `MbArray`s.

```rust
pub struct MbGrid {                    // one per dq-layer; owns everything per-MB
    pub mb_type: Vec<u32>,
    pub mv: Vec<[[i16; 2]; 16]>,       // indexed [mb_xy]
    pub ref_index: Vec<[i8; 16]>,
    pub nzc: Vec<[i8; 24]>,
    pub slice_idc: Vec<i32>,
    pub scaled_tcoeff: Vec<[i16; 384]>,
    pub luma_qp: Vec<u8>, pub chroma_qp: Vec<[u8; 2]>,
    …                                   // the full union of sMb/SDqLayer/SMB-pointer fields
}
```

- Neighbor access (`left = xy-1`, `top = xy-mb_width`) is plain indexing with the existing availability checks as the bounds logic — reads of `[xy-1]` while writing `[xy]` through one `&mut MbGrid` are safe Rust, no aliasing question at all.
- The decoder's repeated `if !pDec.is_null() { pDec->pMbType } else { pCurDqLayer->pMbType }` fork stays as an explicit two-source choice (`grid` vs `pool.get(dec_id).mb_meta`) — same logic, borrow-checkable because they are genuinely different owners.
- Encoder `SMB` drops its 5 pointer fields; accessors compute `mb_idx * K` — the survey calls this "the highest-leverage single change in the port".
- `SMbCache` (encoder, `#[repr(C, align(16))]` with hand-inserted padding) becomes a plain struct of fixed-size arrays (`[u8; 256]`, `[i16; 384]`, …) with `#[repr(align(16))]` kept for future SIMD; the 13 sub-allocation pointers into one arena die with the arena. Hand-inserted padding fields and the ABI asserts for it are deleted.

#### 2.2.5 Dispatch policy — replaces T6

Three tiers, in place of every `Option<unsafe extern "C" fn>`:

1. **CPU-feature tables → direct calls.** Nothing in the port dispatches to anything but the `_c` kernel (all `_sse2/_neon/_mmi/_lsx` entries are delegating stubs or dead externs — including 62 never-referenced `extern` declarations in `mc.rs:793-863`). Delete the tables, call the function. When real SIMD arrives, reintroduce as safe fn-pointer tables selected once at init (`fn(&mut PlaneCursorMut, …)` pointers are safe types) or `#[target_feature]` behind a tiny dispatch shim — a decision for that day, invisible to this plan.
2. **Config dispatch → `enum` + `match`.** CAVLC-vs-CABAC slice writer (`pfWelsSpatialWriteMbSyn`), MD strategy family (`pfInterMd` & co., ~15 members), search method tables (`pfSearchMethod[]`), stash/pop pair, `pfRc` — all selected once from encoder config: an `enum EntropyCoder { Cavlc, Cabac }` etc., matched at the call site. The 21 `transmute`s in `decode_slice.rs` exist only to erase concrete fn types into `*mut c_void`-parameter typedefs — they disappear with the tables.
3. **Real strategy objects → traits.** `IWelsParametersetStrategy` (16-entry vtable) and `IWelsReferenceStrategy` (7-entry) become `Box<dyn ParamsetStrategy>` / `Box<dyn RefStrategy>` — textbook conversions, zero aliasing subtlety per the survey. `IWelsVP` (the processing library's 7-slot vtable, one implementation, two call sites) doesn't even need a trait: inline `SWelsVpContext` into `CWelsPreProcess` and `match` on the method enum. Watch the documented footgun: today a `None` slot silently returns success (`processing/mod.rs:5-20` records the bug this caused) — the rewrite makes those paths statically total.

#### 2.2.6 Context decomposition — replaces T8

`SWelsDecoderContext` and `sWelsEncCtx` don't survive as monoliths; `&mut everything` through one struct would fight the borrow checker at every two-subsystem call. Decompose along the lines the code already draws:

```rust
pub struct Decoder {
    params: DecodingParams,
    raw: RawDataBuffer,            // Vec + offsets (was sRawData/sSavedData)
    au: AccessUnitList,            // NAL bookkeeping, ranges not pointers
    paramsets: ParamSetStore,      // owned SPS/PPS arrays; "active" = sps_id/pps_id (kills pSps/pPps self-ref)
    dpb: PicPool,
    layer: DqLayerState,           // incl. MbGrid
    cabac: CabacState,             // ctx tables + engine (offsets only)
    dec_state: FrameState,         // pDec/pTempDec as Option<PicId>, flags, counters
    ec: EcState,
    stats: DecoderStatistics,
    last_pic: LastDecPicInfo,
    reorder: ReorderState,
    logger: Logger,
}
```

- Functions become methods on the subsystem that owns their data; cross-subsystem functions take the 2-3 parts they need as separate `&`/`&mut` parameters (`fn decode_mb(layer: &mut DqLayerState, dpb: &mut PicPool, cabac: &mut CabacState, …)`). This is the same move `manage_dec_ref.rs` already made informally.
- Wels function names are kept (`WelsDecodeSlice` stays `WelsDecodeSlice` until the optional rename phase) so the C++ diff column survives.
- `Default` becomes derivable per subsystem; all 26 `mem::zeroed()` sites die here.
- Same treatment for `sWelsEncCtx` → `Encoder { params, funcs-gone, layers, dpb, rc, vaa, preprocess, slicing, out, stats, … }` per the encoder survey's field classification (12 owned singletons, 7 owned flat arrays, 6 owned arrays-of-owned, 7 non-owning cursors→indices, ref-list aliases→`PicId`, per-thread buffers→`Vec<Vec<u8>>`).
- The logger back-pointer (`sLogCtx.pCodecInstance` pointing at the encoder itself) is replaced by a `Logger` value that owns its sink — in the safe API a `Box<dyn Fn(Level, &str)>`; the C trace-callback POD remains only as API surface translated at the boundary.

#### 2.2.7 Threading model (encoder only) — replaces T9

Facts from the survey that make this easy: tasks are fork/join per frame with a barrier; disjointness is already expressed as "task owns index `m_iThreadIdx` of several parallel arrays" (claimed via `QueryEmptyThread` under a mutex); sync primitives are already `std::sync`; a single-threaded inline fallback already exists; the C++ event machinery was already found dead and omitted. The decoder has no threading at all — its stubs get deleted, and future decoder MT (if ever) is designed fresh on the safe architecture.

Target shape:

```rust
pub struct SliceJob<'f> {
    slice_idx: usize,
    bs_buf: &'f mut [u8],          // this thread's chunk of the bs-buffer pool
    slice: &'f mut SliceState,      // from Vec<SliceState>, split per job
    // read-only shared: &EncConfig, &PicPool view, &LayerConst
}
std::thread::scope(|s| {
    for job in jobs { s.spawn(|| encode_slice(job, shared)); }
});                                  // join == the barrier
```

- Fixed slice modes: partition `Vec<SliceState>` and the per-thread bs buffers with `split_at_mut`/`chunks_mut`, one job per slice, exactly the current claiming logic minus the mutex.
- `SM_SIZELIMITED` (dynamic slicing): a work queue of slice indices via `mpsc` + per-worker owned scratch; results stitched in slice order (the assembly is already order-based, which is why MT output is deterministic today). Determinism gate: MT output byte-equals today's MT output (already exercised by diffharness's threads argument).
- The persistent pool (avoids re-spawning threads per frame) is rebuilt safely if profiling shows spawn cost matters: worker threads + `mpsc::Sender<Job>` where `Job: FnOnce + Send`, completion counted by the existing safe `WelsTaskBarrier`. `TaskPtr`, the `usize` laundering, the `*mut c_void` mutexes (`WelsMutexInit`), the `static mut GLOBAL_THREAD_POOL` (→ `OnceLock`), and all 5 `unsafe impl` pairs are deleted.
- Deblocking runs after join, as today.

#### 2.2.8 The API: safe core + preserved C-ABI boundary — replaces T10

Two layers. The **safe core API** is what the whole crate converges on internally:

```rust
pub struct Decoder { inner: DecoderState }          // Send; one instance == one C object
impl Decoder {
    pub fn new(params: &SDecodingParam) -> Result<Self, DecoderError>;
    pub fn decode_frame(&mut self, nal: &[u8]) -> Result<DecodeOutput<'_>, DecoderError>;
    pub fn set_option(&mut self, opt: DecoderOption) -> …;   // typed enum internally
    pub fn flush(&mut self) -> Option<DecodeOutput<'_>>;
}
pub struct DecodeOutput<'a> { pub planes: [PlaneRef<'a>; 3], pub info: SBufferInfo }
```

The borrowed-return problem the C API papers over (`ppDst` planes point into the DPB; `SFrameBSInfo.pBsBuf` aliases encoder buffers) is solved here with lifetime-bound views (`DecodeOutput<'a>` borrowing `&'a mut self`), matching the C contract "valid until the next call" — the borrow checker now *enforces* what the C header only documents. Exporting this layer publicly for Rust consumers is free and worth doing.

The **C-ABI boundary** (`src/api/codec_api.rs`) is permanent public surface and stays exactly as the drop-in contract requires:

- `ISVCEncoderVtbl`/`ISVCDecoderVtbl` structs, the `lpVtbl`-first object layout, all vtable slots **including** the `DecodeParser`/`DecodeFrameEx` stubs (slot order is ABI; stub behavior is a documented limitation), the 7 `no_mangle` factories/version functions, and `EXTAPI` calling-convention parity. What *changes* is only what's behind the thunks: `CWelsDecoderImpl { base, pVtbl, pCtx: *mut SWelsDecoderContext, … }` becomes `{ base, pVtbl, inner: Decoder }` — each thunk null-checks, downcasts `pThis`, translates raw arguments to safe types, calls the safe core, and translates back (filling `SFrameBSInfo`/`ppDst` with pointers derived from the borrowed views, whose validity window is identical to today's).
- `SEncParamExt`, `SDecodingParam`, `SFrameBSInfo`, `SBufferInfo`, `SParserBsInfo`, `OpenH264Version` & co. stay `#[repr(C)]`; `api/abi_guard.rs` grows to pin every struct that crosses the boundary (add the few it doesn't yet cover).
- `(ENCODER_OPTION, *mut c_void)` Get/SetOption pairs: the boundary does the `void*` decoding it already does, then calls typed internals. The trace-callback option stores the raw C `(fn, void*)` pair inside the boundary and wraps it as the internal `Logger` sink — the only place a C callback is ever invoked.
- Crate becomes `crate-type = ["rlib", "cdylib", "staticlib"]`. Soname/version-script naming to match a system `libopenh264.so.N` is a packaging concern outside the crate (document the linker flags; don't encode a distro's soname in Cargo).
- Threading contract of the C API: instances are not internally synchronized but must be usable from different threads (one thread at a time per instance), and separate instances must work concurrently. Safe core enforces this by construction: `Decoder: Send` / `Encoder: Send` asserted with compile-time tests, no thread-locals, and the encoder's process-wide thread-pool singleton (matching C++'s refcounted global) lives in an `OnceLock`.
- **This module is the one place `unsafe` survives**, under `#![deny(unsafe_code)]` at the crate root with a scoped `#[allow(unsafe_code)]` on `api` (deny, not forbid, precisely so this exception can exist). Target size: a few hundred lines of pure translation, reviewable in one sitting, with a `# Safety` comment per thunk stating the C contract it relies on.
- Tests/benches/diffharness **keep driving the C API forever** — that's now a feature, not transitional debt: the conformance suite continuously exercises the exact surface external users get, and the measuring instrument never moves at all (the Phase 8 test-migration risk from rev 1 of this plan disappears).

### 2.3 Definition of done

- Crate root carries `#![deny(unsafe_code)]`; the **only** `#[allow(unsafe_code)]` in the tree is on `src/api/` (the FFI boundary, §2.2.8). Every other module — decoder, encoder, common, processing, the safe core API — is unsafe-free and raw-pointer-free. `libc` removed from `[dependencies]`.
- `*mut`/`*const` appear in exactly two places: the `#[repr(C)]` structs and vtable/function-pointer types that *are* the public C ABI, and the boundary thunks that translate them. No raw pointer ever flows past the boundary into codec code.
- The crate builds as `["rlib", "cdylib", "staticlib"]`; the cdylib exports exactly the 7 upstream symbols (plus nothing that collides), and the external-ABI harness (§7.2 gate 7) passes: the Rust dylib, loaded via `dlopen` by a C++ driver compiled against upstream's `codec_api.h`, runs the conformance flows with hashes identical to the in-process runs.
- Benches keep their `dlopen` unsafe (dev-only, inherently FFI); the fuzz crate and diffharness drivers are safe (the diffharness's Rust side may use the C API like any external consumer would).
- All conformance/e2e/loopback/option tests pass with unchanged hashes, unchanged frame counts, unchanged `#[ignore]` set; diffharness sweep unchanged in both build profiles; benches within the agreed budget (§7.4).

---

## 3. The hard problems, called out (with their resolutions)

Most of the 85k lines convert mechanically once §2.2 exists. These are the places that don't, each with the decided approach — so no future session has to re-litigate them:

- **P1 — `decode_slice.rs` (5,261 lines) touches everything at once**: plane cursors, dispatch, MbGrid, DPB borrow splits, B-slice dual `sMCRefMember` cursor structs, 19 transmutes. Resolution: it is converted *last* within Phase 5, after every vocabulary type is proven elsewhere; the B-slice MC path uses `PicPool::cur_and_refs` to hold current-`&mut` + two ref-`&` simultaneously; `sMCRefMember` becomes `{ pic: PicId, plane_origin: (isize, isize) }` resolved at the kernel call.
- **P2 — the `sMb`↔`SDqLayer` double path** (`decoder_core.rs:3843-3869`) is UB *today* and blocks everything. Resolution: `MbGrid` (§2.2.4) becomes the single owner in Phase 5.2, `SMbCache`'s 25 arrays deleted; do it in one PR per array-family with shims.
- **P3 — decoder pointer-identity semantics** (3 sites). Resolution: `PicId` equality; add a targeted regression test per site (boundary-strength on a stream with duplicate-POC refs; EC self-copy stream) before converting.
- **P4 — self-referential ctx fields** (`pSps/pPps` into own arrays; dequant caches). Resolution: active-paramset *ids* + lookup at use; dequant tables addressed `[pps_idx][qp]` directly — the cached mid-pointers were pure C convenience.
- **P5 — `ExpandBsBuffer` rebase + NAL payload pointers surviving realloc.** Resolution: ranges/offsets (§2.2.2); the rebase function is deleted; add a unit test that grows the buffer mid-AU and checks parse continuity (there's a latent-bug class here that offsets fix *silently* — make it visible).
- **P6 — the 1-byte overread slop** in `dec_golomb.rs:128-150`. Resolution: guard bytes + identical predicate (§2.2.2); verify error-code parity on truncated-stream tests.
- **P7 — word-punning**. Resolution: `from_ne_bytes/to_ne_bytes` everywhere (T7); these are byte-moves, endian-neutral in effect; codegen verified by the perf gate. The two `write_unaligned`-over-`i8[]` sites in `svc_mode_decision.rs:817,846` are rewritten as two byte stores with a comment, since they're provenance-violating today.
- **P8 — `wels_preprocess.rs` ownership cycle**: it owns the spatial `SPicture` pool *and* holds `m_pEncCtx: *mut sWelsEncCtx` while the ctx holds `pVpp` — the one genuine cycle in the crate. Resolution: invert control — the pic pool moves up into `Encoder`; preprocess methods take `(&mut PrepState, &mut PicPool, &EncConfig)` parameters instead of reaching back through a stored ctx pointer. No `Rc/Weak` anywhere.
- **P9 — VAA out-pointer bundle** (`SVAACalcResult` holding 8 raw out-pointers into encoder arrays). Resolution: `Process` takes an `&mut VaaOutputs { sad8x8: &mut [[i32;4]], … }` view struct built fresh per call from encoder-owned `Vec`s; the stored-pointer struct dies.
- **P10 — encoder slice-list realloc invalidating outstanding `SSlice` pointers** (`ReallocateSliceList`, `svc_encode_slice.rs:2520+`). Resolution: `Vec<SliceState>` + indices everywhere; "current slice" is `usize`. The survey confirmed `SSlice` has no parent back-pointers and every entry point already receives ctx+slice as parameters — signatures become `(ctx parts, slice_idx)`.
- **P11 — `pMvdCost` mid-table pointer with negative indexing.** Resolution: `(table: &[u16], bias: usize)` pair; accessor `mvd_cost(d: i32) = table[(bias as i32 + d) as usize]`.
- **P12 — `struct_bytes_eq` memcmp equality on SPS/PPS** (padding-sensitive). Resolution: derived `PartialEq` on value structs; padding is deterministically zero today (structs are zero-initialized), so field-wise equality is behavior-preserving; keep one test comparing overwrite-detection behavior on the SPS-switch conformance streams.
- **P13 — panic policy.** Decoder: bitstream-derived values must reach error codes, never panics — the C++ error paths already exist and are ported; slice-index panics are reserved for port bugs (they'd be silent corruption in C). Fuzzing (§7.3) enforces "no panics on arbitrary input" continuously. Encoder: API-boundary validation as today; internal panics = bugs.
- **P14 — losing C++ diffability mid-refactor.** Resolution: function names/granularity preserved until the final optional rename; every converted fn keeps its `/// C++: codec/decoder/core/src/foo.cpp WelsFoo` header; module docs keep the source-file mapping; byte-gates at every merge mean h264dec triangulation stays available throughout.

---

## 4. Mechanical recipes

The repeatable transformations, so sessions can be scripted/parallelized. Each recipe = one PR shape.

- **R1 — `*mut Ctx` → `&mut` receiver sweep.** For files that only do `(*pCtx).field` (e.g. `rc.rs`: 751 derefs, 1 pointer-arith op; `au_set.rs` same profile): change ~33 signatures to `&mut`, drop `unsafe`, let rustc list every call site. Purely textual; ideal early-win recipe once its callers hold references.
- **R2 — kernel designature.** `(pPred: *mut u8, kiStride: i32)` → `(c: &mut PlaneCursorMut)` / fixed blocks `(dct: &mut [i16; 16], ff: &[i16; 8])`. Applies to ~120 kernels across `*_mb_aux`, `get_intra_predictor` ×2, `mc.rs`, `sad_common`, `intra_pred_common`, `deblocking_common`, `expand_pic`, `vaacalc`, `sample.rs`.
- **R3 — pointer-field → index-field.** `pCurDqLayer: *mut SDqLayer` → `cur_dq: usize`; `pRefList0: [*mut SPicture; 16]` → `[Option<PicId>; 16]`; `pSps: *mut SSps` → `active_sps: u8`.
- **R4 — free-cascade → `Drop`.** Delete `WelsUninitEncoderExt`-style cascades as their targets become owned; the `CMemoryAlign` leak counter is replaced by Rust ownership (keep its byte-usage stat only if the MEMORY statistics API needs it).
- **R5 — vtable → trait/enum** (§2.2.5 tiers).
- **R6 — `mem::zeroed` → `#[derive(Default)]`** as pointer fields disappear per struct.
- **R7 — strangler shims.** When converting a callee whose callers are still raw: keep the old `unsafe fn` signature as a 3-line adapter (`from_raw_parts` + call safe core), marked `// SHIM(phaseN)`, deleted when the last caller converts. This keeps every PR small, compiling, and gate-green. CI greps for `SHIM(` to ensure the count trends to zero by each phase's exit.

---

## 5. Phase plan

Estimates are in focused sessions (the unit this port has been built in). Total: **≈45–60 sessions**. Every phase ends with the full gate battery (§7.5) green.

### Phase 0 — Guardrails, dead-code purge, tooling *(3 sessions)*

1. ~~**Close out the release-segfault question**~~ **DONE (T3, 2026-08-07)** — reproduced at `eb463dbd`, root-caused to a 16-byte `uiBS` written 32 bytes deep in `DeblockingMbAvcbase`/`DeblockingBSCalc_c`, and confirmed already fixed at HEAD by `e6fce464`. Details in §1.4 and [`phase0_findings.md`](phase0_findings.md) §F1. No fix commit was needed in Phase 0.
2. Recover the deleted handoff/status docs into `rust/docs/` (`git show eb463dbd^:rust/docs/...`) — they hold measured facts (knobs the C++ itself rejects, non-byte-verifiable comparisons, the zsh word-splitting trap in sweeps) the gate battery depends on. Mark them "recovered, claims unverified". Resolve the uncommitted `sweep.sh` modification sitting in the working tree (inspect, then commit or drop with a reason).
3. Record baselines: conformance hashes + frame counts, diffharness sweep outputs **in both build profiles** across `st`, `mt`, and the new `def` preset, bench numbers (3-run medians), unsafe-ratchet counts (§7.1) checked in as `rust/tools/unsafe_baseline.json`.
4. Delete dead unsafe now (no behavior change): the 62 unreferenced SIMD externs (`mc.rs:793-863`); SIMD delegating stubs everywhere they exist (`encode_mb_aux.rs:772-930`, `sample.rs`, `intra_pred_common.rs` `_mmi/_lsx/_sse2/_neon` aliases, decoder equivalents) with tables re-pointed at `_c`; decoder threading scaffolding (`SWelsDecThreadInfo`, `SWelsDecoderThreadCTX`, `EventCreate`, dead `pReadyEvent`/thread-gated `pNzc` alloc paths, `GetThreadCount` → literal 1); duplicated bitstream writer (`svc_encode_slice.rs:498-585`); `use std::ffi::c_void` in lib.rs.
5. Stand up the fuzz crate (`rust/fuzz/`, cargo-fuzz, decode target seeded from `res/`), run it for a baseline corpus; it runs continuously from here on.
6. Add the ratchet script + a `just`/shell gate runner so every session can run the whole battery in one command.

*Exit gate: release and debug builds both pass the battery with identical bytes; ratchet baseline committed. Risk: the segfault fix itself — contained by the diffharness before/after comparison in the debug profile, where behavior is defined.*

### Phase 1 — Safe vocabulary types *(3 sessions)*

Build `src/safe/`: `PaddedPlane`/`PlaneCursorMut`, `BsCursor`/`BsWriter`, `PicId`/`PicPool` skeleton, `MbGrid` skeleton, `DecodeError` plumbing helpers. Unit tests including property tests against the existing unsafe implementations (same random inputs → same outputs), run under **Miri**. Nothing is wired into the codec yet.

*Exit gate: unit tests + Miri clean; codec untouched. Risk: API-shape mistakes — mitigated by piloting on one consumer each in Phase 2/3 before mass adoption.*

### Phase 2 — Leaf DSP kernels, both codecs *(5–7 sessions)*

R2 across: `decoder/decode_mb_aux.rs` → `encoder/encode_mb_aux.rs` + `sample.rs` + `encoder/decode_mb_aux.rs` → `common/sad_common.rs` + `intra_pred_common.rs` + `mc.rs` (fold the `[[fn;4];4]` quarter-pel table into a `match` or safe-fn table) → `common/deblocking_common.rs` (mind the stride-swap V/H trick and `-3*stride` reads — cursor handles both) → `common/expand_pic.rs` + `decoder_core.rs` expand functions → `processing/vaacalc.rs` + `adaptive_quantization.rs` kernels → both `get_intra_predictor.rs` (the biggest: 44 decoder kernels + encoder set; scriptable — uniform signature, uniform punning pattern; keep the decoder/encoder same-name-different-signature functions distinct).

Callers keep calling through R7 shims where tables still exist; tables themselves get retyped to safe fn pointers as each family completes. Delete `#[unsafe(no_mangle)]`/`extern "C"` from all kernels (nothing links them; benches compare via dlopen, not symbol interposition).

*Exit gate: full battery; bench delta budget applies for the first time. Risk: perf regressions from bounds checks in 4×4 loops — see §7.4 idioms; convert `get_intra_predictor` early in the phase to get the worst case measured.*

### Phase 3 — Bitstream layer *(4–5 sessions)*

1. Decoder read side: `bit_stream.rs`, `dec_golomb.rs` (guard-byte design P6), `cabac_decoder.rs` (offset engine + handoff), `SDataBuffer` → `RawDataBuffer`, `nalu.rs` payload ranges, delete `ExpandBsBuffer` rebasing.
2. Encoder write side: `vlc_encoder.rs` → safe `BsWriter`; `set_mb_syn_cabac.rs` cursor triple; `svc_set_mb_syn_cavlc.rs` rollback snapshots + `pEndBuf - pCurBuf` space checks → `len - pos`; `nal_encap.rs` owned buffers.
3. `SBitStringAux` itself remains as a compat shell only where still-unconverted structs embed it (`SDqLayer::pBitStringAux` waits for Phase 5); the shell is deleted in Phase 5/6.

*Exit gate: battery + a dedicated malformed-stream error-code parity test + fuzz burn-in on the new reader. Risk: the P6 slop and CABAC end-ladder byte counting — both have precise existing tests (truncated streams in conformance set).*

### Phase 4 — Dispatch de-virtualization *(3 sessions)*

Tier 1 deletions (tables → direct calls; the Phase 0/2 work makes this mostly deletion), tier 2 enums (`EntropyCoder`, MD/ME strategy enums, RC mode — note `SWelsRcFunc` maps to an enum over the five RC modes already proven byte-identical), tier 3 traits (`ParamsetStrategy`, `RefStrategy`), `IWelsVP` inlined per §2.2.5, all 21+2 transmutes gone, `wels_func_ptr_def.rs` reduced to the ~15 algorithm choices and then dissolved into config-typed fields on `Encoder`.

*Exit gate: battery. Risk: low — every replaced pointer provably pointed at exactly one function per config; assert-map the table-init functions before deleting them.*

### Phase 5 — Decoder structural rewrite *(9–12 sessions)* — the first pivot

Order inside the phase (each step lands with shims + full gates):

1. **5.1 Picture & DPB**: `Picture` re-struct (PaddedPlanes + Vecs), `PicPool`, `pic_queue.rs` recycling predicate, `manage_dec_ref.rs` (near-mechanical per survey), `error_concealment.rs` identity sites, P3 regression tests first.
2. **5.2 MbGrid**: kill `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned `MbGrid`; re-point `parse_mb_syn_*` cache fills (their 30-entry scratch caches become `&mut` locals passed down — ~40 signatures, mechanical).
3. **5.3 Neighbor & MV machinery**: `mv_pred.rs` (punning → byte ops, `SetRectBlock` → typed generic on the grid), colocated reads via `cur_and_ref`.
4. **5.4 Deblocking driver** (`decoder/deblocking.rs`): `SDeblockingFilter` holds `PicId`s + plane cursors built per-MB; identity compare per P3.
5. **5.5 `decoder_core.rs`**: allocation → constructors (`AllocPicture` dies into `Picture::new`), paramset store (P4), context decomposition per §2.2.6, `Drop` teardown, `Default` derives.
6. **5.6 `decode_slice.rs`** last (P1), including EC MC paths; delete the last decoder shims and `SBitStringAux` shell; decoder modules get `#![deny(unsafe_code)]` one by one.

*Exit gate: battery with special attention to frame-count parity and the `#[ignore]` set; long fuzz run; decoder src/ is unsafe-free. Risk: highest of the plan — this is where borrow-splits are designed for real. Mitigation: strict step order above, each step's shims keep the tree green, and any step can pause for a session boundary without leaving broken state.*

### Phase 6 — Encoder structural rewrite *(10–14 sessions)* — the second pivot

Same playbook, informed by Phase 5's patterns:

1. **6.1** `ref_list_mgr_svc.rs` → `PicId` lists (survey: zero identity comparisons, pure index conversion) + `RefStrategy` trait from Phase 4.
2. **6.2** Picture pool ownership out of `wels_preprocess.rs` (P8 inversion); `SVAACalcResult` → out-views (P9); `processing/` fully safe here.
3. **6.3** `SMbCache` → owned fixed arrays; plane cursors in MD/ME (`md.rs`, `svc_base_layer_md.rs`, `svc_mode_decision.rs`, `svc_motion_estimate.rs` incl. P11, `svc_encode_mb.rs`) — the hot path; watch the bench budget per-PR.
4. **6.4** Slice/layer structures: `Vec<SliceState>` + indices (P10), `svc_enc_slice_segment.rs` maps → `Vec<u16>`, `encoder/deblocking.rs` finishing its half-done slice conversion.
5. **6.5** R1 sweeps: `rc.rs`, `au_set.rs`, `paraset_strategy` remnants, `wels_encoder_ext.rs` internals.
6. **6.6** The pivot: `encoder_context.rs` + `encoder_ext.rs` — `RequestMemorySvc` → constructors, free cascade → `Drop`, field taxonomy applied (§2.2.6), `encoder/abi_guard.rs` asserts deleted struct-by-struct in the same commits that de-`repr(C)` them.

*Exit gate: battery + diffharness full sweep (slice modes × RC modes × threads) byte-identical. Risk: 6.3 perf; 6.6 is a big-bang file but by then it's the last raw consumer standing.*

### Phase 7 — Threading rework *(3–4 sessions)*

§2.2.7: scoped fork/join for fixed slicing; channel work-queue for SM_SIZELIMITED; safe persistent pool only if spawn cost shows up in benches; delete `TaskPtr`, laundering, `void*` mutexes, `static mut` singleton, all 5 `unsafe impl` pairs; `wels_task_management.rs`/`slice_multi_threading.rs`/`wels_thread_pool.rs` collapse to a fraction of their size.

*Exit gate: MT determinism (MT bitstream == today's MT bitstream, across thread counts), loom-style stress not required — the model is fork/join with owned splits. Risk: SM_SIZELIMITED work distribution must not change slice boundaries; it's index-driven, keep the claiming order identical.*

### Phase 8 — C-ABI boundary hardening *(3–4 sessions)*

§2.2.8. The externally visible API does not change at all; this phase moves the safety line up to it and makes the drop-in story real:

1. Carve the safe core `Decoder`/`Encoder` types out of what phases 5–7 produced (mostly naming/visibility — the subsystems already exist); assert `Send` via compile tests; export the safe API for Rust consumers.
2. Rewire the boundary: `CWelsDecoderImpl`/`CWelsH264SVCEncoderImpl` hold the safe types; each of the 24 thunks becomes translate-in → safe call → translate-out with a written `# Safety` contract; the vtable structs, slot order (including `DecodeParser`/`DecodeFrameEx` stubs), factories, and version functions are untouched. Trace-callback plumbing lands here (raw C pair stored at the boundary, wrapped as the internal `Logger` sink).
3. `crate-type = ["rlib", "cdylib", "staticlib"]`; extend `api/abi_guard.rs` to pin every boundary-crossing struct (add `SParserBsInfo`, `OpenH264Version`, capability struct if missing).
4. Build the **external-ABI harness**: a small C++ driver compiled against upstream `codec_api.h` that `dlopen`s the Rust cdylib and runs the decoder-conformance and loopback flows; its hashes must equal the in-process results. Stretch, high-value: point upstream's gtest API suites at the Rust dylib — the repo already contains the gtest tree and the `gtest_repro` flow.
5. Scoped-lint endgame for the module: `api/` gets `#[allow(unsafe_code)]`; crate root gets `#![deny(unsafe_code)]` (this flips in Phase 9 once stragglers are gone).

Tests, benches, and diffharness are deliberately **not** ported off the C API — they keep exercising the drop-in surface permanently (§2.2.8). The C++-side flow caveat stands: `DecodeFrame2`-vs-`DecodeFrameNoDelay` ordering differences are a known divergence class, and the boundary must preserve exact call-sequence semantics.

*Exit gate: battery + the new external-ABI harness green; exported-symbol list of the cdylib == upstream's 7. Risk: pointer-filling of output structs from borrowed views must reproduce the exact validity windows C callers assume — mitigated by the harness driving the same sequences C consumers use.*

### Phase 9 — Hygiene endgame *(2–3 sessions)*

`#![deny(unsafe_code)]` at crate root with the single scoped `#[allow(unsafe_code)]` on `src/api/` (§2.2.8); remove every other `#![allow(...)]` blanket except naming allowances (kept until D5 is decided); `libc` dropped; clippy (`pedantic` triage) + final Miri/fuzz marathons; docs: module-level C++ provenance headers verified present; ratchet file replaced by the lint (its only remaining job is watching `api/`'s size); optional D5 rename executed or explicitly declined; revisit D4 — the workspace split that turns `deny`+`allow` into a true `#![forbid(unsafe_code)]` core crate.

---

## 6. File → phase map

| Phase | Files (fate) |
|---|---|
| 0 | `mc.rs` (dead externs), `encode_mb_aux.rs`/`sample.rs`/`intra_pred_common.rs`/decoder kernels (dead SIMD stubs), `codec_api.rs` (2 stubs), decoder thread scaffolding in `decoder_context.rs`/`picture.rs`/`pic_queue.rs`/`decoder_core.rs`, `svc_encode_slice.rs` (writer dupe), `lib.rs` |
| 1 | new `safe/` module (+ unit tests) |
| 2 | `decoder/decode_mb_aux.rs`, `encoder/{encode_mb_aux,decode_mb_aux,sample}.rs`, `common/{sad_common,intra_pred_common,mc,deblocking_common,expand_pic}.rs`, `decoder/get_intra_predictor.rs`, `encoder/get_intra_predictor.rs`, `processing/{vaacalc,adaptive_quantization}.rs` (kernels) |
| 3 | `decoder/{bit_stream,dec_golomb,cabac_decoder,nalu}.rs`, `encoder/{vlc_encoder,set_mb_syn_cabac,svc_set_mb_syn_cavlc,nal_encap}.rs`, `common/wels_common_defs.rs` (SBitStringAux shell) |
| 4 | `encoder/wels_func_ptr_def.rs` (dissolved), `encoder/paraset_strategy.rs`, `ref_list_mgr_svc.rs` (vtable half), `processing/mod.rs` + `wels_preprocess.rs` (IWelsVP half), `decoder/decoder_core.rs:1913-1995` + `decode_slice.rs` transmutes, `decoder/decode_slice.rs`/`deblocking.rs` table calls |
| 5 | `decoder/{picture,pic_queue,manage_dec_ref,error_concealment,mv_pred,parse_mb_syn_cavlc,parse_mb_syn_cabac,deblocking,decoder_core,decode_slice,decoder_context,fmo,slice,parameter_sets}.rs` (last four mostly type/receiver touch-ups) |
| 6 | `encoder/{ref_list_mgr_svc,picture,wels_preprocess,md,svc_base_layer_md,svc_mode_decision,svc_motion_estimate,svc_encode_mb,svc_encode_slice,svc_enc_slice_segment,deblocking,rc,au_set,param_svc,wels_encoder_ext,encoder_context,encoder_ext,abi_guard}.rs`, `processing/{background_detection,complexity_analysis,scene_change_detection}.rs`, `common/memory_align.rs` (deleted) |
| 7 | `common/wels_thread_pool.rs`, `encoder/{wels_task_management,slice_multi_threading}.rs` |
| 8 | `api/codec_api.rs` (internals rewired; ABI surface frozen), `api/abi_guard.rs` (extended), `Cargo.toml` (cdylib), new external-ABI harness under `tools/`; `tests/*`/`benches/*`/`diffharness/rust_enc` deliberately untouched — they stay on the C API |
| 9 | `lib.rs`, crate-wide lint/doc passes |

Untouched throughout: `decoder/vlc_tables.rs`, `common/cpu_core.rs` (trivial), `tests/common/`.

---

## 7. Verification & tooling

### 7.1 The unsafe ratchet
`rust/tools/unsafe_ratchet.sh`: per-file counts of `unsafe fn`, `unsafe {`, `*mut|*const`, `transmute`, `unsafe impl`, `mem::zeroed`, `no_mangle`, `SHIM(` vs a checked-in baseline JSON. CI (or the session-start checklist) fails on any increase; each phase commits a decreased baseline. This — not `unsafe_op_in_unsafe_fn` churn — is the progress meter. (Flipping that lint crate-wide would add thousands of `unsafe {}` wrappers only to delete them again; don't. It gets enabled per-module as modules go safe, where it's vacuous.)

### 7.2 The gate battery (every merge)
0. Both build profiles compile and agree (debug **and** `--release`; the pre-Phase-0 release segfault means any debug/release divergence is treated as UB evidence, not flakiness).
1. `cargo test` and `cargo test --release` — all green, `#[ignore]` set unchanged.
2. Conformance SHA-1s **and frame counts** unchanged (a decode error that drops frames must not hide behind a "different hash" — check counts first).
3. Loopback + option-sweep + LTR tests unchanged.
4. Diffharness: at minimum the gate configuration; full sweep (`st`, `mt`, `def` — slice modes × RC × threads × the defaults path) at phase exits.
5. Ratchet decreased or equal; `SHIM(` count per plan.
6. Fuzz: corpus regression run, no new panics/timeouts.
7. From Phase 8 on: external-ABI harness (C++ driver + Rust cdylib via dlopen) hashes match in-process results; cdylib exported-symbol list unchanged.

### 7.3 Fuzzing (from Phase 0, forever)
`fuzz_targets/decode_annexb.rs`: feed arbitrary bytes through the full decode+flush sequence; assert no panic, no abort, bounded memory. Seed with `res/`. After Phase 3, add a structured mutator target for NAL-level mutation. Stretch (worth it by Phase 5): differential target comparing Rust vs dlopen'd C++ on error codes + output hashes — it converts the byte-exactness property into a fuzzable invariant.

### 7.4 Performance budget
- Baseline: Phase 0 bench medians (`c_vs_rust_bench`, `decode_1080p_bench`). All comparisons are scalar-vs-scalar (the C++ dylib never dispatches SIMD), so ratios are meaningful.
- Budget: ≤5% regression per phase, ≤10% cumulative at Phase 9, tracked in a checked-in log. Anything over 5% in a single PR gets investigated before merge.
- Safe-but-fast idioms, in order of preference: fixed-size array windows (`&mut plane[i..i+16].try_into().unwrap()` → `&mut [u8;16]` — eliminates per-access checks), `chunks_exact`/`chunks_exact_mut` row walking, hoisting `row_mut` slices out of inner loops, iterator zips for src/dst pairs. `get_unchecked` is **banned** — if a hot loop can't be made fast safely, restructure the loop, don't reintroduce unsafe.
- Expect some *wins*: direct calls replacing indirect dispatch (Phase 4) and `match` replacing fn-pointer tables are LLVM-inlinable where the C design never was.

### 7.5 Session workflow
Each session: run battery → pick next plan item → convert with shims → battery → commit with ratchet update → update this doc's checkbox (add a `## Progress` appendix on first execution session). The phase structure is deliberately interruptible; no step leaves the tree broken across a session boundary.

---

## 8. Sequencing rationale & parallelism

```
P0 ──► P1 ──► P2 (kernels) ──► P4 (dispatch) ──► P5 (decoder pivot) ──► P8 (API) ──► P9
         └──► P3 (bitstream) ──┘                └► P6 (encoder pivot) ─► P7 (threads) ┘
```

- **Common-first** because both codecs consume the same vocabulary types and kernels; **decoder pivot before encoder pivot** because the decoder has the strongest external ground truth (JVT streams + h264dec triangulation), no threading, and is 30% smaller — it's where the arena/grid/borrow-split patterns get proven before the bigger encoder applies them.
- **Threading after encoder ownership** (the split-borrow shape is unknowable before `sWelsEncCtx` is decomposed) and **the API boundary last** — the boundary can only shrink to a thin translation shim once the safe core it translates *to* exists, and until then the C API (and the tests driving it) stays byte-for-byte still.
- Parallelizable across concurrent sessions/agents: within Phase 2 (kernel families are independent), Phase 3's read vs write sides, Phase 5 vs early Phase 6 steps (6.1/6.2 don't depend on decoder work — only on Phases 1–4), and any R1 sweep. The pivots (5.5/5.6, 6.6) and Phase 7 are single-track.

---

## 9. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| More latent UB like the release-deblocking segfault surfaces mid-refactor (an optimizer or layout change flips behavior that was never defined) | **High** (raised: the known instance turned out to be a stack overflow executing thousands of times per frame on the mainline path, silently, for the port's whole life) | High | Phase 0 root-caused the known instance (§1.4, already fixed by `e6fce464`) and added the dual-profile gate; any later debug/release disagreement halts the phase until root-caused — it is always a pre-existing bug being exposed, and fixing it is in scope. Note the instance was *invisible to every gate*: 341/341 byte-identical sweeps ran on top of it. Byte-exactness does not imply soundness, which is the argument for the fuzzer (§7.3) and for Phase 2 replacing `*mut u8` kernel arguments with real array types |
| Silent behavior change on malformed streams (EC paths, slop P6) | Medium | High (conformance can't see it) | Error-code parity tests, fuzz differential vs C++, malformed-stream corpus from the ignored e2e cases |
| Perf regression in MD/ME/intra hot loops | High (locally) | Medium | §7.4 idioms, per-PR budget, convert worst case early (Phase 2 `get_intra_predictor`) |
| `decode_slice.rs`/`encoder_ext.rs` pivots stall mid-way | Medium | High | Strict internal step order, shims keep tree green, each step is a session-sized unit |
| Borrow-splits force design churn late (a subsystem needs 4 parts at once) | Medium | Medium | §2.2.6 decomposition reviewed against the top-20 fattest call paths *before* Phase 5 starts (one session of dry-run signature sketching) |
| MT output nondeterminism after Phase 7 | Low | High | Slice-order stitching preserved, determinism gate across thread counts, diffharness threads sweep |
| Losing the C++ debugging column | Low | High | P14 conventions, gates at every merge keep divergence windows one-PR wide |
| Boundary translation subtly changes C-visible semantics (output-pointer validity windows, option `void*` decoding, callback reentrancy) | Medium | High | Tests never leave the C API; external-ABI harness drives the dylib exactly as a C consumer; thunk-by-thunk `# Safety` contracts reviewed against upstream `codec_api.h` docs |
| ABI layout drift on platforms we don't build (Windows `EXTAPI`, 32-bit) | Low | Medium | Inherited from upstream's no-virtual-dtor + `__cdecl` design; `api/abi_guard.rs` pins struct layouts; add per-platform CI only when a platform is actually claimed |

---

## 10. Open decisions (need Eugene's call, none block Phases 0–4)

- **D1 — Handle hygiene: RESOLVED (2026-08-07, by implementing the recommendation).** Plain indices with release semantics identical to C recycling, plus a `#[cfg(debug_assertions)]` generation counter per slot, bumped by `Pool::replace` and checked in every accessor. Handle equality ignores the generation in both profiles, so identity semantics (P3) cannot differ between debug and release. Built and tested in Phase 1 (`src/safe/pool.rs`); no full generational arena.
- **D2 — C ABI future: RESOLVED (2026-08-07): drop-in replacement is a requirement.** The C-ABI surface (vtables, 7 exports, `repr(C)` structs, slot order) is frozen as public contract; `src/api/` is the sole `unsafe` module; crate ships as cdylib/staticlib; §2.2.8 and Phase 8 encode the consequences. Follow-on packaging (soname, version script, pkg-config) is tracked outside this plan.
- **D3 — Decoder MT:** the C++ has a threaded decoder the port stubbed out. This plan deletes the stubs. If decoder MT is wanted later, it's a fresh design on the safe architecture (frame-level pipelining with `PicPool` + per-frame `MbGrid` is well-suited) — flagging so the deletion isn't a surprise.
- **D4 — Workspace split:** single crate (status quo, recommended through Phase 9) vs splitting afterwards into `openh264-core` (pure safe codec, `#![forbid(unsafe_code)]` — the strongest possible statement) + `openh264-capi` (the cdylib boundary, the only unsafe). With D2 resolved, the split earns more: it turns "deny with one allow" into a hard `forbid`, and gives Rust consumers a dependency with zero unsafe anywhere in its tree. Still deferred to post-Phase-9 — module boundaries make the split mechanical then.
- **D5 — The great rename:** after Phase 9, keep Wels/Hungarian names (maximum C++ diffability, zero churn) vs rename to idiomatic snake_case with `/// C++:` mapping headers (better for external contributors, one-time loss of easy diffing). Can be decided per-module; recommendation: decide only once the C++ reference has stopped earning its keep.
- **D6 — Error style:** keep i32 error-code returns internally (maximum C++ parity, recommended through Phase 8) vs migrate internals to `Result<_, DecodeError>` during Phase 9 (the public API gets `Result` at Phase 8 regardless, translating at the boundary).

---

*Sources: three subsystem surveys (decoder, encoder, common/api/processing) conducted 2026-08-07 against `eb463dbd`, with file:line references verified at survey time; per-file difficulty tiers and counts come from those surveys. The taxonomy table (§1.2) is the index back into the details.*

---

## Progress

Per-phase checklist, updated at the end of every session. The running narrative —
gate numbers, what was tried, what to do next — is in
[`safety_refactor_log.md`](safety_refactor_log.md); findings that are recorded rather
than fixed are in [`phase0_findings.md`](phase0_findings.md).

### Phase 0 — guardrails, dead-code purge, tooling

- [x] **T1 — session-start control run.** Green at `353791f7`: `cargo test` and
      `cargo test --release` 294 passed / 0 failed / 20 ignored; `sweep.sh st mt def`
      341/341 byte-identical; both benches all-rows bit-identical.
- [x] **T2 — resolve the dangling working tree.** The `sweep.sh` modification had
      already been committed as `353791f7` before this session started. The plan
      update found in its place was committed as inherited (`87127430`), and the
      session brief recorded in-tree (`156b9abb`).
- [x] **T3 — the `eb463dbd` release segfault.** Reproduced, root-caused, closed. It was
      a stack buffer overflow (16-byte `uiBS`, 32-byte writes), not a pointer-wiring
      gap; already fixed at HEAD by `e6fce464`, so no fix commit was needed. Recorded
      in §1.4, `phase0_findings.md` §F1, and auto-memory. `86b40ed1`
- [x] **T4 — recover docs, record baselines.** `ac5b91d2` (handoff, status log and four
      audit scripts, each marked recovered-and-unverified), `89c05aa2`
      (`perf_baseline.md`, 3-run medians, machine info, and the measured proof that the
      encoder bench measures frame-skipping rather than encoding without `FFMPEG` set).
- [x] **T5a — 62 unreferenced SIMD externs** in `mc.rs`. `89aa6109`
- [x] **T5b — SIMD delegating stubs.** 113 definitions across four files, plus the
      CPU-flag branches that installed them, machine-verified to delegate to exactly the
      `_c` already in each slot. `a2a19d3c`, `fbe45f11`
- [ ] **T5c — decoder threading scaffolding.** Not started. Step 1 of the recipe is
      already proven and written up in the log: `pThreadCtx`/`pLastThreadCtx`/
      `pCsDecoder` are never assigned. **`GetThreadCount` must become a literal `0`, not
      the `1` the brief says** — `api/codec_api.rs:1831` branches on `<= 0`.
- [x] **T5d — duplicated bitstream writer.** No deletion: there are four copies, not
      two, and they are not identical. Recorded as `phase0_findings.md` §F2 per the stop
      rule; Phase 3.2 owns the dedupe. `dc8487cb`
- [ ] **T5e — small stragglers.** Not started; depends on T5c's dead-code harvest.
- [ ] **T6 — ratchet script + gate runner.** Not started. Note for whoever writes it:
      the naive `unsafe fn` pattern misses `unsafe extern "C" fn` and reported no change
      across 113 deleted stubs — count `unsafe (extern "C" )?fn`.
- [ ] **T7 — fuzz crate.** Not started, and **deferred by direction** during the Phase 1
      session (2026-08-07): "skip fuzzing, do phase 1". `cargo-fuzz` is still not
      installed; nightly now is (Phase 1 needed it for Miri). Nothing else depends on
      it, but §7.2 gate 6 and §7.3 stay unavailable until it lands.
- [x] **T8 — bookkeeping.** This appendix, `safety_refactor_log.md`, and the auto-memory
      updates. Re-run at the end of each Phase 0 session.

Tooling that landed alongside: `da5c06ae` made the diffharness able to build and run a
**release** driver at all (`RUST_ENC_PROFILE`), which is what makes §7.2 gate 0
executable; `f1c90948` fixed a bash 3.2 regression in it.

### Phase 1 — safe vocabulary types

**Started ahead of Phase 0's exit gate, by direction** (2026-08-07). Phase 0's T5c,
T5e and T6 are still open and T7 is deferred; the session was told to skip fuzzing and
proceed to Phase 1. That is sound for *this* phase specifically — Phase 1 changes no
codec code, adds no `unsafe`, and its own gate is the full battery, which was run as a
control first and again at the end — but the missing ratchet script (T6) means Phase 1
could not run `unsafe_ratchet.sh check`. Substituted: the raw counts below, and the
structural fact that every file in `src/safe/` is `#![forbid(unsafe_code)]`.

- [x] **T1 — control battery.** `cargo test` and `cargo test --release`
      294 / 0 / 20 each; `sweep.sh st mt def` 341/341 debug, 340/341 release with the
      single failure at `t=4 sm=3 n=600` — F3's exact signature. The exit run behaved
      the same way, so the rate was measured against the control commit directly (8 mt
      sweeps each, 960 configurations: 1 failure at `53e211f7`, 2 at HEAD — noise, same
      signature). F3's write-up updated with both refinements.
- [x] **T2 — module skeleton, error plumbing, test PRNG.** `448c8118`. Note for the
      record: the plan said the error codes live in `decoder_context.rs`; they do not —
      that file has only `ERR_NONE`, and the reader codes are declared *twice*, in
      `decoder/bit_stream.rs` and `decoder/dec_golomb.rs`. `err.rs` reuses them and
      carries a test pinning the two copies together.
- [x] **T3 — `PaddedPlane` + plane cursors.** `952c8b1d`. §2.2.1 updated to the built
      API.
- [x] **T4 + T5 — `BsCursor` and `BsWriter`.** `5b47b1fe`. §2.2.2 updated; two
      deviations and two findings (F4, F5) recorded.
- [x] **T6 — `Pool`/handles and the `MbGrid` skeleton.** `e3a09459`, `066471ea`.
      §2.2.3 updated (`mut_and_rest` replaces `cur_and_refs`, generic over `T`);
      D1 resolved by implementing the recommendation.
- [x] **T7 — error plumbing.** Landed with T2 as `ErrInfo`; per D6 it stays a
      transparent newtype over the C++ `int32_t` codes.
- [x] **T8 — Miri.** `cargo +nightly miri test --lib safe::` clean, 63/63. Both
      differential files clean under Miri as well — including the *old* unsafe
      implementations they drive, which is free UB coverage of `dec_golomb`,
      `bit_stream` and `vlc_encoder`. No test needed `#[cfg_attr(miri, ignore)]`.
      Sample counts scale down under `cfg!(miri)` so the gate stays minutes, not hours.
- [x] **T9 — bookkeeping.** This appendix, `phase1_findings.md`, the log entry, and the
      §2.2/§10 updates above.

Counts, for the ratchet baseline whenever Phase 0 T6 lands: `src/safe/` is 2,517
lines over 7 files — 1,681 of implementation and documentation, the rest in-module
tests — and contains **zero** `unsafe` of any spelling (the only three occurrences of
the word are in `mod.rs`'s prose). 63 in-module unit tests. The
differential tests add 18 more in two integration files that *do* use `unsafe` (they
must — they drive the raw-pointer reference implementations). Test totals moved 294 → **375** in debug and **373** in release; the two-test
difference is deliberate — `pool`'s stale-handle tests are `#[cfg(debug_assertions)]`,
because the behaviour they check only exists in a debug build. Every *pre-existing* test
binary keeps its exact control count, and the 20-test `#[ignore]` set is unchanged.

### Phases 2–9

Not started. Phase 2's first action is the pilot conversion of
`decoder/decode_mb_aux.rs` onto the plane API (plan §Phase 2), deliberately small so
an API-shape mistake surfaces before mass adoption.
