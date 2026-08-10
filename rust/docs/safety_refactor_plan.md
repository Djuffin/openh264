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
- Kernels take `PlaneCursorMut` instead of `(pPred: *mut u8, kiStride: i32)`. **[P2] The `(buf, center, stride)` triple escape hatch is not needed and should not be used** — see the pilot verdicts below.

**Pilot verdicts (Phase 2 T2, `decoder/decode_mb_aux.rs`).** The Phase 1 hand-off asked two questions before the API was adopted by ~120 kernels. Both are answered, and the answers are binding for the rest of Phase 2:

1. **Cursor, not triple — the choice was a false one.** `PlaneCursorMut` *is* the triple: three fields, passed by `&mut`, whose `row_mut` is one add and one slice index. The only per-call cost the triple avoids is the `assert!` in `new`, which runs once per kernel invocation, not once per row. The decode bench came out **1.3–1.8% faster** with the safe kernels live ([`perf_baseline.md`](perf_baseline.md) §Phase 2 T2, 3 runs per side). Use the cursor everywhere; reach for the triple only with a bench number in hand that says otherwise.
2. **`row_mut` hoists fine — but the C++ loop order is what has to change.** The 4x4 IDCT wrote its output *column*-major (four strided stores per column), which no row-window idiom can help. Transposing it to row-major made each row one bounds check and one `&mut [u8; 4]`. That transposition is bit-exact **only because every sample of the block is read and written exactly once**, which is a per-kernel proof obligation, not a blanket licence. Check it before transposing; when it fails, keep the C++ order.
3. **`PlaneCursorMut::reborrow(dx, dy)` was added** (`952c8b1d`'s API could not express it). `advance` consumes the cursor, which is right for a walk (`pDstY.add(16)`) and wrong for a composite kernel that hands each sub-block to an inner kernel and carries on — `IdctFourResAddPred_c` calling `IdctResAddPred_c` four times. Expect the same shape in `mc.rs` and `deblocking_common.rs`.
4. **Shim contracts are short exactly when a kernel reaches forward only.** These four do, so the reachable span is `(bh-1)*stride + bw` — derivable from the signature, needing no knowledge of the plane's padding. The families that read `-1`/`-stride` (`get_intra_predictor`), `-3*stride` (deblocking) or write the padding (`expand_pic`) will need `from_raw_parts_mut(p.sub(k*stride + k), …)` and a contract that names the padding constant. Budget for that; it is where the phase's real documentation value is.
- The MC clamp (`decode_slice.rs:1072-1091`) stays byte-for-byte: the clamp guarantees the biased index is in range; slice indexing then *proves* it, converting a silent-corruption class into a loud panic if the port ever miscomputes.
- Cross-plane pairs (MC: read ref picture, write current) are two views of two different `PaddedPlane`s — no conflict. Same-picture src/dst in error concealment goes through a `copy_within`-style method on one plane.
- Border expansion is a **free function over the raw geometry for now** — Phase 2 T6 built it as `expand_picture(&mut [u8], stride, w, h, pad)` in `common/expand_pic.rs`, because in Phase 2 the allocation is still C-owned and arrives at the shim as a mid-plane pointer. The original intent stands as the Phase 5 packaging step: once `Picture` owns `PaddedPlane`s, the free function becomes (or is wrapped by) a `PaddedPlane` method, and the shim's span reconstruction (`expand_shim_span`, `decoder_core.rs`) dies with the shim.

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

Estimates are in focused sessions (the unit this port has been built in). Total: **≈50–65 sessions** (revised 2026-08-08 from ≈45–60 after Phase 2's scale correction; see the calibration row in §9 — count before sizing, and treat per-phase figures as −0/+40% ranges). Every phase ends with the full gate battery (§7.5) green.

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

### Phase 2 — Leaf DSP kernels, both codecs *(6–8 sessions; resized 2026-08-08)*

*Scale correction from execution: the "~120 kernels" figure undercounted by ~60% — the real total, counted as `unsafe fn` definitions in the phase's files, is ≈190, and per-family estimates in briefs have been wrong in both directions (T2 "~13" was 4; T3 "44" was 42; T4's "26" held). Count the file before sizing the session, and treat every kernel figure in this section as an estimate.*

R2 across: `decoder/decode_mb_aux.rs` → `encoder/encode_mb_aux.rs` + `sample.rs` + `encoder/decode_mb_aux.rs` → `common/sad_common.rs` + `intra_pred_common.rs` + `mc.rs` (fold the `[[fn;4];4]` quarter-pel table into a `match` or safe-fn table) → `common/deblocking_common.rs` (mind the stride-swap V/H trick and `-3*stride` reads — cursor handles both) → `common/expand_pic.rs` + `decoder_core.rs` expand functions → `processing/vaacalc.rs` + `adaptive_quantization.rs` kernels → both `get_intra_predictor.rs` (the biggest: 44 decoder kernels + encoder set; scriptable — uniform signature, uniform punning pattern; keep the decoder/encoder same-name-different-signature functions distinct).

Callers keep calling through R7 shims where tables still exist; tables themselves get retyped to safe fn pointers as each family completes. Delete `#[unsafe(no_mangle)]`/`extern "C"` from all kernels (nothing links them; benches compare via dlopen, not symbol interposition).

**[P2] What this phase does and does not move on the ratchet.** Measured on the pilot family and true of every family in it: `no_mangle` falls, `SHIM(` rises, `unsafe_block` **rises**, and `unsafe_fn` and `raw_ptr` do not move at all. That is not a disappointment, it is the strangler pattern's arithmetic — the shim keeps the raw signature, so the pointers and the `unsafe fn` stay until Phase 5 converts the *callers* and deletes the shim; and the explicit `unsafe {}` around each `from_raw_parts` is *counted* unsafe replacing the *uncounted* unguarded derefs that `#![allow(unsafe_op_in_unsafe_fn)]` hides inside an `unsafe fn` body (§1.1). The metric got more honest, not worse. Consequence for the workflow: **commit B of each family regenerates the baseline**, after running `check` and confirming the increases are confined to the file being converted. Phase 2's real deliverable is the safe kernels and their written contracts; the count collapse is Phases 4–5 cashing this in.

*Exit gate: full battery; bench delta budget applies for the first time. Risk: perf regressions from bounds checks in 4×4 loops — see §7.4 idioms; convert `get_intra_predictor` early in the phase to get the worst case measured.*

### Phase 3 — Bitstream layer *(4–5 sessions)*

1. Decoder read side: `bit_stream.rs`, `dec_golomb.rs` (guard-byte design P6), `cabac_decoder.rs` (offset engine + handoff), `SDataBuffer` → `RawDataBuffer`, `nalu.rs` payload ranges, delete `ExpandBsBuffer` rebasing.
2. Encoder write side: `vlc_encoder.rs` → safe `BsWriter`; `set_mb_syn_cabac.rs` cursor triple; `svc_set_mb_syn_cavlc.rs` rollback snapshots + `pEndBuf - pCurBuf` space checks → `len - pos`; `nal_encap.rs` owned buffers.
3. `SBitStringAux` itself remains as a compat shell only where still-unconverted structs embed it (`SDqLayer::pBitStringAux` waits for Phase 5); the shell is deleted in Phase 5/6.

*Exit gate: battery + a dedicated malformed-stream error-code parity test + fuzz burn-in on the new reader. Risk: the P6 slop and CABAC end-ladder byte counting — both have precise existing tests (truncated streams in conformance set).*

### Phase 4 — Dispatch de-virtualization *(3 sessions; split 4a/4b and resequenced by D-seq-1, 2026-08-10)*

**D-seq-1 (2026-08-10): Phase 4a runs immediately after Phase 2's exit, before Phase 3.** The §8 dependency graph always permitted it (P2 → P4 with P3 on a parallel track); what makes it the right schedule now is the ledger: cumulative decode is ≈ +17% on the fastest stream and encoder ≈ +11%, all of it fixed per-call scaffolding that direct dispatch is expected to let LLVM inline away. Running 4a first (a) executes the recovery checkpoint while the deficits are still only kernel-family deficits, (b) gives Phase 3's new decoder-side shim families (bitstream cursors, on the same hot streams) a recovered baseline and full tripwire headroom to land into, (c) triggers the parked families' first re-attempt point (T5-sad and any T7 parkees — earlier re-landing means less time with raw kernels live, which is the safety goal speaking, and F10 showed parked raw code is where latent UB sits), and (d) deletes real unsafe surface in its own right: the `Option<unsafe extern "C" fn>` tables, the 21+2 transmutes, and the untyped installer casts T6 deliberately left behind (`decoder_core.rs:1906`, `:920-921`).

- **Phase 4a (first, ~2 sessions):** kernel-dispatch de-virtualization — `mc.rs`'s consumers first (the D-perf-3 protocol, preserved verbatim: direct calls to `#[inline]` shims, `Option`-unwrap semantics verified post-init, one-pair measurement both benches), then the decoder kernel tables (intra-pred arrays, `sBlockFunc`, `sDeblockingFunc`, `sMcFunc`, expand, the cache-fill transmutes in `decode_slice.rs`), then the encoder's ~55 CPU-dispatch members. **Checkpoint duties:** re-measure every ledger row and record recovery; re-attempt every parked family (bodies re-measured with dispatch direct — D-perf-2's slices-and-offsets idea is on the table here); if recovery does not materialize, downgrade the ledger's remaining recovery claims to Phase 5/6 as D-perf-3's fallback prescribed.
- **Phase 4b (after Phase 3, ~1 session):** the config-dispatch enums that touch what Phase 3 rewrites (`pfWelsSpatialWriteMbSyn` CAVLC/CABAC, RC-mode fn table → enum) plus the strategy vtables (`ParamsetStrategy`, `RefStrategy`) and `IWelsVP` if Phase 3/6 haven't absorbed them naturally.

Tier 1 deletions (tables → direct calls; the Phase 0/2 work makes this mostly deletion), tier 2 enums (`EntropyCoder`, MD/ME strategy enums, RC mode — note `SWelsRcFunc` maps to an enum over the five RC modes already proven byte-identical), tier 3 traits (`ParamsetStrategy`, `RefStrategy`), `IWelsVP` inlined per §2.2.5, all 21+2 transmutes gone, `wels_func_ptr_def.rs` reduced to the ~15 algorithm choices and then dissolved into config-typed fields on `Encoder`.

*Exit gate: battery, **plus the D-perf-1 recovery checkpoint**: re-measure every deficit-ledger family's streams with dispatch direct (shims now inlinable into callers — span arithmetic folds against caller constants, `from_raw_parts` inlines to ~nothing) and record the recovery in the ledger. Risk: low — every replaced pointer provably pointed at exactly one function per config; assert-map the table-init functions before deleting them.*

### Phase 5 — Decoder structural rewrite *(9–12 sessions)* — the first pivot

Order inside the phase (each step lands with shims + full gates):

1. **5.1 Picture & DPB**: `Picture` re-struct (PaddedPlanes + Vecs), `PicPool`, `pic_queue.rs` recycling predicate, `manage_dec_ref.rs` (near-mechanical per survey), `error_concealment.rs` identity sites, P3 regression tests first.
2. **5.2 MbGrid**: kill `sMb`/`SDqLayer` double path (P2); `SDqLayer` → `DqLayerState` with owned `MbGrid`; re-point `parse_mb_syn_*` cache fills (their 30-entry scratch caches become `&mut` locals passed down — ~40 signatures, mechanical).
3. **5.3 Neighbor & MV machinery**: `mv_pred.rs` (punning → byte ops, `SetRectBlock` → typed generic on the grid), colocated reads via `cur_and_ref`.
4. **5.4 Deblocking driver** (`decoder/deblocking.rs`): `SDeblockingFilter` holds `PicId`s + plane cursors built per-MB; identity compare per P3.
5. **5.5 `decoder_core.rs`**: allocation → constructors (`AllocPicture` dies into `Picture::new`), paramset store (P4), context decomposition per §2.2.6, `Drop` teardown, `Default` derives.
6. **5.6 `decode_slice.rs`** last (P1), including EC MC paths; delete the last decoder shims and `SBitStringAux` shell; decoder modules get `#![deny(unsafe_code)]` one by one.

*Exit gate: battery with special attention to frame-count parity and the `#[ignore]` set; long fuzz run; decoder src/ is unsafe-free; **every §7.4 deficit-ledger entry whose shims died in this phase must clear** (the ledger empties as the scaffolding it measures is deleted). Risk: highest of the plan — this is where borrow-splits are designed for real. Mitigation: strict step order above, each step's shims keep the tree green, and any step can pause for a session boundary without leaving broken state.*

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

Built in Phase 2, session A. Invocations, from anywhere:

```bash
bash rust/tools/unsafe_ratchet.sh generate   # (re)write rust/tools/unsafe_baseline.json
bash rust/tools/unsafe_ratchet.sh check      # non-zero if any file × metric increased
bash rust/tools/unsafe_ratchet.sh report     # same output, always exit 0
```

Two counting rules the script encodes, both learned the hard way:

- **`unsafe fn` alone does not match `unsafe extern "C" fn`.** Phase 0's T5b deleted 113 definitions spelled that way and the naive pattern reported no change at all. The pattern is `unsafe (extern "C" )?fn`.
- **That pattern also matches function-pointer *types*** — `Option<unsafe extern "C" fn(..)>` occurs ~210 times in the dispatch tables. Definitions are therefore counted **line-anchored**; the pointer types show up in `raw_ptr`/`transmute`, which is where Phase 4 will show its work. Baseline at `956a8c07`: `unsafe_fn` 1372, `unsafe_block` 507, `raw_ptr` 5390, `transmute` 23, `unsafe_impl` 11, `mem_zeroed` 26, `no_mangle` 42, `SHIM(` 0. (§1.1's "~922 `unsafe fn`" was the naive count; 1372 is the real one.)

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

#### Current regime — v3, decision D-perf-4 (Eugene, 2026-08-09): safety first, speed tracked and recovered at checkpoints

*Supersedes the v2 gating rules below (kept as decision history). Direction: interim perf must not consume sessions or block conversion — the big unsafe mass is in Phases 3/5/6, and every session spent on a perf box delays it. We still want to be as fast as possible: fast idioms stay mandatory, everything stays measured and ledgered, and recovery is consolidated at the checkpoints instead of litigated per family.*

- **Priority order for interim work: byte-exactness > safety progress (ratchet) > speed.** Speed is never traded silently — it is ledgered and recovered at defined points — but it no longer stops a phase.
- **No hard ceiling; one tripwire.** The v2 "≤10% per stream, breach stops the phase" rule is retired. Instead: if a swap would push the **cumulative deficit past +25% median on any bench stream**, that family **parks** (proven, uninstalled — the existing §Parked machinery) instead of swapping; everything under the tripwire **swaps and ledgers by default**. A single family projecting >15% on a stream gets flagged in its ledger entry but still lands if the cumulative holds. No session stops, no escalation mid-phase — egregious surprises go in the log for Eugene, work continues.
- **Measurement, lightweight but honest:** commit B runs **one interleaved pair on both benches** (enough for a ledger entry and a tripwire check; the full 3-pair-medians protocol moves to phase exits only). The instrument rules stand — interleaved pairs (never sequential), null run before any "noise" claim, working set sized from the real caller, post-swap "raw" recovered via `git show`. The per-family microbenchmark is now **diagnostic, not mandatory** — build it when a number surprises, not before every family.
- **No optimization boxes during conversion phases.** If a family lands ugly: one *short* look (profile + disassembly, an hour, not a half-session box), then ledger-or-park by the tripwire arithmetic and move on. Multi-attempt investigations (T4's, T5-sad's) are retired until the recovery checkpoints.
- **The recovery checkpoints are where speed comes back**, and they are unchanged: **Phase 4** (direct dispatch — re-measure every ledgered family; the `mc.rs` dispatch-forward experiment from D-perf-3 folds back into Phase 4 proper as its first task, and D-perf-2's slices-and-offsets idea for tiny-block kernels is queued there too); **Phase 5/6** (shim deletion and caller conversion — scaffolding cost and parked families' raw bodies both die here by construction); and a **dedicated perf-recovery pass at Phase 9**, driven by the ledger, with the end-state target unchanged: **≤10% cumulative vs C++ scalar at Phase 9, aspirationally parity**. The ledger and §Parked tables are the contract that "for now" means *for now*.
- **Fast-by-construction stays binding:** the idiom catalog (const-generic copy widths, zips, exact-span trims, inline parity, `row()` over walkers) and the negative-results list are still rules — writing the fast version first costs nothing and is most of "as fast as possible".

*The v2 rules and decision records D-perf-1…3 follow as history; where they say "hard ceiling", "breach stops the phase", "body-parity precondition", or "half-session box", D-perf-4 supersedes them.*

*Restated 2026-08-08 after T4 (`common/mc.rs`) became the first family to exceed the original flat budget and the investigation showed the flat budget was measuring the wrong thing during a strangler phase. Decision record below.*

- Baseline: Phase 0 bench medians (`c_vs_rust_bench`, `decode_1080p_bench`). All comparisons are scalar-vs-scalar (the C++ dylib never dispatches SIMD), so ratios are meaningful.
- **Measurement protocol (normative since T4, extended by T5):** sequential `cargo bench` runs of the *same binary* drift ~3%, which cannot judge a 5% budget. Build control and candidate binaries, keep both on disk, run them **interleaved in one loop**, 3+ pairs, medians — and interleave *variants inside* a microbenchmark process too, not just binaries. For hot families, build the per-kernel scratch microbenchmark (old-via-`git show` vs new, per phase and block shape) *first*, not after the numbers look bad — it is what separates kernel cost from scaffolding cost. Profile (`/usr/bin/sample`) and **disassemble** before theorizing; T4 and T5 each burned two build-and-bench cycles on hypotheses that one disassembly refuted.
- **A microbenchmark's working set is part of its correctness (T5).** T5's SAD microbenchmark ran at a 1984-byte stride over 190 KB, so every row access missed cache and the safe kernels' per-row overhead hid behind the misses: it reported 0.82–1.33x where the encoder reported **+16.8%**. The same benchmark L1-resident reported 1.0–2.0x and agreed. **Size the working set from the real caller, state it beside the numbers, and measure both residencies when the caller's is not obvious.** Four more microbenchmark bugs T5 hit, each of which produced a plausible table first: no per-iteration `black_box` (LLVM caches results); observing only one of several outputs (LLVM deletes the rest of the kernel while the opaque raw call keeps computing it); a `const` stride where the real kernel takes a runtime one; variants timed in blocks rather than interleaved.
- **`FFMPEG` must be set for every gate run.** It was unset for the whole of Phase 2 sessions A and B, so `c_vs_rust_bench` skipped and the encoder was **unmeasured** across three families. T5's regression is the first one it would have caught. Encoder-side families (T7 especially) have no other end-to-end instrument.
- **Bisect a swap by file before optimising it.** T5 read as +13.9% overall; one extra build and two bench runs split it into a +16.8% half and a +0.57% half, which turned a wholesale revert into a narrow one.
- **Two ledgers, not one budget:**
  - **End-state regression** — cost intrinsic to the safe kernels/algorithms themselves, as isolated by the paired microbenchmark. Budget unchanged: bodies at ≤1.05x per family (investigate anything over), ≤10% cumulative at Phase 9 on the full-stream benches. This is the number that must hold forever.
  - **Scaffolding deficit** — overhead attributable to the strangler shims (span arithmetic, `from_raw_parts`, constructor asserts at the raw boundary), which Phase 5 deletes with the shims. A family may carry a scaffolding deficit past 5% **only if all three hold**: (a) the paired microbenchmark shows kernel bodies at ≤1.05x, (b) the overhead is demonstrated to be fixed per-call shim cost, not per-sample kernel cost, and (c) a **deficit ledger** entry in `perf_baseline.md` names the family, the measured deficit per stream, and the phase that deletes it. Hard ceiling regardless: **≤10% total regression on any bench stream at any commit** — a breach stops the phase for recovery (slim the specific shims, or unswap that family's commit B as the last resort; the two-commit discipline exists so unswap stays cheap).
- **Recovery checkpoints:** Phase 4 (direct dispatch makes shims inlinable into callers — measure the ledgered families' streams and record the recovery); Phase 5 (shim deletion — every ledger entry must clear; the ledger going empty is part of Phase 5's exit gate).
- Safe-but-fast idioms, as measured through T4 (order matters): **const-generic widths for copies** — `copy_from_slice` on a runtime length is a `memmove` *call*, and `copy_rows::<16>` vs `copy_rows(16)` was 10.8x vs ~1x; fixed-size array windows (`try_into().unwrap()` → `&[u8; 16]`); **zip iterators instead of indexing several slices by `j`** (LLVM will not prove seven bounds facts per sample); a rolling row window over re-fetching rows; `#[inline(always)]`/`#[inline(never)]` parity with the C++'s `inline`-plus-table structure; `row()`'s statically-sized window per row — **not** `chunks()`-based walkers, which measured worse everywhere they were tried (`perf_baseline.md` §Phase 2 T4, the row-walker table). `get_unchecked` is **banned** — if a hot loop can't be made fast safely, restructure the loop, don't reintroduce unsafe.
- Expect some *wins*: direct calls replacing indirect dispatch (Phase 4) and `match` replacing fn-pointer tables are LLVM-inlinable where the C design never was; T2 measured 1.3–1.8% *faster* and T3 a wash.

- **Parked families (added 2026-08-09 after T5-sad):** a family whose *bodies* cannot reach ≤1.05x on the honest microbenchmark at the shim boundary does not qualify for the deficit ledger (nothing later deletes body cost) and does not stay swapped — it is **parked**: safe kernels in-tree, differentially proven, spans pinned, shims unswapped, with a named re-attempt point (the Phase 4 direct-dispatch checkpoint, or the phase that converts its callers). Parked families are tracked in the ledger doc next to the deficits. Corollary, binding for every remaining call-dense tiny-block family (SAD/SATD class): **measure bodies on the L1-resident microbenchmark *before* swapping** — body parity is a precondition of the swap attempt, not something to discover from the stream bench afterwards.
- **Microbenchmark validity (added 2026-08-09):** per-call overhead must be measured **L1-resident** — T5's harness ran a 1984-byte stride over 190 KB and the overhead hid behind cache misses (0.82–1.33x reported against a real +16.8%). And the encoder bench is only a gate when it actually runs: `gates.sh` now locates ffmpeg itself after silently skipping the encoder bench for three encoder-touching families.

**Decision D-perf-1 (2026-08-08): T4's ~7–8% is carried as a scaffolding deficit, not reverted.** Basis: the kernel bodies measure 0.88–0.99x (at parity or better — the end-state cost is zero or negative); the residual is 7–16 ns of fixed per-call shim overhead on the most call-dense family in the codec; three structural mitigations were built, paired-measured, and rejected, so further optimization attempts are closed; and unswapping would forfeit the full battery's continuous exercise of the safe kernels while concentrating re-swap risk exactly where the strangler pattern exists to avoid it. The deficit is ledgered in `perf_baseline.md`, bounded by the ≤10% ceiling, checkpointed at Phase 4, and must clear at Phase 5. *Addendum 2026-08-09: with the encoder bench now actually running, T4's encoder-stream contribution was never isolated — measure it at the next session start and append it to T4's ledger row.*

**Decision D-perf-3 is OPEN and blocks nothing yet, but it is the phase's largest
outstanding question (raised 2026-08-09): the encoder-side ceiling is breached.**
D-perf-1's addendum asked for `mc.rs`'s encoder contribution to be isolated now that
the encoder bench actually runs. It is **+7.6% median and +16.7% worst** across 28
usable stream/thread rows, against a same-binary null floor of ±2% — eleven rows over
§7.4's 10%-per-stream hard ceiling — and T8's +3.5% sits on top of it, so the live
cumulative encoder deficit is roughly **+11%**. T4's ledger *eligibility* is unchanged
(bodies 0.88–0.99x, residual is fixed per-call scaffolding Phase 5 deletes); what is
new is the magnitude, and its cause is structural rather than fixable: motion
estimation calls MC far more often per frame than decoding does, so a fixed per-call
shim cost is levied far more often. The three options are carry-with-a-raised-ceiling,
revert `mc.rs` until Phase 5 converts its callers, or pull Phase 4's direct-dispatch
checkpoint forward for `mc.rs` alone (direct calls are what make a shim inlinable, and
inlining is exactly what deletes a per-call cost). Evidence and the full table are in
`perf_baseline.md` §"The encoder-side ceiling is breached". **This is Eugene's call,
as D-perf-1 was.**

**Decision D-perf-3 (2026-08-09): the encoder ceiling breach is recovered by pulling Phase 4's direct-dispatch checkpoint forward for `mc.rs` alone — boxed, with unswap as the pre-authorized same-session fallback.** The breach: T4's encoder contribution measured +7.6% median / +16.7% worst (11 rows over the 10% ceiling; cumulative with T8 ≈ +11%). Of the three options: raising the ceiling the first time it binds would convert the mechanism from a gate into a suggestion — rejected; unswapping outright clears the breach but retreats from a within-budget decode position and, worse, leaves the **Phase 4 recovery hypothesis untested** while T6/T7 accumulate more scaffolding deficits premised on it. Direct dispatch for `mc.rs`'s consumers is a surgical slice of Phase 4 (~13 call sites already enumerated in T4's log), it attacks the measured mechanism exactly (fixed per-call cost that inlining removes — reinforced by D-perf-2's finding that cursor construction is a large per-call cost at small block sizes), and it converts the ledger's load-bearing assumption into evidence either way. Success metric: every encoder stream's cumulative deficit back ≤10% (mc median expected ≲5%), decode not regressed, all gates green. Fallback if the box closes unmet: revert the dispatch commit, unswap `mc.rs` commit B, park the family (both sides), and **downgrade every "Phase 4 checkpoint" claim in the ledger to "re-evaluate at caller conversion"** — if inlining doesn't recover scaffolding cost here, it won't elsewhere, and the plan must stop promising it. Corollary rule, binding from T6 on (session D's finding): **a per-call deficit scales with call count — an encoder-side family's budget is not its decoder-side budget; every commit B measures both benches before merging.**

**Decision D-perf-2 (2026-08-09): T5-sad gets one bounded re-landing attempt, scheduled with T8 in the next session — T6's order is unaffected, and T7 inherits the verdict either way.** The question the attempt answers is not "can SAD be a bit faster" but the technique question that gates T7's ~30 SAD/SATD-shaped kernels: whether per-row cost on tiny fixed blocks with runtime strides can be made to fold in safe code at the shim boundary at all. Time-box: half a session, hard stop, disassembly-first (both prior guesses measured null; `row_windows` alone was not enough). Exit states, both acceptable: (a) bodies reach ≤1.05x L1-resident and the family re-swaps inside the ceiling — the technique transfers to T7; or (b) it doesn't, T5-sad stays **parked** until the Phase 4 checkpoint or its callers convert (ME is Phase 6.3), and T7's SAD/SATD families go **prove-and-park** from the start instead of burning swap-measure-unswap cycles. What is not acceptable is a third unbounded investigation: one attempt, one verdict, recorded.

### 7.5 Session workflow
Each session: run battery → pick next plan item → convert with shims → battery → commit with ratchet update → update this doc's checkbox (add a `## Progress` appendix on first execution session). The phase structure is deliberately interruptible; no step leaves the tree broken across a session boundary.

The battery is `rust/tools/gates.sh` (built in Phase 2, session A), in four levels so the
cheap ones can run per commit:

```bash
bash rust/tools/gates.sh commit   # cargo test debug+release + ratchet          (~2 min)
bash rust/tools/gates.sh family   # + diffharness st/mt/def in BOTH profiles    (~5 min)
FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh full    # + benches + Miri --lib
FFMPEG=/opt/homebrew/bin/ffmpeg bash rust/tools/gates.sh exit    # + Miri over tests/*differential*
```

It prints one `PASS`/`FAIL`/`SKIP` line per gate and exits non-zero on any failure.
Three behaviours are deliberate: the test gate checks the **ignored count is exactly 20**
as well as the exit status (a test that stops being compiled looks like a test that
passes); a sweep failure prints **F3's retry rule** next to the result rather than
leaving each session to rediscover it; and the encoder bench **SKIPs loudly** without
`FFMPEG`, because its fallback measures the frame-skip path rather than the encode
kernels (`perf_baseline.md` §2). The fuzz gate (§7.2 gate 6) prints a permanent SKIP
until T7 lands.

---

## 8. Sequencing rationale & parallelism

```
P0 ──► P1 ──► P2 (kernels) ──► P4 (dispatch) ──► P5 (decoder pivot) ──► P8 (API) ──► P9
         └──► P3 (bitstream) ──┘                └► P6 (encoder pivot) ─► P7 (threads) ┘
```

- **Common-first** because both codecs consume the same vocabulary types and kernels; **decoder pivot before encoder pivot** because the decoder has the strongest external ground truth (JVT streams + h264dec triangulation), no threading, and is 30% smaller — it's where the arena/grid/borrow-split patterns get proven before the bigger encoder applies them.
- **Phase 4a before Phase 3 (D-seq-1, 2026-08-10):** the graph above always allowed P4 directly after P2; with the Phase 2 ledger carrying decode +17% / encoder +11% of shim scaffolding and the parked families waiting on the dispatch checkpoint, 4a's recovery-and-unpark pass runs first so Phase 3's new shim families land on a recovered baseline. 4b (the entropy/RC enums that touch what Phase 3 rewrites) stays after Phase 3.
- **Threading after encoder ownership** (the split-borrow shape is unknowable before `sWelsEncCtx` is decomposed) and **the API boundary last** — the boundary can only shrink to a thin translation shim once the safe core it translates *to* exists, and until then the C API (and the tests driving it) stays byte-for-byte still.
- Parallelizable across concurrent sessions/agents: within Phase 2 (kernel families are independent), Phase 3's read vs write sides, Phase 5 vs early Phase 6 steps (6.1/6.2 don't depend on decoder work — only on Phases 1–4), and any R1 sweep. The pivots (5.5/5.6, 6.6) and Phase 7 are single-track.

---

## 9. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| More latent UB like the release-deblocking segfault surfaces mid-refactor (an optimizer or layout change flips behavior that was never defined) | **High** (raised: the known instance turned out to be a stack overflow executing thousands of times per frame on the mainline path, silently, for the port's whole life) | High | Phase 0 root-caused the known instance (§1.4, already fixed by `e6fce464`) and added the dual-profile gate; any later debug/release disagreement halts the phase until root-caused — it is always a pre-existing bug being exposed, and fixing it is in scope. Note the instance was *invisible to every gate*: 341/341 byte-identical sweeps ran on top of it. Byte-exactness does not imply soundness, which is the argument for the fuzzer (§7.3) and for Phase 2 replacing `*mut u8` kernel arguments with real array types |
| Silent behavior change on malformed streams (EC paths, slop P6) | Medium | High (conformance can't see it) | Error-code parity tests, fuzz differential vs C++, malformed-stream corpus from the ignored e2e cases |
| Perf: scaffolding deficit on call-dense families (T4's ~7% is the instance; intra worst-case measured a wash, so kernel-intrinsic risk is largely retired) | Medium | Medium | §7.4 two-ledger rules + ≤10% hard ceiling; Phase 4 recovery checkpoint; Phase 5 must clear the ledger; unswap-commit-B as last resort |
| Estimates derived from the surveys run low (Phase 2's kernel count was −60%; per-family brief figures wrong in both directions) | High | Low–Medium | Count before sizing every session; phases 5/6 are file/deref-based (more robust) but treat all session counts as −0/+40% ranges |
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
- [x] **T5c — decoder threading scaffolding.** Done in the Phase 2 session A, in four
      commits, dead duplicates first and entry points last: `3e09a814`, `e3ca3e17`,
      `5fe41f17`, `01ad9ba1`.
      `GetThreadCount` is a literal **`0`**, not `1`, with the reason
      (`api/codec_api.rs:1831` branches on `<= 0`) now in its docstring rather than only
      in the log. `Picture::pNzc` and `Picture::pReadyEvent` went with it, and
      `GetPNzc`'s picture-vs-layer fork collapsed to the layer source because the
      picture side was statically unreachable. Totals: `unsafe_fn` 1372 → 1365,
      `raw_ptr` 5390 → 5352, `unsafe_block` 511 → 507.
- [x] **T5d — duplicated bitstream writer.** No deletion: there are four copies, not
      two, and they are not identical. Recorded as `phase0_findings.md` §F2 per the stop
      rule; Phase 3.2 owns the dedupe. `dc8487cb`
- [x] **T5e — small stragglers.** `4e174285`. The dead `use std::ffi::c_void` in
      `lib.rs` was the whole yield: `RUSTFLAGS="--force-warn dead_code" cargo build`
      turned up nothing belonging to the T5a–T5c families, only pre-existing unrelated
      dead code that the phase's own rule keeps out of scope.
- [x] **T6 — ratchet script + gate runner.** `99f4ab5c`, in the Phase 2 session A.
      Both instruments and their invocations are documented in §7.1/§7.5; the counting
      traps that motivated the note are encoded in the script.
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

### Phase 2 — leaf DSP kernels, both codecs

Sized at 5–7 sessions, resized to 6–8 on 2026-08-08, briefly 8–9, and **7–8 as of
D-perf-4** (four spent through session D; the recovery session is cancelled; remaining:
T6 alone, T7 across two, T9). Original basis: (real total ≈190 fns, not ~120;
T7 alone is 69 — bigger than T3 and T4 combined). Session A landed the preconditions,
the control and the pilot; session B landed T4 and the budget investigation.
Findings from this phase are in [`phase2_findings.md`](phase2_findings.md).

- [x] **Phase 0 T6 — ratchet script + gate runner.** `99f4ab5c`. Both instruments
      built and documented in §7.1/§7.5. Two counting traps encoded: the pattern is
      `unsafe (extern "C" )?fn`, and definitions are counted line-anchored so the ~210
      `Option<unsafe extern "C" fn(..)>` table entries don't inflate it. Baseline at
      `956a8c07`: `unsafe_fn` **1372** (not §1.1's naive 922), `unsafe_block` 507,
      `raw_ptr` 5390, `transmute` 23, `unsafe_impl` 11, `mem_zeroed` 26, `no_mangle` 42,
      `SHIM(` 0. `check` verified to fail on an increase.
- [x] **T1 — control run.** Green at `afcdd785` on the full `exit` battery:
      `cargo test` 375/0/20 and 373/0/20; `sweep.sh st mt def` 341/341 in **both**
      profiles (no F3 hit this session); both benches bit-identical; all three Miri
      invocations clean. Perf control in [`perf_baseline.md`](perf_baseline.md)
      §Phase 2.
- [x] **T2 — PILOT: `decoder/decode_mb_aux.rs`.** `ba13bdbd` (safe kernels +
      differential proof), `ee7818fb` (swap + shims, equivalence entries deleted).
      **Four kernels, not the ~13 the brief expected** — the C++ file has more, the
      Rust port has `IdctResAddPred_c`, `IdctResAddPred8x8_c`, `IdctFourResAddPred_c`
      and `GetI4LumaIChromaAddrTable` and nothing else (314 LOC, not 374).
- [x] **Pilot retro.** Verdicts in §2.2.1 (cursor beats triple; the loop order, not the
      API, is what has to change; `reborrow` added; shim contracts are short only for
      forward-reaching kernels) and in §Phase 2 (what the ratchet does and does not
      move during a strangler phase). Numbers in `perf_baseline.md` §Phase 2 T2:
      **1.3–1.8% faster**, not slower.
- [x] **Phase 0 T5c/T5e — decoder threading scaffolding + stragglers.** `3e09a814`,
      `e3ca3e17`, `5fe41f17`, `01ad9ba1`, `4e174285`. Taken after the pilot rather than
      before it (the session log argues the ordering); Phase 0's checklist above now
      records the outcome. **Phase 0's only remaining open item is T7, the fuzz crate,
      deferred by direction** — so §7.2 gate 6 stays unavailable and `gates.sh` prints a
      permanent SKIP for it. F8 is the first recorded case of a finding a fuzzer would
      plausibly have reached first.
- [x] **T3 — `decoder/get_intra_predictor.rs`** — the designated perf worst case,
      **42** kernels (not 44): 14 I4x4, 14 I8x8, 7 chroma, 7 I16x16. `b7f48311`
      (safe kernels + differential proof), `a4828187` (swap + shims). Verdicts:
      **the worst case is a wash** — every decode-bench row inside ±1%, which is the
      per-side spread (`perf_baseline.md` §Phase 2 T3), so §9's "perf regression in
      MD/ME/intra hot loops" risk can be downgraded. Bounds checks land **per row**,
      not per sample, and `copy_from_slice`/`fill` on fixed-size windows keep the wide
      stores the punned `u32`/`u64` accesses gave. All ~140 punned accesses in the file
      turned out to be pure byte *moves*, so `u32::from_ne_bytes` — T7's nominated
      replacement — was not needed anywhere in it. These are also the first shims whose
      span is not derivable from the signature: they anchor at `pPred - stride - 1` and
      their contract names `PADDING_LENGTH` as the reason the `-1` row and column exist.
- [x] **T4 — `common/mc.rs`**, motion compensation. **26 kernels** (24 safe bodies,
      two of them shared by the `McHorizLuma_c`/`McVertLuma_c` aliases) plus two filter
      helpers; `InitMcFunc` left alone as dispatch plumbing. `46053993` (safe kernels +
      differential proof), `ea52b387` (swap + 28 shims), `e26164e6` (the rejected row
      walker). Firsts for the phase: **two planes** — a `PlaneCursor` read + a
      `PlaneCursorMut` write, since reference and current are different allocations —
      and the first **encoder-side consumers** (`pfLumaHalfpelHor/Ver/Cen` at
      `iWidth + 1` up to 17). The module-internal `pWelsMcFunc_c` quarter-pel table is
      folded into `mc_luma`'s `match` and deleted, per §3.2's single exception.
      **This family is over budget: +8.2% / +7.2% / +7.0%** against §7.4's 5% —
      investigated at length, written up in `perf_baseline.md` §Phase 2 T4, and the
      residual is per-call overhead in the *shim* layer, which Phase 5 deletes. Three
      structural fixes were tried and rejected by paired measurement (by-value cursors,
      shim-to-shim dispatch, and a `rows()`/`rows_mut()` walker added to the Phase 1
      plane API and then reverted). Three real defects were found and fixed: a runtime
      `copy_from_slice` lowering to a `memmove` **call** (10.8x on the zero-MV copy),
      per-sample bounds checks across seven same-length slices, and missing
      `#[inline(always)]`/`#[inline(never)]` parity with the C++.
- [x] **D-perf-1 — the T4 budget call (2026-08-08).** Carried as a scaffolding
      deficit, not reverted; §7.4 restated with the end-state/scaffolding split, the
      interleaved-pairs measurement protocol, the deficit ledger
      (`perf_baseline.md` §Ledger), the ≤10% hard ceiling, and the Phase 4/Phase 5
      recovery checkpoints. Further T4 optimization attempts are **closed** — three
      structural fixes were built, paired-measured, and rejected; do not reopen
      without new evidence of a different mechanism.
- [x] **T5-intra — `common/intra_pred_common.rs`.** `56a3dbf9` (A, with the SAD kernels)
      and `209d3c66` (B). Two 16x16 luma predictors behind shims, **+0.57% median** on
      the encoder bench. Per-kernel reference shapes rather than a shared one — V reads
      the row above, H reads the column left — and both write a *packed* `[u8; 256]`,
      which is what distinguishes them from the same-named 2-arg decoder cousins.
- [~] **T5-sad — `common/sad_common.rs`.** Safe kernels written and differentially proven
      (`56a3dbf9`); swapped in `209d3c66` and **unswapped in `11f82d41`**. The swap cost
      the encoder **+16.8% median, +78% worst**, breaching §7.4's 10% ceiling. The raw
      kernels run; the safe ones are dead but proven and their spans are pinned.
      Re-landing needs a **per-row** overhead fix: the corrected (L1-resident)
      microbenchmark shows cost independent of block width — 4x8, 8x8 and 16x8 all
      ~7.0 ns through the safe kernel — so the per-sample arithmetic is already free and
      only the 16-wide shapes amortise the per-row bounds and iterator work. Rolling the
      row offset and `u8::abs_diff` were both measured and changed nothing.
- [x] **`PlaneCursor::row_windows`.** `840a48dc`. One bounds check per block instead of
      two per row, for kernels whose stride and buffer length are runtime values and
      whose checks therefore cannot fold. Does **not** repeal T4's `chunks` verdict —
      `mc.rs` keeps `row()`, where the widths are const and the checks do fold — and the
      rule is written on the method.
- [x] **D-perf-2 — the T5-sad sequencing call (2026-08-09), and its verdict
      (2026-08-09).** The one bounded attempt was spent. The candidate was T8's
      exact-span shape, which had just taken that family from 2.51x to 1.02x and was
      not in T5's rejected set; on SAD it measured **1.39-2.97x L1-resident even in
      the framing most favourable to it**. **T5-sad stays parked and T7's SAD/SATD go
      prove-and-park from the start.** The box is closed. One thing found on the way
      that a later reopening should start from: `PlaneCursor::new` costs enough at
      this block size that paying it twice per call moved 4x4 from 1.61x to 4.93x, so
      handing these kernels slices-and-offsets instead of cursors is a live idea —
      and a convention change, hence not this phase's. `perf_baseline.md` §Parked.
- [x] **D-perf-3 — the encoder ceiling breach (decided 2026-08-09; superseded by
      D-perf-4 before execution).** T4's encoder side measured +7.6% median / +16.7%
      worst; cumulative with T8 ≈ +11% against the then-live 10% ceiling. The decided
      recovery (mc.rs direct-dispatch forward, unswap fallback) was **cancelled as a
      dedicated session** when D-perf-4 retired the ceiling: the breach no longer
      blocks, the deficit stays ledgered, and the dispatch-forward experiment returns
      to **Phase 4 as its first task**. The rule it produced survives: an
      encoder-side family's budget is not its decoder-side budget; commit B measures
      both benches (now as one interleaved pair each).
- [x] **D-perf-4 — the regime change (Eugene, 2026-08-09).** Safety first; §7.4 v3:
      no hard ceiling, +25%-cumulative tripwire parks a family instead of stopping a
      phase; swap-and-ledger is the default; no optimization boxes; lightweight
      commit-B measurement; recovery consolidated at Phase 4 / Phase 5–6 / a Phase 9
      perf pass with the end-state target unchanged. **T6 is unblocked and is the
      next action.**
- [x] **T6 — `common/deblocking_common.rs` + F1's `uiBS` surgery + expansion
      (`bbb9348e`, `64633e5f`, `d878916b`, `2fe283e4`, `9c32f890`).** All three items
      landed in one session under D-perf-4. Deblocking: 7 safe kernels (6 edge
      filters porting the `(iStrideX, iStrideY)` V/H trick as one `(step_x, step_y)`
      body each, + `nonzero_count`), the 12 ABI wrappers *are* the shims, 6 raw inner
      kernels deleted, all 14 `no_mangle` exports gone; decode +7.8/+2.6/+2.3%,
      encoder flat (not a consumer), ledgered — cumulative decode ≈ +17/+10/+10%,
      under the tripwire. The F1 surgery: `uiBS` is `[[[u8; 4]; 4]; 2]` through the
      `PDeblockingBSCalc` typedef and every signature, the five 32-byte
      `from_raw_parts_mut` sites and the 32-byte read deleted; byte-exact, encoder
      noise-level. Expansion: one pad-parameterised `expand_picture` in
      `common/expand_pic.rs`; the two `_c` shims reconstruct the full allocation
      from the mid-pointer (`expand_shim_span`, pads 32/16) — no call site needed
      the explicit-pad fallback; perf inside the noise floor. Findings: **F10**
      (the parked raw SAD kernels' trailing pointer bump is UB on exactly-sized
      buffers — test accommodated, T7's prove-and-park tests inherit the rule).
      The differential span probes gained a **golden direct-run comparison** after
      a +1-anchor mutation survived the touch-set assertion — span size, touch set
      and anchor are now pinned independently.
- [~] **T7 — the four encoder kernel files. Part 1 complete (session F,
      2026-08-10); part 2 (`encoder/get_intra_predictor.rs`) remains.**
      **F-1 `encoder/encode_mb_aux.rs`**: 21 kernels + installer; `f233d506` (safe
      kernels + differential proof, three mutations killed), `b1fb7448` (swap + 21
      shims). Fixed-array signatures throughout — the two strided Hadamard reads
      carry exact-reach types `[i16; 49]` and `[i16; 241]`. Encoder +2.1% median /
      +9.9% worst usable (T4's per-call mechanism on flat content), decode flat;
      cumulative encoder ≈ +13%, under the tripwire → ledgered.
      **F-2 `encoder/decode_mb_aux.rs`**: **9 leaf kernels by recount** (the brief
      said 6 — five raw bodies live in `svc_encode_mb.rs`; the OnMb composite and
      installer are dispatch plumbing); `fc92dab0` (A), `55e6d7fe` (B, 9 shims).
      Noise-level on both benches. Finding **F11**: `WelsIHadamard4x4Dc`'s plain
      `i16` adds panic in debug above ±2047 where the C++ wraps — third member of
      F8's class, reproduced not repaired.
      **F-3 `encoder/sample.rs` SATD**: proven-and-parked directly per D-perf-2,
      `1383acb5` + `20e84e47` — 7 safe kernels, nothing installs them, raw bodies
      live, re-attempt at Phase 4a with T5-sad. F10 got a **second instance**: the
      composites' sub-block bumps reach past whole-row buffers, so raw-side
      differential buffers are `(h+1)*stride` now (Miri caught the under-sizing
      mid-session). One F3 hit with a **new signature shape** (Rust output *longer*)
      was attributed by the alternating-loop protocol (24/24 clean on the exact
      config; A 0/360 vs B 1/360 preset configs) and appended to F3 as its sixth
      measurement — the signature's output clause is now "any wrong length".
- [x] **T8 — `processing/{vaacalc,adaptive_quantization}.rs` (`d41244c2`,
      `af98f6ab`).** **6 kernels** as recounted (vaacalc 5 + `SampleVariance16x16_c`),
      not the continuation brief's 11. No dispatch table and no `no_mangle` anywhere
      in the family, so all six shims are plain `unsafe fn`. Bodies at 0.69-1.04x
      against the raw ones except `VAACalcSadBgd_c` at **1.44x**, which is recorded
      with its mechanism rather than fixed; end-to-end +3.48% median on the encoder,
      inside the ceiling. Two microbenchmark errors and their fixes are in
      `perf_baseline.md` §Phase 2 T8 — the important one is that **after a swap the
      "raw" side of a microbenchmark is the shim**, so bodies must be measured before
      swapping or recovered with `git show`. Findings: **F9** (`iFrameSad` overflows
      above 32 896 macroblocks) plus two false claims corrected in the converted
      files' own headers.
- [x] **A new instrument step: the null run.** Before attributing an encoder-bench
      reading to noise, run the **same binary in both slots** through the identical
      harness. Measured here at median +0.00% / max +1.81% / zero rows over 5%, which
      turned a suspected artefact into a real +3.48%. Also re-condemned Spatial Ramps
      (-38% between two runs of one binary). Cheap, decisive, and it belongs in front
      of every "that's just noise" conclusion from here on.
- [ ] **T9 — phase exit.** Not started. Besides the standard exit duties, T9 carries
      the **consolidation deliverables** (2026-08-10, full spec in the continuation
      brief §T9): the status preamble at the top of this document, hoisting the
      durable working rules into §7.6, and writing `prompts/phase4a.md` as the
      hand-off. Phase numbers stay as permanent identifiers — no renumbering;
      execution order is owned by this appendix and §8.

### Phases 3–9

Not started. Order per **D-seq-1** (2026-08-10): after Phase 2's T9, the next phase
is **4a** (kernel-dispatch de-virtualization + the recovery/unpark checkpoint), then
Phase 3 (bitstream), then 4b, then Phase 5. Rationale recorded at the §5 Phase 4
heading and §8.
