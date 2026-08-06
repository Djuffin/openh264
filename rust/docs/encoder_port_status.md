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

### Phase 2 — unify the encoder-internal types — **IN PROGRESS**

Gate 2 is **not** met. 63 → 40 duplicated type names; 109 redundant declarations
still to remove. This is the next task and it blocks everything downstream.
Run `rust/tools/find_dup_types.sh` for the current list — silence means the gate
is met. See **Session log — Phase 2 continued** below for what has been done and
for the traps found along the way.

`rust/tools/unify_type.py <TypeName> <canonical-module>` automates the mechanical
part: it deletes the struct/enum and its `impl` blocks from every other encoder
module and inserts a `pub use`, printing what it removed so divergent impls get
reviewed rather than silently dropped. Always diff the canonical definition
against the C++ header first — several copies are wrong, not merely truncated.

Done so far, all canonical in `encoder_context.rs`:

| type | copies | notes |
|---|---|---|
| `SMVUnitXY` | 8 → 1 | all identical; kept the `sDeltaMv`/`sAssignMv` impl from `svc_set_mb_syn_cabac.rs`, the only complete one. Call sites in `svc_set_mb_syn_cavlc.rs` passed `&SMVUnitXY`; C++ takes const-ref, ported as by-value |
| `SDCTCoeff` | 4 → 1 | identical |
| `SPicData` | 2 → 1 | identical |
| `SCropOffset` | 2 → 1 | the `encoder_context.rs` copy had `i32` fields; `wels_common_basis.h:105` says `int16_t`. Kept i16 |
| `SMVComponentUnit` | 4 → 1 | three variants. Correct is `sMotionVectorCache[29]` / `iRefIndexCache[30]` (5×6−1 and 5×6). `svc_mode_decision.rs` had `[30]` for the MV cache; `svc_encode_slice.rs` had an invented `sMotionVector[16]`/`iRefIndex[16]` with no C++ counterpart |

#### Next up: the big context types

`SDqLayer` (×11), `SWelsFuncPtrList` (×10), `sWelsEncCtx` (×9), `SSlice` (×8),
`SMB` (×8), `SLayerInfo` (×7), `SMbCache` (×6). These are the remaining blockers
for Phase 4 and they are mutually dependent, so expect to unify them as one
connected batch rather than one at a time.

Two known traps in that batch, both already visible:

- `slice_multi_threading.rs::SSlice` stands in for `sMbCacheInfo` with `[u8; 128]`
  and `sCabacCtx` with `[u8; 64]`, i.e. opaque byte blobs of the wrong size. Any
  copy that does this is not a translation and must be replaced, not merged.
- `svc_set_mb_syn_cavlc.rs` still compares `(*pEncCtx).eSliceType` against
  `EWelsSliceType::I_SLICE as i32` because that module's `sWelsEncCtx` is a local
  stub whose `eSliceType` is `i32`. The cast is a marker: it disappears when
  `sWelsEncCtx` is unified. Do not "fix" it by changing the enum.

Get the C++ sizes with the probe recipe under **Verifying a unified type** below
before merging any of them.

---

## Session log — Phase 2 continued

### Counts

| metric | start of session | now |
|---|---|---|
| duplicated type names | 59 | 40 |
| redundant declarations | 146 | 109 |
| `transmute` in `src/encoder/` | 17 | 17 |

The audit's figures were 58/145; the discrepancy is that
`rust/tools/find_dup_types.sh` also counts non-`pub` declarations. It is the
Gate 2 check: **silence means the gate is met.**

`SBitStringAux` turned out to have **four** identities, not the two the audit
listed — `decoder/bit_stream.rs` and `encoder/vlc_encoder.rs` each declared a
`TagBitStringAux` with a `SBitStringAux` alias, which a struct-only scan misses.
When counting duplicates, remember to look for type aliases too.

### Types unified this session

| type | copies | canonical | note |
|---|---|---|---|
| `SBitStringAux` | 4 → 1 | `common/wels_common_defs.rs` | common-layer type; the decoder's copy already had the exact C++ field order and is re-exported, so decoder paths are unchanged |
| `EWelsNalUnitType`, `EWelsNalRefIdc` | 2 → 1 each | `common/wels_common_defs.rs` | `encoder_context.rs` had 21 of 32 variants and called value 16 `NAL_UNIT_RESV_16` instead of `NAL_UNIT_DEPTH_PARAM` |
| `SNalUnitHeader`, `SNalUnitHeaderExt` | 2 → 1 each | `common/wels_common_defs.rs` | `svc_encode_slice.rs` had `bIdrFlag` as `u8`; C++ says `bool` |
| `SWelsSliceBs` | 3 → 1 | `nal_encap.rs` | `slice_multi_threading.rs` used `[u8; 64]` for the 48-byte `sBsWrite` |
| `SPicture`, `SScreenBlockFeatureStorage` | 6 → 1, 2 → 1 | **new** `encoder/picture.rs` | see below |
| `SRefList` | 3 → 1 | `encoder_context.rs` | `wels_preprocess.rs` dropped the `+1` on both lists and omitted `pNextBuffer`/`pRef` |
| `SWelsSPS`, `SWelsPPS`, `SSubsetSps`, `SSpsSvcExt` | 4/5/3/2 → 1 | `param_svc.rs` | `SWelsPPS` was ~9× too large, see below |
| `SSliceHeader`, `SSliceHeaderExt` | 5 → 1 each | `svc_encode_slice.rs` | |
| `SRefPicMarking`, `SRefPicListReorderSyntax`, `SMmcoRef`, `SReorderingSyntax` | 2 → 1 each | `ref_list_mgr_svc.rs` | `svc_encode_slice.rs` used `[_; 32]` for both arrays; C++ bounds are 4 and 2 |

`encoder/picture.rs` is new and mirrors `codec/encoder/core/inc/picture.h`. The
`SPicture` that had been in `encoder_context.rs` was badly wrong — `pData` and
`iLineSize` as `[_; 4]` where C++ says 3, an invented `iPOC` field, and ten
fields missing. `svc_motion_estimate.rs`'s copy matched C++ field for field and
supplied the canonical body; `ref_list_mgr_svc.rs` had the only complete
`SetUnref` (the `wels_preprocess.rs` one dropped the
`pScreenBlockFeatureStorage` branch).

### The `DISABLE_FMO_FEATURE` class of defect

`codec/encoder/core/inc/as264_common.h:53` defines `DISABLE_FMO_FEATURE`
unconditionally. Every `#if !defined(DISABLE_FMO_FEATURE)` block in the encoder
headers is therefore **not compiled**, and any field inside one does not exist
in the struct the C++ encoder builds. The port had transcribed those fields
anyway:

- `SWelsPPS` carried the nine FMO fields (`uiNumSliceGroups` … `uiSliceGroupId`),
  making it roughly nine times its real size. C++: **16 bytes**, `iPicInitQp` at
  offset 8. The one place that read a FMO field was the PPS writer, which now
  emits the literal `0` that `au_set.cpp:417` writes in that branch.
- `SSliceHeader` carried `iSliceGroupChangeCycle` (`slice.h:124`). C++: **168
  bytes**.

The decoder has its own copies of `iSliceGroupChangeCycle`; those are correct for
the decoder and were left alone. **When porting any remaining encoder struct,
check for `#if !defined(DISABLE_FMO_FEATURE)` first.**

### Verifying a unified type

Compile a `sizeof` dump against the real headers — this is the fastest way to
settle a layout question and it is how both FMO defects were caught:

```bash
c++ -std=c++11 -I codec/encoder/core/inc -I codec/common/inc -I codec/api/wels probe.cpp -o probe && ./probe
```

with `probe.cpp` including the relevant header, `using namespace WelsEnc;` and
`printf("%zu", sizeof(T));`. Then record the number in
`src/encoder/abi_guard.rs`, which is new this session and asserts the size of
every encoder struct unified so far (21 of them) at compile time. It is the
internal counterpart to `api/abi_guard.rs`. Verified to fire by perturbing an
entry.

### Constants are a second duplication axis

The audit named duplicated *types*; duplicated **constants** are just as bad and
more numerous — **79** names are declared in more than one encoder module
(`ENC_RETURN_SUCCESS` ×11, `MAX_DEPENDENCY_LAYER` ×8, `MB_TYPE_SKIP` ×6, …):

```bash
grep -h "^pub const [A-Z_0-9]*:" rust/crates/openh264-rs/src/encoder/*.rs | sed -E 's/.*const ([A-Z_0-9]+):.*/\1/' | sort | uniq -c | awk '$1>1'
```

Three had **wrong values** and are fixed:

| constant | was | C++ | source |
|---|---|---|---|
| `MAX_SHORT_REF_COUNT` | 16, in three modules | `(MAX_GOP_SIZE>>1)` = **4** | `wels_const.h:115` |
| `MAX_TEMPORAL_LEVEL` | 8, in `ref_list_mgr_svc.rs` | `MAX_TEMPORAL_LAYER_NUM` = **4** | `wels_const.h:102` |
| `BLOCK_SIZE_ALL` | 8, in `encoder_context.rs` | **7** | `wels_const.h:147` |

The stale `MAX_SHORT_REF_COUNT` was not cosmetic: `wels_preprocess.rs` sized
`SRefList::pShortRefList` as `[_; MAX_SHORT_REF_COUNT]` without the `+1` C++
has, while the unref loop translated from `wels_preprocess.cpp:1363` indexes
`i+1` up to `MAX_SHORT_REF_COUNT`, i.e. one element past the array.

Two cautions for whoever continues this:

- The **decoder** legitimately defines `MAX_SHORT_REF_COUNT = 16` in its own
  `codec/decoder/core/inc/wels_const.h:47`. Encoder and decoder really do
  disagree here. Do not "unify" them.
- `MAX_DEPENDENCY_LAYER` = 4 and `MAX_QUALITY_LEVEL` = 4 are **correct**. Both
  sit behind `#if defined(NUM_SPATIAL_LAYERS_CONSTRAINT)` /
  `NUM_QUALITY_LAYERS_CONSTRAINT`, and `wels_const.h:41-42` defines both macros
  in the same header a few lines above their use. A grep that excludes
  `wels_const.h` makes them look undefined and the values look wrong.

### Enum variants shadowed by wrong constants

`svc_set_mb_syn_cavlc.rs` declared `I_SLICE: i32 = 0` and `P_SLICE: i32 = 1`,
shadowing the `EWelsSliceType` variants. C++ (`wels_common_defs.h:163`) has
`P_SLICE = 0`, `B_SLICE = 1`, `I_SLICE = 2`. The mb-type offset switch at
`svc_set_mb_syn_cavlc.cpp:76` was therefore selecting the I-slice offset for P
slices and taking the `default: return` for real I slices. Fixed by deleting the
constants and matching on the enum. `svc_encode_slice.rs` had the same pattern
with `NAL_UNIT_CODED_SLICE_EXT: i32 = 20`, also removed. **Grep for `pub const`
names that duplicate an enum variant — this pattern hides real logic bugs.**

### Audit defect 1.5.1 — fixed

`encoder_context.rs::InitBits` omitted `iLeftBits = 32` and `uiCurBits = 0`,
which `golomb_common.h:67` sets, and instead assigned a `uiBufSize` field with no
C++ counterpart. `vlc_encoder.rs::InitBits` was already a faithful port but
nothing called it. The broken copy is deleted and all three call sites
(`InitBitStream` plus two in `wels_encoder_ext.rs`) now use the faithful one.

### Tooling added

| tool | purpose |
|---|---|
| `rust/tools/find_dup_types.sh` | the Gate 2 check; prints one line per type declared in more than one encoder module |
| `rust/tools/show_type.sh <T> [CppTag]` | dumps the C++ original beside every Rust copy, so the canonical is chosen by diffing |
| `src/encoder/abi_guard.rs` | compile-time size assertions against the C++ headers |

`unify_type.py` had a bug: it placed its `pub use` after the last line matching
`^use [^\n]*\n`, which for a multi-line `use crate::{\n …\n};` is the *first*
line of the group, so the insertion landed inside the braces and broke the parse.
It now matches through to the semicolon.

### Test status

`cargo test`: 234 passed, 1 failed, 21 ignored. The single failure is
`loopback_sha1_test::test_decode_encode_full_cycle_sha1_parity`, unchanged — the
encoder still emits zero bytes, which is the Phase 4 gate. The 53 decoder
conformance tests all still pass. `compare.sh` is still C++ 8034 bytes vs Rust 0;
nothing in this session was expected to change that, since Phase 4 is what wires
the pipeline.
