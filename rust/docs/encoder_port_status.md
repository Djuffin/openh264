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

`codec_unittest` baseline on this machine (darwin/arm64): **533 pass, 1 fail**
(re-measured at Gate 3). The failure is `DecoderDeblocking.DeblockingInit`,
pre-existing and unrelated to the encoder port.

### Linking the C++ function directly is a better oracle than `codec_unittest`

`codec_unittest` exercises the C++, not the port, so it can only tell you what the
reference *should* do — you still have to transcribe the expectation by hand. For any
non-static encoder function you can do better: **link `libopenh264.a` into a throwaway
probe, call the C++ function, and paste its output into a Rust test.** That turns a
reading exercise into a measurement.

```bash
c++ -std=c++11 -I codec/api/wels -I codec/common/inc -I codec/encoder/core/inc \
    -I codec/processing/interface probe.cpp libopenh264.a -o probe && ./probe
```

with `probe.cpp` redeclaring the function inside `namespace WelsEnc { ... }` — the
headers are not always self-contained, and a forward declaration links fine. Static
functions are not reachable this way; everything in `au_set.cpp` and
`svc_enc_slice_segment.cpp` that Phase 3 needed was.

This validated every Phase 3 expectation, and proved the parameter-set writers are
already byte-exact — a Phase 5 result reached during Phase 3. **Use it in Phase 4/5
before hand-deriving any expected value.**

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

- **A.** *(fixed, Phase 1)* Public API structs declared twice — in `lib.rs` *and*
  `api/codec_api.rs` — with incompatible layouts. A local item beats a glob
  re-export, so `crate::SFrameBSInfo` resolved to the 3104-byte `lib.rs` version
  while callers passed the correct 7192-byte one. The C-ABI shims cast between
  them, so every encoder write landed at the wrong offset.
- **B.** *(fixed, Phase 2)* 63 encoder-internal types declared in multiple modules
  with divergent field sets (`SDqLayer` ×11, `SWelsFuncPtrList` ×10,
  `sWelsEncCtx` ×9, …), making each module a compile-clean island.
- **C.** *(fixed, Phase 4)* The context was never built — `pSpsArray`/`pPPSArray`/
  `iSpsNum`/`iPpsNum` never assigned, `pCurDqLayer` null. Phase 3 stopped
  `WelsWriteParameterSets` from *hiding* this; Phase 4's `RequestMemorySvc` /
  `InitDqLayers` build it, pinned by two tests.
- **D.** *(open — blocked)* `WelsEncoderEncodeExtRust` is a sketch: hardcoded IDR,
  one slice buffer, no frame-type/GOP decision, RC, ref lists, preprocessing, or
  padding. Phase 4 identified the specific obstacle — the duplicate
  `SWelsEncCtx` in `wels_preprocess.rs`; see Phase 4 below.

**D is why the encoder still emits zero bytes.**

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
  `MAX_REFERENCE_PICTURE_COUNT_NUM_CAMERA` was 16 instead of 6. Corrected
  **in `wels_encoder_ext.rs` only** — see the correction below.
- `MAX_GOP_SIZE` = 64 fixed to `1 << (MAX_TEMPORAL_LEVEL - 1)` = 8 (audit 1.5.4).

> **Correction (Phase 2).** "Corrected" above held for exactly one of four copies
> of `MAX_SHORT_REF_COUNT`. `encoder_context.rs`, `ref_list_mgr_svc.rs` and
> `wels_preprocess.rs` each kept their own `= 16` until Phase 2. The lesson
> generalises: a constant fixed in the module you were reading is not fixed while
> the same name is declared in seven others. Check with the `grep` under *Five
> constants had wrong values* before trusting any such claim in this file.

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

Constants remain a second duplication axis: **82** names are still declared in
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
| `rust/tools/find_dup_types.sh` | the Gate 2 check — silence means met (types only; see Phase 6) |
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

### Phase 3 — port the remaining per-function modules — **3.7 and 3.9 DONE**

**Gate 3 met.** Everything measured, not asserted:

| check | result |
|---|---|
| `cargo build` | clean, 10 warnings, all pre-existing `dead_code`/`unused` |
| `cargo test` | **260 passed, 1 failed, 20 ignored** (was 235/1/21) |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `find_dup_types.sh` | silent |
| `todo!()` in `src/` | 4 → **2** |
| `compare.sh` | C++ 8034, Rust 0 — unchanged; Phase 4 wires the pipeline |

The one failure is still
`loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity`, the Phase 4 gate.

#### 3.7 — `au_set.rs` complete — **DONE**

Added `WelsCheckLevelLimitation`, `WelsGetLevelIdc`, `WelsGetPaddingOffset`,
`WelsInitSps`, `WelsInitSubsetSps`, `WelsInitPps`, `WelsWriteVUI`,
`WelsWriteSpsSyntax`, `WelsWriteSpsNal`, `WelsWriteSubsetSpsSyntax` and
`WelsWritePpsSyntax`. The ad-hoc writers in `wels_encoder_ext.rs` are deleted.

**The writers are byte-exact with C++**, verified by the linking technique above.
For the harness configuration (160x96, 6fps, baseline, one layer, one ref frame):

```
SPS NAL  42 c0 0d 8c 68 28 d2 01 e1 10 8d 40   (12 bytes)
PPS RBSP ce 3c 80                              (3 bytes)
```

Both are pinned in `au_set.rs`'s test module. The deleted writer produced 8 bytes for
the same SPS — audit defect 1.5.3, the missing VUI. It also dropped both parameter-set
id offsets and had no `WelsWriteSubsetSpsSyntax` at all.

#### 3.8 — `paraset_strategy.rs` — **NEW, pulled in by 3.7**

`WelsWritePpsSyntax` takes an `IWelsParametersetStrategy*`, so the class had to exist.
It is modelled as a **C-style vtable**: a thin pointer to an object whose first word
points at a static `IWelsParametersetStrategyVtbl`, listing the 20 methods in
`paraset_strategy.h` declaration order. A Rust `*mut dyn` would be a 16-byte fat
pointer and mis-size `SWelsFuncPtrList` — the same defect Phase 2 found in
`IWelsReferenceStrategy`. **This is the pattern to reuse for the two remaining
`todo!()`s.**

Two of five strategies are ported: `CWelsParametersetIdConstant` (the Phase-5 gate
configuration) and `CWelsParametersetIdIncreasing` (the `FillDefault` value, so the
default API path depends on it). Because Rust has no implementation inheritance,
`ID_INCREASING_VTBL` reuses the `ConstId_*` thunks for the members the subclass does
not override — which is exactly what the C++ vtable does.

`CreateParametersetStrategy` returns **null** for the three listing strategies rather
than following C++'s `default:` fall-through to `CONSTANT_ID`, which would silently
encode the wrong parameter-set ids. `InitFunctionPointers` (encoder.cpp:227) turns
null into `ENC_RETURN_MEMALLOCERR`.

#### 3.9 — `svc_enc_slice_segment.rs` — **DONE**

`CheckFixedSliceNumMultiSliceSetting`, `CheckRowMbMultiSliceSetting`,
`CheckRasterMultiSliceSetting`, `GomValidCheckSliceNum`, `GomValidCheckSliceMbNum` and
`SliceArgumentValidationFixedSliceMode`. All nine unit-test expectations were taken
from the linked C++ functions, not derived by hand.

One correction to the earlier plan: `SliceArgumentValidationFixedSliceMode` is **not**
in `svc_enc_slice_segment.cpp`. It is defined at `encoder_ext.cpp:174` and only
declared in `svc_enc_slice_segment.h`.

The rest of `svc_enc_slice_segment.cpp` — `InitSliceSegment`, `AssignMbMap*`,
`GetInitialSliceNum`, `InitSlicePEncCtx`, `WelsGetFirstMbOfSlice`,
`WelsGetPrevMbOfSlice`, `WelsGetNumMbInSlice`, `DynamicMaxSliceNumConstraint` — drives
`SSliceCtx::pOverallMbMap` and belongs with Phase 4's context construction.

`api_param_bounds_test::test_encoder_very_large_slices` is un-`#[ignore]`d and asserts
`CM_INIT_PARA_ERROR`. Re-verified against `libopenh264.a` rather than taken on trust:
that configuration leaves `iTargetBitrate` at 0 under `RC_QUALITY_MODE` and upstream
returns `cmInitParaError` (1).

#### Defects found during Phase 3

- **`g_ksLevelLimits` had three columns from the wrong source.** The table in
  `decoder/nalu.rs` transcribed the H.264 spec's Table A-1 instead of
  `common/src/common_tables.cpp:345`: `uiMaxDPBMbs` held `MaxDPB` in 1024-byte units
  rather than `MaxDpbMbs` in macroblocks (891 vs 2376 at level 1.2), `iMinVmv`/`iMaxVmv`
  held luma samples rather than quarter-pel (−64/63 vs −256/255 at level 1.0), and
  `iMaxMvsPer2Mb` was −1 for all 17 rows rather than `0x7fff`/32/16. Every wrong value
  was *narrower* than the real limit, so both the decoder's MV-range check and the
  encoder's `WelsCheckRefFrameLimitationLevelIdcFirst` were over-strict. This table is
  shared with the decoder; conformance re-verified at 53/53 after the fix.
- **The slice-header writers dropped the PPS id offset.** `WelsSliceHeaderWrite` and
  `WelsSliceHeaderExtWrite` wrote `pic_parameter_set_id` as the bare `iPpsId`, where
  `svc_encode_slice.cpp:285/:361` add `pParametersetStrategy->GetPpsIdOffset(...)`.
  Zero under `CONSTANT_ID`, wrong under the default `INCREASING_ID`.
- **`WelsWriteParameterSets` masked blocker C.** It substituted a count of 1 when
  `iSpsNum`/`iPpsNum` were zero and discarded each writer's return value, so an
  unbuilt context returned success. It is now the real `encoder_ext.cpp:2874` flow —
  three loops bounded by the real counts, plus the subset-SPS pass that was missing
  entirely — and returns `ENC_RETURN_UNEXPECTED` when the strategy is null.

#### Still open, carried into Phase 4

- **`WelsMdInterMb`** is unported. C++ assigns it (or
  `WelsMdInterMbEnhancelayer`, which *is* ported) to `pfInterMd` per-slice at
  `svc_encode_slice.cpp:733/736`.
- **`IWelsReferenceStrategy` dispatch** — the two remaining `todo!()`s, at the
  `EndofUpdateRefList` call sites in `ref_list_mgr_svc.rs`. Use the C-style vtable
  from `paraset_strategy.rs`; the shape is now established.
- `param_svc.rs::ParamBaseTranscode` writes `sSpatialLayers[idx]` where C++
  writes `sSpatialLayers[0]`. Harmless at one spatial layer, wrong beyond it.
- The three unported listing strategies in `paraset_strategy.rs`.

### Phase 4 — wire the pipeline — **blocker C DONE, blocker D BLOCKED**

**Gate 4 is NOT met.** `compare.sh` still reports Rust 0 bytes. What changed is
that blocker **C** is cleared and blocker **D** now has one identified, specific
obstacle rather than being open-ended.

| check | result |
|---|---|
| `cargo build` | clean, 12 warnings, all pre-existing |
| `cargo test` | **262 passed, 1 failed, 20 ignored** (was 260/1/20) |
| decoder conformance | 53/53 |
| `find_dup_types.sh` | silent |
| `todo!()` in `src/` | 2 → **0** |
| `compare.sh` | C++ 8034, **Rust 0** — Gate 4 not met |

#### Blocker C — **DONE**

New `encoder_ext.rs` ports the allocation half of `encoder_ext.cpp`:
`WelsGetEncBlockStrideOffset`, `AcquireLayersNals`, `AllocStrideTables`,
`GetMvMvdRange`, `InitMbInfo`, `InitMbListD`, `InitDqLayers`, `RequestMemorySvc`,
`InitSliceSettings`, `GetMultipleThreadIdc`, `WelsInitEncoderExt`,
`WelsUninitEncoderExt`, `FreeSliceInLayer`, `FreeDqLayer`, `FreeRefList`.
`svc_enc_slice_segment.rs` gains the segment-allocation group
(`AssignMbMap*`, `GetInitialSliceNum`, `InitSliceSegment`, `InitSlicePEncCtx`, …).

Two tests in `encoder_ext.rs` measure it for the harness configuration: the
parameter-set arrays are allocated and populated (`iSpsNum` 1, `iPpsNum` 1, SPS
matching the Phase-3 byte-exact values), and the DQ layers, MB map, `InitMbInfo`
neighbour masks, reference lists, stride tables, MVD cost table and reference
strategy all exist.

**`CMemoryAlign`'s `Drop` asserts a zero allocation balance, and that is a good
oracle** — it caught an 11181-byte leak the moment the teardown was incomplete.
Keep using it: any new allocation in this module gets checked for free.

#### `IWelsReferenceStrategy` — **DONE**

Converted from a Rust `trait` + `*mut dyn` to the same C-style vtable
`paraset_strategy.rs` uses. `CreateReferenceStrategy` had been returning a
16-byte fat pointer for an 8-byte `sWelsEncCtx::pReferenceStrategy`. Both
`EndofUpdateRefList` call sites now dispatch through the vtable, so **the tree
has zero `todo!()`**.

#### Blocker D — **BLOCKED on a duplicate context struct**

`WelsEncoderEncodeExt` is unported. It cannot be wired yet because:

> **`wels_preprocess.rs:575` declares its own `SWelsEncCtx`** — a 20-field struct
> that is *not* the canonical ~90-field `sWelsEncCtx` — and then does
> `pub type sWelsEncCtx = SWelsEncCtx;` inside that module, so the lowercase name
> resolves to the fake one there. Field types differ too: `pLtr` and
> `ppRefPicListExt` are inline arrays here and pointers in the real context.

`find_dup_types.sh` missed this because the canonical name is `sWelsEncCtx` and
this one is `SWelsEncCtx` — **different identifiers, differing only in the case of
the leading letter**. This is a fourth blind spot to add to the three already
recorded (type aliases, `src/common/`, functions).

Every field access in the 2592-line preprocessor therefore reads the wrong
offsets when handed a real context, and `WelsEncoderEncodeExt` calls
`BuildSpatialPicList`, `AnalyzeSpatialPic` and `UpdateSpatialPictures` with
exactly that. `WelsInitEncoderExt` casts at the boundary with a comment marking
where this lands, which is enough to build and to run the init tests, but it is
**not sound to call through**.

**Do this first in the next session:** delete `wels_preprocess::SWelsEncCtx` and
its `sWelsEncCtx` alias, import the canonical type, and fix the field accesses
the layout change breaks. Verify with a `sizeof` probe on the C++ `sWelsEncCtx`
and the existing `abi_guard.rs` assertion.

#### Then the rest of blocker D

Replace the `WelsEncoderEncodeExtRust` sketch with `encoder_ext.cpp:3448`. Still
unported and needed by it: `PrepareEncodeFrame`, `WelsInitCurrentLayer`,
`PreprocessSliceCoding`, `PrefetchReferencePicture`, `GetSubSequenceId`,
`AddPrefixNal`, `WritePadding`, `WelsSwapDqLayers`, `ClearFrameBsInfo`,
`StackBackEncoderStatus`, `GetTemporalLevel`. `WelsUpdateRefSyntax`,
`InitFrameCoding`, `InitBitStream`, `GetTimestampForRc`, `SetSliceBoundaryInfo`,
`WelsCodeOneSlice`, `WelsLoadNal`/`WelsUnloadNal`/`WelsEncodeNal` and
`PerformDeblockingFilter` already exist.

The C-ABI shim still calls `WelsInitEncoderExtRust`/`WelsEncoderEncodeExtRust`;
switching it to `WelsInitEncoderExt` is safe only once the preprocessor context
is unified.

**Gate 4:** `compare.sh` reports a non-zero Rust byte count and `./h264dec`
decodes the Rust stream. `loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity`
stops returning `da39a3ee…` (the SHA-1 of the empty string).

#### Defects found during Phase 4

All the same disease — a constant or struct corrected in one copy while others
kept the wrong value:

| item | was | C++ | why it matters |
|---|---|---|---|
| `INTRA_4x4_MODE_NUM` | 16 | **8** (`wels_const.h:48`) | per-MB stride of `pIntra4x4PredModeBlocks`; `md.rs:563` stepped back two MBs |
| `SDqIdc` | `{uiDId,uiQId,uiTId}` | `{u16 iPpsId, u8 iSpsId, i8 uiSpatialId}` (`dq_map.h:50`) | `InitDqLayers` writes the C++ fields |
| `ENC_RETURN_MEMALLOCERR` | 0x02 / 2 in three modules | **0x01** | it is a **bit field**, so wrong values alias other codes |
| `ENC_RETURN_INVALIDINPUT` | 1 | **0x10** | same |
| `ENC_RETURN_UNEXPECTED` | −1 | **0x04** | same |
| `ENC_RETURN_VLCOVERFLOWFOUND` | −1 | **0x40** | same |
| `ENC_RETURN_CORRECTING` | 1 | *not in the C++ enum* | invented; removed |
| `deblocking.rs` `LEFT_MB_POS`/`TOP_MB_POS` | 0x02/0x01 | **0x01/0x02** | dead in both languages, but wrong |

### Phase 5 — byte-exactness — **NOT STARTED**

Drive `compare.sh` until the Annex-B streams are identical, then widen beyond the
gate configuration (single spatial layer, single slice, CAVLC, RC off,
single-threaded, deblocking on, `CONSTANT_ID`, no LTR/denoise/AQ/BGD/scene-change
— see `cxx_enc.cpp`).

**Gate 5:** `compare.sh` exits 0.

The parameter sets are **already byte-exact** as of Phase 3.7 (see the SPS/PPS hex
above), so a first divergence is most likely in the slice layer, not the headers.

Three things to keep in mind when a stream diverges:

- Size assertions cannot catch **field order**; three structs were correctly
  sized and wrongly ordered.
- `#if`/`#ifdef` around a field is a live hazard — four macros in this codebase
  exclude fields the port had transcribed. See *Conditional compilation is a
  defect class*.
- Before hand-deriving an expected value, **link the C++ function and measure it**.
  See *Linking the C++ function directly is a better oracle than `codec_unittest`*.

### Phase 6 — cleanup — **NOT STARTED**

Collapse the remaining **82** duplicated constant names (values are now believed
correct, but one definition each is still the goal), fold the module-level
`#![allow(dead_code, unused_variables, …)]` blankets back to the narrowest scope
that still compiles, and reconcile this document with the final state.

#### `find_dup_types.sh` has a fourth blind spot: case

Phase 4 found `wels_preprocess::SWelsEncCtx` shadowing the canonical
`sWelsEncCtx` — see *Blocker D*. The scan compares identifiers exactly, so two
names differing only in the case of one letter read as unrelated types. Fold a
case-insensitive pass in alongside the `pub fn` pass below.

#### `find_dup_types.sh` has a third blind spot: functions

Phase 2 recorded two blind spots in any struct-only scan (type aliases, and
`src/common/`). Phase 3 found a third — the tool checks *types*, so **duplicated
function definitions pass silently**. Measured during Phase 3:

| function | copies | modules |
|---|---|---|
| `BsWriteBits` | 4 | `nal_encap`, `svc_encode_slice`, `vlc_encoder`, `svc_set_mb_syn_cavlc` |
| `BsWriteUE`, `BsWriteSE`, `BsWriteOneBit` | 3–4 | same set |
| `BsRbspTrailingBits` | 2 | `nal_encap`, `vlc_encoder` |
| `GetCurrentSliceNum` | 3 | `svc_encode_slice`, `deblocking`, `svc_motion_estimate` |
| `WelsGetNextMbOfSlice` | 2 | `svc_encode_slice`, `deblocking` |

All the `Bs*` copies were compared line by line against `bit_stream.h` /
`svc_enc_golomb.h` and are **behaviourally equivalent for well-formed calls**, so this
is a tidiness problem rather than a live bug — but it is the same shape as the defect
class Phase 2 spent its time on, and the same scan will not catch the next one. Extend
`find_dup_types.sh` to cover `pub fn`/`pub unsafe fn` before relying on it again.

`au_set.rs` uses `vlc_encoder`'s copies, which are the closest transcription of the
C++ (they call `WRITE_BE_32` exactly as `bit_stream.h` does).
