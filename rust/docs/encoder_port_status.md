# Encoder port status

Living record of the OpenH264 **encoder** Rust port. The decoder port is mature and
passes conformance; the encoder is the work in progress. Update this file at the end
of every phase.

---

## Reproducing the differential test

The port's definition of done is *byte-identical output with the C++ encoder for the
same input and parameters*. The harness lives in `rust/tools/diffharness/`.

```bash
make -j8 libraries binaries
```

```bash
./rust/tools/diffharness/build.sh
```

```bash
./rust/tools/diffharness/compare.sh res/CiscoVT2people_160x96_6fps.yuv 160 96 5 26 0 -1
```

`compare.sh <yuv> <w> <h> <frames> <qp> <cabac> <gop>` runs both encoders and `cmp`s
the Annex-B output; it exits 0 only when the streams match. It also feeds the Rust
stream to `./h264dec` as a sanity check. Artifacts land in
`rust/tools/diffharness/out/` (gitignored).

### Why not drive `h264enc` directly

`h264enc` derives most of `SEncParamExt` from `FillSpecificParameters()` plus config
files, so matching it from Rust means replicating CLI parsing rather than testing the
encoder. Instead `cxx_enc.cpp` and `rust_enc/main.rs` each set a **fully explicit,
identical** `SEncParamExt`. The only variable under test is the encoder itself.

One trap worth recording: `h264enc` silently drops frames when the input frame rate
and the layer output frame rate disagree — `-frms 5` on a 5-frame file yields
`Frames: 2` unless you also pass `-frin 6 -frout 0 6`. The harness sidesteps this by
setting `fMaxFrameRate` and `sSpatialLayers[0].fFrameRate` to the same value.

### Gate configuration (Phase 5 target)

Single spatial layer, single slice, CAVLC, RC off, single-threaded, deblocking on,
`CONSTANT_ID`, no LTR/denoise/AQ/BGD/scene-change. See `cxx_enc.cpp` for the exact
field-by-field setting.

---

## C++ oracle

| Tool | Command | Status |
|---|---|---|
| `libopenh264.a`, `h264enc`, `h264dec` | `make -j8 libraries binaries` | builds clean |
| `codec_unittest` | `make gtest-bootstrap && make -j8 test` | builds; 534 tests, see below |

`codec_unittest` baseline on this machine (darwin/arm64): **533 pass, 1 fail**.
The failure is `DecoderDeblocking.DeblockingInit`, pre-existing and unrelated to the
encoder port. `EncUT_*` suites all pass and are the per-function oracles for Phase 3.

### ABI ground truth

Extracted from `codec/api/wels/codec_app_def.h` with a `sizeof`/`offsetof` dump
(darwin/arm64, and the same on any LP64 target):

| type | size |
|---|---|
| `SEncParamBase` | 24 |
| `SEncParamExt` | 924 |
| `SSpatialLayerConfig` | 200 |
| `SSliceArgument` | 152 |
| `SSourcePicture` | 80 |
| `SLayerBSInfo` | 56 |
| `SFrameBSInfo` | 7192 |
| `SEncoderStatistics` | 88 |
| `SBitrateInfo` | 8 |
| `OpenH264Version` | 16 |

Key offsets: `SFrameBSInfo::sLayerInfo` 8, `::eFrameType` 7176, `::iFrameSizeInBytes`
7180, `::uiTimeStamp` 7184. `SLayerBSInfo::eFrameType` 4, `::iSubSeqId` 12,
`::iNalCount` 16, `::pNalLengthInByte` 24, `::pBsBuf` 32, `::rPsnr` 40.
`SSourcePicture::pData` 24, `::uiTimeStamp` 64.

These are asserted at compile time in `rust/crates/openh264-rs/src/api/abi_guard.rs`.

---

## Baseline (start of this work, commit `c4247c7b`)

```
frame 0: type=videoFrameTypeInvalid layers=0 bytes=0
frame 1: type=videoFrameTypeInvalid layers=0 bytes=0
...
```

The encoder returned success and emitted **zero bytes** for every frame.
C++ reference for the same input: 8034 bytes (IDR 3295 + 4 P-frames).

`cargo test --test loopback_sha1_test`:

- `test_loopback_encode_and_decode_pipeline` — passed **vacuously** (its decode step is
  guarded by `eFrameType != videoFrameTypeInvalid`, never true).
- `test_decode_encode_full_cycle_sha1_parity` — FAILED with
  `da39a3ee5e6b4b0d3255bfef95601890afd80709`, the SHA-1 of the empty string.

Root causes, all confirmed against the code:

- **A.** Public API structs declared twice — in `lib.rs` *and* `api/codec_api.rs` — with
  incompatible layouts. A local item beats a glob re-export, so `crate::SFrameBSInfo`
  resolved to the 3104-byte `lib.rs` version while callers passed the correct
  7192-byte one. The C-ABI shims cast between them, so every encoder write landed at
  the wrong offset.
- **B.** 63 encoder-internal types declared in multiple modules with divergent field
  sets (`SDqLayer` ×11, `SWelsFuncPtrList` ×10, `sWelsEncCtx` ×9, …), making each
  module a compile-clean island.
- **C.** The context is never built — `pSpsArray`/`pPPSArray`/`iSpsNum`/`iPpsNum` never
  assigned, `pCurDqLayer` stays null.
- **D.** `WelsEncoderEncodeExtRust` is a sketch: hardcoded IDR, one slice buffer, no
  frame-type/GOP decision, RC, ref lists, preprocessing, or padding.

---

## Phase log

### Phase 0 — oracle and baseline — **DONE**

Gate 0 met: `h264enc`/`h264dec`/`codec_unittest` build and run; `compare.sh` is a
reproducible byte-comparison; baseline recorded above.

### Phase 1 — unify the public API structs — **DONE**

Deleted all 26 items `lib.rs` re-declared on top of `api/codec_api.rs` (8 structs,
7 enums, 11 constants); `api/codec_api.rs` is now the single source of truth and
`lib.rs` is 57 lines. Removed the 5 pointer casts in the C-ABI shims. Fixed the
`eFrameType`-as-`i32` assignments.

Enum variants were renamed to the verbatim C++ spellings that `codec_api.rs`
already used (`RCMode::RcQualityMode` → `RC_MODES::RC_QUALITY_MODE`, etc.),
121 sites across 8 modules.

**Gate 1 met.** `cargo build` clean; ABI guards pass; the encoder probe went from

```
frame 0: type=videoFrameTypeInvalid layers=0 bytes=0
```

to

```
frame 0: type=videoFrameTypeIDR layers=1 bytes=0
```

i.e. the encoder's writes now land in the caller's struct. Still 0 bytes — Phase 4.

#### Defects found beyond the audit

- `lib.rs` had two `CM_*` constants with **wrong values**: `CM_UNSUPPORTED_DATA` = 4
  (C: 5) and `CM_UNKNOW_REASON` = 5 (C: 2). It also invented `CM_UNINITIALIZED_ERROR`
  = 2, which is not in `CM_RETURN`; C++ returns `cmInitExpected` (4) when
  uninitialized. All now resolve to `api/codec_api.rs`'s correct values.
- `ENC_RETURN_*` in `wels_encoder_ext.rs` ran 0..5 densely; they are a **bit field**
  in `wels_const.h` (`MEMALLOCERR` 0x01, `UNSUPPORTED_PARA` 0x02, `UNEXPECTED` 0x04,
  `CORRECTED` 0x08, `INVALIDINPUT` 0x10, `MEMOVERFLOWFOUND` 0x20,
  `VLCOVERFLOWFOUND` 0x40, `KNOWN_ISSUE` 0x80). Corrected.
- `MAX_SHORT_REF_COUNT` was 16; `wels_const.h:115` defines it as `MAX_GOP_SIZE >> 1`
  = 4. `LONG_TERM_REF_NUM` was 1 (C: 2) and `LONG_TERM_REF_NUM_SCREEN` 2 (C: 4), so
  `MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA` was 16 instead of 6. Corrected.
- `MAX_GOP_SIZE` = 64 fixed to `1 << (MAX_TEMPORAL_LEVEL - 1)` = 8 (audit 1.5.4).

#### Work pulled forward from later phases

`SWelsSvcCodingParam` had to be unified early: `wels_encoder_ext.rs`'s copy was a
strict subset of `param_svc.rs`'s with truncated bodies (`FillDefault` 16 lines vs
83, `ParamTranscode` 52 vs 187, and a `DetermineTemporalSettings` that ignored the
temporal-id table entirely). Deleted it; `param_svc.rs` is now the only definition.

That exposed a second issue: the deleted copy's `ParamBaseTranscode` carried an
ad-hoc `iPicWidth <= 0` check that **does not exist in C++** — upstream validates in
`ParamValidationExt`, which the port had never implemented. So `ParamValidation`
and `ParamValidationExt` (encoder_ext.cpp:264 and :403, Phase 4 step 2) were ported
now, along with a new `au_set.rs` holding `WelsBitRateVerification`,
`WelsAdjustLevel`, `WelsCheckNumRefSetting`, `WelsCheckRefFrameLimitationNumRefFirst`
and `WelsCheckRefFrameLimitationLevelIdcFirst`. `CheckProfileSetting` and
`CheckLevelSetting` were replaced with the real bodies (they previously just
assigned the field).

Two branches of `ParamValidationExt` are **explicit `todo!()`**, not silent
fall-throughs: `SM_FIXEDSLCNUM_SLICE` and `SM_RASTER_SLICE` need
`SliceArgumentValidationFixedSliceMode` / `CheckRowMbMultiSliceSetting` /
`CheckRasterMultiSliceSetting` from `svc_enc_slice_segment.cpp`, which is a whole
module still to port (Phase 3.9). `SM_SINGLE_SLICE` and `SM_SIZELIMITED_SLICE` are
complete.

#### Test expectations corrected

Two Rust tests asserted `cmResultSuccess` for configurations the **C++ reference
rejects** (verified by running libopenh264.a with the identical parameters):

- `api_param_bounds_test::test_encoder_screen_content_scroll_motion_vector_bounds`
  uses height 1800, not a multiple of 16 → now asserts `CM_INIT_PARA_ERROR`.
- `api_param_bounds_test::test_encoder_very_large_slices` leaves `iTargetBitrate`
  at 0 under the default `RC_QUALITY_MODE` → upstream returns `cmInitParaError`.
  `#[ignore]`d with the reason, because the port reaches the Phase-3.9 `todo!()`
  before it reaches the bitrate check.
- `loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity` likewise never set
  a bitrate or per-layer resolution; those are now filled in as a real caller must.

#### Test status at Gate 1

`cargo test`: 233 passed, 1 failed, 21 ignored. The single failure is
`test_decode_encode_full_cycle_sha1_parity`, unchanged from the baseline and still
for the same reason — the encoder emits zero bytes. That is the Phase 4 gate.
The 53 decoder conformance tests all still pass.

### Phase 2 — unify the encoder-internal types — **DONE**

**Gate 2 met.** `rust/tools/find_dup_types.sh` prints nothing; `src/encoder/`
contains zero `mem::transmute`; build and tests pass.

| metric | before | after |
|---|---|---|
| duplicated type names | 59 | **0** |
| redundant declarations | 146 | **0** |
| `transmute` in `src/encoder/` | 17 | **0** |
| encoder size assertions | 0 | **55** |

The audit's figures were 58/145; `find_dup_types.sh` also counts non-`pub`
declarations. Two blind spots that any struct-only scan shares, both of which hid
a *wrong* definition: **type aliases** (`SBitStringAux` had four identities, not
two) and **`src/common/`** (`SMcFunc`'s fifth copy there was the correct one).

#### How to verify a type

Compile a `sizeof` dump against the real headers — this found every layout defect
below, and it is faster than reading:

```bash
c++ -std=c++11 -I codec/encoder/core/inc -I codec/common/inc -I codec/api/wels -I codec/processing/interface probe.cpp -o probe && ./probe
```

with `probe.cpp` including the header, `using namespace WelsEnc;` and
`printf("%zu", sizeof(T))`. Record the number in `src/encoder/abi_guard.rs` (the
internal counterpart to `api/abi_guard.rs`), which now pins 55 structs at compile
time. Verified to fire by perturbing an entry.

Two limits worth knowing:

- **Size assertions cannot catch field order.** `SBitStringAux`, `SLayerInfo` and
  `SSliceBufferInfo` were all correctly sized and wrongly ordered. Read the header.
- **Not every struct can be pinned.** `SSliceThreading` embeds `pthread_cond_t`
  and `pthread_mutex_t` by value, so its 1256 bytes are libc-specific; this port
  models those as opaque handles. It is excluded with a comment. `sWelsEncCtx`
  has one such member, so it is asserted at `98008 - 64 + 8` with the derivation
  written out.

#### Conditional compilation is a defect class

Fields the build never compiles, which the port had transcribed anyway:

| macro | defined? | fields wrongly included |
|---|---|---|
| `DISABLE_FMO_FEATURE` | **yes**, `as264_common.h:53`, unconditional | `SWelsPPS`'s nine FMO fields (~9x too large); `SSliceHeader::iSliceGroupChangeCycle` |
| `ENABLE_FRAME_DUMP` | **no** — only under `WELS_TESTBED` / `__UNITTEST__` | `SSpatialLayerInternal::sRecFileName` |
| `_DEBUG` | **no** | `SParaSetOffset::eSpsPpsIdStrategy` |
| `STAT_OUTPUT` | **no** (commented out) | `sWelsEncCtx::sStatData`, `::sPerInfo` |

**Check for `#if`/`#ifdef` around every field before porting a struct**, and read
the guard carefully: `wels_const.h:41-42` defines
`NUM_SPATIAL_LAYERS_CONSTRAINT` and `NUM_QUALITY_LAYERS_CONSTRAINT` a few lines
*above* their use, so a grep that excludes the defining file makes
`MAX_DEPENDENCY_LAYER` and `MAX_QUALITY_LEVEL` look wrong when they are correct.

#### Definitions that were wrong, not merely duplicated

- **`SWelsSvcCodingParam`** — C++ derives it from `SEncParamExt`
  (`param_svc.h:106`), so the 924-byte base must be a byte-identical prefix. All
  42 fields were present and correctly typed but **out of order** from
  `bEnableFrameCroppingFlag` on, changing the padding. Now 1240 bytes.
- **`SStateCtx`** — one packed byte in C++ (`set_mb_syn_cabac.h:55`); the port had
  two `u8` fields, doubling every entry of `sWelsCabacContexts[4][52][460]`.
- **`SWelsFuncPtrList`** — 13 of 70 members. Ported in full into the new
  `wels_func_ptr_def.rs` with the fifteen missing function-pointer typedefs.
- **`SParaSetOffset`** — a `[i32; 32]` placeholder for a 1180-byte struct.
- **`SPicture`** — `[_; 4]` arrays where C++ says 3, an invented `iPOC`, ten
  fields missing.
- **`SExpandPicFunc`** — C++ has a two-entry chroma table indexed by 16-alignment,
  not a scalar. The decoder's copy was right, so the canonical is now
  `common/expand_pic.rs`; `ExpandReferencingPicture` was rewritten to match
  `expand_pic.cpp:388`.
- **`SMbCache`** — C++ ends with an anonymous member named `SPicData`, and its
  first three members use `ALIGNED_DECLARE(..., 16)`, expressed here as
  `#[repr(C, align(16))]` plus the 14 bytes the C++ compiler inserts.
- **`ESceneChangeIdc`** — the port invented `NO_SCENE_CHANGE=0`, shifting
  `SIMILAR_SCENE` to 1 and dropping `MEDIUM_CHANGED_SCENE`.
- **`SSampleDealingFunc`** — `[_; 8]` where C++ says `[MAX_BLOCK_TYPE]` = 7.
- **`CWelsPreProcess`** was modelled twice, once as a hand-rolled vtable whose
  `GetRefFrameInfo` entry had no implementation behind it at all.
- **`SPps`** was an invented encoder type (C++ has `SWelsPPS`); it and its sole
  consumer `SDqLayerInfo` were dead code.

#### `pub const` shadowing an enum variant hides logic bugs

`svc_set_mb_syn_cavlc.rs` declared `I_SLICE: i32 = 0` / `P_SLICE: i32 = 1`,
shadowing `EWelsSliceType`. C++ (`wels_common_defs.h:163`) has `P_SLICE = 0`,
`I_SLICE = 2`, so the mb-type offset switch at `svc_set_mb_syn_cavlc.cpp:76` was
selecting the I-slice offset for P slices and returning early for real I slices.
`svc_encode_slice.rs` had the same pattern with `NAL_UNIT_CODED_SLICE_EXT`.
The same disease exists at the **type-alias** level: three different
`PSampleSadSatdCostFunc` aliases, one of which took `*const u8` where
`wels_func_ptr_def.h:127` says `uint8_t*`, making it a distinct function type.

#### Five constants had wrong values

| constant | was | C++ | source |
|---|---|---|---|
| `MAX_SHORT_REF_COUNT` | 16, in three modules | `MAX_GOP_SIZE>>1` = **4** | `wels_const.h:115` |
| `MAX_TEMPORAL_LEVEL` | 8, in `ref_list_mgr_svc.rs` | **4** | `wels_const.h:102` |
| `BLOCK_SIZE_ALL` | 8 | **7** | `wels_const.h:147` |
| `MAX_THREADS_NUM` | 8, in `svc_encode_slice.rs` | **4** | `wels_const.h:69` |
| `BLOCK_STATIC_IDC_ALL` | 5 | **3** | `IWelsVP.h:148` |

The stale `MAX_SHORT_REF_COUNT` also let the `WelsPreprocess` unref loop read one
element past `pShortRefList`. Note the **decoder** legitimately defines its own
`MAX_SHORT_REF_COUNT = 16` (`codec/decoder/core/inc/wels_const.h:47`) — encoder
and decoder really do disagree. Do not "unify" them.

Constants remain a second duplication axis: **79** names are still declared in
more than one encoder module. The values are now believed correct, but one
definition each is still the goal:

```bash
grep -h "^pub const [A-Z_0-9]*:" rust/crates/openh264-rs/src/encoder/*.rs | sed -E 's/.*const ([A-Z_0-9]+):.*/\1/' | sort | uniq -c | awk '$1>1'
```

#### Removing the transmutes surfaced three hidden defects

- `pfInterMd` was assigned `WelsMdSpatialelInterMbIlfmdNoilp`, whose last
  parameter is `Mb_Type`, not `SMbCache*`. C++ never sets `pfInterMd` in
  `InitFunctionPointers`; it assigns `WelsMdInterMb` /
  `WelsMdInterMbEnhancelayer` per-slice at `svc_encode_slice.cpp:733/736`.
  `WelsMdInterMb` is **not ported** — that is Phase 3 work. The unit test that
  asserted `pfInterMd.is_some()` was corrected; it asserted something the C++
  reference does not do.
- `WelsSpatialWriteMbSyn` was a plain Rust `fn`, not `extern "C"`.
- The third `PSampleSadSatdCostFunc` alias above.

The nine `ISVCEncoderVtbl` transmutes turned out to be unnecessary once the
surrounding types were right.

#### Open items carried into Phase 3/4

- **`IWelsReferenceStrategy` dispatch.** `sWelsEncCtx` holds a plain 8-byte
  `IWelsReferenceStrategy*`; the copy in `ref_list_mgr_svc.rs` had a Rust
  `*mut dyn` fat pointer (16 bytes), which mis-sized the struct. Restoring the
  correct width leaves no dispatch mechanism for the two `EndofUpdateRefList`
  call sites, which are now explicit `todo!()`s rather than silent no-ops. Needs
  a C-style vtable on the three concrete strategies, or for `pReferenceStrategy`
  to be wired in Phase 4.
- **`WelsMdInterMb`** is unported (see `pfInterMd` above).
- `param_svc.rs::ParamBaseTranscode` writes `sSpatialLayers[idx]` where C++
  writes `sSpatialLayers[0]`.
- The two Phase-3.9 `todo!()`s in `ParamValidationExt`
  (`SM_FIXEDSLCNUM_SLICE`, `SM_RASTER_SLICE`).

#### Tooling

| tool | purpose |
|---|---|
| `rust/tools/find_dup_types.sh` | the Gate 2 check — silence means met |
| `rust/tools/show_type.sh <T> [CppTag]` | dumps the C++ original beside every Rust copy |
| `src/encoder/abi_guard.rs` | 55 compile-time size assertions |

`unify_type.py` had two insertion bugs, both fixed: it matched `use` only to the
first newline (landing inside a braced `use crate::{...}` group), and it fell
back to position 0 in modules with no top-level `use`, landing above the inner
attributes.

#### Test status at Gate 2

`cargo test`: **235 passed, 1 failed, 21 ignored**. The failure is
`loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity` — the encoder
still emits zero bytes, which is the Phase 4 gate. The 53 decoder conformance
tests all still pass. `compare.sh` still reports C++ 8034 bytes vs Rust 0, which
is expected: Phase 4 is what wires the pipeline.

### Phase 3 onward — **NOT STARTED**
