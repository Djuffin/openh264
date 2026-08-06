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
(re-measured at Gate 4). The failure is `DecoderDeblocking.DeblockingInit`,
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
- **D.** *(fixed, Phase 4)* `WelsEncoderEncodeExtRust` was a sketch: hardcoded IDR,
  one slice buffer, no frame-type/GOP decision, RC, ref lists, preprocessing, or
  padding. It is replaced by the real `encoder_ext.cpp:3448` flow, and the
  duplicate `SWelsEncCtx` in `wels_preprocess.rs` that blocked it is unified.
- **E.** *(half fixed, Phase 4.5)* The **mode-decision layer was unported** — 22 of
  the 32 functions in `svc_base_layer_md.cpp` — and so were three whole files it
  depends on. The **intra half is now ported and byte-exact**; the inter half is
  not. See *Phase 4.5* below.

**The encoder emits a byte-identical IDR access unit (3295 bytes) that `h264dec`
decodes to a full 23040-byte YUV frame. Over five frames it emits 17907 bytes where
C++ emits 8034, because the four P frames still have no mode decision. Closing E's
inter half is what remains.**

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
  > **Correction (Phase 4).** This entry understated the problem by a wide margin.
  > `WelsMdInterMb` is one of **22** unported functions in `svc_base_layer_md.cpp`;
  > the whole mode-decision layer is absent, intra as well as inter. See *The
  > mode-decision layer is missing* under Phase 4.
- **`IWelsReferenceStrategy` dispatch** — the two remaining `todo!()`s, at the
  `EndofUpdateRefList` call sites in `ref_list_mgr_svc.rs`. Use the C-style vtable
  from `paraset_strategy.rs`; the shape is now established.
- `param_svc.rs::ParamBaseTranscode` writes `sSpatialLayers[idx]` where C++
  writes `sSpatialLayers[0]`. Harmless at one spatial layer, wrong beyond it.
- The three unported listing strategies in `paraset_strategy.rs`.

### Phase 4 — wire the pipeline — **DONE, with the mode-decision layer carried**

**Gate 4 was partly met at the end of Phase 4; Phase 4.5 met the rest.** The table
below records the state as of Phase 4 — see Phase 4.5 for the current numbers.

| Gate 4 criterion | at end of Phase 4 | now |
|---|---|---|
| `compare.sh` reports a non-zero Rust byte count | met — 136 bytes, was 0 | 3295, byte-identical for one frame |
| the SHA-1 test stops returning `da39a3ee…` | met — `96a32e4b…` | `abb12b2a…`, still not the expected hash |
| `./h264dec` decodes the Rust stream | **not met** — 0 bytes YUV | met — 23040 bytes |

| check | result |
|---|---|
| `cargo build` | clean, 16 warnings, all `dead_code`/`unused` |
| `cargo test` | **262 passed, 1 failed, 20 ignored** |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `find_dup_types.sh` | silent on both passes |
| `todo!()` in `src/` | **0** |
| `compare.sh` | C++ 8034, **Rust 136**, 32-byte common prefix |

#### Blocker C — **DONE** (Phase 4, earlier session)

`encoder_ext.rs` ports the allocation half of `encoder_ext.cpp`:
`WelsGetEncBlockStrideOffset`, `AcquireLayersNals`, `AllocStrideTables`,
`GetMvMvdRange`, `InitMbInfo`, `InitMbListD`, `InitDqLayers`, `RequestMemorySvc`,
`InitSliceSettings`, `GetMultipleThreadIdc`, `WelsInitEncoderExt`,
`WelsUninitEncoderExt`, `FreeSliceInLayer`, `FreeDqLayer`, `FreeRefList`.

**`CMemoryAlign`'s `Drop` asserts a zero allocation balance, and that is a good
oracle** — it caught an 11181-byte leak in that session and a 375-byte one in this
one. Any test that builds a context and tears it down is also a leak test.

#### The preprocessor context — **DONE**

`wels_preprocess.rs` declared its own 15-field `SWelsEncCtx` and then
`pub type sWelsEncCtx = SWelsEncCtx;`, so inside that module the lowercase name
resolved to the fake struct and everything compiled. Every field access in the
2592-line preprocessor read the wrong offsets when handed a real context — which is
exactly what `WelsEncoderEncodeExt` passes to `BuildSpatialPicList`,
`AnalyzeSpatialPic` and `UpdateSpatialPictures`.

All 15 field names existed on the canonical struct; four differed in type. `pLtr`
and `ppRefPicListExt` are pointers in C++, not inline arrays, so the three `pLtr[i]`
sites became `pLtr.add(i)`. `eSliceType` is `EWelsSliceType`, not `i32`.
`SSpatialIndexMap` was a byte-identical copy of `encoder_context::SSpatialPicIndex`,
the name C++ uses. The three boundary casts in `WelsInitEncoderExt` /
`WelsUninitEncoderExt` are gone.

**Pinned rather than assumed.** A `sizeof`/`offsetof` probe against
`encoder_context.h` supplied all 15 offsets, now asserted in `abi_guard.rs` and
verified to fire by perturbing one. All 15 fields precede
`WELS_MUTEX mutexEncoderError` (`encoder_context.h:230`), the one member this port
models differently, so each is the unmodified C++ offset:

```
sLogCtx 0   pSvcParam 24   iMvRange 40   ppRefPicListExt 184   pLtr 320
bCurFrameMarkedAsSceneLtr 328   eSliceType 332   uiDependencyId 361
uiTemporalId 362   pWelsSvcRc 368   pVaa 416   pVpp 424
sSpatialIndexMap 520   bRefOfCurTidIsLtr 600   pMemAlign 1824
```

`SetRefMbType` (`wels_preprocess.cpp:811`) and its call site at `:916` were also
ported; the port had silently dropped them, leaving
`SComplexityAnalysisParam::uiRefMbType` unset. Off the gate path (RC is off there,
so the enclosing branch returns early) but wrong beyond it.

#### Blocker D — **DONE**

`WelsEncoderEncodeExt` is the real `encoder_ext.cpp:3448` flow. New in
`encoder_ext.rs`: `GetTemporalLevel`, `GetSubSequenceId`, `WelsSwapDqLayers`,
`PrefetchReferencePicture`, `ClearFrameBsInfo`, `StackBackEncoderStatus`,
`WelsInitCurrentLayer`, `AddPrefixNal`, `WritePadding`, `SetFastCodingFunc`,
`SetNormalCodingFunc`, `SetMeMethod`, `PreprocessSliceCoding`,
`PicPartitionNumDecision`, `WriteSsvcParaset`, `WriteSavcParaset`,
`PrepareEncodeFrame`, `WelsEncoderEncodeExt`. Both C-ABI call sites now use
`WelsInitEncoderExt` / `WelsEncoderEncodeExt` / `WelsUninitEncoderExt`.

What the harness measures on the gate configuration:

```
C++  8034 bytes
Rust  136 bytes
common prefix 32 bytes
```

```
0000  0000 0001 6742 c00d 8c68 28d2 01e1 108d   <- SPS, byte-identical
0010  4000 0000 0168 ce3c 8000 0000 0165 b800   <- PPS + IDR slice NAL header
0020  04 ...  (C++)   vs   09 ...  (Rust)       <- divergence
```

The SPS and PPS NALs are byte-identical **through the real pipeline**, which
confirms Phase 3.7 end to end rather than only in a unit test. The encoder reports
the correct frame types — IDR + 4 P over 5 frames — so the frame-type/GOP decision,
rate control, reference lists and preprocessing all run. Divergence begins three
bytes into the IDR slice NAL, in the macroblock layer.

#### Branches that return an explicit error rather than falling through

* `iMultipleThreadIdc > 1` — every multi-threaded slice path needs `pTaskManage`,
  `InitAllSlicesInThread` and `SliceLayerInfoUpdate`, none ported.
* `SM_SIZELIMITED_SLICE` — needs `WelsCodeOnePicPartition` and
  `WelsInitCurrentDlayerMltslc`.
* The three `SPS_LISTING` strategies in `PrepareEncodeFrame` — `WriteSavcParaset_Listing`
  is unported and `CreateParametersetStrategy` already returns null for them.

All three are `ENC_RETURN_UNSUPPORTED_PARA`, and each is named in the doc comment on
`WelsEncoderEncodeExt`.

### The mode-decision layer is missing — this is what Gate 4 now needs

**The previous status doc understated this.** It recorded only "`WelsMdInterMb` is
unported". In fact **22 of the 32 functions in `svc_base_layer_md.cpp` (2041 lines)
are unported**, and there is no `svc_base_layer_md.rs` at all:

`WelsMdIntraInit`, `WelsMdInterInit`, `WelsMdI4x4`, `WelsMdI4x4Fast`,
`WelsMdIntraChroma`, `WelsMdIntraFinePartition`, `WelsMdIntraFinePartitionVaa`,
`WelsMdIntraMb`, `WelsMdP16x8`, `WelsMdP8x16`, `WelsMdP4x4`, `WelsMdP8x4`,
`WelsMdP4x8`, `WelsMdInterFinePartition`, `WelsMdInterFinePartitionVaa`,
`WelsMdPSkipEnc`, `WelsMdInterMbRefinement`, `WelsMdFirstIntraMode`,
`WelsMdInterMb`, `WelsMdInterDoubleCheckPskip`, `WelsMdInterEncode`,
`WelsMdInterSaveSadAndRefMbType`.

The consequence is concrete and visible in the Rust source: `WelsISliceMdEnc`
(`svc_encode_slice.rs:1147`) goes straight from `pfWelsRcMbInit` to
`pfWelsSpatialWriteMbSyn`, **omitting `WelsMdIntraInit` and `WelsMdIntraMb`**, which
C++ calls at `svc_encode_slice.cpp:562` and `:566`. It writes macroblock syntax for
macroblocks whose mode was never decided and whose residual was never computed.
That is why the stream diverges exactly where it does, and why `h264dec` gets
nothing out of it.

`PreprocessSliceCoding`, `SetFastCodingFunc` and `SetNormalCodingFunc` therefore
cannot assign `pfIntraFineMd`, `pfInterFineMd` or `pfFirstIntraMode`. Those three
lines are commented `// UNPORTED` at the exact point C++ assigns them, rather than
being pointed at a substitute.

#### Sizes, for planning

| function | lines | note |
|---|---|---|
| `WelsMdIntraInit` | 61 | I-slice entry, `svc_base_layer_md.cpp:259` |
| `WelsMdIntraMb` | 7 | :956, calls `WelsMdI16x16` + `WelsMdIntraSecondaryModesEnc` |
| `WelsMdI16x16` | 53 | :365 |
| `WelsMdI4x4` | 129 | :418 |
| `WelsMdI4x4Fast` | 318 | :548 — the `LOW_COMPLEXITY` path the gate config takes |
| `WelsMdIntraChroma` | 65 | :867 |
| `WelsMdIntraFinePartition` / `…Vaa` | 9 / 13 | :932 / :942 |
| `WelsMdInterMb` | 42 | `svc_base_layer_md.cpp:1858` |

Already ported and reusable: `WelsMdIntraSecondaryModesEnc`, `MdIntraAnalysisVaaInfo`,
`WelsIMbChromaEncode`, `WelsEncRecI16x16Y`, `WelsMdInterJudgePskip`,
`WelsMdInterDecidedPskip`, `WelsMdInterSecondaryModesEnc`, `WelsMdP16x16`,
`WelsRecPskip`, `PredictSad`, `PredictSadSkip`.

> **Correction (Phase 4.5).** Two entries in that "already ported and reusable" list
> were **stubs**, and one more was subtly wrong. Do not trust a name's presence in
> this tree as evidence that its body does the work:
>
> - `WelsMdIntraSecondaryModesEnc` set `uiCbp = 0` and `pSadCost[0] = 0` and nothing
>   else — no `pfIntraFineMd`, no `WelsEncRecI16x16Y`, no `WelsMdIntraChroma`, no
>   `WelsIMbChromaEncode`.
> - `WelsMdInterSecondaryModesEnc` still is one: it omits `pfFirstIntraMode`,
>   `pfSetScrollingMv`, `pfInterFineMd`, `WelsMdInterMbRefinement` and
>   `WelsMdInterDoubleCheckPskip`, and inlines a partial `WelsMdInterEncode`.
> - `WelsIMbChromaEncode` dropped both `WelsEncRecUV` calls, so chroma was never
>   quantised and no chroma CBP was ever set.
>
> **Grep the body against the C++ before relying on any "ported" claim.** The audit
> that produced these lists matched on function names.

**Start with the I-slice path** (`WelsMdIntraInit` → `WelsMdIntraMb` → `WelsMdI16x16`
→ `WelsMdIntraChroma`, plus `WelsMdI4x4Fast` for `LOW_COMPLEXITY`). That alone should
make the IDR frame decodable and is the shortest route to the third Gate 4 criterion.

#### Defects found during Phase 4

| item | was | C++ | why it matters |
|---|---|---|---|
| `INTRA_4x4_MODE_NUM` | 16 | **8** (`wels_const.h:48`) | per-MB stride of `pIntra4x4PredModeBlocks` |
| `SDqIdc` | `{uiDId,uiQId,uiTId}` | `{u16 iPpsId, u8 iSpsId, i8 uiSpatialId}` | `InitDqLayers` writes the C++ fields |
| `ENC_RETURN_*` (4 of them) | dense 0..5 / −1 | a **bit field** | wrong values alias other codes |
| `deblocking.rs` `LEFT_MB_POS`/`TOP_MB_POS` | 0x02/0x01 | **0x01/0x02** | dead in both, but wrong |
| `VIDEO_CODING_LAYER` / `NON_VIDEO_CODING_LAYER` | 0 / 1 | **1 / 0** (`codec_app_def.h:200`) | the two were **swapped**, so every `SLayerBSInfo::uiLayerType` was mislabelled — parameter-set layers tagged VCL and slice layers non-VCL |
| `SWelsPps` in `svc_set_mb_syn_cabac.rs` | 1 field, `uiChromaQpIndexOffset: u32` | C++ has no such type | reading it would have returned `SWelsPPS::iSpsId`; dead, deleted |
| `WelsUninitEncoderExt` | never called `WelsRcFreeMemory` | `encoder_ext.cpp:1982` calls it **before** freeing `pWelsSvcRc` | leaked the per-layer `pTemporalOverRc` blocks, 375 bytes |
| the three `*Rust` sketch entry points | freed `CMemoryAlign` memory with `Box`/`Vec::from_raw_parts` | — | heap corruption (SIGTRAP in `libsystem_malloc`) the moment `Initialize` used the real init; all three deleted |

#### A test expectation corrected

`api_lifecycle_test::test_encoder_create_and_destroy_lifecycle` asserted
`CM_RESULT_SUCCESS` from `EncodeFrame` for a 160x120 source picture with **null
`pData`** against a 320x240 encoder. Verified against `libopenh264.a` with the
identical call sequence: upstream returns **5** (`cmUnsupportedData`). The old
expectation passed only because `WelsEncoderEncodeExtRust` validated nothing.

#### `find_dup_types.sh` gained a case-insensitive pass

The fourth blind spot — `SWelsEncCtx` vs `sWelsEncCtx` — is now covered by a second
pass, self-tested by planting a colliding pair and confirming it is reported. It
immediately found a second live instance, the invented `SWelsPps` above.

Remaining blind spots, each of which has hidden a real bug: **type aliases**,
**`src/common/`** (and the decoder tree — the scan only reads `src/encoder/`; there
are currently two `EWelsSliceType` declarations in `src/decoder/`), **functions**, and
**renames of the same layout to a different identifier** (`SSpatialIndexMap` vs
`SSpatialPicIndex`), which no identifier comparison can catch.

### Phase 4.5 — the intra mode-decision path — **DONE, byte-exact**

**The IDR access unit is byte-identical with the C++.** Measured, not asserted:

```
./rust/tools/diffharness/compare.sh res/CiscoVT2people_160x96_6fps.yuv 160 96 1 26 0 -1
  C++  : 3295 bytes
  Rust : 3295 bytes
  RESULT: BYTE-IDENTICAL
  Rust stream decodes to 23040 bytes YUV
```

All three Gate 4 criteria are now met (the third — `h264dec` decodes the Rust stream
— was the one outstanding). Over the five-frame sequence:

```
./rust/tools/diffharness/compare.sh res/CiscoVT2people_160x96_6fps.yuv 160 96 5 26 0 -1
  C++  : 8034 bytes    NALs: SPS 17, PPS 8, IDR 3270, P 893/940/1219/1687
  Rust : 17907 bytes   NALs: SPS 17, PPS 8, IDR 3270, P 3643 x4
  common prefix 3304 bytes (was 32)
  Rust stream decodes to 23040 bytes YUV (1 frame: the IDR)
```

| check | result |
|---|---|
| `cargo build` | clean, 16 warnings, all `dead_code`/`unused` |
| `cargo test --no-fail-fast` | **287 passed, 2 failed, 20 ignored** (was 262/1/20) |
| decoder conformance | 53/53 |
| `codec_unittest` | 533/534, only `DecoderDeblocking.DeblockingInit` |
| `find_dup_types.sh` | silent on both passes |
| `todo!()` in `src/` | **0** |

The two failures: `loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity`,
which encodes P frames and so needs the inter path (hash moved `da39a3ee` →
`96a32e4b` → `abb12b2a`, still not the expected `34fc3aee`); and a **pre-existing**
doctest in `decoder/decoder_core.rs` whose licence header parses as Rust. The
latter was never reached before because `cargo test` stops at the first failing
binary — use `--no-fail-fast` to see the real totals.

#### Three whole files were unported, not just `svc_base_layer_md.cpp`

The Phase-4 plan named `svc_base_layer_md.cpp` as the blocker. It was necessary but
far from sufficient — the first thing the new `WelsMdI16x16` did was unwrap a `None`:

| file | what was missing |
|---|---|
| `get_intra_predictor.cpp` (754 lines) | **all 26 intra predictors** and `WelsInitIntraPredFuncs`. `pfGetLumaI16x16Pred`, `pfGetLumaI4x4Pred` and `pfGetChromaPred` were declared but never filled. Now `encoder/get_intra_predictor.rs`. |
| `sample.cpp` (SATD half) | the seven `WelsSampleSatd*_c` kernels and `WelsInitSampleSadFunc`. Now `encoder/sample.rs`; the SADs already existed in `common/sad_common.rs`. |
| `decode_mb_aux.cpp` (recon half) | `WelsDequant4x4/Four4x4/IHadamard4x4`, `WelsIDctRecI16x16Dc`, `WelsIDctT4RecOnMb`, `WelsInitReconstructionFuncs`. Now `encoder/decode_mb_aux.rs`. |

**`InitFunctionPointers` called almost none of the C++'s init functions.** It set
three `pfSetMemZero*` slots, `WelsInitBGDFunc`, two DCT pointers, four CAVLC
pointers, `DeblockingInit`, `WelsRcInitFuncPointers` and the paraset strategy — and
skipped `WelsInitIntraPredFuncs`, `WelsInitMeFunc`, `WelsInitSampleSadFunc`,
`WelsInitSCDPskipFunc`, `InitIntraAnalysisVaaInfo`, `InitMcFunc`, `InitCoeffFunc`,
`WelsInitEncodingFuncs`, `WelsInitReconstructionFuncs`, `WelsBlockFuncInit` and
`InitFillNeighborCacheInterFunc`. Four of those already existed in the tree, fully
ported, and were simply never called. It now calls all of them.

`InitCoeffFunc` is new and **incomplete for CABAC**: `StashMBStatusCabac`,
`StashPopMBStatusCabac` and `GetBsPosCabac` are unported, so
`InitFunctionPointers` returns `ENC_RETURN_UNSUPPORTED_PARA` for
`iEntropyCodingModeFlag != 0` rather than running with three null pointers.
`WelsWriteSliceEndSyn`'s CABAC branch is an explicit `unimplemented!()` behind that
same gate.

#### The `Combined3` SIMD branches are deliberately not translated

`WelsMdI16x16`, `WelsMdI4x4` and `WelsMdIntraChroma` each open with a branch taken
only when `sSampleDealingFuncs.pfIntra*Combined3` is non-null. Those slots are
assigned exclusively from SIMD kernels in `sample.cpp`, all behind a `uiCpuFlag`
test. **Measured** against `libopenh264.a` on this machine (darwin/arm64):

```
cpu flags = 0x00000000  logic procs = 0
WELS_CPU_NEON = 0x00000004 -> clear
pfIntra16x16Combined3Sad  = 0x0      (and the other four)
```

so the reference takes the scalar branch; this port has no SIMD and never assigns
them either. Each of the three sites calls `assert_no_combined3`, which panics with
an explicit message rather than silently taking the wrong branch. **This measurement
also confirms the `cpp-dylib-runs-scalar-no-simd` note applies to `libopenh264.a`,
not just the dylib.**

#### Seven latent defects, each found by measurement

Every one was found by bisecting the bitstream against an instrumented C++ build,
not by reading. The technique that worked, in order of the questions it answered:

1. dump per-MB `uiMbType`/`uiCbp`/mode/cost/`pNonZeroCount` from both encoders and
   `diff` — separates mode decision from the writer;
2. dump the per-4x4-block `iBestMode`/`iBestCost` and the neighbour samples the
   predictor reads — separates the search from the reconstruction feeding it;
3. dump `BsGetBitsPos` after each syntax stage — localises a bit-count difference to
   one syntax element.

| defect | detail |
|---|---|
| `wels_preprocess.rs::AllocPicture` | Hand-rolled and wrong in four ways: **no `PADDING_LENGTH` border** (`pData[0] == pBuffer`, so every intra predictor on the top macroblock row read *before* the allocation); `iLineSize[0] = WELS_ALIGN(w,32)` = 160 instead of `WELS_ALIGN(WELS_ALIGN(w,16) + 2*PADDING_LENGTH, 32)` = 224, which **disagreed with the stride `pStrideDecBlockOffset` was built from**, so 4x4 reconstructions landed in the wrong rows; no `uiRefMbType`/`pRefMbQp`/`sMvList`/`pMbSkipSad`; and Rust's global allocator where the caller frees through `CMemoryAlign`. Replaced with the real `picture_handle.cpp:51` body. This is the picture that supplies `pDecPic`. |
| `WelsIMbChromaEncode` | Omitted **both** `WelsEncRecUV` calls and reordered the rest, so `pCurRS` reached the IDCT holding raw DCT coefficients, `uiCbp` never got its chroma bits and `pNonZeroCount[16..24]` stayed zero. |
| `WelsMdIntraSecondaryModesEnc` | A stub; see the correction above. |
| `WelsWriteSliceEndSyn` | **Absent**, and with it `BsRbspTrailingBits` + `BsFlush`. Every slice lost its final byte. |
| `WelsSliceHeaderExtInit` | Dropped `iFrameNum` and `uiIdrPicId` (`svc_encode_slice.cpp:97-98`). The IDR header wrote `ue(0)` (1 bit) where C++ writes `ue(1)` (3 bits) — a 2-bit shift of the entire slice payload, which is why the divergence had always been at byte 0x20. |
| `WriteBlockResidualCavlc` | Indexed the run-before VLC with an invented `(zeros_left.min(7) - 1).max(0)` instead of `g_kuiZeroLeftMap[zeros_left]` — **one row off for every block with a run to code**. The correct table already existed in `vlc_encoder.rs`, unused. |
| `WelsMdI16x16` | Read `pMemPredLuma` where `svc_base_layer_md.cpp:369` reads `pMemPredMb`, and hardcoded `pfSampleSad` where the C++ costs with `pfMdCost` — which `SetNormalCodingFunc` points at `pfSampleSatd`. Harmless in fast mode, wrong in normal mode. |

#### Four more constants had wrong values

The pattern from Phases 1-4 held again. All four were **live**:

| constant | was | C++ | consequence |
|---|---|---|---|
| `md.rs` `MB_TYPE_*` (all eight) | dense `0x01..0x80` ladder from `MB_TYPE_SKIP` | `INTRA4x4` 0x01 … `SKIP` 0x100 (`wels_common_defs.h:275`) | `FillNeighborCacheIntra` tested `uiMbType & MB_TYPE_INTRA4x4` as 0x40, so neighbour I4x4 modes were never inherited; `FillNeighborCacheInter*` compared against the wrong `SKIP` |
| `md.rs` `TOPLEFT_MB_POS` / `TOPRIGHT_MB_POS` | 0x04 / 0x08 | **0x04 is TOPRIGHT, 0x08 is TOPLEFT** (`wels_common_basis.h:125`) | swapped at 12 sites, including the `uiNeighborIntra` bits that index `g_kiIntra16AvaliMode` and `g_kiNeighborIntraToI4x4` |
| `svc_encode_slice.rs` `MB_TYPE_INTRA_BL` | 0x04 | 0x400 (0x04 is `MB_TYPE_INTRA8x8`) | live in `IS_SVC_INTER` |
| `svc_encode_slice.rs` `MB_TYPE_SKIP` | 0x80 | 0x100 (0x80 is `MB_TYPE_8x8_REF0`) | live in `IS_SKIP` |

Note the `uiNeighborIntra` trap: `mb_cache.h:118` documents `TOPLEFT 0x04,
TOPRIGHT 0x08` while `svc_enc_macroblock.h:58` documents `uiNeighborAvail` as
`TOPRIGHT 0x04, TOPLEFT 0x08`. Both are correct — `FillNeighborCacheIntra`
deliberately *reverses* the two bits when it re-encodes availability. Do not
"reconcile" them.

#### Two more duplicate-alias instances

`common/sad_common.rs` declared a **fifth** `PSampleSadSatdCostFunc`, with
`*const u8` where `wels_func_ptr_def.h:127` says `uint8_t*` — a distinct function
type from the one the `SSampleDealingFunc` tables hold — and a second
`PSample4SadCostFunc`. Its seven SAD kernels carried the same wrong constness. Both
aliases now re-export the canonical declarations. `svc_mode_decision.rs` also had a
dead `SSampleDealingFuncs` (note the trailing `s`), a rename of the C++
`SSampleDealingFunc` — the *fifth* blind spot recorded under Phase 6, found here for
the second time. `svc_encode_mb.rs::WelsIDctFourT4_c` was renamed to its C++ name
`WelsIDctFourT4Rec_c`.

### Phase 4.6 — the inter mode-decision path — **NOT STARTED**

This is what Gate B needs, and it is bigger than the Phase-4 plan recorded. Beyond
the ~770 lines of `svc_base_layer_md.cpp` already listed there, **twelve helpers and
three tables it calls are also unported** — verified by grep, not assumed:

```
InitMe                     (svc_base_layer_md.cpp:964, static inline)
UpdateP16x8MotionInfo      update_P8x16_motion_info   UpdateP8x8MotionInfo
UpdateP4x4MotionInfo       UpdateP8x4MotionInfo       UpdateP4x8MotionInfo
UpdateP16x8Motion2Cache    UpdateP8x16Motion2Cache    UpdateP8x8Motion2Cache
UpdateP4x4Motion2Cache     UpdateP8x4Motion2Cache     UpdateP4x8Motion2Cache
g_kuiSmb4AddrIn256         g_kiPixStrideIdx8x8        g_kiPixStrideIdx4x4
```

Present and reusable (bodies **not** verified against the C++ — see the correction
above): `MeRefineFracPixel`, `InitMeRefinePointer`, `PredSkipMv`, `WelsDctMb`,
`WelsTryPYskip`, `WelsTryPUVskip`, `UpdateP16x16MotionInfo`, `WelsInterMbEncode`,
`WelsPMbChromaEncode`, `WelsMdP16x16`, `WelsMdP8x8`, `WelsMdInterJudgePskip`,
`WelsMdInterDecidedPskip`, `WelsMdInterUpdatePskip`, `WelsRecPskip`,
`WelsMdBackgroundMbEnc`, `PredictSad`, `PredictSadSkip`.

`WelsMdInterMbLoop` (`svc_encode_slice.rs`) also needs three corrections, all
visible by diffing it against `svc_encode_slice.cpp:WelsMdInterMbLoop`:

- it never calls `WelsMdIntraInit` or `WelsMdInterInit` before the re-encoding label;
- it never calls `WelsMdInterSaveSadAndRefMbType` after `pfInterMd`;
- it calls `pfStashMBStatus` unconditionally and does not guard the VLC-overflow
  retry on `!iEntropyCodingModeFlag`, and it omits `WelsInitSliceCabac`.

(The same three were true of `WelsISliceMdEnc` and are fixed there.)

Finally, `PreprocessSliceCoding` still cannot assign `pfInterFineMd` or
`pfFirstIntraMode`, and `pfInterMd` is never assigned at all — C++ sets it per-slice
at `svc_encode_slice.cpp:733/736`. `pfIntraFineMd` **is** now assigned, by
`SetFastCodingFunc`/`SetNormalCodingFunc`.

**Gate B:** `compare.sh` reports a Rust byte count in the same order of magnitude as
the C++ 8034, and `h264dec` decodes all five frames.

### Phase 5 — byte-exactness — **INTRA DONE, blocked on the inter path**

Drive `compare.sh` until the Annex-B streams are identical, then widen beyond the
gate configuration (single spatial layer, single slice, CAVLC, RC off,
single-threaded, deblocking on, `CONSTANT_ID`, no LTR/denoise/AQ/BGD/scene-change
— see `cxx_enc.cpp`).

**Gate 5:** `compare.sh` exits 0.

Current state: **a one-frame encode is byte-identical** (3295 bytes); a five-frame
encode has a **3304-byte common prefix** — the whole IDR access unit plus the first
nine bytes of the first P NAL — against C++ 8034 vs Rust 17907. Everything still
divergent is the unported inter mode decision. **Port Phase 4.6 first; only then is
a byte-level hunt on the P frames meaningful.**

Four things to keep in mind when a stream diverges:

- **Bisect with instrumentation, do not read.** Patch both encoders to dump the same
  per-macroblock state and `diff` it, then narrow: MD state → per-block search →
  `BsGetBitsPos` after each syntax element. Every Phase-4.5 defect fell out of that
  in three steps. Revert the C++ patch with `git checkout codec/` afterwards.

- Size assertions cannot catch **field order**; three structs were correctly
  sized and wrongly ordered. Offsets can be asserted directly —
  `abi_guard.rs` now does this for the fifteen `sWelsEncCtx` fields the
  preprocessor touches.
- `#if`/`#ifdef` around a field is a live hazard — four macros in this codebase
  exclude fields the port had transcribed. See *Conditional compilation is a
  defect class*.
- Before hand-deriving an expected value, **link the C++ function and measure it**.
  This session used it twice more: for the fifteen `offsetof` values above, and to
  establish that upstream's `EncodeFrame` returns 5 for a null-`pData` source
  picture where a Rust test had asserted success.

### Phase 6 — cleanup — **case-insensitive scan DONE, rest NOT STARTED**

Collapse the remaining **82** duplicated constant names (values are now believed
correct, but one definition each is still the goal), fold the module-level
`#![allow(dead_code, unused_variables, …)]` blankets back to the narrowest scope
that still compiles, and reconcile this document with the final state.

> Phase 4.5 corrected four more duplicated constants — all live, all in modules
> other than the one where the name had already been "fixed". The count of 82 is
> not evidence the values agree; it is only a count of names. Re-run the `grep`
> under *Five constants had wrong values* and check each **value** against the
> header before trusting any of them.

**A seventh blind spot, and the most expensive one so far: `find_dup_types.sh` and
every audit built on it match on *names*. A function whose name matches the C++ but
whose body does a fraction of the work passes silently, and three such stubs cost
this session most of its time.** Nothing currently detects them. A crude but
effective check while porting: compare the set of function calls in the Rust body
against the set in the C++ body.

#### `find_dup_types.sh`'s fourth blind spot — case — is **CLOSED**

Phase 4 found `wels_preprocess::SWelsEncCtx` shadowing the canonical
`sWelsEncCtx`. The scan compared identifiers exactly, so two names differing only in
the case of one letter read as unrelated types. A second, case-insensitive pass is
now in the script, self-tested by planting a colliding pair and confirming it is
reported. It immediately found a second live instance: an invented, dead `SWelsPps`
in `svc_set_mb_syn_cabac.rs` whose sole field would have read `SWelsPPS::iSpsId`.

A **fifth** blind spot remains and no identifier comparison can close it: the same
layout **renamed**. `wels_preprocess::SSpatialIndexMap` was a byte-identical copy of
`encoder_context::SSpatialPicIndex`, the name C++ uses. Only reading the header
catches that.

A **sixth**: the scan reads `src/encoder/` only. `src/common/` was already recorded;
`src/decoder/` is not scanned either, and currently declares `EWelsSliceType` twice
(`pic_queue.rs:79` and `slice.rs:46`).

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
